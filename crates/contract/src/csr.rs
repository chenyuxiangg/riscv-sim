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

/// `misa` — machine ISA register.
///
/// ⚠️ **Read-only on this platform.**  Two different things must not be
/// confused here:
///
/// * **What RISC-V says**: `misa` is **WARL and writable** — a guest may
///   write it, and a legal write changes which extensions the
///   implementation claims to support (the reference side honours that:
///   `csrci misa, 4` really clears the `C` bit there).
/// * **What this platform does**: a guest write is **ignored** and the
///   register always reads back [`CSR_MISA_VALUE`].  The address rule
///   would make it writable (`0x301 < 0x800`) ⇒ the platform adds it to
///   the read-only set — see [`csr_is_read_only`].
///
/// ⇒ the platform's choice is **stricter than the specification**, and it
/// is a **real difference** from the reference side (the ledger records it
/// as D-95).  It is *not* "we match the reference side".
pub const CSR_MISA: u16 = 0x301;

/// `mscratch` — machine scratch register.
///
/// ⚠️ **Implemented** on this platform: it is a plain storage CSR.  The
/// upstream `rv32si` CSR test exercises twelve read-modify-write semantics
/// on it, so "unimplemented ⇒ write ignored" would silently change their
/// meaning.
pub const CSR_MSCRATCH: u16 = 0x340;

/// `misa` — the value this platform reports.
///
/// * `bits 31:30` = **MXL** = `01` ⇒ **RV32**;
/// * extension bits: **I** (bit 8) · **M** (bit 12) · **C** (bit 2).
///
/// ⚠️ **Only the extensions we actually implement are set** — no `A`
/// (atomics), no `F`/`D` (floating point), no `S`/`U` (privilege levels).
/// The IALIGN field (bits 29:26) is left **0**, exactly as the
/// reference side leaves it.
///
/// ⇒ the reference side's `misa` is **not** bit-identical to ours.
/// Measured (the reference side runs with its default `--priv=MSU`):
///
/// | | MXL | I | M | C | **S** (18) | **U** (20) | IALIGN (29:26) |
/// |---|---|---|---|---|---|---|---|
/// | reference side | 1 | 1 | 1 | 1 | **1** | **1** | 0 |
/// | this platform  | 1 | 1 | 1 | 1 | 0 | 0 | 0 |
///
/// ⇒ the difference is **exactly** the two privilege-level bits: the
/// reference side implements S-mode and U-mode, this platform does
/// not ⇒ reporting them would be a lie.  ⇒ **the MXL field and the
/// I/M/C bits are comparable across implementations**; the
/// privilege-level bits differ **by design**.
///
/// ⚠️ The reference side's `misa` is also **WARL-writable** (a guest
/// may clear bits — measured: `csrci misa, 4` really clears `C`);
/// ours is **read-only** (a guest write is ignored).  That is a real
/// semantic difference, not a "coverage equivalence" — see the ledger
/// (D-95).
pub const CSR_MISA_VALUE: u32 = (1 << 30) | (1 << 8) | (1 << 12) | (1 << 2);

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

/// `mstatus.MPP` — the privilege level the hart was in when the trap
/// was taken.
///
/// **Bits 12:11** of `mstatus` (standard).
///
/// ⚠️ The field is **WARL**: an implementation may only hold values
/// that name a privilege level it actually supports.  This platform
/// implements **M-mode only** ⇒ the only legal value is
/// [`MSTATUS_MPP_M`] ⇒ `misa` reports **no `S` and no `U` bit**, and
/// the three places that touch `MPP` (trap entry, `mret`, the guest's
/// own `csrw mstatus`) all agree on that one value.
///
/// The reference side (Spike, default `--priv=MSU`) supports `S` and
/// `U` as well ⇒ it may hold `0`/`1`/`3` ⇒ the two sides' `MPP` can
/// differ after a `mret` (Spike drops to the least-privileged
/// supported mode, i.e. `U` = 0, while this platform keeps `M` = 3).
/// That difference is a **capability** difference, not a defect — the
/// ledger records it (D-96).
pub const MSTATUS_MPP_SHIFT: u32 = 11;

/// The `mstatus.MPP` value for **machine mode** — the only privilege
/// level this platform implements.
pub const MSTATUS_MPP_M: u32 = 3;

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
/// **standard interrupt bit space** (`11` / `7`) are **different numbering
/// spaces**; this function is the **only** place that translates between
/// them.
///
/// * `1` (UART rx-ready) and `2` (UART tx-empty) → bit **11** (`MEIE` /
///   `MEIP`)
/// * `3` (timer compare) → bit **7** (`MTIE` / `MTIP`)
/// * anything else → `None` — the line has **no** standard interrupt bit.
///
/// ⚠️ A `None` result does **not** mean "disabled": it means "this line
/// is not gated by a standard interrupt bit". The controller keeps such a
/// line's enable bit **as it is** when it refreshes the mask — clearing
/// it would silently switch the line off.
///
/// ⚠️ **The same bit number serves `mie` and `mip`** — RISC-V gives
/// `MEIE`/`MEIP` and `MTIE`/`MTIP` the *same* bit numbers, so the map
/// is **one** table, not two.  (The frozen spec for this batch,
/// `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md` adjudication C,
/// renamed the function to say so: the old name spelled only `mie`, and
/// a name that serves two registers is a name with two meanings.)
pub fn std_irq_bit_for_line(line: u16) -> Option<u32> {
    match line {
        IRQ_UART_RX_READY | IRQ_UART_TX_EMPTY => Some(MIE_MEIE_BIT),
        IRQ_TIMER_COMPARE => Some(MIE_MTIE_BIT),
        _ => None,
    }
}

/// The line → standard interrupt bit map, under its **old** name.
///
/// ⚠️ **Deprecated in favour of [`std_irq_bit_for_line`]** — kept as an
/// alias so existing call sites keep working.  It means the same thing;
/// the new name simply does not pretend the map serves only `mie`.
pub fn mie_bit_for_line(line: u16) -> Option<u32> {
    std_irq_bit_for_line(line)
}

/// A set of pending interrupt lines → the `mip` bit mask that reports them.
///
/// The map is the **same** one [`std_irq_bit_for_line`] uses, because
/// `mie` and `mip` share their bit numbers per interrupt source
/// (`MEIE`/`MEIP` = 11, `MTIE`/`MTIP` = 7).
///
/// ⚠️ **Lines 1 and 2 collapse into one bit** (both are external
/// interrupts and share `MEIP`) ⇒ a guest reading `mip` **cannot tell
/// which** UART line is pending.  The line number only reaches the guest
/// at delivery time, through `mcause = IRQ_BASE | line`.
pub fn pending_lines_to_mip_mask(lines: &[u16]) -> u32 {
    let mut mask = 0u32;
    for &line in lines {
        if let Some(bit) = std_irq_bit_for_line(line) {
            mask |= 1 << bit;
        }
    }
    mask
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

/// Whether this platform **implements** a CSR.
///
/// ⚠️ **This is the table that decides what a guest write does** (see
/// [`csr_write_is_ignored`]).  The rule is a **platform rule**, chosen
/// because the platform's own golden comparison found the previous
/// behaviour ("store whatever the guest wrote") wrong:
///
/// * **unimplemented** ⇒ the write is **ignored**, and a later read
///   returns **0**;
/// * **implemented** ⇒ the write lands in the CSR file.
///
/// ⚠️ **"Unimplemented ⇒ read 0, no trap" is NOT what the reference side
/// does.**  Measured against the reference side (the golden corpus's
/// Spike, default `--priv=MSU`):
///
/// | access | reference side | this platform |
/// |---|---|---|
/// | `csrr cycle` (no Zicntr) | **illegal-instruction trap** | reads 0, no trap |
/// | `csrw mhartid, 0x123` (read-only CSR) | **illegal-instruction trap** | write ignored, reads 0 |
/// | `csrw medeleg, 0x123` → `csrr medeleg` | **0x22** (delegation is implemented there; the write is masked) | 0 |
///
/// ⇒ the platform is deliberately **more permissive** here: reading a CSR
/// the platform does not implement is *not* an error.  The difference is
/// recorded in the ledger (D-92 / D-94) — do **not** read this rule as
/// "we match the reference side".
///
/// ⚠️ The read-only quarter (`>= 0x800`) is uniformly "unimplemented" on
/// this platform: `mhartid`, `mcycle`, `minstret`, `mtime` all read **0**
/// and ignore writes.  The reference side **models** `mcycle` (it reads a
/// real counter) ⇒ that is a counter gap, not a match (D-94).
///
/// ⚠️ `medeleg` / `mideleg` are **not** in the list: delegation is an
/// S-mode feature and this platform is M-mode only ⇒ a guest write to
/// them is **ignored** and they read back 0.  The reference side (with
/// its default `--priv=MSU`) **does** implement delegation ⇒ the two
/// sides differ here (D-94).
pub fn csr_is_implemented(csr: u16) -> bool {
    matches!(
        csr,
        CSR_MSTATUS
            | CSR_MISA
            | CSR_MIE
            | CSR_MTVEC
            | CSR_MSCRATCH
            | CSR_MEPC
            | CSR_MCAUSE
            | CSR_MTVAL
            | CSR_MIP
            | CSR_IRQ_CLEAR
    )
}

/// Whether a CSR is **read-only** for the guest.
///
/// That is the case when the **address rule** makes it read-only, **or**
/// when the platform lists it as read-only on top of that (`misa`).
///
/// ⚠️ `mip` is *not* here: it is writable **by the address rule**, but
/// RISC-V makes `MEIP` / `MTIP` **read-only in M-mode** — only the
/// platform may raise them, so a guest writing `mip` must not be able to
/// fake a pending interrupt.  That is the **platform's** rule, and it
/// lives in [`csr_write_is_ignored`] with `mip`'s own line of defence.
pub fn csr_is_read_only(csr: u16) -> bool {
    is_csr_read_only(csr) || csr == CSR_MISA
}

/// Whether a write to a CSR is **ignored**.
///
/// That is the case when
///
/// * the **address rule** (or the platform's read-only list) makes the
///   CSR read-only, **or**
/// * the CSR is `mip` — by the address rule it is writable
///   (`0x344 < 0x800`), but RISC-V makes `MEIP` / `MTIP`
///   **read-only in M-mode** — only the platform may raise them, so a
///   guest writing `mip` must not be able to fake a pending interrupt,
///   **or**
/// * ⚠️ **the platform does not implement the CSR** — then the write is
///   **ignored** and a later read returns **0**.
///
/// ⚠️ The last clause is the one the golden comparison found us missing
/// (ledger **D-92**): the reference side ignores writes to CSRs it does
/// not implement (`tdata1` reads back `0`), while we used to store the
/// value and hand it straight back.  A guest could therefore *invent* a
/// register the platform does not have.
///
/// ⚠️ **This rule is about the guest's writes only.**  The platform's own
/// writes (e.g. the `mip` projection) go through `csr_write` directly and
/// are **not** filtered by this function.
pub fn csr_write_is_ignored(csr: u16) -> bool {
    csr_is_read_only(csr) || csr == CSR_MIP || !csr_is_implemented(csr)
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

    /// ⚠️ **只读规则的边界必须被钉死** —— 规则一旦被改成另一个**也说得通**
    /// 的形式（例如「高两位都是 11」），平台用到的 CSR **恰好**都判成一样，
    /// 于是**没有一条用例会红**：那种改法看上去「无害」，实际是把
    /// `0x900..0x9FF` / `0xB00..0xBFF` 这两段标准里**可写**的区间
    /// 悄悄判成只读。
    ///
    /// 这条用例把**四个区的边界**都写出来，让规则**换一种写法**也会变红。
    #[test]
    fn the_read_only_rule_is_pinned_at_the_quarter_boundaries() {
        // 下半区（`0x000..0x7FF`）一律**可写**。
        assert!(!is_csr_read_only(0x0000));
        assert!(!is_csr_read_only(0x03FF));
        assert!(!is_csr_read_only(0x0400));
        assert!(!is_csr_read_only(0x07FF));
        // 上半区（`0x800..0xFFF`）一律**只读**。
        assert!(is_csr_read_only(0x0800));
        assert!(is_csr_read_only(0x0900));
        assert!(is_csr_read_only(0x0A00));
        assert!(is_csr_read_only(0x0B00));
        assert!(is_csr_read_only(0x0C00));
        assert!(is_csr_read_only(0x0FFF));
    }

    /// ⚠️ **本平台的规则比 RISC-V 粗**，这里把差异**写死**。
    ///
    /// RISC-V 在 `0x800..0xBFF` 之内还按 bit 8 再分读写
    /// （`0x8xx` / `0xAxx` 只读，`0x9xx` / `0xBxx` 可写）；
    /// 本平台**一律**当只读（地址 bit 11 置位 ⇒ 只读）。
    ///
    /// 差异对本平台**无影响**：平台用到的 CSR 里**没有**落在这条差异上的
    /// —— `mtime`（`0xBFF8`）按 RISC-V 本来就是**只读**的。
    #[test]
    fn the_coarser_read_only_rule_does_not_affect_any_csr_the_platform_uses() {
        // 标准里「可写」而本平台判「只读」的两段里，平台**一个 CSR 都没有**。
        // ⚠️ 这里比的是**最终效果**（写会不会被忽略），不是地址规则本身 ——
        // `mip` 按地址规则是**可写**的（`0x344 < 0x800`），是**平台规则**
        // 把它判成只读的（RISC-V：MEIP/MTIP 在 M 模式只读）。
        for csr in [
            CSR_MSTATUS,
            CSR_MIE,
            CSR_MTVEC,
            CSR_MEPC,
            CSR_MCAUSE,
            CSR_MTVAL,
            CSR_MIP,
            CSR_MTIME,
            CSR_IRQ_CLEAR,
        ] {
            let standard_write_ignored = match csr {
                // `mip`：RISC-V 规定 MEIP / MTIP 在 M 模式**只读**。
                0x344 => true,
                // `mtime`：RV32 规定它是**只读** CSR。
                0xBFF8 => true,
                // 其余全在 `0x000..0x7FF`（可写区）或 `0x7C0`（自定义可写区）。
                _ => false,
            };
            assert_eq!(
                csr_write_is_ignored(csr),
                standard_write_ignored,
                "csr {csr:#x} 的写是否被忽略，必须与 RISC-V 一致"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 裁决 C —— `mie` 与 `mip` 共用同一张位号表
    // -----------------------------------------------------------------------

    /// ⚠️ **一张表，两个名字** —— `mie` 与 `mip` 的位号同源
    /// （`MEIE`/`MEIP` = 11，`MTIE`/`MTIP` = 7）。
    ///
    /// 旧名 `mie_bit_for_line` 说的是「只服务 `mie`」⇒ **名字与用途不符**
    /// ⇒ 留作别名，但新名必须与旧名**逐线相等**。
    #[test]
    fn the_standard_irq_bit_table_serves_mie_and_mip_alike() {
        for line in [IRQ_UART_RX_READY, IRQ_UART_TX_EMPTY, IRQ_TIMER_COMPARE] {
            assert_eq!(
                std_irq_bit_for_line(line),
                mie_bit_for_line(line),
                "线 {line} 的标准中断位号，两个入口必须一致"
            );
        }
        // 位号本身钉死：**换一种写法**（例如把 UART 线分到不同位）也会红。
        assert_eq!(std_irq_bit_for_line(IRQ_UART_RX_READY), Some(11));
        assert_eq!(std_irq_bit_for_line(IRQ_UART_TX_EMPTY), Some(11));
        assert_eq!(std_irq_bit_for_line(IRQ_TIMER_COMPARE), Some(7));
        assert_eq!(std_irq_bit_for_line(0), None);
    }

    /// 挂起的线 ⇒ `mip` 位掩码。
    ///
    /// ⚠️ **线 1 与线 2 折叠成同一位**（两条 UART 线共用 `MEIP`）⇒
    /// guest 读 `mip` **分辨不出**是哪条线挂起；线号只在投递时通过
    /// `mcause = IRQ_BASE | line` 到达 guest。
    ///
    /// 这条用例把「折叠」钉死 —— 若哪天给两条 UART 线分了不同的位
    /// （例如给 `MEIP` 之外再加一位），这里会红，而 guest 的行为差异
    /// 需要**另一条**用例单独钉。
    #[test]
    fn pending_lines_project_onto_the_mip_bit_mask() {
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_UART_RX_READY]),
            1 << 11,
            "UART rx 线 ⇒ MEIP 位"
        );
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_UART_TX_EMPTY]),
            1 << 11,
            "UART tx 线与 rx 线共用 MEIP 位"
        );
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_TIMER_COMPARE]),
            1 << 7,
            "定时器线 ⇒ MTIP 位"
        );
        // 两条 UART 线**折叠成一位**（不是两位）。
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_UART_RX_READY, IRQ_UART_TX_EMPTY]),
            1 << 11,
            "两条 UART 线必须折叠进同一位"
        );
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_UART_RX_READY, IRQ_TIMER_COMPARE]),
            (1 << 11) | (1 << 7),
            "两种来源同时挂起 ⇒ 两个位都置上"
        );
        assert_eq!(pending_lines_to_mip_mask(&[]), 0, "没有挂起 ⇒ 没有位");
    }

    // -----------------------------------------------------------------------
    // 裁决 D —— 「已实现 CSR 表」
    // -----------------------------------------------------------------------

    /// 已实现清单**必须恰好**是平台真的有的那批 CSR。
    ///
    /// ⚠️ 清单外的 CSR（`tdata1` / `medeleg` / …）写必须被忽略、读回 0
    /// —— 与参考侧一致（黄金对拍实测发现：参考侧写 `tdata1` 读回 0）。
    #[test]
    fn the_implemented_csr_table_is_exactly_the_platforms_set() {
        for csr in [
            CSR_MSTATUS,
            CSR_MISA,
            CSR_MIE,
            CSR_MTVEC,
            CSR_MSCRATCH,
            CSR_MEPC,
            CSR_MCAUSE,
            CSR_MTVAL,
            CSR_MIP,
            CSR_IRQ_CLEAR,
        ] {
            assert!(csr_is_implemented(csr), "csr {csr:#x} 必须算已实现");
        }
        // ⚠️ 明确**不在**清单里的那几个 —— 每一个都有理由。
        for (csr, why) in [
            (0x7A1u16, "tdata1 —— 调试器 CSR，本平台没有"),
            (0x7A2u16, "tdata2 —— 调试器 CSR，本平台没有"),
            (0x302u16, "medeleg —— S 模式委派，本平台只有 M 模式"),
            (0x303u16, "mideleg —— S 模式委派，本平台只有 M 模式"),
            (0xF14u16, "mhartid —— 计数器，本平台没实现"),
        ] {
            assert!(
                !csr_is_implemented(csr),
                "csr {csr:#x} 不该算已实现（{why}）"
            );
        }
    }

    /// 未实现的 CSR：**写被忽略** ⇒ 读回 0。
    ///
    /// ⚠️ 这是黄金对拍查出来的缺陷（D-92）：平台原来把写**存了**，
    /// 于是 guest 能**凭空造出**一个平台没有的寄存器。
    #[test]
    fn an_unimplemented_csr_ignores_the_write() {
        for csr in [0x7A1u16, 0x7A2, 0x302, 0x303, 0xF14] {
            assert!(
                csr_write_is_ignored(csr),
                "csr {csr:#x} 未实现 ⇒ 写必须被忽略"
            );
        }
        // 已实现的那批仍然可写（否则这条规则就过头了）。
        for csr in [CSR_MSTATUS, CSR_MIE, CSR_MTVEC, CSR_MSCRATCH, CSR_MEPC] {
            assert!(
                !csr_write_is_ignored(csr),
                "csr {csr:#x} 已实现 ⇒ 写不能被忽略"
            );
        }
    }

    // -----------------------------------------------------------------------
    // 裁决 E —— `misa`
    // -----------------------------------------------------------------------

    /// `misa` 对 guest **只读** —— 地址规则（`0x301 < 0x800`）本来是可写
    /// 的，是平台**额外**加的只读。
    #[test]
    fn misa_is_read_only_for_the_guest() {
        assert!(!is_csr_read_only(CSR_MISA), "地址规则本身不拦 misa");
        assert!(csr_is_read_only(CSR_MISA), "平台规则必须拦 misa");
        assert!(csr_write_is_ignored(CSR_MISA), "misa 的写必须被忽略");
    }

    /// `misa` 报的内容：**MXL = RV32**，加上平台**真的实现了**的扩展位。
    ///
    /// ⚠️ **只填已实现的扩展** —— 填了没实现的扩展，guest 就会以为平台
    /// 支持它，然后去用一条平台不支持的指令。
    ///
    /// ⚠️ **不填 bit29（IALIGN = 32）**：参考侧填了，本平台不填 ⇒ 两侧
    /// 的 `misa` **逐位不等**，只有 MXL 字段可比。这个差异已在 STAGE-6
    /// §0.1 的实测基线里诚实标注。
    #[test]
    fn misa_reports_rv32_and_only_the_implemented_extensions() {
        // MXL = bits 31:30 = 01 ⇒ RV32。
        assert_eq!(CSR_MISA_VALUE >> 30, 0b01, "MXL 必须是 RV32");
        // 已实现：I（bit 8）、M（bit 12）、C（bit 2）。
        assert_ne!(CSR_MISA_VALUE & (1 << 8), 0, "I 扩展位");
        assert_ne!(CSR_MISA_VALUE & (1 << 12), 0, "M 扩展位");
        assert_ne!(CSR_MISA_VALUE & (1 << 2), 0, "C 扩展位");
        // 未实现 ⇒ 位必须**清零**。
        for (bit, name) in [
            (0u32, "A（原子）"),
            (3, "D（双精度浮点）"),
            (5, "F（单精度浮点）"),
            (18, "S（监管者模式）"),
            (20, "U（用户模式）"),
            (29, "IALIGN = 32"),
        ] {
            assert_eq!(
                CSR_MISA_VALUE & (1 << bit),
                0,
                "{name} 位（bit {bit}）必须清零 —— 平台没实现它"
            );
        }
    }
}
