//! M-08 断点管理器 (Breakpoint Manager).
//!
//! Manages PC breakpoints. The kernel inserts an illegal-instruction
//! trap at the breakpoint address; the M-12 engine translates the
//! trap back into a breakpoint event for the host.
//!
//! TODO(MVP): implement the address ↔ instruction translation cache
//! so the kernel can re-emit the original instruction at the
//! breakpoint after a step-over.
//!
//! Status: stub — public port only.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::Breakpoint;
use std::collections::BTreeMap;

/// Internal breakpoint table.
#[derive(Debug, Default)]
pub struct BreakpointTable {
    inner: BTreeMap<u32, Breakpoint>,
}

impl BreakpointTable {
    /// Create a new empty table.
    pub fn new() -> Self { Self::default() }

    /// Add or update a breakpoint. Stable id is assigned by the host.
    pub fn set(&mut self, bp: Breakpoint) -> Result<(), PlatformError> {
        let _ = self.inner.insert(bp.addr, bp);
        Ok(())
    }

    /// Remove a breakpoint by id. Returns the address it was bound to.
    pub fn clear(&mut self, id: u32) -> Result<Option<u32>, PlatformError> {
        let key = self.inner.iter().find(|(_, v)| v.id == id).map(|(k, _)| *k);
        if let Some(k) = key {
            let _ = self.inner.remove(&k);
            Ok(Some(k))
        } else {
            Ok(None)
        }
    }

    /// Iterate over active breakpoints in address order.
    pub fn iter(&self) -> impl Iterator<Item = &Breakpoint> { self.inner.values() }

    /// Family used by errors raised from this module.
    pub const FAMILY: ErrorFamily = ErrorFamily::Debug;
}
