//! Unified command / event protocol (LLD §3.2.1, AD §3).
//!
//! Every interaction between the front-end, server, and kernel flows as
//! a [`Message`]. Commands travel host → kernel; events travel kernel → host.

use serde::{Deserialize, Serialize};

use crate::model::{
    Breakpoint, DeltaRegister, DisasmEntryDto, Project, SimState, TraceFormat, TraceSource,
    Watchpoint, WaveformHistory, WriteSize,
};
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

/// Reserved admin / sysadmin operations (M-22 + M-23 + M-21). Always
/// gated by an admin role check on the receiving side.
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
    /// **v3** (STAGE-5) adds the M-11 debugging surface.
    #[test]
    fn the_schema_version_is_three() {
        assert_eq!(crate::SCHEMA_VERSION, 3);
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
        let msg = Message::command(7, CommandPayload::Nop);
        assert_eq!(msg.schema_version, crate::SCHEMA_VERSION);
        let json = serde_json::to_value(&msg).expect("json");
        assert_eq!(json["schema_version"], 3);
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
            }],
        };
        assert_eq!(
            serde_json::to_value(&disasm).expect("json"),
            serde_json::json!({ "Disasm": { "entries": [
                { "addr": 4096, "code": 267387027,
                  "mnemonic": "addi x1, x0, 255", "width": 4 } ] } })
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
}
