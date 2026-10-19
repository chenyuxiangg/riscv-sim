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
        "view.regmem",
    ])],
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
 */

/** @type {ReadonlyMap<string, ViewDescriptor>} */
const VIEW_REGISTRY = new Map([
    ["product", {
        hash: "product",
        name: "产品视图",
        capability: "view.productTree",
        panel: '[data-view-panel="product"]',
        role: "product",
    }],
    ["embedded", {
        hash: "embedded",
        name: "嵌入式工作台",
        capability: "view.productTree",
        panel: '[data-view-panel="embedded"]',
        role: "embedded",
    }],
    ["debug", {
        hash: "debug",
        name: "调试视图",
        capability: "view.debug",
        panel: '[data-view-panel="debug"]',
        role: "embedded",
    }],
    ["regmem", {
        hash: "regmem",
        name: "寄存器/内存",
        capability: "view.regmem",
        panel: '[data-view-panel="regmem"]',
        role: "embedded",
    }],
    ["waveform", {
        hash: "waveform",
        name: "波形视图",
        capability: "view.waveform",
        panel: '[data-view-panel="waveform"]',
        role: "embedded",
    }],
    ["terminal", {
        hash: "terminal",
        name: "串口终端",
        capability: "view.terminal",
        panel: '[data-view-panel="terminal"]',
        role: "product",
    }],
]);

const FALLBACK_VIEW = {
    hash: "fallback",
    name: "未识别 / 无权限",
    capability: "view.productTree",
    panel: '[data-view-panel="fallback"]',
    role: "product",
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
        activeView = mountedViews.get(role) ?? [...mountedViews.values()][0] ?? null;
    }
    mountPanels();
    return {
        role: currentRole,
        capBits: [...currentCapBits],
        views: [...mountedViews.values()].map((v) => ({ hash: v.hash, name: v.name })),
        activeView: activeView ? { hash: activeView.hash, name: activeView.name } : null,
    };
}

/**
 * Hash router (LLD §3.2). Returns the RenderEvent envelope.
 *
 * @param {string} hash — raw `location.hash` (incl. leading `#`)
 */
export function route(hash) {
    const viewId = (hash || "").replace(/^#\/?/, "").split("/")[0] || currentRole;
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
        syncTabState(target.hash);
        return evt;
    }
    // Save current view state to the store.
    if (activeView) {
        viewStore.setState(activeView.hash, {});
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
    if (activeView) {
        dispatchRenderEvent({
            viewId: activeView.hash,
            type: "update",
            state: viewStore.getState(activeView.hash) ?? {},
        });
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** Mount the per-view panels into the DOM (idempotent). */
function mountPanels() {
    const seen = new Set();
    for (const v of VIEW_REGISTRY.values()) {
        if (!currentCapBits.has(v.capability)) continue;
        let panel = document.querySelector(v.panel);
        if (!panel) {
            panel = createPanel(v);
            document.getElementById("app")?.appendChild(panel);
        }
        seen.add(v.hash);
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
    el.innerHTML = `
        <h2>${escapeHtml(v.name)}</h2>
        <div class="view-body" data-view-body="${v.hash}"></div>
    `;
    return el;
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

function logEctx05(reason, hash) {
    console.warn(`[M-01] E_CTX-05 ${reason}`, hash);
}

function dispatchRenderEvent(evt) {
    const ev = new CustomEvent("riscv:render", { detail: evt });
    window.dispatchEvent(ev);
    return evt;
}

/** Minimal DOM text escape (NFR-04). */
function escapeHtml(s) {
    return String(s)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

// ---------------------------------------------------------------------------
// Bootstrap (replaces the `mount*` no-op from the MVP skeleton).
// ---------------------------------------------------------------------------

/** @returns {Promise<void>} */
export async function mountWorkbench() {
    const role = resolveRole();
    assembleWorkspace(role);
    window.addEventListener("hashchange", () => {
        route(location.hash);
    });
    // Run once with whatever the URL says.
    route(location.hash || `#/${currentRole}`);
}
