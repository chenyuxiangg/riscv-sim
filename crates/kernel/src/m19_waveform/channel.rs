//! Channel metadata + pipeline policy types (M-19 v0.9 §2.1, table 71).
//!
//! M-19 keeps a small per-channel table so the exporter can label the
//! channels and size the VCD variables. The table is *metadata only* —
//! the actual edge stream lives in the per-channel ring buffers
//! ([`super::ring`]).
//!
//! The channel table is deliberately separate from the M-18 channel
//! table: M-18 owns *probe* bindings (source + signal → channel),
//! M-19 owns *rendering* metadata (label + value width → channel).
//! The wiring code copies the relevant fields across at boot.

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::EdgeKind;

/// Rendering metadata for one waveform channel (M-19 v0.9 §2.1).
///
/// `value_width` is the bit width of the signal: 1 for a GPIO pin,
/// 8/16/32/64 for a byte/half/word/double-word bus signal. The
/// exporter uses it to pick the VCD `$var` width and to decide
/// between scalar (`0`/`1`) and vector (`b1010 …`) value encodings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelMeta {
    /// Display label (e.g. `"GPIO0.OUT"`, `"UART0.TX"`).
    pub label: String,
    /// Bit width of the signal (1, 8, 16, 32, 64).
    pub value_width: u8,
    /// Whether the channel is expected to produce `Rising` / `Falling`
    /// edges (single-bit) or only `Value` edges (multi-bit).
    ///
    /// This mirrors M-18's `ChannelKind`. The exporter does not need
    /// it (VCD carries values, not edge kinds) but the CSV exporter
    /// renders the edge kind verbatim, and the frontend uses the
    /// distinction to draw the transition arrow.
    pub single_bit: bool,
}

impl ChannelMeta {
    /// Metadata for a single-bit signal (a GPIO pin, an IRQ line).
    pub fn single_bit(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value_width: 1,
            single_bit: true,
        }
    }

    /// Metadata for a multi-bit signal.
    pub fn multi_bit(label: impl Into<String>, value_width: u8) -> Self {
        Self {
            label: label.into(),
            value_width: value_width.max(1),
            single_bit: false,
        }
    }

    /// Default metadata for a channel that was never declared: a
    /// 64-bit value channel labelled `"ch<N>"`.
    ///
    /// M-19 never drops an event just because the channel was not
    /// declared — a missing declaration is a wiring mistake, and
    /// silently discarding the observation would hide it. The event
    /// is recorded with this fallback metadata instead.
    pub fn fallback(ch: u8) -> Self {
        Self {
            label: format!("ch{ch}"),
            value_width: 64,
            single_bit: false,
        }
    }
}

/// Overflow handling policy (M-19 v0.9 §2.1, §3.3).
///
/// Both policies **evict the oldest entry** when a channel's ring
/// buffer is full — the newest observation has already happened in
/// the simulation, so refusing to record it would fabricate a gap in
/// the history (M-19 §6.2(5): "历史丢失部分以标记提示，不伪造数据").
///
/// The difference is what happens to the *simulation*:
///
/// - [`OverflowPolicy::DropOldestMark`] — keep simulating. The loss
///   is recorded in the channel's `overflow_count` so the frontend
///   can render a "history truncated" marker. This is the default.
/// - [`OverflowPolicy::PauseSim`] — additionally request a pause of
///   the simulation through the M-12 pause hook, so the caller can
///   decide whether to slow down, reduce the probe rate, or accept
///   the loss. Pausing is a control-plane operation; it does not
///   modify registers or memory, so it stays within the read-only
///   observation constraint (M-19 v0.9 §1.3, review 19-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverflowPolicy {
    /// Drop the oldest entry, mark the overflow, keep simulating.
    #[default]
    DropOldestMark,
    /// Drop the oldest entry, mark the overflow, and request a
    /// simulation pause.
    PauseSim,
}

impl OverflowPolicy {
    /// Stable wire code (`0` = drop oldest, `1` = pause sim).
    pub fn code(self) -> u8 {
        match self {
            Self::DropOldestMark => 0,
            Self::PauseSim => 1,
        }
    }

    /// Parse a wire code. Returns `None` for an unknown value — the
    /// caller must reject it and keep the current policy
    /// (M-19 v0.9 §6.1 "无效策略").
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::DropOldestMark),
            1 => Some(Self::PauseSim),
            _ => None,
        }
    }

    /// Human-readable name (log field).
    pub fn name(self) -> &'static str {
        match self {
            Self::DropOldestMark => "DROP_OLDEST_MARK",
            Self::PauseSim => "PAUSE_SIM",
        }
    }
}

/// Wire format of the incremental push payload (M-19 v0.9 §2.1).
///
/// The design doc's load analysis (review 19-1) shows the peak
/// payload rate is ≈24.4 MiB/s, which pushes the *binary* form with
/// a fixed-length header; compression is a P1 extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireFormat {
    /// Little-endian binary with a fixed-length header.
    #[default]
    BinLeHeader,
    /// Little-endian binary with a fixed-length header, compressed.
    /// Reserved for P1 (M-19 v0.9 §1.3).
    BinLeCompressed,
}

impl WireFormat {
    /// Stable wire code.
    pub fn code(self) -> u8 {
        match self {
            Self::BinLeHeader => 0,
            Self::BinLeCompressed => 1,
        }
    }

    /// Parse a wire code.
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::BinLeHeader),
            1 => Some(Self::BinLeCompressed),
            _ => None,
        }
    }
}

/// Reject an invalid configuration value (M-19 v0.9 §6.1).
///
/// M-19 commits to "no new error codes" (§6.1), so this reuses the
/// existing `Observability` family's first scenario (`E_OBS-01`),
/// which M-18 already uses for observability configuration
/// conflicts.
pub fn invalid_config(what: &str, value: u8) -> PlatformError {
    PlatformError::new(
        super::WAVEFORM_FAMILY,
        1,
        format!("invalid {what}: {value}"),
    )
}

/// Render an [`EdgeKind`] as the short text used by the CSV export.
pub fn edge_kind_text(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Rising => "Rising",
        EdgeKind::Falling => "Falling",
        EdgeKind::Value => "Value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::error::ErrorFamily;

    #[test]
    fn overflow_policy_codes_round_trip() {
        for p in [OverflowPolicy::DropOldestMark, OverflowPolicy::PauseSim] {
            assert_eq!(OverflowPolicy::from_code(p.code()), Some(p));
        }
    }

    #[test]
    fn overflow_policy_rejects_unknown_code() {
        assert_eq!(OverflowPolicy::from_code(2), None);
        assert_eq!(OverflowPolicy::from_code(255), None);
    }

    #[test]
    fn default_overflow_policy_is_drop_oldest_mark() {
        assert_eq!(OverflowPolicy::default(), OverflowPolicy::DropOldestMark);
    }

    #[test]
    fn wire_format_codes_round_trip() {
        for f in [WireFormat::BinLeHeader, WireFormat::BinLeCompressed] {
            assert_eq!(WireFormat::from_code(f.code()), Some(f));
        }
        assert_eq!(WireFormat::from_code(9), None);
    }

    #[test]
    fn invalid_config_is_observability_scenario_one() {
        let e = invalid_config("overflow policy", 7);
        assert_eq!(e.family, ErrorFamily::Observability);
        assert_eq!(e.scenario, 1);
        assert!(e.to_string().starts_with("E_OBS-01"));
    }

    #[test]
    fn channel_meta_fallback_uses_64_bit_width() {
        let m = ChannelMeta::fallback(3);
        assert_eq!(m.label, "ch3");
        assert_eq!(m.value_width, 64);
        assert!(!m.single_bit);
    }

    #[test]
    fn multi_bit_meta_clamps_width_to_at_least_one() {
        assert_eq!(ChannelMeta::multi_bit("X", 0).value_width, 1);
    }
}
