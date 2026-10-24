//! Platform shared contracts (LLD §2.5).
//!
//! Every cross-module interaction in `riscv_sim_kernel` / `riscv_sim_server` /
//! the JS front-end must flow through a type defined in this crate. The
//! inner implementation language of each module is unconstrained, but the
//! `driving` / `driven` port signatures, DTOs, event tags, and error codes
//! are part of the public protocol and must remain stable across refactors.
//!
//! Sub-modules:
//! - [`model`] — the five core data models + the P1 Interconnect Link.
//! - [`protocol`] — unified command/event message envelope.
//! - [`error`] — error code family + per-subsystem scenario codes.
//! - [`port`] — driving / driven port signatures for ExecutionBackend,
//!   Peripheral, debug services, and the observability pipeline.
//! - [`session`] — the **session-level** snapshot payload (M-20 / M-23
//!   §2.1.1): checkpoint metadata + the M-12/M-17 engine segment + memory
//!   pages + the memory-map topology + peripheral state + raised IRQ lines
//!   (debt ledger D-36 / D-37 / D-41). Frozen spec:
//!   `docs/detailed-design/STAGE-3_Session_Snapshot.md`.
//! - [`snapshot`] — serializable snapshot DTOs carrying the pending event
//!   queue, so the **engine's** scheduler state can be persisted *and*
//!   restored deterministically (debt ledger D-24). Peripheral models
//!   (M-15) and the interrupt controller (M-16) are **not** part of it.
//! - [`numstr`] — decimal-string transport for the virtual-time fields
//!   (contract v2 spec §2).
//! - [`platform`] — the **platform description** (v0.27 · D-123-B): the
//!   declared memory map (regions + peripheral devices) plus the
//!   **single** validator for it. Frozen spec:
//!   `docs/detailed-design/STAGE-12_V027_Platform_Description.md` §1.
//! - [`csr`] — the machine-mode CSR vocabulary and the platform's
//!   interrupt-line space (CSR numbers · `mie` bit positions · the
//!   line → `mie` bit mapping). Frozen spec:
//!   `docs/detailed-design/STAGE-4_Interrupt_Controller.md` §2.1.
//!
//! # Schema version
//!
//! [`SCHEMA_VERSION`] was `1` from v0.10 until contract v2. The v2
//! upgrade is the **first** time the contract changed shape, and the
//! frozen spec for it is
//! `docs/detailed-design/STAGE-2_Contract_V2.md` — read that file
//! before changing anything in this crate: every variant name, field
//! name and wire representation listed there is **frozen**.
//!
//! Contract **v3** adds the M-11 debugging surface (disassembly,
//! source lookup, trace export). Its frozen spec is
//! `docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md` §3 — the
//! same rule applies: every variant name, field name and wire
//! representation listed there is **frozen**.
//!
//! Contract **v4** adds the guest **console** byte stream and the
//! per-instruction source location on the disassembly entries:
//!
//! - new event `EventPayload::Console { bytes }` — the bytes the guest
//!   wrote to the console UART. **Bytes, not a string**: the guest may
//!   emit arbitrary bytes (ANSI escapes, `\r`, a half-written UTF-8
//!   sequence) and the byte stream is the truth (the Spike comparison is
//!   byte-wise). serde's default for `Vec<u8>` is a **JSON array of
//!   numbers** — not base64, not a string;
//! - two **optional** DTO fields on `model::DisasmEntryDto` (`file` /
//!   `line`) — the per-instruction source location (debt ledger D-102).
//!   `None` is a **legal** value and means "no line table / not hit";
//!   the fields are **never** fabricated.
//!
//! Contract **v7** adds the **board delta / unload / budget split** batch
//! (v0.31 · D-137 / D-138 / D-139):
//!
//! - new command `CommandPayload::UnloadProject` — **unload** the project
//!   (clear the *running* state; the image bytes and the user's board
//!   both stay ⇒ re-loading does not need the file picker again);
//! - new command `CommandPayload::PatchMemoryMap { ops, expected_revision }`
//!   — the **atomic delta** update of the board memory map (located by
//!   `base` ⇒ independent of the array order);
//! - `CommandPayload::SetMemoryMap` gains `expected_revision` — the
//!   **optional** expected revision ⇒ a submission based on a stale
//!   revision is **rejected** (`E_PLT_13`) instead of silently overwriting
//!   somebody else's change;
//! - new enum `protocol::MemoryMapOp` — the delta operations
//!   (`UpsertRegion` / `RemoveRegion` / `SetBudget`). ⚠️ **serde's
//!   default external tagging** (same shape as `CommandPayload`) ⇒ the
//!   wire keys are the Rust variant names;
//! - `platform::PlatformDescription` gains `max_region_bytes` — the
//!   **runtime** physical-occupancy ceiling, split away from
//!   `region_budget_bytes` (the **declaration-time** ceiling). Absent ⇒
//!   it **equals** the declaration-time one ⇒ old descriptions behave
//!   **byte for byte** as before ⇒ **the description format version does
//!   not move** (`PLATFORM_SCHEMA_VERSION` stays `1`);
//! - new event `EventPayload::MemoryMap { description, pending, loaded,
//!   revision }` — the board memory map's **complete** state.
//!
//! Contract **v6** adds the **front-end configurable board memory map**
//! (v0.29 · D-124):
//!
//! - new command `CommandPayload::SetMemoryMap { description }` — the
//!   board the user configured **in the front end**. It carries a
//!   **whole** [`platform::PlatformDescription`] — the description DTO
//!   the contract layer **already has** ⇒ **zero new DTO** — and **not**
//!   a patch ⇒ there is no "current value + patch" second truth;
//! - new event `EventPayload::MemoryMap { description, pending }` — the
//!   **currently effective** map (the front end's memory-map table's
//!   data source ⇒ the mock table becomes real) plus a flag telling
//!   whether the host holds a **pending** board configuration;
//! - ⚠️ v6 changes **nothing else**: no DTO change, no renumbering. The
//!   v5 shapes stay exactly as they were.
//!
//! Contract **v5** adds the **console input** direction:
//!
//! - new command `CommandPayload::WriteConsole { bytes }` — the bytes
//!   the host writes **into** the console UART (the RX direction).
//!   ⚠️ **Bytes, not a string**: the guest's byte stream is the truth
//!   in **both** directions, so this command is the exact mirror of
//!   v4's `EventPayload::Console { bytes }` (which is the TX
//!   direction). serde's default for `Vec<u8>` is a **JSON array of
//!   numbers** — not base64, not a string;
//! - ⚠️ v5 changes **nothing else**: no new event, no DTO change, no
//!   renumbering. The v4 shapes stay exactly as they were.

// MVP: docs on every public variant field are TODO. PROGRESS.md §7
// 2026-10-19 decision: workspace `missing_docs = "allow"` until each
// public item is documented, then re-tighten to `deny`.

pub mod csr;
pub mod error;
pub mod model;
pub mod numstr;
pub mod platform;
pub mod port;
pub mod protocol;
pub mod session;
pub mod snapshot;

/// Protocol schema version. Bump on every breaking change; consumers
/// negotiate via the `schema_version` field in every [`protocol::Message`].
///
/// **v7** (contract v7 — the board delta / unload / budget split batch,
/// v0.31 · D-137 / D-138 / D-139):
/// - new command `CommandPayload::UnloadProject` — **unload** the project
///   (clear the *running* state; the image bytes and the user's board
///   both stay);
/// - new command `CommandPayload::PatchMemoryMap { ops, expected_revision }`
///   — the **atomic delta** update of the board memory map;
/// - `CommandPayload::SetMemoryMap` gains the **optional**
///   `expected_revision` ⇒ a stale submission is **rejected**
///   (`E_PLT_13`) instead of silently overwriting;
/// - new enum `protocol::MemoryMapOp` (`UpsertRegion` / `RemoveRegion` /
///   `SetBudget`) — ⚠️ **serde's default external tagging** ⇒ the wire
///   keys are the Rust variant names;
/// - `platform::PlatformDescription` gains `max_region_bytes` (the
///   **runtime** ceiling; absent ⇒ it **equals**
///   `region_budget_bytes`) ⇒ ⚠️ the **description format version does
///   not move** (`PLATFORM_SCHEMA_VERSION` stays `1`: the format only
///   **gained** one field with a default ⇒ an old description is still
///   legal ⇒ raising it would invalidate every board stored by v0.30);
/// - new event shape `EventPayload::MemoryMap { description, pending,
///   loaded, revision }` — the board memory map's **complete** state.
///
/// **v6** (contract v6 — the front-end configurable board memory map):
/// - new command `CommandPayload::SetMemoryMap { description }` — the
///   board the user configured **in the front end**. It carries a
///   **whole** [`platform::PlatformDescription`] (the contract layer
///   **already has** it ⇒ **zero new DTO**), **not** a patch ⇒ there is
///   no second source of truth;
/// - new event `EventPayload::MemoryMap { description, pending }` — the
///   **currently effective** map (the front end's memory-map table's
///   data source) plus a flag telling whether the host holds a
///   **pending** board configuration;
/// - ⚠️ **no other shape changed** in v6: no DTO change, no
///   renumbering. The v5 shapes stay exactly as they were.
///
/// **v5** (contract v5 — the console **input** direction):
/// - new command `CommandPayload::WriteConsole { bytes }` — the bytes
///   the host writes **into** the console UART (RX). serde serialises
///   `Vec<u8>` as a **JSON array of numbers** (not base64, not a
///   string) — the exact mirror of v4's `EventPayload::Console`
///   (which is the TX direction);
/// - ⚠️ **no other shape changed** in v5.
///
/// **v4** (contract v4 — the OSZ runtime batch):
/// - new event `EventPayload::Console { bytes }` — the guest's console
///   (UART TX) byte stream. serde serialises `Vec<u8>` as a **JSON array
///   of numbers** (not base64, not a string);
/// - `model::DisasmEntryDto` gains two **optional** fields: `file` and
///   `line` (the per-instruction source location, debt ledger D-102).
///   `None` is a legal value; it is **never** fabricated.
///
/// The v3 shape is **frozen** by the STAGE-5 spec §3 — read it before
/// changing anything about those variants.
///
/// **v3** (STAGE-5, `docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`):
/// - new commands `Disasm` / `ResolveLine` / `ExportTrace` /
///   `SetTraceEnabled`;
/// - new events `Disasm` / `SourceLocation` / `TraceExport` /
///   `TraceState`;
/// - new DTOs `model::DisasmEntryDto`, `model::TraceSource`,
///   `model::TraceFormat`.
///
/// **v2** (contract v2, spec `docs/detailed-design/STAGE-2_Contract_V2.md`):
/// - the virtual-time fields travel as **decimal strings** (see
///   [`numstr`]) — a `u64` becomes a JSON number, and a JSON number is
///   exact only up to `2^53` ps ≈ 2.5 hours of simulated time;
/// - new commands `ReadRegisters` / `WriteRegister` / `Negotiate` and
///   new events `Registers` / `Negotiated` / `SimStateDelta`;
/// - `model::Waveform` → `model::WaveformHistory` and the matching event
///   variant `WaveformDelta` → `WaveformHistory` (D-34);
/// - `ErrorFamily::Protocol` (`"PROTO"`) — the family the LLD's
///   `E_PROTO-01`..`E_PROTO-08` codes belong to.
pub const SCHEMA_VERSION: u32 = 7;
