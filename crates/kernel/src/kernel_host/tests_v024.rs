//! Lane H —— **契约 v5**（`WriteConsole`）+ 宿主接线 + 反汇编行号接线。
//!
//! 冻结规格：`docs/detailed-design/STAGE-9_V024_Leftovers.md` §1 裁决 A /
//! B / G、§2.1 验收 H-1 · H-2 · H-6、§2.3 验收 J-1 ~ J-5。
//!
//! ## 这些用例在盯什么
//!
//! v0.23 的教训是「**加了守卫 ≠ 守卫接上了**」——要查它的**比较点**。
//! 同样的教训套在 `WriteConsole` 上：
//!
//! * 「命令被 dispatch 接住了」≠「字节进了 UART 的 **RX FIFO**」；
//! * 「字节进了 FIFO」≠「`LSR` 的 **`RX_VALID`** 置位了」；
//! * 「`RX_VALID` 置位了」≠「guest 读 `RHR` 读到的是**刚写的**那个字节」；
//! * 「同步器跑过了」≠「M-16 的**外部中断线**真的被拉起、guest 真的看到
//!   `mcause = 0x8000_000B`」；
//! * 「开了 RX 中断使能会抬线」≠「**没开**就不抬线」——后者才是判别力。
//!
//! ⇒ 本文件**不**断言「我调用了某个内部函数」，而是断言**可观察的后果**。
//!
//! ## 断言值一律写字面量
//!
//! 不从被测实现反推 —— 例如 `mcause` 断言 **`0x8000_000B`**（RISC-V 的
//! **架构 cause 码 11** = 机器外部中断），**不是**平台内部的线号 `1`。
//! 断言内部线号会与实现一起漂移 ⇒ 那条用例恒绿。

use riscv_sim_contract::model::{
    DisasmEntryDto, ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
    WriteSize,
};
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use crate::m14_bus::BusWidth;

use super::platform;
use super::{KernelHost, MemoryProfile};

/// `qemu-virt` 描述（**取值来源**：从描述查，不抄宿主常量）。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 描述里某个**器件**的基址（按器件 id 查）。
fn device_base(device_id: &str) -> u32 {
    platform::device_base(&qemu_virt_description(), device_id)
        .unwrap_or_else(|| panic!("qemu-virt 描述里必须有器件 {device_id}"))
}

/// UART0 的 RHR（接收保持寄存器）地址 —— **基址来自描述** + 偏移 `0x0`。
fn uart0_rhr() -> u32 {
    device_base("UART0")
}
/// UART0 的 IER（中断使能寄存器）地址 —— **基址来自描述** + 偏移 `0x1`。
fn uart0_ier() -> u32 {
    device_base("UART0") + 1
}
/// UART0 的 LSR（线路状态寄存器）地址 —— **基址来自描述** + 偏移 `0x5`。
fn uart0_lsr() -> u32 {
    device_base("UART0") + 5
}

/// `LSR` 的 `RX_VALID` 位（bit0）—— **RX FIFO 里有字节**。
const LSR_RX_VALID: u8 = 0x01;
/// `LSR` 的溢出位（bit2）—— **来晚了，字节被丢了**。
const LSR_OVERRUN: u8 = 0x04;

/// `IER` 的 RX 中断使能位（bit0）。
const IER_RX_ENABLE: u8 = 0x01;

/// 默认连线的**目标源号** —— **取值来源 = 默认连线那一份**。
///
/// ⚠️ **为什么不是写字面量 10**：v0.34（D-125）的主题是「一个约束一个
/// 落点」⇒ ⇒ 那个源号在宿主里**只写一次**（[`platform::default_wiring`]
/// 那一处）⇒ ⇒ 这里**引用**它，不另抄一份。
fn default_plic_source() -> u32 {
    platform::default_wiring(&qemu_virt_description())
        .into_iter()
        .next()
        .expect("qemu-virt 描述必须有默认连线")
        .source
}

/// PLIC 的**默认连线源**（UART RX）的优先级寄存器地址 —— **基址来自描述**。
fn plic_priority_uart_source() -> u32 {
    device_base("PLIC") + 4 * default_plic_source()
}
/// PLIC 的 context 0 的 enable 寄存器地址（默认连线的源落在第 0 个字）。
fn plic_enable_ctx0() -> u32 {
    device_base("PLIC") + 0x2000
}
/// PLIC 的 context 0 的门限寄存器地址。
fn plic_threshold_ctx0() -> u32 {
    device_base("PLIC") + 0x200000
}

/// 平台的**内部中断线号** —— UART 的 RX 就绪（`IRQ_UART_RX_READY` = 1）。
///
/// ⚠️ 这是**内部线号**，**不是**架构 cause 码：架构码是 `11`（外部中断）
/// ⇒ guest 看到的 `mcause` 是 `0x8000_000B`。两处断言**分别**用各自的
/// 字面量 —— 混用会把「架构码」与「平台线号」这两个空间糊在一起。
const IRQ_LINE_UART_RX: u16 = 1;

/// guest 看到的 `mcause` —— **架构**字面量。
///
/// ⚠️ `0x8000_000B` = `IRQ_BASE(0x8000_0000) | 11`：**机器外部中断**。
/// 这是 RISC-V 架构手册里的 **cause 码 11**，真实 RTOS 的中断分发就是按
/// 它写的 ⇒ 报别的值，真实 RTOS **一个都匹配不上**。
const MCAUSE_MACHINE_EXTERNAL: u32 = 0x8000_000B;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 造一个**带 UART0** 的宿主（QEMU virt 档）。
///
/// ⚠️ **必须**是 QEMU virt 档：UART0 / CLINT / PLIC **只有**这一档才映射
/// （默认档只有两段 RAM 与 GPIO）。
fn host_with_uart() -> KernelHost {
    KernelHost::new_with_profile(MemoryProfile::QemuVirt)
}

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 读 UART 的一个字节寄存器（guest 视角）。
fn read_reg(host: &KernelHost, addr: u32) -> u8 {
    host.bus()
        .read_only(addr, BusWidth::Byte)
        .unwrap_or_else(|e| panic!("读 0x{addr:08x} 失败：{e}")) as u8
}

/// 读 UART 的 `LSR`。
fn lsr(host: &KernelHost) -> u8 {
    read_reg(host, uart0_lsr())
}

/// 把 guest 侧的 PLIC / UART 中断配置**摆好**（PLIC 的**默认连线源**
/// 优先级 1、context 0 使能、门限 0）。
fn configure_plic(host: &mut KernelHost) {
    host.bus_mut()
        .write(plic_priority_uart_source(), BusWidth::Word, 1)
        .expect("写 PLIC 优先级");
    host.bus_mut()
        .write(
            plic_enable_ctx0(),
            BusWidth::Word,
            1 << default_plic_source(),
        )
        .expect("写 PLIC 使能");
    host.bus_mut()
        .write(plic_threshold_ctx0(), BusWidth::Word, 0)
        .expect("写 PLIC 门限");
}

/// 打开 UART 的 RX 中断使能（`IER` bit0）。
fn enable_uart_rx_interrupt(host: &mut KernelHost) {
    host.bus_mut()
        .write(uart0_ier(), BusWidth::Byte, u32::from(IER_RX_ENABLE))
        .expect("写 UART 的 IER");
}

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
///
/// ⚠️ **为什么自己造**：真实 RTOS（osz）的 ELF 在**仓外**（不进仓库），
/// 而「宿主接线对不对」这件事必须有一条**在仓内**的用例钉住 ⇒ 用一个
/// 手造的、只有几条指令的 ELF 来驱动它。
///
/// ⚠️ **落址必须在 `0x8400_0000`**：宿主的内存图**档位**是按镜像**用到
/// 的地址**挑的 ⇒ 落在这里才会切到 **QEMU virt 档**（UART0 / PLIC 才
/// 存在）。
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
    out[28..32].copy_from_slice(&(EHDR as u32).to_le_bytes()); // e_phoff = 52
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

/// 中断固件的基址（QEMU virt 档的 RAM 窗口起点）。
const FW_BASE: u32 = 0x8400_0000;

/// 中断固件的机器码（手写编码，小端）。
///
/// ```text
/// 0x00  lui   x5, 0x84000      ; x5 = 0x8400_0000
/// 0x04  addi  x5, x5, 0x20     ; x5 = handler 的地址
/// 0x08  lui   x6, 0x8          ; x6 = 0x8000
/// 0x0c  srli  x6, x6, 4        ; x6 = 0x800 = 1 << 11（**MEIE** 位号）
/// 0x10  csrrw x0, mtvec, x5    ; mtvec = handler（直接模式）
/// 0x14  csrrw x0, mie, x6      ; mie 的 bit 11 —— 使能 line 1
/// 0x18  csrrsi x0, mstatus, 8 ; mstatus.MIE（bit 3）= 1
/// 0x1c  jal   x0, 0            ; 主循环（自环，等中断）
/// 0x20  csrr  x8, mcause       ; handler：x8 = **guest 看到的 mcause**
/// 0x24  addi  x7, x7, 1        ; handler：计数器 +1
/// 0x28  csrrwi x0, irq_clear, 2 ; handler：清 line 1（位掩码 bit 1）
/// 0x2c  mret                  ; handler：回到 mepc
/// ```
///
/// ⚠️ **手写编码**：仓库里没有 RISC-V 汇编器（交叉工具链在**仓库外**），
/// 每一行都对着 M-13 的译码规则核对过（CSR 指令的 csr 号在 bits[31:20]、
/// zimm 在 bits[19:15]、funct3 在 bits[14:12]）。
///
/// ⚠️ **`mie` 的值是 `1 << 11`（= 2048），不是 `11`**：`mie` 的**位号**
/// 是 11（`MEIE`），而 CSR 里存的是**位图**。`csrrwi` 的 zimm 只有 5 位
/// ⇒ 写不进 2048 ⇒ 得先把它装进一个寄存器。
const FIRMWARE: [u32; 12] = [
    0x8400_02B7, // lui   x5, 0x84000
    0x0202_8293, // addi  x5, x5, 0x20
    0x0000_8337, // lui   x6, 0x8
    0x0043_5313, // srli  x6, x6, 4
    0x3052_9073, // csrrw x0, mtvec, x5
    0x3043_1073, // csrrw x0, mie, x6
    0x3004_6073, // csrrsi x0, mstatus, 8
    0x0000_006F, // jal   x0, 0
    0x3420_2473, // csrr  x8, mcause
    0x0013_8393, // addi  x7, x7, 1
    0x7C01_5073, // csrrwi x0, irq_clear, 2
    0x3020_0073, // mret
];

/// 固件的字节。
fn firmware_bytes() -> Vec<u8> {
    FIRMWARE.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// 造一个只含一个 ELF 镜像的工程。
fn elf_project(elf: Vec<u8>) -> Project {
    Project {
        id: "proj-v024".to_string(),
        name: "v024".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "image.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(elf),
        }],
        version: ProjectVersion::default(),
    }
}

/// 装载固件并返回宿主（固件落在 QEMU virt 的 RAM ⇒ 档位自动切过去）。
fn host_with_firmware() -> KernelHost {
    let mut host = KernelHost::new();
    let elf = minimal_elf(FW_BASE, &firmware_bytes());
    let events = dispatch(&mut host, CommandPayload::LoadProject(elf_project(elf)));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载不该报错：{events:?}"
    );
    assert_eq!(
        host.memory_profile(),
        MemoryProfile::QemuVirt,
        "固件落在 0x8400_0000 ⇒ 必须切到 QEMU virt 档（否则没有 UART0）"
    );
    host
}

// ---------------------------------------------------------------------------
// 1. `WriteConsole` → UART 的 RX FIFO（验收 H-1 / H-2）
// ---------------------------------------------------------------------------

/// (a) 写进去的字节**真的在 UART 的接收 FIFO 里** —— guest 读 `RHR`
/// 读到的就是它。
///
/// ⚠️ 判别力：字节没进 FIFO ⇒ 读出来是 `0x00`（**不是** `0x41`）。
#[test]
fn write_console_puts_the_byte_into_the_uart_rx_fifo() {
    let mut host = host_with_uart();
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x00,
        "还没写任何东西 ⇒ RHR 必须是空的（0）"
    );

    let events = dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x41] },
    );
    assert!(!events.is_empty(), "命令必须**有**回复");

    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x41,
        "guest 读 RHR 必须读到**刚写的那个字节**（0x41）"
    );
}

/// (b) 写进去之后，`LSR` 的 `RX_VALID`（bit0）**置位**。
///
/// ⚠️ 这是 **UART 模型自己的事**（`receive()` 做的）⇒ 宿主**不许**手动
/// 改 `LSR`（手动改就是「第二个真相」：寄存器与 FIFO 各说各话）。
#[test]
fn write_console_sets_the_lsr_rx_valid_bit() {
    let mut host = host_with_uart();
    assert_eq!(
        lsr(&host) & LSR_RX_VALID,
        0x00,
        "还没写任何东西 ⇒ RX_VALID 必须是 0"
    );

    dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x41] },
    );

    assert_eq!(
        lsr(&host) & LSR_RX_VALID,
        LSR_RX_VALID,
        "RX FIFO 里有字节 ⇒ LSR 的 RX_VALID（bit0）必须置位"
    );
}

/// (c) guest 读 `RHR` 拿到的是**刚写的那个字节** —— 不是旧值、不是 0。
///
/// ⚠️ 这条与 (a) 是**不同的坏**：FIFO 里有**别的**字节（例如上一轮残留
/// 的）时，(a) 仍绿、这条红 ⇒ 两条一起才有判别力。
#[test]
fn the_guest_reads_back_exactly_the_byte_that_was_written() {
    let mut host = host_with_uart();

    // 先写一个字节、**读掉**它（清空 FIFO）。
    dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x5A] },
    );
    assert_eq!(read_reg(&host, uart0_rhr()), 0x5A, "第一个字节必须能读出来");
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x00,
        "读掉之后 FIFO 必须空（RHR 是**读清**寄存器）"
    );

    // 再写**另一个**字节 ⇒ 读出来的必须是**新的**那个，不是旧的 0x5A。
    dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x42] },
    );
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x42,
        "读出来的必须是**刚写的**那个字节（0x42），不是上一个（0x5A）"
    );
}

/// (f) `WriteConsole` 的回复**至少有一个事件** —— 前端的 Promise 要落地。
///
/// ⚠️ 前端的 M-05 命令路由是「**第一个**带 `reqId` 的事件 resolve 掉
/// 这个命令」⇒ 返回空数组 ⇒ Promise **永远不落地**（只能等超时）。
#[test]
fn write_console_always_answers_with_at_least_one_event() {
    let mut host = host_with_uart();
    let events = dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x41] },
    );
    assert!(
        !events.is_empty(),
        "回复不能是空数组 —— 前端靠第一个事件落地 Promise"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::Tick { .. })),
        "回复里必须有一个**回复事件**（Tick），实际：{events:?}"
    );
}

/// (g) 空 `bytes` ⇒ **不报错**、**照样有回复**（幂等，不是错误）。
#[test]
fn an_empty_write_console_is_a_no_op_that_still_answers() {
    let mut host = host_with_uart();
    let events = dispatch(&mut host, CommandPayload::WriteConsole { bytes: vec![] });

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "空字节**不是**错误：{events:?}"
    );
    assert!(!events.is_empty(), "空字节也必须**有**回复");
    assert_eq!(
        lsr(&host) & LSR_RX_VALID,
        0x00,
        "空字节 ⇒ FIFO 里仍然什么都没有"
    );
}

/// (h-1) 一条命令写**多个**字节 ⇒ **三个都进 FIFO**、**顺序不丢**。
///
/// ⚠️ **v0.24 修正**：这条用例原先断言的是「UART 的 RX FIFO 只有 1 字节深
/// ⇒ 一条命令写 3 个字节只有第一个进得去，其余丢弃并锁存 overrun」——
/// 那与冻结规格 STAGE-9 裁决 B 第一步（「把**字节**塞进 UART 的 RX
/// FIFO」，**复数**）以及批次目标（「让 osz 的 shell **真的能用**」＝
/// **一行**命令送得进去）**不符** ⇒ 接收侧已加深成 16 字节深的 FIFO
/// （M-15 · Lane I），本条改为断言**新语义**。
///
/// ⚠️ 判别力：字节**没进** FIFO ⇒ `RHR` 读出来是 `0x00`（不是 `0x41`）；
/// 顺序乱了 ⇒ 第二、三个字节读出来的不是 `0x42` / `0x43`。
#[test]
fn several_bytes_in_one_command_all_reach_the_fifo_in_order() {
    let mut host = host_with_uart();
    dispatch(
        &mut host,
        CommandPayload::WriteConsole {
            bytes: vec![0x41, 0x42, 0x43],
        },
    );

    // ⚠️ **先看 `LSR`，再读 `RHR`**：读 `RHR` 是**读清**（消费掉 FIFO 里
    // 的**队头**）⇒ 读完最后一个字节 `RX_VALID` 才没了。
    let status = lsr(&host);
    assert_eq!(status & LSR_RX_VALID, LSR_RX_VALID, "RX_VALID 必须置位");
    assert_eq!(
        status & LSR_OVERRUN,
        0x00,
        "三个字节**没有**塞满 16 字节深的 FIFO ⇒ 不许锁存 overrun"
    );

    // 三个字节**都在** FIFO 里，**顺序**不变。
    assert_eq!(read_reg(&host, uart0_rhr()), 0x41, "队头必须是 0x41");
    assert_eq!(read_reg(&host, uart0_rhr()), 0x42, "第二个必须是 0x42");
    assert_eq!(read_reg(&host, uart0_rhr()), 0x43, "第三个必须是 0x43");
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x00,
        "三个字节读完之后 FIFO 必须空"
    );
}

/// (h-1b) 一条命令写**超过 FIFO 深度**的字节 ⇒ 留下的是**最早**的那些，
/// 多出来的被**丢**，且**丢是看得见的**（`LSR` 的 overrun 位锁存）。
///
/// ⚠️ **口径与 TX 捕获环一致**：**丢最新、保最早**（最早的字节是输入流的
/// **前缀**，丢前缀比丢尾巴更难诊断）。
#[test]
fn a_write_console_longer_than_the_fifo_drops_the_newest_and_latches_the_overrun() {
    let mut host = host_with_uart();
    // 20 个字节 > 16 字节深的 FIFO ⇒ 最后 4 个（0x64..=0x67）被丢。
    let bytes: Vec<u8> = (0x40u8..0x54).collect();
    assert_eq!(bytes.len(), 20, "夹具：必须比 FIFO 深");
    dispatch(&mut host, CommandPayload::WriteConsole { bytes });

    let status = lsr(&host);
    assert_eq!(
        status & LSR_OVERRUN,
        LSR_OVERRUN,
        "多出来的字节被丢掉 ⇒ 溢出位（bit2）必须**锁存**（丢是看得见的）"
    );
    assert_eq!(
        status & LSR_RX_VALID,
        LSR_RX_VALID,
        "FIFO 里还有字节 ⇒ RX_VALID 必须置位"
    );

    // 留下的是**最早**的 16 个字节，顺序不变。
    for i in 0..16u8 {
        assert_eq!(
            read_reg(&host, uart0_rhr()),
            0x40 + i,
            "留下的必须是**最早**的字节，且顺序不变"
        );
    }
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x00,
        "被丢的那 4 个字节不许出现在 FIFO 里"
    );
}

/// (h-2) 一条命令一个字节 ⇒ 字节**按顺序**都有（不丢、不乱序）。
///
/// ⚠️ 这条是 (h-1) 的**另一半**：FIFO 深度是模型的限制 ⇒ 「顺序」这条
/// 不变式只能在**读得过来**的粒度上验证。
#[test]
fn one_byte_per_command_keeps_the_order() {
    let mut host = host_with_uart();
    for byte in [0x41u8, 0x42, 0x43, 0x44] {
        let events = dispatch(
            &mut host,
            CommandPayload::WriteConsole { bytes: vec![byte] },
        );
        assert!(!events.is_empty(), "每条命令都必须有回复");
        assert_eq!(
            read_reg(&host, uart0_rhr()),
            byte,
            "读出来的必须就是这一条命令写的那个字节（顺序不能乱）"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. RX 中断 → M-16 的外部中断线 → guest 的 `mcause`（验收 H-2 / H-6）
// ---------------------------------------------------------------------------

/// (d) `IER` 的 bit0（RX 中断使能）**开着** ⇒ 同步后 M-16 的**外部中断
/// 线**被拉起 ⇒ guest 看到的 `mcause` 是 **`0x8000_000B`**（架构码 11）。
///
/// ⚠️ 断言值是**架构字面量**：`0x8000_000B` = `IRQ_BASE | 11`（机器外部
/// 中断）。**不是**平台内部的线号 `1` —— 报线号的话，真实 RTOS 的中断
/// 分发**一个都匹配不上**。
#[test]
fn the_rx_interrupt_reaches_the_guest_with_the_architectural_mcause() {
    let mut host = host_with_firmware();
    configure_plic(&mut host);
    enable_uart_rx_interrupt(&mut host);

    // 写一个字节 ⇒ 字节进 FIFO ⇒ `LSR` 的 RX_VALID 置位 ⇒ 同步器把
    // 「UART 的 RX 就绪」抬成 M-16 的外部中断线。
    let events = dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x41] },
    );
    assert!(!events.is_empty(), "命令必须有回复");

    // ---- 中断线真的被拉起了吗 ----
    assert!(
        host.intc_pending_lines().contains(&IRQ_LINE_UART_RX),
        "PLIC 交付的源必须拉起 M-16 的外部中断线（内部线号 1），实际挂起：{:?}",
        host.intc_pending_lines()
    );
    assert!(
        host.intc_state().raised.contains(&IRQ_LINE_UART_RX),
        "线电平必须被断言（raised），实际：{:?}",
        host.intc_state().raised
    );

    // ---- guest 真的收到中断了吗 ----
    dispatch(&mut host, CommandPayload::Continue);

    assert_eq!(
        host.registers()[8],
        MCAUSE_MACHINE_EXTERNAL,
        "guest 读到的 mcause 必须是 **0x8000_000B**（架构码 11 = 机器外部中断）"
    );
    assert!(
        host.registers()[7] >= 1,
        "handler 必须跑过一遍（x7 是 handler 的计数器），实际 {}",
        host.registers()[7]
    );
}

/// (e) `IER` 的 bit0 **没开** ⇒ 同步后**不**抬线。
///
/// ⚠️ **判别力**：这条与 (d) 必须**分得开**（开了会抬、不开不抬）⇒ 只
/// 写「开了会抬」不够 —— 那条用例在「永远抬线」的实现下也是绿的。
#[test]
fn without_the_rx_interrupt_enable_no_line_is_raised() {
    let mut host = host_with_firmware();
    configure_plic(&mut host);
    // ⚠️ **不开** `IER` 的 bit0（RX 中断使能）。
    host.bus_mut()
        .write(uart0_ier(), BusWidth::Byte, 0x00)
        .expect("写 UART 的 IER");

    dispatch(
        &mut host,
        CommandPayload::WriteConsole { bytes: vec![0x41] },
    );

    // 字节**确实**进了 FIFO（这条用例的前提）—— 否则它会因为「什么都没
    // 发生」而假绿。
    //
    // ⚠️ **先看 `LSR`，再读 `RHR`**：读 `RHR` 会**消费掉** FIFO 里的字节。
    assert_eq!(
        lsr(&host) & LSR_RX_VALID,
        LSR_RX_VALID,
        "RX_VALID 必须置位（否则这条用例没有判别力）"
    );
    assert_eq!(
        read_reg(&host, uart0_rhr()),
        0x41,
        "字节必须真的进了 FIFO（否则这条用例没有判别力）"
    );

    // ⚠️ **不抬线**。
    assert!(
        !host.intc_pending_lines().contains(&IRQ_LINE_UART_RX),
        "没开 RX 中断使能 ⇒ 同步器**不该**抬线，实际挂起：{:?}",
        host.intc_pending_lines()
    );
    assert!(
        !host.intc_state().raised.contains(&IRQ_LINE_UART_RX),
        "没开 RX 中断使能 ⇒ 线电平不该被断言，实际：{:?}",
        host.intc_state().raised
    );

    // guest 也不该收到中断 ⇒ handler 一次都没跑过。
    dispatch(&mut host, CommandPayload::Continue);
    assert_eq!(
        host.registers()[8],
        0x0000_0000,
        "没开 RX 中断使能 ⇒ guest 不该进 handler（x8 仍是 0）"
    );
    assert_eq!(
        host.registers()[7],
        0,
        "没开 RX 中断使能 ⇒ handler 不该跑过（x7 仍是 0）"
    );
}

// ---------------------------------------------------------------------------
// 3. 反汇编的行号（验收 J-1 ~ J-5）
// ---------------------------------------------------------------------------

/// 取第一条 `Disasm` 事件里的条目。
fn disasm_entries(events: &[EventPayload]) -> &[DisasmEntryDto] {
    for e in events {
        if let EventPayload::Disasm { entries } = e {
            return entries;
        }
    }
    panic!("没有 Disasm 事件：{events:?}");
}

/// DWARF 真产物的节区集合（`.debug_line` + `.debug_line_str`）。
///
/// ⚠️ 字节来自 **真工具链产物** ⇒ 仓库自包含，测试**不依赖**外部工具链。
fn dwarf_sections() -> Vec<(String, Vec<u8>)> {
    use crate::m11_symbol::dwarf::fixtures::{OSZ_DL, OSZ_DLS};
    vec![
        (
            crate::m11_symbol::dwarf::DEBUG_LINE_SECTION.to_string(),
            OSZ_DL.to_vec(),
        ),
        (
            crate::m11_symbol::dwarf::DEBUG_LINE_STR_SECTION.to_string(),
            OSZ_DLS.to_vec(),
        ),
    ]
}

/// (i) 有 DWARF 的镜像 ⇒ `Disasm` 事件的条目 `file` / `line` 是**真值**。
///
/// ⚠️ 断言值是**真产物**里的真值（文件名来自 `.debug_line_str`，行号来自
/// `.debug_line`）—— 不从被测实现反推。
#[test]
fn a_dwarf_image_gives_the_disassembly_real_file_and_line() {
    let mut host = KernelHost::new();

    // 在 `0x8000_0000`（高段 RAM）放一条指令 —— DWARF 行表覆盖这个地址。
    dispatch(
        &mut host,
        CommandPayload::WriteMemory {
            addr: 0x8000_0000,
            size: WriteSize::Word,
            value: 0x02A0_0093, // addi x1, x0, 42
        },
    );
    host.rebuild_symbol_index(&dwarf_sections())
        .expect("索引重建不得报错");

    let events = dispatch(
        &mut host,
        CommandPayload::Disasm {
            addr: 0x8000_0000,
            count: 1,
        },
    );
    let entries = disasm_entries(&events);
    assert_eq!(entries.len(), 1, "恰好一条：{entries:?}");
    assert_eq!(entries[0].addr, 0x8000_0000);
    assert_eq!(
        entries[0].file,
        Some("/tmp/osz-probe/kernel/base/hook/src/hook.c".to_string()),
        "文件名必须来自 `.debug_line_str`（真实路径）"
    );
    assert_eq!(
        entries[0].line,
        Some(8),
        "行号必须是真的（1 起的行号，不是 0、不是空串）"
    );
}

/// (j) 没有 DWARF 的镜像 ⇒ `Disasm` 条目的 `file` / `line` 都是
/// **`None`**（**不猜**）。
///
/// ⚠️ 「查过行表、没命中」（`Missed`）与「压根没有 DWARF」（`NoDwarf`）
/// 在 DTO 上**确实分不开**（契约只有两个可空字段）⇒ **都**是 `None`。
/// ⚠️ **绝不**是 `Some("")`、**绝不**是 `Some(0)`。
#[test]
fn an_image_without_dwarf_leaves_the_disassembly_file_and_line_none() {
    let mut host = KernelHost::new();
    // 裸镜像（没有调试节区）⇒ 没有 DWARF。
    dispatch(
        &mut host,
        CommandPayload::LoadProject(Project {
            id: "proj-v024-nodwarf".to_string(),
            name: "v024-nodwarf".to_string(),
            tree: ProductTree::default(),
            images: vec![ImageRef {
                name: "demo-firmware.bin".to_string(),
                kind: ImageKind::RawHex,
                source: ImageSource::Bytes(super::DEMO_FIRMWARE_BYTES.to_vec()),
            }],
            version: ProjectVersion::default(),
        }),
    );

    let events = dispatch(&mut host, CommandPayload::Disasm { addr: 0, count: 3 });
    let entries = disasm_entries(&events);
    assert_eq!(entries.len(), 3, "恰好三条：{entries:?}");
    for entry in entries {
        assert_eq!(
            entry.file, None,
            "没有 DWARF ⇒ `file` 必须是 None（不是 Some(\"\")、不是猜的）"
        );
        assert_eq!(
            entry.line, None,
            "没有 DWARF ⇒ `line` 必须是 None（不是 Some(0)、不是猜的）"
        );
    }
}
