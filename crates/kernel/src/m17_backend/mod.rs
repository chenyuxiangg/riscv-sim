//! M-17 ExecutionBackend 执行后端 (Execution Backend Abstraction).
//!
//! Implements the `ExecutionBackend` port from `riscv_sim_contract`,
//! wrapping the M-12 engine and M-13 ISS behind a single object
//! that the debug services (M-08, M-09, M-10) talk to.
//!
//! A second implementation (QEMU via GDB RSP) is reserved for P1
//! and will live in the same crate but be feature-gated.
//!
//! ## 状态单一真相
//!
//! 平台状态（寄存器 / PC / 虚拟时钟）住在 **M-12 引擎**里，后端**不持有
//! 第二份**。后端要驱动 ISS 时，向引擎**借**一份可变引用
//! ([`crate::m12_engine::Engine::state_mut`])——借，而不是克隆：克隆会让
//! ISS 推进一份引擎永远看不到的寄存器表。
//!
//! ## 断点 / 观察点的 id 归属
//!
//! 契约的 `SetBreakpoint(Breakpoint)` / `SetWatchpoint(Watchpoint)` **带 id**
//! （宿主分配），而 `ExecutionBackend` 端口的 `set_breakpoint` /
//! `set_watchpoint` **不带 id**。两者不是同一层：
//!
//! - **端口层**（`set_breakpoint(addr, enabled)`）语义是「按地址切换」：
//!   地址上已有断点就改它的使能位，没有就新建（自动分配 id）。
//! - **契约层**（`set_breakpoint_with_id(bp)`）用**宿主给的 id** 落表，
//!   宿主才能在 `ClearBreakpoint{id}` 时按 id 精确删除。
//!
//! 两个入口操作**同一张表**，不存在第二份状态。

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::csr::{
    pending_lines_to_mip_mask, CSR_IRQ_CLEAR, CSR_MIE, CSR_MIP, CSR_MSTATUS,
};
use riscv_sim_contract::error::{
    ErrorFamily, PlatformError, DEBUG_FAMILY, E_DBG_01_INVALID_REGISTER_INDEX,
};
use riscv_sim_contract::model::{Breakpoint, SimState, WatchAccess, Watchpoint, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::HaltReason;
use riscv_sim_contract::snapshot::EngineSnapshotDto;

use crate::m08_breakpoint::{BreakpointTable, WatchpointTable};
use crate::m12_engine::{
    Engine, ExecOutcome, InstructionExecutor, PeripheralTicker, RunReason, TIME_QUANTUM_PS,
};
use crate::m13_iss::csr::{csr_read, csr_write};
use crate::m13_iss::{enter_trap, write_gpr, Iss, StepOutcome};
use crate::m14_bus::{AccessDir, Bus, BusWidth};
use crate::m16_interrupt::{InterruptPort, PendingIntr, MAX_MASKABLE_LINE};

/// 「中断控制器正被占用」的场景码（`INT` 族）。
///
/// ⚠️ **不是新错误码** —— 它是**既有**的码（契约层 §5 的表里列着
/// `E_INT_01_CONTROLLER_BUSY`），本批只是把它的**唯一定义**从宿主搬到了
/// 这里：M-17 的 [`SharedInterruptPort`] 是**第一个**会真的撞上
/// `RefCell` 争用的地方（它在**指令边界**上借控制器，而同一批次里
/// 外设还会经 `InterruptSink::raise` 借**同一个**控制器）。
///
/// 宿主仍然**转出**这个名字（`kernel_host::E_INT_01_CONTROLLER_BUSY`），
/// 所以对外的路径一个字都没变。
pub const E_INT_01_CONTROLLER_BUSY: u16 = 1;

/// 一条指令的虚拟时间成本（皮秒）。
///
/// 第 1 阶段没有真正的流水线模型，每条指令按 **1 ns** 计——与前端的
/// mock 内核同口径，前端时间轴不需要因为换内核而重标定。
///
/// ⚠️ **口径统一（D-07）**：值不再是这里独立写死的 `1_000`，而是转调
/// M-12 的 [`crate::m12_engine::INSTRUCTION_COST_PS`]（= 时间量子 1 µs ÷
/// 每量子指令配额 1000 条 = 1 ns）。「量子 ↔ 指令」的换算只允许有**一处**
/// 定义，否则量子与指令成本会各自漂移。
pub const INSTRUCTION_COST_PS: u64 = crate::m12_engine::INSTRUCTION_COST_PS;

/// **中断固件**（手写机器码，小端）。
///
/// ```text
/// 0x00  addi  x5, x0, 0x20     ← handler 的地址
/// 0x04  lui  x6, 0x8           ← x6 = 0x8000
/// 0x08  srli x6, x6, 4         ← x6 = 0x800 = 1 << 11（**MEIE** 位号）
/// 0x0c  csrrw x0, mtvec, x5     ← mtvec = 0x20（直接模式）
/// 0x10  csrrw x0, mie, x6       ← mie 的 bit 11 —— 使能 line 1
/// 0x14  csrrsi x0, mstatus, 8   ← mstatus.MIE（bit 3）= 1
/// 0x18  jal   x0, 0x18          ← 主循环（自环，等中断）
/// 0x1c  nop
/// 0x20  addi  x7, x7, 1         ← handler：计数器 +1
/// 0x24  csrrwi x0, irq_clear, 2 ← handler：清 line 1（位掩码 bit 1）
/// 0x28  mret                    ← handler：回到 mepc
/// ```
///
/// ⚠️ **手写编码**：这一份固件是**手打**的字节（仓库里没有 RISC-V 汇编器
/// —— 交叉工具链在**仓库外**，见 `tools/fetch-golden-deps.sh`），每一行都对着
/// M-13 的译码规则核对过（CSR 指令的 csr 号在 bits[31:20]、zimm 在
/// bits[19:15]、funct3 在 bits[14:12]）。
///
/// ⚠️ **与端到端夹具的两份固件来源不同**：
/// `crates/kernel/tests/intc_end_to_end.rs` 的那份是**汇编器产出**的
/// （`riscv-none-elf-as -march=rv32i_zicsr` + `objdump` 逐条核对，汇编源在
/// `crates/kernel/tests/fixtures/intc_fw.S`）；**这一份**是手打的。
///
/// ⚠️ **`mie` 的值是 `1 << 11`（= 2048），不是 `11`**：`mie` 的
/// **位号**是 11（`MEIE`），而 CSR 里存的是**位图**。`csrrwi` 的
/// zimm 只有 5 位 ⇒ 写不进 2048 ⇒ 得先把它装进一个寄存器。
pub const IRQ_FIRMWARE_BYTES: [u8; 44] = [
    // 0x00  addi  x5, x0, 0x20     → 0x0200_0293
    0x93, 0x02, 0x00, 0x02, //
    // 0x04  lui  x6, 0x8           → 0x0000_8337
    0x37, 0x83, 0x00, 0x00, //
    // 0x08  srli x6, x6, 4         → 0x0043_5313
    0x13, 0x53, 0x43, 0x00, //
    // 0x0c  csrrw x0, mtvec, x5     → 0x3052_9073
    0x73, 0x90, 0x52, 0x30, //
    // 0x10  csrrw x0, mie, x6       → 0x3043_1073
    0x73, 0x10, 0x43, 0x30, //
    // 0x14  csrrsi x0, mstatus, 8   → 0x3004_6073
    0x73, 0x60, 0x04, 0x30, //
    // 0x18  jal   x0, 0x18          → 0x0000_006f
    0x6f, 0x00, 0x00, 0x00, //
    // 0x1c  nop
    0x13, 0x00, 0x00, 0x00, //
    // 0x20  addi  x7, x7, 1         → 0x0013_8393
    0x93, 0x83, 0x13, 0x00, //
    // 0x24  csrrwi x0, irq_clear, 2 → 0x7c01_5073
    0x73, 0x50, 0x01, 0x7c, //
    // 0x28  mret                    → 0x3020_0073
    0x73, 0x00, 0x20, 0x30,
];

/// 把 `state.pending_writes` 里的写**真正提交**到总线（D-07）。
///
/// 提交之后队列被清空 —— 否则同一个写会被提交两次。
///
/// **失败的那一条不会被丢掉**：写失败时错误向上抛，队列里剩下的写
/// （包括失败的那条）原样保留，调用方可以重试或诊断。
///
/// 返回成功提交的条数。
fn commit_pending_writes(state: &mut SimState, bus: &mut Bus) -> Result<usize, PlatformError> {
    let mut committed = 0usize;
    while let Some(write) = state.pending_writes.first().cloned() {
        bus.write(write.paddr, BusWidth::from(write.size), write.value)?;
        state.pending_writes.remove(0);
        committed += 1;
    }
    Ok(committed)
}

/// 把 `Rc<RefCell<T>>` 适配成 [`InterruptPort`]。
///
/// **为什么需要它**：M-16 的控制器在宿主里是**共享**的——引擎那条
/// `InterruptSink::raise` 通路与 M-17 的采样 / 交付通路必须看到**同一个**
/// 控制器（同一个量**一个**真相）。于是它得住在 `Rc<RefCell<…>>` 后面，
/// 而 [`InterruptPort`] 的方法要 `&mut self` ⇒ 需要一层适配。
///
/// ⚠️ **每次调用才借**（不是把 `RefMut` 持在手上）：M-17 的执行器在
/// **整个批次**里都持有这个端口的引用，而**同一条批次**里外设还会经
/// `InterruptSink::raise` 拉起 IRQ 行——那也要借**同一个**控制器。若
/// 适配器把 `RefMut` 持在手上，外设的拉起就**永远失败**
/// （`E_INT_01`），而失败是**看得见**的：guest 的中断从此再也拉不起来。
///
/// ⚠️ **争用不 panic**：借不到就**报错**（`E_INT_01`）或**当作没有**
/// （`take_intr` / `clear` 没有错误通道）——绝不 `unwrap`。
pub struct SharedInterruptPort<T: InterruptPort + 'static> {
    inner: Rc<RefCell<T>>,
}

impl<T: InterruptPort + 'static> SharedInterruptPort<T> {
    /// 造一个共享端口。
    pub fn new(inner: Rc<RefCell<T>>) -> Self {
        Self { inner }
    }
}

impl<T: InterruptPort + 'static> InterruptPort for SharedInterruptPort<T> {
    fn pending_intr(&self) -> Option<PendingIntr> {
        // 借不到就**当作没有**：`pending_intr` 只是「有没有待交付」的
        // 查询，没有错误通道。真正的交付在 `take_intr` 里判。
        self.inner.try_borrow().ok().and_then(|c| c.pending_intr())
    }

    fn take_intr(&mut self) -> Option<PendingIntr> {
        match self.inner.try_borrow_mut() {
            Ok(mut controller) => controller.take_intr(),
            // 争用 ⇒ 本轮**不交付**（挂起项**不丢**，下一次采样再来）。
            Err(_) => None,
        }
    }

    fn clear(&mut self, line: u16) {
        if let Ok(mut controller) = self.inner.try_borrow_mut() {
            controller.clear(line);
        }
    }

    fn poll(
        &mut self,
        now_ps: u64,
        mie: u32,
        mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        let mut controller = self.inner.try_borrow_mut().map_err(|_| {
            PlatformError::new(
                ErrorFamily::Interrupt,
                E_INT_01_CONTROLLER_BUSY,
                "中断控制器正被占用，这一轮**没有**采样",
            )
        })?;
        controller.poll(now_ps, mie, mstatus)
    }

    fn pending_lines(&self) -> Option<Vec<u16>> {
        // ⚠️ **借不到 ⇒ `None`（状态未知）**，**不是**「没有挂起」
        // （台账 D-100）：把「读不到」当成「没有挂起」⇒ 算出来的 `mip`
        // 掩码是 0 ⇒ 一次**失败的读**变成了 guest 看得见的状态改写。
        self.inner
            .try_borrow()
            .ok()
            .and_then(|controller| controller.pending_lines())
    }
}

/// 「没有装中断控制器」时的**空端口**。
///
/// 后端的端口字段**总是**有值——执行器要拿 `&mut dyn InterruptPort`
/// （冻结规格 §4.1 的签名），所以「还没装」这个状态也得有一个**说得清**
/// 的实现：它什么都不做（不采样、不交付、不清除）。
struct NullInterruptPort;

impl InterruptPort for NullInterruptPort {
    fn pending_intr(&self) -> Option<PendingIntr> {
        None
    }
    fn take_intr(&mut self) -> Option<PendingIntr> {
        None
    }
    fn clear(&mut self, _line: u16) {}
    fn poll(
        &mut self,
        _now_ps: u64,
        _mie: u32,
        _mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError> {
        Ok(None)
    }
    fn pending_lines(&self) -> Option<Vec<u16>> {
        // 空端口**没有**挂起线 —— 这是**知道的**「没有」，不是「读不到」。
        Some(Vec::new())
    }
}

/// M-17 的指令执行器：把 M-13 的 ISS 接进 M-12 的唯一主循环（D-07）。
///
/// 这是 M-12 的 [`InstructionExecutor`] 接缝的**唯一实现**。它借用
/// 后端手里的 ISS 与总线（不复制、不克隆），因此：
///
/// - 依赖方向单向：M-12 不认识 M-13，M-13 不认识 M-12；
/// - 状态仍然只有**一份**：ISS 推进的就是引擎持有的那份
///   [`SimState`]，虚拟时钟也在这份状态上。
///
/// 一条指令的执行顺序（M-08 LLD §3.2 / M-17 LLD）：
///
/// 1. **断点命中检测** —— 挂在**取指之前**，命中时该条指令尚未执行，
///    状态零副作用；
/// 2. **执行**（访存经 M-14 总线，钩子照常触发）；
/// 3. **推进虚拟时钟**（`INSTRUCTION_COST_PS`）；
/// 4. **提交待写**（`pending_writes` → 总线）；
/// 5. **观察点命中检测** —— 访存发生在指令**内部**，只能在指令返回后判。
pub struct IssExecutor<'a> {
    /// M-13 ISS。
    iss: &'a mut Iss,
    /// M-14 总线（访存与提交都走它）。
    bus: &'a mut Bus,
    /// M-08 断点表（取指前的命中判定）。
    breakpoints: &'a BreakpointTable,
    /// 观察点命中槽（与后端共享同一份）。
    watch_hit: Rc<RefCell<Option<Watchpoint>>>,
    /// 宿主暂停标志（与后端共享同一份）—— 裁决 C。
    pause_requested: &'a mut bool,
    /// M-16 的中断控制器（接缝 #2 采样 / 接缝 #3 拉取式交付 / 接缝 #4
    /// `irq_clear` 副作用的**同一个**端口）。
    intc: &'a mut dyn InterruptPort,
    /// 上次**量子边界采样**的时刻（跨批次持久 —— 裁决 I）。
    last_poll_ps: &'a mut u64,
}

impl<'a> IssExecutor<'a> {
    /// 构造一个执行器。
    ///
    /// ⚠️ `last_poll_ps` **必须**由**后端**持有并传进来：执行器是**每批次
    /// 新建**的（`step_n` / `continue_until_event` 各建一个）。挂在执行器上
    /// ⇒ 每次新批次都从 0 开始 ⇒ 「上次采样时刻」永远是 0 ⇒ **每个批次的
    /// 第一条指令都会触发一次采样** ⇒ 「仲裁只在量子边界发生」这条不变式
    /// 当场破掉（STAGE-4 裁决 I）。
    pub fn new(
        iss: &'a mut Iss,
        bus: &'a mut Bus,
        breakpoints: &'a BreakpointTable,
        watch_hit: Rc<RefCell<Option<Watchpoint>>>,
        intc: &'a mut dyn InterruptPort,
        last_poll_ps: &'a mut u64,
        pause_requested: &'a mut bool,
    ) -> Self {
        Self {
            iss,
            bus,
            breakpoints,
            watch_hit,
            pause_requested,
            intc,
            last_poll_ps,
        }
    }

    /// 该地址上有没有**生效中**的断点（取指之前判定）。
    ///
    /// 单独暴露出来，是为了让调用方能在**推进别的状态之前**先问一句
    /// ——`step_n` 必须在同步外设时钟**之前**问（V-A 检视 · F3）：一条
    /// **没有执行**的指令不该让外设的时钟也走一格。
    pub fn breakpoint_at(&self, pc: u32) -> bool {
        self.breakpoints.hit_check(pc).is_some()
    }

    /// **取走**一次宿主暂停请求（取走即消失）。
    ///
    /// 裁决 C：`pause()` 的语义 =「在**下一条指令边界**停下」。宿主调用
    /// 是**同步**的——它不可能在一条指令执行的中途插进来，所以「立刻停下」
    /// 是**做不到**的语义。于是暂停请求变成一个**标志位**，由执行器在
    /// 指令边界上查。
    ///
    /// ⚠️ **取走即消失**：一次暂停**只影响一个批次**。若标志位不清，
    /// 后面每一个批次都会在同一处停下，机器就**再也跑不动了**。
    pub fn take_pause(&mut self) -> bool {
        if *self.pause_requested {
            *self.pause_requested = false;
            true
        } else {
            false
        }
    }

    /// 按**位掩码**清除中断线（接缝 #4 的副作用）。
    ///
    /// 掩码的 bit L = 清除 line L（平台自定义 CSR `irq_clear` 的口径）。
    ///
    /// ⚠️ **写两次同一个值不会丢清除请求**：清除的判据是「这条指令**写了**
    /// `irq_clear`」，**不是**「值变了」（STAGE-4 §4 接缝 #4；v0.19 检视轮
    /// **F-1**）—— guest 的 handler 每次都写**同一个**掩码，若按「值变了」
    /// 判定，**第二次**就会被静默丢弃 ⇒ 那条线被单级防护**永久堵死**。
    fn clear_lines_by_mask(&mut self, mask: u32) {
        for line in 0..=MAX_MASKABLE_LINE {
            if mask & (1u32 << line) != 0 {
                self.intc.clear(line);
            }
        }
    }

    /// 把 M-16 的**挂起队列**投影进 CSR `mip`（D-84 / STAGE-6 裁决 B）。
    ///
    /// ⚠️ **落盘在 M-17，不在 M-13**：M-13 对 M-16 **零依赖**（裁决 B 的
    /// 理由 1）⇒ 读侧投影要么给 `SimState` 加字段（= CSR 的**第二落点**），
    /// 要么改 `csr_read` 的签名（≈10 个调用点）⇒ 侵入面远大于写侧。
    ///
    /// ⚠️ **平台的写不能经过 M-13 的「刚写了哪个 CSR」回报**（STAGE-4
    /// §646）：否则 M-17 会把「写了 `mip`」当成「写了 `irq_clear``
    /// ⇒ **清错线**。写侧投影天然规避这一点。
    ///
    /// ⚠️ **只在值变化时写** ⇒ 没有中断的 guest 的 CSR 文件**不含**
    /// `(0x344, …)` 条目 ⇒ 保住会话快照的「空」判定语义（验收 #4）。
    ///
    /// ⚠️ **时机**：只有两个刷新点 —— 量子边界 `poll()` 之后、
    /// `irq_clear` 副作用之后 ⇒ guest 读 `mip` 得到的是**上一个量子边界**
    /// 的挂起快照（量子 = 1 µs = 1000 条指令）⇒ **不是精确值**。
    fn project_mip(&self, state: &mut SimState) {
        // ⚠️ **读不到 ⇒ 不写**（台账 **D-100**）。
        //
        // 「读不到」**不是**「没有挂起」：把它当成「没有挂起」⇒ 算出的
        // 掩码是 0 ⇒ **一次失败的读**变成了 guest 看得见的状态改写
        // （`mip` 里的挂起位被抹掉，直到下一个刷新点才恢复）。
        //
        // ⇒ 状态未知时**保持原样**：guest 下一次读 `mip` 拿到的仍是**上
        // 一次已知的**投影值，而不是一个被伪造出来的 0。
        let Some(lines) = self.intc.pending_lines() else {
            return;
        };
        let mask = pending_lines_to_mip_mask(&lines);
        if csr_read(state, CSR_MIP) != mask {
            csr_write(state, CSR_MIP, mask);
        }
    }
}

impl InstructionExecutor for IssExecutor<'_> {
    fn step(&mut self, state: &mut SimState) -> Result<ExecOutcome, PlatformError> {
        // (0) 宿主暂停 —— 停在**下一条指令边界**（裁决 C）。
        //
        //     放在断点判定**之前**：暂停是**宿主的意图**，断点是 guest
        //     侧的条件；两者同时成立时，宿主说停就停。
        //
        //     `consumed: false` —— 被停下的那条指令**没有执行**，所以
        //     「已执行条数」不能多算一条。
        if self.take_pause() {
            return Ok(ExecOutcome::Stop {
                reason: HaltReason::UserPause,
                consumed: false,
            });
        }

        // (1) 断点命中检测 —— 挂在**取指之前**（M-08 LLD §3.2）。
        //     命中时该条指令尚未执行，状态零副作用。
        if self.breakpoint_at(state.pc) {
            return Ok(ExecOutcome::Stop {
                reason: HaltReason::Breakpoint,
                consumed: false,
            });
        }

        // (2) **量子边界采样**（接缝 #2）+ **拉取式交付**（接缝 #3）。
        //
        //     顺序是规格 §4 的那一条：断点 → 采样 → 交付 → 执行。
        //
        //     ⚠️ **只在跨过量子边界时采样**：`now_ps - last_poll_ps >=
        //     TIME_QUANTUM_PS`。采样读的是 CSR 文件里的 `mie` / `mstatus`
        //     （**使能的唯一真相**是 CSR `mie`，裁决 D）。
        let now_ps = state.virtual_time_ps;
        if now_ps.saturating_sub(*self.last_poll_ps) >= TIME_QUANTUM_PS {
            *self.last_poll_ps = now_ps;
            let mie = csr_read(state, CSR_MIE);
            let mstatus = csr_read(state, CSR_MSTATUS);
            self.intc.poll(now_ps, mie, mstatus)?;
            // ⚠️ **刷新点 ①**（STAGE-6 裁决 B）：量子边界采样之后，把
            //     M-16 的**挂起队列**投影进 CSR `mip` ⇒ guest 读 `mip`
            //     能看到挂起线（验收 #1）。
            self.project_mip(state);
        }

        // (2-bis) **把虚拟时间告诉总线**（M-14 的 `Bus::set_now_ps`）。
        //
        //     总线上的每一次读 / 写都会带一个时间戳（`BusAccess::ts_ps`）
        //     ⇒ M-18 的访存 trace 环里才有**真实**的时间轴。
        //
        //     ⚠️ **没有这一步**，M-18 的事件时间戳**恒为 0** ⇒ M-11 的
        //     执行轨迹导出里时间列全是 `0` ⇒ 时间轴**无意义**（导出看着
        //     有数据，却读不出「什么时候发生的」）。
        //
        //     ⚠️ M-14 的 `set_now_ps` 文档写着「Read / write callers
        //     should call this first so the trace events carry the
        //     current `now_ps`」——**这个接缝一直存在，只是没人接线**。
        //
        //     口径：**一条指令 = 一个时间戳**。取指、数据访存、待写提交
        //     （外设的写也走同一条提交路径）都落在**这条指令的起始时刻**
        //     ——与 D-46 的外设时钟同步点（指令边界）**同一个**口径。
        self.bus.set_now_ps(now_ps);

        // (3) 交付：**拉取式**（LLD 评审 16-4）—— M-16 登记了交付，M-17
        //     在**取指边界**把它取走并陷入。
        //
        //     ⚠️ 中断交付**不停批次**（不像异常那样停）：中断是「透明」的，
        //     handler 紧接着在同一条指令边界后开始执行。
        if let Some(intr) = self.intc.take_intr() {
            // 中断**没有**故障地址 ⇒ `mtval = 0`（规格 §3.5 的表）。
            // 陷入的其余部分（`mepc` / `mcause` / `mtval` / 关中断 / 跳
            // `mtvec`）由 M-13 的**唯一**陷入入口 `enter_trap` 负责 ——
            // 异常路径与中断路径**共用**它。
            enter_trap(state, intr.cause, 0);
            return Ok(ExecOutcome::InterruptTaken);
        }

        // (4) 执行一条指令（访存经 M-14）。
        //
        //     ⚠️ `irq_clear` 的**副作用**（接缝 #4）在 M-17，**不在** M-13：
        //     M-13 只管把值写进 CSR 文件，并**回报**「刚写了哪个 CSR」；
        //     「写了要清哪些线」由 M-17 承担。
        let outcome = self.iss.step_one_via_bus(state, self.bus);
        // ⚠️ **判据是「这条指令写了 `irq_clear`」，不是「`irq_clear` 的值
        //     变了」**（STAGE-4 §4 接缝 #4 的原话）。
        //
        //     「值变了」是**错的**：guest 的 handler **每次都写同一个掩码**
        //     ⇒ 第二次写时「值没变」⇒ 清除被**静默丢弃** ⇒ `active_irq`
        //     永不复位 ⇒ M-16 的单级嵌套防护把这条线**永久堵死**
        //     （v0.19 检视轮 **F-1**）。
        if self.iss.take_last_csr_written() == Some(CSR_IRQ_CLEAR) {
            self.clear_lines_by_mask(csr_read(state, CSR_IRQ_CLEAR));
            // ⚠️ **刷新点 ②**（STAGE-6 裁决 B）：清线的副作用之后，
            //     重新投影 `mip` ⇒ guest 清掉的线**立刻**从 `mip` 消失。
            self.project_mip(state);
        }
        // ⚠️ 这个 `match` **穷举** `StepOutcome` 的全部变体，**没有**兜底臂 ——
        //    M-13 以后再加变体，M-17 会**编译不过**（而不是静默走兜底）。
        //
        //    ⚠️ 这里**曾经**有三臂是**死分支**（`StepOutcome` 里的
        //    `HostPause` / `Breakpoint` / `Watchpoint` **从来没有生产者**）：
        //    「M-13 产生的宿主暂停」在架构上就是错的（**M-13 不可能知道
        //    宿主要暂停**）；M-08 的断点由 **M-17 在取指前**自己判；
        //    M-10 的观察点由**总线 hook** 记录、M-17 在指令返回后判。
        //    三个变体已随 **STAGE-4 裁决 C** 与台账 **D-59 / D-72** 删除。
        let trapped = match outcome {
            Ok(StepOutcome::Advanced) => false,
            Ok(StepOutcome::Trapped { .. }) => {
                // 陷入的指令**也算执行了**——它被取指、被译码、被触发。
                // 但它**没有退休**，所以虚拟时钟**不推进**（见 (3)）。
                true
            }
            Err(err) => {
                // 取指 / 访存失败（E_BUS-01 等）——**报错**，不是陷阱。
                //
                // ⚠️ 旧注释写「按陷阱处理」而代码是 `return Err(err)`——
                // 注释与代码矛盾（V-A 检视 · F12）。这里**如实**说一句：
                // 错误**向上传播**给调用方，由调用方决定怎么处理；执行器
                // 自己不把它降级成 `HaltReason::Trap`。
                //
                // （批处理那侧的 `step_n` 会把它记成 `Trap` 停止原因——那是
                // **批处理层**的选择，不是执行器这一层的行为。）
                return Err(err);
            }
        };

        // (5) 推进虚拟时钟（**陷阱不推进**——陷入的指令没有正常退休）。
        if !trapped {
            state.virtual_time_ps = state.virtual_time_ps.saturating_add(INSTRUCTION_COST_PS);
        }

        // (6) 提交待写（D-07）——`pending_writes` 从此**不只是被写进
        //     去**，而是真的落到总线上。
        //
        //     ⚠️ **在陷阱判定之前**（V-A 检视 · F4）：外设在 tick 期间
        //     已经发生的动作，与 guest 这条指令成不成功**无关**。写在
        //     队列里不提交 = 外设的写**永远看不见**。
        commit_pending_writes(state, self.bus)?;

        if trapped {
            return Ok(ExecOutcome::Stop {
                reason: HaltReason::Trap,
                consumed: true,
            });
        }

        // (7) 观察点命中 —— 访存发生在指令**内部**，所以只能在指令返回后
        //     判定。这里只**看**不取：命中槽由后端在批次结束时消费
        //     （一次命中不能在下一批里继续报），而宿主还需要知道**是哪
        //     一个**观察点命中的。
        if self.watch_hit.borrow().is_some() {
            return Ok(ExecOutcome::Stop {
                reason: HaltReason::Watchpoint,
                consumed: true,
            });
        }

        Ok(ExecOutcome::Advanced)
    }
}

/// The default backend: the in-tree RV32IMC ISS behind the M-12
/// scheduler. `B` is the event bus the engine publishes ticks on; `I`
/// is the interrupt sink peripherals raise IRQs through.
///
/// The engine is the **single source of truth** for the platform state:
/// the backend keeps no second copy of it.
pub struct SelfIssBackend<B: EventBus, I: InterruptSink> {
    iss: Iss,
    engine: Engine<B, I>,
    /// The M-14 memory bus the ISS fetches from / stores to.
    bus: Bus,
    /// M-08 breakpoint table — keyed by address, with an id reverse index.
    breakpoints: BreakpointTable,
    /// M-08 watchpoint table — shared with the bus hook (see
    /// [`SelfIssBackend::install_watch_hook`]).
    watchpoints: Rc<RefCell<WatchpointTable>>,
    /// The watchpoint hit the bus hook recorded since the last check.
    watch_hit: Rc<RefCell<Option<Watchpoint>>>,
    /// Why the last step batch stopped (`None` = it ran its budget out).
    last_stop: Option<HaltReason>,
    /// The watchpoint that fired in the last step batch, if any.
    last_watchpoint: Option<Watchpoint>,
    /// Next auto-assigned watchpoint id (port-level entries only).
    next_watch_id: u32,
    /// Total instructions executed since the last reset.
    total_executed: u64,
    /// 宿主请求暂停（裁决 C）。
    ///
    /// `pause()` 置位；执行器在**指令边界**上查它 ⇒ 命中就停。
    ///
    /// ⚠️ **取走即消失**（见 [`IssExecutor::take_pause`]）：一次暂停**只
    /// 影响一个批次**。否则后面每个批次都会在同一处停下，机器就再也
    /// 跑不动了。
    pause_requested: bool,
    /// M-16 的中断控制器**端口**（接缝 #2 / #3 / #4 的落点）。
    ///
    /// ⚠️ **不是**引擎那个 `I`：引擎持有的是 `InterruptSink`（外设**拉起**
    /// IRQ 行那条路），这里是 `InterruptPort`（**采样 / 交付 / 清除**那条
    /// 路）。两者在宿主里指向**同一个**控制器（`Rc<RefCell<…>>` 共享），
    /// **不是**两个控制器。
    ///
    /// ⚠️ 它**必须**是后端的一个**独立字段**：执行器要同时拿到
    /// `&mut Engine`（平台状态）与 `&mut dyn InterruptPort`（控制器），
    /// 而引擎**自己**就持有 `I` ⇒ 同一个结构体里再放一个「指向 `I` 的
    /// 引用」就是**自引用借用**（Rust 不允许，STAGE-4 裁决 I）。
    intc_port: Box<dyn InterruptPort>,
    /// 上次**量子边界采样**的时刻（跨批次持久 —— 裁决 I）。
    ///
    /// ⚠️ **必须**挂在**后端**上，不能挂在执行器上：执行器是**每批次
    /// 新建**的 ⇒ 挂在执行器上等于「每个批次的第一条指令都采样一次」。
    last_poll_ps: u64,
}

impl<B: EventBus, I: InterruptSink> SelfIssBackend<B, I> {
    /// Build a backend.
    ///
    /// The memory bus starts **empty** — the host maps RAM once it knows
    /// the project's images (see [`SelfIssBackend::with_memory_bus`]).
    pub fn new(event_bus: B, intc: I) -> Self {
        let mut backend = Self {
            iss: Iss::default(),
            engine: Engine::new(event_bus, intc),
            bus: Bus::new(),
            breakpoints: BreakpointTable::new(),
            watchpoints: Rc::new(RefCell::new(WatchpointTable::new())),
            watch_hit: Rc::new(RefCell::new(None)),
            last_stop: None,
            last_watchpoint: None,
            next_watch_id: 1,
            total_executed: 0,
            pause_requested: false,
            intc_port: Box::new(NullInterruptPort),
            last_poll_ps: 0,
        };
        backend.install_watch_hook();
        backend
    }

    /// Replace the memory bus (M-14) and re-install the watchpoint hook.
    ///
    /// The hook lives **in** the bus, so a new bus needs a new hook.
    pub fn with_memory_bus(mut self, bus: Bus) -> Self {
        self.bus = bus;
        self.install_watch_hook();
        self
    }

    /// Borrow the underlying engine (read-only).
    ///
    /// M-17 owns the engine; this accessor lets the host inspect the
    /// scheduler (pending events, virtual clock) without giving up the
    /// backend's own port surface.
    pub fn engine(&self) -> &Engine<B, I> {
        &self.engine
    }

    /// Borrow the underlying engine (mutable) — e.g. to schedule events.
    pub fn engine_mut(&mut self) -> &mut Engine<B, I> {
        &mut self.engine
    }

    /// Borrow the memory bus (M-14) read-only.
    pub fn bus(&self) -> &Bus {
        &self.bus
    }

    /// Borrow the memory bus (M-14) mutable.
    pub fn bus_mut(&mut self) -> &mut Bus {
        &mut self.bus
    }

    /// Why the last step batch stopped, if it stopped for a reason other
    /// than "the instruction budget ran out".
    pub fn last_stop_reason(&self) -> Option<HaltReason> {
        self.last_stop
    }

    /// Total instructions executed since the last reset.
    pub fn total_executed(&self) -> u64 {
        self.total_executed
    }

    /// 装上 M-16 的中断控制器**端口**（接缝 #2 / #3 / #4）。
    ///
    /// 装上之后，执行器才会在**取指边界**上：
    ///
    /// 1. 按**量子边界**采样并仲裁（接缝 #2）；
    /// 2. **拉取式**取走交付并陷入（接缝 #3）；
    /// 3. 在 guest 写 `irq_clear` 之后清掉对应的线（接缝 #4）。
    ///
    /// ⚠️ 端口是**独占**的：后端**接管**了这个盒子（`Box`），调用方若还要
    /// 观察控制器（导出状态、请求中断），得**另外**持有一份共享句柄
    /// （[`SharedInterruptPort`] 就是干这个的）。
    ///
    /// 幂等：重复装只是换掉端口，不会产生第二份状态。
    pub fn install_interrupt_port(&mut self, port: Box<dyn InterruptPort>) {
        self.intc_port = port;
        // 换控制器 ⇒ 「上次采样时刻」不再有意义（新的控制器还没被采样过）。
        self.last_poll_ps = 0;
    }

    /// 当前装着的 M-16 端口（只读）。
    pub fn interrupt_port(&self) -> &dyn InterruptPort {
        self.intc_port.as_ref()
    }

    /// The M-08 breakpoint table (read-only).
    pub fn breakpoints(&self) -> &BreakpointTable {
        &self.breakpoints
    }

    /// The M-08 watchpoint table (read-only).
    pub fn watchpoints(&self) -> std::cell::Ref<'_, WatchpointTable> {
        self.watchpoints.borrow()
    }

    /// Register a breakpoint with the **host-assigned** contract id.
    ///
    /// This is the contract-level entry point: the id in `bp` is the one
    /// the host will later use in `ClearBreakpoint{id}`.
    ///
    /// Invalid address / duplicate registration → `E_CTX-07` (from M-08).
    pub fn set_breakpoint_with_id(&mut self, bp: Breakpoint) -> Result<(), PlatformError> {
        self.breakpoints.set(bp)
    }

    /// Remove a breakpoint by the **host-assigned** contract id.
    ///
    /// Idempotent: an unknown id removes nothing and reports `None`.
    pub fn clear_breakpoint_by_id(&mut self, id: u32) -> Result<Option<u32>, PlatformError> {
        self.breakpoints.clear(id)
    }

    /// Enable / disable a breakpoint by the **host-assigned** contract id.
    pub fn set_breakpoint_enabled(
        &mut self,
        id: u32,
        enabled: bool,
    ) -> Result<bool, PlatformError> {
        self.breakpoints.set_enabled(id, enabled)
    }

    /// Register a watchpoint with the **host-assigned** contract id.
    pub fn set_watchpoint_with_id(&mut self, wp: Watchpoint) -> Result<(), PlatformError> {
        self.watchpoints.borrow_mut().set(wp)
    }

    /// Remove a watchpoint by the **host-assigned** contract id.
    pub fn clear_watchpoint_by_id(&mut self, id: u32) -> Option<Watchpoint> {
        self.watchpoints.borrow_mut().clear(id)
    }

    /// Set the program counter.
    ///
    /// Used after an image load: the reset vector comes from the image
    /// (the ELF entry point, or 0 for a raw blob), not from a constant
    /// baked into the backend.
    pub fn set_pc(&mut self, pc: u32) {
        self.engine.state_mut().pc = pc;
    }

    /// 通用寄存器组（x0..x31）的**当前值**。
    ///
    /// 读的是**引擎持有的那份** [`SimState`]——后端不持有第二份寄存器表
    /// （见模块头「状态单一真相」）。返回值是一次**快照**，不是活引用：
    /// 调用方拿到的是调用那一刻的值。
    ///
    /// ⚠️ **x0 恒为 0**（D-04）：x0 在体系结构上恒等于零，所以这里**不**
    /// 直接返回原始槽位——一份被损坏的快照（`Restore` 带进来的脏 x0）
    /// 不能让 x0 泄出非零值。写路径同理（见 [`SelfIssBackend::set_gpr`]）。
    pub fn register_file(&self) -> [u32; 32] {
        let mut gpr = self.engine.snapshot().gpr;
        gpr[0] = 0;
        gpr
    }

    /// 写**一个**通用寄存器（调试器写寄存器）。
    ///
    /// 写的是引擎持有的那份状态——**没有影子副本**（见模块头「状态单一
    /// 真相」）。下一次 `register_file()` / `snapshot()` 就能看到新值。
    ///
    /// - `index >= 32` → **拒绝**（`E_DBG-01`）。⚠️ 这里**拒绝**而不是
    ///   钳位：写钳位会**静默**写到别的寄存器——用户以为改了 x40，
    ///   实际改的是 x7。读越界没有副作用，所以读是钳位、写是拒绝。
    /// - `index == 0` → **丢弃**（D-04）：x0 恒等于零。调用方仍然会
    ///   收到回复（值恒为 0），不会把它当成一次失败。
    pub fn set_gpr(&mut self, index: u32, value: u32) -> Result<(), PlatformError> {
        if index >= 32 {
            return Err(PlatformError::new(
                DEBUG_FAMILY,
                E_DBG_01_INVALID_REGISTER_INDEX,
                format!("寄存器下标越界：x{index}（合法范围 x0..x31）"),
            ));
        }
        // `write_gpr` 是 M-13 的**唯一**写路径——它自己挡掉了 x0（D-04）。
        // 这里不重复实现那条规则，免得两处规则漂移。
        let state = self.engine.state_mut();
        write_gpr(state, index as u8, value);
        Ok(())
    }

    /// Replace the memory bus with a **fresh, empty** one.
    ///
    /// A new project is a new session (FR-13): the old memory image does
    /// not carry over. The watchpoint hook is re-installed because it
    /// lives **in** the bus.
    pub fn reset_memory_map(&mut self) {
        self.bus = Bus::new();
        self.install_watch_hook();
        // 换内存图 = 换外设集合：登记表要跟着清（D-07），否则主循环会
        // 一直去 tick 早已不存在的基址。
        self.engine.clear_peripherals();
    }

    /// **装上外设 tick 接缝**，并登记总线上的每一个外设（D-07）。
    ///
    /// 这一步是**时间驱动链路**的起点：没有它，
    /// [`SchedEvent::PeripheralTick`] 永远是一个空事件（`dispatch` 里的
    /// ticker 是 `None`），外设的时钟也永远不被虚拟时间推进。
    ///
    /// 接缝的形态是「按物理地址 tick 单个外设」：M-12 的
    /// [`Engine::tick_peripherals`] 逐个登记地址调用它，
    /// [`SchedEvent::PeripheralTick`] 事件也带着地址调用它——**同一个
    /// 回调，两条路径**。
    ///
    /// 幂等：重复调用只会重建接缝并（重新）登记同一批地址，不会产生
    /// 重复的 tick 事件（事件由 M-12 的主循环负责排期）。
    pub fn install_peripheral_ticker(&mut self) {
        let instances = self.bus.peripheral_instances();
        // The handles are `Rc` — cloning them is cheap and lets the
        // registration loop and the tick closure share one list.
        let bases: Vec<u32> = instances.iter().map(|(base, _)| *base).collect();
        let ticker = PeripheralTicker::new(move |paddr, now_ps| {
            // The event carries the peripheral's base address — look up
            // the instance and advance **that** one's clock.
            let Some((_, handle)) = instances.iter().find(|(base, _)| *base == paddr) else {
                // An address that isn't on the bus (a peripheral that was
                // replaced) — nothing to advance, and **not** an error:
                // the event is stale, not broken.
                return Ok(Vec::new());
            };
            let Ok(mut periph) = handle.try_borrow_mut() else {
                // The peripheral is busy (it is on the dispatch path) —
                // skip this tick. The next one comes along.
                return Ok(Vec::new());
            };
            periph.tick(now_ps)
        });
        self.engine.set_peripheral_ticker(ticker);
        for base in bases {
            self.engine.register_peripheral(base);
        }
    }

    /// Install the bus hook that matches watchpoints.
    ///
    /// The hook is a `Box<dyn FnMut(&BusAccess)>` — a `'static` closure —
    /// so it cannot borrow the backend. It therefore captures **shared
    /// handles** (`Rc<RefCell<…>>`) to the watchpoint table and to the
    /// hit slot; the backend holds the same handles.
    ///
    /// The hook records the **first** matching watchpoint of a step
    /// batch; the batch's stop reason is decided after the instruction
    /// returns (the access happens *inside* the instruction).
    fn install_watch_hook(&mut self) {
        let watchpoints = Rc::clone(&self.watchpoints);
        let watch_hit = Rc::clone(&self.watch_hit);
        // `add_hook`（追加），**不是** `set_hook`（替换全部）：总线上还
        // 要挂 M-18 的探针 hook（D-46），两个消费者都得看见每一次访存，
        // 后装的不能把先装的顶掉。
        //
        // 三个调用点（`new` / `with_memory_bus` / `reset_memory_map`）
        // 拿到的都是**没有**本模块 hook 的总线，所以不会重复安装。
        self.bus
            .add_hook(Box::new(move |access: &crate::m14_bus::BusAccess| {
                // A failed access is not an observed access — it never
                // reached memory, so it must not fire a watchpoint.
                if !access.ok {
                    return;
                }
                // An **instruction fetch** is not a data access. A
                // watchpoint watches the guest's data, so a read
                // watchpoint on a *code* address must not fire on every
                // instruction fetched from there (V-A 检视 · 🟠-2).
                if access.is_fetch {
                    return;
                }
                let is_write = matches!(access.dir, AccessDir::Write);
                let size: WriteSize = access.width.into();
                let hit = watchpoints
                    .borrow()
                    .match_access(access.addr, size, is_write);
                if let Some(wp) = hit {
                    let mut slot = watch_hit.borrow_mut();
                    if slot.is_none() {
                        *slot = Some(wp);
                    }
                }
            }));
    }

    /// The watchpoint that fired in the last step batch, if any.
    ///
    /// The bus hook's hit slot is **consumed** by the step loop (a
    /// watchpoint that fired must not keep firing on the next batch);
    /// what fired is recorded here instead of being thrown away, so the
    /// host can still tell *which* watchpoint it was.
    pub fn last_watchpoint_hit(&self) -> Option<&Watchpoint> {
        self.last_watchpoint.as_ref()
    }
}

impl<B: EventBus, I: InterruptSink> ExecutionBackend for SelfIssBackend<B, I> {
    fn reset(&mut self) -> Result<(), PlatformError> {
        // The engine is the source of truth for the platform state — a
        // reset that left it alone would keep the queue, the virtual
        // clock and the register file exactly as they were, and
        // `snapshot()` would go on reporting the pre-reset state.
        self.engine.reset();
        // Debug metadata belongs to the *session*, not to the machine
        // state: a reset starts a new run of the same program, so the
        // breakpoints survive it. (A `LoadProject` clears them through
        // `clear_debug_state`.)
        self.last_stop = None;
        self.last_watchpoint = None;
        self.total_executed = 0;
        // 暂停请求属于**上一次批次**的残留 —— reset 之后不该还有效。
        self.pause_requested = false;
        // 虚拟时钟归零 ⇒ 「上次采样时刻」也归零，否则第一次采样会被推迟
        // 一整个量子（复位之后中断迟迟不交付）。
        self.last_poll_ps = 0;
        *self.watch_hit.borrow_mut() = None;
        Ok(())
    }

    fn step_n(&mut self, n: u32) -> Result<u32, PlatformError> {
        let SelfIssBackend {
            iss,
            engine,
            bus,
            breakpoints,
            watch_hit,
            last_stop,
            last_watchpoint,
            total_executed,
            pause_requested,
            intc_port,
            last_poll_ps,
            ..
        } = self;

        let mut executed = 0u32;
        *last_stop = None;
        *last_watchpoint = None;
        // The hit slot carries **this batch's** hits only. It has to be
        // cleared here, not just consumed at the end of a batch: a hit can
        // also arrive from *outside* a step batch — a debugger write
        // (`WriteMemory`) fires the same bus hook — and such a leftover
        // would be mistaken for a hit of the **next** batch, stopping the
        // guest on an instruction that never touched the watched address
        // (V-A 检视 · 🟠-1).
        *watch_hit.borrow_mut() = None;

        // The instruction path shares the **same** executor as the
        // time-driven path (D-07): breakpoint judgement, watchpoint
        // judgement, the virtual clock and the pending-write commit all
        // live in one place — there is no second "step" implementation
        // that could drift from this one.
        let mut executor = IssExecutor::new(
            iss,
            bus,
            breakpoints,
            Rc::clone(watch_hit),
            intc_port.as_mut(),
            last_poll_ps,
            pause_requested,
        );

        for _ in 0..n {
            // (1) 宿主暂停 —— 停在**下一条指令边界**（裁决 C）。
            //
            //     与断点判定**同型**（都在取指之前、都在外设时钟同步之前）：
            //     一条**没有执行**的指令不该让外设的时钟也走一格。
            //
            //     ⚠️ **取走即消失**：一次暂停只影响**一个**批次（见
            //     [`IssExecutor::take_pause`]）。
            if executor.take_pause() {
                *last_stop = Some(HaltReason::UserPause);
                break;
            }

            // (2) 断点命中检测 —— 挂在**取指之前**（M-08 LLD §3.2）。
            //     命中时该条指令尚未执行，状态零副作用——**外设时钟也不
            //     推进**（V-A 检视 · F3：一条没执行的指令不该让外设的
            //     时钟也走一格）。
            if executor.breakpoint_at(engine.pc()) {
                *last_stop = Some(HaltReason::Breakpoint);
                break;
            }

            // (3) 外设时钟同步（**指令边界**）—— 让本条指令里发生的探针
            //     边沿带上**真实**的虚拟时间戳（D-46）。
            //
            //     没有这一步，M-15 的 GPIO 记下的 `now_ps` 永远是 0，
            //     边沿虽然是真的（值、次数都对），时间轴却是平的——
            //     波形看起来「有数据」，却读不出「什么时候变的」。
            //
            //     粒度是「每条指令一次」：这是探针边沿时间戳能**精确到
            //     单条指令**的唯一办法。契约里 `Peripheral::tick` 写的
            //     「每量子一次」是**下限**（至少要走到），不是上限。
            //
            //     ⚠️ 走的是 M-12 的 `tick_peripherals`（唯一的外设时钟
            //     同步点），不是 M-14 的 `Bus::tick_peripherals`——两处
            //     各调一次 `Peripheral::tick` 就是**两个真相**（D-07）。
            let now_ps = engine.now_ps();
            engine.tick_peripherals(now_ps)?;

            // (4) 执行一条指令（陷阱 / 观察点的判定都在执行器里）。
            match executor.step(engine.state_mut()) {
                Ok(ExecOutcome::Advanced) => {
                    executed += 1;
                    *total_executed += 1;
                }
                Ok(ExecOutcome::InterruptTaken) => {
                    // 中断交付**没有**指令退休 ⇒ 不计入 `executed` /
                    // `total_executed`（STAGE-4 裁决 I）。批次**不停**。
                }
                Ok(ExecOutcome::Stop { reason, consumed }) => {
                    if consumed {
                        executed += 1;
                        *total_executed += 1;
                    }
                    *last_stop = Some(reason);
                    break;
                }
                Err(err) => {
                    // 取指 / 访存失败（E_BUS-01 等）——**批处理层**把它记成
                    // 陷阱停止原因（并把错误向上传播）。
                    *last_stop = Some(HaltReason::Trap);
                    return Err(err);
                }
            }
        }

        // The hit slot belongs to this batch — consume it (a hit must not
        // keep firing in the next batch). Only a batch that actually
        // stopped on a watchpoint reports *which* watchpoint fired.
        if let Some(wp) = watch_hit.borrow_mut().take() {
            if *last_stop == Some(HaltReason::Watchpoint) {
                *last_watchpoint = Some(wp);
            }
        }
        Ok(executed)
    }

    fn continue_until_event(&mut self) -> Result<(), PlatformError> {
        // **时间驱动**：走 M-12 的**唯一主循环**（LLD §3.1 `runUntil`）。
        //
        // 与 `step_n` 的分工（D-07）：
        //
        // - `step_n` = **指令驱动**：恰好 n 条，调试器语义；
        // - 本方法 = **时间驱动**：量子化主循环 —— 派发到期事件
        //   （外设 tick / 定时器到期）、每量子执行至多 1000 条指令、
        //   在**指令边界**抢占。
        //
        // 停止条件：断点 / 陷阱 / 观察点 / 暂停，或本轮指令预算耗尽
        // （让出给宿主）。
        let SelfIssBackend {
            iss,
            engine,
            bus,
            breakpoints,
            watch_hit,
            last_stop,
            last_watchpoint,
            total_executed,
            pause_requested,
            intc_port,
            last_poll_ps,
            ..
        } = self;

        *last_stop = None;
        *last_watchpoint = None;
        *watch_hit.borrow_mut() = None;

        let mut executor = IssExecutor::new(
            iss,
            bus,
            breakpoints,
            Rc::clone(watch_hit),
            intc_port.as_mut(),
            last_poll_ps,
            pause_requested,
        );
        // `u64::MAX` = 「不设时间上限」——本轮的边界是**指令预算**，
        // 由 M-12 的 `instruction_budget` 决定。
        let outcome = engine.run_until(&mut executor, u64::MAX)?;
        *total_executed += outcome.instructions;
        if let RunReason::Stopped(reason) = outcome.reason {
            *last_stop = Some(reason);
        }
        if let Some(wp) = watch_hit.borrow_mut().take() {
            if *last_stop == Some(HaltReason::Watchpoint) {
                *last_watchpoint = Some(wp);
            }
        }
        Ok(())
    }

    /// 宿主请求暂停（裁决 C）。
    ///
    /// 语义 =「在**下一条指令边界**停下」——**不是**「立刻停下」：宿主
    /// 调用是**同步**的，宿主**不可能**在一条指令执行的中途插进来，
    /// 所以「立刻」是**做不到**的语义，写进文档就是骗人。
    ///
    /// 实现落点：置位 [`SelfIssBackend::pause_requested`]，由执行器在
    /// 指令边界上查它（`step_n` 在**外设时钟同步之前**查，时间驱动路径
    /// 在执行器里查——两条路径共用同一个标志位）。
    ///
    /// ⚠️ **取走即消失**：标志位被**消费**它的那一批清掉 ⇒ 一次暂停
    /// **只影响一个批次**（见 [`IssExecutor::take_pause`]）。
    fn pause(&mut self) -> Result<(), PlatformError> {
        self.pause_requested = true;
        self.last_stop = Some(HaltReason::UserPause);
        Ok(())
    }

    fn snapshot(&self) -> Result<SimState, PlatformError> {
        Ok(self.engine.snapshot())
    }

    fn restore(&mut self, state: &SimState) -> Result<(), PlatformError> {
        self.engine.restore(state.clone());
        // 虚拟时间被**换掉**了 ⇒ 「上次采样时刻」不再可比：归零，让
        // 恢复后的第一条指令就重新采样（否则恢复到一个更早的时刻时，
        // 中断要等虚拟时间追上旧值才交付）。
        self.last_poll_ps = 0;
        Ok(())
    }

    /// Capture a **complete engine-level** snapshot: platform state **and**
    /// the pending event queue, in the contract's serializable form.
    ///
    /// Unlike [`ExecutionBackend::snapshot`] (partial), this round-trips
    /// deterministically through [`ExecutionBackend::restore_full`] — the
    /// queue contents, the queue order, the insertion counter and the
    /// virtual time all survive (D-24 / D-05).
    ///
    /// **Scope limit**: peripheral models (M-15) and the interrupt
    /// controller (M-16) are **not** part of it — the engine does not own
    /// them and cannot serialize them.
    fn snapshot_full(&self) -> Result<EngineSnapshotDto, PlatformError> {
        Ok(self.engine.snapshot_dto())
    }

    /// Restore a complete snapshot captured by
    /// [`ExecutionBackend::snapshot_full`].
    ///
    /// The pending event queue is restored **as it was**, so events queued
    /// after the snapshot was taken no longer fire — this is the
    /// deterministic restore that the partial
    /// [`ExecutionBackend::restore`] cannot provide.
    ///
    /// The snapshot is **validated before it is applied**: a malformed one
    /// is rejected with an error and the engine is left untouched.
    fn restore_full(&mut self, snapshot: &EngineSnapshotDto) -> Result<(), PlatformError> {
        self.engine.restore_dto(snapshot)?;
        // 同 [`Self::restore`]：虚拟时间换了 ⇒ 「上次采样时刻」归零。
        self.last_poll_ps = 0;
        Ok(())
    }

    fn read_memory(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        // The port method takes `&self`, but the bus's own `read` needs
        // `&mut self` to bump the counters. `read_only` is the read-only
        // twin: same value, same error — but the debugger's peek does
        // **not** count as a bus access (otherwise every peek would show
        // up in the statistics, in the M-18 trace, and in a watchpoint
        // match).
        self.bus.read_only(addr, BusWidth::from(size))
    }

    fn write_memory(
        &mut self,
        addr: u32,
        size: WriteSize,
        value: u32,
    ) -> Result<(), PlatformError> {
        self.bus.write(addr, BusWidth::from(size), value)
    }

    fn set_breakpoint(&mut self, addr: u32, enabled: bool) -> Result<(), PlatformError> {
        // Port-level semantics: **toggle by address**. A breakpoint
        // already bound to `addr` has its enable bit flipped; otherwise
        // a new one is created with an auto-assigned id.
        if let Some(bp) = self.breakpoints.get_by_addr(addr).cloned() {
            return self.breakpoints.set_enabled(bp.id, enabled).map(|_| ());
        }
        let id = self.next_free_breakpoint_id();
        self.breakpoints.set(Breakpoint {
            id,
            addr,
            source_hint: None,
            enabled,
        })
    }

    fn set_watchpoint(
        &mut self,
        addr: u32,
        size: WriteSize,
        write: bool,
        enabled: bool,
    ) -> Result<(), PlatformError> {
        let access = if write {
            WatchAccess::Write
        } else {
            WatchAccess::Read
        };
        let mut table = self.watchpoints.borrow_mut();
        // Toggle an existing watchpoint bound to the same address.
        let existing = table
            .iter()
            .find(|wp| wp.addr == addr && wp.size == size && wp.access == access)
            .map(|wp| wp.id);
        if let Some(id) = existing {
            if enabled {
                let mut wp = table.get(id).cloned().unwrap_or(Watchpoint {
                    id,
                    addr,
                    size,
                    access,
                });
                wp.access = access;
                wp.size = size;
                table.set(wp)?;
                return Ok(());
            }
            // The contract's `Watchpoint` has **no enable bit** (unlike
            // `Breakpoint`), so "disable" can only mean "remove".
            table.clear(id);
            return Ok(());
        }
        if !enabled {
            // Nothing to disable — idempotent success.
            return Ok(());
        }
        let id = self.next_watch_id;
        self.next_watch_id = self.next_watch_id.wrapping_add(1);
        table.set(Watchpoint {
            id,
            addr,
            size,
            access,
        })
    }
}

impl<B: EventBus, I: InterruptSink> SelfIssBackend<B, I> {
    /// Next free breakpoint id for the port-level (address-toggle) path.
    fn next_free_breakpoint_id(&self) -> u32 {
        let mut id = 1u32;
        while self.breakpoints.get(id).is_some() {
            id = id.wrapping_add(1);
            if id == 0 {
                break;
            }
        }
        id
    }

    /// Clear the debug metadata (breakpoints + watchpoints).
    ///
    /// Used by the host when a **new project** is loaded: a new project
    /// is a new session, and the old session's breakpoints do not carry
    /// over (FR-13 — session isolation).
    pub fn clear_debug_state(&mut self) {
        self.breakpoints.clear_all();
        self.watchpoints.borrow_mut().clear_all();
        *self.watch_hit.borrow_mut() = None;
        self.last_watchpoint = None;
    }
}

impl<B: EventBus, I: InterruptSink> Default for SelfIssBackend<B, I>
where
    B: Default,
    I: Default,
{
    fn default() -> Self {
        Self::new(B::default(), I::default())
    }
}

/// Family used by errors raised from this module.
pub const BACKEND_FAMILY: ErrorFamily = ErrorFamily::Iss;

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::csr::{
        std_irq_bit_for_line, CSR_MCAUSE, CSR_MEPC, CSR_MIE, CSR_MIP, CSR_MTVAL, CSR_MTVEC,
    };
    use riscv_sim_contract::model::Breakpoint;
    use riscv_sim_contract::protocol::EventPayload;

    use crate::m16_interrupt::{InterruptController, SOURCE_PLATFORM, TRIGGER_EDGE};

    /// 事件总线 + 中断汇的最小实现。
    #[derive(Debug, Default)]
    struct NullSinks;

    impl EventBus for NullSinks {
        fn publish(&mut self, _evt: EventPayload) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    impl InterruptSink for NullSinks {
        fn raise(&mut self, _irq: u16) -> Result<(), PlatformError> {
            Ok(())
        }
        fn lower(&mut self, _irq: u16) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    /// demo 程序：`addi x1,x0,42` / `addi x2,x0,7` / `add x3,x1,x2` /
    /// `sw x3,0x100(x0)` / `jal x0,0x20` / … / `addi x4,x0,1`（0x20）。
    const DEMO: [u8; 40] = [
        0x93, 0x00, 0xa0, 0x02, 0x13, 0x01, 0x70, 0x00, 0xb3, 0x81, 0x20, 0x00, 0x23, 0x20, 0x30,
        0x10, 0x6f, 0x00, 0x00, 0x01, 0x13, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x13, 0x00,
        0x00, 0x00, 0x13, 0x02, 0x10, 0x00, 0x6f, 0x00, 0x00, 0x00,
    ];

    /// 造一个后端，低段 RAM 已映射并装入 demo 程序。
    fn backend_with_demo() -> SelfIssBackend<NullSinks, NullSinks> {
        backend_with_program(&DEMO)
    }

    /// 造一个后端，低段 RAM 已映射并装入给定程序（从地址 0 起）。
    fn backend_with_program(program: &[u8]) -> SelfIssBackend<NullSinks, NullSinks> {
        let mut bus = Bus::new();
        bus.map_ram(0, 0x1_0000, program).expect("map ram");
        SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus)
    }

    /// 观察点 hook 用 `add_hook`（追加），**不是** `set_hook`（替换全部）。
    ///
    /// 总线上要挂**不止一个**消费者：M-17 的观察点匹配器 + M-18 的访存
    /// trace 环（D-46）。后装的不能把先装的顶掉。
    ///
    /// ⚠️ 判别力：把 `install_watch_hook` 里的 `add_hook` 改回 `set_hook`
    /// ——这条用例立刻变红（别人的 hook 被清掉了）。**与装配顺序无关**：
    /// 这里故意先在总线上装一个「别人的 hook」，再交给后端。
    #[test]
    fn installing_the_watch_hook_does_not_evict_an_existing_hook() {
        use std::cell::Cell;
        use std::rc::Rc;

        let seen = Rc::new(Cell::new(0u32));
        let mut bus = Bus::new();
        bus.map_ram(0, 0x1_0000, &DEMO).expect("map ram");
        let counter = Rc::clone(&seen);
        bus.add_hook(Box::new(move |_a: &crate::m14_bus::BusAccess| {
            counter.set(counter.get() + 1);
        }));

        let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
        backend.step_n(1).expect("step");

        assert!(
            seen.get() > 0,
            "总线上别人装好的 hook 必须还在——观察点 hook 不能顶掉它"
        );
        assert!(
            backend.bus().hook_count() >= 2,
            "总线上必须同时有「别人的 hook」与「观察点 hook」，实测 {}",
            backend.bus().hook_count()
        );
    }

    #[test]
    fn step_n_executes_guest_instructions() {
        let mut backend = backend_with_demo();
        let executed = backend.step_n(3).expect("step");
        assert_eq!(executed, 3);
        let state = backend.snapshot().expect("snapshot");
        assert_eq!(state.gpr[1], 42);
        assert_eq!(state.gpr[2], 7);
        assert_eq!(state.gpr[3], 49);
        assert_eq!(state.pc, 0x0c);
    }

    #[test]
    fn step_n_advances_the_virtual_clock() {
        let mut backend = backend_with_demo();
        assert_eq!(backend.engine().now_ps(), 0);
        backend.step_n(3).expect("step");
        assert_eq!(
            backend.engine().now_ps(),
            3 * INSTRUCTION_COST_PS,
            "每条指令推进一次虚拟时钟"
        );
    }

    #[test]
    fn bus_accesses_carry_the_virtual_time() {
        use std::cell::RefCell;
        use std::rc::Rc;

        // ⚠️ **总线上的访问必须带真实的时间戳**（M-14 的 `BusAccess::ts_ps`）。
        //
        //     没有 `Bus::set_now_ps` 的那次接线 ⇒ 时间戳**恒为 0** ⇒
        //     M-18 的访存 trace 环里全是 0 ⇒ M-11 的执行轨迹导出**时间轴是
        //     平的**（数据看着有，却读不出「什么时候发生的」）。
        //
        // 判别式：**时间戳必须随指令递增**（全 0 ⇒ 不递增 ⇒ 变红）。
        let stamps: Rc<RefCell<Vec<u64>>> = Rc::new(RefCell::new(Vec::new()));
        let mut bus = Bus::new();
        bus.map_ram(0, 0x1_0000, &DEMO).expect("map ram");
        let sink = Rc::clone(&stamps);
        bus.add_hook(Box::new(move |access: &crate::m14_bus::BusAccess| {
            sink.borrow_mut().push(access.ts_ps);
        }));

        let mut backend = SelfIssBackend::new(NullSinks, NullSinks).with_memory_bus(bus);
        backend.step_n(3).expect("step");

        let seen = stamps.borrow().clone();
        assert_eq!(seen.len(), 3, "三条指令 ⇒ 三次取指：{seen:?}");
        // 第一条指令在 t=0 跑（虚拟时钟从 0 起）⇒ 第一个戳是 0；
        // **后面**的必须**递增** ⇒ 恒 0 会在这里变红。
        assert_eq!(seen[0], 0, "第一条指令跑在虚拟时刻 0");
        assert_eq!(seen[1], INSTRUCTION_COST_PS, "第二条指令跑在上一条之后");
        assert_eq!(seen[2], 2 * INSTRUCTION_COST_PS, "依次递增");
    }

    #[test]
    fn step_n_stops_at_a_breakpoint_without_executing_it() {
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 1,
                addr: 0x20,
                source_hint: None,
                enabled: true,
            })
            .expect("set breakpoint");
        let executed = backend.step_n(100).expect("step");
        assert_eq!(executed, 5, "0x00..0x10 共 5 条，然后命中 0x20");
        assert_eq!(backend.snapshot().expect("snapshot").pc, 0x20);
        assert_eq!(backend.last_stop_reason(), Some(HaltReason::Breakpoint));
    }

    #[test]
    fn a_disabled_breakpoint_does_not_stop_execution() {
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 1,
                addr: 0x20,
                source_hint: None,
                enabled: false,
            })
            .expect("set breakpoint");
        backend.step_n(6).expect("step");
        assert_eq!(backend.last_stop_reason(), None);
    }

    #[test]
    fn step_n_reports_a_trap_on_an_unmapped_fetch() {
        let mut backend: SelfIssBackend<NullSinks, NullSinks> =
            SelfIssBackend::new(NullSinks, NullSinks);
        let err = backend.step_n(1).unwrap_err();
        assert_eq!(err.family, ErrorFamily::Iss);
        assert_eq!(backend.last_stop_reason(), Some(HaltReason::Trap));
    }

    #[test]
    fn write_then_read_memory_round_trips() {
        let mut backend = backend_with_demo();
        backend
            .write_memory(0x200, WriteSize::Word, 0xdead_beef)
            .expect("write");
        assert_eq!(
            backend.read_memory(0x200, WriteSize::Word).expect("read"),
            0xdead_beef
        );
    }

    #[test]
    fn read_memory_on_an_unmapped_address_is_an_error() {
        let backend = backend_with_demo();
        let err = backend
            .read_memory(0x4000_0000, WriteSize::Word)
            .unwrap_err();
        assert_eq!(err.family, ErrorFamily::Bus);
        assert_eq!(err.scenario, 1);
    }

    #[test]
    fn read_memory_does_not_count_as_a_bus_access() {
        let backend = backend_with_demo();
        let before = backend.bus().stats().reads;
        backend.read_memory(0, WriteSize::Word).expect("read");
        assert_eq!(
            backend.bus().stats().reads,
            before,
            "调试读不得计入总线统计"
        );
    }

    #[test]
    fn clear_breakpoint_by_id_is_idempotent() {
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 7,
                addr: 0x20,
                source_hint: None,
                enabled: true,
            })
            .expect("set");
        assert_eq!(
            backend.clear_breakpoint_by_id(7).expect("clear"),
            Some(0x20)
        );
        assert_eq!(backend.clear_breakpoint_by_id(7).expect("clear"), None);
        assert_eq!(backend.breakpoints().len(), 0);
    }

    #[test]
    fn a_watchpoint_fires_on_a_matching_store() {
        let mut backend = backend_with_demo();
        backend
            .set_watchpoint_with_id(Watchpoint {
                id: 1,
                addr: 0x100,
                size: WriteSize::Word,
                access: WatchAccess::Write,
            })
            .expect("set watchpoint");
        // 执行到 `sw x3, 0x100(x0)`（第 4 条指令）。
        backend.step_n(4).expect("step");
        assert_eq!(backend.last_stop_reason(), Some(HaltReason::Watchpoint));
        assert_eq!(
            backend.last_watchpoint_hit().map(|w| w.addr),
            Some(0x100),
            "命中的观察点地址"
        );
        // 命中槽被消费掉，下一批不再重复报。
        backend.step_n(1).expect("step");
        assert_ne!(backend.last_stop_reason(), Some(HaltReason::Watchpoint));
    }

    #[test]
    fn a_watchpoint_does_not_fire_on_a_read() {
        let mut backend = backend_with_demo();
        backend
            .set_watchpoint_with_id(Watchpoint {
                id: 1,
                addr: 0x100,
                size: WriteSize::Word,
                access: WatchAccess::Read,
            })
            .expect("set watchpoint");
        backend.step_n(4).expect("step");
        assert_ne!(
            backend.last_stop_reason(),
            Some(HaltReason::Watchpoint),
            "只监视读，就不该被写命中"
        );
    }

    #[test]
    fn reset_clears_the_stop_reason_but_keeps_the_breakpoints() {
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 1,
                addr: 0x20,
                source_hint: None,
                enabled: true,
            })
            .expect("set");
        backend.step_n(1).expect("step");
        backend.reset().expect("reset");
        assert_eq!(backend.last_stop_reason(), None);
        assert_eq!(backend.breakpoints().len(), 1, "reset 不该清掉断点");
        assert_eq!(backend.snapshot().expect("snapshot").gpr[1], 0);
    }

    #[test]
    fn clear_debug_state_removes_the_breakpoints() {
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 1,
                addr: 0x20,
                source_hint: None,
                enabled: true,
            })
            .expect("set");
        backend.clear_debug_state();
        assert_eq!(backend.breakpoints().len(), 0);
        assert_eq!(backend.watchpoints().len(), 0);
    }

    // -- V-A 检视闭环（🟠-1 / 🟠-2 / 🟡-2 / 🟡-3）---------------------------

    #[test]
    fn a_debugger_write_does_not_leak_into_the_next_step_batch() {
        // 回归 V-A 🟠-1：调试器写内存会触发同一条总线 hook，把命中槽填上；
        // 那一批没有 step 去消费它，残留到**下一批**被误当成本批的命中 ——
        // 于是 guest 在一条根本没碰过被监视地址的指令上假停机。
        let mut backend = backend_with_demo();
        backend
            .set_watchpoint_with_id(Watchpoint {
                id: 1,
                addr: 0x200,
                size: WriteSize::Word,
                access: WatchAccess::Write,
            })
            .expect("set watchpoint");
        // 调试器写一次被监视的地址（不是 guest 干的）。
        backend
            .write_memory(0x200, WriteSize::Word, 0x1234)
            .expect("debugger write");
        // 接下来跑一条完全不碰 0x200 的指令（`addi x1,x0,42`）。
        let executed = backend.step_n(1).expect("step");
        assert_eq!(executed, 1, "这条指令不该被观察点打断");
        assert_ne!(
            backend.last_stop_reason(),
            Some(HaltReason::Watchpoint),
            "调试器的写不得让下一条 guest 指令误报观察点命中"
        );
        assert_eq!(
            backend.last_watchpoint_hit().map(|w| w.addr),
            None,
            "本批次没有命中任何观察点"
        );
    }

    #[test]
    fn an_instruction_fetch_does_not_fire_a_read_watchpoint() {
        // 回归 V-A 🟠-2：取指也是一次总线读。若不区分「取指」与「数据读」，
        // 挂在**代码地址**上的读观察点会被每一次取指误触发。
        let mut backend = backend_with_demo();
        backend
            .set_watchpoint_with_id(Watchpoint {
                id: 1,
                addr: 0x00,
                size: WriteSize::Word,
                access: WatchAccess::Read,
            })
            .expect("set watchpoint");
        let executed = backend.step_n(3).expect("step");
        assert_eq!(executed, 3, "取指不得触发观察点");
        assert_ne!(
            backend.last_stop_reason(),
            Some(HaltReason::Watchpoint),
            "取指不是数据访问，不得命中读观察点"
        );
    }

    #[test]
    fn a_data_read_still_fires_a_read_watchpoint() {
        // 🟠-2 的**对照**：区分取指之后，真正的**数据读**仍然要命中读
        // 观察点 —— 否则这个修复就变成了「读观察点整个失灵」。
        //
        // 程序只有一条 `lw x1, 0x100(x0)`（0x10002083，小端字节 83 20 00
        // 10），它是对 0x100 的一次**数据读**。
        //
        // ⚠️ 编码核对：L 型立即数的 imm[11:5] 在 **bit[31:25]**，
        // imm[4:0] 在 bit[24:20]。0x100 → imm[11:5] = 8 → 8 << 25 =
        // 0x10000000，加上 funct3=2（0x2000）、rd=1（0x80）、opcode=3
        // → 0x10002083。
        let mut backend = backend_with_program(&[0x83, 0x20, 0x00, 0x10]);
        backend
            .set_watchpoint_with_id(Watchpoint {
                id: 1,
                addr: 0x100,
                size: WriteSize::Word,
                access: WatchAccess::Read,
            })
            .expect("set watchpoint");
        backend.step_n(1).expect("step");
        assert_eq!(
            backend.last_stop_reason(),
            Some(HaltReason::Watchpoint),
            "guest 的数据读仍然要命中读观察点"
        );
        assert_eq!(backend.last_watchpoint_hit().map(|w| w.addr), Some(0x100));
    }

    #[test]
    fn the_execution_count_pins_down_what_counts_as_executed() {
        // 回归 V-A 🟡-2：计数口径此前只写在代码注释里，没有任何用例守护。
        //
        // 口径（两条都要钉住）：
        // ① **陷入**的指令算「执行了」——它被取指、被译码、被触发；
        // ② **取指失败**（总线错误）不算——那条指令根本没被取到。
        //
        // ① 用「奇地址 PC」触发：对齐检查先于取指（M-13 LLD 评审 13-3）。
        let mut backend = backend_with_demo();
        backend.set_pc(1);
        let executed = backend.step_n(1).expect("step");
        assert_eq!(executed, 1, "陷入的指令计入执行数");
        assert_eq!(backend.total_executed(), 1);
        assert_eq!(backend.last_stop_reason(), Some(HaltReason::Trap));

        // ② 用「没有任何内存映射」触发取指失败。
        let mut backend: SelfIssBackend<NullSinks, NullSinks> =
            SelfIssBackend::new(NullSinks, NullSinks);
        let err = backend.step_n(1).unwrap_err();
        assert_eq!(err.family, ErrorFamily::Iss);
        assert_eq!(
            backend.total_executed(),
            0,
            "取指失败的那条指令没有被取到，不计入执行数"
        );
    }

    #[test]
    fn total_executed_counts_every_executed_instruction() {
        // 回归 V-A 🟡-2：`total_executed` 此前**零引用**，改坏也没有测试变红。
        let mut backend = backend_with_demo();
        backend.step_n(3).expect("step");
        assert_eq!(backend.total_executed(), 3);
        backend.step_n(2).expect("step");
        assert_eq!(backend.total_executed(), 5, "计数跨批次累加");
    }

    #[test]
    fn continue_until_event_runs_the_quantum_main_loop() {
        // **D-07 的行为订正**：第 1 阶段 `continue_until_event` 只是「把
        // 事件队列排空」，**不执行** guest 指令（那时的队列恒空，于是它
        // 等价于空操作）。D-07 把它接到 M-12 的**唯一主循环**上：时间
        // 驱动与指令驱动在主循环里交替，Continue 因此真的会跑代码。
        //
        // 判别式钉住三件事：
        // 1. 指令**真的**被执行了（PC 前进、计数增加）；
        // 2. 一轮的边界是**指令预算**，不是「跑到没事件」；
        // 3. 执行的条数是**量子配额**的整数倍（量子化调度）。
        let mut backend = backend_with_demo();
        let pc_before = backend.snapshot().expect("snapshot").pc;
        backend.continue_until_event().expect("run until event");

        assert!(
            backend.total_executed() > 0,
            "时间驱动路径必须真的执行 guest 指令"
        );
        assert_ne!(
            backend.snapshot().expect("snapshot").pc,
            pc_before,
            "PC 必须前进"
        );
        assert_eq!(
            backend.total_executed() as u32 % crate::m12_engine::QUANTUM_INSTR_QUOTA,
            0,
            "一轮的执行条数必须是量子配额的整数倍（量子化调度）"
        );
        // ⚠️ **这一条是 D-57 的判别式本体**，而且它**必须**是**相对**的。
        //
        // 「一轮跑满指令预算」与「一轮停在量子边界」的区别是**十倍**
        // （10 000 条 vs 1 000 条）。但断言「正好等于
        // `DEFAULT_INSTRUCTION_BUDGET`」是**自指**的——那个常量**就是**
        // 实现用的那个，把它改成 1 000，断言照样绿：「改了行为」与
        // 「没改行为」在那种断言下**无法分辨**（D-73）。
        //
        // 所以判别式落在**量子配额**上（一个与预算无关的量）：
        // 「跑过**不止一个量子**」——停在量子边界是**恰好**一个量子，
        // 跑满预算则是**多个**量子。
        assert!(
            backend.total_executed() > u64::from(crate::m12_engine::QUANTUM_INSTR_QUOTA),
            "一轮必须跑过**不止一个量子**（量子配额 {} 条）—— \
             停在量子边界是恰好一个量子，跑满指令预算才是多个量子",
            crate::m12_engine::QUANTUM_INSTR_QUOTA
        );
        assert_eq!(
            backend.total_executed(),
            u64::from(crate::m12_engine::DEFAULT_INSTRUCTION_BUDGET),
            "没有断点/陷阱时，一轮应当正好跑满指令预算然后让出给宿主"
        );
    }

    #[test]
    fn continue_until_event_stops_at_a_breakpoint() {
        // 时间驱动路径也必须**认断点**——否则「跑到断点停下」这个
        // 调试器最基本的语义在 Continue 下就没了。
        let mut backend = backend_with_demo();
        backend
            .set_breakpoint_with_id(Breakpoint {
                id: 7,
                addr: 0,
                source_hint: None,
                enabled: true,
            })
            .expect("set breakpoint");

        backend.continue_until_event().expect("run until event");

        assert_eq!(
            backend.last_stop_reason(),
            Some(HaltReason::Breakpoint),
            "断点必须让时间驱动路径停下来"
        );
        assert_eq!(
            backend.total_executed(),
            0,
            "断点挂在取指之前，命中的那条指令**没有**被执行"
        );
    }

    // ---- 裁决 C：`pause()` = 在**下一条指令边界**停下 ----

    /// `pause()` ⇒ 批次在**下一条指令边界**停（裁决 C / 验收 31）。
    ///
    /// 判别式落在**输出**上：停下的那条指令**没有执行** ⇒
    /// 「已执行条数」不多算、PC 不前进、停止原因是 `UserPause`。
    ///
    /// ⚠️ 期望值用**外部**常量（`HaltReason::UserPause`），不自指实现。
    #[test]
    fn pause_stops_the_batch_at_the_next_instruction_boundary() {
        let mut backend = backend_with_demo();
        let pc_before = backend.snapshot().expect("snapshot").pc;

        backend.pause().expect("pause");
        assert_eq!(
            backend.last_stop_reason(),
            Some(HaltReason::UserPause),
            "pause() 之后宿主立刻就能看到「暂停」这个停止原因"
        );

        let executed = backend.step_n(10).expect("step");
        assert_eq!(
            executed, 0,
            "被暂停的那条指令**没有执行**——已执行条数不能多算一条"
        );
        assert_eq!(
            backend.total_executed(),
            0,
            "已执行条数必须说真话：暂停的那条指令没有退休"
        );
        assert_eq!(
            backend.last_stop_reason(),
            Some(HaltReason::UserPause),
            "停止原因必须是 UserPause"
        );
        assert_eq!(
            backend.snapshot().expect("snapshot").pc,
            pc_before,
            "PC 不能前进——暂停发生在**指令边界**（取指之前）"
        );
    }

    /// 一次暂停**只影响一个批次**（裁决 C）。
    ///
    /// ⚠️ 判别力：若标志位**取走不消失**，后面每个批次都会在同一处停下，
    /// 机器就**再也跑不动了**——这条用例会把那种实现打红（第二个批次
    /// 的已执行条数会是 0）。
    #[test]
    fn a_pause_affects_exactly_one_batch() {
        let mut backend = backend_with_demo();
        backend.pause().expect("pause");
        assert_eq!(backend.step_n(10).expect("step"), 0, "第一批被暂停");

        let executed = backend.step_n(10).expect("step");
        assert_eq!(executed, 10, "暂停只影响**一个**批次——下一批必须照常跑");
        assert_eq!(backend.total_executed(), 10);
    }

    /// 时间驱动路径（`continue_until_event`）也认暂停。
    ///
    /// 两条路径共用**同一个**执行器，但暂停标志是**后端**上的一个字段
    /// ——若只在一侧查，时间驱动路径就会**无视**暂停。
    #[test]
    fn pause_stops_the_time_driven_path_too() {
        let mut backend = backend_with_demo();
        backend.pause().expect("pause");

        backend.continue_until_event().expect("run until event");

        assert_eq!(
            backend.last_stop_reason(),
            Some(HaltReason::UserPause),
            "时间驱动路径也必须在指令边界认暂停"
        );
        assert_eq!(backend.total_executed(), 0, "被暂停的那条指令**没有执行**");
    }

    /// `reset()` 把暂停请求一起清掉。
    ///
    /// 暂停请求属于**上一次运行**的残留 —— reset 之后不该还有效，否则
    /// reset 之后的第一批会莫名其妙地零条停下。
    #[test]
    fn a_reset_clears_a_pending_pause() {
        let mut backend = backend_with_demo();
        backend.pause().expect("pause");
        backend.reset().expect("reset");

        let executed = backend.step_n(3).expect("step");
        assert_eq!(executed, 3, "reset 之后不该还有效的暂停请求");
    }

    // ------------------------------------------------------------------
    // 中断接线（STAGE-4 接缝 #2 / #3 / #4）
    // ------------------------------------------------------------------

    /// 固件里 handler 的地址（= `mtvec`）。
    const IRQ_HANDLER_ADDR: u32 = 0x20;

    /// 固件里主循环的地址（= 陷入时的 `pc`，也是 `mepc`）。
    const IRQ_MAIN_LOOP_ADDR: u32 = 0x18;

    /// 固件里那条「handler 计数器」用的寄存器。
    const IRQ_HANDLER_COUNTER_REG: u32 = 7;

    /// 固件清的那条线（line 1 = UART rx-ready）。
    const IRQ_LINE: u16 = 1;

    /// 造一个**带中断控制器**的后端 + 控制器本身的共享句柄。
    ///
    /// 返回控制器句柄是**必要的**：验收要求断言「挂起项被清 / `active_irq`
    /// 复位」——那是**控制器导出的状态**，不是「我调用了某个函数」。
    fn backend_with_irq(
        program: &[u8],
    ) -> (
        SelfIssBackend<NullSinks, NullSinks>,
        Rc<RefCell<InterruptController>>,
    ) {
        let controller = Rc::new(RefCell::new(InterruptController::new()));
        let mut backend = backend_with_program(program);
        backend.install_interrupt_port(Box::new(SharedInterruptPort::new(Rc::clone(&controller))));
        (backend, controller)
    }

    /// 一个量子能跑多少条指令（= 量子长度 ÷ 单条指令成本）。
    fn instructions_per_quantum() -> u32 {
        (crate::m12_engine::TIME_QUANTUM_PS / crate::m12_engine::INSTRUCTION_COST_PS) as u32
    }

    /// 读 guest 的一个 CSR（**唯一**入口是 M-13 的 `csr_read`）。
    fn guest_csr(backend: &SelfIssBackend<NullSinks, NullSinks>, csr: u16) -> u32 {
        let state = backend.snapshot().expect("snapshot");
        crate::m13_iss::csr::csr_read(&state, csr)
    }

    /// **量子边界**才仲裁（验收 26）。
    ///
    /// 判别式落在**输出**上：量子边界之前 `mcause` **没有**被写（没有陷入），
    /// 跨过边界之后它才出现。
    ///
    /// ⚠️ 若采样改成「每条指令一次」，第一条指令就会交付中断 ⇒
    /// 「边界之前没有陷入」这条断言当场变红。
    #[test]
    fn an_interrupt_is_delivered_only_at_a_quantum_boundary() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");

        let per_quantum = instructions_per_quantum();
        backend.step_n(per_quantum).expect("step");
        assert_eq!(
            guest_csr(&backend, CSR_MCAUSE),
            0,
            "量子边界**之前**不许仲裁 —— 一条指令执行中间不仲裁"
        );

        backend.step_n(1).expect("step");
        assert_eq!(
            guest_csr(&backend, CSR_MCAUSE),
            riscv_sim_contract::csr::IRQ_BASE | u32::from(IRQ_LINE),
            "跨过量子边界就必须交付"
        );
    }

    /// 中断交付 ⇒ `pc` 跳到 `mtvec`，`mcause` = `IRQ_BASE | line`，
    /// `mepc` = 被中断处（验收 27）。
    #[test]
    fn the_delivery_jumps_to_mtvec_and_records_the_cause() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");

        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");

        let state = backend.snapshot().expect("snapshot");
        assert_eq!(
            state.pc, IRQ_HANDLER_ADDR,
            "交付之后 PC 必须跳到 mtvec（直接模式）"
        );
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, CSR_MCAUSE),
            riscv_sim_contract::csr::IRQ_BASE | u32::from(IRQ_LINE),
            "mcause 必须是 `IRQ_BASE | line`"
        );
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, CSR_MEPC),
            IRQ_MAIN_LOOP_ADDR,
            "mepc 必须是**被中断处**的 PC"
        );
        // ⚠️ **「值是 0」必须配「这条 CSR 在文件里」** —— `csr_read` 对
        //    「文件里没有的 CSR」也返回 0 ⇒ 单独断言「值是 0」是**真空
        //    断言**（v0.19 检视轮 V-B · F-3）。
        assert!(
            state.csrs.iter().any(|(n, _)| *n == CSR_MTVAL),
            "中断路径必须**真的写**过 mtval（不是「读不到就是 0」的那种假绿）"
        );
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, CSR_MTVAL),
            0,
            "中断没有故障地址 ⇒ mtval = 0"
        );
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, CSR_MTVEC),
            IRQ_HANDLER_ADDR,
            "mtvec 是 guest 自己写的那个值"
        );
    }

    /// 写 `irq_clear` ⇒ 挂起项被清、`active_irq` 复位（验收 28）。
    ///
    /// ⚠️ **判别力**：只断言「CSR 写成功」是**恒真断言**。这里断言的是
    /// **控制器导出的状态**——挂起队列空了、`active_irq` 复位了。
    /// 若 M-17 的副作用没接上，这两条都会红。
    #[test]
    fn writing_irq_clear_clears_the_pending_entry_and_resets_active_irq() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");

        // 交付 + handler（handler 里写 `irq_clear`）。
        backend
            .step_n(instructions_per_quantum() + 4)
            .expect("step");

        let controller = controller.borrow();
        assert!(
            controller.pending_lines().is_empty(),
            "`irq_clear` 写完，挂起队列必须空 —— 实际还挂着 {:?}",
            controller.pending_lines()
        );
        assert_eq!(
            controller.active_irq(),
            riscv_sim_contract::csr::NO_ACTIVE_IRQ,
            "`irq_clear` 写完，active_irq 必须复位"
        );
    }

    /// `MRET` 之后回到 `mepc`，且能**再次**收到同一中断（验收 29）。
    ///
    /// 判别式：handler 的**计数器**（x6）——它数的是 handler 跑了几遍。
    /// 第二次交付要发生，`irq_clear` 的副作用就必须真的把 `active_irq`
    /// 复位了（否则单级嵌套防护会挡住第二次交付）。
    #[test]
    fn the_same_interrupt_is_delivered_again_after_mret() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");

        // 第一遍：交付 → handler → mret 回到主循环。
        backend
            .step_n(instructions_per_quantum() + 4)
            .expect("step");
        let state = backend.snapshot().expect("snapshot");
        assert_eq!(state.pc, IRQ_MAIN_LOOP_ADDR, "mret 之后必须回到 mepc");
        assert_eq!(
            state.gpr[IRQ_HANDLER_COUNTER_REG as usize], 1,
            "handler 必须跑过一遍"
        );

        // 第二遍：再请求一次同一条线 → 下一个量子边界再交付一次。
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request again");
        backend
            .step_n(instructions_per_quantum() + 4)
            .expect("step");

        let state = backend.snapshot().expect("snapshot");
        assert_eq!(
            state.gpr[IRQ_HANDLER_COUNTER_REG as usize], 2,
            "清过之后，同一条中断必须能**再次**被交付"
        );
        assert_eq!(
            crate::m13_iss::csr::csr_read(&state, CSR_MCAUSE),
            riscv_sim_contract::csr::IRQ_BASE | u32::from(IRQ_LINE),
            "第二次交付的 mcause 一样"
        );

        // ⚠️ **断言必须落在「第二次交付之后」的控制器状态上**，不能只数
        //    「handler 跑了几次」—— 那个量在「**第二次清除被跳过**」时
        //    **照样成立**（handler 确实跑了，只是它写的 `irq_clear` 没生效）。
        //
        //    guest 每次 handler 都写**同一个**掩码 ⇒ 「值变了才清」的闸门
        //    会把第二次清除**静默丢弃** ⇒ `active_irq` 不复位 ⇒ M-16 的
        //    单级嵌套防护把这条线**永久堵死**。
        let controller = controller.borrow();
        assert!(
            controller.pending_lines().is_empty(),
            "第二次 handler 写 irq_clear 也必须真的清掉挂起项 —— 实际还挂着 {:?}",
            controller.pending_lines()
        );
        assert_eq!(
            controller.active_irq(),
            riscv_sim_contract::csr::NO_ACTIVE_IRQ,
            "第二次 handler 写 irq_clear 也必须复位 active_irq（否则单级防护把这条线永久堵死）"
        );
    }

    /// 中断交付**不计入**已执行条数（STAGE-4 裁决 I 在 M-17 这一侧）。
    ///
    /// 判别式：`total_executed` 与「真正退休的指令数」对不上——交付那一轮
    /// 没有指令退休。
    #[test]
    fn the_delivery_does_not_count_as_an_executed_instruction() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");

        let per_quantum = instructions_per_quantum();
        let executed = backend.step_n(per_quantum + 1).expect("step");
        assert_eq!(
            executed,
            per_quantum,
            "跑了 {} 步，但**交付**那一轮没有指令退休 ⇒ 执行的条数是 {}，\
             不是 {}",
            per_quantum + 1,
            per_quantum,
            per_quantum + 1
        );
    }

    /// 「上次采样时刻」必须**跨批次**保留（STAGE-4 裁决 I）。
    ///
    /// ⚠️ 判别式落在 M-16 **导出的状态**上（`mtime` = 最近一次采样的
    /// 虚拟时间）。若「上次采样时刻」挂在**执行器**上（执行器是**每批次
    /// 新建**的），下一批的第一条指令就会再采样一次 ⇒ `mtime` 变成那条
    /// 指令的时刻 ⇒ 「仲裁只在量子边界发生」这条不变式当场破掉。
    #[test]
    fn the_quantum_boundary_sampling_survives_across_batches() {
        let (mut backend, controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        let per_quantum = instructions_per_quantum();

        backend.step_n(per_quantum + 1).expect("step");
        let sampled_at = controller.borrow().mtime();
        assert_eq!(
            sampled_at,
            u64::from(per_quantum) * crate::m12_engine::INSTRUCTION_COST_PS,
            "第一次采样必须发生在**第一个量子边界**上"
        );

        // 下一批的第一条指令：离上一个量子边界只有 1 ns ⇒ **不许**再采样。
        backend.step_n(1).expect("step");
        assert_eq!(
            controller.borrow().mtime(),
            sampled_at,
            "「上次采样时刻」必须**跨批次**保留 —— 挂在执行器上会让每个批次的\
             第一条指令都采样一次"
        );
    }

    /// 没装中断控制器时，后端**照常跑**（空端口什么都不做）。
    ///
    /// ⚠️ 这条是**回归护栏**：`install_interrupt_port` 之前的那些用法
    /// （宿主之外的直接调用）不能因为端口字段变成必填就崩掉。
    #[test]
    fn a_backend_without_a_controller_still_runs() {
        let mut backend = backend_with_demo();
        let executed = backend.step_n(3).expect("step");
        assert_eq!(executed, 3);
    }

    // ------------------------------------------------------------------
    // `mip` 投影（D-84 / STAGE-6 裁决 B）
    // ------------------------------------------------------------------

    /// 一个**会写 `mip`** 的中断固件（验收 #5）。
    ///
    /// ```text
    /// 0x00  addi  x5, x0, 0x20    → handler 地址
    /// 0x04  lui   x6, 0x8         → 0x8000
    /// 0x08  srli  x6, x6, 4       → 0x800（`mie` 的 MEIP 位 = 11）
    /// 0x0c  csrrw x0, mtvec, x5
    /// 0x10  csrrw x0, mie, x6
    /// 0x14  csrrsi x0, mstatus, 8 → MIE = 1
    /// 0x18  jal   x0, 0           → 主循环自旋
    /// 0x1c  nop
    /// 0x20  csrrwi x0, mip, 31    → handler：**写 `mip`**（必须被忽略）
    /// 0x24  jal   x0, 0           → 自旋（**不清线** ⇒ 线一直挂起）
    /// ```
    const MIP_WRITE_FIRMWARE: [u8; 40] = [
        0x93, 0x02, 0x00, 0x02, // 0x00 addi  x5, x0, 0x20
        0x37, 0x83, 0x00, 0x00, // 0x04 lui   x6, 0x8
        0x13, 0x53, 0x43, 0x00, // 0x08 srli  x6, x6, 4
        0x73, 0x90, 0x52, 0x30, // 0x0c csrrw x0, mtvec, x5
        0x73, 0x10, 0x43, 0x30, // 0x10 csrrw x0, mie, x6
        0x73, 0x60, 0x04, 0x30, // 0x14 csrrsi x0, mstatus, 8
        0x6f, 0x00, 0x00, 0x00, // 0x18 jal   x0, 0
        0x13, 0x00, 0x00, 0x00, // 0x1c nop
        0x73, 0xd0, 0x4f, 0x34, // 0x20 csrrwi x0, mip, 31
        0x6f, 0x00, 0x00, 0x00, // 0x24 jal   x0, 0
    ];

    /// 造一个后端，**挂起**一条线，但**先不**跑任何指令。
    fn backend_with_a_pending_line(
        program: &[u8],
    ) -> (
        SelfIssBackend<NullSinks, NullSinks>,
        Rc<RefCell<InterruptController>>,
    ) {
        let (backend, controller) = backend_with_irq(program);
        controller
            .borrow_mut()
            .request(IRQ_LINE, 0, SOURCE_PLATFORM, TRIGGER_EDGE)
            .expect("request");
        (backend, controller)
    }

    /// ⚠️ **挂起的线必须在 `mip` 里看得见**（验收 #1）。
    ///
    /// 判别式落在**输出**上：guest 读 CSR `mip` 的值。
    ///
    /// ⚠️ 断言的是**位**而不是「`mip` 非零」：非零这条断言太弱
    /// （任何一位被置上都能通过）⇒ 换成「**那条线的位**被置上」。
    #[test]
    fn a_pending_line_shows_up_in_mip() {
        let (mut backend, _controller) = backend_with_a_pending_line(&IRQ_FIRMWARE_BYTES);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");

        let mip = guest_csr(&backend, CSR_MIP);
        let bit = std_irq_bit_for_line(IRQ_LINE).expect("线 1 有标准中断位");
        assert_ne!(
            mip & (1 << bit),
            0,
            "挂起的线必须在 mip 里看得见 —— 实得 {mip:#x}（期望位 {bit}）"
        );
        // ⚠️ **必须落在 CSR 文件里** —— `csr_read` 对「文件里没有的 CSR」
        //    也返回 0 ⇒ 单独断言「值非零」不够，这里再钉住「文件里有」。
        let state = backend.snapshot().expect("snapshot");
        assert!(
            state.csrs.iter().any(|(number, _)| *number == CSR_MIP),
            "mip 必须落进 CSR 文件 —— 实得 {:?}",
            state.csrs
        );
    }

    /// ⚠️ **`mip` 与 `mie` 用的是**同一个位号**（验收 #2 / 裁决 C）。
    ///
    /// RISC-V 里 `MEIE` / `MEIP` 是**同一个 bit** ⇒ 同一条线在两个寄存器
    /// 里必须落在**同一位**上。
    ///
    /// 固件写的 `mie` = `0x800`（位 11）⇒ `mip` 也必须是 `0x800`。
    /// 若位号表写错（例如 UART 线映到位 7），这条立刻变红。
    #[test]
    fn the_mip_bit_is_the_same_bit_the_mie_bit_uses() {
        let (mut backend, _controller) = backend_with_a_pending_line(&IRQ_FIRMWARE_BYTES);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");

        let mie = guest_csr(&backend, CSR_MIE);
        let mip = guest_csr(&backend, CSR_MIP);
        assert_eq!(mie, 1 << 11, "固件写的是 `mie` 的 MEIP 位（11）");
        assert_eq!(mip, mie, "同一条线的 `mip` 位号必须与 `mie` 位号相同");
    }

    /// ⚠️ **没有挂起线 ⇒ CSR 文件里**不得**多出 `mip` 条目**（验收 #4）。
    ///
    /// 投影是「**值变化才写**」⇒ 没有挂起线时 `mip` 恒为 0 ⇒ 平台
    /// **一条都不写** ⇒ CSR 文件保持原样（保住会话快照的「空」判定）。
    #[test]
    fn a_guest_without_a_pending_line_has_no_mip_entry() {
        let (mut backend, _controller) = backend_with_irq(&IRQ_FIRMWARE_BYTES);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");

        let state = backend.snapshot().expect("snapshot");
        assert!(
            !state.csrs.iter().any(|(number, _)| *number == CSR_MIP),
            "没有挂起线时 CSR 文件不得多出 mip 条目 —— 实得 {:?}",
            state.csrs
        );
    }

    /// ⚠️ **guest 写 `mip` 无效**（验收 #5）。
    ///
    /// RISC-V 里 `MEIP` / `MTIP` 在 **M 模式只读** ⇒ guest 不能靠写
    /// `mip` 伪造一个「挂起中断」骗自己。
    ///
    /// ⚠️ 断言的**时机**很重要：必须在「写 `mip` 之后、下一个量子边界
    /// 之前」断言 —— 再往后平台的投影会**把值刷回去**，那就能把一个
    /// 「写被放行了」的缺陷**掩盖掉**。
    #[test]
    fn a_guest_write_to_mip_is_ignored() {
        let (mut backend, _controller) = backend_with_a_pending_line(&MIP_WRITE_FIRMWARE);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");
        let before = guest_csr(&backend, CSR_MIP);
        assert_eq!(before, 1 << 11, "前置条件：投影已经落地");

        // handler 的第一条指令 = `csrrwi x0, mip, 31`。
        backend.step_n(1).expect("step");
        let after = guest_csr(&backend, CSR_MIP);
        assert_eq!(
            after, before,
            "guest 写 mip 必须无效 —— 实得 {after:#x}（写前 {before:#x}）"
        );
    }

    /// ⚠️ **guest 清掉线 ⇒ `mip` 里的那位**立刻**消失**（刷新点 ②）。
    ///
    /// 刷新点 ② 在 `irq_clear` 副作用**之后** ⇒ 清线的**同一个指令
    /// 边界**上，`mip` 就已经反映了「线没了」。
    #[test]
    fn clearing_the_line_takes_its_bit_out_of_mip() {
        let (mut backend, _controller) = backend_with_a_pending_line(&IRQ_FIRMWARE_BYTES);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");
        assert_eq!(guest_csr(&backend, CSR_MIP), 1 << 11, "前置条件：线已挂起");

        // handler：`addi x7,x7,1` → `csrrwi x0, irq_clear, 2` → `mret`
        backend.step_n(2).expect("step");
        assert_eq!(
            guest_csr(&backend, CSR_MIP),
            0,
            "线被清掉后 mip 的那一位必须消失（刷新点 ②）"
        );
    }

    /// ⚠️ **读不到挂起线 ⇒ `mip` 不被写**（台账 **D-100** · 验收 **C-3**）。
    ///
    /// `pending_lines()` 在共享适配器**借不到**控制器时返回 `None`
    /// （**状态未知**）。那一刻**绝不能**把「读不到」当成「没有挂起」
    /// ⇒ 算出的掩码是 0 ⇒ **一次失败的读**变成了 guest 看得见的状态
    /// 改写（`mip` 里的挂起位被抹掉）。
    ///
    /// ⚠️ **判别式**：先把 `mip` 弄成**非 0**（线 1 挂起 ⇒ `0x800`），
    /// 再造争用，然后让 guest 走一次刷新点 ②（`irq_clear` 的副作用）——
    /// 那一次刷新**读不到**挂起线 ⇒ `mip` 必须**保持原样**。
    ///
    /// ⚠️ **断言必须落在「值」上**：断言「`mip` 条目还在」不够（写 0
    /// 也会留下条目）。
    #[test]
    fn a_contended_port_does_not_rewrite_mip() {
        let (mut backend, controller) = backend_with_a_pending_line(&IRQ_FIRMWARE_BYTES);
        backend
            .step_n(instructions_per_quantum() + 1)
            .expect("step");
        // 前置条件：量子边界的刷新点 ① 已经把挂起线投影进 `mip`。
        assert_eq!(
            guest_csr(&backend, CSR_MIP),
            1 << 11,
            "前置条件：线 1 挂起 ⇒ mip 必须是 MEIP 位"
        );

        // ⚠️ **造争用**：借住控制器不放 ⇒ 端口的 `try_borrow` 失败 ⇒
        // `pending_lines()` 返回 `None`（状态未知）。
        let guard = controller.borrow_mut();
        // handler 的两条指令：`addi x7,x7,1` → `csrrwi x0, irq_clear, 2`
        // （第二条触发刷新点 ②）。
        backend.step_n(2).expect("step");
        drop(guard);

        assert_eq!(
            guest_csr(&backend, CSR_MIP),
            1 << 11,
            "读不到挂起线时 `mip` **不得**被改写（读失败 ≠ 没有挂起）"
        );
    }
}
