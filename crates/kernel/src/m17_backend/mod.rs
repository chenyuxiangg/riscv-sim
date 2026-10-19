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
