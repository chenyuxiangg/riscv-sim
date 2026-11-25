// M-07 · 同源自动进服务端模式（`--front-dir` 那一半的前端判据）。
//
// 场景：容器里页面与 API 是**同一个端口**，用户拿到的地址就是
// `http://host:8080/`，没有也不会去手写 `?server=`。前端必须自己认出
// 「我就是服务端发出来的」，否则页面会去起本地 Worker —— 而镜像里没有
// WASM 产物 ⇒ 状态栏写「脚本没启动」，看起来像坏掉。
//
// 判据按「真会怎么坏」写：
//   - 必须**真的**读到 `kernel` + `schema_version`（随便一个 200 的网页不算）；
//   - 探测失败 / 超时 / 形状不对 ⇒ `null`，**不抛**（本地模式不许被探测拖死）；
//   - `file://` 与非 http(s) 的同源 ⇒ 直接 `null`（没有同源可言）；
//   - 令牌走 `?token=` 仍然认（服务端开了 `--token` 时）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const serverMode = await importFront("services/m07_servermode.js");
const transport = await importFront("services/m07_transport.js");

function fakeFetch(answer) {
    const calls = [];
    const fn = async (url, init) => {
        calls.push({ url, init });
        return answer(url, init);
    };
    fn.calls = calls;
    return fn;
}

const okVersion = () =>
    new Response(
        JSON.stringify({
            kernel: "0.1.0",
            schema_version: 19,
            contract_version: "0.1.0",
        }),
        { status: 200, headers: { "content-type": "application/json" } },
    );

describe("M-07 同源探测 · 判据", () => {
    test("服务端就在同源时返回 location.origin 与版本三件套", async () => {
        const fetchImpl = fakeFetch(() => okVersion());
        const detected = await serverMode.detectSameOriginServer({
            fetchImpl,
            origin: "http://127.0.0.1:8080",
        });
        assert.equal(detected.serverUrl, "http://127.0.0.1:8080");
        assert.equal(detected.sessionId, null, "会话要由 ensureSession 现建");
        assert.equal(detected.detected, true);
        assert.equal(fetchImpl.calls[0].url, "http://127.0.0.1:8080/api/version");
        assert.equal(fetchImpl.calls[0].init.method, "GET");
    });

    test("末尾斜杠不会拼出 `//api/version`", async () => {
        const fetchImpl = fakeFetch(() => okVersion());
        await serverMode.detectSameOriginServer({
            fetchImpl,
            origin: "http://sim.example.com/",
        });
        assert.equal(fetchImpl.calls[0].url, "http://sim.example.com/api/version");
    });

    test("普通网页的 200 不算服务端（没有版本三件套 ⇒ null）", async () => {
        const fetchImpl = fakeFetch(
            () => new Response("<!doctype html><title>hi</title>", { status: 200 }),
        );
        const detected = await serverMode.detectSameOriginServer({
            fetchImpl,
            origin: "http://127.0.0.1:8090",
        });
        assert.equal(detected, null);
    });

    test("404 / 连不上 / 畸形 JSON 都只回 null，不抛", async () => {
        for (const answer of [
            () => new Response("not found", { status: 404 }),
            () => {
                throw new Error("ECONNREFUSED");
            },
            () => new Response("{ not json", { status: 200 }),
        ]) {
            const detected = await serverMode.detectSameOriginServer({
                fetchImpl: fakeFetch(answer),
                origin: "http://127.0.0.1:9876",
            });
            assert.equal(detected, null, "本地模式不许被一次探测失败拖死");
        }
    });

    test("file:// 与非 http(s) 同源直接跳过（一个请求都不发）", async () => {
        for (const origin of ["file:///Users/x/front/index.html", "null", "", null]) {
            const fetchImpl = fakeFetch(() => okVersion());
            const detected = await serverMode.detectSameOriginServer({ fetchImpl, origin });
            assert.equal(detected, null);
            assert.equal(fetchImpl.calls.length, 0);
        }
    });

    test("`?token=` 会跟到探测结果上（服务端开了 --token 时仍然能建会话）", async () => {
        const fetchImpl = fakeFetch(() => okVersion());
        const detected = await serverMode.detectSameOriginServer({
            fetchImpl,
            origin: "http://127.0.0.1:8080",
            token: "s3cret",
        });
        assert.equal(detected.token, "s3cret");
    });
});

describe("M-07 事件流 · 令牌走查询串（浏览器不能带头）", () => {
    const ws = "ws://127.0.0.1:8080/api/sessions/sess-000001/events/ws";

    test("有令牌就拼上 ?token=（服务端只对升级请求放行这一种）", () => {
        assert.equal(transport.withStreamToken(ws, "s3cret"), `${ws}?token=s3cret`);
    });

    test("地址里已经有查询串 ⇒ 用 & 接上，不重复 `?`", () => {
        assert.equal(
            transport.withStreamToken(`${ws}?since=7`, "s3cret"),
            `${ws}?since=7&token=s3cret`,
        );
    });

    test("没有令牌 ⇒ 一个字都不加（回环开发不必带）", () => {
        for (const token of [null, undefined, "", "   "]) {
            assert.equal(transport.withStreamToken(ws, token), ws);
        }
    });

    test("令牌里的特殊字符会被编码（服务端解回来再比）", () => {
        assert.equal(transport.withStreamToken(ws, "a b&c"), `${ws}?token=a%20b%26c`);
    });
});
