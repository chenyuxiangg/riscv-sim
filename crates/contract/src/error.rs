//! Error code family + per-subsystem scenario codes (LLD §3.4).
//!
//! Format: `E_<FAMILY>-<SCENARIO>` where family is a 3–4 char subsystem tag
//! and scenario is a 2-digit code. New scenario codes are additive; old
//! codes never change meaning.

use core::fmt;

/// Top-level error type carried over the platform protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError {
    /// Family prefix (e.g. `"CTX"`, `"ISS"`, `"BUS"`, `"PER"`, `"DBG"`).
    pub family: ErrorFamily,
    /// Scenario code within the family. `0` is reserved for "unspecified".
    pub scenario: u16,
    /// Human-readable message for diagnostics (admin channel only).
    pub message: String,
}

impl PlatformError {
    /// Convenience constructor.
    pub fn new(family: ErrorFamily, scenario: u16, message: impl Into<String>) -> Self {
        Self {
            family,
            scenario,
            message: message.into(),
        }
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "E_{}-{:02}: {}",
            self.family.as_str(),
            self.scenario,
            self.message
        )
    }
}

impl std::error::Error for PlatformError {}

/// Top-level subsystem family for error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorFamily {
    /// Session / command / view-store context (M-04, M-05, M-03).
    Context,
    /// ISS / execution backend (M-13, M-17).
    Iss,
    /// Event-driven engine, scheduler, time quantum (M-12).
    Engine,
    /// Bus and memory map (M-14).
    Bus,
    /// Peripheral modeling (M-15).
    Peripheral,
    /// Interrupt controller (M-16).
    Interrupt,
    /// Debug services: breakpoint, step, register/memory (M-08, M-09, M-10).
    Debug,
    /// Observability: probe, waveform, trace, symbol (M-18, M-19, M-11).
    Observability,
    /// Snapshot and deterministic event flow (M-20).
    Snapshot,
    /// ELF / DWARF / object parsing (M-06, M-11).
    Loader,
    /// Health-management system (M-22).
    Health,
    /// Data store / persistence (M-23).
    Storage,
    /// Transport adapter (M-07).
    Transport,
}

impl ErrorFamily {
    /// 3–4 char tag used in the textual error code.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Context => "CTX",
            Self::Iss => "ISS",
            Self::Engine => "ENG",
            Self::Bus => "BUS",
            Self::Peripheral => "PER",
            Self::Interrupt => "INT",
            Self::Debug => "DBG",
            Self::Observability => "OBS",
            Self::Snapshot => "SNAP",
            Self::Loader => "LDR",
            Self::Health => "HMS",
            Self::Storage => "STORE",
            Self::Transport => "TRX",
        }
    }
}
