//! M-14 → M-15 → M-18 → M-19 端到端接线测试（N5「GPIO 翻转可见」内核侧闭合）.
//!
//! 验证 PROGRESS.md §5.4 **N5** 的内核路径：
//!
//! ```text
//! 固件写 GPIO OUT
//!    → M-14 总线派发到外设实例（D-12：map_peripheral_instance）
//!    → M-15 GpioBank 状态变化 → on_probe 上报 "OUT"（D-12）
//!    → M-18 ProbeRegistry 边沿判定（CH_GPIO0）
//!    → EdgeSink → M-19 WaveformPipeline 通道里出现期望的边沿
//! ```
//!
//! 本测试**刻意走跨模块路径**：真实的 `Bus`、真实的 `PeripheralRegistry`、
//! 真实的 `ProbeRegistry`、真实的 `WaveformPipeline` 一路串下来，中间
//! 没有一步是直接调 `sample()` / `encode_edge()` 绕过去的。
//!
//! 另外覆盖本接线 PR 的两个欠账：
//! - **D-12**：外设窗口 size 与 `Peripheral::size()` 不一致 → 挂载被拒
//!   （E_BUS-08）。
//! - **D-14**：总线读外设一律走 `read_clear` —— UART RX 连续读两次，
//!   第二次拿不到同一个字节。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{EdgeKind, PendingWrite, WriteSize};
use riscv_sim_contract::port::Peripheral;
use riscv_sim_kernel::m14_bus::{Bus, BusWidth};
use riscv_sim_kernel::m15_peripheral::gpio::{GPIO_WINDOW, SIGNAL_OUT};
use riscv_sim_kernel::m15_peripheral::timer::{SIGNAL_CMP, TIMER_WINDOW};
use riscv_sim_kernel::m15_peripheral::uart::{SIGNAL_TX, UART_WINDOW};
use riscv_sim_kernel::m15_peripheral::{GpioBank, PeripheralRegistry, TimerRiscv, Uart16550};
use riscv_sim_kernel::m18_probe::{
    ChannelKind, ProbeRegistry, SampleCfg, CH_GPIO0, CH_TIMER_CMP, CH_UART_TX,
};
use riscv_sim_kernel::m19_waveform::{ChannelMeta, SharedPipeline, WaveformPipeline};

const GPIO_BASE: u32 = 0x4000_0000;
const UART_BASE: u32 = 0x2000_0000;
const TIM_BASE: u32 = 0x6000_0000;

/// 一个窗口大小可以任意指定的外设（F-7 专用）。
///
/// 真实的三个外设窗口都是固定的小值，没法驱动「窗口超上限」这条分支，
/// 所以这里造一个只有 `size()` 可配的最小外设：寄存器语义是空壳，
/// 本用例只关心**挂载阶段**的大小校验。
struct SizedPeripheral {
    id: String,
    base: u32,
    size: u32,
}

impl SizedPeripheral {
    fn new(id: &str, base: u32, size: u32) -> Self {
        Self {
            id: id.to_string(),
            base,
            size,
        }
    }
}

impl Peripheral for SizedPeripheral {
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
    fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
}

/// The whole wired-up platform: bus + peripherals + probes + waveform.
struct Harness {
    bus: Bus,
    probes: Rc<RefCell<ProbeRegistry>>,
    wave: SharedPipeline,
    peripherals: PeripheralRegistry,
}

impl Harness {
    /// Advance the peripheral clock the way M-12's quantum loop does
    /// (D-07 wires M-12 → `Peripheral::tick`; until that lands the
    /// host drives it by hand — the timestamp the probe report carries
    /// is exactly this value).
    fn tick(&mut self, id: &str, now_ps: u64) {
        self.peripherals
            .get_mut(id)
            .expect("registered peripheral")
            .tick(now_ps)
            .expect("tick");
    }
}

/// Build the fully wired platform.
///
/// The wiring is the three-step recipe the coordinator can reuse in
/// M-12's boot sequence:
///
/// 1. M-15 creates the instances and registers them (the registry owns
///    them, the bus only gets a handle).
/// 2. M-18 binds each peripheral's signal to a channel and installs the
///    probe callback (`ProbeRegistry::attach_signal`).
/// 3. M-14 maps the instance onto the bus (`Bus::map_peripheral_instance`).
fn build() -> Harness {
    let mut bus = Bus::new();
    bus.map_ram(0x8000_0000, 0x1000, &[]).expect("map ram");

    // ---- M-19: the waveform consumer ----
    let wave = SharedPipeline::new({
        let mut p = WaveformPipeline::new();
        p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
        p.define_channel(CH_TIMER_CMP, ChannelMeta::multi_bit("TIM0.CMP", 1));
        p.define_channel(CH_UART_TX, ChannelMeta::multi_bit("UART0.TX", 8));
        p
    });

    // ---- M-18: the probe registry ----
    let probes = Rc::new(RefCell::new(ProbeRegistry::new()));
    probes
        .borrow_mut()
        .set_edge_sink(Some(Box::new(wave.clone())));

    // ---- M-15: the peripheral instances (the registry owns them) ----
    let mut peripherals = PeripheralRegistry::new();

    let mut gpio = GpioBank::new("GPIO0", GPIO_BASE);
    gpio.set_directions(0xFFFF_FFFF); // all pins are outputs
    peripherals.register(gpio).expect("register gpio");

    let mut uart = Uart16550::new("UART0", UART_BASE, 115_200, 5);
    uart.receive(b'Z'); // a byte is waiting in the RX FIFO
    peripherals.register(uart).expect("register uart");

    peripherals
        .register(TimerRiscv::new("TIM0", TIM_BASE, 7))
        .expect("register timer");

    // ---- M-18 ↔ M-15: bind each signal to a channel ----
    //
    // GPIO "OUT" carries a pin bitmap → a single-bit channel only sees
    // pin 0, so the test drives pin 0. UART "TX" / TIM "CMP" are
    // per-event streams → pulse channels (two identical bytes in a row
    // are two distinct events).
    let gpio = peripherals.handle("GPIO0").expect("gpio handle");
    let uart = peripherals.handle("UART0").expect("uart handle");
    let tim = peripherals.handle("TIM0").expect("timer handle");

    {
        let mut p = gpio.borrow_mut();
        ProbeRegistry::attach_signal(
            Rc::clone(&probes),
            &mut *p,
            SIGNAL_OUT,
            CH_GPIO0,
            ChannelKind::SingleBit,
            1,
        )
        .expect("attach gpio signal");
    }
    {
        let mut p = uart.borrow_mut();
        ProbeRegistry::attach_signal(
            Rc::clone(&probes),
            &mut *p,
            SIGNAL_TX,
            CH_UART_TX,
            ChannelKind::Pulse,
            8,
        )
        .expect("attach uart signal");
    }
    {
        let mut p = tim.borrow_mut();
        ProbeRegistry::attach_signal(
            Rc::clone(&probes),
            &mut *p,
            SIGNAL_CMP,
            CH_TIMER_CMP,
            ChannelKind::Pulse,
            1,
        )
        .expect("attach timer signal");
    }

    // Open the probe: master switch + the three channels.
    // (`set_rate` replaces the whole config, so the enable bits have
    // to be re-stated here even though `define_channel` enabled them.)
    let mut cfg = SampleCfg {
        master_enable: true,
        ..Default::default()
    };
    cfg.enable_channel(CH_GPIO0);
    cfg.enable_channel(CH_TIMER_CMP);
    cfg.enable_channel(CH_UART_TX);
    probes.borrow_mut().set_rate(cfg);

    // ---- M-14: map the instances onto the bus ----
    bus.map_peripheral_instance(GPIO_BASE, GPIO_WINDOW, gpio)
        .expect("map gpio");
    bus.map_peripheral_instance(UART_BASE, UART_WINDOW, uart)
        .expect("map uart");
    bus.map_peripheral_instance(TIM_BASE, TIMER_WINDOW, tim)
        .expect("map timer");

    Harness {
        bus,
        probes,
        wave,
        peripherals,
    }
}

// ---------------------------------------------------------------------------
// N5 — GPIO 翻转可见
// ---------------------------------------------------------------------------

#[test]
fn firmware_gpio_write_reaches_the_waveform_pipeline() {
    // N5 的内核闭合证据：固件写 OUT → M-18 产生边沿 → M-19 通道里看得见。
    let mut h = build();

    // M-12 的量子循环推进外设时钟（边沿的时间戳来源）。
    h.tick("GPIO0", 1_000);

    // 固件写 GPIO OUT：pin 0 拉高。
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b1)
        .expect("write gpio out");

    // 波形里必须出现该通道的边沿。
    {
        // M-18 这一段也要有证据：探针注册表里确实积累了一条边沿。
        let probes = h.probes.borrow();
        assert_eq!(
            probes.edge_count(),
            1,
            "M-18 must have accumulated the GPIO edge"
        );
        assert_eq!(probes.edges_for(CH_GPIO0).len(), 1);
        assert_eq!(probes.edges_for(CH_GPIO0)[0].value, 1);
    }
    {
        let mut pipeline = h.wave.pipeline_mut();
        assert_eq!(
            pipeline.channel_len(CH_GPIO0),
            1,
            "a GPIO level change must produce exactly one waveform edge"
        );
        let delta = pipeline.push_delta(2_000).expect("batch");
        assert_eq!(delta.entries.len(), 1);
        let e = &delta.entries[0];
        assert_eq!(e.ch, CH_GPIO0);
        assert_eq!(e.value, 1, "the reported value is the pin bitmap");
        assert_eq!(e.t_ps, 1_000, "the edge carries the peripheral's clock");
        // 通道上的第一次观察没有前值可比 → 报初始电平（Value），
        // 而不是一次跳变（M-18 的首样本规则）。
        assert_eq!(e.kind, EdgeKind::Value);
    }

    // 固件再写一次同样的电平 → 电平没变 → 不应再产生边沿。
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b1)
        .expect("write gpio out again");
    h.tick("GPIO0", 3_000);
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b0)
        .expect("write gpio out low");
    {
        let mut pipeline = h.wave.pipeline_mut();
        assert_eq!(
            pipeline.channel_len(CH_GPIO0),
            2,
            "an unchanged level must not produce a second edge"
        );
        let delta = pipeline.push_delta(4_000).expect("batch");
        assert_eq!(delta.entries.len(), 1);
        let e = &delta.entries[0];
        assert_eq!(e.value, 0);
        assert_eq!(e.t_ps, 3_000);
        assert_eq!(e.kind, EdgeKind::Falling, "1 → 0 is a falling edge");
    }

    // 第三次：0 → 1 → 上升沿。
    h.tick("GPIO0", 5_000);
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b1)
        .expect("write gpio out high");
    {
        let mut pipeline = h.wave.pipeline_mut();
        let delta = pipeline.push_delta(6_000).expect("batch");
        assert_eq!(delta.entries.len(), 1);
        let e = &delta.entries[0];
        assert_eq!(e.value, 1);
        assert_eq!(e.kind, EdgeKind::Rising, "0 → 1 is a rising edge");
    }
}

#[test]
fn gpio_edge_is_visible_in_the_csv_export() {
    // N5 的用户可见形态：导出的 CSV 里能看到这条边沿。
    let mut h = build();
    h.tick("GPIO0", 1_000);
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b1)
        .expect("write gpio out");

    let mut pipeline = h.wave.pipeline_mut();
    let handle = pipeline.export_csv(
        &riscv_sim_kernel::m19_waveform::ExportFilter::all(),
        0,
        None,
    );
    assert!(handle.done);
    assert_eq!(handle.total, 1);
    assert!(handle.chunk.contains("GPIO0.OUT"), "{}", handle.chunk);
    assert!(handle.chunk.contains("1000"), "{}", handle.chunk);
}

// ---------------------------------------------------------------------------
// D-12 — 挂载校验
// ---------------------------------------------------------------------------

#[test]
fn peripheral_window_size_must_match_the_peripheral_size() {
    // D-12：总线窗口 size 与 `Peripheral::size()` 不一致 → 挂载被拒
    // （E_BUS-08），否则总线会派发到外设窗口之外。
    let mut peripherals = PeripheralRegistry::new();
    peripherals
        .register(GpioBank::new("GPIO0", GPIO_BASE))
        .expect("register gpio");
    let handle = peripherals.handle("GPIO0").expect("handle");

    let mut bus = Bus::new();
    let err = bus
        .map_peripheral_instance(GPIO_BASE, GPIO_WINDOW + 4, Rc::clone(&handle))
        .expect_err("size mismatch must be rejected");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 8, "size mismatch raises E_BUS-08");
    assert_eq!(bus.region_count(), 0, "a rejected mount leaves no region");

    // 正确 size → 挂载成功。
    bus.map_peripheral_instance(GPIO_BASE, GPIO_WINDOW, handle)
        .expect("matching size must be accepted");
    assert_eq!(bus.region_count(), 1);
}

#[test]
fn overlapping_peripheral_window_is_rejected() {
    // 外设窗口与已有区间重叠 → E_BUS-03，且旧配置保留。
    let mut peripherals = PeripheralRegistry::new();
    peripherals
        .register(GpioBank::new("GPIO0", GPIO_BASE))
        .expect("register gpio");
    let handle = peripherals.handle("GPIO0").expect("handle");

    let mut bus = Bus::new();
    bus.map_ram(GPIO_BASE, 0x100, &[]).expect("map ram");
    let err = bus
        .map_peripheral_instance(GPIO_BASE, GPIO_WINDOW, handle)
        .expect_err("overlap must be rejected");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 3, "overlap raises E_BUS-03");
    assert_eq!(bus.region_count(), 1, "the old region set is preserved");
}

#[test]
fn access_past_the_peripheral_window_is_rejected() {
    // 越界访问：起始地址在窗口内、但访问跨出窗口 → E_BUS-01
    // （窗口之后的字节没有映射）。
    let h = build();
    let mut bus = h.bus;
    let err = bus
        .read(GPIO_BASE + GPIO_WINDOW - 1, BusWidth::Word)
        .expect_err("access past the window must fail");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 1, "unmapped byte slice raises E_BUS-01");
}

#[test]
fn peripheral_dispatch_is_counted_in_the_bus_stats() {
    // 外设命中要计入 `periph_hits`（接线之前恒为 0）。
    let mut h = build();
    h.bus
        .write(GPIO_BASE + 0x4, BusWidth::Word, 0b1)
        .expect("write gpio out");
    h.bus
        .read(GPIO_BASE, BusWidth::Word)
        .expect("read gpio dir");
    let stats = h.bus.stats();
    assert_eq!(stats.periph_hits, 2, "one write + one read on a peripheral");
}

// ---------------------------------------------------------------------------
// D-14 — 总线读外设走 read_clear
// ---------------------------------------------------------------------------

#[test]
fn uart_rx_read_twice_does_not_return_the_same_byte() {
    // D-14 回归：总线读外设必须走 `read_clear`。
    // 若总线调的是 `read`（非破坏性），第二次读会拿到同一个字节 ——
    // UART RX「只能收 1 字节」的原缺陷就静默复现了。
    let mut h = build();

    let first = h.bus.read(UART_BASE, BusWidth::Byte).expect("read uart rx");
    assert_eq!(first, u32::from(b'Z'), "the queued byte comes out first");

    let second = h
        .bus
        .read(UART_BASE, BusWidth::Byte)
        .expect("read uart rx again");
    assert_eq!(second, 0, "the RX FIFO must be drained by the first read");

    // 第三次同样拿不到字节（FIFO 仍然是空的）。
    let third = h
        .bus
        .read(UART_BASE, BusWidth::Byte)
        .expect("read uart rx a third time");
    assert_eq!(third, 0);
}

// ---------------------------------------------------------------------------
// 另外两个外设的信号
// ---------------------------------------------------------------------------

#[test]
fn uart_tx_byte_reaches_the_waveform_pipeline() {
    // 固件写 UART DATA → 外设上报 "TX"（值为字节本身）。
    let mut h = build();
    h.tick("UART0", 7_000);
    h.bus
        .write(UART_BASE, BusWidth::Byte, u32::from(b'T'))
        .expect("write uart data");

    let mut pipeline = h.wave.pipeline_mut();
    assert_eq!(pipeline.channel_len(CH_UART_TX), 1);
    let delta = pipeline.push_delta(8_000).expect("batch");
    assert_eq!(delta.entries.len(), 1);
    let e = &delta.entries[0];
    assert_eq!(e.ch, CH_UART_TX);
    assert_eq!(e.value, u64::from(b'T'));
    assert_eq!(e.t_ps, 7_000);
}

#[test]
fn timer_compare_hit_reaches_the_waveform_pipeline() {
    // 固件写 mtimecmp → 时间推进到比较点 → 外设上报 "CMP"。
    let mut h = build();
    h.bus
        .write(TIM_BASE + 0x8, BusWidth::Word, 1_000)
        .expect("write mtimecmp lo");

    // 时间还没到比较点 → 不产生边沿。
    h.tick("TIM0", 500);
    {
        let pipeline = h.wave.pipeline_mut();
        assert_eq!(pipeline.channel_len(CH_TIMER_CMP), 0);
    }

    // 越过比较点 → 命中。
    h.tick("TIM0", 2_000);
    {
        let mut pipeline = h.wave.pipeline_mut();
        assert_eq!(pipeline.channel_len(CH_TIMER_CMP), 1);
        let delta = pipeline.push_delta(3_000).expect("batch");
        assert_eq!(delta.entries.len(), 1);
        let e = &delta.entries[0];
        assert_eq!(e.ch, CH_TIMER_CMP);
        assert_eq!(e.value, 1, "the compare hit flag");
        assert_eq!(e.t_ps, 2_000);
    }
}

// ---------------------------------------------------------------------------
// D-10 — 区域大小上限（M-14 侧）
// ---------------------------------------------------------------------------

#[test]
fn oversized_region_is_rejected_without_allocating() {
    // D-10（M-14 侧）：畸形固件可以声明 GiB 级的段；总线必须**挡住**它，
    // 否则浏览器标签页直接 OOM。
    //
    // ⚠️ **v0.26（STAGE-11 裁决 B）：护栏的「被测量」从「声明大小」改成
    // 「物理占用」** ⇒ 一个 GiB 的**声明**现在**免费**（懒分配 ⇒ 零物理
    // 代价）⇒ 但 D-10 的保护**没有丢**：它现在挡的是「**真占**那么多」。
    //
    // ⇒ 这条用例钉的是**两件事**：① 声明不花钱；② 真占超预算被拒，且
    // **被拒时不留下任何已物化的页**。
    let mut bus = Bus::new();

    // ① 声明一个 256 MiB 的区域：**成功**，而且**零物理占用**。
    bus.map_ram(0, 0x1000_0000, &[])
        .expect("a big declaration is free now — it materializes nothing");
    assert_eq!(bus.region_count(), 1);
    assert_eq!(
        bus.ram_allocated_bytes(),
        0,
        "声明不占物理内存（这正是懒分配要买到的东西）"
    );

    // ② 真占超预算 ⇒ **E_BUS-09**，且**零页留下**。
    let mut small = Bus::with_max_region_bytes(0x1000);
    let err = small
        .map_ram(0, 0x1000, &[0u8; 0x1001])
        .expect_err("a region that really occupies more than the cap must be rejected");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 9, "over-cap occupancy raises E_BUS-09");
    assert_eq!(
        small.ram_allocated_bytes(),
        0,
        "被拒的区域**不得**留下任何已物化的页"
    );
    assert_eq!(small.region_count(), 0);
}

#[test]
fn the_region_size_cap_is_configurable() {
    // 上限可配：默认 16 MiB，宿主可按自己的预算策略调整。
    //
    // ⚠️ **v0.26（STAGE-11 裁决 B）**：上限现在管的是**物理占用**，不是
    // 声明大小 ⇒ 「声明多大」不再被拒，**「真占多少」才被拒**。
    let mut bus = Bus::with_max_region_bytes(0x1000);
    assert_eq!(bus.max_region_bytes(), 0x1000);

    // 声明本身**免费** ⇒ 超过上限的声明也**合法**。
    bus.map_ram(0x2000, 0x1001, &[])
        .expect("a declaration over the cap is free now");
    assert_eq!(bus.ram_allocated_bytes(), 0);

    // 但**物化**第二页就会超过上限（0x1000 = 1 页）⇒ 被拒。
    //
    // ⚠️ 区域必须**够大**（0x2000 = 2 页）才能让第二个写落在**区域内部**：
    // 否则 `0x1000` 落在区域**外面** ⇒ 那报的是 **E_BUS-01（未映射）**，
    // 压根走不到护栏 ⇒ 断言会挂在场景码上而不是挂在护栏上。
    bus.map_ram(0, 0x2000, &[])
        .expect("a two-page region is free to declare");
    bus.write(0, BusWidth::Byte, 0xAA)
        .expect("the first page fits");
    let err = bus
        .write(0x1000, BusWidth::Byte, 0xBB)
        .expect_err("a second page is over the cap");
    assert_eq!(err.scenario, 9, "over-cap occupancy raises E_BUS-09");
    assert_eq!(
        bus.ram_allocated_bytes(),
        4096,
        "被拒的那次写**不得**留下已物化的页"
    );

    // 调整上限后同样的写**成功** ⇒ 护栏**只能放宽，不能去掉**。
    bus.set_max_region_bytes(0x2000);
    bus.write(0x1000, BusWidth::Byte, 0xBB)
        .expect("raising the cap works");
    assert_eq!(bus.ram_allocated_bytes(), 2 * 4096);
}

#[test]
fn oversized_peripheral_window_is_rejected_by_the_region_cap() {
    // F-7（M-14 侧盲区）：外设路径的区域大小上限必须被拒绝。
    //
    // `map_ram` / `map_rom` 有**两层**保护（`prepare_bytes` 的预分配检查
    // + `map_region` 的统一上限），而 `map_peripheral_instance` 只有
    // `map_region` 这一层。之前的接线测试只覆盖了 `map_ram` 路径，
    // 于是「外设路径的上限」这条防线**没有任何测试守护**——一旦
    // `map_region` 里的上限检查被误删，外设路径会静默放行。
    //
    // 这里造一个 `size()` = 1 GiB 的外设（远超默认 16 MiB 上限），
    // 挂载必须被拒，且错误必须是 E_BUS-09。
    const HUGE_BASE: u32 = 0x4000_0000;
    const HUGE_SIZE: u32 = 0x4000_0000; // 1 GiB

    let mut peripherals = PeripheralRegistry::new();
    peripherals
        .register(SizedPeripheral::new("HUGE0", HUGE_BASE, HUGE_SIZE))
        .expect("register the oversized peripheral");
    let handle = peripherals.handle("HUGE0").expect("handle");

    let mut bus = Bus::new();
    let err = bus
        .map_peripheral_instance(HUGE_BASE, HUGE_SIZE, Rc::clone(&handle))
        .expect_err("外设路径的区域大小上限必须被拒绝");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(
        err.scenario, 9,
        "an over-cap peripheral window must raise E_BUS-09"
    );
    assert_eq!(
        bus.region_count(),
        0,
        "a rejected mount must leave the region set untouched"
    );

    // 窗口 size 与外设声明不一致时先报 E_BUS-08 —— 说明 1 GiB 那个
    // 用例确实是被**上限**挡下来的，而不是被尺寸校验挡下来的。
    let err = bus
        .map_peripheral_instance(HUGE_BASE, HUGE_SIZE - 4, handle)
        .expect_err("a size mismatch is rejected too");
    assert_eq!(err.scenario, 8, "size mismatch raises E_BUS-08");
}

#[test]
fn a_peripheral_window_within_the_cap_still_mounts() {
    // 回归保护：上限之内的外设窗口照常挂载（别把上限检查写成"一律拒绝"）。
    const SMALL_BASE: u32 = 0x7000_0000;
    const SMALL_SIZE: u32 = 0x1000;

    let mut peripherals = PeripheralRegistry::new();
    peripherals
        .register(SizedPeripheral::new("SMALL0", SMALL_BASE, SMALL_SIZE))
        .expect("register the small peripheral");
    let handle = peripherals.handle("SMALL0").expect("handle");

    let mut bus = Bus::new();
    bus.map_peripheral_instance(SMALL_BASE, SMALL_SIZE, handle)
        .expect("a window under the cap must be accepted");
    assert_eq!(bus.region_count(), 1);
}
