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
//! （契约层的 `MSTATUS_WARL_MASK`），M-13 只**引用**它（「一个约束一个落点」）。
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
    counter_shadow, mstatus_warl, COUNTEREN_CY, COUNTEREN_IR, COUNTEREN_TM, CSR_CYCLE, CSR_CYCLEH,
    CSR_INSTRET, CSR_INSTRETH, CSR_MCAUSE, CSR_MCOUNTEREN, CSR_MEDELEG, CSR_MEPC, CSR_MHARTID,
    CSR_MHARTID_VALUE, CSR_MIDELEG, CSR_MISA, CSR_MISA_VALUE, CSR_MSTATUS, CSR_MTVAL, CSR_MTVEC,
    CSR_PRIV, CSR_SATP, CSR_SCAUSE, CSR_SCOUNTEREN, CSR_SEPC, CSR_SIE, CSR_SIP, CSR_SSCRATCH,
    CSR_SSTATUS, CSR_STVAL, CSR_STVEC, CSR_TIME, CSR_TIMEH, CSR_UCOUNTEREN, IRQ_BASE,
    MEPC_ALIGN_MASK, MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT, MSTATUS_MPP_SHIFT, MSTATUS_SIE_BIT,
    MSTATUS_SPIE_BIT, MSTATUS_SPP_BIT, PRIV_M, PRIV_S, PRIV_U, SSTATUS_VIEW_MASK,
    SSTATUS_WARL_MASK,
};
use riscv_sim_contract::model::SimState;

/// Read a CSR out of the generic CSR file.
///
/// A CSR that is **not in the file** reads as **0** — it does **not**
/// trap (STAGE-4 §2.2: "读：找不到 ⇒ 返回 0（不是陷阱）").
///
/// ⚠️ **Two CSRs are answered by the platform itself**, not by the file:
///
/// * **`misa`** — it is **not** stored in the file but **computed**
///   from the platform's constant ([`CSR_MISA_VALUE`]), because it
///   reports what the platform *is* rather than what the guest last
///   wrote.  Reading it must answer the same on every read — a guest
///   that could write `misa` could otherwise claim an extension the
///   platform does not have.
///
/// * **`mhartid`** — same reason, different direction: the platform is
///   **single-core** ⇒ the answer is the constant
///   [`CSR_MHARTID_VALUE`] (= 0).  ⚠️ **It must be answered by
///   design**, not by the "not in the file ⇒ 0" accident: `mhartid` is
///   read-only, so the file never legitimately holds it — a file that
///   *did* hold a value (a restored session payload, say) must still
///   read back as the platform's hart id, never as the file's content.
///
/// ⚠️ Keeping `misa` out of the file also leaves the CSR file **empty**
/// for a guest that has touched no CSR — which is what the session
/// snapshot's emptiness predicate (`EngineSnapshotDto::is_empty`: "no
/// registers written, **no CSRs**, no pending writes, clock at zero")
/// reads. A pre-seeded `misa` entry would have broken that (see the
/// frozen spec for this batch,
/// `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md` §3.4).
///
/// # The Zicntr **shadows**
///
/// ⚠️ **四个影子 CSR 是**推导量**，不是第二份存储**（STAGE-20 裁决 D）——
///
/// | 影子 | 它报告谁 |
/// | --- | --- |
/// | `cycle` | `mcycle` |
/// | `cycleh` | `mcycleh` |
/// | `instret` | `minstret` |
/// | `instreth` | `minstreth` |
///
/// ⇒ ⇒ 读**影子** = 读**那个机器态计数器在 CSR 文件里的条目** ⇒ ⇒
///
/// * 快照里**不会出现两份值** ⇒ ⇒ 不存在「改一个不改另一个」的不同步；
/// * 影子**不进**文件 ⇒ ⇒ 「文件里有 `cycle` 条目」这件事**结构上不可能**。
///
/// ⚠️ **影子指向只有契约层那一份**（[`counter_shadow`]）—— 这里**只引用**，
/// **不**自己写一份 `match`（STAGE-20 裁决 C）。
///
/// ⚠️ **`time` / `timeh` 恒 0**（STAGE-20 裁决 K / R10）—— 参考侧也恒 0
/// ⇒ ⇒ 逐位一致 ⇒ 不影响黄金对拍。⚠️ **但功能确实缺失**：真值要接 M-15 的
/// CLINT ⇒ ⇒ 那是**单独登记的欠账**，**不是**本批的活。
///
/// ⚠️ **写路径不需要特判**：影子被**只读规则**拦住 ⇒ ⇒ 写不进文件 ⇒ ⇒
/// [`counter_shadow`] 只在读路径用。
pub(crate) fn csr_read(state: &SimState, csr: u16) -> u32 {
    if csr == CSR_MISA {
        return CSR_MISA_VALUE;
    }
    if csr == CSR_MHARTID {
        return CSR_MHARTID_VALUE;
    }
    // ⚠️ **Zicntr 的影子**（裁决 D）—— 读它跟随的那个机器态计数器。
    if let Some(source) = counter_shadow(csr) {
        return file_value(state, source);
    }
    // ⚠️ **`time` / `timeh` 恒 0**（裁决 K）—— **平台自己**回答，不是
    // 「文件里没有 ⇒ 读回 0」那条**巧合**（同 [`CSR_MHARTID`] 的理由：
    // 一份畸形载荷把 `time` 塞进文件时，读回来**仍然**必须是 0）。
    if csr == CSR_TIME || csr == CSR_TIMEH {
        return 0;
    }
    // ⚠️ **伪 CSR `CSR_PRIV`**（STAGE-22 §2.5）—— 客体的读**必须**是 **0**
    //    ⇒ ⇒ 平台自己读特权级**不走这条路**（走 [`priv_get`]）⇒ ⇒ 两个
    //    落点、两条路径，互不干扰。
    if csr == CSR_PRIV {
        return 0;
    }
    // ⚠️ **`sstatus` 是 `mstatus` 的**位视图**，不是第二份存储（**F30**）：
    //    M 模式写 `sstatus`（`0x22`）⇒ S 模式读回 `0x22` ⇒ ⇒ 同一个字段。
    //
    //    ⚠️ 视图掩码 `0x122`（`SIE` | `SPIE` | `SPP`）**不含 `SD`**（**F29**）。
    if csr == CSR_SSTATUS {
        return file_value(state, CSR_MSTATUS) & SSTATUS_VIEW_MASK;
    }
    // ⚠️ **`sie` / `sip` 受 `mideleg` 门控**（**F18**）。
    //
    //    ⚠️ **读侧同形是本平台的明示取舍**：**F18 只测了写侧**（`mideleg`
    //    没置位 ⇒ 写 `0x2` 读回 0）⇒ ⇒ 读侧按**同一个掩码**处理，理由是
    //    「能写进去的位才读得出来」⇒ ⇒ 不引入第二套口径。
    if csr == CSR_SIE || csr == CSR_SIP {
        return file_value(state, csr) & file_value(state, CSR_MIDELEG);
    }
    file_value(state, csr)
}

/// The CSR file's raw content for `csr` — **0** when it is not in the file.
///
/// ⚠️ 这是「文件查找」这**一条**路径的唯一实现 —— [`csr_read`] 的三条
/// 特判（`misa` / `mhartid` / 影子）**最终都落到这里** ⇒ ⇒ 「文件里有
/// 什么」**只有一处答案**。
fn file_value(state: &SimState, csr: u16) -> u32 {
    state
        .csrs
        .iter()
        .find(|(number, _)| *number == csr)
        .map(|(_, value)| *value)
        .unwrap_or(0)
}

/// The CSR file's entry for `csr` —— **`None`** when it is not in the file.
///
/// ⚠️ **为什么需要它**：「文件里没有这一条」与「文件里这一条的值是 **0**」
/// 是**两件事** ⇒ ⇒ 「读回 0」那条路径分不出来 ⇒ ⇒ 伪 CSR `CSR_PRIV` 的
/// 读值（**U 模式 = 0**！）**必须**用这一条。
fn file_entry(state: &SimState, csr: u16) -> Option<u32> {
    state
        .csrs
        .iter()
        .find(|(number, _)| *number == csr)
        .map(|(_, value)| *value)
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
    // ⚠️ **PMP 的 CSR 有自己的写规则**（合法化 + 锁）⇒ ⇒ 由 PMP 模块接管
    //    （STAGE-21 裁决 E / F13）⇒ ⇒ 它们**不**走 [`apply_warl`]。
    if let Some(value) = super::pmp::legalize_write(state, csr, value) {
        csr_write_raw(state, csr, value);
        return;
    }
    // ⚠️ **`sstatus` 不是第二份存储**（**F30**）—— 客体的写落到 `mstatus`
    //    的对应位上 ⇒ ⇒ 同一个字段。
    //
    //    ⚠️ **写掩码是客体的那一份**（`0x22` ⇒ 只有 `SIE` 与 `SPIE`，
    //    **F8**）⇒ ⇒ **`SPP` 写不进去**（参考侧实测：`sstatus.SPP` 恒 0）。
    //
    //    ⚠️ **旧值取自 `mstatus` 文件条目**（不是 `sstatus` 的读视图）⇒ ⇒
    //    客体的一次 `csrs sstatus, x` **不会**把平台自己写进去的 `SPP`
    //    顺手清掉 ⇒ ⇒ 「客体写不到它」与「平台写得到它」两件事分开。
    if csr == CSR_SSTATUS {
        let old = file_value(state, CSR_MSTATUS);
        let updated = (old & !SSTATUS_WARL_MASK) | (value & SSTATUS_WARL_MASK);
        csr_write(state, CSR_MSTATUS, updated);
        return;
    }
    let value = apply_warl(state, csr, value);
    csr_write_raw(state, csr, value);
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

/// The **WARL**（写一次读回）规则：写进去的值先过一遍合法化。
///
/// ⚠️ 掩码常量在**契约层**（`MSTATUS_WARL_MASK` / [`SSTATUS_WARL_MASK`] /
/// [`MEPC_ALIGN_MASK`]）—— 这里只**引用**，**不**另写一份字面量。
///
/// ⚠️ **`mstatus` 走的是**函数**[`mstatus_warl`] 而不是掩码**（STAGE-22
/// 裁决 C）：掩码表达不了「`MPP` 写保留值 2 ⇒ **该字段**保留旧值、同一次
/// 写里的其它位照常落进去」（**F33**）⇒ ⇒ 「整次丢弃」会把 `SIE` 也吞掉。
fn apply_warl(state: &SimState, csr: u16, value: u32) -> u32 {
    match csr {
        // ⚠️ **`mstatus` 的 WARL 是函数**（`MPP` 的保留值语义）—— 见
        //    [`mstatus_warl`]。
        CSR_MSTATUS => mstatus_warl(file_value(state, CSR_MSTATUS), value),
        // ⚠️ **`mepc` / `sepc` 的用户写被 IALIGN 掩码** —— 与参考侧逐位一致
        // （实测：`csrw mepc, 0x1003` 读回 **0x1002**，即掩 `~1`；`sepc` 的
        // bit 0 恒 **0**，**F15**）。
        //
        // ⚠️ **但陷入门不走这条路** —— [`enter_trap`] 用
        // [`csr_write_raw`] 写**精确的 PC**。两条路径的差别在**合法
        // 地址上不可观测**（每条合法指令地址都是偶数，`~1` 动不了它），
        // 但用原始写把「陷入门要回到出错的那条指令」这件事**显式**写下来。
        CSR_MEPC | CSR_SEPC => value & MEPC_ALIGN_MASK,
        // ⚠️ **`mtvec` / `stvec` 的 MODE 字段是 1 位 WARL**（**F13**）：
        // 写 **2** ⇒ 读 **0**；写 **3** ⇒ 读 **1** ⇒ ⇒ 合法化 =
        // `(value & !0x3) | (value & 0x1)`。
        //
        // ⚠️ **M 模式语料零回归**：上游 env 写的 `mtvec` 是 **4 字节
        // 对齐**的陷阱向量地址 ⇒ bit 0 = bit 1 = 0 ⇒ 合法化是**恒等**。
        CSR_MTVEC | CSR_STVEC => (value & !0x3) | (value & 0x1),
        // ⚠️ **`sie` / `sip` 的写受 `mideleg` 门控**（**F18**）—— `mideleg`
        // 里没有的位**写不进去**。
        CSR_SIE | CSR_SIP => value & file_value(state, CSR_MIDELEG),
        // ⚠️ **`satp` 只支持 Bare** ⇒ ⇒ MODE 字段恒 **0** ⇒ ⇒ 写进去的值
        // **恒为 0**（写 `0x1` 读回 `0`，验收 **B-13**）。
        //
        // ⚠️ 参考侧接受 Sv32 ⇒ ⇒ 两侧的 `satp` **不可比** ⇒ ⇒ 不进黄金
        // 语料（STAGE-22 §10 第 1 条）。
        CSR_SATP => 0,
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

/// The `sret` **jump target**: `sepc` masked by **IALIGN**.
///
/// ⚠️ 与 [`mepc_aligned`] **同口径**（同一个掩码常量）⇒ ⇒ 两个向量表
/// **同规则** ⇒ ⇒ 不会「一个清两位、一个清一位」。
pub fn sepc_aligned(state: &SimState) -> u32 {
    csr_read(state, CSR_SEPC) & MEPC_ALIGN_MASK
}

// ---------------------------------------------------------------------------
// The privilege level (STAGE-22 · v0.38)
// ---------------------------------------------------------------------------

/// The current **privilege level** —— `M` / `S` / `U`。
///
/// ⚠️ **它不住在 `SimState` 的字段里** —— 它住在 CSR 文件里的**伪 CSR**
/// [`CSR_PRIV`] 那一条 ⇒ ⇒ 快照天然一致 ⇒ ⇒ **不升**
/// `SIM_STATE_SCHEMA_VERSION`（STAGE-22 裁决 E）。
///
/// ⚠️ **读它不走 [`csr_read`]** —— 客体的读**必须**是 0（§2.5）⇒ ⇒ 平台
/// 自己读**直接读文件**（[`file_value`]）⇒ ⇒ 两个落点、两条路径，互不干扰。
///
/// ⚠️ **文件里没有这一条 ⇒ 复位即机器态**（[`PRIV_M`]）。
pub fn priv_get(state: &SimState) -> u32 {
    match file_entry(state, CSR_PRIV) {
        // ⚠️ **文件里没有这一条 ⇒ 复位即机器态**。
        //
        // ⚠️ **必须用 [`file_entry`] 而不是 [`file_value`]** —— 「没有这一条」
        // 与「这一条的值是 0」是**两件事**（**0 = U 模式**！）⇒ ⇒ 用
        // `file_value` 会把「复位」读成「U 模式」。
        None => PRIV_M,
        Some(PRIV_U) => PRIV_U,
        Some(PRIV_S) => PRIV_S,
        Some(PRIV_M) => PRIV_M,
        // ⚠️ **文件里有一个非法值**（畸形载荷）⇒ ⇒ 退回**机器态**（最保守
        // 的那个级别）⇒ ⇒ 不会因为一条坏条目把 hart 降到 U 模式。
        Some(_) => PRIV_M,
    }
}

/// Set the current **privilege level**。
///
/// ⚠️ **只在级别真的变了的时候才写** —— 复位后一直待在 M 模式的话，CSR
/// 文件保持**空** ⇒ ⇒ `m12_engine` 的快照「空」判据
/// （`EngineSnapshotDto::is_empty`）**不被破坏**。
///
/// ⚠️ **非法级别被忽略** —— 特权级只有三个合法取值（[`PRIV_U`] /
/// [`PRIV_S`] / [`PRIV_M`]），与 `mstatus.MPP` 的编码**同构** ⇒ ⇒ 一份
/// 字面量。
pub fn priv_set(state: &mut SimState, level: u32) {
    if !matches!(level, PRIV_U | PRIV_S | PRIV_M) {
        return;
    }
    if priv_get(state) == level {
        return;
    }
    csr_write_raw(state, CSR_PRIV, level);
}

/// The **platform's own** write to `mstatus`'s bits.
///
/// ⚠️ 它**绕过客体的 `sstatus` 掩码**（`0x22`）⇒ ⇒ 陷阱入口与 `sret`
/// **能**写 `SPP`（bit 8）与 `SPIE` ⇒ ⇒ 那就是 **F28**（委托陷阱之后
/// `sstatus.SPP` = **1** ⇒ `sret` 回到 **S 模式**）的实现方式。
///
/// ⚠️ **客体的写走 [`csr_write`]** —— 它被 `0x22` 约束 ⇒ ⇒ 「平台写得到
/// 它、客体写不到它」这两件事**分处两条路径** ⇒ ⇒ 不会互相覆盖。
pub(crate) fn mstatus_bits(state: &mut SimState, clear: u32, set: u32) {
    let old = file_value(state, CSR_MSTATUS);
    csr_write_raw(state, CSR_MSTATUS, (old & !clear) | set);
}

// ---------------------------------------------------------------------------
// The per-level accessibility table (STAGE-22 裁决 H)
// ---------------------------------------------------------------------------

/// The family a CSR belongs to —— **按级可访问性**的查表键。
///
/// ⚠️ **不是**「特权级 ≤ 当前级」的粗判（**F11**：M 模式**可以**读 S 模式
/// CSR ⇒ ⇒ 特权级的方向是**单向**的）⇒ ⇒ 必须**逐族**写死。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CsrFamily {
    /// Machine-level CSR —— **只有 M 模式**能访问。
    Machine,
    /// Supervisor-level CSR —— **M 与 S** 模式能访问；**U 模式 ⇒ cause 2**。
    Supervisor,
    /// User-level CSR —— 三级都能访问。
    User,
    /// 用户态计数器 —— 三级都能访问，**但** S / U 模式要过**使能链**。
    Counter,
    /// 伪 CSR（`CSR_PRIV`）—— 客体的读是 **0**、客体的写被**忽略** ⇒ ⇒
    /// **不是**非法指令。
    Pseudo,
}

/// Which family does `csr` belong to?
fn csr_family(csr: u16) -> CsrFamily {
    if csr == CSR_PRIV {
        return CsrFamily::Pseudo;
    }
    if counteren_bit(csr).is_some() {
        return CsrFamily::Counter;
    }
    match csr {
        // ⚠️ **`scounteren`（`0x906`）是监管者级 CSR** —— 特权规范的地址映射
        //    里 bits `[11:10] == 0b10` 就是「监管者级」⇒ ⇒ 它与其它 S 模式
        //    CSR **同族**（M 与 S 能访问，U 模式 ⇒ cause 2）。
        CSR_SSTATUS | CSR_SIE | CSR_STVEC | CSR_SSCRATCH | CSR_SEPC | CSR_SCAUSE | CSR_STVAL
        | CSR_SIP | CSR_SATP | CSR_SCOUNTEREN => CsrFamily::Supervisor,
        // ⚠️ **`ucounteren`（`0x005`）是用户态 CSR** —— 三级都能访问。
        CSR_UCOUNTEREN => CsrFamily::User,
        // ⚠️ **其余（含 PMP 与全部机器态 CSR）都是机器态族**。
        _ => CsrFamily::Machine,
    }
}

/// The `*counteren` bit that gates a user-level counter CSR.
///
/// ⚠️ `cycle` / `cycleh` ⇒ `CY`(0) · `time` / `timeh` ⇒ `TM`(1) ·
/// `instret` / `instreth` ⇒ `IR`(2)（STAGE-22 裁决 L）。
fn counteren_bit(csr: u16) -> Option<u32> {
    match csr {
        CSR_CYCLE | CSR_CYCLEH => Some(COUNTEREN_CY),
        CSR_TIME | CSR_TIMEH => Some(COUNTEREN_TM),
        CSR_INSTRET | CSR_INSTRETH => Some(COUNTEREN_IR),
        _ => None,
    }
}

/// The counter **enable chain** for the current level (STAGE-22 裁决 L）。
///
/// * **M 模式**：不需要使能链。
/// * **S 模式**：需要 `mcounteren.CY` **且** `scounteren.CY`。
/// * **U 模式**：再需要 `ucounteren.CY`。
///
/// ⚠️ 缺一 ⇒ ⇒ **cause 2**（不是「读回 0」）⇒ ⇒ 判别力来自「**能不能
/// 读**」，不是「读到什么」。
fn counter_access_allowed(state: &SimState, csr: u16, level: u32) -> bool {
    let Some(bit) = counteren_bit(csr) else {
        return true;
    };
    // ⚠️ **M 模式不需要使能链**（裁决 L 的第一行）—— 使能链**只**门控
    // S / U 模式的读。
    if level == PRIV_M {
        return true;
    }
    let mask = 1u32 << bit;
    if file_value(state, CSR_MCOUNTEREN) & mask == 0 {
        return false;
    }
    if file_value(state, CSR_SCOUNTEREN) & mask == 0 {
        return false;
    }
    if level == PRIV_U && file_value(state, CSR_UCOUNTEREN) & mask == 0 {
        return false;
    }
    true
}

/// May the current privilege level touch `csr`?
///
/// **两问**（STAGE-22 订正 5）：
///
/// 1. **这个 CSR 实现了吗？** —— **没实现**的 CSR 在 **M 模式**是「读回
///    **0** / 写被**忽略**」（台账 **D-92** 的既有行为，**不变**），在
///    **S / U 模式**是 **cause 2**（**F26**）。
/// 2. **实现了的话，当前级别能访问吗？** —— **逐族**查表（见
///    [`csr_family`]）。
///
/// ⚠️ **读与写同形**（**F26**）：S 模式**写** M 模式 CSR 也是 **cause 2**
/// ⇒ ⇒ 六条 CSR 指令的读写路径**共用**这一个入口。
///
/// ⚠️ **M 模式下这个函数从不拒绝** ⇒ ⇒ 既有 M 模式语料**零回归**。
pub(crate) fn csr_access_allowed(state: &SimState, csr: u16) -> bool {
    let level = priv_get(state);
    match csr_family(csr) {
        // ⚠️ **伪 CSR**：客体的读是 **0**、客体的写被**忽略** ⇒ ⇒ 任何
        //    级别都**不**是非法指令（§2.5 / 验收 A-8 / A-10）。
        CsrFamily::Pseudo => true,
        // ⚠️ **用户态计数器**：三级都能读，**但** S / U 模式要过**使能链**。
        CsrFamily::Counter => counter_access_allowed(state, csr, level),
        // ⚠️ **S 模式 CSR**：M 与 S 能访问；**U 模式 ⇒ cause 2**。
        CsrFamily::Supervisor => level == PRIV_M || level == PRIV_S,
        // ⚠️ **用户态 CSR**（`ucounteren`）：三级都能访问。
        CsrFamily::User => true,
        // ⚠️ **机器态 CSR**（含**未实现**的那些）：**只有 M 模式**能访问。
        //    M 模式下的「未实现 ⇒ 读回 0 / 写被忽略」由读路径与写路径
        //    **各自**处理 —— 这里**不**重复判它（**一个约束一个落点**）。
        CsrFamily::Machine => level == PRIV_M,
    }
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

/// The trap **target** for a vector table (`tvec`).
///
/// ⚠️ **MODE 字段是 1 位**（**F13**：写 2 ⇒ 读 0；写 3 ⇒ 读 1）⇒ ⇒ 向量
/// 模式的偏移 = `4 × (cause & 0x3)` ⇒ ⇒ **最多 4 项**（**不是** `0x7` ——
///
/// 那只对 2 位的 MODE 字段成立）。
///
/// ⚠️ **两个向量表同口径** —— `mtvec` 与 `stvec` 走**这一个**函数。
fn trap_target(tvec: u32, cause: u32) -> u32 {
    let base = tvec & !0x3;
    if tvec & 0x1 != 0 {
        base.wrapping_add(4 * (cause & 0x3))
    } else {
        base
    }
}

/// Enter the trap handler —— **分两级**（STAGE-22 裁决 I）。
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
///    **vectored** mode to `base + 4 × (cause & 0x3)`.
///
/// Returns the **new PC** (the value written into [`SimState::pc`]),
/// so the caller can report it without reading the state back.
///
/// # The two levels (STAGE-22 · 裁决 I)
///
/// | 陷入时的特权级 | 委托 | 落点 | 状态 CSR |
/// | --- | --- | --- | --- |
/// | M | — | `mtvec` | `mcause` / `mepc` / `mtval` |
/// | S | `medeleg[cause]` = 1 | `stvec` | `scause` / `sepc` / `stval` |
/// | S | `medeleg[cause]` = 0 | `mtvec` | `mcause` / `mepc` / `mtval` |
/// | U | `medeleg[cause]` = 1 | `stvec` | `scause` / `sepc` / `stval` |
/// | U | `medeleg[cause]` = 0 | `mtvec` | `mcause` / `mepc` / `mtval` |
///
/// ⚠️ **M 模式不受 `medeleg` 影响** —— 委托只作用于 S / U 模式下发生的
/// 陷阱 ⇒ ⇒ M 模式的陷阱**永远**走 `mtvec` ⇒ ⇒ 这条**不需要**额外实现。
///
/// ⚠️ **未委托的陷阱落 `mtvec`** ⇒ ⇒ `mstatus.MPP` ← **陷入前的级别**
/// （不是恒 3）⇒ ⇒ M 模式的 handler 能用 `mret` 回到 S / U 模式。
///
/// ⚠️ **委托的陷阱落 `stvec`** ⇒ ⇒ `sstatus.SPP` ← **陷入前的级别**
/// ⇒ ⇒ `sret` 从陷阱处理器**回到 S 模式**（**F28**）。
pub fn enter_trap(state: &mut SimState, cause: u32, mtval: u32) -> u32 {
    let level = priv_get(state);
    if level == PRIV_M {
        return enter_machine_trap(state, cause, mtval, PRIV_M);
    }
    // S / U：查 `medeleg` 的委托位。
    //
    // ⚠️ **cause 可能是中断 cause**（带 `IRQ_BASE` 的高位）⇒ ⇒ 读位之前
    // **先剥掉高位** ⇒ ⇒ 并且 `medeleg` 只有 64 位 ⇒ ⇒ 越界一律**不委托**。
    let code = cause & !IRQ_BASE;
    let delegated = code < 64 && (file_value(state, CSR_MEDELEG) >> code) & 1 == 1;
    if delegated {
        enter_supervisor_trap(state, cause, mtval, level)
    } else {
        enter_machine_trap(state, cause, mtval, level)
    }
}

/// The **M-level** trap entry.
///
/// ⚠️ `MPP := from_level` is **not** cosmetic: the reference side sets it
/// too (measured — Spike reports `mstatus = 0x0018_0080` inside a trap
/// handler, this platform used to report `0x0000_0080`), and the M-13 LLD
/// §5.2.4 says so explicitly ("MPIE←MIE，MIE←0，MPP←M").
///
/// ⚠️ **`from_level` 不是恒 3** —— S / U 模式下发生的**未委托**陷阱落这里
/// ⇒ ⇒ `MPP` ← **陷入前的级别** ⇒ ⇒ `mret` 能回到 S / U 模式。
fn enter_machine_trap(state: &mut SimState, cause: u32, mtval: u32, from_level: u32) -> u32 {
    // 1. mepc := pc — the handler returns to the faulting instruction.
    //
    // ⚠️ **原始写**（不过 WARL 掩码）：压缩指令可能是 **2 字节对齐**的
    // ⇒ 掩掉低两位会让「从陷阱返回」落到指令中间（见 [`csr_write`]）。
    csr_write_raw(state, CSR_MEPC, state.pc);
    // 2. mcause := cause.
    csr_write(state, CSR_MCAUSE, cause);
    // 3. mtval := mtval.
    csr_write(state, CSR_MTVAL, mtval);

    // 4. mstatus: MPIE := MIE, then MIE := 0, then MPP := from_level.
    let mstatus = file_value(state, CSR_MSTATUS);
    let mie = 1u32 << MSTATUS_MIE_BIT;
    let mpie = 1u32 << MSTATUS_MPIE_BIT;
    let mpp = 0x3u32 << MSTATUS_MPP_SHIFT;
    let saved = if mstatus & mie != 0 { mpie } else { 0 };
    mstatus_bits(
        state,
        mie | mpie | mpp,
        saved | (from_level << MSTATUS_MPP_SHIFT),
    );

    // 4-bis. **特权级 ← M**。
    //
    // ⚠️ **未委托的陷阱是在 M 模式下被处理的** —— 特权规范：「未委托的
    // 陷阱把 `xPP` 设为陷入前的级别，而**陷阱本身在 M 模式下被处理**」。
    //
    // ⚠️ **这一条不是可选的**：M 级的陷阱向量要读 `mcause`（**M 模式
    // CSR**）⇒ ⇒ 若特权级**留在 S / U**，那一次读就是**非法指令** ⇒ ⇒
    // 再次陷入 ⇒ ⇒ **挂死**（参考侧实测 **F20** 描述的正是这个现象）。
    //
    // ⚠️ **M 模式下的陷阱是恒等**（`priv_set` 发现级别没变 ⇒ 不写文件）
    // ⇒ ⇒ 既有 M 模式语料**零回归**。
    priv_set(state, PRIV_M);

    // 5. pc := mtvec (direct) or base + 4 × (cause & 0x3) (vectored).
    let target = trap_target(file_value(state, CSR_MTVEC), cause);
    state.pc = target;
    target
}

/// The **S-level** trap entry —— 委托的陷阱落这里。
///
/// ⚠️ **`sstatus` 的三个位走 `mstatus` 的原始位操作**（[`mstatus_bits`]）⇒
/// ⇒ **不受客体的 `0x22` 掩码约束** ⇒ ⇒ `SPP` 能被写成 **1** ⇒ ⇒ 那就是
/// **F28**。
fn enter_supervisor_trap(state: &mut SimState, cause: u32, mtval: u32, from_level: u32) -> u32 {
    // 1. sepc := pc — **原始写**（与 `mepc` 同口径：精确的 PC）。
    csr_write_raw(state, CSR_SEPC, state.pc);
    // 2. scause := cause.
    csr_write(state, CSR_SCAUSE, cause);
    // 3. stval := mtval.
    csr_write(state, CSR_STVAL, mtval);

    // 4. sstatus: SPIE := SIE, then SIE := 0, then SPP := from_level.
    //
    // ⚠️ **SPP ← 陷入前的级别**：S 模式陷入 ⇒ `SPP` = 1 ⇒ ⇒ `sret` 回到
    // **S 模式**；U 模式陷入 ⇒ `SPP` = 0 ⇒ ⇒ 回到 **U 模式**。
    let mstatus = file_value(state, CSR_MSTATUS);
    let sie = 1u32 << MSTATUS_SIE_BIT;
    let spie = 1u32 << MSTATUS_SPIE_BIT;
    let spp = 1u32 << MSTATUS_SPP_BIT;
    let saved = if mstatus & sie != 0 { spie } else { 0 };
    mstatus_bits(
        state,
        sie | spie | spp,
        saved | (from_level << MSTATUS_SPP_BIT),
    );

    // 5. 特权级 ← S（S 模式 CSR 从这里开始可见）。
    priv_set(state, PRIV_S);

    // 6. pc := stvec (direct) or base + 4 × (cause & 0x3) (vectored).
    let target = trap_target(file_value(state, CSR_STVEC), cause);
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

    /// ⚠️ **`mhartid` 的值是平台的常量，不是 CSR 文件里的内容**。
    ///
    /// 这是「**正式实现**」与「碰巧读回 0」的区别：`mhartid` 是**只读**
    /// CSR（`0xF14` 在只读区）⇒ 正常路径下文件里**永远没有**它 ⇒
    /// 「文件里没有 ⇒ 读回 0」这条**巧合**会给出**同样的答案** ⇒ 光断言
    /// 「读回 0」**证明不了**任何东西。
    ///
    /// ⇒ 这条用例**故意**往文件里塞一个非 0 的 `mhartid`（模拟一份畸形
    /// 的会话载荷），然后断言读回来**仍然是** 0 —— 只有「按常量回答」
    /// 的实现才过得了这一关。
    #[test]
    fn the_mhartid_value_is_the_platforms_constant_not_the_file_content() {
        let mut state = SimState::default();
        // 平台自己的写（原始写）—— 模拟一份塞了 `mhartid` 的载荷。
        csr_write_raw(&mut state, CSR_MHARTID, 0x1234_5678);
        assert_eq!(
            csr_value(&state, CSR_MHARTID),
            0x1234_5678,
            "对照：文件里的内容确实被写进去了"
        );
        assert_eq!(
            csr_read(&state, CSR_MHARTID),
            0,
            "读 mhartid 必须答平台的常量（单核 ⇒ 0），**不是**文件里的内容"
        );
    }
}
