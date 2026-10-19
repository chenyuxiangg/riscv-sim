//! M-21 日志系统 (Logging System).
//!
//! In-process ring buffer of structured log records. Only the
//! admin channel surfaces logs in the front-end (LLD §4.21, P1
//! gate). The buffer is bounded so an OOM guest cannot exhaust
//! host memory.
//!
//! Status: stub — ring buffer + structured records are wired; the
//! bridge to `tracing` / `log` is TODO.

use riscv_sim_contract::error::ErrorFamily;
use std::collections::VecDeque;

/// Maximum number of log records kept in the ring buffer.
pub const MAX_LOG_RECORDS: usize = 4096;

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Diagnostic chatter.
    Trace,
    /// Useful operational info.
    Info,
    /// Recoverable anomaly.
    Warn,
    /// Recoverable error.
    Error,
    /// Unrecoverable; engine halted.
    Fatal,
}

impl Default for Level { fn default() -> Self { Self::Info } }

/// One log record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// Unix epoch in milliseconds.
    pub ts_ms: u64,
    /// Severity.
    pub level: Level,
    /// Source module id (e.g. `"m12_engine"`).
    pub source: String,
    /// Human-readable message.
    pub message: String,
}

impl Default for LogRecord {
    fn default() -> Self { Self { ts_ms: 0, level: Level::Info, source: String::new(), message: String::new() } }
}

/// Log buffer. Bounded by [`MAX_LOG_RECORDS`].
#[derive(Debug, Default)]
pub struct LogBuffer {
    records: VecDeque<LogRecord>,
}

impl LogBuffer {
    /// New empty buffer.
    pub fn new() -> Self { Self::default() }

    /// Push a record, dropping the oldest if the buffer is full.
    pub fn push(&mut self, rec: LogRecord) {
        if self.records.len() >= MAX_LOG_RECORDS {
            let _ = self.records.pop_front();
        }
        self.records.push_back(rec);
    }

    /// Drain all records (admin-only operation).
    pub fn drain(&mut self) -> Vec<LogRecord> { self.records.drain(..).collect() }

    /// Read-only access.
    pub fn iter(&self) -> impl Iterator<Item = &LogRecord> { self.records.iter() }
}

/// Family used by errors raised from this module.
pub const LOG_FAMILY: ErrorFamily = ErrorFamily::Context;
