// M-04 会话管理器 gate.
//
// The session manager owns two things the gate can check precisely:
// the lifecycle state machine (illegal transitions must be rejected
// with E_CTX-01) and the reconnect semantics (the link may drop while
// the session stays alive — that is the `reconnecting` state).

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

const m04 = await importFront("services/m04_session.js");

/** A transport double with the M-07 surface the session uses. */
function createFakeTransport() {
    /** @type {Set<Function>} */
    const statusHandlers = new Set();
    const transport = {
        connected: true,
        bound: null,
        role: null,
        onStatus(handler) {
            statusHandlers.add(handler);
            return () => statusHandlers.delete(handler);
        },
        bindSession(sessionId, role) {
            transport.bound = sessionId;
            transport.role = role;
            // The real M-07 notifies its status subscribers on every
            // status change — including the session binding. The
            // session manager must survive that synchronous callback.
            transport.notify();
        },
        unbindSession() {
            transport.bound = null;
            transport.notify();
        },
        notify() {
            for (const fn of [...statusHandlers]) {
                fn(transport.getTransportStatus());
            }
        },
        getTransportStatus() {
            return {
                mode: "worker",
                connected: transport.connected,
                connState: transport.connected ? "ready" : "closed",
            };
        },
        setConnected(value) {
            transport.connected = value;
            for (const fn of [...statusHandlers]) {
                fn(transport.getTransportStatus());
            }
        },
    };
    return transport;
}

describe("M-04 会话生命周期", () => {
    test("创建会话 → 绑定传输 → 就绪", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        const created = await session.createSession({ role: "embedded" });
        assert.equal(created.state, "ready");
        assert.match(created.id, /^sess-\d+-[0-9a-f]{6}$/);
        assert.equal(transport.bound, created.id);
        assert.equal(transport.role, "embedded");
        session.close();
    });

    test("会话 id 唯一，且可由调用方指定", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        const a = await session.createSession({ role: "embedded" });
        await session.closeSession();
        const b = await session.createSession({ role: "embedded" });
        assert.notEqual(a.id, b.id);
        await session.closeSession();
        const c = await session.createSession({ role: "embedded", id: "sess-fixed" });
        assert.equal(c.id, "sess-fixed");
        session.close();
    });

    test("状态变化推给订阅者", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        const seen = [];
        const off = session.subscribe((s) => seen.push(s.state));
        await session.createSession({ role: "product" });
        off();
        assert.ok(seen.includes("creating"), "没有观察到 creating 状态");
        assert.ok(seen.includes("ready"), "没有观察到 ready 状态");
        session.close();
    });

    test("退订后不再收到状态变化", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        let hits = 0;
        const off = session.subscribe(() => { hits += 1; });
        await session.createSession({ role: "product" });
        const after = hits;
        off();
        await session.closeSession();
        assert.equal(hits, after, "退订后仍收到状态变化");
        session.close();
    });

    test("重复创建是非法迁移（E_CTX-01）", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        await session.createSession({ role: "embedded" });
        await assert.rejects(session.createSession({ role: "embedded" }), /E_CTX-01/);
        assert.equal(session.getSession().state, "ready", "非法迁移后状态被改坏了");
        session.close();
    });

    test("关闭会话 → 传输解绑 → 终态", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        await session.createSession({ role: "embedded" });
        const closed = await session.closeSession();
        assert.equal(closed.state, "closed");
        assert.equal(transport.bound, null, "关闭后传输仍绑定着会话");
        session.close();
    });

    test("关闭不存在的会话报 E_CTX-11", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        await assert.rejects(session.closeSession(), /E_CTX-11/);
        session.close();
    });
});

describe("M-04 断线重连语义", () => {
    test("链路断开 → reconnecting；链路恢复 → ready 并重新绑定", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        const created = await session.createSession({ role: "embedded" });
        transport.setConnected(false);
        assert.equal(session.getSession().state, "reconnecting");
        // The session id survives the outage.
        assert.equal(session.getSession().id, created.id);
        transport.bound = null;
        transport.setConnected(true);
        assert.equal(session.getSession().state, "ready");
        assert.equal(transport.bound, created.id, "重连后没有重新绑定会话");
        session.close();
    });

    test("链路未就绪时创建会话 → 停在 creating，链路起来后自动就绪", async () => {
        const transport = createFakeTransport();
        transport.connected = false;
        const session = await m04.mountSession({ transport });
        const created = await session.createSession({ role: "embedded" });
        assert.equal(created.state, "creating");
        assert.equal(session.isReady(), false);
        transport.setConnected(true);
        assert.equal(session.getSession().state, "ready");
        assert.equal(session.isReady(), true);
        session.close();
    });

    test("已关闭的会话不再跟随链路状态变化", async () => {
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        await session.createSession({ role: "embedded" });
        await session.closeSession();
        transport.setConnected(false);
        assert.equal(session.getSession().state, "closed");
        session.close();
    });

    test("重连时的重新绑定不会重入状态回调（回归）", async () => {
        // 回归：重连处理里若先 `bindSession` 再迁移，绑定会同步触发状态
        // 回调 → 回调再次绑定 → 无限递归（真机复验时炸过一次）。
        const transport = createFakeTransport();
        const session = await m04.mountSession({ transport });
        await session.createSession({ role: "embedded" });
        transport.setConnected(false);
        assert.equal(session.getSession().state, "reconnecting");
        transport.setConnected(true);
        assert.equal(session.getSession().state, "ready");
        assert.equal(transport.bound, session.getSession().id);
        session.close();
    });
});
