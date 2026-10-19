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
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::{DropRange, EdgeEvent, WaveformDelta};
use riscv_sim_contract::port::{DeltaSink, EdgeSink};

use super::channel::{invalid_config, ChannelMeta, OverflowPolicy, WireFormat};
use super::delta::DeltaQueue;
use super::export::{ExportFilter, ExportFormat, ExportHandle, ExportSession};
use super::ring::{ChannelRing, RingEntry, ENTRY_BYTES, RING_CAP};

/// Default channel count the memory budget is sized for (M-19 v0.9 §2.1).
pub const BUDGET_CHANNELS: u32 = 4;

/// Memory budget baseline in bytes (M-19 v0.9 §2.1, review 19-2).
///
/// `4 channels × 2×10⁵ entries × 32 B` = 25.6 MB. The budget is a
/// **capacity-planning** figure: with fixed-size rings the runtime
/// footprint can never exceed the full capacity, so there is no
/// runtime over-budget state — the check happens when the caller
/// plans a new channel or a larger window, and when the pipeline
/// auto-creates a ring for an undeclared channel.
pub const MEM_BUDGET_BYTES: u64 = BUDGET_CHANNELS as u64 * RING_CAP as u64 * ENTRY_BYTES;

/// Default push batch window in milliseconds (M-19 v0.9 §2.1, §3.2).
pub const DEFAULT_BATCH_MS: u32 = 20;

/// Maximum number of remembered "unconsumed" batch ranges.
///
/// A batch pushed while no [`DeltaSink`] was installed is recorded
/// here so a consumer can discover what never reached a sink. The
/// list is bounded for the same reason the drop list is.
pub const UNCONSUMED_HISTORY_CAP: usize = 4096;

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
    /// True when the channel had no ring yet and the memory budget
    /// refused to create one (M-19 review #9). The event is still
    /// forwarded to the push stream; only the *history window* for
    /// that channel is missing.
    pub ring_refused: bool,
}

impl EncodeOutcome {
    /// The "nothing happened" outcome.
    pub const NONE: Self = Self {
        overflowed: false,
        pause_requested: false,
        ring_refused: false,
    };
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
    /// Number of dropped push batches (sink delivery failed).
    pub dropped_batches: u64,
    /// Number of batches pushed while no sink was installed.
    pub unconsumed_batches: u64,
    /// Number of simulation-pause requests.
    pub pause_requests: u64,
    /// Number of channels whose ring creation was refused by the
    /// memory budget.
    pub rings_refused: u64,
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
    /// Cumulative refused ring creations.
    rings_refused: u64,
    /// Batches pushed while no sink was installed.
    unconsumed: VecDeque<DropRange>,
    /// Outcome of the most recent `encode_edge`.
    last_outcome: EncodeOutcome,
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
            .field("unconsumed_batches", &self.unconsumed.len())
            .field("pause_requests", &self.pause_requests)
            .field("rings_refused", &self.rings_refused)
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
            rings_refused: 0,
            unconsumed: VecDeque::new(),
            last_outcome: EncodeOutcome::NONE,
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
        self.ensure_ring(ch);
    }

    /// Metadata of a channel, if declared.
    pub fn channel_meta(&self, ch: u8) -> Option<&ChannelMeta> {
        self.channels.get(&ch)
    }

    /// Declared channels, in channel-number order.
    pub fn channel_ids(&self) -> Vec<u8> {
        self.channels.keys().copied().collect()
    }

    /// Create the ring for `ch` if it does not exist yet.
    ///
    /// **Memory-budget guard (M-19 review #9):** the previous
    /// implementation created rings on demand from `encode_edge`,
    /// which let a host feed more channels than the 25 MB budget
    /// covers — the budget was only ever checked by the *planning*
    /// call `reconcile_budget`, so the automatic path bypassed it
    /// entirely.
    ///
    /// The check is in bytes: adding one more ring of `ring_cap`
    /// entries must keep the total planned footprint within
    /// [`MEM_BUDGET_BYTES`]. With the default capacity that is
    /// exactly [`BUDGET_CHANNELS`] channels.
    ///
    /// Returns `false` when the budget refuses the new ring.
    fn ensure_ring(&mut self, ch: u8) -> bool {
        if self.rings.contains_key(&ch) {
            return true;
        }
        let used = (self.rings.len() as u64 + 1) * self.ring_cap as u64 * ENTRY_BYTES;
        if used > MEM_BUDGET_BYTES {
            self.rings_refused += 1;
            return false;
        }
        self.rings
            .insert(ch, ChannelRing::with_capacity(self.ring_cap));
        true
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

        // The memory budget may refuse a new ring; the event is still
        // forwarded to the push stream, only its history window is
        // missing.
        let ring_refused = !self.ensure_ring(ev.ch);

        let mut outcome = EncodeOutcome {
            overflowed: false,
            pause_requested: false,
            ring_refused,
        };

        if let Some(ring) = self.rings.get_mut(&ev.ch) {
            outcome.overflowed = ring.push(RingEntry::from(ev));

            // Both policies evict the oldest entry (the push above
            // already did it). The difference is whether the
            // simulation is asked to pause as well.
            if outcome.overflowed && self.policy == OverflowPolicy::PauseSim {
                outcome.pause_requested = true;
                self.pause_requests += 1;
                if let Some(hook) = self.pause_hook.as_mut() {
                    hook(ev.ch);
                }
            }
        }

        // The event is always accumulated into the push batch: the
        // batch is the live stream the frontend consumes, and the
        // observation already happened.
        self.delta.enqueue(ev);

        self.last_outcome = outcome;
        outcome
    }

    /// Outcome of the most recent [`Self::encode_edge`].
    ///
    /// The [`EdgeSink`] port returns `()`, so the overflow /
    /// pause information is retrieved through this observer
    /// (M-19 review #14).
    pub fn last_outcome(&self) -> EncodeOutcome {
        self.last_outcome
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
    /// ## Delivery semantics (M-19 review #2)
    ///
    /// There are three outcomes, and they are **not** the same thing:
    ///
    /// | Situation | Content | Recorded as |
    /// | --- | --- | --- |
    /// | Sink installed, delivery succeeded | delivered | — |
    /// | Sink installed, delivery failed | **lost** | `dropped_ranges` (a hole the consumer refills over M-07 `resumeBySeq`) |
    /// | No sink installed | handed to the **direct caller** via the return value | `unconsumed_ranges` (queryable: "this never reached a sink") |
    ///
    /// The previous implementation treated the third case as a
    /// success: the batch was taken, the sequence space advanced, and
    /// **nothing** was recorded — so a consumer had no way to learn
    /// that the content was missing. That violated this module's own
    /// documented invariant.
    ///
    /// The sequence space advances in every case, so the *numbers*
    /// stay gap-free; the content holes live in the two recorded
    /// lists.
    pub fn push_delta(&mut self, now_ps: u64) -> Option<WaveformDelta> {
        let delta = self.delta.take_batch(now_ps)?;
        self.last_push_ps = Some(now_ps);

        match self.delta_sink.as_mut() {
            Some(sink) => {
                if sink.push_delta(&delta).is_err() {
                    // Delivery failed: the content is gone. Record
                    // the hole so the consumer can refill it.
                    let _ = self.delta.record_drop(DropRange {
                        seq_start: delta.seq_start,
                        seq_end: delta.seq_end,
                    });
                }
            }
            None => {
                // No sink: the batch is delivered to the direct
                // caller through the return value. Record the range
                // as "never reached a sink" so the situation is
                // visible instead of silent.
                self.record_unconsumed(DropRange {
                    seq_start: delta.seq_start,
                    seq_end: delta.seq_end,
                });
            }
        }
        Some(delta)
    }

    /// Record a batch that was pushed without a sink attached.
    fn record_unconsumed(&mut self, range: DropRange) {
        self.unconsumed.push_back(range);
        while self.unconsumed.len() > UNCONSUMED_HISTORY_CAP {
            self.unconsumed.pop_front();
        }
    }

    /// Ranges that were pushed while no sink was installed.
    ///
    /// These are the batches that never reached a subscriber. If the
    /// direct caller consumed them, they are simply informational; if
    /// it dropped them, they mark the content holes.
    pub fn unconsumed_ranges(&self) -> impl ExactSizeIterator<Item = &DropRange> {
        self.unconsumed.iter()
    }

    /// Number of batches pushed while no sink was installed.
    pub fn unconsumed_batches(&self) -> u64 {
        self.unconsumed.len() as u64
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
    /// full capacity. The *automatic* ring-creation path is guarded
    /// separately by [`Self::ensure_ring`].
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
            super::WAVEFORM_BUDGET_FAMILY,
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

    /// Number of channels whose ring creation the budget refused.
    pub fn rings_refused(&self) -> u64 {
        self.rings_refused
    }

    // -----------------------------------------------------------------
    // Export (M-19 v0.9 §3.4)
    // -----------------------------------------------------------------

    /// Start a resumable, chunked export.
    ///
    /// **This is the recommended path for sequential chunking.** The
    /// session keeps the merge cursor alive between chunks, so a full
    /// window export costs O(n) total instead of O(n²/chunk)
    /// (M-19 review #4).
    ///
    /// The session borrows the pipeline immutably for its lifetime:
    /// the export is a read-only snapshot of the window that existed
    /// when the session started, and the pipeline cannot be mutated
    /// while it is in flight.
    pub fn export_session(
        &mut self,
        format: ExportFormat,
        filter: ExportFilter,
    ) -> ExportSession<'_> {
        // The merge requires each channel's entries to be in
        // non-decreasing time order. The ring stores entries in
        // *arrival* order, which equals time order because the
        // virtual clock is monotonic — but a caller that feeds a
        // stale timestamp would break that invariant, so the export
        // path reorders any offending channel first (defensively, and
        // without dropping anything). Done **once** per session.
        for ring in self.rings.values_mut() {
            ring.sort_by_time();
        }
        let overflow_marked = self.rings.values().any(|r| r.overflow_count() > 0);
        let dropped = self.delta.dropped_ranges_vec();
        ExportSession::new(
            &self.rings,
            &self.channels,
            filter,
            format,
            overflow_marked,
            dropped,
        )
    }

    /// Export one chunk of the waveform.
    ///
    /// `next_offset` is an index into the merged stream; pass the
    /// returned [`ExportHandle::next_offset`] back in to continue.
    /// `on_progress` is invoked with the completion percentage after
    /// the chunk is rendered.
    ///
    /// **Cost:** resuming at `next_offset` re-walks the merged stream
    /// up to that point, so this entry point is O(next_offset) per
    /// call. It is meant for *resuming* an export at a known anchor
    /// and for small one-off exports. For sequential chunking over a
    /// large window use [`Self::export_session`], which is O(n) in
    /// total.
    pub fn export(
        &mut self,
        format: ExportFormat,
        filter: &ExportFilter,
        next_offset: usize,
        on_progress: Option<&mut dyn FnMut(u8)>,
    ) -> ExportHandle {
        let mut session = self.export_session(format, filter.clone());
        session.skip_to(next_offset);
        match on_progress {
            Some(cb) => session.next_chunk_with_progress(cb),
            None => session.next_chunk(),
        }
    }

    /// Export a CSV chunk. See [`Self::export`] for the cost model.
    pub fn export_csv(
        &mut self,
        filter: &ExportFilter,
        next_offset: usize,
        on_progress: Option<&mut dyn FnMut(u8)>,
    ) -> ExportHandle {
        self.export(ExportFormat::Csv, filter, next_offset, on_progress)
    }

    /// Export a VCD chunk. See [`Self::export`] for the cost model.
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
            unconsumed_batches: self.unconsumed.len() as u64,
            pause_requests: self.pause_requests,
            rings_refused: self.rings_refused,
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

    /// Ranges whose content was **lost** (a sink delivery failed).
    pub fn dropped_ranges(&self) -> impl ExactSizeIterator<Item = &DropRange> {
        self.delta.dropped_ranges()
    }

    /// Snapshot of the lost-content ranges.
    pub fn dropped_ranges_vec(&self) -> Vec<DropRange> {
        self.delta.dropped_ranges_vec()
    }

    /// Number of dropped push batches (sink delivery failed).
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

/// The shared pipeline could not be borrowed because it is already
/// borrowed elsewhere.
///
/// Returned by [`SharedPipeline::try_pipeline`] and
/// [`SharedPipeline::try_pipeline_mut`] (M-19 review #10: the two
/// accessors used to have different panic semantics — the hot path
/// was safe and the control path was not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorrowConflict;

impl std::fmt::Display for BorrowConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the waveform pipeline is already borrowed")
    }
}

impl std::error::Error for BorrowConflict {}

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
    ///
    /// **Panics** if the pipeline is currently mutably borrowed. Use
    /// [`Self::try_pipeline`] in a context where a panic would be
    /// unrecoverable — the probe hot path is the canonical example.
    pub fn pipeline(&self) -> Ref<'_, WaveformPipeline> {
        self.inner.borrow()
    }

    /// Mutable shared access to the pipeline.
    ///
    /// **Panics** if the pipeline is currently borrowed. Use
    /// [`Self::try_pipeline_mut`] in a context where a panic would be
    /// unrecoverable.
    pub fn pipeline_mut(&self) -> RefMut<'_, WaveformPipeline> {
        self.inner.borrow_mut()
    }

    /// Shared access to the pipeline, or an error when the borrow is
    /// contended.
    ///
    /// This is the non-panicking counterpart of [`Self::pipeline`]
    /// (M-19 review #10: the two accessors used to have different
    /// panic semantics, and the *control* side was the fragile one).
    pub fn try_pipeline(&self) -> Result<Ref<'_, WaveformPipeline>, BorrowConflict> {
        self.inner.try_borrow().map_err(|_| BorrowConflict)
    }

    /// Mutable shared access to the pipeline, or an error when the
    /// borrow is contended.
    ///
    /// This is the non-panicking counterpart of [`Self::pipeline_mut`].
    pub fn try_pipeline_mut(&self) -> Result<RefMut<'_, WaveformPipeline>, BorrowConflict> {
        self.inner.try_borrow_mut().map_err(|_| BorrowConflict)
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
