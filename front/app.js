// RISC-V Sim — front-end bootstrap.
//
// Layout: vanilla ES modules, no bundler. See AD §9.7.
// Routing + workspace assembly live in m01_workbench (LLD §3.2);
// view rendering lives in m02_viewcomponents (LLD §3.3). This file
// only does wiring + dev-mode helpers.

import { mountWorkbench } from "./views/m01_workbench.js";
import { mountViewComponents, buildDemoStoreState } from "./views/m02_viewcomponents.js";
import { mountViewStore } from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import { mountTransport } from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";

const status = document.getElementById("status");

/**
 * Wire the dev-mode "seed demo bus" button. Real data binding lands
 * once M-07 transport is wired up.
 */
function wireSeedButton() {
    const btn = document.getElementById("seed-demo");
    if (!btn) return;
    btn.addEventListener("click", () => {
        const storeState = buildDemoStoreState();
        const evt = new CustomEvent("riscv:render", {
            detail: { viewId: "product", type: "update", state: storeState },
        });
        window.dispatchEvent(evt);
        status.textContent = "已 seed demo 内存映射（M-14 1 RAM + 2 PERIPHERAL）";
    });
}

/** Expose the seeder on `window.riscv_demo` for console convenience. */
function exposeDevHelpers() {
    window.riscv_demo = {
        seedBus: () => {
            const btn = document.getElementById("seed-demo");
            btn?.click();
        },
        buildDemoStoreState,
    };
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
    wireSeedButton();
    exposeDevHelpers();
    status.textContent = "MVP 就绪 — kernel 待编译";
}

bootstrap().catch((err) => {
    console.error("bootstrap failed:", err);
    status.textContent = `启动失败: ${err?.message ?? err}`;
});
