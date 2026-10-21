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

/// Error family used by the protocol-level codes below.
pub const PROTO_FAMILY: ErrorFamily = ErrorFamily::Protocol;

/// Error family used by the debug-service codes below.
pub const DEBUG_FAMILY: ErrorFamily = ErrorFamily::Debug;

// ---------------------------------------------------------------------------
// Protocol family — `E_PROTO-01`..`E_PROTO-08`
// ---------------------------------------------------------------------------

/// The LLD's protocol-error codes, verbatim.
///
/// The **family** landed in contract v2 (D-32 gap 5 — before v2 the LLD's
/// `E_PROTO-*` codes had no contract-side home at all). The **scenario
/// codes** land with it: a family without its scenarios is only half a
/// home, and the kernel would otherwise have to spell `"E_PROTO-01"` as a
/// bare string literal — which is exactly how the typo class of bug this
/// ledger keeps catching gets in.
///
/// Each meaning below is copied from the module design specification's
/// error-code table (chapter 6).
///
/// Scenario code: the protocol version does not match the one this build
/// speaks (or the host asked for a version it cannot speak).
pub const E_PROTO_01_VERSION_MISMATCH: u16 = 1;

/// Scenario code: an unknown field was seen. **Ignored** (the contract's
/// "only add, never change" rule) — logged only.
pub const E_PROTO_02_UNKNOWN_FIELD: u16 = 2;

/// Scenario code: malformed ELF / HEX / DWARF input.
pub const E_PROTO_03_MALFORMED_IMAGE: u16 = 3;

/// Scenario code: the input exceeds a size / complexity limit.
pub const E_PROTO_04_INPUT_TOO_LARGE: u16 = 4;

/// Scenario code: a sequence-number gap (the transport resumes by pulling
/// the missing messages).
pub const E_PROTO_05_SEQUENCE_GAP: u16 = 5;

/// Scenario code: the transport frame's CRC did not match.
pub const E_PROTO_06_CRC_MISMATCH: u16 = 6;

/// Scenario code: a critical event's acknowledgement timed out.
pub const E_PROTO_07_ACK_TIMEOUT: u16 = 7;

/// Scenario code: the message's shape is malformed (missing field, wrong
/// type, unknown variant).
pub const E_PROTO_08_BAD_MESSAGE: u16 = 8;

// ---------------------------------------------------------------------------
// Debug family — `E_DBG-01`
// ---------------------------------------------------------------------------

/// Scenario code: the register index is outside the 32-entry register
/// file.
///
/// **Write-only.** Reading out of range is *clamped* (a read has no side
/// effect), but writing out of range is **rejected** — clamping a write
/// would silently write a *different* register than the caller named.
///
/// The `Debug` family is the contract's declared home for the debug
/// services (breakpoint / step / register-and-memory, i.e. M-08 / M-09 /
/// M-10); M-09 already declares it as its family.
pub const E_DBG_01_INVALID_REGISTER_INDEX: u16 = 1;

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

    /// The scenario constants must render as the LLD's codes, digit for
    /// digit. A constant that renders `E_PROTO-06` but is *spelled*
    /// `E_PROTO-6` in a table somewhere is how the two drift apart.
    #[test]
    fn the_protocol_scenario_codes_render_as_the_lld_spells_them() {
        let cases = [
            (E_PROTO_01_VERSION_MISMATCH, "E_PROTO-01"),
            (E_PROTO_02_UNKNOWN_FIELD, "E_PROTO-02"),
            (E_PROTO_03_MALFORMED_IMAGE, "E_PROTO-03"),
            (E_PROTO_04_INPUT_TOO_LARGE, "E_PROTO-04"),
            (E_PROTO_05_SEQUENCE_GAP, "E_PROTO-05"),
            (E_PROTO_06_CRC_MISMATCH, "E_PROTO-06"),
            (E_PROTO_07_ACK_TIMEOUT, "E_PROTO-07"),
            (E_PROTO_08_BAD_MESSAGE, "E_PROTO-08"),
        ];
        for (scenario, expected) in cases {
            let err = PlatformError::new(PROTO_FAMILY, scenario, "x");
            assert_eq!(
                err.to_string(),
                format!("{expected}: x"),
                "场景码 {scenario} 渲染成了 {err}"
            );
        }
    }

    /// The debug family renders `E_DBG-01` — the code the contract-v2
    /// spec uses for "register index out of range".
    #[test]
    fn the_debug_scenario_code_renders_as_e_dbg_01() {
        let err = PlatformError::new(
            DEBUG_FAMILY,
            E_DBG_01_INVALID_REGISTER_INDEX,
            "register index 99 is out of range",
        );
        assert_eq!(
            err.to_string(),
            "E_DBG-01: register index 99 is out of range"
        );
    }
}
