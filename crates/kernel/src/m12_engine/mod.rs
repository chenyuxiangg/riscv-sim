//! M-12 事件驱动仿真引擎 (Event-Driven Simulation Engine).
//!
//! 实现量子化混合调度主循环（AD ADR-1，LLD §4.12）与确定性事件队列内核。
//! 本模块的三条硬约束（对应跨模块欠账 D-05 / D-06 / D-17）：
//!
//! - **时间单一真相**：虚拟时钟只存在于 [`SimState::virtual_time_ps`]，
//!   引擎不再保留第二份 `now_ps`。因此 `restore` 天然把时间一并还原
//!   （D-05），也不存在「引擎时钟与快照时钟不一致」的可能。
//! - **推进不空转**：推进时直接跳到下一个到期事件，而不是逐个量子空转
//!   （M-12 §3.3「空闲区间批量推进」）。1 秒虚拟跨度的事件序列只耗
//!   O(事件数) 步，而不是 O(时间/量子) 步（D-06）。
//! - **序号不撞号**：插入序 `seq` 是引擎的**独立单调计数器**，不与事件
//!   队列的内容耦合；恢复时会把计数器顶到队列已占用值之上，因此恢复后
//!   继续 `schedule` 不会与队列中已有事件撞号（D-17）。
//! - **停滞会被抓住**：唯一主循环带**无进展守卫**（LLD §3.5「watchdog
//!   无进展检测」的引擎侧落点）—— 连续 [`NO_PROGRESS_WINDOW`] 次「既没有
//!   指令退休、虚拟时间也没有前进、也没有到期事件被消费」⇒ **报错退出**，
//!   而不是无限自旋（V-D 检视 · VD-F3）。
//!
//! 快照有三种形态：[`Engine::snapshot`] 返回契约级 [`SimState`]（M-17
//! 交给宿主的那一份，签名保持不变）；[`Engine::snapshot_full`] 返回
//! [`EngineSnapshot`]，额外携带待处理事件队列与插入序计数器 —— **只有
//! 它能精确还原引擎**（D-05）；[`Engine::snapshot_dto`] 返回契约层的
//! 可序列化镜像 [`EngineSnapshotDto`]，让确定性的快照**落得了盘**
//! （D-24 根因修复）。
//!
//! TODO(MVP): 把调度器接到 M-20 快照落盘路径上；调度内核（时间推进 /
//! 事件队列 / 稳定排序 / 快照还原）与**唯一主循环**（[`Engine::run_until`]，
//! D-07）已经可用。
//!
//! ## 指令驱动与时间驱动如何交替（D-07）
//!
//! - **时间驱动是骨架**：事件队列 + 量子边界（1 µs）决定「什么时候
//!   派发事件、什么时候推进外设时钟」；
//! - **指令驱动是量子内的填充物**：一个量子内最多执行
//!   [`QUANTUM_INSTR_QUOTA`] 条指令；
//! - 二者在**指令边界**交汇：每条指令之后检查一次「有没有事件到期」，
//!   到期就抢占（安全检查点 = 量子边界 ∨ 指令边界 ∨ 事件边界）。
//!
//! 指令本身的执行通过 [`InstructionExecutor`] 接缝注入 —— M-12 **不**
//! 依赖 M-13，依赖方向是单向的（M-17 实现该接缝）。

use std::fmt;

use riscv_sim_contract::csr::csr_is_implemented;
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, SimState};
use riscv_sim_contract::port::{EventBus, InterruptSink};
use riscv_sim_contract::protocol::{EventPayload, HaltReason};

mod queue;
mod snapshot;

pub use queue::{EventQueue, QueuedEvent};

/// 时间量子（皮秒）—— **1 µs**（LLD §2.1 `quantum`，默认 1 µs，可配置）。
///
/// ⚠️ **口径订正（D-07）**：1 µs = **1 000 000 ps**。历史实现写的是
/// `1_000`（1 ns），注释却写「1 µs」—— 差了 1000 倍，而且与「每量子
/// 1000 条指令」的自洽换算对不上。现在这三个常量构成一个闭合的换算：
///
/// ```text
/// TIME_QUANTUM_PS = QUANTUM_INSTR_QUOTA × INSTRUCTION_COST_PS
///                 = 1 000 × 1 000 ps = 1 000 000 ps = 1 µs
/// ```
///
/// 量子长短的**可观测后果**：量子边沿是「外设时钟同步」与「事件派发」的
/// 节奏；量子从 1 ns 变成 1 µs 之后，跨一个量子要 1000 条指令，而不是 1 条。
pub const TIME_QUANTUM_PS: u64 = 1_000_000;

/// 每量子的指令配额（LLD §2.1 `quantumInstrQuota`，默认 **1000 条/量子**）。
///
/// 量子 ↔ 指令的映射（评审 12-1）：一个量子内最多执行这么多条指令，
/// 之后必须回到安全检查点（派发到期事件 / 采样 IRQ / 让出宿主）。
pub const QUANTUM_INSTR_QUOTA: u32 = 1_000;

/// 单条指令的虚拟时间成本（皮秒）= 量子 / 每量子指令配额 = **1 ns**。
///
/// 与 M-17 的 `INSTRUCTION_COST_PS` 同值 —— 后者现在转调本常量，
/// 避免「同一个换算在两处各写一遍」。
pub const INSTRUCTION_COST_PS: u64 = TIME_QUANTUM_PS / QUANTUM_INSTR_QUOTA as u64;

/// Default guest-instruction budget before the engine yields to the host.
///
/// ⚠️ **它不是 watchdog**（V-D 检视 · **VD-F3** 的注释订正）：它是**一轮
/// 运行窗口**的指令预算 —— 引擎跑满这么多条 guest 指令就让出给宿主
/// （[`RunReason::BudgetExhausted`]）。LLD §2.1 把它叫「run-window 指令
/// 预算」；LLD §3.5 的 **watchdog（无进展检测）**是**另一套**机制，由
/// [`Engine::run_until`] 里的 [`NO_PROGRESS_WINDOW`] 守卫实现。
///
/// ⚠️ 旧注释写的是「watchdog pre-check（M-12 §5.2 + **LLD §4.12.6**）」——
/// 那个章节**在 LLD 文本里根本不存在**（`4.12` 出现 0 次），而它描述的
/// 机制也**没有**在这里实现 ⇒ 一条**假陈述**（V-D 实测确认）。
///
/// The value lives in the contract so that a default-constructed
/// [`riscv_sim_contract::snapshot::EngineSnapshotDto`] and a fresh engine
/// agree on it — one source of truth, no drift.
pub const DEFAULT_INSTRUCTION_BUDGET: u32 =
    riscv_sim_contract::snapshot::DEFAULT_INSTRUCTION_BUDGET;

/// **无进展检测窗口**（LLD §3.5「watchdog 无进展检测」的引擎侧实现）。
///
/// 主循环里**连续**这么多次「既没有指令退休、虚拟时间也没有前进、也没有
/// 到期事件被消费」⇒ 判定**停滞**（活锁 / guest 失控）⇒ [`Engine::run_until`]
/// **报错退出**（`Err`），而不是**无限自旋**。
///
/// ⚠️ **口径**：LLD 的 `progressWindow` 默认 10⁶，单位是**指令**；这里的
/// 单位是**「无进展的迭代」**——而停滞时**一条指令都没退休**，两者不可直接
/// 换算。取 **1024** 的理由：
///
/// - **要小**：活锁必须在**有限步内**被抓到（否则整个测试二进制挂死，
///   而且生产路径上这就是一个「永不返回的主循环」）；
/// - **要大**：合法路径上「一次中断交付 → handler 的第一条指令退休」只
///   产生**一次**无进展迭代 ⇒ 1024 留了**三个数量级**的余量，不会误伤。
///
/// ⚠️ LLD §3.5 的三类「外部可见进展」（事件分发 / 中断交付 / MMIO 访问）
/// 在本引擎里的落点：**事件分发** = [`Engine::dispatch_due_events`] 派发了
/// 至少一件事件；**指令退休 / 虚拟时间前进** = 执行器真的往前走了；
/// **中断交付**本身**不算**进展 —— 一条被反复交付、从不退休的中断正是
/// VD-F3 的活锁形态，把它算成进展就等于**把活锁放过去**。
pub const NO_PROGRESS_WINDOW: u32 = 1_024;

/// ⚠️ **场景码住在契约层**（v0.24 · D-120 · 冻结规格 STAGE-9 裁决 G）
/// —— M-12 **不再自写一份**字面量。
///
/// ⚠️ **为什么是 `pub use` 而不是删掉**：这三个常量**曾经**是 M-12 的
/// **公开 API**（`riscv_sim_kernel::m12_engine::E_ENG_03_NO_PROGRESS`）
/// ⇒ 删掉它们会打断外部引用。`pub use` 让那条路径**继续存在**，而
/// **值**来自契约层 ⇒ 全仓**只有一个定义处**（契约层的 `error.rs`）。
///
/// ⚠️ **依赖方向是对的**：内核 crate 依赖契约 crate（内核 → 契约）
/// ⇒ **不构成循环依赖**。
pub use riscv_sim_contract::error::{
    E_ENG_01_BUS_PUBLISH_FAILED, E_ENG_02_QUEUE_INCONSISTENT, E_ENG_03_NO_PROGRESS,
};

/// Scheduling event held in the event queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedEvent {
    /// A peripheral asks to be ticked at the given virtual time.
    PeripheralTick { paddr: u32, at_ps: u64 },
    /// A timer expiry (M-15 internal timers).
    TimerExpiry { irq: u16, at_ps: u64 },
    /// A pending memory write to commit at the next quantum boundary.
    Commit(PendingWrite),
}

impl Default for SchedEvent {
    fn default() -> Self {
        Self::TimerExpiry { irq: 0, at_ps: 0 }
    }
}

/// 外设 tick 的回调端口（M-12 ↔ M-14/M-15 的接缝，见 D-07）。
///
/// 契约层目前没有「按物理地址找到外设实例并 tick 它」的端口（总线侧接线
/// 由并行工作流负责，接口未定）。为了让 M-12 的 `PeripheralTick` 分支
/// 结构完整且可测，这里在 **m12 内部**定义一个回调式端口：
///
/// ```text
/// tick(paddr, now_ps) -> Result<Vec<PendingWrite>, PlatformError>
/// ```
///
/// 语义与契约的 [`riscv_sim_contract::port::Peripheral::tick`] 一致：返回
/// 本次 tick 期间外设希望提交到总线上的写。引擎把这些写并入
/// [`SimState::pending_writes`]，与 [`SchedEvent::Commit`] 走同一条提交
/// 路径。
///
/// 期望的最终形态（需协调者决策，见交付报告）：要么在契约层新增
/// `PeripheralTickSink` 端口并由 M-14 的总线实现，要么保留本回调端口、
/// 由 M-17 在装配时把总线的外设注册表接进来。
pub struct PeripheralTicker {
    tick: Box<dyn FnMut(u32, u64) -> Result<Vec<PendingWrite>, PlatformError>>,
}

impl PeripheralTicker {
    /// 构造一个 tick 回调。
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut(u32, u64) -> Result<Vec<PendingWrite>, PlatformError> + 'static,
    {
        Self { tick: Box::new(f) }
    }

    /// 触发一次外设 tick。
    pub fn tick(&mut self, paddr: u32, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        (self.tick)(paddr, now_ps)
    }
}

impl fmt::Debug for PeripheralTicker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PeripheralTicker")
    }
}

/// 一条 guest 指令的执行结果（M-12 视角）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecOutcome {
    /// 指令正常退休（PC 已前进）。
    Advanced,
    /// **交付了一个中断**（**没有**指令退休）—— 继续跑，不要计入已执行条数。
    ///
    /// 中断交付发生在**取指边界**（M-17 的执行器）：M-16 仲裁出待交付的
    /// 中断 ⇒ 陷入（`mepc` / `mcause` / `mtval` 写好，PC 跳到 `mtvec`）。
    /// 那条被中断的指令**没有执行**，虚拟时钟也**没有**推进。
    ///
    /// ⚠️ **为什么必须有这个变体**（STAGE-4 裁决 I）：若复用 [`Self::Advanced`]，
    /// 主循环会把它**计成一条已执行指令**（`instructions += 1`）→「已执行条数」
    /// 每交付一次中断就多一条 —— 一个**说不出口的偏差**。中断是「透明」的：
    /// 交付**不停批次**，handler 紧接着在同一条指令边界后开始执行。
    InterruptTaken,
    /// 需要停下：断点 / 陷阱 / 观察点 / 宿主暂停。
    ///
    /// 停止原因用契约的 [`HaltReason`] 表达 —— 那是跨模块唯一的「为什么停」
    /// 词汇表（契约冻结，不能另起一套）。
    Stop {
        /// 为什么停。
        reason: HaltReason,
        /// 这条指令是否**已经被执行**。
        ///
        /// 断点命中 = **没有**执行（命中在取指之前，状态零副作用）；
        /// 陷阱 = 执行了（被取指、被译码、被触发）。两条路径的「已执行
        /// 条数」因此不同，主循环要照实计数。
        consumed: bool,
    },
}

/// 「执行一条 guest 指令」的端口（M-12 ↔ M-13 的接缝）。
///
/// **为什么不是契约端口**：契约层冻结，新增端口要等 `schema_version = 2`
/// （台账 D-32）。而 LLD §2.3 明确要求主循环里要有 `iss_step(s)` 这一步，
/// 所以 M-12 必须能「驱动一条指令」—— 但 M-12 不该知道 M-13 的
/// `StepOutcome`（那是 M-13 的内部词汇表）。
///
/// 于是 M-12 在**自己内部**定义这个 trait（与 [`PeripheralTicker`] 同一
/// 套路：模块内部的回调式接缝），由 M-17 实现：
///
/// ```text
/// M-12（调度器）──executes──▶ M-17（后端）──drives──▶ M-13（ISS）
/// ```
///
/// 依赖方向单向：M-17 依赖 M-12 与 M-13，M-12 **不依赖** M-13。
/// 这样 M-12 的单元测试可以拿一个假的执行器喂它，完全不需要 ISS。
pub trait InstructionExecutor {
    /// 执行**一条** guest 指令。
    ///
    /// 实现方负责：断点判定（取指之前）、执行本身、观察点判定（访存发生
    /// 在指令内部，只能在指令返回后判），以及**推进虚拟时钟**。
    fn step(&mut self, state: &mut SimState) -> Result<ExecOutcome, PlatformError>;
}

/// 唯一主循环的停止原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunReason {
    /// 到达目标虚拟时间（LLD `runUntil(t)` 的 `TIME_REACHED`）。
    TimeReached,
    /// 本轮指令预算耗尽 —— 让出给宿主（安全检查点）。
    BudgetExhausted,
    /// 被断点 / 陷阱 / 观察点 / 宿主暂停打断。
    Stopped(HaltReason),
}

/// 一轮主循环的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    /// 本轮实际执行的指令条数。
    pub instructions: u64,
    /// 本轮经过的**安全检查点**数（= 主循环外层的轮数）。
    ///
    /// ⚠️ **口径**（V-A 检视 · F9）：它计的是「外层循环转了几圈」，
    /// **不是**「虚拟时间跨过了几个量子」。一轮 = **派发到期事件** +
    /// **至多一个量子**的指令；被**事件抢占**的量子会算成两轮
    /// （事件边界 = 抢占点 = 额外的安全检查点）。
    ///
    /// 于是「没有抢占」时它恰好等于「跨过的量子数」，「有抢占」时它
    /// **大于**量子数 —— 这正是「事件边界 = 抢占点」的可观测形状。
    pub quanta: u64,
    /// 为什么停。
    pub reason: RunReason,
}

/// 引擎的**完整**快照 —— 足以把引擎逐位还原。
///
/// [`SimState`] 里装不下事件队列（契约冻结，不能扩字段），所以队列、
/// 队列顺序与插入序计数器都由本结构承载（D-05）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineSnapshot {
    /// 平台状态：寄存器 / PC / CSR / 待提交写 / 虚拟时间。
    pub state: SimState,
    /// 待处理事件，**按规范出队序**（`at_ps` 升序、同刻按 `seq` 升序）
    /// 排列 —— 与逐条出队顺序一致。
    pub queue: Vec<QueuedEvent>,
    /// 下一个事件将获得的插入序（单调计数器）。
    pub next_seq: u64,
    /// 本次运行的每量子指令预算。
    pub instruction_budget: u32,
}

/// 一次「派发到期事件」的**结果**（内部结构体）。
///
/// ⚠️ 存在的理由只有一个：**「派发了多少件事件」是「外部可见进展」的判据**
/// （LLD §3.5 的 progressBase 更新点 ①）—— 没有它，主循环**分不清**
/// 「这一轮推进了」与「这一轮原地打转」，watchdog 的守卫也就无从谈起
/// （V-D 检视 · **VD-F3**）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct DueDispatch {
    /// 本次派发的事件条数（`> 0` ⇒ 有外部可见进展）。
    dispatched: usize,
    /// 本次派发的**外设 tick** 事件（`paddr`, 到期时刻）—— 调用方据此
    /// **续排**下一个周期。
    reticked: Vec<(u32, u64)>,
}

/// The event-driven engine itself.
#[derive(Debug)]
pub struct Engine<B: EventBus, I: InterruptSink> {
    /// Pending events, time-ordered by `(at_ps, seq)`.
    queue: EventQueue,
    /// Instruction budget for the current **run window** — the engine
    /// executes at most this many guest instructions per
    /// [`Engine::run_until`] call, then yields to the host
    /// ([`RunReason::BudgetExhausted`]).
    ///
    /// ⚠️ **它不是 watchdog**（V-D 检视 · **VD-F3** 的注释订正）：旧注释
    /// 写的是「watchdog pre-check（M-12 §5.2 + **LLD §4.12.6**）」——
    /// 那个章节**在 LLD 文本里不存在**（`4.12` 出现 0 次），而且这里
    /// 描述的机制**确实已经接进主循环**（`run_until` 在**指令边界**上查
    /// 预算），只是它**不是** watchdog。watchdog 是**另一套**机制：
    /// [`NO_PROGRESS_WINDOW`] 的**无进展检测**（LLD §3.5）。
    instruction_budget: u32,
    /// Bus / event / interrupt sinks.
    bus: B,
    intc: I,
    /// Platform state. **The virtual clock lives here**
    /// (`state.virtual_time_ps`) — the engine keeps no second copy of the
    /// time, so `restore` restores the clock for free (D-05).
    state: SimState,
    /// Monotonic insertion counter for the stable-sort tiebreaker.
    ///
    /// Kept as an engine field rather than derived from the queue: the
    /// counter must never be handed out twice, and it must survive a
    /// restore of a queue that is not part of [`SimState`] (D-17).
    next_seq: u64,
    /// Number of clock advances performed since the engine was created.
    ///
    /// Diagnostic only — it counts *steps*, not virtual time, and is
    /// deliberately not part of the snapshot (it is a metric, not state).
    advance_steps: u64,
    /// Optional peripheral-tick seam (D-07).
    ticker: Option<PeripheralTicker>,
    /// **已登记的外设**基址（D-07）。
    ///
    /// 登记表回答的是「时钟推进要推进谁」：M-12 自己不知道总线上挂了
    /// 哪些外设（那是 M-14/M-15 的事），但它必须能在**指令边界**逐个
    /// 推进它们的外设时钟——否则探针边沿的时间戳恒为 0（D-46）。
    ///
    /// 登记是**装配期**的一次性动作（谁把外设挂到总线上，谁来登记），
    /// 不是运行期的动态增删。
    peripherals: Vec<u32>,
}

impl<B: EventBus, I: InterruptSink> Engine<B, I> {
    /// Construct a fresh engine.
    pub fn new(bus: B, intc: I) -> Self {
        Self {
            queue: EventQueue::new(),
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
            bus,
            intc,
            state: SimState::default(),
            next_seq: 0,
            advance_steps: 0,
            ticker: None,
            peripherals: Vec::new(),
        }
    }

    /// Reset the engine to its **entry state**.
    ///
    /// Clears the pending event queue, the platform state (registers, PC,
    /// CSRs, pending writes, virtual clock), the insertion counter and the
    /// step counter, and restores the default instruction budget — i.e.
    /// the engine ends up in the state [`Engine::new`] would have
    /// produced.
    ///
    /// The injected dependencies (event bus, interrupt sink), the
    /// peripheral-tick seam ([`Engine::set_peripheral_ticker`]) and the
    /// peripheral registry ([`Engine::register_peripheral`]) are
    /// **wiring**, not simulation state: they are deliberately left in
    /// place, because silently dropping the ticker would stop peripherals
    /// from ever being ticked again.
    pub fn reset(&mut self) {
        self.queue = EventQueue::new();
        self.state = SimState::default();
        self.next_seq = 0;
        self.advance_steps = 0;
        self.instruction_budget = DEFAULT_INSTRUCTION_BUDGET;
    }

    /// Borrow the injected interrupt sink (read-only).
    ///
    /// The interrupt sink is an injected dependency, not engine state — it
    /// is deliberately **not** part of any snapshot. Exposing it read-only
    /// lets a host observe which IRQ lines the engine raised without
    /// letting it mutate the engine.
    pub fn intc(&self) -> &I {
        &self.intc
    }

    /// Current virtual time in picoseconds.
    ///
    /// Single-sourced from [`SimState::virtual_time_ps`] — the engine has
    /// no second copy of the clock (D-05).
    pub fn now_ps(&self) -> u64 {
        self.state.virtual_time_ps
    }

    /// Current program counter.
    ///
    /// Cheap accessor for the hot path (the execution backend checks it
    /// once per instruction) — [`Engine::snapshot`] would clone the
    /// whole platform state just to read one word.
    pub fn pc(&self) -> u32 {
        self.state.pc
    }

    /// Borrow the platform state **mutably**.
    ///
    /// The engine is the **single owner** of the platform state (the
    /// virtual clock included — see [`Engine::now_ps`]). This accessor
    /// exists so the execution backend (M-17) can drive the ISS (M-13)
    /// against *that* state instead of keeping a second copy of it.
    ///
    /// Borrowing (rather than handing out an owned clone) is what keeps
    /// the "one source of truth" property: a clone would let the ISS
    /// advance a register file the engine never sees.
    ///
    /// The caller is expected to write back through the same borrow; the
    /// engine re-reads the state on every snapshot / restore, so no
    /// explicit "commit" step is needed.
    pub fn state_mut(&mut self) -> &mut SimState {
        &mut self.state
    }

    /// Advance the virtual clock by `delta_ps` picoseconds.
    ///
    /// The clock lives in [`SimState::virtual_time_ps`] and nowhere else
    /// (D-05). Advancing it here — instead of letting every caller keep
    /// its own `now_ps` — is what makes a snapshot restore the clock for
    /// free.
    ///
    /// Saturation instead of wrapping: a clock that wraps around would
    /// make the event queue's ordering meaningless.
    pub fn advance_virtual_time(&mut self, delta_ps: u64) {
        self.state.virtual_time_ps = self.state.virtual_time_ps.saturating_add(delta_ps);
    }

    /// Number of clock advances performed since the engine was created.
    ///
    /// Because the engine batch-advances across idle intervals, this is
    /// *not* the number of quanta — see [`Engine::run_until_idle`].
    pub fn advance_steps(&self) -> u64 {
        self.advance_steps
    }

    /// Number of pending (not yet dispatched) events.
    pub fn pending_event_count(&self) -> usize {
        self.queue.len()
    }

    /// The insertion sequence number the next scheduled event will get.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Instruction budget for the current quantum.
    pub fn instruction_budget(&self) -> u32 {
        self.instruction_budget
    }

    /// Set the instruction budget for the current quantum.
    pub fn set_instruction_budget(&mut self, budget: u32) {
        self.instruction_budget = budget;
    }

    /// Install the peripheral-tick seam (D-07).
    ///
    /// The callback receives the physical address of the peripheral and the
    /// current virtual time, and returns the writes the peripheral wants
    /// committed. Until the bus side (M-14/M-15) exposes a registry, this
    /// is how a [`SchedEvent::PeripheralTick`] reaches a peripheral.
    pub fn set_peripheral_ticker(&mut self, ticker: PeripheralTicker) {
        self.ticker = Some(ticker);
    }

    /// 登记一个**可被时钟推进**的外设（D-07）。
    ///
    /// 登记回答的是「推进谁」：M-12 不知道总线上挂了哪些外设，但外设的
    /// 时钟必须由虚拟时间推进，否则探针边沿的时间戳恒为 0（D-46）。
    ///
    /// **幂等**：同一个基址登记两次只算一次。
    pub fn register_peripheral(&mut self, paddr: u32) {
        if !self.peripherals.contains(&paddr) {
            self.peripherals.push(paddr);
        }
    }

    /// 已登记的外设基址（只读）。
    pub fn registered_peripherals(&self) -> &[u32] {
        &self.peripherals
    }

    /// 清空外设登记表（D-07）。
    ///
    /// 换工程 = 换内存图 = 换外设集合（M-17 的 `reset_memory_map`）。
    /// 旧的外设基址如果留在表里，主循环会**徒劳地**去 tick 一个已经不
    /// 存在的地址 —— 每次都是一次空转，而且登记表会越积越多。
    pub fn clear_peripherals(&mut self) {
        self.peripherals.clear();
    }

    /// 下一个量子边界的虚拟时间。
    ///
    /// 量子边界 = 时间的整数倍 [`TIME_QUANTUM_PS`]。当前时刻本身若已经
    /// 落在边界上，返回的是**下一个**边界（边界是「刚过」，不是「就在」）。
    pub fn next_quantum_boundary(&self) -> u64 {
        let now = self.now_ps();
        // 饱和算术：虚拟时间接近 `u64::MAX` 时（狂跑几个世纪），
        // `now / QUANTUM + 1` 再乘回去会**溢出**——debug 构建直接 panic。
        (now / TIME_QUANTUM_PS)
            .saturating_add(1)
            .saturating_mul(TIME_QUANTUM_PS)
            .max(now)
    }

    /// **推进所有已登记外设的时钟**（D-07）——外设时钟的唯一同步点。
    ///
    /// 每个外设的 [`Peripheral::tick`] 都会被调用一次，返回的写并入
    /// [`SimState::pending_writes`]（与 [`SchedEvent::Commit`] 同一条提交
    /// 路径）。
    ///
    /// 粒度由调用方决定：生产代码在**每条指令之前**调用它（精细时间戳，
    /// D-46），主循环在**量子边界**也会通过
    /// [`SchedEvent::PeripheralTick`] 触发一次（契约口径「每量子一次」）。
    /// 两次调用的时间戳是单调的——量子边界必然**不早于**最后一次指令
    /// 边界的同步。
    ///
    /// 返回本次推进的外设个数。
    ///
    /// [`Peripheral::tick`]: riscv_sim_contract::port::Peripheral::tick
    pub fn tick_peripherals(&mut self, now_ps: u64) -> Result<usize, PlatformError> {
        self.tick_peripherals_except(now_ps, &[])
    }

    /// 同 [`Engine::tick_peripherals`]，但**跳过** `skip` 里的外设。
    ///
    /// 存在的理由：量子边界的 [`SchedEvent::PeripheralTick`] 事件**已经**
    /// 在当前时刻推进过这些外设了，紧接着的第一条指令**不该**再推进一次
    /// ——那会让同一个外设在**同一虚拟时刻**被 tick 两次，而两条驱动路径
    /// （`step_n` 与 `run_until`）的外设时钟会因此**对不上**
    /// （V-A 检视 · F1）。
    ///
    /// 跳过之后，两条路径的外设推进序列**逐条相同**：每条指令推进一次，
    /// 量子边界那一次由事件代劳。
    pub fn tick_peripherals_except(
        &mut self,
        now_ps: u64,
        skip: &[u32],
    ) -> Result<usize, PlatformError> {
        let Self {
            ticker,
            peripherals,
            state,
            ..
        } = self;
        let Some(ticker) = ticker.as_mut() else {
            return Ok(0);
        };
        let mut ticked = 0usize;
        let mut first_error: Option<PlatformError> = None;
        for &paddr in peripherals.iter() {
            if skip.contains(&paddr) {
                continue;
            }
            match ticker.tick(paddr, now_ps) {
                Ok(writes) => {
                    state.pending_writes.extend(writes);
                    ticked += 1;
                }
                Err(err) => {
                    // 报错的外设**不许牵连别人**：其余外设**照样 tick**
                    // ——时钟同步不能被别人拖停（D-46 · V-A-4）。记下
                    // **第一个**错误，继续把剩下的走完，最后再抛。
                    //
                    // 已经收下的写**一条不丢**（它们已经在
                    // `state.pending_writes` 里）。
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }
        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(ticked)
    }

    /// Schedule an event for the given virtual time.
    pub fn schedule(&mut self, at_ps: u64, event: SchedEvent) {
        let _ = self.schedule_event(at_ps, event);
    }

    /// Schedule an event and return its insertion sequence number.
    ///
    /// The `seq` is the stable-sort tiebreaker for events that share a
    /// virtual time, and doubles as the cancellation token (M-12 §3.2).
    ///
    /// **Late events are normalized, never dropped** (评审 12-6): an event
    /// scheduled for a time already in the past is moved to the current
    /// virtual time, keeping its insertion order.
    pub fn schedule_event(&mut self, at_ps: u64, event: SchedEvent) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let at_ps = at_ps.max(self.now_ps());
        self.queue.push(QueuedEvent { at_ps, seq, event });
        self.sync_seq_mirror();
        seq
    }

    /// Run the engine until the queue is empty or a host pause is
    /// requested. Returns the number of **time quanta consumed**.
    ///
    /// The unit is virtual time, not loop iterations: the engine
    /// batch-advances across idle intervals (M-12 §3.3), so a 1-second
    /// virtual span costs a handful of steps while still reporting
    /// 1 000 000 quanta. Use [`Engine::advance_steps`] for the step
    /// count.
    ///
    /// Each step: advance the virtual clock to the next due event, publish
    /// a [`EventPayload::Tick`], then dispatch every event that is due at
    /// the new time. An event scheduled exactly at the current time is
    /// dispatched by the step that reaches it — dispatch never happens
    /// late, because the clock lands exactly on the event time.
    ///
    /// ⚠️ **它不执行 guest 指令**——这是「纯事件推进」的低层原语
    /// （把队列排空）。**生产路径走 [`Engine::run_until`]**（LLD §3.1 的
    /// 唯一主循环：派发事件 + 量子内执行指令 + 在事件边界抢占）。
    /// 本方法保留是因为它是**公开 API**（只增不减），也是「快照还原后把
    /// 队列跑干净」这类场景的合适工具。
    pub fn run_until_idle(&mut self) -> Result<u64, PlatformError> {
        self.run_steps(u64::MAX)
    }

    /// Run at most `max_steps` advance steps. Returns the number of time
    /// quanta consumed (same unit as [`Engine::run_until_idle`]).
    ///
    /// A step is one clock advance + the dispatch of the events that fall
    /// due at the new time. This is the granularity the D-23 determinism
    /// assertion uses ("snapshot → 继续推进 N 步").
    pub fn run_steps(&mut self, max_steps: u64) -> Result<u64, PlatformError> {
        let start_ps = self.now_ps();
        let mut steps = 0u64;
        while steps < max_steps {
            // Idle interval (M-12 §3.1 / §3.3): the guest has nothing to
            // do until the next event is due, so the virtual clock jumps
            // straight to that event instead of stepping through empty
            // quanta. This is what keeps a long virtual span at
            // O(events) steps instead of O(time / quantum) (D-06).
            let Some(next) = self.queue.peek() else {
                break;
            };
            let target = next.at_ps;
            if target > self.now_ps() {
                self.state.virtual_time_ps = target;
            }

            self.bus
                .publish(EventPayload::Tick {
                    elapsed_ps: self.now_ps(),
                    pc: self.state.pc,
                })
                .map_err(|e| {
                    PlatformError::new(
                        ErrorFamily::Engine,
                        E_ENG_01_BUS_PUBLISH_FAILED,
                        e.to_string(),
                    )
                })?;
            steps += 1;
            self.advance_steps = self.advance_steps.wrapping_add(1);

            // Dispatch every event that is due at the current time.
            //
            // ⚠️ 这里**不续排**周期性的外设 tick（`dispatch_due_events`
            // 的返回值被丢弃）：本方法的语义是「把队列排空」，而续排会
            // 让队列永远不空 → 死循环。续排只发生在唯一主循环
            // `Engine::run_until` 里（D-07）。
            self.dispatch_due_events()?;
        }
        Ok(quanta_between(start_ps, self.now_ps()))
    }

    fn dispatch(&mut self, event: SchedEvent) -> Result<(), PlatformError> {
        match event {
            SchedEvent::PeripheralTick { paddr, at_ps: _ } => {
                // D-07：**真正**调用外设 tick 接缝。
                //
                // 时间戳用**当前虚拟时钟**而不是事件自带的 `at_ps`：事件
                // 可能被**迟到**派发（LLD 评审 12-6 的归一化），而外设
                // 记下的时间必须是它「实际被推进的那一刻」——否则探针
                // 边沿会带着一个未来的时间戳。
                let now = self.now_ps();
                let writes = match self.ticker.as_mut() {
                    Some(ticker) => ticker.tick(paddr, now)?,
                    None => Vec::new(),
                };
                self.state.pending_writes.extend(writes);
                Ok(())
            }
            SchedEvent::TimerExpiry { irq, at_ps: _ } => self.intc.raise(irq),
            SchedEvent::Commit(write) => {
                // Commit happens at the next bus access; for the
                // skeleton we just stash it on the state.
                self.state.pending_writes.push(write);
                Ok(())
            }
        }
    }

    /// 派发**当前时刻已到期**的全部事件。
    ///
    /// 返回本次派发的**结果**：派发了多少件事件（= **外部可见进展**的
    /// 判据，LLD §3.5 的 progressBase 更新点 ①）与本次派发的外设 tick
    /// 事件（`paddr`, 到期时刻）—— 调用方（唯一主循环 [`Engine::run_until`]）
    /// 据此**续排**下一个周期，从而让「外设时钟」这条时间驱动链路自持。
    ///
    /// ⚠️ **续排不在这里做**：`run_until_idle` / `run_steps` 语义是
    /// 「把队列排空」，如果派发即续排，队列**永远不会空**，这两个方法
    /// 会死循环。周期性的续排只发生在唯一主循环里。
    fn dispatch_due_events(&mut self) -> Result<DueDispatch, PlatformError> {
        let mut reticked = Vec::new();
        let mut dispatched = 0usize;
        while let Some(t) = self.queue.peek().map(|e| e.at_ps) {
            if t > self.now_ps() {
                break;
            }
            // `peek` said there is one — but `pop` is a separate
            // call, and a panic here would take the whole WASM
            // worker down (WASM 里 panic = Worker 崩). Fail loudly
            // with an error instead (V-A 检视 · ⚪-4)。
            let evt = self.queue.pop().ok_or_else(|| {
                PlatformError::new(
                    ErrorFamily::Engine,
                    E_ENG_02_QUEUE_INCONSISTENT,
                    "事件队列在 peek 与 pop 之间不一致",
                )
            })?;
            if let SchedEvent::PeripheralTick { paddr, .. } = &evt.event {
                reticked.push((*paddr, evt.at_ps));
            }
            dispatched += 1;
            self.dispatch(evt.event)?;
        }
        Ok(DueDispatch {
            dispatched,
            reticked,
        })
    }

    /// 队列里是否有**此刻已经到期**的事件（事件边界 = 抢占点）。
    fn has_due_event(&self) -> bool {
        self.queue.peek().is_some_and(|e| e.at_ps <= self.now_ps())
    }

    /// **确保**每个已登记的外设都有一件待派发的 tick 事件（D-07）。
    ///
    /// 这是时间驱动链路的**自愈**部分：主循环保证每个登记的外设都有一
    /// 次 tick 在**下一个量子边界**等着，因此事件即使被弄丢（快照恢复
    /// 之后、外设被换掉之后），外设的时钟也不会永久停摆。
    fn ensure_peripheral_ticks(&mut self) {
        let missing: Vec<u32> = self
            .peripherals
            .iter()
            .copied()
            .filter(|paddr| {
                !self.queue.iter().any(|e| {
                    matches!(e.event, SchedEvent::PeripheralTick { paddr: q, .. } if q == *paddr)
                })
            })
            .collect();
        if missing.is_empty() {
            return;
        }
        let at = self.next_quantum_boundary();
        for paddr in missing {
            self.schedule_event(at, SchedEvent::PeripheralTick { paddr, at_ps: at });
        }
    }

    /// **唯一主循环**（LLD §3.1 `runUntil`）——量子化混合调度。
    ///
    /// 一轮循环：
    ///
    /// 1. 派发**到期事件**（外设 tick / 定时器到期 / 提交）；
    /// 2. 在本量子内执行**至多 [`QUANTUM_INSTR_QUOTA`] 条**指令；
    /// 3. 每条指令之后检查**事件边界**——有事件到期就立刻回到第 1 步
    ///    （安全检查点 = 量子边界 ∨ 指令边界 ∨ 事件边界，LLD §2.3）；
    /// 4. 量子结束回到第 1 步。
    ///
    /// 停止条件（任一命中即返回）：
    ///
    /// - 虚拟时间到达 `until_ps`（[`RunReason::TimeReached`]）；
    /// - 本轮指令预算（[`Engine::instruction_budget`]）耗尽
    ///   （[`RunReason::BudgetExhausted`]）——让出给宿主；
    /// - 执行器请求停止（断点 / 陷阱 / 观察点 / 暂停，
    ///   [`RunReason::Stopped`]）；
    /// - **无进展守卫触发**（[`NO_PROGRESS_WINDOW`]）⇒ 返回 **`Err`**
    ///   （`E_ENG-03`）——**不是** [`RunOutcome`] 的一种停止原因：
    ///   「停滞」是**平台故障**（活锁 / guest 失控），不是 guest 的正常
    ///   停止语义（LLD §3.5 · 评审 12-2 / 12-3：watchdog 与 guest trap
    ///   **严格区分**）。
    ///
    /// **指令驱动与时间驱动如何交替**（D-07 的核心问题）：时间驱动是
    /// **骨架**（事件队列 + 量子边界），指令驱动是量子内的**填充物**。
    /// 二者在**指令边界**交汇——每执行一条指令就问一次「有没有事件到期」，
    /// 到期就抢占、派发、继续。没有事件时，guest 一直有指令可执行，
    /// 时间随指令推进；没有指令时（`wfi` 之类）时间才会被事件推进。
    ///
    /// 与 [`Engine::run_until_idle`] 的分工：后者是「把队列排空」的一次性
    /// 工具（不执行指令、不维持周期），本方法是生产路径。
    pub fn run_until<E: InstructionExecutor>(
        &mut self,
        executor: &mut E,
        until_ps: u64,
    ) -> Result<RunOutcome, PlatformError> {
        let mut instructions = 0u64;
        let mut quanta = 0u64;
        let budget = u64::from(self.instruction_budget);
        // **无进展计数**（LLD §3.5 的 watchdog · V-D 检视 VD-F3）：连续
        // 多少次迭代「既没有指令退休、虚拟时间也没有前进、也没有到期事件
        // 被消费」。它是**唯一**能抓住「活锁」的东西 —— 见
        // [`NO_PROGRESS_WINDOW`]。
        let mut no_progress = 0u32;
        loop {
            if self.now_ps() >= until_ps {
                return Ok(RunOutcome {
                    instructions,
                    quanta,
                    reason: RunReason::TimeReached,
                });
            }

            // (1) 派发到期事件，并把外设 tick **续排**到下一个量子边界
            //     ——时间驱动链路自持（D-07）。
            //
            //     续排之前先「确保」每个登记的外设都有一件 tick 在等着：
            //     事件被弄丢时（快照恢复、外设被换掉）链路会自愈。
            self.ensure_peripheral_ticks();
            let dispatched = self.dispatch_due_events()?;
            // 派发了到期事件 ⇒ **外部可见进展**（LLD §3.5 的 progressBase
            // 更新点 ①）⇒ 无进展计数归零。
            if dispatched.dispatched > 0 {
                no_progress = 0;
            }
            for (paddr, at_ps) in &dispatched.reticked {
                // ⚠️ **陈旧事件不续排**（V-A 检视 · F6）：登记表被清空
                // （换工程 = 换外设集合）之后，队列里还留着旧基址的 tick
                // 事件——续排它就等于让一件**永远等不到主人**的事件永远
                // 留在队列里。派发一次就够了（ticker 找不到实例 → 空转）。
                if !self.peripherals.contains(paddr) {
                    continue;
                }
                let next = at_ps.saturating_add(TIME_QUANTUM_PS);
                self.schedule_event(
                    next,
                    SchedEvent::PeripheralTick {
                        paddr: *paddr,
                        at_ps: next,
                    },
                );
            }
            // 刚刚被**事件**推进过的外设，在**同一时刻**不该再被指令边界
            // 推进一次（V-A 检视 · F1）——那会让同一外设在同一虚拟时刻被
            // tick 两次，两条驱动路径的外设时钟也就对不上。
            //
            // 只对**本量子的第一条**指令生效：第二条指令的时刻已经
            // 往前走了 1 ns，必须照常推进。
            let mut just_ticked: Vec<u32> = dispatched
                .reticked
                .iter()
                .map(|(paddr, _)| *paddr)
                .collect();

            // (2) 量子内执行指令。
            let mut done = 0u32;
            while done < QUANTUM_INSTR_QUOTA {
                // **不冲过**目标时间（V-A 检视 · F5）：执行一条指令会把
                // 时钟往前推 `INSTRUCTION_COST_PS`，所以要**先预检**——
                // 只在「已经到达」时才停，会让 `run_until(t)` 落到 `t`
                // 之后最多一条指令的距离上（`run_until(4500)` → 5000）。
                if self.now_ps().saturating_add(INSTRUCTION_COST_PS) > until_ps {
                    break;
                }
                // 指令预算也要在**指令边界**上查（安全检查点 = 量子边界
                // ∨ 指令边界 ∨ 事件边界）。只在量子边界查的话，一次
                // **抢占**会把量子劈成两半，于是这一轮会**超预算**最多
                // 一个量子——「预算」就不再是预算了。
                if instructions >= budget {
                    break;
                }
                // 外设时钟同步（**指令边界**）——精细时间戳（D-46）。
                let now = self.now_ps();
                self.tick_peripherals_except(now, &just_ticked)?;
                just_ticked.clear();
                // 记下**执行前**的虚拟时间：执行器**可能**推进它（正规实现
                // 在「中断交付」时不推进，但守卫不能依赖那条约定）。
                let before_ps = self.now_ps();
                match executor.step(self.state_mut())? {
                    ExecOutcome::Advanced => {
                        instructions += 1;
                        done += 1;
                        // 有指令退休 ⇒ 有进展 ⇒ 无进展计数归零。
                        no_progress = 0;
                    }
                    ExecOutcome::InterruptTaken => {
                        // 中断交付**没有**指令退休 ⇒ 既不计入 `instructions`
                        // （「已执行条数」必须说真话），也**不**计入本量子的
                        // 配额 `done`，更**不是**停止原因 —— 中断是「透明」的：
                        // handler 紧接着在下一条指令边界后开始执行。
                        //
                        // ⚠️ **这里曾经写着「不会空转」，那个论证是错的**
                        // （V-D 检视 · **VD-F3**）：它依赖「M-16 的 `active_irq`
                        // 单级防护（未清除前不再交付）」这条**不变式**——
                        // 而**不变式破掉时没有任何守卫**：三个记账全不动
                        // （不增 `instructions`、不增 `done`、不推进虚拟时间）
                        // ⇒ 内层循环**永不退出**。V-D 实测：8 秒内执行器被调用
                        // **334 956 736 次**，`run_until` 永不返回。
                        //
                        // ⇒ 现在由**无进展守卫**（LLD §3.5 的 watchdog）兜底：
                        // 连续 [`NO_PROGRESS_WINDOW`] 次「既没有指令退休、
                        // 虚拟时间也没有前进、也没有到期事件被消费」⇒ **报错
                        // 退出**。
                        if self.now_ps() != before_ps {
                            // 虚拟时间前进了 ⇒ 有进展。
                            no_progress = 0;
                        } else {
                            no_progress = no_progress.saturating_add(1);
                            if no_progress >= NO_PROGRESS_WINDOW {
                                return Err(PlatformError::new(
                                    ErrorFamily::Engine,
                                    E_ENG_03_NO_PROGRESS,
                                    format!(
                                        "主循环无进展：连续 {no_progress} 次既没有指令退休、\
                                         虚拟时间也没有前进、也没有到期事件被消费 \
                                         （watchdog 窗口 {NO_PROGRESS_WINDOW}）—— \
                                         判定为停滞并中止"
                                    ),
                                ));
                            }
                        }
                    }
                    ExecOutcome::Stop { reason, consumed } => {
                        if consumed {
                            instructions += 1;
                        }
                        return Ok(RunOutcome {
                            instructions,
                            quanta,
                            reason: RunReason::Stopped(reason),
                        });
                    }
                }
                // 事件边界 = 抢占点：有事件到期就回到 (1)。
                if self.has_due_event() {
                    break;
                }
            }
            quanta += 1;

            // (3) 安全检查点：指令预算耗尽 → 让出给宿主。
            if instructions >= budget {
                return Ok(RunOutcome {
                    instructions,
                    quanta,
                    reason: RunReason::BudgetExhausted,
                });
            }

            // (4) ⚠️ **不许空转**（V-A 检视 · F5 闭环）：内层循环一条指令
            //     都没执行 → 只可能是「`now + 指令成本 > until_ps`」把时间
            //     预检挡住了（预算耗尽的情况上面已经返回了）。
            //
            //     不在这里收手，外层就会**永远转下去**：时间到头了 → 内层
            //     一条也不执行 → 外层再转一圈还是同样的结果 → **死循环**。
            //     （这是 F5 的预检式修法带来的副作用——原版 `now >= until_ps`
            //     总能靠「多执行一条」把时间推过头，侥幸不死。）
            if done == 0 {
                return Ok(RunOutcome {
                    instructions,
                    quanta,
                    reason: RunReason::TimeReached,
                });
            }
        }
    }

    /// Snapshot the deterministic engine state (M-20 §4.20).
    ///
    /// Returns the contract-level [`SimState`]. It carries the platform
    /// state, the virtual time and the insertion counter — but **not** the
    /// event queue, which the contract has no room for. Use
    /// [`Engine::snapshot_full`] when the queue has to be restored too.
    pub fn snapshot(&self) -> SimState {
        let mut state = self.state.clone();
        self.sync_state_seq(&mut state);
        state
    }

    /// Restore from a snapshot.
    ///
    /// **Partial restore**: the platform state — including the virtual
    /// clock ([`SimState::virtual_time_ps`]) — is restored, but the event
    /// queue is *not* part of [`SimState`] and therefore stays as it is.
    ///
    /// **This is not a deterministic round-trip.** Events queued *after*
    /// the snapshot was taken are still pending and will still fire, so
    /// a run after a restore can diverge from an uninterrupted run with
    /// the same seed. Concretely: schedule A, snapshot, schedule B,
    /// restore, run — both A and B fire, while an uninterrupted run
    /// from the same seed would only fire A.
    ///
    /// Use [`Engine::restore_full`] together with
    /// [`Engine::snapshot_full`] whenever determinism is required: that
    /// pair round-trips the queue contents, the queue order, the
    /// insertion counter and the virtual time (D-05).
    /// [`Engine::snapshot`] / [`Engine::restore`] are the contract-level
    /// pair, and the contract has no room for the queue.
    ///
    /// The insertion counter is taken from the restored state and then
    /// raised above every `seq` still queued, so scheduling after a
    /// restore can never collide with a queued event (D-17).
    pub fn restore(&mut self, state: SimState) {
        self.state = state;
        drop_unimplemented_csrs(&mut self.state);
        let floor = self.queue.max_seq().map_or(0, |seq| seq.saturating_add(1));
        self.next_seq = self.state.event_queue_head.max(floor);
        self.sync_seq_mirror();
    }

    /// Snapshot the engine *including* the pending event queue.
    ///
    /// This is the form that can be restored exactly: queue contents, queue
    /// order, the insertion counter and the virtual time (D-05).
    ///
    /// ## ⚠️ 队列里的**迟到事件会被归一化**（V-D 检视 · **VD-F1**）
    ///
    /// 契约层的 [`riscv_sim_contract::snapshot::EngineSnapshotDto::validate`]
    /// 的**检查 7** 拒绝「队首事件的到期时刻**早于**快照的虚拟时间」——
    /// 理由是「一件在快照时刻**已经到期**的事件，还原后会**立刻**触发，
    /// 那就不是『T 时刻的状态』了」。
    ///
    /// ⚠️ 契约层那条注释里写着「**引擎不会产出这种快照**」——**那是错的**
    /// （V-D 实测确认）：虚拟时钟会随指令退休往前冲，而事件只在**量子
    /// 边界**派发 ⇒ 只要有一件事件的到期时刻落在两个派发点之间，队列里
    /// 就会留下一件「**已到期未派发**」的事件 ⇒ 引擎**自己**产出的快照
    /// **过不了自己的校验器** ⇒ `capture_session → restore_session` 与
    /// `M23::put_snapshot` 一起失败。
    ///
    /// ⇒ 修法落在**引擎侧**（**不放宽契约层的校验器** —— 契约层本批不
    /// 升版）：出快照时把**迟到**事件的到期时刻**拉到当前时刻**
    /// （`max(at_ps, now)`）—— 这正是 [`Engine::schedule_event`] **入队
    /// 时**已经在用的那条归一化规则（评审 12-6「迟到事件归一为当前
    /// 时间」），契约层 [`riscv_sim_contract::snapshot::QueuedEventDto::at_ps`]
    /// 的文档也**已经**这么写了（「it is normalised: a late event is pulled
    /// forward to `max(at_ps, now)`」）—— 只是引擎**出快照时**漏了这一步。
    ///
    /// ⇒ 于是**引擎产出的快照永远能过自己的校验器**。
    ///
    /// ⚠️ **口径**：归一化**不改变事件何时触发** —— 一件已到期的事件在
    /// 活着的引擎里本来就会在**下一次派发**时立刻触发（那一次派发就在
    /// 当前时刻），归一化只是把「它到期于 5000 ps」这句话换成「它到期于
    /// **当前**时刻」，而**当前时刻正是它本来就会被触发的那一刻**。
    ///
    /// ⚠️ **一处已知的次序代价**：若队列里有**两件以上**迟到事件，且它们
    /// 的 `seq` 次序与到期时刻次序**相反**，归一化会把它们拉到同一个时刻
    /// ⇒ 规范序（`(at_ps, seq)`）按 `seq` 排 ⇒ **出队次序可能与活着的
    /// 引擎不同**。这是「同一时刻多件事件」这种边角情形的代价；相比之下
    /// 「引擎产出的快照**根本落不了盘**」是更大的问题。
    pub fn snapshot_full(&self) -> EngineSnapshot {
        let mut state = self.state.clone();
        self.sync_state_seq(&mut state);
        let now = state.virtual_time_ps;
        let mut queue = self.queue.to_ordered_vec();
        // 归一化「已到期未派发」的事件（VD-F1）：把它们拉到当前时刻 ⇒
        // 队首事件的到期时刻**不再早于**快照的虚拟时间 ⇒ 契约的检查 7
        // 通过。
        let mut clamped = false;
        for event in &mut queue {
            if event.at_ps < now {
                event.at_ps = now;
                clamped = true;
            }
        }
        if clamped {
            // 归一化可能打乱规范序（`at_ps` 变了）⇒ 重新排一次。
            // `(at_ps, seq)` 是**全序**（队列里 `seq` 互不相同）⇒ 排完
            // 一定是规范序。
            queue.sort_by_key(|e| e.sort_key());
        }
        EngineSnapshot {
            state,
            queue,
            next_seq: self.next_seq,
            instruction_budget: self.instruction_budget,
        }
    }

    /// Restore an engine from a full snapshot.
    ///
    /// Restores the queue contents *and* their order, the insertion
    /// counter and the virtual time, so that continuing to run afterwards
    /// yields exactly the same event sequence and the same results as
    /// never having restored (D-05 / FR-12).
    pub fn restore_full(&mut self, snapshot: &EngineSnapshot) {
        self.state = snapshot.state.clone();
        // ⚠️ **过滤 CSR 文件**（台账 **D-101 ①** · 验收 **C-4**）。
        //
        // 快照是**外部**来的 ⇒ 里面可能有平台**没实现**的 CSR（`0x7A1`
        // 之类）。平台的规则是「**未实现 ⇒ 写被忽略、读回 0**」（契约层
        // `csr_write_is_ignored`）—— **整体克隆** CSR 文件会**绕过**那条
        // 规则：外部构造的快照里塞一个 `0x7A1`，guest 就能读出非 0 ⇒
        // 凭空多出一个平台根本没有的寄存器。
        //
        // ⇒ 恢复时按契约层的「已实现 CSR 表」过滤：**未实现的 CSR 不写**
        // （不写 ⇒ 读回 0，与「写被忽略」的效果一致）。
        drop_unimplemented_csrs(&mut self.state);
        self.queue = EventQueue::from_ordered_vec(snapshot.queue.clone());
        self.next_seq = snapshot.next_seq;
        self.instruction_budget = snapshot.instruction_budget;
        // Defensive: a hand-built snapshot could name a `next_seq` that is
        // already taken by a queued event. For a consistent snapshot this
        // is a no-op (`next_seq` is always above every queued `seq`).
        //
        // ⚠️ **它不是死代码**（V-D 检视 · **VD-F2** 的结论）：本方法收的是
        // **内部类型** [`EngineSnapshot`] —— 字段全公开，**外部可以手工
        // 构造**一份 `next_seq` **小于**队列里最大 `seq` 的快照 ⇒ 没有这条
        // 修正，恢复后第一次 `schedule` 就会**撞号**（D-17）。
        //
        // ⚠️ 走契约 DTO 那条路（[`Engine::restore_dto`]）时，契约的
        // `validate()` 已经**先**查过「`next_seq` 严格大于每个排队
        // `seq`」（检查 5）⇒ 那条路上它确实是 no-op —— 但**两条路不是
        // 同一件事**：一条是「不可信存储」，一条是「公开 API 的入参」。
        // 覆盖见用例 `restoring_a_hand_built_snapshot_raises_the_insertion_counter_above_the_queue`。
        let floor = self.queue.max_seq().map_or(0, |seq| seq.saturating_add(1));
        if self.next_seq < floor {
            self.next_seq = floor;
        }
        self.sync_seq_mirror();
    }

    /// Keep [`SimState::event_queue_head`] in sync with the insertion
    /// counter.
    ///
    /// `event_queue_head` is **not** a pointer into the event queue — the
    /// queue is not representable in the contract. It is the monotonic
    /// insertion counter: the stable-sort tiebreaker the *next*
    /// [`Engine::schedule`] call will assign. Keeping it in the contract
    /// state is what lets a partial [`Engine::restore`] continue the
    /// sequence without reusing a `seq` (D-17).
    fn sync_seq_mirror(&mut self) {
        self.state.event_queue_head = self.next_seq;
    }

    /// Same as [`Engine::sync_seq_mirror`], for a state value that is not
    /// owned by the engine (snapshot copy).
    fn sync_state_seq(&self, state: &mut SimState) {
        state.event_queue_head = self.next_seq;
    }
}

impl<B: EventBus, I: InterruptSink> Default for Engine<B, I>
where
    B: Default,
    I: Default,
{
    fn default() -> Self {
        Self::new(B::default(), I::default())
    }
}

/// Drop the CSRs the platform **does not implement** from a CSR file that
/// came from **outside**（台账 **D-101 ①** · 验收 **C-4**）。
///
/// The platform's rule is "**unimplemented ⇒ the write is ignored and a
/// later read returns 0**"（the contract layer's
/// `csr_write_is_ignored`）—— cloning the CSR file wholesale **bypasses**
/// that rule: a hand-built snapshot carrying `0x7A1` would let the guest
/// read back a register the platform does not have.
///
/// ⇒ The restore paths filter the file through the contract's
/// "**implemented CSR table**": an unimplemented CSR is **not written**
/// (not written ⇒ it reads back 0, exactly like an ignored write).
///
/// ⚠️ The filter is **not** applied to the platform's own writes: the
/// guest's write path already refuses them at the source.
fn drop_unimplemented_csrs(state: &mut SimState) {
    state.csrs.retain(|(number, _)| csr_is_implemented(*number));
}

/// Number of time quanta spanned by the half-open interval
/// `(from_ps, to_ps]`.
fn quanta_between(from_ps: u64, to_ps: u64) -> u64 {
    if to_ps <= from_ps {
        return 0;
    }
    (to_ps / TIME_QUANTUM_PS) - (from_ps / TIME_QUANTUM_PS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal in-memory event bus for tests.
    #[derive(Default)]
    struct TestBus {
        count: u32,
    }
    impl EventBus for TestBus {
        fn publish(&mut self, _: EventPayload) -> Result<(), PlatformError> {
            self.count += 1;
            Ok(())
        }
    }
    impl InterruptSink for TestBus {
        fn raise(&mut self, _: u16) -> Result<(), PlatformError> {
            Ok(())
        }
        fn lower(&mut self, _: u16) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    /// Event bus that records the tick trace, for determinism assertions.
    #[derive(Debug, Default, Clone)]
    struct RecordingBus {
        ticks: Vec<(u64, u32)>,
    }
    impl EventBus for RecordingBus {
        fn publish(&mut self, evt: EventPayload) -> Result<(), PlatformError> {
            if let EventPayload::Tick { elapsed_ps, pc } = evt {
                self.ticks.push((elapsed_ps, pc));
            }
            Ok(())
        }
    }

    /// Interrupt sink that records the raised IRQ lines.
    #[derive(Debug, Default, Clone)]
    struct RecordingIntc {
        raised: Vec<u16>,
        lowered: Vec<u16>,
    }
    impl InterruptSink for RecordingIntc {
        fn raise(&mut self, irq: u16) -> Result<(), PlatformError> {
            self.raised.push(irq);
            Ok(())
        }
        fn lower(&mut self, irq: u16) -> Result<(), PlatformError> {
            self.lowered.push(irq);
            Ok(())
        }
    }

    fn timer(irq: u16, at_ps: u64) -> SchedEvent {
        SchedEvent::TimerExpiry { irq, at_ps }
    }

    /// Run to idle and return the trace segment produced by *this* run.
    ///
    /// The event bus and the interrupt sink are injected dependencies, not
    /// part of the engine snapshot, so their traces keep accumulating
    /// across a restore. Comparing per-run segments is what "the same run
    /// happened twice" actually means.
    fn drain_run(eng: &mut Engine<RecordingBus, RecordingIntc>) -> (Vec<(u64, u32)>, Vec<u16>) {
        let tick_start = eng.bus.ticks.len();
        let irq_start = eng.intc.raised.len();
        eng.run_until_idle().expect("run");
        (
            eng.bus.ticks[tick_start..].to_vec(),
            eng.intc.raised[irq_start..].to_vec(),
        )
    }

    /// Run to idle and return only the IRQs raised by *this* run.
    fn drain_raised(eng: &mut Engine<RecordingBus, RecordingIntc>) -> Vec<u16> {
        drain_run(eng).1
    }

    // ------------------------------------------------------------------
    // Baseline
    // ------------------------------------------------------------------

    #[test]
    fn engine_advances_time() {
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        eng.schedule(
            2 * TIME_QUANTUM_PS,
            SchedEvent::TimerExpiry { irq: 1, at_ps: 0 },
        );
        let quanta = eng.run_until_idle().expect("run");
        assert_eq!(quanta, 2);
        assert!(eng.now_ps() >= 2 * TIME_QUANTUM_PS);
    }

    // ------------------------------------------------------------------
    // reset — the engine must really return to its entry state
    // ------------------------------------------------------------------

    #[test]
    fn engine_reset_clears_the_queue_the_clock_and_the_counter() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(5 * TIME_QUANTUM_PS, timer(1, 5 * TIME_QUANTUM_PS));
        eng.run_until_idle().expect("run");
        assert!(eng.now_ps() > 0);
        assert_eq!(eng.next_seq(), 1);

        eng.reset();

        assert_eq!(eng.pending_event_count(), 0, "队列必须被清空");
        assert_eq!(eng.now_ps(), 0, "虚拟时钟必须回到 0");
        assert_eq!(eng.next_seq(), 0, "插入序计数器必须归零");
        assert_eq!(eng.advance_steps(), 0, "步数计数必须归零");
        assert_eq!(
            eng.instruction_budget(),
            DEFAULT_INSTRUCTION_BUDGET,
            "指令预算必须回到默认值"
        );
        assert_eq!(
            eng.snapshot(),
            SimState::default(),
            "reset 后的快照必须与全新引擎的快照逐字相同"
        );
    }

    #[test]
    fn engine_reset_keeps_the_peripheral_ticker_wired() {
        // The ticker is *wiring*, not simulation state: dropping it on
        // reset would silently stop peripherals from ever being ticked.
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = seen.clone();
        eng.set_peripheral_ticker(PeripheralTicker::new(move |paddr, at_ps| {
            sink.borrow_mut().push((paddr, at_ps));
            Ok(Vec::new())
        }));

        eng.reset();
        eng.schedule(
            700,
            SchedEvent::PeripheralTick {
                paddr: 0x4000_0000,
                at_ps: 700,
            },
        );
        eng.run_until_idle().expect("run");

        assert_eq!(
            *seen.borrow(),
            vec![(0x4000_0000, 700)],
            "reset 之后 ticker 仍必须被触发"
        );
    }

    // ------------------------------------------------------------------
    // D-05 — snapshot / restore completeness
    // ------------------------------------------------------------------

    #[test]
    fn restore_restores_virtual_time() {
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        eng.schedule(5 * TIME_QUANTUM_PS, timer(1, 5 * TIME_QUANTUM_PS));
        eng.run_until_idle().expect("run");
        assert_eq!(eng.now_ps(), 5 * TIME_QUANTUM_PS);

        let snap = eng.snapshot();
        eng.schedule(9 * TIME_QUANTUM_PS, timer(2, 9 * TIME_QUANTUM_PS));
        eng.run_until_idle().expect("run");
        assert_eq!(eng.now_ps(), 9 * TIME_QUANTUM_PS);

        // The clock is stored in the state, so a restore brings it back.
        eng.restore(snap);
        assert_eq!(eng.now_ps(), 5 * TIME_QUANTUM_PS);
    }

    #[test]
    fn snapshot_full_restores_queue_and_order() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(1_000, timer(1, 1_000));
        eng.schedule(2_000, timer(2, 2_000));
        eng.schedule(3_000, timer(3, 3_000));

        let snap = eng.snapshot_full();
        assert_eq!(snap.queue.len(), 3);
        // Canonical order: (at_ps, seq) ascending.
        assert_eq!(
            snap.queue.iter().map(|e| e.at_ps).collect::<Vec<_>>(),
            vec![1_000, 2_000, 3_000]
        );

        eng.run_until_idle().expect("run");
        assert_eq!(eng.pending_event_count(), 0);

        eng.restore_full(&snap);
        assert_eq!(eng.pending_event_count(), 3);
        assert_eq!(eng.now_ps(), 0);
        assert_eq!(eng.next_seq(), snap.next_seq);

        // The restored queue dispatches in exactly the original order.
        assert_eq!(drain_raised(&mut eng), vec![1, 2, 3]);
        assert_eq!(eng.now_ps(), 3_000);
    }

    #[test]
    fn restore_full_resumes_at_the_snapshot_time() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(1_000, timer(1, 1_000));
        eng.schedule(4_000, timer(2, 4_000));
        eng.run_steps(1).expect("run");
        assert_eq!(eng.now_ps(), 1_000);

        let snap = eng.snapshot_full();
        eng.run_until_idle().expect("run");
        assert_eq!(eng.now_ps(), 4_000);

        eng.restore_full(&snap);
        assert_eq!(eng.now_ps(), 1_000);
        assert_eq!(eng.pending_event_count(), 1);
        eng.run_until_idle().expect("run");
        assert_eq!(eng.now_ps(), 4_000);
    }

    // ------------------------------------------------------------------
    // D-101 ① — the restore path must not bypass "unimplemented ⇒ 0"
    // ------------------------------------------------------------------

    /// **恢复一份含未实现 CSR 的快照 ⇒ 那条 CSR 读回 0**（台账
    /// **D-101 ①** · 验收 **C-4**）。
    ///
    /// The platform's rule is "**unimplemented ⇒ the write is ignored and
    /// a later read returns 0**". The restore path used to clone the CSR
    /// file wholesale ⇒ an **externally built** snapshot carrying `0x7A1`
    /// would hand the guest a register the platform does not have.
    ///
    /// ⚠️ The assertion lands on the **file**, not on a value: "the CSR is
    /// **not in the file**" is what makes the guest read 0 (M-13's rule:
    /// a CSR that is not in the file reads 0 and does not trap). Asserting
    /// "the value is 0" alone would be **vacuous** — an absent CSR reads 0
    /// too.
    ///
    /// ⚠️ The **implemented** CSRs must survive — a filter that dropped
    /// everything would make this assertion pass.
    #[test]
    fn restoring_a_snapshot_drops_the_csrs_the_platform_does_not_implement() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut snap = eng.snapshot_full();
        // 一份**外部构造**的快照：一个平台**没实现**的 CSR（`tdata1`）与
        // 一个**已实现**的（`mstatus`）。
        snap.state.csrs.push((0x300, 0x0000_0088));
        snap.state.csrs.push((0x7A1, 0x7A1_0000));

        eng.restore_full(&snap);

        assert!(
            !eng.state.csrs.iter().any(|(number, _)| *number == 0x7A1),
            "未实现的 CSR 不得被恢复 —— 实得 {:?}",
            eng.state.csrs
        );
        assert_eq!(
            eng.state
                .csrs
                .iter()
                .find(|(number, _)| *number == 0x300)
                .map(|(_, value)| *value),
            Some(0x0000_0088),
            "已实现的 CSR 必须活下来（过滤不能把整个文件清空）"
        );
    }

    /// **部分恢复**（[`Engine::restore`]）走**同一条**过滤规则。
    ///
    /// ⚠️ 两条恢复路径面对的是同一类外部数据 ⇒ 只堵一条等于没堵。
    #[test]
    fn restoring_a_partial_state_drops_the_csrs_the_platform_does_not_implement() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut state = SimState::default();
        state.csrs.push((0x7A1, 0x7A1_0000));

        eng.restore(state);

        assert!(
            !eng.state.csrs.iter().any(|(number, _)| *number == 0x7A1),
            "未实现的 CSR 不得被恢复 —— 实得 {:?}",
            eng.state.csrs
        );
    }

    // ------------------------------------------------------------------
    // D-06 — no idling through empty quanta
    // ------------------------------------------------------------------

    #[test]
    fn single_far_future_event_costs_one_step() {
        // The regression this guards: the old skeleton advanced one
        // 1 µs quantum per event, so an event at t = 1e12 ps burned
        // 1e9 iterations.
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(1_000_000_000_000, timer(1, 1_000_000_000_000));

        let before = eng.advance_steps();
        let quanta = eng.run_until_idle().expect("run");
        let steps = eng.advance_steps() - before;

        assert_eq!(steps, 1, "a far-future event must not burn quanta");
        // 量子是 1 µs（D-07 订正），所以 1e12 ps 跨 1e6 个量子。
        // ⚠️ 这里**按常量**写期望值，而不是写死数字——量子的口径变了，
        // 这条用例的**意图**（「远期事件不能烧掉一堆量子」）不受影响。
        assert_eq!(quanta, 1_000_000_000_000 / TIME_QUANTUM_PS);
        assert_eq!(eng.now_ps(), 1_000_000_000_000);
        assert_eq!(eng.bus.ticks.len(), 1);
    }

    #[test]
    fn long_span_advance_is_event_bounded() {
        // Performance baseline: a 1-second virtual span (1e12 ps) covered
        // by 64 events must cost O(events) steps, not O(time / quantum).
        const SPAN_PS: u64 = 1_000_000_000_000;
        const EVENTS: u64 = 64;

        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        for i in 0..EVENTS {
            let at = SPAN_PS * i / (EVENTS - 1);
            eng.schedule(at, timer((i % 8) as u16, at));
        }

        let before = eng.advance_steps();
        let quanta = eng.run_until_idle().expect("run");
        let steps = eng.advance_steps() - before;

        // Explicit upper bound: one step per event, no more.
        assert_eq!(steps, EVENTS, "advance steps must be O(events)");
        assert!(
            steps < SPAN_PS / TIME_QUANTUM_PS,
            "steps ({steps}) must stay far below the quantum count ({})",
            SPAN_PS / TIME_QUANTUM_PS
        );
        // The virtual span is still fully covered.
        assert_eq!(quanta, SPAN_PS / TIME_QUANTUM_PS);
        assert_eq!(eng.now_ps(), SPAN_PS);
        // One tick per step — the bus is not flooded either.
        assert_eq!(eng.bus.ticks.len() as u64, EVENTS);
    }

    #[test]
    fn events_are_dispatched_at_their_exact_time() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(1_500, timer(1, 1_500));
        eng.schedule(1_600, timer(2, 1_600));
        eng.run_until_idle().expect("run");
        // No quantum-boundary rounding: each event lands on its own time.
        assert_eq!(
            eng.bus.ticks.iter().map(|t| t.0).collect::<Vec<_>>(),
            vec![1_500, 1_600]
        );
    }

    // ------------------------------------------------------------------
    // D-17 — insertion sequence must not collide
    // ------------------------------------------------------------------

    #[test]
    fn schedule_assigns_monotonic_insertion_seq() {
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        let a = eng.schedule_event(1, timer(1, 1));
        let b = eng.schedule_event(2, timer(2, 2));
        let c = eng.schedule_event(3, timer(3, 3));
        assert_eq!((a, b, c), (0, 1, 2));
        assert_eq!(eng.next_seq(), 3);
    }

    #[test]
    fn restore_does_not_reuse_a_queued_seq() {
        let mut eng: Engine<TestBus, TestBus> = Engine::default();
        eng.schedule(10, timer(1, 10));
        let queued_seq = eng.snapshot_full().queue[0].seq;

        // Restore a state whose counter is behind the queued event.
        let state = SimState {
            event_queue_head: 0,
            ..SimState::default()
        };
        eng.restore(state);

        let fresh = eng.schedule_event(20, timer(2, 20));
        assert!(
            fresh > queued_seq,
            "restored engine reused seq {queued_seq} (fresh = {fresh})"
        );
        // And the two queued events still pop in insertion order.
        let order: Vec<u64> = eng.snapshot_full().queue.iter().map(|e| e.seq).collect();
        assert_eq!(order, vec![queued_seq, fresh]);
    }

    #[test]
    fn same_time_events_keep_insertion_order_after_restore() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        for irq in 0..4u16 {
            eng.schedule(1_000, timer(irq, 1_000));
        }
        let snap = eng.snapshot_full();
        assert_eq!(drain_raised(&mut eng), vec![0, 1, 2, 3]);

        eng.restore_full(&snap);
        assert_eq!(drain_raised(&mut eng), vec![0, 1, 2, 3]);
    }

    #[test]
    fn late_event_is_normalized_not_dropped() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(5_000, timer(1, 5_000));
        eng.run_until_idle().expect("run");
        assert_eq!(eng.now_ps(), 5_000);

        // Scheduled in the past: normalized to "now" (评审 12-6).
        eng.schedule(1_000, timer(2, 1_000));
        let snap = eng.snapshot_full();
        assert_eq!(snap.queue[0].at_ps, 5_000);

        eng.run_until_idle().expect("run");
        assert_eq!(eng.intc.raised, vec![1, 2]);
    }

    // ------------------------------------------------------------------
    // D-23 gate gap #4 — determinism across snapshot / restore
    // ------------------------------------------------------------------

    /// Observable trace of a run segment.
    #[derive(Debug, PartialEq, Eq)]
    struct Trace {
        ticks: Vec<(u64, u32)>,
        raised: Vec<u16>,
        steps: u64,
        quanta: u64,
        now_ps: u64,
        state: SimState,
        queue: Vec<QueuedEvent>,
    }

    fn run_and_trace(eng: &mut Engine<RecordingBus, RecordingIntc>, steps: u64) -> Trace {
        let tick_start = eng.bus.ticks.len();
        let irq_start = eng.intc.raised.len();
        let step_start = eng.advance_steps();
        let quanta = eng.run_steps(steps).expect("run");
        Trace {
            ticks: eng.bus.ticks[tick_start..].to_vec(),
            raised: eng.intc.raised[irq_start..].to_vec(),
            steps: eng.advance_steps() - step_start,
            quanta,
            now_ps: eng.now_ps(),
            state: eng.snapshot(),
            queue: eng.snapshot_full().queue,
        }
    }

    fn build_scenario() -> Engine<RecordingBus, RecordingIntc> {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.schedule(1_000, timer(1, 1_000));
        eng.schedule(2_000, timer(2, 2_000));
        eng.schedule(2_000, timer(3, 2_000));
        eng.schedule(
            3_000,
            SchedEvent::Commit(PendingWrite {
                paddr: 0x1000_0000,
                size: riscv_sim_contract::model::WriteSize::Word,
                value: 0xdead_beef,
                source: "m12-test".to_string(),
            }),
        );
        eng.schedule(9_000, timer(4, 9_000));
        eng
    }

    #[test]
    fn snapshot_restore_continue_is_deterministic() {
        let mut eng = build_scenario();
        let snap = eng.snapshot_full();

        // Run A: continue for two steps from the snapshot point.
        let a = run_and_trace(&mut eng, 2);
        assert_eq!(a.steps, 2);
        assert_eq!(a.raised, vec![1, 2, 3]);
        assert_eq!(a.now_ps, 2_000);

        // Restore to the snapshot point and run the same two steps.
        eng.restore_full(&snap);
        let b = run_and_trace(&mut eng, 2);

        // Everything observable must match: tick trace, IRQ trace, step
        // count, virtual time, platform state and the remaining queue.
        assert_eq!(a.ticks, b.ticks);
        assert_eq!(a.raised, b.raised);
        assert_eq!(a.steps, b.steps);
        assert_eq!(a.quanta, b.quanta);
        assert_eq!(a.now_ps, b.now_ps);
        assert_eq!(a.state, b.state);
        assert_eq!(a.queue, b.queue);
    }

    #[test]
    fn snapshot_restore_continue_matches_unrestored_run() {
        // The stronger form: a restored engine must produce the same
        // result as an engine that was never restored at all.
        let mut restored = build_scenario();
        let snap = restored.snapshot_full();
        drain_run(&mut restored);
        restored.restore_full(&snap);
        let (restored_ticks, restored_raised) = drain_run(&mut restored);

        let mut straight = build_scenario();
        let (straight_ticks, straight_raised) = drain_run(&mut straight);

        assert_eq!(restored_ticks, straight_ticks);
        assert_eq!(restored_raised, straight_raised);
        assert_eq!(restored.now_ps(), straight.now_ps());
        assert_eq!(restored.snapshot(), straight.snapshot());
        assert_eq!(
            restored.snapshot_full().queue,
            straight.snapshot_full().queue
        );
    }

    // ------------------------------------------------------------------
    // D-07 — peripheral tick seam
    // ------------------------------------------------------------------

    #[test]
    fn peripheral_tick_reaches_the_installed_ticker() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = seen.clone();
        eng.set_peripheral_ticker(PeripheralTicker::new(move |paddr, now_ps| {
            sink.borrow_mut().push((paddr, now_ps));
            Ok(vec![PendingWrite {
                paddr,
                size: riscv_sim_contract::model::WriteSize::Word,
                value: 0x1234,
                source: "m12-test".to_string(),
            }])
        }));

        eng.schedule(
            700,
            SchedEvent::PeripheralTick {
                paddr: 0x4000_0000,
                at_ps: 700,
            },
        );
        eng.run_until_idle().expect("run");

        assert_eq!(*seen.borrow(), vec![(0x4000_0000, 700)]);
        assert_eq!(eng.snapshot().pending_writes.len(), 1);
        assert_eq!(eng.snapshot().pending_writes[0].paddr, 0x4000_0000);
    }

    // ------------------------------------------------------------------
    // 中断交付（STAGE-4 裁决 I）：`ExecOutcome::InterruptTaken`
    // ------------------------------------------------------------------

    /// 一个**假**执行器：前 `interrupts` 次返回 [`ExecOutcome::InterruptTaken`]，
    /// 之后返回 [`ExecOutcome::Advanced`]。
    ///
    /// 它把「中断交付」与「指令退休」分成两件事——真实执行器（M-17）也是
    /// 这么做的：交付时那条指令**没有执行**，虚拟时钟也**没有**推进。
    struct InterruptThenAdvance {
        interrupts: u32,
        calls: u64,
    }

    impl InstructionExecutor for InterruptThenAdvance {
        fn step(&mut self, state: &mut SimState) -> Result<ExecOutcome, PlatformError> {
            self.calls += 1;
            if self.calls <= u64::from(self.interrupts) {
                // 中断交付：没有指令退休 ⇒ 时钟不推进。
                return Ok(ExecOutcome::InterruptTaken);
            }
            state.virtual_time_ps = state.virtual_time_ps.saturating_add(INSTRUCTION_COST_PS);
            Ok(ExecOutcome::Advanced)
        }
    }

    /// 中断交付**不计入**已执行条数，也**不停**批次（裁决 I）。
    ///
    /// 判别式落在**输出**上：`RunOutcome::instructions`（已执行条数）与
    /// `RunOutcome::reason`（停止原因）。
    ///
    /// ⚠️ 若把 `InterruptTaken` 复用成 `Advanced`，`instructions` 会多算
    /// 3 条（10003 ≠ 10000）——这条用例当场变红。
    #[test]
    fn an_interrupt_delivery_is_neither_counted_nor_a_stop() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut executor = InterruptThenAdvance {
            interrupts: 3,
            calls: 0,
        };

        let outcome = eng.run_until(&mut executor, u64::MAX).expect("run");

        assert_eq!(
            outcome.instructions,
            u64::from(DEFAULT_INSTRUCTION_BUDGET),
            "中断交付**没有**指令退休 ⇒ 已执行条数不能多算"
        );
        assert_eq!(
            executor.calls,
            u64::from(DEFAULT_INSTRUCTION_BUDGET) + 3,
            "执行器被调用的次数 = 已执行条数 + 中断交付次数（交付不占配额）"
        );
        assert_eq!(
            outcome.reason,
            RunReason::BudgetExhausted,
            "中断交付**不是**停止原因——它是「透明」的，handler 紧接着跑"
        );
    }

    // ------------------------------------------------------------------
    // VD-F3 — watchdog：无进展检测（LLD §3.5）
    // ------------------------------------------------------------------

    /// 一个**永远**返回 [`ExecOutcome::InterruptTaken`] 的执行器 ——
    /// 用来复现 VD-F3 的活锁形态。
    ///
    /// 它模拟的是「M-16 的 `active_irq` 单级防护破掉」之后的世界：中断被
    /// **反复**交付，而每一次交付都**不退休指令、不推进虚拟时间**。
    struct AlwaysInterrupt {
        calls: u64,
    }

    impl InstructionExecutor for AlwaysInterrupt {
        fn step(&mut self, _state: &mut SimState) -> Result<ExecOutcome, PlatformError> {
            self.calls += 1;
            // 中断交付：没有指令退休 ⇒ 时钟不推进 ⇒ 三个记账全不动。
            Ok(ExecOutcome::InterruptTaken)
        }
    }

    /// **活锁必须被抓住**：`run_until` 要**报错退出**，而不是永远自旋
    /// （V-D 检视 · **VD-F3**）。
    ///
    /// ⚠️ **这条用例在修复前会挂死整个测试二进制**（VD-F 实测：8 秒内执行器
    /// 被调用 3.35 亿次，`run_until` 永不返回）——所以「用例能跑完」本身就是
    /// 这条判据的一部分：守卫必须在**有限步内**触发。
    ///
    /// ⚠️ 断言落在**错误族 / 场景码**上，而不只是「返回了 Err」：守卫触发时
    /// 报的是「平台故障」（`E_ENG-03`），不是 guest 的正常停止语义
    /// （LLD §3.5 · 评审 12-2 / 12-3：watchdog 与 guest trap 严格区分）。
    #[test]
    fn a_runaway_interrupt_delivery_is_reported_instead_of_spinning_forever() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut executor = AlwaysInterrupt { calls: 0 };

        let err = eng
            .run_until(&mut executor, u64::MAX)
            .expect_err("活锁必须报错退出，而不是无限自旋");

        assert_eq!(
            err.family,
            ErrorFamily::Engine,
            "停滞是**平台故障**，必须落在引擎族里（实得 {:?}）",
            err.family
        );
        assert_eq!(
            err.scenario, E_ENG_03_NO_PROGRESS,
            "必须是「无进展」这一个场景码（实得 {}）",
            err.scenario
        );
        assert!(
            executor.calls <= u64::from(NO_PROGRESS_WINDOW),
            "守卫必须在有限步内触发（实得 {} 次调用，窗口 {}）",
            executor.calls,
            NO_PROGRESS_WINDOW
        );
    }

    /// **合法路径不许被守卫误伤**：一串中断交付之后 handler 真的退休了
    /// 指令 ⇒ 主循环必须正常跑完（让出给宿主），而不是报错。
    ///
    /// ⚠️ 交付 **500** 次（远小于 [`NO_PROGRESS_WINDOW`] = 1024）——这条用例
    /// 钉住的是「守卫的阈值**不能太紧**」：阈值一旦被下调到 500 以下，它
    /// 立刻变红。
    #[test]
    fn a_burst_of_interrupt_deliveries_still_makes_progress() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut executor = InterruptThenAdvance {
            interrupts: 500,
            calls: 0,
        };

        let outcome = eng
            .run_until(&mut executor, u64::MAX)
            .expect("合法路径不许被守卫误伤");

        assert_eq!(
            outcome.reason,
            RunReason::BudgetExhausted,
            "handler 退休了指令 ⇒ 有进展 ⇒ 主循环正常让出给宿主"
        );
        assert_eq!(
            outcome.instructions,
            u64::from(DEFAULT_INSTRUCTION_BUDGET),
            "中断交付仍然不计入已执行条数"
        );
    }

    // ------------------------------------------------------------------
    // VD-F1 — 引擎自己产出的快照必须过契约自己的校验器
    // ------------------------------------------------------------------

    /// 造一个「队列里有一件**已到期未派发**事件」的引擎状态。
    ///
    /// ⚠️ 走**内部类型** [`EngineSnapshot`]（不是契约 DTO）——契约 DTO 那条
    /// 路会先 `validate()`（检查 5 / 7）⇒ 坏快照**根本进不到**引擎里。
    fn engine_with_a_due_event() -> (Engine<RecordingBus, RecordingIntc>, EngineSnapshot) {
        let hand_built = EngineSnapshot {
            state: SimState {
                virtual_time_ps: 6_000,
                event_queue_head: 1,
                ..SimState::default()
            },
            queue: vec![QueuedEvent {
                at_ps: 5_000,
                seq: 0,
                event: SchedEvent::TimerExpiry {
                    irq: 7,
                    at_ps: 5_000,
                },
            }],
            next_seq: 1,
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
        };
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        eng.restore_full(&hand_built);
        (eng, hand_built)
    }

    /// **引擎自己产出的快照必须过契约自己的校验器**（V-D 检视 · **VD-F1**）。
    ///
    /// 现象：队列里若有一件「**已到期未派发**」的事件（`at_ps < now_ps`），
    /// 契约的检查 7 直接**拒收** ⇒ `capture_session → restore_session` 与
    /// `M23::put_snapshot` 一起失败。
    #[test]
    fn a_snapshot_holding_a_due_event_still_passes_the_contract_validator() {
        let (mut eng, hand_built) = engine_with_a_due_event();

        // 对照：那份**未归一化**的手工快照确实过不了契约的校验器（检查 7）
        // ⇒ 差别确实在「引擎出快照时有没有归一化」这一步。
        let raw = riscv_sim_contract::snapshot::EngineSnapshotDto::from(&hand_built);
        let raw_err = raw
            .validate()
            .expect_err("未归一化的快照必须被契约的校验器拒绝");
        assert_eq!(
            raw_err.scenario,
            riscv_sim_contract::snapshot::E_SNAP_02_MALFORMED_QUEUE,
            "拒收的原因必须是「队列畸形」（检查 7），实得 {}",
            raw_err.scenario
        );

        // 引擎**自己**产出的快照必须能过自己的校验器。
        let dto = eng.snapshot_dto();
        dto.validate()
            .expect("引擎产出的快照必须过契约自己的校验器");
        assert_eq!(
            dto.queue[0].at_ps, 6_000,
            "迟到的事件必须被归一化到当前时刻（与入队时的归一化同一条规则）"
        );

        // 往返：还原必须成功，且那件事件**不许丢**。
        eng.restore_dto(&dto).expect("往返必须成功");
        assert_eq!(eng.pending_event_count(), 1, "事件不许在往返里丢掉");
        // 再拍一次快照必须**逐字相同**（归一化是幂等的）。
        assert_eq!(
            eng.snapshot_dto(),
            dto,
            "归一化必须幂等：还原后再拍一次，快照必须逐字相同"
        );
    }

    /// **会话级载荷**（`capture_session → restore_session` 与
    /// `M23::put_snapshot` 走的那条路）必须也能过校验。
    ///
    /// 会话载荷的第 10 条检查 = 「引擎段自己过自己的校验器」⇒ 引擎段修好
    /// 了，这条路才通。
    #[test]
    fn a_session_payload_carrying_the_engine_snapshot_validates() {
        let (eng, _hand_built) = engine_with_a_due_event();
        let payload = riscv_sim_contract::session::SessionSnapshotDto {
            m12_engine: eng.snapshot_dto(),
            ..Default::default()
        };
        payload.validate().expect("带着引擎段的会话载荷必须过校验");
    }

    // ------------------------------------------------------------------
    // VD-F2 — `restore_full` 的 `next_seq` 下限修正必须有判别力
    // ------------------------------------------------------------------

    /// **恢复一份 `next_seq` 小于队列最大 `seq` 的快照 ⇒ 计数器必须被抬到
    /// 队列之上**（V-D 检视 · **VD-F2**）。
    ///
    /// ⚠️ 必须走**内部类型** [`EngineSnapshot`]：契约 DTO 那条路会先
    /// `validate()`（检查 5「`event.seq < next_seq`」）⇒ 坏快照**进不到**
    /// `restore_full`。
    ///
    /// ⚠️ 断言落在**可观测后果**上（恢复后第一次 `schedule` 拿到的 `seq`
    /// 必须没被用过），而不只是「计数器被抬了」——「抬了」是实现细节，
    /// 「不撞号」才是这条修正要保的东西（D-17）。
    #[test]
    fn restoring_a_hand_built_snapshot_raises_the_insertion_counter_above_the_queue() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let hand_built = EngineSnapshot {
            state: SimState::default(),
            queue: vec![
                QueuedEvent {
                    at_ps: 1_000,
                    seq: 3,
                    event: SchedEvent::TimerExpiry {
                        irq: 1,
                        at_ps: 1_000,
                    },
                },
                QueuedEvent {
                    at_ps: 2_000,
                    seq: 7,
                    event: SchedEvent::TimerExpiry {
                        irq: 2,
                        at_ps: 2_000,
                    },
                },
            ],
            // ⚠️ **故意**给一个**小于**队列最大 `seq` 的计数器。
            next_seq: 0,
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
        };

        eng.restore_full(&hand_built);

        assert!(
            eng.next_seq() > 7,
            "插入序计数器必须被抬到队列最大 seq 之上（实得 {}）",
            eng.next_seq()
        );
        let fresh = eng.schedule_event(
            3_000,
            SchedEvent::TimerExpiry {
                irq: 3,
                at_ps: 3_000,
            },
        );
        assert!(
            fresh > 7,
            "恢复后排的事件必须拿到一个没被用过的 seq（实得 {fresh}）"
        );
        let seqs: Vec<u64> = eng.snapshot_full().queue.iter().map(|e| e.seq).collect();
        assert_eq!(
            seqs,
            vec![3, 7, fresh],
            "队列里的 seq 必须互不相同（D-17：不撞号）"
        );
    }

    // ------------------------------------------------------------------
    // D-120：引擎族场景码**只住在契约层**（STAGE-9 裁决 G）
    // ------------------------------------------------------------------

    /// ⚠️ **等值断言**：M-12 用的三个场景码必须**等于**契约层的那三个。
    ///
    /// ⚠️ **为什么这条不是废话**：M-12 **曾经**自己定义过这三个常量
    /// （各带一份字面量）⇒ 契约层登记了
    /// 一份、M-12 又写了一份 ⇒ 将来改一处忘了另一处，契约层说的码与
    /// 内核报的码就会**不一致**，而前端是按**契约层**的码分派的。
    ///
    /// ⚠️ 断言的是**值相等**（不是「源码里有几处 `E_ENG_0`」）——
    /// 后者是**排版耦合**的检查（改个缩进就变红），而这一条在
    /// 「M-12 又写回一份本地字面量」时**真的**变红。
    #[test]
    fn the_engine_scenario_codes_are_the_contract_layers_ones() {
        assert_eq!(
            E_ENG_01_BUS_PUBLISH_FAILED,
            riscv_sim_contract::error::E_ENG_01_BUS_PUBLISH_FAILED,
            "E_ENG-01 的值必须来自契约层"
        );
        assert_eq!(
            E_ENG_02_QUEUE_INCONSISTENT,
            riscv_sim_contract::error::E_ENG_02_QUEUE_INCONSISTENT,
            "E_ENG-02 的值必须来自契约层"
        );
        assert_eq!(
            E_ENG_03_NO_PROGRESS,
            riscv_sim_contract::error::E_ENG_03_NO_PROGRESS,
            "E_ENG-03 的值必须来自契约层"
        );
    }

    /// ⚠️ **对拍用例**：M-12 **实际报出来的**码必须与**契约层场景码表**
    /// 里的码**逐字相同**。
    ///
    /// ⚠️ 这条比上面那条更硬：上面那条比的是「M-12 用的常量」与「契约层
    /// 的常量」，而这条比的是「**引擎报给调用方的那一个数**」——前端
    /// 拿到的就是它。
    #[test]
    fn the_reported_no_progress_code_is_the_contract_layers_code() {
        let mut eng: Engine<RecordingBus, RecordingIntc> = Engine::default();
        let mut executor = AlwaysInterrupt { calls: 0 };

        let err = eng
            .run_until(&mut executor, u64::MAX)
            .expect_err("活锁必须报错退出");

        assert_eq!(
            err.scenario,
            riscv_sim_contract::error::E_ENG_03_NO_PROGRESS,
            "M-12 报出来的码必须与契约层场景码表里的码逐字相同"
        );
        assert_eq!(
            err.scenario, 3,
            "E_ENG-03 的场景码字面量是 3（架构字面量，不从被测实现反推）"
        );
    }
}
