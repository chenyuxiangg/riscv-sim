//! M-06 ELF / Raw-HEX 解析器 (Loader).
//!
//! Parses ELF files and raw HEX / binary blobs into a stream of
//! `(paddr, bytes)` segments the bus can consume.
//!
//! TODO(MVP): integrate the `object` crate (Rust, BSD-2-Clause) for ELF
//! parsing; for Raw-Hex, write a small Motorola / Intel HEX parser
//! in-tree. DWARF line-table parsing is owned by M-11.
//!
//! Status: stub — public port only.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};

/// Output of a successful load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadImage {
    /// `(paddr, bytes)` segments, sorted by `paddr`.
    pub segments: Vec<(u32, Vec<u8>)>,
    /// Entry point (ELF `e_entry`, or 0 for raw blobs).
    pub entry: u32,
}

impl Default for LoadImage {
    fn default() -> Self { Self { segments: Vec::new(), entry: 0 } }
}

/// Image kind — see [`riscv_sim_contract::model::ImageKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Elf,
    RawHex,
}

impl Default for Kind { fn default() -> Self { Self::Elf } }

/// Loader trait. The frontend / server use this to materialise a
/// `LoadImage` from bytes.
pub trait Loader {
    /// Parse `bytes` as `kind`. Returns the populated `LoadImage`.
    fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError>;
}

/// Reference implementation. MVP-failsafe: returns the bytes
/// verbatim at address 0.
pub struct DefaultLoader;

impl Loader for DefaultLoader {
    fn load(&self, bytes: &[u8], kind: Kind) -> Result<LoadImage, PlatformError> {
        let segments = vec![(0u32, bytes.to_vec())];
        let entry = match kind {
            Kind::Elf    => 0x1000,    // M-06 §4.6 default reset vector for ELF
            Kind::RawHex => 0x0000,
        };
        Ok(LoadImage { segments, entry })
    }
}

/// Convenience helper for callers that already know the kind.
pub fn detect_kind(extension: &str) -> Kind {
    match extension.to_ascii_lowercase().as_str() {
        "elf" | "out" => Kind::Elf,
        "hex" | "ihex" | "bin" => Kind::RawHex,
        _ => Kind::Elf,
    }
}

/// Stub the platform error family used by the loader.
pub const LOADER_FAMILY: ErrorFamily = ErrorFamily::Loader;
