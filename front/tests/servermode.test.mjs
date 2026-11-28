// M-07 · 服务端模式的浏览器门槛（R-13 的「指到那就能用」）—— 单测。
//
// 这一层做三件事：**读 `?server=` → 建/验会话 → 水合一次状态**。
// 一条都不该靠真服务端才能验，所以 `fetch` 全部用假实现；真链路的证据在
// 真机冒烟（起服务 + 开页面）那一侧。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const mode = await importFront("services/m07_servermode.js");

// ⚠️ `app.js` 在**模块作用域**读 `document.getElementById("status")` ⇒ ⇒ DOM
// 替身必须在 import 之前装好；`__RVSIM_NO_BOOTSTRAP__` 关掉自启动（这一份
// 用例只验导出的 `describeTransport`，不让引导流程跑起来）。
installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;
const app = await importFront("app.js");

/**
 * 一条假 `fetch`：按「方法 + 路径」查表回答，并把请求记下来供断言。
 *
 * @param {Record<string, {status?: number, body?: any}>} table
 */
function fakeFetch(table) {
    const calls = [];
    const impl = async (url, init = {}) => {
        const path = String(url).replace(/^http:\/\/[^/]+/, "");
        const key = `${init.method ?? "GET"} ${path}`;
        calls.push({ key, url, init });
        const entry = table[key];
        if (!entry) {
            return {
                ok: false,
                status: 404,
                statusText: "Not Found",
                json: async () => ({ code: "E_SESS-01", detail: "session not found", status: 404 }),
            };
        }
        const status = entry.status ?? 200;
        return {
            ok: status < 400,
            status,
            statusText: status < 400 ? "OK" : "Error",
            json: async () => entry.body,
        };
    };
    impl.calls = calls;
    return impl;
}

describe("M-07 server-mode · URL 参数（?server=）", () => {
    test("没给 server ⇒ null（本地 Worker 模式一个字都不改）", () => {
        assert.equal(mode.parseServerParams(""), null);
        assert.equal(mode.parseServerParams("?session=sess-1"), null);
        assert.equal(mode.parseServerParams("?server="), null);
    });

    test("裸 host:port / ws:// / http(s):// 都规范化成 REST 基地址", () => {
        assert.equal(mode.normalizeServerUrl("127.0.0.1:9876"), "http://127.0.0.1:9876");
        assert.equal(mode.normalizeServerUrl("ws://127.0.0.1:9876/"), "http://127.0.0.1:9876");
        assert.equal(mode.normalizeServerUrl("wss://sim.example.com"), "https://sim.example.com");
        assert.equal(mode.normalizeServerUrl("http://127.0.0.1:9876"), "http://127.0.0.1:9876");
    });

    test("session / token / board 一起解析，空串当没给", () => {
        const params = mode.parseServerParams(
            "?server=127.0.0.1:9876&session=sess-000003&token=t0k&board=qemu-virt",
        );
        assert.deepEqual(params, {
            serverUrl: "http://127.0.0.1:9876",
            sessionId: "sess-000003",
            token: "t0k",
            board: "qemu-virt",
        });
        const bare = mode.parseServerParams("?server=127.0.0.1:9876&session=&token=");
        assert.equal(bare.sessionId, null, "空 session 等于没指定");
        assert.equal(bare.token, null, "空 token 等于没带令牌");
    });

    test("空 serverUrl 是显式错误（E_TRX-03），不是悄悄退回 Worker", () => {
        assert.throws(() => mode.normalizeServerUrl("   "), /E_TRX-03/);
    });
});

describe("M-07 server-mode · 建/验会话（ensureSession）", () => {
    test("给了 session 且还在 ⇒ 只验活，不改服务端任何东西", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions/sess-7": {
                body: { session: { session_id: "sess-7", board: "qemu-virt" }, pc: 12 },
            },
        });
        const bound = await mode.ensureSession({
            serverUrl: "127.0.0.1:9876",
            sessionId: "sess-7",
            fetchImpl,
        });
        assert.deepEqual(bound, {
            sessionId: "sess-7",
            created: false,
            restarted: false,
            board: "qemu-virt",
        });
        assert.equal(fetchImpl.calls.length, 1, "验活之外不该再发请求");
        assert.equal(fetchImpl.calls[0].key, "GET /api/sessions/sess-7");
    });

    test("没给 session ⇒ POST /api/sessions（board 带进请求体）", async () => {
        const fetchImpl = fakeFetch({
            "POST /api/sessions": {
                status: 201,
                body: { session_id: "sess-9", board: "qemu-virt" },
            },
        });
        const bound = await mode.ensureSession({
            serverUrl: "http://127.0.0.1:9876",
            board: "qemu-virt",
            token: "t0k",
            fetchImpl,
        });
        assert.equal(bound.sessionId, "sess-9");
        assert.equal(bound.created, true);
        assert.equal(bound.restarted, false);
        const call = fetchImpl.calls[0];
        assert.equal(call.key, "POST /api/sessions");
        assert.deepEqual(JSON.parse(call.init.body), { board: "qemu-virt" });
        assert.equal(call.init.headers["x-auth-token"], "t0k");
        // 账号模式下同源页面靠 `sim_auth` cookie 认人 ⇒ 请求必须带 cookie
        // （`omit` 会让 POST /api/sessions 一律 401 ⇒ 页面只剩 E_TRX-03）。
        assert.equal(call.init.credentials, "same-origin");
    });

    test("同源开页（reuse）⇒ 复用最近一条空闲会话，不 POST 新建", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions": {
                body: {
                    quota: { max: 2, used: 2 },
                    sessions: [
                        { session_id: "sess-old", busy: false, age_ms: 5000 },
                        { session_id: "sess-busy", busy: true, age_ms: 100 },
                        { session_id: "sess-new", busy: false, age_ms: 200 },
                    ],
                },
            },
            "GET /api/sessions/sess-new": {
                body: { session: { session_id: "sess-new", board: "qemu-virt" }, pc: 0 },
            },
        });
        const bound = await mode.ensureSession({
            serverUrl: "http://127.0.0.1:9876",
            reuse: true,
            fetchImpl,
        });
        assert.equal(bound.sessionId, "sess-new", "空闲且最新的一条");
        assert.equal(bound.created, false, "复用不是新建");
        assert.equal(bound.board, "qemu-virt");
        assert.deepEqual(
            fetchImpl.calls.map((c) => c.key),
            ["GET /api/sessions", "GET /api/sessions/sess-new"],
        );
    });

    test("reuse 但列表空 ⇒ 退回 POST 新建（配额没满时照旧可用）", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions": { body: { quota: { max: 2, used: 0 }, sessions: [] } },
            "POST /api/sessions": { status: 201, body: { session_id: "sess-1", board: null } },
        });
        const bound = await mode.ensureSession({
            serverUrl: "http://127.0.0.1:9876",
            reuse: true,
            fetchImpl,
        });
        assert.equal(bound.sessionId, "sess-1");
        assert.equal(bound.created, true);
    });

    test("给了 session 但已经没了（404 E_SESS-01）⇒ 重建一条并标 restarted", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions/sess-gone": {
                status: 404,
                body: { code: "E_SESS-01", detail: "session not found", status: 404 },
            },
            "POST /api/sessions": { status: 201, body: { session_id: "sess-new", board: null } },
        });
        const bound = await mode.ensureSession({
            serverUrl: "127.0.0.1:9876",
            sessionId: "sess-gone",
            fetchImpl,
        });
        assert.equal(bound.sessionId, "sess-new");
        assert.equal(bound.restarted, true, "重建必须说出来，不能假装还是原会话");
        assert.equal(fetchImpl.calls[1].key, "POST /api/sessions");
    });

    test("500 / 连不上不是「会话没了」⇒ 原样抛，不重建", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions/sess-x": {
                status: 500,
                body: { code: "E_SESS-03", detail: "session thread died", status: 500 },
            },
        });
        await assert.rejects(
            mode.ensureSession({ serverUrl: "127.0.0.1:9876", sessionId: "sess-x", fetchImpl }),
            /E_SESS-03/,
        );
        assert.equal(fetchImpl.calls.length, 1, "失败后不该再偷偷建一条");

        const offline = async () => {
            throw new TypeError("Failed to fetch");
        };
        offline.calls = [];
        await assert.rejects(
            mode.ensureSession({ serverUrl: "127.0.0.1:9876", sessionId: "sess-x", fetchImpl: offline }),
            /Failed to fetch/,
        );
    });
});

describe("M-07 server-mode · 状态水合（fetchSessionState / 事件形态）", () => {
    test("GET …/state 的形状原样回来", async () => {
        const fetchImpl = fakeFetch({
            "GET /api/sessions/sess-7/state": {
                body: { pc: 2147483700, registers: Array.from({ length: 32 }, (_, i) => i + 1), executed: 7, t_ps: 1234 },
            },
        });
        const state = await mode.fetchSessionState({
            serverUrl: "127.0.0.1:9876",
            sessionId: "sess-7",
            fetchImpl,
        });
        assert.equal(state.pc, 2147483700);
        assert.equal(state.executed, 7);
    });

    test("32 个裸数 ⇒ SimStateDelta 的 {index,value} 形态（上层不必认第二种）", () => {
        const delta = mode.stateDeltaEvent({
            pc: 0x1234,
            registers: [10, 20, 30],
        });
        assert.deepEqual(delta, {
            pc: 0x1234,
            registers: [
                { index: 0, value: 10 },
                { index: 1, value: 20 },
                { index: 2, value: 30 },
            ],
        });
        assert.equal(mode.stateDeltaEvent(null), null);
    });

    test("t_ps ⇒ Tick 的 elapsed_ps（十进制字符串）；缺失/畸形一律 null，不编 0", () => {
        assert.deepEqual(mode.tickEvent({ pc: 12, t_ps: 1234 }), {
            pc: 12,
            elapsed_ps: "1234",
        });
        assert.equal(mode.tickEvent({ pc: 12 }), null, "没有 t_ps ⇒ null（0 会让时间轴倒退）");
        assert.equal(mode.tickEvent({ pc: 12, t_ps: "12.5" }), null);
        assert.equal(mode.tickEvent(null), null);
    });
});

describe("M-07 server-mode · 状态栏把 resume 说出来（R-13 不许静默丢帧）", () => {
    test("已连接 + 有 resume ⇒ 状态栏带那句话", () => {
        const text = app.describeTransport({
            connected: true,
            mode: "websocket",
            label: "server websocket",
            lastResume: "事件丢帧（从 seq 5 起续读）",
        });
        assert.equal(text, "已连接 · server websocket · 事件丢帧（从 seq 5 起续读）");
    });

    test("没有 resume ⇒ 文案与从前一致（不凭空多一段）", () => {
        assert.equal(
            app.describeTransport({ connected: true, mode: "websocket", label: "server websocket" }),
            "已连接 · server websocket",
        );
    });
});
