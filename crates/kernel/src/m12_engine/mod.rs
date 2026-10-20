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
//!
//! 快照有三种形态：[`Engine::snapshot`] 返回契约级 [`SimState`]（M-17
//! 交给宿主的那一份，签名保持不变）；[`Engine::snapshot_full`] 返回
//! [`EngineSnapshot`]，额外携带待处理事件队列与插入序计数器 —— **只有
//! 它能精确还原引擎**（D-05）；[`Engine::snapshot_dto`] 返回契约层的
//! 可序列化镜像 [`EngineSnapshotDto`]，让确定性的快照**落得了盘**
//! （D-24 根因修复）。
//!
//! TODO(MVP): 把调度器接到 [`crate::m13_iss`] 的译码器、外设 tick 回调
//! 与 M-20 快照落盘路径上；当前的调度内核（时间推进 / 事件队列 / 稳定
//! 排序 / 快照还原）已经可用。

use std::fmt;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, SimState};
use riscv_sim_contract::port::{EventBus, InterruptSink};
use riscv_sim_contract::protocol::EventPayload;

mod queue;
mod snapshot;

pub use queue::{EventQueue, QueuedEvent};

/// Default time quantum, in picoseconds. Configurable per-project (M-12
/// §4.12.2).
pub const TIME_QUANTUM_PS: u64 = 1_000; // 1 µs

/// Default guest-instruction budget before the engine yields to the host.
/// Implemented as the watchdog pre-check (M-12 §5.2 + LLD §4.12.6).
pub const DEFAULT_INSTRUCTION_BUDGET: u32 = 10_000;

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

/// The event-driven engine itself.
#[derive(Debug)]
pub struct Engine<B: EventBus, I: InterruptSink> {
    /// Pending events, time-ordered by `(at_ps, seq)`.
    queue: EventQueue,
    /// Instruction budget for the current quantum. Reserved for the
    /// watchdog pre-check (M-12 §5.2 + LLD §4.12.6); not yet threaded
    /// through `dispatch` in the MVP skeleton.
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
    /// The injected dependencies (event bus, interrupt sink) and the
    /// peripheral-tick seam ([`Engine::set_peripheral_ticker`]) are
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

    /// Borrow the injected event bus (read-only).
    ///
    /// The bus is a dependency, not engine state — it is deliberately
    /// **not** part of any snapshot. Exposing it read-only lets the host
    /// observe what the engine published without letting it mutate the
    /// engine.
    pub fn bus(&self) -> &B {
        &self.bus
    }

    /// Borrow the injected interrupt sink (read-only).
    ///
    /// Same posture as [`Engine::bus`]: an injected dependency, not
    /// snapshot state.
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
                .map_err(|e| PlatformError::new(ErrorFamily::Engine, 1, e.to_string()))?;
            steps += 1;
            self.advance_steps = self.advance_steps.wrapping_add(1);

            // Dispatch every event that is due at the current time.
            while let Some(t) = self.queue.peek().map(|e| e.at_ps) {
                if t > self.now_ps() {
                    break;
                }
                let evt = self.queue.pop().expect("peek said some");
                self.dispatch(evt.event)?;
            }
        }
        Ok(quanta_between(start_ps, self.now_ps()))
    }

    fn dispatch(&mut self, event: SchedEvent) -> Result<(), PlatformError> {
        match event {
            SchedEvent::PeripheralTick { paddr, at_ps } => {
                // D-07: the peripheral registry lives on the bus side
                // (M-14 / M-15) and the contract has no port for "find the
                // peripheral at `paddr` and tick it" yet. The seam below is
                // the shape we expect it to plug into — a
                // `PeripheralTicker` callback installed by whoever owns
                // the bus (see `Engine::set_peripheral_ticker`). Until
                // then the event is a no-op.
                let writes = match self.ticker.as_mut() {
                    Some(ticker) => ticker.tick(paddr, at_ps)?,
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
        let floor = self.queue.max_seq().map_or(0, |seq| seq.saturating_add(1));
        self.next_seq = self.state.event_queue_head.max(floor);
        self.sync_seq_mirror();
    }

    /// Snapshot the engine *including* the pending event queue.
    ///
    /// This is the form that can be restored exactly: queue contents, queue
    /// order, the insertion counter and the virtual time (D-05).
    pub fn snapshot_full(&self) -> EngineSnapshot {
        let mut state = self.state.clone();
        self.sync_state_seq(&mut state);
        EngineSnapshot {
            state,
            queue: self.queue.to_ordered_vec(),
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
        self.queue = EventQueue::from_ordered_vec(snapshot.queue.clone());
        self.next_seq = snapshot.next_seq;
        self.instruction_budget = snapshot.instruction_budget;
        // Defensive: a hand-built snapshot could name a `next_seq` that is
        // already taken by a queued event. For a consistent snapshot this
        // is a no-op (`next_seq` is always above every queued `seq`).
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
        assert_eq!(quanta, 1_000_000_000);
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
}
