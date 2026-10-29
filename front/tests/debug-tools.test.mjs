// 调试器增强批的前端门禁（契约 v9 · STAGE-42）—— **C-1 ~ C-13**。
//
// ⚠️ **本套件盯住验收 C-1 ~ C-13，每一条都有对应用例**（一条不落）⇒ ⇒
//   规格 §6.3 的前端组**全覆盖**。
//
// ⚠️ **判据不许是空的**（规格 §6 的总纲）⇒ ⇒ 每条用例都先钉住「**被观测的
//   对象存在**」（输入框在、按钮在、候选**个数**、转储**行数与字节数**）⇒ ⇒
//   「对象整个没了」时不会「零断言假绿」。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器** + `closest()`。
//
// ⚠️ **状态写入必须在订阅回调里**（本仓踩过的坑）⇒ ⇒ 「内核回了答复」与
//   「界面显示答复」之间**隔着一次 re-render** ⇒ ⇒ 用例走的是**真的**
//   `wireDebug` 订阅 ⇒ ⇒ 事件是从订阅回调进去的。
//
// ⚠️ **内核原文不许被改写**（C-13）⇒ ⇒ 断言用的是**逐字相等**，不是
//   「包含」⇒ ⇒ 前端一旦把它换成自己的文案，这条立刻红。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `addr-input.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意
// 两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const msg = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();
// ⚠️ 面板是 M-01 在挂载时造出来的 ⇒ ⇒ 不挂工作台就**没有**视图正文。
await m01.mountWorkbench();
await new Promise((r) => setTimeout(r, 0));

// ---------------------------------------------------------------------------
// 替身与工具
// ---------------------------------------------------------------------------

/**
 * 一个**够用**的命令路由器替身：记下派发的载荷 + 收住 topic 订阅。
 *
 * ⚠️ **`subscribe` 必须真的收住回调** ⇒ ⇒ 用例才能把「内核的答复」从
 *   **订阅回调**那条路喂进去 ⇒ ⇒ 这才验得到「状态写在订阅回调里」。
 *
 * @returns {{sent: object[], handlers: Map<string, Set<Function>>, subscribe: Function, dispatchCommand: Function, emit: Function}}
 */
function fakeRouter() {
    /** @type {object[]} */
    const sent = [];
    /** @type {Map<string, Set<Function>>} */
    const handlers = new Map();
    return {
        sent,
        handlers,
        subscribe(topic, handler) {
            let set = handlers.get(topic);
            if (!set) {
                set = new Set();
                handlers.set(topic, set);
            }
            set.add(handler);
            return () => set.delete(handler);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            return { value: {} };
        },
        /** 模拟内核发来一个事件（**走订阅回调那条路**）。 */
        emit(topic, value) {
            for (const h of [...(handlers.get(topic) ?? [])]) {
                h({ variant: "Event", value, topic, seq: 0, reqId: null });
            }
        },
    };
}

/**
 * 把视图层的 `riscv:command` 信号接到 **app 层**（生产接线）。
 *
 * @param {object} router
 * @returns {() => void} 解除监听
 */
function forwardCommands(router) {
    const handler = (ev) => {
        void app.handleCommand(ev.detail, { commandRouter: router });
    };
    dom.window.addEventListener("riscv:command", handler);
    return () => dom.window.removeEventListener("riscv:command", handler);
}

/** 路由到某个视图并让它渲染一次。 */
function show(viewId) {
    m01.route(`#/${viewId}`);
    const el = dom.document.querySelector(`[data-view-body="${viewId}"]`);
    assert.ok(el, `视图 ${viewId} 的正文容器没挂上`);
    return el;
}

/**
 * 在某个输入框里「敲字」（**写草稿 + 清旧提示**）。
 *
 * @param {object} body — 视图正文
 * @param {string} inputSelector
 * @param {string} text
 * @returns {object} 输入框元素
 */
function type(body, inputSelector, text) {
    const input = body.querySelector(inputSelector);
    assert.ok(input, `输入框不存在：${inputSelector}`);
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    return input;
}

/** 派发出去的那些命令里，某一类的那些。 */
function sentOf(router, key) {
    return router.sent.filter((p) => p && Object.prototype.hasOwnProperty.call(p, key));
}

// ---------------------------------------------------------------------------
// C-1：契约镜像的版本 + 两个上限 == 跨层夹具
// ---------------------------------------------------------------------------

describe("契约镜像的版本与两个上限（视图层 · C-1）", () => {
    test("C-1 · `SCHEMA_VERSION` = 9，两个上限**逐字**等于跨层夹具", () => {
        // ⚠️ **夹具是「两个真相结构上不可能」的唯一保障**（裁决 K）⇒ ⇒
        //   这里读**那一个文件** ⇒ ⇒ 前端镜像与 Rust 常量任一处改动 ⇒ ⇒
        //   这条立刻红。
        const fixture = JSON.parse(readFileSync(
            resolve(repoRoot, "crates/contract/fixtures/debug-limits.json"),
            "utf8",
        ));
        assert.equal(msg.SCHEMA_VERSION, 9, "前端镜像的信封版本必须是 9（契约 v9）");
        assert.equal(
            msg.DUMP_MAX_BYTES,
            fixture.dump_max_bytes,
            "前端镜像的转储上限必须**逐字**等于跨层夹具（两个真相结构上不可能）",
        );
        assert.equal(
            msg.SYMBOL_CANDIDATE_LIMIT,
            fixture.symbol_candidate_limit,
            "前端镜像的候选上限必须**逐字**等于跨层夹具（两个真相结构上不可能）",
        );
        // ⚠️ **夹具本身的值也钉住** ⇒ ⇒ 有人同时改了两侧 ⇒ ⇒ 这条红。
        assert.equal(fixture.dump_max_bytes, 4096, "夹具里的转储上限必须是 4096");
        assert.equal(fixture.symbol_candidate_limit, 16, "夹具里的候选上限必须是 16");
    });

    test("C-1 · 界面上的「≤ 4096」**用的是镜像常量**（不是抄的字面量）", () => {
        const fixture = JSON.parse(readFileSync(
            resolve(repoRoot, "crates/contract/fixtures/debug-limits.json"),
            "utf8",
        ));
        const html = m02.renderMemoryDumpRow({});
        assert.ok(
            html.includes(`≤ ${fixture.dump_max_bytes}`),
            "转储输入框的上限必须**显示夹具里的那个数**（镜像常量 ⇒ 不是抄的）",
        );
        // ⚠️ **视图层必须从契约镜像 import 那个常量** ⇒ ⇒ 抄一份到视图层
        //   就会红。
        const src = readFileSync(resolve(frontDir, "views/m02_viewcomponents.js"), "utf8");
        assert.ok(
            /import\s*\{[^}]*DUMP_MAX_BYTES[^}]*\}\s*from\s*"\.\.\/lib\/message\.js"/.test(src),
            "视图层必须**从契约镜像**导入 `DUMP_MAX_BYTES`（不新造一份）",
        );
    });

    test("C-1 · 另外三个版本号**一个都不升**（规格 §2.8）", () => {
        assert.equal(msg.PLATFORM_SCHEMA_VERSION, 1, "描述格式版本必须仍是 1");
        assert.equal(msg.PLATFORM_RULES_VERSION, 3, "规则集版本必须仍是 3");
    });
});

// ---------------------------------------------------------------------------
// C-2：四个新构造器的线上形状
// ---------------------------------------------------------------------------

describe("四个新构造器的线上形状（视图层 · C-2）", () => {
    test("C-2 · 两条命令的载荷键名与 Rust 侧逐字一致", () => {
        assert.deepEqual(
            msg.Command.resolveSymbol("main"),
            { ResolveSymbol: { name: "main" } },
            "`ResolveSymbol` 的线上形状必须是 `{name}`（键名逐字对齐 Rust）",
        );
        assert.deepEqual(
            msg.Command.dumpMemory(0x1000, 16),
            { DumpMemory: { addr: 0x1000, len: 16 } },
            "`DumpMemory` 的线上形状必须是 `{addr, len}`（键名逐字对齐 Rust）",
        );
    });

    test("C-2 · 两条事件的载荷键名与 Rust 侧逐字一致", () => {
        const symbol = { name: "main", addr: 0x1000, size: 4 };
        assert.deepEqual(
            msg.Event.symbolResolved("main", symbol, [symbol], 3),
            {
                SymbolResolved: {
                    name: "main",
                    symbol,
                    candidates: [symbol],
                    total: 3,
                },
            },
            "`SymbolResolved` 的线上形状必须是 `{name, symbol, candidates, total}`",
        );
        assert.deepEqual(
            msg.Event.memoryDump(0x1000, [104, 105]),
            { MemoryDump: { addr: 0x1000, bytes: [104, 105] } },
            "`MemoryDump` 的线上形状必须是 `{addr, bytes}`（字节是 JSON 数组）",
        );
    });

    test("C-2 · 两个 topic 的拼写与规格逐字一致", () => {
        assert.equal(msg.EVENT_TOPIC.SymbolResolved, "debug.symbol");
        assert.equal(msg.EVENT_TOPIC.MemoryDump, "debug.dump");
    });

    test("C-2 · 字节被钳到 0..255（线上是**无符号**字节值）", () => {
        assert.deepEqual(
            msg.Event.memoryDump(0, [256, -1, 300]).MemoryDump.bytes,
            [0, 255, 44],
            "字节必须钳到 0..255（与 `Console` 事件同一个口径）",
        );
    });
});

// ---------------------------------------------------------------------------
// C-3 ~ C-6：符号查询
// ---------------------------------------------------------------------------

describe("符号查询（视图层 · C-3 ~ C-6）", () => {
    let router;
    let stop;

    beforeEach(() => {
        m03.reset();
        router = fakeRouter();
        app.wireDebug(router);
        stop = forwardCommands(router);
    });

    test("C-3 · 符号框填 `main` + 点「查找」⇒ ⇒ **发出一条** `ResolveSymbol`", () => {
        const body = show("debug");
        type(body, "[data-bp-symbol-input]", "main");
        const buttons = body.querySelectorAll("[data-action=\"lookup-symbol\"]");
        assert.equal(buttons.length, 1, "必须**有**一个「查找」按钮");
        buttons[0].click();
        stop();

        const sent = sentOf(router, "ResolveSymbol");
        assert.equal(sent.length, 1, "点一次「查找」必须发**一条**命令（不多发、不少发）");
        assert.deepEqual(sent[0].ResolveSymbol, { name: "main" }, "名字必须**逐字**是输入框里的那个");
        assert.equal(
            sentOf(router, "SetBreakpoint").length,
            0,
            "查符号**不许**顺手下断点（下断点是点候选那一步的事）",
        );
    });

    test("C-3 · 回车 = 「查找」（一次输入 = 一次查询）", () => {
        const body = show("debug");
        const input = type(body, "[data-bp-symbol-input]", "main");
        const ev = new Event("keydown", { bubbles: true });
        ev.key = "Enter";
        input.dispatchEvent(ev);
        stop();

        const sent = sentOf(router, "ResolveSymbol");
        assert.equal(sent.length, 1, "回车必须**发出一条**查询命令");
        assert.deepEqual(sent[0].ResolveSymbol, { name: "main" });
    });

    test("C-4 · 收到 `SymbolResolved` ⇒ ⇒ 候选区渲染出候选（断言**个数**）", () => {
        const body = show("debug");
        assert.ok(
            body.querySelector("[data-bp-symbol-candidates]"),
            "必须**有**候选容器（data-bp-symbol-candidates）",
        );
        router.emit("debug.symbol", {
            name: "ma",
            symbol: { name: "main", addr: 0x80001234, size: 4 },
            candidates: [
                { name: "main_loop", addr: 0x80001240, size: 4 },
                { name: "malloc", addr: 0x80001250, size: 8 },
            ],
            total: 17,
        });

        const after = show("debug");
        const box = after.querySelector("[data-bp-symbol-candidates]");
        assert.ok(box, "必须**有**候选容器");
        // ⚠️ **不许用后代组合器** ⇒ ⇒ 按容器作用域查。
        const candidates = box.querySelectorAll(".symbol-candidate");
        assert.equal(candidates.length, 3, "候选个数必须是 3（精确命中 1 + 前缀候选 2）");
        // ⚠️ **精确命中排第一**（规格 §5.1）⇒ ⇒ 逐字断言它的形状。
        const first = candidates[0];
        assert.equal(first.getAttribute("data-action"), "set-symbol-breakpoint");
        assert.equal(first.getAttribute("data-addr"), "0x80001234");
        assert.equal(first.getAttribute("data-symbol"), "main");
        assert.equal(first.textContent, "main @ 0x80001234");
        assert.equal(candidates[1].getAttribute("data-addr"), "0x80001240");
        assert.equal(candidates[2].getAttribute("data-symbol"), "malloc");
    });

    test("C-5 · 候选为空且无精确命中 ⇒ ⇒ 显示「没有这个符号」", () => {
        show("debug");
        router.emit("debug.symbol", {
            name: "zzz",
            symbol: null,
            candidates: [],
            total: 17,
        });

        const after = show("debug");
        const box = after.querySelector("[data-bp-symbol-candidates]");
        assert.ok(box, "必须**有**候选容器");
        assert.equal(
            box.querySelectorAll(".symbol-candidate").length,
            0,
            "没有命中时**不许**渲染出候选按钮",
        );        assert.ok(
            box.textContent.includes("没有这个符号"),
            "必须显示「没有这个符号」",
        );
    });

    test("C-6 · `total == 0` ⇒ ⇒ 显示「没有符号表」那一行（**另一条文案**）", () => {
        show("debug");
        router.emit("debug.symbol", { name: "zzz", symbol: null, candidates: [], total: 0 });
        const noTable = show("debug").querySelector("[data-bp-symbol-candidates]").textContent;

        m03.reset();
        router.emit("debug.symbol", { name: "zzz", symbol: null, candidates: [], total: 17 });
        const hasTable = show("debug").querySelector("[data-bp-symbol-candidates]").textContent;

        assert.ok(noTable.includes("没有这个符号"), "两态都要说「没有这个符号」");
        assert.ok(
            noTable.includes("没有符号表"),
            "`total == 0` 必须显示「没有符号表」（镜像可能被 strip 了）",
        );
        assert.ok(
            hasTable.includes("共有 17 个符号"),
            "`total > 0` 必须显示「共有 N 个符号」",
        );
        assert.notEqual(
            noTable,
            hasTable,
            "两态**必须能区分**（镜像没符号 ≠ 名字写错了）",
        );
    });
});

// ---------------------------------------------------------------------------
// C-7 ~ C-10：下断点（条件 + 符号）
// ---------------------------------------------------------------------------

describe("下断点：条件与符号（视图层 · C-7 ~ C-10）", () => {
    let router;
    let stop;

    beforeEach(() => {
        m03.reset();
        router = fakeRouter();
        app.wireDebug(router);
        stop = forwardCommands(router);
    });

    /** 把符号候选喂进去（一次**内核答复**）。 */
    function seedCandidates() {
        router.emit("debug.symbol", {
            name: "main",
            symbol: { name: "main", addr: 0x80001234, size: 4 },
            candidates: [],
            total: 17,
        });
        return show("debug");
    }

    test("C-7 · 点候选 ⇒ ⇒ 发出 `SetBreakpoint`，`addr` 与 `source_hint` 逐字一致", () => {
        const body = seedCandidates();
        const candidate = body.querySelector("[data-bp-symbol-candidates]").querySelectorAll(".symbol-candidate");
        assert.equal(candidate.length, 1, "必须**有**一个候选按钮");
        candidate[0].click();
        stop();

        const sent = sentOf(router, "SetBreakpoint");
        assert.equal(sent.length, 1, "点一次候选必须发**一条** `SetBreakpoint`");
        assert.equal(sent[0].SetBreakpoint.addr, 0x80001234, "地址必须**逐字**是候选的地址");
        assert.equal(sent[0].SetBreakpoint.source_hint, "main", "`source_hint` 必须是候选的符号名");
        assert.equal(sent[0].SetBreakpoint.id, 0x80001234, "断点 id 仍然是**地址**（既有语义不变）");
    });

    test("C-8 · 条件框非空 ⇒ ⇒ `SetBreakpoint.condition` **逐字**等于输入框内容", () => {
        const body = seedCandidates();
        type(body, "[data-bp-cond-input]", "x1 == 5");
        const candidate = body.querySelector("[data-bp-symbol-candidates]").querySelectorAll(".symbol-candidate");
        candidate[0].click();
        stop();

        const sent = sentOf(router, "SetBreakpoint");
        assert.equal(sent.length, 1);
        assert.equal(sent[0].SetBreakpoint.condition, "x1 == 5", "条件必须**逐字**是输入框里的那个");
    });

    test("C-9 · 条件框为空 ⇒ ⇒ `condition` 是 **`null`**（不是空串）", () => {
        const body = show("debug");
        type(body, "[data-bp-addr-input]", "0x2000");
        body.querySelector("[data-addr-input=\"breakpoint\"]")
            .querySelector("[data-action=\"set-breakpoint\"]")
            .click();
        stop();

        const sent = sentOf(router, "SetBreakpoint");
        assert.equal(sent.length, 1, "地址入口必须发**一条** `SetBreakpoint`");
        assert.equal(sent[0].SetBreakpoint.condition, null, "无条件时 `condition` 必须是 `null`");
        assert.notEqual(sent[0].SetBreakpoint.condition, "", "**不许**是空串");
    });

    test("C-8 · 条件框对**地址断点**也通用（裁决 O）", () => {
        const body = show("debug");
        type(body, "[data-bp-addr-input]", "0x2000");
        type(body, "[data-bp-cond-input]", "a0 != 0");
        body.querySelector("[data-addr-input=\"breakpoint\"]")
            .querySelector("[data-action=\"set-breakpoint\"]")
            .click();
        stop();

        const sent = sentOf(router, "SetBreakpoint");
        assert.equal(sent.length, 1);
        assert.equal(sent[0].SetBreakpoint.condition, "a0 != 0", "地址断点也必须带上条件");
    });

    test("C-10 · 断点列表**显示**符号名", () => {
        const body = seedCandidates();
        body.querySelector("[data-bp-symbol-candidates]").querySelectorAll(".symbol-candidate")[0].click();
        stop();

        const after = show("debug");
        const rows = after.querySelectorAll("[data-bp-row]");
        assert.equal(rows.length, 1, "断点列表必须**有**一行（对象存在 ⇒ 不是空断言）");
        assert.ok(
            rows[0].textContent.includes("main"),
            "断点那一行必须**显示符号名**",
        );
        // ⚠️ **既有那行「断点 id = 地址」保持不变** ⇒ ⇒ 符号名是**另起**的
        //   一个 `.note` ⇒ ⇒ 两个 `.note` 并列。
        const notes = rows[0].querySelectorAll(".note");
        assert.equal(notes.length, 2, "断点行里必须**两个** `.note`（符号名 + 既有那句）");
        const texts = notes.map((n) => n.textContent);
        assert.ok(texts.includes("main"), "其中一个 `.note` 必须是符号名");
        assert.ok(texts.includes("断点 id = 地址"), "既有那句「断点 id = 地址」必须**保持不变**");
    });
});

// ---------------------------------------------------------------------------
// C-11 ~ C-12：整段转储
// ---------------------------------------------------------------------------

describe("整段转储（视图层 · C-11 ~ C-12）", () => {
    let router;
    let stop;

    beforeEach(() => {
        m03.reset();
        router = fakeRouter();
        app.wireDebug(router);
        stop = forwardCommands(router);
    });

    test("C-11 · 填地址 + 长度 + 点「转储」⇒ ⇒ 发出 `DumpMemory`，参数逐字一致", () => {
        const body = show("regmem");
        const box = body.querySelector("[data-dump-input=\"memory\"]");
        assert.ok(box, "必须**有**转储输入的容器");
        assert.ok(box.querySelector("[data-dump-addr-input]"), "必须**有**起始地址输入框");
        assert.ok(box.querySelector("[data-dump-len-input]"), "必须**有**长度输入框");
        type(body, "[data-dump-addr-input]", "0x1000");
        type(body, "[data-dump-len-input]", "32");
        box.querySelector("[data-action=\"dump-memory\"]").click();
        stop();

        const sent = sentOf(router, "DumpMemory");
        assert.equal(sent.length, 1, "点一次「转储」必须发**一条**命令");
        assert.deepEqual(
            sent[0].DumpMemory,
            { addr: 0x1000, len: 32 },
            "地址与长度必须**逐字**是界面给的那两个",
        );
    });

    test("C-11 · 长度**超上限也照发**（前端不预校验 ⇒ ⇒ 内核那条路必须可达）", () => {
        const body = show("regmem");
        type(body, "[data-dump-addr-input]", "0x1000");
        type(body, "[data-dump-len-input]", "99999");
        body.querySelector("[data-dump-input=\"memory\"]")
            .querySelector("[data-action=\"dump-memory\"]")
            .click();
        stop();

        const sent = sentOf(router, "DumpMemory");
        assert.equal(sent.length, 1, "超限**也要发**（前端不写第二份校验器）");
        assert.equal(sent[0].DumpMemory.len, 99999, "长度必须**原样**发出去（不静默截断）");
    });

    test("C-12 · 收到 `MemoryDump` ⇒ ⇒ 转储区渲染出**行数与字节数**", () => {
        show("regmem");
        const bytes = Array.from({ length: 40 }, (_, i) => 0x41 + i);
        router.emit("debug.dump", { addr: 0x1000, bytes });

        const after = show("regmem");
        const box = after.querySelector("[data-mem-dump]");
        assert.ok(box, "必须**有**转储结果的容器");
        assert.equal(
            box.getAttribute("data-mem-dump-rows"),
            "3",
            "40 字节 = 16 + 16 + 8 ⇒ ⇒ **三行**",
        );
        assert.equal(
            box.getAttribute("data-mem-dump-bytes"),
            "40",
            "字节数必须是 40（断言具体数字，不是「非空」）",
        );
        const rows = box.querySelectorAll(".dump-row");
        assert.equal(rows.length, 3, "表格里必须**三行**");
        assert.equal(rows[0].getAttribute("data-dump-off"), "00000000");
        assert.equal(rows[1].getAttribute("data-dump-off"), "00000010");
        assert.equal(rows[2].getAttribute("data-dump-off"), "00000020");
        assert.equal(
            rows[0].querySelector(".dump-hex").textContent,
            "41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 50",
            "十六进制列必须是**每字节 2 位小写**、空格分隔、**每行 16 字节**",
        );
        assert.equal(
            rows[0].querySelector(".dump-ascii").textContent,
            "ABCDEFGHIJKLMNOP",
            "ASCII 列必须是可打印字符**原样**",
        );
        assert.equal(
            rows[2].querySelector(".dump-hex").textContent,
            "61 62 63 64 65 66 67 68",
            "最后一行可以不足 16 个字节",
        );
        assert.equal(
            rows[2].querySelector(".dump-ascii").textContent,
            "abcdefgh",
            "最后一行只有 8 个字节",
        );
    });

    test("C-12 · 非可打印字节在 ASCII 列里是 `.`", () => {
        show("regmem");
        router.emit("debug.dump", { addr: 0, bytes: [0x00, 0x1f, 0x20, 0x41, 0x7e, 0x7f, 0xff] });
        const row = show("regmem").querySelector("[data-mem-dump]").querySelector(".dump-row");
        assert.equal(
            row.querySelector(".dump-ascii").textContent,
            ".. A~..",
            "只有 0x20..0x7e 原样，其余一律 `.`",
        );
    });
});

// ---------------------------------------------------------------------------
// C-13：内核原文上屏（不吞、不改写）
// ---------------------------------------------------------------------------

describe("内核原文上屏（视图层 · C-13）", () => {
    let router;
    let stop;

    beforeEach(() => {
        m03.reset();
        router = fakeRouter();
        app.wireDebug(router);
        stop = forwardCommands(router);
    });

    test("C-13 · `E_DBG-03` ⇒ ⇒ 内核原文显示在转储那一位", async () => {
        await app.handleCommand(
            { action: "dump-memory", params: { addr: 0x1000, len: 99999 } },
            { commandRouter: router },
        );
        stop();
        const raw = "转储长度越界：99999（必须是 1 .. 4096）";
        router.emit("error", { code: "E_DBG-03", message: raw });

        const after = show("regmem");
        const slot = after.querySelector("[data-dump-error]");
        assert.ok(slot, "必须**有**转储的错误位");
        assert.equal(
            slot.textContent,
            raw,
            "错误位上必须是**内核原文**（逐字相等 ⇒ ⇒ 不吞、不改写）",
        );
    });

    test("C-13 · `E_DBG-02` ⇒ ⇒ 内核原文显示在条件那一位 + 调试面板那一处", async () => {
        await app.handleCommand(
            { action: "set-breakpoint", params: { addr: 0x2000, condition: "x1 ==" } },
            { commandRouter: router },
        );
        stop();
        const raw = "条件表达式无效：语法错（缺右操作数）";
        router.emit("error", { code: "E_DBG-02", message: raw });

        const after = show("debug");
        const slot = after.querySelector("[data-bp-cond-error]");
        assert.ok(slot, "必须**有**条件的错误位");
        assert.equal(
            slot.textContent,
            raw,
            "条件那一位上必须是**内核原文**（逐字相等）",
        );
        // ⚠️ **调试面板既有的那一处也在**（真机断言会**同时**读这两个位置）。
        const panel = after.querySelector("[data-debug-error]");
        assert.ok(panel, "必须**有**调试面板的错误位");
        assert.ok(
            panel.textContent.includes(raw),
            "调试面板上也要**看得见**内核原文",
        );
    });

    test("C-13 · `E_DBG-02`（符号名为空）⇒ ⇒ 原文落在符号那一位", async () => {
        await app.handleCommand(
            { action: "lookup-symbol", params: { name: "" } },
            { commandRouter: router },
        );
        stop();
        const raw = "符号名不能为空";
        router.emit("error", { code: "E_DBG-02", message: raw });

        const after = show("debug");
        const slot = after.querySelector("[data-bp-symbol-error]");
        assert.ok(slot, "必须**有**符号的错误位");
        assert.equal(slot.textContent, raw, "符号那一位上必须是**内核原文**");
    });
});
