// M-05 命令路由与事件分发 gate.
//
// The router is driven through a **fake transport**: the test hands the
// router a transport double and then feeds it frames, so the
// correlation, the timeout and the dispatch rules can be checked
// deterministically (no worker, no threads, no timers longer than the
// test needs).

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

const m05 = await importFront("services/m05_commandrouter.js");
const { commandMessage, eventMessage, Command, Event } = await importFront("lib/message.js");

/** A transport double with the M-07 surface the router uses. */
function createFakeTransport() {
    /** @type {Set<Function>} */
    const messageHandlers = new Set();
    /** @type {Set<Function>} */
    const statusHandlers = new Set();
    const transport = {
        sent: [],
        connected: true,
        onMessage(handler) {
            messageHandlers.add(handler);
            return () => messageHandlers.delete(handler);
        },
        onStatus(handler) {
            statusHandlers.add(handler);
            return () => statusHandlers.delete(handler);
        },
        sendCommand(message, options = {}) {
            transport.sent.push({ message, reqId: options.reqId ?? null });
        },
        getTransportStatus() {
            return { mode: "worker", connected: transport.connected, connState: "ready" };
        },
        /** Deliver a frame to the router. */
        emit(frame) {
            for (const fn of [...messageHandlers]) fn(frame);
        },
        /** Flip the link state. */
        setConnected(value) {
            transport.connected = value;
            for (const fn of [...statusHandlers]) {
                fn(transport.getTransportStatus());
            }
        },
    };
    return transport;
}

/** Build an inbound event frame. */
function eventFrame(reqId, payload, seq = 1) {
    return {
        type: "event",
        seq,
        sessionId: "sess-1",
        role: "embedded",
        reqId,
        payload: eventMessage(seq, payload),
    };
}

describe("M-05 命令路由 · 出站", () => {
    test("命令被包成契约信封并交给传输层", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        router.dispatchCommand(Command.stepN(4)).catch(() => {});
        assert.equal(transport.sent.length, 1);
        const { message, reqId } = transport.sent[0];
        assert.equal(message.schema_version, 1);
        assert.equal(message.direction, "Command");
        assert.deepEqual(message.payload, { Command: { StepN: { n: 4 } } });
        assert.equal(typeof reqId, "number");
        router.close();
    });

    test("响应事件按关联 id 匹配回命令", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const promise = router.dispatchCommand(Command.nop());
        const reqId = transport.sent[0].reqId;
        transport.emit(eventFrame(reqId, Event.tick(42, 0x40)));
        const event = await promise;
        assert.equal(event.variant, "Tick");
        assert.equal(event.value.elapsed_ps, 42);
        router.close();
    });

    test("乱序响应各自归位（按 reqId 配对，不靠到达顺序）", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const first = router.dispatchCommand(Command.stepN(1));
        const second = router.dispatchCommand(Command.stepN(2));
        const [idA, idB] = transport.sent.map((s) => s.reqId);
        // The second answer arrives first.
        transport.emit(eventFrame(idB, Event.tick(2, 0x8)));
        transport.emit(eventFrame(idA, Event.tick(1, 0x4)));
        const [a, b] = await Promise.all([first, second]);
        assert.equal(a.value.elapsed_ps, 1);
        assert.equal(b.value.elapsed_ps, 2);
        router.close();
    });

    test("内核返回 Error 事件时命令被拒绝（错误码透传）", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const promise = router.dispatchCommand(Command.readMemory(0x9000_0000, "Word"));
        const reqId = transport.sent[0].reqId;
        transport.emit(eventFrame(reqId, Event.error("E_BUS-01", "未映射地址")));
        await assert.rejects(promise, /E_BUS-01/);
        router.close();
    });

    test("命令超时按 E_CTX-06 拒绝", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        await assert.rejects(
            router.dispatchCommand(Command.nop(), { timeoutMs: 20 }),
            /E_CTX-06/,
        );
        assert.equal(router.pendingCount(), 0, "超时后待决命令没有清理");
        router.close();
    });

    test("链路断开时在途命令被拒绝", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const promise = router.dispatchCommand(Command.nop());
        transport.setConnected(false);
        await assert.rejects(promise, /E_CTX-06/);
        router.close();
    });

    test("没有挂载传输层时命令直接被拒", async () => {
        const router = await m05.mountCommandRouter({});
        await assert.rejects(router.dispatchCommand(Command.nop()), /E_TRX-01/);
        router.close();
    });
});

describe("M-05 事件分发", () => {
    test("订阅者按 topic 收到事件，退订后不再收到", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        const off = router.subscribe("tick", (event) => seen.push(event));
        transport.emit(eventFrame(null, Event.tick(1, 0)));
        off();
        transport.emit(eventFrame(null, Event.tick(2, 0)));
        assert.equal(seen.length, 1);
        assert.equal(seen[0].value.elapsed_ps, 1);
        router.close();
    });

    test("unsubscribe 按 (topic, handler) 精确退订", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        const handler = (event) => seen.push(event);
        router.subscribe("tick", handler);
        assert.equal(router.unsubscribe("tick", handler), true);
        assert.equal(router.unsubscribe("tick", handler), false);
        transport.emit(eventFrame(null, Event.tick(1, 0)));
        assert.equal(seen.length, 0);
        router.close();
    });

    test("一次性订阅只收第一条", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const promise = router.once("halted");
        transport.emit(eventFrame(null, Event.halted("Breakpoint", 0x40)));
        transport.emit(eventFrame(null, Event.halted("Trap", 0x10)));
        const event = await promise;
        assert.equal(event.value.reason, "Breakpoint");
        router.close();
    });

    test("通配 topic 收到全部事件", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        router.subscribe("*", (event) => seen.push(event.variant));
        transport.emit(eventFrame(null, Event.tick(1, 0)));
        transport.emit(eventFrame(null, Event.halted("Trap", 0)));
        transport.emit(eventFrame(null, Event.memoryRead(0, "Word", 0)));
        assert.deepEqual(seen, ["Tick", "Halted", "MemoryRead"]);
        router.close();
    });

    test("畸形入站消息变成 error 事件而不是静默丢弃", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        router.subscribe("error", (event) => seen.push(event));
        transport.emit({ type: "event", seq: 1, reqId: null, payload: { garbage: true } });
        assert.equal(seen.length, 1);
        assert.match(seen[0].value.code, /E_PROTO-0[18]/);
        router.close();
    });

    test("非事件方向的入站消息被忽略", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        let seen = 0;
        router.subscribe("*", () => { seen += 1; });
        transport.emit({
            type: "event",
            seq: 1,
            reqId: null,
            payload: commandMessage(1, Command.nop()),
        });
        assert.equal(seen, 0);
        router.close();
    });

    test("订阅者抛错不会打断其它订阅者", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const seen = [];
        router.subscribe("tick", () => { throw new Error("boom"); });
        router.subscribe("tick", (event) => seen.push(event));
        transport.emit(eventFrame(null, Event.tick(1, 0)));
        assert.equal(seen.length, 1);
        router.close();
    });
});
