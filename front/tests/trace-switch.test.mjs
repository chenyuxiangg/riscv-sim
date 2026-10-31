// trace 记录开关的**点击路径**门禁。
//
// ⚠️ **为什么另起一个文件**（而不是塞进 `disasm.test.mjs`）：此前这条路上
//   **一条判据都没有** —— `disasm.test.mjs` 只在**服务层**测（直接
//   `router.dispatchCommand(Command.setTraceEnabled(true))`）⇒ ⇒ **按钮本身
//   从来没被点过** ⇒ ⇒ 「按钮点了没反应」这类缺陷**结构上**不可能被门禁发现
//   ⇒ ⇒ 本文件盯的就是**那一段**：视图 → 信号 → app → 命令 → 内核 → 视图。
//
// ⚠️ **三组判据**：
//
//   1. **纯函数层**（`front/lib/trace-switch.js`）：状态推导与「下一个值」
//      ⇒ ⇒ Node 侧零 DOM 就能测 ⇒ ⇒ 分得出「是哪一环坏的」。
//   2. **点击路径**（生产信号路径）：点按钮 ⇒ ⇒ 命令真的发出去 ⇒ ⇒ 内核的
//      报告回到**视图状态** ⇒ ⇒ 界面**看得见**状态。
//   3. **反向对照**：状态是「关」时点一下 ⇒ ⇒ 发的是 `true`（不是 `false`）
//      ⇒ ⇒ 「点一下就是反着来」这条**真的被验**，而不是恒真。
//
// ⚠️ **状态行必须三态都在**（未查询 / 已开启 / 已关闭）⇒ ⇒ 此前只有「开着」
//   才有那行提示 ⇒ ⇒ 「关着」与「从没点过」在界面上**一模一样** ⇒ ⇒ 用户
//   判断不了自己开没开 ⇒ ⇒ 「点了没反应」。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（同 `disasm.test.mjs`）⇒ ⇒ 套件**不依赖时序**。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Command, eventMessage } = await importFront("lib/message.js");
const {
    TRACE_SWITCH_OFF,
    TRACE_SWITCH_ON,
    TRACE_SWITCH_UNKNOWN,
    nextTraceEnabled,
    traceSwitchState,
} = await importFront("lib/trace-switch.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");

await m02.mountViewComponents();

/**
 * An **in-process** transport: the mock kernel answers synchronously ⇒ ⇒
 * 整条链路（视图 → 信号 → app → M-05 → mock → M-05 → 视图）在一个 tick 内。
 *
 * @param {object} kernel
 */
function inProcessTransport(kernel) {
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
            const frames = kernel.handle({
                type: "command",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: message,
            });
            for (const frame of frames) {
                for (const fn of [...handlers]) fn(frame);
            }
        },
        emit(payload) {
            const frame = {
                type: "event",
                seq: 1,
                sessionId: "sess-test",
                role: "embedded",
                reqId: null,
                payload: eventMessage(1, payload),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    return transport;
}

/**
 * Build a clean wiring: mock kernel → transport → M-05 → 调试面接线，
 * plus the mounted + rendered debug view.
 *
 * @returns {{kernel: object, transport: object, router: object}}
 */
async function setup() {
    m03.reset();
    const kernel = createMockKernel();
    const transport = inProcessTransport(kernel);
    const router = await m05.mountCommandRouter({ transport });
    app.wireDebug(router);
    m01.assembleWorkspace("embedded");
    m01.route("#/debug");
    return { kernel, transport, router };
}

/**
 * Click the trace switch and let the **production signal path** run:
 * the view emits `riscv:command`, the app turns it into an M-05 command.
 *
 * ⚠️ **返回 promise** ⇒ ⇒ 用例能 `await` 它 ⇒ ⇒ 断言不会落在「状态还没回来」
 *   那一格上（`handleCommand` 是 async ⇒ ⇒ `await` 回来比订阅回调**更晚** ⇒ ⇒
 *   不 await 就是**竞态**）。
 *
 * @param {object} router
 * @param {object} button
 * @returns {Promise<void>}
 */
async function clickSwitch(router, button) {
    /** @type {Promise<void>[]} */
    const pending = [];
    const listener = (ev) => {
        pending.push(app.handleCommand(ev.detail, { commandRouter: router }));
    };
    dom.window.addEventListener("riscv:command", listener);
    button.dispatchEvent(new Event("click"));
    dom.window.removeEventListener("riscv:command", listener);
    await Promise.all(pending);
}

/** The trace switch button in the debug toolbar. */
function switchButton() {
    const btn = dom.document.querySelector('[data-action="trace-toggle"]');
    assert.ok(btn, "调试工具栏里没有 trace 开关按钮");
    return btn;
}

/** The persistent trace status line. */
function statusLine() {
    const el = dom.document.querySelector("[data-trace-state]");
    assert.ok(el, "没有 trace 状态行（应当常驻：开与关都在）");
    return el;
}

// ---------------------------------------------------------------------------

describe("trace 开关 · 纯函数层", () => {
    test("状态推导：没报告过 ⇒ 未查询（不猜）", () => {
        assert.equal(traceSwitchState(null).state, TRACE_SWITCH_UNKNOWN);
        assert.equal(traceSwitchState(undefined).state, TRACE_SWITCH_UNKNOWN);
        // ⚠️ **`enabled` 不是布尔 ⇒ 不猜** ⇒ ⇒ 「半份报告」不许当成「开着」。
        assert.equal(traceSwitchState({ len: 3 }).state, TRACE_SWITCH_UNKNOWN);
        assert.equal(traceSwitchState({ enabled: "yes" }).state, TRACE_SWITCH_UNKNOWN);
    });

    test("状态推导：开着 / 关着 + 条数 + 丢弃数", () => {
        const on = traceSwitchState({ enabled: true, len: 12345, dropped: 7 });
        assert.equal(on.state, TRACE_SWITCH_ON);
        assert.equal(on.on, true);
        assert.equal(on.len, 12345);
        assert.equal(on.dropped, 7);

        const off = traceSwitchState({ enabled: false, len: 10, dropped: 0 });
        assert.equal(off.state, TRACE_SWITCH_OFF);
        assert.equal(off.on, false);
        assert.equal(off.len, 10);
    });

    test("下一个值：未查询 ⇒ 开启（第一次点顺带查询）", () => {
        assert.equal(nextTraceEnabled(null), true);
        assert.equal(nextTraceEnabled({ enabled: true }), false);
        assert.equal(nextTraceEnabled({ enabled: false }), true);
    });
});

// ---------------------------------------------------------------------------

describe("trace 开关 · 点击路径（视图 → 信号 → app → 命令 → 内核 → 视图）", () => {
    test("页面里**只有**一个 trace 开关入口（不是两个按钮）", async () => {
        const { router } = await setup();
        assert.equal(
            dom.document.querySelectorAll('[data-action="trace-toggle"]').length,
            1,
            "trace 开关入口不是一个（应当是一个带状态的开关）",
        );
        assert.equal(
            dom.document.querySelectorAll('[data-action="trace-enable"], [data-action="trace-disable"]').length,
            0,
            "旧的「开启 / 关闭」两个按钮还在（应当并成一个开关）",
        );
        router.close();
    });

    test("点一下 ⇒ 命令真的发出去（SetTraceEnabled{enabled:true}）", async () => {
        const { router, transport } = await setup();
        await clickSwitch(router, switchButton());
        assert.equal(transport.sent.length, 1, "点开关没有发出命令");
        assert.deepEqual(
            transport.sent[0].message.payload?.Command,
            { SetTraceEnabled: { enabled: true } },
            "第一次点必须是「开启」",
        );
        router.close();
    });

    test("内核的报告回到界面：状态行说「已开启」+ 条数 + 按钮标签跟着变", async () => {
        const { router } = await setup();
        assert.equal(statusLine().getAttribute("data-trace-switch"), "unknown");

        await clickSwitch(router, switchButton());

        const line = statusLine();
        assert.equal(line.getAttribute("data-trace-switch"), "on", "状态行没有变成 on");
        const main = line.querySelector(".trace-state-main");
        assert.ok(main, "状态行的主句不在");
        assert.ok(main.textContent.includes("已开启"), "状态行没有说「已开启」");
        assert.ok(main.textContent.includes("降速"), "状态行没有说明会降速");
        // ⚠️ **条数来自内核的报告**（`TraceState.len`）⇒ ⇒ 不是前端编的。
        assert.ok(
            /环里\s*0\s*条/.test(line.textContent),
            `状态行没有显示内核报告的条数：${line.textContent}`,
        );

        const btn = switchButton();
        assert.ok(
            btn.textContent.includes("已开启"),
            `按钮标签没有显示状态：${btn.textContent}`,
        );
        assert.ok(
            btn.textContent.includes("点击关闭"),
            `按钮标签没有说点了会发生什么：${btn.textContent}`,
        );
        router.close();
    });

    test("再点一下 ⇒ 关掉（反向对照：发的是 false，且界面说「已关闭」）", async () => {
        const { router, transport } = await setup();
        await clickSwitch(router, switchButton());
        await clickSwitch(router, switchButton());

        assert.equal(transport.sent.length, 2, "第二次点没有发出命令");
        assert.deepEqual(
            transport.sent[1].message.payload?.Command,
            { SetTraceEnabled: { enabled: false } },
            "第二次点必须是「关闭」",
        );
        const line = statusLine();
        assert.equal(line.getAttribute("data-trace-switch"), "off", "状态行没有变成 off");
        assert.ok(line.textContent.includes("已关闭"), "状态行没有说「已关闭」");
        assert.ok(
            switchButton().textContent.includes("点击开启"),
            "关掉后按钮没说「点击开启」",
        );
        router.close();
    });

    test("关着的时候点 ⇒ 发的是 true（不是恒真地发同一个值）", async () => {
        const { router, transport } = await setup();
        // ⚠️ **先关**：内核默认开着 ⇒ ⇒ 第一步先关掉 ⇒ ⇒ 第二步才是
        //   「从关到开」⇒ ⇒ 这条断言**真的**在验「点一下就是反着来」。
        await router.dispatchCommand(Command.setTraceEnabled(false));
        const before = transport.sent.length;
        await clickSwitch(router, switchButton());
        assert.equal(transport.sent.length, before + 1, "点开关没有发出命令");
        assert.deepEqual(
            transport.sent[transport.sent.length - 1].message.payload?.Command,
            { SetTraceEnabled: { enabled: true } },
            "从「关」点一下必须发 true",
        );
        router.close();
    });

    test("命令发不出去 ⇒ 错误显示在**调试面板**里（不是只在页面底下的页脚）", async () => {
        const { router } = await setup();
        // ⚠️ **模拟传输失败**（内核没连上 / 超时）⇒ ⇒ 「点了没反应」的第二个
        //   来源就在这儿 ⇒ ⇒ 失败必须**看得见**。
        router.close();
        await clickSwitch(router, switchButton());
        const err = dom.document.querySelector("[data-debug-error]");
        assert.ok(err, "命令失败了但界面上没有任何提示");
        assert.ok(
            err.textContent.includes("trace"),
            `错误位没有说明是 trace 开关这一条：${err.textContent}`,
        );
    });
});
