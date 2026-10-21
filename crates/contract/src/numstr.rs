//! Decimal-string transport for 64-bit counters (contract v2, spec §2).
//!
//! # Why
//!
//! A `u64` serialises to a **JSON number**, and a JSON number becomes an
//! IEEE-754 double on the JavaScript side — which is exact only up to
//! `2^53`.  Virtual time is measured in picoseconds, so
//!
//! ```text
//! 2^53 ps = 9 007 199 254 740 992 ps ≈ 9 007 s ≈ 2.5 hours of simulated time
//! ```
//!
//! The quantum is **1 µs**, so a 2.5-hour run crosses that boundary — and
//! past it the front end would silently lose precision on every
//! timestamp, which shows up as a waveform time axis that jumps.
//!
//! The rule (spec §2.2) is therefore: **quantities that grow with the
//! simulated time travel as decimal strings**; every other `u64`
//! (counters, sequence numbers, signal values) stays a JSON number,
//! because their reachable upper bounds are far above `2^53` anyway.
//!
//! # What this module does *not* change
//!
//! The **Rust types stay `u64`**.  Only the *wire representation*
//! changes, so no kernel code has to change: the arithmetic, the
//! comparisons and the assignments all stay exactly as they were.
//!
//! # Deserialisation is lenient on purpose
//!
//! [`u64_decimal::deserialize`] accepts a decimal **string** *and* a plain
//! **number**.  A number is what an older producer sent; accepting it
//! keeps the "only add, never change" spirit of the contract.  The
//! catch is that a number has *already* lost precision above `2^53` —
//! the documentation says so explicitly, and the front end's
//! `psToNumber` helper refuses to guess.

use serde::de::{Unexpected, Visitor};
use serde::{Deserializer, Serializer};

/// Serialise a `u64` as a decimal **string** and read it back.
///
/// Wire form: `"9007199254740992"` (no sign, no separators, base 10).
///
/// Use it on a field with:
///
/// ```ignore
/// #[serde(with = "crate::numstr::u64_decimal")]
/// pub t_ps: u64,
/// ```
pub mod u64_decimal {
    use super::*;

    /// Write the value as a decimal string.
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    /// Read the value from a decimal string **or** a plain number.
    ///
    /// A number is accepted for backwards compatibility with producers
    /// that predate the decimal-string rule; it carries the usual JSON
    /// caveat of being exact only up to `2^53`.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        struct U64Visitor;

        impl<'de> Visitor<'de> for U64Visitor {
            type Value = u64;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a decimal string or a non-negative integer")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                v.parse::<u64>()
                    .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(v)
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                u64::try_from(v).map_err(|_| E::invalid_value(Unexpected::Signed(v), &self))
            }

            fn visit_u32<E: serde::de::Error>(self, v: u32) -> Result<Self::Value, E> {
                Ok(u64::from(v))
            }
        }

        deserializer.deserialize_any(U64Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::u64_decimal;

    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct Holder {
        #[serde(with = "u64_decimal")]
        value: u64,
    }

    #[test]
    fn a_value_is_written_as_a_decimal_string() {
        let json = serde_json::to_string(&Holder {
            value: 9_007_199_254_740_992,
        })
        .unwrap();
        assert_eq!(json, r#"{"value":"9007199254740992"}"#);
    }

    #[test]
    fn a_decimal_string_is_read_back() {
        let holder: Holder = serde_json::from_str(r#"{"value":"9007199254740992"}"#).unwrap();
        assert_eq!(holder.value, 9_007_199_254_740_992);
    }

    #[test]
    fn a_plain_number_is_accepted_too() {
        let holder: Holder = serde_json::from_str(r#"{"value":42}"#).unwrap();
        assert_eq!(holder.value, 42);
    }

    #[test]
    fn a_malformed_value_is_rejected() {
        assert!(serde_json::from_str::<Holder>(r#"{"value":"nope"}"#).is_err());
        assert!(serde_json::from_str::<Holder>(r#"{"value":-1}"#).is_err());
        assert!(serde_json::from_str::<Holder>(r#"{"value":null}"#).is_err());
    }

    #[test]
    fn the_round_trip_is_lossless_above_two_to_the_53() {
        for v in [
            0u64,
            1,
            u32::MAX as u64,
            9_007_199_254_740_992, // 2^53
            9_007_199_254_740_993, // 2^53 + 1 — the first value a double cannot hold
            u64::MAX,
        ] {
            let json = serde_json::to_string(&Holder { value: v }).unwrap();
            let back: Holder = serde_json::from_str(&json).unwrap();
            assert_eq!(back.value, v, "round trip lost {v}");
        }
    }
}
