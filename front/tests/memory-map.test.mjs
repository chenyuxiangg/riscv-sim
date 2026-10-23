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
 * @param {{rejectSetMemoryMap?: Error}} [options]
 * @returns {object} the router double
 */
function fakeKernel(options = {}) {
    /** @type {Map<string, Set<Function>>} */
    const subs = new Map();
    /** @type {object[]} */
    const sent = [];
    /** 内核侧的主题集合（`LoadProject` 会清空它）。 */
    const topics = new Set();

    return {
        sent,
        topics,
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
            }
            return { value: {} };
        },
        /**
         * 前端侧收到一条 `memory.map` 事件。
         *
         * ⚠️ 这一条**不**看内核侧的主题集合：它是「内核推了一条事件」这件事
         * 本身（内核侧订没订由 `emitMemoryMapIfSubscribed` 单独守）。
         */
        emitMemoryMap(description, pending = false) {
            const handlers = subs.get("memory.map");
            if (!handlers || handlers.size === 0) return false;
            const event = {
                variant: "MemoryMap",
                topic: "memory.map",
                value: { description, pending },
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
        assert.deepEqual(
            Object.keys(cmd.SetMemoryMap),
            ["description"],
            "`SetMemoryMap` 只带一整份描述，不带「当前值 + 补丁」",
        );
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
