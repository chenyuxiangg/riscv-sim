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
    const input = dom.document.querySelector("[data-image-input]");
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
    const input = dom.document.querySelector("[data-image-input]");
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
    const btn = dom.document.querySelector("[data-action='load-image']");
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
        const inputs = dom.document.querySelectorAll("[data-image-input]");
        assert.equal(inputs.length, 1, `文件选择器应恰好一个，实测 ${inputs.length}`);
        const btns = dom.document.querySelectorAll("[data-action='load-image']");
        assert.equal(btns.length, 1, `「加载」按钮应恰好一个，实测 ${btns.length}`);
    });

    test("选择器接受 `.elf` / `.hex` / `.bin`", async () => {
        await setup();
        const input = dom.document.querySelector("[data-image-input]");
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
