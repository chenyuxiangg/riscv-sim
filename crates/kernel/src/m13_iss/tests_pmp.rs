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
//!
//! ## v0.40：PMP 的**特权口径**（D-153）
//!
//! ⚠️ 有效特权级 = `MPRV` ? `MPP` : 当前特权级（**取指永远用当前特权级**）⇒ ⇒
//! 旧口径塌缩成「只看 `MPRV`」⇒ ⇒ 两个方向都错 ⇒ ⇒ 这一节把**四格**钉住。
//!
//! ⚠️ **四格各自的判别力**（**变异实测**：把内核改回旧的「只看 `MPRV`」口径
//! 之后跑这一节）：
//!
//! * `(a)` M 模式 + `MPRV` = 0 ⇒ ⇒ 旧实现**也**放行 ⇒ ⇒ **零判别力**
//!   ⇒ ⇒ 它是 `(b)` 的**控制组**；
//! * `(b)` M 模式 + `MPRV` = 1 且 `MPP` = 3 ⇒ ⇒ 旧实现**拒** / 新实现**放行**
//!   ⇒ ⇒ **有判别力**（旧口径「过严」的那一格）；
//! * `(c)` M 模式 + `MPRV` = 1 且 `MPP` = 0 ⇒ ⇒ 旧实现**也**拒 ⇒ ⇒ 对旧口径
//!   **零判别力** ⇒ ⇒ 但它**钉住**「`MPRV` = 1 ⇒ ⇒ 不等于 M」这条 ⇒ ⇒ 它是
//!   `(b)` 的**控制组**；
//! * `(d)` **S 模式** + `MPRV` = 0 ⇒ ⇒ 旧实现**放行** / 新实现**拒** ⇒ ⇒ **有
//!   判别力** ⇒ ⇒ ⚠️ 而且是**安全方向**的那一条（旧实现 ⇒ ⇒ S 模式的代码能写
//!   它不该写的内存）。

use riscv_sim_contract::csr::{
    PmpAccess, CSR_MSTATUS, CSR_PMPADDR0, CSR_PMPCFG0, MSTATUS_MPP_M, MSTATUS_MPP_SHIFT,
    MSTATUS_MPP_U, MSTATUS_MPRV_BIT, PRIV_S,
};
use riscv_sim_contract::model::{SimState, WriteSize};

use super::csr;
use super::executor::{StepOutcome, TrapCause};
use super::pmp::{pmp_allows, pmp_fault_addr};
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

/// 把 `mstatus` 写成「`MPP` = `mpp`，**`MPRV` = 0**」。
///
/// ⚠️ **为什么用 `csrw` 而不是 `csrrs`**：`csrrs` 只会**置位** ⇒ ⇒ 摆不出
/// 「`MPP` = 0」那一种 ⇒ ⇒ 这一节要**显式**摆 `MPP` ⇒ ⇒ 写整个寄存器。
fn set_mpp(h: &mut Harness, state: &mut SimState, pc: u32, mpp: u32) {
    state.gpr[1] = mpp << MSTATUS_MPP_SHIFT;
    h.step_one_at(state, pc, csrw(CSR_MSTATUS, 1));
}

/// 把 `mstatus` 写成「`MPRV` = 1 **且** `MPP` = `mpp`」。
///
/// ⚠️ **为什么用 `csrw` 而不是 `csrrs`**：`csrrs` 只会**置位** ⇒ ⇒ 摆不出
/// 「`MPP` = 0」那一种 ⇒ ⇒ 这一节要**显式**摆 `MPP` ⇒ ⇒ 写整个寄存器。
fn set_mprv_and_mpp(h: &mut Harness, state: &mut SimState, pc: u32, mpp: u32) {
    state.gpr[1] = (1u32 << MSTATUS_MPRV_BIT) | (mpp << MSTATUS_MPP_SHIFT);
    h.step_one_at(state, pc, csrw(CSR_MSTATUS, 1));
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
    // ⚠️ **`mtval` 报的是「失败那一片的起始地址」**（台账 D-154 · 参考侧实测）
    // ⇒ ⇒ 这一次的访问是 [0x7ffe, 0x8002) ⇒ ⇒ 片 1 = [0x7ffe, 0x8000)（被条目 1
    // 允许）⇒ ⇒ 片 2 = [0x8000, 0x8002)（落进 DENY 条目）⇒ ⇒ 失败片的起始是
    // **0x8000**，**不是**访问的有效地址 **0x7ffe**。
    assert_eq!(
        csr_value(&state, 0x343),
        0x8000,
        "mtval 必须是**失败片的起始**（参考侧口径），不是访问的有效地址"
    );

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

// ---------------------------------------------------------------------------
// v0.40 —— **有效特权级**（D-153 · PMP 的特权口径）
// ---------------------------------------------------------------------------
//
// ⚠️ 这一节的四格**互相只差一个变量** ⇒ ⇒ 它们合起来就是**必要对照**：
//
// | 格 | 当前特权级 | `MPRV` | `MPP` | 有效特权级 | 判决 |
// | --- | --- | --- | --- | --- | --- |
// | (a) | M | 0 | 任意 | **M** | **放行** |
// | (b) | M | 1 | M | **M** | **放行** |
// | (c) | M | 1 | U | **U** | **拒** |
// | (d) | **S** | 0 | 任意 | **S** | **拒** |
//
// ⚠️ **四格各自的判别力**（**变异实测**：把内核改回旧的「只看 `MPRV`」口径之后
// 跑这一节 ⇒ ⇒ 红的正好是 `(b)` 与 `(d)` 这两格 + 「S 模式取指」那一条）：
//
// * `(a)` 旧实现**也**放行 ⇒ ⇒ 零判别力 ⇒ ⇒ 它是 `(b)` 的**控制组**；
// * `(b)` 旧实现**拒** / 新实现**放行** ⇒ ⇒ 有判别力（旧口径「过严」那格）；
// * `(c)` 旧实现**也**拒 ⇒ ⇒ 对旧口径零判别力 ⇒ ⇒ 它钉住「`MPRV` = 1 ⇒ ⇒
//   **不等于** M」⇒ ⇒ 它是 `(b)` 的**控制组**；
// * `(d)` 旧实现**放行** / 新实现**拒** ⇒ ⇒ 有判别力 ⇒ ⇒ ⚠️ 而且是**安全
//   方向**的那一条（旧实现 ⇒ ⇒ S 模式的代码能写它不该写的内存）。

/// **(a)** —— **M 模式 + `MPRV` = 0** ⇒ ⇒ 有效特权级是 **M** ⇒ ⇒ 豁免 ⇒ ⇒
/// **放行**。
///
/// ⚠️ **控制组**：`(b)` 与它**只差 `MPRV`** ⇒ ⇒ 「M 模式一律被拒」的实现过不了
/// 这一对。
#[test]
fn an_m_mode_access_is_exempt_when_mprv_is_clear_whatever_mpp_says() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NA4 @ 0x200，只有 `R`（没有 `W`）⇒ ⇒ 若被检查，写必被拒。
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    // `MPRV` = 0 ⇒ ⇒ `MPP` 摆成什么都**不**影响有效特权级 ⇒ ⇒ 这里摆成 M。
    set_mpp(&mut h, &mut state, BASE + 8, MSTATUS_MPP_M);
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "MPRV = 0 ⇒ ⇒ 有效特权级 = 当前特权级 = M ⇒ ⇒ 豁免（实际 {:?}）",
        outcome
    );
}

/// **(b)** —— **M 模式 + `MPRV` = 1 + `MPP` = 3（M）** ⇒ ⇒ 有效特权级是
/// **M** ⇒ ⇒ 豁免 ⇒ ⇒ **放行**。
///
/// ⚠️ **这条是旧实现错的那一格**（旧口径只看 `MPRV` ⇒ ⇒ `MPRV` = 1 ⇒ ⇒ 一律
/// 受检 ⇒ ⇒ 拒）⇒ ⇒ 它与 `(a)` 构成**必要对照**（只差 `MPRV`）⇒ ⇒ 也能钉住
/// 「`MPRV` = 1 就一定被拒」那个写法。
#[test]
fn an_m_mode_access_is_exempt_when_mprv_is_set_and_mpp_is_machine() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    set_mprv_and_mpp(&mut h, &mut state, BASE + 8, MSTATUS_MPP_M);
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    assert!(
        matches!(outcome, StepOutcome::Advanced),
        "MPRV = 1 且 MPP = 3 ⇒ ⇒ 有效特权级 = M ⇒ ⇒ 豁免（实际 {:?}）",
        outcome
    );
}

/// **(c)** —— **M 模式 + `MPRV` = 1 + `MPP` = 0（U）** ⇒ ⇒ 有效特权级是
/// **U** ⇒ ⇒ **施加 PMP** ⇒ ⇒ **拒**。
///
/// ⚠️ 它与 `(b)` **只差 `MPP`** ⇒ ⇒ 「`MPRV` = 1 ⇒ ⇒ 一律 M 模式豁免」与
/// 「`MPRV` = 1 ⇒ ⇒ 一律受检」这两个写法**都过不了**这一对。
#[test]
fn an_m_mode_access_is_checked_when_mprv_is_set_and_mpp_is_user() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, DATA >> 2, 0x11);
    set_mprv_and_mpp(&mut h, &mut state, BASE + 8, MSTATUS_MPP_U);
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!(
            "MPRV = 1 且 MPP = 0 ⇒ ⇒ 有效特权级 = U ⇒ ⇒ 必须被拒（实际 {:?}）",
            other
        ),
    }
    assert_eq!(csr_value(&state, 0x343), DATA, "mtval 必须是出错地址");
}

/// **(d)** —— **S 模式 + `MPRV` = 0** ⇒ ⇒ 有效特权级是 **S** ⇒ ⇒ **施加
/// PMP** ⇒ ⇒ **拒**。
///
/// ⚠️ **这一格是旧实现错得最厉害的那一格**：旧口径只看 `MPRV` ⇒ ⇒ `MPRV` = 0
/// ⇒ ⇒ 豁免 ⇒ ⇒ **放行** ⇒ ⇒ ⚠️ **安全方向的漏洞**（S 模式的代码能写它不该写
/// 的内存）⇒ ⇒ 它与 `(a)` **只差当前特权级** ⇒ ⇒ 构成**必要对照**。
///
/// ⚠️ **取指也要能被允许**：S 模式下**执行**这条指令 ⇒ ⇒ 取指落在条目 0 里
/// ⇒ ⇒ 条目 0 必须给 `X` ⇒ ⇒ 否则先撞上的是「取指被拒」而不是「写被拒」。
#[test]
fn a_supervisor_mode_access_is_checked_even_when_mprv_is_clear() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NAPOT 4 KiB @ 0x1000（代码那块），带 `X` ⇒ ⇒ S 模式的取指被允许。
    // 条目 1 = NA4 @ 0x200（DATA），只有 `R`（没有 `W`）⇒ ⇒ S 模式的写被拒。
    configure_entry0(&mut h, &mut state, BASE, (BASE >> 2) | 0x1ff, 0x1d);
    configure_entry1(&mut h, &mut state, BASE + 4, DATA >> 2, 0x1d, 0x11);
    // 切到 S 模式（`MPRV` = 0 ⇒ ⇒ 有效特权级 = 当前特权级 = S）。
    csr::priv_set(&mut state, PRIV_S);
    let outcome = store_to_data(&mut h, &mut state, BASE + 8);
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!("S 模式 ⇒ ⇒ PMP 必须被施加 ⇒ ⇒ 写被拒（实际 {:?}）", other),
    }
    assert_eq!(csr_value(&state, 0x343), DATA, "mtval 必须是出错地址");
}

/// ⚠️ **安全方向的护栏**（端到端）：**S 模式** + `MPRV` = 1 + `MPP` = 3（M）
/// ⇒ ⇒ 有效特权级**必须**是 **S** ⇒ ⇒ PMP **必须**被施加 ⇒ ⇒ 写被拒。
///
/// ⚠️ **为什么这一格是必须的**：那个状态**guest 代码走不到**（进 S 模式要求
/// `MPP` = S，而 `mret` 不改 `MPP`）⇒ ⇒ **但是它能从「快照恢复」到达**
/// （`mstatus` 与伪 CSR `CSR_PRIV` **都在快照里**）⇒ ⇒ 若有效特权级被抬到 M
/// ⇒ ⇒ 豁免 PMP ⇒ ⇒ ⚠️ **安全方向的洞** ⇒ ⇒ `MPRV` 必须**带门**。
///
/// ⚠️ **判别力**：对「`MPRV` 不带门」那个写法**恒红** ⇒ ⇒ 「门开反了」也红。
#[test]
fn mprv_does_not_lift_a_supervisor_mode_access_to_machine_level() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NAPOT 4 KiB @ 0x1000（代码那块），带 `X` ⇒ ⇒ S 模式的取指被允许。
    // 条目 1 = NA4 @ 0x200（DATA），只有 `R`（没有 `W`）⇒ ⇒ S 模式的写被拒。
    configure_entry0(&mut h, &mut state, BASE, (BASE >> 2) | 0x1ff, 0x1d);
    configure_entry1(&mut h, &mut state, BASE + 4, DATA >> 2, 0x1d, 0x11);
    // `MPRV` = 1 且 `MPP` = 3（M）⇒ ⇒ **M 模式**下有效特权级是 M ⇒ ⇒ 豁免。
    set_mprv_and_mpp(&mut h, &mut state, BASE + 8, MSTATUS_MPP_M);
    // 切到 S 模式 ⇒ ⇒ `MPRV` **不再生效**（门）⇒ ⇒ 有效特权级**恒**是 S。
    csr::priv_set(&mut state, PRIV_S);
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!(
            "S 模式 ⇒ ⇒ MPRV 不生效 ⇒ ⇒ 有效特权级 = S ⇒ ⇒ 必须被拒（实际 {:?}）",
            other
        ),
    }
    assert_eq!(csr_value(&state, 0x343), DATA, "mtval 必须是出错地址");
}

/// ⚠️ **取指在 S 模式下不被豁免** —— `MPRV` **不**影响取指（F11）⇒ ⇒ 取指的
/// 有效特权级 = **当前特权级** ⇒ ⇒ S 模式的取指**要**过 PMP ⇒ ⇒ 落在没有 `X`
/// 权限的条目里 ⇒ ⇒ 拒。
///
/// ⚠️ **控制组**：同一个条目，**M 模式** ⇒ ⇒ 豁免 ⇒ ⇒ 放行 ⇒ ⇒ 「取指永远被
/// 拒」的实现过不了这一对。
#[test]
fn an_instruction_fetch_is_not_exempt_in_supervisor_mode() {
    // 条目 0 = NAPOT 4 KiB @ 0x2000，**没有 `X`** ⇒ ⇒ S 模式的取指被拒。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, (FETCH_TARGET >> 2) | 0x1ff, 0x19);
    csr::priv_set(&mut state, PRIV_S);
    assert!(
        !pmp_allows(&state, FETCH_TARGET, 4, PmpAccess::Fetch),
        "S 模式 ⇒ ⇒ 取指**不**豁免 ⇒ ⇒ 没有 X 权限 ⇒ ⇒ 必须被拒"
    );

    // ⚠️ **控制组**：同一个条目，**M 模式** ⇒ ⇒ 豁免 ⇒ ⇒ 放行。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, (FETCH_TARGET >> 2) | 0x1ff, 0x19);
    assert!(
        pmp_allows(&state, FETCH_TARGET, 4, PmpAccess::Fetch),
        "M 模式 ⇒ ⇒ 取指豁免 ⇒ ⇒ 必须放行"
    );
}

/// ⚠️ **M 模式下的取指不吃 `MPRV`** —— 取指的有效特权级 = **当前特权级**
/// （F11）⇒ ⇒ `MPRV` = 1 且 `MPP` = 0（U）时 ⇒ ⇒ **取指**仍然**放行**
/// （当前特权级是 M ⇒ ⇒ 豁免）⇒ ⇒ 而**同一状态**下的**数据访存**被**拒**
/// （有效特权级 = U ⇒ ⇒ 施加 PMP）⇒ ⇒ 这两条断言**成对**，缺一不可：
///
/// * 只钉「取指放行」⇒ ⇒ 「`MPRV` 一律被忽略」的那个实现也能过；
/// * 只钉「数据访存被拒」⇒ ⇒ 「`MPRV` 一律生效（包括取指）」的那个实现
///   也能过。
///
/// ⚠️ **取指为什么落在条目 0 上**：条目 0 = NAPOT 4 KiB @ `FETCH_TARGET`，
/// **只有 `R`**（既没有 `W`，也没有 `X`）⇒ ⇒ 若这条取指**要**过 PMP ⇒ ⇒
/// 它必被拒 ⇒ ⇒ 「放行」这一条断言**只可能**来自「M 模式豁免」。
///
/// ⚠️ **数据访存为什么落在「没有条目命中」那一支**：`DATA` **不在**条目 0
/// 的区间里 ⇒ ⇒ 这一条走的是「没有任何条目命中」那一支 ⇒ ⇒ 有效特权级是
/// U 时**必拒**、是 M 时**必放行** ⇒ ⇒ 「被拒」这一条断言**只可能**来自
/// 「`MPRV` 把数据访存的有效特权级换成了 `MPP`」。
#[test]
fn an_instruction_fetch_in_machine_mode_is_not_affected_by_mprv() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_entry0(&mut h, &mut state, BASE, (FETCH_TARGET >> 2) | 0x1ff, 0x19);
    // M 模式 + `MPRV` = 1 + `MPP` = 0（U）⇒ ⇒ **数据访存**的有效特权级是 U。
    set_mprv_and_mpp(&mut h, &mut state, BASE + 8, MSTATUS_MPP_U);

    // ---- 取指：有效特权级 = 当前特权级（M）⇒ ⇒ 豁免 ⇒ ⇒ 放行 ----------
    assert!(
        pmp_allows(&state, FETCH_TARGET, 4, PmpAccess::Fetch),
        "取指的有效特权级 = 当前特权级（M）⇒ ⇒ MPRV 不影响取指 ⇒ ⇒ 必须放行"
    );

    // ---- 同一状态下的数据访存：有效特权级 = U ⇒ ⇒ 施加 PMP ⇒ ⇒ 拒 ----
    let outcome = store_to_data(&mut h, &mut state, BASE + 12);
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!(
            "MPRV = 1 且 MPP = 0 ⇒ ⇒ 数据访存的有效特权级 = U ⇒ ⇒ 必须被拒（实际 {:?}）",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// v0.41 —— **失败片的起始地址**（D-154 · PMP 拒绝时上报的 `mtval`）
// ---------------------------------------------------------------------------
//
// ⚠️ **参考侧的口径**（实测，STAGE-26 §1.2 F1 + 本节 §「探针」）：
//
// * 未对齐 4 字节读 @ `0x8003_FFFE`（尾部两字节落进 DENY 区间）⇒ ⇒ 参考侧
//   `mtval` = **`0x8004_0000`** —— **出错那一片的起始**，**不是**访问的有效
//   地址（`0x8003_FFFE`）⇒ ⇒ 差**一个偏移量**；
// * 两侧的 `mcause` **都是 5** ⇒ ⇒ 只看 `mcause` **分不出**这一条；
// * ⚠️ **「片」的口径**（本批用探针补测）：参考侧报的是**片与 4 字节块的交集
//   的起点**，**不是**那个 4 字节块自己的起点 ⇒ ⇒ 访问 @ `0x8003_FFFE` 且
//   **第一片**被拒时，参考侧报 `0x8003_FFFE`（**不是** `0x8003_FFFC`）。
//
// ⚠️ **判别力在哪一条**：**第一片**失败时，「片的起始」**恒等于**访问的有效
// 地址 ⇒ ⇒ 那一条**单独**分不出两种口径 ⇒ ⇒ **判别力全在「第二片」那一条**
// ⇒ ⇒ 下面第 ① 条**必须**钉住它。
//
// ⚠️ **一个约束一个落点**（STAGE-26 裁决 B / C）：失败片地址**由判定本身产出**
// ⇒ ⇒ `pmp_fault_addr` 是**唯一的**判定入口 ⇒ ⇒ `pmp_allows` **由它实现**
// ⇒ ⇒ 「允不允许」与「报哪一个地址」**不可能漂移**。

/// ① —— **失败片是第二片** ⇒ ⇒ 报的**就是那一片的起始**（`0x8004_0000`），
/// **不是**访问的有效地址（`0x8003_FFFE`）。
///
/// ⚠️ **这一条是整批的判别力所在**：参考侧实测 `mtval` = `0x8004_0000`
/// ⇒ ⇒ 「报有效地址」那个写法**过不了**它。
#[test]
fn the_failing_piece_of_a_straddling_load_is_the_second_piece_start() {
    // 条目 0 = DENY（NAPOT 4 KiB @ 0x80040000），条目 1 = ALLOW
    // （NAPOT 1 MiB @ 0x80000000）—— 与语料 `local/pmp_straddle` 的配置 A 同形。
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
    // 未对齐 4 字节读 @ 0x8003FFFE ⇒ ⇒ 片 1 = [0x8003FFFE, 0x80040000)（ALLOW）
    // ⇒ ⇒ 片 2 = [0x80040000, 0x80040002)（DENY）⇒ ⇒ **失败片是第二片**。
    let fault = pmp_fault_addr(&state, 0x8003_fffe, 4, PmpAccess::Load);
    assert_eq!(
        fault,
        Some(0x8004_0000),
        "失败片的起始 = 0x80040000（参考侧实测 F1）"
    );
    assert_ne!(
        fault,
        Some(0x8003_fffe),
        "⚠️ 失败片的起始**不是**访问的有效地址（D-154 的那一个偏移量）"
    );
    // ⚠️ **同一条链**（裁决 B）：`pmp_allows` **由** `pmp_fault_addr` **实现**
    // ⇒ ⇒ 「允不允许」与「报哪一个地址」**不可能漂移**。
    assert!(
        !pmp_allows(&state, 0x8003_fffe, 4, PmpAccess::Load),
        "同一个访问必须**同时**被判为「被拒」"
    );
}

/// ② —— **失败片是第一片** ⇒ ⇒ 报的**也是那一片的起始**。
///
/// ⚠️ **配置**：条目 0 = NA4 @ `0x8003_FFFC`（**无权限** ⇒ DENY）⇒ ⇒ 访问的
/// **第一片**（`[0x8003FFFE, 0x80040000)`）就落在它里面 ⇒ ⇒ 失败的是**第一片**。
///
/// ⚠️ **本条**单独**分不出**「片的起始」与「有效地址」两种口径（第一片失败时
/// 两者**恒等**）⇒ ⇒ 判别力在 ① ⇒ ⇒ 但它**钉住**「失败片的起始**不是**那个
/// 4 字节块自己的起点」（`0x8003_FFFC`）⇒ ⇒ 参考侧实测报的是 `0x8003_FFFE`。
#[test]
fn the_failing_piece_of_an_access_whose_first_piece_fails_is_that_piece_start() {
    // 条目 0 = NA4 @ 0x8003FFFC（`pmpaddr` = 0x8003FFFC >> 2 = 0x2000FFFF，
    // `A` = NA4 = 0b10 ⇒ `pmpcfg` 字节 0 = 0x10），**无权限** ⇒ DENY。
    let mut h = Harness::new();
    let mut state = SimState::default();
    configure_two_entries(
        &mut h,
        &mut state,
        BASE,
        (0x2000_ffff, 0x10),
        PROBE_ALLOW_ENTRY,
    );
    set_mprv(&mut h, &mut state, BASE + 12);
    let fault = pmp_fault_addr(&state, 0x8003_fffe, 4, PmpAccess::Load);
    assert_eq!(
        fault,
        Some(0x8003_fffe),
        "失败片是第一片 ⇒ ⇒ 报的是**那一片的起始**（不是块起点 0x8003FFFC）"
    );
    assert_ne!(
        fault,
        Some(0x8003_fffc),
        "⚠️ 失败片的起始**不是**那个 4 字节块自己的起点（参考侧实测）"
    );
}

/// ③ —— 整个访问落在**允许**的条目里 ⇒ ⇒ **没有失败片** ⇒ ⇒ `None`（不陷）。
///
/// ⚠️ **控制组**：这一条**必须**在 ⇒ ⇒ 「一律报一个地址」的实现过不了它。
#[test]
fn an_access_entirely_inside_an_allowed_entry_has_no_failing_piece() {
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
    // 对齐的读，完整落在条目 0（ALLOW）里 ⇒ ⇒ 没有失败片。
    assert_eq!(
        pmp_fault_addr(&state, 0x8000_2000, 4, PmpAccess::Load),
        None,
        "完整落在允许条目里的访问 ⇒ ⇒ 没有失败片"
    );
    // 跨**两个** ALLOW 条目的未对齐读 ⇒ ⇒ 两片**各自**被允许 ⇒ ⇒ 也没有失败片。
    assert_eq!(
        pmp_fault_addr(&state, 0x800f_fffe, 4, PmpAccess::Load),
        None,
        "跨两个 ALLOW 条目的读 ⇒ ⇒ 没有失败片"
    );
    assert_eq!(
        pmp_fault_addr(&state, 0x800f_fffe, 4, PmpAccess::Store),
        None,
        "跨两个 ALLOW 条目的写 ⇒ ⇒ 没有失败片"
    );
}

/// ④ —— 「**没有条目命中**」那一支在 **S 模式**下 ⇒ ⇒ 失败 ⇒ ⇒ `Some`（**不是**
/// `None`）⇒ ⇒ **兜底也是逐片的**。
///
/// ⚠️ **配置**：条目 0 = NAPOT 4 KiB @ `BASE`（带 `X` ⇒ ⇒ S 模式的取指被允许）
/// ⇒ ⇒ 条目 1 = NA4 @ `0x8000`（只有 `R`）⇒ ⇒ 访问的**第一片**
/// `[0x8002, 0x8004)` 被条目 1 允许，**第二片** `[0x8004, 0x8006)`
/// **一条条目都不命中** ⇒ ⇒ 走「没有条目命中」那一支 ⇒ ⇒ S 模式下**必拒**。
///
/// ⚠️ **这一条钉住**：「兜底」**不是**按整次访问判的 ⇒ ⇒ **逐片**。
#[test]
fn the_no_entry_matched_fallback_is_also_per_piece() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 条目 0 = NAPOT 4 KiB @ BASE，带 `X` ⇒ ⇒ S 模式的取指被允许。
    configure_entry0(&mut h, &mut state, BASE, (BASE >> 2) | 0x1ff, 0x1d);
    // 条目 1 = NA4 @ 0x8000（`pmpaddr` = 0x8000 >> 2 = 0x2000），只有 `R`。
    configure_entry1(&mut h, &mut state, BASE + 4, 0x2000, 0x1d, 0x11);
    // 切到 S 模式 ⇒ ⇒ 有效特权级 = S ⇒ ⇒ 「没有条目命中」的那一支**必拒**。
    csr::priv_set(&mut state, PRIV_S);
    let fault = pmp_fault_addr(&state, 0x8002, 4, PmpAccess::Load);
    assert_eq!(
        fault,
        Some(0x8004),
        "第二片**一条条目都不命中** ⇒ ⇒ S 模式下**必拒** ⇒ ⇒ 兜底也是**逐片**的"
    );
    // ⚠️ **控制组**：**对齐**的读（同一套条目）⇒ ⇒ 完整落在条目 1 里 ⇒ ⇒ 放行。
    assert_eq!(
        pmp_fault_addr(&state, 0x8000, 4, PmpAccess::Load),
        None,
        "对齐的读完整落在允许条目里 ⇒ ⇒ 没有失败片"
    );
}

/// ⑤ —— **端到端**：被 PMP 拒的**读**上报的 `mtval` 是**失败片的起始**。
///
/// ⚠️ 这是**内核**那条链的端到端证据（裁决 B）：`executor.rs` 的 load 臂把
/// **失败片地址**交给 `enter_trap` ⇒ ⇒ `mtval` = `0x8004_0000` ⇒ ⇒ **不是**
/// 访问的有效地址 `0x8003_FFFE`。
#[test]
fn a_pmp_load_trap_reports_the_failing_piece_start_not_the_effective_address() {
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
    state.gpr[2] = 0x8003_fffe;
    let outcome = h.step_one_at(&mut state, BASE + 16, lw(3, 2));
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::LoadAccessFault);
            assert_eq!(cause.mcause(), 5, "load access fault 的 mcause 是 5");
        }
        other => panic!("尾部落进 DENY 条目 ⇒ 必须被拒（实际 {:?}）", other),
    }
    assert_eq!(
        csr_value(&state, 0x343),
        0x8004_0000,
        "mtval 必须是**失败片的起始**（参考侧实测），不是访问的有效地址"
    );
}

/// ⑥ —— **端到端**：被 PMP 拒的**写**上报的 `mtval` **也是**失败片的起始。
///
/// ⚠️ 它与 ⑤ **成对**（读与写两条臂）⇒ ⇒ 「只改了读」的实现过不了它。
#[test]
fn a_pmp_store_trap_reports_the_failing_piece_start_not_the_effective_address() {
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
    state.gpr[1] = 0x1122_3344;
    state.gpr[2] = 0x8003_fffe;
    let outcome = h.step_one_at(&mut state, BASE + 16, sw(1, 2));
    match outcome {
        StepOutcome::Trapped { cause } => {
            assert_eq!(cause, TrapCause::StoreAccessFault);
            assert_eq!(cause.mcause(), 7, "store access fault 的 mcause 是 7");
        }
        other => panic!("尾部落进 DENY 条目 ⇒ 必须被拒（实际 {:?}）", other),
    }
    assert_eq!(
        csr_value(&state, 0x343),
        0x8004_0000,
        "mtval 必须是**失败片的起始**（参考侧实测），不是访问的有效地址"
    );
}
