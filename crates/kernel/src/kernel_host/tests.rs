//! M-24?? — kernel host 单元测试。
//!
//! （模块编号：内核宿主**不是** 23 个详细设计模块之一，见模块文档。）
//!
//! 这些用例覆盖 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1 的
//! **规范映射表**：每条命令产生哪些事件、什么顺序、首��是哪个。
//!
//! demo 固件与前端 `front/dev/demo-firmware.js` **逐字节相同**——两份
//! 固件必须一致，否则前端看到的和内核跑的不是同一个程序。

use super::*;

use riscv_sim_contract::model::{ProductTree, ProjectVersion, SimState};

/// demo 固件 —— **引用共享常量**，不抄一份。
///
/// 权威副本在 `kernel_host::DEMO_FIRMWARE_BYTES`（Rust 侧唯一一份），
/// 它与前端 `front/dev/demo-firmware.js` 逐字节相同。
use super::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// demo 固件里的断点靶点地址。
const DEMO_BREAKPOINT_ADDR: u32 = 0x20;

/// demo 固件写进内存的目标地址。
const DEMO_STORE_ADDR: u32 = 0x100;

/// demo 固件写进内存的值（42 + 7）。
const DEMO_STORE_VALUE: u32 = 49;

/// 造一个挂着 demo 固件的工程。
fn demo_project() -> Project {
    Project {
        id: "proj-demo".to_string(),
        name: "demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 取第一条 `Tick`。
fn first_tick(events: &[EventPayload]) -> (u64, u32) {
    for e in events {
        if let EventPayload::Tick { elapsed_ps, pc } = e {
            return (*elapsed_ps, *pc);
        }
    }
    panic!("没有 Tick 事件：{events:?}");
}

/// 取第一条 `Halted`。
fn first_halted(events: &[EventPayload]) -> (HaltReason, u32) {
    for e in events {
        if let EventPayload::Halted { reason, pc } = e {
            return (*reason, *pc);
        }
    }
    panic!("没有 Halted 事件：{events:?}");
}

/// 取第一条 `Error`。
fn first_error(events: &[EventPayload]) -> (String, String) {
    for e in events {
        if let EventPayload::Error { code, message } = e {
            return (code.clone(), message.clone());
        }
    }
    panic!("没有 Error 事件：{events:?}");
}

/// 取第一条 `WaveformDelta` 的流 id 列表。
fn first_waveform_ids(events: &[EventPayload]) -> Vec<String> {
    for e in events {
        if let EventPayload::WaveformDelta(wave) = e {
            return wave.streams.iter().map(|s| s.id.clone()).collect();
        }
    }
    panic!("没有 WaveformDelta 事件：{events:?}");
}

/// 从 `WaveformDelta` 里读一个寄存器的值。
fn waveform_value(events: &[EventPayload], id: &str) -> Option<u64> {
    for e in events {
        if let EventPayload::WaveformDelta(wave) = e {
            for stream in &wave.streams {
                if stream.id == id {
                    return stream.edges.last().map(|edge| edge.value);
                }
            }
        }
    }
    None
}

// -- 命令映射 -------------------------------------------------------------

#[test]
fn nop_produces_a_tick() {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::Nop);
    assert_eq!(events.len(), 1);
    let (_, pc) = first_tick(&events);
    assert_eq!(pc, 0, "空会话的 PC 是复位向量");
}

#[test]
fn load_project_resets_and_clears_the_session() {
    let mut host = KernelHost::new();
    // 先订阅，再装载 —— 装载会清掉订阅（新工程 = 新会话，FR-13）。
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        events.len(),
        1,
        "装载后订阅已被清空，不应再推波形：{events:?}"
    );
    first_tick(&events);
}

#[test]
fn load_project_maps_the_image_at_base_zero() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 第一条指令的机器码必须在 0 处。
    let value = host.read_memory(0, WriteSize::Word).expect("read");
    assert_eq!(value, 0x02A0_0093, "demo 固件必须落在基址 0");
}

#[test]
fn step_n_executes_the_guest_program() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 3 });
    let (_, pc) = first_tick(&events);
    assert_eq!(pc, 0x0c, "3 条指令后 PC 应当是 0x0c");
    assert_eq!(waveform_value(&events, "cpu.gpr.x1"), Some(42), "x1 = 42");
    assert_eq!(waveform_value(&events, "cpu.gpr.x2"), Some(7), "x2 = 7");
    assert_eq!(waveform_value(&events, "cpu.gpr.x3"), Some(49), "x3 = 49");
    // x0 恒为 0，而且**没变**——增量推送里不该出现它。
    assert!(
        waveform_value(&events, "cpu.gpr.x0").is_none(),
        "x0 没变，不该出现在增量里"
    );
}

#[test]
fn step_n_pushes_only_the_dirty_registers() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    // 第一步：x1 变了。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    let ids = first_waveform_ids(&events);
    assert!(ids.contains(&"cpu.gpr.x1".to_string()), "x1 应当出现");
    assert!(
        !ids.contains(&"cpu.gpr.x2".to_string()),
        "x2 没变，不该出现在增量里：{ids:?}"
    );
    // 第二步：x2 变了，x1 没变。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    let ids = first_waveform_ids(&events);
    assert!(ids.contains(&"cpu.gpr.x2".to_string()), "x2 应当出现");
    assert!(
        !ids.contains(&"cpu.gpr.x1".to_string()),
        "x1 没变，不该再出现：{ids:?}"
    );
}

#[test]
fn subscribe_registers_pushes_a_full_snapshot() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    let ids = first_waveform_ids(&events);
    assert_eq!(ids.len(), 33, "全量快照 = 32 条 GPR + 1 条 pc：{ids:?}");
    assert!(ids.contains(&"cpu.pc".to_string()));
}

#[test]
fn store_then_read_memory_round_trips() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 4 }); // 执行到 sw
    let events = dispatch(
        &mut host,
        CommandPayload::ReadMemory {
            addr: DEMO_STORE_ADDR,
            size: WriteSize::Word,
        },
    );
    match &events[0] {
        EventPayload::MemoryRead { addr, size, value } => {
            assert_eq!(*addr, DEMO_STORE_ADDR);
            assert_eq!(*size, WriteSize::Word);
            assert_eq!(*value, DEMO_STORE_VALUE, "sw 写进去的是 42 + 7");
        }
        other => panic!("应当是 MemoryRead：{other:?}"),
    }
}

#[test]
fn read_memory_on_unmapped_address_is_an_error() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 0x5000_0000 不在默认内存图里。
    //
    // ⚠️ 口径：默认内存图**现在**还映射了 GPIO（0x4000_0000，D-46），
    // 所以「未映射」的样例地址换成了 0x5000_0000。原来的 0x4000_0000
    // 现在读得到东西（DIR 寄存器），拿它当「未映射」会让这条用例失去
    // 判别力。
    let events = dispatch(
        &mut host,
        CommandPayload::ReadMemory {
            addr: 0x5000_0000,
            size: WriteSize::Word,
        },
    );
    assert_eq!(events.len(), 1, "错误事件取代正常响应");
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_BUS-01");
}

#[test]
fn write_memory_then_read_back() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0x200,
            size: WriteSize::Word,
            value: 0x1234,
        },
    );
    first_tick(&events);
    let value = host.read_memory(0x200, WriteSize::Word).expect("read");
    assert_eq!(value, 0x1234);
}

#[test]
fn write_memory_to_unmapped_address_is_an_error() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 0x5000_0000 不在默认内存图里（0x4000_0000 现在是 GPIO，见
    // `read_memory_on_unmapped_address_is_an_error` 的口径说明）。
    let events = dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0x5000_0000,
            size: WriteSize::Word,
            value: 1,
        },
    );
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_BUS-01");
}

// -- 断点 -----------------------------------------------------------------

#[test]
fn breakpoint_halts_execution_at_its_address() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
        }),
    );
    first_tick(&events);

    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, pc) = first_halted(&events);
    assert_eq!(reason, HaltReason::Breakpoint);
    assert_eq!(pc, DEMO_BREAKPOINT_ADDR, "断点命中时 PC 停在断点地址");
}

#[test]
fn breakpoint_does_not_modify_the_machine_state() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
        }),
    );
    // 跑到断点前（5 条指令：0x00..0x10）。
    dispatch(&mut host, CommandPayload::StepN { n: 5 });
    let before = host.read_memory(0, WriteSize::Word).expect("read");
    dispatch(&mut host, CommandPayload::Continue);
    let after = host.read_memory(0, WriteSize::Word).expect("read");
    assert_eq!(before, after, "命中断点不得改写内存");
}

#[test]
fn misaligned_breakpoint_is_rejected() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: 0x21,
            source_hint: None,
            enabled: true,
        }),
    );
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_CTX-07", "未对齐的断点地址必须被拒绝");
}

#[test]
fn duplicate_breakpoint_is_rejected() {
    let mut host = KernelHost::new();
    let bp = riscv_sim_contract::model::Breakpoint {
        id: 1,
        addr: DEMO_BREAKPOINT_ADDR,
        source_hint: None,
        enabled: true,
    };
    dispatch(&mut host, CommandPayload::SetBreakpoint(bp.clone()));
    let events = dispatch(&mut host, CommandPayload::SetBreakpoint(bp));
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_CTX-07", "重复注册必须被拒绝");
}

#[test]
fn clear_breakpoint_with_unknown_id_is_idempotent() {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::ClearBreakpoint { id: 999 });
    assert_eq!(events.len(), 1);
    first_tick(&events);
}

#[test]
fn cleared_breakpoint_no_longer_halts() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 7,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
        }),
    );
    dispatch(&mut host, CommandPayload::ClearBreakpoint { id: 7 });
    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, _) = first_halted(&events);
    assert_eq!(
        reason,
        HaltReason::EndOfExecution,
        "断点被清掉后只能跑到预算上限"
    );
}

// -- 其它命令 -------------------------------------------------------------

#[test]
fn pause_reports_a_user_pause_halt() {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::Pause);
    let (reason, _) = first_halted(&events);
    assert_eq!(reason, HaltReason::UserPause);
}

#[test]
fn snapshot_returns_an_increasing_id() {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::Snapshot);
    match &events[0] {
        EventPayload::SnapshotCreated { id } => assert_eq!(*id, 1),
        other => panic!("应当是 SnapshotCreated：{other:?}"),
    }
    let events = dispatch(&mut host, CommandPayload::Snapshot);
    match &events[0] {
        EventPayload::SnapshotCreated { id } => assert_eq!(*id, 2),
        other => panic!("应当是 SnapshotCreated：{other:?}"),
    }
}

#[test]
fn admin_is_not_enabled() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::Admin {
            op: riscv_sim_contract::protocol::AdminOp::DrainLogs,
        },
    );
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_CTX-05");
}

#[test]
fn unsubscribe_stops_the_register_pushes() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    dispatch(
        &mut host,
        CommandPayload::Unsubscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::WaveformDelta(_))),
        "退订之后不再推波形：{events:?}"
    );
}

#[test]
fn fetch_from_unmapped_memory_is_reported_as_an_error() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 把 PC 挪到未映射地址（契约的 `Restore` 带完整 `SimState`）。
    //
    // ⚠️ 用 0x5000_0000 而不是 0x4000_0000：后者现在是 GPIO 的窗口，
    // 取指会**成功**（读回 DIR = 0），这条用例要验的是「取指失败」，
    // 拿一个能取到的地址去验就验错了东西。
    let state = SimState {
        pc: 0x5000_0000,
        ..SimState::default()
    };
    dispatch(
        &mut host,
        CommandPayload::Restore {
            state: Box::new(state),
        },
    );
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_ISS-01", "取指失败由 M-13 包成 ISS 族错误");
    let (reason, _) = first_halted(&events);
    assert_eq!(reason, HaltReason::Trap);
}

// -- JSON 桥 --------------------------------------------------------------

#[test]
fn handle_json_returns_a_json_array_of_events() {
    let mut host = KernelHost::new();
    let json = serde_json::to_string(&Message::command(1, CommandPayload::Nop)).unwrap();
    let out = host.handle_json(&json);
    let parsed: Vec<Message> = serde_json::from_str(&out).expect("出参必须是事件数组");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].direction, Direction::Event);
    assert_eq!(parsed[0].seq, 1, "内核自己的序号从 1 起");
    assert!(matches!(parsed[0].payload, Payload::Event(_)));
}

#[test]
fn handle_json_numbers_the_events_monotonically() {
    let mut host = KernelHost::new();
    let json = serde_json::to_string(&Message::command(1, CommandPayload::Nop)).unwrap();
    let out = host.handle_json(&json);
    let parsed: Vec<Message> = serde_json::from_str(&out).unwrap();
    let out2 = host.handle_json(&json);
    let parsed2: Vec<Message> = serde_json::from_str(&out2).unwrap();
    assert_eq!(parsed2[0].seq, parsed[0].seq + 1, "序号必须单调递增");
}

/// 从一条契约消息里取出事件载荷。
fn payload_events(msg: &Message) -> EventPayload {
    match &msg.payload {
        Payload::Event(e) => e.clone(),
        Payload::Command(_) => panic!("不是事件消息"),
    }
}

#[test]
fn handle_json_on_garbage_returns_an_error_event() {
    let mut host = KernelHost::new();
    let out = host.handle_json("{not json");
    let parsed: Vec<Message> = serde_json::from_str(&out).expect("出参仍然是合法 JSON 数组");
    assert_eq!(parsed.len(), 1);
    let (code, _) = first_error(&[payload_events(&parsed[0])]);
    assert_eq!(code, "E_PROTO-08");
}

#[test]
fn handle_json_rejects_an_event_direction_message() {
    let mut host = KernelHost::new();
    let json = serde_json::to_string(&Message::event(
        1,
        EventPayload::Tick {
            elapsed_ps: 0,
            pc: 0,
        },
    ))
    .unwrap();
    let out = host.handle_json(&json);
    let parsed: Vec<Message> = serde_json::from_str(&out).unwrap();
    let (code, _) = first_error(&[payload_events(&parsed[0])]);
    assert_eq!(code, "E_PROTO-08");
}

#[test]
fn handle_json_rejects_an_old_schema_version() {
    let mut host = KernelHost::new();
    let mut msg = Message::command(1, CommandPayload::Nop);
    msg.schema_version = 0;
    let json = serde_json::to_string(&msg).unwrap();
    let out = host.handle_json(&json);
    let parsed: Vec<Message> = serde_json::from_str(&out).unwrap();
    let (code, _) = first_error(&[payload_events(&parsed[0])]);
    assert_eq!(code, "E_PROTO-01");
}

#[test]
fn handle_json_never_panics_on_a_hostile_input() {
    let mut host = KernelHost::new();
    for junk in [
        "",
        "null",
        "[]",
        "{}",
        r#"{"schema_version":1,"direction":"Command","seq":1,"payload":{"Command":"Nope"}}"#,
        r#"{"schema_version":1,"direction":"Command","seq":1,"payload":{"Command":{"StepN":{"n":-1}}}}"#,
        r#"{"schema_version":1,"direction":"Command","seq":1,"payload":{"Command":{"ReadMemory":{"addr":4294967295,"size":"Word"}}}}"#,
    ] {
        let out = host.handle_json(junk);
        let parsed: Vec<Message> = serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("输入 {junk:?} 的出参不是合法 JSON 数组：{e}"));
        assert!(!parsed.is_empty(), "输入 {junk:?} 至少要回一个事件");
    }
}

// -- V-A 检视闭环（🟡-1 / 🟡-4 / 🟡-5 / 🟡-6 / ⚪-6）-----------------------

#[test]
fn read_memory_command_has_no_side_effect_on_the_bus() {
    // 回归 V-A 🟡-1：命令层的「ReadMemory 不产生副作用」此前**没有**
    // 任何用例守护——把命令层改成走普通 `bus.read()`（进统计、进 hook），
    // 全部测试仍然绿，而调试器的 peek 会开始误触发读观察点。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let before = host.backend.bus().stats().reads;
    let events = dispatch(
        &mut host,
        CommandPayload::ReadMemory {
            addr: DEMO_STORE_ADDR,
            size: WriteSize::Word,
        },
    );
    assert_eq!(
        host.backend.bus().stats().reads,
        before,
        "调试读不得计入总线统计"
    );
    // 命令确实读到了值。
    match &events[0] {
        EventPayload::MemoryRead { addr, value, .. } => {
            assert_eq!(*addr, DEMO_STORE_ADDR);
            assert_eq!(*value, 0, "刚装载时那个位置还没有被写过");
        }
        other => panic!("ReadMemory 应当回 MemoryRead，实际 {other:?}"),
    }
}

#[test]
fn a_debugger_read_does_not_fire_a_read_watchpoint() {
    // 回归 V-A 🟡-1 的第二半：调试读命中了一个读观察点之后，
    // **下一条** guest 指令不得误报观察点命中。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetWatchpoint(riscv_sim_contract::model::Watchpoint {
            id: 1,
            addr: DEMO_STORE_ADDR,
            size: WriteSize::Word,
            access: riscv_sim_contract::model::WatchAccess::Read,
        }),
    );
    // 调试器读一次被监视的地址。
    dispatch(
        &mut host,
        CommandPayload::ReadMemory {
            addr: DEMO_STORE_ADDR,
            size: WriteSize::Word,
        },
    );
    // 接下来跑一条指令：不得因为刚才那次调试读而假停。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    assert!(
        !events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Watchpoint,
                ..
            }
        )),
        "调试读不得让下一条 guest 指令误报观察点：{events:?}"
    );
}

#[test]
fn a_breakpoint_run_emits_tick_then_waveform_then_halted() {
    // 回归 V-A 🟡-4：桥规格 §3.1 规定「事件序列（有序）」，且前端靠
    // 「**首个**事件携带 reqId」关联命令——顺序不是可有可无的装饰。
    // 此前所有用例都只「找第一条某类型事件」，把 Halted 挪到前面也没人管。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: "sim.registers".to_string(),
        },
    );
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            EventPayload::Tick { .. } => "Tick",
            EventPayload::WaveformDelta(_) => "WaveformDelta",
            EventPayload::Halted { .. } => "Halted",
            EventPayload::Error { .. } => "Error",
            _ => "其它",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["Tick", "WaveformDelta", "Halted"],
        "命中断点时的事件顺序必须是 Tick → WaveformDelta → Halted"
    );
}

#[test]
fn an_image_that_runs_past_the_mapped_region_is_rejected() {
    // 回归 V-A 🟡-5：`load_bytes` 是镜像装载的**唯一**写路径，越界必须
    // 报 E_BUS-01。此前把越界错误吞掉，全部测试仍然绿。
    let mut host = KernelHost::new();
    let mut project = demo_project();
    // 低段 RAM 是 1 MiB —— 镜像比它大一个字节就会越界。
    let mut oversized = vec![0u8; 0x10_0000 + 1];
    oversized[0] = 0x93;
    project.images[0].source = ImageSource::Bytes(oversized);
    let events = dispatch(&mut host, CommandPayload::LoadProject(project));
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_BUS-01", "越界的镜像必须被拒绝");
}

#[test]
fn restore_rejects_a_state_with_a_foreign_schema_version() {
    // 回归 V-A 🟡-6：契约的 `SimState` 带 `schema_version`，但
    // `Engine::restore` 不校验它——一份版本错配的快照会被**静默接受**，
    // 把 PC 与整份寄存器换掉，而调用方连一个错都看不到。
    let mut host = KernelHost::new();
    let state = SimState {
        schema_version: SIM_STATE_SCHEMA_VERSION + 1,
        ..SimState::default()
    };
    let events = dispatch(
        &mut host,
        CommandPayload::Restore {
            state: Box::new(state),
        },
    );
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_SNAP-05", "版本不符的快照必须被拒绝");
}

#[test]
fn restore_accepts_a_state_with_the_current_schema_version() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::Restore {
            state: Box::new(SimState::default()),
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::Tick { .. })),
        "版本相符的快照应当被接受：{events:?}"
    );
}

#[test]
fn an_envelope_from_a_newer_schema_is_refused() {
    // 回归 V-A ⚪-6：只拒「版本更低」的话，一个来自更高版本 schema 的
    // 消息会被**静默地**少解析几个字段（契约的 serde 派生没有
    // `deny_unknown_fields`）——那比直接拒绝糟得多。
    let mut host = KernelHost::new();
    let mut msg = Message::command(1, CommandPayload::Nop);
    msg.schema_version = SCHEMA_VERSION + 1;
    let events = host.handle_message(&msg);
    let (code, _) = first_error(&events);
    assert_eq!(code, "E_PROTO-01", "版本更高的信封必须被拒绝");
}
