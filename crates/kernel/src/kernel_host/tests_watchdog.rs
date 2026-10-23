//! v0.28 批 Lane D2 —— **宿主看门狗**的接线验证（台账 **D-115**）。
//!
//! 本文件盯的是**宿主**这一层的处置链路（冻结规格 STAGE-13 §3.3）：
//!
//! ```text
//! M-12 的无进展守卫触发 ──► Err(E_ENG-03)
//!        └─► 宿主窗口判定 ──► 第 3 个窗口 ──► 触发处置
//!                 ├─► hms.report(Watchdog/GuestRun) ⇒ Fatal ⇒ Health::Critical
//!                 ├─► M-21 环里一条 ctx = "ENG-WATCHDOG" 的日志
//!                 ├─► 幂等标志 watchdog_fired
//!                 └─► degrade(Terminate) ⇒ 内核被终止
//! ```
//!
//! ⚠️ **怎么造「引擎停滞」**：装一个**假的** `InterruptPort`，它的
//! `take_intr()` **恒**返回一条待交付中断 ⇒ 执行器每条指令都走
//! `InterruptTaken` ⇒ 永远不退休指令 ⇒ **M-12 的无进展守卫在 1024 次后
//! 触发**。
//!
//! ⚠️ **为什么必须用假端口**：M-16 的 `active_irq` 单级防护（未清除前不再
//! 交付）会让**真实**控制器只交付一次 ⇒ 活锁形态根本造不出来。这正是
//! VD-F3 当年实测到的那个形态。
//!
//! ⚠️ **断言全部落在输出上**：HMS 的信号流 / 健康状态、M-21 的环、契约的
//! 错误事件、引擎的快照。没有一条断言是「我调用了某个内部函数」。

use riscv_sim_contract::csr::{arch_mcause_for_line, IRQ_TIMER_COMPARE};
use riscv_sim_contract::error::{ErrorFamily, PlatformError, E_HMS_02_FATAL_TERMINATED};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload, Message};

use super::watchdog::{
    classify_window, HostWatchdog, WindowOutcome, WindowVerdict, HOST_WATCHDOG_BUDGET_MS,
    HOST_WATCHDOG_CONFIRMATIONS,
};
use super::KernelHost;
use crate::m16_interrupt::{InterruptPort, PendingIntr};
use crate::m21_logging::LogLevel;
use crate::m22_hms::{Health, Severity};

/// 一个**永远**有待交付中断的假端口 —— 活锁形态的来源。
///
/// ⚠️ 它**不是** M-16 的控制器：真控制器的 `active_irq` 单级防护会让它只交付
/// 一次（VD-F3 实测）⇒ 活锁造不出来。
struct AlwaysPendingPort {
    line: u16,
    cause: u32,
}

impl AlwaysPendingPort {
    fn new() -> Self {
        let line = IRQ_TIMER_COMPARE;
        let cause = arch_mcause_for_line(line).expect("定时器线有架构 cause 码");
        Self { line, cause }
    }

    fn intr(&self) -> PendingIntr {
        PendingIntr {
            line: self.line,
            cause: self.cause,
        }
    }
}

impl InterruptPort for AlwaysPendingPort {
    fn pending_intr(&self) -> Option<PendingIntr> {
        Some(self.intr())
    }

    /// ⚠️ **恒**返回一条 ⇒ 每次取指边界都交付一次 ⇒ 指令永远不退休。
    fn take_intr(&mut self) -> Option<PendingIntr> {
        Some(self.intr())
    }

    fn clear(&mut self, _line: u16) {}

    /// 量子边界的采样：本端口**不**登记交付（交付由 `take_intr` 无条件给）。
    fn poll(
        &mut self,
        _now_ps: u64,
        _mie: u32,
        _mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        Ok(None)
    }

    fn pending_lines(&self) -> Option<Vec<u16>> {
        Some(vec![self.line])
    }
}

/// 规格冻结的**日志上下文**（字面量）。
///
/// ⚠️ **刻意不用 [`super::watchdog::WATCHDOG_LOG_CTX`]**：拿常量断言常量 =
/// **恒真** = 没有判别力（实测：把常量改错，用例照样全绿）⇒ 断言必须钉
/// **规格的字面量**。
const EXPECTED_LOG_CTX: &str = "ENG-WATCHDOG";

/// 规格冻结的**停滞场景码**（字面量）—— 同上，刻意不用常量。
const EXPECTED_ERR_CODE: &str = "E_ENG-03";

/// 造一个**注定停滞**的宿主（假中断端口已装好）。
fn stalled_host() -> KernelHost {
    let mut host = KernelHost::new();
    host.backend
        .install_interrupt_port(Box::new(AlwaysPendingPort::new()));
    host
}

/// 发一条命令，收它的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 事件里的错误条目（`code`, `message`）。
fn errors_of(events: &[EventPayload]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            EventPayload::Error { code, message } => Some((code.clone(), message.clone())),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// D-8 · `classify_window` 是**纯函数**
// ---------------------------------------------------------------------------

/// **D-8**：`classify_window` 不依赖真实时钟 ⇒ 三条判定各自可钉。
///
/// ⚠️ **判别力**：把 `>` 写成 `>=`（边界）、或把 `stalled` 短路去掉，本条
/// 立刻变红。
#[test]
fn classify_window_is_a_pure_function() {
    assert_eq!(
        classify_window(HOST_WATCHDOG_BUDGET_MS + 1, false),
        WindowOutcome::NoProgress,
        "宿主时间超窗 ⇒ 无进展"
    );
    assert_eq!(
        classify_window(0, true),
        WindowOutcome::NoProgress,
        "停滞 ⇒ 无进展"
    );
    assert_eq!(
        classify_window(0, false),
        WindowOutcome::Progress,
        "又快又不停 ⇒ 有进展"
    );
    // 边界：预算**之内**（含）算有进展（判据是严格大于）。
    assert_eq!(
        classify_window(HOST_WATCHDOG_BUDGET_MS, false),
        WindowOutcome::Progress,
        "恰好等于预算 ⇒ 仍在预算之内"
    );
}

/// 状态机：连续**三个**无进展窗口才触发，且**只触发一次**。
///
/// ⚠️ **判别力**：把阈值改成 1、或去掉幂等标志，本条立刻变红。
#[test]
fn the_watchdog_fires_on_the_third_consecutive_window() {
    let mut watchdog = HostWatchdog::new();
    assert!(!watchdog.observe(WindowOutcome::NoProgress));
    assert_eq!(watchdog.streak(), 1);
    assert!(!watchdog.observe(WindowOutcome::NoProgress));
    assert_eq!(watchdog.streak(), 2);
    assert!(
        watchdog.observe(WindowOutcome::NoProgress),
        "第 3 个窗口必须触发"
    );
    assert_eq!(watchdog.streak(), HOST_WATCHDOG_CONFIRMATIONS);
    assert!(watchdog.fired());
    // 幂等：触发过之后不再触发。
    assert!(!watchdog.observe(WindowOutcome::NoProgress));
    assert!(watchdog.fired());
}

/// 「连续」是真的连续：有进展会把 streak 清零。
#[test]
fn a_progress_window_resets_the_streak() {
    let mut watchdog = HostWatchdog::new();
    watchdog.observe(WindowOutcome::NoProgress);
    watchdog.observe(WindowOutcome::NoProgress);
    watchdog.observe(WindowOutcome::Progress);
    assert_eq!(watchdog.streak(), 0, "有进展 ⇒ 连续计数清零");
    assert!(!watchdog.fired());
    // 清零之后又要攒满三个才触发。
    assert!(!watchdog.observe(WindowOutcome::NoProgress));
    assert!(!watchdog.observe(WindowOutcome::NoProgress));
    assert!(watchdog.observe(WindowOutcome::NoProgress));
}

// ---------------------------------------------------------------------------
// D-6 / D-10 · 前两个窗口**不上报**、宿主**继续**跑
// ---------------------------------------------------------------------------

/// **D-6 / D-10**：只喂 **1** 个 / **2** 个窗口 ⇒ `report` **未**被调、
/// M-21 环**无** `ENG-WATCHDOG`、内核**未**被终止 ⇒ 宿主**继续**跑。
///
/// ⚠️ **判别力**：把「停滞就上抛」写成「立刻返回 `Err`」⇒ 本条变红
/// （第 1 个窗口就该报错）。
#[test]
fn the_first_two_stalled_windows_report_nothing() {
    let mut host = stalled_host();

    for window in 1..=2u32 {
        assert_eq!(
            host.observe_continue_window(),
            WindowVerdict::Stalled,
            "第 {window} 个窗口必须被**吞掉**（裁决 G）"
        );
        assert_eq!(
            host.hms.signal_count(),
            0,
            "第 {window} 个窗口不得上报健康信号"
        );
        assert!(
            host.log_entries()
                .iter()
                .all(|entry| entry.ctx != EXPECTED_LOG_CTX),
            "第 {window} 个窗口不得写 WATCHDOG 日志"
        );
        assert!(!host.watchdog_fired(), "第 {window} 个窗口不得触发");
        assert!(!host.hms.terminated(), "第 {window} 个窗口不得终止内核");
    }

    // 宿主**继续**跑：streak 攒到 2，第 3 个窗口才触发。
    assert_eq!(host.watchdog.streak(), 2);
}

/// **D-10**：停滞错误**第 3 次**才上抛，且上抛的是**终止**错误。
///
/// ⚠️ **判别力**：「停滞就上抛」⇒ 第 1 个窗口就 `Err` ⇒ 本条变红。
#[test]
fn the_stall_is_only_propagated_on_the_third_window() {
    let mut host = stalled_host();

    assert_eq!(host.observe_continue_window(), WindowVerdict::Stalled);
    assert_eq!(host.observe_continue_window(), WindowVerdict::Stalled);

    match host.observe_continue_window() {
        WindowVerdict::Fired(err) => {
            assert_eq!(err.family, ErrorFamily::Health, "终止错误的族是 Health");
            assert_eq!(
                err.scenario, E_HMS_02_FATAL_TERMINATED,
                "终止错误用的是 E_HMS-02（规格 §8.1：宿主上报）"
            );
            assert!(
                err.message.contains(EXPECTED_ERR_CODE),
                "消息里必须说得出停滞场景码，实际 {:?}",
                err.message
            );
        }
        other => panic!("第 3 个窗口必须触发处置，实际 {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// D-3 / D-4 / D-5 · 触发后的处置
// ---------------------------------------------------------------------------

/// **D-3**：连续三个无进展窗口 ⇒ `hms.report` 被调 ⇒ `Severity::Fatal` ⇒
/// `Health::Critical`。
///
/// ⚠️ **判别力**：少喂一个窗口、或把 `SignalKind::Watchdog` 写成别的 ⇒
/// 分级不是 `Fatal` ⇒ 本条变红。
#[test]
fn three_stalled_windows_report_a_fatal_health_signal() {
    let mut host = stalled_host();

    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }

    assert_eq!(host.hms.signal_count(), 1, "必须**恰好**上报一条");
    assert_eq!(
        host.hms.severities()[0],
        Severity::Fatal,
        "Watchdog + GuestRun ⇒ 致命"
    );
    assert_eq!(host.health(), Health::Critical, "有致命信号 ⇒ Critical");
    assert!(host.watchdog_fired(), "幂等标志必须置位");
    assert!(host.hms.terminated(), "内核必须已被终止");
    // 信号的内容：源、种类、上下文、场景码。
    let signal = &host.hms.signals()[0];
    assert_eq!(signal.kind, crate::m22_hms::SignalKind::Watchdog);
    assert_eq!(signal.ctx, crate::m22_hms::SignalCtx::GuestRun);
    assert_eq!(signal.src, crate::m22_hms::SignalSrc::Engine);
    assert_eq!(signal.err_code.as_deref(), Some(EXPECTED_ERR_CODE));
}

/// **D-5**：M-21 环里有一条 `ctx == "ENG-WATCHDOG"`、`err_code == "E_ENG-03"`
/// 的条目，且**没被截断**。
///
/// ⚠️ **判别力**：不记日志 ⇒ 环里没有这条 ⇒ 本条变红。
#[test]
fn the_watchdog_writes_one_log_entry() {
    let mut host = stalled_host();

    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }

    let entries = host.log_entries();
    let watchdog_entries: Vec<_> = entries
        .iter()
        .filter(|entry| entry.ctx == EXPECTED_LOG_CTX)
        .collect();
    assert_eq!(
        watchdog_entries.len(),
        1,
        "必须**恰好**一条 WATCHDOG 日志，实际 {:?}",
        entries
    );
    let entry = watchdog_entries[0];
    assert_eq!(entry.level, LogLevel::Error);
    assert_eq!(entry.err_code, EXPECTED_ERR_CODE);
    assert!(
        !entry.truncated,
        "ctx（12 字节）与 err_code（9 字节）都在 M-21 的上限内 ⇒ 不该被截断"
    );
    assert!(
        entry.msg.contains("看门狗"),
        "消息必须说得出这是看门狗，实际 {:?}",
        entry.msg
    );
}

/// **D-4**：**只报一次** —— 再喂几个窗口 ⇒ `watchdog_fired` 仍为真、上报
/// 次数**不增**、日志也不增。
///
/// ⚠️ **判别力**：去掉幂等标志 ⇒ 每次触发都上报 ⇒ 本条变红。
#[test]
fn the_watchdog_reports_only_once() {
    let mut host = stalled_host();

    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }
    assert_eq!(host.hms.signal_count(), 1);

    // 再喂几个窗口（内核已经死了，但窗口照喂）。
    for _ in 0..3 {
        host.observe_continue_window();
    }

    assert!(host.watchdog_fired(), "幂等标志必须仍为真");
    assert_eq!(host.hms.signal_count(), 1, "上报次数**不得**增加");
    assert_eq!(
        host.log_entries()
            .iter()
            .filter(|entry| entry.ctx == EXPECTED_LOG_CTX)
            .count(),
        1,
        "WATCHDOG 日志**不得**增加"
    );
}

// ---------------------------------------------------------------------------
// D-7 · 重入不改引擎状态
// ---------------------------------------------------------------------------

/// **D-7**：重入**不改状态** —— 三次重入后的引擎快照 == 一次重入后的。
///
/// ⚠️ **判别力**：守卫触发时若顺手改了引擎状态（推进虚拟时间 / 改寄存器），
/// 两份快照就会不等 ⇒ 本条变红。
#[test]
fn re_entering_the_engine_does_not_change_its_state() {
    let mut one_window = stalled_host();
    assert_eq!(one_window.observe_continue_window(), WindowVerdict::Stalled);
    let after_one = one_window.engine_snapshot();

    let mut three_windows = stalled_host();
    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        three_windows.observe_continue_window();
    }
    let after_three = three_windows.engine_snapshot();

    assert_eq!(
        after_one, after_three,
        "重入**不得**改变引擎状态（虚拟时间也不该前进 —— 没有指令退休）"
    );
}

// ---------------------------------------------------------------------------
// 端到端 · 命令路径
// ---------------------------------------------------------------------------

/// `Continue` 命令 ⇒ 三个窗口 ⇒ 错误事件（`E_HMS-02`）+ HMS 致命 + 日志。
///
/// ⚠️ **判别力**：接线漏在 `on_run` 之外 ⇒ 命令路径上看不到处置 ⇒ 本条变红。
#[test]
fn the_continue_command_reports_the_termination() {
    let mut host = stalled_host();

    let events = dispatch(&mut host, CommandPayload::Continue);

    let errors = errors_of(&events);
    assert_eq!(errors.len(), 1, "必须**恰好**一个错误事件，实际 {errors:?}");
    assert_eq!(
        errors[0].0, "E_HMS-02",
        "前端据此区分「内核被终止」与「普通错误」（规格 §8.1）"
    );
    assert_eq!(host.health(), Health::Critical);
    assert_eq!(host.hms.signal_count(), 1);
    assert!(
        host.log_entries()
            .iter()
            .any(|entry| entry.ctx == EXPECTED_LOG_CTX),
        "处置必须留下 WATCHDOG 日志"
    );
}

/// `StepN` 路径**不得**把**原始的**停滞错误直接上抛（裁决 G）。
///
/// ⚠️ 这条断言在「Lane D1 的 `step_n` 守卫」合入前后**都成立**：合入前
/// `step_n` 不报停滞错误；合入后它报 `E_ENG-03`，而宿主**吞掉**它 ⇒
/// 前端看到的至多是 `E_HMS-02`。
#[test]
fn the_step_n_path_never_leaks_the_raw_stall_error() {
    let mut host = stalled_host();

    let events = dispatch(&mut host, CommandPayload::StepN { n: 10_000 });

    for (code, _) in errors_of(&events) {
        assert_ne!(code, "E_ENG-03", "停滞错误不得**立即**上抛（裁决 G）");
    }
}

/// 看门狗触发后，宿主**不再接受新的运行**（内核已经死了）。
///
/// ⚠️ **判别力**：去掉「已触发 ⇒ 不再跑」的闸门 ⇒ 第二次运行会**再进一次
/// 引擎**，而引擎仍然是死的 ⇒ 本条变红（要么多出上报，要么挂死）。
#[test]
fn a_terminated_kernel_accepts_no_further_run() {
    let mut host = stalled_host();
    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }
    assert!(host.hms.terminated());

    // 再跑一次 ⇒ 不得再上报（幂等）。
    let events = dispatch(&mut host, CommandPayload::Continue);
    assert_eq!(host.hms.signal_count(), 1, "上报次数**不得**增加");
    let errors = errors_of(&events);
    assert_eq!(errors.len(), 1, "必须仍是那一个终止错误");
    assert_eq!(errors[0].0, "E_HMS-02");
}

/// 判据只认 `E_ENG-03` —— 别的错误**不是**活锁。
///
/// ⚠️ **判别力**：把 `stalled` 写成「任何 `Err` 都算停滞」⇒ 这条变红
/// （普通错误会被当成停滞吞掉）。
#[test]
fn only_the_no_progress_code_counts_as_a_stall() {
    let mut host = KernelHost::new();
    let other = PlatformError::new(ErrorFamily::Engine, 1, "别的引擎错误");

    match host.judge_window(0, &Err(other.clone())) {
        WindowVerdict::Failed(err) => {
            assert_eq!(err.scenario, 1, "别的错误必须**原样**上抛");
        }
        other_verdict => panic!("别的错误不得被当成停滞，实际 {other_verdict:?}"),
    }
    assert_eq!(host.hms.signal_count(), 0, "别的错误不得触发上报");
    assert!(!host.watchdog_fired());
}
