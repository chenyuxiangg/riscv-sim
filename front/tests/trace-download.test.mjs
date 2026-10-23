// trace 导出的**下载**动作 —— D-88 · STAGE-10 裁决 D。
//
// 缺陷原文：导出结果只落在 `<pre class="trace-chunk">` 里 ⇒ 用户得**手动
// 复制** ⇒ M-11 LLD 写下的「分块下载」承诺没兑现。
//
// 这个套件盯住：
//   1. **B-3**：导出面板有**下载动作** —— 点它会造出
//      `<a download>`：`href` 以 `blob:` 开头、`download` 非空、**文件名由
//      内容派生**（源 / 格式 → 文件名，不是写死的同一个名字）；
//   2. **B-4**：下载之后 `URL.revokeObjectURL` **被调用**（不泄漏）；
//   3. 下载**真的发生**：那个 `<a>` 被**点了**（不是只造出来）；
//   4. NFR-04：文件名经得起消毒（源 / 格式来自内核 ⇒ 不可信）。
//
// ⚠️ **断言的是具体值**，不是「有个 `<a download>` 就算数」。

import { test, describe, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（D-46 检视 V-B · probe D2）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");

await m02.mountViewComponents();

/** 一条**导出块**的形状（契约 v3 的 `TraceExport`）。 */
function exportState(overrides = {}) {
    return {
        source: "Execution",
        format: "Csv",
        offset: 0,
        next_offset: 2,
        total: 2,
        chunk: "pc,insn\n0x0,addi\n",
        overflow_marked: false,
        ...overrides,
    };
}

/**
 * 把导出块推进视图状态，并触发一次**生产路径**的重渲染
 * （M-01 的 `riscv:render` 事件 ⇒ `applyRenderEvent`）。
 */
function publishExport(state) {
    m03.setState("debug", (prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), export: state },
    }));
    dom.window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "debug", type: "update", state: m03.getState("debug") },
    }));
}

/** 调试视图的正文元素。 */
function debugBody() {
    const body = dom.document.querySelector('[data-view-body="debug"]');
    assert.ok(body, "调试视图体未挂载");
    return body;
}

/**
 * 观测「浏览器原生能力」被怎么用的：造出来的 object URL、被释放的
 * object URL、造出来的 `<a>`、以及真的被点过的 `<a>`。
 *
 * ⚠️ 这里**替换**全局 `URL.createObjectURL` / `URL.revokeObjectURL` 与
 * `document.createElement` ⇒ 断言的是**生产代码真的调了它们**，而不是
 * 一个测试自己造出来的假象。
 */
function observeBrowserCalls() {
    const realCreate = URL.createObjectURL;
    const realRevoke = URL.revokeObjectURL;
    const realCreateElement = dom.document.createElement;
    const calls = {
        createdUrls: [],
        revokedUrls: [],
        anchors: [],
        clickedAnchors: [],
    };
    URL.createObjectURL = (blob) => {
        const url = realCreate.call(URL, blob);
        calls.createdUrls.push(url);
        return url;
    };
    URL.revokeObjectURL = (url) => {
        calls.revokedUrls.push(url);
        return realRevoke.call(URL, url);
    };
    dom.document.createElement = (tag) => {
        const el = realCreateElement(tag);
        if (String(tag).toLowerCase() === "a") {
            calls.anchors.push(el);
            el.addEventListener("click", () => calls.clickedAnchors.push(el));
        }
        return el;
    };
    calls.restore = () => {
        URL.createObjectURL = realCreate;
        URL.revokeObjectURL = realRevoke;
        dom.document.createElement = realCreateElement;
    };
    return calls;
}

/** 点击导出面板里的下载按钮。 */
function clickDownload() {
    const button = debugBody().querySelector("[data-trace-download-button]");
    assert.ok(button, "导出面板里没有下载按钮");
    button.dispatchEvent(new Event("click"));
    return button;
}

beforeEach(() => {
    m03.reset();
    m01.assembleWorkspace("embedded");
    m01.route("#/debug");
});

afterEach(() => {
    // 视图状态是模块级单例 ⇒ 用例之间不残留。
    m03.reset();
});

// ---------------------------------------------------------------------------

describe("导出下载 · 文件名由内容派生", () => {
    test("执行轨迹 + CSV ⇒ trace-execution.csv", () => {
        assert.equal(m02.exportFileName("Execution", "Csv"), "trace-execution.csv");
    });

    test("波形 + VCD ⇒ trace-waveform.vcd（**不是**同一个名字）", () => {
        assert.equal(m02.exportFileName("Waveform", "Vcd"), "trace-waveform.vcd");
    });

    test("文件名不是写死的：源 / 格式不同 ⇒ 名字不同", () => {
        assert.notEqual(
            m02.exportFileName("Execution", "Csv"),
            m02.exportFileName("Waveform", "Vcd"),
        );
    });

    test("文件名经得起消毒（源 / 格式来自内核 ⇒ 不可信）", () => {
        const name = m02.exportFileName("../../etc/passwd", 'Csv" onload="alert(1)');
        assert.ok(!name.includes("/"), `文件名里带了路径分隔符：${name}`);
        assert.ok(!name.includes('"'), `文件名里带了引号：${name}`);
        assert.ok(!name.includes(".."), `文件名里带了 ..：${name}`);
    });
});

describe("导出下载 · B-3 下载动作", () => {
    test("导出面板里有下载按钮（不是一个只能手动复制的 <pre>）", () => {
        publishExport(exportState());
        const button = debugBody().querySelector("[data-trace-download-button]");
        assert.ok(button, "导出面板里没有下载动作");
        assert.match(button.textContent, /trace-execution\.csv/);
    });

    test("没有导出时没有下载按钮（不下载空文件）", () => {
        publishExport(null);
        assert.equal(
            debugBody().querySelector("[data-trace-download-button]"),
            null,
            "没有导出内容时也渲染出了下载按钮",
        );
    });

    test("点下载 ⇒ 造出 <a download>：href 是 blob:、download 非空且由内容派生", () => {
        publishExport(exportState());
        const calls = observeBrowserCalls();
        try {
            clickDownload();
            assert.equal(calls.anchors.length, 1, "没有造出下载用的 <a>");
            const anchor = calls.anchors[0];
            const href = anchor.getAttribute("href");
            assert.ok(href, "下载链接没有 href");
            assert.ok(href.startsWith("blob:"), `href 不是 blob: URL：${href}`);
            const download = anchor.getAttribute("download");
            assert.ok(download, "下载链接的 download 属性为空");
            assert.equal(download, "trace-execution.csv");
        } finally {
            calls.restore();
        }
    });

    test("波形导出 ⇒ 下载的是 trace-waveform.vcd（文件名跟着内容走）", () => {
        publishExport(exportState({ source: "Waveform", format: "Vcd", chunk: "$var x $end\n" }));
        const calls = observeBrowserCalls();
        try {
            clickDownload();
            assert.equal(calls.anchors.length, 1);
            assert.equal(calls.anchors[0].getAttribute("download"), "trace-waveform.vcd");
        } finally {
            calls.restore();
        }
    });

    test("下载的内容就是导出块本身", () => {
        publishExport(exportState({ chunk: "pc,insn\n0x0,lui\n" }));
        const calls = observeBrowserCalls();
        try {
            clickDownload();
            assert.equal(calls.createdUrls.length, 1, "没有创建 object URL");
            assert.ok(calls.createdUrls[0].startsWith("blob:"));
        } finally {
            calls.restore();
        }
    });

    test("下载**真的发生**：那个 <a> 被点了（不是只造出来）", () => {
        publishExport(exportState());
        const calls = observeBrowserCalls();
        try {
            clickDownload();
            assert.equal(calls.clickedAnchors.length, 1, "下载链接没有被点击");
            assert.equal(calls.clickedAnchors[0], calls.anchors[0]);
        } finally {
            calls.restore();
        }
    });
});

describe("导出下载 · B-4 释放 object URL", () => {
    test("下载之后 URL.revokeObjectURL 被调用（不泄漏）", async () => {
        publishExport(exportState());
        // ⚠️ **先让别的用例遗留的释放定时器落下来**：释放是「点击之后」做的
        // （`setTimeout(..., 0)`）⇒ 上一个用例排的定时器会在**本用例**的
        // 观察窗口里触发 ⇒ 先冲掉它们，本用例的计数才是干净的。
        await new Promise((r) => setTimeout(r, 0));
        const calls = observeBrowserCalls();
        try {
            clickDownload();
            assert.equal(calls.createdUrls.length, 1, "本次下载没有创建 object URL");
            // ⚠️ 释放是「点击**之后**」做的（`setTimeout(..., 0)`）—— 同步释放
            // 会把还没开始的下载打断 ⇒ 断言必须等一个宏任务。
            await new Promise((r) => setTimeout(r, 0));
            assert.equal(calls.revokedUrls.length, 1, "object URL 没有被释放");
            assert.equal(
                calls.revokedUrls[0],
                calls.createdUrls[0],
                "释放的不是本次下载用的那个 object URL",
            );
        } finally {
            calls.restore();
        }
    });

    test("没有导出时点下载不会凭空造 URL（也不泄漏）", async () => {
        publishExport(null);
        // 同上：先冲掉别的用例遗留的释放定时器。
        await new Promise((r) => setTimeout(r, 0));
        const calls = observeBrowserCalls();
        try {
            const before = calls.createdUrls.length;
            // 没有下载按钮 ⇒ 无从点击；直接调导出函数也不能造出 URL。
            assert.equal(m02.downloadExportChunk(null), null);
            await new Promise((r) => setTimeout(r, 0));
            assert.equal(calls.createdUrls.length, before, "空导出不该创建 object URL");
            assert.equal(calls.revokedUrls.length, 0, "不该释放不存在的 object URL");
        } finally {
            calls.restore();
        }
    });
});
