//! D-82 的 **guest 指令级**端到端判别力（v0.27 · STAGE-12 §4 · 裁决 H）。
//!
//! ## 这条用例补的是哪一段
//!
//! D-82 的台账原文说「CLINT 的固件侧 MMIO 通路推迟到 P1」—— **实测结论
//! 是台账过时了**：CLINT 早已挂在总线上，M-14 的 `PeripheralBound` 派发
//! 路径本来就是通用的，契约层的 kind 值域也不用改。
//!
//! ⇒ **真实剩余缺口不是「通路不存在」，而是「通路没有 guest 指令级的
//! 端到端判别力」**：
//!
//! - 宿主侧那条 `the_clint_mtimecmp_reaches_the_interrupt_controller`
//!   （`kernel_host/tests_osz_wiring.rs`）是**宿主直接写总线**
//!   （`host.bus_mut().write(...)`）⇒ 它证明「写总线 → CLINT 存下 → 同步
//!   → M-16 拿到比较值」；
//! - M-16 侧有单测证明「电平语义」。
//!
//! ⇒ **两端各测一半** ⇒ 中间那一段（**guest 指令写 → 定时器线挂起 →
//! 交付 → 陷入**）**没有一条用例**。⇒ **本文件就是那一段。**
//!
//! ## 夹具：一段**汇编器产出**的机器码
//!
//! 固件是 `riscv-none-elf-as -march=rv32im_zicsr -mabi=ilp32` 产出、
//! 再用 `objdump` 逐条核对、`objcopy` 逐字节核对过的字节（小端，16 条 ×
//! 4 字节 = 64 字节）。⚠️ **工具链在仓外**（`~/riscv-sim-golden/
//! toolchain/bin`），**不进仓库** ⇒ 字节与源码之间**没有自动一致性
//! 检查** ⇒ [`the_firmware_bytes_are_still_the_ones_the_source_file_describes`]
//! 是**人可复核**的那一道防线。
//!
//! ```text
//! 0x00 lui   x5, 0x84000      ; x5 = 0x8400_0000（固件基址 = QEMU virt 高段 RAM）
//! 0x04 addi  x5, x5, 0x28     ; x5 = 0x8400_0028（handler 的绝对地址）
//! 0x08 lui   x6, 0x2004       ; x6 = 0x0200_4000（CLINT 的 mtimecmp 低字）
//! 0x0c li    x7, 1            ; x7 = 1（**一个** CLINT 周期）
//! 0x10 sw    x7, 0(x6)        ; 写 mtimecmp 低 32 位（高半保持 0 ⇒ 值 = 1 周期）
//! 0x14 li    x8, 0x80         ; x8 = 1 << 7 = **MTIE 位号 7**
//! 0x18 csrrw x0, mie, x8      ; mie 使能定时器线（内部线 3）
//! 0x1c csrrsi x0, mstatus, 8  ; mstatus.MIE（bit 3）= 1
//! 0x20 csrrw x0, mtvec, x5    ; mtvec = handler（直接模式）
//! 0x24 jal   x0, 0x24         ; loop —— 自旋，等中断
//! 0x28 csrr  x9,  mcause      ; handler：x9 = guest 看到的 mcause
//! 0x2c csrr  x10, mepc        ; handler：x10 = guest 看到的 mepc
//! 0x30 csrr  x11, mtval       ; handler：x11 = guest 看到的 mtval
//! 0x34 addi  x12, x12, 1      ; handler：计数器（handler 跑了几遍）
//! 0x38 csrrwi x0, 0x7c0, 8    ; handler：清 line 3（**位掩码 bit 3 = 8**）
//! 0x3c mret                   ; handler：回到 mepc
//! ```
//!
//! ## ⚠️ `0x08` 那条：`lui` 的立即数是「**地址右移 12 位**」
//!
//! ⚠️ 施工期实测挖出的**第三条固件缺陷**（规格已按**第七次订正**
//! `fb22ea2` 改好，本文件用的是**订正后**的字节）：
//!
//! - 订正前的字节表写 `0x08 lui x6, 0x20040` / `0x20040337`，注释说
//!   「`x6 = 0x0200_4000` = CLINT 的 `mtimecmp`」⇒ ⚠️ **那个立即数错了
//!   一位十六进制数字**；
//! - RISC-V 的 `LUI` 把 20 位立即数**左移 12 位** ⇒ `0x20040` 落地是
//!   **`0x2004_0000`**，**不是** `0x0200_4000` ⇒ **写 `lui` 之前要先
//!   算「地址右移 12 位」，不许拿看着像地址的一段直接填**；
//! - ⇒ **后果**：guest 的 `sw x7, 0(x6)` 写到 `0x2004_0000` ⇒ 那个地址
//!   在 `qemu-virt` 描述里**没有区域** ⇒ guest 得到 **store access
//!   fault**（`mcause = 7`）⇒ 而且那次陷阱发生时 `mtvec` **还没设**
//!   （`mtvec` 在第 9 条指令才写、`store` 在第 5 条）⇒ 陷阱跳到
//!   `mtvec = 0` ⇒ 落进低段 RAM 的全零区 ⇒ 执行非法指令 ⇒ 再次陷入 ⇒
//!   **无限陷阱循环** ⇒ 最后「陷入」确实发生了，但 `mcause` 是 store
//!   fault 的 `7`，**不是**定时器中断的 `0x8000_0007`；
//! - ⇒ **修法**：立即数改成 **`0x2004`** ⇒ `0x2004 << 12 = 0x0200_4000`
//!   = CLINT 基址（`0x0200_0000`）+ `mtimecmp` 偏移（`0x4000`）⇒ 机器码
//!   `0x02004337`（小端 `37 43 00 02`）。
//!
//! ⇒ **实测证据**（仓外交叉工具链，`objdump` 逐条核对 —— 反汇编行自证
//! 目标地址）：
//!
//! ```text
//! 00000000 <_start>:
//!    0: 20040337    lui  t1,0x20040      ← x1 = 0x2004_0000  ✘
//!    4: 020043b7    lui  t2,0x2004       ← x2 = 0x0200_4000  ✔
//! ```
//!
//! ⇒ 其余 **15** 条指令与订正后的字节表**逐字节相同**。
//!
//! ### 三个必须写进注释的坑
//!
//! ① **`handler` 在偏移 `0x28`** ⇒ `addi x5, x5, 0x28` 里的常量**与固件
//!    布局耦合**（`loop` 之前正好 10 条指令 × 4 字节）⇒ **改固件条数必须
//!    同步改这个常量**。
//!
//! ② **`irq_clear` 的值是「位掩码」，不是「线号」**（M-17
//!    `clear_lines_by_mask`：判据是 `mask & (1 << line) != 0`）⇒ 定时器
//!    **线 3** ⇒ 写 **`8`**（`1 << 3`），**不是 3**。⚠️ 对照：v0.24 那条
//!    UART 用例写的是 `2`（清线 1）⇒ 两个值**长得像**但含义不同。
//!
//! ③ **`csrrwi` 的立即数只有 5 位** ⇒ `8`（`1 << 3`）放得进，但
//!    `1 << 7` = 128 **放不进** ⇒ **清线 7 用 `csrrwi` 做不到**。
//!
//! ### ⚠️ `0x38` 那条的 CSR 号是 `0x7c0`（**不是 `0x7c1`**）
//!
//! 契约层的 `irq_clear` 是 `CSR_IRQ_CLEAR = 0x7C0`；`0x7C1` **不在**
//! `csr_is_implemented` 的名单里 ⇒ 按 `csr_write_is_ignored` ⇒ **guest
//! 写 `0x7C1` 被静默忽略** ⇒ **定时器线永不清** ⇒ handler 里的 `mret`
//! 落回 `mepc` 后**立刻再次陷入** ⇒ 而「handler 计数器 ≥ 1」**照样能过**
//! ⇒ **用例看起来绿，其实根本没测到「清线」这条通路** ⇒ 这正是「假绿」
//! 的定义。⇒ 判别式落在**控制器的导出状态**上（线 3 有没有被清掉）。
//!
//! ## ⚠️ 为什么是**两条** `StepN`
//!
//! guest 在**批次内**写下的 `mtimecmp`，要等批次边界的 `sync_interrupts`
//! 才被喂进 M-16 ⇒ **本批次内**定时器不会来 ⇒ **一条 `StepN` 不够**：
//!
//! - **第 1 条**：跑够写 `mtimecmp` + 使能 `mie` + `mstatus.MIE` +
//!   `mtvec`，然后进自旋 ⇒ 跑完时 `sync_interrupts` 把 `mtimecmp` 喂进
//!   M-16；
//! - **第 2 条**：跑 ≥ 2 个量子 ⇒ 期间 `poll()` 在量子边界看到
//!   `now_ps >= mtimecmp_ps` ⇒ 挂起线 3 ⇒ 拉取式交付 ⇒ `enter_trap` ⇒
//!   guest 陷入。
//!
//! ⚠️ **别把这两条合并成一条** —— 合并后用例会变红，而**变红的原因不在
//! 被测行为**（比较值还没进 M-16），很容易被误判成「平台坏了」。
//!
//! ## 时序（数字全部来自实现，不是手算）
//!
//! | 事实 | 数值 | 出处 |
//! | --- | --- | --- |
//! | 每条指令推进的虚拟时间 | `INSTRUCTION_COST_PS` = **1 000 ps** | `m12_engine/mod.rs` |
//! | 时间量子 | `TIME_QUANTUM_PS` = **1 µs** = **1 000 条指令** | `m12_engine/mod.rs` |
//! | M-16 的 `poll()` 采样点 | **量子边界**（每 1 000 条指令一次） | `m17_backend/mod.rs` |
//! | CLINT 的频率 | 10 MHz ⇒ **1 周期 = 100 000 ps = 100 ns** | `clint.rs`（`PS_PER_SEC / freq_hz`）|
//! | `mtimecmp = 1` 周期 ⇒ M-16 的比较值 | **100 000 ps** | `clint.mtimecmp_ps()` |
//!
//! ⇒ 推进 100 000 ps 只需 100 条指令，但 **`poll()` 只在量子边界采样**
//! ⇒ **实际至少要跨过 1 个量子** ⇒ 第 2 条跑 **2 000** 条（2 个量子，
//! 给足余量）即可。

use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MEPC, CSR_MTVAL, CSR_MTVEC};
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use riscv_sim_kernel::kernel_host::platform;
use riscv_sim_kernel::kernel_host::KernelHost;

// ---------------------------------------------------------------------------
// 夹具：固件字节（汇编器产出 + `objdump` 逐条核对 + `objcopy` 逐字节核对）
// ---------------------------------------------------------------------------

/// D-82 的固件机器码（小端，16 条 × 4 字节 = 64 字节）。
///
/// ⚠️ **来源在仓外**：`riscv-none-elf-as -march=rv32im_zicsr -mabi=ilp32`
/// ⇒ `objdump -d` 逐条核对 ⇒ `objcopy -O binary --only-section=.text`。
/// ⚠️ **必须带 `zicsr`**（否则 `csrr*` 全部报「需要 zicsr 扩展」）。
/// ⚠️ **不要用 `-march=rv32imc`**：压缩指令会让每条指令变成 2 字节 ⇒
/// `handler` 的偏移不再是 `0x28` ⇒ 偏移常量与固件布局耦合。
const D82_FIRMWARE: [u8; 64] = [
    // 0x00 lui   x5, 0x84000
    0xB7, 0x02, 0x00, 0x84, //
    // 0x04 addi  x5, x5, 0x28
    0x93, 0x82, 0x82, 0x02, //
    // 0x08 lui   x6, 0x2004
    //
    // ⚠️ **立即数必须是 `0x2004`**（不是 `0x20040`）：`lui` 的立即数是
    // 20 位并**左移 12 位** ⇒ `0x20040` 落地是未映射的 `0x2004_0000`
    // ⇒ guest 得到 store access fault ⇒ 根本到不了定时器（见文件头的
    // 说明）。⇒ `0x2004 << 12 = 0x0200_4000` = CLINT 基址 + `mtimecmp`
    // 偏移 `0x4000`。
    0x37, 0x43, 0x00, 0x02, //
    // 0x0c li    x7, 1
    0x93, 0x03, 0x10, 0x00, //
    // 0x10 sw    x7, 0(x6)
    0x23, 0x20, 0x73, 0x00, //
    // 0x14 li    x8, 0x80
    0x13, 0x04, 0x00, 0x08, //
    // 0x18 csrrw x0, mie, x8
    0x73, 0x10, 0x44, 0x30, //
    // 0x1c csrrsi x0, mstatus, 8
    0x73, 0x60, 0x04, 0x30, //
    // 0x20 csrrw x0, mtvec, x5
    0x73, 0x90, 0x52, 0x30, //
    // 0x24 jal   x0, 0x24   （loop）
    0x6F, 0x00, 0x00, 0x00, //
    // 0x28 csrr  x9, mcause  （handler 入口）
    0xF3, 0x24, 0x20, 0x34, //
    // 0x2c csrr  x10, mepc
    0x73, 0x25, 0x10, 0x34, //
    // 0x30 csrr  x11, mtval
    0xF3, 0x25, 0x30, 0x34, //
    // 0x34 addi  x12, x12, 1
    0x13, 0x06, 0x16, 0x00, //
    // 0x38 csrrwi x0, 0x7c0, 8   ← **CSR 号是 0x7c0，不是 0x7c1**
    0x73, 0x50, 0x04, 0x7C, //
    // 0x3c mret
    0x73, 0x00, 0x20, 0x30, //
];

/// 固件的**落址**（QEMU virt 档高段 RAM 内）。
///
/// ⚠️ **为什么必须是这个地址**：宿主的内存图档位是按镜像**用到的地址**
/// 挑的（裁决 E）⇒ 落在这里才会切到 **`qemu-virt`** 档 ⇒ **CLINT 才
/// 存在**（默认档没有 CLINT / PLIC / UART）。
const FW_BASE: u32 = 0x8400_0000;

/// 自旋指令（`loop`）的**绝对地址** —— 陷阱时 `mepc` 应当落在它上面。
///
/// ⚠️ **写字面量，不写 `FW_BASE + 0x24`**（规格 §4.4 的明令）：写成
/// 从常量反推的形式 ⇒ 改固件布局时断言会**跟着变** ⇒ 变成恒绿。
const SPIN_ABS: u32 = 0x8400_0024;

/// handler 的**绝对地址**（固件把它装进 `mtvec`）。
const HANDLER_ABS: u32 = 0x8400_0028;

/// 平台的**内部中断线号** —— 定时器比较（`IRQ_TIMER_COMPARE`）。
///
/// ⚠️ **内部线号 3** 与**架构 cause 码 7** 是**两个不同的空间**（见下面
/// `MCAUSE_TIMER`）⇒ 断言 `mcause` 时只能用 **7**，写 3 就是错的。
const LINE_TIMER: u16 = 3;

/// guest 看到的 `mcause` —— **架构字面量**。
///
/// = `IRQ_BASE`（0x8000_0000，中断位）| `CAUSE_MACHINE_TIMER`（7，机器
/// 定时器中断）。
///
/// ⚠️ **不许从实现反推**（不写 `arch_mcause_for_line(LINE_TIMER)`）——
/// 那会让用例跟实现同源 ⇒ 改实现时跟着变 ⇒ 变成恒绿。
const MCAUSE_TIMER: u32 = 0x8000_0007;

/// 一个 CLINT 周期折合多少 ps —— **`PS_PER_SEC / freq_hz`**。
///
/// ⚠️ **不许手算**：照抄 `clint.rs` 里那一行的算式（`PS_PER_SEC` 是
/// `1e12` ps/s，`freq_hz` 取自 `qemu-virt` 描述里 CLINT 的器件参数）。
const PS_PER_SEC: u64 = 1_000_000_000_000;

/// 第 1 批的条数 —— 跑够**写 `mtimecmp` + 使能 `mie` + `mstatus.MIE` +
/// `mtvec`**，然后进自旋。
///
/// 固件的前 **10** 条指令就是这些（到 `loop` 为止 0x24 字节）⇒ 100 条
/// **足够**（多出来的 90 条全在自旋里）。
const SETUP_STEPS: u32 = 100;

/// 第 2 批的条数 —— **≥ 2 个量子**（1 000 条 / 量子）。
///
/// ⚠️ **为什么 2 000 够**：`mtimecmp = 1` 周期 ⇒ 比较值 = 100 000 ps；
/// 虚拟时间每条指令推进 1 000 ps ⇒ 100 条就够到 100 000 ps，但
/// **`poll()` 只在量子边界采样**（每 1 000 条一次）⇒ **实际至少要跨过
/// 1 个量子** ⇒ 2 000 条（2 个量子）给足余量。
const TRAP_STEPS: u32 = 2_000;

/// handler 的**计数器**所在的通用寄存器（`x12`）。
const HANDLER_COUNTER_REG: usize = 12;

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

/// CLINT 的**频率**（Hz）—— 取自描述里 CLINT 的器件参数。
fn clint_freq_hz() -> u64 {
    let description = qemu_virt_description();
    description
        .regions
        .iter()
        .filter_map(|region| match region {
            RegionSpec::Peripheral { device, .. } => match device {
                DeviceSpec::Clint { id, freq_hz, .. } if id == "CLINT" => Some(*freq_hz),
                _ => None,
            },
            _ => None,
        })
        .next()
        .expect("qemu-virt 描述里必须有 CLINT")
}

/// 一个 CLINT 周期折合多少 ps —— **`PS_PER_SEC / freq_hz`**。
fn ps_per_cycle() -> u64 {
    PS_PER_SEC / clint_freq_hz()
}

/// CLINT 的 `mtimecmp` **低字**地址 —— **基址来自描述** + 偏移 `0x4000`。
fn clint_mtimecmp_lo() -> u32 {
    device_base("CLINT") + 0x4000
}

// ---------------------------------------------------------------------------
// 夹具：宿主
// ---------------------------------------------------------------------------

/// 造一个最小的 ELF32（RV32 / 小端 / ET_EXEC），只有一个 PT_LOAD。
///
/// ⚠️ **为什么要自己造**：真实 RTOS 的 ELF 在**仓外**（不进仓库），而
/// 「guest 指令级的 MMIO 通路」必须有一条**在仓内**的用例钉住 ⇒ 用一个
/// 手造的、只有 16 条指令的 ELF 来驱动它。
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
fn d82_project() -> Project {
    Project {
        id: "proj-d82".to_string(),
        name: "d82".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "d82.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(minimal_elf(FW_BASE, &D82_FIRMWARE)),
        }],
        version: ProjectVersion::default(),
    }
}

/// 装载固件并返回宿主。
///
/// ⚠️ **落址决定档位**：固件落在 `0x8400_0000` ⇒ 宿主自动切到
/// `qemu-virt` 档 ⇒ **CLINT 存在** ⇒ 定时器通路存在。
fn host_with_d82_firmware() -> KernelHost {
    let mut host = KernelHost::new();
    let events = host.handle_message(&Message::command(
        1,
        CommandPayload::LoadProject(d82_project()),
    ));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载 D-82 固件必须成功：{events:?}"
    );
    assert_eq!(
        host.platform_id(),
        platform::PLATFORM_ID_QEMU_VIRT,
        "固件落在 0x8400_0000 ⇒ 必须自动切到 qemu-virt 档（只有它有 CLINT）"
    );
    host
}

/// 发一条命令并拿回事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 跑 `n` 条指令（**不许**出错）。
fn step(host: &mut KernelHost, n: u32) {
    let events = dispatch(host, CommandPayload::StepN { n });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "步进 {n} 条必须成功：{events:?}"
    );
}

/// 跑完**两条** `StepN`（为什么是两条，见文件头的说明）。
fn run_to_trap(host: &mut KernelHost) {
    // 第 1 条：写 `mtimecmp` + 使能 + 装 `mtvec`，然后进自旋。
    // 跑完时批次的 `sync_interrupts` 把 `mtimecmp` 喂进 M-16。
    step(host, SETUP_STEPS);
    // 第 2 条：跨过量子边界 ⇒ `poll()` 看到比较值 ⇒ 挂起线 3 ⇒ 交付 ⇒ 陷入。
    step(host, TRAP_STEPS);
}

/// CSR 文件里某个 CSR 的值（读不到 = 0）。
///
/// ⚠️ **这个助手对「文件里没有的 CSR」也返回 0** ⇒ 单独用它断言
/// 「某个 CSR 的值是 0」是**真空断言** ⇒ 要断言「值是 0」就得**同时**
/// 断言「这条 CSR **在文件里**」——见 [`csr_is_present`]。
fn csr(host: &KernelHost, number: u16) -> u32 {
    host.engine_snapshot()
        .state
        .csrs
        .iter()
        .find(|(n, _)| *n == number)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 某个 CSR **在 CSR 文件里**（不是「它的值是 0」）。
fn csr_is_present(host: &KernelHost, number: u16) -> bool {
    host.engine_snapshot()
        .state
        .csrs
        .iter()
        .any(|(n, _)| *n == number)
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

/// **夹具防漂移**：固件字节的**每一条关键指令**必须还是「我们以为的那
/// 些指令」。
///
/// ⚠️ 仓库里**没有** RISC-V 汇编器（交叉工具链在**仓库外**）⇒ 字节与
/// 汇编源之间**没有自动一致性检查** ⇒ 这条用例是**人可复核**的那一道：
/// 它把每条关键指令的编码写死，字节被改动就会红。
#[test]
fn the_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x00 lui x5, 0x84000（固件基址）
    assert_eq!(&D82_FIRMWARE[0x00..0x04], &[0xB7, 0x02, 0x00, 0x84]);
    // 0x04 addi x5, x5, 0x28（handler 的地址 —— **与固件布局耦合**）
    assert_eq!(&D82_FIRMWARE[0x04..0x08], &[0x93, 0x82, 0x82, 0x02]);
    // 0x08 lui x6, 0x2004（**不是** 0x20040 —— `lui` 的立即数是「地址
    // 右移 12 位」；0x20040 会落地成未映射的 0x2004_0000 ⇒ store fault）
    assert_eq!(&D82_FIRMWARE[0x08..0x0c], &[0x37, 0x43, 0x00, 0x02]);
    // 0x0c li x7, 1（**一个** CLINT 周期 —— 必须是 1，不是 0）
    assert_eq!(&D82_FIRMWARE[0x0c..0x10], &[0x93, 0x03, 0x10, 0x00]);
    // 0x10 sw x7, 0(x6)（写 mtimecmp）
    assert_eq!(&D82_FIRMWARE[0x10..0x14], &[0x23, 0x20, 0x73, 0x00]);
    // 0x14 li x8, 0x80（= 1 << 7 = **MTIE 位号 7**）
    assert_eq!(&D82_FIRMWARE[0x14..0x18], &[0x13, 0x04, 0x00, 0x08]);
    // 0x18 csrrw x0, mie, x8
    assert_eq!(&D82_FIRMWARE[0x18..0x1c], &[0x73, 0x10, 0x44, 0x30]);
    // 0x1c csrrsi x0, mstatus, 8（mstatus.MIE = bit 3）
    assert_eq!(&D82_FIRMWARE[0x1c..0x20], &[0x73, 0x60, 0x04, 0x30]);
    // 0x20 csrrw x0, mtvec, x5
    assert_eq!(&D82_FIRMWARE[0x20..0x24], &[0x73, 0x90, 0x52, 0x30]);
    // 0x24 jal x0, 0x24（loop —— `mepc` 应当落在它上面）
    assert_eq!(&D82_FIRMWARE[0x24..0x28], &[0x6F, 0x00, 0x00, 0x00]);
    // 0x28 csrr x9, mcause（**handler 入口**）
    assert_eq!(&D82_FIRMWARE[0x28..0x2c], &[0xF3, 0x24, 0x20, 0x34]);
    // 0x2c csrr x10, mepc
    assert_eq!(&D82_FIRMWARE[0x2c..0x30], &[0x73, 0x25, 0x10, 0x34]);
    // 0x30 csrr x11, mtval
    assert_eq!(&D82_FIRMWARE[0x30..0x34], &[0xF3, 0x25, 0x30, 0x34]);
    // 0x34 addi x12, x12, 1（handler 计数器）
    assert_eq!(&D82_FIRMWARE[0x34..0x38], &[0x13, 0x06, 0x16, 0x00]);
    // 0x38 csrrwi x0, 0x7c0, 8（irq_clear —— **CSR 号必须是 0x7c0**）
    //
    // ⚠️ 这一条是「CSR 号错一位 = 假绿」那道缺陷的**护栏**：写 `0x7c1`
    // ⇒ 被 `csr_write_is_ignored` **静默忽略** ⇒ 定时器线永不清 ⇒ 而
    // 「handler 计数器 ≥ 1」**照样能过** ⇒ 用例看起来绿、实际没测到清线。
    assert_eq!(&D82_FIRMWARE[0x38..0x3c], &[0x73, 0x50, 0x04, 0x7C]);
    // 0x3c mret
    assert_eq!(&D82_FIRMWARE[0x3c..0x40], &[0x73, 0x00, 0x20, 0x30]);
    // 固件总长 64 字节（0x40）
    assert_eq!(D82_FIRMWARE.len(), 64, "固件长度变了");
}

/// **夹具防漂移**：固件里**写死**的 CLINT 地址必须与**描述**对得上。
///
/// ⚠️ 固件的 `lui x6, 0x20040` 是**机器码里的常量** ⇒ 描述一旦改了
/// CLINT 的基址，这条固件就会写到**别的地址**上 ⇒ 而「写到别的地址」
/// 表现得像「定时器通路坏了」⇒ 极难定位。⇒ 这条用例把两者的对应关系
/// **钉住**：描述里的 `CLINT` 基址 + `mtimecmp` 偏移 `0x4000` 必须等于
/// 固件写死的 `0x0200_4000`。
#[test]
fn the_firmware_hardcoded_clint_address_matches_the_description() {
    assert_eq!(
        clint_mtimecmp_lo(),
        0x0200_4000,
        "固件写死的 mtimecmp 地址（lui x6, 0x20040 ⇒ 0x0200_4000）必须等于 \
         描述里的 CLINT 基址 + mtimecmp 偏移 0x4000 —— 描述改了这条就红"
    );
    // 频率也对上（1 周期 = 100 000 ps = 100 ns）。
    assert_eq!(
        clint_freq_hz(),
        10_000_000,
        "CLINT 的频率必须与固件假设的 10 MHz 一致"
    );
}

/// **C-1**：guest 指令写 `mtimecmp` + 使能 MTIE ⇒ **真的陷入**。
///
/// 判别式落在**架构字面量**上：
/// - `mcause == 0x8000_0007`（中断位 | **架构 cause 码 7** = 机器定时器
///   中断；⚠️ **不是**内部线号 3）；
/// - `mepc == 0x8400_0024`（自旋指令的**绝对地址**，**字面量**）。
#[test]
fn the_guest_traps_with_the_machine_timer_cause() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    assert_eq!(
        csr(&host, CSR_MCAUSE),
        MCAUSE_TIMER,
        "mcause 必须是「中断位 | **架构 cause 码 7**」（机器定时器中断），\
         不是内部线号 3"
    );
    assert_eq!(
        csr(&host, CSR_MEPC),
        SPIN_ABS,
        "mepc 必须落在自旋指令（jal x0, 0x24）的绝对地址上"
    );
    // 顺带钉住「guest 真的装了向量表」——否则「handler 跑过了」这件事
    // 可能来自别的路径。
    assert_eq!(
        csr(&host, CSR_MTVEC),
        HANDLER_ABS,
        "mtvec 必须是 guest 自己写的那张向量表（handler 的绝对地址）"
    );
}

/// **C-3**：陷入时 `mtval == 0`（定时器中断没有错因值）。
///
/// ⚠️ **同时**断言 `mtval` **在 CSR 文件里** ⇒ 否则「读不到就是 0」会让
/// 这条变成**真空断言**。
#[test]
fn the_guest_sees_no_fault_address_on_a_timer_interrupt() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    assert!(
        csr_is_present(&host, CSR_MTVAL),
        "中断路径必须**真的写**过 mtval（不是「读不到就是 0」的那种假绿）"
    );
    assert_eq!(
        csr(&host, CSR_MTVAL),
        0,
        "中断没有故障地址 ⇒ mtval 必须是 0"
    );
}

/// **判别式 4**：handler 的计数器（`x12`）≥ 1 ⇒ 真的进了 handler。
///
/// ⚠️ **写死「≥ 1」，不写「== 1」**：定时器是**电平**语义 ⇒ 清掉线之后
/// 比较值**仍然**有效、而虚拟时间**只增不减** ⇒ **下一个量子边界又会
/// 请求线 3** ⇒ **反复陷入是平台的正常行为** ⇒ 「陷入恰好一次」不成立
/// ⇒ 拿它当判据会**恒红**。
///
/// ⚠️ 这条判据只证明「**确实进了 handler**」（不是停在别处）⇒ 它与
/// 「清线有没有生效」是**两件独立的事** ⇒ 「清线」由
/// [`the_handler_clears_the_timer_line_in_the_interrupt_controller`]
/// 单独负责 ⇒ **两条判据各管一件事，不互相顶替**。
#[test]
fn the_handler_runs_at_least_once() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    let counter = host.registers()[HANDLER_COUNTER_REG];
    assert!(
        counter >= 1,
        "handler 必须至少跑过一次（x12 = {counter}）—— 没跑过说明 guest 根本没陷入"
    );
}

/// **比较值真的被喂进了 M-16**（不只是「guest 写总线这条路通」）。
///
/// ⚠️ 这一条比「去掉 `sw x7, 0(x6)`」**更严格**：`mtimecmp == 0` 在
/// CLINT 侧是「无期限」、在 M-16 侧是「**定时器关闭**」⇒ 定时器**永不
/// 到期** ⇒ ⇒ 它证明「**比较值真的进了 M-16**」。
#[test]
fn the_guest_written_compare_value_reaches_the_interrupt_controller() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    // ---- 前半段：guest 的**指令**写进了 CLINT ----
    //
    // ⚠️ 走的是**公开的调试读面**（`bus().read`）⇒ 读的是**器件自己的**
    // 状态，不是宿主缓存。
    let mtimecmp_lo = host
        .read_memory(clint_mtimecmp_lo(), WriteSize::Word)
        .expect("读 CLINT 的 mtimecmp 低字必须成功");
    assert_eq!(
        mtimecmp_lo, 1,
        "guest 的 `sw x7, 0(x6)` 必须把 1（**一个** CLINT 周期）写进 CLINT"
    );

    // ---- 后半段：CLINT 的比较值换算成 ps 喂进了 M-16 ----
    //
    // ⚠️ 期望值**照抄实现的算式**：`PS_PER_SEC / freq_hz`（不许手算）。
    assert_eq!(
        host.intc_timer_compare_ps(),
        ps_per_cycle(),
        "M-16 的比较值必须是 `PS_PER_SEC / freq_hz` = {} ps（一个 CLINT 周期）",
        ps_per_cycle()
    );
}

/// **判别式 6**：handler 的 `irq_clear` ⇒ **定时器线 3 真的被清掉了**。
///
/// ⚠️ **判据落在控制器的导出状态上**（不是「不抛错」、不是「陷入了几次」）：
/// `host.intc_state()` 是**公开 API** ⇒ `raised` / `pending` / `active_irq`
/// 三个字段都能看。
///
/// ⚠️ **为什么不用「陷入次数」当判据**：定时器是**电平**语义 ⇒ 清掉线
/// 之后比较值**仍然**有效、而虚拟时间**只增不减** ⇒ **下一个量子边界又会
/// 请求线 3** ⇒ **反复陷入是平台的正常行为** ⇒ 「陷入恰好一次」不成立。
///
/// ⚠️ **前置断言（非真空）**：比较值必须**真的进了 M-16** ⇒ 否则「线 3
/// 没被清」与「线 3 **从来没被拉起**」**不可区分** ⇒ 这条断言会真空
/// （例如把 `mtimecmp` 写成 0 时它照样是绿的）。
#[test]
fn the_handler_clears_the_timer_line_in_the_interrupt_controller() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    // ---- 前置：定时器**真的被武装过**（否则下面三条断言是真空的）----
    assert_ne!(
        host.intc_timer_compare_ps(),
        0,
        "比较值必须是 0 与非 0 之外的确定值（0 = 定时器关闭）—— \
         定时器没被武装 ⇒ 「线 3 没被清」与「线 3 从没被拉起」不可区分"
    );

    let state = host.intc_state();
    assert!(
        !state.raised.contains(&LINE_TIMER),
        "handler 清过线 ⇒ `raised` 里不许再有定时器线 {LINE_TIMER}：{:?}",
        state.raised
    );
    assert!(
        state.pending.iter().all(|p| p.line != LINE_TIMER),
        "handler 清过线 ⇒ 挂起队列里不许再有定时器线 {LINE_TIMER}：{:?}",
        state.pending
    );
    assert_eq!(
        state.active_irq,
        riscv_sim_contract::csr::NO_ACTIVE_IRQ,
        "handler 清过线 ⇒ `active_irq` 必须复位（否则这条线被单级防护永久堵死）"
    );
    assert_eq!(
        host.intc_delegation_failures(),
        0,
        "中断委派不许失败（失败时 intc_state 返回的是默认值 —— 正好等于断言值）"
    );
}

/// **`mret` 之后回到 `mepc`**（自旋那条），不是乱跳。
#[test]
fn the_handler_returns_to_the_spin_instruction() {
    let mut host = host_with_d82_firmware();
    run_to_trap(&mut host);

    assert_eq!(host.pc(), SPIN_ABS, "mret 之后必须回到 mepc（自旋那条）");
}
