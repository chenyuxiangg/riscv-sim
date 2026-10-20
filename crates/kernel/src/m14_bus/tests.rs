//! M-14 unit + integration tests (LLD §7).
//!
//! Coverage: map / unmap validation (E_BUS-03 / 04 / 06), lookup,
//! read / write happy paths with 8/16/32 widths, ROM-write ignore,
//! cross-region read & write split, full-unmapped reads (E_BUS-01),
//! hook capture, stats accounting.

use super::*;
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::Peripheral;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Mock peripheral — exercises the bus → instance dispatch (D-12 / D-14).
// ---------------------------------------------------------------------------

/// A minimal peripheral that records **which** read path the bus used.
///
/// The counters are `Cell`s because the contract's `Peripheral::read`
/// takes `&self` while `read_clear` / `write` take `&mut self`.
#[derive(Debug, Default)]
struct MockPeripheral {
    id: String,
    base: u32,
    size: u32,
    reads: Cell<u32>,
    read_clears: Cell<u32>,
    writes: Cell<u32>,
    last_write: Cell<Option<(u32, WriteSize, u32)>>,
    fail: bool,
}

impl MockPeripheral {
    fn new(id: &str, base: u32, size: u32) -> Self {
        Self {
            id: id.to_string(),
            base,
            size,
            ..Default::default()
        }
    }
}

impl Peripheral for MockPeripheral {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        self.size
    }
    fn read(&self, _offset: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        self.reads.set(self.reads.get() + 1);
        Ok(0xA5A5_0000)
    }
    fn read_clear(&mut self, offset: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        self.read_clears.set(self.read_clears.get() + 1);
        if self.fail {
            return Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                "mock peripheral failure",
            ));
        }
        Ok(0x5A5A_0000 | offset)
    }
    fn write(&mut self, offset: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        self.writes.set(self.writes.get() + 1);
        self.last_write.set(Some((offset, size, value)));
        if self.fail {
            return Err(PlatformError::new(
                ErrorFamily::Peripheral,
                2,
                "mock peripheral failure",
            ));
        }
        Ok(())
    }
    fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
}

// ---------------------------------------------------------------------------
// map / unmap / lookup
// ---------------------------------------------------------------------------

#[test]
fn map_ram_succeeds_and_lookup_returns_region() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x100, &[0xDE, 0xAD, 0xBE, 0xEF])
        .unwrap();
    let region = bus.lookup(0x1000).expect("region must exist");
    assert_eq!(region.base, 0x1000);
    assert_eq!(region.size, 0x100);
    assert_eq!(region.kind, RegionType::Ram);
    assert_eq!(bus.region_count(), 1);
}

#[test]
fn map_rejects_zero_size_with_e_bus_06() {
    let mut bus = Bus::new();
    let err = bus.map_ram(0x1000, 0, &[]).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 6, "size=0 must yield E_BUS-06");
}

#[test]
fn map_rejects_overlap_with_e_bus_03_and_keeps_old() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x100, &[]).unwrap();
    let err = bus.map_ram(0x1080, 0x100, &[]).unwrap_err();
    assert_eq!(err.scenario, 3, "overlap must yield E_BUS-03");
    // The first region is still in place (评审 14-3: 失败保留旧配置).
    assert_eq!(bus.region_count(), 1);
    assert!(bus.lookup(0x1050).is_some());
}

#[test]
fn map_rejects_out_of_bus_with_e_bus_04() {
    let mut bus = Bus::new();
    let err = bus.map_ram(0xFFFF_FFF0, 0x20, &[]).unwrap_err();
    assert_eq!(err.scenario, 4, "out-of-bus must yield E_BUS-04");
}

#[test]
fn unmap_is_idempotent() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x100, &[]).unwrap();
    assert!(bus.unmap(0x1000), "first unmap removes region");
    assert!(!bus.unmap(0x1000), "second unmap is a no-op");
    assert_eq!(bus.region_count(), 0);
}

#[test]
fn lookup_returns_none_for_unmapped_address() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x100, &[]).unwrap();
    assert!(bus.lookup(0x0500).is_none(), "below first region");
    assert!(bus.lookup(0x2000).is_none(), "above first region");
    // Inside the region.
    assert!(bus.lookup(0x1050).is_some());
}

// ---------------------------------------------------------------------------
// read / write happy path — width 8 / 16 / 32, little-endian
// ---------------------------------------------------------------------------

#[test]
fn ram_read_write_byte() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x100, &[0xAA, 0xBB, 0xCC, 0xDD])
        .unwrap();
    assert_eq!(bus.read(0x1000, BusWidth::Byte).unwrap(), 0xAA);
    assert_eq!(bus.read(0x1003, BusWidth::Byte).unwrap(), 0xDD);
    bus.write(0x1002, BusWidth::Byte, 0xFF).unwrap();
    assert_eq!(bus.read(0x1002, BusWidth::Byte).unwrap(), 0xFF);
}

#[test]
fn ram_read_write_half_little_endian() {
    let mut bus = Bus::new();
    bus.map_ram(0x2000, 0x100, &[0x34, 0x12, 0x78, 0x56])
        .unwrap();
    assert_eq!(bus.read(0x2000, BusWidth::Half).unwrap(), 0x1234);
    assert_eq!(bus.read(0x2002, BusWidth::Half).unwrap(), 0x5678);
    bus.write(0x2000, BusWidth::Half, 0xBEEF).unwrap();
    assert_eq!(bus.read(0x2000, BusWidth::Half).unwrap(), 0xBEEF);
}

#[test]
fn ram_read_write_word_little_endian() {
    let mut bus = Bus::new();
    bus.map_ram(0x3000, 0x100, &[0xEF, 0xBE, 0xAD, 0xDE])
        .unwrap();
    assert_eq!(bus.read(0x3000, BusWidth::Word).unwrap(), 0xDEAD_BEEF);
    bus.write(0x3000, BusWidth::Word, 0x1234_5678).unwrap();
    assert_eq!(bus.read(0x3000, BusWidth::Word).unwrap(), 0x1234_5678);
}

// ---------------------------------------------------------------------------
// ROM-write ignore + access-time error scenarios.
// ---------------------------------------------------------------------------

#[test]
fn rom_write_is_silently_dropped() {
    let mut bus = Bus::new();
    bus.map_rom(0x4000, 0x100, &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    assert_eq!(bus.read(0x4000, BusWidth::Word).unwrap(), 0x4433_2211);
    bus.write(0x4000, BusWidth::Word, 0xFFFF_FFFF).unwrap();
    // ROM contents unchanged.
    assert_eq!(bus.read(0x4000, BusWidth::Word).unwrap(), 0x4433_2211);
}

#[test]
fn unmapped_read_yields_e_bus_01() {
    let mut bus = Bus::new();
    let err = bus.read(0x9999, BusWidth::Word).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 1);
}

#[test]
fn unmapped_write_yields_e_bus_01() {
    let mut bus = Bus::new();
    let err = bus.write(0x9999, BusWidth::Word, 0xABCD).unwrap_err();
    assert_eq!(err.scenario, 1);
}

#[test]
fn read_past_end_of_region_yields_e_bus_01() {
    // Review #14: the previous assertion accepted either E_BUS-01 or
    // E_BUS-04. The word read at `0x5010` (region ends at 0x500F) crosses
    // a wholly-unmapped byte slice, so it MUST surface E_BUS-01 (no
    // mapping at all); E_BUS-04 is reserved for size-overflow /
    // out-of-bus base+size arithmetic.
    let mut bus = Bus::new();
    bus.map_ram(0x5000, 0x10, &[]).unwrap();
    let err = bus.read(0x5010, BusWidth::Word).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(
        err.scenario, 1,
        "expected E_BUS-01, got scenario {}",
        err.scenario
    );
}

#[test]
fn byte_read_past_end_of_region_yields_e_bus_01() {
    // Review #14 / N6: byte-sized reads past the end of a mapped
    // region go through `read_aligned`'s `lookup(addr)` first; the
    // lookup miss surfaces E_BUS-01, NOT E_BUS-04. The previous
    // `|| scenario == 4` clause was a hedge against implementation
    // uncertainty — tighten to scenario == 1 (the byte path never
    // reaches `read_bytes`).
    let mut bus = Bus::new();
    bus.map_ram(0x5000, 0x10, &[]).unwrap();
    let err = bus.read(0x5010, BusWidth::Byte).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 1, "byte past end must be E_BUS-01");
}

#[test]
fn peripheral_read_returns_e_bus_05_until_m15_wired() {
    // Review #13: lock the "M-15 not wired" contract via a positive
    // test. M-15 lands, this test is updated to expect a successful
    // dispatch and verify `periph_hits` increments by 1.
    let mut bus = Bus::new();
    bus.map_peripheral(0x2000_0000, 0x100, PeripheralId(1))
        .expect("map peripheral");
    let err = bus.read(0x2000_0010, BusWidth::Word).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 5, "peripheral dispatch not wired → E_BUS-05");
    // Read did NOT land: periph_hits stays 0.
    assert_eq!(bus.stats().periph_hits, 0);
}

#[test]
fn peripheral_write_returns_e_bus_05_until_m15_wired() {
    let mut bus = Bus::new();
    bus.map_peripheral(0x2000_0000, 0x100, PeripheralId(1))
        .expect("map peripheral");
    let err = bus.write(0x2000_0010, BusWidth::Word, 0xABCD).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 5);
    assert_eq!(bus.stats().periph_hits, 0);
}

// ---------------------------------------------------------------------------
// Cross-region read / write — split semantics.
// ---------------------------------------------------------------------------

#[test]
fn cross_region_half_read_splits_byte_by_byte() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x4, &[0x11, 0x22, 0x33, 0x44]).unwrap();
    bus.map_ram(0x1004, 0x4, &[0x55, 0x66, 0x77, 0x88]).unwrap();
    // Half-word straddling boundary: low byte @ 0x1003 = 0x44,
    // high byte @ 0x1004 = 0x55 → little-endian word = 0x5544.
    assert_eq!(bus.read(0x1003, BusWidth::Half).unwrap(), 0x5544);
}

#[test]
fn cross_region_word_write_uses_two_phase_semantics() {
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x4, &[0; 4]).unwrap();
    bus.map_ram(0x1004, 0x4, &[0; 4]).unwrap();
    // Word straddling boundary: write 0xAABBCCDD.
    bus.write(0x1002, BusWidth::Word, 0xAABB_CCDD).unwrap();
    assert_eq!(bus.read(0x1002, BusWidth::Byte).unwrap(), 0xDD);
    assert_eq!(bus.read(0x1003, BusWidth::Byte).unwrap(), 0xCC);
    assert_eq!(bus.read(0x1004, BusWidth::Byte).unwrap(), 0xBB);
    assert_eq!(bus.read(0x1005, BusWidth::Byte).unwrap(), 0xAA);
}

#[test]
fn cross_region_write_into_unmapped_aborts_atomically() {
    let mut bus = Bus::new();
    bus.map_ram(0x2000, 0x4, &[0xAA, 0xAA, 0xAA, 0xAA]).unwrap();
    // No region at 0x2004 → word straddles. Phase 1 must abort.
    let err = bus.write(0x2002, BusWidth::Word, 0xFFFF_FFFF).unwrap_err();
    assert_eq!(err.scenario, 1, "unmapped slice must yield E_BUS-01");
    // Whole original region untouched.
    assert_eq!(bus.read(0x2000, BusWidth::Word).unwrap(), 0xAAAA_AAAA);
}

#[test]
fn cross_region_write_silently_skips_rom_slice() {
    let mut bus = Bus::new();
    bus.map_ram(0x3000, 0x4, &[0x11, 0x22, 0x33, 0x44]).unwrap();
    bus.map_rom(0x3004, 0x4, &[0xAA, 0xBB, 0xCC, 0xDD]).unwrap();
    // Word straddles RAM[0x3000..4] + ROM[0x3004..8]. ROM slice is
    // silently dropped.
    bus.write(0x3002, BusWidth::Word, 0x9988_7766).unwrap();
    // RAM tail updated.
    assert_eq!(bus.read(0x3002, BusWidth::Half).unwrap(), 0x7766);
    // ROM unchanged.
    assert_eq!(bus.read(0x3004, BusWidth::Word).unwrap(), 0xDDCC_BBAA);
}

// ---------------------------------------------------------------------------
// Hook capture + stats.
// ---------------------------------------------------------------------------

#[test]
fn hook_receives_every_access_with_current_ts() {
    // The hook closure must capture 'static state because `BusHook` is
    // `Box<dyn FnMut>`. Use `Rc<RefCell<_>>` so the outer test can also
    // observe the captured events.
    let sink: Rc<RefCell<Vec<BusAccess>>> = Rc::new(RefCell::new(Vec::new()));
    let sink_for_hook = Rc::clone(&sink);
    let mut bus = Bus::new();
    bus.map_ram(0x4000, 0x10, &[1, 2, 3, 4]).unwrap();
    bus.set_hook(Some(Box::new(move |a: &BusAccess| {
        sink_for_hook.borrow_mut().push(*a);
    })));
    bus.set_now_ps(42);
    bus.read(0x4000, BusWidth::Word).expect("read ok");
    bus.write(0x4000, BusWidth::Word, 0xABCD).expect("write ok");
    let events = sink.borrow();
    assert_eq!(events.len(), 2, "one event per dispatch, not per byte");
    assert_eq!(events[0].addr, 0x4000);
    assert_eq!(events[0].ts_ps, 42);
    assert_eq!(events[0].dir, AccessDir::Read);
    assert_eq!(events[1].dir, AccessDir::Write);
    assert_eq!(events[1].addr, 0x4000);
}

#[test]
fn stats_split_between_ram_rom_periph_hits() {
    // Review #3: counter semantics after splitting `ram_hits` from the
    // ROM-aware field, and reviving the dormant `periph_hits`.
    let mut bus = Bus::new();
    // RAM — reads & writes both bump `ram_hits`.
    bus.map_ram(0x1000, 0x10, &[1, 2, 3, 4]).unwrap();
    bus.read(0x1000, BusWidth::Word).expect("read ram");
    bus.write(0x1000, BusWidth::Word, 0xABCD)
        .expect("write ram");
    // ROM — reads bump `rom_hits`, writes are silently dropped.
    bus.map_rom(0x2000, 0x10, &[0xAA, 0xBB, 0xCC, 0xDD])
        .unwrap();
    bus.read(0x2000, BusWidth::Word).expect("read rom");
    bus.write(0x2000, BusWidth::Word, 0xFFFF_FFFF)
        .expect("write rom (dropped)");

    let stats = bus.stats();
    assert_eq!(stats.reads, 2);
    assert_eq!(stats.writes, 2);
    assert_eq!(stats.ram_hits, 2, "read + write on RAM");
    assert_eq!(
        stats.rom_hits, 1,
        "read on ROM only; writes silently dropped"
    );
    assert_eq!(stats.periph_hits, 0, "M-15 not wired yet");
}

#[test]
fn map_rejects_init_too_large_with_e_bus_07() {
    // Review #7: distinguish "init payload > region size" (malformed
    // payload) from "base+size overflows 4 GiB" (topology error).
    let mut bus = Bus::new();
    let err = bus.map_ram(0x6000, 0x10, &[0u8; 0x20]).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 7, "init oversized → E_BUS-07");
    assert_eq!(
        bus.region_count(),
        0,
        "map failure preserves the prior set (评审 14-3)"
    );
}

#[test]
fn stats_accumulate_reads_writes_and_misses() {
    let mut bus = Bus::new();
    bus.map_ram(0x5000, 0x10, &[1, 2, 3, 4]).unwrap();
    bus.read(0x5000, BusWidth::Word).expect("read ok");
    bus.write(0x5000, BusWidth::Word, 0xABCD).expect("write ok");
    let _ = bus.read(0x9000, BusWidth::Word); // miss — error ignored
    let stats = bus.stats();
    assert_eq!(stats.reads, 2);
    assert_eq!(stats.writes, 1);
    assert_eq!(stats.ram_hits, 2, "read + write on RAM both bump ram_hits");
    assert_eq!(stats.rom_hits, 0);
    assert_eq!(stats.periph_hits, 0);
    assert_eq!(stats.misses, 1);
    assert_eq!(
        stats.unmapped_splits, 1,
        "unmapped word read counted in unmapped_splits (review #5)"
    );
    assert_eq!(stats.unaligned_splits, 0);
}

#[test]
fn hook_carries_ok_flag_for_failed_accesses() {
    // Review #2: hook fires for failed accesses too, with `ok=false`
    // and `err_scenario` populated.
    let sink: Rc<RefCell<Vec<BusAccess>>> = Rc::new(RefCell::new(Vec::new()));
    let sink_for_hook = Rc::clone(&sink);
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x10, &[0xAA; 16]).unwrap();
    bus.set_hook(Some(Box::new(move |a: &BusAccess| {
        sink_for_hook.borrow_mut().push(*a);
    })));
    bus.read(0x9000, BusWidth::Word).expect_err("must miss");
    let events = sink.borrow();
    assert_eq!(events.len(), 1);
    assert!(!events[0].ok);
    assert_eq!(events[0].err_scenario, Some(1));
}

#[test]
fn hook_succeed_entry_has_ok_true() {
    // Review #2 — counterpart to `hook_carries_ok_flag_for_failed_accesses`:
    // the happy path still reports `ok = true` and `err_scenario = None`.
    let sink: Rc<RefCell<Vec<BusAccess>>> = Rc::new(RefCell::new(Vec::new()));
    let sink_for_hook = Rc::clone(&sink);
    let mut bus = Bus::new();
    bus.map_ram(0x1000, 0x10, &[0xAA; 16]).unwrap();
    bus.set_hook(Some(Box::new(move |a: &BusAccess| {
        sink_for_hook.borrow_mut().push(*a);
    })));
    bus.read(0x1004, BusWidth::Half).expect("read ok");
    let events = sink.borrow();
    assert_eq!(events.len(), 1);
    assert!(events[0].ok);
    assert_eq!(events[0].err_scenario, None);
}

#[test]
fn busmaster_trait_dispatches_to_bus() {
    let mut bus = Bus::new();
    bus.map_ram(0x6000, 0x10, &[0xAA, 0xBB, 0xCC, 0xDD])
        .unwrap();
    let master: &mut dyn BusMaster = &mut bus;
    let val = master.read(0x6000, WriteSize::Word).unwrap();
    assert_eq!(val, 0xDDCC_BBAA);
    master.write(0x6000, WriteSize::Word, 0x1234_5678).unwrap();
    let val = master.read(0x6002, WriteSize::Half).unwrap();
    assert_eq!(val, 0x1234);
}

// ---------------------------------------------------------------------------
// Peripheral instance dispatch (D-12 / D-14).
// ---------------------------------------------------------------------------

#[test]
fn peripheral_instance_dispatches_reads_and_writes() {
    // D-12: a mapped instance really receives the guest accesses.
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    let mut bus = Bus::new();
    bus.map_peripheral_instance(0x3000, 0x10, Rc::clone(&mock) as _)
        .expect("map peripheral instance");

    let val = bus.read(0x3004, BusWidth::Word).expect("read periph");
    assert_eq!(val, 0x5A5A_0004, "the read came from the instance");
    bus.write(0x3008, BusWidth::Word, 0xDEAD_BEEF)
        .expect("write periph");

    let m = mock.borrow();
    assert_eq!(m.writes.get(), 1);
    assert_eq!(
        m.last_write.get(),
        Some((0x8, WriteSize::Word, 0xDEAD_BEEF)),
        "the offset is region-relative"
    );
    assert_eq!(bus.stats().periph_hits, 2, "one read + one write");
}

#[test]
fn peripheral_read_uses_read_clear_not_read() {
    // D-14: the bus must call `read_clear`, never `read`. A peripheral
    // whose read is destructive (UART RX FIFO) would silently keep
    // handing out the same byte otherwise — with no compile-time hint.
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    let mut bus = Bus::new();
    bus.map_peripheral_instance(0x3000, 0x10, Rc::clone(&mock) as _)
        .expect("map peripheral instance");

    bus.read(0x3000, BusWidth::Word).expect("read periph");
    bus.read(0x3000, BusWidth::Word).expect("read periph again");

    let m = mock.borrow();
    assert_eq!(
        m.read_clears.get(),
        2,
        "both reads took the read-clear path"
    );
    assert_eq!(m.reads.get(), 0, "the plain read path must not be used");
}

#[test]
fn peripheral_error_is_surfaced_as_e_bus_05() {
    // M-14 LLD §6.1: an outer peripheral error becomes E_BUS-05 — it
    // must stay distinguishable from the unmapped E_BUS-01.
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    mock.borrow_mut().fail = true;
    let mut bus = Bus::new();
    bus.map_peripheral_instance(0x3000, 0x10, Rc::clone(&mock) as _)
        .expect("map peripheral instance");

    let err = bus.read(0x3000, BusWidth::Word).expect_err("must fail");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 5, "peripheral error → E_BUS-05");
    assert!(
        err.message.contains("mock peripheral failure"),
        "the original reason must survive: {}",
        err.message
    );

    let err = bus.write(0x3000, BusWidth::Word, 1).expect_err("must fail");
    assert_eq!(err.scenario, 5);
}

#[test]
fn peripheral_size_mismatch_is_rejected_with_e_bus_08() {
    // D-12: the bus window must match `Peripheral::size()`.
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    let mut bus = Bus::new();
    let err = bus
        .map_peripheral_instance(0x3000, 0x20, Rc::clone(&mock) as _)
        .expect_err("size mismatch must be rejected");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 8, "size mismatch → E_BUS-08");
    assert_eq!(bus.region_count(), 0, "a rejected mount leaves no region");
}

#[test]
fn busy_peripheral_instance_yields_e_bus_05_instead_of_a_panic() {
    // The instance is already mutably borrowed by the caller → the
    // dispatch must not panic (a panic in the bus path would take the
    // whole simulation down).
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    let mut bus = Bus::new();
    bus.map_peripheral_instance(0x3000, 0x10, Rc::clone(&mock) as _)
        .expect("map peripheral instance");

    let _held = mock.borrow_mut();
    let err = bus.read(0x3000, BusWidth::Word).expect_err("must fail");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 5, "a busy instance → E_BUS-05");
}

#[test]
fn id_only_peripheral_mapping_still_reports_not_wired() {
    // The legacy id-only mapping has no instance behind it — it must
    // keep reporting "not wired" (E_BUS-05) rather than pretending to
    // work.
    let mut bus = Bus::new();
    bus.map_peripheral(0x2000_0000, 0x100, PeripheralId(1))
        .expect("map peripheral");
    let err = bus.read(0x2000_0010, BusWidth::Word).unwrap_err();
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 5);
}

// ---------------------------------------------------------------------------
// D-10 — per-region size cap (M-14 side).
// ---------------------------------------------------------------------------

#[test]
fn oversized_ram_region_is_rejected_before_allocating() {
    // D-10: a malformed firmware image can declare a ~4 GiB segment.
    // The cap must reject it *before* the backing store is allocated.
    let mut bus = Bus::new();
    let err = bus
        .map_ram(0, 0x4000_0000, &[])
        .expect_err("a 1 GiB region must be rejected");
    assert_eq!(err.family, ErrorFamily::Bus);
    assert_eq!(err.scenario, 9, "over-cap region → E_BUS-09");
    assert_eq!(bus.region_count(), 0);
}

#[test]
fn oversized_rom_region_is_rejected_before_allocating() {
    // The ROM path allocates too — it must be capped as well.
    let mut bus = Bus::new();
    let err = bus
        .map_rom(0, 0x4000_0000, &[])
        .expect_err("a 1 GiB region must be rejected");
    assert_eq!(err.scenario, 9);
}

#[test]
fn the_region_size_cap_is_configurable() {
    let mut bus = Bus::with_max_region_bytes(0x1000);
    assert_eq!(bus.max_region_bytes(), 0x1000);
    bus.map_ram(0, 0x1000, &[])
        .expect("a region at the cap is fine");
    let err = bus
        .map_ram(0x2000, 0x1001, &[])
        .expect_err("a region over the cap must be rejected");
    assert_eq!(err.scenario, 9);
    // The cap can be raised at runtime (the host owns the budget).
    bus.set_max_region_bytes(0x2000);
    bus.map_ram(0x2000, 0x1001, &[])
        .expect("raising the cap works");
}

#[test]
fn the_default_cap_leaves_normal_regions_untouched() {
    // The default cap must not get in the way of a realistic image.
    let mut bus = Bus::new();
    assert_eq!(bus.max_region_bytes(), DEFAULT_MAX_REGION_BYTES);
    bus.map_ram(0x8000_0000, 0x1000, &[0xAA; 4])
        .expect("normal region");
    bus.map_ram(0x8001_0000, 0x10_0000, &[])
        .expect("16 MiB region");
    assert_eq!(bus.region_count(), 2);
}

#[test]
fn the_cap_also_guards_the_peripheral_window() {
    // A peripheral window claims an address interval just like a RAM
    // region does — the cap applies to it too.
    let mock = Rc::new(RefCell::new(MockPeripheral::new("MOCK0", 0x3000, 0x10)));
    let mut bus = Bus::with_max_region_bytes(0x8);
    let err = bus
        .map_peripheral_instance(0x3000, 0x10, Rc::clone(&mock) as _)
        .expect_err("over-cap peripheral window must be rejected");
    assert_eq!(err.scenario, 9);
}
