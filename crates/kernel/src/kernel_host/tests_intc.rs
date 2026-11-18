//! v0.19 接线批 —— **宿主侧**的中断接线验证。
//!
//! 本文件盯的是**宿主**这一层的接缝（接线点 #5 / #6 / #7）：
//!
//! - 接缝 #5：**中断脉冲**探针（M-16 → M-18，台账 **D-26**）——
//!   一次 `request()` 必须让 M-18 收到**恰好一条** `irq_pulse` 边沿；
//! - 接缝 #6：`set_timer_compare`（裁决 E）—— 定时器中断的入口；
//! - 接缝 #7：`clear_irq` —— 调试器 / 固件清中断。
//!
//! ⚠️ 断言全部落在**输出**上：M-18 收到的**边沿**、M-16 **导出的状态**
//! （`M16IntcDto`）。没有一条断言是「我调用了某个内部函数」。

use crate::kernel_host::platform;
use crate::m14_bus::BusWidth;
use crate::m15_peripheral::gpio::reg as gpio_reg;
use riscv_sim_contract::csr::{IRQ_SOFTWARE, IRQ_TIMER_COMPARE};
use riscv_sim_contract::model::{
    Edge, ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, WriteSize,
};
use riscv_sim_contract::port::Peripheral;
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use super::{KernelHost, DEMO_FIRMWARE_BYTES, IRQ_UART_RX_READY, QEMU_VIRT_UART0_BASE};
use crate::m17_backend::IRQ_FIRMWARE_BYTES;
use crate::m18_probe::CH_IRQ_EXTERNAL;

/// 造一个挂着 demo 固件的工程（与 D-46 那套夹具同形，但**不共享**——
/// 那个文件不在本 lane 的所有权里）。
fn demo_project() -> Project {
    Project {
        id: "proj-intc".to_string(),
        name: "intc".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 发一条命令，收它的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 某条核中断通道收到的**电平**边沿（不排空，只是看）。
fn irq_level_edges(host: &KernelHost, ch: u8) -> Vec<Edge> {
    let registry = host.probes.as_ref().expect("探针注册表必须装上");
    registry.borrow().edges_for(ch).to_vec()
}

/// 一次 `request()` ⇒ 该线自己的电平通道收到**恰好一条** 0→1 边沿。
///
/// ⚠️ **判别力**：抬线回调没装上、或通道没注册成 `SingleBit`，就一条
/// 边沿都收不到；值必须是 **1**（电平），**不是线号** —— 旧脉冲通道
/// 把线号当值画，正是这次要消掉的东西。
#[test]
fn one_request_produces_exactly_one_level_edge() {
    let mut host = KernelHost::new();
    assert!(
        irq_level_edges(&host, CH_IRQ_EXTERNAL).is_empty(),
        "还没请求过中断，边沿必须是空的"
    );

    host.raise_irq(1).expect("raise irq");

    let edges = irq_level_edges(&host, CH_IRQ_EXTERNAL);
    assert_eq!(
        edges.len(),
        1,
        "一次 request() 必须产生**恰好一条**电平边沿，实际 {} 条",
        edges.len()
    );
    assert_eq!(edges[0].value, 1, "电平通道的值是 1（有中断），不是线号");
}

/// 装上工程（换总线、换探针注册表）之后，脉冲回调**必须还在**。
///
/// ⚠️ D-46 的教训：「`LoadProject` 之后管道仍在」——换平台会重建整条
/// 探针管道，脉冲回调是其中一环。
#[test]
fn the_level_sink_survives_a_project_reload() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    host.raise_irq(1).expect("raise irq");

    assert_eq!(
        irq_level_edges(&host, CH_IRQ_EXTERNAL).len(),
        1,
        "装工程之后电平回调必须还在（换平台会重建探针管道）"
    );
}

/// `clear_irq` ⇒ 挂起项与线电平都被清掉（接缝 #7）。
///
/// 判别式落在 M-16 **导出的状态**上（段 5），不是「我调用了 clear」。
#[test]
fn clear_irq_clears_the_pending_entry() {
    let mut host = KernelHost::new();
    host.raise_irq(1).expect("raise irq");

    let before = host.intc_state();
    assert_eq!(before.pending.len(), 1, "请求之后挂起队列里应当有一条");
    assert_eq!(before.raised, vec![1], "线电平也应当被断言");

    host.clear_irq(1).expect("clear irq");

    let after = host.intc_state();
    assert!(
        after.pending.is_empty(),
        "清完之后挂起队列必须空，实际 {:?}",
        after.pending
    );
    assert!(
        after.raised.is_empty(),
        "清完之后线电平必须撤下，实际 {:?}",
        after.raised
    );
}

/// `set_timer_compare` ⇒ 定时器线在**量子边界**被挂起（接缝 #6 / 裁决 E）。
///
/// ⚠️ 固件侧的 MMIO 通路推迟到 P1（台账 **D-82**），本批的入口是**宿主
/// API**。判别式落在 M-16 导出的**挂起队列**上。
#[test]
fn the_timer_compare_line_is_raised_at_a_quantum_boundary() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 比较值 = 1000 ps：跑第一个量子（1 µs = 1 000 000 ps）就会越过它。
    host.set_timer_compare(1_000).expect("set timer compare");

    dispatch(&mut host, CommandPayload::Continue);

    let state = host.intc_state();
    assert!(
        state.pending.iter().any(|p| p.line == IRQ_TIMER_COMPARE),
        "定时器线必须在挂起队列里，实际 {:?}",
        state.pending
    );
}

/// **guest 写 CLINT 的 `msip` ⇒ 软件中断线（线 4）真的被抬起来 / 撤下去**
/// （v0.77 补线：寄存器 → 宿主搬运这一段）。
///
/// ⚠️ 判别力：器件层早就实现了 `msip`（可写可读、进快照），但宿主
/// `sync_interrupts` 若**不读**它，写寄存器完全不产生任何线 —— 症状是
/// 静默无反应。这条断言落在 M-16 导出的**挂起队列**上，不是「寄存器可读
/// 回」。
///
/// ⚠️ 写必须由 **guest 指令**发出：调试器的 `WriteMemory` 走的是调试侧的
/// 内存通道，**不**经过外设窗口（`read_memory(0x0200_0000)` 会报未映射）。
#[test]
fn writing_the_clint_msip_raises_the_software_line() {
    // 固件（3 条写 1，随后 2 条写 0，最后自旋）：
    //   lui   x5, 0x02000     ; x5 = 0x0200_0000（CLINT 的 msip）
    //   addi  x6, x0, 1
    //   sw    x6, 0(x5)       ; msip = 1（抬线）
    //   addi  x6, x0, 0
    //   sw    x6, 0(x5)       ; msip = 0（撤线）
    //   jal   x0, 0           ; 自旋
    const SOFTWARE_FIRMWARE: [u8; 24] = [
        0xB7, 0x02, 0x00, 0x02, // lui  x5, 0x02000
        0x13, 0x03, 0x10, 0x00, // addi x6, x0, 1
        0x23, 0xA0, 0x62, 0x00, // sw   x6, 0(x5)
        0x13, 0x03, 0x00, 0x00, // addi x6, x0, 0
        0x23, 0xA0, 0x62, 0x00, // sw   x6, 0(x5)
        0x6F, 0x00, 0x00, 0x00, // jal  x0, 0
    ];

    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(
        &mut host,
        CommandPayload::LoadProject(Project {
            id: "proj-intc-msip".to_string(),
            name: "intc-msip".to_string(),
            tree: ProductTree::default(),
            images: vec![ImageRef {
                name: "software-irq.bin".to_string(),
                kind: ImageKind::RawHex,
                source: ImageSource::Bytes(SOFTWARE_FIRMWARE.to_vec()),
            }],
            version: ProjectVersion::default(),
        }),
    );

    // 前置：msip = 0 时没有线 4。
    host.sync_interrupts_now();
    assert!(
        !host
            .intc_state()
            .pending
            .iter()
            .any(|p| p.line == IRQ_SOFTWARE),
        "msip = 0 时不该有软件中断线"
    );

    // 跑完前 3 条指令（停在 `sw` 之后）：msip = 1。
    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    host.sync_interrupts_now();
    let raised = host.intc_state();
    assert!(
        raised.pending.iter().any(|p| p.line == IRQ_SOFTWARE),
        "guest 写 msip = 1 必须抬软件中断线，实际 {:?}",
        raised.pending
    );

    // 再跑 2 条（`addi 0` + `sw 0`）：msip 落电平 ⇒ 线必须被撤掉。
    dispatch(&mut host, CommandPayload::StepN { n: 2 });
    host.sync_interrupts_now();
    let lowered = host.intc_state();
    assert!(
        !lowered.pending.iter().any(|p| p.line == IRQ_SOFTWARE),
        "guest 写 msip = 0 必须撤掉软件中断线，实际 {:?}",
        lowered.pending
    );
}

/// `set_timer_compare(0)` ⇒ 定时器**关闭**（不挂起任何东西）。
///
/// ⚠️ `0` 必须表示「关闭」而不是「阈值 0」——否则 `mtime >= 0` 恒成立，
/// 那是一条**永不消失**的中断。
#[test]
fn a_zero_timer_compare_arms_nothing() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    host.set_timer_compare(0).expect("set timer compare");

    dispatch(&mut host, CommandPayload::Continue);

    let state = host.intc_state();
    assert!(
        state.pending.is_empty(),
        "定时器关闭时不该有任何挂起，实际 {:?}",
        state.pending
    );
}

/// **端到端**：宿主拉起的 IRQ 真的**到达了 guest**。
///
/// ⚠️ 这条用例盯的是**宿主那一段接线**：宿主必须把**同一个**控制器交给
/// M-17 的执行器（接缝 #2 / #3）。若宿主忘了装（或者装的是**另一个**
/// 控制器），M-16 会一直挂着那条线，而 guest **永远收不到**。
///
/// 判别式落在 **guest 的寄存器**上：固件的 handler 每跑一遍就把 x7 加一。
#[test]
fn a_raised_irq_really_reaches_the_guest() {
    let mut host = KernelHost::new();
    // ⚠️ **v0.76（STAGE-52 · B1）**：无用户板装载被拒（`E_PLT-22`）⇒ ⇒
    //   测试前置条件改为**显式配一块默认板**（装载语义不变）。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    dispatch(
        &mut host,
        CommandPayload::LoadProject(Project {
            id: "proj-intc-e2e".to_string(),
            name: "intc-e2e".to_string(),
            tree: ProductTree::default(),
            images: vec![ImageRef {
                name: "irq-firmware.bin".to_string(),
                kind: ImageKind::RawHex,
                source: ImageSource::Bytes(IRQ_FIRMWARE_BYTES.to_vec()),
            }],
            version: ProjectVersion::default(),
        }),
    );

    host.raise_irq(1).expect("raise irq");
    dispatch(&mut host, CommandPayload::Continue);

    assert_eq!(
        host.registers()[7],
        1,
        "guest 的 handler 必须跑过一遍 —— 宿主拉起的 IRQ 没有到达 guest"
    );
    assert_eq!(host.pc(), 0x18, "mret 之后 guest 回到主循环");
}

/// **GPIO 的 pending 由 guest 用 W1C 消费，宿主不替它清**（Renode 口径）。
///
/// 完整时序：宿主 sync 只抬 PLIC 源（不 claim）→ guest claim →
/// guest W1C 设备 pending → guest complete。claim 之后、complete 之前，
/// 宿主每拍 sync 都**不能**把同一个源重新挂起（否则会重复投递）。
#[test]
fn gpio_pending_survives_host_sync_and_is_cleared_by_guest_w1c() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );

    const GPIO_BASE: u32 = 0x4000_0000;
    const GPIO_IRQ_SOURCE: u32 = 11;

    let plic = host.devices.plic("PLIC").expect("qemu-virt 必须有 PLIC");
    {
        let mut p = plic.borrow_mut();
        p.set_threshold(0, 0);
        p.enable(0, GPIO_IRQ_SOURCE, true);
        p.set_priority(GPIO_IRQ_SOURCE, 1);
    }

    let gpio = host.devices.gpio("GPIO0").expect("qemu-virt 必须有 GPIO0");
    gpio.borrow_mut().set_directions(0);
    gpio.borrow_mut()
        .write(gpio_reg::RISE_IE, WriteSize::Word, 0b101)
        .expect("使能两路输入的上升沿");
    gpio.borrow_mut().set_external_inputs(0b101);
    assert!(
        gpio.borrow().pending_pin(0) && gpio.borrow().pending_pin(2),
        "前置：两路边沿都必须锁存"
    );

    host.sync_interrupts_now();
    assert!(
        gpio.borrow().pending_pin(0) && gpio.borrow().pending_pin(2),
        "宿主 sync 不许替 guest 消费 pending（必须留到 W1C）"
    );
    assert!(
        plic.borrow().pending(GPIO_IRQ_SOURCE),
        "pending 还在，PLIC 源必须保持挂起（宿主 sync 不得 claim）"
    );

    // guest 读 Claim：消费 PLIC pending，压入 active 栈。
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(GPIO_IRQ_SOURCE),
        "guest claim 必须拿到 GPIO 源"
    );
    assert!(
        !plic.borrow().pending(GPIO_IRQ_SOURCE),
        "claim 必须清掉 PLIC 的 pending"
    );

    host.bus_mut()
        .write(GPIO_BASE + gpio_reg::PENDING, BusWidth::Word, 0b01)
        .expect("guest 必须能写 GPIO 的 PENDING 寄存器");
    assert!(
        !gpio.borrow().pending_pin(0) && gpio.borrow().pending_pin(2),
        "W1C 只清写 1 的位，另一路必须还在"
    );

    host.sync_interrupts_now();
    assert!(
        !plic.borrow().pending(GPIO_IRQ_SOURCE),
        "源还在 active，宿主不得在 complete 前重新挂起"
    );

    host.bus_mut()
        .write(GPIO_BASE + gpio_reg::PENDING, BusWidth::Word, 0b100)
        .expect("guest 必须能清最后一路");
    assert!(
        !gpio.borrow().pending_pin(2),
        "guest 写 1 之后最后一路 pending 必须消失"
    );

    // 设备线落下之后宿主 sync，再 complete：不得重新挂起。
    host.sync_interrupts_now();
    plic.borrow_mut().complete(0, GPIO_IRQ_SOURCE);
    assert!(
        !plic.borrow().pending(GPIO_IRQ_SOURCE),
        "全部 pending 清零 + complete 之后 PLIC 源必须消失"
    );
    assert_eq!(plic.borrow_mut().claim(0), None, "不再有可交付的源");
}

/// **active 的高优先级源压住低优先级源的外部线**（Renode
/// `RefreshInterrupt`：pending 优先级必须严格大于 active 栈顶）。
///
/// 判别力：UART 源 10（优先级 5）与 GPIO 源 11（优先级 1）同时挂起，
/// guest claim 走 UART 后，外部线必须落下；不补这条压制时，GPIO 会把
/// 线一直抬着，形成优先级反转。
#[test]
fn a_lower_priority_source_does_not_raise_the_external_line_while_higher_is_active() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );

    const UART_SOURCE: u32 = 10;
    const GPIO_SOURCE: u32 = 11;

    // UART：打开 RX 中断（IER bit0），再塞一个字节。
    host.bus_mut()
        .write(QEMU_VIRT_UART0_BASE + 1, BusWidth::Byte, 0x01)
        .expect("guest 必须能写 UART 的 IER");
    host.uart_receive(0x41);

    // GPIO：一路上升沿。
    let gpio = host.devices.gpio("GPIO0").expect("qemu-virt 必须有 GPIO0");
    gpio.borrow_mut().set_directions(0);
    gpio.borrow_mut()
        .write(gpio_reg::RISE_IE, WriteSize::Word, 0b1)
        .expect("使能一路输入的上升沿");
    gpio.borrow_mut().set_external_inputs(0b1);

    // PLIC：UART 高、GPIO 低。
    let plic = host.devices.plic("PLIC").expect("qemu-virt 必须有 PLIC");
    {
        let mut p = plic.borrow_mut();
        p.set_threshold(0, 0);
        for (source, priority) in [(UART_SOURCE, 5u8), (GPIO_SOURCE, 1u8)] {
            p.enable(0, source, true);
            p.set_priority(source, priority);
        }
    }

    host.sync_interrupts_now();
    assert!(plic.borrow().pending(UART_SOURCE), "UART 源必须挂起");
    assert!(plic.borrow().pending(GPIO_SOURCE), "GPIO 源必须挂起");
    assert!(
        host.intc_state()
            .pending
            .iter()
            .any(|p| p.line == IRQ_UART_RX_READY),
        "两个源都在，外部线必须抬起"
    );

    // guest claim 走 UART（高优先级）。
    assert_eq!(
        plic.borrow_mut().claim(0),
        Some(UART_SOURCE),
        "先交付高优先级的 UART 源"
    );
    host.sync_interrupts_now();
    assert!(
        plic.borrow().pending(GPIO_SOURCE),
        "GPIO 仍在排队（它没有被 claim）"
    );
    assert!(
        !host
            .intc_state()
            .pending
            .iter()
            .any(|p| p.line == IRQ_UART_RX_READY),
        "UART 还在 active，低优先级 GPIO 不得把外部线抬起来"
    );

    // guest 读走 UART 字节 ⇒ 网关线落下；再 complete ⇒ 轮到 GPIO。
    host.bus_mut()
        .read(QEMU_VIRT_UART0_BASE, BusWidth::Byte)
        .expect("guest 必须能读 UART 的 RBR");
    plic.borrow_mut().complete(0, UART_SOURCE);
    host.sync_interrupts_now();
    assert!(
        host.intc_state()
            .pending
            .iter()
            .any(|p| p.line == IRQ_UART_RX_READY),
        "高优先级源完成后，GPIO 才允许抬线"
    );
}

/// **未使能的边沿不许抬 PLIC 源**（RISE_IE/FALL_IE 的端到端判别力）。
#[test]
fn gpio_pending_requires_edge_enable_to_reach_plic() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );

    const GPIO_IRQ_SOURCE: u32 = 11;
    let gpio = host.devices.gpio("GPIO0").expect("qemu-virt 必须有 GPIO0");
    let plic = host.devices.plic("PLIC").expect("qemu-virt 必须有 PLIC");
    gpio.borrow_mut().set_directions(0);

    // 未使能：边沿不进 pending，也不许抬 PLIC 源。
    gpio.borrow_mut().set_external_inputs(0b1);
    host.sync_interrupts_now();
    assert!(!gpio.borrow().pending_pin(0), "未使能的边沿不许进 pending");
    assert!(
        !plic.borrow().pending(GPIO_IRQ_SOURCE),
        "没有 pending 就不许抬 PLIC 源"
    );

    // 使能上升沿之后：从 0 回到 1 必须进 pending 并抬源。
    gpio.borrow_mut()
        .write(gpio_reg::RISE_IE, WriteSize::Word, 0b1)
        .expect("使能上升沿");
    gpio.borrow_mut().set_external_inputs(0b0);
    assert!(!gpio.borrow().pending_pin(0), "下降沿仍未使能");
    gpio.borrow_mut().set_external_inputs(0b1);
    host.sync_interrupts_now();
    assert!(gpio.borrow().pending_pin(0), "已使能的上升沿必须进 pending");
    assert!(
        plic.borrow().pending(GPIO_IRQ_SOURCE),
        "有 pending 就必须抬 PLIC 源"
    );
}

/// **UART 的设备通道必须既有上升沿、也有下降沿**（v0.79 追问：
/// 「为什么只观测到上升沿」的判别用例）。
///
/// 口径：16550 的 RX 中断是**电平**——源电平 = `IER.bit0 ∧ LSR.RX_VALID`。
/// 所以「塞一个字节 ⇒ 抬线」与「guest 把 RBR 读空 ⇒ 落线」必须**都**在
/// `irq.dev.uart0` 上留下边沿。只抬不落 ⇒ 用户看到的就是「一条永远为 1
/// 的线」，那要么是固件没排空（语义如此），要么是本用例要抓的缺陷。
#[test]
fn the_uart_device_channel_falls_when_the_receive_buffer_is_drained() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );

    let ch = host
        .irq_src_channels
        .iter()
        .find(|(s, _)| *s == 10)
        .map(|(_, ch)| *ch)
        .expect("qemu-virt 的 UART0 是 PLIC 源 10 ⇒ 必须有设备通道");

    // 固件侧：打开 RX 中断，来一个字节。
    host.bus_mut()
        .write(QEMU_VIRT_UART0_BASE + 1, BusWidth::Byte, 0x01)
        .expect("guest 必须能写 UART 的 IER");
    host.uart_receive(0x41);
    host.sync_interrupts_now();
    let after_rx = irq_level_edges(&host, ch);
    assert_eq!(
        after_rx.iter().map(|e| e.value).collect::<Vec<_>>(),
        vec![1u64],
        "来了一个字节 ⇒ 设备通道必须抬到 1"
    );

    // guest 侧：把 RBR 读走（读空 ⇒ LSR.RX_VALID 落）。
    let _ = host.read_memory(QEMU_VIRT_UART0_BASE, WriteSize::Byte);

    host.sync_interrupts_now();
    let all = irq_level_edges(&host, ch);
    assert_eq!(
        all.iter().map(|e| e.value).collect::<Vec<_>>(),
        vec![1u64, 0u64],
        "读空之后 ⇒ 设备通道必须**落**到 0（这正是「只有上升沿」缺的那一半）"
    );

/// v0.82 · **FIFO 开 + 水位 4** 时设备通道的行为：3 个字节不抬、第 4 个抬、
/// 读走一个就落 —— 这就是「不再形成中断风暴」的端到端证据。
#[test]
fn the_uart_channel_with_fifo_mode_needs_the_trigger_level() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT)
                .expect("内置 qemu-virt 必须存在"),
            expected_revision: None,
        },
    );

    let ch = host
        .irq_src_channels
        .iter()
        .find(|(s, _)| *s == 10)
        .map(|(_, ch)| *ch)
        .expect("qemu-virt 的 UART0 是 PLIC 源 10 ⇒ 必须有设备通道");

    // 固件侧：IER.RDI = 1，FCR = FE(1) | 水位 4（bit6-7 = 01）⇒ 0x41。
    host.bus_mut()
        .write(QEMU_VIRT_UART0_BASE + 1, BusWidth::Byte, 0x01)
        .expect("写 IER");
    host.bus_mut()
        .write(QEMU_VIRT_UART0_BASE + 2, BusWidth::Byte, 0x41)
        .expect("写 FCR");

    for b in [0x41u8, 0x42, 0x43] {
        host.uart_receive(b);
    }
    host.sync_interrupts_now();
    assert!(
        irq_level_edges(&host, ch).is_empty(),
        "深度 3 < 水位 4 ⇒ 网关电平**不许**抬（旧实现会抬 ⇒ 风暴起点）"
    );

    host.uart_receive(0x44);
    host.sync_interrupts_now();
    assert_eq!(
        irq_level_edges(&host, ch)
            .iter()
            .map(|e| e.value)
            .collect::<Vec<_>>(),
        vec![1u64],
        "深度 4 = 水位 ⇒ 抬"
    );

    let _ = host.read_memory(QEMU_VIRT_UART0_BASE, WriteSize::Byte);
    host.sync_interrupts_now();
    assert_eq!(
        irq_level_edges(&host, ch)
            .iter()
            .map(|e| e.value)
            .collect::<Vec<_>>(),
        vec![1u64, 0u64],
        "读走一个 ⇒ 深度 3 < 水位 ⇒ 电平**自己落**（不再需要读空整个 FIFO）"
    );
}
}
