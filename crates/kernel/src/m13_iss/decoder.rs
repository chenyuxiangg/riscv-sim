//! Hand-written RISC-V instruction decoder for RV32I + M + C.
//!
//! MVP scope: just enough to decode `addi` end-to-end so the harness
//! in `super::tests` passes. The full RV32IMC table is laid out as
//! a placeholder; widen as we expand the supported ISA.

use riscv_sim_contract::model::WriteSize;

/// Length of a 32-bit instruction, in bytes.
pub const LEN_32: u8 = 4;

/// Length of a 16-bit (compressed, RV32C) instruction, in bytes.
pub const LEN_16: u8 = 2;

/// Decoded instruction. Fields are populated only when meaningful for
/// the opcode (e.g. `imm` is unused for `R`-type ALU ops).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// 7-bit primary opcode.
    pub opcode: u8,
    /// 3-bit funct3 (sub-opcode).
    pub funct3: u8,
    /// 7-bit funct7 (for R-type / M-extension).
    pub funct7: u8,
    /// Destination register (rd), or 0 if unused.
    pub rd: u8,
    /// Source register 1.
    pub rs1: u8,
    /// Source register 2.
    pub rs2: u8,
    /// Sign- or zero-extended immediate, when applicable.
    pub imm: i32,
    /// Original 32-bit instruction word.
    pub raw: u32,
    /// PC the instruction was fetched from.
    pub pc: u32,
    /// Instruction length in bytes — 4 for a 32-bit encoding, 2 for a
    /// compressed one.
    ///
    /// The PC advances by this amount, so the decoder has to fill it
    /// in correctly: a decoder that always reports 4 makes every
    /// compressed instruction land on a half-word boundary (D-16).
    pub len: u8,
}

/// Decoder configuration. MVP only needs to recognise the handful of
/// instructions the harness exercises.
#[derive(Debug, Default, Clone, Copy)]
pub struct Decoder {
    _private: (),
}

impl Decoder {
    /// Decode a single 32-bit instruction. The compressed (16-bit)
    /// extension is not yet supported.
    pub fn decode(&self, raw: u32, pc: u32) -> Decoded {
        let opcode = (raw & 0x7F) as u8;
        let rd = ((raw >> 7) & 0x1F) as u8;
        let funct3 = ((raw >> 12) & 0x07) as u8;
        let rs1 = ((raw >> 15) & 0x1F) as u8;
        let rs2 = ((raw >> 20) & 0x1F) as u8;
        let funct7 = ((raw >> 25) & 0x7F) as u8;

        // Sign-extended 12-bit immediate for I-type.
        let imm_i = ((raw >> 20) & 0xFFF) as i32;
        let imm = (imm_i << 20) >> 20;

        // Instruction width: every 32-bit encoding has bits[1:0] = 0b11,
        // a compressed (RV32C) encoding never does (RISC-V spec, base
        // instruction formats). RV32C is not implemented yet, but the
        // length is decoded here so the PC can step by 2 once it is,
        // without touching the decoder's consumers again (D-16).
        let len = if raw & 0b11 == 0b11 { LEN_32 } else { LEN_16 };

        Decoded {
            opcode,
            funct3,
            funct7,
            rd,
            rs1,
            rs2,
            imm,
            raw,
            pc,
            len,
        }
    }
}

/// Width of the access the executor should perform when reading
/// instruction memory. RV32C is a future task; the MVP only handles
/// 32-bit fetches.
pub fn fetch_size() -> WriteSize {
    WriteSize::Word
}
