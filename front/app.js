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
import { mountViewComponents, seedBus, get as getViewComponent } from "./views/m02_viewcomponents.js";
import * as viewStore from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import * as transportApi from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
import { Command, TRACE_FORMAT, TRACE_SOURCE } from "./lib/message.js";
// ⚠️ 只导入**纯函数**（错误码拆分）⇒ 不引入服务层依赖，也不构成循环依赖。
import { splitErrorCode } from "./views/components/memory-map.js";
import { buildMockProject } from "./dev/mock-project.js";
import { buildDemoStoreState, seedDemoState } from "./dev/demo-state.js";

/** URL of the WASM kernel Worker (same-origin HTTP required). */
const KERNEL_WORKER_URL = "./kernel-worker.js";

/** How many instructions a disassembly request asks for by default. */
const DEFAULT_DISASM_COUNT = 16;

/**
 * The views that receive the **register / memory snapshot** pushed by M-10.
 *
 * ⚠️ **「source」必须在里面**（STAGE-7 验收 A-16）：源码视图的「解析当前
 * PC 的源码行」按钮从 `state.sim.pc` 取地址 ⇒ 若这个列表里没有
 * `source`，它**永远拿不到 PC** ⇒ 按钮恒问地址 **0**。
 *
 * ⚠️ 这条是 **V-B 检视（VB-1）查出的生产路径断裂**：M-10 的
 * `viewIds` 默认值是 `["debug", "regmem"]`，而 `app.js` 调
 * `mountRegisterMemory` 时**没传** ⇒ 走默认值 ⇒ `source` 收不到
 * `sim.pc`。**用例当时是绿的**，因为它**手工**往 source 视图状态里塞了
 * `sim.pc` ⇒ 断言的是一个**测试自己造出来的**状态，不是生产状态。
 *
 * ⇒ 这里把列表**显式**写出来（唯一真相），并让生产调用**用它**。
 *
 * ⚠️ **「embedded」也必须在里面**（D-33 · STAGE-10 裁决 C）：落地视图要显示
 * **当前 PC** 与**虚拟时间** ⇒ 它得跟调试视图一样收到 M-10 推进的那份
 * `sim.pc` / `timeVirt` ⇒ 前端**不造**第二个 PC 来源。
 */
export const REGISTER_MEMORY_VIEW_IDS = ["debug", "regmem", "source", "embedded"];

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

// ---------------------------------------------------------------------------
// 板级内存图（STAGE-14 · v0.29）的前端侧状态
// ---------------------------------------------------------------------------

/**
 * **当前装载着的工程**（`model::Project` 的线上形态）。
 *
 * ⚠️ 「重新装载工程」按钮需要它：板级配置是**待生效**的，宿主在**下一次**
 * `LoadProject` 时才应用 ⇒ 前端必须能**再发一次同一个工程**。
 *
 * ⚠️ 它由 {@link loadProjectIntoKernel} 记下 ⇒ **每一条**装载路径（启动、
 * 「reset」命令、装载仓外镜像）都会被记到 ⇒ 不会漏。
 *
 * @type {object|null}
 */
let loadedProject = null;

/**
 * 把「上一次提交板级内存图的错误」写进「memory-map」视图状态并重渲染。
 *
 * ⚠️ **错误显示在面板上**，不只是页脚一行字：验收 C-5 要的是「校验失败必须
 * 在界面上显示」⇒ 面板上有一块带 `data-memory-map-error` 与
 * `data-error-code` / `data-error-family` / `data-error-scenario` 的区域。
 *
 * ⚠️ 判定靠**族 + 码**（`E_PLT-07` ⇒ 族 `PLT`、场景 `07`），**不靠文案**。
 *
 * @param {{code: string, message: string}|null} error — `null` 表示清掉
 */
function setMemoryMapError(error) {
    viewStore.setState("memory-map", (prev) => ({ ...prev, error: error ?? null }));
    publishViewState("memory-map");
}

/**
 * Push the **transport link state** into a view's state.
 *
 * ⚠️ **连接状态住在服务层**（M-07 的 `getTransportStatus()` / `onStatus()`）
 * ⇒ 视图组件**不**直接摸服务层（那是 app.js 这一层的活）⇒ 这里把状态
 * **推进**视图状态（M-03），落地视图再从 `state.link` 读。
 *
 * ⚠️ 只有**落地视图**（`embedded`）拿它 ⇒ 只推进这一个视图，不给每个视图都
 * 塞一份连接状态。
 *
 * @param {{mode?: string, connected?: boolean, label?: string, lastError?: string|null}|undefined} transport
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

/**
 * A non-negative integer from a command parameter (malformed → fallback).
 *
 * The parameters arrive from the DOM (`data-*` attributes ⇒ strings), so
 * they need a conversion before they reach the contract. A malformed
 * value must not become `NaN` on the wire — that is not valid JSON.
 *
 * @param {unknown} raw
 * @param {number} [fallback]
 * @returns {number}
 */
function toInt(raw, fallback = 0) {
    const n = Math.trunc(Number(raw));
    return Number.isFinite(n) ? n : fallback;
}

/**
 * 一串 **0..255 的整数**（契约 v5 的 `WriteConsole.bytes`）。
 *
 * ⚠️ **传字符串会被内核按 `E_PROTO-08` 拒掉**（不会默默接受）⇒ 前端
 * 在这里就把它挡下来，并给出**说清是哪个字节**的错。
 *
 * @param {unknown} raw
 * @returns {number[]}
 */
export function toByteArray(raw) {
    if (!Array.isArray(raw)) {
        throw new Error(
            `console-send 的 bytes 必须是字节数组（0..255 的整数），收到：${typeof raw}`,
        );
    }
    return raw.map((b) => {
        const n = Number(b);
        if (!Number.isInteger(n) || n < 0 || n > 255) {
            throw new Error(`console-send 的字节必须是 0..255 的整数：${JSON.stringify(b)}`);
        }
        return n;
    });
}

/**
 * Translate a view-layer `riscv:command` signal into an M-05 command.
 *
 * The view layer does not know the service layer — it only emits the
 * signal (the same rule the demo seeder follows). Every failure is
 * reported in the footer note; nothing is thrown at the view.
 *
 * ⚠️ **导出是为了让它可测**（同 D-46 检视 V-B 的教训）：此前它是私有
 * 函数，前端门禁里一条用例都碰不到它——把「视图 → 命令」这段接线删掉，
 * 门禁仍然全绿。这个导出让那段接线**可被断言**。
 *
 * @param {{action?: string, viewId?: string, params?: object|null}} detail
 * @param {{commandRouter: object, registerMemory: object, waveform: object}} services
 */
export async function handleCommand(detail, services) {
    const {
        commandRouter,
        registerMemory,
        waveform,
        debug,
        console: consoleWiring,
        memoryMap: memoryMapWiring,
    } = services;
    const action = detail?.action;
    /** 探针流重订失败（仅「reset」路径可能非空）。 */
    let resetProbeError = null;
    /** console 流重订失败（仅「reset」路径可能非空）。 */
    let resetConsoleError = null;
    /** 内存图流重订失败（仅装载路径可能非空）。 */
    let resetMemoryMapError = null;
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
                // 装载工程**并且**把三条内核侧订阅都强制重订
                // （内核侧的订阅随工程一起失效了 —— 新工程 = 新会话，
                // FR-13）。这几步绑在一个函数里，调用点不会漏掉后两步。
                //
                // ⚠️ 探针流 / console 流重订失败**不判复位失败**——工程装好了、
                // 寄存器流也重订了（`loadProjectIntoKernel` 把失败单独兜住）。
                {
                    const result = await loadProjectIntoKernel(
                        {
                            commandRouter,
                            registerMemory,
                            waveform,
                            debug,
                            console: consoleWiring,
                            memoryMap: memoryMapWiring,
                        },
                        buildMockProject(),
                    );
                    resetProbeError = result.probeError;
                    resetConsoleError = result.consoleError;
                    resetMemoryMapError = result.memoryMapError;
                }
                break;
            case "read-regs":
                await registerMemory.readRegisters();
                break;
            case "read-mem":
                await registerMemory.readMemory(0x100, "Word");
                break;
            case "disasm": {
                // The disassembly request (M-11): the panel asks for a
                // bounded window starting at the current PC.
                const addr = toInt(detail.params?.addr, 0);
                const count = toInt(detail.params?.count, DEFAULT_DISASM_COUNT);
                await commandRouter.dispatchCommand(Command.disasm(addr, count));
                break;
            }
            case "resolve-line": {
                // 源码视图问内核「当前 PC 对应哪一行」（契约 v3 的
                // `ResolveLine`，M-11 的 DWARF 行表）。
                //
                // ⚠️ 源码**文本**不进内核（STAGE-7 裁决 E）：这条命令只
                // 问「行号」，问回来的也只有 `file` + `line`。
                const addr = toInt(detail.params?.addr, 0);
                await commandRouter.dispatchCommand(Command.resolveLine(addr));
                break;
            }
            case "console-send": {
                // 终端输入框：文本**按 UTF-8 编码**成的字节 ⇒ 往当前会话
                // 唯一的 UART0 写（契约 v5 的 `WriteConsole`）。
                //
                // ⚠️ **只送原始字节**：不追加换行、不模拟终端控制序列
                // （Ctrl-C / 退格 / 历史）。**回显来自 guest**，平台不做
                // 本地回显。
                const bytes = toByteArray(detail.params?.bytes);
                await commandRouter.dispatchCommand(Command.writeConsole(bytes));
                break;
            }
            case "export-trace": {
                // ⚠️ An **illegal** source/format combination is rejected
                // by the kernel (`E_CTX-03`) — the front end does not
                // pre-validate it, it just displays the kernel's answer.
                const source = String(detail.params?.source ?? TRACE_SOURCE.Execution);
                const format = String(detail.params?.format ?? TRACE_FORMAT.Csv);
                const offset = toInt(detail.params?.offset, 0);
                const limit = toInt(detail.params?.limit, 0);
                await commandRouter.dispatchCommand(
                    Command.exportTrace(source, format, offset, limit),
                );
                break;
            }
            case "trace-enable":
                await commandRouter.dispatchCommand(Command.setTraceEnabled(true));
                break;
            case "trace-disable":
                await commandRouter.dispatchCommand(Command.setTraceEnabled(false));
                break;
            case "submit-memory-map": {
                // 提交板级内存图（契约 v6 的 `SetMemoryMap`）。
                //
                // ⚠️ **载荷是整份描述**（契约已有的 `PlatformDescription`）⇒
                // 没有「当前值 + 补丁」的第二个真相。
                //
                // ⚠️ **前端不自写校验**（裁决 E）：契约层那个唯一校验器在内核
                // 侧 ⇒ 校验失败由内核报 `E_PLT-*` ⇒ 这里**如实显示**它。
                const description = detail?.params?.description ?? null;
                try {
                    await commandRouter.dispatchCommand(Command.setMemoryMap(description));
                    setMemoryMapError(null);
                } catch (err) {
                    // ⚠️ **校验失败必须在界面上显示**（验收 C-5）⇒ 码与文案
                    // 写进「memory-map」视图状态 ⇒ 面板上有一块
                    // `data-memory-map-error`。
                    setMemoryMapError(splitErrorCode(err));
                    throw err;
                }
                break;
            }
            case "reload-project": {
                // 「重新装载工程」⇒ 宿主在这次 `LoadProject` 时应用**待生效**的
                // 板级配置（规格 §2.2 / 裁决 B：已装载镜像时不重建总线 ⇒ 新板
                // 等下一次装载生效 ⇒ 界面必须显式呈现「待生效」）。
                const project = loadedProject;
                if (!project) {
                    // ⚠️ **直接返回**：走到 switch 后面那行 `setNote` 会把这条
                    // 具体的话覆盖成「命令完成」⇒ 用户看不到真正的原因。
                    setNote("还没有装载过工程 —— 无从「重新装载」");
                    return;
                }
                const result = await loadProjectIntoKernel(
                    {
                        commandRouter,
                        registerMemory,
                        waveform,
                        debug,
                        console: consoleWiring,
                        memoryMap: memoryMapWiring,
                    },
                    project,
                );
                resetProbeError = result.probeError;
                resetConsoleError = result.consoleError;
                resetMemoryMapError = result.memoryMapError;
                break;
            }
            default:
                return;
        }
        setNote(
            resetProbeError || resetConsoleError || resetMemoryMapError
                ? `命令 ${action} 完成，但流订阅不可用：${[
                    resetProbeError ? `探针流 ${resetProbeError?.message ?? resetProbeError}` : null,
                    resetConsoleError ? `console 流 ${resetConsoleError?.message ?? resetConsoleError}` : null,
                    resetMemoryMapError
                        ? `内存图流 ${resetMemoryMapError?.message ?? resetMemoryMapError}`
                        : null,
                ].filter(Boolean).join("；")}`
                : `命令 ${action} 完成`,
        );
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
    const { session, commandRouter, registerMemory, waveform, debug, workspace } = services;
    try {
        await session.createSession({ role: workspace.role });
        const project = buildMockProject();
        // 装载工程**并且**把内核侧订阅都强制重订。
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
        const { probeError, consoleError, memoryMapError } = await loadProjectIntoKernel(
            {
                commandRouter,
                registerMemory,
                waveform,
                debug,
                console: services.console,
                memoryMap: services.memoryMap,
            },
            project,
        );
        viewStore.setState("product", (prev) => ({
            ...prev,
            tree: project.tree,
        }));
        publishViewState("product");
        await commandRouter.dispatchCommand(Command.nop());
        if (probeError) {
            // ⚠️ **内核是好的**——工程装好了、寄存器流也重订了、连 Nop
            // 往返都成功了。只是**探针流**订阅不上（波形视图会是空的）。
            //
            // 这里**不能**说「内核未就绪」——那会把排查引到错误的方向
            // （D-46 检视 V-B）。
            setNote(`内核已就绪，但探针流不可用：${probeError?.message ?? probeError}`);
            return;
        }
        if (consoleError) {
            // 同理：console 流订阅不上 ⇒ 终端永远是空的，但内核是好的。
            setNote(`内核已就绪，但 console 流不可用：${consoleError?.message ?? consoleError}`);
            return;
        }
        if (memoryMapError) {
            // 同理：内存图流订阅不上 ⇒ 内存映射表是空的，但内核是好的。
            setNote(`内核已就绪，但内存图流不可用：${memoryMapError?.message ?? memoryMapError}`);
            return;
        }
        setNote("WASM 内核已就绪（Nop 往返成功）");
    } catch (err) {
        setNote(`内核未就绪：${err?.message ?? err}`);
    }
}

/**
 * 装载工程，并把**三条**内核侧订阅都强制重订。
 *
 * 四步绑在一个函数里（而不是散落在调用点的「记得再订阅一次」注释里）：
 * 调用点只管调它，不会有人忘记后两步。
 *
 * 1. `waveform.reset()` —— 新工程 = 新会话，上一个会话的探针通道
 *    不该继续挂在视图上；
 * 2. `console.reset()` —— 新程序 = 新 console，上一个程序打到串口上的
 *    字节不该留在屏幕上（与 D-45 同一个坑：状态必须跟着工程一起重建）；
 * 3. `registerMemory.loadProject()` —— 装载工程 + 强制重订寄存器流
 *    （D-45 的教训：内核侧的订阅会随工程一起失效）；
 * 4. `waveform.subscribeProbeStream()` + `console.subscribeConsoleStream()`
 *    —— 强制重订两条**流**订阅。同样的道理：`LoadProject` 把内核的主题
 *    集合清空了，探针流与 console 流不重订就**静默停掉**。
 *
 * ⚠️ **`console` 接线是必需的**（不是可选参数）：漏传它 ⇒ console 流
 * 永远订阅不上 ⇒ 终端**静默**是空的，而页面上看不出任何异常。宁可
 * 抛错也不要静默。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {object} project — `model::Project` shaped value
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>} —
 *   `probeError` 非空表示**探针流**重订失败；`consoleError` 非空表示
 *   **console 流**重订失败；`memoryMapError` 非空表示**内存图流**重订失败。
 *   **工程已经装载、寄存器流已经重订**（前 3 步成功）
 *
 * ⚠️ **第 4 步失败不许牵连前 3 步**（D-46 检视 V-B）。几步串在一起时，
 * 第 4 步抛错会让整个 `loadProjectIntoKernel` 失败——而前 3 步**已经
 * 成功了**：工程装好了、寄存器流也重订了。调用方若因此判定「内核未就绪」，
 * 就会连带跳过 Nop 往返、产品树渲染，提示语还会**误导**（说内核没就绪，
 * 其实内核好得很，只是探针流订阅不上）。
 *
 * 所以第 4 步**各自单独**兜住，失败作为 `probeError` / `consoleError` 返回
 * ——**不静默**：调用点负责把它显示出来。
 *
 * ⚠️ **两条流各自兜住**（v0.23 · console）：探针流订阅不上**不该**牵连
 * console 流（反之亦然）——它们是两条独立的流。
 *
 * ⚠️ **导出是为了让它可测**（D-46 检视 V-B）。此前它是**私有**函数，
 * 前端门禁里**一条用例都碰不到它**——把第 3 步（探针流重订）删掉，
 * `tools/check-front.sh` 仍然 **152/152 全绿**：D-46 最核心的不变量
 * 「`LoadProject` 后探针流被强制重订」**根本没有被守**。
 * 与 D-45 同一个坑——「我记得要重订」写在注释里，不如用一条用例钉住。
 */
export async function loadProjectIntoKernel(services, project) {
    const { registerMemory, waveform, debug, console: consoleWiring } = services;
    if (!consoleWiring) {
        // ⚠️ **不静默**：console 接线漏了 ⇒ 终端永远是空的，而页面上看不出
        // 任何异常 ⇒ 这是最难查的一类故障。宁可当场抛错。
        throw new Error(
            "loadProjectIntoKernel 缺少 console 接线：console 流必须被强制重订" +
            "（LoadProject 清掉了内核的主题集合）",
        );
    }
    // ⚠️ **记下当前工程** ⇒ 「重新装载工程」按钮能再发一次同一个工程
    // （板级配置是**待生效**的 ⇒ 宿主在下一次 `LoadProject` 时才应用）。
    loadedProject = project;
    waveform.reset();
    // ⚠️ 新工程 = 新程序 ⇒ **上一个程序的反汇编**留在面板上就是错的
    // （同一个 D-45 的坑：状态必须跟着工程一起重建）。
    // `debug` 是可选的——不是每个调用点都装了调试面。
    debug?.reset?.();
    // ⚠️ 新程序 ⇒ 上一个程序的 console 字节不算数（同一个坑）。
    consoleWiring.reset();
    await registerMemory.loadProject(project);
    let probeError = null;
    try {
        await waveform.subscribeProbeStream();
    } catch (err) {
        // 前面的成功**保留**：工程已装载、寄存器流已重订。
        probeError = err;
    }
    let consoleError = null;
    try {
        // ⚠️ **必须在装载之后**：内核侧的订阅随 `LoadProject` 一起失效了。
        await consoleWiring.subscribeConsoleStream();
    } catch (err) {
        consoleError = err;
    }
    // ⚠️ **内存图流也要强制重订**（STAGE-14）：`LoadProject` 清掉了内核的
    // 主题集合 ⇒ 不重订就**静默**停在旧图上（和 D-45 / D-46 同一个坑）。
    //
    // ⚠️ **接线是可选的**（`?.`）：不是每个调用点都装了内存图面 ⇒ 缺了不
    // 该让整条装载流程失败。生产路径（`bootstrap()`）**必接**。
    let memoryMapError = null;
    try {
        await services.memoryMap?.subscribeMemoryMapStream?.();
    } catch (err) {
        memoryMapError = err;
    }
    return { probeError, consoleError, memoryMapError };
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
 * 内核侧把探针边沿推成 `EventPayload::WaveformHistory`，**通道列表**和
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

    commandRouter.subscribe("waveform.history", (event) => {
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

/**
 * Wire the guest's **console** byte stream (contract v4's `Console` event)
 * into the terminal view.
 *
 * ⚠️ **console 是流，不是命令的回复**：内核侧**订阅了** `sim.console`
 * 才会推（`console_event()` 没订阅就返回 `None`）⇒ 前端必须先订阅。
 *
 * ⚠️ **载荷是字节数组**（`bytes: [104, 105]`），不是字符串 ⇒ 解码交给
 * `terminalView.onDelta`（分块展开，几万个字节也不会爆调用栈）。
 *
 * ⚠️ **主题字面量**：`"sim.console"` 必须与 `front/lib/message.js` 的
 * `EVENT_TOPIC.Console` **逐字一致**（唯一真相在那儿；这里是使用方，
 * 与 `wireWaveform` 的 `"waveform.history"` 同一个口径）。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeConsoleStream: () => Promise<object>}}
 */
export function wireConsole(commandRouter) {
    commandRouter.subscribe("sim.console", (event) => {
        const bytes = event.value?.bytes;
        if (!Array.isArray(bytes) || bytes.length === 0) return;
        const terminal = getViewComponent("terminal");
        if (!terminal) return;
        // ⚠️ 走**既有的** `onDelta` 路径：它把字节追加进 store（唯一权威）
        // 并用 `textContent` 写 DOM（**不解析 HTML** ⇒ ANSI 转义与 `<` `&`
        // 都是安全的）。这条路径此前是**死接口**（`termDelta` 没有生产者）。
        terminal.onDelta({ termDelta: { bytes } });
    });

    return {
        /**
         * 新程序 ⇒ 上一个程序打到串口上的字节不算数（与 D-45 同一个坑：
         * 状态必须跟着工程一起重建）。
         */
        reset() {
            viewStore.setState("terminal", (prev) => ({
                ...prev,
                term: { ...(prev.term ?? {}), bytes: "" },
            }));
            publishViewState("terminal");
        },
        /**
         * 订阅内核侧的 console 流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合，不重订
         * 的话 console 推送会**静默停掉**（和 D-45 / D-46 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeConsoleStream() {
            return commandRouter.dispatchCommand(Command.subscribe("sim.console"));
        },
    };
}

// ---------------------------------------------------------------------------
// 板级内存图（STAGE-14 · v0.29）
// ---------------------------------------------------------------------------

/**
 * Wire the kernel's **board memory map** (contract v6's `MemoryMap` event)
 * into the product view's M-14 table and the board-memory-map editor.
 *
 * ⚠️ **唯一缓存**（规格 §3.3 / 裁决 F）：「当前生效的图」只有**一份** ⇒
 * 产品视图那张 M-14 内存映射表与板级内存图编辑器拿到的是**同一个对象
 * 引用**（不是两份拷贝）⇒「两个视图显示的是不是同一张图」这件事是**结构上**
 * 成立的，不是靠两边各写一遍同步逻辑维持的。
 *
 * ⚠️ **事件载荷是「当前生效的图」**（不是「用户配的那份」）⇒ 表格显示的
 * 就是内核此刻真正在用的那张图；「待生效」由事件里的 `pending` 标志给出
 * （宿主的 `pending` 是**推导量**：用户的板 ≠ 当前生效的图）。
 *
 * ⚠️ **主题字面量**：`"memory.map"` 必须与 `front/lib/message.js` 的
 * `EVENT_TOPIC.MemoryMap` 逐字一致（与 `wireConsole` 同一个口径）。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeMemoryMapStream: () => Promise<object>, get: () => object|null}}
 */
export function wireMemoryMap(commandRouter) {
    /**
     * **当前生效**的图（`{description, pending}`）——唯一的那一份。
     *
     * @type {{description: object|null, pending: boolean}|null}
     */
    let current = null;

    commandRouter.subscribe("memory.map", (event) => {
        current = {
            description: event.value?.description ?? null,
            pending: event.value?.pending === true,
        };
        // ⚠️ **同一个引用**写进两个视图状态 ⇒ 两个视图不可能显示不同的图。
        viewStore.setState("product", (prev) => ({ ...prev, memoryMap: current }));
        viewStore.setState("memory-map", (prev) => ({ ...prev, memoryMap: current }));
        publishViewState("product");
        publishViewState("memory-map");
    });

    return {
        /**
         * 新会话：上一个会话的内存图不算数（与 D-45 / D-46 同一个坑：状态
         * 必须跟着工程一起重建）。
         */
        reset() {
            current = null;
            viewStore.setState("product", (prev) => ({ ...prev, memoryMap: current }));
            viewStore.setState("memory-map", (prev) => ({ ...prev, memoryMap: current }));
            publishViewState("product");
            publishViewState("memory-map");
        },
        /**
         * 订阅内核侧的内存图流。
         *
         * 必须是**强制**的：`LoadProject` 清掉了内核的主题集合 ⇒ 不重订就
         * **静默**停在旧图上（和 D-45 / D-46 是同一个坑）。
         *
         * @returns {Promise<object>}
         */
        async subscribeMemoryMapStream() {
            return commandRouter.dispatchCommand(Command.subscribe("memory.map"));
        },
        /** 当前缓存（测试用：断言两个视图拿到的是**同一个**对象）。 */
        get() {
            return current;
        },
    };
}

/**
 * Wire the M-11 debug surface (disassembly / trace export / trace switch)
 * into the debug view.
 *
 * The four events (`Disasm` / `SourceLocation` / `TraceExport` /
 * `TraceState`) are **reply events** (STAGE-5 §9.1): the kernel sends
 * them straight back to the requester, correlated by the transport
 * frame's `reqId` — they are **not** pushed streams, so there is
 * nothing to subscribe to on the kernel side.
 *
 * The router still publishes every event to its topic subscribers, so
 * the debug view picks them up by topic exactly like M-10 does with
 * `sim.registers`.
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void}}
 */
export function wireDebug(commandRouter) {
    /** Push the disassembly entries into the debug view's state. */
    commandRouter.subscribe("debug.disasm", (event) => {
        const entries = Array.isArray(event.value?.entries) ? event.value.entries : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            sim: { ...(prev.sim ?? {}), disasm: entries },
        }));
        publishViewState("debug");
    });

    /** Push the export block into the debug view's state. */
    commandRouter.subscribe("debug.export", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), export: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /** Push the trace recording state into the debug view's state. */
    commandRouter.subscribe("debug.trace", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), trace: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /**
     * The kernel's source-location reply (M-11's DWARF line table).
     *
     * ⚠️ `SourceLocation` 是**回复事件**（不是推送流）⇒ 内核侧没有可
     * 订阅的东西，但路由器会把每个事件发布给 topic 订阅者 ⇒ 源码视图
     * 按 topic 接住它（与 `debug.disasm` 同一条路）。
     *
     * ⚠️ **两种「没有答案」要能区分**：`line === 0` = 根本没有 DWARF；
     * `line === 0xFFFFFFFF` = 有 DWARF 但地址没命中行表。视图**如实**
     * 区分这两种，不显示错误的行号。
     */
    commandRouter.subscribe("debug.source", (event) => {
        viewStore.setState("source", (prev) => ({
            ...prev,
            source: {
                ...(prev.source ?? {}),
                location: event.value ?? null,
                // 换了一个地址 ⇒ 之前选定的候选**不再适用**。
                selected: null,
            },
        }));
        publishViewState("source");
    });

    /**
     * The kernel's error replies (M-05's error path — the `Error` event
     * is published to the `error` topic **and** rejects the pending
     * command). The debug view shows the last one; there is no second
     * error channel.
     */
    commandRouter.subscribe("error", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                error: {
                    code: event.value?.code ?? "E_PROTO-08",
                    message: event.value?.message ?? "",
                },
            },
        }));
        publishViewState("debug");
    });

    return {
        /**
         * New session: the disassembly / export data of the **previous**
         * program does not count.
         *
         * ⚠️ The trace **switch** state and the last error are **kept**:
         * they are the kernel's last *report*, not program data — the
         * front end does not second-guess the kernel about them.
         */
        reset() {
            viewStore.setState("debug", (prev) => ({
                ...prev,
                sim: { ...(prev.sim ?? {}), disasm: [] },
                debug: { ...(prev.debug ?? {}), export: null },
            }));
            publishViewState("debug");
            // ⚠️ **源码树留着**，但**行号**要清掉：行号是**上一个程序**的
            // 数据（新工程 = 新程序 ⇒ 旧行号是错的）。源码树是**用户装的
            // 资产**，与程序无关 ⇒ 不能跟着新工程一起没掉（STAGE-7 验收
            // A-20：装载工程后源码树仍在）。
            viewStore.setState("source", (prev) => ({
                ...prev,
                source: { ...(prev.source ?? {}), location: null, selected: null },
            }));
            publishViewState("source");
        },
    };
}

// ---------------------------------------------------------------------------
// 可编程的源码树入口（K-2）
//
// ⚠️ **源码树只进 M-03 的视图状态，不进内核、不进契约**（STAGE-7 裁决 A
// 的老规矩）：内核**不联网、不碰文件系统**（契约的 `ImageSource::Url`
// 会报 `E_LDR-01` 就是这个道理）⇒ 源码只能由前端拿到。
//
// 这个入口是给**脚本**用的（真机脚本没法操作文件选择器）⇒ 页面上的
// `window.riscv_source.setTree(treeMap)` 就是它。
// ---------------------------------------------------------------------------

/**
 * 把一棵源码树**规整**成「路径 → 内容」的对象。
 *
 * ⚠️ **接受 `Map`**（任务书指定的形态）、**也**接受普通对象与
 * `[key, value]` 对的数组 ⇒ 调用方不必为了喂它而改自己的数据结构。
 *
 * @param {Map<string, string>|Record<string, string>|Iterable<[string, string]>|null|undefined} tree
 * @returns {Record<string, string>}
 */
export function normalizeSourceTree(tree) {
    /** @type {Record<string, string>} */
    const out = {};
    if (!tree) return out;
    // `Map`（有 `get` / `forEach`）。
    if (typeof tree.get === "function" && typeof tree.forEach === "function") {
        tree.forEach((value, key) => {
            out[String(key)] = String(value ?? "");
        });
        return out;
    }
    // `[key, value]` 对的数组。
    if (Array.isArray(tree)) {
        for (const pair of tree) {
            if (!Array.isArray(pair) || pair.length < 2) continue;
            out[String(pair[0])] = String(pair[1] ?? "");
        }
        return out;
    }
    // 普通对象。
    if (typeof tree === "object") {
        for (const [key, value] of Object.entries(tree)) {
            out[String(key)] = String(value ?? "");
        }
    }
    return out;
}

/**
 * 把一棵源码树写进 **M-03 视图状态**（**不进内核**）。
 *
 * ⚠️ **并入**已有的树，不丢用户已经装好的文件（与文件选择器那条路
 * 同一口径）。
 *
 * @param {Map<string, string>|Record<string, string>|Iterable<[string, string]>} tree
 * @returns {number} 树里的文件数
 */
export function setSourceTree(tree) {
    const normalized = normalizeSourceTree(tree);
    viewStore.setState("source", (prev) => ({
        ...prev,
        source: {
            ...(prev.source ?? {}),
            tree: { ...(prev.source?.tree ?? {}), ...normalized },
        },
    }));
    publishViewState("source");
    return Object.keys(normalized).length;
}

/**
 * 把可编程的源码树入口暴露到 `window.riscv_source`。
 *
 * ⚠️ **这不是 dev-only 的 demo 辅助**（与 `window.riscv_demo` 不同）：
 * 真机脚本（`tools/browser-verify.mjs`）没法操作文件选择器 ⇒ 需要一条
 * **可编程**的路把源码树送进前端。
 *
 * @returns {void}
 */
export function exposeSourceLoader() {
    window.riscv_source = {
        setTree: (tree) => setSourceTree(tree),
    };
}

/**
 * 把一组「**脚本可调用**」的命令暴露给页面上的脚本。
 *
 * ⚠️ **这不是产品 UI**：真实 RTOS 的复验脚本（`tools/browser-verify.mjs`）
 * 需要「发一条命令 ⇒ 拿到它的回复」这一段能力——而前端的 UI 里**没有**
 * 「设置断点」或「读 UART 寄存器」的入口（那是 P1 的活）。
 *
 * ⚠️ **只暴露命名清楚的窄接口**，不把整个路由器塞给脚本 ⇒ 脚本能做的事
 * 是**看得见**的（每加一个方法都是一次显式的决定）。
 *
 * @param {object} commandRouter
 * @returns {void}
 */
export function exposeScriptCommands(commandRouter) {
    /**
     * 发一条命令，等它的回复事件 ⇒ 返回回复事件的 `value`。
     *
     * @param {object} payload — `CommandPayload` 的线上形态
     * @param {number} [timeoutMs]
     * @returns {Promise<object|null>}
     */
    async function dispatch(payload, timeoutMs) {
        const event = await commandRouter.dispatchCommand(
            payload,
            timeoutMs ? { timeoutMs } : {},
        );
        return event?.value ?? null;
    }

    /**
     * 最近一次「**停下**」的报告（契约的 `Halted` 事件）。
     *
     * ⚠️ **为什么要有它**：命令的**回复**是「带同一个 reqId 的第一个
     * 事件」——那可能是 `Tick`、也可能是 `Console` ⇒ 从回复里**读不出**
     * 「为什么停、停在哪儿」。⇒ 这里另外订阅 `halted` 主题，把
     * `Halted { reason, pc }` **如实**记下来。
     *
     * @type {{reason: string|null, pc: number}|null}
     */
    let lastHalt = null;
    commandRouter.subscribe("halted", (event) => {
        lastHalt = {
            reason: event.value?.reason ?? null,
            pc: Number(event.value?.pc ?? 0) >>> 0,
        };
    });

    window.riscv_kernel = {
        /** 最近一次「停下」的原因与 PC（`Halted` 事件）；还没有就是 `null`。 */
        lastHalt: () => lastHalt,
        /**
         * 在 `addr` 下断点（`id` 是 M-08 的断点号）。
         *
         * ⚠️ **地址必须对齐**（RISC-V 压缩指令 2 字节对齐）——M-08 会把
         * 未对齐的地址按 `E_CTX-07` 拒掉。
         */
        setBreakpoint: (addr, id = 1) => dispatch(Command.setBreakpoint({
            id: Number(id) >>> 0,
            addr: Number(addr) >>> 0,
            source_hint: null,
            enabled: true,
        })),
        /** 撤掉一个断点。 */
        clearBreakpoint: (id = 1) => dispatch(Command.clearBreakpoint(Number(id) >>> 0)),
        /** 读一段内存（`size` 是契约的 `WriteSize` 标签：`Byte` / `Half` / `Word`）。 */
        readMemory: (addr, size = "Word") => dispatch(
            Command.readMemory(Number(addr) >>> 0, size),
        ),
        /**
         * 写一段内存（`size` 是契约的 `WriteSize` 标签：`Byte` / `Half` / `Word`）。
         *
         * ⚠️ **为什么前端 UI 里没有这个入口**：真实 RTOS 的复验脚本要往
         * guest 看得见的地方写一个字节（判别式「新 RAM 段可写可读」），而
         * 页面上没有「写内存」的按钮 ⇒ 只能由脚本走这条可编程的路。
         */
        writeMemory: (addr, size = "Word", value = 0) => dispatch(
            Command.writeMemory(Number(addr) >>> 0, size, Number(value) >>> 0),
        ),
        /**
         * 继续跑。
         *
         * ⚠️ **超时放宽到 30 s**：M-05 的默认超时是 500 ms，而真实 RTOS
         * 跑一批（到下一个事件 / 断点）常常超过它 ⇒ 默认值会让这条命令
         * **假超时**（内核还在跑，前端却报了 `E_CTX-06`）。
         */
        continueRun: () => dispatch(Command.continueRun(), 30000),
    };
}

// ---------------------------------------------------------------------------
// 装载**仓外**的镜像（真实 RTOS 的 ELF 不进仓库）
// ---------------------------------------------------------------------------

/**
 * 按 URL 取镜像的**字节**。
 *
 * ⚠️ **内核不联网**：契约的 `ImageSource::Url` 会报 `E_LDR-01` ⇒ 前端
 * 必须**先把字节取回来**，再把字节塞进 `LoadProject`。
 *
 * ⚠️ **不许截断**：`arrayBuffer()` 拿的是**完整**响应体（镜像有几百 KiB：
 * osz 的 ELF 是 282 280 字节）⇒ 不能用「读一部分就停」的方式取。
 *
 * @param {string} url
 * @returns {Promise<Uint8Array>}
 */
export async function fetchImageBytes(url) {
    const res = await fetch(url);
    if (!res.ok) {
        throw new Error(`取镜像字节失败（HTTP ${res.status}）：${url}`);
    }
    const buf = await res.arrayBuffer();
    return new Uint8Array(buf);
}

/**
 * 造一个「只含一个镜像」的工程（`model::Project` 的线上形态）。
 *
 * ⚠️ **`source` 是外部标签枚举**：`model::ImageSource::Bytes(Vec<u8>)`
 * 的 serde 形态是 `{"Bytes":[...]}`（**不是** base64、**不是**字符串）。
 *
 * ⚠️ **`isa` 必须是 `"RV32IMC"`**：契约层的 `ISA` 常量，别的值会被
 * 内核判「工程 ISA 与内核支持的不一致」。
 *
 * @param {string} url — 镜像的来源（只用来取默认名字）
 * @param {Uint8Array|number[]} bytes — 镜像字节
 * @param {{id?: string, name?: string, imageName?: string, kind?: string, boardName?: string, toolchain?: string}} [options]
 * @returns {object}
 */
export function buildImageProject(url, bytes, options = {}) {
    const fileName = String(url).split("/").filter(Boolean).pop() ?? "";
    const imageName = options.imageName ?? (fileName || "image.elf");
    return {
        id: options.id ?? "proj-external-image",
        name: options.name ?? imageName,
        tree: {
            root: {
                name: options.boardName ?? "MainBoard",
                component_id: null,
                children: [],
            },
        },
        images: [{
            name: imageName,
            kind: options.kind ?? "Elf",
            source: { Bytes: Array.from(bytes) },
        }],
        version: {
            schema: 1,
            isa: "RV32IMC",
            toolchain: options.toolchain ?? "external (browser)",
        },
    };
}

/**
 * 「按 URL 取镜像字节 → 发 `LoadProject`」的入口。
 *
 * ⚠️ **装载之后必须强制重订订阅**：`LoadProject` 会清掉内核侧的主题集合
 * ⇒ 订阅 `sim.console` 必须在装载**之后**重订，否则 console 一个字节都
 * 收不到（`loadProjectIntoKernel` 把这件事绑在装载流程里，调用点不会漏）。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {string} url
 * @param {object} [options] — 传给 {@link buildImageProject}
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, byteLength: number, project: object}>}
 */
export async function loadImageFromUrl(services, url, options = {}) {
    const bytes = await fetchImageBytes(url);
    const project = buildImageProject(url, bytes, options);
    const result = await loadProjectIntoKernel(services, project);
    return { ...result, byteLength: bytes.length, project };
}

/**
 * 反复发 `Continue`，直到 console 里出现 `needle`（或到点）。
 *
 * ⚠️ **必须一条一条地发**：M-07 的 Worker 是**串行**处理帧的（一条链），
 * 而 M-05 的命令超时只有 500 ms ⇒ 一次并发发上百条的话，最后几条一定
 * 超时（内核照样会跑，但前端会记一串 `E_CTX-06`，且判定不可信）。
 *
 * ⚠️ **判据是 console 里的具体字面量**（如 `"OSZ$ "`），不是「非空」——
 * 「非空」抓不到「跑的是 demo 固件而不是真实 RTOS」这种错。
 *
 * @param {{commandRouter: object}} services
 * @param {string} needle — 期待的 console 片段
 * @param {{timeoutMs?: number, maxBatches?: number, commandTimeoutMs?: number}} [options]
 * @returns {Promise<{matched: boolean, batches: number, text: string}>}
 */
export async function runUntilConsole(services, needle, options = {}) {
    const { commandRouter } = services;
    const timeoutMs = Number(options.timeoutMs ?? 60000);
    const commandTimeoutMs = Number(options.commandTimeoutMs ?? 5000);
    const deadline = Date.now() + timeoutMs;
    let batches = 0;
    let text = "";
    while (Date.now() < deadline) {
        await commandRouter.dispatchCommand(
            Command.continueRun(),
            { timeoutMs: commandTimeoutMs },
        );
        batches += 1;
        text = viewStore.getState("terminal")?.term?.bytes ?? "";
        if (needle && text.includes(needle)) {
            return { matched: true, batches, text };
        }
        if (options.maxBatches && batches >= Number(options.maxBatches)) break;
    }
    return { matched: Boolean(needle) && text.includes(needle), batches, text };
}

/**
 * 把「装载仓外镜像 + 跑到某个 console 前缀」暴露给页面上的脚本。
 *
 * ⚠️ 这**不是** dev-only 的 demo 辅助（`window.riscv_demo`）：真实 RTOS 的
 * 镜像**在仓外**（不进仓库），浏览器要拿到它就得**自己**把字节取回来——
 * 这条链没有 UI 入口，只能由脚本（`tools/browser-verify.mjs`）驱动。
 *
 * @param {object} services
 */
function exposeImageLoader(services) {
    window.riscv_image = {
        loadFromUrl: (url, options) => loadImageFromUrl(services, url, options),
        runUntilConsole: (needle, options) => runUntilConsole(services, needle, options),
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
        // ⚠️ **必须显式传**：M-10 的默认 `viewIds` 不含 `source`
        // ⇒ 源码视图拿不到 `sim.pc`（V-B 检视 VB-1）。
        viewIds: REGISTER_MEMORY_VIEW_IDS,
    });
    // 探针接线（D-46）必须在**任何**装载之前挂好：
    // `startSession` 会装载工程并订阅探针流，订阅的响应（通道列表）
    // 到达时如果还没订阅 `waveform.history`，那批数据就丢了。
    const waveform = wireWaveform(commandRouter);
    // M-11 调试面（反汇编 / trace 导出 / trace 开关）同样必须在**任何**
    // 可能产生这些回复的命令之前挂好——回复事件虽然不订阅，但路由器会
    // 把每个事件发布给 topic 订阅者，订阅晚了这一批就丢了。
    const debug = wireDebug(commandRouter);
    // console 流（契约 v4 的 `Console` 事件）同样必须**在任何**装载之前
    // 挂好：内核侧**订阅了** `sim.console` 才会推字节流。
    const consoleWiring = wireConsole(commandRouter);
    // 板级内存图（契约 v6 的 `MemoryMap` 事件）同样必须**在任何**装载之前
    // 挂好：内核侧**订阅了** `memory.map` 才会推那张图（裁决 J：订阅即强制
    // 推一条当前生效的图）⇒ 不先挂好，第一批图就丢了。
    const memoryMapWiring = wireMemoryMap(commandRouter);

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
    // ⚠️ 可编程的源码树入口（K-2）：真机脚本没法操作文件选择器 ⇒ 需要一条
    // **可编程**的路把源码树送进前端（**不进内核**）。
    exposeSourceLoader();
    describeWorkspace(workspace);

    // ⚠️ **console 接线**必须传给**每一个**装载点（`startSession` 与
    // 「reset」命令）：漏了 ⇒ `loadProjectIntoKernel` 抛错（它要求
    // console 流被强制重订）。
    const services = {
        commandRouter,
        registerMemory,
        waveform,
        debug,
        console: consoleWiring,
        memoryMap: memoryMapWiring,
    };
    exposeImageLoader(services);
    // ⚠️ 脚本用的**命令门**（真机段要「下断点」/「读 UART 寄存器」——
    // 前端 UI 里没有这两个入口）。
    exposeScriptCommands(commandRouter);

    // The status bar follows the **live** transport state, not a
    // one-shot snapshot: the kernel worker connects a tick after the
    // page came up, and the bar has to say so.
    //
    // ⚠️ **落地视图也要知道连接状态**（D-33 · 裁决 C 的最小内容之一）⇒ 状态
    // 变化时**同时**推进视图状态（视图层不碰服务层 ⇒ 由这一层转递）。
    transportApi.onStatus((st) => {
        renderStatus(st);
        publishLinkState(st);
    });
    renderStatus(transportApi.getTransportStatus());
    publishLinkState(transportApi.getTransportStatus());

    window.addEventListener("riscv:command", (ev) => {
        void handleCommand(ev.detail, services);
    });

    // Fire and forget — the session comes up on its own.
    void startSession({ session, ...services, workspace });
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
