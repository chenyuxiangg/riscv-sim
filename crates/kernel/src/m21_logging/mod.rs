//! M-21 日志系统 (Logging System).
//!
//! LLD M-21 的落地实现：**结构化日志条目（8 字段）** + **五级分带环形缓冲** +
//! **UTF-8 字符边界截断** + 运行时级别过滤 + 管理员通道的 `query` / `export`。
//!
//! 冻结规格：`docs/detailed-design/STAGE-13_V028_Watchdog_HMS_Logging.md`
//! （§2.3 裁决 C 五级无 `Fatal` · §2.5 裁决 E 五带 · §2.6 裁决 M 截断 ·
//! §2.7 裁决 I / L 角色边界 · §3.1 冻结 API · §5.2 验收 B-1 ~ B-10）。
//!
//! # 五带模型（裁决 E）
//!
//! LLD 内部三处容量口径互相矛盾（§2.1「debug+info 10³」vs §2.2 每级一行 vs
//! §1.2「共 7×10³ 条」），STAGE-13 §2.5 逐字对账后裁决为 **五级各一个带**：
//! 带间**独立容量预算**、带间**独立溢出计数**（一个带满了不牵动别的带）。
//!
//! | 级别 | WASM 形态（默认） | 服务端 / 导出形态 |
//! | --- | --- | --- |
//! | TRACE | 4 000 | 100 000 |
//! | DEBUG | 1 000 | 10 000 |
//! | INFO | 1 000 | 10 000 |
//! | WARN | 500 | 5 000 |
//! | ERROR | 500 | 5 000 |
//!
//! 默认合计 **7 000 条**（= LLD §1.2 的「共 7×10³ 条」与 192B 槽 × 7×10³ ≈ 1.34MB
//! 的内存预算逐字对账）。
//!
//! # 截断语义（裁决 M）
//!
//! `ctx ≤ 24` 字节、`err_code ≤ 12` 字节、`msg ≤ 128` 字节；**任一**字段被截 ⇒
//! 该条目的 `truncated = true`。截断**只按 UTF-8 字符边界**切 ⇒ **绝不切半个
//! 字符**（中文被切成乱码是最典型的失败形态）。**错误码与上下文不因 `msg`
//! 被截而丢失** —— 三个字段各自独立截断。
//!
//! # ⚠️ 「零分配」**不承诺**（裁决 M）
//!
//! LLD §3.1 / §3.3 的原文是「槽位预分配于 RingBuffer 实例化时」「msg 定长槽
//! 拷贝」⇒「log() 期间不分配大块内存」。**在 Rust 的 `String` 模型下这条不成立**：
//! 「定长槽」要么用 `[u8; N]`（UTF-8 截断会切在多字节字符中间），要么用
//! `String`（每次写入分配）。⇒ 本实现按 STAGE-13 §2.6 裁决 M **只承诺长度上界**
//! （上面三条字节上限）与 **UTF-8 边界截断**，**不**承诺零分配（台账 **D-128**
//! 如实登记「定长槽语义降级为有界长度」）。
//!
//! # ⚠️ 日志的**订阅面**仍是进程内的（裁决 I / L），但**管理员通道**已上契约层
//!
//! LLD §3.4 的角色边界：产品 / 嵌入式工作台**不订阅**系统日志流（L1 边界）⇒
//! 本模块的 `LogEntry` / `LogLevel` / `LogQuery` / `LogBuffer` **仍然是进程内类型**
//! （内核与宿主同进程）⇒ **前端零订阅改动**是那条边界的可验证判别式。
//!
//! ⚠️ **v0.44（STAGE-29 · D-129）**：**管理员通道**要跨进程边界 ⇒ 契约层
//! **新增了一份形状** [`riscv_sim_contract::admin::LogEntryDto`] ⇒ 但**转换住在
//! 本模块**（日志条目是**它**拥有的形状 · 裁决 I）⇒ **契约层不依赖内核**
//! （依赖方向仍是单向：内核 → 契约层）。
//!
//! ⚠️ **这不是「日志流上契约层」**：订阅面（L1 角色边界）**一个字未改** ⇒
//! 变的只有**管理员通道**这一条显式出口（宿主侧有开关 · 默认关 · 裁决 G）。
//!
//! # 其它实现约定
//!
//! * **虚拟时间由调用方传入** —— 真相是 M-12 的时钟（评审 21-4），本模块**不**
//!   自己取时钟 ⇒ 导出里的 `time_virt_ps` 是**真实值**，不是恒 0。
//! * **入口过滤**：低于阈值 ⇒ 直接丢弃（不入带、**不计** `records_written`、
//!   **计** `dropped_by_filter`），切换即时生效（`set_level` 返回**旧**阈值）。
//! * **`query` 是纯读**：不移动任何游标、不改任何计数 ⇒ 连续两次 `query` 结果相同。
//! * **结果顺序**：`query` / `export` 的结果按**级别带升序**（TRACE → ERROR），
//!   带内按**写入先后**（旧 → 新）。规格未规定顺序 ⇒ 这是本实现选定的口径。
//!
//! # 全局最近口径（`query_recent` · 台账 **D-134**）
//!
//! `query` 的形状是「**带 → 条目**」两层（管理员通常要「错误级的全部」）⇒
//! **跨带没有全局时间序** ⇒ 「**全局最近 N 条**」是**另一个形状**（要按虚拟
//! 时间排序的**扁平**列表）⇒ ⇒ 本模块**新增**一个入口 [`LogBuffer::query_recent`]，
//! **既有的 `query` 一个字未改**（带状返回是管理员视图的既有形状 ⇒ 改了会破掉
//! 既有断言 ⇒ 那是破坏性变更 ⇒ 要升版 ⇒ 本批不升版）。
//!
//! * **排序键 = `time_virt_ps`（虚拟时间）** ⇒ ⇒ **不用**宿主时间（宿主时间
//!   **不可复现** ⇒ 同一份快照两次回放的顺序会不同 ⇒ 违反「确定性范围」那条
//!   架构约束）。
//! * **同刻条目的次序是确定的**：排序键 = `(time_virt_ps, 带序, 带内写入序)`
//!   ⇒ 带序 = TRACE → DEBUG → INFO → WARN → ERROR（= `LogLevel::rank()`）；
//!   带内写入序 = 该带里的写入先后 ⇒ ⇒ 两次跑**逐条相同**。

use riscv_sim_contract::admin::LogEntryDto;
use riscv_sim_contract::error::ErrorFamily;
use std::collections::VecDeque;

/// 上下文（`ctx`）字段的字节上界。
const MAX_CTX_BYTES: usize = 24;
/// 错误码（`err_code`）字段的字节上界。
const MAX_ERR_CODE_BYTES: usize = 12;
/// 消息（`msg`）字段的字节上界。
const MAX_MSG_BYTES: usize = 128;

/// CSV 导出的表头 —— 与 [`LogEntry`] 的 8 个字段**一一对应**（顺序也一致）。
const CSV_HEADER: &str = "time_virt_ps,time_host_ms,session_id,level,ctx,err_code,msg,truncated";

/// 日志级别（五级）。⚠️ **没有 `Fatal`** —— 致命属 M-22 的 `Severity`（裁决 C）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl LogLevel {
    /// `"TRACE"` / `"DEBUG"` / `"INFO"` / `"WARN"` / `"ERROR"`
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "TRACE",
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }

    /// 五级序（Trace = 0 … Error = 4）—— 过滤判据 `level.rank() >= filter.rank()`
    pub fn rank(self) -> u8 {
        // 判别式就是规范里的等级序（`Trace = 0` … `Error = 4`）⇒ 单一真相。
        self as u8
    }

    /// 该级的默认带容量（WASM 形态，见 §2.5）
    pub fn default_capacity(self) -> usize {
        LevelCapacities::default().of(self)
    }
}

/// 各带容量（可配置覆盖；默认 = §2.5 的 WASM 形态）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelCapacities {
    pub trace: usize,
    pub debug: usize,
    pub info: usize,
    pub warn: usize,
    pub error: usize,
}

impl Default for LevelCapacities {
    fn default() -> Self {
        // WASM 压缩形态（§2.5 裁决 E）：合计 7 000 条。
        Self {
            trace: 4_000,
            debug: 1_000,
            info: 1_000,
            warn: 500,
            error: 500,
        }
    }
}

impl LevelCapacities {
    /// 服务端 / 导出形态的容量（§2.5）
    pub fn server_profile() -> Self {
        Self {
            trace: 100_000,
            debug: 10_000,
            info: 10_000,
            warn: 5_000,
            error: 5_000,
        }
    }

    /// 查某一级的带容量
    pub fn of(self, level: LogLevel) -> usize {
        match level {
            LogLevel::Trace => self.trace,
            LogLevel::Debug => self.debug,
            LogLevel::Info => self.info,
            LogLevel::Warn => self.warn,
            LogLevel::Error => self.error,
        }
    }
}

/// 结构化日志条目（LLD M-21 §2.1.1 的 `logEntry`，8 字段）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// 虚拟时间（ps）—— 真相是 M-12 的时钟，调用方传入
    pub time_virt_ps: u64,
    /// 宿主时间（ms，单调）
    pub time_host_ms: u64,
    /// 会话关联标签（MVP 恒为 0，见 §2.8）
    pub session_id: u32,
    pub level: LogLevel,
    /// 「模块-流程点」标识，≤ 24 字节
    pub ctx: String,
    /// 完整场景码，可空，≤ 12 字节
    pub err_code: String,
    /// 消息文本，≤ 128 字节
    pub msg: String,
    /// 任一字段被截断时为 true
    pub truncated: bool,
}

impl LogEntry {
    /// 跨进程形状（契约 v8 · STAGE-29 · D-129）—— **逐字段镜像**。
    ///
    /// ⚠️ **转换住在 M-21**（日志条目是**它**拥有的形状 · 裁决 I）⇒ 契约层
    /// **不依赖内核**（依赖方向仍单向：内核 → 契约层）。
    ///
    /// ⚠️ **两处类型不一样**（这正是转换存在的理由）：
    ///
    /// - `level` 是**枚举** [`LogLevel`] ⇒ 线上是**字符串** ⇒ 取**内核自己的
    ///   拼写**（`TRACE` / `DEBUG` / `INFO` / `WARN` / `ERROR`，**大写** ·
    ///   **没有** `fatal` 那一级）；
    /// - `err_code` 是**普通字符串**，**空串 = 没有码** ⇒ 线上是
    ///   `Option<String>` ⇒ **空串 ⇒ `None`**（“没有码”与“空码”**分得开**）。
    ///
    /// ⚠️ 两个时间字段是 `u64` ⇒ **直接复制**（十进制字符串的策略由契约层的
    /// `u64_decimal` 负责 · 裁决 J）。
    pub fn to_dto(&self) -> LogEntryDto {
        LogEntryDto {
            time_virt_ps: self.time_virt_ps,
            time_host_ms: self.time_host_ms,
            session_id: self.session_id,
            level: self.level.as_str().to_string(),
            ctx: self.ctx.clone(),
            err_code: if self.err_code.is_empty() {
                None
            } else {
                Some(self.err_code.clone())
            },
            msg: self.msg.clone(),
            truncated: self.truncated,
        }
    }
}

/// 查询过滤（管理员通道）
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    /// 最低级别（含）
    pub min_level: Option<LogLevel>,
    /// 会话过滤
    pub session_id: Option<u32>,
    /// 虚拟时间下界（含）
    pub from_virt_ps: Option<u64>,
    /// 虚拟时间上界（含）
    pub to_virt_ps: Option<u64>,
}

/// 按级别分带的环形缓冲（五带，§2.5）
#[derive(Debug, Clone)]
pub struct LogBuffer {
    session_id: u32,
    filter: LogLevel,
    /// ⚠️ 规格 §3.1 的结构体清单里没列这一格，但 `with_capacities` 必须**存住**
    /// 容量才能判溢出 ⇒ 私有字段，不进公开 API。
    capacities: LevelCapacities,
    bands: [VecDeque<LogEntry>; 5],
    overflow_counts: [u64; 5],
    written: u64, // **入缓冲**的条数（被过滤掉的**不计**）
    dropped_by_filter: u64,
}

impl LogBuffer {
    /// 新缓冲（session_id 注入；默认容量 = WASM 形态）
    pub fn new(session_id: u32) -> Self {
        Self::with_capacities(session_id, LevelCapacities::default())
    }

    /// 新缓冲（自定义容量）
    pub fn with_capacities(session_id: u32, caps: LevelCapacities) -> Self {
        Self {
            session_id,
            // 默认阈值 = TRACE ⇒ **默认不过滤**（默认不丢任何一条）。
            filter: LogLevel::Trace,
            capacities: caps,
            bands: std::array::from_fn(|_| VecDeque::new()),
            overflow_counts: [0; 5],
            written: 0,
            dropped_by_filter: 0,
        }
    }

    /// 统一采集入口。**虚拟时间由调用方传入**（真相是 M-12 的时钟，§3.1 注）。
    pub fn log(
        &mut self,
        level: LogLevel,
        ctx: &str,
        msg: &str,
        err_code: &str,
        time_virt_ps: u64,
        time_host_ms: u64,
    ) {
        // ① 入口过滤：低于阈值 ⇒ 直接丢弃（不入带、不计 written、计 dropped）。
        if level.rank() < self.filter.rank() {
            self.dropped_by_filter += 1;
            return;
        }
        // ② 逐字段独立截断（UTF-8 字符边界）⇒ 任一被截则 truncated = true。
        let (ctx, ctx_cut) = truncate_utf8(ctx, MAX_CTX_BYTES);
        let (err_code, code_cut) = truncate_utf8(err_code, MAX_ERR_CODE_BYTES);
        let (msg, msg_cut) = truncate_utf8(msg, MAX_MSG_BYTES);
        let entry = LogEntry {
            time_virt_ps,
            time_host_ms,
            session_id: self.session_id,
            level,
            ctx,
            err_code,
            msg,
            truncated: ctx_cut || code_cut || msg_cut,
        };
        // ③ 入带：满了 ⇒ 丢该带最旧一条 + 该带溢出计数（别的带不受影响）。
        let idx = usize::from(level.rank());
        let capacity = self.capacities.of(level);
        let band = &mut self.bands[idx];
        band.push_back(entry);
        while band.len() > capacity {
            band.pop_front();
            self.overflow_counts[idx] += 1;
        }
        self.written += 1;
    }

    /// 管理员检索：**快照读**，不移动游标、不改缓冲状态（LLD 评审 21-2）
    pub fn query(&self, filter: &LogQuery) -> Vec<LogEntry> {
        let mut out = Vec::new();
        for band in &self.bands {
            for entry in band {
                if matches_query(entry, filter) {
                    out.push(entry.clone());
                }
            }
        }
        out
    }

    /// 管理员检索：**全局最近 N 条**（按**虚拟时间**排序的**扁平**列表）。
    ///
    /// ⚠️ **排序键 = `time_virt_ps`（虚拟时间）** —— **不用**宿主时间
    ///   （宿主时间**不可复现** ⇒ 同一份快照两次回放的顺序会不同 ⇒ 违反
    ///   「确定性范围」那条架构约束）。
    /// ⚠️ **同刻（虚拟时间相同）条目的次序是确定的**：排序键 =
    ///   `(time_virt_ps, 带序, 带内写入序)` ⇒ 带序 = TRACE → DEBUG → INFO →
    ///   WARN → ERROR（= `LogLevel::rank()`）；带内写入序 = 该带里的写入先后
    ///   ⇒ ⇒ 两次跑**逐条相同**。
    /// ⚠️ **既有的 [`LogBuffer::query`] 一个字未改**（带状返回是管理员视图的
    ///   既有形状）⇒ ⇒ 本入口是**新增**的另一种形状。
    ///
    /// `n` 的边界：`0` ⇒ **空**；`> 命中条数` ⇒ **全部**（按序）。
    ///
    /// 返回的是**虚拟时间上最新**的那 `n` 条（按虚拟时间**升序**排列 ⇒
    /// 最后一条是最新的）。
    pub fn query_recent(&self, filter: &LogQuery, n: usize) -> Vec<LogEntry> {
        if n == 0 {
            return Vec::new();
        }
        // 收集：按**带序**（TRACE → ERROR）逐带收 ⇒ 带内是写入先后 ⇒
        // 这个初始顺序就是「(带序, 带内写入序)」。
        let mut out: Vec<LogEntry> = Vec::new();
        for band in &self.bands {
            for entry in band {
                if matches_query(entry, filter) {
                    out.push(entry.clone());
                }
            }
        }
        // ⚠️ `sort_by_key` 是**稳定**排序 ⇒ 同刻条目保持上面的初始顺序。
        out.sort_by_key(|entry| entry.time_virt_ps);
        // 取**最后** n 条（虚拟时间上最新的那些）⇒ 「最近」名副其实。
        let start = out.len().saturating_sub(n);
        out.split_off(start)
    }

    /// 导出 CSV（分块内容拼成一段；字段与 §3.1 的 8 字段一一对应）
    pub fn export_csv(&self, filter: &LogQuery) -> String {
        let mut out = String::with_capacity(CSV_HEADER.len() + 64);
        out.push_str(CSV_HEADER);
        out.push('\n');
        for entry in self.query(filter) {
            out.push_str(&entry.time_virt_ps.to_string());
            out.push(',');
            out.push_str(&entry.time_host_ms.to_string());
            out.push(',');
            out.push_str(&entry.session_id.to_string());
            out.push(',');
            out.push_str(entry.level.as_str());
            out.push(',');
            out.push_str(&csv_field(&entry.ctx));
            out.push(',');
            out.push_str(&csv_field(&entry.err_code));
            out.push(',');
            out.push_str(&csv_field(&entry.msg));
            out.push(',');
            out.push_str(if entry.truncated { "true" } else { "false" });
            out.push('\n');
        }
        out
    }

    /// 导出 JSON（同上）
    pub fn export_json(&self, filter: &LogQuery) -> String {
        let mut out = String::from("[");
        for (i, entry) in self.query(filter).iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            out.push_str("\"time_virt_ps\":");
            out.push_str(&entry.time_virt_ps.to_string());
            out.push_str(",\"time_host_ms\":");
            out.push_str(&entry.time_host_ms.to_string());
            out.push_str(",\"session_id\":");
            out.push_str(&entry.session_id.to_string());
            out.push_str(",\"level\":\"");
            out.push_str(entry.level.as_str());
            out.push_str("\",\"ctx\":");
            out.push_str(&json_string(&entry.ctx));
            out.push_str(",\"err_code\":");
            out.push_str(&json_string(&entry.err_code));
            out.push_str(",\"msg\":");
            out.push_str(&json_string(&entry.msg));
            out.push_str(",\"truncated\":");
            out.push_str(if entry.truncated { "true" } else { "false" });
            out.push('}');
        }
        out.push(']');
        out
    }

    /// 运行时级别切换（仅管理员）。返回**旧**阈值。
    pub fn set_level(&mut self, level: LogLevel) -> LogLevel {
        std::mem::replace(&mut self.filter, level)
    }

    /// 当前阈值
    pub fn level(&self) -> LogLevel {
        self.filter
    }

    /// 总溢出次数（各带之和）
    pub fn overflow_count(&self) -> u64 {
        self.overflow_counts.iter().sum()
    }

    /// 某级的溢出次数
    pub fn overflow_count_of(&self, level: LogLevel) -> u64 {
        self.overflow_counts[usize::from(level.rank())]
    }

    /// 入缓冲的条数（被过滤掉的不计）
    pub fn records_written(&self) -> u64 {
        self.written
    }

    /// 被级别过滤丢弃的条数
    pub fn dropped_by_filter(&self) -> u64 {
        self.dropped_by_filter
    }

    /// 当前驻留条数
    pub fn len(&self) -> usize {
        self.bands.iter().map(VecDeque::len).sum()
    }

    /// 缓冲是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 清空（保留计数器）
    pub fn clear(&mut self) {
        for band in &mut self.bands {
            band.clear();
        }
    }

    /// 会话标签
    pub fn session_id(&self) -> u32 {
        self.session_id
    }
}

/// 查询判据（`min_level` / `session_id` / 虚拟时间区间）。
fn matches_query(entry: &LogEntry, query: &LogQuery) -> bool {
    if query
        .min_level
        .is_some_and(|min| entry.level.rank() < min.rank())
    {
        return false;
    }
    if query.session_id.is_some_and(|sid| entry.session_id != sid) {
        return false;
    }
    if query
        .from_virt_ps
        .is_some_and(|from| entry.time_virt_ps < from)
    {
        return false;
    }
    if query.to_virt_ps.is_some_and(|to| entry.time_virt_ps > to) {
        return false;
    }
    true
}

/// UTF-8 字符边界截断：返回（截断后的字符串，是否被截断）。
///
/// ⚠️ **绝不切半个字符** —— 切点回退到最近的字符边界。
fn truncate_utf8(s: &str, max_bytes: usize) -> (String, bool) {
    if s.len() <= max_bytes {
        return (s.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// CSV 字段转义（含逗号 / 引号 / 换行的字段加引号，里面的引号成对写出）。
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for ch in s.chars() {
            if ch == '"' {
                out.push('"');
            }
            out.push(ch);
        }
        out.push('"');
        out
    } else {
        s.to_string()
    }
}

/// JSON 字符串字面量（含引号与转义）。
fn json_string(s: &str) -> String {
    // `&str` 的序列化不会失败 ⇒ 失败分支只是防御性兜底。
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Family used by errors raised from this module.
pub const LOG_FAMILY: ErrorFamily = ErrorFamily::Context;

#[cfg(test)]
mod tests;
