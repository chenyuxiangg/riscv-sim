//! D-46 —— **探针管道接线**的端到端验证。
//!
//! 第 1 阶段结束时波形视图是**空的**：真实的探针管道（M-18 判沿 /
//! M-19 环形缓冲）根本没接到 M-14 的总线 hook 上，台账 **D-46**。
//! 本文件证明这条链路现在是**真的**通的：
//!
//! ```text
//! guest 写 GPIO0.OUT
//!    → M-14 总线派发 → M-15 GpioBank 判「电平真变了」→ report("OUT")
//!    → M-18 ProbeRegistry 判沿 → 边沿进 M-19 的环（EdgeSink）
//!    → kernel_host 取边沿 → 契约 EventPayload::WaveformDelta
//!    → 前端波形视图列出通道
//! ```
//!
//! ## 这些用例在防什么
//!
//! 每一��都对应一个**曾经可能悄悄坏掉**的接缝：
//!
//! - 通道**列表**必须推得出来 —— 「有这条信号但还没变化」和「根本没这条
//!   信号」是两回事，只靠边沿表达不出来；
//! - 边沿的**值**必须是真的 —— 不是 mock 凭空造的 `probe.*` 流；
//! - 边沿的**时间戳**必须不是 0 —— 时间轴平了等于半个假象；
//! - `LoadProject` 之后管道**必须还在** —— 换总线会带走总线上的 hook；
//! - 观察点 hook 与探针 hook **必须共存** —— 单槽 hook 会互相顶掉。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, Waveform, WriteSize,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};
use riscv_sim_kernel::kernel_host::{
    KernelHost, DEFAULT_GPIO_BASE, GPIO_PERIPH_ID, GPIO_SIGNAL_NAME, TOPIC_WAVEFORM,
};
use riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS;

/// demo 固件 —— **引用共享常量**，不抄一份。
///
/// Rust 侧的权威副本是 `kernel_host::DEMO_FIRMWARE_BYTES`；它与前端
/// `front/dev/demo-firmware.js` 逐字节相同（下面那条防漂移用例直接读
/// 那份 JS 比对）。
use riscv_sim_kernel::kernel_host::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// demo 固件写的 GPIO 通道 id（M-18 的 `"{source}.{signal}"`）。
const GPIO_STREAM_ID: &str = "GPIO0.OUT";

/// 固件里那条 `sw x6, 4(x5)` 所在的指令序号（1 起）——第 8 条。
const GPIO_WRITE_INSTRUCTION: u32 = 8;

/// 造一个挂着 demo 固件的工程。
fn demo_project() -> Project {
    Project {
        id: "proj-d46".to_string(),
        name: "d46".to_string(),
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

/// 从一批事件里挑出**最后一条**探针波形（`WaveformDelta`）。
///
/// ⚠️ 必须挑「最后一条」：一次 `Subscribe` 会推两条波形事件——寄存器
/// 全量快照 + 探针通道列表。挑第一条会拿到寄存器那条，断言就落空了。
fn last_probe_waveform(events: &[EventPayload]) -> Option<Waveform> {
    events.iter().rev().find_map(|e| match e {
        EventPayload::WaveformDelta(wave) => Some(wave.clone()),
        _ => None,
    })
}

/// 从一批事件里挑出**所有**探针波形。
fn all_waveforms(events: &[EventPayload]) -> Vec<Waveform> {
    events
        .iter()
        .filter_map(|e| match e {
            EventPayload::WaveformDelta(wave) => Some(wave.clone()),
            _ => None,
        })
        .collect()
}

/// 在波形里找一条流。
fn stream<'a>(
    wave: &'a Waveform,
    id: &str,
) -> Option<&'a riscv_sim_contract::model::WaveformStream> {
    wave.streams.iter().find(|s| s.id == id)
}

/// 装载 demo 工程 + 订阅探针流，返回宿主。
fn host_with_probe_subscription() -> KernelHost {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    host
}

// -- 通道列表 ---------------------------------------------------------------

#[test]
fn subscribing_lists_the_gpio_channel_even_before_any_edge() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    let wave = last_probe_waveform(&events).expect("订阅后必须推一条波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        0,
        "还没跑，通道应当是空的——但**通道必须在**"
    );
}

#[test]
fn the_channel_id_is_the_peripheral_id_plus_the_signal_name() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    let wave = last_probe_waveform(&events).expect("订阅后必须推一条波形");
    let ids: Vec<&str> = wave.streams.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![GPIO_STREAM_ID],
        "通道 id 必须是 `{{periph id}}.{{signal}}`：{ids:?}"
    );
    assert_eq!(
        GPIO_STREAM_ID,
        format!("{GPIO_PERIPH_ID}.{GPIO_SIGNAL_NAME}"),
        "通道 id 的两段必须真的是外设 id 与信号名"
    );
}

#[test]
fn no_probe_waveform_is_pushed_when_the_topic_is_not_subscribed() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 没订阅 `sim.waveform` —— 不该有探针波形。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 8 });
    assert!(
        all_waveforms(&events).is_empty(),
        "没订阅就不该推波形：{events:?}"
    );
}

// -- 真实边沿 ---------------------------------------------------------------

#[test]
fn the_guest_write_to_gpio_produces_a_real_edge() {
    let mut host = host_with_probe_subscription();
    // 跑到固件的第 8 条指令（`sw x6, 4(x5)`）之后。
    let events = dispatch(
        &mut host,
        CommandPayload::StepN {
            n: GPIO_WRITE_INSTRUCTION,
        },
    );
    let wave = last_probe_waveform(&events).expect("跑完固件必须推一条探针波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        1,
        "固件写了一次 OUT，应当恰好一条边沿：{:?}",
        gpio.edges
    );
    assert_eq!(gpio.edges[0].value, 1, "边沿的值必须是固件写进去的引脚位图");
}

#[test]
fn the_edge_carries_a_real_timestamp() {
    let mut host = host_with_probe_subscription();
    let events = dispatch(
        &mut host,
        CommandPayload::StepN {
            n: GPIO_WRITE_INSTRUCTION,
        },
    );
    let wave = last_probe_waveform(&events).expect("跑完固件必须推一条探针波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    let t_ps = gpio.edges[0].t_ps;
    assert!(
        t_ps > 0,
        "边沿的时间戳不能是 0——外设时钟没同步上，时间轴就是平的"
    );
    // 第 8 条指令开始时，虚拟时钟已经走过 7 条指令。
    assert_eq!(
        t_ps,
        7 * INSTRUCTION_COST_PS,
        "边沿的时间戳应当等于该条指令开始时的虚拟时间"
    );
}

#[test]
fn a_second_run_of_the_same_value_produces_no_new_edge() {
    let mut host = host_with_probe_subscription();
    dispatch(
        &mut host,
        CommandPayload::StepN {
            n: GPIO_WRITE_INSTRUCTION,
        },
    );
    // 固件接着自环（0x24 `jal x0, 0`），不再动 GPIO。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 4 });
    let waves = all_waveforms(&events);
    let total_edges: usize = waves
        .iter()
        .flat_map(|w| w.streams.iter())
        .map(|s| s.edges.len())
        .sum();
    assert_eq!(total_edges, 0, "电平没变就不该再产生边沿：{events:?}");
}

#[test]
fn the_gpio_window_is_mapped_in_the_default_memory_map() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 读 GPIO 的 DIR 寄存器（偏移 0）——固件没配过方向，应当是 0，
    // 但**读得到**本身就说明窗口映射好了。
    let value = host
        .read_memory(DEFAULT_GPIO_BASE, WriteSize::Word)
        .expect("GPIO 窗口必须已映射");
    assert_eq!(value, 0, "GPIO 的 DIR 寄存器默认是 0");
}

// -- 装载工程之后管道还在 ---------------------------------------------------

#[test]
fn load_project_reinstalls_the_probe_pipeline() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    // 再装一次工程 —— 换了一条**全新**的总线。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 重新订阅（装载会清掉内核侧的主题）。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    let wave = last_probe_waveform(&events).expect("订阅后必须推一条波形");
    assert!(
        stream(&wave, GPIO_STREAM_ID).is_some(),
        "装载工程之后探针管道必须还在：{events:?}"
    );
    // 而且**真的能产生边沿** —— 通道在但回调没装上，是另一种坏法。
    let events = dispatch(
        &mut host,
        CommandPayload::StepN {
            n: GPIO_WRITE_INSTRUCTION,
        },
    );
    let wave = last_probe_waveform(&events).expect("跑完固件必须推一条探针波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        1,
        "装载工程之后边沿必须还能产生：{:?}",
        gpio.edges
    );
}

// -- 与观察点共存 -----------------------------------------------------------

#[test]
fn the_watchpoint_hook_and_the_probe_hook_coexist() {
    let mut host = host_with_probe_subscription();
    // 观察点挂在 0x100 —— demo 固件第 4 条指令写的就是它。
    dispatch(
        &mut host,
        CommandPayload::SetWatchpoint(riscv_sim_contract::model::Watchpoint {
            id: 1,
            addr: 0x100,
            size: WriteSize::Word,
            access: riscv_sim_contract::model::WatchAccess::Write,
        }),
    );
    // 前 4 条指令：观察点应当命中（访存发生在指令内部，命中槽在指令
    // 返回后消费）。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 4 });
    assert!(
        events.iter().any(
            |e| matches!(e, EventPayload::Halted { reason, .. } if *reason
                == riscv_sim_contract::protocol::HaltReason::Watchpoint)
        ),
        "观察点必须仍然命中——总线 hook 被探针的顶掉了吗：{events:?}"
    );
    // 再跑 4 条：固件写 GPIO，探针边沿必须照常产生。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 4 });
    let wave = last_probe_waveform(&events).expect("跑完固件必须推一条探针波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        1,
        "探针 hook 必须仍然工作——总线 hook 被观察点的顶掉了吗：{:?}",
        gpio.edges
    );
}

// -- 两份 demo 固件不许漂移 -------------------------------------------------

#[test]
fn the_rust_demo_firmware_matches_the_frontend_copy() {
    // 前端那份是**手写字节**，Rust 这份也是。两份一旦漂移，前端看到的
    // 和内核跑的就不是同一个程序——而两边各自都有测试，绿着测不同的
    // 固件。所以这里直接读那份 JS 比对。
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../front/dev/demo-firmware.js"
    );
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读不到 {path}：{e}"));
    let start = text
        .find("DEMO_FIRMWARE_BYTES = Object.freeze([")
        .expect("前端固件文件里找不到字节数组");
    let body = &text[start..];
    let end = body.find("]);").expect("字节数组没有闭合");
    let body = &body[..end];
    // 先剥掉行注释——注释里也写着机器码（`// 0x00  addi x1, x0, 42
    // → 0x02A00093`），不剥的话会把注释里的字面量一起解析进来。
    let body: String = body
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let parsed: Vec<u8> = body
        .split(|c: char| !(c.is_ascii_hexdigit() || c == 'x'))
        .filter(|s| s.starts_with("0x") && s.len() == 4)
        .map(|s| u8::from_str_radix(&s[2..], 16).expect("字节字面量"))
        .collect();
    assert_eq!(parsed, DEMO_BYTES.to_vec(), "Rust 与前端的 demo 固件漂移了");
}
