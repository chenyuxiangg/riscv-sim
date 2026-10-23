//! Error code family + per-subsystem scenario codes (LLD §3.4).
//!
//! Format: `E_<FAMILY>-<SCENARIO>` where family is a 3–5 char subsystem tag
//! and scenario is a 2-digit code. New scenario codes are additive; old
//! codes never change meaning.
//!
//! ⚠️ The tag length was **3–4** until contract v2, which was **wrong**:
//! two of the fifteen families are 5 characters — `PROTO` (added in v2)
//! and `STORE` (pre-existing) — and the module design specification has
//! always spelled the protocol family `E_PROTO-0x`. The constraint is
//! now **3–5**; the tags themselves are unchanged, so no error code on
//! the wire moved (debt ledger **D-68**'s sibling fix).

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

/// Declare the error families **once** — the enum, the wire tag table
/// and [`ALL_FAMILIES`] are all expanded from this single list.
///
/// ```ignore
/// error_families! {
///     /// doc
///     Context => "CTX",
/// }
/// ```
///
/// The doc comment on each variant is carried over to the generated
/// variant, so the documentation stays where a reader expects it.
///
/// ⚠️ The macro is the **single source of truth**: adding a family means
/// adding one line here, and the tag table plus the completeness list
/// follow automatically. There is no way to add a variant that the
/// checks do not see.
///
/// (Before this, the enum and the two lists were maintained **by hand**,
/// and the completeness check compared only their **lengths** — so a new
/// variant added to the enum but forgotten in both lists left the check
/// **green**. V-A 检视 · 中危 5.)
///
/// ⚠️ **This macro declares the *families*, not the *scenario codes*.**
/// A family's scenario codes live with the module that owns the scenario:
/// the protocol codes in this file, the engine-segment codes in
/// [`crate::snapshot`], the session-payload codes in [`crate::session`],
/// the peripheral-port codes in [`crate::port`], the bus codes in the
/// kernel's `m14_bus`. A code that **two** modules need to name must
/// live in the **contract layer** — otherwise the same number ends up
/// spelled in two places and drifts apart (one code, two truths).
macro_rules! error_families {
    ($( $(#[$meta:meta])* $variant:ident => $tag:literal, )*) => {
        /// Top-level subsystem family for error codes.
        ///
        /// The variants and their **wire tags** are declared in **one**
        /// place by the `error_families!` macro — the enum,
        /// [`ErrorFamily::as_str`] and [`ALL_FAMILIES`] are all expanded
        /// from that single declaration. A new family that is not
        /// declared there is a **compile error**.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ErrorFamily {
            $(
                $(#[$meta])*
                $variant,
            )*
        }

        impl ErrorFamily {
            /// 3–5 char tag used in the textual error code.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $tag,)*
                }
            }
        }

        /// Every [`ErrorFamily`] variant, in declaration order.
        ///
        /// Expanded from the same declaration as the enum — it cannot
        /// drift out of sync with it.
        pub const ALL_FAMILIES: &[ErrorFamily] = &[
            $(ErrorFamily::$variant,)*
        ];
    };
}

error_families! {
    /// Session / command / view-store context (M-04, M-05, M-03).
    Context => "CTX",
    /// ISS / execution backend (M-13, M-17).
    Iss => "ISS",
    /// Event-driven engine, scheduler, time quantum (M-12).
    Engine => "ENG",
    /// Bus and memory map (M-14).
    Bus => "BUS",
    /// Peripheral modeling (M-15).
    Peripheral => "PER",
    /// Interrupt controller (M-16).
    Interrupt => "INT",
    /// Debug services: breakpoint, step, register/memory (M-08, M-09, M-10).
    Debug => "DBG",
    /// Observability: probe, waveform, trace, symbol (M-18, M-19, M-11).
    Observability => "OBS",
    /// Snapshot and deterministic event flow (M-20), plus the contract
    /// layer's snapshot DTO validation codes (`E_SNAP-01`..`E_SNAP-05`,
    /// see [`crate::snapshot`]; `E_SNAP-06`, the session-payload code,
    /// see [`crate::session`]).
    Snapshot => "SNAP",
    /// ELF / DWARF / object parsing (M-06, M-11).
    Loader => "LDR",
    /// Health-management system (M-22).
    Health => "HMS",
    /// Data store / persistence (M-23).
    Storage => "STORE",
    /// Transport adapter (M-07).
    Transport => "TRX",
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
    Protocol => "PROTO",
    /// Platform-description errors: the **memory map** described by a
    /// platform description (debt ledger **D-123-B** · frozen spec
    /// `docs/detailed-design/STAGE-12_V027_Platform_Description.md`).
    ///
    /// This family covers the **description layer** only — a malformed
    /// platform description (wrong format version, empty id, no regions,
    /// zero-sized / overflowing / overlapping regions, a region over the
    /// declared budget, a duplicated device id). Its scenario codes live
    /// in this file, next to the family they belong to.
    ///
    /// ⚠️ **Not** the bus family: the bus codes (`E_BUS-*`) are about a
    /// **live** memory map on the wire; these are about the **declared**
    /// one, before anything is mapped. A description that says "two
    /// regions overlap" (`E_PLT-06`) and a bus that ends up with two
    /// overlapping regions (`E_BUS-03`) are two different layers, so they
    /// carry two different codes (one code, two meanings is the disease
    /// this file keeps fighting).
    Platform => "PLT",
}

/// Error family used by the protocol-level codes below.
pub const PROTO_FAMILY: ErrorFamily = ErrorFamily::Protocol;

/// Error family used by the debug-service codes below.
pub const DEBUG_FAMILY: ErrorFamily = ErrorFamily::Debug;

/// Error family used by the engine-segment codes below.
pub const ENGINE_FAMILY: ErrorFamily = ErrorFamily::Engine;

/// Error family used by the platform-description codes below.
pub const PLATFORM_FAMILY: ErrorFamily = ErrorFamily::Platform;

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

// ---------------------------------------------------------------------------
// Engine family — `E_ENG-01`..`E_ENG-03`
// ---------------------------------------------------------------------------

/// Scenario code: the engine could not publish an event onto the bus
/// (M-12's event publication failed).
///
/// ⚠️ **Registered in contract v4** (debt ledger **D-116**): these three
/// engine codes used to live **only** inside the kernel crate ⇒ the front
/// end received the error but had **no contract-side registry** to
/// dispatch on ⇒ it could not tell "the engine is wedged" from any other
/// error family. The **contract layer** is the single home for a code
/// that more than one module has to name.
pub const E_ENG_01_BUS_PUBLISH_FAILED: u16 = 1;

/// Scenario code: the engine's event queue is inconsistent (an event was
/// scheduled in a way that violates the queue's invariants).
pub const E_ENG_02_QUEUE_INCONSISTENT: u16 = 2;

/// Scenario code: the engine made **no progress** — the guest kept
/// executing without any observable state change for the whole
/// no-progress window.
///
/// ⚠️ This is a **platform** fault, strictly distinct from a guest trap:
/// the guest did not trap, the *engine* did not move. The front end must
/// not present it as "the program crashed" — the session is wedged and
/// the engine bailed out on purpose.
pub const E_ENG_03_NO_PROGRESS: u16 = 3;

// ---------------------------------------------------------------------------
// Platform family — `E_PLT-01`..`E_PLT-11`
// ---------------------------------------------------------------------------

/// Scenario code: the platform description's `schema_version` is not the
/// one this build speaks.
pub const E_PLT_01_SCHEMA_VERSION: u16 = 1;

/// Scenario code: the platform description's `id` is empty.
///
/// The `id` is the platform's **primary key** (built-in descriptions are
/// selected by it) ⇒ an empty one names nothing.
pub const E_PLT_02_EMPTY_ID: u16 = 2;

/// Scenario code: the platform description declares **no** region at all.
pub const E_PLT_03_NO_REGIONS: u16 = 3;

/// Scenario code: a RAM / ROM region declares `size == 0`.
///
/// ⚠️ **Same meaning, different layer, different code**: M-14's
/// `E_BUS-06` says the same thing about a region on a **live** bus; this
/// one says it about a **declared** region, before anything is mapped.
pub const E_PLT_04_ZERO_SIZE: u16 = 4;

/// Scenario code: a region's `base + size` overflows the 32-bit address
/// space.
///
/// ⚠️ **Same meaning, different layer, different code**: M-14's
/// `E_BUS-04` is the runtime check on a live bus.
pub const E_PLT_05_OVERFLOW: u16 = 5;

/// Scenario code: two declared regions **overlap**.
pub const E_PLT_06_OVERLAP: u16 = 6;

/// Scenario code: a RAM / ROM region's **declared size** exceeds the
/// description's `region_budget_bytes`.
///
/// ⚠️ **This is a declaration-time check, and it is NOT the same
/// constraint as M-14's `E_BUS-09`.** Two different layers, two different
/// semantics, two different codes:
///
/// | | layer | measured |
/// | --- | --- | --- |
/// | `E_PLT_07` (this one) | **declaration** (before mapping) | the **declared** size |
/// | `E_BUS-09` | **runtime** (after mapping) | the **physical occupancy** (RAM is lazily allocated) |
///
/// ⇒ they must not be merged, and this code deliberately does **not**
/// reuse `E_BUS_09`'s number (one code, two meanings).
pub const E_PLT_07_OVER_BUDGET: u16 = 7;

/// Scenario code: two declared peripheral devices share the same `id`.
///
/// The restore path matches a device state onto a live instance **by
/// id** ⇒ two devices with one id are two states competing for one
/// instance.
pub const E_PLT_08_DUPLICATE_DEVICE_ID: u16 = 8;

/// Scenario code: the requested **platform id** does not exist.
///
/// ⚠️ **Host-side rule (V-10) — owned by the contract layer.** The
/// description's *declaration* rules are V-1 .. V-9; "the id you asked
/// for names no built-in description" is a rule about **looking a
/// description up**, and the frozen spec §1.6 puts it here ⇒ the host
/// must **not** invent its own code for it (one constraint, one landing
/// spot).
pub const E_PLT_09_UNKNOWN_PLATFORM: u16 = 9;

/// Scenario code: the platform description **file cannot be read**
/// (missing path, denied permission, I/O failure).
///
/// ⚠️ **Host-side rule (V-11) — owned by the contract layer**, for the
/// same reason as [`E_PLT_09_UNKNOWN_PLATFORM`].
pub const E_PLT_10_PLATFORM_FILE_UNREADABLE: u16 = 10;

/// Scenario code: the platform description file is **not a legal
/// description** (not valid JSON, or a shape the serde layer rejects —
/// rule V-8).
///
/// ⚠️ **Host-side rule (V-12) — owned by the contract layer**, for the
/// same reason as [`E_PLT_09_UNKNOWN_PLATFORM`].
pub const E_PLT_11_PLATFORM_FILE_MALFORMED: u16 = 11;

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

    /// The engine family's three scenario codes are **registered in the
    /// contract layer** (debt ledger **D-116**).
    ///
    /// Before v4 they lived only inside the kernel crate ⇒ the front end
    /// received `E_ENG-03` but had no contract-side table to look it up
    /// in ⇒ it could not dispatch on the code.
    ///
    /// The assertion lands on the **rendered** code (digit for digit) and
    /// on the **lookup**: a code that renders `E_ENG-3` (one digit) would
    /// pass a "the constant is 3" check and still break every front-end
    /// dispatch that greps for `E_ENG-03`.
    #[test]
    fn the_engine_scenario_codes_are_registered_in_the_contract_layer() {
        // The lookup the front end does: family + scenario → the wire code.
        let cases = [
            (E_ENG_01_BUS_PUBLISH_FAILED, "E_ENG-01"),
            (E_ENG_02_QUEUE_INCONSISTENT, "E_ENG-02"),
            (E_ENG_03_NO_PROGRESS, "E_ENG-03"),
        ];
        for (scenario, expected) in cases {
            let err = PlatformError::new(ENGINE_FAMILY, scenario, "x");
            assert_eq!(
                err.to_string(),
                format!("{expected}: x"),
                "场景码 {scenario} 渲染成了 {err}"
            );
        }

        // The no-progress code is the one the front end has to be able to
        // recognise: it is a **platform** fault, not a guest trap.
        assert_eq!(E_ENG_03_NO_PROGRESS, 3);
        let no_progress = PlatformError::new(
            ENGINE_FAMILY,
            E_ENG_03_NO_PROGRESS,
            "引擎在 1024 条指令内没有进展",
        );
        assert_eq!(no_progress.family, ErrorFamily::Engine);
        assert_eq!(no_progress.scenario, E_ENG_03_NO_PROGRESS);
        assert!(no_progress.to_string().starts_with("E_ENG-03: "));
    }

    /// **Every** family's wire tag is pinned here.
    ///
    /// The tag is the **prefix of every error code on the wire** — it is
    /// the part a debugger greps for and the part the module design
    /// specification's error-code table lists. Before this test only
    /// `PROTO` and `TRX` had a guard, so renaming any of the other
    /// twelve tags would have turned **no** test red while every error
    /// code of that family changed shape on the wire.
    ///
    /// That is not hypothetical: the LLD's error table spelled the
    /// peripheral family `E_PP` while the contract (and the code) emit
    /// `E_PER` — a live doc/code conflict that went unnoticed precisely
    /// because nothing pinned the tag (debt ledger **D-69**).
    ///
    /// The table is the **single** place the tags are written down; the
    /// `as_str` implementation is checked against it, not the other way
    /// round.
    #[test]
    fn every_family_tag_is_pinned() {
        const TAGS: &[(ErrorFamily, &str)] = &[
            (ErrorFamily::Context, "CTX"),
            (ErrorFamily::Iss, "ISS"),
            (ErrorFamily::Engine, "ENG"),
            (ErrorFamily::Bus, "BUS"),
            (ErrorFamily::Peripheral, "PER"),
            (ErrorFamily::Interrupt, "INT"),
            (ErrorFamily::Debug, "DBG"),
            (ErrorFamily::Observability, "OBS"),
            (ErrorFamily::Snapshot, "SNAP"),
            (ErrorFamily::Loader, "LDR"),
            (ErrorFamily::Health, "HMS"),
            (ErrorFamily::Storage, "STORE"),
            (ErrorFamily::Transport, "TRX"),
            (ErrorFamily::Protocol, "PROTO"),
            (ErrorFamily::Platform, "PLT"),
        ];

        // No family may be forgotten: the list must cover *every* variant.
        assert_eq!(
            TAGS.len(),
            ALL_FAMILIES.len(),
            "族清单漏了某个 ErrorFamily 变体 —— 新增族时必须在这里补一行"
        );

        for family in ALL_FAMILIES {
            let tag = TAGS
                .iter()
                .find(|(f, _)| f == family)
                .map(|(_, t)| *t)
                .unwrap_or_else(|| panic!("{family:?} 没有在族标签表里登记"));
            assert_eq!(
                family.as_str(),
                tag,
                "{family:?} 的线上族标签变了 —— 错误码的前缀变了"
            );
            // The module header says the tag is a 3–5 character
            // subsystem tag; a 2- or 6-character tag would break the
            // `E_<FAMILY>-<SCENARIO>` format the LLD's tables assume.
            assert!(
                tag.len() == 3 || tag.len() == 4 || tag.len() == 5,
                "族标签 {tag} 不是 3–5 个字符"
            );
        }
    }
}
