//! Per-channel waveform ring buffer (M-19 v0.9 §2.1, table 71).
//!
//! Each channel owns an independent fixed-capacity FIFO of
//! [`RingEntry`]s. On overflow the **oldest** entry is evicted and the
//! channel's `overflow_count` is incremented — the frontend renders
//! that counter as a "history truncated" marker (M-19 v0.9 §6.1).
//!
//! The ring is the *history window*: it is what the exporter reads
//! for CSV / VCD output and what the frontend replays when it wants
//! to redraw from scratch. The live push stream is the delta queue
//! ([`super::delta`]) — the two are deliberately separate, exactly as
//! M-18 keeps its bus-trace stream separate from its edge stream.
//!
//! The implementation mirrors M-18's `TraceStream` (same write-cursor
//! ring math, same "store the capacity explicitly rather than trusting
//! `Vec::capacity()`" rule from M-18 review #13) so the two modules
//! behave identically under overflow.

use riscv_sim_contract::model::{EdgeEvent, EdgeKind};

/// Default per-channel ring capacity (M-19 v0.9 §1.1: 2×10⁵ events).
///
/// The design doc calls this an "estimated baseline; calibrate after
/// the MVP lands". It is `pub` so a caller can pick a different window
/// without forking the module.
pub const RING_CAP: usize = 200_000;

/// Budget accounting size of one ring entry, in bytes.
///
/// The design doc sizes a `RingEntry` as "t 8B + ch 1B + value 8B +
/// edge 1B + 对齐" = 32 B. The actual Rust struct is smaller (24 B
/// once padded), so 32 B is a conservative **upper bound** used for
/// the memory-budget accounting — under-counting would make the 25 MB
/// budget a lie. `ring_entry_fits_budget` asserts the invariant.
pub const ENTRY_BYTES: u64 = 32;

/// One edge-encoded waveform entry (M-19 v0.9 §2.1.1).
///
/// The design's `RingEntry` is `{t, ch, value, edge}` — the channel
/// number is redundant for a per-channel ring, but it is kept so an
/// entry stays self-describing when it is merged across channels for
/// the export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingEntry {
    /// Virtual time of the edge (picoseconds).
    pub t_ps: u64,
    /// New value of the signal.
    pub value: u64,
    /// Channel number.
    pub ch: u8,
    /// Edge classification.
    pub kind: EdgeKind,
}

impl From<EdgeEvent> for RingEntry {
    fn from(ev: EdgeEvent) -> Self {
        Self {
            t_ps: ev.t_ps,
            value: ev.value,
            ch: ev.ch,
            kind: ev.kind,
        }
    }
}

impl RingEntry {
    /// Build an entry directly.
    pub fn new(t_ps: u64, ch: u8, value: u64, kind: EdgeKind) -> Self {
        Self {
            t_ps,
            value,
            ch,
            kind,
        }
    }
}

/// Fixed-capacity FIFO of [`RingEntry`]s with a drop-oldest overflow
/// counter.
///
/// `Clone` is derived: the buffer is cloneable and the bookkeeping
/// integers are `Copy`, so snapshotting a channel's window is cheap
/// (same rationale as M-18 review #14).
#[derive(Debug, Clone)]
pub struct ChannelRing {
    buf: Vec<RingEntry>,
    /// Next write index; also the index of the oldest entry once the
    /// ring has wrapped.
    head: usize,
    /// Number of valid entries (≤ `initial_cap`).
    len: usize,
    /// Cumulative count of evicted entries. Never reset by
    /// [`ChannelRing::clear`] — it is a lifetime counter.
    overflow_count: u64,
    /// Capacity locked at construction. Stored explicitly rather than
    /// read from `buf.capacity()` (M-18 review #13: `Vec::clone` is
    /// allowed to shrink the capacity, so trusting `capacity()` would
    /// break the ring math).
    initial_cap: usize,
}

impl Default for ChannelRing {
    fn default() -> Self {
        Self::with_capacity(RING_CAP)
    }
}

impl ChannelRing {
    /// Build a ring with a custom capacity.
    ///
    /// **Panics** when `cap == 0` — a zero-capacity ring could never
    /// record anything, which is always a caller mistake. Use
    /// [`ChannelRing::default`] for the MVP capacity.
    pub fn with_capacity(cap: usize) -> Self {
        assert!(
            cap > 0,
            "channel ring capacity must be > 0; use default() for MVP"
        );
        Self {
            buf: Vec::with_capacity(cap),
            head: 0,
            len: 0,
            overflow_count: 0,
            initial_cap: cap,
        }
    }

    /// Number of entries currently held.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the ring holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Configured capacity.
    ///
    /// **M-19 review #1 (2026-10-19):** this accessor used to carry a
    /// `debug_assert_eq!(self.buf.capacity(), self.initial_cap)`. That
    /// assertion's premise is false — `Vec::clone` allocates exactly
    /// `len` slots, so a cloned ring's `buf.capacity()` is `len`, not
    /// `initial_cap` — and it panicked in debug builds on any
    /// `clone()` followed by `capacity()`. The error message
    /// ("ring math would be incorrect") was also factually wrong: the
    /// ring math uses `initial_cap`, never `buf.capacity()`, so it
    /// stays correct across a clone.
    ///
    /// `initial_cap` is the single source of truth for the ring math
    /// (M-18 review #13's rule); this accessor simply reports it.
    pub fn capacity(&self) -> usize {
        debug_assert!(
            self.len <= self.initial_cap,
            "ChannelRing::len ({}) exceeds initial_cap ({})",
            self.len,
            self.initial_cap
        );
        self.initial_cap
    }

    /// Cumulative number of entries evicted by overflow.
    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
    }

    /// Append an entry.
    ///
    /// Returns `true` when the ring was already full and the oldest
    /// entry was evicted to make room (the caller uses this to apply
    /// the overflow policy — see [`super::channel::OverflowPolicy`]).
    pub fn push(&mut self, entry: RingEntry) -> bool {
        debug_assert!(
            self.len <= self.initial_cap,
            "ChannelRing::len ({}) exceeds initial_cap ({})",
            self.len,
            self.initial_cap
        );
        if self.len < self.initial_cap {
            // First-fill path: append in order so the wrap semantics
            // are correct on the first wrap.
            self.buf.push(entry);
            self.len += 1;
            false
        } else {
            // Wrap path: overwrite the slot at `head` (the oldest
            // entry) and advance the cursor.
            self.buf[self.head] = entry;
            self.head = (self.head + 1) % self.initial_cap;
            self.overflow_count += 1;
            true
        }
    }

    /// Iterate the entries in chronological order (oldest first).
    pub fn iter(&self) -> RingIter<'_> {
        RingIter {
            buf: &self.buf,
            head: self.head,
            cap: self.initial_cap,
            len: self.len,
            pos: 0,
        }
    }

    /// True when the entries are in non-decreasing time order.
    ///
    /// The ring stores entries in **arrival** order. Because the
    /// simulation's virtual clock is monotonic and M-18 samples in
    /// time order, arrival order *is* time order in practice — this
    /// method is the cheap check the export path uses to decide
    /// whether a defensive reorder is needed.
    pub fn is_time_ordered(&self) -> bool {
        let mut prev: Option<u64> = None;
        for e in self.iter() {
            if let Some(p) = prev {
                if e.t_ps < p {
                    return false;
                }
            }
            prev = Some(e.t_ps);
        }
        true
    }

    /// Reorder the entries in place so they are in non-decreasing
    /// time order.
    ///
    /// This is a **defensive** operation: the export path calls it
    /// only when [`ChannelRing::is_time_ordered`] reports a
    /// violation, which should not happen with a monotonic virtual
    /// clock. It preserves the entry set (nothing is dropped) and
    /// only changes the internal layout.
    pub fn sort_by_time(&mut self) {
        if self.is_time_ordered() {
            return;
        }
        let mut entries: Vec<RingEntry> = self.iter().collect();
        // The channel number breaks ties so the order is total and
        // therefore deterministic.
        entries.sort_by_key(|e| (e.t_ps, e.ch));
        debug_assert_eq!(entries.len(), self.len);
        debug_assert_eq!(entries.len(), self.buf.len());
        self.buf.copy_from_slice(&entries);
        self.head = 0;
    }

    /// Drop every entry. The `overflow_count` lifetime counter is
    /// preserved.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.head = 0;
        self.len = 0;
    }
}

/// Chronological iterator over a [`ChannelRing`].
///
/// Allocation-free: it walks the ring from the write cursor without
/// materialising the entries.
#[derive(Debug, Clone)]
pub struct RingIter<'a> {
    buf: &'a [RingEntry],
    head: usize,
    cap: usize,
    len: usize,
    pos: usize,
}

impl Iterator for RingIter<'_> {
    type Item = RingEntry;

    fn next(&mut self) -> Option<RingEntry> {
        if self.pos >= self.len {
            return None;
        }
        let idx = (self.head + self.pos) % self.cap;
        self.pos += 1;
        Some(self.buf[idx])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.pos;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for RingIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(t: u64, v: u64) -> RingEntry {
        RingEntry::new(t, 1, v, EdgeKind::Value)
    }

    #[test]
    fn first_fill_preserves_insertion_order() {
        let mut r = ChannelRing::with_capacity(4);
        assert!(!r.push(e(0, 0)));
        assert!(!r.push(e(1, 1)));
        assert_eq!(r.iter().map(|x| x.t_ps).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn overflow_evicts_oldest_and_counts() {
        let mut r = ChannelRing::with_capacity(3);
        for i in 0..5 {
            r.push(e(i, i));
        }
        assert_eq!(r.len(), 3);
        assert_eq!(r.overflow_count(), 2);
        assert_eq!(r.iter().map(|x| x.t_ps).collect::<Vec<_>>(), vec![2, 3, 4]);
    }

    #[test]
    fn overflow_keeps_newest_entry() {
        let mut r = ChannelRing::with_capacity(2);
        r.push(e(10, 1));
        r.push(e(20, 2));
        assert!(r.push(e(30, 3)));
        let last = r.iter().last().expect("entry");
        assert_eq!(last.t_ps, 30);
    }

    #[test]
    fn clear_preserves_overflow_counter() {
        let mut r = ChannelRing::with_capacity(1);
        r.push(e(0, 0));
        r.push(e(1, 1));
        assert_eq!(r.overflow_count(), 1);
        r.clear();
        assert!(r.is_empty());
        assert_eq!(r.overflow_count(), 1);
    }

    #[test]
    fn clone_is_independent() {
        let mut r = ChannelRing::with_capacity(4);
        r.push(e(0, 0));
        r.push(e(1, 1));
        let mut c = r.clone();
        c.push(e(2, 2));
        assert_eq!(r.len(), 2);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn clone_then_capacity_does_not_panic() {
        // M-19 review #1 (2026-10-19): `Vec::clone` allocates exactly
        // `len` slots, so the clone's `buf.capacity()` is `len` rather
        // than `initial_cap`. The old assertion in `capacity()` had a
        // false premise and panicked here in debug builds.
        let mut r = ChannelRing::with_capacity(8);
        r.push(e(0, 0));
        let c = r.clone();
        assert_eq!(c.capacity(), 8);
        assert_eq!(c.len(), 1);
        // The clone is still a fully functional ring.
        let mut c = c;
        c.push(e(1, 1));
        assert_eq!(c.iter().map(|x| x.t_ps).collect::<Vec<_>>(), vec![0, 1]);
    }

    #[test]
    fn cloned_ring_keeps_working_overflow_semantics() {
        // A clone must overflow at the same threshold as the original.
        let mut r = ChannelRing::with_capacity(2);
        r.push(e(0, 0));
        r.push(e(1, 1));
        let mut c = r.clone();
        assert!(c.push(e(2, 2)));
        assert_eq!(c.overflow_count(), 1);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn iterator_reports_exact_size() {
        let mut r = ChannelRing::with_capacity(4);
        for i in 0..3 {
            r.push(e(i, i));
        }
        let it = r.iter();
        assert_eq!(it.len(), 3);
        assert_eq!(it.count(), 3);
    }

    #[test]
    fn time_order_check_detects_disorder() {
        let mut r = ChannelRing::with_capacity(8);
        r.push(e(10, 0));
        r.push(e(20, 1));
        assert!(r.is_time_ordered());
        r.push(e(15, 2));
        assert!(!r.is_time_ordered());
    }

    #[test]
    fn sort_by_time_reorders_without_losing_entries() {
        let mut r = ChannelRing::with_capacity(8);
        r.push(e(30, 0));
        r.push(e(10, 0));
        r.push(e(20, 0));
        assert!(!r.is_time_ordered());
        r.sort_by_time();
        assert!(r.is_time_ordered());
        assert_eq!(r.len(), 3);
        assert_eq!(
            r.iter().map(|x| x.t_ps).collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn sort_by_time_is_a_noop_on_an_ordered_ring() {
        let mut r = ChannelRing::with_capacity(8);
        r.push(e(10, 0));
        r.push(e(20, 0));
        r.sort_by_time();
        assert_eq!(r.iter().map(|x| x.t_ps).collect::<Vec<_>>(), vec![10, 20]);
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn zero_capacity_panics_with_clear_message() {
        let _ = ChannelRing::with_capacity(0);
    }
}
