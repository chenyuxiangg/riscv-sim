// v0.24 · Lane K —— 「终端视图能真的把字送进 guest 的 UART」的前端门禁。
//
// ⚠️ **本批（xterm 直输模式）口径变更**：旧版是「独立输入框 + 发送按钮」的
// 行缓冲模式 ⇒ ⇒ 整段草稿在点发送 / 按 Enter 时一次性发出。新版是**真实
// 终端直输**：键盘由 xterm 表面接管，逐键（含控制字节 / CSI 序列）经
// `terminalInputBytes` 映射成字节 ⇒ ⇒ `dispatchTerminalBytes` 发出。
//
// 这套件盯三组不变式：
//
//   **A. 直输形态渲染**
//     1. 视图里**不再有**独立输入框与「发送」按钮；xterm 持久宿主 + 降级
//        `<pre>` 都在；
//     2. 四档行结束符按钮都渲染出来，当前档带 `data-eol-selected`；
//     3. 行结束符的选择**重渲染不丢**（住在 M-03 视图状态里）。
//
//   **B. 纯函数 `terminalInputBytes`（字节映射，零 DOM 可测）**
//     4. 普通文本按 **UTF-8** 编码（`"A"` ⇒ `[65]`、`"你"` ⇒ `[228,189,160]`）；
//     5. 特殊字符（`<` / `&` / `"`）原样送字节，不做 HTML 转义；
//     6. 回车（`\r` / `\n`）按当前档 EOL 映射：CR / LF / CRLF / 不加；
//     7. 控制字节原样透传：Ctrl-C（0x03）、退格（0x7f）、方向键 CSI 序列；
//     8. 未知 EOL 档回落默认 CR。
//
//   **C. 信号与命令链路（视图层 → M-05 → 线上）**
//     9. `dispatchTerminalBytes` 发一条 `console-send` 信号，落到线上是
//        `WriteConsole { bytes: [...] }`；
//    10. RX 计数按实际发出的字节数累计（含 EOL 字节）；
//    11. 空字节数组**不发**命令、不涨 RX；
//    12. **回显来自 guest**：平台本地不产生回显命令；
//    13. 重渲染之后信号路径仍然通（再发一次照样落到线上）。
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推。

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
/** 默认档追加的那**一个**字节（CR = 0x0D）。 */
const BYTES_CR = [13];
/** LF = 0x0A。 */
const BYTES_LF = [10];
/** CRLF = 0x0D 0x0A。 */
const BYTES_CRLF = [13, 10];

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
    // 视图状态里的终端缓冲清空。
    m03.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), bytes: "" },
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

/**
 * 终端视图当前的**行结束符按钮**（每次重渲染都是**新**元素 ⇒ 每次重新取）。
 *
 * @param {string} id
 * @returns {object}
 */
function eolButton(id) {
    const el = dom.document.querySelector(`[data-eol="${id}"]`);
    assert.ok(el, `终端视图没有渲染出行结束符按钮 ${id}`);
    return el;
}

/**
 * 点一个**行结束符按钮**（切档）。
 *
 * @param {string} id
 */
function clickEol(id) {
    eolButton(id).dispatchEvent(new Event("click"));
}

// ---------------------------------------------------------------------------
// A. 直输形态渲染
// ---------------------------------------------------------------------------

describe("终端视图 · 直输形态（xterm 直输模式）", () => {
    test("不再渲染独立输入框与「发送」按钮；xterm 宿主与降级屏幕都在", async () => {
        await setup();
        assert.equal(
            dom.document.querySelector("[data-terminal-input]"),
            null,
            "旧版独立输入框必须下线（直输由 xterm 表面接管）",
        );
        assert.equal(
            dom.document.querySelector('[data-action="console-send"]'),
            null,
            "旧版「发送」按钮必须下线（直输逐键发出）",
        );
        assert.ok(
            dom.document.querySelector("[data-xterm-host]"),
            "xterm 持久宿主必须渲染出来（库就绪后接管）",
        );
        assert.ok(
            dom.document.querySelector("[data-terminal-out]"),
            "降级 `<pre>` 屏幕必须保留（库不可达 / 原始字节档的退路）",
        );
    });

    test("四档行结束符按钮都渲染出来，当前档带 `data-eol-selected`", async () => {
        await setup();
        for (const id of ["cr", "lf", "crlf", "none"]) {
            const el = eolButton(id);
            assert.equal(el.tagName, "BUTTON", `行结束符 ${id} 必须是按钮`);
        }
        assert.equal(
            eolButton("cr").getAttribute("data-eol-selected"),
            "true",
            "默认档必须是 CR",
        );
        assert.equal(
            eolButton("lf").getAttribute("data-eol-selected"),
            null,
            "非当前档不许带 `data-eol-selected`",
        );
    });

    test("⚠️ 行结束符的选择**重渲染不丢**（它住在 M-03 的视图状态里）", async () => {
        await setup();
        clickEol("lf");
        publishTerminal();
        assert.equal(
            eolButton("lf").getAttribute("data-eol-selected"),
            "true",
            "重渲染之后当前档必须还是 LF —— 选择必须住在视图状态里，不在 DOM 里",
        );
    });
});

// ---------------------------------------------------------------------------
// B. 纯函数 `terminalInputBytes`
// ---------------------------------------------------------------------------

describe("终端视图 · 按键字节映射（terminalInputBytes 纯函数）", () => {
    test("普通文本按 **UTF-8** 编码（ASCII / 多字节字符）", () => {
        assert.deepEqual(m02.terminalInputBytes("A", "cr"), BYTES_A);
        assert.deepEqual(m02.terminalInputBytes("你", "cr"), BYTES_NI);
    });

    test("特殊字符（`<` / `&` / `\"`）被**原样**送字节（不做 HTML 转义）", () => {
        assert.deepEqual(
            m02.terminalInputBytes("<&\"", "cr"),
            BYTES_SPECIAL,
            "特殊字符被转义了（或被吃掉了）",
        );
    });

    test("回车（`\\r` / `\\n`）按当前档 EOL 映射：CR / LF / CRLF / 不加", () => {
        assert.deepEqual(m02.terminalInputBytes("\r", "cr"), BYTES_CR);
        assert.deepEqual(m02.terminalInputBytes("\n", "cr"), BYTES_CR);
        assert.deepEqual(m02.terminalInputBytes("\r", "lf"), BYTES_LF);
        assert.deepEqual(m02.terminalInputBytes("\n", "lf"), BYTES_LF);
        assert.deepEqual(m02.terminalInputBytes("\r", "crlf"), BYTES_CRLF);
        assert.deepEqual(m02.terminalInputBytes("\r", "none"), []);
    });

    test("控制字节原样透传：Ctrl-C / 退格 / 方向键 CSI 序列", () => {
        // Ctrl-C = 0x03；退格 = 0x7f；方向键 ↑ = ESC [ A。
        assert.deepEqual(m02.terminalInputBytes("\x03", "cr"), [3]);
        assert.deepEqual(m02.terminalInputBytes("\x7f", "cr"), [127]);
        assert.deepEqual(m02.terminalInputBytes("\x1b[A", "cr"), [27, 91, 65]);
    });

    test("未知 EOL 档回落默认 CR", () => {
        assert.deepEqual(m02.terminalInputBytes("\r", "bogus"), BYTES_CR);
    });
});

// ---------------------------------------------------------------------------
// C. 信号与命令链路
// ---------------------------------------------------------------------------

describe("终端视图 · 信号与命令链路（直输 → WriteConsole）", () => {
    test("发出的字节落到线上是 `WriteConsole { bytes: [...] }`", async () => {
        const { transport, signals } = await setup();
        m02.dispatchTerminalBytes(BYTES_A);

        // -- 视图层只发**信号**（不直接调服务层）--------------------------
        assert.equal(signals.length, 1, "没有发出 riscv:command 信号");
        assert.equal(signals[0].action, "console-send");
        assert.deepEqual(signals[0].params.bytes, BYTES_A);

        // -- 真的落到了线上 ------------------------------------------------
        assert.equal(transport.sent.length, 1, "命令没有发到 M-05");
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_A } },
            "线上形态必须是 WriteConsole { bytes: [...] }",
        );
    });

    test("RX 计数按实际发出的字节数累计（含回车 EOL 字节）", async () => {
        await setup();
        // 敲 "hi" 两个键 ⇒ 回车按默认档补 CR 一个。
        m02.dispatchTerminalBytes(m02.terminalInputBytes("hi", "cr"));
        m02.dispatchTerminalBytes(m02.terminalInputBytes("\r", "cr"));
        assert.equal(
            m03.getState("terminal")?.term?.rxBytes,
            3,
            "RX 必须按实际发出字节累计（2 个字符 + 1 个 CR）",
        );
        // 「不加」那一档 ⇒ 回车不补字节。
        m02.dispatchTerminalBytes(m02.terminalInputBytes("\r", "none"));
        assert.equal(
            m03.getState("terminal")?.term?.rxBytes,
            3,
            "EOL = none 时回车不发字节 ⇒ RX 不该再涨",
        );
    });

    test("空字节数组**不发**命令、不涨 RX", async () => {
        const { transport, signals } = await setup();
        const id = m02.dispatchTerminalBytes([]);
        assert.equal(id, undefined, "空字节不该返回请求 id");
        assert.equal(signals.length, 0, "空字节不该发信号");
        assert.equal(transport.sent.length, 0, "空字节不该落到线上");
        assert.equal(
            Number(m03.getState("terminal")?.term?.rxBytes ?? 0),
            0,
            "空字节不该涨 RX",
        );
    });

    test("回显**不**由平台本地产生（只发 WriteConsole，没有多余命令）", async () => {
        const { transport, signals } = await setup();
        m02.dispatchTerminalBytes(BYTES_A);
        // ⚠️ **只发一条**命令（`WriteConsole`）⇒ 没有第二条「本地回显」。
        assert.equal(signals.length, 1, "平台不该自己再发一条命令（回显来自 guest）");
        assert.equal(
            transport.sent.filter((s) => s.message.payload.Command.Nop !== undefined).length,
            0,
            "不该有 Nop 之类的多余命令",
        );
    });

    test("⚠️ 重渲染之后信号路径仍然通（再发一次照样落到线上）", async () => {
        const { transport } = await setup();
        publishTerminal();
        m02.dispatchTerminalBytes(BYTES_A);
        assert.equal(transport.sent.length, 1, "重渲染之后没有发出命令");
        assert.deepEqual(
            transport.sent[0].message.payload.Command,
            { WriteConsole: { bytes: BYTES_A } },
            "重渲染之后发出的字节不对",
        );
    });
});
