//! STAGE-43 · v0.66 批 —— **内核侧中间层**的宿主侧门禁（K 组）。
//!
//! ⚠️ **本文件钉的是「内核怎么算」**：器件类型注册表的内容 + 逐实例状态
//! 的**推导**（已绑定 / 待生效）。
//!
//! ⚠️ **断言全部落在输出上**：命令回的事件 ⇒ ⇒ 没有一条断言是「我调用了
//! 某个内部函数」。
//!
//! ⚠️ **判别力**：
//! - 把 `base` 一律填生效图的基址 ⇒ ⇒ 「待生效」那条**变红**（K-7）；
//! - 把注册表的窗口大小写死 ⇒ ⇒ K-2 **变红**；
//! - 事件**不推** ⇒ ⇒ 真机门禁那条**变红**。

use crate::kernel_host::platform;
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload};

use super::tests_watchdog::dispatch;
use super::KernelHost;

/// 造一个**最小**的宿主（不装载任何工程）。
fn host() -> KernelHost {
    KernelHost::new()
}

/// 一条只含 RAM 段的描述。
fn ram_only_description(id: &str) -> PlatformDescription {
    PlatformDescription {
        schema_version: riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        id: id.to_string(),
        name: id.to_string(),
        priority: 100,
        region_budget_bytes: 1024 * 1024,
        max_region_bytes: None,
        regions: vec![RegionSpec::Ram {
            base: Some(0),
            size: 1024 * 1024,
        }],
        description: None,
        wiring: None,
        roles: None,
        pins: None,
        mcu: None,
    }
}

/// 一条含 RAM + 两个外设窗口的描述（GPIO0 @ 0x4000_0000 · UART0 @ 0x1000_0000）。
fn board_description(id: &str) -> PlatformDescription {
    let mut description = ram_only_description(id);
    description.regions.push(RegionSpec::Peripheral {
        base: Some(0x4000_0000),
        device: DeviceSpec::Gpio {
            id: "GPIO0".to_string(),
        },
    });
    description.regions.push(RegionSpec::Peripheral {
        base: Some(0x1000_0000),
        device: DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 1,
            baud_rate: 115200,
        },
    });
    description
}

/// 事件里的器件类型表。
fn types_of(events: &[EventPayload]) -> Vec<riscv_sim_contract::model::DeviceTypeInfo> {
    events
        .iter()
        .find_map(|event| match event {
            EventPayload::DeviceTypes { types } => Some(types.clone()),
            _ => None,
        })
        .expect("没有收到 DeviceTypes 事件")
}

/// 事件里的逐实例清单。
fn instances_of(events: &[EventPayload]) -> Vec<riscv_sim_contract::model::DeviceInstanceInfo> {
    events
        .iter()
        .find_map(|event| match event {
            EventPayload::DeviceInstances { instances } => Some(instances.clone()),
            _ => None,
        })
        .expect("没有收到 DeviceInstances 事件")
}

// ---------------------------------------------------------------------------
// K 组
// ---------------------------------------------------------------------------

/// K-1 —— 类型注册表**含 6 种**（内核实现了 6 种；那个定时器**如实**报出来）。
#[test]
fn the_type_registry_reports_the_six_types_the_kernel_implements() {
    let mut host = host();
    let events = dispatch(&mut host, CommandPayload::ListDeviceTypes);
    let types = types_of(&events);
    let kinds: Vec<&str> = types.iter().map(|t| t.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "clint",
            "gpio",
            "pinmux",
            "plic",
            "timer_riscv",
            "uart16550"
        ],
        "类型注册表的内容不对（顺序按类型名升序）",
    );
}

/// K-2 —— 每个类型的**窗口大小**与器件模型自报的那一个**逐字相同**。
#[test]
fn every_type_reports_the_window_size_the_device_model_self_reports() {
    let mut host = host();
    let events = dispatch(&mut host, CommandPayload::ListDeviceTypes);
    let types = types_of(&events);
    let window = |kind: &str| {
        types
            .iter()
            .find(|t| t.kind == kind)
            .map(|t| t.window_bytes)
            .unwrap_or_else(|| panic!("类型表里没有 {kind}"))
    };
    assert_eq!(window("gpio"), crate::m15_peripheral::gpio::GPIO_WINDOW);
    assert_eq!(
        window("uart16550"),
        crate::m15_peripheral::uart::UART_WINDOW
    );
    assert_eq!(window("clint"), crate::m15_peripheral::clint::CLINT_WINDOW);
    assert_eq!(window("plic"), crate::m15_peripheral::plic::PLIC_WINDOW);
    assert_eq!(
        window("timer_riscv"),
        crate::m15_peripheral::timer::TIMER_WINDOW
    );
}

/// K-3 —— 寄存器表**只含偏移**（没有基址）⇒ ⇒ 集成时能绑到任意基址。
#[test]
fn the_register_table_carries_offsets_only() {
    let mut host = host();
    let events = dispatch(&mut host, CommandPayload::ListDeviceTypes);
    let types = types_of(&events);
    let uart = types
        .iter()
        .find(|t| t.kind == "uart16550")
        .expect("类型表里没有 uart16550");
    assert_eq!(uart.registers[0].name, "THR");
    assert_eq!(uart.registers[0].offset, 0);
    assert_eq!(
        uart.registers[7].name, "SCR",
        "ns16550 的寄存器表缺项（THR..SCR 共 8 个）",
    );
    assert_eq!(uart.registers[7].offset, 7);
}

/// K-4 —— 参数合法区间**引用契约层那一份**（PLIC 的源数上界 = 1024）。
#[test]
fn the_parameter_domain_is_the_contract_layers_one() {
    let mut host = host();
    let events = dispatch(&mut host, CommandPayload::ListDeviceTypes);
    let types = types_of(&events);
    let plic = types
        .iter()
        .find(|t| t.kind == "plic")
        .expect("类型表里没有 plic");
    let sources = plic
        .params
        .iter()
        .find(|p| p.name == "sources")
        .expect("plic 的参数表里没有 sources");
    assert_eq!(sources.min, Some(1));
    assert_eq!(
        sources.max,
        Some(u64::from(riscv_sim_contract::platform::MAX_PLIC_SOURCES)),
    );
    // ⚠️ `baud_rate` **没有**上界（架构没规定）⇒ ⇒ 不许编一个。
    let uart = types
        .iter()
        .find(|t| t.kind == "uart16550")
        .expect("类型表里没有 uart16550");
    let baud = uart
        .params
        .iter()
        .find(|p| p.name == "baud_rate")
        .expect("uart 的参数表里没有 baud_rate");
    assert_eq!(baud.max, None);
}

/// K-5 —— 建图之后，逐实例清单的条目数 **==** 生效图里的器件数。
#[test]
fn the_instance_list_has_one_entry_per_device_in_the_effective_map() {
    let mut host = host();
    // 先设一块板（未装载镜像 ⇒ 立即生效）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_description("board-a"),
            expected_revision: None,
        },
    );
    let events = dispatch(&mut host, CommandPayload::ListDeviceInstances);
    let instances = instances_of(&events);
    assert_eq!(instances.len(), 2, "两个外设窗口 ⇒ 两条实例记录");
    let ids: Vec<&str> = instances.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["GPIO0", "UART0"],
        "实例清单没按 id 升序（或内容不对）"
    );
}

/// K-6 —— 每个实例的 `base` **等于**它在生效图里的基址（已绑定）。
#[test]
fn every_instance_reports_the_base_address_it_is_bound_to() {
    let mut host = host();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_description("board-a"),
            expected_revision: None,
        },
    );
    let events = dispatch(&mut host, CommandPayload::ListDeviceInstances);
    let instances = instances_of(&events);
    let gpio = instances
        .iter()
        .find(|i| i.id == "GPIO0")
        .expect("没有 GPIO0");
    assert_eq!(gpio.base, Some(0x4000_0000), "GPIO0 的基址不对");
    assert_eq!(gpio.kind, "gpio");
    assert_eq!(gpio.window_bytes, crate::m15_peripheral::gpio::GPIO_WINDOW);
    let uart = instances
        .iter()
        .find(|i| i.id == "UART0")
        .expect("没有 UART0");
    assert_eq!(uart.base, Some(0x1000_0000), "UART0 的基址不对");
}

/// K-7 —— **待生效**：改了板但**还没装载** ⇒ 那些实例的 `base` 是 `None`。
#[test]
fn an_instance_of_a_board_that_is_not_in_effect_yet_has_no_base() {
    let mut host = host();
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
    // 先装一块板并**装载镜像** ⇒ 之后改板**挂起待生效**。
    let project = Project {
        id: "p".to_string(),
        name: "p".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(super::DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion {
            schema: 1,
            isa: riscv_sim_contract::model::ISA.to_string(),
            toolchain: "test".to_string(),
        },
    };
    dispatch(&mut host, CommandPayload::LoadProject(project));
    // 换一块板（多一个器件、改一个基址）⇒ 挂起待生效。
    let mut board = board_description("board-b");
    board.regions.push(RegionSpec::Peripheral {
        base: Some(0x2000_0000),
        device: DeviceSpec::Gpio {
            id: "GPIO1".to_string(),
        },
    });
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board,
            expected_revision: None,
        },
    );
    let events = dispatch(&mut host, CommandPayload::ListDeviceInstances);
    let instances = instances_of(&events);
    // ⚠️ **待生效的板**才是主角 ⇒ ⇒ 报的是它（用户板优先，裁决 K）。
    assert_eq!(instances.len(), 3, "待生效的板上有三个器件实例");
    let gpio1 = instances
        .iter()
        .find(|i| i.id == "GPIO1")
        .expect("没有 GPIO1");
    assert_eq!(gpio1.base, None, "还没生效的实例**不许**有基址");
    // ⚠️ **已生效的那两个**（id 与基址都没变）⇒ ⇒ 仍然有基址。
    let gpio0 = instances
        .iter()
        .find(|i| i.id == "GPIO0")
        .expect("没有 GPIO0");
    assert_eq!(gpio0.base, Some(0x4000_0000), "没改的实例不该变成待生效");
}

/// K-8 —— 改板**被接受**之后 ⇒ ⇒ 逐实例状态**跟着更新**（同一个落点）。
///
/// ⚠️ **要装载过镜像** ⇒ ⇒ 改板**挂起待生效** ⇒ ⇒ 那条指令才有「变化」
/// 可看 ⇒ ⇒ 否则（未装载）改板**立即生效** ⇒ ⇒ 实例表里那个实例**仍然**
/// 有基址（只是基址变了）。
#[test]
fn accepting_a_board_change_updates_the_instance_state() {
    let mut host = host();
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
    // 装载镜像 ⇒ 之后的改板**挂起待生效**。
    let project = Project {
        id: "p".to_string(),
        name: "p".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(super::DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion {
            schema: 1,
            isa: riscv_sim_contract::model::ISA.to_string(),
            toolchain: "test".to_string(),
        },
    };
    dispatch(&mut host, CommandPayload::LoadProject(project));
    // 先设一块板（挂起待生效）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_description("board-a"),
            expected_revision: None,
        },
    );
    // 改 UART0 的基址 ⇒ ⇒ 那个实例就**不在**生效的图里了。
    let mut board = board_description("board-b");
    for region in board.regions.iter_mut() {
        if let RegionSpec::Peripheral { base, device } = region {
            if device.id() == "UART0" {
                *base = Some(0x2000_0000);
            }
        }
    }
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board,
            expected_revision: None,
        },
    );
    // ⚠️ **同一个时机**推逐实例状态（裁决 G）⇒ ⇒ 不用再发一次查询命令。
    let instances = instances_of(&events);
    let uart = instances
        .iter()
        .find(|i| i.id == "UART0")
        .expect("没有 UART0");
    assert_eq!(
        uart.base, None,
        "基址变了 ⇒ ⇒ 那个实例**不再**在生效的图里 ⇒ ⇒ base 必须是 None",
    );
}
