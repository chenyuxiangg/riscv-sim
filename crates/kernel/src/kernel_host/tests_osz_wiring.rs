//! 宿主接线的**接线本身**的用例（v0.23 批）。
//!
//! ## 这些用例在盯什么
//!
//! v0.22 的教训是「**加了守卫 ≠ 守卫接上了**」——要查它的**比较点**，
//! 不能只查它的变量。同样的教训套在接线上：
//!
//! * 「内存图里有 UART」≠「guest 写进去的字节能被取出来」；
//! * 「CLINT 里有 `mtimecmp`」≠「它真的喂给了 M-16」；
//! * 「PLIC 里有 pending 位」≠「M-16 的外部线真的被拉起来了」。
//!
//! ⇒ 本文件**不**断言「结构体里有个字段」，而是断言**可观察的后果**：
//! 字节流真的出来了、定时器比较值真的到位了、中断线真的挂起了。
//!
//! ## 断言值一律写字面量
//!
//! 不从被测实现反推（例如不写 `assert_eq!(x, clint.mtimecmp_ps())`）——
//! 那样改实现时用例会跟着变，变成恒绿。

use riscv_sim_contract::model::{ImageKind, ImageRef, ImageSource, ProductTree, Project};
use riscv_sim_contract::protocol::{CommandPayload, Message};

use crate::m14_bus::BusWidth;

use super::{
    KernelHost, MemoryProfile, PLIC_SOURCE_UART, QEMU_VIRT_CLINT_BASE, QEMU_VIRT_PLIC_BASE,
    QEMU_VIRT_RAM_BASE, QEMU_VIRT_RAM_SIZE, QEMU_VIRT_UART0_BASE,
};

/// UART0 的 THR（发送保持寄存器）地址。
const UART0_THR: u32 = QEMU_VIRT_UART0_BASE;
/// UART0 的 IER（中断使能）地址。
const UART0_IER: u32 = QEMU_VIRT_UART0_BASE + 1;
/// CLINT 的 `mtimecmp` 低字地址。
const CLINT_MTIMECMP_LO: u32 = QEMU_VIRT_CLINT_BASE + 0x4000;
/// CLINT 的 `mtimecmp` 高字地址。
const CLINT_MTIMECMP_HI: u32 = QEMU_VIRT_CLINT_BASE + 0x4004;
/// PLIC 的 source 10 的优先级寄存器地址。
const PLIC_PRIORITY_10: u32 = QEMU_VIRT_PLIC_BASE + 4 * PLIC_SOURCE_UART;
/// PLIC 的 context 0 的 enable 寄存器地址（source 10 落在第 0 个字）。
const PLIC_ENABLE_CTX0: u32 = QEMU_VIRT_PLIC_BASE + 0x2000;
/// PLIC 的 context 0 的门限寄存器地址。
const PLIC_THRESHOLD_CTX0: u32 = QEMU_VIRT_PLIC_BASE + 0x200000;

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
///
/// ⚠️ **为什么要自己造**：真实 RTOS 的 ELF 在**仓外**（不进仓库），
/// 而「宿主接线对不对」这件事必须有一条**在仓内**的用例钉住 ⇒ 用一个
/// 手造的、只有几条指令的 ELF 来驱动它。
fn minimal_elf(vaddr: u32, code: &[u8]) -> Vec<u8> {
    const EHDR: usize = 52;
    const PHDR: usize = 32;
    let mut out = vec![0u8; EHDR + PHDR + code.len()];
    // e_ident
    out[0] = 0x7f;
    out[1] = b'E';
    out[2] = b'L';
    out[3] = b'F';
    out[4] = 1; // EI_CLASS = ELFCLASS32
    out[5] = 1; // EI_DATA = ELFDATA2LSB
    out[6] = 1; // EI_VERSION
                // e_type = ET_EXEC
    out[16..18].copy_from_slice(&2u16.to_le_bytes());
    // e_machine = EM_RISCV (243)
    out[18..20].copy_from_slice(&243u16.to_le_bytes());
    // e_version
    out[20..24].copy_from_slice(&1u32.to_le_bytes());
    // e_entry
    out[24..28].copy_from_slice(&vaddr.to_le_bytes());
    // e_phoff = 52
    out[28..32].copy_from_slice(&(EHDR as u32).to_le_bytes());
    // e_ehsize / e_phentsize / e_phnum
    out[40..42].copy_from_slice(&(EHDR as u16).to_le_bytes());
    out[42..44].copy_from_slice(&(PHDR as u16).to_le_bytes());
    out[44..46].copy_from_slice(&1u16.to_le_bytes());
    // --- program header ---
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

/// 「往 UART0 写两个字节」的机器码。
///
/// ```text
/// lui  x1, 0x10000      ; x1 = 0x1000_0000（UART0 基址）
/// addi x2, x0, 72      ; 'H'
/// sb   x2, 0(x1)
/// addi x2, x0, 105     ; 'i'
/// sb   x2, 0(x1)
/// ```
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
        id: "proj-wiring".to_string(),
        name: "wiring".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "image.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: Default::default(),
    }
}

/// 装载一个工程并返回它产生的事件。
fn load(
    host: &mut KernelHost,
    project: Project,
) -> Vec<riscv_sim_contract::protocol::EventPayload> {
    host.handle_message(&Message::command(1, CommandPayload::LoadProject(project)))
}

/// ---- 内存图档位 ----

#[test]
fn the_default_profile_leaves_the_qemu_virt_window_unmapped() {
    let host = KernelHost::new();
    assert_eq!(host.memory_profile(), MemoryProfile::Default);
    // ⚠️ 默认档**不许**铺 128 MiB —— 那 128 MiB 是真的物理内存，每个
    // 宿主实例都吃一份 ⇒ 默认铺满会让测试跑不动。
    assert!(
        host.bus().lookup(0x8400_0000).is_none(),
        "默认档不该映射 0x8400_0000"
    );
    assert!(
        host.bus().lookup(QEMU_VIRT_UART0_BASE).is_none(),
        "默认档不该映射 UART0"
    );
}

#[test]
fn the_qemu_virt_profile_maps_the_whole_128_mib_and_the_three_devices() {
    let host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_eq!(host.memory_profile(), MemoryProfile::QemuVirt);

    // RAM：起点、终点、**堆尾**（osz 的 best-fit 分配器在堆尾写节点）。
    assert!(host.bus().lookup(QEMU_VIRT_RAM_BASE).is_some());
    assert!(
        host.bus()
            .lookup(QEMU_VIRT_RAM_BASE + QEMU_VIRT_RAM_SIZE - 4)
            .is_some(),
        "128 MiB 的最后 4 字节必须被映射 —— osz 的堆尾在那儿"
    );
    // ⚠️ **v0.26 · STAGE-11 裁决 G**：128 MiB 现在是**一个**区域（不是 8 个
    // 16 MiB 的）。
    //
    // ⚠️ **为什么不再拆**：旧 M-14 的每区域上限按**声明大小**卡（16 MiB）
    // ⇒ 128 MiB 只能拆成 8 段；新 M-14 按**物理占用**卡 ⇒ 一个 128 MiB
    // 的 region **登记成本 ≈ 0**（只有写过的页才占物理内存）⇒ 不需要拆。
    //
    // ⚠️ **走 `export_map()` 而不是 `bus().regions()`**：M-14 的 `Region`
    // 字段在 D-13（裁决 D）之后是**私有**的 ⇒ 断言必须落在契约层的
    // `RegionDto` 上（`base` / `size` / `kind` / `target` 是 `pub`）⇒
    // 这个改法在**旧 M-14 与新 M-14 两棵树里都成立**。
    let map = host.bus().export_map();
    let ram_in_window: Vec<_> = map
        .regions
        .iter()
        .filter(|r| {
            r.kind == "ram"
                && r.base >= QEMU_VIRT_RAM_BASE
                && r.base < QEMU_VIRT_RAM_BASE + QEMU_VIRT_RAM_SIZE
        })
        .collect();
    assert_eq!(
        ram_in_window.len(),
        1,
        "128 MiB 必须是**一个** RAM 区域（v0.26 之前是 8 个 16 MiB 的），实际是 {:#?}",
        ram_in_window
    );
    for r in &ram_in_window {
        assert_eq!(
            r.size,
            128 * 1024 * 1024,
            "QemuVirt 的 RAM 必须是**一个** 128 MiB 的区域，0x{:08x} 那个却只有 {} 字节",
            r.base,
            r.size
        );
    }
    assert_eq!(
        ram_in_window[0].base, QEMU_VIRT_RAM_BASE,
        "那一个 RAM 区域必须从 0x8000_0000 起（guest 看到的地址空间不变）"
    );

    // 三个 QEMU virt 外设。
    assert!(host.bus().lookup(QEMU_VIRT_UART0_BASE).is_some(), "UART0");
    assert!(host.bus().lookup(QEMU_VIRT_CLINT_BASE).is_some(), "CLINT");
    assert!(host.bus().lookup(QEMU_VIRT_PLIC_BASE).is_some(), "PLIC");
}

/// 一个落在 QEMU virt 窗口里的镜像 ⇒ 宿主自动切档。
#[test]
fn an_image_outside_the_default_window_switches_the_profile() {
    let mut host = KernelHost::new();
    assert_eq!(host.memory_profile(), MemoryProfile::Default);

    // 落址 0x8400_0000（osz 的数据 / bss / 栈 / 堆那一段）。
    let elf = minimal_elf(0x8400_0000, &uart_write_two_bytes());
    let events = load(&mut host, elf_project(elf));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, riscv_sim_contract::protocol::EventPayload::Error { .. })),
        "装载不该报错：{events:?}"
    );
    assert_eq!(
        host.memory_profile(),
        MemoryProfile::QemuVirt,
        "镜像落在 0x8400_0000 ⇒ 必须切到 QEMU virt 档（默认档只铺 4 MiB）"
    );
}

/// ---- console 通路（TX） ----

#[test]
fn a_guest_that_writes_the_uart_is_drained_from_the_console() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    let elf = minimal_elf(0x8400_0000, &uart_write_two_bytes());
    let events = load(&mut host, elf_project(elf));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, riscv_sim_contract::protocol::EventPayload::Error { .. })),
        "装载不该报错：{events:?}"
    );

    assert_eq!(host.console_pending(), 0, "还没跑，不该有 console 字节");

    // 跑 5 条指令（那 5 条指令正好是「往 UART0 写两个字节」）。
    host.handle_message(&Message::command(2, CommandPayload::StepN { n: 5 }));

    // ⚠️ **可观察的后果**：字节真的取出来了，顺序不变。
    assert_eq!(
        host.drain_console(),
        b"Hi".to_vec(),
        "guest 写进 UART THR 的两个字节必须原样取出来"
    );
    assert_eq!(host.console_pending(), 0, "取走之后不该还挂着");
    assert!(host.drain_console().is_empty(), "取走两次不该拿到两遍");
}

/// ---- 定时器通路（CLINT → M-16） ----

#[test]
fn the_clint_mtimecmp_reaches_the_interrupt_controller() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);

    // osz 的分步 64 位写：先写低位 -1，再写低位真值，最后写高位。
    host.bus_mut()
        .write(CLINT_MTIMECMP_LO, BusWidth::Word, 0xFFFF_FFFF)
        .unwrap();
    host.bus_mut()
        .write(CLINT_MTIMECMP_LO, BusWidth::Word, 1000)
        .unwrap();
    host.bus_mut()
        .write(CLINT_MTIMECMP_HI, BusWidth::Word, 0)
        .unwrap();

    // 同步之前 M-16 还没拿到比较值。
    assert_eq!(
        host.intc_timer_compare_ps(),
        0,
        "同步之前 M-16 不该有定时器比较值"
    );

    host.sync_interrupts_now();

    // 1000 个周期 @ 10 MHz = 100 ms = 100 000 000 ps。
    //
    // ⚠️ 断言值写**字面量**：单位换算错一个数量级，这条用例立刻红。
    assert_eq!(
        host.intc_timer_compare_ps(),
        100_000_000,
        "CLINT 的 mtimecmp（周期）必须换算成 ps 喂给 M-16"
    );
}

/// ---- 外部中断通路（UART → PLIC → M-16） ----

#[test]
fn a_plic_source_raises_the_external_line() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);

    // guest 侧的 PLIC 配置：source 10 优先级 1、context 0 使能、门限 0。
    host.bus_mut()
        .write(PLIC_PRIORITY_10, BusWidth::Word, 1)
        .unwrap();
    host.bus_mut()
        .write(PLIC_ENABLE_CTX0, BusWidth::Word, 1 << PLIC_SOURCE_UART)
        .unwrap();
    host.bus_mut()
        .write(PLIC_THRESHOLD_CTX0, BusWidth::Word, 0)
        .unwrap();
    // UART 的 IER bit0 = 允许 RX 中断。
    host.bus_mut()
        .write(UART0_IER, BusWidth::Byte, 0x01)
        .unwrap();

    assert!(
        host.intc_pending_lines().is_empty(),
        "还没喂数据，不该有中断挂起"
    );

    // 宿主往 UART 的 RX FIFO 塞一个字节 ⇒ UART 该抬线了。
    host.uart_receive(b'x');
    host.sync_interrupts_now();

    // ⚠️ 平台的**外部中断线**是内部线号 1（`IRQ_UART_RX_READY`）——
    // 它的架构 cause 码是 11（外部中断）。
    assert!(
        host.intc_pending_lines().contains(&1),
        "PLIC 交付的源必须拉起 M-16 的外部中断线，实际挂起的是 {:?}",
        host.intc_pending_lines()
    );
}

/// guest 直接往 UART 的 THR 写字节 ⇒ 不用跑任何指令也能进 console。
///
/// ⚠️ 这条钉的是「**映射**真的在总线上」：如果 UART 没被映射，写入会
/// 报「未映射访问」而不是进捕获环。
#[test]
fn the_uart_is_reachable_through_the_bus() {
    let mut host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    host.bus_mut()
        .write(UART0_THR, BusWidth::Byte, u32::from(b'A'))
        .unwrap();
    assert_eq!(host.drain_console(), b"A".to_vec());
}
