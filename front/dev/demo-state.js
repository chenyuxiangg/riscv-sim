// Dev-only demo data source (MVP).
//
// M-07 transport has not landed yet, so there is no live kernel behind
// the workbench. To keep the view layer exercisable, the product view
// renders a static "intelligent light" product tree.
//
// The demo data deliberately lives *outside* the production render path
// (M-02): M-02 only emits the `riscv:seed` signal, and the dev wiring
// in app.js is what turns that signal into state. When M-07 lands, this
// file and the dev-only button go away together.
//
// ⚠️ **这里没有假内存映射**（STAGE-14 · v0.29 · 规格 §2.6）：内存映射表
// 改成显示内核 `memory.map` 事件里的**当前生效**的图 ⇒ demo 塞一份假图进去
// 就是「第二个真相源」⇒ 前端会显示一张内核根本没在用的板子。
//
// ⇒ 「没有数据」与「数据是空的」由**渲染层**分得开（`data-memory-map-table`
// 的 `no-data` / `empty` 两种取值），不需要靠假数据撑场面。
//
// Related: FRONTEND_Review.md #15.

import * as viewStore from "../views/m03_viewstore.js";

/**
 * Build a tiny product tree plus a minimal sim state.
 *
 * @returns {object} a store-state shaped object
 */
export function buildDemoStoreState() {
    return {
        timeVirt: 0,
        version: 1,
        sim: {
            pc: 0x0,
            gpr: Array(32).fill(0),
            disasm: [],
        },
        wave: { channels: [] },
        term: { bytes: "" },
        events: {},
        tree: {
            root: {
                name: "MainBoard",
                component_id: "rv-sim",
                children: [
                    { name: "GPIO0", component_id: "GPIO0", children: [] },
                    { name: "UART0", component_id: "UART0", children: [] },
                ],
            },
        },
    };
}

/**
 * Write the demo state into the view store and ask the view to re-render.
 *
 * The store is the single source of truth: the render event carries the
 * *stored* state, not a detached copy, so a later re-render cannot
 * silently roll the demo data back.
 *
 * @param {string} [viewId]
 * @returns {object} the state now held by the store
 */
export function seedDemoState(viewId = "product") {
    viewStore.setState(viewId, buildDemoStoreState());
    const state = viewStore.getState(viewId);
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId, type: "update", state },
    }));
    return state;
}
