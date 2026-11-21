//! 可观测只读面：M-22 健康 / 降级计数与动作、M-21 日志环快照。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::KernelHost;
use crate::m21_logging::{LogEntry, LogQuery};
use crate::m22_hms::{DegradeAction, Health};
use riscv_sim_contract::error::PlatformError;

impl KernelHost {
    /// 整体健康状态（M-22 派生 · v0.28 · D-115）。
    ///
    /// ⚠️ **派生**语义：有 FATAL 信号 ⇒ `Critical`；只有 CRITICAL / WARNING
    /// ⇒ `Degraded`；否则 `Ok` ⇒ **只降不升**。
    pub fn health(&self) -> Health {
        self.hms.state()
    }

    /// 看门狗是否已经触发过（幂等标志 · M-22）。
    pub fn watchdog_fired(&self) -> bool {
        self.hms.watchdog_fired()
    }

    /// 降级执行器被问过的**总次数**（v0.46 · STAGE-31 · 验收 **C-1 / C-3**）。
    ///
    /// ⚠️ **零** = 降级**没有**走派发路径（M-22 走了自己的内置实现，或者宿主
    /// **没装**执行器）⇒ ⇒ 这个数是「降级真的问过执行器」的**唯一**证据。
    ///
    /// ⚠️ **它数的是「被问了几次」** ⇒ ⇒ 失败会**重试** ⇒ ⇒ 一次失败的降级
    /// 会让这个数**大于 1** ⇒ ⇒ 「不重试」那条判据就是断言它**恰好 1**。
    pub fn degrade_dispatch_total(&self) -> u32 {
        self.degrade_dispatch.borrow().total
    }

    /// 最后一次被问到的**降级动作**（v0.46 · STAGE-31 · 验收 **C-1 / C-3**）。
    ///
    /// ⚠️ `None` = 执行器**一次都没被问过** ⇒ ⇒ 降级**没有**走派发路径。
    pub fn degrade_dispatch_last(&self) -> Option<DegradeAction> {
        self.degrade_dispatch.borrow().last
    }

    /// M-22 的**降级失败**记录（`E_HMS-01` 那一族 · v0.46 · STAGE-31 · 验收 **C-3**）。
    ///
    /// ⚠️ **一次运行只报一次** ⇒ ⇒ 空 ⇒ ⇒ 「降级**没有**失败」⇒ ⇒
    /// `Unsupported`（做不了）**不是**失败 ⇒ ⇒ 它**不会**出现在这里。
    ///
    /// ⚠️ 这条让「降级失败」这件事**在宿主层看得见**（不是只活在 M-22 内部）。
    pub fn degrade_failures(&self) -> &[PlatformError] {
        self.hms.degrade_failures()
    }

    /// M-21 日志环的快照（管理员通道的读出口 · v0.28 · D-115）。
    ///
    /// ⚠️ **纯读**：不改缓冲、不动计数。
    /// ⚠️ 借不到缓冲（别处正持有着可变借用）⇒ **退化成空列表**而不是
    /// panic —— 诊断路径不该把内核打挂。
    pub fn log_entries(&self) -> Vec<LogEntry> {
        self.log_buffer
            .try_borrow()
            .map(|buffer| buffer.query(&LogQuery::default()))
            .unwrap_or_default()
    }
}
