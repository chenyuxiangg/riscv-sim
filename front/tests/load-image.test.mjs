// STAGE-32 · v0.47 批 → **v0.58 批** —— 「界面上加载固件」的前端门禁（A 组）。
//
// 这个套件盯**一条用户能自己走通的路**：
//
//   选一个文件 ⇒ ⇒ 读出字节 ⇒ ⇒ 点「加载固件」⇒ ⇒ 真的发一条 `LoadProject`
//   ⇒ ⇒ 装载后**五条流全部重订** ⇒ ⇒ **装完就停**（⛔ 不自动跑）。
//
// ⚠️ **核心不变式**（与源码视图那条老规矩**正好相反**）：
//   源码树**不进内核**（STAGE-7 裁决 E）；而镜像**必须**进内核（否则没东西
//   可跑）⇒ ⇒ 两条路的语义**不同** ⇒ ⇒ 本批**不许**把镜像也做成「只进
//   前端」。
//
// ⚠️ **「装完就停」也是不变式**：加载固件**不**自动跑 ⇒ ⇒ 用户自己点
// 「继续」⇒ ⇒ 这条一旦破掉，「加载」就变成了「加载并跑」，用户会吓一跳。
//
// ⚠️ **v0.58 批补的两条**（用户直接反馈的缺陷）：
//   ① **入口只有一个** ⇒ ⇒ 界面上**不许**再出现第二个「加载」入口
//      （曾经有两个 ⇒ ⇒ 用户在**一处**装完、到**另一处**再点 ⇒ ⇒ 撞上
//      「没有待加载的文件」⇒ ⇒ 而那次装载**其实成功了**）。
//   ② **连点两次不许报假失败** ⇒ ⇒ 装成功之后再点一下 ⇒ ⇒ 状态**仍是
//      `ok`** ⇒ ⇒ ⛔ **不许**变成 `error`（界面在骗人）。
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
 * 在**页头**那个入口里「选一个文件」—— 走**真实**的 `change` 事件路径。
 *
 * ⚠️ **必须走 `change`**：浏览器把选中的文件**只**交给 `change`（点击时
 * `input.files` 是空的）⇒ ⇒ 用例必须复现**那条**路 ⇒ ⇒ 否则测的是另一
 * 条路。
 *
 * @param {string} name — 文件名
 * @param {number[]} bytes — 文件字节
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
    const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
    assert.ok(input, "页头没有渲染文件选择器");
    input.files = [{
        name,
        arrayBuffer: () => Promise.reject(new Error("模拟的读取失败")),
    }];
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
 * 数一数线上帧里有几条 `LoadProject`。
 *
 * @param {{sent: any[]}} transport
 * @returns {number}
 */
function loadProjectCount(transport) {
    return transport.sent.filter(
        (s) => s.message?.payload?.Command && "LoadProject" in s.message.payload.Command,
    ).length;
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
 * 读产品视图那块**状态块**（`data-image-load-status`）。
 *
 * ⚠️ **v0.58 批**：那块面板**不再有入口** ⇒ ⇒ 它只显示「当前固件」的状态。
 *
 * @returns {{status: string|null, text: string}}
 */
function productStatus() {
    const status = dom.document.querySelector("[data-image-load-status]");
    return {
        status: status ? String(status.getAttribute("data-image-load-status") ?? "") : null,
        text: status ? String(status.textContent ?? "") : "",
    };
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

describe("点「加载固件」· **四格判定**（v0.58 批）", () => {
    test("没选过文件 ⇒ ⇒ 判成「真的没文件」（⇒ ⇒ 那一格才报错）", () => {
        assert.equal(loader.imageLoadDecision({ status: "idle" }).kind, "no-file");
        assert.equal(loader.imageLoadDecision(null).kind, "no-file");
    });

    test("有待加载的文件 ⇒ ⇒ 判成「装它」", () => {
        const pending = { bytes: [...ELF_BYTES], name: ELF_NAME, kind: "Elf" };
        const decision = loader.imageLoadDecision({ status: "idle", pending });
        assert.equal(decision.kind, "load");
        assert.deepEqual(decision.pending, pending);
    });

    test("⛔ **装成功过** ⇒ ⇒ 判成「已装载」（**不是**「没文件」！）", () => {
        // ⚠️ **这一格就是用户报的缺陷**：装完再点一下 ⇒ ⇒ 上次**成功**了 ⇒ ⇒
        // ⛔ **不许**落进「没有待加载的文件」那一格 ⇒ ⇒ 否则界面在骗人。
        const decision = loader.imageLoadDecision({
            status: "ok",
            name: ELF_NAME,
            byteLength: ELF_BYTE_LENGTH,
            pending: null,
        });
        assert.equal(decision.kind, "already-loaded");
        assert.equal(decision.name, ELF_NAME);
    });

    test("正在装载 ⇒ ⇒ 判成「忽略」（**幂等** ⇒ ⇒ 连点两下只发一条命令）", () => {
        // ⚠️ **判定顺序是硬约束**：`loading` **先**判 ⇒ ⇒ 否则在途时还看得到
        // `pending`（成功前它一直在）⇒ ⇒ 于是会**再发一条** `LoadProject`。
        const decision = loader.imageLoadDecision({
            status: "loading",
            pending: { bytes: [...ELF_BYTES], name: ELF_NAME, kind: "Elf" },
        });
        assert.equal(decision.kind, "loading");
    });
});

describe("点「加载固件」· **「读取中」那一格**（v0.59 批 · 竞态）", () => {
    test("正在读那个文件 ⇒ ⇒ 判成「读取中」（⛔ **不是**「已装载」）", () => {
        // ⚠️ **这一格就是 v0.59 批实测撞到的那个竞态**：读字节是**异步**的 ⇒ ⇒
        // 读取期间状态里**还留着上一次的** `"ok"` ⇒ ⇒ 若不先判「读取中」⇒ ⇒
        // 用户刚选的新文件那一下**会被吞掉**（界面说「已装载，无需重复加载」，
        // 而固件**根本没装**⇒ ⇒ 真机门禁实测偶发「PC 没回去」）。
        const decision = loader.imageLoadDecision({
            status: "reading",
            name: ELF_NAME,
            byteLength: ELF_BYTE_LENGTH,
            pending: null,
        });
        assert.equal(decision.kind, "reading");
    });

    test("⛔ **读取中** ⇒ ⇒ 即便还留着**旧的** `pending`，也**不**判成「装它」", () => {
        // ⚠️ 上一次装载**失败**时 `pending` 是**保留**的（用户点一下就能重试）
        // ⇒ ⇒ 读取期间它还是**旧的**那份 ⇒ ⇒ 先判 `pending` 就会拿**旧字节**
        // 去装 ⇒ ⇒ 那是另一个更隐蔽的错。
        const decision = loader.imageLoadDecision({
            status: "reading",
            pending: { bytes: [...ELF_BYTES], name: ELF_NAME, kind: "Elf" },
        });
        assert.equal(decision.kind, "reading");
    });

    test("状态条在读取期间说「正在读取固件…」（⛔ **不说**「已装载」）", () => {
        const short = loader.describeImageLoadShort({ status: "reading" });
        assert.equal(short, "正在读取固件…");
        assert.ok(!short.includes("已装载"), `读取期间状态条不该说「已装载」：${short}`);
        const long = loader.describeImageLoad({ status: "reading" });
        assert.ok(long.includes("正在读取"), `长版状态说明没有「正在读取」：${long}`);
    });

    test("⛔ **「读取中」必须在字节到手之前写进状态**（否则那一格永远不出现）", async () => {
        await setup();
        // ⚠️ 一个**慢**读取的文件：`arrayBuffer()` 何时 resolve 由**我们**决定
        // ⇒ ⇒ 于是能在「字节还没到」的那一刻检查状态。
        let release = null;
        const slowFile = {
            name: ELF_NAME,
            size: ELF_BYTE_LENGTH,
            arrayBuffer: () => new Promise((res) => {
                release = () => res(new Uint8Array(ELF_BYTES).buffer);
            }),
        };
        const pending = loader.loadImageFileIntoPending(slowFile);
        // ⚠️ **此刻字节还没到** ⇒ ⇒ 状态必须已经是「读取中」。
        const during = m03.getState("product")?.imageLoad;
        assert.equal(
            during?.status,
            "reading",
            `读取期间状态应是 reading，实测 ${JSON.stringify(during)}`,
        );
        assert.ok(
            !loader.describeImageLoadShort(during).includes("已装载"),
            "读取期间状态条不该说「已装载」",
        );
        // ⚠️ **读完之后** ⇒ ⇒ 状态回到 `idle` 且 `pending` 就绪 ⇒ ⇒ 那才是
        // 「可以点加载」的那一刻。
        release();
        await pending;
        const after = m03.getState("product")?.imageLoad;
        assert.equal(after?.status, "idle", `读完之后状态应是 idle，实测 ${JSON.stringify(after)}`);
        assert.ok(after?.pending, "读完之后 pending 应当就绪");
        assert.equal(loader.imageLoadDecision(after).kind, "load");
    });
});

describe("加载入口 · **只有一个**（v0.58 批）", () => {
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

    test("⚠️ **全页面只有那一个**入口（⛔ 不许再有第二个）", async () => {
        // ⚠️ **用户定案「只应该有一个入口」** ⇒ ⇒ 这条判据把它**钉住** ⇒ ⇒
        // 将来谁再加一个入口 ⇒ ⇒ 门禁**当场变红**。
        await setup();
        mountTopbar(makeServices({}));
        const inputs = dom.document.querySelectorAll("[data-image-input]");
        assert.equal(inputs.length, 1, `全页面的文件选择器应恰好一个，实测 ${inputs.length}`);
        const btns = dom.document.querySelectorAll('[data-action="load-image"]');
        assert.equal(btns.length, 1, `全页面的加载按钮应恰好一个，实测 ${btns.length}`);
    });

    test("产品视图里**没有**文件选择器与加载按钮（那块面板**只显示状态**）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        const panel = dom.document.querySelector('[data-view-body="product"]');
        assert.ok(panel, "产品视图没渲染出来");
        assert.equal(
            panel.querySelectorAll("[data-image-input]").length,
            0,
            "产品视图里还有文件选择器（⇒ ⇒ 入口有两个）",
        );
        assert.equal(
            panel.querySelectorAll('[data-action="load-image"]').length,
            0,
            "产品视图里还有加载按钮（⇒ ⇒ 入口有两个）",
        );
        // ⚠️ **但状态块还在** ⇒ ⇒ 「当前固件是什么 / 上次装载成没成」**仍然
        // 看得见** ⇒ ⇒ 撤掉入口**不等于**撤掉反馈。
        assert.ok(panel.querySelector("[data-image-load-status]"), "产品视图里没有加载状态块");
    });

    test("选择器接受 `.elf` / `.hex` / `.bin`", async () => {
        await setup();
        mountTopbar(makeServices({}));
        const input = dom.document.querySelector('input[data-image-load-from="topbar"]');
        const accept = String(input.getAttribute("accept") ?? "");
        for (const ext of [".elf", ".hex", ".bin"]) {
            assert.ok(
                accept.includes(ext),
                `accept 属性缺 ${ext}（实测 "${accept}"）`,
            );
        }
    });
});

describe("页头 · 选文件 ⇒ ⇒ 真的装载", () => {
    test("选文件后**不发命令**（字节先读好，按钮才发）", async () => {
        const { transport } = await setup();
        mountTopbar(makeServices({}));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        assert.equal(
            transport.sent.length,
            0,
            `选文件不该发命令，却发了 ${JSON.stringify(transport.sent)}`,
        );
    });

    test("选完文件 ⇒ ⇒ 状态条显示**文件名与字节数**（选了什么看得见）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        const brief = topbarBrief();
        assert.equal(brief.status, "idle", "选完文件不该是 error");
        assert.ok(brief.text.includes(ELF_NAME), `页头状态条没有文件名：${brief.text}`);
        assert.ok(brief.text.includes(String(ELF_BYTE_LENGTH)), `页头状态条没有字节数：${brief.text}`);
        // ⚠️ **同源**：产品视图那块状态块显示的是**同一个**文件名。
        const status = productStatus();
        assert.ok(
            status.text.includes(ELF_NAME),
            `产品视图的状态块与页头不是同一份状态：${status.text}`,
        );
    });

    test("文件**读不出来** ⇒ ⇒ 状态显示**原因**（⛔ 不静默）", async () => {
        await setup();
        mountTopbar(makeServices({}));
        await selectUnreadableFile();
        const brief = topbarBrief();
        assert.equal(brief.status, "error", "读文件失败时状态应是 error");
        assert.ok(brief.text.includes("模拟的读取失败"), `状态条没显示失败原因：${brief.text}`);
    });

    test("点「加载固件」⇒ ⇒ **真的** `LoadProject` 上线（⛔ 不是假按钮）", async () => {
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
        const settled = await waitFor(() => topbarBrief().status === "ok");
        assert.ok(
            settled,
            `页头状态条没有变成 ok（文案：${JSON.stringify(topbarBrief().text)}）`,
        );
    });

    test("装载后**五条流全部重订**", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        const topics = [];
        mountTopbar(makeServices(router, topics));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        await waitFor(() => topics.length >= EXPECTED_STREAM_COUNT);

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
        const router = await makeRouter(transport);
        mountTopbar(makeServices(router));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        await waitFor(() => loadProjectCount(transport) > 0);
        await waitFor(() => topbarBrief().status === "ok");

        const ran = transport.sent.filter(
            (s) => s.message?.payload?.Command && "Continue" in s.message.payload.Command,
        );
        assert.equal(
            ran.length,
            0,
            `装载后不该自动跑，却发了 ${ran.length} 条 ContinueRun`,
        );
    });

    test("装载成功后状态显示**文件名**与**字节数**", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        mountTopbar(makeServices(router));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        await waitFor(() => productStatus().status === "ok");

        const status = productStatus();
        assert.equal(status.status, "ok");
        assert.ok(status.text.includes(ELF_NAME), `状态行没有文件名：${status.text}`);
        assert.ok(
            status.text.includes(String(ELF_BYTE_LENGTH)),
            `状态行没有字节数：${status.text}`,
        );
    });

    test("装载失败 ⇒ 显示**原文**（⛔ 不吞）", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        const services = makeServices(router);
        mountTopbar(services);
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);

        const boom = new Error("E_LDR-02: 镜像格式不对");
        // ⚠️ **`loadProjectIntoKernel` 会先调 `registerMemory.loadProject`**
        // ⇒ ⇒ 让它抛 ⇒ ⇒ 错误原文必须是 `E_LDR-02: 镜像格式不对`。
        services.registerMemory.loadProject = () => { throw boom; };

        clickTopbarLoad();
        await waitFor(() => productStatus().status === "error");

        const status = productStatus();
        assert.equal(status.status, "error");
        assert.ok(
            status.text.includes("E_LDR-02"),
            `失败状态没有显示内核原文：${status.text}`,
        );
    });

    test("装载失败 ⇒ ⇒ **再点一次能重试**（⛔ 不是「没有待加载的文件」）", async () => {
        // ⚠️ **根因链的一环**：失败时**清掉** `pending` ⇒ ⇒ 用户想重试同一份
        // 文件 ⇒ ⇒ 却撞上「没有待加载的文件」⇒ ⇒ 得**重新选一遍**文件。
        const { transport } = await setup();
        const router = await makeRouter(transport);
        const services = makeServices(router);
        mountTopbar(services);
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);

        let failed = false;
        const boom = new Error("E_LDR-02: 镜像格式不对");
        const real = services.registerMemory.loadProject.bind(services.registerMemory);
        services.registerMemory.loadProject = async (project) => {
            if (!failed) {
                failed = true;
                throw boom;
            }
            return real(project);
        };

        clickTopbarLoad();
        await waitFor(() => productStatus().status === "error");
        assert.equal(loadProjectCount(transport), 0, "第一次就该失败，不该上线");

        // ⚠️ **再点一次** ⇒ ⇒ 同一份文件**重试** ⇒ ⇒ 真的发出 `LoadProject`。
        clickTopbarLoad();
        const retried = await waitFor(() => loadProjectCount(transport) === 1);
        assert.ok(
            retried,
            `失败后再点一次没有重试（线上帧：${JSON.stringify(transport.sent)}）`,
        );
        await waitFor(() => productStatus().status === "ok");
    });
});

describe("连点两次 · **不许**报假失败（用户报的缺陷）", () => {
    test("装成功后再点一次 ⇒ ⇒ 状态**仍是 ok**（⛔ 不许变成 error）", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        mountTopbar(makeServices(router));
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);

        // -- 第一次：真的装上 ------------------------------------------------
        clickTopbarLoad();
        await waitFor(() => topbarBrief().status === "ok");
        assert.equal(loadProjectCount(transport), 1, "第一次点击没有真的装载");

        // -- 第二次：**不许**报假失败 ----------------------------------------
        clickTopbarLoad();
        await new Promise((r) => setTimeout(r, 20));

        const brief = topbarBrief();
        assert.equal(
            brief.status,
            "ok",
            `第二次点击把状态变成了 ${JSON.stringify(brief.status)}：${brief.text}`,
        );
        assert.ok(
            !brief.text.includes("装载失败"),
            `第二次点击报了失败：${brief.text}`,
        );
        assert.ok(
            !brief.text.includes("没有待加载的文件"),
            `第二次点击报了「没有待加载的文件」：${brief.text}`,
        );
        const status = productStatus();
        assert.equal(status.status, "ok", `产品视图的状态块也变成 ${status.status}`);
        assert.ok(
            !status.text.includes("装载失败"),
            `产品视图的状态块报了失败：${status.text}`,
        );
        // ⚠️ **也不许把上一份字节又发一遍** ⇒ ⇒ 线上仍然只有**一条**
        // `LoadProject`。
        assert.equal(
            loadProjectCount(transport),
            1,
            `第二次点击又发了一条 LoadProject（线上 ${loadProjectCount(transport)} 条）`,
        );
    });

    test("连点两下（在途）⇒ ⇒ **只**发一条 `LoadProject`（幂等）", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        const services = makeServices(router);

        // ⚠️ **让第一次装载卡在「等回包」那一步** ⇒ ⇒ 第二下点在「在途」那一格
        // ⇒ ⇒ 判据才有判别力（不然两次点击都是「已完成」，测的是别的东西）。
        //
        // ⚠️ **帧是同步发出去的**（`dispatchCommand` 里的 `sendCommand` 是同步
        // 调用）⇒ ⇒ 所以「在途」期间**线上已经有那条 `LoadProject`** ⇒ ⇒
        // 判据可以数**线上帧**（不是数内部调用次数 ⇒ ⇒ 不钉实现细节）。
        let release = null;
        const gate = new Promise((resolve) => { release = resolve; });
        const real = services.registerMemory.loadProject.bind(services.registerMemory);
        services.registerMemory.loadProject = async (project) => {
            const inflight = real(project);
            await gate;
            return inflight;
        };

        mountTopbar(services);
        await selectFileInTopbar(ELF_NAME, ELF_BYTES);
        clickTopbarLoad();
        clickTopbarLoad();
        await new Promise((r) => setTimeout(r, 20));

        assert.equal(
            loadProjectCount(transport),
            1,
            `连点两下发了 ${loadProjectCount(transport)} 条 LoadProject（内核被连着换两次工程）`,
        );
        release();
        await waitFor(() => topbarBrief().status === "ok");
    });

    test("真的没选过文件就点 ⇒ ⇒ 报「没有待加载的文件」（⛔ 那一格才报错）", async () => {
        const { transport } = await setup();
        const router = await makeRouter(transport);
        mountTopbar(makeServices(router));
        clickTopbarLoad();
        await new Promise((r) => setTimeout(r, 0));

        const brief = topbarBrief();
        assert.equal(brief.status, "error", "没选文件时状态应是 error");
        assert.ok(brief.text.includes("没有待加载的文件"), `状态条没显示原因：${brief.text}`);
        assert.equal(loadProjectCount(transport), 0, "没选文件时不该发命令");
    });
});

describe("实现唯一性（⛔ 不许有第二份）", () => {
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

    test("⛔ 视图层**不再**有 `load-image` 动作（⇒ ⇒ 入口只有一个）", async () => {
        const src = readFileSync(resolve(frontDir, "views/m02_viewcomponents.js"), "utf8");
        assert.ok(
            !src.includes('emitCommand("load-image"'),
            "视图层还在发 load-image 信号（⇒ ⇒ 入口不止一个）",
        );
        assert.ok(
            !src.includes('data-action="load-image"'),
            "视图层还在渲染加载按钮（⇒ ⇒ 入口不止一个）",
        );
    });
});
