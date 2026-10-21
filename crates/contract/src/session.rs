//! Session-level snapshot payload — the carrier for **a whole session**.
//!
//! # Why this module exists
//!
//! [`crate::snapshot::EngineSnapshotDto`] reproduces the **engine**: the
//! scheduler's pending queue, the platform state (registers, PC, CSRs,
//! pending writes, virtual clock), the insertion counter and the
//! run-window instruction budget. It deliberately does **not** capture
//! the peripheral models (M-15) or the interrupt controller (M-16) —
//! the engine does not own them and cannot serialize them.
//!
//! Persisting only that DTO reproduces an *engine*, **not a session**:
//! the UART FIFOs, the timer `mtime`/`mtimecmp`, the GPIO direction and
//! output latches and the raised IRQ lines are all missing. That gap is
//! debt ledger **D-36**.
//!
//! This module supplies the missing piece: the **session-level payload**
//! defined by M-20 / M-23's detailed design §2.1.1, assembled by M-20
//! and persisted by M-23 (debt ledger **D-37**), together with the
//! per-module export / import interfaces it implies (**D-41**).
//!
//! # The payload shape
//!
//! ```text
//! SessionSnapshotDto {
//!   schema_version,          // this payload's own format version
//!   cp_meta,                 // segment 0 — checkpoint metadata (M-20)
//!   m12_engine,              // segment 1 — the M-12/M-17 segment
//!   m13_state,               // segment 2 — memory pages
//!   m14_map,                 // segment 3 — the memory-map topology
//!   m15_periph,              // segment 4 — peripheral internal state
//!   m16_intc,                // segment 5 — raised IRQ lines
//! }
//! ```
//!
//! The frozen spec is `docs/detailed-design/STAGE-3_Session_Snapshot.md`;
//! it also records the three **adjudications** this shape rests on
//! (A: the structure listing in the LLD omitted the M-12/M-17 segment
//! its own prose demanded; B: the registers have exactly **one** home —
//! `m12_engine.state` — so `m13_state` carries only the memory view;
//! C: M-16 gets a segment because it has a real producer).
//!
//! # Two version numbers, on purpose
//!
//! [`SESSION_SNAPSHOT_SCHEMA_VERSION`] governs the **outer** payload;
//! [`crate::snapshot::SNAPSHOT_SCHEMA_VERSION`] governs the **inner**
//! M-12/M-17 segment that is reused verbatim. Changing the inner shape
//! does not necessarily change the outer one (the inner has its own
//! migration chain); changing the outer shape always does.
//!
//! # Compatibility
//!
//! Adding this module is **purely additive**: no existing type, field or
//! trait method changed shape. The **frozen** [`crate::snapshot`]
//! module is reused as-is — its `EngineSnapshotDto` is segment 1, and
//! its shape is **not** touched.

use serde::{Deserialize, Serialize};

use crate::error::PlatformError;
use crate::model::SIM_STATE_SCHEMA_VERSION;
use crate::snapshot::{EngineSnapshotDto, SNAPSHOT_FAMILY, SNAPSHOT_SCHEMA_VERSION};

/// Format version of the **session-level** payload.
///
/// Bump on every incompatible change to [`SessionSnapshotDto`] or its
/// members. [`SessionSnapshotDto::validate`] rejects versions it does
/// not know, so a newer producer's payload is never silently
/// mis-parsed.
///
/// ⚠️ **This is not the same number as** [`crate::snapshot::SNAPSHOT_SCHEMA_VERSION`]
/// — that one governs the **M-12/M-17 segment** reused as segment 1.
pub const SESSION_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Scenario code: the session payload's shape is malformed — its
/// segment tables are not canonically ordered, or they contradict each
/// other.
///
/// ⚠️ **A dedicated code, on purpose.** `E_SNAP-02` means "the *event
/// queue* is malformed"; reusing it for "the *region table* is not
/// sorted" would make one code mean two things, and a consumer reading
/// `E_SNAP-02` could not tell which table to look at. One scenario,
/// one code.
pub const E_SNAP_06_MALFORMED_SESSION: u16 = 6;

/// Checkpoint metadata — segment 0.
///
/// Mirrors M-20's `Checkpoint` structure: the identity of a checkpoint
/// and the anchors needed to reconcile or replay from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointMetaDto {
    /// Checkpoint id (monotonic).
    pub id: u32,
    /// Virtual time at the checkpoint — **decimal string** (contract v2
    /// §2.2: a growing quantity must not be a JSON number).
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_virt: u64,
    /// Host time (ms) at the checkpoint — **decimal string** (it grows
    /// too, so it follows the same rule).
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_host: u64,
    /// Base sequence number — the anchor for the sparse input-event
    /// stream that follows this checkpoint.
    pub base_seq: u32,
    /// Persisted schema version — equals
    /// [`SESSION_SNAPSHOT_SCHEMA_VERSION`] for a payload this build
    /// writes.
    pub version: u32,
}

impl Default for CheckpointMetaDto {
    fn default() -> Self {
        Self {
            id: 0,
            t_virt: 0,
            t_host: 0,
            base_seq: 0,
            version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        }
    }
}

/// One page of guest-visible memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemPageDto {
    /// Page's start physical address.
    pub base: u32,
    /// Page contents.
    ///
    /// ⚠️ **Not padded to a page boundary** — the last page of a region
    /// may be shorter. Padding would silently change the region's
    /// length on restore.
    pub bytes: Vec<u8>,
}

/// M-13's share of the payload — segment 2.
///
/// ⚠️ **The registers are deliberately *not* here.** The CPU registers
/// (`gpr` / `pc` / `csrs`) have exactly **one** home: the platform
/// state inside `m12_engine.state`. Storing them a second time would
/// give one quantity two homes — and two homes means the restore path
/// has to pick a winner, which means the loser silently rots (the
/// project has been bitten by this shape repeatedly — see D-73's
/// self-referential assertion and D-53's silent degradation).
///
/// What *is* here is the **memory view**: the RAM contents the guest
/// can see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct M13StateDto {
    /// Guest-visible memory pages, ordered by `base`.
    pub mem_pages: Vec<MemPageDto>,
}

/// One entry of the memory-map topology — segment 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegionDto {
    /// Interval base address.
    pub base: u32,
    /// Interval size in bytes.
    pub size: u32,
    /// Region kind — `"ram"`, `"rom"` or `"peripheral"` (a **lower-case
    /// string**, not an enum ordinal: the wire form must survive a
    /// reordering of the enum).
    pub kind: String,
    /// Target identifier — empty for `ram` / `rom`; the peripheral id
    /// (e.g. `"GPIO0"`) for `peripheral`.
    ///
    /// ⚠️ A **bound** peripheral instance is not serializable — only its
    /// id is. The restore path re-binds by id.
    pub target: String,
}

/// The memory-map topology — segment 3.
///
/// ⚠️ **ROM contents are not here.** ROM content travels with the
/// firmware image — the firmware is an *input* of a session, not part
/// of its state. RAM contents live in segment 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct M14MapDto {
    /// Regions, ordered by `base`.
    pub regions: Vec<RegionDto>,
}

/// UART internal state — segment 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UartStateDto {
    /// Peripheral id (the key used to match the live instance).
    pub id: String,
    /// Byte waiting to be transmitted.
    pub tx: Option<u8>,
    /// Byte waiting to be read by the guest.
    pub rx: Option<u8>,
    /// Status register.
    pub status: u32,
    /// IRQ line number.
    pub irq_line: u16,
    /// Peripheral's authoritative virtual timestamp — **decimal string**.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub now_ps: u64,
}

/// GPIO bank internal state — segment 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpioStateDto {
    /// Peripheral id.
    pub id: String,
    /// Direction mask (1 = output).
    pub directions: u32,
    /// Output latch.
    pub outputs: u32,
    /// Input latch.
    pub inputs: u32,
    /// IRQ line per pin.
    pub irq_lines: Vec<u16>,
    /// Pending IRQ bitmask.
    pub pending_irqs: u32,
    /// Peripheral's authoritative virtual timestamp — **decimal string**.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub now_ps: u64,
}

/// Timer internal state — segment 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerStateDto {
    /// Peripheral id.
    pub id: String,
    /// IRQ line number.
    pub irq_line: u16,
    /// `mtime` — **decimal string**.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub mtime: u64,
    /// `mtimecmp` — **decimal string**.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub mtimecmp: u64,
}

/// One peripheral's share of the session state — an element of segment 4.
///
/// The variant carries the **kind**: a UART is always a UART, so the
/// kind is a property of the peripheral rather than a field that could
/// disagree with the payload it sits in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeripheralSessionState {
    /// A UART's state.
    Uart(UartStateDto),
    /// A GPIO bank's state.
    Gpio(GpioStateDto),
    /// A timer's state.
    Timer(TimerStateDto),
}

impl PeripheralSessionState {
    /// The peripheral's stable id.
    pub fn id(&self) -> &str {
        match self {
            Self::Uart(s) => &s.id,
            Self::Gpio(s) => &s.id,
            Self::Timer(s) => &s.id,
        }
    }
}

/// Peripheral internal state — segment 4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct M15PeriphDto {
    /// UART instances, ordered by id.
    pub uarts: Vec<UartStateDto>,
    /// GPIO banks, ordered by id.
    pub gpios: Vec<GpioStateDto>,
    /// Timers, ordered by id.
    pub timers: Vec<TimerStateDto>,
}

/// Interrupt-controller state — segment 5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct M16IntcDto {
    /// Currently raised IRQ lines, ascending and deduplicated.
    pub raised: Vec<u16>,
}

/// The session-level snapshot payload.
///
/// See the [module docs](self) for the shape and the three
/// adjudications it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshotDto {
    /// Format version of this payload.
    pub schema_version: u32,
    /// Segment 0 — checkpoint metadata.
    pub cp_meta: CheckpointMetaDto,
    /// Segment 1 — the M-12/M-17 segment (the frozen
    /// [`EngineSnapshotDto`], reused verbatim).
    pub m12_engine: EngineSnapshotDto,
    /// Segment 2 — memory pages.
    pub m13_state: M13StateDto,
    /// Segment 3 — memory-map topology.
    pub m14_map: M14MapDto,
    /// Segment 4 — peripheral internal state.
    pub m15_periph: M15PeriphDto,
    /// Segment 5 — raised IRQ lines.
    pub m16_intc: M16IntcDto,
}

impl Default for SessionSnapshotDto {
    fn default() -> Self {
        Self {
            schema_version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            cp_meta: CheckpointMetaDto::default(),
            m12_engine: EngineSnapshotDto::default(),
            m13_state: M13StateDto::default(),
            m14_map: M14MapDto::default(),
            m15_periph: M15PeriphDto::default(),
            m16_intc: M16IntcDto::default(),
        }
    }
}

impl SessionSnapshotDto {
    /// Validate the payload **before** any of it is applied.
    ///
    /// ⚠️ **All checks run before the caller writes anything.** A
    /// half-applied snapshot is worse than a rejected one: it leaves
    /// the session in a state that no checkpoint ever described, and
    /// nothing downstream can tell that from a legitimate one.
    ///
    /// Checks (frozen spec §5):
    ///
    /// 1. `schema_version` — known?
    /// 2. `cp_meta.version` — known?
    /// 3. inner `m12_engine.schema_version` — known?
    /// 4. inner `m12_engine.state.schema_version` — known?
    /// 5. `m14_map.regions` — ascending by `base`, non-overlapping
    /// 6. `m13_state.mem_pages` — ascending by `base`, non-overlapping,
    ///    and each page fits inside a RAM region
    /// 7. `m15_periph` — no duplicate id within a kind
    /// 8. `m16_intc.raised` — ascending, no duplicates
    /// 9. the inner engine snapshot validates on its own terms
    pub fn validate(&self) -> Result<(), PlatformError> {
        if self.schema_version != SESSION_SNAPSHOT_SCHEMA_VERSION {
            return Err(unsupported_version(
                "会话快照版本不受支持",
                self.schema_version,
                SESSION_SNAPSHOT_SCHEMA_VERSION,
            ));
        }
        if self.cp_meta.version != SESSION_SNAPSHOT_SCHEMA_VERSION {
            return Err(unsupported_version(
                "检查点元数据的版本不受支持",
                self.cp_meta.version,
                SESSION_SNAPSHOT_SCHEMA_VERSION,
            ));
        }
        if self.m12_engine.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(unsupported_version(
                "引擎段的版本不受支持",
                self.m12_engine.schema_version,
                SNAPSHOT_SCHEMA_VERSION,
            ));
        }
        if self.m12_engine.state.schema_version != SIM_STATE_SCHEMA_VERSION {
            return Err(unsupported_version(
                "引擎段内平台状态的版本不受支持",
                self.m12_engine.state.schema_version,
                SIM_STATE_SCHEMA_VERSION,
            ));
        }

        // 5 — the region table must be canonically ordered and disjoint.
        let mut prev_end: Option<(u32, u32)> = None;
        for r in &self.m14_map.regions {
            if r.size == 0 {
                return Err(self.malformed("内存映射里有大小为 0 的区域"));
            }
            let end = r
                .base
                .checked_add(r.size)
                .ok_or_else(|| self.malformed("内存映射里有溢出的区域（base + size 越过 u32）"))?;
            if let Some((pbase, pend)) = prev_end {
                if r.base < pend {
                    return Err(self.malformed(&format!(
                        "内存映射的区域未按 base 升序或互相重叠：{pbase:#x} 与 {:#x}",
                        r.base
                    )));
                }
                let _ = pbase;
            }
            prev_end = Some((r.base, end));
        }

        // 6 — the page table must be canonically ordered and disjoint,
        //     and every page must fit inside a RAM region.
        let mut prev_page_end: Option<u32> = None;
        for p in &self.m13_state.mem_pages {
            let end = p
                .base
                .checked_add(p.bytes.len() as u32)
                .ok_or_else(|| self.malformed("内存页越过 u32 边界"))?;
            if let Some(prev) = prev_page_end {
                if p.base < prev {
                    return Err(self.malformed("内存页未按 base 升序或互相重叠"));
                }
            }
            prev_page_end = Some(end);

            let fits = self.m14_map.regions.iter().any(|r| {
                r.kind == "ram" && p.base >= r.base && end <= r.base.saturating_add(r.size)
            });
            if !fits {
                return Err(self.malformed(&format!(
                    "内存页 {:#x}..{:#x} 不落在任何 RAM 区域内",
                    p.base, end
                )));
            }
        }

        // 7 — no duplicate peripheral id within a kind.
        check_unique(self.m15_periph.uarts.iter().map(|u| u.id.as_str()), "UART")?;
        check_unique(self.m15_periph.gpios.iter().map(|g| g.id.as_str()), "GPIO")?;
        check_unique(
            self.m15_periph.timers.iter().map(|t| t.id.as_str()),
            "定时器",
        )?;

        // 8 — the raised lines must be ascending and deduplicated.
        let mut prev: Option<u16> = None;
        for line in &self.m16_intc.raised {
            if let Some(p) = prev {
                if *line <= p {
                    return Err(self.malformed("已拉起的 IRQ 行未升序或有重复"));
                }
            }
            prev = Some(*line);
        }

        // 9 — the inner engine snapshot validates on its own terms.
        self.m12_engine.validate()?;

        Ok(())
    }

    /// Build the "malformed session payload" error.
    fn malformed(&self, msg: &str) -> PlatformError {
        PlatformError::new(
            SNAPSHOT_FAMILY,
            E_SNAP_06_MALFORMED_SESSION,
            format!("会话快照载荷畸形：{msg}"),
        )
    }
}

/// Build the "unsupported version" error.
///
/// Reuses the snapshot family's `E_SNAP-05` — "the embedded state
/// carries a format version this build does not understand" — because
/// the *meaning* is identical for the outer payload, the checkpoint
/// metadata and the two nested versions. One scenario, one code.
fn unsupported_version(what: &str, got: u32, want: u32) -> PlatformError {
    PlatformError::new(
        SNAPSHOT_FAMILY,
        crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA,
        format!("{what}：{got} != {want}"),
    )
}

/// Reject duplicate ids, and (as a convenience) report them in order.
fn check_unique<'a, I>(ids: I, what: &str) -> Result<(), PlatformError>
where
    I: Iterator<Item = &'a str>,
{
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if seen.contains(&id) {
            return Err(PlatformError::new(
                SNAPSHOT_FAMILY,
                E_SNAP_06_MALFORMED_SESSION,
                format!("会话快照载荷畸形：{what} 的 id 重复（{id}）"),
            ));
        }
        seen.push(id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SessionSnapshotDto {
        SessionSnapshotDto {
            schema_version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            cp_meta: CheckpointMetaDto {
                id: 7,
                t_virt: 1_234_567,
                t_host: 987,
                base_seq: 3,
                version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            },
            m12_engine: EngineSnapshotDto::default(),
            m13_state: M13StateDto {
                mem_pages: vec![MemPageDto {
                    base: 0x1000,
                    bytes: vec![1, 2, 3, 4],
                }],
            },
            m14_map: M14MapDto {
                regions: vec![RegionDto {
                    base: 0x1000,
                    size: 0x1000,
                    kind: "ram".to_string(),
                    target: String::new(),
                }],
            },
            m15_periph: M15PeriphDto {
                uarts: vec![UartStateDto {
                    id: "UART0".to_string(),
                    tx: Some(0x41),
                    rx: None,
                    status: 0,
                    irq_line: 1,
                    now_ps: 500,
                }],
                gpios: vec![GpioStateDto {
                    id: "GPIO0".to_string(),
                    directions: 0xF,
                    outputs: 0x1,
                    inputs: 0x2,
                    irq_lines: vec![4, 5],
                    pending_irqs: 0,
                    now_ps: 600,
                }],
                timers: vec![TimerStateDto {
                    id: "TIMER0".to_string(),
                    irq_line: 7,
                    mtime: 900,
                    mtimecmp: 1000,
                }],
            },
            m16_intc: M16IntcDto { raised: vec![1, 3] },
        }
    }

    // ------------------------------------------------------------------
    // 形状
    // ------------------------------------------------------------------

    /// The payload's **shape** is frozen: exact key sets, exact nesting.
    #[test]
    fn the_session_payload_shape_is_pinned_exactly() {
        let json = serde_json::to_value(sample()).expect("json");
        let obj = json.as_object().expect("object");

        let mut top: Vec<&str> = obj.keys().map(String::as_str).collect();
        top.sort_unstable();
        assert_eq!(
            top,
            vec![
                "cp_meta",
                "m12_engine",
                "m13_state",
                "m14_map",
                "m15_periph",
                "m16_intc",
                "schema_version",
            ],
            "会话载荷的顶层键集必须精确固定"
        );

        // 段 0
        let cp = obj
            .get("cp_meta")
            .and_then(|v| v.as_object())
            .expect("cp_meta");
        let mut cpk: Vec<&str> = cp.keys().map(String::as_str).collect();
        cpk.sort_unstable();
        assert_eq!(cpk, vec!["base_seq", "id", "t_host", "t_virt", "version"]);

        // 段 2
        let m13 = obj
            .get("m13_state")
            .and_then(|v| v.as_object())
            .expect("m13");
        let pages = m13
            .get("mem_pages")
            .and_then(|v| v.as_array())
            .expect("pages");
        let page = pages[0].as_object().expect("page");
        let mut pk: Vec<&str> = page.keys().map(String::as_str).collect();
        pk.sort_unstable();
        assert_eq!(pk, vec!["base", "bytes"]);

        // 段 3
        let m14 = obj.get("m14_map").and_then(|v| v.as_object()).expect("m14");
        let regions = m14
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        let region = regions[0].as_object().expect("region");
        let mut rk: Vec<&str> = region.keys().map(String::as_str).collect();
        rk.sort_unstable();
        assert_eq!(rk, vec!["base", "kind", "size", "target"]);

        // 段 4
        let m15 = obj
            .get("m15_periph")
            .and_then(|v| v.as_object())
            .expect("m15");
        let mut m15k: Vec<&str> = m15.keys().map(String::as_str).collect();
        m15k.sort_unstable();
        assert_eq!(m15k, vec!["gpios", "timers", "uarts"]);
        let uarts = m15.get("uarts").and_then(|v| v.as_array()).expect("uarts");
        let uart = uarts[0].as_object().expect("uart");
        let mut uk: Vec<&str> = uart.keys().map(String::as_str).collect();
        uk.sort_unstable();
        assert_eq!(uk, vec!["id", "irq_line", "now_ps", "rx", "status", "tx"]);

        // 段 5
        let m16 = obj
            .get("m16_intc")
            .and_then(|v| v.as_object())
            .expect("m16");
        let mut m16k: Vec<&str> = m16.keys().map(String::as_str).collect();
        m16k.sort_unstable();
        assert_eq!(m16k, vec!["raised"]);
    }

    /// ⚠️ **All six** virtual-time fields travel as **decimal strings**.
    ///
    /// The `u64_decimal` deserialiser accepts **both** forms, so a
    /// silently reverted field does not error — it just goes back to
    /// the v1 wire format (a JSON number). That is exactly the silent
    /// degradation the whole numeric policy exists to remove, and it is
    /// why the assertion has to be on the **shape**, not on "it parses".
    #[test]
    fn every_virtual_time_field_in_the_session_payload_is_a_decimal_string() {
        let json = serde_json::to_value(sample()).expect("json");

        let checks: [(&[&str], &str); 6] = [
            (&["cp_meta", "t_virt"], "cp_meta.t_virt"),
            (&["cp_meta", "t_host"], "cp_meta.t_host"),
            (&["m15_periph", "uarts", "0", "now_ps"], "uarts[0].now_ps"),
            (&["m15_periph", "gpios", "0", "now_ps"], "gpios[0].now_ps"),
            (&["m15_periph", "timers", "0", "mtime"], "timers[0].mtime"),
            (
                &["m15_periph", "timers", "0", "mtimecmp"],
                "timers[0].mtimecmp",
            ),
        ];

        for (path, what) in checks {
            let mut cur = &json;
            let mut ok = true;
            for p in path {
                let next = match p {
                    // an array index
                    s if s.chars().all(|c| c.is_ascii_digit()) => cur
                        .get(s.parse::<usize>().expect("index"))
                        .ok_or(())
                        .map(|v| v),
                    s => cur.get(*s).ok_or(()).map(|v| v),
                };
                match next {
                    Ok(v) => cur = v,
                    Err(()) => {
                        ok = false;
                        break;
                    }
                }
            }
            assert!(ok, "{what} 的路径 {path:?} 在样例载荷里不存在");
            assert!(
                cur.is_string(),
                "{what} 的线上表示必须是**十进制字符串**，实际是 {cur}"
            );
            let text = cur.as_str().expect("刚断言过是字符串");
            assert!(
                text.chars().all(|c| c.is_ascii_digit()),
                "{what} 的字符串形态必须是纯十进制，实际是 {text:?}"
            );
        }
    }

    /// The same six fields, reached by a **different** traversal
    /// (JSON pointer instead of a path array).
    ///
    /// ⚠️ Two assertions that reach the same value by **different
    /// mechanisms** are stronger than one: a bug in the traversal
    /// itself cannot make both go red. (Same principle as the two
    /// `crc` cases — opposite directions, neither can hide the other.)
    #[test]
    fn the_gpio_and_timer_segments_carry_their_time_fields() {
        let json = serde_json::to_value(sample()).expect("json");

        let gpio_now = json
            .pointer("/m15_periph/gpios/0/now_ps")
            .expect("gpio now_ps");
        assert!(gpio_now.is_string(), "gpios[0].now_ps 必须是字符串");

        let mtime = json
            .pointer("/m15_periph/timers/0/mtime")
            .expect("timer mtime");
        assert!(mtime.is_string(), "timers[0].mtime 必须是字符串");

        let mtimecmp = json
            .pointer("/m15_periph/timers/0/mtimecmp")
            .expect("timer mtimecmp");
        assert!(mtimecmp.is_string(), "timers[0].mtimecmp 必须是字符串");
    }

    /// A **float** must not be accepted as a virtual time — `1.5` would
    /// silently truncate to 1.
    #[test]
    fn a_float_is_not_accepted_as_a_virtual_time() {
        let raw = r#"{
          "schema_version": 1,
          "cp_meta": {"id":0,"t_virt":1.5,"t_host":"0","base_seq":0,"version":1},
          "m12_engine": null, "m13_state": null, "m14_map": null,
          "m15_periph": null, "m16_intc": null
        }"#;
        assert!(
            serde_json::from_str::<SessionSnapshotDto>(raw).is_err(),
            "浮点必须被拒——1.5 静默截成 1 就是精度丢失"
        );
    }

    // ------------------------------------------------------------------
    // 校验
    // ------------------------------------------------------------------

    #[test]
    fn a_well_formed_payload_validates() {
        sample().validate().expect("样例载荷应当通过校验");
    }

    #[test]
    fn an_unknown_outer_version_is_rejected() {
        let mut s = sample();
        s.schema_version = 999;
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(
            err.scenario,
            crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA
        );
    }

    #[test]
    fn an_unknown_checkpoint_version_is_rejected() {
        let mut s = sample();
        s.cp_meta.version = 999;
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(
            err.scenario,
            crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA
        );
    }

    #[test]
    fn an_unknown_inner_engine_version_is_rejected() {
        let mut s = sample();
        s.m12_engine.schema_version = 999;
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(
            err.scenario,
            crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA
        );
    }

    #[test]
    fn an_unknown_inner_state_version_is_rejected() {
        let mut s = sample();
        s.m12_engine.state.schema_version = 999;
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(
            err.scenario,
            crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA
        );
    }

    #[test]
    fn unsorted_regions_are_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![
            RegionDto {
                base: 0x2000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            },
            RegionDto {
                base: 0x1000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            },
        ];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn overlapping_regions_are_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![
            RegionDto {
                base: 0x1000,
                size: 0x2000,
                kind: "ram".to_string(),
                target: String::new(),
            },
            RegionDto {
                base: 0x2000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            },
        ];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn unsorted_pages_are_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![RegionDto {
            base: 0,
            size: 0x4000,
            kind: "ram".to_string(),
            target: String::new(),
        }];
        s.m13_state.mem_pages = vec![
            MemPageDto {
                base: 0x2000,
                bytes: vec![0; 4],
            },
            MemPageDto {
                base: 0x1000,
                bytes: vec![0; 4],
            },
        ];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn a_page_outside_every_ram_region_is_rejected() {
        let mut s = sample();
        s.m13_state.mem_pages = vec![MemPageDto {
            base: 0x9000,
            bytes: vec![0; 4],
        }];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn a_page_in_a_rom_region_is_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![RegionDto {
            base: 0x1000,
            size: 0x1000,
            kind: "rom".to_string(),
            target: String::new(),
        }];
        let err = s.validate().expect_err("ROM 里不该有可写的页");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn a_duplicate_peripheral_id_is_rejected() {
        let mut s = sample();
        let uart = s.m15_periph.uarts[0].clone();
        s.m15_periph.uarts = vec![uart.clone(), uart];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn unsorted_raised_irq_lines_are_rejected() {
        let mut s = sample();
        s.m16_intc.raised = vec![3, 1];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    #[test]
    fn duplicate_raised_irq_lines_are_rejected() {
        let mut s = sample();
        s.m16_intc.raised = vec![1, 1];
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// The inner engine snapshot's own rules still apply — a session
    /// payload cannot smuggle in a malformed engine segment.
    #[test]
    fn a_malformed_inner_engine_snapshot_is_rejected() {
        let mut s = sample();
        // A queued event that was already due → the inner validate()
        // rejects it (`E_SNAP_02`).
        s.m12_engine.queue.push(crate::snapshot::QueuedEventDto {
            at_ps: 0,
            seq: 0,
            event: crate::snapshot::SchedEventDto::PeripheralTick {
                paddr: 0x2000_0000,
                at_ps: 0,
            },
        });
        let err = s.validate().expect_err("必须被拒");
        assert_eq!(err.scenario, crate::snapshot::E_SNAP_02_MALFORMED_QUEUE);
    }

    /// ⚠️ **Literal** assertion — the outer version is pinned by a
    /// number, not by a constant.
    ///
    /// If it were symbolic, changing the constant would keep every test
    /// green while the payload format quietly moved. (The same shape of
    /// gap closed D-73 and V-A's high-risk #1.)
    #[test]
    fn the_session_snapshot_schema_version_is_one() {
        assert_eq!(
            SESSION_SNAPSHOT_SCHEMA_VERSION, 1,
            "会话载荷的格式版本必须是 1——改回去会让旧载荷被静默接受"
        );
    }
}
