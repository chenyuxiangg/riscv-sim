// M-07 传输适配器 gate.
//
// The transport is the only module that touches the outside world, so
// the gate drives it through **injected** adapters: a fake Worker (the
// browser shape) and a fake WebSocket. That keeps the test
// deterministic — no real sockets, no real threads — while still
// exercising the framing, the status machine and the error paths.
//
// The end-to-end path (real worker file + real mock kernel) lives in
// `mock-kernel.test.mjs`.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));
void dom;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m07 = await importFront("services/m07_transport.js");
const { commandMessage, eventMessage, Command, Event } = await importFront("lib/message.js");

/** A fake Worker with the browser's event shape. */
function createFakeWorker() {
    /** @type {Map<string, Set<Function>>} */
    const listeners = new Map();
    const worker = {
        sent: [],
        terminated: false,
        postMessage(data) {
            worker.sent.push(data);
        },
        addEventListener(type, handler) {
            let set = listeners.get(type);
            if (!set) {
                set = new Set();
                listeners.set(type, set);
            }
            set.add(handler);
        },
        terminate() {
            worker.terminated = true;
        },
        emit(type, data) {
            const set = listeners.get(type);
            if (!set) return;
            for (const fn of [...set]) fn({ data });
        },
    };
    return worker;
}

/** A fake WebSocket with the browser's callback shape. */
class FakeWebSocket {
    constructor(url) {
        FakeWebSocket.last = this;
        this.url = url;
        this.sent = [];
        this.readyState = 0;
    }

    send(data) {
        this.sent.push(data);
    }

    close() {
        this.readyState = 3;
        this.onclose?.({});
    }
}

describe("M-07 传输适配器 · worker 形态", () => {
    test("挂载后状态为 worker/ready/connected", async () => {
        const worker = createFakeWorker();
        const status = await m07.mountTransport({
            mode: "worker",
            workerUrl: "./dev/mock-kernel-worker.js",
            createWorker: () => worker,
        });
        assert.equal(status.mode, "worker");
        assert.equal(status.connected, true);
        assert.equal(status.connState, "ready");
        assert.equal(status.lastError, null);
    });

    test("状态变化会推给订阅者（不是一次性快照）", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        const seen = [];
        const off = m07.onStatus((st) => seen.push(st.connState));
        m07.bindSession("sess-x", "embedded");
        worker.emit("error", { message: "boom" });
        off();
        assert.ok(seen.includes("ready"), "绑定会话没有触发状态变更");
        assert.equal(seen[seen.length - 1], "closed", "Worker 错误没有把状态置为 closed");
    });

    test("退订后不再收到状态变更", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        let hits = 0;
        const off = m07.onStatus(() => { hits += 1; });
        m07.bindSession("sess-y");
        const after = hits;
        off();
        m07.bindSession("sess-z");
        assert.equal(hits, after, "退订后仍收到状态变更");
    });

    test("出站命令被包成传输帧（带会话 / 角色 / 关联 id）", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        m07.bindSession("sess-1", "product");
        const seq = m07.sendCommand(commandMessage(1, Command.stepN(2)), { reqId: 42 });
        assert.equal(worker.sent.length, 1);
        const frame = worker.sent[0];
        assert.equal(frame.type, "command");
        assert.equal(frame.seq, seq);
        assert.equal(frame.sessionId, "sess-1");
        assert.equal(frame.role, "product");
        assert.equal(frame.reqId, 42);
        // The payload is the frozen contract envelope, untouched.
        assert.equal(frame.payload.schema_version, 1);
        assert.equal(frame.payload.direction, "Command");
        assert.deepEqual(frame.payload.payload, { Command: { StepN: { n: 2 } } });
    });

    test("入站帧被解包后交给消息订阅者", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        const seen = [];
        const off = m07.onMessage((frame) => seen.push(frame));
        worker.emit("message", {
            type: "event",
            seq: 1,
            sessionId: "sess-1",
            role: "embedded",
            reqId: 7,
            payload: eventMessage(1, Event.tick(3, 4)),
        });
        off();
        assert.equal(seen.length, 1);
        assert.equal(seen[0].reqId, 7);
        assert.equal(seen[0].payload.direction, "Event");
    });

    test("畸形入站帧只记错误，不会把传输层打断", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        let delivered = 0;
        const off = m07.onMessage(() => { delivered += 1; });
        worker.emit("message", { nonsense: true });
        off();
        assert.equal(delivered, 0);
        assert.match(m07.getTransportStatus().lastError, /E_PROTO-08/);
        assert.equal(m07.getTransportStatus().connected, true, "畸形帧不应断开通道");
    });

    test("没有 Worker 的环境（Node 门禁）明确报「未连接」而不抛异常", async () => {
        // No `createWorker` → the default factory hits the missing
        // global. The mount must degrade, not explode.
        const status = await m07.mountTransport({
            mode: "worker",
            workerUrl: "./dev/mock-kernel-worker.js",
        });
        assert.equal(status.connected, false);
        assert.equal(status.connState, "closed");
        assert.match(status.lastError, /E_TRX-02/);
    });

    test("默认挂载指向 WASM 内核 worker（不再是 mock worker）", async () => {
        // No `workerUrl` and no `createWorker` → the default factory
        // hits the missing `Worker` global and the error message carries
        // the URL it tried to load.
        const status = await m07.mountTransport({ mode: "worker" });
        assert.match(status.lastError, /\.\/kernel-worker\.js/, "默认 workerUrl 不是 WASM 内核");
        assert.equal(status.label, "wasm worker", "默认 label 不是 wasm worker");
    });

    test("未连接时发命令抛 E_TRX-01", async () => {
        await m07.mountTransport({ mode: "worker" });
        assert.throws(() => m07.sendCommand(commandMessage(1, Command.nop())), /E_TRX-01/);
    });

    test("close() 关闭通道并终止 Worker", async () => {
        const worker = createFakeWorker();
        await m07.mountTransport({ mode: "worker", createWorker: () => worker });
        const status = m07.close();
        assert.equal(status.connected, false);
        assert.equal(status.connState, "closed");
        assert.equal(worker.terminated, true);
    });
});

describe("M-07 传输适配器 · websocket 形态（P1 骨架）", () => {
    test("没有 URL 时明确处于「未连接」", async () => {
        const status = await m07.mountTransport({ mode: "websocket" });
        assert.equal(status.mode, "websocket");
        assert.equal(status.connected, false);
        assert.equal(status.connState, "closed");
        assert.match(status.lastError, /E_TRX-03/);
    });

    test("连接成功后状态转为 ready，收到消息即解包", async () => {
        const original = globalThis.WebSocket;
        globalThis.WebSocket = FakeWebSocket;
        try {
            const status = await m07.mountTransport({ mode: "websocket", url: "ws://example.invalid" });
            assert.equal(status.connected, false, "握手前不应算已连接");
            const socket = FakeWebSocket.last;
            socket.onopen?.({});
            assert.equal(m07.getTransportStatus().connected, true);
            assert.equal(m07.getTransportStatus().connState, "ready");

            const seen = [];
            const off = m07.onMessage((frame) => seen.push(frame));
            socket.onmessage?.({
                data: JSON.stringify({
                    type: "event",
                    seq: 1,
                    sessionId: null,
                    role: null,
                    reqId: 1,
                    payload: eventMessage(1, Event.halted("Trap", 0x40)),
                }),
            });
            off();
            assert.equal(seen.length, 1);
            assert.equal(seen[0].payload.payload.Event.Halted.reason, "Trap");
        } finally {
            globalThis.WebSocket = original;
        }
    });

    test("socket 关闭后状态回到 closed", async () => {
        const original = globalThis.WebSocket;
        globalThis.WebSocket = FakeWebSocket;
        try {
            await m07.mountTransport({ mode: "websocket", url: "ws://example.invalid" });
            const socket = FakeWebSocket.last;
            socket.onopen?.({});
            socket.onclose?.({});
            assert.equal(m07.getTransportStatus().connected, false);
            assert.equal(m07.getTransportStatus().connState, "closed");
        } finally {
            globalThis.WebSocket = original;
        }
    });
});
