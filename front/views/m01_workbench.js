// M-01 工作台装配与多视图路由 (Workbench assembly & multi-view routing).
//
// Implements LLD §3.1:
//   - assembleWorkspace(role)   — 视图组合 × 能力位 × 通道可见性
//   - route(hash)               — hash 路由 + 复校验 → 兜底 E_CTX-05
//   - switchView(target)        — 状态保持 + hash 写回 + RenderEvent 派发
//
// MVP scope (this slice):
//   - role → 默认 'embedded'，URL `?role=product` 可覆盖
//   - capBits → 先内置 role 默认集合，M-05 negotiate 接入后改从协议取
//   - RenderEvent → 派发到 `window` 的 `riscv:render` CustomEvent，
//     M-02 视图组件注册监听 (M-01 §01-8)
//   - 不实现 admin / 多能力位组合 (评审 01-5)

import * as viewStore from "./m03_viewstore.js";

// ---------------------------------------------------------------------------
// Capability bits — coarse MVP set; will be sourced from M-05 negotiate later.
// ---------------------------------------------------------------------------

/** @type {ReadonlyMap<string, ReadonlySet<string>>} */
const ROLE_CAPABILITIES = new Map([
    ["product", new Set([
        "view.productTree",
        "view.waveform",
        "view.terminal",
    ])],
    ["embedded", new Set([
        "view.productTree",
        "view.debug",
        "view.waveform",
        "view.terminal",
        "view.source",
    ])],
]);

/**
 * Explicit role → default view mapping.
 *
 * The role name and the view hash used to be spelled the same way, so
 * `mountedViews.get(role)` happened to work — a coincidence, not a
 * contract. A role whose name differs from its view hash would have
 * silently fallen back to "the first mounted view"
 * (FRONTEND_Review.md #14).
 *
 * @type {ReadonlyMap<string, string>}
 */
const ROLE_DEFAULT_VIEW = new Map([
    ["product", "product"],
    ["embedded", "embedded"],
]);

/** @type {ReadonlySet<string>} */
const FALLBACK_CAPS = new Set([
    "view.productTree",
    "view.debug",
    "view.waveform",
    "view.terminal",
]);

/** Pick the default capability set for a given role. */
function capBitsFor(role) {
    return ROLE_CAPABILITIES.get(role) ?? FALLBACK_CAPS;
}

// ---------------------------------------------------------------------------
// View registry — hash → View descriptor (M-01 §2.1.1)
// ---------------------------------------------------------------------------

/**
 * @typedef {object} ViewDescriptor
 * @property {string} hash      — viewId, used as the canonical route
 * @property {string} name      — display name
 * @property {string} capability — required capability bit
 * @property {string} panel     — DOM selector (`[data-view-panel="..."]`)
 * @property {string} role      — workbench role this view lives under
 * @property {string} group     — **侧边栏分组**（`run` / `platform`）
 * @property {string} hint      — 面板标题右边那行**提示**
 */

/**
 * 侧边栏的**分组**（STAGE-34 · v0.49 批 · 裁决 C）。
 *
 * ⚠️ **分组只有一个落点**：`VIEW_REGISTRY` 每项的 `group` 字段 ⇒ ⇒ 侧边栏
 * **按它渲染** ⇒ ⇒ 不另设一张「视图 → 分组」的表 ⇒ ⇒ 那是**第二个真相**。
 *
 * ⚠️ **顺序也是落点**：这里列出来的顺序 = 侧边栏里的上下顺序。
 */
const NAV_GROUPS = [
    { id: "run", label: "运行与调试" },
    { id: "platform", label: "平台" },
];

/**
 * 侧边栏导航项的**内联 SVG 图标**（裁决 I —— 零依赖 ⇒ ⇒ 不用图标字体库、
 * 不用图片外链）。
 *
 * ⚠️ **形状是常量**（不含任何用户数据）⇒ ⇒ 用 DOM 调用逐个造（**不是**
 * `innerHTML` 拼字符串）⇒ ⇒ ① 守住 M-01「不拼 HTML 字符串」那条既有门禁
 * ② SVG 元素在真浏览器里**只能**用 `createElementNS` 造（`createElement`
 * 出来的是 HTML 元素，**不渲染**）。
 *
 * @type {Readonly<Record<string, [string, Record<string, string>][]>>}
 */
const SVG_NS = "http://www.w3.org/2000/svg";
const NAV_ICONS = {
    embedded: [
        ["rect", { x: "3", y: "3", width: "7", height: "7", rx: "1.5" }],
        ["rect", { x: "14", y: "3", width: "7", height: "7", rx: "1.5" }],
        ["rect", { x: "3", y: "14", width: "7", height: "7", rx: "1.5" }],
        ["rect", { x: "14", y: "14", width: "7", height: "7", rx: "1.5" }],
    ],
    debug: [["path", { d: "M9 3l6 9-6 9" }]],
    waveform: [["path", { d: "M2 12h4l2-6 4 12 3-9 2 3h5" }]],
    terminal: [
        ["rect", { x: "3", y: "4", width: "18", height: "16", rx: "3" }],
        ["path", { d: "M7 9l3 3-3 3M13 15h4" }],
    ],
    product: [
        ["rect", { x: "4", y: "10", width: "16", height: "11", rx: "2" }],
        ["path", { d: "M8 10V7a4 4 0 018 0v3" }],
    ],
    "memory-map": [
        ["rect", { x: "3", y: "5", width: "18", height: "14", rx: "2" }],
        ["path", { d: "M3 12h18M9 5v14M15 5v14" }],
    ],
    source: [["path", { d: "M8 7l-4 5 4 5M16 7l4 5-4 5M13 5l-2 14" }]],
};

/**
 * 造一个导航项的 **SVG 图标**。
 *
 * ⚠️ **逐个 DOM 调用**（不是 `innerHTML`）⇒ ⇒ ① 形状是常量、不含用户数据
 * ② M-01 那条「不拼 HTML 字符串」的既有门禁**一字不改**仍然成立。
 *
 * @param {string} hash — 视图 hash（决定用哪个图标）
 * @returns {SVGElement|Element|null}
 */
function buildNavIcon(hash) {
    const shapes = NAV_ICONS[hash];
    if (!shapes || typeof document?.createElementNS !== "function") return null;
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("width", "15");
    svg.setAttribute("height", "15");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "1.8");
    for (const [tag, attrs] of shapes) {
        const node = document.createElementNS(SVG_NS, tag);
        for (const [k, val] of Object.entries(attrs)) node.setAttribute(k, val);
        svg.appendChild(node);
    }
    return svg;
}

/** @type {ReadonlyMap<string, ViewDescriptor>} */
const VIEW_REGISTRY = new Map([
    ["product", {
        hash: "product",
        name: "产品视图",
        capability: "view.productTree",
        panel: '[data-view-panel="product"]',
        role: "product",
        group: "platform",
        hint: "器件树 · 板级内存图",
    }],
    ["embedded", {
        hash: "embedded",
        name: "嵌入式工作台",
        capability: "view.productTree",
        panel: '[data-view-panel="embedded"]',
        role: "embedded",
        group: "run",
        hint: "PC · 虚拟时间 · 三个子视图入口",
    }],
    ["debug", {
        hash: "debug",
        name: "调试视图",
        capability: "view.debug",
        panel: '[data-view-panel="debug"]',
        role: "embedded",
        group: "run",
        hint: "反汇编 · 断点 · trace · 寄存器 / 内存",
    }],
    ["waveform", {
        hash: "waveform",
        name: "波形视图",
        capability: "view.waveform",
        panel: '[data-view-panel="waveform"]',
        role: "embedded",
        group: "run",
        hint: "通道列表与边沿计数",
    }],
    // 源码视图（STAGE-7 裁决 E / F）：源码树**在前端本地**（不进内核），
    // 视图只负责「装载 + 高亮」。
    ["source", {
        hash: "source",
        name: "源码视图",
        capability: "view.source",
        panel: '[data-view-panel="source"]',
        role: "embedded",
        group: "platform",
        hint: "本地源码树 · 行号高亮",
    }],
    ["terminal", {
        hash: "terminal",
        name: "串口终端",
        capability: "view.terminal",
        panel: '[data-view-panel="terminal"]',
        role: "product",
        group: "run",
        hint: "字节流实时上屏 · 可向 guest 打字",
    }],
    // 板级内存图编辑器（STAGE-14 · v0.29）。
    //
    // ⚠️ **独立工作面**：编辑器**不**塞进产品树视图（规格 §3.3）⇒ 它是
    // 自己的一条路由 + 自己的一块面板。
    //
    // ⚠️ **能力位复用 `view.productTree`**（两个角色都有它）⇒ 不新增能力位
    // 概念、也不动内核的 `KERNEL_CAP_BITS`（那是内核的分级数值，与前端的
    // 字符串能力位是两个不同的东西）。
    ["memory-map", {
        hash: "memory-map",
        name: "板级内存图",
        capability: "view.productTree",
        panel: '[data-view-panel="memory-map"]',
        role: "product",
        group: "platform",
        hint: "段表编辑 · 差量提交 · 板档案",
    }],
]);

const FALLBACK_VIEW = {
    hash: "fallback",
    name: "未识别 / 无权限",
    capability: "view.productTree",
    panel: '[data-view-panel="fallback"]',
    role: "product",
    group: "platform",
    hint: "路由或能力位不认识这一屏",
};

// ---------------------------------------------------------------------------
// Workbench state (per-session)
// ---------------------------------------------------------------------------

/** @type {string} */
let currentRole = "embedded";
/** @type {Set<string>} */
let currentCapBits = new Set(FALLBACK_CAPS);
/** @type {ViewDescriptor|null} */
let activeView = VIEW_REGISTRY.get("embedded") ?? null;
/** @type {Map<string, ViewDescriptor>} cached mounted views per assemble */
const mountedViews = new Map();

/**
 * 抽屉开关**只订阅一次**的标志（STAGE-34 · v0.49 批）。
 * @type {boolean}
 */
let sidebarToggleWired = false;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Resolve `role` from URL `?role=` query string, falling back to the
 * previous session default or 'embedded'.
 *
 * @param {Location} [loc]
 * @returns {"product"|"embedded"}
 */
export function resolveRole(loc = location) {
    const params = new URLSearchParams(loc.search);
    const r = params.get("role");
    if (r === "product" || r === "embedded") return r;
    return currentRole;
}

/**
 * Assemble a workbench for the given role. Returns a snapshot of the
 * assembled views + active view. Failures (unknown role) fall back to
 * the embedded workbench (LLD §3.1 + §4.1.1.5).
 *
 * @param {"product"|"embedded"} role
 */
export function assembleWorkspace(role) {
    if (role !== "product" && role !== "embedded") {
        console.warn("[M-01] unknown role, falling back to embedded", role);
        role = "embedded";
    }
    currentRole = role;
    currentCapBits = new Set(capBitsFor(role));
    mountedViews.clear();
    for (const v of VIEW_REGISTRY.values()) {
        if (currentCapBits.has(v.capability)) {
            mountedViews.set(v.hash, v);
        }
    }
    if (!activeView || !mountedViews.has(activeView.hash)) {
        const defaultHash = ROLE_DEFAULT_VIEW.get(role);
        activeView = (defaultHash ? mountedViews.get(defaultHash) : null)
            ?? [...mountedViews.values()][0]
            ?? null;
    }
    mountPanels();
    renderSidebar();
    wireSidebarToggle();
    return currentWorkspace();
}

/**
 * Read-only snapshot of the currently assembled workspace.
 *
 * The bootstrap fills the footer from it, and the front-end gate asserts
 * against it — the gate must not depend on a **dev-only** global
 * (FRONTEND_Review.md R-18: removing the dev helpers is a legitimate
 * refactor and must not turn the gate red).
 *
 * @returns {{role: string, capBits: string[], views: {hash: string, name: string, group: string}[], activeView: {hash: string, name: string}|null}}
 */
export function currentWorkspace() {
    return {
        role: currentRole,
        capBits: [...currentCapBits],
        // ⚠️ **带上 `group`**（STAGE-34 · v0.49 批）⇒ ⇒ 前端门禁据此断言
        // 「每个视图都有分组」⇒ ⇒ 分组**只有一个落点**（`VIEW_REGISTRY`）。
        views: [...mountedViews.values()].map((v) => ({
            hash: v.hash,
            name: v.name,
            group: v.group,
        })),
        activeView: activeView ? { hash: activeView.hash, name: activeView.name } : null,
    };
}

/**
 * Hash router (LLD §3.2). Returns the RenderEvent envelope.
 *
 * @param {string} hash — raw `location.hash` (incl. leading `#`)
 */
export function route(hash) {
    const viewId = (hash || "").replace(/^#\/?/, "").split("/")[0]
        || ROLE_DEFAULT_VIEW.get(currentRole)
        || currentRole;
    const target = VIEW_REGISTRY.get(viewId);
    if (!target) {
        logEctx05("unregistered-route", hash);
        return switchView(FALLBACK_VIEW);
    }
    if (!currentCapBits.has(target.capability)) {
        logEctx05("capability-denied", hash);
        return switchView(FALLBACK_VIEW);
    }
    return switchView(target);
}

/**
 * Update capability bits at runtime (e.g. when M-05 negotiate completes
 * and reports the real session capabilities). Re-runs the active route
 * to ensure the current view is still permitted; if not, snaps to the
 * role default view.
 *
 * @param {Iterable<string>} caps
 */
export function setCapabilities(caps) {
    currentCapBits = new Set(caps);
    if (activeView && !currentCapBits.has(activeView.capability)) {
        route(`#/${currentRole}`);
    }
    // Re-render against the new capabilities: views that just became
    // permitted need their panels mounted, and the active view may be
    // stale (FRONTEND_Review.md #6).
    refreshAll();
}

/**
 * Switch the active view (LLD §3.3). Returns the dispatched RenderEvent.
 *
 * @param {ViewDescriptor} target
 */
export function switchView(target) {
    if (activeView && activeView.hash === target.hash) {
        // Same view: still re-dispatch so the content refreshes — the old
        // `noop` return left the view stale when the route was re-entered
        // (FRONTEND_Review.md #6).
        const evt = {
            viewId: target.hash,
            type: "update",
            state: viewStore.getState(target.hash) ?? {},
        };
        dispatchRenderEvent(evt);
        // Re-show the panel: `mountPanels()` hides every panel, and the
        // bootstrap path lands here on the very first route (the initial
        // view is pre-selected), so skipping this leaves the main area
        // blank (FRONTEND_Review.md #19).
        showPanel(target);
        // The re-render rebuilt the DOM — put the view's UI interaction
        // state back (FRONTEND_Review.md R-15).
        restoreUiState(target.hash);
        syncTabState(target.hash);
        return evt;
    }
    // Capture the outgoing view's UI interaction state (LLD §3.3).
    // The kernel state already lives in M-03 — what is missing there is
    // the *UI* state (scroll positions, folding), which only the DOM
    // knows about (FRONTEND_Review.md #7).
    if (activeView) {
        captureUiState(activeView.hash);
    }
    // Restore / initialise the target view state.
    let state = viewStore.getState(target.hash);
    let isFirstEnter = state === undefined;
    if (isFirstEnter) {
        state = {};
        viewStore.setState(target.hash, state);
    }
    activeView = target;
    // Sync the URL hash (replace, do not pollute history — 评审 01-4).
    const newHash = `#/${target.hash}`;
    if (location.hash !== newHash) {
        try {
            history.replaceState(null, "", newHash);
        } catch (_e) {
            location.hash = newHash;
        }
    }
    // Toggle panel visibility.
    showPanel(target);
    // Highlight the active tab.
    syncTabState(target.hash);
    const evt = {
        viewId: target.hash,
        type: isFirstEnter ? "mount" : "update",
        state,
    };
    dispatchRenderEvent(evt);
    // Put the view's UI interaction state back after the re-render.
    restoreUiState(target.hash);
    return evt;
}

/**
 * Re-render all currently-mounted views against fresh state (e.g. when
 * a capability flip lands). The active view gets a full re-render; the
 * rest are dispatched as `update` events so the components can decide
 * to refresh.
 */
export function refreshAll() {
    mountPanels();
    renderSidebar();
    if (!activeView) return;
    // mountPanels() hides every panel — the active one has to be shown
    // again, otherwise a capability flip blanks the screen.
    showPanel(activeView);
    dispatchRenderEvent({
        viewId: activeView.hash,
        type: "update",
        state: viewStore.getState(activeView.hash) ?? {},
    });
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** Mount the per-view panels into the DOM (idempotent). */
function mountPanels() {
    for (const v of VIEW_REGISTRY.values()) {
        if (!currentCapBits.has(v.capability)) continue;
        let panel = document.querySelector(v.panel);
        if (!panel) {
            panel = createPanel(v);
            document.getElementById("app")?.appendChild(panel);
        }
    }
    // Always ensure a fallback panel exists.
    if (!document.querySelector(FALLBACK_VIEW.panel)) {
        const fb = createPanel(FALLBACK_VIEW);
        document.getElementById("app")?.appendChild(fb);
    }
    // Hide every panel; the active one is shown in showPanel().
    for (const panel of document.querySelectorAll("[data-view-panel]")) {
        panel.classList.add("hidden");
    }
}

function createPanel(v) {
    const el = document.createElement("section");
    el.dataset.viewPanel = v.hash;
    el.classList.add("view", "hidden");
    // Build the panel with DOM calls instead of an HTML string: the
    // view name and hash then never travel through a markup parser
    // (FRONTEND_Review.md #10).
    //
    // ⚠️ **STAGE-34 · v0.49 批 · 裁决 G**：外框换成原型的
    // `.vhead`（标题 + 提示）**加** `.card`（内容容器）。
    // ⚠️ **标题文字一个字都不改**（仍是 `v.name`）⇒ ⇒ 前端门禁对页脚
    // 文案的那条断言**零改动**。
    const head = document.createElement("div");
    head.className = "vhead";
    // ⚠️ **元素是 `h2` 不是 `h1`** ⇒ ⇒ 真机门禁的落地页 / 渲染 / 源码三段
    // 都用 `active.querySelector("h2")` 取标题 ⇒ ⇒ 换成 `h1` 会让那**五条**
    // 断言**悄悄变红**（判据口径变了）⇒ ⇒ **保留 `h2`** ⇒ ⇒ 92 条断言
    // **一字不改**仍然成立（STAGE-34 §8 订正 4）。
    const heading = document.createElement("h2");
    heading.textContent = v.name;
    head.appendChild(heading);
    if (v.hint) {
        const hint = document.createElement("span");
        hint.className = "hint";
        hint.textContent = v.hint;
        head.appendChild(hint);
    }
    const card = document.createElement("div");
    card.className = "card";
    const body = document.createElement("div");
    body.className = "view-body";
    body.dataset.viewBody = v.hash;
    card.appendChild(body);
    el.appendChild(head);
    el.appendChild(card);
    return el;
}

/**
 * 渲染**左侧分组导航**（STAGE-34 · v0.49 批 · 裁决 C）。
 *
 * ⚠️ **分组只有一个落点**：`VIEW_REGISTRY` 每项的 `group` 字段 ⇒ ⇒ 这里
 * **按它分组** ⇒ ⇒ 不另设一张「视图 → 分组」的表（那是第二个真相）。
 * ⚠️ **只显示当前角色**挂载了的视图（`mountedViews`）⇒ ⇒ 侧边栏与
 * `mountPanels()` 的**口径一致** ⇒ ⇒ 不会出现「点得动但没面板」。
 * ⚠️ 导航项**保留 `data-view-tab`**（裁决 B）⇒ ⇒ `syncTabState()`
 * **一个字都不用改** ⇒ ⇒ 「当前项」仍是 `aria-current="page"`。
 *
 * @returns {number} 渲染出来的导航项数
 */
export function renderSidebar() {
    const host = document.querySelector("[data-sidebar]");
    if (!host) return 0;
    // ⚠️ **清空容器不用 `innerHTML` 赋值**（逐个摘掉子节点）⇒ ⇒ M-01 那条
    // 「不拼 HTML 字符串」的既有门禁（它匹配的是 `.innerHTML` **后面跟等号**
    // 这个形状）**一字不改**仍然成立 ⇒ ⇒ 真浏览器里 `childNodes` 是 live 的
    // NodeList ⇒ ⇒ 这么写也对。
    //
    // ⚠️ **这条注释自己就是证据**：那条门禁只匹配**赋值**形状 ⇒ ⇒ 所以**注释
    // 里也不能写出那个形状** ⇒ ⇒ 否则源码里一句解释性的散文就会把门禁撞红
    // （FRONTEND_Review.md R-17：过宽的静态断言 = 假红）⇒ ⇒ **实测踩到了**。
    while (host.childNodes.length) host.removeChild(host.childNodes[0]);
    let count = 0;
    for (const g of NAV_GROUPS) {
        const items = [...mountedViews.values()].filter((v) => v.group === g.id);
        if (!items.length) continue;
        const group = document.createElement("div");
        group.className = "nav-group";
        const groupLabel = document.createElement("div");
        groupLabel.className = "nav-label";
        groupLabel.textContent = g.label;
        group.appendChild(groupLabel);
        for (const v of items) {
            const item = document.createElement("a");
            item.className = "nav-item";
            item.setAttribute("href", `#/${v.hash}`);
            // ⚠️ **保留 `data-view-tab`** ⇒ ⇒ 旧标签页那套语义不变。
            item.setAttribute("data-view-tab", v.hash);
            const ic = document.createElement("span");
            ic.className = "nav-ic";
            ic.setAttribute("aria-hidden", "true");
            // ⚠️ **SVG 用 `createElementNS` 造**（不是 `innerHTML`）⇒ ⇒
            // 形状是常量、不含用户数据 ⇒ ⇒ 而且 M-01 那条「不拼 HTML
            // 字符串」的既有门禁**一字不改**仍然成立。
            const icon = buildNavIcon(v.hash);
            if (icon) ic.appendChild(icon);
            item.appendChild(ic);
            const text = document.createElement("span");
            text.textContent = v.name;
            item.appendChild(text);
            group.appendChild(item);
            count += 1;
        }
        host.appendChild(group);
    }
    // 重建完 DOM ⇒ ⇒ 「当前项」的高亮要重新打一遍。
    syncTabState(activeView?.hash ?? "");
    return count;
}

/**
 * 窄屏抽屉的开关（STAGE-34 · v0.49 批 · 裁决 H）。
 *
 * ⚠️ 宽屏（≥ 720px）侧边栏**常驻**⇒ ⇒ 抽屉那个 `.open` **只影响窄屏**
 * （CSS 决定）⇒ ⇒ 这个函数在宽屏上点了也**看不出差别**（但也无害）。
 */
function wireSidebarToggle() {
    // ⚠️ **只订阅一次** ⇒ ⇒ 否则 `assembleWorkspace()` 每次调用都会**叠一层**
    // 监听器 ⇒ ⇒ 点一下开关会**切换多次** ⇒ ⇒ 看起来像「没反应」。
    if (sidebarToggleWired) return;
    const toggle = document.querySelector("[data-sidebar-toggle]");
    const sidebar = document.querySelector("[data-sidebar]");
    if (!toggle || !sidebar) return;
    sidebarToggleWired = true;
    toggle.addEventListener("click", () => {
        sidebar.classList.toggle("open");
    });
    // 点导航项 ⇒ ⇒ 抽屉**顺手收起**（窄屏下不挡住刚切过去的视图）。
    sidebar.addEventListener("click", (ev) => {
        const item = ev.target?.closest?.("[data-view-tab]");
        if (item) sidebar.classList.remove("open");
    });
}

function showPanel(target) {
    for (const panel of document.querySelectorAll("[data-view-panel]")) {
        panel.classList.toggle(
            "hidden",
            panel.getAttribute("data-view-panel") !== target.hash,
        );
    }
}

/**
 * Sync the tab strip's active state. The CSS highlights
 * `[aria-current="page"]`, so the attribute — not a class — is the
 * single source of truth for "which view am I looking at"
 * (FRONTEND_Review.md #5).
 *
 * @param {string} hash — the active view hash
 */
function syncTabState(hash) {
    for (const tab of document.querySelectorAll("[data-view-tab]")) {
        if (tab.getAttribute("data-view-tab") === hash) {
            tab.setAttribute("aria-current", "page");
        } else {
            tab.removeAttribute("aria-current");
        }
    }
}

/**
 * Capture the UI interaction state of a view before leaving it
 * (LLD §3.3 — "per-view UI interaction state": folding, zoom, terminal
 * scroll position). Only the DOM knows these values, so they are read
 * on the way out and written back into M-03 on the way in.
 *
 * Only values that can actually exist are captured: `.view-body` has no
 * `overflow` and is therefore **not** a scroll container — its
 * `scrollTop` is always 0 in a real browser. The terminal output
 * (`.terminal-out`, `overflow:auto`) is the only real scroll container
 * in the current view set (FRONTEND_Review.md R-04).
 *
 * @param {string} viewId
 */
function captureUiState(viewId) {
    const body = document.querySelector(`[data-view-body="${viewId}"]`);
    if (!body) return;
    const terminal = body.querySelector("[data-terminal-out]");
    // Function form: `ui` is merged key-by-key instead of being
    // replaced wholesale, so future sub-keys (zoom, folding) survive
    // (FRONTEND_Review.md R-06).
    viewStore.setState(viewId, (prev) => ({
        ...prev,
        ui: {
            ...(prev.ui ?? {}),
            terminalScrollTop: terminal ? terminal.scrollTop : 0,
        },
    }));
}

/**
 * Restore the UI interaction state of a view after it has been rendered.
 *
 * @param {string} viewId
 */
function restoreUiState(viewId) {
    const body = document.querySelector(`[data-view-body="${viewId}"]`);
    if (!body) return;
    const ui = viewStore.getState(viewId)?.ui;
    if (!ui) return;
    const terminal = body.querySelector("[data-terminal-out]");
    if (terminal && typeof ui.terminalScrollTop === "number") {
        terminal.scrollTop = ui.terminalScrollTop;
    }
}

function logEctx05(reason, hash) {
    console.warn(`[M-01] E_CTX-05 ${reason}`, hash);
}

function dispatchRenderEvent(evt) {
    const ev = new CustomEvent("riscv:render", { detail: evt });
    window.dispatchEvent(ev);
    return evt;
}

// ---------------------------------------------------------------------------
// Bootstrap (replaces the `mount*` no-op from the MVP skeleton).
// ---------------------------------------------------------------------------

/**
 * Mount the workbench and route to the initial view.
 *
 * The initial route is deferred to a microtask on purpose: it dispatches
 * the *first* render event, and every view component has to be subscribed
 * before that happens. Mounting synchronously here would lose that first
 * event whenever the component library is mounted after the workbench —
 * the page would come up with every view body empty
 * (FRONTEND_Review.md #20).
 *
 * @returns {Promise<object>} the assembled workspace snapshot
 */
export async function mountWorkbench() {
    const role = resolveRole();
    const workspace = assembleWorkspace(role);
    window.addEventListener("hashchange", () => {
        route(location.hash);
    });
    // Run once with whatever the URL says — after the current task, so
    // that sibling mounts have registered their listeners.
    queueMicrotask(() => {
        route(location.hash || `#/${currentRole}`);
    });
    return workspace;
}
