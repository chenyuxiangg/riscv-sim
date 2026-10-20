// Dev-only mock kernel **Worker** entry (browser side).
//
// The Worker hosts the pure-JS mock kernel (`./mock-kernel.js`) and
// relays transport frames. It is loaded with
// `new Worker("./dev/mock-kernel-worker.js", { type: "module" })`.
//
// ⚠️ Same-origin constraint (AGENTS.md): a Worker (like an ES module)
// must be served over HTTP(S). Opening `front/index.html` via `file://`
// makes the browser refuse to load this file.
//
// The `self.document` probe distinguishes a Worker global scope from a
// window: a window has a `document`, a worker does not. The guard keeps
// the module import-safe outside a Worker (the ESM link check in the
// front-end gate imports it, and the node:test harness runs the very
// same file inside a `node:worker_threads` Worker with a `self` shim).

import { createMockKernel } from "./mock-kernel.js";

const kernel = createMockKernel();

if (typeof self !== "undefined" && typeof self.document === "undefined") {
    self.addEventListener("message", (ev) => {
        const frames = kernel.handle(ev?.data);
        for (const frame of frames) {
            self.postMessage(frame);
        }
    });
}
