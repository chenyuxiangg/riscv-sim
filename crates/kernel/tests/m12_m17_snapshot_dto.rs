//! 契约快照 DTO 的端到端证据（D-24 根因修复）。
//!
//! D-24 的原始复现（独立检视 V2 · F-1）：
//!
//! ```text
//! schedule A; snapshot; schedule B; restore; run
//! ```
//!
//! 结果是 **A 与 B 都触发**，而「同种子从未 restore 的运行」只有 A ——
//! 因为契约的 [`SimState`] 装不下事件队列，partial 还原后快照之后入队的
//! 事件仍然留在队列里。
//!
//! 本文件把**同一个场景跑两遍**：
//!
//! - partial 路径（契约 `snapshot` / `restore`）—— 复现发散，锁住现状；
//! - full 路径（契约 `snapshot_full` / `restore_full`，载体是新增的
//!   可序列化 [`EngineSnapshotDto`]）—— 证明不再发散。
//!
//! 另有 JSON 落盘往返、M-17 后端接线、契约默认实现的证据。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::EventPayload;
use riscv_sim_contract::snapshot::{EngineSnapshotDto, E_SNAP_03_UNSUPPORTED_RESTORE};

use riscv_sim_kernel::m12_engine::{Engine, SchedEvent, TIME_QUANTUM_PS};
use riscv_sim_kernel::m17_backend::SelfIssBackend;

// ---------------------------------------------------------------------------
// 测试脚手架
// ---------------------------------------------------------------------------

/// 记录被拉起的 IRQ 行的中断控制器。
#[derive(Debug, Default, Clone)]
struct RecordingIntc {
    raised: Vec<u16>,
}

impl InterruptSink for RecordingIntc {
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.raised.push(irq);
        Ok(())
    }
    fn lower(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// 什么都不做的事件总线。
#[derive(Debug, Default, Clone)]
struct NullBus;

impl EventBus for NullBus {
    fn publish(&mut self, _evt: EventPayload) -> Result<(), PlatformError> {
        Ok(())
    }
}

type TestEngine = Engine<NullBus, RecordingIntc>;

fn timer(irq: u16, at_ps: u64) -> SchedEvent {
    SchedEvent::TimerExpiry { irq, at_ps }
}

/// 跑到底，并只返回**本次运行**新拉起的 IRQ。
fn drain_raised(eng: &mut TestEngine) -> Vec<u16> {
    let start = eng.intc().raised.len();
    eng.run_until_idle().expect("run");
    eng.intc().raised[start..].to_vec()
}

// ---------------------------------------------------------------------------
// 1. D-24 原始复现：partial 路径仍然发散
// ---------------------------------------------------------------------------

#[test]
fn partial_restore_still_lets_post_snapshot_events_fire() {
    // 这是 D-24 的**现状**（partial 契约路径）。把它钉成一条用例，
    // 免得有人日后误以为 partial 已经是确定性的。
    let mut eng = TestEngine::default();
    eng.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));

    let partial: SimState = eng.snapshot();

    eng.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));

    eng.restore(partial);
    let irqs = drain_raised(&mut eng);

    assert_eq!(
        irqs,
        vec![1, 2],
        "partial 还原后，快照之后入队的事件仍会触发 —— 这正是 D-24 描述的发散"
    );
}

// ---------------------------------------------------------------------------
// 2. full 路径：同样的场景不再发散
// ---------------------------------------------------------------------------

#[test]
fn full_restore_drops_post_snapshot_events() {
    let mut eng = TestEngine::default();
    eng.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));

    let full = eng.snapshot_dto();

    eng.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));

    eng.restore_dto(&full);
    let irqs = drain_raised(&mut eng);

    assert_eq!(irqs, vec![1], "完整快照还原后，快照之后入队的事件不再触发");
}

#[test]
fn full_restore_matches_a_run_from_the_same_snapshot_state() {
    // 确定性性质（D-05 / FR-12）：从快照还原后的推进，必须与「从同样的
    // 状态出发、但没有还原过」的推进产生完全相同的事件序列。
    //
    // 参照组：直接排两个事件、然后跑到底。
    let mut reference = TestEngine::default();
    reference.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    reference.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));
    let expected = drain_raised(&mut reference);

    // 实验组：先排出同样的两个事件，导出快照，再还原到一个新引擎。
    let mut sampled = TestEngine::default();
    sampled.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    sampled.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));
    let dto = sampled.snapshot_dto();

    let mut restored = TestEngine::default();
    restored.restore_dto(&dto);
    let actual = drain_raised(&mut restored);

    assert_eq!(
        actual, expected,
        "从快照还原后的推进必须与从同样状态出发的推进完全一致"
    );
}

// ---------------------------------------------------------------------------
// 3. 落盘往返：DTO → JSON → DTO → 引擎
// ---------------------------------------------------------------------------

#[test]
fn dto_survives_a_json_round_trip_and_still_restores_deterministically() {
    // 参照组：三个事件，直接跑到底。
    let mut reference = TestEngine::default();
    reference.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    reference.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));
    reference.schedule(3 * TIME_QUANTUM_PS, timer(3, 3 * TIME_QUANTUM_PS));
    let expected = drain_raised(&mut reference);

    // 采样：同样的三个事件排队中，导出 DTO。
    let mut eng = TestEngine::default();
    eng.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    eng.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));
    eng.schedule(3 * TIME_QUANTUM_PS, timer(3, 3 * TIME_QUANTUM_PS));

    let dto = eng.snapshot_dto();
    dto.validate().expect("引擎导出的快照必须自洽");

    // 落盘：序列化成 JSON，再读回来。
    let json = serde_json::to_string(&dto).expect("serialize");
    let restored: EngineSnapshotDto = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(dto, restored, "JSON 往返不得丢字段");

    // 换一个新引擎，用读回来的 DTO 还原。
    let mut fresh = TestEngine::default();
    fresh.restore_dto(&restored);
    let actual = drain_raised(&mut fresh);

    assert_eq!(actual, expected, "从 JSON 还原后的推进必须与参照组一致");
}

// ---------------------------------------------------------------------------
// 4. 队列内容真的进了 DTO（不是空壳）
// ---------------------------------------------------------------------------

#[test]
fn dto_carries_the_actual_pending_queue() {
    let mut eng = TestEngine::default();
    eng.schedule(TIME_QUANTUM_PS, timer(11, TIME_QUANTUM_PS));
    eng.schedule(2 * TIME_QUANTUM_PS, timer(22, 2 * TIME_QUANTUM_PS));

    let dto = eng.snapshot_dto();

    assert_eq!(dto.queue.len(), 2, "队列必须被完整导出");
    assert_eq!(dto.queue[0].at_ps, TIME_QUANTUM_PS);
    assert_eq!(dto.queue[1].at_ps, 2 * TIME_QUANTUM_PS);
    assert_eq!(dto.next_seq, 2, "插入序计数器必须一并导出");
    assert!(
        dto.state.event_queue_head == dto.next_seq,
        "SimState 里的插入序镜像必须与 next_seq 一致"
    );
}

// ---------------------------------------------------------------------------
// 5. M-17 后端接线
// ---------------------------------------------------------------------------

#[test]
fn backend_exposes_the_full_snapshot_through_the_contract() {
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    backend
        .engine_mut()
        .schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    backend
        .engine_mut()
        .schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));

    let dto = backend
        .snapshot_full()
        .expect("snapshot_full")
        .expect("SelfIssBackend 必须支持完整快照");
    assert_eq!(dto.queue.len(), 2);

    // 排一个快照之后的事件，还原后它不该再触发。
    backend
        .engine_mut()
        .schedule(3 * TIME_QUANTUM_PS, timer(3, 3 * TIME_QUANTUM_PS));
    backend.restore_full(&dto).expect("restore_full");

    let start = backend.engine().intc().raised.len();
    backend.continue_until_event().expect("run");
    let irqs = &backend.engine().intc().raised[start..];
    assert_eq!(irqs, &[1, 2], "还原后只应触发快照里有的事件");
}

// ---------------------------------------------------------------------------
// 6. 契约默认实现（additive pattern）
// ---------------------------------------------------------------------------

/// 一个**不**覆盖新方法的后端 —— 用来锁住契约的默认行为。
struct MinimalBackend;

impl ExecutionBackend for MinimalBackend {
    fn reset(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn step_n(&mut self, _n: u32) -> Result<u32, PlatformError> {
        Ok(0)
    }
    fn continue_until_event(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn pause(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn snapshot(&self) -> Result<SimState, PlatformError> {
        Ok(SimState::default())
    }
    fn restore(&mut self, _state: &SimState) -> Result<(), PlatformError> {
        Ok(())
    }
    fn read_memory(&self, _addr: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        Ok(0)
    }
    fn write_memory(
        &mut self,
        _addr: u32,
        _size: WriteSize,
        _value: u32,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_breakpoint(&mut self, _addr: u32, _enabled: bool) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_watchpoint(
        &mut self,
        _addr: u32,
        _size: WriteSize,
        _write: bool,
        _enabled: bool,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
}

#[test]
fn backend_reset_returns_to_the_entry_state() {
    // `ExecutionBackend::reset` 的契约是「Reset to entry state」。此前
    // M-17 的实现只重置了后端自己那份 `state` 镜像，**引擎原封不动** ——
    // 队列、虚拟时钟、寄存器全留着，`snapshot()` 继续返回 reset 之前的状态。
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    backend
        .engine_mut()
        .schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    backend.engine_mut().run_until_idle().expect("run");
    assert!(
        backend.snapshot().expect("snap").virtual_time_ps > 0,
        "运行之后虚拟时钟应当已经前进"
    );

    backend.reset().expect("reset");

    assert_eq!(
        backend.engine().pending_event_count(),
        0,
        "reset 之后待处理队列必须为空"
    );
    assert_eq!(
        backend.snapshot().expect("snap"),
        SimState::default(),
        "reset 之后的快照必须与全新后端的快照逐字相同"
    );

    // 完整快照同样必须回到入口状态。
    let fresh: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());
    assert_eq!(
        backend.snapshot_full().expect("full"),
        fresh.snapshot_full().expect("full"),
        "reset 之后的完整快照必须与全新后端一致"
    );
}

#[test]
fn contract_defaults_are_additive_and_fail_loudly() {
    let mut backend = MinimalBackend;

    // 不支持完整快照的后端必须**如实**说没有，而不是给一个空队列假装支持。
    assert!(
        backend.snapshot_full().expect("snapshot_full").is_none(),
        "默认实现必须返回 None"
    );

    // 还原必须**大声失败**：静默跳过会让调用方误以为自己续上了确定性状态。
    let err = backend
        .restore_full(&EngineSnapshotDto::default())
        .expect_err("默认实现必须报错");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_03_UNSUPPORTED_RESTORE);
}
