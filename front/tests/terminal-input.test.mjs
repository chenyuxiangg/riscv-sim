// v0.24 · Lane K —— 「终端视图能真的把字送进 guest 的 UART」的前端门禁。
//
// 这个套件盯 K-1 的六条不变式（外加一条加分项）：
//
//   1. 终端视图**渲染出**输入框与「发送」按钮；
//   2. 输入文本点发送 ⇒ **真的发出** `WriteConsole` 命令，且字节是输入
//      文本的 **UTF-8 字节**（`"A"` ⇒ `[65]`、`"你"` ⇒ `[228, 189, 160]`）；
//   3. 发送后**输入框被清空**；
//   4. ⚠️ **重渲染不丢草稿**：输入了文本之后触发一次重渲染 ⇒ 输入框的
//      值**还在**（草稿住在 M-03 的视图状态里，不在 DOM 里）；
//   5. ⚠️ **重渲染不丢监听器**：重渲染之后再点发送 ⇒ **仍然**发出命令
//      （监听器绑在**容器**上做事件委托，不绑在会被重建的输入框上）；
//   6. 输入框里的**特殊字符**（`<` / `&` / `"`）被**原样**送出
//      （不是被 HTML 转义后的东西）；
//   7. （加分项）Enter 键也能发送。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。
//
// ⚠️ **为什么第 4 / 5 条是这套件的核心**：`applyRenderEvent` 每次都
// `body.innerHTML = html` ⇒ **输入框元素被重建** ⇒ ① 直接绑在输入框上的
// 监听器**会丢**（点第二次就没反应）；② 只存在 DOM 里的草稿**会被清空**
// （每来一个 console 字节就重渲染一次 ⇒ 每来一个字节就清一次输入框 ⇒
// 根本没法打字）。这两条就是那两条坑的**判别式**。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `console.test.mjs` 同一条理由）：
// `bootstrap()` 是**异步**的，它会调 `console.reset()` ——而 M-03 的视图
// 状态是**模块级单例** ⇒ 那条 reset 可能落在**任意两个用例之间**。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Event: EventPayload, eventMessage } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** `"A"` 的 UTF-8 字节。 */
const BYTES_A = [65];
/** `"你"` 的 UTF-8 字节（U+4F60 ⇒ E4 BD A0）。 */
const BYTES_NI = [228, 189, 160];
/** `"<&\""` 的 UTF-8 字节（`<` = 60、`&` = 38、`"` = 34）。 */
const BYTES_SPECIAL = [60, 38, 34];

// ---------------------------------------------------------------------------
// 传输替身
// ---------------------------------------------------------------------------

/**
 * 一个**只记录**命令的传输替身（进程内同步回帧）。
 *
 * ⚠️ **为什么不用 mock 内核**：`front/dev/mock-kernel.js` **不归本 lane**
 * （文件所有权清单里没有它）⇒ 不许改它；而且它也**不认识** `WriteConsole`
 * （契约 v5 新增的命令）⇒ 用它做判别式只会测到「内核不认识这条命令」。
 *
 * @returns {object} the transport double
 */
function recordingTransport() {
    /** @type {Set<Function>} */
    const handlers = new Set();
    const transport = {
        /** @type {Array<{message: object, reqId: number|null}>} */
        sent: [],
        onMessage(handler) {
            handlers.add(handler);
            return () => handlers.delete(handler);
        },
        onStatus() { return () => {}; },
        getTransportStatus() { return { mode: "worker", connected: true }; },
        sendCommand(message, options = {}) {
            const reqId = options.reqId ?? null;
            transport.sent.push({ message, reqId });
            // 回一个**通用**事件帧，让 `dispatchCommand` 的 Promise 能落地
            // （回复内容与本套件无关：这里盯的是**发出去的那条命令**）。
            const frame = {
                type: "event",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: eventMessage(message.seq, EventPayload.tick(0, 0)),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    return transport;
}

/**
 * 装一套干净的接线并**渲染**终端视图。
 *
 * @returns {Promise<{router: object, transport: object, signals: object[]}>}
 */
async function setup() {
    m03.reset();
    const transport = recordingTransport();
    const router = await m05.mountCommandRouter({ transport });
    m01.assembleWorkspace("embedded");
    m01.route("#/terminal");
    // 视图状态里的终端缓冲清空（`m03.reset()` 已经清了，但路由会写一次）。
    m03.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), bytes: "", draft: "" },
    }));
    publishTerminal();
    /** @type {object[]} */
    const signals = [];
    dom.window.addEventListener("riscv:command", (ev) => {
        signals.push(ev.detail);
        void app.handleCommand(ev.detail, { commandRouter: router });
    });
    return { router, transport, signals };
}

/** 触发一次**生产路径**的重渲染（M-01 的 `riscv:render` 事件）。 */
function publishTerminal() {
    dom.window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "terminal", type: "update", state: m03.getState("terminal") },
    }));
}

/** 终端视图当前的输入框元素（每次重渲染都是**新**元素 ⇒ 每次重新取）。 */
function terminalInput() {
    const el = dom.document.querySelector("[data-terminal-input]");
    assert.ok(el, "终端视图没有渲染出输入框");
    return el;
}

/** 终端视图当前的「发送」按钮。 */
function sendButton() {
    const el = dom.document.querySelector('[data-action="console-send"]');
    assert.ok(el, "终端视图没有渲染出「发送」按钮");
    return el;
}

/**
 * 在输入框里「敲」一段文本（模拟用户输入）。
 *
 * ⚠️ 必须**派发 `input` 事件**：草稿是靠 `input` 事件进视图状态的
 * （真实浏览器里用户每敲一个键都会发它）。
 *
 * @param {string} text
 */
function typeIntoInput(text) {
    const input = terminalInput();
    input.value = text;
    input.dispatchEvent(new Event("input"));
}

// ---------------------------------------------------------------------------

describe("终端视图 · 输入框与发送按钮（K-1）", () => {
    test("终端视图渲染出输入框与「发送」按钮", async () => {
        await setup();
        const input = terminalInput();
        assert.equal(input.getAttribute("type"), "text", "输入框必须是文本框");
        const button = sendButton();
        assert.equal(button.tagName, "BUTTON", "「发送」必须是按钮");
        assert.ok(
            button.textContent.includes("发送"),
            `按钮上应该有「发送」两个字：${button.textContent}`,
        );
    });

    test("点「发送」真的发出 `WriteConsole` 命令，字节是输入文本的 UTF-8 字节", async () => {
        const { transport, signals } = await setup();
        typeIntoInput("A");
        sendButton().dispatchEvent(new Event("click"));

        // -- 视图层只发**信号**（不直接调服务层）--------------------------
        assert.equal(signals.length, 1, "点击没有发出 riscv:command 信号");
        assert.equal(signals[0].action, "console-send");
        assert.deepEqual(signals[0].params.bytes, BYTES_A, "信号里带的字节不对");

        // -- 真的落到了线上 ------------------------------------------------
        assert.equal(transport.sent.length, 1, "命令没有发到 M-05");
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_A } },
            "线上形态必须是 WriteConsole { bytes: [...] }",
        );
    });

    test("多字节字符按 **UTF-8** 编码（\"你\" ⇒ [228, 189, 160]，不是一个码位）", async () => {
        const { transport } = await setup();
        typeIntoInput("你");
        sendButton().dispatchEvent(new Event("click"));
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_NI } },
        );
    });

    test("发送后**输入框被清空**", async () => {
        await setup();
        typeIntoInput("A");
        assert.equal(terminalInput().value, "A", "先得真的敲进去");
        sendButton().dispatchEvent(new Event("click"));
        assert.equal(terminalInput().value, "", "发送后输入框没有清空");
        assert.equal(
            m03.getState("terminal")?.term?.draft,
            "",
            "草稿也必须清空（视图状态是唯一真相）",
        );
    });

    test("⚠️ 重渲染**不丢草稿**（草稿住在 M-03 视图状态里，不在 DOM 里）", async () => {
        await setup();
        typeIntoInput("A");
        assert.equal(m03.getState("terminal")?.term?.draft, "A", "草稿必须进视图状态");

        // 触发一次**生产路径**的重渲染（M-01 的 `riscv:render` 事件 ⇒
        // `applyRenderEvent` ⇒ `body.innerHTML = html` ⇒ 输入框被重建）。
        publishTerminal();

        assert.equal(
            terminalInput().value,
            "A",
            "重渲染把草稿弄丢了 —— 草稿必须住在 M-03 的视图状态里",
        );
    });

    test("⚠️ 重渲染**不丢监听器**（重渲染之后再点发送，仍然发出命令）", async () => {
        const { transport } = await setup();
        typeIntoInput("A");
        publishTerminal();

        // 重新取元素（重渲染后是**新**的元素）⇒ 点它。
        sendButton().dispatchEvent(new Event("click"));

        assert.equal(transport.sent.length, 1, "重渲染之后点发送没有发出命令");
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_A } },
            "重渲染之后发出的字节不对",
        );
    });

    test("特殊字符（`<` / `&` / `\"`）被**原样**送出（不是 HTML 转义后的东西）", async () => {
        const { transport } = await setup();
        typeIntoInput("<&\"");
        sendButton().dispatchEvent(new Event("click"));
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_SPECIAL } },
            "特殊字符被转义了（或被吃掉了）",
        );
    });

    test("（加分项）Enter 键也能发送", async () => {
        const { transport } = await setup();
        typeIntoInput("A");
        const ev = new Event("keydown");
        // ⚠️ 替身不实现 `key` ⇒ 自己挂上（真实浏览器里 `keydown` 带 `key`）。
        Object.defineProperty(ev, "key", { value: "Enter", configurable: true });
        terminalInput().dispatchEvent(ev);
        assert.equal(transport.sent.length, 1, "Enter 没有发出命令");
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_A } },
        );
    });

    test("回显**不**由平台本地产生（发送不发任何 `Console` 事件）", async () => {
        const { transport, signals } = await setup();
        typeIntoInput("A");
        sendButton().dispatchEvent(new Event("click"));
        // ⚠️ **只发一条**命令（`WriteConsole`）⇒ 没有第二条「本地回显」。
        assert.equal(signals.length, 1, "平台不该自己再发一条命令（回显来自 guest）");
        assert.equal(
            transport.sent.filter((s) => s.message.payload.Command.Nop !== undefined).length,
            0,
            "不该有 Nop 之类的多余命令",
        );
    });
});
