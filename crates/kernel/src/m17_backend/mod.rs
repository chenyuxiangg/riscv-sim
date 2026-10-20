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

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{Breakpoint, SimState, WatchAccess, Watchpoint, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::HaltReason;
use riscv_sim_contract::snapshot::EngineSnapshotDto;

use crate::m08_breakpoint::{BreakpointTable, WatchpointTable};
use crate::m12_engine::Engine;
use crate::m13_iss::{Iss, StepOutcome};
use crate::m14_bus::{AccessDir, Bus, BusWidth};

/// 一条指令的虚拟时间成本（皮秒）。
///
/// 第 1 阶段没有真正的流水线模型，每条指令按 **1 ns** 计——与前端的
/// mock 内核同口径，前端时间轴不需要因为换内核而重标定。
pub const INSTRUCTION_COST_PS: u64 = 1_000;

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

    /// Replace the memory bus with a **fresh, empty** one.
    ///
    /// A new project is a new session (FR-13): the old memory image does
    /// not carry over. The watchpoint hook is re-installed because it
    /// lives **in** the bus.
    pub fn reset_memory_map(&mut self) {
        self.bus = Bus::new();
        self.install_watch_hook();
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
        self.bus
            .set_hook(Some(Box::new(move |access: &crate::m14_bus::BusAccess| {
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
            })));
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

        for _ in 0..n {
            // (1) 断点命中检测 —— 挂在**取指之前**（M-08 LLD §3.2）。
            //     命中时该条指令尚未执行，状态零副作用。
            let pc = engine.pc();
            if breakpoints.hit_check(pc).is_some() {
                *last_stop = Some(HaltReason::Breakpoint);
                break;
            }

            // (2) 执行一条指令。
            let outcome = {
                let state = engine.state_mut();
                iss.step_one_via_bus(state, bus)
            };
            match outcome {
                Ok(StepOutcome::Advanced) => {
                    executed += 1;
                    *total_executed += 1;
                    engine.advance_virtual_time(INSTRUCTION_COST_PS);
                }
                Ok(StepOutcome::Trapped { .. }) => {
                    // 陷入的指令也算「执行了」——它被取指、被译码、被触发。
                    executed += 1;
                    *total_executed += 1;
                    *last_stop = Some(HaltReason::Trap);
                    break;
                }
                Ok(StepOutcome::Breakpoint) => {
                    *last_stop = Some(HaltReason::Breakpoint);
                    break;
                }
                Ok(StepOutcome::Watchpoint) => {
                    *last_stop = Some(HaltReason::Watchpoint);
                    break;
                }
                Ok(StepOutcome::HostPause) => {
                    *last_stop = Some(HaltReason::UserPause);
                    break;
                }
                Err(err) => {
                    // 取指 / 访存失败（E_BUS-01 等）——按陷阱处理。
                    *last_stop = Some(HaltReason::Trap);
                    return Err(err);
                }
            }

            // (3) 观察点命中 —— 访问发生在指令**内部**，所以只能在
            //     指令返回后判定。命中槽在这里**消费**掉：一次命中
            //     不能在下一批里继续报。
            if let Some(wp) = watch_hit.borrow_mut().take() {
                *last_watchpoint = Some(wp);
                *last_stop = Some(HaltReason::Watchpoint);
                break;
            }
        }
        Ok(executed)
    }

    fn continue_until_event(&mut self) -> Result<(), PlatformError> {
        // **时间驱动**：推进 M-12 调度器（外设 tick / 定时器到期 / 量子
        // 边界），把到期的事件派发掉。
        //
        // 它**不**执行 guest 指令 —— 那是 `step_n` 的职责。契约把两者
        // 分开正是为了这个（M-17 LLD 评审 17-1「step 与 runUntil 语义
        // 分离」）：指令驱动与时间驱动是两条不同的轴。
        //
        // 第 1 阶段事件队列恒空（没有外设 / 定时器接线），所以这里等价于
        // 「没有可派发的事件」；宿主侧的「继续运行」由 `step_n` + 指令
        // 预算实现（见 `kernel_host::KernelHost::on_run`）。
        self.engine.run_until_idle().map(|_| ())
    }

    fn pause(&mut self) -> Result<(), PlatformError> {
        self.last_stop = Some(HaltReason::UserPause);
        Ok(())
    }

    fn snapshot(&self) -> Result<SimState, PlatformError> {
        Ok(self.engine.snapshot())
    }

    fn restore(&mut self, state: &SimState) -> Result<(), PlatformError> {
        self.engine.restore(state.clone());
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
        self.engine.restore_dto(snapshot)
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
    use riscv_sim_contract::model::Breakpoint;
    use riscv_sim_contract::protocol::EventPayload;

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
    fn continue_until_event_does_not_execute_guest_instructions() {
        // 回归 V-A 🟡-3：`continue_until_event` 是**时间驱动**（推进 M-12
        // 调度器），**不执行** guest 指令（M-17 LLD 评审 17-1「step 与
        // runUntil 语义分离」）。把一条 guest 指令塞进去也不该有测试变红 ——
        // 这条用例把它钉住。
        let mut backend = backend_with_demo();
        let pc_before = backend.snapshot().expect("snapshot").pc;
        backend.continue_until_event().expect("run until event");
        assert_eq!(backend.total_executed(), 0, "runUntil 不得执行 guest 指令");
        assert_eq!(
            backend.snapshot().expect("snapshot").pc,
            pc_before,
            "runUntil 不得推进 PC"
        );
    }
}
