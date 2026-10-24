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

use riscv_sim_contract::csr::{CSR_MSTATUS, CSR_PMPADDR0, CSR_PMPCFG0, MSTATUS_MPRV_BIT};
use riscv_sim_contract::model::SimState;

use super::executor::{StepOutcome, TrapCause};
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
