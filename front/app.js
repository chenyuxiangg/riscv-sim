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

/** URL of the WASM kernel Worker (same-origin HTTP required). */
const KERNEL_WORKER_URL = "./kernel-worker.js";

const status = document.getElementById("status");

/**
 * Wire the dev-only "seed demo bus" affordances:
 *   - the top-bar button
 *   - the `riscv:seed` signal emitted by the product view
 * The demo seeder stays available as a fallback for the parts the
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
 * @param {{commandRouter: object, registerMemory: object, waveform: object}} services
 */
async function handleCommand(detail, services) {
    const { commandRouter, registerMemory, waveform } = services;
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
                // 装载工程**并且**把两条内核侧订阅都强制重订
                // （内核侧的订阅随工程一起失效了 —— 新工程 = 新会话，
                // FR-13）。这三步绑在一个函数里，调用点不会漏掉后两步。
                await loadProjectIntoKernel(
                    { commandRouter, registerMemory, waveform },
                    buildMockProject(),
                );
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
 * the register + probe streams and probe the link with a `Nop` round-trip.
 *
 * Never throws — a missing kernel (e.g. the front-end gate running
 * under Node, where `Worker` does not exist) must not take the whole
 * workbench down.
 *
 * @param {{session: object, commandRouter: object, registerMemory: object, waveform: object, workspace: object}} services
 */
async function startSession(services) {
    const { session, commandRouter, registerMemory, waveform, workspace } = services;
    try {
        await session.createSession({ role: workspace.role });
        const project = buildMockProject();
        // 装载工程**并且**把两条内核侧订阅都强制重订。
        //
        // 为什么必须是「强制」：`LoadProject` 会清掉内核侧的主题集合
        // （新工程 = 新会话，FR-13），而 `ensureSubscribed()` 是**幂等**
        // 的——本地标志可能还写着「已订阅」（会话绑定
        // `createSession` → `bindSession` → 传输状态变化会触发一次），
        // 实际内核侧的订阅却已经随旧工程一起没了。结果就是寄存器推送
        // **静默停掉**，视图停在旧值上，不报任何错。
        //
        // 这个缺陷在 mock 内核时代就存在，是第 1 阶段前端接上**真内核**
        // 之后、被真机断言「x1 必须是 42」逼出来的。
        await loadProjectIntoKernel({ commandRouter, registerMemory, waveform }, project);
        viewStore.setState("product", (prev) => ({
            ...prev,
            tree: project.tree,
        }));
        publishViewState("product");
        await commandRouter.dispatchCommand(Command.nop());
        setNote("WASM 内核已就绪（Nop 往返成功）");
    } catch (err) {
        setNote(`内核未就绪：${err?.message ?? err}`);
    }
}

/**
 * 装载工程，并把**两条**内核侧订阅都强制重订。
 *
 * 三步绑在一个函数里（而不是散落在调用点的「记得再订阅一次」注释里）：
 * 调用点只管调它，不会有人忘记后两步。
 *
 * 1. `waveform.reset()` —— 新工程 = 新会话，上一个会话的探针通道
 *    不该继续挂在视图上；
 * 2. `registerMemory.loadProject()` —— 装载工程 + 强制重订寄存器流
 *    （D-45 的教训：内核侧的订阅会随工程一起失效）；
 * 3. `waveform.subscribeProbeStream()` —— 强制重订探针流。同样的道理：
 *    `LoadProject` 把内核的主题集合清空了，探针流不重订就**静默停掉**。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object}} services
 * @param {object} project — `model::Project` shaped value
 *
 * ⚠️ **导出是为了让它可测**（D-46 检视 V-B）。此前它是**私有**函数，
 * 前端门禁里**一条用例都碰不到它**——把第 3 步（探针流重订）删掉，
 * `tools/check-front.sh` 仍然 **152/152 全绿**：D-46 最核心的不变量
 * 「`LoadProject` 后探针流被强制重订」**根本没有被守**。
 * 与 D-45 同一个坑——「我记得要重订」写在注释里，不如用一条用例钉住。
 */
export async function loadProjectIntoKernel(services, project) {
    const { registerMemory, waveform } = services;
    waveform.reset();
    await registerMemory.loadProject(project);
    await waveform.subscribeProbeStream();
}

/**
 * 波形流的 id 前缀：**寄存器**波形流归 M-10（寄存器视图）消费。
 *
 * ⚠️ 判别方向不能反：第 1 阶段那版写的是「只认 `probe.` 开头的」，
 * 而**真内核**的探针通道 id 是 `"{外设 id}.{信号名}"`（M-18 的
 * `drain_waveform`），也就是 `GPIO0.OUT`——它不叫 `probe.*`。写成只认
 * `probe.` 的话，真内核的通道会被**全部过滤掉**，波形视图永远是空的。
 *
 * 所以判别式是「**不是**寄存器流」，而不是「是某种前缀」。
 */
const REGISTER_STREAM_PREFIX = "cpu.";

/**
 * Wire the kernel's probe channels into the waveform view (D-46).
 *
 * 内核侧把探针边沿推成 `EventPayload::WaveformDelta`，**通道列表**和
 * **边沿增量**走的是同一个事件形状（契约冻结，没有单独的「通道列表」
 * 变体）。所以这里：
 *
 * - 通道列表：从边沿事件的 `streams` 里读（订阅时内核会**强制**推一次
 *   空边沿的波形，通道就在里面）；
 * - 边沿计数：**累加**。内核每次推送的是「自上次推送以来」的边沿
 *   （`drain_waveform` 会清空），直接覆盖的话，视图上永远只有最后一批
 *   的边沿数。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeProbeStream: () => Promise<object>}}
 */
export function wireWaveform(commandRouter) {
    /**
     * 累加出来的通道表。内核每次只推**增量**，所以边沿数必须在这里
     * 累加，不能被下一批覆盖。
     *
     * @type {Map<string, {id: string, label: string, edges: number, lastValue: number|null}>}
     */
    const channels = new Map();

    commandRouter.subscribe("waveform.delta", (event) => {
        const streams = (event.value?.streams ?? []).filter(
            (s) => !String(s.id ?? "").startsWith(REGISTER_STREAM_PREFIX),
        );
        if (streams.length === 0) return;
        for (const stream of streams) {
            const id = String(stream.id ?? "");
            if (!id) continue;
            const edges = Array.isArray(stream.edges) ? stream.edges : [];
            const entry = channels.get(id) ?? {
                id,
                label: stream.label ?? id,
                edges: 0,
                lastValue: null,
            };
            entry.label = stream.label ?? entry.label ?? id;
            entry.edges += edges.length;
            if (edges.length > 0) {
                entry.lastValue = edges[edges.length - 1].value ?? entry.lastValue;
            }
            channels.set(id, entry);
        }
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                channels: [...channels.values()],
            },
        }));
        publishViewState("waveform");
    });

    return {
        /** 新会话：旧的通道不算数（FR-13 —— 每个 session 独立命名空间）。 */
        reset() {
            channels.clear();
            viewStore.setState("waveform", (prev) => ({
                ...prev,
                wave: { ...(prev.wave ?? {}), channels: [] },
            }));
            publishViewState("waveform");
        },
        /**
         * 订阅内核侧的探针流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合，不重订
         * 的话探针推送会**静默停掉**（和 D-45 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeProbeStream() {
            return commandRouter.dispatchCommand(Command.subscribe("sim.waveform"));
        },
    };
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
        workerUrl: KERNEL_WORKER_URL,
    });
    const commandRouter = await mountCommandRouter({ transport: transportApi });
    const session = await mountSession({ transport: transportApi, commandRouter });
    const registerMemory = await mountRegisterMemory({
        commandRouter,
        transport: transportApi,
        viewStore,
    });
    // 探针接线（D-46）必须在**任何**装载之前挂好：
    // `startSession` 会装载工程并订阅探针流，订阅的响应（通道列表）
    // 到达时如果还没订阅 `waveform.delta`，那批数据就丢了。
    const waveform = wireWaveform(commandRouter);

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
    // one-shot snapshot: the kernel worker connects a tick after the
    // page came up, and the bar has to say so.
    transportApi.onStatus((st) => renderStatus(st));
    renderStatus(transportApi.getTransportStatus());

    window.addEventListener("riscv:command", (ev) => {
        void handleCommand(ev.detail, { commandRouter, registerMemory, waveform });
    });

    // Fire and forget — the session comes up on its own.
    void startSession({ session, commandRouter, registerMemory, waveform, workspace });
}

/**
 * 自动启动的开关。
 *
 * ⚠️ `app.js` 是**引导入口**——**导入即启动**（`bootstrap()` 是模块级的
 * 副作用）。这对浏览器是对的，对**导入即断言**的前端门禁是灾难：
 * `bootstrap()` 里的 `startSession()` 是**异步**的，它会调
 * `waveform.reset()`——而 M-03 的视图状态是**模块级单例**。于是那条
 * reset 可能落在**任意两个用例之间**，把上一个用例刚推进 store 的通道
 * 清掉。
 *
 * 实测（V-B 检视 · probe D2）：推入 1 条通道后 0 ms 再看，store 里的通道
 * **变成 0 条**——reset 落下来了。当前的用例之所以还是绿的，**只因为时序
 * 恰好对**。
 *
 * 所以：需要导入 `app.js` 但**不要**它的自启动副作用的测试，先设
 * `globalThis.__RVSIM_NO_BOOTSTRAP__ = true`。
 */
const NO_BOOTSTRAP_FLAG = "__RVSIM_NO_BOOTSTRAP__";

if (!globalThis[NO_BOOTSTRAP_FLAG]) {
    bootstrap().catch((err) => {
        console.error("bootstrap failed:", err);
        status.textContent = `启动失败: ${err?.message ?? err}`;
    });
}
