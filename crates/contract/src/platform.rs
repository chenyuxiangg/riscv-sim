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
//! | **this module** | the *declaration* — version, id, regions, sizes, overlaps, budget, duplicate device id | `E_PLT-01`..`E_PLT-08` |
//! | **M-14** | the *live bus* — unmapped access, region over the runtime budget | `E_BUS-*` |
//!
//! ⚠️ The two budget rules are **not** the same constraint and must not
//! be merged: [`E_PLT_07_OVER_BUDGET`] judges the **declared** size
//! (before anything is mapped), M-14's `E_BUS-09` judges the **physical
//! occupancy** (after mapping — RAM is lazily allocated since v0.26, so
//! a region may declare a large size and cost nothing).

use serde::{Deserialize, Serialize};

use crate::error::{
    PlatformError, E_PLT_01_SCHEMA_VERSION, E_PLT_02_EMPTY_ID, E_PLT_03_NO_REGIONS,
    E_PLT_04_ZERO_SIZE, E_PLT_05_OVERFLOW, E_PLT_06_OVERLAP, E_PLT_07_OVER_BUDGET,
    E_PLT_08_DUPLICATE_DEVICE_ID, PLATFORM_FAMILY,
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
    ///
    /// ⚠️ **V-8 lives in the serde layer, not here.** An unknown device
    /// kind or a missing parameter fails during deserialization, before
    /// any value exists to validate ⇒ this method never sees it. That is
    /// why the family carries **eight** scenario codes and not nine.
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
    /// `skip_serializing_if`) ⇒ the key set is the six mandatory keys,
    /// not seven.
    #[test]
    fn a2_the_top_level_key_set_is_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        assert_eq!(
            keys(&json),
            vec![
                "id",
                "name",
                "priority",
                "region_budget_bytes",
                "regions",
                "schema_version",
            ],
            "平台描述的顶层键集必须精确固定"
        );

        // With a note attached, the optional key **appears** — and still
        // nothing else does.
        let mut with_note = sample();
        with_note.description = Some("人读说明".to_string());
        let json = serde_json::to_value(with_note).expect("json");
        assert_eq!(
            keys(&json),
            vec![
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

    /// **A-6** — the eight scenario codes render as the frozen spec's
    /// codes, digit for digit.
    #[test]
    fn a6_the_platform_scenario_codes_render_as_the_spec_spells_them() {
        let cases = [
            (E_PLT_01_SCHEMA_VERSION, "E_PLT-01"),
            (E_PLT_02_EMPTY_ID, "E_PLT-02"),
            (E_PLT_03_NO_REGIONS, "E_PLT-03"),
            (E_PLT_04_ZERO_SIZE, "E_PLT-04"),
            (E_PLT_05_OVERFLOW, "E_PLT-05"),
            (E_PLT_06_OVERLAP, "E_PLT-06"),
            (E_PLT_07_OVER_BUDGET, "E_PLT-07"),
            (E_PLT_08_DUPLICATE_DEVICE_ID, "E_PLT-08"),
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
}
