//! `riscv-golden` — run one RISC-V ELF to its verdict and print it.
//!
//! This is the **candidate** side of the golden comparison (Phase 2).
//! The **reference** side is Spike (`spike --isa=rv32imc_zicclsm_zifencei`),
//! and `tools/golden-run.sh` drives both and compares.
//!
//! ## What it does
//!
//! 1. Parses the ELF with M-06 and locates the `tohost` symbol.
//! 2. Maps RAM on an M-14 bus and loads the image segments.
//! 3. Runs the M-13 ISS until the guest writes to `tohost`.
//! 4. Decodes the HTIF verdict and prints it.
//!
//! ## The HTIF verdict protocol
//!
//! Spike's built-in HTIF treats `tohost` as a 64-bit word, but the
//! **payload field inside it is only 48 bits wide** — `fesvr/device.h`
//! says:
//!
//! ```text
//! uint64_t payload() { return tohost << 16 >> 16; }
//! ```
//!
//! Bits 48..63 of `tohost` are therefore **ignored by Spike**.  What
//! matters is:
//!
//! ```text
//! payload & 1  ->  the hart asks to exit
//! exit_code    =  payload >> 1        (0 == pass, non-zero == fail)
//! ```
//!
//! (Spike's `syscall_t::handle_syscall` does exactly
//! `if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }` and
//! `htif_t::exit_code()` is `exitcode >> 1`.)  Our minimal test
//! environment (`tests/golden/env/riscv_test.h`) writes exactly that
//! encoding, so the verdict is byte-compatible with the upstream
//! riscv-tests env.
//!
//! ⚠️ Decoding the **full** 64-bit word instead of the 48-bit payload
//! is not a cosmetic difference: a non-zero high word would turn a
//! `PASS` into a `FAIL` — and could even produce the impossible verdict
//! `FAIL 0` (which the protocol reserves for "pass").  See the review
//! finding **H-1** in `docs/reviews/REVIEWS.md`.
//!
//! ## Output contract
//!
//! The last line is always `VERDICT <kind> [<detail>]`, where `<kind>`
//! is one of:
//!
//! | kind        | meaning                                              |
//! |-------------|------------------------------------------------------|
//! | `PASS`      | the guest exited with code 0                          |
//! | `FAIL`      | the guest exited with a non-zero code (`<testnum>`)   |
//! | `NOVERDICT` | the guest never wrote `tohost` before the budget ran out |
//! | `ERROR`     | the image could not be loaded or the run faulted      |
//!
//! The process exit code mirrors the verdict: `0` = PASS, `1` = FAIL,
//! `2` = NOVERDICT, `3` = ERROR.  **Every** exit path prints a `VERDICT`
//! line — including a malformed command line — so the shell harness can
//! always parse the outcome.
//!
//! ## Scope note
//!
//! This binary is **harness code**, not a platform module: it composes
//! M-06 / M-13 / M-14 directly and is deliberately kept out of the
//! 23-module design (LLD).  The minimal ELF symbol lookup below is
//! likewise harness-local — when M-11 (symbol table / DWARF) lands, the
//! harness should switch to it rather than keep two symbol readers.

use std::cell::Cell;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::SimState;
use riscv_sim_kernel::m06_elf::{DefaultLoader, Kind};
use riscv_sim_kernel::m13_iss::{Iss, IssConfig, StepOutcome};
use riscv_sim_kernel::m14_bus::{AccessDir, Bus, BusWidth};

/// Base of the RAM window the loaded image lives in.
///
/// The golden environment's linker script places the image at
/// `0x8000_0000` (the RISC-V reset vector), which is also where Spike's
/// default memory starts.
const RAM_BASE: u32 = 0x8000_0000;

/// Size of the RAM window, in bytes.
///
/// This must match the **reference** side's memory map exactly
/// (`spike -m0x80000000:0x400000`).  If the candidate mapped only the
/// image's own span, a program touching an address *outside* the image
/// but *inside* Spike's memory would fault on the candidate while the
/// reference ran fine — a false mismatch.  Both sides therefore expose
/// the same 4 MiB.
const RAM_BYTES: u32 = 0x0040_0000;

/// Stack window.
///
/// The golden env sets `sp = 0x7fff_f000`; the RV32I corpus never
/// touches the stack, but mapping it keeps the runner usable for
/// programs that do.
///
/// The window is `[STACK_BASE, STACK_BASE + STACK_BYTES)` — i.e.
/// `[0x7fff_e000, 0x7fff_f000)` — and the initial `sp` is the **top**
/// of that window (`0x7fff_f000`), so a downward-growing stack lands
/// inside the mapped region.
///
/// ⚠️ The window deliberately ends **one page below** `RAM_BASE`.  An
/// earlier revision used `STACK_BASE = 0x7fff_f000` and set
/// `sp = STACK_BASE + STACK_BYTES`, which computes to `0x8000_0000` —
/// the **exact address the image is loaded at**.  It only worked because
/// the golden env overwrote `sp` before the first push; any program
/// that did not go through that env would have pushed over its own
/// code.
const STACK_BASE: u32 = 0x7fff_e000;
const STACK_BYTES: u32 = 0x1000;

/// Initial stack pointer: the top of the stack window, matching the
/// golden environment's `li sp, 0x7fff_f000`.
const STACK_TOP: u32 = STACK_BASE + STACK_BYTES;

/// Default instruction budget.  The RV32I corpus runs in well under a
/// thousand instructions; a million is a generous ceiling that still
/// terminates on a runaway guest.
const DEFAULT_BUDGET: u64 = 1_000_000;

/// Largest ELF we are willing to read into memory.
///
/// M-06's `ParseLimits` only bite *after* the whole file is in memory, so
/// the runner caps the read itself (review finding L-5).
const MAX_ELF_BYTES: u64 = 64 * 1024 * 1024;

/// The HTIF payload field is **48 bits** wide.
///
/// `fesvr/device.h`: `uint64_t payload() { return tohost << 16 >> 16; }`
/// — bits 48..63 of `tohost` are shifted out and never reach Spike's
/// exit-code logic.  Decoding the full 64-bit word would let a non-zero
/// high word invert a `PASS` into a `FAIL` (review finding H-1).
const HTIF_PAYLOAD_MASK: u64 = 0x0000_ffff_ffff_ffff;

// Compile-time invariants of the memory map.
//
// The stack window must not overlap the loaded image, and the initial
// `sp` must sit *inside* the mapped stack window — the RISC-V stack
// grows downwards, so the first push lands at `sp - 4`.  An earlier
// revision mapped the window *above* `sp`, which put the first push
// outside the region (review finding M-5).
const _: () = {
    assert!(
        STACK_TOP <= RAM_BASE,
        "the stack window must not overlap the image"
    );
    assert!(STACK_TOP > STACK_BASE, "the stack window must not be empty");
    assert!(
        STACK_TOP - 4 >= STACK_BASE,
        "the first push (sp - 4) must land inside the stack window"
    );
};

/// Verdict of one golden run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail(u32),
    /// The guest never produced a verdict (budget exhausted, or the
    /// reference side timed out).  The wire token is `NOVERDICT` — that
    /// spelling is the protocol term used by `tools/golden-run.sh`.
    Inconclusive,
    Error(String),
}

impl Verdict {
    /// Process exit code mirroring the verdict.
    fn exit_code(&self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Fail(_) => 1,
            Self::Inconclusive => 2,
            Self::Error(_) => 3,
        }
    }

    /// The machine-readable single line the shell harness parses.
    fn line(&self) -> String {
        match self {
            Self::Pass => "VERDICT PASS".to_string(),
            Self::Fail(n) => format!("VERDICT FAIL {n}"),
            Self::Inconclusive => "VERDICT NOVERDICT".to_string(),
            Self::Error(msg) => format!("VERDICT ERROR {msg}"),
        }
    }
}

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    path: String,
    tohost_override: Option<u32>,
    budget: u64,
    verbose: bool,
}

/// Parse the command line.  **Never panics** — every malformed input
/// becomes an error message, and the caller turns that into a
/// `VERDICT ERROR` line plus exit code 3 (review finding H-2).
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut path: Option<String> = None;
    let mut tohost_override: Option<u32> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut verbose = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tohost" => {
                let raw = it.next().ok_or("--tohost needs a value")?;
                tohost_override = Some(parse_hex_u32(raw)?);
            }
            "--budget" => {
                let raw = it.next().ok_or("--budget needs a value")?;
                budget = raw
                    .parse::<u64>()
                    .map_err(|_| format!("bad --budget value: {raw}"))?;
            }
            "-v" | "--verbose" => verbose = true,
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option {other}"));
            }
            other => {
                if path.is_some() {
                    return Err(format!("unexpected extra argument {other}"));
                }
                path = Some(other.to_string());
            }
        }
    }

    let path = path.ok_or("missing <elf> argument")?;
    Ok(Config {
        path,
        tohost_override,
        budget,
        verbose,
    })
}

/// Parse a hexadecimal `u32`, with an optional `0x` / `0X` prefix.
///
/// Hex is the format for every numeric address in this harness; the
/// prefix is optional but accepted in either case.
fn parse_hex_u32(raw: &str) -> Result<u32, String> {
    let trimmed = raw.trim();
    let digits = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if digits.is_empty() {
        return Err(format!("bad hexadecimal value: {raw}"));
    }
    u32::from_str_radix(digits, 16).map_err(|_| format!("bad hexadecimal value: {raw}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "usage: riscv-golden <elf> [--tohost <hex>] [--budget <n>] [-v]\n\
             \n\
             Runs the ELF on the M-13 ISS and prints the HTIF verdict."
        );
        return ExitCode::SUCCESS;
    }

    let verdict = match parse_args(&args) {
        Ok(cfg) => run(&cfg),
        Err(msg) => {
            eprintln!("riscv-golden: {msg}");
            Verdict::Error(msg)
        }
    };
    println!("{}", verdict.line());
    ExitCode::from(verdict.exit_code())
}

/// Load and run one image, returning its verdict.
fn run(cfg: &Config) -> Verdict {
    let bytes = match read_capped(&cfg.path) {
        Ok(b) => b,
        Err(e) => return Verdict::Error(format!("read:{}", e.message)),
    };

    let tohost = match cfg
        .tohost_override
        .or_else(|| find_symbol(&bytes, "tohost"))
    {
        Some(a) => a,
        None => {
            return Verdict::Error(
                "no `tohost` symbol in ELF (the golden env must define it)".to_string(),
            )
        }
    };

    let image = match DefaultLoader.load(&bytes, Kind::Elf) {
        Ok(img) => img,
        Err(e) => return Verdict::Error(format!("load:{}", e.message)),
    };

    if cfg.verbose {
        eprintln!(
            "[golden] entry=0x{:08x} tohost=0x{:08x} segments={}",
            image.entry,
            tohost,
            image.segments.len()
        );
        for (paddr, seg) in &image.segments {
            eprintln!(
                "[golden]   segment 0x{paddr:08x} .. 0x{:08x}",
                paddr.wrapping_add(seg.len() as u32)
            );
        }
    }

    // --- memory map -------------------------------------------------------
    // The RAM window is the reference side's window (4 MiB at RAM_BASE),
    // widened if the image itself reaches past it.
    let mut ram_end = RAM_BASE + RAM_BYTES;
    for (paddr, seg) in &image.segments {
        let end = paddr.wrapping_add(seg.len() as u32);
        if end > ram_end {
            ram_end = end;
        }
    }
    let ram_bytes = ram_end
        .wrapping_sub(RAM_BASE)
        .next_multiple_of(0x1000)
        .max(RAM_BYTES);

    let mut bus = Bus::new();
    if let Err(e) = bus.map_ram(RAM_BASE, ram_bytes, &[]) {
        return Verdict::Error(format!("map_ram:{}", e.message));
    }
    if let Err(e) = bus.map_ram(STACK_BASE, STACK_BYTES, &[]) {
        return Verdict::Error(format!("map_stack:{}", e.message));
    }

    for (paddr, seg) in &image.segments {
        for (i, b) in seg.iter().enumerate() {
            let addr = paddr.wrapping_add(i as u32);
            if let Err(e) = bus.write(addr, BusWidth::Byte, *b as u32) {
                return Verdict::Error(format!("load@0x{addr:08x}:{}", e.message));
            }
        }
    }

    // --- tohost write detection ------------------------------------------
    //
    // The bus hook reports the address and direction of every access but
    // not the value, so the hook only raises a flag and the verdict is
    // decoded from memory afterwards.
    let tohost_end = tohost.wrapping_add(8);
    let touched = Rc::new(Cell::new(false));
    {
        let touched = Rc::clone(&touched);
        bus.set_hook(Some(Box::new(move |acc| {
            if acc.ok && acc.dir == AccessDir::Write && acc.addr >= tohost && acc.addr < tohost_end
            {
                touched.set(true);
            }
        })));
    }

    // --- run --------------------------------------------------------------
    let mut gpr = [0u32; 32];
    gpr[2] = STACK_TOP;
    let mut state = SimState {
        pc: image.entry,
        gpr,
        ..Default::default()
    };

    let mut iss = Iss::new(IssConfig {
        reset_vector: image.entry,
        quantum_budget: cfg.budget.min(u32::MAX as u64) as u32,
    });

    let mut steps: u64 = 0;
    loop {
        if steps >= cfg.budget {
            if cfg.verbose {
                eprintln!(
                    "[golden] budget exhausted after {steps} steps, pc=0x{:08x}",
                    state.pc
                );
            }
            return Verdict::Inconclusive;
        }
        steps += 1;

        let outcome = match iss.step_one_via_bus(&mut state, &mut bus) {
            Ok(o) => o,
            Err(e) => return Verdict::Error(format!("step@0x{:08x}:{}", state.pc, e.message)),
        };

        if touched.get() {
            match decode_htif(&mut bus, tohost) {
                Ok(Some(v)) => return v,
                Ok(None) => {}
                Err(e) => {
                    return Verdict::Error(format!("read tohost@0x{tohost:08x}:{}", e.message))
                }
            }
        }

        if let StepOutcome::Trapped { cause } = outcome {
            return Verdict::Error(format!("trap@{:#x}:{:?}", state.pc, cause));
        }
    }
}

/// Read a file, refusing anything larger than [`MAX_ELF_BYTES`].
fn read_capped(path: &str) -> Result<Vec<u8>, PlatformError> {
    let meta = fs::metadata(path).map_err(|e| {
        PlatformError::new(
            riscv_sim_contract::error::ErrorFamily::Loader,
            1,
            e.to_string(),
        )
    })?;
    if meta.len() > MAX_ELF_BYTES {
        return Err(PlatformError::new(
            riscv_sim_contract::error::ErrorFamily::Loader,
            1,
            format!(
                "file too large: {} bytes (limit {MAX_ELF_BYTES})",
                meta.len()
            ),
        ));
    }
    fs::read(path).map_err(|e| {
        PlatformError::new(
            riscv_sim_contract::error::ErrorFamily::Loader,
            1,
            e.to_string(),
        )
    })
}

/// Decode the `tohost` word the way Spike's HTIF does.
///
/// Returns `Ok(None)` when the word does not request an exit (bit 0
/// clear), in which case the guest is still running.  Spike's own
/// behaviour in that branch is to dispatch a **proxy syscall** by
/// reading a request structure out of guest memory; this runner does
/// not implement that branch (it is not needed by the corpus, whose
/// environment always sets bit 0), so the run simply continues until the
/// budget runs out.
fn decode_htif(bus: &mut Bus, tohost: u32) -> Result<Option<Verdict>, PlatformError> {
    let low = bus.read(tohost, BusWidth::Word)?;
    let high = bus.read(tohost.wrapping_add(4), BusWidth::Word)?;

    // Only the low 48 bits carry the payload — see the module docs.
    let payload = ((low as u64) | ((high as u64) << 32)) & HTIF_PAYLOAD_MASK;

    // Spike: `if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }`
    if payload & 1 == 0 {
        return Ok(None);
    }
    // Spike: `exit_code() { return exitcode.value_or(0) >> 1; }`, and the
    // result is an `int` — so the exit code is the payload shifted right
    // by one and truncated to 32 bits.
    let exit_code = ((payload >> 1) & 0xffff_ffff) as u32;
    if exit_code == 0 {
        Ok(Some(Verdict::Pass))
    } else {
        Ok(Some(Verdict::Fail(exit_code)))
    }
}

/// Look up a symbol's value in an ELF32 little-endian file.
///
/// Harness-local minimal reader: it walks the section headers, finds
/// `SHT_SYMTAB`, and resolves one name against the linked string table.
/// It returns `None` for anything it does not understand rather than
/// failing, so a caller can fall back to an explicit `--tohost`.
///
/// Every index is bounds-checked, so a malformed or truncated file
/// yields `None` instead of a panic (review finding M-1 / the repo's
/// standing "malformed input must not panic" rule).
fn find_symbol(bytes: &[u8], want: &str) -> Option<u32> {
    // ELF32 header: e_shoff@32, e_shentsize@46, e_shnum@48, e_shstrndx@50.
    const SHT_SYMTAB: u32 = 2;
    /// Smallest possible ELF32 symbol entry (st_name..st_shndx).
    const MIN_SYM_ENT_SIZE: u32 = 16;

    if bytes.len() < 52 || &bytes[0..4] != b"\x7fELF" {
        return None;
    }
    if bytes[4] != 1 || bytes[5] != 1 {
        // 64-bit or big-endian: not supported by this harness reader.
        return None;
    }

    let rd_u16 = |off: usize| -> Option<u16> {
        bytes
            .get(off..off + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
    };
    let rd_u32 = |off: usize| -> Option<u32> {
        bytes
            .get(off..off + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };

    let shoff = rd_u32(32)? as usize;
    let shentsize = rd_u16(46)? as usize;
    let shnum = rd_u16(48)? as usize;
    if shentsize < 40 || shoff == 0 {
        return None;
    }

    let section = |index: usize| -> Option<&[u8]> {
        let off = shoff.checked_add(index.checked_mul(shentsize)?)?;
        bytes.get(off..off + 40)
    };

    for i in 0..shnum {
        let sh = section(i)?;
        let sh_type = u32::from_le_bytes(sh[4..8].try_into().ok()?);
        if sh_type != SHT_SYMTAB {
            continue;
        }
        let sym_off = u32::from_le_bytes(sh[16..20].try_into().ok()?) as usize;
        let sym_size = u32::from_le_bytes(sh[20..24].try_into().ok()?) as usize;
        let str_index = u32::from_le_bytes(sh[24..28].try_into().ok()?) as usize;
        let entsize = u32::from_le_bytes(sh[36..40].try_into().ok()?) as usize;

        // A symbol entry must be at least `MIN_SYM_ENT_SIZE` bytes, or
        // the walk below would read overlapping fields (review finding
        // L-7).
        if entsize < MIN_SYM_ENT_SIZE as usize {
            return None;
        }

        let str_sh = section(str_index)?;
        let str_off = u32::from_le_bytes(str_sh[16..20].try_into().ok()?) as usize;
        let str_size = u32::from_le_bytes(str_sh[20..24].try_into().ok()?) as usize;
        let strtab = bytes.get(str_off..str_off + str_size)?;

        let mut p = sym_off;
        let end = sym_off.saturating_add(sym_size);
        while p.saturating_add(MIN_SYM_ENT_SIZE as usize) <= end {
            // ELF32 symbol layout: st_name@0, st_value@4, st_size@8,
            // st_info@12, st_other@13, st_shndx@14.
            let name_off = rd_u32(p)? as usize;
            let value = rd_u32(p + 4)?;
            if let Some(name) = strtab.get(name_off..) {
                let name = name.split(|b| *b == 0).next().unwrap_or(&[]);
                if name == want.as_bytes() {
                    return Some(value);
                }
            }
            p = p.saturating_add(entsize);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but valid ELF32 little-endian image with one
    /// symbol table holding a single named symbol.
    ///
    /// Layout:
    /// ```text
    /// [0   .. 52)  ELF header
    /// [52  .. 92)  section 0 — SHT_NULL
    /// [92  .. 132) section 1 — SHT_SYMTAB
    /// [132 .. 172) section 2 — SHT_STRTAB
    /// [172 .. 204) symbol 0 — the mandatory null symbol
    /// [204 .. 220) symbol 1 — `name`
    /// [220 .. ..)  string table
    /// ```
    fn synth_elf(name: &str, value: u32) -> Vec<u8> {
        let mut strtab = vec![0u8];
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);

        let symtab_off = 172usize;
        let strtab_off = symtab_off + 32;
        let shoff = 52usize;

        let mut out = vec![0u8; 52];
        out[0..4].copy_from_slice(b"\x7fELF");
        out[4] = 1; // ELFCLASS32
        out[5] = 1; // ELFDATA2LSB
        out[6] = 1; // EV_CURRENT
        out[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
        out[18..20].copy_from_slice(&243u16.to_le_bytes()); // e_machine = EM_RISCV
        out[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
        out[24..28].copy_from_slice(&0x8000_0000u32.to_le_bytes()); // e_entry
        out[32..36].copy_from_slice(&(shoff as u32).to_le_bytes());
        out[40..42].copy_from_slice(&16u16.to_le_bytes()); // e_ehsize
        out[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
        out[48..50].copy_from_slice(&3u16.to_le_bytes()); // e_shnum
        out[50..52].copy_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        // Section 0: SHT_NULL — the mandatory placeholder.  Without it
        // the symtab would land at index 0 and `sh_link = 2` would point
        // past the end of the section table.
        out.extend_from_slice(&[0u8; 40]);

        // Section 1: SHT_SYMTAB.
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_name
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_type = SHT_SYMTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(symtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&32u32.to_le_bytes()); // sh_size (2 entries)
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_link -> strtab
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&4u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&16u32.to_le_bytes()); // sh_entsize

        // Section 2: SHT_STRTAB.
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_name
        out.extend_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(strtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&(strtab.len() as u32).to_le_bytes()); // sh_size
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize

        assert_eq!(out.len(), symtab_off);
        // Symbol 0 — the mandatory null symbol.
        out.extend_from_slice(&[0u8; 16]);
        // Symbol 1 — our named symbol.
        out.extend_from_slice(&1u32.to_le_bytes()); // st_name (offset in strtab)
        out.extend_from_slice(&value.to_le_bytes()); // st_value
        out.extend_from_slice(&8u32.to_le_bytes()); // st_size
        out.push(0); // st_info
        out.push(0); // st_other
        out.extend_from_slice(&1u16.to_le_bytes()); // st_shndx

        assert_eq!(out.len(), strtab_off);
        out.extend_from_slice(&strtab);
        out
    }

    #[test]
    fn find_symbol_reads_the_st_value_field() {
        let elf = synth_elf("tohost", 0x8000_1000);
        assert_eq!(find_symbol(&elf, "tohost"), Some(0x8000_1000));
    }

    #[test]
    fn find_symbol_returns_none_for_an_unknown_name() {
        let elf = synth_elf("tohost", 0x8000_1000);
        assert_eq!(find_symbol(&elf, "fromhost"), None);
    }

    #[test]
    fn find_symbol_survives_malformed_input() {
        // Every one of these must return None rather than panic: the
        // repo's standing rule is that malformed input never panics.
        let elf = synth_elf("tohost", 0x8000_1000);

        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("too short", vec![0u8; 51]),
            ("bad magic", {
                let mut b = elf.clone();
                b[1] = b'X';
                b
            }),
            ("64-bit class", {
                let mut b = elf.clone();
                b[4] = 2;
                b
            }),
            ("big endian", {
                let mut b = elf.clone();
                b[5] = 2;
                b
            }),
            ("absurd e_shoff", {
                let mut b = elf.clone();
                b[32..36].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
                b
            }),
            ("zero e_shentsize", {
                let mut b = elf.clone();
                b[46..48].copy_from_slice(&0u16.to_le_bytes());
                b
            }),
            ("truncated body", elf[..100].to_vec()),
            ("truncated symbol table", elf[..180].to_vec()),
            ("sh_entsize too small", {
                let mut b = elf.clone();
                // Section 1's sh_entsize lives at
                // e_shoff + 1 * e_shentsize + 36.
                let at = 52 + 40 + 36;
                b[at..at + 4].copy_from_slice(&8u32.to_le_bytes());
                b
            }),
        ];

        for (name, bytes) in cases {
            // Must not panic, and must not resolve a symbol.
            let got = find_symbol(&bytes, "tohost");
            assert_eq!(got, None, "case {name} should not resolve a symbol");
        }

        // A section count larger than the real table is **not** a
        // failure: the reader walks the table and finds the genuine
        // symtab before running off the end.  The invariant here is only
        // "no panic".
        let mut b = elf.clone();
        b[48..50].copy_from_slice(&0xffffu16.to_le_bytes());
        let _ = find_symbol(&b, "tohost");
    }

    /// Build a bus with a RAM window and write `value` at `addr`.
    fn bus_with_word(addr: u32, value: u32) -> Bus {
        let mut bus = Bus::new();
        bus.map_ram(0x8000_0000, 0x1_0000, &[]).unwrap();
        bus.write(addr, BusWidth::Word, value).unwrap();
        bus
    }

    #[test]
    fn decode_htif_reads_pass_and_fail_like_spike() {
        // Spike: payload & 1 -> exit; exit code = payload >> 1.
        let tohost = 0x8000_1000u32;

        // tohost = 1  ->  exit code 0  ->  PASS
        let mut bus = bus_with_word(tohost, 1);
        assert_eq!(decode_htif(&mut bus, tohost).unwrap(), Some(Verdict::Pass));

        // tohost = (3 << 1) | 1 = 7  ->  exit code 3  ->  FAIL 3
        let mut bus = bus_with_word(tohost, 7);
        assert_eq!(
            decode_htif(&mut bus, tohost).unwrap(),
            Some(Verdict::Fail(3))
        );

        // tohost = (1 << 1) | 1 = 3  ->  exit code 1  ->  FAIL 1
        let mut bus = bus_with_word(tohost, 3);
        assert_eq!(
            decode_htif(&mut bus, tohost).unwrap(),
            Some(Verdict::Fail(1))
        );
    }

    /// Build a bus whose `tohost` word holds the given low and high halves.
    fn bus_with_tohost(low: u32, high: u32) -> Bus {
        let tohost = 0x8000_1000u32;
        let mut bus = Bus::new();
        bus.map_ram(0x8000_0000, 0x1_0000, &[]).unwrap();
        bus.write(tohost, BusWidth::Word, low).unwrap();
        bus.write(tohost + 4, BusWidth::Word, high).unwrap();
        bus
    }

    #[test]
    fn decode_htif_masks_bits_above_48() {
        // Spike's payload field is only 48 bits wide
        // (`tohost << 16 >> 16`): bits 48..63 of `tohost` are dropped.
        // A high word in that dead zone must not change the verdict.
        let tohost = 0x8000_1000u32;

        // High word = 0x0001_0000 -> bit 48, i.e. *outside* the
        // 48-bit payload.  Spike sees payload == 7 -> exit code 3.
        let mut bus = bus_with_tohost(7, 0x0001_0000);
        assert_eq!(
            decode_htif(&mut bus, tohost).unwrap(),
            Some(Verdict::Fail(3))
        );
    }

    #[test]
    fn decode_htif_truncates_the_exit_code_before_the_zero_check() {
        // The exact regression of review finding H-1: `tohost` =
        // 0x0000_0002_0000_0001.
        //
        // Spike: payload = 0x0000_0002_0000_0001, `exitcode` =
        // payload, and `exit_code()` is `exitcode >> 1` **as an int** —
        // so the shift happens in 64 bits and the result is then
        // truncated to 32 bits: (1 << 32) -> 0 -> **PASS**.
        //
        // The pre-review runner shifted in 64 bits, checked the zero
        // *before* truncating, and reported the impossible `FAIL 0`.
        let tohost = 0x8000_1000u32;
        let mut bus = bus_with_tohost(1, 0x0000_0002);
        assert_eq!(decode_htif(&mut bus, tohost).unwrap(), Some(Verdict::Pass));
    }

    #[test]
    fn decode_htif_counts_the_high_word_inside_the_48_bit_payload() {
        // High word = 0x0000_0002 -> bit 33, i.e. *inside* the 48-bit
        // payload, so it counts: payload = 0x0000_0002_0000_0007,
        // exit code = (payload >> 1) truncated to 32 bits = 3.
        let tohost = 0x8000_1000u32;
        let mut bus = bus_with_tohost(7, 0x0000_0002);
        assert_eq!(
            decode_htif(&mut bus, tohost).unwrap(),
            Some(Verdict::Fail(3))
        );
    }

    #[test]
    fn decode_htif_ignores_a_word_without_the_exit_bit() {
        let tohost = 0x8000_1000u32;
        let mut bus = bus_with_word(tohost, 2);
        assert_eq!(decode_htif(&mut bus, tohost).unwrap(), None);
    }

    #[test]
    fn parse_args_accepts_the_documented_forms() {
        let args = vec![
            "prog.elf".to_string(),
            "--tohost".to_string(),
            "0x80001000".to_string(),
            "--budget".to_string(),
            "1234".to_string(),
            "-v".to_string(),
        ];
        let cfg = parse_args(&args).unwrap();
        assert_eq!(cfg.path, "prog.elf");
        assert_eq!(cfg.tohost_override, Some(0x8000_1000));
        assert_eq!(cfg.budget, 1234);
        assert!(cfg.verbose);
    }

    #[test]
    fn parse_args_rejects_bad_input_without_panicking() {
        let bad: Vec<Vec<String>> = vec![
            vec![],
            vec!["a.elf".to_string(), "b.elf".to_string()],
            vec!["a.elf".to_string(), "--tohost".to_string()],
            vec![
                "a.elf".to_string(),
                "--tohost".to_string(),
                "zzz".to_string(),
            ],
            vec!["a.elf".to_string(), "--budget".to_string()],
            vec![
                "a.elf".to_string(),
                "--budget".to_string(),
                "abc".to_string(),
            ],
            vec!["a.elf".to_string(), "--bogus".to_string()],
        ];
        for args in bad {
            assert!(
                parse_args(&args).is_err(),
                "args {args:?} should be rejected"
            );
        }
    }

    #[test]
    fn parse_hex_accepts_both_prefix_spellings() {
        assert_eq!(parse_hex_u32("0x80001000"), Ok(0x8000_1000));
        assert_eq!(parse_hex_u32("0X80001000"), Ok(0x8000_1000));
        assert_eq!(parse_hex_u32("80001000"), Ok(0x8000_1000));
        assert!(parse_hex_u32("").is_err());
        assert!(parse_hex_u32("0x").is_err());
        assert!(parse_hex_u32("nothex").is_err());
    }
}
