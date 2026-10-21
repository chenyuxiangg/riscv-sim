//! M-16 中断控制器 (Interrupt Controller).
//!
//! Owns the RISC-V `mip` / `mie` / `mcause` / `me pc` CSRs and the
//! platform-level interrupt router. The scheduler samples pending
//! interrupts at every quantum boundary (1µs by default).
//!
//! Status: stub — port adopted; no CSR plumbing yet.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::port::InterruptSink;
use riscv_sim_contract::session::M16IntcDto;
use std::collections::BTreeSet;

/// Pending interrupt line state. Edge vs level is decided per
/// peripheral; the controller is line-state-only.
#[derive(Debug, Default)]
pub struct InterruptController {
    raised: BTreeSet<u16>,
}

impl InterruptController {
    /// New empty controller.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drain pending IRQs that the engine can sample at the next quantum.
    pub fn drain_pending(&mut self) -> Vec<u16> {
        self.raised.iter().copied().collect()
    }

    /// Export the **raised IRQ lines** — segment 5 of the session
    /// payload (STAGE-3 §3.4).
    ///
    /// The lines come out **ascending and deduplicated** — the
    /// controller's state is a [`BTreeSet`], so the order is canonical
    /// by construction, not by a post-hoc sort.
    ///
    /// ⚠️ This segment is **empty in the current tree**: the CSR
    /// plumbing (`mip` / `mie` / `mcause` / `mepc`) is not wired yet,
    /// so nothing raises a line in production. That is an honest
    /// reflection of the current state, not a defect (STAGE-3 §1.2
    /// 裁决 C).
    pub fn export_state(&self) -> M16IntcDto {
        M16IntcDto {
            raised: self.raised.iter().copied().collect(),
        }
    }

    /// Restore the **raised IRQ lines** — segment 5's consumer
    /// (STAGE-3 §3.4).
    ///
    /// The restored set **replaces** the current one: the payload
    /// *describes* the interrupt state, so the controller must end up
    /// in exactly that state — a line that was raised before the
    /// snapshot and is not in the payload must not survive the
    /// restore.
    ///
    /// The set keeps the payload **canonical**: duplicates collapse and
    /// the order is ascending, whatever order the payload used. (The
    /// payload's own shape — ascending, no duplicates — is checked by
    /// the contract's [`SessionSnapshotDto::validate`](riscv_sim_contract::session::SessionSnapshotDto::validate),
    /// which rejects a malformed payload before it is applied.)
    pub fn load_state(&mut self, state: &M16IntcDto) {
        self.raised = state.raised.iter().copied().collect();
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

#[cfg(test)]
mod tests;
