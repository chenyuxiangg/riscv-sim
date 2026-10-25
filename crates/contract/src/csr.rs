//! Machine-mode CSR vocabulary and the platform's interrupt-line space.
//!
//! # Why this module exists
//!
//! Four facts are needed by **more than one** module, and each of them
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
//!    enters the trap) must agree on;
//! 4. the **line → architectural `mcause` code** mapping
//!    ([`arch_cause_for_line`]) — a **third** numbering space, again
//!    different from the two above: the architectural interrupt codes
//!    are `3` (software) / `7` (timer) / `11` (external), while the
//!    platform's internal line space is `1 / 2 / 3`.
//!
//! Putting them here makes each of them a **single landing spot**
//! (one constraint, one home) instead of two copies that can drift.
//!
//! ⚠️ **The three numbering spaces are deliberately kept apart** (see
//! [`arch_cause_for_line`]): folding the line space into the
//! architectural cause space would break the `mie` / `mip` bit map
//! (ledger **D-84 / D-100**), and reporting the line space as the
//! cause code is a **real defect** — a guest that dispatches on the
//! architectural cause code (every real M-mode RTOS does) would match
//! **none** of them.
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
/// (atomics), no `F`/`D` (floating point).  The IALIGN field (bits 29:26)
/// is left **0**, exactly as the reference side leaves it.
///
/// ⇒ since **v0.38** this platform implements **S-mode and U-mode**
/// ⇒ ⇒ the two privilege-level bits are set too ⇒ ⇒ the reference
/// side's `misa` is now **bit-identical** to ours.
///
/// Measured (the reference side runs the **golden ISA string**,
/// `rv32imc_zicsr_zicclsm_zifencei_zicntr`, **without** `--priv` — that
/// is exactly how the golden harness runs it):
///
/// | | MXL | I | M | C | **S** (18) | **U** (20) | IALIGN (29:26) |
/// |---|---|---|---|---|---|---|---|
/// | reference side | 1 | 1 | 1 | 1 | **1** | **1** | 0 |
/// | this platform  | 1 | 1 | 1 | 1 | **1** | **1** | 0 |
///
/// ⇒ ⇒ **逐位相同**（STAGE-22 · F3 / F24 / F27）⇒ ⇒ 验收 **X-1**。
///
/// ⚠️ The reference side's `misa` is also **WARL-writable** (a guest
/// may clear bits — measured: `csrci misa, 4` really clears `C`);
/// ours is **read-only** (a guest write is ignored).  That is a real
/// semantic difference, not a "coverage equivalence" — see the ledger
/// (D-95).
pub const CSR_MISA_VALUE: u32 = (1 << 30) | (1 << 20) | (1 << 18) | (1 << 8) | (1 << 12) | (1 << 2);

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

/// `mcountinhibit` — machine counter-inhibit register (Zicntr).
///
/// ⚠️ **It is a plain read/write storage register on this platform** —
/// a guest write **lands** and reads back (measured on the reference
/// side: writing `5` reads back `5`).
///
/// ⚠️ **It does NOT gate anything here, and that is *correct*, not a
/// hole.**  The RISC-V rule is that `mcountinhibit` inhibits the
/// **user-mode** reads of `cycle` / `instret` / `time`; this platform
/// is **M-mode only** ⇒ ⇒ in M-mode the inhibit has **no effect** by
/// specification.  Measured on the reference side: writing `5` then
/// reading `cycle` still yields the normal running count.
///
/// ⇒ The register is implemented (so writes do not trap and do read
/// back) while the *gating* is a no-op — the two facts are separate,
/// and the upstream `rv32mi/instret_overflow` case writes
/// `csrci mcountinhibit, 0x4` precisely because the reset value is
/// unspecified.
pub const CSR_MCOUNTINHIBIT: u16 = 0x320;

/// `mcycle` — machine cycle counter (Zicntr), **low 32 bits**.
///
/// ⚠️ **Writable** (RISC-V: machine-level read/write custom).  A guest
/// write lands in the CSR file and reads back.
///
/// ⚠️ The counter is **64 bits wide**; `mcycle` is its **low half** and
/// [`CSR_MCYCLEH`] is the high half.  See [`counter_pair`] — the
/// platform keeps the two halves as **two CSR-file entries**, which is
/// bit-for-bit equivalent to one 64-bit counter (with carry) and keeps
/// the state inside the CSR file, so session snapshots round-trip it
/// for free.
pub const CSR_MCYCLE: u16 = 0xB00;

/// `minstret` — machine retired-instruction counter (Zicntr), **low 32
/// bits**.
///
/// ⚠️ **Writable**, same as [`CSR_MCYCLE`]; the high half is
/// [`CSR_MINSTRETH`].
pub const CSR_MINSTRET: u16 = 0xB02;

/// `mcycleh` — machine cycle counter, **high 32 bits**.
///
/// ⚠️ **Writable** (machine-level read/write custom).  Writing it
/// changes the **high half only** — the low half is untouched.
pub const CSR_MCYCLEH: u16 = 0xB80;

/// `minstreth` — machine retired-instruction counter, **high 32 bits**.
///
/// ⚠️ **Writable**, same as [`CSR_MCYCLEH`].
pub const CSR_MINSTRETH: u16 = 0xB82;

/// `cycle` — the **user-visible** cycle counter (Zicntr shadow).
///
/// ⚠️ **Read-only**, and it is a **shadow**: it reports the value of
/// [`CSR_MCYCLE`].  It is **not** a second copy of the counter — the
/// value is *derived* from the machine counter, so there is no second
/// source of truth to drift.  See [`counter_shadow`].
///
/// ⚠️ The address rule makes it read-only: `0xC00` has bits
/// `[11:10] == 0b11`.
pub const CSR_CYCLE: u16 = 0xC00;

/// `time` — the **user-visible** wall-clock timer (Zicntr shadow of the
/// CLINT's `mtime`).
///
/// ⚠️ **Read-only** by the address rule.
///
/// ⚠️ **This platform reads it as `0`.**  The reference side also reads
/// it as `0` (measured: its `mtime` is never advanced in the golden
/// configuration) ⇒ ⇒ the two sides agree.  ⚠️ But the **feature** is
/// still missing here — a real `time` needs the M-15 CLINT wired into
/// the CSR read path, which is a cross-module change ⇒ ⇒ tracked as a
/// ledger entry rather than silently pretended.
pub const CSR_TIME: u16 = 0xC01;

/// `instret` — the **user-visible** retired-instruction counter (Zicntr
/// shadow).
///
/// ⚠️ **Read-only**, and a **shadow** of [`CSR_MINSTRET`] — see
/// [`counter_shadow`].
pub const CSR_INSTRET: u16 = 0xC02;

/// `cycleh` — the **user-visible** cycle counter, **high 32 bits**.
///
/// ⚠️ **Read-only**, a shadow of [`CSR_MCYCLEH`].
pub const CSR_CYCLEH: u16 = 0xC80;

/// `timeh` — the **user-visible** wall-clock timer, **high 32 bits**.
///
/// ⚠️ **Read-only**, and reads as `0` like [`CSR_TIME`].
pub const CSR_TIMEH: u16 = 0xC81;

/// `instreth` — the **user-visible** retired-instruction counter, **high
/// 32 bits**.
///
/// ⚠️ **Read-only**, a shadow of [`CSR_MINSTRETH`].
pub const CSR_INSTRETH: u16 = 0xC82;

/// `mtime` — the wall-clock timer counter (RV32 read-only CSR).
pub const CSR_MTIME: u16 = 0xBFF8;

/// `mhartid` — the hart identifier (RV32 read-only CSR).
///
/// ⚠️ **It is read-only by the RISC-V address rule** (`0xF14` has bits
/// `[11:10] == 0b11` ⇒ the read-only range `0xC00..0xFFF`) ⇒ a guest
/// write is **ignored** by [`csr_write_is_ignored`] no matter what the
/// implemented-CSR table says.
pub const CSR_MHARTID: u16 = 0xF14;

/// `mhartid` — the value this platform reports.
///
/// The platform is **single-core** ⇒ **hart 0**.
///
/// ⚠️ **It is a constant, not a CSR-file entry**: the platform
/// **implements** the register (see [`csr_is_implemented`]), so the
/// "unimplemented ⇒ reads 0" accident must not be what makes it read
/// zero — the ISS answers with **this** value, whatever the CSR file
/// happens to contain.
pub const CSR_MHARTID_VALUE: u32 = 0;

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

// ---------------------------------------------------------------------------
// S-mode / U-mode (STAGE-22 · v0.38)
// ---------------------------------------------------------------------------

/// `sstatus` — supervisor status (**a bit-view of `mstatus`**, not a
/// second storage — see [`SSTATUS_VIEW_MASK`]).
pub const CSR_SSTATUS: u16 = 0x100;

/// `sie` — supervisor interrupt-enable.
pub const CSR_SIE: u16 = 0x104;

/// `stvec` — supervisor trap-vector base address and mode.
pub const CSR_STVEC: u16 = 0x105;

/// `sscratch` — supervisor scratch register.
pub const CSR_SSCRATCH: u16 = 0x140;

/// `sepc` — the address the supervisor trap handler returns to.
pub const CSR_SEPC: u16 = 0x141;

/// `scause` — the supervisor trap reason.
pub const CSR_SCAUSE: u16 = 0x142;

/// `stval` — the supervisor trap's auxiliary value.
pub const CSR_STVAL: u16 = 0x143;

/// `sip` — supervisor interrupt-pending.
pub const CSR_SIP: u16 = 0x144;

/// `satp` — supervisor address translation and protection.
///
/// ⚠️ **Only Bare is supported** ⇒ ⇒ the reference side (which has Sv32) is
/// **not comparable** ⇒ ⇒ `satp` does **not** enter the golden corpus
/// (STAGE-22 §10 第 1 条).
pub const CSR_SATP: u16 = 0x180;

/// `medeleg` — the trap delegation register.
pub const CSR_MEDELEG: u16 = 0x302;

/// `mideleg` — the interrupt delegation register.
pub const CSR_MIDELEG: u16 = 0x303;

/// `mcounteren` — machine counter-enable.
pub const CSR_MCOUNTEREN: u16 = 0x306;

/// `scounteren` — supervisor counter-enable.
pub const CSR_SCOUNTEREN: u16 = 0x906;

/// `ucounteren` — user counter-enable.
pub const CSR_UCOUNTEREN: u16 = 0x005;

/// `CSR_PRIV` — the **pseudo CSR** that holds the current privilege level.
///
/// ⚠️ **This is not an architectural CSR.** It is the platform's way of
/// keeping the privilege level **inside the CSR file** instead of adding a
/// field to `SimState` (which would bump `SIM_STATE_SCHEMA_VERSION` and
/// invalidate every stored snapshot — ledger D-143's family).
///
/// * address: **`0x0C0`** — the custom read-only quarter (`0x000`~`0x0FF`),
///   so a guest writing it is architecturally illegal;
/// * value: the privilege level, encoded **exactly like `mstatus.MPP`**
///   ([`PRIV_U`] / [`PRIV_S`] / [`PRIV_M`]) ⇒ ⇒ **one** set of literals.
///
/// ⚠️ **Two guards, each with its own landing spot**:
///
/// * the guest's **write** is dropped — enforced by [`csr_is_read_only`]
///   (STAGE-22 订正 6: leaning on "unimplemented CSRs ignore writes" is
///   **false** here, because `CSR_PRIV` **must** be in
///   [`csr_is_implemented`] for the snapshot to round-trip);
/// * the guest's **read** returns **0** — enforced by the kernel's
///   `csr_read` special case ⇒ ⇒ the platform reads the level through
///   `priv_get`, which goes to the **file** directly.
pub const CSR_PRIV: u16 = 0x0C0;

/// The **user** privilege level.
pub const PRIV_U: u32 = 0;

/// The **supervisor** privilege level.
pub const PRIV_S: u32 = 1;

/// The **machine** privilege level.
pub const PRIV_M: u32 = 3;

/// `mstatus.SIE` — the supervisor interrupt-enable bit (bit 1).
pub const MSTATUS_SIE_BIT: u32 = 1;

/// `mstatus.SPIE` — "previous `SIE`" (bit 5).
pub const MSTATUS_SPIE_BIT: u32 = 5;

/// `mstatus.SPP` — "previous privilege" (bit 8).
///
/// ⚠️ **Measured (F7)**: writable in M-mode, and **the same field** as
/// `sstatus.SPP`.
pub const MSTATUS_SPP_BIT: u32 = 8;

/// `mstatus.MPP` — the **user** privilege level (the `MPP` encoding).
pub const MSTATUS_MPP_U: u32 = 0;

/// `mstatus.MPP` — the **supervisor** privilege level.
pub const MSTATUS_MPP_S: u32 = 1;

/// `mcounteren` / `scounteren` / `ucounteren` — the `cycle` counter bit.
pub const COUNTEREN_CY: u32 = 0;

/// `mcounteren` / `scounteren` / `ucounteren` — the `time` counter bit.
pub const COUNTEREN_TM: u32 = 1;

/// `mcounteren` / `scounteren` / `ucounteren` — the `instret` counter bit.
pub const COUNTEREN_IR: u32 = 2;

// ---------------------------------------------------------------------------
// Trap causes (exceptions — **not** interrupts)
// ---------------------------------------------------------------------------

/// Exception cause **2** — illegal instruction.
///
/// ⚠️ **This is the exception cause**, not the interrupt cause: the
/// interrupt cause 3 is [`CAUSE_MACHINE_SOFTWARE`] and the interrupt cause
/// 11 is [`CAUSE_MACHINE_EXTERNAL`] — **same numbers, different spaces**.
pub const CAUSE_ILLEGAL_INSTRUCTION: u32 = 2;

/// Exception cause **3** — breakpoint (`ebreak` / `sbreak`).
pub const CAUSE_BREAKPOINT: u32 = 3;

/// Exception cause **8** — environment call from **U-mode**.
pub const CAUSE_USER_ECALL: u32 = 8;

/// Exception cause **9** — environment call from **HS-mode** (S-mode).
pub const CAUSE_SUPERVISOR_ECALL: u32 = 9;

/// Exception cause **11** — environment call from **M-mode**.
pub const CAUSE_MACHINE_ECALL: u32 = 11;

/// `mstatus.MIE` — the **global** interrupt-enable bit.
///
/// Bit 3 of `mstatus` (standard).
pub const MSTATUS_MIE_BIT: u32 = 3;

/// `mstatus`'s **modify privilege** bit — `MPRV` (F10 / F11).
///
/// ⚠️ When it is set, M-mode **loads and stores** go through the PMP check
/// exactly like an S/U-mode access would (F10).  ⚠️ It does **not** affect
/// instruction fetch (F11).
pub const MSTATUS_MPRV_BIT: u32 = 17;

/// `mstatus.MPIE` — "previous `MIE`", saved on trap entry and restored
/// by `mret`.
///
/// Bit 7 of `mstatus` (standard).
pub const MSTATUS_MPIE_BIT: u32 = 7;

/// `mstatus` 的 **WARL 掩码**：写进去时**只保留这些位**，其余位
/// （**保留位** / WPRI）**写被忽略**、读回 **0**。
///
/// RISC-V 特权规范的 `mstatus` 位定义（RV32，20191213 版布局）：
///
/// | 位 | 字段 | 位 | 字段 |
/// | --- | --- | --- | --- |
/// | 1 | `SIE` | 15:16 | `XS` |
/// | 3 | `MIE` | 17 | `MPRV` |
/// | 5 | `SPIE` | 18 | `SUM` |
/// | 6 | `UBE` | 19 | `MXR` |
/// | 7 | `MPIE` | 20 | `TVM` |
/// | 8 | `SPP` | 21 | `TW` |
/// | 9:10 | `VS` | 22 | `TSR` |
/// | 11:12 | `MPP` | 31 | `SD` |
/// | 13:14 | `FS` | | |
///
/// ⇒ **保留位** = `0` / `2` / `4` / `6` / `23` / `24` ~ `30`；另有一批
/// **本平台不支持的扩展位**（`VS` 9:10 / `XS` 15:16 的一半 / `UBE`）⇒ ⇒
/// 掩码 = **`0x007E_79AA`**。
///
/// ⚠️ **这个掩码是**实测**出来的，不是照规范抄的**（STAGE-22 · **F6**）：
/// 参考侧「写 `0xFFFFFFFF` 之后读回 `0x807E_79AA`」⇒ ⇒ 本平台现在
/// **与参考侧逐位相同** ⇒ ⇒ 台账 **D-98** 的「写全 1 之后两侧的
/// `mstatus` 不会逐位相等」这条**已知残留已消除**。
///
/// ⚠️ **bit 31（`SD`）不在这个掩码里**（**F40**）：`SD` 是**推导位** ⇒ ⇒
/// **不可直接写**（实测：写 `0x8000_0000` ⇒ 读回 `0x0000_0000`）⇒ ⇒ 它
/// 由 `FS` / `XS` 推出来 ⇒ ⇒ 推导在 [`mstatus_warl`] 里 ⇒ ⇒ **写全 1
/// 之后读回的是 `0x807E_79AA`**（`FS = 3` ⇒ `SD = 1`）⇒ ⇒ 掩码与读回值
/// **差的那一位就是 `SD`** ⇒ ⇒ 两条**不是**矛盾，是「可写位」与「最终值」的
/// 区别。
///
/// ⚠️ **旧值 `0x807F_FFEA` 是超集**（多出 bit 6 / 9 / 10 / 15 / 16）⇒ ⇒
/// guest 能写那些「扩展不存在就不该可写」的位而参考侧会丢掉 ⇒ ⇒ **真
/// bug**，v0.38 修掉。
///
/// ⚠️ **`MPP`（11:12）留在掩码里**：`MPP` 是 **WARL**，合法值 = **{0 = U,
/// 1 = S, 3 = M}**（F5）⇒ ⇒ 它的**合法化**不在这个掩码里 ⇒ ⇒ 见
/// [`mstatus_warl`]。
///
/// ⚠️ **掩码表达不了「保留值 ⇒ 整次写被丢弃」**（F5）⇒ ⇒ **`mstatus` 的
/// 写侧必须走 [`mstatus_warl`] 这个函数**，不能只做 `& mask`。
pub const MSTATUS_WARL_MASK: u32 = 0x007E_79AA;

/// `mstatus.FS` 字段（bits 14:13）—— **浮点脏位**，`3` = Dirty。
pub const MSTATUS_FS_MASK: u32 = 0x3 << 13;

/// `mstatus.XS` 字段（bits 16:15）—— **向量脏位**，`3` = Dirty。
///
/// ⚠️ 本平台**不实现向量扩展** ⇒ ⇒ `XS` **不可写**（不在
/// [`MSTATUS_WARL_MASK`] 里）⇒ ⇒ 它**恒为 0** ⇒ ⇒ 但 [`mstatus_warl`]
/// 仍然按规范把 `XS` 算进 `SD` 的推导 ⇒ ⇒ 口径与参考侧同形。
pub const MSTATUS_XS_MASK: u32 = 0x3 << 15;

/// `mstatus.SD`（bit 31）—— **推导位**：= (`FS` == Dirty) ∨ (`XS` == Dirty)。
///
/// ⚠️ **实测（F40）**：参考侧「只把 `FS` 写成 3（`0x6000`）、不碰 bit 31」
/// ⇒ ⇒ 读回 **`0x8000_6000`** ⇒ ⇒ `SD` 被**推成 1**；而「直接写 bit 31」
/// ⇒ ⇒ 读回 **`0x0000_0000`** ⇒ ⇒ **直接写无效** ⇒ ⇒ `SD` **不是存储位**。
pub const MSTATUS_SD_BIT: u32 = 1 << 31;

/// `mstatus` 的 WARL 合法化 —— **唯一的落点**。
///
/// ⚠️ **它不是掩码**：掩码表达不了「`MPP` 保留值 ⇒ **合法化到 0**」。
///
/// **实测（F5 + F39）**：写保留值 `MPP = 2` ⇒ 参考侧**把该字段合法化到 0**
/// ⇒ ⇒ **不是**「保留旧值」⇒ ⇒ **也**不是「整次写被丢弃」——同一次写里的 `SIE`
/// **落进去了**：清零后写 `0x1002`（`MPP = 2` 且 `SIE = 1`）⇒ 读回
/// **`0x0000_0002`**；先写 `0x1800`（`MPP = 3`）再写 `0x1002` ⇒ 读回**仍是
/// `0x0000_0002`** ⇒ ⇒ 旧值被**覆盖成 0**，不是被保留。
///
/// **规则（顺序钉死）**：
///
/// 1. `masked = new & MSTATUS_WARL_MASK`（实测掩码，F6）；
/// 2. 取 `masked` 的 `MPP` 字段（bits 12:11）；
/// 3. 若该字段 ∉ **{0, 1, 3}**（保留值 = **2**）⇒ ⇒ **该字段合法化到 0**
///    ⇒ ⇒ **同一次写里的其它位照常落进去**；
/// 4. 否则 ⇒ `MPP` 取新值；
/// 5. 返回「掩码后的新值」+「合法化后的 `MPP`」。
///
/// ⚠️ **为什么没有 `old` 参数**：合法化到 0 的语义**不依赖旧值** ⇒ ⇒ 留一个
/// 读不到用处的参数就是**谎言** ⇒ ⇒ 函数**只吃新值**。
///
/// ⚠️ **`mstatus.SPP`（bit 8）在 M 模式可写**（F7）⇒ ⇒ 它在掩码里 ⇒ ⇒
/// 由这条规则一并处理。
///
/// ⚠️ **唯一落点**：M-13 的 CSR 写路径**只调用**它，**不**另写一份
/// `& mask`（同 v0.37 的 `pmpcfg` 纪律）。
pub fn mstatus_warl(new: u32) -> u32 {
    let masked = new & MSTATUS_WARL_MASK;
    let mpp_mask = 0x3 << MSTATUS_MPP_SHIFT;
    let mpp_new = (masked & mpp_mask) >> MSTATUS_MPP_SHIFT;
    let mpp = if mpp_new == MSTATUS_MPP_U || mpp_new == MSTATUS_MPP_S || mpp_new == MSTATUS_MPP_M {
        mpp_new
    } else {
        // 保留值 ⇒ **合法化到 0**（U），**不是**保留旧值（F39）。
        MSTATUS_MPP_U
    };
    // ⚠️ `SD`（bit 31）是**推导位**（F40）⇒ ⇒ 不可直接写 ⇒ ⇒ 由 `FS` /
    // `XS` 推出来 ⇒ ⇒ 「写全 1」读回 `0x807E_79AA`（`FS = 3` ⇒ `SD = 1`），
    // 而「只写 `FS = 3`」也读回 `0x8000_6000`（实测逐位相同）。
    let fs = masked & MSTATUS_FS_MASK;
    let xs = masked & MSTATUS_XS_MASK;
    let sd = if fs == MSTATUS_FS_MASK || xs == MSTATUS_XS_MASK {
        MSTATUS_SD_BIT
    } else {
        0
    };
    (masked & !mpp_mask) | (mpp << MSTATUS_MPP_SHIFT) | sd
}

/// `sstatus` 的**客体写**掩码 —— 客体写 `sstatus` 时会落到 `mstatus` 的哪些位。
///
/// ⚠️ **实测（F38）**：参考侧在 **S 模式**写 `sstatus = 0xFFFFFFFF` ⇒ 读回
/// **`0x800C_6122`**；在 **M 模式**写同一个值 ⇒ 读回 `mstatus` =
/// **`0x800C_61A2`** ⇒ ⇒ 两者**只差 `MPIE`（bit 7）** ⇒ ⇒ 写掩码 =
/// **`0x804C_61A2`**（`SIE` \| `SPIE` \| `MPIE` \| `SPP` \| `FS[14:13]` \|
/// `SUM` \| `MXR` \| `SD`）。
///
/// ⚠️ **S 模式与 M 模式的写掩码相同**（F33）⇒ ⇒ 参考侧**不按特权裁剪**
/// `sstatus` 的写掩码。
///
/// ⚠️ **与特权规范的差异（明示取舍，登记在 `tests/golden/README.md` 的已知偏差
/// 表）**：特权规范里 `SPP` 只在 M 模式可写、`SUM` / `MXR` 是 M 级字段；而参考侧
/// **允许 S 模式写 `sstatus.SPP`**，`SUM` / `MXR` 也**在 `sstatus` 的视图里**
/// ⇒ ⇒ 本平台**对齐参考侧**（黄金对拍的口径就是参考侧）。
pub const SSTATUS_WARL_MASK: u32 = 0x804C_61A2;

/// `sstatus` 的**读视图**掩码 —— `sstatus` 是 `mstatus` 的**位视图**。
///
/// ⚠️ **不是**一份第二存储（F30）：M 模式写 `sstatus` ⇒ S 模式读回**同一个
/// 字段** ⇒ ⇒ 同一份存储。
///
/// ⚠️ **实测（F38）**：M 模式把 `mstatus` 写满（读回 `0x807E_79AA`）⇒ S 模式
/// 读 `sstatus` = **`0x800C_6122`** ⇒ ⇒ 视图掩码 = **`0x800D_E162`**
/// （`SIE` \| `SPIE` \| `UBE` \| `SPP` \| `FS[14:13]` \| `XS[16:15]` \| `SUM` \|
/// `MXR` \| `SD`）。
///
/// ⚠️ **`UBE`（bit 6）与 `XS`（bits 16:15）在参考侧恒为 0**（`mstatus` 的写掩码
/// `0x807E_79AA` 不含这两处）⇒ ⇒ 它们在不在视图里**都不可观测** ⇒ ⇒ 取
/// 「在视图里」是为了与参考侧的掩码**逐字一致**。
///
/// ⚠️ **不含** `MIE`（bit 3）/ `MPIE`（bit 7）/ `MPP`（bits 12:11）/ `MPRV`
/// （bit 17）/ `TVM` / `TW` / `TSR` ⇒ ⇒ **S 模式看不到机器态的字段**。
pub const SSTATUS_VIEW_MASK: u32 = 0x800D_E162;

/// `medeleg` 的**合法位集合**（per-bit WARL）—— 参考侧实测。
///
/// ⚠️ **实测（F35 + F36）**：参考侧写 `medeleg = 0xFFFFFFFF` ⇒ 读回 **`0x0008_B3FE`**；
/// 写 `0x0000_FFFF` ⇒ 读回 **`0x0000_B3FE`** ⇒ ⇒ 两者都是「请求值 ∧ 同一个
/// 合法位集合」⇒ ⇒ **per-bit WARL**，合法位集合 = **`0x0008_B3FE`**。
///
/// ⚠️ **落点**：M-13 的 CSR 写路径对 `medeleg` **必须**先过这个掩码 ⇒ ⇒ 否则
/// 本平台会把「指令地址未对齐」（bit 0）也当成可委托 ⇒ ⇒ 与参考侧**逐位不等**
/// （参考侧把 bit 0 丢掉）。
pub const MEDELEG_WRITABLE_MASK: u32 = 0x0008_B3FE;

/// `mepc` 取 PC 时要**掩掉**的位：`mret` 跳到 `mepc` 时按 **IALIGN**
/// 掩码（特权规范）—— 本平台实现了 **C 扩展**（`misa` 的 C 位 = 1）⇒
/// **IALIGN = 16** ⇒ 只清**最低 1 位**。
///
/// ⚠️ **只清 1 位，不清 2 位**：台账 D-93 原文写的「`mepc[1:0]` 当作
/// 零」是 **IALIGN = 32**（没有 C 扩展）下的口径，**对不上本平台**。
/// 参考侧实测（同一个探针 ELF 两侧跑）：
///
/// * 陷入门把**精确 PC**写进 `mepc`（压缩指令 `c.sw` 在 `0x8000014a`
///   ⇒ 参考侧 `mepc = 0x8000014a`，**bit 1 保留**）；
/// * `mret` 落回 `4k+2`（而不是 `4k`）⇒ 掩码是 `~1`。
///
/// ⇒ 掩码是 **`~0x1`**（`0xFFFFFFFE`）。
///
/// ⚠️ **掩码只作用在 `mret` 取 PC 的那一步**，**不是**写侧：`csrr mepc`
/// 必须读回**写入的精确值**（handler 要能读到真实的出错地址）。
///
/// ⚠️ **唯一的落点**：这个常量是「`mret` 取 PC 时按 IALIGN 掩码」这条
/// 规则的**唯一**定义处 —— M-13 的 `mret` 目标计算**引用**它，**不**另写
/// 一份字面量。
pub const MEPC_ALIGN_MASK: u32 = 0xFFFF_FFFE;

/// `mstatus.MPP` — the privilege level the hart was in when the trap
/// was taken.
///
/// **Bits 12:11** of `mstatus` (standard).
///
/// ⚠️ The field is **WARL**: an implementation may only hold values
/// that name a privilege level it actually supports.  Since **v0.38** this
/// platform supports **M, S and U** ⇒ ⇒ the legal values are **{0 = U,
/// 1 = S, 3 = M}** (STAGE-22 · **F5**) ⇒ ⇒ `misa` reports the `S` and
/// the `U` bit ⇒ ⇒ and the three places that touch `MPP` (the trap entry,
/// `mret`, the guest's own `csrw mstatus`) all agree on that set.
///
/// ⚠️ **Writing a reserved value (2) drops the WHOLE write** (F5) ⇒ ⇒ that
/// rule lives in [`mstatus_warl`], **not** in a mask.
///
/// The reference side (Spike, default `--priv=MSU`) supports the same
/// three levels ⇒ ⇒ the two sides' `MPP` agree.
pub const MSTATUS_MPP_SHIFT: u32 = 11;

/// The `mstatus.MPP` value for **machine mode**.
pub const MSTATUS_MPP_M: u32 = 3;

/// `mie.MEIE` — machine **external** interrupt enable (bit 11).
pub const MIE_MEIE_BIT: u32 = 11;

/// `mie.MTIE` — machine **timer** interrupt enable (bit 7).
pub const MIE_MTIE_BIT: u32 = 7;

/// The interrupt bit of `mcause` — `mcause[31]`.
///
/// An interrupt cause is `IRQ_BASE | <architectural cause code>`; an
/// exception cause is a small plain number (`0` / `2` / `3` / `4` / `6`
/// / `11`).
///
/// ⚠️ The **architectural cause code** is **not** the platform's
/// internal line number — see [`arch_cause_for_line`].
pub const IRQ_BASE: u32 = 0x8000_0000;

/// Architectural `mcause` code — **machine software interrupt**.
///
/// ⚠️ The platform has **no software-interrupt source** today ⇒
/// [`arch_cause_for_line`] never returns this code.  It is spelled out
/// anyway because the code is part of the architectural vocabulary the
/// platform must speak: a guest that switches on `mcause` may name it.
pub const CAUSE_MACHINE_SOFTWARE: u32 = 3;

/// Architectural `mcause` code — **machine timer interrupt**.
pub const CAUSE_MACHINE_TIMER: u32 = 7;

/// Architectural `mcause` code — **machine external interrupt**.
pub const CAUSE_MACHINE_EXTERNAL: u32 = 11;

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
///
/// ⚠️ **Do not merge this table with [`arch_cause_for_line`]** — RISC-V
/// makes the bit numbers *happen to* equal the architectural cause
/// codes (`MTIE` = 7 = the timer cause, `MEIE` = 11 = the external
/// cause), but the platform's **internal line numbers** (`1 / 2 / 3`)
/// are a **third** space, and the delivery path needs the *cause* one.
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
/// which** UART line is pending.  The architectural cause code only
/// reaches the guest at delivery time, through `mcause` (see
/// [`arch_cause_for_line`]).
pub fn pending_lines_to_mip_mask(lines: &[u16]) -> u32 {
    let mut mask = 0u32;
    for &line in lines {
        if let Some(bit) = std_irq_bit_for_line(line) {
            mask |= 1 << bit;
        }
    }
    mask
}

/// The platform's **internal interrupt line** → the **architectural
/// `mcause` interrupt code**.
///
/// # Why this map exists at all
///
/// The platform numbers its interrupt lines `1 / 2 / 3` (UART rx-ready
/// / UART tx-empty / timer-compare).  RISC-V numbers the *machine
/// interrupt causes* `3` (software) / `7` (timer) / `11` (external).
/// **Those are two different numbering spaces**, and the guest only
/// ever sees the second one: `mcause` is what a trap handler reads.
///
/// Reporting the **internal line number** in `mcause` is therefore a
/// **real defect**, not a cosmetic difference — a guest that dispatches
/// on the architectural cause code (every real M-mode RTOS does) would
/// match **none** of them.
///
/// # The three spaces, kept apart
///
/// | space | values | who uses it |
/// | --- | --- | --- |
/// | internal **line** | `1` / `2` / `3` | the controller, the enable mask, `irq_clear` |
/// | **`mie` / `mip` bit** | `11` (MEIE) / `7` (MTIE) | [`std_irq_bit_for_line`] |
/// | **architectural cause** | `3` / `7` / `11` | **this** map ⇒ `mcause` |
///
/// ⚠️ **This map does not touch the other two** — the `mie` / `mip` bit
/// map keeps its own table ([`std_irq_bit_for_line`]), and the internal
/// line numbers keep their own values.  Folding them together would
/// break the `mip` projection (ledger **D-84 / D-100**).
///
/// # The mapping
///
/// * `3` (timer-compare) → **7** ([`CAUSE_MACHINE_TIMER`])
/// * `1` / `2` (UART rx-ready / tx-empty) → **11**
///   ([`CAUSE_MACHINE_EXTERNAL`]) — M-mode has **one** external
///   interrupt bit, so both UART lines share the external cause, just
///   as they share the `MEIP` bit
/// * anything else → `None` — the line has **no** architectural cause
///   code, so it **must not** be delivered to a guest (a delivered
///   interrupt whose `mcause` is not an architectural code is exactly
///   the defect this map exists to remove)
///
/// ⚠️ **The software-interrupt code (3) is never produced** — the
/// platform has no software-interrupt source today
/// ([`CAUSE_MACHINE_SOFTWARE`]).
pub fn arch_cause_for_line(line: u16) -> Option<u32> {
    match line {
        IRQ_TIMER_COMPARE => Some(CAUSE_MACHINE_TIMER),
        IRQ_UART_RX_READY | IRQ_UART_TX_EMPTY => Some(CAUSE_MACHINE_EXTERNAL),
        _ => None,
    }
}

/// The platform's internal interrupt line → the **full `mcause` value**
/// the guest reads (`IRQ_BASE | <architectural cause code>`).
///
/// ⚠️ This is the **only** way an interrupt `mcause` is built: the
/// architectural code comes from [`arch_cause_for_line`] and the
/// interrupt bit from [`IRQ_BASE`].  The **internal line number is
/// never OR-ed in** — it is a different numbering space.
pub fn arch_mcause_for_line(line: u16) -> Option<u32> {
    arch_cause_for_line(line).map(|code| IRQ_BASE | code)
}

/// Whether a CSR address is **read-only** by the RISC-V address rule.
///
/// ⚠️ **This function used to be wrong, and the wrongness was a hard
/// blocker** (ledger **D-94**, batch v0.36 / spec `STAGE-20`).
///
/// The old implementation was
///
/// ```text
/// csr & 0x800 != 0
/// ```
///
/// — "bit 11 set ⇒ read-only".  That is **not** the RISC-V rule, and it
/// contradicts the very table in this function's own doc comment.  The
/// real rule is the **CSR address mapping** (Privileged Spec): read-only
/// ranges are
///
/// | range | meaning | access |
/// | --- | --- | --- |
/// | `0xC00..0xC7F` | machine-level read-only, standard | **read-only** |
/// | `0xC80..0xCFF` | machine-level read-only, custom | **read-only** |
/// | `0x900..0x9FF` | user-level read-only, custom | **read-only** |
/// | `0xA20..0xA3F` | machine-level read-only, standard | **read-only** |
/// | `0xAA0..0xABF` | machine-level read-only, standard | **read-only** |
/// | `0xB20..0xB3F` | machine-level read-only, custom | **read-only** |
/// | `0xBA0..0xBBF` | machine-level read-only, custom | **read-only** |
///
/// and the machine-level **read/write** ranges are `0x000..0x0FF`,
/// `0x200..0x3FF`, `0x400..0x7FF`, `0x800..0x8FF`, `0xA00..0xA1F`,
/// `0xA80..0xA9F`, `0xAC0..0xAFF`, `0xB00..0xB1F`, `0xB80..0xB9F`,
/// `0xBC0..0xBFF` …
///
/// ⇒ ⇒ **the old bit-11 test wrongly made `0x800..0xBFF` read-only**,
/// which included the Zicntr machine counters `mcycle` (`0xB00`),
/// `minstret` (`0xB02`), `mcycleh` (`0xB80`) and `minstreth` (`0xB82`)
/// ⇒ ⇒ a guest could not write them at all ⇒ ⇒ **the counters could
/// not have been implemented at all**.
///
/// ⚠️ **`mtime` is the one place where the address rule and the
/// architecture disagree**: `0xBFF8` lands in `0xBC0..0xBFF`, which the
/// address mapping makes **read/write** — but `mtime` is
/// **architecturally read-only** (it is the CLINT's timer register, not
/// a plain CSR).  Its read-only-ness therefore lives in the **platform**
/// list, not in the address rule — see [`csr_is_read_only`].
///
/// Consequences on this platform after the fix: `mcycle` / `minstret` /
/// `mcycleh` / `minstreth` / `mcountinhibit` are **writable**; the
/// Zicntr shadows `cycle` / `time` / `instret` / `cycleh` / `timeh` /
/// `instreth` are **read-only**; `mip` (`0x344`) is *writable by this
/// rule* (see [`csr_write_is_ignored`] for the platform's extra rule),
/// and the platform's `irq_clear` (`0x7C0`) is writable — it sits in the
/// read-write **custom** quarter `0x7C0..0x7FF`.
pub fn is_csr_read_only(csr: u16) -> bool {
    // ⚠️ **bits `[11:10] == 0b11` ⇒ read-only.**  This single test covers
    // `0xC00..0xFFF` (machine-level read-only, standard `0xC00..0xC7F` and
    // custom `0xC80..0xCFF`) **and** the `0xBC0..0xBFF` custom read-only
    // slice — which is why `mtime` (`0xBFF8`) is read-only **by the rule
    // itself**, with no platform exception needed.
    if csr & 0xC00 == 0xC00 {
        return true;
    }
    match csr & 0xF00 {
        // 0x900..=0x9FF —— **supervisor-level read/write** (custom).
        //
        // ⚠️ **STAGE-22 订正**：这一段**不是**只读区 —— 特权规范的地址映射
        // 里 bits `[11:10] == 0b10` 是「监管者级**可读写**」⇒ ⇒
        // `scounteren`（`0x906`）**必须可写**（实测 F19：置位之后 S 模式才
        // 能读 `cycle`）⇒ ⇒ 把它当只读 ⇒ ⇒ 那次写被**吞掉** ⇒ ⇒ 使能链
        // **永远接不上**。
        0x900 => false,
        // 0xA00..=0xAFF —— machine-level standard: `0xA20..=0xA3F` and
        // `0xAA0..=0xABF` are read-only, the rest writable.
        // ⚠️ The mask is `0xFE0` (bits `[11:5]`, 0x20 granularity), **not**
        // `0x0F0`: the `0xA20..=0xA3F` slice spans **32** addresses and
        // they do **not** share one `0x0F0` value.
        0xA00 => matches!(csr & 0xFE0, 0xA20 | 0xAA0),
        // 0xB00..=0xBFF —— machine-level custom: `0xB20..=0xB3F` and
        // `0xBA0..=0xBBF` are read-only, the rest writable.
        0xB00 => matches!(csr & 0xFE0, 0xB20 | 0xBA0),
        // 0x800..=0x8FF is user-level **read/write** custom — writable.
        _ => false,
    }
}

/// The half-pair of a **Zicntr machine counter**.
///
/// The platform's counters are **64 bits wide** and the RISC-V CSR
/// numbering splits each of them into a **low** and a **high** CSR:
///
/// | counter | low half | high half |
/// | --- | --- | --- |
/// | cycle | [`CSR_MCYCLE`] | [`CSR_MCYCLEH`] |
/// | instret | [`CSR_MINSTRET`] | [`CSR_MINSTRETH`] |
///
/// ⚠️ **Both halves are writable and they are independent** — writing
/// the high half does not disturb the low one (measured on the
/// reference side).
///
/// ⇒ ⇒ This mapping is the **single landing spot** for "which two CSRs
/// are the two halves of this counter" — the ISS must **not** write its
/// own match on this.
pub fn counter_pair(csr: u16) -> Option<(u16, u16)> {
    match csr {
        CSR_MCYCLE | CSR_MCYCLEH => Some((CSR_MCYCLE, CSR_MCYCLEH)),
        CSR_MINSTRET | CSR_MINSTRETH => Some((CSR_MINSTRET, CSR_MINSTRETH)),
        _ => None,
    }
}

/// The machine counter a **user-visible shadow** reports.
///
/// Zicntr's user-level counters (`cycle` / `instret` and their high
/// halves) are **shadows**: they report the machine counter's value
/// rather than holding a copy of it.
///
/// ⚠️ **The shadow is *derived*, never stored** — there is no second
/// copy of the counter in the CSR file, so the two can never drift.
///
/// ⚠️ **`time` / `timeh` are deliberately absent**: this platform reads
/// them as `0` (the reference side does too), because a real `time`
/// needs the M-15 CLINT wired into the CSR read path.
pub fn counter_shadow(csr: u16) -> Option<u16> {
    match csr {
        CSR_CYCLE => Some(CSR_MCYCLE),
        CSR_CYCLEH => Some(CSR_MCYCLEH),
        CSR_INSTRET => Some(CSR_MINSTRET),
        CSR_INSTRETH => Some(CSR_MINSTRETH),
        _ => None,
    }
}
// ---------------------------------------------------------------------------
// PMP — physical memory protection (STAGE-21 · v0.37)
// ---------------------------------------------------------------------------
//
// ⚠️ **Every number in this block was measured against the reference side
// first** (frozen spec §1.2, facts F1..F19) — none of them is guessed:
//
// * `pmpcfg`'s byte layout is `L` = bit 7, `A` = bits 4:3, `X` = bit 2,
//   `W` = bit 1, `R` = bit 0 (F1) — **the same order upstream riscv-tests'
//   `env/encoding.h` uses** ⇒ ⇒ that file needs **no** change;
// * RV32's `pmpcfg{i}` holds **four** 8-bit entries (F2) ⇒ ⇒ entry *n* lives
//   in `pmpcfg[n / 4]`, byte `n % 4`;
// * `pmpaddr` stores the address **shifted right by two** (F3) ⇒ ⇒ the
//   granularity is four bytes.

/// `pmpcfg0` — PMP configuration of entries 0..3.
pub const CSR_PMPCFG0: u16 = 0x3A0;
/// `pmpcfg3` — PMP configuration of entries 12..15.
pub const CSR_PMPCFG3: u16 = 0x3A3;
/// `pmpaddr0` — PMP address register of entry 0.
pub const CSR_PMPADDR0: u16 = 0x3B0;
/// `pmpaddr15` — PMP address register of entry 15.
pub const CSR_PMPADDR15: u16 = 0x3BF;

/// How many PMP entries the platform implements (F15).
///
/// ⚠️ The reference side implements **16** entries for RV32 ⇒ ⇒ we match it.
/// A 64-entry implementation would be a superset, but every entry beyond 16
/// would be a **divergence** from the oracle the golden corpus compares
/// against ⇒ ⇒ the platform deliberately stops at 16.
pub const PMP_ENTRY_COUNT: usize = 16;

/// `pmpaddr` holds the address shifted right by this many bits (F3).
pub const PMP_GRAINULARITY_SHIFT: u32 = 2;

/// `pmpcfg`'s lock bit (F1).
pub const PMP_CFG_L: u8 = 0x80;
/// `pmpcfg`'s address-matching mode field (F1).
pub const PMP_CFG_A_MASK: u8 = 0x18;
/// Shift of the address-matching mode field (F1).
pub const PMP_CFG_A_SHIFT: u32 = 3;
/// `pmpcfg`'s execute permission (F1).
pub const PMP_CFG_X: u8 = 0x04;
/// `pmpcfg`'s write permission (F1).
pub const PMP_CFG_W: u8 = 0x02;
/// `pmpcfg`'s read permission (F1).
pub const PMP_CFG_R: u8 = 0x01;
/// `pmpcfg`'s **reserved** bits 6:5 — they read back as zero (F20).
pub const PMP_CFG_RESERVED: u8 = 0x60;

/// Address-matching mode: the entry is **disabled** (F1).
pub const PMP_A_OFF: u8 = 0;
/// Address-matching mode: **top of range** (F1).
pub const PMP_A_TOR: u8 = 1;
/// Address-matching mode: **naturally aligned four bytes** (F1).
pub const PMP_A_NA4: u8 = 2;
/// Address-matching mode: **naturally aligned power of two** (F1).
pub const PMP_A_NAPOT: u8 = 3;

/// The three kinds of access PMP distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmpAccess {
    /// A load — needs the `R` permission.
    Load,
    /// A store — needs the `W` permission.
    Store,
    /// An instruction fetch — needs the `X` permission.
    Fetch,
}

/// One decoded `pmpcfg` entry (F1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PmpCfg {
    /// The lock bit: the entry's permissions are enforced in **every**
    /// privilege mode, and its registers stop accepting writes (F12 / F13).
    pub l: bool,
    /// The address-matching mode ([`PMP_A_OFF`] .. [`PMP_A_NAPOT`]).
    pub a: u8,
    /// Read permission.
    pub r: bool,
    /// Write permission.
    pub w: bool,
    /// Execute permission.
    pub x: bool,
}

/// Decode one 8-bit `pmpcfg` entry (F1).
pub fn pmp_decode_cfg(byte: u8) -> PmpCfg {
    PmpCfg {
        l: byte & PMP_CFG_L != 0,
        a: (byte >> PMP_CFG_A_SHIFT) & (PMP_CFG_A_MASK >> PMP_CFG_A_SHIFT),
        r: byte & PMP_CFG_R != 0,
        w: byte & PMP_CFG_W != 0,
        x: byte & PMP_CFG_X != 0,
    }
}

/// The address range one PMP entry covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PmpRegion {
    /// The first byte of the range.
    pub base: u64,
    /// The length of the range, in bytes.
    pub size: u64,
}

impl PmpRegion {
    /// Does the range cover **every** byte of the access at `addr`?
    ///
    /// ⚠️ The spec requires the deciding entry to match **all** bytes of the
    /// access ⇒ ⇒ a straddling access is denied unless one entry covers it
    /// completely.
    pub fn covers(&self, addr: u32, size: u32) -> bool {
        let start = u64::from(addr);
        let end = start + u64::from(size);
        start >= self.base && end <= self.base + self.size
    }
}

/// The address range a PMP entry covers, as `None` when it matches **nothing**.
///
/// * `prev_addr` — the **previous** entry's `pmpaddr` (zero for entry 0);
///   the spec ignores the previous entry's *mode* here, so an `OFF` entry
///   still provides the bound.
/// * `cur_addr` — this entry's `pmpaddr`.
///
/// ⚠️ **The three modes differ in more than their formula** (F4 / F5 / F6):
///
/// * **NAPOT** — `n` = the number of trailing ones in `pmpaddr`; the range is
///   `[(pmpaddr & !(2^n - 1)) << 2, + 2^(n+3))` — **half open**.
/// * **NA4** — `pmpaddr << 2`, **four bytes**, **closed**.
/// * **TOR** — `[prev_addr << 2, cur_addr << 2)` — **half open**; an entry
///   whose lower bound is **not** below its upper bound matches nothing.
pub fn pmp_region(cfg: PmpCfg, prev_addr: u32, cur_addr: u32) -> Option<PmpRegion> {
    match cfg.a {
        PMP_A_OFF => None,
        PMP_A_NA4 => Some(PmpRegion {
            base: u64::from(cur_addr) << PMP_GRAINULARITY_SHIFT,
            size: 4,
        }),
        PMP_A_NAPOT => {
            let n = cur_addr.trailing_ones();
            let mask = if n >= u32::BITS {
                u32::MAX
            } else {
                (1u32 << n) - 1
            };
            Some(PmpRegion {
                base: u64::from(cur_addr & !mask) << PMP_GRAINULARITY_SHIFT,
                size: 1u64 << (n + 3),
            })
        }
        PMP_A_TOR => {
            if cur_addr <= prev_addr {
                return None;
            }
            Some(PmpRegion {
                base: u64::from(prev_addr) << PMP_GRAINULARITY_SHIFT,
                size: (u64::from(cur_addr) - u64::from(prev_addr)) << PMP_GRAINULARITY_SHIFT,
            })
        }
        _ => None,
    }
}

/// The **WARL legalisation** of one `pmpcfg` byte (F20).
///
/// ⚠️ The reference side does **not** store the written byte verbatim:
///
/// * the **reserved** bits 6:5 read back as zero (`0xff` ⇒ `0x9f`);
/// * **`R` = 0 together with `W` = 1 is a reserved combination** ⇒ ⇒ the
///   reference side **drops the `W` bit** (`0x1a` ⇒ `0x18`, `0x2a` ⇒ `0x08`)
///   ⇒ ⇒ and *that* is why such an entry cannot write at all (F9): the `W`
///   bit is not even there any more.
///
/// ⇒ ⇒ everything else reads back verbatim (`0x01`, `0x04`, `0x1f`, …).
pub fn pmp_cfg_warl(byte: u8) -> u8 {
    let mut value = byte & !PMP_CFG_RESERVED;
    if value & PMP_CFG_R == 0 {
        value &= !PMP_CFG_W;
    }
    value
}

/// Does the entry grant the access?
///
/// ⚠️ **`R` = 0 together with `W` = 1 is a reserved combination** (F9) ⇒ ⇒ the
/// reference side denies such a store ⇒ ⇒ so do we: the spec calls the
/// combination reserved, and "no write permission" is the only defensible
/// reading of a reserved encoding.
pub fn pmp_permits(cfg: PmpCfg, access: PmpAccess) -> bool {
    match access {
        PmpAccess::Load => cfg.r,
        PmpAccess::Store => cfg.w && cfg.r,
        PmpAccess::Fetch => cfg.x,
    }
}

/// The whole PMP decision for one access.
///
/// * `entries` — `(cfg, pmpaddr)` per entry, **in entry order** (entry 0
///   first); the slice may be shorter than [`PMP_ENTRY_COUNT`].
/// * `m_mode_bypass` — the caller states that PMP does **not** constrain
///   this access: an M-mode load/store with `mstatus.MPRV` = 0.  ⚠️ It is
///   **never** set for a fetch — `MPRV` does not affect instruction fetch
///   (F11).  A **locked** entry constrains the access anyway (F12).
///
/// The decision chain is: **find the deciding entry** (the lowest-numbered
/// entry that covers every byte of the access — F7) ⇒ ⇒ **privilege**
/// (F10 / F11 / F12) ⇒ ⇒ **permission bits** (F9).
pub fn pmp_access_allowed(
    entries: &[(PmpCfg, u32)],
    addr: u32,
    size: u32,
    access: PmpAccess,
    m_mode_bypass: bool,
) -> bool {
    let mut prev_addr = 0u32;
    for (cfg, cur_addr) in entries {
        if let Some(region) = pmp_region(*cfg, prev_addr, *cur_addr) {
            if region.covers(addr, size) {
                // M-mode is exempt unless the deciding entry is **locked**.
                if m_mode_bypass && !cfg.l {
                    return true;
                }
                return pmp_permits(*cfg, access);
            }
        }
        prev_addr = *cur_addr;
    }
    // No entry matched the access.
    if m_mode_bypass {
        // "If no PMP entry matches an M-mode effective address, the access
        // succeeds."
        return true;
    }
    // "If no PMP entry matches an S-mode or U-mode effective address, but at
    // least one PMP entry is implemented, the access fails." (F8)
    !entries.iter().any(|(cfg, _)| cfg.a != PMP_A_OFF)
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
/// ⚠️ The read-only range (`0xC00..0xFFF`) is **not** uniformly
/// "unimplemented" any more: `mhartid` **is** implemented (it answers
/// [`CSR_MHARTID_VALUE`] — the single hart of this single-core
/// platform).
///
/// ⚠️ **The Zicntr machine counters are implemented too** (batch v0.36,
/// spec `STAGE-20`): `mcycle` / `mcycleh` / `minstret` / `minstreth`
/// are writable and hold the two halves of two 64-bit counters, and
/// `mcountinhibit` is writable storage.  ⚠️ **The user-level shadows
/// (`cycle` / `instret` / `cycleh` / `instreth` / `time` / `timeh`) are
/// deliberately NOT in this list** — they are **read-only by the
/// address rule**, so they are already rejected by
/// [`csr_write_is_ignored`] without needing an entry here.
///
/// ⚠️ **Being implemented does not make a CSR writable**: `mhartid` is
/// read-only by the RISC-V address rule (`0xF14` is in `0xC00..0xFFF`),
/// so [`csr_write_is_ignored`] rejects the write **anyway** — the two
/// rules agree, which is what keeps the value stable.
///
/// ⚠️ `medeleg` / `mideleg` are **not** in the list: delegation is an
/// S-mode feature and this platform is M-mode only ⇒ a guest write to
/// them is **ignored** and they read back 0.  The reference side (with
/// its default `--priv=MSU`) **does** implement delegation ⇒ the two
/// sides differ here (D-94).
pub fn csr_is_implemented(csr: u16) -> bool {
    // ⚠️ **PMP 的 CSR 必须被认作已实现**（裁决 E）—— 否则
    // [`csr_write_is_ignored`] 会把写吞掉 ⇒ ⇒ PMP 变成一排**不起作用的寄存器**。
    if (CSR_PMPCFG0..=CSR_PMPCFG3).contains(&csr) || (CSR_PMPADDR0..=CSR_PMPADDR15).contains(&csr) {
        return true;
    }
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
            | CSR_MHARTID
            | CSR_MCOUNTINHIBIT
            | CSR_MCYCLE
            | CSR_MCYCLEH
            | CSR_MINSTRET
            | CSR_MINSTRETH
            // ---- S-mode / U-mode (STAGE-22 · 裁决 F) ----
            | CSR_SSTATUS
            | CSR_SIE
            | CSR_STVEC
            | CSR_SSCRATCH
            | CSR_SEPC
            | CSR_SCAUSE
            | CSR_STVAL
            | CSR_SIP
            | CSR_SATP
            | CSR_MEDELEG
            | CSR_MIDELEG
            | CSR_MCOUNTEREN
            | CSR_SCOUNTEREN
            | CSR_UCOUNTEREN
            // ⚠️ **伪 CSR 必须被认作已实现**（STAGE-22 裁决 F）—— 否则
            // 快照的 `csrs.retain(csr_is_implemented)` 会把它**丢掉** ⇒ ⇒
            // 还原之后**特权级归零** ⇒ ⇒ 快照不往返。
            | CSR_PRIV
    )
}

/// Whether a CSR is **read-only** for the guest.
///
/// That is the case when the **address rule** makes it read-only, **or**
/// when the platform lists it as read-only on top of that (`misa`).
///
/// ⚠️ **`mtime` does NOT need a platform entry**: its bits `[11:10]` are
/// `0b11` ⇒ the **address rule itself** makes it read-only (it lands in
/// the `0xBC0..0xBFF` custom read-only slice).  That is what keeps the
/// address-rule fix ([`is_csr_read_only`]) **behaviour-preserving** for
/// it.
///
/// ⚠️ `mip` is *not* here: it is writable **by the address rule**, but
/// RISC-V makes `MEIP` / `MTIP` **read-only in M-mode** — only the
/// platform may raise them, so a guest writing `mip` must not be able to
/// fake a pending interrupt.  That is the **platform's** rule, and it
/// lives in [`csr_write_is_ignored`] with `mip`'s own line of defence.
pub fn csr_is_read_only(csr: u16) -> bool {
    // ⚠️ **伪 CSR `CSR_PRIV` 必须在这里**（STAGE-22 订正 6）—— 否则客体的
    // 写会**真的落进** CSR 文件 ⇒ ⇒ guest 能改自己的特权级。
    // ⚠️ 靠「未实现 ⇒ 写被忽略」是**假的**：裁决 F 要求 `CSR_PRIV` 进
    // [`csr_is_implemented`]（否则快照不往返）⇒ ⇒ 那时
    // `!csr_is_implemented` 那一条**不再成立** ⇒ ⇒ 必须有**这一条**。
    is_csr_read_only(csr) || csr == CSR_MISA || csr == CSR_PRIV
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

    /// ⚠️ **`mtime` 的只读来自**地址规则**本身** —— 不是平台清单。
    ///
    /// `0xBFF8` 的 bits `[11:10]` 就是 `0b11` ⇒ 它落在只读区
    /// `0xBC0..0xBFF`（custom read-only）⇒ ⇒ 规则**自己**就拦它。
    ///
    /// ⇒ ⇒ 这条用例同时是**判别力**：若有人把规则改成「只判
    /// `0xC00..0xFFF`」（漏掉 `0xBC0..0xBFF`），`mtime` 会变**可写**
    /// ⇒ ⇒ 这里会**红** ⇒ ⇒ 那正是本批修掉的那个 bug 的形状。
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
    /// 的形式，平台用到的 CSR **恰好**都判成一样，于是**没有一条用例会红**：
    /// 那种改法看上去「无害」，实际是把标准里**可写**的区间悄悄判成只读。
    ///
    /// ⚠️ **这条用例在本批被重写**：旧版本把 `0x800..0xBFF` **一律**当
    /// 只读（地址 bit 11 置位）—— 而那正是本批修掉的 bug。旧版本之所以
    /// 长期没红，是因为平台用到的 CSR **一个都不落在** `0x800..0xBFF`
    /// 的**可写**子区间上（`mtime` 在 `0xBC0..0xBFF`，按标准本来就
    /// 只读… ⚠️ 不，按标准它是**可写**区，只读性来自**架构**）⇒ ⇒
    /// 直到本批要实现 `mcycle`（`0xB00`，标准里的**可写**区）才暴露。
    ///
    /// ⇒ ⇒ 现在按**标准 CSR 地址映射**逐区间钉死。
    #[test]
    fn the_read_only_rule_is_pinned_at_the_spec_boundaries() {
        // `0x000..0x7FF` —— 用户级 / 监管级：一律**可写**。
        assert!(!is_csr_read_only(0x0000));
        assert!(!is_csr_read_only(0x03FF));
        assert!(!is_csr_read_only(0x0400));
        assert!(!is_csr_read_only(0x07FF));
        // `0x800..0x8FF` —— 用户级 custom **可写**。
        assert!(!is_csr_read_only(0x0800));
        assert!(!is_csr_read_only(0x08FF));
        // `0x900..0x9FF` —— **监管者级 custom 可写**（STAGE-22 订正）。
        // ⚠️ **STAGE-22 订正**：`0x0900..=0x09FF` 是**监管者级可读写**，
        // 不是只读区 ⇒ ⇒ `scounteren`（`0x906`）必须能写（F19）。
        assert!(!is_csr_read_only(0x0900));
        assert!(!is_csr_read_only(CSR_SCOUNTEREN));
        assert!(!is_csr_read_only(0x09FF));
        // `0xA00..0xA1F` 可写 · `0xA20..0xA3F` 只读 ·
        // `0xA80..0xA9F` 可写 · `0xAA0..0xABF` 只读。
        assert!(!is_csr_read_only(0x0A00));
        assert!(!is_csr_read_only(0x0A1F));
        assert!(is_csr_read_only(0x0A20));
        assert!(is_csr_read_only(0x0A3F));
        assert!(!is_csr_read_only(0x0A80));
        assert!(!is_csr_read_only(0x0A9F));
        assert!(is_csr_read_only(0x0AA0));
        assert!(is_csr_read_only(0x0ABF));
        // `0xB00..0xB1F` 可写 · `0xB20..0xB3F` 只读 ·
        // `0xB80..0xB9F` 可写 · `0xBA0..0xBBF` 只读。
        assert!(!is_csr_read_only(0x0B00));
        assert!(!is_csr_read_only(0x0B1F));
        assert!(is_csr_read_only(0x0B20));
        assert!(is_csr_read_only(0x0B3F));
        assert!(!is_csr_read_only(0x0B80));
        assert!(!is_csr_read_only(0x0B9F));
        assert!(is_csr_read_only(0x0BA0));
        assert!(is_csr_read_only(0x0BBF));
        // `0xC00..0xFFF` —— 机器级：**一律只读**。
        assert!(is_csr_read_only(0x0C00));
        assert!(is_csr_read_only(0x0C7F));
        assert!(is_csr_read_only(0x0C80));
        assert!(is_csr_read_only(0x0FFF));
    }

    /// ⚠️ **本批改了只读规则 ⇒ 既有 CSR 的分类必须**逐条**不变。**
    ///
    /// 旧版本（`the_coarser_read_only_rule_does_not_affect_any_csr_the_platform_uses`）
    /// 钉的是「粗规则**无影响**」—— 现在规则不粗了 ⇒ ⇒ 那条用例的前提
    /// 没了 ⇒ ⇒ 改成**回归**：既有 9 个 CSR 的「写是否被忽略」必须与
    /// RISC-V **逐条**一致。
    ///
    /// ⚠️ 这里比的是**最终效果**（写会不会被忽略），不是地址规则本身 ——
    /// `mip` 按地址规则是**可写**的（`0x344`），是**平台规则**把它判成
    /// 只读的（RISC-V：MEIP/MTIP 在 M 模式只读）；`mtime` 按地址规则也是
    /// **可写**的（`0xBC0..0xBFF`），是**架构 + 平台清单**把它判成只读的。
    #[test]
    fn the_read_only_rule_change_does_not_affect_the_pre_existing_csrs() {
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
    // v0.23 批 —— 内部线号 → 架构 cause 码（第三个编号空间）
    // -----------------------------------------------------------------------

    /// ⚠️ **架构 cause 码与内部线号是**两个**编号空间** —— 这条用例就是
    /// 那条护栏。
    ///
    /// 平台的内部线号是 `1 / 2 / 3`，而 RISC-V 的机器中断 cause 码是
    /// `3`（软件）/ `7`（定时器）/ `11`（外部）。
    ///
    /// ⚠️ **断言值写架构字面量**（`3` / `7` / `11`），**不是**从映射函数
    /// 反推的 ⇒ 改了映射而忘了改语义，这条会红。
    ///
    /// ⚠️ **两者的数字必须**不相等**：如果哪天有人把「定时器线」的架构
    /// 码写成 3（= 内部线号），这条会红 —— 那正是本批要修的缺陷本身
    /// （`mcause` 报的是内部线号 ⇒ guest 一个都匹配不上）。
    #[test]
    fn the_architectural_cause_codes_are_not_the_internal_line_numbers() {
        // 内部线号 —— 平台自己的空间，**一个字都不能改**。
        assert_eq!(IRQ_UART_RX_READY, 1, "内部线号：UART rx-ready");
        assert_eq!(IRQ_UART_TX_EMPTY, 2, "内部线号：UART tx-empty");
        assert_eq!(IRQ_TIMER_COMPARE, 3, "内部线号：timer-compare");

        // 架构 cause 码 —— RISC-V 的空间。
        assert_eq!(CAUSE_MACHINE_SOFTWARE, 3, "架构码：机器软件中断");
        assert_eq!(CAUSE_MACHINE_TIMER, 7, "架构码：机器定时器中断");
        assert_eq!(CAUSE_MACHINE_EXTERNAL, 11, "架构码：机器外部中断");

        // ⚠️ **护栏本体**：定时器线的**内部线号**是 3，而它的**架构码**
        // 是 7 —— 两者**必须不相等**。
        assert_eq!(IRQ_TIMER_COMPARE, 3);
        assert_eq!(arch_cause_for_line(IRQ_TIMER_COMPARE), Some(7));
        assert_ne!(
            u32::from(IRQ_TIMER_COMPARE),
            arch_cause_for_line(IRQ_TIMER_COMPARE).expect("定时器线有架构码"),
            "内部线号与架构 cause 码**不能**是同一个数字"
        );
        // UART 线同理：内部 1 / 2，架构 11。
        assert_ne!(
            u32::from(IRQ_UART_RX_READY),
            arch_cause_for_line(IRQ_UART_RX_READY).expect("UART 线有架构码"),
            "内部线号与架构 cause 码**不能**是同一个数字"
        );
    }

    /// 映射表本身：**架构码**。
    ///
    /// * 定时器线 ⇒ **7**（机器定时器中断）
    /// * 两条 UART 线 ⇒ **11**（机器外部中断）—— M 模式只有**一个**外部
    ///   中断位，两条 UART 线与 `MEIP` 一样共享它
    /// * 其它线 ⇒ `None`（**没有**架构 cause 码 ⇒ 不得交付给 guest）
    #[test]
    fn the_arch_cause_map_speaks_the_riscv_codes() {
        assert_eq!(arch_cause_for_line(IRQ_TIMER_COMPARE), Some(7));
        assert_eq!(arch_cause_for_line(IRQ_UART_RX_READY), Some(11));
        assert_eq!(arch_cause_for_line(IRQ_UART_TX_EMPTY), Some(11));
        // 没有架构 cause 码的线 —— 一律 `None`。
        assert_eq!(arch_cause_for_line(0), None);
        assert_eq!(arch_cause_for_line(9), None);
        assert_eq!(arch_cause_for_line(77), None);
    }

    /// 交付给 guest 的 **`mcause`** = 中断位 | **架构码**。
    ///
    /// ⚠️ **断言值是架构字面量**（`0x8000_0007` / `0x8000_000B`）——
    /// 一个真实 M 模式 RTOS 的中断分发就是按这两个数字写的。
    #[test]
    fn the_full_mcause_is_the_interrupt_bit_plus_the_architectural_code() {
        assert_eq!(
            arch_mcause_for_line(IRQ_TIMER_COMPARE),
            Some(0x8000_0007),
            "定时器中断 ⇒ mcause = 0x8000_0007（架构码 7）"
        );
        assert_eq!(
            arch_mcause_for_line(IRQ_UART_RX_READY),
            Some(0x8000_000B),
            "外部中断 ⇒ mcause = 0x8000_000B（架构码 11）"
        );
        assert_eq!(
            arch_mcause_for_line(IRQ_UART_TX_EMPTY),
            Some(0x8000_000B),
            "外部中断 ⇒ mcause = 0x8000_000B（架构码 11）"
        );
        assert_eq!(arch_mcause_for_line(9), None);
    }

    /// ⚠️ **新增 cause 映射**不许**动** `mie` / `mip` 的位号映射。
    ///
    /// `mip` 投影是 D-84 / D-100 已闭环的语义 ⇒ 位号表必须**逐字节**保持。
    /// 这条用例把位号**钉死**（架构字面量 `11` / `7`），并同时钉住
    /// 「挂起定时器线 ⇒ bit 7、挂起 UART 线 ⇒ bit 11」。
    #[test]
    fn the_mie_bit_map_is_untouched_by_the_cause_map() {
        // 位号表本身。
        assert_eq!(std_irq_bit_for_line(IRQ_UART_RX_READY), Some(11));
        assert_eq!(std_irq_bit_for_line(IRQ_UART_TX_EMPTY), Some(11));
        assert_eq!(std_irq_bit_for_line(IRQ_TIMER_COMPARE), Some(7));
        assert_eq!(std_irq_bit_for_line(0), None);
        // `mip` 投影。
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_TIMER_COMPARE]),
            1 << 7,
            "挂起定时器线 ⇒ mip 的 bit 7 置位"
        );
        assert_eq!(
            pending_lines_to_mip_mask(&[IRQ_UART_RX_READY]),
            1 << 11,
            "挂起 UART 线 ⇒ mip 的 bit 11 置位"
        );
        // ⚠️ **RISC-V 有意让 `mie` / `mip` 的位号 = 中断的 cause 号**
        // （MTIE 的位 7 = 定时器 cause 7；MEIE 的位 11 = 外部 cause 11）
        // ⇒ 这两个空间**数字上重合**，但它们**不是**平台的内部线号空间
        // （`1 / 2 / 3`）。⇒ 平台报 cause 时**必须**查 cause 映射表，
        // 不能拿内部线号凑 —— 那正是本批修掉的缺陷。
        assert_eq!(MIE_MTIE_BIT, 7);
        assert_eq!(MIE_MEIE_BIT, 11);
        assert_eq!(MIE_MTIE_BIT, CAUSE_MACHINE_TIMER);
        assert_eq!(MIE_MEIE_BIT, CAUSE_MACHINE_EXTERNAL);
    }

    // -----------------------------------------------------------------------
    // 裁决 D —— 「已实现 CSR 表」
    // -----------------------------------------------------------------------

    /// 已实现清单**必须恰好**是平台真的有的那批 CSR。
    ///
    /// ⚠️ 清单外的 CSR（`tdata1` / `medeleg` / …）写必须被忽略、读回 0
    /// —— 与参考侧一致（黄金对拍实测发现：参考侧写 `tdata1` 读回 0）。
    ///
    /// ⚠️ **`mhartid` 现在算已实现**（v0.23 批）：平台是**单核** ⇒
    /// `mhartid` 恒为 **0**，而那个 0 必须是**设计**出来的（常量），不是
    /// 「没实现 ⇒ 碰巧读回 0」的运气。
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
            CSR_MHARTID,
            // ⚠️ Zicntr（batch v0.36）：机器态计数器**已实现** ——
            // 之前它们在这个清单里是「未实现」⇒ 写被忽略 ⇒ 计数器根本
            // 写不进去。
            CSR_MCOUNTINHIBIT,
            CSR_MCYCLE,
            CSR_MCYCLEH,
            CSR_MINSTRET,
            CSR_MINSTRETH,
        ] {
            assert!(csr_is_implemented(csr), "csr {csr:#x} 必须算已实现");
        }
        // ⚠️ 明确**不在**清单里的那几个 —— 每一个都有理由。
        for (csr, why) in [
            (0x7A1u16, "tdata1 —— 调试器 CSR，本平台没有"),
            (0x7A2u16, "tdata2 —— 调试器 CSR，本平台没有"),
            (0x308u16, "mstatush —— 本平台不实现 64 位的高半部 mstatus"),
            (
                0xC00u16,
                "cycle —— 用户态影子，它是**推导量**（读 mcycle）不是存储",
            ),
            (
                0xC02u16,
                "instret —— 用户态影子，它是**推导量**（读 minstret）不是存储",
            ),
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
    ///
    /// ⚠️ **`mhartid` 不在这张表里**（v0.23 批）：它**已实现**，但**只读**
    /// —— 写照样被忽略，只是理由从「没实现」换成了「地址规则只读」。
    #[test]
    fn an_unimplemented_csr_ignores_the_write() {
        // ⚠️ `0xB00`（`mcycle`）**不在**这个列表里了 —— 它在本批被
        // **实现**了（Zicntr 计数器，见 `STAGE-20`）。⇒ 这里换成
        // `0x306`（`mcounteren`）—— 本平台没有 U 模式 ⇒ 未实现。
        // ⚠️ `0x306`（`mcounteren`）/ `0x302`（`medeleg`）/ `0x303`
        // （`mideleg`）**不在**这个列表里了 —— v0.38 起它们**已实现**
        // （STAGE-22 裁决 F）。
        for csr in [0x7A1u16, 0x7A2, 0x308, 0x30A, 0x30C] {
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

    /// `mhartid`：**已实现**（单核 ⇒ 恒为 0）但**只读**。
    ///
    /// ⚠️ **两条规则必须同时成立**：实现 ⇒ 平台**答得出**这个 CSR 的值；
    /// 只读 ⇒ guest **改不了**它（`0xF14` 在只读区 `>= 0x800`）。
    ///
    /// ⚠️ 断言值用**架构字面量**：`0xF14` 是 `mhartid` 的标准编号，
    /// **0** 是单核平台的值 —— 不是从任何映射函数反推的。
    #[test]
    fn mhartid_is_implemented_and_read_only() {
        assert_eq!(CSR_MHARTID, 0xF14, "mhartid 的标准 CSR 编号");
        assert_eq!(CSR_MHARTID_VALUE, 0, "单核平台 ⇒ hart 0");
        assert!(
            csr_is_implemented(CSR_MHARTID),
            "mhartid 已实现 ⇒ 平台必须答得出它的值"
        );
        assert!(
            is_csr_read_only(CSR_MHARTID),
            "mhartid 在只读区 ⇒ 地址规则本身必须拦它的写"
        );
        assert!(csr_write_is_ignored(CSR_MHARTID), "mhartid 的写必须被忽略");
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
        // 已实现（v0.38 起）：S（bit 18）、U（bit 20）⇒ ⇒ 与参考侧**逐位
        // 相同**（STAGE-22 · F3 / F24 / F27）。
        assert_ne!(CSR_MISA_VALUE & (1 << 18), 0, "S 扩展位（v0.38 起已实现）");
        assert_ne!(CSR_MISA_VALUE & (1 << 20), 0, "U 扩展位（v0.38 起已实现）");
        // 未实现 ⇒ 位必须**清零**。
        for (bit, name) in [
            (0u32, "A（原子）"),
            (3, "D（双精度浮点）"),
            (5, "F（单精度浮点）"),
            (29, "IALIGN = 32"),
        ] {
            assert_eq!(
                CSR_MISA_VALUE & (1 << bit),
                0,
                "{name} 位（bit {bit}）必须清零 —— 平台没实现它"
            );
        }
    }

    // -----------------------------------------------------------------------
    // D-93 / D-98 —— 两个 WARL 掩码
    // -----------------------------------------------------------------------

    /// `mstatus` 的 WARL 掩码：**每一个保留位都不在里面**，而**每一个
    /// 已定义的字段都在里面**。
    ///
    /// ⚠️ 断言用的是**架构字面量**（`0x01` / `0x1800` / …），**不是**
    /// `MSTATUS_WARL_MASK` 自己 ⇒ 改了常量而忘了改语义，这条会红。
    #[test]
    fn the_mstatus_warl_mask_drops_the_reserved_bits_and_keeps_the_fields() {
        // ⚠️ **掩码是实测出来的**（STAGE-22 · F6 / **F40**）⇒ ⇒ 这里钉的是
        // **那个值**，不是「按规范推出来的值」。
        assert_eq!(
            MSTATUS_WARL_MASK, 0x007E_79AA,
            "mstatus 的**直接可写位**必须与参考侧逐位相同（F6 / F40）"
        );
        assert_eq!(
            MSTATUS_WARL_MASK & MSTATUS_SD_BIT,
            0,
            "SD 是推导位 ⇒ 不可直接写 ⇒ 必须在掩码之外（F40）"
        );
        // 保留位（WPRI）：0 / 2 / 4 / 23 / 24~30。
        for (bits, name) in [
            (0x0000_0001u32, "bit 0"),
            (0x0000_0004, "bit 2"),
            (0x0000_0010, "bit 4"),
            (0x0080_0000, "bit 23"),
            (0x7F00_0000, "bits 24~30"),
        ] {
            assert_eq!(
                MSTATUS_WARL_MASK & bits,
                0,
                "{name} 是保留位 ⇒ 必须在掩码之外"
            );
        }
        // 本平台**不支持**的扩展位 ⇒ 参考侧也写不进去（F6）⇒ ⇒ 必须**在
        // 掩码之外**：UBE（6）、VS（9:10）、XS 的高位（16）。
        for (bits, name) in [
            (0x0000_0040u32, "UBE"),
            (0x0000_0600, "VS"),
            (0x0001_0000, "XS 的高位（16）"),
        ] {
            assert_eq!(
                MSTATUS_WARL_MASK & bits,
                0,
                "{name} 是本平台不支持的扩展位 ⇒ 参考侧写不进去 ⇒ 必须在掩码之外（F6）"
            );
        }
        // 本平台**用到**的字段：一个都不能被掩掉。
        for (bits, name) in [
            (0x0000_0002u32, "SIE"),
            (0x0000_0008, "MIE"),
            (0x0000_0020, "SPIE"),
            (0x0000_0080, "MPIE"),
            (0x0000_0100, "SPP"),
            (0x0000_1800, "MPP"),
            (0x0002_0000, "MPRV"),
            (0x0004_0000, "SUM"),
            (0x0008_0000, "MXR"),
            (0x0010_0000, "TVM"),
            (0x0020_0000, "TW"),
            (0x0040_0000, "TSR"),
            (0x0000_6000, "FS"),
        ] {
            assert_eq!(
                MSTATUS_WARL_MASK & bits,
                bits,
                "{name} 是已定义的字段 ⇒ 必须在掩码之内"
            );
        }
        // ⚠️ **`SD` 是唯一的例外**：它是**推导位**（F40）⇒ ⇒ **不可直接写**
        // ⇒ ⇒ 不在掩码之内（上面的断言已经钉住）⇒ ⇒ 它的值由 `FS`/`XS` 推
        // 出来 ⇒ ⇒ 见 `mstatus_warl_derives_the_sd_bit_from_fs_and_xs`。
    }

    /// `mepc` 的对齐掩码：**只**去掉最低一位（IALIGN = 16，因为本平台
    /// 实现了 C 扩展），其余一位不少。
    ///
    /// ⚠️ 判据用**架构字面量**（`0x1` / `0xFFFFFFFE`），**不是**常量
    /// 自己 ⇒ 改了掩码而忘了改语义，这条会红。
    #[test]
    fn the_mepc_align_mask_drops_exactly_the_lowest_bit() {
        assert_eq!(
            MEPC_ALIGN_MASK & 0x0000_0001,
            0,
            "mret 取 PC 时必须清掉最低一位（IALIGN = 16）"
        );
        assert_eq!(
            MEPC_ALIGN_MASK & 0x0000_0002,
            0x0000_0002,
            "bit 1 必须保留 —— 压缩指令是 2 字节对齐的（IALIGN = 16，不是 32）"
        );
        assert_eq!(
            MEPC_ALIGN_MASK & 0xFFFF_FFFE,
            MEPC_ALIGN_MASK,
            "除最低一位外，其余位一个都不能少"
        );
    }

    // -----------------------------------------------------------------------
    // 裁决 C / E —— Zicntr 计数器（batch v0.36 · spec STAGE-20）
    // -----------------------------------------------------------------------

    /// ⚠️ **十二个计数器 CSR 的分类必须**逐个**钉死。**
    ///
    /// 这条用例是本批那个 bug 的**反例钉**：旧规则（bit 11 置位 ⇒ 只读）
    /// 会把 `mcycle` / `minstret` / `mcycleh` / `minstreth` 判成只读 ⇒ ⇒
    /// 计数器**根本写不进去** ⇒ ⇒ 这里会红。
    #[test]
    fn the_counter_csrs_are_classified_by_the_spec() {
        // 机器态计数器与 `mcountinhibit`：**可写**。
        for csr in [
            CSR_MCYCLE,
            CSR_MINSTRET,
            CSR_MCYCLEH,
            CSR_MINSTRETH,
            CSR_MCOUNTINHIBIT,
        ] {
            assert!(!is_csr_read_only(csr), "csr {csr:#x} 必须可写");
            assert!(csr_is_implemented(csr), "csr {csr:#x} 必须已实现");
            assert!(!csr_write_is_ignored(csr), "csr {csr:#x} 的写不得被忽略");
        }
        // 用户态影子：**只读**（地址规则：`0xC00..0xFFF`）。
        for csr in [
            CSR_CYCLE,
            CSR_TIME,
            CSR_INSTRET,
            CSR_CYCLEH,
            CSR_TIMEH,
            CSR_INSTRETH,
        ] {
            assert!(is_csr_read_only(csr), "csr {csr:#x} 必须只读");
            assert!(csr_write_is_ignored(csr), "csr {csr:#x} 的写必须被忽略");
        }
    }

    /// ⚠️ **两半的配对必须钉死** —— 半部配错 ⇒ 低半部回绕时不进位 ⇒
    /// 高半部停在 `0xFFFFFFFF` ⇒ 上游 `rv32mi/instret_overflow` 的
    /// `TEST_CASE(4)`（期望 `minstreth == 0`）就会红。
    #[test]
    fn the_counter_halves_are_paired() {
        assert_eq!(
            counter_pair(CSR_MCYCLE),
            Some((CSR_MCYCLE, CSR_MCYCLEH)),
            "mcycle 的两半"
        );
        assert_eq!(
            counter_pair(CSR_MCYCLEH),
            Some((CSR_MCYCLE, CSR_MCYCLEH)),
            "mcycleh 的两半（低半部在前）"
        );
        assert_eq!(
            counter_pair(CSR_MINSTRET),
            Some((CSR_MINSTRET, CSR_MINSTRETH)),
            "minstret 的两半"
        );
        assert_eq!(
            counter_pair(CSR_MINSTRETH),
            Some((CSR_MINSTRET, CSR_MINSTRETH)),
            "minstreth 的两半（低半部在前）"
        );
        // 影子**不是**计数器 ⇒ 没有半部对。
        for csr in [CSR_CYCLE, CSR_INSTRET, CSR_CYCLEH, CSR_INSTRETH] {
            assert_eq!(counter_pair(csr), None, "影子 {csr:#x} 没有半部对");
        }
        // 别的 CSR 也没有。
        assert_eq!(counter_pair(CSR_MSTATUS), None);
        assert_eq!(counter_pair(CSR_MCOUNTINHIBIT), None);
    }

    /// ⚠️ **影子的指向必须钉死** —— 指错了 ⇒ guest 读 `instret` 读到的是
    /// 周期数 ⇒ 行为完全错，而**编译期一点提示都没有**。
    #[test]
    fn the_shadows_point_at_their_machine_counter() {
        assert_eq!(counter_shadow(CSR_CYCLE), Some(CSR_MCYCLE));
        assert_eq!(counter_shadow(CSR_CYCLEH), Some(CSR_MCYCLEH));
        assert_eq!(counter_shadow(CSR_INSTRET), Some(CSR_MINSTRET));
        assert_eq!(counter_shadow(CSR_INSTRETH), Some(CSR_MINSTRETH));
        // ⚠️ `time` / `timeh` **故意不在表里** —— 本平台读它们为 0
        // （要接 M-15 的 CLINT 才能有真值）。
        assert_eq!(counter_shadow(CSR_TIME), None);
        assert_eq!(counter_shadow(CSR_TIMEH), None);
        // 机器态计数器**自己**不是影子。
        assert_eq!(counter_shadow(CSR_MCYCLE), None);
        assert_eq!(counter_shadow(CSR_MINSTRET), None);
        assert_eq!(counter_shadow(CSR_MSTATUS), None);
    }

    /// ⚠️ **半部与影子的地址必须钉死** —— 改地址 ⇒ guest 读到的 CSR
    /// 完全不是那个东西，而**没有任何一条用例会立刻红**。
    #[test]
    fn the_counter_csr_numbers_are_pinned() {
        assert_eq!(CSR_MCOUNTINHIBIT, 0x320);
        assert_eq!(CSR_MCYCLE, 0xB00);
        assert_eq!(CSR_MINSTRET, 0xB02);
        assert_eq!(CSR_MCYCLEH, 0xB80);
        assert_eq!(CSR_MINSTRETH, 0xB82);
        assert_eq!(CSR_CYCLE, 0xC00);
        assert_eq!(CSR_TIME, 0xC01);
        assert_eq!(CSR_INSTRET, 0xC02);
        assert_eq!(CSR_CYCLEH, 0xC80);
        assert_eq!(CSR_TIMEH, 0xC81);
        assert_eq!(CSR_INSTRETH, 0xC82);
    }

    // -----------------------------------------------------------------------
    // PMP (STAGE-21 · v0.37) —— 全部断言都对着**参考侧实测**的事实
    // -----------------------------------------------------------------------

    /// ⚠️ **位序必须钉死**（F1）—— 错一位 ⇒ guest 的权限判定整个反过来，
    /// 而**没有任何一条内部用例会立刻红**（只有判据语料会）。
    #[test]
    fn the_pmp_cfg_byte_layout_is_pinned() {
        let cfg = pmp_decode_cfg(0x9f);
        assert!(cfg.l);
        assert_eq!(cfg.a, PMP_A_NAPOT);
        assert!(cfg.r && cfg.w && cfg.x);
        // 逐位：R = bit0、W = bit1、X = bit2；A = bit4:3。
        assert!(pmp_decode_cfg(0x01).r);
        assert!(!pmp_decode_cfg(0x01).w);
        assert!(!pmp_decode_cfg(0x01).x);
        assert!(!pmp_decode_cfg(0x02).r);
        assert!(pmp_decode_cfg(0x02).w);
        assert!(!pmp_decode_cfg(0x04).r);
        assert!(!pmp_decode_cfg(0x04).w);
        assert!(pmp_decode_cfg(0x04).x);
        assert_eq!(pmp_decode_cfg(0x00).a, PMP_A_OFF);
        assert_eq!(pmp_decode_cfg(0x08).a, PMP_A_TOR);
        assert_eq!(pmp_decode_cfg(0x10).a, PMP_A_NA4);
        assert_eq!(pmp_decode_cfg(0x18).a, PMP_A_NAPOT);
        assert!(!pmp_decode_cfg(0x00).l);
        assert!(pmp_decode_cfg(0x80).l);
    }

    /// ⚠️ **`MPRV` 的位号必须钉死**（F10 / F11）—— 错一位 ⇒ M 模式的访存
    /// 口径整个反过来，而**没有任何一条内部用例会立刻红**（只有判据语料会）。
    ///
    /// ⚠️ 这个位号是**实测**出来的：参考侧在置上它之后才从「M 模式旁路」
    /// 变成「受检」。
    #[test]
    fn the_mprv_bit_is_pinned() {
        assert_eq!(MSTATUS_MPRV_BIT, 17);
    }

    /// ⚠️ **CSR 号必须钉死**（F1 / F2 / F15）—— 错一位 ⇒ 条目整个错位。
    #[test]
    fn the_pmp_csr_numbers_are_pinned() {
        assert_eq!(CSR_PMPCFG0, 0x3A0);
        assert_eq!(CSR_PMPCFG3, 0x3A3);
        assert_eq!(CSR_PMPADDR0, 0x3B0);
        assert_eq!(CSR_PMPADDR15, 0x3BF);
        assert_eq!(PMP_ENTRY_COUNT, 16);
        assert_eq!(PMP_GRAINULARITY_SHIFT, 2);
    }

    /// ⚠️ **NAPOT 的区间公式必须逐字对上参考侧**（F4）—— 三个**实测点**
    /// 分别对应 4 KiB / 256 KiB / 1 MiB，**外加**全 1 的「整个空间」。
    #[test]
    fn napot_regions_match_the_reference_side() {
        let napot = pmp_decode_cfg(0x1f);
        // 4 KiB @ 0x80000000（pmpaddr = 0x80000000 >> 2，加 9 个末尾 1）。
        let region = pmp_region(napot, 0, 0x2000_01ff).expect("NAPOT entry");
        assert_eq!(region.base, 0x8000_0000);
        assert_eq!(region.size, 4096);
        assert!(region.covers(0x8000_0000, 4));
        assert!(region.covers(0x8000_0ffc, 4));
        assert!(!region.covers(0x8000_1000, 4));
        // 256 KiB @ 0x80000000（17 个末尾 1 ⇒ 2^18 字节）。
        let region = pmp_region(napot, 0, 0x2000_7fff).expect("NAPOT entry");
        assert_eq!(region.base, 0x8000_0000);
        assert_eq!(region.size, 256 * 1024);
        assert!(region.covers(0x8003_fffc, 4));
        assert!(!region.covers(0x8004_0000, 4));
        // 1 MiB @ 0x80000000（19 个末尾 1 ⇒ 2^20 字节）。
        let region = pmp_region(napot, 0, 0x2001_ffff).expect("NAPOT entry");
        assert_eq!(region.base, 0x8000_0000);
        assert_eq!(region.size, 1024 * 1024);
        assert!(region.covers(0x800f_fffc, 4));
        assert!(!region.covers(0x8010_0000, 4));
        // 全 1 ⇒ n = 32 ⇒ base = 0、size = 2^35 ⇒ 覆盖**整个** 32 位空间。
        let region = pmp_region(napot, 0, u32::MAX).expect("NAPOT entry");
        assert_eq!(region.base, 0);
        assert_eq!(region.size, 1u64 << 35);
        assert!(region.covers(0x0000_0000, 4));
        assert!(region.covers(0x7fff_f000, 4));
        assert!(region.covers(0x803f_fffc, 4));
    }

    /// ⚠️ **NA4 是四个字节**（F5）—— 差一位 ⇒ 一条判据就绿了。
    #[test]
    fn na4_regions_match_the_reference_side() {
        let na4 = pmp_decode_cfg(0x17);
        let region = pmp_region(na4, 0, 0x2000_0800).expect("NA4 entry");
        assert_eq!(region.base, 0x8000_2000);
        assert_eq!(region.size, 4);
        assert!(region.covers(0x8000_2000, 4));
        assert!(!region.covers(0x8000_2004, 4));
    }

    /// ⚠️ **TOR 的上界是半开的**（F6）—— 闭区间会多放行一个地址。
    #[test]
    fn tor_regions_match_the_reference_side() {
        let tor = pmp_decode_cfg(0x0f);
        // 条目 0 的下界是 0 ⇒ [0, 0x8000_7ffc)。
        let region = pmp_region(tor, 0, 0x2000_1fff).expect("TOR entry");
        assert_eq!(region.base, 0);
        assert_eq!(region.size, 0x8000_7ffc);
        assert!(region.covers(0x8000_0000, 4));
        assert!(region.covers(0x8000_7ff8, 4));
        assert!(!region.covers(0x8000_7ffc, 4));
        // 下界不小于上界 ⇒ **不匹配任何地址**。
        assert!(pmp_region(tor, 0x2000_1fff, 0x2000_1fff).is_none());
        assert!(pmp_region(tor, 0x2000_2000, 0x2000_1fff).is_none());
    }

    /// ⚠️ **优先级 = 最低编号的「命中」条目**（F7）—— 这条判别力直接对着
    /// 「任一放行即可」那个错判 ⇒ ⇒ 两条用例分别测两个方向。
    #[test]
    fn the_lowest_numbered_matching_entry_decides() {
        // 条目 0 命中但**没有 W**；条目 1 也命中且**有 W** ⇒ ⇒ 条目 0 说了算 ⇒ 拒。
        let entries = [
            (pmp_decode_cfg(0x1d), 0x2000_01ff),
            (pmp_decode_cfg(0x16), 0x2000_0000),
        ];
        assert!(!pmp_access_allowed(
            &entries,
            0x8000_0000,
            4,
            PmpAccess::Store,
            false
        ));
        // 条目 0 命中且**有 W**；条目 1 命中但**没有 W** ⇒ ⇒ 放行。
        let entries = [
            (pmp_decode_cfg(0x1f), 0x2000_01ff),
            (pmp_decode_cfg(0x15), 0x2000_0000),
        ];
        assert!(pmp_access_allowed(
            &entries,
            0x8000_0000,
            4,
            PmpAccess::Store,
            false
        ));
        // 条目 0 **不命中**、条目 1 命中 ⇒ ⇒ 条目 1 说了算 ⇒ 放行。
        let entries = [
            (pmp_decode_cfg(0x18), 0x2001_01ff),
            (pmp_decode_cfg(0x1f), 0x2001_ffff),
        ];
        assert!(pmp_access_allowed(
            &entries,
            0x8000_2000,
            4,
            PmpAccess::Load,
            false
        ));
    }

    /// ⚠️ **没有条目命中 ⇒ 拒**（F8）—— 只要有 ≥1 个 A≠OFF 的条目。
    #[test]
    fn an_access_without_a_matching_entry_is_denied() {
        let entries = [(pmp_decode_cfg(0x1f), 0x2000_01ff)];
        assert!(!pmp_access_allowed(
            &entries,
            0x8000_3000,
            4,
            PmpAccess::Load,
            false
        ));
        // 全部条目都是 OFF ⇒ PMP 未实现 ⇒ 放行。
        let entries = [(pmp_decode_cfg(0x00), 0x2000_01ff)];
        assert!(pmp_access_allowed(
            &entries,
            0x8000_3000,
            4,
            PmpAccess::Load,
            false
        ));
    }

    /// ⚠️ **`R`=0 且 `W`=1 是保留组合 ⇒ 写被拒**（F9）。
    #[test]
    fn write_permission_needs_read_permission() {
        assert!(!pmp_permits(pmp_decode_cfg(0x1a), PmpAccess::Store));
        assert!(!pmp_permits(pmp_decode_cfg(0x1e), PmpAccess::Store));
        assert!(pmp_permits(pmp_decode_cfg(0x1b), PmpAccess::Store));
        assert!(pmp_permits(pmp_decode_cfg(0x1f), PmpAccess::Store));
        // 读与取指各自只看自己那一位。
        assert!(pmp_permits(pmp_decode_cfg(0x19), PmpAccess::Load));
        assert!(!pmp_permits(pmp_decode_cfg(0x1c), PmpAccess::Load));
        assert!(pmp_permits(pmp_decode_cfg(0x1c), PmpAccess::Fetch));
        assert!(!pmp_permits(pmp_decode_cfg(0x19), PmpAccess::Fetch));
    }

    /// ⚠️ **`pmpcfg` 的写入会被合法化**（F20）—— 保留位 6:5 读回 0，
    /// 保留组合 `R`=0 ∧ `W`=1 丢掉 `W` 位，其余**逐字**读回。
    ///
    /// ⚠️ **判别力**：这三条断言**互不相同** ⇒ ⇒ 「一律清零」「一律照抄」
    /// 这两种写法都过不了 ⇒ ⇒ 合法化必须是**逐位**的那一种。
    #[test]
    fn the_pmp_cfg_writes_are_legalised() {
        // 保留位 6:5 ⇒ 读回 0。
        assert_eq!(pmp_cfg_warl(0xff), 0x9f);
        assert_eq!(pmp_cfg_warl(0x60), 0x00);
        // 保留组合 `R`=0 ∧ `W`=1 ⇒ 丢掉 `W` 位。
        assert_eq!(pmp_cfg_warl(0x1a), 0x18);
        assert_eq!(pmp_cfg_warl(0x1e), 0x1c);
        assert_eq!(pmp_cfg_warl(0x2a), 0x08);
        // 合法值**逐字**读回 ⇒ ⇒ 合法化不是「一律清零」，也不是「一律照抄」。
        assert_eq!(pmp_cfg_warl(0x00), 0x00);
        assert_eq!(pmp_cfg_warl(0x01), 0x01);
        assert_eq!(pmp_cfg_warl(0x04), 0x04);
        assert_eq!(pmp_cfg_warl(0x1b), 0x1b);
        assert_eq!(pmp_cfg_warl(0x1f), 0x1f);
        assert_eq!(pmp_cfg_warl(0x9f), 0x9f);
        // ⚠️ **幂等**：合法化一个已经合法的值**不动它** ⇒ ⇒ 读回再写、
        // 再读回是**同一个值**（快照往返稳定）。
        for byte in [0x00u8, 0x01, 0x04, 0x1b, 0x1f, 0x9f] {
            assert_eq!(pmp_cfg_warl(pmp_cfg_warl(byte)), byte, "合法化必须幂等");
        }
    }

    /// ⚠️ **M 模式旁路，但锁住的条目照样管**（F10 / F12）⇒ ⇒ 这两条是
    /// 「MPRV 口径」与「L 位的意义」各自的判别力。
    #[test]
    fn m_mode_is_exempt_unless_the_entry_is_locked() {
        // 条目 0 命中、没有 W、**没锁** ⇒ M 模式（MPRV=0）⇒ 放行。
        let entries = [(pmp_decode_cfg(0x1d), 0x2000_01ff)];
        assert!(pmp_access_allowed(
            &entries,
            0x8000_0000,
            4,
            PmpAccess::Store,
            true
        ));
        // 同样的条目，**MPRV=1** ⇒ 受检 ⇒ 拒。
        assert!(!pmp_access_allowed(
            &entries,
            0x8000_0000,
            4,
            PmpAccess::Store,
            false
        ));
        // **锁住** ⇒ M 模式也强制执行 ⇒ 拒。
        let entries = [(pmp_decode_cfg(0x9d), 0x2000_01ff)];
        assert!(!pmp_access_allowed(
            &entries,
            0x8000_0000,
            4,
            PmpAccess::Store,
            true
        ));
        // 没有条目命中时，M 模式 ⇒ 放行。
        let entries = [(pmp_decode_cfg(0x1f), 0x2000_01ff)];
        assert!(pmp_access_allowed(
            &entries,
            0x8000_3000,
            4,
            PmpAccess::Store,
            true
        ));
    }

    /// ⚠️ **PMP 的 CSR 必须被认作已实现**（裁决 E）—— 不认 ⇒ 写被吞 ⇒ ⇒
    /// PMP 变成一排**不起作用的寄存器**，而**没有任何一条用例会立刻红**。
    #[test]
    fn the_pmp_csrs_are_implemented() {
        for csr in CSR_PMPCFG0..=CSR_PMPCFG3 {
            assert!(
                csr_is_implemented(csr),
                "pmpcfg {csr:#x} must be implemented"
            );
            assert!(
                !csr_write_is_ignored(csr),
                "pmpcfg {csr:#x} must accept writes"
            );
            assert!(!is_csr_read_only(csr), "pmpcfg {csr:#x} must be writable");
        }
        for csr in CSR_PMPADDR0..=CSR_PMPADDR15 {
            assert!(
                csr_is_implemented(csr),
                "pmpaddr {csr:#x} must be implemented"
            );
            assert!(
                !csr_write_is_ignored(csr),
                "pmpaddr {csr:#x} must accept writes"
            );
        }
        // 未实现的那些 ⇒ 写被忽略、读回 0（WARL）。
        assert!(!csr_is_implemented(0x3A4));
        assert!(csr_write_is_ignored(0x3A4));
        assert!(!csr_is_implemented(0x3C0));
    }

    // ------------------------------------------------------------------
    // STAGE-22 · v0.38 · S 模式（验收 A-1 ~ A-11）
    // ------------------------------------------------------------------

    /// **A-1** · `mstatus` 的真实可写掩码 = **`0x807E_79AA`**（F6）。
    ///
    /// ⚠️ 判别式用的是**架构字面量**，不是 `MSTATUS_WARL_MASK` 自己 ⇒
    /// 把掩码改坏（或整个去掉）这条会红。
    #[test]
    fn mstatus_warl_keeps_exactly_the_measured_writable_bits() {
        assert_eq!(
            mstatus_warl(0xFFFF_FFFF),
            0x807E_79AA,
            "写全 1 之后 mstatus 必须读回参考侧实测的那个值（F6）"
        );
    }

    /// **F40** · `mstatus.SD`（bit 31）是**推导位**：= (`FS` == 3) ∨ (`XS` == 3)。
    ///
    /// ⚠️ **三条实测**：① 只写 `FS = 3`（`0x6000`）⇒ ⇒ 读回 **`0x8000_6000`**
    /// （`SD` 被**推成 1**）；② 只写 `FS = 1`（`0x2000`）⇒ ⇒ 读回
    /// **`0x0000_2000`**（`SD` 是 0）；③ 直接写 bit 31（`0x8000_0000`）⇒ ⇒
    /// 读回 **`0x0000_0000`** ⇒ ⇒ **直接写无效** ⇒ ⇒ 它**不是**存储位。
    ///
    /// ⚠️ 判别式用的是**架构字面量**，不是常量自己 ⇒ ⇒ 改了常量而忘了改
    /// 语义，这条会红。
    #[test]
    fn mstatus_warl_derives_the_sd_bit_from_fs_and_xs() {
        assert_eq!(
            mstatus_warl(0x0000_6000),
            0x8000_6000,
            "只把 FS 写成 3 ⇒ SD 必须被推成 1（F40）"
        );
        assert_eq!(
            mstatus_warl(0x0000_2000),
            0x0000_2000,
            "FS = 1（Initial）⇒ SD 必须是 0（F40）"
        );
        assert_eq!(
            mstatus_warl(0x8000_0000),
            0x0000_0000,
            "直接写 SD 无效 ⇒ 它是推导位（F40）"
        );
    }

    /// **A-2** · `MPP` 的**保留值 ⇒ 合法化到 0**，**同一次写里的其它位照常落
    /// 进去**（F5 + **F39**）。
    ///
    /// ⚠️ 关键在**「只丢 `MPP` 字段」**：清零后写 `0x1002`（`MPP = 2` 且
    /// `SIE = 1`）⇒ 参考侧读回低字节 **`0x02`** ⇒ ⇒ `SIE` **落进去了**
    /// ⇒ ⇒ 「整次丢弃」那个读法会让这条**红**。
    ///
    /// ⚠️ 关键在**「不是保留旧值」**：先写 `0x1800`（`MPP = 3`）再写
    /// `0x1002` ⇒ 参考侧读回**仍是 `0x0000_0002`** ⇒ ⇒ 旧值被**覆盖成 0**
    /// ⇒ ⇒ 「保留旧值」那个读法会让这条**红**。
    ///
    /// ⚠️ `MPP` 字段是 **bits 12:11** ⇒ `MPP = 2` 写的是 **`0x1000`**
    /// （`0x1800` 是 `MPP = 3`，**合法** ⇒ ⇒ 用它测「保留值」是测不到的）。
    #[test]
    fn mstatus_warl_legalizes_the_reserved_mpp_to_zero() {
        assert_eq!(
            mstatus_warl(0x0000_1002),
            0x0000_0002,
            "MPP = 2 是保留值 ⇒ 该字段合法化到 0，但 SIE 必须落进去（F39）"
        );
        assert_eq!(
            mstatus_warl(0x0000_1000),
            0x0000_0000,
            "只写 MPP = 2 ⇒ 整次写没有任何可落的位 ⇒ 结果是 0（F39）"
        );
        assert_eq!(
            mstatus_warl(0x0000_1802),
            0x0000_1802,
            "MPP = 3 是合法值 ⇒ 原样落进去（F5）"
        );
    }

    /// **A-2 的另一半** · 合法的 `MPP` 值 {0, 1, 3} 都**不**触发合法化。
    #[test]
    fn mstatus_warl_accepts_the_legal_mpp_values() {
        for mpp in [MSTATUS_MPP_U, MSTATUS_MPP_S, MSTATUS_MPP_M] {
            let new = mpp << MSTATUS_MPP_SHIFT;
            assert_eq!(
                mstatus_warl(new),
                new,
                "MPP = {mpp} 是合法值 ⇒ 写必须落进去（F5）"
            );
        }
    }

    /// **A-4** · 14 个新 CSR **必须**被认作已实现 ⇒ ⇒ 它们的写**不被吞掉**。
    #[test]
    fn the_s_and_u_mode_csr_are_implemented() {
        for csr in [
            CSR_SSTATUS,
            CSR_SIE,
            CSR_STVEC,
            CSR_SSCRATCH,
            CSR_SEPC,
            CSR_SCAUSE,
            CSR_STVAL,
            CSR_SIP,
            CSR_SATP,
            CSR_MEDELEG,
            CSR_MIDELEG,
            CSR_MCOUNTEREN,
            CSR_SCOUNTEREN,
            CSR_UCOUNTEREN,
        ] {
            assert!(
                csr_is_implemented(csr),
                "csr {csr:#x} 必须被认作已实现，否则写会被吞掉（裁决 F）"
            );
            assert!(
                !csr_write_is_ignored(csr),
                "csr {csr:#x} 必须接受写（裁决 F）"
            );
        }
    }

    /// **A-3 / A-10** · 伪 CSR `CSR_PRIV`：**已实现**（快照要留住它）**且**
    /// **只读**（客体改不了自己的特权级）。
    #[test]
    fn the_privilege_pseudo_csr_is_implemented_but_read_only() {
        assert!(
            csr_is_implemented(CSR_PRIV),
            "CSR_PRIV 必须被认作已实现 ⇒ 否则快照的 retain 会丢掉它 ⇒ 特权级归零"
        );
        assert!(
            csr_is_read_only(CSR_PRIV),
            "CSR_PRIV 必须只读 ⇒ 否则客体的写会真的落进文件 ⇒ guest 能改自己的特权级"
        );
        assert!(
            csr_write_is_ignored(CSR_PRIV),
            "CSR_PRIV 的客体写必须被忽略"
        );
    }

    /// **A-5 / A-9** · `misa` 与参考侧**逐位相同**（F3 / F24 / F27）。
    #[test]
    fn misa_is_bit_identical_to_the_reference_side() {
        assert_eq!(
            CSR_MISA_VALUE, 0x4014_1104,
            "misa 必须与参考侧逐位相同：MXL=1、I、M、C、S 位、U 位、X 位"
        );
        assert_ne!(CSR_MISA_VALUE & (1 << 18), 0, "S 位（18）必须置着");
        assert_ne!(CSR_MISA_VALUE & (1 << 20), 0, "U 位（20）必须置着");
    }

    /// **A-6** · `sstatus` 的客体写掩码 = **`0x804C_61A2`**（F38）。
    ///
    /// ⚠️ 判别式用的是**架构字面量**，不是常量自己 ⇒ ⇒ 把掩码改坏这条会红。
    #[test]
    fn the_sstatus_write_mask_is_the_measured_one() {
        assert_eq!(SSTATUS_WARL_MASK, 0x804C_61A2);
        assert_ne!(
            SSTATUS_WARL_MASK & (1 << MSTATUS_SPP_BIT),
            0,
            "SPP 在写掩码里 ⇒ ⇒ 参考侧允许 S 模式写 sstatus.SPP（F38）"
        );
        assert_ne!(
            SSTATUS_WARL_MASK & (1 << MSTATUS_SIE_BIT),
            0,
            "SIE 在写掩码里（F38）"
        );
        assert_eq!(
            SSTATUS_WARL_MASK & (1 << MSTATUS_MPP_SHIFT),
            0,
            "MPP 不在 sstatus 的写掩码里（F38）"
        );
    }

    /// **A-11** · `sstatus` 的读视图 = **`0x800D_E162`** ⇒ ⇒ **含 `SD`**（F38）。
    #[test]
    fn the_sstatus_view_mask_is_the_measured_one() {
        assert_eq!(SSTATUS_VIEW_MASK, 0x800D_E162);
        assert_ne!(SSTATUS_VIEW_MASK & (1 << 31), 0, "SD 在视图里（F38）");
        assert_ne!(
            SSTATUS_VIEW_MASK & (1 << MSTATUS_SPP_BIT),
            0,
            "SPP 在视图里（F7）"
        );
        assert_eq!(
            SSTATUS_VIEW_MASK & (1 << 3),
            0,
            "MIE 不在视图里 ⇒ ⇒ S 模式看不到机器态的使能位（F38）"
        );
    }

    /// **F35** · `medeleg` 的合法位集合 = **`0x0008_B3FE`**（per-bit WARL）。
    #[test]
    fn the_medeleg_writable_mask_is_the_measured_one() {
        assert_eq!(MEDELEG_WRITABLE_MASK, 0x0008_B3FE);
        assert_eq!(
            MEDELEG_WRITABLE_MASK & 0x0000_B109,
            0x0000_B108,
            "参考侧把 medeleg 的 bit 0 丢掉（F35 + F36）"
        );
        assert_eq!(
            MEDELEG_WRITABLE_MASK & 0x0000_FFFF,
            0x0000_B3FE,
            "只写低 16 位 ⇒ 高位那些位拿不到（F35）"
        );
    }
}
