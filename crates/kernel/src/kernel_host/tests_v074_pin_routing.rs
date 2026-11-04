// v0.74 批 · STAGE-50 · **B2 + E** —— 引脚级连线 + 通用连接路由。
//
// ## 这一套盯住什么
//
// 规格 `docs/detailed-design/STAGE-50_V074_Pin_Level_Routing.md` §4 里与
// **B2（引脚层）** 与 **E（通用连接路由）** 有关的那几条：
//
// - **K-1**：**引脚级连线真的通电** —— 描述里写「GPIO0 的**引脚 3** ⇒ PLIC
//   源 11」⇒ ⇒ 内核**真的**把那个源的**挂起位**置上（⇒ ⇒ guest **claim
//   得到**它）⇒ ⇒ **不是**「描述被接受了」那种弱证据；
// - **K-2**：⚠️ **必要对照** —— GPIO 的**一次性**语义 ⇒ ⇒ 抬过一次之后**不再
//   重抬**（⇒ ⇒ guest claim + complete 之后**源不再挂起**）⇒ ⇒ **不活锁**；
// - **K-3**：⚠️ **必要对照** —— **UART 的电平语义逐字不变**（FIFO 空 ⇒ ⇒ 源
//   **撤销**）⇒ ⇒ 既有 19 条 D-158 用例**一条不许红**；
// - **K-4** ~ **K-7**：四条**非法描述**⇒ ⇒ 一律**拒**（既有码 `E_PLT-16` ⇒ ⇒
//   **零新场景码**）；
// - **K-8**：⚠️ **必要对照** —— **老描述（`rx_ready`）逐字不变** ⇒ ⇒ 存量板档案
//   **零风险**。
//
// ## ⚠️ **为什么判据是「guest claim 得到那个源」**
//
// ⚠️ 「描述被内核接受了」**证明不了**线真的通电（v0.71 批 D-177 那条缺陷
//   就是**描述被接受、线被静默丢弃**）⇒ ⇒ ⇒ 判据必须落在**架构层能观察到
//   的那件事**上 ⇒ ⇒ ⇒ guest **claim** 得到**那个源号**。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。

use riscv_sim_contract::error::{E_PLT_16_WIRING_INVALID, PLATFORM_FAMILY};
use riscv_sim_contract::platform::{PlatformDescription, WiringFrom, WiringSpec, WiringTo};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m15_peripheral::gpio::GpioBank;
use crate::m15_peripheral::plic::Plic;

use super::platform;
use super::KernelHost;

/// qemu-virt 里 **UART0** 的基址（`0x4000_0000`）。
const UART0_BASE: u32 = 268_435_456;

/// qemu-virt 描述（**带 GPIO0** 与 **64 源的 PLIC**）⇒ ⇒ 引脚级连线的板
/// 可以直接基于它。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 一块**带引脚级连线**的板：GPIO0 的**引脚 3** ⇒ PLIC 的**源 11**。
fn board_with_pin_wire() -> PlatformDescription {
    let mut description = qemu_virt_description();
    description.wiring = Some(vec![WiringSpec {
        from: WiringFrom {
            device: "GPIO0".to_string(),
            event: "pin_irq".to_string(),
            pin: Some(3),
        },
        to: WiringTo {
            device: "PLIC".to_string(),
            source: 11,
        },
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

/// **打开**某台 UART 的 **RX 中断**（IER bit0）—— 与既有测试**同一手法**
/// （总线上写 MMIO）。
///
/// ⚠️ **为什么必须**：电平判据是 **LSR bit0 ∧ IER bit0** ⇒ ⇒ **IER 没开**
///   ⇒ ⇒ 电平**永远不起来** ⇒ ⇒ ⇒ 「电平语义」这条**必须**先把 IER 打开。
fn enable_uart_rx_interrupt(host: &mut KernelHost, uart_base: u32) {
    host.bus_mut()
        .write(uart_base + 0x1, crate::m14_bus::BusWidth::Byte, 0x01)
        .expect("写 UART 的 IER");
}

/// 让 guest **能 claim** 某个源（**使能** + **优先级 > 门限`0`**）。
///
/// ⚠️ **为什么需要**：PLIC 的 `claim` 要**源已使能**且**优先级严格大于门限**
///   （默认门限 0）⇒ ⇒ 否则源**挂着**也 claim 不到（⇒ ⇒ 判据会「看起来」
///   失败 ⇒ ⇒ 其实只是**没使能**）。
fn enable_source(plic: &std::rc::Rc<std::cell::RefCell<Plic>>, source: u32) {
    let mut p = plic.borrow_mut();
    p.set_priority(source, 1);
    p.enable(0, source, true);
}

/// **A-1** —— **四个版本号**（描述格式 3 · 规则集 5 · 信封 11 · 快照 2）。
///
/// ⚠️ **为什么这一组在内核侧**（而不是只在契约层）：变异自检的**红集**要
///   **可复算**⇒ ⇒ ⇒ 同一批的判据**放在同一条门禁**里才量得出来 ⇒ ⇒ ⇒
///   （契约层自己也有那份 ⇒ ⇒ **两个落点**？—— **不**是：那边断的是「**这些
///   数被写在契约层**」，这边断的是「**这一批就是这几个数**」⇒ ⇒ 两个**不同**
///   的性质 ⇒ ⇒ 各有各的落点）。
#[test]
fn a1_the_four_version_numbers_are_the_current_ones() {
    assert_eq!(
        riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        4,
        "描述格式版本（v0.75 批升到 4：顶层加了可选的 `mcu` 段）"
    );
    assert_eq!(
        riscv_sim_contract::platform::PLATFORM_RULES_VERSION,
        6,
        "规则集版本（v0.75 批升到 6：加 V-23 —— `mcu` 段词法/形状校验）"
    );
    assert_eq!(
        riscv_sim_contract::SCHEMA_VERSION,
        11,
        "契约信封版本**不动**（本批的改动**不是**信封形状的变化）"
    );
    assert_eq!(
        riscv_sim_contract::model::SIM_STATE_SCHEMA_VERSION,
        2,
        "会话快照版本**不动**（⇒ ⇒ GPIO 的中断语义**没变** ⇒ ⇒ 快照不动）"
    );
}

/// **A-2** —— **老描述**（`schema_version = 2` · `wiring.from` **没有** `pin`）
/// ⇒ ⇒ **仍能解析**且**仍合法**（⇒ ⇒ **缺省优先** ⇒ ⇒ 存量板档案零风险）。
#[test]
fn a2_an_old_description_still_parses_and_is_still_legal() {
    let raw = r#"{
        "schema_version": 2,
        "id": "old-board",
        "name": "老板",
        "priority": 100,
        "auto_selectable": false,
        "region_budget_bytes": 1048576,
        "regions": [
            { "kind": "ram", "base": 0, "size": 1048576 },
            { "kind": "peripheral", "base": 268435456,
              "device": { "device": "uart16550", "id": "UART0", "irq_line": 10, "baud_rate": 115200 } },
            { "kind": "peripheral", "base": 201326592,
              "device": { "device": "plic", "id": "PLIC", "sources": 64, "contexts": 16 } }
        ],
        "wiring": [
            { "from": { "device": "UART0", "event": "rx_ready" },
              "to": { "device": "PLIC", "source": 10 } }
        ]
    }"#;
    let description: PlatformDescription =
        serde_json::from_str(raw).expect("老描述必须仍能解析（缺省优先）");
    assert_eq!(
        description.schema_version, 2,
        "老描述的版本号**不许**被改写"
    );
    assert!(
        description.wiring.as_ref().unwrap()[0].from.pin.is_none(),
        "老描述的 `wiring.from` **没有** `pin` ⇒ ⇒ 读出来必须是 `None`"
    );
    assert!(
        description.validate().is_ok(),
        "老描述必须仍然合法：{:?}",
        description.validate()
    );
}

/// **A-3** —— `wiring.from.pin` 的**线上形态**逐字钉住（**缺省时不带这个键**）。
#[test]
fn a3_the_pin_field_has_a_pinned_wire_shape() {
    // ⚠️ **带 pin** ⇒ ⇒ 线上**有**这个键。
    let with_pin = serde_json::json!({
        "from": { "device": "GPIO0", "event": "pin_irq", "pin": 3 },
        "to": { "device": "PLIC", "source": 11 }
    });
    let spec: WiringSpec =
        serde_json::from_value(with_pin.clone()).expect("带 pin 的连线必须能解析");
    assert_eq!(spec.from.pin, Some(3));
    assert_eq!(
        serde_json::to_value(&spec).expect("json"),
        with_pin,
        "带 pin 的连线往返必须**逐字相同**"
    );

    // ⚠️ **缺省** ⇒ ⇒ 线上**不带**这个键（⇒ ⇒ 老描述**逐字不变**）。
    let without_pin = serde_json::json!({
        "from": { "device": "UART0", "event": "rx_ready" },
        "to": { "device": "PLIC", "source": 10 }
    });
    let spec2: WiringSpec =
        serde_json::from_value(without_pin.clone()).expect("缺省的连线必须能解析");
    assert_eq!(spec2.from.pin, None);
    assert_eq!(
        serde_json::to_value(&spec2).expect("json"),
        without_pin,
        "缺省时线上**不许**多出 `pin` 这个键"
    );
}

/// **K-1** —— 引脚级连线**真的通电**：GPIO0 的**引脚 3** ⇒ PLIC 源 11 ⇒ ⇒
/// 内核**真的**把那个源的**挂起位**置上（⇒ ⇒ guest **claim 得到**它）。
#[test]
fn k1_a_pin_level_wire_really_carries_the_interrupt() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_pin_wire());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ ⚠️ **不许手工配线号**（v0.74 · STAGE-50 · 施工期订正 7）：那根引脚的
    //   IRQ 线号**必须**由**连线表**绑上去（⇒ ⇒ 「这根引脚接到哪个源」只有
    //   一处定义）⇒ ⇒ ⇒ 这里**手工**配了 ⇒ ⇒ 那条判据就**证明不了**「连线
    //   表真的把线接上了」⇒ ⇒ ⇒ 所以这里**什么都不配**。
    let gpio = gpio_of(&host, "GPIO0");

    // ⚠️ **制造一个边沿**（宿主驱动输入电平变化）⇒ ⇒ 设备锁存 pending。
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));

    // ⚠️ **跑一轮同步** ⇒ ⇒ 宿主把 pending 消费掉 ⇒ ⇒ 抬 PLIC 的源 11。
    host.sync_interrupts_now();

    let plic = plic_of(&host, "PLIC");
    assert!(
        plic.borrow().pending(11),
        "引脚 3 的 IRQ 必须把 PLIC 的**源 11**挂起（真的通电）"
    );
    enable_source(&plic, 11);
    // ⚠️ **guest 真的 claim 得到**那个源号（⇒ ⇒ 架构层能观察到）。
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(11),
        "guest 必须 claim 得到**那个源**（⇒ ⇒ 不是「描述被接受了」那种弱证据）"
    );
}

/// **K-1b** —— **连线表真的把引脚的 IRQ 线号绑上了**（⇒ ⇒ 「这根引脚接到
/// 哪个源」**只有一处**定义 ⇒ ⇒ 换板改连线不用改代码）。
///
/// ⚠️ **为什么单独一条**：K-1 断的是「**端到端**真的通电」⇒ ⇒ 若哪一步
///   坏了它也会红 ⇒ ⇒ ⇒ **归因**分不清 ⇒ ⇒ ⇒ 这里**直接**读那根引脚的
///   IRQ 线号（⇒ ⇒ 断「绑定」这**一件**事）。
#[test]
fn k1b_the_wiring_table_binds_the_pins_irq_line() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_pin_wire());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );
    let gpio = gpio_of(&host, "GPIO0");
    // ⚠️ **读那根引脚的线号**（M-15 的 `irq_lines[3]`）⇒ ⇒ 它必须就是连线
    //   里的**源号**（11）⇒ ⇒ ⇒ 「线号 == 源号」这条约定**成立**。
    assert_eq!(
        gpio.borrow().irq_line_for_pin(3),
        Some(11),
        "连线表必须把那根引脚的 IRQ 线号绑成连线上的源号（11）"
    );
    // ⚠️ **别的引脚不许被顺带绑上**（⇒ ⇒ 绑定是**逐根**的 ⇒ ⇒ 不是「整台
    //   器件一起配」）。
    assert_eq!(
        gpio.borrow().irq_line_for_pin(4),
        None,
        "没被连线提到的引脚**不许**被绑（否则又是「整台一起配」）"
    );
}

/// **K-1c** —— **guest 自己驱动那根引脚**（写 `TOGGLE`）⇒ ⇒ 那根线的 IRQ
/// **照样**抬起来 ⇒ ⇒ ⇒ **真机上也是这么走的**（固件写 TOGGLE ⇒ ⇒ 边沿）。
///
/// ⚠️ **为什么单独一条**：K-1 用的是**宿主驱动输入电平**（`set_external_inputs`）
///   ⇒ ⇒ 那是**调试**用的入口 ⇒ ⇒ ⇒ 真实固件是**自己写 `TOGGLE`** ⇒ ⇒ ⇒
///   那条路**必须**也通（⇒ ⇒ 否则真机上的固件永远领不到那个源）。
#[test]
fn k1c_a_guest_driven_toggle_also_raises_the_line() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_pin_wire());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    // ⚠️ **guest 自己写 `TOGGLE`**（偏移 0xc · 值 = 1 << 3）⇒ ⇒ 引脚 3 翻转
    //   ⇒ ⇒ **输出边沿** ⇒ ⇒ GPIO 锁存 pending。
    host.bus_mut()
        .write(0x4000_0000 + 0xc, crate::m14_bus::BusWidth::Word, 1u32 << 3)
        .expect("写 GPIO 的 TOGGLE");

    host.sync_interrupts_now();
    let plic = plic_of(&host, "PLIC");
    assert!(
        plic.borrow().pending(11),
        "guest 写 TOGGLE 造成的边沿**必须**把源 11 挂起（⇒ ⇒ 真机上固件走的正是这条路）"
    );
}

/// **K-1d** —— **装载工程**（`LoadProject`）之后，那根线**照样**通电 ⇒ ⇒ ⇒
/// 「装载工程」这条路上**绑定不许丢**（⇒ ⇒ 真机上的顺序正是「先配板、
/// 再装载工程」）。
///
/// ⚠️ **为什么单独一条**：`LoadProject` 会**重建平台**（⇒ ⇒ 外设**重建**
///   ⇒ ⇒ GPIO 的 `irq_lines` **回到未配置**）⇒ ⇒ ⇒ 那条绑定**必须**在
///   **重建之后**再做一遍（⇒ ⇒ 否则固件一装载、线就死了）。
#[test]
fn k1d_the_binding_survives_a_project_load() {
    use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};

    let mut host = KernelHost::new();
    // ⚠️ **先配板**（⇒ ⇒ 用户的板 = 引脚级那块）。
    let events = load_board(&mut host, board_with_pin_wire());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );
    // ⚠️ **再装载工程**（⇒ ⇒ 平台**重建** ⇒ ⇒ 外设**重建**）。
    let project = Project {
        id: "proj-pin".to_string(),
        name: "pin".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "pin.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(vec![0x13, 0x00, 0x00, 0x00]),
        }],
        version: Default::default(),
    };
    let events = host.handle_message(&Message::command(2, CommandPayload::LoadProject(project)));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装载工程不该报错：{events:?}"
    );

    // ⚠️ **绑定还在**（⇒ ⇒ 重建之后**又绑了一遍**）。
    let gpio = gpio_of(&host, "GPIO0");
    assert_eq!(
        gpio.borrow().irq_line_for_pin(3),
        Some(11),
        "装载工程之后那根引脚的 IRQ 线号**必须**还是连线上那个源号"
    );

    // ⚠️ **而且真的通电**（⇒ ⇒ guest 写 TOGGLE ⇒ ⇒ 源 11 挂起）。
    host.bus_mut()
        .write(0x4000_0000 + 0xc, crate::m14_bus::BusWidth::Word, 1u32 << 3)
        .expect("写 GPIO 的 TOGGLE");
    host.sync_interrupts_now();
    let plic = plic_of(&host, "PLIC");
    assert!(
        plic.borrow().pending(11),
        "装载工程之后那根线**照样**通电（⇒ ⇒ 真机上「先配板、再装载」的顺序成立）"
    );
}

/// **K-2** —— ⚠️ **必要对照** —— GPIO 的**一次性**语义 ⇒ ⇒ 抬过一次之后**不再
/// 重抬**（⇒ ⇒ guest claim + complete 之后**源不再挂起**）⇒ ⇒ **不活锁**。
#[test]
fn k2_a_pin_level_wire_does_not_retrigger() {
    let mut host = KernelHost::new();
    let events = load_board(&mut host, board_with_pin_wire());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    let gpio = gpio_of(&host, "GPIO0");
    let before = gpio.borrow().inputs();
    gpio.borrow_mut().set_external_inputs(before ^ (1 << 3));

    // ⚠️ **抬一次**。
    host.sync_interrupts_now();
    let plic = plic_of(&host, "PLIC");
    enable_source(&plic, 11);
    assert!(plic.borrow().pending(11), "第一次必须挂起");
    // ⚠️ **guest 收走**（claim + complete）⇒ ⇒ PLIC 的挂起位被清。
    let claimed = plic.borrow_mut().claim(0);
    assert_eq!(claimed, Some(11), "guest 必须 claim 得到那个源");
    plic.borrow_mut().complete(0, 11);
    assert!(!plic.borrow().pending(11), "complete 之后源不该再挂着");

    // ⚠️ **再跑几轮同步**（**没有新的边沿**）⇒ ⇒ **不许**再挂起 ⇒ ⇒ **不活锁**。
    for _ in 0..3 {
        host.sync_interrupts_now();
    }
    assert!(
        !plic.borrow().pending(11),
        "没有新的边沿 ⇒ ⇒ 源**不许**再挂起（⇒ ⇒ 一次性语义 ⇒ ⇒ **不活锁**）"
    );
}

/// **K-3** —— ⚠️ **必要对照** —— **UART 的电平语义逐字不变**（FIFO 空 ⇒ ⇒ 源
/// **撤销**）⇒ ⇒ 既有 D-158 那 19 条**一条不许红**（它们在别的文件里）。
#[test]
fn k3_the_uart_level_semantics_is_unchanged() {
    let mut host = KernelHost::new();
    let mut description = qemu_virt_description();
    // ⚠️ **默认连线**（第一个 UART 的 rx_ready ⇒ PLIC 源 10）⇒ ⇒ 缺省口径。
    description.wiring = None;
    let events = load_board(&mut host, description);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "装板不该报错：{events:?}"
    );

    let plic = plic_of(&host, "PLIC");
    enable_source(&plic, 10);
    // ⚠️ **打开 UART 的 RX 中断**（IER bit0）⇒ ⇒ 否则电平**永远不起来**。
    enable_uart_rx_interrupt(&mut host, UART0_BASE);
    // ⚠️ **FIFO 空** ⇒ ⇒ 源**不该**挂着。
    host.sync_interrupts_now();
    assert!(
        !plic.borrow().pending(10),
        "UART 的 rx_ready 是**电平**语义 ⇒ ⇒ FIFO 空 ⇒ ⇒ 源**撤销**（逐字不变）"
    );

    // ⚠️ **塞一个字节** ⇒ ⇒ 电平**起来** ⇒ ⇒ 源**挂起**。
    host.uart_receive(b'a');
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(10),
        "FIFO 有字节 ⇒ ⇒ 电平起来 ⇒ ⇒ 源挂起（电平语义逐字不变）"
    );
}

/// **K-4** —— 事件名**不在词表** ⇒ ⇒ 描述**被拒**（既有码 `E_PLT-16`）⇒ ⇒
/// **零新场景码**。
#[test]
fn k4_an_unknown_event_name_is_rejected() {
    let mut description = board_with_pin_wire();
    description.wiring.as_mut().unwrap()[0].from.event = "no_such_event".to_string();
    let err = description.validate().expect_err("词表外的事件名必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(
        err.scenario, E_PLT_16_WIRING_INVALID,
        "必须是**既有码** `E_PLT-16`（⇒ ⇒ 零新场景码）"
    );
}

/// **K-5** —— `pin_irq` 的源**不是 GPIO** ⇒ ⇒ 描述**被拒**（既有码）。
#[test]
fn k5_a_pin_event_from_a_non_gpio_is_rejected() {
    let mut description = board_with_pin_wire();
    description.wiring.as_mut().unwrap()[0].from.device = "UART0".to_string();
    let err = description.validate().expect_err("源不是 GPIO 必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(
        err.scenario, E_PLT_16_WIRING_INVALID,
        "必须是既有码 `E_PLT-16`"
    );
}

/// **K-6** —— 给了 `pin` 但事件**不是**引脚级 ⇒ ⇒ 描述**被拒**（既有码）。
#[test]
fn k6_a_pin_on_a_non_pin_event_is_rejected() {
    let mut description = qemu_virt_description();
    description.wiring = Some(vec![WiringSpec {
        from: WiringFrom {
            device: "UART0".to_string(),
            event: "rx_ready".to_string(),
            pin: Some(3),
        },
        to: WiringTo {
            device: "PLIC".to_string(),
            source: 10,
        },
    }]);
    let err = description
        .validate()
        .expect_err("非引脚级事件给了 pin 必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(
        err.scenario, E_PLT_16_WIRING_INVALID,
        "必须是既有码 `E_PLT-16`（⇒ ⇒ **不静默忽略** pin）"
    );
}

/// **K-7** —— `pin` **越界**（≥ 引脚数）⇒ ⇒ 描述**被拒**（既有码）。
#[test]
fn k7_an_out_of_range_pin_is_rejected() {
    let mut description = board_with_pin_wire();
    description.wiring.as_mut().unwrap()[0].from.pin = Some(200);
    let err = description.validate().expect_err("越界的引脚号必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(
        err.scenario, E_PLT_16_WIRING_INVALID,
        "必须是既有码 `E_PLT-16`（⇒ ⇒ 校验源 = 契约层 GPIO_PIN_COUNT）"
    );
}

/// **K-8** —— ⚠️ **必要对照** —— **老描述（`rx_ready`）逐字不变** ⇒ ⇒ 存量板档案
/// **零风险**。
#[test]
fn k8_an_old_description_is_unchanged_byte_for_byte() {
    let description = qemu_virt_description();
    // ⚠️ **老描述**（`schema_version = 1` · `wiring` 缺省 · 没有 `pin`）⇒ ⇒
    //   **仍然合法**（⇒ ⇒ 缺省优先）。
    assert!(
        description.validate().is_ok(),
        "老描述必须仍然合法（缺省优先 · 存量零风险）：{:?}",
        description.validate()
    );
    // ⚠️ **老描述装得上**（⇒ ⇒ 真的能装，不只是「合法」）。
    let mut host = KernelHost::new();
    let events = load_board(&mut host, description);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventPayload::Error { .. })),
        "老描述必须装得上：{events:?}"
    );
    // ⚠️ **默认连线逐字不变**（第一个 UART ⇒ PLIC 源 10）。
    let plic = plic_of(&host, "PLIC");
    enable_source(&plic, 10);
    enable_uart_rx_interrupt(&mut host, UART0_BASE);
    host.uart_receive(b'x');
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(10),
        "老描述的默认连线（UART0 ⇒ 源 10）必须照旧通电"
    );
}
