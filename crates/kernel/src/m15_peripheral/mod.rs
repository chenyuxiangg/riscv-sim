//! M-15 外设模型层 (Peripheral Model Layer).
//!
//! Adopts the "tiered peripheral" pattern (AD §4.3, LLD §4.15):
//! - Each peripheral implements the [`Peripheral`] port from
//!   `riscv_sim_contract::port`.
//! - MVP ships with three behaviour-level peripherals: UART0,
//!   GPIO0, TIM0.
//! - More accurate models can be plugged in later (scripted / HDL
//!   co-sim) without touching the engine.
//!
//! Status: stub — port adopted; no concrete peripherals yet.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::WriteSize;
use riscv_sim_contract::port::{InterruptSink, Peripheral};
use std::collections::BTreeMap;

/// Peripheral registry — keyed by stable id.
#[derive(Default)]
pub struct PeripheralRegistry {
    by_id: BTreeMap<String, Box<dyn Peripheral>>,
}

// Manual `Debug` so we don't have to require every `Peripheral` impl to be
// `Debug` (the trait object can't carry that bound).
impl std::fmt::Debug for PeripheralRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeripheralRegistry")
            .field("count", &self.by_id.len())
            .finish()
    }
}

impl PeripheralRegistry {
    /// New empty registry.
    pub fn new() -> Self { Self::default() }

    /// Register a peripheral.
    pub fn register<P: Peripheral + 'static>(&mut self, p: P) {
        let _ = self.by_id.insert(p.id().to_string(), Box::new(p));
    }

    /// Look up by id.
    pub fn get(&self, id: &str) -> Option<&dyn Peripheral> { self.by_id.get(id).map(|b| &**b) }

    /// Look up by id (mut).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut (dyn Peripheral + 'static)> {
        // The `move` makes the closure take ownership of the `&mut Box`
        // so the returned `&mut dyn Peripheral` can be tied to the
        // outer function lifetime, not to the `Option`'s borrow.
        self.by_id.get_mut(id).map(move |b| &mut **b)
    }

    /// Iterate all registered peripherals.
    pub fn iter(&self) -> impl Iterator<Item = &dyn Peripheral> { self.by_id.values().map(|b| &**b) }
}

/// A trivial UART stub used to exercise the port shape. Real UART
/// semantics (FIFO, baud rate, line discipline) are out of MVP scope.
#[derive(Debug)]
pub struct DummyUart {
    base: u32,
    id: String,
}

impl DummyUart {
    /// New dummy UART at `base`.
    pub fn new(id: impl Into<String>, base: u32) -> Self {
        Self { base, id: id.into() }
    }
}

impl Peripheral for DummyUart {
    fn id(&self) -> &str { &self.id }
    fn base_addr(&self) -> u32 { self.base }
    fn size(&self) -> u32 { 0x1000 }
    fn read(&self, _offset: u32, _size: WriteSize) -> Result<u32, PlatformError> { Ok(0) }
    fn write(&mut self, _offset: u32, _size: WriteSize, _value: u32) -> Result<(), PlatformError> { Ok(()) }
    fn tick(&mut self, _now_ps: u64) -> Result<Vec<riscv_sim_contract::model::PendingWrite>, PlatformError> { Ok(Vec::new()) }
}

/// Helper that lets a peripheral push IRQs back into the engine.
pub fn raise_irq<S: InterruptSink + ?Sized>(sink: &mut S, irq: u16) -> Result<(), PlatformError> {
    sink.raise(irq)
}

/// Family used by errors raised from this module.
pub const PERIPHERAL_FAMILY: ErrorFamily = ErrorFamily::Peripheral;
