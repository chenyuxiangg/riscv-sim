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

// MVP: docs on every public variant field are TODO. PROGRESS.md §7
// 2026-10-19 decision: workspace `missing_docs = "allow"` until each
// public item is documented, then re-tighten to `deny`.

pub mod error;
pub mod model;
pub mod port;
pub mod protocol;
pub mod snapshot;

/// Protocol schema version. Bump on every breaking change; consumers
/// negotiate via the `schema_version` field in every [`protocol::Message`].
pub const SCHEMA_VERSION: u32 = 1;
