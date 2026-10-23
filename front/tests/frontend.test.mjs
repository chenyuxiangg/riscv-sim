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
const m07 = await importFront("services/m07_transport.js");

/** Let the async bootstrap settle. */
const tick = () => new Promise((r) => setTimeout(r, 0));
await tick();
// The real DOM fires `DOMContentLoaded` after the deferred module scripts
// have run, so a module that registers a listener in its top level does
// receive it. The stub dispatches it explicitly for the same reason
// (FRONTEND_Review.md R-15).
let moduleLevelDomContentLoadedHits = 0;
dom.document.addEventListener("DOMContentLoaded", () => {
    moduleLevelDomContentLoadedHits += 1;
});
dom.fireDOMContentLoaded();
// 快照取自「模块加载完、首次派发」这一刻——后续自测里的额外派发不应
// 影响这个不变量。
const startupDomContentLoadedHits = moduleLevelDomContentLoadedHits;
const startupDomContentLoadedDispatches = dom.domContentLoadedDispatches;

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

    test("页脚由真实工作台状态派生（写死视图列表会被抓）", () => {
        // 复审 R-02：原来的断言是宽泛正则 `/role=(product|embedded)/`，
        // 硬编码的 "role=embedded" 同样匹配 → 对 #12 的回归完全无感知。
        // 改为绑定**真实**工作台快照里的 role 与全部视图名。
        const info = dom.document.getElementById("build-info");
        // 取自**生产模块**的只读快照，而不是 dev-only 全局——清理 dev
        // helper 属于正常重构，不该把门禁撞红（FRONTEND_Review.md R-18）。
        const workspace = m01.currentWorkspace();
        assert.ok(workspace, "缺少工作台快照");
        assert.ok(
            info.textContent.includes(`role=${workspace.role}`),
            `页脚 role 与工作台快照不符：${info.textContent}`,
        );
        for (const view of workspace.views) {
            assert.ok(
                info.textContent.includes(view.name),
                `页脚缺少视图「${view.name}」——写死的文案过不了这条断言`,
            );
        }
    });

    test("状态栏由 transport 状态派生（写死文案会被抓）", () => {
        // 复审 R-02：原断言只排除「启动失败」「未连接」两个字符串，
        // 降级文案同样满足 → 对 #11 的回归无感知。
        // 改为：① 状态栏文本必须等于**实际** transport 状态的推导结果；
        //       ② 推导结果必须随 transport 状态变化；
        //       ③ transport 缺失（挂载结果错绑）必须读起来不一样。
        const status = dom.document.getElementById("status");
        const transport = m07.getTransportStatus();
        assert.ok(transport, "缺少 transport 状态");
        assert.equal(
            status.textContent,
            app.describeTransport(transport),
            "状态栏文案与 describeTransport 的输出不一致——它不是推导出来的",
        );
        assert.notEqual(
            app.describeTransport({ mode: "none", connected: false }),
            app.describeTransport({ mode: "worker", connected: true }),
            "describeTransport 必须随 transport 状态变化",
        );
        assert.notEqual(
            app.describeTransport({ mode: "none", connected: false }),
            app.describeTransport(undefined),
            "transport 缺失必须读起来不一样——否则挂载结果错绑不可见",
        );
        // renderStatus 必须真的把推导结果写进 DOM
        const scratch = dom.document.createElement("span");
        app.renderStatus({ mode: "worker", connected: true }, scratch);
        assert.match(scratch.textContent, /已连接/);
    });

    test("M-01 不用 innerHTML 拼面板（不得把数据拼进 HTML 字符串）", () => {
        // 复审 R-02：#10（createPanel 改回 innerHTML 字符串拼接）此前
        // 门禁完全无感知。这条把「M-01 只用 createElement + dataset
        // 构造面板」锁成可回归的约定。
        const src = readFileSync(resolve(frontDir, "views/m01_workbench.js"), "utf8");
        // 只匹配**赋值**（`x.innerHTML =`），不匹配散文——否则源码里一句
        // 「这里不用 innerHTML」的注释就会把门禁撞红（FRONTEND_Review.md
        // R-17：过宽的静态断言 = 假红）。
        assert.ok(
            !/\.innerHTML\s*=/.test(src),
            "M-01 不应通过 innerHTML 拼接面板内容（#10）",
        );
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

    test("UI 交互态只采集真实存在的滚动容器", () => {
        // 复审 R-04：`.view-body` 没有 overflow，不是滚动容器，它的
        // scrollTop 在真实浏览器里恒为 0 ——采集它等于采集一个死值。
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        m01.route("#/product");
        m01.route("#/terminal");
        const ui = m03.getState("terminal").ui;
        assert.deepEqual(
            Object.keys(ui).sort(),
            ["terminalScrollTop"],
            `采集了不存在的滚动容器：${Object.keys(ui).join(", ")}`,
        );
    });

    test("同视图重入也还原 UI 交互态", () => {
        // 复审 R-15：同视图分支会重建 DOM（滚动位置被重置），必须同样
        // 走 restoreUiState。
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        $("[data-terminal-out]").scrollTop = 42;
        m01.route("#/terminal");
        assert.equal($("[data-terminal-out]").scrollTop, 42, "同视图重入未还原 UI 交互态");
    });

    test("UI 交互态按键合并（不整块替换 ui）", () => {
        // 复审 R-06：`setState` 是浅 merge，写 `ui` 时整块替换会抹掉
        // 将来新增的子键（如 zoom / 折叠态）。
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        m03.setState("terminal", { ui: { zoom: 2 } });
        m01.route("#/product");
        m01.route("#/terminal");
        assert.equal(m03.getState("terminal").ui.zoom, 2, "ui 子键被整块替换抹掉了");
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

    test("终端增量在「重渲染夹击」下不重复追加（store 为唯一权威）", () => {
        // 复审 R-07：旧实现先写 store、再基于 DOM 当前值 `+= bytes`。
        // 一旦有订阅者在写入后触发一次重渲染（将来的 M-07 接线即如此），
        // DOM 会先从 store 重建（已含该字节）再被追加一次 → 重复。
        m01.assembleWorkspace("embedded");
        m01.route("#/terminal");
        m03.setState("terminal", { term: { bytes: "" } });
        const terminal = m02.get("terminal");
        const off = m03.subscribe("terminal", () => {
            const out = $("[data-terminal-out]");
            if (out) out.textContent = m03.getState("terminal")?.term?.bytes ?? "";
        });
        terminal.onDelta({ termDelta: { bytes: "<a>" } });
        off();
        const text = $("[data-terminal-out]").textContent;
        const occurrences = text.split("<a>").length - 1;
        assert.equal(occurrences, 1, `终端内容重复追加：${JSON.stringify(text)}`);
        assert.equal(m03.getState("terminal").term.bytes, "<a>");
    });

    test("demo 按钮可点击（seedDemoBus 未定义回归）", () => {
        m01.assembleWorkspace("embedded");
        m01.route("#/product");
        // 走**生产信号**而不是 dev-only 全局——门禁不该依赖
        // window.riscv_demo（FRONTEND_Review.md R-18）。
        dom.window.dispatchEvent(new CustomEvent("riscv:seed", { detail: { viewId: "product" } }));
        const button = $("[data-action='seed-demo']");
        assert.ok(button, "demo 按钮未渲染");

        // The click must actually reach the handler: the button emits the
        // `riscv:seed` signal, and the demo data is re-seeded.
        let signals = 0;
        const listener = () => { signals += 1; };
        dom.window.addEventListener("riscv:seed", listener);
        // ⚠️ **不再断言 `sim.memoryMap`**（STAGE-14 · v0.29）：demo 里的那份
        // **假内存映射**已经删掉（规格 §2.6：内存映射表显示内核 `memory.map`
        // 事件里的当前生效图 ⇒ demo 塞一份假图就是第二个真相源）⇒ 这里改断言
        // demo **产品树**被重新 seed（同一件事：点击确实走到了 handler）。
        m03.setState("product", { tree: { root: { name: "被清空", children: [] } } });
        // A handler that throws would propagate out of dispatchEvent.
        button.dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:seed", listener);

        assert.equal(signals, 1, "点击 demo 按钮没有发出 riscv:seed 信号");
        assert.equal(
            m03.getState("product").tree.root.children.length, 2,
            "点击 demo 按钮没有重新 seed demo 状态",
        );
        assert.equal(
            m03.getState("product").sim.memoryMap, undefined,
            "demo 状态里不该再有假内存映射（内存映射表的唯一数据源是内核事件）",
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

// ---------------------------------------------------------------------------
// DOM stub 保真度自测（FRONTEND_Review.md R-03 / R-05 / R-12 / R-13 / R-14）
//
// stub 本身就是门禁的基础设施——它保真度不足会让整套前端门禁「假绿」。
// 复审实测出的 5 处缺口在此各锁一条，防止以后再退化。
// ---------------------------------------------------------------------------

describe("DOM stub 保真度", () => {
    test("选择器算符按真实语义匹配（^= / $= / *= / ~= / |=）", () => {
        const debugTab = dom.document.querySelector('[data-view-tab="debug"]');
        assert.ok(debugTab, "缺少 debug 标签");
        assert.equal(debugTab.matches('[data-view-tab^="deb"]'), true);
        assert.equal(debugTab.matches('[data-view-tab$="bug"]'), true);
        assert.equal(debugTab.matches('[data-view-tab*="ebu"]'), true);
        assert.equal(debugTab.matches('[data-view-tab="product"]'), false);
        // 旧实现把 ^= 退化成「属性存在即匹配」→ 会选中 product
        assert.equal(
            dom.document.querySelector('[data-view-tab^="deb"]')?.getAttribute("data-view-tab"),
            "debug",
        );
    });

    test("未实现的选择器语法显式抛错（不静默假绿）", () => {
        assert.throws(
            () => dom.document.querySelector('[data-view-tab@="debug"]'),
            /不支持的选择器/,
        );
    });

    test("{ once: true } 监听只触发一次", () => {
        const el = dom.document.createElement("div");
        let hits = 0;
        el.addEventListener("click", () => { hits += 1; }, { once: true });
        el.dispatchEvent(new Event("click"));
        el.dispatchEvent(new Event("click"));
        assert.equal(hits, 1);
    });

    test("stopPropagation 阻止事件到达祖先", () => {
        const parent = dom.document.createElement("div");
        const child = dom.document.createElement("span");
        parent.appendChild(child);
        let bubbled = 0;
        parent.addEventListener("click", () => { bubbled += 1; });
        child.addEventListener("click", (ev) => { ev.stopPropagation(); });
        child.dispatchEvent(new Event("click"));
        assert.equal(bubbled, 0, "stopPropagation 未被尊重");
    });

    test("document 支持 addEventListener，DOMContentLoaded 在派发后被收到", () => {
        assert.equal(typeof dom.document.addEventListener, "function");
        let hits = 0;
        const listener = () => { hits += 1; };
        dom.document.addEventListener("DOMContentLoaded", listener);
        dom.fireDOMContentLoaded();
        dom.document.removeEventListener("DOMContentLoaded", listener);
        dom.fireDOMContentLoaded();
        assert.equal(hits, 1, "DOMContentLoaded 派发后监听未收到（或未随 removeEventListener 失效）");
    });

    test("DOMContentLoaded 在模块脚本之后才派发，且只派发一次", () => {
        // R-15：stub 若在 installDom 时就派发，模块顶层注册的监听器永远
        // 收不到 —— 前端会「浏览器跑、门禁不跑」。
        assert.equal(startupDomContentLoadedHits, 1, "模块顶层注册的监听器没收到事件");
        assert.equal(
            startupDomContentLoadedDispatches, 1,
            "DOMContentLoaded 派发次数异常——不得在模块脚本之前自动派发",
        );
    });

    test("document 的 capture 监听器也会触发", () => {
        let hits = 0;
        const listener = () => { hits += 1; };
        dom.document.addEventListener("custom-evt", listener, { capture: true });
        dom.document.dispatchEvent(new Event("custom-evt"));
        assert.equal(hits, 1, "document 的 capture 监听器未触发");
    });

    test("removeEventListener 只移除同一相位的监听", () => {
        // R-19：DOM 规范要求按 (type, callback, capture) 三元组匹配；
        // 旧实现只看 handler，会误删另一相位的注册。
        // 监听挂在**祖先**上——挂在事件目标上时两个相位都属 AT_TARGET(2)，
        // 无法区分。
        const parent = dom.document.createElement("div");
        const child = dom.document.createElement("span");
        parent.appendChild(child);
        const phases = [];
        const handler = (ev) => { phases.push(ev.eventPhase); };
        parent.addEventListener("x", handler, { capture: true });  // 捕获相位
        parent.addEventListener("x", handler);                    // 冒泡相位
        parent.removeEventListener("x", handler);                 // 只移除冒泡相位
        child.dispatchEvent(new Event("x"));
        assert.deepEqual(phases, [1], "removeEventListener 移除了错误相位的监听");
    });

    test("window.location 可用且与全局 location 同源", () => {
        assert.equal(dom.window.location, dom.location);
    });

    test("textContent 赋值后 innerHTML 返回转义文本", () => {
        const el = dom.document.createElement("div");
        el.textContent = "<b>&";
        assert.equal(el.innerHTML, "&lt;b&gt;&amp;");
    });
});
