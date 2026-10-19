//! Channel definitions (M-18 v0.8 §2.1, table 68).
//!
//! Each probed signal is bound to a **channel**: a (ch, source, signal,
//! kind, value_width) tuple. The registry looks up channels by `ch` to
//! decide whether to emit an edge event for an incoming `sample()` call.
//!
//! MVP defines ≥4 channels by convention; the registry is open to
//! more (the channel map is `BTreeMap<u8, …>`, not a fixed array).
//! The standard channel numbers are exposed as `CH_*` constants below
//! for callers that want to bind the same well-known channels.

use riscv_sim_contract::model::EdgeKind;

/// Well-known channel numbers (M-18 v0.8 §2.1.1).
///
/// These are the four channels the design doc requires by default.
/// Other channels may be added at any `ch` number — the registry
/// treats the channel map as an open table.
pub const CH_TIMER_CMP: u8 = 0;
pub const CH_GPIO0: u8 = 1;
pub const CH_IRQ_PULSE: u8 = 2;
pub const CH_KEY_STATE: u8 = 3;

/// What kind of signal the channel carries (M-18 v0.8 §2.1.1, §18-3).
///
/// `SingleBit` is the only kind for which the registry classifies
/// `Rising` vs `Falling`. `MultiBit` collapses both into `Value` (the
/// frontend draws a step on the new value; the arrow direction is
/// "ambiguous" by design). `Pulse` always emits, regardless of value
/// repetition — the canonical use is the IRQ-pulse channel
/// (M-18 v0.8 §18-2: every `irq_request` produces an event, no
/// value comparison, same-line repeats do not coalesce).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// One-bit signal — registry classifies `Rising` / `Falling`.
    SingleBit,
    /// Multi-bit word — only value-change events, kind always `Value`.
    MultiBit,
    /// Pulse / event stream — every observation emits, no comparison.
    Pulse,
}

/// Channel binding (M-18 v0.8 §2.1.1, table 68).
///
/// Constructed at attach time; lives in the registry's channel map.
/// The `source` + `signal` pair identifies where the channel's
/// observations come from; the registry uses these to filter
/// periph-side calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDef {
    /// Channel number (registry key).
    pub ch: u8,
    /// Logical source id (e.g. `"BUS"`, `"GPIO0"`, `"UART0"`).
    pub source: String,
    /// Signal name within the source (e.g. `"read"`, `"OUT"`, `"TX"`).
    pub signal: String,
    /// How the registry should classify the channel's edges.
    pub kind: ChannelKind,
    /// Sample-value width in bits (1, 8, 16, 32, 64). The registry
    /// forwards this verbatim to consumers; it does NOT truncate
    /// values (M-18 v0.8 §18-3: timer channels carry 64-bit values
    /// without truncation).
    pub value_width: u8,
}

impl ChannelDef {
    /// Convenience constructor for a single-bit channel.
    pub fn single_bit(ch: u8, source: impl Into<String>, signal: impl Into<String>) -> Self {
        Self {
            ch,
            source: source.into(),
            signal: signal.into(),
            kind: ChannelKind::SingleBit,
            value_width: 1,
        }
    }

    /// Convenience constructor for a multi-bit channel.
    pub fn multi_bit(
        ch: u8,
        source: impl Into<String>,
        signal: impl Into<String>,
        value_width: u8,
    ) -> Self {
        Self {
            ch,
            source: source.into(),
            signal: signal.into(),
            kind: ChannelKind::MultiBit,
            value_width,
        }
    }

    /// Convenience constructor for a pulse channel.
    pub fn pulse(ch: u8, source: impl Into<String>, signal: impl Into<String>) -> Self {
        Self {
            ch,
            source: source.into(),
            signal: signal.into(),
            kind: ChannelKind::Pulse,
            // Pulse channels typically carry an IRQ line or event id;
            // we still report the raw u64 the peripheral sent.
            value_width: 64,
        }
    }

    /// Classify a value transition into an [`EdgeKind`].
    ///
    /// Single-bit channels distinguish rising from falling; multi-bit
    /// channels collapse both into `Value`; pulse channels always
    /// report `Value` (the kind is informational for the renderer).
    ///
    /// **Single-bit masking (M-18 review #9, 2026-10-19):** for
    /// `SingleBit` channels, `prev` and `next` are masked to their
    /// least-significant bit before classification. This makes the
    /// classification robust against caller mistakes (a peripheral
    /// that sends a multi-bit value on a 1-bit channel) — the
    /// classification is always well-defined.
    pub fn classify(&self, prev: u64, next: u64) -> EdgeKind {
        match self.kind {
            ChannelKind::SingleBit => {
                let prev_bit = prev & 1;
                let next_bit = next & 1;
                if prev_bit == 0 && next_bit == 1 {
                    EdgeKind::Rising
                } else if prev_bit == 1 && next_bit == 0 {
                    EdgeKind::Falling
                } else {
                    // Same-value should have been filtered earlier;
                    // this branch is only hit if the caller skipped
                    // the duplicate check.
                    EdgeKind::Value
                }
            }
            ChannelKind::MultiBit | ChannelKind::Pulse => EdgeKind::Value,
        }
    }
}
