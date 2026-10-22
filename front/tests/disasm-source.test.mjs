// v0.24 · Lane K —— 「反汇编面板逐条显示 file / line」的前端门禁（K-3）。
//
// 这个套件盯三件事：
//
//   1. 条目**有** `file` + `line` ⇒ 行显示成 `file:line`；
//   2. 条目**没有**行号 ⇒ 显示「**无行号**」——**不是**空、**不是** `0`、
//      **不是** `null`；
//   3. 条目**只有一半**（只有 `file` 或只有 `line`）⇒ **如实**显示，
//      **不编造**另一半出来。
//
// ⚠️ **判别力**：第 2 条是「绝不伪造行号」这条不变式的守门人——把
// 「无行号」改成显示 `0` 或 `null`（或留空）⇒ 本套件**必须变红**。
// 第 3 条防的是另一半编造：只有 `file` 时显示一个**编出来的**行号
// （或只有 `line` 时显示一个**编出来的**文件名）⇒ 也必须变红。
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

// ⚠️ **关掉 `app.js` 的自启动**（与 `disasm.test.mjs` 同一条理由）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 一条**有**源码位置的条目。 */
const ENTRY_WITH_SOURCE = {
    addr: 0x84000010,
    code: 0x02a00093,
    mnemonic: "addi ra, zero, 42",
    width: 4,
    file: "/tmp/osz-probe/kernel/base/hook/src/hook.c",
    line: 42,
};

/** 一条**没有**源码位置的条目（契约 v4 的两个 `Option` 都是 `null`）。 */
const ENTRY_WITHOUT_SOURCE = {
    addr: 0x84000014,
    code: 0x00008067,
    mnemonic: "ret",
    width: 4,
    file: null,
    line: null,
};

/** 「无行号」的**如实**措辞。 */
const NO_LINE = "无行号";

// ---------------------------------------------------------------------------

/**
 * 把一组反汇编条目塞进视图状态并**渲染**调试视图。
 *
 * ⚠️ 状态是**直接注入**的：mock 内核给不出真 DWARF 行表 ⇒ 这里要测的
 * 是**视图的活**（怎么显示 `file` / `line`），不是内核的活。
 *
 * @param {object[]} entries
 */
function renderWithEntries(entries) {
    m03.setState("debug", (prev) => ({
        ...prev,
        sim: { ...(prev.sim ?? {}), disasm: entries },
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

/** 第 n 行的「源码」单元文本。 */
function sourceCellText(index) {
    const rows = debugBody().querySelectorAll("tr.disasm-row");
    const row = rows[index];
    assert.ok(row, `第 ${index} 行没渲染`);
    const cell = row.querySelector(".disasm-source");
    assert.ok(cell, `第 ${index} 行没有「源码」单元`);
    return String(cell.textContent ?? "");
}

// ---------------------------------------------------------------------------

describe("反汇编面板 · 逐条源码位置（K-3）", () => {
    test("有条目**有** file + line ⇒ 显示 `file:line`", async () => {
        m03.reset();
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        renderWithEntries([ENTRY_WITH_SOURCE]);
        assert.equal(
            sourceCellText(0),
            "/tmp/osz-probe/kernel/base/hook/src/hook.c:42",
        );
    });

    test("条目**没有**行号 ⇒ 显示「无行号」（不是空 / 0 / null）", async () => {
        m03.reset();
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        renderWithEntries([ENTRY_WITHOUT_SOURCE]);
        const text = sourceCellText(0);
        assert.equal(text, NO_LINE, `应当显示「${NO_LINE}」，实际是 ${JSON.stringify(text)}`);
        assert.notEqual(text, "", "不许留空");
        assert.notEqual(text, "0", "不许显示 0");
        assert.notEqual(text, "null", "不许显示 null");
        assert.notEqual(text, "undefined", "不许显示 undefined");
    });

    test("条目**只有** file 没有 line ⇒ 如实显示，不编造行号", async () => {
        m03.reset();
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        renderWithEntries([{ ...ENTRY_WITH_SOURCE, line: null }]);
        const text = sourceCellText(0);
        assert.ok(
            text.includes("/tmp/osz-probe/kernel/base/hook/src/hook.c"),
            `文件名应当如实显示：${text}`,
        );
        assert.ok(text.includes(NO_LINE), `行号缺失应当说清楚：${text}`);
        assert.ok(!text.includes(":42"), `不许编造一个行号出来：${text}`);
        assert.ok(!/\d/.test(text.replace(/\/tmp\/osz-probe\/kernel\/base\/hook\/src\/hook\.c/g, "")),
            `除路径外不许出现任何数字（那是编出来的行号）：${text}`);
    });

    test("条目**只有** line 没有 file ⇒ 如实显示，不编造文件名", async () => {
        m03.reset();
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        renderWithEntries([{ ...ENTRY_WITH_SOURCE, file: null }]);
        const text = sourceCellText(0);
        assert.ok(text.includes("42"), `行号应当如实显示：${text}`);
        assert.ok(!text.includes(".c"), `不许编造一个文件名出来：${text}`);
        assert.ok(!text.includes("hook"), `不许编造一个文件名出来：${text}`);
    });

    test("`line` 是 0 ⇒ 同样算「无行号」（行号是 1 起的）", async () => {
        m03.reset();
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        renderWithEntries([{ ...ENTRY_WITH_SOURCE, line: 0 }]);
        assert.equal(sourceCellText(0).includes(NO_LINE), true);
    });

    test("纯函数 `formatSourceLocation` 的四种形态（写死的字面量）", async () => {
        const { formatSourceLocation, NO_SOURCE_LINE_LABEL } = m02;
        assert.equal(NO_SOURCE_LINE_LABEL, NO_LINE);
        assert.equal(
            formatSourceLocation({ file: "src/main.c", line: 42 }),
            "src/main.c:42",
        );
        assert.equal(formatSourceLocation({ file: null, line: null }), NO_LINE);
        assert.equal(formatSourceLocation({ file: undefined, line: undefined }), NO_LINE);
        assert.equal(formatSourceLocation(null), NO_LINE);
        assert.ok(
            formatSourceLocation({ file: "src/main.c", line: null }).includes(NO_LINE),
        );
        assert.ok(
            formatSourceLocation({ file: null, line: 42 }).includes("42"),
        );
    });
});
