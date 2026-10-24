//! Platform description — the **declared** memory map (v0.27 · D-123-B).
//!
//! # What this is
//!
//! A **platform description** is the data form of "what memory map does
//! this board have": which regions exist, where they start, how big they
//! are, and which peripheral device sits at which base address. It is
//! the input that lets the host build a memory map **without hard-coding
//! one** — before this module, the host had two profiles (`Default` /
//! `QemuVirt`) whose addresses and device constructions were literals in
//! the host's source code, so "use a different board" meant "edit the
//! host".
//!
//! The frozen spec is
//! `docs/detailed-design/STAGE-12_V027_Platform_Description.md` §1 (the
//! format) and §2.1 (this file's landing spot).
//!
//! # Where the rules live
//!
//! **All** of the description's validation rules live in
//! [`PlatformDescription::validate`] — one constraint, one landing spot.
//! The host must not write a second validator, and neither must the
//! front end when it starts mirroring this shape.
//!
//! # Three layers, three sets of codes
//!
//! | layer | what it judges | codes |
//! | --- | --- | --- |
//! | **serde** | the *shape* — unknown device kind, missing parameter, wrong parameter type | serde's own error (V-8) |
//! | **this module** | the *declaration* — version, id, regions, sizes, overlaps, budget, duplicate device id, device parameters' legal value domain | `E_PLT-01`..`E_PLT-08` · `E_PLT-12` |
//! | **the host** | the *lookup* — unknown platform id, unreadable file, malformed file | `E_PLT-09`..`E_PLT-11` (owned **here**, judged by the host) |
//! | **M-14** | the *live bus* — unmapped access, region over the runtime budget | `E_BUS-*` |
//!
//! ⚠️ The two budget rules are **not** the same constraint and must not
//! be merged: [`E_PLT_07_OVER_BUDGET`] judges the **declared** size
//! (before anything is mapped), M-14's `E_BUS-09` judges the **physical
//! occupancy** (after mapping — RAM is lazily allocated since v0.26, so
//! a region may declare a large size and cost nothing).
//!
//! ⚠️ **The host-side codes live here too** (V-10 .. V-12): the *rules*
//! are the host's, but a code with **two** landing spots is a code with
//! two meanings ⇒ the contract layer owns every `E_PLT-*` number.

use serde::{Deserialize, Serialize};

use crate::error::{
    PlatformError, E_PLT_01_SCHEMA_VERSION, E_PLT_02_EMPTY_ID, E_PLT_03_NO_REGIONS,
    E_PLT_04_ZERO_SIZE, E_PLT_05_OVERFLOW, E_PLT_06_OVERLAP, E_PLT_07_OVER_BUDGET,
    E_PLT_08_DUPLICATE_DEVICE_ID, E_PLT_12_PARAMETER_OUT_OF_RANGE, PLATFORM_FAMILY,
};
use crate::session::{REGION_KIND_PERIPHERAL, REGION_KIND_RAM, REGION_KIND_ROM};

/// Format version of the **platform description**.
///
/// ⚠️ **This is not the session payload's version** and it is not the
/// contract's [`crate::SCHEMA_VERSION`] either: a platform description
/// is **not** a session payload — it never travels inside
/// [`crate::session::SessionSnapshotDto`] — so it carries its **own**
/// version number and the two never move together.
///
/// ⚠️ The description's format version is also **not** the contract
/// schema version: the contract version governs the **message shape**
/// (what the front end mirrors), the description version governs the
/// **memory-map description shape** (what the host reads). Adding this
/// module therefore requires **no** front-end change at all.
pub const PLATFORM_SCHEMA_VERSION: u32 = 1;

/// The **upper bound** of a CLINT's timer frequency (1 THz).
///
/// ⚠️ The platform's smallest time granularity is **1 ps**
/// (`PS_PER_SEC = 1e12`) ⇒ a frequency above 1 THz means a period
/// **shorter than one picosecond** ⇒ the platform cannot express it (the
/// device model's `ps_per_cycle()` would compute `0`).
///
/// ⚠️ This is the **only** place the number lives: M-15 / the host
/// **reference** it, they never copy it (one constant, one truth — the
/// same discipline as v0.22's `MSTATUS_WARL_MASK`).
pub const MAX_CLINT_FREQ_HZ: u64 = 1_000_000_000_000;

/// The **upper bound** of the number of harts a CLINT serves (4096).
///
/// ⚠️ From the device model's register layout (not from taste): the
/// `msip` area is `0x0 + 4×hart` and must not run into the `mtimecmp`
/// area at `0x4000` ⇒ `hart < 4096`; the `mtimecmp` area
/// (`0x4000 + 8×hart`) must not run into the `mtime` area at `0xBFF8` ⇒
/// the same bound, from another road.
pub const MAX_CLINT_HARTS: u32 = 4096;

/// The **upper bound** of the number of interrupt sources a PLIC has
/// (1024).
///
/// ⚠️ From the device model's register layout: the Priority area is
/// `0x0 + 4×source` and must not run into the Pending area at `0x1000`
/// ⇒ `source < 1024`; the Enable area gives the same bound from another
/// road (32 words per context ⇒ `ceil(sources/32) ≤ 32`).
pub const MAX_PLIC_SOURCES: u32 = 1024;

/// The **upper bound** of the number of interrupt contexts a PLIC has
/// (16).
///
/// ⚠️ From the device model's register layout: the Threshold / Claim area
/// is `0x200000 + 0x1000×ctx` and must stay inside the `0x210000` window
/// ⇒ `ctx ≤ 16`.
pub const MAX_PLIC_CONTEXTS: u32 = 16;

/// The default value of [`PlatformDescription::auto_selectable`].
///
/// ⚠️ **Absent ⇒ `true`**: a description that says nothing about
/// auto-selection **does** take part in it ⇒ a hand-written description
/// behaves like a normal platform, not like a demo one.
fn default_true() -> bool {
    true
}

/// A platform description — the declared memory map of one board.
///
/// The wire form is JSON (the only serialization format the repository
/// already uses — see the frozen spec §1.1), e.g.:
///
/// ```json
/// {
///   "schema_version": 1,
///   "id": "qemu-virt",
///   "name": "QEMU virt",
///   "priority": 20,
///   "auto_selectable": true,
///   "region_budget_bytes": 134217728,
///   "regions": [
///     { "kind": "ram", "base": 0, "size": 1048576 },
///     { "kind": "peripheral", "base": 1073741824,
///       "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } }
///   ]
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformDescription {
    /// Format version of this description — must be
    /// [`PLATFORM_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Platform identifier — the platform's **primary key** (built-in
    /// descriptions are selected by it). Must not be empty.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Auto-selection order — **smaller wins** (the frozen spec §3 裁决 E:
    /// the order must not depend on registration order or on capacity).
    pub priority: u32,
    /// Does this description take part in **auto-selection**?
    ///
    /// ⚠️ **This is where the "only the high segment is looked at"
    /// difference lives** (the frozen spec §3 裁决 E). The auto-selection
    /// judgement asks a description for **all** of its RAM regions, so a
    /// description whose **low** segment is bigger than the other
    /// platforms' would win images those platforms cannot hold ⇒ the
    /// demo platform sets this to `false` ⇒ it can only be named
    /// **explicitly**.
    ///
    /// ⚠️ **Default `true`**: absent from the JSON means "a normal
    /// platform" ⇒ a hand-written description participates unless it says
    /// otherwise.
    #[serde(default = "default_true")]
    pub auto_selectable: bool,
    /// Per-region budget in bytes — the ceiling for one region's
    /// **declared** size.
    ///
    /// ⚠️ This is a **declaration-time** budget. The runtime budget
    /// (M-14's `E_BUS-09`) measures the **physical occupancy** instead
    /// (RAM is lazily allocated) ⇒ two constraints, two layers, two
    /// codes.
    pub region_budget_bytes: u32,
    /// The declared regions — may be listed in any order (the overlap
    /// check sorts by `base` first).
    pub regions: Vec<RegionSpec>,
    /// Human-readable note — the compensation for JSON's inability to
    /// carry comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One declared region — internally tagged by **`kind`**.
///
/// ⚠️ The internal tag's key is literally `kind`, matching the region
/// kind vocabulary of [`crate::session::RegionDto::kind`] — so a
/// description needs **no** translation when it is compared with an
/// exported memory map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegionSpec {
    /// Ordinary memory (lazily allocated since v0.26).
    Ram {
        /// Interval base address.
        base: u32,
        /// Interval size in bytes.
        size: u32,
    },
    /// Read-only memory (writes are silently dropped).
    Rom {
        /// Interval base address.
        base: u32,
        /// Interval size in bytes.
        size: u32,
    },
    /// A peripheral window.
    ///
    /// ⚠️ **There is deliberately no `size` field here**: the window size
    /// is the device's own register map (`Peripheral::size()` in the
    /// contract layer's `port` module), so writing it into the
    /// description would create a **second source of truth** that goes
    /// stale the moment a device's register map changes.
    Peripheral {
        /// Interval base address.
        base: u32,
        /// The device sitting at `base`.
        device: DeviceSpec,
    },
}

/// One declared peripheral device — internally tagged by **`device`**.
///
/// ⚠️ The internal tag's key is literally `device` (**not** `kind` —
/// that one is already the region's tag, and **not** `type`): the wire
/// form is `{"device": "uart16550", "id": "UART0", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "device", rename_all = "snake_case")]
pub enum DeviceSpec {
    /// A GPIO bank.
    Gpio {
        /// The device id — the key the restore path matches on.
        id: String,
    },
    /// An ns16550 UART.
    Uart16550 {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The interrupt line this UART raises.
        irq_line: u16,
        /// The nominal baud rate.
        baud_rate: u32,
    },
    /// A CLINT (timer / software interrupt).
    Clint {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The timer frequency in Hz.
        freq_hz: u64,
        /// The number of harts this CLINT serves.
        harts: u32,
    },
    /// A PLIC (interrupt controller).
    Plic {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The number of interrupt sources.
        sources: u32,
        /// The number of interrupt contexts.
        contexts: u32,
    },
}

impl PlatformDescription {
    /// Validate this description against the frozen rules.
    ///
    /// The rules, in the order they are checked (the first violated one
    /// is the one reported):
    ///
    /// | rule | judgement | code |
    /// | --- | --- | --- |
    /// | V-1 | `schema_version != PLATFORM_SCHEMA_VERSION` | `E_PLT-01` |
    /// | V-2 | `id` is empty | `E_PLT-02` |
    /// | V-3 | `regions` is empty | `E_PLT-03` |
    /// | V-4 | a RAM / ROM region's `size == 0` | `E_PLT-04` |
    /// | V-5 | `base + size` overflows `u32` | `E_PLT-05` |
    /// | V-6 | two regions overlap | `E_PLT-06` |
    /// | V-7 | a RAM / ROM region's **declared** size `> region_budget_bytes` | `E_PLT-07` |
    /// | V-8 | unknown device kind / missing parameter / wrong parameter type | **serde** (not here) |
    /// | V-9 | two peripheral devices share an `id` | `E_PLT-08` |
    /// | V-13 | a device parameter is **below** its legal lower bound | `E_PLT-12` |
    /// | V-14 | a device parameter is **above** its legal upper bound | `E_PLT-12` |
    /// | V-15 | a UART's `irq_line` is not a valid PLIC source id (only when the map has a PLIC) | `E_PLT-12` |
    ///
    /// ⚠️ **V-8 lives in the serde layer, not here.** An unknown device
    /// kind or a missing parameter fails during deserialization, before
    /// any value exists to validate ⇒ this method never sees it. That is
    /// why the **declaration** rules carry **eight** scenario codes and
    /// not nine.
    ///
    /// ⚠️ **V-10 ~ V-12 are not judged here either** — they are the
    /// *host-side* lookup rules (unknown platform id, unreadable file,
    /// malformed file). Their codes are owned by the contract layer
    /// (`E_PLT-09` .. `E_PLT-11`) so that a code has exactly one landing
    /// spot, but the **host** is the one that detects them.
    ///
    /// ⚠️ **V-13 ~ V-15 were appended after V-9** (v0.30 · D-141) ⇒ the
    /// report order of V-1 ~ V-9 is **unchanged, byte for byte** ⇒ no
    /// existing judgement moves. They **share one** scenario code
    /// (`E_PLT-12`): a single field out of range and a broken cross-field
    /// relation mean the same thing — 「某个器件参数的值不在它的合法取值域内」
    /// ⇒ one code, not two.
    ///
    /// ⚠️ **Judgements never look at the message text** — they assert
    /// `family == Platform && scenario == <code>`. Rewording a message
    /// must not turn a test red, and a message must never carry the
    /// meaning.
    ///
    /// ⚠️ **V-7 is a declaration-time check and is deliberately not M-14's
    /// `E_BUS-09`**: v0.26 made RAM lazily allocated, so the runtime
    /// guard measures the **physical occupancy** and no longer catches a
    /// huge *declaration* ⇒ the description layer is the only place left
    /// that can ⇒ it must. Two layers, two semantics, two codes.
    pub fn validate(&self) -> Result<(), PlatformError> {
        // V-1 — the format version this build speaks.
        if self.schema_version != PLATFORM_SCHEMA_VERSION {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_01_SCHEMA_VERSION,
                format!(
                    "平台描述的格式版本不受支持：{} != {}",
                    self.schema_version, PLATFORM_SCHEMA_VERSION
                ),
            ));
        }

        // V-2 — the platform's primary key must name something.
        if self.id.is_empty() {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_02_EMPTY_ID,
                "平台描述的 id 不能为空",
            ));
        }

        // V-3 — no region at all is no memory map.
        if self.regions.is_empty() {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_03_NO_REGIONS,
                "平台描述没有声明任何区域",
            ));
        }

        // V-4 + V-5 — per region, in the author's order.
        //
        // ⚠️ Only RAM / ROM have a **declared** size here; a peripheral
        // window's size is the device's own register map (裁决 D), so
        // there is nothing to check for it at this layer.
        for region in &self.regions {
            let Some(size) = region.declared_size() else {
                continue;
            };
            if size == 0 {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_04_ZERO_SIZE,
                    format!("区域 {:#x} 的大小为 0", region.base()),
                ));
            }
            if region.base().checked_add(size).is_none() {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_05_OVERFLOW,
                    format!(
                        "区域 {:#x} + {:#x} 越过了 u32 的地址空间",
                        region.base(),
                        size
                    ),
                ));
            }
        }

        // V-6 — overlap.
        //
        // ⚠️ **Sorted by `base` first** (the description may list the
        // regions in any order) and then compared pair by pair. The
        // description holds a handful of regions, so the quadratic scan
        // costs nothing and needs no "adjacent pairs are enough"
        // argument to be obviously complete.
        let mut sorted: Vec<&RegionSpec> = self.regions.iter().collect();
        sorted.sort_by_key(|region| region.base());
        for i in 0..sorted.len() {
            for j in (i + 1)..sorted.len() {
                if regions_overlap(sorted[i], sorted[j]) {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_06_OVERLAP,
                        format!(
                            "区域 {:#x} 与区域 {:#x} 重叠",
                            sorted[i].base(),
                            sorted[j].base()
                        ),
                    ));
                }
            }
        }

        // V-7 — the **declared** size against the description's budget.
        //
        // ⚠️ `>` and not `>=`: a region whose size is **exactly** the
        // budget is allowed (the built-in `qemu-virt` description declares
        // a 128 MiB RAM against a 128 MiB budget) — the same wording as
        // M-14's runtime guard.
        for region in &self.regions {
            let Some(size) = region.declared_size() else {
                continue;
            };
            if size > self.region_budget_bytes {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_07_OVER_BUDGET,
                    format!(
                        "区域 {:#x} 声明的大小 {:#x} 超过了每区域预算 {:#x}",
                        region.base(),
                        size,
                        self.region_budget_bytes
                    ),
                ));
            }
        }

        // V-9 — two devices with one id.
        //
        // ⚠️ The restore path matches a device state onto a live instance
        // **by id** ⇒ two entries with one id are two states competing
        // for one instance. The check spans **all** device kinds (a UART
        // and a GPIO may not share an id either) — same rule as the
        // session payload's own duplicate-id check.
        let mut seen: Vec<&str> = Vec::new();
        for region in &self.regions {
            let Some(id) = region.device_id() else {
                continue;
            };
            if seen.contains(&id) {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_08_DUPLICATE_DEVICE_ID,
                    format!("外设器件的 id 重复：{id}"),
                ));
            }
            seen.push(id);
        }

        // V-13 / V-14 / V-15 — the devices' **legal value domain**
        // (v0.30 · D-141 · frozen spec STAGE-15 §3.1).
        //
        // ⚠️ **Appended after V-9** ⇒ the report order of V-1 ~ V-9 is
        // unchanged, byte for byte (the frozen spec's acceptance A-9).
        //
        // ⚠️ **All three rules share one scenario code** (`E_PLT-12`): a
        // single field out of range and a broken cross-field relation
        // mean the same thing — 「某个器件参数的值不在它的合法取值域内」
        // ⇒ one code, not two.
        //
        // ⚠️ The description layer **rejects as written** — it does not
        // "helpfully" correct the value (the device models clamp
        // `harts` / `freq_hz` / `sources` / `contexts` to at least 1, which
        // would silently hide the mistake ⇒ the model layer's clamping is
        // only a fallback for code paths that bypass the description).

        // V-13 — the **lower** bounds (one field at a time).
        for region in &self.regions {
            let RegionSpec::Peripheral { device, .. } = region else {
                continue;
            };
            for (field, value, lower, _) in device.numeric_parameters() {
                if value < lower {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（{kind}）的参数 {field} = {value} 小于合法下界 {lower}",
                            id = device.id(),
                            kind = device.kind(),
                        ),
                    ));
                }
            }
        }

        // V-14 — the **upper** bounds (one field at a time).
        //
        // ⚠️ Only the fields that have an upper bound at all take part:
        // `uart16550.baud_rate` has **none** (the RISC-V architecture does
        // not cap the baud rate) ⇒ **no number is invented** for it.
        for region in &self.regions {
            let RegionSpec::Peripheral { device, .. } = region else {
                continue;
            };
            for (field, value, _, upper) in device.numeric_parameters() {
                let Some(upper) = upper else {
                    continue;
                };
                if value > upper {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（{kind}）的参数 {field} = {value} 超过合法上界 {upper}",
                            id = device.id(),
                            kind = device.kind(),
                        ),
                    ));
                }
            }
        }

        // V-15 — the **cross-field** rule: a UART's `irq_line` must be a
        // valid PLIC source id.
        //
        // ⚠️ **Judged only when the map has a PLIC** (裁决 G): no PLIC does
        // **not** make the board illegal — it just has no interrupt
        // routing ⇒ `irq_line` is meaningless but harmless ⇒ refusing such
        // a board would refuse a legal one.
        //
        // ⚠️ The bound is the **smallest** `sources` in the map ⇒ every
        // PLIC in the map accepts the line ⇒ conservative and unambiguous.
        let plic_sources = self.regions.iter().filter_map(|region| {
            let RegionSpec::Peripheral {
                device: DeviceSpec::Plic { sources, .. },
                ..
            } = region
            else {
                return None;
            };
            Some(*sources)
        });
        if let Some(min_sources) = plic_sources.min() {
            for region in &self.regions {
                let RegionSpec::Peripheral {
                    device: DeviceSpec::Uart16550 { id, irq_line, .. },
                    ..
                } = region
                else {
                    continue;
                };
                if u32::from(*irq_line) >= min_sources {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（uart16550）的参数 irq_line = {irq_line} 不在 PLIC 的中断源数之内（图中最小的 sources = {min_sources}）"
                        ),
                    ));
                }
            }
        }

        Ok(())
    }
}

impl RegionSpec {
    /// The region's base address.
    pub fn base(&self) -> u32 {
        match self {
            RegionSpec::Ram { base, .. }
            | RegionSpec::Rom { base, .. }
            | RegionSpec::Peripheral { base, .. } => *base,
        }
    }

    /// The region's **declared** size in bytes — `None` for a peripheral
    /// window, whose size belongs to the device's register map (裁决 D).
    pub fn declared_size(&self) -> Option<u32> {
        match self {
            RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => Some(*size),
            RegionSpec::Peripheral { .. } => None,
        }
    }

    /// The region kind — one of the three vocabulary strings of
    /// [`crate::session`] (`"ram"` / `"rom"` / `"peripheral"`).
    pub fn kind(&self) -> &'static str {
        match self {
            RegionSpec::Ram { .. } => REGION_KIND_RAM,
            RegionSpec::Rom { .. } => REGION_KIND_ROM,
            RegionSpec::Peripheral { .. } => REGION_KIND_PERIPHERAL,
        }
    }

    /// The peripheral device's id — `None` for a RAM / ROM region.
    pub fn device_id(&self) -> Option<&str> {
        match self {
            RegionSpec::Peripheral { device, .. } => Some(device.id()),
            RegionSpec::Ram { .. } | RegionSpec::Rom { .. } => None,
        }
    }
}

impl DeviceSpec {
    /// The device id — the key the restore path matches on.
    pub fn id(&self) -> &str {
        match self {
            DeviceSpec::Gpio { id }
            | DeviceSpec::Uart16550 { id, .. }
            | DeviceSpec::Clint { id, .. }
            | DeviceSpec::Plic { id, .. } => id,
        }
    }

    /// The device kind on the wire (`"gpio"` / `"uart16550"` / `"clint"` /
    /// `"plic"`) — the value of the `device` internal tag.
    pub fn kind(&self) -> &'static str {
        match self {
            DeviceSpec::Gpio { .. } => "gpio",
            DeviceSpec::Uart16550 { .. } => "uart16550",
            DeviceSpec::Clint { .. } => "clint",
            DeviceSpec::Plic { .. } => "plic",
        }
    }

    /// The device's **numeric** parameters and their legal value domain.
    ///
    /// The tuple is `(field name, value, lower bound, upper bound)`; the
    /// upper bound is `None` when the architecture gives **no** upper bound
    /// — that is the case for `uart16550.baud_rate` (the RISC-V
    /// architecture does not cap the baud rate ⇒ **no number is
    /// invented** for it).
    ///
    /// ⚠️ `gpio` has **no** numeric parameter ⇒ it yields an empty list ⇒
    /// **no** new rule applies to it.
    ///
    /// ⚠️ The bounds live in **this** module ([`MAX_CLINT_FREQ_HZ`] and
    /// friends) ⇒ one definition, everybody else references them.
    fn numeric_parameters(&self) -> Vec<(&'static str, u64, u64, Option<u64>)> {
        match self {
            DeviceSpec::Gpio { .. } => Vec::new(),
            DeviceSpec::Uart16550 { baud_rate, .. } => {
                vec![("baud_rate", u64::from(*baud_rate), 1, None)]
            }
            DeviceSpec::Clint { freq_hz, harts, .. } => vec![
                ("freq_hz", *freq_hz, 1, Some(MAX_CLINT_FREQ_HZ)),
                (
                    "harts",
                    u64::from(*harts),
                    1,
                    Some(u64::from(MAX_CLINT_HARTS)),
                ),
            ],
            DeviceSpec::Plic {
                sources, contexts, ..
            } => vec![
                (
                    "sources",
                    u64::from(*sources),
                    1,
                    Some(u64::from(MAX_PLIC_SOURCES)),
                ),
                (
                    "contexts",
                    u64::from(*contexts),
                    1,
                    Some(u64::from(MAX_PLIC_CONTEXTS)),
                ),
            ],
        }
    }
}

/// What the description **knows** about one region's extent.
///
/// ⚠️ A peripheral window declares only its **base**: its size is the
/// device's register map (裁决 D), so at this layer it is unknown.
#[derive(Debug, Clone, Copy)]
enum DeclaredExtent {
    /// A RAM / ROM region — a **known** half-open interval `[lo, hi)`.
    Interval { lo: u64, hi: u64 },
    /// A peripheral window — only the base address is declared.
    BaseOnly { addr: u64 },
}

/// Do two declared regions overlap?
///
/// ⚠️ The peripheral case is deliberately conservative: the window's
/// size is unknown here, but it is **never empty** (every device's
/// register map occupies at least one byte) ⇒ a peripheral whose base
/// falls inside a declared interval **does** overlap it, and two
/// peripherals sharing a base overlap each other. These are exactly the
/// overlaps M-14 will report at mapping time — the description layer
/// catches them earlier and with its own code.
fn regions_overlap(a: &RegionSpec, b: &RegionSpec) -> bool {
    let a = match a {
        RegionSpec::Ram { base, size } | RegionSpec::Rom { base, size } => {
            DeclaredExtent::Interval {
                lo: u64::from(*base),
                hi: u64::from(*base) + u64::from(*size),
            }
        }
        RegionSpec::Peripheral { base, .. } => DeclaredExtent::BaseOnly {
            addr: u64::from(*base),
        },
    };
    let b = match b {
        RegionSpec::Ram { base, size } | RegionSpec::Rom { base, size } => {
            DeclaredExtent::Interval {
                lo: u64::from(*base),
                hi: u64::from(*base) + u64::from(*size),
            }
        }
        RegionSpec::Peripheral { base, .. } => DeclaredExtent::BaseOnly {
            addr: u64::from(*base),
        },
    };
    match (a, b) {
        (
            DeclaredExtent::Interval { lo: a_lo, hi: a_hi },
            DeclaredExtent::Interval { lo: b_lo, hi: b_hi },
        ) => a_lo < b_hi && b_lo < a_hi,
        (DeclaredExtent::Interval { lo, hi }, DeclaredExtent::BaseOnly { addr }) => {
            lo <= addr && addr < hi
        }
        (DeclaredExtent::BaseOnly { addr }, DeclaredExtent::Interval { lo, hi }) => {
            lo <= addr && addr < hi
        }
        (DeclaredExtent::BaseOnly { addr: a_addr }, DeclaredExtent::BaseOnly { addr: b_addr }) => {
            a_addr == b_addr
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `qemu-virt` built-in description, as the frozen spec §1.2 /
    /// §1.4 / §1.5 spells it.
    ///
    /// ⚠️ The addresses and sizes here are **architecture literals**, not
    /// values read back from the implementation ⇒ the test cannot pass by
    /// construction.
    fn sample() -> PlatformDescription {
        PlatformDescription {
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "qemu-virt".to_string(),
            name: "QEMU virt".to_string(),
            priority: 20,
            auto_selectable: true,
            region_budget_bytes: 134_217_728,
            regions: vec![
                RegionSpec::Ram {
                    base: 0,
                    size: 1_048_576,
                },
                RegionSpec::Ram {
                    base: 0x8000_0000,
                    size: 134_217_728,
                },
                RegionSpec::Peripheral {
                    base: 0x4000_0000,
                    device: DeviceSpec::Gpio {
                        id: "GPIO0".to_string(),
                    },
                },
                RegionSpec::Peripheral {
                    base: 0x1000_0000,
                    device: DeviceSpec::Uart16550 {
                        id: "UART0".to_string(),
                        irq_line: 1,
                        baud_rate: 115_200,
                    },
                },
                RegionSpec::Peripheral {
                    base: 0x0200_0000,
                    device: DeviceSpec::Clint {
                        id: "CLINT".to_string(),
                        freq_hz: 10_000_000,
                        harts: 1,
                    },
                },
                RegionSpec::Peripheral {
                    base: 0x0C00_0000,
                    device: DeviceSpec::Plic {
                        id: "PLIC".to_string(),
                        sources: 64,
                        contexts: 16,
                    },
                },
            ],
            description: None,
        }
    }

    fn keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("必须是对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    // -----------------------------------------------------------------
    // A-1 — the format version
    // -----------------------------------------------------------------

    /// **A-1** — the description's format version is `1`.
    ///
    /// ⚠️ Asserted against the **literal** `1`, not against the constant
    /// it also defines ⇒ the constant cannot vouch for itself.
    #[test]
    fn a1_the_platform_schema_version_is_one() {
        assert_eq!(PLATFORM_SCHEMA_VERSION, 1);
    }

    /// **A-1 / V-1** — a description whose `schema_version` is not the one
    /// this build speaks is rejected with `E_PLT-01`.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **not** on the
    /// message text.
    #[test]
    fn v1_a_wrong_schema_version_is_rejected() {
        let mut desc = sample();
        desc.schema_version = 2;
        let err = desc.validate().expect_err("版本不对必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_01_SCHEMA_VERSION);
        assert!(err.to_string().starts_with("E_PLT-01: "));
    }

    // -----------------------------------------------------------------
    // A-2 — the key sets are pinned
    // -----------------------------------------------------------------

    /// **A-2** — the description's **top-level key set** is pinned.
    ///
    /// ⚠️ `description` is **absent** when it is `None` (it carries
    /// `skip_serializing_if`) ⇒ the key set is the seven mandatory keys,
    /// not eight.
    ///
    /// ⚠️ `auto_selectable` **is** one of them: it is a plain field (no
    /// `skip_serializing_if`) ⇒ it is always written, because a
    /// description that hides it would hide **the difference between the
    /// real platforms and the demo one**.
    #[test]
    fn a2_the_top_level_key_set_is_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        assert_eq!(
            keys(&json),
            vec![
                "auto_selectable",
                "id",
                "name",
                "priority",
                "region_budget_bytes",
                "regions",
                "schema_version",
            ],
            "平台描述的顶层键集必须精确固定（七个）"
        );

        // With a note attached, the optional key **appears** — and still
        // nothing else does.
        let mut with_note = sample();
        with_note.description = Some("人读说明".to_string());
        let json = serde_json::to_value(with_note).expect("json");
        assert_eq!(
            keys(&json),
            vec![
                "auto_selectable",
                "description",
                "id",
                "name",
                "priority",
                "region_budget_bytes",
                "regions",
                "schema_version",
            ],
            "带说明的描述只多出 description 一个键"
        );
    }

    /// **A-2 (续)** — a description that **omits** `auto_selectable` parses
    /// with the default `true`.
    ///
    /// ⚠️ **The default is `true`, not `false`**: a hand-written
    /// description that says nothing about auto-selection must behave
    /// like a **normal** platform (it takes part in auto-selection) ⇒
    /// only a description that **explicitly** opts out is excluded.
    #[test]
    fn a2_an_absent_auto_selectable_flag_defaults_to_true() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 4096,
            "regions": [ { "kind": "ram", "base": 0, "size": 1024 } ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert!(
            desc.auto_selectable,
            "没写 auto_selectable 的描述默认参与自动选档"
        );
    }

    /// **A-2** — the **region** key sets are pinned, one per `kind`.
    ///
    /// ⚠️ A peripheral region carries **no** `size`: the window size is
    /// the device's register map (裁决 D) ⇒ a `size` key here would be a
    /// second source of truth.
    #[test]
    fn a2_the_region_key_sets_are_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        assert_eq!(regions.len(), 6);

        let ram = keys(&regions[0]);
        assert_eq!(ram, vec!["base", "kind", "size"], "RAM 条目的键集");

        let peripheral = keys(&regions[2]);
        assert_eq!(
            peripheral,
            vec!["base", "device", "kind"],
            "外设条目的键集（没有 size）"
        );
    }

    /// **A-2** — the **device** key sets are pinned, one per device kind.
    #[test]
    fn a2_the_device_key_sets_are_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");

        let device =
            |index: usize| -> &serde_json::Value { regions[index].get("device").expect("device") };

        assert_eq!(keys(device(2)), vec!["device", "id"], "GPIO 的键集");
        assert_eq!(
            keys(device(3)),
            vec!["baud_rate", "device", "id", "irq_line"],
            "UART 的键集"
        );
        assert_eq!(
            keys(device(4)),
            vec!["device", "freq_hz", "harts", "id"],
            "CLINT 的键集"
        );
        assert_eq!(
            keys(device(5)),
            vec!["contexts", "device", "id", "sources"],
            "PLIC 的键集"
        );
    }

    // -----------------------------------------------------------------
    // A-3 — one rule, one scenario code
    // -----------------------------------------------------------------

    /// **A-3 / V-2** — an empty platform id is rejected with `E_PLT-02`.
    #[test]
    fn v2_an_empty_id_is_rejected() {
        let mut desc = sample();
        desc.id = String::new();
        let err = desc.validate().expect_err("空 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_02_EMPTY_ID);
    }

    /// **A-3 / V-3** — a description with no region at all is rejected
    /// with `E_PLT-03`.
    #[test]
    fn v3_a_description_without_regions_is_rejected() {
        let mut desc = sample();
        desc.regions.clear();
        let err = desc.validate().expect_err("没有区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_03_NO_REGIONS);
    }

    /// **A-3 / V-4** — a RAM region of size zero is rejected with
    /// `E_PLT-04`.
    ///
    /// ⚠️ Same meaning as M-14's `E_BUS-06`, **different layer** ⇒
    /// different code.
    #[test]
    fn v4_a_zero_sized_region_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![RegionSpec::Ram {
            base: 0x1000,
            size: 0,
        }];
        let err = desc.validate().expect_err("大小为 0 的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_04_ZERO_SIZE);
    }

    /// **A-3 / V-5** — a region whose `base + size` leaves the 32-bit
    /// address space is rejected with `E_PLT-05`.
    #[test]
    fn v5_a_region_overflowing_u32_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![RegionSpec::Ram {
            base: 0xFFFF_F000,
            size: 0x2000,
        }];
        let err = desc.validate().expect_err("越界的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_05_OVERFLOW);
    }

    /// **A-3 / V-6** — two overlapping regions are rejected with
    /// `E_PLT-06`.
    ///
    /// ⚠️ The regions are listed **out of order** (the later one starts
    /// lower) ⇒ the check must sort by `base` first, otherwise it would
    /// miss the overlap.
    #[test]
    fn v6_two_overlapping_regions_are_rejected() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Ram {
                base: 0x2000,
                size: 0x1000,
            },
            RegionSpec::Ram {
                base: 0x1000,
                size: 0x2000,
            },
        ];
        let err = desc.validate().expect_err("重叠的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_06_OVERLAP);
    }

    /// **A-3 / V-6** — a peripheral window whose base falls inside a
    /// declared RAM interval overlaps it.
    ///
    /// ⚠️ The window's **size** is unknown at this layer (it is the
    /// device's register map) ⇒ the judgement is "the base lies inside
    /// the interval", which is the conservative reading and agrees with
    /// what M-14 reports when the map is actually built.
    #[test]
    fn v6_a_peripheral_inside_a_ram_region_overlaps_it() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Ram {
                base: 0x1000,
                size: 0x1000,
            },
            RegionSpec::Peripheral {
                base: 0x1800,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
        ];
        let err = desc.validate().expect_err("落在 RAM 区间内的外设必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_06_OVERLAP);
    }

    /// **A-3 / V-7** — a RAM region whose **declared** size exceeds the
    /// description's budget is rejected with `E_PLT-07`.
    ///
    /// ⚠️ **Not** M-14's `E_BUS-09`: that one judges the **physical
    /// occupancy** at runtime, this one judges the **declaration**.
    #[test]
    fn v7_a_region_over_the_declared_budget_is_rejected() {
        let mut desc = sample();
        desc.region_budget_bytes = 1024;
        desc.regions = vec![RegionSpec::Ram {
            base: 0x1000,
            size: 2048,
        }];
        let err = desc.validate().expect_err("超预算的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_07_OVER_BUDGET);
    }

    /// **A-3 / V-7** — a region whose size is **exactly** the budget is
    /// allowed (`>` and not `>=`).
    ///
    /// ⚠️ The built-in `qemu-virt` description declares a 128 MiB RAM
    /// against a 128 MiB budget ⇒ `>=` would reject a valid platform.
    #[test]
    fn v7_a_region_exactly_at_the_budget_is_allowed() {
        let mut desc = sample();
        desc.region_budget_bytes = 134_217_728;
        desc.regions = vec![RegionSpec::Ram {
            base: 0x8000_0000,
            size: 134_217_728,
        }];
        assert!(desc.validate().is_ok(), "恰好等于预算的区域必须被接受");
    }

    /// **A-3 / V-8** — an unknown device kind fails during
    /// **deserialization**, not during validation.
    ///
    /// ⚠️ This is why the family has **eight** scenario codes and not
    /// nine: the shape rule lives in the serde layer.
    #[test]
    fn v8_an_unknown_device_kind_fails_in_the_serde_layer() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 1024,
            "regions": [ { "kind": "peripheral", "base": 0, "device": { "device": "nvme", "id": "X" } } ]
        }"#;
        let err =
            serde_json::from_str::<PlatformDescription>(raw).expect_err("未知器件种类必须解析失败");
        let text = err.to_string();
        assert!(
            text.contains("nvme"),
            "serde 的报错必须点名那个未知的器件种类，实际：{text}"
        );
    }

    /// **A-3 / V-8** — a device with a **missing** parameter fails in the
    /// serde layer too.
    #[test]
    fn v8_a_missing_device_parameter_fails_in_the_serde_layer() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 1024,
            "regions": [ { "kind": "peripheral", "base": 0, "device": { "device": "uart16550", "id": "U0" } } ]
        }"#;
        assert!(
            serde_json::from_str::<PlatformDescription>(raw).is_err(),
            "缺参数的器件条目必须解析失败"
        );
    }

    /// **A-3 / V-9** — two peripheral devices sharing an id are rejected
    /// with `E_PLT-08`.
    ///
    /// ⚠️ The two devices are of **different kinds** (a GPIO and a UART)
    /// ⇒ the check spans kinds, exactly like the session payload's own
    /// duplicate-id rule.
    #[test]
    fn v9_a_duplicated_device_id_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Peripheral {
                base: 0x1000,
                device: DeviceSpec::Gpio {
                    id: "DUP".to_string(),
                },
            },
            RegionSpec::Peripheral {
                base: 0x2000,
                device: DeviceSpec::Uart16550 {
                    id: "DUP".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                },
            },
        ];
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID);
    }

    /// **A-3** — a well-formed description passes validation.
    ///
    /// ⚠️ Without this, "everything is rejected" would satisfy every
    /// rejection case above.
    #[test]
    fn a_valid_description_passes_validation() {
        assert!(sample().validate().is_ok());
    }

    // -----------------------------------------------------------------
    // A-4 — the round trip
    // -----------------------------------------------------------------

    /// **A-4** — the description survives a JSON round trip field by
    /// field.
    #[test]
    fn the_description_survives_a_round_trip() {
        let original = sample();
        let text = serde_json::to_string(&original).expect("serialize");
        let back: PlatformDescription = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, original);
        assert_eq!(back.id, original.id);
        assert_eq!(back.name, original.name);
        assert_eq!(back.priority, original.priority);
        assert_eq!(back.region_budget_bytes, original.region_budget_bytes);
        assert_eq!(back.regions, original.regions);
        assert_eq!(back.description, original.description);
    }

    /// **A-4** — the maximum budget round-trips; **one more** is rejected
    /// in the **serde** layer.
    ///
    /// ⚠️ The budget is a `u32` (not a `u64`): it is a **per-region**
    /// byte budget and the address space is 4 GiB ⇒ a budget beyond
    /// `u32::MAX` is meaningless. Out-of-range values therefore fail
    /// during deserialization — they never reach `validate()`.
    #[test]
    fn the_maximum_budget_round_trips_and_one_more_is_rejected_by_serde() {
        let mut desc = sample();
        desc.region_budget_bytes = u32::MAX;
        let text = serde_json::to_string(&desc).expect("serialize");
        let back: PlatformDescription = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.region_budget_bytes, 4_294_967_295);

        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 4294967296,
            "regions": [ { "kind": "ram", "base": 0, "size": 1024 } ]
        }"#;
        assert!(
            serde_json::from_str::<PlatformDescription>(raw).is_err(),
            "超过 u32::MAX 的预算必须在 serde 阶段被拒"
        );
    }

    // -----------------------------------------------------------------
    // A-5 — the internal tag keys
    // -----------------------------------------------------------------

    /// **A-5** — the internal tags are literally `kind` (regions) and
    /// `device` (devices) — **not** `type`, **not** `t`.
    #[test]
    fn the_internal_tag_keys_are_kind_and_device() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");

        let ram = regions[0].as_object().expect("ram");
        assert_eq!(ram.get("kind").and_then(|v| v.as_str()), Some("ram"));
        assert!(ram.get("type").is_none(), "区域条目不许有 type 键");
        assert!(ram.get("t").is_none(), "区域条目不许有 t 键");

        let peripheral = regions[3].as_object().expect("peripheral");
        assert_eq!(
            peripheral.get("kind").and_then(|v| v.as_str()),
            Some("peripheral")
        );
        let device = peripheral
            .get("device")
            .and_then(|v| v.as_object())
            .expect("device");
        assert_eq!(
            device.get("device").and_then(|v| v.as_str()),
            Some("uart16550")
        );
        assert!(device.get("type").is_none(), "器件条目不许有 type 键");
        assert!(device.get("kind").is_none(), "器件条目不许有 kind 键");
    }

    /// **A-5** — the description layer and the snapshot topology speak
    /// **one** region-kind vocabulary.
    ///
    /// ⚠️ The three constants live in the contract layer (`session`) and
    /// the kernel's `m14_bus` re-exports them ⇒ the description's `kind`
    /// values are literally those strings ⇒ a description and an exported
    /// memory map need **no** translation.
    #[test]
    fn the_region_kinds_are_the_snapshot_topology_vocabulary() {
        assert_eq!(REGION_KIND_RAM, "ram");
        assert_eq!(REGION_KIND_ROM, "rom");
        assert_eq!(REGION_KIND_PERIPHERAL, "peripheral");

        assert_eq!(RegionSpec::Ram { base: 0, size: 1 }.kind(), "ram");
        assert_eq!(RegionSpec::Rom { base: 0, size: 1 }.kind(), "rom");
        assert_eq!(
            RegionSpec::Peripheral {
                base: 0,
                device: DeviceSpec::Gpio {
                    id: "G".to_string()
                }
            }
            .kind(),
            "peripheral"
        );

        // …and the **wire** form agrees with the vocabulary.
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        assert_eq!(regions[0].get("kind").and_then(|v| v.as_str()), Some("ram"));
        assert_eq!(
            regions[2].get("kind").and_then(|v| v.as_str()),
            Some("peripheral")
        );
    }

    /// **A-5** — the device kinds on the wire agree with
    /// [`DeviceSpec::kind`].
    #[test]
    fn the_device_kinds_on_the_wire_agree_with_the_helper() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        let wire = |index: usize| -> &str {
            regions[index]
                .get("device")
                .and_then(|v| v.get("device"))
                .and_then(|v| v.as_str())
                .expect("device tag")
        };
        assert_eq!(wire(2), DeviceSpec::Gpio { id: "G".into() }.kind());
        assert_eq!(
            wire(3),
            DeviceSpec::Uart16550 {
                id: "U".into(),
                irq_line: 1,
                baud_rate: 115_200
            }
            .kind()
        );
        assert_eq!(
            wire(4),
            DeviceSpec::Clint {
                id: "C".into(),
                freq_hz: 10_000_000,
                harts: 1
            }
            .kind()
        );
        assert_eq!(
            wire(5),
            DeviceSpec::Plic {
                id: "P".into(),
                sources: 64,
                contexts: 16
            }
            .kind()
        );
    }

    // -----------------------------------------------------------------
    // The frozen example
    // -----------------------------------------------------------------

    /// The frozen spec's example (§1.2 / §1.4 / §1.5) parses into the
    /// expected value, field by field.
    #[test]
    fn the_frozen_example_description_parses() {
        let raw = r#"{
            "schema_version": 1,
            "id": "qemu-virt",
            "name": "QEMU virt",
            "priority": 20,
            "auto_selectable": true,
            "region_budget_bytes": 134217728,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "rom", "base": 4096, "size": 65536 },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert_eq!(desc.schema_version, 1);
        assert_eq!(desc.id, "qemu-virt");
        assert_eq!(desc.name, "QEMU virt");
        assert_eq!(desc.priority, 20);
        assert!(desc.auto_selectable, "冻结示例里写明了 auto_selectable");
        assert_eq!(desc.region_budget_bytes, 134_217_728);
        assert_eq!(desc.description, None);
        assert_eq!(desc.regions.len(), 3);
        assert_eq!(
            desc.regions[0],
            RegionSpec::Ram {
                base: 0,
                size: 1_048_576
            }
        );
        assert_eq!(
            desc.regions[1],
            RegionSpec::Rom {
                base: 4096,
                size: 65_536
            }
        );
        assert_eq!(
            desc.regions[2],
            RegionSpec::Peripheral {
                base: 1_073_741_824,
                device: DeviceSpec::Uart16550 {
                    id: "UART0".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                }
            }
        );
    }

    // -----------------------------------------------------------------
    // A-6 / A-8 — the error family and the untouched contract version
    // -----------------------------------------------------------------

    /// **A-6** — the platform error family is registered with its
    /// **three-character** wire tag.
    #[test]
    fn a6_the_platform_family_is_registered() {
        assert_eq!(crate::error::ErrorFamily::Platform.as_str(), "PLT");
        assert_eq!(PLATFORM_FAMILY, crate::error::ErrorFamily::Platform);
        assert!(
            crate::error::ALL_FAMILIES.contains(&crate::error::ErrorFamily::Platform),
            "Platform 必须出现在族清单里"
        );
    }

    /// **A-6** — the scenario codes render as the frozen spec's codes,
    /// digit for digit.
    ///
    /// ⚠️ The three **host-side** codes (V-10 .. V-12) are listed here
    /// too: they are owned by the contract layer even though the host is
    /// the one that detects them ⇒ a code has exactly one landing spot.
    #[test]
    fn a6_the_platform_scenario_codes_render_as_the_spec_spells_them() {
        use crate::error::{
            E_PLT_09_UNKNOWN_PLATFORM, E_PLT_10_PLATFORM_FILE_UNREADABLE,
            E_PLT_11_PLATFORM_FILE_MALFORMED,
        };
        let cases = [
            (E_PLT_01_SCHEMA_VERSION, "E_PLT-01"),
            (E_PLT_02_EMPTY_ID, "E_PLT-02"),
            (E_PLT_03_NO_REGIONS, "E_PLT-03"),
            (E_PLT_04_ZERO_SIZE, "E_PLT-04"),
            (E_PLT_05_OVERFLOW, "E_PLT-05"),
            (E_PLT_06_OVERLAP, "E_PLT-06"),
            (E_PLT_07_OVER_BUDGET, "E_PLT-07"),
            (E_PLT_08_DUPLICATE_DEVICE_ID, "E_PLT-08"),
            (E_PLT_09_UNKNOWN_PLATFORM, "E_PLT-09"),
            (E_PLT_10_PLATFORM_FILE_UNREADABLE, "E_PLT-10"),
            (E_PLT_11_PLATFORM_FILE_MALFORMED, "E_PLT-11"),
            (E_PLT_12_PARAMETER_OUT_OF_RANGE, "E_PLT-12"),
        ];
        for (scenario, expected) in cases {
            let err = PlatformError::new(PLATFORM_FAMILY, scenario, "x");
            assert_eq!(
                err.to_string(),
                format!("{expected}: x"),
                "场景码 {scenario} 渲染成了 {err}"
            );
        }
    }

    /// **A-8** — the contract layer's **session payload** version is
    /// untouched: a platform description is **not** a session payload, so
    /// it does not move that number.
    #[test]
    fn a8_the_session_payload_version_is_untouched() {
        assert_eq!(crate::session::SESSION_SNAPSHOT_SCHEMA_VERSION, 3);
    }

    // -----------------------------------------------------------------
    // STAGE-15 · v0.30 · D-141 — the devices' legal value domain
    // (V-13 ~ V-15 · `E_PLT-12`)
    //
    // ⚠️ The **A-n** labels below are the frozen spec STAGE-15 §5.1's
    // acceptance items (A-1 ~ A-9) — **not** the older "A-n" labels the
    // STAGE-12 cases above carry ⇒ every label here says "STAGE-15".
    // -----------------------------------------------------------------

    /// A description made of the given **peripheral devices** only.
    ///
    /// ⚠️ Each device gets its own window base ⇒ the map has **no**
    /// overlap (V-6) ⇒ the cases below are about the **device
    /// parameters**, not about the regions.
    fn board_with(devices: Vec<DeviceSpec>) -> PlatformDescription {
        PlatformDescription {
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "board-under-test".to_string(),
            name: "Board under test".to_string(),
            priority: 1,
            auto_selectable: true,
            region_budget_bytes: 134_217_728,
            regions: devices
                .into_iter()
                .enumerate()
                .map(|(index, device)| RegionSpec::Peripheral {
                    base: 0x1000_0000u32 + (index as u32) * 0x1_0000,
                    device,
                })
                .collect(),
            description: None,
        }
    }

    /// Assert that a description is rejected with the platform family's
    /// `E_PLT-12` scenario code.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **never** on the
    /// message text ⇒ rewording the message must not turn any case red.
    fn assert_parameter_out_of_range(desc: &PlatformDescription, what: &str) {
        let err = desc.validate().expect_err(what);
        assert_eq!(err.family, PLATFORM_FAMILY, "{what} 的族不是 Platform");
        assert_eq!(
            err.scenario, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "{what} 的场景码不是 E_PLT-12（实际 {err}）"
        );
    }

    // -------------------------------------------------------------
    // STAGE-15 A-1 — V-13: the lower bounds
    // -------------------------------------------------------------

    /// **STAGE-15 A-1 / V-13** — a UART whose `baud_rate` is **zero** is
    /// rejected with `E_PLT-12`.
    ///
    /// ⚠️ The map has **no PLIC** ⇒ V-15 does not apply ⇒ this case is
    /// about the **single field** alone.
    #[test]
    fn v13_a_zero_baud_rate_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 0,
        }]);
        assert_parameter_out_of_range(&desc, "波特率为 0 的 UART");
    }

    /// **STAGE-15 A-2 / V-13** — a CLINT whose `freq_hz` is **zero** is
    /// rejected with `E_PLT-12`.
    #[test]
    fn v13_a_zero_clint_frequency_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 0,
            harts: 1,
        }]);
        assert_parameter_out_of_range(&desc, "频率为 0 的 CLINT");
    }

    /// **STAGE-15 A-3 / V-13** — a CLINT with **zero harts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_clint_without_harts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 0,
        }]);
        assert_parameter_out_of_range(&desc, "hart 数为 0 的 CLINT");
    }

    /// **STAGE-15 A-3 / V-13** — a PLIC with **zero sources** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_plic_without_sources_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 0,
            contexts: 16,
        }]);
        assert_parameter_out_of_range(&desc, "中断源数为 0 的 PLIC");
    }

    /// **STAGE-15 A-3 / V-13** — a PLIC with **zero contexts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_plic_without_contexts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 0,
        }]);
        assert_parameter_out_of_range(&desc, "context 数为 0 的 PLIC");
    }

    // -------------------------------------------------------------
    // STAGE-15 A-4 — V-14: the upper bounds
    // -------------------------------------------------------------

    /// **STAGE-15 A-4 / V-14** — a CLINT whose `freq_hz` is **one above
    /// 1 THz** is rejected with `E_PLT-12`.
    ///
    /// ⚠️ The bound is written as an **architecture literal**
    /// (`1_000_000_000_001`), not as `MAX_CLINT_FREQ_HZ + 1` ⇒ a wrong
    /// constant cannot make this case pass.
    #[test]
    fn v14_a_clint_frequency_above_one_terahertz_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1_000_000_000_001,
            harts: 1,
        }]);
        assert_parameter_out_of_range(&desc, "频率超过 1 THz 的 CLINT");
    }

    /// **STAGE-15 A-4 / V-14** — a CLINT with **4097 harts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_clint_with_more_than_4096_harts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 4097,
        }]);
        assert_parameter_out_of_range(&desc, "hart 数超过 4096 的 CLINT");
    }

    /// **STAGE-15 A-4 / V-14** — a PLIC with **1025 sources** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_plic_with_more_than_1024_sources_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1025,
            contexts: 16,
        }]);
        assert_parameter_out_of_range(&desc, "中断源数超过 1024 的 PLIC");
    }

    /// **STAGE-15 A-4 / V-14** — a PLIC with **17 contexts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_plic_with_more_than_16_contexts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 17,
        }]);
        assert_parameter_out_of_range(&desc, "context 数超过 16 的 PLIC");
    }

    // -------------------------------------------------------------
    // STAGE-15 A-5 — the boundary values are legal
    // -------------------------------------------------------------

    /// **STAGE-15 A-5 / V-13** — a baud rate of **1** is legal (the lower
    /// bound is inclusive).
    #[test]
    fn v13_the_lowest_legal_baud_rate_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 1,
        }]);
        assert!(desc.validate().is_ok(), "波特率 1 必须被接受");
    }

    /// **STAGE-15 A-5 / V-13** — a CLINT frequency of **1 Hz** is legal.
    #[test]
    fn v13_the_lowest_legal_clint_frequency_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "频率 1 Hz 必须被接受");
    }

    /// **STAGE-15 A-5 / V-13** — **one hart** is legal.
    #[test]
    fn v13_the_lowest_legal_clint_hart_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单 hart 的 CLINT 必须被接受");
    }

    /// **STAGE-15 A-5 / V-13** — **one source** is legal.
    #[test]
    fn v13_the_lowest_legal_plic_source_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1,
            contexts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单中断源的 PLIC 必须被接受");
    }

    /// **STAGE-15 A-5 / V-13** — **one context** is legal.
    #[test]
    fn v13_the_lowest_legal_plic_context_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单 context 的 PLIC 必须被接受");
    }

    /// **STAGE-15 A-5 / V-14** — a CLINT frequency of exactly
    /// **1 THz** is legal (the upper bound is inclusive).
    #[test]
    fn v14_the_highest_legal_clint_frequency_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1_000_000_000_000,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "1 THz 的 CLINT 必须被接受");
    }

    /// **STAGE-15 A-5 / V-14** — exactly **4096 harts** are legal.
    #[test]
    fn v14_the_highest_legal_clint_hart_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 4096,
        }]);
        assert!(desc.validate().is_ok(), "4096 个 hart 的 CLINT 必须被接受");
    }

    /// **STAGE-15 A-5 / V-14** — exactly **1024 sources** are legal.
    #[test]
    fn v14_the_highest_legal_plic_source_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1024,
            contexts: 16,
        }]);
        assert!(desc.validate().is_ok(), "1024 个中断源的 PLIC 必须被接受");
    }

    /// **STAGE-15 A-5 / V-14** — exactly **16 contexts** are legal.
    #[test]
    fn v14_the_highest_legal_plic_context_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 16,
        }]);
        assert!(desc.validate().is_ok(), "16 个 context 的 PLIC 必须被接受");
    }

    /// **STAGE-15 A-5 / V-14** — the baud rate has **no upper bound**.
    ///
    /// ⚠️ The RISC-V architecture does not cap the baud rate ⇒ the
    /// description layer must not invent one ⇒ the largest value the
    /// field can hold is still legal.
    #[test]
    fn v14_the_baud_rate_has_no_upper_bound() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: u32::MAX,
        }]);
        assert!(desc.validate().is_ok(), "波特率没有上界 ⇒ 不该被拒");
    }

    // -------------------------------------------------------------
    // STAGE-15 A-6 / A-7 — V-15: the cross-field rule
    // -------------------------------------------------------------

    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **20** is rejected with `E_PLT-12`.
    ///
    /// ⚠️ PLIC source ids are **0-based** ⇒ the legal ids of a 16-source
    /// PLIC are `0 ..= 15` ⇒ `20` is outside them.
    #[test]
    fn v15_an_irq_line_beyond_the_plic_source_count_is_rejected() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 20,
                baud_rate: 115_200,
            },
        ]);
        assert_parameter_out_of_range(&desc, "中断线超出 PLIC 源数的 UART");
    }

    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **15** (the highest legal id) is accepted.
    #[test]
    fn v15_the_highest_legal_irq_line_is_accepted() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 15,
                baud_rate: 115_200,
            },
        ]);
        assert!(desc.validate().is_ok(), "irq_line 15 必须被接受");
    }

    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **16** — one past the highest legal id — is
    /// rejected with `E_PLT-12`.
    ///
    /// ⚠️ PLIC source ids are **0-based** ⇒ a 16-source PLIC's legal ids
    /// are `0 ..= 15` ⇒ `16` is the first **illegal** one. This is the
    /// case that separates `<` from `<=`.
    #[test]
    fn v15_an_irq_line_equal_to_the_plic_source_count_is_rejected() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 16,
                baud_rate: 115_200,
            },
        ]);
        assert_parameter_out_of_range(&desc, "中断线等于 PLIC 源数的 UART");
    }

    /// **STAGE-15 A-7 / V-15** — a map with **no PLIC** and a UART whose
    /// `irq_line` is **65535** is accepted.
    ///
    /// ⚠️ **No PLIC does not make the board illegal** (裁决 G): the board
    /// simply has no interrupt routing ⇒ `irq_line` is meaningless but
    /// harmless ⇒ refusing it would refuse a legal board.
    #[test]
    fn v15_an_irq_line_without_any_plic_is_not_judged() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 65_535,
            baud_rate: 115_200,
        }]);
        assert!(
            desc.validate().is_ok(),
            "没有 PLIC 时 irq_line 不该被判非法"
        );
    }

    // -------------------------------------------------------------
    // STAGE-15 A-8 — the built-in descriptions stay legal
    // -------------------------------------------------------------

    /// **STAGE-15 A-8** — the built-in `default` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ **This copy is hand-written from
    /// `kernel_host/platforms/default.json`** — the contract layer does
    /// **not** depend on the kernel crate ⇒ the equivalent copy lives
    /// here (the parameters are copied field by field).
    #[test]
    fn a8_the_builtin_default_description_is_still_legal() {
        let raw = r#"{
            "schema_version": 1,
            "id": "default",
            "name": "Default",
            "priority": 10,
            "auto_selectable": true,
            "region_budget_bytes": 16777216,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0" } },
                { "kind": "ram", "base": 2147483648, "size": 4194304 }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 default 描述必须仍然合法");
    }

    /// **STAGE-15 A-8** — the built-in `minimal` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ **This copy is hand-written from
    /// `kernel_host/platforms/minimal.json`** — see the note in the
    /// `default` case above.
    #[test]
    fn a8_the_builtin_minimal_description_is_still_legal() {
        let raw = r#"{
            "schema_version": 1,
            "id": "minimal",
            "name": "Minimal (zero-change demo platform)",
            "priority": 30,
            "auto_selectable": false,
            "region_budget_bytes": 16777216,
            "regions": [
                { "kind": "ram", "base": 0, "size": 2097152 },
                { "kind": "ram", "base": 268435456, "size": 16777216 }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 minimal 描述必须仍然合法");
    }

    /// **STAGE-15 A-8** — the built-in `qemu-virt` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ **This copy is hand-written from
    /// `kernel_host/platforms/qemu-virt.json`** — see the note in the
    /// `default` case above.
    ///
    /// ⚠️ Its PLIC has **16 contexts** ⇒ it sits **exactly on** the upper
    /// bound ⇒ a `>=` comparison would reject the kernel's own board.
    #[test]
    fn a8_the_builtin_qemu_virt_description_is_still_legal() {
        let raw = r#"{
            "schema_version": 1,
            "id": "qemu-virt",
            "name": "QEMU virt",
            "priority": 20,
            "auto_selectable": true,
            "region_budget_bytes": 134217728,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "peripheral", "base": 33554432,
                  "device": { "device": "clint", "id": "CLINT", "freq_hz": 10000000, "harts": 1 } },
                { "kind": "peripheral", "base": 201326592,
                  "device": { "device": "plic", "id": "PLIC", "sources": 64, "contexts": 16 } },
                { "kind": "peripheral", "base": 268435456,
                  "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0" } },
                { "kind": "ram", "base": 2147483648, "size": 134217728 }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 qemu-virt 描述必须仍然合法");
    }

    // -------------------------------------------------------------
    // STAGE-15 A-9 — the report order of V-1 ~ V-9 does not move
    // -------------------------------------------------------------

    /// **STAGE-15 A-9** — a description that breaks **both** V-2 (empty
    /// `id`) and V-13 (a zero baud rate) reports **V-2**.
    ///
    /// ⚠️ The new rules were **appended after V-9** ⇒ the report order
    /// of the old rules is untouched.
    #[test]
    fn a9_the_empty_id_is_reported_before_a_parameter_violation() {
        let mut desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 0,
        }]);
        desc.id = String::new();
        let err = desc.validate().expect_err("空 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_02_EMPTY_ID,
            "同时违反 V-2 与 V-13 时必须先报 V-2（实际 {err}）"
        );
    }

    /// **STAGE-15 A-9** — a description that breaks **both** V-9 (two
    /// devices share an `id`) and V-13 (a zero baud rate) reports **V-9**.
    #[test]
    fn a9_the_duplicated_device_id_is_reported_before_a_parameter_violation() {
        let desc = board_with(vec![
            DeviceSpec::Uart16550 {
                id: "DUP".to_string(),
                irq_line: 0,
                baud_rate: 0,
            },
            DeviceSpec::Gpio {
                id: "DUP".to_string(),
            },
        ]);
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "同时违反 V-9 与 V-13 时必须先报 V-9（实际 {err}）"
        );
    }
}
