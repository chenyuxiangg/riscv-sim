//! M-13 的 Zicntr 硬件计数器 —— `mcycle` / `minstret` 与它们的影子。
//!
//! 冻结规格：`docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md`
//! §3.1~§3.6（内核设计）、§5.2（B 组验收 B-1 ~ B-10）与 §1.2（参考侧
//! 十四条**实测**事实 R1 ~ R14）。
//!
//! ## 这些用例在看什么
//!
//! * **CSR 文件的内容**（[`csr_value`] 直接读文件，**不经**实现的读路径
//!   —— 用实现的读路径查实现的写路径是**自指**的，两边一起错也会绿），
//! * **通用寄存器的值**（`csrr` 读出来的数 ⇒ ⇒ 走的是**真的读路径**），
//! * **`StepOutcome`** 的形状（`Advanced` / `Trapped`）。
//!
//! ## 读数怎么算
//!
//! ⚠️ **每条退休指令给两个计数器各 +1** ⇒ ⇒ **CSR 文件里
//! [`csr_value`] 读到的 `minstret` 就是「已退休指令数」** ⇒ ⇒ 断言写成
//! 「跑完 k 条之后文件里是 k」比「读出来是 k」**少一层歧义**（读指令
//! **自己**也要退休 ⇒ ⇒ 它读到的是**它自己退休之前**的值）。
//!
//! ## 判别力
//!
//! ⚠️ 每条用例都必须**能红**。凡是「一个断言在典型输入下有等价写法」的
//! 地方，本文件都**配了一条让两者不等价的输入**：
//!
//! | 容易写成「恒真」的断言 | 本文件配的判别式 |
//! | --- | --- |
//! | 「N 条后 == N」（「每条 +2」也能过） | 读数**序列**必须是 `0,1,2,3…` ⇒ ⇒ 相邻**差 1** |
//! | 「写完读回 == 写进去的」（之后再也不加也能过） | 写**之后隔 1 条 / 3 条**再读 ⇒ ⇒ 必须是 `+1` / `+3` |
//! | 「写 `mcycle` 抑制了 `minstret`」（抑制过头也过） | **反向**用例：写 `mcycle` **不**抑制 `minstret` |
//! | 「影子 == 机器态」（两边都恒 0 也过） | 先**写**一个非 0 值 ⇒ ⇒ 影子必须报出那个值 |
//! | 「陷阱不计数」（陷阱根本不执行也过） | **同构对照**：普通指令**计入**、陷阱指令**不**计入 |

use riscv_sim_contract::csr::{
    CSR_CYCLE, CSR_CYCLEH, CSR_INSTRET, CSR_INSTRETH, CSR_MCOUNTINHIBIT, CSR_MCYCLE, CSR_MCYCLEH,
    CSR_MINSTRET, CSR_MINSTRETH, CSR_TIME, CSR_TIMEH,
};
use riscv_sim_contract::model::SimState;

use super::executor::{StepOutcome, TrapCause};
use super::tests_common::*;

// ---------------------------------------------------------------------------
// Encodings
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Drivers
// ---------------------------------------------------------------------------

/// Run a sequence of instructions, each placed at its own address.
///
/// ⚠️ 每条指令**单独**摆放 ⇒ ⇒ 陷阱之后 PC 被改成 `mtvec` 也**不影响**
/// 下一条指令的位置（`step_one_at` 会先把 PC 设回本条指令的地址）。
fn run(h: &mut Harness, state: &mut SimState, words: &[u32]) -> Vec<StepOutcome> {
    let mut outcomes = Vec::with_capacity(words.len());
    for (i, word) in words.iter().enumerate() {
        outcomes.push(h.step_one_at(state, BASE + 4 * i as u32, *word));
    }
    outcomes
}

/// Run `n` `nop`s, then read the counter **out of the CSR file**.
///
/// ⚠️ 文件里的 `minstret` 就是**已退休指令数**（每条退休指令 +1）⇒ ⇒
/// 这条读法**不含**「读指令自己也要退休」那层歧义。
fn count_after_nops(n: u32, csr: u16) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    for i in 0..n {
        h.step_one_at(&mut state, BASE + 4 * i, nop());
    }
    csr_value(&state, csr)
}

/// Run `n` `nop`s, then read the counter **through a `csrr`**（走**真的**
/// 读路径）。
///
/// ⚠️ 读到的值是**读指令退休之前**的值（R5）⇒ ⇒ 就是 `n`。
fn read_after_nops(n: u32, csr: u16) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    for i in 0..n {
        h.step_one_at(&mut state, BASE + 4 * i, nop());
    }
    h.step_one_at(&mut state, BASE + 4 * n, csrr(csr, 10));
    state.gpr[10]
}

/// Write `value` into `machine` and **immediately** read `shadow`.
///
/// ⚠️ 「紧接着」是**关键**：写计数器的那条指令**自己**不 +1 ⇒ ⇒ 中间
/// **插一条**别的指令，读到的就已经是「写值 + 1」了。
fn write_then_read(machine: u16, shadow: u16, value: u32) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = value;
    h.step_one_at(&mut state, BASE, csrw(machine, 1));
    h.step_one_at(&mut state, BASE + 4, csrr(shadow, 10));
    state.gpr[10]
}

/// Write `value` into `minstret`, run `nops` `nop`s, then read `minstret`
/// **through a `csrr`**.
///
/// ⚠️ 这条把「写值」与「之后又加了几条」**分开**数 ⇒ ⇒ 「隔 1 条」与
/// 「隔 3 条」是**两条独立**的观测（R4）。
fn write_minstret_then_read(value: u32, nops: u32) -> u32 {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = value;
    h.step_one_at(&mut state, BASE, csrw(CSR_MINSTRET, 1));
    for i in 0..nops {
        h.step_one_at(&mut state, BASE + 4 + 4 * i, nop());
    }
    h.step_one_at(&mut state, BASE + 8 + 4 * nops, csrr(CSR_MINSTRET, 10));
    state.gpr[10]
}

// ---------------------------------------------------------------------------
// B-1 — the counters start at zero
// ---------------------------------------------------------------------------

/// **B-1** —— 初始状态读 `mcycle` / `minstret` / `cycle` / `instret` == **0**。
///
/// ⚠️ 「文件里没有 ⇒ 读回 0」这条**巧合**证明不了任何东西 ⇒ ⇒ 每个 CSR
/// 都用**一个全新的程序**真读一次 —— 同一个程序里连读两次，第二次读到的
/// 已经是「第一条读指令退休之后」的值了。
#[test]
fn the_counters_start_at_zero() {
    let state = SimState::default();
    assert_eq!(csr_value(&state, CSR_MCYCLE), 0, "mcycle 初始必须是 0");
    assert_eq!(csr_value(&state, CSR_MINSTRET), 0, "minstret 初始必须是 0");
    assert_eq!(csr_value(&state, CSR_MCYCLEH), 0, "mcycleh 初始必须是 0");
    assert_eq!(
        csr_value(&state, CSR_MINSTRETH),
        0,
        "minstreth 初始必须是 0"
    );

    for csr in [
        CSR_MCYCLE,
        CSR_MINSTRET,
        CSR_CYCLE,
        CSR_INSTRET,
        CSR_CYCLEH,
        CSR_INSTRETH,
    ] {
        assert_eq!(read_after_nops(0, csr), 0, "csr {csr:#x} 初始必须读回 0");
    }
}

// ---------------------------------------------------------------------------
// B-2 — every retired instruction advances the count by exactly one
// ---------------------------------------------------------------------------

/// **B-2** —— 执行 N 条普通指令后读 `minstret` == **N**。
///
/// ⚠️ **判别力**：只断言「N 条后 == N」的话，「每条 **+2**」也能过 ⇒ ⇒
/// 这里断言**读数的序列**必须是 `0, 1, 2, 3, …` ⇒ ⇒ 相邻两次**差 1**
/// ⇒ ⇒ 「+2」的实现会给出 `0, 2, 4, 6` ⇒ ⇒ **红**。
#[test]
fn every_retired_instruction_advances_the_count_by_exactly_one() {
    for n in 0..=4u32 {
        assert_eq!(
            count_after_nops(n, CSR_MINSTRET),
            n,
            "{n} 条指令之后 minstret 必须是 {n} ⇒ ⇒ 每条**恰好** +1"
        );
    }
    // ⚠️ **绝对值**也要钉住（规格 B-2 的字面表述）。
    assert_eq!(
        count_after_nops(10, CSR_MINSTRET),
        10,
        "10 条指令之后 == 10"
    );
    // ⚠️ `mcycle` 与 `minstret` **都**每条 +1（本平台无停顿模型 ⇒ ⇒
    // 两者数值**相同**，与参考侧一致）。
    assert_eq!(
        count_after_nops(10, CSR_MCYCLE),
        10,
        "10 条指令之后 mcycle 必须是 10"
    );
    // ⚠️ **读路径**与**文件**必须一致（R5：读到的是退休前的值）。
    assert_eq!(
        read_after_nops(7, CSR_MINSTRET),
        7,
        "7 条指令之后读 minstret 必须是 7"
    );
}

/// **B-2（另一条 dispatch 路径）** —— **总线口**也必须计数。
///
/// ⚠️ 生产路径是 [`super::Iss::step_one_via_bus`] ⇒ ⇒ 如果自增只落在
/// 闭包口，生产路径的计数就是 **0** ⇒ ⇒ 这条用例是「**两条路径共用同一
/// 个落点**」的证据（规格 §3.2 的理由 1）。
#[test]
fn the_counters_advance_on_the_bus_dispatch_route_too() {
    // 三条 `nop` + 一条 `csrr`。
    let words = [nop(), nop(), nop(), csrr(CSR_MINSTRET, 10)];
    let mut bytes = image(&words);
    let mut bh = BusHarness::new(&bytes);
    let mut state = SimState::default();
    for _ in 0..4 {
        bh.step(&mut state).expect("bus route must not error");
    }
    assert_eq!(
        state.gpr[10], 3,
        "总线口执行 3 条 nop 之后，minstret 必须是 3 ⇒ ⇒ 两条 dispatch 路径共用同一个落点"
    );
    bytes.clear();
}

// ---------------------------------------------------------------------------
// B-3 — the shadows report the machine counters
// ---------------------------------------------------------------------------

/// **B-3** —— 读 `cycle` / `instret` 的值与读 `mcycle` / `minstret`
/// **逐位相同**。
///
/// ⚠️ **同构对照**（规格 §1.1 的方法论第 3 条）：两个程序**布局逐条相同**、
/// **只差被读的那个 CSR 号** ⇒ ⇒ 两次读发生在**同一个退休点** ⇒ ⇒
/// 直接比结果，**不做差值推导**。
#[test]
fn the_shadows_read_the_same_bits_as_the_machine_counters() {
    assert_eq!(
        read_after_nops(5, CSR_CYCLE),
        read_after_nops(5, CSR_MCYCLE),
        "影子 `cycle` 必须报出 `mcycle` 的**同一份**值"
    );
    assert_eq!(
        read_after_nops(5, CSR_INSTRET),
        read_after_nops(5, CSR_MINSTRET),
        "影子 `instret` 必须报出 `minstret` 的**同一份**值"
    );
    assert_eq!(
        read_after_nops(5, CSR_CYCLEH),
        read_after_nops(5, CSR_MCYCLEH),
        "影子 `cycleh` 必须报出 `mcycleh` 的**同一份**值"
    );
    assert_eq!(
        read_after_nops(5, CSR_INSTRETH),
        read_after_nops(5, CSR_MINSTRETH),
        "影子 `instreth` 必须报出 `minstreth` 的**同一份**值"
    );
}

/// **B-3（判别力）** —— 影子报的是**机器态计数器那个值**，不是**它自己**
/// 的某份副本。
///
/// ⚠️ 只断言「两者相等」的话，**两个都恒 0** 也能过 ⇒ ⇒ 这里先**写**一个
/// 非 0 的值进去，再读影子 ⇒ ⇒ 影子必须报出**那个值** ⇒ ⇒ 一个「影子是
/// 一份独立计数器」的实现会报出别的数 ⇒ ⇒ **红**。
#[test]
fn the_shadows_report_the_value_written_into_the_machine_counter() {
    assert_eq!(
        write_then_read(CSR_MCYCLE, CSR_CYCLE, 100),
        100,
        "写 mcycle = 100 之后紧接着读 cycle 必须是 100"
    );
    assert_eq!(
        write_then_read(CSR_MINSTRET, CSR_INSTRET, 100),
        100,
        "写 minstret = 100 之后紧接着读 instret 必须是 100"
    );
    assert_eq!(
        write_then_read(CSR_MCYCLEH, CSR_CYCLEH, 0x11),
        0x11,
        "写 mcycleh = 0x11 之后紧接着读 cycleh 必须是 0x11"
    );
    assert_eq!(
        write_then_read(CSR_MINSTRETH, CSR_INSTRETH, 0x11),
        0x11,
        "写 minstreth = 0x11 之后紧接着读 instreth 必须是 0x11"
    );
}

/// **影子是推导量，不是第二份存储**（裁决 D）。
///
/// ⚠️ 影子**不进** CSR 文件 ⇒ ⇒ 快照里不会出现两份值 ⇒ ⇒ 不会出现
/// 「改一个不改另一个」的不同步。
#[test]
fn the_shadows_are_derived_and_never_stored() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, BASE, csrr(CSR_CYCLE, 10));
    h.step_one_at(&mut state, BASE + 4, csrr(CSR_INSTRET, 11));
    h.step_one_at(&mut state, BASE + 8, csrr(CSR_CYCLEH, 12));
    h.step_one_at(&mut state, BASE + 12, csrr(CSR_INSTRETH, 13));
    for shadow in [CSR_CYCLE, CSR_INSTRET, CSR_CYCLEH, CSR_INSTRETH] {
        assert!(
            !csr_numbers(&state).contains(&shadow),
            "影子 {shadow:#x} **不得**进 CSR 文件 ⇒ ⇒ 影子是推导量"
        );
    }
    // ⚠️ **对照**：机器态计数器**必须**在文件里（它才是落点）。
    assert!(
        csr_numbers(&state).contains(&CSR_MCYCLE),
        "机器态计数器必须落在 CSR 文件里"
    );
    assert!(
        csr_numbers(&state).contains(&CSR_MINSTRET),
        "机器态计数器必须落在 CSR 文件里"
    );
}

// ---------------------------------------------------------------------------
// B-4 — writing a counter reads back exactly what was written
// ---------------------------------------------------------------------------

/// **B-4** —— 写 `mcycle` / `minstret` **立刻**读回 == **写进去的值**。
///
/// ⚠️ 这条同时钉住 **R3**（写的那条指令**自己**不递增）与 **R1**（可写）。
#[test]
fn writing_a_counter_reads_back_exactly_what_was_written() {
    for csr in [CSR_MCYCLE, CSR_MINSTRET, CSR_MCYCLEH, CSR_MINSTRETH] {
        assert_eq!(
            write_then_read(csr, csr, 0x1234_5678),
            0x1234_5678,
            "写 {csr:#x} = 0x12345678 之后立刻读回必须是那个值"
        );
    }
}

/// **R2** —— 写一个半部**不改变**另一个半部。
#[test]
fn writing_one_half_does_not_disturb_the_other() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xAAAA_AAAA;
    // 先写低半部。
    h.step_one_at(&mut state, BASE, csrw(CSR_MINSTRET, 1));
    // 再写高半部 —— 低半部**不能**被它动。
    h.step_one_at(&mut state, BASE + 4, csrw(CSR_MINSTRETH, 1));
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        0xAAAA_AAAA,
        "写 minstreth 不得改动 minstret（低半部）"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRETH),
        0xAAAA_AAAA,
        "minstreth 必须是自己写进去的那个值"
    );
}

// ---------------------------------------------------------------------------
// B-5 — after a write, the count keeps advancing by one
// ---------------------------------------------------------------------------

/// **B-5** —— 写 `minstret` 后隔 **1** 条指令再读 == 写值 **+1**；隔 **3**
/// 条 == 写值 **+3**。
///
/// ⚠️ **判别力**：只断言「写完立刻读 == 写值」的话，一个「**之后再也不
/// 加**」的实现也能过 ⇒ ⇒ 这里断言**之后**的读数。
#[test]
fn the_count_keeps_advancing_after_a_write() {
    assert_eq!(
        write_minstret_then_read(100, 0),
        100,
        "写的那条指令**自己**不 +1（R3）"
    );
    assert_eq!(
        write_minstret_then_read(100, 1),
        101,
        "隔 1 条指令之后必须读到 101 ⇒ ⇒ 每条 +1"
    );
    assert_eq!(
        write_minstret_then_read(100, 3),
        103,
        "隔 3 条指令之后必须读到 103 ⇒ ⇒ 100 + 3"
    );
}

// ---------------------------------------------------------------------------
// B-6 — the suppression is per counter
// ---------------------------------------------------------------------------

/// **B-6** —— 写 `mcycle` **不**抑制 `minstret` 的 +1。
#[test]
fn writing_mcycle_does_not_suppress_instret() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x2222_2222;
    // 两条 nop ⇒ minstret = 2。
    h.step_one_at(&mut state, BASE, nop());
    h.step_one_at(&mut state, BASE + 4, nop());
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "两条 nop 之后 minstret == 2"
    );
    // 写 mcycle —— 它**只**抑制 mcycle 自己的 +1。
    h.step_one_at(&mut state, BASE + 8, csrw(CSR_MCYCLE, 1));
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        3,
        "写 mcycle 之后 minstret 必须照常 +1（抑制是**按计数器**分的，R14）"
    );
    assert_eq!(
        csr_value(&state, CSR_MCYCLE),
        0x2222_2222,
        "mcycle 必须是写进去的那个值"
    );
    // ⚠️ 再走一次**读路径**确认。
    h.step_one_at(&mut state, BASE + 12, csrr(CSR_MINSTRET, 10));
    assert_eq!(state.gpr[10], 3, "读路径必须报出同一个值");
}

/// **B-6（反向）** —— 写 `minstret` **不**抑制 `mcycle` 的 +1。
#[test]
fn writing_minstret_does_not_suppress_mcycle() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x3333_3333;
    h.step_one_at(&mut state, BASE, nop());
    h.step_one_at(&mut state, BASE + 4, nop());
    assert_eq!(
        csr_value(&state, CSR_MCYCLE),
        2,
        "两条 nop 之后 mcycle == 2"
    );
    h.step_one_at(&mut state, BASE + 8, csrw(CSR_MINSTRET, 1));
    assert_eq!(
        csr_value(&state, CSR_MCYCLE),
        3,
        "写 minstret 之后 mcycle 必须照常 +1（抑制是**按计数器**分的，R14）"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        0x3333_3333,
        "minstret 必须是写进去的那个值"
    );
    h.step_one_at(&mut state, BASE + 12, csrr(CSR_MCYCLE, 10));
    assert_eq!(state.gpr[10], 3, "读路径必须报出同一个值");
}

/// **抑制对六个 CSR 写指令**全部**成立**（裁决 H）。
///
/// ⚠️ 六个写指令分支**都**在「写入未被忽略时」置位 `wrote_csr` ⇒ ⇒ 每一
/// 种写法都**必须**抑制那个计数器本指令的 +1。
///
/// ⚠️ **判据是「写进去的值原样读回」** —— 起点是 `minstret = 1` ⇒ ⇒ 若
/// 抑制**没**生效，读回的就是「写值 + 1」⇒ ⇒ 每一条期望值都**能把**
/// 「抑制生效」与「抑制失效」分开。
///
/// ⚠️ **反向判别**：`csrrs` 配 `rs1 == x0` 是**纯读**（规范如此）⇒ ⇒ 它
/// **不写** ⇒ ⇒ **不**抑制 ⇒ ⇒ 那条指令**照常** +1。
#[test]
fn every_csr_write_form_suppresses_the_counter_it_writes() {
    // (名字, 编码, 抑制生效时必须读回的值)
    let cases: [(&str, u32, u32); 6] = [
        // `csrrw x0, minstret, x1`（x1 = 0xFFFFFFFF）⇒ 写进去就是它。
        ("csrrw", csrw(CSR_MINSTRET, 1), 0xFFFF_FFFF),
        // `csrrs x0, minstret, x1`（x1 = 0xFFFFFFFF）⇒ 1 | 0xFFFFFFFF。
        ("csrrs", csrrs(CSR_MINSTRET, 1, 0), 0xFFFF_FFFF),
        // `csrrc x0, minstret, x1`（x1 = 0xFFFFFFFF）⇒ 1 & !0xFFFFFFFF = 0。
        ("csrrc", csrrc(CSR_MINSTRET, 1, 0), 0),
        // `csrrwi x0, minstret, 1` ⇒ 写进去就是 1。
        ("csrrwi", csrwi(CSR_MINSTRET, 1), 1),
        // `csrrsi x0, minstret, 1` ⇒ 1 | 1 = 1。
        ("csrrsi", csrrsi(CSR_MINSTRET, 1, 0), 1),
        // `csrrci x0, minstret, 1` ⇒ 1 & !1 = 0。
        ("csrrci", csrci(CSR_MINSTRET, 1), 0),
    ];
    for (name, write, expected) in cases {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[1] = 0xFFFF_FFFF;
        h.step_one_at(&mut state, BASE, nop()); // minstret = 1
        h.step_one_at(&mut state, BASE + 4, write);
        assert_eq!(
            csr_value(&state, CSR_MINSTRET),
            expected,
            "{name} 写了 minstret ⇒ ⇒ 读回必须是**写进去的那个值**\
             （若抑制没生效，这里会是「写值 + 1」）"
        );
    }
    // ⚠️ **反向**：`csrrs` 配 `rs1 == x0` 是**纯读** ⇒ ⇒ **不**抑制。
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, BASE, nop()); // minstret = 1
    h.step_one_at(&mut state, BASE + 4, csrrs(CSR_MINSTRET, 0, 10));
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "纯读（csrrs 配 rs1 == x0）**不写** ⇒ ⇒ **不**抑制 ⇒ ⇒ 照常 +1"
    );
}

// ---------------------------------------------------------------------------
// B-7 — writing the high half suppresses that instruction too
// ---------------------------------------------------------------------------

/// **B-7** —— 写 `minstreth`（**高半部**）**抑制** `minstret` 本指令的
/// +1，且**只**影响那一条。
///
/// ⚠️ 判定必须看**低半部是谁** ⇒ ⇒ 写高半部**也**抑制（R13）。
#[test]
fn writing_the_high_half_suppresses_only_that_one_instruction() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x11;
    // 两条 nop ⇒ minstret = 2。
    h.step_one_at(&mut state, BASE, nop());
    h.step_one_at(&mut state, BASE + 4, nop());
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "两条 nop 之后 minstret == 2"
    );
    assert_eq!(csr_value(&state, CSR_MINSTRETH), 0, "高半部初始是 0");

    // 写**高半部** ⇒ ⇒ minstret 本指令**不** +1（R13）。
    h.step_one_at(&mut state, BASE + 8, csrw(CSR_MINSTRETH, 1));
    assert_eq!(
        csr_value(&state, CSR_MINSTRETH),
        0x11,
        "minstreth 必须是写进去的那个值"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "写 minstreth 之后 minstret **不得** +1（R13：写高半部也抑制）"
    );

    // ⚠️ **只影响那一条** ⇒ ⇒ 下一条指令照常 +1。
    h.step_one_at(&mut state, BASE + 12, nop());
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        3,
        "抑制只影响**那一条**指令 ⇒ ⇒ 之后照常 +1"
    );
}

// ---------------------------------------------------------------------------
// B-8 — the low half wraps into the high half
// ---------------------------------------------------------------------------

/// **B-8** —— 低半部回绕时**高半部跟着 +1**（两半 + 进位 ≡ u64）。
///
/// ⚠️ 这是**上游 `rv32mi/instret_overflow` 的 `TEST_CASE(3)` / `(4)`**：
/// 写 `minstret` = `0xFFFFFFFF`、写 `minstreth` = `0xFFFFFFFF`、一条 `nop`
/// ⇒ ⇒ 低半部回绕到 `0`、高半部**也**回绕到 `0`。
#[test]
fn the_low_half_wraps_into_the_high_half() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    // 写低半部与高半部（两条写指令**都**抑制 minstret 的 +1）。
    h.step_one_at(&mut state, BASE, csrw(CSR_MINSTRET, 1));
    h.step_one_at(&mut state, BASE + 4, csrw(CSR_MINSTRETH, 1));
    assert_eq!(csr_value(&state, CSR_MINSTRET), 0xFFFF_FFFF);
    assert_eq!(csr_value(&state, CSR_MINSTRETH), 0xFFFF_FFFF);

    // 一条 nop ⇒ 低半部回绕，高半部跟着 +1（并**自己**回绕）。
    h.step_one_at(&mut state, BASE + 8, nop());
    assert_eq!(csr_value(&state, CSR_MINSTRET), 0, "低半部必须回绕到 0");
    assert_eq!(
        csr_value(&state, CSR_MINSTRETH),
        0,
        "高半部必须**跟着**进位（0xFFFFFFFF + 1 → 0）⇒ ⇒ 整体 64 位回绕"
    );
}

/// **B-8（进位本身）** —— 低半部从 `0xFFFF_FFFF` 回绕时，高半部**必须 +1**
/// （不是回绕）。
#[test]
fn the_high_half_is_incremented_when_the_low_half_wraps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    h.step_one_at(&mut state, BASE, csrw(CSR_MINSTRET, 1));
    h.step_one_at(&mut state, BASE + 4, csrw(CSR_MINSTRETH, 1));
    // 把高半部改成 5 —— 用 `csrw` 写一个**非 0** 值。
    state.gpr[1] = 5;
    h.step_one_at(&mut state, BASE + 8, csrw(CSR_MINSTRETH, 1));
    assert_eq!(csr_value(&state, CSR_MINSTRETH), 5);
    // 一条 nop ⇒ 低半部回绕 ⇒ 高半部 5 → 6。
    h.step_one_at(&mut state, BASE + 12, nop());
    assert_eq!(csr_value(&state, CSR_MINSTRET), 0, "低半部必须回绕到 0");
    assert_eq!(
        csr_value(&state, CSR_MINSTRETH),
        6,
        "低半部回绕时高半部必须**跟着 +1**（5 → 6）⇒ ⇒ 两半 + 进位 ≡ u64"
    );
}

// ---------------------------------------------------------------------------
// B-9 — `mcountinhibit` is writable and does not gate
// ---------------------------------------------------------------------------

/// **B-9** —— 写 `mcountinhibit` **不陷入**，且**写后能读回**。
///
/// ⚠️ 上游 `rv32mi/instret_overflow` 会 `csrci mcountinhibit, 0x4` ⇒ ⇒
/// **写它绝不能陷入**（规格 §3.5 的硬要求）。
#[test]
fn mcountinhibit_is_writable_and_never_traps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    let outcomes = run(
        &mut h,
        &mut state,
        &[
            csrw(CSR_MCOUNTINHIBIT, 1),
            // 上游的写法：`csrci mcountinhibit, 0x4`（**清** bit 2）。
            csrci(CSR_MCOUNTINHIBIT, 0x4),
            csrr(CSR_MCOUNTINHIBIT, 10),
        ],
    );
    assert_eq!(
        outcomes[0],
        StepOutcome::Advanced,
        "写 mcountinhibit 绝不能陷入"
    );
    assert_eq!(
        outcomes[1],
        StepOutcome::Advanced,
        "csrci mcountinhibit 绝不能陷入（上游 instret_overflow 的写法）"
    );
    assert_eq!(
        state.gpr[10], 0xFFFF_FFFB,
        "mcountinhibit 必须能读回写进去的值（R8）—— csrci 清掉 bit 2"
    );
    // `csrwi` 的形式同样可写。
    let outcomes = run(
        &mut h,
        &mut state,
        &[csrwi(CSR_MCOUNTINHIBIT, 0x5), csrr(CSR_MCOUNTINHIBIT, 11)],
    );
    assert_eq!(
        outcomes[0],
        StepOutcome::Advanced,
        "csrwi mcountinhibit 不陷入"
    );
    assert_eq!(state.gpr[11], 0x5, "mcountinhibit 必须读回 5");
}

/// **R9 / 裁决 J** —— `mcountinhibit` **不门控**任何读数。
///
/// ⚠️ 这**不是**漏实现：规范上门控**只作用于 U 模式**读 `cycle` / `instret`
/// / `time` ⇒ ⇒ 本平台**只有 M 模式** ⇒ ⇒ M 模式下按规范就**无效果**
/// （参考侧实测一致：R9）。
#[test]
fn mcountinhibit_does_not_gate_the_counters() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    // 把**所有**门控位都置上（bit 1 = CY，bit 2 = IR）。
    let outcomes = run(
        &mut h,
        &mut state,
        &[csrw(CSR_MCOUNTINHIBIT, 1), nop(), nop()],
    );
    assert_eq!(
        outcomes[0],
        StepOutcome::Advanced,
        "写 mcountinhibit 不陷入"
    );
    assert_eq!(
        csr_value(&state, CSR_MCOUNTINHIBIT),
        0xFFFF_FFFF,
        "mcountinhibit 必须真的写进去了"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        3,
        "mcountinhibit 置上之后 minstret 仍必须照常 +1（R9：M 模式下无门控效果）\
         ⇒ ⇒ 若门控生效，这里会是 0"
    );
}

// ---------------------------------------------------------------------------
// B-10 — a trapping instruction does not retire
// ---------------------------------------------------------------------------

/// **B-10** —— 陷阱指令（`ecall`）**不**计入退休计数；普通指令计入。
///
/// ⚠️ 这是「**落点选对了**」的关键证据：陷阱分支是**提前 `return`** ⇒ ⇒
/// **到不了**自增那行 ⇒ ⇒ 陷阱**天然**不退休（裁决 G / R11）。
#[test]
fn a_trapping_instruction_does_not_retire() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // 两条普通指令 ⇒ minstret = 2。
    h.step_one_at(&mut state, BASE, nop());
    h.step_one_at(&mut state, BASE + 4, nop());
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "两条 nop 之后 minstret == 2"
    );

    // 一条 `ecall` ⇒ 陷阱 ⇒ **不**退休。
    let outcome = h.step_one_at(&mut state, BASE + 8, ecall());
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
        },
        "ecall 必须陷入"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "陷阱指令**不得**计入退休计数（R11）"
    );

    // ⚠️ **同构对照**（规格 §1.1 的方法论第 4 条：互斥判据必须有同构
    // 对照）：同样**三条**指令，第三条换成 `nop` ⇒ ⇒ 计数是 **3**。
    let mut control = Harness::new();
    let mut control_state = SimState::default();
    control.step_one_at(&mut control_state, BASE, nop());
    control.step_one_at(&mut control_state, BASE + 4, nop());
    control.step_one_at(&mut control_state, BASE + 8, nop());
    assert_eq!(
        csr_value(&control_state, CSR_MINSTRET),
        3,
        "同构对照：三条**普通**指令 ⇒ 计数是 3"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "第三条换成 `ecall` ⇒ ⇒ 计数只有 2 ⇒ ⇒ 陷阱指令不计入退休"
    );

    // ⚠️ 陷阱之后**普通**指令照常计入。
    h.step_one_at(&mut state, BASE + 12, nop());
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        3,
        "陷阱之后的普通指令照常计入"
    );
    h.step_one_at(&mut state, BASE + 16, csrr(CSR_MINSTRET, 10));
    assert_eq!(state.gpr[10], 3, "读路径必须报出同一个值");
}

// ---------------------------------------------------------------------------
// R7 / R10 — the shadows are read-only, `time` reads zero
// ---------------------------------------------------------------------------

/// **R7** —— 影子 CSR **只读**：写被忽略，且**不**抑制任何计数器。
///
/// ⚠️ 影子写**不**抑制 ⇒ ⇒ 那条指令**照常** +1（判别力：抑制过头也会红）。
#[test]
fn writing_a_shadow_is_ignored_and_does_not_suppress() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x1234;
    h.step_one_at(&mut state, BASE, nop());
    let outcome = h.step_one_at(&mut state, BASE + 4, csrw(CSR_CYCLE, 1));
    assert_eq!(outcome, StepOutcome::Advanced, "写影子不陷入");
    assert!(
        !csr_numbers(&state).contains(&CSR_CYCLE),
        "影子的写必须被忽略 ⇒ ⇒ 不得进 CSR 文件"
    );
    assert_eq!(
        csr_value(&state, CSR_MINSTRET),
        2,
        "写影子**不得**抑制计数 ⇒ ⇒ 那条指令照常 +1"
    );
}

/// **R10** —— `time` / `timeh` **恒 0**。
///
/// ⚠️ 参考侧也恒 0（参考侧未接 `mtime`）⇒ ⇒ 逐位一致 ⇒ 不影响黄金对拍。
/// ⚠️ **但功能确实缺失** ⇒ 真值要接 M-15 的 CLINT ⇒ ⇒ 那是**单独登记的
/// 欠账**（裁决 K），不是本批的活。
#[test]
fn time_and_timeh_read_zero() {
    for csr in [CSR_TIME, CSR_TIMEH] {
        assert_eq!(read_after_nops(0, csr), 0, "csr {csr:#x} 必须恒读 0（R10）");
        // ⚠️ **对照**：跑过指令之后**仍然是 0**（不是一个跟着走的计数器）。
        assert_eq!(
            read_after_nops(5, csr),
            0,
            "csr {csr:#x} 跑过 5 条指令之后**仍然**必须是 0（R10）"
        );
    }
}

/// **R7（另一个影子）** —— 写 `instret` 同样被忽略。
#[test]
fn writing_the_instret_shadow_is_ignored() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x5678;
    h.step_one_at(&mut state, BASE, csrw(CSR_INSTRET, 1));
    assert!(
        !csr_numbers(&state).contains(&CSR_INSTRET),
        "instret 的写必须被忽略"
    );
    // ⚠️ **对照**：机器态计数器**可以**写 ⇒ ⇒ 影子与机器态不是一回事。
    h.step_one_at(&mut state, BASE + 4, csrw(CSR_MINSTRET, 1));
    assert!(
        csr_numbers(&state).contains(&CSR_MINSTRET),
        "机器态计数器必须可以写"
    );
}
