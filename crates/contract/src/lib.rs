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
//! - [`snapshot`] — serializable snapshot DTOs carrying the pending event
//!   queue, so the **engine's** scheduler state can be persisted *and*
//!   restored deterministically (debt ledger D-24). Peripheral models
//!   (M-15) and the interrupt controller (M-16) are **not** part of it.
//! - [`numstr`] — decimal-string transport for the virtual-time fields
//!   (contract v2 spec §2).
//!
//! # Schema version
//!
//! [`SCHEMA_VERSION`] was `1` from v0.10 until contract v2. The v2
//! upgrade is the **first** time the contract changed shape, and the
//! frozen spec for it is
//! `docs/detailed-design/STAGE-2_Contract_V2.md` — read that file
//! before changing anything in this crate: every variant name, field
//! name and wire representation listed there is **frozen**.

// MVP: docs on every public variant field are TODO. PROGRESS.md §7
// 2026-10-19 decision: workspace `missing_docs = "allow"` until each
// public item is documented, then re-tighten to `deny`.

pub mod error;
pub mod model;
pub mod numstr;
pub mod port;
pub mod protocol;
pub mod snapshot;

/// Protocol schema version. Bump on every breaking change; consumers
/// negotiate via the `schema_version` field in every [`protocol::Message`].
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
pub const SCHEMA_VERSION: u32 = 2;
