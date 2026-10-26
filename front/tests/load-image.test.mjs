// STAGE-32 · v0.47 批 —— 「界面上加载固件」的前端门禁（A 组）。
//
// 这个套件盯**一条用户能自己走通的路**：
//
//   选一个文件 ⇒ ⇒ 读出字节 ⇒ ⇒ 发一条 `load-image` 信号 ⇒ ⇒ app 层
//   把字节变成工程 ⇒ ⇒ 真的发一条 `LoadProject` 命令 ⇒ ⇒ 装载后**五条流
//   全部重订** ⇒ ⇒ **装完就停**（⛔ 不自动跑）。
//
// ⚠️ **核心不变式**（与源码视图那条老规矩**正好相反**）：
//   源码树**不进内核**（STAGE-7 裁决 E）；而镜像**必须**进内核（否则没东西
//   可跑）⇒ ⇒ 两条路的语义**不同** ⇒ ⇒ 本批**不许**把镜像也做成「只进
//   前端」。
//
// ⚠️ **「装完就停」也是不变式**：加载固件**不**自动跑 ⇒ ⇒ 用户自己点
// 「继续」⇒ ⇒ 这条一旦破掉，「加载」就变成了「加载并跑」，用户会吓一跳。
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

// ⚠️ **关掉 `app.js` 的自启动**（与 `source-tree.test.mjs` 同一条理由）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const loader = await importFront("views/components/image-loader.js");
const { Command, Event: EventPayload, eventMessage } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 一个**假**的 ELF 头（4 字节 magic + 一点内容）—— 够读、够传、够断言。 */
const ELF_BYTES = Object.freeze([0x7f, 0x45, 0x4c, 0x46, 0x01, 0x01, 0x01, 0x00]);

/** 一个**假**的原始字节镜像（**不是** ELF magic）⇒ ⇒ 判 RawHex。 */
const RAW_BYTES = Object.freeze([0xde, 0xad, 0xbe, 0xef]);

/** 用户选的文件名。 */
const ELF_NAME = "demo-user.elf";
const RAW_NAME = "demo-user.bin";

/** 期望的镜像字节数。 */
const ELF_BYTE_LENGTH = 8;
const RAW_BYTE_LENGTH = 4;

/** 期望的**流**订阅条数（寄存器 / 状态增量 / 探针 / console / 内存图）。 */
const EXPECTED_STREAM_COUNT = 5;

// ---------------------------------------------------------------------------

/**
 * 装一套干净的接线（视图状态是**模块级单例**，用例之间共享 ⇒ 每次都重置）。
 *
 * @returns {Promise<{router: object, transport: object}>}
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
    return { router, transport };
}

/**
 * 在产品视图里「选一个文件」—— 走**真实**的 `change` 事件路径。
 *
 * ⚠️ **必须走 `change`**：浏览器把选中的文件**只**交给 `change`（点击时
 * `input.files` 是空的）⇒ ⇒ 用例必须复现**那条**路 ⇒ ⇒ 否则测的是另一
 * 条路。
 *
 * @param {string} name — 文件名
 * @param {number[]} bytes — 文件字节
 */
async function selectFile(name, bytes) {
    const input = dom.document.querySelector('input[data-image-load-from="product"]');
    assert.ok(input, "产品视图没有渲染文件选择器");
    const file = new File([new Uint8Array(bytes)], name);
    // ⚠️ **stub 的 `files`**：DOM 替身没有真的文件列表 ⇒ ⇒ 手工塞上
    // （真实浏览器由用户操作产生它）。
    input.files = [file];
    // ⚠️ **`target` 由 `dispatchEvent` 自己设**（DOM 替身会 shadow Node 的
    // 只读 getter）⇒ ⇒ 这里**不**手工设 ⇒ ⇒ 跟既有测试同一条路。
    input.dispatchEvent(new Event("change"));
    // ⚠️ **等一拍**：`change` 的处理器是 `async`（要 `await` 读字节）。
    await new Promise((r) => setTimeout(r, 0));
}

/**
 * 选一个**读不出来**的文件（`arrayBuffer()` 直接 reject）—— 走**真实**的
 * `change` 事件路径。
 *
 * ⚠️ **为什么要有这条**：规格的 A-11 是「文件读失败时显示**原因**，⛔ 不静默」
 * ⇒ ⇒ 那条验收**原本没有用例** ⇒ ⇒ **零判别力**（把它删掉也没人红）⇒ ⇒
 * 必须补一条。
 *
 * @param {string} [name]
 */
async function selectUnreadableFile(name = "unreadable.elf") {
    const input = dom.document.querySelector('input[data-image-load-from="product"]');
    assert.ok(input, "产品视图没有渲染文件选择器");
    input.files = [{
        name,
        arrayBuffer: () => Promise.reject(new Error("模拟的读取失败")),
    }];
    input.dispatchEvent(new Event("change"));
    // ⚠️ **等一拍**：`change` 的处理器是 `async`（要 `await` 读字节）。
    await new Promise((r) => setTimeout(r, 0));
}

/**
 * 点「加载」按钮 —— 走**真实**的 `click` 事件路径。
 */
function clickLoad() {
    const btn = dom.document.querySelector('[data-action="load-image"][data-image-load-from="product"]');
    assert.ok(btn, "产品视图没有渲染「加载」按钮");
    btn.dispatchEvent(new Event("click"));
}

/**
 * 收集窗口上的 `riscv:command` 信号。
 *
 * @returns {{signals: any[], stop: () => void}}
 */
function captureCommands() {
    /** @type {any[]} */
    const signals = [];
    const handler = (ev) => signals.push(ev.detail);
    dom.window.addEventListener("riscv:command", handler);
    return { signals, stop: () => dom.window.removeEventListener("riscv:command", handler) };
}

// ---------------------------------------------------------------------------

describe("固件加载器 · 纯逻辑", () => {
    test("扩展名 ⇒ 契约层的 `ImageKind`（`.elf` ⇒ Elf）", () => {
        assert.equal(loader.imageKindFor("demo-user.elf"), "Elf");
        assert.equal(loader.imageKindFor("demo-user.out"), "Elf");
    });

    test("未知扩展名 ⇒ RawHex（**不**猜 ELF）", () => {
        assert.equal(loader.imageKindFor("demo-user.bin"), "RawHex");
        assert.equal(loader.imageKindFor("demo-user"), "RawHex");
        assert.equal(loader.imageKindFor("demo-user.hex"), "RawHex");
    });

    test("字节 ⇒ 工程（`ImageSource::Bytes` 同形 + `isa` 是 RV32IMC）", () => {
        const project = loader.buildImageProjectFromBytes(ELF_BYTES, ELF_NAME);
        assert.equal(project.version.isa, "RV32IMC");
        assert.equal(project.images.length, 1);
        assert.equal(project.images[0].name, ELF_NAME);
        assert.equal(project.images[0].kind, "Elf");
        // ⚠️ **外部标签枚举**：`{"Bytes":[...]}`（不是 base64、不是字符串）。
        assert.deepEqual(project.images[0].source.Bytes, [...ELF_BYTES]);
    });
});

describe("产品视图 · 加载固件的 UI 入口", () => {
    test("产品视图渲染出**一个**文件选择器与一个「加载」按钮", async () => {
        await setup();
        const inputs = dom.document.querySelectorAll('input[data-image-load-from="product"]');
        assert.equal(inputs.length, 1, `产品视图的文件选择器应恰好一个，实测 ${inputs.length}`);
        const btns = dom.document.querySelectorAll('[data-action="load-image"][data-image-load-from="product"]');
        assert.equal(btns.length, 1, `产品视图的「加载」按钮应恰好一个，实测 ${btns.length}`);
    });

    test("选择器接受 `.elf` / `.hex` / `.bin`", async () => {
        await setup();
        const input = dom.document.querySelector('input[data-image-load-from="product"]');
        const accept = String(input.getAttribute("accept") ?? "");
        for (const ext of [".elf", ".hex", ".bin"]) {
            assert.ok(
                accept.includes(ext),
                `accept 属性缺 ${ext}（实测 "${accept}"）`,
            );
        }
    });

    test("选文件后**不发命令**（字节先读好，按钮才发）", async () => {
        const { transport } = await setup();
        await selectFile(ELF_NAME, ELF_BYTES);
        assert.equal(
            transport.sent.length,
            0,
            `选文件不该发命令，却发了 ${JSON.stringify(transport.sent)}`,
        );
    });

    test("选完文件 ⇒ 面板显示「已选择」+ 文件名 + 字节数（**选了什么看得见**）", async () => {
        await setup();
        await selectFile(ELF_NAME, ELF_BYTES);
        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(status, "产品视图没有渲染加载状态");
        assert.equal(
            status.getAttribute("data-image-load-status"),
            "idle",
            "选完文件不该是 error",
        );
        const text = String(status.textContent || "");
        assert.ok(text.includes(ELF_NAME), `面板没显示文件名：${text}`);
        assert.ok(text.includes(String(ELF_BYTE_LENGTH)), `面板没显示字节数：${text}`);
    });

    test("文件**读不出来** ⇒ 面板显示**原因**（⛔ 不静默）", async () => {
        await setup();
        await selectUnreadableFile();
        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(status, "产品视图没有渲染加载状态");
        assert.equal(
            status.getAttribute("data-image-load-status"),
            "error",
            "读文件失败时状态应是 error",
        );
        const text = String(status.textContent || "");
        assert.ok(text.includes("模拟的读取失败"), `面板没显示失败原因：${text}`);
    });

    test("点「加载」⇒ 发出一条 `load-image` 信号，载荷含**真字节**与文件名", async () => {
        await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        assert.equal(signals.length, 1, `应恰好一条信号，实测 ${signals.length}`);
        assert.equal(signals[0].action, "load-image");
        assert.equal(signals[0].viewId, "product");
        // ⚠️ **真字节**（不是字符串、不是 base64）。
        assert.deepEqual(signals[0].params.bytes, [...ELF_BYTES]);
        assert.equal(signals[0].params.name, ELF_NAME);
        assert.equal(signals[0].params.kind, "Elf");
    });

    test("没有待加载的文件就点「加载」⇒ 报**明确**的错误（⛔ 不静默）", async () => {
        await setup();
        const { signals, stop } = captureCommands();
        clickLoad();
        stop();

        assert.equal(signals.length, 0, "没有文件时不该发信号");
        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(status, "产品视图没有渲染加载状态");
        assert.equal(
            status.getAttribute("data-image-load-status"),
            "error",
            "没有文件时状态应是 error",
        );
    });
});

describe("app 层 · `load-image` ⇒ 真的 `LoadProject`", () => {
    test("`load-image` ⇒ 线上帧里有一条 `LoadProject`", async () => {
        const { transport } = await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        // ⚠️ **app 层的处理器**是监听 `riscv:command` 的 ⇒ ⇒ 手动喂它。
        const router = await makeRouter(transport);
        await app.handleCommand(signals[0], {
            commandRouter: router,
            registerMemory: makeRegisterMemory(router),
            waveform: makeWaveform(),
            debug: makeDebug(),
            console: makeConsole(),
            memoryMap: makeMemoryMap(),
        });

        // ⚠️ **线上形状** = `payload: { Command: { LoadProject: … } }`
        // （`commandMessage()` 把载荷包在 `Command` 里）⇒ ⇒ 断言必须按
        // **这一层**找 ⇒ ⇒ 找错层 ⇒ ⇒ 断言恒 0 条 ⇒ ⇒ 假红。
        const loadFrames = transport.sent.filter(
            (s) => s.message?.payload?.Command && "LoadProject" in s.message.payload.Command,
        );
        assert.equal(loadFrames.length, 1, `应恰好一条 LoadProject，实测 ${loadFrames.length}`);
        const project = loadFrames[0].message.payload.Command.LoadProject;
        assert.equal(project.version.isa, "RV32IMC");
        assert.deepEqual(project.images[0].source.Bytes, [...ELF_BYTES]);
    });

    test("装载后**五条流全部重订**", async () => {
        const { transport } = await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        const topics = [];
        const router = await makeRouter(transport);
        await app.handleCommand(signals[0], {
            commandRouter: router,
            registerMemory: makeRegisterMemory(router, topics),
            waveform: makeWaveform(topics),
            debug: makeDebug(),
            console: makeConsole(topics),
            memoryMap: makeMemoryMap(topics),
        });

        // 寄存器流 + 状态增量流（由 M-10 那一条路管）+ 探针 + console + 内存图
        for (const topic of [
            "sim.registers",
            "sim.state.delta",
            "sim.waveform",
            "sim.console",
            "memory.map",
        ]) {
            assert.ok(
                topics.includes(topic),
                `装载后没有重订 ${topic}（实测订阅了 ${JSON.stringify(topics)}）`,
            );
        }
    });

    test("⚠️ **装完就停**（⛔ 不自动发 `ContinueRun`）", async () => {
        const { transport } = await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        const router = await makeRouter(transport);
        await app.handleCommand(signals[0], {
            commandRouter: router,
            registerMemory: makeRegisterMemory(router),
            waveform: makeWaveform(),
            debug: makeDebug(),
            console: makeConsole(),
            memoryMap: makeMemoryMap(),
        });

        const ran = transport.sent.filter(
            (s) => s.message?.payload?.Command && "Continue" in s.message.payload.Command,
        );
        assert.equal(
            ran.length,
            0,
            `装载后不该自动跑，却发了 ${ran.length} 条 ContinueRun`,
        );
    });

    test("装载成功后产品视图显示**文件名**与**字节数**", async () => {
        const { transport } = await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        const router = await makeRouter(transport);
        await app.handleCommand(signals[0], {
            commandRouter: router,
            registerMemory: makeRegisterMemory(router),
            waveform: makeWaveform(),
            debug: makeDebug(),
            console: makeConsole(),
            memoryMap: makeMemoryMap(),
        });

        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(status, "产品视图没有渲染加载状态");
        assert.equal(status.getAttribute("data-image-load-status"), "ok");
        const text = String(status.textContent ?? "");
        assert.ok(text.includes(ELF_NAME), `状态行没有文件名：${text}`);
        assert.ok(
            text.includes(String(ELF_BYTE_LENGTH)),
            `状态行没有字节数：${text}`,
        );
    });

    test("装载失败 ⇒ 显示**原文**（⛔ 不吞）", async () => {
        await setup();
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();

        const boom = new Error("E_LDR-02: 镜像格式不对");
        const failing = {
            commandRouter: {
                dispatchCommand() { throw boom; },
            },
            registerMemory: makeRegisterMemory({ dispatchCommand() { throw boom; } }),
            waveform: makeWaveform(),
            debug: makeDebug(),
            console: makeConsole(),
            memoryMap: makeMemoryMap(),
        };
        // ⚠️ **`loadProjectIntoKernel` 会先调 `registerMemory.loadProject`**
        // ⇒ ⇒ 让它抛 ⇒ ⇒ 错误原文必须是 `E_LDR-02: 镜像格式不对`。
        failing.registerMemory.loadProject = () => { throw boom; };

        await app.handleCommand(signals[0], failing);

        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(status, "产品视图没有渲染加载状态");
        assert.equal(status.getAttribute("data-image-load-status"), "error");
        assert.ok(
            String(status.textContent ?? "").includes("E_LDR-02"),
            `失败状态没有显示内核原文：${status.textContent}`,
        );
    });
});

// ---------------------------------------------------------------------------
// 接线小工具
// ---------------------------------------------------------------------------

/**
 * 造一个命令路由器（记下订阅过哪些主题）。
 *
 * @param {{sent: any[]}} transport
 * @returns {Promise<object>}
 */
async function makeRouter(transport) {
    const router = await m05.mountCommandRouter({ transport });
    return router;
}

/**
 * 造一个 M-10 的替身（记下订阅过的主题）。
 *
 * ⚠️ **发 `LoadProject` 命令的是 M-10 的 `loadProject()`**（真实实现：
 * `front/services/m10_registermemory.js` 的 `await router.dispatchCommand(
 * Command.loadProject(project))`）⇒ ⇒ 替身**必须真的发那条命令**
 * ⇒ ⇒ 否则「线上帧里有一条 `LoadProject`」这条断言**测不到任何东西**
 * （它会永远 0 条 ⇒ ⇒ 假红）。
 *
 * ⚠️ **`loadProject` 自己就要重订寄存器流**（真实 M-10 的 `loadProject()`
 * 内部会做这件事⇒ ⇒ `loadProjectIntoKernel` 才敢传 `skipRegisters: true`）
 * ⇒ ⇒ 替身必须**在这里**记下那两条 ⇒ ⇒ 否则「五条流全重订」那条断言
 * 会**假红**。
 *
 * @param {object} router — 真命令路由器
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

/**
 * 造一个波形接线的替身。
 *
 * @param {string[]} [topics]
 */
function makeWaveform(topics = []) {
    return {
        reset() {},
        async subscribeProbeStream() { topics.push("sim.waveform"); },
    };
}

/**
 * 造一个调试面接线的替身。
 */
function makeDebug() {
    return { reset() {} };
}

/**
 * 造一个 console 接线的替身。
 *
 * @param {string[]} [topics]
 */
function makeConsole(topics = []) {
    return {
        reset() {},
        async subscribeConsoleStream() { topics.push("sim.console"); },
    };
}

/**
 * 造一个内存图接线的替身。
 *
 * @param {string[]} [topics]
 */
function makeMemoryMap(topics = []) {
    return {
        reset() {},
        async subscribeMemoryMapStream() { topics.push("memory.map"); },
    };
}

// ---------------------------------------------------------------------------
// STAGE-33 · v0.48 批 —— 页头那个「加载固件」入口
// ---------------------------------------------------------------------------
//
// ⚠️ **这一段盯的是「用户找不找得到」**，不是「功能有没有做」——STAGE-32 把
// 功能做出来了，但做在**第二个标签页的最下面** ⇒ ⇒ 用户打开页面**根本看不到**
// ⇒ ⇒ 这一段盯的是：**页头常驻** ⇒ ⇒ **默认落地页上就有那个按钮**。
//
// ⚠️ **两个入口共用同一条装载路径** ⇒ ⇒ 这一段还要证明「**不是两条路**」。

/**
 * 把**页头**那个控件挂上去（app 层那件事：`wireTopbarImageLoader`）。
 *
 * ⚠️ **不启动整个 bootstrap**：app.js 的自启动在测试里是关的 ⇒ ⇒ 这里只调
 * 挂载那一个函数 ⇒ ⇒ 「页头有没有入口」这件事**可以单独**被断言。
 *
 * @param {object} services
 */
function mountTopbar(services) {
    app.wireTopbarImageLoader(services);
}

/**
 * 造一套 app 层要用的服务替身。
 *
 * @param {object} router
 * @param {string[]} [topics]
 * @returns {object}
 */
function makeServices(router, topics = []) {
    return {
        commandRouter: router,
        registerMemory: makeRegisterMemory(router, topics),
        waveform: makeWaveform(topics),
        debug: makeDebug(),
        console: makeConsole(topics),
        memoryMap: makeMemoryMap(topics),
    };
}

/**
 * 等一个条件成立（轮询）。
 *
 * ⚠️ **必须等**：装载那条路是 **async**（要 await 内核回包）⇒ ⇒ 点击之后
 * 立刻断言会看到「还没发生」。
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
 * 取线上帧里的 `LoadProject` 载荷。
 *
 * ⚠️ **线上形状** = `payload: { Command: { LoadProject: … } }` ⇒ ⇒ 断言必须
 * 按**这一层**找 ⇒ ⇒ 找错层 ⇒ ⇒ 断言恒 0 条 ⇒ ⇒ 假红。
 *
 * @param {{sent: any[]}} transport
 * @returns {object|null}
 */
function loadProjectPayload(transport) {
    const frame = transport.sent.find(
        (s) => s.message?.payload?.Command && "LoadProject" in s.message.payload.Command,
    );
    return frame ? frame.message.payload.Command.LoadProject : null;
}

/**
 * 走**页头**那条路：选文件 ⇒ ⇒ 点「加载固件」。
 *
 * @param {object} dom
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

/**
 * 点**页头**那个「加载固件」按钮。
 */
function clickTopbarLoad() {
    const btn = dom.document.querySelector('[data-action="load-image"][data-image-load-from="topbar"]');
    assert.ok(btn, "页头没有渲染「加载固件」按钮");
    btn.dispatchEvent(new Event("click"));
}

describe("页头 · 「加载固件」入口（STAGE-33）", () => {
    test("页头渲染出**一个**文件选择器与一个**「加载固件」**按钮", async () => {
        await setup();
        mountTopbar(makeServices({}));
        const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
        assert.ok(input, "页头没有渲染文件选择器");
        assert.equal(String(input.getAttribute("type") ?? ""), "file");
        const btn = dom.document.querySelector('[data-action="load-image"][data-image-load-from="topbar"]');
        assert.ok(btn, "页头没有渲染「加载固件」按钮");
        assert.equal(String(btn.textContent ?? "").trim(), "加载固件");
    });

    test("两个入口的 `data-image-load-from` **互不相同**（门禁的选择器才无歧义）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        const top = dom.document.querySelector('input[data-image-load-from="topbar"]');
        const prod = dom.document.querySelector('input[data-image-load-from="product"]');
        assert.ok(top, "页头没有文件选择器");
        assert.ok(prod, "产品视图没有文件选择器");
        assert.notEqual(
            top.getAttribute("data-image-load-from"),
            prod.getAttribute("data-image-load-from"),
            "两个入口的位置取值必须不同，否则真机门禁会选错输入框",
        );
    });

    test("页头选文件 ⇒ ⇒ 状态条显示**文件名与字节数**（与产品视图那份**同源**）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        const brief = dom.document.querySelector("[data-image-brief]");
        assert.ok(brief, "页头没有渲染那条短状态");
        assert.equal(brief.getAttribute("data-image-brief-status"), "idle");
        const text = String(brief.textContent ?? "");
        assert.ok(text.includes(ELF_NAME), `页头状态条没有文件名：${text}`);
        assert.ok(text.includes(String(ELF_BYTE_LENGTH)), `页头状态条没有字节数：${text}`);
        // ⚠️ **同源**：产品视图那块状态条显示的是**同一个**文件名。
        const status = dom.document.querySelector("[data-image-load-status]");
        assert.ok(
            String(status.textContent ?? "").includes(ELF_NAME),
            "产品视图的状态条与页头不是同一份状态",
        );
    });

    test("页头点「加载固件」⇒ ⇒ **真的** `LoadProject` 上线（⛔ 不是假按钮）", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        mountTopbar(makeServices(router));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        const got = await waitFor(() => loadProjectPayload(transport) !== null);
        assert.ok(got, `页头那条路没有发出 LoadProject（线上帧：${JSON.stringify(transport.sent)}）`);
        const project = loadProjectPayload(transport);
        assert.equal(project.version.isa, "RV32IMC");
        assert.deepEqual(project.images[0].source.Bytes, [...ELF_BYTES]);
        assert.equal(project.images[0].name, ELF_NAME);
        // ⚠️ **必须等「最终状态」**：`LoadProject` 帧是**中途**发出去的 ⇒ ⇒
        // 状态更新（`setImageLoadState` + `publishProductState`）在它**之后**
        // ⇒ ⇒ 只等帧就断言 ⇒ ⇒ 会**竞态**（实测踩过：断言时状态还是 `idle`）。
        const brief = dom.document.querySelector("[data-image-brief]");
        const settled = await waitFor(
            () => brief.getAttribute("data-image-brief-status") === "ok",
        );
        assert.ok(
            settled,
            `页头状态条没有变成 ok（文案：${String(brief.textContent ?? "")}）`,
        );
    });

    test("两个入口装的是**同一个工程**（⇒ ⇒ 走的是**同一条**装载路径）", async () => {
        // -- 产品视图那条 ------------------------------------------------
        const a = await setup();
        const routerA = await makeRouter(a.transport);
        const servicesA = makeServices(routerA);
        const { signals, stop } = captureCommands();
        await selectFile(ELF_NAME, ELF_BYTES);
        clickLoad();
        stop();
        await app.handleCommand(signals[0], servicesA);
        const viaView = loadProjectPayload(a.transport);
        assert.ok(viaView, "产品视图那条路没有发出 LoadProject");

        // -- 页头那条 ------------------------------------------------------
        const b = await setup();
        const routerB = await makeRouter(b.transport);
        mountTopbar(makeServices(routerB));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        await waitFor(() => loadProjectPayload(b.transport) !== null);
        const viaTopbar = loadProjectPayload(b.transport);
        assert.ok(viaTopbar, "页头那条路没有发出 LoadProject");

        // ⇒ ⇒ **逐字段相同** ⇒ ⇒ 两条入口落到**同一条**装载路径上。
        assert.deepEqual(viaTopbar, viaView);
    });

    test("「读字节 + 存 pending」**只有一处实现**（⛔ 视图层不许有第二份）", async () => {
        const src = readFileSync(resolve(frontDir, "views/m02_viewcomponents.js"), "utf8");
        assert.ok(
            !/function\s+pendingImage\s*\(/.test(src),
            "视图层自己又写了一份 pendingImage（⇒ ⇒ 两份实现会漂移）",
        );
        assert.ok(
            !/function\s+loadImageFileIntoPending\s*\(/.test(src),
            "视图层自己又写了一份 loadImageFileIntoPending（⇒ ⇒ 两份实现会漂移）",
        );
        assert.equal(typeof loader.pendingImage, "function");
        assert.equal(typeof loader.loadImageFileIntoPending, "function");
    });

    test("装载命令的处理函数**只有一处**，且页头那条路**必须**走它", async () => {
        const src = readFileSync(resolve(frontDir, "app.js"), "utf8");
        const defs = src.match(/function\s+handleLoadImageCommand\s*\(/g) ?? [];
        assert.equal(defs.length, 1, `handleLoadImageCommand 应恰好定义一次，实测 ${defs.length}`);
        assert.ok(src.includes('case "load-image"'), "命令 switch 里没有 load-image 分支");
        // ⚠️ **页头那条路必须调它** ⇒ ⇒ 否则就是**两条**装载路径 ⇒ ⇒ 之后
        // 「重订流 / 清残留」会分叉。
        const start = src.indexOf("export function wireTopbarImageLoader");
        assert.ok(start > 0, "app.js 里没有 wireTopbarImageLoader");
        const body = src.slice(start, src.indexOf("\n}\n", start));
        assert.ok(
            body.includes("handleLoadImageCommand"),
            "页头那条路没有走 handleLoadImageCommand（⇒ ⇒ 两条装载路径）",
        );
    });

    test("页头点「加载固件」但**没选文件** ⇒ ⇒ 如实显示原因（⛔ 不静默）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        clickTopbarLoad();
        await new Promise((r) => setTimeout(r, 0));
        const brief = dom.document.querySelector("[data-image-brief]");
        assert.equal(
            brief.getAttribute("data-image-brief-status"),
            "error",
            "没选文件时状态应是 error",
        );
        const text = String(brief.textContent ?? "");
        assert.ok(text.includes("没有待加载的文件"), `页头状态条没显示原因：${text}`);
    });
});
