//! RV32I coverage for M-13: decode tables, execution semantics and the
//! architectural edge cases (x0 hardwiring, sign extension, shift
//! amounts, branch / jump immediate splicing, misaligned accesses,
//! illegal encodings).
//!
//! The shared scaffolding lives here so every case drives the ISS the
//! same way the production path does:
//!
//! * [`Harness`] — byte-addressable RAM + [`Iss`], stepping through
//!   `Iss::step_one` (closure memory port).
//! * [`BusHarness`] — the same program driven through
//!   `Iss::step_one_via_bus` (M-14 bus memory port), used to prove the
//!   two dispatch routes share one semantic path (D-11).

use std::cell::RefCell;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};

use crate::m14_bus::Bus;

use super::decoder::Decoder;
use super::executor::{read_gpr, StepOutcome, TrapCause};
use super::Iss;

// ---------------------------------------------------------------------------
// Instruction encoders
// ---------------------------------------------------------------------------

/// Opcode of the OP-IMM group.
const OP_IMM: u8 = 0b001_0011;
/// Opcode of the AUIPC group.
const AUIPC: u8 = 0b001_0111;
/// Opcode of the OP (register-register) group.
const OP: u8 = 0b011_0011;
/// Opcode of the LUI group.
const LUI: u8 = 0b011_0111;

/// Encode an I-type instruction (12-bit signed immediate).
fn enc_i(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (((imm as u32) & 0xFFF) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode an R-type instruction (`funct7 | rs2 | rs1 | funct3 | rd | opcode`).
fn enc_r(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((funct7 as u32) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode an OP-IMM shift instruction
/// (`funct7 | shamt | rs1 | funct3 | rd | opcode`).
fn enc_shift(funct7: u8, shamt: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((funct7 as u32) << 25)
        | (((shamt as u32) & 0x1F) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode a U-type instruction (20-bit immediate in bits 31:12).
fn enc_u(imm20: u32, rd: u8, opcode: u8) -> u32 {
    ((imm20 & 0xF_FFFF) << 12) | ((rd as u32) << 7) | (opcode as u32)
}

// ---------------------------------------------------------------------------
// Test memory
// ---------------------------------------------------------------------------

/// Byte-addressable little-endian RAM backing the ISS memory port.
struct TestRam {
    base: u32,
    bytes: Vec<u8>,
}

impl TestRam {
    fn new(base: u32, size: usize) -> Self {
        Self {
            base,
            bytes: vec![0u8; size],
        }
    }

    fn offset(&self, addr: u32) -> Option<usize> {
        let rel = addr.checked_sub(self.base)? as usize;
        if rel < self.bytes.len() {
            Some(rel)
        } else {
            None
        }
    }

    /// Plant a little-endian word sequence at `addr`.
    fn load(&mut self, addr: u32, words: &[u32]) {
        for (i, word) in words.iter().enumerate() {
            let at = addr + (i as u32) * 4;
            let off = self.offset(at).expect("program image fits in RAM");
            self.bytes[off..off + 4].copy_from_slice(&word.to_le_bytes());
        }
    }

    fn read(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let off = self
            .offset(addr)
            .ok_or_else(|| PlatformError::new(ErrorFamily::Iss, 1, "unmapped read"))?;
        let width = width_bytes(size);
        if off + width > self.bytes.len() {
            return Err(PlatformError::new(ErrorFamily::Iss, 1, "read past end"));
        }
        let mut value = 0u32;
        for i in 0..width {
            value |= (self.bytes[off + i] as u32) << (8 * i);
        }
        Ok(value)
    }

    fn write(&mut self, addr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        let off = self
            .offset(addr)
            .ok_or_else(|| PlatformError::new(ErrorFamily::Iss, 1, "unmapped write"))?;
        let width = width_bytes(size);
        if off + width > self.bytes.len() {
            return Err(PlatformError::new(ErrorFamily::Iss, 1, "write past end"));
        }
        for i in 0..width {
            self.bytes[off + i] = (value >> (8 * i)) as u8;
        }
        Ok(())
    }
}

fn width_bytes(size: WriteSize) -> usize {
    match size {
        WriteSize::Byte => 1,
        WriteSize::Half => 2,
        WriteSize::Word => 4,
    }
}

/// ISS + RAM harness driving the closure memory port.
struct Harness {
    ram: RefCell<TestRam>,
    iss: Iss,
}

impl Harness {
    fn new() -> Self {
        Self {
            ram: RefCell::new(TestRam::new(0, 64 * 1024)),
            iss: Iss::default(),
        }
    }

    fn load(&mut self, addr: u32, words: &[u32]) {
        self.ram.borrow_mut().load(addr, words);
    }

    fn step(&mut self, state: &mut SimState) -> Result<StepOutcome, PlatformError> {
        let ram = &self.ram;
        self.iss.step_one(
            state,
            |addr, size| ram.borrow().read(addr, size),
            |addr, size, value| ram.borrow_mut().write(addr, size, value),
        )
    }

    /// Step a single instruction placed at `pc` and return the outcome.
    fn step_one_at(&mut self, state: &mut SimState, pc: u32, word: u32) -> StepOutcome {
        self.load(pc, &[word]);
        state.pc = pc;
        self.step(state).expect("step succeeds")
    }
}

/// ISS + M-14 bus harness driving the production dispatch path.
struct BusHarness {
    bus: Bus,
    iss: Iss,
}

impl BusHarness {
    fn new(program: &[u32]) -> Self {
        let mut bytes = Vec::new();
        for word in program {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        let mut bus = Bus::new();
        bus.map_ram(0, 0x1_0000, &bytes).expect("map ram");
        Self {
            bus,
            iss: Iss::default(),
        }
    }

    fn step(&mut self, state: &mut SimState) -> Result<StepOutcome, PlatformError> {
        self.iss.step_one_via_bus(state, &mut self.bus)
    }
}

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
    let mut h = BusHarness::new(&[word]);
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

    let mut bh = BusHarness::new(&[word]);
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

/// A compressed encoding is not executable yet: it must trap and leave
/// the PC alone rather than execute the upper half-word as if it were
/// a 32-bit instruction.
#[test]
fn compressed_encoding_traps_and_leaves_the_pc_alone() {
    let mut h = Harness::new();
    let mut state = SimState::default();
    let outcome = h.step_one_at(&mut state, 0x1000, 0x0000_0001);
    assert_eq!(
        outcome,
        StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction
        }
    );
    assert_eq!(
        state.pc, 0x1000,
        "a trapped instruction must not advance the PC"
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
