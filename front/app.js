// RISC-V Sim — front-end bootstrap.
//
// Layout: vanilla ES modules, no bundler. See AD §9.7.
// Routing + workspace assembly live in m01_workbench (LLD §3.2);
// view rendering lives in m02_viewcomponents (LLD §3.3). This file
// only does wiring + dev-mode helpers.

import { mountWorkbench } from "./views/m01_workbench.js";
import { mountViewComponents, seedBus } from "./views/m02_viewcomponents.js";
import { mountViewStore } from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import { mountTransport } from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
import { buildDemoStoreState, seedDemoState } from "./dev/demo-state.js";

const status = document.getElementById("status");

/**
 * Wire the dev-only "seed demo bus" affordances:
 *   - the top-bar button
 *   - the `riscv:seed` signal emitted by the product view
 * Real data binding lands once M-07 transport is wired up.
 */
function wireDevSeeding() {
    const btn = document.getElementById("seed-demo");
    btn?.addEventListener("click", () => requestDemoSeed());
    window.addEventListener("riscv:seed", (ev) => {
        seedDemoState(ev.detail?.viewId ?? "product");
    });
}

/** Ask for a demo re-seed and report it in the status bar. */
function requestDemoSeed(viewId = "product") {
    seedBus(viewId);
    status.textContent = "已 seed demo 内存映射（M-14 1 RAM + 2 PERIPHERAL）";
}

/** Expose the seeder on `window.riscv_demo` for console convenience. */
function exposeDevHelpers() {
    window.riscv_demo = {
        seedBus: () => requestDemoSeed(),
        buildDemoStoreState,
    };
}

/**
 * Derive the status-bar text from the real transport state instead of
 * hardcoding it (FRONTEND_Review.md #11).
 *
 * Exported so the front-end gate can assert that the status bar is
 * *derived* rather than a hardcoded string (FRONTEND_Review.md R-02).
 *
 * @param {{mode: string, connected: boolean}|undefined} transport
 * @returns {string}
 */
export function describeTransport(transport) {
    if (!transport) {
        // A missing transport descriptor must read differently from a
        // present-but-disconnected one — otherwise a mis-bound mount
        // result (e.g. the workspace snapshot landing in the transport
        // slot) is invisible to the gate (FRONTEND_Review.md R-01/R-02).
        return "工作台已就绪 — kernel transport 状态未知（M-07 未挂载）";
    }
    if (transport.connected) {
        return `kernel 已连接 — ${transport.mode} transport`;
    }
    return "工作台已就绪 — kernel transport 未连接（M-07 待接线）";
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
function describeWorkspace(workspace) {
    const info = document.getElementById("build-info");
    if (!info) return;
    const names = (workspace?.views ?? []).map((v) => v.name).join(" / ");
    info.textContent = `当前工作台：role=${workspace?.role ?? "?"} · ${names}`;
}

async function bootstrap() {
    status.textContent = "正在挂载模块…";
    // Two redundant safeguards guarantee that the view component library
    // is subscribed **before** the workbench dispatches its first
    // `riscv:render` event (otherwise every view body stays empty —
    // FRONTEND_Review.md #20):
    //   1. object literal order — `components` is mounted first, so the
    //      subscription is registered synchronously;
    //   2. `mountWorkbench()` defers its initial route to a microtask
    //      (m01), so even an async mount cannot miss the event.
    //
    // The mounts are collected in a **named** object and resolved by key:
    // the key→value mapping is derived from the same object, so adding
    // or reordering a mount can no longer silently mis-bind a result
    // (FRONTEND_Review.md R-01). Object literal evaluation order is still
    // source order, so safeguard 1 is unaffected.
    const pending = {
        components: mountViewComponents(),
        workspace: mountWorkbench(),
        viewStore: mountViewStore(),
        session: mountSession(),
        commandRouter: mountCommandRouter(),
        transport: mountTransport(),
        registerMemory: mountRegisterMemory(),
    };
    const { components, workspace, transport } = Object.fromEntries(
        await Promise.all(
            Object.entries(pending).map(async ([key, promise]) => [key, await promise]),
        ),
    );
    wireDevSeeding();
    exposeDevHelpers();
    describeWorkspace(workspace);
    renderStatus(transport);
}

bootstrap().catch((err) => {
    console.error("bootstrap failed:", err);
    status.textContent = `启动失败: ${err?.message ?? err}`;
});
