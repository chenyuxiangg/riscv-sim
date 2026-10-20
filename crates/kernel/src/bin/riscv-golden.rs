//! `riscv-golden` — run one RISC-V ELF to its verdict and print it.
//!
//! This is the **candidate** side of the golden comparison (Phase 2).
//! The **reference** side is Spike (`spike --isa=rv32imc_zicclsm`), and
//! `tools/golden-run.sh` drives both and compares.
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
//! Spike's built-in HTIF treats `tohost` as a 64-bit word:
//!
//! ```text
//! tohost = (exit_code << 1) | 1   ->  the hart asks to exit with `exit_code`
//! ```
//!
//! (Spike's `syscall_t::handle_syscall` does exactly
//! `if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }` and
//! `exit_code()` is `exitcode >> 1`.)  Our minimal test environment
//! (`tests/golden/env/riscv_test.h`) writes exactly that encoding, so
//! the verdict is byte-compatible with the upstream riscv-tests env.
//!
//! Exit code `0` means the test passed; any other value is the failing
//! test number.
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
//! `2` = NOVERDICT, `3` = ERROR.
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

/// Smallest RAM window we are willing to map, in bytes.
const MIN_RAM_BYTES: u32 = 0x1_0000;

/// Stack window.  The golden env sets `sp = 0x7fff_f000`; the RV32I
/// corpus never touches the stack, but mapping it keeps the runner
/// usable for programs that do.
const STACK_BASE: u32 = 0x7fff_f000;
const STACK_BYTES: u32 = 0x1000;

/// Default instruction budget.  The RV32I corpus runs in well under a
/// thousand instructions; a million is a generous ceiling that still
/// terminates on a runaway guest.
const DEFAULT_BUDGET: u64 = 1_000_000;

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

fn main() -> ExitCode {
    let mut path: Option<String> = None;
    let mut tohost_override: Option<u32> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut verbose = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tohost" => {
                let raw = args.next().unwrap_or_default();
                let v = u32::from_str_radix(raw.trim_start_matches("0x"), 16)
                    .unwrap_or_else(|_| panic!("bad --tohost value: {raw}"));
                tohost_override = Some(v);
            }
            "--budget" => {
                let raw = args.next().unwrap_or_default();
                budget = raw
                    .parse::<u64>()
                    .unwrap_or_else(|_| panic!("bad --budget value: {raw}"));
            }
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!(
                    "usage: riscv-golden <elf> [--tohost <hex>] [--budget <n>] [-v]\n\
                     \n\
                     Runs the ELF on the M-13 ISS and prints the HTIF verdict."
                );
                return ExitCode::SUCCESS;
            }
            other if other.starts_with("--") => {
                eprintln!("riscv-golden: unknown option {other}");
                return ExitCode::from(3);
            }
            other => {
                if path.is_some() {
                    eprintln!("riscv-golden: unexpected extra argument {other}");
                    return ExitCode::from(3);
                }
                path = Some(other.to_string());
            }
        }
    }

    let Some(path) = path else {
        eprintln!("riscv-golden: missing <elf> argument");
        return ExitCode::from(3);
    };

    let verdict = run(&path, tohost_override, budget, verbose);
    println!("{}", verdict.line());
    ExitCode::from(verdict.exit_code())
}

/// Load and run one image, returning its verdict.
fn run(path: &str, tohost_override: Option<u32>, budget: u64, verbose: bool) -> Verdict {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => return Verdict::Error(format!("read:{e}")),
    };

    let tohost = match tohost_override.or_else(|| find_symbol(&bytes, "tohost")) {
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

    if verbose {
        eprintln!(
            "[golden] entry=0x{:08x} tohost=0x{:08x} segments={}",
            image.entry,
            tohost,
            image.segments.len()
        );
        for (paddr, seg) in &image.segments {
            eprintln!(
                "[golden]   segment 0x{paddr:08x} .. 0x{:08x}",
                paddr + seg.len() as u32
            );
        }
    }

    // --- memory map -------------------------------------------------------
    let mut ram_end = RAM_BASE;
    for (paddr, seg) in &image.segments {
        let end = paddr.wrapping_add(seg.len() as u32);
        if end > ram_end {
            ram_end = end;
        }
    }
    let ram_bytes = ram_end
        .saturating_sub(RAM_BASE)
        .max(MIN_RAM_BYTES)
        .next_multiple_of(0x1000);

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
    gpr[2] = STACK_BASE + STACK_BYTES;
    let mut state = SimState {
        pc: image.entry,
        gpr,
        ..Default::default()
    };

    let mut iss = Iss::new(IssConfig {
        reset_vector: image.entry,
        quantum_budget: budget.min(u32::MAX as u64) as u32,
    });

    let mut steps: u64 = 0;
    loop {
        if steps >= budget {
            if verbose {
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
            if let Some(v) = decode_htif(&mut bus, tohost) {
                return v;
            }
        }

        if let StepOutcome::Trapped { cause } = outcome {
            return Verdict::Error(format!("trap@{:#x}:{:?}", state.pc, cause));
        }
    }
}

/// Decode the 64-bit `tohost` word the way Spike's HTIF does.
///
/// Returns `None` when the word does not request an exit (bit 0 clear),
/// in which case the guest is still running.
fn decode_htif(bus: &mut Bus, tohost: u32) -> Option<Verdict> {
    let low = bus.read(tohost, BusWidth::Word).ok()?;
    let high = bus.read(tohost.wrapping_add(4), BusWidth::Word).ok()?;
    let value = (low as u64) | ((high as u64) << 32);

    // Spike: `if (cmd.payload() & 1) { htif->exitcode = cmd.payload(); }`
    if value & 1 == 0 {
        return None;
    }
    // Spike: `exit_code() { return exitcode >> 1; }`
    let exit_code = value >> 1;
    if exit_code == 0 {
        Some(Verdict::Pass)
    } else {
        Some(Verdict::Fail(exit_code as u32))
    }
}

/// Look up a symbol's value in an ELF32 little-endian file.
///
/// Harness-local minimal reader: it walks the section headers, finds
/// `SHT_SYMTAB`, and resolves one name against the linked string table.
/// It returns `None` for anything it does not understand rather than
/// failing, so a caller can fall back to an explicit `--tohost`.
fn find_symbol(bytes: &[u8], want: &str) -> Option<u32> {
    // ELF32 header: e_shoff@32, e_shentsize@46, e_shnum@48, e_shstrndx@50.
    const SHT_SYMTAB: u32 = 2;
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

        let str_sh = section(str_index)?;
        let str_off = u32::from_le_bytes(str_sh[16..20].try_into().ok()?) as usize;
        let str_size = u32::from_le_bytes(str_sh[20..24].try_into().ok()?) as usize;
        let strtab = bytes.get(str_off..str_off + str_size)?;

        let entsize = if entsize == 0 { 16 } else { entsize };
        let mut p = sym_off;
        let end = sym_off.saturating_add(sym_size);
        while p.saturating_add(16) <= end {
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
