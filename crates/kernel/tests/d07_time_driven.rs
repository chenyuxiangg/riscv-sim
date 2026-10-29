//! D-07「唤醒时间驱动链路」端到端测试。
//!
//! D-07 的台账原话是「M-12 的 `dispatch` 真正调用它」——但实测下来，
//! 那只是**最后一处**断口。整条链路有**四处**断口，缺一不可：
//!
//! | # | 断口 | 缺了什么 | 谁来补 |
//! | --- | --- | --- | --- |
//! | 1 | **没人装 ticker** | `Engine::set_peripheral_ticker` 在生产代码里**从无调用** | M-17 / 宿主 |
//! | 2 | **没人排 `PeripheralTick`** | 事件队列里**永远没有**外设 tick 事件 | M-12 主循环 |
//! | 3 | **`pending_writes` 只写不读** | 外设 tick 返回的写**永远落不到总线上** | M-17 执行器 |
//! | 4 | **时间驱动主循环无人调用** | `run_until`（LLD 的唯一主循环）**从不被执行** | M-17 `Continue` |
//!
//! 本文件对着**四处断口**逐条钉住判别式，判别式一律落在**被依赖方自己
//! 身上**：
//!
//! - 断口 1/4 → 宿主与后端的**可观测行为**（外设时钟有没有被推进、
//!   探针边沿有没有带上真实时间戳）；
//! - 断口 2 → **事件队列的内容**（有没有 `PeripheralTick` 事件、
//!   有没有被派发）；
//! - 断口 3 → **总线上的内存**（外设写的值读不读得到）。
//!
//! 每条用例都做过反向变异验证（改坏实现 → 必须变红），口径见
//! `docs/reviews/REVIEWS.md`。
//!
//! ## 本文件钉的是**默认**口径
//!
//! 量子（1 µs）、每量子指令配额（1000 条）、每轮指令预算（10 000 条）
//! 在 LLD 里是**可按工程配置**的，但代码里目前是编译期常量。本文件钉住
//! 的是**默认值**——把配额改成 20 000（一次合法的配置改动）会让若干用例
//! 变红，这是**已知且已登记**的耦合（台账 **D-62**）。
//!
//! 凡是**本来就与配置无关**的不变量（两条路径的推进序列逐条相同、
//! `until_ps` 不被冲过、迟到事件记当前时钟），本文件一律写成**相对单条
//! 指令成本**的形式——它们不该因为改配置而变红。确实需要特定配置的用例，
//! 则在开头调用 [`assert_the_budget_spans_a_quantum`]，让前提不成立时
//! **红得清楚**。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::{Breakpoint, PendingWrite, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink, Peripheral};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, HaltReason, Message};
use riscv_sim_kernel::kernel_host::KernelHost;
use riscv_sim_kernel::m12_engine::{
    Engine, ExecOutcome, InstructionExecutor, PeripheralTicker, SchedEvent, TIME_QUANTUM_PS,
};
use riscv_sim_kernel::m14_bus::{AccessDir, Bus, BusAccess, BusWidth};
use riscv_sim_kernel::m17_backend::SelfIssBackend;

/// 外设的 tick 记录（共享句柄）——测试读它就知道外设被推进过几次。
type TickLog = Rc<RefCell<Vec<u64>>>;
/// 外设想提交的写（共享句柄）。
type PendingLog = Rc<RefCell<Vec<PendingWrite>>>;

const RAM_BASE: u32 = 0;
const RAM_SIZE: u32 = 0x1_0000;
const PERIPH_BASE: u32 = 0x4000_0000;
const PERIPH_SIZE: u32 = 0x1000;
/// 外设 tick 时想写进 RAM 的地址。
///
/// ⚠️ 必须在 guest **跑不到**的地方：一轮预算 10 000 条指令 = 40 000 字节，
/// 所以目标地址要落在 0x9C40 之后，否则这条写会把固件自己改坏
/// （`0x00C0_FFEE` 不是合法指令 → 陷阱 → 测试测到的就是陷阱而不是
/// 提交路径）。
const PENDING_TARGET: u32 = 0xF000;
const PENDING_VALUE: u32 = 0x00C0_FFEE;

// ---------------------------------------------------------------------------
// 测试替身
// ---------------------------------------------------------------------------

/// 什么都不做的汇（事件总线 / 中断汇）。
#[derive(Debug, Default, Clone)]
struct NullSinks;

impl EventBus for NullSinks {
    fn publish(&mut self, _evt: EventPayload) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl InterruptSink for NullSinks {
    fn raise(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
    fn lower(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// 记录自己被 tick 过几次、每次的时间戳，并在 tick 时**要求**总线写一条
/// 数据（`PendingWrite`）。
///
/// 这是本文件的核心观察点：外设的 `tick` 是整条时间驱动链路的**咽喉**，
/// 它被调用与否、被调用几次、时间戳是什么，全都能从这里读出来。
struct RecordingPeripheral {
    id: String,
    base: u32,
    size: u32,
    /// 每次 tick 的时间戳（按调用顺序）。
    ticks: TickLog,
    /// tick 时想提交的写。
    pending: PendingLog,
    /// 只在**这个时刻**吐出待写的写（`None` = 第一次 tick 就吐，之后不再吐）。
    ///
    /// 为什么需要它：外设 tick 有**两条**生产路径（指令边界 / 量子边界
    /// 事件），两者拿到的是**同一个**回调。若夹具每次都吐同一条写，两条
    /// 路径拿到的是**同一份数据** —— 删掉任一条路径的「写收集」分支，
    /// 用例照样全绿（V-B 检视 · F-01）。限定「只在某个时刻吐」才能把两条
    /// 路径**分开**。
    emit_at: Option<u64>,
    /// 已经吐过一次。
    emitted: bool,
}

impl RecordingPeripheral {
    fn spawn(base: u32, size: u32) -> (Rc<RefCell<dyn Peripheral>>, TickLog, PendingLog) {
        Self::spawn_at(base, size, None)
    }

    /// `emit_at`：`Some(t)` = **只在** `now_ps == t` 那一次吐写；`None` =
    /// 第一次 tick 吐一次（之后不再吐）。
    fn spawn_at(
        base: u32,
        size: u32,
        emit_at: Option<u64>,
    ) -> (Rc<RefCell<dyn Peripheral>>, TickLog, PendingLog) {
        let ticks = Rc::new(RefCell::new(Vec::new()));
        let pending = Rc::new(RefCell::new(Vec::new()));
        let me = Rc::new(RefCell::new(Self {
            id: format!("REC{base:X}"),
            base,
            size,
            ticks: Rc::clone(&ticks),
            pending: Rc::clone(&pending),
            emit_at,
            emitted: false,
        }));
        (me, ticks, pending)
    }
}

impl Peripheral for RecordingPeripheral {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        self.size
    }
    fn read(&self, _offset: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        Ok(0)
    }
    fn write(&mut self, _offset: u32, _size: WriteSize, _value: u32) -> Result<(), PlatformError> {
        Ok(())
    }
    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        self.ticks.borrow_mut().push(now_ps);
        // 只吐**一次**——同一条写不许被重复提交几万次（V-B 检视 · F-14）。
        let due = match self.emit_at {
            Some(t) => now_ps == t,
            None => !self.emitted,
        };
        if !due {
            return Ok(Vec::new());
        }
        self.emitted = true;
        let mut pending = self.pending.borrow_mut();
        let out = pending.clone();
        pending.clear();
        Ok(out)
    }
}

/// 本文件的若干用例要求「**一轮指令预算能跨过一个量子边界**」。
///
/// 默认配置下成立（10 000 条 × 1 ns = 10 µs > 1 µs）。若将来按工程配置
/// 量子 / 配额 / 预算，这个前提可能不成立——那时这些用例要拆成「默认值」
/// 与「可配置性」两组（台账 **D-62**）。
///
/// ⚠️ 这个断言的作用是**让红红得清楚**：前提不成立时给一句人话，而不是让
/// 后面某个断言莫名其妙地失败。
fn assert_the_budget_spans_a_quantum() {
    let budget = u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET);
    let cost = riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS;
    assert!(
        budget * cost > TIME_QUANTUM_PS,
        "本用例要求一轮指令预算能跨过一个量子边界：budget={budget} cost={cost} 量子={TIME_QUANTUM_PS}"
    );
}

/// 一条 `nop`（`addi x0, x0, 0`）。
const NOP: u32 = 0x0000_0013;

/// 低段 RAM 填满 `nop`（一直到 `PENDING_TARGET` 之前）。
///
/// ⚠️ 程序必须**填满** guest 在本轮预算内能跑到的全部范围：否则 guest
/// 跑到程序末尾会撞上未初始化的零（`0x0000_0000` = 非法指令）而陷入，
/// 测试就会在「陷阱」而不是在「时间驱动」上失败——测的就不是想测的东西。
fn nop_program() -> Vec<u8> {
    let len = PENDING_TARGET as usize;
    let mut out = Vec::with_capacity(len);
    for _ in 0..(len / 4) {
        out.extend_from_slice(&NOP.to_le_bytes());
    }
    out
}

/// 一条**非法指令**（全零 → opcode 0 不是合法的 RV32I 指令）。
///
/// 用来构造「第一条指令就陷入」的场景。
fn trap_program() -> Vec<u8> {
    vec![0u8; 4]
}

/// 造一个后端：低段 RAM + 一个会记账的外设。
fn backend_with_peripheral(
    program: &[u8],
) -> (SelfIssBackend<NullSinks, NullSinks>, TickLog, PendingLog) {
    backend_with_peripheral_emit_at(program, None)
}

/// `emit_at`：见 [`RecordingPeripheral`] 的同名字段——限定外设**只在某个
/// 时刻**吐出待写的写，用来把两条生产路径**分开**。
fn backend_with_peripheral_emit_at(
    program: &[u8],
    emit_at: Option<u64>,
) -> (SelfIssBackend<NullSinks, NullSinks>, TickLog, PendingLog) {
    let (periph, ticks, pending) = RecordingPeripheral::spawn_at(PERIPH_BASE, PERIPH_SIZE, emit_at);
    let mut bus = Bus::new();
    bus.map_ram(RAM_BASE, RAM_SIZE, program).expect("map ram");
    bus.map_peripheral_instance(PERIPH_BASE, PERIPH_SIZE, periph)
        .expect("map peripheral");
    let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
    backend.install_peripheral_ticker();
    (backend, ticks, pending)
}

// ---------------------------------------------------------------------------
// 宿主层面：Continue 走的是时间驱动路径
// ---------------------------------------------------------------------------

#[test]
fn the_continue_path_leaves_a_pending_peripheral_tick() {
    // 宿主层面的判别式：Continue 走的是**时间驱动**路径，所以跑完之后
    // 事件队列里**必须**留着一件待派发的外设 tick 事件（下一个量子
    // 边界的那一次心跳）。
    //
    // 指令驱动的路径（`StepN`）不排期它 → 队列是空的。这两个断言合
    // 起来，就把「宿主把 Continue 接到时间驱动路径上」这件事钉住了。
    let mut host = KernelHost::new();
    let _ = host.handle_message(&Message::command(1, CommandPayload::Continue));
    assert!(
        host.pending_event_count() > 0,
        "Continue 之后必须留着一件外设 tick 事件——那是时间驱动链路的心跳"
    );

    let mut host2 = KernelHost::new();
    let _ = host2.handle_message(&Message::command(2, CommandPayload::StepN { n: 1 }));
    assert_eq!(
        host2.pending_event_count(),
        0,
        "指令驱动的路径不排期外设 tick 事件"
    );
}

#[test]
fn a_failing_peripheral_does_not_stop_the_others_from_ticking() {
    // D-46 · V-A-4 的性质在**新**路径上必须仍然成立：一个外设 tick
    // 报错，其余外设**照样**被推进——时钟同步不能被别人拖停。
    //
    // ⚠️ 这条性质原来是 M-14 的 `Bus::tick_peripherals` 提供的；D-07 把
    // 外设时钟的推进点**收敛**到 M-12 的 `tick_peripherals` 之后，性质
    // 必须**跟着搬过来**——否则就是「改实现时把别处的性质弄丢了」。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    engine.register_peripheral(0x2000);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    engine.set_peripheral_ticker(PeripheralTicker::new(move |paddr, _now_ps| {
        sink.borrow_mut().push(paddr);
        if paddr == 0x1000 {
            return Err(PlatformError::new(
                riscv_sim_contract::error::ErrorFamily::Peripheral,
                1,
                "boom",
            ));
        }
        Ok(Vec::new())
    }));

    let err = engine
        .tick_peripherals(42)
        .expect_err("第一个外设报错必须报出来");
    assert_eq!(
        err.family,
        riscv_sim_contract::error::ErrorFamily::Peripheral
    );
    assert_eq!(
        *seen.borrow(),
        vec![0x1000, 0x2000],
        "报错的外设不许牵连别人——其余外设照样 tick"
    );
}

// ---------------------------------------------------------------------------
// 口径：量子 ↔ 指令的换算
// ---------------------------------------------------------------------------

#[test]
fn the_quantum_is_one_microsecond_and_the_quota_is_a_thousand_instructions() {
    // LLD §2.1：`quantum` 默认 **1 µs**，`quantumInstrQuota` 默认 **1000
    // 条/量子**。两者的换算就是单条指令的成本（1 ns）。
    //
    // ⚠️ 历史实现把量子写成了 `1_000` ps（1 ns），注释却写「1 µs」——
    // 差了 1000 倍。这条用例把**绝对值**与**换算关系**一起钉住：
    // 少一条都不行（只钉关系的话，把三个常量一起改错仍然会自洽）。
    assert_eq!(
        TIME_QUANTUM_PS, 1_000_000,
        "时间量子是 1 µs（1 000 000 ps）"
    );
    assert_eq!(
        riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA,
        1_000,
        "每量子 1000 条指令"
    );
    assert_eq!(
        riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
        1_000,
        "单条指令 1 ns"
    );
    // ⚠️ **不写「量子 = 配额 × 单条指令成本」**（V-B 检视 · F-15）：那条是
    // **恒真式**——`INSTRUCTION_COST_PS` 本来就**定义成**
    // `TIME_QUANTUM_PS / QUANTUM_INSTR_QUOTA`，断言它等于自己的定义只是
    // 把「配置」重说一遍。
    //
    // 真正要守的是「**换算只有一个真相**」：M-17 不得自己另算一遍单条指令
    // 的成本——两个真相迟早会漂。
    assert_eq!(
        riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
        riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS,
        "单条指令的成本只有一个真相——M-17 必须转调 M-12，不能自己另算一遍"
    );
}

// ---------------------------------------------------------------------------
// 断口 1 + 4：生产代码真的装了 ticker，时间驱动主循环真的被调用
// ---------------------------------------------------------------------------

#[test]
fn the_peripheral_clock_is_advanced_by_the_time_driven_path() {
    // 断口 1（没人装 ticker）+ 断口 4（时间驱动主循环无人调用）。
    //
    // 判别式落在**外设自己身上**：它被 tick 过没有、被 tick 了几次。
    // 把 M-17 的 `install_peripheral_ticker` 改成空操作 → 次数是 0 →
    // 变红；把 `Continue` 改回「只排空事件、不执行指令」→ 次数是 0 →
    // 变红。
    let (mut backend, ticks, _pending) = backend_with_peripheral(&nop_program());
    assert!(
        ticks.borrow().is_empty(),
        "装配完还不该有任何 tick——tick 是运行期的事"
    );

    backend.continue_until_event().expect("continue");

    let seen = ticks.borrow();
    assert!(
        !seen.is_empty(),
        "时间驱动路径必须推进外设的时钟——否则外设永远停在 t=0"
    );
    // 时间戳必须单调不减：外设的时钟只能往前走。
    for w in seen.windows(2) {
        assert!(w[1] >= w[0], "外设时钟必须单调：{:?} → {:?}", w[0], w[1]);
    }
    // 最后一次 tick 的时间戳必须**紧贴**引擎的权威虚拟时间（差不超过
    // 一条指令）——外设记的时间就是引擎的时间。
    let now_ps = backend.engine().now_ps();
    let last = *seen.last().expect("至少一次 tick");
    assert!(
        now_ps >= last && now_ps - last <= riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
        "外设记下的时间必须紧贴引擎的权威虚拟时间：tick={last} now={now_ps}"
    );
    // ⚠️ **口径（V-A 检视 · F1 闭环后）**：tick 的条数必须**正好等于**
    // 执行的指令数——**不是**「多出几次」。
    //
    // 旧口径写的是「事件驱动的 tick 是额外的那几次」（`>`）：那是因为
    // 量子边界的 `PeripheralTick` 事件与本量子**第一条**指令的指令边界
    // 同步**各 tick 了一次**——同一个外设在**同一虚拟时刻**被 tick 两次。
    // 后果是两条驱动路径的外设时钟**对不上**（`StepN` 10 000 次 vs
    // `Continue` 10 009 次）。
    //
    // 现在：量子边界的事件**代劳**本量子第一条指令的那一次推进（指令
    // 边界跳过刚被事件 tick 过的外设），于是一个量子 = 1 次事件 tick +
    // (N-1) 次指令边界 tick = **N 次**，与指令数严格相等。
    //
    // 判别式因此落在「**条数相等**」上：把 F1 的修复改回去（两处都
    // tick）→ 条数变成 N+1 → 变红；把指令边界的 tick 整个去掉 → 条数
    // 变成 0 → 变红。
    assert_eq!(
        seen.len() as u64,
        backend.total_executed(),
        "tick 的条数必须正好等于执行的指令数——事件代劳本量子第一条的那一次：ticks={} executed={}",
        seen.len(),
        backend.total_executed()
    );
}

#[test]
fn both_paths_tick_the_peripheral_clock_identically() {
    // F1 的**不变量**（V-A 检视 · 🟠-1）：「指令驱动」与「时间驱动」是
    // **同一条**链路的两个入口——外设的时钟推进序列必须**逐条相同**。
    //
    // ⚠️ 这条不变量在 F1 修复**之前**是不成立的：同一个外设在**同一
    // 虚拟时刻**被 tick 两次（量子边界的事件一次 + 本量子第一条指令的
    // 指令边界一次），于是 `StepN` 与 `Continue` 的 tick 序列差了一截。
    //
    // 判别式落在**两个外设自己的记账**上（不是「跑起来没报错」）：
    // 逐条比对时间戳序列。
    //
    // 跨量子边界取样（`N > QUANTUM_INSTR_QUOTA`）：量子边界正是两条
    // 路径曾经分叉的地方，只在量子内部取样抓不到它。
    assert_the_budget_spans_a_quantum();
    // 取样长度**相对配额**给出：跨过量子边界即可，不必钉死具体数字
    // （换配置时也成立）。
    let n = riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA + 200;
    assert!(
        n > riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA,
        "取样必须跨过量子边界，否则抓不到 F1"
    );

    let (mut a, a_ticks, _a_pending) = backend_with_peripheral(&nop_program());
    a.step_n(n).expect("step");

    let (mut b, b_ticks, _b_pending) = backend_with_peripheral(&nop_program());
    b.continue_until_event().expect("continue");

    let a_seq = a_ticks.borrow().clone();
    let b_seq = b_ticks.borrow().clone();
    assert_eq!(
        a_seq.len(),
        n as usize,
        "指令驱动路径：每条指令推进一次外设时钟"
    );
    assert!(
        b_seq.len() >= n as usize,
        "时间驱动路径至少要走完同样的条数：b={}",
        b_seq.len()
    );
    assert_eq!(
        &b_seq[..n as usize],
        &a_seq[..],
        "两条路径的外设时钟序列必须逐条相同——\
         指令驱动是时间驱动量子内的填充物，不是另一条时间线"
    );
    // 逐条的时间戳必须正好是「指令序号 × 单条指令成本」——量子边界的
    // 事件代劳的那一次**不改变**时间戳序列（它顶替的正是那一条指令
    // 本来要记下的时刻）。
    for (j, ts) in a_seq.iter().enumerate() {
        assert_eq!(
            *ts,
            j as u64 * riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
            "第 {j} 次 tick 的时间戳必须是该指令的时刻"
        );
    }
}

#[test]
fn the_instruction_path_also_advances_the_peripheral_clock() {
    // 断口 1 的另一半：`step_n` 走的是**指令驱动**路径，它也必须推进
    // 外设时钟——否则探针边沿的时间戳在单步下恒为 0（D-46 的老坑）。
    let (mut backend, ticks, _pending) = backend_with_peripheral(&nop_program());
    backend.step_n(3).expect("step");
    assert_eq!(ticks.borrow().len(), 3, "每条指令之前推进一次外设时钟");
    assert_eq!(
        backend.engine().now_ps(),
        3 * riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS
    );
}

// ---------------------------------------------------------------------------
// 断口 3：`pending_writes` 必须真的落到总线上
// ---------------------------------------------------------------------------

#[test]
fn a_pending_write_from_a_peripheral_tick_reaches_the_bus() {
    // 断口 3：`pending_writes` 此前**只写不读**——外设 tick 返回的写被
    // 塞进 `state.pending_writes` 就再也没人管过。
    //
    // 判别式落在**总线上的内存**里：外设要的写，读得到吗？
    let (mut backend, _ticks, pending) = backend_with_peripheral(&nop_program());
    pending.borrow_mut().push(PendingWrite {
        paddr: PENDING_TARGET,
        size: WriteSize::Word,
        value: PENDING_VALUE,
        source: "d07-test".to_string(),
    });

    backend.continue_until_event().expect("continue");

    let value = backend
        .bus()
        .read_only(PENDING_TARGET, BusWidth::Word)
        .expect("read the committed word");
    assert_eq!(
        value, PENDING_VALUE,
        "外设 tick 要提交的写必须真的落到总线上"
    );
    // 提交之后队列必须被清空——否则同一个写会被提交两次。
    let q = backend.snapshot().expect("snapshot").pending_writes;
    assert!(q.is_empty(), "提交过的写不能留在队列里：{q:?}");
}

#[test]
fn a_failed_commit_keeps_the_write_in_the_queue() {
    // 「写一条不丢」：提交失败时，失败的那一条**留在队列里**，不静默丢。
    let (mut backend, _ticks, pending) = backend_with_peripheral(&nop_program());
    // 故意指向一个**没有映射**的地址 → 总线报错。
    pending.borrow_mut().push(PendingWrite {
        paddr: 0x7FFF_0000,
        size: WriteSize::Word,
        value: 0xDEAD,
        source: "d07-test".to_string(),
    });

    let err = backend
        .continue_until_event()
        .expect_err("提交到未映射地址必须报错");
    assert_eq!(err.family, riscv_sim_contract::error::ErrorFamily::Bus);
    let queued = backend.snapshot().expect("snapshot").pending_writes;
    assert_eq!(queued.len(), 1, "失败的那一条写不能被丢掉：{queued:?}");
    assert_eq!(queued[0].paddr, 0x7FFF_0000);
}

// ---------------------------------------------------------------------------
// 断口 2：`PeripheralTick` 事件被排期、被派发
// ---------------------------------------------------------------------------

#[test]
fn a_peripheral_tick_event_is_scheduled_at_the_quantum_boundary() {
    // 断口 2：事件队列里必须有 `PeripheralTick`，而且它到期于**量子
    // 边界**（1 µs），不是随便什么时刻。
    //
    // 判别式落在**队列的内容**上（不是「跑起来没报错」）。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(PERIPH_BASE);

    // 排一个空事件，让主循环跑起来。
    engine.schedule(
        10 * TIME_QUANTUM_PS,
        SchedEvent::TimerExpiry { irq: 1, at_ps: 0 },
    );
    let mut executor = CountingExecutor::new();
    engine
        .run_until(&mut executor, u64::MAX)
        .expect("run until");

    let queue = engine.snapshot_full().queue;
    let ticks: Vec<u64> = queue
        .iter()
        .filter_map(|e| match e.event {
            SchedEvent::PeripheralTick { at_ps, .. } => Some(at_ps),
            _ => None,
        })
        .collect();
    assert!(!ticks.is_empty(), "队列里必须有外设 tick 事件：{queue:?}");
    for at in &ticks {
        assert_eq!(
            at % TIME_QUANTUM_PS,
            0,
            "外设 tick 必须落在量子边界上：at={at}"
        );
    }
}

/// 计数器执行器：每次 `step` 推进一次虚拟时钟（1 ns），不产生任何副作用。
struct CountingExecutor {
    steps: u64,
}

impl CountingExecutor {
    fn new() -> Self {
        Self { steps: 0 }
    }
}

impl InstructionExecutor for CountingExecutor {
    fn step(
        &mut self,
        state: &mut riscv_sim_contract::model::SimState,
    ) -> Result<ExecOutcome, PlatformError> {
        self.steps += 1;
        state.virtual_time_ps = state
            .virtual_time_ps
            .saturating_add(riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS);
        Ok(ExecOutcome::Advanced)
    }
}

// ---------------------------------------------------------------------------
// 指令驱动与时间驱动如何交替
// ---------------------------------------------------------------------------

#[test]
fn one_quantum_executes_at_most_the_instruction_quota() {
    // 「指令驱动填充量子」：一个量子内最多执行
    // `QUANTUM_INSTR_QUOTA` 条指令，之后必须回到安全检查点。
    //
    // 判别式：把配额改成 10 000（本轮预算）→ 用例不变红说明它没在守
    // 量子边界——所以这里断言的是**一个量子内的条数**，而不是一轮的总数。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(PERIPH_BASE);
    let mut executor = CountingExecutor::new();
    let outcome = engine
        .run_until(&mut executor, u64::MAX)
        .expect("run until");
    assert_eq!(
        outcome.instructions,
        u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET),
        "一轮跑满指令预算就停"
    );
    // 量子数 = **向上取整**（预算 / 每量子配额）——预算不必是配额的整数倍
    // （最后一个量子可以是**半个**）。
    let budget = u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET);
    let quota = u64::from(riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA);
    assert_eq!(
        outcome.quanta,
        budget.div_ceil(quota),
        "量子数 = ⌈预算 / 每量子配额⌉"
    );
}

#[test]
fn a_due_event_preempts_the_quantum_at_an_instruction_boundary() {
    // 「时间驱动打断指令驱动」：事件到期时，量子**立刻**让路（事件边界
    // = 抢占点），而不是等整个量子跑完。
    //
    // 判别式：事件派发的时刻必须落在**量子内部**，且派发之后量子继续跑
    // （没有被吞掉）。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    // 一个在 1.5 µs 到期的事件 —— 落在第 2 个量子**内部**。
    engine.schedule(
        TIME_QUANTUM_PS + TIME_QUANTUM_PS / 2,
        SchedEvent::TimerExpiry { irq: 3, at_ps: 0 },
    );
    let mut executor = CountingExecutor::new();
    let outcome = engine
        .run_until(&mut executor, u64::MAX)
        .expect("run until");

    // 事件在**量子内部**到期 → 量子被**劈成两半** → 量子数比
    // 「预算 / 配额」多**恰好一个**。这正是「事件边界 = 抢占点」的可观测
    // 形状。
    //
    // ⚠️ **收紧到 `==` 而不是 `>`**（V-B 检视 · F-06）：`>` 只证明「多切
    // 了**至少**一次」——把量子粒度改成 1 条指令，它照样绿（quanta 会变成
    // 10 000 > 10）。`==` 才把「**恰好**被这一个事件劈了一次」钉住。
    let full = u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET)
        / u64::from(riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA);
    assert_eq!(
        outcome.quanta,
        full + 1,
        "量子内部到期的事件必须**恰好**劈开量子：quanta={} full={full}",
        outcome.quanta
    );
    assert_eq!(
        outcome.instructions,
        u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET),
        "抢占之后量子**继续**跑——事件不吞掉任何预算"
    );
}

#[test]
fn the_time_driven_path_stops_at_a_breakpoint() {
    // 时间驱动路径必须认断点——否则「跑到断点停下」这个调试器最基本的
    // 语义在 Continue 下就没了。
    let (mut backend, _ticks, _pending) = backend_with_peripheral(&nop_program());
    backend
        .set_breakpoint_with_id(Breakpoint {
            id: 7,
            addr: 0,
            source_hint: None,
            enabled: true,
            condition: None,
        })
        .expect("set breakpoint");

    backend.continue_until_event().expect("continue");

    assert_eq!(
        backend.last_stop_reason(),
        Some(HaltReason::Breakpoint),
        "断点必须让时间驱动路径停下来"
    );
    assert_eq!(
        backend.total_executed(),
        0,
        "断点挂在取指之前，命中的那条指令**没有**被执行"
    );
}

#[test]
fn the_instruction_path_and_the_time_driven_path_agree() {
    // 两条路径共用**同一个**执行器（`IssExecutor`）——所以同一段程序，
    // 「按指令跑」与「按时间跑」必须得到**同一个结果**。
    //
    // 判别式：给 `StepN` 与 `Continue` 两条路各放一个断点，跑到同一个
    // 地址，比对寄存器与 PC。
    let program = nop_program();
    let mut a = backend_with_peripheral(&program);
    let mut b = backend_with_peripheral(&program);

    // 断点放在第 8 条指令处（每条 4 字节）。
    let bp = |id: u32| Breakpoint {
        id,
        addr: 8 * 4,
        source_hint: None,
        enabled: true,
        condition: None,
    };
    a.0.set_breakpoint_with_id(bp(1)).expect("bp");
    b.0.set_breakpoint_with_id(bp(2)).expect("bp");

    a.0.step_n(100).expect("step");
    b.0.continue_until_event().expect("continue");

    assert_eq!(
        a.0.snapshot().expect("snapshot").pc,
        b.0.snapshot().expect("snapshot").pc,
        "两条路径必须停在同一个地址"
    );
    assert_eq!(a.0.total_executed(), b.0.total_executed());
    assert_eq!(
        a.0.last_stop_reason(),
        b.0.last_stop_reason(),
        "两条路径的停止原因必须一致"
    );
}

// ---------------------------------------------------------------------------
// 判别力缺口回填（V-A 检视 · 7 处变异「0 变红」）
//
// 这一节补的是**反向变异自检里 0 变红**的那些变异——它们改坏了实现，
// 但**没有任何一条用例会红**。注释里声称的不变量，必须有一条能变红的
// 用例守着；否则它只是一句愿望。
// ---------------------------------------------------------------------------

#[test]
fn a_late_dispatched_tick_records_the_current_clock_not_the_event_timestamp() {
    // 变异 M9（发现 F10）：`dispatch` 用**当前虚拟时钟**而不是事件自带的
    // `at_ps`——代码注释说「否则探针边沿会带着一个未来的时间戳」，但这个
    // 不变量**没有用例守**（变异 0 变红）。
    //
    // 场景：事件被**迟到**派发（LLD 评审 12-6 的归一化）——它自称
    // 「500 ps 到期」，可引擎的时钟已经走到 1000 ps 才轮到它。外设记下
    // 的必须是它**实际被推进的那一刻**（1000），不是 500。
    //
    // ⚠️ 这个外设**故意不登记**进引擎的登记表：登记了就会多出「指令
    // 边界」的推进，测的就不是「事件派发那一刻」的时间戳了。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    engine.set_peripheral_ticker(PeripheralTicker::new(move |_paddr, now_ps| {
        sink.borrow_mut().push(now_ps);
        Ok(Vec::new())
    }));
    // 事件自称「**半条指令**之后」到期——到期时刻也**相对单条指令成本**
    // 给出，换配置时它**仍然不是**某个指令边界（那正是本用例要的：
    // 「到期时刻」与「实际派发时刻」必须不同）。
    let half = riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS / 2;
    engine.schedule(
        half,
        SchedEvent::PeripheralTick {
            paddr: 0x1000,
            at_ps: half,
        },
    );

    let mut executor = CountingExecutor::new();
    engine
        .run_until(&mut executor, u64::MAX)
        .expect("run until");

    let log = seen.borrow();
    assert_eq!(
        *log,
        vec![riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS],
        "迟到派发的事件必须记下**当前**时钟（{}），不是事件自带的 at_ps（{}）",
        riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS,
        half
    );
}

#[test]
fn registering_the_same_peripheral_twice_ticks_it_only_once() {
    // 变异 M18（发现 F7）：`register_peripheral` 的「幂等」写在注释里，
    // 但**没有用例守**（变异 0 变红）。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    engine.register_peripheral(0x1000);
    assert_eq!(
        engine.registered_peripherals(),
        &[0x1000u32][..],
        "同一个基址登记两次只算一次"
    );

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    engine.set_peripheral_ticker(PeripheralTicker::new(move |paddr, now_ps| {
        sink.borrow_mut().push((paddr, now_ps));
        Ok(Vec::new())
    }));
    engine.tick_peripherals(42).expect("tick");
    assert_eq!(
        *seen.borrow(),
        vec![(0x1000, 42)],
        "同一个外设只被推进一次——登记两次不等于推进两次"
    );
}

#[test]
fn a_stale_peripheral_tick_event_is_not_rescheduled_forever() {
    // 变异 M20（发现 F6）：换基址（= 换工程 = 换外设集合）之后，队列里
    // **旧基址**的 tick 事件被无限续排——一件**永远等不到主人**的事件
    // 永远留在队列里。
    //
    // 判别式落在**队列的内容**上：换基址之后再跑一轮，队列里必须**只剩**
    // 新基址的那一件。
    assert_the_budget_spans_a_quantum();
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    let mut executor = CountingExecutor::new();
    engine.run_until(&mut executor, u64::MAX).expect("run");

    let periphs = |e: &Engine<NullSinks, NullSinks>| -> Vec<u32> {
        e.snapshot_full()
            .queue
            .iter()
            .filter_map(|ev| match ev.event {
                SchedEvent::PeripheralTick { paddr, .. } => Some(paddr),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        periphs(&engine),
        vec![0x1000],
        "第一轮之后队列里有一件 0x1000 的 tick"
    );

    // 换基址：登记表清空，登记新的外设。
    engine.clear_peripherals();
    engine.register_peripheral(0x2000);
    engine.run_until(&mut executor, u64::MAX).expect("run");

    let left = periphs(&engine);
    assert!(
        !left.contains(&0x1000),
        "旧基址的 tick 事件不许被续排——它永远等不到主人：{left:?}"
    );
    assert_eq!(left, vec![0x2000], "队列里只该剩新基址的那一件：{left:?}");
}

#[test]
fn run_until_does_not_overshoot_the_requested_time() {
    // 变异 M21（发现 F5）：`until_ps` 会被**冲过**最多一条指令的成本
    // （`run_until(4500)` 落到 5000）。语义是「跑到某个时刻」，那就**不该**
    // 越过它。
    //
    // ⚠️ 顺带守住「不许空转」：内层循环因为时间预检而一条也没执行时，
    // 外层必须收手（否则外层永远转下去 → 死循环）。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    let mut executor = CountingExecutor::new();
    // 目标时刻取「4 条指令 + 半条指令」——**相对单条指令成本**写成，
    // 换配置也不会变红。
    let cost = riscv_sim_kernel::m12_engine::INSTRUCTION_COST_PS;
    let until = 4 * cost + cost / 2;
    let outcome = engine.run_until(&mut executor, until).expect("run until");

    assert_eq!(
        outcome.instructions, 4,
        "4 条指令 = 4×{cost} ps；第 5 条会越过 {until}，不许跑"
    );
    assert_eq!(engine.now_ps(), 4 * cost);
    assert!(
        engine.now_ps() <= until,
        "不许冲过目标时刻：now={} until={until}",
        engine.now_ps()
    );
    assert_eq!(
        outcome.reason,
        riscv_sim_kernel::m12_engine::RunReason::TimeReached,
        "时间到头就该停"
    );
}

#[test]
fn a_failed_commit_still_charges_the_instruction_cost() {
    // 变异 M22：「提交待写」与「推进时钟」的**顺序**没有用例守。
    //
    // 口径：一条指令**一旦开始执行**，它的成本就记上账——即使它的副作用
    // （外设写）提交失败。那条指令已经取指、译码、退休了；让它变成
    // 「免费」会让虚拟时间与 guest 的实际进度脱节。
    //
    // 对照：**陷阱不推进时钟**（陷入的指令没有退休）——但陷阱**照样**
    // 提交（发现 F4）。
    let (mut backend, _ticks, pending) = backend_with_peripheral(&nop_program());
    // 故意指向一个**没有映射**的地址 → 总线报错。
    pending.borrow_mut().push(PendingWrite {
        paddr: 0x7FFF_0000,
        size: WriteSize::Word,
        value: 0xDEAD,
        source: "d07-test".to_string(),
    });

    let err = backend.step_n(1).expect_err("提交到未映射地址必须报错");
    assert_eq!(err.family, riscv_sim_contract::error::ErrorFamily::Bus);
    assert_eq!(
        backend.engine().now_ps(),
        riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
        "指令的成本必须先记账——提交失败不许让这条指令变成免费"
    );
}

#[test]
fn a_breakpoint_hit_does_not_advance_the_peripheral_clock() {
    // 变异 M23（发现 F3）：`step_n` 的**断点检查 ↔ 外设 tick 顺序**没有
    // 用例守。断点挂在取指之前 → 命中的那条指令**没有执行** → 它的成本
    // 不该记账，**外设的时钟也不该走一格**。
    let (mut backend, ticks, _pending) = backend_with_peripheral(&nop_program());
    backend
        .set_breakpoint_with_id(Breakpoint {
            id: 1,
            addr: 0,
            source_hint: None,
            enabled: true,
            condition: None,
        })
        .expect("set breakpoint");

    let executed = backend.step_n(1).expect("step");
    assert_eq!(executed, 0, "命中的那条指令没有执行");
    assert!(
        ticks.borrow().is_empty(),
        "没执行的指令不该让外设的时钟走一格：{:?}",
        ticks.borrow()
    );
}

#[test]
fn engine_reset_keeps_the_peripheral_registry() {
    // 变异 M24（发现 F11）：`Engine::reset()` 是否保留外设登记表没有
    // 判别力。
    //
    // 口径：`reset` = **重跑同一段程序**（同一套平台）——外设登记表与
    // ticker 接线属于**平台**，不属于**运行状态**，所以必须**活下来**。
    // （换工程换的才是平台，那时才清 —— 见 `clear_peripherals`。）
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    engine.reset();
    assert_eq!(
        engine.registered_peripherals(),
        &[0x1000u32][..],
        "reset 不该清掉外设登记表——平台没换"
    );
}

#[test]
fn a_trap_still_commits_the_pending_writes() {
    // 发现 F4：陷阱路径**不提交** `pending_writes`——外设在 tick 期间
    // 已经发生的动作，与 guest 这条指令成不成功**无关**。写在队列里
    // 不提交 = 外设的写**永远看不见**（写不丢，但**不可见**）。
    //
    // 判别式落在**总线上的内存**里：陷阱之后读得到吗？
    let (mut backend, _ticks, pending) = backend_with_peripheral(&trap_program());
    pending.borrow_mut().push(PendingWrite {
        paddr: PENDING_TARGET,
        size: WriteSize::Word,
        value: PENDING_VALUE,
        source: "d07-test".to_string(),
    });

    let executed = backend.step_n(1).expect("step");
    assert_eq!(
        executed, 1,
        "陷入的指令也算执行了（被取指、被译码、被触发）"
    );
    assert_eq!(
        backend.last_stop_reason(),
        Some(HaltReason::Trap),
        "第一条指令就该陷入"
    );

    let value = backend
        .bus()
        .read_only(PENDING_TARGET, BusWidth::Word)
        .expect("read the committed word");
    assert_eq!(
        value, PENDING_VALUE,
        "陷阱也必须提交外设的写——外设的动作与 guest 的成败无关"
    );
}

// ---------------------------------------------------------------------------
// V-B 检视轮的判别力回填（断口 3 的两条路径分离 + 若干生命周期不变量）
// ---------------------------------------------------------------------------

#[test]
fn the_instruction_boundary_path_delivers_the_peripheral_write() {
    // V-B 检视 · F-01：断口 3 的判别式在**两条生产路径之间冗余**——夹具
    // 每次都吐同一条写，两条路径拿到的是**同一份数据**，删掉任一条路径的
    // 「写收集」分支用例照样全绿。
    //
    // 这条把夹具限定在**指令边界那一刻**（t=0）吐写：量子边界那条路径
    // 拿不到 → 删掉「指令边界的写收集」就会变红。
    let (mut backend, _ticks, pending) = backend_with_peripheral_emit_at(&nop_program(), Some(0));
    pending.borrow_mut().push(PendingWrite {
        paddr: PENDING_TARGET,
        size: WriteSize::Word,
        value: PENDING_VALUE,
        source: "d07-test".to_string(),
    });

    backend.step_n(1).expect("step");

    let value = backend
        .bus()
        .read_only(PENDING_TARGET, BusWidth::Word)
        .expect("read the committed word");
    assert_eq!(
        value, PENDING_VALUE,
        "指令边界这条路径必须把外设的写收集起来并提交"
    );
}

#[test]
fn the_quantum_boundary_event_path_delivers_the_peripheral_write() {
    // 同上，但把夹具限定在**量子边界那一刻**（1 µs）吐写。
    //
    // ⚠️ F1 闭环之后，量子边界那一刻的 tick **只有事件路径**会做（指令
    // 边界的那一条被「刚 tick 过就跳过」挡掉了）——所以这条用例守的正
    // 是「事件路径的写收集」。
    assert_the_budget_spans_a_quantum();
    let (mut backend, _ticks, pending) =
        backend_with_peripheral_emit_at(&nop_program(), Some(TIME_QUANTUM_PS));
    pending.borrow_mut().push(PendingWrite {
        paddr: PENDING_TARGET,
        size: WriteSize::Word,
        value: PENDING_VALUE,
        source: "d07-test".to_string(),
    });

    backend.continue_until_event().expect("continue");

    let value = backend
        .bus()
        .read_only(PENDING_TARGET, BusWidth::Word)
        .expect("read the committed word");
    assert_eq!(
        value, PENDING_VALUE,
        "量子边界事件路径必须把外设的写收集起来并提交"
    );
}

#[test]
fn the_peripheral_write_is_committed_exactly_once() {
    // V-B 检视 · F-01 / F-14：夹具每次都吐同一条写 → 同一条写整轮被重复
    // 提交上万次；「队列必须被清空」靠的是「每 tick 补一条、每指令提交
    // 一条」的**动态平衡**——那不等于「提交了恰好一次」。
    //
    // 判别式落在**总线的写次数**上（不是「值对不对」）。
    let (periph, _ticks, pending) =
        RecordingPeripheral::spawn_at(PERIPH_BASE, PERIPH_SIZE, Some(0));
    pending.borrow_mut().push(PendingWrite {
        paddr: PENDING_TARGET,
        size: WriteSize::Word,
        value: PENDING_VALUE,
        source: "d07-test".to_string(),
    });

    let mut bus = Bus::new();
    bus.map_ram(RAM_BASE, RAM_SIZE, &nop_program())
        .expect("map ram");
    bus.map_peripheral_instance(PERIPH_BASE, PERIPH_SIZE, periph)
        .expect("map peripheral");
    let writes = Rc::new(RefCell::new(0usize));
    let counter = Rc::clone(&writes);
    bus.add_hook(Box::new(move |acc: &BusAccess| {
        if acc.addr == PENDING_TARGET && acc.dir == AccessDir::Write {
            *counter.borrow_mut() += 1;
        }
    }));
    let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
    backend.install_peripheral_ticker();

    backend.continue_until_event().expect("continue");

    assert_eq!(
        *writes.borrow(),
        1,
        "同一条写必须**恰好**被提交一次——不许重复提交"
    );
}

#[test]
fn a_failing_peripheral_does_not_drop_the_writes_already_collected() {
    // V-B 检视 · F-02：M-12 的注释明写「**已经收到的写一条都不丢**」，但
    // **没有用例守**——现有的「报错的外设不牵连别人」用例里 ticker 返回的
    // 是 `Ok(vec![])`，根本不涉及写。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    engine.register_peripheral(0x2000);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    engine.set_peripheral_ticker(PeripheralTicker::new(move |paddr, _now_ps| {
        sink.borrow_mut().push(paddr);
        if paddr == 0x2000 {
            return Err(PlatformError::new(
                riscv_sim_contract::error::ErrorFamily::Peripheral,
                1,
                "boom",
            ));
        }
        Ok(vec![PendingWrite {
            paddr: 0xF000,
            size: WriteSize::Word,
            value: 0x00AB_CDEF,
            source: "d07-test".to_string(),
        }])
    }));

    let err = engine
        .tick_peripherals(42)
        .expect_err("第二个外设报错必须报出来");
    assert_eq!(
        err.family,
        riscv_sim_contract::error::ErrorFamily::Peripheral
    );

    let queued = engine.snapshot().pending_writes;
    assert_eq!(
        queued.len(),
        1,
        "报错的外设不许牵连别人——**已经收下的写一条都不丢**：{queued:?}"
    );
    assert_eq!(queued[0].value, 0x00AB_CDEF);
}

#[test]
fn the_next_quantum_boundary_is_strictly_after_now() {
    // V-B 检视 · F-04：外设 tick 事件必须排在**下一个**量子边界——排在
    // 「当前」边界上会让外设在 t=0 就被 tick（提前一整个量子），而现有
    // 用例只查「落在量子边界上」，抓不到「早了整整一个量子」。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    assert_eq!(engine.now_ps(), 0);
    assert_eq!(
        engine.next_quantum_boundary(),
        TIME_QUANTUM_PS,
        "下一个量子边界必须**严格在**当前时刻之后"
    );
    engine.advance_virtual_time(TIME_QUANTUM_PS);
    assert_eq!(
        engine.next_quantum_boundary(),
        2 * TIME_QUANTUM_PS,
        "已经站在边界上时，下一个边界是**再往后**一个"
    );
}

#[test]
fn the_instruction_budget_is_never_exceeded_even_inside_a_quantum() {
    // V-B 检视 · F-07：「预算在**指令边界**上查」这条不变式此前只是
    // **偶然**被守着的——真正抓住它的是兄弟用例的副作用。
    //
    // 口径：预算是**硬上限**。一个量子内的指令条数可能超过「预算里还剩
    // 多少」——所以预算必须在**指令边界**上查，不能只在量子边界查。
    //
    // 判别式：把预算设成**不是配额整数倍**的数（1 500）——「只在量子
    // 边界查」的实现会超支 500 条。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.set_instruction_budget(1_500);
    let mut executor = CountingExecutor::new();
    let outcome = engine
        .run_until(&mut executor, u64::MAX)
        .expect("run until");
    assert_eq!(
        outcome.instructions, 1_500,
        "预算必须**恰好**用满，一条都不许超"
    );
    assert_eq!(
        outcome.reason,
        riscv_sim_kernel::m12_engine::RunReason::BudgetExhausted,
        "预算耗尽要**如实**报出来"
    );
}

#[test]
fn swapping_the_memory_map_leaves_only_the_new_peripherals_registered() {
    // V-B 检视 · F-10：外设登记表的生命周期无守卫——「换内存图」（= 换
    // 工程 = 换外设集合）之后，登记表必须只含**新**外设；旧基址留下来
    // 的话，主循环会一直去 tick 一个**没有主人**的地址。
    let (periph, _ticks, _pending) = RecordingPeripheral::spawn(PERIPH_BASE, PERIPH_SIZE);
    let mut bus = Bus::new();
    bus.map_ram(RAM_BASE, RAM_SIZE, &nop_program())
        .expect("map ram");
    bus.map_peripheral_instance(PERIPH_BASE, PERIPH_SIZE, periph)
        .expect("map peripheral");
    let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
    backend.install_peripheral_ticker();
    assert_eq!(
        backend.engine().registered_peripherals(),
        &[PERIPH_BASE][..],
        "装上 ticker 之后外设必须被登记"
    );

    backend.reset_memory_map();

    assert!(
        backend.engine().registered_peripherals().is_empty(),
        "换内存图 = 换外设集合——旧基址不许留在登记表里：{:?}",
        backend.engine().registered_peripherals()
    );
}

#[test]
fn each_peripheral_is_ticked_by_its_own_address() {
    // V-B 检视 · F-11：生产 ticker 闭包的**按地址分派**无守卫——闭包若
    // 忽略 `paddr`（永远 tick 第一个实例），全仓用例照样全绿：生产只挂
    // 1 个外设，夹具也只挂 1 个。
    let base_a = PERIPH_BASE;
    let base_b = PERIPH_BASE + PERIPH_SIZE;
    let (a, a_ticks, _a_pending) = RecordingPeripheral::spawn(base_a, PERIPH_SIZE);
    let (b, b_ticks, _b_pending) = RecordingPeripheral::spawn(base_b, PERIPH_SIZE);
    let mut bus = Bus::new();
    bus.map_ram(RAM_BASE, RAM_SIZE, &nop_program())
        .expect("map ram");
    bus.map_peripheral_instance(base_a, PERIPH_SIZE, a)
        .expect("map a");
    bus.map_peripheral_instance(base_b, PERIPH_SIZE, b)
        .expect("map b");
    let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
    backend.install_peripheral_ticker();

    backend.step_n(1).expect("step");

    assert_eq!(
        a_ticks.borrow().len(),
        1,
        "第一个外设必须被**它自己的地址**tick"
    );
    assert_eq!(
        b_ticks.borrow().len(),
        1,
        "第二个外设必须被**它自己的地址**tick"
    );
}

#[test]
fn the_peripheral_clock_keeps_running_after_a_snapshot_restore() {
    // V-B 检视：**快照恢复 × 时间驱动**的交互此前**无任何用例**——D-07
    // 把 `m12_m17_snapshot_dto.rs` 那条用例的驱动方式从
    // `continue_until_event` 改成 `run_until_idle` 之后，这一格就空了。
    //
    // 口径：外设登记表与 ticker 接线属于**平台**，不属于**运行状态**——
    // 恢复运行状态**不该**把它们清掉（清掉 = 外设的时钟从此停摆）。
    let mut engine: Engine<NullSinks, NullSinks> = Engine::default();
    engine.register_peripheral(0x1000);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    engine.set_peripheral_ticker(PeripheralTicker::new(move |_paddr, now_ps| {
        sink.borrow_mut().push(now_ps);
        Ok(Vec::new())
    }));

    let mut executor = CountingExecutor::new();
    engine.run_until(&mut executor, u64::MAX).expect("run");
    let before = seen.borrow().len();
    assert!(before > 0, "恢复之前外设必须被 tick 过");

    let snap = engine.snapshot_full();
    engine.restore_full(&snap);
    assert_eq!(
        engine.registered_peripherals(),
        &[0x1000u32][..],
        "恢复不该清掉外设登记表"
    );

    let mut executor2 = CountingExecutor::new();
    engine.run_until(&mut executor2, u64::MAX).expect("run");
    assert!(seen.borrow().len() > before, "恢复之后外设的时钟必须继续走");
}

#[test]
fn a_stop_reason_does_not_leak_into_the_next_batch() {
    // V-B 检视 · F-12：停止原因**跨轮残留**会让宿主误报「又停了」。
    // `step_n` 与 `continue_until_event` 都必须在**每批次开头**清空
    // `last_stop`——上一批的断点不代表这一批也断了。
    let (mut backend, _ticks, _pending) = backend_with_peripheral(&nop_program());
    backend
        .set_breakpoint_with_id(Breakpoint {
            id: 1,
            addr: 0,
            source_hint: None,
            enabled: true,
            condition: None,
        })
        .expect("set breakpoint");
    backend.step_n(1).expect("step");
    assert_eq!(backend.last_stop_reason(), Some(HaltReason::Breakpoint));

    backend.clear_breakpoint_by_id(1).expect("clear breakpoint");
    backend.step_n(1).expect("step");
    assert_eq!(
        backend.last_stop_reason(),
        None,
        "上一批的停止原因不许残留到下一批——否则宿主会误报 Halted"
    );
}
