// Dev-only demo data source (MVP).
//
// M-07 transport has not landed yet, so there is no live kernel behind
// the workbench. To keep the view layer exercisable, the product view
// renders a static "intelligent light" memory map mirroring what
// `m14_bus::Bus::regions()` returns after a typical load.
//
// The demo data deliberately lives *outside* the production render path
// (M-02): M-02 only emits the `riscv:seed` signal, and the dev wiring
// in app.js is what turns that signal into state. When M-07 lands, this
// file and the dev-only button go away together.
//
// Related: FRONTEND_Review.md #15.

import * as viewStore from "../views/m03_viewstore.js";

/**
 * Build a fake `sim.memoryMap` plus a tiny product tree.
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
            memoryMap: [
                { base: 0x0000_0000, size: 0x1_0000, type: "RAM", detail: "[0, 64 KiB) firmware image" },
                { base: 0x2000_0000, size: 0x1000, type: "PERIPHERAL", detail: "UART0 (id=1, 4 KiB)" },
                { base: 0x4000_0000, size: 0x1000, type: "PERIPHERAL", detail: "GPIO0 (id=2, 4 KiB)" },
            ],
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
