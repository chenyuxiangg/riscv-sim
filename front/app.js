// RISC-V Sim — front-end bootstrap.
//
// Layout: vanilla ES modules, no bundler. See AD §9.7.
// Routing + workspace assembly live in m01_workbench (LLD §3.2);
// view rendering lives in m02_viewcomponents (LLD §3.3). This file
// wires the four service modules (M-07 transport → M-05 command router
// → M-04 session / M-10 register-memory) to the workbench and routes
// the view layer's `riscv:command` signal into the command router.
//
// Service dependency order (PROGRESS §6.1):
//   M-07 transport → M-05 command router → M-04 session
//   M-10 register/memory service → M-05 (+ M-07 for the link state)
// The order is a real dependency, so the service layer is mounted
// **sequentially** — a parallel `Promise.all` would hand the router a
// transport that does not exist yet.

import { mountWorkbench } from "./views/m01_workbench.js";
import { mountViewComponents, seedBus } from "./views/m02_viewcomponents.js";
import * as viewStore from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import * as transportApi from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
import { Command } from "./lib/message.js";
import { buildMockProject } from "./dev/mock-project.js";
import { buildDemoStoreState, seedDemoState } from "./dev/demo-state.js";

/** URL of the dev-only mock kernel Worker (same-origin HTTP required). */
const MOCK_WORKER_URL = "./dev/mock-kernel-worker.js";

const status = document.getElementById("status");

/**
 * Wire the dev-only "seed demo bus" affordances:
 *   - the top-bar button
 *   - the `riscv:seed` signal emitted by the product view
 * The demo seeder stays available as a fallback for the parts the mock
 * kernel does not model yet (the M-14 memory map).
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
 * @param {{mode: string, connected: boolean, label?: string, lastError?: string|null}|undefined} transport
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
        const label = transport.label ?? `${transport.mode} transport`;
        return `kernel 已连接 — ${label}`;
    }
    const reason = transport.lastError ? transport.lastError : "M-07 待接线";
    return `工作台已就绪 — kernel transport 未连接（${reason}）`;
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
 * Write a short note about the last command / handshake into the
 * footer. The status bar stays reserved for the transport state (the
 * gate asserts that it is derived from it), so command feedback goes
 * here.
 *
 * @param {string} text
 */
function setNote(text) {
    const note = document.getElementById("kernel-note");
    if (note) note.textContent = text;
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

/**
 * Ask the workbench to re-render one view from the store.
 *
 * @param {string} viewId
 */
function publishViewState(viewId) {
    const state = viewStore.getState(viewId);
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId, type: "update", state },
    }));
}

/**
 * Translate a view-layer `riscv:command` signal into an M-05 command.
 *
 * The view layer does not know the service layer — it only emits the
 * signal (the same rule the demo seeder follows). Every failure is
 * reported in the footer note; nothing is thrown at the view.
 *
 * @param {{action?: string, viewId?: string}} detail
 * @param {{commandRouter: object, registerMemory: object}} services
 */
async function handleCommand(detail, services) {
    const { commandRouter, registerMemory } = services;
    const action = detail?.action;
    try {
        switch (action) {
            case "step":
                await commandRouter.dispatchCommand(Command.stepN(1));
                break;
            case "step10":
                await commandRouter.dispatchCommand(Command.stepN(10));
                break;
            case "continue":
                await commandRouter.dispatchCommand(Command.continueRun());
                break;
            case "pause":
                await commandRouter.dispatchCommand(Command.pause());
                break;
            case "reset":
                await commandRouter.dispatchCommand(Command.loadProject(buildMockProject()));
                // A new project is a new session: the kernel-side
                // register subscription died with the old project, so
                // the host has to subscribe again — otherwise the
                // register view silently goes stale.
                await registerMemory.readRegisters();
                break;
            case "read-regs":
                await registerMemory.readRegisters();
                break;
            case "read-mem":
                await registerMemory.readMemory(0x100, "Word");
                break;
            default:
                return;
        }
        setNote(`命令 ${action} 完成`);
    } catch (err) {
        setNote(`命令 ${action} 失败：${err?.message ?? err}`);
    }
}

/**
 * Bring the session up: create it, load the mock project, subscribe to
 * the register stream and probe the link with a `Nop` round-trip.
 *
 * Never throws — a missing kernel (e.g. the front-end gate running
 * under Node, where `Worker` does not exist) must not take the whole
 * workbench down.
 *
 * @param {{session: object, commandRouter: object, registerMemory: object, workspace: object}} services
 */
async function startSession(services) {
    const { session, commandRouter, registerMemory, workspace } = services;
    try {
        await session.createSession({ role: workspace.role });
        const project = buildMockProject();
        await commandRouter.dispatchCommand(Command.loadProject(project));
        viewStore.setState("product", (prev) => ({
            ...prev,
            tree: project.tree,
        }));
        publishViewState("product");
        await registerMemory.ensureSubscribed();
        await commandRouter.dispatchCommand(Command.nop());
        setNote("mock 内核已就绪（Nop 往返成功）");
    } catch (err) {
        setNote(`内核未就绪：${err?.message ?? err}`);
    }
}

/**
 * Push the probe channels of the kernel into the waveform view.
 *
 * @param {object} commandRouter
 */
function wireWaveform(commandRouter) {
    commandRouter.subscribe("waveform.delta", (event) => {
        const streams = (event.value?.streams ?? []).filter((s) =>
            String(s.id ?? "").startsWith("probe."));
        if (streams.length === 0) return;
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                channels: streams.map((s) => ({
                    id: s.id,
                    label: s.label ?? s.id,
                    edges: (s.edges ?? []).length,
                })),
            },
        }));
        publishViewState("waveform");
    });
}

async function bootstrap() {
    status.textContent = "正在挂载模块…";
    // Two redundant safeguards guarantee that the view component library
    // is subscribed **before** the workbench dispatches its first
    // `riscv:render` event (otherwise every view body stays empty —
    // FRONTEND_Review.md #20):
    //   1. `mountViewComponents()` registers its listener synchronously
    //      and is called first;
    //   2. `mountWorkbench()` defers its initial route to a microtask
    //      (m01), so even an async mount cannot miss the event.
    //
    // The mounts are collected in a **named** object and resolved by key:
    // the key→value mapping is derived from the same object, so adding
    // or reordering a mount can no longer silently mis-bind a result
    // (FRONTEND_Review.md R-01). Object literal evaluation order is still
    // source order, so safeguard 1 is unaffected.
    const componentsPromise = mountViewComponents();
    const workspacePromise = mountWorkbench();
    const viewStorePromise = viewStore.mountViewStore();

    // The service layer is mounted in dependency order (M-07 → M-05 →
    // M-04 / M-10). `mountTransport` never throws: a missing `Worker`
    // (Node under the front-end gate) or a `file://` origin end up as an
    // explicit "not connected" status instead of a dead page.
    //
    // `mountTransport` returns the **status snapshot**; the adapter API
    // itself is the module namespace (ES modules are singletons), so
    // the router / session / regmem service get the live adapter.
    const transportStatus = await transportApi.mountTransport({
        mode: "worker",
        workerUrl: MOCK_WORKER_URL,
    });
    const commandRouter = await mountCommandRouter({ transport: transportApi });
    const session = await mountSession({ transport: transportApi, commandRouter });
    const registerMemory = await mountRegisterMemory({
        commandRouter,
        transport: transportApi,
        viewStore,
    });

    const pending = {
        components: componentsPromise,
        workspace: workspacePromise,
        viewStore: viewStorePromise,
        commandRouter,
        transport: transportStatus,
        session,
        registerMemory,
    };
    const { components, workspace } = Object.fromEntries(
        await Promise.all(
            Object.entries(pending).map(async ([key, value]) => [key, await value]),
        ),
    );
    void components;

    wireDevSeeding();
    exposeDevHelpers();
    describeWorkspace(workspace);

    // The status bar follows the **live** transport state, not a
    // one-shot snapshot: the mock kernel connects a tick after the
    // page came up, and the bar has to say so.
    transportApi.onStatus((st) => renderStatus(st));
    renderStatus(transportApi.getTransportStatus());

    window.addEventListener("riscv:command", (ev) => {
        void handleCommand(ev.detail, { commandRouter, registerMemory });
    });
    wireWaveform(commandRouter);

    // Fire and forget — the session comes up on its own.
    void startSession({ session, commandRouter, registerMemory, workspace });
}

bootstrap().catch((err) => {
    console.error("bootstrap failed:", err);
    status.textContent = `启动失败: ${err?.message ?? err}`;
});
