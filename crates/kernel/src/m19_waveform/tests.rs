//! M-19 unit tests (M-19 v0.9 §7.1, LLD test table 91 / 92).
//!
//! The cases follow the design doc's test table one-to-one:
//!
//! | Design case | Test |
//! | --- | --- |
//! | 边沿编码只记变化点 | [`edge_encoding_records_only_change_points`] |
//! | 批量节流推送 | [`push_delta_batches_with_monotonic_sequence`] |
//! | 波形刷新 ≤200ms | [`waveform_refresh_stays_within_200ms_budget`] |
//! | 溢出丢最旧 + 标记 | [`overflow_drops_oldest_and_marks`] |
//! | 暂停仿真策略 | [`pause_sim_policy_requests_a_pause_on_overflow`] |
//! | 容量规划对账 | [`budget_reconcile_rejects_an_over_budget_plan`] |
//! | 推送失败语义 | [`push_failure_drops_the_batch_and_records_the_range`] |
//! | 跨通道导出归并 | [`export_merges_channels_in_time_order`] |
//! | 调度者周期推送 | [`batch_due_follows_the_20ms_window`] |
//! | 导出续导 / 进度 | [`export_chunks_are_resumable_and_report_progress`] |
//! | 推送失败续拉 | [`dropped_ranges_identify_the_refillable_window`] |
//! | 契约测试 | the `contract_*` cases below |

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{EdgeEvent, EdgeKind, WaveformDelta};
use riscv_sim_contract::port::DeltaSink;

use crate::m18_probe::{ChannelDef, ProbeRegistry, SampleCfg, CH_GPIO0};
use crate::m19_waveform::{
    ChannelMeta, EncodeOutcome, ExportFilter, ExportFormat, OverflowPolicy, SharedPipeline,
    WaveformPipeline, WireFormat, ENTRY_BYTES, EXPORT_CHUNK, MEM_BUDGET_BYTES, RING_CAP,
};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// A delta subscriber that records what it received and can be told
/// to fail.
#[derive(Default)]
struct RecordingSink {
    batches: Vec<WaveformDelta>,
    fail: bool,
}

impl RecordingSink {
    fn failing() -> Self {
        Self {
            batches: Vec::new(),
            fail: true,
        }
    }
}

impl DeltaSink for RecordingSink {
    fn push_delta(&mut self, delta: &WaveformDelta) -> Result<(), PlatformError> {
        if self.fail {
            return Err(PlatformError::new(
                ErrorFamily::Observability,
                1,
                "test sink is down",
            ));
        }
        self.batches.push(delta.clone());
        Ok(())
    }
}

/// A sink that can be inspected from the outside while it is
/// installed inside the pipeline.
#[derive(Clone, Default)]
struct SharedSink {
    inner: Rc<RefCell<RecordingSink>>,
}

impl SharedSink {
    fn new() -> Self {
        Self::default()
    }

    fn failing() -> Self {
        Self {
            inner: Rc::new(RefCell::new(RecordingSink::failing())),
        }
    }

    fn batches(&self) -> Vec<WaveformDelta> {
        self.inner.borrow().batches.clone()
    }
}

impl DeltaSink for SharedSink {
    fn push_delta(&mut self, delta: &WaveformDelta) -> Result<(), PlatformError> {
        let mut guard = self.inner.borrow_mut();
        guard.push_delta(delta)
    }
}

fn rising(t: u64, ch: u8) -> EdgeEvent {
    EdgeEvent {
        t_ps: t,
        ch,
        value: 1,
        kind: EdgeKind::Rising,
    }
}

fn falling(t: u64, ch: u8) -> EdgeEvent {
    EdgeEvent {
        t_ps: t,
        ch,
        value: 0,
        kind: EdgeKind::Falling,
    }
}

/// A pipeline with one declared single-bit channel.
fn gpio_pipeline() -> WaveformPipeline {
    let mut p = WaveformPipeline::new();
    p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
    p
}

/// Extract the data rows of a CSV export, skipping the comment
/// metadata and the column header.
fn data_rows(chunk: &str) -> Vec<Vec<&str>> {
    chunk
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("t_ps"))
        .map(|l| l.split(',').collect())
        .collect()
}

/// Count the fields of one CSV row, honouring RFC 4180 quoting
/// (a comma inside a quoted field is not a separator).
fn csv_field_count(row: &str) -> usize {
    let mut fields = 1;
    let mut in_quotes = false;
    let mut chars = row.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields += 1;
        }
    }
    fields
}

// ---------------------------------------------------------------------------
// 边沿编码只记变化点
// ---------------------------------------------------------------------------

#[test]
fn edge_encoding_records_only_change_points() {
    let mut p = gpio_pipeline();
    p.encode_edge(rising(10, CH_GPIO0));
    p.encode_edge(falling(20, CH_GPIO0));
    // A same-value repeat is *not* re-filtered by M-19 — M-18 does
    // that. What M-19 guarantees is that it encodes exactly what it
    // is given, once, into the channel's window.
    assert_eq!(p.channel_len(CH_GPIO0), 2);
    assert_eq!(p.stats().buffered, 2);
}

#[test]
fn undeclared_channel_is_recorded_with_fallback_metadata() {
    let mut p = WaveformPipeline::new();
    let outcome = p.encode_edge(rising(10, 42));
    assert!(!outcome.overflowed);
    assert_eq!(p.channel_len(42), 1);
    let meta = p.channel_meta(42).expect("fallback metadata");
    assert_eq!(meta.label, "ch42");
}

#[test]
fn ring_entry_fits_the_memory_budget_accounting() {
    // The budget assumes 32 B per entry. The real struct must never
    // be *larger*, otherwise the 25 MB budget would be a lie.
    assert!(
        std::mem::size_of::<crate::m19_waveform::RingEntry>() as u64 <= ENTRY_BYTES,
        "RingEntry grew past the accounted {} B per entry",
        ENTRY_BYTES
    );
}

#[test]
fn memory_budget_matches_the_design_figure() {
    // 4 channels x 2e5 entries x 32 B.
    assert_eq!(MEM_BUDGET_BYTES, 25_600_000);
    assert_eq!(MEM_BUDGET_BYTES, 4 * RING_CAP as u64 * ENTRY_BYTES);
}

// ---------------------------------------------------------------------------
// 批量节流推送
// ---------------------------------------------------------------------------

#[test]
fn push_delta_batches_with_monotonic_sequence() {
    let mut p = gpio_pipeline();
    p.encode_edge(rising(10, CH_GPIO0));
    p.encode_edge(falling(20, CH_GPIO0));
    p.encode_edge(rising(30, CH_GPIO0));

    let d = p.push_delta(100).expect("batch");
    assert_eq!(d.seq_start, 0);
    assert_eq!(d.seq_end, 3);
    assert_eq!(d.t_virt, 100);
    assert_eq!(d.entries.len(), 3);

    // The next batch continues the sequence without a gap.
    p.encode_edge(falling(40, CH_GPIO0));
    let d2 = p.push_delta(200).expect("batch");
    assert_eq!(d2.seq_start, 3);
    assert_eq!(d2.seq_end, 4);
    assert!(d2.seq_start >= d.seq_end);
}

#[test]
fn push_delta_returns_none_when_nothing_is_pending() {
    let mut p = gpio_pipeline();
    assert!(p.push_delta(10).is_none());
    p.encode_edge(rising(10, CH_GPIO0));
    assert!(p.push_delta(20).is_some());
    // The batch was drained — no second push.
    assert!(p.push_delta(30).is_none());
}

#[test]
fn push_delta_delivers_to_the_installed_sink() {
    let mut p = gpio_pipeline();
    let sink = SharedSink::new();
    p.set_delta_sink(Some(Box::new(sink.clone())));
    p.encode_edge(rising(10, CH_GPIO0));
    p.push_delta(20);
    let batches = sink.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].entries.len(), 1);
}

// ---------------------------------------------------------------------------
// 调度者周期推送（19-5）
// ---------------------------------------------------------------------------

#[test]
fn batch_due_follows_the_20ms_window() {
    let mut p = gpio_pipeline();
    assert_eq!(p.batch_ms(), 20);
    assert_eq!(p.batch_window_ps(), 20_000_000);

    // Nothing pending: never due.
    assert!(!p.batch_due(1_000_000_000));

    p.encode_edge(rising(10, CH_GPIO0));
    // First batch: due immediately (there is no previous push).
    assert!(p.batch_due(10));

    p.push_delta(10);
    // Nothing pending: still not due.
    assert!(!p.batch_due(10 + 20_000_000));

    p.encode_edge(rising(15, CH_GPIO0));
    // Not yet due — the window has not elapsed.
    assert!(!p.batch_due(10 + 19_000_000));
    // Due once the window elapsed.
    assert!(p.batch_due(10 + 20_000_000));
}

#[test]
fn scheduler_drives_repeated_pushes() {
    // Emulate M-12's periodic callback: every 20 ms, flush the
    // pending batch.
    let mut p = gpio_pipeline();
    let sink = SharedSink::new();
    p.set_delta_sink(Some(Box::new(sink.clone())));

    let mut now = 0u64;
    for _ in 0..5u64 {
        p.encode_edge(rising(now, CH_GPIO0));
        now += 20_000_000;
        if p.batch_due(now) {
            p.push_delta(now);
        }
    }
    let batches = sink.batches();
    assert_eq!(batches.len(), 5);
    // Sequence numbers are contiguous across the batches.
    for (i, b) in batches.iter().enumerate() {
        assert_eq!(b.seq_start, i as u64);
        assert_eq!(b.seq_end, i as u64 + 1);
    }
}

// ---------------------------------------------------------------------------
// 波形刷新 ≤200ms（表91）
// ---------------------------------------------------------------------------

#[test]
fn waveform_refresh_stays_within_200ms_budget() {
    // 4 channels at the "regular" observation rate of ~4k events/s
    // per channel: 16k events, encoded and batched. The frontend
    // refresh budget is 200 ms.
    let mut p = WaveformPipeline::new();
    for ch in 0..4u8 {
        p.define_channel(ch, ChannelMeta::multi_bit(format!("CH{ch}"), 32));
    }
    let start = std::time::Instant::now();
    for i in 0..16_000u64 {
        p.encode_edge(EdgeEvent {
            t_ps: i * 1_000,
            ch: (i % 4) as u8,
            value: i,
            kind: EdgeKind::Value,
        });
    }
    let delta = p.push_delta(16_000_000).expect("batch");
    let elapsed = start.elapsed();
    assert_eq!(delta.entries.len(), 16_000);
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "waveform batch took {elapsed:?}, over the 200 ms refresh budget"
    );
}

// ---------------------------------------------------------------------------
// 溢出丢最旧 + 标记（图4）
// ---------------------------------------------------------------------------

#[test]
fn overflow_drops_oldest_and_marks() {
    let mut p = WaveformPipeline::with_ring_capacity(4);
    p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));

    for i in 0..6u64 {
        p.encode_edge(rising(i, CH_GPIO0));
    }

    assert_eq!(p.channel_len(CH_GPIO0), 4);
    assert_eq!(p.overflow_count(CH_GPIO0), 2);
    assert_eq!(p.stats().overflow_total, 2);
    // The newest window is retained (the two oldest were evicted).
    let exported = p.export_csv(&ExportFilter::all(), 0, None);
    assert!(exported.overflow_marked);
    let times: Vec<u64> = data_rows(&exported.chunk)
        .iter()
        .map(|r| r[0].parse().expect("numeric t_ps"))
        .collect();
    assert_eq!(times, vec![2, 3, 4, 5]);
    // Every retained row carries the channel label.
    assert!(data_rows(&exported.chunk)
        .iter()
        .all(|r| r[2] == "GPIO0.OUT" && r[1] == CH_GPIO0.to_string()));
}

#[test]
fn drop_oldest_policy_does_not_request_a_pause() {
    let mut p = WaveformPipeline::with_ring_capacity(2);
    p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
    let paused = Rc::new(RefCell::new(Vec::new()));
    {
        let paused = Rc::clone(&paused);
        p.set_pause_hook(Some(Box::new(move |ch| paused.borrow_mut().push(ch))));
    }
    for i in 0..5u64 {
        p.encode_edge(rising(i, CH_GPIO0));
    }
    assert!(paused.borrow().is_empty());
    assert_eq!(p.stats().pause_requests, 0);
}

// ---------------------------------------------------------------------------
// 暂停仿真策略（PAUSE_SIM）
// ---------------------------------------------------------------------------

#[test]
fn pause_sim_policy_requests_a_pause_on_overflow() {
    let mut p = WaveformPipeline::with_ring_capacity(2);
    p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
    let paused = Rc::new(RefCell::new(Vec::new()));
    {
        let paused = Rc::clone(&paused);
        p.set_pause_hook(Some(Box::new(move |ch| paused.borrow_mut().push(ch))));
    }

    p.set_overflow_policy(OverflowPolicy::PauseSim);
    assert_eq!(p.overflow_policy(), OverflowPolicy::PauseSim);

    // Not full yet: no pause.
    p.encode_edge(rising(1, CH_GPIO0));
    p.encode_edge(rising(2, CH_GPIO0));
    assert!(paused.borrow().is_empty());

    // Third event overflows the ring -> pause requested for that channel.
    let outcome = p.encode_edge(rising(3, CH_GPIO0));
    assert!(outcome.overflowed);
    assert!(outcome.pause_requested);
    assert_eq!(*paused.borrow(), vec![CH_GPIO0]);
    assert_eq!(p.stats().pause_requests, 1);
}

#[test]
fn overflow_policy_rejects_an_unknown_code_and_keeps_the_current_one() {
    let mut p = WaveformPipeline::new();
    p.set_overflow_policy(OverflowPolicy::PauseSim);
    let err = p
        .set_overflow_policy_code(7)
        .expect_err("unknown code must be rejected");
    assert_eq!(err.family, ErrorFamily::Observability);
    assert_eq!(err.scenario, 1);
    // The current policy is preserved.
    assert_eq!(p.overflow_policy(), OverflowPolicy::PauseSim);
    // A known code is accepted.
    p.set_overflow_policy_code(0).expect("known code");
    assert_eq!(p.overflow_policy(), OverflowPolicy::DropOldestMark);
}

// ---------------------------------------------------------------------------
// 容量规划对账（19-2）
// ---------------------------------------------------------------------------

#[test]
fn budget_reconcile_rejects_an_over_budget_plan() {
    let mut p = WaveformPipeline::new();
    let throttles = Rc::new(RefCell::new(Vec::new()));
    {
        let throttles = Rc::clone(&throttles);
        p.set_rate_hook(Some(Box::new(move |n| throttles.borrow_mut().push(n))));
    }

    // The default plan fits.
    p.reconcile_budget(4, RING_CAP as u32)
        .expect("default plan fits");
    assert_eq!(p.budget_reconciles(), 0);
    assert!(throttles.borrow().is_empty());

    // Growing the window past the budget is rejected and forces
    // downsampling.
    let err = p
        .reconcile_budget(8, RING_CAP as u32)
        .expect_err("over budget");
    assert_eq!(err.family, ErrorFamily::Context);
    assert_eq!(err.scenario, 2);
    assert_eq!(p.budget_reconciles(), 1);
    assert_eq!(*throttles.borrow(), vec![2]);
    assert_eq!(p.throttle_n(), 2);
}

// ---------------------------------------------------------------------------
// 推送失败语义（19-3）
// ---------------------------------------------------------------------------

#[test]
fn push_failure_drops_the_batch_and_records_the_range() {
    let mut p = gpio_pipeline();
    p.set_delta_sink(Some(Box::new(SharedSink::failing())));

    p.encode_edge(rising(10, CH_GPIO0));
    p.encode_edge(falling(20, CH_GPIO0));
    let d = p.push_delta(30).expect("batch is still produced");

    // The sequence space moved forward regardless of the failure.
    assert_eq!(d.seq_start, 0);
    assert_eq!(d.seq_end, 2);
    assert_eq!(p.dropped_batches(), 1);

    // The lost content is recorded as a range.
    let ranges = p.dropped_ranges_vec();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].seq_start, 0);
    assert_eq!(ranges[0].seq_end, 2);
    assert_eq!(ranges[0].len(), 2);

    // The next batch continues the sequence without a gap.
    p.encode_edge(rising(40, CH_GPIO0));
    let d2 = p.push_delta(50).expect("batch");
    assert_eq!(d2.seq_start, 2);
    assert_eq!(d2.seq_end, 3);
}

#[test]
fn push_failure_does_not_block_the_pipeline() {
    let mut p = gpio_pipeline();
    p.set_delta_sink(Some(Box::new(SharedSink::failing())));
    for i in 0..100u64 {
        p.encode_edge(rising(i, CH_GPIO0));
    }
    // The batch is produced, dropped, and the pipeline keeps going.
    let d = p.push_delta(100).expect("batch");
    assert_eq!(d.entries.len(), 100);
    assert_eq!(p.dropped_batches(), 1);
    assert!(p.push_delta(200).is_none());
}

// ---------------------------------------------------------------------------
// 推送失败续拉（P-09）
// ---------------------------------------------------------------------------

#[test]
fn dropped_ranges_identify_the_refillable_window() {
    let mut p = gpio_pipeline();
    p.set_delta_sink(Some(Box::new(SharedSink::failing())));

    // Three batches, all dropped.
    for batch in 0..3u64 {
        for i in 0..2u64 {
            p.encode_edge(rising(batch * 10 + i, CH_GPIO0));
        }
        p.push_delta(batch * 20);
    }

    let ranges = p.dropped_ranges_vec();
    assert_eq!(ranges.len(), 1);
    // Adjacent ranges are merged into one contiguous hole — the
    // consumer can request a refill for the whole window.
    assert_eq!(ranges[0].seq_start, 0);
    assert_eq!(ranges[0].seq_end, 6);
}

// ---------------------------------------------------------------------------
// 跨通道导出归并（19-4）
// ---------------------------------------------------------------------------

#[test]
fn export_merges_channels_in_time_order() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    p.define_channel(1, ChannelMeta::multi_bit("UART0.TX", 8));

    p.encode_edge(rising(30, 0));
    p.encode_edge(EdgeEvent {
        t_ps: 10,
        ch: 1,
        value: 0x41,
        kind: EdgeKind::Value,
    });
    p.encode_edge(EdgeEvent {
        t_ps: 20,
        ch: 1,
        value: 0x42,
        kind: EdgeKind::Value,
    });

    let handle = p.export_csv(&ExportFilter::all(), 0, None);
    assert!(handle.done);
    assert_eq!(handle.total, 3);
    assert_eq!(handle.progress_pct, 100);

    // The rows are ordered by time, not by channel.
    let times: Vec<u64> = handle
        .chunk
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("t_ps"))
        .filter_map(|l| l.split(',').next()?.parse().ok())
        .collect();
    assert_eq!(times, vec![10, 20, 30]);
}

#[test]
fn export_is_deterministic_for_the_same_input() {
    let mut p = gpio_pipeline();
    for i in 0..50u64 {
        p.encode_edge(rising(i, CH_GPIO0));
    }
    let a = p.export_csv(&ExportFilter::all(), 0, None);
    let b = p.export_csv(&ExportFilter::all(), 0, None);
    assert_eq!(a.chunk, b.chunk);
}

#[test]
fn export_filter_selects_channels_and_time_window() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    p.define_channel(1, ChannelMeta::single_bit("GPIO1.OUT"));
    p.encode_edge(rising(10, 0));
    p.encode_edge(rising(20, 1));
    p.encode_edge(rising(30, 0));

    let by_channel = p.export_csv(&ExportFilter::all().with_channel(1), 0, None);
    assert_eq!(by_channel.total, 1);
    assert!(by_channel.chunk.contains("GPIO1.OUT"));
    assert!(!by_channel.chunk.contains("GPIO0.OUT"));

    let by_time = p.export_csv(&ExportFilter::all().with_time_range(15, 25), 0, None);
    assert_eq!(by_time.total, 1);
    assert!(by_time.chunk.contains("20,"));
}

// ---------------------------------------------------------------------------
// 导出续导 / 进度（FR-14）
// ---------------------------------------------------------------------------

#[test]
fn export_chunks_are_resumable_and_report_progress() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    let n = EXPORT_CHUNK + 25;
    for i in 0..n as u64 {
        p.encode_edge(rising(i, 0));
    }

    let mut progress_log: Vec<u8> = Vec::new();
    let mut offset = 0usize;
    let mut chunks: Vec<String> = Vec::new();
    let mut done = false;
    let mut guard = 0;
    while !done {
        let mut cb = |pct: u8| progress_log.push(pct);
        let h = p.export_csv(&ExportFilter::all(), offset, Some(&mut cb));
        offset = h.next_offset;
        done = h.done;
        chunks.push(h.chunk);
        guard += 1;
        assert!(guard < 10, "export did not terminate");
    }

    assert_eq!(chunks.len(), 2);
    // Only the first chunk carries the header.
    assert!(chunks[0].starts_with("# riscv-sim waveform export"));
    assert!(!chunks[1].starts_with("# riscv-sim waveform export"));
    // Progress was reported for both chunks and ends at 100.
    assert_eq!(progress_log.len(), 2);
    assert_eq!(*progress_log.last().unwrap(), 100);
    assert!(progress_log[0] < 100);
    assert_eq!(offset, n);
}

#[test]
fn export_resume_offset_continues_where_it_stopped() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    for i in 0..100u64 {
        p.encode_edge(rising(i, 0));
    }
    let first = p.export_csv(&ExportFilter::all(), 0, None);
    assert_eq!(first.total, 100);
    assert_eq!(first.next_offset, 100);

    // Resuming from an arbitrary offset continues where it stopped.
    let mid = p.export_csv(&ExportFilter::all(), 50, None);
    let times: Vec<u64> = data_rows(&mid.chunk)
        .iter()
        .map(|r| r[0].parse().expect("numeric t_ps"))
        .collect();
    assert_eq!(times.first().copied(), Some(50));
    assert_eq!(times.last().copied(), Some(99));
    assert_eq!(times.len(), 50);
}

#[test]
fn export_reports_the_overflow_marker() {
    let mut p = WaveformPipeline::with_ring_capacity(2);
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    for i in 0..5u64 {
        p.encode_edge(rising(i, 0));
    }
    let h = p.export_csv(&ExportFilter::all(), 0, None);
    assert!(h.overflow_marked);
    assert!(h.chunk.contains("overflow_marked=true"));
}

#[test]
fn vcd_export_declares_variables_and_value_changes() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    p.encode_edge(rising(100, 0));
    p.encode_edge(falling(200, 0));

    let h = p.export_vcd(&ExportFilter::all(), 0, None);
    assert!(h.chunk.contains("$timescale 1ps $end"));
    assert!(h.chunk.contains("$var wire 1 ch0 GPIO0.OUT $end"));
    assert!(h.chunk.contains("#100"));
    assert!(h.chunk.contains("1ch0"));
    assert!(h.chunk.contains("#200"));
    assert!(h.chunk.contains("0ch0"));
}

// ---------------------------------------------------------------------------
// 契约测试（LLD §9.1.1 端口契约）
// ---------------------------------------------------------------------------

#[test]
fn contract_edge_sink_port_is_usable_from_m18() {
    // The port contract: M-18 calls `encode_edge` with the classified
    // edge; M-19 records it.
    let shared = SharedPipeline::new(gpio_pipeline());
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(CH_GPIO0, "GPIO0", "OUT"))
        .expect("channel");
    reg.set_edge_sink(Some(Box::new(shared.clone())));
    assert!(reg.has_edge_sink());

    let mut cfg = SampleCfg {
        master_enable: true,
        ..Default::default()
    };
    cfg.enable_channel(CH_GPIO0);
    reg.set_rate(cfg);

    reg.sample(10, CH_GPIO0, 1).expect("first sample emits");
    reg.sample(20, CH_GPIO0, 0).expect("second sample emits");
    // A same-value repeat is filtered by M-18 (edge-only contract).
    assert!(reg.sample(30, CH_GPIO0, 0).is_none());

    let p = shared.pipeline();
    assert_eq!(p.channel_len(CH_GPIO0), 2);
    assert_eq!(p.stats().pending, 2);
}

#[test]
fn contract_demo_gpio_toggle_produces_rising_and_falling() {
    // The M-19 design doc's demo case: writing the GPIO OUT latch
    // flips a pin, and the probe drain yields one rising and one
    // falling event.
    let shared = SharedPipeline::new(gpio_pipeline());
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(CH_GPIO0, "GPIO0", "OUT"))
        .expect("channel");
    reg.set_edge_sink(Some(Box::new(shared.clone())));
    let mut cfg = SampleCfg {
        master_enable: true,
        ..Default::default()
    };
    cfg.enable_channel(CH_GPIO0);
    reg.set_rate(cfg);

    // The pin starts low: the first observation has no prior to
    // compare against, so M-18 reports it as a plain `Value` (the
    // initial level) rather than a transition.
    reg.sample(10, CH_GPIO0, 0).expect("initial level");
    // Then the peripheral flips the pin: 0 -> 1 -> 0.
    reg.sample(20, CH_GPIO0, 1).expect("rising");
    reg.sample(30, CH_GPIO0, 0).expect("falling");

    let mut p = shared.pipeline_mut();
    let delta = p.push_delta(40).expect("batch");
    assert_eq!(delta.entries.len(), 3);
    assert_eq!(delta.entries[0].kind, EdgeKind::Value);
    assert_eq!(delta.entries[1].kind, EdgeKind::Rising);
    assert_eq!(delta.entries[2].kind, EdgeKind::Falling);
    assert_eq!(delta.entries[1].t_ps, 20);
    assert_eq!(delta.entries[2].t_ps, 30);
}

#[test]
fn contract_shared_pipeline_drops_contended_observations_without_panicking() {
    let shared = SharedPipeline::new(gpio_pipeline());
    {
        // Hold a borrow, then feed the sink: the observation is
        // dropped and counted instead of panicking (M-18 review #3's
        // lesson).
        let _guard = shared.pipeline_mut();
        let mut sink = shared.clone();
        riscv_sim_contract::port::EdgeSink::encode_edge(&mut sink, rising(10, CH_GPIO0));
        assert_eq!(shared.contended(), 1);
    }
    assert_eq!(shared.pipeline().channel_len(CH_GPIO0), 0);
}

#[test]
fn contract_pipeline_implements_the_edge_sink_port_directly() {
    let mut p = gpio_pipeline();
    riscv_sim_contract::port::EdgeSink::encode_edge(&mut p, rising(10, CH_GPIO0));
    assert_eq!(p.channel_len(CH_GPIO0), 1);
}

#[test]
fn contract_wire_format_is_configurable() {
    let mut p = WaveformPipeline::new();
    assert_eq!(p.wire_format(), WireFormat::BinLeHeader);
    p.set_wire_format(WireFormat::BinLeCompressed);
    assert_eq!(p.wire_format(), WireFormat::BinLeCompressed);
}

#[test]
fn contract_empty_export_is_a_valid_empty_document() {
    let mut p = WaveformPipeline::new();
    let h = p.export_csv(&ExportFilter::all(), 0, None);
    assert!(h.done);
    assert_eq!(h.total, 0);
    assert_eq!(h.progress_pct, 100);
    assert!(h.chunk.contains("t_ps,ch,label,value,edge"));
}

// ---------------------------------------------------------------------------
// M-19 review 闭环回归（2026-10-19）
//
// 下面这些用例对应 docs/reviews/M-19_WaveformPipeline_Review.md 的问题项，
// 每条都锁定一个「修之前会失败、修之后必须成立」的不变量。
// ---------------------------------------------------------------------------

/// #1 阻断级：`ChannelRing::clone()` 之后调用 `capacity()` 不得 panic。
///
/// 旧实现在 `capacity()` 里断言 `buf.capacity() == initial_cap`，而
/// `Vec::clone` 只按 `len` 分配容量，前提必然不成立。
#[test]
fn review_1_clone_then_capacity_does_not_panic() {
    let mut p = WaveformPipeline::with_ring_capacity(8);
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    p.encode_edge(rising(1, 0));
    // The pipeline's rings are cloned through the public API path.
    let mut ring = crate::m19_waveform::ChannelRing::with_capacity(8);
    ring.push(crate::m19_waveform::RingEntry::new(
        1,
        0,
        0,
        EdgeKind::Value,
    ));
    let c = ring.clone();
    assert_eq!(c.capacity(), 8);
    assert_eq!(c.len(), 1);
}

/// #2 高危：没有安装 `delta_sink` 时推送，内容空洞必须**可查**。
///
/// 旧实现把批次取走、序列号推进，却什么都不记 —— 消费方永远不知道
/// 内容没到过 sink。
#[test]
fn review_2_push_without_a_sink_is_recorded_as_unconsumed() {
    let mut p = gpio_pipeline();
    p.encode_edge(rising(10, CH_GPIO0));
    p.encode_edge(falling(20, CH_GPIO0));
    let d = p.push_delta(30).expect("batch");
    assert_eq!(d.seq_start, 0);
    assert_eq!(d.seq_end, 2);

    // 「没到 sink」是可见的。
    let unconsumed: Vec<_> = p.unconsumed_ranges().copied().collect();
    assert_eq!(unconsumed.len(), 1);
    assert_eq!(unconsumed[0].seq_start, 0);
    assert_eq!(unconsumed[0].seq_end, 2);
    assert_eq!(p.unconsumed_batches(), 1);

    // 但它不是「投递失败」—— 内容交到了直接调用方手里。
    assert_eq!(p.dropped_batches(), 0);
    assert!(p.dropped_ranges_vec().is_empty());
}

/// #2 高危（另一半）：装了 sink 但投递失败 → 记 `dropped`，不记 unconsumed。
#[test]
fn review_2_push_failure_is_recorded_as_dropped_not_unconsumed() {
    let mut p = gpio_pipeline();
    p.set_delta_sink(Some(Box::new(SharedSink::failing())));
    p.encode_edge(rising(10, CH_GPIO0));
    p.push_delta(20);
    assert_eq!(p.dropped_batches(), 1);
    assert_eq!(p.unconsumed_batches(), 0);
    let dropped = p.dropped_ranges_vec();
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].seq_start, 0);
    assert_eq!(dropped[0].seq_end, 1);
}

/// #3 高危：宿主任意的 label 不得破坏 CSV 列结构 / VCD 语法。
#[test]
fn review_3_hostile_labels_do_not_break_the_export() {
    let mut p = WaveformPipeline::new();
    p.define_channel(
        0,
        ChannelMeta {
            label: "a,b\"c$end d".into(),
            value_width: 1,
            single_bit: true,
        },
    );
    p.encode_edge(rising(1, 0));

    // CSV：数据行的列数必须仍是 5（标签被引号包起来，标签内部的逗号
    // 不算分隔符）。
    let csv = p.export_csv(&ExportFilter::all(), 0, None);
    let data = csv
        .chunk
        .lines()
        .find(|l| l.starts_with("1,"))
        .expect("data row");
    assert_eq!(csv_field_count(data), 5);

    // VCD：`$var` 命令不能被标签里的 `$end` 提前截断。
    let vcd = p.export_vcd(&ExportFilter::all(), 0, None);
    let var_line = vcd
        .chunk
        .lines()
        .find(|l| l.starts_with("$var"))
        .expect("a $var line");
    assert_eq!(var_line.matches("$end").count(), 1);
}

/// #4 高危：分块导出必须是 O(n) 总代价，不是 O(n²/chunk)。
///
/// 旧实现每个 chunk 重建归并游标并从 0 重新 skip 一遍。
#[test]
fn review_4_sequential_export_is_linear_in_total_entries() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    let n = EXPORT_CHUNK * 2 + 500;
    for i in 0..n as u64 {
        p.encode_edge(rising(i, 0));
    }

    let mut session = p.export_session(ExportFormat::Csv, ExportFilter::all());
    let mut chunks = 0;
    loop {
        let h = session.next_chunk();
        chunks += 1;
        assert!(chunks < 10, "export did not terminate");
        if h.done {
            break;
        }
    }
    assert_eq!(chunks, 3);
    // 关键断言：归并游标总共只被推进了 n 次。
    // 旧实现会是 ~n²/(2·CHUNK) 量级。
    assert_eq!(session.advances(), n as u64);
}

/// #4 高危（另一半）：一次性 / 续导出入口仍然可用，且续导锚点正确。
#[test]
fn review_4_resume_entry_point_keeps_the_anchor() {
    let mut p = WaveformPipeline::new();
    p.define_channel(0, ChannelMeta::single_bit("GPIO0.OUT"));
    for i in 0..50u64 {
        p.encode_edge(rising(i, 0));
    }
    let h = p.export_csv(&ExportFilter::all(), 0, None);
    assert_eq!(h.total, 50);
    assert_eq!(h.next_offset, 50);
    assert!(h.done);
    let resumed = p.export_csv(&ExportFilter::all(), 25, None);
    assert_eq!(resumed.total, 50);
    assert_eq!(resumed.next_offset, 50);
}

/// #9 中危：自动建环不得绕过内存预算。
#[test]
fn review_9_automatic_ring_creation_respects_the_memory_budget() {
    let mut p = WaveformPipeline::new();
    // 4 个通道刚好占满 25.6 MB 预算。
    for ch in 0..4u8 {
        p.encode_edge(rising(0, ch));
    }
    assert_eq!(p.stats().channel_count, 4);
    assert_eq!(p.rings_refused(), 0);

    // 第 5 个超预算 → 拒绝建环。
    let outcome = p.encode_edge(rising(0, 4));
    assert!(outcome.ring_refused);
    assert_eq!(p.stats().channel_count, 4);
    assert_eq!(p.rings_refused(), 1);
    // 但事件仍然进入实时推送流（只有历史窗口缺失）。
    assert_eq!(p.stats().pending, 5);
}

/// #10 中危：共享管道的访问口径必须一致 —— 提供不 panic 的孪生方法。
#[test]
fn review_10_shared_pipeline_has_non_panicking_accessors() {
    let shared = SharedPipeline::new(gpio_pipeline());
    let guard = shared.try_pipeline_mut().expect("free borrow");
    // 第二次可变借用被拒绝，而不是 panic。
    assert!(shared.try_pipeline_mut().is_err());
    assert!(shared.try_pipeline().is_err());
    drop(guard);
    // 释放后恢复可用。
    assert!(shared.try_pipeline_mut().is_ok());
}

/// #14 低危：`EdgeSink` 端口返回 `()`，编码结果需要观察者入口。
#[test]
fn review_14_last_outcome_exposes_the_encode_result() {
    let mut p = WaveformPipeline::with_ring_capacity(2);
    p.define_channel(CH_GPIO0, ChannelMeta::single_bit("GPIO0.OUT"));
    p.encode_edge(rising(1, CH_GPIO0));
    assert_eq!(
        p.last_outcome(),
        EncodeOutcome {
            overflowed: false,
            pause_requested: false,
            ring_refused: false
        }
    );
    p.encode_edge(rising(2, CH_GPIO0));
    assert!(!p.last_outcome().overflowed);
    // 第三个事件撑爆容量 → 溢出被观察到。
    p.encode_edge(rising(3, CH_GPIO0));
    assert!(p.last_outcome().overflowed);
}

/// #7 / #8 中危：丢弃区间有界、校验、且相邻合并。
#[test]
fn review_8_drop_ranges_are_validated_and_merged() {
    let mut p = gpio_pipeline();
    p.set_delta_sink(Some(Box::new(SharedSink::failing())));
    // 连续三个批次全部投递失败 → 合并成一个连续空洞。
    for batch in 0..3u64 {
        p.encode_edge(rising(batch * 10, CH_GPIO0));
        p.push_delta(batch * 20);
    }
    let ranges = p.dropped_ranges_vec();
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].seq_start, 0);
    assert_eq!(ranges[0].seq_end, 3);
    assert_eq!(p.dropped_batches(), 3);
}
