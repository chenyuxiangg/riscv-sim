//! 宿主侧降级执行器（v0.46 批 · STAGE-31 · 台账 **D-130** · 裁决 **J / M**）。
//!
//! ⚠️ **它只映射宿主「够得到」的旋钮**（事实 **F9**）⇒ ⇒ 不假装实现了
//! 做不到的事：
//!
//! | 动作 | 宿主够得到？ | 落点 |
//! | --- | --- | --- |
//! | `Terminate` | **只需表态** | 终止的**实体效果**由宿主回的 `E_HMS-02` 承载（裁决 **M**）⇒ ⇒ 执行器**不重复实现**终止逻辑 |
//! | `TraceDown` | ✅ | M-18 `set_rate`（采样变零开销空操作）＋ M-19 `set_overflow_policy`（**不因波形溢出而暂停仿真**） |
//! | `SnapshotRestore` | ❌ | `backend` / `snapshots` 是宿主**独占**字段 ⇒ ⇒ 执行器够不着 |
//! | `Pause` / `Disconnect` | ❌ | M-04 / M-07 是**前端**模块 ⇒ ⇒ 内核侧**结构上不可达** |
//!
//! ⚠️ 三档够不着的动作 ⇒ ⇒ **明确表态做不了**（`Unsupported`）⇒ ⇒ 那**不是
//! 失败** ⇒ ⇒ **不重试**、**不**报 `E_HMS-01`（裁决 **C**）。
//!
//! ⚠️ **执行器随句柄一起装**（裁决 **I**）⇒ ⇒ 它拿到的 M-18 / M-19 句柄
//! 与宿主**永远是同一份** ⇒ ⇒ **结构上不可能**持有过期句柄。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError, E_HMS_01_DEGRADE_FAILED};

use crate::m18_probe::{ProbeRegistry, SampleCfg};
use crate::m19_waveform::{OverflowPolicy, SharedPipeline};
use crate::m22_hms::{DegradeAction, DegradeAttempt, DegradeExecutor};

/// 降级派发记录（宿主与执行器**共享同一份** ⇒ 一个真相）。
///
/// ⚠️ 它**只记数 + 记最后一次动作** ⇒ ⇒ **不记内容** ⇒ ⇒ 它**不是**第二个
/// 真相源（只是「派发路径真的走过」这件事的**可观测出口** · 验收 **C-1** /
/// **C-3**）。
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct DegradeDispatch {
    /// 执行器被问过的**总次数**。
    pub(super) total: u32,
    /// 最后一次被问到的动作。
    pub(super) last: Option<DegradeAction>,
}

/// 宿主侧降级执行器（把 [`DegradeAction`] 翻译到宿主**够得到**的旋钮）。
pub(super) struct HostDegradeExecutor {
    /// M-18 探针注册表（共享句柄 ⇒ 与宿主**同一份**）。
    probes: Rc<RefCell<ProbeRegistry>>,
    /// M-19 波形管道（共享句柄 ⇒ 与宿主**同一份**）。
    waveform: SharedPipeline,
    /// 派发记录（共享同一份 ⇒ 一个真相）。
    dispatch: Rc<RefCell<DegradeDispatch>>,
}

impl HostDegradeExecutor {
    /// 造一个执行器（宿主装配用 —— 裁决 **I**：**随句柄一起装**）。
    pub(super) fn new(
        probes: Rc<RefCell<ProbeRegistry>>,
        waveform: SharedPipeline,
        dispatch: Rc<RefCell<DegradeDispatch>>,
    ) -> Self {
        Self {
            probes,
            waveform,
            dispatch,
        }
    }

    /// `TraceDown` 的**两个旋钮**（M-18 采样 + M-19 溢出策略）。
    ///
    /// ⚠️ **不暂停仿真**：M-19 的溢出策略改成 `DropOldestMark` ⇒ ⇒ 波形环
    /// 满了就丢最旧的（并标记）⇒ ⇒ **不**请求暂停仿真。
    ///
    /// ⚠️ **默认策略本来就是** `DropOldestMark`（事实 **F11**）⇒ ⇒ 这一步在
    /// 默认配置下是**幂等设置**⇒ ⇒ 它的判别力必须靠「**先**设成 `PauseSim`、
    /// **再**降级」才看得出来（§9.3 第 2 条）。
    fn trace_down(&self) -> DegradeAttempt {
        // ① M-18：采样变**零开销空操作**（`master_enable = false`）。
        //
        // ⚠️ **用非阻塞式借用**：RefCell 争用会 panic ⇒ ⇒ 在 WASM 里 panic =
        // Worker 崩 ⇒ ⇒ 争用时**如实说「没做成」**（`Failed`）而不是**假装
        // 做到了**。
        let Ok(mut registry) = self.probes.try_borrow_mut() else {
            return DegradeAttempt::Failed(PlatformError::new(
                ErrorFamily::Health,
                E_HMS_01_DEGRADE_FAILED,
                "降级动作 TraceDown 未完成：M-18 探针注册表正被占用（RefCell 争用）",
            ));
        };
        registry.set_rate(SampleCfg {
            master_enable: false,
            enable: 0,
            throttle_n: 1,
        });
        drop(registry);

        // ② M-19：**不因波形溢出而暂停仿真**。
        let Ok(mut pipeline) = self.waveform.try_pipeline_mut() else {
            return DegradeAttempt::Failed(PlatformError::new(
                ErrorFamily::Health,
                E_HMS_01_DEGRADE_FAILED,
                "降级动作 TraceDown 未完成：M-19 波形管道正被占用（RefCell 争用）",
            ));
        };
        pipeline.set_overflow_policy(OverflowPolicy::DropOldestMark);
        DegradeAttempt::Done
    }
}

impl DegradeExecutor for HostDegradeExecutor {
    fn try_degrade(&mut self, action: DegradeAction) -> DegradeAttempt {
        // ⚠️ **每次被问都要记进派发记录** ⇒ ⇒ 「降级真的走了派发路径」这件事
        // 在宿主层**看得见**（验收 C-1 / C-3）⇒ ⇒ 这不是「M-22 自己硬编码
        // 处理」的形状。
        {
            let mut dispatch = self.dispatch.borrow_mut();
            dispatch.total += 1;
            dispatch.last = Some(action);
        }
        match action {
            // ⚠️ 裁决 **M**：`Terminate` **只表态** ⇒ ⇒ 终止的**实体效果**仍由
            // 宿主回的 `E_HMS-02` 承载 ⇒ ⇒ 这里**不重复实现**终止逻辑
            // （那会造出第二个真相：谁说「已终止」）。
            DegradeAction::Terminate => DegradeAttempt::Done,
            // ⚠️ 裁决 **J**：`TraceDown` 只映射**够得到**的两个旋钮。
            DegradeAction::TraceDown => self.trace_down(),
            // ⚠️ 三档够不着的 ⇒ ⇒ **明确表态做不了** ⇒ ⇒ `Unsupported`
            // ⇒ ⇒ **不是失败** ⇒ ⇒ 不重试、不报 `E_HMS-01`（裁决 **C**）。
            DegradeAction::SnapshotRestore | DegradeAction::Pause | DegradeAction::Disconnect => {
                DegradeAttempt::Unsupported
            }
            // ⚠️ `None` = 「**不做**降级」⇒ ⇒ M-22 走 `Skipped`、**根本不问
            // 执行器** ⇒ ⇒ 这一支**结构上不可达**（§9.3 第 3 条）⇒ ⇒ 这里
            // 只能表态「做不了」。
            DegradeAction::None => DegradeAttempt::Unsupported,
        }
    }
}
