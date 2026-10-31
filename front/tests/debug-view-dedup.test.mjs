// 调试视图**去重**批的前端门禁（v0.63 批）。
//
// ⚠️ **本批的主题是「界面上的重复按钮」**：用户直接提的 —— 「UI 界面上调试
//   视图有很多按钮与顶部栏功能重复，需要删除调试视图里重复的按钮」⇒ ⇒
//   同一个动作在**两处**出现 ⇒ ⇒ 用户得猜「哪个才是真的」⇒ ⇒ 本套件盯住
//   **去重之后的那张分布图**。
//
// ⚠️ **分布图就是判据**（不是「某个按钮还在不在」）：
//
//   | 动作 | 落点 |
//   | --- | --- |
//   | 运行 / 单步 / 暂停 / 复位 | **顶栏**（全局运行条） |
//   | 读寄存器 | **寄存器/内存视图** |
//   | 反汇编 / 按步数单步 | **调试视图的反汇编卡片** |
//   | 导出波形 (VCD) | **波形视图** |
//   | trace 记录开关 / 下载轨迹 | **调试视图的 trace 卡片** |
//
// ⇒ ⇒ 每条判据都**双向**断言（「那边**没有**」＋「这边**有**」）⇒ ⇒ 只断言
//   一半的话，「按钮整个不见了」也会绿 ⇒ ⇒ 那是**空转**的判据。
//
// ⚠️ **顶栏那个按钮的文案**（用户拍板「当加载之后顶栏的运行应该显示运行，
//   点击停之后需要切换为继续」）⇒ ⇒ 「运行 / 继续」是**同一个动作**的两种
//   说法 ⇒ ⇒ 按钮上必须**说出当下是哪种**。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m05 = await importFront("services/m05_commandrouter.js");
const { Command, eventMessage } = await importFront("lib/message.js");
const { createMockKernel } = await importFront("dev/mock-kernel.js");
const { normalizeStepCount, runActionLabel } = await importFront("lib/run-action.js");

await m02.mountViewComponents();

/**
 * An **in-process** transport: the mock kernel answers synchronously ⇒ ⇒
 * 整条链路在一个 tick 内。
 *
 * @param {object} kernel
 */
function inProcessTransport(kernel) {
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
            const frames = kernel.handle({
                type: "command",
                seq: message.seq,
                sessionId: "sess-test",
                role: "embedded",
                reqId,
                payload: message,
            });
            for (const frame of frames) {
                for (const fn of [...handlers]) fn(frame);
            }
        },
        emit(payload) {
            const frame = {
                type: "event",
                seq: 1,
                sessionId: "sess-test",
                role: "embedded",
                reqId: null,
                payload: eventMessage(1, payload),
            };
            for (const fn of [...handlers]) fn(frame);
        },
    };
    return transport;
}

/**
 * Build a clean wiring: mock kernel → transport → M-05 → 调试面接线，
 * plus the mounted + rendered debug view.
 *
 * @returns {{kernel: object, transport: object, router: object}}
 */
async function setup() {
    m03.reset();
    const kernel = createMockKernel();
    const transport = inProcessTransport(kernel);
    const router = await m05.mountCommandRouter({ transport });
    app.wireDebug(router);
    app.wireTopbarRunbar({ commandRouter: router });
    m01.assembleWorkspace("embedded");
    m01.route("#/debug");
    return { kernel, transport, router };
}

/** The debug view's body element. */
function debugBody() {
    const body = dom.document.querySelector('[data-view-body="debug"]');
    assert.ok(body, "调试视图体未挂载");
    return body;
}

/** The waveform view's body element. */
function waveformBody() {
    const body = dom.document.querySelector('[data-view-body="waveform"]');
    assert.ok(body, "波形视图体未挂载");
    return body;
}

/**
 * Click a button and let the **production signal path** run.
 *
 * @param {object} router
 * @param {object} button
 * @returns {Promise<void>}
 */
async function clickAndDispatch(router, button) {
    /** @type {Promise<void>[]} */
    const pending = [];
    const listener = (ev) => {
        pending.push(app.handleCommand(ev.detail, { commandRouter: router }));
    };
    dom.window.addEventListener("riscv:command", listener);
    button.dispatchEvent(new Event("click"));
    dom.window.removeEventListener("riscv:command", listener);
    await Promise.all(pending);
}

/** Every `StepN` command that reached the kernel. */
function stepCommands(transport) {
    return transport.sent
        .map((s) => s.message.payload?.Command?.StepN)
        .filter((c) => c !== undefined);
}

/** Every `ExportTrace` command that reached the kernel. */
function exportCommands(transport) {
    return transport.sent
        .map((s) => s.message.payload?.Command?.ExportTrace)
        .filter((c) => c !== undefined);
}

// ---------------------------------------------------------------------------

describe("调试视图 · 重复按钮**撤掉**了", () => {
    test("调试视图里**没有**那五个与别处重复的按钮", async () => {
        const { router } = await setup();
        const body = debugBody();
        // ⚠️ **「read-regs」从这份清单里移走了**（v0.65 批）⇒ ⇒ 它的落点
        //   **搬进了调试视图**（寄存器窗口）⇒ ⇒ 「它不在调试视图里」这条
        //   断言**已经不成立** ⇒ ⇒ 由下面那条「它在寄存器窗口里」接着管。
        for (const action of ["step", "step10", "continue", "pause", "reset"]) {
            assert.equal(
                body.querySelectorAll(`[data-action="${action}"]`).length,
                0,
                `调试视图里还有「${action}」按钮（应当只在它的落点出现一次）`,
            );
        }
        router.close();
    });

    test("调试视图**没有**工具条了（那一整块撤掉）", async () => {
        const { router } = await setup();
        assert.equal(
            debugBody().querySelectorAll(".debug-toolbar").length,
            0,
            "调试视图里还有工具条（那一块应当整块撤掉）",
        );
        router.close();
    });

    test("**顶栏**那四个按钮还在（运行 / 单步 / 暂停 / 复位）", async () => {
        const { router } = await setup();
        for (const action of ["run", "step", "pause", "reset"]) {
            assert.ok(
                dom.document.querySelector(`[data-topbar-action="${action}"]`),
                `顶栏运行条上缺 ${action} 按钮`,
            );
        }
        router.close();
    });

    test("「读寄存器」在**调试视图的寄存器窗口**里（那儿才是它的落点 · v0.65 批）", async () => {
        const { router } = await setup();
        m01.route("#/debug");
        const body = dom.document.querySelector('[data-view-body="debug"]');
        assert.ok(body, "调试视图体未挂载");
        // ⚠️ **落点从「寄存器 / 内存视图」搬到了调试视图**（v0.65 批）⇒ ⇒
        //   「同一个动作只出现在一处」这条规矩**继续成立** ⇒ ⇒ 调试视图里
        //   **恰好一个**「读寄存器」按钮。
        assert.equal(
            body.querySelectorAll('[data-action="read-regs"]').length,
            1,
            "调试视图的寄存器窗口里没有「读寄存器」按钮（或出现了不止一个）",
        );
        // ⚠️ **它必须落在「寄存器窗口」那张卡里**（不是内存窗口）⇒ ⇒ 判据
        //   抓**位置**，不抓「页面上有没有」。
        const win = body.querySelector('[data-window="registers"]');
        assert.ok(win, "调试视图里没有「寄存器窗口」");
        assert.equal(
            win.querySelectorAll('[data-action="read-regs"]').length,
            1,
            "「读寄存器」不在「寄存器窗口」里",
        );
        assert.equal(
            body.querySelector('[data-window="memory"]')
                ?.querySelectorAll('[data-action="read-regs"]').length ?? 0,
            0,
            "「读寄存器」跑进「内存窗口」里去了",
        );
        router.close();
    });
});

describe("调试视图 · 反汇编卡片承担「反汇编」与「按步数单步」", () => {
    test("「反汇编」按钮在**反汇编卡片**里（不在工具条里）", async () => {
        const { router } = await setup();
        const btn = debugBody().querySelector('[data-action="disasm"]');
        assert.ok(btn, "调试视图里没有「反汇编」按钮");
        assert.ok(
            btn.closest?.(".disasm"),
            "「反汇编」按钮不在反汇编卡片里（它的落点变了）",
        );
        router.close();
    });

    test("反汇编卡片里有**步数输入框** + 「按步数单步」按钮", async () => {
        const { router } = await setup();
        const body = debugBody();
        const input = body.querySelector("[data-step-count-input]");
        assert.ok(input, "没有步数输入框");
        const btn = body.querySelector('[data-action="step-n"]');
        assert.ok(btn, "没有「按步数单步」按钮");
        assert.ok(
            btn.closest?.(".disasm"),
            "「按步数单步」按钮不在反汇编卡片里",
        );
        router.close();
    });

    test("步数输入框的**默认值**是 10（原先那个「单步 ×10」的习惯不变）", async () => {
        const { router } = await setup();
        const input = debugBody().querySelector("[data-step-count-input]");
        assert.ok(input, "没有步数输入框");
        assert.equal(String(input.getAttribute("value") ?? ""), "10");
        router.close();
    });

    test("点「按步数单步」⇒ ⇒ 发出**带步数**的 StepN（走生产信号路径）", async () => {
        const { router, transport } = await setup();
        const input = debugBody().querySelector("[data-step-count-input]");
        assert.ok(input, "没有步数输入框");
        input.value = "7";
        const btn = debugBody().querySelector('[data-action="step-n"]');
        assert.ok(btn, "没有「按步数单步」按钮");

        await clickAndDispatch(router, btn);

        const steps = stepCommands(transport);
        assert.equal(steps.length, 1, "「按步数单步」没有发出命令");
        assert.equal(steps[0].n, 7, "命令里的步数不是输入框里填的 7");
        router.close();
    });

    test("步数**归一**（纯函数）：非数 / 0 / 负数 ⇒ ⇒ 1，太大 ⇒ ⇒ `u32` 上界", () => {
        assert.equal(normalizeStepCount("10"), 10);
        assert.equal(normalizeStepCount(""), 1);
        assert.equal(normalizeStepCount("abc"), 1);
        assert.equal(normalizeStepCount("0"), 1);
        assert.equal(normalizeStepCount("-3"), 1);
        assert.equal(normalizeStepCount("2.7"), 2);
        assert.equal(normalizeStepCount("99999999999"), 0xffff_ffff);
    });
});

describe("调试视图 · trace 卡片承担「记录开关」与「下载」", () => {
    test("trace 卡片里有**开关** + **整份下载**（两个都在那一张卡里）", async () => {
        const { router } = await setup();
        const body = debugBody();
        assert.equal(
            body.querySelectorAll('[data-action="trace-toggle"]').length,
            1,
            "trace 开关不是一个（应当是一个带状态的开关）",
        );
        assert.equal(
            body.querySelectorAll('[data-action="download-trace-local"]').length,
            1,
            "整份下载的入口不是一个",
        );
        assert.ok(
            body.querySelector("[data-trace-state]"),
            "trace 状态行不在（它应当与开关同在 trace 卡片里）",
        );
        router.close();
    });

    test("「下载这一块」那个按钮**不在**了（只留整份下载）", async () => {
        const { router } = await setup();
        assert.equal(
            debugBody().querySelectorAll('[data-action="download-trace"]').length,
            0,
            "「下载这一块」还在（用户拍板：只留整份下载）",
        );
        router.close();
    });
});

describe("波形视图 · 承担「导出波形」", () => {
    test("波形视图里有「导出波形 (VCD)」按钮", async () => {
        const { router } = await setup();
        m01.route("#/waveform");
        const btn = waveformBody().querySelector('[data-action="export-trace"]');
        assert.ok(btn, "波形视图里没有导出按钮");
        assert.equal(btn.getAttribute("data-source"), "Waveform");
        assert.equal(btn.getAttribute("data-format"), "Vcd");
        router.close();
    });

    test("点它 ⇒ ⇒ 发出 ExportTrace{Waveform, Vcd}（走生产信号路径）", async () => {
        const { router, transport } = await setup();
        m01.route("#/waveform");
        const btn = waveformBody().querySelector('[data-action="export-trace"]');
        assert.ok(btn, "波形视图里没有导出按钮");

        await clickAndDispatch(router, btn);

        const exports = exportCommands(transport);
        assert.equal(exports.length, 1, "点导出没有发出 ExportTrace");
        assert.equal(exports[0].source, "Waveform");
        assert.equal(exports[0].format, "Vcd");
        router.close();
    });

    test("调试视图里**没有**导出波形按钮了（它只有那一个落点）", async () => {
        const { router } = await setup();
        assert.equal(
            debugBody().querySelectorAll('[data-action="export-trace"][data-source="Waveform"]').length,
            0,
            "调试视图里还有「导出波形」按钮（应当只在波形视图里）",
        );
        router.close();
    });
});

describe("模块图 · 旧缓存**不能**把页面砸成白屏", () => {
    /**
     * `front/lib/run-loop.js` 的**导出集合**（**故意写死**）。
     *
     * ⚠️ **为什么写死**：ES module 的**链接期**是**全有或全无**的 ⇒ ⇒ 「新的
     *   `app.js`」+「浏览器缓存里的旧 `run-loop.js`」⇒ ⇒ 只要 app.js 从旧
     *   文件里 import 了一个**旧文件没有的名字**⇒ ⇒ **整个 app.js 都不执行**
     *   ⇒ ⇒ 页面**只剩静态 HTML** ⇒ ⇒ 状态停在初值 ⇒ ⇒ 用户看到的是
     *   「**一片空白 + 未连接**」⇒ ⇒ 而真相是「**脚本根本没启动**」。
     *
     * ⇒ ⇒ 所以这条判据钉住：**新能力一律放新模块**（`front/lib/run-action.js`）
     *   ⇒ ⇒ 旧缓存那份**逐字可链接** ⇒ ⇒ 最坏只是「**新功能没有**」
     *   （降级）⇒ ⇒ **不会白屏**。
     */
    const RUN_LOOP_EXPORTS = [
        "HALT_REASON_BREAKPOINT",
        "HALT_REASON_END_OF_EXECUTION",
        "HALT_REASON_TRAP",
        "HALT_REASON_USER_PAUSE",
        "HALT_REASON_WATCHPOINT",
        "LIVE_HIDDEN_SYMBOL",
        "RUN_ACTION_CONTINUE",
        "RUN_ACTION_NONE",
        "RUN_ACTION_STOP",
        "RUN_MODE_IDLE",
        "RUN_MODE_RUNNING",
        "bumpBusy",
        "busyOf",
        "initialRunState",
        "isKernelRunning",
        "liveValue",
        "nextRunAction",
        "runStateAfter",
        "startRunState",
        "stopRunState",
    ];

    /** 从源码文本里抽出**导出名**（`export function/const/class` 与 `export {…}`）。 */
    function exportedNames(source) {
        const named = [...source.matchAll(/export\s+(?:function|const|class|let)\s+([A-Za-z0-9_$]+)/g)]
            .map((m) => m[1]);
        const braced = [...source.matchAll(/export\s*\{([^}]*)\}/g)]
            .flatMap((m) => m[1].split(",").map((x) => x.trim()).filter(Boolean));
        return [...new Set([...named, ...braced])].sort();
    }

    test("`lib/run-loop.js` 的导出集合**一个字都没改**（新能力住在新模块）", async () => {
        const source = readFileSync(resolve(frontDir, "lib/run-loop.js"), "utf8");
        assert.deepEqual(
            exportedNames(source),
            [...RUN_LOOP_EXPORTS].sort(),
            "既有模块的导出集合被改了 ⇒ ⇒ 浏览器缓存里的旧副本就链接不上 ⇒ ⇒ 白屏",
        );
    });

    test("`app.js` 从 `lib/run-loop.js` 导入的名字**全都在导出集合里**", async () => {
        const source = readFileSync(resolve(frontDir, "app.js"), "utf8");
        const m = /import\s*\{([^}]*)\}\s*from\s*"\.\/lib\/run-loop\.js"/.exec(source);
        assert.ok(m, "app.js 里没有从 ./lib/run-loop.js 的 import 语句");
        const imported = m[1].split(",").map((x) => x.trim()).filter(Boolean);
        assert.ok(imported.length > 0, "import 名单是空的");
        for (const name of imported) {
            assert.ok(
                RUN_LOOP_EXPORTS.includes(name),
                `app.js 从 run-loop.js 导入了 ${name}，而**旧缓存那份没有它**`
                + " ⇒ ⇒ 链接失败 ⇒ ⇒ 整个 app.js 都不执行 ⇒ ⇒ 白屏",
            );
        }
    });

    test("新能力住在**新模块**里（新文件 ⇒ ⇒ 从来没被缓存过 ⇒ ⇒ 必然是新的）", async () => {
        const source = readFileSync(resolve(frontDir, "lib/run-action.js"), "utf8");
        for (const name of ["markStarted", "runActionLabel", "normalizeStepCount"]) {
            assert.match(
                source,
                new RegExp(`export function ${name}\\b`),
                `新模块里没有 ${name}`,
            );
        }
    });
});

describe("白屏 · 要能**自解释**", () => {
    test("状态初值是「启动中…」而不是「未连接」（两者**不是**一回事）", () => {
        const html = readFileSync(resolve(frontDir, "index.html"), "utf8");
        assert.match(html, /id="status"[^>]*>启动中…</, "状态初值应当是「启动中…」");
        assert.ok(
            !/id="status"[^>]*>未连接</.test(html),
            "状态初值又写回「未连接」了 ⇒ ⇒ 「脚本没启动」会被误读成「内核没连上」",
        );
    });

    /**
     * 把 index.html 里那段**内联脚本**取出来，在 stub 的 DOM 上**真跑一遍**。
     *
     * ⚠️ **为什么必须「真跑」**：那段脚本是 **inline classic script** ⇒ ⇒ 它
     *   **不能** import 任何模块 ⇒ ⇒ 它**必须自包含**（模块图整个坏掉的时候，
     *   **只有它**还能说话）⇒ ⇒ 于是它**没法**被 Node 侧 import 来测 ⇒ ⇒
     *   唯一的测法就是「把它的源码取出来、在 stub 的 DOM 上执行」⇒ ⇒ 判据抓的
     *   才是**行为**（定时器有没有排上、触发后状态位变成什么）⇒ ⇒ 而**不是**
     *   「源码里有没有那句话」（那句话可以**根本没执行** ⇒ ⇒ 零判别力）。
     *
     * @returns {{timers: Function[], status: Element}}
     */
    function runInlineBootScript() {
        const html = readFileSync(resolve(frontDir, "index.html"), "utf8");
        const m = /<script>\s*([\s\S]*?)<\/script>/.exec(html);
        assert.ok(m, "index.html 里没有那段内联脚本");
        /** @type {Function[]} */
        const timers = [];
        const fakeWindow = {
            setTimeout(fn) {
                timers.push(fn);
                return timers.length;
            },
        };
        // ⚠️ `location.protocol` 用 **http:** ⇒ ⇒ 「file:// 请通过 HTTP 打开」
        //   那一支**不触发** ⇒ ⇒ 本用例只盯启动自检。
        const run = new Function("window", "document", "location", m[1]);
        run(fakeWindow, dom.document, { protocol: "http:" });
        const status = dom.document.getElementById("status");
        assert.ok(status, "状态位不在 DOM 里");
        return { timers, status };
    }

    test("内联脚本**真的**排了一个启动自检（脚本没起来 ⇒ ⇒ 会说话）", () => {
        const { timers, status } = runInlineBootScript();
        assert.equal(timers.length, 1, "内联脚本没有排启动自检（白屏就没人说话了）");
        assert.equal(
            String(status.textContent ?? ""),
            "启动中…",
            "刚排上自检时状态位应当还是初值",
        );
    });

    test("启动自检**触发后**给出**可执行**的提示（硬刷新）", () => {
        const { timers, status } = runInlineBootScript();
        timers[0]();
        const text = String(status.textContent ?? "");
        assert.match(text, /脚本没启动/, `状态位没有说清真相：${text}`);
        assert.match(text, /Shift\+R/, `提示里没有硬刷新这个动作：${text}`);
    });

    test("**反向对照**：脚本**起来了**（状态位已是真实状态）⇒ ⇒ 自检**不许**打扰", () => {
        const { timers, status } = runInlineBootScript();
        status.textContent = "kernel 已连接 — wasm worker";
        timers[0]();
        assert.equal(
            String(status.textContent ?? ""),
            "kernel 已连接 — wasm worker",
            "脚本明明起来了，自检却把状态位覆盖了",
        );
    });
});

describe("顶栏 · 「运行 / 继续」文案切换", () => {
    /** 顶栏那个运行按钮。 */
    function runButton() {
        const btn = dom.document.querySelector('[data-topbar-action="run"]');
        assert.ok(btn, "顶栏运行条上没有运行按钮");
        return btn;
    }

    test("**装载之后**显示「运行」", async () => {
        const { router } = await setup();
        assert.equal(String(runButton().textContent ?? "").trim(), "运行");
        router.close();
    });

    test("**单步之后**切成「继续」（程序离开过起点）", async () => {
        const { router } = await setup();
        await app.handleCommand({ action: "step" }, { commandRouter: router });
        assert.equal(String(runButton().textContent ?? "").trim(), "继续");
        router.close();
    });

    test("**暂停之后**仍然是「继续」（暂停不抹掉「跑过没有」）", async () => {
        const { router } = await setup();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        await app.handleCommand({ action: "pause" }, { commandRouter: router });
        assert.equal(String(runButton().textContent ?? "").trim(), "继续");
        router.close();
    });

    test("**复位之后**回到「运行」（新工程 = 新起点）", async () => {
        const { router } = await setup();
        await app.handleCommand({ action: "step" }, { commandRouter: router });
        assert.equal(String(runButton().textContent ?? "").trim(), "继续");
        await app.handleCommand({ action: "reset" }, { commandRouter: router });
        assert.equal(String(runButton().textContent ?? "").trim(), "运行");
        router.close();
    });

    test("文案判据是**纯函数**（界面与判据引同一处）", () => {
        assert.equal(runActionLabel({ started: false }), "运行");
        assert.equal(runActionLabel({ started: true }), "继续");
    });
});
