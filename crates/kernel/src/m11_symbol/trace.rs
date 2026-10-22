//! M-11 执行轨迹环形缓冲 + 导出 (Execution Trace Ring Buffer / Export).
//!
//! 冻结规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`
//! （§2 裁决 D「一条导出命令、两条数据源」· §2 裁决 H「trace 性能开关」
//! · §5.2 验收 #13 / #14 / #15 / #16 / #17）。
//! 模块设计：`docs/detailed-design/M-11_SymbolDwarfTrace.html`
//! （§2.1 表47 `traceBuffer` · 11-2 溢出丢最旧 · 11-6 kind 映射 · 11-7 续导）。
//!
//! # 这个环**是什么**
//!
//! M-11 自己的**执行轨迹**（指令 + 访存）环形缓冲：
//!
//! - **写入方** = 宿主（`kernel_host`）—— M-17 的取指钩子（指令）与
//!   M-18 的访存事件流（访存）；
//! - **读出方** = 契约命令 `ExportTrace { source: Execution, .. }`
//!   （裁决 D：**执行轨迹的数据源就在 M-11 自己这里**，宿主只做路由）。
//!
//! ⚠️ 本模块**不依赖** M-19 —— 波形导出由宿主直接调 M-19（裁决 D），
//! 执行轨迹导出才是 M-11 的活 ⇒ 依赖方向与 LLD §9.1 一致。
//!
//! # 三条不可动摇的不变式
//!
//! 1. **开关关闭 ⇒ 零开销**（裁决 H）：`enabled == false` 时 `record*`
//!    **立即返回** —— 不分配、不入环、不计数。环的数组本身也是**懒分配**
//!    的：第一条记录真的进来时才建。
//! 2. **溢出只丢一条最旧**（11-2）：环满 ⇒ 写旧 head 位置再前移，
//!    `dropped` 逐条累加，**不告警风暴**。
//! 3. **VCD 不适用**（裁决 D）：执行轨迹是「指令 / 访存流水」，不是
//!    「信号随时间变化」⇒ `TraceFormat::Vcd` 一律**拒绝**（`E_CTX-03`），
//!    绝不「换一个格式凑合」。
//!
//! # 导出形状（**被测试逐字钉住**）
//!
//! - **CSV**：表头 `time_ps,addr,kind,data,width`。表头**只出现在
//!   `offset == 0` 的那一块** —— 续导的后续块只有数据行 ⇒ 各块拼起来
//!   就是**一份** CSV（而不是多行重复表头）。
//! - **JSON**：`{"entries":[{"time_ps":"…","addr":N,"kind":"…","data":N,"width":N}, …]}`
//!   —— 每块**自包含**（JSON 没有「表头」的概念，调用方按需拼接）。
//! - **时间列是十进制字符串**（契约层 `numstr` 的数值策略）：虚拟时间是
//!   64 位 picosecond 计数，超过 `2^53` 后 JSON number 会丢精度 ⇒
//!   CSV 单元**带引号**（`"1000"`）、JSON 是**字符串**（`"1000"`）。
//!   Rust 内部仍然是 `u64`（算术不受影响）。
//!
//! ⚠️ 本批**没有**筛选器（LLD §3.3 的时间 / 地址筛选未实现）⇒ `total`
//! 就是环内条目数。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{TraceDir, TraceEvent, TraceFormat};

/// 默认环容量（LLD 表47：`T_CAP = 10^5` 条目，估算基线）。
///
/// ⚠️ 这是**估算基线**，LLD 明说「实现后实测校准」⇒ 校准是 P1 项，
/// 不是本批的活。
pub const TRACE_CAP: usize = 100_000;

/// 单块导出的默认条目上限（`limit == 0` 时使用）。
///
/// 取 10⁴ 条：一块 CSV 约 10⁴ × 24 B ≈ 240 KiB，JSON 约 10⁴ × 80 B
/// ≈ 800 KiB —— 单块在浏览器里不至于卡住主线程，又不至于让续导
/// 往返太碎（每块一次命令 / 一次事件）。
pub const EXPORT_DEFAULT_LIMIT: u32 = 10_000;

/// 错误码场景号：非法导出格式（`E_CTX-03`）。
///
/// ⚠️ 「一场景一码」：宿主把 `ExportTrace` 的非法 `source`/`format`
/// 组合报成 `E_CTX-03`（STAGE-5 §3.4），M-11 侧的**执行轨迹**拒绝
/// VCD 也落在**同一个码**上 —— 两者是同一件事：「这个组合不合法」。
pub const E_CTX_03_ILLEGAL_EXPORT_FORMAT: u16 = 3;

/// 导出类错误归属的族（`E_CTX`）。
pub const TRACE_FAMILY: ErrorFamily = ErrorFamily::Context;

/// CSV 的表头行（**第一块**才带；续导的后续块只有数据行）。
pub const CSV_HEADER: &str = "time_ps,addr,kind,data,width";

/// 轨迹条目的种类（LLD 11-6：`kind = 0/1/2`）。
///
/// ⚠️ **方向语义以 M-18 的 [`TraceDir`] 为准**（裁决 G 的同款纪律）：
/// 不新造一套方向常量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraceKind {
    /// `kind = 0`：指令执行（来自 M-17 的取指钩子，`data` = 机器码，
    /// `width` = 0）。
    #[default]
    Instruction,
    /// `kind = 1`：访存读（M-18 访存事件，`dir = read`）。
    Read,
    /// `kind = 2`：访存写（M-18 访存事件，`dir = write`）。
    Write,
}

impl TraceKind {
    /// LLD 11-6 的数值 kind 码（0 / 1 / 2）。
    pub fn as_code(self) -> u8 {
        match self {
            TraceKind::Instruction => 0,
            TraceKind::Read => 1,
            TraceKind::Write => 2,
        }
    }

    /// 导出里使用的**稳定英文标识**（CSV / JSON 一致）。
    ///
    /// ⚠️ 全小写 snake_case —— 导出是给**外部工具**看的，标识必须
    /// 稳定且不随内部枚举改名而变。
    pub fn as_str(self) -> &'static str {
        match self {
            TraceKind::Instruction => "instruction",
            TraceKind::Read => "read",
            TraceKind::Write => "write",
        }
    }

    /// M-18 的访存方向 → kind（LLD 11-6：`dir = 0` → `kind = 1`（读）、
    /// `dir = 1` → `kind = 2`（写））。
    pub fn from_dir(dir: TraceDir) -> Self {
        match dir {
            TraceDir::Read => TraceKind::Read,
            TraceDir::Write => TraceKind::Write,
        }
    }
}

/// 执行轨迹的一条记录（LLD 表47 `TraceEntry`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TraceEntry {
    /// 虚拟时间（M-12 权威时钟，picosecond）。
    ///
    /// ⚠️ 线上（导出内容里）走**十进制字符串**（契约层 `numstr` 的数值
    /// 策略）；Rust 内部仍是 `u64`。
    pub time_virt: u64,
    /// 指令地址 / 访存地址。
    pub addr: u32,
    /// 条目种类。
    pub kind: TraceKind,
    /// 指令机器码，或访存值。
    ///
    /// ⚠️ **诚实点**：M-18 的访存事件流（契约 `TraceEvent`）**不带值**
    /// ⇒ 用 [`TraceBuffer::record`] 记进来的访存条目 `data` 恒为 0；
    /// 只有指令条目带机器码，只有走 [`TraceBuffer::record_with_value`]
    /// 的访存条目才带值。**不猜值。**
    pub data: u32,
    /// 访存宽度（字节）；指令条目 = 0。
    pub width: u8,
}

/// 一块导出结果（LLD 11-7 的续导协议）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportChunk {
    /// 本块起点（条目偏移）。
    pub offset: u64,
    /// 下一块起点（= 本块末尾 + 1；已到末尾 ⇒ 等于 `total`）。
    ///
    /// 调用方把它**原样**当成下一次 `export` 的 `offset` ⇒ 不重不漏。
    pub next_offset: u64,
    /// 命中的总条目数。
    pub total: u64,
    /// 本块序列化字节数。
    pub bytes: u64,
    /// 本块内容（文本）。
    pub chunk: String,
}

/// 执行轨迹环形缓冲（LLD 表47 `traceBuffer`）。
///
/// ⚠️ 环满 ⇒ **每次仅丢一条最旧**（11-2），`dropped` 计数准确。
#[derive(Debug, Clone)]
pub struct TraceBuffer {
    /// 环数组（**懒分配**：第一条记录进来时才建）。
    buf: Vec<TraceEntry>,
    /// 环容量（`buf.len()` 在分配后恒等于它）。
    cap: usize,
    /// 写游标（下一个写入位置；满时它指向**最旧**的那一条）。
    head: usize,
    /// 当前条目数（≤ `cap`）。
    len: usize,
    /// 累计丢弃（溢出）计数。
    dropped: u64,
    /// 性能开关（裁决 H）。
    enabled: bool,
}

impl Default for TraceBuffer {
    fn default() -> Self {
        Self::new(TRACE_CAP)
    }
}

impl TraceBuffer {
    /// 新建一个容量为 `cap` 的环。
    ///
    /// ⚠️ `cap` 必须 > 0 —— 容量是**装配期**的决策，不是运行期输入
    /// （与 M-18 `TraceStream::new` 的同一纪律）。
    ///
    /// ⚠️ **默认 `enabled = true`**：环的容量有界（[`TRACE_CAP`]），
    /// 而 D-51 的闭环要求「访存 trace 环**有消费者**」⇒ 默认不写会让
    /// 导出永远是空的。关掉它是**调试态之外的提速手段**（评审 R9：
    /// 「退出调试态可关闭恢复全速」）。
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "trace 环容量必须 > 0（默认值见 TRACE_CAP）");
        Self {
            buf: Vec::new(),
            cap,
            head: 0,
            len: 0,
            dropped: 0,
            enabled: true,
        }
    }

    /// 环容量（条目数）。
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// 性能开关（裁决 H）。
    ///
    /// `false` ⇒ 之后的 `record*` **立即返回**（零开销）；
    /// `true` ⇒ 开始记录（宿主负责显示降速提示，不在本模块职责内）。
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// 当前是否在记录。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 记录一条 **M-18 访存事件**。
    ///
    /// ⚠️ M-18 的事件**不带值** ⇒ 条目的 `data` 恒为 0（见
    /// [`TraceEntry::data`]）。要带值请用 [`TraceBuffer::record_with_value`]。
    ///
    /// 开关关闭时**立即返回**（不分配、不入环、不计数）。
    pub fn record(&mut self, ev: &TraceEvent) {
        if !self.enabled {
            return;
        }
        self.push(TraceEntry {
            time_virt: ev.t_ps,
            addr: ev.addr,
            kind: TraceKind::from_dir(ev.dir),
            data: 0,
            width: ev.width,
        });
    }

    /// 记录一条**带值**的访存事件（调用方手里有值时用）。
    ///
    /// 开关关闭时**立即返回**。
    pub fn record_with_value(&mut self, ev: &TraceEvent, data: u32) {
        if !self.enabled {
            return;
        }
        self.push(TraceEntry {
            time_virt: ev.t_ps,
            addr: ev.addr,
            kind: TraceKind::from_dir(ev.dir),
            data,
            width: ev.width,
        });
    }

    /// 记录一条**指令执行**（来自 M-17 的取指钩子）。
    ///
    /// `code` 是指令机器码（原码），`width` = 0（LLD 11-6：指令条目的
    /// 宽度恒为 0）。
    ///
    /// 开关关闭时**立即返回**。
    pub fn record_instruction(&mut self, time_virt: u64, addr: u32, code: u32) {
        if !self.enabled {
            return;
        }
        self.push(TraceEntry {
            time_virt,
            addr,
            kind: TraceKind::Instruction,
            data: code,
            width: 0,
        });
    }

    /// 记录一条**原始条目**。
    ///
    /// 开关关闭时**立即返回**。
    pub fn record_entry(&mut self, entry: TraceEntry) {
        if !self.enabled {
            return;
        }
        self.push(entry);
    }

    /// 当前条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 当前是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 累计丢弃（溢出）条数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 按**时间顺序**（最旧 → 最新）遍历环内条目。
    ///
    /// ⚠️ 溢出之后，环里留下的是**最新的**那 `cap` 条 —— 遍历顺序仍是
    /// 时间顺序。
    pub fn entries(&self) -> impl Iterator<Item = &TraceEntry> {
        let oldest = self.oldest_index();
        let cap = self.cap;
        (0..self.len).map(move |i| &self.buf[(oldest + i) % cap])
    }

    /// 清空环（装载新工程 / 重新开始时用）。
    ///
    /// ⚠️ **重置 `dropped`** —— 丢弃计数是「本轮」的账，不是会话终生的账。
    /// `enabled` **不动**（开关是用户的决定，不是数据的属性）。
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.dropped = 0;
    }

    /// 导出**一块**（续导协议，LLD 11-7）。
    ///
    /// - `offset` 的单位是**条目数**（不是字节）；
    /// - `limit == 0` ⇒ 用 [`EXPORT_DEFAULT_LIMIT`]；
    /// - `offset > total` ⇒ 返回**空块**（`next_offset == offset`），
    ///   **不是错误**（调用方可能拿着上一轮的 offset 来续导）；
    /// - `TraceFormat::Vcd` ⇒ **拒绝**（`E_CTX-03`，裁决 D）。
    ///
    /// 续导用法：把返回的 `next_offset` 原样当成下一次调用的 `offset`，
    /// 直到 `next_offset == total` ⇒ 累计条目数**恰好** == `total`
    /// （不重不漏）。
    pub fn export(
        &self,
        format: TraceFormat,
        offset: u64,
        limit: u32,
    ) -> Result<ExportChunk, PlatformError> {
        let total = self.len as u64;
        let limit = if limit == 0 {
            EXPORT_DEFAULT_LIMIT
        } else {
            limit
        };
        // ⚠️ 越界 ⇒ 空块（**不是**错误）。
        let take: usize = if offset >= total {
            0
        } else {
            let remaining = (total - offset).min(u32::MAX as u64) as u32;
            limit.min(remaining) as usize
        };

        let text = match format {
            TraceFormat::Csv => Ok(self.render_csv(offset, take)),
            TraceFormat::Json => Ok(self.render_json(offset, take)),
            TraceFormat::Vcd => Err(PlatformError::new(
                TRACE_FAMILY,
                E_CTX_03_ILLEGAL_EXPORT_FORMAT,
                "执行轨迹不支持 VCD 导出（VCD 描述信号随时间变化，不适用于指令/访存流水）",
            )),
        }?;

        let next_offset = offset + take as u64;
        Ok(ExportChunk {
            offset,
            next_offset,
            total,
            bytes: text.len() as u64,
            chunk: text,
        })
    }

    // -----------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------

    /// 入环（**只在开关打开时被调用**）。
    ///
    /// ⚠️ 溢出语义逐字照抄 LLD 11-2 的伪代码：**写旧 head 位置再前移**
    /// ⇒ 每次**仅**丢一条最旧。
    fn push(&mut self, entry: TraceEntry) {
        // 懒分配：环数组在第一条记录进来时才建（开关关闭 ⇒ 零分配）。
        if self.buf.len() != self.cap {
            self.buf = vec![TraceEntry::default(); self.cap];
        }
        if self.len == self.cap {
            self.buf[self.head] = entry;
            self.head = (self.head + 1) % self.cap;
            self.dropped += 1;
        } else {
            self.buf[(self.head + self.len) % self.cap] = entry;
            self.len += 1;
        }
    }

    /// 最旧一条在环数组里的下标。
    ///
    /// ⚠️ 本环的布局（照抄 LLD 11-2 的伪代码）是「**条目从 `head` 起
    /// 连续存放**」：`head` 是**下一个写入位置**，因此**最旧的一条就在
    /// `head`** —— 环满时 `head` 恰好指向最旧的那一条（写它、再前移）。
    fn oldest_index(&self) -> usize {
        self.head
    }

    /// 渲染 CSV 块。
    ///
    /// ⚠️ **表头只在 `offset == 0` 的那块出现** ⇒ 各块拼起来是一份 CSV。
    fn render_csv(&self, offset: u64, take: usize) -> String {
        let mut out = String::new();
        if offset == 0 && take > 0 {
            out.push_str(CSV_HEADER);
            out.push('\n');
        }
        for e in self.entries().skip(offset as usize).take(take) {
            // ⚠️ 时间是**带引号的十进制字符串** —— CSV 没有类型系统，
            // 引号是唯一能让读者看出「这不是一个整数」的办法。
            out.push('"');
            out.push_str(&e.time_virt.to_string());
            out.push_str("\",");
            out.push_str(&e.addr.to_string());
            out.push(',');
            out.push_str(e.kind.as_str());
            out.push(',');
            out.push_str(&e.data.to_string());
            out.push(',');
            out.push_str(&e.width.to_string());
            out.push('\n');
        }
        out
    }

    /// 渲染 JSON 块。
    ///
    /// ⚠️ 手工拼字符串而不是用 `serde`：本模块**只输出**固定形状，
    /// 里面没有任何需要转义的自由文本（时间 = 数字，kind = 固定标识），
    /// 所以手拼既能逐字钉住形状，也不会引入 `serde` 依赖
    /// （kernel crate 没有 `serde` 依赖）。
    fn render_json(&self, offset: u64, take: usize) -> String {
        let mut out = String::from("{\"entries\":[");
        let mut first = true;
        for e in self.entries().skip(offset as usize).take(take) {
            if !first {
                out.push(',');
            }
            first = false;
            out.push_str("{\"time_ps\":\"");
            out.push_str(&e.time_virt.to_string());
            out.push_str("\",\"addr\":");
            out.push_str(&e.addr.to_string());
            out.push_str(",\"kind\":\"");
            out.push_str(e.kind.as_str());
            out.push_str("\",\"data\":");
            out.push_str(&e.data.to_string());
            out.push_str(",\"width\":");
            out.push_str(&e.width.to_string());
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::TraceEvent;

    /// 一条访存事件。
    fn mem(t_ps: u64, addr: u32, width: u8, dir: TraceDir) -> TraceEvent {
        TraceEvent {
            t_ps,
            addr,
            width,
            dir,
            is_fetch: false,
        }
    }

    /// 一个装了 `n` 条指令条目的环（第 i 条的地址是 `0x1000 + i`）。
    fn ring_with_instructions(n: usize) -> TraceBuffer {
        let mut buf = TraceBuffer::new(64);
        for i in 0..n {
            buf.record_instruction(1000 + i as u64, 0x1000 + i as u32, 0x13);
        }
        buf
    }

    // -----------------------------------------------------------------
    // 1. 性能开关（裁决 H）
    // -----------------------------------------------------------------

    #[test]
    fn a_disabled_buffer_records_nothing() {
        let mut buf = TraceBuffer::new(64);
        buf.set_enabled(false);
        assert!(!buf.is_enabled());

        buf.record(&mem(1, 0x2000, 4, TraceDir::Read));
        buf.record(&mem(2, 0x2004, 4, TraceDir::Write));
        buf.record_instruction(3, 0x3000, 0x13);
        buf.record_entry(TraceEntry::default());

        assert_eq!(buf.len(), 0, "开关关闭 ⇒ 一条都不能进环");
        assert_eq!(buf.dropped(), 0);
        assert_eq!(buf.entries().count(), 0);
    }

    #[test]
    fn enabling_the_switch_starts_recording() {
        let mut buf = TraceBuffer::new(64);
        buf.set_enabled(false);
        buf.record_instruction(1, 0x1000, 0x13);
        assert_eq!(buf.len(), 0);

        buf.set_enabled(true);
        assert!(buf.is_enabled());
        buf.record_instruction(2, 0x1004, 0x13);
        assert_eq!(buf.len(), 1, "开关打开 ⇒ 开始记录");
        assert_eq!(buf.entries().next().map(|e| e.addr), Some(0x1004));
    }

    // -----------------------------------------------------------------
    // 2. 溢出：每次仅丢一条最旧（11-2）
    // -----------------------------------------------------------------

    #[test]
    fn overflow_drops_only_the_oldest_entry() {
        let cap = 8usize;
        let mut buf = TraceBuffer::new(cap);
        for i in 0..(cap + 5) {
            buf.record_instruction(i as u64, 0x1000 + i as u32, i as u32);
        }

        assert_eq!(buf.len(), cap, "环最多装 cap 条");
        assert_eq!(buf.dropped(), 5, "丢了 5 条就要**准确**记 5");

        // 最新的 cap 条**完整**保留，最旧的 5 条**确实**没了。
        let addrs: Vec<u32> = buf.entries().map(|e| e.addr).collect();
        assert_eq!(
            addrs,
            (5..(cap as u32 + 5))
                .map(|i| 0x1000 + i)
                .collect::<Vec<_>>(),
            "留下的必须是**最新**的 cap 条"
        );
        // 逐条断言（不只看首尾）。
        for (idx, e) in buf.entries().enumerate() {
            assert_eq!(e.addr, 0x1000 + 5 + idx as u32);
            assert_eq!(e.time_virt, 5 + idx as u64);
        }
    }

    #[test]
    fn the_ring_stays_chronological_after_overflow() {
        let mut buf = TraceBuffer::new(4);
        for i in 0..10u64 {
            buf.record_instruction(i, 0x1000 + i as u32, 0);
        }
        let times: Vec<u64> = buf.entries().map(|e| e.time_virt).collect();
        assert_eq!(times, vec![6, 7, 8, 9], "溢出后仍然是时间顺序");
    }

    // -----------------------------------------------------------------
    // 3. kind 映射（LLD 11-6）
    // -----------------------------------------------------------------

    #[test]
    fn the_m18_direction_maps_onto_the_kind() {
        assert_eq!(TraceKind::from_dir(TraceDir::Read), TraceKind::Read);
        assert_eq!(TraceKind::from_dir(TraceDir::Write), TraceKind::Write);

        let mut buf = TraceBuffer::new(8);
        buf.record(&mem(1, 0x2000, 4, TraceDir::Read));
        buf.record(&mem(2, 0x2004, 1, TraceDir::Write));

        let entries: Vec<TraceEntry> = buf.entries().copied().collect();
        assert_eq!(entries[0].kind, TraceKind::Read);
        assert_eq!(entries[0].width, 4);
        assert_eq!(entries[1].kind, TraceKind::Write);
        assert_eq!(entries[1].width, 1);
    }

    #[test]
    fn the_kind_codes_follow_the_lld() {
        assert_eq!(TraceKind::Instruction.as_code(), 0);
        assert_eq!(TraceKind::Read.as_code(), 1);
        assert_eq!(TraceKind::Write.as_code(), 2);
        assert_eq!(TraceKind::Instruction.as_str(), "instruction");
        assert_eq!(TraceKind::Read.as_str(), "read");
        assert_eq!(TraceKind::Write.as_str(), "write");
    }

    // -----------------------------------------------------------------
    // 4. 导出格式校验（裁决 D）
    // -----------------------------------------------------------------

    #[test]
    fn vcd_is_rejected_for_the_execution_trace() {
        let buf = ring_with_instructions(4);
        let err = buf
            .export(TraceFormat::Vcd, 0, 0)
            .expect_err("执行轨迹不是信号波形 ⇒ VCD 不适用");
        assert_eq!(err.family, ErrorFamily::Context);
        assert_eq!(err.scenario, E_CTX_03_ILLEGAL_EXPORT_FORMAT);
        assert!(err.to_string().starts_with("E_CTX-03:"), "实际：{err}");
    }

    #[test]
    fn csv_and_json_are_accepted() {
        let buf = ring_with_instructions(4);
        assert!(buf.export(TraceFormat::Csv, 0, 0).is_ok());
        assert!(buf.export(TraceFormat::Json, 0, 0).is_ok());
    }

    // -----------------------------------------------------------------
    // 5. 续导：不重不漏（LLD 11-7）
    // -----------------------------------------------------------------

    #[test]
    fn the_continuation_covers_every_entry_exactly_once() {
        let n = 37usize;
        let buf = ring_with_instructions(n);

        let mut seen: Vec<u32> = Vec::new();
        let mut offset = 0u64;
        let mut guard = 0;
        loop {
            let chunk = buf
                .export(TraceFormat::Json, offset, 5)
                .expect("导出必须成功");
            assert_eq!(chunk.total, n as u64);
            assert_eq!(chunk.offset, offset);

            // 每块自己解析出来，数条目。
            let v: serde_json::Value = serde_json::from_str(&chunk.chunk).expect("JSON 必须可解析");
            let entries = v["entries"].as_array().expect("entries 必须是数组");
            for e in entries {
                let addr = e["addr"].as_u64().expect("addr 必须是数字");
                seen.push(addr as u32);
            }

            assert!(
                chunk.next_offset > offset || chunk.next_offset == chunk.total,
                "next_offset 必须递进（或已到末尾）"
            );
            offset = chunk.next_offset;
            guard += 1;
            assert!(guard < 100, "续导必须收敛");
            if offset >= chunk.total {
                break;
            }
        }

        assert_eq!(seen.len(), n, "累计条目数必须**恰好** == total");
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), n, "**不能有重复**");
        let expected: Vec<u32> = (0..n).map(|i| 0x1000 + i as u32).collect();
        assert_eq!(seen, expected, "顺序必须是时间顺序且一条不漏");
    }

    #[test]
    fn the_csv_continuation_also_loses_nothing() {
        let n = 11usize;
        let buf = ring_with_instructions(n);

        let mut rows = 0usize;
        let mut offset = 0u64;
        let mut guard = 0;
        loop {
            let chunk = buf
                .export(TraceFormat::Csv, offset, 3)
                .expect("导出必须成功");
            let mut lines: Vec<&str> = chunk.chunk.lines().collect();
            if chunk.offset == 0 {
                // 第一块带表头 —— 续导拼起来才是一份 CSV。
                assert_eq!(lines.remove(0), CSV_HEADER);
            }
            rows += lines.len();
            offset = chunk.next_offset;
            // ⚠️ 守卫：`next_offset` 不推进 ⇒ **立刻失败**而不是把测试
            // 进程挂死（挂死的测试比红的测试难查得多）。
            guard += 1;
            assert!(guard < 100, "续导必须收敛（next_offset 不推进）");
            if offset >= chunk.total {
                break;
            }
        }
        assert_eq!(rows, n, "累计数据行数必须 == total");
    }

    // -----------------------------------------------------------------
    // 6. offset 越界 ⇒ 空块（不是错误）
    // -----------------------------------------------------------------

    #[test]
    fn an_offset_past_the_end_yields_an_empty_chunk() {
        let buf = ring_with_instructions(4);
        let chunk = buf
            .export(TraceFormat::Json, 99, 10)
            .expect("越界**不是**错误");
        assert_eq!(chunk.offset, 99);
        assert_eq!(chunk.next_offset, 99, "空块 ⇒ next_offset == offset");
        assert_eq!(chunk.total, 4);
        assert_eq!(
            chunk.chunk, "{\"entries\":[]}",
            "JSON 的空块是**空数组**（不是空串——空串不是合法 JSON）"
        );
        assert_eq!(chunk.bytes, chunk.chunk.len() as u64);
    }

    #[test]
    fn an_out_of_range_offset_yields_an_empty_csv_block() {
        let buf = ring_with_instructions(4);
        let chunk = buf.export(TraceFormat::Csv, 99, 10).expect("越界不是错误");
        assert_eq!(chunk.next_offset, 99);
        assert_eq!(chunk.total, 4);
        assert!(chunk.chunk.is_empty(), "CSV 的空块就是空文本");
        assert_eq!(chunk.bytes, 0);
    }

    #[test]
    fn an_offset_at_the_end_yields_an_empty_chunk() {
        let buf = ring_with_instructions(4);
        let chunk = buf.export(TraceFormat::Csv, 4, 10).expect("空块");
        assert_eq!(chunk.next_offset, 4);
        assert_eq!(chunk.total, 4);
        assert!(chunk.chunk.is_empty());
    }

    // -----------------------------------------------------------------
    // 7. CSV / JSON 的形状被钉住
    // -----------------------------------------------------------------

    #[test]
    fn the_csv_shape_is_pinned() {
        let mut buf = TraceBuffer::new(16);
        buf.record_instruction(1000, 0x1000, 0x13);
        buf.record(&mem(2000, 0x2000, 4, TraceDir::Read));
        buf.record_with_value(&mem(3000, 0x3000, 4, TraceDir::Write), 0xdead_beef);

        let chunk = buf.export(TraceFormat::Csv, 0, 0).expect("导出");
        assert_eq!(
            chunk.chunk,
            "time_ps,addr,kind,data,width\n\
             \"1000\",4096,instruction,19,0\n\
             \"2000\",8192,read,0,4\n\
             \"3000\",12288,write,3735928559,4\n",
            "CSV 的表头与每一行都**逐字**钉住"
        );
        assert_eq!(chunk.bytes, chunk.chunk.len() as u64);
    }

    #[test]
    fn the_json_shape_is_pinned() {
        let mut buf = TraceBuffer::new(16);
        buf.record_instruction(1000, 0x1000, 0x13);
        buf.record(&mem(2000, 0x2000, 4, TraceDir::Read));

        let chunk = buf.export(TraceFormat::Json, 0, 0).expect("导出");
        assert_eq!(
            chunk.chunk,
            "{\"entries\":[\
             {\"time_ps\":\"1000\",\"addr\":4096,\"kind\":\"instruction\",\"data\":19,\"width\":0},\
             {\"time_ps\":\"2000\",\"addr\":8192,\"kind\":\"read\",\"data\":0,\"width\":4}\
             ]}",
            "JSON 的形状**逐字**钉住"
        );
    }

    #[test]
    fn the_json_entry_keys_are_exactly_these_five() {
        let buf = ring_with_instructions(1);
        let chunk = buf.export(TraceFormat::Json, 0, 0).expect("导出");
        let v: serde_json::Value = serde_json::from_str(&chunk.chunk).expect("JSON");
        let entry = &v["entries"][0];
        let mut keys: Vec<&str> = entry
            .as_object()
            .expect("对象")
            .keys()
            .map(String::as_str)
            .collect();
        // ⚠️ 排序后再断言 —— `serde_json` 的 Map 默认是 BTreeMap
        // （键按字典序），断言**集合**而不是**顺序**，免得依赖
        // `preserve_order` feature 有没有开。
        keys.sort_unstable();
        assert_eq!(keys, vec!["addr", "data", "kind", "time_ps", "width"]);
    }

    // -----------------------------------------------------------------
    // 8. 虚拟时间走十进制字符串（契约层数值策略）
    // -----------------------------------------------------------------

    #[test]
    fn the_virtual_time_travels_as_a_decimal_string() {
        // 超过 2^53 的时间：JSON number 会丢精度 ⇒ 必须走字符串。
        let big = 9_007_199_254_740_993u64;
        let mut buf = TraceBuffer::new(8);
        buf.record_instruction(big, 0x1000, 0x13);

        let csv = buf.export(TraceFormat::Csv, 0, 0).expect("导出");
        assert!(
            csv.chunk.contains("\"9007199254740993\","),
            "CSV 的时间单元必须是**带引号的十进制字符串**，实际：\n{}",
            csv.chunk
        );

        let json = buf.export(TraceFormat::Json, 0, 0).expect("导出");
        let v: serde_json::Value = serde_json::from_str(&json.chunk).expect("JSON");
        assert_eq!(
            v["entries"][0]["time_ps"],
            serde_json::json!("9007199254740993")
        );
        assert!(
            v["entries"][0]["time_ps"].is_string(),
            "时间必须是字符串，不是 JSON number"
        );
        // 其余字段仍是数字（u32 精确）。
        assert_eq!(v["entries"][0]["addr"], serde_json::json!(4096));
        assert_eq!(v["entries"][0]["width"], serde_json::json!(0));
    }

    // -----------------------------------------------------------------
    // 9. limit == 0 ⇒ 默认上限
    // -----------------------------------------------------------------

    #[test]
    fn a_zero_limit_uses_the_default_limit() {
        let n = EXPORT_DEFAULT_LIMIT as usize + 5;
        let mut buf = TraceBuffer::new(n + 8);
        for i in 0..n {
            buf.record_instruction(i as u64, 0x1000 + i as u32, 0);
        }
        let chunk = buf.export(TraceFormat::Json, 0, 0).expect("导出");
        let v: serde_json::Value = serde_json::from_str(&chunk.chunk).expect("JSON");
        assert_eq!(
            v["entries"].as_array().expect("数组").len(),
            EXPORT_DEFAULT_LIMIT as usize,
            "limit == 0 ⇒ 用内核默认上限"
        );
        assert_eq!(chunk.next_offset, EXPORT_DEFAULT_LIMIT as u64);
    }

    // -----------------------------------------------------------------
    // 10. 杂项
    // -----------------------------------------------------------------

    #[test]
    fn an_empty_ring_exports_an_empty_chunk() {
        let buf = TraceBuffer::new(8);
        let csv = buf.export(TraceFormat::Csv, 0, 0).expect("导出");
        assert_eq!(csv.total, 0);
        assert_eq!(csv.next_offset, 0);
        assert!(csv.chunk.is_empty(), "空环 ⇒ 空块（连表头都没有）");

        let json = buf.export(TraceFormat::Json, 0, 0).expect("导出");
        assert_eq!(json.chunk, "{\"entries\":[]}");
    }

    #[test]
    fn clearing_resets_the_ring_and_the_drop_counter() {
        let mut buf = TraceBuffer::new(4);
        for i in 0..10u32 {
            buf.record_instruction(i as u64, 0x1000 + i, 0);
        }
        assert_eq!(buf.dropped(), 6);
        buf.clear();
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.dropped(), 0, "清空 ⇒ 丢弃账也归零");
        assert!(buf.is_empty());
    }
}
