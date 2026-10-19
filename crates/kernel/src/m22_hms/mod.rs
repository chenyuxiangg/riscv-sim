//! M-22 健康管理 HMS (Health Management System).
//!
//! Watches the kernel for runaway conditions (M-12 watchdog
//! timeouts, OOM, deadlocked peripherals) and dispatches degraded
//! states to the right role channel (LLD §4.22).
//!
//! Status: stub — escalation rules are defined; the watchdog
//! integration with M-12 is TODO(MVP).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};

/// Health state, monotonically degraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    /// Everything nominal.
    Ok,
    /// One or more non-fatal anomalies observed.
    Degraded,
    /// Watchdog tripped or irrecoverable; the engine halted.
    Critical,
}

impl Default for Health { fn default() -> Self { Self::Ok } }

/// HMS state. Single-thread, the kernel serializes state updates.
#[derive(Debug, Default)]
pub struct Hms {
    state: Health,
    last_escalation: Option<String>,
}

impl Hms {
    /// New HMS in [`Health::Ok`].
    pub fn new() -> Self { Self::default() }

    /// Current state.
    pub fn state(&self) -> Health { self.state }

    /// Report an anomaly. The HMS picks the right escalation per the
    /// LLD §4.22.2 rules.
    pub fn report(&mut self, kind: &str) -> Result<Health, PlatformError> {
        let next = match self.state {
            Health::Ok       => Health::Degraded,
            Health::Degraded => Health::Critical,
            Health::Critical => Health::Critical,
        };
        if next != self.state {
            self.last_escalation = Some(kind.to_string());
            self.state = next;
        }
        Ok(next)
    }
}

/// Family used by errors raised from this module.
pub const HMS_FAMILY: ErrorFamily = ErrorFamily::Health;
