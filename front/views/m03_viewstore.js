// M-03 视图状态管理 (View Store).
//
// MVP scope (LLD §3.3):
//   - 按 viewId 存 UI 交互态（面板折叠、波形缩放、终端滚动位置）。
//   - 不订阅事件流 — M-07 transport 接入后接 S1/W1/T1/E1 合并 storeState。
//   - 公开 setState(viewId, patch) / getState(viewId) / getAll(viewId) /
//     subscribe(viewId, fn)，给 M-01 switchView 与 M-02 视图组件用。
//
// 状态是模块级单例（前端 ES module 自然单例）。
// 不通过事件总线（M-05）做服务间同步 — 这部分等 M-07 接入。

/** @type {Map<string, object>} viewId → 状态 */
const viewState = new Map();

/** @type {Map<string, Set<Function>>} viewId → 订阅者集合 */
const subscribers = new Map();

/**
 * Replace-or-merge per-view state. Passing a function lets subscribers
 * derive the next state from the previous one (LLD §3.3).
 *
 * @param {string} viewId
 * @param {object|((prev: object) => object)} patch
 */
export function setState(viewId, patch) {
    const prev = viewState.get(viewId) ?? {};
    const next = typeof patch === "function" ? patch(prev) : { ...prev, ...patch };
    viewState.set(viewId, next);
    const subs = subscribers.get(viewId);
    if (subs) for (const fn of subs) fn(next, prev);
    return next;
}

/**
 * Read per-view state. Returns `undefined` when the view has not been
 * visited yet (M-01 §3.3 `?? defaultState(target)`).
 *
 * @param {string} viewId
 */
export function getState(viewId) {
    return viewState.get(viewId);
}

/**
 * Snapshot all view states — used by M-20 snapshot serializer (P1
 * contract kept simple so M-23 IndexedDB persistence can read it).
 */
export function snapshot() {
    return Object.fromEntries(viewState.entries());
}

/**
 * Subscribe to per-view state changes. Returns the unsubscribe fn.
 *
 * @param {string} viewId
 * @param {(next: object, prev: object) => void} handler
 */
export function subscribe(viewId, handler) {
    let set = subscribers.get(viewId);
    if (!set) {
        set = new Set();
        subscribers.set(viewId, set);
    }
    set.add(handler);
    return () => set.delete(handler);
}

/**
 * Test / dev-only: reset all view states. Used by bootstrap tests and
 * by `assembleWorkspace` when the role flips.
 */
export function reset() {
    viewState.clear();
    for (const set of subscribers.values()) set.clear();
    subscribers.clear();
}
