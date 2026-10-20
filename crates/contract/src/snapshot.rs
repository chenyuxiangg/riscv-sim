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
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Error family used by snapshot validation failures.
pub const SNAPSHOT_FAMILY: ErrorFamily = ErrorFamily::Snapshot;

/// Scenario code: the snapshot's `schema_version` is not the one this
/// build understands.
pub const E_SNAP_01_UNSUPPORTED_SCHEMA: u16 = 1;

/// Scenario code: the pending event queue is malformed (not canonically
/// ordered, duplicated `seq`, or `next_seq` would collide with a queued
/// event).
pub const E_SNAP_02_MALFORMED_QUEUE: u16 = 2;

/// Scenario code: the backend does not support deterministic snapshot
/// restore. Raised by the default implementation of
/// [`crate::port::ExecutionBackend::restore_full`].
pub const E_SNAP_03_UNSUPPORTED_RESTORE: u16 = 3;

/// Serializable mirror of a scheduled event.
///
/// The variants mirror the engine's internal event kinds one-for-one.
/// Variant names are `snake_case` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchedEventDto {
    /// A peripheral asks to be ticked at the given virtual time.
    PeripheralTick {
        /// Physical address of the peripheral's register window.
        paddr: u32,
        /// Virtual time (ps) at which the tick is due.
        at_ps: u64,
    },
    /// A timer expiry.
    TimerExpiry {
        /// IRQ line raised on expiry.
        irq: u16,
        /// Virtual time (ps) at which the timer expires.
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
    /// Event due time (virtual time, ps).
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

/// A **complete** engine snapshot — enough to restore the engine
/// bit-for-bit.
///
/// Unlike [`SimState`], this carries the pending event queue, so a
/// snapshot/restore pair built on this type is a deterministic round-trip:
/// queue contents, queue order, the insertion counter and the virtual
/// time all survive (D-05 / FR-12).
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
            instruction_budget: 0,
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
    /// 2. the queue is in canonical dequeue order — `at_ps` ascending,
    ///    ties broken by `seq` ascending;
    /// 3. no two queued events share a `seq`;
    /// 4. [`Self::next_seq`] is strictly above every queued `seq`, so
    ///    scheduling after a restore can never collide with a queued
    ///    event (D-17).
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
        assert!(
            json.contains("peripheral_tick"),
            "event kind missing: {json}"
        );
        assert!(json.contains("timer_expiry"), "event kind missing: {json}");
        assert!(json.contains("commit"), "event kind missing: {json}");
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
        let err = snapshot.validate().expect_err("must reject");
        assert_eq!(err.scenario, E_SNAP_02_MALFORMED_QUEUE);
    }

    #[test]
    fn validate_accepts_an_empty_queue() {
        let snapshot = EngineSnapshotDto::default();
        assert!(snapshot.validate().is_ok());
    }
}
