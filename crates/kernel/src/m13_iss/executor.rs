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

/// Executor wrapper. Holds the memory accessors so the per-instruction
/// site stays readable.
pub struct Executor<F, G>
where
    F: Fn(u32, riscv_sim_contract::model::WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, riscv_sim_contract::model::WriteSize, u32) -> Result<(), PlatformError>,
{
    /// The fields below are wired through `Executor::new` but the
    /// MVP-only `ADDI` path never invokes them. Used by LOAD / STORE
    /// once M-13 widens to the full RV32IMC set (LLD §4.13).
    #[allow(dead_code)]
    read_mem: F,
    #[allow(dead_code)]
    write_mem: G,
}

impl<F, G> Executor<F, G>
where
    F: Fn(u32, riscv_sim_contract::model::WriteSize) -> Result<u32, PlatformError>,
    G: Fn(u32, riscv_sim_contract::model::WriteSize, u32) -> Result<(), PlatformError>,
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
        // 0b0010011 = I-type ALU (addi / slti / etc.). MVP only implements
        // ADDI (funct3 == 0x0) to keep the harness small.
        if d.opcode == 0b0010011 {
            if d.funct3 == 0x0 {
                // ADDI: rd = rs1 + imm
                let lhs = read_gpr(state, d.rs1) as i32;
                let result = lhs.wrapping_add(d.imm) as u32;
                write_gpr(state, d.rd, result);
                state.pc = state.pc.wrapping_add(4);
                return Ok(StepOutcome::Advanced);
            }
            return Ok(StepOutcome::Trapped {
                cause: TrapCause::IllegalInstruction,
            });
        }

        Err(PlatformError::new(
            ErrorFamily::Iss,
            2,
            format!(
                "unimplemented opcode 0b{:07b} at pc=0x{:08x}",
                d.opcode, d.pc
            ),
        ))
    }
}
