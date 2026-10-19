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

// MVP: docs on every public variant field are TODO. Once the protocol
// shape stabilises (post-M0), re-tighten to `deny(missing_docs)`.
#![warn(missing_docs)]

pub mod error;
pub mod model;
pub mod port;
pub mod protocol;

/// Protocol schema version. Bump on every breaking change; consumers
/// negotiate via the `schema_version` field in every [`protocol::Message`].
pub const SCHEMA_VERSION: u32 = 1;
