# STAGE-31 · 第 30 批（v0.46）· 降级机制正确化批（D-130）

> **状态**：**冻结**（本批唯一的规格权威）⇒ ⇒ 本文件在批末**一个字不改**
> ⇒ ⇒ 施工期的一切变化只记在 **§8 订正**里。
>
> **上游**：STAGE-30（v0.45 · 角色准入收口批）⇒ ⇒ 本批**不改** STAGE-30 的规格
> （只定口径、不搬文本）。
>
> **台账**：`docs/reviews/REVIEWS.md` 的 **D-130**（`⏳ **未闭环**`）。

---

## §0 · 本批定位

**主题**：把「降级」这件事从「**只有一个动作真的实现了**」推进到「**机制正确 +
够得到的旋钮真能用**」。

- **做**：**机制正确化**（执行器接口从两态改三态 ⇒ ⇒ 「未实现」由**执行器**表态
  ⇒ ⇒ 重试与终态记账**通用化** ⇒ ⇒ 降级在宿主里**只有一个入口**）**加上**
  **够得到的那两个旋钮**（M-18 采样 + M-19 溢出策略 ⇒ ⇒ 即 `TraceDown`）。
- **不做**：**宿主大重构**（`backend` / `snapshots` / `trace` 不改 `Rc<RefCell<>>`）
  ⇒ ⇒ `SnapshotRestore` / `Pause` / `Disconnect` **三档本批够不着** ⇒ ⇒ 逐条
  给**结构性理由**与 **C 做法候选**（台账 D-130 末列）。
- **契约层零升版**（`SCHEMA_VERSION` 仍 **8** / `SIM_STATE_SCHEMA_VERSION` 仍 **2**
  / `PLATFORM_SCHEMA_VERSION` 仍 **1** / `PLATFORM_RULES_VERSION` 仍 **3**）
  ⇒ ⇒ **零**新 DTO / **零**新命令 / **零**新事件 ⇒ ⇒ **前端零改动**。
- **D-130 不销账** ⇒ ⇒ 按「**一条欠账整体没有闭环就不减**」⇒ ⇒ **真实未闭环
  62 → 62 不变**。

⚠️ **为什么不做宿主大重构**：把宿主三个独占字段改成共享可变大改的是**依赖方向**
⇒ ⇒ 「宿主能引用自己的后端」这件事一旦成立，**降级执行器就能碰到一切** ⇒ ⇒
「够不着」这个诚实边界就会消失 ⇒ ⇒ 但那时**降级执行器**与**宿主本体**的边界
也一起消失了 ⇒ ⇒ 收益（多三档）与代价（宿主形态被降级机制绑架）**不成比例**
⇒ ⇒ **本批不换**。

---

## §1 · 实测事实（**逐条实测**，不是推断）

> ⚠️ 每条事实都必须**能在当前树上复现** ⇒ ⇒ **落点列给的是文件 + 标识符**，
> 不是行号（行号会漂）。

### §1.1 · 口径

- 「**够得到**」= 执行器（住在 `Hms` 内部）**能拿到宿主那份对象的同一个句柄**
  ⇒ ⇒ 判据 = **宿主字段是不是共享句柄**（`Rc<RefCell<…>>` / 可克隆的
  `SharedPipeline`）⇒ ⇒ **独占**字段（`backend` / `snapshots` / `trace`）
  ⇒ ⇒ **够不着**。
- 「**走派发路径**」= `Hms::degrade` 把动作**问**了执行器 ⇒ ⇒ 与「M-22 自己
  硬编码处理」**相对**。

### §1.2 · 事实表

| 编号 | 事实 | 落点 |
| --- | --- | --- |
| **F1** | `DegradeExecutor::try_degrade` 的返回类型是 `Result<(), PlatformError>` ⇒ ⇒ **两态** ⇒ ⇒ 执行器**无法表态「这个动作我实现不了」** ⇒ ⇒ 「未实现」只能由 M-22 **硬编码**产出 | `crates/kernel/src/m22_hms/mod.rs` · `pub trait DegradeExecutor` / `fn try_degrade` |
| **F2** | `Hms::degrade` 的 `match` 只有两个分支：`Terminate` ⇒ 走 `degrade_terminate`；**其余一律** `Unsupported` ⇒ ⇒ ⇒ **执行器永远只被问到 `Terminate`** ⇒ ⇒ ⇒ 注入式执行器**名存实亡** | `crates/kernel/src/m22_hms/mod.rs` · `pub fn degrade` |
| **F3** | 重试循环**写死**在 `Terminate` 那条路径里（`while tries < DEGRADE_MAX_TRIES`）⇒ ⇒ 且终态上报的消息**写死「降级动作 Terminate」** ⇒ ⇒ ⇒ 换成别的动作 ⇒ ⇒ **重试与记账都不成立** | `crates/kernel/src/m22_hms/mod.rs` · `pub const DEGRADE_MAX_TRIES` / `fn report_degrade_failure` |
| **F4** | `DEGRADE_MAX_TRIES = 3`（口径：**至多 3 次尝试**，不是「1 次 + 3 次重试」）⇒ ⇒ 终态上报时 `tries == 3` | `crates/kernel/src/m22_hms/mod.rs` · `pub const DEGRADE_MAX_TRIES` |
| **F5** | M-22 层**已有**两条相关用例：`c10_unimplemented_action_is_unsupported_and_never_retried`（未实现 ⇒ 不重试、不报 `E_HMS-01`）与 `c11_failing_executor_is_retried_three_times_and_reports_once`（重试**恰好 3 次** + **恰好一次** `E_HMS-01`）⇒ ⇒ ⚠️ 但 c10 测的是**硬编码**分支（执行器**根本没被问到**）⇒ ⇒ ⇒ 「执行器表态未实现」这条**从未被测过** | `crates/kernel/src/m22_hms/mod.rs` · `fn c10_unimplemented_action_is_unsupported_and_never_retried` / `fn c11_failing_executor_is_retried_three_times_and_reports_once` |
| **F6** | 宿主**从不**调用 `install_executor` ⇒ ⇒ ⇒ `DegradeExecutor` 在生产里是**死代码** ⇒ ⇒ ⇒ 台账「宿主侧零覆盖」那条**成立** | `crates/kernel/src/m22_hms/mod.rs` · `pub fn install_executor` 与 `crates/kernel/src/kernel_host/mod.rs` |
| **F7** | 宿主**绕过策略**直接调 `self.hms.degrade(DegradeAction::Terminate)`（动作**写死**）⇒ ⇒ ⇒ `Hms::report` 里的策略分支（`degrade_policy` 默认为空）在生产里**永不触发** ⇒ ⇒ ⇒ 「查策略」这件事**在生产里没发生** | `crates/kernel/src/kernel_host/mod.rs` · `fn handle_watchdog_trigger` |
| **F8** | 宿主的处置顺序是 ① `hms.report` ② M-21 日志 ③ `mark_watchdog_fired()` ④ **写死的** `hms.degrade(Terminate)` ⑤ `terminated_error` ⇒ ⇒ ⇒ 降级发生在**幂等标志置位之后** ⇒ ⇒ 而 `report` 的降级闸门是 `!watchdog_fired` ⇒ ⇒ ⇒ 策略分支要生效 ⇒ ⇒ 宿主必须**先设策略**、**不自己调** `degrade` | `crates/kernel/src/kernel_host/mod.rs` · `fn handle_watchdog_trigger` |
| **F9** | 宿主的**够得到的**旋钮只有两个：M-18 `probes: Option<Rc<RefCell<ProbeRegistry>>>` 与 M-19 `waveform: Option<SharedPipeline>`（**两者都可克隆** ⇒ ⇒ 与宿主**同一份**）⇒ ⇒ 而 `backend` / `snapshots` / `trace` 是**独占**字段 ⇒ ⇒ ⇒ 执行器**够不着**「恢复会话」与「关 M-11 轨迹环」 | `crates/kernel/src/kernel_host/mod.rs` · `probes` / `waveform` / `backend` / `snapshots` / `trace` |
| **F10** | M-18 的采样配置有**公开读取口** `ProbeRegistry::sample_cfg()`；M-19 的溢出策略有 `WaveformPipeline::overflow_policy()` ⇒ ⇒ ⇒ 「采样被关掉」「策略被改回」两件事**可观测** | `crates/kernel/src/m18_probe/registry.rs` · `pub fn sample_cfg` 与 `crates/kernel/src/m19_waveform/pipeline.rs` · `pub fn overflow_policy` |
| **F11** | M-19 的 `OverflowPolicy` **默认就是 `DropOldestMark`** ⇒ ⇒ ⇒ 「降级时把溢出策略设成 `DropOldestMark`」在默认配置下是**幂等设置** ⇒ ⇒ ⇒ 它的判别力**必须**靠「先把策略设成 `PauseSim`、再降级」才看得出来 ⇒ ⇒ ⇒ 否则那条断言**恒真** | `crates/kernel/src/m19_waveform/channel.rs` · `pub enum OverflowPolicy` |
| **F12** | `install_probe_pipeline` 是 M-18 / M-19 句柄的**唯一产生点**（`KernelHost::new()` 必经；`rebuild_platform` 会重装一遍）⇒ ⇒ ⇒ 降级执行器**随句柄一起重装** ⇒ ⇒ ⇒ 结构上**不可能**持有过期句柄 | `crates/kernel/src/kernel_host/mod.rs` · `fn install_probe_pipeline` |
| **F13** | `DegradeOutcome` 在**全仓**只被 M-22 自己匹配（**零**外部用法）⇒ ⇒ ⇒ 给它加一个新变体是**仓内**破坏性变更、**不是**跨 crate 破坏 ⇒ ⇒ ⇒ 契约层零影响 | `crates/kernel/src/m22_hms/mod.rs` · `pub enum DegradeOutcome` |
| **F14** | `PlatformError` 派生了 `Clone` / `PartialEq` / `Eq` ⇒ ⇒ ⇒ 「失败」那一态**可以带明细**而不丢 `Clone` / `Eq` ⇒ ⇒ ⇒ 执行器的三态**不必**用 `Option<PlatformError>` 兼职 | `crates/contract/src/error.rs` · `pub struct PlatformError` |
| **F15** | 契约层零升版可行：四个版本常量本批**一个字都不动** ⇒ ⇒ ⇒ 降级的宿主侧实现**不需要**新命令 / 新事件 ⇒ ⇒ ⇒ `Pause` / `Disconnect` 的**跨边界**部分（要 M-04 / M-07 ⇒ ⇒ **前端**模块）本批**做不了**、**也不需要** | `crates/contract/src/lib.rs` · `SCHEMA_VERSION` |

---

## §2 · 现状盘点（**哪一步已经做完了**）

| 台账 D-130 做法 | 状态 | 依据 |
| --- | --- | --- |
| 做法 A（`Hms` 持 `Option<Box<dyn DegradeExecutor>>`） | ✅ **已落地**（形态早就在）⇒ ⇒ ⚠️ 但**接口只有两态** ⇒ ⇒ 「未实现」只能硬编码 ⇒ ⇒ ⇒ 形态在、**语义不对** | F1 / F6 |
| 做法 B（宿主实现执行器，映射到 M-20 / M-04 / M-07 / M-19） | ❌ **未做**（宿主**零**注入点）⇒ ⇒ 本批做**够得到的那一半**（M-18 + M-19） | F6 / F9 / F12 |
| 做法 C（每个动作补一条端到端用例） | ❌ **未做**（仓内零覆盖）⇒ ⇒ 本批补**够得到的那一档** + M-22 层的通用路径 | F5 / F9 |
| 陷阱①（`Unsupported` 不是失败 ⇒ 不重试、不报 `E_HMS-01`） | ⚠️ **半满足**：`Unsupported` 确实不重试、不报码 ⇒ ⇒ 但它**只**由 M-22 硬编码 ⇒ ⇒ ⇒ 执行器**无法**表态 ⇒ ⇒ ⇒ 「不是失败」这条**没有第二个来源** | F1 / F2 / F5 |
| 陷阱②（`Terminate` 是终态、`Pause` / `Disconnect` 可恢复 ⇒ 宿主执行器不能一刀切） | ⚠️ **半满足**：M-22 只认 `Terminate` ⇒ ⇒ 谈不上「一刀切」⇒ ⇒ ⇒ 但也谈不上「区分」⇒ ⇒ ⇒ 本批把区分**显式化** | F2 / F3 |
| 陷阱③（失败 ⇒ 重试至多 3 次 ⇒ 终态**恰好一次** `E_HMS-01`） | ✅ **M-22 层已满足**（`c11`）⇒ ⇒ ⚠️ 但**只对 `Terminate`** ⇒ ⇒ ⇒ 通用化之后这条**对所有动作成立** | F3 / F4 / F5 |
| 判别力清单①（假执行器失败 ⇒ 重试恰好 3 次 + `E_HMS-01` 只报一次） | ✅ **M-22 层已覆盖**（`c11`）⇒ ⇒ ⚠️ **宿主层测不了**（宿主执行器不会失败） | F5 / F6 |
| 判别力清单②（动作 `Unsupported` ⇒ 不重试且不报 `E_HMS-01`） | ⚠️ **只覆盖硬编码分支**（`c10`）⇒ ⇒ ⇒ 「执行器表态未实现」**从未被测** | F1 / F5 |

---

## §3 · 做法

### §3.1 · 契约层（**零改动**）

- `crates/contract/**` **零 diff** ⇒ ⇒ 四个版本常量**一个字都不动**
  （A-1 / A-2）。
- ⚠️ **为什么不需要新命令 / 新事件**：本批的降级**全部在宿主进程内**完成
  ⇒ ⇒ 「降级发生了」这件事由**宿主既有**的 `E_HMS-02` 承载 ⇒ ⇒ 前端零改动
  （A-3）。

### §3.2 · M-22 侧（`crates/kernel/src/m22_hms/mod.rs` · **Lane A 独占**）

#### 3.2.1 · 执行器接口改成**三态**（B-1）

```rust
/// 降级动作执行器的**一次尝试**结果（**三态**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradeAttempt {
    /// 动作**执行成功**。
    Done,
    /// 动作**未实现**（这套装配做不了）⇒ ⇒ **不是失败**：不重试、**不**报 `E_HMS-01`。
    Unsupported,
    /// 动作**失败**（带明细）⇒ ⇒ 上层**重试**（至多 3 次）。
    Failed(PlatformError),
}

/// 降级动作执行器（**可注入**，宿主装配）。
pub trait DegradeExecutor {
    /// 执行一次降级动作。
    fn try_degrade(&mut self, action: DegradeAction) -> DegradeAttempt;
}
```

⚠️ **接口逐字钉死**（Lane B 按这一份实现 ⇒ ⇒ 两条 lane 可并行）。

#### 3.2.2 · `degrade` 改成**通用路径**（B-2 / B-5 / B-6）

```
degrade(action)
├─ action == None            ⇒ Skipped      （**不做降级**；**不**问执行器）
├─ action == Terminate
│   └─ 没有执行器            ⇒ 内置实现（置 terminated）⇒ Ok { tries: 1 }
├─ 没有执行器
│   └─ action != Terminate   ⇒ Unsupported  （**做不了**；**不是失败** ⇒ 不重试、不报码）
└─ 有执行器
    └─ 问执行器（至多 3 次）
        ├─ Done              ⇒ Ok { action, tries }（**只有 Terminate 才置 terminated**）
        ├─ Unsupported       ⇒ Unsupported { action }（**不重试**、**不**报码）
        └─ Failed(e)         ⇒ 重试；用尽 ⇒ Failed { action, tries: 3 }
                                 ⇒ **恰好一次** `E_HMS-01`（消息里的动作名按实际动作）
```

- **B-5**：`DegradeAction::None` ⇒ 新增 `DegradeOutcome::Skipped { action }`
  ⇒ ⇒ **不**复用 `Unsupported`（`Unsupported` 的含义被本批钉死成「**未实现**」）
  ⇒ ⇒ ⇒ 「不做降级」与「做不了」**两个意思两个名字**。
- **B-6**：**只有** `Terminate` 会置 `terminated` ⇒ ⇒ 其它动作（可恢复的那些）
  ⇒ ⇒ ⇒ **不**置 ⇒ ⇒ ⇒ 陷阱②的「终态 vs 可恢复」**在数据上分得开**。

#### 3.2.3 · 文档措辞订正（B-7）

M-22 里**五组旧措辞**（共 **8 处**出现）**全部**改成新语义：

| 组 | 旧措辞（**一个字都不许留**） | 出现在 |
| --- | --- | --- |
| ① | 「降级动作本批只实现 `Terminate`」 | 模块头 |
| ② | 「本批**未实现**」 | `DegradeAction` 的四个变体文档 |
| ③ | 「本批唯一有实现的动作」 | `DegradeAction::Terminate` 的文档 |
| ④ | 「只有 `Terminate` 有实现」 | `DegradeOutcome::Unsupported` 与 `degrade` 的文档 |
| ⑤ | 「本批只落 `Terminate`」 | `DegradeExecutor` trait 的文档 |

⚠️ **不许留旧句原文**（那会让下一批的判据踩空 ⇒ ⇒ 判据会命中「文档里说本批
只实现了一个动作」这句**已经过时**的话）。

### §3.3 · 宿主侧（`crates/kernel/src/kernel_host/mod.rs` + `degrade.rs` · **Lane B 独占**）

#### 3.3.1 · 宿主侧降级执行器（C-1）

```rust
/// 降级派发记录（宿主与执行器**共享同一份** ⇒ 一个真相）。
#[derive(Debug, Clone, Copy, Default)]
pub struct DegradeDispatch {
    /// 执行器被问过的**总次数**。
    pub total: u32,
    /// 最后一次被问到的动作。
    pub last: Option<DegradeAction>,
}

/// 宿主侧降级执行器（把 `DegradeAction` 翻译到宿主**够得到**的旋钮）。
pub struct HostDegradeExecutor {
    /// M-18 探针注册表（共享句柄 ⇒ 与宿主**同一份**）。
    probes: Rc<RefCell<ProbeRegistry>>,
    /// M-19 波形管道（共享句柄 ⇒ 与宿主**同一份**）。
    waveform: SharedPipeline,
    /// 派发记录（共享同一份 ⇒ 一个真相）。
    dispatch: Rc<RefCell<DegradeDispatch>>,
}
```

- `Terminate` ⇒ **表态** `Done`（**不重复实现**终止逻辑 ⇒ 见裁决 M）。
- `TraceDown` ⇒ **两个旋钮**：① M-18 `set_rate(SampleCfg { master_enable: false,
  .. })`（⇒ `sample` 变成零开销空操作）② M-19
  `set_overflow_policy(OverflowPolicy::DropOldestMark)`（⇒ **不因波形溢出而暂停
  仿真**）⇒ ⇒ `Done`。
- `SnapshotRestore` / `Pause` / `Disconnect` ⇒ `Unsupported`（**明确表态**
  「这套装配做不了」⇒ ⇒ 不是失败 ⇒ ⇒ 不重试、不报码）。

#### 3.3.2 · 接线（C-2）

- **装配**：宿主在 `install_probe_pipeline` 的**末尾**（句柄产生之后）装执行器
  ⇒ ⇒ 句柄换新时**连执行器一起换** ⇒ ⇒ **结构上不可能**持有过期句柄（F12 /
  裁决 I）。
- **默认策略**：宿主在装配时 `set_degrade_policy(Severity::Fatal,
  DegradeAction::Terminate)` ⇒ ⇒ **默认行为一字不变**（C-6 / 裁决 H）。
- **删掉写死的那行**：`handle_watchdog_trigger` 里那行 `hms.degrade(Terminate)`
  **删掉** ⇒ ⇒ 降级在宿主里**只有一个入口**（M-22 `report` 里的策略分支）
  ⇒ ⇒ **杜绝**「策略分支 + 宿主直做」并存导致重复降级（裁决 G）。

#### 3.3.3 · 可观测面（C-3）

- `KernelHost::degrade_dispatch_total()` ⇒ 执行器被问过的总次数。
- `KernelHost::degrade_dispatch_last()` ⇒ 最后一次被问到的动作。
- `KernelHost::degrade_failures()` ⇒ M-22 的降级失败记录（`E_HMS-01` 那一族）
  ⇒ ⇒ 让「降级失败」这件事**在宿主层看得见**（不是只活在 M-22 内部）。

### §3.4 · 宿主层判别力用例（`crates/kernel/src/kernel_host/tests_degrade.rs` · **Lane B 独占**）

| 编号 | 用例 | 断言什么 |
| --- | --- | --- |
| C-3a | 看门狗触发 ⇒ 走**派发路径** | `degrade_dispatch_last() == Some(Terminate)`、`degrade_dispatch_total() == 1`、`hms.terminated()` |
| C-4a | `TraceDown` ⇒ M-18 采样**真的关了** | `probes.sample_cfg().master_enable == false` |
| C-4b | `TraceDown` ⇒ M-19 溢出策略**不暂停仿真** | 先把策略设成 `PauseSim` ⇒ 降级后读回 `DropOldestMark`（⚠️ 默认值已是它 ⇒ 必须**先改成 `PauseSim`** 才看得出 ⇒ 见 F11） |
| C-4c | `TraceDown` **不**置 `terminated` | 可恢复动作**不是终态**（陷阱②） |
| C-5 | 三档够不着的动作 ⇒ `Unsupported` | 不重试（`degrade_dispatch_total() == 1`）、不报 `E_HMS-01`（`degrade_failures()` 为空） |
| C-6 | 默认行为**一字不变** | 看门狗触发后宿主回的**仍是** `E_HMS-02`（回归） |

### §3.5 · 前端与工具（**零改动**）

- `front/**` **零 diff**（A-3）。
- `tools/**`：只有 `tools/stage31_check.py` 是**新增**（统筹方写）⇒ ⇒ 既有工具
  **零改动**。

---

## §4 · 文件所有权（lane 切分）

| Lane | 文件（**独占**） | 做什么 |
| --- | --- | --- |
| **A** | `crates/kernel/src/m22_hms/mod.rs` | §3.2 全部（三态接口 · 通用 `degrade` · `Skipped` · 文档措辞 · M-22 层用例） |
| **B** | `crates/kernel/src/kernel_host/mod.rs` · `crates/kernel/src/kernel_host/degrade.rs`（新） · `crates/kernel/src/kernel_host/tests_degrade.rs`（新） · `crates/kernel/src/kernel_host/tests_watchdog.rs` | §3.3 + §3.4 全部 |
| **统筹方** | `docs/detailed-design/STAGE-31_V046_Degrade_Mechanism_D130.md` · `tools/stage31_check.py` · `docs/reviews/REVIEWS.md` · `README.md` · `AGENTS.md` · `PROGRESS.md` | 规格 · 自检 · 台账 · 文档收口 |

⚠️ **lane 之间的接口** = §3.2.1 的 trait 签名 ⇒ ⇒ **逐字钉死** ⇒ ⇒ 两条 lane
可**并行**（Lane B 按签名实现，不等 Lane A 落地）。

---

## §5 · 验收

### §5.1 · A 组（契约层与版本）

| 编号 | 验收 |
| --- | --- |
| **A-1** | 四个版本常量**逐字不变**：`SCHEMA_VERSION = 8` / `SIM_STATE_SCHEMA_VERSION = 2` / `PLATFORM_SCHEMA_VERSION = 1` / `PLATFORM_RULES_VERSION = 3` |
| **A-2** | `crates/contract/**` **零 diff** |
| **A-3** | `front/**` **零 diff** |

### §5.2 · B 组（M-22 侧）

| 编号 | 验收 |
| --- | --- |
| **B-1** | `DegradeExecutor::try_degrade` 返回 `DegradeAttempt`（**三态**）⇒ ⇒ `DegradeAttempt` 带 `Done` / `Unsupported` / `Failed(PlatformError)` 三个变体 |
| **B-2** | `Hms::degrade` 对**每个**动作（除 `None`）都**问执行器** ⇒ ⇒ 执行器被问到 `Terminate` 以外的动作时**也会被问到**（不是硬编码 `Unsupported`） |
| **B-3** | 执行器表态 `Unsupported` ⇒ ⇒ **不重试**（执行器**只被问一次**）⇒ ⇒ **不**报 `E_HMS-01` |
| **B-4** | 执行器表态 `Failed` ⇒ ⇒ 重试**恰好 3 次** ⇒ ⇒ **恰好一次** `E_HMS-01` ⇒ ⇒ 消息里的动作名是**实际动作**（不是写死的 `Terminate`） |
| **B-5** | `DegradeAction::None` ⇒ ⇒ `DegradeOutcome::Skipped { action: None }` ⇒ ⇒ **不**问执行器 ⇒ ⇒ **不**是 `Unsupported` |
| **B-6** | **只有** `Terminate` 会置 `terminated` ⇒ ⇒ 其它动作成功**不置** |
| **B-7** | §3.2.3 的**五处**文档措辞**全部**改成新语义 ⇒ ⇒ **旧句原文一个都不许留** |

### §5.3 · C 组（宿主层）

| 编号 | 验收 |
| --- | --- |
| **C-1** | 宿主**装了**降级执行器 ⇒ ⇒ 降级**走派发路径**（`degrade_dispatch_last()` 非空） |
| **C-2** | 宿主**删掉**写死的那行 `hms.degrade(Terminate)` ⇒ ⇒ 降级**只有一个入口**（M-22 的策略分支）⇒ ⇒ 且宿主**设了默认策略** `Fatal → Terminate` |
| **C-3** | 看门狗触发 ⇒ ⇒ 派发 `Terminate` **恰好一次** ⇒ ⇒ `hms.terminated()` 为真 |
| **C-4** | `TraceDown` ⇒ ⇒ M-18 采样**真的关了**（`sample_cfg().master_enable == false`）⇒ ⇒ 且 M-19 溢出策略**是** `DropOldestMark`（**先设成 `PauseSim`** 再降级）⇒ ⇒ 且**不**置 `terminated` |
| **C-5** | `SnapshotRestore` / `Pause` / `Disconnect` ⇒ ⇒ 执行器表态 `Unsupported` ⇒ ⇒ **不重试**（只被问一次）⇒ ⇒ **不**报 `E_HMS-01` |
| **C-6** | **默认行为一字不变**：看门狗触发后宿主回的**仍是** `E_HMS-02`（`E_HMS_02_FATAL_TERMINATED`） |

### §5.4 · T 组（台账与文档）

| 编号 | 验收 |
| --- | --- |
| **T-1** | 台账 **D-130** 末列**按 C 做法级别**更新（根因链 / 分条做法 + 工作量 / 落地陷阱 / 判别力清单 / 破坏性变更预警）⇒ ⇒ **问题列一个字不动** |
| **T-2** | 台账 **D-130** 标记**仍是** `⏳ **未闭环**` ⇒ ⇒ **真实未闭环 62 不变** ⇒ ⇒ §0.1 合计三个数**不变** |
| **T-3** | `README.md` / `AGENTS.md` / `PROGRESS.md` 的**数字从当前树重算**（Rust 条数 / 前端条数 / 文档篇数 / 规格份数）⇒ ⇒ 且新增 **STAGE-31** 条目 |
| **T-4** | `tools/stage31_check.py` **存在**且**退出码 0** ⇒ ⇒ 且**十七 → 十八**个 stage 自检全部 0 红 |

### §5.5 · X 组（已知边界，如实登记）

| 编号 | 边界 |
| --- | --- |
| **X-1** | **M-11 轨迹环关不掉**：`trace: TraceBuffer` 是宿主**独占**字段 ⇒ ⇒ 执行器够不着 ⇒ ⇒ `TraceDown` 只关**采样**与**溢出策略** ⇒ ⇒ 「关 trace」这件事**只做了一半** |
| **X-2** | **`SnapshotRestore` 够不着**：`backend` / `snapshots` 是宿主**独占**字段 ⇒ ⇒ 恢复会话**只有** `KernelHost::restore_session`（宿主自己调）⇒ ⇒ 执行器**够不到** ⇒ ⇒ 该档本批**不做** |
| **X-3** | **`Pause` / `Disconnect` 是跨边界的**：M-04 会话与 M-07 传输是**前端**模块 ⇒ ⇒ 内核侧**结构上不可达** ⇒ ⇒ 本批**不做** ⇒ ⇒ 要做**必须**升契约版本（`SCHEMA_VERSION` 8 → 9）⇒ ⇒ **不在本批** |
| **X-4** | **宿主层测不了「执行器失败」**：宿主执行器对 `Terminate` 只表态、不会失败 ⇒ ⇒ 台账判别力清单①（重试 3 次 + 恰好一次 `E_HMS-01`）**只在 M-22 层**有覆盖 ⇒ ⇒ 宿主层**结构性不可达** |
| **X-5** | **`TraceDown` ≠「暂停仿真」**：本批的 `TraceDown` 是「**关采样** + **不因溢出而暂停**」⇒ ⇒ 引擎**不停** ⇒ ⇒ 与 LLD M-22 §3.4 那句「关闭 trace 降载（要 M-12 / M-19）」**不完全一致**（M-12 那一半够不着）⇒ ⇒ **如实登记** |
| **X-6** | **宿主看不见降级结果**：`Hms::report` 的返回形状**只有分级** ⇒ ⇒ 宿主**拿不到** `DegradeOutcome` ⇒ ⇒ 若降级失败，宿主**仍**会上报 `E_HMS-02` ⇒ ⇒ ⚠️ 当前**不可达**（宿主执行器不会失败）⇒ ⇒ 改 `report` 的返回形状是**契约层**的事 ⇒ ⇒ 本批**不做** |

---

## §6 · 门禁

**十六项**（与历批同）：`cargo fmt` / `cargo clippy -D warnings` /
`cargo test --workspace --no-fail-fast --lib --tests` / `tools/check-front.sh` /
`tools/golden-run.sh --self-check` + `--keep-going`（+ `--check-mem`）/
`tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` /
`node tools/browser-verify.mjs` / `tools/boot-osz.sh` / 各 `tools/stage*_check.py` /
`tools/ledger_check.py --ledger` + `--self-test` / `tools/error_code_check.py` +
`--self-test` / `tools/doc_check.py --baseline` / `tools/front_test_attrib.py` +
`--selftest` / `tools/count_scale.py` / 变异自检。

⚠️ **本批新增两项专项门禁**（写进 `tools/stage31_check.py`）：

- **7g · D-130 的三档「够不着」必须**如实写进台账** ⇒ ⇒ 台账 D-130 末列必须
  出现 `SnapshotRestore` / `Pause` / `Disconnect` 三个名字 ⇒ ⇒ **不许**用
  「其它动作暂不支持」一句话糊过去。
- **7h · 「宿主只有**一个**降级入口」**逐字判** ⇒ ⇒ `hms.degrade(` 在
  `crates/kernel/src/kernel_host/` **整个目录**（生产代码 **+** 测试）里
  **必须 0 次出现** ⇒ ⇒ ⇒ 降级**只**由 M-22 的策略分支触发 ⇒ ⇒ ⇒ 「策略分支 +
  宿主直做」并存 ⇒ ⇒ ⇒ **必须红**。
- **7i · M-22 的**五组旧措辞**必须**一个都不剩** ⇒ ⇒ 逐组字面量**禁列**
  ⇒ ⇒ ⇒ 「文档被改软 / 改回旧口径」⇒ ⇒ ⇒ **必须红**。

---

## §7 · 裁决

### 裁决 A · 本批只做 D-130 的**内核侧**部分，D-130 **不销账**

三档够不着的逐条给理由（X-2 / X-3）与 C 做法候选，写进台账 D-130 末列
⇒ ⇒ **一条欠账整体没有闭环就不减** ⇒ ⇒ 真实未闭环 62 不变。

### 裁决 B · 执行器接口改成**三态**

`DegradeAttempt::{Done, Unsupported, Failed(PlatformError)}` ⇒ ⇒ 「未实现」
由**执行器**表态 ⇒ ⇒ 否则 `Unsupported` 只能硬编码 ⇒ ⇒ 注入式执行器名存实亡。

### 裁决 C · `Unsupported` **不是失败**

不重试、**不**报 `E_HMS-01`（台账陷阱①）⇒ ⇒ 执行器表态 `Unsupported` 时
**只被问一次**。

### 裁决 D · 重试与终态记账**通用化**

重试循环与 `E_HMS-01` 的消息**不再写死 `Terminate`** ⇒ ⇒ 消息里的动作名是
**实际动作** ⇒ ⇒ 换动作时**不必**再改记账。

### 裁决 E · `None` = 「不做降级」⇒ 新增 `DegradeOutcome::Skipped`

**不**复用 `Unsupported` ⇒ ⇒ 「做不了」与「不需要做」**两个意思两个名字**
⇒ ⇒ ⚠️ `DegradeOutcome` 全仓只被 M-22 匹配（F13）⇒ ⇒ 加变体是**仓内**变更。

### 裁决 F · **只有** `Terminate` 置 `terminated`

其它动作**可恢复** ⇒ ⇒ 终态与可恢复**在数据上分得开**（台账陷阱②）。

### 裁决 G · 宿主的降级**只有一个入口**

删掉 `handle_watchdog_trigger` 里写死的那行 ⇒ ⇒ 降级只由 M-22 `report` 的
策略分支触发 ⇒ ⇒ **杜绝**「两条机制并存 ⇒ 重复降级」。

### 裁决 H · 宿主在装配时设**默认策略** `Fatal → Terminate`

⇒ ⇒ **默认行为一字不变**（C-6）⇒ ⇒ 「默认」由**策略表**承载 ⇒ ⇒ **不**是
「宿主直接调 `degrade`」。

### 裁决 I · 执行器**随句柄一起重装**

装在 `install_probe_pipeline` 的**末尾** ⇒ ⇒ 句柄换新时执行器**一起换**
⇒ ⇒ **结构上不可能**持有过期句柄 ⇒ ⇒ 「一个约束一个落点」。

### 裁决 J · 执行器只映射**够得到**的旋钮

`TraceDown` → M-18 采样 + M-19 溢出策略；`Terminate` → 表态；其余三档 →
`Unsupported` ⇒ ⇒ **不**假装实现了做不到的事。

### 裁决 K · 契约层零升版 / 前端零改动

`Pause` / `Disconnect` 的跨边界部分**不做** ⇒ ⇒ `SCHEMA_VERSION` 仍 **8**
⇒ ⇒ I2C 那批的 v8 → v9 **顺延未受影响**。

### 裁决 L · M-20 恢复会话够不着 ⇒ 本批不做，并给 C 做法候选

候选：**A** 把 `snapshots`（或它的「最近一次快照」槽）改成**共享句柄** ⇒ ⇒
执行器够得着 ⇒ ⇒ 代价 = 宿主形态被降级机制牵动；**B** 宿主在**降级之前**自己
捕获快照并放进一个**共享槽** ⇒ ⇒ 执行器从槽里取 ⇒ ⇒ 代价 = 多一个共享状态；
**C** 把「恢复」做成**契约命令**（`CommandPayload::Restore` 已有）⇒ ⇒ 宿主把
快照塞进命令载荷 ⇒ ⇒ 代价 = 跨进程语义 + 契约层可能要升版 ⇒ ⇒ ⇒ **本批选
不选**都**记进台账**，不替用户拍板。

### 裁决 M · 宿主侧「终止」的实体效果仍由 `E_HMS-02` 承载

执行器的 `Terminate` 分支**只表态**（`Done`）⇒ ⇒ **不重复实现**终止逻辑
⇒ ⇒ 那会造出第二个真相（谁说「已终止」）。

### 裁决 N · 本批**不**改 `Hms::report` 的返回形状

降级结果**留在 M-22 内部** ⇒ ⇒ 宿主拿不到 `DegradeOutcome`（X-6）⇒ ⇒ 改
返回形状是**契约层**的事 ⇒ ⇒ 本批**不做**。

---

## §8 · 施工期订正

> ⚠️ **格式**：每条 = 「规格原文 / 实测 / 已改成」三段 ⇒ ⇒ **本节是批末唯一
> 允许改的地方**（本文件的其余部分**一个字不改**）。

### 订正 1 · §9.1 的期望红集从**占位**改成**实测值**

**规格原文**：§9.1 的「期望红集」那一列写的是**占位**（不写具体红集）⇒ ⇒
§9.2 的三条对照同理（没有「实测」列）。

**实测**：**统筹方**在**最终树**上**独立**跑（不采信 lane 自报）：T1 红 **1** 条 ·
T2 红 **2** 条 · T3 红 **4** 条 · T4 红 **1** 条 · T5 红 **0** 条；T-C1 / T-C2 /
T-C3 全 **0 红**。⇒ ⇒ 与两条 lane 的**自报红集逐条一致**（Lane A 的 T1 / T2、
Lane B 的 M1 / M2）⇒ ⇒ 但 **T5** 两条都报 **0 红**（见订正 3）。

**已改成**：§9.1 的那一列已改成「**实测红集**」并逐条填上**用例全名** ⇒ ⇒
T5 那一行的期望被改成「**0 条**」并注明根因 ⇒ ⇒ §9.2 补了「实测」列，三条对照
全是 **0 红**。

---

### 订正 2 · §3.2.2 的树**没写**「没有执行器且动作不是 `Terminate`」这一支

**规格原文**：§3.2.2 的树把「没有执行器」与「有执行器」合在一句（「其余动作、或
**有**执行器时 ⇒ ⇒ 问执行器」）⇒ ⇒ **没有执行器**且动作**不是** `Terminate`
的这一支**没有单独写** ⇒ ⇒ 实现者**必须自己决定**它是什么。

**实测**：Lane A 按「**做不了 ⇒ `Unsupported`**、**不是失败** ⇒ 不重试、不报码」
实现 ⇒ ⇒ 并新增 `c16_without_an_executor_only_terminate_has_a_built_in_path`
钉住**两条**分支（`Terminate` ⇒ 内置 `Ok{tries:1}` + 置 `terminated`；其余 ⇒
`Unsupported`）⇒ ⇒ ⚠️ **宿主层结构上判别不了这一支**（宿主恒有执行器）⇒ ⇒
它**只能**在 M-22 层判别。

**已改成**：§3.2.2 的树**补上**了这一支（「没有执行器 ⇒ `action != Terminate` ⇒
`Unsupported`」）⇒ ⇒ 措辞与实现**逐字对齐** ⇒ ⇒ §9.3 补第 5 条记录「这一支
只能在 M-22 层判别」。

---

### 订正 3 · §9.1 的 **T5** 在**宿主层零判别力**（实测 0 红）

**规格原文**：§9.1 把 T5（执行器对 `SnapshotRestore` 改成返回 `Done`）列在
「真变异（期望**红**）」里 ⇒ ⇒ 隐含假设「这一条在宿主层**能**红」。

**实测**：**0 红**（统筹方独立跑 + Lane B 自报，两边一致）⇒ ⇒ 根因**有两条**：
① **X-6** —— 宿主拿不到 `DegradeOutcome`；② 三档够不着的动作**本来就不动任何
旋钮** ⇒ ⇒ 「**做到了**」与「**做不了**」在宿主层**观测等价** ⇒ ⇒ ⇒ 这不是
「没人测」，是**结构上测不了**。

**已改成**：§9.1 的 T5 那一行**期望改成「0 条」**并写明根因与「判据归 M-22
层」（`c10` / `c16` 覆盖）⇒ ⇒ §9.3 补第 4 条 ⇒ ⇒ **不许**在宿主层假装它有
判别力 ⇒ ⇒ 规格**不新增**任何用例去凑这一条。

---

### 订正 4 · §9.2 的 **T-C3** 措辞歧义

**规格原文**：§9.2 的 T-C3 写「把 `set_overflow_policy` 的调用挪到 M-18 那一半
**之后**（顺序不变）」⇒ ⇒ ⚠️ 但实现里那个调用**本来就在** M-18 那一半之后
⇒ ⇒ 「挪到之后」是**空操作** ⇒ ⇒ 作为**对照变异**它**什么都没改** ⇒ ⇒
**零判别力**（不是「证明了语义不变」，是「压根没动」）。

**实测**：改成**真做一次等价对调** —— 把 M-19 那一半挪到 M-18 那一半**之前**
（两个旋钮彼此独立 ⇒ ⇒ 语义不变）⇒ ⇒ 实测 **0 红**（宿主 253 条全过）⇒ ⇒
这才是**有意义的**对照。

**已改成**：§9.2 的 T-C3 那一行改成「把 `TraceDown` 那**两个旋钮的调用先后
对调**（M-19 那一半挪到 M-18 那一半**之前**）—— ⚠️ 语义不变」⇒ ⇒ 并补了
「实测」列。

---

### 订正 5 · 统筹方变异脚本的**备份文件名撞车** ⇒ ⇒ 「0 红」可能是**假绿**

**规格原文**：§9 只规定「每条变异前先还原」与「记录完整红集」⇒ ⇒ **没写**备份
怎么存 ⇒ ⇒ 规格层面不要求。

**实测**：统筹方第一次跑 §9 变异时，脚本用 `path.name` 当备份名 ⇒ ⇒
`m22_hms/mod.rs` 与 `kernel_host/mod.rs` **同名** ⇒ ⇒ 备份**只写了一份**（写的
是 M-22 那份）⇒ ⇒ 还原时**把 M-22 的内容覆盖到了宿主的 `mod.rs`** ⇒ ⇒ 宿主
模块**整个消失** ⇒ ⇒ 宿主侧三条变异**全跑在一棵没有宿主模块的树上** ⇒ ⇒
**T4 与 T5 都报「0 红」** ⇒ ⇒ ⚠️ **那是假绿**（T4 实测是**红 1 条**）。根因 =
「**一个过滤串匹配不到任何测试**」与「**所有测试都通过**」在输出上**长得一样**
（都只有一行 `test result: ok. 0 passed`）⇒ ⇒ 脚本当时**只看「有没有 FAILED
行」**⇒ ⇒ **编译失败**与**模块消失**都被读成了「没有判别力」。

**已改成**：① 备份名改成**带路径**的（`m22_hms__mod.rs` / `kernel_host__mod.rs`）
⇒ ⇒ **结构上不可能撞名**；② 脚本**先断言基线干净**（红集为空）**且**必须
**跑得出 `test result:`** ⇒ ⇒ 编译失败 / 过滤串失配 ⇒ ⇒ **当成错误**而不是
0 红；③ 收尾**无条件还原**（`try/finally`）+ `md5` 逐字节校验。⚠️ **两条通用
判据**（跨项目可复用）：**「变异驱动的备份名必须带路径，不能只带文件名」**
（同一个仓里同名文件到处都是：`mod.rs` / `tests.rs` / `lib.rs`）；**「『没跑到
测试』与『测试都过了』必须分开报」** ⇒ ⇒ 否则「**0 红**」这个结论**可能是
假的**。

---

### 订正 6 · 规格 **X-4**「宿主执行器不会失败」严格说是**实践上成立**

**规格原文**：§5.5 的 X-4 写「宿主执行器对 `Terminate` 只表态、不会失败 ⇒ ⇒
宿主层**结构性不可达**」。

**实测**：Lane B 给 `TraceDown` 留了「`RefCell` 争用 ⇒ **诚实报 `Failed`**」的
路径 ⇒ ⇒ 「不会失败」是**实践上**成立（争用几乎不会发生），**不是结构上**
不可达 ⇒ ⇒ 保留这条路径是**对的**（阻塞式借用会在 WASM 里 panic ⇒ ⇒ Worker
崩；假装做到 ⇒ ⇒ 撒谎）。

**已改成**：规格**正文不改**（只定口径、不搬文本）⇒ ⇒ 差异记在这一条 ⇒ ⇒
台账 D-130 末列与本批 PROGRESS 里都按「**实践上**成立」记 ⇒ ⇒ **不许**把它
写成「结构上不可达」。

---

### 订正 7 · 宿主看门狗**无论**降级动作为何都回 `E_HMS-02`

**规格原文**：§3.3.2 说「默认策略 `Fatal → Terminate` ⇒ ⇒ **默认行为一字
不变**」⇒ ⇒ §5.5 的 X-6 说宿主看不见降级结果。

**实测**：宿主把策略改成 `TraceDown` 时，看门狗**仍然**回
`E_HMS_02_FATAL_TERMINATED` 且消息写着「**已终止内核**」⇒ ⇒ 而此时
`hms.terminated()` 是**假**（`TraceDown` **不**置它）⇒ ⇒ ⇒ **消息与标志
不一致**。

**已改成**：这是**宿主既有行为**（**默认策略下**那句话是**准确**的）⇒ ⇒ 本批
**不改**（改它要动 `terminated_error` 的口径 ⇒ ⇒ 那是**契约层**语义 ⇒ ⇒ 裁决
N 已定「不改 `report` 的返回形状」）⇒ ⇒ 差异记在这一条 + 台账 D-130 末列的
「诚实边界」。

---

## §9 · 变异自检

### §9.1 · 真变异（期望**红**）

| 编号 | 落点 | 变异内容 | 实测红集（**统筹方**在**最终树**上独立跑） |
| --- | --- | --- | --- |
| **T1** | M-22 | `DegradeAttempt::Unsupported` 分支改成走**重试**路径 | **1 条** —— `m22_hms::tests::c10_unimplemented_action_is_unsupported_and_never_retried` |
| **T2** | M-22 | `degrade` 里 `action == Terminate` 的判断改成恒假（⇒ `terminated` 永不置位） | **2 条** —— `m22_hms::tests::c8_watchdog_fired_stops_repeated_degrade` · `m22_hms::tests::c14_only_terminate_sets_the_terminated_flag` |
| **T3** | 宿主 | 把 `handle_watchdog_trigger` 里那行写死的 `degrade` **加回来** | **4 条** —— `kernel_host::tests_degrade::the_watchdog_trigger_goes_through_the_dispatch_path` · `kernel_host::tests_degrade::the_trace_down_action_turns_the_sampling_off` · `kernel_host::tests_degrade::the_trace_down_action_is_not_a_terminal_state` · `kernel_host::tests_degrade::the_three_unreachable_actions_are_unsupported_and_not_failures` |
| **T4** | 宿主 | `TraceDown` 的 M-18 那一半改成 `master_enable: true` | **1 条** —— `kernel_host::tests_degrade::the_trace_down_action_turns_the_sampling_off` |
| **T5** | 宿主 | 执行器对 `SnapshotRestore` 改成返回 `Done` | **0 条** —— ⚠️ **宿主层零判别力** ⇒ ⇒ 根因见 **§9.3 第 4 条**与 **§8 订正 3** ⇒ ⇒ 该判据**归 M-22 层**（`c10` / `c16` 覆盖）⇒ ⇒ **不许**在宿主层假装它有判别力 |

### §9.2 · 对照变异（语义不变 ⇒ **必须 0 红**）

| 编号 | 落点 | 变异内容 | 实测 |
| --- | --- | --- | --- |
| **T-C1** | M-22 | 把 `DEGRADE_MAX_TRIES` 的字面量 `3` 写成 `3u32` | **0 红** |
| **T-C2** | 宿主 | 把 `DegradeDispatch` 的字段顺序对调 | **0 红** |
| **T-C3** | 宿主 | 把 `TraceDown` 那**两个旋钮的调用先后对调**（M-19 那一半挪到 M-18 那一半**之前**）—— ⚠️ 语义不变 | **0 红** |

### §9.3 · 判别力边界自查（本批的门禁**抓不到**什么）

1. **「宿主层测不了执行器失败」**（X-4）⇒ ⇒ 台账判别力清单①**只在 M-22 层**
   有覆盖 ⇒ ⇒ 宿主层**结构性不可达** ⇒ ⇒ 如实登记，**不假装**。
2. **「`TraceDown` 的 M-19 那一半在默认配置下是幂等设置」**（F11）⇒ ⇒ 只写
   「降级后策略 == `DropOldestMark`」的断言**恒真** ⇒ ⇒ 必须**先设成
   `PauseSim`** ⇒ ⇒ 判别力清单里写死这一点。
3. **「`DegradeOutcome::Skipped` 与 `Unsupported` 在宿主层**不可达**」**
   ⇒ ⇒ `None` 只有 M-22 层测得到 ⇒ ⇒ 如实登记。
4. **「T5 在宿主层零判别力」**（**实测 0 红**）—— 把执行器对 `SnapshotRestore`
   改成返回 `Done`，宿主层**一条都不红** ⇒ ⇒ 根因 = **X-6**（宿主拿不到
   `DegradeOutcome`）**加上**三档够不着的动作**本来就不动任何旋钮** ⇒ ⇒
   「做到了」与「做不了」在宿主层**观测等价** ⇒ ⇒ 该判据**归 M-22 层**
   （`c10` / `c16` 覆盖）⇒ ⇒ **不许**在宿主层假装它有判别力。
5. **「没有执行器且动作不是 `Terminate`」那一支只在 M-22 层判别**（§8 订正 2）
   ⇒ ⇒ 宿主**恒有执行器** ⇒ ⇒ 这一支在宿主层**结构上不可达** ⇒ ⇒ 如实登记。

---

## §10 · 复现命令

```bash
python3 tools/stage31_check.py docs/detailed-design/STAGE-31_V046_Degrade_Mechanism_D130.md
cargo test -p riscv_sim_kernel --lib m22_hms
cargo test -p riscv_sim_kernel --lib kernel_host
python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md
python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')
python3 tools/count_scale.py
```
