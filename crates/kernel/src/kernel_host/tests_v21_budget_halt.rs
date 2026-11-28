//! 契约 **v21** —— 「窗口预算用尽」从 `EndOfExecution` 里拆出来的宿主侧门禁。
//!
//! 判据两条：
//!
//! - **B1**：`Continue` 跑满一个指令预算窗口之后，事件是
//!   `Halted{HaltReason::BudgetExhausted}`（**不是** `EndOfExecution`）⇒ ⇒
//!   驱动方分得清「这一窗到点了」与「程序跑完了」；
//! - **B2**：`StepN` 跑满 n 条**不发** `Halted`（调试器语义，本来就正常完成）
//!   ⇒ ⇒ 拆变体不能把 StepN 也拖下水。
//!
//! ⚠️ **为什么不是「按事件长得像就断言」**：两条都看 `Halted.reason` 这个
//! **枚举值**，并且同时断言虚拟时间真的前进了（窗口真跑了指令）——否则
//! 「零进展也报 `BudgetExhausted`」的实现同样会绿。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, HaltReason, Message};

use crate::kernel_host::{platform, KernelHost};

/// 自跳固件（`jal x0, 0`）—— 永远不会自己停，正好当「机器还在跑」的样本。
const SPIN_FIRMWARE: [u8; 4] = [0x6f, 0x00, 0x00, 0x00];

/// 一条退休指令推进的虚拟时间（= `m17_backend::INSTRUCTION_COST_PS`）。
const COST_PS: u64 = 1_000;
/// 一个 Continue 窗口的指令预算（`KernelHost::DEFAULT_INSTRUCTION_BUDGET`）。
const BUDGET: u64 = 10_000;

fn spin_project() -> Project {
    Project {
        id: "proj-v21".to_string(),
        name: "v21-budget-halt".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "spin.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(SPIN_FIRMWARE.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

fn load(host: &mut KernelHost) {
    dispatch(
        host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    let _ = dispatch(host, CommandPayload::LoadProject(spin_project()));
}

fn halt_reason(events: &[EventPayload]) -> Option<HaltReason> {
    events.iter().find_map(|event| match event {
        EventPayload::Halted { reason, .. } => Some(*reason),
        _ => None,
    })
}

fn elapsed_ps(events: &[EventPayload]) -> Option<u64> {
    events.iter().find_map(|event| match event {
        EventPayload::Tick { elapsed_ps, .. } => Some(*elapsed_ps),
        _ => None,
    })
}

/// **B1**：`Continue` 一个窗口 = 一个预算 ⇒ `BudgetExhausted`，而且再发一次
/// 还能继续跑（机器没停）。
#[test]
fn a_continue_window_reports_budget_exhausted_and_can_continue() {
    let mut host = KernelHost::new();
    load(&mut host);

    let first = dispatch(&mut host, CommandPayload::Continue);
    assert_eq!(
        halt_reason(&first),
        Some(HaltReason::BudgetExhausted),
        "窗口跑满必须是 `BudgetExhausted`，不能退回到 `EndOfExecution`"
    );
    assert_eq!(
        elapsed_ps(&first),
        Some(BUDGET * COST_PS),
        "一个窗口必须真的跑了 {} 条指令（否则上面那条断言可以靠「零进展」蒙绿）",
        BUDGET
    );

    let second = dispatch(&mut host, CommandPayload::Continue);
    assert_eq!(
        halt_reason(&second),
        Some(HaltReason::BudgetExhausted),
        "第二次 Continue 是同一个语义"
    );
    assert_eq!(
        elapsed_ps(&second),
        Some(2 * BUDGET * COST_PS),
        "第二次必须从第一次停的地方**续上**（机器没停）"
    );
}

/// **B2**：`StepN` 跑满 n 条**不发** `Halted`（v20 的口径不许被这次拆变体带偏）。
#[test]
fn a_step_command_never_emits_a_halt() {
    let mut host = KernelHost::new();
    load(&mut host);

    let events = dispatch(&mut host, CommandPayload::StepN { n: 7 });
    assert_eq!(
        halt_reason(&events),
        None,
        "StepN 跑满 7 条是正常完成 ⇒ 一条 Halted 都不该有"
    );
    assert_eq!(elapsed_ps(&events), Some(7 * COST_PS), "7 条 = 7000 ps");
}
