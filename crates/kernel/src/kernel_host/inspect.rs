//! 调试面可观测计数：M-19 波形管道、M-18 探针边沿 / trace 环、M-11 符号表 /
//! DWARF 状态与反汇编缓存、执行轨迹环的只读读面。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。

use super::KernelHost;
use crate::m11_symbol::SymbolService;
use crate::m19_waveform::SharedPipeline;

impl KernelHost {
    /// M-19 波形管道（D-46）—— 边沿的环形缓冲。
    ///
    /// 供「**M-18 → M-19 的握手真的在跑**」这类断言用：把
    /// `set_edge_sink` 那行去掉的话，环里的条数恒为 0，断言立刻变红
    /// （V-A 检视 · 🟠-1 —— 此前 M-19 在生产路径上是**死代码**，
    /// 删掉握手整套用例照样全绿）。
    pub fn waveform_pipeline(&self) -> Option<&SharedPipeline> {
        self.waveform.as_ref()
    }

    /// 探针管道**装配失败**的累计次数（V-A 检视 · 🟡-6 / 🟡-7）。
    ///
    /// 非零 = 有通道或回调没接上，波形视图会少东西——但宿主不会报错，
    /// 所以这个计数器是唯一的信号。
    pub fn probe_bridge_failures(&self) -> u32 {
        self.probe_bridge_failures
    }

    /// 因超出 [`MAX_WAVEFORM_EDGES_PER_EVENT`] 而被截断的边沿条数。
    ///
    /// 非零 = 有一条事件里的边沿**不全**（保留的是最新的那些）。
    pub fn waveform_edges_truncated(&self) -> u64 {
        self.waveform_edges_truncated
    }

    /// M-18 的边沿存储因达到**每通道上限**而丢弃的边沿条数。
    ///
    /// 非零 = 边沿时间轴上有一个**洞**（最旧的那批被淘汰了）。它与
    /// [`KernelHost::waveform_edges_truncated`] 是**两层**上限：这一层
    /// 管**内存**，那一层管**单条事件的大小**。
    pub fn probe_edges_dropped(&self) -> u64 {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.dropped_edge_count())
            .unwrap_or(0)
    }

    /// 当前总线上装了几个 bus hook。
    ///
    /// 正常情况下是 **2 个**：M-17 的观察点匹配器 + M-18 的访存 trace 环
    /// （D-46）。**换内存图会换总线**——新总线上的 hook 从零开始，
    /// 管道必须在上面重装一遍，否则访存 trace 环会**静默**收不到任何
    /// 访存（M-11 的数据源就断了）。
    pub fn bus_hook_count(&self) -> usize {
        self.backend.bus().hook_count()
    }

    // -- M-11 调试面的可观测性（契约 v3） -----------------------------------

    /// M-11 的反汇编缓存里有多少条目（按地址索引）。
    ///
    /// 用来钉住「失效覆盖被写地址的指令区间」（裁决 G）：失效一个地址之后，
    /// 这个数应当**减少 1**，而不是清零。
    pub fn disasm_cache_len(&self) -> usize {
        self.disassembler.cache_len()
    }

    /// M-11 的反汇编缓存**命中**过多少次（`Disassembler::lookup`）。
    ///
    /// ⚠️ 规格 §9.4 说「`disasmCache` 可以兜住（访存轨迹那次重复取指）」
    /// ⇒ 这个计数是那句话的**可观测**证据：访存轨迹转移时若命中缓存，
    /// 宿主的**总线读次数**就少了一次。
    pub fn disasm_cache_hits(&self) -> u64 {
        self.disassembler.lookup_hits()
    }

    /// M-11 的符号表里有多少条符号。
    pub fn symbol_count(&self) -> usize {
        self.index.symbol_count()
    }

    /// 按**名字**查符号（走 M-11 的 [`SymbolService`] **端口**）。
    ///
    /// 未知名 ⇒ `None`（**不是错误**，验收 #7）。
    ///
    /// ⚠️ **走端口而不是直接摸 `SymbolTable`**（v0.20 检视 B · D-4）：端口
    /// 此前**全仓 0 处实现** ⇒ 它是**死代码**，「按地址查符号」这条能力
    /// 也**没有生产路径**。⇒ 宿主走端口 ⇒ 端口有实现者、也有调用者。
    pub fn lookup_symbol(&self, name: &str) -> Option<riscv_sim_contract::model::Symbol> {
        self.index.lookup_symbol(name).ok().flatten()
    }

    /// 按**地址**查符号（走 M-11 的 [`SymbolService`] **端口**）—— 命中
    /// 「起始地址 ≤ `addr` 且**最近**的那个」。
    ///
    /// 比所有符号的起始地址都小 ⇒ `None`（**不是错误**）。
    pub fn symbol_at(&self, addr: u32) -> Option<riscv_sim_contract::model::Symbol> {
        self.index.symbol_at(addr).ok().flatten()
    }

    /// M-11 的 DWARF 状态（`Absent` = 这个会话没有 DWARF）。
    pub fn dwarf_status(&self) -> crate::m11_symbol::DwarfStatus {
        self.index.dwarf_status()
    }

    /// 按**名字前缀**查符号（v0.57 批 · STAGE-42）。
    ///
    /// ⚠️ **走 M-11 的端口**（[`SymbolService::lookup_prefix`]）⇒ ⇒ 与
    /// [`Self::lookup_symbol`] 走**同一条**路 ⇒ ⇒ 「按名字查」与「按前缀
    /// 查」**不会**给出两套不同的答案。
    pub fn lookup_symbol_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Vec<riscv_sim_contract::model::Symbol> {
        self.index.lookup_prefix(prefix, limit).unwrap_or_default()
    }

    /// M-11 的执行轨迹环里有多少条目。
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// M-11 的执行轨迹环累计丢弃了多少条（溢出）。
    pub fn trace_dropped(&self) -> u64 {
        self.trace.dropped()
    }

    /// 执行轨迹的记录开关（裁决 H）。
    pub fn trace_enabled(&self) -> bool {
        self.trace.is_enabled()
    }

    /// M-18 的访存 trace 环里现在有多少条目。
    ///
    /// ⚠️ 这是 **D-51 的闭环观测点**：宿主每跑完一个批次就把它**转移**进
    /// M-11 的环 ⇒ 跑完一批之后这个数应当回到 **0**（M-18 的环不再
    /// 无界增长）。
    pub fn probe_trace_len(&self) -> usize {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.trace_len())
            .unwrap_or(0)
    }

    /// M-18 的访存 trace 环**累计丢了多少条**（环溢出，丢最旧）。
    ///
    /// ⚠️ **非零 = 失效通道有损**（v0.20 检视 A · 重要 1）：这个环同时是
    /// **反汇编缓存的失效通道**（裁决 G：访存写事件 ⇒ 失效覆盖指令区间）⇒
    /// 写事件被丢掉 ⇒ 缓存条目不失效 ⇒ **反汇编给出旧机器码**。
    ///
    /// ⇒ 宿主把 `StepN` 按指令预算**分块**（[`KernelHost::step_in_chunks`]）
    /// ⇒ 单块事件数 ≪ 环的容量 ⇒ 这个数**恒为 0**。
    pub fn probe_trace_dropped(&self) -> u64 {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.trace_dropped())
            .unwrap_or(0)
    }

    /// M-18 的**边沿存储**里现在有多少条边沿。
    ///
    /// ⚠️ 这是 **D-50 的判别点**：M-18 的边沿存储与 M-19 的环是**两份**
    /// 数据。把 M-18 的存储抽干（订阅会推一次波形历史）之后，波形导出
    /// **仍然**有内容 ⇒ 它读的是 M-19 的环。
    pub fn probe_edge_count(&self) -> usize {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.edge_count())
            .unwrap_or(0)
    }

    /// 被**丢弃**的 M-18 访存事件数（宿主侧的镜像写 + trace 关闭期间的事件
    /// + 切换开关时的清场）。
    ///
    /// 非零 = 有事件**没有**进 M-11 的执行轨迹环。丢弃**可见**，不静默。
    pub fn trace_events_discarded(&self) -> u64 {
        self.trace_events_discarded
    }
}
