//! M-16 中断控制器 (Interrupt Controller).
//!
//! Owns the RISC-V `mip` / `mie` / `mcause` / `me pc` CSRs and the
//! platform-level interrupt router. The scheduler samples pending
//! interrupts at every quantum boundary (1µs by default).
//!
//! Status: stub — port adopted; no CSR plumbing yet.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::port::InterruptSink;
use std::collections::BTreeSet;

/// Pending interrupt line state. Edge vs level is decided per
/// peripheral; the controller is line-state-only.
#[derive(Debug, Default)]
pub struct InterruptController {
    raised: BTreeSet<u16>,
}

impl InterruptController {
    /// New empty controller.
    pub fn new() -> Self { Self::default() }

    /// Drain pending IRQs that the engine can sample at the next quantum.
    pub fn drain_pending(&mut self) -> Vec<u16> {
        self.raised.iter().copied().collect()
    }
}

impl InterruptSink for InterruptController {
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError> {
        let _ = self.raised.insert(irq);
        Ok(())
    }
    fn lower(&mut self, irq: u16) -> Result<(), PlatformError> {
        let _ = self.raised.remove(&irq);
        Ok(())
    }
}

/// Family used by errors raised from this module.
pub const INT_FAMILY: ErrorFamily = ErrorFamily::Interrupt;
