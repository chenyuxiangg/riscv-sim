//! v0.59 批（复位语义回归批）—— **装载新工程 = 新会话 ⇒ 内核可复活**。
//!
//! ⚠️ **用户报的现象**：「UI 上点击复位按钮后为啥无法再运行了」。
//!
//! ⚠️ **本文件钉的是第三段**（内核侧）：**看门狗终止态跨 `LoadProject` 存活**
//! ⇒ ⇒ 复位救不回一个被看门狗杀死的会话 ⇒ ⇒ 「复位回到初始态」（FR-06 验收）
//! **做不到**。
//!
//! ⚠️ **断言全部落在输出上**：命令回的事件、健康状态、看门狗的幂等标志。
//! 没有一条断言是「我调用了某个内部函数」。
//!
//! ⚠️ **判别力**：把宿主里那两行重置删掉 ⇒ ⇒ 本文件**逐条变红**（⇒ ⇒ 每条
//! 判据都**有人管** ⇒ ⇒ 不是「判据存在但没人管」）。
//!
//! ⚠️ **第一段与第二段**（前端）落在 `front/tests/reset-semantics.test.mjs`
//! 与真机门禁的 `loadImage` 段 ⇒ ⇒ 三段**分层**钉。

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion,
};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload};

use super::tests_watchdog::{dispatch, errors_of, stalled_host};
use super::watchdog::HOST_WATCHDOG_CONFIRMATIONS;
use super::KernelHost;
use crate::m16_interrupt::{InterruptPort, PendingIntr};
use crate::m22_hms::Health;

/// 一个**永远没有**待交付中断的端口（安静端口）。
///
/// ⚠️ **为什么需要它**：「看门狗杀死的内核 ⇒ 装载新工程后**能再跑**」那条
/// ⇒ ⇒ 要看得见「能跑」⇒ ⇒ 就得让 guest **不再停滞**⇒ ⇒ 把那个「永远待交付」
/// 的端口换掉 ⇒ ⇒ 否则复位后跑的**第一件事**就是又停滞 ⇒ ⇒ 又触发 ⇒ ⇒
/// 「能跑」这件事**看不见**（⇒ ⇒ 那条断言会变成「又一次终止错误」⇒ ⇒
/// **分不出**「旧的终止态还在」与「新的触发」⇒ ⇒ **零判别力**）。
struct QuietPort;

impl InterruptPort for QuietPort {
    fn pending_intr(&self) -> Option<PendingIntr> {
        None
    }

    fn take_intr(&mut self) -> Option<PendingIntr> {
        None
    }

    fn clear(&mut self, _line: u16) {}

    fn poll(
        &mut self,
        _now_ps: u64,
        _mie: u32,
        _mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        Ok(None)
    }

    fn pending_lines(&self) -> Option<Vec<u16>> {
        Some(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 造一个挂着 demo 固件的工程。
///
/// ⚠️ **固件字节引用共享常量**（`kernel_host::DEMO_FIRMWARE_BYTES`）⇒ ⇒ 不抄
/// 第二份。
fn demo_project() -> Project {
    Project {
        id: "proj-demo".to_string(),
        name: "demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(super::DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 造一个**装不上**的工程（把裸机器码说成 ELF ⇒ ⇒ 容器畸形 ⇒ ⇒ 装载失败）。
///
/// ⚠️ **为什么要有它**：「装载**失败** ⇒ 会话**没换** ⇒ ⇒ 健康状态**不动**」
/// ⇒ ⇒ 那条断言需要一条**真的失败**的装载 ⇒ ⇒ 否则它恒绿。
fn broken_project() -> Project {
    Project {
        id: "proj-broken".to_string(),
        name: "broken".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "broken.elf".to_string(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(vec![0x00, 0x01, 0x02, 0x03]),
        }],
        version: ProjectVersion::default(),
    }
}

/// 造一个**注定停滞**的宿主并把看门狗**喂到触发**。
///
/// ⚠️ **夹具复用** [`stalled_host`]（同层可见 · `pub(super)`）⇒ ⇒ 「造一个停滞
/// 宿主」这件事**只有一处** ⇒ ⇒ 与 `tests_degrade.rs` 看到的是同一种形态。
fn terminated_host() -> KernelHost {
    let mut host = stalled_host();
    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }
    host
}

/// 取第一条 `Tick` 里的 PC（装载成功 ⇒ ⇒ 内核把 PC 设在入口）。
fn first_tick_pc(events: &[EventPayload]) -> Option<u32> {
    events.iter().find_map(|e| match e {
        EventPayload::Tick { pc, .. } => Some(*pc),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// **看门狗杀死的内核 ⇒ 装载新工程后能再跑**（用户报的那句「复位后无法再运行」
/// 的内核侧）。
///
/// ⚠️ **判别力**：把宿主里 `self.watchdog = HostWatchdog::new()` 那一行删掉
/// ⇒ ⇒ `Continue` 仍然返回「内核已被看门狗终止」⇒ ⇒ 本条**立刻变红**。
#[test]
fn the_watchdog_terminated_kernel_is_revived_by_loading_a_project() {
    let mut host = terminated_host();
    assert!(host.hms.terminated(), "前置条件：内核已经被看门狗终止");
    // ⚠️ **换成安静端口** ⇒ ⇒ guest 不再停滞 ⇒ ⇒ 「能再跑」这件事**看得见**
    // （否则复位后跑的第一件事就是又停滞 ⇒ ⇒ 又终止 ⇒ ⇒ 分不出新旧）。
    host.backend.install_interrupt_port(Box::new(QuietPort));

    // 复位 = 装载新工程。
    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert!(
        errors_of(&events).is_empty(),
        "装载必须成功，实际 {:?}",
        errors_of(&events)
    );

    // ⇒ ⇒ 内核**能再跑**了（**不再**是终止错误）。
    let run = dispatch(&mut host, CommandPayload::Continue);
    let errors = errors_of(&run);
    assert!(
        !errors.iter().any(|(code, _)| code == "E_HMS-02"),
        "装载新工程后**不该**再报终止错误，实际 {:?}",
        errors
    );
    assert!(
        errors.is_empty(),
        "装载新工程后**不该**有任何错误，实际 {:?}",
        errors
    );
}

/// **装载新工程 ⇒ 看门狗的幂等标志与健康终态都清掉**（不是只清一个）。
///
/// ⚠️ **为什么两个都要清**：只清宿主侧那个 ⇒ ⇒ 对外仍然说「内核已被终止」⇒ ⇒
/// **内核活着但界面说它死了**（两个真相）⇒ ⇒ 而且 `Hms::report` 的降级闸门
/// （`!watchdog_fired`）**关着**⇒ ⇒ 第二次看门狗**不会**再终止内核。
#[test]
fn loading_a_project_clears_the_watchdog_flag_and_the_health_state() {
    let mut host = terminated_host();
    assert!(host.watchdog_fired(), "前置条件：看门狗已触发");
    assert_eq!(
        host.health(),
        Health::Critical,
        "前置条件：健康状态是 Critical"
    );

    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert!(
        errors_of(&events).is_empty(),
        "装载必须成功，实际 {:?}",
        errors_of(&events)
    );

    assert!(!host.watchdog.fired(), "看门狗的幂等标志必须被清掉");
    assert!(!host.hms.terminated(), "健康管理的终态必须被清掉");
    assert_eq!(
        host.health(),
        Health::Ok,
        "新会话 ⇒ 健康状态必须回到 Ok（⛔ 不是「内核活着但健康状态说它死了」）"
    );
}

/// **正向对照**：**没**触发过看门狗的宿主 ⇒ 装载新工程后健康状态**本来就是**
/// `Ok` ⇒ ⇒ 上面那条不是恒绿。
#[test]
fn a_host_that_never_fired_the_watchdog_is_healthy_after_loading() {
    let mut host = KernelHost::new();
    assert_eq!(host.health(), Health::Ok, "前置条件：本来就是 Ok");

    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert!(
        errors_of(&events).is_empty(),
        "装载必须成功，实际 {:?}",
        errors_of(&events)
    );

    assert_eq!(host.health(), Health::Ok, "装载后仍然是 Ok");
    assert!(!host.hms.terminated());
}

/// **装载**失败** ⇒ 会话**没换** ⇒ ⇒ 健康状态**不动**。
///
/// ⚠️ **为什么这条要钉**：重置放在「全部步骤成功之后」⇒ ⇒ 装载失败时**不动**它
/// ⇒ ⇒ 「失败 ⇒ 旧会话还在」这件事**看得见**（⛔ 不许半重置）。
#[test]
fn a_failed_load_does_not_reset_the_health_state() {
    let mut host = terminated_host();
    assert_eq!(
        host.health(),
        Health::Critical,
        "前置条件：健康状态是 Critical"
    );

    let events = dispatch(&mut host, CommandPayload::LoadProject(broken_project()));
    let errors = errors_of(&events);
    assert_eq!(errors.len(), 1, "装载必须失败，实际 {errors:?}");
    assert!(
        errors[0].0.starts_with("E_LDR-"),
        "必须是装载族的错误码，实际 {:?}",
        errors[0]
    );

    assert_eq!(
        host.health(),
        Health::Critical,
        "装载失败 ⇒ 会话没换 ⇒ 健康状态**不许**被清"
    );
    assert!(host.hms.terminated(), "装载失败 ⇒ 终态**不许**被清");
}

/// **装载之后 ⇒ 第二次看门狗触发**仍然会**终止内核**（降级闸门重新打开）。
///
/// ⚠️ **判别力**：若 `reset_session` 把**降级策略**也清了（策略是**配置**不是
/// 会话状态）⇒ ⇒ 第二次触发**不会**终止 ⇒ ⇒ 本条变红。
#[test]
fn the_watchdog_can_terminate_the_kernel_again_after_a_reload() {
    let mut host = terminated_host();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert!(!host.hms.terminated(), "前置条件：装载后内核是活的");

    // 停滞形态还在（那个假中断端口**没**被卸掉）⇒ ⇒ 再跑三个窗口就又触发。
    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }

    assert!(
        host.watchdog.fired(),
        "第二次看门狗必须**又**触发（降级闸门重新打开）"
    );
    assert!(
        host.hms.terminated(),
        "第二次看门狗必须**又**终止内核（降级策略是**配置**，不该被清）"
    );
}

/// **装载新工程 ⇒ PC 回到入口**（复位 = 回到当前工程的起点）。
#[test]
fn loading_a_project_moves_the_pc_back_to_the_entry() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    // 跑一段 ⇒ PC 离开入口。
    dispatch(&mut host, CommandPayload::Continue);
    let after_run = host.backend.engine().snapshot().pc;
    assert_ne!(after_run, 0, "前置条件：PC 已经离开入口");

    let events = dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        first_tick_pc(&events),
        Some(0),
        "装载后 PC 必须回到入口（raw 镜像的入口是 0）"
    );
}
