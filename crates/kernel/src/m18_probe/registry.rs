//! Probe registry — orchestrator (M-18 v0.8 §3, §5, §6, §7).
//!
//! Owns:
//! - the channel map (one [`ChannelDef`] per `ch`),
//! - the sample configuration (master enable + per-channel enable
//!   bitmask + per-channel throttle counter),
//! - the per-channel edge-event store (drained by `drain_waveform`),
//! - the bus trace ring buffer (drained by `drain_trace`),
//! - a small diagnostics counter for attach conflicts (informational
//!   only — the registry does not raise `PlatformError` on conflict
//!   because M-18 §6.1 commits to "no new error code"; conflicts log
//!   a warning and are ignored).
//!
//! Edge judgment rules (M-18 v0.8 §3.3, §18-3, §18-4):
//! 1. `master_enable == false` → return `None` (zero-overhead path).
//! 2. Channel not in enable bitmask → return `None`.
//! 3. Per-channel throttle: every Nth observation emits (the rest
//!    are dropped at the source).
//! 4. Pulse channels emit on every observation (no value comparison,
//!    no throttle interaction other than #3).
//! 5. Same-value observation on a SingleBit / MultiBit channel →
//!    no event (the canonical edge-only contract).
//! 6. Different-value observation → emit with kind from
//!    `ChannelDef::classify`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{Edge, EdgeEvent, TraceDir, TraceEvent, Waveform, WaveformStream};
use riscv_sim_contract::port::{EdgeSink, ProbeSink};

use crate::m14_bus::{AccessDir, BusAccess, BusHook};

use super::channel::{ChannelDef, ChannelKind};
use super::trace_stream::TraceStream;

/// Errors raised by the probe registry. M-18 v0.8 §6.1 commits to
/// "no new error code" for the M-18 module itself, but the registry
/// still surfaces *attachment* errors as `PlatformError` so callers
/// can decide how to handle the situation (log + skip vs. panic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// `attach_signal` (or `define_channel`) tried to register a
    /// channel number that is already in the map. M-18 v0.8 §18-6:
    /// attach-time conflict is rejected; the existing binding is
    /// preserved.
    ChannelConflict { ch: u8, existing_source: String },
}

impl ProbeError {
    /// Family used in the `PlatformError` we surface (Observability,
    /// the same family M-18 declares in the contract `ErrorFamily`
    /// mapping).
    pub fn family(&self) -> ErrorFamily {
        ErrorFamily::Observability
    }

    /// Numeric scenario within the family. We pick a stable, small
    /// value that the frontend can match on.
    pub fn scenario(&self) -> u16 {
        match self {
            ProbeError::ChannelConflict { .. } => 1, // OBS-01
        }
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::ChannelConflict {
                ch,
                existing_source,
            } => {
                write!(f, "channel {ch} already bound to {existing_source}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

impl From<ProbeError> for PlatformError {
    fn from(e: ProbeError) -> Self {
        PlatformError::new(e.family(), e.scenario(), e.to_string())
    }
}

/// Sampling configuration (M-18 v0.8 §2.1.1, §3.4).
///
/// `master_enable` is the global performance switch. `enable` is a
/// bitmask indexed by channel number (bit `ch` controls channel `ch`).
/// `throttle_n` is the per-channel throttle coefficient (1 = no
/// throttle, 4 = every 4th observation emits, etc.). The registry
/// also tracks a "drop oldest" overflow policy for the waveform
/// stream (the bus trace stream has its own ring buffer with its
/// own drop policy in [`TraceStream`]).
#[derive(Debug, Clone)]
pub struct SampleCfg {
    /// Performance switch — `false` makes `sample` a zero-overhead no-op.
    pub master_enable: bool,
    /// Bitmask indexed by channel. Bit `ch` enables channel `ch`.
    pub enable: u32,
    /// Per-channel throttle coefficient. The struct carries a single
    /// value because the design doc's `setRate` sets one global
    /// throttle; per-channel overrides are not in the MVP (§3.4
    /// "sampleCfg.throttleN" is a single field).
    pub throttle_n: u8,
}

impl Default for SampleCfg {
    fn default() -> Self {
        // Defaults: all off, master off, no throttle. The caller
        // (typically M-01 boot or a test) turns things on explicitly.
        Self {
            master_enable: false,
            enable: 0,
            throttle_n: 1,
        }
    }
}

impl SampleCfg {
    /// True if channel `ch` is enabled in the bitmask.
    pub fn is_enabled(&self, ch: u8) -> bool {
        (self.enable >> ch) & 1 != 0
    }

    /// Enable a single channel. Convenience for tests + the demo
    /// wiring path.
    pub fn enable_channel(&mut self, ch: u8) {
        self.enable |= 1 << ch;
    }
}

/// Probe registry: the central M-18 orchestrator.
///
/// The registry is single-threaded (`!Send` by virtue of `BTreeMap`
/// not requiring it; the closure types are also non-`Send`). The
/// `Rc<RefCell<>>` wrappers around it let it participate in bus
/// hook closures without moving ownership.
///
/// `Debug` is implemented by hand because the optional edge sink
/// (M-19 v0.9 §1.1) is a `Box<dyn EdgeSink>`, and trait objects do
/// not implement `Debug`.
pub struct ProbeRegistry {
    channels: BTreeMap<u8, ChannelDef>,
    /// Source-and-signal index for fast periph-side routing:
    /// `lookup[(source, signal)] = ch`. The reverse direction
    /// (`channels: ch → def`) is still authoritative; this index is
    /// a cache.
    lookup: BTreeMap<(String, String), u8>,
    cfg: SampleCfg,
    /// Per-channel previous value (for edge judgment). `None` =
    /// "not yet observed" — the first observation always emits
    /// because we have no prior to compare against (the design
    /// doc's pseudo-code leaves `prev` uninitialized, which would
    /// silently drop the first observation on every channel; we
    /// fix that here).
    prev: BTreeMap<u8, Option<u64>>,
    /// Per-channel throttle counter (counts down from `throttle_n` to
    /// 0; emit only when it hits 0, then reset to `throttle_n`).
    throttle: BTreeMap<u8, u8>,
    /// Per-channel accumulated edge events (drained by
    /// [`ProbeRegistry::drain_waveform`]).
    edges: BTreeMap<u8, Vec<Edge>>,
    /// Bus trace ring buffer.
    trace: TraceStream,
    /// Diagnostics: count of attach-time conflicts encountered.
    /// Increments inside `define_channel` / `install_signal_probe`
    /// when a caller passes a `ch` that's already in the map.
    conflict_warns: u32,
    /// Diagnostics: count of times a probe callback tried to
    /// `borrow_mut()` the registry while the caller was holding
    /// another borrow. The callback drops the observation in this
    /// case (M-18 review #3, 2026-10-19: avoid the
    /// `RefCell::borrow_mut()` panic in the closure hot path).
    ///
    /// Stored in a separate `Rc<Cell<u64>>` so the closure can
    /// increment it without going through the contended borrow
    /// (chicken/egg). The accessor `borrow_contended()` returns
    /// the cell's current value.
    borrow_contended: Rc<Cell<u64>>,
    /// Optional downstream edge sink (M-19 v0.9 §1.1).
    ///
    /// This is the M-18 → M-19 handoff: the M-19 design doc's *"M-18 的
    /// m19Encode 即本模块 encodeEdge 入口"*. When a sink is installed,
    /// every edge the registry emits is also handed to it as an
    /// [`EdgeEvent`] carrying the classified [`EdgeKind`].
    ///
    /// `None` by default so the registry stays standalone and pays
    /// nothing when no waveform consumer is attached.
    edge_sink: Option<Box<dyn EdgeSink>>,
}

impl Default for ProbeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ProbeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProbeRegistry")
            .field("channels", &self.channels)
            .field("cfg", &self.cfg)
            .field(
                "edge_channels",
                &self.edges.keys().copied().collect::<Vec<_>>(),
            )
            .field("trace_len", &self.trace.len())
            .field("conflict_warns", &self.conflict_warns)
            .field("borrow_contended", &self.borrow_contended.get())
            .field("has_edge_sink", &self.edge_sink.is_some())
            .finish_non_exhaustive()
    }
}

impl ProbeRegistry {
    /// New empty registry. No channels, no edges, default sample
    /// configuration (master off, all channels off).
    pub fn new() -> Self {
        Self {
            channels: BTreeMap::new(),
            lookup: BTreeMap::new(),
            cfg: SampleCfg::default(),
            prev: BTreeMap::new(),
            throttle: BTreeMap::new(),
            edges: BTreeMap::new(),
            trace: TraceStream::default(),
            conflict_warns: 0,
            borrow_contended: Rc::new(Cell::new(0)),
            edge_sink: None,
        }
    }

    /// Number of channels currently bound.
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Diagnostics: cumulative count of attach-time conflicts.
    pub fn conflict_warns(&self) -> u32 {
        self.conflict_warns
    }

    /// Diagnostics: cumulative count of probe-callback borrow
    /// contentions (M-18 review #3). When a peripheral fires a
    /// `ProbeSink` while the caller is still holding a `borrow()`
    /// on the registry, the callback drops the observation and
    /// bumps this counter instead of panicking.
    pub fn borrow_contended(&self) -> u64 {
        self.borrow_contended.get()
    }

    /// Read-only access to the sample configuration.
    pub fn sample_cfg(&self) -> &SampleCfg {
        &self.cfg
    }

    /// Total number of edges currently buffered across all channels
    /// (useful for tests + diagnostics).
    pub fn edge_count(&self) -> usize {
        self.edges.values().map(|v| v.len()).sum()
    }

    /// Number of bus trace events currently in the ring.
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// Number of bus trace events dropped due to ring overflow.
    pub fn trace_dropped(&self) -> u64 {
        self.trace.dropped()
    }

    // -----------------------------------------------------------------
    // Channel definition / attach
    // -----------------------------------------------------------------

    /// Register a channel. Returns `Err(ChannelConflict)` if a
    /// channel with the same `ch` is already defined; the existing
    /// binding is preserved (M-18 v0.8 §18-6).
    ///
    /// **Auto-enable side effect (M-18 review #6, 2026-10-19):**
    /// `define_channel` sets the channel's bit in
    /// `SampleCfg::enable`, so the new channel is *active* as soon
    /// as `master_enable` is on. If you want to register a batch
    /// of channels and then enable a specific subset, follow up
    /// with [`Self::set_rate`] to clear the bits you don't want
    /// (e.g. `cfg.enable &= !(1 << CH_GPIO0)`).
    pub fn define_channel(&mut self, def: ChannelDef) -> Result<(), ProbeError> {
        // Clone out the existing source first so the immutable
        // borrow on `self.channels` ends before the mutable
        // borrow on `self.conflict_warns`.
        let existing_source = self.channels.get(&def.ch).map(|d| d.source.clone());
        if let Some(src) = existing_source {
            self.conflict_warns += 1;
            return Err(ProbeError::ChannelConflict {
                ch: def.ch,
                existing_source: src,
            });
        }
        let key = (def.source.clone(), def.signal.clone());
        self.lookup.insert(key, def.ch);
        self.cfg.enable_channel(def.ch); // auto-enable (caller can clear)
        self.channels.insert(def.ch, def);
        Ok(())
    }

    /// Build a [`BusHook`] closure that routes every bus access
    /// into `record_bus_access`. The hook holds an `Rc<RefCell<>>`
    /// to the registry so the same instance can service the bus and
    /// be observed by the caller.
    ///
    /// Note: takes the registry as an `Rc<RefCell<>>` parameter
    /// (not `self: Rc<RefCell<Self>>`) because Rust does not
    /// support `Rc<RefCell<Self>>` as a method receiver type.
    pub fn make_bus_hook(reg: Rc<RefCell<Self>>) -> BusHook {
        let me = Rc::clone(&reg);
        Box::new(move |access: &BusAccess| {
            me.borrow_mut().record_bus_access(access);
        })
    }

    /// Convenience wiring: build a probe callback that routes
    /// periph-side signal observations into the channel bound to
    /// `(periph_id, signal)` (M-18 v0.8 §3.2, §18-2, additive
    /// contract method `Peripheral::on_probe`).
    ///
    /// Returns the `Box<dyn FnMut>` for the caller to pass into
    /// `Peripheral::on_probe(Some(cb))`. The closure filters
    /// signal-name mismatches so a peripheral that misuses the
    /// callback (passes a different signal name) does not pollute
    /// the wrong channel.
    ///
    /// Note: this function does **not** enforce channel-conflict
    /// semantics — that lives in [`Self::define_channel`], which is
    /// the only place where a `ch` can be claimed. Re-calling
    /// `wire_signal_probe` for an already-attached `(periph_id,
    /// signal)` is allowed and replaces the previous sink; this
    /// matches the "Peripheral::on_probe accepts an Option<…> and
    /// replaces the existing callback" contract.
    ///
    /// ## Hot-path performance (M-18 review #1, 2026-10-19)
    ///
    /// The returned closure **routes the observation directly to
    /// `sample_inner(ch, …)`** — it does not go through
    /// `record_signal(periph_id, signal, …)`. This avoids two
    /// `String::to_string()` allocations per observation in the
    /// GPIO / UART / Timer hot path. The `(periph_id, signal) → ch`
    /// mapping is recorded in the lookup table at install time so
    /// ad-hoc callers (e.g. tests) can still use `record_signal`.
    ///
    /// ## Borrow-contention safety (M-18 review #3, 2026-10-19)
    ///
    /// The closure uses `try_borrow_mut()` instead of
    /// `borrow_mut()`. If the caller is currently holding a
    /// `borrow()` on the registry, the closure drops the
    /// observation and increments `borrow_contended` rather than
    /// panicking. The M-15 ↔ M-18 wiring will produce high-
    /// frequency callbacks; a panic in the hot path would be
    /// unrecoverable from a peripheral interrupt context.
    pub fn wire_signal_probe(
        reg: Rc<RefCell<Self>>,
        periph_id: &str,
        signal: &str,
        ch: u8,
    ) -> ProbeSink {
        // Record the channel association in the lookup table so
        // `record_signal` (the ad-hoc path) can route the
        // observation. The lookup key is `(periph_id, signal)`;
        // we replace any previous entry for the same key (the
        // contract allows callback replacement).
        {
            let mut borrow = reg.borrow_mut();
            borrow
                .lookup
                .insert((periph_id.to_string(), signal.to_string()), ch);
        }
        let me = Rc::clone(&reg);
        let contention_cell = Rc::clone(&reg.borrow().borrow_contended);
        // `periph_id` is captured for the lookup-table path only;
        // the closure below does not use it (it goes via
        // `sample_inner` directly). We still capture to keep the
        // borrow checker happy with the existing `Box<dyn FnMut>`.
        let _periph_id = periph_id.to_string();
        let signal = signal.to_string();
        Box::new(move |sig, value, now_ps| {
            if sig != signal {
                return; // signal-name mismatch: filter at the boundary
            }
            // Hot path: skip the lookup, skip the record_signal
            // double-allocation. If the borrow is contended (the
            // caller holds a `borrow()` on `me`), drop the
            // observation and bump the diagnostic counter
            // instead of panicking.
            if let Ok(mut r) = me.try_borrow_mut() {
                let _ = r.sample_inner(now_ps, ch, value);
            } else {
                // Contended: increment the separate counter cell
                // and drop the observation. We do NOT spin or
                // block; the closure runs in a peripheral
                // callback context where blocking would deadlock
                // or extend the borrow past the call site.
                contention_cell.set(contention_cell.get() + 1);
            }
        })
    }

    // -----------------------------------------------------------------
    // Recording (called by the bus hook + periph callbacks)
    // -----------------------------------------------------------------

    /// Record one bus access into the trace ring buffer. The hook
    /// filter is trivial: every access is recorded (M-18 v0.8 §2.1
    /// "全量访存事件流"). Errored accesses (`ok == false`) are
    /// also recorded so the trace shows the bus-error spike pattern.
    pub fn record_bus_access(&mut self, access: &BusAccess) {
        let width = access.width.bytes() as u8; // BusWidth: Byte=1, Half=2, Word=4
        let dir = match access.dir {
            AccessDir::Read => TraceDir::Read,
            AccessDir::Write => TraceDir::Write,
        };
        let ev = TraceEvent {
            t_ps: access.ts_ps,
            addr: access.addr,
            width,
            dir,
        };
        let _ = self.trace.push(ev);
    }

    /// Record one periph-side signal observation. The registry
    /// looks up the channel bound to `(periph_id, signal)`; if no
    /// binding exists, the call is silently dropped (the design
    /// doc §6.1 says "unbound signal: log + drop").
    pub fn record_signal(&mut self, periph_id: &str, signal: &str, value: u64, now_ps: u64) {
        let ch = match self
            .lookup
            .get(&(periph_id.to_string(), signal.to_string()))
        {
            Some(&ch) => ch,
            None => return, // unbound: silently drop
        };
        let _ = self.sample_inner(now_ps, ch, value);
    }

    /// Direct `sample()` call (M-18 v0.8 §3.3, table 69). The
    /// central edge-judgment function; every other entry point
    /// (`record_signal`, `record_irq_pulse`, future timers) routes
    /// through here.
    ///
    /// Returns the emitted edge event, or `None` if the observation
    /// was filtered (master off, channel off, throttled, or a
    /// same-value repeat on a non-pulse channel).
    pub fn sample(&mut self, time_ps: u64, ch: u8, value: u64) -> Option<Edge> {
        self.sample_inner(time_ps, ch, value)
    }

    /// `sample()` for the IRQ-pulse channel (M-18 v0.8 §3.5, N2).
    /// Same semantics as the general `sample`; the convenience
    /// exists so the call site reads "irq_pulse" instead of a
    /// bare channel number.
    ///
    /// **Preconditions (M-18 review #7, 2026-10-19):**
    ///
    /// The caller must have previously invoked
    /// [`Self::define_channel`] with `ch == CH_IRQ_PULSE` and
    /// `kind == ChannelKind::Pulse`. If the channel is undefined
    /// or registered with a non-pulse kind, this method silently
    /// returns `None` — there is no way to distinguish "no pulse
    /// arrived" from "channel not registered". This is by design
    /// (the registry does not auto-create channels to keep channel
    /// ownership explicit), but callers should set up the channel
    /// once at boot and not rely on the silent no-op path.
    pub fn sample_irq_pulse(&mut self, time_ps: u64, irq_line: u64) -> Option<Edge> {
        // CH_IRQ_PULSE must be defined as a pulse channel for this
        // to be meaningful. The edge judgment rules (pulse → always
        // emit) do not depend on the channel being registered as
        // Pulse, only on `kind == Pulse`; we re-check that here for
        // the no-channel case.
        let kind = self
            .channels
            .get(&super::channel::CH_IRQ_PULSE)
            .map(|d| d.kind);
        match kind {
            Some(ChannelKind::Pulse) => {
                self.sample_inner(time_ps, super::channel::CH_IRQ_PULSE, irq_line)
            }
            // If the caller forgot to register the pulse channel we
            // surface a no-op rather than emit on an undefined
            // channel. The registry does not auto-register to keep
            // channel ownership explicit.
            _ => None,
        }
    }

    fn sample_inner(&mut self, time_ps: u64, ch: u8, value: u64) -> Option<Edge> {
        // 1. Master switch — zero-overhead path.
        if !self.cfg.master_enable {
            return None;
        }
        // 2. Per-channel enable.
        if !self.cfg.is_enabled(ch) {
            return None;
        }
        // 3. Per-channel throttle. We do this *before* the kind
        //    lookup so a throttled observation is not even compared
        //    against `prev` — that comparison would force a load
        //    on the hot path that throttling is supposed to avoid.
        let throttle_n = self.cfg.throttle_n.max(1);
        let counter = self.throttle.entry(ch).or_insert(throttle_n);
        if *counter > 1 {
            *counter -= 1;
            return None;
        }
        *counter = throttle_n;

        // 4. Look up the channel definition. If the caller routed
        //    to an unknown channel, drop.
        let def = self.channels.get(&ch)?;

        // 5. Edge judgment. `prev` is `None` on the first
        //    observation for a channel — the first observation
        //    always emits, because we have no prior to compare
        //    against. After that, same-value on a non-pulse
        //    channel is filtered.
        let prev = self.prev.get(&ch).copied().flatten();
        let kind = def.kind;
        let classified = match kind {
            ChannelKind::Pulse => {
                // Pulse: every observation emits, regardless of
                // value. We still update `prev` so the consumer
                // can see the latest value if it cares.
                let p = prev.unwrap_or(value);
                Some(def.classify(p, value))
            }
            ChannelKind::SingleBit | ChannelKind::MultiBit => match prev {
                None => Some(def.classify(value, value)), // first sample
                Some(p) if p == value => None,
                Some(p) => Some(def.classify(p, value)),
            },
        };

        // 6. Update `prev` and emit if we classified an edge.
        self.prev.insert(ch, Some(value));
        // The classified kind is forwarded to the downstream edge
        // sink (M-19) below. It used to be discarded here
        // (M-18 review #4) because the contract's `Edge` struct
        // carries only `{t_ps, value}` and nothing downstream could
        // hold the kind. The M-19 v0.9 handoff now gives it a
        // carrier (`EdgeEvent`), so the information is no longer
        // lost.
        let kind = classified?;
        let edge = Edge {
            t_ps: time_ps,
            value,
        };
        self.edges.entry(ch).or_default().push(edge.clone());
        // 7. Hand the edge to the optional M-19 sink. This is the
        //    "m19Encode → encodeEdge" entry the M-19 design doc
        //    describes (M-19 v0.9 §1.1). The `Option` check keeps
        //    the no-consumer path free of any extra work.
        if let Some(sink) = self.edge_sink.as_mut() {
            sink.encode_edge(EdgeEvent {
                t_ps: time_ps,
                ch,
                value,
                kind,
            });
        }
        Some(edge)
    }

    // -----------------------------------------------------------------
    // setRate + drain
    // -----------------------------------------------------------------

    /// Apply a new sample configuration (M-18 v0.8 §3.4, table 69).
    /// Throttle counters are *not* reset; the new `throttle_n` takes
    /// effect on the next `sample` call after each channel's counter
    /// reaches 0.
    pub fn set_rate(&mut self, cfg: SampleCfg) {
        self.cfg = cfg;
    }

    /// Install (or remove) the downstream edge sink (M-19 v0.9 §1.1).
    ///
    /// The sink receives every edge the registry emits, together with
    /// the classified [`EdgeKind`]. M-19's `WaveformPipeline`
    /// implements the [`EdgeSink`] port; the wiring code installs it
    /// once at boot:
    ///
    /// ```ignore
    /// let pipeline = SharedPipeline::new(WaveformPipeline::new());
    /// reg.set_edge_sink(Some(Box::new(pipeline.clone())));
    /// ```
    ///
    /// Pass `None` to detach. While no sink is installed the registry
    /// behaves exactly as before — the check in `sample_inner` is a
    /// single `Option` test on the hot path.
    pub fn set_edge_sink(&mut self, sink: Option<Box<dyn EdgeSink>>) {
        self.edge_sink = sink;
    }

    /// True when a downstream edge sink is installed.
    pub fn has_edge_sink(&self) -> bool {
        self.edge_sink.is_some()
    }

    /// Drain all per-channel edge events into a [`Waveform`]. The
    /// per-channel `Vec<Edge>`s are emptied; the channel map and
    /// other state are untouched.
    pub fn drain_waveform(&mut self) -> Waveform {
        let mut streams = Vec::new();
        // Iterate in channel-number order so the output is stable
        // (matters for tests + diffable exports).
        for (ch, def) in self.channels.iter() {
            let edges = self.edges.remove(ch).unwrap_or_default();
            let id = format!("{}.{}", def.source, def.signal);
            streams.push(WaveformStream {
                label: id.clone(),
                id,
                edges,
            });
        }
        Waveform {
            schema_version: 1,
            streams,
        }
    }

    /// Drain the bus trace ring buffer. Returns events in time
    /// order; the ring is empty after the call but `dropped` and
    /// `seq` lifetime counters are preserved.
    pub fn drain_trace(&mut self) -> Vec<riscv_sim_contract::model::TraceEvent> {
        self.trace.drain()
    }

    /// Inspect a single channel's accumulated edges without
    /// draining. Useful for tests and incremental UI updates.
    pub fn edges_for(&self, ch: u8) -> &[Edge] {
        self.edges.get(&ch).map(|v| v.as_slice()).unwrap_or(&[])
    }
}
