//! Five core data models (LLD §2.5, AD §7).
//!
//! These are the public DTOs that flow on the protocol wire. The five
//! families:
//! - [`Project`]   — product tree (M-01) + persistent identity (M-23)
//! - [`SimState`]  — full simulation snapshot (M-12/M-13/M-14 + M-20)
//! - [`DebugMeta`] — symbols, breakpoints, watch points (M-08, M-10, M-11)
//! - [`Waveform`]  — edge-encoded time series (M-18, M-19)
//! - [`Message`]   — see [`crate::protocol`]

use serde::{Deserialize, Serialize};

/// RISC-V ISA base covered by the in-tree ISS.
pub const ISA: &str = "RV32IMC";

/// Project = product tree + persistent identity (FR-13, M-23).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Stable project id (UUID-v4).
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Product tree (root board → chip → peripherals). Modeled in M-01.
    pub tree: ProductTree,
    /// Initial RAM / ROM image references (parsed by M-06).
    pub images: Vec<ImageRef>,
    /// Toolchain / project version metadata.
    pub version: ProjectVersion,
}

impl Project {
    /// Validate that the project references a known ISA.
    pub fn validate_isa(&self) -> Result<(), &'static str> {
        if self.version.isa == ISA {
            Ok(())
        } else {
            Err("project ISA does not match the kernel's supported ISA")
        }
    }
}

/// Top-down product hierarchy used by the product workbench (M-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProductTree {
    /// Root board node.
    pub root: TreeNode,
}

/// A single node in the product tree. The same DTO is reused for the 2D
/// schematic view and the P1 3D view (M-01 §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeNode {
    /// Display name (e.g. "MainBoard", "GD32VF103", "UART0").
    pub name: String,
    /// Optional chip / peripheral identifier.
    pub component_id: Option<String>,
    /// Child nodes (chips, peripherals, sub-boards).
    pub children: Vec<TreeNode>,
}

impl Default for TreeNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            component_id: None,
            children: Vec::new(),
        }
    }
}

/// Reference to a loadable image. The kernel will hand this to M-06
/// (ELF/Raw-HEX parser) to obtain a binary blob to load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    /// Image name (e.g. "firmware.elf").
    pub name: String,
    /// Image kind.
    pub kind: ImageKind,
    /// Bytes or URL — depending on transport. For MVP, the host (browser or
    /// server) resolves the URL and hands the kernel raw bytes.
    pub source: ImageSource,
}

impl Default for ImageRef {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: ImageKind::Elf,
            source: ImageSource::Bytes(Vec::new()),
        }
    }
}

/// Image format selector (M-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageKind {
    /// ELF file (RISC-V).
    Elf,
    /// Raw HEX / binary blob.
    RawHex,
}

impl Default for ImageKind {
    fn default() -> Self {
        Self::Elf
    }
}

/// Where the kernel should read the image bytes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageSource {
    /// Image is already loaded as raw bytes (browser file drop / server
    /// upload). The kernel never re-fetches the URL.
    Bytes(Vec<u8>),
    /// Reserved for P1: image is fetched from a URL by the host. Today the
    /// host must resolve it and pass [`ImageSource::Bytes`].
    Url(String),
}

impl Default for ImageSource {
    fn default() -> Self {
        Self::Bytes(Vec::new())
    }
}

/// Project version metadata (ISA, toolchain, schema).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectVersion {
    /// Project schema version, monotonically increasing.
    pub schema: u32,
    /// ISA base, e.g. `"RV32IMC"`. Validated against [`ISA`].
    pub isa: String,
    /// Free-form toolchain identifier (e.g. `"gcc-rv32imc 13.2"`).
    pub toolchain: String,
}

impl Default for ProjectVersion {
    fn default() -> Self {
        Self {
            schema: 1,
            isa: ISA.to_string(),
            toolchain: String::new(),
        }
    }
}

/// Format version of the [`SimState`] structure itself.
///
/// Distinct from [`crate::snapshot::SNAPSHOT_SCHEMA_VERSION`], which
/// versions the *snapshot DTO family* that embeds a [`SimState`]. Both
/// versions travel inside one snapshot document and **both** are checked
/// by [`crate::snapshot::EngineSnapshotDto::validate`].
///
/// Bump on every incompatible change to the field set of [`SimState`].
pub const SIM_STATE_SCHEMA_VERSION: u32 = 1;

/// SimState = everything needed to deterministically resume a paused
/// simulation (M-20). The fields are intentionally serializable so the
/// snapshot can be persisted to IndexedDB (MVP) or a server store (P1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimState {
    /// Schema version of the [`SimState`] format — see
    /// [`SIM_STATE_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Virtual time (picoseconds) at the snapshot point.
    ///
    /// Travels as a **decimal string** — see [`crate::numstr`] and the
    /// contract v2 spec §2.2: a `u64` would become a JSON number, and a
    /// JSON number is exact only up to `2^53` ps ≈ 2.5 hours of simulated
    /// time, which a 1 µs quantum reaches in a long run.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub virtual_time_ps: u64,
    /// ISS general-purpose registers (x0..x31, 32-bit each).
    pub gpr: [u32; 32],
    /// Program counter.
    pub pc: u32,
    /// CSR file (placeholder: concrete CSR set depends on M-13 detail).
    pub csrs: Vec<(u16, u32)>,
    /// Pending memory writes from the current quantum.
    pub pending_writes: Vec<PendingWrite>,
    /// Monotonic **insertion counter** for the scheduler's event queue.
    ///
    /// It is *not* a pointer into the queue — [`SimState`] itself has no
    /// room for the queue. M-12 uses it as the next-event `seq` floor, so
    /// a restored state cannot reuse a sequence number that is still
    /// queued (D-17). The field name is historical.
    ///
    /// The queue itself lives in
    /// [`crate::snapshot::EngineSnapshotDto`], which is the only snapshot
    /// form that round-trips deterministically.
    pub event_queue_head: u64,
    /// Deterministic seed (M-12 §1.1 评审 R8).
    pub seed: u64,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            schema_version: SIM_STATE_SCHEMA_VERSION,
            virtual_time_ps: 0,
            gpr: [0u32; 32],
            pc: 0,
            csrs: Vec::new(),
            pending_writes: Vec::new(),
            event_queue_head: 0,
            seed: 0,
        }
    }
}

/// A memory write captured in the current quantum, replayed at commit.
/// Storing them inside [`SimState`] (rather than mutating RAM directly) is
/// what guarantees deterministic re-execution (M-12 §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWrite {
    /// Physical address.
    pub paddr: u32,
    /// Width and value.
    pub size: WriteSize,
    /// Value to commit.
    pub value: u32,
    /// Originating component id (for diagnostics).
    pub source: String,
}

impl Default for PendingWrite {
    fn default() -> Self {
        Self {
            paddr: 0,
            size: WriteSize::Byte,
            value: 0,
            source: String::new(),
        }
    }
}

/// Bus access width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteSize {
    /// 8-bit access.
    Byte,
    /// 16-bit (halfword) access.
    Half,
    /// 32-bit (word) access.
    Word,
}

impl Default for WriteSize {
    fn default() -> Self {
        Self::Byte
    }
}

/// Debug metadata = symbols + breakpoints + watch points (M-08, M-10, M-11).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugMeta {
    /// DWARF-derived line table. M-11 owns the parsing.
    pub line_table: Vec<LineEntry>,
    /// Symbol table (function / variable).
    pub symbols: Vec<Symbol>,
    /// Active breakpoints (M-08).
    pub breakpoints: Vec<Breakpoint>,
    /// Watch points (M-10 §4.10).
    pub watchpoints: Vec<Watchpoint>,
}

/// A DWARF-derived source-line binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEntry {
    /// Instruction address.
    pub addr: u32,
    /// Source file (URL or relative path inside the project).
    pub file: String,
    /// 1-based line number.
    pub line: u32,
}

impl Default for LineEntry {
    fn default() -> Self {
        Self {
            addr: 0,
            file: String::new(),
            line: 0,
        }
    }
}

/// Function or variable symbol (M-11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// Symbol name.
    pub name: String,
    /// Optional address.
    pub addr: Option<u32>,
    /// Size in bytes (variables).
    pub size: Option<u32>,
}

impl Default for Symbol {
    fn default() -> Self {
        Self {
            name: String::new(),
            addr: None,
            size: None,
        }
    }
}

/// A single breakpoint (M-08).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Stable breakpoint id.
    pub id: u32,
    /// Trigger address.
    pub addr: u32,
    /// Optional source location hint (UI display only).
    pub source_hint: Option<String>,
    /// Whether the breakpoint is currently enabled.
    pub enabled: bool,
}

impl Default for Breakpoint {
    fn default() -> Self {
        Self {
            id: 0,
            addr: 0,
            source_hint: None,
            enabled: true,
        }
    }
}

/// A watch point (M-10). Fires when the access matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watchpoint {
    /// Stable watch point id.
    pub id: u32,
    /// Watched address.
    pub addr: u32,
    /// Access size to match.
    pub size: WriteSize,
    /// Read / write / either.
    pub access: WatchAccess,
}

impl Default for Watchpoint {
    fn default() -> Self {
        Self {
            id: 0,
            addr: 0,
            size: WriteSize::Word,
            access: WatchAccess::Write,
        }
    }
}

/// Watchpoint access kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchAccess {
    /// Match read accesses.
    Read,
    /// Match write accesses.
    Write,
    /// Match both read and write accesses.
    Either,
}

impl Default for WatchAccess {
    fn default() -> Self {
        Self::Write
    }
}

/// One changed register inside a
/// [`crate::protocol::EventPayload::SimStateDelta`].
///
/// Added in contract v2: the LLD's `simStateDelta` has been referenced by
/// M-02 / M-03 / M-10 since v0.11, but the contract had no way to carry
/// it — the front end rebuilt its register view out of waveform edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaRegister {
    /// Register index (0..=31).
    pub index: u32,
    /// The new value.
    pub value: u32,
}

impl Default for DeltaRegister {
    fn default() -> Self {
        Self { index: 0, value: 0 }
    }
}

/// Edge-encoded waveform fragment (M-18, M-19).
///
/// Waveforms are NOT periodic samples — they are an *edge stream*: a list
/// of "value changed at virtual time t" entries per signal. The frontend
/// renders the staircase by replaying the edges (M-19 §4.19).
///
/// **Renamed in contract v2** (D-34): the type was called `Waveform`,
/// which collided with the *live batch* shape `WaveformDelta` — and
/// `EventPayload::WaveformDelta` used to carry **this** type, so the
/// name on the event said "delta" while the content was a history.
/// The two shapes now have one name each: this one is the **history**,
/// [`WaveformDelta`] is the **batch**.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformHistory {
    /// Schema version of the edge format.
    pub schema_version: u32,
    /// One edge-encoded stream per probed signal.
    pub streams: Vec<WaveformStream>,
}

/// One signal's edge-encoded history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformStream {
    /// Stable signal id (e.g. "UART0.TX", "BUS.A1.write").
    pub id: String,
    /// Display name.
    pub label: String,
    /// Edges, time-ordered.
    pub edges: Vec<Edge>,
}

/// One edge: a value change at a specific virtual time.
///
/// **Shape note (M-19 review #13, 2026-10-19):** the contract carries
/// **two** edge types on purpose, and they are not interchangeable:
///
/// - [`Edge`] — the *export / history* shape. It belongs to a
///   [`WaveformStream`], so the channel is already implied by the
///   stream it lives in; only the timestamp and the value are needed.
///   This is what M-18's `ProbeRegistry` accumulates per channel and
///   what the front-end replays to redraw a waveform.
/// - [`EdgeEvent`] — the *live* shape. It crosses the M-18 → M-19
///   boundary as a standalone event, so it must carry its own
///   channel number and the classified edge kind.
///
/// Merging them (by adding `ch` / `kind` to `Edge`) would change an
/// already-frozen contract type and would bloat the history shape
/// with fields its container already implies. Keeping them separate
/// costs one extra 18-byte struct on the live path, which is the
/// cheaper trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Virtual time (picoseconds) of the edge.
    ///
    /// Travels as a **decimal string** — see [`crate::numstr`] and the
    /// contract v2 spec §2.2.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_ps: u64,
    /// New value (interpretation depends on the signal kind; bits, level, etc).
    pub value: u64,
}

impl Default for Edge {
    fn default() -> Self {
        Self { t_ps: 0, value: 0 }
    }
}

/// Edge kind (M-18 v0.8 §18-3).
///
/// Single-bit channels (e.g. one GPIO pin) distinguish `Rising` from
/// `Falling`. Multi-bit channels collapse both into `Value` because
/// rising/falling on a multi-bit word is ambiguous (which bit?). The
/// frontend renders both kinds the same way (staircase step on the
/// new value); `Rising` / `Falling` is a hint for the rendered arrow
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// 0 → 1 transition on a single-bit channel.
    Rising,
    /// 1 → 0 transition on a single-bit channel.
    Falling,
    /// Multi-bit value change (rising / falling not classified).
    Value,
}

/// Bus trace direction (M-18 v0.8 §18-1, §18-8, review #11 2026-10-19).
///
/// Replaces the bare `dir: u8` magic-number field that originally
/// lived on `TraceEvent`. The wire encoding is `0` for read, `1`
/// for write (matches the `tracing`-style convention so external
/// tooling can filter on the raw byte without knowing the kernel
/// enum) — but the in-memory representation is a typed enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceDir {
    /// Memory read access.
    Read,
    /// Memory write access.
    Write,
}

impl TraceDir {
    /// Wire byte (`0` = read, `1` = write) for external tooling.
    pub fn as_byte(self) -> u8 {
        match self {
            TraceDir::Read => 0,
            TraceDir::Write => 1,
        }
    }
}

impl Default for TraceDir {
    fn default() -> Self {
        TraceDir::Read
    }
}

/// Bus trace event (M-18 v0.8 §18-1, §18-8).
///
/// One event per `BusAccess` that passes the trace filter. Lives in
/// the contract so M-11 (trace export) can consume it without
/// depending on the kernel crate.
///
/// Width is byte-count (1, 2, 4). Direction is the typed
/// [`TraceDir`] enum (M-18 review #11, 2026-10-19; previously was
/// a `u8` magic number).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Virtual time of the bus access (ps).
    ///
    /// Travels as a **decimal string** — see [`crate::numstr`] and the
    /// contract v2 spec §2.2.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_ps: u64,
    /// Physical address of the *first byte* of the access.
    pub addr: u32,
    /// Access width in bytes (1, 2, 4).
    pub width: u8,
    /// Access direction.
    pub dir: TraceDir,
}

/// Edge event crossing the M-18 → M-19 boundary (M-19 v0.9 §1.1).
///
/// M-18 performs the edge *judgment* (rising / falling / value) and
/// M-19 performs the edge *encoding* (write `{t, ch, value, edge}`
/// into the per-channel ring). This struct is the carrier between
/// the two: M-18's `m19Encode` entry point hands one of these to
/// M-19's `encodeEdge`.
///
/// Lives in the contract because it is the payload of the
/// [`crate::port::EdgeSink`] port — the M-18 → M-19 handoff must not
/// depend on the kernel crate from either side.
///
/// This type is deliberately lean (18 bytes before padding): it is
/// allocated on every observed edge, so it carries no labels or
/// channel metadata. Channel metadata (label, value width) is
/// registered separately with M-19's `define_channel`.
///
/// **Shape note:** see [`Edge`] for why the contract carries this
/// type *and* the history-shaped [`Edge`] side by side
/// (M-19 review #13, 2026-10-19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeEvent {
    /// Virtual time of the edge (picoseconds).
    ///
    /// Travels as a **decimal string** — see [`crate::numstr`] and the
    /// contract v2 spec §2.2.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_ps: u64,
    /// Channel number the edge belongs to.
    pub ch: u8,
    /// New value of the signal.
    pub value: u64,
    /// Edge classification (rising / falling / value).
    pub kind: EdgeKind,
}

/// Incremental waveform push payload (M-19 v0.9 §3.2, table 71).
///
/// The unit of the M-19 → M-03 (W1 subscribe) push. The frontend
/// merges the entries of consecutive deltas into its render buffer.
///
/// The sequence numbers are **monotonic and gap-free in the number
/// space**: `seq_end` of one delta equals `seq_start` of the next,
/// even when a batch was dropped by a push failure. The *content*
/// hole is recorded separately in [`DropRange`], so a consumer that
/// sees a dropped range can request a resume for that range
/// (M-19 §6.2(2), aligned with M-07 `resumeBySeq` / `E_PROTO-05`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformDelta {
    /// First sequence number covered by this batch.
    pub seq_start: u64,
    /// Last sequence number covered by this batch (exclusive end;
    /// `seq_end - seq_start` == number of entries).
    pub seq_end: u64,
    /// Virtual time of the batch (picoseconds). Sourced from the
    /// authoritative virtual clock (M-12) so the frontend's time axis
    /// matches the simulation's.
    ///
    /// Travels as a **decimal string** — see [`crate::numstr`] and the
    /// contract v2 spec §2.2.
    #[serde(with = "crate::numstr::u64_decimal")]
    pub t_virt: u64,
    /// Edge events in this batch, in arrival order.
    pub entries: Vec<EdgeEvent>,
}

/// A range of sequence numbers whose content was dropped
/// (M-19 v0.9 §3.2, §6.1 — "推送失败").
///
/// Recorded when a batch fails to be delivered to the subscriber.
/// The sequence *numbers* were still consumed (the number space stays
/// gap-free), so this range — not a gap in `seq_start`/`seq_end` —
/// is what tells the consumer that content is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropRange {
    /// First sequence number in the dropped range.
    pub seq_start: u64,
    /// One past the last sequence number in the dropped range.
    pub seq_end: u64,
}

impl DropRange {
    /// Number of sequence numbers covered by this range.
    pub fn len(&self) -> u64 {
        self.seq_end.saturating_sub(self.seq_start)
    }

    /// True when the range covers no sequence numbers.
    pub fn is_empty(&self) -> bool {
        self.seq_end <= self.seq_start
    }
}
