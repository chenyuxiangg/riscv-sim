//! M-18 → M-19 跨模块集成测试（观测管道端到端）.
//!
//! 验证 PROGRESS.md §5.4 **N5「GPIO 翻转可见」** 的内核侧链路：
//!
//! ```text
//! M-14 总线访存 ──bus hook──▶ M-18 ProbeRegistry
//!                                  │  (边沿判定 + EdgeKind)
//!                                  │  EdgeSink 端口
//!                                  ▼
//!                            M-19 WaveformPipeline
//!                                  │  (边沿编码 + 环形缓冲 + 批量推送)
//!                                  ▼
//!                            CSV / VCD 分块导出
//! ```
//!
//! 本测试刻意走**跨模块**路径（真实 `Bus` + 真实 `ProbeRegistry` +
//! 真实 `WaveformPipeline`），而不是直接调 `encode_edge`——单元测试
//! 已经覆盖了后者。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::model::EdgeKind;
use riscv_sim_kernel::m14_bus::{Bus, BusWidth};
use riscv_sim_kernel::m18_probe::{ChannelDef, ProbeRegistry, SampleCfg, CH_GPIO0, CH_KEY_STATE};
use riscv_sim_kernel::m19_waveform::{ChannelMeta, ExportFilter, SharedPipeline, WaveformPipeline};

/// 建一条带探针的链路：返回 (总线, 探针注册表, 共享管道)。
///
/// 探针注册表通过 `Rc<RefCell<..>>` 挂在总线的 hook 上，这样总线
/// 访存和探针采样走的是同一套 wiring——这正是生产环境的接法。
fn build_pipeline() -> (Bus, Rc<RefCell<ProbeRegistry>>, SharedPipeline) {
    let mut bus = Bus::new();
    bus.map_ram(0x8000_0000, 0x1000, &[]).expect("map ram");

    let shared = SharedPipeline::new({
        let mut p = WaveformPipeline::new();
        p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
        p.define_channel(CH_KEY_STATE, ChannelMeta::single_bit("KEY0.IN"));
        p
    });

    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    reg.borrow_mut()
        .define_channel(ChannelDef::single_bit(CH_GPIO0, "GPIO0", "OUT"))
        .expect("gpio channel");
    reg.borrow_mut()
        .define_channel(ChannelDef::single_bit(CH_KEY_STATE, "KEY0", "IN"))
        .expect("key channel");

    // M-18 → M-19 的握手：把 M-19 装成探针的边沿接收端。
    reg.borrow_mut()
        .set_edge_sink(Some(Box::new(shared.clone())));

    // 总线访存 hook：完整访存流进 M-18 的 trace 环（M-11 导出用）。
    let hook_reg = Rc::clone(&reg);
    bus.set_hook(Some(Box::new(
        move |access: &riscv_sim_kernel::m14_bus::BusAccess| {
            hook_reg.borrow_mut().record_bus_access(access);
        },
    )));

    // 打开探针：master + 全部通道。
    let mut cfg = SampleCfg {
        master_enable: true,
        ..Default::default()
    };
    cfg.enable_channel(CH_GPIO0);
    cfg.enable_channel(CH_KEY_STATE);
    reg.borrow_mut().set_rate(cfg);

    (bus, reg, shared)
}

#[test]
fn gpio_toggle_reaches_the_waveform_pipeline() {
    let (_bus, reg, shared) = build_pipeline();

    // 引脚初始为低电平：首次观察没有前值可比，M-18 报 `Value`
    // （初始电平），而不是一次跳变。
    reg.borrow_mut()
        .sample(5, CH_GPIO0, 0)
        .expect("initial level");
    // 固件驱动 LED 引脚翻转：0 -> 1 -> 0。
    reg.borrow_mut().sample(10, CH_GPIO0, 1).expect("rising");
    reg.borrow_mut().sample(20, CH_GPIO0, 0).expect("falling");

    let mut pipeline = shared.pipeline_mut();
    assert_eq!(pipeline.channel_len(CH_GPIO0), 3);

    let delta = pipeline.push_delta(30).expect("batch");
    assert_eq!(delta.entries.len(), 3);
    assert_eq!(delta.entries[0].kind, EdgeKind::Value);
    assert_eq!(delta.entries[1].kind, EdgeKind::Rising);
    assert_eq!(delta.entries[2].kind, EdgeKind::Falling);
    assert_eq!(delta.seq_start, 0);
    assert_eq!(delta.seq_end, 3);
}

#[test]
fn bus_accesses_are_traced_and_do_not_pollute_the_waveform() {
    let (mut bus, reg, shared) = build_pipeline();

    bus.write(0x8000_0000, BusWidth::Word, 0xDEAD_BEEF)
        .expect("write");
    bus.read(0x8000_0000, BusWidth::Word).expect("read");

    // 访存进的是 trace 流，不是波形通道——两条流相互独立。
    assert_eq!(reg.borrow().trace_len(), 2);
    assert_eq!(shared.pipeline().stats().buffered, 0);

    // 读回写入的值，确认总线本身工作正常。
    assert_eq!(
        bus.read(0x8000_0000, BusWidth::Word).expect("read back"),
        0xDEAD_BEEF
    );
}

#[test]
fn multiple_channels_merge_into_one_time_ordered_export() {
    let (_bus, reg, shared) = build_pipeline();

    // 两个通道交叉注入。
    reg.borrow_mut().sample(30, CH_GPIO0, 1).expect("gpio");
    reg.borrow_mut().sample(10, CH_KEY_STATE, 1).expect("key");
    reg.borrow_mut().sample(20, CH_GPIO0, 0).expect("gpio");

    let mut pipeline = shared.pipeline_mut();
    let handle = pipeline.export_csv(&ExportFilter::all(), 0, None);
    assert!(handle.done);
    assert_eq!(handle.total, 3);

    // 导出流按 t_virt 单调（跨通道归并）。
    let times: Vec<u64> = handle
        .chunk
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("t_ps"))
        .map(|l| l.split(',').next().expect("cell").parse().expect("t_ps"))
        .collect();
    assert_eq!(times, vec![10, 20, 30]);

    // 两个通道的标签都出现在导出里。
    assert!(handle.chunk.contains("KEY0.IN"));
    assert!(handle.chunk.contains("GPIO0.OUT"));
}

#[test]
fn export_can_be_filtered_to_a_single_channel() {
    let (_bus, reg, shared) = build_pipeline();
    reg.borrow_mut().sample(10, CH_GPIO0, 1).expect("gpio");
    reg.borrow_mut().sample(20, CH_KEY_STATE, 1).expect("key");

    let mut pipeline = shared.pipeline_mut();
    let handle = pipeline.export_csv(&ExportFilter::all().with_channel(CH_GPIO0), 0, None);
    assert_eq!(handle.total, 1);
    assert!(handle.chunk.contains("GPIO0.OUT"));
    assert!(!handle.chunk.contains("KEY0.IN"));
}

#[test]
fn vcd_export_of_a_gpio_toggle_is_well_formed() {
    let (_bus, reg, shared) = build_pipeline();
    reg.borrow_mut().sample(100, CH_GPIO0, 1).expect("rising");
    reg.borrow_mut().sample(200, CH_GPIO0, 0).expect("falling");

    let mut pipeline = shared.pipeline_mut();
    let handle = pipeline.export_vcd(&ExportFilter::all(), 0, None);
    assert!(handle.chunk.contains("$timescale 1ps $end"));
    assert!(handle.chunk.contains("$var wire 1 ch1 GPIO0.OUT $end"));
    assert!(handle.chunk.contains("#100"));
    assert!(handle.chunk.contains("1ch1"));
    assert!(handle.chunk.contains("#200"));
    assert!(handle.chunk.contains("0ch1"));
}
