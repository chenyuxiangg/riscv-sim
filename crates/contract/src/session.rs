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

/// Number of pins in a **GPIO bank** — the single landing spot for that
/// number.
///
/// ⚠️ **One number, one home.** The GPIO's per-pin IRQ list
/// ([`GpioStateDto::irq_lines`]) must have **exactly** this many entries:
/// fewer would leave the remaining pins' IRQ lines undefined, more would
/// carry lines that do not exist. M-15's `PIN_COUNT` **references** this
/// constant instead of spelling out `32` a second time — two copies of
/// the same number is two truths, and the moment one of them moves the
/// payload silently stops validating.
pub const GPIO_PIN_COUNT: usize = 32;

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
    ///
    /// ⚠️ **Exactly [`GPIO_PIN_COUNT`] entries** — one per pin, in pin
    /// order. Fewer leaves the remaining pins' lines undefined, more
    /// carries lines that do not exist; neither is padded nor truncated
    /// (see [`SessionSnapshotDto::validate`]).
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
///
/// ⚠️ **Two different things live here, and they are not the same**:
///
/// * [`Self::raised`] — the **line level**: which interrupt lines are
///   currently *asserted*;
/// * [`Self::pending`] — the **pending queue**: the entries that are
///   waiting to be arbitrated and delivered.
///
/// The invariant between them is **one-directional**: every line in
/// `pending` is also in `raised` (a queued interrupt is an asserted
/// one), but not the other way round. The asymmetry is what lets an
/// **old** payload — one captured while M-16 was still a stub, with
/// lines in `raised` and no queue at all — restore **faithfully**
/// instead of silently losing its state.
///
/// ⚠️ Every field added after the original `raised` is
/// `#[serde(default)]`, so an old payload still deserialises — and the
/// defaults are not merely tolerated, they are **correct** for that
/// era: a stub-era session really had no queue, no enable mask and no
/// active interrupt. The session payload's `schema_version` therefore
/// stays `1` (adjudication F of the frozen spec
/// `docs/detailed-design/STAGE-4_Interrupt_Controller.md` §1.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct M16IntcDto {
    /// Currently raised IRQ lines, ascending and deduplicated.
    #[serde(default)]
    pub raised: Vec<u16>,
    /// The pending queue, in canonical order: `request_time_ps`
    /// ascending, ties broken by `insert_seq` ascending.
    #[serde(default)]
    pub pending: Vec<PendingIrqDto>,
    /// Per-line priorities, ordered by line.
    #[serde(default)]
    pub priorities: Vec<IrqPriorityDto>,
    /// The enable mask, indexed **by line** (bit L = line L).
    #[serde(default)]
    pub enabled_mask: u32,
    /// The interrupt line that was delivered and not yet cleared
    /// ([`crate::csr::NO_ACTIVE_IRQ`] = none).
    #[serde(default = "no_active_irq")]
    pub active_irq: u16,
    /// The timer counter (virtual-time equivalent) — decimal string.
    #[serde(default, with = "crate::numstr::u64_decimal")]
    pub mtime_ps: u64,
    /// The timer compare value — decimal string.
    #[serde(default, with = "crate::numstr::u64_decimal")]
    pub mtimecmp_ps: u64,
}

/// The default for [`M16IntcDto::active_irq`].
///
/// ⚠️ It is [`crate::csr::NO_ACTIVE_IRQ`] and **not** `0`: `0` would
/// read as "line 0 is the active interrupt", and the payload validator
/// would then reject every payload that carries the default (line 0 is
/// not in the queue).
fn no_active_irq() -> u16 {
    crate::csr::NO_ACTIVE_IRQ
}

impl Default for M16IntcDto {
    fn default() -> Self {
        Self {
            raised: Vec::new(),
            pending: Vec::new(),
            priorities: Vec::new(),
            enabled_mask: 0,
            active_irq: no_active_irq(),
            mtime_ps: 0,
            mtimecmp_ps: 0,
        }
    }
}

/// One entry of the interrupt controller's pending queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingIrqDto {
    /// The interrupt line.
    pub line: u16,
    /// The line's priority (higher wins).
    pub prio: u8,
    /// When the line was requested (virtual time) — decimal string.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub request_time_ps: u64,
    /// The insertion counter — the tie-breaker that makes the queue
    /// order **deterministic** when two lines are requested at the same
    /// virtual time.
    pub insert_seq: u64,
    /// Trigger mode: `0` = level, `1` = edge.
    pub trigger_type: u8,
    /// The request source: `"uart"` / `"gpio"` / `"timer"` /
    /// `"platform"`.
    pub source_type: String,
}

/// One line's priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrqPriorityDto {
    /// The interrupt line.
    pub line: u16,
    /// The line's priority (higher wins).
    pub prio: u8,
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
    /// 4. inner `m12_engine.state.schema_version` — known? (**the inner
    ///    validator is its single home** — see the note below)
    /// 5. `m14_map.regions` — ascending by `base`, non-overlapping
    /// 6. `m13_state.mem_pages` — ascending by `base`, non-overlapping,
    ///    and each page fits inside a RAM region
    /// 7. `m15_periph` — no duplicate id, **across kinds** as well
    /// 8. `m15_periph.gpios[*].irq_lines` — exactly [`GPIO_PIN_COUNT`]
    ///    entries
    /// 9. `m16_intc.raised` — ascending, no duplicates
    /// 10. the inner engine snapshot validates on its own terms
    /// 11. `m16_intc.pending` — one entry per line, in canonical
    ///     `(request_time_ps, insert_seq)` order
    /// 12. `m16_intc.priorities` — ascending by line, no duplicates
    /// 13. `m16_intc.active_irq` — either "none" or a line that **is**
    ///     in the pending queue
    /// 14. `m16_intc.pending` — every queued line is also in `raised`
    ///
    /// ⚠️ Checks **11 ~ 14** were added by the M-16 landing batch
    /// (frozen spec `docs/detailed-design/STAGE-4_Interrupt_Controller.md`
    /// §2.4). They describe the **queue** the interrupt controller
    /// acquired when it stopped being a stub; before that, segment 5
    /// carried only the asserted lines.
    ///
    /// ⚠️ The numbers are the **frozen spec's** (§5) — the same numbers
    /// the spec's table uses, so a reader can line the two up.
    ///
    /// ⚠️ The inner `m12_engine.state.schema_version` is **not** checked
    /// here — see the comment at the check itself. The frozen spec
    /// §5 lists it; the inner validator is its single home.
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
        // ⚠️ **No check for `m12_engine.state.schema_version` here on
        // purpose** — the inner `EngineSnapshotDto::validate()` already
        // rejects it with the **same** scenario code
        // (`E_SNAP_05_UNSUPPORTED_STATE_SCHEMA`, see
        // `crate::snapshot`). A second copy of the same rule would be a
        // second landing spot for one constraint: dropping either copy
        // would leave the behaviour unchanged, so neither could be
        // covered by a discriminating test. The inner check is the
        // single home for that rule.
        //
        // ⚠️ The engine-segment version above **is** still checked here,
        // because the inner one reports a *different* code
        // (`E_SNAP_01_UNSUPPORTED_SCHEMA`); the outer check decides
        // which code the session payload surfaces.

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

        // 7 — no duplicate peripheral id — **across kinds** as well.
        //
        // ⚠️ A UART and a GPIO may not share an id: the id is the
        // peripheral's **identity**, and the restore path matches the
        // payload onto the live instances **by id**. Two entries with
        // one id = one of them silently wins (or both are written into
        // the same instance) → the restored session differs from the
        // captured one, with nothing to show for it.
        check_unique(
            self.m15_periph
                .uarts
                .iter()
                .map(|u| (u.id.as_str(), "UART"))
                .chain(
                    self.m15_periph
                        .gpios
                        .iter()
                        .map(|g| (g.id.as_str(), "GPIO")),
                )
                .chain(
                    self.m15_periph
                        .timers
                        .iter()
                        .map(|t| (t.id.as_str(), "定时器")),
                ),
        )?;

        // 8 — every GPIO bank must carry **exactly one IRQ line per pin**.
        //
        // ⚠️ Neither padded nor truncated: padding fabricates lines the
        // firmware never wrote, truncation drops lines it did. Both are
        // rejected.
        for g in &self.m15_periph.gpios {
            if g.irq_lines.len() != GPIO_PIN_COUNT {
                return Err(self.malformed(&format!(
                    "GPIO '{}' 的 irq_lines 有 {} 条，应恰好 {GPIO_PIN_COUNT} 条（不补齐也不截断）",
                    g.id,
                    g.irq_lines.len()
                )));
            }
        }

        // 9 — the raised lines must be ascending and deduplicated.
        let mut prev: Option<u16> = None;
        for line in &self.m16_intc.raised {
            if let Some(p) = prev {
                if *line <= p {
                    return Err(self.malformed("已拉起的 IRQ 行未升序或有重复"));
                }
            }
            prev = Some(*line);
        }

        // 11 — the pending queue: one entry per line (the controller
        // deduplicates a repeated request), in canonical order.
        //
        // The canonical order is `(request_time_ps, insert_seq)` — the
        // same key the controller itself uses, so "canonically ordered"
        // here means "in the order the controller would have produced".
        let mut queued_lines: Vec<u16> = Vec::new();
        let mut prev_key: Option<(u64, u64)> = None;
        for entry in &self.m16_intc.pending {
            if queued_lines.contains(&entry.line) {
                return Err(self.malformed(&format!(
                    "挂起队列里 IRQ 行 {} 出现了两次（同线去重是控制器的不变式）",
                    entry.line
                )));
            }
            queued_lines.push(entry.line);
            let key = (entry.request_time_ps, entry.insert_seq);
            if let Some(prev) = prev_key {
                if key <= prev {
                    return Err(self.malformed(&format!(
                        "挂起队列未按 (请求时刻, 插入序) 升序：({}) 跟在 ({}) 后面",
                        key.0, prev.0
                    )));
                }
            }
            prev_key = Some(key);
        }

        // 12 — the priority table: one entry per line, ordered by line.
        let mut prev_line: Option<u16> = None;
        for pr in &self.m16_intc.priorities {
            if let Some(p) = prev_line {
                if pr.line <= p {
                    return Err(self.malformed(&format!(
                        "优先级表未按 IRQ 行升序或有重复：行 {} 跟在行 {} 后面",
                        pr.line, p
                    )));
                }
            }
            prev_line = Some(pr.line);
        }

        // 13 — the delivered-and-not-cleared line must still be queued.
        //
        // An active line that is *not* in the queue would be a line the
        // controller can neither re-deliver nor clear — a payload that
        // describes a state the controller cannot be in.
        if self.m16_intc.active_irq != crate::csr::NO_ACTIVE_IRQ
            && !queued_lines.contains(&self.m16_intc.active_irq)
        {
            return Err(self.malformed(&format!(
                "已交付但未清除的 IRQ 行 {} 不在挂起队列里",
                self.m16_intc.active_irq
            )));
        }

        // 14 — every queued line must also be asserted.
        //
        // The invariant is **one-directional** (`pending ⊆ raised`), not
        // equality: a payload captured while M-16 was a stub carries
        // asserted lines and an empty queue, and that is a *faithful*
        // description of that era's state.
        for entry in &self.m16_intc.pending {
            if !self.m16_intc.raised.contains(&entry.line) {
                return Err(self.malformed(&format!(
                    "挂起队列里的 IRQ 行 {} 没有被拉起（队列是拉起行的子集）",
                    entry.line
                )));
            }
        }

        // 10 — the inner engine snapshot validates on its own terms.
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

/// Reject duplicate peripheral ids, and report them with the kind they
/// came from.
///
/// ⚠️ The check spans **all three kinds**: an id that is a UART *and* a
/// GPIO is a duplicate too. The restore path matches by id, so one id
/// with two entries is two states competing for one instance.
fn check_unique<'a, I>(ids: I) -> Result<(), PlatformError>
where
    I: Iterator<Item = (&'a str, &'a str)>,
{
    let mut seen: Vec<&str> = Vec::new();
    for (id, kind) in ids {
        if seen.contains(&id) {
            return Err(PlatformError::new(
                SNAPSHOT_FAMILY,
                E_SNAP_06_MALFORMED_SESSION,
                format!(
                    "会话快照载荷畸形：{kind} 的 id 重复（{id}）——同一个 id 不能既是 UART 又是 GPIO"
                ),
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
                    irq_lines: (0..GPIO_PIN_COUNT).map(|i| 4 + i as u16).collect(),
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
            m16_intc: M16IntcDto {
                raised: vec![1, 3],
                ..M16IntcDto::default()
            },
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
        let mut m13k: Vec<&str> = m13.keys().map(String::as_str).collect();
        m13k.sort_unstable();
        assert_eq!(m13k, vec!["mem_pages"], "m13_state 的键集必须精确固定");
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
        let mut m14k: Vec<&str> = m14.keys().map(String::as_str).collect();
        m14k.sort_unstable();
        assert_eq!(m14k, vec!["regions"], "m14_map 的键集必须精确固定");
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

        // ⚠️ **GPIO 与定时器也要钉**——它们各自带**时间字段**
        // （`now_ps` / `mtime` / `mtimecmp`），不钉住键集就等于给
        // 「往里塞一个新字段」开了后门（V-B 检视的 C-1）。
        let gpios = m15.get("gpios").and_then(|v| v.as_array()).expect("gpios");
        let gpio = gpios[0].as_object().expect("gpio");
        let mut gk: Vec<&str> = gpio.keys().map(String::as_str).collect();
        gk.sort_unstable();
        assert_eq!(
            gk,
            vec![
                "directions",
                "id",
                "inputs",
                "irq_lines",
                "now_ps",
                "outputs",
                "pending_irqs",
            ],
            "GPIO 状态的键集必须精确固定——多一个字段就是载荷形状变了"
        );

        let timers = m15
            .get("timers")
            .and_then(|v| v.as_array())
            .expect("timers");
        let timer = timers[0].as_object().expect("timer");
        let mut tk: Vec<&str> = timer.keys().map(String::as_str).collect();
        tk.sort_unstable();
        assert_eq!(
            tk,
            vec!["id", "irq_line", "mtime", "mtimecmp"],
            "定时器状态的键集必须精确固定——多一个字段就是载荷形状变了"
        );

        // 段 5
        let m16 = obj
            .get("m16_intc")
            .and_then(|v| v.as_object())
            .expect("m16");
        let mut m16k: Vec<&str> = m16.keys().map(String::as_str).collect();
        m16k.sort_unstable();
        assert_eq!(
            m16k,
            vec![
                "active_irq",
                "enabled_mask",
                "mtime_ps",
                "mtimecmp_ps",
                "pending",
                "priorities",
                "raised",
            ],
            "段 5 的键集必须精确固定——M-16 落地后段 5 带的是整个控制器状态"
        );
    }

    /// ⚠️ **All eight** virtual-time fields travel as **decimal strings**.
    ///
    /// The `u64_decimal` deserialiser accepts **both** forms, so a
    /// silently reverted field does not error — it just goes back to
    /// the v1 wire format (a JSON number). That is exactly the silent
    /// degradation the whole numeric policy exists to remove, and it is
    /// why the assertion has to be on the **shape**, not on "it parses".
    ///
    /// ⚠️ The **count** is pinned separately, by
    /// [`exactly_eight_virtual_time_fields_carry_the_decimal_attribute`]
    /// — the per-field assertions above would all still pass if a
    /// **ninth** field quietly joined the set, and the spec's "eight"
    /// would quietly become a lie.
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
                    s if s.chars().all(|c| c.is_ascii_digit()) => {
                        cur.get(s.parse::<usize>().expect("index"))
                    }
                    s => cur.get(*s),
                };
                match next {
                    Some(v) => cur = v,
                    None => {
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

    /// ⚠️ **语义**判别式——钉的是**线上表示**的集合，不是源码里的
    /// 属性行数。
    ///
    /// ⚠️ **旧写法是排版耦合的**：`include_str!("session.rs")` + 数
    /// `starts_with("#[serde(with")` 的行——把一个属性改成**多行**写法
    /// （语义完全不变）就会让门禁变红。一次纯 `rustfmt` 就可能这么排
    /// → 一次纯格式化就能把门禁弄红，那是**假红**。
    ///
    /// 现在的口径是**线上表示**：带
    /// `#[serde(with = "crate::numstr::u64_decimal")]` 的字段，线上必然是
    /// **十进制字符串**；不带属性的 `u64`，线上是 JSON **数字**。于是
    /// 「带该属性的字段集合」与「线上形态是十进制字符串的字段集合」是
    /// **同一个集合**——后者与排版无关。
    ///
    /// ⚠️ **夹具的不变量**：样例载荷里 UART / GPIO / 定时器**各恰好
    /// 一台** → 值的个数 == 字段声明的个数。（多一台 UART 就会多出第
    /// 七个十进制字符串 → 本用例变红：这是**故意**的，夹具是本用例
    /// 契约的一部分。）
    ///
    /// ⚠️ **两处刻意的排除**：
    /// ① 段 1（`m12_engine`）——它是**冻结的**内层快照，数值策略与版本链
    ///    在 `snapshot.rs` / `model.rs` 里；规格 §4 的「6 个」说的是
    ///    **本文件**（`session.rs`）里的字段。
    /// ② `id` / `kind` / `target` —— 载荷里除时间字段外只有这三个
    ///    **非时间**字符串字段（形状守卫已钉住它们）；按**键名**排除，
    ///    夹具里的 id 就算变成纯数字也不会污染计数。
    #[test]
    fn exactly_eight_virtual_time_fields_carry_the_decimal_attribute() {
        let json = serde_json::to_value(sample()).expect("json");
        let top = json.as_object().expect("object");

        let mut paths: Vec<String> = Vec::new();
        for (key, value) in top {
            if key == "m12_engine" {
                continue;
            }
            decimal_string_paths(value, key, &mut paths);
        }
        paths.sort();

        assert_eq!(
            paths.len(),
            8,
            "会话载荷里带十进制字符串属性的字段必须恰好 8 个 \
             （cp_meta.t_virt / cp_meta.t_host / uarts[0].now_ps / \
             gpios[0].now_ps / timers[0].mtime / timers[0].mtimecmp / \
             m16_intc.mtime_ps / m16_intc.mtimecmp_ps）\
             ——实际线上是十进制字符串的字段：{paths:?}"
        );
        assert_eq!(
            paths,
            vec![
                "cp_meta.t_host",
                "cp_meta.t_virt",
                "m15_periph.gpios.0.now_ps",
                "m15_periph.timers.0.mtime",
                "m15_periph.timers.0.mtimecmp",
                "m15_periph.uarts.0.now_ps",
                "m16_intc.mtime_ps",
                "m16_intc.mtimecmp_ps",
            ],
            "十进制字符串字段的集合必须**恰好**是规格 §4 表里的那八个——\
             多一个或少一个，「8 个」这句话就是空话"
        );
    }

    /// Collect the JSON paths of every leaf whose **wire form** is a
    /// decimal string — the form `u64_decimal` produces.
    ///
    /// ⚠️ The three **non-time** string keys (`id` / `kind` / `target`)
    /// are skipped **by key name**: they are the payload's only other
    /// string fields (the shape guard already pins them), and skipping
    /// them keeps a fixture whose id happens to be numeric from
    /// polluting the count.
    fn decimal_string_paths(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    if matches!(key.as_str(), "id" | "kind" | "target") {
                        continue;
                    }
                    let path = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    decimal_string_paths(child, &path, out);
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    decimal_string_paths(child, &format!("{prefix}.{index}"), out);
                }
            }
            serde_json::Value::String(text)
                if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) =>
            {
                out.push(prefix.to_string());
            }
            _ => {}
        }
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

    /// ⚠️ **升序但重叠**的内存页必须被拒——这是规格 §5 第 6 类
    /// 「按 base 升序**且不重叠**」的后半句。
    ///
    /// ⚠️ 既有用例 `unsorted_pages_are_rejected` 只造了**乱序**
    /// （`0x2000` 在前、`0x1000` 在后）——那种载荷在「只查升序」的
    /// 实现下**照样**被拒 → 判别力缺口：把页表的重叠检查删掉
    /// （`prev_page_end` 存 `base` 而不是 `end`），**没有一条用例会红**。
    ///
    /// ⚠️ 本用例的夹具刻意**只含这一个畸形**：两页都完整落在 RAM 区域
    /// 内（否则「不落在任何 RAM 区域内」那条检查会**替**重叠检查接住
    /// 载荷，变异就伪装成绿的）。
    #[test]
    fn overlapping_pages_are_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![RegionDto {
            base: 0,
            size: 0x4000,
            kind: "ram".to_string(),
            target: String::new(),
        }];
        // 0x1000..0x2800 与 0x2000..0x3000 —— **按 base 升序**，但互相重叠。
        s.m13_state.mem_pages = vec![
            MemPageDto {
                base: 0x1000,
                bytes: vec![0; 0x1800],
            },
            MemPageDto {
                base: 0x2000,
                bytes: vec![0; 0x1000],
            },
        ];
        let err = s.validate().expect_err("升序但重叠的内存页必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// ⚠️ **大小为 0 的区域**必须被拒——零长度的区间在内存图里是
    /// **无法落地的地址空间**（谁落在里面？）。
    ///
    /// ⚠️ 夹具刻意**清空内存页**：否则页面会因为「不落在任何 RAM 区域
    /// 内」被拒——**同一个错误码**（`E_SNAP_06`）却由**另一条**检查
    /// 报出 → 删掉本规则后用例照样绿 → 判别力缺口。
    #[test]
    fn a_zero_sized_region_is_rejected() {
        let mut s = sample();
        s.m14_map.regions = vec![RegionDto {
            base: 0x1000,
            size: 0,
            kind: "ram".to_string(),
            target: String::new(),
        }];
        s.m13_state.mem_pages.clear();
        let err = s.validate().expect_err("大小为 0 的区域必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// ⚠️ **`base + size` 越过 u32** 的区域必须被拒——区间的**上界**
    /// 算不出来 = 这条区域**没有上界**。
    ///
    /// ⚠️ 夹具同样**清空内存页**：页面落不进这个区域，会由「不落在
    /// 任何 RAM 区域内」那条检查报出**同一个错误码** → 掩盖本规则。
    #[test]
    fn a_region_whose_end_overflows_u32_is_rejected() {
        let mut s = sample();
        // 0xFFFF_F000 + 0x2000 = 0x1_0000_1000 > u32::MAX
        s.m14_map.regions = vec![RegionDto {
            base: 0xFFFF_F000,
            size: 0x2000,
            kind: "ram".to_string(),
            target: String::new(),
        }];
        s.m13_state.mem_pages.clear();
        let err = s.validate().expect_err("上界越过 u32 的区域必须被拒");
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

    /// ⚠️ **跨类型**的 id 重复也是重复——同一个 id 不能既是 UART 又是
    /// GPIO。
    ///
    /// 恢复路径**按 id** 把载荷匹配到活实例上：一个 id 挂两条状态 =
    /// 两条状态抢一个实例（或者两条都被写进同一个实例）→ 还原出来的
    /// 会话与采出来的那份**不一样**，而载荷里没有任何东西能提示这件事。
    #[test]
    fn a_peripheral_id_duplicated_across_kinds_is_rejected() {
        let mut s = sample();
        // GPIO 冒用了 UART 的 id。
        s.m15_periph.gpios[0].id = s.m15_periph.uarts[0].id.clone();
        let err = s.validate().expect_err("跨类型的 id 重复必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 定时器与 UART 同名 —— 同一条规则，**另一对**类型。
    #[test]
    fn a_peripheral_id_duplicated_between_a_timer_and_a_uart_is_rejected() {
        let mut s = sample();
        s.m15_periph.timers[0].id = s.m15_periph.uarts[0].id.clone();
        let err = s.validate().expect_err("跨类型的 id 重复必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// GPIO 的每引脚 IRQ 线**少一条** → 拒。
    ///
    /// ⚠️ **不补齐**：补齐 = 造出固件没写过的事实。
    #[test]
    fn a_gpio_with_too_few_irq_lines_is_rejected() {
        let mut s = sample();
        s.m15_periph.gpios[0].irq_lines = vec![1, 2, 3];
        let err = s.validate().expect_err("irq_lines 少一条必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// GPIO 的每引脚 IRQ 线**多一条** → 拒。
    ///
    /// ⚠️ **不截断**：截断 = 丢状态。
    #[test]
    fn a_gpio_with_too_many_irq_lines_is_rejected() {
        let mut s = sample();
        s.m15_periph.gpios[0].irq_lines = vec![0xFFFF; GPIO_PIN_COUNT + 1];
        let err = s.validate().expect_err("irq_lines 多一条必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 恰好**每引脚一条** → 载荷通过校验。
    ///
    /// ⚠️ 这条是上面两条的**反面**：只有「少一条 / 多一条都拒」而没有
    /// 「刚好一条放行」，那两条断言就分不清「校验器在数条数」和「校验器
    /// 什么都拒」。
    #[test]
    fn a_gpio_with_exactly_one_irq_line_per_pin_is_accepted() {
        let mut s = sample();
        s.m15_periph.gpios[0].irq_lines = vec![0xFFFF; GPIO_PIN_COUNT];
        s.validate()
            .expect("恰好每引脚一条的 GPIO 状态必须通过校验");
    }

    /// ⚠️ **数字由常量自己说**：引脚数改了，这条用例的「恰好」也要跟着
    /// 变——所以断言的是**条数**，不是字面量 `32`。
    ///
    /// 这条钉住的是「契约层导出的那个数」与「载荷的形状」是**同一个数**：
    /// 把 `GPIO_PIN_COUNT` 改成 16 之后，M-15 的 `PIN_COUNT` 必须跟着变
    /// （它引用的是这个常量），否则 GPIO 的导出就会带 32 条线而契约层
    /// 只认 16 条 ——**载荷再也过不了自己的校验**。
    #[test]
    fn the_gpio_pin_count_is_the_single_landing_spot() {
        assert_eq!(GPIO_PIN_COUNT, 32, "MVP 的 GPIO  Bank 是 32 引脚");
        let mut s = sample();
        s.m15_periph.gpios[0].irq_lines = vec![0xFFFF; GPIO_PIN_COUNT];
        s.validate()
            .expect("按契约层导出的引脚数构造的载荷必须通过校验");
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

    /// ⚠️ **Literal** assertion — the malformed-payload code must not
    /// collide with the queue-malformed code.
    ///
    /// `E_SNAP_02` means "the *event queue* is malformed" and
    /// `E_SNAP_06` means "the *session payload* is malformed". If the
    /// two ever share a number, a consumer reading the code can no
    /// longer tell which table to look at — **one code, two meanings**
    /// (the same trap as D-69's `E_PP` / `E_PER`).
    #[test]
    fn the_malformed_session_code_is_its_own() {
        assert_eq!(
            E_SNAP_06_MALFORMED_SESSION, 6,
            "会话载荷畸形的场景码必须是 6"
        );
        assert_ne!(
            E_SNAP_06_MALFORMED_SESSION,
            crate::snapshot::E_SNAP_02_MALFORMED_QUEUE,
            "「会话载荷畸形」与「事件队列畸形」不能共用一个码"
        );
        assert_ne!(
            E_SNAP_06_MALFORMED_SESSION,
            crate::snapshot::E_SNAP_05_UNSUPPORTED_STATE_SCHEMA,
            "「会话载荷畸形」与「版本不受支持」不能共用一个码"
        );
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

    // ------------------------------------------------------------------
    // 段 5 的扩展（M-16 落地批 · STAGE-4 §2.4）
    // ------------------------------------------------------------------

    /// **旧载荷**（M-16 还是 stub 的时候捕获的）**仍然能反序列化**。
    ///
    /// ⚠️ 这一条是「加字段」能不能成立的**前提**：段 5 的形状变了，
    /// 但会话载荷的 `schema_version` **没有**升 —— 前提就是旧载荷
    /// 读得回来。
    ///
    /// ⚠️ 而且不只是「读得回来」：默认值必须是**那个时代的真实状态**
    /// （没有队列 / 没有使能掩码 / 没有活动中断），否则「读得回来但
    /// 读出来是错的」比「读不回来」更糟。
    #[test]
    fn an_old_segment_5_payload_still_deserialises() {
        let json = r#"{"raised":[1,3]}"#;
        let dto: M16IntcDto = serde_json::from_str(json).expect("旧形状必须能读");
        assert_eq!(dto.raised, vec![1, 3]);
        assert!(
            dto.pending.is_empty(),
            "旧载荷没有队列——那正是 stub 时代的真实状态"
        );
        assert_eq!(dto.enabled_mask, 0);
        assert_eq!(dto.mtime_ps, 0);
        assert_eq!(dto.mtimecmp_ps, 0);
    }

    /// 「没有活动中断」的标记**不是 0**。
    ///
    /// ⚠️ 如果它是 0，那么每个带默认值的载荷都会被校验器拒掉
    /// （「活动中断行 0 不在队列里」）——而那会让**所有**旧载荷失效。
    #[test]
    fn the_default_active_irq_is_the_no_active_marker() {
        let dto = M16IntcDto::default();
        assert_eq!(dto.active_irq, crate::csr::NO_ACTIVE_IRQ);
        assert_ne!(dto.active_irq, 0, "0 会被读成「行 0 是活动中断」");
    }

    /// 虚拟时间字段走**十进制字符串**（契约 v2 §2.2）。
    ///
    /// ⚠️ 这是「判别式落在字段的**线上表示**上」：断言的是 JSON 里的
    /// 文本形状，不是「值对不对」。
    #[test]
    fn the_segment_5_virtual_time_fields_travel_as_decimal_strings() {
        let dto = M16IntcDto {
            mtime_ps: 9_007_199_254_740_992,
            mtimecmp_ps: 42,
            ..M16IntcDto::default()
        };
        let json = serde_json::to_string(&dto).expect("序列化必须成功");
        assert!(
            json.contains(r#""mtime_ps":"9007199254740992""#),
            "mtime_ps 必须走十进制字符串：{json}"
        );
        assert!(
            json.contains(r#""mtimecmp_ps":"42""#),
            "mtimecmp_ps 必须走十进制字符串：{json}"
        );
        let back: M16IntcDto = serde_json::from_str(&json).expect("往返必须成功");
        assert_eq!(back.mtime_ps, dto.mtime_ps);
        assert_eq!(back.mtimecmp_ps, dto.mtimecmp_ps);
    }

    /// 段 5 的**字段名**在 JSON 上是稳定的（载荷是**公开**格式）。
    #[test]
    fn the_segment_5_field_names_are_stable() {
        let dto = M16IntcDto::default();
        let json = serde_json::to_string(&dto).expect("序列化必须成功");
        for key in [
            "raised",
            "pending",
            "priorities",
            "enabled_mask",
            "active_irq",
            "mtime_ps",
            "mtimecmp_ps",
        ] {
            assert!(
                json.contains(&format!("\"{key}\"")),
                "段 5 必须带字段 `{key}`：{json}"
            );
        }
    }

    /// 构造一个**合法**的段 5 —— 用来给「拒」的用例当反面。
    fn well_formed_segment_5() -> M16IntcDto {
        M16IntcDto {
            raised: vec![1, 3],
            pending: vec![
                PendingIrqDto {
                    line: 1,
                    prio: 1,
                    request_time_ps: 100,
                    insert_seq: 1,
                    trigger_type: 1,
                    source_type: "uart".to_string(),
                },
                PendingIrqDto {
                    line: 3,
                    prio: 2,
                    request_time_ps: 200,
                    insert_seq: 2,
                    trigger_type: 0,
                    source_type: "timer".to_string(),
                },
            ],
            priorities: vec![
                IrqPriorityDto { line: 1, prio: 1 },
                IrqPriorityDto { line: 3, prio: 2 },
            ],
            enabled_mask: 0b1010,
            active_irq: crate::csr::NO_ACTIVE_IRQ,
            mtime_ps: 500,
            mtimecmp_ps: 1_000,
        }
    }

    /// 合法形状**通过**校验 —— 上面每一条「拒」的用例都需要这条做对照，
    /// 否则「校验器什么都拒」也能让它们全绿。
    #[test]
    fn a_well_formed_segment_5_is_accepted() {
        let mut s = sample();
        s.m16_intc = well_formed_segment_5();
        s.validate().expect("合法形状必须通过校验");
    }

    /// 挂起队列里**同一条线出现两次** → 拒。
    ///
    /// ⚠️ 同线去重是控制器的不变式：一条线在队列里只能有一条。
    #[test]
    fn a_duplicated_pending_line_is_rejected() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        let dup = seg.pending[0].clone();
        seg.pending[1] = dup;
        s.m16_intc = seg;
        let err = s.validate().expect_err("同线重复必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 挂起队列**顺序不对** → 拒。
    ///
    /// ⚠️ 判别式落在**排序键** `(请求时刻, 插入序)` 上：把两条的
    /// 请求时刻对调，顺序就反了。
    #[test]
    fn an_out_of_order_pending_queue_is_rejected() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        seg.pending.swap(0, 1);
        s.m16_intc = seg;
        let err = s.validate().expect_err("队列顺序不对必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 优先级表**顺序不对** → 拒。
    #[test]
    fn an_unsorted_priority_table_is_rejected() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        seg.priorities.swap(0, 1);
        s.m16_intc = seg;
        let err = s.validate().expect_err("优先级表顺序不对必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 「已交付未清除」的行**不在队列里** → 拒。
    ///
    /// ⚠️ 这种载荷描述的状态控制器**到不了**：一条既不能被再次交付、
    /// 也不能被清除的行。
    #[test]
    fn an_active_line_that_is_not_queued_is_rejected() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        seg.active_irq = 2; // 行 2 既不在队列里，也不在 raised 里
        s.m16_intc = seg;
        let err = s.validate().expect_err("活动行不在队列里必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 队列里的行**没有被拉起** → 拒。
    ///
    /// ⚠️ 这条守的是**单向**不变式 `pending ⊆ raised` 的方向：
    /// 「队列里的行必须是被断言的」成立，而反向**不**要求成立
    /// （stub 时代的载荷就是「拉起了若干行、队列是空的」）。
    #[test]
    fn a_queued_line_that_is_not_raised_is_rejected() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        seg.raised = vec![3]; // 队列里有行 1，但行 1 没被拉起
        s.m16_intc = seg;
        let err = s.validate().expect_err("队列里的行没被拉起必须被拒");
        assert_eq!(err.scenario, E_SNAP_06_MALFORMED_SESSION);
    }

    /// 反向**不**要求成立：`raised` 可以有队列里没有的行。
    ///
    /// ⚠️ 这是上面那条的**反面**，也是「旧载荷必须还能过校验」这条
    /// 裁决的落点 —— 如果这个方向也被拒，所有 stub 时代的载荷都会
    /// 变成废载荷。
    #[test]
    fn a_raised_line_without_a_queue_entry_is_accepted() {
        let mut s = sample();
        let mut seg = well_formed_segment_5();
        seg.pending.clear();
        seg.raised = vec![1, 3];
        seg.active_irq = crate::csr::NO_ACTIVE_IRQ;
        s.m16_intc = seg;
        s.validate()
            .expect("「拉起了但没排队」是 stub 时代的真实状态，必须放行");
    }
}
