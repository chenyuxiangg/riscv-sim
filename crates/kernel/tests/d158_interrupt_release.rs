//! D-158 的判别力（v0.55 · STAGE-40 §3.1）。
//!
//! ## 这条文件补的是哪一段
//!
//! D-158 的台账原文说「UART RX 中断在第一次中断之后被**永久堵死**」⇒ ⇒
//! 根因是**平台侧**的：M-16 的挂起队列**只加不减**（解除的唯一入口是
//! guest 写一个**平台私有 CSR `0x7C0`**⇒ ⇒ **标准固件根本不认识它**）⇒ ⇒
//! 第一次中断之后 `active_irq` 永远不复位 ⇒ ⇒ 单级嵌套护栏把**之后所有
//! 中断**永久堵死。
//!
//! ⇒ **真实剩余缺口不是「平台没有中断」**，而是「**平台的标准固件路径**
//! 没有 guest 指令级的端到端判别力」：
//!
//! - `crates/kernel/tests/d82_clint_end_to_end.rs` 那条固件**自己写
//!   `csrrwi x0, 0x7c0, 8`** ⇒ ⇒ 它走的是**平台私有路径**⇒ ⇒ 它测不到
//!   「标准固件能不能解开」；
//! - M-16 的单测证明「`clear()` 能撤线」⇒ ⇒ 但没人证明「**平台会自己
//!   撤**」。
//!
//! ⇒ **本文件就是那一段**：两段**不写私有 CSR** 的固件 ⇒ ⇒ 一段走
//! **定时器**（证 Fix A）、一段走 **UART + PLIC**（证 Fix B，**与 osz
//! 同一条路**）。
//!
//! ⇒ ⇒ **施工期实测之后扩到四段**（见 §8 订正 1）：根因不是「两处」而是
//! **四处** ⇒ ⇒ 而「不解除源」与「只排空不 complete」这两类**必须**有 ⇒ ⇒
//! 否则自写固件**太礼貌**（会写 Claim = complete ⇒ ⇒ 掩盖「平台没有参考侧
//! 那个 `claimed` 位」这件事）⇒ ⇒ 测不到真缺陷。
//!
//! ## 四段固件（汇编器产出 + `objdump` 逐条核对 + `objcopy` 逐字节核对）
//!
//! ⚠️ **工具链在仓外**（`~/riscv-sim-golden/toolchain/bin`），**不进仓库**
//! ⇒ ⇒ 字节与源码之间**没有自动一致性检查** ⇒ ⇒
//! [`the_firmware_bytes_are_still_the_ones_the_source_file_describes`] 是
//! **人可复核**的那一道防线。
//!
//! ### 固件一（UART 场景）—— 与 osz 同一条路
//!
//! ```text
//! 0x00 lui   x5, 0x84000      ; x5 = 0x8400_0000（固件基址 = QEMU virt 高段 RAM）
//! 0x04 addi  x5, x5, 0x50     ; x5 = 0x8400_0050（handler 的绝对地址）
//! 0x08 lui   x6, 0x10000      ; x6 = 0x1000_0000（UART0 基址）
//! 0x0c li    x7, 1
//! 0x10 sb    x7, 1(x6)        ; UART 的 IER @ 0x1 = 1（使能 RX 中断）
//! 0x14 lui   x8, 0xc000       ; x8 = 0x0C00_0000（PLIC 基址）
//! 0x18 li    x9, 10           ; 源号 10
//! 0x1c slli  x9, x9, 2        ; x9 = 40（优先级寄存器的偏移 = 4 × 源号）
//! 0x20 add   x8, x8, x9       ; x8 = &priority[10]
//! 0x24 li    x10, 1
//! 0x28 sw    x10, 0(x8)       ; priority[10] = 1（必须**严格大于**门限 0）
//! 0x2c lui   x8, 0xc002       ; x8 = 0x0C00_2000（使能区，context 0）
//! 0x30 li    x10, 0x400       ; bit 10
//! 0x34 sw    x10, 0(x8)       ; enable[ctx0] 的 bit 10 = 1
//! 0x38 lui   x11, 0x1         ; li 0x800 的高半（`li` 展开成两条）
//! 0x3c addi  x11, x11, -2048  ; x11 = 0x800 = 1 << 11（mie 的 MEIE 位）
//! 0x40 csrrw x0, mie, x11
//! 0x44 csrrsi x0, mstatus, 8  ; mstatus.MIE（bit 3）= 1
//! 0x48 csrrw x0, mtvec, x5     ; mtvec = handler（直接模式）
//! 0x4c jal   x0, 0x4c         ; loop —— 自旋，等中断
//! 0x50 lui   x6, 0xc200       ; x6 = 0x0C20_0000（PLIC 的 context 区）
//! 0x54 addi  x6, x6, 4        ; x6 = 0x0C20_0004（**Claim 寄存器**）
//! 0x58 lw    x7, 0(x6)        ; x7 = 源号（**读 Claim**）
//! 0x5c sw    x7, 0(x6)        ; **写 Claim = complete**（与 osz 同一手法）
//! 0x60 lui   x8, 0x10000      ; x8 = 0x1000_0000（UART0 基址）
//! 0x64 lbu   x9, 0(x8)        ; 读 RHR ⇒ **把 RX FIFO 排空**
//! 0x68 addi  x12, x12, 1      ; handler 计数器（**不是** x10 —— setup 用过它）
//! 0x6c mret
//! ```
//!
//! ⚠️ **读 Claim 的偏移是 `+4`** —— 这不是本平台自创的：**Spike 的
//! `riscv/plic.cc` 写着 `CONTEXT_CLAIM 4`**（`CONTEXT_THRESHOLD 0`）⇒ ⇒
//! **平台的布局与标准一致**（契约层零变更、M-15 零改动）。
//!
//! ⚠️ **handler 必须把 RX FIFO 排空**（`lbu` 读 RHR）⇒ ⇒ 否则 UART 仍然
//! 「有字节」⇒ ⇒ 平台的 `sync_interrupts` 会**再**请求一次 PLIC 源 ⇒ ⇒
//! 「guest 解除源」这件事**根本没发生**⇒ ⇒ 测不到 Fix B。
//!
//! ⚠️ **handler 必须写 Claim**（complete）⇒ ⇒ 否则 PLIC 的挂起位**仍在**
//! ⇒ ⇒ `plic.claim()` 仍返回 `Some` ⇒ ⇒ 平台**继续**请求 ⇒ ⇒ 那正是
//! 「不掩盖 guest 的 bug」的行为（裁决 E）⇒ ⇒ **这条固件走的是 osz 的
//! 手法**，两边一致。
//!
//! ### 固件二（定时器场景）—— 与 D-82 那段**只差一条指令**
//!
//! ```text
//! 0x00 lui   x5, 0x84000
//! 0x04 addi  x5, x5, 0x2c     ; handler 在 0x2c
//! 0x08 lui   x6, 0x2004       ; x6 = 0x0200_4000（CLINT 的 mtimecmp 低字）
//! 0x0c li    x7, 1
//! 0x10 sw    x7, 0(x6)        ; 写 mtimecmp（一个 CLINT 周期）
//! 0x14 lui   x8, 0x1          ; li 0x880 的高半
//! 0x18 addi  x8, x8, -1920    ; x8 = 0x880 = MTIE(bit 7) | **MEIE(bit 11)**
//! 0x1c csrrw x0, mie, x8
//! 0x20 csrrsi x0, mstatus, 8
//! 0x24 csrrw x0, mtvec, x5
//! 0x28 jal   x0, 0x28         ; loop
//! 0x2c csrr  x9,  mcause
//! 0x30 csrr  x10, mepc
//! 0x34 csrr  x11, mtval
//! 0x38 addi  x12, x12, 1      ; handler 计数器
//! 0x3c sw    x0, 0(x6)        ; ← **与 D-82 的唯一差别**：把 mtimecmp 写成 0
//!                                （= 关定时器）⇒ ⇒ guest **用标准手法**解除
//! 0x40 mret
//! ```
//!
//! ⚠️ **D-82 那条固件在 `0x38` 写的是 `csrrwi x0, 0x7c0, 8`** ⇒ ⇒ 它走的是
//! **平台私有 CSR** ⇒ ⇒ 它证明不了「标准固件能解开」⇒ ⇒ 本段把那条换成
//! `sw x0, 0(x6)`（**写 `mtimecmp` = 0 = 关定时器**）⇒ ⇒ guest 用的是
//! **标准手法**（写源）⇒ ⇒ 平台必须**自己**撤线。
//!
//! ### 固件三（f3 · 「只数 + `mret`」）—— **不解除源**
//!
//! ```text
//! 0x00 .. 0x4c  setup（与固件一**逐字相同**）
//! 0x4c jal   x0, 0x4c         ; loop
//! 0x50 addi  x12, x12, 1      ; handler 计数器
//! 0x54 mret
//! ```
//!
//! ⚠️ **它什么都不解除** —— 不读 Claim、不写 Claim、不读 RHR ⇒ ⇒ 设备线
//! **一直**挂着 ⇒ ⇒ PLIC 的源**一直**挂起 ⇒ ⇒ 外部线**一直**被请求。
//!
//! ⇒ ⇒ **它测的是「`mret` 之后还能不能**再交付**」** ⇒ ⇒ 修之前「服务中」
//! 标记永不复位 ⇒ ⇒ 第二次交付被单级嵌套护栏**永久**挡住 ⇒ ⇒ 计数器
//! **永远停在 1**。
//!
//! ### 固件四（f4 · 「读 RHR + 数 + `mret`」）—— **排空但不 complete**
//!
//! ```text
//! 0x00 .. 0x4c  setup（与固件一**逐字相同**）
//! 0x4c jal   x0, 0x4c         ; loop
//! 0x50 lui   x6, 0x10000
//! 0x54 lbu   x9, 0(x6)        ; 读 RHR ⇒ **排空 RX FIFO**
//! 0x58 addi  x12, x12, 1      ; handler 计数器
//! 0x5c mret
//! ```
//!
//! ⚠️ **它排空了 FIFO（设备线落 ⇒ 源该被撤销）**，但**照样不碰 Claim**
//! ⇒ ⇒ 「源被撤销」这件事**只可能**由平台自己做 ⇒ ⇒ 它测的是**平台侧**
//! 的撤线 ⇒ ⇒ 修之前源挂起位被**锁存** ⇒ ⇒ `claim()` 永远返回 `Some`
//! ⇒ ⇒ 外部线**永远**撤不掉。
//!
//! ## 为什么是**多条** `StepN`
//!
//! guest 在**批次内**写下的 `mtimecmp` / PLIC 配置，要等**批次边界**的
//! `sync_interrupts` 才被喂进 M-16 ⇒ ⇒ 「第一条 `StepN` 跑完时同步发生」
//! ⇒ ⇒ 「后面那些 `StepN` 里才交付得出来」。
//!
//! ⚠️ **中断交付本身**发生在**量子边界**（每 1 000 条指令一次，1 000 ps
//! 一条）⇒ ⇒ 一条 2 000 步的 `StepN` 足以跨过两个量子 ⇒ ⇒ 够用。

use riscv_sim_contract::csr::{
    CSR_MCAUSE, CSR_MEPC, CSR_MIP, CSR_MTVEC, IRQ_TIMER_COMPARE, IRQ_UART_RX_READY, MIE_MEIE_BIT,
    NO_ACTIVE_IRQ,
};
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::platform::{PlatformDescription, RegionSpec, WIRING_EVENT_RX_READY};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};
use riscv_sim_contract::session::M16IntcDto;

use riscv_sim_kernel::kernel_host::platform;
use riscv_sim_kernel::kernel_host::KernelHost;
use riscv_sim_kernel::m14_bus::BusWidth;

// ---------------------------------------------------------------------------
// 夹具：固件字节（汇编器产出 + `objdump` 逐条核对 + `objcopy` 逐字节核对）
// ---------------------------------------------------------------------------

/// UART 场景的固件机器码（小端，28 条 × 4 字节 = 112 字节）。
///
/// ⚠️ **来源在仓外**：`riscv-none-elf-as -march=rv32im_zicsr -mabi=ilp32`
/// ⇒ `objdump -d` 逐条核对 ⇒ `objcopy -O binary --only-section=.text.init`。
/// ⚠️ **必须带 `zicsr`**（否则 `csrr*` 全部报「需要 zicsr 扩展」）。
/// ⚠️ **不要用 `-march=rv32imc`**：压缩指令会让每条指令变成 2 字节 ⇒
/// `handler` 的偏移不再是 `0x50` ⇒ 偏移常量与固件布局耦合。
const UART_FIRMWARE: [u8; 112] = [
    // 0x00 lui   x5, 0x84000
    0xB7, 0x02, 0x00, 0x84, //
    // 0x04 addi  x5, x5, 0x50（**与固件布局耦合**）
    0x93, 0x82, 0x02, 0x05, //
    // 0x08 lui   x6, 0x10000（UART0 基址）
    0x37, 0x03, 0x00, 0x10, //
    // 0x0c li    x7, 1
    0x93, 0x03, 0x10, 0x00, //
    // 0x10 sb    x7, 1(x6)（IER @ 0x1 = 1）
    0xA3, 0x00, 0x73, 0x00, //
    // 0x14 lui   x8, 0xc000（PLIC 基址）
    0x37, 0x04, 0x00, 0x0C, //
    // 0x18 li    x9, 10（源号）
    0x93, 0x04, 0xA0, 0x00, //
    // 0x1c slli  x9, x9, 2
    0x93, 0x94, 0x24, 0x00, //
    // 0x20 add   x8, x8, x9
    0x33, 0x04, 0x94, 0x00, //
    // 0x24 li    x10, 1
    0x13, 0x05, 0x10, 0x00, //
    // 0x28 sw    x10, 0(x8)（priority[10] = 1）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x2c lui   x8, 0xc002（使能区，context 0）
    0x37, 0x24, 0x00, 0x0C, //
    // 0x30 li    x10, 0x400（bit 10）
    0x13, 0x05, 0x00, 0x40, //
    // 0x34 sw    x10, 0(x8)（enable[ctx0] 的 bit 10）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x38 lui   x11, 0x1（`li 0x800` 的高半）
    0xB7, 0x15, 0x00, 0x00, //
    // 0x3c addi  x11, x11, -2048（x11 = 0x800 = mie 的 MEIE 位）
    0x93, 0x85, 0x05, 0x80, //
    // 0x40 csrrw x0, mie, x11
    0x73, 0x90, 0x45, 0x30, //
    // 0x44 csrrsi x0, mstatus, 8（mstatus.MIE = bit 3）
    0x73, 0x60, 0x04, 0x30, //
    // 0x48 csrrw x0, mtvec, x5
    0x73, 0x90, 0x52, 0x30, //
    // 0x4c jal   x0, 0x4c（loop）
    0x6F, 0x00, 0x00, 0x00, //
    // 0x50 lui   x6, 0xc200（PLIC 的 context 区）
    0x37, 0x03, 0x20, 0x0C, //
    // 0x54 addi  x6, x6, 4（**Claim 寄存器** —— 标准布局是 `+4`）
    0x13, 0x03, 0x43, 0x00, //
    // 0x58 lw    x7, 0(x6)（读 Claim ⇒ 拿源号）
    0x83, 0x23, 0x03, 0x00, //
    // 0x5c sw    x7, 0(x6)（写 Claim = complete）
    0x23, 0x20, 0x73, 0x00, //
    // 0x60 lui   x8, 0x10000（UART0 基址）
    0x37, 0x04, 0x00, 0x10, //
    // 0x64 lbu   x9, 0(x8)（读 RHR ⇒ **排空 RX FIFO**）
    0x83, 0x44, 0x04, 0x00, //
    // 0x68 addi  x12, x12, 1（handler 计数器）
    //
    // ⚠️ **必须用 `x12`**：setup 那段用过 `x10`（装的是使能位 0x400）⇒ ⇒
    // 用 `x10` 当计数器 ⇒ ⇒ 「计数器 ≥ 2」**恒真**（起点就是 1024）⇒ ⇒
    // 那条断言是**空的**⇒ ⇒ 踩的就是「空转通过」那一族。
    0x13, 0x06, 0x16, 0x00, //
    // 0x6c mret
    0x73, 0x00, 0x20, 0x30, //
];

/// 定时器场景的固件机器码（小端，16 条 × 4 字节 = 64 字节）。
///
/// ⚠️ **与 `d82_clint_end_to_end.rs` 的那段只差 `0x38` 那一条**：那边写
/// `csrrwi x0, 0x7c0, 8`（**平台私有 CSR**）⇒ ⇒ 测不到「标准固件能不能
/// 解开」⇒ ⇒ 这边写 `sw x0, 0(x6)`（**写源 = 关定时器**）。
const TIMER_FIRMWARE: [u8; 68] = [
    // 0x00 lui   x5, 0x84000
    0xB7, 0x02, 0x00, 0x84, //
    // 0x04 addi  x5, x5, 0x2c（**与固件布局耦合**）
    0x93, 0x82, 0xC2, 0x02, //
    // 0x08 lui   x6, 0x2004（CLINT 的 mtimecmp 低字）
    0x37, 0x43, 0x00, 0x02, //
    // 0x0c li    x7, 1
    0x93, 0x03, 0x10, 0x00, //
    // 0x10 sw    x7, 0(x6)
    0x23, 0x20, 0x73, 0x00, //
    // 0x14 lui   x8, 0x1（`li 0x880` 的高半）
    0x37, 0x14, 0x00, 0x00, //
    // 0x18 addi  x8, x8, -1920（x8 = 0x880 = MTIE(bit 7) | MEIE(bit 11)）
    0x13, 0x04, 0x04, 0x88, //
    // 0x1c csrrw x0, mie, x8
    0x73, 0x10, 0x44, 0x30, //
    // 0x20 csrrsi x0, mstatus, 8
    0x73, 0x60, 0x04, 0x30, //
    // 0x24 csrrw x0, mtvec, x5
    0x73, 0x90, 0x52, 0x30, //
    // 0x28 jal   x0, 0x28（loop）
    0x6F, 0x00, 0x00, 0x00, //
    // 0x2c csrr  x9, mcause
    0xF3, 0x24, 0x20, 0x34, //
    // 0x30 csrr  x10, mepc
    0x73, 0x25, 0x10, 0x34, //
    // 0x34 csrr  x11, mtval
    0xF3, 0x25, 0x30, 0x34, //
    // 0x38 addi  x12, x12, 1（handler 计数器）
    0x13, 0x06, 0x16, 0x00, //
    // 0x3c sw    x0, 0(x6)  ← **与 D-82 的唯一差别**：写 mtimecmp = 0（关定时器）
    0x23, 0x20, 0x03, 0x00, //
    // 0x40 mret
    0x73, 0x00, 0x20, 0x30, //
];

/// 固件的**落址**（QEMU virt 档高段 RAM 内）。
///
/// ⚠️ **为什么必须是这个地址**：宿主的内存图档位是按镜像**用到的地址**
/// 挑的 ⇒ 落在这里才会切到 **`qemu-virt`** 档 ⇒ **PLIC / UART0 才存在**
/// （默认档没有）。
/// **固件三（f3 · 「只数 + `mret`」）** —— 22 条 × 4 字节 = **88 字节**。
///
/// ```text
/// 0x00 .. 0x4c  setup（与固件一**逐字相同**）
/// 0x4c jal   x0, 0x4c         ; loop
/// 0x50 addi  x12, x12, 1      ; handler 计数器
/// 0x54 mret
/// ```
///
/// ⚠️ **它什么都不解除** —— 不读 Claim、不写 Claim（= 不 complete）、不读
/// RHR（= 不排空 FIFO）⇒ ⇒ 设备中断线**一直**挂着 ⇒ ⇒ 平台的 PLIC 源
/// **一直**挂起 ⇒ ⇒ 外部线**一直**被请求。
///
/// ⇒ ⇒ **它测的是「`mret` 之后还能不能**再交付**」** ⇒ ⇒ 如果平台在
/// `mret` 时不释放「服务中」标记 ⇒ ⇒ 单级嵌套护栏把第二次交付**永久**
/// 挡住 ⇒ ⇒ handler 计数器**永远停在 1**。
///
/// ⚠️ **为什么必须有这一段**：既有那段（[`UART_FIRMWARE`]）**会**写
/// Claim = complete ⇒ ⇒ 它**掩盖**了「平台没有参考侧那个 `claimed` 位」
/// 这件事 ⇒ ⇒ 必须有**不解除源**的那一段才暴露得出真缺陷。
const NO_CLAIM_FIRMWARE: [u8; 88] = [
    // 0x00 lui   x5, 0x84000
    0xB7, 0x02, 0x00, 0x84, //
    // 0x04 addi  x5, x5, 0x50（**与固件布局耦合**）
    0x93, 0x82, 0x02, 0x05, //
    // 0x08 lui   x6, 0x10000（UART0 基址）
    0x37, 0x03, 0x00, 0x10, //
    // 0x0c li    x7, 1
    0x93, 0x03, 0x10, 0x00, //
    // 0x10 sb    x7, 1(x6)（IER @ 0x1 = 1）
    0xA3, 0x00, 0x73, 0x00, //
    // 0x14 lui   x8, 0xc000（PLIC 基址）
    0x37, 0x04, 0x00, 0x0C, //
    // 0x18 li    x9, 10（源号）
    0x93, 0x04, 0xA0, 0x00, //
    // 0x1c slli  x9, x9, 2
    0x93, 0x94, 0x24, 0x00, //
    // 0x20 add   x8, x8, x9
    0x33, 0x04, 0x94, 0x00, //
    // 0x24 li    x10, 1
    0x13, 0x05, 0x10, 0x00, //
    // 0x28 sw    x10, 0(x8)（priority[10] = 1）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x2c lui   x8, 0xc002（使能区，context 0）
    0x37, 0x24, 0x00, 0x0C, //
    // 0x30 li    x10, 0x400（bit 10）
    0x13, 0x05, 0x00, 0x40, //
    // 0x34 sw    x10, 0(x8)（enable[ctx0] 的 bit 10）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x38 lui   x11, 0x1
    0xB7, 0x15, 0x00, 0x00, //
    // 0x3c addi  x11, x11, -2048（mie.MEIE = 0x800）
    0x93, 0x85, 0x05, 0x80, //
    // 0x40 csrrw x0, mie, x11
    0x73, 0x90, 0x45, 0x30, //
    // 0x44 csrrsi x0, mstatus, 8（mstatus.MIE）
    0x73, 0x60, 0x04, 0x30, //
    // 0x48 csrrw x0, mtvec, x5
    0x73, 0x90, 0x52, 0x30, //
    // 0x4c jal   x0, 0x4c（loop）
    0x6F, 0x00, 0x00, 0x00, //
    // 0x50 addi  x12, x12, 1（handler 计数器）
    0x13, 0x06, 0x16, 0x00, //
    // 0x54 mret
    0x73, 0x00, 0x20, 0x30, //
];

/// **固件四（f4 · 「读 RHR + 数 + `mret`」）** —— 24 条 × 4 字节 = **96 字节**。
///
/// ```text
/// 0x00 .. 0x4c  setup（与固件一**逐字相同**）
/// 0x4c jal   x0, 0x4c         ; loop
/// 0x50 lui   x6, 0x10000      ; UART0 基址
/// 0x54 lbu   x9, 0(x6)        ; 读 RHR ⇒ **把 RX FIFO 排空**
/// 0x58 addi  x12, x12, 1      ; handler 计数器
/// 0x5c mret
/// ```
///
/// ⚠️ **它与固件三只差一条**（`lbu`）⇒ ⇒ 它**排空**了 FIFO ⇒ ⇒ 设备中断
/// 线**落下** ⇒ ⇒ 源**该**被撤销 ⇒ ⇒ **但它照样不碰 Claim** ⇒ ⇒ 「源被
/// 撤销」这件事**只可能**由平台自己做（guest 没写 complete）⇒ ⇒ 它测的
/// 是**平台侧的撤线**。
const DRAIN_ONLY_FIRMWARE: [u8; 96] = [
    // 0x00 lui   x5, 0x84000
    0xB7, 0x02, 0x00, 0x84, //
    // 0x04 addi  x5, x5, 0x50（**与固件布局耦合**）
    0x93, 0x82, 0x02, 0x05, //
    // 0x08 lui   x6, 0x10000（UART0 基址）
    0x37, 0x03, 0x00, 0x10, //
    // 0x0c li    x7, 1
    0x93, 0x03, 0x10, 0x00, //
    // 0x10 sb    x7, 1(x6)（IER @ 0x1 = 1）
    0xA3, 0x00, 0x73, 0x00, //
    // 0x14 lui   x8, 0xc000（PLIC 基址）
    0x37, 0x04, 0x00, 0x0C, //
    // 0x18 li    x9, 10（源号）
    0x93, 0x04, 0xA0, 0x00, //
    // 0x1c slli  x9, x9, 2
    0x93, 0x94, 0x24, 0x00, //
    // 0x20 add   x8, x8, x9
    0x33, 0x04, 0x94, 0x00, //
    // 0x24 li    x10, 1
    0x13, 0x05, 0x10, 0x00, //
    // 0x28 sw    x10, 0(x8)（priority[10] = 1）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x2c lui   x8, 0xc002（使能区，context 0）
    0x37, 0x24, 0x00, 0x0C, //
    // 0x30 li    x10, 0x400（bit 10）
    0x13, 0x05, 0x00, 0x40, //
    // 0x34 sw    x10, 0(x8)（enable[ctx0] 的 bit 10）
    0x23, 0x20, 0xA4, 0x00, //
    // 0x38 lui   x11, 0x1
    0xB7, 0x15, 0x00, 0x00, //
    // 0x3c addi  x11, x11, -2048（mie.MEIE = 0x800）
    0x93, 0x85, 0x05, 0x80, //
    // 0x40 csrrw x0, mie, x11
    0x73, 0x90, 0x45, 0x30, //
    // 0x44 csrrsi x0, mstatus, 8（mstatus.MIE）
    0x73, 0x60, 0x04, 0x30, //
    // 0x48 csrrw x0, mtvec, x5
    0x73, 0x90, 0x52, 0x30, //
    // 0x4c jal   x0, 0x4c（loop）
    0x6F, 0x00, 0x00, 0x00, //
    // 0x50 lui   x6, 0x10000（UART0 基址）
    0x37, 0x03, 0x00, 0x10, //
    // 0x54 lbu   x9, 0(x6)（读 RHR ⇒ **排空 RX FIFO**）
    0x83, 0x44, 0x03, 0x00, //
    // 0x58 addi  x12, x12, 1（handler 计数器）
    0x13, 0x06, 0x16, 0x00, //
    // 0x5c mret
    0x73, 0x00, 0x20, 0x30, //
];

const FW_BASE: u32 = 0x8400_0000;

/// UART 场景固件的自旋指令（`loop`）的**绝对地址**。
const UART_SPIN_ABS: u32 = 0x8400_004C;

/// UART 场景固件的 handler 的**绝对地址**。
const UART_HANDLER_ABS: u32 = 0x8400_0050;

/// 定时器场景固件的自旋指令的**绝对地址**。
const TIMER_SPIN_ABS: u32 = 0x8400_0028;

/// 定时器场景固件的 handler 的**绝对地址**。
const TIMER_HANDLER_ABS: u32 = 0x8400_002C;

/// 平台的**内部中断线号** —— UART rx-ready（`IRQ_UART_RX_READY`）。
const LINE_EXTERNAL: u16 = IRQ_UART_RX_READY;

/// 平台的**内部中断线号** —— 定时器比较（`IRQ_TIMER_COMPARE`）。
const LINE_TIMER: u16 = IRQ_TIMER_COMPARE;

/// guest 看到的 `mcause` —— **架构字面量**：中断位 | 机器外部中断（11）。
///
/// ⚠️ **不许从实现反推**（不写 `arch_mcause_for_line(...)`）—— 那会让
/// 用例跟实现同源 ⇒ 改实现时跟着变 ⇒ 变成恒绿。
const MCAUSE_EXTERNAL: u32 = 0x8000_000B;

/// 固件写死的 **PLIC 源号**（UART0 的 rx-ready）。
///
/// ⚠️ **不许从实现反推**（不写「连线表里的那个」）—— 固件的机器码里
/// **写死了** 10 ⇒ ⇒ 这条常量必须与机器码一致 ⇒ ⇒ 断言它与连线表一致
/// 的那条用例（[`the_firmware_source_id_matches_the_wiring_table`]）就是
/// 「机器码没漂」的那道防线。
const FIRMWARE_SOURCE_ID: u32 = 10;

/// 固件写死的 **PLIC 基址**（`lui x8, 0xc000` ⇒ `0x0C00_0000`）。
const PLIC_BASE: u32 = 0x0C00_0000;

/// 固件写死的 **UART0 基址**（`lui x6, 0x10000` ⇒ `0x1000_0000`）。
const UART_BASE: u32 = 0x1000_0000;

/// PLIC 的**优先级**寄存器偏移（`4 × 源号`）。
const PLIC_PRIORITY_STRIDE: u32 = 4;

/// PLIC 的**使能**区偏移。
const PLIC_ENABLE_BASE: u32 = 0x2000;

/// PLIC 的 **context** 区偏移（门限 / Claim 都在这里）。
const PLIC_CONTEXT_BASE: u32 = 0x200_000;

/// PLIC 的 **Claim** 寄存器在 context 区里的偏移。
///
/// ⚠️ **`+4` 不是本平台自创** —— Spike 的 `riscv/plic.cc` 写着
/// `CONTEXT_THRESHOLD 0` / `CONTEXT_CLAIM 4` ⇒ ⇒ **标准布局**。
const PLIC_CLAIM_OFFSET: u32 = 4;

/// 机器 context 的编号（PLIC context 0 = 机器模式）。
const PLIC_CONTEXT_MACHINE: u32 = 0;

/// handler 计数器所在的通用寄存器（`x12`）。
///
/// ⚠️ **必须是 setup 那段没碰过的寄存器** —— `x10` 在 setup 里装的是
/// PLIC 的使能位（`0x400`）⇒ ⇒ 用它当计数器 ⇒ ⇒ 「计数器 ≥ 2」**恒真**
/// ⇒ ⇒ 断言变空（踩的就是「空转通过」那一族）。
const HANDLER_COUNTER_REG: usize = 12;

/// 平台 context 区里每个 context 的间距。
const PLIC_CONTEXT_STRIDE: u32 = 0x1000;

/// 让能区的每个 context 的间距。
const PLIC_ENABLE_STRIDE: u32 = 0x80;

/// PLIC 源的优先级寄存器地址。
fn plic_priority_addr(source: u32) -> u32 {
    PLIC_BASE + PLIC_PRIORITY_STRIDE * source
}

/// PLIC 某 context 的使能寄存器地址（源号 < 32 ⇒ 落在第 0 个字）。
fn plic_enable_addr(context: u32) -> u32 {
    PLIC_BASE + PLIC_ENABLE_BASE + PLIC_ENABLE_STRIDE * context
}

/// PLIC 某 context 的 Claim 寄存器地址。
fn plic_claim_addr(context: u32) -> u32 {
    PLIC_BASE + PLIC_CONTEXT_BASE + PLIC_CONTEXT_STRIDE * context + PLIC_CLAIM_OFFSET
}

/// UART 的 **IER** 寄存器地址（ns16550 布局：`0x1`）。
const UART_IER_OFFSET: u32 = 0x1;

/// UART 的 **RHR** 寄存器地址（ns16550 布局：`0x0`，读它 = 弹出队头）。
const UART_RHR_OFFSET: u32 = 0x0;

/// 第一批的条数 —— 跑够「写 `mtimecmp` + 使能 + 装 `mtvec`」，然后进自旋。
const SETUP_STEPS: u32 = 100;

/// 后面的条数 —— **≥ 2 个量子**（1 000 条 / 量子）。
const TRAP_STEPS: u32 = 2_000;

// ---------------------------------------------------------------------------
// 描述取值（**取值来源 = 平台描述**，不抄宿主常量）
// ---------------------------------------------------------------------------

/// `qemu-virt` 描述。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 描述里某个**器件**的基址（按器件 id 查）。
fn device_base(device_id: &str) -> u32 {
    platform::device_base(&qemu_virt_description(), device_id)
        .unwrap_or_else(|| panic!("qemu-virt 描述里必须有器件 {device_id}"))
}

/// 描述里**有没有**某个器件（用于「这个 id 在描述里吗」的前置断言）。
fn device_exists(device_id: &str) -> bool {
    qemu_virt_description()
        .regions
        .iter()
        .any(|region| match region {
            RegionSpec::Peripheral { device, .. } => match device {
                riscv_sim_contract::platform::DeviceSpec::Uart16550 { id, .. } => id == device_id,
                riscv_sim_contract::platform::DeviceSpec::Plic { id, .. } => id == device_id,
                riscv_sim_contract::platform::DeviceSpec::Clint { id, .. } => id == device_id,
                _ => false,
            },
            _ => false,
        })
}

// ---------------------------------------------------------------------------
// 夹具：宿主
// ---------------------------------------------------------------------------

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

/// 造一个只含一个 ELF 镜像的工程。
fn project_with(code: &[u8]) -> Project {
    Project {
        id: "proj-d158".to_string(),
        name: "d158".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "d158.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(minimal_elf(FW_BASE, code)),
        }],
        version: ProjectVersion::default(),
    }
}

/// 装载固件并返回宿主。
fn host_with(code: &[u8]) -> KernelHost {
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::LoadProject(project_with(code)),
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载固件必须成功：{events:?}"
    );
    assert_eq!(
        host.platform_id(),
        platform::PLATFORM_ID_QEMU_VIRT,
        "固件落在 0x8400_0000 ⇒ 必须自动切到 qemu-virt 档（只有它有 PLIC / UART）"
    );
    host
}

/// 跑 `n` 条指令（**不许**出错）。
fn step(host: &mut KernelHost, n: u32) {
    let events = host.handle_message(&Message::command(1, CommandPayload::StepN { n }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "步进 {n} 条必须成功：{events:?}"
    );
}

/// CSR 文件里某个 CSR 的值（读不到 = 0）。
fn csr(host: &KernelHost, number: u16) -> u32 {
    host.engine_snapshot()
        .state
        .csrs
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 控制器的导出状态。
fn intc_state(host: &KernelHost) -> M16IntcDto {
    host.intc_state()
}

/// 某条线**是不是**在挂起队列里。
fn is_pending(state: &M16IntcDto, line: u16) -> bool {
    state.pending.iter().any(|p| p.line == line)
}

/// 某条线**是不是**被断言着。
fn is_raised(state: &M16IntcDto, line: u16) -> bool {
    state.raised.contains(&line)
}

// ---------------------------------------------------------------------------
// 用例：夹具防漂移
// ---------------------------------------------------------------------------

/// **夹具防漂移**：UART 固件的**每一条关键指令**必须还是「我们以为的那些
/// 指令」。
///
/// ⚠️ 仓库里**没有** RISC-V 汇编器（交叉工具链在**仓库外**）⇒ ⇒ 字节与
/// 汇编源之间**没有自动一致性检查** ⇒ ⇒ 这条用例是**人可复核**的那一道。
#[test]
fn the_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x00 lui x5, 0x84000（固件基址）
    assert_eq!(&UART_FIRMWARE[0x00..0x04], &[0xB7, 0x02, 0x00, 0x84]);
    // 0x04 addi x5, x5, 0x50（**与固件布局耦合**）
    assert_eq!(&UART_FIRMWARE[0x04..0x08], &[0x93, 0x82, 0x02, 0x05]);
    // 0x08 lui x6, 0x10000（UART0 基址）
    assert_eq!(&UART_FIRMWARE[0x08..0x0c], &[0x37, 0x03, 0x00, 0x10]);
    // 0x10 sb x7, 1(x6)（IER @ 0x1）
    assert_eq!(&UART_FIRMWARE[0x10..0x14], &[0xA3, 0x00, 0x73, 0x00]);
    // 0x18 li x9, 10（**源号**）
    assert_eq!(&UART_FIRMWARE[0x18..0x1c], &[0x93, 0x04, 0xA0, 0x00]);
    // 0x28 sw x10, 0(x8)（priority[10] = 1）
    assert_eq!(&UART_FIRMWARE[0x28..0x2c], &[0x23, 0x20, 0xA4, 0x00]);
    // 0x2c lui x8, 0xc002（使能区）
    assert_eq!(&UART_FIRMWARE[0x2c..0x30], &[0x37, 0x24, 0x00, 0x0C]);
    // 0x30 li x10, 0x400（bit 10）
    assert_eq!(&UART_FIRMWARE[0x30..0x34], &[0x13, 0x05, 0x00, 0x40]);
    // 0x3c addi x11, x11, -2048（mie 的 MEIE 位 = 0x800）
    assert_eq!(&UART_FIRMWARE[0x3c..0x40], &[0x93, 0x85, 0x05, 0x80]);
    // 0x40 csrrw x0, mie, x11
    assert_eq!(&UART_FIRMWARE[0x40..0x44], &[0x73, 0x90, 0x45, 0x30]);
    // 0x48 csrrw x0, mtvec, x5
    assert_eq!(&UART_FIRMWARE[0x48..0x4c], &[0x73, 0x90, 0x52, 0x30]);
    // 0x4c jal x0, 0x4c（loop —— `mepc` 应当落在它上面）
    assert_eq!(&UART_FIRMWARE[0x4c..0x50], &[0x6F, 0x00, 0x00, 0x00]);
    // 0x50 lui x6, 0xc200（PLIC 的 context 区）
    assert_eq!(&UART_FIRMWARE[0x50..0x54], &[0x37, 0x03, 0x20, 0x0C]);
    // 0x54 addi x6, x6, 4（**Claim 在 `+4`** —— 标准布局）
    assert_eq!(&UART_FIRMWARE[0x54..0x58], &[0x13, 0x03, 0x43, 0x00]);
    // 0x58 lw x7, 0(x6)（读 Claim）
    assert_eq!(&UART_FIRMWARE[0x58..0x5c], &[0x83, 0x23, 0x03, 0x00]);
    // 0x5c sw x7, 0(x6)（写 Claim = complete）
    assert_eq!(&UART_FIRMWARE[0x5c..0x60], &[0x23, 0x20, 0x73, 0x00]);
    // 0x64 lbu x9, 0(x8)（读 RHR ⇒ **排空 RX FIFO**）
    assert_eq!(&UART_FIRMWARE[0x64..0x68], &[0x83, 0x44, 0x04, 0x00]);
    // 0x68 addi x12, x12, 1（handler 计数器 —— **不许**用 x10，setup 用过它）
    assert_eq!(&UART_FIRMWARE[0x68..0x6c], &[0x13, 0x06, 0x16, 0x00]);
    // 0x6c mret
    assert_eq!(&UART_FIRMWARE[0x6c..0x70], &[0x73, 0x00, 0x20, 0x30]);
    // 固件总长 112 字节（0x70）
    assert_eq!(UART_FIRMWARE.len(), 112, "UART 固件长度变了");
}

/// **夹具防漂移**：定时器固件的关键指令。
#[test]
fn the_timer_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x08 lui x6, 0x2004（CLINT 的 mtimecmp 低字 —— **不是** 0x20040）
    assert_eq!(&TIMER_FIRMWARE[0x08..0x0c], &[0x37, 0x43, 0x00, 0x02]);
    // 0x18 addi x8, x8, -1920（mie = 0x880 = MTIE | **MEIE**）
    assert_eq!(&TIMER_FIRMWARE[0x18..0x1c], &[0x13, 0x04, 0x04, 0x88]);
    // 0x1c csrrw x0, mie, x8
    assert_eq!(&TIMER_FIRMWARE[0x1c..0x20], &[0x73, 0x10, 0x44, 0x30]);
    // 0x24 csrrw x0, mtvec, x5
    assert_eq!(&TIMER_FIRMWARE[0x24..0x28], &[0x73, 0x90, 0x52, 0x30]);
    // 0x28 jal x0, 0x28（loop）
    assert_eq!(&TIMER_FIRMWARE[0x28..0x2c], &[0x6F, 0x00, 0x00, 0x00]);
    // 0x38 addi x12, x12, 1（handler 计数器）
    assert_eq!(&TIMER_FIRMWARE[0x38..0x3c], &[0x13, 0x06, 0x16, 0x00]);
    // 0x3c sw x0, 0(x6)  ← **与 D-82 的唯一差别**：写 mtimecmp = 0
    //
    // ⚠️ 这条是「**不许**写平台私有 CSR」那道防线：写 `csrrwi x0, 0x7c0, 8`
    // ⇒ ⇒ 走的是**平台私有路径**⇒ ⇒ 用例就测不到「标准固件能不能解开」。
    assert_eq!(&TIMER_FIRMWARE[0x3c..0x40], &[0x23, 0x20, 0x03, 0x00]);
    // 0x40 mret
    assert_eq!(&TIMER_FIRMWARE[0x40..0x44], &[0x73, 0x00, 0x20, 0x30]);
    assert_eq!(TIMER_FIRMWARE.len(), 68, "定时器固件长度变了");
}

/// **夹具防漂移**：固件写死的**源号**必须与**连线表**对得上。
///
/// ⚠️ 固件的源号是**机器码里的常量** ⇒ ⇒ 连线表一旦改了源号，那段固件
/// 就给**别的源**设了优先级与使能 ⇒ ⇒ 表现得像「UART 中断通路坏了」⇒ ⇒
/// 极难定位。⇒ 这条用例把两者的对应关系**钉住**。
#[test]
fn the_firmware_source_id_matches_the_wiring_table() {
    let host = host_with(&UART_FIRMWARE);
    let source = host
        .wiring_table()
        .iter()
        .find(|w| w.event == WIRING_EVENT_RX_READY)
        .map(|w| w.source)
        .expect("连线表里必须有 rx_ready 那一条");
    assert_eq!(
        source, FIRMWARE_SOURCE_ID,
        "固件写死的源号（机器码里的常量）必须与连线表一致 —— \
         连线表改了源号这条就红"
    );
}

/// **夹具防漂移**：固件写死的**基址**必须与**描述**对得上。
#[test]
fn the_firmware_hardcoded_bases_match_the_description() {
    assert_eq!(
        device_base("UART0"),
        UART_BASE,
        "固件写死的 UART0 基址必须等于描述里的那个"
    );
    assert_eq!(
        device_base("PLIC"),
        PLIC_BASE,
        "固件写死的 PLIC 基址必须等于描述里的那个"
    );
    assert!(
        device_exists("UART0") && device_exists("PLIC"),
        "描述里必须有 UART0 与 PLIC"
    );
}

// ---------------------------------------------------------------------------
// 用例：宿主层（Fix B · 外部线的撤线）
// ---------------------------------------------------------------------------

/// 把 UART 的 **IER** 打开（使能 RX 中断）。
///
/// ⚠️ 平台的 `sync_interrupts` 第 ② 步的判据是
/// 「`LSR` 的 RX_VALID ∧ `IER` 的 bit0」⇒ ⇒ 两者缺一不可。
fn enable_uart_rx_interrupt(host: &mut KernelHost) {
    host.bus_mut()
        .write(
            UART_BASE + UART_IER_OFFSET,
            BusWidth::from(WriteSize::Byte),
            1,
        )
        .expect("写 UART 的 IER 必须成功");
}

/// 把 PLIC 的某个源**武装起来**（优先级 > 门限 ∧ 在 context 0 里使能）。
fn arm_plic_source(host: &mut KernelHost, source: u32) {
    host.bus_mut()
        .write(
            plic_priority_addr(source),
            BusWidth::from(WriteSize::Word),
            1,
        )
        .expect("写 PLIC 的优先级必须成功");
    host.bus_mut()
        .write(
            plic_enable_addr(PLIC_CONTEXT_MACHINE),
            BusWidth::from(WriteSize::Word),
            1u32 << source,
        )
        .expect("写 PLIC 的使能位必须成功");
}

/// **A-6**：PLIC 侧**有**可交付的源 ⇒ ⇒ 平台**请求**外部线。
///
/// ⚠️ **前置（非真空）**：UART 的 RX FIFO 里必须**真的有字节**（`LSR` 的
/// RX_VALID）⇒ ⇒ 否则 `sync_interrupts` 的第 ② 步不会去请求 PLIC ⇒ ⇒
/// 「外部线没被请求」与「压根没有字节」**不可区分**。
#[test]
fn the_platform_raises_the_external_line_when_the_plic_has_a_deliverable_source() {
    let mut host = host_with(&UART_FIRMWARE);
    enable_uart_rx_interrupt(&mut host);
    arm_plic_source(&mut host, FIRMWARE_SOURCE_ID);

    // 一个字节进 RX FIFO。
    host.uart_receive(b'A');
    host.sync_interrupts_now();

    let state = intc_state(&host);
    assert!(
        is_raised(&state, LINE_EXTERNAL),
        "PLIC 有可交付的源 ⇒ 外部线必须被断言：{:?}",
        state.raised
    );
    assert!(
        is_pending(&state, LINE_EXTERNAL),
        "PLIC 有可交付的源 ⇒ 外部线必须在挂起队列里：{:?}",
        state.pending
    );
}

/// **A-9**：guest **没有**解除源 ⇒ ⇒ 平台**继续**请求（**不掩盖 guest 的
/// bug** —— 裁决 E）。
#[test]
fn the_platform_keeps_asking_while_the_guest_has_not_released_the_source() {
    let mut host = host_with(&UART_FIRMWARE);
    enable_uart_rx_interrupt(&mut host);
    arm_plic_source(&mut host, FIRMWARE_SOURCE_ID);

    host.uart_receive(b'A');
    host.sync_interrupts_now();
    assert!(
        is_raised(&intc_state(&host), LINE_EXTERNAL),
        "前置：第一轮必须已经抬起了外部线"
    );

    // 第二轮：guest **没有**解除源（PLIC 的挂起位仍在）⇒ ⇒ 平台必须继续请求。
    host.sync_interrupts_now();

    let state = intc_state(&host);
    assert!(
        is_raised(&state, LINE_EXTERNAL),
        "guest 没解除源 ⇒ 平台必须**继续**请求（不替 guest 掩盖）：{:?}",
        state.raised
    );
    assert!(
        is_pending(&state, LINE_EXTERNAL),
        "guest 没解除源 ⇒ 挂起项必须留着：{:?}",
        state.pending
    );
}

/// **A-5**：PLIC 侧**没有**可交付的源 ⇒ ⇒ 平台**撤掉**外部线。
///
/// ⚠️ **两个前置缺一不可**（否则这条是真空的）：
/// - UART 的 RX FIFO 必须**被排空**（guest 读了 RHR）⇒ ⇒ 否则第 ② 步会
///   **再**请求一次 PLIC 源 ⇒ ⇒ `claim()` 仍返回 `Some` ⇒ ⇒ 撤不了线；
/// - PLIC 的挂起位必须**被解除**（guest 写了 Claim = complete）⇒ ⇒ 否则
///   `claim()` 仍返回 `Some`。
#[test]
fn the_platform_releases_the_external_line_when_the_plic_has_nothing_to_deliver() {
    let mut host = host_with(&UART_FIRMWARE);
    enable_uart_rx_interrupt(&mut host);
    arm_plic_source(&mut host, FIRMWARE_SOURCE_ID);

    host.uart_receive(b'A');
    host.sync_interrupts_now();
    assert!(
        is_raised(&intc_state(&host), LINE_EXTERNAL),
        "前置：第一轮必须已经抬起了外部线"
    );

    // ---- guest 侧的两个动作（与 osz 同一手法）----
    // ① 读 RHR ⇒ 排空 RX FIFO。
    host.read_memory(UART_BASE + UART_RHR_OFFSET, WriteSize::Byte)
        .expect("读 UART 的 RHR 必须成功");
    // ② 写 Claim = complete ⇒ PLIC 的挂起位被清。
    host.bus_mut()
        .write(
            plic_claim_addr(PLIC_CONTEXT_MACHINE),
            BusWidth::from(WriteSize::Word),
            FIRMWARE_SOURCE_ID,
        )
        .expect("写 PLIC 的 Claim 必须成功");

    host.sync_interrupts_now();

    let state = intc_state(&host);
    assert!(
        !is_raised(&state, LINE_EXTERNAL),
        "PLIC 没有可交付的源 ⇒ 外部线必须被撤掉：{:?}",
        state.raised
    );
    assert!(
        !is_pending(&state, LINE_EXTERNAL),
        "PLIC 没有可交付的源 ⇒ 外部线的挂起项必须被移除：{:?}",
        state.pending
    );
    assert_eq!(
        state.active_irq, NO_ACTIVE_IRQ,
        "PLIC 没有可交付的源 ⇒ `active_irq` 必须复位（否则外部线被单级防护永久堵死）"
    );
}

// ---------------------------------------------------------------------------
// 用例：guest 指令级（Fix B · 端到端 —— 与 osz 同一条路）
// ---------------------------------------------------------------------------

/// **A-7（核心判别式）**：**两次** UART 字节 ⇒ ⇒ handler 至少跑**两次**。
///
/// ⚠️ **这条在修之前必然是红的**：第一次中断之后，M-16 的外部线**永远**
/// 留在挂起队列里、`active_irq` **永远**不复位 ⇒ ⇒ 第二个字节的请求被
/// **同线去重**折叠掉 ⇒ ⇒ 单级护栏挡住交付 ⇒ ⇒ handler **只跑一次**。
///
/// ⚠️ **判别式落在 handler 的计数器上**（`x10`）—— 不是「没报错」。
#[test]
fn the_guest_handles_two_uart_interrupts_in_a_row() {
    let mut host = host_with(&UART_FIRMWARE);

    // ---- 第一批：guest 把 PLIC / UART / mie / mstatus / mtvec 装好 ----
    step(&mut host, SETUP_STEPS);

    // ---- 前置（非真空）：计数器此刻必须还是 **0** ----
    //
    // ⚠️ **没有这条前置，下面那句「计数器 ≥ 2」就是空的** —— 计数器的
    // 起点一旦不是 0，「≥ 2」可能一开始就成立 ⇒ ⇒ 断言恒真 ⇒ ⇒ 踩的就是
    // 「空转通过」那一族（**对象没了还绿吗**）。
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        0,
        "前置：handler 计数器在 setup 之后必须还是 0 —— 否则「计数器 ≥ 2」恒真"
    );

    // ---- 第一个字节 ----
    host.uart_receive(b'A');
    host.sync_interrupts_now();
    step(&mut host, TRAP_STEPS);

    let first = host.registers()[HANDLER_COUNTER_REG];
    assert!(
        first >= 1,
        "第一个字节必须真的触发一次外部中断（handler 计数器 x12 = {first}）"
    );

    // ---- 第二个字节 ----
    host.uart_receive(b'B');
    host.sync_interrupts_now();
    step(&mut host, TRAP_STEPS);

    let second = host.registers()[HANDLER_COUNTER_REG];
    assert!(
        second >= 2,
        "第二个字节必须**再**触发一次外部中断（handler 计数器 x12 = {second}）—— \
         修之前外部线永远撤不掉 ⇒ ⇒ 第二次永远交付不了"
    );
}

/// **A-7 的配套**：第一次中断的 `mcause` 必须是**架构字面量**
/// `0x8000_000B`（机器外部中断），**不是**平台的内部线号 1。
#[test]
fn the_guest_sees_the_architectural_external_interrupt_cause() {
    let mut host = host_with(&UART_FIRMWARE);
    step(&mut host, SETUP_STEPS);
    host.uart_receive(b'A');
    host.sync_interrupts_now();
    step(&mut host, TRAP_STEPS);

    assert_eq!(
        csr(&host, CSR_MCAUSE),
        MCAUSE_EXTERNAL,
        "mcause 必须是「中断位 | **架构 cause 码 11**」（机器外部中断），\
         不是内部线号 1"
    );
    assert_eq!(
        csr(&host, CSR_MEPC),
        UART_SPIN_ABS,
        "mepc 必须落在自旋指令（jal x0, 0x4c）的绝对地址上"
    );
    assert_eq!(
        csr(&host, CSR_MTVEC),
        UART_HANDLER_ABS,
        "mtvec 必须是 guest 自己写的那张向量表（handler 的绝对地址）"
    );
}

// ---------------------------------------------------------------------------
// 用例：guest 指令级（Fix A · 定时器的标准解除手法）
// ---------------------------------------------------------------------------

/// **A-8**：guest 用**标准手法**解除定时器（写 `mtimecmp` = 0）⇒ ⇒ 平台
/// 必须**自己**撤掉定时器线。
///
/// ⚠️ 这段固件**不写**平台私有 CSR `0x7C0` ⇒ ⇒ 它走的是**标准路径**⇒ ⇒
/// 修之前：定时器线**永远**撤不掉 ⇒ ⇒ `active_irq` 永远不复位 ⇒ ⇒
/// **之后所有中断永久堵死**。
#[test]
fn the_platform_releases_the_timer_line_when_the_guest_reprograms_the_compare_value() {
    let mut host = host_with(&TIMER_FIRMWARE);

    // ---- 第一批：写 `mtimecmp` + 使能 + 装 `mtvec`，然后进自旋 ----
    step(&mut host, SETUP_STEPS);

    // ---- 前置（非真空）：计数器此刻必须还是 **0** ----
    //
    // ⚠️ 同 A-7：没有这条前置，「计数器 ≥ 1」可能一开始就成立 ⇒ ⇒ 断言恒真。
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        0,
        "前置：handler 计数器在 setup 之后必须还是 0 —— 否则「计数器 ≥ 1」恒真"
    );

    // ---- 第二批：跨过量子边界 ⇒ 定时器到期 ⇒ 交付 ⇒ 陷入 ----
    step(&mut host, TRAP_STEPS);

    // ---- 前置：handler 真的跑过（否则下面全是真空断言）----
    let counter = host.registers()[HANDLER_COUNTER_REG];
    assert!(
        counter >= 1,
        "handler 必须至少跑过一次（x12 = {counter}）—— 没跑过说明 guest 根本没陷入"
    );

    let state = intc_state(&host);
    assert!(
        !is_raised(&state, LINE_TIMER),
        "guest 用**标准手法**解除（写 mtimecmp = 0）⇒ 定时器线必须被撤掉：{:?}",
        state.raised
    );
    assert!(
        !is_pending(&state, LINE_TIMER),
        "guest 用**标准手法**解除 ⇒ 挂起队列里不许还有定时器线：{:?}",
        state.pending
    );
    assert_eq!(
        state.active_irq, NO_ACTIVE_IRQ,
        "guest 用**标准手法**解除 ⇒ `active_irq` 必须复位（否则这条线被单级防护永久堵死）"
    );
    assert_eq!(
        host.intc_delegation_failures(),
        0,
        "中断委派不许失败（失败时 intc_state 返回的是默认值 —— 正好等于断言值）"
    );
    // 顺带钉住「guest 真的用自己的向量表处理了这次中断」。
    assert_eq!(
        csr(&host, CSR_MTVEC),
        TIMER_HANDLER_ABS,
        "mtvec 必须是 guest 自己写的那张向量表（handler 的绝对地址）"
    );
    assert_eq!(
        csr(&host, CSR_MEPC),
        TIMER_SPIN_ABS,
        "mepc 必须落在自旋指令的绝对地址上"
    );
}

/// **A-8 的配套**：解除之后，**另一条**中断（外部）必须**交付**得出来。
///
/// ⚠️ 这是 D-158 的**因果链**那一步：定时器占着 `active_irq` ⇒ ⇒ 外部
/// 中断永远交付不了 ⇒ ⇒ 解除之后必须交付得了。
///
/// ⚠️ **判别式落在 `mcause` 上**（**交付**了），**不是**落在「外部线被
/// 抬起」上（那件事与释放无关 ⇒ ⇒ 零判别力）。
#[test]
fn the_external_interrupt_is_delivered_after_the_timer_line_was_released() {
    let mut host = host_with(&TIMER_FIRMWARE);
    enable_uart_rx_interrupt(&mut host);
    arm_plic_source(&mut host, FIRMWARE_SOURCE_ID);

    // ---- 先让定时器陷一次（占住 `active_irq`）----
    step(&mut host, SETUP_STEPS);
    step(&mut host, TRAP_STEPS);
    assert!(host.registers()[12] >= 1, "前置：定时器中断必须真的交付过");
    assert_eq!(
        csr(&host, CSR_MCAUSE),
        0x8000_0007,
        "前置：此刻 `mcause` 必须还是**定时器**中断（架构 cause 码 7）"
    );

    // ---- 然后来一个 UART 字节 ⇒ 外部中断必须**交付**得出来 ----
    host.uart_receive(b'A');
    host.sync_interrupts_now();
    step(&mut host, TRAP_STEPS);

    assert_eq!(
        csr(&host, CSR_MCAUSE),
        MCAUSE_EXTERNAL,
        "定时器线被解除之后 ⇒ 外部中断必须真的**交付**（mcause = 机器外部中断）—— \
         修之前定时器占着 `active_irq` ⇒ ⇒ 外部中断永远交付不了"
    );
}

// ---------------------------------------------------------------------------
// 用例：契约层零升版
// ---------------------------------------------------------------------------

/// **A-10**：契约层的四个版本常量 —— **本批（v0.57 · STAGE-42）** 的口径。
///
/// ⚠️ **D-158 批（v0.55）** 的那一条是「四个版本号**一个都不许动**」⇒ ⇒ 当时 `SCHEMA_VERSION` 是 **8**。
///
/// ⚠️ **v0.57 批（STAGE-42）** 是**契约升版**的那一批（符号查询 + 整段转储 + 条件断点表达式 ⇒ ⇒ 两条新命令 + 两条新事件）⇒ ⇒ **信封版本号从 8 升到 9** ⇒ ⇒ **literal 跟着走**（同 `crates/contract/src/platform.rs` 里那条 `s16_a1`）。
///
/// ⚠️ **另外三个版本号仍然一个都不动**（STAGE-42 §2.8）⇒ ⇒ 那三条断言**一字未改** ⇒ ⇒ 快照 / 板级描述 / 板级规则集**没有**因为这一批变形。
#[test]
fn the_contract_layer_version_numbers_are_untouched() {
    assert_eq!(
        riscv_sim_contract::SCHEMA_VERSION,
        9,
        "契约层的信封版本号（v0.57 批升到 9：新命令 + 新事件）"
    );
    assert_eq!(
        riscv_sim_contract::model::SIM_STATE_SCHEMA_VERSION,
        2,
        "会话快照的版本号不许动"
    );
    assert_eq!(
        riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        1,
        "板级描述的版本号不许动"
    );
    assert_eq!(
        riscv_sim_contract::platform::PLATFORM_RULES_VERSION,
        3,
        "板级合法性规则集的版本号不许动"
    );
}

// ---------------------------------------------------------------------------
// 夹具：第三、四段固件的防漂移
// ---------------------------------------------------------------------------

/// `lui x6, 0xc200` 的机器码（**PLIC 的 context 区**基址）。
///
/// ⚠️ **f3 / f4 里不许出现它** —— 出现了就说明那段固件**碰了 Claim** ⇒ ⇒
/// 它就不再是「不解除源」的那一段 ⇒ ⇒ 那些用例测的就不再是「平台侧」的
/// 撤线 ⇒ ⇒ **判别力悄悄换了一个对象**。
const PLIC_CONTEXT_LUI: [u8; 4] = [0x37, 0x03, 0x20, 0x0C];

/// **夹具防漂移**：f3 那段固件的关键指令。
///
/// ⚠️ **它与固件一**逐字相同**（setup 部分）** —— ⇒ ⇒ 两段固件的差别**只有**
/// handler ⇒ ⇒ 「差别是什么」这件事本身被钉住。
#[test]
fn the_no_claim_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x04 addi x5, x5, 0x50（**与固件布局耦合**）
    assert_eq!(&NO_CLAIM_FIRMWARE[0x04..0x08], &[0x93, 0x82, 0x02, 0x05]);
    // 0x10 sb x7, 1(x6)（IER @ 0x1）
    assert_eq!(&NO_CLAIM_FIRMWARE[0x10..0x14], &[0xA3, 0x00, 0x73, 0x00]);
    // 0x18 li x9, 10（**源号**）
    assert_eq!(&NO_CLAIM_FIRMWARE[0x18..0x1c], &[0x93, 0x04, 0xA0, 0x00]);
    // 0x40 csrrw x0, mie, x11
    assert_eq!(&NO_CLAIM_FIRMWARE[0x40..0x44], &[0x73, 0x90, 0x45, 0x30]);
    // 0x44 csrrsi x0, mstatus, 8
    assert_eq!(&NO_CLAIM_FIRMWARE[0x44..0x48], &[0x73, 0x60, 0x04, 0x30]);
    // 0x48 csrrw x0, mtvec, x5
    assert_eq!(&NO_CLAIM_FIRMWARE[0x48..0x4c], &[0x73, 0x90, 0x52, 0x30]);
    // 0x4c jal x0, 0x4c（loop）
    assert_eq!(&NO_CLAIM_FIRMWARE[0x4c..0x50], &[0x6F, 0x00, 0x00, 0x00]);
    // 0x50 addi x12, x12, 1（handler 计数器）
    assert_eq!(&NO_CLAIM_FIRMWARE[0x50..0x54], &[0x13, 0x06, 0x16, 0x00]);
    // 0x54 mret
    assert_eq!(&NO_CLAIM_FIRMWARE[0x54..0x58], &[0x73, 0x00, 0x20, 0x30]);
    // 固件总长 88 字节（0x58）
    assert_eq!(NO_CLAIM_FIRMWARE.len(), 88, "f3 固件长度变了");
    // ⚠️ **不许**碰 PLIC 的 context 区 ⇒ ⇒ 「不解除源」这件事被钉住。
    assert!(
        !NO_CLAIM_FIRMWARE
            .windows(4)
            .any(|w| w == PLIC_CONTEXT_LUI.as_slice()),
        "f3 固件里不许出现 `lui x6, 0xc200`（PLIC context 区）—— \
         它必须**不碰** Claim，否则它就不再是「不解除源」的那一段"
    );
}

/// **夹具防漂移**：f4 那段固件的关键指令。
#[test]
fn the_drain_only_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x04 addi x5, x5, 0x50
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x04..0x08], &[0x93, 0x82, 0x02, 0x05]);
    // 0x10 sb x7, 1(x6)（IER @ 0x1）
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x10..0x14], &[0xA3, 0x00, 0x73, 0x00]);
    // 0x40 csrrw x0, mie, x11
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x40..0x44], &[0x73, 0x90, 0x45, 0x30]);
    // 0x48 csrrw x0, mtvec, x5
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x48..0x4c], &[0x73, 0x90, 0x52, 0x30]);
    // 0x4c jal x0, 0x4c（loop）
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x4c..0x50], &[0x6F, 0x00, 0x00, 0x00]);
    // 0x50 lui x6, 0x10000（UART0 基址）
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x50..0x54], &[0x37, 0x03, 0x00, 0x10]);
    // 0x54 lbu x9, 0(x6)（读 RHR ⇒ **排空 RX FIFO**）
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x54..0x58], &[0x83, 0x44, 0x03, 0x00]);
    // 0x58 addi x12, x12, 1（handler 计数器）
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x58..0x5c], &[0x13, 0x06, 0x16, 0x00]);
    // 0x5c mret
    assert_eq!(&DRAIN_ONLY_FIRMWARE[0x5c..0x60], &[0x73, 0x00, 0x20, 0x30]);
    // 固件总长 96 字节（0x60）
    assert_eq!(DRAIN_ONLY_FIRMWARE.len(), 96, "f4 固件长度变了");
    // ⚠️ **不许**碰 PLIC 的 context 区 ⇒ ⇒ 「源只可能由平台撤销」被钉住。
    assert!(
        !DRAIN_ONLY_FIRMWARE
            .windows(4)
            .any(|w| w == PLIC_CONTEXT_LUI.as_slice()),
        "f4 固件里不许出现 `lui x6, 0xc200`（PLIC context 区）—— \
         它必须**不碰** Claim，否则「源被撤销」就不是平台单方面做的了"
    );
}

// ---------------------------------------------------------------------------
// 夹具：观测用的辅助
// ---------------------------------------------------------------------------

/// UART 的 **LSR** 寄存器地址（ns16550 布局：`0x5`）。
const UART_LSR_OFFSET: u32 = 0x5;

/// LSR 的 **RX_VALID** 位（bit 0）—— 「RX FIFO 里还有字节」。
const UART_LSR_RX_VALID: u32 = 0x01;

/// RX FIFO 里**还有没有**字节（读 LSR 的 bit 0）。
///
/// ⚠️ **读 LSR 是无副作用的**（只有 RHR 是读破坏寄存器）⇒ ⇒ 这条读数
/// **不会**把 FIFO 排空 ⇒ ⇒ 不会影响被观测的那件事。
fn uart_rx_valid(host: &KernelHost) -> bool {
    let lsr = host
        .read_memory(UART_BASE + UART_LSR_OFFSET, WriteSize::Byte)
        .expect("读 UART 的 LSR 必须成功");
    lsr & UART_LSR_RX_VALID != 0
}

/// `mip` 里的 **MEIP** 位（机器外部中断的挂起位）。
///
/// ⚠️ **从契约层的 `MIE_MEIE_BIT` 推出来**（不写死 11）⇒ ⇒ `mie` 与 `mip`
/// 共用那张位号表（契约层 `std_irq_bit_for_line` 的原话）⇒ ⇒ 这里跟着走。
const MIP_MEIP_MASK: u32 = 1 << MIE_MEIE_BIT;

/// 跑批次直到 handler 计数器**至少**到 `target`（撞上界就红）。
///
/// ⚠️ **为什么必须有上界**：判据「计数器能涨」**必须**在**有限**步内成立
/// ⇒ ⇒ 没有上界 ⇒ ⇒ 实现坏了的时候这条用例会**永远**跑下去 ⇒ ⇒ 门禁的
/// 失败模式变成「卡」而不是「红」（那是被咬过的那一族）。
fn run_until_counter(host: &mut KernelHost, target: u32, what: &str) {
    let mut rounds = 0u32;
    while host.registers()[HANDLER_COUNTER_REG] < target {
        step(host, TRAP_STEPS);
        rounds += 1;
        assert!(
            rounds < 40,
            "{what} —— 40 轮之后 handler 计数器仍然是 {}（目标 {target}）",
            host.registers()[HANDLER_COUNTER_REG]
        );
    }
}

// ---------------------------------------------------------------------------
// 用例：guest 指令级（Fix C · 「服务中」标记的释放）
// ---------------------------------------------------------------------------

/// **A-11**：guest 把 RX FIFO **排空**（但**不**写 Claim）⇒ ⇒ 设备中断线
/// **落下** ⇒ ⇒ 平台的 PLIC 源**必须被撤销** ⇒ ⇒ 外部线**必须被撤掉**。
///
/// ⚠️ **这一段专治「源挂起位被锁存」** —— 修复前：宿主第 ② 步**只**
/// `request` **从不撤销** ⇒ ⇒ 源**永久**挂起 ⇒ ⇒ `plic.claim()` **永远**
/// 返回 `Some` ⇒ ⇒ 外部线**永远**撤不掉 ⇒ ⇒ 这条**必然红**。
#[test]
fn the_platform_withdraws_the_external_line_once_the_device_line_drops() {
    let mut host = host_with(&DRAIN_ONLY_FIRMWARE);

    // ---- 前置（非真空）：setup 之后计数器必须还是 **0** ----
    step(&mut host, SETUP_STEPS);
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        0,
        "前置：handler 计数器在 setup 之后必须还是 0 —— 否则「跑过 ≥ 1」恒真"
    );

    // ---- 一个字节进 FIFO ⇒ 外部线被抬起 ----
    host.uart_receive(b'A');
    host.sync_interrupts_now();
    assert!(
        is_raised(&intc_state(&host), LINE_EXTERNAL),
        "前置：第一轮必须已经抬起了外部线 —— 否则「线被撤掉」恒真"
    );

    // ---- 跑到 handler 真的跑过（它读 RHR ⇒ FIFO 被排空）----
    run_until_counter(
        &mut host,
        1,
        "外部中断必须真的交付一次（handler 一次也没跑 ⇒ 后面全是真空断言）",
    );

    // ---- 前置：FIFO 必须已经被排空（否则「撤线」不是因为设备线落了）----
    assert!(
        !uart_rx_valid(&host),
        "前置：handler 必须已经把 RX FIFO 排空（它读 RHR）"
    );

    // ---- 判据：外部线必须被撤掉 ----
    let state = intc_state(&host);
    assert!(
        !is_raised(&state, LINE_EXTERNAL),
        "设备线落下 ⇒ PLIC 的源被撤销 ⇒ 外部线必须被撤掉：{:?}",
        state.raised
    );
    assert!(
        !is_pending(&state, LINE_EXTERNAL),
        "设备线落下 ⇒ 外部线的挂起项必须被移除：{:?}",
        state.pending
    );
}

/// **A-12**：`mret` 释放的是「**服务中**标记」，**不是**「挂起项」。
///
/// ⇒ ⇒ 两个断言：
/// - **判别式**：handler 计数器**能涨到 2** ⇒ ⇒ 「服务中」标记在 `mret`
///   时被释放了 ⇒ ⇒ 同一条线**能再交付一次**（修复前：单级嵌套护栏把
///   第二次交付**永久**挡住 ⇒ ⇒ 计数器**永远停在 1** ⇒ ⇒ 这条**必然红**）。
/// - **护栏**：挂起项**还在** ⇒ ⇒ 释放**没有**顺手把挂起项清掉 ⇒ ⇒
///   「释放 ≠ 清 pending」（裁决 C）。
#[test]
fn the_trap_return_releases_the_serving_flag_without_clearing_the_pending_queue() {
    let mut host = host_with(&NO_CLAIM_FIRMWARE);

    step(&mut host, SETUP_STEPS);
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        0,
        "前置：handler 计数器在 setup 之后必须还是 0"
    );

    host.uart_receive(b'A');
    host.sync_interrupts_now();
    assert!(
        is_raised(&intc_state(&host), LINE_EXTERNAL),
        "前置：第一轮必须已经抬起了外部线"
    );

    // ---- 判别式：同一条线必须能**再交付一次** ----
    run_until_counter(
        &mut host,
        2,
        "「服务中」标记必须在 `mret` 时被释放（同一条线要能再交付一次）—— \
         修复前它永不复位 ⇒ ⇒ 单级嵌套护栏把第二次交付永久挡住",
    );

    // ---- 护栏：挂起项**还在**（guest 一直没解除源）----
    let state = intc_state(&host);
    assert!(
        is_pending(&state, LINE_EXTERNAL),
        "释放「服务中」标记**不许**顺手清掉挂起项（guest 一直没解除源）：{:?}",
        state.pending
    );
}

/// **A-13**：架构的 `mip` 挂起位**只**由 guest 的动作清 —— `mret` **不清**它。
///
/// ⚠️ **这是用户明确纠正的那条架构语义**：`mret` 只做「trap 返回 + 恢复
/// 特权模式 / 中断使能」⇒ ⇒ **清 pending 是软件在 ISR 里写外设 / PLIC /
/// CLINT 的动作**（那是 guest 的职责）⇒ ⇒ 本批**不做**任何「`mret` 时清
/// 架构 pending」的事。
///
/// ⇒ ⇒ f3 那段固件**什么都不解除** ⇒ ⇒ handler 已经 `mret` 过 ⇒ ⇒ 架构的
/// `mip` 里的 **MEIP 位必须仍然置上**。
#[test]
fn the_trap_return_does_not_clear_the_architectural_pending_bit() {
    let mut host = host_with(&NO_CLAIM_FIRMWARE);

    step(&mut host, SETUP_STEPS);
    host.uart_receive(b'A');
    host.sync_interrupts_now();

    run_until_counter(
        &mut host,
        1,
        "外部中断必须真的交付一次（handler 一次也没跑 ⇒ 后面全是真空断言）",
    );

    let mip = csr(&host, CSR_MIP);
    assert!(
        mip & MIP_MEIP_MASK != 0,
        "`mret` **不许**清架构的挂起位（清 pending 是 guest 自己写外设的动作）—— \
         mip = {mip:#010x}"
    );
}

/// **A-14**：**一次**送**两个**字节 ⇒ ⇒ handler 计数器**至少 2**。
///
/// ⚠️ **这一条是前端真实形状的判别式**：前端的「原文 + 行结束符」是
/// **一次**把整行（含 CR）送进去 ⇒ ⇒ 两个字节**同时**在 FIFO 里 ⇒ ⇒
/// guest 的 handler **一次只读一个字节**（osz 就是这样）⇒ ⇒ 必须**两次**
/// 交付才排得空。
///
/// ⚠️ **修复前必然红**：第一次交付之后「服务中」标记**永远**不复位 ⇒ ⇒
/// 第二次交付被单级嵌套护栏**永久**挡住 ⇒ ⇒ 计数器**停在 1**。
#[test]
fn the_guest_handles_two_bytes_delivered_in_one_go() {
    let mut host = host_with(&UART_FIRMWARE);

    step(&mut host, SETUP_STEPS);
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        0,
        "前置：handler 计数器在 setup 之后必须还是 0"
    );

    // ---- 一次送两个字节（前端「原文 + CR」的形状）----
    host.uart_receive(b'A');
    host.uart_receive(b'B');
    host.sync_interrupts_now();
    assert!(
        is_raised(&intc_state(&host), LINE_EXTERNAL),
        "前置：第一轮必须已经抬起了外部线"
    );

    run_until_counter(
        &mut host,
        2,
        "一次送两个字节 ⇒ 必须**两次**交付（guest 的 handler 一次只读一个字节）",
    );

    // ---- FIFO 必须被排空（两个字节都被 guest 读走了）----
    assert!(
        !uart_rx_valid(&host),
        "两个字节必须都被 guest 读走（RX FIFO 仍非空 ⇒ 交付次数不够）"
    );
}

/// **A-15**：源被**撤销**之后 ⇒ ⇒ **不再**有新的交付（计数器**停住**）。
///
/// ⚠️ **这一条是「收线」那一半**：A-11 证明「线被撤掉」，这一条证明「撤掉
/// 之后**真的不再交付**」⇒ ⇒ 否则「撤线」只是一次**状态位**的变化。
///
/// ⚠️ **修复前必然红**：源挂起位被**锁存** ⇒ ⇒ 外部线**永远**被请求 ⇒ ⇒
/// guest **一直**重新陷入 ⇒ ⇒ 计数器**一直**涨 ⇒ ⇒ 停不住。
#[test]
fn the_delivery_stops_once_the_source_has_been_withdrawn() {
    let mut host = host_with(&DRAIN_ONLY_FIRMWARE);

    step(&mut host, SETUP_STEPS);
    host.uart_receive(b'A');
    host.sync_interrupts_now();

    // ---- 跑到 handler 真的跑过（它读 RHR ⇒ FIFO 被排空 ⇒ 源该被撤销）----
    run_until_counter(
        &mut host,
        1,
        "外部中断必须真的交付一次（handler 一次也没跑 ⇒ 后面全是真空断言）",
    );

    // ---- 前置：FIFO 必须已经被排空（否则「停住」是假的）----
    assert!(
        !uart_rx_valid(&host),
        "前置：handler 必须已经把 RX FIFO 排空"
    );

    // ---- 再跑几批 ⇒ 计数器必须**停住**----
    let before = host.registers()[HANDLER_COUNTER_REG];
    for _ in 0..3 {
        step(&mut host, TRAP_STEPS);
    }
    assert_eq!(
        host.registers()[HANDLER_COUNTER_REG],
        before,
        "源被撤销之后 ⇒ 必须**不再**有新的交付（计数器停住）—— \
         修复前源挂起位被锁存 ⇒ ⇒ 外部线永远被请求 ⇒ ⇒ guest 一直重新陷入"
    );
}
