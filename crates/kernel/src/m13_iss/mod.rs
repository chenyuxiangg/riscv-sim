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

use riscv_sim_contract::csr::PmpAccess;

pub mod alu;
pub mod compressed;
pub mod csr;
pub mod decoder;
pub mod executor;

/// Zicntr 硬件计数器（`mcycle` / `minstret` 与它们的影子）。
///
/// ⚠️ 它是 [`executor::execute_one`] 的**内部**部件 —— 对外**不导出**任何
/// 符号：计数器的**读**在 [`csr::csr_read`] 里，**自增**的**唯一落点**在
/// [`executor::execute_one`] 的正常完成出口（STAGE-20 裁决 F）。
mod counters;

/// PMP —— 物理内存保护（STAGE-21 · v0.37）。
///
/// ⚠️ 它是**内部**部件：判定只有一个入口 [`pmp::pmp_allows`]，取指与读与写
/// 三条访存路径都调它（裁决 G）⇒ ⇒ 对外不导出任何符号。
mod pmp;

pub use decoder::{Decoded, Decoder};
pub use executor::{
    classify, execute_one, read_gpr, write_gpr, Executor, MemoryAccess, MisalignedPolicy, Op,
    StepOutcome, TrapCause,
};

/// The platform's **single** trap entry (STAGE-4 §3.5).
///
/// The exception path and the interrupt path both go through it, so
/// "what a trap does to the architectural state" has one answer.
pub use csr::enter_trap;

/// The current **privilege level** —— `M` / `S` / `U`（STAGE-22）。
///
/// ⚠️ 它住在 CSR 文件里的**伪 CSR** `CSR_PRIV` 那一条 ⇒ ⇒ 快照天然一致
/// ⇒ ⇒ **不升** `SIM_STATE_SCHEMA_VERSION`。
pub use csr::{priv_get, priv_set};

use self::executor::is_unmapped_access;
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};

use crate::m14_bus::{Bus, BusWidth};

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
        Self {
            reset_vector: 0x0000_0000,
            quantum_budget: 10_000,
        }
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
    misaligned: MisalignedPolicy,
    /// 上一条指令**写入**的 CSR 号（`None` = 没写 / 还没跑过）。
    ///
    /// ⚠️ 这是**给平台层看的回报**，不是 ISS 自己的副作用：ISS 只**报告**
    /// 「我写了哪个 CSR」，「写了要做什么」（例如 `irq_clear` ⇒ 清中断线）
    /// 由 **M-17** 决定（STAGE-4 §4 接缝 #4）。
    ///
    /// ⚠️ **为什么必须有它**：平台层若靠「**值变了**」来猜「guest 写了
    /// 这个 CSR」，那么 guest 写**同一个值**两次时，第二次就会被**静默
    /// 丢弃** —— 那是 v0.19 检视轮查出的 **F-1**（第二次 `irq_clear`
    /// 被跳过 ⇒ `active_irq` 永不复位 ⇒ 那条线被单级防护**永久堵死**）。
    last_csr_written: Option<u16>,
}

impl Iss {
    /// Construct a new ISS with the given configuration.
    pub fn new(cfg: IssConfig) -> Self {
        Self {
            cfg,
            decoder: Decoder::default(),
            misaligned: MisalignedPolicy::default(),
            last_csr_written: None,
        }
    }

    /// 取走「上一条指令写入的 CSR 号」（**取走即消失**）。
    ///
    /// 返回 `None` = 上一条指令**没有写**任何 CSR（或还没跑过指令）。
    ///
    /// ⚠️ **一次只对应一条指令**：每执行一条指令，这个值就被**重置**
    /// ——所以平台层必须**紧接着**那一步取走它，隔一条再取就取不到了。
    pub fn take_last_csr_written(&mut self) -> Option<u16> {
        self.last_csr_written.take()
    }

    /// Select the policy for misaligned data accesses.
    ///
    /// Defaults to [`MisalignedPolicy::Allow`] — LLD §3.6(2) / §5.2.1
    /// require the Spike-aligned behaviour ("默认不触发…保持 golden
    /// 兼容"). Use [`MisalignedPolicy::Trap`] to opt into strict
    /// diagnostics, where an unaligned access surfaces as a trap
    /// instead of being performed.
    pub fn with_misaligned_policy(mut self, policy: MisalignedPolicy) -> Self {
        self.misaligned = policy;
        self
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
        // ⚠️ **先清零**（与 [`Iss::step_one_via_bus`] 同口径）：每条指令只
        //    对应**一次**回报。
        self.last_csr_written = None;
        if state.pc & 0x1 != 0 {
            // Misaligned instruction address: the alignment check
            // precedes the fetch itself (LLD §3.1, 评审 13-3).
            //
            // ⚠️ The cause is **0** (instruction address misaligned) —
            // a *different* number from a misaligned load (`4`) or
            // store (`6`). That split is the whole point of ledger
            // **D-63**; the old single `MisalignedAccess` variant
            // reported the load / store code for the fetch as well.
            //
            // `mtval` carries the **faulting address** (STAGE-4 §3.5).
            let cause = TrapCause::InstructionAddressMisaligned;
            enter_trap(state, cause.mcause(), state.pc);
            return Ok(StepOutcome::Trapped { cause });
        }
        // ⚠️ **取指也受 PMP 约束**（F14）—— 没有 `X` 权限 ⇒ ⇒ `mcause = 1`。
        //
        // ⚠️ 这里查的是 **4 字节**（指令的最大长度）：取指前还不知道这条指令
        //    是 2 字节还是 4 字节 ⇒ ⇒ 用**最大**窗口 ⇒ ⇒ 宁可多拒一点，也不要
        //    让一次跨边界的取指「半截通过」。
        //
        // ⚠️ 判定走的是**共享**的 [`fetch_pmp_trap`] ⇒ ⇒ 与
        //    [`Iss::step_one_via_bus`] 那条路**同口径**（D-11）⇒ ⇒ 不会出现
        //    「一条路查 PMP、另一条不查」的分裂。
        if let Some(cause) = fetch_pmp_trap(state) {
            enter_trap(state, cause.mcause(), state.pc);
            return Ok(StepOutcome::Trapped { cause });
        }
        let raw = read_mem(state.pc, WriteSize::Word).map_err(|e| {
            PlatformError::new(
                ErrorFamily::Iss,
                1,
                format!("fetch at pc={:08x}: {}", state.pc, e),
            )
        })?;
        let decoded = self.decoder.decode(raw, state.pc);
        let mut exec = Executor::new(read_mem, write_mem).with_misaligned_policy(self.misaligned);
        let mut wrote_csr = None;
        let outcome = exec.execute_with_report(state, &decoded, &mut wrote_csr);
        self.last_csr_written = wrote_csr;
        outcome
    }
    /// Run a single instruction with all memory traffic routed through
    /// the supplied [`Bus`] (M-14). This is the production dispatch
    /// path — every fetch / load / store goes through the address
    /// decoder and contributes to the bus's stats / hook.
    ///
    /// Errors raised by the bus are **not** flattened into one error
    /// code: the bus's own family / scenario code is carried through
    /// (only the message gains the address context).  That is what
    /// lets the ISS tell an **unmapped address** (`E_BUS-01`) from
    /// every other bus error.
    ///
    /// ⚠️ An unmapped **data** access is an architectural **access
    /// fault**: the load traps with `mcause = 5`, the store with
    /// `mcause = 7`, and `mtval` carries the faulting address — the
    /// guest's trap handler gets a chance to run, exactly as on the
    /// reference side (STAGE-7 adjudication G, ledger **D-99**).
    /// Every **other** bus error is propagated to the host unchanged.
    ///
    /// ⚠️ An unmapped **fetch** is an architectural **access fault**
    /// too: the fetch traps with `mcause = 1`, `mtval` carries the
    /// faulting fetch address and `mepc` carries the same address (the
    /// instruction that could not be fetched) — exactly as the
    /// reference side reports it (STAGE-10 adjudication B, ledger
    /// **D-103**).  Every **other** bus error is propagated to the host
    /// unchanged.
    pub fn step_one_via_bus(
        &mut self,
        state: &mut SimState,
        bus: &mut Bus,
    ) -> Result<StepOutcome, PlatformError> {
        // ⚠️ **先清零**：每条指令只对应**一次**回报 —— 平台层必须**紧接着**
        //    那一步取走它（[`Iss::take_last_csr_written`]）。
        self.last_csr_written = None;
        if state.pc & 0x1 != 0 {
            // Misaligned instruction address: the alignment check
            // precedes the fetch itself (LLD §3.1, 评审 13-3).
            //
            // ⚠️ The cause is **0** (instruction address misaligned) —
            // see the note in [`Iss::step_one`].
            let cause = TrapCause::InstructionAddressMisaligned;
            enter_trap(state, cause.mcause(), state.pc);
            return Ok(StepOutcome::Trapped { cause });
        }
        // The fetch goes through `read_fetch_instruction`, not `read`: an
        // instruction fetch is a read on the bus, but a **watchpoint is
        // about the guest's data accesses** — a read watchpoint placed
        // on a code address must not fire on every instruction fetched
        // from there (V-A 检视 · 🟠-2).
        //
        // ⚠️ `read_fetch_instruction` reads **only the bytes the
        //    instruction really occupies** (2 for a compressed one, 4
        //    otherwise) ⇒ a compressed instruction in the **last two
        //    bytes** of a mapped region is executable (ledger **D-86**),
        //    and the fetch stays **one** bus access (one event, width =
        //    the instruction width).
        // ⚠️ **取指也受 PMP 约束**（F14）—— 没有 `X` 权限 ⇒ ⇒ `mcause = 1`。
        //
        // ⚠️ 这里查的是 **4 字节**（指令的最大长度）：取指前还不知道这条指令
        // 是 2 字节还是 4 字节 ⇒ ⇒ 用**最大**窗口 ⇒ ⇒ 宁可多拒一点，也不要
        // 让一次跨边界的取指「半截通过」。
        //
        // ⚠️ 判定走的是**共享**的 [`fetch_pmp_trap`] ⇒ ⇒ 与 [`Iss::step_one`]
        // 那条路**同口径**（D-11）⇒ ⇒ 不会出现「一条路查 PMP、另一条不查」。
        if let Some(cause) = fetch_pmp_trap(state) {
            enter_trap(state, cause.mcause(), state.pc);
            return Ok(StepOutcome::Trapped { cause });
        }
        let raw = match bus.read_fetch_instruction(state.pc) {
            Ok(raw) => raw,
            Err(e) => {
                // ⚠️ **未映射 ⇒ 架构陷阱 cause 1**（STAGE-10 裁决 B）。
                //
                // The判据 is the **same** one the data path uses
                // (`is_unmapped_access`: family `Bus` **and** scenario
                // `E_BUS-01`) ⇒ "unmapped" and "any other bus error"
                // stay distinguishable.
                if !is_unmapped_access(&e) {
                    // Every **other** bus error is a host-side problem:
                    // it is propagated **unchanged** (family / scenario
                    // code intact, only the message gains the address
                    // context) and never laundered into a guest trap.
                    return Err(PlatformError::new(
                        e.family,
                        e.scenario,
                        format!("fetch at pc=0x{:08x}: {}", state.pc, e),
                    ));
                }
                let cause = TrapCause::InstructionAccessFault;
                // `mtval` = the faulting fetch address; `mepc` = the
                // same address (`enter_trap` takes `mepc` from
                // `state.pc`, which still holds the address that could
                // **not** be fetched — the PC is not advanced anywhere
                // else on this path).
                enter_trap(state, cause.mcause(), state.pc);
                return Ok(StepOutcome::Trapped { cause });
            }
        };
        let decoded = self.decoder.decode(raw, state.pc);
        let mut mem = BusMemory::new(bus);
        let mut wrote_csr = None;
        let outcome = execute_one(&mut mem, state, &decoded, self.misaligned, &mut wrote_csr);
        self.last_csr_written = wrote_csr;
        outcome
    }
}

impl Default for Iss {
    fn default() -> Self {
        Self::new(IssConfig::default())
    }
}

/// The PMP check for an **instruction fetch** — one implementation, two
/// dispatch routes.
///
/// ⚠️ **两条派发路径必须同口径**（D-11）：[`Iss::step_one`]（闭包内存口）与
/// [`Iss::step_one_via_bus`]（M-14 总线口）都走这一个函数 ⇒ ⇒ 不会出现
/// 「一条路查 PMP、另一条不查」的分裂 —— 这正是本批实测抓到的那个坑
/// （取指的检查一度只落在总线口那条路上）。
///
/// ⚠️ 查的是 **4 字节**（指令的最大长度）：取指前还不知道这条指令是 2 字节
/// 还是 4 字节 ⇒ ⇒ 用**最大**窗口 ⇒ ⇒ 宁可多拒一点，也不要让一次跨边界的
/// 取指「半截通过」。
///
/// ⚠️ 返回 `Some(cause)` ⇒ ⇒ 调用方负责**陷阱化**（`mcause` + `mtval`）。
fn fetch_pmp_trap(state: &SimState) -> Option<TrapCause> {
    if pmp::pmp_allows(state, state.pc, 4, PmpAccess::Fetch) {
        None
    } else {
        Some(TrapCause::InstructionAccessFault)
    }
}

/// Memory adapter that routes every ISS access through a live [`Bus`]
/// (M-14).
///
/// The adapter exists only to bridge the bus's `&mut self` API to the
/// executor's memory port: the inner `Executor` holds two closures
/// that would each need a `&mut Bus` at the same time, which Rust
/// cannot lend out. Holding the bus in a struct and handing the
/// executor a `&mut BusMemory` keeps the borrow exclusive while
/// letting the two routes share one execution path (D-11).
struct BusMemory<'a> {
    bus: &'a mut Bus,
}

impl<'a> BusMemory<'a> {
    fn new(bus: &'a mut Bus) -> Self {
        Self { bus }
    }
}

impl MemoryAccess for BusMemory<'_> {
    fn read(&mut self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        self.bus.read(addr, BusWidth::from(size)).map_err(|e| {
            // ⚠️ **保留总线的族与场景码**（STAGE-7 裁决 G）。
            //
            // 以前这里把总线错误**重新包成** `{Iss, 1}`，消息里留一句
            // `E_BUS-01` 的文本。那样一来，执行器就**分不出**「地址没
            // 映射」（⇒ 架构陷阱）与「其它总线错误」（⇒ 原样上抛）——
            // 两者在错误里长得一模一样，只能靠**匹配消息文本**来分，
            // 那正是台账 **D-29** 批评的做法。
            //
            // ⇒ 这里只**加地址上下文**（消息），**族与场景码原样带过来**：
            // 执行器按 `family == Bus && scenario == 1` 判「未映射」。
            PlatformError::new(
                e.family,
                e.scenario,
                format!("load at 0x{:08x}: {}", addr, e),
            )
        })
    }

    fn write(&mut self, addr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError> {
        self.bus
            .write(addr, BusWidth::from(size), value)
            .map_err(|e| {
                // 同上：**族与场景码原样带过来**（STAGE-7 裁决 G）。
                PlatformError::new(
                    e.family,
                    e.scenario,
                    format!("store at 0x{:08x}: {}", addr, e),
                )
            })
    }
}

#[cfg(test)]
mod tests_common;
#[cfg(test)]
mod tests_counters;
#[cfg(test)]
mod tests_csr;
#[cfg(test)]
mod tests_pmp;
#[cfg(test)]
mod tests_rv32c;
#[cfg(test)]
mod tests_rv32i;
#[cfg(test)]
mod tests_rv32m;
#[cfg(test)]
mod tests_smode;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m13_iss::tests_common::{enc_i, enc_s, LOAD, STORE};
    use crate::m14_bus::Bus;
    use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MEPC, CSR_MTVAL, CSR_MTVEC};
    use riscv_sim_contract::model::WriteSize;
    fn make_mem(contents: &[(u32, u32)]) -> Vec<(u32, u32)> {
        contents.to_vec()
    }

    /// Read from a tiny associative "RAM".
    fn read(mem: &[(u32, u32)], addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        for (a, v) in mem {
            if *a == addr {
                return Ok(match size {
                    WriteSize::Byte => v & 0xFF,
                    WriteSize::Half => v & 0xFFFF,
                    WriteSize::Word => *v,
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
        let outcome = iss
            .step_one(&mut state, |a, s| read(&mem, a, s), |_, _, _| Ok(()))
            .expect("step");
        assert_eq!(state.gpr[1], 42);
        assert_eq!(state.pc, 4);
        assert!(matches!(outcome, StepOutcome::Advanced));
    }

    /// Integration: ADDI with the fetch / dispatch path fully routed
    /// through `m14_bus::Bus`. Sets up a 64 KiB RAM region, plants
    /// `addi x1, x0, 42` at address 0, then `step_one_via_bus`.
    #[test]
    fn addi_via_bus_executes() {
        // addi x1, x0, 42 → opcode=0010011 rd=1 rs1=0 funct3=0 imm=42
        // imm[11:0] = 42 = 0x02A → 0b0000_0010_1010, so bits[31:20] = 0x02A
        // instruction word: 0x02A0_0093
        let program = [0x02A0_0093u32];
        let program_bytes: [u8; 4] = program
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let mut bus = Bus::new();
        bus.map_ram(0, 0x1_0000, &program_bytes).expect("map ram");
        let mut state = SimState {
            pc: 0,
            ..SimState::default()
        };
        let mut iss = Iss::default();
        let outcome = iss
            .step_one_via_bus(&mut state, &mut bus)
            .expect("step via bus");

        assert!(matches!(outcome, StepOutcome::Advanced));
        assert_eq!(state.gpr[1], 42, "x1 must hold 42 after addi");
        assert_eq!(state.pc, 4);
        // Bus recorded the fetch and contributed to ram_hits.
        let stats = bus.stats();
        assert_eq!(stats.reads, 1, "fetch was the only access");
        assert_eq!(stats.ram_hits, 1);
        assert_eq!(stats.misses, 0);
    }

    /// Integration: fetch from an unmapped PC is an **architectural
    /// access fault** (`mcause = 1`), not a host-side error.
    ///
    /// The PC must stay even — an odd PC is a misaligned fetch and is
    /// reported as a trap, not as a bus error (LLD §3.1, 评审 13-3).
    ///
    /// ⚠️ STAGE-10 裁决 B / ledger **D-103**: this used to surface as
    /// an `{Iss, 1}` error.  The reference side (Spike) traps with
    /// `mcause = 1` — measured, see
    /// `tests/golden/local/unmapped_fetch.S`.
    #[test]
    fn addi_via_bus_unmapped_pc_traps() {
        let mut bus = Bus::new();
        // No regions mapped.
        let mut state = SimState {
            pc: 0xDEAD_BEE0,
            ..SimState::default()
        };
        csr::csr_write(&mut state, CSR_MTVEC, 0x2000);
        let mut iss = Iss::default();
        let outcome = iss
            .step_one_via_bus(&mut state, &mut bus)
            .expect("an unmapped fetch is a trap, not an error");
        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::InstructionAccessFault
            }
        );
        // ⚠️ The **architectural** literals, not values derived from the
        //    implementation under test.
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 1);
        assert_eq!(csr::csr_read(&state, CSR_MTVAL), 0xDEAD_BEE0);
        assert_eq!(csr::csr_read(&state, CSR_MEPC), 0xDEAD_BEE0);
        assert_eq!(state.pc, 0x2000, "the PC goes to the trap vector");
    }

    /// Integration: a store to an unmapped address traps with
    /// `mcause = 7` on the **production** path (M-14 bus).
    ///
    /// ⚠️ This is the test that pins the **seam** the trap conversion
    /// depends on: the bus's `E_BUS-01` has to reach the executor with
    /// its family / scenario code intact.  The adapter used to
    /// re-wrap every bus error as `{Iss, 1}`, which made "unmapped"
    /// indistinguishable from every other bus error — and the only way
    /// out would have been matching the **message text** (ledger
    /// **D-29**).
    #[test]
    fn a_store_to_an_unmapped_address_traps_through_the_bus() {
        let program = [enc_s(0, 6, 5, 0b010, STORE)];
        let bytes: [u8; 4] = program
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let mut bus = Bus::new();
        bus.map_ram(0, 0x1000, &bytes).expect("map ram");
        let mut state = SimState {
            pc: 0,
            gpr: {
                let mut gpr = [0u32; 32];
                gpr[5] = 0x9000_0000; // not mapped by anybody
                gpr
            },
            ..SimState::default()
        };
        csr::csr_write(&mut state, CSR_MTVEC, 0x2000);

        let mut iss = Iss::default();
        let outcome = iss
            .step_one_via_bus(&mut state, &mut bus)
            .expect("an unmapped access is a trap, not an error");

        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::StoreAccessFault
            }
        );
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 7);
        assert_eq!(csr::csr_read(&state, CSR_MTVAL), 0x9000_0000);
        assert_eq!(
            csr::csr_read(&state, CSR_MEPC),
            0,
            "mepc is the faulting instruction"
        );
        assert_eq!(state.pc, 0x2000, "the PC goes to the trap vector");
    }

    /// Integration: a load from an unmapped address traps with
    /// `mcause = 5` on the production path.
    #[test]
    fn a_load_from_an_unmapped_address_traps_through_the_bus() {
        let program = [enc_i(0, 5, 0b010, 6, LOAD)];
        let bytes: [u8; 4] = program
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();

        let mut bus = Bus::new();
        bus.map_ram(0, 0x1000, &bytes).expect("map ram");
        let mut state = SimState {
            pc: 0,
            gpr: {
                let mut gpr = [0u32; 32];
                gpr[5] = 0x9000_0000;
                gpr
            },
            ..SimState::default()
        };
        csr::csr_write(&mut state, CSR_MTVEC, 0x2000);

        let mut iss = Iss::default();
        let outcome = iss
            .step_one_via_bus(&mut state, &mut bus)
            .expect("an unmapped access is a trap, not an error");

        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::LoadAccessFault
            }
        );
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 5);
        assert_eq!(csr::csr_read(&state, CSR_MTVAL), 0x9000_0000);
    }
}
