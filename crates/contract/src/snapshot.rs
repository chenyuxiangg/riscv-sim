//! Serializable snapshot DTOs — the contract-level carrier for
//! deterministic snapshots.
//!
//! # Why this module exists
//!
//! [`crate::model::SimState`] carries the platform state, the virtual
//! clock and the event-queue insertion counter — but **not the pending
//! event queue itself**. Consequently
//! [`crate::port::ExecutionBackend::snapshot`] /
//! [`crate::port::ExecutionBackend::restore`] can only perform a *partial*
//! restore: events queued *after* the snapshot was taken are still pending
//! and still fire, so a run after a restore diverges from an uninterrupted
//! run with the same seed (debt ledger D-24).
//!
//! M-12 does have an engine-level snapshot that carries the queue
//! (`EngineSnapshot`), but it is **not serializable** — the kernel crate
//! deliberately has no serde dependency, and the contract layer cannot
//! host the kernel's internal types.
//!
//! This module supplies the missing third piece: a **contract-level,
//! serializable mirror** of the engine snapshot. M-12 owns the
//! bidirectional conversion (see `crates/kernel/src/m12_engine/snapshot.rs`),
//! which keeps the DTO and the engine in lockstep without either side
//! depending on the other's internals. With the DTO in place, persistence
//! (M-20 snapshot manager + M-23 DataStore) becomes implementable.
//!
//! # What this snapshot does **not** capture
//!
//! The DTO covers the **engine**: the scheduler's pending event queue, the
//! platform state (registers, PC, CSRs, pending writes, virtual clock) and
//! the insertion counter. It deliberately does **not** capture:
//!
//! - **peripheral models** (M-15) — UART FIFOs and status, timer
//!   `mtime`/`mtimecmp`, GPIO direction/output latches, …;
//! - **the interrupt controller** (M-16) — which IRQ lines are currently
//!   raised;
//! - the engine's `advance_steps` counter (a diagnostic metric, not state).
//!
//! Those live in objects the engine does not own and cannot serialize.
//! **A host that persists only this DTO does not reproduce a session** —
//! it reproduces the *engine*. Rehydrating peripherals and the interrupt
//! controller needs their own persisted representation (M-20 / M-23); that
//! gap is tracked in `docs/reviews/CROSS_MODULE_DEBT.md` (D-36).
//!
//! # Where the persisted-session format lives
//!
//! The persisted snapshot format is **not** defined here. It is owned by
//! M-20 / M-23's detailed design §2.1.1, which defines a single complete
//! object:
//!
//! ```text
//! Snapshot { cpMeta, m13State, m14Map, m15Periph }
//! ```
//!
//! (`DebugMeta` is a *project-level* store of its own — review 23-8 — and
//! is not part of the snapshot payload.)
//!
//! This DTO is the carrier for the **M-12/M-17 segment** of that object:
//! the scheduler queue, the platform state, the insertion counter, the
//! virtual clock and the per-quantum instruction budget. M-20 assembles the
//! full `Snapshot` from that segment plus the M-13 / M-14 / M-15 segments.
//!
//! The earlier LLD wording ("the snapshot payload is `SimState + DebugMeta`")
//! contradicted that detailed design; the discrepancy was **adjudicated on
//! 2026-10-20 in favour of the detailed design §2.1.1**, and the module
//! specification has since been corrected to match. See D-37 in the debt
//! ledger.
//!
//! # Compatibility
//!
//! Adding this module is **purely additive**: no existing type, field or
//! trait method changed shape. The version of the DTO format is carried
//! by [`EngineSnapshotDto::schema_version`]; consumers should refuse
//! snapshots whose version they do not know (see
//! [`EngineSnapshotDto::validate`]).

use serde::{Deserialize, Serialize};

use crate::error::{ErrorFamily, PlatformError};
use crate::model::{PendingWrite, SimState};

/// Format version of the snapshot DTO family.
///
/// Bump on every incompatible change to [`EngineSnapshotDto`] or its
/// members. [`EngineSnapshotDto::validate`] rejects versions it does not
/// know, so a newer producer's snapshot is never silently mis-parsed.
///
/// **Rule**: changing the field set of [`SimState`] requires bumping
/// **both** this constant and [`crate::model::SIM_STATE_SCHEMA_VERSION`].
/// The two describe different formats and both travel inside one
/// document; bumping only one of them lets an old reader accept a new
/// document and silently mis-parse it.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Default per-quantum guest instruction budget.
///
/// Mirrors the engine's own default so that a default-constructed
/// [`EngineSnapshotDto`] describes a plausible engine rather than one
/// with a budget of zero.
pub const DEFAULT_INSTRUCTION_BUDGET: u32 = 10_000;

/// Error family used by snapshot validation failures.
pub const SNAPSHOT_FAMILY: ErrorFamily = ErrorFamily::Snapshot;

/// Scenario code: the snapshot's `schema_version` is not the one this
/// build understands.
pub const E_SNAP_01_UNSUPPORTED_SCHEMA: u16 = 1;

/// Scenario code: the pending event queue is malformed (not canonically
/// ordered, duplicated `seq`, or `next_seq` would collide with a queued
/// event).
pub const E_SNAP_02_MALFORMED_QUEUE: u16 = 2;

/// Scenario code: the backend does not support complete (deterministic)
/// snapshots. Raised by the default implementations of
/// [`crate::port::ExecutionBackend::snapshot_full`] and
/// [`crate::port::ExecutionBackend::restore_full`].
pub const E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT: u16 = 3;

/// Scenario code: the snapshot's insertion-order mirror
/// (`state.event_queue_head`) disagrees with its authoritative counterpart
/// (`next_seq`).
pub const E_SNAP_04_QUEUE_HEAD_MISMATCH: u16 = 4;

/// Scenario code: the embedded [`SimState`] carries a format version this
/// build does not understand.
pub const E_SNAP_05_UNSUPPORTED_STATE_SCHEMA: u16 = 5;

/// Serializable mirror of a scheduled event.
///
/// The variants mirror the engine's internal event kinds one-for-one.
/// Variant names are `snake_case` on the wire.
///
/// **The `at_ps` inside a payload is not the same thing as the queue
/// entry's `at_ps`.** The engine normalises a late event's *queue entry*
/// to `max(at_ps, now)`, but leaves the payload's own `at_ps` at the
/// value the producer supplied. The two can therefore legitimately differ
/// — see [`QueuedEventDto::at_ps`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedEventDto {
    /// A peripheral asks to be ticked at the given virtual time.
    PeripheralTick {
        /// Physical address of the peripheral's register window.
        paddr: u32,
        /// The **producer's** intended tick time — the value the peripheral
        /// asked for, *not* the normalised queue-entry time.
        ///
        /// This is the value handed to the peripheral-tick callback, so it
        /// is **live** state: changing it changes when the peripheral is
        /// ticked. It may differ from [`QueuedEventDto::at_ps`].
        at_ps: u64,
    },
    /// A timer expiry.
    TimerExpiry {
        /// IRQ line raised on expiry.
        irq: u16,
        /// The **producer's** intended expiry time. Kept only so this DTO
        /// mirrors the engine's `SchedEvent` one-for-one — the engine's
        /// dispatch does **not** read it (the queue entry's time drives
        /// the clock), so it is inert.
        at_ps: u64,
    },
    /// A pending memory write to commit at the next quantum boundary.
    Commit(PendingWrite),
}

impl Default for SchedEventDto {
    fn default() -> Self {
        Self::TimerExpiry { irq: 0, at_ps: 0 }
    }
}

/// One queued event in snapshot form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedEventDto {
    /// The queue entry's **authoritative** due time (virtual time, ps) —
    /// the `at_ps` half of the canonical dequeue sort key `(at_ps, seq)`.
    ///
    /// This is what the engine's clock actually follows, and it is
    /// normalised: a late event is pulled forward to `max(at_ps, now)`.
    /// The payload's own `at_ps` (see [`SchedEventDto`]) keeps the
    /// producer's value and may differ.
    pub at_ps: u64,
    /// Insertion order — the stable tiebreaker for events due at the
    /// same virtual time, and the event's cancellation token.
    pub seq: u64,
    /// Event payload.
    pub event: SchedEventDto,
}

impl Default for QueuedEventDto {
    fn default() -> Self {
        Self {
            at_ps: 0,
            seq: 0,
            event: SchedEventDto::default(),
        }
    }
}

/// A **complete engine-level** snapshot — enough to restore the *engine*
/// to the state it was in.
///
/// Unlike [`SimState`], this carries the pending event queue, so a
/// snapshot/restore pair built on this type is a deterministic round-trip
/// **of the engine**: queue contents, queue order, the insertion counter
/// and the virtual time all survive (D-05 / FR-12).
///
/// **Scope limit**: it does **not** capture peripheral models (M-15) or
/// the interrupt controller (M-16) — see the module documentation for the
/// full list. Persisting only this DTO reproduces the engine, **not** a
/// whole session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineSnapshotDto {
    /// Format version of this DTO family — see [`SNAPSHOT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Platform state: registers, PC, CSRs, pending writes, virtual time.
    pub state: SimState,
    /// Pending events, in canonical dequeue order (`at_ps` ascending,
    /// ties broken by `seq` ascending).
    pub queue: Vec<QueuedEventDto>,
    /// The insertion order the *next* scheduled event will receive.
    pub next_seq: u64,
    /// Per-quantum guest instruction budget.
    pub instruction_budget: u32,
}

impl Default for EngineSnapshotDto {
    fn default() -> Self {
        Self {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            state: SimState::default(),
            queue: Vec::new(),
            next_seq: 0,
            // Mirrors the engine's own default (`DEFAULT_INSTRUCTION_BUDGET`)
            // so a default-constructed DTO still describes a plausible
            // engine, instead of a budget of 0 that no engine would have.
            instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
        }
    }
}

impl EngineSnapshotDto {
    /// Check the snapshot's internal consistency.
    ///
    /// A snapshot may come from an untrusted store (IndexedDB in the
    /// browser, a server-side store in P1), so a malformed queue must be
    /// rejected rather than silently loaded — otherwise the determinism
    /// guarantee (FR-12) is lost without anybody noticing.
    ///
    /// The checks are:
    /// 1. [`Self::schema_version`] equals [`SNAPSHOT_SCHEMA_VERSION`];
    /// 2. the embedded [`SimState`]'s own `schema_version` equals
    ///    [`crate::model::SIM_STATE_SCHEMA_VERSION`] — the two versions
    ///    describe **different** formats and both travel in the same
    ///    document, so both must be checked;
    /// 3. the queue is in canonical dequeue order — `at_ps` ascending,
    ///    ties broken by `seq` ascending;
    /// 4. no two queued events share a `seq`;
    /// 5. [`Self::next_seq`] is strictly above every queued `seq`, so
    ///    scheduling after a restore can never collide with a queued
    ///    event (D-17);
    /// 6. [`SimState::event_queue_head`] equals [`Self::next_seq`] — the
    ///    DTO stores the insertion counter twice, and the engine always
    ///    writes the two equal. A snapshot where they differ did not come
    ///    from this engine, and silently picking one of them would make
    ///    the outcome depend on which field the reader happened to trust.
    ///
    /// Returns `Ok(())` when the snapshot is self-consistent.
    pub fn validate(&self) -> Result<(), PlatformError> {
        if self.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(PlatformError::new(
                SNAPSHOT_FAMILY,
                E_SNAP_01_UNSUPPORTED_SCHEMA,
                format!(
                    "unsupported snapshot schema version {} (this build understands {})",
                    self.schema_version, SNAPSHOT_SCHEMA_VERSION
                ),
            ));
        }

        if self.state.schema_version != crate::model::SIM_STATE_SCHEMA_VERSION {
            return Err(PlatformError::new(
                SNAPSHOT_FAMILY,
                E_SNAP_05_UNSUPPORTED_STATE_SCHEMA,
                format!(
                    "unsupported SimState schema version {} (this build understands {})",
                    self.state.schema_version,
                    crate::model::SIM_STATE_SCHEMA_VERSION
                ),
            ));
        }

        if self.state.event_queue_head != self.next_seq {
            return Err(PlatformError::new(
                SNAPSHOT_FAMILY,
                E_SNAP_04_QUEUE_HEAD_MISMATCH,
                format!(
                    "insertion-order mirror disagrees: state.event_queue_head = {}, next_seq = {}",
                    self.state.event_queue_head, self.next_seq
                ),
            ));
        }

        let mut previous: Option<(u64, u64)> = None;
        let mut seen_seqs = std::collections::BTreeSet::new();
        for event in &self.queue {
            let key = (event.at_ps, event.seq);
            if let Some(prev) = previous {
                if key <= prev {
                    return Err(PlatformError::new(
                        SNAPSHOT_FAMILY,
                        E_SNAP_02_MALFORMED_QUEUE,
                        format!(
                            "event queue is not in canonical order: ({}, {}) follows ({}, {})",
                            key.0, key.1, prev.0, prev.1
                        ),
                    ));
                }
            }
            // The canonical order is keyed on `(at_ps, seq)`, so it cannot
            // by itself catch two events sharing a `seq` at *different*
            // times — the `seq` must be unique on its own.
            if !seen_seqs.insert(event.seq) {
                return Err(PlatformError::new(
                    SNAPSHOT_FAMILY,
                    E_SNAP_02_MALFORMED_QUEUE,
                    format!("duplicated event seq {} in the queue", event.seq),
                ));
            }
            if event.seq >= self.next_seq {
                return Err(PlatformError::new(
                    SNAPSHOT_FAMILY,
                    E_SNAP_02_MALFORMED_QUEUE,
                    format!(
                        "queued seq {} is not below next_seq {} — restoring would reuse a sequence number",
                        event.seq, self.next_seq
                    ),
                ));
            }
            previous = Some(key);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WriteSize;

    fn sample_snapshot() -> EngineSnapshotDto {
        EngineSnapshotDto {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            state: SimState {
                schema_version: 1,
                virtual_time_ps: 1_000,
                pc: 0x8000_0004,
                gpr: [0u32; 32],
                csrs: vec![(0x300, 0x0000_0000)],
                pending_writes: vec![PendingWrite {
                    paddr: 0x2000_0000,
                    size: WriteSize::Word,
                    value: 0xdead_beef,
                    source: "UART0".to_string(),
                }],
                event_queue_head: 3,
                seed: 42,
            },
            queue: vec![
                QueuedEventDto {
                    at_ps: 2_000,
                    seq: 0,
                    event: SchedEventDto::PeripheralTick {
                        paddr: 0x4000_0000,
                        at_ps: 2_000,
                    },
                },
                QueuedEventDto {
                    at_ps: 3_000,
                    seq: 1,
                    event: SchedEventDto::TimerExpiry {
                        irq: 7,
                        at_ps: 3_000,
                    },
                },
                QueuedEventDto {
                    at_ps: 3_000,
                    seq: 2,
                    event: SchedEventDto::Commit(PendingWrite {
                        paddr: 0x2000_0004,
                        size: WriteSize::Byte,
                        value: 0x11,
                        source: "GPIO0".to_string(),
                    }),
                },
            ],
            next_seq: 3,
            instruction_budget: 10_000,
        }
    }

    // The two JSON tests need `serde_json`, which is an **optional**
    // dependency of this crate (the `json` feature). The crate must stay
    // buildable — and testable — with `--no-default-features`, so they are
    // feature-gated rather than unconditional.
    //
    // Note: the DTOs themselves only need `serde`, which is a *non-optional*
    // dependency — the "serializable" promise holds with or without the
    // `json` feature. Only the JSON *text* round-trip needs `serde_json`.
    #[cfg(feature = "json")]
    #[test]
    fn snapshot_dto_round_trips_through_json() {
        let snapshot = sample_snapshot();
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let back: EngineSnapshotDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snapshot, back);
    }

    #[cfg(feature = "json")]
    #[test]
    fn snapshot_dto_json_carries_the_event_queue() {
        let json = serde_json::to_string(&sample_snapshot()).expect("serialize");
        // The queue must be visible on the wire — that is the whole point
        // of the DTO (D-24).
        assert!(json.contains("\"queue\""), "queue missing from {json}");
        // Note the **quoted** key form: a bare `contains("commit")` also
        // matches `"commit_write"`, so it would not notice an event
        // variant being renamed — which would make every previously
        // persisted snapshot unreadable.
        assert!(
            json.contains("\"peripheral_tick\":"),
            "event kind missing: {json}"
        );
        assert!(
            json.contains("\"timer_expiry\":"),
            "event kind missing: {json}"
        );
        assert!(json.contains("\"commit\":"), "event kind missing: {json}");
    }

    #[cfg(feature = "json")]
    #[test]
    fn snapshot_dto_json_shape_is_pinned_exactly() {
        // The DTO exists to be a **persisted format**. Its wire shape must
        // therefore be pinned exactly, not by loose substring matching.
        //
        // Independent review V-A found that renaming the `Commit` variant
        // to `CommitWrite` — a pure wire-format break that would make
        // every stored snapshot unreadable — passed the **entire** test
        // suite, because the only shape assertion was `contains("commit")`
        // and `"commit_write".contains("commit")` is true.
        let dto = EngineSnapshotDto {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            state: SimState {
                schema_version: crate::model::SIM_STATE_SCHEMA_VERSION,
                virtual_time_ps: 0,
                gpr: [0u32; 32],
                pc: 0,
                csrs: Vec::new(),
                pending_writes: Vec::new(),
                event_queue_head: 2,
                seed: 0,
            },
            queue: vec![
                QueuedEventDto {
                    at_ps: 10,
                    seq: 0,
                    event: SchedEventDto::PeripheralTick {
                        paddr: 1,
                        at_ps: 10,
                    },
                },
                QueuedEventDto {
                    at_ps: 20,
                    seq: 1,
                    event: SchedEventDto::TimerExpiry { irq: 2, at_ps: 20 },
                },
                QueuedEventDto {
                    at_ps: 30,
                    seq: 2,
                    event: SchedEventDto::Commit(PendingWrite {
                        paddr: 0x2000_0000,
                        size: WriteSize::Word,
                        value: 0x1234_5678,
                        source: "UART0".to_string(),
                    }),
                },
            ],
            next_seq: 3,
            instruction_budget: 10_000,
        };

        let json = serde_json::to_string(&dto).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json value");

        // 1. 顶层键集必须精确固定（serde_json 的 Map 按字典序排列）。
        let top_keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            top_keys,
            [
                "instruction_budget",
                "next_seq",
                "queue",
                "schema_version",
                "state"
            ],
            "顶层键集变了 —— 落盘格式是破坏性变更"
        );

        // 2. 队列部分的**精确** JSON：事件变体名、字段名、字段顺序全部锁定。
        //    直接序列化 `dto.queue`（而不是经由 `serde_json::Value` 中转 ——
        //    后者会把键按字典序重排，测的就不是真实线上顺序了）。
        //    三种事件变体**全部**出现在这里：漏掉任何一种，那个变体就仍然
        //    没有被钉住。
        let queue_json = serde_json::to_string(&dto.queue).expect("queue json");
        assert_eq!(
            queue_json,
            r#"[{"at_ps":10,"seq":0,"event":{"peripheral_tick":{"paddr":1,"at_ps":10}}},{"at_ps":20,"seq":1,"event":{"timer_expiry":{"irq":2,"at_ps":20}}},{"at_ps":30,"seq":2,"event":{"commit":{"paddr":536870912,"size":"Word","value":305419896,"source":"UART0"}}}]"#,
            "队列的线上形状变了 —— 旧快照将无法被读回"
        );

        // 3. 状态部分的键集同样固定。
        let state_keys: Vec<&str> = value["state"]
            .as_object()
            .expect("state object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            state_keys,
            [
                "csrs",
                "event_queue_head",
                "gpr",
                "pc",
                "pending_writes",
                "schema_version",
                "seed",
                "virtual_time_ps"
            ],
            "状态部分的键集变了"
        );
    }

    #[test]
    fn validate_accepts_a_consistent_snapshot() {
        assert!(sample_snapshot().validate().is_ok());
    }

    #[test]
    fn validate_rejects_an_unknown_schema_version() {
        let mut snapshot = sample_snapshot();
        snapshot.schema_version = SNAPSHOT_SCHEMA_VERSION + 1;
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.family, SNAPSHOT_FAMILY);
        assert_eq!(err.scenario, E_SNAP_01_UNSUPPORTED_SCHEMA);
    }

    #[test]
    fn validate_rejects_a_queue_out_of_canonical_order() {
        let mut snapshot = sample_snapshot();
        snapshot.queue.swap(0, 1);
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.family, SNAPSHOT_FAMILY);
        assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
    }

    #[test]
    fn validate_rejects_a_duplicated_seq() {
        let mut snapshot = sample_snapshot();
        snapshot.queue[1].seq = snapshot.queue[0].seq;
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
    }

    #[test]
    fn validate_rejects_a_next_seq_that_would_collide() {
        let mut snapshot = sample_snapshot();
        snapshot.next_seq = 2; // collides with the queued seq 2
        snapshot.state.event_queue_head = 2; // keep the mirror consistent
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
    }

    #[test]
    fn validate_rejects_a_queue_head_that_disagrees_with_next_seq() {
        // The DTO stores the insertion counter twice. A snapshot where the
        // two disagree did not come from this engine — silently trusting
        // one of them would make the outcome depend on which field the
        // reader happened to read.
        let mut snapshot = sample_snapshot();
        snapshot.state.event_queue_head = 999;
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.family, SNAPSHOT_FAMILY);
        assert_eq!(err.scenario, E_SNAP_04_QUEUE_HEAD_MISMATCH);
    }

    #[test]
    fn validate_rejects_an_unsupported_state_schema_version() {
        // `SimState` carries its own format version, and it travels in the
        // same document as the DTO's version. Both must be checked.
        let mut snapshot = sample_snapshot();
        snapshot.state.schema_version = crate::model::SIM_STATE_SCHEMA_VERSION + 1;
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.family, SNAPSHOT_FAMILY);
        assert_eq!(err.scenario, E_SNAP_05_UNSUPPORTED_STATE_SCHEMA);
    }

    #[test]
    fn validate_rejects_a_queue_with_a_descending_seq_at_the_same_time() {
        // The canonical order is `(at_ps, seq)` — the `seq` tiebreaker
        // must be enforced too, not just the `at_ps` half. Without this
        // check a queue with two events due at the same time but with
        // descending `seq` would pass, and their dequeue order would then
        // be whatever the heap happened to produce.
        //
        // The `seq` values here are deliberately **distinct but
        // descending** — a duplicate would be caught by the separate
        // uniqueness check, which would mask a broken tiebreaker.
        let mut snapshot = sample_snapshot();
        snapshot.queue[1].seq = 5;
        snapshot.queue[2].seq = 3; // same at_ps as queue[1], lower seq
        snapshot.next_seq = 10; // keep every queued seq below next_seq
        snapshot.state.event_queue_head = 10;

        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
    }

    #[test]
    fn validate_accepts_an_empty_queue() {
        let snapshot = EngineSnapshotDto::default();
        assert!(snapshot.validate().is_ok());
    }
}
