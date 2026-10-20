//! WASM export surface for the browser front-end.
//!
//! **Status: 第 0 步的最小可运行面。** The goal here is narrow and
//! deliberate: make the documented WASM build path
//! (`tools/build-wasm.sh`) actually *work*, and prove the
//! JavaScript ↔ WASM boundary behaves (string marshalling, UTF-8,
//! no accidental copies of the kernel's state).
//!
//! The real command/event bridge — a `process_message(json) -> json`
//! entry point that speaks the frozen
//! [`riscv_sim_contract::protocol::Message`] envelope — belongs to
//! the **第 1 阶段** (WASM 导出层 + 前端接真实内核) and is
//! deliberately *not* faked here. A half-implemented bridge that
//! silently drops commands is exactly the failure mode this project
//! has been burned by before (D-24: a snapshot API that looked
//! complete but was not).
//!
//! ## What the front-end may rely on today
//!
//! - [`kernel_version`] — the kernel's version string, for
//!   diagnostics and for the front-end to detect a stale `.wasm`.
//! - [`protocol_schema_version`] — the contract's
//!   [`riscv_sim_contract::SCHEMA_VERSION`], so the front-end can
//!   refuse a kernel whose protocol it does not understand instead of
//!   mis-parsing messages.
//! - [`ping`] — echoes a string back. It proves the JS↔WASM string
//!   boundary; it is **not** a simulation entry point.
//!
//! ## Target gating
//!
//! The whole module is `#[cfg(target_arch = "wasm32")]`, and
//! `wasm-bindgen` is a **target-specific dependency**. Native builds
//! never compile this file and never pull `wasm-bindgen` into their
//! dependency graph — the kernel's native footprint is unchanged.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

/// The kernel's version string.
#[wasm_bindgen]
pub fn kernel_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The contract protocol schema version this kernel speaks.
///
/// The front-end's JS mirror (`front/lib/message.js`) carries its own
/// `SCHEMA_VERSION`; comparing the two is how a stale `.wasm` in the
/// browser's cache gets caught instead of mis-parsed.
#[wasm_bindgen]
pub fn protocol_schema_version() -> u32 {
    riscv_sim_contract::SCHEMA_VERSION
}

/// Echo a string back across the JS↔WASM boundary.
///
/// Proves the string marshalling works (UTF-8 round-trip, no
/// truncation) before any real command path depends on it.
#[wasm_bindgen]
pub fn ping(text: &str) -> String {
    format!("pong:{text}")
}
