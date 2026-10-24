// v0.32 · Lane B —— 「乱序到达」的护栏与用例（STAGE-17 · D-146 · 裁决 F / G / H）
//
// 本套件盯住验收 C-1 ~ C-6：
//
//   C-1 收到**严格更旧**的 `memory.map` 事件 ⇒ **不覆盖**已有状态 ⇒ 丢弃计数 +1
//   C-2 收到**相同**修订号的事件 ⇒ **照收**（卸载后的重订推送是**同号** ⇒ 裁决 G）
//   C-3 收到**更新**的事件 ⇒ **照收** ⇒ 状态推进
//   C-4 护栏**不改提交路径**：基底旧了就**照常提交**（载荷形状不变）⇒
//       ⇒ 内核的 `expected_revision` **照常兜底**（裁决 H）
//   C-5 乱序之后提交 ⇒ 带的是**较新**的修订号 + 差量基底 == **较新**那张图
//       ⇒ ⇒ **真内核接受**那一半**不在本文件**（真机层的 T-6）
//   C-6 护栏**不改**「已被内核接受」这个**推导量** ⇒ 落库门槛照旧可用
//
// ⚠️ **C-5 只钉前端层可证的那半**（规格 §9 订正 1）：默认前端门禁用的是
// `front/tests/fake-wasm-module.mjs`（**假 wasm**）⇒ 真 wasm 产物
// `front/assets/pkg/` 被 `.gitignore` 忽略 ⇒ **默认门禁里没有真内核**
// ⇒ ⇒ 「真内核接受」那一半归真机层的 **T-6**（`tools/browser-verify.mjs`）
// ⇒ ⇒ 这里**不假造**一条「内核接受」的断言。
//
// ⚠️ **判别力（对照必须成组）**：C-1 / C-2 / C-3 是**一组**（「更旧」/
// 「同号」/「更新」）⇒ 少了任何一条，「一律不更新」或「一律更新」都会让
// 另外两条**假绿** ⇒ 三条必须同时在。
//
// ⚠️ **判据是族 + 码，不靠文案**（与既有前端门禁同一口径）。
//
// ⚠️ **护栏的唯一落点** = `front/app.js` 的 `wireMemoryMap` 事件处理器
// ⇒ ⇒ 视图层 / 服务层**不复制**这条判据 ⇒ 本文件只从**接线**上观察它。

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

// ⚠️ **关掉 `app.js` 的自启动**（与 `memory-map.test.mjs` 同一条理由）：M-03 的
// 视图状态是**模块级单例** ⇒ bootstrap 的异步 reset 可能落在任意两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const message = await importFront("lib/message.js");
const mm = await importFront("views/components/memory-map.js");

// 视图组件的渲染监听器（`app.js` 的 bootstrap 会挂它；这里自启动关掉了）。
await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/**
 * **旧**的那张板（字面量）：**两段**（一段 RAM + 一段 ROM）。
 */
const OLD_BOARD = {
    schema_version: 1,
    id: "ordering-board",
    name: "乱序板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        { kind: "rom", base: 4096, size: 1048576 },
    ],
};

/**
 * **新**的那张板（字面量）：**一段**（ROM 那一段没了）。
 *
 * ⚠️ **元数据与 OLD_BOARD 逐字相同** ⇒ 只有**段**不同 ⇒ 这样「基底是哪一张」
 * 才会体现在**差量载荷**上（若元数据不同 ⇒ 提交会走**整份替换** ⇒ 载荷里
 * 根本看不出基底是谁 ⇒ 判别力就没了）。
 */
const NEW_BOARD = {
    schema_version: 1,
    id: "ordering-board",
    name: "乱序板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
    ],
};

// ---------------------------------------------------------------------------
// 假内核（**照着真内核的样子**做）
// ---------------------------------------------------------------------------

/**
 * 一个**照着真内核的关键行为**做的假内核。
 *
 * ⚠️ 照抄的三条真行为（缺一条 ⇒ 对应的验收就**证伪不了**）：
 *
 *   ① **「订阅即强制推一条当前生效的图」**（STAGE-14 §3.2 · 裁决 J）⇒
 *      订阅时推的那条事件，修订号与手上那份**相同**。
 *   ② **`UnloadProject` 不 +1 修订号**（STAGE-16 §3.3 第 ③ 步）⇒ 卸载后
 *      重订推来的那条事件，修订号**仍然相同**，但 `loaded` 变成 `false`
 *      ⇒ **同号、内容不同** ⇒ 这正是裁决 G 要挡的那条（挡掉 ⇒ 前端停在旧状态）。
 *   ③ **修订号检查**（STAGE-16 §3.1 第 ① 步）⇒ `expected_revision` 与当前
 *      不一致 ⇒ **明确拒绝**（`E_PLT-13`）⇒ 并发丢更新**看得见**。
 *
 * @returns {object} the router double
 */
function fakeKernel() {
    /** @type {Map<string, Set<Function>>} 前端侧（router 侧）的订阅表 */
    const subs = new Map();
    /** @type {object[]} 发出去的命令 */
    const sent = [];
    /** 内核侧的主题集合（`LoadProject` / `UnloadProject` 会清空它）。 */
    const topics = new Set();
    /** 内核自己的「当前生效的图」（内核的那一份，不是前端的）。 */
    let kernelMap = { description: null, pending: false, loaded: false, revision: 0 };

    /** 把内核当前的图推给前端（前端侧订阅者）。 */
    const pushMemoryMap = () => {
        const handlers = subs.get("memory.map");
        if (!handlers || handlers.size === 0) return false;
        const event = {
            variant: "MemoryMap",
            topic: "memory.map",
            value: { ...kernelMap },
        };
        for (const fn of handlers) fn(event);
        return true;
    };

    /** 修订号检查（真内核的 `check_revision`）。 */
    const checkRevision = (expected) => {
        if (expected === null || expected === undefined) return;
        if (expected === kernelMap.revision) return;
        throw message.protocolError(
            "E_PLT-13",
            `期望的修订号 ${expected} 与宿主当前的修订号 ${kernelMap.revision} 不一致（图已被改过）`,
        );
    };

    return {
        sent,
        topics,
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
                // ⚠️ 真行为 ①：订阅 `memory.map` ⇒ **立即**推一条当前生效的图。
                if (payload.Subscribe.topic === "memory.map") pushMemoryMap();
            } else if (payload && typeof payload === "object" && "SetMemoryMap" in payload) {
                // ⚠️ 真行为 ③：修订号检查（**先**于任何应用）。
                checkRevision(payload.SetMemoryMap.expected_revision);
            } else if (payload && typeof payload === "object" && "PatchMemoryMap" in payload) {
                checkRevision(payload.PatchMemoryMap.expected_revision);
            } else if (payload === "UnloadProject") {
                // ⚠️ 真行为 ②：清订阅 + **修订号不 +1** + 「有没有装载镜像」变假
                // ⇒ 回一条**同修订号**的内存图事件（`loaded` 变了）。
                topics.clear();
                kernelMap = { ...kernelMap, loaded: false };
                pushMemoryMap();
            }
            return { value: {} };
        },
        /**
         * 内核推一条 `memory.map` 事件（**内核自己的**那份也跟着更新）。
         *
         * @param {object} description
         * @param {{pending?: boolean, loaded?: boolean, revision?: number}} [flags]
         * @returns {boolean} 前端收到了吗
         */
        emitMemoryMap(description, flags = {}) {
            kernelMap = {
                description,
                pending: flags.pending === true,
                loaded: flags.loaded === true,
                revision: Number(flags.revision ?? 0),
            };
            return pushMemoryMap();
        },
        /**
         * 内核的修订号**偷偷**往前走了（前端不知道 ⇒ 前端的基底**旧了**）。
         *
         * @param {number} [by]
         */
        bumpRevision(by = 1) {
            kernelMap = { ...kernelMap, revision: kernelMap.revision + by };
        },
    };
}

// ---------------------------------------------------------------------------
// 现场
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
 * 装一套「**卸载路径**」用的服务接线（用**真的** M-10 ⇒ 替身编的数骗不了人）。
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
 * @param {object} draft
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

// ---------------------------------------------------------------------------

describe("板级内存图 · 乱序到达（STAGE-17 · C-1 ~ C-6）", () => {
    test("C-1 **严格更旧**的事件 ⇒ **不覆盖**已有状态 ⇒ 丢弃计数 +1", () => {
        setup();
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 5 });
        assert.equal(wiring.get().revision, 5, "前置：手上是较新那份（修订号 5）");
        // ⚠️ **乱序**：一个**严格更旧**的事件**后到**。
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });

        assert.equal(
            wiring.get().revision, 5,
            "严格更旧的事件不许覆盖较新那份的修订号",
        );
        assert.equal(
            wiring.get().description.regions.length, 1,
            "较新那张图必须留着（旧那张有两段 ⇒ 段数对不上就是被覆盖了）",
        );
        assert.equal(
            wiring.discardedCount(), 1,
            "丢弃必须**可观测**（计数 +1 ⇒ 判定靠数、不靠文案）",
        );
        // ⚠️ **两个视图状态**也不许被旧事件改掉（它们与缓存是同一个引用）。
        assert.equal(m03.getState("memory-map").memoryMap.revision, 5);
        assert.equal(m03.getState("memory-map").memoryMap.description.regions.length, 1);
        assert.equal(m03.getState("product").memoryMap.revision, 5);
    });

    test("C-2 **相同**修订号的事件 ⇒ **照收**（卸载后的重订推送是同号 ⇒ 裁决 G）", async () => {
        setup();
        const router = fakeKernel();
        const services = await unloadServices(router);
        const wiring = services.memoryMap;
        // 内核此刻的图：修订号 4、**已装载镜像**。
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 4 });
        assert.equal(wiring.get().revision, 4, "前置：手上是修订号 4");
        assert.equal(wiring.get().loaded, true, "前置：已装载镜像");

        // ⚠️ 「卸载工程」⇒ 内核清订阅 + **修订号不 +1** + 「已装载镜像」变假
        // ⇒ 随后的重订推送是**同号**、但**内容不同**（`loaded = false`）。
        await app.handleCommand({ action: "unload-project" }, services);

        assert.equal(
            wiring.get().revision, 4,
            "卸载不改图 ⇒ 修订号不变（同号事件必须**照收**）",
        );
        assert.equal(
            wiring.get().loaded, false,
            "**同号**事件必须照收 ⇒ `loaded` 要更新（挡掉 ⇒ 前端停在旧状态）",
        );
        assert.equal(
            wiring.discardedCount(), 0,
            "同号事件**不许**被丢弃（裁决 G）",
        );
    });

    test("C-3 **更新**的事件 ⇒ **照收** ⇒ 状态推进", () => {
        setup();
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 5 });

        assert.equal(wiring.get().revision, 5, "更新的修订号必须被收下");
        assert.equal(
            wiring.get().description.regions.length, 1,
            "更新的那张图必须生效（否则「一律不更新」也会让这条假绿）",
        );
        assert.equal(
            wiring.discardedCount(), 0,
            "更新的事件**不许**被丢弃（否则「一律丢弃」也会让这条假绿）",
        );
        assert.equal(m03.getState("memory-map").memoryMap.revision, 5);
    });

    test("C-4 护栏**不改提交路径**：基底旧了就**照常提交** ⇒ `expected_revision` 照常兜底", async () => {
        setup();
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 7 });
        // ⚠️ 内核的修订号**偷偷**往前走了（前端不知道 ⇒ 前端的基底**旧了**）。
        router.bumpRevision(2);
        assert.equal(wiring.get().revision, 7, "前置：前端手上还是旧的那个修订号");

        const draft = mm.draftFromDescription(NEW_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);

        // ⚠️ 命令**照常发了**（护栏不静默吞、不改载荷形状）。
        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(
            cmd,
            "基底旧了也要**照常提交**（护栏不许静默吞掉提交 ⇒ 裁决 H）",
        );
        assert.deepEqual(
            Object.keys(cmd.PatchMemoryMap).sort(),
            ["expected_revision", "ops"],
            "载荷形状不许变（`expected_revision` 照常带上）",
        );
        assert.equal(
            cmd.PatchMemoryMap.expected_revision, 7,
            "期望修订号照常带上（内核据此兜底）",
        );
        // ⚠️ 内核的修订号检查**照常兜底** ⇒ `E_PLT-13` 如实显示（不吞掉）。
        const box = $("[data-memory-map-error]");
        assert.ok(box, "内核的拒绝必须如实显示（不静默）");
        assert.equal(box.getAttribute("data-error-code"), "E_PLT-13");
        assert.equal(box.getAttribute("data-error-family"), "PLT", "族必须是 PLT");
        assert.equal(box.getAttribute("data-error-scenario"), "13");
    });

    test("C-5 乱序之后提交 ⇒ 带的是**较新**的修订号 + 差量基底 == **较新**那张图", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        // ⚠️ 顺序：**先新后旧** ⇒ 旧的那条是**乱序到达**的。
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 5 });
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });

        const draft = mm.draftFromDescription(NEW_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);

        const cmd = findCommand(router.sent, "PatchMemoryMap");
        assert.ok(cmd, "改一段 ⇒ 应当走差量");
        assert.equal(
            cmd.PatchMemoryMap.expected_revision, 5,
            "带上的必须是**较新**那个修订号（不是乱序到达的旧那个）",
        );
        // ⚠️ **差量基底 == 较新那张图**：基底若是旧那张（两段）⇒ 载荷里会
        // 多出一条 `RemoveRegion`（删掉旧图里才有的 ROM 段）⇒ 段数对不上。
        assert.equal(
            cmd.PatchMemoryMap.ops.length, 1,
            `载荷里只能有那一段的 op（基底若被换成旧图 ⇒ 会多出 RemoveRegion）：`
            + JSON.stringify(cmd.PatchMemoryMap.ops),
        );
        assert.deepEqual(cmd.PatchMemoryMap.ops[0], {
            UpsertRegion: { spec: { kind: "ram", base: 2147483648, size: 4194304 } },
        });
    });

    test("C-6 护栏**不改**「已被内核接受」这个推导量 ⇒ 落库门槛照旧可用", async () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 5 });
        const draft = mm.draftFromDescription(NEW_BOARD);
        draft.regions[0].size = "4194304";
        await submitDraft(router, draft);

        // ⚠️ 提交成功 ⇒ 这一份**已被内核接受** ⇒ 落库门槛解禁。
        assert.equal(
            mm.isDraftAccepted(m03.getState("memory-map")), true,
            "前置：草稿已被内核接受（推导量为真）",
        );
        assert.equal(
            $("[data-mmap-validated]").getAttribute("data-mmap-validated"), "1",
            "面板上必须显示「已被内核接受」",
        );
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), false,
            "「保存当前板」必须**可用**（落库门槛）",
        );

        // ⚠️ 一个**乱序**（更旧）的事件到达 ⇒ 护栏丢弃它 ⇒ 推导量不许变。
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });
        assert.equal(
            mm.isDraftAccepted(m03.getState("memory-map")), true,
            "护栏不许改「已被内核接受」这个推导量",
        );
        assert.equal(
            $("[data-mmap-validated]").getAttribute("data-mmap-validated"), "1",
            "面板上仍必须显示「已被内核接受」",
        );
        assert.equal(
            $("[data-action='save-memory-map']").hasAttribute("disabled"), false,
            "落库门槛必须**照旧可用**",
        );
    });

    test("丢弃计数**只增不减**（`reset()` 不抹掉证据）", () => {
        setup();
        const router = fakeKernel();
        const wiring = app.wireMemoryMap(router);
        router.emitMemoryMap(NEW_BOARD, { loaded: true, revision: 5 });
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });
        assert.equal(wiring.discardedCount(), 1);
        wiring.reset();
        assert.equal(
            wiring.discardedCount(), 1,
            "`reset()` 只清「当前生效的图」⇒ 丢弃的**证据**不许被抹掉",
        );
        // ⚠️ 清完之后，下一个事件**重新**成为「手上那个」。
        router.emitMemoryMap(OLD_BOARD, { loaded: true, revision: 3 });
        assert.equal(wiring.get().revision, 3, "清掉缓存之后 ⇒ 新事件照收");
        assert.equal(wiring.discardedCount(), 1, "计数不因新事件而变");
    });
});
