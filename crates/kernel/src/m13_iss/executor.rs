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

    // An encoding the ISS cannot execute is an illegal instruction, not
    // a platform failure: both dispatch routes report it identically
    // (D-11). The engine turns this into mcause = 2 once trap delivery
    // is wired (LLD §5.2.4).
    Ok(StepOutcome::Trapped {
        cause: TrapCause::IllegalInstruction,
    })
}
