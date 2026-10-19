//! Waveform export — chunked CSV / VCD (M-19 v0.9 §3.4).
//!
//! The exporter reads the per-channel ring buffers ([`super::ring`]),
//! merges them into a single `t_virt`-ordered stream, and hands the
//! result out in chunks of [`EXPORT_CHUNK`] entries.
//!
//! ## Chunking and resume
//!
//! Two entry points exist, and they differ in *cost*, not in output:
//!
//! - [`ExportSession`] — the recommended path for sequential chunking.
//!   It keeps the merge cursor alive between chunks, so the whole
//!   export costs **O(n) total** instead of O(n²/chunk). This is
//!   M-19 review #4's fix: the previous implementation rebuilt the
//!   cursor and re-skipped from the start on every chunk.
//! - [`super::pipeline::WaveformPipeline::export_csv`] /
//!   [`super::pipeline::WaveformPipeline::export_vcd`] — one-shot /
//!   *resume* entry points. They take a `next_offset` and skip up to
//!   it, which costs O(next_offset) **per call**. Use them to resume
//!   an export at a known anchor, or for small one-off exports; use
//!   [`ExportSession`] when walking a large window chunk by chunk.
//!
//! `next_offset` is an index into the **merged** stream, so it is a
//! stable resume anchor: re-running the export with the same filter
//! and the same ring contents continues exactly where the previous
//! chunk stopped.
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

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};
use std::fmt::Write as _;

use riscv_sim_contract::model::DropRange;

use super::channel::{edge_kind_text, ChannelMeta};
use super::ring::{RingEntry, RingIter};

/// Entries per export chunk (M-19 v0.9 §3.4: 10⁴ entries per chunk).
pub const EXPORT_CHUNK: usize = 10_000;

/// A set of channel numbers, stored as a fixed 256-bit mask.
///
/// Channel numbers are `u8`, so the whole universe fits in four
/// `u64` words. Membership tests are therefore O(1) — M-19 review #5
/// flagged the previous `Vec<u8>::contains` linear scan, which
/// multiplied the per-edge cost by the number of selected channels
/// across a whole-window export.
///
/// A bitmask also makes duplicate insertion impossible, which the
/// previous `Vec` allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelSet {
    bits: [u64; 4],
}

impl ChannelSet {
    /// Empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a channel. Adding the same channel twice is a no-op.
    pub fn insert(&mut self, ch: u8) {
        let idx = usize::from(ch) / 64;
        let bit = u64::from(ch) % 64;
        self.bits[idx] |= 1u64 << bit;
    }

    /// True when `ch` is in the set.
    pub fn contains(&self, ch: u8) -> bool {
        let idx = usize::from(ch) / 64;
        let bit = u64::from(ch) % 64;
        self.bits[idx] & (1u64 << bit) != 0
    }

    /// Number of channels in the set.
    pub fn len(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// True when the set is empty.
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|w| *w == 0)
    }

    /// The channel numbers, in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        (0u16..=255).filter_map(move |c| {
            let ch = c as u8;
            self.contains(ch).then_some(ch)
        })
    }
}

/// Which channels and which time window to export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportFilter {
    /// Channel subset. `None` exports every channel that holds
    /// entries.
    pub channels: Option<ChannelSet>,
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
        self.channels.get_or_insert_with(ChannelSet::new).insert(ch);
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
            Some(set) => set.contains(ch),
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
///
/// The drop ranges are **borrowed**, not copied: an export of a
/// long-running session would otherwise clone the whole drop history
/// into every chunk (M-19 review #7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportMeta<'a> {
    /// True when at least one channel dropped entries to overflow.
    pub overflow_marked: bool,
    /// Ranges whose content was dropped by a failed push.
    pub dropped_ranges: &'a [DropRange],
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
    /// Set once the merged stream has reached the filter's start
    /// bound. Past that point the start check is redundant (M-19
    /// review #16: the merged stream is time-ordered, so no entry
    /// after the window start can be before it).
    past_start: bool,
    /// Diagnostic counter: how many entries the cursor has pulled off
    /// the heap. Used by the export performance regression test to
    /// assert the session path is O(n) rather than O(n²/chunk).
    advances: u64,
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
        let past_start = filter.t_start.is_none();
        Self {
            streams,
            heap,
            filter,
            stopped: false,
            past_start,
            advances: 0,
        }
    }

    /// Next entry of the merged stream that passes the filter.
    pub fn next_entry(&mut self) -> Option<RingEntry> {
        if self.stopped {
            return None;
        }
        loop {
            let Reverse((t, _ch, si)) = self.heap.pop()?;
            self.advances += 1;
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
            if !self.past_start {
                if let Some(start) = self.filter.t_start {
                    if t < start {
                        continue;
                    }
                }
                // The stream is time-ordered, so every later entry is
                // at or after the window start — the check can be
                // dropped from here on.
                self.past_start = true;
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

    /// Diagnostic: how many entries the cursor has pulled off the
    /// heap since it was created.
    pub fn advances(&self) -> u64 {
        self.advances
    }
}

/// Count the entries of the merged stream that pass the filter.
///
/// Used for the export progress. This is an O(n) scan, but it does
/// not allocate — and an [`ExportSession`] computes it **once** at
/// session start, not once per chunk.
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

/// Escape one field for CSV output (RFC 4180).
///
/// A field is quoted when it contains a comma, a double quote, a
/// newline or a carriage return; embedded double quotes are doubled.
///
/// **NFR-04 (M-19 review #3, 2026-10-19):** the channel label is
/// host-supplied free text. Without escaping, a label containing a
/// comma silently breaks the column structure of the whole export.
pub fn csv_escape(field: &str) -> Cow<'_, str> {
    if !field.contains([',', '"', '\n', '\r']) {
        return Cow::Borrowed(field);
    }
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for c in field.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    Cow::Owned(out)
}

/// Sanitize a label for use inside a VCD `$var` reference.
///
/// The VCD `$var` line is `$var <type> <size> <id> <reference> $end`,
/// where the *reference* is the last field before `$end`. A reference
/// may contain dots and dashes (`GPIO0.OUT`, `BUS.A1.write`), but it
/// must not contain `$` (it would terminate the command early) or
/// control characters / newlines (they would break the line
/// structure).
///
/// The label is host-supplied free text, so everything outside the
/// safe set is replaced with `_` (M-19 review #3).
pub fn vcd_label(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '[' | ']' | '/') {
                c
            } else {
                '_'
            }
        })
        .collect()
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
    total: usize,
    meta: &ExportMeta,
) -> String {
    let mut out = String::new();
    if include_header {
        out.push_str("# riscv-sim waveform export (M-19)\n");
        let _ = writeln!(
            out,
            "# entries={total} overflow_marked={} dropped_ranges={}",
            meta.overflow_marked,
            meta.dropped_ranges.len()
        );
        for r in meta.dropped_ranges {
            let _ = writeln!(out, "# dropped_range={}..{}", r.seq_start, r.seq_end);
        }
        out.push_str("t_ps,ch,label,value,edge\n");
    }
    for e in entries {
        let label = metas
            .get(&e.ch)
            .map(|m| csv_escape(&m.label))
            .unwrap_or(Cow::Borrowed("ch?"));
        // `write!` writes straight into `out` — no intermediate
        // `String` per row (M-19 review #6).
        let _ = writeln!(
            out,
            "{},{},{},{},{}",
            e.t_ps,
            e.ch,
            label,
            e.value,
            edge_kind_text(e.kind)
        );
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
            let _ = writeln!(out, "$var wire {width} ch{ch} {} $end", vcd_label(label));
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
            let _ = writeln!(out, "#{}", e.t_ps);
            last_t = Some(e.t_ps);
        }
        let width = metas.get(&e.ch).map(|m| m.value_width).unwrap_or(1);
        if width <= 1 {
            let _ = writeln!(out, "{}ch{}", e.value & 1, e.ch);
        } else {
            let _ = writeln!(out, "b{:b} ch{}", e.value, e.ch);
        }
    }
    out
}

/// A resumable, chunked export.
///
/// The session keeps the merge cursor alive between chunks, so a
/// sequential export costs **O(n) total** rather than O(n²/chunk)
/// (M-19 review #4). It also computes the total entry count and the
/// time-ordering pass **once**, at session start.
///
/// The session borrows the pipeline immutably for its lifetime, so
/// the pipeline cannot be mutated while an export is in flight. That
/// is deliberate: the export is a read-only snapshot of the window
/// that existed when the session started.
pub struct ExportSession<'a> {
    cursor: MergedCursor<'a>,
    metas: &'a BTreeMap<u8, ChannelMeta>,
    format: ExportFormat,
    total: usize,
    consumed: usize,
    overflow_marked: bool,
    dropped_ranges: Vec<DropRange>,
    vcd_channels: Vec<u8>,
}

impl<'a> ExportSession<'a> {
    /// Build a session over the given rings.
    ///
    /// The caller is responsible for having time-ordered the rings
    /// first (the pipeline does this in `export_session`).
    pub fn new(
        rings: &'a BTreeMap<u8, super::ring::ChannelRing>,
        metas: &'a BTreeMap<u8, ChannelMeta>,
        filter: ExportFilter,
        format: ExportFormat,
        overflow_marked: bool,
        dropped_ranges: Vec<DropRange>,
    ) -> Self {
        let total = count_filtered(rings, &filter);
        let vcd_channels: Vec<u8> = match &filter.channels {
            Some(set) => set.iter().collect(),
            None => rings.keys().copied().collect(),
        };
        Self {
            cursor: MergedCursor::new(rings, filter),
            metas,
            format,
            total,
            consumed: 0,
            overflow_marked,
            dropped_ranges,
            vcd_channels,
        }
    }

    /// Total number of entries in the merged stream.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Number of entries already handed out.
    pub fn consumed(&self) -> usize {
        self.consumed
    }

    /// True when the whole stream has been handed out.
    pub fn is_done(&self) -> bool {
        self.consumed >= self.total
    }

    /// Diagnostic: how many merge-cursor advances the session has
    /// performed. A sequential export over `n` entries performs `n`
    /// advances in total — that is the O(n) property.
    pub fn advances(&self) -> u64 {
        self.cursor.advances()
    }

    /// Skip forward to a merged-stream offset.
    ///
    /// Used to resume an export at a known anchor. Costs
    /// O(next_offset) — prefer walking the session forward when
    /// exporting sequentially.
    pub fn skip_to(&mut self, next_offset: usize) {
        if next_offset <= self.consumed {
            return;
        }
        let want = next_offset - self.consumed;
        let skipped = self.cursor.skip(want);
        self.consumed += skipped;
    }

    /// Render the next chunk.
    pub fn next_chunk(&mut self) -> ExportHandle {
        let mut entries: Vec<RingEntry> = Vec::with_capacity(EXPORT_CHUNK.min(self.total));
        while entries.len() < EXPORT_CHUNK {
            match self.cursor.next_entry() {
                Some(e) => entries.push(e),
                None => break,
            }
        }
        self.consumed += entries.len();

        let done = self.consumed >= self.total;
        let progress_pct = if self.total == 0 {
            100
        } else {
            ((self.consumed as u128 * 100) / self.total as u128).min(100) as u8
        };

        // The header belongs to the very first chunk. Compute it from
        // the pre-increment position so it stays correct when the
        // session is resumed mid-stream.
        let first_chunk = self.consumed - entries.len() == 0;

        let meta = ExportMeta {
            overflow_marked: self.overflow_marked,
            dropped_ranges: &self.dropped_ranges,
        };

        let chunk = match self.format {
            ExportFormat::Csv => render_csv(&entries, self.metas, first_chunk, self.total, &meta),
            ExportFormat::Vcd => render_vcd(&entries, self.metas, &self.vcd_channels, first_chunk),
        };

        ExportHandle {
            chunk,
            next_offset: self.consumed,
            total: self.total,
            done,
            progress_pct,
            overflow_marked: self.overflow_marked,
            dropped_ranges: self.dropped_ranges.clone(),
        }
    }

    /// Render the next chunk, reporting progress to `on_progress`.
    pub fn next_chunk_with_progress(&mut self, on_progress: &mut dyn FnMut(u8)) -> ExportHandle {
        let h = self.next_chunk();
        on_progress(h.progress_pct);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m19_waveform::ring::ChannelRing;
    use riscv_sim_contract::model::EdgeKind;

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
    fn channel_set_is_a_bitmask() {
        let mut s = ChannelSet::new();
        assert!(s.is_empty());
        s.insert(0);
        s.insert(63);
        s.insert(64);
        s.insert(255);
        // Duplicate insertion is a no-op (the old Vec allowed dupes).
        s.insert(64);
        assert!(s.contains(0));
        assert!(s.contains(63));
        assert!(s.contains(64));
        assert!(s.contains(255));
        assert!(!s.contains(1));
        assert!(!s.contains(65));
        assert_eq!(s.len(), 4);
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![0, 63, 64, 255]);
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
            overflow_marked: false,
            dropped_ranges: &[],
        };
        let first = render_csv(&entries, &metas, true, 1, &meta);
        assert!(first.contains("t_ps,ch,label,value,edge"));
        assert!(first.contains("100,1,GPIO0.OUT,1,Rising"));
        let second = render_csv(&entries, &metas, false, 1, &meta);
        assert!(!second.contains("t_ps,ch,label,value,edge"));
        assert!(second.contains("100,1,GPIO0.OUT,1,Rising"));
    }

    #[test]
    fn csv_records_overflow_marker() {
        let metas = BTreeMap::new();
        let dropped = vec![DropRange {
            seq_start: 1,
            seq_end: 4,
        }];
        let meta = ExportMeta {
            overflow_marked: true,
            dropped_ranges: &dropped,
        };
        let out = render_csv(&[], &metas, true, 0, &meta);
        assert!(out.contains("overflow_marked=true"));
        assert!(out.contains("dropped_range=1..4"));
    }

    #[test]
    fn csv_escapes_host_supplied_labels() {
        // M-19 review #3 (NFR-04): a label with a comma must not
        // break the column structure.
        let mut metas = BTreeMap::new();
        metas.insert(
            0u8,
            ChannelMeta {
                label: "a,b\"c\nd".into(),
                value_width: 1,
                single_bit: true,
            },
        );
        let entries = vec![RingEntry::new(1, 0, 5, EdgeKind::Value)];
        let meta = ExportMeta {
            overflow_marked: false,
            dropped_ranges: &[],
        };
        let out = render_csv(&entries, &metas, true, 1, &meta);
        // The row must still parse as 5 fields, even though the
        // quoted label contains a newline (RFC 4180 allows a quoted
        // field to span lines).
        let rows = parse_csv(&out);
        let data = rows.last().expect("a data row");
        assert_eq!(data.len(), 5);
        assert_eq!(data[0], "1");
        assert_eq!(data[1], "0");
        assert_eq!(data[2], "a,b\"c\nd");
        assert_eq!(data[3], "5");
        assert_eq!(data[4], "Value");
    }

    #[test]
    fn csv_escape_leaves_plain_fields_untouched() {
        assert!(matches!(csv_escape("GPIO0.OUT"), Cow::Borrowed(_)));
        assert!(matches!(csv_escape("a,b"), Cow::Owned(_)));
    }

    #[test]
    fn vcd_sanitizes_host_supplied_labels() {
        // Dots and dashes stay (they are common in signal names and
        // legal in a VCD reference); `$`, whitespace and control
        // characters are neutralised.
        assert_eq!(vcd_label("GPIO0.OUT"), "GPIO0.OUT");
        assert_eq!(vcd_label("BUS.A1.write"), "BUS.A1.write");
        assert_eq!(vcd_label("has space"), "has_space");
        assert_eq!(vcd_label("$end"), "_end");
        assert_eq!(vcd_label("a\nb"), "a_b");
        assert_eq!(vcd_label("ok_1"), "ok_1");
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
    fn vcd_label_cannot_break_the_var_command() {
        // A label containing `$end` must not terminate the `$var`
        // command early.
        let mut metas = BTreeMap::new();
        metas.insert(1u8, ChannelMeta::single_bit("evil $end $var wire 1 x"));
        let entries = vec![RingEntry::new(100, 1, 1, EdgeKind::Rising)];
        let out = render_vcd(&entries, &metas, &[1], true);
        let var_line = out
            .lines()
            .find(|l| l.starts_with("$var"))
            .expect("a $var line");
        assert_eq!(var_line.matches("$end").count(), 1);
        assert!(var_line.ends_with("$end"));
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

    /// Minimal RFC 4180 parser used to assert the CSV column count
    /// survives hostile labels. Handles quoted fields that span
    /// lines, which is legal CSV.
    fn parse_csv(text: &str) -> Vec<Vec<String>> {
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut fields: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if in_quotes {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        cur.push('"');
                    } else {
                        in_quotes = false;
                    }
                } else {
                    cur.push(c);
                }
            } else if c == '"' {
                in_quotes = true;
            } else if c == ',' {
                fields.push(std::mem::take(&mut cur));
            } else if c == '\n' {
                fields.push(std::mem::take(&mut cur));
                rows.push(std::mem::take(&mut fields));
            } else {
                cur.push(c);
            }
        }
        if !cur.is_empty() || !fields.is_empty() {
            fields.push(cur);
            rows.push(fields);
        }
        rows.retain(|r| !r.is_empty() || r.len() > 1);
        rows
    }
}
