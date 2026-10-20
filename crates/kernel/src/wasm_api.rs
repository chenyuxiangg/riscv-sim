//! WASM export surface for the browser front-end.
//!
//! **Status: 第 1 阶段。** The goal here is narrow and deliberate: make the
//! documented WASM build path (`tools/build-wasm.sh`) actually *work*, prove
//! the JavaScript ↔ WASM boundary behaves (string marshalling, UTF-8,
//! no accidental copies of the kernel's state), and expose the **protocol
//! bridge** the browser Worker drives.
//!
//! ## 桥的形状
//!
//! `process_message(json) -> json` 是**唯一**的会话入口：
//!
//! - **入参**：一个契约 [`riscv_sim_contract::protocol::Message`] 的 JSON
//!   文本，`direction` 必须是 `"Command"`；
//! - **出参**：一个 JSON **数组**，元素是契约 `Message`，`direction` 是
//!   `"Event"`。
//!
//! 形状由 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` 冻结。
//!
//! ## 会话状态
//!
//! 内核在 WASM 实例内**保留一个会话**（寄存器 / 内存图 / 断点表 / 订阅表）。
//! 它住在 [`thread_local!`] 里——workspace 禁 `unsafe`（`unsafe_code =
//! "forbid"`），而 WASM 的「全局变量」在 Rust 里必须靠 `static mut` 或
//! `unsafe` 才能拿到，两者都不可用。`thread_local!` + `RefCell` 是这个约束
//! 下唯一干净的写法：它不需要 `unsafe`，也不会在多线程下产生别名问题
//! （WASM 本来就是单线程）。
//!
//! ## 绝不 panic
//!
//! WASM 里 panic = 整个 Worker 崩（`panic = "abort"`）。所以这条路径上
//! **没有** `unwrap` / `expect` / 显式 `panic!`——畸形输入由
//! [`crate::kernel_host::KernelHost::handle_json`] 翻译成一条 `E_PROTO-08`
//! 错误事件。
//!
//! ## Target gating
//!
//! The whole module is `#[cfg(target_arch = "wasm32")]`, and
//! `wasm-bindgen` is a **target-specific dependency**. Native builds
//! never compile this file and never pull `wasm-bindgen` into their
//! dependency graph — the kernel's native footprint is unchanged.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::kernel_host::KernelHost;

thread_local! {
    /// The one session this WASM instance hosts.
    ///
    /// `thread_local!` + `RefCell` instead of a `static mut`: the
    /// workspace forbids `unsafe` outright (`unsafe_code = "forbid"`),
    /// and a `static mut` would need `unsafe` to touch. The browser
    /// worker is single-threaded, so a thread-local *is* the global.
    static SESSION: RefCell<KernelHost> = RefCell::new(KernelHost::new());
}

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

/// The protocol bridge: one contract command in, a batch of contract
/// events out.
///
/// # Panics
///
/// Never. A malformed input comes back as an `Error` event
/// (`E_PROTO-08`), never as a panic — a panic would take the browser
/// worker down with it.
#[wasm_bindgen]
pub fn process_message(json: &str) -> String {
    SESSION.with(|session| {
        let mut host = session.borrow_mut();
        host.handle_json(json)
    })
}

/// Drop the current session and start a fresh one.
///
/// A new session means a new memory map, a new register file, no
/// breakpoints and no topic subscriptions. The front end uses it when the
/// user opens a different project.
#[wasm_bindgen]
pub fn reset_session() {
    SESSION.with(|session| {
        let mut host = session.borrow_mut();
        *host = KernelHost::new();
    });
}
