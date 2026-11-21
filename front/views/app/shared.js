// App 层跨族公共辅助（STAGE-33 起从 app.js 抽出）。
//
// 这里只放被命令族 / 接线族 / 工程装载族**共同**调用、且不持有可变业务
// 状态的小辅助：页头短备注写入、单视图状态重渲染派发。
import * as viewStore from "../m03_viewstore.js";

/**
 * Write a short note about the last command / handshake into the
 * footer. The status bar stays reserved for the transport state (the
 * gate asserts that it is derived from it), so command feedback goes
 * here.
 *
 * @param {string} text
 */
export function setNote(text) {
    const note = document.getElementById("kernel-note");
    if (note) note.textContent = text;
}

/**
 * Ask the workbench to re-render one view from the store.
 *
 * @param {string} viewId
 */
export function publishViewState(viewId) {
    const state = viewStore.getState(viewId);
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId, type: "update", state },
    }));
}
