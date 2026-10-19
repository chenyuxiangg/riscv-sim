//! M-15 外设模型层 (Peripheral Model Layer).
//!
//! Adopt the "tiered peripheral" pattern (AD §4.3, M-15 v1.2):
//! - Each peripheral implements the [`Peripheral`] port from
//!   `riscv_sim_contract::port`.
//! - MVP ships three behaviour-level peripherals:
//!   [`Uart16550`], [`GpioBank`], [`TimerRiscv`].
//! - [`PeripheralRegistry`] is the registry that the kernel can
//!   consult at attach / map-peripheral time. Today the registry is a
//!   `BTreeMap<id, Box<dyn Peripheral>>` keyed by the string id
//!   returned by `Peripheral::id()`. Wiring into M-14 / m17_backend is
//!   **not yet done** (see PROGRESS.md §6.3 `M-15 Bus 接线下一 PR`).
//!
//! Fidelity metadata ([`FidelityLevel`], per M-15 §2.1.1) is exposed
//! via [`PeripheralRegistry::describe`] for the front-end display
//! (评审 R5). All MVP peripherals are fidelity level = 1 (行为级).

use riscv_sim_contract::error::ErrorFamily;
use std::collections::BTreeMap;

pub mod gpio;
pub mod reg;
pub mod timer;
pub mod uart;

pub use gpio::GpioBank;
pub use timer::TimerRiscv;
pub use uart::Uart16550;
// M-15 review #10: re-export `Peripheral` directly so external code
// can write `m15_peripheral::Peripheral` (the previous `PeripheralTrait`
// alias forced callers back to `riscv_sim_contract::port::Peripheral`).
pub use riscv_sim_contract::port::Peripheral;

/// Fidelity level (M-15 §2.1.1) — coarse granularity for the
/// front-end "Fidelity" column (评审 R5). M-15 review #17: this is
/// the real carrier behind `describe()`; was previously a stub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelityLevel {
    /// Behavioural: state-machine only, no timing.
    Behavioural = 1,
    /// Transactional: behavioural + bus-cycle accurate.
    Transactional = 2,
    /// Timing-approximate: transactional + bounded timing accuracy.
    TimingApprox = 3,
    /// Cycle-accurate: pin/cycle level.
    CycleAccurate = 4,
}

impl FidelityLevel {
    /// One-line label used by the front-end (M-15 review #17).
    pub fn label(self) -> &'static str {
        match self {
            FidelityLevel::Behavioural => "behavioural-level",
            FidelityLevel::Transactional => "transactional-level",
            FidelityLevel::TimingApprox => "timing-approx-level",
            FidelityLevel::CycleAccurate => "cycle-accurate-level",
        }
    }
}

/// Peripheral registry — keyed by the stable id returned by
/// [`Peripheral::id`]. The kernel can attach any registered instance
/// to the bus at attach time.
#[derive(Default)]
pub struct PeripheralRegistry {
    by_id: BTreeMap<String, Box<dyn Peripheral>>,
}

/// Manual `Debug` impl because `Box<dyn Peripheral>` has no `Debug`
/// bound.
impl std::fmt::Debug for PeripheralRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeripheralRegistry")
            .field("count", &self.by_id.len())
            .field("ids", &self.by_id.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PeripheralRegistry {
    /// New empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a peripheral. Returns `Err(RegisterError::Duplicate)`
    /// if a peripheral with the same id is already registered
    /// (M-15 review #9: previously silently replaced the existing
    /// entry).
    pub fn register<P: Peripheral + 'static>(&mut self, p: P) -> Result<&mut Self, RegisterError> {
        let id = p.id().to_string();
        if self.by_id.contains_key(&id) {
            return Err(RegisterError::Duplicate(id));
        }
        let _ = self.by_id.insert(id, Box::new(p));
        Ok(self)
    }

    /// Look up by id.
    pub fn get(&self, id: &str) -> Option<&dyn Peripheral> {
        self.by_id.get(id).map(|b| &**b)
    }

    /// Look up by id (mut). The lifetime gymnastics pin the returned
    /// borrow to `self`, not to the `BTreeMap` entry.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut (dyn Peripheral + 'static)> {
        self.by_id.get_mut(id).map(move |b| &mut **b)
    }

    /// Iterate all registered peripherals.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Peripheral> {
        self.by_id.values().map(|b| &**b)
    }

    /// Number of registered peripherals.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True when no peripherals are registered.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Compact one-line fidelity description used by the front-end
    /// "Fidelity" column (评审 R5, M-15 review #17).
    pub fn describe(&self, id: &str) -> Option<&'static str> {
        if self.get(id).is_some() {
            Some(FidelityLevel::Behavioural.label())
        } else {
            None
        }
    }
}

/// Error returned by [`PeripheralRegistry::register`] when an id
/// collision is detected (M-15 review #9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A peripheral with this id is already registered.
    Duplicate(String),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::Duplicate(id) => {
                write!(f, "peripheral id '{}' already registered", id)
            }
        }
    }
}

impl std::error::Error for RegisterError {}

/// Family used by errors raised from this module.
pub const PERIPHERAL_FAMILY: ErrorFamily = ErrorFamily::Peripheral;

#[cfg(test)]
mod tests;
