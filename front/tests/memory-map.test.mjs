// v0.29 · Lane C —— 「前端可配置的板级内存图」的门禁（STAGE-14）。
//
// 本套件盯住验收 C-1 ~ C-7：
//
//   C-1 编辑器能**增 / 删** RAM 段
//   C-2 编辑器能**增 / 删外设窗口**并选**器件类型**（4 种）
//   C-3 点提交 ⇒ **真的发出** `SetMemoryMap` 命令，载荷形状与契约**逐字同形**
//   C-4 **内存映射表显示真数据**（不再是 mock）
//   C-5 校验失败（`E_PLT-07`）⇒ **界面上说出来**（不是静默、不是只进日志）
//   C-6 `region_budget_bytes` **可改**且 V-7 约束**显式呈现**
//   C-7 既有前端断言**一条未改**（由 `tools/check-front.sh` 全绿保证）
//
// ⚠️ **C-8（端到端）不在本文件**：它要真 Chrome + 真 WASM 内核 ⇒ 归
// `tools/browser-verify.mjs`。这里**不假造**一条「端到端通过」的断言。
//
// ⚠️ **判据是族 + 码，不靠文案**：`E_PLT-07` 的族是 `PLT` ⇒ 断言同时钉住
// `data-error-code` / `data-error-family` / `data-error-scenario` 三个钩子。

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

// ⚠️ **关掉 `app.js` 的自启动**（与 `console.test.mjs` 同一条理由）：M-03 的
// 视图状态是**模块级单例** ⇒ bootstrap 的异步 reset 可能落在任意两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const message = await importFront("lib/message.js");
const mm = await importFront("views/components/memory-map.js");
const demo = await importFront("dev/demo-state.js");

// 视图组件的渲染监听器（`app.js` 的 bootstrap 会挂它；这里自启动关掉了）。
await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/**
 * 一份「用户配的板」——形状与契约层的 `PlatformDescription` **逐字同形**
 * （serde 内部标签：区域用 `kind`、器件用 `device`）。
 */
const USER_BOARD = {
    schema_version: 1,
    id: "user-board",
    name: "用户板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        {
            kind: "peripheral",
            base: 1073741824,
            device: { device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 },
        },
    ],
};

// ---------------------------------------------------------------------------
// 假内核
// ---------------------------------------------------------------------------

/**
 * 一个**会清空主题集合**的假内核（学真内核的样子）。
 *
 * ⚠️ 真内核的 `LoadProject` 会清掉内核侧的主题集合（新工程 = 新会话，FR-13）
 * ⇒ 装载之后不重订，内核就**不再推** `memory.map` ⇒ 内存映射表**永远**停在
 * 旧图上，而页面上看不出任何异常（与 D-45 / D-46 同一个坑）。
 *
 * ⚠️ **v0.31**：`UnloadProject` 也清主题集合（裁决 C 第 2 步：新会话 ⇒
 * 订阅全部失效）⇒ 卸载之后**必须重订全部流**。
 *
 * @param {{rejectSetMemoryMap?: Error, rejectPatchMemoryMap?: Error}} [options]
 * @returns {object} the router double
 */
function fakeKernel(options = {}) {
    /** @type {Map<string, Set<Function>>} */
    const subs = new Map();
    /** @type {object[]} */
    const sent = [];
    /** 内核侧的主题集合（`LoadProject` / `UnloadProject` 会清空它）。 */
    const topics = new Set();

    return {
        sent,
        topics,
        /** 前端侧（router 侧）的订阅表 —— 「这条流还活着吗」看它。 */
        subs,
        subscribe(topic, handler) {
            if (!subs.has(topic)) subs.set(topic, new Set());
            subs.get(topic).add(handler);
            return () => subs.get(topic).delete(handler);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            if (payload && typeof payload === "object" && "LoadProject" in payload) {
                topics.clear();
            } else if (payload && typeof payload === "object" && "Subscribe" in payload) {
                topics.add(payload.Subscribe.topic);
            } else if (payload && typeof payload === "object" && "SetMemoryMap" in payload) {
                if (options.rejectSetMemoryMap) throw options.rejectSetMemoryMap;
            } else if (payload && typeof payload === "object" && "PatchMemoryMap" in payload) {
                if (options.rejectPatchMemoryMap) throw options.rejectPatchMemoryMap;
            } else if (payload === "UnloadProject") {
                // ⚠️ **unit 变体** ⇒ 线上形态是裸字符串（不是 `{UnloadProject:{}}`）。
                topics.clear();
            }
            return { value: {} };
        },
        /**
         * 前端侧收到一条 `memory.map` 事件。
         *
         * ⚠️ 这一条**不**看内核侧的主题集合：它是「内核推了一条事件」这件事
         * 本身（内核侧订没订由 `emitMemoryMapIfSubscribed` 单独守）。
         *
         * ⚠️ **v0.31**：事件多了 `loaded` 与 `revision` 两个字段（裁决 M）。
         */
        emitMemoryMap(description, pending = false, loaded = false, revision = 0) {
            const handlers = subs.get("memory.map");
            if (!handlers || handlers.size === 0) return false;
            const event = {
                variant: "MemoryMap",
                topic: "memory.map",
                value: { description, pending, loaded, revision },
            };
            for (const fn of handlers) fn(event);
            return true;
        },
        /**
         * 内核侧推一条 `memory.map` 事件 —— **内核侧订阅了**才推（真内核就是
         * 这样：没订阅就没有事件）。
         */
        emitMemoryMapIfSubscribed(description, pending = false) {
            if (!topics.has("memory.map")) return false;
            return this.emitMemoryMap(description, pending);
        },
    };
}

/**
 * 造一套「服务替身」：只盯内存图这一路。
 *
 * @param {object} router
 * @returns {object} the services object
 */
function servicesFor(router, memoryMap = null) {
    return {
        commandRouter: router,
        registerMemory: {
            async loadProject(project) {
                // ⚠️ **真的**发一条 `LoadProject` ⇒ 假内核据此清空主题集合。
                await router.dispatchCommand(message.Command.loadProject(project));
            },
        },
        waveform: { reset() {}, async subscribeProbeStream() {} },
        console: { reset() {}, async subscribeConsoleStream() {} },
        memoryMap,
    };
}

// ---------------------------------------------------------------------------
// 现场
// ---------------------------------------------------------------------------

/**
 * 装一套干净的「板级内存图编辑器」现场。
 *
 * ⚠️ **视图状态是模块级单例** ⇒ 每个用例都重来一遍（否则上一个用例的草稿会
 * 漏进下一个用例）。
 *
 * @param {string} [hash] — 要路由到的视图
 */
function setup(hash = "#/memory-map") {
    m03.reset();
    m01.assembleWorkspace("product");
    m01.route(hash);
}

/**
 * 编辑器当前的草稿。
 *
 * ⚠️ 走 `currentDraft`（渲染与采集**同一个**基底）而不是直接读
 * `state.draft`：用户还没动过编辑器时，草稿只在**渲染**里存在（由当前生效
 * 的图或默认草稿兜底），还没被写进视图状态。
 */
function draft() {
    return mm.currentDraft(m03.getState("memory-map"));
}

// ---------------------------------------------------------------------------

describe("板级内存图 · 编辑器（规格 §3.3）", () => {
    test("C-1 编辑器能**增** RAM 段", () => {
        setup();
        const before = draft().regions.length;
        $("[data-action='add-ram']").dispatchEvent(new Event("click"));
        const regions = draft().regions;
        assert.equal(regions.length, before + 1, "点「+ RAM 段」没有加出新段");
        assert.equal(regions[regions.length - 1].kind, "ram", "新段的类型必须是 ram");
    });

    test("C-1 编辑器能**删** RAM 段", () => {
        setup();
        $("[data-action='add-ram']").dispatchEvent(new Event("click"));
        const before = draft().regions.length;
        const buttons = $$("[data-action='remove-region']");
        const last = buttons[buttons.length - 1];
        last.dispatchEvent(new Event("click"));
        assert.equal(draft().regions.length, before - 1, "点「删除」没有删掉那一段");
    });

    test("C-2 编辑器能**增**外设窗口", () => {
        setup();
        $("[data-action='add-peripheral']").dispatchEvent(new Event("click"));
        const periph = draft().regions.filter((r) => r.kind === "peripheral");
        assert.equal(periph.length, 1, "点「+ 外设窗口」没有加出窗口");
        assert.equal(periph[0].device.device, "gpio", "新窗口的器件类型必须是 gpio");
        assert.ok(periph[0].base !== undefined, "外设窗口必须有基址");
        assert.equal(
            periph[0].size, undefined,
            "外设窗口**没有** size 字段（窗口大小由器件自报，写进去就是第二个真相）",
        );
    });

    test("C-2 编辑器能**删**外设窗口", () => {
        setup();
        $("[data-action='add-peripheral']").dispatchEvent(new Event("click"));
        const index = draft().regions.findIndex((r) => r.kind === "peripheral");
        const button = $$("[data-action='remove-region']").find(
            (b) => Number(b.getAttribute("data-mmap-index")) === index,
        );
        assert.ok(button, "外设窗口那一行没有删除按钮");
        button.dispatchEvent(new Event("click"));
        assert.equal(
            draft().regions.filter((r) => r.kind === "peripheral").length, 0,
            "点「删除」没有删掉那个外设窗口",
        );
    });

    test("C-2 器件类型下拉框正好是契约里的 4 种", () => {
        setup();
        $("[data-action='add-peripheral']").dispatchEvent(new Event("click"));
        const select = $("[data-mm-device-kind]");
        assert.ok(select, "没有器件类型下拉框");
        assert.deepEqual(
            select.querySelectorAll("option").map((o) => o.getAttribute("value")),
            ["gpio", "uart16550", "clint", "plic"],
            "器件类型必须正好是契约 `DeviceSpec` 的 4 种",
        );
    });

    test("C-2 换器件类型 ⇒ 参数集跟着换（该类型的参数补齐）", () => {
        setup();
        $("[data-action='add-peripheral']").dispatchEvent(new Event("click"));
        const select = $("[data-mm-device-kind]");
        select.value = "uart16550";
        select.dispatchEvent(new Event("change"));
        const device = draft().regions.find((r) => r.kind === "peripheral").device;
        assert.equal(device.device, "uart16550");
        assert.equal(device.id, "UART0");
        assert.ok(device.irq_line !== undefined, "UART 少了 irq_line");
        assert.ok(device.baud_rate !== undefined, "UART 少了 baud_rate");
    });

    test("C-3 点提交 ⇒ 发出 `submit-memory-map`，载荷是契约的描述形状", () => {
        setup();
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='submit-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "点提交没有发出命令信号");
        assert.equal(detail.action, "submit-memory-map");
        const description = detail.params.description;
        assert.equal(
            description.schema_version, 1,
            "描述的 `schema_version` 必须是 `PLATFORM_SCHEMA_VERSION` = 1（不是信封的 6）",
        );
        assert.deepEqual(
            Object.keys(description).sort(),
            [
                "auto_selectable", "id", "name", "priority",
                "region_budget_bytes", "regions", "schema_version",
            ],
            "描述的字段必须与契约层的 `PlatformDescription` 逐字同形（不多不少）",
        );
        const region = description.regions[0];
        assert.deepEqual(
            Object.keys(region).sort(), ["base", "kind", "size"],
            "RAM 段的形状必须是 `{kind, base, size}`（serde 内部标签）",
        );
        assert.equal(region.kind, "ram");
        assert.equal(typeof region.base, "number");
        assert.equal(typeof region.size, "number");
    });

    test("C-3 app 层把提交变成真正的 `SetMemoryMap` 命令（载荷逐字同形）", async () => {
        const router = fakeKernel();
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: router },
        );
        const cmd = router.sent.find((c) => c && typeof c === "object" && "SetMemoryMap" in c);
        assert.ok(cmd, "没有发出 `SetMemoryMap` 命令");
        assert.deepEqual(cmd, message.Command.setMemoryMap(USER_BOARD), "命令载荷必须逐字同形");
        assert.deepEqual(cmd.SetMemoryMap.description, USER_BOARD);
    });

    test("C-3 提交的是**整份**描述（不是 patch ⇒ 没有第二个真相）", async () => {
        const router = fakeKernel();
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: router },
        );
        const cmd = router.sent.find((c) => c && typeof c === "object" && "SetMemoryMap" in c);
        // ⚠️ **契约 v7（v0.31 · D-137）**：`SetMemoryMap` **多了一个**
        // `expected_revision` 字段（并发丢更新看得见）⇒ 线上字段是**这两个**
        // ⇒ 断言跟着契约走（不是「前端自己加的字段」）。
        assert.deepEqual(
            Object.keys(cmd.SetMemoryMap).sort(),
            ["description", "expected_revision"],
            "`SetMemoryMap` 带一整份描述 + 期望修订号，不带「当前值 + 补丁」",
        );
        assert.equal(
            cmd.SetMemoryMap.ops, undefined,
            "`SetMemoryMap` **不带**差量操作（差量走 `PatchMemoryMap`）",
        );
        assert.deepEqual(cmd.SetMemoryMap.description, USER_BOARD);
    });

    test("C-5 校验失败（E_PLT-07）⇒ **界面上说出来**（不静默、只进日志不行）", async () => {
        setup();
        const router = fakeKernel({
            rejectSetMemoryMap: message.protocolError(
                "E_PLT-07",
                "段的声明 size 超过 region_budget_bytes",
            ),
        });
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: router },
        );
        const error = m03.getState("memory-map")?.error;
        assert.ok(error, "错误没有进视图状态（⇒ 界面上不会有话说）");
        assert.equal(error.code, "E_PLT-07");
        const box = $("[data-memory-map-error]");
        assert.ok(box, "面板上没有错误块");
        assert.equal(box.getAttribute("data-error-code"), "E_PLT-07");
        assert.equal(box.getAttribute("data-error-family"), "PLT", "族必须是 PLT（不是 CTX）");
        assert.equal(box.getAttribute("data-error-scenario"), "07");
        assert.ok(
            box.textContent.includes("E_PLT-07"),
            "错误块里必须能看到错误码（只放文案不放码不算「说出来」）",
        );
    });

    test("C-5 提交成功 ⇒ 上一次的错误被清掉", async () => {
        setup();
        const failing = fakeKernel({
            rejectSetMemoryMap: message.protocolError("E_PLT-07", "超预算"),
        });
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: failing },
        );
        assert.ok($("[data-memory-map-error]"), "错误块应该在");
        const ok = fakeKernel();
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: ok },
        );
        assert.equal(m03.getState("memory-map")?.error, null, "成功之后错误必须被清掉");
        assert.equal($("[data-memory-map-error]"), null, "错误块应该消失");
    });

    test("C-6 `region_budget_bytes` 可改且**标注单位是字节**", () => {
        setup();
        const input = $("[data-mm-budget]");
        assert.ok(input, "没有 `region_budget_bytes` 输入框");
        const label = input.parentNode.textContent;
        assert.ok(label.includes("字节"), `必须标注单位是字节：${label}`);
        input.value = "16777216";
        input.dispatchEvent(new Event("input"));
        assert.equal(
            draft().region_budget_bytes, "16777216",
            "改预算没有进草稿（⇒ 用户改不动它）",
        );
    });

    test("C-6 V-7 约束**显式呈现**（说出超预算会报什么码）", () => {
        setup();
        const note = $("[data-mmap-budget-note]");
        assert.ok(note, "没有 V-7 的说明");
        assert.ok(note.textContent.includes("V-7"), "必须写出 V-7 这条约束");
        assert.ok(
            note.textContent.includes("E_PLT-07"),
            "必须说出超预算会报什么码（否则用户看不懂为什么被拒）",
        );
    });

    test("「待生效」显式呈现（事件里的 `pending = true`）", () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, true);
        assert.ok($("[data-memory-map-pending]"), "`pending` 时必须显式显示「待生效」");
        assert.ok(
            $("[data-memory-map-pending]").textContent.includes("重新装载工程"),
            "「待生效」必须说清楚怎么才能生效",
        );
    });

    test("「重新装载工程」按钮存在", () => {
        setup();
        assert.ok($("[data-action='reload-project']"), "没有「重新装载工程」按钮");
    });
});

describe("板级内存图 · M-14 内存映射表显示真数据（C-4）", () => {
    test("C-4 内存映射表显示内核推来的**真数据**", () => {
        setup("#/product");
        const router = fakeKernel();
        app.wireMemoryMap(router);
        assert.equal(router.emitMemoryMap(USER_BOARD, false), true, "内核该推这个事件");
        const rows = $$("[data-region-base]");
        assert.equal(rows.length, 2, "内存映射表该有两行");
        assert.equal(rows[0].getAttribute("data-region-base"), "2147483648");
        assert.equal(rows[0].getAttribute("data-region-kind"), "ram");
        assert.equal(rows[0].getAttribute("data-region-size"), "8388608");
        assert.equal(rows[1].getAttribute("data-region-base"), "1073741824");
        assert.equal(rows[1].getAttribute("data-region-kind"), "peripheral");
    });

    test("C-4 外设行**不挂** `data-region-size`（窗口大小由器件自报，没有数据源）", () => {
        setup("#/product");
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false);
        const rows = $$("[data-region-base]");
        const peripheral = rows.find((r) => r.getAttribute("data-region-kind") === "peripheral");
        assert.ok(peripheral, "没有外设行");
        assert.equal(
            peripheral.getAttribute("data-region-size"), null,
            "外设行不许挂 `data-region-size`（那个数是编出来的）",
        );
        assert.ok(
            peripheral.textContent.includes("由器件自报"),
            "size 列必须如实显示「由器件自报」",
        );
    });

    test("C-4 `data-region-base` / `data-region-size` 是**十进制字符串**", () => {
        setup("#/product");
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false);
        for (const row of $$("[data-region-base]")) {
            const base = row.getAttribute("data-region-base");
            assert.match(base, /^[0-9]+$/, `base 必须是十进制字符串：${base}`);
            const size = row.getAttribute("data-region-size");
            if (size !== null) assert.match(size, /^[0-9]+$/, `size 必须是十进制字符串：${size}`);
        }
    });

    test("C-4 demo 状态里**没有**假内存映射了", () => {
        const state = demo.buildDemoStoreState();
        assert.equal(
            state.sim.memoryMap, undefined,
            "demo 里不该再有假内存映射（内存映射表的唯一数据源是内核事件）",
        );
    });

    test("C-4 「没有数据」与「数据是空的」分得开", () => {
        setup("#/product");
        assert.ok(
            $("[data-memory-map-table='no-data']"),
            "没收到事件时必须显示「尚无数据」",
        );
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(
            {
                schema_version: 1, id: "empty", name: "空板", priority: 0,
                auto_selectable: false, region_budget_bytes: 0, regions: [],
            },
            false,
        );
        assert.ok(
            $("[data-memory-map-table='empty']"),
            "收到一条 0 段的图时必须显示「图为空」（不是「尚无数据」）",
        );
    });

    test("「product」与「memory-map」两个视图拿到的是**同一个**缓存对象", () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false);
        const product = m03.getState("product")?.memoryMap;
        const editor = m03.getState("memory-map")?.memoryMap;
        assert.ok(product, "产品视图没有拿到内存图");
        assert.equal(
            product, editor,
            "两个视图必须是**同一个对象引用**（不是两份拷贝 ⇒ 不会各说各话）",
        );
        assert.equal(product.pending, false);
    });
});

describe("板级内存图 · 装载工程后的强制重订", () => {
    test("装载工程后内存图流被**强制重订**（否则表永远停在旧图上）", async () => {
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        await wiring.subscribeMemoryMapStream();
        assert.equal(router.topics.has("memory.map"), true, "先订阅上了");

        await app.loadProjectIntoKernel(servicesFor(router, wiring), { id: "p" });
        assert.equal(
            router.topics.has("memory.map"), true,
            "装载工程后内存图流没有被强制重订 —— 内核不会再推任何图",
        );
        assert.equal(
            router.emitMemoryMapIfSubscribed(USER_BOARD, false), true,
            "装载之后内存图一个事件都收不到",
        );
    });

    test("内存图的订阅发生在 `LoadProject` **之后**", async () => {
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        const services = servicesFor(router, wiring);
        const before = router.sent.length;
        await app.loadProjectIntoKernel(services, { id: "p" });
        const kinds = router.sent.slice(before).map((c) => {
            if (c && typeof c === "object" && "LoadProject" in c) return "LoadProject";
            if (c && typeof c === "object" && "Subscribe" in c) return c.Subscribe.topic;
            return String(c);
        });
        assert.deepEqual(
            kinds,
            ["LoadProject", "memory.map"],
            "内存图的订阅必须**在装载之后** —— 装载前的订阅会被一起清掉",
        );
    });

    test("内存图流重订失败**不牵连**工程装载（报出来，不抛）", async () => {
        const router = fakeKernel();
        const services = servicesFor(router, {
            reset() {},
            async subscribeMemoryMapStream() {
                throw new Error("内核不回");
            },
        });
        const result = await app.loadProjectIntoKernel(services, { id: "p" });
        assert.ok(result.memoryMapError instanceof Error, "失败必须**报出来**（不静默）");
        assert.match(String(result.memoryMapError?.message), /内核不回/);
    });

    test("「重新装载工程」⇒ 再发一次同一个工程（宿主在下一次装载时应用待生效配置）", async () => {
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        const services = servicesFor(router, wiring);
        const project = { id: "p", name: "工程" };
        await app.loadProjectIntoKernel(services, project);
        const count = () => router.sent.filter((c) => c && "LoadProject" in c).length;
        const before = count();
        await app.handleCommand({ action: "reload-project" }, services);
        assert.equal(
            count(), before + 1,
            "「重新装载工程」没有再发一次 `LoadProject` ⇒ 待生效的板永远不生效",
        );
        const last = router.sent.filter((c) => c && "LoadProject" in c).pop();
        assert.deepEqual(last.LoadProject, project, "必须重发**同一个**工程");
    });
});

describe("板级内存图 · 描述形状（契约逐字同形）", () => {
    test("草稿 ⇒ 线上描述：字段不多不少（草稿的内部字段漏不进契约）", () => {
        const description = mm.draftToDescription(mm.defaultDraft());
        assert.deepEqual(
            Object.keys(description).sort(),
            [
                "auto_selectable", "id", "name", "priority",
                "region_budget_bytes", "regions", "schema_version",
            ],
        );
    });

    test("外设窗口的线上形状**没有** `size`", () => {
        const draft = mm.addRegion(mm.defaultDraft(), "peripheral");
        const description = mm.draftToDescription(draft);
        const peripheral = description.regions.find((r) => r.kind === "peripheral");
        assert.ok(peripheral, "没有外设窗口");
        assert.deepEqual(Object.keys(peripheral).sort(), ["base", "device", "kind"]);
        assert.equal(peripheral.size, undefined, "外设窗口**没有** size（补一个就是编数据）");
    });

    test("器件的线上形状是 serde 内部标签 `device`", () => {
        const draft = mm.addRegion(mm.defaultDraft(), "peripheral");
        const description = mm.draftToDescription(draft);
        const peripheral = description.regions.find((r) => r.kind === "peripheral");
        assert.equal(peripheral.device.device, "gpio");
        assert.deepEqual(Object.keys(peripheral.device).sort(), ["device", "id"]);
    });

    test("非法数字**不写进草稿**（前端不猜用户的意图）", () => {
        assert.equal(mm.parseNumber("0x8000"), 32768, "十六进制必须能解析");
        assert.equal(mm.parseNumber(""), null, "空栏是「没看懂」，不是 0");
        assert.equal(mm.parseNumber("abc"), null);
        assert.equal(mm.parseNumber("12"), 12);
    });

    test("错误码按**族 + 码**解析（不靠文案）", () => {
        assert.deepEqual(
            mm.parseErrorCode("E_PLT-07"),
            { code: "E_PLT-07", family: "PLT", scenario: "07" },
        );
    });
});

// ---------------------------------------------------------------------------
// 整合批（v0.31）—— 真内核回传的描述**多一个键**时，差量路径不许退化
// ---------------------------------------------------------------------------

describe("整合批 · 真内核的八字段描述 vs 前端的七字段草稿", () => {
    /**
     * 真内核回传的那份描述 —— serde **恒定写出** `max_region_bytes`
     * （契约层钉死了顶层键集：八个）⇒ **八个顶层键**。
     *
     * ⚠️ 这一份**不是**从被测实现反推的：它是「契约层序列化出来的形状」
     * 的手写副本（键名与值都写死）。
     */
    const KERNEL_SHAPED = {
        schema_version: 1,
        id: "custom",
        name: "自定义板",
        priority: 100,
        auto_selectable: false,
        region_budget_bytes: 8388608,
        max_region_bytes: null,
        regions: [{ kind: "ram", base: 2147483648, size: 8388608 }],
    };

    test("前端提交的描述**不带** `max_region_bytes`（裁决 L：只暴露一个预算旋钮）", () => {
        const description = mm.draftToDescription(mm.defaultDraft());
        assert.equal(
            Object.prototype.hasOwnProperty.call(description, "max_region_bytes"),
            false,
            "前端的线上描述**没有**这个键（缺省 ⇒ 内核按 `None` 处理）",
        );
    });

    test("基底多一个 `max_region_bytes: null` ⇒ **不**被判成整份替换", () => {
        // ⚠️ 这一条是**真内核**那一侧的形状 ⇒ 假内核回传的是七字段
        // ⇒ 没有这条用例，归一化被删掉也**没人会发现**。
        const draftDescription = mm.draftToDescription(mm.defaultDraft());
        const plan = mm.planMemoryMapUpdate(KERNEL_SHAPED, draftDescription);
        assert.notEqual(
            plan.mode,
            "replace",
            "缺省与 null 是同一件事 ⇒ 不能被判成整份替换",
        );
        assert.equal(plan.mode, "none", "什么都没改 ⇒ 不发命令");
    });

    test("基底多一个 `max_region_bytes: null` ⇒ 改一段仍然走 **patch**（载荷只有一个 op）", () => {
        // ⚠️ 这是**真内核**那一侧最要紧的一条：改了段 ⇒ 必须是差量，
        // 且载荷里**只有**那一段（验收 C-3 的形状，但基底是八个键）。
        const draft = mm.cloneDraft(mm.defaultDraft());
        draft.regions = [{ kind: "ram", base: "2147483648", size: "4194304" }];
        const draftDescription = mm.draftToDescription(draft);
        const plan = mm.planMemoryMapUpdate(KERNEL_SHAPED, draftDescription);
        assert.equal(plan.mode, "patch", `差量路径必须还在（实际是 ${plan.mode}）`);
        assert.deepEqual(
            plan.ops.map((op) => Object.keys(op)[0]),
            ["UpsertRegion"],
            "载荷里**只有**那一段（不能混进别的 op）",
        );
    });

    test("基底多一个 `max_region_bytes: null` ⇒ **不产生**多余的 `SetBudget`", () => {
        const draftDescription = mm.draftToDescription(mm.defaultDraft());
        const ops = mm.diffMemoryMapOps(KERNEL_SHAPED, draftDescription);
        assert.deepEqual(
            ops.filter((op) => "SetBudget" in op),
            [],
            "预算没改 ⇒ 不该有 `SetBudget`（否则每次提交都白涨一次修订号）",
        );
    });

    test("基底带**真值**的 `max_region_bytes` ⇒ 必须产生 `SetBudget`（不许静默忽略）", () => {
        // ⚠️ 这一条是上面那条的**反面**：让「缺省 ≡ null」与「真值」不等价
        // ⇒ 运行期上界的变化**看得见**（而不是被当成没变）。
        const base = { ...KERNEL_SHAPED, max_region_bytes: 4096 };
        const draftDescription = mm.draftToDescription(mm.defaultDraft());
        const ops = mm.diffMemoryMapOps(base, draftDescription);
        const budgets = ops.filter((op) => "SetBudget" in op);
        assert.equal(budgets.length, 1, "运行期上界变了 ⇒ 必须有一条 `SetBudget`");
        assert.deepEqual(
            budgets[0].SetBudget,
            { region_budget_bytes: 8388608, max_region_bytes: null },
            "`SetBudget` 两个字段都要带上（验收 C-5）",
        );
    });
});

// ---------------------------------------------------------------------------
// 第 13 批（STAGE-15 · v0.30）· Lane C —— 板档案 + 多板并存（D-136 / D-140）
//
// 本套件盯住验收 C-1 ~ C-10：
//
//   C-1 板档案表渲染出**每块板**的名字与 `id`（按 `data-*` 判，不靠文案）
//   C-2 「保存当前板」⇒ 档案里**多一块**；「另存为」⇒ 多一块且旧块仍在
//   C-3 「选用」⇒ 草稿被**载入**（逐字段相同）⇒ **内核状态不变**
//   C-4 编辑已有板时 `id` 输入框**只读**（改 `id` 的入口不存在）
//   C-5 另存为给**已存在**的 `id` ⇒ **拒**并明说
//   C-6 删除一块板 ⇒ 其余板**不受影响**；被删的从档案里消失
//   C-7 草稿**未被内核接受过** ⇒ 「保存 / 另存为」**禁用**并明说「先提交」
//   C-8 提交**成功后** ⇒ 「保存 / 另存为」**解禁**
//   C-9 提交**失败**（`E_PLT-12`）⇒ 错误**如实显示** ⇒ **不落库**
//   C-10 持久化不可用 ⇒ 界面**显式**提示「板档案只存在于本会话」
//
// ⚠️ **板档案服务（M-23）是 Lane B 的产出**（规格 §3.3 冻结了 API 形状）
// ⇒ 本文件用**受保护的动态导入**：它缺席时**只有**下面这批新用例红（并说清
// 原因），既有用例一条不受影响 ⇒ 统筹方把 B 合进来之后重跑即可。
// ---------------------------------------------------------------------------

let boardStoreApi = null;
let boardStoreImportError = null;
try {
    boardStoreApi = await importFront("services/m23_boardstore.js");
} catch (err) {
    boardStoreImportError = err;
}

/**
 * 取板档案服务（缺席 ⇒ **明确失败**，不静默跳过 ⇒ 不会「假绿」）。
 *
 * @returns {object}
 */
function requireBoardStoreApi() {
    if (!boardStoreApi) {
        throw new Error(
            "板档案服务（front/services/m23_boardstore.js）尚未并入"
            + "（Lane B 的产出）⇒ 本批视图层用例无法运行"
            + (boardStoreImportError ? `：${boardStoreImportError.message}` : ""),
        );
    }
    return boardStoreApi;
}

/**
 * 一套**内存后端**的板档案存储（走服务层的冻结 API）。
 *
 * @returns {object}
 */
function memoryBoardStore() {
    const api = requireBoardStoreApi();
    return api.createBoardStore(api.createMemoryBackend());
}

/**
 * 一份草稿（**字面量**，不从被测实现反推）。
 *
 * @param {string} id
 * @param {string} name
 * @returns {object}
 */
function customDraft(id, name) {
    return {
        schema_version: 1,
        id,
        name,
        priority: "100",
        auto_selectable: false,
        region_budget_bytes: "8388608",
        regions: [{ kind: "ram", base: "2147483648", size: "8388608" }],
    };
}

/**
 * 让当前草稿**被内核接受过**（走真实的提交命令）。
 *
 * @param {object} router
 * @param {object} draft
 * @returns {Promise<object>} 实际提交的描述
 */
async function acceptDraft(router, draft) {
    m03.setState("memory-map", (prev) => ({ ...prev, draft }));
    const description = mm.draftToDescription(draft);
    await app.handleCommand(
        { action: "submit-memory-map", params: { description } },
        { commandRouter: router },
    );
    return description;
}

/**
 * 让面板按当前视图状态重渲染一次（重入当前视图也会派发 render 事件）。
 */
function rerender() {
    m01.route("#/memory-map");
}

/**
 * 档案里的 id 列表（**按字母序** ⇒ 不依赖 `mergeBoard` 的插入顺序）。
 *
 * @returns {string[]}
 */
function archiveIds() {
    const state = m03.getState("memory-map") ?? {};
    const archive = Array.isArray(state.archive) ? state.archive : [];
    return archive.map((e) => String(e.id)).sort();
}

describe("板档案与多板并存（STAGE-15 · C-1 ~ C-10）", () => {
    test("C-1 板档案表渲染出每块板的名字与 id（按 data-* 判，不靠文案）", () => {
        setup();
        m03.setState("memory-map", (prev) => ({
            ...prev,
            archive: [
                { id: "board-a", description: { ...USER_BOARD, id: "board-a", name: "板 A" } },
                { id: "board-b", description: { ...USER_BOARD, id: "board-b", name: "板 B" } },
            ],
        }));
        rerender();
        const rows = $$("[data-mmap-board-row]");
        assert.equal(rows.length, 2, "板档案表该有两行");
        assert.deepEqual(
            rows.map((r) => r.getAttribute("data-mmap-board-id")),
            ["board-a", "board-b"],
            "每块的 id 必须挂在 data-mmap-board-id 上",
        );
        assert.deepEqual(
            rows.map((r) => r.getAttribute("data-mmap-board-name")),
            ["板 A", "板 B"],
            "每块的板名必须挂在 data-mmap-board-name 上",
        );
        assert.ok($("[data-mmap-archive]"), "没有板档案表");
    });

    test("C-2 「保存当前板」⇒ 档案里多一块（键 = 草稿的 id）", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand(
            { action: "save-memory-map" },
            { commandRouter: router, boardStore: store },
        );
        assert.deepEqual(archiveIds(), ["saved-board"], "档案里该多出这一块板");
        const stored = await store.list();
        assert.deepEqual(
            stored.map((e) => String(e.id)),
            ["saved-board"],
            "板必须真的落库（不是只改了界面上的数组）",
        );
        rerender();
        assert.equal($$("[data-mmap-board-row]").length, 1, "面板上要看得见这一块板");
    });

    test("C-2 「另存为」给新 id ⇒ 多一块且旧块仍在", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand({ action: "save-memory-map" }, services);
        await app.handleCommand(
            { action: "save-memory-map-as", params: { id: "copy-board", name: "副本" } },
            services,
        );
        assert.deepEqual(
            archiveIds(),
            ["copy-board", "saved-board"],
            "另存为要多一块，旧块仍在",
        );
        const stored = await store.list();
        assert.deepEqual(
            stored.map((e) => String(e.id)).sort(),
            ["copy-board", "saved-board"],
            "两块板都要落库",
        );
    });

    test("C-3 「选用」某块板 ⇒ 草稿被载入（逐字段相同）⇒ 内核状态不变", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand({ action: "save-memory-map" }, services);
        const sentBefore = router.sent.length;
        await app.handleCommand(
            { action: "select-memory-map", params: { id: "saved-board" } },
            services,
        );
        assert.equal(
            router.sent.length, sentBefore,
            "「选用」不许发任何内核命令（内核状态不变）",
        );
        const state = m03.getState("memory-map");
        assert.equal(state.selectedId, "saved-board", "选中的板 id 要跟着变");
        const loaded = mm.draftToDescription(state.draft);
        const archived = state.archive.find((e) => e.id === "saved-board").description;
        assert.ok(
            mm.sameDescription(loaded, archived),
            "载入的草稿必须与档案里那份**逐字段相同**",
        );
    });

    test("C-4 选中已有板 ⇒ id 输入框只读（改 id 的入口不存在）", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand({ action: "save-memory-map" }, services);
        rerender();
        const input = $("[data-mmap-field='id']");
        assert.ok(input, "没有板 id 输入框");
        assert.equal(input.hasAttribute("readonly"), true, "已有板的 id 必须只读");
        // 新板（没选中档案里的一块）⇒ id **可编辑**（否则新板起不了名）。
        m03.setState("memory-map", (prev) => ({ ...prev, selectedId: null }));
        rerender();
        assert.equal(
            $("[data-mmap-field='id']").hasAttribute("readonly"), false,
            "新板的 id 必须可编辑",
        );
    });

    test("C-5 另存为用已存在的 id ⇒ 拒绝并明说", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand({ action: "save-memory-map" }, services);
        await app.handleCommand(
            { action: "save-memory-map-as", params: { id: "saved-board", name: "重名" } },
            services,
        );
        assert.deepEqual(archiveIds(), ["saved-board"], "被拒绝的另存为不许落库");
        const note = dom.document.getElementById("kernel-note").textContent;
        assert.ok(note.includes("已被占用"), `必须明说为什么拒绝：${note}`);
    });

    test("C-6 删除一块板 ⇒ 其余板不受影响，被删的从档案里消失", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        await app.handleCommand({ action: "save-memory-map" }, services);
        await app.handleCommand(
            { action: "save-memory-map-as", params: { id: "copy-board", name: "副本" } },
            services,
        );
        await app.handleCommand(
            { action: "delete-memory-map", params: { id: "copy-board" } },
            services,
        );
        assert.deepEqual(
            archiveIds(),
            ["saved-board"],
            "被删的那块要从档案里消失，其余板不受影响",
        );
        const stored = await store.list();
        assert.deepEqual(
            stored.map((e) => String(e.id)),
            ["saved-board"],
            "持久化层也要删掉（不然刷新后它又回来了）",
        );
        rerender();
        const rows = $$("[data-mmap-board-row]");
        assert.equal(rows.length, 1);
        assert.equal(rows[0].getAttribute("data-mmap-board-id"), "saved-board");
    });

    test("C-7 草稿没被内核接受过 ⇒ 「保存 / 另存为」禁用并明说「先提交」", () => {
        setup();
        const save = $("[data-action='save-memory-map']");
        const saveAs = $("[data-action='save-memory-map-as']");
        assert.ok(save && saveAs, "板档案工具条上的按钮不在");
        assert.equal(save.hasAttribute("disabled"), true, "没被接受过就不许保存");
        assert.equal(saveAs.hasAttribute("disabled"), true, "没被接受过就不许另存为");
        const validated = $("[data-mmap-validated]");
        assert.ok(validated, "没有校验状态");
        assert.equal(validated.getAttribute("data-mmap-validated"), "0");
        assert.ok(validated.textContent.includes("先提交"), "必须明说「先提交」");
    });

    test("C-8 提交成功后 ⇒ 「保存 / 另存为」解禁", async () => {
        setup();
        const router = fakeKernel();
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        assert.equal(
            $("[data-mmap-validated]").getAttribute("data-mmap-validated"), "1",
            "提交成功后校验状态必须变成「已接受」",
        );
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), false,
            "提交成功后「保存当前板」必须解禁",
        );
        assert.equal(
            $("[data-action='save-memory-map-as']").hasAttribute("disabled"), false,
            "提交成功后「另存为板」必须解禁",
        );
    });

    test("C-9 提交失败（E_PLT-12）⇒ 错误如实显示 ⇒ 不落库", async () => {
        setup();
        const router = fakeKernel({
            rejectSetMemoryMap: message.protocolError(
                "E_PLT-12",
                "器件参数 baud_rate 不在合法取值域内",
            ),
        });
        await app.handleCommand(
            { action: "submit-memory-map", params: { description: USER_BOARD } },
            { commandRouter: router },
        );
        const box = $("[data-memory-map-error]");
        assert.ok(box, "面板上没有错误块");
        assert.equal(box.getAttribute("data-error-code"), "E_PLT-12");
        assert.equal(box.getAttribute("data-error-family"), "PLT", "族必须是 PLT");
        assert.equal(box.getAttribute("data-error-scenario"), "12");
        assert.ok(box.textContent.includes("E_PLT-12"), "错误块里必须能看到错误码");
        assert.ok(
            box.textContent.includes("合法取值域"),
            `错误块里必须能看到内核给的消息：${box.textContent}`,
        );
        // ⚠️ **不落库**：失败时 `acceptedDescription` 不动 ⇒ 保存仍然禁用。
        assert.equal(m03.getState("memory-map").acceptedDescription, null);
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), true,
            "提交失败后仍然不许保存",
        );
        assert.deepEqual(archiveIds(), [], "失败的提交不许落库");
    });

    test("落库门槛：草稿没被内核接受过 ⇒ 「保存 / 另存为」都被拒绝（档案不变）", async () => {
        setup();
        const router = fakeKernel();
        const store = memoryBoardStore();
        const services = { commandRouter: router, boardStore: store };
        // ⚠️ **不提交** ⇒ 草稿没被内核接受过 ⇒ 前端**不许**落库
        // （STAGE-15 裁决 B：库里不许有永远装不上的板）。
        m03.setState("memory-map", (prev) => ({
            ...prev,
            draft: customDraft("never-accepted", "没提交过的板"),
        }));
        await app.handleCommand({ action: "save-memory-map" }, services);
        await app.handleCommand(
            { action: "save-memory-map-as", params: { id: "copy-never", name: "副本" } },
            services,
        );
        assert.deepEqual(archiveIds(), [], "没被内核接受过的描述不许落库");
        const stored = await store.list();
        assert.deepEqual(
            stored.map((e) => String(e.id)),
            [],
            "持久化层也不许落库（不只是界面上的数组没变）",
        );
        const note = dom.document.getElementById("kernel-note").textContent;
        assert.ok(note.includes("先提交"), `必须明说「先提交」：${note}`);
    });

    test("C-10 没有持久化存储 ⇒ 界面显式提示「板档案只存在于本会话」", async () => {
        setup();
        const router = fakeKernel();
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        // ⚠️ **不给** `boardStore` ⇒ 没有持久化存储（等价于「浏览器不支持
        // IndexedDB」）⇒ 必须**显式**降级，不静默假装成功。
        await app.handleCommand({ action: "save-memory-map" }, { commandRouter: router });
        const status = $("[data-mmap-archive-status]");
        assert.ok(status, "没有持久化状态");
        assert.equal(
            status.getAttribute("data-mmap-archive-status"), "unsupported",
            "持久化不可用时状态必须是 unsupported",
        );
        assert.ok(
            status.textContent.includes("板档案只存在于本会话"),
            `必须明说「板档案只存在于本会话」：${status.textContent}`,
        );
    });

    test("「已被内核接受」是**推导量**：改了草稿就自动失效", async () => {
        setup();
        const router = fakeKernel();
        const draft = customDraft("saved-board", "保存的板");
        await acceptDraft(router, draft);
        assert.equal($("[data-mmap-validated]").getAttribute("data-mmap-validated"), "1");
        // 改一笔草稿 ⇒ 推导量自动变假（**不靠**手工清标志）。
        m03.setState("memory-map", (prev) => ({
            ...prev,
            draft: { ...draft, name: "改过的板" },
        }));
        rerender();
        assert.equal(
            $("[data-mmap-validated]").getAttribute("data-mmap-validated"), "0",
            "草稿一改，「已被内核接受」就必须自动失效",
        );
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), true,
            "草稿一改就不许保存（落库门槛）",
        );
    });

    test("「另存为」按钮读面板上的两个输入框（不用 prompt）", async () => {
        setup();
        const router = fakeKernel();
        // ⚠️ 按钮在被接受前是 `disabled` ⇒ 真浏览器里点不动 ⇒ 先让它解禁。
        await acceptDraft(router, customDraft("saved-board", "保存的板"));
        const idInput = $("[data-mmap-new-id]");
        const nameInput = $("[data-mmap-new-name]");
        assert.ok(idInput, "没有新板 id 输入框");
        assert.ok(nameInput, "没有新板名输入框");
        idInput.value = "new-board";
        nameInput.value = "新板";
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='save-memory-map-as']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "点「另存为」没有发出命令信号");
        assert.equal(detail.action, "save-memory-map-as");
        assert.equal(detail.params.id, "new-board", "新 id 必须从输入框读");
        assert.equal(detail.params.name, "新板", "新板名必须从输入框读");
    });

    test("「选用」按钮把板 id 带进命令信号（视图层不自己改档案）", () => {
        setup();
        m03.setState("memory-map", (prev) => ({
            ...prev,
            archive: [
                { id: "board-a", description: { ...USER_BOARD, id: "board-a", name: "板 A" } },
            ],
        }));
        rerender();
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='select-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "点「选用」没有发出命令信号");
        assert.equal(detail.action, "select-memory-map");
        assert.equal(detail.params.id, "board-a", "板 id 必须带在命令参数里");
    });

    test("「删除」按钮把板 id 带进命令信号", () => {
        setup();
        m03.setState("memory-map", (prev) => ({
            ...prev,
            archive: [
                { id: "board-a", description: { ...USER_BOARD, id: "board-a", name: "板 A" } },
            ],
        }));
        rerender();
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='delete-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "点「删除」没有发出命令信号");
        assert.equal(detail.action, "delete-memory-map");
        assert.equal(detail.params.id, "board-a", "板 id 必须带在命令参数里");
    });

    test("规范 JSON **按键排序** ⇒ 键序不同的同内容描述判为**相同**", () => {
        // ⚠️ **这条钉的是「排序」本身**（**统筹方在收口阶段补的判别力缺口**）：
        // 把 `canonicalize` 里的 `.sort()` 去掉 ⇒ 两份**键序不同**但内容相同
        // 的描述就**不再相等** ⇒ 「已被内核接受」这个**推导量**会**静默失效**
        // （用户明明没改草稿，却一直被判定成「没被接受」）⇒ 而当时
        // **没有任何用例会红**（实测：删掉 `.sort()` 后全量 389 条仍全绿）。
        //
        // ⇒ 内容**不同**的必须判为**不同**（别把比较写成恒真）。
        const a = { b: 2, a: 1, regions: [{ size: 4, base: 8 }] };
        const b = { a: 1, b: 2, regions: [{ base: 8, size: 4 }] };
        assert.equal(
            mm.canonicalJson(a),
            mm.canonicalJson(b),
            "键序不同的同内容必须规范化成同一串",
        );
        assert.ok(mm.sameDescription(a, b), "sameDescription 必须判为相同");
        assert.equal(mm.sameDescription(a, { ...b, b: 3 }), false, "内容不同必须判为不同");
    });

    test("前端**不显示**器件参数的区间常量（区间由内核校验）", () => {
        setup();
        const note = $("[data-mmap-device-range-note]");
        assert.ok(note, "没有器件参数区间的说明");
        assert.ok(note.textContent.includes("E_PLT-12"), "必须说出越界会报什么码");
        // ⚠️ **不许**出现具体的上界数字（那会是第二份真相）。
        assert.ok(
            !/\b(1024|4096|1000000000000)\b/.test(note.textContent),
            `前端不许显示区间常量：${note.textContent}`,
        );
    });
});

// ---------------------------------------------------------------------------
// 第 14 批（STAGE-16 · v0.31）· Lane C —— 差量 / 修订号 / 卸载（D-137 / D-139）
//
// 本套件盯住验收 C-1 ~ C-14：
//
//   C-1  提交时**带上** `expected_revision`（内核事件带来的那个）
//   C-2  **没收到过** `memory.map` 事件 ⇒ `expected_revision` 为 `null`
//   C-3  改一段的 size ⇒ 提交的是 **`PatchMemoryMap`** ⇒ 只有那一段的 op
//   C-4  删一段 ⇒ 载荷里是 `RemoveRegion` ⇒ base 正确
//   C-5  改预算 ⇒ 载荷里是 `SetBudget` ⇒ 两个预算字段都带上
//   C-6  改 `id`（元数据）⇒ 提交的是**整份替换**（`SetMemoryMap`）
//   C-7  段**重排** ⇒ 差量**按 base 定位** ⇒ 命中同一条
//   C-8  `E_PLT-13`（修订号冲突）⇒ **如实显示**在错误区（不吞掉）
//   C-9  「卸载工程」按钮 ⇒ 发出 `UnloadProject` ⇒ 不发别的契约命令
//   C-10 卸载之后 ⇒ **五条流全部被重订**（D-45 / D-46 同形状）
//   C-11 卸载之后 ⇒ 前端缓存**清空**（寄存器 / 波形 / 反汇编 / console）
//   C-12 「卸载工程」**不**清「当前工程」的记忆 ⇒ 「重新装载工程」仍可用
//   C-13 编辑器**没有**第二个预算输入框（裁决 L）
//   C-14 差量提交成功后 ⇒ 「已被内核接受」的那一份**逐字段等于草稿**
//
// ⚠️ **判据是族 + 码，不靠文案**（与本文件既有口径一致）。
// ---------------------------------------------------------------------------

/**
 * 一块**两段**的板（字面量，不从被测实现反推）—— C-7 的「段重排」要用。
 */
const TWO_REGION_BOARD = {
    schema_version: 1,
    id: "two-region-board",
    name: "两段板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        { kind: "rom", base: 4096, size: 1048576 },
    ],
};

/**
 * 装一套「**卸载路径**」用的服务接线。
 *
 * ⚠️ **用真的 M-10**（不是替身）⇒ 「卸载之后寄存器缓存被清空」这条断言
 * 看的是**真的**缓存 ⇒ 替身自己编的数骗不了人。
 *
 * ⚠️ **前端侧（router 侧）的订阅**由这些**真的**接线建立：
 * M-10 订 `sim.registers` + `sim.state.delta`；`wireWaveform` 订
 * `waveform.history`；`wireConsole` 订 `sim.console`；`wireMemoryMap` 订
 * `memory.map` ⇒ **正好五条**（裁决 F 点名的那五条）。
 *
 * @param {object} router
 * @returns {Promise<object>} the services object
 */
async function unloadServices(router) {
    const m10 = await importFront("services/m10_registermemory.js");
    const registerMemory = await m10.mountRegisterMemory({
        commandRouter: router,
        viewStore: m03,
    });
    return {
        commandRouter: router,
        registerMemory,
        waveform: app.wireWaveform(router),
        console: app.wireConsole(router),
        memoryMap: app.wireMemoryMap(router),
        debug: app.wireDebug(router),
    };
}

/**
 * 提交一次草稿（走 app 层的真实路径）。
 *
 * @param {object} router
 * @param {object} draft — 草稿
 */
async function submitDraft(router, draft) {
    m03.setState("memory-map", (prev) => ({ ...prev, draft }));
    await app.handleCommand(
        { action: "submit-memory-map", params: { description: mm.draftToDescription(draft) } },
        { commandRouter: router },
    );
}

/**
 * 找一条命令（按变体名）。
 *
 * @param {object[]} sent
 * @param {string} variant
 * @returns {object|undefined}
 */
function findCommand(sent, variant) {
    return sent.find((c) => c && typeof c === "object" && variant in c);
}

describe("板级内存图 · 差量与期望修订号（STAGE-16 · C-1 ~ C-8）", () => {
    test("C-1 提交时**带上** `expected_revision`（内核事件带来的那个）", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 7);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "改一段的 size 应当走差量");
        assert.equal(
            cmd.PatchMemoryMap.expected_revision, 7,
            "必须带上内核事件里的修订号（并发丢更新才看得见）",
        );
    });

    test("C-2 **没收到过** `memory.map` 事件 ⇒ `expected_revision` 为 `null`（不检查）", async () => {
        setup();
        const router = fakeKernel();
        await submitDraft(router, mm.draftFromDescription(USER_BOARD));
        const cmd = findCommand(router.sent, "SetMemoryMap");
        assert.ok(cmd, "没有基底 ⇒ 整份替换");
        assert.equal(
            cmd.SetMemoryMap.expected_revision, null,
            "没收到过事件 ⇒ 不检查修订号（传 null）",
        );
    });

    test("C-3 改一段的 size ⇒ 提交的是 `PatchMemoryMap`，载荷里**只有**那一段的 op", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "改一段的 size 不该重发整图");
        assert.equal(
            findCommand(router.sent, "SetMemoryMap"), undefined,
            "不该同时发整份替换（那是两条路 ⇒ 两个真相）",
        );
        assert.equal(cmd.PatchMemoryMap.ops.length, 1, "载荷里**只有**那一段的 op");
        assert.deepEqual(cmd.PatchMemoryMap.ops[0], {
            UpsertRegion: { spec: { kind: "ram", base: 2147483648, size: 4194304 } },
        });
    });

    test("C-4 删一段 ⇒ 载荷里是 `RemoveRegion`，base 正确", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.regions = draft.regions.filter((r) => r.kind === "ram");
        await submitDraft(router, draft);
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "删一段应当走差量");
        assert.deepEqual(cmd.PatchMemoryMap.ops, [
            { RemoveRegion: { base: 1073741824 } },
        ]);
    });

    test("C-5 改预算 ⇒ 载荷里是 `SetBudget`，两个预算字段都带上", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.region_budget_bytes = "16777216";
        await submitDraft(router, draft);
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "改预算应当走差量");
        assert.equal(cmd.PatchMemoryMap.ops.length, 1, "载荷里只有预算那一条 op");
        const op = cmd.PatchMemoryMap.ops[0];
        assert.deepEqual(Object.keys(op), ["SetBudget"], "操作名必须是 `SetBudget`");
        assert.deepEqual(
            Object.keys(op.SetBudget).sort(),
            ["max_region_bytes", "region_budget_bytes"],
            "两个预算字段都要带上（契约层那两个字段）",
        );
        assert.equal(op.SetBudget.region_budget_bytes, 16777216);
    });

    test("C-6 改 `id`（元数据）⇒ 提交的是**整份替换**（`SetMemoryMap`）", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.id = "another-board";
        await submitDraft(router, draft);
        assert.equal(
            findCommand(router.sent, "PatchMemoryMap"), undefined,
            "改了元数据不许走差量（差量不覆盖元数据 ⇒ 裁决 D）",
        );
        const cmd = findCommand(router.sent, "SetMemoryMap");
        assert.ok(cmd, "改了元数据必须走整份替换");
        assert.equal(cmd.SetMemoryMap.description.id, "another-board");
    });

    test("C-7 段**重排** ⇒ 差量**按 base 定位** ⇒ 命中同一条", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(TWO_REGION_BOARD, false, true, 3);
        // ⚠️ **重排**（数组顺序变了）+ 改其中一段的 size。
        const draft = mm.draftFromDescription(TWO_REGION_BOARD);
        draft.regions.reverse();
        draft.regions[0].size = "2097152"; // 改的是 **ROM** 那一段（现在是第 0 段）
        await submitDraft(router, draft);
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "应当走差量");
        assert.equal(
            cmd.PatchMemoryMap.ops.length, 1,
            "重排不该产生额外的 op（按 base 定位 ⇒ 命中同一条）",
        );
        assert.deepEqual(cmd.PatchMemoryMap.ops[0], {
            UpsertRegion: { spec: { kind: "rom", base: 4096, size: 2097152 } },
        });
    });

    test("C-7 纯重排（内容没变）⇒ **不发命令**（差量为空 ⇒ 不白涨修订号）", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(TWO_REGION_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(TWO_REGION_BOARD);
        draft.regions.reverse();
        await submitDraft(router, draft);
        assert.equal(
            router.sent.length, 0,
            "什么都没改（只是顺序变了）⇒ 不该发任何命令",
        );
        const note = dom.document.getElementById("kernel-note").textContent;
        assert.ok(note.includes("没有改动"), `必须明说「没有改动」：${note}`);
    });

    test("C-8 `E_PLT-13`（修订号冲突）⇒ **如实显示**在错误区（不吞掉）", async () => {
        setup();
        const router = fakeKernel({
            rejectPatchMemoryMap: message.protocolError(
                "E_PLT-13",
                "期望的修订号与当前的不一致",
            ),
        });
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 3);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);
        const box = $("[data-memory-map-error]");
        assert.ok(box, "面板上没有错误块（冲突被吞掉了？）");
        assert.equal(box.getAttribute("data-error-code"), "E_PLT-13");
        assert.equal(box.getAttribute("data-error-family"), "PLT", "族必须是 PLT");
        assert.equal(box.getAttribute("data-error-scenario"), "13");
        assert.ok(box.textContent.includes("E_PLT-13"), "错误块里必须能看到错误码");
        assert.ok(
            box.textContent.includes("刷新"),
            `必须说清下一步（刷新后再提交）：${box.textContent}`,
        );
    });

    test("修订号显示在面板上（没收到过事件 ⇒ `0`；收到过 ⇒ 那个数）", () => {
        setup();
        const el = $("[data-mmap-revision]");
        assert.ok(el, "面板上没有修订号元素");
        assert.equal(el.textContent, "0", "没收到过事件时必须显示 0（不许是 undefined / 空）");
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 9);
        assert.equal($("[data-mmap-revision]").textContent, "9");
    });
});

describe("板级内存图 · 卸载工程（STAGE-16 · C-9 ~ C-13）", () => {
    test("C-9 「卸载工程」按钮 ⇒ 发出 `UnloadProject` ⇒ **不发别的契约命令**", async () => {
        setup();
        const router = fakeKernel();
        const services = await unloadServices(router);
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='unload-project']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "点「卸载工程」没有发出命令信号");
        assert.equal(detail.action, "unload-project");
        const before = router.sent.length;
        await app.handleCommand(detail, services);
        const sent = router.sent.slice(before);
        // ⚠️ **unit 变体** ⇒ 线上形态是**裸字符串** `"UnloadProject"`。
        assert.ok(
            sent.includes("UnloadProject"),
            `必须发出 UnloadProject，实际发出：${JSON.stringify(sent)}`,
        );
        const others = sent.filter(
            (c) => !(c && typeof c === "object" && "Subscribe" in c) && c !== "UnloadProject",
        );
        assert.deepEqual(
            others, [],
            `卸载不许发别的契约命令（重订订阅除外）：${JSON.stringify(others)}`,
        );
    });

    test("C-10 卸载之后 ⇒ **五条流全部被重订**（漏一个就是静默停流）", async () => {
        setup();
        const router = fakeKernel();
        const services = await unloadServices(router);
        // ⚠️ **先订阅上**（内核侧）⇒ 卸载把它们清掉 ⇒ 之后必须全在。
        await services.waveform.subscribeProbeStream();
        await services.console.subscribeConsoleStream();
        await services.memoryMap.subscribeMemoryMapStream();
        await services.registerMemory.resubscribe();
        assert.equal(router.topics.size, 4, "前置：四条内核侧订阅都在");
        await app.handleCommand({ action: "unload-project" }, services);
        // ① **内核侧**：卸载清空了主题集合 ⇒ 之后必须被重订回来。
        for (const topic of ["sim.registers", "sim.waveform", "sim.console", "memory.map"]) {
            assert.equal(
                router.topics.has(topic), true,
                `卸载之后 ${topic} 没有被重订 —— 那条流会静默停掉`,
            );
        }
        // ② **前端侧**：五条事件流的前端订阅还活着。
        for (const topic of [
            "sim.registers",
            "sim.state.delta",
            "waveform.history",
            "sim.console",
            "memory.map",
        ]) {
            assert.equal(
                router.subs.has(topic), true,
                `前端侧 ${topic} 的订阅没了 —— 那条流没人消费了`,
            );
        }
    });

    test("C-11 卸载之后 ⇒ 前端缓存**清空**（寄存器 / 波形 / 反汇编 / console）", async () => {
        setup();
        const router = fakeKernel();
        const services = await unloadServices(router);
        // ⚠️ 造出「上一个程序」的状态（寄存器 / 波形 / 反汇编 / console）。
        //
        // ⚠️ **寄存器缓存必须真的非零**：否则「缓存被清空」这条断言会**恒
        // 真**（清零一个本来就是零的缓存 ⇒ 看不出区别）⇒ 那样它就**钉不住**
        // 任何东西（实测：把「卸载不清寄存器缓存」这个变异放进去，全量仍
        // 409 条全绿 ⇒ 判别力缺口 ⇒ 补上**真的**非零缓存）。
        for (const fn of router.subs.get("sim.registers") ?? []) {
            fn({ variant: "Registers", topic: "sim.registers", value: { index: 1, values: [42] } });
        }
        assert.equal(
            services.registerMemory.getSnapshot().gpr[1], 42,
            "前置：寄存器缓存里必须有一个非零值",
        );
        m03.setState("waveform", {
            wave: { channels: [{ id: "x", label: "x", edges: 1, lastValue: 1 }] },
        });
        m03.setState("debug", {
            sim: { disasm: [{ addr: 1, code: 2, mnemonic: "nop", width: 4, file: null, line: null }] },
        });
        m03.setState("terminal", { term: { bytes: "hello" } });
        assert.equal(m03.getState("terminal").term.bytes, "hello");
        await app.handleCommand({ action: "unload-project" }, services);
        assert.deepEqual(
            services.registerMemory.getSnapshot().gpr,
            new Array(32).fill(0),
            "寄存器缓存没被清空（状态必须跟着会话一起重建）",
        );
        assert.equal(services.registerMemory.getSnapshot().pc, 0, "PC 没被清空");
        assert.deepEqual(m03.getState("waveform").wave.channels, [], "波形缓存没被清空");
        assert.deepEqual(m03.getState("debug").sim.disasm, [], "反汇编面板没被清空");
        assert.equal(m03.getState("terminal").term.bytes, "", "console 已收字节没被清空");
    });

    test("C-12 「卸载工程」**不**清「当前工程」的记忆 ⇒ 「重新装载工程」仍可用", async () => {
        setup();
        const router = fakeKernel();
        const services = await unloadServices(router);
        const project = { id: "p", name: "工程" };
        await app.loadProjectIntoKernel(services, project);
        await app.handleCommand({ action: "unload-project" }, services);
        const count = () => router.sent.filter(
            (c) => c && typeof c === "object" && "LoadProject" in c,
        ).length;
        const before = count();
        await app.handleCommand({ action: "reload-project" }, services);
        assert.equal(
            count(), before + 1,
            "卸载之后「重新装载工程」必须仍可用（用户不必重新选文件）",
        );
        const note = dom.document.getElementById("kernel-note").textContent;
        assert.ok(
            !note.includes("还没有装载过工程"),
            `卸载不该清掉「当前工程」的记忆：${note}`,
        );
    });

    test("C-13 编辑器**没有**第二个预算输入框（裁决 L）", () => {
        setup();
        assert.equal(
            $$("[data-mmap-field='region_budget_bytes']").length, 1,
            "预算输入框必须只有一个",
        );
        assert.equal(
            $$("[data-mmap-field='max_region_bytes']").length, 0,
            "不许出现第二个预算旋钮（运行期上界由内核管）",
        );
        const note = $("[data-mmap-budget-note]");
        assert.ok(note, "没有预算说明");
        assert.ok(
            !note.textContent.includes("运行期物理占用"),
            `旧措辞（同一个数还是运行期物理占用的上界）必须改掉：${note.textContent}`,
        );
        assert.ok(
            !note.textContent.includes("同一个数"),
            `旧措辞必须改掉：${note.textContent}`,
        );
        assert.ok(
            note.textContent.includes("声明大小上限"),
            `必须说清这是「每段的声明大小上限」：${note.textContent}`,
        );
    });
});

describe("板级内存图 · 差量提交后的落库门槛（STAGE-16 · C-14）", () => {
    test("C-14 差量提交成功后 ⇒ 「已被内核接受」的那一份**逐字段等于草稿**", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(USER_BOARD, false, true, 2);
        const draft = mm.draftFromDescription(USER_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);
        const state = m03.getState("memory-map");
        const accepted = state.acceptedDescription;
        assert.ok(accepted, "提交成功后必须记下被内核接受的那一份");
        assert.ok(
            mm.sameDescription(accepted, mm.draftToDescription(draft)),
            "被接受的那一份必须与草稿**逐字段相同**（裁决 I 的推论）",
        );
        assert.equal(
            mm.isDraftAccepted(state), true,
            "「已被内核接受」这个推导量必须为真",
        );
        assert.equal(
            $("[data-mmap-validated]").getAttribute("data-mmap-validated"), "1",
            "面板上的校验状态必须变成「已接受」",
        );
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), false,
            "落库门槛必须照旧可用（差量提交成功后也能存板）",
        );
    });
});
