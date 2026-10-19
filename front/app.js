// RISC-V Sim — front-end bootstrap (MVP placeholder).
//
// Layout: vanilla ES modules, no bundler. See AD §9.7 "原生 JS + 多视图
// hash 路由" and the M-01 / M-02 / M-03 module stubs under `front/views`
// and `front/services`.
//
// Routing: hash-based (e.g. `#/embedded/regs`). MVP just toggles the
// <section> visibility; M-03 ViewStore is the next thing to wire up.

import { mountWorkbench } from "./views/m01_workbench.js";
import { mountViewComponents } from "./views/m02_viewcomponents.js";
import { mountViewStore } from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import { mountTransport } from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";

const status = document.getElementById("status");

/**
 * Tiny hash router. Each route is `{workbench}/{subview?}`.
 * The router only toggles the visible <section> in MVP; M-03 will
 * own the actual state.
 */
function applyRoute() {
    const hash = location.hash.replace(/^#\/?/, "");
    const [wb, sub] = hash.split("/");
    const workbench = wb || "product";
    document.querySelectorAll(".tab").forEach((b) => b.removeAttribute("aria-current"));
    document.querySelector(`.tab[data-view="${workbench}"]`)?.setAttribute("aria-current", "page");
    for (const view of document.querySelectorAll(".view")) view.classList.add("hidden");
    const target = document.getElementById(`view-${workbench}`);
    if (target) target.classList.remove("hidden");
    if (workbench === "embedded" && sub) {
        for (const panel of document.querySelectorAll("[data-subview-panel]")) panel.classList.add("hidden");
        document.querySelector(`[data-subview-panel="${sub}"]`)?.classList.remove("hidden");
    }
}

async function bootstrap() {
    status.textContent = "正在挂载模块…";
    await Promise.all([
        mountWorkbench(),
        mountViewComponents(),
        mountViewStore(),
        mountSession(),
        mountCommandRouter(),
        mountTransport(),
        mountRegisterMemory(),
    ]);
    window.addEventListener("hashchange", applyRoute);
    applyRoute();
    status.textContent = "MVP 就绪 — kernel 待编译";
}

bootstrap().catch((err) => {
    console.error("bootstrap failed:", err);
    status.textContent = `启动失败: ${err?.message ?? err}`;
});
