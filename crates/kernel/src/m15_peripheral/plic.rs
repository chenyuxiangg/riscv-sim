//! PLIC — Platform-Level Interrupt Controller（QEMU virt 布局，v0.23 批 Lane C）。
//!
//! # 寄存器布局（QEMU virt）
//!
//! | 区域 | 偏移 | 说明 |
//! | --- | --- | --- |
//! | Priority | `0x0` + 4×source | 每个中断源的优先级（写；可读回） |
//! | Pending | `0x1000` + 4×(source/32) | 挂起位图（读） |
//! | Enable | `0x2000` + 0x80×ctx + 4×(source/32) | 中断使能位图（写；可读回） |
//! | Priority Threshold | `0x200000` + 0x1000×ctx | 门限（写；可读回） |
//! | Claim / Complete | `0x200004` + 0x1000×ctx | **读** = guest claim（选最高优先级源、**清 pending**、压 active 栈）；**写** = complete（弹 active 栈） |
//!
//! 基址 `0x0C000000`（QEMU virt 的 `VIRT_PLIC_BASE`）。
//!
//! # 三条必须做对的语义（Renode / RISC-V PLIC 规范）
//!
//! 1. **claim 读有副作用**：原子选出该 context 最高优先级的待交付源、
//!    **清掉它的 pending**、压入该 context 的 active 栈；没有可交付源
//!    时读回 `0`。claim 的判据是「优先级严格大于 threshold」。
//! 2. **complete 写只弹 active 栈**：源仍为高电平（网关未 retract）且
//!    仍使能时才**重新置 pending**；源已落下则只移除，不再挂起。
//! 3. **complete 必须容忍非法 source id**。osz 的 `interrupt_handler` 在
//!    处理**定时器**中断时也会执行 `PLIC_NOTIFY_COMPLETE(base, ctx,
//!    source_id - 16)`，而定时器的 `source_id` 是 7 ⇒ `7 - 16 = -9` ⇒ 以
//!    无符号写进 Claim/Complete 寄存器的是 `0xFFFF_FFF7`。⇒ 越界的 id
//!    **忽略即可**：不 panic、不越界、不误清别人的 pending。
//!
//! # osz 的用法
//!
//! - hart0 的 machine context = `0`（osz `plic.h`：`PLIC_HART0_MACHINE`）。
//! - osz 的 UART 中断源 = **10**（`UART_INT_NUM = 0x1A = 26`，减掉
//!   `LOCAL_INTERRUPT_MAX_NUM = 16` 得 10）。
//! - osz 启动时立刻写 `Priority Threshold @ context 0` 与 `Enable` 位图
//!   ⇒ 这两处写**必须真的落地**（本模型两条都实现了，且可读回）。
//!
//! # 边界
//!
//! PLIC **只暴露状态与寄存器语义**，不直接调用 M-16（M-15 的外设一律
//! 「暴露状态、由宿主驱动」，同 `gpio.rs` / `timer.rs`）。宿主用**只读**
//! 的 [`Plic::deliverable_source`] 判断要不要抬 M-16 外部线；外设抬线用
//! [`Plic::set_source_level`]。**claim 是 guest 的权利**，宿主不得调用。
//!
//! ⚠️ 本外设**参与会话快照**（v0.24 · D-117）：契约层的
//! `PeripheralSessionState` 加了 `Plic` 变体 ⇒ 本器件实现
//! `session_state()` / `restore_session_state()` /
//! `validate_session_state()` 三个方法。
//!
//! ⚠️ **快照带什么**：每个源的**优先级** · 每个 context 的**使能位图**
//! · 每个 context 的**门限** · **挂起位图** · 每个源的**网关电平** ·
//! 每个 context 的 **active 栈** · `now_ps`。
//!
//! ⚠️ **源 0 是保留源**（QEMU 语义：claim 从 1 开始扫）：它占一个
//! **槽位**是器件本身的事实（优先级表就有那么多个槽）⇒ 快照**不凭空
//! 多造**一个源，也**不少带**一个槽位 ⇒ 优先级表的**长度**必须**恰好**
//! 等于器件的源数。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::{Peripheral, ProbeSink};
use riscv_sim_contract::session::{PeripheralSessionState, PlicStateDto};
use std::cell::RefCell;

use super::reg::{extract_subword, register_offset};
use super::ProbeSlot;

/// QEMU virt 的 PLIC 基址（`VIRT_PLIC_BASE`）。
pub const PLIC_BASE: u32 = 0x0C00_0000;

/// PLIC 的寄存器窗口大小。
///
/// ⚠️ 必须容得下最后一个 context 的 Claim 寄存器：
/// `0x200000 + 0x1000 × (context 数 - 1) + 4`。16 个 context ⇒ 到
/// `0x20F008` ⇒ 窗口取 `0x210000`。
pub const PLIC_WINDOW: u32 = 0x21_0000;

/// 默认的中断源数量。
///
/// ⚠️ QEMU virt 的老版本是 32 个源；osz 的 UART 源是 10，而 osz 启动时
/// 用的**绝对**中断号是 26（`UART_INT_NUM`）⇒ 取 64，两种口径都在
/// 范围内。**源数是构造参数**，不写死。
pub const DEFAULT_SOURCE_COUNT: u32 = 64;

/// 默认的 context 数量。
///
/// QEMU virt 是「每 hart 两个 context（M 模式 + S 模式）× 最多 8 个
/// hart」= 16。osz 只用 hart0 的 machine context（= 0）。
pub const DEFAULT_CONTEXT_COUNT: u32 = 16;

/// 探针信号名：某个源被挂起（值 = 源 id）。
pub const SIGNAL_PENDING: &str = "PENDING";

/// 寄存器偏移（**相对基址**）。
pub mod reg {
    /// Pending 区基址：`0x1000` + 4×(source/32)。
    pub const PENDING: u32 = 0x1000;
    /// Enable 区基址：`0x2000` + 0x80×context + 4×(source/32)。
    pub const ENABLE: u32 = 0x2000;
    /// 每个 context 的 Enable 区长度（QEMU virt 固定 0x80）。
    pub const ENABLE_STRIDE: u32 = 0x80;
    /// Priority Threshold 区基址：`0x200000` + 0x1000×context。
    pub const THRESHOLD: u32 = 0x200_000;
    /// 每个 context 的门限 / Claim 区间距。
    pub const CONTEXT_STRIDE: u32 = 0x1000;
    /// Claim / Complete 相对门限区的偏移（`0x200004`）。
    pub const CLAIM: u32 = 4;
}

/// 「这个偏移没有对应的寄存器」——平台的**外设错误族**。
///
/// ⚠️ 族 / 场景码与 `gpio.rs` / `uart.rs` / `timer.rs` 完全一致
/// （`ErrorFamily::Peripheral` + 场景码 `2`）：同一个含义不能有两个码。
fn undefined_offset(off: u32) -> PlatformError {
    PlatformError::new(
        ErrorFamily::Peripheral,
        2,
        format!("PLIC offset 0x{:x} not defined", off),
    )
}

/// 解码后的寄存器位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reg {
    /// Priority[source]
    Priority(u32),
    /// Pending 位图的第 `word` 个 32 位字
    Pending(u32),
    /// Enable 位图（context, word）
    Enable(u32, u32),
    /// Priority Threshold[context]
    Threshold(u32),
    /// Claim / Complete[context]
    Claim(u32),
}

/// PLIC（平台级中断控制器）——QEMU virt 布局。
#[derive(Debug)]
pub struct Plic {
    base: u32,
    id: String,
    /// 中断源数量（Priority / Pending / Enable 三区的源 id 上界）。
    source_count: u32,
    /// context 数量（Enable 区与门限 / Claim 区的 context id 上界）。
    context_count: u32,
    /// 每个源的优先级（QEMU 里是 1 字节；本模型同）。
    priority: Vec<u8>,
    /// 挂起位图（每 32 个源一个字）。
    ///
    /// ⚠️ **IP 位**：claim 读会把它清掉，complete 只在源仍有效时重新置上。
    /// `RefCell` 是因为 [`Peripheral::read`] 是 `&self`，而读 Claim 寄存器
    /// **有副作用**（必须消费掉这个源）。
    pending: RefCell<Vec<u32>>,
    /// 每个源的**网关输入电平**（Renode `IrqSource.State`）。
    ///
    /// ⚠️ 它与 `pending` 是**两件事**：源落下不会撤销已经送进 PLIC 的
    /// 请求（Renode / PLIC 规范：网关不能 retract）；claim 也不改变它。
    asserted: RefCell<Vec<bool>>,
    /// 每个 context 的 **active 栈**（Renode `IrqContext.activeInterrupts`）。
    active: RefCell<Vec<Vec<u32>>>,
    /// 使能位图（每 context 一份，每 32 个源一个字）。
    enable: Vec<Vec<u32>>,
    /// 每个 context 的优先级门限。
    threshold: Vec<u8>,
    /// 最近一次观察到的虚拟时间（ps）。
    now_ps: u64,
    /// M-18 探针回调（D-12）。空 = 没有观察者。
    probe: RefCell<ProbeSlot>,
}

impl Plic {
    /// 默认规模（64 源 / 16 context）的 PLIC。
    pub fn new(id: impl Into<String>, base: u32) -> Self {
        Self::with_capacity(id, base, DEFAULT_SOURCE_COUNT, DEFAULT_CONTEXT_COUNT)
    }

    /// 指定中断源数量与 context 数量的 PLIC。
    pub fn with_capacity(
        id: impl Into<String>,
        base: u32,
        source_count: u32,
        context_count: u32,
    ) -> Self {
        let source_count = source_count.max(1);
        let context_count = context_count.max(1);
        let words = source_count.div_ceil(32);
        Self {
            base,
            id: id.into(),
            source_count,
            context_count,
            priority: vec![0; source_count as usize],
            pending: RefCell::new(vec![0; words as usize]),
            asserted: RefCell::new(vec![false; source_count as usize]),
            active: RefCell::new(vec![Vec::new(); context_count as usize]),
            enable: vec![vec![0; words as usize]; context_count as usize],
            threshold: vec![0; context_count as usize],
            now_ps: 0,
            probe: RefCell::new(ProbeSlot::default()),
        }
    }

    // -----------------------------------------------------------------------
    // 宿主侧接口
    // -----------------------------------------------------------------------

    /// 中断源数量。
    pub fn source_count(&self) -> u32 {
        self.source_count
    }

    /// context 数量。
    pub fn context_count(&self) -> u32 {
        self.context_count
    }

    /// 设置某个源的优先级。
    ///
    /// ⚠️ 越界的源 id **被忽略**（不入状态、不 panic）——与
    /// [`Plic::complete`] 对非法 id 的口径一致。
    pub fn set_priority(&mut self, source: u32, priority: u8) {
        if let Some(slot) = self.priority.get_mut(source as usize) {
            *slot = priority;
        }
    }

    /// 读回某个源的优先级。源 id 越界 → `None`。
    pub fn priority(&self, source: u32) -> Option<u8> {
        self.priority.get(source as usize).copied()
    }

    /// 设置某个 context 下某个源的使能位。
    ///
    /// ⚠️ 越界的 context / 源 id **被忽略**（口径同 [`Plic::set_priority`]）。
    pub fn enable(&mut self, context: u32, source: u32, on: bool) {
        if source >= self.source_count || context >= self.context_count {
            return;
        }
        let word = (source / 32) as usize;
        let bit = 1u32 << (source % 32);
        let slot = match self.enable.get_mut(context as usize) {
            Some(ctx) => match ctx.get_mut(word) {
                Some(w) => w,
                None => return,
            },
            None => return,
        };
        if on {
            *slot |= bit;
        } else {
            *slot &= !bit;
        }
    }

    /// 读回某个 context 下某个源的使能位。越界 → `None`。
    pub fn enabled(&self, context: u32, source: u32) -> Option<bool> {
        let word = (source / 32) as usize;
        let bit = 1u32 << (source % 32);
        let ctx = self.enable.get(context as usize)?;
        let w = ctx.get(word)?;
        Some(w & bit != 0)
    }

    /// 设置某个 context 的优先级门限。
    ///
    /// ⚠️ 越界的 context **被忽略**（口径同 [`Plic::set_priority`]）。
    pub fn set_threshold(&mut self, context: u32, threshold: u8) {
        if let Some(slot) = self.threshold.get_mut(context as usize) {
            *slot = threshold;
        }
    }

    /// 读回某个 context 的门限。context 越界 → `None`。
    pub fn threshold(&self, context: u32) -> Option<u8> {
        self.threshold.get(context as usize).copied()
    }

    /// 外设抬线：把某个源的**网关输入**置为有效（宿主调用）。
    ///
    /// ⚠️ 越界的源 id 被忽略。
    pub fn request(&mut self, source: u32) {
        self.set_source_level(source, true);
    }

    /// 设置某个源的**网关输入电平**（宿主每拍同步设备线）。
    ///
    /// Renode 口径：
    /// - `false → true`：网关把请求送进 PLIC，置 pending；
    /// - `true → false`：**不撤销**已经送进 PLIC 的请求（网关不能
    ///   retract）⇒ pending 留到 guest claim；
    /// - 已 active 的源再次为高：不重新置 pending（等 complete）。
    ///
    /// ⚠️ 越界的源 id 被忽略。
    pub fn set_pending(&mut self, source: u32, level: bool) {
        self.set_source_level(source, level);
    }

    /// 设置网关输入电平（`set_pending` 的实名版本）。
    pub fn set_source_level(&mut self, source: u32, level: bool) {
        if source >= self.source_count {
            return;
        }
        let was = self.asserted.borrow()[source as usize];
        if was == level {
            return;
        }
        self.asserted.borrow_mut()[source as usize] = level;
        if level {
            self.set_ip(source, true);
        }
        // ⚠️ **落下不撤销 pending**：Renode 的 `OnGPIO(value=false)` 只改
        // `IrqSource.State`，不动 pending。请求一旦进 PLIC 就不能 retract。
    }

    /// 直接清掉某个源的 pending（只给测试 / 内部恢复用；guest 不能写
    /// Pending 区）。
    pub fn clear_pending(&mut self, source: u32) {
        self.set_ip(source, false);
    }

    /// 某个源是否挂起。越界的源 id → `false`。
    pub fn pending(&self, source: u32) -> bool {
        let word = (source / 32) as usize;
        let bit = 1u32 << (source % 32);
        self.pending
            .borrow()
            .get(word)
            .is_some_and(|w| w & bit != 0)
    }

    /// **当前应该交付哪个源**（宿主拿它去 M-16 抬线；**只读、不消费**）。
    ///
    /// 判据（Renode / RISC-V PLIC）：
    /// - 该源**已挂起**；
    /// - 该源在**这个 context 下已使能**；
    /// - 该源的优先级**严格大于**这个 context 的门限。
    ///
    /// 多个源同时满足时取**优先级最高**的；优先级相同时取**编号最小**的。
    ///
    /// ⚠️ **source 0 是保留源** ⇒ 永远不会被交付。
    ///
    /// ⚠️ 宿主**只能**用这个只读接口抬线；[`Plic::claim`] 是 guest 的
    /// 领取动作，会消费 pending。
    pub fn deliverable_source(&self, context: u32) -> Option<u32> {
        let threshold = *self.threshold.get(context as usize)?;
        let pending = self.pending.borrow();
        let mut best: Option<(u8, u32)> = None;
        for source in 1..self.source_count {
            let priority = self.priority[source as usize];
            if priority <= threshold {
                continue;
            }
            let word = (source / 32) as usize;
            let bit = 1u32 << (source % 32);
            if !pending.get(word).is_some_and(|w| w & bit != 0) {
                continue;
            }
            let enabled = self
                .enable
                .get(context as usize)
                .and_then(|ctx| ctx.get(word))
                .is_some_and(|w| w & bit != 0);
            if !enabled {
                continue;
            }
            match best {
                Some((best_priority, _)) if priority <= best_priority => {}
                _ => best = Some((priority, source)),
            }
        }
        best.map(|(_, source)| source)
    }

    /// **guest claim**：返回源并消费它的 pending，压入该 context 的
    /// active 栈。同一个源在 complete 之前不会再次被 claim。
    ///
    /// ⚠️ 这是**有副作用**的动作；宿主抬线只能读
    /// [`Plic::deliverable_source`]。
    pub fn claim(&mut self, context: u32) -> Option<u32> {
        self.claim_at(context)
    }

    /// **guest complete**：只弹 active 栈；pending 在 claim 时已经清掉。
    ///
    /// - 源仍为高电平（`asserted`）且仍使能 ⇒ 网关重新置 pending；
    /// - 源已落下 / 已禁用 ⇒ 不重新置 pending。
    ///
    /// ⚠️ **非法 source id 必须被忽略**：osz 在处理定时器中断时会把
    /// `source_id - 16 = -9`（无符号 `0xFFFF_FFF7`）写进来 ⇒ 越界的 id
    /// 直接返回：不 panic、不越界、**不误清别人的 pending**。
    ///
    /// ⚠️ 只有 **active 栈顶** 的源能被 complete；不匹配的写被忽略。
    pub fn complete(&mut self, context: u32, source: u32) {
        if source == 0 || source >= self.source_count || context >= self.context_count {
            return;
        }
        {
            let mut active = self.active.borrow_mut();
            let Some(stack) = active.get_mut(context as usize) else {
                return;
            };
            if stack.last().copied() != Some(source) {
                return;
            }
            stack.pop();
        }
        let asserted = self.asserted.borrow()[source as usize];
        let enabled = self.enabled(context, source).unwrap_or(false);
        if asserted {
            // Renode 的 `MarkSourceAsPending` 只在使能时才置 pending；
            // 已 assert 但被禁用 ⇒ 保持现状（不额外清）。
            if enabled {
                self.set_ip(source, true);
            }
        } else {
            // 网关线已落下：complete 清掉这个源的 pending。
            self.set_ip(source, false);
        }
    }

    /// 上报一次探针观测（M-18 管道）。
    fn report(&self, signal: &str, value: u64) {
        let now = self.now_ps;
        self.probe.borrow_mut().report(signal, value, now);
    }

    /// 直接改 IP 位（`&self`：guest 读 Claim 的路径也要用）。
    fn set_ip(&self, source: u32, pending: bool) {
        if source >= self.source_count {
            return;
        }
        let word = (source / 32) as usize;
        let bit = 1u32 << (source % 32);
        let prev = {
            let mut words = self.pending.borrow_mut();
            let Some(slot) = words.get_mut(word) else {
                return;
            };
            let prev = *slot & bit != 0;
            if pending {
                *slot |= bit;
            } else {
                *slot &= !bit;
            }
            prev
        };
        if prev != pending {
            self.report(SIGNAL_PENDING, u64::from(source));
        }
    }

    /// claim 的 `&self` 实现（寄存器读路径用）。
    fn claim_at(&self, context: u32) -> Option<u32> {
        let source = self.deliverable_source(context)?;
        self.set_ip(source, false);
        let mut active = self.active.borrow_mut();
        let stack = active.get_mut(context as usize)?;
        stack.push(source);
        Some(source)
    }

    /// The PLIC's **own** integrity rules for a session state.
    ///
    /// ⚠️ **数量必须与器件能力相符** ——
    ///
    /// - 优先级表的**长度** = 器件的**源数**（下标即源号 ⇒ 多一条就是
    ///   「凭空多出一个源」，少一条就是「某个源没被描述」）；
    /// - 门限 / 使能位图的**长度** = 器件的 **context 数**；
    /// - 位图的**字数** = 器件的位图字数（每 32 个源一个字）。
    ///
    /// ⚠️ 这条规则**只能**住在器件里：契约层知道「两个列表长度必须
    /// 一致」（**自洽**），但它**不知道**这个 PLIC 有多少个源 / context。
    fn check_state(&self, state: &PlicStateDto) -> Result<(), PlatformError> {
        let sources = self.source_count as usize;
        let contexts = self.context_count as usize;
        let words = self.pending_words() as usize;

        if state.priorities.len() != sources {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个源的优先级，器件只有 {} 个源",
                    self.id,
                    state.priorities.len(),
                    self.source_count
                ),
            ));
        }
        if state.thresholds.len() != contexts {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个 context 的门限，器件只有 {} 个 context",
                    self.id,
                    state.thresholds.len(),
                    self.context_count
                ),
            ));
        }
        if state.enables.len() != contexts {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个 context 的使能位图，器件只有 {} 个 context",
                    self.id,
                    state.enables.len(),
                    self.context_count
                ),
            ));
        }
        if state.pending.len() != words {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个字的挂起位图，器件的位图是 {} 个字",
                    self.id,
                    state.pending.len(),
                    words
                ),
            ));
        }
        if state.asserted.len() != sources {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个源的电平，器件只有 {} 个源",
                    self.id,
                    state.asserted.len(),
                    self.source_count
                ),
            ));
        }
        if state.active.len() != contexts {
            return Err(PlatformError::new(
                super::PERIPHERAL_FAMILY,
                super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                format!(
                    "PLIC '{}' 的会话状态带 {} 个 context 的 active 栈，器件只有 {} 个 context",
                    self.id,
                    state.active.len(),
                    self.context_count
                ),
            ));
        }
        for (ctx, stack) in state.active.iter().enumerate() {
            if let Some(source) = stack.iter().find(|s| **s == 0 || **s >= self.source_count) {
                return Err(PlatformError::new(
                    super::PERIPHERAL_FAMILY,
                    super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                    format!(
                        "PLIC '{}' 的 context {ctx} 的 active 栈里有非法源号 {source}",
                        self.id
                    ),
                ));
            }
        }
        for (ctx, bitmap) in state.enables.iter().enumerate() {
            if bitmap.len() != words {
                return Err(PlatformError::new(
                    super::PERIPHERAL_FAMILY,
                    super::E_PER_04_UNSUPPORTED_SESSION_STATE,
                    format!(
                        "PLIC '{}' 的 context {ctx} 的使能位图有 {} 个字，器件的位图是 {} 个字",
                        self.id,
                        bitmap.len(),
                        words
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// 寄存器读写（`register_offset` / `extract_subword`）在 `super::reg`
/// （M-15 review N1 的 DRY 合并）。
impl Peripheral for Plic {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        PLIC_WINDOW
    }

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let off = register_offset(offset);
        match self.decode(off)? {
            Reg::Priority(source) => {
                // QEMU 存的是 1 字节优先级。
                self.priority[source as usize] = (value & 0xFF) as u8;
                Ok(())
            }
            Reg::Pending(_word) => {
                // Pending 是**只读**的（QEMU 也不接受写）——写它没有效果。
                Ok(())
            }
            Reg::Enable(context, word) => {
                // 整字替换（QEMU 语义）。
                self.enable[context as usize][word as usize] = value;
                Ok(())
            }
            Reg::Threshold(context) => {
                self.threshold[context as usize] = (value & 0xFF) as u8;
                Ok(())
            }
            Reg::Claim(context) => {
                // 写 = 通知完成；⚠️ 非法 source id 必须在 `complete` 里
                // 被忽略（osz 会把 `0xFFFF_FFF7` 写进来）。
                self.complete(context, value);
                Ok(())
            }
        }
    }

    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        // 引擎拥有虚拟时钟（M-12）；`tick` 是它触达外设的路径。
        self.now_ps = now_ps;
        Ok(Vec::new())
    }

    /// 装上 / 摘掉 M-18 探针回调（D-12）。
    fn on_probe(&mut self, cb: Option<ProbeSink>) {
        self.probe.borrow_mut().set(cb);
    }

    /// This PLIC's share of the **session snapshot** (segment 4 —
    /// v0.24 · D-117).
    ///
    /// ⚠️ **源 0 在优先级表里占一个槽位**（下标即源号）—— 它是器件
    /// 自己的表头，不是快照**多造**的源。
    fn session_state(&self) -> Option<PeripheralSessionState> {
        Some(PeripheralSessionState::Plic(PlicStateDto {
            id: self.id.clone(),
            priorities: self.priority.clone(),
            enables: self.enable.clone(),
            thresholds: self.threshold.clone(),
            pending: self.pending.borrow().clone(),
            asserted: self.asserted.borrow().clone(),
            active: self.active.borrow().clone(),
            now_ps: self.now_ps,
        }))
    }

    /// Restore the PLIC's session state (segment 4's consumer).
    ///
    /// ⚠️ **挂起位**一并还原 —— 它们是**器件的状态**（外设抬线留下的
    /// 痕迹）⇒ 不还原 ⇒ 还原出来的 PLIC 少了一堆挂起的中断源。
    fn restore_session_state(
        &mut self,
        state: &PeripheralSessionState,
    ) -> Result<(), PlatformError> {
        let PeripheralSessionState::Plic(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Plic,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.check_state(s)?;
        self.priority = s.priorities.clone();
        self.enable = s.enables.clone();
        self.threshold = s.thresholds.clone();
        *self.pending.borrow_mut() = s.pending.clone();
        *self.asserted.borrow_mut() = s.asserted.clone();
        *self.active.borrow_mut() = s.active.clone();
        self.now_ps = s.now_ps;
        Ok(())
    }

    /// **Read-only** acceptance check for this PLIC's session state.
    ///
    /// ⚠️ **默认实现是「放行」** ⇒ 本器件**必须**覆盖它：否则一个数量
    /// 对不上的载荷会被**静默接受**，还原出来的 PLIC 与采快照的**不是
    /// 同一个**。
    fn validate_session_state(&self, state: &PeripheralSessionState) -> Result<(), PlatformError> {
        let PeripheralSessionState::Plic(s) = state else {
            return Err(super::wrong_state_kind(
                &self.id,
                super::SessionKind::Plic,
                super::SessionKind::of(state),
            ));
        };
        super::check_same_id(&self.id, &s.id)?;
        self.check_state(s)
    }
}

impl Plic {
    /// 把偏移解码成寄存器位置。
    ///
    /// ⚠️ **越界的源 id / context id 一律当作「没有这个寄存器」** ⇒ 报
    /// 平台的错误族（不许静默忽略、不许 panic）。
    fn decode(&self, off: u32) -> Result<Reg, PlatformError> {
        // ---- Priority 区：`0x0` + 4×source ----
        if off < reg::PENDING {
            let source = off / 4;
            if source >= self.source_count {
                return Err(undefined_offset(off));
            }
            return Ok(Reg::Priority(source));
        }

        // ---- Pending 区：`0x1000` + 4×(source/32) ----
        if off < reg::ENABLE {
            let word = (off - reg::PENDING) / 4;
            if word >= self.pending_words() {
                return Err(undefined_offset(off));
            }
            return Ok(Reg::Pending(word));
        }

        // ---- Enable 区：`0x2000` + 0x80×context + 4×(source/32) ----
        if off < reg::THRESHOLD {
            let rel = off - reg::ENABLE;
            let context = rel / reg::ENABLE_STRIDE;
            let word = (rel % reg::ENABLE_STRIDE) / 4;
            if context >= self.context_count || word >= self.pending_words() {
                return Err(undefined_offset(off));
            }
            return Ok(Reg::Enable(context, word));
        }

        // ---- 门限 / Claim 区：`0x200000` + 0x1000×context ----
        let rel = off - reg::THRESHOLD;
        let context = rel / reg::CONTEXT_STRIDE;
        let within = rel % reg::CONTEXT_STRIDE;
        if context >= self.context_count {
            return Err(undefined_offset(off));
        }
        match within {
            0 => Ok(Reg::Threshold(context)),
            reg::CLAIM => Ok(Reg::Claim(context)),
            _ => Err(undefined_offset(off)),
        }
    }

    /// Pending / Enable 位图的字数（每 32 个源一个字）。
    fn pending_words(&self) -> u32 {
        self.source_count.div_ceil(32)
    }

    /// 读一个字（32 位对齐）寄存器。
    fn read_word(&self, off: u32) -> Result<u32, PlatformError> {
        match self.decode(off)? {
            Reg::Priority(source) => Ok(u32::from(self.priority[source as usize])),
            Reg::Pending(word) => Ok(self.pending.borrow()[word as usize]),
            Reg::Enable(context, word) => Ok(self.enable[context as usize][word as usize]),
            Reg::Threshold(context) => Ok(u32::from(self.threshold[context as usize])),
            // 读 Claim = guest 领取：会消费 pending 并压 active 栈。
            Reg::Claim(context) => Ok(self.claim_at(context).unwrap_or(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// 记录探针观测的日志。
    type ProbeLog = Rc<RefCell<Vec<(String, u64, u64)>>>;

    fn recording_probe() -> (ProbeLog, ProbeSink) {
        let log: ProbeLog = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&log);
        (
            log,
            Box::new(move |signal: &str, value: u64, now_ps: u64| {
                sink.borrow_mut().push((signal.to_string(), value, now_ps));
            }),
        )
    }

    /// osz 的 UART 中断源（`UART_INT_NUM 26 - LOCAL_INTERRUPT_MAX_NUM 16`）。
    const UART_SOURCE: u32 = 10;

    /// Priority 写入后可读回。
    #[test]
    fn priority_is_writable_and_readable() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        // osz 的 UART 源 10 ⇒ Priority 偏移 = 0x0 + 4×10 = 0x28。
        plic.write(0x28, WriteSize::Word, 3).unwrap();
        assert_eq!(
            plic.read(0x28, WriteSize::Word).unwrap(),
            3,
            "Priority 必须读回刚写的值"
        );
        assert_eq!(plic.priority(UART_SOURCE), Some(3));

        // 别的源互不干扰。
        plic.write(0x2C, WriteSize::Word, 7).unwrap();
        assert_eq!(plic.read(0x28, WriteSize::Word).unwrap(), 3);
        assert_eq!(plic.read(0x2C, WriteSize::Word).unwrap(), 7);
    }

    /// Enable 位图的 set / clear 语义 —— **含 `source/32` 的跨字边界**。
    #[test]
    fn the_enable_bitmap_handles_the_word_boundary() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        // source 31 ⇒ 字 0 的 bit 31；source 32 ⇒ 字 1 的 bit 0。
        plic.enable(0, 31, true);
        plic.enable(0, 32, true);

        assert!(plic.enabled(0, 31).unwrap());
        assert!(plic.enabled(0, 32).unwrap());

        // 寄存器读回：Enable 区 context 0 的字 0 = 0x2000，字 1 = 0x2004。
        assert_eq!(
            plic.read(0x2000, WriteSize::Word).unwrap(),
            0x8000_0000,
            "字 0 必须是 source 31 的那一位"
        );
        assert_eq!(
            plic.read(0x2004, WriteSize::Word).unwrap(),
            0x0000_0001,
            "字 1 必须是 source 32 的那一位"
        );

        // 清掉 source 31：字 0 归零，字 1 不受影响。
        plic.enable(0, 31, false);
        assert_eq!(plic.read(0x2000, WriteSize::Word).unwrap(), 0x0000_0000);
        assert_eq!(plic.read(0x2004, WriteSize::Word).unwrap(), 0x0000_0001);
        assert!(!plic.enabled(0, 31).unwrap());
        assert!(plic.enabled(0, 32).unwrap());
    }

    /// Enable 位图的**寄存器写**是整字替换（QEMU 语义）。
    #[test]
    fn writing_the_enable_register_replaces_the_whole_word() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.write(0x2000, WriteSize::Word, 0xFFFF_FFFF).unwrap();
        assert!(plic.enabled(0, 0).unwrap());
        assert!(plic.enabled(0, 31).unwrap());
        assert!(!plic.enabled(0, 32).unwrap(), "字 1 不该被字 0 的写影响");

        plic.write(0x2000, WriteSize::Word, 0x0000_0000).unwrap();
        assert!(!plic.enabled(0, 0).unwrap());
        assert!(!plic.enabled(0, 31).unwrap());
    }

    /// Threshold 写入后可读回（osz 启动时立刻写 context 0 的门限）。
    #[test]
    fn threshold_is_writable_and_readable() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.write(0x0020_0000, WriteSize::Word, 2).unwrap();
        assert_eq!(plic.read(0x0020_0000, WriteSize::Word).unwrap(), 2);
        assert_eq!(plic.threshold(0), Some(2));

        // context 1 的门限在 `0x200000 + 0x1000`。
        plic.write(0x0020_1000, WriteSize::Word, 5).unwrap();
        assert_eq!(plic.read(0x0020_1000, WriteSize::Word).unwrap(), 5);
        assert_eq!(plic.threshold(1), Some(5));
        assert_eq!(plic.threshold(0), Some(2), "两个 context 互不干扰");
    }

    /// claim 返回**优先级最高且严格大于门限**的源。
    #[test]
    fn claim_returns_the_highest_priority_source_above_the_threshold() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 1);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);
        assert_eq!(plic.claim(0), Some(UART_SOURCE));

        // 另一个优先级更高的源抢到。
        plic.enable(0, 11, true);
        plic.set_priority(11, 5);
        plic.request(11);
        assert_eq!(plic.claim(0), Some(11), "优先级更高的源必须胜出");

        // 优先级低的源不参与。
        plic.enable(0, 12, true);
        plic.set_priority(12, 2);
        plic.request(12);
        // 先让 11 完成：它仍为高 ⇒ 重新挂起，才能在 12 面前再次胜出。
        plic.complete(0, 11);
        assert_eq!(plic.claim(0), Some(11));
    }

    /// **优先级等于门限的源不被交付**（QEMU / SiFive 语义是「> 门限」）。
    #[test]
    fn a_source_equal_to_the_threshold_is_not_claimed() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 3);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        assert_eq!(plic.claim(0), None, "优先级等于门限的源不得被交付");
        assert_eq!(
            plic.read(0x0020_0004, WriteSize::Word).unwrap(),
            0,
            "读 Claim 寄存器同样不得返回它"
        );

        // 高一点点就能拿到。
        plic.set_priority(UART_SOURCE, 4);
        assert_eq!(plic.claim(0), Some(UART_SOURCE));
    }

    /// 优先级相同时取**编号最小**的源（QEMU 的扫描顺序）。
    #[test]
    fn a_priority_tie_is_broken_by_the_lowest_source_id() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        for source in [5u32, 9, 17] {
            plic.enable(0, source, true);
            plic.set_priority(source, 4);
            plic.request(source);
        }
        assert_eq!(plic.claim(0), Some(5), "同优先级取编号最小的");
    }

    /// 没使能或没挂起的源不得被交付。
    #[test]
    fn claim_needs_the_source_to_be_enabled_and_pending() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        plic.set_priority(UART_SOURCE, 3);

        // 没使能 ⇒ 不交付。
        plic.request(UART_SOURCE);
        assert_eq!(plic.claim(0), None, "没使能的源不得被交付");

        // 使能了但没挂起 ⇒ 不交付。
        // ⚠️ 先落下电平（网关不 retract），再直接清 IP 造出「没挂起」。
        plic.set_pending(UART_SOURCE, false);
        plic.clear_pending(UART_SOURCE);
        plic.enable(0, UART_SOURCE, true);
        assert_eq!(plic.claim(0), None, "没挂起的源不得被交付");

        // 挂起 ⇒ 交付。
        plic.request(UART_SOURCE);
        assert_eq!(plic.claim(0), Some(UART_SOURCE));
    }

    /// **读 Claim 寄存器会消费掉 pending**（Renode / RISC-V PLIC 规范）：
    /// 同一次 claim 不能把同一个源交付两次。
    #[test]
    fn a_claim_read_consumes_the_pending_bit() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        assert_eq!(
            plic.read(0x0020_0004, WriteSize::Word).unwrap(),
            UART_SOURCE,
            "读 Claim 必须拿到源"
        );
        assert!(
            !plic.pending(UART_SOURCE),
            "claim 必须原子清掉该源的 pending"
        );
        assert_eq!(
            plic.read(0x0020_0004, WriteSize::Word).unwrap(),
            0,
            "同一个源在 complete 之前不得被第二次 claim"
        );
    }

    /// **complete 本身不清 pending**：pending 在 claim 时已经清掉。
    /// 没有 claim 就写 complete 是空操作。
    #[test]
    fn a_complete_without_a_claim_does_not_clear_pending() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        plic.complete(0, UART_SOURCE);
        assert!(
            plic.pending(UART_SOURCE),
            "没有 active 的 complete 不得清掉别人的 pending"
        );
    }

    /// **源仍为高电平时，complete 让网关重新置 pending**（Renode：
    /// `CompleteHandlingInterrupt` 里 `if(irq.State) MarkSourceAsPending`）。
    #[test]
    fn a_complete_repends_a_source_that_is_still_asserted() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        assert_eq!(plic.claim(0), Some(UART_SOURCE));
        assert!(!plic.pending(UART_SOURCE), "claim 先清 pending");

        plic.complete(0, UART_SOURCE);
        assert!(
            plic.pending(UART_SOURCE),
            "电平仍为高 ⇒ complete 后必须重新挂起"
        );
        assert_eq!(
            plic.claim(0),
            Some(UART_SOURCE),
            "重新挂起后必须能再次被交付"
        );
    }

    /// 源在服务期间落下时，complete **不再**重新置 pending。
    #[test]
    fn a_complete_does_not_repend_after_the_level_deasserts() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        assert_eq!(plic.claim(0), Some(UART_SOURCE));
        // 宿主观察到设备线落下（电平源不再有效）。
        plic.set_pending(UART_SOURCE, false);
        plic.complete(0, UART_SOURCE);

        assert!(
            !plic.pending(UART_SOURCE),
            "线已落下 ⇒ complete 后不得重新挂起"
        );
        assert_eq!(plic.claim(0), None);
    }

    /// **非法 source id 的 complete 必须被忽略**：不 panic、不越界、
    /// **不误清别人的 pending**（osz 的定时器中断会写 `0xFFFF_FFF7`）。
    #[test]
    fn complete_ignores_an_invalid_source_id() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        // osz 的 `source_id - 16`（定时器 source 7 ⇒ -9 ⇒ 0xFFFF_FFF7）。
        plic.write(0x0020_0004, WriteSize::Word, 0xFFFF_FFF7)
            .unwrap();
        plic.complete(0, 0xFFFF_FFF7);

        assert!(plic.pending(UART_SOURCE), "非法 id 不得误清别人的 pending");
        assert_eq!(
            plic.read(0x1000, WriteSize::Word).unwrap(),
            0x0000_0400,
            "挂起位图必须原封不动（source 10 ⇒ bit 10）"
        );

        // 越界但**合法形状**的 id（源数 64 ⇒ 64 越界）同样被忽略。
        plic.complete(0, 64);
        assert!(plic.pending(UART_SOURCE));

        // source 0 是保留源，同样被忽略。
        plic.complete(0, 0);
        assert!(plic.pending(UART_SOURCE));
    }

    /// 合法的 complete 只弹 active；源已落下时不再重新挂起。
    #[test]
    fn a_complete_after_claim_clears_when_the_source_deasserted() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);
        assert!(plic.pending(UART_SOURCE));

        assert_eq!(plic.claim(0), Some(UART_SOURCE));
        assert!(!plic.pending(UART_SOURCE), "claim 先消费 pending");
        // 服务期间设备线落下。
        plic.set_pending(UART_SOURCE, false);
        plic.write(0x0020_0004, WriteSize::Word, UART_SOURCE)
            .unwrap();
        assert!(
            !plic.pending(UART_SOURCE),
            "线已落下 ⇒ complete 不得重新挂起"
        );
        assert_eq!(plic.claim(0), None, "清完之后不再有可交付的源");
    }

    /// 挂起位图（Pending 区）随宿主的 request 变化。
    #[test]
    fn the_pending_register_reflects_the_host_request() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        // source 10 ⇒ 字 0 的 bit 10；source 32 ⇒ 字 1 的 bit 0。
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);
        plic.request(32);

        assert_eq!(
            plic.read(0x1000, WriteSize::Word).unwrap(),
            0x0000_0400,
            "Pending 字 0 必须是 source 10 的那一位"
        );
        assert_eq!(
            plic.read(0x1004, WriteSize::Word).unwrap(),
            0x0000_0001,
            "Pending 字 1 必须是 source 32 的那一位"
        );

        // ⚠️ 电平落下**不撤销**已经送进 PLIC 的请求（Renode 网关语义）。
        plic.set_pending(UART_SOURCE, false);
        assert_eq!(
            plic.read(0x1000, WriteSize::Word).unwrap(),
            0x0000_0400,
            "网关不能 retract ⇒ 线落下后 pending 仍在"
        );

        // 只有 claim 才消费它。
        assert_eq!(plic.claim(0), Some(UART_SOURCE));
        assert_eq!(plic.read(0x1000, WriteSize::Word).unwrap(), 0);
    }

    /// 非法 / 未映射的偏移必须报**平台的错误族**。
    #[test]
    fn undefined_offset_reports_the_peripheral_error_family() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        let err = plic
            .read(0x3000, WriteSize::Word)
            .expect_err("未映射的偏移必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
        assert_eq!(err.scenario, 2, "场景码必须与 gpio/uart/timer 一致");

        let err = plic
            .write(0x3000, WriteSize::Word, 0)
            .expect_err("未映射的偏移必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);

        // 门限区里既不是门限也不是 Claim 的偏移 ⇒ 未映射。
        let err = plic
            .read(0x0020_0008, WriteSize::Word)
            .expect_err("门限区里的空洞必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);
    }

    /// 越界的源 id / context id 是**未映射的寄存器** ⇒ 报错，不静默。
    #[test]
    fn an_out_of_range_source_or_context_is_rejected() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        // 源数 64 ⇒ source 64 的 Priority 偏移 0x100 越界。
        let err = plic
            .write(0x100, WriteSize::Word, 1)
            .expect_err("越界的源 id 必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);

        // context 数 16 ⇒ context 16 的门限偏移 0x210000 越界。
        let err = plic
            .read(0x0021_0000, WriteSize::Word)
            .expect_err("越界的 context 必须报错");
        assert_eq!(err.family, ErrorFamily::Peripheral);
    }

    /// 读 Claim 寄存器与 `claim()` 走同一条有副作用的路径。
    #[test]
    fn the_claim_register_and_the_host_api_agree() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.set_threshold(0, 0);
        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);

        assert_eq!(
            plic.read(0x0020_0004, WriteSize::Word).unwrap(),
            UART_SOURCE,
            "读 Claim 寄存器必须给出同一个源"
        );
        assert!(
            !plic.pending(UART_SOURCE),
            "读 Claim 与 claim() 一样消费 pending"
        );
        assert_eq!(plic.claim(0), None, "第二次 claim 必须为空");
        plic.complete(0, UART_SOURCE);
        assert_eq!(
            plic.claim(0),
            Some(UART_SOURCE),
            "complete 后源仍为高 ⇒ 再次可交付"
        );
    }

    /// 不同 context 的门限 / 使能互不干扰（osz 用 context 0）。
    #[test]
    fn contexts_are_independent() {
        let mut plic = Plic::new("PLIC0", PLIC_BASE);

        plic.enable(0, UART_SOURCE, true);
        plic.set_priority(UART_SOURCE, 3);
        plic.request(UART_SOURCE);
        plic.set_threshold(0, 0);
        plic.set_threshold(1, 5);

        assert_eq!(plic.claim(0), Some(UART_SOURCE), "context 0 门限低");
        assert_eq!(plic.claim(1), None, "context 1 门限高 + 没使能 ⇒ 不交付");
    }

    /// 探针管道能看到挂起 / 撤销挂起（D-12 口径）。
    #[test]
    fn the_probe_pipeline_sees_pending_changes() {
        let (log, sink) = recording_probe();
        let mut plic = Plic::new("PLIC0", PLIC_BASE);
        plic.on_probe(Some(sink));
        plic.tick(1_000).unwrap();

        plic.request(UART_SOURCE);
        plic.clear_pending(UART_SOURCE);

        let log = log.borrow();
        assert_eq!(log.len(), 2, "挂起与撤销各上报一次");
        assert_eq!(log[0].0, SIGNAL_PENDING);
        assert_eq!(log[0].1, u64::from(UART_SOURCE), "上报的是源 id");
        assert_eq!(log[0].2, 1_000, "时间戳是外设的时钟");
        assert_eq!(log[1].0, SIGNAL_PENDING);
    }
}
