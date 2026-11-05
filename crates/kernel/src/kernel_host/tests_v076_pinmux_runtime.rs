//! STAGE-52 · v0.76 批 —— **pinmux 中断路由**的宿主侧门禁（K 组运行时）。
//!
//! ⚠️ **本文件钉的是「宿主把引脚 × 当前功能 → irq 源」抬上 PLIC**（裁决
//! C1 / J / K）⇒ ⇒ 与 `tests_v074_pin_routing.rs` 的**接线**路径**并行不
//! 互写**（K-6 · 分层禁令）：
//!
//! - 接线路径（v0.74）：`wiring.from.pin` ⇒ 编译期把**引脚 IRQ 线号**写进
//!   GPIO 的 `irq_lines[pin]` ⇒ 宿主 `take_irq_line_for_pin` 消费 ⇒
//!   **线号 == 源号** 才抬；
//! - pinmux 路径（v0.76 · 本文件）：**不碰** GPIO 的 `irq_lines` ⇒ ⇒ 读
//!   GPIO 的 **pending 位**（引脚有沿事件）⇒ 查 pinmux 器件的**当前功能**
//!   ⇒ 按「当前功能 → irq 源」抬 PLIC ⇒ ⇒ 中断绑「**引脚 × 当前功能**」。
//!
//! ⚠️ **判别力（K 组运行时）**：
//! - K-4：固件写 pinmux 选择 `gpio_in` ⇒ 引脚沿 ⇒ 源 11 挂起 ⇒ guest
//!   claim 得到它（通电）；
//! - K-5：同引脚两功能（`uart_tx`·源 3 / `gpio_in`·源 11）⇒ 先选 A 抬 3、
//!   切 B 抬 11 ⇒ 同引脚两时刻两中断；
//! - K-6：pinmux 路径**不写** GPIO `irq_lines[pin]`（`irq_line_for_pin` 仍
//!   `None`）⇒ 与接线路径并行不互写；
//! - K-8：方向行为 —— output 引脚由固件写 OUT 驱动电平、input 引脚由
//!   `set_external_inputs` 喂入 ⇒ 两边沿都会抬**当前功能**的源；
//! - K-10：写不存在的功能索引（越界）⇒ `E_PLT-23`（报错不静默）。

// ⚠️ 断言码一律写数字字面量（`23`），不引常量 —— 与
// tests_board_delta_equivalence 同款纪律。
use riscv_sim_contract::error::PLATFORM_FAMILY;
use riscv_sim_contract::platform::{
    parse_gpio_pin_name, DeviceSpec, PinDirection, PinFunction, PinIrq, PinSpec,
    PlatformDescription, RegionSpec,
};
use riscv_sim_contract::port::Peripheral;
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m15_peripheral::gpio::GpioBank;
use crate::m15_peripheral::plic::Plic;

use super::platform;
use super::KernelHost;

/// qemu-virt 里 **GPIO0** 的基址（`0x4000_0000`）。
const GPIO0_BASE: u32 = 1_073_741_824;
/// 测试板给 **PINMUX** 留的基址（`0x4000_1000` —— 不与 qemu-virt 既有外设
/// 重叠）。
const PINMUX_BASE: u32 = GPIO0_BASE + 0x1000;

/// 一块**带 pinmux + 引脚规格**的板（基于 qemu-virt · 单功能）。
fn board_with_single_function() -> PlatformDescription {
    let mut description =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置 qemu-virt");
    description.regions.push(RegionSpec::Peripheral {
        base: Some(PINMUX_BASE),
        device: DeviceSpec::Pinmux {
            id: "PINMUX".to_string(),
        },
    });
    description.pins = Some(vec![PinSpec {
        device: "GPIO0".to_string(),
        pin: "pin3".to_string(),
        functions: vec![PinFunction {
            name: "gpio_in".to_string(),
            direction: PinDirection::Input,
            irq: Some(PinIrq { source: 11 }),
        }],
    }]);
    description
}

/// 同引脚**两个功能**（`uart_tx`·源 3 / `gpio_in`·源 11）—— K-5 的板。
fn board_with_two_functions() -> PlatformDescription {
    let mut description =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置 qemu-virt");
    description.regions.push(RegionSpec::Peripheral {
        base: Some(PINMUX_BASE),
        device: DeviceSpec::Pinmux {
            id: "PINMUX".to_string(),
        },
    });
    description.pins = Some(vec![PinSpec {
        device: "GPIO0".to_string(),
        pin: "pin3".to_string(),
        functions: vec![
            PinFunction {
                name: "uart_tx".to_string(),
                direction: PinDirection::Output,
                irq: Some(PinIrq { source: 3 }),
            },
            PinFunction {
                name: "gpio_in".to_string(),
                direction: PinDirection::Input,
                irq: Some(PinIrq { source: 11 }),
            },
        ],
    }]);
    description
}

/// 完整中断链板（对齐真机 BV_PINMUX_BOARD —— 固件地址同源）。
/// 基于内置 qemu-virt（有 CLINT/PLIC/UART/GPIO），把 PLIC base 挪到
/// `0xc000_0000`（固件逐字写死）并加 pinmux + pins。
fn board_with_full_irq_chain() -> PlatformDescription {
    let mut description =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置 qemu-virt");
    // PLIC base → 0xc000_0000（固件 x2 = lui 0xc0000）。
    for region in description.regions.iter_mut() {
        if let RegionSpec::Peripheral { base, device } = region {
            if matches!(device, DeviceSpec::Plic { .. }) {
                *base = Some(0xc000_0000);
            }
        }
    }
    description.regions.push(RegionSpec::Peripheral {
        base: Some(PINMUX_BASE),
        device: DeviceSpec::Pinmux {
            id: "PINMUX".to_string(),
        },
    });
    description.pins = Some(vec![PinSpec {
        device: "GPIO0".to_string(),
        pin: "pin3".to_string(),
        functions: vec![
            PinFunction {
                name: "uart_tx".to_string(),
                direction: PinDirection::Output,
                irq: Some(PinIrq { source: 3 }),
            },
            PinFunction {
                name: "gpio_in".to_string(),
                direction: PinDirection::Input,
                irq: Some(PinIrq { source: 11 }),
            },
        ],
    }]);
    description
}

/// 把板装进宿主（**真走** `SetMemoryMap`）⇒ ⇒ 回**生效图**。
fn load_board(host: &mut KernelHost, description: PlatformDescription) -> Vec<EventPayload> {
    host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description,
            expected_revision: None,
        },
    ))
}

/// 宿主手里那台 **PLIC** 的句柄（按 id 查）。
fn plic_of(host: &KernelHost, id: &str) -> std::rc::Rc<std::cell::RefCell<Plic>> {
    host.device_table()
        .get(id)
        .and_then(|entry| match &entry.handle {
            platform::DeviceHandle::Plic(handle) => Some(std::rc::Rc::clone(handle)),
            _ => None,
        })
        .expect("宿主手里有那台 PLIC")
}

/// 宿主手里那台 **GPIO** 的句柄（按 id 查）。
fn gpio_of(host: &KernelHost, id: &str) -> std::rc::Rc<std::cell::RefCell<GpioBank>> {
    host.device_table()
        .get(id)
        .and_then(|entry| match &entry.handle {
            platform::DeviceHandle::Gpio(handle) => Some(std::rc::Rc::clone(handle)),
            _ => None,
        })
        .expect("宿主手里有那台 GPIO")
}

/// 让 guest **能 claim** 某个源（**使能** + **优先级 > 门限`0`**）。
fn enable_source(plic: &std::rc::Rc<std::cell::RefCell<Plic>>, source: u32) {
    let mut p = plic.borrow_mut();
    p.set_priority(source, 1);
    p.enable(0, source, true);
}

/// 固件写 pinmux 的**选择寄存器**（偏移 `0x4 × 引脚号` ⇒ ⇒ 写功能索引）。
fn firmware_select(host: &mut KernelHost, pin: u32, function_index: u32) {
    host.bus_mut()
        .write(
            PINMUX_BASE + 4 * pin,
            crate::m14_bus::BusWidth::Word,
            function_index,
        )
        .expect("固件写 pinmux 选择寄存器");
}

/// **K-4** —— **pinmux 通电**：固件写选择 `gpio_in` ⇒ 引脚沿 ⇒ 源 11 被抬
/// ⇒ ⇒ guest claim 得到它。
#[test]
fn k4_the_pinmux_path_raises_the_current_functions_source() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_single_function());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **固件写选择**：选中 `gpio_in`（功能索引 0 ⇒ ⇒ 默认就是它 ⇒ ⇒
    //   显式写一遍 ⇒ ⇒ 「由固件决定」这条真的被走过）。
    firmware_select(&mut host, 3, 0);

    // ⚠️ **制造一个引脚沿**（宿主驱动输入电平变化 —— 调试入口）。
    let gpio = gpio_of(&host, "GPIO0");
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));

    // ⚠️ **跑一轮同步** ⇒ ⇒ 宿主按「当前功能 → irq 源」抬 PLIC。
    host.sync_interrupts_now();

    let plic = plic_of(&host, "PLIC");
    assert!(
        plic.borrow().pending(11),
        "pinmux 通电：GPIO0.pin3 的沿必须把**源 11**挂起（当前功能 gpio_in → 源 11）"
    );
    enable_source(&plic, 11);
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(11),
        "guest 必须 claim 得到那个源（⇒ ⇒ 不只是「描述被接受」那种弱证据）"
    );
}

/// **K-5** —— **切换跟着变**：同引脚两功能两源 ⇒ 先选 `uart_tx` 抬源 3 ⇒
/// 切 `gpio_in` 再抬源 11 ⇒ ⇒ 同引脚两时刻两中断。
#[test]
fn k5_switching_functions_moves_the_interrupt_to_the_new_source() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_full_irq_chain());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    let plic = plic_of(&host, "PLIC");
    let gpio = gpio_of(&host, "GPIO0");

    // ---- 第一时刻：选中 `uart_tx`（索引 0）⇒ 沿 ⇒ 源 3 ----
    firmware_select(&mut host, 3, 0);
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));
    host.sync_interrupts_now();
    enable_source(&plic, 3);
    assert!(
        plic.borrow().pending(3),
        "选 uart_tx ⇒ ⇒ 沿 ⇒ ⇒ 必须抬**源 3**"
    );
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(3),
        "guest 第一时刻必须 claim 得到源 3"
    );
    plic.borrow_mut().complete(0, 3);
    assert!(!plic.borrow().pending(3), "complete 之后源 3 不该再挂着");

    // ---- 第二时刻：切到 `gpio_in`（索引 1）⇒ 再沿 ⇒ 源 11 ----
    firmware_select(&mut host, 3, 1);
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));
    host.sync_interrupts_now();
    enable_source(&plic, 11);
    assert!(
        plic.borrow().pending(11),
        "切到 gpio_in ⇒ ⇒ 沿 ⇒ ⇒ 必须抬**源 11**（同引脚两时刻两中断）"
    );
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(11),
        "guest 第二时刻必须 claim 得到源 11"
    );
}

/// **K-6** —— **分层禁令**：pinmux 路径**不写** GPIO 的 `irq_lines[pin]`
/// ⇒ ⇒ 通电之后 `irq_line_for_pin(3)` 仍是 `None`（那根引脚**没被接线表**
/// 绑过）⇒ ⇒ 与接线路径并行不互写。
#[test]
fn k6_the_pinmux_path_never_writes_gpio_irq_lines() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_single_function());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    let gpio = gpio_of(&host, "GPIO0");
    let plic = plic_of(&host, "PLIC");

    // ⚠️ **装完板 ⇒ 那根引脚必须没有被接线表绑上**（pinmux 路径不写）。
    assert_eq!(
        gpio.borrow().irq_line_for_pin(3),
        None,
        "pinmux 路径**不许**写 GPIO 的 irq_lines[3]（分层禁令 · 裁决 K）"
    );

    // ⚠️ **通电照常**（沿 ⇒ 源 11）⇒ ⇒ 走的**不是**「线号 == 源号」那条路。
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(11),
        "不写 irq_lines 也照样把源 11 挂起（pinmux 路径按「当前功能 → 源」抬）"
    );
    // ⚠️ **再确认**：通电之后 irq_lines 依然没被写（⇒ ⇒ 不是「抬完才写」）。
    assert_eq!(
        gpio.borrow().irq_line_for_pin(3),
        None,
        "通电之后 irq_lines[3] 依然必须是 None（自始至终不碰）"
    );
    // ⚠️ **消费掉**（一次性 ⇒ ⇒ 不重抬）。
    plic.borrow_mut().complete(0, 11);
    for _ in 0..3 {
        host.sync_interrupts_now();
    }
    assert!(
        !plic.borrow().pending(11),
        "没有新沿 ⇒ ⇒ pinmux 路径也不活锁（一次性语义）"
    );
}

/// **K-8** —— **方向行为**：output 引脚由固件写 OUT 驱动电平、input 引脚
/// 由 `set_external_inputs` 喂入 ⇒ ⇒ 两种方向下的沿都会抬**当前功能**的源
/// （GPIO 的方向语义不被 pinmux 引入破坏）。
#[test]
fn k8_both_directions_still_latch_through_the_pinmux_path() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_single_function());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    let plic = plic_of(&host, "PLIC");
    let gpio = gpio_of(&host, "GPIO0");

    // ---- input 方向（DIR = 0 · 默认）⇒ `set_external_inputs` 可喂 ----
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(11),
        "input 引脚：外部喂入的沿 ⇒ ⇒ 抬当前功能（gpio_in）的源 11"
    );
    plic.borrow_mut().complete(0, 11);

    // ---- output 方向（DIR = 1<<3）⇒ 固件写 OUT 驱动电平可见 ----
    host.bus_mut()
        .write(GPIO0_BASE + 0x0, crate::m14_bus::BusWidth::Word, 1u32 << 3)
        .expect("固件写 GPIO 的 DIR");
    host.bus_mut()
        .write(GPIO0_BASE + 0x4, crate::m14_bus::BusWidth::Word, 1u32 << 3)
        .expect("固件写 GPIO 的 OUT 驱动电平");
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(11),
        "output 引脚：固件写 OUT 的沿 ⇒ ⇒ 照常抬源 11（方向语义未被破坏）"
    );
}

/// **K-10** —— **pinmux 运行时**：写**不存在的功能索引**（越界）⇒ ⇒
/// `E_PLT-23`（报错不静默）。
#[test]
fn k10_an_out_of_range_function_index_is_rejected_with_e_plt_23() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_two_functions());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **器件层**（E_PLT-23 的**诞生地**）：该引脚只有 2 个功能（索引
    //   0/1）⇒ ⇒ 直接写 2 必须被拒（码 23 · 报错不静默）。
    let mux = host
        .device_table()
        .get("PINMUX")
        .and_then(|entry| match &entry.handle {
            platform::DeviceHandle::Pinmux(handle) => Some(std::rc::Rc::clone(handle)),
            _ => None,
        })
        .expect("宿主手里有那台 PINMUX");
    let err = mux
        .borrow_mut()
        .write(4 * 3, riscv_sim_contract::model::WriteSize::Word, 2)
        .expect_err("越界的功能索引必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY, "必须是平台族错误");
    assert_eq!(err.scenario, 23, "必须是 E_PLT-23（报错不静默）");
    // ⚠️ 非法写**不落地**：当前功能仍是之前那个（默认 0 ⇒ uart_tx）。
    assert_eq!(
        mux.borrow().current_function(3),
        Some(0),
        "越界写必须被拒且不改变当前功能"
    );
    // ⚠️ **总线层**（固件视角）：外设错误被总线**包一层** ⇒ ⇒ 固件看到的是
    //   `E_BUS-05`（Bus · 5）⇒ ⇒ 不是静默吞掉（报错不静默 · 语义守恒）。
    let err = host
        .bus_mut()
        .write(PINMUX_BASE + 4 * 3, crate::m14_bus::BusWidth::Word, 2u32)
        .expect_err("总线写越界功能索引也必须被拒（E_BUS-05 包装）");
    assert_eq!(
        err.family,
        crate::m14_bus::BUS_FAMILY,
        "总线层必须是 Bus 族"
    );
    assert_eq!(err.scenario, 5, "总线层必须是 E_BUS-05（peripheral error）");
    assert_eq!(
        mux.borrow().current_function(3),
        Some(0),
        "总线写越界同样不落地"
    );
    // ⚠️ **对照**：`parse_gpio_pin_name` 与路由表同源 —— pin3 的解析结果
    //   必须是 3（⇒ ⇒ 上面所有测试的引脚号都来自同一个解析规则）。
    assert_eq!(parse_gpio_pin_name("pin3"), Some(3));
}
