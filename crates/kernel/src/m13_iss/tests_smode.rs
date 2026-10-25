//! M-13 的 **S 模式 / U 模式**内核语义（STAGE-22 · v0.38）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-22_V038_Smode.md`。
//!
//! ## 判别力纪律
//!
//! 每条断言都用**架构字面量**（`0x000C_6122` / `0x800D_E162` / `0x0008_B3FE`
//! / `cause 2` / `MPP = 1` …），
//! **不**从被测的常量推出来 ⇒ ⇒ 把常量改坏时这条测试**必须变红**。
//!
//! ⚠️ **`sstatus` 的断言走「真的 `csrr` 指令」**：它是 `mstatus` 的**位视图**
//! ⇒ ⇒ CSR **文件**里**没有** `sstatus` 那一条 ⇒ ⇒ 直接读文件永远读到 0
//! ⇒ ⇒ 唯一能观察到它的路径是**客体执行一条 `csrr sstatus, x1`**。
//!
//! ⚠️ **特权级的观察也走客体的路**：特权级藏在 CSR 文件的**伪 CSR**
//! `CSR_PRIV` 里 ⇒ ⇒ 客体的读**必须**是 0 ⇒ ⇒ 平台的读走 `priv_get`。

use riscv_sim_contract::csr::{
    csr_is_implemented, CSR_CYCLE, CSR_MCAUSE, CSR_MCOUNTEREN, CSR_MEDELEG, CSR_MEPC, CSR_MISA,
    CSR_MSTATUS, CSR_MTVEC, CSR_PRIV, CSR_SATP, CSR_SCAUSE, CSR_SCOUNTEREN, CSR_SEPC, CSR_SIE,
    CSR_SIP, CSR_SSTATUS, CSR_STVAL, CSR_STVEC, CSR_UCOUNTEREN, PRIV_M, PRIV_S, PRIV_U,
};
use riscv_sim_contract::model::SimState;

use super::csr;
use super::decoder::Decoder;
use super::executor::{classify, Op, StepOutcome, TrapCause};
use super::tests_common::*;

// ---------------------------------------------------------------------------
// Architectural literals —— 判别力全靠它们
// ---------------------------------------------------------------------------

/// 异常 cause **2** —— 非法指令。
const CAUSE_ILLEGAL: u32 = 2;
/// 异常 cause **3** —— 断点。
const CAUSE_BREAKPOINT: u32 = 3;
/// 异常 cause **8** —— **U 模式**的环境调用。
const CAUSE_USER_ECALL: u32 = 8;
/// 异常 cause **9** —— **S 模式**的环境调用。
const CAUSE_SUPERVISOR_ECALL: u32 = 9;
/// 异常 cause **11** —— **M 模式**的环境调用。
const CAUSE_MACHINE_ECALL: u32 = 11;

/// `mstatus.MIE`（bit 3）。
const MSTATUS_MIE: u32 = 0x0000_0008;
/// `mstatus.MPIE`（bit 7）。
const MSTATUS_MPIE: u32 = 0x0000_0080;
/// `mstatus.MPP` 字段（bits 12:11）。
const MSTATUS_MPP_FIELD: u32 = 0x0000_1800;
/// `mstatus.MPP` = **U**（0）。
const MPP_U: u32 = 0;
/// `mstatus.MPP` = **S**（1）。
const MPP_S: u32 = 1;
/// `mstatus.MPP` = **M**（3）。
const MPP_M: u32 = 3;

/// `sstatus` 的**客体写**掩码 —— **实测**（F39）。
///
/// ⚠️ **不是**旧口径的 `0x22` —— 那个数是**被 8 位退出码截断**的观测给出的
/// **低字节**（真正的写掩码的**低字节**恰好也是 `0x22`，所以截断观测「看
/// 起来」自洽 ⇒ ⇒ 这就是它骗人的地方）。
const SSTATUS_WRITE_MASK: u32 = 0x000C_6122;

/// `sstatus` 的**读视图**掩码 —— **实测**（F39）。
///
/// ⚠️ **不是**旧口径的 `0x122` —— 那是**低 16 位**。
const SSTATUS_VIEW_MASK: u32 = 0x800D_E162;

/// `medeleg` 的**合法位集合** —— **实测**（F40，per-bit WARL）。
const MEDELEG_LEGAL_BITS: u32 = 0x0008_B3FE;
/// `mstatus.SPP`（bit 8）—— `sstatus.SPP` 与它是**同一个字段**（**F7**）。
const MSTATUS_SPP: u32 = 0x0000_0100;
/// `mstatus.SIE`（bit 1）。
const MSTATUS_SIE: u32 = 0x0000_0002;
/// `mstatus.SPIE`（bit 5）。
const MSTATUS_SPIE: u32 = 0x0000_0020;

/// 陷阱向量地址（测试用）。
const MTVEC: u32 = 0x2000;
/// S 级陷阱向量地址（测试用）。
const STVEC: u32 = 0x3000;
/// 测试程序的地址。
const PC: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 一个**处在 `level` 特权级**的初态。
fn state_in(level: u32) -> SimState {
    let mut state = SimState::default();
    csr::priv_set(&mut state, level);
    state
}

/// 一个处在 `level` 特权级、且 CSR 文件里预置了 `entries` 的初态。
fn state_with(level: u32, entries: &[(u16, u32)]) -> SimState {
    let mut state = SimState {
        csrs: entries.to_vec(),
        ..SimState::default()
    };
    csr::priv_set(&mut state, level);
    state
}

/// 执行一条指令，返回它的结局。
fn step(h: &mut Harness, state: &mut SimState, pc: u32, word: u32) -> StepOutcome {
    h.step_one_at(state, pc, word)
}

/// `csrw <csr>, <rs1>` —— 先把 `value` 放进 `rs1`，再执行那条写指令。
fn csrw_with(h: &mut Harness, state: &mut SimState, csr: u16, rs1: u8, value: u32) -> StepOutcome {
    state.gpr[rs1 as usize] = value;
    let pc = state.pc;
    h.step_one_at(state, pc, csrw(csr, rs1))
}

/// **客体视角**读一个 CSR —— 一条真的 `csrr <csr>, x1` 指令。
///
/// ⚠️ 读**可能**陷阱（按级不可访问）⇒ ⇒ 结局与读到的值**一起**返回。
fn guest_read(h: &mut Harness, state: &mut SimState, csr: u16) -> (StepOutcome, u32) {
    let pc = state.pc;
    let outcome = h.step_one_at(state, pc, csrr(csr, 1));
    (outcome, state.gpr[1])
}

/// 装好陷阱向量与委托掩码。
fn arm(state: &mut SimState, medeleg: u32) {
    csr::csr_write(state, CSR_MTVEC, MTVEC);
    csr::csr_write(state, CSR_STVEC, STVEC);
    csr::csr_write(state, CSR_MEDELEG, medeleg);
}

/// `sret` 的编码（`0x1020_0073`）。
fn sret_insn() -> u32 {
    enc_i(0x102, 0, 0b000, 0, SYSTEM)
}

/// `mret` 的编码（`0x3020_0073`）。
fn mret_insn() -> u32 {
    enc_i(0x302, 0, 0b000, 0, SYSTEM)
}

/// `ebreak` 的编码（`0x0010_0073`）。
fn ebreak_insn() -> u32 {
    enc_i(1, 0, 0b000, 0, SYSTEM)
}

// ---------------------------------------------------------------------------
// The decode level
// ---------------------------------------------------------------------------

/// `SRET` 必须解成 [`Op::Sret`] —— **不是**非法指令。
///
/// ⚠️ **编码逐位钉死**：`0x1020_0073` 是架构标准编码（`funct3 = 0`、
/// `imm = 0x102`）⇒ 写错一位这里就红。
#[test]
fn the_sret_encoding_resolves_to_sret() {
    let d = Decoder::default().decode(sret_insn(), PC);
    assert_eq!(classify(&d), Some(Op::Sret));
    assert_eq!(d.imm, 0x102, "the SRET immediate field is 0x102");
    assert_eq!(sret_insn(), 0x1020_0073, "SRET 的编码必须逐位是 0x10200073");
}

// ---------------------------------------------------------------------------
// The privilege level model
// ---------------------------------------------------------------------------

/// 复位即**机器态** —— CSR 文件里**没有** `CSR_PRIV` 那一条。
///
/// ⚠️ **这条同时钉住「文件保持空」**：M-12 的快照「空」判据
/// （`EngineSnapshotDto::is_empty`）读的就是「CSR 文件里有没有东西」。
#[test]
fn a_fresh_state_is_in_machine_mode_without_touching_the_csr_file() {
    let state = SimState::default();
    assert_eq!(csr::priv_get(&state), PRIV_M, "复位即机器态");
    assert!(
        !state.csrs.iter().any(|(number, _)| *number == CSR_PRIV),
        "复位后 CSR 文件里**不该**有 CSR_PRIV 那一条 ⇒ 快照的「空」判据不被破坏"
    );
}

/// 特权级**住在 CSR 文件里** ⇒ ⇒ 快照往返之后**不丢**（验收 **A-3**）。
///
/// ⚠️ 两条判据：
/// ① **序列化往返**（`SimState` 是可序列化的 ⇒ 快照存的就是它）；
/// ② **`m12_engine` 的 retain 过滤**（`drop_unimplemented_csrs`）—— 它按
///    契约层的「已实现」表过滤 ⇒ ⇒ `CSR_PRIV` **必须**在那张表里，否则
///    过滤之后特权级归零。
#[test]
fn the_privilege_level_survives_a_snapshot_round_trip() {
    let state = state_in(PRIV_S);
    assert_eq!(csr::priv_get(&state), PRIV_S);

    // ① 序列化往返。
    let json = serde_json::to_string(&state).expect("SimState is serializable");
    let restored: SimState = serde_json::from_str(&json).expect("SimState is deserializable");
    assert_eq!(
        csr::priv_get(&restored),
        PRIV_S,
        "快照往返之后特权级**不能**丢"
    );

    // ② M-12 的那个 retain 过滤（同一张契约层的表）。
    let mut filtered = state.clone();
    filtered
        .csrs
        .retain(|(number, _)| csr_is_implemented(*number));
    assert_eq!(
        csr::priv_get(&filtered),
        PRIV_S,
        "M-12 的 retain 过滤**不能**把 CSR_PRIV 丢掉"
    );
}

/// 客体的**读** `CSR_PRIV` 必须读到 **0** —— 任何级别都读到 0。
///
/// ⚠️ 这与「平台自己读特权级」是**两条路径**（`csr_read` vs `priv_get`）。
#[test]
fn the_guest_reads_zero_for_the_privilege_pseudo_csr() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let (outcome, value) = guest_read(&mut h, &mut state, CSR_PRIV);
    assert_eq!(outcome, StepOutcome::Advanced, "读伪 CSR **不**是非法指令");
    assert_eq!(value, 0, "客体的读**必须**是 0");
}

/// 客体的**写** `CSR_PRIV` 被**忽略** —— 特权级**不能**被 guest 改掉。
#[test]
fn the_guest_cannot_write_the_privilege_pseudo_csr() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_PRIV, 1, PRIV_M);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr::priv_get(&state),
        PRIV_S,
        "客体的写**必须**被忽略 ⇒ 特权级不变"
    );
}

// ---------------------------------------------------------------------------
// The per-level accessibility table
// ---------------------------------------------------------------------------

/// **S 模式读 M 模式 CSR ⇒ cause 2**（**F10**）—— 五条各来一遍。
///
/// ⚠️ 判别式是**架构字面量** `2`，不是 `TrapCause::…::mcause()`。
#[test]
fn reading_a_machine_csr_from_supervisor_mode_is_an_illegal_instruction() {
    for &number in &[CSR_MSTATUS, CSR_MEPC, CSR_MEDELEG, CSR_MCOUNTEREN, CSR_MISA] {
        let mut h = Harness::new();
        let mut state = state_in(PRIV_S);
        let (outcome, _) = guest_read(&mut h, &mut state, number);
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction
            },
            "S 模式读机器态 CSR {number:#06x} ⇒ 非法指令"
        );
        assert_eq!(
            csr_value(&state, CSR_MCAUSE),
            CAUSE_ILLEGAL,
            "S 模式读机器态 CSR {number:#06x} ⇒ mcause 必须是 2"
        );
    }
}

/// **S 模式写 M 模式 CSR ⇒ cause 2**（**F26**）—— 不是「被忽略」。
///
/// ⚠️ 这条与上一条**同形** ⇒ ⇒ 读写**共用**同一张表。
#[test]
fn writing_a_machine_csr_from_supervisor_mode_is_an_illegal_instruction() {
    for &number in &[CSR_MSTATUS, CSR_MEPC, CSR_MEDELEG, CSR_MCOUNTEREN, CSR_MISA] {
        let mut h = Harness::new();
        let mut state = state_in(PRIV_S);
        let outcome = csrw_with(&mut h, &mut state, number, 1, 0x1234);
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction
            },
            "S 模式写机器态 CSR {number:#06x} ⇒ 非法指令（**不是**被忽略）"
        );
        assert_eq!(
            csr_value(&state, CSR_MCAUSE),
            CAUSE_ILLEGAL,
            "S 模式写机器态 CSR {number:#06x} ⇒ mcause 必须是 2"
        );
    }
}

/// **M 模式读 S 模式 CSR ⇒ 不陷阱**（**F11**）—— 特权级是**单向**的。
#[test]
fn reading_a_supervisor_csr_from_machine_mode_does_not_trap() {
    let mut h = Harness::new();
    // 先在 M 模式下把 `sscratch` 写成一个可辨认的值。
    let mut state = SimState::default();
    csr::csr_write(&mut state, 0x140, 0x1234);
    let (outcome, value) = guest_read(&mut h, &mut state, 0x140);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "M 模式读 S 模式 CSR **不**陷阱（F11）"
    );
    assert_eq!(value, 0x1234, "读回的是 S 级的状态");
}

/// **U 模式读 S 模式 CSR ⇒ cause 2**。
#[test]
fn reading_a_supervisor_csr_from_user_mode_is_an_illegal_instruction() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "U 模式读 S 模式 CSR ⇒ 非法指令"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);
}

/// **未实现**的 CSR 在 **S 模式 ⇒ cause 2**（**F26** / 验收 **B-16**）。
///
/// ⚠️ 判别式：**同一条 CSR** 在 M 模式**读回 0**（台账 **D-92** 的既有
/// 行为，**不变**）⇒ ⇒ 两条断言合起来才把「未实现」与「按级不可访问」
/// **分开**。
#[test]
fn an_unimplemented_csr_traps_in_supervisor_mode_but_reads_zero_in_machine_mode() {
    // `0x0C1` —— 契约层**没有**实现它（`csr_is_implemented` = false）。
    const UNIMPLEMENTED: u16 = 0x0C1;

    // M 模式：读回 0，写被忽略（既有行为）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let (outcome, value) = guest_read(&mut h, &mut state, UNIMPLEMENTED);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "M 模式读未实现的 CSR 不陷阱"
    );
    assert_eq!(value, 0, "M 模式读未实现的 CSR ⇒ 读回 0（D-92）");

    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, UNIMPLEMENTED, 1, 0xABCD);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, UNIMPLEMENTED),
        0,
        "M 模式写未实现的 CSR ⇒ 写被忽略（D-92）"
    );

    // S 模式：cause 2。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let (outcome, _) = guest_read(&mut h, &mut state, UNIMPLEMENTED);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "S 模式读未实现的 CSR ⇒ 非法指令（F26）"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);
}

// ---------------------------------------------------------------------------
// `sstatus` —— `mstatus` 的位视图
// ---------------------------------------------------------------------------

/// **`sstatus` 在 M / S 两级是同一存储**（**F30**，验收 **B-18**）。
///
/// ⚠️ M 模式写 `sstatus`（`0x22`）⇒ ⇒ S 模式读回 **`0x22`**。
/// ⚠️ 写的值只碰 `SIE` 与 `SPIE` ⇒ ⇒ 读回**逐字相同** ⇒ ⇒ 读路径与写
/// 路径在**同一个存储**上工作。
#[test]
fn sstatus_is_the_same_storage_in_machine_and_supervisor_mode() {
    let mut h = Harness::new();
    let mut state = SimState::default();

    // M 模式写 `sstatus`（`0x22` = `SIE` | `SPIE`，只碰这两位）。
    let outcome = csrw_with(&mut h, &mut state, CSR_SSTATUS, 1, 0x22);
    assert_eq!(outcome, StepOutcome::Advanced);

    // 切到 S 模式再读。
    csr::priv_set(&mut state, PRIV_S);
    let (outcome, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        value, 0x22,
        "M 模式写 sstatus ⇒ S 模式读回同一份存储（F30）"
    );
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x22,
        "mstatus 上落的就是那两位（同一份存储，F30）"
    );
}

/// **客体的写掩码是实测的那一个**（**F41**）—— 不是旧口径的 `0x22`。
///
/// ⚠️ 判别式是**架构字面量**（不是从被测常量推出来的）：
/// * **S 模式**写 `sstatus = 0xFFFFFFFF` ⇒ ⇒ 读回 **`0x800C_6122`**
///   （= 写掩码 ∧ 读视图 ⇒ ⇒ **逐字**是参考侧实测的那个值）；
/// * **M 模式**写 `sstatus = 0xFFFFFFFF` ⇒ ⇒ `mstatus` 读回
///   **`0x800C_6122`**（= 写掩码 `0x000C_6122` ∪ 推导位 `SD`）。
///
/// ⚠️ **S 模式与 M 模式的写掩码相同**（F39）⇒ ⇒ 参考侧**不按特权裁剪**
/// ⇒ ⇒ 两条断言合起来把「写掩码」钉死。
#[test]
fn the_guest_write_mask_of_sstatus_is_the_measured_one() {
    // 写掩码本身：**逐位**实测（F41）= `0x000C_6122`。
    // ⚠️ 这里用**架构字面量**钉住它 ⇒ ⇒ 改了本地常量而忘了改语义，这条会红。
    assert_eq!(
        SSTATUS_WRITE_MASK, 0x000C_6122,
        "sstatus 的客体写掩码必须是逐位实测的那个值（F41）"
    );
    // S 模式：写全 1 ⇒ 读回 `0x800C_6122`。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_SSTATUS, 1, 0xFFFF_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value, 0x800C_6122,
        "S 模式写 sstatus 全 1 ⇒ 读回 0x800C6122（F39 实测）"
    );

    // M 模式：写全 1 ⇒ `mstatus` 读回 `0x800C_6122`（写掩码 ∪ 推导位 `SD`）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_SSTATUS, 1, 0xFFFF_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x800C_6122,
        "M 模式写 sstatus 全 1 ⇒ mstatus 读回 0x800C6122（F41 逐位实测：\
         写掩码 0x000C6122 ∪ 推导位 SD）"
    );
}

/// **S 模式可以写 `sstatus.SPP`** —— **本轮新增的判别点**。
///
/// ⚠️ **实测（F39）**：S 模式写 `sstatus = 0x100` ⇒ ⇒ 读回 **`0x0000_0100`**
/// ⇒ ⇒ **参考侧允许 S 模式写 `sstatus.SPP`**（与特权规范不同，本平台**对齐
/// 参考侧** —— 黄金对拍的口径就是参考侧）。
///
/// ⚠️ **旧口径的 `0x22` 写掩码会把 `SPP` 挡在外面** ⇒ ⇒ 旧实现读回 **0**
/// ⇒ ⇒ 这条用例对「写掩码取错」有**判别力**。
#[test]
fn supervisor_mode_can_write_the_spp_bit_of_sstatus() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_SSTATUS, 1, 0x100);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value, 0x0000_0100,
        "S 模式写 sstatus.SPP ⇒ 读回 0x100（F39 实测）"
    );
    assert_eq!(
        value & MSTATUS_SPP,
        MSTATUS_SPP,
        "S 模式写进去的 SPP 必须真的落在 mstatus.SPP 上（F39）"
    );
    assert_eq!(
        csr_value(&state, CSR_MSTATUS) & MSTATUS_SPP,
        MSTATUS_SPP,
        "mstatus.SPP 必须真的被写进去（同一份存储，F30）"
    );
}

/// **读视图**里**含 `SD`**（**F39**）—— 旧口径的 `0x122` 把 `SD` 挡在外面。
///
/// ⚠️ **两步**：① M 模式把 `mstatus.FS` **变脏**（写 `0x6000` ⇒ `FS` = 0b11）
/// ⇒ ⇒ 硬件把 `SD`（bit 31）**置位**（`SD` 是**只读**的，由 `FS` / `XS` 决定）
/// ⇒ ⇒ ② S 模式读 `sstatus` ⇒ ⇒ **看得见 `SD`**。
#[test]
fn the_read_view_of_sstatus_contains_sd() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // M 模式：把 `mstatus.FS` 变脏 ⇒ `SD` 由硬件置位。
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0x6000);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x8000_6000,
        "FS 变脏 ⇒ SD 置位（SD 是只读的，由 FS / XS 决定）"
    );

    csr::priv_set(&mut state, PRIV_S);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value & 0x8000_0000,
        0x8000_0000,
        "sstatus 的读视图**含 SD** ⇒ S 模式看得见 FS 变脏（F39）"
    );
    assert_eq!(
        value, 0x8000_6000,
        "S 模式读 sstatus ⇒ FS[14:13] 与 SD 都看得见（F39）"
    );
}

/// **`SD` 是推导位**（**F40**）—— S 模式经 `sstatus` 把 `FS` 写成 3 ⇒ ⇒
/// 读回的 `sstatus` 里**必须带 `SD`**。
///
/// ⚠️ **实测（F40）**：只把 `FS` 写成 3（`0x6000`）且**不碰 bit 31** ⇒ ⇒
/// 参考侧读回 **`0x8000_6000`** ⇒ ⇒ `SD` 被**推成 1** ⇒ ⇒ `SD` 是**推导
/// 位**，不是存储位。
///
/// ⚠️ **为什么这条要紧**：黄金语料 `local/smode_csr` 的**第 12 条检查**
/// 就是「把 `FS` 写成 3 之后 S 模式读 `sstatus` 要看到 `SD`」⇒ ⇒ 若内核
/// 按纯存储实现 ⇒ ⇒ 那一条**红** ⇒ ⇒ 与参考侧**逐位不等**。
///
/// ⚠️ 判别式用**架构字面量** `0x0000_6000`（`FS = 3`）与 `0x8000_0000`
/// （`SD`）—— **不**从契约层的常量推。
#[test]
fn supervisor_mode_writing_fs_through_sstatus_sees_the_derived_sd_bit() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);

    // S 模式经 `sstatus` 把 `FS` 写成 3。
    let outcome = csrw_with(&mut h, &mut state, CSR_SSTATUS, 1, 0x0000_6000);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSTATUS) & 0x0000_6000,
        0x0000_6000,
        "FS 必须真的落在 mstatus 上"
    );

    // 读回的 `sstatus` 里 bit 31（`SD`）是 1 —— 那是**推导**出来的。
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value & 0x8000_0000,
        0x8000_0000,
        "SD 必须跟着 FS 一起被推导出来（F40）"
    );
    assert_eq!(
        value & 0x0000_6000,
        0x0000_6000,
        "FS[14:13] 也必须看得见（F40）"
    );
}

/// **直接写 `SD` 无效**（**F40**）—— `SD` **不是**存储位。
///
/// ⚠️ **实测（F40）**：写 `mstatus = 0x8000_0000` ⇒ ⇒ 参考侧读回
/// **`0x0000_0000`** ⇒ ⇒ 直接写 bit 31 **无效**。
///
/// ⚠️ **判别力**：一个「按纯存储实现 `SD`」的错法会读回 `0x8000_0000`
/// ⇒ ⇒ 这条用例**必红** ⇒ ⇒ 它与「`FS` 变脏 ⇒ `SD` 被推成 1」那一条
/// 合起来才把「推导位」钉死。
#[test]
fn writing_the_sd_bit_directly_has_no_effect() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0x8000_0000);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x0000_0000,
        "直接写 SD 无效 ⇒ 读回 0（F40 实测）"
    );
}

/// **`mstatus` 的每一个可写位都落到 `sstatus` 的视图里吗？** —— 只有**实测**
/// 出来的那**若干位**。
///
/// ⚠️ **这是视图掩码最锐利的一条判据**：M 模式把 `mstatus` 的**全部**
/// 可写位（`0x807E79AA`）都置上 ⇒ ⇒ S 模式读 `sstatus` 看到的**必须**
/// 恰好是 **`0x800C_6122`** —— 多一位（视图太宽）或少一位（视图太窄）都红。
///
/// ⚠️ **实测（F39）**：参考侧「M 模式写 `mstatus` 全 1 ⇒ 读 `sstatus`」
/// 的观测值**就是** `0x800C_6122` ⇒ ⇒ 视图掩码 = `0x800C_6122` 与
/// `mstatus` 的可写位的交集 = **`0x800D_E162`**。
#[test]
fn the_view_of_sstatus_is_the_measured_one() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 写全 1 ⇒ `mstatus` 的 WARL 掩码留下 `0x807E79AA`（实测掩码，F6）。
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0xFFFF_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MSTATUS),
        0x807E_79AA,
        "mstatus 的实测 WARL 掩码（F6）"
    );

    csr::priv_set(&mut state, PRIV_S);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value, 0x800C_6122,
        "sstatus 的读视图必须**恰好**是实测的那一个（F39）"
    );
    assert_eq!(
        SSTATUS_VIEW_MASK & 0x8000_0000,
        0x8000_0000,
        "视图**含 SD**（F39）"
    );
}

// ---------------------------------------------------------------------------
// The trap entry —— two levels
// ---------------------------------------------------------------------------

/// **委托路由**（验收 **B-14**）：`medeleg[cause]` = 0 ⇒ 落 `mtvec`；
/// = 1 ⇒ 落 `stvec`。
#[test]
fn delegation_routes_the_trap_to_stvec_or_mtvec() {
    // cause 9（S 模式的环境调用）**未委托** ⇒ ⇒ 落 `mtvec`，写 `mcause`。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(
        state.pc, MTVEC,
        "未委托的陷阱落 mtvec（mcause / mepc / mtval）"
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        CAUSE_SUPERVISOR_ECALL,
        "未委托 ⇒ 写的是 mcause"
    );
    assert_eq!(csr_value(&state, CSR_SEPC), 0, "未委托 ⇒ **不**写 sepc");

    // 同一件事**委托**了 ⇒ ⇒ 落 `stvec`，写 `scause`。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 1 << CAUSE_SUPERVISOR_ECALL);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(state.pc, STVEC, "委托的陷阱落 stvec");
    assert_eq!(
        csr_value(&state, CSR_SCAUSE),
        CAUSE_SUPERVISOR_ECALL,
        "委托 ⇒ 写的是 scause"
    );
    assert_eq!(csr_value(&state, CSR_SEPC), PC, "委托 ⇒ sepc 记精确的 PC");
    assert_eq!(csr_value(&state, CSR_MCAUSE), 0, "委托 ⇒ **不**写 mcause");
}

/// **委托陷阱之后 `sstatus.SPP` = 1**（**F28**，验收 **B-17**）。
///
/// ⚠️ 两条断言合起来才钉住它：
/// ① `SPP` 读回 **1**（平台自己的写**不受**客体的 `0x22` 约束）；
/// ② `sret` 从陷阱处理器**回到 S 模式**（不是 U 模式）。
#[test]
fn a_delegated_trap_sets_spp_to_one_and_sret_returns_to_supervisor_mode() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 1 << CAUSE_SUPERVISOR_ECALL);

    let outcome = step(&mut h, &mut state, PC, ecall());
    assert!(matches!(outcome, StepOutcome::Trapped { .. }));

    // ① SPP = 1。
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value & MSTATUS_SPP,
        MSTATUS_SPP,
        "委托陷阱之后 sstatus.SPP 必须是 1（F28）"
    );

    // ② sret 回到 **S 模式**。
    csr::csr_write(&mut state, CSR_SEPC, 0x4000);
    let outcome = step(&mut h, &mut state, STVEC, sret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.pc, 0x4000, "sret 跳到 sepc");
    assert_eq!(
        csr::priv_get(&state),
        PRIV_S,
        "从陷阱处理器 sret ⇒ 回到 **S 模式**（SPP = 1）"
    );
}

/// **U 模式陷入（委托）⇒ `SPP` = 0** ⇒ ⇒ `sret` 落到 **U 模式**。
#[test]
fn a_delegated_trap_from_user_mode_sets_spp_to_zero() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    arm(&mut state, 1 << CAUSE_USER_ECALL);

    let outcome = step(&mut h, &mut state, PC, ecall());
    assert!(matches!(outcome, StepOutcome::Trapped { .. }));
    let (_, value) = guest_read(&mut h, &mut state, CSR_SSTATUS);
    assert_eq!(
        value & MSTATUS_SPP,
        0,
        "U 模式陷入 ⇒ SPP = 0（陷入前的级别是 U）"
    );
    assert_eq!(
        csr::priv_get(&state),
        PRIV_S,
        "委托的陷阱 ⇒ 陷阱在 **S 模式**下被处理"
    );
}

/// **未委托**的陷阱 ⇒ ⇒ 陷阱在 **M 模式**下被处理（**F20** 的机制）。
///
/// ⚠️ 这条很关键：M 级的陷阱向量要读 `mcause`（**M 模式 CSR**）⇒ ⇒ 若
/// 特权级留在 S / U，那一次读就是非法指令 ⇒ ⇒ 再次陷入 ⇒ ⇒ **挂死**。
#[test]
fn an_undelegated_trap_is_taken_in_machine_mode() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert!(matches!(outcome, StepOutcome::Trapped { .. }));
    assert_eq!(
        csr::priv_get(&state),
        PRIV_M,
        "未委托的陷阱 ⇒ 陷阱在 **M 模式**下被处理"
    );
    // MPP 记的是**陷入前的级别**（S）⇒ ⇒ `mret` 能回到 S 模式。
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & MSTATUS_MPP_FIELD,
        MPP_S << 11,
        "未委托的陷阱 ⇒ mstatus.MPP 记陷入前的级别（S）"
    );
}

// ---------------------------------------------------------------------------
// The instruction semantics —— `ecall` / `ebreak`
// ---------------------------------------------------------------------------

/// **`ecall` 的 cause 按级**（验收 **B-5** / **B-20**）。
///
/// ⚠️ 三个数字都是**架构字面量**：`11` / `9` / `8`。
#[test]
fn ecall_reports_the_cause_of_the_current_privilege_level() {
    // M 模式 ⇒ 11。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_M);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        CAUSE_MACHINE_ECALL,
        "M 模式的 ecall ⇒ cause 11"
    );

    // S 模式 ⇒ 9。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        CAUSE_SUPERVISOR_ECALL,
        "S 模式的 ecall ⇒ cause 9"
    );

    // U 模式 ⇒ 8。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        }
    );
    assert_eq!(
        csr_value(&state, CSR_MCAUSE),
        CAUSE_USER_ECALL,
        "U 模式的 ecall ⇒ cause 8"
    );
}

/// **`ebreak` 的 cause 恒为 3**（验收 **B-4**）—— 三级一样。
///
/// ⚠️ 工具链把 `sbreak` 汇编成 `ebreak`（**F32**）⇒ ⇒ 不需要新的操作码。
#[test]
fn ebreak_reports_cause_three_in_every_privilege_level() {
    for &level in &[PRIV_M, PRIV_S, PRIV_U] {
        let mut h = Harness::new();
        let mut state = state_in(level);
        arm(&mut state, 0);
        let outcome = step(&mut h, &mut state, PC, ebreak_insn());
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::Breakpoint
            },
            "ebreak 在级别 {level} ⇒ 断点"
        );
        assert_eq!(
            csr_value(&state, CSR_MCAUSE),
            CAUSE_BREAKPOINT,
            "ebreak 在级别 {level} ⇒ cause 恒为 3"
        );
    }
}

// ---------------------------------------------------------------------------
// `mret` / `sret`
// ---------------------------------------------------------------------------

/// **`mret` 的语义**（验收 **B-1**）。
///
/// ⚠️ 判别式是**架构字面量**：`MIE` = bit 3、`MPIE` = bit 7、`MPP` 字段。
///
/// ⚠️ **`MPIE` / `MIE` 的方向按特权规范**：`mret` 做的是
/// **`MIE := 旧 MPIE`**、`MPIE := 1` ⇒ ⇒ 「旧 `MPIE` 是 1 ⇒ `MIE` 变成 1」
/// 这条断言**只能**在这种读法下变绿。
#[test]
fn mret_restores_mie_from_mpie_sets_mpie_and_clears_mpp() {
    let mut h = Harness::new();
    // 旧 mstatus：MIE = 1、MPIE = 0、MPP = M。
    let mut state = state_with(PRIV_M, &[(CSR_MSTATUS, MSTATUS_MIE | (MPP_M << 11))]);
    csr::csr_write(&mut state, CSR_MEPC, 0x1003);

    let outcome = step(&mut h, &mut state, PC, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.pc, 0x1002, "mret 跳到 mepc，按 IALIGN 只清最低 1 位");

    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & MSTATUS_MIE,
        0,
        "MIE := 旧 MPIE（旧 MPIE 是 0）⇒ MIE 必须是 0"
    );
    assert_ne!(mstatus & MSTATUS_MPIE, 0, "MPIE := 1（特权规范）");
    assert_eq!(
        mstatus & MSTATUS_MPP_FIELD,
        MPP_U << 11,
        "mret 之后 MPP := U(0)"
    );
}

/// **`mret` 的特权级来自旧的 `MPP`**（验收 **B-19**）。
///
/// ⚠️ `MPP` = 1 ⇒ ⇒ 落到 **S 模式**；`MPP` = 0 ⇒ ⇒ 落到 **U 模式**。
#[test]
fn mret_takes_the_privilege_level_from_the_old_mpp() {
    // MPP = S ⇒ 落到 S 模式。
    let mut h = Harness::new();
    let mut state = state_with(PRIV_M, &[(CSR_MSTATUS, MPP_S << 11)]);
    let outcome = step(&mut h, &mut state, PC, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr::priv_get(&state),
        PRIV_S,
        "旧的 MPP = 1 ⇒ mret 落到 **S 模式**"
    );

    // MPP = U ⇒ 落到 U 模式。
    let mut h = Harness::new();
    let mut state = state_with(PRIV_M, &[(CSR_MSTATUS, MPP_U << 11)]);
    let outcome = step(&mut h, &mut state, PC, mret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr::priv_get(&state),
        PRIV_U,
        "旧的 MPP = 0 ⇒ mret 落到 **U 模式**"
    );
}

/// **`mret` 在 S 模式 ⇒ cause 2**（验收 **B-15**）。
///
/// ⚠️ `mepc` 在 S 模式**不可访问** ⇒ ⇒ S 模式**根本回不去 M 模式**。
#[test]
fn mret_in_supervisor_mode_is_an_illegal_instruction() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, mret_insn());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "S 模式的 mret ⇒ 非法指令（F23 / B-15）"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);
}

/// **`sret` 的语义**（验收 **B-2**）。
///
/// ⚠️ 判别式：`SIE` = bit 1、`SPIE` = bit 5、`SPP` = bit 8。
#[test]
fn sret_restores_sie_from_spie_sets_spie_and_clears_spp() {
    let mut h = Harness::new();
    // 旧 mstatus：SIE = 1、SPIE = 0、SPP = 0（普通 S 模式，没经过陷阱）。
    let mut state = state_with(
        PRIV_S,
        &[
            (CSR_MSTATUS, MSTATUS_SIE | (MPP_M << 11)),
            (CSR_SEPC, 0x5000),
        ],
    );

    let outcome = step(&mut h, &mut state, PC, sret_insn());
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(state.pc, 0x5000, "sret 跳到 sepc");

    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & MSTATUS_SIE,
        0,
        "SIE := 旧 SPIE（旧 SPIE 是 0）⇒ SIE 必须是 0"
    );
    assert_ne!(mstatus & MSTATUS_SPIE, 0, "SPIE := 1（特权规范）");
    assert_eq!(mstatus & MSTATUS_SPP, 0, "sret 之后 SPP := 0");
    assert_eq!(
        csr::priv_get(&state),
        PRIV_U,
        "SPP = 0 ⇒ sret 落到 **U 模式**（F9）"
    );
}

/// **`sret` 在 M 模式 ⇒ cause 2**（验收 **B-15**）。
#[test]
fn sret_in_machine_mode_is_an_illegal_instruction() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_M);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, sret_insn());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "M 模式的 sret ⇒ 非法指令（B-15）"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);
}

/// **`sret` 在 U 模式 ⇒ cause 2**（验收 **B-15**）。
#[test]
fn sret_in_user_mode_is_an_illegal_instruction() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    arm(&mut state, 0);
    let outcome = step(&mut h, &mut state, PC, sret_insn());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "U 模式的 sret ⇒ 非法指令（B-15）"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);
}

// ---------------------------------------------------------------------------
// The WARL rules of the S-mode CSRs
// ---------------------------------------------------------------------------

/// **`stvec` / `mtvec` 的 MODE 字段是 1 位 WARL**（**F13**，验收 **B-8**）。
///
/// ⚠️ 写 **2** ⇒ 读 **0**；写 **3** ⇒ 读 **1**。
#[test]
fn the_tvec_mode_field_is_one_bit_warl() {
    // `stvec`（S 模式）。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_STVEC, 1, STVEC | 2);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_STVEC);
    assert_eq!(value, STVEC, "stvec 写 MODE=2 ⇒ 读回 0（F13）");

    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_STVEC, 1, STVEC | 3);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_STVEC);
    assert_eq!(value, STVEC | 1, "stvec 写 MODE=3 ⇒ 读回 1（F13）");

    // `mtvec`（M 模式）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MTVEC, 1, MTVEC | 2);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_MTVEC);
    assert_eq!(value, MTVEC, "mtvec 写 MODE=2 ⇒ 读回 0（F13）");

    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MTVEC, 1, MTVEC | 3);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_MTVEC);
    assert_eq!(value, MTVEC | 1, "mtvec 写 MODE=3 ⇒ 读回 1（F13）");
}

/// **`sepc` 的 bit 0 恒 0**（**F15**，验收 **B-9**）。
#[test]
fn sepc_bit_zero_is_always_zero() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let outcome = csrw_with(&mut h, &mut state, CSR_SEPC, 1, 0x1235);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SEPC);
    assert_eq!(
        value, 0x1234,
        "sepc 的 bit 0 恒 0（IALIGN = 32 的口径，F15）"
    );
    assert_eq!(value & 1, 0, "sepc 的最低位必须是 0");
}

/// **`scause` / `stval` 可写**（**F16**，验收 **B-10**）。
#[test]
fn scause_and_stval_are_writable() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);

    let outcome = csrw_with(&mut h, &mut state, CSR_SCAUSE, 1, 0x55);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SCAUSE);
    assert_eq!(value, 0x55, "scause 必须可写（F16）");

    let outcome = csrw_with(&mut h, &mut state, CSR_STVAL, 1, 0x77);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_STVAL);
    assert_eq!(value, 0x77, "stval 必须可写（F16）");
}

/// **`sie` / `sip` 的写受 `mideleg` 门控**（**F18**，验收 **B-11**）。
///
/// ⚠️ `mideleg` 的 bit 1 = `SSIP` ⇒ ⇒ 门控的就是 `0x2`。
#[test]
fn sie_and_sip_writes_are_gated_by_mideleg() {
    // `mideleg` 没置位 ⇒ ⇒ 写 `0x2` 读回 **0**。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, 0x303, 0);
    let outcome = csrw_with(&mut h, &mut state, CSR_SIE, 1, 0x2);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SIE);
    assert_eq!(value, 0, "mideleg 没置位 ⇒ sie 写不进去（F18）");

    // `mideleg` 置位 ⇒ ⇒ 写 `0x2` 读回 **2**。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, 0x303, 0x2);
    let outcome = csrw_with(&mut h, &mut state, CSR_SIE, 1, 0x2);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SIE);
    assert_eq!(value, 0x2, "mideleg 置位 ⇒ sie 写得进去（F18）");

    // `sip` 同形。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, 0x303, 0x2);
    let outcome = csrw_with(&mut h, &mut state, CSR_SIP, 1, 0x2);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SIP);
    assert_eq!(value, 0x2, "mideleg 置位 ⇒ sip 写得进去（F18）");
}

/// **`satp` 只支持 Bare**（验收 **B-13**）—— 写 `0x1` 读回 **0**。
#[test]
fn satp_only_supports_bare() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_SATP, 1, 0x1);
    assert_eq!(outcome, StepOutcome::Advanced);
    let (_, value) = guest_read(&mut h, &mut state, CSR_SATP);
    assert_eq!(value, 0, "satp 只支持 Bare ⇒ 写 0x1 读回 0（B-13）");
}

// ---------------------------------------------------------------------------
// The counter enable chain
// ---------------------------------------------------------------------------

/// **计数器的使能链**（验收 **B-12**，**F19**）。
///
/// ⚠️ S 模式读 `cycle` 需要 `mcounteren.CY` **且** `scounteren.CY` ⇒ ⇒
/// 缺一 ⇒ ⇒ **cause 2**（不是「读回 0」）。
#[test]
fn cycle_in_supervisor_mode_needs_the_enable_chain() {
    // 两条都缺 ⇒ cause 2。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "使能链不完整 ⇒ 非法指令（F19 / B-12）"
    );
    assert_eq!(csr_value(&state, CSR_MCAUSE), CAUSE_ILLEGAL);

    // 只置 `mcounteren` ⇒ 仍然 cause 2。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, CSR_MCOUNTEREN, 0x1);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "只置 mcounteren ⇒ 仍然非法指令（F19）"
    );

    // 只置 `scounteren` ⇒ 仍然 cause 2。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, CSR_SCOUNTEREN, 0x1);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "只置 scounteren ⇒ 仍然非法指令（F19）"
    );

    // 两条都置 ⇒ 可以读。
    let mut h = Harness::new();
    let mut state = state_in(PRIV_S);
    csr::csr_write(&mut state, CSR_MCOUNTEREN, 0x1);
    csr::csr_write(&mut state, CSR_SCOUNTEREN, 0x1);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "两条都置 ⇒ S 模式可以读 cycle（F19）"
    );
}

/// **U 模式的使能链多一条** —— 还要 `ucounteren`（验收 **B-12**）。
#[test]
fn cycle_in_user_mode_needs_ucounteren_as_well() {
    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    csr::csr_write(&mut state, CSR_MCOUNTEREN, 0x1);
    csr::csr_write(&mut state, CSR_SCOUNTEREN, 0x1);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "U 模式还差 ucounteren ⇒ 非法指令"
    );

    let mut h = Harness::new();
    let mut state = state_in(PRIV_U);
    csr::csr_write(&mut state, CSR_MCOUNTEREN, 0x1);
    csr::csr_write(&mut state, CSR_SCOUNTEREN, 0x1);
    csr::csr_write(&mut state, CSR_UCOUNTEREN, 0x1);
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "三条都置 ⇒ U 模式可以读 cycle"
    );
}

/// **M 模式不需要使能链** —— 直接可读。
#[test]
fn cycle_in_machine_mode_needs_no_enable_chain() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let (outcome, _) = guest_read(&mut h, &mut state, CSR_CYCLE);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "M 模式读 cycle 不需要使能链"
    );
}

// ---------------------------------------------------------------------------
// `medeleg` —— the legal bit set (F40)
// ---------------------------------------------------------------------------

/// **`medeleg` 的写受合法位集合约束**（**F40**，per-bit WARL）。
///
/// ⚠️ **实测**：写 `0xFFFFFFFF` ⇒ 读回 **`0x0008_B3FE`**；写 `0x0000_FFFF`
/// ⇒ 读回 **`0x0000_B3FE`** ⇒ ⇒ 两者都是「请求值 ∧ **同一个**合法位集合」。
///
/// ⚠️ **这条有独立落点**（M-13 的 CSR 写路径自己的臂 + 契约层自己的常量）
/// ⇒ ⇒ 不是塞进 `mstatus_warl` 之类的地方。
#[test]
fn medeleg_writes_are_masked_to_the_legal_bit_set() {
    let mut h = Harness::new();
    let mut state = SimState::default();

    // 写全 1 ⇒ 读回 `0x0008_B3FE`。
    let outcome = csrw_with(&mut h, &mut state, CSR_MEDELEG, 1, 0xFFFF_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MEDELEG),
        MEDELEG_LEGAL_BITS,
        "medeleg 写全 1 ⇒ 读回 0x0008B3FE（F40 实测）"
    );

    // 写低 16 位 ⇒ 读回 `0x0000_B3FE`（同一个掩码的另一条证据）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MEDELEG, 1, 0x0000_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MEDELEG),
        0x0000_B3FE,
        "medeleg 写低 16 位 ⇒ 读回 0x0000B3FE（F40 实测）"
    );

    // 写 `0xB109` ⇒ 读回 `0xB108`（bit 0 不可写 —— 「指令地址未对齐」不是
    // 一个可委托的陷阱）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MEDELEG, 1, 0xB109);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(
        csr_value(&state, CSR_MEDELEG),
        0x0000_B108,
        "medeleg 写 0xB109 ⇒ 读回 0xB108（F40 实测：bit 0 不可写）"
    );
}

// ---------------------------------------------------------------------------
// The vector table offset
// ---------------------------------------------------------------------------

/// **向量模式的偏移取 `cause & 0x7`** —— 不是 `0x3`。
///
/// ⚠️ **口径**：既有 `mtvec` 实现用的就是 `0x7` ⇒ ⇒ 两个向量表**同口径**
/// ⇒ ⇒ **零黄金回归**（语料的 env 用的是**直接**模式）。
///
/// ⚠️ **判别式**：cause **5**（load access fault）⇒ ⇒ 偏移 = `4 × 5` =
/// **20** ⇒ ⇒ `0x3` 口径会算成 **12** ⇒ ⇒ 两者**差 8 字节** ⇒ ⇒ 这条
/// 用例对「偏移掩码取错」有判别力。
#[test]
fn the_vector_offset_uses_the_low_three_bits_of_the_cause() {
    let mut state = SimState::default();
    // `mtvec` = 基址 | 1（向量模式）。
    csr::csr_write(&mut state, CSR_MTVEC, MTVEC | 1);
    let target = csr::enter_trap(&mut state, 5, 0);
    assert_eq!(
        target,
        MTVEC + 20,
        "向量模式的偏移 = 4 × (cause & 0x7) ⇒ cause 5 ⇒ +20"
    );
}

// ---------------------------------------------------------------------------
// `mstatus` —— the WARL function
// ---------------------------------------------------------------------------

/// **`mstatus` 的 `MPP` 保留值语义**（**F39**，验收 **B-21**）。
///
/// ⚠️ 判别式是**架构字面量** `0x0002`。
///
/// ⚠️ **合法化到 0**：写 `0x1002`（`MPP = 2` 且 `SIE = 1`）⇒ ⇒ 读回
/// **`0x0002`** ⇒ ⇒ `MPP` **被覆盖成 0**（U），**不是**保留旧值 ⇒ ⇒ 但
/// `SIE` **落进去了** ⇒ ⇒ 也**不是**「整次丢弃」。
///
/// ⚠️ **旧口径的「保留旧值」是错的**：它来自**被 8 位退出码截断**的观测 ⇒
/// ⇒ 截断值 `0x02` 被当成了完整值 ⇒ ⇒ 旧值到底是 0 还是 3 分不出来。
#[test]
fn mstatus_legalizes_the_reserved_mpp_to_zero_but_lands_the_other_bits() {
    // 旧值 0 ⇒ 写 `0x1002` ⇒ 结果 `0x0002`。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0x1002);
    assert_eq!(outcome, StepOutcome::Advanced);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus, 0x0002,
        "写 MPP=2 且 SIE=1 ⇒ 读回 0x0002（F39：MPP 合法化到 0）"
    );
    assert_ne!(
        mstatus & MSTATUS_SIE,
        0,
        "同一次写里的 SIE **落进去了**（不是整次丢弃）"
    );

    // 旧值 3 ⇒ 写 `0x1002` ⇒ 结果**仍是** `0x0002`（旧值被**覆盖**成 0）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, MPP_M << 11);
    assert_eq!(outcome, StepOutcome::Advanced);
    assert_eq!(csr_value(&state, CSR_MSTATUS), MPP_M << 11);

    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0x1002);
    assert_eq!(outcome, StepOutcome::Advanced);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus, 0x0002,
        "旧的 MPP=3 ⇒ 写保留值 ⇒ 读回**仍是** 0x0002（F39：旧值被覆盖成 0）"
    );
    assert_eq!(
        mstatus & 0x0000_1800,
        0,
        "MPP 必须被覆盖成 0（U），**不是**保留旧值 3"
    );
}

/// **`mstatus` 的保留位写被忽略**（台账 **D-98**）—— 掩码是实测值。
///
/// ⚠️ 判别式用的是**架构字面量**（`0x15` / `0x7F80_0000`）。
#[test]
fn mstatus_reserved_bits_are_dropped_on_write() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = csrw_with(&mut h, &mut state, CSR_MSTATUS, 1, 0xFFFF_FFFF);
    assert_eq!(outcome, StepOutcome::Advanced);
    let mstatus = csr_value(&state, CSR_MSTATUS);
    assert_eq!(
        mstatus & 0x0000_0015,
        0,
        "mstatus 的保留位（0/2/4）写被忽略 ⇒ 读回必须是 0，实际 {mstatus:#010x}"
    );
    assert_eq!(
        mstatus & 0x7F80_0000,
        0,
        "mstatus 的保留位（23~30）写被忽略 ⇒ 读回必须是 0，实际 {mstatus:#010x}"
    );
    // ⚠️ **本平台不支持的扩展位**也写不进去（**F6**：实测掩码 `0x807E79AA`）。
    //    那几位是：`UBE`(6) · `VS`(9:10) · `XS`(15:16)。
    assert_eq!(
        mstatus & 0x0001_8640,
        0,
        "VS / XS / UBE 这些「扩展不存在就不该可写」的位必须被丢掉（F6）"
    );
}
