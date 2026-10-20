// End-to-end gate: M-07 transport frame → **real WASM host worker file**
// → WASM bridge → contract message → transport frame.
//
// The file under test is `front/kernel-worker.js` — the very file the
// browser loads. It runs inside a `node:worker_threads` Worker with the
// browser Worker API shimmed on (`node-worker-host-wasm.mjs`), and the
// WASM module is the fake double (`fake-wasm-module.mjs`) injected
// through the worker's `self.__RISCV_WASM_URL__` test seam: the real
// wasm-pack output is gitignored, so the gate must not depend on it.
//
// What is asserted here is the **bridge** (spec §4): the frame →
// message → frame mapping, the reqId placement (first event answers the
// command, the rest are pushes) and the failure paths (a throwing
// kernel, a non-JSON answer, a dead init) — all of which must come
// back as `Error` events instead of taking the worker down.

import { test, describe, after } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { Worker as NodeWorker } from "node:worker_threads";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const workers = [];

/**
 * A browser-shaped Worker backed by `node:worker_threads`, running the
 * real `front/kernel-worker.js` against the fake wasm module.
 *
 * @param {string} mode — the fake module's behaviour mode
 */
function createWasmWorker(mode) {
    const wasmUrl = new URL(`./fake-wasm-module.mjs?mode=${mode}`, import.meta.url).href;
    const worker = new NodeWorker(new URL("./node-worker-host-wasm.mjs", import.meta.url), {
        workerData: { wasmUrl },
    });
    workers.push(worker);

    /** @type {object[]} */
    const frames = [];
    /** @type {Error[]} */
    const errors = [];
    /** @type {Function[]} */
    const waiters = [];

    function notify() {
        for (const waiter of waiters.splice(0)) waiter();
    }

    worker.on("message", (data) => {
        frames.push(data);
        notify();
    });
    // The worker throws when the wasm init fails (spec §4). Without a
    // listener Node would turn that into an uncaught exception in the
    // test process, so the error is captured here.
    worker.on("error", (err) => {
        errors.push(err);
        notify();
    });

    return {
        /** @param {object} frame */
        send(frame) {
            worker.postMessage(frame);
        },
        frames: () => frames.slice(),
        errors: () => errors.slice(),
        /**
         * @param {() => boolean} predicate
         * @param {number} [timeoutMs]
         */
        async waitFor(predicate, timeoutMs = 3000) {
            const start = Date.now();
            while (!predicate()) {
                if (Date.now() - start > timeoutMs) {
                    throw new Error("waitFor 超时：条件一直不成立");
                }
                await new Promise((r) => setTimeout(r, 5));
            }
        },
    };
}

/**
 * A command frame carrying one `CommandPayload` value.
 *
 * @param {object|string} payload — the inner `CommandPayload`
 * @param {{reqId?: number|null, sessionId?: string|null, role?: string|null}} [opts]
 * @returns {object}
 */
function commandFrame(payload, opts = {}) {
    return {
        type: "command",
        seq: 1,
        sessionId: opts.sessionId ?? "sess-1",
        role: opts.role ?? "embedded",
        reqId: opts.reqId ?? 7,
        payload: {
            schema_version: 1,
            direction: "Command",
            seq: 1,
            payload: { Command: payload },
        },
    };
}

after(async () => {
    for (const worker of workers) await worker.terminate();
});

describe("WASM 传输宿主 · 帧 → 消息 → 帧", () => {
    test("Nop 命令回一个事件帧（reqId / 会话 / 角色原样回传）", async () => {
        const host = createWasmWorker("nop");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.type, "event", "回帧类型必须是 event");
        assert.equal(frame.reqId, 7, "首个事件必须带命令的 reqId");
        assert.equal(frame.sessionId, "sess-1", "sessionId 必须原样回传");
        assert.equal(frame.role, "embedded", "role 必须原样回传");
        assert.equal(frame.payload.direction, "Event", "payload 必须是契约事件消息");
        assert.deepEqual(
            frame.payload.payload,
            { Event: { Tick: { elapsed_ps: 0, pc: 0 } } },
            "事件负载不是内核给出的契约形状",
        );
        // The frame's seq is the event's own seq (spec §4).
        assert.equal(frame.seq, frame.payload.seq, "帧 seq 必须等于事件的 seq");
    });

    test("首个事件带 reqId，后续事件带 null（推送类）", async () => {
        const host = createWasmWorker("two");
        host.send(commandFrame("Nop", { reqId: 42 }));
        await host.waitFor(() => host.frames().length >= 2);

        const [first, second] = host.frames();
        assert.equal(first.reqId, 42, "首个事件必须带 reqId");
        assert.equal(second.reqId, null, "后续事件必须带 null");
        assert.equal(second.sessionId, "sess-1", "推送事件也要带会话");
        assert.equal(second.role, "embedded", "推送事件也要带角色");
        assert.equal(
            second.payload.payload.Event.Halted.reason,
            "Breakpoint",
            "第二个事件没有原样传回内核的事件负载",
        );
    });

    test("内核抛错 → 回一个 E_TRX-04 的 Error 事件", async () => {
        const host = createWasmWorker("throw");
        host.send(commandFrame("Nop", { reqId: 9 }));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.type, "event");
        assert.equal(frame.reqId, 9, "错误帧必须带命令的 reqId");
        assert.equal(frame.payload.direction, "Event");
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-04");
        assert.match(
            frame.payload.payload.Event.Error.message,
            /内核炸了/,
            "错误消息必须带上异常文本",
        );
    });

    test("内核返回非法 JSON → 回一个 E_TRX-04 的 Error 事件", async () => {
        const host = createWasmWorker("bad-json");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-04");
        assert.match(frame.payload.payload.Event.Error.message, /JSON/i);
    });

    test("内核返回的不是数组 → 回一个 E_TRX-04 的 Error 事件", async () => {
        const host = createWasmWorker("not-array");
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-04");
        assert.match(frame.payload.payload.Event.Error.message, /数组/);
    });

    test("非命令类型的帧被忽略（不产生回帧）", async () => {
        const host = createWasmWorker("nop");
        host.send({
            type: "event",
            seq: 3,
            sessionId: "sess-1",
            role: "embedded",
            reqId: null,
            payload: {
                schema_version: 1,
                direction: "Event",
                seq: 3,
                payload: { Event: { Tick: { elapsed_ps: 0, pc: 0 } } },
            },
        });
        await new Promise((r) => setTimeout(r, 200));
        assert.equal(host.frames().length, 0, "非命令帧不该产生回帧");
    });

    test("畸形帧被丢弃（不产生回帧，也不打挂 worker）", async () => {
        const host = createWasmWorker("nop");
        host.send({ nonsense: true });
        await new Promise((r) => setTimeout(r, 200));
        assert.equal(host.frames().length, 0, "畸形帧不该产生回帧");

        // The worker is still alive: a valid command still works.
        host.send(commandFrame("Nop"));
        await host.waitFor(() => host.frames().length >= 1);
        assert.equal(host.frames()[0].type, "event");
    });

    test("WASM 初始化失败 → 回一个 E_TRX-04 的 Error 事件", async () => {
        const host = createWasmWorker("init-fail");
        // No command is sent: the failure must be reported on its own.
        await host.waitFor(() => host.frames().length >= 1);

        const frame = host.frames()[0];
        assert.equal(frame.payload.payload.Event.Error.code, "E_TRX-04");
        assert.match(
            frame.payload.payload.Event.Error.message,
            /初始化失败/,
            "初始化失败的错误消息必须说明原因",
        );
    });
});

describe("WASM 传输宿主 · import 安全性", () => {
    test("在 Node 主线程里 import 无副作用、不抛错", async () => {
        assert.equal(
            typeof globalThis.self,
            "undefined",
            "Node 主线程里没有 self 全局（守卫的前提）",
        );
        const mod = await import(pathToFileURL(resolve(frontDir, "kernel-worker.js")).href);
        assert.ok(mod, "kernel-worker.js 未能被 import");
        assert.equal(
            typeof globalThis.self,
            "undefined",
            "import 不该在全局作用域留下任何痕迹",
        );
    });
});
