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

use riscv_sim_contract::csr::{
    csr_write_is_ignored, PmpAccess, CAUSE_MACHINE_ECALL, CAUSE_SUPERVISOR_ECALL, CAUSE_USER_ECALL,
    CSR_MSTATUS, MSTATUS_MIE_BIT, MSTATUS_MPIE_BIT, MSTATUS_MPP_M, MSTATUS_MPP_S,
    MSTATUS_MPP_SHIFT, MSTATUS_SIE_BIT, MSTATUS_SPIE_BIT, MSTATUS_SPP_BIT, PRIV_M, PRIV_S, PRIV_U,
};
use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{SimState, WriteSize};

use super::alu;
use super::counters;
use super::csr;
use super::decoder::Decoded;
use super::pmp;

/// Result of a single instruction execution.
///
/// ⚠️ **只列 M-13 自己能知道的事**：指令退休了、还是触发了陷阱。
///
/// ⚠️ 这里**曾经**有三个变体是**死接口**（从来没有生产者）：
/// `HostPause`（宿主暂停 —— **M-13 不可能知道**宿主要暂停）·
/// `Breakpoint`（M-08 的断点 —— **M-17 在取指前**自己判，M-13 无从知晓）·
/// `Watchpoint`（M-10 的观察点 —— 访存命中由**总线 hook** 记录，M-17 在
/// 指令返回后判）。三者已随 **STAGE-4 裁决 C** 与台账 **D-59 / D-72**
/// **删除**。删掉它们之后，这个 `match` **穷举**全部变体 ⇒ M-13 以后再加
/// 变体，M-17 会**编译不过**（而不是静默走兜底）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// Instruction retired normally; PC advanced.
    Advanced,
    /// Instruction triggered a trap (illegal / misaligned / bus error).
    Trapped { cause: TrapCause },
}

impl Default for StepOutcome {
    fn default() -> Self {
        Self::Advanced
    }
}

/// Trap reason taxonomy.
///
/// The variants are the **taxonomy**; the number the trap reports is
/// the architectural `mcause` code, and [`TrapCause::mcause`] is the
/// **only** place that maps one to the other (one constraint, one
/// home).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapCause {
    /// Illegal instruction (`mcause = 2`).
    IllegalInstruction,
    /// Misaligned **instruction** address (`mcause = 0`).
    ///
    /// The fetch address was not half-word aligned. This is a
    /// **different** cause from a misaligned data access: RISC-V
    /// numbers the three of them `0` / `4` / `6`, and one variant
    /// covering all three (the old `MisalignedAccess`) reported the
    /// wrong number for two of them (ledger **D-63**).
    InstructionAddressMisaligned,
    /// **Instruction access fault** (`mcause = 1`).
    ///
    /// The guest tried to **fetch** an instruction from an address that
    /// is **not mapped** in the memory map.  RISC-V splits the three
    /// access faults into three numbers — instruction (`1`), load (`5`)
    /// and store / AMO (`7`) — so the instruction side needs its own
    /// variant (ledger **D-103**).
    ///
    /// ⚠️ **Only an unmapped address raises this.**  A bus error that
    /// is a *configuration* mistake (a peripheral that is not wired, a
    /// region that is too large, …) is **not** a guest-visible fault:
    /// it is propagated to the host unchanged (STAGE-7 adjudication G
    /// / ledger **D-99**, STAGE-10 adjudication B).
    ///
    /// ⚠️ **`mtval` carries the faulting fetch address; `mepc` carries
    /// the PC at the trap entry** — which, on this path, **is** that
    /// same address: the instruction that caused the trap is the one
    /// that **could not be fetched**, and the PC still holds it when
    /// the trap is taken (the PC is advanced nowhere else on this
    /// path).  That is the platform's own trap-entry semantics
    /// (`mepc := state.pc`), and the reference side agrees: measured
    /// through the golden environment's own tohost pipeline, Spike
    /// reports `mcause = 1`, `mtval = 0x9000_0000` and
    /// `mepc = 0x9000_0000` for a fetch at `0x9000_0000` (see
    /// `tests/golden/local/unmapped_fetch.S`).
    InstructionAccessFault,
    /// Misaligned **load** address (`mcause = 4`).
    LoadAddressMisaligned,
    /// Misaligned **store** address (`mcause = 6`).
    StoreAddressMisaligned,
    /// **Load access fault** (`mcause = 5`).
    ///
    /// The guest loaded from an address that is **not mapped** in the
    /// memory map.  RISC-V splits the three access faults into three
    /// numbers — instruction (`1`), load (`5`) and store / AMO (`7`) —
    /// and a single "access fault" variant would report the wrong
    /// number for two of the three (the same split that ledger
    /// **D-63** forced for the misaligned causes).
    ///
    /// ⚠️ **Only an unmapped address raises this.**  A bus error that
    /// is a *configuration* mistake (a region that is too large, a
    /// peripheral that is not wired, …) is **not** a guest-visible
    /// fault: it is propagated to the host unchanged (STAGE-7
    /// adjudication G / ledger **D-99**).
    LoadAccessFault,
    /// **Store access fault** (`mcause = 7`).
    ///
    /// The guest stored to an address that is **not mapped** in the
    /// memory map.  Same scope as [`TrapCause::LoadAccessFault`]: only
    /// an unmapped address traps; every other bus error is propagated
    /// to the host unchanged.
    StoreAccessFault,
    /// Out-of-range physical address.
    OutOfRange,
    /// Environment call from M-mode (`mcause = 11`).
    ///
    /// ⚠️ **cause 的编号按当前特权级而定**（STAGE-22 裁决 K / **F19**）：
    /// M ⇒ **11**，S ⇒ **9**，U ⇒ **8** ⇒ ⇒ 写 `mcause` 的那一步走
    /// [`ecall_cause`]（**唯一**那张按级的表）⇒ ⇒ 这里给出的 **11** 是
    /// **M 模式**那一个。
    ///
    /// ⚠️ **工具链把 `scall` 汇编成 `ecall`**（**F32**）⇒ ⇒ **不需要**新的
    /// `SCALL` 操作码 ⇒ ⇒ 只需要**按当前特权级选 cause**。
    Ecall,
    /// Guest `EBREAK` (`mcause = 3`). Distinct from a *host* breakpoint
    /// inserted by M-08 (which M-17 checks **before** the fetch, so it
    /// never reaches the ISS at all).
    Breakpoint,
    /// Reserved for a future cause.
    ///
    /// **Not raised by RV32M**: a division by zero and the
    /// `INT_MIN / -1` overflow are architecturally *defined results*,
    /// not traps (LLD §3.6(11)), so the M extension never reports
    /// this.  The variant is kept so the taxonomy does not have to
    /// change shape when a genuine divide-by-zero cause does land.
    ///
    /// ⚠️ It is **not** removed by this batch (ledger **D-66** owns
    /// that decision): removing a public variant and adding the CSR
    /// instruction family in the same batch would mix two changes and
    /// dilute the discriminating power of both.
    IntegerDivideByZero,
}

impl TrapCause {
    /// The architectural `mcause` code of this cause.
    ///
    /// This is the **single** mapping from the taxonomy to the number
    /// the guest reads out of `mcause` — the exception path, the
    /// interrupt path and the tests all go through it, so the number
    /// cannot drift away from the variant.
    ///
    /// The codes are the RISC-V exception codes:
    /// `0` instruction address misaligned · `1` instruction access fault ·
    /// `2` illegal instruction · `3` breakpoint · `4` load address
    /// misaligned · `5` load access fault · `6` store address misaligned ·
    /// `7` store / AMO access fault · `8` environment call from U-mode ·
    /// `9` environment call from S-mode · `11` environment call from M-mode.
    ///
    /// ⚠️ The two **dead** variants have **no architectural code**:
    ///
    /// * `OutOfRange` — RISC-V splits "the access was not allowed"
    ///   into three access faults (instruction / load / store), and the
    ///   variant does not say which one it means. It has **no
    ///   producer** today — **all three** access faults are traps now
    ///   (`InstructionAccessFault` = 1 / `LoadAccessFault` = 5 /
    ///   `StoreAccessFault` = 7, v0.22 + v0.25 批) — so its number is a
    ///   **placeholder** and is unobservable;
    /// * `IntegerDivideByZero` — RISC-V defines a division by zero as
    ///   an **architectural result**, not an exception, so there is no
    ///   code for it at all. Also a **dead** variant (ledger
    ///   **D-66**).
    ///
    /// Both use the **custom** range (`mcause >= 16`), which the
    /// RISC-V manual reserves for platform-specific causes.
    pub fn mcause(self) -> u32 {
        match self {
            TrapCause::InstructionAddressMisaligned => 0,
            TrapCause::InstructionAccessFault => 1,
            TrapCause::IllegalInstruction => 2,
            TrapCause::Breakpoint => 3,
            TrapCause::LoadAddressMisaligned => 4,
            TrapCause::LoadAccessFault => 5,
            TrapCause::StoreAddressMisaligned => 6,
            TrapCause::StoreAccessFault => 7,
            TrapCause::Ecall => 11,
            TrapCause::OutOfRange => 16,
            TrapCause::IntegerDivideByZero => 17,
        }
    }
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
    misaligned: MisalignedPolicy,
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
            misaligned: MisalignedPolicy::default(),
        }
    }

    /// Select the policy for misaligned data accesses.
    pub fn with_misaligned_policy(mut self, policy: MisalignedPolicy) -> Self {
        self.misaligned = policy;
        self
    }

    /// Execute one decoded instruction, mutating the [`SimState`].
    pub fn execute(
        &mut self,
        state: &mut SimState,
        d: &Decoded,
    ) -> Result<StepOutcome, PlatformError> {
        let misaligned = self.misaligned;
        let mut wrote_csr = None;
        execute_one(self, state, d, misaligned, &mut wrote_csr)
    }

    /// Execute one decoded instruction **and report** which CSR it
    /// wrote (`None` = it wrote none).
    ///
    /// This is the variant the platform layer uses: the ISS *reports*
    /// what it did, and the platform decides what a write to a platform
    /// CSR *means* (STAGE-4 §4 接缝 #4).
    pub fn execute_with_report(
        &mut self,
        state: &mut SimState,
        d: &Decoded,
        wrote_csr: &mut Option<u16>,
    ) -> Result<StepOutcome, PlatformError> {
        let misaligned = self.misaligned;
        execute_one(self, state, d, misaligned, wrote_csr)
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

/// Policy for misaligned data accesses.
///
/// The RISC-V spec lets an implementation either raise an
/// address-misaligned exception or perform the access. LLD §3.6(2) and
/// §5.2.1 both ask for the Spike-aligned behaviour (perform it) and
/// say so explicitly — "默认不触发（模拟器与 Spike 一致允许非对齐）"
/// / "不触发，保持 golden 兼容". The default therefore **performs**
/// the access.
///
/// **"与 Spike 一致" needs the Zicclsm qualifier.** Spike only performs
/// misaligned accesses when the Zicclsm extension is enabled; a stock
/// `spike --isa=rv32imc` traps on them. See the
/// [`MisalignedPolicy::Allow`] variant docs for the exact mechanism and
/// the ledger entry **D-42** that tracks the wording fix.
///
/// [`MisalignedPolicy::Trap`] stays available as a strict diagnostic
/// mode: a debugger that wants unaligned accesses surfaced rather than
/// silently performed can opt into it. It is *not* the default because
/// a default of `Trap` would diverge from Spike on any program that
/// touches an unaligned address, which is exactly what the golden
/// comparison (Wave 2) measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MisalignedPolicy {
    /// Perform the access anyway (LLD default).
    ///
    /// **"Spike-compatible" needs a qualifier.** Spike only *performs*
    /// misaligned accesses when the **Zicclsm** extension is enabled —
    /// `riscv/mmu.cc` does
    /// `if (!is_misaligned_enabled()) throw trap_load_address_misaligned(...)`
    /// and `is_misaligned_enabled()` is
    /// `proc->extension_enabled(EXT_ZICCLSM)`.  A stock
    /// `spike --isa=rv32imc` therefore **traps** on a misaligned access,
    /// while `spike --isa=rv32imc_zicclsm` performs it.
    ///
    /// This default matches the **Zicclsm-enabled** Spike, which is what
    /// the golden comparison pins (`tools/golden-run.sh`, ISA
    /// `rv32imc_zicsr_zicclsm_zifencei_zicntr`) — see
    /// `tests/golden/README.md`.
    /// The wording in LLD §3.6(2) / §5.2.1 ("与 Spike 一致允许非对齐")
    /// is only true under that qualifier; the ledger entry **D-42**
    /// tracks the wording fix.
    #[default]
    Allow,
    /// Raise a misaligned-access trap (strict diagnostic mode).
    Trap,
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
    /// MUL — low 32 bits of the product (M extension).
    Mul,
    /// MULH — high 32 bits of the signed product (M extension).
    Mulh,
    /// MULHSU — high 32 bits of the signed × unsigned product (M
    /// extension).
    Mulhsu,
    /// MULHU — high 32 bits of the unsigned product (M extension).
    Mulhu,
    /// DIV — signed division (M extension).
    Div,
    /// DIVU — unsigned division (M extension).
    Divu,
    /// REM — signed remainder (M extension).
    Rem,
    /// REMU — unsigned remainder (M extension).
    Remu,
    /// LB — load byte, sign-extended.
    Lb,
    /// LH — load halfword, sign-extended.
    Lh,
    /// LW — load word.
    Lw,
    /// LBU — load byte, zero-extended.
    Lbu,
    /// LHU — load halfword, zero-extended.
    Lhu,
    /// SB — store byte.
    Sb,
    /// SH — store halfword.
    Sh,
    /// SW — store word.
    Sw,
    /// BEQ — branch if equal.
    Beq,
    /// BNE — branch if not equal.
    Bne,
    /// BLT — branch if less than, signed.
    Blt,
    /// BGE — branch if greater than or equal, signed.
    Bge,
    /// BLTU — branch if less than, unsigned.
    Bltu,
    /// BGEU — branch if greater than or equal, unsigned.
    Bgeu,
    /// JAL — jump and link.
    Jal,
    /// JALR — jump and link, register.
    Jalr,
    /// FENCE — memory ordering fence (a no-op here).
    Fence,
    /// FENCE.I — instruction fetch fence (a no-op here).
    FenceI,
    /// ECALL — environment call.
    Ecall,
    /// EBREAK — breakpoint.
    Ebreak,
    /// CSRRW — swap a CSR with a register.
    Csrrw,
    /// CSRRS — set the bits of a CSR from a register.
    Csrrs,
    /// CSRRC — clear the bits of a CSR from a register.
    Csrrc,
    /// CSRRWI — swap a CSR with a 5-bit immediate.
    Csrrwi,
    /// CSRRSI — set the bits of a CSR with a 5-bit immediate.
    Csrrsi,
    /// CSRRCI — clear the bits of a CSR with a 5-bit immediate.
    Csrrci,
    /// MRET — return from a machine-mode trap handler.
    Mret,
    /// SRET — return from a supervisor-mode trap handler.
    ///
    /// ⚠️ **仅 S 模式**（STAGE-22 裁决 J）—— 在 M / U 模式 ⇒ ⇒ **cause 2**
    /// （非法指令）。
    Sret,
    /// WFI — wait for interrupt (`0x10500073`).
    ///
    /// ⚠️ **本平台把它实现成 nop**（退休、PC 前进、无副作用、无陷阱）——
    /// 口径与理由写在**执行臂**上（见 `Op::Wfi` 的执行处）。
    Wfi,
}

/// Second-level decode: map a decode key onto a concrete operation.
///
/// `None` means the key is not a legal RV32I encoding — the caller
/// turns that into an illegal-instruction trap (LLD §3.2: a key that
/// misses the table is an illegal instruction).
pub fn classify(d: &Decoded) -> Option<Op> {
    if d.illegal {
        // A reserved compressed encoding has no 32-bit equivalent, so
        // it never resolves to an operation: it is an illegal
        // instruction, exactly like an unknown 32-bit opcode.
        return None;
    }
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
            // RV32M: funct7 = 0000001 selects the multiply / divide
            // group (LLD §3.4).  The funct3 field alone is not enough
            // to tell MUL from MULH — the whole group shares it.
            (0b000, 0b000_0001) => Some(Op::Mul),
            (0b001, 0b000_0001) => Some(Op::Mulh),
            (0b010, 0b000_0001) => Some(Op::Mulhsu),
            (0b011, 0b000_0001) => Some(Op::Mulhu),
            (0b100, 0b000_0001) => Some(Op::Div),
            (0b101, 0b000_0001) => Some(Op::Divu),
            (0b110, 0b000_0001) => Some(Op::Rem),
            (0b111, 0b000_0001) => Some(Op::Remu),
            _ => None,
        },
        // LOAD.
        0b000_0011 => match d.funct3 {
            0b000 => Some(Op::Lb),
            0b001 => Some(Op::Lh),
            0b010 => Some(Op::Lw),
            0b100 => Some(Op::Lbu),
            0b101 => Some(Op::Lhu),
            _ => None,
        },
        // STORE.
        0b010_0011 => match d.funct3 {
            0b000 => Some(Op::Sb),
            0b001 => Some(Op::Sh),
            0b010 => Some(Op::Sw),
            _ => None,
        },
        // BRANCH.
        0b110_0011 => match d.funct3 {
            0b000 => Some(Op::Beq),
            0b001 => Some(Op::Bne),
            0b100 => Some(Op::Blt),
            0b101 => Some(Op::Bge),
            0b110 => Some(Op::Bltu),
            0b111 => Some(Op::Bgeu),
            _ => None,
        },
        // JAL — the J-type format has no funct3 field, bits 14:12
        // belong to the immediate.
        0b110_1111 => Some(Op::Jal),
        // JALR.
        0b110_0111 => (d.funct3 == 0b000).then_some(Op::Jalr),
        // MISC-MEM: FENCE / FENCE.I.
        0b000_1111 => match d.funct3 {
            0b000 => Some(Op::Fence),
            0b001 => Some(Op::FenceI),
            _ => None,
        },
        // SYSTEM: ECALL / EBREAK are selected by the 12-bit immediate
        // **and only when funct3 is zero**.
        //
        // The RISC-V unprivileged spec reserves `funct3 == 0b000` in
        // this opcode for ECALL / EBREAK; every other funct3 is a CSR
        // instruction (CSRRW / CSRRS / CSRRC and their immediate
        // variants CSRRWI / CSRRSI / CSRRCI). Selecting on the
        // immediate alone would misroute a CSR-shaped encoding such
        // as `0x00001073` (csr = 0, funct3 = 0b001) into the ECALL
        // handler and report mcause 11 instead of 2 — the trap would
        // reach the wrong handler as soon as CSR support lands.
        //
        // `funct3 == 0b000` with an immediate other than 0 / 1 is the
        // **privileged** part of the opcode: `0x302` is MRET (the
        // machine-mode trap return), `0x105` is WFI, `0x102` is SRET.
        // ⚠️ **SRET 是 v0.38 新解码的**（STAGE-22 订正 8）—— 之前
        // `0x102` 落在「非法」那一档 ⇒ ⇒ 现在它是一条**真的**指令。
        0b111_0011 => match (d.funct3, d.imm) {
            (0b000, 0) => Some(Op::Ecall),
            (0b000, 1) => Some(Op::Ebreak),
            // MRET — the standard encoding is `0x3020_0073`, i.e. the
            // 12-bit immediate field holds 0x302.
            (0b000, 0x302) => Some(Op::Mret),
            // SRET — the standard encoding is `0x1020_0073`, i.e. the
            // 12-bit immediate field holds 0x102.
            (0b000, 0x102) => Some(Op::Sret),
            // WFI — the standard encoding is `0x1050_0073`, i.e. the
            // 12-bit immediate field holds 0x105.
            (0b000, 0x105) => Some(Op::Wfi),
            // The CSR instruction family (funct3 001..111; 0b100 is
            // reserved and stays illegal).
            (0b001, _) => Some(Op::Csrrw),
            (0b010, _) => Some(Op::Csrrs),
            (0b011, _) => Some(Op::Csrrc),
            (0b101, _) => Some(Op::Csrrwi),
            (0b110, _) => Some(Op::Csrrsi),
            (0b111, _) => Some(Op::Csrrci),
            _ => None,
        },
        _ => None,
    }
}

/// Evaluate a branch condition against its two source registers.
fn branch_taken(op: Op, lhs: u32, rhs: u32) -> bool {
    match op {
        Op::Beq => lhs == rhs,
        Op::Bne => lhs != rhs,
        Op::Blt => (lhs as i32) < (rhs as i32),
        Op::Bge => (lhs as i32) >= (rhs as i32),
        Op::Bltu => lhs < rhs,
        Op::Bgeu => lhs >= rhs,
        // Only branch operations reach this function.
        _ => false,
    }
}

/// Access width of a load instruction.
fn load_width(op: Op) -> WriteSize {
    match op {
        Op::Lb | Op::Lbu => WriteSize::Byte,
        Op::Lh | Op::Lhu => WriteSize::Half,
        _ => WriteSize::Word,
    }
}

/// Value a load writes into the destination register: sign- or
/// zero-extended to 32 bits.
fn extend_load(op: Op, raw: u32) -> u32 {
    match op {
        Op::Lb => alu::sext8(raw),
        Op::Lbu => alu::zext8(raw),
        Op::Lh => alu::sext16(raw),
        Op::Lhu => alu::zext16(raw),
        _ => raw,
    }
}

/// Access width of a store instruction.
fn store_width(op: Op) -> WriteSize {
    match op {
        Op::Sb => WriteSize::Byte,
        Op::Sh => WriteSize::Half,
        _ => WriteSize::Word,
    }
}

/// Value a store writes: the low bytes of the source register.
///
/// The ISS masks the source register down to the access width
/// **before** handing the value to the bus. That is the
/// architectural rule (`sb` writes the low 8 bits, `sh` the low 16)
/// and the only way the bus can stay consistent between its three
/// write paths:
///
/// - the RAM path masks per byte (`Bus::write_bytes`),
/// - the peripheral path forwards the value unchanged together with
///   the access width, and the M-15 models only mask where their
///   register layout requires it (`GPIO`'s pin mask is `u32::MAX`),
/// - the cross-region split path in the M-14 LLD (§3.2) already
///   slices the value byte-wise.
///
/// Masking here makes all three observe the same 8/16-bit value, and
/// makes the value the M-18 bus hook records identical to the value
/// the target actually received.
///
/// Review V1 defect 4 flagged this mask as redundant because
/// removing it left every test green — that was a coverage gap (no
/// test asserted what a sub-word store delivers to a peripheral), not
/// evidence of redundancy.
fn truncate_store(op: Op, value: u32) -> u32 {
    match op {
        Op::Sb => alu::zext8(value),
        Op::Sh => alu::zext16(value),
        _ => value,
    }
}

/// Whether an address satisfies the natural alignment of the access.
fn is_aligned(addr: u32, size: WriteSize) -> bool {
    match size {
        WriteSize::Byte => true,
        WriteSize::Half => addr & 0x1 == 0,
        WriteSize::Word => addr & 0x3 == 0,
    }
}

/// The bus scenario code for "**the address is not mapped**" (`E_BUS-01`).
///
/// M-14's error table says it verbatim:
/// `E_BUS-01 unmapped → PlatformError{Bus, 1}`.
///
/// ⚠️ The number is spelled **here** because M-14 has no named constant
/// for `E_BUS-01` (it has them for `03` / `04` / `06` / `09` / `10` /
/// `11` / `12`).  Writing the literal in one place — with the source
/// of truth named in the comment — is better than a second spelling of
/// the code scattered over the tree ("one code, two truths").
const E_BUS_01_UNMAPPED: u16 = 1;

/// Whether a failed memory access means "**the address is not mapped**".
///
/// ⚠️ The判据 is the error's **family + scenario code** — never its
/// message text.  Matching on the message is the mistake ledger
/// **D-29** calls out: a message is a human-readable string, not a
/// contract, and it changes when somebody rewords it.
///
/// ⚠️ Only `E_BUS-01` qualifies.  A bus error that is a
/// **configuration** mistake (a region that is too large, a peripheral
/// that is not wired, a size mismatch) is a *host-side* problem: it is
/// propagated unchanged and never becomes a guest trap (STAGE-7
/// adjudication G).
///
/// ⚠️ **`pub(crate)`** (STAGE-10 裁决 B): the **fetch** path needs the
/// **same**判据 as the data path — "unmapped" and "any other bus error"
/// must stay distinguishable on both sides, or an unmapped fetch could
/// not become the architectural instruction access fault (`mcause = 1`)
/// without also swallowing the host-side errors.  Only the **visibility**
/// changed; the判据 itself is untouched.
pub(crate) fn is_unmapped_access(err: &PlatformError) -> bool {
    err.family == ErrorFamily::Bus && err.scenario == E_BUS_01_UNMAPPED
}

/// PC after an instruction that falls through to the next one.
///
/// The step is the **decoded instruction length**, not a hard-coded
/// 4: a decoder that always reports 4 makes every compressed (RV32C)
/// instruction land on the wrong half-word boundary (D-16). The
/// `Decoded::len` field exists precisely so the PC can advance by 2
/// once the C extension lands.
///
/// `pub(crate)` so the module tests can pin the behaviour: a
/// hard-coded `+ 4` here would silently break every compressed
/// instruction (review V1 defect 2).
pub(crate) fn sequential(d: &Decoded) -> u32 {
    d.pc.wrapping_add(d.len as u32)
}

/// PC after a taken branch: the branch target is relative to the PC of
/// the branch instruction itself.
fn branch(d: &Decoded) -> u32 {
    d.pc.wrapping_add(d.imm as u32)
}

/// The architectural `mcause` code of an **environment call** taken from
/// `level`.
///
/// ⚠️ **cause 按级**（STAGE-22 裁决 K / **F19**）：M ⇒ **11**，S ⇒ **9**，
/// U ⇒ **8**。
///
/// ⚠️ **数字只有一个落点** —— 契约层的 `CAUSE_MACHINE_ECALL` /
/// `CAUSE_SUPERVISOR_ECALL` / `CAUSE_USER_ECALL` ⇒ ⇒ 这里**只引用**，
/// **不**另写一份字面量。
///
/// ⚠️ **为什么它不是 `TrapCause` 的一个变体**：变体表是**架构分类**
/// （「环境调用」就是「环境调用」），**编号**才是按级的那一份 ⇒ ⇒ 两件事
/// 分处两处 ⇒ ⇒ 变体表**不变**（⇒ ⇒ M-13 的集成门禁
/// `m13_fetch_path::the_cause_1_mapping_lives_in_exactly_one_place` 钉住
/// 的「变体 → 编号」那张表的**行数**也不变）。
fn ecall_cause(level: u32) -> u32 {
    match level {
        PRIV_M => CAUSE_MACHINE_ECALL,
        PRIV_S => CAUSE_SUPERVISOR_ECALL,
        _ => CAUSE_USER_ECALL,
    }
}

/// Is this operation one of the **six CSR instructions**?
///
/// ⚠️ **唯一**的「这条指令要不要走按级检查」判据 ⇒ ⇒ 六条指令**共用**
/// 那一处检查（读与写**同形**，**F26**）。
fn is_csr_op(op: Op) -> bool {
    matches!(
        op,
        Op::Csrrw | Op::Csrrs | Op::Csrrc | Op::Csrrwi | Op::Csrrsi | Op::Csrrci
    )
}

/// The single execution path shared by every dispatch route.
///
/// `mem` is the memory port: it is threaded through here so that both
/// dispatch routes are structurally identical, and so that the load /
/// store instructions land on one code path rather than two (D-11).
///
/// `wrote_csr` receives the **CSR number this instruction wrote** (`None`
/// = it wrote no CSR). The ISS itself has **no** side effects of its own:
/// it only *reports* what it did, and the platform layer (M-17) decides
/// what a write to a platform CSR like `irq_clear` *means* (STAGE-4 §4
/// 接缝 #4).
///
/// ⚠️ The report is produced by the **write arms themselves**, so the
/// "does this instruction write?" decision has exactly **one** home —
/// there is no second copy of it to drift.
///
/// # The retirement accounting lives **here**
///
/// ⚠️ **自增的唯一落点** = 本函数的**正常完成出口**（STAGE-20 §3.2 /
/// 裁决 F）—— 陷阱分支全部**提前 `return`** ⇒ ⇒ 陷阱指令**不**退休
/// （裁决 G）⇒ ⇒ CSR 指令读到的值天然是**退休前**的值。
///
/// ⚠️ `wrote_csr` 同时是**抑制**的输入：写计数器的那条指令**自己**不 +1
/// （裁决 H）⇒ ⇒ **零新增状态、零新增参数**。
pub fn execute_one<M: MemoryAccess>(
    mem: &mut M,
    state: &mut SimState,
    d: &Decoded,
    misaligned: MisalignedPolicy,
    wrote_csr: &mut Option<u16>,
) -> Result<StepOutcome, PlatformError> {
    let op = match classify(d) {
        Some(op) => op,
        None => {
            // An encoding the ISS cannot execute is an illegal
            // instruction, not a platform failure: both dispatch routes
            // report it identically (D-11). The engine turns this into
            // mcause = 2 once trap delivery is wired (LLD §5.2.4).
            //
            // A reserved *compressed* encoding lands here too — the
            // decoder flags it and `classify` refuses it, so the
            // compressed path reports illegal instructions through the
            // same door as the 32-bit one.
            //
            // `mtval` carries the **instruction word**: the 32-bit
            // encoding, or the **16-bit original code** of a
            // compressed encoding (STAGE-4 §3.5).
            let cause = TrapCause::IllegalInstruction;
            let mtval = if d.compressed {
                d.compressed_raw as u32
            } else {
                d.raw
            };
            csr::enter_trap(state, cause.mcause(), mtval);
            return Ok(StepOutcome::Trapped { cause });
        }
    };

    // ⚠️ **CSR 指令的按级检查**（STAGE-22 裁决 H / 订正 5）—— **六条** CSR
    // 指令（`csrrw` / `csrrs` / `csrrc` / `csrrwi` / `csrrsi` / `csrrci`）
    // **共用**这一处 ⇒ ⇒ 读与写**同形**（**F26**：S 模式**写** M 模式 CSR
    // 也是 **cause 2**，不是「被忽略」）。
    //
    // ⚠️ **判据只有一条**：当前级别能不能访问这个 CSR ⇒ ⇒ **不实现**的 CSR
    // 在 M 模式**照旧**（读回 0 / 写被忽略，台账 **D-92**）⇒ ⇒ **M 模式
    // 语料零回归**。
    if is_csr_op(op) {
        let number = csr::csr_number(d.raw);
        if !csr::csr_access_allowed(state, number) {
            let cause = TrapCause::IllegalInstruction;
            // ⚠️ `mtval` 携带**指令字**（与「解码不出来」那条路径同口径）。
            csr::enter_trap(state, cause.mcause(), d.raw);
            return Ok(StepOutcome::Trapped { cause });
        }
    }

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
        Op::Mul => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::mul(lhs, rhs));
            sequential(d)
        }
        Op::Mulh => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::mulh(lhs, rhs));
            sequential(d)
        }
        Op::Mulhsu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::mulhsu(lhs, rhs));
            sequential(d)
        }
        Op::Mulhu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::mulhu(lhs, rhs));
            sequential(d)
        }
        Op::Div => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::div(lhs, rhs));
            sequential(d)
        }
        Op::Divu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::divu(lhs, rhs));
            sequential(d)
        }
        Op::Rem => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::rem(lhs, rhs));
            sequential(d)
        }
        Op::Remu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            write_gpr(state, d.rd, alu::remu(lhs, rhs));
            sequential(d)
        }
        Op::Lb | Op::Lbu | Op::Lh | Op::Lhu | Op::Lw => {
            let addr = read_gpr(state, d.rs1).wrapping_add(d.imm as u32);
            let size = load_width(op);
            if misaligned == MisalignedPolicy::Trap && !is_aligned(addr, size) {
                // A misaligned **load** is `mcause = 4` — not the same
                // code as a misaligned fetch (`0`) or store (`6`).
                let cause = TrapCause::LoadAddressMisaligned;
                csr::enter_trap(state, cause.mcause(), addr);
                return Ok(StepOutcome::Trapped { cause });
            }
            // ⚠️ **PMP 在未映射检查之前**（裁决 K）—— 规范把 PMP 放在物理
            //    地址一级 ⇒ ⇒ 被 PMP 拒的访问根本不会走到总线。
            if !pmp::pmp_allows(state, addr, pmp::access_bytes(size), PmpAccess::Load) {
                let cause = TrapCause::LoadAccessFault;
                csr::enter_trap(state, cause.mcause(), addr);
                return Ok(StepOutcome::Trapped { cause });
            }
            let raw = match mem.read(addr, size) {
                Ok(raw) => raw,
                Err(err) => {
                    // An unmapped address is a **guest-visible access
                    // fault**, not a platform failure: the guest's trap
                    // handler has to get a chance to run — that is what
                    // the reference side does, and the divergence was
                    // ledger **D-99** (STAGE-7 adjudication G).
                    //
                    // ⚠️ **Only** `E_BUS-01` lands here.  Every other
                    // bus error is a host-side problem and is
                    // propagated **unchanged** — it is not the guest's
                    // fault and must not be laundered into a trap.
                    if !is_unmapped_access(&err) {
                        return Err(err);
                    }
                    let cause = TrapCause::LoadAccessFault;
                    // `mtval` carries the **faulting address** — the
                    // manual's rule for access faults.
                    //
                    // ⚠️ `enter_trap` takes `mepc` from `state.pc`,
                    // which still holds the faulting instruction's
                    // address (the PC is only advanced at the *end* of
                    // `execute_one`) ⇒ **the PC does not advance** and
                    // the handler returns to the faulting instruction.
                    csr::enter_trap(state, cause.mcause(), addr);
                    return Ok(StepOutcome::Trapped { cause });
                }
            };
            write_gpr(state, d.rd, extend_load(op, raw));
            sequential(d)
        }
        Op::Sb | Op::Sh | Op::Sw => {
            let addr = read_gpr(state, d.rs1).wrapping_add(d.imm as u32);
            let size = store_width(op);
            if misaligned == MisalignedPolicy::Trap && !is_aligned(addr, size) {
                // A misaligned **store** is `mcause = 6`.
                let cause = TrapCause::StoreAddressMisaligned;
                csr::enter_trap(state, cause.mcause(), addr);
                return Ok(StepOutcome::Trapped { cause });
            }
            // ⚠️ **PMP 在未映射检查之前**（裁决 K）。
            if !pmp::pmp_allows(state, addr, pmp::access_bytes(size), PmpAccess::Store) {
                let cause = TrapCause::StoreAccessFault;
                csr::enter_trap(state, cause.mcause(), addr);
                return Ok(StepOutcome::Trapped { cause });
            }
            let value = truncate_store(op, read_gpr(state, d.rs2));
            match mem.write(addr, size, value) {
                Ok(()) => {}
                Err(err) => {
                    // Same rule as the load arm: **only** an unmapped
                    // address becomes a trap; every other bus error is
                    // propagated unchanged.
                    if !is_unmapped_access(&err) {
                        return Err(err);
                    }
                    let cause = TrapCause::StoreAccessFault;
                    // `mtval` = the faulting address; `mepc` = the
                    // faulting instruction (the PC does not advance).
                    csr::enter_trap(state, cause.mcause(), addr);
                    return Ok(StepOutcome::Trapped { cause });
                }
            }
            sequential(d)
        }
        Op::Beq | Op::Bne | Op::Blt | Op::Bge | Op::Bltu | Op::Bgeu => {
            let lhs = read_gpr(state, d.rs1);
            let rhs = read_gpr(state, d.rs2);
            if branch_taken(op, lhs, rhs) {
                branch(d)
            } else {
                sequential(d)
            }
        }
        Op::Jal => {
            // The link register holds the address of the instruction
            // *after* the jump.
            write_gpr(state, d.rd, d.pc.wrapping_add(d.len as u32));
            branch(d)
        }
        Op::Jalr => {
            // The target is computed from rs1 before rd is written, so
            // that `jalr rd, imm(rs1)` with rd == rs1 behaves as the
            // spec requires.
            let target = read_gpr(state, d.rs1).wrapping_add(d.imm as u32) & !1u32;
            write_gpr(state, d.rd, d.pc.wrapping_add(d.len as u32));
            target
        }
        Op::Fence | Op::FenceI => {
            // The single-core model is sequentially consistent, so the
            // fences are no-ops (LLD §3.6(5)).
            sequential(d)
        }
        Op::Ecall => {
            // ⚠️ **cause 按当前特权级**（STAGE-22 裁决 K / **F19**）：
            // M ⇒ **11**，S ⇒ **9**，U ⇒ **8**。
            //
            // ⚠️ **不需要新的 `SCALL` 操作码**（**F32**）：工具链在语料的
            // march 下把 `scall` 汇编成 `ecall` ⇒ ⇒ 现有这条 `ecall` 按级
            // 选 cause 就够。
            //
            // An environment call carries **no** auxiliary value.
            let cause = TrapCause::Ecall;
            csr::enter_trap(state, ecall_cause(csr::priv_get(state)), 0);
            return Ok(StepOutcome::Trapped { cause });
        }
        Op::Ebreak => {
            // A breakpoint carries **no** auxiliary value either.
            let cause = TrapCause::Breakpoint;
            csr::enter_trap(state, cause.mcause(), 0);
            return Ok(StepOutcome::Trapped { cause });
        }
        Op::Csrrw => {
            // `rd = old`; `csr = rs1`.
            //
            // ⚠️ With `rd == x0` the manual says the CSR is **not
            // read** at all — the old value is simply dropped. Nothing
            // observable depends on it here (a CSR read has no side
            // effect), but the rule is implemented rather than
            // approximated: the read is *skipped*, not just unused.
            let number = csr::csr_number(d.raw);
            let value = read_gpr(state, d.rs1);
            if d.rd != 0 {
                let old = csr::csr_read(state, number);
                write_gpr(state, d.rd, old);
            }
            if !csr_write_is_ignored(number) {
                csr::csr_write(state, number, value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Csrrs => {
            // `rd = old`; `csr |= rs1` — but only when `rs1 != x0`:
            // with `rs1 == x0` the instruction is a **pure read** and
            // must not write the CSR at all (not even to the value it
            // already holds).
            let number = csr::csr_number(d.raw);
            let value = read_gpr(state, d.rs1);
            let old = csr::csr_read(state, number);
            write_gpr(state, d.rd, old);
            if d.rs1 != 0 && !csr_write_is_ignored(number) {
                csr::csr_write(state, number, old | value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Csrrc => {
            // `rd = old`; `csr &= !rs1` — same "no write with
            // `rs1 == x0`" rule as [`Op::Csrrs`].
            let number = csr::csr_number(d.raw);
            let value = read_gpr(state, d.rs1);
            let old = csr::csr_read(state, number);
            write_gpr(state, d.rd, old);
            if d.rs1 != 0 && !csr_write_is_ignored(number) {
                csr::csr_write(state, number, old & !value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Csrrwi => {
            // `CSRRW` with the `rs1` field read as a 5-bit immediate.
            let number = csr::csr_number(d.raw);
            let value = csr::csr_zimm(d.raw);
            if d.rd != 0 {
                let old = csr::csr_read(state, number);
                write_gpr(state, d.rd, old);
            }
            if !csr_write_is_ignored(number) {
                csr::csr_write(state, number, value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Csrrsi => {
            // `CSRRS` with the `rs1` field read as a 5-bit immediate.
            // A zero immediate means "read only" — no write.
            let number = csr::csr_number(d.raw);
            let value = csr::csr_zimm(d.raw);
            let old = csr::csr_read(state, number);
            write_gpr(state, d.rd, old);
            if value != 0 && !csr_write_is_ignored(number) {
                csr::csr_write(state, number, old | value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Csrrci => {
            // `CSRRC` with the `rs1` field read as a 5-bit immediate.
            // A zero immediate means "read only" — no write.
            let number = csr::csr_number(d.raw);
            let value = csr::csr_zimm(d.raw);
            let old = csr::csr_read(state, number);
            write_gpr(state, d.rd, old);
            if value != 0 && !csr_write_is_ignored(number) {
                csr::csr_write(state, number, old & !value);
                *wrote_csr = Some(number);
            }
            sequential(d)
        }
        Op::Wfi => {
            // ⚠️ **本平台把 `wfi` 实现成 nop** —— 这是**明示的取舍**，不是
            // 「忘了实现」。
            //
            // **架构语义**：`wfi` 是 M 模式的 `SYSTEM` 指令（编码
            // `0x10500073`），语义是「**等中断**」；但特权规范本身把它
            // 定义成一条 **hint** —— 「实现**可以**把它当成 nop」，尤其
            // 在中断被关掉（`mstatus.MIE == 0`）的时候。
            //
            // **本平台选 nop 的理由**：
            //
            // 1. **真等待需要引擎级的支持**：让 `wfi` 真的阻塞在引擎里，
            //    得改 M-12 的唯一主循环（`run_until`）与 M-17 的取指边界
            //    —— 那是**别的模块**的落点（而且要动 `InstructionExecutor`
            //    的接缝形状），不在本批的范围里。ISS 是「一条指令 = 一个
            //    纯函数」的模型，**没有**「阻塞到某条件满足」的表达。
            // 2. **ISS 这一层没有任何时间副作用**：ISS **不碰**虚拟时间
            //    （虚拟时间由 M-12 / M-17 按退休指令数推进）⇒ `wfi`
            //    在这里**不**多推时间、**不**改任何 CSR、**不**碰内存。
            //    ⚠️ 但它**照常退休** ⇒ 在引擎那一层它与一条 nop **一样**
            //    占用一条指令的时间成本（虚拟时间照常前进）。这正是
            //    「不死锁」的关键（见第 3 点），也是「idle 任务退化成忙等」
            //    的代价（见下面「代价」一段）。
            // 3. **不会引入活锁**：这条指令**照常退休**、**PC 照常前进**
            //    ⇒ 引擎的「无进展守卫」（M-12 的 `NO_PROGRESS_WINDOW`）
            //    看到的是**进展**；虚拟时间照常前进 ⇒ 定时器中断照样会
            //    到达 ⇒ guest 的 idle 循环**不会**把引擎卡死。
            //
            // **代价（如实记下）**：guest 的 idle 任务退化成**忙等** ——
            // 它省不下 CPU，也拿不到「低功耗」语义；但功能上（定时器
            // 中断最终会送达）是对的。
            //
            // ⚠️ **不要**在这里偷偷改成「真的阻塞」—— 那样会引入活锁
            // （没有挂起中断时永远不返回）。
            sequential(d)
        }
        Op::Mret => {
            // ⚠️ **`mret` 仅 M 模式**（STAGE-22 裁决 J / **F23**）：`mepc`
            // 在 S 模式**不可访问** ⇒ ⇒ S 模式**根本回不去 M 模式** ⇒ ⇒
            // 在 S / U 模式执行 `mret` ⇒ ⇒ **cause 2**（非法指令）。
            if csr::priv_get(state) != PRIV_M {
                let cause = TrapCause::IllegalInstruction;
                csr::enter_trap(state, cause.mcause(), d.raw);
                return Ok(StepOutcome::Trapped { cause });
            }
            // `pc := mepc`; `mstatus.MIE := mstatus.MPIE`;
            // `mstatus.MPIE := 1`.
            //
            // ⚠️ **`mepc` 的对齐掩码只发生在这里**（台账 D-93 / STAGE-7 裁决 I）。
            //
            // **用户写会掩码**：`csrw mepc` 走 `apply_warl`（`~1`）—— 参考侧
            // 实测：写 `0x1003` 读回 **`0x1002`**；陷入门则走
            // [`csr::csr_write_raw`] 写**精确的 PC**（不丢 bit 1，handler
            // 要能回到**出错的指令**）。
            //
            // **掩码按 IALIGN**：特权规范说 `mret` 取 PC 时按 IALIGN 清低位。
            // 本平台的 `misa` **声明支持 C 扩展** ⇒ IALIGN = 16
            // ⇒ 只清**最低 1 位**（[`csr::mepc_aligned`]），**不是**低两位。
            //
            // **参考侧实测**（Spike，同一个探针 ELF 两侧跑）：一条位于
            // `0x8000014a` 的压缩指令取陷阱后，`mret` 落回 `4k+2`（不是 `4k`）
            // ⇒ 掩码确实是 `~1`。若这里写成 `~3`，压缩指令的陷入返回会
            // **落进指令中间**。
            let target = csr::mepc_aligned(state);
            let mstatus = csr::csr_read(state, CSR_MSTATUS);
            let mie = 1u32 << MSTATUS_MIE_BIT;
            let mpie = 1u32 << MSTATUS_MPIE_BIT;
            let mpp_mask = 0x3u32 << MSTATUS_MPP_SHIFT;
            // ⚠️ **特权级来自「旧的 `MPP`」** ⇒ ⇒ **先读旧值再清** ⇒ ⇒
            // `MPP := U` 的那一步**不能**影响这一次读取（STAGE-22 裁决 J）。
            let old_mpp = (mstatus & mpp_mask) >> MSTATUS_MPP_SHIFT;
            let restored = if mstatus & mpie != 0 { mie } else { 0 };
            // `MIE := 旧 MPIE`；`MPIE := 1`；`MPP := U(0)`。
            csr::mstatus_bits(state, mie | mpie | mpp_mask, restored | mpie);
            // 特权级 ← 旧的 `MPP`（`M` ⇒ M、`S` ⇒ S、`U` ⇒ U）。
            let level = match old_mpp {
                MSTATUS_MPP_S => PRIV_S,
                MSTATUS_MPP_M => PRIV_M,
                _ => PRIV_U,
            };
            csr::priv_set(state, level);
            target
        }
        Op::Sret => {
            // ⚠️ **`sret` 仅 S 模式**（STAGE-22 裁决 J）—— 在 M / U 模式 ⇒ ⇒
            // **cause 2**（非法指令）。
            if csr::priv_get(state) != PRIV_S {
                let cause = TrapCause::IllegalInstruction;
                csr::enter_trap(state, cause.mcause(), d.raw);
                return Ok(StepOutcome::Trapped { cause });
            }
            // `pc := sepc`; `sstatus.SIE := sstatus.SPIE`;
            // `sstatus.SPIE := 1`; `sstatus.SPP := U(0)`.
            //
            // ⚠️ **`sepc` 的对齐掩码与 `mepc` 同口径**（[`csr::sepc_aligned`]）
            // ⇒ ⇒ 两个向量表**同规则**。
            let target = csr::sepc_aligned(state);
            let mstatus = csr::csr_read(state, CSR_MSTATUS);
            let sie = 1u32 << MSTATUS_SIE_BIT;
            let spie = 1u32 << MSTATUS_SPIE_BIT;
            let spp = 1u32 << MSTATUS_SPP_BIT;
            // ⚠️ **落点由 `SPP` 决定**（**F9**）：`SPP` = 1 ⇒ ⇒ 回到 **S
            // 模式**（陷阱处理器，**F28**）；`SPP` = 0 ⇒ ⇒ 落到 **U 模式**。
            let previous = mstatus & spp != 0;
            let restored = if mstatus & spie != 0 { sie } else { 0 };
            // `SIE := 旧 SPIE`；`SPIE := 1`；`SPP := 0`。
            //
            // ⚠️ **走 `mstatus` 的原始位操作**（[`csr::mstatus_bits`]）⇒ ⇒
            // **不受客体的 `0x22` 掩码约束** ⇒ ⇒ `SPP` 清得掉。
            csr::mstatus_bits(state, sie | spie | spp, restored | spie);
            csr::priv_set(state, if previous { PRIV_S } else { PRIV_U });
            target
        }
    };

    state.pc = next_pc;

    // ⚠️ **退休计数**（Zicntr）—— 自增的**唯一落点**（STAGE-20 §3.2 /
    // 裁决 F）。
    //
    // ⚠️ **为什么落在这里**（三条理由，逐条可验）：
    //
    // 1. **两条 dispatch 路径都经 `execute_one`** ⇒ ⇒ 闭包口与总线口
    //    **共用**这个函数 ⇒ ⇒ 不可能两条路径行为不一致（裁决 F）。
    // 2. **陷阱指令天然被排除**（裁决 G）—— 上面每一个陷阱分支都是
    //    **提前 `return`**（`Ecall` / `Ebreak` / 非法指令 / 非对齐 / 访问
    //    故障）⇒ ⇒ **到不了**这一行 ⇒ ⇒ R11 自动成立，**不需要**额外
    //    判「这是不是陷阱」。
    // 3. **读到的值 = 退休前的值**（R5）—— CSR 指令在**本函数内部**读
    //    文件 ⇒ ⇒ 读到的值是**自增前**的。
    //
    // ⚠️ **不要**把这一行搬到 [`crate::m13_iss::step_one`] /
    // [`crate::m13_iss::step_one_via_bus`] 或 `m17_backend` 里 —— 那会变成
    // **两个真相源**，而两条路径的计数就会**不一致**。
    //
    // ⚠️ **抑制**（写计数器的那条指令自己**不** +1）由
    // [`counters::retire`] 按 `wrote_csr` 判定 ⇒ ⇒ **零新增状态、零新增
    // 参数、零 DTO**（裁决 H）。
    counters::retire(state, *wrote_csr);

    Ok(StepOutcome::Advanced)
}

// ---------------------------------------------------------------------------
// Tests — D-99: an unmapped access is an architectural trap
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::m13_iss::decoder::Decoder;
    use crate::m13_iss::tests_common::{enc_i, enc_s, LOAD, STORE};
    use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MEPC, CSR_MTVAL, CSR_MTVEC};

    /// The address the tests fault on: outside every mapped region.
    const FAULT_ADDR: u32 = 0x9000_0000;

    /// The address of the faulting instruction.
    const FAULT_PC: u32 = 0x1000;

    /// The trap vector the tests install.
    const MTVEC: u32 = 0x2000;

    /// The bus error the M-14 raises for an unmapped address.
    fn unmapped() -> PlatformError {
        PlatformError::new(ErrorFamily::Bus, E_BUS_01_UNMAPPED, "unmapped")
    }

    /// A state with `mtvec` programmed and the PC on the faulting
    /// instruction.
    fn armed_state() -> SimState {
        let mut state = SimState {
            pc: FAULT_PC,
            ..SimState::default()
        };
        csr::csr_write(&mut state, CSR_MTVEC, MTVEC);
        state
    }

    /// `sw x6, 0(x5)` — a store whose address is `x5 + imm`.
    fn store_word() -> u32 {
        enc_s(0, 6, 5, 0b010, STORE)
    }

    /// `lw x6, 0(x5)` — a load whose address is `x5 + imm`.
    fn load_word() -> u32 {
        enc_i(0, 5, 0b010, 6, LOAD)
    }

    /// Execute one instruction whose every memory access fails with
    /// `err`.
    fn exec_failing(
        state: &mut SimState,
        word: u32,
        err: &PlatformError,
    ) -> Result<StepOutcome, PlatformError> {
        let d = Decoder::default().decode(word, FAULT_PC);
        let err = err.clone();
        let mut exec = Executor::new(
            |_addr, _size| Err(err.clone()),
            |_addr, _size, _value| Err(err.clone()),
        );
        exec.execute(state, &d)
    }

    /// A load from an unmapped address traps with `mcause = 5`
    /// (load access fault) and `mtval` = the faulting address.
    ///
    /// ⚠️ The **number** is asserted, not just the variant: the guest
    /// reads `mcause` and compares it against the architectural value,
    /// so a wrong number is a real divergence from the reference side.
    #[test]
    fn an_unmapped_load_traps_with_cause_five() {
        let mut state = armed_state();
        state.gpr[5] = FAULT_ADDR;

        let outcome = exec_failing(&mut state, load_word(), &unmapped()).expect("no error escapes");

        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::LoadAccessFault
            }
        );
        assert_eq!(
            TrapCause::LoadAccessFault.mcause(),
            5,
            "load access fault is mcause 5"
        );
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 5);
        assert_eq!(
            csr::csr_read(&state, CSR_MTVAL),
            FAULT_ADDR,
            "mtval must carry the faulting address"
        );
        assert_eq!(
            state.gpr[6], 0,
            "a faulting load must not write its destination register"
        );
    }

    /// A store to an unmapped address traps with `mcause = 7`
    /// (store access fault) and `mtval` = the faulting address.
    #[test]
    fn an_unmapped_store_traps_with_cause_seven() {
        let mut state = armed_state();
        state.gpr[5] = FAULT_ADDR;

        let outcome =
            exec_failing(&mut state, store_word(), &unmapped()).expect("no error escapes");

        assert_eq!(
            outcome,
            StepOutcome::Trapped {
                cause: TrapCause::StoreAccessFault
            }
        );
        assert_eq!(
            TrapCause::StoreAccessFault.mcause(),
            7,
            "store access fault is mcause 7"
        );
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 7);
        assert_eq!(
            csr::csr_read(&state, CSR_MTVAL),
            FAULT_ADDR,
            "mtval must carry the faulting address"
        );
    }

    /// The load and the store must report **different** numbers.
    ///
    /// ⚠️ This is the whole point of the split: one "access fault"
    /// variant would make one of the two sides diverge from the
    /// reference (the same class of defect ledger **D-63** forced the
    /// misaligned causes apart for).
    #[test]
    fn load_and_store_access_faults_have_different_numbers() {
        assert_eq!(TrapCause::LoadAccessFault.mcause(), 5);
        assert_eq!(TrapCause::StoreAccessFault.mcause(), 7);
        assert_ne!(
            TrapCause::LoadAccessFault.mcause(),
            TrapCause::StoreAccessFault.mcause()
        );
    }

    /// The PC does **not** advance when the access faults: `mepc` is
    /// the faulting instruction's address, and the PC is redirected to
    /// the trap vector.
    ///
    /// ⚠️ If the PC advanced, the guest's handler would return to the
    /// instruction *after* the faulting one and never re-execute it —
    /// the architectural rule is "the handler returns to the faulting
    /// instruction" (STAGE-7 adjudication G).
    #[test]
    fn a_faulting_access_does_not_advance_the_pc() {
        let mut state = armed_state();
        state.gpr[5] = FAULT_ADDR;

        exec_failing(&mut state, store_word(), &unmapped()).expect("no error escapes");

        assert_eq!(
            csr::csr_read(&state, CSR_MEPC),
            FAULT_PC,
            "mepc must be the address of the faulting instruction"
        );
        assert_ne!(
            csr::csr_read(&state, CSR_MEPC),
            FAULT_PC + 4,
            "mepc must not be the next instruction"
        );
        assert_eq!(
            state.pc, MTVEC,
            "the PC must be redirected to the trap vector"
        );
    }

    /// Another bus error is **not** laundered into a trap: it is
    /// propagated to the host unchanged (STAGE-7 adjudication G).
    ///
    /// ⚠️ `E_BUS-04` is "the region runs past the end of the bus" — a
    /// **configuration** mistake, not the guest's fault.
    #[test]
    fn another_bus_error_is_propagated_unchanged() {
        let mut state = armed_state();
        state.gpr[5] = FAULT_ADDR;
        let err = PlatformError::new(ErrorFamily::Bus, 4, "region out of range");

        let got = exec_failing(&mut state, store_word(), &err).expect_err("must not be swallowed");

        assert_eq!(got.family, ErrorFamily::Bus);
        assert_eq!(got.scenario, 4, "the original scenario code must survive");
        assert_eq!(
            csr::csr_read(&state, CSR_MCAUSE),
            0,
            "a host-side error must not enter the trap path"
        );
        assert_eq!(state.pc, FAULT_PC, "the PC must not move");
    }

    /// Only `E_BUS-01` is an access fault: an error of another family
    /// is propagated, even if its scenario code happens to be `1`.
    ///
    /// ⚠️ Without this case, a "family == Bus" check alone would be
    /// enough to pass — and an `ISS-01` error (the ISS's own family)
    /// would be turned into a guest trap.
    #[test]
    fn an_error_of_another_family_is_not_an_access_fault() {
        let mut state = armed_state();
        state.gpr[5] = FAULT_ADDR;
        let err = PlatformError::new(ErrorFamily::Iss, 1, "unmapped read");

        let got = exec_failing(&mut state, load_word(), &err).expect_err("must not be swallowed");

        assert_eq!(got.family, ErrorFamily::Iss);
        assert_eq!(got.scenario, 1);
        assert_eq!(csr::csr_read(&state, CSR_MCAUSE), 0);
    }
}
