// M-10 寄存器内存服务 gate.
//
// The service is driven through the **real** command router (M-05) and
// a transport double, so the test covers the actual production path:
// request → router → transport → (test) frame → router → service cache
// → view store.

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m05 = await importFront("services/m05_commandrouter.js");
const m10 = await importFront("services/m10_registermemory.js");
const viewStore = await importFront("views/m03_viewstore.js");
const { eventMessage, Event, waveform } = await importFront("lib/message.js");

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
        emit(frame) {
            for (const fn of [...messageHandlers]) fn(frame);
        },
        /** Flip the link state and notify the status subscribers. */
        setConnected(value) {
            transport.connected = value;
            for (const fn of [...statusHandlers]) {
                fn(transport.getTransportStatus());
            }
        },
    };
    return transport;
}

/** Let the promise chain settle. */
const tick = () => new Promise((r) => setTimeout(r, 0));

/** An inbound event frame. */
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

/** A register snapshot as the kernel would push it. */
function registerSnapshot(gpr, pc, tPs = 1000) {
    const streams = gpr.map((value, i) => ({
        id: `cpu.gpr.x${i}`,
        label: `x${i}`,
        edges: [{ t_ps: tPs, value }],
    }));
    streams.push({ id: "cpu.pc", label: "pc", edges: [{ t_ps: tPs, value: pc }] });
    return waveform(streams);
}

describe("M-10 寄存器内存服务", () => {
    test("读寄存器走 M-05，快照落到缓存", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const gpr = new Array(32).fill(0);
        gpr[1] = 42;
        gpr[3] = 49;
        const promise = service.readRegisters();
        const reqId = transport.sent[0].reqId;
        // The subscribe command is a `Subscribe` on the register topic.
        assert.deepEqual(transport.sent[0].message.payload, { Command: { Subscribe: { topic: "sim.registers" } } });
        transport.emit(eventFrame(reqId, Event.waveformDelta(registerSnapshot(gpr, 0x08))));
        await promise;

        const snapshot = service.getSnapshot();
        assert.equal(snapshot.gpr[1], 42);
        assert.equal(snapshot.gpr[3], 49);
        assert.equal(snapshot.pc, 0x08);
        router.close();
        service.close();
    });

    test("寄存器增量只覆盖被改动的寄存器", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const gpr = new Array(32).fill(0);
        gpr[1] = 42;
        transport.emit(eventFrame(null, Event.waveformDelta(registerSnapshot(gpr, 0x04))));
        assert.equal(service.getSnapshot().gpr[1], 42);

        // A delta that only carries x2 keeps x1 untouched.
        transport.emit(eventFrame(null, Event.waveformDelta(waveform([
            { id: "cpu.gpr.x2", label: "x2", edges: [{ t_ps: 2000, value: 7 }] },
        ]))));
        const snapshot = service.getSnapshot();
        assert.equal(snapshot.gpr[2], 7);
        assert.equal(snapshot.gpr[1], 42, "增量把未改动的寄存器清掉了");
        router.close();
        service.close();
    });

    test("读内存走 M-05，值进入内存缓存", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const promise = service.readMemory(0x100, "Word");
        const reqId = transport.sent[0].reqId;
        assert.deepEqual(transport.sent[0].message.payload, { Command: { ReadMemory: { addr: 0x100, size: "Word" } } });
        transport.emit(eventFrame(reqId, Event.memoryRead(0x100, "Word", 49)));
        const value = await promise;
        assert.deepEqual(value, { addr: 0x100, size: "Word", value: 49 });
        assert.equal(service.getSnapshot().memory["0x00000100"], 49);
        router.close();
        service.close();
    });

    test("写内存后再读回（前端不猜内核状态）", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const promise = service.writeMemory(0x200, "Word", 0x1234);
        // First the write command, then the read-back.
        const writeReq = transport.sent[0].reqId;
        assert.deepEqual(transport.sent[0].message.payload, {
            Command: { WriteMemory: { addr: 0x200, size: "Word", value: 0x1234 } },
        });
        transport.emit(eventFrame(writeReq, Event.tick(10, 0)));
        // The read-back is only dispatched once the write command has
        // been answered — let the promise chain settle first.
        await tick();
        const readReq = transport.sent[1].reqId;
        assert.deepEqual(transport.sent[1].message.payload, { Command: { ReadMemory: { addr: 0x200, size: "Word" } } });
        transport.emit(eventFrame(readReq, Event.memoryRead(0x200, "Word", 0x1234)));
        const value = await promise;
        assert.equal(value.value, 0x1234);
        router.close();
        service.close();
    });

    test("未映射地址的读失败被记进 lastError", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const promise = service.readMemory(0x9000_0000, "Word");
        const reqId = transport.sent[0].reqId;
        transport.emit(eventFrame(reqId, Event.error("E_BUS-01", "未映射地址")));
        await assert.rejects(promise, /E_BUS-01/);
        assert.match(service.getSnapshot().lastError, /E_BUS-01/);
        router.close();
        service.close();
    });

    test("快照推送给订阅者", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });
        const seen = [];
        const off = service.subscribe((snapshot) => seen.push(snapshot.gpr[1]));
        transport.emit(eventFrame(null, Event.waveformDelta(registerSnapshot(
            Object.assign(new Array(32).fill(0), { 1: 42 }),
            0x04,
        ))));
        off();
        assert.deepEqual(seen, [42]);
        router.close();
        service.close();
    });

    test("快照写进视图 store 并触发重渲染", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport, viewStore });

        const renders = [];
        const listener = (ev) => renders.push(ev.detail?.viewId);
        dom.window.addEventListener("riscv:render", listener);
        transport.emit(eventFrame(null, Event.waveformDelta(registerSnapshot(
            Object.assign(new Array(32).fill(0), { 5: 99 }),
            0x10,
        ))));
        transport.emit(eventFrame(null, Event.tick(5000, 0x10)));
        service.flushRender();
        dom.window.removeEventListener("riscv:render", listener);

        const state = viewStore.getState("debug");
        assert.equal(state.sim.gpr[5], 99);
        assert.equal(state.sim.pc, 0x10);
        assert.equal(state.timeVirt, 5000);
        assert.ok(renders.includes("debug"), "没有触发 debug 视图的重渲染");
        assert.ok(renders.includes("regmem"), "没有触发 regmem 视图的重渲染");
        router.close();
        service.close();
    });

    test("store 写入是合并语义（不整块替换 sim）", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport, viewStore });
        viewStore.setState("debug", { sim: { disasm: [{ pc: 0 }] } });
        transport.emit(eventFrame(null, Event.waveformDelta(registerSnapshot(new Array(32).fill(0), 0x20))));
        service.flushRender();
        const sim = viewStore.getState("debug").sim;
        assert.equal(sim.disasm.length, 1, "sim 被整块替换，反汇编没了");
        assert.equal(sim.pc, 0x20);
        router.close();
        service.close();
    });

    test("断线重连后重新订阅寄存器（不丢增量）", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        // 1) link up → the service subscribes to the register topic.
        const first = service.ensureSubscribed();
        assert.equal(transport.sent.length, 1);
        assert.deepEqual(
            transport.sent[0].message.payload,
            { Command: { Subscribe: { topic: "sim.registers" } } },
        );
        transport.emit(eventFrame(
            transport.sent[0].reqId,
            Event.waveformDelta(registerSnapshot(new Array(32).fill(0), 0)),
        ));
        await first;

        // 2) the link goes down — the kernel-side subscription dies with
        //    it, so the service must forget that it is subscribed.
        transport.setConnected(false);

        // 3) the link comes back → the service subscribes again.
        transport.setConnected(true);
        await tick();
        assert.equal(transport.sent.length, 2, "重连后没有重新订阅");
        assert.deepEqual(
            transport.sent[1].message.payload,
            { Command: { Subscribe: { topic: "sim.registers" } } },
        );

        // Answer the second subscribe so no command is left pending.
        transport.emit(eventFrame(
            transport.sent[1].reqId,
            Event.waveformDelta(registerSnapshot(new Array(32).fill(0), 0)),
        ));
        await tick();
        router.close();
        service.close();
    });

    test("链路没断时不重复订阅", async () => {
        const transport = createFakeTransport();
        const router = await m05.mountCommandRouter({ transport });
        const service = await m10.mountRegisterMemory({ commandRouter: router, transport });

        const first = service.ensureSubscribed();
        transport.emit(eventFrame(
            transport.sent[0].reqId,
            Event.waveformDelta(registerSnapshot(new Array(32).fill(0), 0)),
        ));
        await first;
        // A repeated status notification must not re-subscribe.
        transport.setConnected(true);
        await tick();
        assert.equal(transport.sent.length, 1, "链路没断却重复订阅了");
        router.close();
        service.close();
    });
});
