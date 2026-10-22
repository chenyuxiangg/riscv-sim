//! M-16 中断控制器 (Interrupt Controller).
//!
//! 简化 PLIC / CLINT 的**行为级等价**（LLD §1.1 / §3）：外设
//! `request(line)` → 挂起队列 → 量子边界采样（刷新使能 + 定时器轮询）→
//! 优先级仲裁 → 登记交付 → M-13 在取指边界**拉取**（`take_intr`）→ 陷入
//! `mtvec` → handler → `clear(line)` → 恢复执行。
//!
//! 冻结规格：`docs/detailed-design/STAGE-4_Interrupt_Controller.md`
//! （§1.2 裁决 A~H · §3.1~§3.8 · §4.1 跨 lane 的 Rust 签名）。
//!
//! # 三条不可动摇的不变式
//!
//! 1. **同线去重** —— 同一条线连来两次，挂起队列里只有 **1** 条
//!    （LLD 评审 16-2）。
//! 2. **仲裁确定性** —— 取**最高优先级**；同优先级按
//!    `(request_time_ps, insert_seq)` 取**最早**。排序键里**没有宿主
//!    时钟**，也没有哈希迭代序（队列是 `Vec`，优先级表是 `BTreeMap`）。
//! 3. **未使能不丢** —— 未使能的中断**保持挂起**；未使能的高优先级
//!    **不阻塞**已使能的低优先级（LLD 评审 16-1）。
//!
//! # 依赖方向
//!
//! 本模块**不依赖** M-12 / M-13 / M-17：
//!
//! - 量子边界采样由**调用方**驱动 —— 具体是 **M-17 的执行器**
//!   （`IssExecutor::step` 在**取指边界**上按「跨过量子」判据调
//!   [`InterruptController::poll`]），调用方把 CSR 文件里的 `mie` /
//!   `mstatus` 读出来传进 `poll`；
//! - 交付是**拉取式**的：M-17 在取指边界取走
//!   [`InterruptController::take_intr`]（LLD 评审 16-4），
//!   再交给 M-13 的**唯一**陷阱入口 `enter_trap`。
//!
//! ⚠️ **没有「M-12 的 `InterruptPoller` 接缝」** —— 那个接缝在
//! **STAGE-4 裁决 I** 里被**否掉**了：它会要求 M-12 持有一个闭包，
//! 而那个闭包要反过来借用 M-17 自己的 `Iss` ⇒ **自引用借用**（Rust 不允许）。
//! 接缝因此落在 **M-17 的执行器**里，M-12 的 `Engine` **零改动**。
//!
//! 本模块只认**契约层**的词汇（[`riscv_sim_contract::csr`] 的 CSR 编号 /
//! `mie` 位映射 / **架构 cause 码映射**）与**契约层**的会话载荷形状
//! （[`M16IntcDto`]）——一个约束一个落点。
//!
//! # 线号空间、`mie` 位号空间、cause 码空间是**三个**空间
//!
//! 平台的**中断线**是 `1`（UART rx-ready）/ `2`（UART tx-empty）/
//! `3`（timer-compare），`mie` 用的是 RISC-V 的**标准位号**
//! `11`（`MEIE`）/ `7`（`MTIE`），而交付给 guest 的 **`mcause`** 用的是
//! **架构 cause 码** `7`（定时器）/ `11`（外部）。
//!
//! * 线 → `mie` 位：映射**只有一处**
//!   （[`riscv_sim_contract::csr::std_irq_bit_for_line`]），本模块的使能
//!   刷新从它取；
//! * 线 → 架构 cause 码：映射**只有一处**
//!   （[`riscv_sim_contract::csr::arch_cause_for_line`]），**交付**从它取。
//!
//! ⚠️ **三个空间不许合并** —— 报 `mcause` 时用内部线号是**真 bug**
//! （真实 M 模式 RTOS 的中断分发按架构码写 ⇒ 线号一个都匹配不上）。
//!
//! ⚠️ **`mip` 今天的**唯一**消费者还没有实现** —— guest 读 `mip` 恒得 0
//! （平台是**推**式交付：中断直接陷入，不需要 guest 去轮询）。台账
//! **D-84** 记着这件事；`mie_bit_for_line` 的**反向**映射（线 → `mip` 位）
//! 是 P1 的落点。

use riscv_sim_contract::csr::{
    arch_mcause_for_line, mie_bit_for_line, IRQ_TIMER_COMPARE, IRQ_UART_RX_READY,
    IRQ_UART_TX_EMPTY, MSTATUS_MIE_BIT, NO_ACTIVE_IRQ,
};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::port::InterruptSink;
use riscv_sim_contract::session::{IrqPriorityDto, M16IntcDto, PendingIrqDto};
use std::collections::{BTreeMap, BTreeSet};

/// 触发模式：**电平**（timer-compare —— `mtime >= mtimecmp` 持续成立，
/// 改写 `mtimecmp` 之前不撤）。
pub const TRIGGER_LEVEL: u8 = 0;

/// 触发模式：**边沿**（UART rx-ready / tx-empty —— M-15 事件触发，
/// 单次请求）。
pub const TRIGGER_EDGE: u8 = 1;

/// 请求源类型：宿主 / 平台自己拉起的线（[`InterruptSink::raise`]）。
pub const SOURCE_PLATFORM: &str = "platform";

/// 请求源类型：定时器比较到期（控制器自己的定时器轮询）。
pub const SOURCE_TIMER: &str = "timer";

/// 使能掩码能表达的**最大线号**（掩码是 `u32`，bit L = line L）。
///
/// ⚠️ 线号 **≥ 32** 的中断**没有**使能位 ⇒ 它们可以挂起，但**永远
/// 不会被仲裁交付**（仲裁只认「挂起 ∧ 使能」）。平台今天只有 3 条线，
/// 这条边界是「掩码宽度」的自然结果，不是策略选择。
pub const MAX_MASKABLE_LINE: u16 = 31;

/// 一条待交付的中断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingIntr {
    /// 中断线号。
    ///
    /// ⚠️ 这是**平台的内部线号**（`1 / 2 / 3`）—— 它**不**是 guest 看
    /// 到的 `mcause`。
    pub line: u16,
    /// `mcause` 的值（中断 = `IRQ_BASE | <架构 cause 码>`）。
    ///
    /// ⚠️ **架构 cause 码**（定时器 `7` / 外部 `11`）由契约层的
    /// [`riscv_sim_contract::csr::arch_cause_for_line`] 给出 —— **不是**
    /// 内部线号。内部线号当 cause 报出去是**真 bug**：guest 的中断
    /// 分发按架构码写，一个都匹配不上。
    pub cause: u32,
}

/// 挂起队列里的一条中断。
///
/// ⚠️ 排序键是 `(request_time_ps, insert_seq)` —— **不含宿主时钟**，
/// 所以同一串时间戳化的输入事件序列必然给出同一串交付序列
/// （AD §8.1 D0：确定性）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingIrq {
    /// 中断线号。
    line: u16,
    /// 优先级（**数值越大越优先**）。
    prio: u8,
    /// 请求时的虚拟时间。
    request_time_ps: u64,
    /// 插入序（排序的次键 —— 同一虚拟时刻上的请求靠它定序）。
    insert_seq: u64,
    /// 触发模式（[`TRIGGER_LEVEL`] / [`TRIGGER_EDGE`]）。
    trigger_type: u8,
    /// 请求源类型（`"uart"` / `"gpio"` / `"timer"` / `"platform"`）。
    source_type: String,
}

impl PendingIrq {
    /// 该条目的**排序键**。
    fn key(&self) -> (u64, u64) {
        (self.request_time_ps, self.insert_seq)
    }
}

/// 平台的**默认优先级**（冻结规格 §1.2 裁决 D 的表）。
///
/// | 线 | 名称 | 默认优先级 |
/// | --- | --- | --- |
/// | 1 | UART rx-ready | 1 |
/// | 2 | UART tx-empty | 1 |
/// | 3 | timer-compare | 2（最高） |
///
/// ⚠️ **没有**在优先级表里显式设置过的线，仲裁时用这里的**回落值**。
fn default_priority(line: u16) -> u8 {
    match line {
        IRQ_TIMER_COMPARE => 2,
        IRQ_UART_RX_READY | IRQ_UART_TX_EMPTY => 1,
        _ => 0,
    }
}

/// M-17 需要的中断控制器能力。
///
/// ⚠️ **内核 crate 内部**的 trait，**不进契约层** —— 契约层已经有
/// [`InterruptSink`]（`raise` / `lower`），再加一个契约端口就是**第二个
/// 真相**。
///
/// ⚠️ 它必须**对象安全**：M-17 的执行器用 `&mut dyn InterruptPort` 持有
/// 它（M-17 自己就拥有控制器，闭包捕获它 = 自引用借用 ⇒ Rust 不允许）。
pub trait InterruptPort {
    /// 已登记但尚未被取走的交付。
    fn pending_intr(&self) -> Option<PendingIntr>;
    /// 取走交付（**取走即消失**）。
    fn take_intr(&mut self) -> Option<PendingIntr>;
    /// 清除一条线（移除挂起项 + 撤销线电平 + 复位 `active_irq`）。
    fn clear(&mut self, line: u16);
    /// 量子边界采样 + 仲裁。返回**本次登记的交付**（`None` = 本轮不交付）。
    fn poll(
        &mut self,
        now_ps: u64,
        mie: u32,
        mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError>;
    /// 当前**挂起**的中断线（**电平**语义：保持到 [`Self::clear`]）。
    ///
    /// ⚠️ 这是 CSR `mip` 的**唯一来源**（D-84）：`mip` = 这些线的
    /// **位掩码投影**（位号表见契约层 `std_irq_bit_for_line`）。
    ///
    /// ⚠️ **不是** `raised`（断言线集）—— `raised` 是**边沿**语义，
    /// 用完即清；`mip` 的位义是「中断**挂起**」。
    ///
    /// # `None` = **状态未知**（台账 **D-100**）
    ///
    /// 共享适配器（`Rc<RefCell<…>>`）在**借不到**的时候**读不到**
    /// 挂起队列 —— 那**不是**「没有挂起」，而是「**不知道**有没有挂起」。
    ///
    /// ⇒ 返回 `None` 表示「**读不到**」，调用方**必须**把它与「没有挂起」
    /// 分开处理：**读失败不得改写 guest 可见的状态**（把「读不到」当成
    /// 「没有挂起」⇒ 算出的 `mip` 掩码是 0 ⇒ 一次**失败的读**变成了
    /// 一次 guest 看得见的状态改写）。
    fn pending_lines(&self) -> Option<Vec<u16>>;
}

/// 中断控制器（简化 PLIC / CLINT）。
///
/// 状态与不变式见[模块文档](self)。
pub struct InterruptController {
    /// 挂起队列（按 `(request_time_ps, insert_seq)` 升序）。
    pending: Vec<PendingIrq>,
    /// 插入序计数器（排序的次键）。
    insert_seq: u64,
    /// line → 优先级。
    priorities: BTreeMap<u16, u8>,
    /// 使能掩码（**按 line 索引**：bit L = line L）。
    enabled_mask: u32,
    /// 当前已交付未清除的中断线（[`NO_ACTIVE_IRQ`] = 无）。
    active_irq: u16,
    /// 已断言的线（电平状态；边沿线用完即清）。
    raised: BTreeSet<u16>,
    /// 定时器计数（虚拟时间等价值）。
    mtime_ps: u64,
    /// 定时器比较值。
    mtimecmp_ps: u64,
    /// 交付交接结构（LLD 评审 16-4：M-13 拉取式）。
    pending_delivery: Option<PendingIntr>,
    /// 中断脉冲回调（→ M-18 `sample_irq_pulse`）。
    pulse_sink: Option<Box<dyn FnMut(u64, u16)>>,
    /// 最近一次 [`InterruptController::poll`] 收到的虚拟时间。
    ///
    /// ⚠️ **为什么需要它**：契约层的 [`InterruptSink::raise`] 只有
    /// `irq: u16` 一个参数 —— 它**拿不到**虚拟时间，而 `request()` 需要
    /// `now_ps`。所以控制器**缓存**最近一次采样的虚拟时间，`raise()` 用
    /// **缓存值**当请求时刻（一次 `poll` 都还没有过时是 `0`）。
    ///
    /// ⚠️ 这个缓存**必须是虚拟时间**、不能是宿主时钟：同一个种子 +
    /// 同一串时间戳化的输入事件序列 ⇒ 结果唯一（AD §8.1 D0）。
    last_now_ps: u64,
}

impl Default for InterruptController {
    fn default() -> Self {
        let mut priorities = BTreeMap::new();
        // 平台的默认优先级表 —— 显式落在表里，导出的载荷因此**看得见**
        // 每条线的优先级，而不是「表外的隐式回落」。
        for (line, prio) in [
            (IRQ_UART_RX_READY, default_priority(IRQ_UART_RX_READY)),
            (IRQ_UART_TX_EMPTY, default_priority(IRQ_UART_TX_EMPTY)),
            (IRQ_TIMER_COMPARE, default_priority(IRQ_TIMER_COMPARE)),
        ] {
            priorities.insert(line, prio);
        }
        Self {
            pending: Vec::new(),
            insert_seq: 0,
            priorities,
            enabled_mask: 0,
            active_irq: NO_ACTIVE_IRQ,
            raised: BTreeSet::new(),
            mtime_ps: 0,
            mtimecmp_ps: 0,
            pending_delivery: None,
            pulse_sink: None,
            last_now_ps: 0,
        }
    }
}

impl std::fmt::Debug for InterruptController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ⚠️ 手写 `Debug`：脉冲回调是 `Box<dyn FnMut …>`，**没有**
        // `Debug` —— 它不进调试输出（那只是一段闭包）。
        f.debug_struct("InterruptController")
            .field("pending", &self.pending)
            .field("insert_seq", &self.insert_seq)
            .field("priorities", &self.priorities)
            .field("enabled_mask", &self.enabled_mask)
            .field("active_irq", &self.active_irq)
            .field("raised", &self.raised)
            .field("mtime_ps", &self.mtime_ps)
            .field("mtimecmp_ps", &self.mtimecmp_ps)
            .field("pending_delivery", &self.pending_delivery)
            .field("has_pulse_sink", &self.pulse_sink.is_some())
            .field("last_now_ps", &self.last_now_ps)
            .finish()
    }
}

impl InterruptController {
    /// 新建一个空的控制器。
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------------------
    // 请求 / 清除
    // ------------------------------------------------------------------

    /// 登记一条挂起请求（**同线去重**）。
    ///
    /// 返回 `false` 表示该线**已在**挂起队列里（本次请求被折叠：
    /// 不追加、不改时刻、不发脉冲）。
    ///
    /// ⚠️ **虚拟时间是参数，不是内部读的** —— 排序键
    /// `(request_time_ps, insert_seq)` 必须由调用方给出，这样同一串
    /// 时间戳化的输入事件序列必然给出同一串交付序列。
    ///
    /// ⚠️ 本实现**当前没有失败路径**；签名里的 `Result` 是冻结规格
    /// §4.1 的要求（契约层的 [`InterruptSink`] 用同一个错误族报告
    /// 「控制器忙」）。
    pub fn request(
        &mut self,
        line: u16,
        now_ps: u64,
        source_type: &str,
        trigger_type: u8,
    ) -> Result<bool, PlatformError> {
        // 同线去重（LLD 评审 16-2）：已挂起即满足，**不堆积**。
        if self.pending.iter().any(|p| p.line == line) {
            return Ok(false);
        }

        let seq = self.insert_seq.wrapping_add(1);
        self.insert_seq = seq;
        let entry = PendingIrq {
            line,
            prio: self.priority_of(line),
            request_time_ps: now_ps,
            insert_seq: seq,
            trigger_type,
            source_type: source_type.to_string(),
        };
        // 插到**保持升序**的位置（`(request_time_ps, insert_seq)`）。
        let at = self.pending.partition_point(|p| p.key() <= entry.key());
        self.pending.insert(at, entry);

        // 线被断言 —— 队列是已断言线的**子集**（契约层校验的第 14 条）。
        self.raised.insert(line);

        // 中断脉冲（D-26）：**真的登记**了一条挂起才发 —— 被折叠的
        // 重复请求**不发**脉冲。
        if let Some(sink) = self.pulse_sink.as_mut() {
            sink(now_ps, line);
        }
        Ok(true)
    }

    /// 清除一条线：移除挂起项 + 撤销线电平 + 复位 `active_irq`。
    ///
    /// ⚠️ **已登记但尚未被取走的交付也一并丢弃** —— 「清掉了」就不能
    /// 再交付（LLD §7.1 的 `irqClear` 用例：挂起移除 + `activeIrq` 复位
    /// + **不再重复交付**）。
    pub fn clear(&mut self, line: u16) {
        self.pending.retain(|p| p.line != line);
        self.raised.remove(&line);
        if self.active_irq == line {
            self.active_irq = NO_ACTIVE_IRQ;
        }
        if self.pending_delivery.map(|d| d.line) == Some(line) {
            self.pending_delivery = None;
        }
    }

    // ------------------------------------------------------------------
    // 量子边界采样与仲裁
    // ------------------------------------------------------------------

    /// 量子边界的**采样 + 仲裁**。返回**本次登记的交付**（`None` = 本轮
    /// 不交付）。
    ///
    /// 顺序（冻结规格 §3.4，每一步都必须可单独判别）：
    ///
    /// 1. **刷新使能**：`enabled_mask` 的 bit L := `mie` 的 bit
    ///    `mie_bit_for_line(L)`。**没有** `mie` 位映射的线**保持原值**
    ///    —— 清零 = 静默关中断。
    /// 2. **定时器轮询**：`mtime_ps := now_ps`；`mtime_ps >= mtimecmp_ps`
    ///    且 `mtimecmp_ps != 0` ⇒ 按**电平**语义挂起 timer-compare
    ///    （同线去重 ⇒ 不会堆积）。
    /// 3. **单级嵌套防护**：`active_irq != NO_ACTIVE_IRQ` ⇒ 直接返回
    ///    `None`（不覆盖已交付的中断）。
    /// 4. **全局使能**：`mstatus.MIE` 为 0 ⇒ 返回 `None`（**挂起不丢**）。
    /// 5. **候选筛选 + 仲裁**：挂起 ∧ `enabled_mask` 的 bit L 为 1 的项里，
    ///    取最高优先级；同优先级按 `(request_time_ps, insert_seq)` 升序取
    ///    第一个。
    /// 6. **登记交付**：`pending_delivery = Some(…)`；`active_irq = line`。
    ///
    /// ⚠️ 步骤 1~2 在步骤 3~5 **之前**：所以「全局关」的时候定时器照样
    /// 挂起（挂起不丢），下一次采样时它会被交付。
    ///
    /// ⚠️ 规格 §3.4 把「候选筛选」列在「全局使能」**之前**；实现里两者
    /// **可交换**（筛选没有副作用，两者都只是「不交付」），这里按**实际
    /// 执行序**写。
    pub fn poll(
        &mut self,
        now_ps: u64,
        mie: u32,
        mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        // 记住这次采样的虚拟时间 —— [`InterruptSink::raise`] 没有时间
        // 参数，它用这个缓存值（虚拟时间，不是宿主时钟）。
        self.last_now_ps = now_ps;

        // 1. 刷新使能（量子边界从 CSR `mie` 拉取）。
        self.refresh_enabled_mask(mie);

        // 2. 定时器轮询（电平语义 ⇒ 同线去重 ⇒ 不堆积）。
        self.mtime_ps = now_ps;
        if self.mtimecmp_ps != 0 && self.mtime_ps >= self.mtimecmp_ps {
            self.request(IRQ_TIMER_COMPARE, now_ps, SOURCE_TIMER, TRIGGER_LEVEL)?;
        }

        // 3. 单级嵌套防护（LLD 评审 16-6）。
        if self.active_irq != NO_ACTIVE_IRQ {
            return Ok(None);
        }

        // 4. 全局使能（LLD 评审 16-10 的两层判定之全局层）。
        if (mstatus >> MSTATUS_MIE_BIT) & 1 == 0 {
            return Ok(None);
        }

        // 5. 候选筛选 + 仲裁。
        let Some(best) = self.arbitrate() else {
            return Ok(None);
        };

        // 6. 登记交付（LLD 评审 16-4：交接结构，M-13 拉取）。
        let line = best.line;
        //
        // ⚠️ **`mcause` 报的是架构 cause 码，不是内部线号**（v0.23 批）。
        //
        // 平台的内部线号是 `1 / 2 / 3`，而 RISC-V 的机器中断 cause 码是
        // `3`（软件）/ `7`（定时器）/ `11`（外部）⇒ 交付给 guest 的
        // `mcause` 必须是 `0x8000_0007`（定时器）/ `0x8000_000B`（外部）。
        //
        // 报内部线号是**真 bug**：真实 M 模式 RTOS 的中断分发完全按架构
        // 码写 ⇒ 报线号的话**一个都匹配不上**。
        //
        // ⚠️ **没有架构 cause 码的线不交付** —— 交付一条 `mcause` 不是
        // 架构码的中断，等于把上面那个缺陷换个形式再犯一次。这种线
        // **保持挂起**（与「未使能不丢」同一条不变式）。
        let Some(cause) = arch_mcause_for_line(line) else {
            return Ok(None);
        };
        let intr = PendingIntr { line, cause };
        self.pending_delivery = Some(intr);
        self.active_irq = line;
        Ok(Some(intr))
    }

    /// 在「挂起 ∧ 使能」的集合内选出要交付的那一条。
    ///
    /// 最高优先级优先；同优先级按 `(request_time_ps, insert_seq)` 升序取
    /// **最早**的（确定性）。
    fn arbitrate(&self) -> Option<&PendingIrq> {
        let mut best: Option<&PendingIrq> = None;
        for candidate in &self.pending {
            if !self.is_line_enabled(candidate.line) {
                // 未使能：跳过，但**保持挂起**（LLD 评审 16-1）。
                continue;
            }
            let better = match best {
                None => true,
                Some(current) => {
                    candidate.prio > current.prio
                        || (candidate.prio == current.prio && candidate.key() < current.key())
                }
            };
            if better {
                best = Some(candidate);
            }
        }
        best
    }

    /// 量子边界的**使能刷新**：bit L := `mie` 的 bit `mie_bit_for_line(L)`。
    ///
    /// ⚠️ **没有** `mie` 位映射的线**保持原值** —— 静默清零 = 静默关
    /// 中断（那是「没人注意到它被关掉」的那种坏）。
    fn refresh_enabled_mask(&mut self, mie: u32) {
        for line in 0..=MAX_MASKABLE_LINE {
            let Some(bit) = mie_bit_for_line(line) else {
                // 没有映射 ⇒ 保持原值。
                continue;
            };
            let mask_bit = 1u32 << line;
            if (mie >> bit) & 1 == 1 {
                self.enabled_mask |= mask_bit;
            } else {
                self.enabled_mask &= !mask_bit;
            }
        }
    }

    /// 某条线当前**是否使能**。
    fn is_line_enabled(&self, line: u16) -> bool {
        line <= MAX_MASKABLE_LINE && (self.enabled_mask >> line) & 1 == 1
    }

    // ------------------------------------------------------------------
    // 拉取式交付（LLD 评审 16-4）
    // ------------------------------------------------------------------

    /// 已登记但尚未被取走的交付。
    pub fn pending_intr(&self) -> Option<PendingIntr> {
        self.pending_delivery
    }

    /// 取走交付（**取走即消失**）。
    ///
    /// ⚠️ 取走**不**复位 `active_irq` —— 已交付的中断要等 handler 跑完
    /// 由 guest 经 `irq_clear` 清掉（单级语义，LLD 评审 16-6）。
    pub fn take_intr(&mut self) -> Option<PendingIntr> {
        self.pending_delivery.take()
    }

    // ------------------------------------------------------------------
    // 配置
    // ------------------------------------------------------------------

    /// 设置某条线的优先级（**数值越大越优先**）。
    pub fn set_priority(&mut self, line: u16, prio: u8) {
        self.priorities.insert(line, prio);
    }

    /// 某条线的优先级：优先级表里的值，没有就用**平台默认值**。
    pub fn priority_of(&self, line: u16) -> u8 {
        self.priorities
            .get(&line)
            .copied()
            .unwrap_or_else(|| default_priority(line))
    }

    /// 直接设置使能掩码（**按 line 索引**：bit L = line L）。
    ///
    /// ⚠️ 量子边界的**刷新**会覆盖有 `mie` 位映射的那些位 —— 那是
    /// 「使能的唯一真相是 CSR `mie`」（冻结规格 §1.2 裁决 D）。
    pub fn set_enabled_mask(&mut self, mask: u32) {
        self.enabled_mask = mask;
    }

    /// 当前使能掩码。
    pub fn enabled_mask(&self) -> u32 {
        self.enabled_mask
    }

    /// 设置定时器比较值（`mtime >= mtimecmp` ⇒ 挂起 timer-compare）。
    ///
    /// ⚠️ `0` 表示**定时器关闭** —— 否则 `mtime >= 0` 恒成立，定时器会
    /// 变成一条永不消失的中断。
    pub fn set_timer_compare(&mut self, mtimecmp_ps: u64) {
        self.mtimecmp_ps = mtimecmp_ps;
    }

    /// 定时器比较值。
    pub fn timer_compare(&self) -> u64 {
        self.mtimecmp_ps
    }

    /// 定时器计数（虚拟时间等价值）。
    pub fn mtime(&self) -> u64 {
        self.mtime_ps
    }

    // ------------------------------------------------------------------
    // 观测
    // ------------------------------------------------------------------

    /// 挂起队列（**规范序**：`(request_time_ps, insert_seq)` 升序）。
    pub fn pending_lines(&self) -> Vec<u16> {
        self.pending.iter().map(|p| p.line).collect()
    }

    /// 当前已交付未清除的中断线（[`NO_ACTIVE_IRQ`] = 无）。
    pub fn active_irq(&self) -> u16 {
        self.active_irq
    }

    /// 已断言的线（**升序**）。
    pub fn raised_lines(&self) -> Vec<u16> {
        self.raised.iter().copied().collect()
    }

    // ------------------------------------------------------------------
    // 中断脉冲回调（D-26）
    // ------------------------------------------------------------------

    /// 安装「中断脉冲」回调：`FnMut(now_ps, line)`。宿主把它接到 M-18 的
    /// `sample_irq_pulse`。
    ///
    /// ⚠️ 回调只在**真的登记**了一条挂起时触发（同线去重折叠掉的重复
    /// 请求**不发**脉冲）。
    pub fn set_pulse_sink(&mut self, sink: Box<dyn FnMut(u64, u16)>) {
        self.pulse_sink = Some(sink);
    }

    // ------------------------------------------------------------------
    // 会话状态（契约载荷段 5）
    // ------------------------------------------------------------------

    /// 导出会话载荷**段 5**（[`M16IntcDto`]）。
    ///
    /// ⚠️ 导出的形状满足契约层校验的第 11~14 条：队列里**同线不重复**、
    /// 按 `(request_time_ps, insert_seq)` **严格升序**；`active_irq` 要么
    /// 是「无」要么**在队列里**；队列里的每条线都**已被断言**
    /// （`pending ⊆ raised`）。
    pub fn export_state(&self) -> M16IntcDto {
        M16IntcDto {
            raised: self.raised.iter().copied().collect(),
            pending: self
                .pending
                .iter()
                .map(|p| PendingIrqDto {
                    line: p.line,
                    prio: p.prio,
                    request_time_ps: p.request_time_ps,
                    insert_seq: p.insert_seq,
                    trigger_type: p.trigger_type,
                    source_type: p.source_type.clone(),
                })
                .collect(),
            priorities: self
                .priorities
                .iter()
                .map(|(line, prio)| IrqPriorityDto {
                    line: *line,
                    prio: *prio,
                })
                .collect(),
            enabled_mask: self.enabled_mask,
            active_irq: self.active_irq,
            mtime_ps: self.mtime_ps,
            mtimecmp_ps: self.mtimecmp_ps,
        }
    }

    /// 回灌会话载荷**段 5** —— **整体替换**语义。
    ///
    /// - `pending` **权威地**决定挂起队列；
    /// - `raised` **权威地**决定已断言的线集合；
    /// - `priorities` / `enabled_mask` / `active_irq` / `mtime_ps` /
    ///   `mtimecmp_ps` 逐字段替换。
    ///
    /// ⚠️ **旧形状载荷**（只有 `raised`）**必须**能吃下：那时真的**没有**
    /// 队列（`pending` 空），所以回灌**不**凭空造出队列条目 —— 只还原
    /// 已断言的线（冻结规格 §2.3 裁决 F）。
    ///
    /// ⚠️ 载荷**可能**不是规范序（校验在契约层，回灌在这里）—— 回灌把
    /// 队列整理成规范序，并按 line **去重**（保留最早的那条），这样控制器
    /// 自己的「同线去重」不变式不会因为一份畸形载荷而破掉。
    pub fn load_state(&mut self, state: &M16IntcDto) {
        let mut pending: Vec<PendingIrq> = Vec::with_capacity(state.pending.len());
        for entry in &state.pending {
            if pending.iter().any(|p| p.line == entry.line) {
                // 同线去重：保留最早的那条。
                continue;
            }
            pending.push(PendingIrq {
                line: entry.line,
                prio: entry.prio,
                request_time_ps: entry.request_time_ps,
                insert_seq: entry.insert_seq,
                trigger_type: entry.trigger_type,
                source_type: entry.source_type.clone(),
            });
        }
        pending.sort_by_key(|p| p.key());
        self.pending = pending;
        // 插入序计数器必须**越过**载荷里已有的每一条 —— 否则回灌后的
        // 新请求会与载荷里的条目撞序号。
        self.insert_seq = self.pending.iter().map(|p| p.insert_seq).max().unwrap_or(0);

        self.priorities = state.priorities.iter().map(|p| (p.line, p.prio)).collect();
        self.enabled_mask = state.enabled_mask;
        self.active_irq = state.active_irq;
        self.raised = state.raised.iter().copied().collect();
        self.mtime_ps = state.mtime_ps;
        self.mtimecmp_ps = state.mtimecmp_ps;
        // 交付交接结构是**瞬态**的，载荷里没有它 ⇒ 回灌后没有待交付的
        // 中断（`active_irq` 仍然还原 ⇒ 单级嵌套防护照旧生效）。
        self.pending_delivery = None;
    }
}

impl InterruptPort for InterruptController {
    fn pending_intr(&self) -> Option<PendingIntr> {
        InterruptController::pending_intr(self)
    }

    fn take_intr(&mut self) -> Option<PendingIntr> {
        InterruptController::take_intr(self)
    }

    fn clear(&mut self, line: u16) {
        InterruptController::clear(self, line);
    }

    fn poll(
        &mut self,
        now_ps: u64,
        mie: u32,
        mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        InterruptController::poll(self, now_ps, mie, mstatus)
    }

    fn pending_lines(&self) -> Option<Vec<u16>> {
        // ⚠️ 控制器**自己**的挂起队列随时都读得到 ⇒ 永远是 `Some`
        // （「读不到」只可能发生在**共享适配器**那一层）。
        Some(InterruptController::pending_lines(self))
    }
}

impl InterruptSink for InterruptController {
    /// 拉起一条 IRQ 线 —— 语义是**登记一条挂起**（= [`Self::request`]），
    /// 不是「只置线电平」。
    ///
    /// ⚠️ 契约层的这个端口**只有** `irq: u16` 一个参数 ⇒ 请求时刻用
    /// **最近一次 `poll` 收到的虚拟时间**（控制器内部的 `last_now_ps` 缓存）；
    /// 一次 `poll` 都还没有过时是 `0`。**不是**宿主时钟 —— 确定性要求
    /// 排序键里没有宿主时间。
    ///
    /// ⚠️ 请求源是 `"platform"`、触发模式是**边沿**（固件侧的外设中断源
    /// 今天还不存在 —— 台账 D-25）。
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError> {
        let now_ps = self.last_now_ps;
        self.request(irq, now_ps, SOURCE_PLATFORM, TRIGGER_EDGE)?;
        Ok(())
    }

    /// 撤下一条 IRQ 线 —— 语义是 [`Self::clear`]（移除挂起项 + 撤销线
    /// 电平 + 复位 `active_irq`）。
    fn lower(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.clear(irq);
        Ok(())
    }
}

/// Family used by errors raised from this module.
pub const INT_FAMILY: ErrorFamily = ErrorFamily::Interrupt;

#[cfg(test)]
mod tests;
