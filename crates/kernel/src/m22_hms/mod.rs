//! M-22 健康管理 HMS（Health Management System）—— LLD M-22 §1~§7 的实现。
//!
//! **职责分离**（LLD M-22 §1.1 / §7.2 (1)）：**M-21 只记录，M-22 只监控分发**。
//! 两个模块之间是**单向**依赖：M-22 → M-21（管理员分发的日志关联由 M-21 的
//! [`LogEntry`] 承载），**M-21 不依赖 M-22**（日志溢出上报由**宿主**做）。
//!
//! **分级与整体健康是两个东西**（冻结规格 §2.4 · 裁决 D）：[`Severity`] 是
//! **单条信号**的级别；[`Health`] 是**整体**状态，**由信号流派生** ——
//! 有 FATAL ⇒ `Critical`；有 CRITICAL / WARNING ⇒ `Degraded`；否则 `Ok`。
//! ⚠️ 派生结果**只降不升**（§2.4 的单调性）：恢复是**显式动作**，不会自动回升。
//!
//! **角色边界**（LLD M-22 §3.3 / §3.4 · 裁决 J）：`role` 由**宿主**在构造时设定，
//! **默认 `Product`**（保守：默认看不见原始日志）。产品 / 嵌入式工作台拿到的
//! [`DispatchResult::Notice`] 只有**脱敏提示**（级别 + 错误码 + 上下文）；
//! 管理员拿到的 [`DispatchResult::Admin`] 才有诊断详情（双时间戳）与日志关联。
//!
//! **降级动作本批只实现 `Terminate`**（冻结规格 §0.4 ⑦ / §2.9 · 裁决 H）：
//! 其余动作（`SnapshotRestore` / `Pause` / `Disconnect` / `TraceDown`）**声明但
//! `Unsupported`** —— `Unsupported` **不是失败**：不重试、**不**报 `E_HMS-01`。
//! 恢复（回退检查点 + 输入重放）本批**不做**（要 M-20）。
//!
//! ⚠️ **旧命名已作废**（冻结规格 §2.1 · 裁决 A）：LLD 与台账里那个「致命常量」的
//! 旧写法（`HMS_` 前缀 + `FATAL`）**不是任何符号**，本模块**不提供**它。
//! 致命信号走 [`HealthSignal`] { `kind: SignalKind::Watchdog`,
//! `ctx: SignalCtx::GuestRun` } ⇒ [`Hms::classify`] 判出 [`Severity::Fatal`]。
//!
//! **归档通道是注入的**（冻结规格 §3.2 / §3.4 · 裁决 K）：[`Hms`] 持有宿主注入的
//! `Option<Box<dyn DataStore>>`（M-23）⇒ **M-22 → M-23 单向**。
//! ⚠️ M-22 **不造**自己的归档后端（那会出现「两个真相」）；⚠️ M-23 **不**实现
//! M-22 的 trait（那是**依赖倒置**）。
//!
//! **日志源也是注入的**（冻结规格 §3.2 · 第五次订正）：[`Hms`] 持有宿主注入的
//! `Option<Rc<RefCell<LogBuffer>>>`（M-21）⇒ 管理员分发的 `log_ref` 从**真实**
//! 日志环里取（`query_recent` 结果的**最后 8 条**，即**按虚拟时间**的全局最近）；
//! **没有注入时恒空**（如实降级，不编造）。
//! ⇒ 产品 / 嵌入式角色**一条日志都拿不到**（角色边界的判别力落在这里）。
//!
//! ⚠️ **保留带有上界，满了之后**聚合**（台账 **D-133**）：LLD 要求 FATAL /
//! CRITICAL 信号**永不丢弃**（取证需要）⇒ 极端场景（大量致命信号）下保留带
//! 会**无上限**增长 ⇒ ⇒ 本批给它**一条常量上界**
//! （[`RETAINED_BAND_CAPACITY`]）⇒ 满了之后**不是**「丢最旧的」（那就**违背**
//! 了「永不丢弃」）⇒ ⇒ **整带折叠成**一个聚合信号**（保**首条**原文 +
//! **计数**，见 [`Hms::retained_total`]）⇒ ⇒ **条数**有界 ⇒ **内存**有界。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError, E_HMS_01_DEGRADE_FAILED};

use crate::m21_logging::{LogBuffer, LogEntry, LogQuery};
use crate::m23_datastore::{DataStore, E_STORE_01_ARCHIVE_FAILED};

/// 健康信号流的环形容量（LLD M-22 §6.2 (11)「健康信号流环形 ≤10⁴ 条」）。
pub const SIGNAL_RING_CAPACITY: usize = 10_000;

/// 指标环的环形容量（LLD M-22 §6.2 (11)「指标环形 ≤4096 条」）。
pub const METRIC_RING_CAPACITY: usize = 4096;

/// 降级动作的重试上界（LLD M-22 §6.2 (1)「重试上界 ≤3 次 + 终态」）。
///
/// ⚠️ 口径：**至多 3 次尝试**（不是「1 次 + 3 次重试」）—— 终态上报
/// `E_HMS-01` 时 `tries == 3`。
pub const DEGRADE_MAX_TRIES: u32 = 3;

/// 保留带（FATAL / CRITICAL）的**明细上界**（台账 **D-133**）。
///
/// ⚠️ **一条常量**（不散落在多处）⇒ 满了之后**整带折叠成**一个聚合信号
///   （保**首条**原文 + **计数**，见 [`Hms::retained_total`]）⇒ ⇒ **不是**
///   「丢最旧的」（那会违背 LLD 的「永不丢弃」）⇒ ⇒ 取证要的「有过致命信号」
///   这个**事实**由**计数**承载。
///
/// ⚠️ **取值口径**：致命 / 严重信号是**稀有**事件 ⇒ 明细留 256 条足够逐条取证；
///   再往后的**条数**由 [`Hms::retained_total`] 承载（**只增不减**）。
pub const RETAINED_BAND_CAPACITY: usize = 256;

/// 信号源（LLD M-22 §2.1.1 的 `src`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalSrc {
    /// M-04 会话。
    Session,
    /// M-05 命令。
    Command,
    /// M-07 传输。
    Transport,
    /// M-12 引擎。
    Engine,
    /// M-17 后端。
    Backend,
    /// M-13 / M-14 模型内（guest trap）。
    Trap,
}

/// 信号种类（LLD M-22 §2.1.1 的 `kind`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalKind {
    /// 引擎心跳。
    Heartbeat,
    /// 会话状态机变化。
    State,
    /// 错误码上报。
    Error,
    /// 命令超时。
    Timeout,
    /// 后端崩溃。
    Crash,
    /// 传输连接事件。
    Conn,
    /// guest 失控看门狗（LLD 的 `WATCHDOG`；**不是**契约事件）。
    Watchdog,
    /// guest trap（模型内异常，正常调试语义）。
    Trap,
}

/// 分级（LLD M-22 §2.1.1 的 `Severity`）。
///
/// ⚠️ **刻意不 derive `PartialOrd` / `Ord`**（冻结规格 §3.2 · 第五次订正）：变体声明顺序是
/// `Fatal` → `Critical` → `Warning` → `InfoNoDegrade`，而**致命是语义上最重的**
/// ⇒ derive 出来的 `Ord` 会让 `Fatal` 变成**最小值** ⇒ 任何写
/// `sev.max(other)` / `sev.min(other)` 的调用者都会**拿到相反的结论**。
/// ⇒ 严重度比较一律走显式的 [`Severity::severity_rank`]（`Fatal` = 3 最大）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// 致命（guest 失控）—— 终止内核。
    Fatal,
    /// 严重 —— 走降级策略。
    Critical,
    /// 警告 —— 仅记录 / 提示。
    Warning,
    /// 仅提示 / 仅记录，**不降级**（guest trap 与分级表未命中的兜底）。
    InfoNoDegrade,
}

impl Severity {
    /// 严重度序（**越大越严重**）：`InfoNoDegrade` = 0 … `Fatal` = 3。
    ///
    /// ⚠️ 这是**唯一**的严重度比较入口 —— 本类型**刻意不实现 `Ord`**（见类型文档）。
    pub fn severity_rank(self) -> u8 {
        match self {
            Severity::InfoNoDegrade => 0,
            Severity::Warning => 1,
            Severity::Critical => 2,
            Severity::Fatal => 3,
        }
    }
}

/// 信号上下文（LLD 里 `ctx` 是 uint32；`GUEST_RUN` 是关键取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalCtx {
    /// guest 正在运行（`GUEST_RUN`）—— 与 `Watchdog` 组合 ⇒ 致命。
    GuestRun,
    /// 会话。
    Session,
    /// 队列。
    Queue,
    /// 连接。
    Connection,
    /// 其它上下文（携带原始标识）。
    Other(u32),
}

/// 健康信号（LLD M-22 §2.1.1 的 `HealthSignal`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthSignal {
    /// 信号源。
    pub src: SignalSrc,
    /// 信号种类。
    pub kind: SignalKind,
    /// 完整错误码（可空）。
    pub err_code: Option<String>,
    /// 虚拟时间（ps）—— 真相是 M-12 的时钟，**调用方传入**。
    pub t_virt_ps: u64,
    /// 宿主时间（ms）—— 调用方传入。
    pub t_host_ms: u64,
    /// 上下文标识。
    pub ctx: SignalCtx,
}

/// 分角色（LLD M-22 §3.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 系统管理员 —— 诊断详情 + 日志关联。
    Admin,
    /// 产品 / 嵌入式工作台 —— 脱敏提示（**不含原始日志**）。
    Product,
}

/// 降级动作（LLD M-22 §3.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeAction {
    /// 不做降级。
    None,
    /// 回退崩溃前最近检查点 + 输入重放（要 M-20 —— 本批**未实现**）。
    SnapshotRestore,
    /// 暂停会话（要 M-04 —— 本批**未实现**）。
    Pause,
    /// 断开外设（要 M-07 —— 本批**未实现**）。
    Disconnect,
    /// 关闭 trace 降载（要 M-12 / M-19 —— 本批**未实现**）。
    TraceDown,
    /// 终止失控内核（**本批唯一有实现的动作**）。
    Terminate,
}

/// 降级结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradeOutcome {
    /// 动作执行成功（第几次尝试成功的）。
    Ok {
        /// 动作。
        action: DegradeAction,
        /// 成功的尝试次数（1 = 一次就成了）。
        tries: u32,
    },
    /// 动作**未实现**（本批只有 `Terminate` 有实现）—— **不是失败**（裁决 H）。
    Unsupported {
        /// 动作。
        action: DegradeAction,
    },
    /// 动作失败且重试用尽（≤3 次）⇒ 终态上报 `E_HMS-01`。
    Failed {
        /// 动作。
        action: DegradeAction,
        /// 用掉的尝试次数。
        tries: u32,
    },
}

/// 脱敏提示（产品 / 嵌入式角色，**不含原始日志**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeMsg {
    /// 级别（脱敏后展示）。
    pub severity: Severity,
    /// 错误码（可展示部分）。
    pub err_code: Option<String>,
    /// 上下文标识（脱敏）。
    pub ctx: SignalCtx,
}

/// 诊断详情（管理员角色，含 M-21 日志关联）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminDiagnosis {
    /// 级别。
    pub severity: Severity,
    /// 完整错误码。
    pub err_code: Option<String>,
    /// 上下文标识。
    pub ctx: SignalCtx,
    /// 虚拟时间（ps）。
    pub t_virt_ps: u64,
    /// 宿主时间（ms）。
    pub t_host_ms: u64,
    /// M-21 日志关联（**引用**，不是日志内容本身）。
    ///
    /// ⚠️ 内容来自**注入的**日志源（[`Hms::install_log_source`]）—— 管理员分支
    /// 取 `query_recent` 的**最后 8 条**（**按虚拟时间**的全局最近）；
    /// **没有注入日志源时恒空**（如实降级）。
    /// 关联的**类型**是 M-21 的 [`LogEntry`] ⇒ 依赖方向 **M-22 → M-21 单向**。
    pub log_ref: Vec<LogEntry>,
}

/// 指标项（LLD M-22 §3.5）。
#[derive(Debug, Clone, PartialEq)]
pub struct Metric {
    /// 虚拟时间（ps）。
    pub t_virt_ps: u64,
    /// 指标名（MIPS / P50 / 队列水位 …）。
    pub name: String,
    /// 指标值。
    pub value: f64,
}

/// 脱敏诊断包（LLD M-22 §3.5 · 评审 P-23：**不含原始日志**）。
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticKit {
    /// 错误码（可空）。
    pub err_code: Option<String>,
    /// 快照摘要。
    ///
    /// ⚠️ 完整快照属 M-20（要 M-20 的快照通道）⇒ 本批给的是**状态摘要**
    /// （健康状态 / 信号计数 / 冻结 / 终止 / 幂等标志）。
    pub snapshot_summary: String,
    /// 稀疏输入序列尾部。
    ///
    /// ⚠️ M-20 的输入序列通道本批**不可达** ⇒ 这里放的是**信号流尾部**的渲染
    /// （取证要素，**不含原始日志**）。
    pub input_seq_tail: Vec<String>,
}

/// 整体健康状态（冻结规格 §2.4 · 裁决 D）。
///
/// ⚠️ **派生**语义：**不是** `report` 的副作用计数器 —— 它由信号流
/// **推导**出来（有 FATAL ⇒ `Critical`；有 CRITICAL / WARNING ⇒ `Degraded`；
/// 否则 `Ok`），且**只降不升**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// 一切正常。
    Ok,
    /// 观察到非致命异常。
    Degraded,
    /// 致命信号已入流 —— 内核已被判死。
    Critical,
}

impl Default for Health {
    fn default() -> Self {
        Self::Ok
    }
}

/// 分发结果。
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    /// 产品 / 嵌入式角色：脱敏提示（**不含任何日志条目**）。
    Notice(NoticeMsg),
    /// 管理员角色：诊断详情 + 日志关联。
    Admin(AdminDiagnosis),
}

/// 降级动作执行器（**可注入**，宿主装配）。
///
/// ⚠️ 注入的是**宿主**的执行器 ⇒ M-22 不自己实现「暂停 / 断连 / 关 trace」
/// 这些跨模块副作用（裁决 H：本批只落 `Terminate`）。
pub trait DegradeExecutor {
    /// 执行一次降级动作。返回 `Err` ⇒ 动作失败 ⇒ 上层**重试**（至多 3 次）。
    fn try_degrade(&mut self, action: DegradeAction) -> Result<(), PlatformError>;
}

/// HMS。
///
/// ⚠️ 冻结规格 §3.2 的字段清单 + **两个私有字段**（`worst_severity` /
/// `degrade_failures`）—— 理由见各自的注释：前者撑起 §2.4 的「只降不升」，
/// 后者是「终态上报 `E_HMS-01`」的**唯一可观察出口**（冻结 API 没有别的通道）。
///
/// ⚠️ **不能 derive `Clone`**（冻结规格 §3.2 · 第五次订正）：`executor` 与 `store` 是
/// **trait 对象**（`Box<dyn …>`）⇒ 既不可克隆也没有 `Debug` ⇒ `Clone` 派生
/// **编译不过**。⇒ 本类型**不提供 `Clone`**，`Debug` **手写**。
pub struct Hms {
    /// 角色（宿主设定，默认 `Product`）。
    role: Role,
    /// 信号流（环形；冻结后不再丢最旧）。
    signals: VecDeque<HealthSignal>,
    /// 与 `signals` **同序**的分级结果（流里存的是**分级后**的值）。
    severities: VecDeque<Severity>,
    /// FATAL / CRITICAL 保留带 —— **永不丢弃**（**有上界**，见 [`RETAINED_BAND_CAPACITY`]）。
    ///
    /// ⚠️ **上界满了之后不是「丢最旧的」** ⇒ ⇒ **整带折叠成**一个聚合信号
    ///   （保**首条**原文）⇒ 折叠后本字段**只留 1 条** ⇒ 条数有界 ⇒ 内存有界。
    /// ⚠️ **返回形状不许改**（改了就是破坏性变更 ⇒ 就要升版 ⇒ 本批不升版）
    ///   ⇒ ⇒ 只改**内部存储** ⇒ 折叠出来的**条数**由 [`Hms::retained_total`] 承载。
    retained: VecDeque<(HealthSignal, Severity)>,
    /// 进入过保留带的 FATAL / CRITICAL 信号**总数**（**只增不减**）。
    ///
    /// ⚠️ 折叠**之前**它等于 `retained.len()`；折叠**之后**它**大于**
    ///   `retained.len()` ⇒ ⇒ 「有多少条致命信号」这个**事实**不丢（不静默丢）。
    retained_total: u64,
    /// 保留带是否已经**折叠**成聚合信号（折叠后**只计数**、不再展开明细）。
    retained_collapsed: bool,
    /// 最近一条带错误码的致命 / 严重信号（**派生量**）。
    ///
    /// ⚠️ 折叠后明细里只剩**首条** ⇒ 「最近一条带码的致命信号」若只从明细里找
    ///   会**答错** ⇒ ⇒ 这里单独留住它（口径与折叠**前**逐字相同）。
    retained_last_code: Option<String>,
    /// 崩溃冻结（有过 FATAL 信号 ⇒ 不再丢最旧）。
    frozen: bool,
    /// 分级映射（可配置）。
    severity_table: HashMap<(SignalSrc, String), Severity>,
    /// 降级策略（可配置）。
    degrade_policy: HashMap<Severity, DegradeAction>,
    /// 指标环（4096 条，满则丢最旧）。
    metrics: VecDeque<Metric>,
    /// 幂等标志：watchdog 是否已触发过（一次运行只触发一次）。
    watchdog_fired: bool,
    /// 内核是否已被终止。
    terminated: bool,
    /// 最近一次升级的原因。
    last_escalation: Option<String>,
    /// 降级动作执行器（可注入，宿主装配）。
    executor: Option<Box<dyn DegradeExecutor>>,
    /// M-23 的归档后端（可注入，宿主装配）—— 诊断包归档走它。
    ///
    /// ⚠️ **注入**的，不自己造（那会出现「两个真相」）。
    store: Option<Box<dyn DataStore>>,
    /// M-21 的日志缓冲（可注入，宿主装配）—— 管理员分发的**日志关联**从这里取。
    ///
    /// ⚠️ 没有它，`AdminDiagnosis.log_ref` **恒空** ⇒ 「管理员看得到日志关联、
    /// 产品看不到」这条**角色边界**只剩结构、**没有判别力** ⇒ 注入后 Admin 分支
    /// 能真的带上日志条目，Product 分支一条都带不上。
    /// ⚠️ 形状与 M-18 的探针表同款（`Rc<RefCell<…>>`，内核单线程）。
    logs: Option<Rc<RefCell<LogBuffer>>>,
    /// 信号流的**粘性最大级别** —— `Health` 由此派生。
    ///
    /// ⚠️ 为什么需要它：信号流是**环形**的，最旧的会被丢掉 ⇒ 只看「当前窗口」
    /// 会让 `Health` **回升**（`Degraded → Ok`），违反 §2.4 的单调性。
    /// 粘性最大值让 `Health` **只降不升**。
    worst_severity: Severity,
    /// 终态上报的降级失败（`E_HMS-01`）—— **一次运行只上报一次**。
    degrade_failures: Vec<PlatformError>,
}

impl fmt::Debug for Hms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hms")
            .field("role", &self.role)
            .field("signals", &self.signals.len())
            .field("severities", &self.severities.len())
            .field("retained", &self.retained.len())
            .field("frozen", &self.frozen)
            .field("severity_table", &self.severity_table.len())
            .field("degrade_policy", &self.degrade_policy.len())
            .field("metrics", &self.metrics.len())
            .field("watchdog_fired", &self.watchdog_fired)
            .field("terminated", &self.terminated)
            .field("last_escalation", &self.last_escalation)
            .field("worst_severity", &self.worst_severity)
            .field("degrade_failures", &self.degrade_failures.len())
            .field("executor_installed", &self.executor.is_some())
            .field("store_installed", &self.store.is_some())
            .field("log_source_installed", &self.logs.is_some())
            .finish()
    }
}

impl Default for Hms {
    fn default() -> Self {
        Self::new()
    }
}

impl Hms {
    /// 新建 HMS（`role = Product`，`watchdog_fired = false`）。
    pub fn new() -> Self {
        Self::with_role(Role::Product)
    }

    /// 新建 HMS，角色由**宿主**设定（裁决 J：默认 `Product`）。
    pub fn with_role(role: Role) -> Self {
        Self {
            role,
            signals: VecDeque::new(),
            severities: VecDeque::new(),
            retained: VecDeque::new(),
            retained_total: 0,
            retained_collapsed: false,
            retained_last_code: None,
            frozen: false,
            severity_table: HashMap::new(),
            degrade_policy: HashMap::new(),
            metrics: VecDeque::new(),
            watchdog_fired: false,
            terminated: false,
            last_escalation: None,
            executor: None,
            store: None,
            logs: None,
            worst_severity: Severity::InfoNoDegrade,
            degrade_failures: Vec::new(),
        }
    }

    /// 注入降级动作执行器（宿主装配）。
    pub fn install_executor(&mut self, ex: Box<dyn DegradeExecutor>) {
        self.executor = Some(ex);
    }

    /// 注入归档后端（宿主装配）—— 不注入则 [`Hms::archive_diagnostic_kit`] 报
    /// 「没有归档通道」而不是静默成功。
    pub fn install_store(&mut self, store: Box<dyn DataStore>) {
        self.store = Some(store);
    }

    /// 注入日志缓冲（宿主装配）—— 管理员分发的日志关联从这里取。
    ///
    /// ⚠️ **没注入时 `log_ref` 恒空**（如实降级，不编造）。
    pub fn install_log_source(&mut self, logs: Rc<RefCell<LogBuffer>>) {
        self.logs = Some(logs);
    }

    /// 当前角色。
    pub fn role(&self) -> Role {
        self.role
    }

    /// 上报：**先分级再入流** → 分角色分发 → 查降级策略 → 降级（LLD §3.1）。
    ///
    /// 返回**分级后的**严重度。
    ///
    /// ⚠️ **幂等**（D-115 的核心判据）：`watchdog_fired` 为真时，本方法**不重复
    /// 触发降级**（信号照常入流 —— 流是取证用的）。
    pub fn report(&mut self, signal: HealthSignal) -> Severity {
        // ① 先分级（定级后再入流，保留策略据此判定）。
        let severity = self.classify(&signal);

        // ② 入流（流里存的是**分级后**的结果：`severities` 与 `signals` 同序）。
        //    崩溃冻结在入流**之前**置位 ⇒ 致命信号本身也不会把最旧的挤掉。
        if severity == Severity::Fatal {
            self.frozen = true;
        }
        self.push_signal(signal.clone(), severity);

        // ③ 保留带（FATAL / CRITICAL **永不丢弃** ⇒ 但**有上界**，D-133）。
        if matches!(severity, Severity::Fatal | Severity::Critical) {
            // ③-1 **条数**先记下来（**只增不减**）⇒ 折叠后「有多少条」不丢。
            self.retained_total += 1;
            // ③-2 最近一条带码的致命 / 严重信号（派生量 ⇒ 折叠后仍答得出）。
            if let Some(code) = signal.err_code.as_deref() {
                self.retained_last_code = Some(code.to_string());
            }
            if self.retained_collapsed {
                // 已折叠 ⇒ **只计数**，不再展开明细（带长有界）。
            } else if self.retained.len() >= RETAINED_BAND_CAPACITY {
                // ⚠️ 满了 ⇒ **整带折叠成**一个聚合信号（保**首条**原文 + 计数）。
                //    **不是**「丢最旧的」—— 那会**违背** LLD 的「永不丢弃」。
                let first = self.retained.front().cloned();
                self.retained.clear();
                if let Some(first) = first {
                    self.retained.push_back(first);
                }
                self.retained_collapsed = true;
            } else {
                self.retained.push_back((signal.clone(), severity));
            }
        }

        // 单调性：粘性最大级别（§2.4「只降不升」）。
        self.worst_severity = worse(self.worst_severity, severity);

        // 升级原因（致命 / 严重都算一次「升级」）。
        if matches!(severity, Severity::Fatal | Severity::Critical) {
            self.last_escalation = Some(escalation_reason(&signal));
        }

        // ④ 分发（分角色）。结果由公开的 [`Hms::dispatch`] 暴露；`report` 的返回值
        //    按冻结 API 只承载**分级结果**。
        let _dispatched = self.dispatch(&signal, severity);

        // ⑤ 查策略 → ⑥ 降级。
        //    ⚠️ **降级闸门（规格 §3.2 实现要点 2）**：LLD §3.1 的伪码是
        //    `if (act != NONE && (sev == FATAL || sev == CRITICAL)) degrade(act);`
        //    ⇒ **只有 `Fatal` / `Critical` 才降级**；`Warning` 即使命中策略也
        //    **只记录不降级**（不许写成「非 InfoNoDegrade 就降级」—— 那会让
        //    `Warning` **越权**触发降级动作）。
        //    ⚠️ 幂等：标志为真 ⇒ **不重复触发**降级。
        if matches!(severity, Severity::Fatal | Severity::Critical) && !self.watchdog_fired {
            if let Some(action) = self.degrade_policy.get(&severity).copied() {
                if action != DegradeAction::None {
                    let _outcome = self.degrade(action);
                }
            }
        }

        severity
    }

    /// 分级（LLD §3.2 的三条规则 + 一条兜底，**顺序固定**）：
    ///
    /// 1. `kind == Watchdog && ctx == GuestRun` ⇒ [`Severity::Fatal`]；
    /// 2. `kind == Trap` ⇒ [`Severity::InfoNoDegrade`]；
    /// 3. 分级表命中 ⇒ 表值；
    /// 4. 未命中 ⇒ [`Severity::InfoNoDegrade`]（**不降级**）。
    pub fn classify(&self, signal: &HealthSignal) -> Severity {
        // 规则 1：guest 失控（watchdog 超时）⇒ 致命。
        if signal.kind == SignalKind::Watchdog && signal.ctx == SignalCtx::GuestRun {
            return Severity::Fatal;
        }
        // 规则 2：guest trap = 模型内异常（正常调试语义）⇒ 仅提示，不降级。
        if signal.kind == SignalKind::Trap {
            return Severity::InfoNoDegrade;
        }
        // 规则 3：分级表命中 ⇒ 表值（可配置）。
        //   ⚠️ 没有错误码的信号**不查表**（表的键含错误码）⇒ 落到兜底。
        if let Some(code) = signal.err_code.as_deref() {
            if let Some(sev) = self
                .severity_table
                .get(&(signal.src, code.to_string()))
                .copied()
            {
                return sev;
            }
        }
        // 规则 4：未命中 ⇒ 仅记录 / 提示（不降级）。
        Severity::InfoNoDegrade
    }

    /// 配分级映射（可配置）。
    pub fn set_severity(&mut self, src: SignalSrc, err_code: &str, sev: Severity) {
        self.severity_table.insert((src, err_code.to_string()), sev);
    }

    /// 配降级策略。
    pub fn set_degrade_policy(&mut self, sev: Severity, action: DegradeAction) {
        self.degrade_policy.insert(sev, action);
    }

    /// 整体健康状态（由信号流派生，§2.4）。
    ///
    /// ⚠️ **只降不升**：恢复是显式动作（本批不做）。
    pub fn state(&self) -> Health {
        match self.worst_severity {
            Severity::Fatal => Health::Critical,
            Severity::Critical | Severity::Warning => Health::Degraded,
            Severity::InfoNoDegrade => Health::Ok,
        }
    }

    /// 最近一次升级的原因。
    pub fn last_escalation(&self) -> Option<&str> {
        self.last_escalation.as_deref()
    }

    /// 幂等标志：watchdog 是否已触发过（一次运行只触发一次）。
    pub fn watchdog_fired(&self) -> bool {
        self.watchdog_fired
    }

    /// 手动置位幂等标志（宿主在触发处置后调用）。
    pub fn mark_watchdog_fired(&mut self) {
        self.watchdog_fired = true;
    }

    /// 内核是否已被终止。
    pub fn terminated(&self) -> bool {
        self.terminated
    }

    /// 信号流（只读快照）。
    pub fn signals(&self) -> Vec<HealthSignal> {
        self.signals.iter().cloned().collect()
    }

    /// 信号流长度。
    pub fn signal_count(&self) -> usize {
        self.signals.len()
    }

    /// 信号流的**分级结果**（与 [`Hms::signals`] **同序**）。
    pub fn severities(&self) -> Vec<Severity> {
        self.severities.iter().copied().collect()
    }

    /// 保留带（FATAL / CRITICAL 不丢）。
    ///
    /// ⚠️ **返回形状不许改**（改了就是破坏性变更 ⇒ 就要升版 ⇒ 本批不升版）。
    /// ⚠️ 保留带**满了之后**会**折叠**成**一个**聚合信号（保**首条**原文）⇒
    ///   ⇒ 折叠后本方法返回**一条** ⇒ 「有多少条」看 [`Hms::retained_total`]。
    pub fn retained(&self) -> Vec<(HealthSignal, Severity)> {
        self.retained.iter().cloned().collect()
    }

    /// 进入过保留带的 FATAL / CRITICAL 信号**总数**（**只增不减**）。
    ///
    /// ⚠️ 折叠**之前**它等于 `retained().len()`；折叠**之后**它**大于**
    ///   `retained().len()` ⇒ ⇒ 「有过多少条致命信号」这个**事实**不丢
    ///   （**不静默丢** ⇒ 裁决 H）。
    pub fn retained_total(&self) -> u64 {
        self.retained_total
    }

    /// 信号流是否已冻结（崩溃后不再丢最旧）。
    pub fn frozen(&self) -> bool {
        self.frozen
    }

    /// 分角色分发结果。
    ///
    /// - `Role::Product` ⇒ [`DispatchResult::Notice`]：**只有**级别 / 错误码 /
    ///   上下文 —— **不含任何日志条目**（角色边界）。
    /// - `Role::Admin` ⇒ [`DispatchResult::Admin`]：诊断详情（双时间戳）+ 日志关联。
    ///
    /// ⚠️ Admin 分支的日志关联从**注入的**日志源取（`query_recent` 的最后 8 条，
    ///   即**按虚拟时间**的全局最近）；没注入日志源 ⇒ `log_ref` **恒空**
    ///   （如实降级，不编造）。
    pub fn dispatch(&self, signal: &HealthSignal, severity: Severity) -> DispatchResult {
        match self.role {
            Role::Product => DispatchResult::Notice(NoticeMsg {
                severity,
                err_code: signal.err_code.clone(),
                ctx: signal.ctx,
            }),
            Role::Admin => DispatchResult::Admin(AdminDiagnosis {
                severity,
                err_code: signal.err_code.clone(),
                ctx: signal.ctx,
                t_virt_ps: signal.t_virt_ps,
                t_host_ms: signal.t_host_ms,
                // M-21 的日志关联（**类型**是 M-21 的 LogEntry ⇒ M-22 → M-21 单向）。
                log_ref: self.log_ref_tail(),
            }),
        }
    }

    /// 管理员分发的日志关联：**注入日志源**里**按虚拟时间**的**全局最近 8 条**。
    ///
    /// ⚠️ 走 M-21 的 [`LogBuffer::query_recent`]（台账 **D-134**）⇒ ⇒ 「最后
    ///   8 条」**名副其实**（旧的 `query` 是**按带**返回的 ⇒ 它的「最后 8 条」
    ///   落点是**最高非空带的末尾**、**不是**全局最近）。
    ///
    /// ⚠️ **最保守的借用口径**：只克隆 `Rc`（不克隆缓冲本身），用 `try_borrow`
    /// 取**只读**视图 ⇒ 万一别处正持有可变借用，**退化成空列表**而不是 panic
    /// （诊断路径不该把内核打挂）。
    fn log_ref_tail(&self) -> Vec<LogEntry> {
        const LOG_REF_TAIL: usize = 8;
        let Some(logs) = self.logs.as_ref() else {
            // 没有注入日志源 ⇒ 恒空（如实降级，不编造）。
            return Vec::new();
        };
        let Ok(buffer) = logs.try_borrow() else {
            return Vec::new();
        };
        buffer.query_recent(&LogQuery::default(), LOG_REF_TAIL)
    }

    /// 采集指标（环形 4096，满则丢最旧）。
    pub fn collect_metric(&mut self, name: &str, value: f64, t_virt_ps: u64) {
        self.metrics.push_back(Metric {
            t_virt_ps,
            name: name.to_string(),
            value,
        });
        while self.metrics.len() > METRIC_RING_CAPACITY {
            self.metrics.pop_front();
        }
    }

    /// 指标快照。
    pub fn metrics(&self) -> Vec<Metric> {
        self.metrics.iter().cloned().collect()
    }

    /// 终态上报的降级失败（`E_HMS-01`）—— **一次运行只上报一次**。
    ///
    /// ⚠️ 这是冻结 API 里**唯一**能观察到「终态上报」的出口（`degrade` 的返回值
    /// 按冻结 API 只承载动作与尝试次数，不带错误对象）。
    pub fn degrade_failures(&self) -> &[PlatformError] {
        &self.degrade_failures
    }

    /// 导出脱敏诊断包（**不含原始日志**）。
    ///
    /// `err_code` 为 `None` 时回落到**最近一条致命 / 严重信号**的错误码。
    pub fn export_diagnostic_kit(&self, err_code: Option<&str>) -> DiagnosticKit {
        let code = match err_code {
            Some(code) => Some(code.to_string()),
            None => self.last_retained_code(),
        };
        let snapshot_summary = format!(
            "health={:?} signals={} retained={} retained_total={} frozen={} terminated={} watchdog_fired={}",
            self.state(),
            self.signals.len(),
            self.retained.len(),
            self.retained_total,
            self.frozen,
            self.terminated,
            self.watchdog_fired
        );
        DiagnosticKit {
            err_code: code,
            snapshot_summary,
            input_seq_tail: self.signal_tail(),
        }
    }

    /// 归档诊断包（调 M-23 的 `archive`，§3.4）。
    ///
    /// ⚠️ **没有归档通道**（宿主没注入后端）⇒ 报错，**不**静默成功。
    pub fn archive_diagnostic_kit(&mut self, err_code: Option<&str>) -> Result<(), PlatformError> {
        let kit = self.export_diagnostic_kit(err_code);
        let name = Self::diagnostic_kit_name(kit.err_code.as_deref());
        let bytes = serde_json::to_vec(&kit_to_json(&kit)).map_err(|e| {
            PlatformError::new(
                ErrorFamily::Storage,
                E_STORE_01_ARCHIVE_FAILED,
                format!("诊断包序列化失败：{e}"),
            )
        })?;
        let store = self.store.as_mut().ok_or_else(|| {
            PlatformError::new(
                ErrorFamily::Storage,
                E_STORE_01_ARCHIVE_FAILED,
                "没有归档通道：宿主未注入归档后端（install_store）",
            )
        })?;
        store.archive(&name, &bytes)
    }

    /// 诊断包的归档名（**同名覆盖**，与 M-23 的归档语义一致）。
    ///
    /// ⚠️ 错误码里的非 `[A-Za-z0-9_-]` 字符被替换成 `_`（归档名是文件名形态）。
    pub fn diagnostic_kit_name(err_code: Option<&str>) -> String {
        match err_code {
            Some(code) => {
                let sanitized: String = code
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                format!("diagnostic-kit-{sanitized}")
            }
            None => "diagnostic-kit".to_string(),
        }
    }

    /// 执行降级（重试 ≤3 次；未实现动作 ⇒ `Unsupported`）。
    ///
    /// - **只有 `Terminate` 有实现**（裁决 H）：没有执行器时走**内置实现**
    ///   （置 `terminated`）；宿主注入了执行器时交给执行器，失败**重试至多 3 次**。
    /// - 其余动作 ⇒ [`DegradeOutcome::Unsupported`]：**不重试**、**不**报 `E_HMS-01`。
    /// - 重试用尽 ⇒ [`DegradeOutcome::Failed`] ⇒ **终态上报 `E_HMS-01`**
    ///   （**一次运行只上报一次**）。
    pub fn degrade(&mut self, action: DegradeAction) -> DegradeOutcome {
        match action {
            DegradeAction::Terminate => self.degrade_terminate(),
            // ⚠️ 裁决 H：未实现的动作**不是失败** —— 不重试、不报码。
            other => DegradeOutcome::Unsupported { action: other },
        }
    }

    /// `Terminate` 的降级路径（唯一有实现的动作）。
    fn degrade_terminate(&mut self) -> DegradeOutcome {
        let Some(executor) = self.executor.as_mut() else {
            // 内置实现：置 `terminated`（宿主据此停止接受新动作）。
            self.terminated = true;
            return DegradeOutcome::Ok {
                action: DegradeAction::Terminate,
                tries: 1,
            };
        };

        let mut tries: u32 = 0;
        let mut last_err: Option<PlatformError> = None;
        while tries < DEGRADE_MAX_TRIES {
            tries += 1;
            match executor.try_degrade(DegradeAction::Terminate) {
                Ok(()) => {
                    self.terminated = true;
                    return DegradeOutcome::Ok {
                        action: DegradeAction::Terminate,
                        tries,
                    };
                }
                Err(e) => last_err = Some(e),
            }
        }
        // 终态：上报 E_HMS-01（一次运行只上报一次）。
        let detail = last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "无明细".to_string());
        self.report_degrade_failure(tries, detail);
        DegradeOutcome::Failed {
            action: DegradeAction::Terminate,
            tries,
        }
    }

    /// 终态上报 `E_HMS-01`（**一次运行只上报一次**）。
    fn report_degrade_failure(&mut self, tries: u32, detail: String) {
        if !self.degrade_failures.is_empty() {
            // 已经报过了 —— 一次运行只上报一次。
            return;
        }
        self.degrade_failures.push(PlatformError::new(
            ErrorFamily::Health,
            E_HMS_01_DEGRADE_FAILED,
            format!("降级动作 Terminate 重试 {tries} 次仍失败：{detail}"),
        ));
    }

    /// 入流（环形；未冻结时满则丢最旧）。
    fn push_signal(&mut self, signal: HealthSignal, severity: Severity) {
        self.signals.push_back(signal);
        self.severities.push_back(severity);
        if !self.frozen && self.signals.len() > SIGNAL_RING_CAPACITY {
            self.signals.pop_front();
            self.severities.pop_front();
        }
    }

    /// 最近一条带错误码的致命 / 严重信号（保留带倒序找）。
    ///
    /// ⚠️ 口径与折叠**前**逐字相同 ⇒ ⇒ 折叠后**仍**答得出「最近那条带码的
    ///   致命信号」（否则折叠会把这个事实**静默**丢掉 ⇒ 违背裁决 H）。
    fn last_retained_code(&self) -> Option<String> {
        self.retained_last_code.clone()
    }

    /// 信号流尾部的渲染（取证要素，**不含原始日志**）。
    fn signal_tail(&self) -> Vec<String> {
        const TAIL: usize = 16;
        self.signals
            .iter()
            .rev()
            .take(TAIL)
            .map(render_signal)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

/// 严重度序的内部口径 —— 走公开的 [`Severity::severity_rank`]（越大越严重）。
fn worse(a: Severity, b: Severity) -> Severity {
    if b.severity_rank() > a.severity_rank() {
        b
    } else {
        a
    }
}

/// 升级原因（给 `last_escalation` 用）。
fn escalation_reason(signal: &HealthSignal) -> String {
    match signal.err_code.as_deref() {
        Some(code) => format!("{code} ({:?}/{:?})", signal.kind, signal.ctx),
        None => format!("{:?}/{:?}", signal.kind, signal.ctx),
    }
}

/// 单条信号的渲染（诊断包用；**不含原始日志**）。
fn render_signal(signal: &HealthSignal) -> String {
    let code = signal.err_code.as_deref().unwrap_or("-");
    format!(
        "src={:?} kind={:?} ctx={:?} err={code} t_virt={} t_host={}",
        signal.src, signal.kind, signal.ctx, signal.t_virt_ps, signal.t_host_ms
    )
}

/// 诊断包 → JSON 值（**不含原始日志**）。
///
/// ⚠️ 手写 JSON 而不是 `#[derive(Serialize)]`：内核 crate 的直接依赖里只有
/// `serde_json`（`serde` 的 derive 不在依赖表里）⇒ 不引入新依赖。
fn kit_to_json(kit: &DiagnosticKit) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "err_code".to_string(),
        match kit.err_code.as_deref() {
            Some(code) => serde_json::Value::String(code.to_string()),
            None => serde_json::Value::Null,
        },
    );
    obj.insert(
        "snapshot_summary".to_string(),
        serde_json::Value::String(kit.snapshot_summary.clone()),
    );
    obj.insert(
        "input_seq_tail".to_string(),
        serde_json::Value::Array(
            kit.input_seq_tail
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );
    serde_json::Value::Object(obj)
}

/// Family used by errors raised from this module.
pub const HMS_FAMILY: ErrorFamily = ErrorFamily::Health;

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use riscv_sim_contract::error::E_ENG_03_NO_PROGRESS;
    use riscv_sim_contract::model::Project;
    use riscv_sim_contract::session::SessionSnapshotDto;

    use super::*;
    use crate::m21_logging::LogLevel;
    use crate::m23_datastore::memory::InMemoryDataStore;

    // ---------------------------------------------------------------- 辅助

    /// 造一个装了 `n` 条日志的日志源（M-21 的 `LogBuffer`）。
    fn log_source_with_entries(n: usize) -> Rc<RefCell<LogBuffer>> {
        let mut buffer = LogBuffer::new(0);
        for i in 0..n {
            buffer.log(
                LogLevel::Error,
                "M22",
                &format!("日志条目 {i}"),
                "E_HMS-01",
                i as u64 * 10,
                i as u64,
            );
        }
        Rc::new(RefCell::new(buffer))
    }

    fn watchdog_signal() -> HealthSignal {
        HealthSignal {
            src: SignalSrc::Engine,
            kind: SignalKind::Watchdog,
            err_code: Some("E_ENG-03".to_string()),
            t_virt_ps: 1_000,
            t_host_ms: 7,
            ctx: SignalCtx::GuestRun,
        }
    }

    fn trap_signal() -> HealthSignal {
        HealthSignal {
            src: SignalSrc::Trap,
            kind: SignalKind::Trap,
            err_code: Some("E_TRAP".to_string()),
            t_virt_ps: 2_000,
            t_host_ms: 8,
            ctx: SignalCtx::Other(3),
        }
    }

    fn heartbeat_signal(code: &str) -> HealthSignal {
        HealthSignal {
            src: SignalSrc::Command,
            kind: SignalKind::Timeout,
            err_code: Some(code.to_string()),
            t_virt_ps: 3_000,
            t_host_ms: 9,
            ctx: SignalCtx::Queue,
        }
    }

    /// 计数执行器（记录被调了几次；可选失败）。
    struct CountingExecutor {
        calls: Rc<Cell<u32>>,
        fail: bool,
    }

    impl CountingExecutor {
        fn new(calls: Rc<Cell<u32>>) -> Self {
            Self { calls, fail: false }
        }

        fn failing(calls: Rc<Cell<u32>>) -> Self {
            Self { calls, fail: true }
        }
    }

    impl DegradeExecutor for CountingExecutor {
        fn try_degrade(&mut self, _action: DegradeAction) -> Result<(), PlatformError> {
            self.calls.set(self.calls.get() + 1);
            if self.fail {
                Err(PlatformError::new(
                    ErrorFamily::Engine,
                    E_ENG_03_NO_PROGRESS,
                    "测试执行器：故意失败",
                ))
            } else {
                Ok(())
            }
        }
    }

    /// 包着 M-23 真实实现的归档后端（测试要读回落档字节 ⇒ 共享引用）。
    struct SharedStore {
        inner: Rc<RefCell<InMemoryDataStore>>,
    }

    impl DataStore for SharedStore {
        fn put_project(&mut self, p: &Project) -> Result<(), PlatformError> {
            self.inner.borrow_mut().put_project(p)
        }

        fn get_project(&self, id: &str) -> Result<Option<Project>, PlatformError> {
            self.inner.borrow().get_project(id)
        }

        fn put_snapshot(&mut self, s: &SessionSnapshotDto) -> Result<(), PlatformError> {
            self.inner.borrow_mut().put_snapshot(s)
        }

        fn get_snapshot(&self, id: u64) -> Result<Option<SessionSnapshotDto>, PlatformError> {
            self.inner.borrow().get_snapshot(id)
        }

        fn list_projects(&self) -> Result<Vec<String>, PlatformError> {
            self.inner.borrow().list_projects()
        }

        fn archive(&mut self, name: &str, bytes: &[u8]) -> Result<(), PlatformError> {
            self.inner.borrow_mut().archive(name, bytes)
        }
    }

    // ---------------------------------------------------- C-1 · 分级：watchdog

    /// C-1 · `classify`：Watchdog + GuestRun ⇒ `Fatal`（判成 `Critical` 就红）。
    #[test]
    fn c1_classify_watchdog_on_guest_run_is_fatal() {
        let hms = Hms::new();
        assert_eq!(hms.classify(&watchdog_signal()), Severity::Fatal);

        // 规则 1 在**最前**：分级表里配了别的值也不改判。
        let mut hms = hms;
        hms.set_severity(SignalSrc::Engine, "E_ENG-03", Severity::Warning);
        assert_eq!(hms.classify(&watchdog_signal()), Severity::Fatal);

        // ctx 不是 GuestRun ⇒ 落到分级表（这里配 Warning ⇒ Warning）。
        let mut other = watchdog_signal();
        other.ctx = SignalCtx::Session;
        assert_eq!(hms.classify(&other), Severity::Warning);
    }

    // -------------------------------------------------------- C-2 · 分级：trap

    /// C-2 · `classify`：Trap ⇒ `InfoNoDegrade`（判成 `Warning` 就红）。
    #[test]
    fn c2_classify_trap_is_info_no_degrade() {
        let mut hms = Hms::new();
        // 即使分级表里配了 Warning，guest trap 仍然只是提示。
        hms.set_severity(SignalSrc::Trap, "E_TRAP", Severity::Warning);
        assert_eq!(hms.classify(&trap_signal()), Severity::InfoNoDegrade);
    }

    // ------------------------------------------------- C-3 · 分级表命中 / 未命中

    /// C-3 · `classify`：分级表命中 ⇒ 表值；未命中 ⇒ `InfoNoDegrade`。
    #[test]
    fn c3_classify_table_hit_and_miss() {
        let mut hms = Hms::new();
        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Critical);
        assert_eq!(
            hms.classify(&heartbeat_signal("E_CTX-06")),
            Severity::Critical
        );
        // 未命中 ⇒ 兜底（**不降级**）。
        assert_eq!(
            hms.classify(&heartbeat_signal("E_CTX-99")),
            Severity::InfoNoDegrade
        );
        // 分级表的键含**信号源**：同一个错误码换个源 ⇒ 未命中。
        let mut other = heartbeat_signal("E_CTX-06");
        other.src = SignalSrc::Transport;
        assert_eq!(hms.classify(&other), Severity::InfoNoDegrade);
    }

    // ------------------------------------------------------- C-4 · 先分级再入流

    /// C-4 · `report` **先分级再入流** ⇒ 流里的 severity 是分级后的值。
    #[test]
    fn c4_report_stores_the_classified_severity() {
        let mut hms = Hms::new();
        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Critical);

        let sev = hms.report(heartbeat_signal("E_CTX-06"));
        assert_eq!(sev, Severity::Critical);

        // 流里存的是**分级后**的结果，且 `severities` 与 `signals` 同序。
        let signals = hms.signals();
        let severities = hms.severities();
        assert_eq!(signals.len(), 1);
        assert_eq!(severities.len(), signals.len());
        assert_eq!(severities[0], Severity::Critical);
        assert_eq!(signals[0].kind, SignalKind::Timeout);
        assert_eq!(hms.signal_count(), 1);
    }

    // --------------------------------------------------------- C-5 · 保留带

    /// C-5 · FATAL / CRITICAL 进**保留带**，流满也不丢。
    #[test]
    fn c5_fatal_and_critical_enter_the_retained_band() {
        let mut hms = Hms::new();

        // 先把环形流灌满（超过容量 ⇒ 未冻结时丢最旧）。
        // ⚠️ 灌的是**分级表未命中**的码 ⇒ 只是 `InfoNoDegrade`，不进保留带。
        for _ in 0..(SIGNAL_RING_CAPACITY + 10) {
            hms.report(heartbeat_signal("E_CTX-07"));
        }
        assert_eq!(hms.signal_count(), SIGNAL_RING_CAPACITY);
        assert!(hms.retained().is_empty());

        // 致命 + 严重 ⇒ 进保留带。
        hms.report(watchdog_signal());
        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Critical);
        hms.report(heartbeat_signal("E_CTX-06"));

        let retained = hms.retained();
        assert_eq!(retained.len(), 2);
        assert!(retained
            .iter()
            .any(|(s, sev)| *sev == Severity::Fatal && s.kind == SignalKind::Watchdog));
        assert!(retained
            .iter()
            .any(|(s, sev)| *sev == Severity::Critical && s.kind == SignalKind::Timeout));

        // 再灌满一轮 ⇒ 保留带**一条不少**（永不丢弃）。
        for _ in 0..(SIGNAL_RING_CAPACITY + 10) {
            hms.report(heartbeat_signal("E_CTX-07"));
        }
        assert_eq!(hms.retained().len(), 2, "保留带永不丢弃");
    }

    // ----------------------------------------------------------- C-6 · 崩溃冻结

    /// C-6 · 有 FATAL ⇒ `frozen` ⇒ 流不再丢最旧。
    #[test]
    fn c6_fatal_freezes_the_signal_ring() {
        let mut hms = Hms::new();

        // 未冻结：灌满 ⇒ 丢最旧。
        for _ in 0..(SIGNAL_RING_CAPACITY + 5) {
            hms.report(heartbeat_signal("E_CTX-06"));
        }
        assert_eq!(hms.signal_count(), SIGNAL_RING_CAPACITY);
        assert!(!hms.frozen());

        // 致命信号 ⇒ 冻结。
        hms.report(watchdog_signal());
        assert!(hms.frozen());

        // 冻结后：再灌 ⇒ **不丢最旧**（长度超过容量）。
        for _ in 0..100 {
            hms.report(heartbeat_signal("E_CTX-06"));
        }
        assert!(
            hms.signal_count() > SIGNAL_RING_CAPACITY,
            "冻结后不得再丢最旧（实际 {}）",
            hms.signal_count()
        );
        assert_eq!(hms.signal_count(), SIGNAL_RING_CAPACITY + 101);
        // severities 与 signals 仍然同序（丢的时候是一起丢的）。
        assert_eq!(hms.severities().len(), hms.signals().len());
    }

    // ------------------------------------------------------- C-7 · Health 派生

    /// C-7 · `Health` 派生：有 FATAL ⇒ `Critical`；只有 WARNING ⇒ `Degraded`；
    /// 无 ⇒ `Ok`。
    #[test]
    fn c7_health_is_derived_and_never_improves() {
        let mut hms = Hms::new();
        assert_eq!(hms.state(), Health::Ok);

        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Warning);
        hms.report(heartbeat_signal("E_CTX-06"));
        assert_eq!(hms.state(), Health::Degraded);

        let mut hms = hms;
        hms.report(watchdog_signal());
        assert_eq!(hms.state(), Health::Critical);

        // **只降不升**：后来的轻量信号不能把状态拉回 `Degraded` / `Ok`。
        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Warning);
        for _ in 0..10 {
            hms.report(heartbeat_signal("E_CTX-06"));
        }
        assert_eq!(hms.state(), Health::Critical, "Health 只降不升");
    }

    // ------------------------------------------------------------ C-8 · 幂等

    /// C-8 · **幂等**：`watchdog_fired` 为真 ⇒ 重复 `report` **不重复触发降级**。
    #[test]
    fn c8_watchdog_fired_stops_repeated_degrade() {
        let calls = Rc::new(Cell::new(0u32));
        let mut hms = Hms::new();
        hms.install_executor(Box::new(CountingExecutor::new(calls.clone())));
        hms.set_degrade_policy(Severity::Fatal, DegradeAction::Terminate);

        // 第一次 report ⇒ 触发降级（执行器被调一次）。
        let sev = hms.report(watchdog_signal());
        assert_eq!(sev, Severity::Fatal);
        assert_eq!(calls.get(), 1, "首次 report 应当触发一次降级");
        assert!(hms.terminated());

        // 幂等标志置位后再报 ⇒ **不再触发**降级。
        hms.mark_watchdog_fired();
        assert!(hms.watchdog_fired());
        let sev = hms.report(watchdog_signal());
        assert_eq!(sev, Severity::Fatal);
        assert_eq!(calls.get(), 1, "幂等标志为真 ⇒ 不得重复触发降级");
        assert_eq!(hms.degrade_failures().len(), 0);
    }

    // --------------------------------------------------------- C-9 · 角色边界

    /// C-9 · **角色边界**：`Role::Product` 的分发结果里**搜不到任何日志文本**；
    /// `Role::Admin` 的**含** `log_ref`。
    #[test]
    fn c9_product_dispatch_carries_no_log_text() {
        let hms = Hms::with_role(Role::Product);
        let signal = watchdog_signal();

        let product = hms.dispatch(&signal, Severity::Fatal);

        // 结构性：Product 只能是**脱敏提示**。
        let notice = match &product {
            DispatchResult::Notice(n) => n,
            other => panic!("Product 角色的分发结果必须是脱敏提示：{other:?}"),
        };
        assert_eq!(notice.severity, Severity::Fatal);
        assert_eq!(notice.err_code.as_deref(), Some("E_ENG-03"));
        assert_eq!(notice.ctx, SignalCtx::GuestRun);

        // 文本扫描：**真的去搜** —— 结果里不许出现任何日志文本 / 日志字段。
        let text = format!("{product:?}");
        for forbidden in [
            "LogEntry",
            "log_ref",
            "log",
            "msg",
            "truncated",
            "time_virt",
            "time_host",
        ] {
            assert!(
                !text.contains(forbidden),
                "Product 的分发结果里搜到了日志文本 {forbidden:?}：{text}"
            );
        }
        // 脱敏：提示里**没有**诊断详情才有的双时间戳。
        assert!(!text.contains("t_virt_ps"));
        assert!(!text.contains("t_host_ms"));

        // 管理员：诊断详情 + 日志关联字段。
        let admin_hms = Hms::with_role(Role::Admin);
        let admin = admin_hms.dispatch(&signal, Severity::Fatal);
        let diag = match &admin {
            DispatchResult::Admin(d) => d,
            other => panic!("Admin 角色的分发结果必须是诊断详情：{other:?}"),
        };
        assert_eq!(diag.severity, Severity::Fatal);
        assert_eq!(diag.t_virt_ps, signal.t_virt_ps);
        assert_eq!(diag.t_host_ms, signal.t_host_ms);
        assert!(
            diag.log_ref.is_empty(),
            "没有注入日志源 ⇒ 关联列表为空（如实降级）"
        );
        // 管理员路径**确实**带日志关联字段（结构上可辨）。
        assert!(format!("{admin:?}").contains("log_ref"));
    }

    /// C-9 的 **Admin 半边**（有真判别力）：注入日志源 ⇒ `log_ref` **非空**，
    /// 且内容与 M-21 的 `LogBuffer` **一致**（取**最后 8 条**）。
    ///
    /// ⚠️ 取法是 `query_recent`（D-134）⇒ 但那个缓冲**单带** ⇒ 「按带」与
    ///   「按虚拟时间」**恰好**同序 ⇒ 本条的**取值**不变。
    #[test]
    fn c9_admin_dispatch_carries_the_log_association_when_a_log_source_is_installed() {
        let logs = log_source_with_entries(10);
        let mut hms = Hms::with_role(Role::Admin);
        hms.install_log_source(logs.clone());

        let admin = hms.dispatch(&watchdog_signal(), Severity::Fatal);
        let diag = match &admin {
            DispatchResult::Admin(d) => d,
            other => panic!("Admin 角色的分发结果必须是诊断详情：{other:?}"),
        };

        // 非空，且就是**最后 8 条**。
        let expected = logs.borrow().query(&LogQuery::default());
        assert_eq!(expected.len(), 10);
        assert_eq!(diag.log_ref.len(), 8, "日志关联取最后 8 条");
        assert_eq!(diag.log_ref, expected[2..].to_vec());
        // 内容确实来自那个日志环。
        assert_eq!(diag.log_ref[0].msg, "日志条目 2");
        assert_eq!(diag.log_ref[7].msg, "日志条目 9");
        assert_eq!(diag.log_ref[7].err_code, "E_HMS-01");
    }

    /// C-9 的 **Product 半边**（注入日志源后仍然一条日志都拿不到）。
    #[test]
    fn c9_product_dispatch_carries_no_log_text_even_with_a_log_source() {
        let mut hms = Hms::with_role(Role::Product);
        hms.install_log_source(log_source_with_entries(10));

        let product = hms.dispatch(&watchdog_signal(), Severity::Fatal);

        // 结构性：Product 只能是脱敏提示。
        let notice = match &product {
            DispatchResult::Notice(n) => n,
            other => panic!("Product 角色的分发结果必须是脱敏提示：{other:?}"),
        };
        assert_eq!(notice.severity, Severity::Fatal);

        // 文本扫描：**真的去搜** —— 注入的日志源里的内容一条都不许漏过来。
        let text = format!("{product:?}");
        for forbidden in [
            "LogEntry",
            "log_ref",
            "log",
            "msg",
            "truncated",
            "time_virt",
            "time_host",
            "日志条目",
            "M22",
            "E_HMS-01",
        ] {
            assert!(
                !text.contains(forbidden),
                "Product 的分发结果里搜到了日志文本 {forbidden:?}：{text}"
            );
        }
    }

    // ------------------------------------------- C-10 · 未实现动作 ⇒ Unsupported

    /// C-10 · 降级：未实现动作 ⇒ `Unsupported`（**不重试**、**不**报 `E_HMS-01`）。
    #[test]
    fn c10_unimplemented_action_is_unsupported_and_never_retried() {
        let calls = Rc::new(Cell::new(0u32));
        let mut hms = Hms::new();
        hms.install_executor(Box::new(CountingExecutor::failing(calls.clone())));

        for action in [
            DegradeAction::None,
            DegradeAction::SnapshotRestore,
            DegradeAction::Pause,
            DegradeAction::Disconnect,
            DegradeAction::TraceDown,
        ] {
            let outcome = hms.degrade(action);
            assert_eq!(outcome, DegradeOutcome::Unsupported { action });
        }
        assert_eq!(calls.get(), 0, "未实现的动作不得走重试路径");
        assert_eq!(
            hms.degrade_failures().len(),
            0,
            "Unsupported 不是失败 ⇒ 不得报 E_HMS-01"
        );
        assert!(!hms.terminated());
    }

    // ---------------------------------------------- C-11 · 重试 ≤3 + 终态上报

    /// C-11 · 降级：可注入的执行器失败 ⇒ 重试**至多 3 次** ⇒ `Failed` ⇒
    /// **恰好一次** `E_HMS-01`。
    #[test]
    fn c11_failing_executor_is_retried_three_times_and_reports_once() {
        let calls = Rc::new(Cell::new(0u32));
        let mut hms = Hms::new();
        hms.install_executor(Box::new(CountingExecutor::failing(calls.clone())));

        let outcome = hms.degrade(DegradeAction::Terminate);
        assert_eq!(
            outcome,
            DegradeOutcome::Failed {
                action: DegradeAction::Terminate,
                tries: 3
            }
        );
        assert_eq!(calls.get(), 3, "重试至多 3 次");
        assert!(!hms.terminated(), "动作没成功 ⇒ 不得置 terminated");

        // **恰好一次** `E_HMS-01`。
        let failures = hms.degrade_failures();
        assert_eq!(failures.len(), 1, "终态只上报一次");
        assert_eq!(failures[0].family, ErrorFamily::Health);
        assert_eq!(failures[0].family, HMS_FAMILY);
        assert_eq!(failures[0].scenario, E_HMS_01_DEGRADE_FAILED);
        assert!(failures[0].to_string().contains("E_HMS-01"));

        // 再失败一次 ⇒ **不重复上报**（一次运行只报一次）。
        let outcome = hms.degrade(DegradeAction::Terminate);
        assert_eq!(
            outcome,
            DegradeOutcome::Failed {
                action: DegradeAction::Terminate,
                tries: 3
            }
        );
        assert_eq!(calls.get(), 6, "每次调用各自重试 3 次");
        assert_eq!(hms.degrade_failures().len(), 1, "一次运行只上报一次");
    }

    // ------------------------------------------ C-12 · 诊断包不含日志 + 可归档

    /// C-12 · 诊断包**不含原始日志**；归档后**字节可读回**（走 M-23）。
    #[test]
    fn c12_diagnostic_kit_has_no_logs_and_archives_readable_bytes() {
        let mut hms = Hms::with_role(Role::Admin);
        hms.report(watchdog_signal());
        hms.report(heartbeat_signal("E_CTX-06"));

        // 结构 + 文本：**没有**日志条目。
        let kit = hms.export_diagnostic_kit(Some("E_HMS-01"));
        assert_eq!(kit.err_code.as_deref(), Some("E_HMS-01"));
        let text = format!("{kit:?}");
        for forbidden in ["LogEntry", "log_ref", "msg:", "level"] {
            assert!(
                !text.contains(forbidden),
                "诊断包里搜到了日志文本 {forbidden:?}：{text}"
            );
        }

        // 归档（走 M-23 的 `archive`）⇒ 字节可读回。
        let store = Rc::new(RefCell::new(InMemoryDataStore::new()));
        hms.install_store(Box::new(SharedStore {
            inner: store.clone(),
        }));
        hms.archive_diagnostic_kit(Some("E_HMS-01"))
            .expect("归档必须成功");

        let name = Hms::diagnostic_kit_name(Some("E_HMS-01"));
        let store_ref = store.borrow();
        let bytes = store_ref.get_archive(&name).expect("归档的字节必须读得回");
        let parsed: serde_json::Value =
            serde_json::from_slice(bytes).expect("归档的字节必须是合法 JSON");
        assert_eq!(parsed["err_code"], serde_json::json!("E_HMS-01"));
        assert!(parsed["snapshot_summary"].as_str().is_some());
        assert!(parsed["input_seq_tail"].is_array());
        // 归档的 JSON 里同样没有日志字段。
        let json_text = parsed.to_string();
        for forbidden in ["LogEntry", "log_ref", "msg\""] {
            assert!(
                !json_text.contains(forbidden),
                "归档字节里搜到了日志文本 {forbidden:?}：{json_text}"
            );
        }
    }

    // ---------------------------------------------- D-134 · `log_ref` 改走
    //                                                 `query_recent`

    /// D-134 · `log_ref` **改走** `query_recent` ⇒ 「最后 N 条」**名副其实**：
    /// 跨带交错写入时，`log_ref` 按**虚拟时间**排（**不是**按带）。
    ///
    /// ⚠️ 这条用例对**旧实现**（取 `query` 的尾部）是**红的** ⇒ ⇒ 真判别力。
    #[test]
    fn admin_log_ref_follows_the_global_virtual_time_order() {
        let mut buffer = LogBuffer::new(0);
        // 跨带交错 ⇒ 带序 ≠ 时间序。
        // ⚠️ 宿主时间与虚拟时间**故意反向** ⇒ 用宿主时间排序会给出**另一个**顺序。
        buffer.log(LogLevel::Error, "M22", "e1", "E_HMS-01", 10, 40);
        buffer.log(LogLevel::Trace, "M22", "t1", "E_HMS-01", 20, 30);
        buffer.log(LogLevel::Info, "M22", "i1", "E_HMS-01", 30, 20);
        buffer.log(LogLevel::Trace, "M22", "t2", "E_HMS-01", 40, 10);
        let logs = Rc::new(RefCell::new(buffer));

        let mut hms = Hms::with_role(Role::Admin);
        hms.install_log_source(logs.clone());
        let admin = hms.dispatch(&watchdog_signal(), Severity::Fatal);
        let diag = match &admin {
            DispatchResult::Admin(d) => d,
            other => panic!("Admin 角色的分发结果必须是诊断详情：{other:?}"),
        };

        let msgs: Vec<&str> = diag.log_ref.iter().map(|e| e.msg.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["e1", "t1", "i1", "t2"],
            "log_ref 按虚拟时间排（不是按带）"
        );

        // 旧的 `query`（按带）给的是**另一个**顺序 ⇒ 两者确实分得开。
        let by_band_entries = logs.borrow().query(&LogQuery::default());
        let by_band: Vec<&str> = by_band_entries.iter().map(|e| e.msg.as_str()).collect();
        assert_eq!(by_band, vec!["t1", "t2", "i1", "e1"], "query 仍按带返回");
    }

    /// D-134 · **角色不变式不许破**（D-131 陷阱③）：`Role::Product` 的诊断结果里
    /// **仍然搜不到任何日志文本** —— 哪怕日志源里**有** `query_recent` 能取到的条目。
    #[test]
    fn product_dispatch_carries_no_log_text_even_when_query_recent_would_return_entries() {
        let mut buffer = LogBuffer::new(0);
        buffer.log(LogLevel::Error, "M22", "机密日志正文甲", "E_HMS-01", 10, 1);
        buffer.log(LogLevel::Trace, "M22", "机密日志正文乙", "E_HMS-01", 20, 2);
        let logs = Rc::new(RefCell::new(buffer));

        // 前提：`query_recent` 在那个缓冲上**确实**能取到条目 ⇒ 这条用例**不空转**。
        assert_eq!(
            logs.borrow().query_recent(&LogQuery::default(), 8).len(),
            2,
            "前提：日志源里确实有可取的条目"
        );

        let mut hms = Hms::with_role(Role::Product);
        hms.install_log_source(logs);
        let product = hms.dispatch(&watchdog_signal(), Severity::Fatal);

        // 文本扫描：**真的去搜** —— 注入的日志源里的内容一条都不许漏过来。
        let text = format!("{product:?}");
        for forbidden in [
            "LogEntry",
            "log_ref",
            "log",
            "msg",
            "truncated",
            "time_virt",
            "time_host",
            "机密日志正文",
            "M22",
            "E_HMS-01",
        ] {
            assert!(
                !text.contains(forbidden),
                "Product 的分发结果里搜到了日志文本 {forbidden:?}：{text}"
            );
        }
    }

    // ------------------------------------------- D-133 · 保留带上界 + 聚合

    /// D-133 · 保留带**有上界**（**一条常量**）⇒ 插满之后**折叠**成**一个**
    /// 聚合信号 ⇒ 保**首条**原文 + **计数** ⇒ ⇒ **不静默丢**（裁决 H）。
    #[test]
    fn retained_band_is_bounded_and_aggregates_when_full() {
        let mut hms = Hms::new();

        // 插到**刚好满** ⇒ 明细一条不少（**返回形状不变**）。
        for i in 0..RETAINED_BAND_CAPACITY {
            let mut signal = watchdog_signal();
            signal.err_code = Some(format!("E_FATAL-{i}"));
            signal.t_virt_ps = i as u64;
            hms.report(signal);
        }
        let retained = hms.retained();
        assert_eq!(retained.len(), RETAINED_BAND_CAPACITY, "满之前明细一条不少");
        assert_eq!(
            hms.retained_total(),
            RETAINED_BAND_CAPACITY as u64,
            "折叠前计数 = 明细条数"
        );
        assert_eq!(retained[0].0.err_code.as_deref(), Some("E_FATAL-0"));
        assert_eq!(retained[0].1, Severity::Fatal);

        // 再插一条 ⇒ **折叠**：明细只剩**首条**（原文逐字保留），**条数**由计数承载。
        let mut extra = watchdog_signal();
        extra.err_code = Some("E_FATAL-OVERFLOW".to_string());
        extra.t_virt_ps = RETAINED_BAND_CAPACITY as u64;
        hms.report(extra);

        let folded = hms.retained();
        assert_eq!(folded.len(), 1, "折叠后明细只剩一条（带长有界）");
        assert_eq!(
            folded[0].0.err_code.as_deref(),
            Some("E_FATAL-0"),
            "首条原文保留"
        );
        assert_eq!(folded[0].1, Severity::Fatal, "首条的分级也保留");
        assert_eq!(
            hms.retained_total(),
            RETAINED_BAND_CAPACITY as u64 + 1,
            "计数正确（折叠的那条也算）"
        );

        // 再插 10 条 ⇒ 带长**不再涨**（有界），计数**继续涨**（**不静默丢**）。
        for i in 0..10 {
            let mut signal = watchdog_signal();
            signal.err_code = Some(format!("E_FATAL-MORE-{i}"));
            hms.report(signal);
        }
        assert_eq!(hms.retained().len(), 1, "带长有界");
        assert_eq!(
            hms.retained_total(),
            RETAINED_BAND_CAPACITY as u64 + 11,
            "计数只增不减"
        );
    }

    /// D-133 · 折叠**之后**，「最近一条带码的致命信号」**仍**答得出（口径与
    /// 折叠**前**逐字相同 ⇒ 折叠不许把这个事实**静默**丢掉）。
    #[test]
    fn the_last_retained_code_survives_the_aggregation() {
        let mut hms = Hms::new();
        let last = RETAINED_BAND_CAPACITY + 4;
        for i in 0..=last {
            let mut signal = watchdog_signal();
            signal.err_code = Some(format!("E_FATAL-{i}"));
            hms.report(signal);
        }
        assert_eq!(hms.retained().len(), 1, "已折叠");
        let kit = hms.export_diagnostic_kit(None);
        let expected = format!("E_FATAL-{last}");
        assert_eq!(
            kit.err_code.as_deref(),
            Some(expected.as_str()),
            "折叠后仍答得出最近一条带码的致命信号"
        );
        // 诊断包摘要里**条数**也可见（不静默丢）。
        assert!(
            kit.snapshot_summary.contains(&format!(
                "retained_total={}",
                RETAINED_BAND_CAPACITY as u64 + 5
            )),
            "摘要里应当看得到保留带的总条数：{}",
            kit.snapshot_summary
        );
    }

    // ------------------------------------------------ 附加 · 其它口径的钉子

    /// 附加 · 默认角色是 `Product`（裁决 J：保守 —— 默认看不见原始日志）。
    #[test]
    fn the_default_role_is_product() {
        assert_eq!(Hms::new().role(), Role::Product);
        assert_eq!(Hms::with_role(Role::Admin).role(), Role::Admin);
    }

    /// 附加 · 指标环容量 4096，满则丢最旧。
    #[test]
    fn the_metric_ring_drops_the_oldest_at_capacity() {
        let mut hms = Hms::new();
        for i in 0..(METRIC_RING_CAPACITY + 10) {
            hms.collect_metric("mips", i as f64, i as u64);
        }
        let metrics = hms.metrics();
        assert_eq!(metrics.len(), METRIC_RING_CAPACITY);
        assert_eq!(metrics[0].name, "mips");
        assert_eq!(metrics[0].value, 10.0, "最旧的应当被丢掉");
        assert_eq!(
            metrics[METRIC_RING_CAPACITY - 1].value,
            (METRIC_RING_CAPACITY + 9) as f64
        );
    }

    /// 附加 · 没有归档通道 ⇒ 报错，**不**静默成功。
    #[test]
    fn archiving_without_a_store_reports_no_archive_channel() {
        let mut hms = Hms::new();
        let err = hms
            .archive_diagnostic_kit(Some("E_HMS-01"))
            .expect_err("没有归档通道必须报错");
        assert_eq!(err.family, ErrorFamily::Storage);
        assert_eq!(err.scenario, E_STORE_01_ARCHIVE_FAILED);
        assert!(err.to_string().contains("归档通道"));
    }

    /// 附加 · 降级策略真的会被 `report` 触发（致命 ⇒ terminate）。
    #[test]
    fn report_triggers_the_configured_degrade_policy() {
        let mut hms = Hms::new();
        hms.set_degrade_policy(Severity::Fatal, DegradeAction::Terminate);
        let sev = hms.report(watchdog_signal());
        assert_eq!(sev, Severity::Fatal);
        assert!(hms.terminated(), "致命信号 ⇒ 策略里的 terminate 必须生效");
        assert_eq!(hms.state(), Health::Critical);
    }

    /// 附加 · `InfoNoDegrade` **不降级**（guest trap 与分级表未命中的兜底）。
    #[test]
    fn info_no_degrade_never_degrades() {
        let calls = Rc::new(Cell::new(0u32));
        let mut hms = Hms::new();
        hms.install_executor(Box::new(CountingExecutor::new(calls.clone())));
        hms.set_degrade_policy(Severity::InfoNoDegrade, DegradeAction::Terminate);

        let sev = hms.report(trap_signal());
        assert_eq!(sev, Severity::InfoNoDegrade);
        assert_eq!(calls.get(), 0, "InfoNoDegrade 不得触发降级");
        assert!(!hms.terminated());
    }

    /// 附加 · **降级闸门只限致命与严重**（规格 §3.2 实现要点 2）：`Warning`
    /// 即使**命中降级策略**也**只记录不降级**（不许越权触发降级动作）。
    #[test]
    fn warning_never_degrades_even_with_a_matching_policy() {
        let calls = Rc::new(Cell::new(0u32));
        let mut hms = Hms::new();
        hms.install_executor(Box::new(CountingExecutor::new(calls.clone())));
        // 分级表把这条判成 Warning，且策略里**明确**配了 terminate。
        hms.set_severity(SignalSrc::Command, "E_CTX-06", Severity::Warning);
        hms.set_degrade_policy(Severity::Warning, DegradeAction::Terminate);

        let sev = hms.report(heartbeat_signal("E_CTX-06"));
        assert_eq!(sev, Severity::Warning);
        assert_eq!(calls.get(), 0, "Warning 越权降级 ⇒ 执行器不得被调用");
        assert!(!hms.terminated());
        assert_eq!(hms.degrade_failures().len(), 0);
        // 但状态**仍然**派生为 Degraded（记录 ≠ 不降级）。
        assert_eq!(hms.state(), Health::Degraded);
    }

    /// 附加 · `Severity::severity_rank`：**越大越严重**，`Fatal` 是最大值。
    ///
    /// ⚠️ `Severity` **刻意不实现 `Ord`** ⇒ 这个方法是唯一的比较入口。
    #[test]
    fn severity_rank_is_monotonic_and_fatal_is_the_largest() {
        assert!(Severity::Fatal.severity_rank() > Severity::Critical.severity_rank());
        assert!(Severity::Critical.severity_rank() > Severity::Warning.severity_rank());
        assert!(Severity::Warning.severity_rank() > Severity::InfoNoDegrade.severity_rank());
        assert_eq!(Severity::Fatal.severity_rank(), 3);
        assert_eq!(Severity::Critical.severity_rank(), 2);
        assert_eq!(Severity::Warning.severity_rank(), 1);
        assert_eq!(Severity::InfoNoDegrade.severity_rank(), 0);
    }

    /// 附加 · 旧的那个「致命常量」写法**不出现在本模块里**（裁决 A）。
    #[test]
    fn the_obsolete_fatal_constant_spelling_is_absent_from_this_module() {
        // ⚠️ 拼出来而不是写出来 —— 否则这条用例自己就把那个标识符写回文件里了。
        let forbidden = ["HMS", "FATAL"].join("_");
        let text = include_str!("mod.rs");
        assert!(
            !text.contains(&forbidden),
            "旧命名已作废（裁决 A）⇒ 代码里不得再出现那个标识符"
        );
    }

    /// 附加 · `E_HMS-01` 的场景码**只引用**、不复制（契约层是唯一定义处）。
    #[test]
    fn the_hms_scenario_code_is_referenced_and_never_redefined_here() {
        let text = include_str!("mod.rs");
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            let is_const_decl = line.starts_with("const ")
                || line.starts_with("pub const ")
                || line.starts_with("pub(crate) const ")
                || line.starts_with("pub(super) const ");
            assert!(
                !(is_const_decl && line.contains("E_HMS_0") && line.contains(": u16")),
                "场景码只能在契约层定义一次；m22_hms 不得复制字面量：{line}"
            );
        }
        // 引用处确实落在契约层。
        assert_eq!(E_HMS_01_DEGRADE_FAILED, 1);
    }
}
