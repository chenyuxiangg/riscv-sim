// Test-only bridge: runs the **real** browser worker file
// (`front/kernel-worker.js`) inside a `node:worker_threads` Worker.
//
// Same shim as `node-worker-host.mjs`, plus the WASM test seam: the
// worker reads `self.__RISCV_WASM_URL__`, so the host injects the URL
// of the fake wasm module before importing the worker file. The wasm
// build itself is not committed (`front/assets/pkg/` is gitignored), so
// this is what lets the gate run the real worker file on a machine that
// never built the wasm.

import { parentPort, workerData } from "node:worker_threads";

globalThis.self = {
    // Test seam — see `front/kernel-worker.js`.
    __RISCV_WASM_URL__: workerData?.wasmUrl ?? null,
    addEventListener(type, handler) {
        if (type === "message") {
            parentPort.on("message", (data) => handler({ data }));
        }
    },
    postMessage(data) {
        parentPort.postMessage(data);
    },
};

await import("../kernel-worker.js");
