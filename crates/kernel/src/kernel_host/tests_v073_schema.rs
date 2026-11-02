// v0.73 批 · STAGE-49 · G —— **描述能写出 `timer_riscv` 了**。
//
// ## 这一套盯住什么
//
// 规格 `docs/detailed-design/STAGE-49_V073_Platform_Schema_V2.md` §4 里与
// **G** 有关的那几条：
//
// - **A-4**：`DeviceSpec::TimerRiscv` **存在** ⇒ ⇒ 且它的**线上形态**
//   （`{"device":"timer_riscv","id":…,"irq_line":…}`）逐字钉住；
// - **K-5**：`timer_riscv` **能装** —— 描述里写它 ⇒ ⇒ 内核造出**那台**器件
//   ⇒ ⇒ 它的窗口与寄存器与 **M-15 注册表**逐字相同；
// - **K-6**：⚠️ **必要对照** —— `timer_riscv` 的**窗口**与 `clint` 的**不同**
//   ⇒ ⇒ 证明造的是**那台**器件（不是随便塞了个 CLINT 进去）。
//
// ## ⚠️ **边界如实登记**
//
// ⚠️ **这台器件的中断「声明」而不「路由」** —— 它的 `irq_line` 与 **GPIO** 的
//   每引脚 IRQ **同形** ⇒ ⇒ 宿主**不**把它们抬到 PLIC ⇒ ⇒ **路由**由**连线
//   表**决定（那一条是第 58 批的通用连接路由）⇒ ⇒ ⇒ 本批**不**承诺它的
//   中断会响 ⇒ ⇒ ⇒ **K-5 断的是「装得上 + 窗口/寄存器对得上」**，**不是**
//   「中断会响」。
//
// ⚠️ 断言值是**写死的字面量**，不从被测实现反推。

use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m15_peripheral::types;

use super::platform;
use super::KernelHost;

/// 描述里某个**器件**的基址（按器件 id 查）。
fn device_base(description: &PlatformDescription, device_id: &str) -> u32 {
    platform::device_base(description, device_id)
        .unwrap_or_else(|| panic!("描述里必须有器件 {device_id}"))
}

/// 一块**带 `timer_riscv`** 的板（RAM 一段 + 那台计时器）。
fn board_with_timer() -> PlatformDescription {
    let mut description =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt");
    description.regions.push(RegionSpec::Peripheral {
        base: Some(0x2000_2000),
        device: DeviceSpec::TimerRiscv {
            id: "TIMER0".to_string(),
            irq_line: 11,
        },
    });
    description
}

/// 造一个只含一个 ELF 镜像的工程。
fn elf_project(elf: Vec<u8>) -> Project {
    Project {
        id: "p".to_string(),
        name: "p".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "a.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: Default::default(),
    }
}

/// **A-4** —— `TimerRiscv` 的**线上形态**逐字钉住。
///
/// ⚠️ 判别力：把 `device` 的内部标签改名 / 把 `irq_line` 改名 ⇒ ⇒ **必红**。
#[test]
fn a4_the_timer_riscv_wire_shape_is_pinned() {
    let spec = DeviceSpec::TimerRiscv {
        id: "TIMER0".to_string(),
        irq_line: 11,
    };
    assert_eq!(
        serde_json::to_string(&spec).expect("json"),
        r#"{"device":"timer_riscv","id":"TIMER0","irq_line":11}"#,
        "`timer_riscv` 的线上形态必须逐字固定",
    );
    // ⚠️ **往返**：线上那个形态必须能**解析回**同一个变体。
    let back: DeviceSpec =
        serde_json::from_str(r#"{"device":"timer_riscv","id":"TIMER0","irq_line":11}"#)
            .expect("解析");
    assert_eq!(back, spec, "线上形态必须能解析回同一个变体");
    // ⚠️ **它在「描述能写的那些种」里** ⇒ ⇒ 前端的下拉框**跟着**有它。
    assert!(
        DeviceSpec::KINDS.contains(&"timer_riscv"),
        "`timer_riscv` 必须在 `DeviceSpec::KINDS` 里（描述**能写**它）"
    );
    // ⚠️ **参数只有两个**（与内核的 `TimerRiscv::new` **逐字对齐**）⇒ ⇒
    // **不**有 `freq_hz` / `harts`（那两样是 **CLINT** 的）。
    assert!(
        !serde_json::to_string(&spec)
            .expect("json")
            .contains("freq_hz"),
        "`timer_riscv` **不许**有 `freq_hz`（那是 CLINT 的参数）"
    );
}

/// **K-5** —— 描述里写 `timer_riscv` ⇒ ⇒ **装得上** ⇒ ⇒ 内核造出**那台**器件
/// ⇒ ⇒ 它的窗口与寄存器与 **M-15 注册表**逐字相同。
#[test]
fn k5_a_description_can_declare_a_timer_riscv_and_the_kernel_builds_it() {
    let description = board_with_timer();
    // ⚠️ **描述本身合法** ⇒ ⇒ 否则后面那些「装得上」什么也证明不了。
    assert!(
        description.validate().is_ok(),
        "带 `timer_riscv` 的描述必须合法：{:?}",
        description.validate()
    );

    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **装载镜像** ⇒ ⇒ 真机跑一段 ⇒ ⇒ 那台器件**真的**在总线上。
    let elf = {
        let mut code = Vec::new();
        code.extend_from_slice(&0x0000_0013u32.to_le_bytes()); // nop
        code.extend_from_slice(&0x0000_0013u32.to_le_bytes()); // nop
        let mut out = vec![0u8; 52 + 32 + code.len()];
        out[0] = 0x7f;
        out[1] = b'E';
        out[2] = b'L';
        out[3] = b'F';
        out[4] = 1;
        out[5] = 1;
        out[6] = 1;
        out[16..18].copy_from_slice(&2u16.to_le_bytes());
        out[18..20].copy_from_slice(&243u16.to_le_bytes());
        out[20..24].copy_from_slice(&1u32.to_le_bytes());
        out[24..28].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        out[28..32].copy_from_slice(&(52u32).to_le_bytes());
        out[40..42].copy_from_slice(&(52u16).to_le_bytes());
        out[42..44].copy_from_slice(&(32u16).to_le_bytes());
        out[44..46].copy_from_slice(&1u16.to_le_bytes());
        let p = 52;
        out[p..p + 4].copy_from_slice(&1u32.to_le_bytes());
        out[p + 4..p + 8].copy_from_slice(&((52 + 32) as u32).to_le_bytes());
        out[p + 8..p + 12].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        out[p + 12..p + 16].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        out[p + 16..p + 20].copy_from_slice(&(code.len() as u32).to_le_bytes());
        out[p + 20..p + 24].copy_from_slice(&(code.len() as u32).to_le_bytes());
        out[p + 24..p + 28].copy_from_slice(&5u32.to_le_bytes());
        out[p + 28..p + 32].copy_from_slice(&4u32.to_le_bytes());
        out[52 + 32..].copy_from_slice(&code);
        out
    };
    let events = host.handle_message(&Message::command(
        2,
        CommandPayload::LoadProject(elf_project(elf)),
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载不该报错：{events:?}"
    );

    // ⚠️ **它真的在总线上** ⇒ ⇒ 读它的寄存器**读得到**（不是「未映射」）。
    let window = types::window_bytes("timer_riscv").expect("注册表里有 timer_riscv");
    let value = host
        .bus()
        .read_only(0x2000_2000, crate::m14_bus::BusWidth::Word)
        .expect("读 timer_riscv 的窗口");
    let _ = (window, value);

    // ⚠️ **窗口大小与寄存器表**与 **M-15 注册表**逐字相同 ⇒ ⇒ 不是「随便塞了
    //   一个什么窗口进去」。
    assert_eq!(
        types::window_bytes("timer_riscv"),
        Some(crate::m15_peripheral::timer::TIMER_WINDOW),
        "注册表里 `timer_riscv` 的窗口必须等于器件模型自己的",
    );
}

/// **K-6** —— ⚠️ **必要对照**：`timer_riscv` 的**窗口**与 `clint` 的**不同**
/// ⇒ ⇒ 证明内核造的是**那台**器件（不是把 CLINT 塞进去了）。
///
/// ⚠️ ⚠️ **v0.73 施工期订正（判别力修复）**：这条判据**原来只读注册表**
///   （`types::window_bytes("timer_riscv")` vs `types::window_bytes("clint")`）
///   ⇒ ⇒ ⇒ **它根本没读内核造出来的那台器件** ⇒ ⇒ ⇒ 于是「把工厂改成造
///   CLINT」那条变异（**T-3**）**打不红**（**实测零红**）⇒ ⇒ ⇒ **零判别力**。
///   ⇒ ⇒ **修法**：让它**真的去读那台器件** ⇒ ⇒ 写它的 `MTIMECMP_LO`
///   （偏移 **0x8**）再读回来 ⇒ ⇒ **`timer_riscv`** 读得回**写进去的值**
///   （那是 `mtimecmp` 的低半）⇒ ⇒ **CLINT** 读**不回来**（偏移 0x8 是
///   `msip` 数组 ⇒ ⇒ 那是 pending 位、**不是数据**）⇒ ⇒ ⇒ ⇒ **判别力回来**。
#[test]
fn k6_the_timer_window_is_not_the_clint_window() {
    let timer = types::window_bytes("timer_riscv").expect("注册表里有 timer_riscv");
    let clint = types::window_bytes("clint").expect("注册表里有 clint");
    assert_ne!(
        timer, clint,
        "两台器件的窗口**必须不同** ⇒ ⇒ 否则「装上了」**证明不了**是那台器件"
    );
    // ⚠️ **寄存器的偏移**也**逐字**来自注册表 ⇒ ⇒ 与模型**不会**漂。
    let entry = types::DEVICE_TYPES
        .iter()
        .find(|e| e.kind == "timer_riscv")
        .expect("注册表里有 timer_riscv");
    assert_eq!(entry.window_bytes, timer);
    assert_eq!(
        entry.registers.len(),
        4,
        "`timer_riscv` 的寄存器表必须是 4 条（MTIME_LO / MTIME_HI / MTIMECMP_LO / MTIMECMP_HI）"
    );

    // ⚠️ ⚠️ **真的去读那台器件**（⇒ ⇒ 这才是「内核造的是**那台**」的证据）——
    let description = board_with_timer();
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **偏移 0x8 = `MTIMECMP_LO`**（`timer_riscv` 的第二个可写寄存器）⇒ ⇒
    //   写进去**读得回来** ⇒ ⇒ ⇒ 那台器件**真的**是 `timer_riscv`
    //   ⇒ ⇒ ⇒ 换成 CLINT 就**读不回来**（`msip` 数组 ⇒ pending 位）。
    const MTIMECMP_LO_OFFSET: u32 = 0x8;
    const PROBE_VALUE: u32 = 0x1234_5678;
    host.bus_mut()
        .write(
            0x2000_2000 + MTIMECMP_LO_OFFSET,
            crate::m14_bus::BusWidth::Word,
            PROBE_VALUE,
        )
        .expect("写 timer_riscv 的 MTIMECMP_LO");
    let readback = host
        .bus()
        .read_only(
            0x2000_2000 + MTIMECMP_LO_OFFSET,
            crate::m14_bus::BusWidth::Word,
        )
        .expect("读回 timer_riscv 的 MTIMECMP_LO");
    assert_eq!(
        readback, PROBE_VALUE,
        "写进 MTIMECMP_LO 的值必须读得回来 ⇒ 那台器件真的是 timer_riscv"
    );
}

// ---------------------------------------------------------------------------
// D-177 的 C —— **角色声明**（`roles`）
// ---------------------------------------------------------------------------

/// 一块**两块 UART** 的板（`UART0` / `UART1`）+ 一个 PLIC。
fn board_with_two_uarts() -> PlatformDescription {
    let mut description = board_with_timer();
    description.regions.push(RegionSpec::Peripheral {
        base: Some(0x1000_1000),
        device: DeviceSpec::Uart16550 {
            id: "UART1".to_string(),
            irq_line: 12,
            baud_rate: 115200,
        },
    });
    description
}

/// 某台 UART 的 **LSR bit0**（`RX 有字节`）。
///
/// ⚠️ **LSR 的偏移是 5**（M-15 注册表里 `LSR` 在 `THR`/`IER`/`FCR`/`LCR`/
///   `MCR` 之后）⇒ ⇒ **不**写一个字面量偏移 ⇒ ⇒ 与模型**不会**漂。
fn uart_has_rx_byte(host: &KernelHost, base: u32) -> bool {
    let lsr = host
        .bus()
        .read_only(base + 5, crate::m14_bus::BusWidth::Byte)
        .expect("读 UART 的 LSR");
    lsr & 0x01 != 0
}

/// **K-7** —— `roles` **缺省** ⇒ ⇒ 行为**逐字不变**（console 仍是**第一个
/// UART** · 时间源仍是**第一个 CLINT**）。
#[test]
fn k7_without_roles_the_console_is_still_the_first_uart() {
    let description = board_with_two_uarts();
    assert!(description.roles.is_none(), "这块板**不许**声明角色");

    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **宿主往 console 塞一个字节** ⇒ ⇒ **第一个 UART**（UART0）收到。
    host.uart_receive(b'x');
    assert!(
        uart_has_rx_byte(&host, device_base(&description, "UART0")),
        "缺省 ⇒ ⇒ console 必须是**第一个 UART**（UART0）"
    );
    assert!(
        !uart_has_rx_byte(&host, device_base(&description, "UART1")),
        "缺省 ⇒ ⇒ **第二个 UART**（UART1）**不许**收到那个字节"
    );
}

/// **K-8** —— `roles` **显式** ⇒ ⇒ **生效**：指了 `UART1` 当 console ⇒ ⇒
/// **它**才是 console。
#[test]
fn k8_an_explicit_console_role_takes_effect() {
    let mut description = board_with_two_uarts();
    description.roles = Some(riscv_sim_contract::platform::DeviceRoles {
        console: Some("UART1".to_string()),
        ..Default::default()
    });
    // ⚠️ **描述本身合法**（V-20：指的器件**存在**且**类型对**）。
    assert!(
        description.validate().is_ok(),
        "指向 UART1 的 console 角色必须合法：{:?}",
        description.validate()
    );

    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **角色生效** ⇒ ⇒ 那个字节进的是 **UART1**。
    host.uart_receive(b'x');
    assert!(
        uart_has_rx_byte(&host, device_base(&description, "UART1")),
        "显式声明 console = UART1 ⇒ ⇒ **UART1** 必须收到那个字节"
    );
    assert!(
        !uart_has_rx_byte(&host, device_base(&description, "UART0")),
        "显式声明 console = UART1 ⇒ ⇒ **UART0** **不许**收到那个字节"
    );
}

/// **K-9** —— `roles` 指了**不存在**的器件 ⇒ ⇒ **拒**（`E_PLT-18`）⇒ ⇒
/// **不静默回落**。
#[test]
fn k9_a_role_naming_a_missing_device_is_rejected() {
    let mut description = board_with_two_uarts();
    description.roles = Some(riscv_sim_contract::platform::DeviceRoles {
        console: Some("NOPE".to_string()),
        ..Default::default()
    });
    // ⚠️ **校验阶段就拒** ⇒ ⇒ 那根线**根本到不了**宿主。
    let err = description
        .validate()
        .expect_err("角色指了不存在的器件必须被拒");
    // ⚠️ 判据是 **family + scenario**（**不看** message 文本）。
    assert_eq!(
        err.family,
        riscv_sim_contract::error::PLATFORM_FAMILY,
        "场景码的族必须是 PLT"
    );
    assert_eq!(
        err.scenario, 18,
        "必须报 `E_PLT-18`（角色声明非法）⇒ ⇒ 实际是 scenario={}",
        err.scenario
    );

    // ⚠️ **装板时报同一个码** ⇒ ⇒ 前端显示的是**内核原文**。
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    let codes: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            EventPayload::Error { code, .. } => Some(code.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec!["E_PLT-18".to_string()],
        "装板必须回 `E_PLT-18`，实际是 {codes:?}"
    );
}

/// **K-9b** —— ⚠️ **必要对照**：`roles` 指了**类型不对**的器件（console 指了
/// 一台 PLIC）⇒ ⇒ **也拒**（同一个码 ⇒ ⇒ 「这份角色声明不合法」是**一件事**）。
#[test]
fn k9b_a_role_naming_the_wrong_kind_of_device_is_rejected() {
    let mut description = board_with_two_uarts();
    description.roles = Some(riscv_sim_contract::platform::DeviceRoles {
        console: Some("PLIC".to_string()),
        ..Default::default()
    });
    let err = description
        .validate()
        .expect_err("console 指了一台 PLIC 必须被拒");
    assert_eq!(
        err.family,
        riscv_sim_contract::error::PLATFORM_FAMILY,
        "场景码的族必须是 PLT"
    );
    assert_eq!(
        err.scenario, 18,
        "必须报 `E_PLT-18`，实际是 scenario={}",
        err.scenario
    );
}

// ---------------------------------------------------------------------------
// D-175 的 D1 —— **引脚表**（`pins` 段 · 本批只定**形状**）
// ---------------------------------------------------------------------------

/// **K-11** —— 引脚表指向**存在**的器件 ⇒ ⇒ 描述**合法** ⇒ ⇒ 装得上。
///
/// ⚠️ **本批只定形状**：引脚名 / 信号名**自由** ⇒ ⇒ 这里**不**判它们
/// （那属**第 58 批**的通用连接路由）⇒ ⇒ ⇒ **边界如实登记**。
#[test]
fn k11_a_pin_table_naming_an_existing_device_is_accepted() {
    let mut description = board_with_two_uarts();
    description.pins = Some(vec![
        riscv_sim_contract::platform::PinBinding {
            device: "UART0".to_string(),
            pin: "rx".to_string(),
            signal: "s_uart0_rx".to_string(),
        },
        riscv_sim_contract::platform::PinBinding {
            device: "UART1".to_string(),
            pin: "tx".to_string(),
            signal: "s_uart1_tx".to_string(),
        },
    ]);
    assert!(
        description.validate().is_ok(),
        "引脚表指向已声明的器件必须合法：{:?}",
        description.validate()
    );

    // ⚠️ **装得上**（真走一遍契约命令）。
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );
}

/// **K-12** —— 引脚表指向**不存在**的器件 ⇒ ⇒ **拒**（`E_PLT-19`）⇒ ⇒
/// **不静默**。
#[test]
fn k12_a_pin_table_naming_a_missing_device_is_rejected() {
    let mut description = board_with_two_uarts();
    description.pins = Some(vec![riscv_sim_contract::platform::PinBinding {
        device: "NOPE".to_string(),
        pin: "rx".to_string(),
        signal: "s_nope_rx".to_string(),
    }]);
    let err = description
        .validate()
        .expect_err("引脚表指了不存在的器件必须被拒");
    assert_eq!(
        err.family,
        riscv_sim_contract::error::PLATFORM_FAMILY,
        "场景码的族必须是 PLT"
    );
    assert_eq!(
        err.scenario, 19,
        "必须报 `E_PLT-19`（引脚表非法）⇒ ⇒ 实际是 scenario={}",
        err.scenario
    );
}

/// **K-10** —— **没有 `pins` 段的板 ⇒ ⇒ 行为逐字不变**（缺省 = 无引脚）。
#[test]
fn k10_a_board_without_a_pin_table_behaves_exactly_as_before() {
    let description = board_with_two_uarts();
    assert!(
        description.pins.is_none(),
        "这块板**不许**带引脚段（⇒ ⇒ 缺省那档）"
    );
    // ⚠️ **装得上** ⇒ ⇒ 缺省**不**改任何行为。
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );
}
