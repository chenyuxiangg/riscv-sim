//! `riscv-golden` — run one RISC-V ELF to its verdict and print it.
//!
//! This is the **candidate** side of the golden comparison (Phase 2).
//! The **reference** side is Spike
//! (`spike --isa=rv32imc_zicsr_zicclsm_zifencei_zicntr`),
//! and `tools/golden-run.sh` drives both and compares.
//!
//! ## What it does
//!
//! 1. Parses the ELF with M-06 and locates the `tohost` symbol (M-11).
//! 2. Maps RAM on an M-14 bus and loads the image segments.
//! 3. Runs the M-13 ISS until the guest writes to `tohost`.
//! 4. Decodes the HTIF verdict and prints it.
//!
//! ## The RAM window —— **the platform description is the single source**
//!
//! The window this harness maps is **not** written down here: it is
//! **asked for** — [`PlatformDescription::ram_window`] picks the RAM
//! segment with the **highest base** out of the **built-in platform
//! description** (compiled in with `include_str!`, so the harness still
//! reads **no** file at run time).
//!
//! ⚠️ **Why that matters** (STAGE-19 §6.2 · 裁决 I): the **reference
//! side** (Spike) needs the *same* window, and until now it had its
//! **own hand-copied** copy of the address ⇒ ⇒ the two could drift
//! silently.  Now the reference side **reads** the window this harness
//! prints (`--print-memory`) ⇒ ⇒ the two agree **by construction**.
//!
//! ### The honest wording about the two memory maps
//!
//! ⚠️ The two sides do **not** expose the **same** address set:
//!
//! | window  | candidate side (this harness) | reference side (Spike) |
//! |---------|-------------------------------|-----------------------|
//! | **RAM** | the description's RAM window  | **the same one** (read from `--print-memory`) |
//! | **stack** | `[0x7fff_e000, 0x7fff_f000)` | `[0x7fff_f000, 0x7fff_f000 + 4096)` |
//!
//! ⇒ ⇒ **the RAM window agrees by construction**; the **stack window is
//! each side's own feature** (Spike puts its initial stack page at
//! `0x7fff_f000` and the reference script keeps that one segment) ⇒ ⇒
//! "the memory map **both** sides expose" would be **inaccurate** ⇒ ⇒
//! do not write that.
//!
//! ## `--print-memory`
//!
//! Prints the RAM window as `base:size` — **decimal** (Spike's
//! multi-region `-m` only accepts decimal) — and exits.  It is a
//! **query**, not a run ⇒ ⇒ it prints **no** `VERDICT` line; the
//! reference side reads that one line and feeds it to Spike.
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
//! M-06 / M-11 / M-13 / M-14 directly and is deliberately kept out of
//! the 23-module design (LLD).  The `tohost` symbol comes from **M-11**
//! (M-06 hands out the `.symtab` / `.strtab` bytes; M-11 parses them)
//! — the harness used to carry its own mini ELF symbol reader, and that
//! second reader is exactly the kind of drift the platform is not
//! supposed to have (**D-90**).

use std::cell::Cell;
use std::fs;
use std::process::ExitCode;
use std::rc::Rc;

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::SimState;
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_kernel::kernel_host::platform::{BUILTIN_JSON_DEFAULT, PLATFORM_ID_DEFAULT};
use riscv_sim_kernel::m06_elf::{DefaultLoader, Kind, LoadImage};
use riscv_sim_kernel::m11_symbol::SymbolTable;
use riscv_sim_kernel::m13_iss::{Iss, IssConfig, StepOutcome};
use riscv_sim_kernel::m14_bus::{AccessDir, Bus, BusWidth, REGION_KIND_RAM};

/// The platform description this harness runs on —— **compiled in**.
///
/// ⚠️ **Zero run-time dependency**: the host's `include_str!` puts the
/// description **inside the binary** ⇒ the harness still reads **no**
/// file at run time (and still runs without a network).
///
/// ⚠️ The JSON comes from the **host's own constant** ⇒ ⇒ the path of
/// the built-in description is written down in **exactly one** place
/// (`kernel_host::platform`) ⇒ ⇒ this harness cannot end up pointing at
/// a **different** file than the host does ⇒ ⇒ no drift.
///
/// ⚠️ This harness still does **not** use the host: it composes M-06 /
/// M-11 / M-13 / M-14 directly and never builds a `KernelHost` ⇒ ⇒ it
/// only *borrows* the description's **text**.
const PLATFORM_JSON: &str = BUILTIN_JSON_DEFAULT;

/// The **id** of that description —— this harness's **one** explicit
/// choice.
///
/// ⚠️ **Chosen by id, never by address** (STAGE-19 §6.2) ⇒ ⇒ "which
/// board do we run on" has **exactly one** landing spot (this constant)
/// ⇒ ⇒ **no** address is written down here.
///
/// ⚠️ **Why `default`**: the golden corpus's images are linked at
/// `0x8000_0000` and span a few kilobytes ⇒ `default`'s high RAM
/// segment (4 MiB) holds them and is the **smallest** window that does
/// ⇒ ⇒ the reference side's `-m` does not claim 128 MiB for nothing
/// (that is `qemu-virt`'s high segment — a real RTOS needs it).
const PLATFORM_ID: &str = PLATFORM_ID_DEFAULT;

/// The base the image is loaded at —— **the RISC-V reset vector**.
///
/// ⚠️ This is a **guard**, **not** the window's definition: the window
/// comes from the **description** ([`PlatformDescription::ram_window`])
/// ⇒ ⇒ but the **base** must be the reset vector ⇒ ⇒ a description
/// that moves the base ⇒ ⇒ **fails loudly** instead of drifting
/// silently (STAGE-19 §6.2).
const REQUIRED_RAM_BASE: u32 = 0x8000_0000;

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
/// ⚠️ **This window is the candidate side's own feature** —— the
/// reference side (Spike) keeps its **initial stack page** at
/// `0x7fff_f000` instead ⇒ ⇒ the two sides' **stack** windows are
/// **not** the same ⇒ ⇒ only the **RAM** window is shared (see the
/// module docs).
///
/// ⚠️ The window deliberately ends **one page below** the image's base.
/// An earlier revision used `STACK_BASE = 0x7fff_f000` and set
/// `sp = STACK_BASE + STACK_BYTES`, which computes to `0x8000_0000` —
/// the **exact address the image is loaded at**. It only worked because
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
// ⚠️ The stack window must not overlap the loaded image, and the initial
// `sp` must sit *inside* the mapped stack window — the RISC-V stack
// grows downwards, so the first push lands at `sp - 4`.  An earlier
// revision mapped the window *above* `sp`, which put the first push
// outside the region (review finding M-5).
//
// ⚠️ The image's base is **not** a constant of this file any more —— it
// comes from the **description** —— but the description's base **must**
// be `REQUIRED_RAM_BASE` (checked at run time, with a loud error) ⇒ ⇒
// this compile-time invariant may use it.
const _: () = {
    assert!(
        STACK_TOP <= REQUIRED_RAM_BASE,
        "the stack window must not overlap the image"
    );
    assert!(STACK_TOP > STACK_BASE, "the stack window must not be empty");
    assert!(
        STACK_TOP - 4 >= STACK_BASE,
        "the first push (sp - 4) must land inside the stack window"
    );
};

/// The platform description this harness runs on —— **parsed + validated**.
///
/// ⚠️ **One parser, one validator**: the description is parsed by
/// `serde` (the contract's own derive) and judged by the contract's
/// [`PlatformDescription::validate`] ⇒ ⇒ this harness does **not** own a
/// second set of description rules.
fn platform_description() -> Result<PlatformDescription, String> {
    let description: PlatformDescription = serde_json::from_str(PLATFORM_JSON)
        .map_err(|err| format!("the built-in platform description is not valid JSON: {err}"))?;
    description
        .validate()
        .map_err(|err| format!("the built-in platform description is invalid: {err}"))?;
    if description.id != PLATFORM_ID {
        return Err(format!(
            "the built-in platform description's id is {:?}, this harness expects {PLATFORM_ID:?}",
            description.id
        ));
    }
    Ok(description)
}

/// The RAM window this harness maps —— **asked for, not written down**.
///
/// ⚠️ **The single source of truth is the platform description**
/// (STAGE-19 裁决 I): "which RAM segment is *the* one" is decided by
/// the contract's [`PlatformDescription::ram_window`] (the segment with
/// the **highest base**) ⇒ ⇒ this harness writes **no** address ⇒ ⇒ the
/// reference side, which **reads** the printed value, cannot drift away
/// from it.
///
/// ⚠️ The **base** is still guarded: it must be the RISC-V reset vector
/// ⇒ ⇒ a description that moves the base **fails loudly** here instead
/// of silently running the image somewhere else.
fn ram_window(description: &PlatformDescription) -> Result<(u32, u64), String> {
    let (base, size) = description
        .ram_window()
        .ok_or_else(|| "the platform description declares no RAM region".to_string())?;
    if base != REQUIRED_RAM_BASE {
        return Err(format!(
            "the RAM window's base must be 0x{REQUIRED_RAM_BASE:08x} (the RISC-V reset \
             vector), the description says 0x{base:08x}"
        ));
    }
    Ok((base, size))
}

/// The window **as the reference side reads it**: `base:size`, **decimal**.
///
/// ⚠️ **Decimal is not a choice**: Spike's multi-region `-m` parser only
/// accepts decimal numbers (its single-region form does accept hex)
/// ⇒ ⇒ the reference side pastes this line **as is** ⇒ ⇒ no base
/// conversion anywhere.
fn format_window(window: (u32, u64)) -> String {
    format!("{}:{}", window.0, window.1)
}

/// **T-5**: the window we **print** must be the window we **actually
/// mapped**.
///
/// ⚠️ The criterion is read from the **bus' own export**
/// ([`Bus::export_map`]) ⇒ ⇒ that is *what is really mapped* ⇒ ⇒ this is
/// **not** the "what I print equals what I print" tautology ⇒ ⇒ whoever
/// replaces the window with a hand-written constant **fails here**
/// (STAGE-19 §6.2 · T-5).
fn assert_mapped_window(bus: &Bus, printed: (u32, u64)) -> Result<(), String> {
    let map = bus.export_map();
    let mapped = map.regions.iter().any(|region| {
        region.kind == REGION_KIND_RAM
            && region.base == printed.0
            && u64::from(region.size) == printed.1
    });
    if mapped {
        return Ok(());
    }
    let actual: Vec<String> = map
        .regions
        .iter()
        .map(|region| format!("0x{:08x}:{}:{}", region.base, region.size, region.kind))
        .collect();
    Err(format!(
        "T-5: the RAM window we print ({}) is not the window we mapped (the bus has {})",
        format_window(printed),
        actual.join(", ")
    ))
}

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
    /// The ELF to run —— **absent** in `--print-memory` mode (that mode
    /// queries the **description**, not an image).
    path: Option<String>,
    /// Print the RAM window and exit (a **query**, not a run).
    print_memory: bool,
    tohost_override: Option<u32>,
    budget: u64,
    verbose: bool,
}

/// Parse the command line.  **Never panics** — every malformed input
/// becomes an error message, and the caller turns that into a
/// `VERDICT ERROR` line plus exit code 3 (review finding H-2).
fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut path: Option<String> = None;
    let mut print_memory = false;
    let mut tohost_override: Option<u32> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut verbose = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--print-memory" => print_memory = true,
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

    // ⚠️ `--print-memory` **does not** need an ELF: the window comes from
    // the **description** ⇒ ⇒ the reference side can ask for it once,
    // before it runs anything.
    let path = if print_memory {
        path
    } else {
        Some(path.ok_or("missing <elf> argument")?)
    };
    Ok(Config {
        path,
        print_memory,
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
            "usage: riscv-golden [--print-memory] <elf> [--tohost <hex>] [--budget <n>] [-v]\n\
             \n\
             Runs the ELF on the M-13 ISS and prints the HTIF verdict.\n\
             \n\
             --print-memory prints the RAM window (base:size, decimal) and exits.\n\
             It is a query, not a run: it prints no VERDICT line."
        );
        return ExitCode::SUCCESS;
    }

    let cfg = match parse_args(&args) {
        Ok(cfg) => cfg,
        Err(msg) => {
            eprintln!("riscv-golden: {msg}");
            println!("{}", Verdict::Error(msg).line());
            return ExitCode::from(3);
        }
    };

    // ⚠️ `--print-memory` is a **query**: it prints the window and exits
    // ⇒ ⇒ **no** VERDICT line ⇒ ⇒ stdout is exactly the one line the
    // reference side pastes into Spike's `-m`.
    if cfg.print_memory {
        return print_memory();
    }

    let verdict = run(&cfg);
    println!("{}", verdict.line());
    ExitCode::from(verdict.exit_code())
}

/// Print the RAM window (`base:size`, decimal) and exit.
///
/// ⚠️ This is what the **reference side** reads: `tools/golden-run.sh`
/// runs it once and feeds the line to Spike's `-m` ⇒ ⇒ the reference
/// side no longer **writes down** an address ⇒ ⇒ the two sides' RAM
/// windows agree **by construction** (STAGE-19 裁决 I).
fn print_memory() -> ExitCode {
    let window = match platform_description().and_then(|description| ram_window(&description)) {
        Ok(window) => window,
        Err(msg) => {
            eprintln!("riscv-golden: {msg}");
            println!("{}", Verdict::Error(msg).line());
            return ExitCode::from(3);
        }
    };
    println!("{}", format_window(window));
    ExitCode::SUCCESS
}

/// Load and run one image, returning its verdict.
fn run(cfg: &Config) -> Verdict {
    // ⚠️ `--print-memory` never reaches here ⇒ ⇒ a path is always present
    // in a run ⇒ ⇒ the guard below is unreachable in practice (it is here
    // because the type says "maybe").
    let Some(path) = cfg.path.as_deref() else {
        return Verdict::Error("missing <elf> argument".to_string());
    };

    // --- the platform description (the RAM window's single source) --------
    //
    // ⚠️ 解析 + 校验都在**契约层**那一份 ⇒ ⇒ 本 harness 不写第二套规则。
    let description = match platform_description() {
        Ok(description) => description,
        Err(msg) => return Verdict::Error(msg),
    };
    // The window we are going to **print** (the reference side reads it)
    // and the window we are going to **map** —— **one** value, **one**
    // source ⇒ ⇒ the two cannot drift apart.
    let window = match ram_window(&description) {
        Ok(window) => window,
        Err(msg) => return Verdict::Error(msg),
    };

    let bytes = match read_capped(path) {
        Ok(b) => b,
        Err(e) => return Verdict::Error(format!("read:{}", e.message)),
    };

    // --- the image -------------------------------------------------------
    //
    // ⚠️ **装载在符号查找之前**：M-11 的符号表来自 M-06 带出来的
    // `.symtab` / `.strtab` 原始字节 ⇒ 没有装载就没有节区。
    let image = match DefaultLoader.load(&bytes, Kind::Elf) {
        Ok(img) => img,
        Err(e) => return Verdict::Error(format!("load:{}", e.message)),
    };

    let tohost = match cfg.tohost_override {
        Some(a) => a,
        None => match lookup_symbol(&image, "tohost") {
            Ok(Some(a)) => a,
            Ok(None) => {
                return Verdict::Error(
                    "no `tohost` symbol in ELF (the golden env must define it)".to_string(),
                )
            }
            Err(e) => return Verdict::Error(format!("symbols:{}", e.message)),
        },
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
    //
    // ⚠️ **The window is the description's RAM segment** —— there is
    // **no** hand-written address left here ⇒ ⇒ the mapped window **is**
    // the one printed to the reference side ⇒ ⇒ the two sides agree **by
    // construction** (STAGE-19 裁决 I).
    //
    // ⚠️ **No widening by the image's span any more**: widening would
    // make the candidate map **more** than the reference side's `-m`
    // ⇒ ⇒ the two drift (and that drift is **invisible** in a green
    // run) ⇒ ⇒ an image that does not fit now **fails loudly** (the
    // `load@0x…` error below) instead of silently widening.
    let map_bytes = match u32::try_from(window.1) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Verdict::Error(format!(
                "the RAM window's size {} does not fit in 32 bits",
                window.1
            ))
        }
    };

    let mut bus = Bus::new();
    if let Err(e) = bus.map_ram(window.0, map_bytes, &[]) {
        return Verdict::Error(format!("map_ram:{}", e.message));
    }
    if let Err(e) = bus.map_ram(STACK_BASE, STACK_BYTES, &[]) {
        return Verdict::Error(format!("map_stack:{}", e.message));
    }

    // --- T-5: the printed window == the window we actually mapped ---------
    //
    // ⚠️ 判据取自**总线自己的导出** ⇒ ⇒ 那是**真正映射了的东西** ⇒ ⇒
    // 谁把窗口换成手写常量，**这里**就红。
    if let Err(msg) = assert_mapped_window(&bus, window) {
        return Verdict::Error(msg);
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

        // ⚠️ **穷举** `StepOutcome` —— 不是 `if let`。
        //
        // `StepOutcome` 今天只有 `Advanced` / `Trapped` 两个变体，可
        // M-17（`m17_backend/mod.rs`）用的是穷举 `match`。用 `if let`
        // 的话，将来若复活第三个变体（v0.19 删过 `HostPause` /
        // `Breakpoint` / `Watchpoint`），这个 runner 会**静默忽略**新
        // 变体并继续跑 ⇒ 判决可能悄悄改变。穷举让编译器逼着表态。
        match outcome {
            StepOutcome::Trapped { cause } => {
                // ⚠️ **陷阱不是错误** —— 它是程序的一部分。
                //
                // ISS 的 `enter_trap` 已经把 `mcause` / `mepc` / `mtval` /
                // `mstatus` 写好，并把 PC 重定向到 `mtvec`（直接模式）或
                // `mtvec + 4 × (cause & 7)`（向量模式）⇒ **陷阱已经投递完毕**。
                //
                // ⇒ 这里**继续跑**，让 guest 的 trap vector 去处理它。
                // （`trap@0x80000004` 那个地址就是 trap vector 本身。）
                //
                // ⚠️ ** runaway 的兜底**：trap vector 自己再陷入 ⇒ 反复投递 ⇒
                // 一直转到 `cfg.budget` 耗尽 ⇒ `Inconclusive`（NOVERDICT）——
                // 这正是「程序发散」应有的判决，不是「平台故障」。
                if cfg.verbose {
                    eprintln!(
                        "[golden] trap at pc=0x{:08x} cause={:?} -> mtvec",
                        state.pc, cause
                    );
                }
            }
            StepOutcome::Advanced => {}
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

/// Look up a symbol's address in the image's **symbol table** (M-11).
///
/// ⚠️ **M-11 是平台里唯一的符号读取者** —— M-06 负责把 `.symtab` /
/// `.strtab` 的**原始字节**带出来，M-11 负责把它们解析成符号表。
/// harness **不再自带**一套迷你 ELF 读取器（D-90）—— 那样平台上会有
/// **两个**符号读取者，**漂移**只是时间问题。
///
/// ⚠️ **畸形符号表 ⇒ `Err`**，不是「安静地查不到」：调用方要能区分
/// 「这个 ELF 里没有这个符号」与「这个 ELF 的符号表坏了」。
fn lookup_symbol(image: &LoadImage, want: &str) -> Result<Option<u32>, PlatformError> {
    let symbols = SymbolTable::from_sections(&image.debug_sections)?;
    // ⚠️ `addr == None` ⇒ **未定义符号**（`SHN_UNDEF`）⇒ 它没有地址
    // ⇒ 与「查不到」同义。
    Ok(symbols.lookup(want).and_then(|symbol| symbol.addr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::platform::RegionSpec;

    /// Build a minimal but valid ELF32 little-endian image with one
    /// symbol table holding a single named symbol.
    ///
    /// ⚠️ **节区必须有名字** —— M-06 的白名单是按**节区名**（`.symtab` /
    /// `.strtab`）筛的，而名字来自 `.shstrtab`。没有 `.shstrtab` ⇒
    /// M-06 带不出任何调试节区 ⇒ M-11 拿到的是**空表**。
    ///
    /// Layout:
    /// ```text
    /// [0   .. 52)  ELF header
    /// [52  .. 212) section headers — 0: NULL, 1: .symtab, 2: .strtab, 3: .shstrtab
    /// [212 .. 244) symbol 0 — the mandatory null symbol
    /// [244 .. 260) symbol 1 — `name`
    /// [260 .. ..)  .strtab, then .shstrtab
    /// ```
    fn synth_elf(name: &str, value: u32) -> Vec<u8> {
        // `.strtab`: a leading NUL (offset 0 = "") then the name.
        let mut strtab = vec![0u8];
        strtab.extend_from_slice(name.as_bytes());
        strtab.push(0);

        // `.shstrtab`: the section names, each NUL-terminated.
        //   0 → ""        (section 0)
        //   1 → ".symtab" (section 1)
        //   9 → ".strtab" (section 2)
        //  17 → ".shstrtab" (section 3)
        let mut shstrtab = vec![0u8];
        for section_name in [".symtab", ".strtab", ".shstrtab"] {
            shstrtab.extend_from_slice(section_name.as_bytes());
            shstrtab.push(0);
        }
        const NAME_SYMTAB: u32 = 1;
        const NAME_STRTAB: u32 = 9;
        const NAME_SHSTRTAB: u32 = 17;

        let shoff = 52usize;
        let symtab_off = shoff + 4 * 40;
        let strtab_off = symtab_off + 32;
        let shstrtab_off = strtab_off + strtab.len();

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
                                                           // ⚠️ `e_phentsize` **必须**是 32（M-06 拒收别的值），
                                                           // `e_phnum = 0` ⇒ 没有程序头 ⇒ 没有可装载段（本夹具只关心符号）。
        out[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
        out[44..46].copy_from_slice(&0u16.to_le_bytes()); // e_phnum
        out[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
        out[48..50].copy_from_slice(&4u16.to_le_bytes()); // e_shnum
        out[50..52].copy_from_slice(&3u16.to_le_bytes()); // e_shstrndx → section 3

        // Section 0: SHT_NULL — the mandatory placeholder.
        out.extend_from_slice(&[0u8; 40]);

        // Section 1: SHT_SYMTAB.
        out.extend_from_slice(&NAME_SYMTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_type = SHT_SYMTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(symtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&32u32.to_le_bytes()); // sh_size (2 entries)
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_link -> strtab
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&4u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&16u32.to_le_bytes()); // sh_entsize

        // Section 2: SHT_STRTAB (`.strtab`).
        out.extend_from_slice(&NAME_STRTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(strtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&(strtab.len() as u32).to_le_bytes()); // sh_size
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize

        // Section 3: SHT_STRTAB (`.shstrtab`) — the section-name table.
        out.extend_from_slice(&NAME_SHSTRTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(shstrtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&(shstrtab.len() as u32).to_le_bytes()); // sh_size
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
        assert_eq!(out.len(), shstrtab_off);
        out.extend_from_slice(&shstrtab);
        out
    }

    /// Build a **loadable** ELF32 image whose entry point is a
    /// self-loop (`jal x0, 0`) — the program never writes `tohost`, so
    /// the runner must exhaust its budget and answer `NOVERDICT`.
    ///
    /// Layout:
    /// ```text
    /// [  0 ..  52) ELF header     (e_phoff = 52, e_shoff = 88)
    /// [ 52 ..  84) program header (PT_LOAD → 0x8000_0000, 4 bytes)
    /// [ 84 ..  88) the code: `jal x0, 0`
    /// [ 88 .. 248) section headers — 0: NULL, 1: .symtab, 2: .strtab, 3: .shstrtab
    /// [248 .. 280) symbols — 0: the null symbol, 1: `tohost`
    /// [280 ..   ) .strtab, then .shstrtab
    /// ```
    fn synth_runaway_elf() -> Vec<u8> {
        const CODE: u32 = 0x0000_006F; // `jal x0, 0` — a self-loop
        const ENTRY: u32 = 0x8000_0000;
        const TOHOST: u32 = 0x8000_1000;

        // `.strtab`: a leading NUL (offset 0 = "") then the name.
        let mut strtab = vec![0u8];
        strtab.extend_from_slice(b"tohost");
        strtab.push(0);

        // `.shstrtab`: the section names, each NUL-terminated.
        let mut shstrtab = vec![0u8];
        for section_name in [".symtab", ".strtab", ".shstrtab"] {
            shstrtab.extend_from_slice(section_name.as_bytes());
            shstrtab.push(0);
        }
        const NAME_SYMTAB: u32 = 1;
        const NAME_STRTAB: u32 = 9;
        const NAME_SHSTRTAB: u32 = 17;

        let shoff = 88usize;
        let symtab_off = shoff + 4 * 40;
        let strtab_off = symtab_off + 32;
        let shstrtab_off = strtab_off + strtab.len();

        let mut out = vec![0u8; 52];
        out[0..4].copy_from_slice(b"\x7fELF");
        out[4] = 1; // ELFCLASS32
        out[5] = 1; // ELFDATA2LSB
        out[6] = 1; // EV_CURRENT
        out[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
        out[18..20].copy_from_slice(&243u16.to_le_bytes()); // e_machine = EM_RISCV
        out[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
        out[24..28].copy_from_slice(&ENTRY.to_le_bytes()); // e_entry
        out[28..32].copy_from_slice(&52u32.to_le_bytes()); // e_phoff
        out[32..36].copy_from_slice(&(shoff as u32).to_le_bytes()); // e_shoff
        out[40..42].copy_from_slice(&52u16.to_le_bytes()); // e_ehsize
        out[42..44].copy_from_slice(&32u16.to_le_bytes()); // e_phentsize
        out[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
        out[46..48].copy_from_slice(&40u16.to_le_bytes()); // e_shentsize
        out[48..50].copy_from_slice(&4u16.to_le_bytes()); // e_shnum
        out[50..52].copy_from_slice(&3u16.to_le_bytes()); // e_shstrndx

        // The program header: one PT_LOAD covering the code.
        out.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
        out.extend_from_slice(&84u32.to_le_bytes()); // p_offset
        out.extend_from_slice(&ENTRY.to_le_bytes()); // p_vaddr
        out.extend_from_slice(&ENTRY.to_le_bytes()); // p_paddr
        out.extend_from_slice(&4u32.to_le_bytes()); // p_filesz
        out.extend_from_slice(&4u32.to_le_bytes()); // p_memsz
        out.extend_from_slice(&5u32.to_le_bytes()); // p_flags = PF_R | PF_X
        out.extend_from_slice(&4u32.to_le_bytes()); // p_align

        // The code: one self-loop.
        out.extend_from_slice(&CODE.to_le_bytes());
        assert_eq!(out.len(), shoff);

        // Section 0: SHT_NULL — the mandatory placeholder.
        out.extend_from_slice(&[0u8; 40]);

        // Section 1: SHT_SYMTAB.
        out.extend_from_slice(&NAME_SYMTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_type = SHT_SYMTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(symtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&32u32.to_le_bytes()); // sh_size (2 entries)
        out.extend_from_slice(&2u32.to_le_bytes()); // sh_link -> strtab
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&4u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&16u32.to_le_bytes()); // sh_entsize

        // Section 2: SHT_STRTAB (`.strtab`).
        out.extend_from_slice(&NAME_STRTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(strtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&(strtab.len() as u32).to_le_bytes()); // sh_size
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize

        // Section 3: SHT_STRTAB (`.shstrtab`) — the section-name table.
        out.extend_from_slice(&NAME_SHSTRTAB.to_le_bytes()); // sh_name
        out.extend_from_slice(&3u32.to_le_bytes()); // sh_type = SHT_STRTAB
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_flags
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        out.extend_from_slice(&(shstrtab_off as u32).to_le_bytes()); // sh_offset
        out.extend_from_slice(&(shstrtab.len() as u32).to_le_bytes()); // sh_size
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        out.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
        out.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize

        assert_eq!(out.len(), symtab_off);
        // Symbol 0 — the mandatory null symbol.
        out.extend_from_slice(&[0u8; 16]);
        // Symbol 1 — `tohost`.
        out.extend_from_slice(&1u32.to_le_bytes()); // st_name (offset in strtab)
        out.extend_from_slice(&TOHOST.to_le_bytes()); // st_value
        out.extend_from_slice(&8u32.to_le_bytes()); // st_size
        out.push(0); // st_info
        out.push(0); // st_other
        out.extend_from_slice(&1u16.to_le_bytes()); // st_shndx

        assert_eq!(out.len(), strtab_off);
        out.extend_from_slice(&strtab);
        assert_eq!(out.len(), shstrtab_off);
        out.extend_from_slice(&shstrtab);
        out
    }

    /// A program that never writes `tohost` must exhaust the budget and
    /// answer `NOVERDICT` — **not** `PASS`, **not** `ERROR`.
    ///
    /// ⚠️ This is the runner's **only** "the program diverged" verdict
    /// (adjudication A's runaway backstop).  Without a case that pins
    /// it the verdict has **no discriminating power**: turning the
    /// budget check into an `ERROR` — or worse, into a `PASS` — would
    /// leave the whole golden gate green while the diverged programs
    /// were being reported as successes.
    #[test]
    fn a_runaway_program_answers_noverdict() {
        let path =
            std::env::temp_dir().join(format!("riscv-golden-runaway-{}.elf", std::process::id()));
        std::fs::write(&path, synth_runaway_elf()).expect("write the runaway image");
        let cfg = Config {
            path: Some(path.to_string_lossy().into_owned()),
            print_memory: false,
            tohost_override: None,
            budget: 32,
            verbose: false,
        };
        let verdict = run(&cfg);
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            verdict,
            Verdict::Inconclusive,
            "a program that never writes tohost must answer NOVERDICT"
        );
    }

    /// Load the image with M-06, then look the symbol up with M-11 —
    /// exactly what [`run`] does.
    fn load_and_lookup(bytes: &[u8], want: &str) -> Result<Option<u32>, PlatformError> {
        let image = DefaultLoader.load(bytes, Kind::Elf)?;
        lookup_symbol(&image, want)
    }

    /// The `tohost` address must come from **M-11's symbol table**.
    #[test]
    fn the_tohost_address_comes_from_the_m11_symbol_table() {
        let elf = synth_elf("tohost", 0x8000_1000);
        assert_eq!(
            load_and_lookup(&elf, "tohost"),
            Ok(Some(0x8000_1000)),
            "M-11 必须读出 .symtab 里的 st_value"
        );
    }

    /// 一个**不存在**的名字查不到（而不是「查到地址 0」）。
    #[test]
    fn an_unknown_symbol_name_is_not_found() {
        let elf = synth_elf("tohost", 0x8000_1000);
        assert_eq!(load_and_lookup(&elf, "fromhost"), Ok(None));
    }

    /// ⚠️ **畸形输入不得 panic** —— 这是仓库的**成文规矩**。
    ///
    /// 现在符号查找走的是 **M-06 + M-11** ⇒ 畸形输入由**它们**挡：
    /// M-06 拒收装载，M-11 拒收畸形符号表。两条路都必须**返回错误**，
    /// 不能 panic，也不能「安静地查到一个地址」。
    #[test]
    fn malformed_input_never_panics_and_never_resolves_a_symbol() {
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
        ];

        for (name, bytes) in cases {
            // Must not panic, and must not resolve a symbol.
            let got = load_and_lookup(&bytes, "tohost");
            assert!(
                matches!(got, Err(_) | Ok(None)),
                "case {name} must not resolve a symbol —— 实得 {got:?}"
            );
        }

        // A section count larger than the real table is **not** a
        // failure: the loader walks the table and finds the genuine
        // sections before running off the end.  The invariant here is
        // only "no panic".
        let mut b = elf.clone();
        b[48..50].copy_from_slice(&0xffffu16.to_le_bytes());
        let _ = load_and_lookup(&b, "tohost");
    }

    /// ⚠️ **节区名丢了 ⇒ 符号表查不到**（而不是「退化到某个地址」）。
    ///
    /// 这是切到 M-11 之后**新出现**的失败模式：M-06 的白名单是按
    /// **节区名**（`.symtab` / `.strtab`）筛的，而名字来自 `.shstrtab`
    /// ⇒ 名字读不出来 ⇒ `.symtab` 带不出来 ⇒ M-11 拿到**空表**。
    ///
    /// ⚠️ 这条用例把「**为什么查不到**」钉死：否则一个「符号表不见了」
    /// 的回归，会以「`tohost` 找不到」的形式出现，而排查方向会指向
    /// 链接脚本而不是节区名。
    #[test]
    fn an_image_without_section_names_has_no_symbol_table() {
        let mut elf = synth_elf("tohost", 0x8000_1000);
        // `e_shstrndx` 越界 ⇒ M-06 读不出任何节区名 ⇒ 白名单没法用 ⇒
        // 它**报告「没有调试节区」**（不是失败）⇒ M-11 是空表。
        elf[50..52].copy_from_slice(&9u16.to_le_bytes());
        assert_eq!(
            load_and_lookup(&elf, "tohost"),
            Ok(None),
            "节区名读不出来 ⇒ M-06 带不出 .symtab ⇒ M-11 是空表"
        );
    }

    /// ⚠️ **节区名表指向一个空节区 ⇒ M-06 拒收**（而不是「安静地放过」）。
    ///
    /// ⚠️ 这与上一条是**不同的**失败模式：`e_shstrndx = 0` 指向的是
    /// `SHT_NULL` 节区（大小为 0）⇒ 名字表**是空的** ⇒ 读任何一个名字
    /// 都越界 ⇒ M-06 报 `E_LDR-01`。
    ///
    /// 两条一起覆盖 ⇒ 「节区名出问题」不会悄悄变成「查到一个错的地址」。
    #[test]
    fn an_image_whose_section_name_table_is_empty_is_rejected() {
        let mut elf = synth_elf("tohost", 0x8000_1000);
        // Point `e_shstrndx` at the NULL section ⇒ its size is 0 ⇒ the
        // section-name table is empty.
        elf[50..52].copy_from_slice(&0u16.to_le_bytes());
        let got = load_and_lookup(&elf, "tohost");
        assert!(
            !matches!(got, Ok(Some(_))),
            "节区名表是空的 ⇒ 绝不能查到一个地址 —— 实得 {got:?}"
        );
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
        assert_eq!(cfg.path.as_deref(), Some("prog.elf"));
        assert_eq!(cfg.tohost_override, Some(0x8000_1000));
        assert_eq!(cfg.budget, 1234);
        assert!(cfg.verbose);
        assert!(!cfg.print_memory);
    }

    #[test]
    fn parse_args_accepts_print_memory_without_an_elf() {
        // ⚠️ `--print-memory` 问的是**描述**，不是镜像 ⇒ ⇒ **不需要** ELF。
        let cfg = parse_args(&["--print-memory".to_string()]).unwrap();
        assert!(cfg.print_memory);
        assert_eq!(cfg.path, None);
    }

    #[test]
    fn the_printed_window_is_the_description_s_ram_window() {
        // ⚠️ T-5 的**前半段**：打印值 == 描述的窗口（**十进制**，`base:size`）。
        let description = platform_description().expect("the built-in description is valid");
        let (base, size) = ram_window(&description).expect("the description declares RAM");
        assert_eq!(base, REQUIRED_RAM_BASE);
        assert_eq!(description.id, PLATFORM_ID);
        let printed = format_window((base, size));
        assert_eq!(printed, format!("{base}:{size}"));
        // ⚠️ **十进制**：Spike 的多段 `-m` 只吃十进制。
        assert!(printed.chars().all(|c| c.is_ascii_digit() || c == ':'));
    }

    #[test]
    fn a_description_whose_base_is_not_the_reset_vector_is_rejected() {
        // ⚠️ 护栏：描述改了 base ⇒ ⇒ **明确报错**，不静默漂移。
        let mut description = platform_description().expect("the built-in description is valid");
        description.regions = vec![RegionSpec::Ram {
            base: 0x1000_0000,
            size: 4096,
        }];
        let err = ram_window(&description).expect_err("the base guard must fire");
        assert!(err.contains("reset vector"), "unexpected error: {err}");
    }

    #[test]
    fn the_window_follows_the_description() {
        // ⚠️ 裁决 J 的第 ① 条（「改描述 ⇒ 打印值**跟着变**」）的**缩微版**：
        //    描述里的 RAM 段改了 ⇒ ⇒ **打印值跟着变** ⇒ ⇒ 窗口**不是**常量
        //    ⇒ ⇒ 参考侧读到的那一行会**自动**跟着描述变 ⇒ ⇒ 两侧不会漂。
        //
        // ⚠️ 真机上那条变异要改**内置描述 JSON**（`kernel_host/platforms/`），
        //    那份文件**不**归本 lane ⇒ ⇒ 这里用同一份描述的**另一组**段
        //    来证明「窗口来自描述」这条机制。
        let mut description = platform_description().expect("the built-in description is valid");
        description.regions = vec![
            RegionSpec::Ram {
                base: 0,
                size: 4096,
            },
            RegionSpec::Ram {
                base: REQUIRED_RAM_BASE,
                size: 8 * 1024 * 1024,
            },
        ];
        let (base, size) = ram_window(&description).expect("the description declares RAM");
        assert_eq!(base, REQUIRED_RAM_BASE);
        assert_eq!(size, 8 * 1024 * 1024);
        assert_eq!(
            format_window((base, size)),
            format!("{base}:{}", 8 * 1024 * 1024)
        );
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
