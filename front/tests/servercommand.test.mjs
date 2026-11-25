// M-07 · 服务端模式的**写通道**（v1.11）gate.
//
// 判据按「前端真的会怎么用」写：
//   - 地址是 `POST /api/sessions/{id}/command`（不是 run/step 那几个 REST 端点）；
//   - 请求体是**整条**契约消息 + 帧的 `reqId`（服务端不翻译命令）；
//   - 回体帧直接喂 `emitFrame`（M-05 按 `reqId` 配对）；
//   - 异步失败必须变成一条 **Error 事件帧**（不然命令白等 500 ms 再报超时）。

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
const serverCommand = await importFront("services/m07_servercommand.js");
const { commandMessage, eventMessage, Command, Event } = await importFront("lib/message.js");
const { encodeFrame } = await importFront("lib/frame.js");

/** A fetch double that records the request and answers with a canned response. */
function createFakeFetch(handler) {
    const calls = [];
    const fakeFetch = async (url, init) => {
        calls.push({ url, init });
        return handler(url, init);
    };
    fakeFetch.calls = calls;
    return fakeFetch;
}

/** A fake WebSocket with the browser's callback shape. */
class FakeWebSocket {
    constructor(url) {
        FakeWebSocket.last = this;
        this.url = url;
        this.sent = [];
    }

    send(data) {
        this.sent.push(data);
    }

    close() {
        this.onclose?.({});
    }
}

describe("M-07 写通道 · 地址与请求体", () => {
    test("地址是契约命令端点（三种 serverUrl 写法都能拼）", () => {
        assert.equal(
            serverCommand.commandUrl("http://127.0.0.1:9876", "sess-000001"),
            "http://127.0.0.1:9876/api/sessions/sess-000001/command",
        );
        assert.equal(
            serverCommand.commandUrl("ws://127.0.0.1:9876", "sess-1"),
            "http://127.0.0.1:9876/api/sessions/sess-1/command",
        );
        assert.equal(
            serverCommand.commandUrl("127.0.0.1:9876/", "s 1"),
            "http://127.0.0.1:9876/api/sessions/s%201/command",
        );
    });

    test("没有 sessionId ⇒ 显式 E_TRX-03（不拼出半条地址）", () => {
        assert.throws(
            () => serverCommand.commandUrl("http://127.0.0.1:9876", null),
            /E_TRX-03/,
        );
    });

    test("请求体是整条契约消息 + reqId（一个字段都不改）", () => {
        const message = commandMessage(3, Command.nop());
        const body = serverCommand.commandBody(message, 7);
        assert.equal(body.message, message, "契约消息必须原样传引用");
        assert.equal(body.reqId, 7);
        assert.equal(serverCommand.commandBody(message).reqId, null);
    });
});

describe("M-07 写通道 · postCommand", () => {
    test("POST 到契约命令端点：JSON + 令牌头 + 原样消息", async () => {
        const message = commandMessage(3, Command.nop());
        const frame = encodeFrame({
            type: "event",
            seq: 101,
            sessionId: "sess-1",
            role: null,
            reqId: 7,
            payload: eventMessage(101, Event.error("E_DBG-03", "示例")),
        });
        const fakeFetch = createFakeFetch(async () =>
            new Response(JSON.stringify({ frames: [frame] }), { status: 200 }));
        const frames = await serverCommand.postCommand({
            serverUrl: "http://127.0.0.1:9876",
            sessionId: "sess-1",
            token: "tk",
            message,
            reqId: 7,
            fetchImpl: fakeFetch,
        });
        assert.equal(fakeFetch.calls.length, 1);
        const { url, init } = fakeFetch.calls[0];
        assert.equal(url, "http://127.0.0.1:9876/api/sessions/sess-1/command");
        assert.equal(init.method, "POST");
        assert.equal(init.headers["content-type"], "application/json");
        assert.equal(init.headers["x-auth-token"], "tk");
        const body = JSON.parse(init.body);
        assert.deepEqual(body, { message, reqId: 7 });
        assert.equal(frames.length, 1);
        assert.equal(frames[0].reqId, 7);
    });

    test("服务端的错误体（code/detail/status）按**码**抛回去", async () => {
        const fakeFetch = createFakeFetch(async () =>
            new Response(JSON.stringify({ code: "E_SESS-01", detail: "no such session", status: 404 }), {
                status: 404,
                headers: { "content-type": "application/json" },
            }));
        await assert.rejects(
            serverCommand.postCommand({
                serverUrl: "http://127.0.0.1:9876",
                sessionId: "sess-1",
                message: commandMessage(1, Command.nop()),
                reqId: 3,
                fetchImpl: fakeFetch,
            }),
            (err) => {
                assert.equal(err.code, "E_SESS-01");
                assert.match(err.message, /HTTP 404/);
                return true;
            },
        );
    });

    test("回体没有 frames ⇒ E_PROTO-08（应答丢了不许当成功）", async () => {
        const fakeFetch = createFakeFetch(async () =>
            new Response(JSON.stringify({}), { status: 200 }));
        await assert.rejects(
            serverCommand.postCommand({
                serverUrl: "http://127.0.0.1:9876",
                sessionId: "sess-1",
                message: commandMessage(1, Command.nop()),
                fetchImpl: fakeFetch,
            }),
            /E_PROTO-08/,
        );
    });

    test("网络层失败 ⇒ E_TRX-03（带地址，便于定位服务没起）", async () => {
        const fakeFetch = createFakeFetch(async () => {
            throw new Error("connection refused");
        });
        await assert.rejects(
            serverCommand.postCommand({
                serverUrl: "http://127.0.0.1:9876",
                sessionId: "sess-1",
                message: commandMessage(1, Command.nop()),
                fetchImpl: fakeFetch,
            }),
            (err) => {
                assert.match(String(err.code), /E_TRX-03/);
                assert.match(err.message, /connection refused/);
                return true;
            },
        );
    });
});

describe("M-07 写通道 · 适配器接线", () => {
    test("serverUrl + sessionId ⇒ send 真的 POST，回体帧进同一条 emitFrame", async () => {
        const original = globalThis.WebSocket;
        globalThis.WebSocket = FakeWebSocket;
        const answer = encodeFrame({
            type: "event",
            seq: 1 << 20,
            sessionId: "sess-1",
            role: null,
            reqId: 5,
            payload: eventMessage(1 << 20, Event.error("E_CTX-05", "示例应答")),
        });
        const fakeFetch = createFakeFetch(async () =>
            new Response(JSON.stringify({ frames: [answer] }), { status: 200 }));
        try {
            await m07.mountTransport({
                mode: "websocket",
                serverUrl: "http://127.0.0.1:9876",
                sessionId: "sess-1",
                role: "host",
                token: "tk",
                fetchImpl: fakeFetch,
            });
            FakeWebSocket.last.onopen?.({});
            const seen = [];
            const off = m07.onMessage((frame) => seen.push(frame));
            m07.sendCommand(commandMessage(1, Command.nop()), { reqId: 5 });
            // POST 是异步的：给 microtask 一轮时间。
            await new Promise((r) => setTimeout(r, 0));
            off();
            assert.equal(fakeFetch.calls.length, 1, "命令必须真的发出去");
            assert.equal(seen.length, 1, "回体帧必须进同一条 emitFrame");
            assert.equal(seen[0].reqId, 5);
            assert.equal(seen[0].payload.seq, 1 << 20);
        } finally {
            globalThis.WebSocket = original;
            m07.close();
        }
    });

    test("POST 失败 ⇒ 回一条带同一个 reqId 的 Error 帧（M-05 才会立刻拒）", async () => {
        const original = globalThis.WebSocket;
        globalThis.WebSocket = FakeWebSocket;
        const fakeFetch = createFakeFetch(async () =>
            new Response(JSON.stringify({ code: "E_SESS-01", detail: "session gone", status: 404 }), {
                status: 404,
                headers: { "content-type": "application/json" },
            }));
        try {
            await m07.mountTransport({
                mode: "websocket",
                serverUrl: "http://127.0.0.1:9876",
                sessionId: "sess-1",
                fetchImpl: fakeFetch,
            });
            FakeWebSocket.last.onopen?.({});
            const seen = [];
            const off = m07.onMessage((frame) => seen.push(frame));
            m07.sendCommand(commandMessage(1, Command.nop()), { reqId: 9 });
            await new Promise((r) => setTimeout(r, 0));
            off();
            assert.equal(seen.length, 1);
            assert.equal(seen[0].reqId, 9, "配对号必须保留（否则命令白等超时）");
            assert.equal(seen[0].payload.payload.Event.Error.code, "E_SESS-01");
        } finally {
            globalThis.WebSocket = original;
            m07.close();
        }
    });
});
