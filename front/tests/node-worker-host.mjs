// Test-only bridge: runs the **real** browser worker file
// (`front/dev/mock-kernel-worker.js`) inside a `node:worker_threads`
// Worker.
//
// The browser Worker API (`self.addEventListener("message")` +
// `self.postMessage`) is shimmed onto the worker scope, so the file
// that ships to the browser is the very file the gate executes — the
// module graph, the import paths and the frame protocol are all
// covered, not a copy of them.

import { parentPort } from "node:worker_threads";

globalThis.self = {
    addEventListener(type, handler) {
        if (type === "message") {
            parentPort.on("message", (data) => handler({ data }));
        }
    },
    postMessage(data) {
        parentPort.postMessage(data);
    },
};

await import("../dev/mock-kernel-worker.js");
