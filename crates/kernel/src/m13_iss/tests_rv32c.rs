//! RV32C coverage for M-13: the 26 compressed instructions of the
//! RV32C base set, their expansion into the equivalent 32-bit
//! instruction, the reserved encodings and the HINT encodings.
//!
//! ## Where the encodings come from
//!
//! The **main table** of legal encodings was produced by GNU as and
//! checked against it; they are **not** hand-assembled bit patterns.
//! That matters: the compressed formats have several bit fields that
//! are easy to mis-place (the CR format's one-bit `funct2` in
//! `inst[12]`, the CA format's `funct2` in `inst[6:5]`, the Q1/100
//! slot's two different formats), and a test that encodes the
//! instruction the same way the implementation decodes it would agree
//! with a broken decoder.
//!
//! The **reserved / HINT table** is a different story: GNU as
//! *refuses* most of those code points ("illegal operands" for
//! `c.srli x8, 0`, `c.slli x0, 0`, `c.lwsp x0, 0`, `c.jr x0`, …), so
//! they were classified against the **reference simulator** instead
//! and cross-checked with an independent spec-derived decoder.
//!
//! ## Where the expected values come from
//!
//! The architectural values are the ones the RISC-V specification
//! defines, cross-checked against Spike (the golden reference) — the
//! `C.LUI` case in particular: the specification's "6-bit immediate
//! sign-extended into the 20-bit LUI field" is *not* the same as
//! "the 6-bit immediate sign-extended and **masked at the high end**"
//! (`0xFFFE_1000` vs `0xFFFF_E000`).  A dedicated Spike probe
//! distinguishes the two: it reports `0xFFFE_1000`.

use riscv_sim_contract::csr::{CSR_MEPC, CSR_MTVEC};
use riscv_sim_contract::model::{SimState, WriteSize};

use super::decoder::{Decoder, LEN_16};
use super::executor::{classify, Op, StepOutcome, TrapCause};
use super::tests_common::*;

// ---------------------------------------------------------------------------
// Golden encodings (GNU as)
// ---------------------------------------------------------------------------

/// `c.nop`
const C_NOP: u16 = 0x0001;
/// `c.addi x8, -16`
const C_ADDI_X8_M16: u16 = 0x1441;
/// `c.addi x31, -16`
const C_ADDI_X31_M16: u16 = 0x1FC1;
/// `c.addi x5, 31`
const C_ADDI_X5_31: u16 = 0x02FD;
/// `c.addi x5, -1`
const C_ADDI_X5_M1: u16 = 0x12FD;
/// `c.li x8, -16`
const C_LI_X8_M16: u16 = 0x5441;
/// `c.li x5, 31`
const C_LI_X5_31: u16 = 0x42FD;
/// `c.lui x8, 1`
const C_LUI_X8_1: u16 = 0x6405;
/// `c.lui x8, 31`
const C_LUI_X8_31: u16 = 0x647D;
/// `c.lui x8, 0xfffe1` — the field is `0b100001`.
const C_LUI_X8_FFFE1: u16 = 0x7405;
/// `c.addi16sp sp, 496`
const C_ADDI16SP_496: u16 = 0x617D;
/// `c.addi16sp sp, -512`
const C_ADDI16SP_M512: u16 = 0x7101;
/// `c.addi16sp sp, 16`
const C_ADDI16SP_16: u16 = 0x6141;
/// `c.addi4spn x8, sp, 1020`
const C_ADDI4SPN_1020: u16 = 0x1FE0;
/// `c.addi4spn x8, sp, 4`
const C_ADDI4SPN_4: u16 = 0x0040;
/// `c.slli x8, 4`
const C_SLLI_X8_4: u16 = 0x0412;
/// `c.slli x8, 31`
const C_SLLI_X8_31: u16 = 0x047E;
/// `c.slli x31, 1` — the destination register is the full five bits.
const C_SLLI_X31_1: u16 = 0x0F86;
/// `c.slli x16, 3`
const C_SLLI_X16_3: u16 = 0x080E;
/// `c.srli x8, 12`
const C_SRLI_X8_12: u16 = 0x8031;
/// `c.srai x8, 12`
const C_SRAI_X8_12: u16 = 0x8431;
/// `c.andi x8, -17`
const C_ANDI_X8_M17: u16 = 0x983D;
/// `c.andi x8, 15`
const C_ANDI_X8_15: u16 = 0x883D;
/// `c.sub x8, x9`
const C_SUB_X8_X9: u16 = 0x8C05;
/// `c.xor x8, x9`
const C_XOR_X8_X9: u16 = 0x8C25;
/// `c.or x8, x9`
const C_OR_X8_X9: u16 = 0x8C45;
/// `c.and x8, x9`
const C_AND_X8_X9: u16 = 0x8C65;
/// `c.mv x5, x10`
const C_MV_X5_X10: u16 = 0x82AA;
/// `c.add x5, x10`
const C_ADD_X5_X10: u16 = 0x92AA;
/// `c.jr x5`
const C_JR_X5: u16 = 0x8282;
/// `c.jalr x5`
const C_JALR_X5: u16 = 0x9282;
/// `c.mv x17, x10` — the register field's **top bit** is set, so
/// `inst[11]` is 1.  This is the discriminating case for the CR
/// format's one-bit `funct2`: a decoder that read the funct2 from
/// `inst[11]` (or `inst[11:10]`) would decode `c.jr x5` correctly by
/// accident, because `inst[11]` happens to be zero there.
const C_MV_X17_X10: u16 = 0x88AA;
/// `c.jr x17` — see `C_MV_X17_X10`.
const C_JR_X17: u16 = 0x8882;
/// `c.mv x31, x10` — `inst[11:10] == 0b11`.  This is the discriminating
/// case for a decoder that reads the CR format's `funct2` as **two**
/// bits from `inst[11:10]`: for every register below `x24` those two
/// bits are not `0b11`, so the mistake hides.
const C_MV_X31_X10: u16 = 0x8FAA;
/// `c.jr x31` — see `C_MV_X31_X10`.
const C_JR_X31: u16 = 0x8F82;
/// `c.ebreak`
const C_EBREAK: u16 = 0x9002;
/// `c.lw x8, 4(x9)`
const C_LW_X8_4_X9: u16 = 0x40C0;
/// `c.lw x8, 124(x9)`
const C_LW_X8_124_X9: u16 = 0x5CE0;
/// `c.sw x8, 4(x9)`
const C_SW_X8_4_X9: u16 = 0xC0C0;
/// `c.lwsp x8, 12(sp)`
const C_LWSP_X8_12: u16 = 0x4432;
/// `c.lwsp x8, 252(sp)`
const C_LWSP_X8_252: u16 = 0x547E;
/// `c.swsp x8, 12(sp)`
const C_SWSP_X8_12: u16 = 0xC622;
/// `c.swsp x31, 252(sp)` — the largest offset, from a high register.
const C_SWSP_X31_252: u16 = 0xDFFE;
/// `c.j` with a +4 byte offset.
const C_J_P4: u16 = 0xA011;
/// `c.j` with a -4 byte offset.
const C_J_M4: u16 = 0xBFF5;
/// `c.jal` with a +4 byte offset.
const C_JAL_P4: u16 = 0x2011;
/// `c.beqz x8` with a +4 byte offset.
const C_BEQZ_X8_P4: u16 = 0xC011;
/// `c.bnez x8` with a +4 byte offset.
const C_BNEZ_X8_P4: u16 = 0xE011;
/// `c.beqz x8` with a +2 byte offset — the smallest forward jump.
const C_BEQZ_X8_P2: u16 = 0xC009;
/// `c.bnez x8` with a +2 byte offset.
const C_BNEZ_X8_P2: u16 = 0xE009;
/// `c.beqz x8` with a +8 byte offset.
const C_BEQZ_X8_P8: u16 = 0xC401;
/// `c.bnez x8` with a +8 byte offset.
const C_BNEZ_X8_P8: u16 = 0xE401;
/// `c.beqz x8` with a +26 byte offset.
const C_BEQZ_X8_P26: u16 = 0xCC09;
/// `c.bnez x8` with a +26 byte offset.
const C_BNEZ_X8_P26: u16 = 0xEC09;
/// `c.beqz x8` with a +80 byte offset — `imm[7:6] == 0b01`.
const C_BEQZ_X8_P80: u16 = 0xC821;
/// `c.bnez x8` with a +80 byte offset.
const C_BNEZ_X8_P80: u16 = 0xE821;
/// `c.beqz x8` with a -4 byte offset — the sign bit is set.
const C_BEQZ_X8_M4: u16 = 0xDC75;
/// `c.bnez x8` with a -4 byte offset.
const C_BNEZ_X8_M4: u16 = 0xFC75;
/// `c.beqz x8` with a -256 byte offset — the far end of the range.
const C_BEQZ_X8_M256: u16 = 0xD001;
/// `c.bnez x8` with a -256 byte offset.
const C_BNEZ_X8_M256: u16 = 0xF001;
/// `c.beqz x8` with a +130 byte offset — `imm[7:6] == 0b10`.
const C_BEQZ_X8_P130: u16 = 0xC049;
/// `c.bnez x8` with a +130 byte offset.
const C_BNEZ_X8_P130: u16 = 0xE049;
/// `c.beqz x8` with a +6 byte offset — `imm[2:1] == 0b11`.
const C_BEQZ_X8_P6: u16 = 0xC019;
/// `c.bnez x8` with a +6 byte offset.
const C_BNEZ_X8_P6: u16 = 0xE019;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Step one compressed instruction at `pc` and return the outcome.
fn step_c(h: &mut Harness, state: &mut SimState, pc: u32, enc: u16) -> StepOutcome {
    h.step_half_at(state, pc, enc)
}

/// Decode a compressed encoding.
fn decode_c(enc: u16) -> super::decoder::Decoded {
    Decoder::default().decode(enc as u32, 0x1000)
}

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

/// Every compressed encoding must decode to a two-byte instruction and
/// to the operation of the 32-bit instruction it stands for.
#[test]
fn every_compressed_instruction_decodes_to_the_equivalent_operation() {
    let table: [(u16, Op); 27] = [
        (C_NOP, Op::Addi),
        (C_ADDI_X8_M16, Op::Addi),
        (C_ADDI_X31_M16, Op::Addi),
        (C_LI_X8_M16, Op::Addi),
        (C_LUI_X8_1, Op::Lui),
        (C_ADDI16SP_496, Op::Addi),
        (C_ADDI4SPN_1020, Op::Addi),
        (C_SLLI_X8_4, Op::Slli),
        (C_SRLI_X8_12, Op::Srli),
        (C_SRAI_X8_12, Op::Srai),
        (C_ANDI_X8_M17, Op::Andi),
        (C_SUB_X8_X9, Op::Sub),
        (C_XOR_X8_X9, Op::Xor),
        (C_OR_X8_X9, Op::Or),
        (C_AND_X8_X9, Op::And),
        (C_J_P4, Op::Jal),
        (C_JAL_P4, Op::Jal),
        (C_BEQZ_X8_P4, Op::Beq),
        (C_BNEZ_X8_P4, Op::Bne),
        (C_LW_X8_4_X9, Op::Lw),
        (C_SW_X8_4_X9, Op::Sw),
        (C_LWSP_X8_12, Op::Lw),
        (C_SWSP_X8_12, Op::Sw),
        (C_JR_X5, Op::Jalr),
        (C_JALR_X5, Op::Jalr),
        (C_MV_X5_X10, Op::Add),
        (C_ADD_X5_X10, Op::Add),
    ];
    for (enc, expected) in table {
        let d = decode_c(enc);
        assert_eq!(d.len, LEN_16, "0x{enc:04x} must be two bytes long");
        assert!(d.compressed, "0x{enc:04x} must be flagged as compressed");
        assert_eq!(d.compressed_raw, enc, "the original encoding is kept");
        assert!(!d.illegal, "0x{enc:04x} must not be illegal");
        assert_eq!(
            classify(&d),
            Some(expected),
            "0x{enc:04x} must decode to {expected:?}"
        );
    }
}

/// The compressed instruction must carry the **fields** of the 32-bit
/// instruction it stands for — not a re-interpretation of the raw
/// bits.
#[test]
fn a_compressed_instruction_carries_the_fields_of_its_32_bit_equivalent() {
    // c.addi x8, -16  ==  addi x8, x8, -16
    let d = decode_c(C_ADDI_X8_M16);
    assert_eq!(d.opcode, OP_IMM);
    assert_eq!(d.rd, 8);
    assert_eq!(d.rs1, 8, "c.addi adds to the register it names");
    assert_eq!(d.imm, -16);

    // c.lw x8, 4(x9)  ==  lw x8, 4(x9)
    let d = decode_c(C_LW_X8_4_X9);
    assert_eq!(d.opcode, LOAD);
    assert_eq!(d.rd, 8);
    assert_eq!(d.rs1, 9);
    assert_eq!(d.imm, 4);

    // c.sub x8, x9  ==  sub x8, x8, x9
    let d = decode_c(C_SUB_X8_X9);
    assert_eq!(d.opcode, OP);
    assert_eq!(d.funct7, 0b010_0000, "c.sub is a SUB, not an ADD");
    assert_eq!(d.rd, 8);
    assert_eq!(d.rs1, 8);
    assert_eq!(d.rs2, 9);

    // c.mv x5, x10  ==  add x5, x0, x10
    let d = decode_c(C_MV_X5_X10);
    assert_eq!(d.opcode, OP);
    assert_eq!(d.rd, 5);
    assert_eq!(d.rs1, 0, "c.mv reads x0");
    assert_eq!(d.rs2, 10);

    // c.jr x5  ==  jalr x0, 0(x5)
    let d = decode_c(C_JR_X5);
    assert_eq!(d.opcode, JALR);
    assert_eq!(d.rd, 0, "c.jr writes no link register");
    assert_eq!(d.rs1, 5);
    assert_eq!(d.imm, 0);

    // c.jalr x5  ==  jalr x1, 0(x5)
    let d = decode_c(C_JALR_X5);
    assert_eq!(d.opcode, JALR);
    assert_eq!(d.rd, 1, "c.jalr links into x1");
    assert_eq!(d.rs1, 5);

    // c.ebreak  ==  ebreak
    let d = decode_c(C_EBREAK);
    assert_eq!(d.opcode, SYSTEM);
    assert_eq!(d.imm, 1, "c.ebreak carries the EBREAK immediate");
}

/// A compressed instruction advances the PC by **two** bytes, never by
/// four (D-16).
#[test]
fn a_compressed_instruction_advances_the_pc_by_two_bytes() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X8_M16);
    assert_eq!(
        state.pc, 0x1002,
        "a compressed instruction must advance the PC by two bytes"
    );
}

/// A 32-bit instruction next to a compressed one must keep advancing
/// by four — the two widths share the fetch, not the step.
#[test]
fn the_two_instruction_widths_step_independently() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X8_M16);
    assert_eq!(state.pc, 0x1002);
    h.step_one_at(&mut state, 0x1002, enc_i(1, 0, 0b000, 1, OP_IMM));
    assert_eq!(state.pc, 0x1006);
}

// ---------------------------------------------------------------------------
// Execution — the arithmetic group
// ---------------------------------------------------------------------------

/// `c.addi` adds the immediate to the register it names.
#[test]
fn c_addi_adds_to_its_own_register() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 100;
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X8_M16);
    assert_eq!(state.gpr[8], 84);

    // The immediate is sign-extended from six bits.
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 0;
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X5_31);
    assert_eq!(state.gpr[5], 31);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 0;
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X5_M1);
    assert_eq!(state.gpr[5], 0xFFFF_FFFF);
}

/// `c.addi` can name **any** register — including the ones the
/// three-bit compressed register field cannot reach.
#[test]
fn c_addi_can_name_any_register() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[31] = 100;
    step_c(&mut h, &mut state, 0x1000, C_ADDI_X31_M16);
    assert_eq!(state.gpr[31], 84, "c.addi x31 must reach x31");
}

/// `c.li` loads a sign-extended immediate.
#[test]
fn c_li_loads_a_sign_extended_immediate() {
    // The destination register is **pre-loaded with a non-zero value**
    // on purpose: `c.li` is `addi rd, x0, imm`, so it must not read
    // `rd`.  With a zero-initialised register the two readings
    // (`addi rd, x0, imm` and `addi rd, rd, imm`) give the same
    // answer, and the case would hold nothing.
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0x0100_0000;
    step_c(&mut h, &mut state, 0x1000, C_LI_X8_M16);
    assert_eq!(state.gpr[8], 0xFFFF_FFF0);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 0x0100_0000;
    step_c(&mut h, &mut state, 0x1000, C_LI_X5_31);
    assert_eq!(state.gpr[5], 31);
}

/// `c.lui` loads the 6-bit immediate into the upper half of the
/// register.
///
/// The value is `sext6(field) << 12` in 32-bit two's complement: the
/// field `0b100001` means `-31`, so the destination is `0xfffe1000`.
/// The specification phrases the same thing as "sign-extended into
/// the 20-bit LUI field", and the two readings coincide in RV32 — a
/// 32-bit left shift already drops exactly those 12 high bits.
///
/// ⚠️ The *wrong* reading is the one that takes the 20 bits at the
/// **high** end of the sign-extended value (`sext6 & 0xfffff000` =
/// `0xffffe000` for the field `0b100001`).  A dedicated reference
/// probe distinguishes the two and reports `0xfffe1000`.
#[test]
fn c_lui_loads_the_truncated_twenty_bit_immediate() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_LUI_X8_1);
    assert_eq!(state.gpr[8], 0x0000_1000);

    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_LUI_X8_31);
    assert_eq!(state.gpr[8], 0x0001_F000);

    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_LUI_X8_FFFE1);
    assert_eq!(
        state.gpr[8], 0xFFFE_1000,
        "the 6-bit field 0b100001 must give the LUI field 0xfffe1"
    );
}

/// `c.addi16sp` adjusts the stack pointer by a multiple of 16.
#[test]
fn c_addi16sp_adjusts_the_stack_pointer() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x1000;
    step_c(&mut h, &mut state, 0x1000, C_ADDI16SP_496);
    assert_eq!(state.gpr[2], 0x1000 + 496);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x1000;
    step_c(&mut h, &mut state, 0x1000, C_ADDI16SP_M512);
    assert_eq!(state.gpr[2], 0x1000 - 512);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x1000;
    step_c(&mut h, &mut state, 0x1000, C_ADDI16SP_16);
    assert_eq!(state.gpr[2], 0x1000 + 16);
}

/// `c.addi4spn` adds a non-zero multiple of 4 to the stack pointer.
#[test]
fn c_addi4spn_adds_to_the_stack_pointer() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x1000;
    step_c(&mut h, &mut state, 0x1000, C_ADDI4SPN_1020);
    assert_eq!(state.gpr[8], 0x1000 + 1020);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x1000;
    step_c(&mut h, &mut state, 0x1000, C_ADDI4SPN_4);
    assert_eq!(state.gpr[8], 0x1000 + 4);
}

/// The compressed shifts use the register field `x8`..`x15` and the
/// same shift semantics as their 32-bit equivalents.
#[test]
fn the_compressed_shifts_behave_like_their_32_bit_equivalents() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0x1234;
    step_c(&mut h, &mut state, 0x1000, C_SLLI_X8_4);
    assert_eq!(state.gpr[8], 0x1234 << 4);

    // The shift amount reaches the whole five-bit range.
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 1;
    step_c(&mut h, &mut state, 0x1000, C_SLLI_X8_31);
    assert_eq!(state.gpr[8], 1u32 << 31);

    // A logical shift right and an arithmetic one differ on a
    // negative value — that is the whole point of having both.
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0xFFFF_F000;
    step_c(&mut h, &mut state, 0x1000, C_SRLI_X8_12);
    assert_eq!(state.gpr[8], 0x000F_FFFF);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0xFFFF_F000;
    step_c(&mut h, &mut state, 0x1000, C_SRAI_X8_12);
    assert_eq!(state.gpr[8], 0xFFFF_FFFF);
}

/// `c.andi` sign-extends its 6-bit immediate.
#[test]
fn c_andi_sign_extends_its_immediate() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0xFFFF_FFFF;
    step_c(&mut h, &mut state, 0x1000, C_ANDI_X8_M17);
    assert_eq!(state.gpr[8], 0xFFFF_FFEF);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0xFFFF_FFFF;
    step_c(&mut h, &mut state, 0x1000, C_ANDI_X8_15);
    assert_eq!(state.gpr[8], 15);
}

/// The four register-register compressed operations must not be
/// confused with each other.
#[test]
fn the_four_compressed_register_operations_are_distinct() {
    let cases: [(u16, u32); 4] = [
        (C_SUB_X8_X9, 0x00EF_F100),
        (C_XOR_X8_X9, 0x00F0_0F00),
        (C_OR_X8_X9, 0x00F0_0FF0),
        (C_AND_X8_X9, 0x0000_00F0),
    ];
    for (enc, expected) in cases {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[8] = 0x00F0_00F0;
        state.gpr[9] = 0x0000_0FF0;
        step_c(&mut h, &mut state, 0x1000, enc);
        assert_eq!(
            state.gpr[8], expected,
            "0x{enc:04x} must produce {expected:#010x}"
        );
    }
}

/// `c.mv` copies a register; `c.add` adds to it.
#[test]
fn c_mv_copies_and_c_add_accumulates() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 1;
    state.gpr[10] = 0x123;
    step_c(&mut h, &mut state, 0x1000, C_MV_X5_X10);
    assert_eq!(state.gpr[5], 0x123, "c.mv copies the source register");

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 1;
    state.gpr[10] = 0x123;
    step_c(&mut h, &mut state, 0x1000, C_ADD_X5_X10);
    assert_eq!(state.gpr[5], 0x124, "c.add adds to the destination");
}

// ---------------------------------------------------------------------------
// Execution — the memory group
// ---------------------------------------------------------------------------

/// `c.lw` loads a word through the compressed offset field.
#[test]
fn c_lw_loads_through_the_compressed_offset() {
    let mut h = Harness::new();
    h.ram.borrow_mut().load(0x2004, &[0xDEAD_BEEF]);
    let mut state = SimState::default();
    state.gpr[9] = 0x2000;
    step_c(&mut h, &mut state, 0x1000, C_LW_X8_4_X9);
    assert_eq!(state.gpr[8], 0xDEAD_BEEF);
}

/// The compressed load offset reaches the whole 7-bit range.
#[test]
fn c_lw_reaches_the_largest_compressed_offset() {
    let mut h = Harness::new();
    h.ram.borrow_mut().load(0x2000 + 124, &[0x1234_5678]);
    let mut state = SimState::default();
    state.gpr[9] = 0x2000;
    step_c(&mut h, &mut state, 0x1000, C_LW_X8_124_X9);
    assert_eq!(state.gpr[8], 0x1234_5678);
}

/// `c.sw` stores a word through the compressed offset field.
#[test]
fn c_sw_stores_through_the_compressed_offset() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0xCAFE_BABE;
    state.gpr[9] = 0x2000;
    step_c(&mut h, &mut state, 0x1000, C_SW_X8_4_X9);
    let value = h
        .ram
        .borrow()
        .read(0x2004, riscv_sim_contract::model::WriteSize::Word)
        .unwrap();
    assert_eq!(value, 0xCAFE_BABE);
}

/// `c.lwsp` / `c.swsp` use the stack pointer as their base.
#[test]
fn the_stack_relative_compressed_accesses_use_sp() {
    let mut h = Harness::new();
    h.ram.borrow_mut().load(0x300C, &[0x0BAD_F00D]);
    let mut state = SimState::default();
    state.gpr[2] = 0x3000;
    step_c(&mut h, &mut state, 0x1000, C_LWSP_X8_12);
    assert_eq!(state.gpr[8], 0x0BAD_F00D);

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x3000;
    state.gpr[8] = 0xFEED_FACE;
    step_c(&mut h, &mut state, 0x1000, C_SWSP_X8_12);
    let value = h
        .ram
        .borrow()
        .read(0x300C, riscv_sim_contract::model::WriteSize::Word)
        .unwrap();
    assert_eq!(value, 0xFEED_FACE);
}

/// The stack-relative load reaches the whole 8-bit offset range.
#[test]
fn c_lwsp_reaches_the_largest_compressed_offset() {
    let mut h = Harness::new();
    h.ram.borrow_mut().load(0x3000 + 252, &[0x0BAD_F00D]);
    let mut state = SimState::default();
    state.gpr[2] = 0x3000;
    step_c(&mut h, &mut state, 0x1000, C_LWSP_X8_252);
    assert_eq!(state.gpr[8], 0x0BAD_F00D);
}

// ---------------------------------------------------------------------------
// Execution — the control-transfer group
// ---------------------------------------------------------------------------

/// `c.j` jumps by the compressed offset and writes no link register.
#[test]
fn c_j_jumps_by_the_compressed_offset() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xDEAD;
    step_c(&mut h, &mut state, 0x1000, C_J_P4);
    assert_eq!(state.pc, 0x1004, "c.j must jump forward by four bytes");
    assert_eq!(state.gpr[1], 0xDEAD, "c.j must not write a link register");
}

/// `c.j` reaches backwards too.
#[test]
fn c_j_reaches_backwards() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_J_M4);
    assert_eq!(state.pc, 0x0FFC, "c.j must jump backwards by four bytes");
}

/// `c.jal` writes the **compressed** return address: `pc + 2`, not
/// `pc + 4` (LLD §3.5, 评审 N1).
#[test]
fn c_jal_links_to_pc_plus_two() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    step_c(&mut h, &mut state, 0x1000, C_JAL_P4);
    assert_eq!(state.gpr[1], 0x1002, "the link register must hold pc + 2");
    assert_eq!(state.pc, 0x1004);
}

/// `c.jalr` links to `pc + 2` as well.
#[test]
fn c_jalr_links_to_pc_plus_two() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 0x2000;
    step_c(&mut h, &mut state, 0x1000, C_JALR_X5);
    assert_eq!(state.gpr[1], 0x1002, "the link register must hold pc + 2");
    assert_eq!(state.pc, 0x2000);
}

/// A 32-bit `jal` still links to `pc + 4` — the compressed adjustment
/// must not leak into the 32-bit path.
#[test]
fn a_32bit_jal_still_links_to_pc_plus_four() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0x1000, enc_j(8, 1, JAL));
    assert_eq!(state.gpr[1], 0x1004, "the 32-bit link register is pc + 4");
    assert_eq!(state.pc, 0x1008);
}

/// `c.jr` jumps without writing a link register.
#[test]
fn c_jr_jumps_without_writing_a_link_register() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[5] = 0x2000;
    state.gpr[1] = 0xDEAD;
    step_c(&mut h, &mut state, 0x1000, C_JR_X5);
    assert_eq!(state.pc, 0x2000);
    assert_eq!(state.gpr[1], 0xDEAD, "c.jr must not write a link register");
}

/// The CR group must keep its **one-bit** `funct2` for every register.
///
/// The CR format's `funct2` lives in `inst[12]`; the register field is
/// the full `inst[11:7]`.  A decoder that read the funct2 from
/// `inst[11]` (or from `inst[11:10]`) decodes `c.jr x5` **by
/// accident** — `inst[11]` happens to be zero for `x5` — and only
/// breaks once the register reaches `x16`, where `inst[11]` is set.
/// Every low-register case therefore hides the bug.
#[test]
fn c_jr_with_a_high_register_still_jumps() {
    // x17: `inst[11]` is set, `inst[11:10]` is `0b10`.
    // x31: `inst[11:10]` is `0b11` — the case a **two-bit** funct2
    // reading gets wrong.
    for (reg, enc) in [(17u8, C_JR_X17), (31u8, C_JR_X31)] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[reg as usize] = 0x2000;
        state.gpr[1] = 0xDEAD;
        step_c(&mut h, &mut state, 0x1000, enc);
        assert_eq!(state.pc, 0x2000, "c.jr x{reg} must jump to x{reg}");
        assert_eq!(
            state.gpr[1], 0xDEAD,
            "c.jr must not write a link register (x{reg})"
        );
    }
}

/// `c.mv` with a **high** destination register must copy, not add.
///
/// `c.mv` is `add rd, x0, rs2` and `c.add` is `add rd, rd, rs2`; the
/// two differ only in the CR format's `funct2` bit.  Reading that bit
/// from the wrong place turns `c.mv` into `c.add` for every register
/// whose `inst[11]` is set.
#[test]
fn c_mv_with_a_high_register_copies_the_source() {
    for (reg, enc) in [(17u8, C_MV_X17_X10), (31u8, C_MV_X31_X10)] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[10] = 0x1234;
        state.gpr[reg as usize] = 0x9999;
        step_c(&mut h, &mut state, 0x1000, enc);
        assert_eq!(
            state.gpr[reg as usize], 0x1234,
            "c.mv must copy, not accumulate"
        );
        assert_eq!(state.gpr[10], 0x1234, "c.mv must not touch the source");
    }
}

/// `c.beqz` / `c.bnez` must land on the right target for **every**
/// field of the 9-bit branch offset.
///
/// The suite only ever branched by `+4`, and decoding that encoding
/// touches a single field (`imm[2:1]`): the sign bit (`inst[12]`),
/// `imm[4:3]` (`inst[11:10]`), `imm[7:6]` (`inst[6:5]`) and `imm[5]`
/// (`inst[2]`) were never exercised.  A decoder that dropped the sign
/// bit, or shifted `imm[7:6]`, was therefore invisible to the unit
/// tests **and** to the golden corpus — upstream's `rvc.S` only uses
/// small forward displacements.
///
/// The offsets below cover **every field** of the offset, and between
/// them every *pattern* inside the two-bit fields as well:
///
/// ```text
/// offset   imm[8]  imm[7:6]  imm[5]  imm[4:3]  imm[2:1]
/// +2         0        00        0        00        01
/// +4         0        00        0        00        10
/// +6         0        00        0        00        11
/// +8         0        00        0        01        00
/// +26        0        00        0        11        01
/// +80        0        01        0        10        00
/// +130       0        10        0        00        01
/// -4         1        11        1        11        10
/// -256       1        00        0        00        00
/// ```
///
/// The table is not hand-transcribed: it is the decomposition of the
/// encodings as the ISS itself decodes them, so a row that disagreed
/// with the decoder would be a bug in the table, not in the decoder.
///
/// The judgement is on the **landing address**, not on "did it
/// branch": a decoder that computes the wrong target still branches.
#[test]
fn c_beqz_and_c_bnez_land_on_the_right_target() {
    let cases: [(u16, i32); 16] = [
        (C_BEQZ_X8_P2, 2),
        (C_BNEZ_X8_P2, 2),
        (C_BEQZ_X8_P6, 6),
        (C_BNEZ_X8_P6, 6),
        (C_BEQZ_X8_P8, 8),
        (C_BNEZ_X8_P8, 8),
        (C_BEQZ_X8_P26, 26),
        (C_BNEZ_X8_P26, 26),
        (C_BEQZ_X8_P80, 80),
        (C_BNEZ_X8_P80, 80),
        (C_BEQZ_X8_P130, 130),
        (C_BNEZ_X8_P130, 130),
        (C_BEQZ_X8_M4, -4),
        (C_BNEZ_X8_M4, -4),
        (C_BEQZ_X8_M256, -256),
        (C_BNEZ_X8_M256, -256),
    ];
    for (enc, offset) in cases {
        let pc = 0x1000u32;
        let target = (pc as i32 + offset) as u32;
        // `c.beqz` takes the branch when the register is zero,
        // `c.bnez` when it is not; the funct3 tells them apart.
        let is_beqz = (enc >> 13) & 0b111 == 0b110;
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[8] = if is_beqz { 0 } else { 1 };
        step_c(&mut h, &mut state, pc, enc);
        assert_eq!(
            state.pc, target,
            "0x{enc:04x} must land on pc {offset} = 0x{target:08x}"
        );
    }
}

/// `c.slli`'s destination register is the **full five bits**.
///
/// `c.slli` is one of the few compressed instructions whose `rd` is
/// not confined to `x8`..`x15`; the suite only ever shifted `x8`, so
/// a decoder that narrowed the field to three bits was invisible —
/// to the unit tests and to the golden corpus alike.
#[test]
fn c_slli_shifts_into_a_high_destination_register() {
    for (reg, enc, amount) in [(31u8, C_SLLI_X31_1, 1u32), (16u8, C_SLLI_X16_3, 3u32)] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        state.gpr[reg as usize] = 1;
        step_c(&mut h, &mut state, 0x1000, enc);
        assert_eq!(
            state.gpr[reg as usize],
            1u32 << amount,
            "c.slli must shift into x{reg}"
        );
    }
}

/// `c.swsp` must reach the **largest** offset from a **high** source
/// register.
///
/// The suite only stored through `c.swsp x8, 12(sp)`: that encoding
/// leaves `uimm[7:6]` (`inst[8:7]`) at zero and uses a three-bit
/// source register, so neither the wide offset nor the five-bit `rs2`
/// field was exercised — by the unit tests or by the golden corpus.
#[test]
fn c_swsp_reaches_the_largest_offset_from_a_high_register() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x2000; // sp
    state.gpr[31] = 0xABCD_EF01;
    step_c(&mut h, &mut state, 0x1000, C_SWSP_X31_252);
    let stored = h
        .ram
        .borrow()
        .read(0x2000 + 252, WriteSize::Word)
        .expect("sp + 252 is mapped");
    assert_eq!(stored, 0xABCD_EF01, "c.swsp must store x31 at sp + 252");
}

/// `c.beqz` branches when the register is zero and falls through
/// otherwise.
#[test]
fn c_beqz_branches_only_on_zero() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0;
    step_c(&mut h, &mut state, 0x1000, C_BEQZ_X8_P4);
    assert_eq!(state.pc, 0x1004, "the branch must be taken");

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 1;
    step_c(&mut h, &mut state, 0x1000, C_BEQZ_X8_P4);
    assert_eq!(state.pc, 0x1002, "the branch must not be taken");
}

/// `c.bnez` is the mirror image of `c.beqz`.
#[test]
fn c_bnez_branches_only_on_non_zero() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 1;
    step_c(&mut h, &mut state, 0x1000, C_BNEZ_X8_P4);
    assert_eq!(state.pc, 0x1004, "the branch must be taken");

    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 0;
    step_c(&mut h, &mut state, 0x1000, C_BNEZ_X8_P4);
    assert_eq!(state.pc, 0x1002, "the branch must not be taken");
}

/// `c.ebreak` raises the breakpoint trap.
///
/// ⚠️ The trap **hands the PC to the vector table** (STAGE-4 §3.5) —
/// the old assertion ("a trapped instruction must not advance") was
/// written when the trap path wrote no CSR and left the PC alone.
#[test]
fn c_ebreak_raises_the_breakpoint_trap() {
    let mut h = Harness::new();
    let mut state = SimState {
        csrs: vec![(CSR_MTVEC, 0x2000)],
        ..SimState::default()
    };
    let outcome = step_c(&mut h, &mut state, 0x1000, C_EBREAK);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Breakpoint
        }
    );
    assert_eq!(
        state.pc, 0x2000,
        "a trap must hand the PC to the vector table"
    );
    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x1000,
        "mepc must point at the faulting instruction"
    );
}

// ---------------------------------------------------------------------------
// Dispatch-route parity
// ---------------------------------------------------------------------------

/// A compressed instruction must behave the same on both dispatch
/// routes (D-11: one execution path).
#[test]
fn a_compressed_instruction_behaves_the_same_on_both_dispatch_routes() {
    // c.addi x8, -16 at address 0, with the upper half-word filled so
    // that the 32-bit route has something to mis-read.
    let mut bytes = vec![0u8; 8];
    bytes[0..2].copy_from_slice(&C_ADDI_X8_M16.to_le_bytes());
    let mut bh = BusHarness::new(&bytes);
    let mut bus_state = SimState::default();
    bus_state.gpr[8] = 100;
    bh.step(&mut bus_state).expect("bus route must not error");
    assert_eq!(bus_state.gpr[8], 84, "the bus route must execute it too");
    assert_eq!(bus_state.pc, 2, "the bus route must step by two bytes");
}

/// A compressed instruction must work at an address that is only
/// half-word aligned — the C extension lowers IALIGN to 16.
#[test]
fn a_compressed_instruction_runs_at_a_half_word_aligned_address() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[8] = 100;
    step_c(&mut h, &mut state, 0x1002, C_ADDI_X8_M16);
    assert_eq!(state.gpr[8], 84);
    assert_eq!(state.pc, 0x1004);
}

// ---------------------------------------------------------------------------
// The reserved and the HINT encodings
// ---------------------------------------------------------------------------

/// Every reserved and every HINT encoding must be classified the way
/// the reference simulator classifies it.
///
/// The table below was produced by running each encoding through the
/// reference simulator: `true` means the instruction retired, `false`
/// means it trapped.  The two implementations have to agree on **all**
/// of them — an encoding that traps on one side and retires on the
/// other is a divergence the golden comparison would eventually
/// report as a mismatch, and it is exactly the kind of corner the
/// corpus does not cover.
///
/// Note the two entries that *jump*: `c.jr` / `c.jalr` with a
/// register of zero jump to address zero, where the reference
/// simulator faults on the **next** fetch — the jump itself retires.
#[test]
fn the_reserved_and_hint_encodings_are_classified_like_the_reference() {
    let table: &[(u16, bool)] = &[
        (0x0001, true),  // c.nop
        (0x0015, true),  // c.addi rd=x0 (HINT)
        (0x0401, true),  // c.addi rd=8 imm=0 (HINT)
        (0x4015, true),  // c.li rd=x0 (HINT)
        (0x6401, false), // c.lui imm=0 (reserved)
        (0x6005, true),  // c.lui rd=x0 (HINT)
        (0x6101, false), // c.addi16sp nzimm=0 (reserved)
        (0x8001, true),  // c.srli shamt=0 (HINT)
        (0x9001, false), // c.srli shamt[5]=1 (reserved in RV32)
        (0x9401, false), // c.srai shamt[5]=1 (reserved in RV32)
        (0x987d, true),  // c.andi imm=-1
        (0x9c05, false), // the CA format with inst[12]=1 (reserved)
        (0x0002, true),  // c.slli rd=x0 (HINT)
        (0x1402, false), // c.slli shamt[5]=1 (reserved in RV32)
        (0x4002, false), // c.lwsp rd=x0 (reserved)
        (0x4402, true),  // c.lwsp rd=8
        (0xc002, true),  // c.swsp rs2=x0
        (0x8002, false), // c.jr rs1=x0 (reserved)
        (0x8402, true),  // c.jr rs1=x8 (jumps to x8)
        (0x8006, true),  // c.mv rd=x0 (no effect)
        (0x8406, true),  // c.mv rd=8
        (0x9002, false), // c.ebreak (traps)
        (0x9402, true),  // c.jalr rs1=x8 (jumps to x8)
        (0x9006, true),  // c.add rd=x0 (no effect)
        (0x9406, true),  // c.add rd=8
        (0x2000, false), // c.fld (D extension)
        (0x6000, false), // c.flw (F extension)
        (0xa000, false), // c.fsw (F extension)
        (0x2002, false), // c.fldsp (D extension)
        (0x6002, false), // c.flwsp (F extension)
        (0xa002, false), // c.fsdsp (D extension)
        (0xe000, false), // quadrant 0, funct3 = 111 (reserved)
        (0xe002, false), // quadrant 2, funct3 = 111 (reserved)
    ];
    for &(enc, retires) in table {
        // The `illegal` flag is part of the public `Decoded` contract:
        // a reserved encoding must be flagged, not merely fail to
        // decode.  It is the flag — not an incidental zero opcode —
        // that the executor refuses on, so the flag needs an observer.
        //
        // `illegal` and "traps" are **not** the same predicate:
        // `c.ebreak` is a perfectly legal instruction that raises a
        // breakpoint trap, so it must not be flagged illegal.
        let d = decode_c(enc);
        let expect_illegal = !retires && enc != C_EBREAK;
        assert_eq!(
            d.illegal, expect_illegal,
            "0x{enc:04x} must be flagged illegal exactly when it is reserved"
        );
        assert!(d.compressed, "0x{enc:04x} must stay flagged as compressed");
        assert_eq!(
            d.compressed_raw, enc,
            "0x{enc:04x} must keep its 16-bit code"
        );

        let mut h = Harness::new();
        // A non-zero vector table, so the trap's PC hand-off is
        // observable (STAGE-4 §3.5: a trap jumps to `mtvec`).
        let mut state = SimState {
            csrs: vec![(CSR_MTVEC, 0x2000)],
            ..SimState::default()
        };
        let outcome = h.step_half_at(&mut state, 0x1000, enc);
        if retires {
            assert!(
                matches!(outcome, StepOutcome::Advanced),
                "0x{enc:04x} must retire, got {outcome:?}"
            );
        } else {
            assert!(
                matches!(outcome, StepOutcome::Trapped { .. }),
                "0x{enc:04x} must trap, got {outcome:?}"
            );
            assert_eq!(
                state.pc, 0x2000,
                "0x{enc:04x}: a trap must hand the PC to the vector table"
            );
            assert_eq!(
                csr_value(&state, CSR_MEPC),
                0x1000,
                "0x{enc:04x}: mepc must point at the faulting instruction"
            );
        }
    }
}

/// The HINT encodings must have **no** architectural effect.
///
/// A HINT that wrote its destination register would not be a HINT, so
/// every register is pre-loaded and must come back untouched.
///
/// ⚠️ The `c.lui` with a **zero immediate** is *not* in this list: it
/// is a **reserved** encoding that traps, not a HINT.  The `c.lui`
/// HINT is the `rd = x0` one (`0x6005`), and writing x0 is not
/// observable — so the sharpest entries here are `0x0401`
/// (`c.addi x8, 0`) and `0x8001` (`c.srli x8, 0`), which write a
/// real register with a value that must stay put.
#[test]
fn a_hint_encoding_leaves_the_architectural_state_alone() {
    for enc in [0x0002u16, 0x0015, 0x0401, 0x4015, 0x6005, 0x8001] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        for r in 1..32 {
            state.gpr[r] = 0x1111_1111;
        }
        let outcome = h.step_half_at(&mut state, 0x1000, enc);
        assert!(
            matches!(outcome, StepOutcome::Advanced),
            "0x{enc:04x} must retire"
        );
        for r in 1..32 {
            assert_eq!(state.gpr[r], 0x1111_1111, "0x{enc:04x} must not write x{r}");
        }
        assert_eq!(state.pc, 0x1002);
    }
}
