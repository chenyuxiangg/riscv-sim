//! v0.46 批（STAGE-31）· 宿主层**降级**的判别力用例（验收 **C-1 ~ C-6**）。
//!
//! ⚠️ 本文件盯的是**降级机制**这一层，不是「某个降级动作的业务效果」：
//!
//! * **派发路径**（验收 **C-1 / C-3**）：降级必须**问**宿主装的执行器 ⇒ ⇒
//!   M-22 自己硬编码处理 ⇒ ⇒ 派发记录恒空 ⇒ ⇒ 本文件立刻变红；
//! * **够得到的那一档**（验收 **C-4**）：`TraceDown` ⇒ ⇒ M-18 的采样**真的**
//!   关了 ＋ M-19 的溢出策略**真的**变成「不因溢出而暂停仿真」；
//! * **做不了的那三档**（验收 **C-5**）：`SnapshotRestore` / `Pause` /
//!   `Disconnect` ⇒ ⇒ 执行器表态「做不了」⇒ ⇒ **不重试**、**不**报 `E_HMS-01`；
//! * **默认行为一字不变**（验收 **C-6**）：看门狗触发后宿主回的**仍是**
//!   `E_HMS-02`。
//!
//! ⚠️ **怎么触发非 `Terminate` 的动作**：**改策略**再触发看门狗 ⇒ ⇒ 降级仍
//! 由 **M-22 的策略分支**触发 ⇒ ⇒ 宿主目录里**没有**任何直接调 `degrade` 的
//! 地方（门禁 **7h**：「降级在宿主里**只有一个入口**」）。

use riscv_sim_contract::protocol::CommandPayload;

use super::tests_watchdog::{dispatch, errors_of, stalled_host};
use super::watchdog::HOST_WATCHDOG_CONFIRMATIONS;
use super::KernelHost;
use crate::m18_probe::SampleCfg;
use crate::m19_waveform::OverflowPolicy;
use crate::m22_hms::{DegradeAction, Severity};

/// 喂满三个无进展窗口 ⇒ 看门狗**触发**。
///
/// ⚠️ 它走的是**宿主自己的**窗口判定 ⇒ ⇒ 前两个窗口被吞掉、第三个才触发 ⇒ ⇒
/// 「降级被派发了几次」这件事**没有被**本夹具放大。
fn fire_watchdog(host: &mut KernelHost) {
    for _ in 0..HOST_WATCHDOG_CONFIRMATIONS {
        host.observe_continue_window();
    }
}

/// M-18 的采样配置（降级前后的**唯一**读出口）。
fn sample_cfg(host: &KernelHost) -> SampleCfg {
    host.probes
        .as_ref()
        .expect("宿主装配时一定会装上 M-18 的探针注册表")
        .borrow()
        .sample_cfg()
        .clone()
}

/// M-19 的溢出策略（降级前后的**唯一**读出口）。
fn overflow_policy(host: &KernelHost) -> OverflowPolicy {
    host.waveform
        .as_ref()
        .expect("宿主装配时一定会装上 M-19 的波形管道")
        .pipeline()
        .overflow_policy()
}

/// 把 M-19 的溢出策略设成给定的那个（降级**之前**用）。
fn set_overflow_policy(host: &KernelHost, policy: OverflowPolicy) {
    host.waveform
        .as_ref()
        .expect("宿主装配时一定会装上 M-19 的波形管道")
        .pipeline_mut()
        .set_overflow_policy(policy);
}

/// 宿主副本（测试用）—— 造一个**注定停滞**的宿主。
fn host_with_policy(action: DegradeAction) -> KernelHost {
    let mut host = stalled_host();
    // ⚠️ **改策略**（不是直接调 `degrade`）⇒ ⇒ 降级仍由 M-22 的策略分支触发
    // ⇒ ⇒ 宿主里**只有那一个入口**。
    host.hms.set_degrade_policy(Severity::Fatal, action);
    host
}

// ---------------------------------------------------------------------------
// C-3 · 看门狗触发 ⇒ 走派发路径
// ---------------------------------------------------------------------------

/// **C-3a**：看门狗触发 ⇒ ⇒ 降级**走派发路径**（派发 `Terminate` **恰好
/// 一次**）⇒ ⇒ 内核被终止。
///
/// ⚠️ **判别力**：把宿主那行写死的 `degrade` 加回来（规格 §9.1 的 **T3**）
/// ⇒ ⇒ 派发次数变成 2 ⇒ ⇒ 本条变红；把执行器的安装去掉 ⇒ ⇒ 派发记录恒空 ⇒
/// ⇒ 本条变红。
#[test]
fn the_watchdog_trigger_goes_through_the_dispatch_path() {
    let mut host = stalled_host();
    assert_eq!(host.degrade_dispatch_total(), 0, "触发前执行器**没**被问过");
    assert_eq!(
        host.degrade_dispatch_last(),
        None,
        "触发前**没有**最后一次动作"
    );

    fire_watchdog(&mut host);

    assert_eq!(
        host.degrade_dispatch_last(),
        Some(DegradeAction::Terminate),
        "降级必须**派发** `Terminate`（而不是 M-22 自己硬编码）"
    );
    assert_eq!(
        host.degrade_dispatch_total(),
        1,
        "派发**恰好一次**（多了 ⇒ 「两个入口」⇒ 重复降级）"
    );
    assert!(
        host.hms.terminated(),
        "内核必须已被终止（`Terminate` 是**唯一**置 `terminated` 的动作）"
    );
}

// ---------------------------------------------------------------------------
// C-4 · `TraceDown` ⇒ 够得到的那两个旋钮**真的**动了
// ---------------------------------------------------------------------------

/// **C-4a**：`TraceDown` ⇒ ⇒ M-18 的采样**真的关了**。
///
/// ⚠️ **判别力**：把 `TraceDown` 的 M-18 那一半改成 `master_enable: true`
/// （规格 §9.1 的 **T4**）⇒ ⇒ 本条变红。
#[test]
fn the_trace_down_action_turns_the_sampling_off() {
    let mut host = host_with_policy(DegradeAction::TraceDown);

    // ⚠️ **夹具前提**：降级前采样**必须开着** ⇒ ⇒ 否则「降级后关掉了」这条
    // 断言**恒真**（零判别力）。
    assert!(
        sample_cfg(&host).master_enable,
        "夹具前提：降级前 M-18 的采样必须**开着**（默认装配开着 master）"
    );

    fire_watchdog(&mut host);

    assert_eq!(
        host.degrade_dispatch_last(),
        Some(DegradeAction::TraceDown),
        "降级必须**派发** `TraceDown`"
    );
    assert!(
        !sample_cfg(&host).master_enable,
        "降级后 M-18 的采样必须**真的关掉**（⇒ `sample` 变成零开销空操作）"
    );
}

/// **C-4b**：`TraceDown` ⇒ ⇒ M-19 的溢出策略**不因波形溢出而暂停仿真**。
///
/// ⚠️ **判别力的前提**：M-19 的溢出策略**默认就是** `DropOldestMark`（事实
/// **F11**）⇒ ⇒ **必须先**设成 `PauseSim` ⇒ ⇒ 降级后才看得出「策略被改回」
/// ⇒ ⇒ 否则这条断言**恒真**（零判别力）。
#[test]
fn the_trace_down_action_stops_the_waveform_from_pausing_the_simulation() {
    let mut host = host_with_policy(DegradeAction::TraceDown);

    // ⚠️ **先**设成 `PauseSim`（默认策略本来就是 `DropOldestMark` ⇒ ⇒ 不先改
    // 的话，下面那条断言**恒真**）。
    set_overflow_policy(&host, OverflowPolicy::PauseSim);
    assert_eq!(
        overflow_policy(&host),
        OverflowPolicy::PauseSim,
        "夹具前提：降级前 M-19 的溢出策略必须是 `PauseSim`"
    );

    fire_watchdog(&mut host);

    assert_eq!(
        overflow_policy(&host),
        OverflowPolicy::DropOldestMark,
        "降级后 M-19 的溢出策略必须是「丢最旧的 + 标记」⇒ ⇒ **不**因波形溢出而暂停仿真"
    );
}

/// **C-4c**：`TraceDown` **不**置 `terminated`（可恢复的动作**不是终态**）。
///
/// ⚠️ **判别力**：把「置 `terminated`」写成「**任何**动作都置」⇒ ⇒ 本条变红。
#[test]
fn the_trace_down_action_is_not_a_terminal_state() {
    let mut host = host_with_policy(DegradeAction::TraceDown);

    fire_watchdog(&mut host);

    assert_eq!(
        host.degrade_dispatch_last(),
        Some(DegradeAction::TraceDown),
        "降级必须**派发** `TraceDown`"
    );
    assert!(
        !host.hms.terminated(),
        "可恢复的动作**不是终态**（陷阱②：终态与可恢复**在数据上分得开**）"
    );
}

// ---------------------------------------------------------------------------
// C-5 · 三档够不着的动作 ⇒ `Unsupported`（不是失败）
// ---------------------------------------------------------------------------

/// **C-5**：`SnapshotRestore` / `Pause` / `Disconnect` ⇒ ⇒ 执行器表态**做不了**
/// ⇒ ⇒ **不重试**（只被问一次）⇒ ⇒ **不**报 `E_HMS-01`。
///
/// ⚠️ **判别力**：把「`Unsupported` 改成走重试路径」⇒ ⇒ 派发次数变成 3 ⇒ ⇒
/// 本条变红；把 `Unsupported` 改成 `Failed` ⇒ ⇒ 会报 `E_HMS-01` ⇒ ⇒ 本条
/// 变红。
///
/// ⚠️ **判别力边界（实测 · 本批自己测出来的）**：把执行器对这三档改成**返回
/// `Done`** ⇒ ⇒ **宿主层 0 红** ⇒ ⇒ 「`Done` 与 `Unsupported` 在宿主层**分不
/// 开**」⇒ ⇒ 根因 = **X-6**（`Hms::report` 的返回形状只有分级 ⇒ ⇒ 宿主拿不到
/// `DegradeOutcome`）⇒ ⇒ 且三档够不着的动作**本来就不动任何旋钮** ⇒ ⇒ 「做
/// 到了」与「做不了」在宿主层**观测等价**。
///
/// ⇒ ⇒ 所以「执行器表态 `Unsupported`」这条**只在 M-22 层**有覆盖（那里有
/// 假执行器）⇒ ⇒ 宿主层**结构性测不了**（如实登记，不假装）。
#[test]
fn the_three_unreachable_actions_are_unsupported_and_not_failures() {
    for action in [
        DegradeAction::SnapshotRestore,
        DegradeAction::Pause,
        DegradeAction::Disconnect,
    ] {
        let mut host = host_with_policy(action);

        fire_watchdog(&mut host);

        assert_eq!(
            host.degrade_dispatch_last(),
            Some(action),
            "{action:?}：降级必须**派发**这个动作（⇒ ⇒ 执行器**被问到**了）"
        );
        assert_eq!(
            host.degrade_dispatch_total(),
            1,
            "{action:?}：做不了的动作**只被问一次**（⇒ ⇒ 不重试）"
        );
        assert!(
            host.degrade_failures().is_empty(),
            "{action:?}：做不了**不是失败** ⇒ ⇒ 不得报 `E_HMS-01`，实际 {:?}",
            host.degrade_failures()
        );
        assert!(
            !host.hms.terminated(),
            "{action:?}：做不了 ⇒ ⇒ 不该终止内核"
        );
    }
}

// ---------------------------------------------------------------------------
// C-6 · 默认行为一字不变
// ---------------------------------------------------------------------------

/// **C-6**：**默认行为一字不变** —— 看门狗触发后宿主回的**仍是** `E_HMS-02`。
///
/// ⚠️ **判别力**：把默认策略删掉（宿主不设 `Fatal → Terminate`）⇒ ⇒ M-22 的
/// 策略分支**不触发**⇒ ⇒ 降级**没**发生 ⇒ ⇒ 内核**没**被终止 ⇒ ⇒ 本条变红。
#[test]
fn the_default_behaviour_is_unchanged() {
    let mut host = stalled_host();

    let events = dispatch(&mut host, CommandPayload::Continue);

    let errors = errors_of(&events);
    assert_eq!(errors.len(), 1, "必须**恰好**一个错误事件，实际 {errors:?}");
    assert_eq!(
        errors[0].0, "E_HMS-02",
        "默认策略下宿主回的**仍是** `E_HMS-02`（规格 §8.1：宿主上报终止错误）"
    );
    // 默认策略 = `Fatal → Terminate` ⇒ ⇒ 降级**真的**发生了 ⇒ ⇒ 内核被终止。
    assert_eq!(
        host.degrade_dispatch_last(),
        Some(DegradeAction::Terminate),
        "默认策略必须**派发** `Terminate`"
    );
    assert!(
        host.hms.terminated(),
        "默认策略下内核必须被终止（默认行为一字不变）"
    );
    assert!(
        host.degrade_failures().is_empty(),
        "默认行为**不得**报 `E_HMS-01`，实际 {:?}",
        host.degrade_failures()
    );
}
