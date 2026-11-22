// App 层 · 连接 / 状态条族（从 app.js 抽出）。
//
// 状态条文案由真实传输状态派生（不是写死字符串）；连接胶囊只切颜色、
// 不写第二份文案；链路状态同时推进落地视图的视图状态。
import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";

const status = document.getElementById("status");

/**
 * 连接状态**胶囊**的点：连上是绿的，没连上是红的。
 *
 * ⚠️ **胶囊里的文字就是 `#status`** ⇒ ⇒ 连接状态的**文案只有一处**
 * （`describeTransport()`）⇒ ⇒ 这里**只切颜色**，**不写第二份文案**。
 *
 * @param {{connected?: boolean}|undefined} transport
 */
export function syncKernelConn(transport) {
    const conn = document.querySelector("[data-kernel-conn]");
    if (!conn) return;
    conn.classList.toggle("is-offline", transport?.connected !== true);
}

/**
 * Derive the status-bar text from the real transport state instead of
 * hardcoding it (FRONTEND_Review.md #11).
 *
 * Exported so the front-end gate can assert that the status bar is
 * *derived* rather than a hardcoded string (FRONTEND_Review.md R-02).
 *
 * @param {{mode: string, connected: boolean, label?: string, lastError?: string|null}|undefined} transport
 * @returns {string}
 */
export function describeTransport(transport) {
    if (!transport) {
        // A missing transport descriptor must read differently from a
        // present-but-disconnected one — otherwise a mis-bound mount
        // result (e.g. the workspace snapshot landing in the transport
        // slot) is invisible to the gate (FRONTEND_Review.md R-01/R-02).
        return "未连接 · 状态未知";
    }
    if (transport.connected) {
        const label = transport.label ?? `${transport.mode}`;
        return `已连接 · ${label}`;
    }
    const reason = transport.lastError ? transport.lastError : "未接线";
    return `未连接 · ${reason}`;
}

/**
 * Write the derived status text into the status bar.
 *
 * @param {{mode: string, connected: boolean}|undefined} transport
 * @param {Element} [el]
 * @returns {string} the text written
 */
export function renderStatus(transport, el = status) {
    el.textContent = describeTransport(transport);
    return el.textContent;
}

/**
 * Fill the footer with what is actually mounted, so the copy cannot go
 * stale the way a hardcoded phase label did (FRONTEND_Review.md #12).
 *
 * @param {{role: string, views: {hash: string, name: string}[]}} workspace
 */
export function describeWorkspace(workspace) {
    const info = document.getElementById("build-info");
    if (!info) return;
    const names = (workspace?.views ?? []).map((v) => v.name).join(" / ");
    info.textContent = `${workspace?.role ?? "?"} · ${names}`;
}

/**
 * 把当前链路状态推进落地视图的视图状态（连接胶囊那一份）。
 *
 * @param {{connected?: boolean, mode?: string, label?: string, lastError?: string|null}|undefined} transport
 * @param {string} [viewId]
 */
export function publishLinkState(transport, viewId = "embedded") {
    viewStore.setState(viewId, (prev) => ({
        ...prev,
        link: {
            connected: transport?.connected === true,
            mode: transport?.mode ?? "none",
            label: transport?.label ?? "",
            lastError: transport?.lastError ?? null,
        },
    }));
    publishViewState(viewId);
}
