// v0.23 · Lane G —— 「真实 RTOS（osz）的 console 在浏览器里看得见」的门禁。
//
// 这个套件盯三件事：
//
//   1. 契约 v4 的 `Console` 事件（载荷是**字节数组** `[104, 105]`）真的落进
//      终端视图 —— **完整**字节流、ANSI 转义与回车**原样**、HTML **不被解析**、
//      28 万字节**不爆调用栈**；
//   2. 装载工程之后 console 流**被强制重订** —— 真内核的 `LoadProject` 会
//      清掉内核侧的主题集合（新工程 = 新会话，FR-13）⇒ 不重订的话 console
//      推送**静默停掉**，终端永远是空的，页面上却看不出任何异常。
//      ⇒ 这一条用一个**会清空主题集合**的假内核来盯（真内核就是这么干的）；
//   3. 「按 URL 取镜像字节 → 发 `LoadProject`」的入口存在，且字节**不截断**
//      （osz 的 ELF 是 282 280 字节）。

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

// ⚠️ **关掉 `app.js` 的自启动**（与 `waveform.test.mjs` 同一条理由）：
// `bootstrap()` 是**异步**的，它会调 `waveform.reset()` / `console.reset()`
// ——而 M-03 的视图状态是**模块级单例** ⇒ 那条 reset 可能落在**任意两个
// 用例之间**，把上一个用例刚推进 store 的字节清掉。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const message = await importFront("lib/message.js");

// 视图组件的渲染监听器（`app.js` 的 bootstrap 会挂它；这里自启动关掉了）。
await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/**
 * osz 跑到 shell 提示符时 console 上的**那 31 个字节**。
 *
 * 内容是 `==== Enter Main ====\n` + `\x1b[2K` + `\r` + `OSZ$ `
 * （**含** ANSI 转义与回车——终端视图要**原样**显示，不许过滤）。
 */
const OSZ_CONSOLE_TEXT = "==== Enter Main ====\n\x1b[2K\rOSZ$ ";

/** 上面那串文本的**字节数组**形态（契约 v4 的线上形态）。 */
const OSZ_CONSOLE_BYTES = [
    61, 61, 61, 61, 32, 69, 110, 116, 101, 114, 32, 77, 97, 105, 110, 32,
    61, 61, 61, 61, 10, 27, 91, 50, 75, 13, 79, 83, 90, 36, 32,
];

/** osz 的 ELF 有多大（仓外产物，**不进仓库**）。 */
const OSZ_ELF_BYTES = 282280;

// ---------------------------------------------------------------------------
// 假内核
// ---------------------------------------------------------------------------

/**
 * 一个**会清空主题集合**的假内核（学真内核的样子）。
 *
 * ⚠️ **为什么必须学这一条**：真内核的 `LoadProject` 会清掉内核侧的主题集合
 * （新工程 = 新会话，FR-13）⇒ 装载之后不重订，console 推送就**静默停掉**。
 * 一个「什么都不做」的假内核会让这条不变量**恒绿** ⇒ 假内核必须复刻
 * 「装载 ⇒ 清空」这一条，否则门禁守不住任何东西。
 *
 * @returns {object} the router double
 */
function fakeKernel() {
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
                // ⚠️ **真内核的行为**：新工程 = 新会话 ⇒ 主题集合被清空。
                topics.clear();
            } else if (payload && typeof payload === "object" && "Subscribe" in payload) {
                topics.add(payload.Subscribe.topic);
            }
            return { value: {} };
        },
        /**
         * 内核推一个 console 事件。
         *
         * ⚠️ **没订阅就不推**（真内核的 `console_event()` 没订阅就返回
         * `None`）——这一条是「强制重订」那条用例的判别力所在。
         *
         * @param {number[]} bytes
         * @returns {boolean} whether the event was actually pushed
         */
        emitConsole(bytes) {
            if (!topics.has("sim.console")) return false;
            const event = {
                variant: "Console",
                topic: "sim.console",
                value: { bytes },
            };
            for (const fn of subs.get("sim.console") ?? []) fn(event);
            return true;
        },
    };
}

/**
 * 装一套干净的接线（视图状态是**模块级单例**，用例之间共享 ⇒ 每次都重置）。
 *
 * ⚠️ **顺带订阅一次 console 流**：真内核**订阅了** `sim.console` 才会推
 * 字节流（`console_event()` 没订阅就返回 `None`）⇒ 不订阅的话内核**根本
 * 不会推**，用例就测不到「事件落进终端」这一段。
 *
 * @returns {Promise<{router: object, consoleWiring: object}>}
 */
async function setup() {
    m01.assembleWorkspace("embedded");
    m01.route("#/terminal");
    m03.setState("terminal", { term: { bytes: "" } });
    const router = fakeKernel();
    const consoleWiring = app.wireConsole(router);
    await consoleWiring.subscribeConsoleStream();
    return { router, consoleWiring };
}

/**
 * 造一套「服务替身」：寄存器流与探针流是空壳，**只**盯 console 这一路。
 *
 * @param {object} router
 * @returns {object} the services object
 */
function servicesFor(router) {
    return {
        commandRouter: router,
        registerMemory: {
            async loadProject(project) {
                // ⚠️ **真的**发一条 `LoadProject` ⇒ 假内核据此清空主题集合。
                await router.dispatchCommand(message.Command.loadProject(project));
            },
        },
        waveform: {
            reset() {},
            async subscribeProbeStream() {
                // ⚠️ **真的**发一条订阅命令（空壳 stub 会让「订阅命令的顺序」
                // 这条断言恒绿）。
                await router.dispatchCommand(message.Command.subscribe("sim.waveform"));
            },
        },
        console: null, // 由调用方填（`wireConsole` 的产物）
    };
}

/** 终端视图当前显示的文本。 */
function terminalText() {
    const out = $("[data-terminal-out]");
    return out ? String(out.textContent ?? "") : "";
}

// ---------------------------------------------------------------------------

describe("console 事件 · 落进终端视图", () => {
    test("`Console` 事件的字节数组被**完整**解码进终端（`==== Enter Main ====`）", async () => {
        const { router } = await setup();
        assert.equal(router.emitConsole(OSZ_CONSOLE_BYTES), true, "内核该推这个事件");
        assert.equal(
            m03.getState("terminal")?.term?.bytes,
            OSZ_CONSOLE_TEXT,
            "终端缓冲必须是那 31 个字节解出来的**原文**",
        );
        assert.equal(
            terminalText(),
            OSZ_CONSOLE_TEXT,
            "终端视图上显示的必须是同一串原文",
        );
    });

    test("ANSI 转义与回车**原样**显示（不过滤）", async () => {
        const { router } = await setup();
        router.emitConsole(OSZ_CONSOLE_BYTES);
        const text = terminalText();
        assert.ok(text.includes("\x1b[2K"), `ANSI 转义被过滤了：${JSON.stringify(text)}`);
        assert.ok(text.includes("\r"), `回车被过滤了：${JSON.stringify(text)}`);
        assert.ok(text.endsWith("OSZ$ "), `提示符不见了：${JSON.stringify(text)}`);
    });

    test("终端用 `textContent` 写（HTML **不被解析**）", async () => {
        const { router } = await setup();
        // `<b>hi</b>` 的字节：60, 98, 62, 104, 105, 60, 47, 98, 62
        router.emitConsole([60, 98, 62, 104, 105, 60, 47, 98, 62]);
        const text = terminalText();
        assert.equal(text, "<b>hi</b>", "终端里必须是字面文本");
        assert.ok(!text.includes("&lt;"), "出现了字面 &lt;（说明被转义过了）");
        const body = $('[data-view-body="terminal"]');
        assert.equal(
            body.querySelectorAll("b").length,
            0,
            "标签被解析成了元素 —— 终端必须用 textContent 写（NFR-04）",
        );
    });

    test("28 万字节**不爆调用栈**且一个字节都不丢（分块解码）", async () => {
        // ⚠️ **判别力**：`String.fromCharCode(...bytes)` 不分块的话，28 万个
        // 实参会**爆调用栈**（RangeError）⇒ 这条用例会直接抛错变红。
        const { router } = await setup();
        const big = new Array(OSZ_ELF_BYTES).fill(65); // 'A'
        router.emitConsole(big);
        const text = terminalText();
        assert.equal(text.length, OSZ_ELF_BYTES, "字节数必须一个不少");
        assert.equal(text[0], "A");
        assert.equal(text[OSZ_ELF_BYTES - 1], "A");
    });

    test("订阅的是 `sim.console` 主题（与 `EVENT_TOPIC.Console` 逐字一致）", async () => {
        assert.equal(
            message.EVENT_TOPIC.Console,
            "sim.console",
            "契约镜像里的 topic 变了",
        );
        const { router, consoleWiring } = await setup();
        await consoleWiring.subscribeConsoleStream();
        const subscribes = router.sent.filter(
            (c) => c && typeof c === "object" && "Subscribe" in c,
        );
        assert.ok(subscribes.length >= 1, "必须真的发过订阅命令");
        assert.ok(
            subscribes.every((c) => c.Subscribe.topic === "sim.console"),
            `订阅的主题不对：${JSON.stringify(subscribes)}`,
        );
        assert.deepEqual(
            subscribes[subscribes.length - 1],
            { Subscribe: { topic: "sim.console" } },
        );
    });

    test("`onDelta` 路径真的被走到了（`termDelta` 不再是死接口）", async () => {
        // ⚠️ 此前 `termDelta` 在前端源码里**只被读、从来没被写** ⇒ 终端视图
        // 是个死接口，契约 v4 的 `Console` 事件到了也没人接。
        const { router } = await setup();
        const before = m03.getState("terminal")?.term?.bytes ?? "";
        router.emitConsole(OSZ_CONSOLE_BYTES);
        const after = m03.getState("terminal")?.term?.bytes ?? "";
        assert.notEqual(after, before, "console 事件必须真的推进终端缓冲");
    });
});

// ---------------------------------------------------------------------------

describe("console 流 · 装载工程后的强制重订", () => {
    test("装载工程后 console **仍然收得到字节**（强制重订）", async () => {
        // ⚠️ **这条是本 lane 最核心的不变量**：真内核的 `LoadProject` 清掉了
        // 主题集合 ⇒ 装载之后不重订，内核就**不再推** console ⇒ 终端**永远**
        // 是空的，而页面上看不出任何异常（与 D-45 / D-46 同一个坑）。
        const { router, consoleWiring } = await setup();
        assert.equal(router.topics.has("sim.console"), true, "先订阅上了");

        const services = servicesFor(router);
        services.console = consoleWiring;
        await app.loadProjectIntoKernel(services, { id: "p" });

        // 装载之后：主题集合被清空过 ⇒ **必须**被重订回来。
        assert.equal(
            router.topics.has("sim.console"),
            true,
            "装载工程后 console 流没有被强制重订 —— 内核不会再推任何字节",
        );
        assert.equal(router.emitConsole(OSZ_CONSOLE_BYTES), true, "内核该推这个事件");
        assert.equal(
            m03.getState("terminal")?.term?.bytes,
            OSZ_CONSOLE_TEXT,
            "装载之后 console 一个字节都收不到",
        );
    });

    test("console 的订阅发生在 `LoadProject` **之后**", async () => {
        const { router, consoleWiring } = await setup();
        const services = servicesFor(router);
        services.console = consoleWiring;
        const before = router.sent.length;
        await app.loadProjectIntoKernel(services, { id: "p" });
        const kinds = router.sent.slice(before).map((c) => {
            if (c && typeof c === "object" && "LoadProject" in c) return "LoadProject";
            if (c && typeof c === "object" && "Subscribe" in c) return c.Subscribe.topic;
            return String(c);
        });
        assert.deepEqual(
            kinds,
            ["LoadProject", "sim.waveform", "sim.console"],
            "console 的订阅必须**在装载之后** —— 装载前的订阅会被一起清掉",
        );
    });

    test("console 流重订失败**不牵连**工程装载（报出来，不抛）", async () => {
        const { router } = await setup();
        const services = servicesFor(router);
        services.console = {
            reset() {},
            async subscribeConsoleStream() {
                throw new Error("内核不回");
            },
        };
        const result = await app.loadProjectIntoKernel(services, { id: "p" });
        assert.ok(result.consoleError instanceof Error, "失败必须**报出来**（不静默）");
        assert.match(String(result.consoleError?.message), /内核不回/);
    });

    test("装载工程**必须**带 console 接线（漏了当场报错，不静默）", async () => {
        // ⚠️ 漏传 console 接线 ⇒ 终端永远是空的，而页面上看不出任何异常 ⇒
        // 这是最难查的一类故障 ⇒ 宁可当场抛错。
        const { router } = await setup();
        const services = servicesFor(router);
        await assert.rejects(
            () => app.loadProjectIntoKernel(services, { id: "p" }),
            /console/,
        );
    });
});

// ---------------------------------------------------------------------------

describe("装载仓外镜像 · 按 URL 取字节", () => {
    /** 装一个假的 `fetch`（Node 的 `fetch` 是全局的 ⇒ 换掉它）。 */
    function withFetch(impl, fn) {
        const original = globalThis.fetch;
        globalThis.fetch = impl;
        try {
            return fn();
        } finally {
            globalThis.fetch = original;
        }
    }

    test("按 URL 取字节 → 发 `LoadProject`（`source` 是 `Bytes` 字节数组）", async () => {
        const router = fakeKernel();
        const payload = new Uint8Array([0x7f, 0x45, 0x4c, 0x46]);
        await withFetch(
            async () => ({ ok: true, status: 200, arrayBuffer: async () => payload.buffer }),
            async () => {
                const services = servicesFor(router);
                services.console = { reset() {}, async subscribeConsoleStream() {} };
                const result = await app.loadImageFromUrl(
                    services,
                    "http://127.0.0.1:9999/osz.elf",
                );
                assert.equal(result.byteLength, 4, "字节数必须如实报出来");
                const load = router.sent.find(
                    (c) => c && typeof c === "object" && "LoadProject" in c,
                );
                assert.ok(load, "必须真的发一条 LoadProject");
                const image = load.LoadProject.images[0];
                assert.deepEqual(image.source, { Bytes: [0x7f, 0x45, 0x4c, 0x46] });
                assert.equal(image.kind, "Elf");
                assert.equal(image.name, "osz.elf");
                assert.equal(load.LoadProject.version.isa, "RV32IMC");
            },
        );
    });

    test("镜像字节**不被截断**（osz 的 ELF 是 282 280 字节）", async () => {
        const router = fakeKernel();
        const payload = new Uint8Array(OSZ_ELF_BYTES);
        payload[0] = 0x7f;
        payload[OSZ_ELF_BYTES - 1] = 0x46;
        await withFetch(
            async () => ({ ok: true, status: 200, arrayBuffer: async () => payload.buffer }),
            async () => {
                const services = servicesFor(router);
                services.console = { reset() {}, async subscribeConsoleStream() {} };
                const result = await app.loadImageFromUrl(
                    services,
                    "http://127.0.0.1:9999/osz.elf",
                );
                assert.equal(result.byteLength, OSZ_ELF_BYTES);
                const load = router.sent.find(
                    (c) => c && typeof c === "object" && "LoadProject" in c,
                );
                const bytes = load.LoadProject.images[0].source.Bytes;
                assert.equal(bytes.length, OSZ_ELF_BYTES, "字节被截断了");
                assert.equal(bytes[0], 0x7f, "首字节不对");
                assert.equal(bytes[OSZ_ELF_BYTES - 1], 0x46, "末字节不对");
            },
        );
    });

    test("取字节失败时报错（不静默）", async () => {
        await withFetch(
            async () => ({ ok: false, status: 404, arrayBuffer: async () => new ArrayBuffer(0) }),
            async () => {
                await assert.rejects(
                    () => app.fetchImageBytes("http://127.0.0.1:9999/missing.elf"),
                    /404/,
                );
            },
        );
    });
});

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------

describe("端到端 · 装载 → 跑 → 终端里看得见 console", () => {
    test("整条链走通：按 URL 取字节 → LoadProject → 跑到提示符 → 终端显示", async () => {
        // ⚠️ 这一条把三段接在一起（装载入口 / 强制重订 / 跑到前缀）——单段
        // 的用例各自绿，但**接起来断掉**的情况它们抓不到。
        const router = fakeKernel();
        const consoleWiring = app.wireConsole(router);
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        m03.setState("terminal", { term: { bytes: "" } });

        // 假内核：guest 跑够两批之后才把提示符打到 UART 上。
        let batches = 0;
        const realDispatch = router.dispatchCommand.bind(router);
        router.dispatchCommand = async (payload, opts) => {
            const out = await realDispatch(payload, opts);
            if (payload === "Continue") {
                batches += 1;
                if (batches >= 2) router.emitConsole(OSZ_CONSOLE_BYTES);
            }
            return out;
        };

        // 镜像字节（假的 ELF 头就够——这一条不解析镜像，只走「取字节 → 装载」）。
        const payload = new Uint8Array([0x7f, 0x45, 0x4c, 0x46]);
        const originalFetch = globalThis.fetch;
        globalThis.fetch = async () => ({
            ok: true,
            status: 200,
            arrayBuffer: async () => payload.buffer,
        });
        let result;
        try {
            const services = servicesFor(router);
            services.console = consoleWiring;
            await app.loadImageFromUrl(services, "http://127.0.0.1:9999/osz.elf");
            result = await app.runUntilConsole(
                { commandRouter: router },
                "OSZ$ ",
                { timeoutMs: 5000 },
            );
        } finally {
            globalThis.fetch = originalFetch;
        }

        assert.equal(result.matched, true, `没匹配上：${JSON.stringify(result.text)}`);
        assert.equal(result.text, OSZ_CONSOLE_TEXT, "终端里必须是 osz 那 31 个字节");
        assert.equal(
            m03.getState("terminal")?.term?.bytes,
            OSZ_CONSOLE_TEXT,
            "视图状态里的终端缓冲也必须一致",
        );
        assert.equal(terminalText(), OSZ_CONSOLE_TEXT, "终端视图上显示的必须一致");
    });
});

// ---------------------------------------------------------------------------

describe("runUntilConsole · 跑到某个 console 前缀", () => {
    test("一条一条发 `Continue`，见到目标字面量就停", async () => {
        const { router } = await setup();
        let batches = 0;
        const realDispatch = router.dispatchCommand.bind(router);
        router.dispatchCommand = async (payload, opts) => {
            const out = await realDispatch(payload, opts);
            if (payload === "Continue") {
                batches += 1;
                // 第一批：只有半个提示符；第二批：完整的提示符。
                router.emitConsole(
                    batches === 1
                        ? [79, 83, 90] // "OSZ"
                        : [36, 32], // "$ "
                );
            }
            return out;
        };
        const result = await app.runUntilConsole(
            { commandRouter: router },
            "OSZ$ ",
            { timeoutMs: 5000 },
        );
        assert.equal(result.matched, true, `没匹配上：${JSON.stringify(result.text)}`);
        assert.equal(result.batches, 2, "必须**一条一条**地发，不能并发");
        assert.equal(result.text, "OSZ$ ");
        assert.equal(
            router.sent.filter((c) => c === "Continue").length,
            2,
            "只该发两条 Continue",
        );
    });

    test("到点还没见到就**如实**说没匹配上（不假装成功）", async () => {
        const { router } = await setup();
        const result = await app.runUntilConsole(
            { commandRouter: router },
            "OSZ$ ",
            { timeoutMs: 5000, maxBatches: 2 },
        );
        assert.equal(result.matched, false);
        assert.equal(result.batches, 2, "跑满了批数上限");
    });
});
