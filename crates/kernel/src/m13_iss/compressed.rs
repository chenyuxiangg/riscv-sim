//! RV32C (compressed) instruction expansion.
//!
//! Every compressed encoding is expanded into the **fields of the
//! equivalent 32-bit instruction** (LLD §3.5「等价 32 位展开」), and the
//! expanded fields are executed by the ordinary RV32I / RV32M path.
//! There is no second executor for compressed instructions: `c.add`
//! *is* `add`, `c.lw` *is* `lw`.
//!
//! ## Why fields and not a 32-bit bit pattern
//!
//! The LLD's decode sketch expands to a 32-bit **encoding**
//! (`wide = c_expand(inst)`) and then fills the fields from it.  That
//! shape works for most of the compressed immediates, but it forces
//! every one of them through a 32-bit immediate field, and the
//! compressed immediates are not all 32-bit-shaped:
//!
//! * `C.LUI`'s immediate is the 6-bit field sign-extended and then
//!   truncated to the **20-bit** LUI field.  For the field `0b100001`
//!   (i.e. -31) that is `0xFFFE1 << 12 = 0xFFFE_1000`, and that is
//!   what the reference simulator produces — a dedicated probe
//!   distinguishes it from the other candidate (`0xFFFF_E000`).
//!   The tempting wrong reading is to take the 20 bits at the **high**
//!   end of the sign-extended value (`sext6 & 0xFFFFF_000` =
//!   `0xFFFF_E000`).  Note that `sext6 << 12` in 32-bit arithmetic is
//!   **not** wrong: the shift truncates to exactly those 20 bits.
//! * `C.ADDI16SP`'s offset is a multiple of 16 in [-512, 496], which
//!   does fit — but only because the compressed field carries the
//!   *byte* offset directly, and an implementation that read the field
//!   as a count and scaled it by 16 would be off by a factor of 16.
//!
//! Expanding to the decoded **fields** states each immediate rule
//! exactly once, carries it at its natural width, and keeps the
//! executor on a single path: `c.add` *is* `add`, `c.lw` *is* `lw`.
//!
//! ## Bit layouts
//!
//! The layouts below were derived from GNU as's own output (every
//! field cross-checked against several operand values) and against the
//! reference simulator.  Two of them are easy to get wrong:
//!
//! * the **CR** format (`C.JR` / `C.MV` / `C.EBREAK` / `C.JALR` /
//!   `C.ADD`) carries a **one-bit** `funct2` in `inst[12]`, and the
//!   register field is the full five bits `inst[11:7]` — the `funct2`
//!   is *not* `inst[11:10]`, because those bits belong to the
//!   register;
//! * the **CI**-group shifts (`C.SRLI` / `C.SRAI` / `C.ANDI`) and the
//!   **CA** group (`C.SUB` / `C.XOR` / `C.OR` / `C.AND`) share the
//!   `Q1/100` slot and are told apart by `inst[11:10]`: the CI
//!   group's `funct2` *is* `inst[11:10]`, while the CA group's is
//!   `inst[6:5]` and the CA format additionally requires
//!   `inst[12] == 0`;

use super::decoder::sign_extend;

/// Opcode of the OP-IMM group.
const OP_IMM: u8 = 0b001_0011;
/// Opcode of the OP (register-register) group.
const OP: u8 = 0b011_0011;
/// Opcode of the LUI group.
const LUI: u8 = 0b011_0111;
/// Opcode of the LOAD group.
const LOAD: u8 = 0b000_0011;
/// Opcode of the STORE group.
const STORE: u8 = 0b010_0011;
/// Opcode of the BRANCH group.
const BRANCH: u8 = 0b110_0011;
/// Opcode of the JALR instruction.
const JALR: u8 = 0b110_0111;
/// Opcode of the JAL instruction.
const JAL: u8 = 0b110_1111;
/// Opcode of the SYSTEM group (ECALL / EBREAK).
const SYSTEM: u8 = 0b111_0011;

/// The architectural fields a compressed instruction expands to.
///
/// The field set is exactly the one
/// [`super::decoder::Decoded`] carries for a 32-bit encoding: a
/// compressed instruction is described by the same
/// opcode / funct3 / funct7 / register / immediate set as the 32-bit
/// instruction it stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expanded {
    /// 7-bit primary opcode of the equivalent 32-bit instruction.
    pub opcode: u8,
    /// 3-bit funct3 of the equivalent 32-bit instruction.
    pub funct3: u8,
    /// 7-bit funct7 of the equivalent 32-bit instruction.
    pub funct7: u8,
    /// Destination register.
    pub rd: u8,
    /// Source register 1.
    pub rs1: u8,
    /// Source register 2.
    pub rs2: u8,
    /// Immediate, already sign-extended where the format is signed.
    pub imm: i32,
}

impl Expanded {
    /// A helper for the instructions that need no register 2.
    fn new(opcode: u8, funct3: u8, rd: u8, rs1: u8, imm: i32) -> Self {
        Self {
            opcode,
            funct3,
            funct7: 0,
            rd,
            rs1,
            rs2: 0,
            imm,
        }
    }
}

/// Expand a 16-bit compressed encoding into the fields of the
/// equivalent 32-bit instruction.
///
/// `None` means the encoding is **reserved**: the RISC-V C extension
/// either assigns those code points to the D / F extensions (which are
/// out of scope for RV32IMC) or leaves them reserved, and the ISS must
/// trap on them as illegal instructions.
///
/// The expansion covers the 26 instructions of the RV32C base set
/// (LLD §3.5).  Everything else is either reserved or belongs to an
/// extension this ISS does not implement.
pub fn expand(inst: u16) -> Option<Expanded> {
    match inst & 0b11 {
        0b00 => expand_q0(inst),
        0b01 => expand_q1(inst),
        0b10 => expand_q2(inst),
        // 0b11 marks a 32-bit encoding — not a compressed instruction.
        _ => None,
    }
}

/// The 6-bit CI immediate: `inst[12]` is the sign bit and `inst[6:2]`
/// carry the low five bits.
fn ci_imm(inst: u16) -> i32 {
    sign_extend(
        ((((inst >> 12) & 0b1) << 5) | ((inst >> 2) & 0x1F)) as u32,
        6,
    )
}

/// The 5-bit shift amount of the compressed shifts: `inst[12]` is
/// `shamt[5]` and `inst[6:2]` carry `shamt[4:0]`.
fn shift_amount(inst: u16) -> i32 {
    ((((inst >> 12) & 0b1) << 5) | ((inst >> 2) & 0x1F)) as i32
}

/// The 7-bit CL / CS load-store offset: `uimm[5:3]` come from
/// `inst[12:10]`, `uimm[2]` from `inst[6]` and `uimm[6]` from
/// `inst[5]`.
fn cl_uimm(inst: u16) -> i32 {
    ((((inst >> 10) & 0b111) << 3) | (((inst >> 6) & 0b1) << 2) | (((inst >> 5) & 0b1) << 6)) as i32
}

/// The 12-bit CJ jump offset (`C.J` / `C.JAL`).
///
/// The field is the **byte** offset: its low bit is always zero and
/// its sign bit is bit 11, so the reachable range is [-2048, 2046]
/// bytes.  (It is not a halfword count that still has to be scaled —
/// the bit pattern already carries the byte distance; scaling it a
/// second time sends every jump twice as far.)
///
/// `inst[12]` is `imm[11]`, `inst[11]` is `imm[4]`, `inst[10:9]` are
/// `imm[9:8]`, `inst[8]` is `imm[10]`, `inst[7]` is `imm[6]`,
/// `inst[6]` is `imm[7]`, `inst[5:3]` are `imm[3:1]` and `inst[2]` is
/// `imm[5]`.
fn cj_imm(inst: u16) -> i32 {
    let imm = (((inst >> 12) & 0b1) << 11)
        | (((inst >> 8) & 0b1) << 10)
        | (((inst >> 9) & 0b11) << 8)
        | (((inst >> 6) & 0b1) << 7)
        | (((inst >> 7) & 0b1) << 6)
        | (((inst >> 2) & 0b1) << 5)
        | (((inst >> 11) & 0b1) << 4)
        | (((inst >> 3) & 0b111) << 1);
    sign_extend(imm as u32, 12)
}

/// The 9-bit CB branch offset (`C.BEQZ` / `C.BNEZ`).
///
/// Like the CJ offset it is the **byte** offset, with the sign at bit
/// 8 and the low bit always zero — the reachable range is
/// [-256, 254] bytes.
///
/// `inst[12]` is `imm[8]`, `inst[11:10]` are `imm[4:3]`, `inst[6:5]`
/// are `imm[7:6]`, `inst[4:3]` are `imm[2:1]` and `inst[2]` is
/// `imm[5]`.
fn cb_imm(inst: u16) -> i32 {
    let imm = (((inst >> 12) & 0b1) << 8)
        | (((inst >> 10) & 0b11) << 3)
        | (((inst >> 5) & 0b11) << 6)
        | (((inst >> 3) & 0b11) << 1)
        | (((inst >> 2) & 0b1) << 5);
    sign_extend(imm as u32, 9)
}

/// Expand a quadrant-0 encoding (`C.ADDI4SPN` / `C.LW` / `C.SW`).
fn expand_q0(inst: u16) -> Option<Expanded> {
    match (inst >> 13) & 0b111 {
        // C.ADDI4SPN — `addi rd', sp, nzuimm`.
        0b000 => {
            // nzuimm[5:4] = inst[12:11], nzuimm[9:6] = inst[10:7],
            // nzuimm[2] = inst[6], nzuimm[3] = inst[5].
            let nzuimm = ((((inst >> 11) & 0b11) << 4)
                | (((inst >> 7) & 0b1111) << 6)
                | (((inst >> 6) & 0b1) << 2)
                | (((inst >> 5) & 0b1) << 3)) as i32;
            if nzuimm == 0 {
                // "The code points with nzuimm=0 are reserved."
                return None;
            }
            let rd = 8 + ((inst >> 2) & 0b111) as u8;
            Some(Expanded::new(OP_IMM, 0b000, rd, 2, nzuimm))
        }
        // C.LW — `lw rd', uimm(rs1')`.
        0b010 => {
            let rd = 8 + ((inst >> 2) & 0b111) as u8;
            let rs1 = 8 + ((inst >> 7) & 0b111) as u8;
            Some(Expanded::new(LOAD, 0b010, rd, rs1, cl_uimm(inst)))
        }
        // C.SW — `sw rs2', uimm(rs1')`.
        //
        // The funct3 of C.SW is 0b110; 0b101 in this quadrant is
        // C.FSW, which belongs to the F extension.
        0b110 => {
            let rs1 = 8 + ((inst >> 7) & 0b111) as u8;
            let rs2 = 8 + ((inst >> 2) & 0b111) as u8;
            let mut e = Expanded::new(STORE, 0b010, 0, rs1, cl_uimm(inst));
            e.rs2 = rs2;
            Some(e)
        }
        // C.FLD / C.FLW / C.FSD / C.FSW (D and F extensions) and the
        // reserved code points are not part of RV32IMC.
        _ => None,
    }
}

/// Expand a quadrant-1 encoding (`C.ADDI` … `C.BNEZ`).
fn expand_q1(inst: u16) -> Option<Expanded> {
    match (inst >> 13) & 0b111 {
        // C.ADDI / C.NOP — `addi rd, rd, nzimm`.
        0b000 => {
            let imm = ci_imm(inst);
            let rd = ((inst >> 7) & 0x1F) as u8;
            Some(Expanded::new(OP_IMM, 0b000, rd, rd, imm))
        }
        // C.JAL — `jal x1, offset` (RV32 only; C.ADDIW in RV64).
        0b001 => Some(Expanded::new(JAL, 0b000, 1, 0, cj_imm(inst))),
        // C.LI — `addi rd, x0, imm`.
        0b010 => {
            let imm = ci_imm(inst);
            let rd = ((inst >> 7) & 0x1F) as u8;
            Some(Expanded::new(OP_IMM, 0b000, rd, 0, imm))
        }
        // C.ADDI16SP (`rd` = x2) and C.LUI (every other `rd`).
        0b011 => {
            let rd = ((inst >> 7) & 0x1F) as u8;
            if rd == 2 {
                // C.ADDI16SP — `addi x2, x2, nzimm`.
                // nzimm[9] = inst[12], nzimm[8:7] = inst[4:3],
                // nzimm[6] = inst[5], nzimm[5] = inst[2],
                // nzimm[4] = inst[6], nzimm[3:0] = 0.
                //
                // The four low bits are always zero, so the offset is a
                // multiple of 16 in [-512, 496] — the bit pattern
                // already carries the *byte* offset, it is not a count
                // that still has to be scaled.
                let nzimm = ((((inst >> 12) & 0b1) << 9)
                    | (((inst >> 3) & 0b11) << 7)
                    | (((inst >> 5) & 0b1) << 6)
                    | (((inst >> 2) & 0b1) << 5)
                    | (((inst >> 6) & 0b1) << 4)) as u32;
                if nzimm == 0 {
                    // "The code point with nzimm=0 is reserved."
                    return None;
                }
                let imm = sign_extend(nzimm, 10);
                Some(Expanded::new(OP_IMM, 0b000, 2, 2, imm))
            } else {
                // C.LUI — `lui rd, imm`.
                let imm = ci_imm(inst);
                if imm == 0 {
                    // "C.LUI is only valid when ... the immediate is
                    // not equal to zero" — a zero immediate is a
                    // **reserved** encoding, and the reference
                    // simulator traps on it.  (It is not a HINT: a
                    // HINT would have to leave rd alone, and the
                    // reference implementation does not retire the
                    // instruction at all.)
                    return None;
                }
                // The LUI immediate field is 20 bits wide.  The
                // compressed immediate is the 6-bit field sign-extended
                // to 32 bits and then truncated to those 20 bits — the
                // reference simulator computes `sext32(RS1 << 12)`,
                // which is the same value.
                let imm20 = (imm as u32) & 0xF_FFFF;
                Some(Expanded::new(LUI, 0b000, rd, 0, (imm20 << 12) as i32))
            }
        }
        // C.SRLI / C.SRAI / C.ANDI and C.SUB / C.XOR / C.OR / C.AND.
        0b100 => expand_q1_100(inst),
        // C.J — `jal x0, offset`.
        0b101 => Some(Expanded::new(JAL, 0b000, 0, 0, cj_imm(inst))),
        // C.BEQZ / C.BNEZ — `beq rs1', x0, offset`.
        0b110 | 0b111 => {
            let rs1 = 8 + ((inst >> 7) & 0b111) as u8;
            let funct3 = ((inst >> 13) & 0b111) as u8 & 0b1;
            Some(Expanded::new(BRANCH, funct3, 0, rs1, cb_imm(inst)))
        }
        _ => None,
    }
}

/// Expand the `Q1/100` slot, which holds two different formats.
///
/// The **CI** group (`C.SRLI` / `C.SRAI` / `C.ANDI`) and the **CA**
/// group (`C.SUB` / `C.XOR` / `C.OR` / `C.AND`) share the slot and are
/// told apart by `inst[11:10]`:
///
/// * `inst[11:10] != 0b11` — CI group.  `inst[11:10]` is the
///   funct2 (00 = SRLI, 01 = SRAI, 10 = ANDI), `inst[12]` is
///   `shamt[5]` / `imm[5]` and `inst[9:7]` is the destination.
/// * `inst[11:10] == 0b11` — CA group.  `inst[12]` must be zero,
///   `inst[6:5]` is the funct2 (00 = SUB, 01 = XOR, 10 = OR,
///   11 = AND), `inst[9:7]` is `rd'` / `rs1'` and `inst[4:2]` is
///   `rs2'`.
///
/// Getting this wrong silently swaps `C.ANDI` for `C.OR`: the CA
/// group's funct2 sits exactly where the CI group's `imm[4:3]` sits.
fn expand_q1_100(inst: u16) -> Option<Expanded> {
    let rd = 8 + ((inst >> 7) & 0b111) as u8;
    if (inst >> 10) & 0b11 != 0b11 {
        // CI group: C.SRLI / C.SRAI / C.ANDI.
        match (inst >> 10) & 0b11 {
            0b00 | 0b01 => {
                // C.SRLI / C.SRAI — `srli rd', rd', shamt`.
                let shamt = shift_amount(inst);
                if shamt & 0b100_000 != 0 {
                    // RV32: `shamt[5]` must be zero; the code points
                    // with `shamt[5]` = 1 are reserved for custom
                    // extensions.
                    return None;
                }
                let funct7 = if (inst >> 10) & 0b11 == 0b01 {
                    0b010_0000
                } else {
                    0b000_0000
                };
                let mut e = Expanded::new(OP_IMM, 0b101, rd, rd, shamt);
                e.funct7 = funct7;
                Some(e)
            }
            0b10 => {
                // C.ANDI — `andi rd', rd', imm`.
                Some(Expanded::new(OP_IMM, 0b111, rd, rd, ci_imm(inst)))
            }
            // Unreachable: `inst[11:10] == 0b11` is the CA group.
            _ => None,
        }
    } else {
        // CA group: C.SUB / C.XOR / C.OR / C.AND.
        if (inst >> 12) & 0b1 != 0 {
            // The CA format requires `inst[12]` = 0.
            return None;
        }
        let rs2 = 8 + ((inst >> 2) & 0b111) as u8;
        let (funct7, funct3) = match (inst >> 5) & 0b11 {
            0b00 => (0b010_0000, 0b000), // C.SUB
            0b01 => (0b000_0000, 0b100), // C.XOR
            0b10 => (0b000_0000, 0b110), // C.OR
            _ => (0b000_0000, 0b111),    // C.AND
        };
        let mut e = Expanded::new(OP, funct3, rd, rd, 0);
        e.funct7 = funct7;
        e.rs2 = rs2;
        Some(e)
    }
}

/// Expand a quadrant-2 encoding (`C.SLLI` … `C.SWSP`).
fn expand_q2(inst: u16) -> Option<Expanded> {
    match (inst >> 13) & 0b111 {
        // C.SLLI — `slli rd, rd, shamt`.
        0b000 => {
            let shamt = shift_amount(inst);
            if shamt & 0b100_000 != 0 {
                // RV32: `shamt[5]` must be zero.
                return None;
            }
            let rd = ((inst >> 7) & 0x1F) as u8;
            // `rd` = x0 is a HINT; expanding it to `slli x0, x0, shamt`
            // writes x0, which the register file discards — no
            // architectural effect either way.
            Some(Expanded::new(OP_IMM, 0b001, rd, rd, shamt))
        }
        // C.LWSP — `lw rd, uimm(x2)`.
        0b010 => {
            let rd = ((inst >> 7) & 0x1F) as u8;
            if rd == 0 {
                // "The code points with rd=x0 are reserved."
                return None;
            }
            // uimm[5] = inst[12], uimm[4:2] = inst[6:4],
            // uimm[7:6] = inst[3:2].
            let uimm = ((((inst >> 12) & 0b1) << 5)
                | (((inst >> 4) & 0b111) << 2)
                | (((inst >> 2) & 0b11) << 6)) as i32;
            Some(Expanded::new(LOAD, 0b010, rd, 2, uimm))
        }
        // CR group: C.JR / C.MV / C.EBREAK / C.JALR / C.ADD.
        0b100 => expand_q2_cr(inst),
        // C.SWSP — `sw rs2, uimm(x2)`.
        0b110 => {
            // uimm[5:2] = inst[12:9], uimm[7:6] = inst[8:7].
            let uimm = ((((inst >> 9) & 0b1111) << 2) | (((inst >> 7) & 0b11) << 6)) as i32;
            let rs2 = ((inst >> 2) & 0x1F) as u8;
            let mut e = Expanded::new(STORE, 0b010, 0, 2, uimm);
            e.rs2 = rs2;
            Some(e)
        }
        // C.FLDSP / C.FLWSP / C.FSDSP and the reserved code points are
        // not part of RV32IMC.
        _ => None,
    }
}

/// Expand the CR slot (`C.JR` / `C.MV` / `C.EBREAK` / `C.JALR` /
/// `C.ADD`).
///
/// The CR format carries a **one-bit** `funct2` in `inst[12]`; the
/// register field is the five bits `inst[11:7]` and the second
/// register is `inst[6:2]`.  `funct2` = 0 selects `C.JR` / `C.MV`,
/// `funct2` = 1 selects `C.EBREAK` / `C.JALR` / `C.ADD`; within a
/// pair, `rs2` = x0 tells the two apart.
fn expand_q2_cr(inst: u16) -> Option<Expanded> {
    let field = ((inst >> 7) & 0x1F) as u8;
    let rs2 = ((inst >> 2) & 0x1F) as u8;
    if (inst >> 12) & 0b1 == 0 {
        if rs2 == 0 {
            // C.JR — `jalr x0, 0(rs1)`.
            if field == 0 {
                // "The code points with rs1=x0 are reserved."
                return None;
            }
            Some(Expanded::new(JALR, 0b000, 0, field, 0))
        } else {
            // C.MV — `add rd, x0, rs2`.
            //
            // `rd` = x0 is **not** reserved: the reference simulator
            // retires the instruction, and `add x0, x0, rs2` writes
            // nothing, so the effect is a no-op.
            let mut e = Expanded::new(OP, 0b000, field, 0, 0);
            e.rs2 = rs2;
            Some(e)
        }
    } else if rs2 == 0 {
        if field == 0 {
            // C.EBREAK — `ebreak`.
            Some(Expanded::new(SYSTEM, 0b000, 0, 0, 1))
        } else {
            // C.JALR — `jalr x1, 0(rs1)`.
            Some(Expanded::new(JALR, 0b000, 1, field, 0))
        }
    } else {
        // C.ADD — `add rd, rd, rs2`.
        //
        // As with C.MV, `rd` = x0 is not reserved: the instruction
        // retires and writes nothing.
        let mut e = Expanded::new(OP, 0b000, field, field, 0);
        e.rs2 = rs2;
        Some(e)
    }
}
