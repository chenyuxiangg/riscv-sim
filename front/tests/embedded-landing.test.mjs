// 落地视图（`embedded`）的组件与内容 —— D-33 · STAGE-10 裁决 C。
//
// 缺陷原文：M-01 的视图注册表里**有** `embedded`（嵌入式角色的落地视图），
// 但 M-02 **没有**为它注册组件 ⇒ 渲染事件 `viewId: "embedded"` 找不到接收者
// ⇒ 打开应用第一眼看到的是「嵌入式工作台」标题 + **空正文**。
//
// 这个套件盯住：
//   1. **B-1**：组件注册表里查得到 `embedded`（不是「查得到别的六个」）；
//   2. 落地视图正文**不是空面板**：有连接状态、当前 PC、虚拟时间；
//   3. **子视图入口**（调试 / 寄存器内存 / 波形 / 源码）四个都在，且
//      `data-*` 标记可被脚本取到（真机脚本靠它断言，不靠文案匹配）；
//   4. 入口**真的能跳**：点击后由 **M-01** 路由到目标视图；
//   5. PC / 虚拟时间**只**来自既有状态源（M-10 推进的 `sim.pc` / `timeVirt`）
//      ⇒ 前端不许自己造第二个 PC 来源。
//
// ⚠️ **文件名独立成文件**：M-03 的视图状态是**模块级单例**，而 `?role=` 是
// **导入时**读的 ⇒ 需要独立进程才能拿到干净的落地视图装配。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（D-46 检视 V-B · probe D2）：`app.js` 是
// 引导入口——**导入即启动**，它的 `bootstrap()` 是异步的，会在任意两个用例
// 之间动 M-03 的模块级单例。这里显式关掉，让套件**不依赖时序运气**。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

/**
 * 装配嵌入式工作台，并把**落地视图**渲染出来。
 *
 * @param {object} [state] — 推进给落地视图的视图状态
 */
function landEmbedded(state = {}) {
    m03.reset();
    m03.setState("embedded", state);
    m01.assembleWorkspace("embedded");
    m01.route("#/embedded");
    const body = dom.document.querySelector('[data-view-body="embedded"]');
    assert.ok(body, "落地视图体未挂载");
    return body;
}

/** 当前可见的面板（`data-view-panel`）。 */
function activePanel() {
    const panels = [...dom.document.querySelectorAll("[data-view-panel]")];
    const active = panels.find((p) => !p.classList.contains("hidden"));
    return active ? active.getAttribute("data-view-panel") : null;
}

// ---------------------------------------------------------------------------

describe("落地视图（embedded）· 组件注册", () => {
    test("B-1：组件注册表里查得到 embedded", () => {
        const component = m02.get("embedded");
        assert.ok(component, "M-02 组件注册表里没有 `embedded` ⇒ 落地视图没有接收者");
        assert.equal(component.id, "embedded");
        // ⚠️ **不是**「查得到别的六个」就够了 —— 断言的是**这一个** id。
        const ids = m02._all().map((c) => c.id);
        assert.ok(ids.includes("embedded"), `注册表里没有 embedded：${ids.join(", ")}`);
    });

    test("B-1：嵌入式角色的落地视图就是 embedded，且它有组件", () => {
        m01.assembleWorkspace("embedded");
        const workspace = m01.currentWorkspace();
        assert.equal(workspace.role, "embedded");
        assert.equal(
            workspace.activeView?.hash,
            "embedded",
            "嵌入式角色的落地视图不是 embedded",
        );
        assert.ok(
            m02.get(workspace.activeView.hash),
            "落地视图没有对应的 M-02 组件 ⇒ 正文必然是空的",
        );
    });

    test("落地视图组件实现了 ViewComponent 的三函数形状", () => {
        const component = m02.get("embedded");
        assert.equal(typeof component.render, "function");
        assert.equal(typeof component.onCommand, "function");
        assert.equal(typeof component.onDelta, "function");
        assert.equal(typeof component.mountFor, "function");
    });
});

describe("落地视图（embedded）· 正文不是空面板", () => {
    test("正文里有连接状态、当前 PC 与虚拟时间", () => {
        const body = landEmbedded({
            sim: { pc: 0x0000_1234 },
            timeVirt: 4200,
            link: { connected: true },
        });
        // ⚠️ **判别力**：只断言「非空」对白屏是绿的 ⇒ 断言落到**具体内容**。
        const link = body.querySelector("[data-embedded-link]");
        assert.ok(link, "落地视图正文里没有连接状态标记");
        assert.equal(link.getAttribute("data-connected"), "1");
        assert.match(link.textContent, /已连接/);

        const pc = body.querySelector("[data-embedded-pc]");
        assert.ok(pc, "落地视图正文里没有 PC 标记");
        assert.equal(pc.textContent.trim(), "0x00001234");

        const time = body.querySelector("[data-embedded-time]");
        assert.ok(time, "落地视图正文里没有虚拟时间标记");
        assert.match(time.textContent, /4200 ps/);
    });

    test("未连接时如实显示「未连接」（不假装连上了）", () => {
        const body = landEmbedded({
            link: { connected: false, lastError: "M-07 待接线" },
        });
        const link = body.querySelector("[data-embedded-link]");
        assert.equal(link.getAttribute("data-connected"), "0");
        assert.match(link.textContent, /未连接/);
        assert.match(link.textContent, /M-07 待接线/);
    });

    test("PC 与虚拟时间只来自视图状态（前端不造第二个 PC 来源）", () => {
        const body = landEmbedded({ sim: { pc: 0xdead_beef }, timeVirt: 99 });
        assert.equal(body.querySelector("[data-embedded-pc]").textContent.trim(), "0xdeadbeef");
        assert.match(body.querySelector("[data-embedded-time]").textContent, /99 ps/);
    });

    test("三个子视图入口都在，且带可被脚本取到的 data-* 标记", () => {
        const body = landEmbedded({});
        const entries = [...body.querySelectorAll("[data-embedded-entry]")];
        const targets = entries.map((el) => el.getAttribute("data-embedded-entry"));
        // ⚠️ **v0.65 批**：「寄存器 / 内存」那个独立视图撤销了 ⇒ ⇒ 入口从
        //   **四个**变**三个** ⇒ ⇒ 那些内容在**调试视图**里 ⇒ ⇒ 不是功能
        //   少了，只是**少一个重复的入口**。
        assert.deepEqual(
            targets.sort(),
            ["debug", "source", "waveform"],
            "三个子视图入口不全（或标记不对）",
        );
        // ⚠️ **不许留着指向已撤销视图的死入口** ⇒ ⇒ 那会让用户点到一个
        //   落到兜底页的死按钮。
        assert.ok(
            !targets.includes("regmem"),
            "还有指向已撤销的「寄存器 / 内存」视图的死入口",
        );
        for (const el of entries) {
            assert.equal(
                el.getAttribute("data-view-target"),
                el.getAttribute("data-embedded-entry"),
                "入口的目标视图与标记不一致",
            );
            assert.ok(el.getAttribute("data-action"), "入口不是可点的（没有 data-action）");
        }
    });
});

describe("落地视图（embedded）· 子视图入口能跳", () => {
    test("点击入口 ⇒ 由 M-01 路由到目标视图", () => {
        const body = landEmbedded({});
        assert.equal(activePanel(), "embedded", "起点不是落地视图");
        // ⚠️ DOM 替身**不支持后代选择器**（空格 / `>`）⇒ 先取视图体，再在
        // 视图体里查。
        const button = body.querySelector('[data-embedded-entry="debug"]');
        assert.ok(button, "找不到调试视图入口");
        button.dispatchEvent(new Event("click"));
        assert.equal(activePanel(), "debug", "点击入口后没有跳到调试视图");
    });

    test("三个入口逐个都能跳（不是一个死按钮）", () => {
        for (const target of ["debug", "waveform", "source"]) {
            const body = landEmbedded({});
            const button = body.querySelector(`[data-embedded-entry="${target}"]`);
            assert.ok(button, `找不到 ${target} 入口`);
            button.dispatchEvent(new Event("click"));
            assert.equal(activePanel(), target, `点击 ${target} 入口没有跳转`);
        }
    });
});

describe("落地视图（embedded）· 连接状态由 app 层推进", () => {
    test("app.publishLinkState 把传输层状态推进视图状态", () => {
        m03.reset();
        app.publishLinkState({ mode: "worker", connected: true, label: "worker transport" });
        const state = m03.getState("embedded");
        assert.equal(state.link.connected, true);
        assert.equal(state.link.mode, "worker");
        assert.equal(state.link.label, "worker transport");
    });

    test("未连接时推进的是「未连接」（不是缺字段）", () => {
        m03.reset();
        app.publishLinkState({ mode: "none", connected: false, lastError: "boom" });
        const state = m03.getState("embedded");
        assert.equal(state.link.connected, false);
        assert.equal(state.link.lastError, "boom");
    });

    test("落地视图与调试视图收到的是**同一份** M-10 推进的 PC", () => {
        // ⚠️ 「embedded」必须在 M-10 的推送名单里（app.js 的
        // `REGISTER_MEMORY_VIEW_IDS`）⇒ 否则落地视图拿不到 `sim.pc`。
        assert.ok(
            app.REGISTER_MEMORY_VIEW_IDS.includes("embedded"),
            "落地视图不在 M-10 的推送名单里 ⇒ 它拿不到 PC",
        );
    });
});
