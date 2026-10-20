//! M-17 ExecutionBackend 执行后端 (Execution Backend Abstraction).
//!
//! Implements the `ExecutionBackend` port from `riscv_sim_contract`,
//! wrapping the M-12 engine and M-13 ISS behind a single object
//! that the debug services (M-08, M-09, M-10) talk to.
//!
//! A second implementation (QEMU via GDB RSP) is reserved for P1
//! and will live in the same crate but be feature-gated.
//!
//! Status: stub — port adapted; debug hookup is TODO(MVP).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::snapshot::EngineSnapshotDto;

use crate::m12_engine::Engine;
use crate::m13_iss::Iss;

/// The default backend: the in-tree RV32IMC ISS behind the M-12
/// scheduler. `B` is the event bus the engine publishes ticks on; `I`
/// is the interrupt sink peripherals raise IRQs through.
pub struct SelfIssBackend<B: EventBus, I: InterruptSink> {
    state: SimState,
    /// Held so the [`ExecutionBackend::step_n`] body can wire it up to
    /// the engine's instruction-budget loop in a later milestone. MVP
    /// does not execute instructions through this path yet.
    #[allow(dead_code)]
    iss: Iss,
    engine: Engine<B, I>,
}

impl<B: EventBus, I: InterruptSink> SelfIssBackend<B, I> {
    /// Build a backend.
    pub fn new(bus: B, intc: I) -> Self {
        Self {
            state: SimState::default(),
            iss: Iss::default(),
            engine: Engine::new(bus, intc),
        }
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
}

impl<B: EventBus, I: InterruptSink> ExecutionBackend for SelfIssBackend<B, I> {
    fn reset(&mut self) -> Result<(), PlatformError> {
        self.state = SimState::default();
        Ok(())
    }
    fn step_n(&mut self, _n: u32) -> Result<u32, PlatformError> {
        // TODO(MVP): drive the engine with the ISS, returning the
        // number of instructions actually executed.
        Ok(0)
    }
    fn continue_until_event(&mut self) -> Result<(), PlatformError> {
        self.engine.run_until_idle().map(|_| ())
    }
    fn pause(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn snapshot(&self) -> Result<SimState, PlatformError> {
        Ok(self.engine.snapshot())
    }
    fn restore(&mut self, state: &SimState) -> Result<(), PlatformError> {
        self.engine.restore(state.clone());
        // Keep the backend's own mirror in step with the engine so the two
        // never disagree about the platform state.
        self.state = self.engine.snapshot();
        Ok(())
    }

    /// Capture a **complete** snapshot: platform state **and** the pending
    /// event queue, in the contract's serializable form.
    ///
    /// Unlike [`ExecutionBackend::snapshot`] (partial), this round-trips
    /// deterministically through [`ExecutionBackend::restore_full`] — the
    /// queue contents, the queue order, the insertion counter and the
    /// virtual time all survive (D-24 / D-05).
    fn snapshot_full(&self) -> Result<Option<EngineSnapshotDto>, PlatformError> {
        Ok(Some(self.engine.snapshot_dto()))
    }

    /// Restore a complete snapshot captured by
    /// [`ExecutionBackend::snapshot_full`].
    ///
    /// The pending event queue is restored **as it was**, so events queued
    /// after the snapshot was taken no longer fire — this is the
    /// deterministic restore that the partial
    /// [`ExecutionBackend::restore`] cannot provide.
    fn restore_full(&mut self, snapshot: &EngineSnapshotDto) -> Result<(), PlatformError> {
        self.engine.restore_dto(snapshot);
        self.state = self.engine.snapshot();
        Ok(())
    }
    fn read_memory(&self, _addr: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        // TODO(MVP): forward to the bus.
        Ok(0)
    }
    fn write_memory(
        &mut self,
        _addr: u32,
        _size: WriteSize,
        _value: u32,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_breakpoint(&mut self, _addr: u32, _enabled: bool) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_watchpoint(
        &mut self,
        _addr: u32,
        _size: WriteSize,
        _write: bool,
        _enabled: bool,
    ) -> Result<(), PlatformError> {
        Ok(())
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
