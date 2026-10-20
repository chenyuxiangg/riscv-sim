//! RISC-V simulation kernel (AD §3, LLD §4).
//!
//! 14 sub-modules map 1-to-1 to the M-06 / M-08 / M-09 / M-11 / M-12 / M-13 /
//! M-14 / M-15 / M-16 / M-17 / M-18 / M-19 / M-20 / M-21 / M-22 / M-23
//! detailed design. All cross-module communication goes through
//! `riscv_sim_contract`.
//!
//! MVP focus: `m12_engine` (event-driven scheduler) and `m13_iss`
//! (RV32IMC interpreter) — these two together can execute a single
//! `addi` instruction end-to-end. The remaining 14 modules are stubbed
//! with their public port / struct shape and TODO bodies.
//!
//! Note: `unsafe_code = "forbid"` is set at the workspace level.

pub mod m06_elf;
pub mod m08_breakpoint;
pub mod m09_step;
pub mod m11_symbol;
pub mod m12_engine;
pub mod m13_iss;
pub mod m14_bus;
pub mod m15_peripheral;
pub mod m16_interrupt;
pub mod m17_backend;
pub mod m18_probe;
pub mod m19_waveform;
pub mod m20_snapshot;
pub mod m21_logging;
pub mod m22_hms;
pub mod m23_datastore;

/// WASM export surface for the browser front-end (wasm32 only).
///
/// Native builds do not compile this module — see D-38.
#[cfg(target_arch = "wasm32")]
pub mod wasm_api;

/// Crate-wide version. Mirrors `Cargo.toml` for log / event payloads.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
