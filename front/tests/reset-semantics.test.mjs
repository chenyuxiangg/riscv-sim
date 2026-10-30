// 复位语义（v0.59 批 · 用户报的现象「复位后无法再运行」）的前端门禁。
//
// ⚠️ **用户报的现象**：UI 上点「复位」之后**再也跑不起来**。
//
// ⚠️ **三段分层钉**（每段都有它自己的那一层判据）：
//   ① **复位装的是「当前工程」**（**本文件**）—— 复位**不是**「重装内置 demo
//      工程」⇒ ⇒ 否则用户装进去的固件被顶掉 ⇒ ⇒ 界面还停在「已装载」⇒ ⇒
//      再点「加载固件」落进 `already-loaded` 那一格 ⇒ ⇒ **不重装** ⇒ ⇒
//      **用户的固件再也跑不起来**；
//   ② **同一个文件重选能再次触发 `change`**（**本文件** + 真机门禁的
//      `loadImage` 段）—— Chrome 的行为是「**值没变 ⇒ 不触发** `change`」⇒ ⇒
//      重选同一个固件**选不动**⇒ ⇒ 固件回不来；
//   ③ **内核可复活**（Rust 门禁 `crates/kernel/src/kernel_host/tests_v059_reset.rs`）
//      —— 看门狗终止态**跨 `LoadProject` 存活** ⇒ ⇒ 复位救不回死掉的会话。
//
// ⚠️ **本文件的判据全部落在「发出去的那条命令」上**（线上帧的载荷）⇒ ⇒
// 测的是**真**行为，不是「我调用了某个内部函数」。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
// 简单复合选择器 ⇒ ⇒ 一律用**扁平选择器**。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const appSource = readFileSync(resolve(frontDir, "app.js"), "utf8");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `run-live.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意
// 两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Command, Event: EventPayload, eventMessage } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await importFront("views/m02_viewcomponents.js").then((m) => m.mountViewComponents());

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 一个**假**的 ELF 头（4 字节 magic + 一点内容）—— 够读、够传、够断言。 */
const ELF_BYTES = Object.freeze([0x7f, 0x45, 0x4c, 0x46, 0x01, 0x01, 0x01, 0x00]);

/** 用户选的文件名。 */
const ELF_NAME = "demo-user.elf";

/** 内置 demo 工程的项目 id（**字面量** ⇒ ⇒ 不从被测实现反推）。 */
const MOCK_PROJECT_ID = "proj-mock-light";

// ---------------------------------------------------------------------------
// 替身
// ---------------------------------------------------------------------------

/**
 * 造一个 M-10 的替身 —— **真的**把 `LoadProject` 发出去。
 *
 * ⚠️ **必须真的发**：否则「线上帧里有一条 `LoadProject`」这条断言**永远 0 条**
 * ⇒ ⇒ 假红。
 *
 * @param {object} router
 * @param {string[]} [topics]
 */
function makeRegisterMemory(router, topics = []) {
    return {
        async loadProject(project) {
            await router.dispatchCommand(Command.loadProject(project));
            topics.push("sim.registers", "sim.state.delta");
        },
        async resubscribe() { topics.push("sim.registers", "sim.state.delta"); },
        async readRegisters() {},
        async readMemory() {},
        reset() {},
    };
}

function makeWaveform(topics = []) {
    return {
        reset() {},
        async subscribeProbeStream() { topics.push("sim.waveform"); },
    };
}

function makeDebug() {
    return { reset() {} };
}

function makeConsole(topics = []) {
    return {
        reset() {},
        async subscribeConsoleStream() { topics.push("sim.console"); },
    };
}

function makeMemoryMap(topics = []) {
    return {
        reset() {},
        async subscribeMemoryMapStream() { topics.push("memory.map"); },
    };
}

/**
 * 装一套干净的接线（视图状态是**模块级单例**，用例之间共享 ⇒ 每次都重置）。
 *
 * @returns {Promise<{router: object, transport: object, services: object}>}
 */
async function setup() {
    m03.reset();
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
            const frame = {
                type: "event",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: eventMessage(message.seq, EventPayload.tick(0, 0)),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    const router = await m05.mountCommandRouter({ transport });
    m01.assembleWorkspace("embedded");
    m01.route("#/product");
    const topics = [];
    const services = {
        commandRouter: router,
        registerMemory: makeRegisterMemory(router, topics),
        waveform: makeWaveform(topics),
        debug: makeDebug(),
        console: makeConsole(topics),
        memoryMap: makeMemoryMap(topics),
    };
    // ⚠️ **页头那个控件**要挂上去（文件选择器在页头 ⇒ ⇒ 复位那条路要能被测到）。
    app.wireTopbarImageLoader(services);
    return { router, transport, services };
}

/**
 * 取线上帧里的 `LoadProject` 载荷（**线上形状** = `payload.Command.LoadProject`）。
 *
 * @param {{sent: any[]}} transport
 * @returns {object[]}
 */
function loadProjects(transport) {
    return transport.sent
        .filter((s) => s.message?.payload?.Command && "LoadProject" in s.message.payload.Command)
        .map((s) => s.message.payload.Command.LoadProject);
}

/**
 * 在**页头**那个入口里「选一个文件」—— 走**真实**的 `change` 事件路径。
 *
 * @param {string} name
 * @param {number[]} bytes
 */
async function selectFileInTopbar(name, bytes) {
    const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
    assert.ok(input, "页头没有渲染文件选择器");
    input.files = [new File([new Uint8Array(bytes)], name)];
    input.dispatchEvent(new Event("change"));
    // ⚠️ **等一拍**：`change` 的处理器是 `async`（要 `await` 读字节）。
    await new Promise((r) => setTimeout(r, 0));
}

function clickTopbarLoad() {
    const btn = dom.document.querySelector('[data-action="load-image"][data-image-load-from="topbar"]');
    assert.ok(btn, "页头没有渲染「加载固件」按钮");
    btn.dispatchEvent(new Event("click"));
}

/**
 * 等一个条件成立（轮询）。
 *
 * @param {() => boolean} predicate
 * @param {number} [timeoutMs]
 * @returns {Promise<boolean>}
 */
async function waitFor(predicate, timeoutMs = 3000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        if (predicate()) return true;
        await new Promise((r) => setTimeout(r, 10));
    }
    return predicate();
}

/**
 * 读页头那条短状态（`data-image-brief`）。
 *
 * @returns {{status: string|null, text: string}}
 */
function topbarBrief() {
    const brief = dom.document.querySelector("[data-image-brief]");
    return {
        status: brief ? String(brief.getAttribute("data-image-brief-status") ?? "") : null,
        text: brief ? String(brief.textContent ?? "") : "",
    };
}

/**
 * 剥掉源码里的**注释**（行注释 + 块注释）。
 *
 * ⚠️ **为什么需要**：判据要匹配的是**代码**，不是**解释代码的那段字**⇒ ⇒
 * 否则注释里恰好出现了那个标识符 ⇒ ⇒ 判据**恒绿**（v0.59 批 · 变异 M3 实测
 * 踩到 ⇒ ⇒ 改回硬编码 demo 工程**照样绿**⇒ ⇒ **假绿**）。
 *
 * ⚠️ **不能动字符串字面量里的内容** ⇒ ⇒ 这里只按「注释的起止标记」切 ⇒ ⇒
 * 字符串里若含 `//` 会被误伤（**已知边界**：本仓 `app.js` 的复位那一段没有
 * 这种字面量 ⇒ ⇒ 判据只对**那一段**生效）。
 *
 * @param {string} src
 * @returns {string}
 */
function stripComments(src) {
    return src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
}

// ---------------------------------------------------------------------------
// ① 复位 · 装的是「当前工程」
// ---------------------------------------------------------------------------

describe("复位 · 装的是**当前工程**（用户报的现象）", () => {
    test("装完用户固件 ⇒ ⇒ 复位 ⇒ ⇒ **重新装载当前工程**（⛔ 不是内置 demo 工程）", async () => {
        const { transport, services } = await setup();

        // ① 用户装自己的固件（走**真**的页头路径）。
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        assert.ok(
            await waitFor(() => loadProjects(transport).length >= 1),
            "装固件后应当发出**一条** LoadProject",
        );
        const loaded = loadProjects(transport)[0];
        assert.equal(
            loaded.images[0].name,
            ELF_NAME,
            "前置条件：内核装的是**用户那个**固件",
        );
        assert.notEqual(
            loaded.id,
            MOCK_PROJECT_ID,
            "前置条件：那**不是**内置 demo 工程",
        );

        // ② 复位。
        await app.handleCommand({ action: "reset" }, services);
        assert.ok(
            await waitFor(() => loadProjects(transport).length >= 2),
            "复位应当再发出**一条** LoadProject",
        );

        // ③ 复位装的是**当前工程**（用户那个固件）⇒ ⇒ **不是** demo。
        const reloaded = loadProjects(transport)[1];
        assert.notEqual(
            reloaded.id,
            MOCK_PROJECT_ID,
            "复位**不许**把用户的工程换回内置 demo 工程（那正是「复位后无法再运行」的根因）",
        );
        assert.equal(
            reloaded.images[0].name,
            ELF_NAME,
            "复位后内核里装的**必须**还是用户那个固件",
        );
        assert.deepEqual(
            reloaded,
            loaded,
            "复位 ⇒ ⇒ 重新装载**当前工程**（⇒ ⇒ 逐字段相同 ⇒ ⇒ 不是一个「像的」工程）",
        );
    });

    test("复位后 ⇒ ⇒ 界面状态条与内核装的**是同一个**（⛔ 不是两个真相）", async () => {
        const { transport, services } = await setup();
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        assert.ok(await waitFor(() => loadProjects(transport).length >= 1));

        await app.handleCommand({ action: "reset" }, services);
        assert.ok(await waitFor(() => loadProjects(transport).length >= 2));

        const brief = topbarBrief();
        const reloaded = loadProjects(transport)[1];
        assert.match(
            brief.text,
            new RegExp(ELF_NAME.replace(/\./g, "\\.")),
            "状态条必须说清「现在装的是哪个固件」",
        );
        assert.equal(
            reloaded.images[0].name,
            ELF_NAME,
            "状态条说的固件与内核装的固件**必须**是同一个（⛔ 不许界面在骗人）",
        );
    });

    test("⚠️ **复位那条路读的是「当前工程」那个变量**（⛔ 不许硬编码 demo 工程）", () => {
        // ⚠️ **结构判据**：把 `case "reset":` 那一段**切出来** ⇒ ⇒ 断言它读的是
        // `loadedProject` ⇒ ⇒ 将来有人改回硬编码 `buildMockProject()` ⇒ ⇒ 这条
        // **立刻变红**。
        //
        // ⚠️⚠️ **必须先剥掉注释再匹配**（v0.59 批 · 变异自检 M3 实测踩到）：
        // 那段代码的**注释里**写着 `loadedProject`（解释「为什么是当前工程」）⇒ ⇒
        // 不剥注释的话 ⇒ ⇒ 改回硬编码 demo 工程**照样绿**⇒ ⇒ **假绿**⇒ ⇒
        // 「判据检查的东西」与「被检查的材料里恰好也有的东西」撞车了。
        const start = appSource.indexOf('case "reset":');
        assert.ok(start > 0, "app.js 里没有 `case \"reset\":`");
        const end = appSource.indexOf('case "read-regs"', start);
        assert.ok(end > start, "app.js 里 `case \"reset\":` 后面没有 `case \"read-regs\"`");
        const block = stripComments(appSource.slice(start, end));
        assert.match(
            block,
            /loadedProject/,
            "复位那条路必须读**当前工程**（`loadedProject`）⇒ ⇒ ⛔ 不许硬编码 demo 工程",
        );
        // ⚠️ **反向对照**：剥完注释之后，那段里**确实**还有 `buildMockProject`
        // （它是**兜底**：`loadedProject ?? buildMockProject()`）⇒ ⇒ 说明剥注释
        // **没有**把整段剥空 ⇒ ⇒ 上面那条不是「什么都没匹配上」的恒真。
        assert.match(
            block,
            /buildMockProject/,
            "剥注释之后这段里**应当**还有兜底那一句（⇒ ⇒ 剥注释没把整段剥空）",
        );
    });
});

// ---------------------------------------------------------------------------
// ② 同一个文件重选
// ---------------------------------------------------------------------------

describe("同一个文件重选 · Chrome 的「值没变 ⇒ 不触发 change」", () => {
    test("点了文件选择器 ⇒ ⇒ **清空当前选择** ⇒ ⇒ 选回同一个文件也会是「变了」", async () => {
        await setup();
        const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
        assert.ok(input, "页头没有渲染文件选择器");

        // ⚠️ **先把「当前选择」摆出来** ⇒ ⇒ 否则断言「清空」是**恒真**的
        // （输入框本来就没有值）⇒ ⇒ **先钉住被观测的对象存在**。
        input.value = "C:\\fakepath\\demo-user.elf";
        assert.equal(input.value, "C:\\fakepath\\demo-user.elf", "前置条件：输入框里确实有选择");

        // ⚠️ **真点一下**（用户点文件选择器 ⇒ ⇒ `click` 先于选择器打开）。
        input.dispatchEvent(new Event("click"));

        assert.equal(
            input.value,
            "",
            "点开选择器之前必须**清空当前选择** ⇒ ⇒ 于是「选回同一个文件」在浏览器"
            + "看来**也是变了** ⇒ ⇒ `change` 照常触发（⛔ 不许「选不动」）",
        );
    });

    test("⚠️ **清的是输入框的值**（⛔ 不是清掉待加载的文件）", async () => {
        await setup();
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        const before = topbarBrief();
        assert.match(
            before.text,
            /demo-user/,
            "前置条件：待加载的文件已经记住了（选完就看得见）",
        );

        const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
        input.value = "C:\\fakepath\\demo-user.elf";
        input.dispatchEvent(new Event("click"));

        const after = topbarBrief();
        assert.equal(
            after.text,
            before.text,
            "点一下选择器**不许**把「待加载的文件」清掉（⇒ ⇒ 「再选同一个文件」"
            + "这条路**只差**浏览器那一次 `change`）",
        );
    });
});
