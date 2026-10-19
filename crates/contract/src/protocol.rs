//! Unified command / event protocol (LLD §3.2.1, AD §3).
//!
//! Every interaction between the front-end, server, and kernel flows as
//! a [`Message`]. Commands travel host → kernel; events travel kernel → host.

use serde::{Deserialize, Serialize};

use crate::model::{Breakpoint, Project, SimState, Watchpoint, Waveform, WriteSize};
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
    /// Run until the next breakpoint, watchpoint, or quantum boundary.
    Continue,
    /// Run exactly `n` ISS instructions.
    StepN {
        n: u32,
    },
    /// Pause the engine at the next quantum boundary.
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
    /// Push waveform edges since the last publish (M-19 incremental push).
    WaveformDelta(Waveform),
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
