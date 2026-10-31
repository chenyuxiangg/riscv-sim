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
const { normalizeStepCount, runActionLabel } = await importFront("lib/run-loop.js");

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
    test("调试视图里**没有**那六个与别处重复的按钮", async () => {
        const { router } = await setup();
        const body = debugBody();
        for (const action of ["step", "step10", "continue", "pause", "reset", "read-regs"]) {
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

    test("「读寄存器」在**寄存器/内存视图**里（那儿才是它的落点）", async () => {
        const { router } = await setup();
        m01.route("#/regmem");
        const body = dom.document.querySelector('[data-view-body="regmem"]');
        assert.ok(body, "寄存器/内存视图体未挂载");
        assert.equal(
            body.querySelectorAll('[data-action="read-regs"]').length,
            1,
            "寄存器/内存视图里没有「读寄存器」按钮",
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
