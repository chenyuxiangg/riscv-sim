//! M-14 总线与内存映射 (Bus & Memory Map).
//!
//! Owns the address decoder (regionTree → region), the RAM backend,
//! and the read / write dispatcher that splits cross-region writes into
//! a two-phase pre-check + commit (评审 14-1).
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
//!   E_BUS-09 region over the cap → PlatformError{Bus, 9}  (D-10)
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
use riscv_sim_contract::model::WriteSize;
use riscv_sim_contract::port::Peripheral;

/// 4 GiB — covers the entire RV32 physical address space.
pub const BUS_SIZE: u64 = 0x1_0000_0000;

/// Default upper bound on a single region's size, in bytes (D-10).
///
/// A malformed firmware image can declare a segment of ~4 GiB; without a
/// cap, `map_ram` would happily `vec![0u8; size]` and OOM the host
/// (in the browser: OOM the tab). 16 MiB matches the order of magnitude
/// of M-06's `ParseLimits::max_region_bytes` default — M-14 does not
/// depend on `m06_elf`, so the constant is defined locally (the two
/// sides are independent; see `docs/reviews/REVIEWS.md` D-10).
pub const DEFAULT_MAX_REGION_BYTES: u64 = 16 * 1024 * 1024;

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
#[derive(Debug)]
pub struct Region {
    /// Interval base address (`uint32_t` — see M-14 §2.1.1).
    pub base: u32,
    /// Interval size in bytes (must be > 0).
    pub size: u32,
    /// Region kind.
    pub kind: RegionType,
    /// Region-local backing store, or peripheral token.
    pub target: RegionTarget,
}

/// Region-local backing store chosen by `kind`.
///
/// `Ram` and `Rom` both carry an owned `Vec<u8>` — `Rom` simply ignores
/// writes. `Peripheral` regions carry either a [`PeripheralId`] (legacy
/// id-only mapping — nothing to dispatch to, E_BUS-05) or a
/// [`BoundPeripheral`] (live instance, D-12).
#[derive(Debug)]
pub enum RegionTarget {
    Ram(RamBackend),
    Rom(RomBackend),
    Peripheral(PeripheralId),
    /// A peripheral region bound to a live instance (D-12 wiring).
    PeripheralBound(BoundPeripheral),
}

/// RAM backing store. Bounds-checked on every read / write.
#[derive(Debug)]
pub struct RamBackend {
    /// Backing bytes (length = `region.size`).
    pub bytes: Vec<u8>,
    /// Bound base address (cached for diagnostics).
    pub base_addr: u32,
}

/// ROM backing store. Identical to RAM but flagged read-only at the
/// dispatch site.
#[derive(Debug)]
pub struct RomBackend {
    /// Initial contents.
    pub bytes: Vec<u8>,
    pub base_addr: u32,
}

impl Region {
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
/// Note: this MVP uses a single big [`Region`] per base, not a
/// page-granular table (P1 reservation, M-14 §1.3). The 4-KiB page
/// design will collapse naturally into this map when needed.
pub struct Bus {
    regions: BTreeMap<u32, Region>,
    hook: Option<BusHook>,
    last_ts_ps: u64,
    stats: BusStats,
    /// Upper bound on a single region's size, in bytes (D-10).
    ///
    /// Checked **before** any backing store is allocated, so a malformed
    /// firmware segment can never make the host allocate GiB-scale
    /// memory. See [`DEFAULT_MAX_REGION_BYTES`].
    max_region_bytes: u64,
    /// Next bus-side peripheral token handed out by
    /// [`Bus::map_peripheral_instance`].
    next_periph_id: u32,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            regions: BTreeMap::new(),
            hook: None,
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
            .field("hook.installed", &self.hook.is_some())
            .field("max_region_bytes", &self.max_region_bytes)
            .finish()
    }
}

impl Bus {
    /// Build an empty bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build an empty bus with a custom per-region size cap (D-10).
    ///
    /// `max_region_bytes` is a plain upper bound: `0` means *no region
    /// fits at all*, `u64::MAX` means *effectively unlimited*. The
    /// default ([`DEFAULT_MAX_REGION_BYTES`]) is the safe one — use
    /// this constructor only when the host has its own budget policy.
    pub fn with_max_region_bytes(max_region_bytes: u64) -> Self {
        Self {
            max_region_bytes,
            ..Self::default()
        }
    }

    /// Current per-region size cap, in bytes (D-10).
    pub fn max_region_bytes(&self) -> u64 {
        self.max_region_bytes
    }

    /// Adjust the per-region size cap at runtime (D-10).
    ///
    /// The cap is enforced on the *next* mapping operation; regions
    /// that are already mapped are untouched.
    pub fn set_max_region_bytes(&mut self, max_region_bytes: u64) {
        self.max_region_bytes = max_region_bytes;
    }

    /// Map a RAM region into the bus.
    pub fn map_ram(&mut self, base: u32, size: u32, init: &[u8]) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Ram,
            target: RegionTarget::Ram(RamBackend {
                bytes: Self::prepare_bytes(base, size, init, cap)?,
                base_addr: base,
            }),
        })
    }

    /// Map a ROM region. Writes will be silently dropped.
    pub fn map_rom(&mut self, base: u32, size: u32, init: &[u8]) -> Result<(), PlatformError> {
        let cap = self.max_region_bytes;
        self.map_region(Region {
            base,
            size,
            kind: RegionType::Rom,
            target: RegionTarget::Rom(RomBackend {
                bytes: Self::prepare_bytes(base, size, init, cap)?,
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
    pub fn map_peripheral(
        &mut self,
        base: u32,
        size: u32,
        id: PeripheralId,
    ) -> Result<(), PlatformError> {
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

    /// Allocate the backing byte vector and validate `init` length.
    ///
    /// **D-10:** the size cap is checked *before* the allocation, so a
    /// malformed firmware segment can never make the host reserve GiB
    /// of memory. `max_region_bytes` is the bus-wide cap
    /// ([`Bus::max_region_bytes`]).
    fn prepare_bytes(
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

    /// Atomic region insert: validate `size > 0`, overflow, the D-10
    /// size cap, and overlap against current regions, then insert. All
    /// failures preserve the existing region set (LLD §3.3).
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
        // D-10: the cap is enforced on *every* mapping path, not only
        // the ones that allocate a backing store — a peripheral window
        // claims an address interval just the same.
        if u64::from(region.size) > self.max_region_bytes {
            return Err(PlatformError::new(
                BUS_FAMILY,
                9,
                format!(
                    "region size {} exceeds the per-region cap of {} bytes (base=0x{:08x})",
                    region.size, self.max_region_bytes, region.base
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

    /// Install / replace the trace hook (M-18 source of bus accesses).
    pub fn set_hook(&mut self, hook: Option<BusHook>) {
        self.hook = hook;
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
        let region = self.regions.get_mut(&base).ok_or_else(|| {
            PlatformError::new(
                BUS_FAMILY,
                1,
                format!("write region vanished mid-flight base=0x{:08x}", base),
            )
        })?;
        Self::write_region(region, off, width, value)?;
        // ROM write silently dropped (评审 14-4) → bump_write_hit does
        // nothing for ROM; per-byte writes also skip the bump.
        self.bump_write_hit(kind);
        Ok(())
    }

    fn write_split(&mut self, addr: u32, width: BusWidth, value: u32) -> Result<(), PlatformError> {
        // Phase 1: pre-check every byte slice before mutating anything.
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
            Self::write_region_byte(region, off, b)?;
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
            RegionTarget::Ram(r) => Self::read_bytes(&r.bytes, off, width),
            RegionTarget::Rom(r) => Self::read_bytes(&r.bytes, off, width),
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
            RegionTarget::Ram(r) => r.bytes.get(off).copied().ok_or_else(|| {
                PlatformError::new(BUS_FAMILY, 4, format!("RAM read past end off=0x{:x}", off))
            }),
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
    ) -> Result<(), PlatformError> {
        match &mut region.target {
            RegionTarget::Ram(r) => Self::write_bytes(&mut r.bytes, off, width, value),
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

    fn write_region_byte(region: &mut Region, off: usize, value: u8) -> Result<(), PlatformError> {
        match &mut region.target {
            RegionTarget::Ram(r) => {
                if let Some(slot) = r.bytes.get_mut(off) {
                    *slot = value;
                    return Ok(());
                }
                Err(PlatformError::new(
                    BUS_FAMILY,
                    4,
                    format!("RAM byte write past end off=0x{:x}", off),
                ))
            }
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

    fn write_bytes(
        bytes: &mut [u8],
        off: usize,
        width: BusWidth,
        value: u32,
    ) -> Result<(), PlatformError> {
        for i in 0..width.bytes() {
            let slot = bytes.get_mut(off + i).ok_or_else(|| {
                PlatformError::new(
                    BUS_FAMILY,
                    4,
                    format!("RAM byte write past end off=0x{:x} width={:?}", off, width),
                )
            })?;
            *slot = ((value >> (i * 8)) & 0xFF) as u8;
        }
        Ok(())
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
        if let Some(hook) = self.hook.as_mut() {
            let evt = BusAccess {
                ts_ps: self.last_ts_ps,
                addr,
                width,
                dir,
                ok,
                err_scenario,
                is_fetch,
            };
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
//   - BusMaster trait shape.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
