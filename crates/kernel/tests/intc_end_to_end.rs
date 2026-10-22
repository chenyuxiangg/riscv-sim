//! 端到端的**中断交付**用例（STAGE-4 §6.4）。
//!
//! 这一组用例把整条链走通：
//!
//! ```text
//! 宿主 raise_irq → M-16 挂起 → 量子边界采样 → 仲裁 → 登记交付
//! → M-13 在取指边界拉取 → enter_trap（写 mepc/mcause/mtval、跳 mtvec）
//! → handler 跑起来 → 写 irq_clear 清线 → mret 回到 mepc
//! ```
//!
//! ## 夹具：一段**手写机器码**
//!
//! 固件是**汇编器产出**的字节（`riscv-none-elf-as -march=rv32i_zicsr`
//! 再用 `objdump` 逐条核对过），**不是手打的编码** —— 手打编码的用例一旦
//! 写错，会**假红**（失败点在夹具，不在被测行为）。
//!
//! ```text
//! 0x00 lui   t0, 0x0            ; t0 = 0
//! 0x04 addi  t0, t0, 0x3c       ; t0 = 0x3c（handler）
//! 0x08 sw    x0, 0x200(x0)      ; 清交付计数
//! 0x0c csrrw x0, mtvec, t0      ; 向量表 = handler
//! 0x10 lui   t1, 0x1
//! 0x14 srli  t1, t1, 1          ; t1 = 0x800（mie 的 MEIE 位，bit 11）
//! 0x18 ori   t1, t1, 0x80       ; t1 = 0x880（再加上 MTIE 位，bit 7）
//! 0x1c csrrw x0, mie, t1
//! 0x20 addi  t2, x0, 8          ; t2 = 8（mstatus 的 MIE 位，bit 3）
//! 0x24 csrrs x0, mstatus, t2
//! 0x28 jal   x0, 0x28           ; 自旋
//! 0x2c..0x38 nop × 4
//! 0x3c handler:
//! 0x3c lw    t4, 0x200(x0)      ; 交付计数 += 1
//! 0x40 addi  t4, t4, 1
//! 0x44 sw    t4, 0x200(x0)
//! 0x48 addi  t3, x0, 2          ; 线 1 的位掩码（bit L = 线 L）
//! 0x4c csrrw x0, 0x7c0, t3      ; irq_clear：清掉线 1
//! 0x50 mret                     ; 回到 mepc
//! ```
//!
//! ⚠️ **`mie` 的位号是 11（MEIE）不是 1**：平台的**中断线号**（1/2/3）与
//! **RISC-V 的 `mie` 位号**（11 / 7）是**两套编号空间**，由契约层的
//! `mie_bit_for_line` 唯一互译（STAGE-4 裁决 D）。把 `mie` 写成 `1`
//! 会**一行都不使能** —— 本用例的固件用 `0x880`（MEIE + MTIE）。
//!
//! ⚠️ **`irq_clear` 的掩码按线号索引**：bit L = 线 L（契约层
//! `CSR_IRQ_CLEAR` 的口径，M-17 `clear_lines_by_mask` 是唯一实现）。
//! 清**线 1** 要写 `0b10` = 2 —— 写 `1` 清的是**线 0**（那条线根本没
//! 被拉起），**handler 会「跑完了但线还在」**，`active_irq` 不复位、
//! 第二次交付被单级嵌套防护挡住。
//!
//! ## 判别式
//!
//! 断言全部落在**可观测的具体值**上：handler 在 `0x200` 留下的**交付计数**
//! （不是「不 panic」、不是「非空」）、CSR 文件里的 `mcause` / `mepc`、
//! 控制器的**导出状态**。固件把「handler 跑了几次」写进内存 —— 那是
//! **端到端**可观测的，不依赖任何内部结构。

use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MEPC, CSR_MTVAL, CSR_MTVEC, NO_ACTIVE_IRQ};
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use riscv_sim_kernel::kernel_host::KernelHost;

/// 端到端固件（汇编器产出 + `objdump` 核对）。
const INTC_FIRMWARE: [u8; 84] = [
    0xB7, 0x02, 0x00, 0x00, 0x93, 0x82, 0xC2, 0x03, 0x23, 0x20, 0x00, 0x20, 0x73, 0x90, 0x52, 0x30,
    0x37, 0x13, 0x00, 0x00, 0x13, 0x53, 0x13, 0x00, 0x13, 0x63, 0x03, 0x08, 0x73, 0x10, 0x43, 0x30,
    0x93, 0x03, 0x80, 0x00, 0x73, 0xA0, 0x03, 0x30, 0x6F, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00,
    0x13, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x83, 0x2E, 0x00, 0x20,
    0x93, 0x8E, 0x1E, 0x00, 0x23, 0x20, 0xD0, 0x21, 0x13, 0x0E, 0x20, 0x00, 0x73, 0x10, 0x0E, 0x7C,
    0x73, 0x00, 0x20, 0x30,
];

/// handler 写交付计数的地址。
const MARKER_ADDR: u32 = 0x200;

/// handler 的地址（固件把它装进 `mtvec`）。
const HANDLER_ADDR: u32 = 0x3C;

/// 自旋指令的地址（陷阱时 `mepc` 应当落在它上面）。
const SPIN_ADDR: u32 = 0x28;

/// 平台的中断线 1（UART rx-ready）。
const LINE_1: u16 = 1;

/// 一个量子 = 1000 条指令（`QUANTUM_INSTR_QUOTA`）。
///
/// ⚠️ 这条常量是**自指**的（引用实现自己的配额）——它**只**用来决定
/// 「跑多少条指令才跨过一个量子」，**判别式落在关系上**
/// （「跨过量子边界之前不许交付 / 之后必须交付」），
/// **不落在绝对值上**。绝对值由 D-07 的用例守着。
const QUANTUM_INSTR_QUOTA: u32 = riscv_sim_kernel::m12_engine::QUANTUM_INSTR_QUOTA;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

fn intc_project() -> Project {
    Project {
        id: "proj-intc".to_string(),
        name: "intc".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "intc-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(INTC_FIRMWARE.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

fn host_with_intc_firmware() -> KernelHost {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::LoadProject(intc_project()));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装载中断固件必须成功：{events:?}"
    );
    host
}

fn step(host: &mut KernelHost, n: u32) {
    let events = dispatch(host, CommandPayload::StepN { n });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "步进 {n} 条必须成功：{events:?}"
    );
}

/// handler 留下的**交付计数**。
fn deliveries(host: &KernelHost) -> u32 {
    host.read_memory(MARKER_ADDR, WriteSize::Word)
        .expect("读交付计数必须成功")
}

/// CSR 文件里某个 CSR 的值（读不到 = 0）。
///
/// ⚠️ **这个助手对「文件里没有的 CSR」也返回 0** ⇒ 单独用它断言
/// 「某个 CSR 的值是 0」是**真空断言**（v0.19 检视轮 V-B · F-3）。
/// 要断言「值是 0」就得**同时**断言「这条 CSR **在文件里**」——
/// 见 [`csr_is_present`]。
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

/// **量子边界之前不交付，跨过之后必须交付**。
///
/// ⚠️ 判别式是**关系**（「边界前 0 次 / 边界后 1 次」），不是绝对值。
#[test]
fn an_interrupt_is_delivered_only_after_the_first_quantum_boundary() {
    let mut host = host_with_intc_firmware();

    // 先把使能写好（固件自己的前几条指令），再请求中断。
    step(&mut host, 16);
    host.raise_irq(LINE_1).expect("拉起中断必须成功");

    // ⚠️ 「边界前」这一步的条数**从量子配额算出来**，不是写死的 8 ——
    //    写死的话，配额一旦被调小，这条会因**夹具**（而不是被测行为）变红
    //    （v0.19 检视轮 V-B 补强 6 / V-A O-7）。
    //
    //    判别式落在**关系**上：跑满**恰好一个量子**（`QUANTUM_INSTR_QUOTA`
    //    条）仍**不许**交付；再跑一个量子就**必须**交付。
    let before_boundary = QUANTUM_INSTR_QUOTA.saturating_sub(16);
    step(&mut host, before_boundary);
    assert_eq!(deliveries(&host), 0, "量子边界之前不许交付中断");

    // 跨过量子边界 ⇒ **必须**交付。
    step(&mut host, QUANTUM_INSTR_QUOTA);
    assert_eq!(deliveries(&host), 1, "跨过量子边界后必须交付一次中断");
}

/// **夹具防漂移**：固件字节的**关键位置**必须还是「我们以为的那些指令」。
///
/// ⚠️ 仓库里**没有** RISC-V 汇编器（交叉工具链在**仓库外**，见
/// `tools/fetch-golden-deps.sh`）⇒ 字节与 `fixtures/intc_fw.S` 之间
/// **没有自动一致性检查**。这条用例是**人可复核**的那一道：它把每条
/// **关键**指令的编码写死，字节被改动就会红（v0.19 检视轮 V-B · G-9）。
#[test]
fn the_firmware_bytes_are_still_the_ones_the_source_file_describes() {
    // 0x00 lui t0, 0x0
    assert_eq!(&INTC_FIRMWARE[0x00..0x04], &[0xB7, 0x02, 0x00, 0x00]);
    // 0x04 addi t0, t0, 0x3c（handler 的地址）
    assert_eq!(&INTC_FIRMWARE[0x04..0x08], &[0x93, 0x82, 0xC2, 0x03]);
    // 0x0c csrrw x0, mtvec, t0
    assert_eq!(&INTC_FIRMWARE[0x0c..0x10], &[0x73, 0x90, 0x52, 0x30]);
    // 0x14 srli t1, t1, 1 · 0x18 ori t1, t1, 0x80
    //   ⇒ t1 = 0x880（mie 的 MEIE bit 11 + MTIE bit 7）
    assert_eq!(
        &INTC_FIRMWARE[0x14..0x1c],
        &[0x13, 0x53, 0x13, 0x00, 0x13, 0x63, 0x03, 0x08]
    );
    // 0x1c csrrw x0, mie, t1
    assert_eq!(&INTC_FIRMWARE[0x1c..0x20], &[0x73, 0x10, 0x43, 0x30]);
    // 0x20 addi t2, x0, 8（mstatus 的 MIE 位，bit 3）· 0x24 csrrs x0, mstatus, t2
    assert_eq!(
        &INTC_FIRMWARE[0x20..0x28],
        &[0x93, 0x03, 0x80, 0x00, 0x73, 0xA0, 0x03, 0x30]
    );
    // 0x48 addi t3, x0, 2 —— 掩码 0b10 = **线 1**（bit L = 线 L）
    assert_eq!(&INTC_FIRMWARE[0x48..0x4c], &[0x13, 0x0E, 0x20, 0x00]);
    // 0x4c csrrw x0, 0x7c0, t3（irq_clear）
    assert_eq!(&INTC_FIRMWARE[0x4c..0x50], &[0x73, 0x10, 0x0E, 0x7C]);
    // 0x50 mret
    assert_eq!(&INTC_FIRMWARE[0x50..0x54], &[0x73, 0x00, 0x20, 0x30]);
    // 固件总长 84 字节（0x54）
    assert_eq!(INTC_FIRMWARE.len(), 84, "固件长度变了");
}

/// 交付 ⇒ 写 `mcause` / `mepc` / `mtval`、跳 `mtvec`。
#[test]
fn the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table() {
    let mut host = host_with_intc_firmware();
    step(&mut host, 16);
    host.raise_irq(LINE_1).expect("拉起中断必须成功");
    step(&mut host, QUANTUM_INSTR_QUOTA * 2);

    assert_eq!(
        csr(&host, CSR_MCAUSE),
        0x8000_000B,
        "mcause 必须是「中断位 | **架构 cause 码 11**」（外部中断），不是内部线号"
    );
    assert_eq!(
        csr(&host, CSR_MEPC),
        SPIN_ADDR,
        "mepc 必须是陷阱时那条待执行的指令的地址"
    );
    assert!(
        csr_is_present(&host, CSR_MTVAL),
        "中断路径必须**真的写**过 mtval（不是「读不到就是 0」的那种假绿）"
    );
    assert_eq!(
        csr(&host, CSR_MTVAL),
        0,
        "中断没有故障地址 ⇒ mtval 必须是 0"
    );
    assert_eq!(
        csr(&host, CSR_MTVEC),
        HANDLER_ADDR,
        "mtvec 必须是 guest 自己写的那张向量表"
    );
}

/// handler 的 `irq_clear` ⇒ 挂起项被清、`active_irq` 复位。
///
/// ⚠️ 断言落在**控制器的导出状态**上（不是「不抛错」）。
#[test]
fn the_handler_clears_the_line_and_resets_the_active_interrupt() {
    let mut host = host_with_intc_firmware();
    step(&mut host, 16);
    host.raise_irq(LINE_1).expect("拉起中断必须成功");
    step(&mut host, QUANTUM_INSTR_QUOTA * 2);

    let state = host.intc_state();
    assert!(
        state.pending.is_empty(),
        "handler 清过线 ⇒ 挂起队列必须空：{:?}",
        state.pending
    );
    assert_eq!(
        state.active_irq, NO_ACTIVE_IRQ,
        "handler 清过线 ⇒ active_irq 必须复位"
    );
    assert_eq!(
        host.intc_delegation_failures(),
        0,
        "中断委派不许失败（失败时 intc_state 返回的是默认值 —— 正好等于断言值）"
    );
}

/// 清过之后**同一个中断还能再来一次**（`mret` 回到 `mepc`，`MIE` 被恢复）。
#[test]
fn the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it() {
    let mut host = host_with_intc_firmware();
    step(&mut host, 16);

    host.raise_irq(LINE_1).expect("拉起中断必须成功");
    step(&mut host, QUANTUM_INSTR_QUOTA * 2);
    assert_eq!(deliveries(&host), 1, "第一次交付必须发生");

    // 第二次请求 ⇒ 第二次交付。
    host.raise_irq(LINE_1).expect("第二次拉起必须成功");
    step(&mut host, QUANTUM_INSTR_QUOTA * 2);
    assert_eq!(deliveries(&host), 2, "清过之后同一个中断必须能再来一次");

    // ⚠️ **断言必须落在「第二次交付之后」的控制器状态上**。
    //
    //    只数「handler 跑了几次」在「**第二次清除被跳过**」时**照样成立**
    //    —— handler 确实跑了，只是它写的 `irq_clear`（guest 每次都写**同一个**
    //    掩码）没生效 ⇒ `active_irq` 不复位 ⇒ 这条线被**永久堵死**。
    let state = host.intc_state();
    assert!(
        state.pending.is_empty(),
        "第二次 handler 清线后挂起队列必须空：{:?}",
        state.pending
    );
    assert_eq!(
        state.active_irq, NO_ACTIVE_IRQ,
        "第二次 handler 清线后 active_irq 必须复位（否则这条线被永久堵死）"
    );
    assert_eq!(
        host.intc_delegation_failures(),
        0,
        "中断委派不许失败（失败时 intc_state 返回的是默认值 —— 正好等于断言值）"
    );
}

/// `mret` 之后回到 `mepc`（而不是回到 handler 或乱跳）。
#[test]
fn the_handler_returns_to_mepc() {
    let mut host = host_with_intc_firmware();
    step(&mut host, 16);
    host.raise_irq(LINE_1).expect("拉起中断必须成功");
    step(&mut host, QUANTUM_INSTR_QUOTA * 2);

    assert_eq!(host.pc(), SPIN_ADDR, "mret 之后必须回到 mepc（自旋那条）");
}
