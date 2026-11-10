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

describe("板级内存图 · 装载工程后的强制重订", () => {
    test("装载工程后内存图流被**强制重订**（否则表永远停在旧图上）", async () => {
        const router = fakeKernel();
        const bridge = app.wireMemoryMap(router);
        await bridge.subscribeMemoryMapStream();
        assert.equal(router.topics.has("memory.map"), true, "先订阅上了");

        await app.loadProjectIntoKernel(servicesFor(router, bridge), { id: "p" });
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
        const bridge = app.wireMemoryMap(router);
        const services = servicesFor(router, bridge);
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
        const bridge = app.wireMemoryMap(router);
        const services = servicesFor(router, bridge);
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
        // ⚠️ **v0.76（STAGE-52 · B3）**：`auto_selectable` 已废弃 ⇒ ⇒
        // 草稿与线上描述**都不再带**这个键。
        assert.deepEqual(
            Object.keys(description).sort(),
            [
                "id", "name", "priority",
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
        assert.deepEqual(Object.keys(peripheral.device).sort(), ["device", "id", "irq_line"]);
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


describe("D-178 · 失败的装载也要重订流（修订号不许停在旧值）", () => {
    test("装载失败 ⇒ 内存图流**仍然被重订**（否则前端学不到新修订号）", async () => {
        const router = fakeKernel();
        const bridge = app.wireMemoryMap(router);
        await bridge.subscribeMemoryMapStream();
        assert.equal(router.topics.has("memory.map"), true, "先订阅上了");

        const services = servicesFor(router, bridge);
        // ⚠️ 装载在**内核已经顶了修订号之后**失败（STAGE-47 §7.1 第 7 条
        //   那个**可达**路径）。
        services.registerMemory.loadProject = async (project) => {
            await router.dispatchCommand(message.Command.loadProject(project));
            // ⚠️ 内核这边：板被应用了 ⇒ 修订号 +1 ⇒ ⇒ 然后装载失败。
            throw new Error("装载失败：固件要的地址不在板上");
        };

        await assert.rejects(
            () => app.loadProjectIntoKernel(services, { id: "p" }),
            /装载失败/,
            "装载失败**必须**照旧抛出来（错误语义一字未变）",
        );
        assert.equal(
            router.topics.has("memory.map"),
            true,
            "装载失败后内存图流**没有**被重订 ⇒ ⇒ 前端学不到内核顶上去的修订号 ⇒ ⇒ 下次改板报 E_PLT-13",
        );
    });

    test("装载失败后重订 ⇒ 前端**真的**学到了内核的新修订号", async () => {
        // ⚠️ **正向对照**：重订之后内核**强推**一次 ⇒ ⇒ 前端的 revision
        //   **真的**变成新值 ⇒ ⇒ 「重订了」不只是「发过一条命令」。
        const router = fakeKernel();
        const bridge = app.wireMemoryMap(router);
        await bridge.subscribeMemoryMapStream();
        // ⚠️ 先学一个**旧**修订号（0）。
        router.emitMemoryMap(USER_BOARD, false, false, 0);
        assert.equal(bridge.get().revision, 0, "先落在旧修订号上");

        const services = servicesFor(router, bridge);
        services.registerMemory.loadProject = async (project) => {
            await router.dispatchCommand(message.Command.loadProject(project));
            throw new Error("装载失败");
        };
        // ⚠️ 假内核：**订阅时**强推当前图（真内核就是这样 ⇒ 契约层
        //   `on_subscribe` 无条件推一次）。
        const origDispatch = router.dispatchCommand.bind(router);
        router.dispatchCommand = async (payload) => {
            const out = await origDispatch(payload);
            if (payload && typeof payload === "object" && "Subscribe" in payload
                && payload.Subscribe.topic === "memory.map") {
                router.emitMemoryMap(USER_BOARD, false, false, 7);
            }
            return out;
        };

        await assert.rejects(
            () => app.loadProjectIntoKernel(services, { id: "p" }),
            /装载失败/,
        );
        assert.equal(
            bridge.get().revision,
            7,
            "前端**没有**学到内核的新修订号 ⇒ ⇒ 它下一次提交会带旧的 expected_revision ⇒ ⇒ 内核报 E_PLT-13",
        );
    });

    test("装载**成功**时行为**一字未变**（只重订一次）", async () => {
        // ⚠️ **必要对照**：成功路径**不许**多发一次订阅 ⇒ ⇒ 否则这条修复
        //   会带来「每次装载多一条命令」的副作用。
        const router = fakeKernel();
        const bridge = app.wireMemoryMap(router);
        await bridge.subscribeMemoryMapStream();
        const services = servicesFor(router, bridge);
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
            "成功路径必须**只**重订一次（失败路径的重订**不许**漏到这里）",
        );
    });
});

// ---------------------------------------------------------------------------
// D-177 的 C —— **角色声明**（`roles`）的草稿往返（STAGE-49 · v0.73）
// ---------------------------------------------------------------------------

describe("STAGE-49 · 角色声明（roles）的草稿往返", () => {
    test("描述里的 `roles` ⇒ ⇒ 草稿里带上 ⇒ ⇒ 提交回去**还在**", () => {
        const description = {
            schema_version: 1,
            id: "roles-board",
            name: "角色板",
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 1048576,
            regions: [{ kind: "ram", base: 0, size: 4096 }],
            roles: { console: "UART1", time_source: "CLINT0" },
        };
        const draft = mm.draftFromDescription(description);
        assert.equal(
            draft.roles?.console, "UART1",
            "描述里的角色**必须**进草稿（否则一提交就把角色丢了 ⇒ ⇒ console 悄悄变回第一个 UART）",
        );
        assert.equal(draft.roles?.time_source, "CLINT0");

        const back = mm.draftToDescription(draft);
        assert.deepEqual(
            back.roles, { console: "UART1", time_source: "CLINT0" },
            "角色必须**逐字段**回到线上描述",
        );
    });

    test("没有 `roles` 的板 ⇒ ⇒ 往返后**仍然没有**（不塞空对象）", () => {
        const description = {
            schema_version: 1,
            id: "no-roles-board",
            name: "无角色板",
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 1048576,
            regions: [{ kind: "ram", base: 0, size: 4096 }],
        };
        const draft = mm.draftFromDescription(description);
        assert.equal(draft.roles, undefined, "没有角色的板**不许**被塞一个空对象");
        const back = mm.draftToDescription(draft);
        assert.equal(
            back.roles, undefined,
            "线上描述**不许**多一个 `roles` 键（与契约层的 skip_serializing_if 同形）",
        );
    });

    test("归一化**不判定**（前端**不**写第二个校验器）", () => {
        // ⚠️ 角色指错了由**内核**的 V-20 裁决 ⇒ ⇒ 前端**只透传** ⇒ ⇒
        // 前端**不许**因为「这个 id 在不在」而改写角色。
        const roles = mm.normalizeRoles({ console: "NOT-THERE" });
        assert.equal(
            roles.console, "NOT-THERE",
            "前端**不许**自己判角色指得对不对 ⇒ ⇒ 原样透传",
        );
        assert.equal(mm.normalizeRoles(null), null, "null ⇒ null");
        assert.equal(mm.normalizeRoles({}), null, "空 ⇒ null");
    });
});

// ---------------------------------------------------------------------------
// STAGE-49 · v0.73 批 —— **描述格式一次升版**（N 组）
//
// 这一套盯住规格 §4.3 的四条：
//
//   N-1 前端镜像的**四处版本号**与契约层**逐字相同**；
//   N-2 前端**零处**自己判「描述格式对不对」⇒ ⇒ 合法性**只**由内核判；
//   N-3 板档案的 `rules_version` 为**旧**的板 ⇒ ⇒ **仍能装** ⇒ ⇒ 前端
//       **显式提示**「按旧规则存的」；
//   N-4 前端**不自动**改写板档案的描述 ⇒ ⇒ 迁移是**用户触发**的。
//
// ⚠️ **N-1 的「四处」** = `SCHEMA_VERSION`（契约信封）· `SIM_STATE_SCHEMA_VERSION`
//   · `PLATFORM_SCHEMA_VERSION`（描述**形状**）· `PLATFORM_RULES_VERSION`（**规则集**）
//   ⇒ ⇒ 四个数**各管各的**（⇒ ⇒ 别混）。
// ---------------------------------------------------------------------------

describe("STAGE-49 · v0.73 描述格式升版（N 组）", () => {
    test("N-1 · 前端镜像的**四处版本号**与契约层**逐字相同**", () => {
        // ⚠️ **逐字读 Rust 源码**（⇒ ⇒ 不是「跟一个常量比」那种恒真写法）。
        const contractSrc = readFileSync(
            resolve(frontDir, "../crates/contract/src/lib.rs"),
            "utf8",
        );
        const platformSrc = readFileSync(
            resolve(frontDir, "../crates/contract/src/platform.rs"),
            "utf8",
        );
        const modelSrc = readFileSync(
            resolve(frontDir, "../crates/contract/src/model.rs"),
            "utf8",
        );
        // ⚠️ **前端镜像导出的是三个**（`SIM_STATE_SCHEMA_VERSION` 是**会话
        //   快照**的版本 ⇒ ⇒ 前端不碰它）⇒ ⇒ 见规格 §7.1 订正 11。
        const expectations = [
            ["SCHEMA_VERSION", contractSrc],
            ["PLATFORM_SCHEMA_VERSION", platformSrc],
            ["PLATFORM_RULES_VERSION", platformSrc],
        ];
        for (const [name, src] of expectations) {
            const m = src.match(new RegExp(`pub const ${name}:\\s*u32\\s*=\\s*(\\d+)`));
            assert.ok(m, `契约层必须定义 ${name}`);
            assert.equal(
                Number(message[name]),
                Number(m[1]),
                `${name}：前端镜像必须与契约层**逐字相同**`,
            );
        }
    });

    test("N-2 · 前端**零处**自己判「描述格式对不对」⇒ ⇒ 合法性**只**由内核判", () => {
        // ⚠️ **读源码**（⇒ ⇒ 前端**不许**出现「描述格式版本对不对」那套判定）。
        const mmSrc = readFileSync(resolve(frontDir, "views/components/memory-map.js"), "utf8");
        const storeSrc = readFileSync(resolve(frontDir, "services/m23_boardstore.js"), "utf8");
        // ⚠️ **允许**的只有「**读**那个版本号去比对」⇒ ⇒ **不许**的是
        //   「**自己判**一份描述**合不合法**」⇒ ⇒ ⇒ 前端**不写第二个校验器**。
        for (const [label, src] of [["memory-map", mmSrc], ["boardstore", storeSrc]]) {
            assert.ok(
                !/schema_version\s*!==|schema_version\s*==\s*\d/.test(src),
                `${label}：前端**不许**自己判描述的格式版本对不对（合法性只由内核判）`,
            );
        }
    });

    test("N-3 · 存量板（`rules_version` 是**旧**的）⇒ ⇒ **仍能装** ⇒ ⇒ 前端**显式提示**", async () => {
        const boardstore = await importFront("services/m23_boardstore.js");
        // ⚠️ **老条目**（按旧规则校验过的）⇒ ⇒ **规则版本落后** ⇒ ⇒ 要提示。
        const oldEntry = {
            id: "old-board",
            description: { schema_version: 1, id: "old-board", name: "老板" },
            rules_version: message.PLATFORM_RULES_VERSION - 1,
        };
        const check = boardstore.checkBoardRules(oldEntry);
        assert.equal(check.ok, false, "老条目的规则版本落后 ⇒ ⇒ 要报出来");
        assert.equal(check.reason, "rules");
        const notice = boardstore.boardRulesNotice(oldEntry);
        assert.ok(notice, "老条目**必须**有一句提示（**不静默**）");
        assert.ok(
            notice.includes(String(message.PLATFORM_RULES_VERSION)),
            "提示里**必须**说清当前规则版本（用户看得见差在哪）",
        );
        // ⚠️ **提示 ≠ 拒用** ⇒ ⇒ 老板**仍能装**（⇒ ⇒ 存量零风险）。
        assert.ok(
            boardstore.checkBoardVersion(oldEntry.description).ok,
            "老描述（schema_version = 1）**仍能装**（缺省优先 ⇒ ⇒ 存量零风险）",
        );
    });

    test("N-4 · 前端**不自动**改写板档案的描述 ⇒ ⇒ 迁移是**用户触发**的", async () => {
        const boardstore = await importFront("services/m23_boardstore.js");
        // ⚠️ **缺基址**的那份描述 ⇒ ⇒ 前端**不许**把它**自动**填上基址
        // （⇒ ⇒ 「由内核决定基址」那个**用户意图**必须**留住**）。
        const description = {
            schema_version: message.PLATFORM_SCHEMA_VERSION,
            id: "auto-base-board",
            name: "自动基址板",
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 1048576,
            regions: [{ kind: "ram", size: 4096 }],
        };
        const draft = mm.draftFromDescription(description);
        assert.equal(
            draft.regions[0].base,
            "",
            "缺基址的段**必须**在草稿里保持**留空**（前端**不许**自动填 0）",
        );
        const back = mm.draftToDescription(draft);
        assert.equal(
            "base" in back.regions[0],
            false,
            "线上描述**不许**多一个 `base` 键（前端**不许**自动改写）",
        );
        // ⚠️ **板档案那一层**也一样：它只**存**用户交的那份 ⇒ ⇒ 不改写。
        const entry = boardstore.boardEntry(description);
        assert.deepEqual(entry.description, description, "板档案**不许**改写描述");
    });
});
