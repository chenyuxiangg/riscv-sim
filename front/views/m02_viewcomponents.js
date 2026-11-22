// M-02 视图组件库 (View component library) — 门面 (facade)。
//
// 本文件**不再持有**任何视图实现：六个注册组件按域拆到
// `views/components/*-panel.js`，注册中心与命令信号在
// `views/component-registry.js`。这里只保留：
//   - 注册表访问器 / 装配引导（render 事件派发、挂载委托）；
//   - 把各面板的公开符号 **re-export** 出去 ⇒ `app.js` 与既有测试的
//     导入路径、符号名一字不变。
//
// Each component implements the three-function shape from LLD §2.1.1:
//   - render(storeState)
//   - onCommand(ev)     → requestId
//   - onDelta(delta)
//
// Render dispatch comes from M-01 `riscv:render` CustomEvents.

import { get } from "./component-registry.js";
export { register, get, _all, reset } from "./component-registry.js";

import { escapeHtml } from "../lib/escape.js";
// The helper used to live here; it now has a single implementation in
// `front/lib/escape.js` (FRONTEND_Review.md #9). Re-exported so existing
// importers keep working.
export { escapeHtml };

// ---- 各面板（副作用：`register(...)` 把自己装进注册表） --------------------

import "./components/product-panel.js";
export {
    adoptProductDescription,
    syncProductBaseline,
    markProductApplied,
    EMBEDDED_VIEW_ENTRIES,
} from "./components/product-panel.js";

import "./components/debug-panel.js";
export {
    exportFileName,
    exportMimeType,
    downloadExportChunk,
    ADDR_INPUT_ERROR,
    MEMORY_READ_HINT,
    MEMORY_DUMP_BUTTON_LABEL,
    parseAddrInput,
    BREAKPOINT_KIND,
    BREAKPOINT_KIND_LABEL,
    BREAKPOINT_SYMBOL_TAB_LIMIT,
    normalizeBreakpointKind,
    renderBreakpointAddRow,
    renderBreakpointRow,
    renderMemoryReadRow,
    renderSymbolCandidate,
    renderSymbolCandidates,
    renderMemoryDumpRow,
    renderMemoryDump,
    DEBUGGER_STATE_KEY,
    DEFAULT_STEP_COUNT,
    debuggerDraft,
    DEFAULT_DUMP_LEN,
    NO_SOURCE_LINE_LABEL,
    sourceLocationParts,
    formatSourceLocation,
    renderRegGrid,
    renderReadbackState,
} from "./components/debug-panel.js";

import "./components/waveform-panel.js";

import "./components/source-panel.js";

import "./components/terminal-panel.js";
export {
    CONSOLE_DECODE_CHUNK,
    decodeConsoleBytes,
    consoleByteCount,
    TERMINAL_EOL,
    TERMINAL_EOL_DEFAULT,
    TERMINAL_EOL_OPTIONS,
    resetTerminalScreen,
    terminalInputBytes,
    dispatchTerminalBytes,
    resetTerminalXterm,
} from "./components/terminal-panel.js";

// ---------------------------------------------------------------------------
// Render dispatcher — receives M-01 `riscv:render` CustomEvents.
// ---------------------------------------------------------------------------

/**
 * 上一次**写进 DOM 的 HTML**（STAGE-38 · 裁决 K）。
 *
 * ⚠️ **按 DOM 容器本身做键**（`WeakMap`）⇒ 容器被换掉（视图被重新挂载）
 * ⇒ 查不到旧值 ⇒ **第一轮一定重建** ⇒ 不会出现「容器是空的却因为
 * 「内容一样」而跳过渲染」这种**空面板**。
 *
 * @type {WeakMap<object, string>}
 */
const lastRenderedHtml = new WeakMap();

/**
 * @param {{ viewId: string, type: string, state: object }} detail
 */
function applyRenderEvent(detail) {
    const c = get(detail.viewId);
    if (!c) return;
    // Locate the per-view mount body and re-render into it.
    const body = document.querySelector(`[data-view-body="${detail.viewId}"]`);
    if (!body) return;
    let html = "";
    try {
        html = c.render(detail.state ?? {});
    } catch (err) {
        html = `<div class="view-error">渲染失败：${escapeHtml(err?.message ?? String(err))}</div>`;
        console.warn(`[M-02 ${detail.viewId}] render error:`, err);
    }
    // ⚠️ **同内容不重建**（裁决 K）⇒ 连续运行每一轮都 `publishViewState("debug")`
    // （F7）⇒ 而运行中活值**全被隐藏** ⇒ 渲染结果逐字相同（F8）⇒ 这一次
    // **不必**把整个视图正文换掉 ⇒ 主线程**不再**被占满 ⇒ 界面**不闪**。
    //
    // ⚠️ **`afterRender` 照旧调用**（裁决 L）⇒ 波形图那一张图**不受**去重
    // 影响 ⇒ 图**照样**跟着数据更新。
    const unchanged = lastRenderedHtml.get(body) === html;
    if (!unchanged) {
        lastRenderedHtml.set(body, html);
        body.innerHTML = html;
    }
    // ⚠️ **可选**的渲染后钩子（裁决 M）。图表（echarts）需要一个**真实 DOM
    // 容器**才能初始化 ⇒ `body.innerHTML = html` 之后容器才存在。只有**声明
    // 了**它的视图会被调用；钩子里做的事失败**不许**把整个渲染搞崩 ⇒ 失败
    // 只该让**那一张图**没有。
    if (typeof c.afterRender === "function") {
        try {
            c.afterRender(body, detail.state ?? {});
        } catch (err) {
            console.warn(`[M-02 ${detail.viewId}] afterRender error:`, err);
        }
    }
}

/**
 * Bind a view component to its DOM panel the first time it is mounted.
 */
function attachMount(viewId) {
    const c = get(viewId);
    if (!c) return;
    const body = document.querySelector(`[data-view-body="${viewId}"]`);
    if (!body || body.dataset.mounted === "1") return;
    body.dataset.mounted = "1";
    c.mountFor(body);
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

/** @returns {Promise<void>} */
export async function mountViewComponents() {
    // Wire render events to all registered components.
    window.addEventListener("riscv:render", (ev) => {
        const detail = ev.detail;
        if (!detail || !detail.viewId) return;
        attachMount(detail.viewId);
        applyRenderEvent(detail);
    });
}
