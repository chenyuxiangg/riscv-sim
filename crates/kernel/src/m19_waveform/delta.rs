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

use std::collections::VecDeque;

use riscv_sim_contract::model::{DropRange, EdgeEvent, WaveformDelta};

/// Maximum number of remembered drop ranges.
///
/// The drop list is a *diagnostic* plus a *resume hint* — a consumer
/// refills the holes through the M-07 resume channel, it does not
/// need an unbounded history. M-19 review #7 flagged the previous
/// unbounded `Vec`, whose growth also made every export chunk more
/// expensive.
pub const DROPPED_HISTORY_CAP: usize = 4096;

/// Why a drop range was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropError {
    /// The range is inverted (`seq_start > seq_end`).
    Inverted {
        /// The rejected start.
        seq_start: u64,
        /// The rejected end.
        seq_end: u64,
    },
    /// The range extends past the allocated sequence space.
    OutOfRange {
        /// The rejected end.
        seq_end: u64,
        /// The highest sequence number handed out so far.
        seq_no: u64,
    },
}

impl std::fmt::Display for DropError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inverted { seq_start, seq_end } => {
                write!(f, "inverted drop range {seq_start}..{seq_end}")
            }
            Self::OutOfRange { seq_end, seq_no } => {
                write!(f, "drop range ends at {seq_end}, past seq_no {seq_no}")
            }
        }
    }
}

impl std::error::Error for DropError {}

/// Incremental push accumulator.
#[derive(Debug, Clone, Default)]
pub struct DeltaQueue {
    /// Events accumulated since the last `take_batch`.
    pending: Vec<EdgeEvent>,
    /// Next sequence number to assign. Monotonic, never rewound.
    seq_no: u64,
    /// Ranges whose content was dropped (push failures). Bounded by
    /// [`DROPPED_HISTORY_CAP`]; adjacent ranges are merged so a long
    /// outage collapses into a single entry.
    dropped: VecDeque<DropRange>,
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
    ///
    /// The range is validated against the allocated sequence space
    /// (M-19 review #8): a range that is inverted, or that reaches
    /// past `seq_no`, is rejected instead of being recorded — a
    /// consumer acting on such a range would ask the resume channel
    /// for data that does not exist.
    ///
    /// A range that is adjacent to or overlaps the most recent
    /// recorded range is **merged** into it, so a sustained outage
    /// collapses into one entry instead of one entry per batch.
    pub fn record_drop(&mut self, range: DropRange) -> Result<(), DropError> {
        if range.seq_start > range.seq_end {
            return Err(DropError::Inverted {
                seq_start: range.seq_start,
                seq_end: range.seq_end,
            });
        }
        if range.seq_end > self.seq_no {
            return Err(DropError::OutOfRange {
                seq_end: range.seq_end,
                seq_no: self.seq_no,
            });
        }
        if range.is_empty() {
            return Ok(());
        }
        self.dropped_batches += 1;
        if let Some(last) = self.dropped.back_mut() {
            // Adjacent (last.seq_end == range.seq_start) or
            // overlapping (range.seq_start < last.seq_end) ranges
            // describe one contiguous hole.
            if range.seq_start <= last.seq_end {
                last.seq_end = last.seq_end.max(range.seq_end);
                return Ok(());
            }
        }
        self.dropped.push_back(range);
        while self.dropped.len() > DROPPED_HISTORY_CAP {
            self.dropped.pop_front();
        }
        Ok(())
    }

    /// Ranges whose content was dropped, oldest first.
    pub fn dropped_ranges(&self) -> impl ExactSizeIterator<Item = &DropRange> {
        self.dropped.iter()
    }

    /// Snapshot of the drop ranges (oldest first).
    pub fn dropped_ranges_vec(&self) -> Vec<DropRange> {
        self.dropped.iter().copied().collect()
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
        for i in 0..4 {
            q.enqueue(ev(i));
        }
        let _ = q.take_batch(0);
        q.record_drop(DropRange {
            seq_start: 0,
            seq_end: 2,
        })
        .expect("valid range");
        q.record_drop(DropRange {
            seq_start: 2,
            seq_end: 4,
        })
        .expect("valid range");
        assert_eq!(q.dropped_batches(), 2);
        // Adjacent ranges are merged into one contiguous hole.
        assert_eq!(q.dropped_ranges().len(), 1);
        assert_eq!(q.dropped_ranges().next().expect("range").seq_end, 4);
    }

    #[test]
    fn empty_drop_range_is_ignored() {
        let mut q = DeltaQueue::new();
        q.record_drop(DropRange {
            seq_start: 0,
            seq_end: 0,
        })
        .expect("empty range is a no-op");
        assert_eq!(q.dropped_batches(), 0);
        assert_eq!(q.dropped_ranges().len(), 0);
    }

    #[test]
    fn inverted_drop_range_is_rejected() {
        let mut q = DeltaQueue::new();
        let err = q
            .record_drop(DropRange {
                seq_start: 5,
                seq_end: 3,
            })
            .expect_err("inverted range");
        assert_eq!(
            err,
            DropError::Inverted {
                seq_start: 5,
                seq_end: 3
            }
        );
        assert_eq!(q.dropped_batches(), 0);
    }

    #[test]
    fn drop_range_past_the_sequence_space_is_rejected() {
        let mut q = DeltaQueue::new();
        q.enqueue(ev(1));
        let _ = q.take_batch(0);
        let err = q
            .record_drop(DropRange {
                seq_start: 0,
                seq_end: 99,
            })
            .expect_err("out of range");
        assert_eq!(
            err,
            DropError::OutOfRange {
                seq_end: 99,
                seq_no: 1
            }
        );
        assert_eq!(q.dropped_batches(), 0);
    }

    #[test]
    fn overlapping_drop_ranges_merge() {
        let mut q = DeltaQueue::new();
        for i in 0..10 {
            q.enqueue(ev(i));
        }
        let _ = q.take_batch(0);
        q.record_drop(DropRange {
            seq_start: 0,
            seq_end: 6,
        })
        .expect("valid");
        q.record_drop(DropRange {
            seq_start: 3,
            seq_end: 8,
        })
        .expect("valid");
        assert_eq!(q.dropped_ranges().len(), 1);
        assert_eq!(q.dropped_ranges().next().expect("range").seq_end, 8);
    }

    #[test]
    fn drop_history_is_bounded() {
        let mut q = DeltaQueue::new();
        // Allocate enough sequence space for many disjoint ranges.
        for _ in 0..(DROPPED_HISTORY_CAP * 2 + 10) {
            q.enqueue(ev(0));
            let _ = q.take_batch(0);
        }
        let mut next = 0u64;
        for _ in 0..(DROPPED_HISTORY_CAP + 10) {
            let _ = q.record_drop(DropRange {
                seq_start: next,
                seq_end: next + 1,
            });
            next += 2; // leave a gap so ranges do not merge
        }
        assert_eq!(q.dropped_ranges().len(), DROPPED_HISTORY_CAP);
    }
}
