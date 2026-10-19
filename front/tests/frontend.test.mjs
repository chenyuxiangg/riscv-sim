// Front-end smoke gate (FRONTEND_Review.md #16).
//
// The front end had **no** automated check of any kind: 203 Rust tests
// were green while the page was 100% blank. This suite is the cheapest
// possible net — it boots the real front end against a minimal DOM
// double and asserts the behaviour the review found broken.
//
// Run: `node --test front/tests/`  (or `tools/check-front.sh`)

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");

// The DOM globals must exist **before** the front-end modules are
// imported — `app.js` reads `document.getElementById("status")` at
// module scope.
const dom = installDom(indexHtml);

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");

/** Let the async bootstrap settle. */
const tick = () => new Promise((r) => setTimeout(r, 0));
await tick();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);
const panelVisible = (hash) => {
    const panel = dom.document.querySelector(`[data-view-panel="${hash}"]`);
    return panel ? !panel.classList.contains("hidden") : null;
};
const currentTabs = () =>
    $$("[data-view-tab]").filter((t) => t.getAttribute("aria-current") === "page")
        .map((t) => t.getAttribute("data-view-tab"));

// ---------------------------------------------------------------------------

describe("bootstrap", () => {
    test("app.js 链接成功并完成挂载（不再白屏）", () => {
        // The import above *is* the link check: a missing export throws a
        // SyntaxError at link time, before any code runs. That was the
        // root cause of the blank page (D-01 / D-02).
        assert.ok(app, "app.js 未能加载");
        assert.equal(
            dom.document.getElementById("status").textContent.startsWith("启动失败"),
            false,
            "bootstrap 失败",
        );
    });

    test("七个视图面板全部挂载，状态栏不再是初始文案", () => {
        const panels = $$("[data-view-panel]").map((p) => p.getAttribute("data-view-panel"));
        for (const hash of ["product", "embedded", "debug", "regmem", "waveform", "terminal", "fallback"]) {
            assert.ok(panels.includes(hash), `缺少面板 ${hash}`);
        }
        assert.notEqual(dom.document.getElementById("status").textContent, "未连接");
    });

    test("页脚文案由 JS 填充，不再写死阶段号", () => {
        const info = dom.document.getElementById("build-info");
        assert.ok(info, "缺少 #build-info");
        assert.match(info.textContent, /role=(product|embedded)/);
    });

    test("恰好一个视图面板可见，且与活动标签一致（不得停在空白主区）", () => {
        // `mountPanels()` hides every panel; the very first route lands in
        // `switchView`'s "same view" branch, which has to re-show it —
        // otherwise the main area stays blank (FRONTEND_Review.md #19).
        const visible = $$("[data-view-panel]").filter((p) => !p.classList.contains("hidden"));
        assert.equal(visible.length, 1, `应当恰好有一个面板可见，实际 ${visible.length}`);
        assert.equal(visible[0].getAttribute("data-view-panel"), currentTabs()[0]);
    });
});

describe("M-03 视图状态管理", () => {
    test("未访问过的 view 返回 undefined", () => {
        assert.equal(m03.getState("never-visited"), undefined);
    });

    test("setState 是 merge 语义，不是替换", () => {
        m03.setState("unit-merge", { a: 1, b: 2 });
        m03.setState("unit-merge", { b: 3 });
        assert.deepEqual(m03.getState("unit-merge"), { a: 1, b: 3 });
    });

    test("setState 接受函数式派生", () => {
        m03.setState("unit-fn", { n: 1 });
        m03.setState("unit-fn", (prev) => ({ n: prev.n + 1 }));
        assert.deepEqual(m03.getState("unit-fn"), { n: 2 });
    });

    test("subscribe 收到变更，unsubscribe 后不再收到", () => {
        const seen = [];
        const off = m03.subscribe("unit-sub", (next, prev) => seen.push([next, prev]));
        m03.setState("unit-sub", { x: 1 });
        off();
        m03.setState("unit-sub", { x: 2 });
        assert.equal(seen.length, 1);
        assert.deepEqual(seen[0][0], { x: 1 });
    });
});

describe("M-01 路由与能力位复校验", () => {
    test("未注册的 hash 回落 fallback 视图", () => {
        m01.assembleWorkspace("embedded");
        const evt = m01.route("#/no-such-view");
        assert.equal(evt.viewId, "fallback");
        assert.equal(panelVisible("fallback"), true);
    });

    test("能力位不允许的视图被拒绝并回落", () => {
        m01.assembleWorkspace("product");
        const evt = m01.route("#/debug");
        assert.equal(evt.viewId, "fallback");
        m01.assembleWorkspace("embedded");
    });

    test("角色决定可见视图集合", () => {
        const product = m01.assembleWorkspace("product");
        const embedded = m01.assembleWorkspace("embedded");
        assert.ok(product.views.some((v) => v.hash === "product"));
        assert.ok(!product.views.some((v) => v.hash === "debug"));
        assert.ok(embedded.views.some((v) => v.hash === "debug"));
    });
});

describe("M-01 视图切换", () => {
    test("活动标签页高亮且唯一", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        assert.deepEqual(currentTabs(), ["debug"]);
    });

    test("重入当前视图仍然派发 render 事件", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/waveform");
        let events = 0;
        const listener = () => { events += 1; };
        dom.window.addEventListener("riscv:render", listener);
        m01.route("#/waveform");
        dom.window.removeEventListener("riscv:render", listener);
        assert.ok(events >= 1, "重入当前视图没有派发 render 事件");
    });

    test("能力位翻转后当前面板仍然可见（refreshAll 不得白屏）", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/debug");
        assert.equal(panelVisible("debug"), true);
        m01.setCapabilities([
            "view.productTree", "view.debug", "view.waveform", "view.terminal", "view.regmem",
        ]);
        assert.equal(panelVisible("debug"), true, "能力位翻转后当前视图被藏掉了");
    });

    test("UI 交互态随切换保存并还原", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        const out = $("[data-terminal-out]");
        assert.ok(out, "终端视图未渲染");
        out.scrollTop = 42;
        m01.route("#/product");
        m01.route("#/terminal");
        assert.equal($("[data-terminal-out]").scrollTop, 42);
        assert.equal(m03.getState("terminal").ui.terminalScrollTop, 42);
    });
});

describe("M-02 视图组件", () => {
    test("终端增量不双层转义，且同步进 store", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        const terminal = m02.get("terminal");
        terminal.onDelta({ termDelta: { bytes: "<x>&y" } });
        const text = $("[data-terminal-out]").textContent;
        assert.ok(text.includes("<x>&y"), `终端内容被错误转义：${text}`);
        assert.ok(!text.includes("&lt;"), "终端内容出现字面 &lt;");
        assert.equal(m03.getState("terminal").term.bytes, "<x>&y");
    });

    test("demo 按钮可点击（seedDemoBus 未定义回归）", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/product");
        dom.window.riscv_demo.seedBus();
        const button = $("[data-action='seed-demo']");
        assert.ok(button, "demo 按钮未渲染");

        // The click must actually reach the handler: the button emits the
        // `riscv:seed` signal, and the demo data is re-seeded.
        let signals = 0;
        const listener = () => { signals += 1; };
        dom.window.addEventListener("riscv:seed", listener);
        m03.setState("product", { sim: { memoryMap: [] } });
        // A handler that throws would propagate out of dispatchEvent.
        button.dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:seed", listener);

        assert.equal(signals, 1, "点击 demo 按钮没有发出 riscv:seed 信号");
        assert.equal(
            m03.getState("product").sim.memoryMap.length, 3,
            "点击 demo 按钮没有重新 seed 内存映射",
        );
    });
});

describe("静态检查", () => {
    test("escapeHtml 只有一份实现", () => {
        const sources = [
            "lib/escape.js",
            "views/m01_workbench.js",
            "views/m02_viewcomponents.js",
        ].map((rel) => readFileSync(resolve(frontDir, rel), "utf8"));
        const definitions = sources
            .map((src) => (src.match(/function escapeHtml/g) ?? []).length)
            .reduce((a, b) => a + b, 0);
        assert.equal(definitions, 1, "escapeHtml 存在多份实现");
    });

    test("index.html 含 file:// 部署提示", () => {
        assert.match(indexHtml, /location\.protocol === "file:"/);
    });

    test("前端测试不再是零覆盖", () => {
        const files = readFileSync(resolve(frontDir, "tests/frontend.test.mjs"), "utf8");
        assert.ok(files.includes("describe("));
        assert.ok(resolve(repoRoot, "tools/check-front.sh"));
    });
});
