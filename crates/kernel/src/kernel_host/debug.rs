//! 调试面事件：反汇编 / 行表 / 符号解析 / 内存转储 / 轨迹与波形导出，以及
//! M-18 访存 trace 环向 M-11 轨迹环的转移 / 丢弃 / drain。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。被父模块调度路径
//! 直接调用的方法提为 `pub(super)`，族内辅助保持私有。

use super::image::{fetch_bytes, illegal_export, source_location_event, waveform_export_event};
use super::pipeline_unavailable;
use super::KernelHost;
use crate::m11_symbol::{LineLookup, SymbolService};
use crate::m19_waveform::ExportFilter;
use riscv_sim_contract::error::{
    PlatformError, DEBUG_FAMILY, E_DBG_03_DUMP_TOO_LARGE, E_DBG_04_SYMBOL_NAME_EMPTY,
};
use riscv_sim_contract::model::{TraceEvent, TraceFilter, TraceFormat, TraceSource};
use riscv_sim_contract::protocol::EventPayload;
use riscv_sim_contract::{DUMP_MAX_BYTES, SYMBOL_CANDIDATE_LIMIT};

impl KernelHost {
    /// 反汇编（契约 v3 · `Disasm`）。
    ///
    /// * 取指来源 = **M-14 的总线**（经 M-11 注入的回调）：M-11 **不**
    ///   依赖 M-17 / M-14 ⇒ 依赖方向单向（LLD §9.1）。
    /// * 读未映射 ⇒ **提前结束序列**、返回**部分序列**（**不是**错误，
    ///   LLD 11-3）——M-11 的 `disasm` 自己处理，宿主只把结果透传。
    /// * `count > MAX_DISASM_COUNT` ⇒ **拒绝**（`E_CTX-02`，M-11 自己
    ///   抛的，宿主只透传）。
    ///
    /// ⚠️ 取指**必须真的只读 `size` 字节**（「读一个字取低半」会让映射区
    /// 末端的压缩指令越界误报，LLD 11-1）——见 [`fetch_bytes`]。
    pub(super) fn on_disasm(&mut self, addr: u32, count: u32) -> Vec<EventPayload> {
        // 借用拆分：`self.backend`（总线）、`self.index`（行表）与
        // `self.disassembler`（可变）是**三个不同的字段** ⇒ 可以同时借。
        //
        // ⚠️ **不 `clone()` 索引**：osz 的符号表 + 行表不小，每条 `Disasm`
        // 都克隆一遍是**白烧**（STAGE-9 裁决 C 的接线要求）。
        let bus = self.backend.bus();
        let index = &self.index;
        let disassembler = &mut self.disassembler;
        let mut read = |addr: u32, size: u32| fetch_bytes(bus, addr, size);
        // ⚠️ 结果**先落到局部变量**：`Err` 分支要调 `self.error_from`
        // （`&self`），而 `self.disassembler` 的可变借用在调用返回后就
        // 结束了 —— 不落局部的话，match 的借用区间会横跨整个 match。
        let result =
            disassembler.disasm_with_lines(addr, count, &mut read, |a| index.resolve_line_entry(a));
        match result {
            Ok(entries) => vec![EventPayload::Disasm { entries }],
            Err(err) => vec![self.error_from(&err)],
        }
    }

    /// 地址 → 源码行（契约 v3 · `ResolveLine`）。
    ///
    /// ⚠️ **两态可区分**（LLD 11-4）：没有 DWARF ⇒ `file = ""` / `line = 0`；
    /// 有 DWARF 但地址没命中 ⇒ `file = ""` / `line = u32::MAX`。
    ///
    /// ⚠️ **诚实口径**：宿主**不自己编造行号**，只把 M-11 的判定原样映射
    /// 成契约形状（映射点见 [`source_location_event`]`）。
    /// ⚠️ **v0.22 批（Wave 2a）**：M-11 的 `dwarf.rs` 已换成**真解析器**
    /// （`.debug_line` 的 v2/v3/v4/v5 行号程序）⇒ 这里**第一次**能拿到
    /// 真 `file` + 真 `line`；「查过行表、没命中」（`Missed`，`line =
    /// u32::MAX`）与「没查」（`NoDwarf`，`line = 0`）**仍然分得开**。
    pub(super) fn on_resolve_line(&mut self, addr: u32) -> Vec<EventPayload> {
        // ⚠️ **走 M-11 的端口**（`SymbolService::resolve_line`）—— 端口
        // 自己分得清「查过行表、没命中」（`Missed`）与「没查」（`NoDwarf`），
        // 宿主**不自己编造**行号（STAGE-7 裁决 J）。
        let lookup = self.index.resolve_line(addr).unwrap_or(LineLookup::NoDwarf);
        vec![source_location_event(addr, lookup)]
    }

    // -- 契约 v9 的调试面（STAGE-42 · v0.57 批） ------------------------------

    /// 按**名字**查符号（契约 v9 · `ResolveSymbol`）。
    ///
    /// ## 三种结果
    ///
    /// | 情况 | `symbol` | `candidates` | `total` |
    /// | --- | --- | --- | --- |
    /// | **精确命中** | `Some(..)` | **不含**它 | 该镜像符号总数 |
    /// | **只有前缀命中** | `None` | 有值 | 该镜像符号总数 |
    /// | **什么都没有** | `None` | 空 | `0` ⇒ 这个镜像**没有符号表** |
    ///
    /// ⚠️ **空名字 / 纯空白 ⇒ 拒绝**（`E_DBG_02`）⇒ ⇒ **不发**
    /// `SymbolResolved`（K-4）⇒ ⇒ 前端能分清「我没发对」与「这个镜像没有
    /// 符号」。
    ///
    /// ⚠️ **符号表是装载镜像时由 M-06 的 `.symtab` 建出来的**⇒ ⇒ 没装载
    /// 镜像 / 镜像被 strip ⇒ `total == 0` ⇒ ⇒ 这两态**可区分**（靠
    /// `total`）。
    ///
    /// ⚠️ **回显的是请求里的名字**（前端按它对上号）⇒ ⇒ 原样回显，不改写。
    pub(super) fn on_resolve_symbol(&mut self, name: String) -> Vec<EventPayload> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            // ⚠️ **不发** `SymbolResolved` ⇒ ⇒ 前端只会看到一条 `Error`。
            return vec![self.error_from(&PlatformError::new(
                DEBUG_FAMILY,
                E_DBG_04_SYMBOL_NAME_EMPTY,
                "符号名为空：无法查询符号（请给出符号名）",
            ))];
        }
        // ⚠️ **精确命中**（走端口 · 与 `lookup_symbol` 同一条路）。
        let exact = self.lookup_symbol(trimmed);
        let limit = SYMBOL_CANDIDATE_LIMIT as usize;
        // ⚠️ **多要一条** ⇒ ⇒ 精确命中被剔掉之后**仍然**能给出满
        //    `SYMBOL_CANDIDATE_LIMIT` 条候选（而不是少一条）。
        let mut candidates = self.lookup_symbol_prefix(trimmed, limit + 1);
        if let Some(exact) = &exact {
            candidates.retain(|candidate| candidate.name != exact.name);
        }
        candidates.truncate(limit);
        vec![EventPayload::SymbolResolved {
            name,
            symbol: exact,
            candidates,
            total: self.index.symbol_count() as u32,
        }]
    }

    /// 转储一段内存（契约 v9 · `DumpMemory`）。
    ///
    /// ⚠️ **原样拒，不静默截断**（裁决 D）：`len == 0` 或
    /// `len > DUMP_MAX_BYTES` ⇒ ⇒ `E_DBG_03` ⇒ ⇒ **不发** `MemoryDump`。
    ///
    /// ⚠️ **走调试视角**（`Bus::read_only`）⇒ ⇒ **不计**总线统计、**不**
    /// 触发探针、**不**匹配观察点（裁决 E）。
    ///
    /// ⚠️ **未映射 ⇒ 总线的错误原样透出**（不吞、不改写）⇒ ⇒ 前端看到
    /// 的是 `E_BUS-01` 与总线的原话。
    pub(super) fn on_dump_memory(&mut self, addr: u32, len: u32) -> Vec<EventPayload> {
        if len == 0 || len > DUMP_MAX_BYTES {
            return vec![self.error_from(&PlatformError::new(
                DEBUG_FAMILY,
                E_DBG_03_DUMP_TOO_LARGE,
                format!("转储长度越界：{len}（允许 1..={DUMP_MAX_BYTES} 字节）"),
            ))];
        }
        match self.backend.read_memory_bytes(addr, len) {
            Ok(bytes) => vec![EventPayload::MemoryDump { addr, bytes }],
            Err(err) => vec![self.error_from(&err)],
        }
    }

    /// 导出（契约 v3 · `ExportTrace`）—— **宿主路由**（STAGE-5 裁决 D）。
    ///
    /// | `source` | `format` | 数据源 |
    /// | --- | --- | --- |
    /// | `Waveform` | `Csv` / `Vcd` | **M-19 的环**（闭环 D-50） |
    /// | `Waveform` | `Json` | **拒绝**（`E_CTX-03`） |
    /// | `Execution` | `Csv` / `Json` | **M-11 的轨迹环**（闭环 D-51） |
    /// | `Execution` | `Vcd` | **拒绝**（`E_CTX-03`） |
    ///
    /// ⚠️ **路由在宿主**：M-11 不依赖 M-19，M-19 也不依赖 M-11 ⇒ LLD §9.1
    /// 的依赖方向不变。
    ///
    /// ⚠️ **`offset` 的单位是「条目数」**，M-19 的 `next_offset` 也是条目
    /// 索引 ⇒ **直接透传**，不换算成字节。
    pub(super) fn on_export_trace(
        &mut self,
        source: TraceSource,
        format: TraceFormat,
        offset: u64,
        limit: u32,
        filter: TraceFilter,
    ) -> Vec<EventPayload> {
        match source {
            // ⚠️ 筛选只对 **Execution** 有意义：波形导出的是「信号随时间
            // 变化」，按地址/时间窗口切会把一条边沿流切成不连续的片段。
            TraceSource::Waveform => self.export_waveform(format, offset),
            TraceSource::Execution => self.export_execution(format, offset, limit, &filter),
        }
    }

    /// 轨迹记录开关（契约 v3 · `SetTraceEnabled`，裁决 H）。
    ///
    /// * `enabled = true` ⇒ 宿主开始把访存轨迹**记录**进 M-11 的环；
    /// * `enabled = false` ⇒ 停（**零开销**：宿主**不调用** `record`，
    ///   M-11 的 `record` 自己也立即返回 —— 规格 §9.7 的「双保险」）。
    ///
    /// ⚠️ **drain 照做**（不受开关管）：M-18 的环是**有界**的，不 drain
    /// 它会涨到上限（常驻几 MB）⇒ 「关闭 trace」反而更费内存。
    ///
    /// 回复里回显 `enabled` + `len` + `dropped`（前端据此显示降速提示，
    /// 评审 R9）。
    ///
    /// ⚠️ **幂等**：重复设置同一个值是安全的（规格 §3.4）。
    pub(super) fn on_set_trace_enabled(&mut self, enabled: bool) -> Vec<EventPayload> {
        // ⚠️ **先清场、再切开关**：开关**切换**的那一刻做一次 drain，把
        // 「上一个状态」攒下的事件丢掉。
        //
        // - 打开：丢掉「关闭期间」攒的那批 —— 用户说「现在开始记录」，
        //   把旧的那批倒回来会违反「关闭 ⇒ 零记录」；
        // - 关闭：丢掉「切换之前」攒的那批 —— 它们属于上一个状态。
        //
        // ⚠️ 正常情况下此刻环里**已经是空的**（每批都 drain 过了）⇒ 这一步
        // 是**防御性**的：万一上一批在转移之前就出错了（例如访存报错），
        // 环里可能还留着半批 ⇒ 切换时清掉，免得它们混进下一个状态。
        self.discard_m18_trace();
        self.trace.set_enabled(enabled);
        vec![EventPayload::TraceState {
            enabled,
            len: self.trace.len() as u32,
            dropped: self.trace.dropped(),
        }]
    }

    /// 波形导出（`source = Waveform`）—— 数据源 = **M-19 的环**。
    ///
    /// ⚠️ **D-50 的要点**：数据源必须是 M-19 的环，**不是** M-18 的边沿
    /// 存储 —— 否则 M-19 的环永远是「只写不读」。
    ///
    /// ⚠️ `limit` 在这条路上**不适用**：M-19 的导出有自己的分块尺寸
    /// （`EXPORT_CHUNK = 10_000` 条），它的 API 里没有 limit 参数 ⇒ 宿主
    /// **不伪造**一个 limit（伪造会让续导协议算错 `next_offset`）。契约的
    /// `limit` 只约束 `source = Execution` 那条路。
    fn export_waveform(&mut self, format: TraceFormat, offset: u64) -> Vec<EventPayload> {
        if format == TraceFormat::Json {
            // M-19 没有 JSON 波形导出 ⇒ **拒绝**，不「换一个格式凑合」。
            return vec![self.error_from(&illegal_export(
                "波形导出不支持 JSON（M-19 只产出 CSV / VCD）",
            ))];
        }
        let Some(pipeline) = self.waveform.clone() else {
            return vec![self.error_from(&pipeline_unavailable(
                "波形管道尚未装配（没有可读的 M-19 环）",
            ))];
        };
        let Ok(mut pipe) = pipeline.try_pipeline_mut() else {
            // 管道正被占用（M-18 正在推边沿）——这是**内部瞬时**状态，
            // 不是客户端的请求有错。报出来而不是静默给一个空块（空块
            // 会被前端当成「这段会话没有波形」）。
            return vec![self.error_from(&pipeline_unavailable(
                "M-19 的波形管道正被占用（RefCell 争用），本次导出未执行",
            ))];
        };
        let filter = ExportFilter::all();
        let next_offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let handle = match format {
            TraceFormat::Csv => pipe.export_csv(&filter, next_offset, None),
            TraceFormat::Vcd => pipe.export_vcd(&filter, next_offset, None),
            // 上面已经拒了。
            TraceFormat::Json => unreachable!("JSON 波形导出在上面已被拒绝"),
        };
        vec![waveform_export_event(handle, format, offset)]
    }

    /// 执行轨迹导出（`source = Execution`）—— 数据源 = **M-11 的轨迹环**。
    ///
    /// ⚠️ `TraceFormat::Vcd` 由 M-11 自己拒（`E_CTX-03`）：执行轨迹不是
    /// 信号波形，VCD 语义不适用（STAGE-5 裁决 D）。
    fn export_execution(
        &mut self,
        format: TraceFormat,
        offset: u64,
        limit: u32,
        filter: &TraceFilter,
    ) -> Vec<EventPayload> {
        match self.trace.export_filtered(format, offset, limit, filter) {
            Ok(chunk) => vec![EventPayload::TraceExport {
                source: TraceSource::Execution,
                format,
                offset: chunk.offset,
                next_offset: chunk.next_offset,
                total: chunk.total,
                bytes: chunk.bytes,
                chunk: chunk.chunk,
                // ⚠️ M-11 的 `ExportChunk` **没有** `overflow_marked` ⇒ 用
                // 环的丢弃计数算（丢了最旧的条目 ⇒ 这份导出**不是**完整
                // 历史）。
                overflow_marked: self.trace.dropped() > 0,
            }],
            Err(err) => vec![self.error_from(&err)],
        }
    }

    /// 把 M-18 的访存 trace 环**转移**进 M-11 的轨迹环（闭环 D-51）。
    ///
    /// ⚠️ **drain 与转移必须同时发生**（台账 D-51）：「在宿主里定期 drain
    /// 掉」会把 M-11 要用的历史冲掉 ⇒ 这里**从不**单独 drain。
    ///
    /// ⚠️ **drain 照做，record 才受开关管**（规格 §9.7 的「零开销」口径）：
    /// 宿主**不调用** `record`、M-11 的 `record` 自己也立即返回（双保险）。
    ///
    /// ⚠️ **缓存失效与记录开关无关**（裁决 G）—— 这是本批修掉的一个
    /// **真实缺陷**：缓存失效是**正确性**问题（「访客改了代码，反汇编必须
    /// 跟着变」），记录开关是**性能**问题（「要不要把访存记进环里」）。
    /// 两者耦合 ⇒ 关闭 trace 期间访客自改代码 ⇒ 缓存**陈旧** ⇒ 用户看到的
    /// 反汇编是**旧的**。⇒ 失效**始终**执行，**只有 record** 受开关管。
    ///
    /// ⚠️ **关闭期间为什么还要 drain**：M-18 的环是**有界**的（`TRACE_CAP
    /// = 200_000`，常驻几 MB）⇒ 不 drain 它会涨到上限 ⇒ 「关闭 trace」反而
    /// 更费内存。drain 的成本 ≪ record 的成本（record 要 clone + 入环）。
    pub(super) fn transfer_trace(&mut self) {
        let enabled = self.trace.is_enabled();
        for ev in self.drain_m18_trace() {
            // ⚠️ **写 ⇒ 失效反汇编缓存**（裁决 G 的失效通道 = M-18 的写
            // 事件）。**先**失效，**后**决定要不要记录 —— 顺序不能反：
            // 失效是正确性，record 是性能。
            if !ev.is_fetch {
                self.disassembler.invalidate_on_access(ev.addr, ev.dir);
            }
            if !enabled {
                // 关闭 ⇒ **零记录**（裁决 H / 验收 #15）：事件被 drain 掉，
                // 但**不进** M-11 的环。
                //
                // ⚠️ 丢弃**可见**（计数暴露给调用方），不静默。
                self.trace_events_discarded += 1;
                continue;
            }
            if ev.is_fetch {
                // 取指 ⇒ 指令条目（kind = 0）⇒ **直接用取指事件当场带
                // 上来的原码**。不再事后按 PC 重取 —— 同一批次里访客
                // 自改代码时，重取会把第一条取指记成改写**后**的码
                // （D-87 的「时间倒流」）。
                self.trace.record_instruction(ev.t_ps, ev.addr, ev.value);
            } else {
                self.trace.record(&ev);
            }
        }
    }

    /// 把 M-18 的访存 trace 环**丢掉**（**有理由**的丢弃，不是「定期 drain」）。
    ///
    /// 两个调用点：
    ///
    /// 1. **装载工程后**：环里是 `load_bytes` 逐字节写镜像产生的**宿主侧**
    ///    写 —— 不是访客的执行历史（而且轨迹环是定容的，塞进去会把访客的
    ///    执行历史挤出去）。
    /// 2. **切换 trace 开关时**：用户说「现在开始记录」⇒ 记录从此刻开始；
    ///    把上一个状态攒下的那批倒回来会违反「关闭 ⇒ 零记录」。
    ///
    /// ⚠️ 丢弃**可见**（计数暴露给调用方），不静默。
    pub(super) fn discard_m18_trace(&mut self) {
        let events = self.drain_m18_trace();
        self.trace_events_discarded += events.len() as u64;
    }

    /// 从 M-18 的环里**取走**全部访存事件（**唯一**的 drain 点）。
    ///
    /// ⚠️ 探针注册表正被占用（总线派发路径里）⇒ 这一轮**放弃**：事件
    /// 留在环里（**不丢**），下一轮再取。
    fn drain_m18_trace(&mut self) -> Vec<TraceEvent> {
        let Some(registry) = self.probes.clone() else {
            return Vec::new();
        };
        let Ok(mut reg) = registry.try_borrow_mut() else {
            return Vec::new();
        };
        reg.drain_trace()
    }
}
