//! M-13 RV32IMC 指令集仿真器 (Instruction Set Simulator).
//!
//! MVP target: interpret-execute the integer base (RV32I) plus the
//! standard `M` and `C` extensions. The decoder is hand-written (no
//! external generator) to keep the dependency surface small and the
//! binary small when compiled to WASM.
//!
//! Determinism: every instruction is a pure function of
//! `(gpr, csr, memory)`; the executor does not read wall-clock time.
//! Floating-point extensions are out of scope for MVP (AD §3.3).
//!
//! TODO(MVP): hook into the [`crate::m12_engine`] scheduler, the
//! [`crate::m14_bus`] for memory access, and the breakpoint /
//! watchpoint traps. The skeleton below compiles and exposes the
//! `decode + execute_one` API for tests.

pub mod decoder;
pub mod executor;

pub use decoder::{Decoded, Decoder};
pub use executor::{Executor, StepOutcome};

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};

/// ISS configuration. The MVP defaults match the architecture decisions
/// (AD §9.7) — RV32IMC, interpretive, reserved space for JIT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssConfig {
    /// Reset vector (PC at boot). Defaults to the standard
    /// `0x0000_0000`; can be overridden by M-06 ELF loading.
    pub reset_vector: u32,
    /// Maximum number of instructions the executor will run inside a
    /// single quantum. See [`crate::m12_engine::DEFAULT_INSTRUCTION_BUDGET`].
    pub quantum_budget: u32,
}

impl Default for IssConfig {
    fn default() -> Self {
        Self { reset_vector: 0x0000_0000, quantum_budget: 10_000 }
    }
}

/// The ISS itself: pure interpreter over [`SimState`].
#[derive(Debug)]
pub struct Iss {
    /// Reserved for the M-12 instruction-budget check; the MVP
    /// skeleton does not yet thread the budget through `step_one`.
    #[allow(dead_code)]
    cfg: IssConfig,
    decoder: Decoder,
}

impl Iss {
    /// Construct a new ISS with the given configuration.
    pub fn new(cfg: IssConfig) -> Self {
        Self { cfg, decoder: Decoder::default() }
    }

    /// Run a single instruction and return the outcome. Memory and CSR
    /// accessors are funcs so the ISS stays a pure interpreter for
    /// deterministic replay.
    pub fn step_one<F, G>(
        &mut self,
        state: &mut SimState,
        read_mem: F,
        write_mem: G,
    ) -> Result<StepOutcome, PlatformError>
    where
        F: Fn(u32, WriteSize) -> Result<u32, PlatformError>,
        G: Fn(u32, WriteSize, u32) -> Result<(), PlatformError>,
    {
        let raw = read_mem(state.pc, WriteSize::Word)
            .map_err(|e| PlatformError::new(ErrorFamily::Iss, 1, format!("fetch at pc={:08x}: {}", state.pc, e)))?;
        let decoded = self.decoder.decode(raw, state.pc);
        let mut exec = Executor::new(read_mem, write_mem);
        exec.execute(state, &decoded)
    }
}

impl Default for Iss {
    fn default() -> Self { Self::new(IssConfig::default()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::WriteSize;

    fn make_mem(contents: &[(u32, u32)]) -> Vec<(u32, u32)> {
        contents.to_vec()
    }

    /// Read from a tiny associative "RAM".
    fn read(mem: &[(u32, u32)], addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        for (a, v) in mem {
            if *a == addr {
                return Ok(match size {
                    WriteSize::Byte  => v & 0xFF,
                    WriteSize::Half  => v & 0xFFFF,
                    WriteSize::Word  => *v,
                });
            }
        }
        Ok(0)
    }

    /// Hand-rolled `addi x1, x0, 42` (0x02A00093). After execution,
    /// x1 should be 42.
    #[test]
    fn addi_executes() {
        let mem = make_mem(&[(0x0000_0000, 0x02A0_0093)]);
        let mut state = SimState::default();
        let mut iss = Iss::default();
        let outcome = iss.step_one(
            &mut state,
            |a, s| read(&mem, a, s),
            |_, _, _| Ok(()),
        ).expect("step");
        assert_eq!(state.gpr[1], 42);
        assert_eq!(state.pc, 4);
        assert!(matches!(outcome, StepOutcome::Advanced));
    }
}
