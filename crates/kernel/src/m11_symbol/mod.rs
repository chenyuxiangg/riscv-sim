//! M-11 符号表 / DWARF / 反汇编 / trace 导出 (Symbol / DWARF / Disasm / Trace).
//!
//! Combines four closely-related responsibilities into a single module:
//! - symbol-table lookup
//! - DWARF line-table resolution (`addr → file:line`)
//! - on-the-fly disassembly (RV32I + M + C)
//! - execution-trace export (CSV / VCD-compatible)
//!
//! The MVP disassembly path is shared with M-13's decoder (it
//! already produces a `Decoded` for the same instruction word).
//!
//! Status: stub — public port only.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{LineEntry, Symbol, Waveform};

/// Public port of M-11. Each method is a separate sub-responsibility.
pub trait SymbolService {
    /// Resolve an address to a source location (DWARF line table).
    fn resolve_line(&self, addr: u32) -> Result<Option<LineEntry>, PlatformError>;
    /// Look up a symbol by name.
    fn lookup_symbol(&self, name: &str) -> Result<Option<Symbol>, PlatformError>;
    /// Look up a symbol by address.
    fn symbol_at(&self, addr: u32) -> Result<Option<Symbol>, PlatformError>;
    /// Disassemble one instruction word into a human-readable string.
    fn disassemble(&self, addr: u32, word: u32) -> Result<String, PlatformError>;
    /// Export the in-memory waveform as a trace blob (CSV / VCD).
    fn export_trace(&self, wf: &Waveform, format: TraceFormat) -> Result<Vec<u8>, PlatformError>;
}

/// Export format for execution traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceFormat {
    /// Comma-separated values.
    Csv,
    /// VCD (Value Change Dump) — minimal support, see LLD §4.11.4.
    Vcd,
    /// Plain text.
    Txt,
}

impl Default for TraceFormat { fn default() -> Self { Self::Csv } }

/// Family used by errors raised from this module.
pub const SYMBOL_FAMILY: ErrorFamily = ErrorFamily::Loader;
