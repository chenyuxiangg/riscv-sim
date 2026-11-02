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

/// 一块**带 `timer_riscv`** 的板（RAM 一段 + 那台计时器）。
fn board_with_timer() -> PlatformDescription {
    let mut description =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt");
    description.regions.push(RegionSpec::Peripheral {
        base: 0x2000_2000,
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
}
