//! Executor for the MVP subset (RV32I ALU + a single I-type immediate).
//!
//! The executor is a pure function over `(gpr, mem, decoded)`. It
//! does not maintain its own state — the caller threads the
//! [`SimState`] through, which is what gives us deterministic replay.
//!
//! `StepOutcome` tells the engine whether the instruction completed,
//! trapped, or paused at a breakpoint.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::SimState;

use super::decoder::Decoded;

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

impl Default for StepOutcome { fn default() -> Self { Self::Advanced } }

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
}

impl Default for TrapCause { fn default() -> Self { Self::IllegalInstruction } }

/// Executor wrapper. Holds the memory accessors so the per-instruction
/// site stays readable.
pub struct Executor<F, G>
where
    F: Fn(u32, riscv_sim_contract::model::WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, riscv_sim_contract::model::WriteSize, u32) -> Result<(), PlatformError>,
{
    read_mem: F,
    write_mem: G,
}

impl<F, G> Executor<F, G>
where
    F: Fn(u32, riscv_sim_contract::model::WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, riscv_sim_contract::model::WriteSize, u32) -> Result<(), PlatformError>,
{
    /// Build a new executor from memory accessors.
    pub fn new(read_mem: F, write_mem: G) -> Self { Self { read_mem, write_mem } }

    /// Execute one decoded instruction, mutating the [`SimState`].
    pub fn execute(&mut self, state: &mut SimState, d: &Decoded) -> Result<StepOutcome, PlatformError> {
        // 0b0010011 = I-type ALU (addi / slti / etc.). MVP only implements
        // ADDI (funct3 == 0x0) to keep the harness small.
        if d.opcode == 0b0010011 {
            if d.funct3 == 0x0 {
                // ADDI: rd = rs1 + imm
                let lhs = state.gpr[d.rs1 as usize] as i32;
                let result = lhs.wrapping_add(d.imm) as u32;
                state.gpr[d.rd as usize] = result;
                state.pc = state.pc.wrapping_add(4);
                return Ok(StepOutcome::Advanced);
            }
            return Ok(StepOutcome::Trapped { cause: TrapCause::IllegalInstruction });
        }

        Err(PlatformError::new(
            ErrorFamily::Iss,
            2,
            format!("unimplemented opcode 0b{:07b} at pc=0x{:08x}", d.opcode, d.pc),
        ))
    }
}
