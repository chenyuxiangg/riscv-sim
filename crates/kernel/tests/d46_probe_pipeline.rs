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
//!    → kernel_host 取边沿 → 契约 EventPayload::WaveformHistory
//!    → 前端波形视图列出通道
//! ```
//!
//! ## 这些用例在防什么
//!
//! 每一条都对应一个**曾经可能悄悄坏掉**的接缝：
//!
//! - 通道**列表**必须推得出来 —— 「有这条信号但还没变化」和「根本没这条
//!   信号」是两回事，只靠边沿表达不出来；
//! - 边沿的**值**必须是真的 —— 不是 mock 凭空造的 `probe.*` 流；
//! - 边沿的**时间戳**必须不是 0 —— 时间轴平了等于半个假象；
//! - `LoadProject` 之后管道**必须还在** —— 换总线会带走总线上的 hook；
//! - 观察点 hook 与探针 hook **必须共存** —— 单槽 hook 会互相顶掉。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WaveformHistory,
    WriteSize,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};
use riscv_sim_kernel::kernel_host::{
    KernelHost, DEFAULT_GPIO_BASE, GPIO_PERIPH_ID, GPIO_SIGNAL_NAME, MAX_WAVEFORM_EDGES_PER_EVENT,
    TOPIC_WAVEFORM,
};
use riscv_sim_kernel::m17_backend::INSTRUCTION_COST_PS;
use riscv_sim_kernel::m18_probe::{CH_GPIO0, EDGE_STORE_CAP};

/// demo 固件 —— **引用共享常量**，不抄一份。
///
/// Rust 侧的权威副本是 `kernel_host::DEMO_FIRMWARE_BYTES`；它与前端
/// `front/dev/demo-firmware.js` 逐字节相同（下面那条防漂移用例直接读
/// 那份 JS 比对）。
use riscv_sim_kernel::kernel_host::DEMO_FIRMWARE_BYTES as DEMO_BYTES;

/// demo 固件写的 GPIO 通道 id（M-18 的 `"{source}.{signal}"`）。
const GPIO_STREAM_ID: &str = "GPIO0.OUT";

/// **中断脉冲**通道 id（M-16 → M-18，接缝 #5 / 台账 D-26）。
///
/// ⚠️ v0.19 起**多了一条**通道：M-16 每次登记一条挂起中断都会发一个
/// 脉冲，M-18 的 `CH_IRQ_PULSE` 通道把它收下来。
const IRQ_PULSE_STREAM_ID: &str = "M16.irq_pulse";

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

/// 从一批事件里挑出**最后一条**探针波形（`WaveformHistory`）。
///
/// ⚠️ 必须挑「最后一条」：一次 `Subscribe` 会推两条波形事件——寄存器
/// 全量快照 + 探针通道列表。挑第一条会拿到寄存器那条，断言就落空了。
fn last_probe_waveform(events: &[EventPayload]) -> Option<WaveformHistory> {
    events.iter().rev().find_map(|e| match e {
        EventPayload::WaveformHistory(wave) => Some(wave.clone()),
        _ => None,
    })
}

/// 从一批事件里挑出**所有**探针波形。
fn all_waveforms(events: &[EventPayload]) -> Vec<WaveformHistory> {
    events
        .iter()
        .filter_map(|e| match e {
            EventPayload::WaveformHistory(wave) => Some(wave.clone()),
            _ => None,
        })
        .collect()
}

/// 在波形里找一条流。
fn stream<'a>(
    wave: &'a WaveformHistory,
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
fn a_fresh_host_already_has_the_probe_pipeline_wired() {
    // ⚠️ `KernelHost::new()` 建好默认内存图（含 GPIO）并装了管道，但
    // **没有**给 GPIO 装上探针回调——`attach_peripheral_probes()` 只在
    // `rebuild_platform()`（装载工程时）里调。
    //
    // 后果：一个**刚建好、还没装载工程**的宿主，GPIO 上的写**不产生任何
    // 边沿**。现有用例全都先 `LoadProject`（走 `rebuild_platform`），
    // 所以这个缺口此前没人看得见。
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    // 直接往 GPIO 的 OUT 写 1 —— 不经任何固件。
    //
    // ⚠️ 波形随**这一次写**就推出来（V-A 检视 · 🟡-8）：调试器写 GPIO
    // 就是在改电平，让用户盯着波形视图却要等下一轮 `Run` 才看到自己
    // 刚写的那一下，是「接线通了但用起来是哑的」。
    let events = dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: DEFAULT_GPIO_BASE + 0x04,
            size: WriteSize::Word,
            value: 1,
        },
    );
    let wave = last_probe_waveform(&events).expect("写完 GPIO 必须立刻推一条探针波形");
    let gpio = stream(&wave, GPIO_STREAM_ID).expect("波形里必须有 GPIO0.OUT 通道");
    assert_eq!(
        gpio.edges.len(),
        1,
        "刚建好的宿主也必须能观察到 GPIO 边沿：{:?}",
        gpio.edges
    );

    // 下一轮 `StepN` **不再**重复推这条边沿——边沿已经发出去了。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 1 });
    assert!(
        last_probe_waveform(&events).is_none(),
        "边沿只推一次：{:?}",
        last_probe_waveform(&events)
    );
}

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
    // ⚠️ **两条**通道（v0.19 起）：GPIO 的 `OUT` 与**中断脉冲**
    // （接缝 #5 / D-26）。两条的 id 都是 `"{source}.{signal}"`。
    assert_eq!(
        ids,
        vec![GPIO_STREAM_ID, IRQ_PULSE_STREAM_ID],
        "通道 id 必须是 `{{source}}.{{signal}}`：{ids:?}"
    );
    assert_eq!(
        GPIO_STREAM_ID,
        format!("{GPIO_PERIPH_ID}.{GPIO_SIGNAL_NAME}"),
        "通道 id 的两段必须真的是外设 id 与信号名"
    );
    assert_eq!(
        IRQ_PULSE_STREAM_ID,
        format!(
            "{}.{}",
            riscv_sim_kernel::kernel_host::IRQ_PULSE_SOURCE,
            riscv_sim_kernel::kernel_host::IRQ_PULSE_SIGNAL
        ),
        "中断脉冲通道的 id 必须也是 `{{source}}.{{signal}}`"
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

#[test]
fn load_project_reinstalls_the_bus_hooks_on_the_new_bus() {
    // 换内存图 = 换一条**全新**的总线，总线上挂的 hook **一起没了**。
    // 管道必须在新总线上**重装一遍**——否则 M-18 的访存 trace 环会
    // **静默**收不到任何访存（M-11 的数据源就断了，而波形视图还是
    // 「有数据」的，缺陷极难被发现）。
    //
    // ⚠️ 判别力：把 `on_load_project` 里的 `rebuild_platform()` 换成
    // 「只重置内存图 + 重画默认内存图」，这条用例立刻变红（新总线上
    // 只有观察点 hook，探针的访存 hook 没装）。
    //
    // ⚠️ 另注：宿主的**运行期重试**（每轮 `Run` 前补装外设回调）**不能**
    // 掩盖这一条——它只补外设回调，不重建注册表、不装总线 hook。
    let mut host = KernelHost::new();
    assert_eq!(
        host.bus_hook_count(),
        2,
        "刚建好的宿主：观察点 hook + 探针访存 hook"
    );

    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        host.bus_hook_count(),
        2,
        "装载工程之后，新总线上必须**重新**装上观察点 hook 与探针访存 hook"
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

// ---------------------------------------------------------------------------
// V-A 检视 · 🟠-1：发布源的**上限**与 M-19 的**活性**
//
// 这一组用例防的是两件事：
//
// 1. **发布的波形流没有上限** —— 一次订阅推出 8 MB 的事件（实测 8 000 条
//    边沿 = 219 KB），把 WASM 的 `postMessage` 与前端的 JSON 解析同时压垮。
// 2. **M-19 在生产路径上是死代码** —— 把 M-18 → M-19 的 `set_edge_sink`
//    握手删掉，D-46 的全部用例**照样全绿**。环只写不读，等于纯内存开销。
//
// ⚠️ 第 2 条是本轮自查抓到的**判别力缺口**：用例「都在」不等于「用得着」。
// ---------------------------------------------------------------------------

/// 造 N 条 GPIO 边沿（**不订阅**波形——边沿只在 M-18 里攒着）。
fn accumulate_gpio_edges(host: &mut KernelHost, n: u32) {
    for i in 0..n {
        // 每次写一个**不同**的值：GPIO 只在电平真变化时才上报边沿。
        let value = i.wrapping_add(1);
        dispatch(
            host,
            CommandPayload::WriteMemory {
                addr: DEFAULT_GPIO_BASE + 0x04,
                size: WriteSize::Word,
                value,
            },
        );
    }
}

#[test]
fn the_m19_ring_actually_receives_the_edges() {
    // M-18 → M-19 的握手**真的在跑**：边沿进得去 M-19 的环。
    //
    // ⚠️ 判别力：把 `install_probe_pipeline()` 里那行 `set_edge_sink` 删掉
    // （或把 `WaveformPipeline` 的 `EdgeSink` 实现改成空操作），这条用例
    // 立刻变红。删掉握手之后 M-19 的环恒为 0 条——**M-19 成了死代码**，
    // 而此前的 D-46 用例一条都不会红。
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    dispatch(&mut host, CommandPayload::StepN { n: 16 });

    let pipeline = host
        .waveform_pipeline()
        .expect("宿主必须持有 M-19 的波形管道");
    assert!(
        pipeline.pipeline().channel_len(CH_GPIO0) > 0,
        "M-19 的环里必须有边沿：握手断了的话这里恒为 0（CH_GPIO0 = {CH_GPIO0}）"
    );
}

#[test]
fn a_published_waveform_event_never_exceeds_the_edge_budget() {
    // 单个事件的边沿**总量**有上限（V-A 检视 · 🟠-1）。
    //
    // 场景：长时间不订阅 → 边沿在 M-18 里攒了一堆 → 订阅的那一刻一次性
    // 推出来。没有上限的话就是一条 8 MB 的事件。
    let mut host = KernelHost::new();
    // 先攒 5 000 条边沿（**不订阅**）。
    accumulate_gpio_edges(&mut host, 5_000);

    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_WAVEFORM.to_string(),
        },
    );
    let wave = last_probe_waveform(&events).expect("订阅后必须推一条波形");
    let total: usize = wave.streams.iter().map(|s| s.edges.len()).sum();
    assert!(
        total <= MAX_WAVEFORM_EDGES_PER_EVENT,
        "单事件的边沿数 {total} 超过上限 {MAX_WAVEFORM_EDGES_PER_EVENT}"
    );
    assert!(
        host.waveform_edges_truncated() > 0,
        "5 000 条边沿挤进 {MAX_WAVEFORM_EDGES_PER_EVENT} 条的上限，必须**报告**截断——\
         不静默丢（V-A 检视 · 🟠-1）"
    );
}

#[test]
fn the_m18_edge_store_is_bounded() {
    // M-18 的每通道边沿存储**有上限**（内存兜底）：超了丢**最旧的**，
    // 并且**计数**——不静默截断。
    let mut host = KernelHost::new();
    // 攒的条数远超 M-18 的每通道上限（`EDGE_STORE_CAP`）。
    accumulate_gpio_edges(&mut host, EDGE_STORE_CAP as u32 + 2_000);
    assert!(
        host.probe_edges_dropped() > 0,
        "边沿存储超过 {EDGE_STORE_CAP} 条时必须报告丢弃——\
         静默截断等于时间轴上凭空少了一段"
    );
}
