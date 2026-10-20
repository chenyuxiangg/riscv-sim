//! M-20 快照与确定性事件流记录 (Snapshot & Deterministic Event Flow).
//!
//! Owns the persistence of [`SimState`](riscv_sim_contract::model::SimState)
//! snapshots to the host's storage layer (IndexedDB in browser, FS in
//! server mode). Snapshots are reproducible: the deterministic
//! `seed + time-stamped input event sequence` is enough to replay
//! from scratch (AD §8.1, LLD §4.20).
//!
//! Status: stub — persistence backend is TODO(MVP).

use riscv_sim_contract::model::SimState;

/// A recorded snapshot, plus the metadata needed to reload it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    /// Stable id (monotonic or UUID).
    pub id: u64,
    /// Captured virtual time (ps).
    pub at_ps: u64,
    /// The state itself.
    pub state: SimState,
    /// Free-form human label.
    pub label: String,
}

impl Default for SnapshotRecord {
    fn default() -> Self {
        Self {
            id: 0,
            at_ps: 0,
            state: SimState::default(),
            label: String::new(),
        }
    }
}

/// Snapshot manager. Holds recent snapshots in memory; the actual
/// persistence is delegated to M-23 (DataStore) once that lands.
#[derive(Debug, Default)]
pub struct SnapshotManager {
    next_id: u64,
    records: Vec<SnapshotRecord>,
}

impl SnapshotManager {
    /// New empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture a snapshot of the given state at the given virtual time.
    pub fn capture(
        &mut self,
        state: &SimState,
        at_ps: u64,
        label: impl Into<String>,
    ) -> SnapshotRecord {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let rec = SnapshotRecord {
            id,
            at_ps,
            state: state.clone(),
            label: label.into(),
        };
        self.records.push(rec.clone());
        rec
    }

    /// Look up a snapshot by id.
    pub fn get(&self, id: u64) -> Option<&SnapshotRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Iterate all stored snapshots, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &SnapshotRecord> {
        self.records.iter().rev()
    }
}

/// Family used by errors raised from this module.
///
/// Re-exported from the contract crate, which owns the error-code families
/// — defining a second constant with the same name and value here would
/// make the two ambiguous the moment a module imports both.
pub use riscv_sim_contract::snapshot::SNAPSHOT_FAMILY;
