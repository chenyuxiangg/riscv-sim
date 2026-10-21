//! RV32I coverage for M-13: decode tables, execution semantics and the
//! architectural edge cases (x0 hardwiring, sign extension, shift
//! amounts, branch / jump immediate splicing, misaligned accesses,
//! illegal encodings).
//!
//! The shared scaffolding lives in [`super::tests_common`] so that
//! every M-13 instruction-set module drives the ISS the same way the
//! production path does — see that module for the harness itself.

use std::cell::RefCell;
use std::rc::Rc;

use riscv_sim_contract::csr::{CSR_MEPC, CSR_MTVEC};
use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::{PendingWrite, SimState, WriteSize};
use riscv_sim_contract::port::Peripheral;

use crate::m14_bus::Bus;

use super::decoder::{Decoded, Decoder};
use super::executor::{
    classify, read_gpr, sequential, MisalignedPolicy, Op, StepOutcome, TrapCause,
};
use super::tests_common::*;
use super::Iss;

// ---------------------------------------------------------------------------
// D-04 — x0 hardwired to zero
// ---------------------------------------------------------------------------

/// `addi x0, x0, 42` must not be able to change x0.
#[test]
fn addi_to_x0_leaves_x0_zero() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let word = enc_i(42, 0, 0b000, 0, OP_IMM);
    h.step_one_at(&mut state, 0, word);
    assert_eq!(state.gpr[0], 0, "x0 must stay hardwired to zero");
}

/// A pre-existing x1 value must not be copied into x0 by `addi x0, x1, 1`.
#[test]
fn addi_reading_x1_cannot_write_x0() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0xDEAD_BEEF;
    let word = enc_i(1, 1, 0b000, 0, OP_IMM);
    h.step_one_at(&mut state, 0, word);
    assert_eq!(state.gpr[0], 0, "x0 must stay hardwired to zero");
    assert_eq!(state.gpr[1], 0xDEAD_BEEF, "x1 must be untouched");
}

/// The bus dispatch path must honour the same x0 rule (D-11: one path).
#[test]
fn bus_path_also_keeps_x0_zero() {
    let word = enc_i(42, 0, 0b000, 0, OP_IMM);
    let mut h = BusHarness::new(&image(&[word]));
    let mut state = SimState::default();
    h.step(&mut state).expect("step via bus");
    assert_eq!(state.gpr[0], 0, "x0 must stay hardwired to zero");
}

/// Even if the register file is corrupted with a non-zero x0, reads of
/// x0 must still observe the hardwired zero.
///
/// Note the assertion is on the *read* path: the ISS discards writes to
/// x0, it does not scrub a slot somebody else corrupted. Callers that
/// need the architectural value must go through [`read_gpr`].
#[test]
fn reading_x0_yields_zero_even_if_register_file_is_corrupted() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[0] = 0x1234_5678;
    let word = enc_i(1, 0, 0b000, 5, OP_IMM);
    h.step_one_at(&mut state, 0, word);
    assert_eq!(state.gpr[5], 1, "rs1 = x0 must read as zero");
    assert_eq!(read_gpr(&state, 0), 0, "x0 must read as zero");
}

// ---------------------------------------------------------------------------
// D-11 — one execution path for both dispatch routes
// ---------------------------------------------------------------------------

/// An encoding the ISS cannot execute must be reported as an illegal
/// instruction on *both* dispatch routes.
///
/// Before D-11 the closure route returned `Trapped` while the bus
/// route returned `Err` — the two routes disagreed about what an
/// illegal instruction even is.
#[test]
fn illegal_instruction_reports_the_same_outcome_on_both_routes() {
    // 0b010_1011 is not a valid RV32I opcode.
    let word = 0x0000_002Bu32;

    let mut h = Harness::new();
    let mut state = SimState::default();
    let closure_outcome = h.step_one_at(&mut state, 0, word);

    let mut bh = BusHarness::new(&image(&[word]));
    let mut bus_state = SimState::default();
    let bus_outcome = bh.step(&mut bus_state).expect("bus route must not error");

    assert_eq!(
        closure_outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        },
        "closure route must trap as an illegal instruction"
    );
    assert_eq!(
        bus_outcome, closure_outcome,
        "both dispatch routes must agree on the outcome"
    );
}

// ---------------------------------------------------------------------------
// D-16 — instruction length field
// ---------------------------------------------------------------------------

/// A 32-bit encoding must decode to a length of 4 bytes.
#[test]
fn decode_reports_four_bytes_for_a_32bit_encoding() {
    let d = Decoder::default().decode(enc_i(1, 0, 0b000, 1, OP_IMM), 0x1000);
    assert_eq!(d.len, 4, "a 32-bit encoding is 4 bytes long");
}

/// A 16-bit (compressed) encoding must decode to a length of 2 bytes.
///
/// Every 32-bit encoding has bits[1:0] = 0b11; a compressed encoding
/// never does, which is how the decoder tells them apart.
#[test]
fn decode_reports_two_bytes_for_a_compressed_encoding() {
    let d = Decoder::default().decode(0x0000_0001, 0x1000);
    assert_eq!(d.len, 2, "a compressed encoding is 2 bytes long");
}

/// The PC must advance by the decoded instruction length, not by a
/// hard-coded 4.
#[test]
fn pc_advances_by_the_decoded_instruction_length() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0x1000, enc_i(1, 0, 0b000, 1, OP_IMM));
    assert_eq!(
        state.pc, 0x1004,
        "PC must advance by the instruction length"
    );
}

/// A **reserved** compressed encoding must trap rather than execute the
/// upper half-word as if it were a 32-bit instruction.
///
/// The word below is `0x02A0_0000`: the low half-word `0x0000` is a
/// reserved `C.ADDI4SPN` (its `nzuimm` is zero), while the upper
/// half-word `0x02A0` *is* a valid 32-bit `addi`.  A decoder that
/// ignored the width would run the upper half-word instead of
/// trapping.
///
/// ⚠️ The trap now **redirects the PC** to the vector table (STAGE-4
/// §3.5) instead of leaving it alone — a trap hands the PC to
/// `mtvec`, it does not leave the faulting address in place.
#[test]
fn reserved_compressed_encoding_traps_and_hands_the_pc_to_the_vector_table() {
    let mut h = Harness::new();
    let mut state = SimState {
        csrs: vec![(CSR_MTVEC, 0x2000)],
        ..SimState::default()
    };
    let outcome = h.step_one_at(&mut state, 0x1000, 0x02A0_0000);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
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
// RV32I — U-type (LUI / AUIPC)
// ---------------------------------------------------------------------------

/// LUI must decode the 20-bit immediate into bits 31:12 with a zeroed
/// low half.
#[test]
fn decode_lui_fills_the_upper_immediate() {
    let d = Decoder::default().decode(enc_u(0x12345, 1, LUI), 0);
    assert_eq!(d.opcode, LUI);
    assert_eq!(d.rd, 1);
    assert_eq!(
        d.imm, 0x1234_5000,
        "U-type immediate keeps the low 12 bits zero"
    );
}

/// AUIPC decodes the same immediate shape as LUI.
#[test]
fn decode_auipc_fills_the_upper_immediate() {
    let d = Decoder::default().decode(enc_u(0x1, 1, AUIPC), 0);
    assert_eq!(d.opcode, AUIPC);
    assert_eq!(d.imm, 0x1000);
}

/// LUI writes the upper immediate into the destination register.
#[test]
fn lui_writes_the_upper_immediate() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0, enc_u(0x12345, 1, LUI));
    assert_eq!(state.gpr[1], 0x1234_5000);
    assert_eq!(state.pc, 4);
}

/// AUIPC adds the upper immediate to the PC of the instruction.
#[test]
fn auipc_adds_the_upper_immediate_to_the_pc() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0x1000, enc_u(0x1, 1, AUIPC));
    assert_eq!(
        state.gpr[1], 0x2000,
        "AUIPC must use the instruction's own PC"
    );
}

// ---------------------------------------------------------------------------
// RV32I — OP-IMM
// ---------------------------------------------------------------------------

/// The I-type immediate is sign-extended from 12 bits.
#[test]
fn decode_addi_sign_extends_the_immediate() {
    let d = Decoder::default().decode(enc_i(-1, 1, 0b000, 2, OP_IMM), 0);
    assert_eq!(d.imm, -1, "a 12-bit immediate must be sign-extended");
    assert_eq!(d.rs1, 1);
    assert_eq!(d.rd, 2);
}

/// The OP-IMM shifts carry a 5-bit shift amount, not a 12-bit immediate.
#[test]
fn decode_shift_immediates_report_the_shift_amount() {
    let slli = Decoder::default().decode(enc_shift(0, 4, 1, 0b001, 2, OP_IMM), 0);
    assert_eq!(slli.imm, 4);
    assert_eq!(slli.funct7, 0);

    let srli = Decoder::default().decode(enc_shift(0, 20, 1, 0b101, 2, OP_IMM), 0);
    assert_eq!(srli.imm, 20);
    assert_eq!(srli.funct7, 0);

    let srai = Decoder::default().decode(enc_shift(0b010_0000, 20, 1, 0b101, 2, OP_IMM), 0);
    assert_eq!(srai.imm, 20);
    assert_eq!(srai.funct7, 0b010_0000);
}

/// ADDI sign-extends its immediate.
#[test]
fn addi_sign_extends_negative_immediates() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 5;
    h.step_one_at(&mut state, 0, enc_i(-3, 1, 0b000, 2, OP_IMM));
    assert_eq!(state.gpr[2], 2, "5 + (-3) = 2");
}

/// ADDI wraps on overflow — RISC-V has no trap-on-overflow.
#[test]
fn addi_wraps_on_overflow() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    h.step_one_at(&mut state, 0, enc_i(1, 1, 0b000, 2, OP_IMM));
    assert_eq!(state.gpr[2], 0, "0xFFFF_FFFF + 1 wraps to 0");
}

/// SLTI compares as signed.
#[test]
fn slti_compares_signed() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX; // -1
    h.step_one_at(&mut state, 0, enc_i(1, 1, 0b010, 2, OP_IMM));
    assert_eq!(state.gpr[2], 1, "-1 < 1 is true");
}

/// SLTIU compares as unsigned — the classic signed/unsigned split.
#[test]
fn sltiu_compares_unsigned() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX; // -1 as signed, huge as unsigned
    h.step_one_at(&mut state, 0, enc_i(1, 1, 0b011, 2, OP_IMM));
    assert_eq!(state.gpr[2], 0, "0xFFFF_FFFF < 1 is false unsigned");
}

/// The bitwise OP-IMMs use the sign-extended immediate.
#[test]
fn bitwise_op_imms_use_the_sign_extended_immediate() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_i(-1, 1, 0b100, 2, OP_IMM));
    assert_eq!(
        state.gpr[2], 0xF0F0_F0F0,
        "xori with -1 inverts the register"
    );

    let mut state = SimState::default();
    state.gpr[1] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_i(-1, 1, 0b110, 2, OP_IMM));
    assert_eq!(state.gpr[2], 0xFFFF_FFFF, "ori with -1 sets every bit");

    let mut state = SimState::default();
    state.gpr[1] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_i(-1, 1, 0b111, 2, OP_IMM));
    assert_eq!(state.gpr[2], 0x0F0F_0F0F, "andi with -1 keeps every bit");
}

/// SLLI shifts left by the immediate shift amount.
#[test]
fn slli_shifts_left_by_the_immediate_amount() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x0000_0003;
    h.step_one_at(&mut state, 0, enc_shift(0, 4, 1, 0b001, 2, OP_IMM));
    assert_eq!(state.gpr[2], 0x30);
}

/// SRLI shifts right logically.
#[test]
fn srli_shifts_right_logically() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x8000_0000;
    h.step_one_at(&mut state, 0, enc_shift(0, 20, 1, 0b101, 2, OP_IMM));
    assert_eq!(
        state.gpr[2], 0x0000_0800,
        "a logical shift must not sign-extend"
    );
}

/// SRAI shifts right arithmetically, preserving the sign bit.
#[test]
fn srai_shifts_right_arithmetically() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x8000_0000;
    h.step_one_at(
        &mut state,
        0,
        enc_shift(0b010_0000, 20, 1, 0b101, 2, OP_IMM),
    );
    assert_eq!(
        state.gpr[2], 0xFFFF_F800,
        "an arithmetic shift must keep the sign"
    );
}

// ---------------------------------------------------------------------------
// RV32I — OP (register-register)
// ---------------------------------------------------------------------------

/// The R-type fields land in the right places.
#[test]
fn decode_r_type_fills_the_register_fields() {
    let d = Decoder::default().decode(enc_r(0b010_0000, 2, 1, 0b000, 3, OP), 0);
    assert_eq!(d.opcode, OP);
    assert_eq!(d.rd, 3);
    assert_eq!(d.rs1, 1);
    assert_eq!(d.rs2, 2);
    assert_eq!(d.funct3, 0b000);
    assert_eq!(d.funct7, 0b010_0000);
}

/// ADD and SUB wrap around.
#[test]
fn add_and_sub_wrap_around() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b000, 3, OP));
    assert_eq!(state.gpr[3], 0, "0xFFFF_FFFF + 1 wraps to 0");

    let mut state = SimState::default();
    state.gpr[1] = 0;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0, enc_r(0b010_0000, 2, 1, 0b000, 3, OP));
    assert_eq!(state.gpr[3], u32::MAX, "0 - 1 wraps to 0xFFFF_FFFF");
}

/// The register shift amount uses only its low 5 bits.
#[test]
fn sll_masks_the_shift_amount_to_five_bits() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 1;
    state.gpr[2] = 33; // 33 & 0x1f == 1
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b001, 3, OP));
    assert_eq!(state.gpr[3], 2, "a shift amount of 33 shifts by 1");
}

/// SLT compares across the signed / unsigned boundary.
#[test]
fn slt_compares_across_the_sign_boundary() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX; // -1 signed
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b010, 3, OP));
    assert_eq!(state.gpr[3], 1, "-1 < 1 is true signed");
}

/// SLTU compares the same pair as unsigned.
#[test]
fn sltu_compares_unsigned() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b011, 3, OP));
    assert_eq!(state.gpr[3], 0, "0xFFFF_FFFF < 1 is false unsigned");
}

/// SRL and SRA differ on a negative value.
#[test]
fn srl_and_sra_differ_on_negative_values() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = 0x8000_0000;
    state.gpr[2] = 20;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b101, 3, OP));
    assert_eq!(state.gpr[3], 0x0000_0800, "SRL is logical");

    let mut state = SimState::default();
    state.gpr[1] = 0x8000_0000;
    state.gpr[2] = 20;
    h.step_one_at(&mut state, 0, enc_r(0b010_0000, 2, 1, 0b101, 3, OP));
    assert_eq!(state.gpr[3], 0xFFFF_F800, "SRA is arithmetic");
}

/// XOR / OR / AND operate on the two source registers.
///
/// The operands are complementary masks, so the expected results are
/// obvious by inspection.
#[test]
fn xor_or_and_operate_on_two_registers() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = 0xF0F0_F0F0;
    state.gpr[2] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b100, 3, OP));
    assert_eq!(
        state.gpr[3], 0xFFFF_FFFF,
        "complementary masks xor to all ones"
    );

    let mut state = SimState::default();
    state.gpr[1] = 0xF0F0_F0F0;
    state.gpr[2] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b110, 3, OP));
    assert_eq!(
        state.gpr[3], 0xFFFF_FFFF,
        "complementary masks or to all ones"
    );

    let mut state = SimState::default();
    state.gpr[1] = 0xF0F0_F0F0;
    state.gpr[2] = 0x0F0F_0F0F;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b111, 3, OP));
    assert_eq!(state.gpr[3], 0x0000_0000, "complementary masks and to zero");
}

// ---------------------------------------------------------------------------
// RV32I — LOAD / STORE
// ---------------------------------------------------------------------------

/// The load fields land in the right places.
#[test]
fn decode_load_fills_the_fields() {
    let d = Decoder::default().decode(enc_i(8, 1, 0b010, 2, LOAD), 0);
    assert_eq!(d.opcode, LOAD);
    assert_eq!(d.funct3, 0b010);
    assert_eq!(d.rd, 2);
    assert_eq!(d.rs1, 1);
    assert_eq!(d.imm, 8);
}

/// The store immediate is spliced from the two imm fields
/// (imm[11:5] = inst[31:25], imm[4:0] = inst[11:7]).
#[test]
fn decode_store_splices_the_immediate() {
    let d = Decoder::default().decode(enc_s(-8, 2, 1, 0b010, STORE), 0);
    assert_eq!(d.opcode, STORE);
    assert_eq!(d.funct3, 0b010);
    assert_eq!(d.rs1, 1);
    assert_eq!(d.rs2, 2);
    assert_eq!(d.imm, -8);

    // The largest positive immediate exercises the imm[11:5] field.
    let big = Decoder::default().decode(enc_s(2047, 2, 1, 0b010, STORE), 0);
    assert_eq!(big.imm, 2047);

    // The most negative immediate exercises the sign bit of imm[11:5].
    let min = Decoder::default().decode(enc_s(-2048, 2, 1, 0b010, STORE), 0);
    assert_eq!(min.imm, -2048);
}

/// LW reads a word and writes it to the destination register.
#[test]
fn lw_reads_a_word_from_memory() {
    let mut h = Harness::new();
    h.load(0x100, &[0xDEAD_BEEF]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b010, 2, LOAD));
    assert_eq!(state.gpr[2], 0xDEAD_BEEF);
}

/// LB sign-extends the loaded byte.
#[test]
fn lb_sign_extends_the_byte() {
    let mut h = Harness::new();
    h.load(0x100, &[0xFFFF_FF80]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b000, 2, LOAD));
    assert_eq!(
        state.gpr[2], 0xFFFF_FF80,
        "0x80 must sign-extend to 0xFFFF_FF80"
    );
}

/// LBU zero-extends the loaded byte.
#[test]
fn lbu_zero_extends_the_byte() {
    let mut h = Harness::new();
    h.load(0x100, &[0xFFFF_FF80]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b100, 2, LOAD));
    assert_eq!(state.gpr[2], 0x0000_0080);
}

/// LH sign-extends the loaded halfword.
#[test]
fn lh_sign_extends_the_halfword() {
    let mut h = Harness::new();
    h.load(0x100, &[0xFFFF_8000]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b001, 2, LOAD));
    assert_eq!(state.gpr[2], 0xFFFF_8000);
}

/// LHU zero-extends the loaded halfword.
#[test]
fn lhu_zero_extends_the_halfword() {
    let mut h = Harness::new();
    h.load(0x100, &[0xFFFF_8000]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b101, 2, LOAD));
    assert_eq!(state.gpr[2], 0x0000_8000);
}

/// SW writes a whole word.
#[test]
fn sw_writes_a_word() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    state.gpr[2] = 0xCAFE_BABE;
    h.step_one_at(&mut state, 0, enc_s(0, 2, 1, 0b010, STORE));
    assert_eq!(
        h.ram.borrow().read(0x100, WriteSize::Word).unwrap(),
        0xCAFE_BABE
    );
}

/// SB writes exactly one byte and leaves its neighbours alone.
#[test]
fn sb_writes_a_single_byte() {
    let mut h = Harness::new();
    h.load(0x100, &[0x1122_3344]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    state.gpr[2] = 0xAABB_CCDD;
    h.step_one_at(&mut state, 0, enc_s(0, 2, 1, 0b000, STORE));
    let ram = h.ram.borrow();
    assert_eq!(ram.read(0x100, WriteSize::Byte).unwrap(), 0xDD);
    assert_eq!(ram.read(0x101, WriteSize::Byte).unwrap(), 0x33);
    assert_eq!(ram.read(0x100, WriteSize::Word).unwrap(), 0x1122_33DD);
}

/// SH writes exactly two bytes.
#[test]
fn sh_writes_a_halfword() {
    let mut h = Harness::new();
    h.load(0x100, &[0x1122_3344]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    state.gpr[2] = 0xAABB_CCDD;
    h.step_one_at(&mut state, 0, enc_s(0, 2, 1, 0b001, STORE));
    let ram = h.ram.borrow();
    assert_eq!(ram.read(0x100, WriteSize::Half).unwrap(), 0xCCDD);
    assert_eq!(ram.read(0x100, WriteSize::Word).unwrap(), 0x1122_CCDD);
}

/// A load into x0 is discarded.
#[test]
fn load_into_x0_is_discarded() {
    let mut h = Harness::new();
    h.load(0x100, &[0xDEAD_BEEF]);
    let mut state = SimState::default();
    state.gpr[1] = 0x100;
    h.step_one_at(&mut state, 0, enc_i(0, 1, 0b010, 0, LOAD));
    assert_eq!(state.gpr[0], 0, "a load into x0 must be discarded");
}

/// A misaligned word access traps under the strict `Trap` policy.
///
/// ⚠️ The cause is **`LoadAddressMisaligned`** (`mcause = 4`) — a
/// misaligned **load** is a different cause from a misaligned fetch
/// (`0`) and from a misaligned store (`6`) (ledger **D-63**).
#[test]
fn misaligned_word_access_traps() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = SimState::default();
    state.gpr[1] = 0x101; // not 4-byte aligned
    let outcome = h.step_one_at(&mut state, 0, enc_i(0, 1, 0b010, 2, LOAD));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::LoadAddressMisaligned
        }
    );
    assert_eq!(
        state.gpr[2], 0,
        "a trapped load must not write the register"
    );
}

/// A misaligned halfword access traps under the strict `Trap` policy.
#[test]
fn misaligned_halfword_access_traps() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    let mut state = SimState::default();
    state.gpr[1] = 0x101; // not 2-byte aligned
    let outcome = h.step_one_at(&mut state, 0, enc_i(0, 1, 0b001, 2, LOAD));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::LoadAddressMisaligned
        }
    );
}

/// A misaligned store traps under the strict `Trap` policy.
///
/// ⚠️ The cause is **`StoreAddressMisaligned`** (`mcause = 6`) — the
/// store's own code, not the load's (`4`).
#[test]
fn misaligned_store_traps() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Trap);
    h.load(0x100, &[0x0000_0000]);
    let mut state = SimState::default();
    state.gpr[1] = 0x102; // not 4-byte aligned
    state.gpr[2] = 0xFFFF_FFFF;
    let outcome = h.step_one_at(&mut state, 0, enc_s(0, 2, 1, 0b010, STORE));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::StoreAddressMisaligned
        }
    );
    assert_eq!(
        h.ram.borrow().read(0x100, WriteSize::Word).unwrap(),
        0,
        "a trapped store must not touch memory"
    );
}

/// The **default** policy performs the misaligned access.
///
/// LLD §3.6(2) / §5.2.1 require the Spike-aligned behaviour
/// ("默认不触发（模拟器与 Spike 一致允许非对齐）" / "不触发，保持 golden
/// 兼容"）。A default of `Trap` would diverge from Spike on any
/// program touching an unaligned address — exactly what the golden
/// comparison measures. So `Allow` is the default and `Trap` is the
/// opt-in strict mode.
#[test]
fn default_policy_performs_the_misaligned_access() {
    let mut h = Harness::new();
    assert_eq!(
        MisalignedPolicy::default(),
        MisalignedPolicy::Allow,
        "the default must be the LLD-mandated Spike-aligned policy"
    );
    h.load(0x100, &[0x1122_3344]);
    let mut state = SimState::default();
    state.gpr[1] = 0x101; // not 4-byte aligned
    let outcome = h.step_one_at(&mut state, 0, enc_i(0, 1, 0b010, 2, LOAD));
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(
        state.gpr[2], 0x0011_2233,
        "the unaligned word is read as-is, Spike-style"
    );
}

/// A byte access is never misaligned.
#[test]
fn byte_accesses_are_never_misaligned() {
    let mut h = Harness::new();
    h.load(0x100, &[0x0000_0000]);
    let mut state = SimState::default();
    state.gpr[1] = 0x101; // odd address, but a byte access is aligned
    state.gpr[2] = 0xAB;
    let outcome = h.step_one_at(&mut state, 0, enc_s(0, 2, 1, 0b000, STORE));
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(h.ram.borrow().read(0x101, WriteSize::Byte).unwrap(), 0xAB);
}

/// The Spike-aligned policy performs the access instead of trapping.
#[test]
fn allow_policy_performs_the_misaligned_access() {
    let mut h = Harness::new();
    h.iss = h.iss.with_misaligned_policy(MisalignedPolicy::Allow);
    h.load(0x100, &[0x1122_3344]);
    let mut state = SimState::default();
    state.gpr[1] = 0x101; // not 4-byte aligned
    let outcome = h.step_one_at(&mut state, 0, enc_i(0, 1, 0b010, 2, LOAD));
    assert!(matches!(outcome, StepOutcome::Advanced));
    assert_eq!(
        state.gpr[2], 0x0011_2233,
        "the unaligned word is read as-is"
    );
}

/// Loads and stores must go through the M-14 bus — the bus is the
/// only place that counts accesses and fires the M-18 probe hook
/// (D-11: a bypassing implementation would silently lose both).
#[test]
fn load_and_store_go_through_the_bus() {
    // lw x2, 0x100(x0) — the data lives at 0x100 in the same region.
    let program = enc_i(0x100, 0, 0b010, 2, LOAD);
    let mut bytes = image(&[program]);
    bytes.resize(0x104, 0);
    bytes[0x100..0x104].copy_from_slice(&0xCAFE_BABE_u32.to_le_bytes());

    let mut h = BusHarness::new(&bytes);
    let mut state = SimState::default();
    h.step(&mut state).expect("step via bus");

    assert_eq!(state.gpr[2], 0xCAFE_BABE, "the load must reach memory");
    let stats = h.bus.stats();
    assert_eq!(stats.reads, 2, "the bus must see the fetch and the load");
}

// ---------------------------------------------------------------------------
// RV32I — BRANCH / JAL / JALR
// ---------------------------------------------------------------------------

/// The test encoders must produce the spec-mandated encodings — a
/// drifting encoder would silently invalidate every other case in
/// this file.
#[test]
fn test_encoders_match_the_specified_encodings() {
    assert_eq!(
        enc_i(42, 0, 0b000, 1, OP_IMM),
        0x02A0_0093,
        "addi x1, x0, 42"
    );
    assert_eq!(enc_u(0x12345, 1, LUI), 0x1234_50B7, "lui x1, 0x12345");
    assert_eq!(enc_u(0x1, 1, AUIPC), 0x0000_1097, "auipc x1, 0x1");
    assert_eq!(enc_i(8, 1, 0b010, 2, LOAD), 0x0080_A103, "lw x2, 8(x1)");
    assert_eq!(enc_s(8, 2, 1, 0b010, STORE), 0x0020_A423, "sw x2, 8(x1)");
    assert_eq!(enc_b(8, 0, 0, 0b000, BRANCH), 0x0000_0463, "beq x0, x0, 8");
    assert_eq!(enc_j(8, 1, JAL), 0x0080_00EF, "jal x1, 8");
    assert_eq!(enc_i(-4, 2, 0b000, 1, JALR), 0xFFC1_00E7, "jalr x1, -4(x2)");
    assert_eq!(enc_r(0, 2, 1, 0b000, 3, OP), 0x0020_81B3, "add x3, x1, x2");
    assert_eq!(
        enc_r(0b010_0000, 2, 1, 0b000, 3, OP),
        0x4020_81B3,
        "sub x3, x1, x2"
    );
}

/// The branch immediate is spliced from the two imm fields and sign
/// extended.
#[test]
fn decode_branch_splices_the_immediate() {
    let d = Decoder::default().decode(enc_b(-8, 2, 1, 0b000, BRANCH), 0);
    assert_eq!(d.opcode, BRANCH);
    assert_eq!(d.funct3, 0b000);
    assert_eq!(d.rs1, 1);
    assert_eq!(d.rs2, 2);
    assert_eq!(d.imm, -8);

    // The most negative branch offset exercises the sign bit of
    // imm[12], which lives in inst[31].
    let min = Decoder::default().decode(enc_b(-4096, 0, 0, 0b000, BRANCH), 0);
    assert_eq!(min.imm, -4096);

    // The largest positive branch offset sets imm[12].
    let max = Decoder::default().decode(enc_b(4094, 0, 0, 0b000, BRANCH), 0);
    assert_eq!(max.imm, 4094);
}

/// The J-type immediate is spliced from four fields and sign extended.
#[test]
fn decode_jal_splices_the_immediate() {
    let d = Decoder::default().decode(enc_j(0x1000, 1, JAL), 0);
    assert_eq!(d.opcode, JAL);
    assert_eq!(d.rd, 1);
    assert_eq!(d.imm, 0x1000);

    let back = Decoder::default().decode(enc_j(-2048, 1, JAL), 0);
    assert_eq!(back.imm, -2048, "a negative J offset must sign extend");
}

/// JALR carries a plain I-type immediate.
#[test]
fn decode_jalr_fills_the_fields() {
    let d = Decoder::default().decode(enc_i(-4, 2, 0b000, 1, JALR), 0);
    assert_eq!(d.opcode, JALR);
    assert_eq!(d.funct3, 0b000);
    assert_eq!(d.rd, 1);
    assert_eq!(d.rs1, 2);
    assert_eq!(d.imm, -4);
}

/// A taken branch redirects the PC; a not-taken branch falls through.
#[test]
fn beq_taken_and_not_taken() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = 7;
    state.gpr[2] = 7;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b000, BRANCH));
    assert_eq!(state.pc, 0x1010, "a taken branch jumps to pc + offset");

    let mut state = SimState::default();
    state.gpr[1] = 7;
    state.gpr[2] = 8;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b000, BRANCH));
    assert_eq!(state.pc, 0x1004, "a not-taken branch falls through");
}

/// BNE is the inverse of BEQ.
#[test]
fn bne_taken_and_not_taken() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = 7;
    state.gpr[2] = 8;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b001, BRANCH));
    assert_eq!(state.pc, 0x1010);

    let mut state = SimState::default();
    state.gpr[1] = 7;
    state.gpr[2] = 7;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b001, BRANCH));
    assert_eq!(state.pc, 0x1004);
}

/// BLT compares signed, BLTU compares unsigned.
#[test]
fn blt_and_bltu_differ_across_the_sign_boundary() {
    let mut h = Harness::new();

    // -1 < 1 is true signed, false unsigned.
    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b100, BRANCH));
    assert_eq!(state.pc, 0x1010, "BLT is signed");

    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b110, BRANCH));
    assert_eq!(state.pc, 0x1004, "BLTU is unsigned");
}

/// BGE and BGEU are the inverses of BLT and BLTU.
#[test]
fn bge_and_bgeu_are_the_inverses() {
    let mut h = Harness::new();

    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b101, BRANCH));
    assert_eq!(state.pc, 0x1004, "BGE is signed: -1 is not >= 1");

    let mut state = SimState::default();
    state.gpr[1] = u32::MAX;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0x1000, enc_b(16, 2, 1, 0b111, BRANCH));
    assert_eq!(state.pc, 0x1010, "BGEU is unsigned: 0xFFFF_FFFF is >= 1");
}

/// A branch with a negative offset jumps backwards.
#[test]
fn branch_with_negative_offset_jumps_backwards() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 1;
    state.gpr[2] = 1;
    h.step_one_at(&mut state, 0x1000, enc_b(-16, 2, 1, 0b000, BRANCH));
    assert_eq!(state.pc, 0x0FF0);
}

/// JAL jumps to the target and links the return address.
#[test]
fn jal_links_and_jumps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0x1000, enc_j(0x1000, 1, JAL));
    assert_eq!(state.pc, 0x2000, "JAL jumps to pc + offset");
    assert_eq!(state.gpr[1], 0x1004, "JAL links the return address");
}

/// JALR jumps to the computed target and links the return address.
#[test]
fn jalr_links_and_jumps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x2000;
    h.step_one_at(&mut state, 0x1000, enc_i(-4, 2, 0b000, 1, JALR));
    assert_eq!(state.pc, 0x1FFC, "JALR jumps to rs1 + imm");
    assert_eq!(state.gpr[1], 0x1004, "JALR links the return address");
}

/// JALR clears the low bit of the computed target.
#[test]
fn jalr_clears_the_low_bit_of_the_target() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[2] = 0x2001; // odd target
    h.step_one_at(&mut state, 0x1000, enc_i(0, 2, 0b000, 1, JALR));
    assert_eq!(state.pc, 0x2000, "the low bit of a JALR target is cleared");
}

/// JALR must read rs1 before writing rd, so that `jalr x1, 0(x1)` links
/// the old value of x1.
#[test]
fn jalr_reads_rs1_before_writing_rd() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 0x3000;
    h.step_one_at(&mut state, 0x1000, enc_i(0, 1, 0b000, 1, JALR));
    assert_eq!(state.pc, 0x3000, "the target comes from the old x1");
    assert_eq!(state.gpr[1], 0x1004, "x1 holds the return address");
}

/// FENCE is a no-op in the sequentially consistent single-core model.
#[test]
fn fence_is_a_no_op() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    // fence iorw, iorw
    h.step_one_at(&mut state, 0x1000, enc_i(0x0FF, 0, 0b000, 0, 0b000_1111));
    assert_eq!(state.pc, 0x1004, "FENCE only advances the PC");
}

/// FENCE.I is a no-op as well.
#[test]
fn fence_i_is_a_no_op() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    h.step_one_at(&mut state, 0x1000, enc_i(0, 0, 0b001, 0, 0b000_1111));
    assert_eq!(state.pc, 0x1004);
}

/// ECALL raises an environment-call trap.
///
/// ⚠️ The trap **hands the PC to the vector table** (STAGE-4 §3.5) —
/// the old assertion ("a trapped instruction must not advance the PC")
/// was written when the trap path wrote no CSR and left the PC alone.
#[test]
fn ecall_traps() {
    let mut h = Harness::new();
    let mut state = SimState {
        csrs: vec![(CSR_MTVEC, 0x2000)],
        ..SimState::default()
    };
    let outcome = h.step_one_at(&mut state, 0x1000, enc_i(0, 0, 0b000, 0, 0b111_0011));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Ecall
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

/// EBREAK raises a breakpoint trap — distinct from a host breakpoint.
#[test]
fn ebreak_traps() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0x1000, enc_i(1, 0, 0b000, 0, 0b111_0011));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::Breakpoint
        }
    );
}

// ---------------------------------------------------------------------------
// RV32I — boundary cases
// ---------------------------------------------------------------------------

/// The B-type immediate must survive the splice for every offset class:
/// zero, both signs, the imm[4:1] field, the imm[10:5] field and the
/// imm[11] / imm[12] sign fields.
#[test]
fn branch_immediate_survives_every_field() {
    for offset in [0i32, 2, -2, 16, -16, 254, -254, 2046, -2048, 4094, -4096] {
        let d = Decoder::default().decode(enc_b(offset, 2, 1, 0b000, BRANCH), 0);
        assert_eq!(
            d.imm, offset,
            "branch offset {} did not survive the immediate splice",
            offset
        );
    }
}

/// The J-type immediate must survive the splice for every offset class.
#[test]
fn jal_immediate_survives_every_field() {
    // The J-type immediate is 21 bits wide and always even, so the
    // representable range is [-0x10_0000, 0x0F_FFFE].
    for offset in [
        0i32, 2, -2, 0x1000, -0x1000, 0x08_0000, -0x08_0000, 0x0F_FFFE, -0x10_0000,
    ] {
        let d = Decoder::default().decode(enc_j(offset, 1, JAL), 0);
        assert_eq!(
            d.imm, offset,
            "jal offset {} did not survive the immediate splice",
            offset
        );
    }
}

/// A register shift amount of 32 or more uses only its low 5 bits.
#[test]
fn register_shift_amounts_use_only_the_low_five_bits() {
    let mut h = Harness::new();

    // 32 & 0x1f == 0 → no shift at all.
    let mut state = SimState::default();
    state.gpr[1] = 0x1234_5678;
    state.gpr[2] = 32;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b001, 3, OP));
    assert_eq!(
        state.gpr[3], 0x1234_5678,
        "a shift amount of 32 shifts by 0"
    );

    // 63 & 0x1f == 31 → the largest shift.
    let mut state = SimState::default();
    state.gpr[1] = 1;
    state.gpr[2] = 63;
    h.step_one_at(&mut state, 0, enc_r(0, 2, 1, 0b001, 3, OP));
    assert_eq!(state.gpr[3], 1 << 31, "a shift amount of 63 shifts by 31");
}

/// An immediate shift amount of 31 is the largest one.
#[test]
fn immediate_shift_amount_of_thirty_one_shifts_by_thirty_one() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    state.gpr[1] = 1;
    h.step_one_at(&mut state, 0, enc_shift(0, 31, 1, 0b001, 2, OP_IMM));
    assert_eq!(state.gpr[2], 1 << 31);
}

/// An illegal funct3 must trap as an illegal instruction.
#[test]
fn illegal_funct3_traps() {
    let mut h = Harness::new();

    // LOAD with funct3 = 0b011 is not a valid encoding.
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0, enc_i(0, 0, 0b011, 2, LOAD));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );

    // BRANCH with funct3 = 0b010 is not a valid encoding.
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0, enc_b(8, 0, 0, 0b010, BRANCH));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
}

/// An illegal funct7 must trap as an illegal instruction.
#[test]
fn illegal_funct7_traps() {
    let mut h = Harness::new();

    // ADD with funct7 = 0b000_0010 is not a valid encoding.  (funct7
    // = 0b000_0001 *is* legal now: it is the M extension's MUL.)
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0, enc_r(0b000_0010, 2, 1, 0b000, 3, OP));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );

    // SLLI with funct7 = 0b000_0001 is not a valid shift encoding.
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0, enc_shift(0b000_0001, 4, 1, 0b001, 3, OP_IMM));
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
}

/// An illegal instruction must not touch the registers.
///
/// ⚠️ The **PC** is no longer "untouched": a trap hands the PC to the
/// vector table (STAGE-4 §3.5). The old assertion ("a trapped
/// instruction must not advance the PC") was written when the trap path
/// wrote no CSR at all; it is replaced by the vector-table hand-off,
/// which is the architectural behaviour now.
#[test]
fn illegal_instruction_leaves_the_state_untouched() {
    let mut h = Harness::new();
    let mut state = SimState {
        csrs: vec![(CSR_MTVEC, 0x2000)],
        ..SimState::default()
    };
    state.gpr[3] = 0x1234_5678;
    let outcome = h.step_one_at(&mut state, 0x1000, enc_r(0b000_0010, 2, 1, 0b000, 3, OP));
    assert!(matches!(outcome, StepOutcome::Trapped { .. }));
    assert_eq!(
        state.pc, 0x2000,
        "a trap must hand the PC to the vector table"
    );
    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x1000,
        "mepc must point at the faulting instruction"
    );
    assert_eq!(
        state.gpr[3], 0x1234_5678,
        "a trapped instruction must not write rd"
    );
}

/// A short program must run instruction after instruction, with the PC
/// advancing by one instruction length each time.
#[test]
fn a_program_runs_instruction_after_instruction() {
    let mut h = Harness::new();
    // addi x1, x0, 5 ; addi x2, x1, 3 ; sub x3, x2, x1
    h.load(
        0,
        &[
            enc_i(5, 0, 0b000, 1, OP_IMM),
            enc_i(3, 1, 0b000, 2, OP_IMM),
            enc_r(0b010_0000, 1, 2, 0b000, 3, OP),
        ],
    );

    let mut state = SimState::default();
    for _ in 0..3 {
        h.step(&mut state).expect("step");
    }
    assert_eq!(state.gpr[1], 5);
    assert_eq!(state.gpr[2], 8);
    assert_eq!(state.gpr[3], 3, "x3 = x2 - x1 = 8 - 5");
    assert_eq!(
        state.pc, 12,
        "three 4-byte instructions advance the PC by 12"
    );
}

/// The ISS is a pure function of its inputs: the same program and the
/// same starting state must produce the same result every time.
#[test]
fn execution_is_deterministic() {
    let program = [
        enc_i(5, 0, 0b000, 1, OP_IMM),
        enc_i(3, 1, 0b000, 2, OP_IMM),
        enc_r(0b010_0000, 1, 2, 0b000, 3, OP),
    ];

    let run = || {
        let mut h = Harness::new();
        h.load(0, &program);
        let mut state = SimState::default();
        for _ in 0..3 {
            h.step(&mut state).expect("step");
        }
        (state.gpr, state.pc)
    };

    let first = run();
    let second = run();
    assert_eq!(first.0, second.0, "the register file must be reproducible");
    assert_eq!(first.1, second.1, "the PC must be reproducible");
}

/// A misaligned instruction address must trap before the fetch
/// happens (LLD §3.1, 评审 13-3).
///
/// ⚠️ The cause is **`InstructionAddressMisaligned`** (`mcause = 0`) —
/// the fetch's own code, not the load's (`4`) or the store's (`6`)
/// (ledger **D-63**).
///
/// ⚠️ The trap **redirects the PC** to the vector table (STAGE-4
/// §3.5): a trapped instruction does not "leave the PC alone" any
/// more — it hands the PC to `mtvec`. The old assertion ("a trapped
/// fetch must not advance the PC") was written when the trap path
/// wrote no CSR at all.
#[test]
fn misaligned_fetch_address_traps() {
    let mut h = Harness::new();
    let mut state = SimState {
        pc: 0x1001, // odd instruction address
        ..SimState::default()
    };
    // A non-zero vector table, so the redirect is observable.
    state.csrs = vec![(CSR_MTVEC, 0x2000)];
    let outcome = h.step(&mut state).expect("must not error");
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::InstructionAddressMisaligned
        }
    );
    assert_eq!(
        state.pc, 0x2000,
        "a trap must hand the PC to the vector table"
    );
    assert_eq!(
        csr_value(&state, CSR_MEPC),
        0x1001,
        "mepc must point at the faulting instruction"
    );
}

// ---------------------------------------------------------------------------
// Review V1 defect 1 — ECALL / EBREAK need funct3 == 0
// ---------------------------------------------------------------------------

/// Every CSR-shaped encoding of the SYSTEM opcode must be dispatched as
/// a **CSR instruction** — never as ECALL or EBREAK.
///
/// The RISC-V unprivileged spec reserves `funct3 == 0b000` in the
/// SYSTEM opcode for ECALL / EBREAK; funct3 001..111 select the CSR
/// instruction family (CSRRW / CSRRS / CSRRC and their immediate
/// variants CSRRWI / CSRRSI / CSRRCI). Selecting on the 12-bit
/// immediate alone would route `0x00001073` (csr = 0, funct3 = 0b001)
/// into the ECALL handler and report mcause 11 instead of 2 — the
/// trap would reach the wrong handler as soon as CSR support lands.
///
/// ⚠️ This test was **rewritten** when the CSR instruction family
/// landed: the encodings are legal instructions now, so they no longer
/// trap. What it still pins is the **dispatch** — a CSR-shaped
/// encoding retires as a CSR instruction, and the decode level names
/// the same operation.
#[test]
fn csr_shaped_system_encodings_are_dispatched_as_csr_instructions() {
    for (funct3, op) in [
        (0b001, Op::Csrrw),
        (0b010, Op::Csrrs),
        (0b011, Op::Csrrc),
        (0b101, Op::Csrrwi),
        (0b110, Op::Csrrsi),
        (0b111, Op::Csrrci),
    ] {
        for csr in [0u32, 1, 5, 0xC00] {
            let mut h = Harness::new();
            let mut state = SimState::default();
            // The CSR number lives in bits 31:20 — the same field the
            // ECALL / EBREAK immediate occupies.
            let word = enc_i(csr as i32, 0, funct3, 0, SYSTEM);
            let outcome = h.step_one_at(&mut state, 0x1000, word);
            assert_eq!(
                outcome,
                StepOutcome::Advanced,
                "csr={csr:#x} funct3={funct3:#b} must retire as a CSR instruction, not trap"
            );
            let d = Decoder::default().decode(word, 0x1000);
            assert_eq!(
                classify(&d),
                Some(op),
                "csr={csr:#x} funct3={funct3:#b} must decode to {op:?}"
            );
        }
    }
}

/// A CSR-shaped encoding whose immediate field happens to be 1 must
/// not be reported as EBREAK.
///
/// ⚠️ Rewritten when the CSR instruction family landed: the encoding
/// is a `CSRRW` now, so the test pins the two things that are
/// observable — it **retires** (it does not trap), and it performs the
/// `CSRRW` write (the CSR file ends up holding the written value).
#[test]
fn a_csr_shaped_system_encoding_is_never_ebreak() {
    // csr = 1, funct3 = 0b001 (CSRRW) — the immediate field is 1.
    let mut h = Harness::new();
    let mut state = SimState::default();
    let word = enc_i(1, 0, 0b001, 0, SYSTEM);
    let outcome = h.step_one_at(&mut state, 0x1000, word);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "csr=1 funct3=0b001 must retire as a CSRRW, not trap as EBREAK"
    );
    assert_eq!(
        csr_value(&state, 1),
        0,
        "CSRRW with rs1 = x0 must write 0 into the CSR"
    );
}

/// SYSTEM with funct3 == 0 but an immediate other than 0 / 1 / 0x302 is
/// neither ECALL nor EBREAK — WFI (`imm = 0x105`) and SRET
/// (`imm = 0x102`) fall into this bucket and stay **illegal**.
///
/// ⚠️ `MRET` (`imm = 0x302`) left this list when the CSR / trap
/// family landed: it is a legal instruction now (pinned by the
/// `tests_csr` module). SRET and WFI stay illegal — the platform is
/// **M-mode only** (STAGE-4 §3.6).
#[test]
fn system_with_an_unexpected_immediate_traps_as_illegal_instruction() {
    for imm in [2i32, 0x102, 0x105] {
        let mut h = Harness::new();
        let mut state = SimState::default();
        let word = enc_i(imm, 0, 0b000, 0, SYSTEM);
        let outcome = h.step_one_at(&mut state, 0x1000, word);
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction
            },
            "SYSTEM imm={imm:#x} must trap as an illegal instruction"
        );
    }
}

// ---------------------------------------------------------------------------
// Review V1 defect 2 — the PC steps by the decoded instruction length
// ---------------------------------------------------------------------------

/// A minimal [`Decoded`] for the tests that only care about the PC
/// step.
fn decoded_at(pc: u32, len: u8) -> Decoded {
    Decoded {
        opcode: OP_IMM,
        funct3: 0b000,
        funct7: 0,
        rd: 0,
        rs1: 0,
        rs2: 0,
        imm: 0,
        raw: 0,
        pc,
        len,
        compressed: len == 2,
        compressed_raw: 0,
        illegal: false,
    }
}

/// The fall-through PC advances by the **decoded length**, not by a
/// hard-coded 4 — a compressed (RV32C) instruction advances by 2.
///
/// A `+ 4` here would silently break every compressed instruction
/// once the C extension lands (D-16).
#[test]
fn sequential_advances_by_the_decoded_instruction_length() {
    assert_eq!(sequential(&decoded_at(0x1000, 2)), 0x1002);
    assert_eq!(sequential(&decoded_at(0x1000, 4)), 0x1004);
}

/// The decoder's length drives the PC step for a real encoding too.
#[test]
fn the_pc_step_follows_the_length_the_decoder_reported() {
    let word = enc_i(42, 0, 0b000, 1, OP_IMM);
    let decoded = Decoder::default().decode(word, 0x1000);
    assert_eq!(decoded.len, 4, "a 32-bit encoding decodes to 4 bytes");
    assert_eq!(sequential(&decoded), 0x1004);
}

// ---------------------------------------------------------------------------
// Review V1 defect 3 — the effective address wraps instead of panicking
// ---------------------------------------------------------------------------

/// A load whose effective address wraps around the 32-bit space must
/// access the wrapped address, not panic.
///
/// `x1 = 0xFFFF_FFFF` plus an offset of 1 wraps to address 0. The
/// address computation uses `wrapping_add`, so the access lands at 0
/// — and a debug build must not abort on the overflow.
#[test]
fn a_load_whose_address_wraps_around_does_not_panic() {
    let mut h = Harness::new();
    // Plant 0x80 at address 0 → LB sign-extends it to 0xFFFF_FF80.
    h.load(0, &[0x0000_0080]);
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    let word = enc_i(1, 1, 0b000, 2, LOAD);
    let outcome = h.step_one_at(&mut state, 0x1000, word);
    assert_eq!(
        outcome,
        StepOutcome::Advanced,
        "the wrapped load must execute, not trap"
    );
    assert_eq!(
        state.gpr[2], 0xFFFF_FF80,
        "the load must read the wrapped address 0"
    );
}

/// The same for a store: the wrapped address is written, and a
/// follow-up load observes it.
#[test]
fn a_store_whose_address_wraps_around_does_not_panic() {
    let mut h = Harness::new();
    h.load(0, &[0]);
    let mut state = SimState::default();
    state.gpr[1] = 0xFFFF_FFFF;
    state.gpr[2] = 0x0000_00AB;
    h.step_one_at(&mut state, 0x1000, enc_s(1, 2, 1, 0b000, STORE));
    // Read it back with `lbu x3, 0(x0)`.
    h.load(0x1004, &[enc_i(0, 0, 0b100, 3, LOAD)]);
    state.pc = 0x1004;
    h.step(&mut state).expect("step");
    assert_eq!(
        state.gpr[3], 0x0000_00AB,
        "the store must have landed on the wrapped address 0"
    );
}

// ---------------------------------------------------------------------------
// Review V1 defect 4 — a sub-word store delivers the truncated value
// ---------------------------------------------------------------------------

/// A peripheral that records the raw value the bus hands it, so the
/// sub-word store semantics can be observed end to end.
#[derive(Default)]
struct RecordingPeripheral {
    /// The last write the bus delivered: `(offset, width, value)`.
    last: Option<(u32, WriteSize, u32)>,
}

impl Peripheral for RecordingPeripheral {
    fn id(&self) -> &str {
        "REC0"
    }

    fn base_addr(&self) -> u32 {
        0x2000
    }

    fn size(&self) -> u32 {
        0x10
    }

    fn read(&self, _offset: u32, _size: WriteSize) -> Result<u32, PlatformError> {
        Ok(0)
    }

    fn write(&mut self, offset: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        self.last = Some((offset, size, value));
        Ok(())
    }

    fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
}

/// A sub-word store must deliver the **truncated** value to a
/// peripheral.
///
/// The M-14 bus hands a peripheral the value unchanged together with
/// the access width, and the M-15 models do not mask it themselves
/// (`GPIO`'s pin mask is `u32::MAX`). The ISS therefore masks the
/// source register down to the access width before the value reaches
/// the bus — otherwise `sb` of `0x1234` would deliver `0x1234` to the
/// target while the very same store to RAM would write `0x34`.
#[test]
fn a_sub_word_store_to_a_peripheral_delivers_the_truncated_value() {
    let periph = Rc::new(RefCell::new(RecordingPeripheral::default()));
    let handle: Rc<RefCell<dyn Peripheral>> = periph.clone();

    let mut bus = Bus::new();
    bus.map_ram(0, 0x1000, &image(&[enc_s(0, 2, 1, 0b000, STORE)]))
        .expect("map ram");
    bus.map_peripheral_instance(0x2000, 0x10, handle)
        .expect("map peripheral");

    let mut iss = Iss::default();
    let mut state = SimState::default();
    state.gpr[1] = 0x2000; // the peripheral window
    state.gpr[2] = 0x1234; // the value to store
    iss.step_one_via_bus(&mut state, &mut bus)
        .expect("step via bus");

    let last = periph.borrow().last.expect("the peripheral saw a write");
    assert_eq!(last.0, 0, "the write landed on the window's first byte");
    assert_eq!(last.1, WriteSize::Byte, "an `sb` is a byte-wide access");
    assert_eq!(
        last.2, 0x0000_0034,
        "an `sb` must deliver only the low 8 bits of the source register"
    );
}

/// The mirror image: a word store delivers the full 32-bit value.
#[test]
fn a_word_store_to_a_peripheral_delivers_the_full_value() {
    let periph = Rc::new(RefCell::new(RecordingPeripheral::default()));
    let handle: Rc<RefCell<dyn Peripheral>> = periph.clone();

    let mut bus = Bus::new();
    bus.map_ram(0, 0x1000, &image(&[enc_s(0, 2, 1, 0b010, STORE)]))
        .expect("map ram");
    bus.map_peripheral_instance(0x2000, 0x10, handle)
        .expect("map peripheral");

    let mut iss = Iss::default();
    let mut state = SimState::default();
    state.gpr[1] = 0x2000;
    state.gpr[2] = 0x1234_5678;
    iss.step_one_via_bus(&mut state, &mut bus)
        .expect("step via bus");

    let last = periph.borrow().last.expect("the peripheral saw a write");
    assert_eq!(last.1, WriteSize::Word, "an `sw` is a word-wide access");
    assert_eq!(
        last.2, 0x1234_5678,
        "an `sw` must deliver all 32 bits of the source register"
    );
}

/// A half-word store delivers the low 16 bits.
#[test]
fn a_half_word_store_to_a_peripheral_delivers_the_truncated_value() {
    let periph = Rc::new(RefCell::new(RecordingPeripheral::default()));
    let handle: Rc<RefCell<dyn Peripheral>> = periph.clone();

    let mut bus = Bus::new();
    bus.map_ram(0, 0x1000, &image(&[enc_s(0, 2, 1, 0b001, STORE)]))
        .expect("map ram");
    bus.map_peripheral_instance(0x2000, 0x10, handle)
        .expect("map peripheral");

    let mut iss = Iss::default();
    let mut state = SimState::default();
    state.gpr[1] = 0x2000;
    state.gpr[2] = 0x1234_5678;
    iss.step_one_via_bus(&mut state, &mut bus)
        .expect("step via bus");

    let last = periph.borrow().last.expect("the peripheral saw a write");
    assert_eq!(last.1, WriteSize::Half, "an `sh` is a half-word access");
    assert_eq!(
        last.2, 0x0000_5678,
        "an `sh` must deliver only the low 16 bits of the source register"
    );
}
