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
import {
    BREAKPOINT_KIND,
    downloadExportChunk,
    mountViewComponents,
    normalizeBreakpointKind,
    parseAddrInput,
} from "./views/m02_viewcomponents.js";
import * as viewStore from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import * as transportApi from "./services/m07_transport.js";
// 服务端模式的浏览器门槛（R-13）：读 `?server=` → 建/验会话 → 水合一次状态。
import * as serverModeApi from "./services/m07_servermode.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
// ⚠️ M-15 器件中间层的前端接线（STAGE-43 · v0.66 · D-175）⇒ ⇒ 器件类型
// 注册表与逐实例状态**只**从内核来（前端不抄第二份）。
import { wireDevices } from "./services/m15_devices.js";
// ⚠️ 模板库（v0.76 · STAGE-52 · 裁决 M）：清单订阅即推 + 按 id 取完整
// 描述 ⇒ ⇒ 前端**不抄**模板数据。
import { wireTemplates } from "./services/m76_templates.js";
import { Command, TRACE_FORMAT, TRACE_SOURCE, WRITE_SIZE } from "./lib/message.js";
import { docsByDevice } from "./views/components/device-docs.js";
import { buildMockProject } from "./dev/mock-project.js";
// ⚠️ **连续运行的纯函数**（STAGE-37 · v0.52 批）⇒ ⇒ 「该不该续发」**剥成纯函数**
// ⇒ ⇒ Node 侧零 DOM 就能测 ⇒ ⇒ 与 STAGE-36 的「边沿合并剥成纯函数」同形。
import {
    RUN_MODE_RUNNING,
    initialRunState,
    startRunState,
    stopRunState,
} from "./lib/run-loop.js";
// ⚠️ **「跑过没有」那一格与「运行 / 继续」文案住在**新模块**
// （`front/lib/run-action.js`）⇒ ⇒ `run-loop.js` 的**导出集合一个字都不改**
// ⇒ ⇒ 浏览器缓存里那份**旧副本照样能链接**⇒ ⇒ 最坏是「**新能力没有**」
// （降级）⇒ ⇒ **不会白屏**（ES module 的链接期是**全有或全无**的 ⇒ ⇒ 旧
// 文件少一个导出 ⇒ ⇒ **整个 app.js 都不执行** ⇒ ⇒ 页面只剩静态 HTML）。
import { markStarted, normalizeStepCount } from "./lib/run-action.js";
// ⚠️ **「读回」的纯函数**（v0.67 批）⇒ ⇒ 「基准怎么抓」「什么时候该读回」
// **剥成纯函数** ⇒ ⇒ Node 侧**零 DOM** 就能验 ⇒ ⇒ 与 v0.52 批的 run-loop
// 同形 ⇒ ⇒ 「该不该读回」不必拖一整条链路才测得到。
import { READBACK_REASON } from "./lib/readback.js";
// ⚠️ **trace 记录开关的判定**（纯函数）⇒ ⇒ 「点一下该发什么」只有一处说了算
// ⇒ ⇒ 视图层渲染的按钮标签与这里发出的命令**引同一处** ⇒ ⇒ 不会对不上。
import { nextTraceEnabled, traceSwitchState, TRACE_SWITCH_OFF } from "./lib/trace-switch.js";
// ⚠️ **「把整条轨迹翻完」的分页收集**（纯逻辑）⇒ ⇒ 「下载轨迹到本地」
//   顺着 `next_offset` 一块一块翻 ⇒ ⇒ 翻完才是**完整**的一份。
import { collectTraceChunks, normalizeTraceFilter } from "./lib/trace-export.js";
// ⚠️ **告警弹窗**（STAGE-38 · v0.53 批）⇒ ⇒ 运行中点「读内存」= 拒绝 + 弹窗。
// ⇒ ⇒ 本仓**第一个**碰 DOM 的 lib 模块（弹窗是**全局**的，不属于任何一个视图）。
import { showAlert } from "./lib/alert.js";
// ⚠️ **顶部渐隐 toast** —— 内存读 / 转储被内核拒绝时的轻量即时反馈（无按钮、
// 自动消失）；与居中模态告警 `alert.js` 分工见 `lib/toast.js` 头注。
import { showToast } from "./lib/toast.js";
// ⚠️ **App 层按域拆分**（`views/app/`）：本文件收口为门面 + bootstrap，
// 各域函数由子模块实现；公开符号在此 re-export ⇒ 既有导入路径与符号不变。
import { setNote } from "./views/app/shared.js";
import {
    syncKernelConn,
    describeTransport,
    renderStatus,
    describeWorkspace,
    publishLinkState,
} from "./views/app/link-status.js";
export { syncKernelConn, describeTransport, renderStatus, publishLinkState };
import {
    toInt,
    toByteArray,
    fetchImageBytes,
    buildImageProject,
} from "./views/app/image-project.js";
export { toByteArray, fetchImageBytes, buildImageProject };
import {
    normalizeSourceTree,
    setSourceTree,
    exposeSourceLoader,
    exposeWaveChartCommands,
} from "./views/app/source-trace.js";
export { normalizeSourceTree, setSourceTree, exposeSourceLoader, exposeWaveChartCommands };
import {
    wireWaveform,
    wireConsole,
    wireMemoryMap,
    ensureTerminalXtermLoaded,
} from "./views/app/probes-wiring.js";
export { wireWaveform, wireConsole, wireMemoryMap };
import {
    setPausePending,
    getLoadedProject,
    setPendingErrorSlot,
} from "./views/app/state.js";
import {
    updateDebugState,
    parseOptionalFilterAddr,
    normalizeCondition,
    memoryToastText,
    writeErrorSlot,
} from "./views/app/debug-state.js";
export { normalizeCondition };
import {
    captureGprBaseline,
    performReadback,
    bumpRunBusy,
    transitionRunState,
    getRunState,
    setRunState,
    isRunning,
    syncTopbarRunButton,
    syncTopbarRunStatus,
    syncTopbarStats,
    wireTopbarRunbar,
    formatVirtualTime,
} from "./views/app/run-state.js";
export {
    getRunState,
    setRunState,
    isRunning,
    syncTopbarRunButton,
    syncTopbarRunStatus,
    syncTopbarStats,
    wireTopbarRunbar,
    formatVirtualTime,
};
import {
    refreshBreakpoints,
    wireDebug,
    exposeScriptCommands,
} from "./views/app/debug-wiring.js";
export {
    refreshBreakpoints,
    wireDebug,
    exposeScriptCommands,
};
import {
    loadBoardStoreModule,
    applyProductBoard,
    loadProjectIntoKernel,
    resubscribeAllStreams,
    createBoardStoreInstance,
    deleteProductArchive,
    patchProductBoard,
    refreshProductArchive,
    writeProductArchive,
    startSession,
} from "./views/app/project-board.js";
export {
    loadBoardStoreModule,
    applyProductBoard,
    loadProjectIntoKernel,
    resubscribeAllStreams,
};
import {
    handleLoadImageCommand,
    loadImageFromUrl,
    loadImageFromBytes,
    runUntilConsole,
    exposeImageLoader,
} from "./views/app/image-loader.js";
export {
    handleLoadImageCommand,
    loadImageFromUrl,
    loadImageFromBytes,
    runUntilConsole,
};
import { wireTopbarImageLoader } from "./views/app/topbar-loader.js";
export { wireTopbarImageLoader };
// ⚠️ **连线报告**（STAGE-48 · v0.72 · D-177 A）—— 「你写的 N 根线里有 M 根
// 当前抬不了」⇒ ⇒ **内核说**的 ⇒ ⇒ 前端**只**归一化形状、**不判定**。

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
 * `viewIds` 默认值里**没有** `source`，而 `app.js` 调
 * `mountRegisterMemory` 时**没传** ⇒ 走默认值 ⇒ `source` 收不到
 * `sim.pc`。**用例当时是绿的**，因为它**手工**往 source 视图状态里塞了
 * `sim.pc` ⇒ 断言的是一个**测试自己造出来的**状态，不是生产状态。
 *
 * ⇒ 这里把列表**显式**写出来（唯一真相），并让生产调用**用它**。
 *
 * ⚠️ **「embedded」也必须在里面**（D-33 · STAGE-10 裁决 C）：落地视图要显示
 * **当前 PC** 与**虚拟时间** ⇒ 它得跟调试视图一样收到 M-10 推进的那份
 * `sim.pc` / `timeVirt` ⇒ 前端**不造**第二个 PC 来源。
 *
 * ⚠️ **「regmem」不在里面了**（v0.65 批）：那个**独立视图**已撤销 ⇒ ⇒
 * 寄存器与内存的明细都在**调试视图**里 ⇒ ⇒ 它已经在列表里 ⇒ ⇒ 列表里
 * **不再有**第二个渲染同样内容的视图 ⇒ ⇒ 「同一个量只显示一处」。
 */
export const REGISTER_MEMORY_VIEW_IDS = ["debug", "source", "embedded"];

const status = document.getElementById("status");


// ---------------------------------------------------------------------------
// 连续运行（STAGE-37 · v0.52 批）
// ---------------------------------------------------------------------------

/**
 * 「运行中点读内存」的告警文案（STAGE-38 · v0.53 批 · 裁决 F）。
 *
 * ⚠️ **文案只有这一处** ⇒ ⇒ 真机门禁读它、界面上显示它 ⇒ ⇒ 同一个词
 * ⇒ ⇒ 「界面上显示的」与「判据说看到的」**天然对得上**。
 *
 * ⚠️ **说清楚下一步该做什么**（「请先暂停再读」）⇒ ⇒ 用户看完知道怎么办
 * ⇒ ⇒ 不是只说一句「不行」。
 *
 * @type {string}
 */
export const READ_MEMORY_BLOCKED_MESSAGE = "程序正在运行，暂时不能读内存 —— 请先暂停再读。";







/**
 * Translate a view-layer `riscv:command` signal into an M-05 command.
 *
 * The view layer does not know the service layer — it only emits the
 * signal. Every failure is
 * reported in the footer note; nothing is thrown at the view.
 *
 * ⚠️ **导出是为了让它可测**（同 D-46 检视 V-B 的教训）：此前它是私有
 * 函数，前端门禁里一条用例都碰不到它——把「视图 → 命令」这段接线删掉，
 * 门禁仍然全绿。这个导出让那段接线**可被断言**。
 *
 * @param {{action?: string, viewId?: string, params?: object|null}} detail
 * @param {{commandRouter: object, registerMemory: object, waveform: object}} services
 */
/**
 * 「**暂停 ⇒ ⇒ 自动把 trace 渲染出来**」（v0.62 批）。
 *
 * ⚠️ **为什么**：trace 开着的时候，用户跑完一段、点暂停 ⇒ ⇒ 他**想看的就是
 *   「刚才录到的东西」** ⇒ ⇒ 而此前**必须再点一次「导出执行轨迹」**才看得到
 *   ⇒ ⇒ 多出来的那一下让「开了 trace ⇒ ⇒ 暂停就有东西看」这条链断了一截。
 *
 * ⚠️ **触发点只有一处**：工具条的「暂停」与顶栏的「暂停」**都**走
 *   `handleCommand({ action: "pause" })` ⇒ ⇒ 助手挂在这里 ⇒ ⇒ 两个按钮
 *   **自动**都有 ⇒ ⇒ **不写第二处**。
 *
 * ⚠️ **什么时候不导出**：**明确关掉了**（用户说了不要记录）⇒ ⇒ 不导出；
 *   **状态未知** ⇒ ⇒ 照样导出（内核**默认就在录** ⇒ ⇒ 面板自己会说出真相 ——
 *   「条目 0 → 10000 / 0」就是「一条都没录到」的真话）。
 *
 * @param {{commandRouter?: object}} services
 * @returns {Promise<object|null>}
 */
async function autoRenderTraceOnPause(services) {
    const router = services?.commandRouter;
    if (!router) return null;
    const sw = traceSwitchState(viewStore.getState("debug")?.debug?.trace ?? null);
    if (sw.state === TRACE_SWITCH_OFF) return null;
    try {
        return await router.dispatchCommand(
            Command.exportTrace(TRACE_SOURCE.Execution, TRACE_FORMAT.Csv, 0, 0),
        );
    } catch (err) {
        // ⚠️ **失败必须让用户看见**：页脚那行提示在**页面最底下**（调试视图比
        //   视口高 ⇒ ⇒ 不滚动看不见）⇒ ⇒ 失败写进**调试面板**的错误位。
        updateDebugState((prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                error: {
                    code: err?.code ?? "E_CTX-06",
                    message: `自动导出 trace 失败：${err?.message ?? String(err)}`,
                },
            },
        }));
        return null;
    }
}

/**
 * 「**下载轨迹到本地**」—— 把**整条**轨迹翻完并落盘（v0.62 批）。
 *
 * ⚠️ **为什么要翻完**：内核的导出是**分块**的（一块 10 000 条）⇒ ⇒ 只发一条
 *   命令拿到的是**第一块** ⇒ ⇒ 把它叫「下载轨迹」是**误导** ⇒ ⇒ 所以这里顺着
 *   `next_offset` **一直翻到末尾**（`collectTraceChunks`）⇒ ⇒ 落盘的才是**完整
 *   的一份**。
 *
 * ⚠️ **每一块的回包都会更新面板**（走 M-05 的 `debug.export` 事件）⇒ ⇒ 用户
 *   看得见「翻到哪儿了」，不用等下载完才有反馈。
 *
 * ⚠️ **下载动作本身复用视图层那一个**（`downloadExportChunk`）⇒ ⇒ 文件名、
 *   MIME、`<a download>` 的造法**只有一处** ⇒ ⇒ 不会两个入口各造一次。
 *
 * @param {{commandRouter?: object}} services
 * @returns {Promise<object|null>}
 */
async function downloadWholeTrace(services, options = {}) {
    const router = services?.commandRouter;
    if (!router) return null;
    // ⚠️ **源 / 格式可指派**（v0.63 批）⇒ ⇒ 「导出波形 (VCD)」之后点下载 ⇒ ⇒
    //   下的是**整份波形**⇒ ⇒ 波形那条路**没有**被这次去重**砍掉**。
    const source = String(options.source ?? TRACE_SOURCE.Execution);
    const format = String(options.format ?? TRACE_FORMAT.Csv);
    const result = await collectTraceChunks(async (offset) => {
        const ev = await router.dispatchCommand(
            Command.exportTrace(source, format, offset, 0, options.filter ?? null),
        );
        return ev?.value ?? {};
    }, { format, filter: options.filter ?? null });
    if (!result.content) return null;
    return downloadExportChunk({ source, format, chunk: result.content });
}

export async function handleCommand(detail, services) {
    const {
        commandRouter,
        registerMemory,
        waveform,
        debug,
        console: consoleBridge,
        memoryMap: memoryMapBridge,
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
                // ⚠️ **先退出连续模式**（裁决 O）⇒ ⇒ 否则下一轮 `Continue`
                // 立刻把这一步盖掉 ⇒ ⇒ 「单步」在连续模式下**不可用**。
                //
                // ⚠️ **顺带记下「跑过」**（v0.63 批）⇒ ⇒ 程序离开过起点 ⇒ ⇒
                //   顶栏那个按钮从「运行」切成「继续」。
                setRunState(markStarted(stopRunState(getRunState())));
                // ⚠️ **基准 = 这一步开始前**（v0.67 批）⇒ ⇒ 走完这一步之后，
                //   高亮说的就是「**这一步改了哪些寄存器**」⇒ ⇒ 时机可预期。
                captureGprBaseline(services);
                await commandRouter.dispatchCommand(Command.stepN(1));
                // ⚠️ **单步走完 ⇒ ⇒ 读回**（v0.67 批）⇒ ⇒ 内核此刻**静止**
                // ⇒ ⇒ 取回来的就是**走完那一步之后**的值。
                await performReadback(services, READBACK_REASON.STEP, { auto: true });
                break;
            case "step-over":
                // 「步过」与「单步」同一条收尾路径：退出连续模式、记基准、
                // 发命令、停下后读回。区别只在命令本身（内核负责返回地址停点）。
                setRunState(markStarted(stopRunState(getRunState())));
                captureGprBaseline(services);
                await commandRouter.dispatchCommand(Command.stepOver());
                await performReadback(services, READBACK_REASON.STEP, { auto: true });
                break;
            case "step-n": {
                // ⚠️ **按步数单步**（v0.63 批 · 用户拍板「在反汇编窗口中增加一个
                //   输入框和一个按钮，用于支持用户指定执行多少步」）⇒ ⇒ 契约层
                //   **零变更**（`StepN { n }` 早就有）⇒ ⇒ 只是把 `n` 从界面拿。
                //
                // ⚠️ **归一在纯函数里**（`normalizeStepCount`）⇒ ⇒ 前端**不写**
                //   第二份校验器 ⇒ ⇒ 非数 / 负数 / 0 归成 1（走 0 条 = 点了没
                //   反应）⇒ ⇒ 太大归到 `u32` 上界（那是**域**，不是业务上限）。
                const steps = normalizeStepCount(detail.params?.steps);
                setRunState(markStarted(stopRunState(getRunState())));
                captureGprBaseline(services);
                await commandRouter.dispatchCommand(Command.stepN(steps));
                await performReadback(services, READBACK_REASON.STEP, { auto: true });
                break;
            }
            case "step10":
                // ⚠️ **与「单步」同一条路**（同裁决 O）⇒ ⇒ 10 条也是「走一段
                // 然后停」⇒ ⇒ 连续模式必须让位。
                transitionRunState(stopRunState(getRunState()));
                captureGprBaseline(services);
                await commandRouter.dispatchCommand(Command.stepN(10));
                await performReadback(services, READBACK_REASON.STEP, { auto: true });
                break;
            case "continue":
                // ⚠️ **「运行」= 连续运行**（裁决 A）⇒ ⇒ 进连续模式 ⇒ 发
                // `Continue`（内核那边仍然是「跑一个窗口」）⇒ ⇒ **前端**在
                // `halted` 订阅里**接着发下一条**（裁决 C）。
                //
                // ⚠️ **幂等**（裁决 I）：已经在连续模式 ⇒ ⇒ **不重发** ⇒ ⇒
                // 连点两下不会变成两个并行窗口。
                if (getRunState().mode === RUN_MODE_RUNNING) break;
                // ⚠️ **「用户要的那次停」作废**（v0.67 批）⇒ ⇒ 用户点过暂停**又**
                //   点了运行 ⇒ ⇒ 那个「等一次停」的意图**已经被覆盖**⇒ ⇒ 不能
                //   留着去匹配**后面**某次停下。
                setPausePending(false);

                // ⚠️ **连续运行 ⇒ ⇒ 程序离开过起点** ⇒ ⇒ 「跑过没有」置真。
                setRunState({ ...startRunState(), started: true });
                // ⚠️ **基准 = 这一趟开始前**（v0.67 批）⇒ ⇒ 跑完停下之后，高亮
                //   说的就是「**这一趟改了哪些寄存器**」⇒ ⇒ 时机可预期。
                captureGprBaseline(services);
                // ⚠️ **在途计数 +1**（STAGE-38 · v0.53 批 · 裁决 B）⇒ ⇒
                // 「内核手上真的有一个窗口在跑」⇒ ⇒ 活值**立刻**隐藏。
                bumpRunBusy(+1);
                try {
                    await commandRouter.dispatchCommand(Command.continueRun());
                } catch (err) {
                    // ⚠️ **发不出去 ⇒ 退出连续模式** ⇒ ⇒ 否则界面显示
                    // 「在跑」而内核**根本没跑** ⇒ ⇒ 那是界面在骗人。
                    //
                    // ⚠️ **计数也要退回来**（裁决 B）⇒ ⇒ 否则内核**根本没跑**
                    // 而计数说「在跑」⇒ ⇒ 活值会**永远**藏着 ⇒ ⇒ 界面在骗人。
                    bumpRunBusy(-1);
                    transitionRunState(stopRunState(getRunState()));
                    throw err;
                }
                break;
            case "pause":
                // ⚠️ **不发 `Pause` 命令**（裁决 E）—— `Pause` 会把
                // `pause_requested` **留在内核里**（取走即消失）⇒ ⇒ 下一次
                // 运行会在第一条指令边界就停（F6）⇒ ⇒ **吃掉一整轮**。
                // ⇒ ⇒ 这里只**停续发**（前端状态）⇒ ⇒ 正在跑的那一个窗口
                // 跑完才停（10 µs 虚拟时间，用户感知不到）。
                //
                // ⚠️ **「跑过没有」那一格保留** ⇒ ⇒ 暂停之后顶栏那个按钮显示
                //   「继续」⇒ ⇒ **继续**这个词才说得通。
                transitionRunState(stopRunState(getRunState()));
                // ⚠️ **记一笔「用户要的那次停」**（v0.67 批）⇒ ⇒ 正在跑的那
                //   **一个**窗口跑完才停 ⇒ ⇒ 那次停下**就是**用户要的那次停 ⇒ ⇒
                //   到了就**自动读回**（寄存器 + 内存单元 + 转储）⇒ ⇒ 内存单元
                //   表**跟着刷新**。
                //
                // ⚠️ **只在「内核真的在执行」时记** ⇒ ⇒ 什么都没跑的时候点暂停
                //   ⇒ ⇒ 不会有任何东西变 ⇒ ⇒ 不该有一次读回。
                if (isRunning()) setPausePending(true);
                // ⚠️ **v0.62 批**：暂停 ⇒ ⇒ **自动把 trace 渲染出来**（trace
                //   开着 / 状态未知时）⇒ ⇒ 用户**点一下暂停**就能看到刚才录到的
                //   东西 ⇒ ⇒ 不用再点一次「导出」。
                await autoRenderTraceOnPause(services);
                break;
            case "reset":
                // ⚠️ **复位 ⇒ 退出连续模式**（裁决 H）⇒ ⇒ 否则复位后还在跑。
                // ⚠️ **「跑过没有」也归零** ⇒ ⇒ 顶栏那个按钮回到「运行」。
                setRunState({ ...initialRunState(), started: false });
                // 装载工程**并且**把三条内核侧订阅都强制重订
                // （内核侧的订阅随工程一起失效了 —— 新工程 = 新会话，
                // FR-13）。这几步绑在一个函数里，调用点不会漏掉后两步。
                //
                // ⚠️ 探针流 / console 流重订失败**不判复位失败**——工程装好了、
                // 寄存器流也重订了（`loadProjectIntoKernel` 把失败单独兜住）。
                //
                // ⚠️⚠️ **复位的对象是「当前工程」**（用户指南 §3.8「复位 = 重新装载
                // 当前工程」+ 需求 FR-06 验收「复位回到初始态」⇒ ⇒ **文档早写对了**，
                // 是**实现跑偏了**）⇒ ⇒ 装的是 `loadedProject`（**当前工程**）⇒ ⇒
                // **不是**内置 demo 工程。
                //
                // ⚠️ **为什么这条必须钉住**（v0.59 批 · 用户报的现象「复位后无法再
                // 运行」）：复位若重装 demo ⇒ ⇒ **用户刚装进去的固件被顶掉** ⇒ ⇒
                // 而界面状态条还停在「已装载」⇒ ⇒ 再点「加载固件」落进
                // `already-loaded` 那一格 ⇒ ⇒ **不重装** ⇒ ⇒ **用户的固件再也跑不起来**。
                // ⇒ ⇒ 「复位」不是「把机器擦干净」，是「**回到当前工程的起点**」。
                //
                // ⚠️ **落点唯一** = `loadedProject`（`loadProjectIntoKernel` 记下的那
                // 一份）⇒ ⇒ 与「重新装载工程」那条命令**读同一份** ⇒ ⇒ **不是第二个
                // 真相** ⇒ ⇒ 「当前工程是什么」这件事**只有一处**说了算。
                //
                // ⚠️ **还没有装载过任何工程** ⇒ ⇒ 回落内置 demo（启动那条路已经装过它
                // 了 ⇒ ⇒ 正常情况下 `loadedProject` 不会是空的 ⇒ ⇒ 这一行只是兜底）。
                //
                // ⚠️ **顺带生效的板级配置**（STAGE-16 裁决 A）：`LoadProject` 会应用
                // 「待生效的板」⇒ ⇒ 复位**与**「重新装载工程」**同一条路** ⇒ ⇒ 两条
                // 路径对「待生效板」的行为**天然一致**（不写第二套）。
                {
                    const result = await loadProjectIntoKernel(
                        {
                            commandRouter,
                            registerMemory,
                            waveform,
                            debug,
                            console: consoleBridge,
                            memoryMap: memoryMapBridge,
                            // ⚠️ 器件中间层（STAGE-43 · v0.66）—— 复位也**要**
                            // 重订那两条流（`LoadProject` 清了内核的主题集合）。
                            devices: services.devices,
                        },
                        getLoadedProject() ?? buildMockProject(),
                    );
                    resetProbeError = result.probeError;
                    resetConsoleError = result.consoleError;
                    resetMemoryMapError = result.memoryMapError;
                }
                break;
            case "write-register": {
                // 寄存器格内编辑框失焦 ⇒ 写一个 GPR（复用契约 v2 的
                // `WriteRegister`，不新增命令）。x0 与运行中由视图层挡掉；
                // 这里只做参数归一，越界仍交给内核拒绝（写是拒绝、读是钳位）。
                const index = Number(detail.params?.index);
                if (!Number.isInteger(index) || index <= 0 || index >= 32) {
                    setNote(`寄存器下标无效：${detail.params?.index}`);
                    return;
                }
                const rawValue = detail.params?.value;
                let value = null;
                if (typeof rawValue === "number" && Number.isFinite(rawValue)) {
                    value = rawValue >>> 0;
                } else {
                    // 复用视图层唯一的十六进制解析器（不在 app 里抄第二份）。
                    const parsed = parseAddrInput(String(rawValue ?? "").trim());
                    if (parsed.ok) value = parsed.value;
                }
                if (value === null) {
                    setNote(`寄存器值无效：${rawValue}`);
                    return;
                }
                await registerMemory.writeRegister(index, value);
                break;
            }
            case "read-regs": {
                // ⚠️ **手动读回**（v0.67 批）⇒ ⇒ 立即把**当前**的寄存器值取回来
                // ⇒ ⇒ 界面显示的是**内核此刻**的值，不是缓存里那份旧的。
                //
                // ⚠️ **不动基准** ⇒ ⇒ 「读回」**不是**「重新取基准」⇒ ⇒ 用户
                //   点这个按钮**不会**把正在查看的那趟对比抹掉⇒ ⇒ 高亮还在。
                //
                // ⚠️ **基准的落点从 `debug.prevGpr` 搬到了 `debug.readback.baseline`**
                //   （v0.67 批 · **口径变更**）⇒ ⇒ 原因见下：
                //     **旧口径** = 「与上一次读回相比」，而那一次「读回」取自 `sim.gpr`
                //     —— 而 `sim.gpr` 是 M-10 **被内核持续推送改写**的**活值**⇒ ⇒
                //     基准与显示值**同源**⇒ ⇒ 高亮几乎永不出现，且时机取决于内核
                //     的推送节奏 ⇒ ⇒ **用户「不知道什么时候会高亮」**（用户原话）。
                //     **新口径** = 「**与本次运行 / 单步开始前相比**」⇒ ⇒ 捕获
                //     时机**唯一**⇒ ⇒ 规则可预测。
                await performReadback(services, READBACK_REASON.MANUAL, { auto: false });
                break;
            }
            case "read-mem": {
                // ⚠️ **运行中不许读内存**（STAGE-38 · v0.53 批 · 裁决 F）
                // ⇒ ⇒ **拒绝执行** + **弹窗** ⇒ ⇒ 「请先暂停再读」。
                //
                // ⚠️ **拒绝的是「执行」不是「显示」** ⇒ ⇒ `ReadMemory`
                // **根本不发出去** ⇒ ⇒ 不是「读了但不给看」。
                //
                // ⚠️ **判据是「内核真的在执行」**（`isRunning()`）⇒ ⇒ 连续
                // 模式**或**脚本口跑的那一个窗口都算 ⇒ ⇒ 一个判据。
                //
                // ⚠️ **地址与宽度都从界面来**（STAGE-39 · v0.54 批 · 裁决 D）
                // ⇒ ⇒ 契约层**零变更**（命令的名字与形状一个字没改）⇒ ⇒
                // 只把**参数来源**从写死改成 `params`。
                if (isRunning()) {
                    showAlert(READ_MEMORY_BLOCKED_MESSAGE);
                    break;
                }
                const readAddr = toInt(detail.params?.addr, 0) >>> 0;
                const rawSize = detail.params?.size;
                const readSize = Object.prototype.hasOwnProperty.call(WRITE_SIZE, rawSize)
                    ? rawSize
                    : WRITE_SIZE.Word;
                try {
                    await registerMemory.readMemory(readAddr, readSize);
                } catch (err) {
                    // ⚠️ **越限 / 未映射等读失败**：顶部 toast 即时说原因；再抛出
                    //    ⇒ ⇒ 外层 catch 照旧写页脚 note，错误位行为不变。
                    showToast(memoryToastText(err));
                    throw err;
                }
                break;
            }
            case "disasm": {
                // The disassembly request (M-11): the panel asks for a
                // bounded window starting at the current PC.
                const addr = toInt(detail.params?.addr, 0);
                const count = toInt(detail.params?.count, DEFAULT_DISASM_COUNT);
                await commandRouter.dispatchCommand(Command.disasm(addr, count));
                break;
            }
            case "disasm-follow": {
                // ⚠️ **「跟随 PC」开关**（STAGE-45 · v0.69 批 · 台账 D-176 做法 B）
                // ⇒ ⇒ 它是**用户意图** ⇒ ⇒ 住**视图状态** `debug.followPc`
                // ⇒ ⇒ **默认关** ⇒ ⇒ 用户**不主动要求**就不动他手动选的那段。
                //
                // ⚠️ **判据取 `=== true`** ⇒ ⇒ 视图状态**没有**这一格时
                //   （第一次进页面）⇒ ⇒ 读到 `undefined` ⇒ ⇒ **当作关** ⇒ ⇒
                //   「默认关」这条规矩**结构上**成立。
                const follow = detail.params?.follow === true;
                updateDebugState((prev) => ({
                    ...prev,
                    debug: { ...(prev.debug ?? {}), followPc: follow },
                }));
                // ⚠️ **勾上时立刻重取一次**（裁决 F）⇒ ⇒ 否则用户勾了**看不到
                //   变化** ⇒ ⇒ 「点了没反应」是最差的 UX ⇒ ⇒ 勾上 = 「现在
                //   就跟上」。
                //
                // ⚠️ **PC 取自 M-10 的快照** ⇒ ⇒ 与顶栏那份**同一个源** ⇒ ⇒
                //   不是第二个真相。
                if (follow) {
                    const pc = Number(registerMemory?.getSnapshot?.()?.pc ?? 0) >>> 0;
                    await commandRouter.dispatchCommand(
                        Command.disasm(pc, DEFAULT_DISASM_COUNT),
                    );
                }
                break;
            }
            case "set-breakpoint": {
                // 三类输入**合一**（STAGE-55 · 表 13）：同一个按钮，载荷按
                // 当前**类型**组装 ⇒ ⇒ 三种类型的落库形态都是
                // `SetBreakpoint { kind, … }` ⇒ ⇒ **零新命令 / 零新 DTO**。
                //
                // ⚠️ **地址类**：id 仍然是地址（「一址一断点」这条内核语义
                //   不变，前端不需要第二个计数器）。
                // ⚠️ **符号类**：名字**由内核**解成地址（`ResolveSymbol`）
                //   ⇒ ⇒ 前端**不编**地址；解不出来就**不下断点**。
                // ⚠️ **条件类**：不绑地址 ⇒ ⇒ `addr = 0` 只是占位，`id = 0`
                //   是「**请内核分配**」（谓词条目没有地址可当 id）。
                const kind = normalizeBreakpointKind(detail.params?.kind);
                setPendingErrorSlot("bp");
                writeErrorSlot("bp", "");
                if (kind === BREAKPOINT_KIND.Condition) {
                    await commandRouter.dispatchCommand(Command.setBreakpoint({
                        id: 0,
                        addr: 0,
                        source_hint: null,
                        enabled: true,
                        kind,
                        condition: String(detail.params?.condition ?? ""),
                    }));
                    await refreshBreakpoints(commandRouter);
                    break;
                }
                if (kind === BREAKPOINT_KIND.Symbol) {
                    const name = String(detail.params?.name ?? "");
                    const reply = await commandRouter.dispatchCommand(Command.resolveSymbol(name));
                    const symbol = reply?.value?.symbol ?? null;
                    const resolved = Number(symbol?.addr);
                    if (!Number.isFinite(resolved)) {
                        // ⚠️ **解不出地址 ⇒ 如实说**：不编一个地址，也不发
                        //   一条注定落到 0 上的断点命令。
                        writeErrorSlot("bp", `没有这个符号 —— ${name}`);
                        break;
                    }
                    const addr = resolved >>> 0;
                    await commandRouter.dispatchCommand(Command.setBreakpoint({
                        id: addr,
                        addr,
                        source_hint: name,
                        enabled: true,
                        kind,
                        condition: null,
                    }));
                    await refreshBreakpoints(commandRouter);
                    break;
                }
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: null,
                    enabled: true,
                    kind: BREAKPOINT_KIND.Address,
                    condition: null,
                }));
                await refreshBreakpoints(commandRouter);
                break;
            }
            case "bp-kind": {
                // 类型下拉**换了**（表 14）：清空草稿与错误位 ⇒ ⇒ 免得把
                // 地址当表达式提交出去。草稿住在共享的 `debugger` 键里
                // （视图层写的同一个落点）。
                viewStore.setState("debugger", (prev) => ({
                    ...prev,
                    bpKind: normalizeBreakpointKind(detail.params?.kind),
                    bpDraft: "",
                    bpTabIndex: -1,
                    // 换类型 ⇒ 之前「收起过下拉」的那个输入不再适用。
                    bpSuggestDismissed: "",
                }));
                updateDebugState((prev) => ({
                    ...prev,
                    debug: { ...(prev.debug ?? {}), bpError: "" },
                }));
                break;
            }
            case "bp-suggest": {
                // 联想下拉的**开 / 收 / 高亮**变了（v0.135 批）⇒ ⇒ 只要
                // **重渲染**（草稿已经由视图层写进 `debugger` 键）⇒ ⇒
                // 下拉的 `data-bp-suggest` 与高亮才跟着变。
                //
                // ⚠️ **不重发查询**：查询由视图层的**输入防抖**触发 ⇒ ⇒ 这里
                // 只是把状态画出来（一个动作一个落点）。
                updateDebugState((prev) => prev);
                break;
            }
            case "set-symbol-breakpoint": {
                // 从**符号候选**点一下下断点（契约 v9 · 裁决 N）。
                //
                // ⚠️ **候选自带地址** ⇒ ⇒ 这一条**不需要**再解析一次
                //   ⇒ ⇒ `kind = Symbol` 只作**来源标记**落库。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                const symbol = detail.params?.symbol ? String(detail.params.symbol) : null;
                setPendingErrorSlot("bp");
                writeErrorSlot("bp", "");
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: symbol,
                    enabled: true,
                    kind: BREAKPOINT_KIND.Symbol,
                    condition: null,
                }));
                await refreshBreakpoints(commandRouter);
                break;
            }
            case "lookup-symbol": {
                // 符号查询（契约 v9 · `ResolveSymbol`）。
                //
                // ⚠️ **谁触发**（v0.135 批）：符号类型下由输入**防抖**触发
                //   （≥4 个字符）；↑↓ 在没有候选时也会手动查一次。
                // ⚠️ **空名字 / 短文不发**：视图层在 <4 个字符时**根本不发**
                //   这条命令 ⇒ ⇒ 用户不会再看到「符号名为空」那条红字。
                // ⚠️ 命令本身**不校验**（前端不写第二套校验器）⇒ ⇒ 内核真拒
                //   （`E_DBG-04`）时仍**原样显示**它的原文。
                const name = String(detail.params?.name ?? "");
                setPendingErrorSlot("bp");
                writeErrorSlot("bp", "");
                await commandRouter.dispatchCommand(Command.resolveSymbol(name));
                break;
            }
            case "dump-memory": {
                // 整段转储（契约 v9 · `DumpMemory`）。
                //
                // ⚠️ **前端不判上限** ⇒ ⇒ **一律发命令** ⇒ ⇒ 让**内核**拒
                //   （`E_DBG-03`）⇒ ⇒ 前端把**内核原文**显示在转储那一位。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                const len = toInt(detail.params?.len, 0);
                setPendingErrorSlot("dump");
                writeErrorSlot("dump", "");
                try {
                    await commandRouter.dispatchCommand(Command.dumpMemory(addr, len));
                } catch (err) {
                    // ⚠️ 转储越限同样顶部 toast；抛出后外层 catch 写页脚，`dump`
                    //    错误位由 `error` 订阅回填（既有行为不变）。
                    showToast(memoryToastText(err));
                    throw err;
                }
                break;
            }
            case "clear-breakpoint": {
                // 撤掉断点 ⇒ ⇒ **按 id**（STAGE-55）：地址类条目的 id 仍
                // 然等于地址，但**谓词条目没有地址** ⇒ ⇒ 只有 id 是可靠的
                // 句柄。列表行自己带着内核给的 id（`data-bp-id`）。
                const id = toInt(detail.params?.id, 0) >>> 0;
                await commandRouter.dispatchCommand(Command.clearBreakpoint(id));
                // ⚠️ **清除成功之后才重拉**：传输失败 / 命令失败时先删会留下
                //   一个「内核还在、界面没了」的反向假状态。
                await refreshBreakpoints(commandRouter);
                break;
            }
            case "list-breakpoints": {
                // 契约 v13：直接把内核的断点表取回来（回执照
                // `debug.breakpoints` 订阅落进视图状态）。
                await commandRouter.dispatchCommand(Command.listBreakpoints());
                break;
            }
            case "list-locals": {
                // STAGE-55 真相候选列表批：只读 ⇒ 取当前帧的「名字 + 位置」
                // （回执照 `debug.locals` 订阅落进视图状态；不做条件补全）。
                await commandRouter.dispatchCommand(Command.listLocals());
                break;
            }
            case "backtrace": {
                // STAGE-55 栈回溯批：只读 ⇒ 直接取内核算好的帧列表
                // （回执照 `debug.backtrace` 订阅落进视图状态）。
                await commandRouter.dispatchCommand(Command.backtrace());
                break;
            }
            case "toggle-breakpoint": {
                // 契约 v13：enable / disable ⇒ ⇒ **按 id**（同上）。
                // 当前状态从**内核回执那一份**（`debug.bpEntries`）里查，
                // 不另存一份启停表（那不是第二份真相，那是同一份）。
                const id = toInt(detail.params?.id, 0) >>> 0;
                const entries = viewStore.getState("debug")?.debug?.bpEntries ?? [];
                const entry = entries.find((e) => toInt(e?.id, 0) === id) ?? null;
                const next = entry ? entry.enabled === false : true;
                await commandRouter.dispatchCommand(Command.setBreakpointEnabled(id, next));
                // 内核回执（`Breakpoints`）会整表覆盖 ⇒ ⇒ 界面不必自己翻。
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
                // 终端直输：按键经视图层映射成 **UTF-8 字节** ⇒ 往当前会话
                // 唯一的 UART0 写（契约 v5 的 `WriteConsole`）。
                //
                // ⚠️ **只送原始字节**：EOL 映射 / 控制序列已在视图层按当前档
                //   处理；**回显来自 guest**，平台不做本地回显。
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
                const addrFrom = parseOptionalFilterAddr(detail.params?.filterAddrFrom);
                const addrTo = parseOptionalFilterAddr(detail.params?.filterAddrTo);
                const normalized = normalizeTraceFilter({
                    timeFrom: detail.params?.filterTimeFrom,
                    timeTo: detail.params?.filterTimeTo,
                    addrFrom,
                    addrTo,
                });
                if (!normalized.ok) {
                    setNote(`筛选条件无效：${normalized.error}`);
                    break;
                }
                // ⚠️ 下载走**同一次导出的筛选条件**：`TraceExport` 回包不带
                //   filter ⇒ 前端记住它，否则「筛完再下载」会静默退回全量。
                updateDebugState((prev) => ({
                    ...prev,
                    debug: { ...(prev.debug ?? {}), traceFilter: normalized.filter },
                }));
                // ⚠️ **v0.89**（用户拍板：「保留导出执行轨迹」）⇒ 界面上的「导出」
                //   就是**唯一**入口 ⇒ ⇒ `data-whole="1"` 的按钮点一下要**落盘**
                //   （此前这里只出「第一块」到 `<pre>`，另外还有一个平行的
                //   「下载轨迹到本地」按钮 —— 两颗按钮说同一件事，用户不知道该
                //   点哪颗）。⇒ ⇒ **一个入口、一份结果**：整份翻完并落盘 ⇒ ⇒
                //   进度行 / `<pre>` 由翻页的每一块**自然**更新（不重复发一遍
                //   第一块）。
                if (detail.params?.whole) {
                    try {
                        const result = await downloadWholeTrace(services, {
                            source,
                            format,
                            filter: normalized.filter,
                        });
                        if (result) {
                            setNote(`已导出 ${result.filename}（内核报告 ${result.total ?? "?"} 条）`);
                        }
                    } catch (err) {
                        updateDebugState((prev) => ({
                            ...prev,
                            debug: {
                                ...(prev.debug ?? {}),
                                error: {
                                    code: err?.code ?? "E_CTX-06",
                                    message: `导出落盘失败：${err?.message ?? String(err)}`,
                                },
                            },
                        }));
                        return;
                    }
                    break;
                }
                await commandRouter.dispatchCommand(
                    Command.exportTrace(source, format, offset, limit, normalized.filter),
                );
                break;
            }
            // ⚠️ **v0.89**：原来的 `case "download-trace-local"` 已删 —— 它服务的
            //   那颗「下载轨迹到本地」按钮已撤（用户拍板：保留「导出执行轨迹」）。
            //   「整份翻完并落盘」能力**没有丢**：它就是 `export-trace` 的
            //   `whole` 分支（上面那一段）。⇒ ⇒ 界面上**只有一个**导出入口，
            //   代码里也**只有一条**落盘路径。
            case "trace-toggle": {
                // ⚠️ **一个开关**（此前是两个按钮）⇒ ⇒ 「下一个值」由**视图状态**
                // 推导 ⇒ ⇒ 判定只有一处（`front/lib/trace-switch.js`）⇒ ⇒ 视图
                // 层渲染的按钮标签与这里发出的命令**不可能**对不上。
                //
                // ⚠️ **状态未知 ⇒ 第一次点 = 开启**：内核只在**这条命令的回复**
                // 里报告开关状态 ⇒ ⇒ 一次都没点过时前端**不知道** ⇒ ⇒ 第一次
                // 点击**顺带**完成「查询」（内核的报告是唯一真相）。
                const current = viewStore.getState("debug")?.debug?.trace ?? null;
                const next = nextTraceEnabled(current);
                try {
                    await commandRouter.dispatchCommand(Command.setTraceEnabled(next));
                } catch (err) {
                    // ⚠️ **失败必须让用户看见**：页脚那行提示在**页面最底下**
                    // （调试视图比视口高 ⇒ ⇒ 不滚动看不见）⇒ ⇒ 「点了没反应」
                    // 的**第二个**来源就在这儿 ⇒ ⇒ 失败写进**调试面板**的错误位
                    // （用户正看着的那块）。
                    updateDebugState((prev) => ({
                        ...prev,
                        debug: {
                            ...(prev.debug ?? {}),
                            error: {
                                code: err?.code ?? "E_CTX-06",
                                message: `切换 trace 记录开关失败：${err?.message ?? String(err)}`,
                            },
                        },
                    }));
                    return;
                }
                break;
            }
            case "create-product":
            case "load-archive":
            case "reload-product": {
                // 产品视图（双栏工作面）的三条通路：
                //   - `create-product` / `load-archive`：「创建」「从模版加载」
                //     ⇒ 换一块板（树与寄存器草稿全部重建）；
                //   - `reload-product`：「重新载入」⇒ 把草稿送进内核**并重新装载
                //     工程**（板级配置是待生效的，宿主在下一次 `LoadProject`
                //     时才应用）。
                //
                // ⚠️ **生效与 lock 无关**：锁只门控编辑，重载照常走。
                const description = detail?.params?.description ?? null;
                if (!description) {
                    setNote("没有可装载的板描述 —— 先在「创建」或「从模版加载」里选一块");
                    return;
                }
                const result = await applyProductBoard(services, description, {
                    fresh: action !== "reload-product",
                    registers: detail?.params?.registers ?? {},
                });
                resetProbeError = result.probeError;
                resetConsoleError = result.consoleError;
                resetMemoryMapError = result.memoryMapError;
                break;
            }
            case "save-board-as": {
                // 「另存为档案」：把**当前草稿 + 当前寄存器文档**复制成库里
                // 另一条档案；当前产品不动（换 id 会触发描述元数据变更）。
                const nextId = String(detail?.params?.id ?? "").trim();
                const nextName = String(detail?.params?.name ?? "").trim();
                if (!nextId) {
                    setNote("档案 id 不能为空。");
                    return;
                }
                const state = viewStore.getState("product") ?? {};
                const draft = state?.board?.description ?? null;
                if (!draft) {
                    setNote("当前没有可另存的产品。");
                    return;
                }
                // ⚠️ id 唯一是**档案**的约束 ⇒ 占用就明确拒绝（覆盖同 id 的
                // 旧档案会静默毁掉别人的板）。
                const archive = Array.isArray(state.archive) ? state.archive : [];
                if (archive.some((row) => String(row?.id) === nextId)
                    || nextId === String(draft.id ?? "")) {
                    setNote(`档案 id「${nextId}」已经存在 —— 换一个。`);
                    return;
                }
                const copy = {
                    ...draft,
                    id: nextId,
                    name: nextName || nextId,
                };
                const saved = await writeProductArchive(
                    services,
                    copy,
                    docsByDevice(state),
                );
                if (saved.saved) {
                    // ⚠️ **v0.77（STAGE-53）**：存完**当次就刷新**档案列表
                    //    ⇒ 否则「另存为」之后从模板加载里**选不到刚存的那块**。
                    await refreshProductArchive(services);
                    setNote(`已保存到本机数据库：${nextId}（产品本身未改动）。`);
                } else {
                    setNote(`另存失败：${saved.reason ?? "库不可用"}（草稿还在，换块板名再试）。`);
                }
                return;
            }
            case "list-templates": {
                // 模版弹层的数据源：板档案服务（M-23）。
                await refreshProductArchive(services);
                return;
            }
            case "unload-project": {
                // 「卸载工程」⇒ 契约 v7 的 `UnloadProject`（D-139）。
                //
                // ⚠️ **只发这一条命令**（不发别的契约命令）⇒ 卸载的是
                // 「当前这个会话」。
                await commandRouter.dispatchCommand(Command.unloadProject());
                // ⚠️ **成功之后**才清前端缓存（状态必须跟着会话一起重建
                // ——D-45 的坑）：寄存器 / 波形 / 反汇编 / console。
                registerMemory?.reset?.();
                waveform?.reset?.();
                debug?.reset?.();
                consoleBridge?.reset?.();
                // ⚠️ 卸载 ⇒ 内核侧的主题集合被清空 ⇒ **必须重订全部流**
                // （裁决 F）⇒ 复用装载那套助手，不写第二套。
                const errors = await resubscribeAllStreams(services);
                resetProbeError = errors.probeError;
                resetConsoleError = errors.consoleError;
                resetMemoryMapError = errors.memoryMapError;
                // ⚠️ **不清「当前工程」的记忆**（`loadedProject`）⇒
                // 「重新装载工程」按钮**仍可用** ⇒ 用户不必重新选文件。
                break;
            }
            case "reload-project": {
                // 「重新装载工程」⇒ 宿主在这次 `LoadProject` 时应用**待生效**的
                // 板级配置（规格 §2.2 / 裁决 B：已装载镜像时不重建总线 ⇒ 新板
                // 等下一次装载生效 ⇒ 界面必须显式呈现「待生效」）。
                const project = getLoadedProject();
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
                        console: consoleBridge,
                        memoryMap: memoryMapBridge,
                        // ⚠️ 器件中间层（STAGE-43 · v0.66）—— 同上。
                        devices: services.devices,
                    },
                    project,
                );
                resetProbeError = result.probeError;
                resetConsoleError = result.consoleError;
                resetMemoryMapError = result.memoryMapError;
                break;
            }
            case "delete-product-archive":
                // 产品视图「档案管理」的删除：只动本机档案。
                await deleteProductArchive(services, detail?.params?.id);
                return;
            // ⚠️ **v0.58 批：这里**没有** `load-image` 分支了** ⇒ ⇒ 装载入口
            // **只有页头那一个** ⇒ ⇒ 视图层**不再**发这条信号 ⇒ ⇒ 这个分支
            // 已是**死代码**⇒ ⇒ 删掉（⛔ 不留不再被用的分支）。
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
 * 服务端模式的启动：绑定会话 + 取一次状态 + 把状态推给视图。
 *
 * ⚠️ 命令面（v1.11 起）已经可用：M-05 → M-07 的写通道走
 *    `POST /api/sessions/{id}/command`（整条契约消息进出）⇒ ⇒ 界面上的
 *    运行 / 单步 / 断点 / 读内存与本地模式同一条路径。
 *
 * ⚠️ 与本地 `startSession()` 仍然**刻意不同**的只有两件：
 *   - 不装载工程：服务端的会话有自己的镜像，页面开起来不该替它 `LoadProject`
 *     （那会把别人的会话清空重来）；
 *   - 不做 `Nop` 往返：没有「刚装载完」这个时刻，也就没有可验证的写通道。
 *     水合只发**事件**（`sim.state.delta` / `tick`）—— 用一次 `GET …/state`
 *     的真实读数，不编造「命令成功」这种前端自己给自己回的假回执。
 *
 * 会话被回收（404）时 `ensureSession` 会重建一条 ⇒ ⇒ 这里照常接，
 * 但页脚要写明「重建了一条」，不假装还是原来那条。
 *
 * @param {{session: object, commandRouter: object, params: object, binding: object|null, failure: (Error|null)}} args
 */
async function startServerMirror({ session, commandRouter, params, binding, failure }) {
    if (!binding) {
        setNote(`服务端模式没起来：${failure?.message ?? "没有可绑定的会话"}`);
        return;
    }
    try {
        await session.createSession({ id: binding.sessionId, role: "host" });
        const state = await serverModeApi.fetchSessionState({
            serverUrl: params.serverUrl,
            sessionId: binding.sessionId,
            token: params.token,
        });
        // 一次性水合：`pc / 寄存器 / 虚拟时间` 先落进视图，后面的推进由 WS
        // 事件流接着喂（服务端的会话默认就订阅了 `sim.state.delta`）。
        const delta = serverModeApi.stateDeltaEvent(state);
        if (delta) {
            commandRouter.publish({
                variant: "SimStateDelta",
                value: delta,
                topic: "sim.state.delta",
                seq: 0,
                reqId: null,
                sessionId: binding.sessionId,
            });
        }
        const tick = serverModeApi.tickEvent(state);
        if (tick) {
            commandRouter.publish({
                variant: "Tick",
                value: tick,
                topic: "tick",
                seq: 0,
                reqId: null,
                sessionId: binding.sessionId,
            });
        }
        const board = binding.board ? ` · 板 ${binding.board}` : "";
        const recycled = binding.restarted ? " · 原会话已回收，重建了一条" : "";
        setNote(`服务端模式：${binding.sessionId}${board}${recycled}（事件走事件流；命令走契约命令端点）`);
    } catch (err) {
        setNote(`服务端模式没起来：${err?.message ?? err}`);
    }
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

    // ⚠️ **服务端模式**（R-13）：页面地址带 `?server=…` 就**不**起本地内核，
    // 改成「REST 建/验会话 + WebSocket 只读事件流」：
    //   `?server=http://127.0.0.1:9876[&session=<id>][&token=<tok>][&board=<id>]`
    // 不给 `?server=` 时一个字都不改（本地 Worker 那条照旧）。
    const serverParams = serverModeApi.parseServerParams(
        typeof globalThis.location?.search === "string" ? globalThis.location.search : "",
    );
    /** @type {{sessionId: string, created: boolean, restarted: boolean, board: string|null}|null} */
    let serverBinding = null;
    /** @type {(Error & {code?: string})|null} */
    let serverFailure = null;
    if (serverParams) {
        try {
            serverBinding = await serverModeApi.ensureSession(serverParams);
        } catch (err) {
            // ⚠️ **不吞**：会话没建起来就如实记下，下面 `startServerMirror`
            //    把原因写到页脚；传输层那条（没有 sessionId ⇒ E_TRX-03）
            //    同时在状态栏，两处说的是「同一件事的两个面」。
            serverFailure = err;
        }
    }

    // The service layer is mounted in dependency order (M-07 → M-05 →
    // M-04 / M-10). `mountTransport` never throws: a missing `Worker`
    // (Node under the front-end gate) or a `file://` origin end up as an
    // explicit "not connected" status instead of a dead page.
    //
    // `mountTransport` returns the **status snapshot**; the adapter API
    // itself is the module namespace (ES modules are singletons), so
    // the router / session / regmem service get the live adapter.
    const transportStatus = await transportApi.mountTransport(
        serverParams
            ? {
                mode: "websocket",
                serverUrl: serverParams.serverUrl,
                sessionId: serverBinding?.sessionId ?? serverParams.sessionId ?? null,
                role: "host",
                // ⚠️ 命令走 `POST /api/sessions/{id}/command`（v1.11）⇒ ⇒ 令牌
                //    必须跟到传输层（服务端的 `require_token` 只认这个头）。
                token: serverParams.token,
            }
            : { mode: "worker", workerUrl: KERNEL_WORKER_URL },
    );
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
    const debug = wireDebug(commandRouter, {
        // ⚠️ **自动读回要 M-10**（v0.67 批）⇒ ⇒ 停下之后把寄存器与内存单元
        //   **重新取回来**⇒ ⇒ 内存单元表跟着刷新。
        registerMemory,
    });
    // console 流（契约 v4 的 `Console` 事件）同样必须**在任何**装载之前
    // 挂好：内核侧**订阅了** `sim.console` 才会推字节流。
    const consoleBridge = wireConsole(commandRouter);
    // 板级内存图（契约 v6 的 `MemoryMap` 事件）同样必须**在任何**装载之前
    // 挂好：内核侧**订阅了** `memory.map` 才会推那张图（裁决 J：订阅即强制
    // 推一条当前生效的图）⇒ 不先挂好，第一批图就丢了。
    const memoryMapBridge = wireMemoryMap(commandRouter);
    // 器件中间层的两张表（STAGE-43 · v0.66）同样必须**在任何**装载之前挂好：
    // 内核侧**订阅了** `device.types` / `device.instances` 才会推 ⇒ ⇒ 不先挂好，
    // 第一批就丢了（与 `memory.map` 同一个坑）。
    const devicesBridge = wireDevices(commandRouter);
    // ⚠️ 模板库（v0.76 · STAGE-52 · 裁决 M）—— 与器件中间层**同形**：
    // 内核侧**订阅了** `platform.templates` 才会推清单 ⇒ ⇒ 不先挂好，
    // 模板库卡就一直「清单还没到」。
    const templatesBridge = wireTemplates(commandRouter);
    // 板档案（STAGE-15 · v0.30）：建存储实例 + **恢复档案列表**。
    //
    // ⚠️ **不自动重新提交**（裁决 D）：刷新后板**还在**，但**要重新提交**才
    // 生效 ⇒ 自动提交在「已装载镜像」时会变成「挂起待生效」⇒ 用户刷新
    // 一次就多一次「改了板没反应」的困惑 ⇒ 这里**不发任何命令**。
    const board = await createBoardStoreInstance();
    if (board.store) {
        try {
            const entries = await board.store.list();
            patchProductBoard({
                archive: Array.isArray(entries) ? entries : [],
                archiveStatus: "ok",
            });
        } catch (err) {
            patchProductBoard({ archiveStatus: "error" });
            setNote(`板档案读取失败：${err?.message ?? err}`);
        }
    } else {
        // ⚠️ **显式**降级（不静默）：面板上明说「板档案只存在于本会话」。
        patchProductBoard({ archiveStatus: board.status });
        if (board.reason) setNote(board.reason);
    }

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
        console: consoleBridge,
        memoryMap: memoryMapBridge,
        // ⚠️ 器件中间层（STAGE-43 · v0.66）⇒ ⇒ 装载时要跟着重订两条流。
        devices: devicesBridge,
        // ⚠️ 模板库（v0.76 · STAGE-52 · 裁决 M）⇒ ⇒ 装载时要跟着重订清单流。
        templates: templatesBridge,
        // ⚠️ 板档案的**存储实例**（可能是 `null` ⇒ 「只存在于本会话」）。
        boardStore: board.store,
    };
    exposeImageLoader(services);
    // ⚠️ **页头的「加载固件」入口**（STAGE-33）⇒ ⇒ 必须在这里挂（`services`
    // 要用）⇒ ⇒ 它的按钮走**同一个** `handleLoadImageCommand` ⇒ ⇒ 一条
    // 装载路径。
    wireTopbarImageLoader(services);
    // ⚠️ **顶栏那条全局运行条**（STAGE-34 · v0.49 批）⇒ ⇒ 三个按钮走注入的
    // `dispatch` ⇒ ⇒ 与调试视图那份**共用同一条命令路径**（`handleCommand`）。
    wireTopbarRunbar(services, (detail) => handleCommand(detail, services));
    // ⚠️ 脚本用的**命令门**（真机段要「下断点」/「读 UART 寄存器」——
    // 前端 UI 里没有这两个入口）。
    exposeScriptCommands(commandRouter);
    // ⚠️ **波形库来源**的脚本口（STAGE-36 · v0.51 批）⇒ ⇒ 真机门禁靠它
    // **主动**触发降级 ⇒ ⇒ ⇒ 降级路径**永远**有判别力。
    exposeWaveChartCommands(waveform);

    // The status bar follows the **live** transport state, not a
    // one-shot snapshot: the kernel worker connects a tick after the
    // page came up, and the bar has to say so.
    //
    // ⚠️ **落地视图也要知道连接状态**（D-33 · 裁决 C 的最小内容之一）⇒ 状态
    // 变化时**同时**推进视图状态（视图层不碰服务层 ⇒ 由这一层转递）。
    transportApi.onStatus((st) => {
        renderStatus(st);
        // ⚠️ **连接胶囊那个点**（STAGE-34）⇒ ⇒ 颜色跟着**真实**传输状态走
        // ⇒ ⇒ 胶囊里的**文字**仍是 `#status` 那一份 ⇒ ⇒ 文案只有一处。
        syncKernelConn(st);
        publishLinkState(st);
    });
    renderStatus(transportApi.getTransportStatus());
    syncKernelConn(transportApi.getTransportStatus());
    publishLinkState(transportApi.getTransportStatus());

    window.addEventListener("riscv:command", (ev) => {
        void handleCommand(ev.detail, services);
    });
    // 终端视图一露面 ⇒ 就懒触发终端库（在收到串口字节之前也能直输）。
    window.addEventListener("riscv:render", (ev) => {
        if (ev.detail?.viewId === "terminal") ensureTerminalXtermLoaded();
    });

    // Fire and forget — the session comes up on its own.
    // ⚠️ 服务端模式**不**走本地 `startSession()`（它会 `LoadProject` + `Nop`
    //    往返，两条都是服务端模式下不存在的写通道）⇒ ⇒ 换成观察面那一支。
    if (serverParams) {
        void startServerMirror({
            session,
            commandRouter,
            params: serverParams,
            binding: serverBinding,
            failure: serverFailure,
        });
    } else {
        void startSession({ session, ...services, workspace });
    }
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
