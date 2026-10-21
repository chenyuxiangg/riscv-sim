//! Machine-mode CSR vocabulary and the platform's interrupt-line space.
//!
//! # Why this module exists
//!
//! Three facts are needed by **more than one** module, and each of them
//! used to live in exactly one place — which is how they drifted apart:
//!
//! 1. the **CSR numbers** the ISS reads and writes (M-13), the trap
//!    handler the controller targets (M-16), and the register view the
//!    debugger shows (M-10) must all agree on what `0x342` is;
//! 2. the **line → `mie` bit** mapping — the platform's interrupt lines
//!    are numbered `1 / 2 / 3`, while `mie` uses the RISC-V bit numbers
//!    `11 / 7`. Two numbering spaces, one mapping, two consumers (M-16
//!    refreshes the enable mask, M-13 reports the pending bits);
//! 3. the **`mcause` interrupt bit** (`IRQ_BASE`), which both the
//!    controller (when it hands an interrupt over) and the ISS (when it
//!    enters the trap) must agree on.
//!
//! Putting them here makes each of them a **single landing spot**
//! (one constraint, one home) instead of two copies that can drift.
//!
//! # Scope
//!
//! This module is **vocabulary only** — numbers, bit positions and the
//! pure mapping functions. It holds **no state** and performs **no**
//! work; the controller's state lives in M-16, the CSR *file* lives in
//! [`crate::model::SimState::csrs`].
//!
//! The frozen spec for this batch is
//! `docs/detailed-design/STAGE-4_Interrupt_Controller.md` §2.1.

/// `mstatus` — global machine status.
pub const CSR_MSTATUS: u16 = 0x300;

/// `mie` — machine interrupt-enable register.
pub const CSR_MIE: u16 = 0x304;

/// `mtvec` — trap-vector base address and mode.
pub const CSR_MTVEC: u16 = 0x305;

/// `mepc` — the address the trap handler returns to.
pub const CSR_MEPC: u16 = 0x341;

/// `mcause` — the trap reason.
pub const CSR_MCAUSE: u16 = 0x342;

/// `mtval` — the trap's auxiliary value (faulting address, instruction
/// word, …).
///
/// This is the landing spot of debt ledger **D-64** ("`mtval` does not
/// exist anywhere in the platform"). It is a **CSR**, not a dedicated
/// field on [`crate::model::SimState`] — see adjudication A of the
/// frozen spec: the generic CSR file already exists, already round-trips
/// through the snapshot, and a second vocabulary would be a second
/// landing spot for the same fact.
pub const CSR_MTVAL: u16 = 0x343;

/// `mip` — machine interrupt-pending register.
///
/// **Read-only** on this platform (RISC-V: `MEIP` / `MTIP` are
/// read-only in M-mode).
pub const CSR_MIP: u16 = 0x344;

/// `mtime` — the wall-clock timer counter (RV32 read-only CSR).
pub const CSR_MTIME: u16 = 0xBFF8;

/// **`irq_clear`** — the platform's custom CSR for clearing an
/// interrupt line.
///
/// Writing a **line bitmask** (bit L = clear line L) to it clears the
/// corresponding pending entries and resets the active interrupt.
///
/// ⚠️ It is deliberately **not** `0xBFF0`: the RISC-V rule says a CSR
/// whose address has bits `[11:10] == 0b11` is **read-only**, and
/// `0xBFF0` is in that range — a write there would be silently dropped
/// and the interrupt could never be cleared. `0x7C0` has bits
/// `[11:10] == 0b01` (writable) and is not assigned by RV32.
///
/// This is a **platform-specific** CSR — it is not part of the RISC-V
/// standard. The difference is registered in the M-16 LLD §3.5.
pub const CSR_IRQ_CLEAR: u16 = 0x7C0;

/// `mstatus.MIE` — the **global** interrupt-enable bit.
///
/// Bit 3 of `mstatus` (standard).
pub const MSTATUS_MIE_BIT: u32 = 3;

/// `mstatus.MPIE` — "previous `MIE`", saved on trap entry and restored
/// by `mret`.
///
/// Bit 7 of `mstatus` (standard).
pub const MSTATUS_MPIE_BIT: u32 = 7;

/// `mie.MEIE` — machine **external** interrupt enable (bit 11).
pub const MIE_MEIE_BIT: u32 = 11;

/// `mie.MTIE` — machine **timer** interrupt enable (bit 7).
pub const MIE_MTIE_BIT: u32 = 7;

/// The interrupt bit of `mcause` — `mcause[31]`.
///
/// An interrupt cause is `IRQ_BASE | line`; an exception cause is a
/// small plain number (`0` / `2` / `3` / `4` / `6` / `11`).
pub const IRQ_BASE: u32 = 0x8000_0000;

/// Interrupt line — UART receive-ready.
///
/// Shares the `MEIE` bit with [`IRQ_UART_TX_EMPTY`]: M-mode has one
/// external-interrupt bit, so the two UART lines cannot be enabled
/// independently through `mie` (see [`mie_bit_for_line`]).
pub const IRQ_UART_RX_READY: u16 = 1;

/// Interrupt line — UART transmit-empty.
pub const IRQ_UART_TX_EMPTY: u16 = 2;

/// Interrupt line — timer compare expiry.
///
/// The **highest** priority line of the platform (LLD §2.2).
pub const IRQ_TIMER_COMPARE: u16 = 3;

/// The "no active interrupt" marker.
///
/// ⚠️ It is **not** `0`: line 0 is not a line at all, but the marker is
/// stored in a `u16` field, and `0` would read as "line 0 is active".
pub const NO_ACTIVE_IRQ: u16 = 0xFFFF;

/// The `mie` / `mip` bit that gates an interrupt line.
///
/// The platform's **line space** (`1` / `2` / `3`) and the RISC-V
/// **`mie` bit space** (`11` / `7`) are **different numbering spaces**;
/// this function is the **only** place that translates between them.
///
/// * `1` (UART rx-ready) and `2` (UART tx-empty) → bit **11** (`MEIE`)
/// * `3` (timer compare) → bit **7** (`MTIE`)
/// * anything else → `None` — the line has **no** standard `mie` bit.
///
/// ⚠️ A `None` result does **not** mean "disabled": it means "this line
/// is not gated by a standard `mie` bit". The controller keeps such a
/// line's enable bit **as it is** when it refreshes the mask — clearing
/// it would silently switch the line off.
pub fn mie_bit_for_line(line: u16) -> Option<u32> {
    match line {
        IRQ_UART_RX_READY | IRQ_UART_TX_EMPTY => Some(MIE_MEIE_BIT),
        IRQ_TIMER_COMPARE => Some(MIE_MTIE_BIT),
        _ => None,
    }
}

/// Whether a CSR address is **read-only** by the RISC-V address rule.
///
/// The rule is a single bit: **bit 11 set ⇒ read-only**. That splits the
/// 12-bit CSR space into the four standard quarters —
///
/// | address | quarter | access |
/// | --- | --- | --- |
/// | `0x000..0x3FF` | user | read / write |
/// | `0x400..0x7FF` | supervisor | read / write |
/// | `0x800..0xBFF` | user | **read-only** |
/// | `0xC00..0xFFF` | machine / debug | **read-only** |
///
/// Consequences on this platform: `mtime` (`0xBFF8`) is read-only,
/// `mip` (`0x344`) is *writable by this rule* (see
/// [`csr_write_is_ignored`] for the platform's extra rule), and the
/// platform's `irq_clear` (`0x7C0`) is writable — it sits in the
/// read-write **custom** quarter `0x7C0..0x7FF`.
pub fn is_csr_read_only(csr: u16) -> bool {
    csr & 0x800 != 0
}

/// Whether a write to a CSR is **ignored**.
///
/// That is the case when the **address rule** makes the CSR read-only,
/// **or** when the platform lists it as read-only on top of that.
///
/// ⚠️ `mip` is the platform's extra case: by the address rule it is
/// writable (`0x344 < 0x800`), but RISC-V makes `MEIP` / `MTIP`
/// **read-only in M-mode** — only the platform may raise them, so a
/// guest writing `mip` must not be able to fake a pending interrupt.
pub fn csr_write_is_ignored(csr: u16) -> bool {
    is_csr_read_only(csr) || csr == CSR_MIP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uart_lines_share_the_external_interrupt_bit() {
        assert_eq!(mie_bit_for_line(IRQ_UART_RX_READY), Some(MIE_MEIE_BIT));
        assert_eq!(mie_bit_for_line(IRQ_UART_TX_EMPTY), Some(MIE_MEIE_BIT));
    }

    #[test]
    fn the_timer_line_uses_the_timer_interrupt_bit() {
        assert_eq!(mie_bit_for_line(IRQ_TIMER_COMPARE), Some(MIE_MTIE_BIT));
    }

    #[test]
    fn an_unknown_line_has_no_standard_mie_bit() {
        assert_eq!(mie_bit_for_line(0), None);
        assert_eq!(mie_bit_for_line(99), None);
    }

    #[test]
    fn the_irq_clear_csr_is_writable() {
        // ⚠️ The whole reason `irq_clear` is not `0xBFF0`: `0xBFF0` is in
        // the read-only quarter, so a write there would be dropped and
        // the interrupt could never be cleared.
        assert!(!is_csr_read_only(CSR_IRQ_CLEAR));
        assert!(!csr_write_is_ignored(CSR_IRQ_CLEAR));
    }

    #[test]
    fn mtime_is_read_only_by_the_address_rule() {
        assert!(is_csr_read_only(CSR_MTIME));
        assert!(csr_write_is_ignored(CSR_MTIME));
    }

    #[test]
    fn mip_is_read_only_by_the_platform_rule() {
        // ⚠️ `0x344 < 0x800` ⇒ the **address rule** alone would let the
        // guest write it. RISC-V makes MEIP / MTIP read-only in M-mode,
        // so the platform adds the rule on top.
        assert!(!is_csr_read_only(CSR_MIP), "地址规则本身不拦 mip");
        assert!(csr_write_is_ignored(CSR_MIP), "平台规则必须拦 mip");
    }

    #[test]
    fn the_trap_csr_are_writable() {
        for csr in [
            CSR_MSTATUS,
            CSR_MIE,
            CSR_MTVEC,
            CSR_MEPC,
            CSR_MCAUSE,
            CSR_MTVAL,
        ] {
            assert!(!csr_write_is_ignored(csr), "csr {csr:#x} 必须可写");
        }
    }

    #[test]
    fn mtvec_is_a_legal_csr_number() {
        // ⚠️ **反例钉住**：本平台曾经按「CSR 地址低两位必须为 0」来判
        // 非法 —— 而 `mtvec` 的标准编号就是 `0x305`（低位 `01`）。
        // 那条规则会让平台**自己的向量表 CSR 变成非法指令**。
        // 这里把标准编号钉死，防止那条错误规则被加回来。
        assert_eq!(CSR_MTVEC, 0x305);
        assert_ne!(CSR_MTVEC & 0x3, 0, "mtvec 的低位本就不是 0");
        assert!(!csr_write_is_ignored(CSR_MTVEC));
    }

    #[test]
    fn the_no_active_marker_is_not_line_zero() {
        assert_ne!(NO_ACTIVE_IRQ, 0);
        assert_eq!(NO_ACTIVE_IRQ, u16::MAX);
    }
}
