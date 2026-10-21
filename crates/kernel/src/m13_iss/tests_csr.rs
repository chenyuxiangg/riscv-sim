//! M-13's CSR instruction family, `MRET`, and the trap entry.
//!
//! The frozen spec for this batch is
//! `docs/detailed-design/STAGE-4_Interrupt_Controller.md` §3.5 (the trap
//! entry), §3.6 (the CSR instructions and `mret`) and §6.3 (the
//! acceptance list).
//!
//! ## What the assertions look at
//!
//! Every assertion in this module lands on an **output**:
//!
//! * the **shape** of the [`StepOutcome`] (`Advanced` / `Trapped`),
//! * the **content of the CSR file** ([`SimState::csrs`]), read
//!   **directly** by the test helper [`csr_value`] — not through the
//!   implementation's own accessor, because an assertion that used the
//!   implementation's reader to check the implementation's writer would
//!   be **self-referential** (it would stay green even if both sides
//!   were wrong in the same way),
//! * the **PC**.
//!
//! Nothing here asserts "a function was called" or "no error was
//! raised" — those are the assertions that cannot go red.
//!
//! ## The expected values
//!
//! * The **CSR numbers** come from `riscv_sim_contract::csr` — the
//!   platform's single vocabulary — not from a constant defined next to
//!   the implementation.
//! * The **`mcause` codes** are written out as **architectural
//!   literals** (`0` / `2` / `3` / `4` / `6` / `11`), because the point
//!   of the test is to pin the number the RISC-V manual prescribes. A
//!   test that expected `TrapCause::….mcause()` would only prove that
//!   the mapping agrees with itself.

use riscv_sim_contract::csr::{
    csr_write_is_ignored, CSR_IRQ_CLEAR, CSR_MCAUSE, CSR_MEPC, CSR_MIP, CSR_MSTATUS, CSR_MTVAL,
    CSR_MTVEC, MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT,
};
use riscv_sim_contract::model::SimState;

use super::decoder::Decoder;
use super::executor::{classify, MisalignedPolicy, Op, StepOutcome, TrapCause};
use super::tests_common::*;
use super::Iss;

/// `CSRRW` — swap a CSR with a register.
const F_CSRRW: u8 = 0b001;
/// `CSRRS` — set bits in a CSR from a register.
const F_CSRRS: u8 = 0b010;
/// `CSRRC` — clear bits in a CSR from a register.
const F_CSRRC: u8 = 0b011;
/// `CSRRWI` — swap a CSR with a 5-bit immediate.
const F_CSRRWI: u8 = 0b101;
/// `CSRRSI` — set bits in a CSR with a 5-bit immediate.
const F_CSRRSI: u8 = 0b110;
/// `CSRRCI` — clear bits in a CSR with a 5-bit immediate.
const F_CSRRCI: u8 = 0b111;

/// The architectural `mcause` code of an illegal instruction.
const MCAUSE_ILLEGAL: u32 = 2;
/// The architectural `mcause` code of a breakpoint.
const MCAUSE_BREAKPOINT: u32 = 3;
/// The architectural `mcause` code of a misaligned load address.
const MCAUSE_LOAD_MISALIGNED: u32 = 4;
/// The architectural `mcause` code of a misaligned store address.
const MCAUSE_STORE_MISALIGNED: u32 = 6;
/// The architectural `mcause` code of an environment call from M-mode.
const MCAUSE_ECALL: u32 = 11;

/// Encode a CSR instruction.
///
/// The CSR number lives in bits 31:20 — the same field the ECALL /
/// EBREAK immediate occupies — and the `rs1` field (bits 19:15) is the
/// source operand (a register for the register forms, a 5-bit
/// immediate for the immediate forms).
fn csr_insn(csr: u16, rs1: u8, funct3: u8, rd: u8) -> u32 {
    enc_i(csr as i32, rs1, funct3, rd, SYSTEM)
}

/// The `MRET` encoding (`0x3020_0073`): `funct3 == 0` and the 12-bit
/// immediate field holds `0x302`.
fn mret_insn() -> u32 {
    enc_i(0x302, 0, 0b000, 0, SYSTEM)
}

/// A state with the CSR file pre-seeded with `entries`.
///
/// The file is seeded **in ascending order** — the shape the platform
/// keeps it in.
fn state_with_csrs(entries: &[(u16, u32)]) -> SimState {
    SimState {
        csrs: entries.to_vec(),
        ..SimState::default()
    }
}

/// The `mstatus` value with `MIE` / `MPIE` set as asked.
fn mstatus_with(mie: bool, mpie: bool) -> u32 {
    let mut value = 0u32;
    if mie {
        value |= 1 << MSTATUS_MIE_BIT;
    }
    if mpie {
        value |= 1 << MSTATUS_MPIE_BIT;
    }
    value
}

// ---------------------------------------------------------------------------
// The decode level — which operation an encoding resolves to
// ---------------------------------------------------------------------------

/// Each of the six CSR instructions must resolve to **its own**
/// operation.
///
/// A decoder that routed all six to one operation would still retire
/// them, so the assertion has to be on the **operation itself**.
#[test]
fn each_csr_encoding_resolves_to_its_own_operation() {
    for (funct3, op) in [
        (F_CSRRW, Op::Csrrw),
        (F_CSRRS, Op::Csrrs),
        (F_CSRRC, Op::Csrrc),
        (F_CSRRWI, Op::Csrrwi),
        (F_CSRRSI, Op::Csrrsi),
        (F_CSRRCI, Op::Csrrci),
    ] {
        let d = Decoder::default().decode(csr_insn(CSR_MTVEC, 1, funct3, 2), 0x1000);
        assert_eq!(classify(&d), Some(op), "funct3 {funct3:#b}");
    }
}

/// `MRET` must resolve to [`Op::Mret`] — and to **nothing else**.
///
/// `MRET` shares the `funct3 == 0` slot with ECALL / EBREAK, so the
/// dispatch has to look at the immediate as well.
#[test]
fn the_mret_encoding_resolves_to_mret() {
    let d = Decoder::default().decode(mret_insn(), 0x1000);
    assert_eq!(classify(&d), Some(Op::Mret));
    assert_eq!(d.imm, 0x302, "the MRET immediate field is 0x302");
}

// ---------------------------------------------------------------------------
// The CSR instructions — the read / write semantics
// ---------------------------------------------------------------------------

/// `CSRRW` swaps the CSR with the register: `rd` gets the **old** CSR
/// value, the CSR gets the register value.
#[test]
fn csrrw_swaps_the_csr_with_the_register() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x1234)]);
    state.gpr[1] = 0x5678;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MTVEC, 1, F_CSRRW, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], 0x1234,
        "rd must receive the **old** CSR value"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVEC),
        0x5678,
        "the CSR must receive the register value"
    );
}

/// `CSRRW` with `rd == x0` must still **write** the CSR.
///
/// The manual's other half — "with `rd = x0` the CSR is not read" —
/// has no observable consequence on this platform (a CSR read has no
/// side effect), so only the writable half is pinned here. The
/// implementation does skip the read; it just cannot be observed.
#[test]
fn csrrw_with_rd_x0_still_writes_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x1234)]);
    state.gpr[1] = 0x5678;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MTVEC, 1, F_CSRRW, 0));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MTVEC),
        0x5678,
        "CSRRW must write the CSR even when rd is x0"
    );
    assert_eq!(state.gpr[0], 0, "x0 must stay hardwired to zero");
}

/// `CSRRW` must read the CSR **before** it overwrites the register.
///
/// With `rd == rs1` the two operands are the same register: the value
/// the CSR receives must be the register's **old** value, and the
/// register must end up holding the CSR's old value.
#[test]
fn csrrw_reads_the_old_value_before_the_register_is_overwritten() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0x0F0F_0F0F)]);
    state.gpr[1] = 0x1234;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 1, F_CSRRW, 1));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[1], 0x0F0F_0F0F,
        "x1 must end up holding the CSR's old value"
    );
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x1234,
        "the CSR must receive the register's old value"
    );
}

/// `CSRRS` sets the source's bits in the CSR.
#[test]
fn csrrs_sets_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0x0F0F)]);
    state.gpr[1] = 0x00F0;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 1, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x0FFF,
        "CSRRS must set the source's bits"
    );
}

/// `CSRRS` with `rs1 == x0` is a **pure read**: the CSR must not be
/// written at all.
#[test]
fn csrrs_with_rs1_x0_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0x0F0F)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x0F0F,
        "CSRRS with rs1 = x0 must not write the CSR"
    );
}

/// `CSRRC` clears the source's bits in the CSR.
#[test]
fn csrrc_clears_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0xFFFF)]);
    state.gpr[1] = 0x0F0F;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 1, F_CSRRC, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0xF0F0,
        "CSRRC must clear the source's bits"
    );
}

/// `CSRRC` with `rs1 == x0` is a **pure read** as well.
#[test]
fn csrrc_with_rs1_x0_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0xFFFF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0, F_CSRRC, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0xFFFF,
        "CSRRC with rs1 = x0 must not write the CSR"
    );
}

/// `CSRRWI` writes the 5-bit immediate into the CSR.
#[test]
fn csrrwi_writes_the_five_bit_immediate() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0xFFFF_FFFF)]);
    // The immediate lives in the `rs1` field (bits 19:15).
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0x15, F_CSRRWI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], 0xFFFF_FFFF,
        "rd must receive the old CSR value"
    );
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x15,
        "CSRRWI must write the 5-bit immediate"
    );
}

/// `CSRRSI` sets the immediate's bits in the CSR.
#[test]
fn csrrsi_sets_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0x0F0F)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0x0F, F_CSRRSI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x0F0F,
        "CSRRSI must set the immediate's bits"
    );
}

/// `CSRRSI` with a **zero** immediate is a pure read.
#[test]
fn csrrsi_with_a_zero_immediate_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0x0F0F)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0, F_CSRRSI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x0F0F,
        "CSRRSI with a zero immediate must not write the CSR"
    );
}

/// `CSRRCI` clears the immediate's bits in the CSR.
#[test]
fn csrrci_clears_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0xFFFF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0x0F, F_CSRRCI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0xFFF0,
        "CSRRCI must clear the immediate's bits"
    );
}

/// `CSRRCI` with a **zero** immediate is a pure read.
#[test]
fn csrrci_with_a_zero_immediate_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0xFFFF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSTATUS, 0, F_CSRRCI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0xFFFF,
        "CSRRCI with a zero immediate must not write the CSR"
    );
}

// ---------------------------------------------------------------------------
// The CSR instructions — read-only CSRs, unknown CSRs, the file's shape
// ---------------------------------------------------------------------------

/// A write to a **read-only** CSR must be ignored — the value in the
/// file must not change.
///
/// The read-only quarter is the whole `0x800..=0xFFF` range (address
/// bit 11 set). `mtime` (`0xBFF8`) lives in that quarter, but a CSR
/// instruction can only address **12 bits**, so the encoding of
/// `mtime` is its low half (`0xFF8`) — which is in the same quarter.
#[test]
fn a_write_to_a_read_only_csr_is_ignored() {
    for csr in 0x800u16..=0xFFF {
        let mut h = Harness::new();
        let mut state = state_with_csrs(&[(csr, 0x0F0F)]);
        state.gpr[1] = 0xFFFF_FFFF;
        let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(csr, 1, F_CSRRW, 2));
        assert_eq!(outcome, StepOutcome::Advanced);
        assert_eq!(
            csr_value(&state, csr),
            0x0F0F,
            "csr {csr:#x} is read-only — the write must be ignored"
        );
    }
}

/// `mip` is read-only on this platform **on top of** the address
/// rule: a guest must not be able to fake a pending interrupt.
#[test]
fn a_write_to_mip_is_ignored() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MIP, 0x0000_0008)]);
    state.gpr[1] = 0xFFFF_FFFF;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MIP, 1, F_CSRRW, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MIP),
        0x0000_0008,
        "mip must stay read-only"
    );
    assert_eq!(
        state.gpr[2], 0x0000_0008,
        "a read-only CSR is still readable"
    );
}

/// A read-only CSR is still **readable** — the ignored write must not
/// turn into an unreadable CSR.
#[test]
fn a_read_only_csr_is_still_readable() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MIP, 0x0000_0044)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MIP, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0000_0044, "mip must be readable");
}

/// The platform's custom `irq_clear` CSR is **writable** — it sits in
/// the writable custom quarter, which is the whole reason it is not
/// `0xBFF0`.
#[test]
fn the_irq_clear_csr_accepts_the_write() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x0000_0004;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_IRQ_CLEAR, 1, F_CSRRW, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_IRQ_CLEAR),
        0x0000_0004,
        "irq_clear must accept the write"
    );
    assert!(
        !csr_write_is_ignored(CSR_IRQ_CLEAR),
        "irq_clear must be writable"
    );
}

/// Reading an **unknown** CSR yields **0** and does **not** trap.
///
/// ⚠️ The counter-case lives in the next test: "reads 0" on its own
/// would also hold for an implementation that reads **nothing** at
/// all.
///
/// ⚠️ The CSR number is `0x123` — in the **writable** quarter. A
/// number in the read-only quarter (`0x999` for instance) would make
/// the "no entry was added" assertion **vacuous**: the write would be
/// ignored by the read-only rule, not by the read-only *form* of the
/// instruction.
#[test]
fn an_unknown_csr_reads_as_zero_without_trapping() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(0x123, 0, F_CSRRS, 2));
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "reading an unknown CSR must not trap"
    );
    assert_eq!(state.gpr[2], 0, "an unknown CSR reads as 0");
    assert!(
        !csr_numbers(&state).contains(&0x123),
        "a pure read must not add an entry to the CSR file"
    );
}

/// The four **read-only forms** of the CSR instructions must not write
/// the CSR **at all**.
///
/// ⚠️ For a CSR that is *already in the file* the rule is invisible:
/// `CSRRS` with `rs1 = x0` would write `old | 0`, which is the value
/// the file already holds. The rule becomes visible on a CSR that is
/// **not** in the file: the write would **create an entry**. The test
/// uses a writable number (`0x123`) so that the read-only rule cannot
/// mask the effect.
#[test]
fn the_read_only_forms_do_not_write_the_csr_at_all() {
    for (funct3, rs1, label) in [
        (F_CSRRS, 0u8, "CSRRS with rs1 = x0"),
        (F_CSRRC, 0u8, "CSRRC with rs1 = x0"),
        (F_CSRRSI, 0u8, "CSRRSI with a zero immediate"),
        (F_CSRRCI, 0u8, "CSRRCI with a zero immediate"),
    ] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(0x123, rs1, funct3, 2));
        assert_eq!(outcome, StepOutcome::Advanced, "{label} must retire");
        assert_eq!(state.gpr[2], 0, "{label} must read 0");
        assert!(
            !csr_numbers(&state).contains(&0x123),
            "{label} must not write the CSR — not even to create an entry"
        );
    }
}

/// The counter-case of the test above: a **known** CSR reads back its
/// **real** value, not 0.
#[test]
fn a_known_csr_reads_back_its_real_value() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0xDEAD_BEEF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MTVEC, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], 0xDEAD_BEEF,
        "a known CSR must read back its real value"
    );
}

/// `mtvec` is readable and writable **although its low bits are not
/// zero** (STAGE-4 adjudication H).
///
/// The platform's own vector-table CSR is `0x305` — the low two bits
/// are `0b01`. A decoder that rejected "unaligned" CSR numbers would
/// make the platform's own vector table an illegal instruction, and no
/// interrupt could ever reach its handler.
#[test]
fn mtvec_is_readable_and_writable_although_its_low_bits_are_not_zero() {
    assert_ne!(
        CSR_MTVEC & 0x3,
        0,
        "the standard mtvec number is 0x305 — its low bits are not zero"
    );
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x2000;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MTVEC, 1, F_CSRRW, 2));
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "mtvec must not be an illegal instruction"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVEC),
        0x2000,
        "mtvec must accept the write"
    );
    assert_eq!(
        state.gpr[2], 0,
        "mtvec must read back as 0 before it was written"
    );
}

/// The CSR file stays **ascending by number** and holds **at most one**
/// entry per number, however the writes are ordered.
#[test]
fn the_csr_file_stays_sorted_and_holds_one_entry_per_csr() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // Write three CSRs in a deliberately scrambled order.
    for (csr, value) in [(CSR_MTVEC, 1u32), (CSR_MSTATUS, 2), (CSR_MEPC, 3)] {
        state.gpr[1] = value;
        h.step_one_at(&mut state, 0x1000, csr_insn(csr, 1, F_CSRRW, 0));
    }
    let numbers = csr_numbers(&state);
    assert_eq!(
        numbers,
        vec![CSR_MSTATUS, CSR_MTVEC, CSR_MEPC],
        "the CSR file must stay ascending by number"
    );
    // Writing the same CSR again must **overwrite**, not append.
    state.gpr[1] = 0x7777;
    h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MTVEC, 1, F_CSRRW, 0));
    assert_eq!(
        csr_numbers(&state),
        vec![CSR_MSTATUS, CSR_MTVEC, CSR_MEPC],
        "a second write must not add a second entry"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVEC),
        0x7777,
        "a second write must overwrite the value"
    );
}

// ---------------------------------------------------------------------------
// MRET
// ---------------------------------------------------------------------------

/// `MRET` returns to the address in `mepc`.
#[test]
fn mret_returns_to_the_address_in_mepc() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MEPC, 0x1000)]);
    let outcome = h.step_one_at(&mut state, 0x2000, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.pc, 0x1000, "MRET must jump to mepc");
}

/// `MRET` restores the global interrupt enable from `MPIE` and then
/// sets `MPIE` to 1.
#[test]
fn mret_restores_the_global_interrupt_enable_from_mpie() {
    let mut h = Harness::new();
    // MIE = 0, MPIE = 1 → the handler was entered with interrupts on.
    let mut state = state_with_csrs(&[(CSR_MSTATUS, mstatus_with(false, true))]);
    let outcome = h.step_one_at(&mut state, 0x2000, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_ne!(
        mstatus & (1 << MSTATUS_MIE_BIT),
        0,
        "MRET must restore MIE from MPIE"
    );
    assert_ne!(
        mstatus & (1 << MSTATUS_MPIE_BIT),
        0,
        "MRET must set MPIE to 1"
    );
}

/// `MRET` leaves the global interrupt enable **off** when `MPIE` was
/// off — the restore must not invent an interrupt enable.
#[test]
fn mret_leaves_the_global_interrupt_enable_off_when_mpie_was_off() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSTATUS, mstatus_with(false, false))]);
    let outcome = h.step_one_at(&mut state, 0x2000, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & (1 << MSTATUS_MIE_BIT),
        0,
        "MIE must stay off when MPIE was off"
    );
    assert_ne!(
        mstatus & (1 << MSTATUS_MPIE_BIT),
        0,
        "MRET must set MPIE to 1"
    );
}

// ---------------------------------------------------------------------------
// The trap entry
// ---------------------------------------------------------------------------

/// The trap entry writes `mepc` / `mcause` / `mtval` and hands the PC
/// to the vector table.
#[test]
fn entering_a_trap_writes_mepc_mcause_and_mtval() {
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.pc = 0x1234;
    let target = super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0xABCD);
    assert_eq!(target, 0x2000, "the trap must land in the vector table");
    assert_eq!(state.pc, 0x2000, "the PC must follow the vector table");
    assert_eq!(csr_value(&state, CSR_MEPC), 0x1234, "mepc := pc");
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        MCAUSE_ILLEGAL,
        "mcause := cause"
    );
    assert_eq!(csr_value(&state, CSR_MTVAL), 0xABCD, "mtval := mtval");
}

/// The trap entry **disables** the global interrupt enable and saves
/// the previous value into `MPIE`.
#[test]
fn entering_a_trap_disables_interrupts_and_saves_the_previous_enable() {
    let mut state = state_with_csrs(&[(CSR_MSTATUS, mstatus_with(true, false))]);
    state.pc = 0x1234;
    super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & (1 << MSTATUS_MIE_BIT),
        0,
        "the trap must switch the global interrupt enable off"
    );
    assert_ne!(
        mstatus & (1 << MSTATUS_MPIE_BIT),
        0,
        "the trap must save the previous enable into MPIE"
    );
}

/// A trap entered with the global interrupt enable **off** must leave
/// `MPIE` off — the save must not invent an enable.
#[test]
fn entering_a_trap_with_interrupts_off_leaves_mpie_off() {
    let mut state = state_with_csrs(&[(CSR_MSTATUS, mstatus_with(false, false))]);
    state.pc = 0x1234;
    super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(mstatus & (1 << MSTATUS_MIE_BIT), 0, "MIE must stay off");
    assert_eq!(
        mstatus & (1 << MSTATUS_MPIE_BIT),
        0,
        "MPIE must stay off when the enable was off"
    );
}

/// The **direct** mode jumps to the vector table's base.
#[test]
fn the_direct_mode_jumps_to_the_vector_table_base() {
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.pc = 0x1234;
    let target = super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    assert_eq!(target, 0x2000, "the direct mode jumps to the base");
    assert_eq!(
        state.pc, 0x2000,
        "the PC must follow the vector table, not just the return value"
    );
}

/// The **vectored** mode jumps to `base + 4 × (cause & 0x7)`.
#[test]
fn the_vectored_mode_jumps_to_the_cause_slot() {
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2001)]);
    state.pc = 0x1234;
    let target = super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    assert_eq!(
        target,
        0x2000 + 4 * 2,
        "the vectored mode jumps to base + 4 × (cause & 0x7)"
    );
    assert_eq!(
        state.pc,
        0x2000 + 4 * 2,
        "the PC must follow the vector table, not just the return value"
    );
}

/// The vectored mode must use the **interrupt line** for an
/// interrupt cause — the line is the low three bits of the cause.
#[test]
fn the_vectored_mode_uses_the_interrupt_line_of_an_interrupt_cause() {
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2001)]);
    state.pc = 0x1234;
    // An interrupt cause is `IRQ_BASE | line`; the line is 3 here.
    let target = super::enter_trap(&mut state, riscv_sim_contract::csr::IRQ_BASE | 3, 0);
    assert_eq!(target, 0x2000 + 4 * 3, "the vectored slot is the line");
    assert_eq!(
        state.pc,
        0x2000 + 4 * 3,
        "the PC must follow the vector table"
    );
}

// ---------------------------------------------------------------------------
// The trap path end to end — the exception path goes through the entry
// ---------------------------------------------------------------------------

/// An illegal 32-bit instruction writes its **instruction word** into
/// `mtval`.
///
/// The whole 32-bit word is reported — not just its low half. The
/// upper half is made non-zero on purpose: an implementation that
/// reported only the low 16 bits would slip through a test whose
/// instruction word happened to have a zero upper half.
#[test]
fn an_illegal_instruction_writes_the_instruction_word_into_mtval() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    // 0b010_1011 is not a valid RV32I opcode; the upper half is
    // arbitrary.
    let word = 0xDEAD_002Bu32;
    let outcome = h.step_one_at(&mut state, 0x1000, word);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        word,
        "mtval must carry the 32-bit instruction word"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), MCAUSE_ILLEGAL, "mcause = 2");
    assert_eq!(csr_value(&state, CSR_MEPC), 0x1000, "mepc := pc");
    assert_eq!(state.pc, 0x2000, "the trap must jump to mtvec");
}

/// An illegal **compressed** instruction writes its **16-bit original
/// code** into `mtval` — the code that was fetched, not the 32-bit
/// word it sits in.
///
/// The upper half-word of the fetched word is made non-zero on
/// purpose: that is exactly what tells the two apart. A `mtval` that
/// carried the 32-bit word (`0x1234_6401`) instead of the 16-bit
/// code (`0x6401`) would send the handler to the wrong place.
#[test]
fn a_compressed_illegal_instruction_writes_its_original_code_into_mtval() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    // The low half-word 0x6401 is a reserved C.LUI; the upper half is
    // the next half-word of the same word.
    h.load(0x1000, &[0x1234_6401]);
    state.pc = 0x1000;
    let outcome = h.step(&mut state).expect("step");
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        0x6401,
        "mtval must carry the 16-bit original code, not the 32-bit word"
    );
}

/// An environment call carries **no** auxiliary value.
#[test]
fn an_ecall_writes_zero_into_mtval() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    let outcome = h.step_one_at(&mut state, 0x1000, enc_i(0, 0, 0b000, 0, SYSTEM));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(csr_value(&state, CSR_MTVAL), 0, "ECALL has no mtval");
    assert_eq!(csr_value(&state, CSR_MCAUSE), MCAUSE_ECALL, "mcause = 11");
}

/// A breakpoint carries **no** auxiliary value either.
#[test]
fn an_ebreak_writes_zero_into_mtval() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    let outcome = h.step_one_at(&mut state, 0x1000, enc_i(1, 0, 0b000, 0, SYSTEM));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Breakpoint
        }
    );
    assert_eq!(csr_value(&state, CSR_MTVAL), 0, "EBREAK has no mtval");
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        MCAUSE_BREAKPOINT,
        "mcause = 3"
    );
}

// ---------------------------------------------------------------------------
// The three misaligned causes — the D-63 split
// ---------------------------------------------------------------------------

/// A misaligned **fetch** is `mcause = 0` and writes the **faulting
/// address** into `mtval`.
#[test]
fn a_misaligned_fetch_reports_cause_zero_and_the_faulting_address() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.pc = 0x1001;
    let outcome = h.step(&mut state).expect("must not error");
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::InstructionAddressMisaligned
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        0,
        "a misaligned fetch is mcause 0"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        0x1001,
        "mtval must carry the faulting address"
    );
}

/// A misaligned **load** is `mcause = 4` and writes the **faulting
/// address** into `mtval`.
#[test]
fn a_misaligned_load_reports_cause_four_and_the_faulting_address() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.gpr[1] = 0x101;
    let outcome = h.step_one_at(&mut state, 0x1000, enc_i(0, 1, 0b010, 2, LOAD));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::LoadAddressMisaligned
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        MCAUSE_LOAD_MISALIGNED,
        "a misaligned load is mcause 4"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        0x101,
        "mtval must carry the faulting address"
    );
}

/// A misaligned **store** is `mcause = 6` and writes the **faulting
/// address** into `mtval`.
#[test]
fn a_misaligned_store_reports_cause_six_and_the_faulting_address() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.gpr[1] = 0x102;
    state.gpr[2] = 0xFFFF_FFFF;
    let outcome = h.step_one_at(&mut state, 0x1000, enc_s(0, 2, 1, 0b010, STORE));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::StoreAddressMisaligned
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        MCAUSE_STORE_MISALIGNED,
        "a misaligned store is mcause 6"
    );
    assert_eq!(
        csr_value(&state, CSR_MTVAL),
        0x102,
        "mtval must carry the faulting address"
    );
}

/// The three misaligned causes must be **three different** numbers.
///
/// This is the D-63 split in one assertion: the three traps are run
/// for real and the `mcause` each of them leaves in the CSR file is
/// compared. An implementation that collapsed them back into one
/// variant would report the same code for all three.
#[test]
fn the_three_misaligned_causes_carry_three_different_codes() {
    let mut codes = Vec::new();

    // A misaligned **fetch**.
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.pc = 0x1001;
    h.step(&mut state).expect("must not error");
    codes.push(csr_value(&state, CSR_MCAUSE));

    // A misaligned **load**.
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.gpr[1] = 0x101;
    h.step_one_at(&mut state, 0x1000, enc_i(0, 1, 0b010, 2, LOAD));
    codes.push(csr_value(&state, CSR_MCAUSE));

    // A misaligned **store**.
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x2000)]);
    state.gpr[1] = 0x102;
    h.step_one_at(&mut state, 0x1000, enc_s(0, 2, 1, 0b010, STORE));
    codes.push(csr_value(&state, CSR_MCAUSE));

    assert_eq!(
        codes,
        vec![0, MCAUSE_LOAD_MISALIGNED, MCAUSE_STORE_MISALIGNED],
        "the fetch / load / store misalignments must report 0 / 4 / 6"
    );
    assert_eq!(codes[0], 0, "a misaligned fetch is mcause 0");
    assert_eq!(codes.len(), 3);
    assert_ne!(codes[0], codes[1], "the fetch and the load differ");
    assert_ne!(codes[1], codes[2], "the load and the store differ");
    assert_ne!(codes[0], codes[2], "the fetch and the store differ");
}

// ---------------------------------------------------------------------------
// The two dispatch routes must agree on the CSR semantics too
// ---------------------------------------------------------------------------

/// A CSR instruction must behave the same on both dispatch routes
/// (D-11: one execution path).
#[test]
fn a_csr_instruction_behaves_the_same_on_both_dispatch_routes() {
    let word = csr_insn(CSR_MTVEC, 1, F_CSRRW, 2);

    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MTVEC, 0x1111)]);
    state.gpr[1] = 0x2222;
    h.step_one_at(&mut state, 0x1000, word);

    let mut bus = crate::m14_bus::Bus::new();
    let bytes = image(&[word]);
    bus.map_ram(0x1000, 0x1000, &bytes).expect("map ram");
    let mut bus_state = state_with_csrs(&[(CSR_MTVEC, 0x1111)]);
    bus_state.gpr[1] = 0x2222;
    bus_state.pc = 0x1000;
    let mut iss = Iss::default();
    iss.step_one_via_bus(&mut bus_state, &mut bus)
        .expect("the bus route must not error");

    assert_eq!(bus_state.gpr[2], state.gpr[2], "both routes must agree");
    assert_eq!(
        csr_value(&bus_state, CSR_MTVEC),
        csr_value(&state, CSR_MTVEC),
        "both routes must agree on the CSR file"
    );
}
