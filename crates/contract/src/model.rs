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
        Self { name: String::new(), component_id: None, children: Vec::new() }
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

impl Default for ImageKind { fn default() -> Self { Self::Elf } }

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

impl Default for ImageSource { fn default() -> Self { Self::Bytes(Vec::new()) } }

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
        Self { schema: 1, isa: ISA.to_string(), toolchain: String::new() }
    }
}

/// SimState = everything needed to deterministically resume a paused
/// simulation (M-20). The fields are intentionally serializable so the
/// snapshot can be persisted to IndexedDB (MVP) or a server store (P1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimState {
    /// Schema version of the snapshot format.
    pub schema_version: u32,
    /// Virtual time (picoseconds) at the snapshot point.
    pub virtual_time_ps: u64,
    /// ISS general-purpose registers (x0..x31, 32-bit each).
    pub gpr: [u32; 32],
    /// Program counter.
    pub pc: u32,
    /// CSR file (placeholder: concrete CSR set depends on M-13 detail).
    pub csrs: Vec<(u16, u32)>,
    /// Pending memory writes from the current quantum.
    pub pending_writes: Vec<PendingWrite>,
    /// Scheduler event queue head pointer.
    pub event_queue_head: u64,
    /// Deterministic seed (M-12 §1.1 评审 R8).
    pub seed: u64,
}

impl Default for SimState {
    fn default() -> Self {
        Self {
            schema_version: 1,
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
        Self { paddr: 0, size: WriteSize::Byte, value: 0, source: String::new() }
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

impl Default for WriteSize { fn default() -> Self { Self::Byte } }

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
    fn default() -> Self { Self { addr: 0, file: String::new(), line: 0 } }
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
    fn default() -> Self { Self { name: String::new(), addr: None, size: None } }
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
    fn default() -> Self { Self { id: 0, addr: 0, source_hint: None, enabled: true } }
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
        Self { id: 0, addr: 0, size: WriteSize::Word, access: WatchAccess::Write }
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

impl Default for WatchAccess { fn default() -> Self { Self::Write } }

/// Edge-encoded waveform fragment (M-18, M-19).
///
/// Waveforms are NOT periodic samples — they are an *edge stream*: a list
/// of "value changed at virtual time t" entries per signal. The frontend
/// renders the staircase by replaying the edges (M-19 §4.19).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waveform {
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Virtual time (picoseconds) of the edge.
    pub t_ps: u64,
    /// New value (interpretation depends on the signal kind; bits, level, etc).
    pub value: u64,
}

impl Default for Edge {
    fn default() -> Self { Self { t_ps: 0, value: 0 } }
}
