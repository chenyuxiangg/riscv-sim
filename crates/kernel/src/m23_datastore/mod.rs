//! M-23 数据存储与持久化 DataStore (Data Store & Persistence).
//!
//! Owns the persistence of `Project`s and `SnapshotRecord`s. The
//! MVP backend is the browser `IndexedDB` (front-end) or a local
//! SQLite file (server). The choice is made at host bootstrap; the
//! kernel never sees the backend directly.
//!
//! Status: stub — trait adopted; the two backends are TODO(MVP).

use crate::m20_snapshot::SnapshotRecord;
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::Project;

/// Storage backend. Implementations are out-of-tree at MVP and
/// selected by the host (front-end or server).
pub trait DataStore {
    /// Persist a project.
    fn put_project(&mut self, p: &Project) -> Result<(), PlatformError>;
    /// Load a project by id.
    fn get_project(&self, id: &str) -> Result<Option<Project>, PlatformError>;
    /// Persist a snapshot.
    fn put_snapshot(&mut self, s: &SnapshotRecord) -> Result<(), PlatformError>;
    /// Load a snapshot by id.
    fn get_snapshot(&self, id: u64) -> Result<Option<SnapshotRecord>, PlatformError>;
    /// List all known project ids, newest first.
    fn list_projects(&self) -> Result<Vec<String>, PlatformError>;
}

/// Family used by errors raised from this module.
pub const STORE_FAMILY: ErrorFamily = ErrorFamily::Storage;
