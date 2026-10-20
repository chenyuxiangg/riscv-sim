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

/// Encode an I-type instruction (12-bit signed immediate).
fn enc_i(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (((imm as u32) & 0xFFF) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
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
