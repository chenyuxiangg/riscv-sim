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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum ELF file size in bytes (default 16 MiB).
    pub elf_size: usize,
    /// Maximum number of PT_LOAD segments (default 64).
    pub segment_count: u32,
    /// Maximum number of DWARF symbol-table entries (default 1e5).
    pub symbol_count: u32,
    /// Maximum number of DWARF line-table entries (default 2e5).
    pub line_entry_count: u32,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            elf_size: 16 * 1024 * 1024,
            segment_count: 64,
            symbol_count: 100_000,
            line_entry_count: 200_000,
        }
    }
}

/// Loader trait. The host picks an implementation appropriate to the
/// incoming file (extension sniff → [`detect_kind`]).
pub trait Loader {
    /// Parse `bytes` as `kind`. Returns the populated [`LoadImage`].
    fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError>;
    /// Same, with caller-supplied parse limits.
    fn load_with_limits(
        &self,
        bytes: &[u8],
        kind: Kind,
        limits: &ParseLimits,
    ) -> Result<LoadImage, PlatformError> {
        // Default: route to `load`, ignore the limits override.
        let _ = limits;
        self.load(bytes, kind)
    }
}

/// Detect image kind from a file extension. Lowercased.
pub fn detect_kind(extension: &str) -> Kind {
    match extension.to_ascii_lowercase().as_str() {
        "elf" | "out" => Kind::Elf,
        "hex" | "ihex" => Kind::Hex,
        "bin" | "raw" => Kind::RawHex,
        _ => Kind::Elf,
    }
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
    fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError> {
        self.load_with_limits(bytes, kind, &ParseLimits::default())
    }

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
    if bytes.len() > limits.elf_size {
        return Err(PlatformError::new(
            FAMILY,
            2,
            format!(
                "raw size {} exceeds parseLimit.elfSize {}",
                bytes.len(),
                limits.elf_size
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
