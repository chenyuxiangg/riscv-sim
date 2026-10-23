//! M-13 / M-14 · the **fetch path** — STAGE-10 裁决 A + B.
//!
//! Three things have to hold at once, and this file is where they are
//! pinned:
//!
//! 1. **the width is real** — a compressed instruction that sits in the
//!    **last two bytes** of a mapped region is **executed**, not just
//!    disassembled (ledger **D-86**);
//! 2. **an unmapped fetch is an architectural trap** — `mcause = 1`
//!    (ledger **D-103**), while every **other** bus error is propagated
//!    unchanged;
//! 3. **one fetch is one event** — the bus hook sees **one** entry per
//!    fetched instruction, and its width is the **instruction** width.
//!
//! ⚠️ **The assertion values are architectural literals.**  `mcause == 1`
//! is written as `1`, not as `TrapCause::InstructionAccessFault.mcause()`
//! — an assertion derived from the implementation under test is a
//! **false green** (the implementation could map the variant to the wrong
//! number and the test would happily agree).
//!
//! ⚠️ **`mepc` is asserted against the platform's own semantics**
//! (`mepc := state.pc` at the trap entry — M-13's single trap entry
//! `enter_trap`), **not** against the reference side's value: the two
//! are *suspected* to differ in the unmapped-fetch scenario and that
//! difference is registered, not resolved, in this batch (ledger
//! **D-124**).

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MEPC, CSR_MTVAL, CSR_MTVEC};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::SimState;
use riscv_sim_kernel::m13_iss::{Iss, StepOutcome, TrapCause};
use riscv_sim_kernel::m14_bus::{Bus, BusAccess, BusWidth, PeripheralId};

// ---------------------------------------------------------------------------
// Encodings — taken from the **golden** table in `m13_iss::tests_rv32c`
// (produced by GNU as), not hand-assembled.
// ---------------------------------------------------------------------------

/// `c.li x8, -16` — a 16-bit compressed instruction (low two bits `0b01`).
const C_LI_X8_M16: u16 = 0x5441;
/// `addi x1, x0, 42` — a 32-bit instruction (low two bits `0b11`).
const ADDI_X1_42: u32 = 0x02A0_0093;

/// Read one CSR out of the state's CSR file.
///
/// The contract exposes the CSR file as a public `Vec<(u16, u32)>`; the
/// kernel's own `csr_read` is `pub(crate)` and therefore out of reach
/// from an integration test.
fn csr_value(state: &SimState, csr: u16) -> u32 {
    state
        .csrs
        .iter()
        .find(|(num, _)| *num == csr)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// Collect every bus access the bus reports.
fn record_accesses(bus: &mut Bus) -> Rc<RefCell<Vec<BusAccess>>> {
    let seen: Rc<RefCell<Vec<BusAccess>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    bus.add_hook(Box::new(move |access: &BusAccess| {
        sink.borrow_mut().push(*access);
    }));
    seen
}

/// A RAM region of `size` bytes at base 0 holding `bytes` (zero-padded).
fn ram_with(bytes: &[u8], size: usize) -> Bus {
    let mut image = vec![0u8; size];
    image[..bytes.len()].copy_from_slice(bytes);
    let mut bus = Bus::new();
    bus.map_ram(0, size as u32, &image).expect("map ram");
    bus
}

// ---------------------------------------------------------------------------
// A-1 · the width is real
// ---------------------------------------------------------------------------

/// **A-1** — a compressed instruction in the **last two bytes** of a
/// mapped region is **executed**: the guest runs into it, the PC
/// advances by **two**, and the register value is right.
///
/// ⚠️ This is the case the old four-byte fetch could not serve: the
/// instruction's second half-word is the region's last byte, so reading
/// four bytes ran past the end of the region ⇒ `E_BUS-01` ⇒ the panel
/// could *show* the instruction but the guest could not *run* it
/// (ledger **D-86**).
#[test]
fn a_compressed_instruction_in_the_last_two_bytes_of_a_region_executes() {
    let region_size = 0x100usize;
    let mut image = vec![0u8; region_size];
    image[region_size - 2..].copy_from_slice(&C_LI_X8_M16.to_le_bytes());

    let mut bus = ram_with(&image, region_size);
    let mut state = SimState {
        pc: (region_size - 2) as u32,
        ..SimState::default()
    };
    let mut iss = Iss::default();

    let outcome = iss
        .step_one_via_bus(&mut state, &mut bus)
        .expect("the compressed instruction at the region's end must execute");

    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[8], 0xFFFF_FFF0, "c.li x8, -16 must land in x8");
    assert_eq!(
        state.pc, region_size as u32,
        "a compressed instruction advances the PC by two"
    );
    let stats = bus.stats();
    assert_eq!(stats.reads, 1, "one instruction, one bus read");
    assert_eq!(stats.ram_hits, 1, "one instruction, one landing");
    assert_eq!(stats.misses, 0, "nothing was unmapped");
}

/// **A-1 (negative half)** — a 32-bit instruction whose **upper half**
/// falls off the end of the region is still a fault.  The width probe
/// must not turn "the first two bytes are there" into "the instruction
/// is there".
#[test]
fn a_32_bit_instruction_that_does_not_fit_is_still_a_fault() {
    let region_size = 0x100usize;
    let mut image = vec![0u8; region_size];
    // The last two bytes hold the **low** half of a 32-bit instruction
    // (low two bits `0b11` ⇒ "this is a 32-bit encoding").
    image[region_size - 2..].copy_from_slice(&0x0093u16.to_le_bytes());

    let mut bus = ram_with(&image, region_size);
    let mut state = SimState {
        pc: (region_size - 2) as u32,
        ..SimState::default()
    };
    csr_set_mtvec(&mut state, 0x2000);
    let mut iss = Iss::default();

    let outcome = iss
        .step_one_via_bus(&mut state, &mut bus)
        .expect("an unmapped fetch is a trap, not an error");
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::InstructionAccessFault
        }
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), 1);
}

// ---------------------------------------------------------------------------
// A-2 · 32-bit instructions are unaffected
// ---------------------------------------------------------------------------

/// **A-2** — a 32-bit instruction still advances the PC by **four** and
/// produces the same result as before (regression).
#[test]
fn a_32_bit_instruction_still_advances_the_pc_by_four() {
    let mut bus = ram_with(&ADDI_X1_42.to_le_bytes(), 0x100);
    let mut state = SimState::default();
    let mut iss = Iss::default();

    let outcome = iss.step_one_via_bus(&mut state, &mut bus).expect("step");
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[1], 42, "x1 must hold 42");
    assert_eq!(state.pc, 4, "a 32-bit instruction advances the PC by four");
}

// ---------------------------------------------------------------------------
// A-3 · one fetch is one event
// ---------------------------------------------------------------------------

/// **A-3** — the bus hook sees **one** entry per fetched instruction,
/// and the entry's width is the **instruction** width (`Half` for a
/// compressed instruction, `Word` for a 32-bit one).
///
/// ⚠️ The width probe must not become a second event: two events per
/// instruction would double the bus read count and put a spurious entry
/// into the M-18 trace ring.
#[test]
fn one_fetch_is_one_event_and_the_width_is_the_instruction_width() {
    // `c.li x8, -16` (2 bytes) followed by `addi x1, x0, 42` (4 bytes).
    let mut image = Vec::new();
    image.extend_from_slice(&C_LI_X8_M16.to_le_bytes());
    image.extend_from_slice(&ADDI_X1_42.to_le_bytes());

    let mut bus = ram_with(&image, 0x100);
    let seen = record_accesses(&mut bus);
    let mut state = SimState::default();
    let mut iss = Iss::default();

    iss.step_one_via_bus(&mut state, &mut bus).expect("step 1");
    iss.step_one_via_bus(&mut state, &mut bus).expect("step 2");

    let events = seen.borrow();
    let fetches: Vec<&BusAccess> = events.iter().filter(|e| e.is_fetch).collect();
    assert_eq!(
        fetches.len(),
        2,
        "two instructions ⇒ two fetch events, not three: {events:?}"
    );
    assert_eq!(fetches[0].width, BusWidth::Half, "compressed ⇒ half word");
    assert_eq!(fetches[1].width, BusWidth::Word, "32-bit ⇒ word");
    assert!(
        events.iter().all(|e| e.is_fetch),
        "this program only fetches: {events:?}"
    );
    assert_eq!(
        bus.stats().reads,
        2,
        "one read dispatch per fetched instruction"
    );
}

/// **A-3 (negative half)** — a **failed** fetch is still **one** event,
/// and it reports the failure instead of pretending it succeeded.
#[test]
fn a_failed_fetch_is_still_exactly_one_event() {
    let mut bus = Bus::new(); // nothing mapped at all
    let seen = record_accesses(&mut bus);
    let mut state = SimState {
        pc: 0x9000_0000,
        ..SimState::default()
    };
    let mut iss = Iss::default();

    let _ = iss.step_one_via_bus(&mut state, &mut bus);

    let events = seen.borrow();
    assert_eq!(events.len(), 1, "one failed fetch ⇒ one event: {events:?}");
    assert!(!events[0].ok, "the event must report the failure");
    assert_eq!(
        events[0].err_scenario,
        Some(1),
        "the event must carry the bus scenario code"
    );
}

// ---------------------------------------------------------------------------
// A-4 · an unmapped fetch is an architectural trap
// ---------------------------------------------------------------------------

/// **A-4** — an unmapped fetch traps with `mcause = 1` and
/// `mtval` = the faulting fetch address.
///
/// ⚠️ `mcause` is asserted as the **architectural literal** `1` — not
/// as a value derived from the implementation under test.
#[test]
fn an_unmapped_fetch_traps_with_the_instruction_access_fault() {
    let mut bus = Bus::new(); // nothing mapped at all
    let mut state = SimState {
        pc: 0x9000_0000,
        ..SimState::default()
    };
    csr_set_mtvec(&mut state, 0x2000);
    let mut iss = Iss::default();

    let outcome = iss
        .step_one_via_bus(&mut state, &mut bus)
        .expect("an unmapped fetch is a trap, not an error");

    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::InstructionAccessFault
        }
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), 1, "mcause = 1");
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        0x9000_0000,
        "mtval = the faulting fetch address"
    );
    assert_eq!(state.pc, 0x2000, "the PC goes to the trap vector");
}

/// **A-4 (mepc)** — the platform's **`mepc := state.pc`** semantics on
/// the unmapped-fetch path.
///
/// ⚠️ This asserts the **platform's** semantics (M-13's single trap
/// entry writes `mepc` from the PC that was current when the trap was
/// taken), **not** the reference side's value: the two are *suspected* to
/// differ here and the difference is registered as ledger **D-124** —
/// it is **not** resolved in this batch.
#[test]
fn the_unmapped_fetch_trap_records_the_pc_at_the_trap_entry_in_mepc() {
    let mut bus = Bus::new(); // nothing mapped at all
    let mut state = SimState {
        pc: 0x9000_0000,
        ..SimState::default()
    };
    csr_set_mtvec(&mut state, 0x2000);
    let mut iss = Iss::default();

    iss.step_one_via_bus(&mut state, &mut bus)
        .expect("an unmapped fetch is a trap, not an error");

    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x9000_0000,
        "mepc := state.pc at the trap entry (the platform's own semantics)"
    );
}

/// **A-4 (mepc, negative half)** — the trap must not have moved the PC
/// before it recorded `mepc`: a fetch failure must leave the PC where
/// it was, so the handler comes back to the same place.
#[test]
fn the_unmapped_fetch_trap_does_not_advance_the_pc_before_recording_mepc() {
    let mut bus = Bus::new();
    let mut state = SimState {
        pc: 0x9000_0000,
        ..SimState::default()
    };
    csr_set_mtvec(&mut state, 0x2000);
    let mut iss = Iss::default();

    iss.step_one_via_bus(&mut state, &mut bus)
        .expect("an unmapped fetch is a trap, not an error");

    let mepc = csr_value(&state, CSR_MEPC);
    assert_eq!(
        mepc, 0x9000_0000,
        "the PC at the trap entry is the address that could not be fetched"
    );
    assert_ne!(
        mepc, state.pc,
        "the trap moved the PC to the vector — mepc must not be that"
    );
}

// ---------------------------------------------------------------------------
// A-5 · only "unmapped" becomes a trap
// ---------------------------------------------------------------------------

/// **A-5** — a bus error that is **not** "unmapped" is propagated
/// **unchanged** and is **not** laundered into `mcause = 1`.
///
/// The peripheral window below is mapped but **not wired** ⇒ the bus
/// answers `E_BUS-05` ("peripheral not wired") — a **host-side**
/// problem, not the guest's fault (STAGE-7 adjudication G, STAGE-10
/// adjudication B).
#[test]
fn a_non_unmapped_bus_error_is_propagated_unchanged() {
    let mut bus = Bus::new();
    bus.map_peripheral(0x1000, 0x10, PeripheralId(1))
        .expect("map an unwired peripheral");
    let mut state = SimState {
        pc: 0x1000,
        ..SimState::default()
    };
    csr_set_mtvec(&mut state, 0x2000);
    let mut iss = Iss::default();

    let err: PlatformError = iss
        .step_one_via_bus(&mut state, &mut bus)
        .expect_err("a host-side bus error must not become a guest trap");

    assert_eq!(
        err.family,
        ErrorFamily::Bus,
        "the bus's own family must survive"
    );
    assert_eq!(err.scenario, 5, "E_BUS-05 — the peripheral is not wired");
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        0,
        "no trap was taken ⇒ mcause must stay untouched"
    );
    assert_eq!(
        state.pc, 0x1000,
        "no trap was taken ⇒ the PC must not have moved"
    );
}

// ---------------------------------------------------------------------------
// A-7 · the cause-1 mapping lives in exactly one place
// ---------------------------------------------------------------------------

/// **A-7** — the mapping from a trap cause to its architectural number
/// lives in **exactly one** place: `TrapCause::mcause`.
///
/// The guard is **source level**: the whole kernel source tree is
/// scanned for a `TrapCause::… => <number>` mapping, and the mapping to
/// the number `1` must appear **once**, in `m13_iss/executor.rs`.
///
/// ⚠️ A behavioural assertion alone would not do: an implementation that
/// mapped the variant to `1` **and** to `7` somewhere else would still
/// pass it.  The scan is what makes "one constraint, one home" a
/// checkable fact instead of a comment.
#[test]
fn the_cause_1_mapping_lives_in_exactly_one_place() {
    // Behavioural half: the architectural number itself.
    assert_eq!(
        TrapCause::InstructionAccessFault.mcause(),
        1,
        "the instruction access fault is mcause 1"
    );

    // Source-level half: exactly one mapping to the number 1.
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits: Vec<String> = Vec::new();
    let mut mappings_total = 0usize;
    scan_for_cause_mappings(&src_root, &src_root, &mut hits, &mut mappings_total);

    assert_eq!(
        hits.len(),
        1,
        "exactly one place may map a trap cause to the number 1: {hits:?}"
    );
    assert!(
        hits[0].contains("m13_iss/executor.rs"),
        "the single mapping must live in m13_iss/executor.rs: {hits:?}"
    );

    // And the **whole** variant→code table lives in that one file too.
    assert_eq!(
        mappings_total, 11,
        "the variant→code table must have one row per trap cause and live \
         in one file (11 variants today)"
    );
}

/// Collect every `TrapCause::… => <number>` mapping in the source tree.
///
/// `hits` receives the files that map a cause to the number **1**;
/// `total` counts every mapping found.
fn scan_for_cause_mappings(
    root: &std::path::Path,
    dir: &std::path::Path,
    hits: &mut Vec<String>,
    total: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_for_cause_mappings(root, &path, hits, total);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("TrapCause::") else {
                continue;
            };
            let Some((_variant, after)) = rest.split_once("=>") else {
                continue;
            };
            let number = after.trim().trim_end_matches(',').trim().to_string();
            if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            *total += 1;
            if number == "1" {
                hits.push(format!("{rel}: {line}"));
            }
        }
    }
}

/// Program `mtvec` through the contract's CSR file.
fn csr_set_mtvec(state: &mut SimState, value: u32) {
    match state.csrs.iter_mut().find(|(num, _)| *num == CSR_MTVEC) {
        Some(entry) => entry.1 = value,
        None => state.csrs.push((CSR_MTVEC, value)),
    }
}
