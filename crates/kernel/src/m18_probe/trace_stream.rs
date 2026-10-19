//! Bus trace ring buffer (M-18 v0.8 §2.1.1, §18-1, §18-5).
//!
//! A separate stream from the per-channel waveform edges: every bus
//! access is recorded here, even accesses that did not produce an
//! edge event (no value change, dropped, errored). The ring buffer
//! has a fixed capacity; when full, the oldest event is dropped and
//! the `dropped` counter is incremented.
//!
//! Why a separate stream?
//! - The waveform (per-channel edge events) is for *rendering* —
//!   only changes, and only on the channels the user is watching.
//! - The trace stream is for *M-11 export* (FR-14: instruction
//!   coverage / memory-access trace) — every access, with address
//!   and width, so external tooling can reconstruct a memory-access
//!   log without re-running the simulation.
//!
//! MVP capacity is `TRACE_CAP` (2×10⁵ events, the design-doc default).
//! This is a per-process constant; consumers that need a larger
//! window can re-instantiate the registry with a different cap
//! (the constant is `pub` for that reason).

use riscv_sim_contract::model::TraceEvent;

/// Default ring-buffer capacity (M-18 v0.8 §18-1: 2×10⁵).
///
/// The design doc calls this an "estimated baseline; calibrate after
/// the MVP lands". We keep the constant `pub` so a future caller can
/// opt for a different cap without forking the module.
pub const TRACE_CAP: usize = 200_000;

/// Ring buffer of [`TraceEvent`]s with FIFO overflow + drop counter.
///
/// Backed by a fixed-size array + write cursor; the oldest event is
/// silently dropped on overflow (M-18 v0.8 §18-1: "drop oldest + mark
/// dropped counter"; this stream is *not* subject to the M-19
/// overflow policy, which only governs per-channel edge events).
///
/// `Clone` is derived (M-18 review #14, 2026-10-19): the inner
/// `Vec<TraceEvent>` is cloneable and the bookkeeping integers are
/// `Copy`, so a snapshot of the stream is cheap.
#[derive(Debug, Clone)]
pub struct TraceStream {
    buf: Vec<TraceEvent>,
    head: usize, // next write index
    len: usize,  // number of valid entries (≤ buf.capacity())
    /// Monotonic sequence number, incremented on every successful
    /// push (not on overflow). M-11 uses this to track its read
    /// position across drain calls.
    seq: u64,
    /// Number of events dropped due to overflow (oldest-first).
    dropped: u64,
    /// Capacity locked at construction. We store it explicitly
    /// (rather than reading `buf.capacity()`) so a future change
    /// to the buffer type does not silently break the ring math
    /// (M-18 review #13, 2026-10-19). Asserted in `push` and
    /// `drain` to catch refactors early.
    initial_cap: usize,
}

impl Default for TraceStream {
    fn default() -> Self {
        Self::with_capacity(TRACE_CAP)
    }
}

impl TraceStream {
    /// Build a stream with a custom capacity. Useful for tests
    /// (small caps) and for future callers that need a larger
    /// window than the MVP default.
    ///
    /// **M-18 review #8, 2026-10-19:** `cap == 0` panics. The
    /// `Vec::with_capacity(0)` is a valid no-op but yields a
    /// zero-capacity buffer that can never record any event. Use
    /// `TraceStream::default()` for the MVP capacity (2×10⁵); use
    /// this constructor only when you need a non-default size.
    pub fn with_capacity(cap: usize) -> Self {
        assert!(
            cap > 0,
            "trace stream capacity must be > 0; use default() for MVP"
        );
        Self {
            buf: Vec::with_capacity(cap),
            head: 0,
            len: 0,
            seq: 0,
            dropped: 0,
            initial_cap: cap,
        }
    }

    /// Number of events currently held in the ring.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when no events are held.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Configured capacity.
    pub fn capacity(&self) -> usize {
        debug_assert_eq!(
            self.buf.capacity(),
            self.initial_cap,
            "TraceStream::buf capacity changed after construction; \
             ring math would be incorrect"
        );
        self.initial_cap
    }

    /// Number of events dropped since construction (cumulative,
    /// never reset by `drain`).
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Current write head; also the next sequence number to be
    /// assigned. M-11 uses this to checkpoint its read position.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Append a new event. If the buffer is at capacity, the oldest
    /// event is dropped (FIFO) and `dropped` is incremented. Returns
    /// the new `seq` value assigned to the event (always
    /// monotonically increasing, even on overflow).
    pub fn push(&mut self, ev: TraceEvent) -> u64 {
        // Invariant: `len` must never exceed `initial_cap`. The
        // ring math uses `initial_cap`, not `buf.capacity()`,
        // because `Vec::clone` is allowed to shrink the buffer's
        // capacity (the new Vec allocates exactly `len` slots).
        // M-18 review #13 (2026-10-19) originally proposed an
        // assertion on `buf.capacity()`; that turned out to be a
        // false positive after `Vec::clone`, so we check the
        // actually-meaningful invariant instead.
        debug_assert!(
            self.len <= self.initial_cap,
            "TraceStream::len ({}) exceeds initial_cap ({})",
            self.len,
            self.initial_cap
        );
        if self.len < self.initial_cap {
            // First-fill path: extend the buffer in order so the
            // ring semantics are correct on the first wrap.
            self.buf.push(ev);
            self.len += 1;
        } else {
            // Wrap path: overwrite the slot at `head`, advance head.
            self.buf[self.head] = ev;
            self.head = (self.head + 1) % self.initial_cap;
            self.dropped += 1;
        }
        let s = self.seq;
        self.seq += 1;
        s
    }

    /// Drain all events in time order (oldest first). Does NOT reset
    /// `dropped` or `seq` — those are lifetime counters.
    pub fn drain(&mut self) -> Vec<TraceEvent> {
        debug_assert!(
            self.len <= self.initial_cap,
            "TraceStream::len ({}) exceeds initial_cap ({})",
            self.len,
            self.initial_cap
        );
        if self.len == 0 {
            return Vec::new();
        }
        let cap = self.initial_cap;
        let mut out = Vec::with_capacity(self.len);
        if self.len < cap {
            // First-fill path: events live in `buf[0..len]` in order.
            out.extend(self.buf.iter().copied());
        } else {
            // Wrap path: events start at `head` (oldest) and wrap.
            for i in 0..cap {
                out.push(self.buf[(self.head + i) % cap]);
            }
        }
        // Reset ring state but keep seq/dropped lifetime counters.
        self.buf.clear();
        self.head = 0;
        self.len = 0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::TraceDir;

    fn ev(t: u64, addr: u32) -> TraceEvent {
        TraceEvent {
            t_ps: t,
            addr,
            width: 4,
            dir: TraceDir::Write,
        }
    }

    #[test]
    fn first_fill_preserves_insertion_order() {
        let mut s = TraceStream::with_capacity(4);
        assert_eq!(s.push(ev(0, 0x100)), 0);
        assert_eq!(s.push(ev(1, 0x104)), 1);
        assert_eq!(
            s.drain().iter().map(|e| e.addr).collect::<Vec<_>>(),
            vec![0x100, 0x104]
        );
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn overflow_drops_oldest_and_increments_dropped_counter() {
        let mut s = TraceStream::with_capacity(3);
        for i in 0..5 {
            s.push(ev(i, 0x100 + i as u32 * 4));
        }
        assert_eq!(s.len(), 3);
        assert_eq!(s.dropped(), 2);
        // seq kept incrementing even on overflow.
        assert_eq!(s.seq(), 5);
        // Oldest two (t=0, t=1) are gone; the surviving three are
        // t=2, t=3, t=4 in that order.
        let drained: Vec<u32> = s.drain().iter().map(|e| e.addr).collect();
        assert_eq!(drained, vec![0x108, 0x10c, 0x110]);
    }

    #[test]
    fn drain_resets_state_but_keeps_lifetime_counters() {
        let mut s = TraceStream::with_capacity(2);
        for i in 0..4 {
            s.push(ev(i, i as u32));
        }
        assert_eq!(s.dropped(), 2);
        let _ = s.drain();
        assert_eq!(s.len(), 0);
        assert_eq!(s.dropped(), 2); // lifetime counter preserved
        assert_eq!(s.seq(), 4); // lifetime counter preserved
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn with_capacity_zero_panics_with_clear_message() {
        // M-18 review #8 (2026-10-19): the panic message should
        // steer the caller to `default()` rather than a bare
        // "assertion failed".
        let _ = TraceStream::with_capacity(0);
    }

    #[test]
    fn trace_stream_clone_shares_events_independently() {
        // M-18 review #14: Clone support. Cloning produces an
        // independent ring (mutating one does not affect the
        // other); the events themselves are equal.
        let mut s = TraceStream::with_capacity(4);
        s.push(ev(0, 0x100));
        s.push(ev(1, 0x104));
        let mut clone = s.clone();
        // State at clone time:
        assert_eq!(clone.len(), 2);
        assert_eq!(clone.seq(), 2);
        assert_eq!(clone.dropped(), 0);
        assert_eq!(
            clone.drain().iter().map(|e| e.addr).collect::<Vec<_>>(),
            vec![0x100, 0x104]
        );
        // Original is unaffected by the clone's drain.
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn trace_event_dir_round_trips_through_typed_enum() {
        // M-18 review #11: the wire byte encoding is preserved.
        let r = TraceEvent {
            t_ps: 0,
            addr: 0,
            width: 1,
            dir: TraceDir::Read,
        };
        let w = TraceEvent {
            t_ps: 0,
            addr: 0,
            width: 1,
            dir: TraceDir::Write,
        };
        assert_eq!(r.dir.as_byte(), 0);
        assert_eq!(w.dir.as_byte(), 1);
        // Default is Read (matches the contract default).
        assert_eq!(TraceDir::default(), TraceDir::Read);
    }
}
