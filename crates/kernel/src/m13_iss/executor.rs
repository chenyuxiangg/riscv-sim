//! Executor for the RV32I instruction set.
//!
//! The executor is a pure function over `(gpr, mem, decoded)`. It
//! does not maintain its own state — the caller threads the
//! [`SimState`] through, which is what gives us deterministic replay.
//!
//! There is exactly **one** execution path ([`execute_one`]). Every
//! dispatch route — the closure-backed [`Executor`] and the bus-backed
//! adapter in the crate root — feeds the same function, so the two
//! routes cannot drift apart in their instruction semantics (D-11).
//!
//! `StepOutcome` tells the engine whether the instruction completed,
//! trapped, or paused at a breakpoint.

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::{SimState, WriteSize};

use super::alu;
use super::decoder::{Decoded, LEN_32};

/// Result of a single instruction execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Instruction retired normally; PC advanced.
    Advanced,
    /// Instruction triggered a trap (illegal / misaligned / bus error).
    Trapped { cause: TrapCause },
    /// Instruction hit a breakpoint inserted by M-08.
    Breakpoint,
    /// Instruction hit a watchpoint inserted by M-10.
    Watchpoint,
    /// Engine pause requested by host mid-quantum.
    HostPause,
}

impl Default for StepOutcome {
    fn default() -> Self {
        Self::Advanced
    }
}

/// Trap reason taxonomy. Mapped to `mtval` by the engine (LLD §4.13.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapCause {
    /// Illegal instruction.
    IllegalInstruction,
    /// Misaligned memory access.
    MisalignedAccess,
    /// Out-of-range physical address.
    OutOfRange,
    /// Environment call from M-mode.
    Ecall,
    /// Guest `EBREAK` (mcause = 3). Distinct from
    /// [`StepOutcome::Breakpoint`], which means a *host* breakpoint
    /// inserted by M-08 rather than a guest instruction.
    Breakpoint,
    /// Reserved for the `M` extension (division by zero, mcause = 6).
    /// The M extension lands in a later wave; the cause is declared
    /// here so the taxonomy does not have to change shape when it does.
    IntegerDivideByZero,
}

impl Default for TrapCause {
    fn default() -> Self {
        Self::IllegalInstruction
    }
}

/// Read a general-purpose register.
///
/// x0 is hardwired to zero: the architectural state of x0 is always
/// readable as 0 regardless of what the register file happens to
/// contain. Every read path must go through this helper (D-04) —
/// a bare `state.gpr[rs1]` read lets a corrupted / externally written
/// x0 leak into the executed semantics.
pub fn read_gpr(state: &SimState, r: u8) -> u32 {
    if r == 0 {
        0
    } else {
        state.gpr[r as usize]
    }
}

/// Write a general-purpose register.
///
/// Writes to x0 are discarded — x0 is hardwired to zero (D-04). Every
/// write path must go through this helper so no instruction can
/// clobber x0.
pub fn write_gpr(state: &mut SimState, r: u8, value: u32) {
    if r != 0 {
        state.gpr[r as usize] = value;
    }
}

/// Memory access port used by the executor.
///
/// The ISS never touches memory directly: every load / store goes
/// through this port. Two adapters implement it — the closure-backed
/// [`Executor`] and the bus-backed adapter in the crate root — so both
/// dispatch routes run identical semantics (D-11). A third adapter can
/// be added (e.g. a flat RAM model) without touching the semantics.
pub trait MemoryAccess {
    /// Read `size` bytes at `addr`; the value is returned in the low
    /// bytes of the result (little-endian).
    fn read(&mut self, addr: u32, size: WriteSize) -> Result<u32, PlatformError>;
    /// Write the low `size` bytes of `value` at `addr`.
    fn write(&mut self, addr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError>;
}

/// Executor wrapper. Holds the memory accessors so the per-instruction
/// site stays readable.
pub struct Executor<F, G>
where
    F: Fn(u32, WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, WriteSize, u32) -> Result<(), PlatformError>,
{
    read_mem: F,
    write_mem: G,
}

impl<F, G> Executor<F, G>
where
    F: Fn(u32, WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, WriteSize, u32) -> Result<(), PlatformError>,
{
    /// Build a new executor from memory accessors.
    pub fn new(read_mem: F, write_mem: G) -> Self {
        Self {
            read_mem,
            write_mem,
        }
    }

    /// Execute one decoded instruction, mutating the [`SimState`].
    pub fn execute(
        &mut self,
        state: &mut SimState,
        d: &Decoded,
    ) -> Result<StepOutcome, PlatformError> {
        execute_one(self, state, d)
    }
}

impl<F, G> MemoryAccess for Executor<F, G>
where
    F: Fn(u32, WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, WriteSize, u32) -> Result<(), PlatformError>,
{
    fn read(&mut self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        (self.read_mem)(addr, size)
    }

    fn write(&mut self, addr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        (self.write_mem)(addr, size, value)
    }
}

/// Instruction identity after two-level decode.
///
/// The LLD's decode key is `opcode | funct3 | funct7` (评审 13-5): the
/// first level picks the instruction category, the second picks the
/// operation inside it. [`classify`] implements exactly that — a key
/// that does not resolve is an illegal instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// LUI — load the upper immediate.
    Lui,
    /// AUIPC — add the upper immediate to the PC.
    Auipc,
    /// ADDI — add immediate.
    Addi,
    /// SLTI — set less than, signed immediate.
    Slti,
    /// SLTIU — set less than, unsigned immediate.
    Sltiu,
    /// XORI — exclusive-or immediate.
    Xori,
    /// ORI — or immediate.
    Ori,
    /// ANDI — and immediate.
    Andi,
    /// SLLI — shift left logical, immediate shift amount.
    Slli,
    /// SRLI — shift right logical, immediate shift amount.
    Srli,
    /// SRAI — shift right arithmetic, immediate shift amount.
    Srai,
    /// ADD — add.
    Add,
    /// SUB — subtract.
    Sub,
    /// SLL — shift left logical.
    Sll,
    /// SLT — set less than, signed.
    Slt,
    /// SLTU — set less than, unsigned.
    Sltu,
    /// XOR — exclusive-or.
    Xor,
    /// SRL — shift right logical.
    Srl,
    /// SRA — shift right arithmetic.
    Sra,
    /// OR — or.
    Or,
    /// AND — and.
    And,
}

/// Second-level decode: map a decode key onto a concrete operation.
///
/// `None` means the key is not a legal RV32I encoding — the caller
/// turns that into an illegal-instruction trap (LLD §3.2: a key that
/// misses the table is an illegal instruction).
pub fn classify(d: &Decoded) -> Option<Op> {
    match d.opcode {
        // U-type. Neither LUI nor AUIPC has a funct3 field — bits
        // 14:12 belong to the immediate.
        0b011_0111 => Some(Op::Lui),
        0b001_0111 => Some(Op::Auipc),
        // OP-IMM.
        0b001_0011 => match d.funct3 {
            0b000 => Some(Op::Addi),
            0b001 => (d.funct7 == 0b000_0000).then_some(Op::Slli),
            0b010 => Some(Op::Slti),
            0b011 => Some(Op::Sltiu),
            0b100 => Some(Op::Xori),
            0b101 => match d.funct7 {
                0b000_0000 => Some(Op::Srli),
                0b010_0000 => Some(Op::Srai),
                _ => None,
            },
            0b110 => Some(Op::Ori),
            0b111 => Some(Op::Andi),
            _ => None,
        },
        // OP (register-register).
        0b011_0011 => match (d.funct3, d.funct7) {
            (0b000, 0b000_0000) => Some(Op::Add),
            (0b000, 0b010_0000) => Some(Op::Sub),
            (0b001, 0b000_0000) => Some(Op::Sll),
            (0b010, 0b000_0000) => Some(Op::Slt),
            (0b011, 0b000_0000) => Some(Op::Sltu),
            (0b100, 0b000_0000) => Some(Op::Xor),
            (0b101, 0b000_0000) => Some(Op::Srl),
            (0b101, 0b010_0000) => Some(Op::Sra),
            (0b110, 0b000_0000) => Some(Op::Or),
            (0b111, 0b000_0000) => Some(Op::And),
            _ => None,
        },
        _ => None,
    }
}

/// PC after an instruction that falls through to the next one.
fn sequential(d: &Decoded) -> u32 {
    d.pc.wrapping_add(d.len as u32)
}

/// The single execution path shared by every dispatch route.
///
/// `_mem` is the memory port: it is threaded through here so that both
/// dispatch routes are structurally identical, and so that the first
/// instructions needing it (LOAD / STORE) land on one code path rather
/// than two (D-11).
pub fn execute_one<M: MemoryAccess>(
    _mem: &mut M,
    state: &mut SimState,
    d: &Decoded,
) -> Result<StepOutcome, PlatformError> {
    if d.len != LEN_32 {
        // A compressed (RV32C) encoding: a legal instruction the ISS
        // cannot execute yet. Trap rather than mis-execute it — the
        // length field is what makes the distinction possible (D-16).
        return Ok(StepOutcome::Trapped {
            cause: TrapCause::IllegalInstruction,
        });
    }

    let op = match classify(d) {
        Some(op) => op,
        None => {
            // An encoding the ISS cannot execute is an illegal
            // instruction, not a platform failure: both dispatch routes
            // report it identically (D-11). The engine turns this into
            // mcause = 2 once trap delivery is wired (LLD §5.2.4).
            return Ok(StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction,
            });
        }
    };

    let next_pc = match op {
        Op::Lui => {
            // The decoded U-type immediate already carries the value in
            // bits 31:12 with a zeroed low half.
            write_gpr(state, d.rd, d.imm as u32);
            sequential(d)
        }
        Op::Auipc => {
            write_gpr(state, d.rd, d.pc.wrapping_add(d.imm as u32));
            sequential(d)
        }
        Op::Addi => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::add(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Slti => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::slt(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Sltiu => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::sltu(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Xori => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::xor(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Ori => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::or(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Andi => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::and(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Slli => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::sll(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Srli => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::srl(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Srai => {
            let lhs = read_gpr(state, d.rs1);
            write_gpr(state, d.rd, alu::sra(lhs, d.imm as u32));
            sequential(d)
        }
        Op::Add => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::add(lhs, rhs));
            sequential(d)
        }
        Op::Sub => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::sub(lhs, rhs));
            sequential(d)
        }
        Op::Sll => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::sll(lhs, rhs));
            sequential(d)
        }
        Op::Slt => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::slt(lhs, rhs));
            sequential(d)
        }
        Op::Sltu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::sltu(lhs, rhs));
            sequential(d)
        }
        Op::Xor => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::xor(lhs, rhs));
            sequential(d)
        }
        Op::Srl => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::srl(lhs, rhs));
            sequential(d)
        }
        Op::Sra => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::sra(lhs, rhs));
            sequential(d)
        }
        Op::Or => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::or(lhs, rhs));
            sequential(d)
        }
        Op::And => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::and(lhs, rhs));
            sequential(d)
        }
    };

    state.pc = next_pc;
    Ok(StepOutcome::Advanced)
}
