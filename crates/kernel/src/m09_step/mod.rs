//! M-09 单步引擎 (Step Engine).
//!
//! 把宿主侧的「单步动词」（step / step-over / step-out / step-line）翻译成
//! **执行后端的一次调用**，并如实报告这次单步**实际做到了什么**。
//!
//! ## 退化口径（重要）
//!
//! M-08 LLD §1.3 把 `stepOver` / `stepOut` 定义为「注入临时断点再继续」，
//! `stepSource` 定义为「按 DWARF 行边界推进」。这两者分别依赖
//!
//! - **调用栈**（返回地址在 `ra` 里的哪一层）——本平台**没有**调用栈跟踪；
//! - **DWARF 行表**（M-11）——本阶段**没有**接线。
//!
//! 因此本阶段 `Over` / `Out` / `Line` **一律退化为指令级单步**，并且
//! [`StepEngine::degradation`] 把这件事**显式说出来**——退化是可见的，
//! 不是悄悄发生的。
//!
//! ## 状态
//!
//! v0.13：真实实现（v0.11 之前是 71 行的空壳，`plan` 对所有 kind 都返回
//! `Plan::One` 且没有任何地方调用它）。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::SimState;
use riscv_sim_contract::port::ExecutionBackend;
use riscv_sim_contract::protocol::HaltReason;

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

impl Default for StepKind {
    fn default() -> Self {
        Self::Instruction
    }
}

impl StepKind {
    /// All step kinds, in a stable order.
    pub const ALL: [StepKind; 4] = [
        StepKind::Instruction,
        StepKind::Over,
        StepKind::Out,
        StepKind::Line,
    ];

    /// Parse a step kind from its wire name (`instruction` / `over` /
    /// `out` / `line`).
    ///
    /// Returns `None` for an unknown name — the caller decides whether an
    /// unknown verb is an error or a fallback.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "instruction" | "step" => Some(Self::Instruction),
            "over" | "step_over" => Some(Self::Over),
            "out" | "step_out" => Some(Self::Out),
            "line" | "step_line" => Some(Self::Line),
            _ => None,
        }
    }

    /// The wire name of this step kind.
    pub fn name(self) -> &'static str {
        match self {
            Self::Instruction => "instruction",
            Self::Over => "over",
            Self::Out => "out",
            Self::Line => "line",
        }
    }
}

/// What the engine should do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Run a single instruction and stop.
    One,
}

impl Default for Plan {
    fn default() -> Self {
        Self::One
    }
}

/// The result of one step request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    /// How many instructions actually executed (may be `0` if the backend
    /// stopped before executing anything — e.g. a breakpoint was already
    /// pending at the current PC).
    pub executed: u32,
    /// The program counter after the step.
    pub pc: u32,
    /// Why the backend stopped, if it stopped for a reason other than
    /// "the instruction budget ran out".
    ///
    /// `None` means the step completed its budget normally.
    pub reason: Option<HaltReason>,
    /// A human-readable note when the requested verb had to be degraded.
    /// `None` for a plain instruction step.
    pub degraded: Option<&'static str>,
}

/// Step engine — pure orchestration. Calls into M-12 / M-08; the
/// kernel wires this up at construction.
#[derive(Debug, Default)]
pub struct StepEngine {
    _private: (),
}

impl StepEngine {
    /// New step engine.
    pub fn new() -> Self {
        Self::default()
    }

    /// Plan a step. The host calls this to learn which engine APIs to
    /// invoke next.
    pub fn plan(&self, kind: StepKind, _state: &SimState) -> Result<Plan, PlatformError> {
        let _ = kind;
        Ok(Plan::One)
    }

    /// The degradation note for a step kind, or `None` when the verb is
    /// served exactly.
    ///
    /// The note is **not** an error — the step still runs, one instruction
    /// at a time. It exists so the host can tell the user what actually
    /// happened instead of pretending `step-over` crossed a call.
    pub fn degradation(&self, kind: StepKind) -> Option<&'static str> {
        match kind {
            StepKind::Instruction => None,
            StepKind::Over => Some("M-09 降级：本平台无调用栈跟踪，step-over 退化为指令级单步"),
            StepKind::Out => Some("M-09 降级：本平台无调用栈跟踪，step-out 退化为指令级单步"),
            StepKind::Line => {
                Some("M-09 降级：DWARF 行表（M-11）尚未接线，step-line 退化为指令级单步")
            }
        }
    }

    /// Execute one step request against the execution backend.
    ///
    /// The backend is driven through the **contract port**
    /// ([`ExecutionBackend::step_n`]), so M-09 never learns whether the
    /// backend is the in-tree ISS or (later) a QEMU process.
    ///
    /// The stop reason is **not** part of the contract port, so it is
    /// reported as `None` here; the host that owns the concrete backend
    /// reads the reason from it directly.
    pub fn execute<B: ExecutionBackend>(
        &self,
        kind: StepKind,
        backend: &mut B,
    ) -> Result<StepOutcome, PlatformError> {
        let plan = self.plan(kind, &SimState::default())?;
        let budget = match plan {
            Plan::One => 1,
        };
        let executed = backend.step_n(budget)?;
        let pc = backend.snapshot()?.pc;
        Ok(StepOutcome {
            executed,
            pc,
            reason: None,
            degraded: self.degradation(kind),
        })
    }

    /// Family used by errors raised from this module.
    pub const FAMILY: ErrorFamily = ErrorFamily::Debug;
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::WriteSize;
    use riscv_sim_contract::snapshot::EngineSnapshotDto;

    /// A backend that executes a fixed script and counts instructions.
    struct FakeBackend {
        executed: u32,
        pc: u32,
    }

    impl FakeBackend {
        fn new(executed: u32, pc: u32) -> Self {
            Self { executed, pc }
        }
    }

    impl ExecutionBackend for FakeBackend {
        fn reset(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
        fn step_n(&mut self, n: u32) -> Result<u32, PlatformError> {
            self.executed += n;
            Ok(n)
        }
        fn continue_until_event(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
        fn pause(&mut self) -> Result<(), PlatformError> {
            Ok(())
        }
        fn snapshot(&self) -> Result<SimState, PlatformError> {
            Ok(SimState {
                pc: self.pc,
                ..SimState::default()
            })
        }
        fn restore(&mut self, state: &SimState) -> Result<(), PlatformError> {
            self.pc = state.pc;
            Ok(())
        }
        fn snapshot_full(&self) -> Result<EngineSnapshotDto, PlatformError> {
            Err(PlatformError::new(ErrorFamily::Snapshot, 3, "unsupported"))
        }
        fn read_memory(&self, _addr: u32, _size: WriteSize) -> Result<u32, PlatformError> {
            Ok(0)
        }
        fn write_memory(
            &mut self,
            _addr: u32,
            _size: WriteSize,
            _value: u32,
        ) -> Result<(), PlatformError> {
            Ok(())
        }
        fn set_breakpoint(&mut self, _addr: u32, _enabled: bool) -> Result<(), PlatformError> {
            Ok(())
        }
        fn set_watchpoint(
            &mut self,
            _addr: u32,
            _size: WriteSize,
            _write: bool,
            _enabled: bool,
        ) -> Result<(), PlatformError> {
            Ok(())
        }
    }

    #[test]
    fn instruction_step_has_no_degradation() {
        let e = StepEngine::new();
        assert_eq!(e.degradation(StepKind::Instruction), None);
    }

    #[test]
    fn over_out_line_are_explicitly_degraded() {
        let e = StepEngine::new();
        for kind in [StepKind::Over, StepKind::Out, StepKind::Line] {
            let note = e.degradation(kind).expect("必须有退化说明");
            assert!(
                note.contains("退化为指令级单步"),
                "退化说明要说清结果：{note}"
            );
        }
    }

    #[test]
    fn plan_is_one_instruction_for_every_kind() {
        let e = StepEngine::new();
        let state = SimState::default();
        for kind in StepKind::ALL {
            assert_eq!(e.plan(kind, &state).unwrap(), Plan::One);
        }
    }

    #[test]
    fn execute_runs_exactly_one_instruction() {
        let e = StepEngine::new();
        let mut backend = FakeBackend::new(0, 0x8000_0004);
        let outcome = e.execute(StepKind::Instruction, &mut backend).unwrap();
        assert_eq!(outcome.executed, 1);
        assert_eq!(outcome.pc, 0x8000_0004);
        assert_eq!(outcome.degraded, None);
        assert_eq!(outcome.reason, None);
    }

    #[test]
    fn execute_reports_the_degradation_of_the_verb() {
        let e = StepEngine::new();
        let mut backend = FakeBackend::new(0, 0);
        let outcome = e.execute(StepKind::Over, &mut backend).unwrap();
        assert!(outcome.degraded.is_some());
        assert_eq!(outcome.executed, 1, "退化不等于不执行");
    }

    #[test]
    fn kind_names_round_trip() {
        for kind in StepKind::ALL {
            assert_eq!(StepKind::from_name(kind.name()), Some(kind));
        }
        assert_eq!(StepKind::from_name("nope"), None);
    }
}
