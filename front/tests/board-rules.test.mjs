// v0.33 · Lane B —— 板档案的「合法性规则集版本」（STAGE-18 · D-143 · 裁决 E ~ J）
//
// 本套件盯住验收 **B-1 ~ B-7**：
//
//   B-1 新落库的条目**带** `rules_version`（= 当前版本 3）
//   B-2 没有 `rules_version` 的老条目 ⇒ `checkBoardRules` 报
//       `{ok:false, reason:"rules"}`（expected = 3、actual = 1）
//   B-3 当前版本条目 ⇒ `checkBoardRules` ⇒ ok
//   B-4 `checkBoardVersion` 对 `schema_version != 1` 仍报 `reason:"version"`
//       ⇒ ⇒ **不与 `rules` 混**（两个 reason，两件事）
//   B-5 选用**旧规则**的板 ⇒ **明确提示**（不静默）
//   B-6 内核**接受** ⇒ 条目**被升级**（带上当前 `rules_version`）⇒ 且**有提示**
//   B-7 内核**拒绝** ⇒ 显示**内核原文** ⇒ **不吞**、**不静默失效**
//
//   C-4 存量板（`rules_version = 2`）⇒ 选用**明确提示** ⇒ 内核**接受**后 ⇒
//       条目**升级**到 3（STAGE-19 · 裁决 F：规则集**收紧** ⇒ 版本 2 → 3 ⇒ ⇒
//       存量板**必须**走 v0.33 铺好的那套机制 ⇒ ⇒ 这是那套机制的**第一次
//       实战** ⇒ ⇒ 「机制只是写在 prose 里」这条风险**被证伪**）
//
// ⚠️ **断言值是写死的字面量**，不从被测实现反推（`3` / `2` / `1` 写死，不读常量）。
// ⚠️ **用例必须能变红**（不许恒真断言）⇒ 每条断言都在钉一个具体行为。
//
// # 为什么 B-5 ~ B-7 走 app 层而不是纯函数
//
// 「选用时提示」与「内核接受后升级」都是 **app 层**的行为（`front/app.js`）
// ⇒ ⇒ 这里**复用既有手法**（`front/tests/memory-map-ordering.test.mjs` 那一套：
// DOM 替身 + 关掉 `app.js` 自启动 + 走 `app.handleCommand` 的**真实接线**）
// ⇒ ⇒ **不另造一套 harness**。
//
// ⚠️ **判别力（对照必须成组）**：B-5 旁边必须再钉一条「**当前**版本的板
// 选用时**不**提示」⇒ 否则「一律都提示」也会让 B-5 假绿。
//
// ⚠️ **零依赖、零构建** ⇒ **不引** `fake-indexeddb` ⇒ 存储层用**内存
// backend**（`createMemoryBackend()`）测 ⇒ 那是**真代码路径**（结构化克隆
// 语义也在内）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
const dom = installDom(indexHtml);

// ⚠️ **关掉 `app.js` 的自启动**（与 `memory-map-ordering.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意两个
// 用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const message = await importFront("lib/message.js");
const boardstore = await importFront("services/m23_boardstore.js");
const mm = await importFront("views/components/memory-map.js");

// 视图组件的渲染监听器（`app.js` 的 bootstrap 会挂它；这里自启动关掉了）。
await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);

/** 页脚那行提示（`app.js` 的 `setNote` 写的地方）。 */
const noteText = () => String(dom.document.getElementById("kernel-note")?.textContent ?? "");

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/**
 * 一块板的**描述**（字面量）。
 *
 * ⚠️ 形状与契约层的 `PlatformDescription` 对齐（`schema_version` + 元数据 +
 * 段列表）⇒ ⇒ `draftFromDescription` 读得出来 ⇒ 「选用」那条路走得通。
 */
const BOARD_DESC = {
    schema_version: 1,
    id: "rules-board",
    name: "规则板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
    ],
};

/**
 * 一条**老条目**：**没有** `rules_version`（⇒ 隐含版本 1）。
 *
 * ⚠️ 这是 STAGE-15 那批落库的条目形状（**恰为两个字段**）⇒ ⇒ 它就是
 * 「规则集收紧之前」的存量。
 */
const OLD_ENTRY = { id: "rules-board", description: BOARD_DESC };

/**
 * 一条**上一批落库的条目**：`rules_version = 2`。
 *
 * ⚠️ **这就是本批的「存量板」**（STAGE-19 · 裁决 F）：规则集**收紧**了
 * （2 → 3）⇒ ⇒ 库里那些**按 2 校验过**的板就是存量 ⇒ ⇒ 它们**必须**走
 * v0.33 铺好的机制（**明确提示** ⇒ 内核**接受** ⇒ **升级**）⇒ ⇒ 这条机制
 * 在本批**第一次实战**。
 *
 * ⚠️ 与 `OLD_ENTRY`（**没有** `rules_version` ⇒ 隐含 1）是**两件事** ⇒ ⇒
 * 两条用例都要在（一条盯「更老的存量」，一条盯「上一批的存量」）。
 */
const V33_ENTRY = { id: "rules-board", description: BOARD_DESC, rules_version: 2 };

/**
 * 一块**带外设器件**的板（字面量）。
 *
 * ⚠️ **实测**：草稿 → 描述的往返**不是**逐字节相同的（器件会被补上
 * `irq_line: 0`、可选的 `max_region_bytes` 会掉）⇒ ⇒ 「升级」**不能**按
 * 「描述逐字段相同」来判（那样就**永远升级不了** ⇒ 静默失效）⇒ ⇒ 按
 * **板 id** 判（板档案的主键就是 `id`）。
 */
const RICH_DESC = {
    schema_version: 1,
    id: "rich-board",
    name: "富板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    max_region_bytes: 4194304,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        {
            kind: "peripheral",
            base: 268435456,
            device: { device: "uart16550", id: "uart0", baud_rate: 115200 },
        },
    ],
};

// ---------------------------------------------------------------------------
// app 层的现场
// ---------------------------------------------------------------------------

/**
 * 装一套干净的现场。
 *
 * ⚠️ **视图状态是模块级单例** ⇒ 每个用例都重来一遍。
 *
 * @param {string} [hash]
 */
function setup(hash = "#/memory-map") {
    m03.reset();
    m01.assembleWorkspace("product");
    m01.route(hash);
}

/**
 * 一个**假内核**（只做「接受 / 拒绝」这一件事）。
 *
 * ⚠️ **判别力**：内核**接受**与内核**拒绝**必须成对出现 ⇒ 只测「接受」的话，
 * 「永远当成接受」也会假绿。
 *
 * @param {{accept?: boolean, error?: Error|null}} [opts]
 * @returns {{sent: object[], subscribe: Function, dispatchCommand: Function}}
 */
function fakeKernel(opts = {}) {
    const accept = opts.accept !== false;
    const error = opts.error ?? null;
    /** @type {object[]} 发出去的命令 */
    const sent = [];
    return {
        sent,
        subscribe() {
            return () => {};
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            if (!accept) throw error ?? new Error("E_PLT-15: 内核拒绝了这份描述");
            return { value: {} };
        },
    };
}

/**
 * 把一条档案放进**视图状态**（模拟「刷新后从库里读回来的档案」）。
 *
 * @param {object} entry
 */
function seedArchive(entry) {
    m03.setState("memory-map", (prev) => ({ ...prev, archive: [entry] }));
}

/**
 * 建一个**真的**板档案存储（内存 backend ⇒ 真代码路径）。
 *
 * @param {object} [entry] — 先落进去的那一条（老条目 ⇒ 没有 `rules_version`）
 * @returns {Promise<{store: object, backend: object}>}
 */
async function makeStore(entry = OLD_ENTRY) {
    const backend = boardstore.createMemoryBackend();
    if (entry) await backend.put(entry);
    return { store: boardstore.createBoardStore(backend), backend };
}

// ---------------------------------------------------------------------------

describe("板档案 · 规则集版本（B-1 ~ B-4）", () => {
    test("B-1 新落库的条目**带** `rules_version`（= 当前版本 3）", () => {
        const entry = boardstore.boardEntry(BOARD_DESC);
        assert.ok(entry, "boardEntry 应当产出一条");
        assert.deepEqual(
            Object.keys(entry).sort(),
            ["description", "id", "registers", "rules_version"],
            "条目字段集合必须是 { id, description, rules_version, registers } 四个",
        );
        assert.equal(entry.rules_version, 3, "规则版本必须是当前版本 3");
        assert.equal(entry.id, "rules-board");
        assert.deepEqual(entry.description, BOARD_DESC, "描述一个字段都不许改");
        // ⚠️ **不存时间戳、不存 `name`**（既有纪律一字不变）。
        assert.equal("name" in entry, false, "`name` 在描述里 ⇒ 条目不另存");
        assert.equal("createdAt" in entry, false);
        assert.equal("updatedAt" in entry, false);
    });

    test("B-1 落库往返 ⇒ 读回来的条目**仍带** `rules_version`", async () => {
        const { store } = await makeStore(null);
        await store.put(boardstore.boardEntry(BOARD_DESC));
        const rows = await store.list();
        assert.equal(rows.length, 1);
        assert.equal(rows[0].rules_version, 3, "读回来的条目必须带着规则版本");
    });

    test("B-2 老条目（**没有** `rules_version`）⇒ 报 `{ok:false, reason:\"rules\"}`", () => {
        const result = boardstore.checkBoardRules(OLD_ENTRY);
        assert.equal(result.ok, false);
        assert.equal(result.reason, "rules");
        assert.equal(result.expected, 3, "期望的是当前规则版本 3");
        assert.equal(result.actual, 1, "老条目隐含的规则版本是 1");
        // ⚠️ **隐含值 1** = 「规则集收紧**之前**的那一版」（裁决 G）。
        assert.equal(boardstore.boardRulesVersion(OLD_ENTRY), 1);
    });

    test("B-2 `rules_version` **不是数字** ⇒ 也隐含 1（不容忍半截数据）", () => {
        for (const bad of ["2", null, undefined, {}, NaN]) {
            const entry = { ...OLD_ENTRY, rules_version: bad };
            assert.equal(boardstore.boardRulesVersion(entry), 1, `rules_version=${String(bad)}`);
            const result = boardstore.checkBoardRules(entry);
            assert.equal(result.ok, false);
            assert.equal(result.actual, 1);
        }
    });

    test("B-3 当前版本的条目 ⇒ `checkBoardRules` ⇒ ok", () => {
        const entry = boardstore.boardEntry(BOARD_DESC);
        const result = boardstore.checkBoardRules(entry);
        assert.equal(result.ok, true);
        assert.equal("reason" in result, false, "通过的结果里不该带 reason");
        assert.equal(boardstore.boardRulesVersion(entry), 3);
    });

    test("B-4 `checkBoardVersion` 对 `schema_version != 1` 仍报 `reason:\"version\"`", () => {
        // ⚠️ **形状**不对 ⇒ `version`。
        const shape = boardstore.checkBoardVersion({ ...BOARD_DESC, schema_version: 2 });
        assert.equal(shape.ok, false);
        assert.equal(shape.reason, "version");
        assert.equal(shape.expected, 1);
        assert.equal(shape.actual, 2);
        // ⚠️ **规则**已收紧 ⇒ `rules` ⇒ ⇒ **两个 reason 不共用**（裁决 H）。
        const rules = boardstore.checkBoardRules(OLD_ENTRY);
        assert.equal(rules.ok, false);
        assert.equal(rules.reason, "rules");
        assert.notEqual(shape.reason, rules.reason, "「形状不对」与「规则已收紧」不许共用一个码");
    });

    test("B-4 两件事**互不干扰**：形状对但规则旧 ⇒ 只报 `rules`", () => {
        const entry = { ...OLD_ENTRY, description: { ...BOARD_DESC, schema_version: 1 } };
        assert.equal(boardstore.checkBoardVersion(entry.description).ok, true);
        const rules = boardstore.checkBoardRules(entry);
        assert.equal(rules.ok, false);
        assert.equal(rules.reason, "rules");
    });
});
