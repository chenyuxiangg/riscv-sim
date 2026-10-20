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
use riscv_sim_kernel::m14_bus::{Bus, BusWidth};
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
}

impl RecordingPeripheral {
    fn spawn(base: u32, size: u32) -> (Rc<RefCell<dyn Peripheral>>, TickLog, PendingLog) {
        let ticks = Rc::new(RefCell::new(Vec::new()));
        let pending = Rc::new(RefCell::new(Vec::new()));
        let me = Rc::new(RefCell::new(Self {
            id: "REC0".to_string(),
            base,
            size,
            ticks: Rc::clone(&ticks),
            pending: Rc::clone(&pending),
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
        Ok(self.pending.borrow().clone())
    }
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

/// 造一个后端：低段 RAM + 一个会记账的外设。
fn backend_with_peripheral(
    program: &[u8],
) -> (SelfIssBackend<NullSinks, NullSinks>, TickLog, PendingLog) {
    let (periph, ticks, pending) = RecordingPeripheral::spawn(PERIPH_BASE, PERIPH_SIZE);
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
    assert_eq!(
        TIME_QUANTUM_PS,
        u64::from(riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA)
            * riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS,
        "量子 = 每量子指令数 × 单条指令成本"
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
    // **事件驱动**的 tick 必须是**额外**的那几次：每条指令前同步一次是
    // 「指令驱动」，量子边界的 `PeripheralTick` 事件是「时间驱动」。
    // 只剩一种驱动时，tick 的条数就会正好等于指令数——判别式因此要写成
    // 「**严格大于**」，而不是「大于等于」。
    assert!(
        seen.len() as u64 > backend.total_executed(),
        "事件驱动的 tick 必须是额外的那几次：ticks={} executed={}",
        seen.len(),
        backend.total_executed()
    );
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
    assert_eq!(
        outcome.quanta,
        u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET)
            / u64::from(riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA),
        "量子数 = 预算 / 每量子配额"
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
    // 「预算 / 配额」多一个。这正是「事件边界 = 抢占点」的可观测形状。
    let full = u64::from(riscv_sim_kernel::m12_engine::DEFAULT_INSTRUCTION_BUDGET)
        / u64::from(riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA);
    assert!(
        outcome.quanta > full,
        "量子内部到期的事件必须劈开量子：quanta={} full={full}",
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
