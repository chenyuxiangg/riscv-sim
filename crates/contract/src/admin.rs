//! The **admin channel** payloads — contract v8 (STAGE-29 · D-129).
//!
//! # Why this module exists
//!
//! The contract already had the **command** side of the admin channel
//! (`CommandPayload::Admin { op: AdminOp }` with `DrainLogs` /
//! `KillGuest` / `PersistProject` / `Custom`) ⇒ ⇒ but it had **no reply
//! shape** ⇒ ⇒ the host could only answer *"admin channel disabled"*
//! for **every** op ⇒ ⇒ the channel was an **empty shell** (debt ledger
//! **D-129**).
//!
//! This module supplies the **two** missing shapes:
//!
//! - [`LogEntryDto`] — one entry of the kernel's log ring (M-21);
//! - [`AdminDiagDto`] — the health-management system's diagnostic kit
//!   (M-22).
//!
//! # What this module is **not**
//!
//! ⚠ **It defines *shapes*, not behaviour.**  The log ring lives in
//! M-21, the diagnostic kit in M-22 ⇒ ⇒ **the conversion lives in the
//! kernel crate** (M-21 / M-22) ⇒ ⇒ **the contract layer does not
//! depend on the kernel** — it only says *how their data travels*.
//!
//! ⚠ **It carries no authorisation.**  Whether the admin channel is
//! enabled at all is the **host's** switch (`admin_enabled`, default
//! **off**) ⇒ ⇒ "who is allowed to turn it on" is a **separate** ledger
//! item (**D-131**) ⇒ ⇒ **no role concept is introduced here**.
//!
//! # The two timestamps
//!
//! `time_virt_ps` is **virtual** time (picoseconds) and `time_host_ms`
//! is the **host** monotonic clock (milliseconds) ⇒ ⇒ **same family of
//! quantity** ⇒ ⇒ **same wire form**: a **decimal string** (the
//! `u64_decimal` policy of contract v2 §2.2, see [`crate::numstr`]).
//!
//! ⚠ A JSON number would become an IEEE-754 double on the JavaScript
//! side, which is exact only up to `2^53` ⇒ ⇒ the virtual-time field
//! **must not** be a number.

use serde::{Deserialize, Serialize};

/// One log entry, crossing the process boundary.
///
/// ⚠ **The fields mirror M-21's `LogEntry` one by one** ⇒ ⇒ the
/// conversion is a **field-by-field copy** ⇒ ⇒ the kernel crate owns it
/// (M-21) ⇒ ⇒ this type owns only the **shape**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntryDto {
    /// Virtual time (picoseconds) — **decimal string** (contract v2 §2.2).
    #[serde(with = "crate::numstr::u64_decimal")]
    pub time_virt_ps: u64,
    /// Host monotonic time (milliseconds) — **decimal string**
    /// (same family as the virtual time ⇒ ⇒ same wire form).
    #[serde(with = "crate::numstr::u64_decimal")]
    pub time_host_ms: u64,
    /// The session this entry belongs to (always `0` in the MVP).
    pub session_id: u32,
    /// The severity: `info` / `warn` / `error` / `fatal`.
    pub level: String,
    /// The "module - flow point" tag.
    pub ctx: String,
    /// The full scenario code, when the entry carries one.
    pub err_code: Option<String>,
    /// The message text.
    pub msg: String,
    /// **True** when any field of this entry was truncated.
    pub truncated: bool,
}

/// The admin diagnostic kit, crossing the process boundary.
///
/// ⚠ **The fields mirror M-22's `DiagnosticKit` one by one** ⇒ ⇒ the
/// conversion is a **field-by-field copy** ⇒ ⇒ the kernel crate owns it
/// (M-22) ⇒ ⇒ this type owns only the **shape**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminDiagDto {
    /// The scenario code the kit is about, when there is one.
    pub err_code: Option<String>,
    /// A one-line summary of the snapshot state.
    pub snapshot_summary: String,
    /// The tail of the input sequence numbers (as decimal strings — the
    /// input sequence is a `u64` counter, and this channel travels it
    /// the same way the other counters travel).
    pub input_seq_tail: Vec<String>,
    /// Whether the host's monotonic clock is available (v0.43 · D-135 ·
    /// an **explicit boolean**, never a missing key).
    pub host_clock_available: bool,
}

#[cfg(test)]
mod tests {
    //! The **wire shape** of the admin channel — pinned against the
    //! canonical fixture.
    //!
    //! ⚠ **Why a fixture file**: the front end has a **JS mirror** of this
    //! contract ⇒ ⇒ two trees describing one shape ⇒ ⇒ the classic
    //! "one shape, two truths" failure ⇒ ⇒ so the shape is written
    //! **once**, into `crates/contract/fixtures/admin-dto.json`, and
    //! **both** sides are pinned to that file:
    //!
    //! - here: the Rust DTOs must serialise to exactly that JSON;
    //! - the front end (`front/tests/admin-dto.test.mjs`): the JS mirror
    //!   must read the same file.
    //!
    //! ⇒ ⇒ **the fixture is the single truth** ⇒ ⇒ neither side may
    //! drift from it without the other side going red.

    use super::{AdminDiagDto, LogEntryDto};
    use crate::protocol::EventPayload;

    /// The canonical shape — **one** file, **two** consumers.
    const FIXTURE: &str = include_str!("../fixtures/admin-dto.json");

    fn fixture() -> serde_json::Value {
        serde_json::from_str(FIXTURE).expect("the canonical fixture must be valid JSON")
    }

    fn sample_entry() -> LogEntryDto {
        LogEntryDto {
            time_virt_ps: 1000,
            time_host_ms: 7,
            session_id: 0,
            level: "info".to_string(),
            ctx: "m21-logging".to_string(),
            err_code: None,
            msg: "hello".to_string(),
            truncated: false,
        }
    }

    fn sample_diag() -> AdminDiagDto {
        AdminDiagDto {
            err_code: None,
            snapshot_summary: "no snapshot".to_string(),
            input_seq_tail: vec!["1".to_string(), "2".to_string()],
            host_clock_available: true,
        }
    }

    #[test]
    fn the_log_entry_dto_has_exactly_eight_fields() {
        // A field-count assertion, not a field-by-field one: adding a
        // field to the DTO must break the **mirror** on the other side
        // (the fixture), and this is the cheap half of that check.
        let json = serde_json::to_value(sample_entry()).expect("json");
        let obj = json.as_object().expect("the DTO is an object");
        assert_eq!(obj.len(), 8, "LogEntryDto must have exactly 8 fields");
        for key in [
            "time_virt_ps",
            "time_host_ms",
            "session_id",
            "level",
            "ctx",
            "err_code",
            "msg",
            "truncated",
        ] {
            assert!(obj.contains_key(key), "LogEntryDto is missing `{key}`");
        }
    }

    #[test]
    fn the_admin_diag_dto_has_exactly_four_fields() {
        let json = serde_json::to_value(sample_diag()).expect("json");
        let obj = json.as_object().expect("the DTO is an object");
        assert_eq!(obj.len(), 4, "AdminDiagDto must have exactly 4 fields");
        for key in [
            "err_code",
            "snapshot_summary",
            "input_seq_tail",
            "host_clock_available",
        ] {
            assert!(obj.contains_key(key), "AdminDiagDto is missing `{key}`");
        }
    }

    #[test]
    fn both_timestamps_travel_as_decimal_strings() {
        let json = serde_json::to_value(sample_entry()).expect("json");
        for key in ["time_virt_ps", "time_host_ms"] {
            let got = json.get(key).expect("field");
            assert!(
                got.is_string(),
                "{key} must travel as a decimal string (contract v2 §2.2), got {got}"
            );
            let text = got.as_str().expect("just asserted a string");
            assert!(
                text.chars().all(|c| c.is_ascii_digit()),
                "{key} must be plain decimal, got {text:?}"
            );
        }
    }

    #[test]
    fn the_logs_event_matches_the_canonical_fixture() {
        let event = EventPayload::AdminLogs {
            entries: vec![sample_entry()],
        };
        let json = serde_json::to_value(event).expect("json");
        let expected = fixture().get("logs_event").expect("fixture member").clone();
        assert_eq!(
            json, expected,
            "the AdminLogs event must match the canonical fixture byte for byte"
        );
    }

    #[test]
    fn the_diag_event_matches_the_canonical_fixture() {
        let event = EventPayload::AdminDiag {
            diag: sample_diag(),
        };
        let json = serde_json::to_value(event).expect("json");
        let expected = fixture().get("diag_event").expect("fixture member").clone();
        assert_eq!(
            json, expected,
            "the AdminDiag event must match the canonical fixture byte for byte"
        );
    }
}
