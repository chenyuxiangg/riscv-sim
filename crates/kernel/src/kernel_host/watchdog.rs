//! 宿主看门狗（v0.28 · 台账 **D-115** · 冻结规格 STAGE-13 §3.3）。
//!
//! ⚠️ **全在宿主侧**（裁决 F）：M-12 的无进展守卫（VD-F3 闭环）只负责
//! **把活锁变成一个错误**（`Err(E_ENG_03_NO_PROGRESS)`）——**处置**是宿主
//! 的事：上报健康信号（M-22）、记一条 `WATCHDOG` 日志（M-21）、置幂等
//! 标志、终止内核。
//!
//! ⚠️ **窗口粒度 = 引擎的一个批次**（规格 §3.3）：
//!
//! - `step_in_chunks` 的**每一个 chunk**（`DEFAULT_INSTRUCTION_BUDGET`
//!   = 10 000 条指令）；
//! - `continue_until_event` 的**每一次调用**（M-17 ⇒ `engine.run_until(..)`
//!   跑满一个指令预算窗口）。
//!
//! ⚠️ **判定是纯函数**（[`classify_window`]）⇒ 不依赖真实时钟 ⇒ 可单测
//! （验收 **D-8**）。
//!
//! ⚠️ **宿主时钟不可用**时的那一套（台账 **D-135**）是
//! [`classify_window_with_clock`] ⇒ ⇒ 旧的 [`classify_window`] **一字未改**
//! （保留成薄封装 · 裁决 F）⇒ ⇒ 既有断言**一条不红**。

/// 宿主时间辅助预算（毫秒）—— LLD M-12 §3.5 的 `hostBudgetUs`（台账 D-115 的默认值）。
pub const HOST_WATCHDOG_BUDGET_MS: u64 = 200;

/// 滑动窗口确认次数 —— LLD M-12 §3.5「需连续 3 个检查窗口均无进展才触发」。
pub const HOST_WATCHDOG_CONFIRMATIONS: u32 = 3;

/// 看门狗日志的上下文标识（**12 字节**）。
///
/// ⚠️ M-21 的 `ctx` 上限是 **24 字节** ⇒ 这里**不会被截断**（验收 D-5）。
pub const WATCHDOG_LOG_CTX: &str = "ENG-WATCHDOG";

/// 看门狗日志 / 健康信号带的场景码（**9 字节**）。
///
/// ⚠️ M-21 的 `err_code` 上限是 **12 字节** ⇒ 这里**不会被截断**（验收 D-5）。
///
/// ⚠️ 它是**引擎侧**的停滞码（`E_ENG-03`），不是 HMS 自己的码 ——
/// HMS 的终止码是 `E_HMS-02`（由宿主上报，见规格 §8.1 的场景码分工）。
pub const WATCHDOG_ERR_CODE: &str = "E_ENG-03";

/// 一个检查窗口的判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOutcome {
    /// 有进展。
    Progress,
    /// 无进展（停滞或宿主时间超窗）。
    NoProgress,
}

/// **纯函数**：窗口判定（可单测，不依赖真实时钟）。
///
/// * `stalled` = 该批次报的是**停滞**（`E_ENG-03`）；
/// * `elapsed_ms` = 该批次耗掉的**宿主**时间。
///
/// ⚠️ 两条判据是**或**的关系：任一成立就算「这个窗口没有进展」。
///
/// ⚠️ **它保留成薄封装**（规格 §2.5 · 裁决 F）：它的**签名与行为一个字
/// 没改** ⇒ ⇒ 既有断言**一条不红** ⇒ ⇒ 它 ≡
/// [`classify_window_with_clock`]（`clock_available = true`）。
pub fn classify_window(elapsed_ms: u64, stalled: bool) -> WindowOutcome {
    classify_window_with_clock(elapsed_ms, stalled, true)
}

/// **纯函数**：窗口判定（**带宿主时钟可用性** · 台账 **D-135**）。
///
/// * `stalled` = 该批次报的是**停滞**（`E_ENG-03`）；
/// * `elapsed_ms` = 该批次耗掉的**宿主**时间；
/// * `clock_available` = 宿主单调时钟**读得到吗**（见
///   [`crate::kernel_host::clock::HostClock::is_available`]）。
///
/// ⚠️ **新语义**：`clock_available == false` ⇒ ⇒ 「**时间预算**」那一路
/// **恒不触发**（**不是**「按 0 算」）⇒ ⇒ 但「**引擎停滞**」那一路
/// **照常**（`stalled` 仍是或项）。
///
/// ⚠️ **为什么这样才对**：宿主时钟**读不到**时（wasm 侧环境**没有**
/// `performance`）⇒ ⇒ 耗时**恒为 0** ⇒ ⇒ 「按 0 算」与「恒不触发」在
/// **数值**上碰巧一样 ⇒ ⇒ 但**语义**上它们**不是**一回事 ⇒ ⇒ 「恒不
/// 触发」是**唯一安全**的选择（台账 D-135 做法 B 的口径）。
///
/// ⚠️ **native 侧逐字同结果**：`available` 恒 `true` ⇒ ⇒ 本函数与旧的
/// [`classify_window`] **逐字同结果** ⇒ ⇒ native 的行为**一个字不变**
/// （规格 §3.4 的**必要对照**）。
pub fn classify_window_with_clock(
    elapsed_ms: u64,
    stalled: bool,
    clock_available: bool,
) -> WindowOutcome {
    if stalled || (clock_available && elapsed_ms > HOST_WATCHDOG_BUDGET_MS) {
        WindowOutcome::NoProgress
    } else {
        WindowOutcome::Progress
    }
}

/// 宿主看门狗状态。
#[derive(Debug, Clone, Default)]
pub struct HostWatchdog {
    /// 连续无进展窗口数。
    streak: u32,
    /// 幂等标志（一次运行只处置一次）。
    fired: bool,
}

impl HostWatchdog {
    /// 造一个**干净**的看门狗（没有 streak、没有触发过）。
    pub fn new() -> Self {
        Self {
            streak: 0,
            fired: false,
        }
    }

    /// 喂一个窗口的结果。返回 `true` 表示**本次触发了**（且只在首次为 true）。
    ///
    /// ⚠️ **幂等**（验收 **D-4**）：触发过之后**永远**返回 `false` ⇒ 处置
    /// 链路只走一次 ⇒ `hms.report` 的调用次数不增。
    ///
    /// ⚠️ **有进展就把 streak 清零** ⇒ 「连续三个窗口」真的是**连续**的。
    pub fn observe(&mut self, outcome: WindowOutcome) -> bool {
        if self.fired {
            return false;
        }
        match outcome {
            WindowOutcome::Progress => {
                self.streak = 0;
                false
            }
            WindowOutcome::NoProgress => {
                self.streak = self.streak.saturating_add(1);
                if self.streak >= HOST_WATCHDOG_CONFIRMATIONS {
                    self.fired = true;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 当前连续无进展窗口数。
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// 是否已触发。
    pub fn fired(&self) -> bool {
        self.fired
    }
}

/// 一个窗口跑完之后，宿主该怎么走（宿主内部 · 不是公开 API）。
///
/// ⚠️ 它把「窗口跑完了」与「窗口报错了」**分开**——这正是裁决 G 的
/// 落点：停滞**不是**立即上抛的普通错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WindowVerdict {
    /// 窗口跑完了（可能有进展，也可能只是没有报错）。
    ///
    /// `executed` = 该批次退休的指令数（`Continue` 路径恒为 0 —— 它按
    /// **时间**驱动，不按指令数报账）。
    Completed { executed: u32 },
    /// 窗口**停滞**（引擎报 `E_ENG-03`），但**还没到**确认次数 ⇒ 宿主
    /// **吞掉**这个错误并继续跑（重入引擎）。
    Stalled,
    /// 看门狗**触发** ⇒ 处置已经做完 ⇒ 上抛终止错误。
    Fired(riscv_sim_contract::error::PlatformError),
    /// 窗口失败但**不是**停滞（别的错误码）⇒ 照旧上抛。
    Failed(riscv_sim_contract::error::PlatformError),
}

// ---------------------------------------------------------------------------
// 用例 · 宿主时钟**不可用**时的窗口判定（台账 **D-135**）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 宿主时钟**不可用** ⇒ ⇒ 「**时间预算**」那一路**恒不触发**。
    ///
    /// ⚠️ **判别力**：把 `clock_available &&` 去掉 ⇒ 本条立刻变红
    /// （`elapsed_ms` 超预算时仍判「有进展」）。
    #[test]
    fn the_time_budget_never_fires_when_the_clock_is_unavailable() {
        assert_eq!(
            classify_window_with_clock(HOST_WATCHDOG_BUDGET_MS + 1, false, false),
            WindowOutcome::Progress,
            "时钟不可用 ⇒ 「时间预算」那一路**恒不触发**（**不是**「按 0 算」）"
        );
        assert_eq!(
            classify_window_with_clock(u64::MAX, false, false),
            WindowOutcome::Progress,
            "时钟不可用 ⇒ 就算耗时是大数也不触发"
        );
        assert_eq!(
            classify_window_with_clock(0, false, false),
            WindowOutcome::Progress,
            "时钟不可用 ⇒ 读数为 0 时也不触发"
        );
    }

    /// 宿主时钟**不可用** ⇒ ⇒ 「**引擎停滞**」那一路**照常**。
    ///
    /// ⚠️ **判别力**：把整个判定写成 `clock_available && (..)` ⇒ 本条变红
    /// （停滞被**吞掉**了 ⇒ 活锁永远不触发 ⇒ 那是**安全方向**的漏洞）。
    #[test]
    fn the_stall_branch_still_fires_when_the_clock_is_unavailable() {
        assert_eq!(
            classify_window_with_clock(0, true, false),
            WindowOutcome::NoProgress,
            "时钟不可用**不得**吞掉「引擎停滞」"
        );
        assert_eq!(
            classify_window_with_clock(u64::MAX, true, false),
            WindowOutcome::NoProgress,
            "时钟不可用**不得**吞掉「引擎停滞」"
        );
    }

    /// 旧的 [`classify_window`] **一字未改** ⇒ ⇒ 它 ≡
    /// [`classify_window_with_clock`]（`clock_available = true`）。
    ///
    /// ⚠️ **判别力**：把薄封装改成 `clock_available = false` ⇒ 本条变红。
    #[test]
    fn the_legacy_function_is_the_new_one_with_an_available_clock() {
        for elapsed in [
            0u64,
            1,
            HOST_WATCHDOG_BUDGET_MS,
            HOST_WATCHDOG_BUDGET_MS + 1,
            u64::MAX,
        ] {
            for stalled in [false, true] {
                assert_eq!(
                    classify_window(elapsed, stalled),
                    classify_window_with_clock(elapsed, stalled, true),
                    "旧函数 ≡ 新函数(clock_available = true)：elapsed={elapsed} stalled={stalled}"
                );
            }
        }
    }

    /// 旧函数的**既有行为**（裁决 F · 既有断言一条不红）。
    #[test]
    fn the_legacy_function_keeps_its_behavior() {
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
}
