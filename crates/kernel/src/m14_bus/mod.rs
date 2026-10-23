//! M-14 总线与内存映射 (Bus & Memory Map).
//!
//! Owns the address decoder (regionTree → region), the RAM backend,
//! and the read / write dispatcher that splits cross-region writes into
//! a two-phase pre-check + commit (评审 14-1).
//!
//! ⚠ **RAM 是页表，不是「一整块 byte vector」**（STAGE-11 裁决 A）：
//! **登记**与**物理页**是两层——登记一个 region 是 O(1) 且**零物理代价**，
//! 页在**写时**才物化。⇒ 读**没写过**的页回 **0**；读**没登记**的地址
//! 仍是 E_BUS-01（M-13 把它变成架构陷阱 cause 5 / 7）——两层合成一层会
//! 当场破掉 D-99 的判别力。
//!
//! Port contract (LLD §9.1.1):
//! - driving: ISS / peripheral / debug services (via [`BusMaster`]).
//! - driven: typed handles per region kind (RAM → [`RamBackend`],
//!   PERIPHERAL → [`PeripheralId`] (id-only, legacy) or
//!   [`BoundPeripheral`] (live instance, D-12); ROM → [`RomBackend`]
//!   is a thin wrapper over RAM with `writable = false`).
//!
//! Error codes (LLD §3.4 / M-14 §6.1):
//!   E_BUS-01 unmapped            → PlatformError{Bus, 1}
//!   E_BUS-02 invalid width       → PlatformError{Bus, 2}
//!   E_BUS-03 overlap             → PlatformError{Bus, 3}
//!   E_BUS-04 overflow (base+size)→ PlatformError{Bus, 4}
//!   E_BUS-05 peripheral error    → PlatformError{Bus, 5}
//!   E_BUS-06 size = 0            → PlatformError{Bus, 6}
//!   E_BUS-07 init > region size  → PlatformError{Bus, 7}
//!   E_BUS-08 periph size mismatch→ PlatformError{Bus, 8}  (D-12)
//!   E_BUS-09 region over the cap → PlatformError{Bus, 9}  (D-10；v0.26 起
//!                                  量的是「物理占用」（RAM）或「声明大小」
//!                                  （ROM / 外设窗口）——码号不变)
//!   E_BUS-10 unknown region kind   → PlatformError{Bus, 10} (STAGE-3 §3.2)
//!   E_BUS-11 page outside every RAM region → PlatformError{Bus, 11} (STAGE-3 §3.2)
//!   E_BUS-12 ROM topology mismatch  → PlatformError{Bus, 12} (STAGE-3 §3.2)
//!
//! Determinism (AD §8.1 D0): reads / writes are synchronous function
//! calls, no hand-shake timing. Virtual time is owned by M-12; this
//! module only stores the last-known tick supplied to the bus hook so
//! replayers can recover it. Trace hook is an opt-in [`BusHook`] that
//! receives every access (M-18 source).

#![allow(clippy::result_large_err)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::Peripheral;
use riscv_sim_contract::session::{M14MapDto, MemPageDto, RegionDto};

/// 4 GiB — covers the entire RV32 physical address space.
pub const BUS_SIZE: u64 = 0x1_0000_0000;

/// Default upper bound on a single region's **budget**, in bytes (D-10).
///
/// A malformed firmware image can declare a segment of ~4 GiB; without a
/// cap, the bus would happily hand out that much and OOM the host (in
/// the browser: OOM the tab). 16 MiB matches the order of magnitude of
/// M-06's `ParseLimits::max_region_bytes` default — M-14 does not depend
/// on `m06_elf`, so the constant is defined locally (the two sides are
/// independent; see `docs/reviews/REVIEWS.md` D-10).
///
/// ⚠ **What the cap measures changed in v0.26 (STAGE-11 裁决 B)** — the
/// constant's **name and value did not**. It used to bound a region's
/// *declared* size; it now bounds the region's **budget**, and the
/// measured quantity depends on the region kind:
///
/// | kind | holds memory? | measured |
/// | --- | --- | --- |
/// | RAM | lazily (only written pages exist) | **physical occupancy** (materialized pages × 4096) |
/// | ROM | yes (the content is part of the image) | **declared size** (== physical occupancy) |
/// | peripheral window | no (it claims an *address interval*) | **declared size** |
///
/// ⇒ **no guard was lost**: a region that really wants GiB-scale memory
/// is still refused (E_BUS-09), while a RAM region may now *declare*
/// 128 MiB and cost nothing until somebody writes to it.
pub const DEFAULT_MAX_REGION_BYTES: u64 = 16 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Session snapshot (STAGE-3 §3.2) — memory pages + memory-map topology.
// ---------------------------------------------------------------------------

/// Size of **one** RAM page — the physical allocation granularity
/// (STAGE-11 裁决 A / C-1).
///
/// ⚠ This is **not** a transport decision any more: 4 KiB is exactly the
/// **materialization granularity** of the lazy RAM page table, so an
/// exported page *is* a page that physically exists. The old 64 KiB
/// chunk (`MEM_PAGE_CHUNK_BYTES`, retired) would have put **one**
/// materialized page into **sixteen** exported entries — fifteen of them
/// pure padding, which is the very disease this batch removes.
pub const MEM_PAGE_BYTES: usize = 4096;

/// Region-kind tags of the memory-map topology — **re-exported** from the
/// contract layer (v0.27 · STAGE-12 §2.3).
///
/// ⚠️ **The definition moved up** (it used to live here): the platform
/// description (contract layer) needs the very same three words, and the
/// contract layer cannot depend on the kernel ⇒ the vocabulary's single
/// home is `riscv_sim_contract::session` (next to [`RegionDto::kind`]),
/// and this module re-exports it.
///
/// ⇒ **the literals did not change** and **no call site changed** — the
/// bus's behaviour, its snapshot export and its assertions are all
/// untouched.
pub use riscv_sim_contract::session::{REGION_KIND_PERIPHERAL, REGION_KIND_RAM, REGION_KIND_ROM};

/// Scenario code: two regions overlap (or an address is already mapped).
///
/// ⚠️ **A named constant, not a literal.** The same number was spelled
/// out as a bare `3` in several places — a bare number carries no
/// meaning, so the same value could later be reused for something else
/// without anybody noticing (one code, two meanings — the same trap as
/// `E_SNAP-02` / `E_SNAP-06`).
pub const E_BUS_03_OVERLAP: u16 = 3;

/// Scenario code: the region's address range does not fit the 32-bit
/// address space (`base + size` overflows, or the range leaves the
/// 4 GiB bus).
pub const E_BUS_04_OUT_OF_RANGE: u16 = 4;

/// Scenario code: the region's size is zero.
pub const E_BUS_06_ZERO_SIZE: u16 = 6;

/// Scenario code: the region exceeds the per-region budget cap.
///
/// ⚠ **The measured quantity changed in v0.26 (STAGE-11 裁决 B)** — the
/// **code did not**: it still means "this region is over the per-region
/// cap", and it is still the *only* code for that meaning (no new code,
/// no one code with two meanings).
///
/// What is measured depends on the region kind: **physical occupancy**
/// for RAM (its materialized pages), **declared size** for ROM and for
/// peripheral windows. ⚠ **Judgements never look at the message text**
/// — they assert `family == Bus && scenario == 9`.
pub const E_BUS_09_TOO_LARGE: u16 = 9;

/// Scenario code: the memory-map topology names a region kind this
/// build does not know.
///
/// ⚠️ A dedicated code, on purpose — `E_BUS-06` means "size == 0" and
/// `E_BUS-03` means "overlap"; neither of them can carry "this string
/// is not a region kind" without becoming two-meaning (the same lesson
/// as `E_SNAP-02` / `E_SNAP-06`, STAGE-3 §5).
pub const E_BUS_10_INVALID_REGION_KIND: u16 = 10;

/// Scenario code: a memory page does not fit inside **any** RAM
/// region of the bus.
///
/// ⚠️ Distinct from `E_BUS-01` ("unmapped address"): that one is about
/// a *guest access* to an address with no mapping, this one is about a
/// *snapshot page* that the memory map does not cover. A consumer that
/// receives `E_BUS-01` looks at the access; one that receives
/// `E_BUS-11` looks at the page table.
pub const E_BUS_11_PAGE_OUTSIDE_RAM: u16 = 11;

/// Scenario code: the ROM topology in the payload does not match the
/// ROM regions on the bus.
///
/// The ROM **content** is not part of the payload (it travels with the
/// firmware image — STAGE-3 §3.1 / §7), so the only place it can come
/// from is the ROM region that is on the bus **right now**. If the
/// payload's ROM regions and the bus's ROM regions do not correspond
/// one-to-one (same `base`, same `size`), the content cannot be
/// carried over faithfully — and a restore that quietly produced an
/// empty ROM would erase the firmware.
///
/// ⚠️ A dedicated code, on purpose: `E_BUS-04` is "base + size
/// overflows the 32-bit address space" and `E_BUS-03` is "two regions
/// overlap" — neither of them can carry "the ROM topology does not
/// match the payload".
pub const E_BUS_12_ROM_TOPOLOGY_MISMATCH: u16 = 12;

// ---------------------------------------------------------------------------
// Widths / access descriptors (kept local so future P1 extensions — e.g. 64-bit
// address space — do not ripple through `contract::WriteSize`).
// ---------------------------------------------------------------------------

/// Width of a single bus access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusWidth {
    /// 8-bit access.
    Byte,
    /// 16-bit (half-word) access.
    Half,
    /// 32-bit (word) access.
    Word,
}

impl BusWidth {
    /// Number of bytes covered by the access.
    pub fn bytes(self) -> usize {
        match self {
            Self::Byte => 1,
            Self::Half => 2,
            Self::Word => 4,
        }
    }
}

impl From<WriteSize> for BusWidth {
    fn from(s: WriteSize) -> Self {
        match s {
            WriteSize::Byte => Self::Byte,
            WriteSize::Half => Self::Half,
            WriteSize::Word => Self::Word,
        }
    }
}

impl From<BusWidth> for WriteSize {
    fn from(w: BusWidth) -> Self {
        match w {
            BusWidth::Byte => Self::Byte,
            BusWidth::Half => Self::Half,
            BusWidth::Word => Self::Word,
        }
    }
}

/// Direction of a single bus access (M-14 §2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDir {
    /// Reading from the addressed region.
    Read,
    /// Writing to the addressed region.
    Write,
}

/// Cumulative counters from bus dispatches (M-14 §2.1.1, revised by
/// `docs/reviews/M-14_BusMemoryMap_Review.md` to split per region
/// kind — see PROGRESS.md §7 decision 2026-10-19 review).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BusStats {
    /// Number of read dispatches. Each `read()` call increments by 1,
    /// regardless of whether the access split across byte slices.
    pub reads: u64,
    /// Number of write dispatches. Each `write()` call increments by 1.
    pub writes: u64,
    /// Number of successful landings on `RAM` regions. Counts both the
    /// happy path and the cross-region path.
    pub ram_hits: u64,
    /// Number of successful landings on `ROM` regions. ROM writes are
    /// silently dropped (评审 14-4) — they do NOT count here.
    pub rom_hits: u64,
    /// Number of successful landings on `Peripheral` regions. Only
    /// non-zero once M-15 wires its `read`/`write` through `Bus`
    /// (currently the bus surfaces E_BUS-05 "not wired" — see
    /// `#13` in the review).
    pub periph_hits: u64,
    /// Number of accesses that hit an unmapped address (E_BUS-01). This
    /// counts per `read()` / `write()` entry, not per byte slice.
    pub misses: u64,
    /// Number of multi-byte accesses whose first and last byte live in
    /// the same mapped region — `unaligned_splits` was previously
    /// conflated with unmapped ends; the review split those two
    /// counters.
    pub unaligned_splits: u64,
    /// Number of multi-byte accesses where at least one end lives in
    /// an unmapped region (separate from `unaligned_splits`). A byte-
    /// sliced read will be partially fulfilled; the surrounding
    /// `read()` will surface E_BUS-01.
    pub unmapped_splits: u64,
}

/// Bus-access event payload — passed to the optional [`BusHook`].
///
/// The hook fires **once per `read()` / `write()` entry**, not per byte
/// slice. A consumer that wants per-slice events must register its own
/// probe inside the matching module (M-18 / M-19). The doc wording prior
/// to the v0.6.1 review ("fire one hook per byte slice") was the
/// wrong way around — see `docs/reviews/M-14_BusMemoryMap_Review.md`
/// #1 / #2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusAccess {
    /// Virtual time at dispatch (ps). Sourced from M-12 authoritative clock.
    pub ts_ps: u64,
    /// Physical address of the *first byte* of the access.
    pub addr: u32,
    /// Width of the access (8 / 16 / 32). For cross-region byte slices,
    /// `width` still reports the original access width; consumers that
    /// need slice fidelity must look at `ok` and `err_scenario`.
    pub width: BusWidth,
    /// Read or write.
    pub dir: AccessDir,
    /// Dispatch outcome:
    /// - `true`  → the access landed on a mapped region and either
    ///   returned the read byte(s) or applied the write (including
    ///   silently-dropped ROM writes).
    /// - `false` → the access surfaced a bus error (E_BUS-01/04/05);
    ///   see `err_scenario` for the exact family+code.
    ///
    /// Consumers (M-18 waveform, M-11 trace export) must filter on
    /// `ok` if they only want well-formed accesses. Before the
    /// v0.6.1 review, the hook never fired for failed accesses — the
    /// reverse of what observability tooling needs.
    pub ok: bool,
    /// When `ok == false`, the [`PlatformError::scenario`] that the
    /// caller will receive. `None` on success.
    pub err_scenario: Option<u16>,
    /// Whether this access is an **instruction fetch** rather than a data
    /// access.
    ///
    /// A fetch is a read on the bus like any other, so the trace layer
    /// (M-18) still sees it — but a **watchpoint is about the guest's data
    /// accesses**, so a fetch must never match one. Without this flag a
    /// read watchpoint placed on a *code* address would fire on every
    /// instruction fetched from there (V-A 检视 · 🟠-2).
    pub is_fetch: bool,
}

/// Optional callback used by M-18 to capture every bus access for
/// waveform / observability. Stored as a `Box<dyn FnMut>` so the kernel
/// crate does not depend on a `tracing`-style runtime.
///
/// MVP scope: single-threaded simulation only, so we do not require
/// `Send`. When the production stack adds multithreaded dispatchers,
/// swap this for `Box<dyn FnMut(&BusAccess) + Send>` (M-12 P2 SMP).
pub type BusHook = Box<dyn FnMut(&BusAccess)>;

/// **外设时钟推进的结果**（[`Bus::tick_peripherals`]）。
///
/// 三件事一起报，不拆成三个返回值：
///
/// - **写**：外设在 tick 期间希望提交到总线上的写。调用方**必须**把它们
///   并进引擎的 `SimState::pending_writes`——不静默丢弃。✅ **提交路径
///   已通**（D-07，v0.15）：M-17 的执行器在每条指令之后把队列抽干并
///   **真正写进总线**（失败的那一条留在队列里，不静默丢）。
///   ⚠️ **生产路径不再直接调本方法**——外设时钟的推进点已**收敛**到
///   M-12 的 `Engine::tick_peripherals`（指令边界同步）与
///   `SchedEvent::PeripheralTick`（量子边界），两者走**同一个** ticker
///   接缝；本方法仍是 M-14 的公开 API，语义与那份接缝**同款**
///   （「报错的外设不许牵连别人」）。
/// - **错误**：第一个报错的外设的错误。**其余外设仍然会被 tick**
///   （V-A 检视 · 🟡-4）——一个外设出错不该让别的外设错过时钟同步。
///   已经收到的写**一条都不丢**。
/// - **跳过**：因为正被总线派发路径占用（`try_borrow_mut` 失败）而没
///   赶上这次 tick 的外设数量。
#[derive(Debug, Default)]
pub struct PeripheralTickOutcome {
    /// 本次 tick 期间外设希望提交到总线上的写。
    pub writes: Vec<PendingWrite>,
    /// 第一个报错的外设的错误（其余外设仍会被 tick）。
    pub error: Option<PlatformError>,
    /// 因为正被派发路径占用而跳过的外设数量。
    pub skipped: usize,
}

// ---------------------------------------------------------------------------
// Region kinds + typed handles (评审 14-12).
// ---------------------------------------------------------------------------

/// Region kind. Determines the dispatch behaviour and the handle kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegionType {
    /// Plain read / write bytes.
    #[default]
    Ram,
    /// Read returns; write is silently dropped (评审 14-4).
    Rom,
    /// Reads / writes forwarded to a peripheral (M-15).
    Peripheral,
}

/// Stable identifier for a peripheral registered on the bus (typed handle).
/// The runtime table from `id → Peripheral impl` lives in M-15; this is
/// just the *bus-side* token so the kernel doesn't have to know about
/// `Box<dyn Peripheral>` lifetimes (评审 14-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PeripheralId(pub u32);

/// A peripheral *instance* bound to a bus region (D-12).
///
/// The bus keeps a **shared handle** (`Rc<RefCell<dyn Peripheral>>`) rather
/// than owning the instance: M-15 stays the owner of the peripheral
/// (M-14 LLD §3.2 / M-15 LLD §4.4.4 — `attach` binds the interval, the
/// instance lives in M-15's instance table), so the engine can still reach
/// the same instance for `tick` / state inspection while the bus
/// dispatches guest accesses into it.
///
/// The trait object is the *contract* trait
/// (`riscv_sim_contract::port::Peripheral`) — M-14 never names an
/// M-15 concrete type, per the "契约先行" rule (LLD §2.5).
pub struct BoundPeripheral {
    /// Bus-side token (assigned by [`Bus::map_peripheral_instance`]).
    id: PeripheralId,
    /// Shared handle to the live instance.
    handle: Rc<RefCell<dyn Peripheral>>,
}

impl BoundPeripheral {
    /// Bus-side token of this binding.
    pub fn id(&self) -> PeripheralId {
        self.id
    }

    /// The shared handle to the live instance.
    pub fn handle(&self) -> &Rc<RefCell<dyn Peripheral>> {
        &self.handle
    }

    /// The peripheral's own string id (e.g. `"GPIO0"`), if the instance is
    /// not currently borrowed.
    pub fn instance_id(&self) -> Option<String> {
        self.handle.try_borrow().ok().map(|p| p.id().to_string())
    }

    /// Borrow the instance for a read-or-write dispatch.
    ///
    /// Returns `None` when the instance is already mutably borrowed — the
    /// caller turns that into E_BUS-05 instead of panicking (a panic in
    /// the dispatch path would take the whole simulation down).
    ///
    /// The bus always dispatches reads through `read_clear` (D-14), so
    /// the mutable borrow is what the read path needs too.
    pub(crate) fn try_write(&self) -> Option<std::cell::RefMut<'_, dyn Peripheral + 'static>> {
        self.handle.try_borrow_mut().ok()
    }
}

// `dyn Peripheral` has no `Debug` bound — hand-roll it.
impl std::fmt::Debug for BoundPeripheral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundPeripheral")
            .field("id", &self.id)
            .field("instance_id", &self.instance_id())
            .finish_non_exhaustive()
    }
}

/// One address interval registered on the bus. Owns its backing store
/// for RAM / ROM regions, or holds the peripheral token for `Peripheral`.
///
/// `base + size` is treated as a left-closed, right-open interval
/// `[base, base+size)`. `size == 0` is rejected at `map` time
/// (E_BUS-06, 评审 14-12).
///
/// ⚠ **The fields are module-private** (STAGE-11 裁决 D / D-13): a
/// `Region` is produced by the bus, never assembled by a caller. The
/// accessors below are the public way in.
#[derive(Debug)]
pub struct Region {
    /// Interval base address (`uint32_t` — see M-14 §2.1.1).
    base: u32,
    /// Interval size in bytes (must be > 0).
    size: u32,
    /// Region kind.
    kind: RegionType,
    /// Region-local backing store, or peripheral token.
    target: RegionTarget,
}

/// Region-local backing store chosen by `kind`.
///
/// `Ram` and `Rom` both carry an owned backing store — `Rom` simply
/// ignores writes. `Peripheral` regions carry either a [`PeripheralId`]
/// (legacy id-only mapping — nothing to dispatch to, E_BUS-05) or a
/// [`BoundPeripheral`] (live instance, D-12).
#[derive(Debug)]
pub enum RegionTarget {
    Ram(RamBackend),
    Rom(RomBackend),
    Peripheral(PeripheralId),
    /// A peripheral region bound to a live instance (D-12 wiring).
    PeripheralBound(BoundPeripheral),
}

/// RAM backing store — a **page table** with write-time allocation
/// (STAGE-11 裁决 A).
///
/// ⚠ **Registration and physical pages are two layers** (D-123): the
/// *region* is registered in O(1) and costs **zero** physical memory;
/// a **page** comes into existence only when something writes into it.
///
/// ⇒ reading a page that was never written reads back **0** — it does
/// **not** fault. Faulting is the *other* layer's business: an address
/// that is **not registered at all** is E_BUS-01 here, and M-13 turns
/// that into the architectural trap (cause 5 / 7). Merging the two
/// layers would destroy D-99's discriminating power.
#[derive(Debug)]
pub struct RamBackend {
    /// Materialized pages: key = page index **within the region**,
    /// value = the 4 KiB payload.
    ///
    /// ⚠ A page that is **not** in this map was never written ⇒ it
    /// reads back as zero and occupies **no** physical memory.
    pages: BTreeMap<u32, Box<[u8]>>,
    /// Bound base address (cached for diagnostics).
    base_addr: u32,
}

/// ROM backing store. Identical to RAM but flagged read-only at the
/// dispatch site — and **not** lazily allocated: the ROM content is
/// part of the firmware image (an *input*), so it is in memory by
/// definition (STAGE-11 §0.3 #8).
#[derive(Debug)]
pub struct RomBackend {
    /// Initial contents.
    bytes: Vec<u8>,
    /// Bound base address (cached for diagnostics).
    base_addr: u32,
}

impl RamBackend {
    /// Number of **materialized** pages.
    ///
    /// ⚠ This **counter** is the only legal way to observe the physical
    /// occupancy: a read value cannot tell "never written" from
    /// "written with a zero" (D-107's shape).
    pub fn allocated_pages(&self) -> usize {
        self.pages.len()
    }

    /// Physical bytes occupied by the materialized pages.
    pub fn allocated_bytes(&self) -> u64 {
        (self.pages.len() * MEM_PAGE_BYTES) as u64
    }

    /// The page index (within the region) that holds the region-relative
    /// offset `off`.
    fn page_index(off: usize) -> u32 {
        (off / MEM_PAGE_BYTES) as u32
    }

    /// Read `width` bytes at the region-relative offset `off`.
    ///
    /// A page that was never materialized reads back as **0** — and the
    /// read **materializes nothing**.
    fn read_at(
        &self,
        region_size: usize,
        off: usize,
        width: BusWidth,
    ) -> Result<u32, PlatformError> {
        let end = off + width.bytes();
        if end > region_size {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_04_OUT_OF_RANGE,
                format!("RAM read past end off=0x{:x} width={:?}", off, width),
            ));
        }
        let mut acc: u32 = 0;
        for i in 0..width.bytes() {
            let a = off + i;
            let page = self.pages.get(&Self::page_index(a));
            let b = page.map(|p| p[a % MEM_PAGE_BYTES]).unwrap_or(0);
            acc |= (b as u32) << (i * 8);
        }
        Ok(acc)
    }

    /// Read one byte at the region-relative offset `off`.
    fn read_byte_at(&self, region_size: usize, off: usize) -> Result<u8, PlatformError> {
        if off >= region_size {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_04_OUT_OF_RANGE,
                format!("RAM read past end off=0x{:x}", off),
            ));
        }
        Ok(self
            .pages
            .get(&Self::page_index(off))
            .map(|p| p[off % MEM_PAGE_BYTES])
            .unwrap_or(0))
    }

    /// Write `width` bytes at the region-relative offset `off`.
    ///
    /// ⚠ **The three steps below are not interchangeable** (裁决 A):
    ///
    /// 1. **region boundary** (`off + width <= region.size`) ⇒
    ///    **E_BUS-04**, and **no page is left behind** — an
    ///    out-of-bounds write must not conjure a page;
    /// 2. **physical occupancy cap** (the pages this access would
    ///    bring into existence, times 4096) ⇒ **E_BUS-09**, again with
    ///    **no page left behind**;
    /// 3. only then **materialize** the pages that are needed and
    ///    write.
    ///
    /// ⚠ A single access **may** straddle two pages (a 4-byte write at
    /// page offset 4092 ends at 4095, but one at page offset 4095 ends
    /// at 4098 ⇒ it touches the next page). The loop below is written
    /// byte-wise for exactly that reason — there is no "an access never
    /// crosses a page" shortcut to get wrong.
    fn write_at(
        &mut self,
        region_size: usize,
        off: usize,
        width: BusWidth,
        value: u32,
        cap: u64,
    ) -> Result<(), PlatformError> {
        let end = off + width.bytes();
        if end > region_size {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_04_OUT_OF_RANGE,
                format!("RAM write past end off=0x{:x} width={:?}", off, width),
            ));
        }
        // The pages this access touches.
        let first = Self::page_index(off);
        let last = Self::page_index(end - 1);
        // ② The cap is checked **before** anything is materialized.
        let physical = (self.projected_pages(first, last) as u64) * (MEM_PAGE_BYTES as u64);
        if physical > cap {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_09_TOO_LARGE,
                format!(
                    "RAM region would physically occupy {physical} allocated bytes \
                     (materialized pages), over the per-region cap of {cap} bytes"
                ),
            ));
        }
        // ③ Materialize, then write.
        for p in first..=last {
            self.pages
                .entry(p)
                .or_insert_with(|| vec![0u8; MEM_PAGE_BYTES].into_boxed_slice());
        }
        for i in 0..width.bytes() {
            let a = off + i;
            let page = self
                .pages
                .get_mut(&Self::page_index(a))
                .expect("the page was just materialized");
            page[a % MEM_PAGE_BYTES] = ((value >> (i * 8)) & 0xFF) as u8;
        }
        Ok(())
    }

    /// The number of pages the region would hold **after** the pages
    /// `first..=last` are materialized.
    ///
    /// Used by the cap check (before anything is materialized) — a
    /// failed write must not leave a page behind (裁决 A).
    fn projected_pages(&self, first: u32, last: u32) -> usize {
        let mut projected = self.pages.len();
        for p in first..=last {
            if !self.pages.contains_key(&p) {
                projected += 1;
            }
        }
        projected
    }
}

impl RomBackend {
    /// The ROM content (read-only).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Region {
    /// The interval's base address.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The interval's size in bytes.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// The region's kind.
    pub fn kind(&self) -> RegionType {
        self.kind
    }

    /// Whether `addr ∈ [self.base, self.base + self.size)`.
    ///
    /// Review #12: previously used `wrapping_add`, which made `addr`
    /// past `0xFFFF_FFFF` silently wrap into `self.base..self.base +
    /// size`. `map_region` already rejects `base + size > u32::MAX`,
    /// so the in-range `contains` result is safe; but for `addr` at
    /// the wrap boundary the previous code could spuriously match.
    /// Use `checked_add` and treat any overflow as "not contained"
    /// — defence in depth against a future API consumer that hands
    /// us an address from an untrusted source.
    pub fn contains(&self, addr: u32) -> bool {
        match self.base.checked_add(self.size) {
            Some(end) => self.base <= addr && addr < end,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Driver port — declared at the trait surface for callers that prefer to
// speak to "any bus master" (ISS / debug / future mocks). The `Bus` type
// itself implements it via `&mut self`; a thin `BusAdapter` below exposes
// the same surface for `&self` use-sites (e.g. inside ISS dispatch where
// only a shared reference is available).
// ---------------------------------------------------------------------------

/// Family used by errors raised from this module (LLD §3.4).
pub const BUS_FAMILY: ErrorFamily = ErrorFamily::Bus;

/// Anything that performs bus-level reads / writes (ISS, peripherals,
/// debug services). Provided so downstream modules can hold a trait
/// object and swap implementations for testing or P1 multi-bus work.
pub trait BusMaster {
    /// Read `size` bytes at `paddr` (zero-extended).
    fn read(&mut self, paddr: u32, size: WriteSize) -> Result<u32, PlatformError>;
    /// Write `size` bytes at `paddr`.
    fn write(&mut self, paddr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError>;
}

impl BusMaster for Bus {
    fn read(&mut self, paddr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        Bus::read(self, paddr, size.into())
    }
    fn write(&mut self, paddr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        Bus::write(self, paddr, size.into(), value)
    }
}

// ---------------------------------------------------------------------------
// The bus itself.
// ---------------------------------------------------------------------------

/// The address decoder + dispatch table.
///
/// Internal layout:
/// - `regions`: `BTreeMap<base, Region>` — base → region. Look-ups are
///   O(log n) over the tree, and the next / previous entry gives us the
///   "neighbour on the right / left" used for cross-region splitting.
/// - `stats`: monotonically increasing counters (per `BusStats` field
///   in M-14 §2.1.1).
///
/// ⚠ **A RAM region is a page table, not one big byte vector**
/// (STAGE-11 裁决 A): the *registration* is O(1) and costs no physical
/// memory, while the **pages** come into existence on write. A region
/// may therefore declare 128 MiB and cost nothing until the guest
/// writes to it.
pub struct Bus {
    regions: BTreeMap<u32, Region>,
    /// The bus-hook callbacks, in installation order.
    ///
    /// **Several** consumers need to observe every bus access: M-17's
    /// watchpoint matcher and M-18's trace ring are two independent
    /// subscribers, and neither may evict the other. A single slot
    /// would have made the second `set_hook` silently replace the
    /// first (D-46).
    hooks: Vec<BusHook>,
    last_ts_ps: u64,
    stats: BusStats,
    /// Upper bound on a single region's budget, in bytes (D-10).
    ///
    /// ⚠ **What is measured depends on the region kind** (STAGE-11
    /// 裁决 B): **physical occupancy** for RAM (its materialized
    /// pages), **declared size** for ROM and for peripheral windows.
    /// Checked **before** any backing store is allocated, so a
    /// malformed firmware segment can never make the host allocate
    /// GiB-scale memory. See [`DEFAULT_MAX_REGION_BYTES`].
    max_region_bytes: u64,
    /// Next bus-side peripheral token handed out by
    /// [`Bus::map_peripheral_instance`].
    next_periph_id: u32,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            regions: BTreeMap::new(),
            hooks: Vec::new(),
            last_ts_ps: 0,
            stats: BusStats::default(),
            max_region_bytes: DEFAULT_MAX_REGION_BYTES,
            next_periph_id: 0,
        }
    }
}

impl std::fmt::Debug for Bus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bus")
            .field("regions.len", &self.regions.len())
            .field("stats", &self.stats)
            .field("hook.count", &self.hooks.len())
            .field("max_region_bytes", &self.max_region_bytes)
            .finish()
    }
}

impl Bus {
    /// Build an empty bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an empty bus with a custom per-region budget cap (D-10).
    ///
    /// `max_region_bytes` is a plain upper bound: `0` means *no region
    /// fits at all*, `u64::MAX` means *effectively unlimited*. The
    /// default ([`DEFAULT_MAX_REGION_BYTES`]) is the safe one — use
    /// this constructor only when the host has its own budget policy.
    ///
    /// ⚠ **What the cap measures** (STAGE-11 裁决 B): **physical
    /// occupancy** for RAM (its materialized pages × 4096), **declared
    /// size** for ROM and for peripheral windows. ⇒ the host's budget
    /// policy really is the host's business: a host that declares a
    /// 128 MiB RAM region simply raises the cap to 128 MiB.
    pub fn with_max_region_bytes(max_region_bytes: u64) -> Self {
        Self {
            max_region_bytes,
            ..Self::default()
        }
    }

    /// Current per-region budget cap, in bytes (D-10).
    ///
    /// ⚠ See [`Bus::with_max_region_bytes`] for what the cap measures.
    pub fn max_region_bytes(&self) -> u64 {
        self.max_region_bytes
    }

    /// Adjust the per-region budget cap at runtime (D-10).
    ///
    /// The cap is enforced on the *next* mapping operation **and** on
    /// the next write that would grow a region's physical occupancy;
    /// regions that are already mapped are untouched.
    pub fn set_max_region_bytes(&mut self, max_region_bytes: u64) {
        self.max_region_bytes = max_region_bytes;
    }

    /// Map a RAM region into the bus.
    ///
    /// ⚠ **The declared `size` is not the memory cost** (STAGE-11
    /// 裁决 A / B): the region is *registered* (O(1), zero physical
    /// cost) and its pages are materialized **on write**. `init`
    /// materializes the pages it covers (that is the only RAM path
    /// that costs anything up front).
    pub fn map_ram(&mut self, base: u32, size: u32, init: &[u8]) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Ram,
            target: RegionTarget::Ram(Self::prepare_ram_pages(base, size, init, cap)?),
        })
    }

    /// Map a ROM region. Writes will be silently dropped.
    ///
    /// ⚠ A ROM region is **not** lazily allocated: its content is part
    /// of the firmware image (an *input*), so it is in memory by
    /// definition ⇒ the cap measures its **declared size**.
    pub fn map_rom(&mut self, base: u32, size: u32, init: &[u8]) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Rom,
            target: RegionTarget::Rom(RomBackend {
                bytes: Self::prepare_rom_bytes(base, size, init, cap)?,
                base_addr: base,
            }),
        })
    }

    /// Map a peripheral region pointing at the given peripheral id.
    ///
    /// **Id-only mapping — nothing to dispatch to.** The bus has no
    /// instance behind the id, so every access to this window surfaces
    /// E_BUS-05. Use [`Bus::map_peripheral_instance`] for real wiring;
    /// this entry point is kept for compatibility with the pre-D-12
    /// call sites (a typed handle without an instance).
    ///
    /// ⚠ A peripheral window holds **no memory** — it claims an
    /// *address interval* — so the cap measures its **declared size**
    /// (it guards against "one device claims a GiB window").
    pub fn map_peripheral(
        &mut self,
        base: u32,
        size: u32,
        id: PeripheralId,
    ) -> Result<(), PlatformError> {
        self.assert_window_within_cap(base, size)?;
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Peripheral,
            target: RegionTarget::Peripheral(id),
        })
    }

    /// Map a peripheral **instance** onto the bus (D-12 wiring).
    ///
    /// This is the real M-14 ↔ M-15 junction: accesses that land in
    /// `[base, base + size)` are dispatched into the instance
    /// (`read_clear` / `write`, see D-14) instead of failing with
    /// E_BUS-05.
    ///
    /// The instance is held as a **shared handle** so M-15 stays the
    /// owner (the engine can still `tick` it and inspect its state).
    ///
    /// ## Validation
    ///
    /// - `size` **must** equal the instance's own
    ///   [`Peripheral::size`] — otherwise the bus window and the
    ///   peripheral's register file disagree and accesses would land
    ///   outside the peripheral's own view. Mismatch → **E_BUS-08**.
    /// - `size` must also respect the bus-wide cap
    ///   ([`Bus::max_region_bytes`], D-10) → **E_BUS-09**.
    ///   ⚠ A peripheral window holds **no memory** ⇒ the cap measures
    ///   its **declared size** (it guards against "one device claims a
    ///   GiB window"), exactly as it did before v0.26.
    /// - Overlap with an existing region → **E_BUS-03**.
    ///
    /// Every failure leaves the existing region set untouched.
    pub fn map_peripheral_instance(
        &mut self,
        base: u32,
        size: u32,
        handle: Rc<RefCell<dyn Peripheral>>,
    ) -> Result<(), PlatformError> {
        // D-12: the window size must match the peripheral's own size.
        // Read it through a shared borrow; a contended instance (the
        // caller is holding it borrowed) is reported as a bus error
        // rather than panicking.
        let (periph_size, periph_id) = {
            let periph = handle.try_borrow().map_err(|_| {
                PlatformError::new(
                    BUS_FAMILY,
                    5,
                    "peripheral instance is borrowed; cannot validate its window size",
                )
            })?;
            (periph.size(), periph.id().to_string())
        };
        if size != periph_size {
            return Err(PlatformError::new(
                BUS_FAMILY,
                8,
                format!(
                    "peripheral window size mismatch: bus window is {size} bytes but \
                     peripheral '{periph_id}' declares {} bytes",
                    periph_size
                ),
            ));
        }
        self.assert_window_within_cap(base, size)?;
        let id = PeripheralId(self.next_periph_id);
        self.next_periph_id = self.next_periph_id.wrapping_add(1);
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Peripheral,
            target: RegionTarget::PeripheralBound(BoundPeripheral { id, handle }),
        })
    }

    /// The peripheral instance bound to the region at `base`, if any.
    pub fn peripheral_at(&self, base: u32) -> Option<&BoundPeripheral> {
        match self.regions.get(&base).map(|r| &r.target) {
            Some(RegionTarget::PeripheralBound(b)) => Some(b),
            _ => None,
        }
    }

    /// D-10: the peripheral window must respect the bus-wide cap.
    ///
    /// ⚠ A peripheral window holds **no memory** ⇒ the measured
    /// quantity is its **declared size** (STAGE-11 裁决 B, 规格订正):
    /// the guard protects against "one device claims a GiB window".
    fn assert_window_within_cap(&self, base: u32, size: u32) -> Result<(), PlatformError> {
        if u64::from(size) > self.max_region_bytes {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_09_TOO_LARGE,
                format!(
                    "peripheral window size {size} exceeds the per-region cap of {} bytes \
                     (base=0x{base:08x})",
                    self.max_region_bytes
                ),
            ));
        }
        Ok(())
    }

    /// Build the **lazily allocated** backing store of a RAM region.
    ///
    /// ⚠ **D-10 / 裁决 B:** the cap is checked against the region's
    /// **physical occupancy** — the pages `init` actually materializes —
    /// and *before* any of them is allocated, so a malformed firmware
    /// segment can never make the host reserve GiB-scale memory. The
    /// region's **declared** size is deliberately **not** measured: it
    /// is an address interval, not a memory cost.
    fn prepare_ram_pages(
        base: u32,
        size: u32,
        init: &[u8],
        max_region_bytes: u64,
    ) -> Result<RamBackend, PlatformError> {
        if size == 0 {
            return Err(PlatformError::new(BUS_FAMILY, 6, "region size must be > 0"));
        }
        // D-10: reject an over-cap physical occupancy *before* allocating.
        let pages = init.len().div_ceil(MEM_PAGE_BYTES);
        let physical = (pages as u64) * (MEM_PAGE_BYTES as u64);
        if physical > max_region_bytes {
            return Err(PlatformError::new(
                BUS_FAMILY,
                E_BUS_09_TOO_LARGE,
                format!(
                    "RAM region would physically occupy {physical} allocated bytes \
                     (materialized pages), over the per-region cap of {max_region_bytes} bytes \
                     (base=0x{base:08x})"
                ),
            ));
        }
        if init.len() > size as usize {
            // Review #7: split this off from E_BUS-04 (overflow) so
            // the host can distinguish a malformed init payload from a
            // legitimately-sized region that happens to overflow the
            // 4 GiB bus.
            return Err(PlatformError::new(
                BUS_FAMILY,
                7,
                format!(
                    "init bytes ({}) exceed region size ({}) at base=0x{:08x}",
                    init.len(),
                    size,
                    base
                ),
            ));
        }
        let mut page_map: BTreeMap<u32, Box<[u8]>> = BTreeMap::new();
        for (index, chunk) in init.chunks(MEM_PAGE_BYTES).enumerate() {
            let mut page = vec![0u8; MEM_PAGE_BYTES].into_boxed_slice();
            page[..chunk.len()].copy_from_slice(chunk);
            page_map.insert(index as u32, page);
        }
        Ok(RamBackend {
            pages: page_map,
            base_addr: base,
        })
    }

    /// Allocate the backing byte vector of a **ROM** region and
    /// validate `init` length.
    ///
    /// **D-10:** the ROM content is part of the firmware image (an
    /// *input*) ⇒ it is in memory by definition ⇒ the cap measures the
    /// region's **declared size**, exactly as it did before v0.26. The
    /// size cap is checked *before* the allocation, so a malformed
    /// firmware segment can never make the host reserve GiB of memory.
    /// `max_region_bytes` is the bus-wide cap
    /// ([`Bus::max_region_bytes`]).
    fn prepare_rom_bytes(
        base: u32,
        size: u32,
        init: &[u8],
        max_region_bytes: u64,
    ) -> Result<Vec<u8>, PlatformError> {
        if size == 0 {
            return Err(PlatformError::new(BUS_FAMILY, 6, "region size must be > 0"));
        }
        // D-10: reject oversized regions *before* allocating.
        if u64::from(size) > max_region_bytes {
            return Err(PlatformError::new(
                BUS_FAMILY,
                9,
                format!(
                    "region size {size} exceeds the per-region cap of {max_region_bytes} bytes \
                     (base=0x{base:08x})"
                ),
            ));
        }
        if init.len() > size as usize {
            // Review #7: split this off from E_BUS-04 (overflow) so
            // the host can distinguish a malformed init payload from a
            // legitimately-sized region that happens to overflow the
            // 4 GiB bus.
            return Err(PlatformError::new(
                BUS_FAMILY,
                7,
                format!(
                    "init bytes ({}) exceed region size ({}) at base=0x{:08x}",
                    init.len(),
                    size,
                    base
                ),
            ));
        }
        let mut bytes = vec![0u8; size as usize];
        bytes[..init.len()].copy_from_slice(init);
        Ok(bytes)
    }

    /// Atomic region insert: validate `size > 0`, overflow, and overlap
    /// against current regions, then insert. All failures preserve the
    /// existing region set (LLD §3.3).
    ///
    /// ⚠ **The D-10 cap is checked by the *kind-specific* mapping paths**,
    /// not here (STAGE-11 裁决 B): RAM is measured by its **physical
    /// occupancy**, ROM and peripheral windows by their **declared
    /// size**. A single generic "declared size" check here would have
    /// re-introduced the very rule the batch removed (and would have
    /// dropped the peripheral-window guard).
    fn map_region(&mut self, mut region: Region) -> Result<(), PlatformError> {
        if region.size == 0 {
            return Err(PlatformError::new(BUS_FAMILY, 6, "region size must be > 0"));
        }
        let end = region.base.checked_add(region.size).ok_or_else(|| {
            PlatformError::new(
                BUS_FAMILY,
                4,
                format!(
                    "base + size overflows u32 (base=0x{:08x}, size=0x{:x})",
                    region.base, region.size
                ),
            )
        })?;
        if u64::from(end) > BUS_SIZE {
            return Err(PlatformError::new(
                BUS_FAMILY,
                4,
                format!(
                    "region 0x{:08x}..0x{:08x} exceeds 4 GiB bus",
                    region.base, end
                ),
            ));
        }
        self.assert_no_overlap(region.base, end)?;
        // Patch the cached base on the inner backend (already done by
        // the helpers, but defensively rewrite).
        match &mut region.target {
            RegionTarget::Ram(r) => r.base_addr = region.base,
            RegionTarget::Rom(r) => r.base_addr = region.base,
            RegionTarget::Peripheral(_) | RegionTarget::PeripheralBound(_) => {}
        }
        self.regions.insert(region.base, region);
        Ok(())
    }

    /// O(log n) overlap check using `BTreeMap::range`.
    fn assert_no_overlap(&self, base: u32, end: u32) -> Result<(), PlatformError> {
        // The base itself must be unused (avoids accidental aliasing).
        if self.regions.contains_key(&base) {
            return Err(PlatformError::new(
                BUS_FAMILY,
                3,
                format!("region 0x{:08x} already mapped", base),
            ));
        }
        // Any predecessor whose `[b, b+sz)` overlaps `[base, end)`.
        if let Some((_, prev)) = self.regions.range(..base).next_back() {
            if prev.contains(base) {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    3,
                    format!(
                        "new region 0x{:08x}..0x{:08x} overlaps existing 0x{:08x}..0x{:08x}",
                        base,
                        end,
                        prev.base,
                        prev.base.wrapping_add(prev.size)
                    ),
                ));
            }
        }
        // Any successor starting inside `[base, end)`.
        if let Some((succ_base, succ)) = self.regions.range(base..).next() {
            if *succ_base < end {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    3,
                    format!(
                        "new region 0x{:08x}..0x{:08x} overlaps existing 0x{:08x}..0x{:08x}",
                        base,
                        end,
                        succ.base,
                        succ.base.wrapping_add(succ.size)
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Idempotent unmap. Returns whether anything was removed.
    pub fn unmap(&mut self, base: u32) -> bool {
        self.regions.remove(&base).is_some()
    }

    /// Look up the region containing `addr`, or `None`.
    pub fn lookup(&self, addr: u32) -> Option<&Region> {
        // `range(..=addr).next_back()` gives the greatest base ≤ addr.
        self.regions
            .range(..=addr)
            .next_back()
            .map(|(_, r)| r)
            .filter(|r| r.contains(addr))
    }

    /// Number of regions currently mapped.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Iterate regions in ascending base order.
    pub fn regions(&self) -> impl Iterator<Item = (&u32, &Region)> {
        self.regions.iter()
    }

    // -----------------------------------------------------------------------
    // Session snapshot (STAGE-3 §3.2): memory pages (segment 2) and the
    // memory-map topology (segment 3).
    // -----------------------------------------------------------------------

    /// Export the **memory-map topology** — segment 3 of the session
    /// payload (STAGE-3 §3.2).
    ///
    /// Every region is listed — RAM, ROM **and** peripheral — ordered by
    /// `base` (the region table is a [`BTreeMap`], so the order is
    /// canonical by construction, not by a post-hoc sort).
    ///
    /// - `kind` is the **lower-case string** the wire format demands
    ///   (`"ram"` / `"rom"` / `"peripheral"`) — never an enum ordinal,
    ///   so a reordering of [`RegionType`] cannot silently change what
    ///   a stored payload means.
    /// - `target` is **empty** for RAM / ROM (they have no identity
    ///   beyond their address range) and carries the **peripheral's own
    ///   id** (`"GPIO0"`) for a peripheral region: a *bound* instance is
    ///   not serializable, only its id is — the restore path re-binds
    ///   by id (STAGE-3 §3.1).
    ///
    /// A peripheral region whose instance is **currently borrowed** (a
    /// dispatch is in flight) cannot report its id; the bus-side token
    /// is used as a fallback (`"periph#<token>"`). That token is **not**
    /// bindable — it only keeps the export lossless instead of
    /// dropping the target on the floor.
    pub fn export_map(&self) -> M14MapDto {
        let mut regions = Vec::with_capacity(self.regions.len());
        for (base, region) in &self.regions {
            let (kind, target) = match &region.target {
                RegionTarget::Ram(_) => (REGION_KIND_RAM, String::new()),
                RegionTarget::Rom(_) => (REGION_KIND_ROM, String::new()),
                RegionTarget::PeripheralBound(bound) => (
                    REGION_KIND_PERIPHERAL,
                    bound
                        .instance_id()
                        .unwrap_or_else(|| format!("periph#{}", bound.id().0)),
                ),
                RegionTarget::Peripheral(id) => {
                    (REGION_KIND_PERIPHERAL, format!("periph#{}", id.0))
                }
            };
            regions.push(RegionDto {
                base: *base,
                size: region.size,
                kind: kind.to_string(),
                target,
            });
        }
        M14MapDto { regions }
    }

    /// Restore the **memory-map topology** — segment 3's consumer
    /// (STAGE-3 §3.2).
    ///
    /// ## What is rebuilt
    ///
    /// **RAM and ROM regions are rebuilt** from the payload: the old
    /// RAM / ROM regions are dropped and new ones are mapped at the
    /// payload's addresses.
    ///
    /// - **RAM**: the rebuilt region starts **zeroed**. The RAM content
    ///   is not part of the topology — it travels in segment 2
    ///   ([`Bus::load_pages`]) and is poured back in afterwards.
    /// - **ROM**: the rebuilt region keeps the **content of the ROM
    ///   region it replaces** (byte for byte). The ROM content is not
    ///   in the payload either (it travels with the **firmware image**,
    ///   which is an *input* of a session — STAGE-3 §3.1 / §7), so the
    ///   only place it can come from is the ROM region that is on the
    ///   bus right now. ⚠️ Rebuilding a ROM region **zeroed** would
    ///   erase the firmware: the restored session would fault on its
    ///   very first instruction.
    ///
    ///   ⚠️ The ROM content is carried over **by region identity**:
    ///   the payload's ROM region must have the **same `base` and the
    ///   same `size`** as the ROM region on the bus. A mismatch is
    ///   **rejected** ([`E_BUS_12_ROM_TOPOLOGY_MISMATCH`]) — the
    ///   content is *not* moved to another address, because a ROM
    ///   whose content does not match its address range is not the
    ///   firmware any more.
    ///
    /// ## What is **kept in place** — and why
    ///
    /// **Peripheral regions are neither deleted nor rebuilt.** They
    /// stay exactly as they are — same address window, same bound
    /// instance.
    ///
    /// ⚠️ This is a **correctness requirement**, not a shortcut. A
    /// peripheral region is an **input** of the session: the host binds
    /// the live instances from the firmware. If `load_map` deleted
    /// those windows, the restored bus would have **no GPIO / UART** —
    /// the firmware's next instruction would fault on a missing
    /// peripheral, and the restored session would silently **diverge**
    /// from the captured one. Keeping them is what makes "snapshot →
    /// restore → run again" reproduce the same session.
    ///
    /// ⚠️ The **peripheral entries inside the payload are therefore
    /// ignored** — they describe the topology, not the instances. The
    /// instance is re-bound by the host, from the firmware.
    ///
    /// ## Validation (all of it runs **before** anything is written)
    ///
    /// - unknown `kind` → **E_BUS-10**;
    /// - `size == 0` → **E_BUS-06**; `base + size` overflows → **E_BUS-04**;
    /// - region larger than the bus-wide cap → **E_BUS-09**;
    /// - two RAM / ROM regions overlap, or one of them overlaps a
    ///   **kept** peripheral region → **E_BUS-03**;
    /// - the ROM regions of the payload and of the bus do not
    ///   correspond one-to-one → **E_BUS-12**.
    ///
    /// A rejected topology leaves the bus **exactly as it was** — a
    /// half-applied map is worse than a rejected one (STAGE-3 §5).
    pub fn load_map(&mut self, map: &M14MapDto) -> Result<(), PlatformError> {
        // ---- Phase 1: validate the whole topology, touch nothing yet. ----
        let mut rebuild: Vec<&RegionDto> = Vec::new();
        for r in &map.regions {
            match r.kind.as_str() {
                REGION_KIND_RAM | REGION_KIND_ROM => {
                    if r.size == 0 {
                        return Err(PlatformError::new(
                            BUS_FAMILY,
                            E_BUS_06_ZERO_SIZE,
                            format!("region 0x{:08x} has size 0", r.base),
                        ));
                    }
                    let end = r.base.checked_add(r.size).ok_or_else(|| {
                        PlatformError::new(
                            BUS_FAMILY,
                            E_BUS_04_OUT_OF_RANGE,
                            format!(
                                "base + size overflows u32 (base=0x{:08x}, size=0x{:x})",
                                r.base, r.size
                            ),
                        )
                    })?;
                    if u64::from(end) > BUS_SIZE {
                        return Err(PlatformError::new(
                            BUS_FAMILY,
                            E_BUS_04_OUT_OF_RANGE,
                            format!("region 0x{:08x}..0x{:08x} exceeds 4 GiB bus", r.base, end),
                        ));
                    }
                    // D-10 / 裁决 B: the cap is measured **per region
                    // kind**. A **RAM** region in the topology holds no
                    // memory yet (its content arrives in segment 2,
                    // `load_pages`, which applies the cap to the pages
                    // it materializes) ⇒ its declared size is **not**
                    // measured here. A **ROM** region keeps its content
                    // ⇒ its declared size **is** its physical occupancy
                    // ⇒ it is measured here.
                    if r.kind == REGION_KIND_ROM && u64::from(r.size) > self.max_region_bytes {
                        return Err(PlatformError::new(
                            BUS_FAMILY,
                            E_BUS_09_TOO_LARGE,
                            format!(
                                "ROM region size {} exceeds the per-region cap of {} bytes \
                                 (base=0x{:08x})",
                                r.size, self.max_region_bytes, r.base
                            ),
                        ));
                    }
                    rebuild.push(r);
                }
                // Kept in place — deliberately not rebuilt (see the doc).
                REGION_KIND_PERIPHERAL => {}
                other => {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_10_INVALID_REGION_KIND,
                        format!("unknown region kind '{other}' at base=0x{:08x}", r.base),
                    ));
                }
            }
        }

        // The rebuilt intervals must not overlap each other …
        let mut intervals: Vec<(u32, u32)> =
            rebuild.iter().map(|r| (r.base, r.base + r.size)).collect();
        intervals.sort_unstable();
        for w in intervals.windows(2) {
            if w[1].0 < w[0].1 {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_03_OVERLAP,
                    format!(
                        "rebuilt regions 0x{:08x}..0x{:08x} and 0x{:08x}..0x{:08x} overlap",
                        w[0].0, w[0].1, w[1].0, w[1].1
                    ),
                ));
            }
        }

        // … nor overlap a peripheral region that stays in place.
        for (base, region) in &self.regions {
            if !matches!(
                region.target,
                RegionTarget::Peripheral(_) | RegionTarget::PeripheralBound(_)
            ) {
                continue;
            }
            let end = base + region.size;
            for (rbase, rend) in &intervals {
                if *rbase < end && *base < *rend {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_03_OVERLAP,
                        format!(
                            "rebuilt region 0x{:08x}..0x{:08x} overlaps the peripheral window \
                             0x{:08x}..0x{:08x} that is kept in place",
                            rbase, rend, base, end
                        ),
                    ));
                }
            }
        }

        // The ROM regions of the payload and of the bus must correspond
        // **one-to-one** (same `base`, same `size`): the ROM content is
        // not in the payload, so the content of the region on the bus is
        // the only thing that can be carried over. A mismatch is
        // rejected instead of producing an empty (i.e. erased) ROM.
        //
        // The content is copied out **now** — the regions are dropped
        // in phase 2, and the copy is what survives the rebuild.
        let current_rom: BTreeMap<u32, (u32, Vec<u8>)> = self
            .regions
            .iter()
            .filter_map(|(base, region)| match &region.target {
                RegionTarget::Rom(rom) => Some((*base, (region.size, rom.bytes().to_vec()))),
                _ => None,
            })
            .collect();

        for r in rebuild.iter().filter(|r| r.kind == REGION_KIND_ROM) {
            match current_rom.get(&r.base) {
                None => {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_12_ROM_TOPOLOGY_MISMATCH,
                        format!(
                            "会话载荷里的 ROM 区域 0x{:08x} 在当前总线上没有对应区域——ROM 内容无法保留",
                            r.base
                        ),
                    ));
                }
                Some((size, _)) if *size != r.size => {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_12_ROM_TOPOLOGY_MISMATCH,
                        format!(
                            "ROM 区域 0x{:08x} 的大小不一致：载荷 {} 字节，总线 {} 字节——ROM 内容无法保留",
                            r.base, r.size, size
                        ),
                    ));
                }
                Some(_) => {}
            }
        }
        for (base, (size, _)) in &current_rom {
            if !rebuild
                .iter()
                .any(|r| r.kind == REGION_KIND_ROM && r.base == *base && r.size == *size)
            {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_12_ROM_TOPOLOGY_MISMATCH,
                    format!(
                        "当前总线上的 ROM 区域 0x{:08x}（{} 字节）不在会话载荷里——重建会丢掉固件",
                        base, size
                    ),
                ));
            }
        }

        // ---- Phase 2: apply. RAM / ROM are rebuilt, peripherals stay. ----
        self.regions
            .retain(|_, r| !matches!(r.target, RegionTarget::Ram(_) | RegionTarget::Rom(_)));
        for r in rebuild {
            if r.kind == REGION_KIND_RAM {
                // The RAM content is not part of the topology — it is
                // poured back in by `load_pages` (segment 2).
                self.map_ram(r.base, r.size, &[])?;
            } else {
                // The ROM content follows the region it replaces.
                let content: &[u8] = current_rom
                    .get(&r.base)
                    .map(|(_, bytes)| bytes.as_slice())
                    .unwrap_or(&[]);
                self.map_rom(r.base, r.size, content)?;
            }
        }
        Ok(())
    }

    /// Export the **guest-visible memory pages** (RAM content) — segment
    /// 2's producer (STAGE-3 §3.2).
    ///
    /// Only **RAM** regions are exported:
    ///
    /// - ROM content travels with the **firmware image** (an input of a
    ///   session, not its state) — see STAGE-3 §3.1;
    /// - peripheral windows hold **no** memory content at all.
    ///
    /// ⚠ **Only the pages that physically exist are exported**
    /// (STAGE-11 裁决 C-2): one entry = one **materialized** 4 KiB page.
    /// A page that was never written is not in the payload — it reads
    /// back as zero anyway, and carrying it would have been the very
    /// payload bloat this batch removes.
    ///
    /// - `base` = `region.base + page_index × 4096` ⇒ the page sits on
    ///   **the region's own page grid** (for a page-aligned region base
    ///   that is the absolute 4 KiB grid as well);
    /// - `bytes` = the page's **valid bytes inside the region**
    ///   (`min(4096, region.size - page_offset)`) ⇒ the **last** page
    ///   of a region may be shorter (the payload never pads a page to a
    ///   page boundary — padding would silently change the region's
    ///   length on restore).
    ///
    /// The pages come out in ascending `base` order (the region table
    /// is a [`BTreeMap`], and so is the page table), and each page's
    /// `base` is the address of its **first** byte.
    pub fn export_pages(&self) -> Vec<MemPageDto> {
        let mut pages: Vec<MemPageDto> = Vec::new();
        for region in self.regions.values() {
            let RegionTarget::Ram(ram) = &region.target else {
                continue;
            };
            for (page_index, page) in &ram.pages {
                let offset = *page_index as usize * MEM_PAGE_BYTES;
                // The valid bytes of this page **inside the region**.
                let valid = (region.size as usize)
                    .saturating_sub(offset)
                    .min(MEM_PAGE_BYTES);
                // `region.base + offset` cannot overflow: the region's
                // `base + size` is checked to fit the 4 GiB bus.
                let base = region.base + offset as u32;
                pages.push(MemPageDto {
                    base,
                    bytes: page[..valid].to_vec(),
                });
            }
        }
        pages
    }

    /// Restore the **guest-visible memory pages** (RAM content) — segment
    /// 2's consumer (STAGE-3 §3.2).
    ///
    /// ⚠️ **`load_map` must run first** (STAGE-3 §3.7): a page is
    /// written *into* an existing RAM region, so without the topology
    /// there is nothing to write into — and the call **fails** with
    /// [`E_BUS_11_PAGE_OUTSIDE_RAM`] rather than silently dropping the
    /// content.
    ///
    /// A page must lie **entirely** inside one RAM region; a page that
    /// starts in a region but runs past its end is **rejected**, not
    /// clipped (clipping would restore a *different* memory image than
    /// the one that was captured).
    ///
    /// ⚠ **The page must sit on its region's page grid** (STAGE-11
    /// 裁决 C-3): `page.base % 4096 == region.base % 4096`. The grid is
    /// **region-relative** — the same grid [`Bus::export_pages`]
    /// produces — ⇒ a page the platform exported itself always loads
    /// back, even when the region's base is not 4 KiB aligned. (On the
    /// platform's real memory maps every base *is* aligned, so the rule
    /// degenerates to "the base is a multiple of 4096".)
    ///
    /// ## Validation runs before anything is written
    ///
    /// Every page is checked first; only then are the bytes written.
    /// A rejected page list leaves the RAM content **untouched** — a
    /// half-applied restore is worse than a rejected one (STAGE-3 §5).
    ///
    /// ⚠ **The restore is subject to the same budget as the guest**
    /// (裁决 C-3): the pages the payload would materialize are counted
    /// against the region's cap **before** anything is written ⇒ a
    /// payload that does not fit the host's budget is **rejected**
    /// (E_BUS-09) rather than half-applied.
    pub fn load_pages(&mut self, pages: &[MemPageDto]) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        // ---- Phase 1: resolve every page to a RAM region, touch nothing. ----
        let mut plan: Vec<(u32, usize)> = Vec::with_capacity(pages.len());
        // region base → the page count the region would have **after**
        // the restore (the existing pages plus the payload's).
        let mut projected: BTreeMap<u32, usize> = BTreeMap::new();
        for page in pages {
            let len = u32::try_from(page.bytes.len()).map_err(|_| {
                PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!("memory page at 0x{:08x} is too large", page.base),
                )
            })?;
            let end = page.base.checked_add(len).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!(
                        "memory page at 0x{:08x} ({} bytes) overflows the 32-bit address space",
                        page.base, len
                    ),
                )
            })?;
            let region_base = self
                .regions
                .iter()
                .find(|(_, r)| {
                    matches!(r.target, RegionTarget::Ram(_))
                        && r.base <= page.base
                        && end <= r.base.saturating_add(r.size)
                })
                .map(|(b, _)| *b)
                .ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_11_PAGE_OUTSIDE_RAM,
                        format!(
                            "memory page 0x{:08x}..0x{:08x} does not fit inside any RAM region \
                             (load the memory map first)",
                            page.base, end
                        ),
                    )
                })?;
            // ⚠ The page must sit on **its region's** page grid.
            if page.base % MEM_PAGE_BYTES as u32 != region_base % MEM_PAGE_BYTES as u32 {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!(
                        "memory page at 0x{:08x} is not on the page grid of RAM region 0x{:08x} \
                         (the page grid is region-relative, {MEM_PAGE_BYTES}-byte pages)",
                        page.base, region_base
                    ),
                ));
            }
            // The pages the payload would materialize in that region.
            let region = &self.regions[&region_base];
            let RegionTarget::Ram(ram) = &region.target else {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!("region 0x{:08x} is not a RAM region", region_base),
                ));
            };
            let offset = (page.base - region_base) as usize;
            let first = RamBackend::page_index(offset);
            let last = if len == 0 {
                first
            } else {
                RamBackend::page_index(offset + len as usize - 1)
            };
            let count = projected.entry(region_base).or_insert(ram.pages.len());
            for p in first..=last {
                if !ram.pages.contains_key(&p) {
                    *count += 1;
                }
            }
            let physical = (*count as u64) * (MEM_PAGE_BYTES as u64);
            if physical > cap {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_09_TOO_LARGE,
                    format!(
                        "restoring the memory pages would make RAM region 0x{:08x} physically \
                         occupy {physical} allocated bytes, over the per-region cap of {cap} bytes",
                        region_base
                    ),
                ));
            }
            plan.push((region_base, offset));
        }

        // ---- Phase 2: write the bytes. ----
        for (page, (region_base, offset)) in pages.iter().zip(plan) {
            let region = self.regions.get_mut(&region_base).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!("RAM region 0x{:08x} vanished mid-restore", region_base),
                )
            })?;
            let RegionTarget::Ram(ram) = &mut region.target else {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    E_BUS_11_PAGE_OUTSIDE_RAM,
                    format!("region 0x{:08x} is not a RAM region", region_base),
                ));
            };
            for (i, byte) in page.bytes.iter().enumerate() {
                let a = offset + i;
                let page_index = RamBackend::page_index(a);
                let slot = ram
                    .pages
                    .entry(page_index)
                    .or_insert_with(|| vec![0u8; MEM_PAGE_BYTES].into_boxed_slice());
                slot[a % MEM_PAGE_BYTES] = *byte;
            }
        }
        Ok(())
    }

    /// Install / replace the trace hook (M-18 source of bus accesses).
    /// **Replace** the bus-hook list with a single callback (or clear it).
    ///
    /// This is the original API and keeps its meaning: `set_hook(None)`
    /// removes every hook, `set_hook(Some(h))` leaves exactly one.
    ///
    /// Prefer [`Bus::add_hook`] when a consumer merely wants to *join*
    /// the observation stream — several independent consumers
    /// (M-17's watchpoint matcher, M-18's trace ring) need to see the
    /// same accesses, and the last `set_hook` would otherwise evict
    /// the others (D-46).
    pub fn set_hook(&mut self, hook: Option<BusHook>) {
        self.hooks.clear();
        if let Some(h) = hook {
            self.hooks.push(h);
        }
    }

    /// **Append** a bus-hook callback, keeping the ones already installed.
    ///
    /// The additive twin of [`Bus::set_hook`]: a consumer that only
    /// observes (M-18's trace ring) joins the stream instead of
    /// replacing it.
    pub fn add_hook(&mut self, hook: BusHook) {
        self.hooks.push(hook);
    }

    /// How many bus hooks are currently installed.
    ///
    /// A bus carries **several independent consumers**: M-17's
    /// watchpoint matcher and M-18's access-trace ring (D-46). The
    /// count is the cheapest way to assert that **both** are on the
    /// wire — a single-slot hook would silently starve one of them.
    pub fn hook_count(&self) -> usize {
        self.hooks.len()
    }

    /// The **live** peripheral instances on this bus, with their base
    /// addresses.
    ///
    /// The assembly layer (M-17 / `kernel_host`) needs this for two
    /// wirings that the bus itself does not own:
    ///
    /// - **D-46** — attaching a probe sink to each peripheral
    ///   ([`crate::m18_probe::ProbeRegistry::attach_signal`]) so the
    ///   waveform pipeline sees the peripheral's state changes;
    /// - **D-07** — ticking each peripheral from the scheduler.
    ///
    /// The handles are cloned `Rc`s, so the caller can hold on to them
    /// after the bus is moved into a backend.
    pub fn peripheral_instances(&self) -> Vec<(u32, Rc<RefCell<dyn Peripheral>>)> {
        self.regions
            .iter()
            .filter_map(|(base, region)| match &region.target {
                RegionTarget::PeripheralBound(bound) => Some((*base, Rc::clone(&bound.handle))),
                _ => None,
            })
            .collect()
    }

    /// **推进**总线上所有**已绑定实例**的外设时钟（D-46）。
    ///
    /// 外设的 `tick(now_ps)` 是它与虚拟时钟之间的唯一同步点：M-15 的
    /// GPIO 在 `tick` 里记下当前时间，之后它上报的探针边沿才带得上
    /// **真实**的时间戳（M-15 `gpio.rs` 的 `now_ps` 字段）。
    ///
    /// 没有这一步，边沿的时间戳会一直是 0 —— 波形看起来「有数据」，
    /// 时间轴却是平的，等于半个假象。
    ///
    /// ## 争用
    ///
    /// 某个外设正被总线派发路径占用（`try_borrow_mut` 失败）时，
    /// **跳过**这一次的 tick——它只是没赶上时钟，下一次 tick 会补上。
    /// 派发路径上的 panic 是不可恢复的，所以这里绝不 `borrow_mut`。
    ///
    /// ## 报错的外设不牵连别人
    ///
    /// 一个外设 `tick` 报错时，**其余外设仍然会被 tick**——否则它们会
    /// 错过时钟同步，探针边沿的时间戳从此对不齐（V-A 检视 · 🟡-4）。
    /// 第一个错误记在 [`PeripheralTickOutcome::error`] 里，**已经收
    /// 到的写一条都不丢**。
    ///
    /// ## 与 D-07 的关系
    ///
    /// 这是**最小**同步：谁调用、什么时候调用由上层决定。D-07 落地后，
    /// 外设的 tick 由 M-12 的调度器在**量子边界**统一驱动（那才是
    /// 架构上的正确粒度），本方法届时退化为调度器 tick 路径的实现细节。
    ///
    /// ⚠️ **已标 `#[deprecated]`**（台账 **D-61**，D-07 检视轮 V-B · ⚪-13）：
    /// D-07 把外设时钟的推进点**收敛**到 M-12 的 `Engine::tick_peripherals`
    /// 之后，本方法在**生产路径上已不可达**——全仓只剩 M-14 自己的单测在调它。
    /// 留着它是为了不破坏公开 API，但**留着就有被接回去的风险**——接回去就又是
    /// 「两个真相」（同一份外设时钟，两处各推一次）。
    #[deprecated(
        note = "D-07 之后外设时钟的唯一推进点是 M-12 的 Engine::tick_peripherals（走 PeripheralTicker 接缝）；本方法在生产路径上已不可达，保留仅为兼容旧的最小同步语义"
    )]
    pub fn tick_peripherals(&self, now_ps: u64) -> PeripheralTickOutcome {
        let mut out = PeripheralTickOutcome::default();
        for region in self.regions.values() {
            let RegionTarget::PeripheralBound(bound) = &region.target else {
                continue;
            };
            let Ok(mut periph) = bound.handle.try_borrow_mut() else {
                // 外设正忙（派发路径上）——跳过这次 tick。
                out.skipped += 1;
                continue;
            };
            match periph.tick(now_ps) {
                Ok(pending) => out.writes.extend(pending),
                Err(err) => {
                    // 记下**第一个**错误，但继续 tick 后面的外设：它们
                    // 不能因为别人的错误而错过时钟同步。
                    if out.error.is_none() {
                        out.error = Some(err);
                    }
                }
            }
        }
        out
    }

    /// Update the most recent virtual time the bus was told about.
    /// Read / write callers should call this first so the trace events
    /// carry the current `now_ps`.
    pub fn set_now_ps(&mut self, ts_ps: u64) {
        self.last_ts_ps = ts_ps;
    }

    /// Snapshot counters.
    pub fn stats(&self) -> BusStats {
        self.stats
    }

    /// The **physical bytes** the RAM regions of this bus occupy.
    ///
    /// ⚠ This **counter** is the only legal way to observe the physical
    /// occupancy (裁决 A): a read value cannot tell "never written" from
    /// "written with a zero", so the page count is the evidence.
    ///
    /// It is the sum over every RAM region of
    /// `materialized pages × 4096` — a page that was never written
    /// costs **nothing**.
    pub fn ram_allocated_bytes(&self) -> u64 {
        self.regions
            .values()
            .filter_map(|region| match &region.target {
                RegionTarget::Ram(ram) => Some(ram.allocated_bytes()),
                _ => None,
            })
            .sum()
    }

    /// Read at address, little-endian, width 8/16/32.
    ///
    /// Fires the [`BusHook`] **once per entry**, with `ok` reflecting
    /// the dispatch outcome. See [`BusAccess`] for the v0.6.1 review
    /// rationale on hook semantics (#1 / #2).
    pub fn read(&mut self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        self.stats.reads += 1;

        let result = self.read_inner(addr, width);
        let (ok, scenario) = match &result {
            Ok(_) => (true, None),
            Err(e) => {
                if matches!(e.family, BUS_FAMILY) && e.scenario == 1 {
                    self.stats.misses += 1;
                }
                (false, Some(e.scenario))
            }
        };
        self.fire_hook(addr, width, AccessDir::Read, ok, scenario, false);
        result
    }

    /// Read for **instruction fetch**.
    ///
    /// Identical to [`Bus::read`] — same value, same error, same
    /// statistics — except that the bus hook is told this access is a
    /// fetch ([`BusAccess::is_fetch`]). A watchpoint is about the guest's
    /// **data** accesses, so a fetch must never match one; without this
    /// distinction a read watchpoint on a code address fires on every
    /// instruction fetched from there (V-A 检视 · 🟠-2).
    pub fn read_fetch(&mut self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        self.stats.reads += 1;

        let result = self.read_inner(addr, width);
        let (ok, scenario) = match &result {
            Ok(_) => (true, None),
            Err(e) => {
                if matches!(e.family, BUS_FAMILY) && e.scenario == 1 {
                    self.stats.misses += 1;
                }
                (false, Some(e.scenario))
            }
        };
        self.fire_hook(addr, width, AccessDir::Read, ok, scenario, true);
        result
    }

    /// Read **one instruction**, honouring its **real** width.
    ///
    /// This is the **single** fetch entry point (STAGE-10 裁决 A). The
    /// width follows from the two low bits of the first half-word — the
    /// **same** rule M-11's `fetch_instruction_code` uses and the **same**
    /// rule the decoder uses (`0b11` ⇒ 32-bit, anything else ⇒ 16-bit
    /// compressed):
    ///
    /// * the low half-word says `0b11` ⇒ 32-bit instruction ⇒ the full
    ///   word is read;
    /// * anything else ⇒ 16-bit compressed instruction ⇒ **only the two
    ///   bytes that are really there** are read.
    ///
    /// ⚠️ **Why the width matters** (ledger **D-86**): a compressed
    /// instruction sitting in the **last two bytes** of a mapped region
    /// used to fault with `E_BUS-01` — the fetch read four bytes, two
    /// of which were past the end of the region ⇒ the panel could
    /// *show* the instruction (M-11 reads only what it needs) but the
    /// guest could not *run* it.
    ///
    /// ⚠️ **One fetch is one bus access**: the width probe is a **peek**
    /// — it counts neither as a read dispatch nor as a hook event — so
    /// `stats.reads`, the landing counters and the hook see **exactly
    /// one** access per fetched instruction, and the hook's `width` is
    /// the **instruction width** (not the probe's).
    ///
    /// ⚠️ The probe does **not** turn a failed fetch into a successful
    /// one: if the half-word itself is unmapped the fetch fails with
    /// `E_BUS-01` — the caller decides what that means (M-13 turns it
    /// into the architectural instruction access fault, `mcause = 1`).
    ///
    /// ⚠️ A 32-bit instruction is **physically** read as 2 + 4 = 6
    /// bytes (probe + word) but it stays **one** fetch, one event.
    pub fn read_fetch_instruction(&mut self, addr: u32) -> Result<u32, PlatformError> {
        self.stats.reads += 1;

        let (result, width) = self.fetch_instruction_measured(addr);
        let (ok, scenario) = match &result {
            Ok(_) => (true, None),
            Err(e) => {
                if matches!(e.family, BUS_FAMILY) && e.scenario == 1 {
                    self.stats.misses += 1;
                }
                (false, Some(e.scenario))
            }
        };
        self.fire_hook(addr, width, AccessDir::Read, ok, scenario, true);
        result
    }

    /// The fetch itself: probe the width, then read the instruction.
    ///
    /// Returns the instruction **and** the width it was read with. The
    /// width is reported even when the read failed: a fetch whose probe
    /// succeeded was a 32-bit instruction, and a fetch whose probe
    /// failed has an **unknown** width (reported as `Word`, the
    /// historical fetch width).
    fn fetch_instruction_measured(&mut self, addr: u32) -> (Result<u32, PlatformError>, BusWidth) {
        // ⚠️ The probe is a **peek**: `read_only` returns the same
        //    value and the same error as a counted read, but it touches
        //    neither the counters nor the hook.
        let half = match self.read_only(addr, BusWidth::Half) {
            Ok(h) => h,
            Err(e) => {
                // The probe itself hit an unmapped address. A counted
                // read would have bumped `unmapped_splits` for the same
                // situation ⇒ the peek keeps the counter honest.
                if matches!(e.family, BUS_FAMILY) && e.scenario == 1 {
                    self.stats.unmapped_splits += 1;
                }
                return (Err(e), BusWidth::Word);
            }
        };
        if half & 0b11 != 0b11 {
            // Compressed instruction: the two probed bytes **are** the
            // instruction ⇒ read nothing else. The landing is counted
            // here because the probe counted nothing.
            self.count_fetch_landing(addr);
            (Ok(half & 0xFFFF), BusWidth::Half)
        } else {
            // 32-bit instruction: read the full word. This is a
            // **counted** read ⇒ it bumps the landing counter (and the
            // split counters) exactly like the old four-byte fetch did.
            match self.read_inner(addr, BusWidth::Word) {
                Ok(word) => (Ok(word), BusWidth::Word),
                Err(e) => (Err(e), BusWidth::Word),
            }
        }
    }

    /// Count **one** landing for a fetch that was served by the width
    /// probe (the probe itself counts nothing).
    ///
    /// Without this, a compressed fetch would bump **no** landing
    /// counter at all — the probe is a peek — and the bus statistics
    /// would say "this instruction was never fetched".
    fn count_fetch_landing(&mut self, addr: u32) {
        let kind = self.lookup(addr).map(|r| r.kind);
        if let Some(kind) = kind {
            self.bump_read_hit(kind);
        }
    }

    /// Bulk-write a byte blob into the bus (image loading path).
    ///
    /// This is the **single write path** for image loading (M-06 LLD
    /// 06-1: 「写 RAM 由内核侧执行」——前端不直调 M-14). Every byte goes
    /// through the normal address decoder, so an image that runs past the
    /// end of a mapped region fails with `E_BUS-01` instead of silently
    /// corrupting whatever lives next to it.
    ///
    /// The writes **are** counted in the bus statistics and **do** fire
    /// the bus hook: loading an image is a write on the bus, and a
    /// consumer that watches the bus must see it. (Watchpoints are
    /// cleared before an image is loaded, so an image load cannot fire a
    /// watchpoint.)
    pub fn load_bytes(&mut self, addr: u32, bytes: &[u8]) -> Result<(), PlatformError> {
        for (i, byte) in bytes.iter().enumerate() {
            // `u32::try_from` rather than `i as u32`: the latter silently
            // **truncates** for a blob larger than 4 GiB (V-A 检视 · ⚪-3).
            let offset = u32::try_from(i).map_err(|_| {
                PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write offset overflow at 0x{:08x}", addr),
                )
            })?;
            let a = addr.checked_add(offset).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write address overflow at 0x{:08x}", addr),
                )
            })?;
            self.write(a, BusWidth::Byte, *byte as u32)?;
        }
        Ok(())
    }

    /// Read **without** touching the access statistics or the bus hook.
    ///
    /// The contract port
    /// [`ExecutionBackend::read_memory`](crate::riscv_sim_contract::port::ExecutionBackend::read_memory)
    /// takes `&self`, but [`Bus::read`] needs `&mut self` to bump the
    /// counters. This method is the read-only twin: it returns exactly
    /// the same value — or exactly the same error — as [`Bus::read`], but
    /// it neither counts the access nor fires the hook.
    ///
    /// That difference matters: a debugger peeking at a word must not
    /// show up in the bus statistics, nor in the M-18 trace, nor in a
    /// watchpoint match (a watchpoint is about the **guest's** accesses).
    pub fn read_only(&self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        match self.crosses_region(addr, width) {
            Crossing::Aligned => {
                let (base, off) = match self.lookup(addr) {
                    Some(r) => (r.base, (addr - r.base) as usize),
                    None => {
                        return Err(PlatformError::new(
                            BUS_FAMILY,
                            1,
                            format!("read unmapped addr=0x{:08x}", addr),
                        ));
                    }
                };
                let region = self.regions.get(&base).ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        1,
                        format!("read region vanished mid-flight base=0x{:08x}", base),
                    )
                })?;
                Self::read_region(region, off, width)
            }
            Crossing::Crosses => {
                let mut acc: u32 = 0;
                for i in 0..width.bytes() {
                    let a = addr.wrapping_add(i as u32);
                    let (base, off) = match self.lookup(a) {
                        Some(r) => (r.base, (a - r.base) as usize),
                        None => {
                            return Err(PlatformError::new(
                                BUS_FAMILY,
                                1,
                                format!("read unmapped byte addr=0x{:08x}", a),
                            ));
                        }
                    };
                    let region = self.regions.get(&base).ok_or_else(|| {
                        PlatformError::new(
                            BUS_FAMILY,
                            1,
                            format!("read region vanished base=0x{:08x}", base),
                        )
                    })?;
                    let b = Self::read_region_byte(region, off)?;
                    acc |= (b as u32) << (i * 8);
                }
                Ok(acc)
            }
            Crossing::Unmapped => Err(PlatformError::new(
                BUS_FAMILY,
                1,
                format!("read unmapped addr=0x{:08x}", addr),
            )),
        }
    }

    /// Inner read path. Either returns the full value or an error
    /// describing which slice failed. No stats / hook side-effects.
    fn read_inner(&mut self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        match self.crosses_region(addr, width) {
            Crossing::Aligned => self.read_aligned(addr, width),
            Crossing::Crosses => {
                self.stats.unaligned_splits += 1;
                self.read_split(addr, width)
            }
            Crossing::Unmapped => {
                self.stats.unmapped_splits += 1;
                Err(PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("read unmapped addr=0x{:08x}", addr),
                ))
            }
        }
    }

    fn read_aligned(&mut self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        // Capture `kind` from `lookup` so we can bump the right
        // counter after the `&Region` borrow ends.
        let (base, off, kind) = match self.lookup(addr) {
            Some(r) => (r.base, (addr - r.base) as usize, r.kind),
            None => {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("read unmapped addr=0x{:08x}", addr),
                ));
            }
        };
        let region = self.regions.get(&base).ok_or_else(|| {
            PlatformError::new(
                BUS_FAMILY,
                1,
                format!("read region vanished mid-flight base=0x{:08x}", base),
            )
        })?;
        let val = Self::read_region(region, off, width)?;
        self.bump_read_hit(kind);
        Ok(val)
    }

    fn read_split(&mut self, addr: u32, width: BusWidth) -> Result<u32, PlatformError> {
        let mut acc: u32 = 0;
        for i in 0..width.bytes() {
            let a = addr.wrapping_add(i as u32);
            let (base, off, kind) = match self.lookup(a) {
                Some(r) => (r.base, (a - r.base) as usize, r.kind),
                None => {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        1,
                        format!("read unmapped byte addr=0x{:08x}", a),
                    ));
                }
            };
            let region = self.regions.get(&base).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("read region vanished base=0x{:08x}", base),
                )
            })?;
            let b = Self::read_region_byte(region, off)?;
            acc |= (b as u32) << (i * 8);
            self.bump_read_hit(kind);
        }
        Ok(acc)
    }

    /// Write at address, little-endian, width 8/16/32. Two-phase for
    /// cross-region accesses (评审 14-1).
    pub fn write(&mut self, addr: u32, width: BusWidth, value: u32) -> Result<(), PlatformError> {
        self.stats.writes += 1;

        let result = self.write_inner(addr, width, value);
        let (ok, scenario) = match &result {
            Ok(()) => (true, None),
            Err(e) => {
                if matches!(e.family, BUS_FAMILY) && e.scenario == 1 {
                    self.stats.misses += 1;
                }
                (false, Some(e.scenario))
            }
        };
        self.fire_hook(addr, width, AccessDir::Write, ok, scenario, false);
        result
    }

    /// Inner write path.
    fn write_inner(&mut self, addr: u32, width: BusWidth, value: u32) -> Result<(), PlatformError> {
        match self.crosses_region(addr, width) {
            Crossing::Aligned => self.write_aligned(addr, width, value),
            Crossing::Crosses => {
                self.stats.unaligned_splits += 1;
                self.write_split(addr, width, value)
            }
            Crossing::Unmapped => {
                self.stats.unmapped_splits += 1;
                Err(PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write unmapped addr=0x{:08x}", addr),
                ))
            }
        }
    }

    fn write_aligned(
        &mut self,
        addr: u32,
        width: BusWidth,
        value: u32,
    ) -> Result<(), PlatformError> {
        let (base, off, kind) = match self.lookup(addr) {
            Some(r) => (r.base, (addr - r.base) as usize, r.kind),
            None => {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write unmapped addr=0x{:08x}", addr),
                ));
            }
        };
        // The cap is read **before** the mutable borrow of the region
        // table — it is the host's budget policy, not the region's.
        let cap = self.max_region_bytes;
        let region = self.regions.get_mut(&base).ok_or_else(|| {
            PlatformError::new(
                BUS_FAMILY,
                1,
                format!("write region vanished mid-flight base=0x{:08x}", base),
            )
        })?;
        Self::write_region(region, off, width, value, cap)?;
        // ROM write silently dropped (评审 14-4) → bump_write_hit does
        // nothing for ROM; per-byte writes also skip the bump.
        self.bump_write_hit(kind);
        Ok(())
    }

    fn write_split(&mut self, addr: u32, width: BusWidth, value: u32) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        // Phase 1: pre-check every byte slice before mutating anything.
        //
        // ⚠ The physical-occupancy cap is pre-checked here too: a
        // cross-region write that cannot fit the host's budget must
        // abort **before** the first byte lands — a half-applied write
        // must not leave a page behind (裁决 A).
        let mut projected: BTreeMap<u32, usize> = BTreeMap::new();
        for i in 0..width.bytes() {
            let a = addr.wrapping_add(i as u32);
            let region = self.lookup(a).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write unmapped byte addr=0x{:08x}", a),
                )
            })?;
            if matches!(region.target, RegionTarget::Rom(_)) {
                continue; // ROM slice is silently dropped.
            }
            if !matches!(
                region.target,
                RegionTarget::Ram(_)
                    | RegionTarget::Peripheral(_)
                    | RegionTarget::PeripheralBound(_)
            ) {
                return Err(PlatformError::new(
                    BUS_FAMILY,
                    5,
                    format!("region at addr=0x{:08x} cannot be written", a),
                ));
            }
            if let RegionTarget::Ram(ram) = &region.target {
                let off = (a - region.base) as usize;
                let page = RamBackend::page_index(off);
                let count = projected.entry(region.base).or_insert(ram.pages.len());
                if !ram.pages.contains_key(&page) {
                    *count += 1;
                }
                let physical = (*count as u64) * (MEM_PAGE_BYTES as u64);
                if physical > cap {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        E_BUS_09_TOO_LARGE,
                        format!(
                            "RAM region would physically occupy {physical} allocated bytes \
                             (materialized pages), over the per-region cap of {cap} bytes"
                        ),
                    ));
                }
            }
        }
        // Phase 2: commit byte-by-byte.
        for i in 0..width.bytes() {
            let a = addr.wrapping_add(i as u32);
            let (base, off, kind) = match self.lookup(a) {
                Some(r) => (r.base, (a - r.base) as usize, r.kind),
                None => {
                    return Err(PlatformError::new(
                        BUS_FAMILY,
                        1,
                        format!("write unmapped byte addr=0x{:08x} (post-phase-1)", a),
                    ));
                }
            };
            let region = self.regions.get_mut(&base).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    1,
                    format!("write region vanished base=0x{:08x}", base),
                )
            })?;
            let b = ((value >> (i * 8)) & 0xFF) as u8;
            Self::write_region_byte(region, off, b, cap)?;
            self.bump_write_hit(kind);
        }
        Ok(())
    }

    /// Increment the matching `*_hits` counter after a *successful*
    /// **read** land on `kind`. Reads on ROM count as `rom_hits`
    /// (the value comes back from the ROM bytes — silence only drops
    /// writes).
    fn bump_read_hit(&mut self, kind: RegionType) {
        match kind {
            RegionType::Ram => self.stats.ram_hits += 1,
            RegionType::Rom => self.stats.rom_hits += 1,
            RegionType::Peripheral => self.stats.periph_hits += 1,
        }
    }

    /// Increment the matching `*_hits` counter after a successful
    /// **write** land. Writes to ROM are silently dropped (评审 14-4)
    /// and therefore bump nothing.
    fn bump_write_hit(&mut self, kind: RegionType) {
        match kind {
            RegionType::Ram => self.stats.ram_hits += 1,
            RegionType::Rom => {}
            RegionType::Peripheral => self.stats.periph_hits += 1,
        }
    }

    // ----- region-local reads -----

    fn read_region(region: &Region, off: usize, width: BusWidth) -> Result<u32, PlatformError> {
        match &region.target {
            // ⚠ A RAM page that was never written reads back as **0**
            // — the read materializes nothing (裁决 A).
            RegionTarget::Ram(r) => r.read_at(region.size as usize, off, width),
            RegionTarget::Rom(r) => Self::read_bytes(r.bytes(), off, width),
            RegionTarget::Peripheral(id) => {
                // Id-only mapping: the bus holds no instance, so there
                // is nothing to dispatch to. Kept for the pre-D-12
                // call sites; real wiring uses `map_peripheral_instance`.
                Err(PlatformError::new(
                    BUS_FAMILY,
                    5,
                    format!(
                        "peripheral read not wired (id={:?}, off=0x{:x}, width={:?}) — \
                         use map_peripheral_instance to bind a live peripheral",
                        id, off, width
                    ),
                ))
            }
            RegionTarget::PeripheralBound(b) => {
                // D-14: reads go through `read_clear`, never `read`.
                // `Peripheral::read_clear` defaults to `read` for
                // non-destructive peripherals, so this is safe for
                // every peripheral — but it is the only way a
                // read-destructive register (UART RX FIFO) drains
                // correctly. Calling `read` here would silently
                // resurrect the "UART RX only receives 1 byte" defect
                // with no compile-time hint.
                let mut periph = b.try_write().ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        5,
                        format!(
                            "peripheral '{}' is busy (read)",
                            b.instance_id().unwrap_or_default()
                        ),
                    )
                })?;
                periph
                    .read_clear(off as u32, width.into())
                    .map_err(|e| Self::periph_error("read", e))
            }
        }
    }

    fn read_region_byte(region: &Region, off: usize) -> Result<u8, PlatformError> {
        match &region.target {
            RegionTarget::Ram(r) => r.read_byte_at(region.size as usize, off),
            RegionTarget::Rom(r) => r.bytes.get(off).copied().ok_or_else(|| {
                PlatformError::new(BUS_FAMILY, 4, format!("ROM read past end off=0x{:x}", off))
            }),
            RegionTarget::Peripheral(id) => Err(PlatformError::new(
                BUS_FAMILY,
                5,
                format!("peripheral byte read not wired (id={:?})", id),
            )),
            RegionTarget::PeripheralBound(b) => {
                // D-14: byte slices take the read-clear path too.
                let mut periph = b.try_write().ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        5,
                        format!(
                            "peripheral '{}' is busy (read)",
                            b.instance_id().unwrap_or_default()
                        ),
                    )
                })?;
                let v = periph
                    .read_clear(off as u32, WriteSize::Byte)
                    .map_err(|e| Self::periph_error("read", e))?;
                Ok((v & 0xFF) as u8)
            }
        }
    }

    // ----- region-local writes -----

    fn write_region(
        region: &mut Region,
        off: usize,
        width: BusWidth,
        value: u32,
        cap: u64,
    ) -> Result<(), PlatformError> {
        match &mut region.target {
            // ⚠ The **same** write-time-allocation path as the byte
            // write below (裁决 A): boundary → cap → materialize → write.
            RegionTarget::Ram(r) => r.write_at(region.size as usize, off, width, value, cap),
            RegionTarget::Rom(_) => {
                // ROM write is silently ignored (评审 14-4). Counter omitted
                // from stats by design.
                Ok(())
            }
            RegionTarget::Peripheral(id) => Err(PlatformError::new(
                BUS_FAMILY,
                5,
                format!("peripheral write not wired (id={:?})", id),
            )),
            RegionTarget::PeripheralBound(b) => {
                let mut periph = b.try_write().ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        5,
                        format!(
                            "peripheral '{}' is busy (write)",
                            b.instance_id().unwrap_or_default()
                        ),
                    )
                })?;
                periph
                    .write(off as u32, width.into(), value)
                    .map_err(|e| Self::periph_error("write", e))
            }
        }
    }

    fn write_region_byte(
        region: &mut Region,
        off: usize,
        value: u8,
        cap: u64,
    ) -> Result<(), PlatformError> {
        match &mut region.target {
            // ⚠ **The same write-time-allocation path** as the wide
            // write above — no "wide writes are lazy, byte writes are
            // eager" fork (裁决 A).
            RegionTarget::Ram(r) => r.write_at(
                region.size as usize,
                off,
                BusWidth::Byte,
                u32::from(value),
                cap,
            ),
            RegionTarget::Rom(_) => Ok(()),
            RegionTarget::Peripheral(id) => Err(PlatformError::new(
                BUS_FAMILY,
                5,
                format!("peripheral byte write not wired (id={:?})", id),
            )),
            RegionTarget::PeripheralBound(b) => {
                let mut periph = b.try_write().ok_or_else(|| {
                    PlatformError::new(
                        BUS_FAMILY,
                        5,
                        format!(
                            "peripheral '{}' is busy (write)",
                            b.instance_id().unwrap_or_default()
                        ),
                    )
                })?;
                periph
                    .write(off as u32, WriteSize::Byte, u32::from(value))
                    .map_err(|e| Self::periph_error("write", e))
            }
        }
    }

    /// Convert a peripheral-level error into the bus-level E_BUS-05
    /// (M-14 LLD §6.1: "外设抛错转 E_BUS-05 外设错误" — the bus
    /// error must stay distinguishable from the unmapped E_BUS-01).
    /// The original message is kept so the host can still see *why*
    /// the peripheral refused the access.
    fn periph_error(op: &str, err: PlatformError) -> PlatformError {
        PlatformError::new(BUS_FAMILY, 5, format!("peripheral {op} failed: {err}"))
    }

    // ----- byte / half / word primitive -----
    //
    // Review #11: these primitives now return `Result` and surface
    // E_BUS-04 on out-of-bounds, matching `read_region_byte` /
    // `write_region_byte`. Previously a silent `unwrap_or(0)` /
    // `if let Some(...)` could mask data corruption if dispatch
    // invariants ever loosened.

    /// Read `width` bytes out of a **flat** byte vector — the ROM path
    /// (a ROM region owns its content as one vector; it is not lazily
    /// allocated).
    fn read_bytes(bytes: &[u8], off: usize, width: BusWidth) -> Result<u32, PlatformError> {
        let mut acc: u32 = 0;
        for i in 0..width.bytes() {
            let b = *bytes.get(off + i).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    4,
                    format!("RAM/ROM read past end off=0x{:x} width={:?}", off, width),
                )
            })?;
            acc |= (b as u32) << (i * 8);
        }
        Ok(acc)
    }

    fn crosses_region(&self, addr: u32, width: BusWidth) -> Crossing {
        if width == BusWidth::Byte {
            return Crossing::Aligned;
        }
        // Review #12: `wrapping_add` would silently wrap for an
        // `addr` near `u32::MAX`. Catch the overflow and route to
        // `Unmapped` so the byte-slice pre-check surfaces E_BUS-01
        // instead of fabricating a false "crosses" classification.
        let last = match addr.checked_add((width.bytes() - 1) as u32) {
            Some(a) => a,
            None => return Crossing::Unmapped,
        };
        match (self.lookup(addr), self.lookup(last)) {
            (Some(a), Some(b)) if a.base != b.base => Crossing::Crosses,
            (Some(_), Some(_)) => Crossing::Aligned,
            _ => Crossing::Unmapped, // review #5: split from `Crosses`
        }
    }

    fn fire_hook(
        &mut self,
        addr: u32,
        width: BusWidth,
        dir: AccessDir,
        ok: bool,
        err_scenario: Option<u16>,
        is_fetch: bool,
    ) {
        if self.hooks.is_empty() {
            return;
        }
        let evt = BusAccess {
            ts_ps: self.last_ts_ps,
            addr,
            width,
            dir,
            ok,
            err_scenario,
            is_fetch,
        };
        for hook in self.hooks.iter_mut() {
            (hook)(&evt);
        }
    }
}

/// Outcome of [`Bus::crosses_region`]. Review #5: split `unaligned_splits`
/// from `unmapped_splits` by returning a three-valued classification
/// instead of `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Crossing {
    /// Single-region access (or byte-width): no split.
    Aligned,
    /// Both ends mapped but distinct regions: split per byte.
    Crosses,
    /// At least one end unmapped: byte-slice pre-check will surface
    /// E_BUS-01 for the bare slices.
    Unmapped,
}

// ---------------------------------------------------------------------------
// Tests cover (see `tests.rs`):
//   - map / unmap validation (E_BUS-03 / 04 / 06 / 07 / 09),
//   - lookup,
//   - read / write happy paths with 8/16/32 widths,
//   - ROM-write ignore,
//   - cross-region read & write split,
//   - full-unmapped reads (E_BUS-01),
//   - hook capture with `ok` / `err_scenario` flag (review #2),
//   - stats accounting split by RegionType (review #3),
//   - peripheral E_BUS-05 lock-down for the id-only mapping (review #13),
//   - peripheral **instance** dispatch: read → `read_clear`, write →
//     `write`, size mismatch → E_BUS-08, busy instance → E_BUS-05
//     (D-12 / D-14),
//   - per-region size cap (E_BUS-09, D-10),
//   - E_BUS-07 init-too-large independent of E_BUS-04 (review #7),
//   - BusMaster trait shape,
//   - **v0.26 验收清单 A-1 ~ A-12**（STAGE-11 裁决 A / B / C / D）：
//     页级懒分配（读没写过的页回 0 且零物化）、未登记地址**仍然**
//     E_BUS-01（不与「没写过」合成一层）、写一页 / 两页的物化计数、
//     护栏按**物理占用**量（RAM）而 ROM / 外设窗口按**声明大小**量、
//     4 KiB 页导出（只装物化过的页）、两阶段还原、字段可见性的
//     **源码级**守卫、以及统计口径逐条不变。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
