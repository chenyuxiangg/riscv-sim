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
//! public API grows by exactly **two** symbols — [`enter_trap`] and
//! [`mepc_aligned`].
//!
//! # The WARL rules
//!
//! `mstatus` 是 **WARL**（写一次读回）：它的**保留位**写被忽略、读回
//! **0**（台账 **D-98**）—— 掩码常量住在契约层
//! （[`MSTATUS_WARL_MASK`]），M-13 只**引用**它（「一个约束一个落点」）。
//!
//! ⚠️ **`mepc` 的三条路径各管各的**（参考侧实测钉死 · v0.22 批）：
//! ① **用户写** `csrw mepc` ⇒ **写侧按 IALIGN 掩码**（`~1`；IALIGN = 16
//!    ⇒ 只清最低 1 位）—— 参考侧实测：写 `0x1003` 读回 **`0x1002`**；
//! ② **陷入门**写 `mepc` ⇒ 写**精确的 PC**、**不丢 bit 1**（走
//!    [`csr_write_raw`]）—— 陷入必须能回到**出错的指令**（压缩指令可能
//!    落在 2 字节对齐的地址上）；
//! ③ **`mret` 取 PC** ⇒ 再掩一次 `~1`（[`mepc_aligned`]）⇒ **幂等**。
//!
//! ⚠️ **①② 在合法指令地址上不可观测**（每条合法地址都是偶数，`~1` 的
//! 掩码动不了它）⇒ 参考侧在陷入门那条路径上究竟写精确 PC 还是写被掩的
//! PC **无法证明**；平台用原始写只是**把意图显式写下来**。

use riscv_sim_contract::csr::{
    CSR_MCAUSE, CSR_MEPC, CSR_MISA, CSR_MISA_VALUE, CSR_MSTATUS, CSR_MTVAL, CSR_MTVEC,
    MEPC_ALIGN_MASK, MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT, MSTATUS_MPP_M, MSTATUS_MPP_SHIFT,
    MSTATUS_WARL_MASK,
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
/// * The CSR is **not in the file** ⇒ the entry is **appended**, and the
///   file stays **ascending by number** — so the same number can
///   never appear twice.
///
/// # The WARL rules
///
/// The value first goes through [`apply_warl`]:
///
/// * `mstatus` ⇒ the **reserved** bits are dropped（台账 **D-98**）—— guest
///   写 `0xFFFFFFFF` 读回的**保留位是 0**。
///
/// ⚠️ **`mepc` 也在这一类里**：**用户写**按 **IALIGN** 掩码（`~1`）——
/// 参考侧实测：写 `0x1003` 读回 **`0x1002`**。
/// ⚠️ **陷入门不走这条路**（它走 [`csr_write_raw`]，写**精确的 PC**、
/// 不丢 bit 1）—— 陷入必须能回到**出错的指令**；
/// `mret` 取 PC 走 [`mepc_aligned`]。
///
/// ⚠️ **平台的内部写走 [`csr_write_raw`]** —— 陷阱入口要把**精确的
/// PC** 写进 `mepc`（压缩指令可能是 **2 字节对齐**的）⇒ 这一步显式地
/// 「不掩码」，不让它依赖调用方的记性。
pub(crate) fn csr_write(state: &mut SimState, csr: u16, value: u32) {
    csr_write_raw(state, csr, apply_warl(csr, value));
}

/// Write a CSR into the generic CSR file **without** the WARL rules.
///
/// 这是**平台自己**的写（陷阱入口写 `mepc` / `mcause` / `mtval` /
/// `mstatus`）—— 它们写的是**架构定义过的位**，不需要掩码；而
/// `mepc` 还要保住**精确的 PC**（见 [`csr_write`] 的说明）。
pub(crate) fn csr_write_raw(state: &mut SimState, csr: u16, value: u32) {
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

/// The **WARL**（写一次读回）规则：写进去的值先过一遍掩码。
///
/// ⚠️ 掩码常量在**契约层**（[`MSTATUS_WARL_MASK`]）—— 这里只**引用**，
/// **不**另写一份字面量。
fn apply_warl(csr: u16, value: u32) -> u32 {
    match csr {
        CSR_MSTATUS => value & MSTATUS_WARL_MASK,
        // ⚠️ **`mepc` 的用户写被 IALIGN 掩码** —— 与参考侧逐位一致
        // （实测：`csrw mepc, 0x1003` 读回 **0x1002**，即掩 `~1`）。
        //
        // ⚠️ **但陷入门不走这条路** —— [`enter_trap`] 用
        // [`csr_write_raw`] 写**精确的 PC**。两条路径的差别在**合法
        // 地址上不可观测**（每条合法指令地址都是偶数，`~1` 动不了它），
        // 但用原始写把「陷入门要回到出错的那条指令」这件事**显式**写下来。
        CSR_MEPC => value & MEPC_ALIGN_MASK,
        _ => value,
    }
}

/// The `mret` **jump target**: `mepc` masked by **IALIGN**.
///
/// 特权规范：`mret` 跳到 `mepc`，取 PC 时按 **IALIGN** 掩码。
///
/// ⚠️ **本平台实现了 C 扩展**（`misa` 的 C 位 = 1）⇒ **IALIGN = 16**
/// ⇒ **只清最低 1 位**（[`MEPC_ALIGN_MASK`] = `~0x1`）。
///
/// ⚠️ **台账 D-93 原文写的「`mepc[1:0]` 当作零」是 IALIGN = 32（没有 C
/// 扩展）下的口径，对不上本平台** —— 参考侧实测（同一个探针 ELF 两侧
/// 跑）：压缩指令 `c.sw` 在 `0x8000014a` 上陷入 ⇒ 参考侧 `mepc =
/// 0x8000014a`（**bit 1 保留**）；两个压缩 store 分别在 `4k` 与
/// `4k+2`，第一次陷入后 handler `mret` ⇒ 参考侧**落回 `4k+2`**
/// ⇒ 掩码是 `~1`。
///
/// # The call site **is** wired
///
/// The `mret` **arm** lives in `m13_iss/executor.rs` and calls
/// [`mepc_aligned`]（**不再**裸读 `CSR_MEPC`）—— v0.22 批由统筹方接上。
///
/// ⇒ 端到端判别式成立：`mepc` 带 bit 1 ⇒ `mret` 之后 PC 的最低 1 位
/// 被清掉（IALIGN = 16 ⇒ 掩 `~1`，**不是** `~3`）。
pub fn mepc_aligned(state: &SimState) -> u32 {
    csr_read(state, CSR_MEPC) & MEPC_ALIGN_MASK
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
///    for the duration of the handler; and `MPP := M` — the privilege
///    level the trap was taken from;
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
    //
    // ⚠️ **原始写**（不过 WARL 掩码）：压缩指令可能是 **2 字节对齐**的
    // ⇒ 掩掉低两位会让「从陷阱返回」落到指令中间（见 [`csr_write`]）。
    csr_write_raw(state, CSR_MEPC, state.pc);
    // 2. mcause := cause.
    csr_write(state, CSR_MCAUSE, cause);
    // 3. mtval := mtval.
    csr_write(state, CSR_MTVAL, mtval);

    // 4. mstatus: MPIE := MIE, then MIE := 0, then MPP := M.
    //
    // ⚠️ `MPP := M` is **not** cosmetic: the reference side sets it too
    // (measured — Spike reports `mstatus = 0x0018_0080` inside a trap
    // handler, this platform used to report `0x0000_0080`), and the
    // M-13 LLD §5.2.4 says so explicitly ("MPIE←MIE，MIE←0，MPP←M").
    // `MPP` is **WARL** and this platform implements M-mode only ⇒ `M`
    // is the only legal value ⇒ the field is written, not masked.
    let mstatus = csr_read(state, CSR_MSTATUS);
    let mie = 1u32 << MSTATUS_MIE_BIT;
    let mpie = 1u32 << MSTATUS_MPIE_BIT;
    let mpp = MSTATUS_MPP_M << MSTATUS_MPP_SHIFT;
    let saved = if mstatus & mie != 0 { mpie } else { 0 };
    csr_write(
        state,
        CSR_MSTATUS,
        (mstatus & !(mie | mpie | mpp)) | saved | mpp,
    );

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

// ---------------------------------------------------------------------------
// The WARL rules (台账 D-93 / D-98)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m13_iss::tests_common::{csr_value, enc_i, Harness, SYSTEM};

    /// `CSRRW` 的编码（`funct3 == 0b001`）。
    fn csrrw(csr: u16, rs1: u8) -> u32 {
        enc_i(csr as i32, rs1, 0b001, 0, SYSTEM)
    }

    /// 一个只设了**一个寄存器**的初态。
    fn state_with_gpr(index: usize, value: u32) -> SimState {
        let mut gpr = [0u32; 32];
        gpr[index] = value;
        SimState {
            gpr,
            ..SimState::default()
        }
    }

    /// **`mstatus` 的保留位写被忽略**（台账 **D-98**）。
    ///
    /// 判别式用的是**架构字面量**（`0x15` / `0x7F80_0000`），**不是**
    /// `MSTATUS_WARL_MASK` 自己 ⇒ 把掩码改坏（或整个去掉）这条会红。
    #[test]
    fn mstatus_reserved_bits_are_dropped_on_write() {
        let mut h = Harness::new();
        let mut state = state_with_gpr(1, 0xFFFF_FFFF);
        h.step_one_at(&mut state, 0x2000, csrrw(CSR_MSTATUS, 1));
        let mstatus = csr_value(&state, CSR_MSTATUS);
        // 保留位（WPRI）：0 / 2 / 4。
        assert_eq!(
            mstatus & 0x0000_0015,
            0,
            "mstatus 的保留位（0/2/4）写被忽略 ⇒ 读回必须是 0，实际 {mstatus:#010x}"
        );
        // 保留位：23 ~ 30。
        assert_eq!(
            mstatus & 0x7F80_0000,
            0,
            "mstatus 的保留位（23~30）写被忽略 ⇒ 读回必须是 0，实际 {mstatus:#010x}"
        );
        // ⚠️ **已定义的字段必须活下来** —— 只断言「保留位是 0」的话，一个
        // 「把整个 `mstatus` 清零」的错法也能让那条断言变绿。
        assert_ne!(mstatus & 0x0000_0008, 0, "MIE（bit 3）必须活下来");
        assert_ne!(mstatus & 0x0000_0080, 0, "MPIE（bit 7）必须活下来");
        assert_eq!(
            mstatus & 0x0000_1800,
            0x0000_1800,
            "MPP（bits 11:12）必须活下来"
        );
    }

    /// **`csrr mepc` 读回的是被 IALIGN 掩过的**值**。
    ///
    /// ⚠️ **用户写会掩码**（`~1`；IALIGN = 16 ⇒ 只清最低 1 位）—— 参考侧
    /// 实测：写 `0x1003` 读回 **`0x1002`**。
    /// ⚠️ **陷入门不掩码**：它走 `csr_write_raw` 写**精确的 PC**（压缩
    /// 指令可能落在 2 字节对齐的地址上）⇒ 那条路径由
    /// `the_trap_entry_writes_the_exact_faulting_pc_into_mepc` 单独钉住。
    ///
    /// 判别式用**架构字面量**（`0x1232`），不是从掩码推出来的值。
    #[test]
    fn mepc_reads_back_the_exact_value_that_was_written() {
        let mut h = Harness::new();
        let mut state = state_with_gpr(1, 0x1232);
        h.step_one_at(&mut state, 0x2000, csrrw(CSR_MEPC, 1));
        assert_eq!(
            csr_value(&state, CSR_MEPC),
            0x1232,
            "mepc 必须读回写入的精确值（bit 1 保留 —— 压缩指令是 2 字节对齐的）"
        );
    }

    /// [`mepc_aligned`] 是「`mret` 取 PC 时按 IALIGN 掩码」这条规则的
    /// **唯一落点**。
    ///
    /// ⚠️ **IALIGN = 16**（本平台实现了 C 扩展）⇒ **只清最低 1 位**，
    /// **bit 1 必须保留**。
    #[test]
    fn mepc_aligned_clears_only_the_lowest_bit() {
        let mut state = SimState::default();
        csr_write_raw(&mut state, CSR_MEPC, 0x3003);
        assert_eq!(
            mepc_aligned(&state),
            0x3002,
            "mret 的目标必须只清最低 1 位（IALIGN = 16）"
        );
    }

    /// **陷阱入口写的是精确的 PC** —— 压缩指令可能是 **2 字节对齐**的。
    ///
    /// ⚠️ 这条与参考侧的实测一致：压缩指令 `c.sw` 在 `0x8000014a` 上陷入
    /// ⇒ 参考侧 `mepc = 0x8000014a`（**bit 1 保留**）⇒ 陷入门**不掩码**。
    ///
    /// ⚠️ 陷阱入口走的是**原始写**（[`csr_write_raw`]）—— 这一步显式地
    /// 「不掩码」，不让它依赖调用方的记性。
    #[test]
    fn the_trap_entry_keeps_the_exact_faulting_address() {
        // 一个 2 字节对齐的地址（压缩指令所在处）。
        let mut state = SimState {
            pc: 0x1232,
            ..SimState::default()
        };
        enter_trap(&mut state, 2, 0);
        assert_eq!(
            csr_value(&state, CSR_MEPC),
            0x1232,
            "陷阱入口必须把**原样的** PC 写进 mepc（压缩指令是 2 字节对齐的）"
        );
    }
}
