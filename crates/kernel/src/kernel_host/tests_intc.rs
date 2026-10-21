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

use riscv_sim_contract::csr::IRQ_TIMER_COMPARE;
use riscv_sim_contract::model::{
    Edge, ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use super::{KernelHost, DEMO_FIRMWARE_BYTES};
use crate::m17_backend::IRQ_FIRMWARE_BYTES;
use crate::m18_probe::CH_IRQ_PULSE;

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

/// M-18 收到的 `irq_pulse` 边沿（**不**排空，只是看）。
fn irq_pulse_edges(host: &KernelHost) -> Vec<Edge> {
    let registry = host.probes.as_ref().expect("探针注册表必须装上");
    registry.borrow().edges_for(CH_IRQ_PULSE).to_vec()
}

/// 一次 `request()` ⇒ M-18 收到**恰好一条** `irq_pulse` 边沿（验收 30）。
///
/// ⚠️ **判别力**：脉冲回调没装上、或者通道没注册成 `Pulse`，M-18 就
/// **一条边沿都收不到**（`sample_irq_pulse` 对未注册的通道返回
/// `None`）——这条断言当场变红。
#[test]
fn one_request_produces_exactly_one_irq_pulse_edge() {
    let mut host = KernelHost::new();
    assert!(
        irq_pulse_edges(&host).is_empty(),
        "还没请求过中断，边沿必须是空的"
    );

    host.raise_irq(1).expect("raise irq");

    let edges = irq_pulse_edges(&host);
    assert_eq!(
        edges.len(),
        1,
        "一次 request() 必须产生**恰好一条** irq_pulse 边沿，实际 {} 条",
        edges.len()
    );
    assert_eq!(edges[0].value, 1, "脉冲的「值」是中断线号");
}

/// 装上工程（换总线、换探针注册表）之后，脉冲回调**必须还在**。
///
/// ⚠️ D-46 的教训：「`LoadProject` 之后管道仍在」——换平台会重建整条
/// 探针管道，脉冲回调是其中一环。
#[test]
fn the_pulse_sink_survives_a_project_reload() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    host.raise_irq(1).expect("raise irq");

    assert_eq!(
        irq_pulse_edges(&host).len(),
        1,
        "装工程之后脉冲回调必须还在（换平台会重建探针管道）"
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

/// `set_timer_compare(0)` ⇒ 定时器**关闭**（不挂起任何东西）。
///
/// ⚠️ `0` 必须表示「关闭」而不是「阈值 0」——否则 `mtime >= 0` 恒成立，
/// 那是一条**永不消失**的中断。
#[test]
fn a_zero_timer_compare_arms_nothing() {
    let mut host = KernelHost::new();
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
