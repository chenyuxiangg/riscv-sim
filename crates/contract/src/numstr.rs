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

#[cfg(test)]
mod field_tests {
    //! The numeric policy applied to **every** virtual-time field.
    //!
    //! ⚠️ **Why this test exists** (V-A 检视 · 高危 2 + 中危 3): the
    //! policy is applied on **10** field **sites** — the spec's §2.2 table
    //! lists **9** fields, but `SchedEventDto` covers **two** variant
    //! sites (`PeripheralTick` / `TimerExpiry`).
    //!
    //! Not **one** of those 10 sites had a **shape** assertion on the
    //! field itself: the existing tests asserted **which keys exist**,
    //! never **what the value looks like**. Deleting any single
    //! `#[serde(with = "u64_decimal")]` left the contract's 32 cases
    //! **and the whole workspace's 631** green (V-A 实测).
    //!
    //! That is the worst kind of gap for this batch: `u64_decimal`'s
    //! *deserialiser* accepts **both** forms, so a silently reverted
    //! field does not error — it just goes back to the v1 wire format
    //! (a JSON number), which is precisely the precision loss the whole
    //! v2 numeric policy exists to remove.
    //!
    //! So: **one** test, **every** site, asserting the wire form
    //! directly. It fails the moment any one of them stops being a
    //! string.

    use super::u64_decimal;
    use crate::model::{Edge, EdgeEvent, EdgeKind, SimState, TraceDir, TraceEvent, WaveformDelta};
    use crate::protocol::EventPayload;
    use crate::snapshot::{QueuedEventDto, SchedEventDto};

    /// Read a nested field out of a JSON document.
    fn at(v: &serde_json::Value, path: &[&str]) -> serde_json::Value {
        let mut cur = v.clone();
        for p in path {
            let next = cur.get(*p).cloned();
            match next {
                Some(n) => cur = n,
                None => panic!("路径 {:?} 不存在：{v}", path),
            }
        }
        cur
    }

    /// Assert that the field at `path` is a **decimal string**.
    fn assert_decimal_string(v: &serde_json::Value, path: &[&str], what: &str) {
        let got = at(v, path);
        assert!(
            got.is_string(),
            "{what} 的线上表示必须是**十进制字符串**（规格 §2.2），实际是 {got}"
        );
        // And it must be *decimal* — a JSON number that happens to be
        // quoted is not the point; the point is that a JS reader can
        // round-trip it without losing precision.
        let text = got.as_str().expect("刚才断言过是字符串");
        assert!(
            text.chars().all(|c| c.is_ascii_digit()),
            "{what} 的字符串形态必须是纯十进制，实际是 {text:?}"
        );
    }

    #[test]
    fn every_virtual_time_field_travels_as_a_decimal_string() {
        // -- model::SimState::virtual_time_ps --------------------------
        let state = SimState {
            virtual_time_ps: 1_234_567,
            ..SimState::default()
        };
        let json = serde_json::to_value(&state).expect("json");
        assert_decimal_string(&json, &["virtual_time_ps"], "SimState.virtual_time_ps");

        // -- model::Edge::t_ps ------------------------------------------
        let edge = Edge { t_ps: 7, value: 1 };
        let json = serde_json::to_value(&edge).expect("json");
        assert_decimal_string(&json, &["t_ps"], "model::Edge::t_ps");

        // -- model::EdgeEvent::t_ps -------------------------------------
        let ev = EdgeEvent {
            t_ps: 8,
            ch: 0,
            value: 1,
            kind: EdgeKind::Value,
        };
        let json = serde_json::to_value(ev).expect("json");
        assert_decimal_string(&json, &["t_ps"], "model::EdgeEvent::t_ps");

        // -- model::TraceEvent::t_ps ------------------------------------
        let tr = TraceEvent {
            t_ps: 9,
            addr: 0x1000,
            width: 4,
            dir: TraceDir::Read,
        };
        let json = serde_json::to_value(tr).expect("json");
        assert_decimal_string(&json, &["t_ps"], "model::TraceEvent::t_ps");

        // -- model::WaveformDelta::t_virt --------------------------------
        let batch = WaveformDelta {
            seq_start: 0,
            seq_end: 0,
            t_virt: 10,
            entries: vec![],
        };
        let json = serde_json::to_value(&batch).expect("json");
        assert_decimal_string(&json, &["t_virt"], "model::WaveformDelta::t_virt");

        // -- protocol::EventPayload::Tick::elapsed_ps -------------------
        let tick = EventPayload::Tick {
            elapsed_ps: 11,
            pc: 0,
        };
        let json = serde_json::to_value(&tick).expect("json");
        assert_decimal_string(&json, &["Tick", "elapsed_ps"], "Tick::elapsed_ps");

        // -- protocol::EventPayload::SimStateDelta::t_ps ---------------
        let delta = EventPayload::SimStateDelta {
            registers: vec![],
            pc: None,
            t_ps: 12,
        };
        let json = serde_json::to_value(&delta).expect("json");
        assert_decimal_string(&json, &["SimStateDelta", "t_ps"], "SimStateDelta::t_ps");

        // -- snapshot::SchedEventDto::at_ps (两个变体各一个) -------------
        let periph = SchedEventDto::PeripheralTick {
            paddr: 0x2000_0000,
            at_ps: 13,
        };
        let json = serde_json::to_value(&periph).expect("json");
        assert_decimal_string(
            &json,
            &["peripheral_tick", "at_ps"],
            "SchedEventDto::PeripheralTick::at_ps",
        );

        let timer = SchedEventDto::TimerExpiry { irq: 2, at_ps: 14 };
        let json = serde_json::to_value(&timer).expect("json");
        assert_decimal_string(
            &json,
            &["timer_expiry", "at_ps"],
            "SchedEventDto::TimerExpiry::at_ps",
        );

        // -- snapshot::QueuedEventDto::at_ps ----------------------------
        let queued = QueuedEventDto {
            at_ps: 15,
            seq: 0,
            event: periph,
        };
        let json = serde_json::to_value(&queued).expect("json");
        assert_decimal_string(&json, &["at_ps"], "QueuedEventDto::at_ps");
    }

    /// The **deserialiser** side of the policy: a decimal string is
    /// read back exactly, and a plain number is still accepted (old
    /// producers) — but a **float** is not.
    ///
    /// ⚠️ The float case matters: `serde_json` will happily hand a
    /// `1.0` to the visitor, and a `u64` field must **not** accept it
    /// silently (`1.5` would truncate to 1).
    #[test]
    fn a_float_is_not_accepted_as_a_virtual_time() {
        #[derive(Debug, serde::Deserialize)]
        struct Holder {
            #[serde(with = "u64_decimal")]
            value: u64,
        }
        assert!(
            serde_json::from_str::<Holder>(r#"{"value":1.5}"#).is_err(),
            "浮点必须被拒——1.5 静默截成 1 就是精度丢失"
        );
        assert!(serde_json::from_str::<Holder>(r#"{"value":1.0}"#).is_err());

        // And the **accepted** forms still work — the policy is "string
        // preferred, legacy number tolerated, float refused", not
        // "everything refused".
        let from_string: Holder = serde_json::from_str(r#"{"value":"9007199254740993"}"#)
            .expect("十进制字符串必须被接受");
        assert_eq!(
            from_string.value, 9_007_199_254_740_993,
            "超出 2^53 的值必须**精确**读回——这正是走字符串的理由"
        );
        let from_number: Holder =
            serde_json::from_str(r#"{"value":42}"#).expect("旧生产者的数字必须仍被接受");
        assert_eq!(from_number.value, 42);
    }
}
