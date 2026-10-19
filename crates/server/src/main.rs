//! `riscv_sim_server` — P1 server-mode host (LLD §3.3).
//!
//! In MVP this is a placeholder that confirms the workspace builds.
//! The real host will:
//! - host the `riscv_sim_kernel` (native target) over a Tokio runtime;
//! - expose a WebSocket transport (M-07) on `127.0.0.1:9876` by default;
//! - accept POST `/api/project` (multipart) for project uploads;
//! - serve a thin static file proxy for the browser-mode front-end.
//!
//! See AD §6 for the dual-form (WASM / server) physical architecture.

use riscv_sim_contract::error::PlatformError;
use riscv_sim_kernel::VERSION;

fn main() -> Result<(), PlatformError> {
    eprintln!("riscv_sim_server {VERSION} — placeholder (P1).");
    eprintln!("MVP lives in the kernel crate + the browser front-end.");
    eprintln!("The HTTP/WebSocket host will be wired here in milestone P1.");
    Ok(())
}
