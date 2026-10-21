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
    /// Snapshot and deterministic event flow (M-20), plus the contract
    /// layer's snapshot DTO validation codes (`E_SNAP-01`..`E_SNAP-05`,
    /// see [`crate::snapshot`]).
    Snapshot,
    /// ELF / DWARF / object parsing (M-06, M-11).
    Loader,
    /// Health-management system (M-22).
    Health,
    /// Data store / persistence (M-23).
    Storage,
    /// Transport adapter (M-07).
    Transport,
    /// Protocol-level errors: schema-version negotiation, envelope
    /// validation, transport-frame integrity (M-05, M-07).
    ///
    /// This is the family the LLD's `E_PROTO-01`..`E_PROTO-08` codes
    /// belong to. Before v2 the contract had **no** family for them (the
    /// transport family is `TRX`), so the LLD's codes had no
    /// contract-side home.
    ///
    /// The two families are **not** interchangeable: `TRX` is a failure
    /// *of the transport itself* (connect, drop, timeout), `PROTO` is a
    /// failure *of the protocol* (version not negotiated, malformed
    /// envelope, CRC mismatch).
    Protocol,
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
            Self::Protocol => "PROTO",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LLD's protocol-error codes are spelled `E_PROTO-0x` — the
    /// family tag has to match the LLD **verbatim**, or every code in
    /// the specification would have no contract-side home (D-32 gap 5).
    #[test]
    fn the_protocol_family_is_spelled_like_the_lld() {
        assert_eq!(ErrorFamily::Protocol.as_str(), "PROTO");
        let err = PlatformError::new(ErrorFamily::Protocol, 6, "crc mismatch");
        assert_eq!(err.to_string(), "E_PROTO-06: crc mismatch");
    }

    /// The two protocol-adjacent families are **not** interchangeable:
    /// `TRX` is a failure of the transport, `PROTO` a failure of the
    /// protocol.
    #[test]
    fn the_transport_and_protocol_families_are_distinct() {
        assert_eq!(ErrorFamily::Transport.as_str(), "TRX");
        assert_ne!(
            ErrorFamily::Transport.as_str(),
            ErrorFamily::Protocol.as_str()
        );
    }
}
