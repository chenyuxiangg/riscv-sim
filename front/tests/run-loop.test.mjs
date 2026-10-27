// 连续运行（STAGE-37 · v0.52 批）的前端门禁（A-1 ~ A-12）。
//
// ⚠️ **分层**：A-1 ~ A-6 验**纯函数**（`front/lib/run-loop.js`，零 DOM）；
//   A-7 ~ A-11 验 **app 层的接线**（`front/app.js` 的 `handleCommand` 与
//   `wireDebug` 那**一个** `halted` 订阅）；A-12 验**顶栏那个按钮**。
//   ⇒ ⇒ **真机**（真的连续跑、真的在断点停、真的暂停）由
//   `tools/browser-verify.mjs` 的 `runLoop` 段（B-34 ~ B-41）负责。
//
// ⚠️ **本批的第一约束是「连续不是命令的语义，是前端循环的语义」**（裁决 A）
//   ⇒ ⇒ 契约层零升版、零新 DTO / 命令 / 事件 ⇒ ⇒ 内核与宿主**零改动**
//   ⇒ ⇒ 这里验的全是**前端**的行为。
//
// ⚠️ **第二约束是「界面上的暂停不许发 `Pause` 命令」**（裁决 E / F6 实测）
//   ⇒ ⇒ A-9 钉的是「**没有** `Pause` 出去」⇒ ⇒ 只钉「状态变了」的话，
//   「停续发 **且** 发 `Pause`」照样绿 ⇒ ⇒ 残留那条 bug 溜过去。
//
// ⚠️ **判据必须正向**：先钉住「被观测的对象存在」（按钮在、状态在），再逐条
//   断言它的内容 ⇒ ⇒ 对象整个没了时不会「零断言假绿」。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器**。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
const appSrc = readFileSync(resolve(frontDir, "app.js"), "utf8");

const dom = installDom(indexHtml);

// ⚠️ **关掉 `app.js` 的自启动**（与 `board-rules.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意
// 两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m03 = await importFront("views/m03_viewstore.js");
const runloop = await importFront("lib/run-loop.js");

const $ = (selector) => dom.document.querySelector(selector);

// ---------------------------------------------------------------------------
// 替身
// ---------------------------------------------------------------------------

/**
 * 一个**够用**的命令路由器替身：记下发出去的命令、记住订阅者、能推事件。
 *
 * ⚠️ **必须能推 `halted` 事件** ⇒ ⇒ 因为连续运行的续发**只**从订阅来
 * （裁决 C / F8）⇒ ⇒ 只测「发出去什么」测不到续发。
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

/** 发出去的命令里，`Continue` 有几条。 */
const continueCount = (router) => router.sent.filter((p) => p === "Continue").length;
/** 发出去的命令里，`Pause` 有几条。 */
const pauseCount = (router) => router.sent.filter((p) => p === "Pause").length;

// ---------------------------------------------------------------------------
// A-1 ~ A-6：纯函数（零 DOM）
// ---------------------------------------------------------------------------

describe("连续运行的判定（纯函数）", () => {
    test("A-1 · running + EndOfExecution ⇒ ⇒ 「续发」", () => {
        assert.equal(
            runloop.nextRunAction({ mode: "running", reason: "EndOfExecution" }),
            "continue",
            "连续模式里内核让出控制权 ⇒ ⇒ 应当续发",
        );
    });

    test("A-2 · idle + EndOfExecution ⇒ ⇒ **不续**（防空转的关键一条）", () => {
        // ⚠️ **这一条最容易写成「空转」**：把 `nextRunAction` 写成「永远
        // `continue`」的话，本条**照样绿** ⇒ ⇒ 它必须**单独**钉住。
        assert.equal(
            runloop.nextRunAction({ mode: "idle", reason: "EndOfExecution" }),
            "none",
            "不在连续模式时内核让出控制权是**正常**的 ⇒ ⇒ 绝不许续发（否则空转）",
        );
    });

    test("A-3 · 四种「真停」原因 ⇒ ⇒ 「停」（四个**逐个**验）", () => {
        for (const reason of ["Breakpoint", "Watchpoint", "Trap", "UserPause"]) {
            assert.equal(
                runloop.nextRunAction({ mode: "running", reason }),
                "stop",
                `${reason} 应当让连续运行停下来`,
            );
            // ⚠️ **idle 时也要停**（否则状态会飘）⇒ ⇒ 两种模式都验。
            assert.equal(
                runloop.nextRunAction({ mode: "idle", reason }),
                "stop",
                `${reason} 在非连续模式下也应当被认出来`,
            );
        }
    });

    test("A-4 · 停 ⇒ ⇒ 退出连续模式 **且** 轮数**保留**", () => {
        const out = runloop.runStateAfter({ mode: "running", windows: 7 }, "Breakpoint");
        assert.equal(out.mode, "idle", "停下之后应当退出连续模式");
        assert.equal(out.windows, 7, "轮数必须**保留**（归零的话每次停下都被抹掉）");
    });

    test("A-5 · 续发 ⇒ ⇒ 仍是连续模式 **且** 轮数 +1", () => {
        const out = runloop.runStateAfter({ mode: "running", windows: 7 }, "EndOfExecution");
        assert.equal(out.mode, "running", "续发之后必须还在连续模式");
        assert.equal(out.windows, 8, "续发一次轮数 +1");
    });

    test("A-6 · 不认识的停下原因 ⇒ ⇒ 「什么都不做」（**不猜**）", () => {
        for (const reason of ["Whatever", "", null, undefined, 42]) {
            assert.equal(
                runloop.nextRunAction({ mode: "running", reason }),
                "none",
                `不认识的停下原因 ${JSON.stringify(reason)} 不许被猜成别的语义`,
            );
        }
    });
});

// ---------------------------------------------------------------------------
// A-7 ~ A-11：app 层的接线
// ---------------------------------------------------------------------------

describe("连续运行的接线（app 层）", () => {
    beforeEach(() => {
        // ⚠️ **视图状态是模块级单例** ⇒ ⇒ 每个用例前清干净 ⇒ ⇒ 用例之间
        // 不会互相污染（否则「幂等」那条会被上一条用例的残留毒到）。
        m03.reset();
    });

    test("A-7 · 「运行」⇒ ⇒ 发出 Continue **且** 视图状态进连续模式", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        assert.equal(continueCount(router), 1, "「运行」应当发出一条 Continue");
        assert.equal(router.sent[0], "Continue", "发出去的必须**就是** Continue");
        assert.equal(
            app.getRunState().mode,
            "running",
            "视图状态应当进连续模式（否则续发的判据永远落在 idle 上）",
        );

        // ⚠️ **「在跑」要看得见**（裁决 G）⇒ ⇒ 顶栏那个状态位**必须**跟着变。
        const status = $("[data-topbar-run-status]");
        assert.ok(status, "顶栏缺「连续运行状态」那个状态位");
        assert.equal(
            status.getAttribute("data-topbar-run-mode"),
            "running",
            "顶栏状态位没有反映出连续模式",
        );
        assert.ok(
            String(status.textContent ?? "").includes("运行中"),
            `顶栏状态位的文案没说出「在跑」：${JSON.stringify(status.textContent ?? "")}`,
        );
    });

    test("A-8 · 「运行」**幂等**：已经在跑时再点 ⇒ ⇒ 不再发第二条 Continue", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        assert.equal(
            continueCount(router),
            1,
            "连点两下「运行」只应发一条（否则是两个并行窗口）",
        );
    });

    test("A-9 · 「暂停」⇒ ⇒ 退出连续模式 **且** **没有** Pause 命令出去", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        await app.handleCommand({ action: "pause" }, { commandRouter: router });

        assert.equal(app.getRunState().mode, "idle", "暂停后应当退出连续模式");
        // ⚠️ **这一条钉的是「没有」** —— 内核里若留下 pending 暂停，下一次
        // 「运行」会在第一条指令边界就停（F6 实测）⇒ ⇒ 吃掉一整轮。
        assert.equal(
            pauseCount(router),
            0,
            "界面的「暂停」不许发 Pause 命令（F6：残留会吃掉下一次运行的一整轮）",
        );
    });

    test("A-10 · 收到 halted(EndOfExecution) 且在连续模式 ⇒ ⇒ **续发第二条** Continue", async () => {
        const router = fakeRouter();
        // ⚠️ **续发只从 `halted` 订阅来**（裁决 C）⇒ ⇒ 必须真的接上那份订阅。
        app.wireDebug(router);
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        assert.equal(continueCount(router), 1, "起手那一条");

        router.emit("halted", { reason: "EndOfExecution", pc: 0x0 });

        assert.equal(
            continueCount(router),
            2,
            "内核让出控制权之后应当**续发**一条 Continue（这就是「连续」）",
        );
        assert.equal(app.getRunState().mode, "running", "续发之后必须还在连续模式");
        assert.equal(app.getRunState().windows, 1, "跑完一轮该记 1 轮");
    });

    test("A-11 · 收到 halted(Breakpoint) ⇒ ⇒ 退出连续模式 **且** 不再发 Continue", async () => {
        const router = fakeRouter();
        app.wireDebug(router);
        await app.handleCommand({ action: "continue" }, { commandRouter: router });
        const before = continueCount(router);

        router.emit("halted", { reason: "Breakpoint", pc: 0x20 });

        assert.equal(
            continueCount(router),
            before,
            "断点之后不许再续发（否则连续模式在断点后还会继续跑）",
        );
        assert.equal(app.getRunState().mode, "idle", "断点之后应当退出连续模式");
        // ⚠️ **轮数保留**（A-4 的同一条语义在 app 层也要成立）⇒ ⇒ 用户看得见
        // 「这一趟跑了几轮才停」。
        assert.equal(app.getRunState().windows, 0, "还没跑完一轮就被断点打断时轮数是 0");
    });
});

// ---------------------------------------------------------------------------
// A-12：顶栏那个「暂停」按钮
// ---------------------------------------------------------------------------

describe("顶栏的「暂停」入口", () => {
    test("A-12 · 顶栏有「暂停」按钮，且与 app.js 的命令分支**逐字对应**", () => {
        const btn = $('[data-topbar-action="pause"]');
        assert.ok(btn, "顶栏运行条上缺「暂停」按钮（连续模式没有真实出口）");
        assert.equal(
            String(btn.textContent ?? "").trim(),
            "暂停",
            "顶栏那个按钮的文案应当是「暂停」",
        );
        // ⚠️ **逐字对应**：按钮的 `data-topbar-action` 与 app.js 里
        // `handleCommand` 的 `case` 分支**必须是同一个词** ⇒ ⇒ 顶栏点一下
        // 才会走对的那条命令路径。
        assert.ok(
            appSrc.includes('{ action: "pause" }'),
            "app.js 里没有把顶栏的「暂停」按钮接到 action: \"pause\"",
        );
        assert.ok(
            /case\s+"pause"\s*:/.test(appSrc),
            "app.js 的命令分支里没有 case \"pause\"",
        );
    });
});
