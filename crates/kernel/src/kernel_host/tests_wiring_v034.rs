//! v0.34 · D-125 —— **器件连线数据化**的 B 组用例（冻结规格 STAGE-19 §5）。
//!
//! ## 这些用例在盯什么
//!
//! v0.34 这条教训是「**同一条约束被写在了不止一个地方 ⇒ 就会漂**」：
//! 「UART 的 RX 中断接到 PLIC 的哪个源」这条约束**曾经**写在**三个**地方
//! （描述里的 `irq_line` / 前端编辑器 / 宿主里写死的 `10`）⇒ ⇒ 三个数各
//! 说各话，而描述里那个 1 **从来没生效过**。
//!
//! ⇒ ⇒ 本文件盯的是**后果**（不是字段、不是结构体）：
//!
//! * **B-1**：描述里写 `rx_ready → PLIC 的源 3` ⇒ guest 侧 claim 到的**是
//!   3**（**不是** 10）⇒ ⇒ **换一块板改连线，宿主零改动**；
//! * **B-2**：没有 `wiring` ⇒ guest 侧 claim 到的**仍是 10** ⇒ ⇒ **存量
//!   行为一字未变**；
//! * **B-3**：描述里没有 UART（或没有 PLIC）⇒ **不 panic**、**不**抬线；
//! * **B-4**：宿主源码里那个写死的 PLIC source 常量**已消失** ⇒ ⇒ 只剩
//!   **默认连线那一处**；
//! * **B-5**：描述里的 `wiring` **覆盖**默认连线的同键项 ⇒ ⇒ 生效的是
//!   描述里那个、**且不**出现两条连线。
//!
//! ## 断言值一律写字面量
//!
//! **B-1 / B-2** 断言的那个源号（3 / 10）是**冻结规格 §5 写下的期望值**
//! ⇒ ⇒ 写字面量（不是从实现反推）⇒ ⇒ 改了默认连线（变异 M2）这条立刻红。
//!
//! ⚠️ **与宿主里「源号只写一处」不矛盾**：那条纪律说的是**实现**里那个数
//! 只有**一处**（默认连线那处）⇒ ⇒ **测试里的期望值是规格的期望值**，不是
//! 实现的第二份副本。

use std::path::{Path, PathBuf};

use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::platform::{
    DeviceSpec, PlatformDescription, RegionSpec, WiringFrom, WiringSpec, WiringTo,
    WIRING_EVENT_RX_READY,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m14_bus::BusWidth;

use super::platform;
use super::KernelHost;

/// `qemu-virt` 描述（**取值来源**：从描述查，不抄宿主常量）。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 描述里某个**器件**的基址（按器件 id 查）。
fn device_base(description: &PlatformDescription, device_id: &str) -> u32 {
    platform::device_base(description, device_id)
        .unwrap_or_else(|| panic!("描述里必须有器件 {device_id}"))
}

/// 描述里**第一个 UART** 的器件 id。
fn first_uart_id(description: &PlatformDescription) -> String {
    description
        .regions
        .iter()
        .find_map(|region| match region {
            RegionSpec::Peripheral {
                device: DeviceSpec::Uart16550 { id, .. },
                ..
            } => Some(id.clone()),
            _ => None,
        })
        .expect("qemu-virt 描述里必须有 UART")
}

/// 一块**带自定义连线**的板：`UART0` 的 `rx_ready` → PLIC 的**源 `source`**。
///
/// ⚠️ **走的是描述的 `wiring` 段**（契约层 v2 的形状）⇒ ⇒ 宿主源码里
/// **没有**这个源号 ⇒ ⇒ 换一块板改连线**不用改宿主**。
fn board_with_wiring(source: u32) -> PlatformDescription {
    let mut description = qemu_virt_description();
    description.wiring = Some(vec![WiringSpec {
        from: WiringFrom {
            device: first_uart_id(&description),
            event: WIRING_EVENT_RX_READY.to_string(),
        },
        to: WiringTo {
            device: "PLIC".to_string(),
            source,
        },
    }]);
    description
}

/// 一块**没有 UART**（或**没有 PLIC**）的板。
///
/// ⚠️ **为什么必须造**：缺了一个端点 ⇒ 默认连线**不生效**（裁决 E）⇒ ⇒
/// 宿主**不许** panic、**不许**抬线。
fn board_without(kind: MissingDevice) -> PlatformDescription {
    let mut description = qemu_virt_description();
    description.regions.retain(|region| match region {
        RegionSpec::Peripheral { device, .. } => !matches!(
            (kind, device),
            (MissingDevice::Uart, DeviceSpec::Uart16550 { .. })
                | (MissingDevice::Plic, DeviceSpec::Plic { .. })
        ),
        _ => true,
    });
    description
}

/// 板上缺哪个器件。
#[derive(Debug, Clone, Copy)]
enum MissingDevice {
    Uart,
    Plic,
}

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
///
/// ⚠️ **为什么要自己造**：真实 RTOS 的 ELF 在**仓外**（不进仓库），
/// 而「宿主接线对不对」这件事必须有一条**在仓内**的用例钉住 ⇒ 用一个
/// 手造的、只有几条指令的 ELF 来驱动它。
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
        id: "proj-wiring-v034".to_string(),
        name: "wiring-v034".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "image.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: Default::default(),
    }
}

/// 宿主源码目录（**运行期**扫 ⇒ ⇒ 新增的文件也在扫描范围内）。
fn host_source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("kernel_host")
}

/// 宿主源码的**全部**文件（`name → 源码`）。
fn host_sources() -> Vec<(String, String)> {
    let dir = host_source_dir();
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读宿主源码目录 {} 失败：{e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("目录项");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读 {} 失败：{e}", path.display()));
        out.push((name, source));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// PLIC 的**优先级**寄存器地址（`0x0 + 4×source`）。
fn plic_priority(base: u32, source: u32) -> u32 {
    base + 4 * source
}

/// PLIC 的 **pending 位图**地址（`0x1000 + 4×(source/32)`）。
fn plic_pending(base: u32, source: u32) -> u32 {
    base + 0x1000 + 4 * (source / 32)
}

/// PLIC 的 **enable** 寄存器地址（`0x2000 + 0x80×context + 4×(source/32)`）。
fn plic_enable(base: u32, context: u32, source: u32) -> u32 {
    base + 0x2000 + 0x80 * context + 4 * (source / 32)
}

/// PLIC 的**门限**寄存器地址（`0x200000 + 0x1000×context`）。
fn plic_threshold(base: u32, context: u32) -> u32 {
    base + 0x200000 + 0x1000 * context
}

/// PLIC 的 **claim** 寄存器地址（`0x200000 + 0x1000×context + 4`）。
///
/// ⚠️ **读它 = guest 领取当前该交付的源**（QEMU 语义）⇒ ⇒ 它是「guest 侧
/// 能观察到的**后果**」⇒ ⇒ B-1 / B-2 断言的就是它。
fn plic_claim(base: u32, context: u32) -> u32 {
    base + 0x200000 + 0x1000 * context + 4
}

/// UART0 的 **IER**（中断使能）地址 —— **基址来自描述** + 偏移 `0x1`。
fn uart0_ier(description: &PlatformDescription) -> u32 {
    device_base(description, &first_uart_id(description)) + 1
}

/// 装配一个宿主：**按契约命令**把用户配的板装上去，再装载一个镜像。
///
/// ⚠️ **为什么走 `SetMemoryMap`**：那是**用户配板**的真实入口 ⇒ ⇒ 「换一块
/// 板改连线」这条路**真的**被走到（不是测试偷偷改了宿主内部状态）。
fn host_with_board(description: &PlatformDescription) -> KernelHost {
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
    assert_eq!(
        host.platform_id(),
        description.id,
        "装板之后生效的板就是用户配的那一块"
    );
    host
}

/// 装载一个镜像（guest 真的在跑）⇒ 返回事件。
fn load_image(host: &mut KernelHost) -> Vec<EventPayload> {
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
    events
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

// ---------------------------------------------------------------------------
// B-1 —— 描述里的自定义连线（**端到端**）
// ---------------------------------------------------------------------------

/// **B-1** —— 描述里把 `UART0` 的 `rx_ready` 接到 PLIC 的**源 3** ⇒ ⇒
/// guest 侧 claim 到的**是 3**（**不是** 10）⇒ ⇒ **换一块板改连线，宿主
/// 零改动**。
///
/// ⚠️ **端到端**：走的是**契约命令**（`SetMemoryMap` 装板 ⇒ `LoadProject`
/// 装载镜像）⇒ ⇒ 板是真的换了、guest 是真的在跑。
#[test]
fn s19_b1_a_custom_wiring_makes_the_guest_claim_source_3() {
    let description = board_with_wiring(3);
    let mut host = host_with_board(&description);
    load_image(&mut host);

    // guest 真的在跑（console 里能看到它写的两个字节）。
    host.handle_message(&Message::command(3, CommandPayload::StepN { n: 5 }));
    assert_eq!(
        host.drain_console(),
        b"Hi".to_vec(),
        "guest 必须真的跑起来（否则下面的 claim 什么也证明不了）"
    );

    let base = device_base(&description, "PLIC");

    // guest 侧：把 PLIC 的**源 3** 配好（优先级 1 / context 0 使能 / 门限 0）。
    configure_plic_source(&mut host, base, 3);

    // UART 的 IER bit0 = 允许 RX 中断。
    host.bus_mut()
        .write(uart0_ier(&description), BusWidth::Byte, 0x01)
        .expect("写 UART 的 IER");

    // 宿主往 UART 的 RX FIFO 塞一个字节 ⇒ UART 该抬线了。
    host.uart_receive(b'x');
    host.sync_interrupts_now();

    // ⚠️ **guest 侧读 claim 寄存器** ⇒ 它就是「guest 能观察到的后果」。
    let claimed = host
        .bus()
        .read_only(plic_claim(base, 0), BusWidth::Word)
        .expect("读 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 3,
        "描述里的连线指向源 3 ⇒ guest 必须 claim 到 3（**不是** 10），实际是 {claimed}"
    );

    // ⚠️ **「不是 10」也要钉住**：源 10 **不许**被挂起（否则「两条线都在」
    // 这种漂移会被 claim 的「取优先级最高那个」规则**藏起来**）。
    let pending = host
        .bus()
        .read_only(plic_pending(base, 10), BusWidth::Word)
        .expect("读 PLIC 的 pending 位图");
    assert_eq!(
        pending & (1 << 10),
        0,
        "宿主**不许**再去抬那个写死的源 10（pending 位图里它必须是 0）"
    );
}

// ---------------------------------------------------------------------------
// B-2 —— 默认连线（存量行为一字未变）
// ---------------------------------------------------------------------------

/// **B-2** —— 描述里**没有** `wiring` ⇒ guest 侧 claim 到的**仍是源 10**
/// ⇒ ⇒ **存量行为一字未变**。
#[test]
fn s19_b2_the_default_wiring_still_claims_source_10() {
    let description = qemu_virt_description();
    assert!(
        description.wiring.is_none(),
        "内置描述**不许**带连线段（缺省 = 走默认连线）"
    );
    let mut host = host_with_board(&description);
    load_image(&mut host);

    let base = device_base(&description, "PLIC");

    // guest 侧：把 PLIC 的**源 10** 配好。
    configure_plic_source(&mut host, base, 10);

    // UART 的 IER bit0 = 允许 RX 中断。
    host.bus_mut()
        .write(uart0_ier(&description), BusWidth::Byte, 0x01)
        .expect("写 UART 的 IER");

    host.uart_receive(b'x');
    host.sync_interrupts_now();

    let claimed = host
        .bus()
        .read_only(plic_claim(base, 0), BusWidth::Word)
        .expect("读 PLIC 的 claim 寄存器");
    assert_eq!(
        claimed, 10,
        "默认连线必须仍然是源 10（存量行为一字未变），实际是 {claimed}"
    );
}

// ---------------------------------------------------------------------------
// B-3 —— 缺一端 ⇒ 不 panic、不抬线
// ---------------------------------------------------------------------------

/// **B-3** —— 描述里**没有** PLIC ⇒ **不 panic**、**不**抬线。
#[test]
fn s19_b3_a_board_without_a_plic_raises_nothing_and_does_not_panic() {
    let description = board_without(MissingDevice::Plic);
    assert!(
        description.validate().is_ok(),
        "缺 PLIC 的描述本身**合法**（PLIC 不是必需的）"
    );
    let mut host = KernelHost::with_description(description);
    assert!(
        host.wiring_table().is_empty(),
        "缺一端 ⇒ **没有连线**（默认连线不生效），实际 {:?}",
        host.wiring_table()
    );

    // ⚠️ **不许 panic**：喂字节 + 同步中断都必须平安无事。
    host.uart_receive(b'x');
    host.sync_interrupts_now();
    assert!(
        host.intc_pending_lines().is_empty(),
        "没有 PLIC ⇒ **不许**抬线，实际挂起 {:?}",
        host.intc_pending_lines()
    );
}

/// **B-3** —— 描述里**没有** UART ⇒ **不 panic**、**不**抬线。
#[test]
fn s19_b3_a_board_without_a_uart_raises_nothing_and_does_not_panic() {
    let description = board_without(MissingDevice::Uart);
    assert!(
        description.validate().is_ok(),
        "缺 UART 的描述本身**合法**（UART 不是必需的）"
    );
    let mut host = KernelHost::with_description(description);
    assert!(
        host.wiring_table().is_empty(),
        "缺一端 ⇒ **没有连线**（默认连线不生效），实际 {:?}",
        host.wiring_table()
    );

    host.uart_receive(b'x');
    host.sync_interrupts_now();
    assert!(
        host.intc_pending_lines().is_empty(),
        "没有 UART ⇒ **不许**抬线，实际挂起 {:?}",
        host.intc_pending_lines()
    );
}

// ---------------------------------------------------------------------------
// B-4 —— 写死的那个常量**已消失**
// ---------------------------------------------------------------------------

/// 那个**已删**的写死常量名 —— ⚠️ **拼出来**（不写成一个连续的字面量）
/// ⇒ ⇒ 本文件自己的源码里**不含**那个串 ⇒ ⇒ 扫全目录时**不会自匹配**。
fn old_const_name() -> String {
    "PLIC_SOURCE".to_string() + "_UART"
}

/// **B-4** —— 宿主源码里那个写死的 PLIC source 常量**已消失** ⇒ ⇒ 只剩
/// **默认连线那一处**。
///
/// ⚠️ **三条一起钉**（缺一条都可能被绕过）：
///
/// 1. **那个常量名整个不在**（扫**全部**宿主源码，含测试文件）；
/// 2. **宿主抬线时不写字面量源号**（没有任何 `.request(<数字>)`）；
/// 3. **默认连线的那个源号在宿主实现里只出现一次**。
#[test]
fn s19_b4_the_hard_coded_plic_source_constant_is_gone() {
    let sources = host_sources();
    assert!(
        !sources.is_empty(),
        "宿主源码目录必须扫得到东西（否则这条用例是**空跑**）"
    );

    // ---- ① 那个常量名整个不在 ----
    let old_const = old_const_name();
    for (name, source) in &sources {
        assert!(
            !source.contains(old_const.as_str()),
            "宿主源码 {name} 里还有那个写死的 PLIC source 常量（{old_const}）"
        );
    }

    // ---- ② 宿主抬线时不写字面量源号 ----
    //
    // ⚠️ **只看宿主实现**（`tests_*` 是测试，不是实现）⇒ ⇒ 判据是「
    // `.request(` 的参数**不是**数字字面量」。
    //
    // ⚠️ **注释行不算**：散文里提一句旧口径**不是**写死的连线 ⇒ ⇒ 只扫
    // **代码行**（整行是注释的跳过）。
    for (name, source) in &sources {
        if name.starts_with("tests_") {
            continue;
        }
        for line in source.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for (index, _) in line.match_indices(".request(") {
                let argument = line[index + ".request(".len()..].trim_start();
                assert!(
                    !argument.starts_with(|c: char| c.is_ascii_digit()),
                    "宿主源码 {name} 里有一处 `.request(<数字字面量>)` ⇒ 那是**写死的** \
                     PLIC 源号（应当来自连线表）"
                );
            }
        }
    }

    // ---- ③ 默认连线的那个源号在宿主实现里**只出现一次** ----
    //
    // ⚠️ **判据**：那个**数**（不是那句源码文本）在宿主实现里**独立出现**
    // **恰好一次** ⇒ ⇒ 宿主里**没有第二处**写着这个源号。
    //
    // ⚠️ **为什么数那个数、而不是那句文本**：源码文本（`source: 10`）会
    // 随默认连线改值而变 ⇒ ⇒ 拿它当判据 ⇒ 改一次默认源号就会**假红** ⇒ ⇒
    // 这里数的是**默认连线当前那个数**（从默认连线那处取）⇒ ⇒ 它跟着改。
    let default_source = platform::default_wiring(&qemu_virt_description())
        .into_iter()
        .next()
        .expect("qemu-virt 描述必须有默认连线")
        .source;
    let mut hits = 0usize;
    for (name, source) in &sources {
        if name.starts_with("tests_") {
            continue;
        }
        let count = decimal_literals(source)
            .into_iter()
            .filter(|value| *value == u64::from(default_source))
            .count();
        assert!(
            count <= 1,
            "宿主源码 {name} 里那个默认源号（{default_source}）出现了 {count} 次 \
             （每个文件最多一处 ⇒ 那就是第二处落点）"
        );
        hits += count;
    }
    assert_eq!(
        hits, 1,
        "默认连线的源号（{default_source}）在宿主实现里必须**只出现一处**，实际出现 {hits} 次"
    );
}

/// 一份源码里**独立出现**的十进制字面量（十六进制与带类型后缀的除外）。
///
/// ⚠️ **为什么要自己扫**：判据是「那个数只出现一次」⇒ ⇒ 注释里的字样
/// **不算**、十六进制（`0x10`）**不算**、格式说明（`#010x`）**不算**、
/// 标识符的一部分（`x10`）**不算**。
fn decimal_literals(source: &str) -> Vec<u64> {
    source
        .lines()
        // ⚠️ 行注释里的字样不算（散文提到一个数 ≠ 写死了那个数）。
        .map(|line| line.split("//").next().unwrap_or(""))
        .flat_map(|code| {
            code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .filter_map(decimal_value)
        })
        .collect()
}

/// 一个 token 是不是一个**十进制字面量**？是的话给出它的值。
///
/// ⚠️ **只认两种形态**：纯十进制（`10` / `1_000`）与带**类型后缀**的十进制
/// （`10u32`）⇒ ⇒ 十六进制（`0x10`）、格式说明（`#010x`）、标识符（`x10`）
/// **一律不算**。
fn decimal_value(token: &str) -> Option<u64> {
    let digits: String = token.chars().filter(|c| *c != '_').collect();
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        return digits.parse::<u64>().ok();
    }
    for suffix in [
        "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    ] {
        if let Some(body) = token.strip_suffix(suffix) {
            let body: String = body.chars().filter(|c| *c != '_').collect();
            if !body.is_empty() && body.chars().all(|c| c.is_ascii_digit()) {
                return body.parse::<u64>().ok();
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// B-5 —— 覆盖（不是追加）
// ---------------------------------------------------------------------------

/// **B-5** —— 描述里的 `wiring` **覆盖**默认连线的**同键项** ⇒ ⇒ 生效的
/// 是描述里那个、**且不**出现两条连线。
#[test]
fn s19_b5_the_description_wiring_overrides_the_default_one_instead_of_adding_one() {
    let description = board_with_wiring(3);
    let host = host_with_board(&description);

    let uart_id = first_uart_id(&description);
    let same_key: Vec<_> = host
        .wiring_table()
        .iter()
        .filter(|w| w.from_device == uart_id && w.event == WIRING_EVENT_RX_READY)
        .collect();
    assert_eq!(
        same_key.len(),
        1,
        "同键（{uart_id} · rx_ready）**只能有一根线**，实际 {:?}",
        same_key
    );
    assert_eq!(
        same_key[0].source, 3,
        "生效的必须是**描述里那个**（默认连线被覆盖），实际是 {}",
        same_key[0].source
    );
    assert_eq!(same_key[0].to_device, "PLIC", "目标器件必须还是那个 PLIC");
}
