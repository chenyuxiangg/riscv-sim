//! R-1 · **陷阱交付策略**（`SetStopPolicy` / `TrapSummary`）的宿主侧门禁。
//!
//! 需求单 v2（OSZ）的三条判据落在这里：
//!
//! - **AC1**：默认策略（`stop_on_trap = true`）下**不许**出现 `TrapSummary`
//!   ⇒ ⇒ 事件序列与改动前一致（逐字节的基线由 `m17_backend` /
//!   `kernel_host` 既有的 1553 条用例守）；
//! - **AC3**：`stop_on_trap = false` + `max_trap_events = N` ⇒ 跑到第 N 个
//!   陷阱**才**停，停因仍是 `HaltReason::Trap`；
//! - **AC4**（本批的载体是回体事件，不是逐条流水）：`TrapSummary` 的
//!   `first_cause` / `first_mepc` / `first_mtvec_target` 必须与 `enter_trap`
//!   实际写下的值一致 ⇒ ⇒ 断言直接对**地址常量**（基址 + 偏移），不是
//!   「事件里有什么就断言什么」的真空比对。
//!
//! ⚠️ **中断不进这条线**：中断走 `ExecOutcome::InterruptTaken`，本来就不停
//! 批次、也不计入陷阱数 ⇒ ⇒ 本文件的四个用例都不碰中断（那由
//! `tests_intc` 守）。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, HaltReason, Message};

use crate::kernel_host::{platform, KernelHost};

/// 固件：`auipc` 先拿走**自己的基址**（不写死地址 ⇒ 装到哪都成立），
/// 把 `mtvec` 设到 `基址 + 0x10`；**0x0c** 上是非法指令（全零），
/// handler（0x10）**也是**非法指令 ⇒ ⇒ 陷阱连环。
const TRAP_LOOP_FIRMWARE: [u8; 20] = [
    0x97, 0x02, 0x00, 0x00, // 0x00  auipc x5, 0        → x5 = pc
    0x93, 0x82, 0x02, 0x01, // 0x04  addi  x5, x5, 16   → x5 = handler
    0x73, 0x90, 0x52, 0x30, // 0x08  csrrw x0, mtvec, x5
    0x00, 0x00, 0x00, 0x00, // 0x0c  非法指令（全零）
    0x00, 0x00, 0x00, 0x00, // 0x10  非法指令（handler 自己也陷）
];

fn trap_project() -> Project {
    Project {
        id: "proj-r1".to_string(),
        name: "r1-stop-policy".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "trap-loop.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(TRAP_LOOP_FIRMWARE.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 配默认板 + 装载固件，回**装载后的入口**（`pc`）。
fn load(host: &mut KernelHost) -> u32 {
    dispatch(
        host,
        CommandPayload::SetMemoryMap {
            description: platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
                .expect("内置 default 必须存在"),
            expected_revision: None,
        },
    );
    let _ = dispatch(host, CommandPayload::LoadProject(trap_project()));
    host.pc()
}

fn halt_reason(events: &[EventPayload]) -> Option<HaltReason> {
    events.iter().find_map(|event| match event {
        EventPayload::Halted { reason, .. } => Some(*reason),
        _ => None,
    })
}

fn trap_summary(events: &[EventPayload]) -> Option<(u32, u32, u32, u32)> {
    events.iter().find_map(|event| match event {
        EventPayload::TrapSummary {
            count,
            first_cause,
            first_mepc,
            first_mtvec_target,
        } => Some((*count, *first_cause, *first_mepc, *first_mtvec_target)),
        _ => None,
    })
}

/// **AC1**：默认策略下第一条陷阱就收掉这一批，且**不发** `TrapSummary`。
#[test]
fn the_default_policy_stops_on_the_first_trap_without_a_summary() {
    let mut host = KernelHost::new();
    load(&mut host);

    let events = dispatch(&mut host, CommandPayload::Continue);

    assert_eq!(
        halt_reason(&events),
        Some(HaltReason::Trap),
        "默认策略 = 交付即停（与改动前一致）"
    );
    assert!(
        trap_summary(&events).is_none(),
        "默认策略下不许出现 TrapSummary —— 否则事件序列就变了（AC1）"
    );
}

/// **AC3 + AC4**：关掉策略、给 4 的配额 ⇒ 跑过 4 个陷阱才停；
/// `TrapSummary` 的三项对**地址常量**（基址 + 0x0c / + 0x10）。
#[test]
fn policy_off_continues_and_stops_at_the_quota_with_a_summary() {
    let mut host = KernelHost::new();
    let base = load(&mut host);

    let ack = dispatch(
        &mut host,
        CommandPayload::SetStopPolicy {
            stop_on_trap: false,
            max_trap_events: Some(4),
        },
    );
    assert!(
        ack.iter()
            .any(|event| matches!(event, EventPayload::Tick { .. })),
        "设策略必须被接受（回 Tick），实际 {ack:?}"
    );

    let events = dispatch(&mut host, CommandPayload::Continue);

    assert_eq!(
        trap_summary(&events),
        Some((4, 2, base + 0x0c, base + 0x10)),
        "count / mcause / mepc / mtvec 落点必须与交付值一致（AC4）"
    );
    assert_eq!(
        halt_reason(&events),
        Some(HaltReason::Trap),
        "配额到数按 Trap 收尾（AC3）"
    );
}

/// **不设配额 ⇒ 当场拒绝**（需求单：「`max_trap_events` 不能省略」）。
#[test]
fn policy_off_without_a_quota_is_rejected() {
    let mut host = KernelHost::new();

    let events = dispatch(
        &mut host,
        CommandPayload::SetStopPolicy {
            stop_on_trap: false,
            max_trap_events: None,
        },
    );

    let code = events
        .iter()
        .find_map(|event| match event {
            EventPayload::Error { code, .. } => Some(code.clone()),
            _ => None,
        })
        .expect("必须报协议错，而不是默默把预算烧穿");
    assert!(
        code.starts_with("E_PROTO-08"),
        "错误码必须是 E_PROTO-08，实际 {code}"
    );
}

/// **换工程 ⇒ 策略回默认**：设了 `false` 之后重新 `LoadProject`，
/// 再跑就必须在**第一条**陷阱停下（而且不再发 `TrapSummary`）。
#[test]
fn load_project_restores_the_default_policy() {
    let mut host = KernelHost::new();
    load(&mut host);
    let _ = dispatch(
        &mut host,
        CommandPayload::SetStopPolicy {
            stop_on_trap: false,
            max_trap_events: Some(4),
        },
    );

    load(&mut host);
    let events = dispatch(&mut host, CommandPayload::Continue);

    assert_eq!(halt_reason(&events), Some(HaltReason::Trap));
    assert!(
        trap_summary(&events).is_none(),
        "装载之后策略必须回默认（否则新固件的崩溃会被静静吞掉）"
    );
}
