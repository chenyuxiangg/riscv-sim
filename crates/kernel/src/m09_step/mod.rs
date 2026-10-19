//! M-09 单步引擎 (Step Engine).
//!
//! Implements the user-facing `StepN` / `StepOver` / `StepOut` verbs.
//! Built on top of M-08 (breakpoints) and M-12 (scheduler); the step
//! engine simply calls the engine with an instruction budget.
//!
//! Status: stub — the verbs are defined; bodies are TODO(MVP).

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::SimState;

/// Step granularity requested by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    /// Step exactly one ISS instruction.
    Instruction,
    /// Step over the current function call (set a temp breakpoint at
    /// the return PC, then continue).
    Over,
    /// Step out of the current function.
    Out,
    /// Step until the next line (requires DWARF, M-11).
    Line,
}

impl Default for StepKind { fn default() -> Self { Self::Instruction } }

/// Step engine — pure orchestration. Calls into M-12 / M-08; the
/// kernel wires this up at construction.
#[derive(Debug, Default)]
pub struct StepEngine {
    _private: (),
}

impl StepEngine {
    /// New step engine.
    pub fn new() -> Self { Self::default() }

    /// Plan a step. The host calls this to learn which engine APIs to
    /// invoke next. MVP always returns `Plan::One`.
    pub fn plan(&self, kind: StepKind, _state: &SimState) -> Result<Plan, PlatformError> {
        Ok(match kind {
            StepKind::Instruction => Plan::One,
            StepKind::Over        => Plan::One, // TODO(MVP): set temp BP
            StepKind::Out         => Plan::One, // TODO(MVP): set temp BP at RA
            StepKind::Line        => Plan::One, // TODO(MVP): DWARF lookup
        })
    }

    /// Family used by errors raised from this module.
    pub const FAMILY: ErrorFamily = ErrorFamily::Debug;
}

/// What the engine should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Run a single instruction and stop.
    One,
}

impl Default for Plan { fn default() -> Self { Self::One } }
