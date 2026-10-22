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
    csr_write_is_ignored, CSR_IRQ_CLEAR, CSR_MCAUSE, CSR_MEPC, CSR_MIP, CSR_MISA, CSR_MISA_VALUE,
    CSR_MSCRATCH, CSR_MSTATUS, CSR_MTVAL, CSR_MTVEC, MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT,
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
//
// ⚠️ **载体 CSR 是 `mscratch`，不是 `mstatus`**：`mstatus` 是 **WARL**
// （台账 **D-98**）—— 它的**保留位**写被忽略、读回 0 ⇒ 它**不能**再当
// 「一个普通的 32 位存储」用（写 `0xFFFF_FFFF` 读回的不是 `0xFFFF_FFFF`）。
// `mscratch` 才是平台文档里那个「plain storage CSR」。
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
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0x0F0F_0F0F)]);
    state.gpr[1] = 0x1234;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 1, F_CSRRW, 1));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[1], 0x0F0F_0F0F,
        "x1 must end up holding the CSR's old value"
    );
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x1234,
        "the CSR must receive the register's old value"
    );
}

/// `CSRRS` sets the source's bits in the CSR.
#[test]
fn csrrs_sets_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0x0F0F)]);
    state.gpr[1] = 0x00F0;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 1, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x0FFF,
        "CSRRS must set the source's bits"
    );
}

/// `CSRRS` with `rs1 == x0` is a **pure read**: the CSR must not be
/// written at all.
#[test]
fn csrrs_with_rs1_x0_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0x0F0F)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x0F0F,
        "CSRRS with rs1 = x0 must not write the CSR"
    );
}

/// `CSRRC` clears the source's bits in the CSR.
#[test]
fn csrrc_clears_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0xFFFF)]);
    state.gpr[1] = 0x0F0F;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 1, F_CSRRC, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0xF0F0,
        "CSRRC must clear the source's bits"
    );
}

/// `CSRRC` with `rs1 == x0` is a **pure read** as well.
#[test]
fn csrrc_with_rs1_x0_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0xFFFF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 0, F_CSRRC, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0xFFFF,
        "CSRRC with rs1 = x0 must not write the CSR"
    );
}

/// `CSRRWI` writes the 5-bit immediate into the CSR.
#[test]
fn csrrwi_writes_the_five_bit_immediate() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0xFFFF_FFFF)]);
    // The immediate lives in the `rs1` field (bits 19:15).
    let outcome = h.step_one_at(
        &mut state,
        0x1000,
        csr_insn(CSR_MSCRATCH, 0x15, F_CSRRWI, 2),
    );
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], 0xFFFF_FFFF,
        "rd must receive the old CSR value"
    );
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x15,
        "CSRRWI must write the 5-bit immediate"
    );
}

/// `CSRRSI` sets the immediate's bits in the CSR.
#[test]
fn csrrsi_sets_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0x0F0F)]);
    let outcome = h.step_one_at(
        &mut state,
        0x1000,
        csr_insn(CSR_MSCRATCH, 0x0F, F_CSRRSI, 2),
    );
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x0F0F,
        "CSRRSI must set the immediate's bits"
    );
}

/// `CSRRSI` with a **zero** immediate is a pure read.
#[test]
fn csrrsi_with_a_zero_immediate_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0x0F0F)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 0, F_CSRRSI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0x0F0F, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x0F0F,
        "CSRRSI with a zero immediate must not write the CSR"
    );
}

/// `CSRRCI` clears the immediate's bits in the CSR.
#[test]
fn csrrci_clears_the_bits_of_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0xFFFF)]);
    let outcome = h.step_one_at(
        &mut state,
        0x1000,
        csr_insn(CSR_MSCRATCH, 0x0F, F_CSRRCI, 2),
    );
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0xFFF0,
        "CSRRCI must clear the immediate's bits"
    );
}

/// `CSRRCI` with a **zero** immediate is a pure read.
#[test]
fn csrrci_with_a_zero_immediate_is_a_pure_read() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MSCRATCH, 0xFFFF)]);
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 0, F_CSRRCI, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.gpr[2], 0xFFFF, "rd must receive the old CSR value");
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
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

/// `MRET` clears **only the lowest bit** of `mepc` — the mask follows
/// **IALIGN**, and this platform declares the **C** extension in `misa`
/// ⇒ IALIGN = 16 ⇒ one bit, not two.
///
/// ⚠️ **Why the discriminator is "bit 1 survives" and not "the low two
/// bits are zero"**: the ledger's original wording (D-93) said
/// "`mepc[1:0]` is treated as zero", which is the **IALIGN = 32** (no C)
/// rule.  A mask of `~3` and a mask of `~1` **agree** on every 4-byte
/// aligned address — which is *every* address the upstream corpus ever
/// puts in `mepc` (it always `la`s an aligned label).  So the only
/// address that tells the two masks apart is one with **bit 1 set**,
/// i.e. the address of a **2-byte-aligned (compressed) instruction**.
///
/// ⇒ The reference side was measured doing exactly this: a `c.sw` at
/// `0x8000014a` trapped, and `mret` landed back on `4k+2` (not `4k`)
/// ⇒ mask `~1`.
///
/// ⚠️ The expected value is written as an **architecture literal**
/// (`0x1002`), not derived from `MEPC_ALIGN_MASK` — otherwise changing
/// the mask would move the expectation with it and the test would stay
/// green (the ledger's own lesson: an assertion that shares a constant
/// with the implementation has no discriminating power).
#[test]
fn mret_clears_only_the_lowest_bit_of_mepc() {
    let mut h = Harness::new();
    // 0x1002 — bit 1 set, bit 0 clear: the address of a compressed
    // instruction.  A `~3` mask would give 0x1000 instead.
    let mut state = state_with_csrs(&[(CSR_MEPC, 0x1002)]);
    let outcome = h.step_one_at(&mut state, 0x2000, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.pc, 0x1002,
        "MRET must clear only mepc's lowest bit (IALIGN=16, C extension) — \
         a `~3` mask would land on 0x1000, i.e. in the middle of the \
         compressed instruction"
    );
}

/// `MRET` clears `mepc`'s lowest bit when it is **set**.
///
/// This is the other half of the boundary: bit 0 set ⇒ it must be
/// cleared.  Together with
/// [`mret_clears_only_the_lowest_bit_of_mepc`] the two cases pin the
/// mask from both sides — writing only one of them would let a
/// `~0` (no mask at all) or a `~3` slip through.
#[test]
fn mret_clears_the_lowest_bit_of_mepc_when_it_is_set() {
    let mut h = Harness::new();
    // 0x1003 — bit 1 set **and** bit 0 set.
    let mut state = state_with_csrs(&[(CSR_MEPC, 0x1003)]);
    let outcome = h.step_one_at(&mut state, 0x2000, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.pc, 0x1002,
        "MRET must clear mepc's bit 0 and keep bit 1 (IALIGN=16)"
    );
}

/// A **user** `csrw mepc` write is masked by **IALIGN** — `~1`.
///
/// ⚠️ This is a **different path** from the trap entry, and the
/// difference is architecturally required:
///
/// | path | reference side (measured) | why |
/// | --- | --- | --- |
/// | **trap entry** writes `mepc` | **exact PC** — a `c.sw` at `0x8000014a` left `mepc = 0x8000014a` (bit 1 **preserved**) | the handler must return to **the faulting instruction**; masking bit 1 would land in its middle |
/// | **user** `csrw mepc` | **masked `~1`** — `csrw mepc, 0x1003` read back **0x1002** | `mepc` is WARL: any legal value may be stored |
///
/// ⚠️ The trap-entry half is pinned by the golden corpus row
/// `local/compressed_trap` (upstream riscv-tests never traps from a
/// 2-byte-aligned instruction, so nothing else would notice).
#[test]
fn a_user_write_to_mepc_is_masked_by_ialign() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    state.gpr[10] = 0x1003; // a0 — bit 1 **and** bit 0 set
    // `csrrw a0, mepc, a0` — writes a0 into mepc, reads the old value back.
    let csrrw = csr_insn(CSR_MEPC, 10, F_CSRRW, 10);
    let _ = h.step_one_at(&mut state, 0x2000, csrrw);
    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x1002,
        "a user write to mepc must be masked by IALIGN (~1) — the \
         reference side reads back 0x1002 for a 0x1003 write"
    );
}

/// The **trap entry** writes `mepc` **without** dropping bit 1 — so a
/// trap taken on a **2-byte-aligned (compressed)** instruction can be
/// returned to.
///
/// ⚠️ **What this pins, precisely**: the trap path's mask is
/// **IALIGN = 16** (`~1`), **not** IALIGN = 32 (`~3`).  A `~3` mask
/// would turn `0x1002` into `0x1000` — i.e. land the handler's return
/// in the **middle** of the compressed instruction.
///
/// ⚠️ **What it does *not* pin** (and cannot, at a legal address): the
/// difference between the trap entry writing the PC *verbatim* and
/// writing it through the IALIGN mask.  Every legal instruction
/// address is **even** (bit 0 clear), so `~1` leaves it unchanged —
/// the two are indistinguishable, and the reference side's behaviour
/// there is **unobservable**.  The platform uses the verbatim write
/// ([`csr::csr_write_raw`]) because it states the intent, but the
/// reference side could not be shown to do the same.
///
/// The expected value is an **architecture literal** (`0x1002`), not
/// derived from `MEPC_ALIGN_MASK` — an assertion that shares a
/// constant with the implementation has no discriminating power.
#[test]
fn the_trap_entry_writes_the_exact_faulting_pc_into_mepc() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    // Trap at a 2-byte-aligned PC — the address of a compressed
    // instruction.  `mepc` must come back **verbatim**.
    let _ = h.step_one_at(&mut state, 0x1002, enc_i(0, 0, 0, 0, 0x0000_0073));
    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x1002,
        "the trap entry must write the exact PC — masking bit 1 would \
         make a handler returning from a compressed instruction land in \
         the middle of it (and would diverge from the reference side)"
    );
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

/// The trap entry records the privilege level it was taken from in
/// `mstatus.MPP`.
///
/// ⚠️ This is **not** cosmetic: the reference side does it (measured —
/// Spike reports `mstatus = 0x0018_0080` inside a trap handler, this
/// platform used to report `0x0000_0080` ⇒ the two sides disagreed
/// field-for-field), and the M-13 LLD §5.2.4 says so explicitly
/// ("`MPIE←MIE，MIE←0，MPP←M`").
///
/// The field is **WARL** and this platform implements M-mode only ⇒
/// `M` (= 3) is the only legal value ⇒ the trap writes it, it does not
/// merely preserve whatever the guest had put there.
///
/// ⚠️ **断言用架构字面量，不用契约层的常量** —— 写成
/// `mstatus >> MSTATUS_MPP_SHIFT == MSTATUS_MPP_M` 的话，断言与生产代码
/// **共用同一个常量** ⇒ 把常量改成 0 时断言照样成立 ⇒ **恒真式**，
/// 断言不了任何东西（反向变异 **V-17** 实测：改成 0 时这条用例**没红**）。
/// ⇒ 这里钉的是**架构事实**：`mstatus` 的 bits 12:11 = `0b11`。
#[test]
fn entering_a_trap_records_machine_mode_in_mpp() {
    let mut state = state_with_csrs(&[(CSR_MSTATUS, 0)]);
    state.pc = 0x1234;
    super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        (mstatus >> 11) & 0b11,
        0b11,
        "the trap must record M-mode in mstatus.MPP (bits 12:11)"
    );
}

/// The trap entry **overwrites** a guest-planted `MPP` — the field says
/// "where the trap came from", not "what the guest last wrote".
#[test]
fn entering_a_trap_overwrites_a_guest_planted_mpp() {
    let planted = 0b01 << 11; // "U-mode" — not a legal value here
    let mut state = state_with_csrs(&[(CSR_MSTATUS, planted)]);
    state.pc = 0x1234;
    super::enter_trap(&mut state, MCAUSE_ILLEGAL, 0);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        (mstatus >> 11) & 0b11,
        0b11,
        "a trap taken in M-mode must record M-mode, whatever the guest wrote"
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

// ---------------------------------------------------------------------------
// The **report** — which CSR did this instruction actually write?
// ---------------------------------------------------------------------------

/// 六条 CSR 指令**各自**都要回报「刚写了哪个 CSR」。
///
/// ⚠️ 这是 v0.19 检视轮 **F-1** 的修复所依赖的判据：M-17 靠这个回报
/// 知道「这条指令写了 `irq_clear`」。判据若只挂在**一条**臂上，其余
/// 五条臂写 `irq_clear` 就会**静默不生效** ⇒ 那条线被单级防护**永久
/// 堵死**（该线再也交付不了任何中断）。
///
/// ⚠️ **准确的后果表述**（本轮收口订正）：F-1 的后果是「**那条线被永久
/// 堵死**」，**不是**「平台活锁」。早先据 K-17 / K-18 的「挂死」观测写下
/// 的「平台活锁」是**错的** —— 那个「挂死」经复核是**假 HANG**
/// （macOS 对新链接二进制的 `exec` 扫描堵住了测试二进制，**与被测代码
/// 无关**；见 `docs/reviews/REVIEWS.md` §3.10.5 第 17 条）。新口径下
/// **K-17 判 RED ✓**。⇒ 「**会不会挂死取决于哪条路径**」：真正会挂死的是
/// **K-10**（`take_intr` **取走不消失** ⇒ 同一交付被反复交付 ⇒ 引擎内层
/// 循环不退出），那才是台账 **D-83** 的生产侧。
///
/// ⚠️ 这条断言是**逐臂**的。`CSRRS` / `CSRRC` / `CSRSI` / `CSRCI` 的
/// 写入值与源操作数有关（`old | value` / `old & !value`），但**只要
/// 执行臂真的调了写**，就必须回报 —— **哪怕写进去的值与原来相同**
/// （那正是 F-1 那个「第二次清除被静默丢弃」的现场）。
///
/// ⚠️ 变异自检 **K-39~K-42** 就是把这四条臂逐条改坏的（退化成
/// 「值变了才回报」）；**K-38** 改的是立即数形式的 `CSRWI` 臂。
///
/// ⚠️ **每条臂写两次**（第二次是**同一个值**）：「值变了才回报」的退化在
/// **第一次**抓不住（第一次值确实变了）⇒ 只写一次的断言对这组变异
/// **0 红** ⇒ 判别力缺口。
#[test]
fn every_csr_instruction_reports_the_csr_number_it_wrote() {
    // (funct3, rs1-field)
    //
    // ⚠️ 立即数形式的 `rs1` 字段装的是 **zimm**（5 位）—— 编码上是
    // 同一个字段，语义上不是同一个操作数。
    for (funct3, rs1_field) in [
        (F_CSRRW, 1u8),
        (F_CSRRS, 1),
        (F_CSRRC, 1),
        (F_CSRRWI, 2),
        (F_CSRRSI, 2),
        (F_CSRRCI, 2),
    ] {
        let mut h = Harness::new();
        let mut state = state_with_csrs(&[]);
        state.gpr[1] = 0x2; // CSRRS / CSRRC 的非零源
                            // ⚠️ **每条臂写两次**：第一次写时值确实变了（0 → 2）⇒ 判据若退化成
                            //    「**值变了**才回报」，**第一次**照样回报 ⇒ 抓不住 ⇒ 必须有
                            //    **第二次**（**同一个值**）才抓得住 —— 那正是 F-1 的现场。
        for round in 1..=2u32 {
            assert_eq!(
                h.iss.take_last_csr_written(),
                None,
                "funct3 = {funct3:03b} 第 {round} 轮之前：上一条指令的回报必须已经被取走"
            );
            let outcome = h.step_one_at(
                &mut state,
                0x1000,
                csr_insn(CSR_IRQ_CLEAR, rs1_field, funct3, 0),
            );
            assert_eq!(
                outcome,
                StepOutcome::Advanced,
                "funct3 = {funct3:03b} 第 {round} 轮"
            );
            assert_eq!(
                h.iss.take_last_csr_written(),
                Some(CSR_IRQ_CLEAR),
                "funct3 = {funct3:03b} 第 {round} 轮：这条指令写了 CSR 0x{CSR_IRQ_CLEAR:03x}，\
                 必须回报它 —— **哪怕写进去的值与上一次完全相同**"
            );
        }
    }
}

/// 被**忽略**的写**不回报**。
///
/// 回报的语义是「这条指令**真的写了**某个 CSR」。只读 CSR（`mip`）
/// 的写被忽略 ⇒ **没写** ⇒ 不该有回报。
///
/// ⚠️ 若这里也回报，M-17 就会把「写了 `mip`」当成「写了 `irq_clear`」
/// ⇒ **清错线**（清掉一条根本没被拉起的线，真正的挂起项还在）。
#[test]
fn an_ignored_write_reports_nothing() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[(CSR_MIP, 0)]);
    state.gpr[1] = 0x1;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MIP, 1, F_CSRRW, 0));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        h.iss.take_last_csr_written(),
        None,
        "只读 CSR 的写被忽略 ⇒ 没有真的写 ⇒ 不该回报"
    );
}

/// **重复写同一个值**也必须回报。
///
/// ⚠️ 这是 F-1 的**现场**：guest 的 handler **每次**都写**同一个**掩码
/// ⇒ 判据若退化成「**值变了**才回报」，**第二次**就会被**静默跳过**
/// ⇒ 第二次清线丢掉 ⇒ `active_irq` 永不复位 ⇒ 那条线被单级防护
/// **永久堵死**。
///
/// ⇒ ⚠️ **只写一次**的断言**抓不住**这个退化（第一次写时值确实变了）
/// ⇒ 这条用例必须**写两次**，且两次写的是**同一个值**。
///
/// 变异自检 **K-18b / K-38**（把回报退化成「值变了才回报」）就是被这条
/// 抓红的 —— 而**不是**被「只写一次」的那条。
#[test]
fn a_repeated_write_of_the_same_value_still_reports_the_csr() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    state.gpr[1] = 0x2;
    for round in 1..=2u32 {
        let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_IRQ_CLEAR, 1, F_CSRRW, 0));
        assert_eq!(outcome, StepOutcome::Advanced, "第 {round} 轮");
        assert_eq!(
            h.iss.take_last_csr_written(),
            Some(CSR_IRQ_CLEAR),
            "第 {round} 轮：这条指令写了 CSR 0x{CSR_IRQ_CLEAR:03x}，必须回报它 —— \
             **哪怕写进去的值与上一次完全相同**"
        );
    }
}

// ---------------------------------------------------------------------------
// 裁决 E —— `misa`
// ---------------------------------------------------------------------------

/// `misa` 必须读出平台的**常量**值 —— 哪怕 guest 之前从没碰过它。
///
/// ⚠️ 断言的是**读出来的值**，不是「文件里有没有这一项」—— `misa` 是
/// **算出来的**，不落在 CSR 文件里（那会让 `SimState::is_empty()` 的
/// 语义变掉）。
#[test]
fn misa_reads_back_the_platform_value() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    // `csrrs rd, misa, x0` —— `rs1 == x0` ⇒ 纯读。
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MISA, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], CSR_MISA_VALUE,
        "misa 必须读出平台常量 0x{CSR_MISA_VALUE:08x}"
    );
}

/// guest **写** `misa` 之后，读回来仍是平台常量。
///
/// ⚠️ `misa` 是 WARL 寄存器，但**本平台的规则是「写被忽略」**——
/// guest 不能通过写 `misa` 宣称一个平台没有的扩展。
#[test]
fn a_write_to_misa_does_not_change_what_it_reads_back() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    state.gpr[1] = 0xFFFF_FFFF;
    // `csrrw x2, misa, x1` —— x2 拿到**旧值**（平台常量）。
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MISA, 1, F_CSRRW, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[2], CSR_MISA_VALUE,
        "csrrw 的 rd 拿到旧值 —— 旧值必须是平台常量"
    );
    // 再读一次：仍是常量（写没生效）。
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MISA, 0, F_CSRRS, 3));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        state.gpr[3], CSR_MISA_VALUE,
        "写 misa 之后读回来必须仍是平台常量"
    );
}

/// ⚠️ **`misa` 不落在 CSR 文件里** —— 它是**算出来的**。
///
/// 理由：`SimState::csrs` 为空 ⇒ 会话快照的**「空」判定**
/// （`EngineSnapshotDto::is_empty`：没有寄存器写入、**没有 CSR**、
/// 没有 pending 写、虚拟时钟为零）仍然成立。若把 `misa` 预置进文件，
/// 一个**只读了一次 `misa`** 的 guest 就不再是「空」的了。
#[test]
fn misa_is_not_stored_in_the_csr_file() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    assert!(state.csrs.is_empty(), "起点必须是空状态 —— 这条用例的前提");
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MISA, 0, F_CSRRS, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert!(
        !csr_numbers(&state).contains(&CSR_MISA),
        "misa 不得进 CSR 文件 —— 实得 {:?}",
        csr_numbers(&state)
    );
    assert!(
        state.csrs.is_empty(),
        "读一次 misa 之后 CSR 文件仍必须是空的 —— 实得 {:?}",
        state.csrs
    );
}

// ---------------------------------------------------------------------------
// 裁决 D —— 未实现的 CSR：写被忽略，读回 0
// ---------------------------------------------------------------------------

/// ⚠️ **未实现的 CSR 写被忽略、读回 0** —— 与参考侧一致。
///
/// 这是黄金对拍查出来的缺陷（D-92）：平台原来把写**存进文件**并原样
/// 读回，于是 guest 能**凭空造出**一个平台没有的寄存器。
///
/// ⚠️ 断言要落在**两处**：文件里**没有**这一项（不是「存了 0」），
/// 以及读回来是 **0**。
#[test]
fn an_unimplemented_csr_ignores_the_write_and_reads_back_zero() {
    for csr in [0x7A1u16, 0x7A2, 0x302, 0x303] {
        let mut h = Harness::new();
        let mut state = state_with_csrs(&[]);
        state.gpr[1] = 0x0123_4567;
        let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(csr, 1, F_CSRRW, 2));
        assert_eq!(outcome, StepOutcome::Advanced, "csr {csr:#x}");
        assert!(
            !csr_numbers(&state).contains(&csr),
            "csr {csr:#x} 未实现 ⇒ 不得进 CSR 文件 —— 实得 {:?}",
            csr_numbers(&state)
        );
        // 读回来必须是 0。
        let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(csr, 0, F_CSRRS, 3));
        assert_eq!(outcome, StepOutcome::Advanced, "csr {csr:#x}");
        assert_eq!(state.gpr[3], 0, "csr {csr:#x} 未实现 ⇒ 读回必须是 0");
    }
}

/// 已实现的 CSR **照常**能写能读 —— 否则「未实现 ⇒ 写被忽略」这条规则
/// 就过头了。
///
/// ⚠️ 用 `mscratch` 当样本：它是**普通存储 CSR**，上游 CSR 用例会拿它
/// 做读改写语义 ⇒ 它必须真的存得住。
#[test]
fn an_implemented_csr_still_takes_the_write() {
    let mut h = Harness::new();
    let mut state = state_with_csrs(&[]);
    state.gpr[1] = 0x0123_4567;
    let outcome = h.step_one_at(&mut state, 0x1000, csr_insn(CSR_MSCRATCH, 1, F_CSRRW, 2));
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSCRATCH),
        0x0123_4567,
        "mscratch 是已实现的存储 CSR ⇒ 写必须落下"
    );
}
