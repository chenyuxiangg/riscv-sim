//! Waveform export — chunked CSV / VCD (M-19 v0.9 §3.4).
//!
//! The exporter reads the per-channel ring buffers ([`super::ring`]),
//! merges them into a single `t_virt`-ordered stream, and hands the
//! result out in chunks of [`EXPORT_CHUNK`] entries.
//!
//! ## Chunking and resume
//!
//! `next_offset` is an index into the **merged** stream, so it is a
//! stable resume anchor: re-running the export with the same filter
//! and the same ring contents continues exactly where the previous
//! chunk stopped. The caller drives the export by feeding the
//! returned `next_offset` back in until `done` is true.
//!
//! ## Merge order
//!
//! The merged stream is sorted by `(t_ps, ch)`. Ties on the timestamp
//! are broken by channel number so the output is **deterministic** —
//! two runs over the same data produce byte-identical exports, which
//! is what makes the exported file diffable and the resume anchor
//! meaningful.
//!
//! ## Merge implementation
//!
//! The merge is a k-way merge over the per-channel ring iterators
//! (a `BinaryHeap` of per-stream heads). It never materialises the
//! whole merged stream, so an export of the full 4 × 2×10⁵ window
//! does not need a second 20 MB allocation on top of the rings.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use riscv_sim_contract::model::{DropRange, EdgeKind};

use super::channel::{edge_kind_text, ChannelMeta};
use super::ring::{RingEntry, RingIter};

/// Entries per export chunk (M-19 v0.9 §3.4: 10⁴ entries per chunk).
pub const EXPORT_CHUNK: usize = 10_000;

/// Which channels and which time window to export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportFilter {
    /// Channel subset. `None` exports every channel that holds
    /// entries.
    pub channels: Option<Vec<u8>>,
    /// Inclusive lower bound on `t_virt` (picoseconds).
    pub t_start: Option<u64>,
    /// Inclusive upper bound on `t_virt` (picoseconds).
    pub t_end: Option<u64>,
}

impl ExportFilter {
    /// A filter that selects everything.
    pub fn all() -> Self {
        Self::default()
    }

    /// Restrict the export to a single channel.
    pub fn with_channel(mut self, ch: u8) -> Self {
        self.channels.get_or_insert_with(Vec::new).push(ch);
        self
    }

    /// Restrict the export to a time window (inclusive on both ends).
    pub fn with_time_range(mut self, start: u64, end: u64) -> Self {
        self.t_start = Some(start);
        self.t_end = Some(end);
        self
    }

    /// True when `ch` passes the channel filter.
    pub fn includes_channel(&self, ch: u8) -> bool {
        match &self.channels {
            None => true,
            Some(list) => list.contains(&ch),
        }
    }

    /// True when `t` passes the time filter.
    pub fn includes_time(&self, t: u64) -> bool {
        if let Some(start) = self.t_start {
            if t < start {
                return false;
            }
        }
        if let Some(end) = self.t_end {
            if t > end {
                return false;
            }
        }
        true
    }
}

/// Export output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// Comma-separated values with a text header.
    Csv,
    /// IEEE 1364-style value change dump.
    Vcd,
}

/// Metadata carried alongside an export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportMeta {
    /// Total number of entries in the merged (filtered) stream.
    pub total: usize,
    /// True when at least one channel dropped entries to overflow.
    pub overflow_marked: bool,
    /// Ranges whose content was dropped by a failed push.
    pub dropped_ranges: Vec<DropRange>,
}

/// One chunk of an export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportHandle {
    /// The rendered chunk (text).
    pub chunk: String,
    /// Index into the merged stream to resume from. Pass it back as
    /// `next_offset` to continue the export.
    pub next_offset: usize,
    /// Total number of entries in the merged stream.
    pub total: usize,
    /// True when this chunk reached the end of the stream.
    pub done: bool,
    /// Completion percentage (`0..=100`).
    pub progress_pct: u8,
    /// True when at least one channel dropped entries to overflow —
    /// the exported window is not the complete history.
    pub overflow_marked: bool,
    /// Ranges whose content was dropped by a failed push.
    pub dropped_ranges: Vec<DropRange>,
}

/// A per-channel stream inside the merge.
struct ChannelStream<'a> {
    iter: RingIter<'a>,
    /// The next entry of this stream, already queued in the heap.
    head: Option<RingEntry>,
}

/// K-way merge over the per-channel ring buffers.
///
/// Yields the merged stream in `(t_ps, ch)` order without
/// materialising it.
///
/// **Precondition:** each channel's entries must be in non-decreasing
/// time order. The ring stores entries in arrival order, which equals
/// time order because the virtual clock is monotonic; the pipeline's
/// export path reorders defensively before building the cursor (see
/// [`super::ring::ChannelRing::sort_by_time`]).
pub struct MergedCursor<'a> {
    streams: Vec<ChannelStream<'a>>,
    /// `(t_ps, ch, stream_index)`, reversed so the `BinaryHeap`
    /// (a max-heap) behaves as a min-heap.
    heap: BinaryHeap<Reverse<(u64, u8, usize)>>,
    filter: ExportFilter,
    /// Set once the cursor walked past the filter's end bound.
    stopped: bool,
}

impl<'a> MergedCursor<'a> {
    /// Build a cursor over the given rings, filtered.
    pub fn new(rings: &'a BTreeMap<u8, super::ring::ChannelRing>, filter: ExportFilter) -> Self {
        let mut streams: Vec<ChannelStream<'a>> = Vec::new();
        let mut heap: BinaryHeap<Reverse<(u64, u8, usize)>> = BinaryHeap::new();
        for (ch, ring) in rings {
            if !filter.includes_channel(*ch) {
                continue;
            }
            let mut iter = ring.iter();
            if let Some(head) = iter.next() {
                let idx = streams.len();
                heap.push(Reverse((head.t_ps, head.ch, idx)));
                streams.push(ChannelStream {
                    iter,
                    head: Some(head),
                });
            }
        }
        Self {
            streams,
            heap,
            filter,
            stopped: false,
        }
    }

    /// Next entry of the merged stream that passes the filter.
    pub fn next_entry(&mut self) -> Option<RingEntry> {
        if self.stopped {
            return None;
        }
        loop {
            let Reverse((t, _ch, si)) = self.heap.pop()?;
            let (entry, next) = {
                let stream = &mut self.streams[si];
                let entry = stream.head.take()?;
                let next = stream.iter.next();
                stream.head = next;
                (entry, next)
            };
            if let Some(next) = next {
                self.heap.push(Reverse((next.t_ps, next.ch, si)));
            }
            if let Some(end) = self.filter.t_end {
                if t > end {
                    // Past the window: the merged stream is ordered by
                    // time, so nothing further can qualify.
                    self.stopped = true;
                    return None;
                }
            }
            if let Some(start) = self.filter.t_start {
                if t < start {
                    continue;
                }
            }
            return Some(entry);
        }
    }

    /// Skip up to `n` entries of the merged stream.
    ///
    /// Returns the number of entries actually skipped — fewer than
    /// `n` when the stream ended early.
    pub fn skip(&mut self, n: usize) -> usize {
        let mut skipped = 0;
        while skipped < n {
            if self.next_entry().is_none() {
                break;
            }
            skipped += 1;
        }
        skipped
    }
}

/// Count the entries of the merged stream that pass the filter.
///
/// Used for the export progress. This is an O(n) scan, but it does
/// not allocate — and it runs once per chunk, not once per entry.
pub fn count_filtered(
    rings: &BTreeMap<u8, super::ring::ChannelRing>,
    filter: &ExportFilter,
) -> usize {
    let mut n = 0;
    for (ch, ring) in rings {
        if !filter.includes_channel(*ch) {
            continue;
        }
        for entry in ring.iter() {
            if filter.includes_time(entry.t_ps) {
                n += 1;
            }
        }
    }
    n
}

/// Render a CSV chunk.
///
/// The header (comment metadata + column names) is emitted only in
/// the first chunk (`include_header`), so concatenating the chunks
/// yields a single well-formed CSV file.
pub fn render_csv(
    entries: &[RingEntry],
    metas: &BTreeMap<u8, ChannelMeta>,
    include_header: bool,
    meta: &ExportMeta,
) -> String {
    let mut out = String::new();
    if include_header {
        out.push_str("# riscv-sim waveform export (M-19)\n");
        out.push_str(&format!(
            "# entries={} overflow_marked={} dropped_ranges={}\n",
            meta.total,
            meta.overflow_marked,
            meta.dropped_ranges.len()
        ));
        for r in &meta.dropped_ranges {
            out.push_str(&format!("# dropped_range={}..{}\n", r.seq_start, r.seq_end));
        }
        out.push_str("t_ps,ch,label,value,edge\n");
    }
    for e in entries {
        let label = metas.get(&e.ch).map(|m| m.label.as_str()).unwrap_or("ch?");
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            e.t_ps,
            e.ch,
            label,
            e.value,
            edge_kind_text(e.kind)
        ));
    }
    out
}

/// Render a VCD chunk.
///
/// The `$var` declarations are emitted only in the first chunk
/// (`include_header`); the value changes follow in every chunk. The
/// overflow marker is *not* written into the VCD body — the VCD
/// grammar has no portable comment command — it travels in the
/// [`ExportHandle`] metadata and in the CSV export instead.
pub fn render_vcd(
    entries: &[RingEntry],
    metas: &BTreeMap<u8, ChannelMeta>,
    channels: &[u8],
    include_header: bool,
) -> String {
    let mut out = String::new();
    if include_header {
        out.push_str("$timescale 1ps $end\n");
        out.push_str("$scope module riscv_sim $end\n");
        for ch in channels {
            let meta = metas.get(ch);
            let width = meta.map(|m| m.value_width.max(1)).unwrap_or(1);
            let label = meta.map(|m| m.label.as_str()).unwrap_or("ch?");
            out.push_str(&format!("$var wire {width} ch{ch} {label} $end\n"));
        }
        out.push_str("$enddefinitions $end\n");
    }
    let mut last_t: Option<u64> = None;
    for e in entries {
        // A repeated `#<t>` for the same timestamp is legal VCD, but
        // within a chunk we suppress the duplicate to keep the file
        // compact. Across a chunk boundary the timestamp is always
        // re-emitted, which is harmless.
        if last_t != Some(e.t_ps) {
            out.push_str(&format!("#{}\n", e.t_ps));
            last_t = Some(e.t_ps);
        }
        let width = metas.get(&e.ch).map(|m| m.value_width).unwrap_or(1);
        if width <= 1 {
            out.push_str(&format!("{}{}\n", e.value & 1, vcd_id(e.ch)));
        } else {
            out.push_str(&format!("b{:b} {}\n", e.value, vcd_id(e.ch)));
        }
    }
    out
}

/// VCD identifier for a channel number.
///
/// Multi-character identifiers are legal in VCD (the standard's own
/// examples use `bus0[0]`), and they avoid the collision problem the
/// printable-ASCII single-character scheme runs into past 94
/// channels.
fn vcd_id(ch: u8) -> String {
    format!("ch{ch}")
}

/// Textual edge kind, re-exported for callers that render their own
/// tables.
pub fn kind_text(kind: EdgeKind) -> &'static str {
    edge_kind_text(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m19_waveform::ring::ChannelRing;

    fn ring_of(entries: &[(u64, u64)]) -> ChannelRing {
        let mut r = ChannelRing::with_capacity(64);
        for (t, v) in entries {
            r.push(RingEntry::new(*t, 1, *v, EdgeKind::Value));
        }
        r
    }

    #[test]
    fn filter_selects_channels() {
        let f = ExportFilter::all().with_channel(2);
        assert!(f.includes_channel(2));
        assert!(!f.includes_channel(1));
        assert!(ExportFilter::all().includes_channel(7));
    }

    #[test]
    fn filter_selects_time_window() {
        let f = ExportFilter::all().with_time_range(10, 20);
        assert!(!f.includes_time(9));
        assert!(f.includes_time(10));
        assert!(f.includes_time(20));
        assert!(!f.includes_time(21));
    }

    #[test]
    fn merged_cursor_orders_by_time_then_channel() {
        let mut rings = BTreeMap::new();
        let mut a = ChannelRing::with_capacity(16);
        a.push(RingEntry::new(10, 1, 0, EdgeKind::Value));
        a.push(RingEntry::new(30, 1, 0, EdgeKind::Value));
        let mut b = ChannelRing::with_capacity(16);
        b.push(RingEntry::new(10, 2, 0, EdgeKind::Value));
        b.push(RingEntry::new(20, 2, 0, EdgeKind::Value));
        rings.insert(1u8, a);
        rings.insert(2u8, b);
        let mut cur = MergedCursor::new(&rings, ExportFilter::all());
        let mut got = Vec::new();
        while let Some(e) = cur.next_entry() {
            got.push((e.t_ps, e.ch));
        }
        assert_eq!(got, vec![(10, 1), (10, 2), (20, 2), (30, 1)]);
    }

    #[test]
    fn merged_cursor_applies_time_filter() {
        let mut rings = BTreeMap::new();
        rings.insert(1u8, ring_of(&[(10, 1), (20, 2), (30, 3)]));
        let mut cur = MergedCursor::new(&rings, ExportFilter::all().with_time_range(15, 25));
        let mut got = Vec::new();
        while let Some(e) = cur.next_entry() {
            got.push(e.t_ps);
        }
        assert_eq!(got, vec![20]);
    }

    #[test]
    fn merged_cursor_skip_advances() {
        let mut rings = BTreeMap::new();
        rings.insert(1u8, ring_of(&[(1, 1), (2, 2), (3, 3)]));
        let mut cur = MergedCursor::new(&rings, ExportFilter::all());
        assert_eq!(cur.skip(2), 2);
        let e = cur.next_entry().expect("entry");
        assert_eq!(e.t_ps, 3);
    }

    #[test]
    fn count_filtered_matches_merged_stream_length() {
        let mut rings = BTreeMap::new();
        rings.insert(1u8, ring_of(&[(10, 1), (20, 2), (30, 3)]));
        let f = ExportFilter::all().with_time_range(15, 25);
        assert_eq!(count_filtered(&rings, &f), 1);
        assert_eq!(count_filtered(&rings, &ExportFilter::all()), 3);
    }

    #[test]
    fn csv_chunk_has_header_only_once() {
        let mut metas = BTreeMap::new();
        metas.insert(
            1u8,
            ChannelMeta {
                label: "GPIO0.OUT".into(),
                value_width: 1,
                single_bit: true,
            },
        );
        let entries = vec![RingEntry::new(100, 1, 1, EdgeKind::Rising)];
        let meta = ExportMeta {
            total: 1,
            overflow_marked: false,
            dropped_ranges: vec![],
        };
        let first = render_csv(&entries, &metas, true, &meta);
        assert!(first.contains("t_ps,ch,label,value,edge"));
        assert!(first.contains("100,1,GPIO0.OUT,1,Rising"));
        let second = render_csv(&entries, &metas, false, &meta);
        assert!(!second.contains("t_ps,ch,label,value,edge"));
        assert!(second.contains("100,1,GPIO0.OUT,1,Rising"));
    }

    #[test]
    fn csv_records_overflow_marker() {
        let metas = BTreeMap::new();
        let meta = ExportMeta {
            total: 0,
            overflow_marked: true,
            dropped_ranges: vec![DropRange {
                seq_start: 1,
                seq_end: 4,
            }],
        };
        let out = render_csv(&[], &metas, true, &meta);
        assert!(out.contains("overflow_marked=true"));
        assert!(out.contains("dropped_range=1..4"));
    }

    #[test]
    fn vcd_header_declares_vars_once() {
        let mut metas = BTreeMap::new();
        metas.insert(1u8, ChannelMeta::single_bit("GPIO0.OUT"));
        let entries = vec![RingEntry::new(100, 1, 1, EdgeKind::Rising)];
        let first = render_vcd(&entries, &metas, &[1], true);
        assert!(first.contains("$timescale 1ps $end"));
        assert!(first.contains("$var wire 1 ch1 GPIO0.OUT $end"));
        assert!(first.contains("#100"));
        assert!(first.contains("1ch1"));
        let second = render_vcd(&entries, &metas, &[1], false);
        assert!(!second.contains("$var"));
        assert!(second.contains("1ch1"));
    }

    #[test]
    fn vcd_uses_vector_form_for_multi_bit_channels() {
        let mut metas = BTreeMap::new();
        metas.insert(3u8, ChannelMeta::multi_bit("WORD", 8));
        let entries = vec![RingEntry::new(5, 3, 0b1010, EdgeKind::Value)];
        let out = render_vcd(&entries, &metas, &[3], true);
        assert!(out.contains("$var wire 8 ch3 WORD $end"));
        assert!(out.contains("b1010 ch3"));
    }
}
