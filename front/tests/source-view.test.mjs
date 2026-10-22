// STAGE-7 验收 A-15..A-20 的前端门禁：源码视图（装载 / 匹配 / 高亮）。
//
// 这个套件盯住四件事：
//
//   1. **路径尾部逐段匹配**（裁决 F）：`../../src/main.c` ↔ `src/main.c`、
//      `./kernel/base/hook/src/hook.c` ↔ `kernel/base/hook/src/hook.c`
//      都能匹配上（前者靠 `.` / `..` 段的**正规化**）；
//   2. **三态如实区分**（裁决 J）：「没有源码树」/「匹配不到」/「匹配到
//      多个」**不合并**——多候选时**列出候选**，⛔ 不静默挑第一个；
//   3. **源码不进内核**（裁决 E）：装载源码**不发**任何命令 / 帧，
//      源码树只进 M-03 视图状态；装载工程后源码树**仍在**；
//   4. **当前 PC ⇒ `ResolveLine` ⇒ 高亮对应行**（A-16）：点击按钮发出
//      `resolve-line` 命令，内核回的 `SourceLocation` 落到视图状态，
//      视图把**那一行**高亮出来。
//
// ⚠️ **断言必须同步**：这里的传输是**进程内**的（mock 内核同步回帧）⇒
// 「事件 → store → 重渲染」全都在同一个 tick 内完成。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（D-46 检视 V-B · probe D2）：`app.js` 是
// 引导入口——**导入即启动**，它的 `bootstrap()` 是异步的，会在任意两个
// 用例之间动 M-03 的模块级单例。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const m10 = await importFront("services/m10_registermemory.js");
const { Command, Event: EventPayload, eventMessage } = await importFront("lib/message.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");
const sourceView = await importFront("views/components/source-view.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

/**
 * An **in-process** transport: the mock kernel answers synchronously, so
 * the whole chain (M-05 → mock kernel → M-05 → 视图) runs inside one
 * tick.
 *
 * @param {object} kernel — the mock kernel
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
        /** Deliver a pushed event (no request id) to the router. */
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
 * plus the mounted + rendered source view.
 *
 * @returns {Promise<{kernel: object, transport: object, router: object, debug: object}>}
 */
async function setup() {
    m03.reset();
    const kernel = createMockKernel();
    const transport = inProcessTransport(kernel);
    const router = await m05.mountCommandRouter({ transport });
    const debug = app.wireDebug(router);
    m01.assembleWorkspace("embedded");
    m01.route("#/source");
    return { kernel, transport, router, debug };
}

/** The source view's body element. */
function sourceBody() {
    const body = dom.document.querySelector('[data-view-body="source"]');
    assert.ok(body, "源码视图体未挂载");
    return body;
}

/**
 * A fake `File` — the DOM stub has no `File`, and the view only needs
 * `name` / `webkitRelativePath` / `text()`.
 *
 * @param {string} name
 * @param {string} text
 * @param {string} [relativePath]
 */
function fakeFile(name, text, relativePath = "") {
    return {
        name,
        webkitRelativePath: relativePath,
        async text() { return text; },
    };
}

/**
 * Load files into the source view through the **production** path: the
 * view's `change` listener reads them and writes the tree into M-03.
 *
 * @param {object} body — the view body
 * @param {Array<object>} files
 */
function loadFiles(body, files) {
    const input = body.querySelector('[data-source-input="files"]');
    assert.ok(input, "文件选择入口不存在");
    input.files = files;
    const ev = new Event("change");
    // The stub dispatches with bubbling; the listener is on the body.
    input.dispatchEvent(ev);
}

// ---------------------------------------------------------------------------
// 1. 路径正规化 + 尾部逐段匹配（裁决 F）
// ---------------------------------------------------------------------------

describe("源码路径匹配（裁决 F）", () => {
    test("尾部逐段匹配：../../src/main.c ↔ src/main.c", () => {
        const tree = { "src/main.c": "int main(){return 0;}" };
        const match = sourceView.matchSourceFile(tree, "../../src/main.c");
        assert.deepEqual(match, { kind: "unique", key: "src/main.c" });
    });

    test("尾部逐段匹配：相对编译出来的 ./kernel/... ↔ kernel/...", () => {
        // ⚠️ 用**相对** include 路径编译时，DWARF 里的目录项长这样
        // （`./kernel/base/hook/src`）⇒ 正规化之后才匹配得上。
        const tree = { "kernel/base/hook/src/hook.c": "int hook(){return 0;}" };
        const match = sourceView.matchSourceFile(tree, "./kernel/base/hook/src/hook.c");
        assert.deepEqual(match, { kind: "unique", key: "kernel/base/hook/src/hook.c" });
    });

    test("尾部逐段匹配：绝对路径 ↔ 相对 key", () => {
        const tree = { "kernel/base/hook/src/hook.c": "x" };
        const match = sourceView.matchSourceFile(tree, "/tmp/osz-probe/kernel/base/hook/src/hook.c");
        assert.deepEqual(match, { kind: "unique", key: "kernel/base/hook/src/hook.c" });
    });

    test("唯一命中：返回那个 key", () => {
        const tree = { "src/main.c": "a", "src/util.c": "b" };
        const match = sourceView.matchSourceFile(tree, "src/util.c");
        assert.deepEqual(match, { kind: "unique", key: "src/util.c" });
    });

    test("多个命中 ⇒ 列出候选（⛔ 不静默挑第一个）", () => {
        const tree = {
            "kernel/base/hook/src/hook.c": "a",
            "other/kernel/base/hook/src/hook.c": "b",
        };
        const match = sourceView.matchSourceFile(tree, "kernel/base/hook/src/hook.c");
        assert.equal(match.kind, "ambiguous");
        assert.deepEqual(match.candidates, [
            "kernel/base/hook/src/hook.c",
            "other/kernel/base/hook/src/hook.c",
        ]);
        assert.equal(match.key, undefined, "多候选时**不得**挑一个出来");
    });

    test("零命中 ⇒ none / no-match（与「没有源码树」不是一回事）", () => {
        const tree = { "src/main.c": "a" };
        const match = sourceView.matchSourceFile(tree, "kernel/base/hook/src/hook.c");
        assert.deepEqual(match, { kind: "none", reason: "no-match" });
    });

    test("没有源码树 ⇒ none / no-tree", () => {
        assert.deepEqual(sourceView.matchSourceFile({}, "src/main.c"), {
            kind: "none",
            reason: "no-tree",
        });
        assert.deepEqual(sourceView.matchSourceFile(null, "src/main.c"), {
            kind: "none",
            reason: "no-tree",
        });
    });

    test("正规化：. 与 .. 段被消解", () => {
        assert.equal(sourceView.normalizePath("./a/b.c"), "a/b.c");
        assert.equal(sourceView.normalizePath("a/../b.c"), "b.c");
        assert.equal(sourceView.normalizePath("../../src/main.c"), "src/main.c");
        assert.equal(sourceView.normalizePath("/abs/path/x.c"), "/abs/path/x.c");
    });

    // ⚠️ **中段**的 `..`（V-B 检视 VB-6）：前导的 `../` 尾部规则自己就能
    // 吸收（短的那串本来就是长的那串的后缀）⇒ 那三条**匹配级**用例
    // **碰不到**正规化 ⇒ 去掉正规化它们照样绿。真正只有正规化能救的是
    // **中段**的 `..`：DWARF 给 `x/a/../b.c`、本地树给 `x/b.c` ⇒
    // 不正规化就**匹配不上**。
    test("尾部逐段匹配：中段的 .. 必须被正规化消解才匹配得上", () => {
        const tree = { "x/b.c": "a" };
        const match = sourceView.matchSourceFile(tree, "x/a/../b.c");
        assert.deepEqual(
            match,
            { kind: "unique", key: "x/b.c" },
            "中段的 `..` 消解后应当匹配上",
        );
    });
});

// ---------------------------------------------------------------------------
// 2. 视图：三态如实显示 + 当前行高亮
// ---------------------------------------------------------------------------

describe("源码视图（A-15 / A-16 / A-17 / A-18 / A-19）", () => {
    test("装载源码 ⇒ 源码树进 M-03 视图状态，且**不发**任何命令", async () => {
        const { transport } = await setup();
        const body = sourceBody();

        // ⚠️ 断言要落在**信号**上，不能只落在帧上：视图层的命令先以
        // `riscv:command` 信号出现，只有被 app.js 接住才会变成帧 ⇒ 只看
        // 帧的话，「多发了一条没人接的命令」是**看不见**的（判别力缺口）。
        /** @type {object[]} */
        const signals = [];
        const listener = (ev) => signals.push(ev.detail);
        dom.window.addEventListener("riscv:command", listener);

        loadFiles(body, [fakeFile("main.c", "int main(void){\n  return 0;\n}\n")]);

        // ⚠️ 文件读取是**异步**的（`file.text()`）⇒ 等一个微任务。
        await new Promise((r) => setTimeout(r, 0));
        dom.window.removeEventListener("riscv:command", listener);

        const tree = m03.getState("source")?.source?.tree ?? {};
        assert.equal(Object.keys(tree).length, 1, "源码树必须进 M-03");
        assert.ok(tree["main.c"], "main.c 必须在树里");
        assert.deepEqual(
            signals,
            [],
            "装载源码**不得**发出任何命令信号（源码不进内核，裁决 E）",
        );
        assert.equal(
            transport.sent.length,
            0,
            "装载源码**不得**发任何帧（源码不进内核，裁决 E）",
        );
    });

    test("目录装载 ⇒ key 是相对路径", async () => {
        await setup();
        const body = sourceBody();
        loadFiles(body, [
            fakeFile("hook.c", "int hook(void){return 0;}\n", "kernel/base/hook/src/hook.c"),
        ]);
        await new Promise((r) => setTimeout(r, 0));
        const tree = m03.getState("source")?.source?.tree ?? {};
        assert.ok(tree["kernel/base/hook/src/hook.c"], "相对路径必须作 key");
    });

    test("零命中 / 无源码树 ⇒ 如实显示，**不**显示错误的行号", async () => {
        await setup();
        const body = sourceBody();
        // 没有源码树 ⇒ 「尚未装载源码」。
        assert.match(body.innerHTML, /尚未装载源码/);

        // 有源码树但匹配不到 ⇒ 「匹配不到」。
        loadFiles(body, [fakeFile("main.c", "x")]);
        await new Promise((r) => setTimeout(r, 0));
        m03.setState("source", (prev) => ({
            ...prev,
            source: {
                ...(prev.source ?? {}),
                location: { addr: 0x1000, file: "kernel/base/hook/src/hook.c", line: 9 },
            },
        }));
        m01.route("#/source");
        const body2 = sourceBody();
        assert.match(body2.innerHTML, /匹配不到/);
        assert.doesNotMatch(body2.innerHTML, /is-current/, "不得高亮任何一行");
    });

    test("当前 PC ⇒ resolve-line 命令 ⇒ SourceLocation ⇒ 高亮对应行", async () => {
        const { transport, router } = await setup();
        const body = sourceBody();
        loadFiles(body, [fakeFile("main.c", "l1\nl2\nl3\nl4\n")]);
        await new Promise((r) => setTimeout(r, 0));

        // ⚠️ **PC 必须走生产接线**（M-10 → `viewIds` → source 视图）。
        // 手工往状态里塞 `sim.pc` 断言的是「测试自己造出来的状态」，
        // 不是生产状态 —— V-B 检视 **VB-1**：生产路径上 source 视图
        // **永远**拿不到 PC（`viewIds` 默认值不含 `source`）⇒ 按钮恒问
        // 地址 0，而门禁当时 243/243 **全绿**。
        //
        // ⚠️ 这里用 `app.REGISTER_MEMORY_VIEW_IDS`（**生产那一份**）：
        // 常量里少了 `"source"` ⇒ 这条用例**跟着**变红 ⇒ 这才是判别力。
        const regmem = await m10.mountRegisterMemory({
            commandRouter: router,
            transport,
            viewStore: m03,
            viewIds: app.REGISTER_MEMORY_VIEW_IDS,
        });
        transport.emit(EventPayload.simStateDelta([], 0x1000, 0));
        regmem.flushRender();
        m01.route("#/source");

        const button = sourceBody().querySelector('[data-action="resolve-line"]');
        assert.ok(button, "「解析当前 PC」按钮不存在");
        assert.equal(
            button.getAttribute("data-addr"),
            String(0x1000),
            "按钮必须带上当前 PC（生产接线：M-10 → viewIds → source）",
        );

        /** @type {object[]} */
        const signals = [];
        const listener = (ev) => {
            signals.push(ev.detail);
            void app.handleCommand(ev.detail, { commandRouter: router });
        };
        dom.window.addEventListener("riscv:command", listener);
        button.dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        assert.equal(signals.length, 1, "必须发出**一条**命令");
        assert.equal(signals[0].action, "resolve-line");
        assert.equal(signals[0].viewId, "source");
        assert.equal(signals[0].params.addr, "4096", "参数是当前 PC");

        // ⚠️ **必须断言命令真的被派发**（V-B 检视 **VB-3**）：只断言信号
        // 的话，把 app.js 的 `case "resolve-line"` 整段删掉（视图发的信号
        // 没人接、点按钮什么都不会发生）门禁照样 243/243 全绿。
        //
        // ⚠️ 线上是**信封**：`{ payload: { Command: { ResolveLine: {...} } } }`。
        const resolveCmds = transport.sent.filter(
            (m) => m.message?.payload?.Command?.ResolveLine !== undefined,
        );
        assert.equal(
            resolveCmds.length,
            1,
            "必须**真的**派发一条 ResolveLine 命令到内核",
        );
        assert.equal(
            Number(resolveCmds[0].message.payload.Command.ResolveLine.addr),
            0x1000,
            "派发的命令必须带上当前 PC",
        );

        // 内核回了 `SourceLocation` ⇒ 落到视图状态 ⇒ 视图高亮那一行。
        transport.emit(
            EventPayload.sourceLocation(0x1000, "main.c", 3),
        );
        const state = m03.getState("source")?.source ?? {};
        assert.equal(state.location?.file, "main.c");
        assert.equal(state.location?.line, 3);

        m01.route("#/source");
        const body2 = sourceBody();
        const html = body2.innerHTML;
        assert.match(html, /is-current/, "当前行必须高亮");
        assert.match(html, /data-src-line="3"/, "高亮的必须是第 3 行");
        // ⚠️ **必须钉住「高亮的是哪一行」**（V-B 检视 **VB-4**）：只断言
        // 「有 `is-current`」+「有 `data-src-line="3"`」的话，把高亮标到
        // **相邻的错误行**（`no === wanted + 1`）门禁照样全绿 —— 那是
        // **两个出自同一个变量**的断言，只验了自洽。
        const marked = body2.querySelector(".src-line.is-current");
        assert.ok(marked, "必须有一行被标成当前行");
        assert.equal(
            marked.getAttribute("data-src-line"),
            "3",
            "被标成当前行的**行号**必须正好是 3",
        );
        assert.match(
            marked.textContent ?? "",
            /l3/,
            "被标成当前行的**内容**必须是第 3 行的内容",
        );
        const first = body2.querySelector(".src-line");
        assert.equal(
            first?.getAttribute("data-src-line"),
            "1",
            "第一行的行号必须是 1（不得整体偏移）",
        );
    });

    test("多候选 ⇒ 视图列出候选，**不**静默挑一个", async () => {
        const { transport } = await setup();
        const body = sourceBody();
        loadFiles(body, [
            fakeFile("hook.c", "a\n", "kernel/base/hook/src/hook.c"),
            fakeFile("hook.c", "b\n", "other/kernel/base/hook/src/hook.c"),
        ]);
        await new Promise((r) => setTimeout(r, 0));

        transport.emit(
            EventPayload.sourceLocation(0x1000, "kernel/base/hook/src/hook.c", 1),
        );
        m01.route("#/source");
        const html = sourceBody().innerHTML;
        assert.match(html, /data-source-candidates/, "必须列出候选");
        assert.match(html, /other\/kernel\/base\/hook\/src\/hook\.c/);
        assert.doesNotMatch(html, /is-current/, "多候选时**不得**擅自高亮");

        // 用户点一个候选 ⇒ 高亮那一行。
        const pick = sourceBody().querySelector('[data-action="pick-candidate"]');
        assert.ok(pick, "候选必须可点选");
        pick.dispatchEvent(new Event("click"));
        const html2 = sourceBody().innerHTML;
        assert.match(html2, /is-current/, "用户选定后才可以高亮");
    });

    test("源码**不进内核**：装载工程后源码树仍在（A-20）", async () => {
        const { debug } = await setup();
        const body = sourceBody();
        loadFiles(body, [fakeFile("main.c", "x\n")]);
        await new Promise((r) => setTimeout(r, 0));
        assert.equal(Object.keys(m03.getState("source")?.source?.tree ?? {}).length, 1);

        // ⚠️ **必须用真的 `wireDebug(router).reset()`**（V-B 检视
        // **VB-2**）：原用例传的是 `{ reset() {} }` **空壳 stub** ⇒ 生产里
        // 真正会动源码树的那段代码**根本没被执行** ⇒ 把生产点改成
        // 「装载工程时把 `source.tree` 一起清空」，门禁 243/243 **照样
        // 全绿** ⇒ 这条断言是**恒绿**的。
        //
        // 这里先塞一个行号进去（模拟「上一个程序的行号」），这样
        // 「清行号、留源码树」两半都能被钉住。
        m03.setState("source", (prev) => ({
            ...prev,
            source: { ...(prev.source ?? {}), location: { addr: 0x1000, file: "main.c", line: 1 } },
        }));

        // 装载新工程（内核侧的事）⇒ 源码树**必须还在**，行号**必须清掉**。
        await app.loadProjectIntoKernel(
            {
                registerMemory: { loadProject: async () => {} },
                waveform: { reset() {}, subscribeProbeStream: async () => ({}) },
                debug,
            },
            {},
        );
        const after = m03.getState("source")?.source ?? {};
        assert.equal(
            Object.keys(after.tree ?? {}).length,
            1,
            "装载工程**不得**把用户装的源码树清掉",
        );
        assert.equal(
            after.location,
            null,
            "装载工程**必须**清掉上一个程序的行号（新工程 = 新程序）",
        );
    });

    // ⚠️ **内核的两种「没有答案」必须分开**（V-B 检视 **VB-5**）：
    //   - `line === 0`          ⇒ 压根没有 DWARF
    //   - `line === 0xFFFFFFFF` ⇒ 有 DWARF 但地址没命中行表
    // 之前这两种与「**还没问**」渲染成**同一句**「尚未解析」⇒ 用户点完
    // 按钮看到没变化 ⇒ 以为没点生效 ⇒ 无限点下去。
    test("内核的两种「没有答案」必须与「尚未解析」分开显示（VB-5）", async () => {
        const { transport } = await setup();
        const body = sourceBody();
        loadFiles(body, [fakeFile("main.c", "l1\nl2\n")]);
        await new Promise((r) => setTimeout(r, 0));

        // 压根没有 DWARF（内核的形状：file = ""，line = 0）
        transport.emit(EventPayload.sourceLocation(0x1000, "", 0));
        m01.route("#/source");
        let html = sourceBody().innerHTML;
        assert.match(html, /没有 DWARF 行表/, "没有 DWARF 必须说清楚");
        assert.doesNotMatch(html, /尚未解析/, "不能与「尚未解析」混成一句");
        assert.doesNotMatch(html, /4294967295/, "不得显示无意义的行号");
        assert.doesNotMatch(html, /is-current/, "没有答案时不得高亮任何行");

        // 有 DWARF 但地址没命中（内核的形状：file = ""，line = 0xFFFFFFFF）
        transport.emit(EventPayload.sourceLocation(0x1000, "", 0xffffffff));
        m01.route("#/source");
        html = sourceBody().innerHTML;
        assert.match(html, /不在 DWARF 行表内/, "地址没命中必须说清楚");
        assert.doesNotMatch(html, /4294967295/, "不得显示无意义的行号");
        assert.doesNotMatch(html, /is-current/, "没有答案时不得高亮任何行");
    });

    test("视图注册：#\/source 路由存在且带 view.source 能力位", async () => {
        await setup();
        const ws = m01.currentWorkspace();
        const view = ws.views.find((v) => v.hash === "source");
        assert.ok(view, "源码视图必须被注册");
        assert.ok(ws.capBits.includes("view.source"));
        const tabs = dom.document.querySelectorAll("[data-view-tab]");
        const tab = [...tabs].find((t) => t.getAttribute("data-view-tab") === "source");
        assert.ok(tab, "顶栏必须有源码页签");
    });
});
