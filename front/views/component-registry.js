// M-02 视图组件注册中心 + 命令信号（拆分自 m02_viewcomponents.js）。
//
// 这个模块**不持有任何视图**：只维护「id → 组件」注册表，以及视图层向
// app 层发命令信号的唯一出口。各面板模块在这里 `register(...)` 自己，
// 门面（m02_viewcomponents.js）只做装配与 re-export。
//
// ⚠️ 单独成模块是为了**避免循环依赖**：面板需要 `register` / `emitCommand`，
// 门面又要 import 面板触发注册；把注册表与信号出口抽到这里，依赖方向是
// 面板 → 本模块 ← 门面，不成环。

/** @type {Map<string, ViewComponent>} */
const components = new Map();

/** Register a view component. Idempotent. */
export function register(c) {
    components.set(c.id, c);
    return c;
}

/** @returns {ViewComponent|undefined} */
export function get(id) {
    return components.get(id);
}

/** Test / dev only. */
export function _all() {
    return [...components.values()];
}

/** Reset the registry. */
export function reset() {
    components.clear();
}

// ---------------------------------------------------------------------------
// Command signal — the view layer never talks to the service layer
// directly: M-02 only emits a signal, `app.js` turns it into state.
// ---------------------------------------------------------------------------

/** Monotonic request id counter (the ViewComponent `onCommand` return). */
let requestCounter = 0;

/**
 * Emit the `riscv:command` signal and return the request id.
 *
 * @param {string} action
 * @param {string} viewId
 * @param {object|null} [params] — command parameters (address / count /
 *   source / format / offset …). They travel **in the signal**: the view
 *   layer must not talk to the service layer, and the DOM is not
 *   reachable from the bootstrap's listener (the signal is dispatched on
 *   `window`, so `event.target` is the window, not the button).
 * @returns {string} the request id
 */
export function emitCommand(action, viewId, params = null) {
    requestCounter += 1;
    const requestId = `cmd-${action}-${requestCounter}`;
    window.dispatchEvent(new CustomEvent("riscv:command", {
        detail: { action, requestId, viewId, params },
    }));
    return requestId;
}
