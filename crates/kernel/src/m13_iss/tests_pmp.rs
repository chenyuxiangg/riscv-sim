//! M-13 的 PMP —— 物理内存保护（STAGE-21 · v0.37）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-21_V037_PMP.md`
//! §3（内核设计）、§5.2（B 组验收 B-1 ~ B-9）与 §1.2（参考侧**实测**事实
//! F1 ~ F20）。
//!
//! ## 这些用例在看什么
//!
//! * **CSR 文件的内容**（[`csr_value`] 直接读文件）—— 写路径与读路径
//!   分开看 ⇒ ⇒ 读路径自己写错也看不出来；
//! * **`StepOutcome` 的形状**（`Advanced` / `Trapped { cause }`）；
//! * **`mtval`**（陷阱带出的地址）。
//!
//! ## 判别力
//!
//! ⚠️ 每条用例都必须**能红**。凡是「一个断言在典型输入下有等价写法」的
//! 地方，本文件都**配了一条让两者不等价的输入**：
//!
//! | 容易写成「恒真」的断言 | 本文件配的判别式 |
//! | --- | --- |
//! | 「`MPRV` 置 1 就被拒」（「**全部**被拒」也能过） | **控制组**：`MPRV` = 0 时同一条访存**必须成功** |
//! | 「锁住的条目会管住 M 模式」（「锁**没**生效」也能过） | 同一条访存在 `MPRV` = 0 时**放行**、在 `L` = 1 时**被拒** |
//! | 「最低编号的条目说了算」（「任一放行即可」也能过） | **两个方向各一条**：低编号禁写 / 低编号放行 |
//! | 「写回等于写进去」（**合法化**也能过） | 合法值**逐字**读回 + 保留组合**被改写** |
//! | 「取指要 `X`」（「取指永远被拒」也能过） | **控制组**：给 `X` 之后取指**放行** |
//! | 「跨界访问**被拒**」（「一律按整段判」也能过） | **两个方向各一条**：尾部落进 DENY ⇒ **拒** / 跨两个 ALLOW ⇒ **放行** |
//!
//! ## v0.39：逐片判（对参考侧实测）
//!
//! ⚠️ 未对齐的 4 字节访问**跨过** 4 字节块边界 ⇒ ⇒ 参考侧**逐片**判 ⇒ ⇒
//! 平台的旧规则（「决定条目必须覆盖访问的**全部**字节」）**两侧都错** ⇒ ⇒
//! 这一节把新规则钉住，并给每条用例配**必要对照**。

use riscv_sim_contract::csr::{
    PmpAccess, CSR_MSTATUS, CSR_PMPADDR0, CSR_PMPCFG0, MSTATUS_MPRV_BIT,
};
use riscv_sim_contract::model::{SimState, WriteSize};

use super::executor::{StepOutcome, TrapCause};
use super::pmp::pmp_allows;
use super::tests_common::*;

// ---------------------------------------------------------------------------
// Encodings / drivers
// ---------------------------------------------------------------------------

/// `lw rd, imm(rs1)` —— `funct3 == 0b010`.
const F_LW: u8 = 0b010;
/// `sw rs2, imm(rs1)` —— `funct3 == 0b010`.
const F_SW: u8 = 0b010;

/// The address the data accesses touch.
const DATA: u32 = 0x200;
/// The address the fetch-denial test jumps to.
const FETCH_TARGET: u32 = 0x2000;

/// `lw rd, 0(rs1)`.
fn lw(rd: u8, rs1: u8) -> u32 {
    enc_i(0, rs1, F_LW, rd, LOAD)
}

/// `sw rs2, 0(rs1)`.
fn sw(rs2: u8, rs1: u8) -> u32 {
    enc_s(0, rs2, rs1, F_SW, STORE)
}

/// Write `value` into a PMP register and **read it back out of the file**.
fn write_then_read(csr: u16, value: u32) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = value;
    h.step_one_at(&mut state, BASE, csrw(csr, 1));
    csr_value(&state, csr)
}

/// Set `mstatus.MPRV` (bit 17).
fn set_mprv(h: &mut Harness, state: &mut SimState, pc: u32) {
    state.gpr[1] = 1u32 << MSTATUS_MPRV_BIT;
    h.step_one_at(state, pc, csrrs(CSR_MSTATUS, 1, 0));
}

/// Configure entry 0 (`pmpaddr0` + the byte 0 of `pmpcfg0`).
fn configure_entry0(h: &mut Harness, state: &mut SimState, pc: u32, addr: u32, cfg: u8) {
    state.gpr[1] = addr;
    h.step_one_at(state, pc, csrw(CSR_PMPADDR0, 1));
    state.gpr[1] = u32::from(cfg);
    h.step_one_at(state, pc + 4, csrw(CSR_PMPCFG0, 1));
}

/// Configure entry 1 (the **byte 1** of `pmpcfg0` — RV32 packs four entries
/// per register!).
///
/// ⚠️ **F2 的坑**：条目 1 落在 `pmpcfg0` 的**字节 1** ⇒ ⇒ 写 `pmpcfg1` 配的
/// 其实是条目 4..7 ⇒ ⇒ 这里必须 `<< 8`。
///
/// ⚠️ **`pmpcfg` 是**整寄存器**写**：一次写会把**所有未锁**的字节换成写进去
///    的那个字节 ⇒ ⇒ 条目 0 的配置**必须**一起写进去，否则它会被抹成
///    `A` = OFF（于是条目 0 根本不命中 ⇒ ⇒ 优先级那条断言就变成假绿）。
fn configure_entry1(h: &mut Harness, state: &mut SimState, pc: u32, addr: u32, cfg0: u8, cfg1: u8) {
    state.gpr[1] = addr;
    h.step_one_at(state, pc, csrw(CSR_PMPADDR0 + 1, 1));
    state.gpr[1] = u32::from(cfg1) << 8 | u32::from(cfg0);
    h.step_one_at(state, pc + 4, csrw(CSR_PMPCFG0, 1));
}

/// A store to `DATA` with the PMP configuration already in place.
///
/// Returns the outcome of the **store** instruction.
fn store_to_data(h: &mut Harness, state: &mut SimState, pc: u32) -> StepOutcome {
    state.gpr[2] = DATA;
    h.step_one_at(state, pc, sw(1, 2))
}

// ---------------------------------------------------------------------------
// B-1 — the PMP registers read back what was written
// ---------------------------------------------------------------------------

/// **B-1** —— `pmpaddr` 读回**逐字相同**（F16），`pmpcfg` 读回**逐字相同**
/// （合法值）。
#[test]
fn the_pmp_registers_read_back_what_was_written() {
    assert_eq!(write_then_read(CSR_PMPADDR0, 0x2000_01ff), 0x2000_01ff);
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x1f), 0x1f);
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x98), 0x98);
}

/// ⚠️ **`pmpcfg` 的写入会被合法化**（F20）：保留位 6:5 读回 0；保留组合
/// `R`=0 ∧ `W`=1 丢掉 `W` 位 ⇒ ⇒ **这正是那种条目写不了的原因**（F9）。
#[test]
fn pmpcfg_writes_are_legalised() {
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x1a), 0x18, "保留组合丢掉 W");
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x1e), 0x1c, "保留组合丢掉 W");
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x2a), 0x08, "保留组合丢掉 W");
    assert_eq!(
        write_then_read(CSR_PMPCFG0, 0xff),
        0x9f,
        "保留位 6:5 读回 0"
    );
    // 合法值**逐字**读回 ⇒ ⇒ 合法化不是「一律清零」。
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x1b), 0x1b);
    assert_eq!(write_then_read(CSR_PMPCFG0, 0x04), 0x04);
}

// ---------------------------------------------------------------------------
// B-2 / B-3 — `MPRV`
// ---------------------------------------------------------------------------

/// **B-2** —— `MPRV` = 0 时 M 模式的访存**不受 PMP 约束**（F10）。
///
/// ⚠️ **控制组**：同一条访存在 `MPRV` = 1 时会被拒（F10 的另一半）⇒ ⇒
/// 「全部被拒」的实现过不了这一条。
#[test]
fn m_mode_access_is_exempt_when_mprv_is_clear() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NA4 @ 0x200，只有 `R`（没有 `W`）⇒ ⇒ 若被检查，写必被拒。
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    let outcome = store_to_data(&mut h, &mut state, BASE + 8);
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "MPRV = 0 ⇒ M 模式不受 PMP 约束（实际 {:?}）",
        outcome
    );
}

/// **B-3** —— `MPRV` = 1 时 M 模式的访存**受 PMP 约束**（F10）：写被拒，
/// `mcause` = 7，`mtval` = 出错地址。
#[test]
fn m_mode_access_is_checked_when_mprv_is_set() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    set_mprv(&mut h, &mut state, BASE + 8);
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!("MPRV = 1 ⇒ 写必须被拒（实际 {:?}）", other),
    }
    assert_eq!(csr_value(&state, 0x343), DATA, "mtval 必须是出错地址");
}

/// ⚠️ **读与写分开**：`R` 给了 ⇒ ⇒ 读**放行**；同一个条目没有 `W` ⇒ ⇒ 写被拒。
#[test]
fn a_read_is_allowed_where_a_store_is_denied() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    set_mprv(&mut h, &mut state, BASE + 8);
    state.gpr[2] = DATA;
    let outcome = h.step_one_at(&mut state, BASE + 12, lw(3, 2));
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "条目给了 R ⇒ ⇒ 读必须放行（实际 {:?}）",
        outcome
    );
}

// ---------------------------------------------------------------------------
// B-8 — no matching entry
// ---------------------------------------------------------------------------

/// **B-8** —— **没有条目命中 ⇒ 拒**（F8）：条目只盖住 `0x200`，访问
/// `0x300` ⇒ ⇒ 拒。
#[test]
fn an_access_outside_every_entry_is_denied() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x1f);
    set_mprv(&mut h, &mut state, BASE + 8);
    state.gpr[2] = DATA + 0x100;
    let outcome = h.step_one_at(&mut state, BASE + 12, sw(1, 2));
    match outcome {
        StepOutcome::Trapped { cause } => assert_eq!(cause, TrapCause::StoreAccessFault),
        other => panic!("没有条目命中 ⇒ 必须被拒（实际 {:?}）", other),
    }
}

// ---------------------------------------------------------------------------
// B-5 — the lock bit
// ---------------------------------------------------------------------------

/// **B-5** —— **`L` = 1 ⇒ M 模式也强制执行**（F12）：`MPRV` = 0 也被拒。
///
/// ⚠️ **控制组**：同一个条目**不锁**时（`MPRV` = 0）放行 ⇒ ⇒ 「M 模式一律
/// 被拒」的实现过不了这一条。
#[test]
fn a_locked_entry_is_enforced_in_m_mode() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // `L` = 1，NA4，只有 `R` ⇒ ⇒ 写必被拒。
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x91);
    let outcome = store_to_data(&mut h, &mut state, BASE + 8);
    match outcome {
        StepOutcome::Trapped { cause } => assert_eq!(cause, TrapCause::StoreAccessFault),
        other => panic!("锁住的条目 ⇒ M 模式也必须被拒（实际 {:?}）", other),
    }
}

/// ⚠️ **控制组**：同样的条目**不锁**时，`MPRV` = 0 ⇒ ⇒ 放行。
#[test]
fn an_unlocked_entry_is_exempt_in_m_mode() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    let outcome = store_to_data(&mut h, &mut state, BASE + 8);
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "没锁的条目 ⇒ M 模式旁路（实际 {:?}）",
        outcome
    );
}

/// **B-6** —— **`L` = 1 ⇒ 写 `pmpcfg` / `pmpaddr` 被忽略**（F13）。
///
/// ⚠️ **锁是按条目的**：条目 0 锁住 ⇒ ⇒ 它的**那一个字节**保持原样；
/// **条目 1（同一个寄存器的另一个字节）照常能写** ⇒ ⇒ 「整个寄存器都锁死」
/// 的实现过不了这一条。
#[test]
fn a_locked_entry_ignores_writes() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 锁住条目 0（`L` = 1），顺便把条目 1 配成 `0x1f`。
    state.gpr[1] = DATA >> 2;
    h.step_one_at(&mut state, BASE, csrw(CSR_PMPADDR0, 1));
    state.gpr[1] = 0x1f91; // 字节 0 = 0x91（锁住），字节 1 = 0x1f
    h.step_one_at(&mut state, BASE + 4, csrw(CSR_PMPCFG0, 1));
    assert_eq!(csr_value(&state, CSR_PMPCFG0), 0x1f91);

    // 改条目 0 ⇒ ⇒ 被忽略（保持 0x91）。
    state.gpr[1] = 0x0018;
    h.step_one_at(&mut state, BASE + 8, csrw(CSR_PMPCFG0, 1));
    assert_eq!(
        csr_value(&state, CSR_PMPCFG0) & 0xff,
        0x91,
        "锁住的条目 ⇒ 写被忽略"
    );

    // 改条目 1 ⇒ ⇒ **照常写进去**（同一个寄存器的另一个字节）。
    state.gpr[1] = 0x1318; // 字节 0 = 0x18（锁住 ⇒ 忽略），字节 1 = 0x13（NA4 + R + W）
    h.step_one_at(&mut state, BASE + 12, csrw(CSR_PMPCFG0, 1));
    assert_eq!(
        csr_value(&state, CSR_PMPCFG0) >> 8,
        0x13,
        "没锁的条目 ⇒ 照常能写"
    );
    assert_eq!(
        csr_value(&state, CSR_PMPCFG0) & 0xff,
        0x91,
        "锁住的条目 ⇒ 保持原样"
    );

    // 改 `pmpaddr0` ⇒ ⇒ 被忽略（条目 0 锁着）。
    state.gpr[1] = 0x1234;
    h.step_one_at(&mut state, BASE + 16, csrw(CSR_PMPADDR0, 1));
    assert_eq!(
        csr_value(&state, CSR_PMPADDR0),
        DATA >> 2,
        "锁住的条目 ⇒ pmpaddr 的写被忽略"
    );
}

// ---------------------------------------------------------------------------
// F7 — the priority rule
// ---------------------------------------------------------------------------

/// **F7** —— **最低编号的「命中」条目说了算**。
///
/// ⚠️ **两个方向各一条** ⇒ ⇒ 「任一放行即可」过不了第一条，
/// 「最低编号说了算」过不了第二条。
#[test]
fn the_lowest_numbered_matching_entry_decides() {
    // 条目 0 命中但**没有 W**；条目 1 也命中且**有 W** ⇒ ⇒ 条目 0 说了算 ⇒ 拒。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    configure_entry1(&mut h, &mut state, BASE + 8, DATA >> 2, 0x11, 0x13);
    set_mprv(&mut h, &mut state, BASE + 16);
    let outcome = store_to_data(&mut h, &mut state, BASE + 20);
    match outcome {
        StepOutcome::Trapped { cause } => assert_eq!(cause, TrapCause::StoreAccessFault),
        other => panic!("最低编号的命中条目没有 W ⇒ 必须被拒（实际 {:?}）", other),
    }

    // 条目 0 命中且**有 W**；条目 1 命中但**没有 W** ⇒ ⇒ 放行。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x13);
    configure_entry1(&mut h, &mut state, BASE + 8, DATA >> 2, 0x13, 0x11);
    set_mprv(&mut h, &mut state, BASE + 16);
    let outcome = store_to_data(&mut h, &mut state, BASE + 20);
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "最低编号的命中条目有 W ⇒ 必须放行（实际 {:?}）",
        outcome
    );
}

// ---------------------------------------------------------------------------
// B-4 — the fetch
// ---------------------------------------------------------------------------

/// **B-4** —— **取指要 `X` 权限**（F14）：没有 `X` ⇒ ⇒ `mcause` = 1
/// （instruction access fault），`mtval` = 取指地址。
///
/// ⚠️ 取指**不受 `MPRV` 影响**（F11）⇒ ⇒ 这里**不**设 `MPRV`，只靠 `L` = 1
/// 让条目在 M 模式也生效。
#[test]
fn an_instruction_fetch_needs_the_x_permission() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NAPOT 4 KiB @ 0x2000，`L` = 1，**无权限** ⇒ ⇒ 取指被拒。
    configure_entry0(&mut h, &mut state, BASE, (FETCH_TARGET >> 2) | 0x1ff, 0x98);
    let outcome = h.step_one_at(&mut state, FETCH_TARGET, nop());
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::InstructionAccessFault);
            assert_eq!(cause.mcause(), 1, "instruction access fault 的 mcause 是 1");
        }
        other => panic!("没有 X 权限 ⇒ 取指必须被拒（实际 {:?}）", other),
    }
    assert_eq!(csr_value(&state, 0x343), FETCH_TARGET, "mtval = 取指地址");
}

/// ⚠️ **控制组**：给上 `X` ⇒ ⇒ 同样的取指**放行** ⇒ ⇒ 「取指永远被拒」的
/// 实现过不了这一条。
#[test]
fn an_instruction_fetch_is_allowed_with_the_x_permission() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 同一个条目，但给上 `X`（`L` 仍在 ⇒ M 模式也生效）。
    configure_entry0(&mut h, &mut state, BASE, (FETCH_TARGET >> 2) | 0x1ff, 0x9c);
    let outcome = h.step_one_at(&mut state, FETCH_TARGET, nop());
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "给了 X ⇒ 取指必须放行（实际 {:?}）",
        outcome
    );
}

// ---------------------------------------------------------------------------
// v0.39 —— **逐片**判（对参考侧**实测**）
// ---------------------------------------------------------------------------
//
// ⚠️ 下面这些条目是**参考侧实测**用的那两套配置（探针 `b_access.S` 与
// `b_access2.S`）⇒ ⇒ 地址与权限**逐字**照抄 ⇒ ⇒ 判据与参考侧同形。
//
// * `pmpaddr` 存的是**右移两位**的地址（F3）；
// * `pmpcfg` 字节：`0x18` = NAPOT 且**无权限**（DENY），
//   `0x1f` = NAPOT 且 R+W+X（ALLOW）。

/// 探针的**条目 0** —— NAPOT 4 KiB @ `0x80040000`，**无权限**（DENY）。
const PROBE_DENY_ENTRY: (u32, u8) = (0x2001_01ff, 0x18);
/// 探针的**条目 1** —— NAPOT 1 MiB @ `0x80000000`，R+W+X（ALLOW）。
const PROBE_ALLOW_ENTRY: (u32, u8) = (0x2001_ffff, 0x1f);
/// 探针 `b_access2.S` 的**条目 1** —— NAPOT 1 MiB @ `0x80100000`，R+W+X。
const PROBE_ALLOW2_ENTRY: (u32, u8) = (0x2005_ffff, 0x1f);

/// 配置**条目 0 与条目 1**（都是 NAPOT）—— 靠**跑** `csrw` 指令。
///
/// ⚠️ **两个条目必须一起写进 `pmpcfg0`**（RV32 一个寄存器装 4 个条目）⇒ ⇒
/// 一次写就把两个字节都带上 ⇒ ⇒ 免得「写第二条时把第一条抹成 OFF」。
fn configure_two_entries(
    h: &mut Harness,
    state: &mut SimState,
    pc: u32,
    entry0: (u32, u8),
    entry1: (u32, u8),
) {
    state.gpr[1] = entry0.0;
    h.step_one_at(state, pc, csrw(CSR_PMPADDR0, 1));
    state.gpr[1] = entry1.0;
    h.step_one_at(state, pc + 4, csrw(CSR_PMPADDR0 + 1, 1));
    state.gpr[1] = u32::from(entry1.1) << 8 | u32::from(entry0.1);
    h.step_one_at(state, pc + 8, csrw(CSR_PMPCFG0, 1));
}

/// ⚠️ **判别式**：未对齐的 4 字节访问**跨过** 4 字节块边界 ⇒ ⇒ 参考侧
/// **逐片**判 ⇒ ⇒ 「整个访问必须落在同一条目内」那个旧读法**过不了**这一对。
///
/// * 尾部落进 DENY 条目 ⇒ ⇒ 参考侧**拒**（mcause 5）；
/// * 跨两个 ALLOW 条目 ⇒ ⇒ 参考侧**放行**。
///
/// ⚠️ **两条方向相反** ⇒ ⇒ 「一律按起始条目判」「一律按整段判」「一律拒」
/// 这三种写法**都过不了这一对** ⇒ ⇒ 这一对就是**必要对照**。
#[test]
fn a_straddling_access_is_judged_piece_by_piece() {
    // 条目 0 = DENY（NAPOT 4 KiB @ 0x80040000），条目 1 = ALLOW
    // （NAPOT 1 MiB @ 0x80000000）。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        PROBE_DENY_ENTRY,
        PROBE_ALLOW_ENTRY,
    );
    set_mprv(&mut h, &mut state, BASE + 12);
    // 未对齐 4 字节读 @ 0x8003FFFE：低 2 字节在 ALLOW 条目 1 里，高 2
    // 字节落进 DENY 条目 0 ⇒ ⇒ **逐片** ⇒ ⇒ **拒**。
    assert!(
        !pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "尾部落进 DENY 条目 ⇒ 必须被拒（参考侧实测：mcause 5）"
    );

    // 两个 ALLOW 条目 ⇒ ⇒ 未对齐 4 字节读 @ 0x800FFFFE **放行**。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        PROBE_ALLOW_ENTRY,
        PROBE_ALLOW2_ENTRY,
    );
    set_mprv(&mut h, &mut state, BASE + 12);
    assert!(
        pmp_allows(&state, 0x800f_fffe, 4, PmpAccess::Load),
        "跨两个 ALLOW 条目 ⇒ 必须放行（参考侧实测：放行，读到混合值）"
    );
    // 写**同样**放行（参考侧实测：未对齐跨界写**完成**）。
    assert!(
        pmp_allows(&state, 0x800f_fffe, 4, PmpAccess::Store),
        "跨两个 ALLOW 条目 ⇒ 写必须放行（参考侧实测：写完成）"
    );
}

/// ⚠️ **MPRV 路径没有被逐片判削弱**：`MPRV` = 1 ⇒ ⇒ M 模式的访存**受检**
/// ⇒ ⇒ 同一串访问**被拒**；`MPRV` = 0 ⇒ ⇒ **放行**。
///
/// ⚠️ **控制组**：两条**只差 `MPRV`** ⇒ ⇒ 「全部被拒」与「全部放行」这两个
/// 实现**都过不了这一对**。
#[test]
fn the_mprv_path_is_not_weakened_by_the_piece_split() {
    // 条目 0 = DENY（NAPOT 4 KiB @ 0x80040000），条目 1 = ALLOW
    // （NAPOT 1 MiB @ 0x80000000）⇒ ⇒ 尾部落进 DENY 条目的那一次访问。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        PROBE_DENY_ENTRY,
        PROBE_ALLOW_ENTRY,
    );
    set_mprv(&mut h, &mut state, BASE + 12);
    assert!(
        !pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "MPRV = 1 ⇒ M 模式受检 ⇒ 必须被拒"
    );

    // ⚠️ **控制组**：同样的条目，`MPRV` = 0 ⇒ ⇒ M 模式旁路 ⇒ ⇒ 放行。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        PROBE_DENY_ENTRY,
        PROBE_ALLOW_ENTRY,
    );
    assert!(
        pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "MPRV = 0 ⇒ M 模式旁路 ⇒ 必须放行"
    );
}

/// ⚠️ **锁住的条目逐片强制**（F12）—— M 模式也不例外。
///
/// ⚠️ **控制组**：同一个条目**不锁**时，M 模式**放行** ⇒ ⇒ 「锁没生效」与
/// 「一律被拒」这两个实现**都过不了这一对**。
#[test]
fn a_locked_entry_constrains_a_straddling_access_in_m_mode() {
    // 条目 0 = **锁住**的 DENY（NAPOT 4 KiB @ 0x80040000），条目 1 = ALLOW。
    let locked_entry = (PROBE_DENY_ENTRY.0, PROBE_DENY_ENTRY.1 | 0x80);
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(&mut h, &mut state, BASE, locked_entry, PROBE_ALLOW_ENTRY);
    // ⚠️ **不**设 `MPRV` ⇒ ⇒ 这一条走的是「M 模式旁路」那条路 ⇒ ⇒ 锁住的
    // 条目**照样**强制 ⇒ ⇒ 拒。
    assert!(
        !pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "锁住的条目 ⇒ M 模式也必须被拒"
    );

    // ⚠️ **控制组**：同一个条目**不锁** ⇒ ⇒ M 模式旁路 ⇒ ⇒ 放行。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        PROBE_DENY_ENTRY,
        PROBE_ALLOW_ENTRY,
    );
    assert!(
        pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "没锁的条目 ⇒ M 模式旁路 ⇒ 必须放行"
    );
}

/// ⚠️ **端到端**：未对齐的 4 字节读**跨过**两个 ALLOW 条目的边界 ⇒ ⇒ 参考侧
/// **执行**它并读回**混合值** `0xBBBB_AAAA` ⇒ ⇒ 平台现在**读回同一个值**。
///
/// ⚠️ **控制组**：**对齐**的读（同一块 RAM、同一套条目）⇒ ⇒ 放行并读回
/// `0xAAAAAAAA` ⇒ ⇒ 「RAM 没准备好」这种失败**不会**伪装成通过。
#[test]
fn a_straddling_load_across_two_allowed_entries_executes() {
    // 条目 0 = ALLOW（NAPOT 32 KiB @ 0x00000000），条目 1 = ALLOW
    // （NAPOT 32 KiB @ 0x00008000）⇒ ⇒ 两个条目**都**允许。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(&mut h, &mut state, BASE, (0xfff, 0x1f), (0x2fff, 0x1f));
    set_mprv(&mut h, &mut state, BASE + 12);
    // RAM：边界**两侧**各放一个字 ⇒ ⇒ 未对齐读会读到**混合值**。
    h.load(0x7ffc, &[0xaaaa_aaaa]);
    h.load(0x8000, &[0xbbbb_bbbb]);

    // 未对齐 4 字节读 @ 0x7FFE ⇒ ⇒ 跨过边界 ⇒ ⇒ 两片**各自**被允许 ⇒ ⇒
    // **执行** ⇒ ⇒ 读回混合值。
    state.gpr[2] = 0x7ffe;
    let outcome = h.step_one_at(&mut state, BASE + 16, lw(3, 2));
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "跨两个 ALLOW 条目 ⇒ 未对齐读必须执行（实际 {:?}）",
        outcome
    );
    assert_eq!(state.gpr[3], 0xbbbb_aaaa, "参考侧实测读回的就是这个混合值");

    // ⚠️ **控制组**：**对齐**的读（同一个 RAM、同一套条目）⇒ ⇒ 放行。
    state.gpr[2] = 0x7ffc;
    let outcome = h.step_one_at(&mut state, BASE + 20, lw(3, 2));
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "对齐读必须放行（实际 {:?}）",
        outcome
    );
    assert_eq!(state.gpr[3], 0xaaaa_aaaa, "对齐读读回的是它自己那个字");
}

/// ⚠️ **端到端**：未对齐的 4 字节**写**跨过两个 ALLOW 条目的边界 ⇒ ⇒ 参考侧
/// **写完成** ⇒ ⇒ 平台的**四个字节都落地**。
///
/// ⚠️ 平台的旧规则会把这一次写**整个拒掉**（连低 2 字节都不落地）⇒ ⇒
/// 这一条正是那个差异的**端到端**证据。
#[test]
fn a_straddling_store_across_two_allowed_entries_lands() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(&mut h, &mut state, BASE, (0xfff, 0x1f), (0x2fff, 0x1f));
    set_mprv(&mut h, &mut state, BASE + 12);

    state.gpr[1] = 0x1122_3344;
    state.gpr[2] = 0x7ffe;
    let outcome = h.step_one_at(&mut state, BASE + 16, sw(1, 2));
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "跨两个 ALLOW 条目 ⇒ 未对齐写必须完成（实际 {:?}）",
        outcome
    );
    // 四个字节**都**落地了 ⇒ ⇒ 读回整个字。
    let value = h
        .ram
        .borrow()
        .read(0x7ffe, WriteSize::Word)
        .expect("read back");
    assert_eq!(value, 0x1122_3344, "未对齐跨界写的四个字节都要落地");
}

/// ⚠️ **端到端**：未对齐的 4 字节读**尾部**落进 DENY 条目 ⇒ ⇒ 参考侧**拒**
/// （mcause 5）⇒ ⇒ 平台也**拒** ⇒ ⇒ **架构陷阱**（不是宿主错误）。
#[test]
fn a_straddling_load_into_a_deny_entry_traps() {
    // 条目 0 = DENY（NAPOT 4 KiB @ 0x00008000），条目 1 = ALLOW
    // （NAPOT 64 KiB @ 0x00000000）⇒ ⇒ 尾部两字节落进 DENY 条目。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(&mut h, &mut state, BASE, (0x21ff, 0x18), (0x1fff, 0x1f));
    set_mprv(&mut h, &mut state, BASE + 12);
    h.load(0x7ffc, &[0xaaaa_aaaa]);
    h.load(0x8000, &[0xbbbb_bbbb]);

    state.gpr[2] = 0x7ffe;
    let outcome = h.step_one_at(&mut state, BASE + 16, lw(3, 2));
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::LoadAccessFault);
            assert_eq!(cause.mcause(), 5, "load access fault 的 mcause 是 5");
        }
        other => panic!("尾部落进 DENY 条目 ⇒ 必须被拒（实际 {:?}）", other),
    }
    assert_eq!(csr_value(&state, 0x343), 0x7ffe, "mtval 必须是出错地址");

    // ⚠️ **控制组**：**对齐**的读（同一块 RAM、同一套条目）⇒ ⇒ 放行。
    state.gpr[2] = 0x7ffc;
    let outcome = h.step_one_at(&mut state, BASE + 20, lw(3, 2));
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "对齐读必须放行（实际 {:?}）",
        outcome
    );
    assert_eq!(state.gpr[3], 0xaaaa_aaaa, "对齐读读回的是它自己那个字");
}
