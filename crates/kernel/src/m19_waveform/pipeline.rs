//! Waveform pipeline orchestrator (M-19 v0.9 §3, §5).
//!
//! Implements the "探针 → 边沿编码 → 环形缓冲 → 增量推送 → 前端渲染 /
//! trace 导出" pipeline (M-19 v0.9 §1.1).
//!
//! ## Data flow
//!
//! ```text
//! M-18 edge event
//!   → encode_edge        (edge encoding; only change points)
//!   → ChannelRing        (per-channel history window, drop-oldest + mark)
//!   → DeltaQueue         (pending push batch)
//!   → push_delta         (≤ batch_ms batch, monotonic sequence number)
//!   → DeltaSink          (M-03 W1 subscriber)
//! ```
//!
//! ## Read-only observation
//!
//! M-19 never mutates simulation state. The one control-plane action
//! it can request is a *pause* (through the [`PauseHook`]), which
//! stops instruction execution without touching registers or memory
//! (M-19 v0.9 §1.3, review 19-9).
//!
//! ## Scheduling
//!
//! M-19 does **not** own a timer. The design doc makes M-12's event
//! loop the scheduler: the engine calls [`WaveformPipeline::push_delta`]
//! from its periodic callback (every `batch_ms`, default 20 ms) and
//! supplies the authoritative virtual time. That keeps M-19 free of
//! wall-clock dependencies, which is what makes the pipeline
//! deterministic and replayable (M-19 v0.9 §8).

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{DropRange, EdgeEvent, WaveformDelta};
use riscv_sim_contract::port::{DeltaSink, EdgeSink};

use super::channel::{invalid_config, ChannelMeta, OverflowPolicy, WireFormat};
use super::delta::DeltaQueue;
use super::export::{
    count_filtered, render_csv, render_vcd, ExportFilter, ExportFormat, ExportHandle, ExportMeta,
    MergedCursor, EXPORT_CHUNK,
};
use super::ring::{ChannelRing, RingEntry, ENTRY_BYTES, RING_CAP};

/// Default channel count the memory budget is sized for (M-19 v0.9 §2.1).
pub const BUDGET_CHANNELS: u32 = 4;

/// Memory budget baseline in bytes (M-19 v0.9 §2.1, review 19-2).
///
/// `4 channels × 2×10⁵ entries × 32 B` = 25.6 MB. The budget is a
/// **capacity-planning** figure: with fixed-size rings the runtime
/// footprint can never exceed the full capacity, so there is no
/// runtime over-budget state — the check happens when the caller
/// plans a new channel or a larger window.
pub const MEM_BUDGET_BYTES: u64 = BUDGET_CHANNELS as u64 * RING_CAP as u64 * ENTRY_BYTES;

/// Default push batch window in milliseconds (M-19 v0.9 §2.1, §3.2).
pub const DEFAULT_BATCH_MS: u32 = 20;

/// Hook invoked when the `PAUSE_SIM` overflow policy trips.
///
/// The argument is the channel number whose ring overflowed. The
/// wiring binds this to M-12's pause semantics; until M-12 grows a
/// pause API the hook may be left unset, in which case the overflow
/// is still counted and reported.
pub type PauseHook = Box<dyn FnMut(u8)>;

/// Hook invoked when the memory-budget reconcile forces downsampling.
///
/// The argument is the new throttle coefficient (observations
/// emitted per Nth sample). The wiring binds this to M-18's
/// `set_rate`.
pub type RateHook = Box<dyn FnMut(u8)>;

/// Outcome of encoding one edge event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeOutcome {
    /// True when the channel's ring was full and the oldest entry was
    /// evicted.
    pub overflowed: bool,
    /// True when the overflow triggered a simulation-pause request.
    pub pause_requested: bool,
}

/// Aggregate statistics of the pipeline (diagnostics + UI readout).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipelineStats {
    /// Number of channels with a ring buffer.
    pub channel_count: usize,
    /// Total number of entries currently held across all rings.
    pub buffered: usize,
    /// Total number of entries evicted by overflow.
    pub overflow_total: u64,
    /// Number of events waiting for the next push.
    pub pending: usize,
    /// Next sequence number that will be assigned.
    pub seq_no: u64,
    /// Number of dropped push batches.
    pub dropped_batches: u64,
    /// Number of simulation-pause requests.
    pub pause_requests: u64,
}

/// The M-19 waveform pipeline.
pub struct WaveformPipeline {
    /// Rendering metadata per channel.
    channels: BTreeMap<u8, ChannelMeta>,
    /// Per-channel history ring.
    rings: BTreeMap<u8, ChannelRing>,
    /// Pending push batch + sequence bookkeeping.
    delta: DeltaQueue,
    /// Overflow policy.
    policy: OverflowPolicy,
    /// Wire format of the push payload.
    wire_format: WireFormat,
    /// Per-channel ring capacity.
    ring_cap: usize,
    /// Push batch window, in milliseconds.
    batch_ms: u32,
    /// Current probe throttle coefficient (used by the budget
    /// reconcile to compute the downsampled rate).
    throttle_n: u8,
    /// Optional subscriber for the pushed deltas.
    delta_sink: Option<Box<dyn DeltaSink>>,
    /// Optional simulation-pause hook.
    pause_hook: Option<PauseHook>,
    /// Optional probe-rate hook (M-18 `set_rate`).
    rate_hook: Option<RateHook>,
    /// Virtual time of the last push.
    last_push_ps: Option<u64>,
    /// Cumulative pause requests.
    pause_requests: u64,
    /// Cumulative budget reconciles that forced downsampling.
    budget_reconciles: u64,
}

impl Default for WaveformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for WaveformPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaveformPipeline")
            .field("channels", &self.channels)
            .field("policy", &self.policy)
            .field("wire_format", &self.wire_format)
            .field("ring_cap", &self.ring_cap)
            .field("batch_ms", &self.batch_ms)
            .field("seq_no", &self.delta.seq_no())
            .field("pending", &self.delta.pending_len())
            .field("dropped_batches", &self.delta.dropped_batches())
            .field("pause_requests", &self.pause_requests)
            .finish_non_exhaustive()
    }
}

impl WaveformPipeline {
    /// New pipeline with the MVP defaults: 2×10⁵ entries per channel,
    /// drop-oldest overflow policy, 20 ms push batch, binary wire
    /// format with a fixed-length header.
    pub fn new() -> Self {
        Self {
            channels: BTreeMap::new(),
            rings: BTreeMap::new(),
            delta: DeltaQueue::new(),
            policy: OverflowPolicy::default(),
            wire_format: WireFormat::default(),
            ring_cap: RING_CAP,
            batch_ms: DEFAULT_BATCH_MS,
            throttle_n: 1,
            delta_sink: None,
            pause_hook: None,
            rate_hook: None,
            last_push_ps: None,
            pause_requests: 0,
            budget_reconciles: 0,
        }
    }

    /// New pipeline with a custom per-channel ring capacity.
    ///
    /// Panics when `cap == 0` — a zero-capacity ring could never
    /// record anything.
    pub fn with_ring_capacity(cap: usize) -> Self {
        assert!(
            cap > 0,
            "waveform ring capacity must be > 0; use new() for the MVP default"
        );
        let mut p = Self::new();
        p.ring_cap = cap;
        p
    }

    // -----------------------------------------------------------------
    // Channel definition
    // -----------------------------------------------------------------

    /// Declare a channel (label + value width).
    ///
    /// Declaring a channel is what gives the exporter a label and a
    /// VCD variable width. It is *not* required for the pipeline to
    /// work: an event for an undeclared channel is still recorded,
    /// using [`ChannelMeta::fallback`] metadata. The wiring code
    /// calls this once per M-18 channel at boot.
    ///
    /// Re-declaring a channel updates its metadata (it is not an
    /// error), so a boot sequence that runs twice is harmless.
    pub fn define_channel(&mut self, ch: u8, meta: ChannelMeta) {
        self.channels.insert(ch, meta);
        self.rings
            .entry(ch)
            .or_insert_with(|| ChannelRing::with_capacity(self.ring_cap));
    }

    /// Metadata of a channel, if declared.
    pub fn channel_meta(&self, ch: u8) -> Option<&ChannelMeta> {
        self.channels.get(&ch)
    }

    /// Declared channels, in channel-number order.
    pub fn channel_ids(&self) -> Vec<u8> {
        self.channels.keys().copied().collect()
    }

    // -----------------------------------------------------------------
    // encodeEdge — the M-18 → M-19 entry point
    // -----------------------------------------------------------------

    /// Encode one edge event: write it into the channel's ring buffer
    /// and accumulate it into the pending push batch.
    ///
    /// This is the implementation of the M-19 design doc's
    /// `encodeEdge(ev)`, and the target of the
    /// [`riscv_sim_contract::port::EdgeSink`] port that M-18 calls.
    ///
    /// Only change points reach this function — M-18 has already
    /// filtered same-value repeats. M-19 does not re-filter: it
    /// encodes what it is given.
    pub fn encode_edge(&mut self, ev: EdgeEvent) -> EncodeOutcome {
        // Unknown channel: fall back to default metadata rather than
        // dropping the observation. Losing an event because of a
        // wiring mistake is worse than labelling it generically.
        self.channels
            .entry(ev.ch)
            .or_insert_with(|| ChannelMeta::fallback(ev.ch));
        let ring = self
            .rings
            .entry(ev.ch)
            .or_insert_with(|| ChannelRing::with_capacity(self.ring_cap));

        let overflowed = ring.push(RingEntry::from(ev));

        // Both policies evict the oldest entry (the push above already
        // did it). The difference is whether the simulation is asked
        // to pause as well.
        let pause_requested = if overflowed && self.policy == OverflowPolicy::PauseSim {
            self.pause_requests += 1;
            if let Some(hook) = self.pause_hook.as_mut() {
                hook(ev.ch);
            }
            true
        } else {
            false
        };

        // The event is always accumulated into the push batch: the
        // batch is the live stream the frontend consumes, and the
        // observation already happened.
        self.delta.enqueue(ev);

        EncodeOutcome {
            overflowed,
            pause_requested,
        }
    }

    // -----------------------------------------------------------------
    // pushDelta — batched, throttled, monotonic push
    // -----------------------------------------------------------------

    /// Push the accumulated batch.
    ///
    /// `now_ps` is the authoritative virtual time (M-12 is the
    /// source; M-19 does not read a clock of its own). The batch
    /// window is `batch_ms`; the caller — normally M-12's periodic
    /// callback — decides when to call this.
    ///
    /// Returns `None` when nothing is pending: an empty batch must
    /// not wake the consumer up.
    ///
    /// When a [`DeltaSink`] is installed and the push fails, the batch
    /// is **dropped** (observation events are explicitly allowed to
    /// be lost, review P-09) and the lost sequence range is recorded
    /// in the drop-range list. The sequence space still advances, so
    /// a consumer detects the hole through the drop ranges rather
    /// than through a gap in the numbers.
    pub fn push_delta(&mut self, now_ps: u64) -> Option<WaveformDelta> {
        let delta = self.delta.take_batch(now_ps)?;
        self.last_push_ps = Some(now_ps);

        let failed = match self.delta_sink.as_mut() {
            Some(sink) => sink.push_delta(&delta).is_err(),
            None => false,
        };
        if failed {
            self.delta.record_drop(DropRange {
                seq_start: delta.seq_start,
                seq_end: delta.seq_end,
            });
        }
        Some(delta)
    }

    /// True when the batch window has elapsed and a push is due.
    ///
    /// The first batch is due immediately (there is no previous push
    /// to measure from); afterwards the window is measured from the
    /// last push. A pipeline with nothing pending is never due.
    pub fn batch_due(&self, now_ps: u64) -> bool {
        if self.delta.is_empty() {
            return false;
        }
        match self.last_push_ps {
            None => true,
            Some(last) => now_ps.saturating_sub(last) >= self.batch_window_ps(),
        }
    }

    /// The batch window in picoseconds.
    pub fn batch_window_ps(&self) -> u64 {
        u64::from(self.batch_ms) * 1_000_000
    }

    /// Virtual time of the last push, if any.
    pub fn last_push_ps(&self) -> Option<u64> {
        self.last_push_ps
    }

    /// Install (or remove) the delta subscriber.
    pub fn set_delta_sink(&mut self, sink: Option<Box<dyn DeltaSink>>) {
        self.delta_sink = sink;
    }

    /// Install (or remove) the simulation-pause hook.
    pub fn set_pause_hook(&mut self, hook: Option<PauseHook>) {
        self.pause_hook = hook;
    }

    /// Install (or remove) the probe-rate hook (M-18 `set_rate`).
    pub fn set_rate_hook(&mut self, hook: Option<RateHook>) {
        self.rate_hook = hook;
    }

    // -----------------------------------------------------------------
    // Policy configuration
    // -----------------------------------------------------------------

    /// Set the overflow policy. Takes effect immediately for the
    /// events that follow.
    pub fn set_overflow_policy(&mut self, policy: OverflowPolicy) {
        self.policy = policy;
    }

    /// Set the overflow policy from its wire code.
    ///
    /// An unknown code is rejected and the current policy is kept
    /// (M-19 v0.9 §6.1 "无效策略").
    pub fn set_overflow_policy_code(&mut self, code: u8) -> Result<(), PlatformError> {
        match OverflowPolicy::from_code(code) {
            Some(p) => {
                self.set_overflow_policy(p);
                Ok(())
            }
            None => Err(invalid_config("overflow policy", code)),
        }
    }

    /// The current overflow policy.
    pub fn overflow_policy(&self) -> OverflowPolicy {
        self.policy
    }

    /// Set the push payload wire format.
    pub fn set_wire_format(&mut self, format: WireFormat) {
        self.wire_format = format;
    }

    /// The current wire format.
    pub fn wire_format(&self) -> WireFormat {
        self.wire_format
    }

    /// Set the push batch window, in milliseconds.
    ///
    /// The design doc fixes the window at ≤20 ms (the main-thread
    /// merge budget). Larger values are accepted for experiments but
    /// push the frontend's ≤200 ms refresh budget at risk.
    pub fn set_batch_ms(&mut self, batch_ms: u32) {
        self.batch_ms = batch_ms;
    }

    /// The current batch window, in milliseconds.
    pub fn batch_ms(&self) -> u32 {
        self.batch_ms
    }

    /// Set the probe throttle coefficient the budget reconcile
    /// doubles when it forces downsampling.
    pub fn set_throttle_n(&mut self, throttle_n: u8) {
        self.throttle_n = throttle_n.max(1);
    }

    /// The current probe throttle coefficient.
    pub fn throttle_n(&self) -> u8 {
        self.throttle_n
    }

    // -----------------------------------------------------------------
    // Memory budget reconcile (M-19 v0.9 §3.5, review 19-2)
    // -----------------------------------------------------------------

    /// Reconcile a capacity plan against the memory budget.
    ///
    /// `used = channel_count × ring_cap × 32 B`. When the plan fits,
    /// this is a no-op. When it does not, the pipeline forces
    /// downsampling (the rate hook is called with a doubled throttle
    /// coefficient) and returns `E_CTX-02` (resource limit — an
    /// existing code, no new one is introduced).
    ///
    /// The check is a *planning* check, not a runtime check: with
    /// fixed-size rings the runtime footprint can never exceed the
    /// full capacity.
    pub fn reconcile_budget(
        &mut self,
        channel_count: u32,
        ring_cap: u32,
    ) -> Result<(), PlatformError> {
        let used = u64::from(channel_count) * u64::from(ring_cap) * ENTRY_BYTES;
        if used <= MEM_BUDGET_BYTES {
            return Ok(());
        }
        self.budget_reconciles += 1;
        let new_throttle = self.throttle_n.saturating_mul(2).max(1);
        self.throttle_n = new_throttle;
        if let Some(hook) = self.rate_hook.as_mut() {
            hook(new_throttle);
        }
        Err(PlatformError::new(
            ErrorFamily::Context,
            2,
            format!(
                "waveform memory budget exceeded: {used} B > {MEM_BUDGET_BYTES} B \
                 ({channel_count} channels x {ring_cap} entries); downsampled to \
                 throttle_n={new_throttle}"
            ),
        ))
    }

    /// Number of budget reconciles that forced downsampling.
    pub fn budget_reconciles(&self) -> u64 {
        self.budget_reconciles
    }

    // -----------------------------------------------------------------
    // Export (M-19 v0.9 §3.4)
    // -----------------------------------------------------------------

    /// Export one chunk of the waveform.
    ///
    /// `next_offset` is an index into the merged stream; pass the
    /// returned [`ExportHandle::next_offset`] back in to continue.
    /// `on_progress` is invoked with the completion percentage after
    /// the chunk is rendered.
    ///
    /// The merge requires each channel's entries to be in
    /// non-decreasing time order. The ring stores entries in
    /// *arrival* order, which equals time order because the virtual
    /// clock is monotonic — but a caller that feeds a stale timestamp
    /// would break that invariant, so the export path reorders any
    /// offending channel first (defensively, and without dropping
    /// anything).
    pub fn export(
        &mut self,
        format: ExportFormat,
        filter: &ExportFilter,
        next_offset: usize,
        on_progress: Option<&mut dyn FnMut(u8)>,
    ) -> ExportHandle {
        for ring in self.rings.values_mut() {
            ring.sort_by_time();
        }

        let total = count_filtered(&self.rings, filter);
        let mut cursor = MergedCursor::new(&self.rings, filter.clone());
        cursor.skip(next_offset);

        let mut entries: Vec<RingEntry> = Vec::new();
        while entries.len() < EXPORT_CHUNK {
            match cursor.next_entry() {
                Some(e) => entries.push(e),
                None => break,
            }
        }

        let consumed = next_offset + entries.len();
        let done = consumed >= total;
        let progress_pct = if total == 0 {
            100
        } else {
            ((consumed as u128 * 100) / total as u128).min(100) as u8
        };

        let overflow_marked = self.rings.values().any(|r| r.overflow_count() > 0);
        let meta = ExportMeta {
            total,
            overflow_marked,
            dropped_ranges: self.delta.dropped_ranges().to_vec(),
        };

        // The VCD header declares every channel that the filter can
        // produce, so the variable list is known before the first
        // entry is rendered.
        let vcd_channels: Vec<u8> = match &filter.channels {
            Some(list) => list.clone(),
            None => self.rings.keys().copied().collect(),
        };

        let chunk = match format {
            ExportFormat::Csv => render_csv(&entries, &self.channels, next_offset == 0, &meta),
            ExportFormat::Vcd => {
                render_vcd(&entries, &self.channels, &vcd_channels, next_offset == 0)
            }
        };

        if let Some(cb) = on_progress {
            cb(progress_pct);
        }

        ExportHandle {
            chunk,
            next_offset: consumed,
            total,
            done,
            progress_pct,
            overflow_marked,
            dropped_ranges: meta.dropped_ranges,
        }
    }

    /// Export a CSV chunk.
    pub fn export_csv(
        &mut self,
        filter: &ExportFilter,
        next_offset: usize,
        on_progress: Option<&mut dyn FnMut(u8)>,
    ) -> ExportHandle {
        self.export(ExportFormat::Csv, filter, next_offset, on_progress)
    }

    /// Export a VCD chunk.
    pub fn export_vcd(
        &mut self,
        filter: &ExportFilter,
        next_offset: usize,
        on_progress: Option<&mut dyn FnMut(u8)>,
    ) -> ExportHandle {
        self.export(ExportFormat::Vcd, filter, next_offset, on_progress)
    }

    // -----------------------------------------------------------------
    // Diagnostics
    // -----------------------------------------------------------------

    /// Aggregate statistics.
    pub fn stats(&self) -> PipelineStats {
        PipelineStats {
            channel_count: self.rings.len(),
            buffered: self.rings.values().map(|r| r.len()).sum(),
            overflow_total: self.rings.values().map(|r| r.overflow_count()).sum(),
            pending: self.delta.pending_len(),
            seq_no: self.delta.seq_no(),
            dropped_batches: self.delta.dropped_batches(),
            pause_requests: self.pause_requests,
        }
    }

    /// Number of entries evicted by overflow on one channel.
    pub fn overflow_count(&self, ch: u8) -> u64 {
        self.rings.get(&ch).map(|r| r.overflow_count()).unwrap_or(0)
    }

    /// Number of entries currently held for one channel.
    pub fn channel_len(&self, ch: u8) -> usize {
        self.rings.get(&ch).map(|r| r.len()).unwrap_or(0)
    }

    /// Ranges whose content was dropped by a failed push.
    pub fn dropped_ranges(&self) -> &[DropRange] {
        self.delta.dropped_ranges()
    }

    /// Number of dropped push batches.
    pub fn dropped_batches(&self) -> u64 {
        self.delta.dropped_batches()
    }
}

/// Implement the contract's [`EdgeSink`] port for the pipeline.
///
/// This is what lets the pipeline be installed directly into M-18's
/// registry (`ProbeRegistry::set_edge_sink`).
impl EdgeSink for WaveformPipeline {
    fn encode_edge(&mut self, ev: EdgeEvent) {
        WaveformPipeline::encode_edge(self, ev);
    }
}

/// A shared handle to a [`WaveformPipeline`].
///
/// The pipeline has to be reachable from two places at once: M-18's
/// registry (which produces the edges) and the host (which drives
/// the push, the export and the policy). [`SharedPipeline`] is the
/// `Rc<RefCell<…>>` wrapper that makes both possible.
///
/// The [`EdgeSink`] implementation uses `try_borrow_mut` and drops
/// the observation when the borrow is contended — the same lesson
/// M-18 learned in review #3: a panic in the probe hot path would be
/// unrecoverable from a peripheral callback context.
#[derive(Clone)]
pub struct SharedPipeline {
    inner: Rc<RefCell<WaveformPipeline>>,
    /// Count of observations dropped because the pipeline was
    /// borrowed elsewhere at that moment.
    contended: Rc<Cell<u64>>,
}

impl std::fmt::Debug for SharedPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedPipeline")
            .field("contended", &self.contended.get())
            .finish_non_exhaustive()
    }
}

impl SharedPipeline {
    /// Wrap a pipeline in a shared handle.
    pub fn new(pipeline: WaveformPipeline) -> Self {
        Self {
            inner: Rc::new(RefCell::new(pipeline)),
            contended: Rc::new(Cell::new(0)),
        }
    }

    /// Shared access to the pipeline.
    pub fn pipeline(&self) -> Ref<'_, WaveformPipeline> {
        self.inner.borrow()
    }

    /// Mutable shared access to the pipeline.
    pub fn pipeline_mut(&self) -> RefMut<'_, WaveformPipeline> {
        self.inner.borrow_mut()
    }

    /// Number of observations dropped because of a borrow conflict.
    pub fn contended(&self) -> u64 {
        self.contended.get()
    }
}

impl EdgeSink for SharedPipeline {
    fn encode_edge(&mut self, ev: EdgeEvent) {
        match self.inner.try_borrow_mut() {
            Ok(mut p) => {
                WaveformPipeline::encode_edge(&mut p, ev);
            }
            Err(_) => {
                // Contended: drop the observation and count it. Never
                // panic here — this runs in the probe hot path.
                self.contended.set(self.contended.get() + 1);
            }
        }
    }
}
