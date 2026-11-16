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
const { collectTraceChunks, normalizeTraceFilter, TRACE_EXPORT_MAX_CHUNKS } = await importFront("lib/trace-export.js");
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
 * Build a clean bridge: mock kernel → transport → M-05 → 调试面接线，
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

describe("trace 导出 · 时间/地址筛选归一（纯逻辑）", () => {
    test("空输入 ⇒ ⇒ 不生成 filter（旧命令形状不变）", () => {
        assert.deepEqual(normalizeTraceFilter({}), { ok: true, filter: null });
    });

    test("时间保持十进制字符串、地址是 u32 数字", () => {
        const result = normalizeTraceFilter({
            timeFrom: "9007199254740993",
            addrFrom: 0x1000,
            addrTo: 0x1fff,
        });
        assert.equal(result.ok, true);
        assert.deepEqual(result.filter, {
            time_from_ps: "9007199254740993",
            addr_from: 0x1000,
            addr_to: 0x1fff,
        });
    });

    test("起点大于终点 ⇒ ⇒ 报错，不静默给空结果", () => {
        assert.equal(normalizeTraceFilter({ timeFrom: "20", timeTo: "10" }).ok, false);
        assert.equal(normalizeTraceFilter({ addrFrom: 0x2000, addrTo: 0x1000 }).ok, false);
        assert.equal(normalizeTraceFilter({ timeFrom: "abc" }).ok, false);
    });
});

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

    test("JSON 多块 ⇒ ⇒ 合并 entries，结果是**合法** JSON（不是两段对象裸拼）", async () => {
        const calls = [];
        const result = await collectTraceChunks(async (offset) => {
            calls.push(offset);
            if (offset === 0) {
                return {
                    chunk: JSON.stringify({ entries: [{ addr: 1 }] }),
                    next_offset: 1,
                    total: 2,
                };
            }
            return {
                chunk: JSON.stringify({ entries: [{ addr: 2 }] }),
                next_offset: 2,
                total: 2,
            };
        }, { format: "Json" });
        assert.deepEqual(calls, [0, 1]);
        assert.deepEqual(
            JSON.parse(result.content),
            { entries: [{ addr: 1 }, { addr: 2 }] },
            "两块 JSON 必须合并成一个对象，否则下载出来的文件不是合法 JSON",
        );
        assert.equal(result.chunks, 2);
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
        assert.ok(chunk.textContent.includes("time_ps,addr,kind,data,width"), "面板里没有导出的内容");
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
    test("执行轨迹给出 CSV / JSON 两个格式入口，下载仍只有一个", async () => {
        const { router } = await setup();
        const csv = debugBody().querySelector(
            '[data-action="export-trace"][data-source="Execution"][data-format="Csv"]',
        );
        const json = debugBody().querySelector(
            '[data-action="export-trace"][data-source="Execution"][data-format="Json"]',
        );
        assert.ok(csv, "没有「导出执行轨迹 (CSV)」按钮");
        assert.ok(json, "没有「导出执行轨迹 (JSON)」按钮");
        assert.equal(
            debugBody().querySelectorAll('[data-action="download-trace-local"]').length,
            1,
            "整份下载入口必须仍然只有一个",
        );
        assert.ok(
            debugBody().querySelector("[data-trace-state]"),
            "trace 卡片不在（格式入口跑到别处去了）",
        );
        router.close();
    });

    test("点 JSON 入口 ⇒ ⇒ 真的发 ExportTrace(Execution, Json)", async () => {
        const { router, transport } = await setup();
        const before = exportCommands(transport).length;
        await clickAndDispatch(
            router,
            debugBody().querySelector(
                '[data-action="export-trace"][data-source="Execution"][data-format="Json"]',
            ),
        );
        const exports = exportCommands(transport).slice(before);
        assert.equal(exports.length, 1, "点 JSON 入口没有发导出命令");
        assert.deepEqual(exports[0], {
            source: "Execution",
            format: "Json",
            offset: 0,
            limit: 0,
        });
        router.close();
    });

    test("填了时间/地址窗口 ⇒ ⇒ ExportTrace 带 filter（含首含尾）", async () => {
        const { router, transport } = await setup();
        const before = exportCommands(transport).length;
        const body = debugBody();
        body.querySelector("[data-trace-time-from]").value = "1000";
        body.querySelector("[data-trace-time-to]").value = "2000";
        body.querySelector("[data-trace-addr-from]").value = "0x1000";
        body.querySelector("[data-trace-addr-to]").value = "0x1fff";
        await clickAndDispatch(
            router,
            body.querySelector(
                '[data-action="export-trace"][data-source="Execution"][data-format="Csv"]',
            ),
        );
        const exports = exportCommands(transport).slice(before);
        assert.equal(exports.length, 1);
        assert.deepEqual(exports[0].filter, {
            time_from_ps: "1000",
            time_to_ps: "2000",
            addr_from: 4096,
            addr_to: 8191,
        });
        router.close();
    });

    test("筛选区间非法 ⇒ ⇒ 不发命令", async () => {
        const { router, transport } = await setup();
        const before = exportCommands(transport).length;
        const body = debugBody();
        body.querySelector("[data-trace-time-from]").value = "2000";
        body.querySelector("[data-trace-time-to]").value = "1000";
        await clickAndDispatch(
            router,
            body.querySelector(
                '[data-action="export-trace"][data-source="Execution"][data-format="Csv"]',
            ),
        );
        assert.equal(exportCommands(transport).length, before, "非法筛选不该发给内核");
        router.close();
    });

    test("筛完再下载 ⇒ ⇒ 续导的每一块都带同一个 filter", async () => {
        const { router, transport } = await setup();
        const body = debugBody();
        body.querySelector("[data-trace-time-from]").value = "1000";
        body.querySelector("[data-trace-time-to]").value = "";
        body.querySelector("[data-trace-addr-from]").value = "";
        body.querySelector("[data-trace-addr-to]").value = "";
        await clickAndDispatch(
            router,
            body.querySelector(
                '[data-action="export-trace"][data-source="Execution"][data-format="Csv"]',
            ),
        );
        const before = exportCommands(transport).length;
        const calls = observeBrowserCalls();
        try {
            await clickAndDispatch(router, body.querySelector('[data-action="download-trace-local"]'));
        } finally {
            calls.restore();
        }
        const exports = exportCommands(transport).slice(before);
        assert.ok(exports.length >= 2, "下载没有翻完所有块");
        for (const cmd of exports) {
            assert.deepEqual(cmd.filter, { time_from_ps: "1000" }, "续导丢了筛选条件");
        }
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

    test("导出 JSON 后点下载 ⇒ ⇒ 落盘的是**合法 JSON**（多块 entries 合并）", async () => {
        const { router } = await setup();
        await clickAndDispatch(
            router,
            debugBody().querySelector(
                '[data-action="export-trace"][data-source="Execution"][data-format="Json"]',
            ),
        );
        let blobText = null;
        const origBlob = globalThis.Blob;
        globalThis.Blob = class extends origBlob {
            constructor(parts, opts) {
                super(parts, opts);
                blobText = parts.join("");
            }
        };
        try {
            await clickAndDispatch(
                router,
                debugBody().querySelector('[data-action="download-trace-local"]'),
            );
        } finally {
            globalThis.Blob = origBlob;
        }
        assert.ok(blobText !== null, "没有造出 Blob");
        const parsed = JSON.parse(blobText);
        assert.equal(parsed.entries.length, 4, "JSON 下载没有合并全部块");
        assert.equal(parsed.entries[0].kind, "instruction");
        assert.equal(parsed.entries[2].addr, 256);
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
        // ⇒ mock 的 Execution:Csv 共 4 条数据行、块大小 2 ⇒ ⇒ 两块拼起来是表头 + 4 行。
        const lines = blobText.split("\n").filter((l) => l.length > 0);
        assert.equal(lines.length, 5, `下载的内容只有 ${lines.length} 行（应当是表头 + 全量 4 行）`);
        assert.ok(lines[0].includes("time_ps,addr,kind,data,width"), "第一行不是表头");
        assert.ok(lines[4].includes('"4000",260,write,42,4'), "最后一行不在（没翻完）");
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
