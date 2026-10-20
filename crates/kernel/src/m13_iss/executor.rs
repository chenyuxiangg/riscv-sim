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
        execute_one(self, state, d, misaligned)
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
    /// `rv32imc_zicclsm_zifencei`) — see `tests/golden/README.md`.
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
        // The ISS does not implement CSR yet, so those encodings trap
        // as an illegal instruction (review V1 defect 1).
        0b111_0011 => match (d.funct3, d.imm) {
            (0b000, 0) => Some(Op::Ecall),
            (0b000, 1) => Some(Op::Ebreak),
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

/// The single execution path shared by every dispatch route.
///
/// `mem` is the memory port: it is threaded through here so that both
/// dispatch routes are structurally identical, and so that the load /
/// store instructions land on one code path rather than two (D-11).
pub fn execute_one<M: MemoryAccess>(
    mem: &mut M,
    state: &mut SimState,
    d: &Decoded,
    misaligned: MisalignedPolicy,
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
        Op::Lb | Op::Lbu | Op::Lh | Op::Lhu | Op::Lw => {
            let addr = read_gpr(state, d.rs1).wrapping_add(d.imm as u32);
            let size = load_width(op);
            if misaligned == MisalignedPolicy::Trap && !is_aligned(addr, size) {
                return Ok(StepOutcome::Trapped {
                    cause: TrapCause::MisalignedAccess,
                });
            }
            let raw = mem.read(addr, size)?;
            write_gpr(state, d.rd, extend_load(op, raw));
            sequential(d)
        }
        Op::Sb | Op::Sh | Op::Sw => {
            let addr = read_gpr(state, d.rs1).wrapping_add(d.imm as u32);
            let size = store_width(op);
            if misaligned == MisalignedPolicy::Trap && !is_aligned(addr, size) {
                return Ok(StepOutcome::Trapped {
                    cause: TrapCause::MisalignedAccess,
                });
            }
            let value = truncate_store(op, read_gpr(state, d.rs2));
            mem.write(addr, size, value)?;
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
            return Ok(StepOutcome::Trapped {
                cause: TrapCause::Ecall,
            })
        }
        Op::Ebreak => {
            return Ok(StepOutcome::Trapped {
                cause: TrapCause::Breakpoint,
            })
        }
    };

    state.pc = next_pc;
    Ok(StepOutcome::Advanced)
}
