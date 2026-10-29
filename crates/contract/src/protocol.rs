//! Unified command / event protocol (LLD §3.2.1, AD §3).
//!
//! Every interaction between the front-end, server, and kernel flows as
//! a [`Message`]. Commands travel host → kernel; events travel kernel → host.

use serde::{Deserialize, Serialize};

use crate::model::{
    Breakpoint, DeltaRegister, DisasmEntryDto, Project, SimState, Symbol, TraceFormat, TraceSource,
    Watchpoint, WaveformHistory, WriteSize,
};
use crate::platform::{PlatformDescription, RegionSpec};
use crate::SCHEMA_VERSION;

/// Direction of a message on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    /// Host → kernel.
    Command,
    /// Kernel → host.
    Event,
}

impl Default for Direction {
    fn default() -> Self {
        Self::Command
    }
}

/// Top-level message envelope. Every byte on the wire is one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// Protocol schema version — used by [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Direction of this message.
    pub direction: Direction,
    /// Monotonically increasing sequence number; gap indicates a dropped
    /// message and the host should reconcile via M-07.
    pub seq: u64,
    /// Tagged payload (command or event).
    pub payload: Payload,
}

impl Message {
    /// Construct a command message.
    pub fn command(seq: u64, payload: impl Into<CommandPayload>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            direction: Direction::Command,
            seq,
            payload: Payload::Command(payload.into()),
        }
    }

    /// Construct an event message.
    pub fn event(seq: u64, payload: impl Into<EventPayload>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            direction: Direction::Event,
            seq,
            payload: Payload::Event(payload.into()),
        }
    }
}

impl Default for Message {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            direction: Direction::Command,
            seq: 0,
            payload: Payload::Command(CommandPayload::Nop),
        }
    }
}

/// Tagged payload — either a command or an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Payload {
    /// Host → kernel command.
    Command(CommandPayload),
    /// Kernel → host event.
    Event(EventPayload),
}

impl Default for Payload {
    fn default() -> Self {
        Self::Command(CommandPayload::Nop)
    }
}

/// All commands the host can issue. Adding a new variant is non-breaking
/// (consumers must handle unknown variants gracefully).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandPayload {
    /// Heartbeat / round-trip latency probe.
    Nop,
    /// Load a new project and reset the engine.
    LoadProject(Project),
    /// Run until the next breakpoint, watchpoint, or trap, **or until the
    /// engine's instruction budget for this run window is exhausted**.
    ///
    /// The quantum boundary is a **preemption point** (the scheduler may
    /// yield there), **not** a stop condition.
    ///
    /// **Contract v2 wording (D-57).** The v1 wording said "or quantum
    /// boundary", which contradicted the implementation: the engine ran
    /// until its instruction budget was exhausted (10 000 instructions =
    /// 10 quanta). The implementation is the one that was kept — it is
    /// the semantics D-07 closed and the `d07_time_driven.rs` suite
    /// pins — so the *documentation* was corrected instead.
    ///
    /// ⚠️ The same wording used to live in
    /// [`crate::port::ExecutionBackend::continue_until_event`], which
    /// was corrected in v2 as well. **Both places said it** — the
    /// variant's doc comment and the port's — and only the port was
    /// caught the first time round.
    Continue,
    /// Run exactly `n` ISS instructions.
    StepN {
        n: u32,
    },
    /// Request a pause.
    ///
    /// The engine records the request and the host reports
    /// `Halted { reason: UserPause }`.
    ///
    /// ⚠️ The v1 wording said "Pause the engine at the next quantum
    /// boundary" — but a command is handled at a **quiescent** point
    /// (the host is not concurrent with the engine's run loop), so
    /// there is no "next quantum boundary" to wait for. The wording now
    /// says what actually happens. (D-57's family; the related dead
    /// interface `StepOutcome::HostPause` was **removed** by the M-16
    /// landing batch — see the debt ledger **D-72**, now closed.)
    Pause,
    /// Set / clear breakpoints and watchpoints.
    SetBreakpoint(Breakpoint),
    ClearBreakpoint {
        id: u32,
    },
    SetWatchpoint(Watchpoint),
    ClearWatchpoint {
        id: u32,
    },
    /// Read `size` bytes at `addr` from the simulated memory map.
    ReadMemory {
        addr: u32,
        size: WriteSize,
    },
    /// Write `size` bytes at `addr` to the simulated memory map.
    WriteMemory {
        addr: u32,
        size: WriteSize,
        value: u32,
    },
    /// Read a range of general-purpose registers (M-10).
    ///
    /// The reply is [`EventPayload::Registers`]. `index` and `count` are
    /// clamped to the 32-register file; the reply always reports the
    /// **actual** start index it used.
    ///
    /// Added in contract v2: before v2 the contract had no register
    /// carrier at all, so the front end had to read registers out of the
    /// edge-encoded waveform streams — which conflated "signal history"
    /// with "register snapshot".
    ReadRegisters {
        /// Index of the first register to read (0 = x0).
        index: u32,
        /// How many consecutive registers to read.
        count: u32,
    },
    /// Write one general-purpose register (M-10).
    ///
    /// Writing x0 is discarded (D-04). The reply is
    /// [`EventPayload::Registers`] carrying the register's new value.
    WriteRegister {
        /// Index of the register to write (0..=31).
        index: u32,
        /// The 32-bit value to write.
        value: u32,
    },
    /// Negotiate the protocol schema version and the session
    /// capabilities (M-05).
    ///
    /// The reply is [`EventPayload::Negotiated`]. The host sends the
    /// **lowest** schema version it can speak; the kernel answers with
    /// the version it settled on.
    Negotiate {
        /// The lowest schema version the host can speak.
        min_schema_version: u32,
        /// Capability bits the host offers (M-05 `capBits`).
        cap_bits: u64,
    },
    /// Subscribe to an event stream. Idempotent.
    Subscribe {
        topic: String,
    },
    /// Unsubscribe from an event stream. Idempotent.
    Unsubscribe {
        topic: String,
    },
    /// Create a snapshot of the current [`SimState`].
    Snapshot,
    /// Restore a previously-created [`SimState`].
    Restore {
        state: Box<SimState>,
    },
    /// Reserved for the admin / sysadmin workbench channel.
    Admin {
        op: AdminOp,
    },
    /// Disassemble a run of instructions (M-11).
    ///
    /// The reply is [`EventPayload::Disasm`]. The sequence may be
    /// **shorter** than `count` — an unmapped address ends the walk
    /// early (truncation, not an error; LLD 11-3).
    ///
    /// Added in contract v3 (STAGE-5).
    Disasm {
        /// Start address.
        addr: u32,
        /// Maximum number of instructions to return.
        count: u32,
    },
    /// Resolve an address to a source location (M-11).
    ///
    /// The reply is [`EventPayload::SourceLocation`]. The reply carries
    /// **two distinct "no answer" states** (LLD 11-4): no DWARF at
    /// all ⇒ `file` empty and `line == 0`; DWARF present but the
    /// address misses the line table ⇒ `file` empty and
    /// `line == u32::MAX`.
    ///
    /// Added in contract v3 (STAGE-5).
    ResolveLine {
        /// Address to resolve (usually the PC).
        addr: u32,
    },
    /// Export a trace (M-11 / M-19).
    ///
    /// The reply is [`EventPayload::TraceExport`]. The **source**
    /// selects the buffer and the **format** selects the serializer;
    /// an illegal combination is rejected with `E_CTX-03`. The host
    /// assembly layer routes `source` to M-19 or M-11, so neither
    /// module depends on the other (STAGE-5 裁决 D).
    ///
    /// Added in contract v3 (STAGE-5).
    ExportTrace {
        /// Which buffer to read from.
        source: TraceSource,
        /// Output format.
        format: TraceFormat,
        /// Resume point, in **entries** (0 for the first chunk).
        offset: u64,
        /// Maximum number of entries in this chunk (0 = kernel default).
        limit: u32,
    },
    /// Turn execution-trace recording on or off (M-11, review R9).
    ///
    /// The reply is [`EventPayload::TraceState`]. While recording is
    /// off the kernel records **nothing** (zero overhead); while it is
    /// on and the simulation is running, the front end shows the
    /// slowdown notice (LLD §6.1).
    ///
    /// Added in contract v3 (STAGE-5).
    SetTraceEnabled {
        /// True = record, false = stop recording.
        enabled: bool,
    },
    /// Write bytes **into** the console UART (the RX direction).
    ///
    /// ⚠️ **This is the exact mirror of v4's [`EventPayload::Console`]**:
    /// that event carries the bytes the guest **wrote out** (TX), this
    /// command carries the bytes the host writes **in** (RX). Both
    /// directions travel as **raw bytes**, because the guest's byte
    /// stream is the truth (the Spike comparison is byte-wise).
    ///
    /// ⚠️ serde's default for `Vec<u8>` is a **JSON array of numbers**
    /// (`[104, 105]`) — **not** base64, **not** a string. The JS
    /// mirror (`front/lib/message.js`'s `Command.writeConsole`) spells
    /// it the same way.
    ///
    /// ⚠️ **One command does one thing**: it writes to the session's
    /// **only** UART (UART0). There is **no** "which UART" selector in
    /// this batch — the platform has exactly one console UART.
    ///
    /// ⚠️ **Do not reuse [`CommandPayload::WriteMemory`]** for this:
    /// writing `0x1000_0000` *is* "writing memory", but it bypasses
    /// the peripheral's semantics (no RX FIFO, no LSR `RX_VALID`, no
    /// interrupt line) ⇒ the guest would read a stale `RHR` while the
    /// `LSR` says "no data" ⇒ **self-contradictory**.
    ///
    /// Added in contract v5.
    WriteConsole {
        /// The raw bytes to hand to the UART's receive FIFO, in order.
        ///
        /// An **empty** vector is legal and is a **no-op** (not an
        /// error): the reply still comes back.
        bytes: Vec<u8>,
    },
    /// **设置板级内存图**（用户在前端配的那张板）。
    ///
    /// ⚠️ **携带的是一整份描述**，不是 patch（裁决 A / 裁决 I / §2.1 ⑤）。
    ///
    /// ⚠️ **零新 DTO**：载荷用的是契约层**已有**的
    /// [`PlatformDescription`]（[`crate::platform`]）⇒ 描述 DTO
    /// **一字未改**（裁决 H）。
    ///
    /// ⚠️ **整份替换** ⇒ 没有「当前值 + 补丁」的第二个真相（D-137）。
    ///
    /// The reply is [`EventPayload::MemoryMap`] — carrying the
    /// **currently effective** map plus a `pending` flag.
    ///
    /// Added in contract v6; **`expected_revision` added in v7**.
    SetMemoryMap {
        /// 板级描述（**契约层已有的** `PlatformDescription` ⇒ 零新 DTO）。
        description: PlatformDescription,
        /// 期望的**修订号**（D-137 · v0.31）—— `None` ⇒ **不检查**。
        ///
        /// ⚠️ `Some(r)` 且 `r` 与宿主当前的修订号**不等** ⇒ 宿主**明确
        /// 拒绝**（`E_PLT_13`）且**什么都不动** ⇒ 并发丢更新**看得见**。
        ///
        /// ⚠️ **`#[serde(default)]`** ⇒ **旧载荷缺这个字段也能解析**
        /// ⇒ 旧调用方**不受影响**（只是**不检查**修订号）。
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    /// **卸载工程**（D-139 · v0.31）—— 清**运行态**；镜像与用户的板**都
    /// 留着**。
    ///
    /// ⚠️ **无载荷**：这条命令**不带任何参数** ⇒ 卸载的是「当前这个
    /// 会话」，不是「某张指定的板」。
    ///
    /// ⚠️ **清运行态**（寄存器 / 事件队列 / 断点 / 观察点）**但留着镜像
    /// 字节** ⇒ 重新装载**不必重新选文件** ⇒ 且 `image_loaded` 变假 ⇒
    /// **之后改板立即生效**。
    ///
    /// ⚠️ **用户的板（`user_board`）也留着** —— 它是**用户意图**，不是
    /// 运行态 ⇒ 卸载不该把它丢掉。
    ///
    /// Added in contract v7.
    UnloadProject,
    /// **差量更新**板级内存图（D-137 · v0.31）。
    ///
    /// ⚠️ **原子**：一条命令携带**一串**操作 ⇒ 宿主**一次性**合成 ⇒
    /// **一次性**整体 `validate()` ⇒ 成功则**一次性**应用 ⇒ 中途失败
    /// **不会半张图**。
    ///
    /// ⚠️ **唯一状态始终是整图** ⇒ 差量只是**到达它的手段** ⇒ 不存在
    /// 第二个真相源。
    ///
    /// The reply is [`EventPayload::MemoryMap`] — carrying the
    /// **currently effective** map plus `pending` / `loaded` / `revision`.
    ///
    /// Added in contract v7.
    PatchMemoryMap {
        /// 这一串差量操作（按顺序施加到**当前生效的图**上）。
        ops: Vec<MemoryMapOp>,
        /// 期望的**修订号**（D-137 · v0.31）—— `None` ⇒ **不检查**。
        ///
        /// ⚠️ 与 [`CommandPayload::SetMemoryMap`] 的同名字段**同义**：
        /// `Some(r)` 且 `r` 与宿主当前的修订号**不等** ⇒ 宿主**明确拒绝**
        /// （`E_PLT_13`）且**什么都不动**。
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    /// 按**名字**查符号（M-11 · v0.57 批）。
    ///
    /// ⚠️ **一条命令同时服务两种用法**（裁决 B）：
    /// - **精确命中** ⇒ [`EventPayload::SymbolResolved`] 的 `symbol` 里有它；
    /// - **只给前缀** ⇒ `candidates` 里有候选。
    ///
    /// ⚠️ **不**另开「列出全部符号」的命令（osz 的符号表有几千条 ⇒ 全量
    /// 回传会撑爆事件）。
    ///
    /// Added in contract v9 (STAGE-42 §2.2).
    ResolveSymbol {
        /// 符号名。**空字符串或纯空白 ⇒ 拒绝**（`E_DBG-02`）。
        name: String,
    },
    /// 转储一段内存（M-10 · v0.57 批）。
    ///
    /// ⚠️ **原样拒，不静默截断**（裁决 D）：`len == 0` 或 `len > DUMP_MAX_BYTES`
    /// ⇒ ⇒ `E_DBG_03`。
    ///
    /// ⚠️ **与 [`CommandPayload::ReadMemory`] 的分工**：那条命令的 `size` 是
    /// `WriteSize`（**1 / 2 / 4 字节**三档）⇒ ⇒ 一次最多看 **4 字节** ⇒ ⇒
    /// 要看**一段**就得有这条新命令（STAGE-42 §1.1）。
    ///
    /// Added in contract v9 (STAGE-42 §2.2).
    DumpMemory {
        /// 起始**物理**地址。
        addr: u32,
        /// 字节数（`1..=DUMP_MAX_BYTES`）。
        len: u32,
    },
}

/// 一条**差量操作** —— 板级内存图的增量（D-137 · v0.31）。
///
/// ⚠️ **serde 默认的外部标签**（与 [`CommandPayload`] 同形）⇒ **线上键
/// 就是 Rust 变体名**：
///
/// ```text
/// {"UpsertRegion": {"spec": {"kind": "ram", "base": 0, "size": 1048576}}}
/// {"RemoveRegion": {"base": 0}}
/// {"SetBudget": {"region_budget_bytes": 16777216, "max_region_bytes": null}}
/// ```
///
/// ⚠️ **不许**给它加 `#[serde(tag = "op")]`（那是**内部标签**）⇒ 裁决 H
/// 的硬约束 ⇒ **外部标签**。
///
/// ⚠️ **那条约束的理由与它的实测记录只写在规格一处**（裁决 H）⇒ 这里
/// 只**引用**，不另写一份 ⇒ 免得两处说法不同步。
///
/// ⚠️ **两层的标签规则不同，各写各的**：`MemoryMapOp` 自己是**外部标签**
/// （键 = 变体名）⇒ 而 `spec` 里的 [`RegionSpec`] 是**内部标签**（线上
/// 是 `{"kind": "ram", ...}`）⇒ **不要**把两层的规则混在一起写。
///
/// ⚠️ **差量不覆盖元数据**（`id` / `name` / `priority` /
/// `auto_selectable`）⇒ 改板的身份属于「换一块板」⇒ 走**整份替换**
/// （[`CommandPayload::SetMemoryMap`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryMapOp {
    /// 插入或替换**一段**（`base` 在 `spec` 里）。
    ///
    /// ⚠️ 按 `spec` 的 `base` 定位：命中 ⇒ **替换**；未命中 ⇒ **追加**
    /// （不是替换）⇒ 合法。
    UpsertRegion {
        /// 那一段的声明（**内部标签** ⇒ 线上是 `{"kind": ...}`）。
        spec: RegionSpec,
    },
    /// 删掉**一段**（按 `base` 定位）。
    ///
    /// ⚠️ `base` 不在生效图里 ⇒ 宿主报 **`E_PLT_14`** 且**什么都不动**
    /// ⇒ 差量**不静默忽略**。
    RemoveRegion {
        /// 那一段的基地址（**定位键**）。
        base: u32,
    },
    /// 改预算（声明期 + 运行期）。
    ///
    /// ⚠️ `max_region_bytes` 为 `None` ⇒ 运行期上界**等于**声明期那个。
    SetBudget {
        /// 声明期每区域的**声明大小**上界（V-7 判它）。
        region_budget_bytes: u32,
        /// 运行期每区域的**物理占用**上界（`None` ⇒ 等于声明期那个）。
        ///
        /// ⚠️ **`#[serde(default)]`**：这个字段的**含义**是可选的（缺省
        /// 即「等于声明期那个」）⇒ 载荷里**不带**它也解析得出 `None` ⇒
        /// 线上形态**不变**（序列化时它**总是**出现，值可以是 `null`）。
        #[serde(default)]
        max_region_bytes: Option<u32>,
    },
}

impl Default for CommandPayload {
    fn default() -> Self {
        Self::Nop
    }
}

impl From<CommandPayload> for Payload {
    fn from(c: CommandPayload) -> Self {
        Self::Command(c)
    }
}

/// Reserved admin / sysadmin operations (M-22 + M-23 + M-21).
///
/// ⚠️ **Gated by the receiving side's own role** ⇒ ⇒ the contract
/// layer carries **no** role / token concept ⇒ ⇒ the host's
/// **role** (`Admin` / `Product`, default `Product`) decides whether the
/// channel answers at all ⇒ ⇒ **who** is allowed to *hold* that role is a
/// **separate** ledger item (**D-131**) ⇒ ⇒ **not** implemented here.
///
/// ⚠️ **v0.44** (STAGE-29 · 裁决 L) rewrote this paragraph ⇒ ⇒ at that
/// time the platform had **no role concept at all** ⇒ ⇒ the old wording
/// promised an admin-role gate that **did not exist** anywhere ⇒ ⇒ **the
/// behaviour never changed** (only the prose) ⇒ ⇒ and the previous
/// sentence is **gone** (⛔ it is **not** quoted here either ⇒ ⇒ so that
/// this file cannot reintroduce it by accident).
///
/// ⚠️ **v0.45** (STAGE-30 · 裁决 C) landed the **role gate** ⇒ ⇒ the
/// role check **does exist now** ⇒ ⇒ so this block must **not** keep
/// claiming otherwise ⇒ ⇒ ⚠️ **but the platform still authenticates
/// nobody** ⇒ ⇒ this block must **not** promise that it does ⇒ ⇒
/// **who** may hold `Admin` is still **D-131**) ⇒ ⇒ **not** implemented
/// here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdminOp {
    /// Dump the kernel log buffer (M-21).
    DrainLogs,
    /// Force-terminate a stuck guest (M-22 watchdog escalation).
    KillGuest,
    /// Trigger a project persistence operation (M-23).
    PersistProject,
    /// Reserved extension point.
    Custom(String),
}

impl Default for AdminOp {
    fn default() -> Self {
        Self::DrainLogs
    }
}

/// All events the kernel can publish. Same extensibility rules as commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventPayload {
    /// Periodic simulation tick (M-12). Includes elapsed time and a coarse
    /// status so the front-end can throttle renders.
    Tick {
        /// Elapsed virtual time (picoseconds) since the run started.
        ///
        /// Travels as a **decimal string** — see [`crate::numstr`] and the
        /// contract v2 spec §2.2.
        #[serde(with = "crate::numstr::u64_decimal")]
        elapsed_ps: u64,
        pc: u32,
    },
    /// The simulation hit a breakpoint or watchpoint and paused.
    Halted {
        reason: HaltReason,
        pc: u32,
    },
    /// Catch-all for kernel errors (see [`crate::error::PlatformError`]).
    Error {
        code: String,
        message: String,
    },
    /// Response to a `ReadMemory` command.
    MemoryRead {
        addr: u32,
        size: WriteSize,
        value: u32,
    },
    /// A batch of general-purpose register values (M-10).
    ///
    /// This is the **first-class** register carrier: before v2 the
    /// front-end had to read registers out of the edge-encoded waveform
    /// streams (the contract had no register event), which conflated
    /// "signal history" with "register snapshot".
    Registers {
        /// Index of the first register in `values` (0 = x0).
        index: u32,
        /// The register values, in ascending index order.
        values: Vec<u32>,
    },
    /// The negotiation result (M-05).
    Negotiated {
        /// The schema version the kernel settled on.
        schema_version: u32,
        /// The capability bits the kernel grants.
        cap_bits: u64,
        /// Kernel build / ISA descriptor (e.g. `"riscv_sim_kernel/0.1.0 RV32IMC"`).
        kernel: String,
    },
    /// An incremental simulation-state update (M-02 `onDelta`).
    ///
    /// The LLD's `simStateDelta` has been referenced by M-02 / M-03 /
    /// M-10 since v0.11 but the contract never carried it — the
    /// front-end's register view was rebuilt from waveform edges.
    SimStateDelta {
        /// The registers that changed, ascending index order.
        registers: Vec<DeltaRegister>,
        /// The new program counter, when it changed.
        pc: Option<u32>,
        /// Virtual time of the update — **decimal string** (spec §2.2).
        #[serde(with = "crate::numstr::u64_decimal")]
        t_ps: u64,
    },
    /// Push waveform edges since the last publish (M-19 incremental push).
    ///
    /// **Renamed in contract v2** (D-34): the variant was called
    /// `WaveformDelta` but carried the **history** shape
    /// ([`WaveformHistory`]) — the name on the event said "delta" while
    /// the content was a history. The *live batch* shape keeps the name
    /// [`crate::model::WaveformDelta`].
    WaveformHistory(WaveformHistory),
    /// Informational: a snapshot was created (carries the snapshot id).
    SnapshotCreated {
        id: u64,
    },
    /// Reserved for P1 multi-board interconnect events.
    LinkUp {
        link_id: String,
    },
    LinkDown {
        link_id: String,
    },
    /// The result of a `Disasm` command (M-11).
    ///
    /// Added in contract v3 (STAGE-5).
    Disasm {
        /// The instruction rows, in address order.
        ///
        /// May be **shorter** than the requested count — that is
        /// truncation (the walk hit an unmapped address), not an error.
        entries: Vec<DisasmEntryDto>,
    },
    /// The result of a `ResolveLine` command (M-11).
    ///
    /// Added in contract v3 (STAGE-5).
    SourceLocation {
        /// The address that was resolved.
        addr: u32,
        /// Source file path (empty when there is no answer).
        file: String,
        /// 1-based line number — `0` means "no DWARF at all",
        /// `u32::MAX` means "DWARF present but the address missed the
        /// line table" (LLD 11-4).
        line: u32,
    },
    /// One chunk of an exported trace (M-11 / M-19).
    ///
    /// Added in contract v3 (STAGE-5).
    TraceExport {
        /// The source that was read (echo).
        source: TraceSource,
        /// The format that was produced (echo).
        format: TraceFormat,
        /// Start of this chunk, in entries.
        offset: u64,
        /// Start of the next chunk — pass it back as `offset` to
        /// resume (the continuation protocol, LLD 11-7).
        next_offset: u64,
        /// Total number of entries that match the filter.
        total: u64,
        /// Serialized size of this chunk, in bytes.
        bytes: u64,
        /// The chunk content.
        chunk: String,
        /// **True when the exported window is not the complete
        /// history** — the source ring overflowed and dropped entries.
        ///
        /// ⚠️ **This is not decoration.** M-19's edge ring and M-11's
        /// trace ring are both **bounded** and drop their oldest
        /// entries on overflow. Without this flag the front end would
        /// present a truncated export as if it were the whole
        /// session — the same "the data exists but nobody is told it
        /// is incomplete" failure the D-50 / D-51 ledger entries are
        /// about.
        ///
        /// - `Waveform` source ⇒ M-19's `ExportHandle::overflow_marked`
        /// - `Execution` source ⇒ M-11's trace buffer dropped count > 0
        overflow_marked: bool,
    },
    /// The state of execution-trace recording (M-11).
    ///
    /// Added in contract v3 (STAGE-5).
    TraceState {
        /// Whether recording is currently on.
        enabled: bool,
        /// Number of entries currently buffered.
        len: u32,
        /// How many entries were dropped by the ring overflow.
        dropped: u64,
    },
    /// Console output produced by the guest (UART TX byte stream).
    ///
    /// ⚠️ **Bytes, not a string** — the guest may emit arbitrary bytes
    /// (ANSI escapes, `\r`, a half-written UTF-8 sequence). The byte
    /// stream is the **truth** (the Spike comparison is byte-wise); the
    /// front end decodes it for display.
    ///
    /// ⚠️ serde's default for `Vec<u8>` is a **JSON array of numbers**
    /// (`[104, 105]`) — **not** base64 and **not** a string. The JS
    /// mirror (`front/lib/message.js`) spells it the same way.
    ///
    /// Added in contract v4.
    Console {
        /// The raw bytes the guest wrote to the console UART.
        bytes: Vec<u8>,
    },
    /// **当前生效的内存图**（前端内存映射表的数据源 ⇒ 闭环 D-124）。
    ///
    /// ⚠️ `description` 是**当前生效**的图，**不是**「用户配的那份」
    /// （裁决 D）⇒ 前端内存映射表显示的**就是它**。
    ///
    /// ⚠️ **草稿不进事件**：前端「编辑器草稿」住在 M-03 视图状态里
    /// （前端自己的东西）⇒ 事件只描述**内核的状态**（裁决 D）。
    ///
    /// ⚠️ **v0.31 (D-137 / D-139)**: the event grew two fields — `loaded`
    /// (is an image loaded at all?) and `revision` (the map's monotonic
    /// revision counter) ⇒ the front end **copies no kernel state of
    /// its own**: whether a board change takes effect immediately is
    /// something the **kernel** says, and the revision the front end
    /// sends back with its next submission comes from **here**.
    ///
    /// ⚠️ `pending` and `loaded` are **not** the same thing: `pending` =
    /// "the user's board has not taken effect", `loaded` = "an image is
    /// running" ⇒ in a loaded session `pending = true` is the **normal**
    /// state (a board change is stored as pending).
    ///
    /// Added in contract v6; **`loaded` / `revision` added in v7**.
    MemoryMap {
        /// **当前生效**的图（裁决 D）。
        description: PlatformDescription,
        /// 宿主是否存着一份**待生效**的板级配置（裁决 B / D）。
        pending: bool,
        /// 有没有装载镜像 ⇒ 决定「改板」是**立即生效**还是**待生效**。
        ///
        /// ⚠️ **推导量**：它由**内核**说 ⇒ 前端**不复制**任何内核状态
        /// （否则就有第二个真相）。
        loaded: bool,
        /// 内存图的**修订号**（单调递增的计数器，裁决 A）。
        ///
        /// ⚠️ 前端下次提交时把它作为 `expected_revision` 带上 ⇒ 并发
        /// 丢更新**看得见**（`E_PLT_13`）。
        revision: u64,
    },
    /// 管理通道：导出的**日志条目**（回复 `CommandPayload::Admin { op:
    /// AdminOp::DrainLogs }`）。
    ///
    /// ⚠ **v8 新增**（STAGE-29 · D-129）⇒ ⇒ 在此之前 `Admin` 命令
    /// **只有命令侧、没有回复侧** ⇒ ⇒ 管理员通道接不上。
    ///
    /// ⚠ 条目的形状是 [`crate::admin::LogEntryDto`] ⇒ ⇒ 转换住在
    /// **M-21**（日志环的拥有者）⇒ ⇒ 契约层不依赖内核。
    ///
    /// ⚠ **有上界**：一次导出**不是**整个日志环 ⇒ ⇒ 超出部分**明确
    /// 告知被截断**（**不是静默**）。
    AdminLogs {
        /// The exported log entries, in virtual-time order.
        entries: Vec<crate::admin::LogEntryDto>,
    },
    /// 管理通道：**管理员诊断包**（回复 `CommandPayload::Admin { op:
    /// AdminOp::Custom("diag") }`）。
    ///
    /// ⚠ **v8 新增**（STAGE-29 · D-129）。
    ///
    /// ⚠ 形状是 [`crate::admin::AdminDiagDto`] ⇒ ⇒ 逐字镜像 M-22 的
    /// `DiagnosticKit`（**不多不少**）⇒ ⇒ 转换住在 **M-22**。
    AdminDiag {
        /// The diagnostic kit.
        diag: crate::admin::AdminDiagDto,
    },
    /// 符号查询的回复（M-11 · v0.57 批）。
    ///
    /// ⚠️ **零新 DTO**：载荷里的符号用的是契约层**已有**的
    /// [`crate::model::Symbol`]（STAGE-42 §2.1）。
    ///
    /// Added in contract v9 (STAGE-42 §2.3).
    SymbolResolved {
        /// **回显**请求里的名字（前端按它对上号）。
        name: String,
        /// **精确命中**的那一条（**没有 ⇒ `None`**；⚠️ 「没有」**不是**错误）。
        symbol: Option<Symbol>,
        /// **前缀候选**（名字以请求名**开头**的那些，按名字**升序**；
        /// **不含**精确命中那条；条数 ≤ `SYMBOL_CANDIDATE_LIMIT`）。
        candidates: Vec<Symbol>,
        /// 该镜像的**符号总数**（`0` ⇒ 这个镜像**没有符号表**）。
        total: u32,
    },
    /// 一段内存的**字节**（M-10 · v0.57 批）。
    ///
    /// Added in contract v9 (STAGE-42 §2.3).
    MemoryDump {
        /// **回显**起始地址。
        addr: u32,
        /// 读到的字节（长度 == 请求的 `len`；⚠️ 线上是 **JSON 数组**，
        /// **不是** base64、**不是**字符串 —— 与 `Console` 事件同口径）。
        bytes: Vec<u8>,
    },
}

impl Default for EventPayload {
    fn default() -> Self {
        Self::Tick {
            elapsed_ps: 0,
            pc: 0,
        }
    }
}

impl From<EventPayload> for Payload {
    fn from(e: EventPayload) -> Self {
        Self::Event(e)
    }
}

/// Why the simulation halted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HaltReason {
    /// Hit a user breakpoint.
    Breakpoint,
    /// Hit a watchpoint.
    Watchpoint,
    /// Reached end of execution (e.g. `mret` to non-existent PC).
    EndOfExecution,
    /// Illegal instruction or bus error.
    Trap,
    /// Engine pause requested by host.
    UserPause,
    /// Platform-level watchdog timeout (M-22).
    Watchdog,
}

impl Default for HaltReason {
    fn default() -> Self {
        Self::UserPause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The schema version is the contract's single version knob — the
    /// front end's JS mirror hard-codes the same number, and the
    /// `contract.test.mjs` gate compares the two.
    ///
    /// **v4** adds the guest console byte stream + the per-instruction
    /// source location on the disassembly entries.
    ///
    /// **v5** adds the console **input** direction (`WriteConsole`).
    ///
    /// **v6** adds the front-end configurable board memory map
    /// (`SetMemoryMap` + `MemoryMap`).
    ///
    /// **v7** adds the board delta / unload / budget split
    /// (`UnloadProject` + `PatchMemoryMap` + `expected_revision` + the
    /// `MemoryMap` event's two new fields).
    ///
    /// **v8** adds the **admin channel**'s reply side
    /// (`EventPayload::AdminLogs` + `EventPayload::AdminDiag` + the
    /// `admin::LogEntryDto` / `admin::AdminDiagDto` DTOs) and the one
    /// cross-boundary scenario code `E_CTX-05`
    /// (STAGE-29 §2.1..§2.3).
    ///
    /// **v9** adds the **debugger enhancements** batch
    /// (`ResolveSymbol` + `DumpMemory` + `SymbolResolved` + `MemoryDump`
    /// + the optional `Breakpoint::condition`) — STAGE-42 §2.
    ///
    /// ⚠️ **A-1**（STAGE-42 §6.1）：断言写**字面量** ⇒ ⇒ 升版 / 漏升版
    /// 都会当场变红。
    #[test]
    fn the_schema_version_is_nine() {
        assert_eq!(crate::SCHEMA_VERSION, 9);
    }

    /// `AdminOp`'s **doc** must state the gate the platform **really**
    /// has — and must **not** promise one it does not.
    ///
    /// ⚠️ **v0.45** (STAGE-30 · 裁决 C) landed the **role gate** ⇒ ⇒ the
    /// gate is now the host's own **role** ⇒ ⇒ the doc says so ⇒ ⇒ and
    /// the ledger's **D-131** (who may *hold* that role) is still open ⇒ ⇒
    /// the doc points at it.
    ///
    /// ⚠️ **v0.44 的旧版断言已经过期**（STAGE-30 §8 订正 5）：那时平台**没有**
    ///   角色概念 ⇒ ⇒ 那条用例断言「文档**不许**承诺角色检查」+「文档**必须**
    ///   写开关**」⇒ ⇒ v0.45 落地之后 ⇒ ⇒ 「角色检查**真的存在**了」⇒ ⇒
    ///   「不许承诺角色检查」**不再是不变式** ⇒ ⇒ 换成「**不许**承诺**认证**
    ///   检查」（平台**确实没有**认证 ⇒ ⇒ 那正是 D-131 还没做的那一段）。
    ///
    /// ⚠️ The check is scoped to **the enum's own doc block** ⇒ ⇒ a
    /// whole-file scan would hit **this test's own source** ⇒ ⇒ and a
    /// test that can only fail is not a test.
    #[test]
    fn the_admin_op_documentation_states_the_role_gate_and_not_an_authentication_one() {
        let source = include_str!("protocol.rs");
        let (before_enum, _) = source
            .split_once("pub enum AdminOp")
            .expect("the AdminOp enum");
        let mut doc_lines: Vec<&str> = Vec::new();
        for line in before_enum.lines().rev() {
            if line.trim_start().starts_with("///") {
                // ⚠️ **剥掉文档标记前缀** ⇒ ⇒ 否则归一化之后行与行之间会留下
                //   `///` ⇒ ⇒ 一条跨行的判据就失活了。
                let text = line.trim_start();
                let text = text.strip_prefix("///").unwrap_or(text);
                doc_lines.push(text.trim());
            } else if line.trim_start().starts_with("#[") {
                // An attribute (`#[derive(...)]`) may sit between the doc
                // block and the item ⇒ ⇒ skip it, do not stop there.
                continue;
            } else {
                break;
            }
        }
        doc_lines.reverse();
        let doc = doc_lines.join("\n");
        assert!(!doc.is_empty(), "AdminOp must keep a doc block of its own");
        // ⚠️ **归一化空白** ⇒ ⇒ 行折叠不该让一条判据失活。
        let flat = doc.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("the receiving side's own role"),
            "AdminOp's documentation must state that the receiving side's own role gates the channel"
        );
        assert!(
            flat.contains("`Admin` / `Product`, default `Product`"),
            "AdminOp's documentation must name both role values and the default role"
        );
        assert!(
            flat.contains("D-131"),
            "AdminOp's documentation must point at the open ledger item for who may hold the role"
        );
        // ⚠️⚠️ **逐处那条**（STAGE-30 §8 订正 5 的配套）：文档里 **D-131**
        //   **出现两次** ⇒ ⇒ 「文档提到 D-131」那条**全局**判据**改一处抓不到**
        //   ⇒ ⇒ ⇒ 必须另加**逐处**判据 ⇒ ⇒ 钉的是「**提到的那处**必须在说
        //   『谁可以持有那个角色』」⇒ ⇒ **不许**只把 D-131 留在别的句子里。
        let d131 = flat
            .find("D-131")
            .expect("AdminOp's documentation must mention D-131");
        let window = &flat[d131.saturating_sub(120)..(d131 + 4).min(flat.len())];
        assert!(
            window.contains("hold"),
            "AdminOp's documentation must point at D-131 where it says who may hold the role"
        );
        // ⚠️ **反向那条**：文档**自称**旧句原文没有被引回来 ⇒ ⇒ 这条断言钉的
        //   就是**文档自己的那个说法** ⇒ ⇒ 不是一条空护栏。
        assert!(
            !flat.contains("Always gated by an admin role check"),
            "AdminOp's documentation must not bring the old sentence back"
        );
        // ⚠️ **不许承诺认证检查**（平台**确实没有**认证 ⇒ ⇒ 那正是 D-131）。
        assert!(
            !flat.contains("authenticates the caller")
                && !flat.contains("gated by an authenticated"),
            "AdminOp's documentation must not promise an authentication check that does not exist"
        );
    }

    /// The three commands contract v2 adds (D-32 gaps 1, 2, 3).
    #[test]
    fn the_v2_commands_exist_with_their_frozen_field_names() {
        let read = CommandPayload::ReadRegisters { index: 1, count: 4 };
        let json = serde_json::to_value(&read).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "ReadRegisters": { "index": 1, "count": 4 } })
        );

        let write = CommandPayload::WriteRegister {
            index: 5,
            value: 0x2A,
        };
        let json = serde_json::to_value(&write).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "WriteRegister": { "index": 5, "value": 42 } })
        );

        let negotiate = CommandPayload::Negotiate {
            min_schema_version: 2,
            cap_bits: 0b1011,
        };
        let json = serde_json::to_value(&negotiate).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "Negotiate": { "min_schema_version": 2, "cap_bits": 11 } })
        );
    }

    /// The three events contract v2 adds (D-32 gaps 1, 3, 7).
    #[test]
    fn the_v2_events_exist_with_their_frozen_field_names() {
        let registers = EventPayload::Registers {
            index: 0,
            values: vec![0, 1, 2],
        };
        let json = serde_json::to_value(&registers).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "Registers": { "index": 0, "values": [0, 1, 2] } })
        );

        let negotiated = EventPayload::Negotiated {
            schema_version: 2,
            cap_bits: 7,
            kernel: "riscv_sim_kernel/0.1.0 RV32IMC".to_string(),
        };
        let json = serde_json::to_value(&negotiated).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "Negotiated": { "schema_version": 2, "cap_bits": 7,
                "kernel": "riscv_sim_kernel/0.1.0 RV32IMC" } })
        );

        let delta = EventPayload::SimStateDelta {
            registers: vec![crate::model::DeltaRegister {
                index: 1,
                value: 42,
            }],
            pc: Some(0x1000),
            t_ps: 1234,
        };
        let json = serde_json::to_value(&delta).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "SimStateDelta": {
                "registers": [{ "index": 1, "value": 42 }],
                "pc": 4096,
                "t_ps": "1234" } })
        );
    }

    /// The virtual-time fields travel as **decimal strings** (v2 spec
    /// §2.2) — the whole point of the rule is that a `u64` would
    /// silently lose precision above `2^53`.
    #[test]
    fn the_virtual_time_travels_as_a_decimal_string() {
        let tick = EventPayload::Tick {
            elapsed_ps: 9_007_199_254_740_993,
            pc: 0,
        };
        let json = serde_json::to_value(&tick).expect("json");
        assert_eq!(
            json,
            serde_json::json!({ "Tick": { "elapsed_ps": "9007199254740993", "pc": 0 } })
        );

        // A value a double could not hold survives the round trip.
        let back: EventPayload = serde_json::from_value(json).expect("round trip");
        match back {
            EventPayload::Tick { elapsed_ps, .. } => {
                assert_eq!(elapsed_ps, 9_007_199_254_740_993);
            }
            other => panic!("expected a Tick, got {other:?}"),
        }
    }

    /// The waveform event was renamed in v2 (D-34): the variant now says
    /// what it carries — the **history** shape, not a delta.
    #[test]
    fn the_waveform_event_carries_the_history_shape() {
        let history = crate::model::WaveformHistory {
            schema_version: 1,
            streams: vec![crate::model::WaveformStream {
                id: "GPIO0.OUT".to_string(),
                label: "GPIO0.OUT".to_string(),
                edges: vec![crate::model::Edge { t_ps: 5, value: 1 }],
            }],
        };
        let payload = EventPayload::WaveformHistory(history);
        let json = serde_json::to_value(&payload).expect("json");
        // The edge's timestamp is a string (v2 numeric policy) and the
        // variant name is the history one.
        assert_eq!(
            json,
            serde_json::json!({ "WaveformHistory": { "schema_version": 1, "streams": [
                { "id": "GPIO0.OUT", "label": "GPIO0.OUT",
                  "edges": [{ "t_ps": "5", "value": 1 }] } ] } })
        );
    }

    /// The envelope carries the schema version, and the version is the
    /// one the front end negotiates against.
    #[test]
    fn the_envelope_carries_the_current_schema_version() {
        let msg = Message::command(8, CommandPayload::Nop);
        assert_eq!(msg.schema_version, crate::SCHEMA_VERSION);
        let json = serde_json::to_value(&msg).expect("json");
        assert_eq!(json["schema_version"], 9);
    }

    // -----------------------------------------------------------------
    // Contract v3 — the M-11 debugging surface (STAGE-5 §3).
    //
    // ⚠️ These shapes are **frozen** by the STAGE-5 spec: every variant
    // name and field name below is part of the wire format, and the
    // front end's JS mirror (`front/lib/message.js`) spells them the
    // same way. Changing one without the other breaks the gate.
    // -----------------------------------------------------------------

    /// The four commands contract v3 adds (STAGE-5 §3.1).
    #[test]
    fn the_v3_commands_exist_with_their_frozen_field_names() {
        let disasm = CommandPayload::Disasm {
            addr: 0x1000,
            count: 8,
        };
        assert_eq!(
            serde_json::to_value(&disasm).expect("json"),
            serde_json::json!({ "Disasm": { "addr": 4096, "count": 8 } })
        );

        let resolve = CommandPayload::ResolveLine { addr: 0x1004 };
        assert_eq!(
            serde_json::to_value(&resolve).expect("json"),
            serde_json::json!({ "ResolveLine": { "addr": 4100 } })
        );

        let export = CommandPayload::ExportTrace {
            source: TraceSource::Execution,
            format: TraceFormat::Csv,
            offset: 16,
            limit: 32,
        };
        assert_eq!(
            serde_json::to_value(&export).expect("json"),
            serde_json::json!({ "ExportTrace": {
                "source": "Execution", "format": "Csv",
                "offset": 16, "limit": 32 } })
        );

        let enable = CommandPayload::SetTraceEnabled { enabled: true };
        assert_eq!(
            serde_json::to_value(&enable).expect("json"),
            serde_json::json!({ "SetTraceEnabled": { "enabled": true } })
        );
    }

    /// The four events contract v3 adds (STAGE-5 §3.2).
    #[test]
    fn the_v3_events_exist_with_their_frozen_field_names() {
        let disasm = EventPayload::Disasm {
            entries: vec![crate::model::DisasmEntryDto {
                addr: 0x1000,
                code: 0x0FF00093,
                mnemonic: "addi x1, x0, 255".to_string(),
                width: 4,
                // 契约 v4（D-102）新增的两个可选字段：没有行表 ⇒ `None`
                // ⇒ 线上是 `null`。
                file: None,
                line: None,
            }],
        };
        assert_eq!(
            serde_json::to_value(&disasm).expect("json"),
            serde_json::json!({ "Disasm": { "entries": [
                { "addr": 4096, "code": 267387027,
                  "mnemonic": "addi x1, x0, 255", "width": 4,
                  "file": null, "line": null } ] } })
        );

        let source = EventPayload::SourceLocation {
            addr: 0x1000,
            file: "demo.c".to_string(),
            line: 7,
        };
        assert_eq!(
            serde_json::to_value(&source).expect("json"),
            serde_json::json!({ "SourceLocation": {
                "addr": 4096, "file": "demo.c", "line": 7 } })
        );

        let export = EventPayload::TraceExport {
            source: TraceSource::Waveform,
            format: TraceFormat::Vcd,
            offset: 0,
            next_offset: 10,
            total: 42,
            bytes: 128,
            chunk: "$var".to_string(),
            overflow_marked: true,
        };
        assert_eq!(
            serde_json::to_value(&export).expect("json"),
            serde_json::json!({ "TraceExport": {
                "source": "Waveform", "format": "Vcd", "offset": 0,
                "next_offset": 10, "total": 42, "bytes": 128,
                "chunk": "$var", "overflow_marked": true } })
        );

        let state = EventPayload::TraceState {
            enabled: true,
            len: 5,
            dropped: 2,
        };
        assert_eq!(
            serde_json::to_value(&state).expect("json"),
            serde_json::json!({ "TraceState": {
                "enabled": true, "len": 5, "dropped": 2 } })
        );
    }

    /// The console event contract v4 adds — the guest's UART TX byte
    /// stream.
    ///
    /// ⚠️ The wire shape is **pinned**: serde's default for `Vec<u8>` is a
    /// **JSON array of numbers** (`[104, 105]`) — **not** base64, **not** a
    /// string. The JS mirror (`front/lib/message.js`'s `Event.console`)
    /// spells it the same way, and the cross-layer gate compares the two
    /// byte for byte.
    ///
    /// The bytes are **not** text: an empty stream is an **empty array**
    /// (`[]`), never `null` — `null` would mean "no value at all", which
    /// is a different thing than "the guest wrote nothing".
    #[test]
    fn the_v4_console_event_carries_the_raw_byte_stream() {
        let console = EventPayload::Console {
            bytes: vec![104, 105, 10],
        };
        assert_eq!(
            serde_json::to_value(&console).expect("json"),
            serde_json::json!({ "Console": { "bytes": [104, 105, 10] } })
        );

        // Round trip: the bytes survive unchanged (a byte is a byte).
        let back: EventPayload =
            serde_json::from_value(serde_json::to_value(&console).expect("json"))
                .expect("round trip");
        match back {
            EventPayload::Console { bytes } => assert_eq!(bytes, vec![104, 105, 10]),
            other => panic!("expected a Console, got {other:?}"),
        }

        // An empty stream is an empty array, not null.
        let empty = EventPayload::Console { bytes: Vec::new() };
        assert_eq!(
            serde_json::to_value(&empty).expect("json"),
            serde_json::json!({ "Console": { "bytes": [] } })
        );
    }

    /// The **two distinct "no answer" states** of a source lookup are
    /// part of the wire format (LLD 11-4): `line == 0` means "no DWARF
    /// at all", `line == u32::MAX` means "DWARF present but the address
    /// missed the line table". Collapsing them would lose the
    /// distinction the front end needs to render "no source info"
    /// differently from "not this line".
    #[test]
    fn the_source_location_distinguishes_degraded_from_missed() {
        let degraded = EventPayload::SourceLocation {
            addr: 0x1000,
            file: String::new(),
            line: 0,
        };
        assert_eq!(
            serde_json::to_value(&degraded).expect("json")["SourceLocation"]["line"],
            0
        );

        let missed = EventPayload::SourceLocation {
            addr: 0x1000,
            file: String::new(),
            line: u32::MAX,
        };
        assert_eq!(
            serde_json::to_value(&missed).expect("json")["SourceLocation"]["line"],
            serde_json::json!(4_294_967_295u64)
        );
    }

    /// The export enums are pinned: `Vcd` is waveform-only and `Json` is
    /// execution-only (STAGE-5 裁决 D). The kernel rejects the illegal
    /// combinations — but the **wire names** must not drift.
    #[test]
    fn the_export_enums_are_pinned() {
        assert_eq!(
            serde_json::to_value(TraceSource::Waveform).expect("json"),
            serde_json::json!("Waveform")
        );
        assert_eq!(
            serde_json::to_value(TraceSource::Execution).expect("json"),
            serde_json::json!("Execution")
        );
        assert_eq!(
            serde_json::to_value(TraceFormat::Csv).expect("json"),
            serde_json::json!("Csv")
        );
        assert_eq!(
            serde_json::to_value(TraceFormat::Vcd).expect("json"),
            serde_json::json!("Vcd")
        );
        assert_eq!(
            serde_json::to_value(TraceFormat::Json).expect("json"),
            serde_json::json!("Json")
        );
    }

    // -----------------------------------------------------------------
    // Contract v5 — the console **input** direction (STAGE-9 裁决 A).
    //
    // ⚠️ The wire shape is **pinned**: serde's default for `Vec<u8>` is a
    // **JSON array of numbers** (`[104, 105]`) — **not** base64, **not** a
    // string. The JS mirror (`front/lib/message.js`'s
    // `Command.writeConsole`) spells it the same way.
    // -----------------------------------------------------------------

    /// The `WriteConsole` command's wire shape is **pinned** to the
    /// string level.
    ///
    /// ⚠️ Why the **string** and not just `serde_json::json!`: the
    /// macro compares **values**, so it would accept a *reordered* key
    /// set as well as a renamed variant tag that happens to carry the
    /// same payload. The **string** form pins the variant tag's exact
    /// spelling, the field's exact spelling and the key order — a
    /// rename on either side turns this red.
    #[test]
    fn the_v5_write_console_command_has_a_pinned_wire_shape() {
        let cmd = CommandPayload::WriteConsole {
            bytes: vec![104, 105],
        };
        assert_eq!(
            serde_json::to_string(&cmd).expect("json"),
            r#"{"WriteConsole":{"bytes":[104,105]}}"#
        );
        // ⚠️ **不是**字符串、**不是** base64 —— 字节就是字节。
        assert_eq!(
            serde_json::to_value(&cmd).expect("json"),
            serde_json::json!({ "WriteConsole": { "bytes": [104, 105] } })
        );
        // 空字节流是**空数组**，不是 `null`（`null` 是「没有值」，
        // 那与「没写任何字节」是**两件事**）。
        assert_eq!(
            serde_json::to_string(&CommandPayload::WriteConsole { bytes: Vec::new() })
                .expect("json"),
            r#"{"WriteConsole":{"bytes":[]}}"#
        );
    }

    /// 往返之后**逐字节相同** —— 含 `0x00` / `0xFF` 这两个边界字节。
    ///
    /// ⚠️ 边界字节是这条用例的判别力所在：若线上表示被改成 base64 或
    /// 字符串，`0x00`（NUL）与 `0xFF` 这类字节最先露馅。
    #[test]
    fn the_v5_write_console_bytes_survive_the_round_trip_byte_for_byte() {
        let bytes = vec![0x00, 0x41, 0x7F, 0x80, 0xFF, 0x0D, 0x0A];
        let cmd = CommandPayload::WriteConsole {
            bytes: bytes.clone(),
        };
        let back: CommandPayload =
            serde_json::from_str(&serde_json::to_string(&cmd).expect("json")).expect("round trip");
        match back {
            CommandPayload::WriteConsole { bytes: got } => {
                assert_eq!(got, bytes, "每个字节都必须原样回来");
                assert_eq!(got.len(), 7, "一个字节都不能多、也不能少");
                assert_eq!(got[0], 0x00, "NUL 字节必须活下来");
                assert_eq!(got[4], 0xFF, "0xFF 必须活下来");
            }
            other => panic!("expected a WriteConsole, got {other:?}"),
        }
    }

    /// **两侧逐字对照**：Rust 侧读前端 JS 镜像的源码，断言里面**字面**
    /// 出现了 `WriteConsole` 这个拼写。
    ///
    /// ⚠️ **为什么要有这条**：契约层是 Rust，JS 镜像是手抄的 ⇒ 改了
    /// 一侧忘了另一侧 ⇒ 前端发出去的命令名内核**根本不认识** ⇒ 命令
    /// 永远没有回复（前端只能等超时），而**两侧各自的测试都绿着**。
    ///
    /// ⚠️ 这里断言的是**字面出现**，不是「JS 的构造器行为对」——行为由
    /// 前端自己的门禁（`front/tests/contract.test.mjs`）盯。
    #[test]
    fn the_v5_write_console_spelling_is_the_same_on_both_sides() {
        // ⚠️ 路径：`crates/contract/src/protocol.rs` → 仓库根是 `../../..`
        const JS_MIRROR: &str = include_str!("../../../front/lib/message.js");
        assert!(
            JS_MIRROR.contains("WriteConsole"),
            "前端 JS 镜像里必须**字面**出现 `WriteConsole` —— 两侧拼写必须一致"
        );
        // ⚠️ 版本号也钉住：JS 镜像的 `SCHEMA_VERSION` 与 Rust 侧**必须
        // 严格相等**。
        //
        // ⚠️ **为什么是「严格相等」而不是「至多落后一代」**：契约层的
        // 升版与前端 JS 镜像的升版**分属两条 lane** ⇒ 并行落地的窗口内
        // JS 侧还是旧的那个数 ⇒ 一条写死「必须相等」的断言会**自己变红**
        // （而错的不是任何一侧）⇒ Lane A 在落地时把它放松成「至多落后
        // 一代」⇒ **那是一个临时的护栏，不是契约**。
        //
        // ⚠️ **窗口已经关闭**（前端那条 lane 已落地 ⇒ 两侧都是 7）⇒
        // 这里恢复成严格相等 ⇒ 「升版只改一侧」这件事**不再能悄悄通过**
        // （前端门禁 `front/tests/contract.test.mjs` 的 `rustSchemaVersion()`
        // 从 Rust 源码推导、不写死 ⇒ 它守的是「字段集合」；这条守的是
        // **版本号本身**）。
        let js_version: u32 = JS_MIRROR
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("export const SCHEMA_VERSION = ")
                    .and_then(|rest| rest.trim().trim_end_matches(';').trim().parse().ok())
            })
            .expect("前端 JS 镜像里必须声明 SCHEMA_VERSION");
        assert_eq!(
            js_version,
            crate::SCHEMA_VERSION,
            "前端 JS 镜像的 SCHEMA_VERSION（{js_version}）必须与 Rust 侧（{}）**严格相等**",
            crate::SCHEMA_VERSION
        );
    }

    // -----------------------------------------------------------------
    // Contract v6 — the front-end configurable board memory map
    // (STAGE-14 §3.1).
    //
    // ⚠️ These shapes are **frozen** by the STAGE-14 spec: every variant
    // name and field name below is part of the wire format, and the
    // front end's JS mirror (`front/lib/message.js`) spells them the
    // same way. Changing one without the other breaks the gate.
    //
    // ⚠️ **Zero new DTO**: the payloads carry the contract layer's
    // **existing** `PlatformDescription` — the description DTOs
    // (`PlatformDescription` / `RegionSpec` / `DeviceSpec`) are
    // **unchanged** (裁决 H).
    // -----------------------------------------------------------------

    /// 一张「用户在前端配的板」—— **在这里现搭**，不用内置平台。
    ///
    /// ⚠️ **为什么不用内置平台**：期望的 JSON 必须是**架构字面量**，
    /// 而不是「内置平台今天恰好长什么样」的镜像 ⇒ 换个内置平台就误红。
    fn a_user_configured_board() -> PlatformDescription {
        PlatformDescription {
            schema_version: crate::platform::PLATFORM_SCHEMA_VERSION,
            id: "user-board".to_string(),
            name: "User board".to_string(),
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 0x0010_0000,
            max_region_bytes: None,
            regions: vec![crate::platform::RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0010_0000,
            }],
            description: None,
            wiring: None,
        }
    }

    /// `SetMemoryMap` 命令的线上形态**钉到字符串**。
    ///
    /// ⚠️ **为什么钉字符串而不是只钉 `serde_json::json!`**：那个宏比较
    /// **值** ⇒ 它接受一个**键序被打乱**的键集，也接受一个「换了名字
    /// 但载荷一样」的变体标签。字符串形态把变体标签的**确切拼写**、
    /// 字段的**确切拼写**与**键序**都钉住 ⇒ 任一侧改名都会红。
    ///
    /// ⚠️ **v7**: the command gained `expected_revision` (after
    /// `description`) and the description gained `max_region_bytes`
    /// (after `region_budget_bytes`) ⇒ the pinned string moves with it.
    #[test]
    fn the_v7_set_memory_map_command_has_a_pinned_wire_shape() {
        let cmd = CommandPayload::SetMemoryMap {
            description: a_user_configured_board(),
            expected_revision: None,
        };
        assert_eq!(
            serde_json::to_string(&cmd).expect("json"),
            r#"{"SetMemoryMap":{"description":{"schema_version":1,"id":"user-board","name":"User board","priority":100,"auto_selectable":false,"region_budget_bytes":1048576,"max_region_bytes":null,"regions":[{"kind":"ram","base":0,"size":1048576}]},"expected_revision":null}}"#
        );
        // ⚠️ 描述 DTO **只多了一个带 default 的可选字段** ⇒ 线上还是
        // `PlatformDescription` 的原样形态（`kind` 标签、`auto_selectable`、
        // 省略的 `description`、**总是出现的** `max_region_bytes`）。
        assert_eq!(
            serde_json::to_value(&cmd).expect("json"),
            serde_json::json!({ "SetMemoryMap": { "description": {
                "schema_version": 1,
                "id": "user-board",
                "name": "User board",
                "priority": 100,
                "auto_selectable": false,
                "region_budget_bytes": 1048576,
                "max_region_bytes": null,
                "regions": [{ "kind": "ram", "base": 0, "size": 1048576 }]
            }, "expected_revision": null } })
        );
    }

    /// `MemoryMap` 事件的线上形态**钉到字符串**。
    ///
    /// ⚠️ `pending` 排在 `description` **之后** ⇒ 字段的**声明顺序**也
    /// 是线上形态的一部分。
    ///
    /// ⚠️ **v7**: the event gained `loaded` and `revision` (after
    /// `pending`) ⇒ the pinned string moves with it.
    #[test]
    fn the_v7_memory_map_event_has_a_pinned_wire_shape() {
        let ev = EventPayload::MemoryMap {
            description: a_user_configured_board(),
            pending: false,
            loaded: true,
            revision: 7,
        };
        assert_eq!(
            serde_json::to_string(&ev).expect("json"),
            r#"{"MemoryMap":{"description":{"schema_version":1,"id":"user-board","name":"User board","priority":100,"auto_selectable":false,"region_budget_bytes":1048576,"max_region_bytes":null,"regions":[{"kind":"ram","base":0,"size":1048576}]},"pending":false,"loaded":true,"revision":7}}"#
        );
        assert_eq!(
            serde_json::to_value(&ev).expect("json"),
            serde_json::json!({ "MemoryMap": { "description": {
                "schema_version": 1,
                "id": "user-board",
                "name": "User board",
                "priority": 100,
                "auto_selectable": false,
                "region_budget_bytes": 1048576,
                "max_region_bytes": null,
                "regions": [{ "kind": "ram", "base": 0, "size": 1048576 }]
            }, "pending": false, "loaded": true, "revision": 7 } })
        );
    }

    /// `pending` 的**两个取值**都必须能往返。
    ///
    /// ⚠️ 判别力：只钉 `false` 的话，把字段**删掉**（serde 默认补
    /// `false`）这条用例仍然绿 ⇒ `true` 那一半才是「字段真的在」、
    /// 且**没有被丢掉**的证据。
    #[test]
    fn the_v6_memory_map_pending_flag_survives_the_round_trip() {
        for pending in [true, false] {
            let ev = EventPayload::MemoryMap {
                description: a_user_configured_board(),
                pending,
                loaded: true,
                revision: 3,
            };
            let back: EventPayload =
                serde_json::from_str(&serde_json::to_string(&ev).expect("json"))
                    .expect("round trip");
            match back {
                EventPayload::MemoryMap { pending: got, .. } => {
                    assert_eq!(got, pending, "`pending` 必须原样回来");
                }
                other => panic!("expected a MemoryMap, got {other:?}"),
            }
        }
    }

    /// 整份描述**逐字段**往返 —— 含 ROM 段与**四种**器件。
    ///
    /// ⚠️ 这一条钉住「**零新 DTO**」：描述 DTO **只多了一个带 default
    /// 的可选字段** ⇒ 它的全部形态（`kind` / `device` 内部标签、器件
    /// 参数）都还是原来的那些 ⇒ 一个字都不该变。
    #[test]
    fn the_v6_memory_map_description_survives_the_round_trip() {
        let description = PlatformDescription {
            schema_version: crate::platform::PLATFORM_SCHEMA_VERSION,
            id: "full-board".to_string(),
            name: "Full board".to_string(),
            priority: 7,
            auto_selectable: true,
            region_budget_bytes: 0x0020_0000,
            max_region_bytes: None,
            regions: vec![
                crate::platform::RegionSpec::Ram {
                    base: 0x0000_0000,
                    size: 0x0010_0000,
                },
                crate::platform::RegionSpec::Rom {
                    base: 0x0010_0000,
                    size: 0x0001_0000,
                },
                crate::platform::RegionSpec::Peripheral {
                    base: 0x1000_0000,
                    device: crate::platform::DeviceSpec::Gpio {
                        id: "GPIO0".to_string(),
                    },
                },
                crate::platform::RegionSpec::Peripheral {
                    base: 0x1000_1000,
                    device: crate::platform::DeviceSpec::Uart16550 {
                        id: "UART0".to_string(),
                        irq_line: 1,
                        baud_rate: 115_200,
                    },
                },
                crate::platform::RegionSpec::Peripheral {
                    base: 0x2000_0000,
                    device: crate::platform::DeviceSpec::Clint {
                        id: "CLINT".to_string(),
                        freq_hz: 10_000_000,
                        harts: 1,
                    },
                },
                crate::platform::RegionSpec::Peripheral {
                    base: 0x2000_1000,
                    device: crate::platform::DeviceSpec::Plic {
                        id: "PLIC".to_string(),
                        sources: 16,
                        contexts: 2,
                    },
                },
            ],
            description: Some("用户配的板".to_string()),
            wiring: None,
        };
        let cmd = CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        };
        let back: CommandPayload =
            serde_json::from_str(&serde_json::to_string(&cmd).expect("json")).expect("round trip");
        match back {
            CommandPayload::SetMemoryMap {
                description: got,
                expected_revision,
            } => {
                assert_eq!(got, description, "整份描述必须逐字段原样回来");
                assert_eq!(expected_revision, None, "期望修订号必须原样回来");
            }
            other => panic!("expected a SetMemoryMap, got {other:?}"),
        }
    }

    /// **两侧逐字对照**：Rust 侧读前端 JS 镜像的源码，断言里面**字面**
    /// 出现了 `SetMemoryMap` / `MemoryMap` / `memory.map` 这几个拼写，
    /// 且两个构造器产出的**形状**与 Rust 侧**逐字同形**。
    ///
    /// ⚠️ **为什么要有这条**：契约层是 Rust，JS 镜像是手抄的 ⇒ 改了
    /// 一侧忘了另一侧 ⇒ 前端发出去的命令名内核**根本不认识** ⇒ 命令
    /// 永远没有回复（前端只能等超时），而**两侧各自的测试都绿着**。
    ///
    /// ⚠️ 这里断言的是**字面出现**，不是「JS 的构造器行为对」——行为由
    /// 前端自己的门禁（`front/tests/contract.test.mjs`）盯。
    #[test]
    fn the_v6_memory_map_spelling_is_the_same_on_both_sides() {
        // ⚠️ 路径：`crates/contract/src/protocol.rs` → 仓库根是 `../../..`
        const JS_MIRROR: &str = include_str!("../../../front/lib/message.js");
        for literal in [
            "SetMemoryMap",
            "MemoryMap",
            "memory.map",
            "setMemoryMap",
            "memoryMap",
        ] {
            assert!(
                JS_MIRROR.contains(literal),
                "前端 JS 镜像里必须**字面**出现 `{literal}` —— 两侧拼写必须一致"
            );
        }
    }

    // -----------------------------------------------------------------
    // v7 — the **shape** cross-check (Rust source vs. the JS mirror).
    // -----------------------------------------------------------------

    /// Tokenize `src` into identifiers / punctuation / string literals,
    /// **skipping** comments.
    ///
    /// ⚠️ **Why a tokenizer and not a literal substring**: the JS mirror
    /// is **hand-written** ⇒ its formatting is nobody's contract ⇒ a
    /// literal check (`"({ SetMemoryMap: { description"`) breaks the
    /// moment somebody spreads a constructor over several lines (which
    /// is exactly what v7 did) ⇒ the assertion would go red **for a
    /// reason that has nothing to do with the wire format**.
    ///
    /// ⚠️ **Comments are skipped** ⇒ a doc comment that mentions a
    /// variant name can never be mistaken for the variant itself.
    ///
    /// @param src — the source text
    /// @param single_quote_is_a_string — `true` for JavaScript (`'x'` is
    ///   a string), `false` for Rust (`'a` is a lifetime)
    /// @returns the token list (identifiers are bare, strings are quoted)
    fn tokenize(src: &str, single_quote_is_a_string: bool) -> Vec<String> {
        let bytes = src.as_bytes();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let c = bytes[i];
            // line comment
            if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            // block comment
            if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                continue;
            }
            // string literal
            let is_quote = c == b'"' || c == b'`' || (single_quote_is_a_string && c == b'\'');
            if is_quote {
                let quote = c;
                i += 1;
                let start = i;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == quote {
                        break;
                    }
                    i += 1;
                }
                let text = src.get(start..i.min(src.len())).unwrap_or("");
                out.push(format!("\"{text}\""));
                i = (i + 1).min(bytes.len());
                continue;
            }
            // identifier
            if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$')
                {
                    i += 1;
                }
                out.push(src[start..i].to_string());
                continue;
            }
            if c.is_ascii_whitespace() {
                i += 1;
                continue;
            }
            out.push((c as char).to_string());
            i += 1;
        }
        out
    }

    /// Is `token` an identifier (not punctuation, not a string literal)?
    fn is_identifier(token: &str) -> bool {
        let mut chars = token.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    }

    /// The **top-level keys** of the block that follows the `key` token,
    /// in source order.
    ///
    /// ⚠️ **What counts as a key**: an identifier that sits *where a key
    /// sits* — right after a `{` or a `,` at the block's own depth.
    /// ⇒ Rust's `description: PlatformDescription` and JavaScript's
    /// shorthand `description` both count; a **value**
    /// (`expected_revision: expectedRevision`) does **not** (it never
    /// follows a `{` or a `,`).
    ///
    /// ⚠️ **Rust attributes** (`#[serde(default)]`) sit between the `,`
    /// and the field ⇒ they are skipped as a unit, and they do **not**
    /// consume the key position.
    ///
    /// @param tokens — the token list
    /// @param key — the variant name
    /// @param js_style — `true` for JavaScript (`Key: { … }`), `false`
    ///   for Rust (`Key { … }`)
    /// @returns the key list, or `None` when the variant is not found
    fn variant_keys(tokens: &[String], key: &str, js_style: bool) -> Option<Vec<String>> {
        let mut idx = 0usize;
        while idx < tokens.len() {
            if tokens[idx] == key {
                // ⚠️ A Rust **declaration** is not preceded by `::` ⇒ a
                // `Type::Variant` construction (or a path) is skipped.
                let is_path = idx > 0 && tokens[idx - 1] == ":";
                let mut next = idx + 1;
                if js_style {
                    // `Key: {` — the colon belongs to the object literal.
                    if next < tokens.len() && tokens[next] == ":" {
                        next += 1;
                    } else {
                        idx += 1;
                        continue;
                    }
                }
                if !is_path && next < tokens.len() && tokens[next] == "{" {
                    return Some(block_keys(tokens, next));
                }
            }
            idx += 1;
        }
        None
    }

    /// Collect the top-level keys of the block whose `{` is at `open`.
    fn block_keys(tokens: &[String], open: usize) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        let mut depth = 0i32;
        let mut parens = 0i32;
        let mut brackets = 0i32;
        let mut at_key_position = false;
        let mut i = open;
        while i < tokens.len() {
            let token = tokens[i].as_str();
            match token {
                "{" => {
                    depth += 1;
                    at_key_position = true;
                }
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    at_key_position = false;
                }
                "(" => parens += 1,
                ")" => parens -= 1,
                "[" => brackets += 1,
                "]" => brackets -= 1,
                "," => {
                    // ⚠️ A comma **inside** a call or an index expression
                    // does not separate two keys.
                    if parens == 0 && brackets == 0 {
                        at_key_position = true;
                    }
                }
                "#" => {
                    // ⚠️ A Rust attribute (`#[serde(default)]`) sits
                    // between the `,` and the field ⇒ skip it as a unit
                    // and **keep** the key position.
                    if i + 1 < tokens.len() && tokens[i + 1] == "[" {
                        let mut j = i + 1;
                        let mut inner = 0i32;
                        while j < tokens.len() {
                            match tokens[j].as_str() {
                                "[" => inner += 1,
                                "]" => {
                                    inner -= 1;
                                    if inner == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            j += 1;
                        }
                        i = j;
                    }
                }
                _ => {
                    if at_key_position && parens == 0 && brackets == 0 && is_identifier(token) {
                        keys.push(token.to_string());
                        at_key_position = false;
                    }
                }
            }
            i += 1;
        }
        keys
    }

    /// **v7** — the three payload shapes must be **the same on both
    /// sides**, field by field, in the same order.
    ///
    /// ⚠️ **Both sides are derived from their own source** ⇒ this is a
    /// real cross-layer check, not a copied truth: the Rust side is read
    /// out of **this very file**, the JavaScript side out of the JS
    /// mirror. ⇒ a field added on one side only shows up here.
    ///
    /// ⚠️ **The order is part of the contract** ⇒ it is compared too.
    ///
    /// ⚠️ **The front gate does the same thing** (`front/tests/contract.test.mjs`
    /// derives the expected shapes from the Rust source) ⇒ this one is
    /// the Rust-side half of the same check, and it fires even when the
    /// front gate is not run.
    #[test]
    fn the_v7_payload_shapes_are_the_same_on_both_sides() {
        // ⚠️ 路径：`crates/contract/src/protocol.rs` → 仓库根是 `../../..`
        const JS_MIRROR: &str = include_str!("../../../front/lib/message.js");
        // ⚠️ 路径：相对于**本文件** ⇒ 就是本文件自己。
        const RUST_SOURCE: &str = include_str!("protocol.rs");

        let rust_tokens = tokenize(RUST_SOURCE, false);
        let js_tokens = tokenize(JS_MIRROR, true);

        for variant in ["SetMemoryMap", "PatchMemoryMap", "MemoryMap"] {
            let rust_keys = variant_keys(&rust_tokens, variant, false)
                .unwrap_or_else(|| panic!("Rust 源码里必须能找到变体 `{variant}`"));
            let js_keys = variant_keys(&js_tokens, variant, true)
                .unwrap_or_else(|| panic!("JS 镜像里必须能找到 `{variant}`"));
            assert_eq!(
                js_keys, rust_keys,
                "`{variant}` 的线上形状两侧必须**逐字段同形且同序**（Rust 侧 {rust_keys:?} / JS 侧 {js_keys:?}）"
            );
        }

        // ⚠️ `UnloadProject` 是 **unit 变体** ⇒ 线上形态是**裸字符串**
        // （不是 `{UnloadProject: {}}`）⇒ Rust 侧用 serde 说，JS 侧用
        // 「必须有一个**字符串字面量** `"UnloadProject"`」说。
        assert_eq!(
            serde_json::to_value(CommandPayload::UnloadProject).expect("json"),
            serde_json::Value::String("UnloadProject".to_string()),
            "`UnloadProject` 是 unit 变体 ⇒ 线上必须是裸字符串"
        );
        assert!(
            js_tokens.iter().any(|t| t == "\"UnloadProject\""),
            "JS 镜像里必须**字面**出现字符串 `\"UnloadProject\"`（unit 变体的线上形态）"
        );
    }

    // -- STAGE-42 §6.1 · A-2 ~ A-5（v0.57 批的契约层判据） -------------------

    /// **A-2**：两条新命令的**线上形状**逐字钉住。
    ///
    /// ⚠️ 判据不许是空的 ⇒ ⇒ 这里断言的是**完整的 JSON 值**（不是「有
    /// 这个键」）⇒ ⇒ 键名改一个字就红。
    #[test]
    fn a2_the_two_new_commands_have_their_wire_shapes_pinned() {
        assert_eq!(
            serde_json::to_value(CommandPayload::ResolveSymbol {
                name: "main".to_string()
            })
            .expect("json"),
            serde_json::json!({"ResolveSymbol": {"name": "main"}}),
        );
        assert_eq!(
            serde_json::to_value(CommandPayload::DumpMemory { addr: 0, len: 16 }).expect("json"),
            serde_json::json!({"DumpMemory": {"addr": 0, "len": 16}}),
        );
        // ⚠️ 反向也要钉住：线上那串 JSON 必须能**解析回**同一个变体。
        let back: CommandPayload =
            serde_json::from_value(serde_json::json!({"ResolveSymbol": {"name": "main"}}))
                .expect("parse");
        assert_eq!(
            back,
            CommandPayload::ResolveSymbol {
                name: "main".to_string()
            }
        );
        let back: CommandPayload =
            serde_json::from_value(serde_json::json!({"DumpMemory": {"addr": 0, "len": 16}}))
                .expect("parse");
        assert_eq!(back, CommandPayload::DumpMemory { addr: 0, len: 16 });
    }

    /// **A-3**：两条新事件的**线上形状**逐字钉住。
    ///
    /// ⚠️ `MemoryDump.bytes` 是 **JSON 数组**（`[104,105]`）⇒ ⇒ **不是**
    /// base64、**不是**字符串（与 `Console` 事件同口径 · F-11）。
    #[test]
    fn a3_the_two_new_events_have_their_wire_shapes_pinned() {
        assert_eq!(
            serde_json::to_value(EventPayload::SymbolResolved {
                name: "main".to_string(),
                symbol: None,
                candidates: Vec::new(),
                total: 0,
            })
            .expect("json"),
            serde_json::json!({
                "SymbolResolved": {
                    "name": "main",
                    "symbol": null,
                    "candidates": [],
                    "total": 0
                }
            }),
        );
        assert_eq!(
            serde_json::to_value(EventPayload::MemoryDump {
                addr: 0,
                bytes: vec![104, 105],
            })
            .expect("json"),
            serde_json::json!({"MemoryDump": {"addr": 0, "bytes": [104, 105]}}),
        );
    }

    /// **A-4**：`Breakpoint.condition` 的线上形态。
    ///
    /// * `None` ⇒ 线上**没有**这个键；
    /// * `Some("x1 == 5")` ⇒ 线上是**字符串**。
    #[test]
    fn a4_the_breakpoint_condition_field_is_optional_on_the_wire() {
        let without = serde_json::to_value(Breakpoint {
            id: 1,
            addr: 0x8000_0000,
            source_hint: None,
            enabled: true,
            condition: None,
        })
        .expect("json");
        assert!(
            without.get("condition").is_none(),
            "`condition == None` ⇒ 线上**没有**这个键：{without:?}"
        );

        let with = serde_json::to_value(Breakpoint {
            id: 1,
            addr: 0x8000_0000,
            source_hint: None,
            enabled: true,
            condition: Some("x1 == 5".to_string()),
        })
        .expect("json");
        assert_eq!(
            with.get("condition"),
            Some(&serde_json::Value::String("x1 == 5".to_string())),
            "`Some(..)` ⇒ 线上是**字符串**"
        );
    }

    /// **A-5**：**旧载荷仍能解析** ⇒ ⇒ 证明 `#[serde(default)]` 生效 ⇒ ⇒
    /// **不是**破坏性格式变更（裁决 A）。
    #[test]
    fn a5_an_old_breakpoint_payload_without_the_condition_still_parses() {
        let old = serde_json::json!({
            "id": 7,
            "addr": 2147483648u64,
            "source_hint": null,
            "enabled": true
        });
        let bp: Breakpoint = serde_json::from_value(old).expect("旧载荷必须仍能解析");
        assert_eq!(bp.id, 7);
        assert_eq!(bp.addr, 0x8000_0000);
        assert!(bp.enabled);
        assert_eq!(bp.condition, None, "缺这个字段 ⇒ `condition == None`");

        // ⚠️ 同一条判据的**命令侧**：旧的 `SetBreakpoint` 载荷也能解析。
        let old_cmd = serde_json::json!({
            "SetBreakpoint": {
                "id": 7,
                "addr": 2147483648u64,
                "source_hint": null,
                "enabled": true
            }
        });
        let cmd: CommandPayload = serde_json::from_value(old_cmd).expect("旧命令载荷必须仍能解析");
        match cmd {
            CommandPayload::SetBreakpoint(bp) => assert_eq!(bp.condition, None),
            other => panic!("必须仍是 SetBreakpoint，实际 {other:?}"),
        }
    }
}
