// M-07 传输适配器 (Transport Adapter).
// MVP: no transport. The two adapters are:
//   - browser-mode  : postMessage to a Web Worker hosting the WASM
//                      kernel built from `crates/kernel`.
//   - server-mode   : WebSocket to `riscv_sim_server` (P1).
// Both share the same `riscv_sim_contract::protocol::Message`
// envelope; the host picks one at startup.

/**
 * @typedef {object} TransportStatus
 * @property {"none"|"worker"|"websocket"} mode — which adapter is in use
 * @property {boolean} connected           — is a kernel reachable right now
 */

/** @type {TransportStatus} */
let transportStatus = { mode: "none", connected: false };

/**
 * Mount the transport adapter.
 *
 * MVP: neither adapter exists yet, so the status is reported as
 * "not connected". The workbench derives its status-bar text from this
 * descriptor instead of hardcoding it (FRONTEND_Review.md #11) — a
 * hardcoded "kernel 待编译" outlived the kernel and became stale.
 *
 * @returns {Promise<TransportStatus>}
 */
export async function mountTransport() {
    transportStatus = { mode: "none", connected: false };
    return transportStatus;
}

/**
 * The transport state as of the last mount.
 *
 * The front-end gate reads the state from here rather than from a
 * dev-only global (FRONTEND_Review.md R-18).
 *
 * @returns {TransportStatus}
 */
export function getTransportStatus() {
    return transportStatus;
}
