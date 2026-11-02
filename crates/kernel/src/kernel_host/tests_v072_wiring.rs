// v0.72 批 · STAGE-48 · D-177 —— **宿主不再把器件实例塌缩成「几个固定角色」**。
//
// 冻结规格：`docs/detailed-design/STAGE-48_V072_Device_Instance_Table.md`
// §4.2（**K 组**）。
//
// ## 这一套盯住什么
//
// - **K-1** 第二个 UART 的线**真的通电**（本批的**里程碑**）；
// - **K-2** 必要对照（源器件换成 GPIO ⇒ ⇒ 那根线**抬不了** ⇒ ⇒ K-1 必红）；
// - **K-3 / K-4** **存量逐字不变**（默认连线仍是源 10 · 覆盖仍是覆盖）；
// - **K-5** 端点不存在 ⇒ ⇒ **描述被拒**（契约层 V-18）⇒ ⇒ 那种线**到不了**
//   宿主 ⇒ ⇒ 报告里**不会**出现它（**边界登记**）；
// - **K-6 / K-7** **合法描述上可达的两条理由**（`source-type` / `overridden`）
//   **各归各的词**；
// - **K-8 ~ K-10** `declared` 的口径 · 报告**跟着图走** · 零板**不 panic**；
// - **K-11** ⚠️ **四条兜底理由**在**绕过描述校验**时**仍能判出来**（它们
//   **不是死词** ⇒ ⇒ 判别力**有**）；
// - **K-12** **恒等式** `declared == live + dropped.len()`。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。
// ⚠️ **每条断言都要能变红**（不许恒真断言）。

use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::platform::{
    DeviceSpec, PlatformDescription, RegionSpec, WiringFrom, WiringSpec, WiringTo,
    WIRING_EVENT_RX_READY,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m14_bus::{Bus, BusWidth};
use crate::m15_peripheral::uart::Uart16550;

use super::platform;
use super::KernelHost;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// `qemu-virt` 描述（**取值来源**：从描述查，不抄宿主常量）。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 描述里某个**器件**的基址（按器件 id 查）。
fn device_base(description: &PlatformDescription, device_id: &str) -> u32 {
    platform::device_base(description, device_id)
        .unwrap_or_else(|| panic!("描述里必须有器件 {device_id}"))
}

/// **第二个**外设的基址（**不与** qemu-virt 的任何器件重叠）。
const SECOND_BASE: u32 = 0x1000_1000;

/// 一块**有第二个 UART** 的板（`UART0` 之外的那台）。
///
/// ⚠️ **为什么要专门造**：内置三档描述都**只有一个 UART** ⇒ ⇒ 「第二个
/// UART 的线能不能通电」这件事**在存量板上根本看不出来**。
fn board_with_second_uart() -> PlatformDescription {
    let mut description = qemu_virt_description();
    description.regions.push(RegionSpec::Peripheral {
        base: SECOND_BASE,
        device: DeviceSpec::Uart16550 {
            id: "UART1".to_string(),
            irq_line: 1,
            baud_rate: 115200,
        },
    });
    description
}

/// 一块**第二个外设是 GPIO** 的板（**必要对照**用 —— 源器件**存在**但**不是
/// UART** ⇒ ⇒ V-18 / V-19 都判它合法 ⇒ ⇒ 宿主**抬不了**它）。
fn board_with_second_gpio() -> PlatformDescription {
    let mut description = qemu_virt_description();
    description.regions.push(RegionSpec::Peripheral {
        base: SECOND_BASE,
        device: DeviceSpec::Gpio {
            id: "GPIO1".to_string(),
        },
    });
    description
}

/// 一根连线（`from` 的 `rx_ready` → `to` 的 `source`）。
fn wire(from: &str, to: &str, source: u32) -> WiringSpec {
    WiringSpec {
        from: WiringFrom {
            device: from.to_string(),
            event: WIRING_EVENT_RX_READY.to_string(),
        },
        to: WiringTo {
            device: to.to_string(),
            source,
        },
    }
}

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
fn minimal_elf(vaddr: u32, code: &[u8]) -> Vec<u8> {
    const EHDR: usize = 52;
    const PHDR: usize = 32;
    let mut out = vec![0u8; EHDR + PHDR + code.len()];
    out[0] = 0x7f;
    out[1] = b'E';
    out[2] = b'L';
    out[3] = b'F';
    out[4] = 1; // EI_CLASS = ELFCLASS32
    out[5] = 1; // EI_DATA = ELFDATA2LSB
    out[6] = 1; // EI_VERSION
    out[16..18].copy_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    out[18..20].copy_from_slice(&243u16.to_le_bytes()); // e_machine = EM_RISCV
    out[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
    out[24..28].copy_from_slice(&vaddr.to_le_bytes()); // e_entry
    out[28..32].copy_from_slice(&(EHDR as u32).to_le_bytes()); // e_phoff
    out[40..42].copy_from_slice(&(EHDR as u16).to_le_bytes()); // e_ehsize
    out[42..44].copy_from_slice(&(PHDR as u16).to_le_bytes()); // e_phentsize
    out[44..46].copy_from_slice(&1u16.to_le_bytes()); // e_phnum
    let p = EHDR;
    out[p..p + 4].copy_from_slice(&1u32.to_le_bytes()); // PT_LOAD
    out[p + 4..p + 8].copy_from_slice(&((EHDR + PHDR) as u32).to_le_bytes()); // p_offset
    out[p + 8..p + 12].copy_from_slice(&vaddr.to_le_bytes()); // p_vaddr
    out[p + 12..p + 16].copy_from_slice(&vaddr.to_le_bytes()); // p_paddr
    out[p + 16..p + 20].copy_from_slice(&(code.len() as u32).to_le_bytes()); // p_filesz
    out[p + 20..p + 24].copy_from_slice(&(code.len() as u32).to_le_bytes()); // p_memsz
    out[p + 24..p + 28].copy_from_slice(&5u32.to_le_bytes()); // p_flags = R+X
    out[p + 28..p + 32].copy_from_slice(&4u32.to_le_bytes()); // p_align
    out[EHDR + PHDR..].copy_from_slice(code);
    out
}

/// 「往 UART0 写两个字节」的机器码（guest 真的在跑的证据）。
fn uart_write_two_bytes() -> Vec<u8> {
    let mut code = Vec::new();
    code.extend_from_slice(&0x1000_00B7u32.to_le_bytes()); // lui x1, 0x10000
    code.extend_from_slice(&0x0480_0113u32.to_le_bytes()); // addi x2, x0, 72
    code.extend_from_slice(&0x0020_8023u32.to_le_bytes()); // sb x2, 0(x1)
    code.extend_from_slice(&0x0690_0113u32.to_le_bytes()); // addi x2, x0, 105
    code.extend_from_slice(&0x0020_8023u32.to_le_bytes()); // sb x2, 0(x1)
    code
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

/// PLIC 的**优先级**寄存器地址。
fn plic_priority(base: u32, source: u32) -> u32 {
    base + 4 * source
}

/// PLIC 的 **pending 位图**地址。
fn plic_pending(base: u32, source: u32) -> u32 {
    base + 0x1000 + 4 * (source / 32)
}

/// PLIC 的 **enable** 寄存器地址。
fn plic_enable(base: u32, context: u32, source: u32) -> u32 {
    base + 0x2000 + 0x80 * context + 4 * (source / 32)
}

/// PLIC 的**门限**寄存器地址。
fn plic_threshold(base: u32, context: u32) -> u32 {
    base + 0x200000 + 0x1000 * context
}

/// PLIC 的 **claim** 寄存器地址（读它 = guest 领取当前该交付的源）。
fn plic_claim(base: u32, context: u32) -> u32 {
    base + 0x200000 + 0x1000 * context + 4
}

/// 某个 UART 的 **IER** 地址（**基址来自描述** + 偏移 `0x1`）。
fn uart_ier(base: u32) -> u32 {
    base + 1
}

/// 装配一个宿主：**按契约命令**把用户配的板装上去 ⇒ 返回事件。
fn host_with_board(description: &PlatformDescription) -> (KernelHost, Vec<EventPayload>) {
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
    (host, events)
}

/// 从事件里取**连线报告**。
fn report_of(events: &[EventPayload]) -> riscv_sim_contract::model::WiringReportDto {
    for e in events {
        if let EventPayload::MemoryMap { wiring_report, .. } = e {
            return wiring_report.clone();
        }
    }
    panic!("事件里必须有 MemoryMap（带连线报告）：{events:?}");
}

/// 装载一个镜像（guest 真的在跑）。
fn load_image(host: &mut KernelHost) {
    let elf = minimal_elf(0x8400_0000, &uart_write_two_bytes());
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
}

/// 把 guest 侧的 PLIC 配置**摆好**（某个源：优先级 1 / context 0 使能 / 门限 0）。
fn configure_plic_source(host: &mut KernelHost, base: u32, source: u32) {
    host.bus_mut()
        .write(plic_priority(base, source), BusWidth::Word, 1)
        .expect("写 PLIC 优先级");
    host.bus_mut()
        .write(
            plic_enable(base, 0, source),
            BusWidth::Word,
            1 << (source % 32),
        )
        .expect("写 PLIC 使能");
    host.bus_mut()
        .write(plic_threshold(base, 0), BusWidth::Word, 0)
        .expect("写 PLIC 门限");
}

/// 某个源**有没有**挂起。
fn plic_source_pending(host: &KernelHost, base: u32, source: u32) -> bool {
    let pending = host
        .bus()
        .read_only(plic_pending(base, source), BusWidth::Word)
        .expect("读 PLIC 的 pending 位图");
    pending & (1 << (source % 32)) != 0
}

// ---------------------------------------------------------------------------
// K-1 —— 第二个 UART 的线**真的通电**（本批的里程碑）
// ---------------------------------------------------------------------------

/// **K-1** —— 描述里写 `UART1.rx_ready → PLIC.source 11` ⇒ ⇒ guest 侧
/// **真的 claim 得到 11**。
///
/// ⚠️ **端到端**：走的是**契约命令**（`SetMemoryMap` 装板 ⇒ `LoadProject`
/// 装载镜像）⇒ ⇒ 板是真的换了、guest 是真的在跑。
///
/// ⚠️ **判别力**：把 `compile_wiring` 末尾那个过滤改回「只留第一个 UART →
/// 第一个 PLIC」⇒ ⇒ 这根线**不在表里** ⇒ ⇒ 源 11 **不**挂起 ⇒ ⇒ claim
/// **不是** 11 ⇒ ⇒ **必红**。
#[test]
fn k1_the_second_uarts_wire_really_carries_the_interrupt() {
    let mut description = board_with_second_uart();
    description.wiring = Some(vec![wire("UART1", "PLIC", 11)]);
    let (mut host, events) = host_with_board(&description);
    load_image(&mut host);

    // ⚠️ **报告**：这根线**抬得了** ⇒ ⇒ 它**不在** `dropped` 里 ⇒ ⇒
    // `declared == 1` ∧ `live == 1` ∧ `dropped` 空。
    let report = report_of(&events);
    assert_eq!(report.declared, 1, "用户写了 1 根线");
    assert_eq!(report.live, 1, "那根线**抬得了** ⇒ ⇒ 它算「在用」");
    assert!(
        report.dropped.is_empty(),
        "那根线不该被报成丢线：{:?}",
        report.dropped
    );

    let plic_base = device_base(&description, "PLIC");
    configure_plic_source(&mut host, plic_base, 11);

    // ⚠️ **打开 UART1 的 RX 中断**（IER bit0）—— **不是** UART0 的。
    host.bus_mut()
        .write(uart_ier(SECOND_BASE), BusWidth::Byte, 0x01)
        .expect("写 UART1 的 IER");

    // ⚠️ 宿主往 **UART1** 的 RX FIFO 塞一个字节 ⇒ ⇒ **UART1** 的
    // `rx_ready` 才可能为真 ⇒ ⇒ **UART0** 的**不**为真。
    assert!(
        host.uart_receive_on("UART1", b'x'),
        "UART1 必须在宿主手里（否则这条用例什么也证明不了）"
    );
    host.sync_interrupts_now();

    // ⚠️ **guest 侧读 claim 寄存器** ⇒ 它就是「guest 能观察到的后果」。
    let claimed = host
        .bus()
        .read_only(plic_claim(plic_base, 0), BusWidth::Word)
        .expect("读 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 11,
        "第二个 UART 的线**必须**真的通电（claim 必须是 11），实际是 {claimed}"
    );

    // ⚠️ **默认连线那根（源 10）不许被误挂** ⇒ ⇒ UART0 没有字节 ⇒ ⇒ 它的
    // `rx_ready` 为假 ⇒ ⇒ 源 10 **不**该挂起。
    assert!(
        !plic_source_pending(&host, plic_base, 10),
        "UART0 没有字节 ⇒ ⇒ 默认连线那根（源 10）**不许**挂起"
    );
}

/// **K-2** —— 必要对照：**源器件换成 GPIO** ⇒ ⇒ 那根线**抬不了** ⇒ ⇒ 源 11
/// **不**挂起 ⇒ ⇒ **K-1 不是「本来就通」**。
#[test]
fn k2_the_control_a_gpio_source_cannot_be_raised() {
    let mut description = board_with_second_gpio();
    description.wiring = Some(vec![wire("GPIO1", "PLIC", 11)]);
    // ⚠️ **描述本身合法**（V-18 只管源器件「在不在」，**不管**它「是什么」）
    // ⇒ ⇒ 板**装得上** ⇒ ⇒ 那种线**真的**能到宿主。
    assert!(
        description.validate().is_ok(),
        "源器件是 GPIO 的连线**必须**合法（V-18 只查存在性）"
    );
    let (mut host, events) = host_with_board(&description);
    load_image(&mut host);

    // ⚠️ **报告**：它**抬不了** ⇒ ⇒ `dropped` 里**有**它 ⇒ ⇒ 理由是
    // `source-type`。
    let report = report_of(&events);
    assert_eq!(report.declared, 1);
    assert_eq!(report.live, 0, "抬不了的线**不许**算「在用」");
    assert_eq!(report.dropped.len(), 1, "那根线必须被**报出来**");
    assert_eq!(
        report.dropped[0].reason,
        riscv_sim_contract::model::WireDropReason::SourceType,
        "理由必须是 source-type"
    );
    assert_eq!(report.dropped[0].from_device, "GPIO1");

    let plic_base = device_base(&description, "PLIC");
    configure_plic_source(&mut host, plic_base, 11);
    host.sync_interrupts_now();

    assert!(
        !plic_source_pending(&host, plic_base, 11),
        "GPIO 的 `rx_ready` **抬不起来** ⇒ ⇒ 源 11 **不许**挂起"
    );
}

/// **K-3** —— **存量逐字不变**：加了第二个 UART 之后，默认连线**仍是源 10**。
#[test]
fn k3_the_default_wiring_still_claims_source_10_even_with_a_second_uart() {
    let description = board_with_second_uart();
    let (mut host, events) = host_with_board(&description);
    load_image(&mut host);

    // ⚠️ **默认连线不计入 `declared`**（K-8 在这里顺带钉住）。
    let report = report_of(&events);
    assert_eq!(
        report.declared, 0,
        "用户**没写**任何线 ⇒ ⇒ `declared` 必须是 0（默认连线不算）"
    );
    assert_eq!(report.live, 0, "默认连线**不**是用户写的 ⇒ ⇒ 不算「在用」");
    assert!(report.dropped.is_empty());

    let plic_base = device_base(&description, "PLIC");
    configure_plic_source(&mut host, plic_base, 10);

    // ⚠️ **console 角色仍是「第一个 UART」** ⇒ ⇒ 往它塞字节。
    let uart0_base = device_base(&description, "UART0");
    host.bus_mut()
        .write(uart_ier(uart0_base), BusWidth::Byte, 0x01)
        .expect("写 UART0 的 IER");
    host.uart_receive(b'x');
    host.sync_interrupts_now();

    let claimed = host
        .bus()
        .read_only(plic_claim(plic_base, 0), BusWidth::Word)
        .expect("读 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 10,
        "默认连线必须**仍是**源 10（存量行为一字未变），实际是 {claimed}"
    );
}

/// **K-4 / K-7** —— 同键两根 ⇒ ⇒ **后者覆盖前者** ⇒ ⇒ 前者被报成
/// `overridden` ⇒ ⇒ `live` 只算**一根**。
#[test]
fn k7_a_shadowed_wire_is_reported_as_overridden() {
    let mut description = board_with_second_uart();
    description.wiring = Some(vec![wire("UART0", "PLIC", 3), wire("UART0", "PLIC", 4)]);
    let (_host, events) = host_with_board(&description);

    let report = report_of(&events);
    assert_eq!(report.declared, 2, "用户写了 2 根线");
    assert_eq!(report.live, 1, "同键 ⇒ ⇒ 只有一个键一根线 ⇒ ⇒ 只算一根");
    assert_eq!(report.dropped.len(), 1, "被顶掉的那根必须被**报出来**");
    assert_eq!(
        report.dropped[0].reason,
        riscv_sim_contract::model::WireDropReason::Overridden,
        "理由必须是 overridden"
    );
    assert_eq!(report.dropped[0].source, 3, "被顶掉的**是前一根**（源 3）");
}

/// **K-4** —— 覆盖**不追加**（承 v0.34 批 `s19_b5`）：表里那根线的源号**是
/// 后一根的**。
#[test]
fn k4_a_shadowed_wire_is_replaced_not_appended() {
    let mut description = board_with_second_uart();
    description.wiring = Some(vec![wire("UART0", "PLIC", 3), wire("UART0", "PLIC", 4)]);
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

    // ⚠️ **端到端**：guest 侧 claim 到的**必须是后一根**的源号（4）⇒ ⇒
    // 覆盖**不追加**。
    load_image(&mut host);
    let plic_base = device_base(&description, "PLIC");
    configure_plic_source(&mut host, plic_base, 4);
    let uart0_base = device_base(&description, "UART0");
    host.bus_mut()
        .write(uart_ier(uart0_base), BusWidth::Byte, 0x01)
        .expect("写 UART0 的 IER");
    host.uart_receive(b'x');
    host.sync_interrupts_now();

    let claimed = host
        .bus()
        .read_only(plic_claim(plic_base, 0), BusWidth::Word)
        .expect("读 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 4,
        "同键⇒⇒ 后一根**覆盖**前一根（**不**追加），实际是 {claimed}"
    );
    assert!(
        !plic_source_pending(&host, plic_base, 3),
        "被顶掉的那根**不许**还挂在那里"
    );
}

/// **K-5** —— 端点**不存在** ⇒ ⇒ **描述被拒**（契约层 V-18）⇒ ⇒ 那种线
/// **根本到不了**宿主 ⇒ ⇒ 报告里**不会**出现它。
///
/// ⚠️ **这是一条边界登记**：它**不是**「报告里有它」⇒ ⇒ 「端点错了」这件
/// 事**由描述校验器管**（内核报 `E_PLT-16`）⇒ ⇒ **不**是连线报告的活。
#[test]
fn k5_a_wire_to_a_missing_device_is_rejected_by_the_description_validator() {
    let mut description = board_with_second_uart();
    description.wiring = Some(vec![wire("NOPE", "PLIC", 11)]);
    assert!(
        description.validate().is_err(),
        "端点不存在的连线**必须**被描述校验器拒掉（V-18）"
    );

    // ⚠️ **端到端**：装板**必须报错**（`E_PLT-16`）⇒ ⇒ 它**到不了**宿主。
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    let errors: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            EventPayload::Error { code, .. } => Some(code.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !errors.is_empty(),
        "装板**必须**报错（描述校验器拒掉了它）：{events:?}"
    );
}

/// **K-9** —— 报告**跟着图走**：换一块板 ⇒ ⇒ 报告**重算** ⇒ ⇒ 不留上一份的
/// 残留。
#[test]
fn k9_the_report_is_recomputed_when_the_board_changes() {
    let mut bad = board_with_second_gpio();
    bad.wiring = Some(vec![wire("GPIO1", "PLIC", 11)]);
    let (mut host, first) = host_with_board(&bad);
    assert_eq!(
        report_of(&first).dropped.len(),
        1,
        "第一块板有一根抬不了的线"
    );

    // ⚠️ **换成一块干净的板** ⇒ ⇒ 报告**必须**跟着变。
    let clean = board_with_second_uart();
    let events = host.handle_message(&Message::command(
        2,
        CommandPayload::SetMemoryMap {
            description: clean.clone(),
            expected_revision: None,
        },
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "换板不该报错：{events:?}"
    );
    let report = report_of(&events);
    assert_eq!(report.declared, 0, "新板没写线");
    assert!(
        report.dropped.is_empty(),
        "上一块板的丢线**不许**留在报告里：{:?}",
        report.dropped
    );
}

/// **K-10** —— **零板时**：没有 PLIC 的板 ⇒ ⇒ **不 panic** ⇒ ⇒ 报告如实。
#[test]
fn k10_a_board_without_a_plic_does_not_panic_and_reports_honestly() {
    let mut description = board_with_second_uart();
    description.regions.retain(|region| match region {
        RegionSpec::Peripheral { device, .. } => !matches!(device, DeviceSpec::Plic { .. }),
        _ => true,
    });
    description.wiring = Some(vec![wire("UART1", "PLIC", 11)]);
    assert!(
        description.validate().is_err(),
        "没有 PLIC 的板**不许**带指向 PLIC 的连线（V-19）"
    );

    // ⚠️ **端到端**：**必须**报错（不是 panic）。
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::SetMemoryMap {
            description,
            expected_revision: None,
        },
    ));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "必须**报错**而不是 panic：{events:?}"
    );
}

/// **K-11** —— ⚠️ **四条兜底理由**（`source-unknown` / `target-unknown` /
/// `target-type` / `source-out-of-range`）在**绕过描述校验**时**仍能判出来**
/// ⇒ ⇒ 它们**不是死词** ⇒ ⇒ 判别力**有**。
///
/// ⚠️ **为什么必须绕过校验**：契约层的 V-18 / V-19 已经拒掉那三类描述 ⇒ ⇒
/// 它们**根本到不了**宿主 ⇒ ⇒ 只在**直接调** `compile_wiring`（喂一份**没过
/// 校验**的描述）时**才**可达。
#[test]
fn k11_the_four_fallback_reasons_are_still_reachable() {
    use riscv_sim_contract::model::WireDropReason;

    // ⚠️ **表从一份合法描述建出来**（表里要有 UART0 / GPIO0 / PLIC）。
    let valid = board_with_second_uart();
    let mut bus = Bus::new();
    let devices = platform::map_platform(&mut bus, &valid).expect("合法描述建图");
    let table = devices.handles;

    // -- ① 源器件不存在 ⇒ ⇒ `source-unknown` --------------------------------
    let mut d = valid.clone();
    d.wiring = Some(vec![wire("NOPE", "PLIC", 11)]);
    let compiled = platform::compile_wiring(&d, &table);
    assert_eq!(compiled.report.dropped.len(), 1, "必须报出一根");
    assert_eq!(
        compiled.report.dropped[0].reason,
        WireDropReason::SourceUnknown
    );

    // -- ② 目标器件不存在 ⇒ ⇒ `target-unknown` --------------------------------
    let mut d = valid.clone();
    d.wiring = Some(vec![wire("UART0", "NOPE", 11)]);
    let compiled = platform::compile_wiring(&d, &table);
    assert_eq!(compiled.report.dropped.len(), 1);
    assert_eq!(
        compiled.report.dropped[0].reason,
        WireDropReason::TargetUnknown
    );

    // -- ③ 目标不是 PLIC ⇒ ⇒ `target-type` -----------------------------------
    let mut d = valid.clone();
    d.wiring = Some(vec![wire("UART0", "GPIO0", 11)]);
    let compiled = platform::compile_wiring(&d, &table);
    assert_eq!(compiled.report.dropped.len(), 1);
    assert_eq!(
        compiled.report.dropped[0].reason,
        WireDropReason::TargetType
    );

    // -- ④ 源号越界 ⇒ ⇒ `source-out-of-range` ---------------------------------
    let mut d = valid.clone();
    d.wiring = Some(vec![wire("UART0", "PLIC", 9999)]);
    let compiled = platform::compile_wiring(&d, &table);
    assert_eq!(compiled.report.dropped.len(), 1);
    assert_eq!(
        compiled.report.dropped[0].reason,
        WireDropReason::SourceOutOfRange
    );
}

/// **K-12** —— **恒等式**：`declared == live + dropped.len()`（**真报告**必须
/// 满足）。
///
/// ⚠️ **判别力**：把 `live` 的计数改成「表里的**全部**线」（含默认连线）⇒ ⇒
/// 恒等式**破掉** ⇒ ⇒ **必红**。
#[test]
fn k12_every_real_report_keeps_the_identity() {
    let cases: Vec<PlatformDescription> = vec![
        // ① 空板（没写线 ⇒ 默认连线**不**计入 `declared`）。
        qemu_virt_description(),
        // ② 一根**抬得了**的线（第二个 UART）。
        {
            let mut d = board_with_second_uart();
            d.wiring = Some(vec![wire("UART1", "PLIC", 11)]);
            d
        },
        // ③ 一根**抬不了**的线（源是 GPIO）。
        {
            let mut d = board_with_second_gpio();
            d.wiring = Some(vec![wire("GPIO1", "PLIC", 11)]);
            d
        },
        // ④ 同键两根 ⇒ ⇒ 一根被顶掉。
        {
            let mut d = board_with_second_uart();
            d.wiring = Some(vec![wire("UART0", "PLIC", 3), wire("UART0", "PLIC", 4)]);
            d
        },
        // ⑤ 混着来：一根抬得了 + 一根抬不了。
        {
            let mut d = board_with_second_uart();
            d.regions.push(RegionSpec::Peripheral {
                base: 0x2000_2000,
                device: DeviceSpec::Gpio {
                    id: "GPIO1".to_string(),
                },
            });
            d.wiring = Some(vec![wire("UART1", "PLIC", 11), wire("GPIO1", "PLIC", 12)]);
            d
        },
    ];
    for description in cases {
        let (_host, events) = host_with_board(&description);
        let report = report_of(&events);
        assert_eq!(
            report.declared,
            report.live + report.dropped.len() as u32,
            "恒等式 `declared == live + dropped.len()` 必须成立（报告：{report:?}）"
        );
    }
}

/// **K-6** —— **合法描述上可达的理由集合** **恰好** 是 {`source-type`,
/// `overridden`} ⇒ ⇒ 「哪两条能发生」这件事**被钉住** ⇒ ⇒ 不会有人以为
/// 六条都能在正常链路上发生。
#[test]
fn k6_on_a_valid_description_only_two_reasons_are_reachable() {
    use riscv_sim_contract::model::WireDropReason;
    use std::collections::BTreeSet;

    let mut seen: BTreeSet<WireDropReason> = BTreeSet::new();

    // -- ① 源不是 UART ⇒ ⇒ `source-type` --------------------------------------
    let mut d = board_with_second_gpio();
    d.wiring = Some(vec![wire("GPIO1", "PLIC", 11)]);
    let (_host, events) = host_with_board(&d);
    for one in report_of(&events).dropped {
        seen.insert(one.reason);
    }

    // -- ② 同键两根 ⇒ ⇒ `overridden` ------------------------------------------
    let mut d = board_with_second_uart();
    d.wiring = Some(vec![wire("UART0", "PLIC", 3), wire("UART0", "PLIC", 4)]);
    let (_host, events) = host_with_board(&d);
    for one in report_of(&events).dropped {
        seen.insert(one.reason);
    }

    let expected: BTreeSet<WireDropReason> =
        BTreeSet::from([WireDropReason::SourceType, WireDropReason::Overridden]);
    assert_eq!(
        seen, expected,
        "合法描述上可达的理由**恰好**是这两条（其余四条是**兜底**）"
    );
}

/// **K-13** —— ⚠️ **宿主手里确实有第二个 UART 的句柄** ⇒ ⇒ 「实例表」这件
/// 事**真的**发生了（不是「表里有、句柄没有」）。
#[test]
fn k13_the_host_really_holds_a_handle_for_the_second_uart() {
    // ⚠️ 这条**不**经过契约命令（它盯的是**宿主内部**的事实）。
    let description = board_with_second_uart();
    let mut bus = Bus::new();
    let devices = platform::map_platform(&mut bus, &description).expect("建图");
    let handle = devices.handles.uart("UART1");
    assert!(handle.is_some(), "宿主**必须**持有第二个 UART 的句柄");
    // ⚠️ **而且那真的是一台 UART**（能塞字节、能报状态）。
    let uart = handle.expect("句柄");
    uart.borrow_mut().receive(b'x');
    assert!(
        uart.borrow().line_status() & 0x01 != 0,
        "第二个 UART 的 RX FIFO 必须**真的**能收到字节"
    );
    // ⚠️ **类型别搞混**：GPIO **没有** UART 句柄。
    let mut with_gpio = board_with_second_gpio();
    with_gpio.regions.push(RegionSpec::Peripheral {
        base: 0x2000_2000,
        device: DeviceSpec::Uart16550 {
            id: "UART2".to_string(),
            irq_line: 1,
            baud_rate: 115200,
        },
    });
    let mut bus2 = Bus::new();
    let devices2 = platform::map_platform(&mut bus2, &with_gpio).expect("建图");
    assert!(
        devices2.handles.uart("GPIO1").is_none(),
        "GPIO **不许**被当成 UART"
    );
    assert!(
        devices2.handles.uart("UART2").is_some(),
        "两个 UART **都要**有句柄"
    );
    // ⚠️ **编译器不许我把未用的类型悄悄带进来** ⇒ ⇒ 明确用一下。
    let _ = Uart16550::new("UART3".to_string(), 0, 115200, 1);
}

/// **K-14** —— **第二个 PLIC 的源也真的抬得到架构层**（裁决 F）。
///
/// ⚠️ **为什么必须有这一条**：裁决 F 把「抬 M-16 外部线」的那一个从「第一个
///   PLIC」升级成「**所有** PLIC」⇒ ⇒ 只用**一块** PLIC 的板**看不出**这个
///   差别 ⇒ ⇒ 变异 T-3（把第 3 步改回「只问第一个 PLIC」）**打不出红** ⇒ ⇒
///   零判别力 ⇒ ⇒ 这一条**就是** T-3 的判别力来源。
#[test]
fn k14_the_second_plics_source_really_reaches_the_architecture() {
    // ⚠️ **两块 PLIC**：PLIC0（第一个）与 PLIC1（**第二个**）。
    let mut description = qemu_virt_description();
    description.regions.push(RegionSpec::Peripheral {
        base: 0x2000_2000,
        device: DeviceSpec::Plic {
            id: "PLIC1".to_string(),
            sources: 64,
            contexts: 16,
        },
    });
    description.wiring = Some(vec![wire("UART0", "PLIC1", 11)]);
    assert!(
        description.validate().is_ok(),
        "指向第二个 PLIC 的连线**必须**合法（V-19 只管「是不是 PLIC」与源号范围）"
    );

    let (mut host, events) = host_with_board(&description);
    load_image(&mut host);

    // ⚠️ **报告**：这根线**抬得了** ⇒ ⇒ 它**不在** `dropped` 里。
    let report = report_of(&events);
    assert_eq!(report.declared, 1);
    assert_eq!(report.live, 1, "第二个 PLIC 的源**抬得了**");
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);

    // ⚠️ guest 侧：把**第二个 PLIC** 的源 11 配好。
    let plic1_base = device_base(&description, "PLIC1");
    configure_plic_source(&mut host, plic1_base, 11);

    // ⚠️ **console 角色**（第一个 UART）打开 RX 中断。
    let uart0_base = device_base(&description, "UART0");
    host.bus_mut()
        .write(uart_ier(uart0_base), BusWidth::Byte, 0x01)
        .expect("写 UART0 的 IER");
    host.uart_receive(b'x');
    host.sync_interrupts_now();

    // ⚠️ **guest 读第二个 PLIC 的 claim 寄存器** ⇒ 它是「guest 能观察到的后果」。
    let claimed = host
        .bus()
        .read_only(plic_claim(plic1_base, 0), BusWidth::Word)
        .expect("读第二个 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 11,
        "第二个 PLIC 的源**必须**真的抬起来（claim 必须是 11），实际是 {claimed}"
    );

    // ⚠️⚠️ **这一条才是裁决 F 的判据** —— **架构层的外部中断线**。
    //
    // ⚠️ **为什么上面那条 claim 断不了裁决 F**：guest 读 PLIC 的 claim 寄存器
    //   走的是**那个 PLIC 自己**的 `best_source` ⇒ ⇒ **根本不经**宿主
    //   `sync_interrupts` 的第 3 步 ⇒ ⇒ 把第 3 步改回「只问第一个 PLIC」
    //   ⇒ ⇒ **那条断言照样绿** ⇒ ⇒ **零判别力**（本批**实测踩到**：T-3 实测
    //   红集是**零红**）。
    //
    // ⚠️ **第 3 步真正做的事**是：任一 PLIC 有可交付源 ⇒ ⇒ 抬 **M-16 的
    //   外部中断线** ⇒ ⇒ 那条线**挂起** ⇒ ⇒ guest 才会陷入 ⇒ ⇒ 所以
    //   **判据必须盯那条线**（`intc_pending_lines`）。
    let pending_lines = host.intc_pending_lines();
    assert!(
        pending_lines.contains(&riscv_sim_contract::csr::IRQ_UART_RX_READY),
        "第二个 PLIC 的源**必须**抬到**架构层的外部中断线**（IRQ_UART_RX_READY）\
        ⇒ ⇒ 否则 guest 永远收不到这个中断（挂起中的线：{pending_lines:?}）"
    );
}

/// **K-8** —— **默认连线不计入 `declared`**（裁决 J）。
///
/// ⚠️ **为什么单独一条**：默认连线（第一个 UART → 第一个 PLIC · 源 10）是
///   **宿主加的**、**不是用户写的**⇒ ⇒ 「你写了 N 根线」这句话**不许**把它
///   算进去 ⇒ ⇒ 否则一块**没写任何线**的板会显示「写了 1 根」⇒ ⇒ 用户会
///   以为自己写错了。
///
/// ⚠️ **判别力**：把 `declared` 改成「数**表里**的线」⇒ ⇒ 这里**必红**。
#[test]
fn k8_the_default_wiring_is_not_counted_as_written_by_the_user() {
    let description = board_with_second_uart();
    assert!(
        description.wiring.is_none(),
        "这块板**不许**带连线段（⇒ ⇒ 用户确实**没写**任何线）"
    );
    let (_host, events) = host_with_board(&description);

    let report = report_of(&events);
    assert_eq!(
        report.declared, 0,
        "用户**没写**任何线 ⇒ ⇒ `declared` 必须是 0（默认连线**不算**）"
    );
    assert_eq!(report.live, 0, "默认连线**不是**用户写的 ⇒ ⇒ 不算「在用」");
    assert!(
        report.dropped.is_empty(),
        "默认连线**抬得了** ⇒ ⇒ 不该被报成丢线：{:?}",
        report.dropped
    );
}
