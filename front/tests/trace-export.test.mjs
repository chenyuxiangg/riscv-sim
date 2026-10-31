// 「暂停 ⇒ ⇒ 自动渲染 trace」与「下载轨迹到本地」的**点击路径**门禁
// （v0.62 批）。
//
// ⚠️ **为什么另起一个文件**：本批改的是**两条用户动作**——
//   ① **点暂停** ⇒ ⇒ 自动把 trace 渲染出来（此前必须再点一次「导出执行轨迹」
//      才看得到 ⇒ ⇒ 「开了 trace ⇒ ⇒ 暂停就有东西看」这条链断了一截）；
//   ② **点「下载轨迹到本地」** ⇒ ⇒ 落盘的必须是**整条**轨迹（内核的导出是
//      分块的 ⇒ ⇒ 只发一条命令拿到的是**第一块** ⇒ ⇒ 把它叫「下载轨迹」是
//      **误导**）。
//
// ⇒ ⇒ 这两条都**只能靠真的点**来验 ⇒ ⇒ 断言走**生产信号路径**（视图 → 信号
//   → app → M-05 → mock 内核 → M-05 → 视图）。
//
// ⚠️ **判据要带反向对照**：① trace **明确关着**时点暂停 ⇒ ⇒ **不许**导出
//   （否则「暂停就导出」会变成无脑行为）；② 「下载」**必须翻完所有块**
//   （只发一条命令就是**没翻完** ⇒ ⇒ 那是「点了没反应」的另一种形态）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Command, eventMessage } = await importFront("lib/message.js");
const { collectTraceChunks, TRACE_EXPORT_MAX_CHUNKS } = await importFront("lib/trace-export.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");

await m02.mountViewComponents();

/**
 * An **in-process** transport: the mock kernel answers synchronously ⇒ ⇒
 * 整条链路在一个 tick 内。
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
    // ⚠️ **顶栏也接线**（v0.63 批）⇒ ⇒ 调试视图里那个「暂停」按钮**撤掉**了 ⇒ ⇒
    //   暂停的**唯一**入口在顶栏 ⇒ ⇒ 用例要点的就是**那个**按钮。
    //
    // ⚠️ `wireTopbarRunbar` **先退旧的监听器、再挂新的**⇒ ⇒ 每个用例各接一次
    //   **不会**让同一个按钮挂上两个监听器 ⇒ ⇒ 点一下**只**触发一次。
    app.wireTopbarRunbar({ commandRouter: router });
    m01.assembleWorkspace("embedded");
    m01.route("#/debug");
    return { kernel, transport, router };
}

/**
 * Click a button and let the **production signal path** run.
 *
 * @param {object} router
 * @param {object} button
 * @returns {Promise<void>}
 */
async function clickAndDispatch(router, button) {
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

/**
 * 点**顶栏**那个「暂停」按钮，并**等它真的跑完**。
 *
 * ⚠️ **为什么要等**：顶栏那个按钮是「点一下就走」的（`void handleCommand(...)`）
 *   ⇒ ⇒ 用例若不等 ⇒ ⇒ 断言会落在「状态还没回来」那一格上（**竞态**）⇒ ⇒
 *   这里**轮询到导出命令真的发出去了**为止 ⇒ ⇒ 断言落在一个确定的格上。
 *
 * @param {object} transport
 * @param {number} before — 点击**前**已经发出几条导出命令
 * @returns {Promise<void>}
 */
async function clickTopbarPause(transport, before, expectExport = true) {
    const btn = dom.document.querySelector('[data-topbar-action="pause"]');
    assert.ok(btn, "顶栏没有「暂停」按钮");
    btn.dispatchEvent(new Event("click"));
    if (!expectExport) {
        // ⚠️ **反向对照**：这条**期望没有导出**⇒ ⇒ 不能等「导出出现了」⇒ ⇒
        //   等一个**固定**的往返时间（够一次命令往返）再断言 ⇒ ⇒ 断言落在
        //   「该来的没来」这一格上 ⇒ ⇒ 不是「还没来就断言了」。
        await new Promise((r) => setTimeout(r, 200));
        return;
    }
    for (let i = 0; i < 100; i += 1) {
        if (exportCommands(transport).length > before) return;
        await new Promise((r) => setTimeout(r, 5));
    }
    throw new Error("点顶栏「暂停」之后没有自动导出（超时）");
}

/** The debug view's body element. */
function debugBody() {
    const body = dom.document.querySelector('[data-view-body="debug"]');
    assert.ok(body, "调试视图体未挂载");
    return body;
}

/** Every `ExportTrace` command that reached the kernel. */
function exportCommands(transport) {
    return transport.sent
        .map((s) => s.message.payload?.Command?.ExportTrace)
        .filter((c) => c !== undefined);
}

// ---------------------------------------------------------------------------

describe("trace 导出 · 分页收集（纯逻辑）", () => {
    test("一块就够 ⇒ ⇒ 一次取回、内容就是那一块", async () => {
        const calls = [];
        const result = await collectTraceChunks(async (offset) => {
            calls.push(offset);
            return { chunk: "a\nb\n", next_offset: 2, total: 2 };
        });
        assert.deepEqual(calls, [0]);
        assert.equal(result.content, "a\nb\n");
        assert.equal(result.chunks, 1);
        assert.equal(result.nextOffset, 2);
        assert.equal(result.total, 2);
        assert.equal(result.truncated, false);
    });

    test("多块 ⇒ ⇒ 顺着 next_offset 翻完（不重不漏）", async () => {
        const calls = [];
        const result = await collectTraceChunks(async (offset) => {
            calls.push(offset);
            if (offset === 0) return { chunk: "a\nb\n", next_offset: 2, total: 4 };
            return { chunk: "c\nd\n", next_offset: 4, total: 4 };
        });
        assert.deepEqual(calls, [0, 2], "续导的起点必须是上一块的 next_offset");
        assert.equal(result.content, "a\nb\nc\nd\n");
        assert.equal(result.chunks, 2);
        assert.equal(result.nextOffset, 4);
        assert.equal(result.truncated, false);
    });

    test("内核不推进 ⇒ ⇒ 停（不死循环）且**如实说没拿全**", async () => {
        let calls = 0;
        const result = await collectTraceChunks(async () => {
            calls += 1;
            return { chunk: "x\n", next_offset: 0, total: 99 };
        });
        assert.equal(calls, 1, "内核不推进时必须停下来");
        assert.equal(result.truncated, true, "没拿全必须如实说");
    });

    test("块数超上限 ⇒ ⇒ 停且如实说没拿全", async () => {
        let calls = 0;
        const result = await collectTraceChunks(async () => {
            calls += 1;
            return { chunk: "x\n", next_offset: calls, total: 100000 };
        }, { maxChunks: 3 });
        assert.equal(calls, 3);
        assert.equal(result.truncated, true);
        assert.ok(TRACE_EXPORT_MAX_CHUNKS >= 20, "上限必须离正常规模（20 块）很远");
    });
});

describe("trace 导出 · 暂停 ⇒ ⇒ 自动渲染", () => {
    test("trace **开着**时点暂停 ⇒ ⇒ 自动发一条 ExportTrace（面板自动有内容）", async () => {
        const { router, transport } = await setup();
        // ⇒ 先让内核回报「开着」。
        await router.dispatchCommand(Command.setTraceEnabled(true));
        const before = exportCommands(transport).length;

        await clickTopbarPause(transport, before);

        const exports = exportCommands(transport);
        assert.equal(exports.length, before + 1, "点暂停没有自动导出");
        assert.deepEqual(exports[exports.length - 1], {
            source: "Execution",
            format: "Csv",
            offset: 0,
            limit: 0,
        });
        // ⇒ ⇒ 面板**真的有内容**（不是只发了命令）。
        const chunk = dom.document.querySelector("[data-trace-chunk]");
        assert.ok(chunk, "面板没有内容区");
        assert.ok(chunk.textContent.includes("kind,addr,value"), "面板里没有导出的内容");
        router.close();
    });

    test("**反向对照**：trace **明确关着**时点暂停 ⇒ ⇒ **不**导出", async () => {
        const { router, transport } = await setup();
        await router.dispatchCommand(Command.setTraceEnabled(false));
        const before = exportCommands(transport).length;

        await clickTopbarPause(transport, before, false);

        assert.equal(
            exportCommands(transport).length,
            before,
            "trace 明确关着时不该自动导出（用户说了不要记录）",
        );
        router.close();
    });

    test("状态**未知**时点暂停 ⇒ ⇒ 照样导出（内核默认就在录 ⇒ 面板自己会说出真相）", async () => {
        const { router, transport } = await setup();
        const before = exportCommands(transport).length;

        await clickTopbarPause(transport, before);

        assert.equal(
            exportCommands(transport).length,
            before + 1,
            "状态未知时也该导出（内核默认就在录）",
        );
        router.close();
    });

    test("暂停**只有顶栏那一个**入口 ⇒ ⇒ 走 app 层就自动导出", async () => {
        const { router, transport } = await setup();
        await router.dispatchCommand(Command.setTraceEnabled(true));
        const before = exportCommands(transport).length;

        // ⚠️ **v0.63 批**：调试视图里那个「暂停」按钮**撤掉**了 ⇒ ⇒ 暂停的**唯一**
        //   入口是顶栏 ⇒ ⇒ 界面**不再有**同一个动作的第二个入口 ⇒ ⇒ 「一条路」
        //   的判据就在这里（顶栏那个按钮**直接调** `handleCommand` ⇒ ⇒ 见
        //   `front/tests/run-loop.test.mjs` 的 A-12 与 `tools/browser-verify.mjs`）。
        await app.handleCommand({ action: "pause" }, { commandRouter: router });

        assert.equal(
            exportCommands(transport).length,
            before + 1,
            "暂停没有自动导出（两个暂停入口必须共用这一条路）",
        );
        router.close();
    });
});

describe("trace 导出 · 「下载轨迹到本地」", () => {
    test("调试视图里**没有**旧的「导出执行轨迹」按钮，新按钮写着「下载轨迹到本地」", async () => {
        const { router } = await setup();
        assert.equal(
            debugBody().querySelector('[data-action="export-trace"][data-source="Execution"]'),
            null,
            "旧的「导出执行轨迹」按钮还在",
        );
        const btn = debugBody().querySelector('[data-action="download-trace-local"]');
        assert.ok(btn, "没有「下载轨迹到本地」按钮");
        assert.match(btn.textContent, /下载轨迹到本地/);
        // ⚠️ **它住在 trace 卡片里**（v0.63 批：工具条整块撤掉了）⇒ ⇒ trace
        //   的东西**只有那一个落点**。
        assert.ok(
            debugBody().querySelector("[data-trace-state]"),
            "trace 卡片不在（下载按钮跑到别处去了）",
        );
        router.close();
    });

    test("点它 ⇒ ⇒ **翻完所有块**并落盘（内容是全部块拼起来的）", async () => {
        const { router, transport } = await setup();
        const before = exportCommands(transport).length;

        // ⚠️ 观察浏览器调用（造 `<a download>`）。
        const calls = observeBrowserCalls();
        let downloaded = null;
        try {
            await clickAndDispatch(router, debugBody().querySelector('[data-action="download-trace-local"]'));
            assert.equal(calls.anchors.length, 1, "没有造出下载用的 <a>");
            downloaded = calls.anchors[0];
        } finally {
            calls.restore();
        }

        // ⚠️ **翻完**：mock 的导出块大小 = 2、Execution:Csv 共 4 行 ⇒ ⇒ 至少两块。
        const exports = exportCommands(transport).slice(before);
        assert.ok(exports.length >= 2, `只发了 ${exports.length} 条导出命令（没翻完）`);
        assert.deepEqual(exports[0].offset, 0, "第一块必须从 0 开始");
        assert.deepEqual(exports[1].offset, 2, "第二块的起点必须是上一块的 next_offset");

        assert.equal(downloaded.getAttribute("download"), "trace-execution.csv");
        assert.ok(downloaded.getAttribute("href").startsWith("blob:"), "href 不是 blob: URL");
        router.close();
    });

    test("下载的内容 = **全部**块（不是第一块）", async () => {
        const { router } = await setup();
        // ⇒ 先在视图状态里放一份导出（面板那块）⇒ ⇒ 再点下载 ⇒ ⇒ 对比两者。
        let blobText = null;
        const origBlob = globalThis.Blob;
        globalThis.Blob = class extends origBlob {
            constructor(parts, opts) {
                super(parts, opts);
                blobText = parts.join("");
            }
        };
        try {
            await clickAndDispatch(router, debugBody().querySelector('[data-action="download-trace-local"]'));
        } finally {
            globalThis.Blob = origBlob;
        }
        assert.ok(blobText !== null, "没有造出 Blob");
        // ⇒ mock 的 Execution:Csv 共 4 行、块大小 2 ⇒ ⇒ 两块拼起来是 4 行。
        const lines = blobText.split("\n").filter((l) => l.length > 0);
        assert.equal(lines.length, 4, `下载的内容只有 ${lines.length} 行（应当是全量 4 行）`);
        assert.ok(lines[0].includes("kind,addr,value"), "第一行不是表头");
        assert.ok(lines[3].includes("mem,00000100,0000002a"), "最后一行不在（没翻完）");
        router.close();
    });
});

/**
 * Observe the browser calls the download helper makes (the `<a>` it creates).
 *
 * @returns {{anchors: object[], restore: () => void}}
 */
function observeBrowserCalls() {
    const doc = dom.document;
    const origCreate = doc.createElement.bind(doc);
    const anchors = [];
    doc.createElement = (tag) => {
        const el = origCreate(tag);
        if (String(tag).toLowerCase() === "a") anchors.push(el);
        return el;
    };
    return {
        anchors,
        restore() {
            doc.createElement = origCreate;
        },
    };
}
