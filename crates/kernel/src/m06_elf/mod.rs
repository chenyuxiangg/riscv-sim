//! M-06 ELF / Raw / HEX 解析器 (Loader).
//!
//! MVP scope (LLD §4.6, M-06 v1.0):
//!   - parse ELF32 LE RV32 (PT_LOAD segments → `Vec<(u32, Vec<u8>)>`).
//!   - parse Raw `bytes` at a caller-provided base.
//!   - parse Intel HEX text (with 0x02 / 0x04 extended-address records,
//!     06-2 / 06-4).
//!   - enforce `parseLimit` (ELF ≤16 MiB, segment ≤64, …) up front, so
//!     a pathological host file never reaches M-14 / M-18.
//!   - On success: return `LoadImage { segments, entry }`. The host
//!     pushes it through M-05 → M-14 `loadImage` (M-06 §06-1 single
//!     write path).
//!
//! ## Image-kind detection
//!
//! The host should call [`detect_kind_with_hint`]: the **magic bytes**
//! decide, and the file extension is only a secondary hint. An unknown
//! extension is treated as a raw blob rather than being assumed to be an
//! ELF (D-21) — a raw blob is the only interpretation that cannot be
//! wrong for an arbitrary byte sequence.
//!
//! DWARF line-table parsing is out of scope (M-5 / M-11). The
//! `LoadImage::entry` field is filled in from the ELF `e_entry`; for
//! Raw / HEX it is `0` (caller may override before feeding M-13).
//!
//! ## Error codes
//!
//! All errors use `ErrorFamily::Loader` (`LDR` family). Scenarios are
//! additive (PROGRESS.md §7).
//!
//! | code    | name               | meaning                                       |
//! | ------- | ------------------ | --------------------------------------------- |
//! | LDR-01  | malformed          | bad ELF magic / class / version / machine; HEX checksum mismatch; unknown record type; missing EOF |
//! | LDR-02  | parse-limit        | file > 16 MiB, >64 segments, record byte count >0xFF, etc. |
//! | LDR-03  | address-overflow   | paddr + memsz crosses 4 GiB; HEX extended+local base overflow |
//! | LDR-04  | out-of-bus         | segment mapped outside the [0, 4 GiB) bus window (currently aliased to LDR-03) |
//!
//! ## Robustness contract
//!
//! Every path in this module returns `Result<_, PlatformError>`: no
//! malformed input may panic, because in WASM a panic takes the worker
//! down. The three error scenarios above are the only failure modes.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};

pub mod elf;
pub mod hex;

/// Re-export the 4 GiB bus size so callers can size their regions
/// consistently (used here for `parseRaw` boundary checks).
pub use crate::m14_bus::BUS_SIZE;

/// Output of a successful load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadImage {
    /// `(paddr, bytes)` segments, sorted by `paddr`. May contain both
    /// `.text` / `.data` filesz bytes and zero-filled `.bss` gaps.
    pub segments: Vec<(u32, Vec<u8>)>,
    /// Entry point (ELF `e_entry`, or 0 for raw / hex blobs).
    pub entry: u32,
}

impl Default for LoadImage {
    fn default() -> Self {
        Self {
            segments: Vec::new(),
            entry: 0,
        }
    }
}

/// Image kind. Mirrors [`riscv_sim_contract::model::ImageKind`] so the
/// M-05 `loadELF` payload is type-correct without round-tripping
/// through `ImageKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// ELF file (RISC-V).
    Elf,
    /// Intel HEX text (IHEX).
    Hex,
    /// Raw binary blob (caller supplies a base address).
    RawHex,
}

impl Default for Kind {
    fn default() -> Self {
        Self::Elf
    }
}

/// Parse-input limits (LLD §2.1.4 + 评审 P-12). They are conservative
/// defaults: any single limit can be reduced by callers but cannot
/// exceed these baselines in the MVP.
///
/// Every field is enforced by the parser that owns the corresponding
/// input: `elf_size` by the ELF parser, `hex_size` by the Intel HEX
/// parser, `raw_size` by the raw parser, `segment_count` by both the ELF
/// and the HEX parser. `symbol_count` / `line_entry_count` bound the
/// DWARF tables and are enforced by M-11, which owns that parsing.
///
/// `total_image_bytes` and `max_region_bytes` bound the **memory the
/// loader materialises**, not the size of the input file. They exist
/// because a tiny file can declare a huge image: a `.bss` gap is not
/// stored in the file, so an 84-byte ELF can declare a gigabyte of RAM
/// and have the loader allocate it (D-10, cross-module with M-14's
/// `prepare_bytes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum ELF file size in bytes (default 16 MiB).
    pub elf_size: usize,
    /// Maximum Intel HEX text size in bytes (default 16 MiB).
    pub hex_size: usize,
    /// Maximum raw-binary file size in bytes (default 16 MiB).
    pub raw_size: usize,
    /// Maximum number of PT_LOAD segments (default 64).
    pub segment_count: u32,
    /// Maximum number of DWARF symbol-table entries (default 1e5).
    pub symbol_count: u32,
    /// Maximum number of DWARF line-table entries (default 2e5).
    pub line_entry_count: u32,
    /// Maximum number of bytes the loader may materialise in total
    /// across every segment of one image (default 64 MiB).
    pub total_image_bytes: u64,
    /// Maximum number of bytes a single segment may declare (default
    /// 16 MiB).
    pub max_region_bytes: u64,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            elf_size: 16 * 1024 * 1024,
            hex_size: 16 * 1024 * 1024,
            raw_size: 16 * 1024 * 1024,
            segment_count: 64,
            symbol_count: 100_000,
            line_entry_count: 200_000,
            total_image_bytes: 64 * 1024 * 1024,
            max_region_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Loader trait. The host picks an implementation appropriate to the
/// incoming file (magic-byte sniff → [`detect_kind_with_hint`]).
///
/// The trait deliberately has **no default body** for
/// [`Loader::load_with_limits`]: an implementation that quietly ignored
/// the caller's limits would defeat the whole point of the limits
/// (D-22). Implementations must enforce every field of the
/// [`ParseLimits`] they are handed.
pub trait Loader {
    /// Parse `bytes` as `kind` with the default [`ParseLimits`].
    fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError> {
        self.load_with_limits(bytes, kind, &ParseLimits::default())
    }
    /// Same, with caller-supplied parse limits. Implementations MUST
    /// honour every field of `limits`.
    fn load_with_limits(
        &self,
        bytes: &[u8],
        kind: Kind,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError>;
}

/// ELF magic: `\x7FELF`.
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// Map a file extension to an image kind, or `None` when the extension
/// carries no usable hint.
fn kind_from_extension(extension: &str) -> Option<Kind> {
    match extension.to_ascii_lowercase().as_str() {
        "elf" | "out" => Some(Kind::Elf),
        "hex" | "ihex" => Some(Kind::Hex),
        "bin" | "raw" => Some(Kind::RawHex),
        _ => None,
    }
}

/// Detect image kind from a file extension. Lowercased.
///
/// An unknown extension no longer guesses ELF (D-21): it falls back to
/// [`Kind::RawHex`], the only interpretation that cannot be wrong for an
/// arbitrary byte sequence. Guessing ELF made every extension-less file
/// fail with "bad ELF magic".
///
/// This helper only looks at the extension. Prefer
/// [`detect_kind_with_hint`], which sniffs the magic bytes first.
pub fn detect_kind(extension: &str) -> Kind {
    kind_from_extension(extension).unwrap_or(Kind::RawHex)
}

/// Sniff the image kind from the file's **magic bytes**.
///
/// Returns `None` when the bytes are inconclusive — i.e. a raw blob that
/// starts with neither the ELF magic nor an IHEX record marker.
///
/// - ELF: the payload starts with `\x7F E L F`.
/// - Intel HEX: the first non-blank line starts with the record marker
///   `:`.
/// - Anything else: inconclusive.
pub fn detect_kind_from_bytes(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(&ELF_MAGIC) {
        return Some(Kind::Elf);
    }
    if looks_like_ihex(bytes) {
        return Some(Kind::Hex);
    }
    None
}

/// Detect the image kind, preferring the **magic bytes** and using the
/// file extension only as a secondary hint (D-21).
///
/// Precedence:
/// 1. Magic bytes positively identify the format (ELF magic / IHEX
///    record marker) → that format wins.
/// 2. Otherwise the extension decides. An extension that names a
///    specific format (`.elf` / `.hex`) is honoured even when the magic
///    does not check out, so the parser reports a precise error
///    ("bad ELF magic" / "missing leading ':'") instead of silently
///    loading the file as a raw blob.
/// 3. Otherwise → [`Kind::RawHex`].
pub fn detect_kind_with_hint(extension: &str, bytes: &[u8]) -> Kind {
    detect_kind_from_bytes(bytes)
        .or_else(|| kind_from_extension(extension))
        .unwrap_or(Kind::RawHex)
}

/// True when the payload looks like Intel HEX text: the first non-blank
/// line starts with the record marker `:`.
fn looks_like_ihex(bytes: &[u8]) -> bool {
    for raw_line in bytes.split(|b| *b == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        let trimmed = trim_ascii_whitespace(line);
        if trimmed.is_empty() {
            continue;
        }
        return trimmed[0] == b':';
    }
    false
}

/// Trim ASCII whitespace from both ends of a byte slice.
fn trim_ascii_whitespace(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// Production loader (PROGRESS.md §5.5 第二步). Supports ELF / Raw /
/// HEX; uses the supplied [`ParseLimits`].
pub struct DefaultLoader;

impl DefaultLoader {
    /// Parse with the default [`ParseLimits`].
    pub fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError> {
        <Self as Loader>::load(self, bytes, kind)
    }

    /// Parse with caller-supplied limits.
    pub fn load_with_limits(
        &self,
        bytes: &[u8],
        kind: Kind,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError> {
        <Self as Loader>::load_with_limits(self, bytes, kind, limits)
    }

    /// Load a raw blob at a caller-supplied base address.
    ///
    /// The [`Loader`] trait carries no base address, so raw images routed
    /// through [`Loader::load`] always land at address 0. Hosts that know
    /// the intended base address should call this method instead.
    pub fn load_raw_at(
        &self,
        bytes: &[u8],
        base: u32,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError> {
        parse_raw(bytes, base, limits)
    }

    /// Inherent form of the trait `load`, callable as `loader.load(...)`
    /// without importing the [`Loader`] trait.
    fn parse(
        &self,
        bytes: &[u8],
        kind: Kind,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError> {
        match kind {
            Kind::Elf => elf::parse(bytes, limits),
            Kind::Hex => hex::parse(
                core::str::from_utf8(bytes).map_err(|e| {
                    PlatformError::new(ErrorFamily::Loader, 1, format!("HEX must be UTF-8: {}", e))
                })?,
                limits,
            ),
            Kind::RawHex => parse_raw(bytes, 0, limits),
        }
    }
}

impl Loader for DefaultLoader {
    fn load_with_limits(
        &self,
        bytes: &[u8],
        kind: Kind,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError> {
        self.parse(bytes, kind, limits)
    }
}

/// Loader for raw-binary images that are mapped at a caller-supplied
/// base address. Returns one segment: `[base, base+bytes.len())`.
pub fn parse_raw(
    bytes: &[u8],
    base: u32,
    limits: &ParseLimits,
) -> Result<LoadImage, PlatformError> {
    const FAMILY: ErrorFamily = ErrorFamily::Loader;
    if bytes.len() > limits.raw_size {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "raw size {} exceeds parseLimit.rawSize {}",
                bytes.len(),
                limits.raw_size
            ),
        ));
    }
    if bytes.is_empty() {
        return Err(PlatformError::new(
            FAMILY,
            1,
            "raw image is empty (nothing to load)",
        ));
    }
    // D-10: a raw blob becomes exactly one region, so the image budget
    // applies to it directly.
    if bytes.len() as u64 > limits.max_region_bytes {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "raw size {} exceeds parseLimit.maxRegionBytes {}",
                bytes.len(),
                limits.max_region_bytes
            ),
        ));
    }
    if bytes.len() as u64 > limits.total_image_bytes {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "image size {} exceeds parseLimit.totalImageBytes {}",
                bytes.len(),
                limits.total_image_bytes
            ),
        ));
    }
    let end = (base as u64)
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| PlatformError::new(FAMILY, 3, "raw base + length overflows u64"))?;
    if end > BUS_SIZE {
        return Err(PlatformError::new(
            FAMILY,
            3,
            format!("raw range 0x{:08x}..0x{:08x} overruns 4 GiB bus", base, end),
        ));
    }
    Ok(LoadImage {
        segments: vec![(base, bytes.to_vec())],
        entry: 0,
    })
}

/// Family used by errors raised from this module. Source design §3.4
/// existing codes; we keep it as a public re-export so downstream
/// kernel crates (M-05 dispatch) don't have to spell the family out.
pub const LOADER_FAMILY: ErrorFamily = ErrorFamily::Loader;

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_malformed;
