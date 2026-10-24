//! Shared test scaffolding for the M-13 instruction-set tests.
//!
//! The harness lives here so that every module drives the ISS the same
//! way the production path does:
//!
//! * [`Harness`] — byte-addressable RAM + [`Iss`], stepping through
//!   `Iss::step_one` (closure memory port).
//! * [`BusHarness`] — the same program driven through
//!   `Iss::step_one_via_bus` (M-14 bus memory port), used to prove the
//!   two dispatch routes share one semantic path (D-11).
//!
//! It also carries the instruction encoders, so that a test states its
//! instruction as an encoding rather than as a hand-rolled bit pattern.

use std::cell::RefCell;

use riscv_sim_contract::csr::counter_pair;
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};

use crate::m14_bus::Bus;

use super::executor::StepOutcome;
use super::Iss;

// ---------------------------------------------------------------------------
// CSR encodings
// ---------------------------------------------------------------------------
//
// ⚠️ **These live here, not in one test file** (STAGE-21): the PMP tests and
// the counter tests both need them ⇒ ⇒ one copy ⇒ ⇒ the two files cannot
// drift apart.

/// `CSRRW`（`funct3 == 0b001`）。
pub(super) const F_CSRRW: u8 = 0b001;
/// `CSRRS`（`funct3 == 0b010`）—— **纯读**用的就是它（`rs1 == x0`）。
pub(super) const F_CSRRS: u8 = 0b010;
/// `CSRRC`（`funct3 == 0b011`）。
pub(super) const F_CSRRC: u8 = 0b011;
/// `CSRRWI`（`funct3 == 0b101`）。
pub(super) const F_CSRRWI: u8 = 0b101;
/// `CSRRSI`（`funct3 == 0b110`）。
pub(super) const F_CSRRSI: u8 = 0b110;
/// `CSRRCI`（`funct3 == 0b111`）。
pub(super) const F_CSRRCI: u8 = 0b111;

/// Encode a CSR instruction.
///
/// The CSR number lives in bits 31:20 — the same field the ECALL / EBREAK
/// immediate occupies — and the `rs1` field (bits 19:15) is the source
/// operand (a register for the register forms, a 5-bit immediate for the
/// immediate forms).
pub(super) fn csr_insn(csr: u16, rs1: u8, funct3: u8, rd: u8) -> u32 {
    enc_i(csr as i32, rs1, funct3, rd, SYSTEM)
}

/// `csrr <rd>, <csr>` —— a **pure read**.
///
/// ⚠️ It is `csrrs rd, csr, x0`, **not** `csrrw`: `csrrw` with `rs1 == x0`
/// **writes** the CSR (writing zero into it!) ⇒ ⇒ 拿它读计数器会把计数器
/// **清零** ⇒ ⇒ 而 `csrrs` with `rs1 == x0` 是规范定义的**纯读**
/// （不写 ⇒ ⇒ 也不触发「写计数器抑制 +1」那条规则）。
pub(super) fn csrr(csr: u16, rd: u8) -> u32 {
    csr_insn(csr, 0, F_CSRRS, rd)
}

/// `csrw <csr>, <rs1>` —— a **write** with no read-back.
pub(super) fn csrw(csr: u16, rs1: u8) -> u32 {
    csr_insn(csr, rs1, F_CSRRW, 0)
}

/// `csrrs <rd>, <csr>, <rs1>` —— set bits from a register.
pub(super) fn csrrs(csr: u16, rs1: u8, rd: u8) -> u32 {
    csr_insn(csr, rs1, F_CSRRS, rd)
}

/// `csrrc <rd>, <csr>, <rs1>` —— clear bits with a register.
pub(super) fn csrrc(csr: u16, rs1: u8, rd: u8) -> u32 {
    csr_insn(csr, rs1, F_CSRRC, rd)
}

/// `csrrsi <rd>, <csr>, <imm>` —— set bits with a 5-bit immediate.
pub(super) fn csrrsi(csr: u16, imm: u8, rd: u8) -> u32 {
    csr_insn(csr, imm, F_CSRRSI, rd)
}

/// `csrwi <csr>, <imm>` —— a write with a 5-bit immediate.
pub(super) fn csrwi(csr: u16, imm: u8) -> u32 {
    csr_insn(csr, imm, F_CSRRWI, 0)
}

/// `csrci <csr>, <imm>` — the form upstream `rv32mi/instret_overflow` uses
/// on `mcountinhibit`.
pub(super) fn csrci(csr: u16, imm: u8) -> u32 {
    csr_insn(csr, imm, F_CSRRCI, 0)
}

/// The address the test programs live at.
pub(super) const BASE: u32 = 0x1000;

/// `addi x0, x0, 0` —— the canonical `nop`.
pub(super) fn nop() -> u32 {
    enc_i(0, 0, 0b000, 0, OP_IMM)
}

/// `ecall` — the environment call.
pub(super) fn ecall() -> u32 {
    enc_i(0, 0, 0b000, 0, SYSTEM)
}

// ---------------------------------------------------------------------------
// Instruction encoders
// ---------------------------------------------------------------------------

/// Opcode of the OP-IMM group.
pub(super) const OP_IMM: u8 = 0b001_0011;
/// Opcode of the AUIPC group.
pub(super) const AUIPC: u8 = 0b001_0111;
/// Opcode of the OP (register-register) group.
pub(super) const OP: u8 = 0b011_0011;
/// Opcode of the LUI group.
pub(super) const LUI: u8 = 0b011_0111;
/// Opcode of the LOAD group.
pub(super) const LOAD: u8 = 0b000_0011;
/// Opcode of the STORE group.
pub(super) const STORE: u8 = 0b010_0011;
/// Opcode of the BRANCH group.
pub(super) const BRANCH: u8 = 0b110_0011;
/// Opcode of the JALR instruction.
pub(super) const JALR: u8 = 0b110_0111;
/// Opcode of the JAL instruction.
pub(super) const JAL: u8 = 0b110_1111;
/// The SYSTEM group (ECALL / EBREAK / CSR instructions).
pub(super) const SYSTEM: u8 = 0b111_0011;

/// Encode an I-type instruction (12-bit signed immediate).
pub(super) fn enc_i(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (((imm as u32) & 0xFFF) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode an R-type instruction (`funct7 | rs2 | rs1 | funct3 | rd | opcode`).
pub(super) fn enc_r(funct7: u8, rs2: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((funct7 as u32) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode an OP-IMM shift instruction
/// (`funct7 | shamt | rs1 | funct3 | rd | opcode`).
pub(super) fn enc_shift(funct7: u8, shamt: u8, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    ((funct7 as u32) << 25)
        | (((shamt as u32) & 0x1F) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// Encode a U-type instruction (20-bit immediate in bits 31:12).
pub(super) fn enc_u(imm20: u32, rd: u8, opcode: u8) -> u32 {
    ((imm20 & 0xF_FFFF) << 12) | ((rd as u32) << 7) | (opcode as u32)
}

/// Encode an S-type instruction (12-bit signed immediate, split fields).
pub(super) fn enc_s(imm: i32, rs2: u8, rs1: u8, funct3: u8, opcode: u8) -> u32 {
    let imm = imm as u32;
    ((imm & 0x1F) << 7)
        | (((imm >> 5) & 0x7F) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | (opcode as u32)
}

/// Little-endian byte image of a word sequence.
pub(super) fn image(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Encode a B-type instruction (13-bit signed immediate, split fields).
pub(super) fn enc_b(imm: i32, rs2: u8, rs1: u8, funct3: u8, opcode: u8) -> u32 {
    let imm = imm as u32;
    (((imm >> 12) & 0x1) << 31)
        | (((imm >> 5) & 0x3F) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | (((imm >> 1) & 0xF) << 8)
        | (((imm >> 11) & 0x1) << 7)
        | (opcode as u32)
}

/// Encode a J-type instruction (21-bit signed immediate, split fields).
pub(super) fn enc_j(imm: i32, rd: u8, opcode: u8) -> u32 {
    let imm = imm as u32;
    (((imm >> 20) & 0x1) << 31)
        | (((imm >> 1) & 0x3FF) << 21)
        | (((imm >> 11) & 0x1) << 20)
        | (((imm >> 12) & 0xFF) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

// ---------------------------------------------------------------------------
// Test memory
// ---------------------------------------------------------------------------

/// Byte-addressable little-endian RAM backing the ISS memory port.
pub(super) struct TestRam {
    pub(super) base: u32,
    pub(super) bytes: Vec<u8>,
}

impl TestRam {
    pub(super) fn new(base: u32, size: usize) -> Self {
        Self {
            base,
            bytes: vec![0u8; size],
        }
    }

    pub(super) fn offset(&self, addr: u32) -> Option<usize> {
        let rel = addr.checked_sub(self.base)? as usize;
        if rel < self.bytes.len() {
            Some(rel)
        } else {
            None
        }
    }

    /// Plant a little-endian word sequence at `addr`.
    pub(super) fn load(&mut self, addr: u32, words: &[u32]) {
        for (i, word) in words.iter().enumerate() {
            let at = addr + (i as u32) * 4;
            let off = self.offset(at).expect("program image fits in RAM");
            self.bytes[off..off + 4].copy_from_slice(&word.to_le_bytes());
        }
    }

    /// Plant a 16-bit (compressed) instruction at `addr`.
    ///
    /// Only the two bytes of the encoding are written: the fetch reads
    /// a whole word, but the decoder looks at the low half-word only
    /// when the encoding says the instruction is compressed.
    pub(super) fn load_half(&mut self, addr: u32, half: u16) {
        let off = self.offset(addr).expect("program image fits in RAM");
        self.bytes[off..off + 2].copy_from_slice(&half.to_le_bytes());
    }

    pub(super) fn read(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
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

    pub(super) fn write(
        &mut self,
        addr: u32,
        size: WriteSize,
        value: u32,
    ) -> Result<(), PlatformError> {
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

pub(super) fn width_bytes(size: WriteSize) -> usize {
    match size {
        WriteSize::Byte => 1,
        WriteSize::Half => 2,
        WriteSize::Word => 4,
    }
}

/// Read a CSR **out of the state's CSR file** — the test-side view.
///
/// ⚠️ The helper reads the file **directly** instead of going through
/// the implementation's accessor: an assertion that used the
/// implementation's own reader to check the implementation's own
/// writer would be **self-referential** — it would stay green even if
/// both sides were wrong in the same way.
pub(super) fn csr_value(state: &SimState, csr: u16) -> u32 {
    state
        .csrs
        .iter()
        .find(|(number, _)| *number == csr)
        .map(|(_, value)| *value)
        .unwrap_or(0)
}

/// The CSR numbers present in the state's CSR file, in file order.
pub(super) fn csr_numbers(state: &SimState) -> Vec<u16> {
    state.csrs.iter().map(|(number, _)| *number).collect()
}

/// The CSR numbers present in the state's CSR file, **minus the Zicntr
/// counters**.
///
/// ⚠️ 计数器的条目是**平台自己**写进去的（**退休计数**，STAGE-20 裁决 F）
/// ⇒ ⇒ 它们**与被测的那条指令无关** ⇒ ⇒ 「这条指令写了哪些 CSR」这条
/// 判据必须把**它们**排除掉 —— 否则每一条**跑过指令**的用例都会看到两个
/// **恒定存在**的条目（`mcycle` / `minstret`）。
///
/// ⚠️ 排除用的是契约层的 [`counter_pair`] ⇒ ⇒ 「哪些 CSR 是计数器」**只有
/// 一处答案**（STAGE-20 裁决 C）。
pub(super) fn csr_numbers_without_counters(state: &SimState) -> Vec<u16> {
    csr_numbers(state)
        .into_iter()
        .filter(|number| counter_pair(*number).is_none())
        .collect()
}

/// ISS + RAM harness driving the closure memory port.
pub(super) struct Harness {
    pub(super) ram: RefCell<TestRam>,
    pub(super) iss: Iss,
}

impl Harness {
    pub(super) fn new() -> Self {
        Self {
            ram: RefCell::new(TestRam::new(0, 64 * 1024)),
            iss: Iss::default(),
        }
    }

    pub(super) fn load(&mut self, addr: u32, words: &[u32]) {
        self.ram.borrow_mut().load(addr, words);
    }

    pub(super) fn step(&mut self, state: &mut SimState) -> Result<StepOutcome, PlatformError> {
        let ram = &self.ram;
        self.iss.step_one(
            state,
            |addr, size| ram.borrow().read(addr, size),
            |addr, size, value| ram.borrow_mut().write(addr, size, value),
        )
    }

    /// Step a single instruction placed at `pc` and return the outcome.
    pub(super) fn step_one_at(&mut self, state: &mut SimState, pc: u32, word: u32) -> StepOutcome {
        self.load(pc, &[word]);
        state.pc = pc;
        self.step(state).expect("step succeeds")
    }

    /// Step a single **compressed** instruction placed at `pc`.
    pub(super) fn step_half_at(&mut self, state: &mut SimState, pc: u32, half: u16) -> StepOutcome {
        self.ram.borrow_mut().load_half(pc, half);
        state.pc = pc;
        self.step(state).expect("step succeeds")
    }
}

/// ISS + M-14 bus harness driving the production dispatch path.
pub(super) struct BusHarness {
    pub(super) bus: Bus,
    pub(super) iss: Iss,
}

impl BusHarness {
    /// Map a 4 KiB RAM region at address 0 holding `bytes`.
    pub(super) fn new(bytes: &[u8]) -> Self {
        let mut bus = Bus::new();
        bus.map_ram(0, 0x1000, bytes).expect("map ram");
        Self {
            bus,
            iss: Iss::default(),
        }
    }

    pub(super) fn step(&mut self, state: &mut SimState) -> Result<StepOutcome, PlatformError> {
        self.iss.step_one_via_bus(state, &mut self.bus)
    }
}
