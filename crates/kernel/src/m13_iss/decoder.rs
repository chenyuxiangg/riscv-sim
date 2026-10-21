//! Hand-written RISC-V instruction decoder for RV32I + M + C.
//!
//! The decoder recognises both instruction widths.  A 32-bit encoding
//! is decoded directly; a 16-bit (compressed, RV32C) encoding is
//! **expanded** into the fields of the equivalent 32-bit instruction
//! ([`super::compressed`]) and those fields are what the rest of the
//! ISS sees — there is no second execution path for compressed
//! instructions.

use riscv_sim_contract::model::WriteSize;

use super::compressed;

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
    /// Original instruction word.
    ///
    /// For a compressed instruction this holds the **16-bit** original
    /// encoding, not the expanded 32-bit instruction it stands for —
    /// the point of the field is to identify *what was fetched*, so a
    /// trap handler can report it.
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
    /// True when the instruction was fetched as a 16-bit compressed
    /// encoding.
    ///
    /// The flag is what tells the executor that a jump's link register
    /// holds `pc + 2` rather than `pc + 4` (LLD §3.5, 评审 N1) — the
    /// PC advance itself already follows [`Decoded::len`].
    pub compressed: bool,
    /// The original 16-bit compressed encoding. Zero when the
    /// instruction was not compressed.
    pub compressed_raw: u16,
    /// True when the encoding is reserved / illegal.
    ///
    /// A reserved compressed encoding has no 32-bit equivalent, so it
    /// cannot be expressed as expanded fields; the ISS reports it as
    /// an illegal instruction.
    pub illegal: bool,
}

/// Decoder configuration. The MVP only needs to recognise the handful of
/// instructions the harness exercises.
#[derive(Debug, Default, Clone, Copy)]
pub struct Decoder {
    _private: (),
}

impl Decoder {
    /// Decode a single instruction.
    ///
    /// The instruction width follows from the two low bits of the
    /// fetched word: `0b11` marks a 32-bit encoding, anything else a
    /// 16-bit compressed one (RISC-V base instruction formats).
    pub fn decode(&self, raw: u32, pc: u32) -> Decoded {
        if raw & 0b11 != 0b11 {
            self.decode_compressed((raw & 0xFFFF) as u16, pc)
        } else {
            self.decode32(raw, pc)
        }
    }

    /// Decode a 32-bit encoding.
    fn decode32(&self, raw: u32, pc: u32) -> Decoded {
        let opcode = (raw & 0x7F) as u8;
        let rd = ((raw >> 7) & 0x1F) as u8;
        let funct3 = ((raw >> 12) & 0x07) as u8;
        let rs1 = ((raw >> 15) & 0x1F) as u8;
        let rs2 = ((raw >> 20) & 0x1F) as u8;
        let funct7 = ((raw >> 25) & 0x7F) as u8;

        let imm = immediate(opcode, funct3, raw);

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
            len: LEN_32,
            compressed: false,
            compressed_raw: 0,
            illegal: false,
        }
    }

    /// Decode a 16-bit compressed encoding.
    ///
    /// The compressed instruction is expanded into the fields of the
    /// equivalent 32-bit instruction; the PC still advances by two
    /// bytes, and a jump's link register therefore holds `pc + 2`.
    fn decode_compressed(&self, inst: u16, pc: u32) -> Decoded {
        match compressed::expand(inst) {
            Some(e) => Decoded {
                opcode: e.opcode,
                funct3: e.funct3,
                funct7: e.funct7,
                rd: e.rd,
                rs1: e.rs1,
                rs2: e.rs2,
                imm: e.imm,
                raw: inst as u32,
                pc,
                len: LEN_16,
                compressed: true,
                compressed_raw: inst,
                illegal: false,
            },
            None => Decoded {
                opcode: 0,
                funct3: 0,
                funct7: 0,
                rd: 0,
                rs1: 0,
                rs2: 0,
                imm: 0,
                raw: inst as u32,
                pc,
                len: LEN_16,
                compressed: true,
                compressed_raw: inst,
                illegal: true,
            },
        }
    }
}

/// Extract the immediate of a 32-bit encoding, sign-extended, according
/// to the instruction's format (LLD §3.1 immediate table).
///
/// The format is selected by the opcode — the only field that tells the
/// formats apart — with one exception: the OP-IMM shift
/// instructions carry a 5-bit shift amount in place of the 12-bit
/// immediate (LLD §3.3 SLLI / SRLI / SRAI).
fn immediate(opcode: u8, funct3: u8, raw: u32) -> i32 {
    match opcode {
        // U-type: imm[31:12] = inst[31:12], the low 12 bits are zero.
        // The value is already the final one — LUI assigns it as-is and
        // AUIPC adds it to the PC.
        0b011_0111 | 0b001_0111 => (raw & 0xFFFF_F000) as i32,
        // S-type: imm[11:5] = inst[31:25], imm[4:0] = inst[11:7].
        // The immediate is 12 bits wide, sign bit in imm[11].
        0b010_0011 => {
            let imm = (((raw >> 25) & 0x7F) << 5) | ((raw >> 7) & 0x1F);
            sign_extend(imm, 12)
        }
        // B-type: imm[12] = inst[31], imm[10:5] = inst[30:25],
        // imm[4:1] = inst[11:8], imm[11] = inst[7]. The immediate is
        // 13 bits wide, sign bit in imm[12], and already scaled by two.
        0b110_0011 => {
            let imm = (((raw >> 31) & 0x1) << 12)
                | (((raw >> 25) & 0x3F) << 5)
                | (((raw >> 8) & 0xF) << 1)
                | (((raw >> 7) & 0x1) << 11);
            sign_extend(imm, 13)
        }
        // J-type: imm[20] = inst[31], imm[10:1] = inst[30:21],
        // imm[11] = inst[20], imm[19:12] = inst[19:12]. The immediate
        // is 21 bits wide, sign bit in imm[20], and already scaled by
        // two.
        0b110_1111 => {
            let imm = (((raw >> 31) & 0x1) << 20)
                | (((raw >> 21) & 0x3FF) << 1)
                | (((raw >> 20) & 0x1) << 11)
                | (((raw >> 12) & 0xFF) << 12);
            sign_extend(imm, 21)
        }
        // OP-IMM shifts carry a 5-bit shift amount, not an immediate.
        0b001_0011 if funct3 == 0b001 || funct3 == 0b101 => ((raw >> 20) & 0x1F) as i32,
        // I-type: imm[11:0] = inst[31:20].
        _ => sign_extend((raw >> 20) & 0xFFF, 12),
    }
}

/// Sign-extend the low `bits` bits of `value` to a 32-bit immediate.
pub(crate) fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

/// Width of the access the executor should perform when reading
/// instruction memory.
///
/// The fetch is always 32 bits wide: the hardware fetches a word and
/// selects the low 16 bits when the instruction turns out to be
/// compressed (LLD §3.5, C.LW row).  A two-byte fetch would have to
/// read the halfword first to learn the width, which doubles the
/// number of bus transactions for every 32-bit instruction and
/// changes the bus statistics the rest of the platform reports.
pub fn fetch_size() -> WriteSize {
    WriteSize::Word
}
