//! M-15 外设模型层 (Peripheral Model Layer).
//!
//! Adopt the "tiered peripheral" pattern (AD §4.3, M-15 v1.2):
//! - Each peripheral implements the [`Peripheral`] port from
//!   `riscv_sim_contract::port`.
//! - MVP ships three behaviour-level peripherals:
//!   [`Uart16550`], [`GpioBank`], [`TimerRiscv`].
//! - [`PeripheralRegistry`] is the registry that the kernel can
//!   consult at attach / map-peripheral time. The registry owns the
//!   instances and hands out [`SharedPeripheral`] handles
//!   (`BTreeMap<id, Rc<RefCell<dyn Peripheral>>>` keyed by the string id
//!   returned by `Peripheral::id()`). The handle is what M-14 keeps
//!   when a peripheral is mapped onto the bus (D-12) — the registry
//!   stays the owner, so the engine can still `tick` the instance and
//!   inspect its state.
//! - The three MVP peripherals override `Peripheral::on_probe` and
//!   report their state changes through the contract's `ProbeSink`
//!   (D-12 / N5): [`GpioBank`] reports `"OUT"`, [`Uart16550`] reports
//!   `"TX"`, [`TimerRiscv`] reports `"CMP"`. The signal names live
//!   next to the peripherals as `SIGNAL_*` constants so the M-18
//!   channel binding and the peripheral can never drift apart.
//!
//! Fidelity metadata ([`FidelityLevel`], per M-15 §2.1.1) is exposed
//! via [`PeripheralRegistry::describe`] for the front-end display
//! (评审 R5). All MVP peripherals are fidelity level = 1 (行为级).

use riscv_sim_contract::error::ErrorFamily;
use riscv_sim_contract::port::ProbeSink;
use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

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

/// A shared handle to a peripheral instance.
///
/// M-14 (the bus) dispatches guest accesses into the instance while the
/// engine keeps a handle for `tick` / state inspection (M-14 LLD §3.2:
/// "命中外设转调 M-15 的 Peripheral.read/write" — the *instance* lives in
/// M-15, the bus only holds a reference). A shared handle is the Rust
/// spelling of that ownership split; the MVP is single-threaded, so
/// `Rc` (not `Arc`) is the right primitive — same posture as M-18's
/// `Rc<RefCell<ProbeRegistry>>`.
pub type SharedPeripheral = Rc<RefCell<dyn Peripheral>>;

/// Wrap a peripheral instance into a [`SharedPeripheral`] handle.
pub fn share<P: Peripheral + 'static>(p: P) -> SharedPeripheral {
    Rc::new(RefCell::new(p))
}

/// Holder for the M-18 probe callback of a peripheral (D-12).
///
/// The contract's `ProbeSink` is a bare closure with no `Debug` bound,
/// so the peripherals keep it in this wrapper to preserve their derived
/// `Debug` impls. `ProbeSlot` is the single place where "a peripheral
/// reports a signal observation" is implemented.
#[derive(Default)]
pub(crate) struct ProbeSlot(Option<ProbeSink>);

impl ProbeSlot {
    /// Install / replace the callback (`None` detaches it).
    pub(crate) fn set(&mut self, cb: Option<ProbeSink>) {
        self.0 = cb;
    }

    /// Report one observation: `(signal, value, now_ps)`.
    ///
    /// A detached slot is a no-op — an unobserved peripheral must not
    /// pay for the probe path.
    pub(crate) fn report(&mut self, signal: &str, value: u64, now_ps: u64) {
        if let Some(sink) = self.0.as_mut() {
            sink(signal, value, now_ps);
        }
    }
}

impl std::fmt::Debug for ProbeSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProbeSlot")
            .field("attached", &self.0.is_some())
            .finish()
    }
}

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
///
/// The registry **owns** the instances and hands out [`SharedPeripheral`]
/// handles (D-12): the bus dispatches guest accesses into the same
/// instance the engine can still `tick` and inspect.
#[derive(Default)]
pub struct PeripheralRegistry {
    by_id: BTreeMap<String, SharedPeripheral>,
}

/// Manual `Debug` impl because `SharedPeripheral` has no `Debug` bound.
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
        let _ = self.by_id.insert(id, share(p));
        Ok(self)
    }

    /// Look up by id.
    pub fn get(&self, id: &str) -> Option<Ref<'_, dyn Peripheral + 'static>> {
        self.by_id.get(id).map(|b| b.borrow())
    }

    /// Look up by id (mut). The lifetime gymnastics pin the returned
    /// borrow to `self`, not to the `BTreeMap` entry.
    pub fn get_mut(&mut self, id: &str) -> Option<RefMut<'_, dyn Peripheral + 'static>> {
        self.by_id.get_mut(id).map(move |b| b.borrow_mut())
    }

    /// The **shared handle** to a registered peripheral.
    ///
    /// This is what the bus needs to dispatch guest accesses into the
    /// instance (D-12): the registry stays the owner, the bus holds a
    /// handle to the very same object.
    pub fn handle(&self, id: &str) -> Option<SharedPeripheral> {
        self.by_id.get(id).cloned()
    }

    /// Iterate all registered peripherals.
    pub fn iter(&self) -> impl Iterator<Item = Ref<'_, dyn Peripheral + 'static>> {
        self.by_id.values().map(|b| b.borrow())
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
