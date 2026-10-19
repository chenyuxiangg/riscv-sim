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
 * @param {{mode: string, connected: boolean}|undefined} transport
 * @returns {string}
 */
function describeTransport(transport) {
    if (transport?.connected) {
        return `kernel 已连接 — ${transport.mode} transport`;
    }
    return "工作台已就绪 — kernel transport 未连接（M-07 待接线）";
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
    // The view component library is mounted **first** on purpose: it
    // subscribes to the `riscv:render` events, and the workbench's
    // initial route dispatches the first one. Mounting the workbench
    // first would drop that event and leave every view body empty
    // (FRONTEND_Review.md #20).
    const [, workspace, , , , transport] = await Promise.all([
        mountViewComponents(),   // 0 → subscribes to the render events
        mountWorkbench(),        // 1 → assembled workspace snapshot
        mountViewStore(),
        mountSession(),
        mountCommandRouter(),
        mountTransport(),        // 5 → live transport state
        mountRegisterMemory(),
    ]);
    wireDevSeeding();
    exposeDevHelpers();
    describeWorkspace(workspace);
    status.textContent = describeTransport(transport);
}

bootstrap().catch((err) => {
    console.error("bootstrap failed:", err);
    status.textContent = `启动失败: ${err?.message ?? err}`;
});
