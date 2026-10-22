//! The CSR file, the CSR accessors, and the **one** trap entry.
//!
//! # The CSR file
//!
//! The platform keeps **every** CSR value in the generic CSR file
//! ([`SimState::csrs`]) — a list of `(number, value)` pairs with **at
//! most one entry per number**, kept **ascending by number**.
//!
//! The file is a **contract** field, and that is the reason it is used
//! for `mtval` / `mstatus` / `mepc` / `mcause` too (STAGE-4
//! adjudication A): it already round-trips through the session
//! snapshot, so a dedicated `mtval` field on [`SimState`] would be a
//! **second landing spot for the same fact**.
//!
//! Access rules (STAGE-4 §2.2):
//!
//! * **read** — a CSR that is **not in the file** reads as **0**; it
//!   does **not** trap (reading an unimplemented CSR is the standard
//!   RV32 behaviour);
//! * **write** — a CSR that **is in the file** is overwritten; a CSR
//!   that is **not** is appended, keeping the file ascending by
//!   number, so the same number can never appear twice.
//!
//! # The trap entry
//!
//! [`enter_trap`] is the platform's **only** trap entry: the
//! **exception** path (illegal instruction / ECALL / EBREAK /
//! misaligned access) and the **interrupt** path (M-16's delivery)
//! both come through it, so "what a trap does to the architectural
//! state" has **one** answer instead of two that can drift.
//!
//! ⚠️ The CSR **accessors** below are `pub(crate)`: they are internal
//! to the kernel crate (M-16 and M-17 may use them), but the crate's
//! public API grows by exactly **one** symbol — [`enter_trap`].

use riscv_sim_contract::csr::{
    CSR_MCAUSE, CSR_MEPC, CSR_MISA, CSR_MISA_VALUE, CSR_MSTATUS, CSR_MTVAL, CSR_MTVEC,
    MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT,
};
use riscv_sim_contract::model::SimState;

/// Read a CSR out of the generic CSR file.
///
/// A CSR that is **not in the file** reads as **0** — it does **not**
/// trap (STAGE-4 §2.2: "读：找不到 ⇒ 返回 0（不是陷阱）").
///
/// ⚠️ **`misa` is the one exception**: it is **not** stored in the file
/// but **computed** from the platform's constant ([`CSR_MISA_VALUE`]),
/// because it reports what the platform *is* rather than what the guest
/// last wrote.  Reading it must answer the same on every read — a guest
/// that could write `misa` could otherwise claim an extension the
/// platform does not have.
///
/// ⚠️ Keeping it out of the file also leaves the CSR file **empty** for a
/// guest that has touched no CSR — which is what the session snapshot's
/// emptiness predicate (`EngineSnapshotDto::is_empty`: "no registers
/// written, **no CSRs**, no pending writes, clock at zero") reads. A
/// pre-seeded `misa` entry would have broken that (see the frozen spec
/// for this batch, `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md`
/// §3.4).
pub(crate) fn csr_read(state: &SimState, csr: u16) -> u32 {
    if csr == CSR_MISA {
        return CSR_MISA_VALUE;
    }
    state
        .csrs
        .iter()
        .find(|(number, _)| *number == csr)
        .map(|(_, value)| *value)
        .unwrap_or(0)
}

/// Write a CSR into the generic CSR file.
///
/// * The CSR **is in the file** ⇒ the value is **overwritten**.
/// * The CSR is **not in the file** ⇒ the entry is **appended**, and
///   the file stays **ascending by number** — so the same number can
///   never appear twice.
pub(crate) fn csr_write(state: &mut SimState, csr: u16, value: u32) {
    if let Some(slot) = state.csrs.iter_mut().find(|(number, _)| *number == csr) {
        slot.1 = value;
        return;
    }
    let at = state
        .csrs
        .iter()
        .position(|(number, _)| *number > csr)
        .unwrap_or(state.csrs.len());
    state.csrs.insert(at, (csr, value));
}

/// The 12-bit CSR number of a CSR instruction encoding (bits 31:20).
///
/// The number is read out of the **raw instruction word**, not out of
/// [`riscv_sim_contract::model::SimState`]'s decoded immediate: the
/// decoder sign-extends the 12-bit field, and a CSR number above
/// `0x7FF` (the whole read-only quarter) would come out negative.
pub(crate) fn csr_number(raw: u32) -> u16 {
    ((raw >> 20) & 0xFFF) as u16
}

/// The 5-bit immediate of the immediate CSR instructions (bits 19:15).
///
/// The immediate variants of the CSR instructions reuse the `rs1`
/// field as a **zero-extended 5-bit immediate**.
pub(crate) fn csr_zimm(raw: u32) -> u32 {
    (raw >> 15) & 0x1F
}

/// Enter the machine-mode trap handler.
///
/// This is the platform's **single** trap entry (STAGE-4 §3.5): the
/// **exception** path and the **interrupt** path both come through
/// here. The steps are **fixed**, in this order:
///
/// 1. `mepc := state.pc` — the address the handler returns to;
/// 2. `mcause := cause` — the raw cause (a small number for an
///    exception, `IRQ_BASE | line` for an interrupt);
/// 3. `mtval := mtval` — the auxiliary value (the faulting address,
///    the instruction word, …);
/// 4. `mstatus`: `MPIE := MIE`, then `MIE := 0` — interrupts are off
///    for the duration of the handler;
/// 5. `state.pc := mtvec` — **direct** mode jumps to the base,
///    **vectored** mode to `base + 4 × (cause & 0x7)`.
///
/// Returns the **new PC** (the value written into [`SimState::pc`]),
/// so the caller can report it without reading the state back.
///
/// # The mode bit
///
/// The mode bit is **bit 0** of `mtvec`: `0` = direct, `1` = vectored.
/// The **base** is the `mtvec` value with its low two bits cleared, so
/// the vector table is 4-byte aligned (as the RISC-V manual requires).
/// ⚠️ RISC-V defines the mode field as bits `[1:0]` and leaves the
/// encodings above `0b01` reserved; this platform reads **bit 0**, so
/// a reserved mode encoding behaves like the direct mode.
pub fn enter_trap(state: &mut SimState, cause: u32, mtval: u32) -> u32 {
    // 1. mepc := pc — the handler returns to the faulting instruction.
    csr_write(state, CSR_MEPC, state.pc);
    // 2. mcause := cause.
    csr_write(state, CSR_MCAUSE, cause);
    // 3. mtval := mtval.
    csr_write(state, CSR_MTVAL, mtval);

    // 4. mstatus: MPIE := MIE, then MIE := 0.
    let mstatus = csr_read(state, CSR_MSTATUS);
    let mie = 1u32 << MSTATUS_MIE_BIT;
    let mpie = 1u32 << MSTATUS_MPIE_BIT;
    let saved = if mstatus & mie != 0 { mpie } else { 0 };
    csr_write(state, CSR_MSTATUS, (mstatus & !(mie | mpie)) | saved);

    // 5. pc := mtvec (direct) or base + 4 × (cause & 0x7) (vectored).
    let mtvec = csr_read(state, CSR_MTVEC);
    let base = mtvec & !0x3;
    let target = if mtvec & 0x1 != 0 {
        base.wrapping_add(4 * (cause & 0x7))
    } else {
        base
    };
    state.pc = target;
    target
}
