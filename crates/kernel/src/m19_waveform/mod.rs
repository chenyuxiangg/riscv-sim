//! M-19 波形管道 (Waveform Pipeline).
//!
//! Implements the "探针 → 边沿编码 → 环形缓冲 → 增量推送 → 前端渲染 /
//! trace 导出" pipeline — the differentiation core of the platform
//! (M-19 v0.9 §1.1, FR-10).
//!
//! ## What this module owns
//!
//! | Concern | Type | Design ref |
//! | --- | --- | --- |
//! | Edge encoding + per-channel history window | [`ChannelRing`] | §2.1 table 71 |
//! | Batched push + monotonic sequence numbers | [`DeltaQueue`] | §2.1 table 71 |
//! | Cross-channel merge + chunked CSV / VCD export | [`export`] | §3.4 |
//! | Overflow policy, memory budget, orchestration | [`WaveformPipeline`] | §3.1-§3.5 |
//!
//! ## Boundaries
//!
//! - **Read-only observation.** M-19 never mutates simulation state.
//!   The only control-plane action it can request is a *pause* through
//!   the [`PauseHook`], which stops instruction execution without
//!   touching registers or memory (review 19-9).
//! - **No timer of its own.** M-12's event loop is the scheduler: it
//!   calls [`WaveformPipeline::push_delta`] from its periodic callback
//!   and supplies the authoritative virtual time (review 19-5). That
//!   keeps M-19 free of wall-clock dependencies, which is what makes
//!   the pipeline deterministic and replayable.
//! - **No new error codes.** The module reuses the existing
//!   `Observability` family (`E_OBS-01`, invalid configuration) and
//!   the existing `E_CTX-02` (resource limit, memory budget).
//!
//! ## Module layout
//!
//! ```text
//! m19_waveform/
//! ├── mod.rs       — re-exports + module doc
//! ├── channel.rs   — ChannelMeta, OverflowPolicy, WireFormat
//! ├── ring.rs      — RingEntry, ChannelRing, RingIter
//! ├── delta.rs     — DeltaQueue (batch + sequence + drop ranges)
//! ├── export.rs    — ExportFilter, MergedCursor, CSV / VCD renderers
//! ├── pipeline.rs  — WaveformPipeline, SharedPipeline
//! └── tests.rs     — unit tests (LLD §7.1)
//! ```

pub mod channel;
pub mod delta;
pub mod export;
pub mod pipeline;
pub mod ring;

pub use channel::{ChannelMeta, OverflowPolicy, WireFormat};
pub use delta::{DeltaQueue, DropError, DROPPED_HISTORY_CAP};
pub use export::{
    count_filtered, csv_escape, render_csv, render_vcd, vcd_label, ChannelSet, ExportFilter,
    ExportFormat, ExportHandle, ExportMeta, ExportSession, MergedCursor, EXPORT_CHUNK,
};
pub use pipeline::{
    EncodeOutcome, PauseHook, PipelineStats, RateHook, SharedPipeline, WaveformPipeline,
    BUDGET_CHANNELS, DEFAULT_BATCH_MS, MEM_BUDGET_BYTES, UNCONSUMED_HISTORY_CAP,
};
pub use ring::{ChannelRing, RingEntry, RingIter, ENTRY_BYTES, RING_CAP};

use riscv_sim_contract::error::ErrorFamily;

/// Family used by configuration errors raised from this module
/// (M-19 v0.9 §6.1).
///
/// M-19 commits to "no new error codes": it reuses the existing
/// `Observability` family for configuration errors and the existing
/// `Context` family for the memory-budget limit.
pub const WAVEFORM_FAMILY: ErrorFamily = ErrorFamily::Observability;

/// Family used by resource-limit errors raised from this module
/// (M-19 v0.9 §3.5 — the memory-budget reconcile returns `E_CTX-02`).
pub const WAVEFORM_BUDGET_FAMILY: ErrorFamily = ErrorFamily::Context;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
