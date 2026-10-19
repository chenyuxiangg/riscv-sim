// M-07 传输适配器 (Transport Adapter).
// MVP: no transport. The two adapters are:
//   - browser-mode  : postMessage to a Web Worker hosting the WASM
//                      kernel built from `crates/kernel`.
//   - server-mode   : WebSocket to `riscv_sim_server` (P1).
// Both share the same `riscv_sim_contract::protocol::Message`
// envelope; the host picks one at startup.

export async function mountTransport() {
    return Promise.resolve();
}
