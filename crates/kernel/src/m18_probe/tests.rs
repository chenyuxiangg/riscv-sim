//! M-18 unit tests (LLD §7.1 / M-18 v0.8 §7.1).
//!
//! Coverage map (≥8 cases per PROGRESS §6.3):
//!
//! | # | Test name                              | Spec ref                          |
//! |---|----------------------------------------|-----------------------------------|
//! | 1 | edge_only_emits_on_value_change        | §18-3 / "edge only on change"     |
//! | 2 | single_bit_rising_and_falling          | §18-3 / SingleBit classify        |
//! | 3 | multi_bit_value_change_no_kind         | §18-3 / MultiBit collapse         |
//! | 4 | pulse_channel_emits_every_event        | §18-2 / N2 IRQ pulse              |
//! | 5 | master_enable_zero_overhead            | §3.3 / performance switch         |
//! | 6 | channel_enable_bitmask_filter          | §3.3 / per-channel enable         |
//! | 7 | throttle_reduces_event_rate            | §18-4 / setRate throttleN         |
//! | 8 | attach_conflict_rejects_existing       | §18-6 / E_CTX-02 semantics        |
//! | 9 | bus_trace_ring_drops_oldest            | §18-1 / 2×10⁵ ring + dropped     |
//! |10 | drain_waveform_resets_edges            | §5.1 / M-19 boundary              |
//! |11 | unbound_signal_dropped_silently        | §6.1 / unbound signal             |
//! |12 | bus_hook_routes_into_trace             | §3.1 / attachTrace                |
//! |13 | attach_signal_routes_into_channel      | §3.2 / attachSignal               |
//!
//! The test count is intentionally above the PROGRESS bar so that a
//! future refactor that breaks one case still has ≥8 coverage.

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{Edge, TraceEvent};

use crate::m14_bus::{AccessDir, BusAccess, BusWidth};
use crate::m18_probe::{
    channel::{ChannelDef, CH_GPIO0, CH_IRQ_PULSE, CH_KEY_STATE},
    registry::{ProbeError, ProbeRegistry, SampleCfg},
    trace_stream::TRACE_CAP,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_bus_access(ts_ps: u64, addr: u32, w: BusWidth, dir: AccessDir, ok: bool) -> BusAccess {
    BusAccess {
        ts_ps,
        addr,
        width: w,
        dir,
        ok,
        err_scenario: if ok { None } else { Some(1) },
        is_fetch: false,
    }
}

fn default_cfg() -> SampleCfg {
    SampleCfg {
        master_enable: true,
        enable: 0,
        throttle_n: 1,
    }
}

// ---------------------------------------------------------------------------
// 1. Edge only on value change (M-18 v0.8 §18-3)
// ---------------------------------------------------------------------------

#[test]
fn edge_only_emits_on_value_change() {
    // Multi-bit channel so the kind would be Value either way;
    // what we assert here is the *quantity* of edges: same value
    // never re-emits.
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "TEST", "sig0", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    reg.set_rate(cfg);

    assert!(reg.sample(0, 0, 0xCAFE_BABE).is_some());
    assert!(
        reg.sample(1, 0, 0xCAFE_BABE).is_none(),
        "same value, no event"
    );
    assert!(
        reg.sample(2, 0, 0xCAFE_BABE).is_none(),
        "still same, no event"
    );
    assert!(reg.sample(3, 0, 0xDEAD_BEEF).is_some(), "new value, emit");
    assert_eq!(reg.edge_count(), 2);
}

// ---------------------------------------------------------------------------
// 2. Single-bit rising / falling (M-18 v0.8 §18-3)
// ---------------------------------------------------------------------------

#[test]
fn single_bit_rising_and_falling() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(0, "GPIO0", "OUT"))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    reg.set_rate(cfg);

    // Initial transition from undefined-prev (defaulted to value)
    // will emit because we never saw 0 first. After that, only
    // toggles emit.
    let rising = reg.sample(0, 0, 1).expect("first observation emits");
    assert_eq!(rising.value, 1);

    // No event on repeat
    assert!(reg.sample(1, 0, 1).is_none());

    // Falling
    let falling = reg.sample(2, 0, 0).expect("falling emits");
    assert_eq!(falling.value, 0);
    assert_eq!(falling.t_ps, 2);

    // Rising again
    let rising2 = reg.sample(3, 0, 1).expect("rising emits");
    assert_eq!(rising2.t_ps, 3);

    assert_eq!(reg.edge_count(), 3);
}

// ---------------------------------------------------------------------------
// 3. Multi-bit value change (kind collapsed to Value)
// ---------------------------------------------------------------------------

#[test]
fn multi_bit_value_change_does_not_classify_edges() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(CH_GPIO0, "GPIO0", "OUT", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(CH_GPIO0);
    reg.set_rate(cfg);

    let e = reg.sample(0, CH_GPIO0, 0x1).unwrap();
    assert_eq!(e.value, 0x1);
    let e = reg.sample(1, CH_GPIO0, 0x2).unwrap();
    assert_eq!(e.value, 0x2);
    // The Edge struct (existing contract type) does not carry a
    // kind field — it predates M-18 v0.8. We can't assert kind
    // directly, but we *can* assert that two distinct values
    // produce two events, which is the observable consequence of
    // "kind collapsed to Value" (a real Rising/Falling filter would
    // have produced no event for 0x1 → 0x2 on a single-bit channel;
    // we use a multi-bit channel here so the kind-collapse matters).
    assert_eq!(reg.edge_count(), 2);
}

// ---------------------------------------------------------------------------
// 4. Pulse channel emits every event (M-18 v0.8 §18-2, N2)
// ---------------------------------------------------------------------------

#[test]
fn pulse_channel_emits_every_event() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::pulse(CH_IRQ_PULSE, "M16", "irq_pulse"))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(CH_IRQ_PULSE);
    reg.set_rate(cfg);

    // Same value, twice. Pulse must emit both times (the IRQ line
    // is a "request happened" event, not a level signal).
    let e1 = reg.sample(0, CH_IRQ_PULSE, 5).expect("pulse 1 emits");
    let e2 = reg
        .sample(1, CH_IRQ_PULSE, 5)
        .expect("pulse 2 emits (same line)");
    let e3 = reg
        .sample(2, CH_IRQ_PULSE, 7)
        .expect("pulse 3 emits (different line)");

    assert_eq!(e1.t_ps, 0);
    assert_eq!(e2.t_ps, 1);
    assert_eq!(e3.t_ps, 2);
    assert_eq!(reg.edge_count(), 3);

    // `sample_irq_pulse` convenience routes to CH_IRQ_PULSE only
    // if the channel is registered as Pulse. This exercises that
    // path:
    let e4 = reg
        .sample_irq_pulse(3, 5)
        .expect("irq_pulse convenience emits");
    assert_eq!(e4.t_ps, 3);
}

// ---------------------------------------------------------------------------
// 5. master_enable = false → zero-overhead (M-18 v0.8 §3.3)
// ---------------------------------------------------------------------------

#[test]
fn master_enable_false_returns_none_for_every_call() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(0, "TEST", "x"))
        .unwrap();
    let cfg = SampleCfg {
        master_enable: false, // ← the test condition
        enable: 0xFFFF_FFFF,  // even with all channels enabled
        throttle_n: 1,
    };
    reg.set_rate(cfg);

    for t in 0..10u64 {
        assert!(reg.sample(t, 0, t).is_none(), "master off, t={t}");
    }
    assert_eq!(reg.edge_count(), 0);
}

// ---------------------------------------------------------------------------
// 6. Channel enable bitmask filter (M-18 v0.8 §3.3)
// ---------------------------------------------------------------------------

#[test]
fn channel_enable_bitmask_filters_disabled_channels() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(0, "A", "a"))
        .unwrap();
    reg.define_channel(ChannelDef::single_bit(1, "B", "b"))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    cfg.enable_channel(1);
    reg.set_rate(cfg);

    // Disable ch=1 via clear-bit.
    let mut cfg = reg.sample_cfg().clone();
    cfg.enable &= !(1u32 << 1);
    reg.set_rate(cfg);

    assert!(reg.sample(0, 0, 1).is_some(), "ch=0 still enabled");
    assert!(reg.sample(0, 1, 1).is_none(), "ch=1 disabled");
    assert_eq!(reg.edge_count(), 1);
}

// ---------------------------------------------------------------------------
// 7. Throttle reduces event rate (M-18 v0.8 §18-4)
// ---------------------------------------------------------------------------

#[test]
fn throttle_n_emits_every_nth_observation() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "TEST", "t", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    cfg.throttle_n = 4;
    reg.set_rate(cfg);

    // The actual code path (M-18 v0.8 §3.3 + the implementation in
    // `registry.rs::sample_inner`):
    //   counter starts at `throttle_n`
    //   if *counter > 1:  *counter -= 1;  return None   (skip)
    //   else (counter == 1):  *counter = throttle_n;  emit
    //
    // So with throttle_n = 4: obs 1 → c=4 > 1, c=3, skip; obs 2 →
    // c=3 → c=2, skip; obs 3 → c=2 → c=1, skip; obs 4 → c=1
    // (NOT > 1), reset to 4, emit. Then the pattern repeats every
    // 4 observations. Over 10 observations: emit on the 4th and
    // 8th → 2 events.
    let mut emitted = 0;
    for i in 0..10u64 {
        if reg.sample(i, 0, i * 100).is_some() {
            emitted += 1;
        }
    }
    assert_eq!(emitted, 2, "throttle=4 → emit on 4th, 8th (2 events)");
}

#[test]
fn throttle_state_survives_set_rate() {
    // M-18 review #15 (2026-10-19): regression test — the design
    // doc says setRate does not reset per-channel throttle
    // counters. Verify the property is locked down.
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "T", "t", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    cfg.throttle_n = 2;
    reg.set_rate(cfg);

    // Run one observation: counter 2 → 1 (skipped).
    assert!(reg.sample(0, 0, 1).is_none());
    // setRate with a different throttle_n: counter should NOT reset.
    let cfg2 = SampleCfg {
        master_enable: true,
        enable: 1u32 << 0,
        throttle_n: 99, // ← doesn't matter; counter keeps its value
    };
    reg.set_rate(cfg2);
    // The next observation should hit counter == 1 and emit (the
    // counter from before setRate is still 1).
    assert!(
        reg.sample(1, 0, 2).is_some(),
        "counter survived setRate; emission at counter==1"
    );
}

// ---------------------------------------------------------------------------
// 8. Attach conflict rejects second binding (M-18 v0.8 §18-6)
// ---------------------------------------------------------------------------

#[test]
fn attach_conflict_rejects_existing_channel() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::single_bit(0, "GPIO0", "OUT"))
        .unwrap();

    // Second define_channel on the same ch → conflict, existing preserved.
    let err = reg
        .define_channel(ChannelDef::single_bit(0, "GPIO1", "OUT"))
        .unwrap_err();
    assert_eq!(
        err,
        ProbeError::ChannelConflict {
            ch: 0,
            existing_source: "GPIO0".to_string()
        }
    );
    assert_eq!(reg.conflict_warns(), 1);
    assert_eq!(reg.channel_count(), 1, "existing binding preserved");

    // Conflict surfaces as a PlatformError in the expected family.
    let platform: PlatformError = err.into();
    assert_eq!(platform.family, ErrorFamily::Observability);
    assert_eq!(platform.scenario, 1);
}

// ---------------------------------------------------------------------------
// 9. Bus trace ring buffer drops oldest (M-18 v0.8 §18-1)
// ---------------------------------------------------------------------------

#[test]
fn a_fetch_access_is_marked_as_such_in_the_trace() {
    // 取指与数据访问在总线上**都是读** —— 唯一能把它们分开的，是
    // M-14 的 `BusAccess::is_fetch` 标志（v0.13 为观察点修的那个）。
    //
    // ⚠️ 这个标志必须**一路带到契约层**（契约 v3 的 `TraceEvent::is_fetch`）：
    // M-11 的执行轨迹靠它把取指映射成 kind=0（指令），否则
    // `TraceKind::Instruction` 会是**死代码**（D-50 记的「实现齐了 ≠
    // 接上了」那种形态）。
    let mut reg = ProbeRegistry::new();

    let mut fetch = make_bus_access(10, 0x1000, BusWidth::Word, AccessDir::Read, true);
    fetch.is_fetch = true;
    reg.record_bus_access(&fetch);

    let data = make_bus_access(20, 0x2000, BusWidth::Word, AccessDir::Read, true);
    reg.record_bus_access(&data);

    let drained: Vec<TraceEvent> = reg.drain_trace();
    assert_eq!(drained.len(), 2, "两条访问都该进环");
    assert!(drained[0].is_fetch, "取指没有带上 is_fetch 标志");
    assert!(!drained[1].is_fetch, "数据访问被误标成取指");
}

#[test]
fn bus_trace_ring_drops_oldest_on_overflow() {
    let mut reg = ProbeRegistry::new();
    // Push exactly TRACE_CAP + 5 events. The oldest 5 should be
    // dropped; the surviving TRACE_CAP events should be the
    // newest TRACE_CAP.
    let extra = 5;
    for i in 0..(TRACE_CAP + extra) as u64 {
        reg.record_bus_access(&make_bus_access(
            i,
            0x1000_0000 + (i as u32) * 4,
            BusWidth::Word,
            AccessDir::Write,
            true,
        ));
    }
    assert_eq!(reg.trace_len(), TRACE_CAP, "ring holds exactly capacity");
    assert_eq!(reg.trace_dropped() as usize, extra, "oldest 5 dropped");

    // Drain and confirm the oldest survivor is i=5 (i=0..4 dropped).
    let drained: Vec<TraceEvent> = reg.drain_trace();
    assert_eq!(drained.len(), TRACE_CAP);
    let first_addr = drained[0].addr;
    let expected_first = 0x1000_0000 + (extra as u32) * 4;
    assert_eq!(first_addr, expected_first, "oldest survivor is i={extra}");
    let last_addr = drained.last().unwrap().addr;
    let expected_last = 0x1000_0000 + ((TRACE_CAP + extra - 1) as u32) * 4;
    assert_eq!(last_addr, expected_last);
}

// ---------------------------------------------------------------------------
// 10. drain_waveform resets per-channel edge buffers
// ---------------------------------------------------------------------------

#[test]
fn drain_waveform_resets_edges() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "A", "a", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    reg.set_rate(cfg);

    reg.sample(0, 0, 1).unwrap();
    reg.sample(1, 0, 2).unwrap();
    assert_eq!(reg.edge_count(), 2);

    let wave = reg.drain_waveform();
    assert_eq!(wave.schema_version, 1);
    assert_eq!(wave.streams.len(), 1);
    let stream = &wave.streams[0];
    assert_eq!(stream.id, "A.a");
    assert_eq!(stream.edges.len(), 2);
    assert_eq!(reg.edge_count(), 0, "edges cleared after drain");

    // Drain again: empty, but stream list is still keyed by ch.
    let wave2 = reg.drain_waveform();
    assert_eq!(wave2.streams.len(), 1);
    assert!(wave2.streams[0].edges.is_empty());
}

// ---------------------------------------------------------------------------
// 11. Unbound signal dropped silently (M-18 v0.8 §6.1)
// ---------------------------------------------------------------------------

#[test]
fn unbound_signal_dropped_silently() {
    let mut reg = ProbeRegistry::new();
    // No channels defined.
    // record_signal on an unbound (periph, signal) is a no-op.
    reg.record_signal("NOPE", "OUT", 1, 0);
    assert_eq!(reg.edge_count(), 0);
    assert_eq!(reg.conflict_warns(), 0);
}

// ---------------------------------------------------------------------------
// 12. Bus hook routes into trace ring (M-18 v0.8 §3.1)
// ---------------------------------------------------------------------------

#[test]
fn make_bus_hook_routes_record_bus_access() {
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut hook = ProbeRegistry::make_bus_hook(Rc::clone(&reg));
    let access = make_bus_access(42, 0x2000_0000, BusWidth::Word, AccessDir::Read, true);
    hook(&access);
    hook(&access);
    let mut borrow = reg.borrow_mut();
    assert_eq!(borrow.trace_len(), 2);
    let drained = borrow.drain_trace();
    assert_eq!(drained[0].t_ps, 42);
    assert_eq!(drained[0].addr, 0x2000_0000);
    assert_eq!(drained[0].width, 4);
    assert_eq!(drained[0].dir.as_byte(), 0, "read = 0");
}

// ---------------------------------------------------------------------------
// 13. wire_signal_probe routes into channel (M-18 v0.8 §3.2)
// ---------------------------------------------------------------------------

#[test]
fn wire_signal_probe_routes_periph_signal_to_channel() {
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    {
        let mut borrow = reg.borrow_mut();
        borrow
            .define_channel(ChannelDef::single_bit(CH_GPIO0, "GPIO0", "OUT"))
            .unwrap();
        let mut cfg = default_cfg();
        cfg.enable_channel(CH_GPIO0);
        borrow.set_rate(cfg);
    }
    let mut sink = ProbeRegistry::wire_signal_probe(Rc::clone(&reg), "GPIO0", "OUT", CH_GPIO0);
    // Run the observations first; the closure borrows the registry
    // mutably on each call, so any external `borrow_mut()` must be
    // dropped before invoking `sink`.
    sink("OUT", 1, 0); // rising (first observation always emits)
    sink("OUT", 0, 1); // falling
    sink("OUT", 0, 2); // no event (same)
                       // Mismatched signal name is filtered at the callback boundary.
    sink("OTHER", 1, 3);
    {
        let borrow = reg.borrow();
        assert_eq!(borrow.edge_count(), 2);
        let ch0 = borrow.edges_for(CH_GPIO0);
        assert_eq!(ch0.len(), 2);
        assert_eq!(ch0[0].value, 1);
        assert_eq!(ch0[1].value, 0);
    }
}

#[test]
fn wire_signal_probe_keeps_channel_and_replaces_sink() {
    // Re-wiring the same channel replaces the previous sink without
    // conflict. Conflict semantics live in define_channel, not
    // wire_signal_probe (the design doc §3.2's "同 ch 冲突拒绝"
    // is enforced at the channel-definition step).
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    {
        let mut borrow = reg.borrow_mut();
        borrow
            .define_channel(ChannelDef::single_bit(0, "GPIO0", "OUT"))
            .unwrap();
    }
    let _first = ProbeRegistry::wire_signal_probe(Rc::clone(&reg), "GPIO0", "OUT", 0);
    // Second wire overwrites the lookup binding but does not error.
    let _second = ProbeRegistry::wire_signal_probe(Rc::clone(&reg), "GPIO0", "OUT", 0);
    assert_eq!(reg.borrow().conflict_warns(), 0);
}

// ---------------------------------------------------------------------------
// 14. edges_for() reflects a single channel's events
// ---------------------------------------------------------------------------

#[test]
fn edges_for_returns_per_channel_slice() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "A", "a", 32))
        .unwrap();
    reg.define_channel(ChannelDef::multi_bit(1, "B", "b", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    cfg.enable_channel(1);
    reg.set_rate(cfg);

    reg.sample(0, 0, 0xAA).unwrap();
    reg.sample(0, 1, 0xBB).unwrap();
    reg.sample(1, 0, 0xCC).unwrap();

    let a = reg.edges_for(0);
    let b = reg.edges_for(1);
    assert_eq!(a.len(), 2);
    assert_eq!(b.len(), 1);
    assert_eq!(
        a[1],
        Edge {
            t_ps: 1,
            value: 0xCC
        }
    );
    assert_eq!(
        b[0],
        Edge {
            t_ps: 0,
            value: 0xBB
        }
    );
}

// ---------------------------------------------------------------------------
// 15. Key-state channel (the 4th default channel) — multi-bit drop logic
// ---------------------------------------------------------------------------

#[test]
fn key_state_channel_drops_unchanged_repeats() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(CH_KEY_STATE, "KEY", "state", 1))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(CH_KEY_STATE);
    reg.set_rate(cfg);

    // Press, release, press, press-again — last "press" is a repeat
    // and must not emit (the canonical edge-only contract).
    let _ = reg.sample(0, CH_KEY_STATE, 1).unwrap();
    assert!(reg.sample(1, CH_KEY_STATE, 1).is_none());
    let _ = reg.sample(2, CH_KEY_STATE, 0).unwrap();
    let _ = reg.sample(3, CH_KEY_STATE, 1).unwrap();
    assert!(reg.sample(4, CH_KEY_STATE, 1).is_none());
    assert_eq!(reg.edge_count(), 3);
}

// ---------------------------------------------------------------------------
// 16. setRate replaces config wholesale (no half-state)
// ---------------------------------------------------------------------------

#[test]
fn set_rate_replaces_whole_config() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "A", "a", 32))
        .unwrap();
    let cfg1 = SampleCfg {
        master_enable: true,
        enable: 1u32 << 0,
        throttle_n: 1,
    };
    reg.set_rate(cfg1.clone());
    assert_eq!(reg.sample_cfg().throttle_n, 1);
    assert!(reg.sample_cfg().master_enable);

    let cfg2 = SampleCfg {
        master_enable: false, // ← key change
        enable: 0,
        throttle_n: 7,
    };
    reg.set_rate(cfg2.clone());
    assert_eq!(reg.sample_cfg().throttle_n, 7);
    assert!(!reg.sample_cfg().master_enable);
    // Throttle counters persist across setRate (per design doc).
    assert!(reg.sample(0, 0, 1).is_none(), "master off after setRate");
}

// ---------------------------------------------------------------------------
// 17. prev survives enable/disable (M-18 review #2 false-positive
//     regression lock, 2026-10-19)
// ---------------------------------------------------------------------------
//
// The review claimed `prev` is "completely lost" when a channel is
// disabled, causing the first observation after re-enable to emit
// unconditionally. Reading the code shows the opposite: `prev` is
// only updated inside `sample_inner` after the master/enable
// checks, but the BTreeMap entry persists across calls. This test
// locks the invariant down so a future refactor doesn't break it.

#[test]
fn prev_survives_enable_disable_cycle() {
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "P", "p", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    reg.set_rate(cfg.clone());

    // Establish prev=42.
    reg.sample(0, 0, 42).expect("first sample emits");

    // Disable ch=0 via the bitmask. master still on.
    cfg.enable &= !(1u32 << 0);
    reg.set_rate(cfg.clone());
    // With the channel disabled, sample is filtered at the
    // enable check, but the BTreeMap entries for `prev` survive.
    assert!(reg.sample(1, 0, 99).is_none(), "ch=0 disabled");

    // Re-enable and sample with the *same* value as the last
    // emitted one. Edge judgment must drop this as a same-value
    // repeat (NOT emit it as a "first observation" — that would
    // mean prev was lost).
    cfg.enable_channel(0);
    reg.set_rate(cfg);
    assert!(
        reg.sample(2, 0, 42).is_none(),
        "prev=42 preserved across disable; same value, no event"
    );
    // A real change does emit.
    let e = reg.sample(3, 0, 43).expect("real change emits");
    assert_eq!(e.value, 43);
}

#[test]
fn prev_survives_master_disable_cycle() {
    // Same invariant for master_enable (vs. per-channel enable).
    let mut reg = ProbeRegistry::new();
    reg.define_channel(ChannelDef::multi_bit(0, "M", "m", 32))
        .unwrap();
    let mut cfg = default_cfg();
    cfg.enable_channel(0);
    reg.set_rate(cfg.clone());

    reg.sample(0, 0, 7).expect("first sample emits");
    cfg.master_enable = false;
    reg.set_rate(cfg.clone());
    assert!(reg.sample(1, 0, 999).is_none(), "master off");
    cfg.master_enable = true;
    reg.set_rate(cfg);
    // Same value as the last emitted one → still filtered.
    assert!(
        reg.sample(2, 0, 7).is_none(),
        "prev=7 preserved across master off"
    );
    // New value emits.
    let e = reg.sample(3, 0, 8).expect("change emits");
    assert_eq!(e.value, 8);
}

// ---------------------------------------------------------------------------
// 18. ChannelDef::classify masks SingleBit (M-18 review #9)
// ---------------------------------------------------------------------------

#[test]
fn classify_masks_single_bit_inputs() {
    use crate::m18_probe::channel::{ChannelDef, ChannelKind};
    use riscv_sim_contract::model::EdgeKind;

    // A SingleBit channel with a "bad" caller sending multi-bit
    // values: classify must still return a well-defined kind.
    let def = ChannelDef::single_bit(0, "X", "x");
    assert_eq!(def.kind, ChannelKind::SingleBit);

    // 0 (LSB=0) → 5 (LSB=1) → Rising (LSB transitions, not the
    // raw value 5 which is non-zero but not "1").
    assert_eq!(def.classify(0, 5), EdgeKind::Rising);
    // 5 (LSB=1) → 2 (LSB=0) → Falling.
    assert_eq!(def.classify(5, 2), EdgeKind::Falling);
    // 5 (LSB=1) → 7 (LSB=1) → Value (same LSB; same-value branch).
    assert_eq!(def.classify(5, 7), EdgeKind::Value);
    // 0 → 0 → Value (same LSB; same-value branch).
    assert_eq!(def.classify(0, 0), EdgeKind::Value);
}

// ---------------------------------------------------------------------------
// 19. wire_signal_probe does NOT panic on contention
//     (M-18 review #3 — borrow_contended diagnostic)
// ---------------------------------------------------------------------------

#[test]
fn wire_signal_probe_does_not_panic_on_contention() {
    use std::cell::RefCell;
    use std::rc::Rc;

    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    reg.borrow_mut()
        .define_channel(ChannelDef::single_bit(0, "Y", "y"))
        .unwrap();
    {
        let mut borrow = reg.borrow_mut();
        let mut cfg = default_cfg();
        cfg.enable_channel(0);
        borrow.set_rate(cfg);
    }
    let mut sink = ProbeRegistry::wire_signal_probe(Rc::clone(&reg), "Y", "y", 0);

    // Hold a borrow on the registry, then trigger the sink. The
    // sink should NOT panic; it should drop the observation and
    // bump borrow_contended.
    let _guard = reg.borrow(); // immutable borrow held
    sink("y", 1, 0); // would have panicked in v0.8
    drop(_guard);

    assert_eq!(
        reg.borrow().borrow_contended(),
        1,
        "contention counter bumped"
    );
    // The dropped observation did NOT produce an edge event.
    assert_eq!(reg.borrow().edge_count(), 0);
}

#[test]
fn wire_signal_probe_normal_path_increments_zero_contention() {
    // Counter is 0 by default and stays 0 when the closure runs
    // without contention.
    use std::cell::RefCell;
    use std::rc::Rc;

    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    reg.borrow_mut()
        .define_channel(ChannelDef::single_bit(0, "Z", "z"))
        .unwrap();
    reg.borrow_mut().set_rate({
        let mut cfg = default_cfg();
        cfg.enable_channel(0);
        cfg
    });
    let mut sink = ProbeRegistry::wire_signal_probe(Rc::clone(&reg), "Z", "z", 0);
    sink("z", 1, 0);
    assert_eq!(reg.borrow().borrow_contended(), 0);
    assert_eq!(reg.borrow().edge_count(), 1);
}

// ---------------------------------------------------------------------------
// 14. attach_signal binds a peripheral to a channel (D-12 / LLD §1.3)
// ---------------------------------------------------------------------------

/// A peripheral that can fire a probe observation on demand.
#[derive(Default)]
struct MockPeripheral {
    id: String,
    sink: Option<riscv_sim_contract::port::ProbeSink>,
    fired: Rc<RefCell<Vec<(String, u64, u64)>>>,
}

impl std::fmt::Debug for MockPeripheral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockPeripheral")
            .field("id", &self.id)
            .field("probe_attached", &self.sink.is_some())
            .finish()
    }
}

impl MockPeripheral {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            sink: None,
            fired: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Fire one observation through the installed probe callback.
    fn fire(&mut self, signal: &str, value: u64, now_ps: u64) {
        self.fired
            .borrow_mut()
            .push((signal.to_string(), value, now_ps));
        if let Some(sink) = self.sink.as_mut() {
            sink(signal, value, now_ps);
        }
    }
}

impl riscv_sim_contract::port::Peripheral for MockPeripheral {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        0
    }
    fn size(&self) -> u32 {
        0x10
    }
    fn read(
        &self,
        _offset: u32,
        _size: riscv_sim_contract::model::WriteSize,
    ) -> Result<u32, PlatformError> {
        Ok(0)
    }
    fn write(
        &mut self,
        _offset: u32,
        _size: riscv_sim_contract::model::WriteSize,
        _value: u32,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
    fn tick(
        &mut self,
        _now_ps: u64,
    ) -> Result<Vec<riscv_sim_contract::model::PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
    fn on_probe(&mut self, cb: Option<riscv_sim_contract::port::ProbeSink>) {
        self.sink = cb;
    }
}

#[test]
fn attach_signal_binds_the_peripheral_and_defines_the_channel() {
    // M-18 LLD §1.3 `attachSignal(periph, ch)`: one call binds the
    // channel and installs the callback into the peripheral.
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut periph = MockPeripheral::new("GPIO0");
    {
        let mut cfg = default_cfg();
        cfg.enable_channel(CH_GPIO0);
        reg.borrow_mut().set_rate(cfg);
    }

    ProbeRegistry::attach_signal(
        Rc::clone(&reg),
        &mut periph,
        "OUT",
        CH_GPIO0,
        crate::m18_probe::channel::ChannelKind::SingleBit,
        1,
    )
    .expect("attach signal");

    assert_eq!(reg.borrow().channel_count(), 1, "the channel is defined");

    // The peripheral's reports now reach the channel.
    periph.fire("OUT", 1, 10);
    periph.fire("OUT", 0, 20);
    {
        let borrow = reg.borrow();
        assert_eq!(borrow.edge_count(), 2);
        let edges = borrow.edges_for(CH_GPIO0);
        assert_eq!(edges[0].value, 1);
        assert_eq!(edges[1].value, 0);
    }
}

#[test]
fn attach_signal_is_idempotent_for_the_same_binding() {
    // Re-attaching the same (peripheral id, signal) to the same
    // channel is a no-op, not a conflict.
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut periph = MockPeripheral::new("GPIO0");
    {
        let mut cfg = default_cfg();
        cfg.enable_channel(CH_GPIO0);
        reg.borrow_mut().set_rate(cfg);
    }
    for _ in 0..2 {
        ProbeRegistry::attach_signal(
            Rc::clone(&reg),
            &mut periph,
            "OUT",
            CH_GPIO0,
            crate::m18_probe::channel::ChannelKind::SingleBit,
            1,
        )
        .expect("attach signal");
    }
    assert_eq!(reg.borrow().channel_count(), 1);
    assert_eq!(reg.borrow().conflict_warns(), 0);
}

#[test]
fn attach_signal_rejects_a_conflicting_channel_binding() {
    // The same channel number bound to a *different* (source, signal)
    // is a conflict (M-18 v0.8 §18-6).
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut periph = MockPeripheral::new("GPIO0");
    reg.borrow_mut()
        .define_channel(ChannelDef::single_bit(CH_GPIO0, "OTHER", "X"))
        .unwrap();

    let err = ProbeRegistry::attach_signal(
        Rc::clone(&reg),
        &mut periph,
        "OUT",
        CH_GPIO0,
        crate::m18_probe::channel::ChannelKind::SingleBit,
        1,
    )
    .expect_err("conflicting binding must be rejected");
    assert_eq!(err.scenario(), 1, "OBS-01");
}

// ---------------------------------------------------------------------------
// 20. make_bus_hook does NOT panic on contention
//     (F-3, 2026-10-20 — 总线 hook 的 RefCell 重入 panic)
// ---------------------------------------------------------------------------

#[test]
fn bus_hook_does_not_panic_on_immutable_borrow_contention() {
    // F-3：调用方在 `bus.read()` / `bus.write()` 期间持有
    // `probes.borrow()`（不可变借用）时触发总线 hook。
    // 旧实现用裸 `borrow_mut()` → RefCell 重入 panic，整机仿真崩掉。
    // 新实现必须：丢弃该次访存记录 + borrow_contended 计数 +1，绝不 panic。
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut hook = ProbeRegistry::make_bus_hook(Rc::clone(&reg));
    let access = make_bus_access(42, 0x2000_0000, BusWidth::Word, AccessDir::Read, true);

    {
        let _guard = reg.borrow(); // 不可变借用在整个 hook 调用期间保持
        hook(&access); // 旧实现在此 panic
    } // 释放借用

    assert_eq!(
        reg.borrow().borrow_contended(),
        1,
        "the contended bus access must be counted, not panic"
    );
    assert_eq!(
        reg.borrow().trace_len(),
        0,
        "the contended access record is dropped (trace must not grow)"
    );
}

#[test]
fn bus_hook_does_not_panic_on_mutable_borrow_contention() {
    // F-3 的另一半：调用方持有 `borrow_mut()`（可变借用）时触发 hook。
    // 同样必须不 panic、计数递增、trace 不增长。
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut hook = ProbeRegistry::make_bus_hook(Rc::clone(&reg));
    let access = make_bus_access(7, 0x4000_0000, BusWidth::Byte, AccessDir::Write, true);

    {
        let mut _guard = reg.borrow_mut(); // 可变借用在整个 hook 调用期间保持
        hook(&access); // 旧实现在此 panic
    } // 释放借用

    assert_eq!(
        reg.borrow().borrow_contended(),
        1,
        "the contended bus access must be counted, not panic"
    );
    assert_eq!(
        reg.borrow().trace_len(),
        0,
        "the contended access record is dropped (trace must not grow)"
    );
}

#[test]
fn bus_hook_normal_path_keeps_the_contention_counter_at_zero() {
    // 正常路径回归保护：无争用时计数保持 0，访存记录照常进 trace。
    let reg = Rc::new(RefCell::new(ProbeRegistry::new()));
    let mut hook = ProbeRegistry::make_bus_hook(Rc::clone(&reg));
    let access = make_bus_access(99, 0x2000_0000, BusWidth::Word, AccessDir::Read, true);

    hook(&access);
    hook(&access);

    assert_eq!(
        reg.borrow().borrow_contended(),
        0,
        "an uncontended bus hook must not report contention"
    );
    assert_eq!(
        reg.borrow().trace_len(),
        2,
        "an uncontended bus hook must still record the accesses"
    );
}
