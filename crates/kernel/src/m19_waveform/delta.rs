//! Delta queue — the incremental push stream (M-19 v0.9 §2.1, table 71).
//!
//! The delta queue accumulates edge events between two `pushDelta`
//! calls. Each call drains the accumulated batch into a
//! [`WaveformDelta`] carrying a **monotonic, gap-free sequence
//! number range**.
//!
//! ## Sequence semantics (M-19 v0.9 §3.2, §6.2(2))
//!
//! - Every edge event consumes exactly one sequence number.
//! - `seq_start` of a batch is the sequence number of its first
//!   event; `seq_end` is one past the last.
//! - The sequence space advances **even when the push fails**. A
//!   failed batch is dropped, not re-queued: observation events are
//!   explicitly allowed to be lost (review P-09), and re-queuing
//!   would let one broken transport stall the whole pipeline.
//! - Because the *numbers* stay gap-free, a consumer cannot detect
//!   the loss from the sequence alone. The content hole is therefore
//!   recorded explicitly as a [`DropRange`], which is what the
//!   consumer hands to the resume channel (M-07 `resumeBySeq` /
//!   `E_PROTO-05`) to refill the missing window.

use riscv_sim_contract::model::{DropRange, EdgeEvent, WaveformDelta};

/// Incremental push accumulator.
#[derive(Debug, Clone, Default)]
pub struct DeltaQueue {
    /// Events accumulated since the last `take_batch`.
    pending: Vec<EdgeEvent>,
    /// Next sequence number to assign. Monotonic, never rewound.
    seq_no: u64,
    /// Ranges whose content was dropped (push failures).
    dropped: Vec<DropRange>,
    /// Number of dropped batches.
    dropped_batches: u64,
}

impl DeltaQueue {
    /// New empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulate one edge event.
    pub fn enqueue(&mut self, ev: EdgeEvent) {
        self.pending.push(ev);
    }

    /// Number of events waiting for the next batch.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// True when nothing is waiting to be pushed.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// The next sequence number that will be assigned.
    pub fn seq_no(&self) -> u64 {
        self.seq_no
    }

    /// Total number of events ever batched (lifetime counter).
    pub fn total_pushed(&self) -> u64 {
        self.seq_no
    }

    /// Take the accumulated events as one batch.
    ///
    /// Returns `None` when nothing is pending — the caller must not
    /// push an empty batch (the design doc's `pushDelta` returns
    /// `null` in that case, so the consumer is not woken up for
    /// nothing).
    ///
    /// The sequence space advances by the batch size, and the pending
    /// buffer is emptied. `t_virt` is stamped by the caller (M-19
    /// takes the authoritative virtual time from M-12 rather than
    /// reading a clock of its own).
    pub fn take_batch(&mut self, t_virt: u64) -> Option<WaveformDelta> {
        if self.pending.is_empty() {
            return None;
        }
        let entries = std::mem::take(&mut self.pending);
        let seq_start = self.seq_no;
        let seq_end = seq_start + entries.len() as u64;
        self.seq_no = seq_end;
        Some(WaveformDelta {
            seq_start,
            seq_end,
            t_virt,
            entries,
        })
    }

    /// Record that the content of `range` was dropped.
    pub fn record_drop(&mut self, range: DropRange) {
        if range.is_empty() {
            return;
        }
        self.dropped.push(range);
        self.dropped_batches += 1;
    }

    /// Ranges whose content was dropped, in the order they happened.
    pub fn dropped_ranges(&self) -> &[DropRange] {
        &self.dropped
    }

    /// Number of dropped batches.
    pub fn dropped_batches(&self) -> u64 {
        self.dropped_batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::EdgeKind;

    fn ev(t: u64) -> EdgeEvent {
        EdgeEvent {
            t_ps: t,
            ch: 1,
            value: t,
            kind: EdgeKind::Value,
        }
    }

    #[test]
    fn empty_queue_yields_no_batch() {
        let mut q = DeltaQueue::new();
        assert!(q.take_batch(0).is_none());
        assert_eq!(q.seq_no(), 0);
    }

    #[test]
    fn batch_carries_monotonic_sequence_range() {
        let mut q = DeltaQueue::new();
        q.enqueue(ev(1));
        q.enqueue(ev(2));
        let d = q.take_batch(100).expect("batch");
        assert_eq!(d.seq_start, 0);
        assert_eq!(d.seq_end, 2);
        assert_eq!(d.t_virt, 100);
        assert_eq!(d.entries.len(), 2);
        // The next batch continues right where this one stopped.
        q.enqueue(ev(3));
        let d2 = q.take_batch(200).expect("batch");
        assert_eq!(d2.seq_start, 2);
        assert_eq!(d2.seq_end, 3);
    }

    #[test]
    fn take_batch_empties_the_pending_buffer() {
        let mut q = DeltaQueue::new();
        q.enqueue(ev(1));
        let _ = q.take_batch(0);
        assert!(q.is_empty());
        assert!(q.take_batch(0).is_none());
    }

    #[test]
    fn sequence_space_stays_gap_free_across_batches() {
        let mut q = DeltaQueue::new();
        for i in 0..10 {
            q.enqueue(ev(i));
            let d = q.take_batch(i).expect("batch");
            assert_eq!(d.seq_end - d.seq_start, 1);
            assert_eq!(d.seq_start, i);
        }
        assert_eq!(q.seq_no(), 10);
    }

    #[test]
    fn drop_ranges_are_recorded_and_counted() {
        let mut q = DeltaQueue::new();
        q.record_drop(DropRange {
            seq_start: 0,
            seq_end: 3,
        });
        q.record_drop(DropRange {
            seq_start: 3,
            seq_end: 4,
        });
        assert_eq!(q.dropped_batches(), 2);
        assert_eq!(q.dropped_ranges().len(), 2);
        assert_eq!(q.dropped_ranges()[0].len(), 3);
    }

    #[test]
    fn empty_drop_range_is_ignored() {
        let mut q = DeltaQueue::new();
        q.record_drop(DropRange {
            seq_start: 5,
            seq_end: 5,
        });
        assert_eq!(q.dropped_batches(), 0);
        assert!(q.dropped_ranges().is_empty());
    }
}
