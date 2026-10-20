//! 契约快照 DTO 的端到端证据（D-24 根因修复）。
//!
//! D-24 的原始复现（独立检视 V2 · F-1）：
//!
//! ```text
//! schedule A; snapshot; schedule B; restore; run
//! ```
//!
//! 结果是 **A 与 B 都触发**，而「从快照时刻出发的运行」只有 A ——
//! 因为契约的 [`SimState`] 装不下事件队列，partial 还原后快照之后入队的
//! 事件仍然留在队列里。
//!
//! 本文件把**同一个场景跑两遍**：
//!
//! - partial 路径（契约 `snapshot` / `restore`）—— 复现发散，锁住现状；
//! - full 路径（契约 `snapshot_full` / `restore_full`，载体是新增的
//!   可序列化 [`EngineSnapshotDto`]）—— 证明不再发散。
//!
//! 另有 JSON 落盘往返、M-17 后端接线、畸形快照拒绝、契约默认实现的证据。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::EventPayload;
use riscv_sim_contract::snapshot::{
    EngineSnapshotDto, E_SNAP_01_UNSUPPORTED_SCHEMA, E_SNAP_02_MALFORMED_QUEUE,
    E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT, E_SNAP_04_QUEUE_HEAD_MISMATCH,
};

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

/// **表征测试（characterization test）**——它钉住的是 D-24 描述的**已知
/// 缺陷**：partial 还原后，快照之后入队的事件仍然会触发。
///
/// ⚠️ **这不是「期望行为」**。若将来有人把 partial 路径也修成确定性的
/// （让 `restore` 连队列一起还原），**本用例会变红，那是修复的信号，不是
/// 回归**。届时请把它改名并改写断言，而不是回退修复。
#[test]
fn characterization_partial_restore_diverges_from_the_snapshot_state() {
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

    // 台账 D-24 的另一半断言：「从快照时刻出发的运行只有 A」。把这一半
    // 也钉住，D-24 的完整描述才算被用例覆盖。
    let mut reference = TestEngine::default();
    reference.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    assert_eq!(
        drain_raised(&mut reference),
        vec![1],
        "从快照时刻出发的运行只应触发 A"
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

    eng.restore_dto(&full).expect("restore_dto");
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
    restored.restore_dto(&dto).expect("restore_dto");
    let actual = drain_raised(&mut restored);

    assert_eq!(
        actual, expected,
        "从快照还原后的推进必须与从同样状态出发的推进完全一致"
    );
}

// ---------------------------------------------------------------------------
// 3. 引擎级逐位往返：pending_writes / 指令预算 / 虚拟时间 / next_seq
// ---------------------------------------------------------------------------

#[test]
fn engine_level_round_trip_preserves_every_snapshot_field() {
    // 一条断言同时盖住四个此前**无任何用例兜住**的字段
    // （独立检视 V-B 的变异 B2 / B5 / B8 全部 0 red）：
    //   · `instruction_budget` —— 文档明写「每量子指令预算全部还原」
    //   · `pending_writes`     —— 已投入使用的提交暂存区
    //   · `virtual_time_ps`    —— 虚拟时钟
    //   · `next_seq`           —— 插入序计数器
    let mut original = TestEngine::default();
    original.set_instruction_budget(4321);
    original.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    original.run_until_idle().expect("run");

    let dto = original.snapshot_dto();
    assert_eq!(dto.instruction_budget, 4321, "指令预算必须被导出");
    assert_ne!(dto.state.virtual_time_ps, 0, "虚拟时钟必须已经被推进");

    let mut restored = TestEngine::default();
    restored.restore_dto(&dto).expect("restore_dto");

    // 引擎级逐位相等：一次覆盖 state / queue / next_seq / instruction_budget
    assert_eq!(
        restored.snapshot_full(),
        original.snapshot_full(),
        "引擎级往返必须逐位相等"
    );
    assert_eq!(
        restored.instruction_budget(),
        4321,
        "还原后指令预算必须与快照一致"
    );
    assert_eq!(
        restored.next_seq(),
        original.next_seq(),
        "还原后插入序计数器必须与快照一致"
    );
}

#[test]
fn pending_writes_survive_the_dto_round_trip() {
    // `pending_writes` 是已投入使用的「提交暂存区」：外设 tick 期间产生的
    // 写先落在这里，量子边界才提交到总线。DTO 转换层一旦把它弄丢，
    // 外设的写就会**静默消失**——此前没有任何用例能抓住（变异 B5 0 red）。
    let mut eng = TestEngine::default();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = seen.clone();
    eng.set_peripheral_ticker(riscv_sim_kernel::m12_engine::PeripheralTicker::new(
        move |paddr, at_ps| {
            sink.borrow_mut().push((paddr, at_ps));
            Ok(vec![riscv_sim_contract::model::PendingWrite {
                paddr: 0x2000_0000,
                size: WriteSize::Word,
                value: 0xcafe_babe,
                source: "test".to_string(),
            }])
        },
    ));
    eng.schedule(
        700,
        SchedEvent::PeripheralTick {
            paddr: 0x4000_0000,
            at_ps: 700,
        },
    );
    eng.run_until_idle().expect("run");
    assert_eq!(eng.snapshot().pending_writes.len(), 1, "夹具前提");

    let dto = eng.snapshot_dto();
    assert_eq!(
        dto.state.pending_writes.len(),
        1,
        "待提交写必须被导出到 DTO"
    );
    assert_eq!(dto.state.pending_writes[0].value, 0xcafe_babe);

    let mut restored = TestEngine::default();
    restored.restore_dto(&dto).expect("restore_dto");
    assert_eq!(
        restored.snapshot().pending_writes,
        dto.state.pending_writes,
        "待提交写必须被还原"
    );
}

// ---------------------------------------------------------------------------
// 4. 落盘往返：DTO → JSON → DTO → 引擎
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
    fresh.restore_dto(&restored).expect("restore_dto");
    let actual = drain_raised(&mut fresh);

    assert_eq!(actual, expected, "从 JSON 还原后的推进必须与参照组一致");
}

// ---------------------------------------------------------------------------
// 5. 队列内容真的进了 DTO（不是空壳）
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
// 6. 畸形快照必须在入口被拒绝（独立检视 V-B · H-1）
// ---------------------------------------------------------------------------

/// 造一个「结构上像样但自相矛盾」的 DTO。
///
/// 夹具在变形之后**同步** `state.event_queue_head` 与 `next_seq`，这样
/// 触发的一定是「变形」本身对应的检查，而不是镜像不一致那条（那条另有
/// 专门的用例）。
fn malformed(mutate: impl FnOnce(&mut EngineSnapshotDto)) -> EngineSnapshotDto {
    let mut eng = TestEngine::default();
    eng.schedule(TIME_QUANTUM_PS, timer(1, TIME_QUANTUM_PS));
    eng.schedule(2 * TIME_QUANTUM_PS, timer(2, 2 * TIME_QUANTUM_PS));
    let mut dto = eng.snapshot_dto();
    mutate(&mut dto);
    dto.state.event_queue_head = dto.next_seq;
    dto
}

#[test]
fn restore_full_rejects_an_unknown_schema_version() {
    let dto = malformed(|d| d.schema_version = 12345);
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    let err = backend.restore_full(&dto).expect_err("必须拒绝");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_01_UNSUPPORTED_SCHEMA);
}

#[test]
fn restore_full_rejects_a_duplicated_seq() {
    let dto = malformed(|d| {
        // 两个事件共用同一个 seq —— 规范序检查与查重都会命中。
        d.queue[0].seq = d.queue[1].seq;
    });
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    let err = backend.restore_full(&dto).expect_err("必须拒绝");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
}

#[test]
fn restore_full_rejects_a_next_seq_that_would_collide() {
    let dto = malformed(|d| d.next_seq = 0); // 与队列里已占用的 seq 撞号
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    let err = backend.restore_full(&dto).expect_err("必须拒绝");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
}

#[test]
fn restore_full_rejects_an_insertion_counter_mismatch() {
    // DTO 把插入序计数器存了两遍；两者不一致说明这份快照不是这份引擎
    // 写出来的，静默挑一个信会让结果取决于读的是哪个字段。
    let mut dto = malformed(|_| {});
    dto.state.event_queue_head = dto.next_seq + 7;
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());

    let err = backend.restore_full(&dto).expect_err("必须拒绝");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_04_QUEUE_HEAD_MISMATCH);
}

#[test]
fn a_rejected_snapshot_leaves_the_engine_untouched() {
    // 「拒绝」必须是**拒绝**，不是「部分应用」：畸形快照被拒之后，引擎
    // 必须还停在原来的状态上。
    let mut backend: SelfIssBackend<NullBus, RecordingIntc> =
        SelfIssBackend::new(NullBus, RecordingIntc::default());
    backend
        .engine_mut()
        .schedule(TIME_QUANTUM_PS, timer(9, TIME_QUANTUM_PS));
    let before = backend.engine().snapshot_full();

    let broken = malformed(|d| d.schema_version = 999);
    assert!(backend.restore_full(&broken).is_err());

    assert_eq!(
        backend.engine().snapshot_full(),
        before,
        "被拒绝的快照不得改动引擎"
    );
}

// ---------------------------------------------------------------------------
// 7. M-17 后端接线
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

    let dto = backend.snapshot_full().expect("snapshot_full");
    assert_eq!(dto.queue.len(), 2);

    // 排一个快照之后的事件，还原后它不该再触发。
    backend
        .engine_mut()
        .schedule(3 * TIME_QUANTUM_PS, timer(3, 3 * TIME_QUANTUM_PS));
    backend.restore_full(&dto).expect("restore_full");

    let start = backend.engine().intc().raised.len();
    // 这里要的是「**把队列排空**」，不是「执行 guest 指令」——
    // `continue_until_event` 走的是唯一主循环，它会真的去取指，而这个
    // 后端**没有映射任何内存**（取指必然失败）。快照 DTO 的语义与
    // 「跑不跑指令」无关，用纯事件推进即可。
    backend.engine_mut().run_until_idle().expect("run");
    let irqs = &backend.engine().intc().raised[start..];
    assert_eq!(irqs, &[1, 2], "还原后只应触发快照里有的事件");
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

// ---------------------------------------------------------------------------
// 8. 契约默认实现（additive pattern）
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
fn contract_defaults_are_additive_and_fail_loudly() {
    let mut backend = MinimalBackend;

    // 两端**对称地大声失败**。这是刻意的：若 `snapshot_full` 返回一个
    // 软性的「没有」，最自然的调用写法 `if let Some(dto) = … { … }` 的
    // `None` 分支会**静默跳过**还原，调用方却以为自己续上了确定性状态
    // —— 那正是 D-24 描述的失败模式。
    let err = backend.snapshot_full().expect_err("必须大声失败");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT);

    let err = backend
        .restore_full(&EngineSnapshotDto::default())
        .expect_err("必须大声失败");
    assert_eq!(err.family, ErrorFamily::Snapshot);
    assert_eq!(err.scenario, E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT);
}
