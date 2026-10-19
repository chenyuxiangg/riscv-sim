//! Shared register-window helpers (M-15 review N1).
//!
//! Extracted from `uart.rs` / `gpio.rs` / `timer.rs` after review #5
//! introduced `register_offset` + `extract_subword` three times. The
//! helpers are byte-addressable but word-aligned at the register
//! level — see [`register_offset`] for the alignment rule and
//! [`extract_subword`] for the `WriteSize`-driven value extraction.

use riscv_sim_contract::model::WriteSize;

/// Map a byte/halfword/word offset to the containing word-aligned
/// register offset (lower 2 bits cleared).
///
/// M-15 review #5: lets firmware use `lb` / `sb` / `lh` / `sh`
/// inside a 4-byte register window without surfacing a spurious
/// `E_PER-02`.
#[inline]
pub fn register_offset(offset: u32) -> u32 {
    offset & !0x3
}

/// Extract the requested sub-register value from a 32-bit word read.
/// `offset` within the word selects which byte/halfword the firmware
/// wants; `size` drives the truncation width.
///
/// M-15 review #5: paired with [`register_offset`] so word-aligned
/// `read()` calls return the same value regardless of which byte
/// offset inside the word the firmware used.
#[inline]
pub fn extract_subword(word: u32, offset: u32, size: WriteSize) -> u32 {
    match size {
        WriteSize::Byte => (word >> ((offset & 0x3) * 8)) & 0xFF,
        WriteSize::Half => (word >> ((offset & 0x2) * 8)) & 0xFFFF,
        WriteSize::Word => word,
    }
}
