// 运行中隐藏活值 + 内存查看告警（STAGE-38 · v0.53 批）的前端门禁（A-1 ~ A-16）。
//
// ⚠️ **分层**：A-1 ~ A-5 验**纯函数**（`front/lib/run-loop.js`，零 DOM）；
//   A-6 ~ A-11 验 **app 层的接线**（`front/app.js` 的 `handleCommand` 与
//   `wireDebug` 那**一个** `halted` 订阅）；A-12 ~ A-13 验**告警弹窗**
//   （`front/lib/alert.js`）；A-14 ~ A-15 验**视图层**（`front/views/
//   m02_viewcomponents.js` 的渲染）；A-16 验**脚本口**那条例外。
//   ⇒ ⇒ **真机**（真的在跑、真的读内存、真的弹窗）由
//   `tools/browser-verify.mjs` 的 `liveReadout` 段（B-42 ~ B-52）负责。
//
// ⚠️ **本批的第一约束是「运行中 = 内核真的在执行」**（裁决 A）
//   ⇒ ⇒ 不只看 `debug.run.mode`（脚本口跑的那**一个**窗口也盖得住）
//   ⇒ ⇒ A-3 钉的就是这一支。
//
// ⚠️ **第二约束是「活值显示固定符号 `-`」**（裁决 D）
//   ⇒ ⇒ **不是**占位文案、**不是**冻结旧值、**不是**隐藏整块
//   ⇒ ⇒ A-5 / A-14 / A-15 三层都钉这个符号。
//
// ⚠️ **第三约束是「拒绝的是执行，不是显示」**（裁决 F）
//   ⇒ ⇒ A-10 钉的是「`ReadMemory` **根本没发出去**」⇒ ⇒ 光判「界面上有
//   弹窗」的话，「拒绝执行但仍然发命令」照样绿。
//
// ⚠️ **判据必须正向**：先钉住「被观测的对象存在」（32 个寄存器格在、PC 在、
//   时间在、按钮在），再逐条断言它的内容 ⇒ ⇒ 对象整个没了时不会「零断言假绿」。
//   ⇒ ⇒ A-14 / A-15 各带一段「不运行时显示**真实值**」的正向对照。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器**。

import { test, describe, beforeEach, mock } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");

const dom = installDom(indexHtml);

// ⚠️ **关掉 `app.js` 的自启动**（与 `run-loop.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意
// 两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const runloop = await importFront("lib/run-loop.js");
const alert = await importFront("lib/alert.js");

const $ = (selector) => dom.document.querySelector(selector);

// ---------------------------------------------------------------------------
// 替身
// ---------------------------------------------------------------------------

/**
 * 一个**够用**的命令路由器替身：记下发出去的命令、记住订阅者、能推事件。
 *
 * ⚠️ **必须能推 `halted` 事件** ⇒ ⇒ 「在途计数」的退休点**只**在那个订阅上
 * （裁决 B）⇒ ⇒ 只测「发出去什么」测不到退休。
 *
 * @returns {object}
 */
function fakeRouter() {
    /** @type {object[]} 发出去的命令 */
    const sent = [];
    /** @type {Map<string, Set<Function>>} 订阅表 */
    const subs = new Map();
    return {
        sent,
        subs,
        subscribe(topic, handler) {
            if (!subs.has(topic)) subs.set(topic, new Set());
            subs.get(topic).add(handler);
            return () => subs.get(topic).delete(handler);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            return { value: {} };
        },
        /**
         * 内核推一条事件（同步）⇒ ⇒ 调用方立刻能看见后果。
         *
         * @param {string} topic
         * @param {object} value
         * @returns {boolean} 前端收到了吗
         */
        emit(topic, value) {
            const handlers = subs.get(topic);
            if (!handlers || handlers.size === 0) return false;
            const event = { variant: "Halted", topic, value };
            for (const fn of [...handlers]) fn(event);
            return true;
        },
    };
}

/**
 * 寄存器 / 内存服务的替身：只记「读了几次」。
 *
 * @returns {{calls: any[], readMemory: (addr: number, size: string) => Promise<object>}}
 */
function fakeRegisterMemory() {
    /** @type {any[]} */
    const calls = [];
    return {
        calls,
        async readMemory(addr, size) {
            calls.push([addr, size]);
            return { value: {} };
        },
    };
}

/**
 * 把视图状态里的 `debug.run` 设成指定的那一份。
 *
 * ⚠️ **视图层读的就是这一份**（裁决 M）⇒ ⇒ 判据必须**写它**而不是绕开它。
 *
 * @param {{mode?: string, windows?: number, busy?: number}} run
 */
function setRunState(run) {
    m03.setState("debug", (prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), run },
    }));
}

/** 32 个寄存器（x1 刻意不等于 0，好让「显示真实值」看得见差别）。 */
const GPR = Array.from({ length: 32 }, (_, i) => (i + 1) * 0x01010101);

// ---------------------------------------------------------------------------
// A-1 ~ A-5：纯函数（零 DOM）
// ---------------------------------------------------------------------------

describe("「运行中」的判据与活值口径（纯函数）", () => {
    test("A-1 · idle + 没有在途 ⇒ ⇒ **不是**运行中", () => {
        assert.equal(
            runloop.isKernelRunning({ mode: "idle", busy: 0 }),
            false,
            "既不在连续模式、又没有在途的 Continue ⇒ ⇒ 不是运行中",
        );
    });

    test("A-2 · running ⇒ ⇒ **是**运行中（哪怕此刻没有在途的窗口）", () => {
        assert.equal(
            runloop.isKernelRunning({ mode: "running", busy: 0 }),
            true,
            "连续模式**正在跑** ⇒ ⇒ 是运行中",
        );
    });

    test("A-3 · idle + **有**在途 ⇒ ⇒ **是**运行中（脚本口那一个窗口）", () => {
        // ⚠️ **这一条最容易写成「空转」**：把判据写成「只看 mode」的话，本条
        // **照样绿** ⇒ ⇒ 它必须**单独**钉住（脚本口跑的那一个窗口不在连续模式里）。
        assert.equal(
            runloop.isKernelRunning({ mode: "idle", busy: 1 }),
            true,
            "脚本口跑的那一个窗口也在跑 ⇒ ⇒ 是运行中（不能只看 mode）",
        );
    });

    test("A-4 · `bumpBusy` 永远**不**小于 0，且**只**动 `busy`", () => {
        assert.equal(
            runloop.bumpBusy({ mode: "idle", windows: 0, busy: 0 }, -1).busy,
            0,
            "多来一次「执行结束」不许把计数推成负数（负数会让 `busy > 0` 假假）",
        );
        assert.equal(
            runloop.bumpBusy({ mode: "idle", windows: 0, busy: 2 }, -1).busy,
            1,
            "减一就是减一",
        );
        assert.equal(
            runloop.bumpBusy({ mode: "idle", windows: 0, busy: 0 }, +1).busy,
            1,
            "加一就是加一",
        );
        const out = runloop.bumpBusy({ mode: "running", windows: 7, busy: 1 }, +1);
        assert.equal(out.mode, "running", "**不**许动模式");
        assert.equal(out.windows, 7, "**不**许动轮数");
    });

    test("A-5 · 运行中 ⇒ ⇒ 固定符号 `-`；不运行 ⇒ ⇒ 原值**逐字**", () => {
        assert.equal(runloop.liveValue(true, "0x00000020"), "-", "运行中只显示固定符号");
        assert.equal(runloop.liveValue(true, 123), "-", "运行中只显示固定符号");
        assert.equal(
            runloop.liveValue(false, "0x00000020"),
            "0x00000020",
            "不运行时原值**逐字**保留",
        );
        assert.equal(runloop.liveValue(false, 123), "123", "不运行时原值**逐字**保留");
        assert.equal(runloop.LIVE_HIDDEN_SYMBOL, "-", "固定符号就是 `-`");
    });
});

// ---------------------------------------------------------------------------
// A-6 ~ A-11：app 层的接线
// ---------------------------------------------------------------------------

describe("运行中隐藏活值 + 读内存拒绝（app 层）", () => {
    beforeEach(() => {
        // ⚠️ **视图状态是模块级单例** ⇒ ⇒ 每个用例前清干净 ⇒ ⇒ 用例之间
        // 不会互相污染。
        m03.reset();
        // ⚠️ **弹窗也要清** ⇒ ⇒ 否则上一个用例留下的弹窗会让本例的
        // 「不该有弹窗」红成**假红**。
        alert.hideAlert();
    });

    test("A-6 · 点「运行」⇒ ⇒ 有在途的 Continue 且 `isRunning()` 为真", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        assert.ok(
            app.getRunState().busy > 0,
            "「运行」之后必须有**在途**的 Continue（否则活值不会立刻隐藏）",
        );
        assert.equal(app.isRunning(), true, "点完「运行」必须是运行中");
    });

    test("A-7 · 内核让出控制权 ⇒ ⇒ **仍然**是运行中（连续模式）", async () => {
        const router = fakeRouter();
        app.wireDebug(router);
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        router.emit("halted", { reason: "EndOfExecution", pc: 0x0 });

        // ⚠️ **连续模式里内核让出控制权**（跑满一个指令预算窗口）⇒ ⇒ 前端
        // 马上会续发 ⇒ ⇒ 那段时间**仍然是**运行中 ⇒ ⇒ 活值**不许**闪一下。
        assert.equal(
            app.isRunning(),
            true,
            "内核让出控制权的那一瞬间仍然是运行中（否则界面会闪一下）",
        );
    });

    test("A-8 · 命中断点 ⇒ ⇒ **不是**运行中（真停 ⇒ ⇒ 活值恢复）", async () => {
        const router = fakeRouter();
        app.wireDebug(router);
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        router.emit("halted", { reason: "Breakpoint", pc: 0x20 });

        assert.equal(
            app.isRunning(),
            false,
            "断点是真停 ⇒ ⇒ 不是运行中 ⇒ ⇒ 活值恢复显示",
        );
        assert.equal(app.getRunState().busy, 0, "真停 ⇒ ⇒ 在途计数归零");
    });

    test("A-9 · 点「暂停」⇒ ⇒ **仍然**是运行中（在途的那一个窗口还在跑）", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        assert.equal(app.isRunning(), true);

        await app.handleCommand({ action: "pause" }, { commandRouter: router });

        // ⚠️ **「暂停」只停续发**（v0.52 裁决 E）⇒ ⇒ 正在跑的那**一个**窗口
        // 还在跑 ⇒ ⇒ 那段时间**仍然**是运行中 ⇒ ⇒ 活值**不**该提前恢复
        // （提前恢复 = 显示**过期**的值）。
        assert.equal(app.getRunState().mode, "idle", "暂停后退出连续模式");
        assert.ok(
            app.getRunState().busy > 0,
            "在途计数必须**保留**（正在跑的那一个窗口还没停）",
        );
        assert.equal(
            app.isRunning(),
            true,
            "在途窗口还在跑 ⇒ ⇒ 仍然是运行中（活值不提前恢复）",
        );
    });

    test("A-10 · 运行中点「读内存」⇒ ⇒ **不发**命令 **且** 弹窗出现", async () => {
        const router = fakeRouter();
        const regmem = fakeRegisterMemory();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        await app.handleCommand(
            { action: "read-mem", params: { addr: 0x100, size: "Word" } },
            { commandRouter: router, registerMemory: regmem },
        );

        // ⚠️ **拒绝的是「执行」不是「显示」**⇒ ⇒ 命令**根本没发出去**。
        assert.equal(
            regmem.calls.length,
            0,
            "运行中读内存必须被**拒绝**（不是「读了但不给看」）",
        );
        assert.equal(alert.alertState(), "open", "必须弹窗告诉用户");
        const text = String($("[data-alert-text]")?.textContent ?? "");
        assert.ok(
            text.includes("请先暂停再读"),
            `弹窗文案必须说清楚下一步怎么做：${JSON.stringify(text)}`,
        );
    });

    test("A-11 · 停止后点「读内存」⇒ ⇒ 正常读，**不**弹窗", async () => {
        const router = fakeRouter();
        const regmem = fakeRegisterMemory();

        await app.handleCommand(
            { action: "read-mem", params: { addr: 0x100, size: "Word" } },
            { commandRouter: router, registerMemory: regmem },
        );

        assert.equal(regmem.calls.length, 1, "停止时读内存必须**真的执行**");
        assert.deepEqual(
            regmem.calls[0],
            [0x100, "Word"],
            "读的地址与宽度**逐字**不变（⚠️ v0.54 批起，地址**来自界面**）",
        );
        assert.equal(alert.alertState(), "", "停止时读内存不许弹窗");
    });

    test("A-16 · 脚本口 `readMemory` 在运行中**不**弹窗（裁决 G）", async () => {
        const router = fakeRouter();
        app.exposeScriptCommands(router);
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        await globalThis.window.riscv_kernel.readMemory(0x100, "Word");

        assert.equal(
            alert.alertState(),
            "",
            "脚本口是**脚本**的路 ⇒ ⇒ 不弹窗（用户拍定 · F14）",
        );
    });
});

// ---------------------------------------------------------------------------
// A-12 ~ A-13：告警弹窗
// ---------------------------------------------------------------------------

describe("告警弹窗（居中模态 · 5 秒渐隐 · 「知道了」关闭）", () => {
    beforeEach(() => {
        alert.hideAlert();
    });

    test("A-12 · 显示 5 秒之后**自动渐隐消失**", () => {
        // ⚠️ **「5 秒」这个数要单独钉住**⇒ ⇒ 否则把常量改成 300 的话
        // 「用常量去 tick」的写法**照样绿**⇒ ⇒ 这里用**字面量**钉。
        assert.equal(
            alert.ALERT_HOLD_MS,
            5000,
            "弹窗必须显示 5 秒（用户拍定）",
        );

        mock.timers.enable({ apis: ["setTimeout"] });
        try {
            alert.showAlert("测试");
            assert.equal(alert.alertState(), "open", "弹窗应当立刻显示");

            mock.timers.tick(alert.ALERT_HOLD_MS - 1);
            assert.equal(alert.alertState(), "open", "还没到 5 秒 ⇒ ⇒ 不许消失");

            mock.timers.tick(1);
            assert.equal(alert.alertState(), "fading", "到 5 秒 ⇒ ⇒ 开始渐隐");

            mock.timers.tick(alert.ALERT_FADE_MS);
            assert.equal(alert.alertState(), "", "渐隐完 ⇒ ⇒ 关掉");
        } finally {
            mock.timers.reset();
        }
    });

    test("A-13 · 点「知道了」⇒ ⇒ **立即**关闭", () => {
        alert.showAlert("测试");
        assert.equal(alert.alertState(), "open", "弹窗应当显示");

        const btn = $("[data-alert-dismiss]");
        assert.ok(btn, "弹窗上必须有「知道了」按钮");
        assert.equal(
            String(btn.textContent ?? "").trim(),
            "知道了",
            "那个按钮的文案应当是「知道了」",
        );

        btn.click();
        assert.equal(
            alert.alertState(),
            "",
            "点了「知道了」必须**立即**关闭（不用再等渐隐）",
        );
    });
});

// ---------------------------------------------------------------------------
// A-14 ~ A-15：视图层
// ---------------------------------------------------------------------------

describe("运行中隐藏活值（视图层）", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("A-14 · 调试视图：运行中 ⇒ ⇒ `-`；不运行 ⇒ ⇒ 真实值（正向对照）", () => {
        const view = m02.get("debug");
        assert.ok(view, "调试视图必须注册着");

        // -- 正向对照：**不运行**时显示真实值 --------------------------
        setRunState({ mode: "idle", windows: 0, busy: 0 });
        const idle = view.render({ sim: { gpr: GPR, pc: 0x20 }, debug: {}, timeVirt: 123 });
        const idleCells = [...idle.matchAll(
            /<span class="muted">x\d+<\/span><code[^>]*>([^<]*)<\/code>/g,
        )].map((m) => m[1]);
        assert.equal(idleCells.length, 32, "寄存器格必须**仍是 32 个**（稳定钩子）");
        assert.ok(
            idleCells.every((c) => c !== "-"),
            "不运行时寄存器格必须显示**真实值**（正向对照）",
        );
        assert.ok(
            idle.includes('<code class="pc">0x00000020</code>'),
            "不运行时 PC 必须显示真实值（正向对照）",
        );
        assert.ok(
            idle.includes('<code class="timeVirt">123 ps</code>'),
            "不运行时虚拟时间必须显示真实值（正向对照）",
        );

        // -- 运行中 ⇒ ⇒ 全部 `-` ---------------------------------------
        setRunState({ mode: "running", windows: 0, busy: 0 });
        const busy = view.render({ sim: { gpr: GPR, pc: 0x20 }, debug: {}, timeVirt: 123 });
        const busyCells = [...busy.matchAll(
            /<span class="muted">x\d+<\/span><code[^>]*>([^<]*)<\/code>/g,
        )].map((m) => m[1]);
        assert.equal(busyCells.length, 32, "寄存器格必须**仍是 32 个**（稳定钩子）");
        assert.ok(
            busyCells.every((c) => c === "-"),
            "运行中寄存器格必须显示固定符号 `-`",
        );
        assert.ok(
            busy.includes('<code class="pc">-</code>'),
            "运行中 PC 必须显示固定符号 `-`",
        );
        assert.ok(
            busy.includes('<code class="timeVirt">-</code>'),
            "运行中虚拟时间必须显示固定符号 `-`",
        );
    });

    test("A-15 · 寄存器视图：运行中 ⇒ ⇒ `-`；不运行 ⇒ ⇒ 真实值（正向对照）", () => {
        const view = m02.get("regmem");
        assert.ok(view, "寄存器视图必须注册着");

        const state = {
            sim: { gpr: GPR, pc: 0x20, mem: { "0x00000100": 0x1234 } },
            timeVirt: 123,
        };
        const memRows = (html) => [...html.matchAll(
            /<tr><td><code>([^<]*)<\/code><\/td>\s*<td><code>([^<]*)<\/code><\/td><\/tr>/g,
        )].map((m) => ({ addr: m[1], value: m[2] }));

        // -- 正向对照：**不运行**时显示真实值 --------------------------
        setRunState({ mode: "idle", windows: 0, busy: 0 });
        const idle = view.render(state);
        const idleCells = [...idle.matchAll(
            /<span class="muted">x\d+<\/span><code[^>]*>([^<]*)<\/code>/g,
        )].map((m) => m[1]);
        assert.equal(idleCells.length, 32, "寄存器格必须**仍是 32 个**（稳定钩子）");
        assert.ok(
            idleCells.every((c) => c !== "-"),
            "不运行时寄存器格必须显示**真实值**（正向对照）",
        );
        assert.ok(idle.includes('<code class="pc">0x00000020</code>'), "不运行时 PC 是真实值");
        assert.ok(idle.includes('<code class="timeVirt">123 ps</code>'), "不运行时间是真实值");
        const idleRows = memRows(idle);
        assert.equal(idleRows.length, 1, "内存单元必须**仍是一行**（稳定钩子）");
        assert.equal(idleRows[0].value, "0x00001234", "不运行时内存值是真实值（正向对照）");

        // -- 运行中 ⇒ ⇒ 全部 `-` ---------------------------------------
        setRunState({ mode: "running", windows: 0, busy: 0 });
        const busy = view.render(state);
        const busyCells = [...busy.matchAll(
            /<span class="muted">x\d+<\/span><code[^>]*>([^<]*)<\/code>/g,
        )].map((m) => m[1]);
        assert.equal(busyCells.length, 32, "寄存器格必须**仍是 32 个**（稳定钩子）");
        assert.ok(
            busyCells.every((c) => c === "-"),
            "运行中寄存器格必须显示固定符号 `-`",
        );
        assert.ok(busy.includes('<code class="pc">-</code>'), "运行中 PC 是 `-`");
        assert.ok(busy.includes('<code class="timeVirt">-</code>'), "运行中时间是 `-`");
        const busyRows = memRows(busy);
        assert.equal(busyRows.length, 1, "内存单元必须**仍是一行**（稳定钩子）");
        assert.equal(busyRows[0].value, "-", "运行中内存值必须显示固定符号 `-`");
    });
});
