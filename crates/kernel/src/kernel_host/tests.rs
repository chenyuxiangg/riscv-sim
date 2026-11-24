//! M-24?? — kernel host 单元测试。
//!
//! （模块编号：内核宿主**不是** 23 个详细设计模块之一，见模块文档。）
//!
//! 这些用例覆盖 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1 的
//! **规范映射表**：每条命令产生哪些事件、什么顺序、首个是哪个。
//!
//! demo 固件与前端 `front/dev/demo-firmware.js` **逐字节相同**——两份
//! 固件必须一致，否则前端看到的和内核跑的不是同一个程序。

use super::*;

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, ProjectVersion, SimState, WriteSize,
};
use riscv_sim_contract::protocol::{Direction, HaltReason, Message, Payload};
use riscv_sim_contract::SCHEMA_VERSION;

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
        if let EventPayload::Halted { reason, pc, .. } = e {
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

/// 取第一条 `WaveformHistory` 的流 id 列表。
fn first_waveform_ids(events: &[EventPayload]) -> Vec<String> {
    for e in events {
        if let EventPayload::WaveformHistory(wave) = e {
            return wave.streams.iter().map(|s| s.id.clone()).collect();
        }
    }
    panic!("没有 WaveformHistory 事件：{events:?}");
}

/// 从 `WaveformHistory` 里读一个寄存器的值。
fn waveform_value(events: &[EventPayload], id: &str) -> Option<u64> {
    for e in events {
        if let EventPayload::WaveformHistory(wave) = e {
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
fn state_delta_subscription_emits_full_then_incremental_register_state() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 订阅先给全量：32 个 GPR + PC。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: "sim.state.delta".to_string(),
        },
    );
    let full = events
        .iter()
        .find_map(|event| match event {
            EventPayload::SimStateDelta { registers, pc, .. } => Some((registers.clone(), *pc)),
            _ => None,
        })
        .expect("订阅 sim.state.delta 必须回一条全量 SimStateDelta");
    assert_eq!(full.0.len(), 32, "首条 SimStateDelta 必须是全量寄存器");
    assert!(full.1.is_some(), "首条 SimStateDelta 必须带 PC");

    // 跑一条：增量只带变过的寄存器 / PC。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    let delta = events
        .iter()
        .find_map(|event| match event {
            EventPayload::SimStateDelta { registers, pc, .. } => Some((registers.clone(), *pc)),
            _ => None,
        })
        .expect("步进后必须回一条 SimStateDelta");
    assert!(delta.0.len() < 32, "增量又把 32 个寄存器全发了一遍");
    assert!(delta.1.is_some(), "PC 变了却没有带 pc");
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // ⚠️ **v0.144（契约 v19）**：装载**成功**多一条回执 ⇒ ⇒ 事件从
    //   1 条变 2 条（`ImageLoaded` + `Tick`）⇒ ⇒ 「订阅被清空」这条判据
    //   **不变**（波形仍然一条都不该有）。
    assert_eq!(
        events.len(),
        2,
        "装载后应是「回执 + tick」，且订阅已被清空、不应再推波形：{events:?}"
    );
    match &events[0] {
        EventPayload::ImageLoaded { .. } => {}
        other => panic!("装载回执必须排第一，实际是 {other:?}"),
    }
    first_tick(&events[1..]);
}

/// **R-04 的装载回执**（契约 v19）：成功装载 ⇒ 一条 `ImageLoaded`，
/// 报告的就是**真写进总线**的那批段。
///
#[test]
fn load_project_emits_an_image_loaded_receipt() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let receipt = events
        .iter()
        .find_map(|event| match event {
            EventPayload::ImageLoaded {
                name,
                kind,
                entry,
                segments,
                elf_bits,
                has_rvc,
                platform_revision,
            } => Some((
                name.clone(),
                kind.clone(),
                *entry,
                segments.clone(),
                *elf_bits,
                *has_rvc,
                *platform_revision,
            )),
            _ => None,
        })
        .expect("成功装载必须回一条 ImageLoaded");
    assert_eq!(receipt.0, "demo-firmware.bin", "name 要回显 ImageRef.name");
    assert_eq!(receipt.1, ImageKind::RawHex, "kind 与请求同源");
    assert_eq!(receipt.2, 0, "raw blob 的 entry 恒为 0");
    assert_eq!(receipt.4, 0, "非 ELF ⇒ elf_bits = 0");
    assert!(!receipt.5, "非 ELF ⇒ 没有 RVC 事实");
    // 段就是那块镜像，**且只报形状**。
    assert_eq!(receipt.3.len(), 1, "一个镜像一段：{:?}", receipt.3);
    assert_eq!(receipt.3[0].base, 0, "raw blob 落在基址 0");
    assert_eq!(
        receipt.3[0].size,
        super::DEMO_FIRMWARE_BYTES.len() as u32,
        "size 必须等于真正写进总线的字节数"
    );
    // 修订号对得上宿主现在的图。
    assert_eq!(receipt.6, host.map_revision(), "platform_revision 要能对回当前图");
}

/// **空工程不发抖**（契约 v19）：配板用的 `images: []` **不是**一次装载
/// ⇒ ⇒ 一条回执都不该有（否则「装了什么 = 什么也没装」）。
#[test]
fn an_empty_project_does_not_emit_an_image_loaded_receipt() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    let mut project = demo_project();
    project.images.clear();
    let events = dispatch(&mut host, CommandPayload::LoadProject(project));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::ImageLoaded { .. })),
        "空工程不该有装载回执：{events:?}"
    );
    // ⚠️ 空工程**也不装镜像** ⇒ ⇒ 机器上一条指令都没有 ⇒ ⇒ PC 停在 0
    //   （「已装载」这条判据的覆盖在 `image_loaded` 自己的用例里，这里只
    //   钉「不发回执」这一件事）。
    assert_eq!(host.pc(), 0, "空工程不该把机器带起来");
}

#[test]
fn load_project_maps_the_image_at_base_zero() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 第一条指令的机器码必须在 0 处。
    let value = host.read_memory(0, WriteSize::Word).expect("read");
    assert_eq!(value, 0x02A0_0093, "demo 固件必须落在基址 0");
}

#[test]
fn step_n_executes_the_guest_program() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
            condition: None,
            ..riscv_sim_contract::model::Breakpoint::default()
        }),
    );
    first_tick(&events);

    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, pc) = first_halted(&events);
    assert_eq!(reason, HaltReason::Breakpoint);
    assert_eq!(pc, DEMO_BREAKPOINT_ADDR, "断点命中时 PC 停在断点地址");
}

/// **S-2**（STAGE-55）：**观察式条件断点**不绑地址，按指令步求值；
/// 命中即停，且停止原因**复用 `Watchpoint`**（不扩 `HaltReason` 枚举）。
#[test]
fn s2_a_condition_breakpoint_halts_at_the_first_instruction_boundary() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 5,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: riscv_sim_contract::model::BreakpointKind::Condition,
            condition: Some("pc >= 0".to_string()),
        }),
    );
    first_tick(&events);
    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, pc) = first_halted(&events);
    assert_eq!(
        reason,
        HaltReason::Watchpoint,
        "观察式命中复用 Watchpoint，不扩枚举"
    );
    assert_eq!(pc, 0, "停在第一条指令的取指之前（一条都没执行）");
}

/// **S-3**（STAGE-55）：谓词**由假变真**时才停 —— 第 0 条指令把 `x1`
/// 改成 42，所以在**下一条**指令边界（`0x04`）命中。
#[test]
fn s3_a_condition_breakpoint_halts_once_the_predicate_turns_true() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 6,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: riscv_sim_contract::model::BreakpointKind::Condition,
            condition: Some("x1 == 42".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    let (reason, pc) = first_halted(&events);
    assert_eq!(reason, HaltReason::Watchpoint);
    assert_eq!(pc, 0x04, "x1 在第 0 条指令后变成 42 ⇒ 在 0x04 停");
}

/// **S-7**（STAGE-55 批 C）：含内存读的谓词在 **store 边界**判 ——
/// demo 固件第 3 条指令（`0x0c`）把 `x3 = 49` 存到 `0x100` ⇒ ⇒ 谓词
/// `[0x100] == 49` 在**写生效之后**为真 ⇒ ⇒ 停在 `0x10`，并带回「是哪条
/// store 触发的」。
#[test]
fn s7_a_memory_condition_halts_at_the_store_boundary_with_the_trigger_address() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 7,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: riscv_sim_contract::model::BreakpointKind::Condition,
            // ⚠️ **写后新值**：旧值（0）会让它假 ⇒ ⇒ 只有「提交之后求值」
            //    这一条路能得到真。
            condition: Some("[0x100] == 49".to_string()),
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    let halted = events
        .iter()
        .find_map(|e| match e {
            EventPayload::Halted {
                reason,
                pc,
                trigger_addr,
            } => Some((*reason, *pc, *trigger_addr)),
            _ => None,
        })
        .expect("store 边界命中必须停");
    assert_eq!(
        halted.0,
        HaltReason::Watchpoint,
        "观察式命中复用 Watchpoint"
    );
    assert_eq!(halted.1, 0x10, "指令已提交 ⇒ PC 已是下一条");
    assert_eq!(
        halted.2,
        Some(0x0c),
        "必须带回触发那次写所属的指令地址（PC 自己已经说不清了）"
    );
}

/// **S-8**（STAGE-55 批 C）：注册期**求不出来**的表达式 —— **允许注册**，
/// 回执里带降级标记（不是拒绝，也不是静默当假）。
#[test]
fn s8_an_unreadable_memory_condition_registers_as_degraded() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 0xDEAD_0000 不在映射表里 ⇒ 现在读不到。
    let events = dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 8,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: riscv_sim_contract::model::BreakpointKind::Condition,
            condition: Some("[0xdead0000] == 1".to_string()),
        }),
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "求值期才可能失败 ⇒ 注册期不该拒"
    );
    let events = dispatch(&mut host, CommandPayload::ListBreakpoints);
    let info = events
        .iter()
        .find_map(|e| match e {
            EventPayload::Breakpoints { breakpoints } => breakpoints
                .iter()
                .find(|info| info.breakpoint.id == 8)
                .cloned(),
            _ => None,
        })
        .expect("降级条目仍然在表里");
    assert!(info.degraded, "回执要带降级标记（界面据此提示）");
}

/// **S-9**（STAGE-55 批 C）：**区间过滤** — 谓词读 `0x104`（= 0，恒真），
/// 而 `0x0c` 的 store 只写 `0x100` ⇒ ⇒ 写区间不相交 ⇒ 这次 store **不**
/// 求值 ⇒ ⇒ 不许在 `0x10` 停下。
#[test]
fn s9_a_disjoint_store_does_not_evaluate_the_predicate() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 9,
            addr: 0,
            source_hint: None,
            enabled: true,
            kind: riscv_sim_contract::model::BreakpointKind::Condition,
            condition: Some("[0x104] == 0".to_string()),
        }),
    );
    // 跑过 `0x0c` 那条 store（4 条指令）——若过滤失效，会在 `0x10` 停。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 4 });
    let halted = events.iter().find_map(|e| match e {
        EventPayload::Halted { pc, .. } => Some(*pc),
        _ => None,
    });
    assert_eq!(
        halted, None,
        "写 `0x100` 与谓词读的 `0x104` 不相交 ⇒ 这次 store 不该求值"
    );
}

/// **S-4**（STAGE-55）：观察式条目的 id **由内核分配** —— 宿主连发两条
/// `id = 0` 的条件断点，两条都要落表，且各自的 id **互不相同**、也**不等于
/// 0**（地址类的「一址一断点」命名空间不受影响）。
#[test]
fn s4_condition_breakpoints_with_id_zero_are_allocated_by_the_kernel() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    for expr in ["x1 == 42", "x2 == 7"] {
        let events = dispatch(
            &mut host,
            CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
                id: 0,
                addr: 0,
                source_hint: None,
                enabled: true,
                kind: riscv_sim_contract::model::BreakpointKind::Condition,
                condition: Some(expr.to_string()),
            }),
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EventPayload::Error { .. })),
            "id = 0 应当被当成「请内核分配」，而不是「id 已被占用」：{expr}"
        );
    }
    let events = dispatch(&mut host, CommandPayload::ListBreakpoints);
    let infos: Vec<_> = events
        .iter()
        .find_map(|e| match e {
            EventPayload::Breakpoints { breakpoints } => Some(breakpoints.clone()),
            _ => None,
        })
        .expect("ListBreakpoints 必须回一张表");
    assert_eq!(infos.len(), 2, "两条条件断点都必须在表里");
    let mut ids: Vec<u32> = infos.iter().map(|i| i.breakpoint.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 2, "两条条件断点的 id 必须互不相同");
    assert!(ids.iter().all(|&id| id != 0), "内核不能把 0 当 id 发出去");
    assert!(
        infos
            .iter()
            .all(|i| i.breakpoint.kind == riscv_sim_contract::model::BreakpointKind::Condition),
        "落表的来源标记必须还是「条件」"
    );
}

#[test]
fn breakpoint_does_not_modify_the_machine_state() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 1,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
            condition: None,
            ..riscv_sim_contract::model::Breakpoint::default()
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
            condition: None,
            ..riscv_sim_contract::model::Breakpoint::default()
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
        condition: None,
        ..riscv_sim_contract::model::Breakpoint::default()
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetBreakpoint(riscv_sim_contract::model::Breakpoint {
            id: 7,
            addr: DEMO_BREAKPOINT_ADDR,
            source_hint: None,
            enabled: true,
            condition: None,
            ..riscv_sim_contract::model::Breakpoint::default()
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
            .any(|e| matches!(e, EventPayload::WaveformHistory(_))),
        "退订之后不再推波形：{events:?}"
    );
}

#[test]
fn fetch_from_unmapped_memory_is_reported_as_a_trap() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ STAGE-10 裁决 B / ledger **D-103**：未映射取指现在是**架构陷阱**
    // （`mcause = 1`），不是宿主侧的错误 ⇒ 宿主报「停在一个陷阱上」，
    // **不再**报 `E_ISS-01` 错误事件。
    let (reason, _) = first_halted(&events);
    assert_eq!(reason, HaltReason::Trap);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "未映射取指是 guest 的陷阱，不是宿主错误：{events:?}"
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
            condition: None,
            ..riscv_sim_contract::model::Breakpoint::default()
        }),
    );
    let events = dispatch(&mut host, CommandPayload::Continue);
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            EventPayload::Tick { .. } => "Tick",
            EventPayload::WaveformHistory(_) => "WaveformHistory",
            EventPayload::Halted { .. } => "Halted",
            EventPayload::Error { .. } => "Error",
            _ => "其它",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["Tick", "WaveformHistory", "Halted"],
        "命中断点时的事件顺序必须是 Tick → WaveformHistory → Halted"
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
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
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
