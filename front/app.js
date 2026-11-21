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
    adoptProductDescription,
    downloadExportChunk,
    markProductApplied,
    mountViewComponents,
    parseAddrInput,
} from "./views/m02_viewcomponents.js";
import * as viewStore from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import * as transportApi from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
// ⚠️ M-15 器件中间层的前端接线（STAGE-43 · v0.66 · D-175）⇒ ⇒ 器件类型
// 注册表与逐实例状态**只**从内核来（前端不抄第二份）。
import { wireDevices } from "./services/m15_devices.js";
// ⚠️ 模板库（v0.76 · STAGE-52 · 裁决 M）：清单订阅即推 + 按 id 取完整
// 描述 ⇒ ⇒ 前端**不抄**模板数据。
import { wireTemplates } from "./services/m76_templates.js";
import { Command, TRACE_FORMAT, TRACE_SOURCE, WRITE_SIZE } from "./lib/message.js";
// ⚠️ 只导入**纯函数**（错误码拆分、草稿转换、「已被内核接受」推导量）⇒
// 不引入服务层依赖，也不构成循环依赖。
import {
    boardErrorHint,
    draftToDescription,
    planMemoryMapUpdate,
    splitErrorCode,
} from "./views/components/memory-map.js";
import { docsByDevice, docsForKeyed } from "./views/components/device-docs.js";
import { projectTreeFrom } from "./views/components/product-workspace.js";
// ⚠️ **镜像加载的状态写入**（STAGE-32 · v0.47 批）⇒ ⇒ 与视图层**共用**
// ⇒ ⇒ 住在一处 ⇒ ⇒ **不写第二份**。
//
// ⚠️ **STAGE-33 · v0.48 批**：页头那个「加载固件」入口**也**要用同一套
// ⇒ ⇒ 于是「读字节 + 存 pending」那段**从视图层搬到了共享组件**
// ⇒ ⇒ 这里 import 的东西变多了 ⇒ ⇒ 但**状态仍然只有一份**
// （`product.imageLoad`）。
//
// ⚠️ **v0.58 批**：入口**收敛成一个**（页头那个）⇒ ⇒ 产品视图里那块重复的
// 面板降级成**纯状态显示**⇒ ⇒ `imageLoadDecision` 是「点一下该怎么办」的
// **唯一**判定 ⇒ ⇒ 「连点两次」的第二下**不再报假失败**。
import {
    describeImageLoadShort,
    IMAGE_ENTRY_FROM,
    imageLoadDecision,
    loadImageFileIntoPending,
    publishProductState,
    renderImageLoaderControl,
    setImageLoadState,
} from "./views/components/image-loader.js";
import { buildMockProject } from "./dev/mock-project.js";
// ⚠️ **连续运行的纯函数**（STAGE-37 · v0.52 批）⇒ ⇒ 「该不该续发」**剥成纯函数**
// ⇒ ⇒ Node 侧零 DOM 就能测 ⇒ ⇒ 与 STAGE-36 的「边沿合并剥成纯函数」同形。
import {
    HALT_REASON_END_OF_EXECUTION,
    RUN_MODE_IDLE,
    RUN_MODE_RUNNING,
    RUN_ACTION_CONTINUE,
    RUN_ACTION_STOP,
    bumpBusy,
    initialRunState,
    isKernelRunning,
    liveValue,
    nextRunAction,
    runStateAfter,
    startRunState,
    stopRunState,
} from "./lib/run-loop.js";
// ⚠️ **「跑过没有」那一格与「运行 / 继续」文案住在**新模块**
// （`front/lib/run-action.js`）⇒ ⇒ `run-loop.js` 的**导出集合一个字都不改**
// ⇒ ⇒ 浏览器缓存里那份**旧副本照样能链接**⇒ ⇒ 最坏是「**新能力没有**」
// （降级）⇒ ⇒ **不会白屏**（ES module 的链接期是**全有或全无**的 ⇒ ⇒ 旧
// 文件少一个导出 ⇒ ⇒ **整个 app.js 都不执行** ⇒ ⇒ 页面只剩静态 HTML）。
import { markStarted, normalizeStepCount, runActionLabel } from "./lib/run-action.js";
// ⚠️ **「读回」的纯函数**（v0.67 批）⇒ ⇒ 「基准怎么抓」「什么时候该读回」
// **剥成纯函数** ⇒ ⇒ Node 侧**零 DOM** 就能验 ⇒ ⇒ 与 v0.52 批的 run-loop
// 同形 ⇒ ⇒ 「该不该读回」不必拖一整条链路才测得到。
import {
    READBACK_REASON,
    captureBaseline,
    mergeReadbackState,
    readbackReasonOf,
    shouldAutoReadback,
} from "./lib/readback.js";
// ⚠️ **trace 记录开关的判定**（纯函数）⇒ ⇒ 「点一下该发什么」只有一处说了算
// ⇒ ⇒ 视图层渲染的按钮标签与这里发出的命令**引同一处** ⇒ ⇒ 不会对不上。
import { nextTraceEnabled, traceSwitchState, TRACE_SWITCH_OFF } from "./lib/trace-switch.js";
// ⚠️ **「反汇编跟随 / 陈旧标注」的纯函数**（STAGE-45 · v0.69 批 · 台账 D-176）
// ⇒ ⇒ 「该不该按新 PC 重取一段」**剥成纯函数** ⇒ ⇒ Node 侧**零 DOM** 就能验
// ⇒ ⇒ 与 v0.67 批的读回**同一手法** ⇒ ⇒ 「停下后要不要跟随」不必拖一整条
// 链路才测得到。
import { shouldFollowDisasm } from "./lib/disasm-follow.js";
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
import { setNote, publishViewState } from "./views/app/shared.js";
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
    consumePausePending,
    getLoadedProject,
    setLoadedProject,
    getPendingErrorSlot,
    setPendingErrorSlot,
} from "./views/app/state.js";
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

/**
 * 页头那条短状态条**只订阅一次**（`riscv:render` 是**窗口级**事件）。
 *
 * ⚠️ **为什么要这个标志**：这个函数可能被多次调用（测试里每次 `setup()` 都
 * 调）⇒ ⇒ 每次都 `addEventListener` ⇒ ⇒ 监听器会**堆积** ⇒ ⇒ 一次状态
 * 变化触发 N 次同步 ⇒ ⇒ 白费。⇒ ⇒ 标志只管**这一个**监听器（控件本体
 * 每次都重挂 ⇒ ⇒ 按钮的监听器总是跟着新元素走 ⇒ ⇒ 不会指向旧元素）。
 */
let topbarBriefWired = false;

/**
 * Wire the **top-bar** firmware loader (STAGE-33 · v0.48 批).
 *
 * ⚠️ **为什么在 app 层**：页头**不属于任何视图面板** ⇒ ⇒ 视图层**管不到**
 * 它 ⇒ ⇒ 事件只能挂在这里（页头由 app 层统一接线）。
 *
 * ⚠️ **控件本体由共享组件渲染**（`renderImageLoaderControl`）⇒ ⇒ `accept`
 * 列表与按钮文案**只有一处** ⇒ ⇒ **不写第二套**。
 *
 * ⚠️ **状态同源**：页头那条短状态条**只读** `product.imageLoad` ⇒ ⇒ 不是
 * 第二个真相 ⇒ ⇒ 同步挂在 **`riscv:render` 事件**上（**不**挂
 * `viewStore.subscribe` ⇒ ⇒ `reset()` 会清空订阅者 ⇒ ⇒ 会永久失联）。
 *
 * ⚠️ **页头的输入框不在视图面板里** ⇒ ⇒ 视图重渲染（`body.innerHTML` 被
 * 换掉）**碰不到它** ⇒ ⇒ 用户选完文件后，页头那个输入框**仍然留着那个
 * 文件** ⇒ ⇒ 真机门禁因此能拿到一条**正向**判据（「输入框里真的有那个
 * 文件」）⇒ ⇒ 而产品视图那个输入框**做不到**（重渲染就被换掉了）。
 *
 * ⚠️ **v0.58 批：这是**唯一**那个加载入口**（产品视图里那个重复的入口
 * 撤掉了）⇒ ⇒ 「点一下该怎么办」由 `imageLoadDecision` **一处**判定 ⇒ ⇒
 * 四格：`loading`（幂等忽略）/ `load`（装）/ `already-loaded`（**不报失败**）
 * / `no-file`（**真的**没选过文件才报错）。
 *
 * ⚠️ **v0.59 批加第五格**：`reading`（**正在读那个文件**）⇒ ⇒ 读取期间
 * ⛔ **不许**说「已装载」⇒ ⇒ 用户刚选的新文件那一下**不会被吞掉**⇒ ⇒
 * 详见 `imageLoadDecision` 那里写的原因。
 *
 * ⚠️ **导出是为了让它可测**（同 `handleCommand` 的理由）：真机门禁要断言
 * 「默认落地页上就有这个入口」⇒ ⇒ 那条断言必须能在**不启动整个 app** 的
 * 情况下把控件挂上去。
 *
 * @param {object} services
 */
export function wireTopbarImageLoader(services) {
    const slot = document.querySelector("[data-image-load-slot]");
    if (!slot) return;
    slot.innerHTML = renderImageLoaderControl({
        label: "固件镜像",
        buttonText: "加载固件",
        withBrief: true,
    });
    // ⚠️ **读字节必须在 `change` 里**（浏览器把文件**只**交给这个事件；
    // 点击时 `input.files` 已经是空的了）⇒ ⇒ 「读字节 + 存 pending」那段
    // **住在共享组件里**（`loadImageFileIntoPending`）⇒ ⇒ **不写第二套**。
    const fileInput = slot.querySelector(
        `[data-image-input][data-image-load-from="${IMAGE_ENTRY_FROM}"]`,
    );
    fileInput?.addEventListener("change", (ev) => {
        const file = ev.target?.files?.[0] ?? null;
        if (!file) return;
        void loadImageFileIntoPending(file);
    });
    // ⚠️⚠️ **同一个文件重选要能再次触发 `change`**（v0.59 批 · 用户报的现象
    // 「复位后无法再运行」的第二段）。
    //
    // ⚠️ **浏览器的行为**：`<input type="file">` 的**值没变** ⇒ ⇒ **不触发**
    // `change` ⇒ ⇒ 用户「装一次 ⇒ 复位 ⇒ **再选同一个固件**」时**选不动** ⇒ ⇒
    // 固件再也回不来 ⇒ ⇒ 界面在骗人（看起来「点了没反应」）。
    //
    // ⚠️ **为什么落点在 `click` 而不是 `change`**：`change` **压根不触发** ⇒ ⇒
    // 在那个事件里清 `value` 是**永远轮不到**的 ⇒ ⇒ 必须**在选择器打开之前**
    // 清 ⇒ ⇒ `click` 是**唯一**能提前知道「用户又要选文件了」的那个事件。
    //
    // ⚠️ **清 `value` 不影响任何既有判据**：清完之后 `input.files` 为空 ⇒ ⇒
    // 用户**选完**文件浏览器**自己**把新文件填回去 ⇒ ⇒ 真机门禁那条「输入框里
    // 真的有那个文件」的**正向**判据**照旧成立**（它走 CDP 选文件 ⇒ ⇒ **不经过**
    // `click` ⇒ ⇒ 值不会被清）。
    fileInput?.addEventListener("click", (ev) => {
        // ⚠️ **清空当前选择** ⇒ ⇒ 于是「选回同一个文件」在浏览器看来**也是变了**
        // ⇒ ⇒ `change` 照常触发 ⇒ ⇒ 「重装同一个固件」这条路**重新通上**。
        ev.target.value = "";
    });
    // ⚠️ **「装载」⇒ 走那**一条**装载路径**（`handleLoadImageCommand`）
    // ⇒ ⇒ 「重订流 / 清残留」只有一处。
    slot.querySelector(`[data-action="load-image"][data-image-load-from="${IMAGE_ENTRY_FROM}"]`)
        ?.addEventListener("click", () => {
            // ⚠️ **判定只有一处**（`imageLoadDecision`）⇒ ⇒ 四格的后果
            // 各不相同 ⇒ ⇒ 尤其「上次装成功了」那一格 **⛔ 不报失败**。
            const decision = imageLoadDecision(viewStore.getState("product")?.imageLoad);
            if (decision.kind === "loading") {
                // ⚠️ **幂等**：正在装 ⇒ ⇒ 这一下**忽略** ⇒ ⇒ 连点两下
                // **不会**发第二条 `LoadProject`（内核被连着换两次工程）。
                return;
            }
            if (decision.kind === "reading") {
                // ⚠️ **读取中**（v0.59 批）⇒ ⇒ 字节**还没到**⇒ ⇒ ⛔ **不许**
                // 说「已装载」⇒ ⇒ 用户刚选的新文件**没被吞掉**⇒ ⇒ 如实说
                // 「正在读」⇒ ⇒ 用户等状态条变成「已选…」再点一下。
                setNote("正在读取固件 —— 等页头状态条变成「已选…」之后再点「加载固件」");
                return;
            }
            if (decision.kind === "already-loaded") {
                // ⚠️ **用户报的缺陷**：装完再点一下 ⇒ ⇒ 上次**成功**了 ⇒ ⇒
                // ⛔ **不许**报「装载失败」⇒ ⇒ 如实说「已装载，无需重复加载」。
                setNote(`固件「${decision.name}」已装载 —— 无需重复加载（要换固件请重新选文件）`);
                return;
            }
            if (decision.kind === "no-file") {
                setImageLoadState({
                    status: "error",
                    error: "没有待加载的文件 —— 请先用文件选择器选一个固件镜像",
                    pending: null,
                });
                publishProductState();
                setNote("加载固件失败：没有待加载的文件 —— 请先用文件选择器选一个固件镜像");
                return;
            }
            void handleLoadImageCommand({ params: decision.pending }, services);
        });
    if (!topbarBriefWired) {
        topbarBriefWired = true;
        window.addEventListener("riscv:render", syncTopbarImageBrief);
    }
    syncTopbarImageBrief();
}

/**
 * 把 `product.imageLoad` **同步**到页头那条短状态条。
 *
 * ⚠️ **只读**（不写）⇒ ⇒ 页头**不持有**第二份状态 ⇒ ⇒ 唯一真相仍是
 * `product.imageLoad`。
 */
function syncTopbarImageBrief() {
    const brief = document.querySelector("[data-image-brief]");
    if (!brief) return;
    const state = viewStore.getState("product")?.imageLoad ?? null;
    brief.textContent = describeImageLoadShort(state);
    brief.setAttribute("data-image-brief-status", String(state?.status ?? "idle"));
}

// ---------------------------------------------------------------------------
// 顶栏那条**全局运行条**（STAGE-34 · v0.49 批）
// ---------------------------------------------------------------------------

/**
 * 把**虚拟时间**格式化成人类可读的一串。
 *
 * ⚠️ **单位是皮秒**（契约层 `Tick::elapsed_ps` / `SimState::virtual_time_ps`
 * 都是 ps）⇒ ⇒ 这里是**真实单位**、不是换算过的纳秒。
 *
 * @param {number|string|null|undefined} ps
 * @returns {string}
 */
export function formatVirtualTime(ps) {
    const v = Number(ps ?? 0);
    if (!Number.isFinite(v) || v < 0) return "—";
    if (v < 1e3) return `${v} ps`;
    if (v < 1e6) return `${(v / 1e3).toFixed(1)} ns`;
    if (v < 1e9) return `${(v / 1e6).toFixed(3)} μs`;
    return `${(v / 1e9).toFixed(3)} ms`;
}

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
 * 记下「**本次运行 / 单步开始前**」的寄存器基准（v0.67 批）。
 *
 * ⚠️ **基准 = 跑之前那一份** ⇒ ⇒ 「什么时候会高亮」这个问题**有答案了**：
 *   运行 / 单步**开始前**记下基准 ⇒ ⇒ 跑完 / 步完停下 ⇒ ⇒ 亮出这一趟改动的
 *   寄存器 ⇒ ⇒ **时机唯一、规则可预测**。
 *
 * ⚠️ **取自 M-10 的缓存**（不是视图状态）⇒ ⇒ 视图状态那份是**节流 200 ms**
 *   推的 ⇒ ⇒ 刚跑完那一下读它会拿到**旧值** ⇒ ⇒ 基准会**错**。
 *
 * ⚠️ **拷贝**（`captureBaseline` 里 `slice`）⇒ ⇒ 基准**不引用**活值 ⇒ ⇒
 *   跑完之后基准**不会**跟着变 ⇒ ⇒ 高亮**不会**恒空（那正是旧缺陷的形态）。
 *
 * @param {{registerMemory?: object|null}} services
 * @returns {number[]|null} 记下的基准
 */
function captureGprBaseline(services) {
    const snapshot = services?.registerMemory?.getSnapshot?.() ?? null;
    const baseline = captureBaseline(snapshot?.gpr ?? null);
    viewStore.setState("debug", (prev) => ({
        ...prev,
        debug: {
            ...(prev.debug ?? {}),
            readback: mergeReadbackState(prev.debug?.readback, {
                baseline,
                baselineAt: Date.now(),
            }),
        },
    }));
    return baseline;
}

/**
 * **读回** —— 把用户正在看的那几样东西**重新从内核取回来**（v0.67 批）。
 *
 * ⚠️ **取三样**：① **寄存器**（32 个 GPR）② **内存单元**（用户读过的那些，
 *   按**同一宽度**重读）③ **「读这一段」的转储结果**（`debug.dump` 那份）⇒ ⇒
 *   「运行 → 暂停」之后，内存单元表**跟着刷新**（用户报的那个现象）。
 *
 * ⚠️ **基准不动** ⇒ ⇒ 「读回」**不是**「重新取基准」⇒ ⇒ 用户点「读寄存器」
 *   **不会**把正在查看的那趟对比抹掉。
 *
 * ⚠️ **失败不吞**：错误**写进 `debug.readback.error`** ⇒ ⇒ 界面**看得见**
 *   ⇒ ⇒ 而不是「静默地没刷新」。
 *
 * @param {object} services — `{ registerMemory, commandRouter }`
 * @param {string} reason — `READBACK_REASON` 里的一个
 * @param {{auto?: boolean}} [options] — `auto` = 是不是自动触发的
 * @returns {Promise<void>}
 */
async function performReadback(services, reason, options = {}) {
    const registerMemory = services?.registerMemory ?? null;
    const commandRouter = services?.commandRouter ?? null;
    const errors = [];
    // ① 寄存器
    try {
        await registerMemory?.readRegisters?.();
    } catch (err) {
        errors.push(err?.message ?? String(err));
    }
    // ② 内存单元（**用户读过的那些** ⇒ ⇒ 地址与宽度都从 M-10 那份来）
    //
    // ⚠️ **宽度必须按原来那份** ⇒ ⇒ 同一个地址按 `Byte` 读与按 `Word` 读是
    //   **两个不同的值** ⇒ ⇒ 按错宽度重读 = 报一个**错的值**。
    const reads = registerMemory?.getSnapshot?.()?.memoryReads ?? [];
    for (const item of reads) {
        try {
            await registerMemory?.readMemory?.(item.addr, item.size);
        } catch (err) {
            errors.push(err?.message ?? String(err));
        }
    }
    // ③ 「读这一段」的转储结果（长度 = 已转储的字节数 ⇒ ⇒ 不用另存一份长度）
    const dump = viewStore.getState("debug")?.debug?.dump ?? null;
    if (
        commandRouter
        && dump
        && Number.isFinite(Number(dump.addr))
        && Array.isArray(dump.bytes)
        && dump.bytes.length > 0
    ) {
        try {
            await commandRouter.dispatchCommand(
                Command.dumpMemory(Number(dump.addr) >>> 0, dump.bytes.length),
            );
        } catch (err) {
            errors.push(err?.message ?? String(err));
        }
    }
    // ④ 「跟随 PC」的重取（STAGE-45 · v0.69 批 · 台账 D-176 做法 B）
    //
    // ⚠️ **落在读回这条链路上** ⇒ ⇒ 「停下」这个时机**只有一处定义**
    //   （真停与单步**同一处**）⇒ ⇒ **不写第二张表** ⇒ ⇒ 连续运行的每个
    //   窗口**不触发**（`EndOfExecution` 不在真停表里）。
    //
    // ⚠️ **判据在纯函数里**（`shouldFollowDisasm`）⇒ ⇒ Node 侧**零 DOM**
    //   就能验 ⇒ ⇒ 这里只是一次**调用**。
    //
    // ⚠️ **PC 取自 M-10 的快照** ⇒ ⇒ 与顶栏那份**同一个源** ⇒ ⇒ 不是第二个
    //   真相；⚠️ 而且它**已经是新的** —— 内核每次 `Continue` / `StepN` 结束
    //   都推一个**带 `pc` 的 `Tick`** ⇒ ⇒ 而 M-05 的 `handleFrame` 是
    //   「**先 resolve 命令的 promise、再 publish 事件**」⇒ ⇒ publish 是
    //   **同步**的 ⇒ ⇒ 微任务恢复时 `Tick` **已经**被 M-10 处理过了 ⇒ ⇒
    //   **无竞态**。
    const followPc = viewStore.getState("debug")?.debug?.followPc === true;
    if (commandRouter && shouldFollowDisasm({ followPc, reason })) {
        const pc = Number(registerMemory?.getSnapshot?.()?.pc ?? 0) >>> 0;
        try {
            await commandRouter.dispatchCommand(
                Command.disasm(pc, DEFAULT_DISASM_COUNT),
            );
        } catch (err) {
            errors.push(err?.message ?? String(err));
        }
    }
    // ⑤ 源码联动（STAGE-7 A-16 · 本批自动化）：**真停**后顺手问内核
    // 「当前 PC 对应哪一行」⇒ ⇒ 源码视图不用再点按钮。
    //
    // ⚠️ **只跟自动读回走**（`options.auto === true`）⇒ ⇒ 手动「读寄存器」
    //   不是停下事件 ⇒ ⇒ 不额外发 `ResolveLine`。
    //
    // ⚠️ **PC 来自 M-10 快照**（与顶栏 / 反汇编同一个源）⇒ ⇒ 不是第二个
    //   真相；PC 未知时**不发**（地址 0 不等于「未知」⇒ ⇒ 见 D-107 的口径）。
    if (commandRouter && options.auto === true) {
        const snapshot = registerMemory?.getSnapshot?.();
        const pcRaw = Number(snapshot?.pc);
        if (Number.isFinite(pcRaw)) {
            try {
                await commandRouter.dispatchCommand(Command.resolveLine(pcRaw >>> 0));
            } catch (err) {
                errors.push(err?.message ?? String(err));
            }
        }
    }
    // ⑥ 写状态（**基准那一格不动** ⇒ ⇒ `mergeReadbackState` 只并 patch）
    updateDebugState((prev) => ({
        ...prev,
        debug: {
            ...(prev.debug ?? {}),
            readback: mergeReadbackState(prev.debug?.readback, {
                reason,
                auto: options.auto === true,
                at: Date.now(),
                error: errors.length > 0 ? errors[0] : null,
            }),
        },
    }));
    // ⚠️ **立刻推一次** ⇒ ⇒ M-10 的推送是**节流 200 ms** 的 ⇒ ⇒ 不推的话，
    //   界面会先显示「旧值 + 新基准」再显示「新值」⇒ ⇒ 那一下的对比是**错的**
    //   ⇒ ⇒ 用户会看到一帧**假高亮**。
    registerMemory?.flushRender?.();
}

/**
 * **连续运行状态**（STAGE-37 · v0.52 批 · 裁决 F）。
 *
 * ⚠️ **住视图状态 `debug.run`**（即 `debug` 视图状态里的 `run` 那一格）⇒ ⇒
 * 与 `debug.halt`（内核的停下报告）**分开**⇒ ⇒ 两个不同的真相、两个落点
 * ⇒ ⇒ 「内核说它停了」与「前端还想跑」**不混在一处**⇒ ⇒ 视图层读哪个
 * 都说得清。
 *
 * ⚠️ **读的时候补默认值** ⇒ ⇒ 视图状态没写过（第一次进页面）时读到的
 * 也是一份**合法**的状态 ⇒ ⇒ 调用方不必到处判空。
 *
 * ⚠️ **`busy` = 在途的 `Continue` 计数**（STAGE-38 · v0.53 批 · 裁决 B）
 * ⇒ ⇒ 它与 `mode` 是**两件事**：`mode` 说「前端还想连续跑」，`busy` 说
 * 「内核手上**真的**有一个窗口在跑」⇒ ⇒ 脚本口跑的那**一个**窗口靠
 * `busy` 盖住（F9 / F10）。
 *
 * @returns {{mode: string, windows: number, busy: number}}
 */
export function getRunState() {
    const run = viewStore.getState("debug")?.debug?.run;
    return {
        mode: run?.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE,
        windows: Number.isFinite(run?.windows) ? Number(run.windows) : 0,
        busy: Number.isFinite(run?.busy) ? Math.max(0, Number(run.busy)) : 0,
        started: run?.started === true,
    };
}

/**
 * 写**连续运行状态**（STAGE-37 · v0.52 批 · 裁决 F）。
 *
 * ⚠️ **写完就同步顶栏那个状态位** ⇒ ⇒ 「在跑」**看得见**（裁决 G）⇒ ⇒
 * 状态位**不另订阅** ⇒ ⇒ 少一条同步路径 ⇒ ⇒ 少一处可能不同步的地方。
 *
 * ⚠️ **两个视图都发布**（STAGE-38 · v0.53 批 · 裁决 M）⇒ ⇒ 寄存器视图
 * 也要跟着隐藏活值 ⇒ ⇒ 不发布它的话它会**等到下一次 M-10 刷新**才变
 * ⇒ ⇒ 「停止后自动恢复最新值」会**慢一拍**。
 *
 * @param {{mode?: string, windows?: number, busy?: number}} next
 * @returns {{mode: string, windows: number, busy: number}} 写进去的那一份
 */
export function setRunState(next) {
    const mode = next?.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const windows = Number.isFinite(next?.windows) ? Number(next.windows) : 0;
    const busy = Number.isFinite(next?.busy) ? Math.max(0, Number(next.busy)) : 0;
    const started = next?.started === true;
    viewStore.setState("debug", (prev) => ({
        ...prev,
        debug: {
            ...(prev.debug ?? {}),
            run: { mode, windows, busy, started },
        },
    }));
    publishViewState("debug");
    syncTopbarRunStatus();
    // ⚠️ **顶栏那一份活值也要跟着隐藏 / 恢复** ⇒ ⇒ 与视图**同拍** ⇒ ⇒
    // 不等下一次 M-10 刷新（那最多差 200 ms）。
    syncTopbarStats(topbarServices);
    return { mode, windows, busy, started };
}

/**
 * 「**内核真的在执行**吗」（STAGE-38 · v0.53 批 · 裁决 A）。
 *
 * ⚠️ **这是全前端唯一的「运行中」判据** ⇒ ⇒ 顶栏 / 调试视图 / 寄存器
 * 视图 / 「读内存」的拒绝**全都问它** ⇒ ⇒ 一处口径 ⇒ ⇒ 不会各自漂。
 *
 * @returns {boolean}
 */
export function isRunning() {
    return isKernelRunning(getRunState());
}

/**
 * 在途计数 ±1（STAGE-38 · v0.53 批 · 裁决 B）。
 *
 * ⚠️ **只数 `Continue`**（F2：`StepN` 通常**不**产 `Halted` ⇒ ⇒ 数它就得
 * 再要一条退休路径 ⇒ ⇒ 两个真相）⇒ ⇒ 退休点**只有一处**（`halted` 订阅）。
 *
 * @param {number} delta — `+1` / `-1`
 * @returns {{mode: string, windows: number, busy: number}}
 */
function bumpRunBusy(delta) {
    return transitionRunState(bumpBusy(getRunState(), delta));
}

/**
 * 迁移**运行状态**，并**保住「跑过没有」那一格**。
 *
 * ⚠️ **为什么需要它**：`front/lib/run-loop.js` 那几个纯函数只搬 `mode` /
 *   `windows` / `busy`（**v0.63 批起那个文件一个字未动** ⇒ ⇒ 缓存里的旧
 *   副本**逐字可用**）⇒ ⇒ 「跑过没有」这一格**由 app 层自己搬**⇒ ⇒ 每次
 *   迁移都得**显式**带上它 ⇒ ⇒ 否则暂停一下那一格就没了 ⇒ ⇒ 顶栏那个按钮
 *   会**莫名其妙**切回「运行」。
 *
 * @param {{mode?: string, windows?: number, busy?: number}} next
 * @param {{started?: boolean}} [prev] — 迁移**前**的状态（默认读当前）
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
function transitionRunState(next, prev = getRunState()) {
    return setRunState({ ...next, started: prev?.started === true });
}

/**
 * 把顶栏那个**运行按钮**的文案同步过来（**运行 / 继续**）。
 *
 * ⚠️ **为什么要有两句话**：用户拍板「**当加载之后顶栏的运行应该显示运行，点击停
 *   之后需要切换为继续（文本切换）**」⇒ ⇒ 那条按钮**同一个动作**（继续往下跑）
 *   但**两种语义**（「从头跑」/「接着跑」）⇒ ⇒ 按钮上必须**说出当下是哪种**⇒ ⇒
 *   用户点之前就知道自己点了会得到什么。
 *
 * ⚠️ **判据只有一处**（`runActionLabel` ⇒ ⇒ 纯函数住在
 *   `front/lib/run-loop.js`）⇒ ⇒ 界面上显示的与判据说看到的**天然对得上**⇒ ⇒
 *   不是两处各写一遍。
 *
 * ⚠️ **同时挂一个原始属性**（`data-topbar-run-label`）⇒ ⇒ 判据可以读**原始
 *   值**而不是解析中文 ⇒ ⇒ 与 `data-topbar-run-mode` 同一手法。
 *
 * @returns {{label: string, started: boolean}|null}
 */
export function syncTopbarRunButton() {
    const btn = document.querySelector('[data-topbar-action="run"]');
    if (!btn) return null;
    const run = getRunState();
    const label = runActionLabel(run);
    btn.textContent = label;
    btn.setAttribute("data-topbar-run-label", label);
    return { label, started: run.started === true };
}

/**
 * 把**连续运行状态**同步到顶栏那个状态位（STAGE-37 · v0.52 批 · 裁决 G）。
 *
 * ⚠️ **文案只有这一处** ⇒ ⇒ 真机门禁读它、前端用例读它 ⇒ ⇒ 同一个词
 * ⇒ ⇒ 「界面上显示的」与「判据说看到的」**天然对得上**。
 *
 * ⚠️ **同时挂两个原始属性**（`data-topbar-run-mode` /
 * `data-topbar-run-windows`）⇒ ⇒ 判据可以读**原始值**而不是解析中文
 * ⇒ ⇒ 与 `data-topbar-time-ps` 同一手法。
 *
 * @returns {{mode: string, windows: number, text: string}|null}
 */
export function syncTopbarRunStatus() {
    // ⚠️ **按钮那一句也要跟着变**（v0.63 批）⇒ ⇒ 「运行 / 继续」与状态位
    //   **同拍** ⇒ ⇒ 不是两处各写一遍 ⇒ ⇒ 少一处可能不同步的地方。
    syncTopbarRunButton();
    const el = document.querySelector("[data-topbar-run-status]");
    if (!el) return null;
    const run = getRunState();
    const text = run.mode === RUN_MODE_RUNNING
        ? `连续运行中 · 已跑 ${run.windows} 轮`
        : `已停止 · 跑了 ${run.windows} 轮`;
    el.textContent = text;
    el.setAttribute("data-topbar-run-mode", run.mode);
    el.setAttribute("data-topbar-run-windows", String(run.windows));
    return { mode: run.mode, windows: run.windows, text };
}

/**
 * 把 **PC** 与**虚拟时间**同步到顶栏运行条。
 *
 * ⚠️ **同一个状态源**：读的是 **M-10 的快照**（`registerMemory.getSnapshot()`）
 * ⇒ ⇒ 与寄存器视图那份**同一个源** ⇒ ⇒ **不是第二个真相**。
 * ⚠️ **不显示指令退休计数** —— 契约层的 `SimState` **没有**那个字段 ⇒ ⇒
 * 无数据来源 ⇒ ⇒ **不编造**（规格 §2 裁决 E）。
 *
 * ⚠️ **运行中显示固定符号 `-`**（STAGE-38 · v0.53 批 · 裁决 D / E）
 * ⇒ ⇒ 显示的那一份**隐藏**、原始 ps 属性**保留真实值**（那是真机门禁
 * **测量**虚拟时间增长的通道 ⇒ ⇒ 隐藏它就等于把门禁的尺子藏起来）。
 *
 * @param {{registerMemory?: object}} services
 * @returns {{pc: number, timeVirt: number, hidden: boolean}|null}
 */
export function syncTopbarStats(services) {
    const pcEl = document.querySelector("[data-topbar-pc-value]");
    const timeEl = document.querySelector("[data-topbar-time-value]");
    if (!pcEl && !timeEl) return null;
    const snap = services?.registerMemory?.getSnapshot?.() ?? null;
    const pc = Number(snap?.pc ?? 0) >>> 0;
    const timeVirt = Number(snap?.timeVirt ?? 0);
    const running = isRunning();
    if (pcEl) pcEl.textContent = liveValue(running, `0x${pc.toString(16).padStart(8, "0")}`);
    if (timeEl) {
        timeEl.textContent = liveValue(running, formatVirtualTime(timeVirt));
        // ⚠️ **同时挂一个原始 ps** ⇒ ⇒ 显示是**人类可读**的自适应单位，
        // 而「同一个状态源」那条判据**逐字可比**（视图里那份是 `123 ps`
        // 这种原始格式）⇒ ⇒ **可读性与判别力两者都要**。
        //
        // ⚠️ **运行中这个属性仍然是真实值**（裁决 E）⇒ ⇒ 那是**测量通道**，
        // 不是显示 ⇒ ⇒ 隐藏它等于把真机门禁的尺子藏起来。
        timeEl.setAttribute("data-topbar-time-ps", String(timeVirt));
    }
    return { pc, timeVirt, hidden: running };
}

/**
 * 顶栏运行条的**订阅退订句柄**（每次 `wireTopbarRunbar()` 先退旧的、再订新的）。
 * @type {(() => void)|null}
 */
let topbarStatsUnsubscribe = null;

/**
 * 顶栏那几个按钮的**点击监听器退订句柄**（v0.63 批）。
 *
 * ⚠️ **为什么也要退旧的**：统计订阅早就退了旧的（上面那个句柄）⇒ ⇒ 但**点击**
 *   监听器是直接 `addEventListener` 的 ⇒ ⇒ 接线两次 ⇒ ⇒ **同一个按钮上挂着两个
 *   监听器** ⇒ ⇒ 点一下**触发两次** ⇒ ⇒ 前端门禁里「点一次 ⇒ ⇒ 恰好一条命令」
 *   那类判据会被**重复触发**带偏 ⇒ ⇒ ��是**先退旧的、再挂新的** ⇒ ⇒ 恰好一个。
 *
 * ⚠️ **它不是「状态的第二份」**：退订句柄只存**函数引用** ⇒ ⇒ 状态仍然只有
 *   `debug.run` 那一份。
 *
 * @type {Array<() => void>}
 */
const topbarButtonUnsubscribes = [];

/**
 * 顶栏那条运行条**当前挂着的 services**（STAGE-38 · v0.53 批）。
 *
 * ⚠️ **为什么需要它**：运行状态一变（开始 / 停止）顶栏那一份活值就要
 * 跟着**隐藏 / 恢复**⇒ ⇒ 而 `setRunState()` 手上**没有** services ⇒ ⇒
 * 这里留一份**引用**（**不是**状态的第二份拷贝 —— 状态仍然只有 M-10 的
 * 快照那一份）⇒ ⇒ 顶栏与视图**同拍**。
 *
 * @type {object|null}
 */
let topbarServices = null;

/**
 * 接线**顶栏那条全局运行条**（STAGE-34 · v0.49 批 · 裁决 K）。
 *
 * ⚠️ **三个按钮直接调 `handleCommand`** ⇒ ⇒ 与调试视图那份**共用同一条
 * 命令处理路径** ⇒ ⇒ **不新增**命令处理分支 ⇒ ⇒ 一条路径。
 *
 * ⚠️ **状态订阅按 services 重新订**：每次 bootstrap 的 `services` 都是**新
 * 对象** ⇒ ⇒ 若用「只订一次」的标志 ⇒ ⇒ 新 services **收不到**更新 ⇒ ⇒
 * 顶栏数字会**冻住** ⇒ ⇒ 所以改成「先退订旧的、再订新的」⇒ ⇒ **恰好一个**
 * 订阅。
 *
 * @param {object} services
 */
export function wireTopbarRunbar(services) {
    const bar = document.querySelector("[data-topbar-runbar]");
    if (!bar) return;
    // ⚠️ **记住 services**（STAGE-38 · v0.53 批）⇒ ⇒ 运行状态一变就能
    // 立刻把顶栏那一份活值**隐藏 / 恢复**⇒ ⇒ 不等下一次 M-10 刷新。
    topbarServices = services ?? null;
    // ⚠️ **先退旧的点击监听器**（v0.63 批）⇒ ⇒ 接线两次**不会**让同一个按钮
    // 挂上两个监听器 ⇒ ⇒ 点一下**只**触发一次 ⇒ ⇒ 「恰好一条命令」那条判据
    // 才站得住。
    while (topbarButtonUnsubscribes.length > 0) {
        topbarButtonUnsubscribes.pop()?.();
    }
    const bind = (selector, onClick) => {
        const btn = bar.querySelector(selector);
        if (!btn) return;
        btn.addEventListener("click", onClick);
        topbarButtonUnsubscribes.push(() => btn.removeEventListener("click", onClick));
    };
    bind('[data-topbar-action="run"]', () => {
        void handleCommand({ action: "continue" }, services);
    });
    bind('[data-topbar-action="step"]', () => {
        void handleCommand({ action: "step" }, services);
    });
    bind('[data-topbar-action="step-over"]', () => {
        void handleCommand({ action: "step-over" }, services);
    });
    bind('[data-topbar-action="reset"]', () => {
        void handleCommand({ action: "reset" }, services);
    });
    // ⚠️ **「暂停」按钮**（STAGE-37 · v0.52 批 · 裁决 G）⇒ ⇒ 与调试视图
    // 那份**共用同一个 action** ⇒ ⇒ **一条路径** ⇒ ⇒ 不写第二套。
    //
    // ⚠️ **v0.63 批**：调试视图里那份**撤掉**了 ⇒ ⇒ 暂停的**唯一**入口是这里
    //   ⇒ ⇒ 界面**不再有**同一个动作的第二个入口。
    bind('[data-topbar-action="pause"]', () => {
        void handleCommand({ action: "pause" }, services);
    });
    if (topbarStatsUnsubscribe) {
        topbarStatsUnsubscribe();
        topbarStatsUnsubscribe = null;
    }
    topbarStatsUnsubscribe =
        services?.registerMemory?.subscribe?.(() => syncTopbarStats(services)) ?? null;
    syncTopbarStats(services);
    // ⚠️ **状态位也要在接线时同步一次** ⇒ ⇒ 页面刚打开就该显示「已停止」
    // ⇒ ⇒ 不是等用户第一次点「运行」才有内容。
    syncTopbarRunStatus();
}


/**
 * 板档案服务（M-23）的**懒加载**句柄。
 *
 * ⚠️ **为什么是动态 import 而不是静态 import**：板档案服务是**另一条
 * lane** 的产出（规格 §3.3 冻结了 API 形状）⇒ 前端**不因为它缺席就
 * 白屏**（D-01 / D-02 的教训：缺一个模块 ⇒ 整个页面空白，而且页面上看不出
 * 任何异常）⇒ 缺席时**显式**降级为「板档案不可用」，**不静默假装成功**。
 *
 * @type {Promise<{api: object|null, error: Error|null}>|null}
 */
let boardStoreModulePromise = null;

/**
 * 取板档案服务模块（**只加载一次**）。
 *
 * @returns {Promise<{api: object|null, error: Error|null}>}
 */
export function loadBoardStoreModule() {
    if (!boardStoreModulePromise) {
        boardStoreModulePromise = import("./services/m23_boardstore.js")
            .then((api) => ({ api, error: null }))
            .catch((error) => ({ api: null, error }));
    }
    return boardStoreModulePromise;
}

/**
 * 建板档案的**存储实例**（持久化层）。
 *
 * ⚠️ **不支持就明确说**（裁决 B / C-10）：`{supported: false}` ⇒ 状态
 * `unsupported` ⇒ 界面明说「板档案只存在于本会话」⇒ **不静默假装成功**。
 *
 * @returns {Promise<{store: object|null, status: string, reason: string|null}>}
 */
async function createBoardStoreInstance() {
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        return {
            store: null,
            status: "error",
            reason: `板档案服务不可用：${error?.message ?? error}`,
        };
    }
    const backend = api.createIndexedDbBackend();
    if (!backend || backend.supported !== true) {
        return {
            store: null,
            status: "unsupported",
            reason: String(backend?.reason ?? "本浏览器没有可用的持久化存储"),
        };
    }
    return { store: api.createBoardStore(backend), status: "ok", reason: null };
}

/**
 * 删除产品视图「档案管理」里的一条板档案。
 *
 * ⚠️ 只动本机档案，不改当前产品、不改内核状态；删除后当前产品仍可继续用。
 *
 * @param {object} services
 * @param {string} id
 */
async function deleteProductArchive(services, id) {
    const state = viewStore.getState("product") ?? {};
    const archive = Array.isArray(state.archive) ? state.archive : [];
    const target = String(id ?? "");
    if (!archive.some((row) => String(row?.id) === target)) {
        setNote(`板档案里没有 id「${target}」这一块`);
        return;
    }
    const store = services?.boardStore ?? null;
    if (store) {
        try {
            await store.remove(target);
        } catch (err) {
            setNote(`板档案删除失败：${err?.message ?? err}`);
            return;
        }
    }
    patchProductBoard({ archive: archive.filter((row) => String(row?.id) !== target) });
    setNote(store
        ? `已删除板档案「${target}」`
        : `已删除板档案「${target}」（板档案只存在于本会话）`);
}

/**
 * 「加载固件」这条命令的**唯一**处理函数（STAGE-33 · v0.48 批）。
 *
 * ⚠️ **为什么抽出来**：曾经有**两个**入口——产品视图里那块面板（视图层，
 * 走 `riscv:command` 信号）与**页头**那个入口（app 层）⇒ ⇒ 两者必须
 * **共用这一个函数**⇒ ⇒ **一条装载路径**（`loadProjectIntoKernel`）⇒ ⇒
 * 重订流与清残留**只有一处**（规格 §3 裁决 G）。
 *
 * ⚠️ **v0.58 批**：入口**收敛成一个**（页头那个）⇒ ⇒ 这个函数是**唯一**
 * 的装载实现（脚本口 `loadFromUrl` 也走它）⇒ ⇒ **一条装载路径**这条规矩
 * **更严**了（不是两条入口凑成一条，而是**只剩一条**）。
 *
 * ⚠️ **与脚本口也共用**：`window.riscv_image.loadFromUrl` 最终走的也是
 * `loadImageFromBytes` ⇒ ⇒ **不写第二套**。
 *
 * ⚠️ **装完就停**（STAGE-32 裁决 D）⇒ ⇒ 这里⛔ **不**发 `ContinueRun`
 * ⇒ ⇒ 用户自己点「继续」。
 *
 * @param {{params?: {bytes?: number[], name?: string, kind?: string}}} detail
 * @param {object} services
 * @returns {Promise<void>}
 */
export async function handleLoadImageCommand(detail, services) {
    const bytes = detail?.params?.bytes;
    if (!Array.isArray(bytes) || bytes.length === 0) {
        setImageLoadState({
            status: "error",
            error: "没有镜像字节 —— 请先用文件选择器选一个固件",
            pending: null,
        });
        publishProductState();
        setNote("加载固件失败：没有镜像字节");
        return;
    }
    // ⚠️ **先写「装载中」再发命令**（v0.58 批 · 幂等的关键）：
    // 这一段是**同步**的（async 函数体在第一个 `await` 之前同步执行）⇒ ⇒
    // 连点两下时，第二下看到的 `status` 已经是 `loading` ⇒ ⇒ `imageLoadDecision`
    // 判成「忽略」⇒ ⇒ **不会**发第二条 `LoadProject`（内核被连着换两次工程）。
    setImageLoadState({ status: "loading", error: null });
    publishProductState();
    try {
        const result = await loadImageFromBytes(services, bytes, {
            name: detail?.params?.name,
            kind: detail?.params?.kind,
        });
        // ⚠️ **产品树**也要换成新工程的（否则还显示上一份工程的树）。
        viewStore.setState("product", (prev) => ({
            ...prev,
            tree: result.project.tree,
        }));
        setImageLoadState({
            status: "ok",
            name: detail?.params?.name ?? "image.bin",
            byteLength: result.byteLength,
            error: null,
            // ⚠️ **装完就把 pending 清掉** ⇒ ⇒ 那份字节**不会**被又发一遍。
            // ⚠️ **但「再点一次」不再报失败**（v0.58 批）：`pending` 没有了
            // ⇒ ⇒ 判定落到「已装载」那一格 ⇒ ⇒ 如实说「已装载，无需重复
            // 加载」⇒ ⇒ ⛔ **不再**报「装载失败：没有待加载的文件」。
            pending: null,
        });
        publishProductState();
        const streamErrors = [
            result.probeError ? `探针流 ${result.probeError.message}` : null,
            result.consoleError ? `console 流 ${result.consoleError.message}` : null,
            result.memoryMapError ? `内存图流 ${result.memoryMapError.message}` : null,
        ].filter(Boolean);
        setNote(streamErrors.length > 0
            ? `已加载固件「${detail?.params?.name ?? "image.bin"}」，但流订阅不可用：${streamErrors.join("；")}`
            : `已加载固件「${detail?.params?.name ?? "image.bin"}」（${result.byteLength} 字节）—— 点「继续」开始运行`);
    } catch (err) {
        // ⚠️ **失败显示内核原文**（STAGE-32 裁决 J）⇒ ⇒ 不吞。
        //
        // ⚠️ **失败时 pending 保留**（v0.58 批）⇒ ⇒ 用户点一下就能**重试**
        // 同一份文件 ⇒ ⇒ 而不是「再点一次 ⇒ 没有待加载的文件」。
        //
        // ⚠️ **⛔ 这里刻意**不写** `pending: null`：`setImageLoadState` 是
        // **合并**语义 ⇒ ⇒ 不写 ⇒ ⇒ 那份 `pending` **原样留着**。
        setImageLoadState({
            status: "error",
            error: err?.message ?? String(err),
        });
        publishProductState();
        setNote(`加载固件失败：${err?.message ?? err}`);
    }
}

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
                // 断点（STAGE-35 · v0.50 批 · 裁决 J）。
                //
                // ⚠️ **id = addr** ⇒ ⇒ 「一址一断点」这条**内核语义**
                // （M-08 的第二条规则）在 id 上就成立 ⇒ ⇒ 前端**不需要**
                // 第二个计数器状态 ⇒ ⇒ **状态只有一份**。
                //
                // ⚠️ **契约层早有这两条命令**（`SetBreakpoint` /
                // `ClearBreakpoint`）⇒ ⇒ **零新命令 / 零新 DTO / 零新事件**
                // ⇒ ⇒ 内核行为**一字不变**（脚本口走的就是同一条路）。
                //
                // ⚠️ **条件（契约 v9 · 裁决 O）**：条件框对**地址断点**与
                // **符号断点**通用 ⇒ ⇒ 这里**显式**带上 ⇒ ⇒ **空 ⇒
                // `null`**（C-9，不是空串）⇒ ⇒ 无条件断点的语义**一字不变**。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                const condition = normalizeCondition(detail.params?.condition);
                // ⚠️ **内核错误落在「条件」那一位** ⇒ ⇒ 先记目标位、再发命令。
                setPendingErrorSlot("cond");
                writeErrorSlot("cond", "");
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: null,
                    enabled: true,
                    condition,
                }));
                // ⚠️ **成功之后才落界面列表**：内核可能因地址未对齐 / 地址
                //   重复 / 条件非法拒绝这条命令 ⇒ ⇒ 先落列表会留下一个内核
                //   并不存在的断点（界面在骗人）。
                requestBreakpoint(addr);
                break;
            }
            case "set-symbol-breakpoint": {
                // 从**符号候选**下断点（契约 v9 · 裁决 N）。
                //
                // ⚠️ **id 仍然是地址**（既有「断点 id = 地址」这条内核语义
                //   **不变**）⇒ ⇒ 前端**不需要**第二个计数器状态。
                // ⚠️ **`source_hint` = 候选符号名** ⇒ ⇒ 列表里能显示它
                //   （C-10）⇒ ⇒ 名字**不是**另存的一份状态 ⇒ ⇒ 它随命令走。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                const symbol = detail.params?.symbol ? String(detail.params.symbol) : null;
                const condition = normalizeCondition(detail.params?.condition);
                setPendingErrorSlot("cond");
                writeErrorSlot("cond", "");
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: symbol,
                    enabled: true,
                    condition,
                }));
                // ⚠️ **成功之后才落列表 / 贴标签**（同上：失败不留假断点）。
                requestBreakpoint(addr);
                if (symbol) labelBreakpointSymbol(addr, symbol);
                break;
            }
            case "lookup-symbol": {
                // 符号查询（契约 v9 · `ResolveSymbol`）。
                //
                // ⚠️ **一次输入 = 一次查询**（裁决 M）⇒ ⇒ 查询按**显式动作**
                //   触发 ⇒ ⇒ 没有「边打字边查」的乱序问题。
                // ⚠️ **空名字也照发** ⇒ ⇒ 内核拒（`E_DBG-02`）⇒ ⇒ 前端
                //   显示**内核原文**（前端**不**写第二套校验器）。
                const name = String(detail.params?.name ?? "");
                setPendingErrorSlot("symbol");
                writeErrorSlot("symbol", "");
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
                // 撤掉断点 ⇒ ⇒ id 同样**就是地址**（裁决 J）⇒ ⇒ 前端列表
                // 只存地址就够 ⇒ ⇒ `ClearBreakpoint { id }` 传的是地址。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                await commandRouter.dispatchCommand(Command.clearBreakpoint(addr));
                // ⚠️ **清除成功之后才撤界面项**：传输失败 / 命令失败时先删
                //   会留下一个「内核还在、界面没了」的反向假状态。
                withdrawBreakpoint(addr);
                break;
            }
            case "list-breakpoints": {
                // 契约 v13：直接把内核的断点表取回来（回执照
                // `debug.breakpoints` 订阅落进视图状态）。
                await commandRouter.dispatchCommand(Command.listBreakpoints());
                break;
            }
            case "toggle-breakpoint": {
                // 契约 v13：enable / disable。id 就是地址；当前状态从视图
                // 状态读，命令成功后再翻本地那一格（回执也会整表覆盖）。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                const current = viewStore.getState("debug")?.debug?.bpEnabled?.[String(addr)] !== false;
                const next = !current;
                await commandRouter.dispatchCommand(Command.setBreakpointEnabled(addr, next));
                updateDebugState((prev) => ({
                    ...prev,
                    debug: {
                        ...(prev.debug ?? {}),
                        bpEnabled: { ...(prev.debug?.bpEnabled ?? {}), [String(addr)]: next },
                    },
                }));
                break;
            }
            case "set-watch-expr": {
                // 契约 v14：新增 / 覆盖观察表达式。id 用视图里的递增计数，
                // 保证同一项可以重复「覆盖」而不是无限追加。
                const expr = String(detail.params?.expr ?? "").trim();
                if (!expr) break;
                const id = toInt(viewStore.getState("debug")?.debug?.watchNextId, 1) >>> 0;
                await commandRouter.dispatchCommand(Command.setWatchExpr({ id, expr }));
                updateDebugState((prev) => ({
                    ...prev,
                    debug: {
                        ...(prev.debug ?? {}),
                        watchNextId: (id + 1) >>> 0,
                        watchExpr: "",
                    },
                }));
                break;
            }
            case "clear-watch-expr": {
                const id = toInt(detail.params?.id, 0) >>> 0;
                await commandRouter.dispatchCommand(Command.clearWatchExpr(id));
                break;
            }
            case "list-watch-exprs": {
                await commandRouter.dispatchCommand(Command.listWatchExprs());
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
/**
 * 产品视图这一侧的板档案状态（住「product」视图状态，与板级内存图视图
 * 那套 `memory-map` 的字段**各住一处**，互不混淆）。
 *
 * @param {object} patch
 */
function patchProductBoard(patch) {
    viewStore.setState("product", (prev) => ({ ...prev, ...patch }));
    publishViewState("product");
}

/**
 * 重新读一次板档案列表（「从模版加载」弹层的数据源）。
 *
 * ⚠️ 读失败**不静默**：状态写 `error` 并显示原因（板档案只是**模版来源**，
 * 读不出来不影响已经装上的产品）。
 *
 * @param {{boardStore?: object|null}} services
 */
async function refreshProductArchive(services) {
    const store = services?.boardStore ?? null;
    if (!store || store.supported === false) {
        patchProductBoard({ archive: [], archiveStatus: "unsupported" });
        return;
    }
    try {
        const entries = await store.list();
        patchProductBoard({
            archive: Array.isArray(entries) ? entries : [],
            archiveStatus: "ok",
        });
    } catch (err) {
        patchProductBoard({ archiveStatus: "error" });
        setNote(`板档案读取失败：${err?.message ?? err}`);
    }
}

/**
 * 把一份「内核接受过的描述 + 寄存器文档」写进**本机数据库**（板档案）。
 *
 * ⚠️ **落库门槛**（沿用规格裁决 B）：只有内核接受过的描述才允许落库 ⇒
 * 调用点都在 `SetMemoryMap` 成功之后；本函数**不**自己判描述合法性。
 *
 * @param {{boardStore?: object|null}} services
 * @param {object} description — 内核接受过的描述
 * @param {object} registers — 按器件身份索引的寄存器文档（可为 `{}`）
 * @returns {Promise<{saved: boolean, id: string|null, reason: string|null}>}
 */
async function writeProductArchive(services, description, registers) {
    const store = services?.boardStore ?? null;
    if (!store || store.supported === false) {
        return { saved: false, id: null, reason: "unsupported" };
    }
    const { api } = await loadBoardStoreModule();
    if (!api) return { saved: false, id: null, reason: "module" };
    const entry = api.boardEntry(description, registers);
    if (!entry) return { saved: false, id: null, reason: "no-id" };
    try {
        await store.put(entry);
    } catch (err) {
        setNote(`板档案写入失败：${err?.message ?? err}`);
        return { saved: false, id: entry.id, reason: "write" };
    }
    await refreshProductArchive(services);
    return { saved: true, id: entry.id, reason: null };
}

/**
 * 把一份板描述**装进内核**（产品视图的「创建 / 从模版加载 / 重新载入」共用）。
 *
 * 三步绑在一个函数里 ⇒ 调用点不会漏掉后两步：
 * 1. `SetMemoryMap` / `PatchMemoryMap` —— 路由仍由 `planMemoryMapUpdate` 判；
 * 2. `LoadProject` —— 宿主在这次装载时应用**待生效**的板；
 * 3. 视图状态扶正 —— 树、板草稿、寄存器基线与「已装载」标志。
 *
 * ⚠️ **基址真提交内核，偏移 / 含义是前端文档层**（`DeviceSpec` 没有寄存器
 * 偏移字段）⇒ 第 3 步把 `docs` 扶正成新基线。
 *
 * @param {object} services
 * @param {object} rawDescription — 草稿形状的描述（数值可能是字符串）
 * @param {{fresh?: boolean, registers?: object}} [options]
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>}
 */
export async function applyProductBoard(services, rawDescription, options = {}) {
    const {
        commandRouter,
        registerMemory,
        waveform,
        debug,
        console: consoleBridge,
        memoryMap: memoryMapBridge,
    } = services;
    const description = draftToDescription(rawDescription ?? {});
    const product = viewStore.getState("product") ?? {};
    const base = product.memoryMap?.description ?? null;
    // ⚠️ **期望修订号**：从没收到过 `memory.map` 事件 ⇒ `null`（不检查）。
    const expectedRevision = product.memoryMap
        ? Number(product.memoryMap.revision ?? 0)
        : null;
    const plan = planMemoryMapUpdate(base, description, {
        pending: product.memoryMap?.pending === true,
    });
    if (plan.mode !== "none") {
        try {
            if (plan.mode === "replace") {
                await commandRouter.dispatchCommand(
                    Command.setMemoryMap(description, expectedRevision),
                );
            } else {
                await commandRouter.dispatchCommand(
                    Command.patchMemoryMap(plan.ops, expectedRevision),
                );
            }

            // ⚠️ 只有**内核接受过**的描述才算「被接受过」（板档案落库门槛）。
            // ⚠️ 接受成功 ⇒ **清掉**上一次的横幅（否则旧错会一直挂着）。
            viewStore.setState("product", (prev) => ({
                ...prev,
                acceptedDescription: description,
                error: null,
            }));
        } catch (err) {
            const { code, message } = splitErrorCode(err);
            // ⚠️ **v0.77（STAGE-53 · 表 11）**：校验失败要在**工作面横幅**上
            //    说出来（用户当时的实感是「弹层关了，什么都没发生」）。
            const hint = boardErrorHint(code);
            const detail = hint ? `${hint}${message ? `（${message}）` : ""}` : message;
            viewStore.setState("product", (prev) => ({
                ...prev,
                error: { code, message: detail || code },
            }));
            setNote(`内存图流不可用：${code} ${message}`);
            throw err;
        }
    }
    // 树跟着描述走：核节点固定，外设节点 = 每段外设的器件 id。
    const tree = projectTreeFrom(description);
    viewStore.setState("product", (prev) => ({ ...prev, tree }));
    // ⚠️ 从**档案**装载时把寄存器文档一起搬回视图状态（按器件身份 → 当前
    // 节点 key）：档案里存了偏移 / 含义 ⇒ 装出来的东西必须和保存时一致。
    const docs = options.fresh === true
        ? docsForKeyed(options.registers ?? {}, description, tree)
        : undefined;
    adoptProductDescription(description, { resetDocs: options.fresh === true, docs });
    // ⚠️ `images: []` 是**如实**的：产品视图这一批还没有固件装载面。
    const project = {
        id: `proj-${description.id || "board"}`,
        name: description.name || "产品",
        tree,
        images: [],
        version: { schema: 1, isa: "RV32IMC", toolchain: "front (product view)" },
    };
    const result = await loadProjectIntoKernel(
        {
            commandRouter,
            registerMemory,
            waveform,
            debug,
            console: consoleBridge,
            memoryMap: memoryMapBridge,
            devices: services.devices,
            templates: services.templates,
        },
        project,
        // ⚠️ **v0.88**：这是「让板生效」的内部空装载 ⇒ 保留「用户当前工程」
        //   （含固件）⇒ 「重新装载工程」才真的重装固件（方案 B）。
        { remember: false },
    );
    markProductApplied();
    // ⚠️ **内核接受过 ⇒ 顺手写进本机数据库**（板档案）：重新载入、创建、
    // 从模版加载三条通路都走这里 ⇒ 库里的板永远等于「最近一次真装上的那份」
    // （带偏移 / 含义）。写库失败只提示、不回滚装载。
    await writeProductArchive(
        services,
        description,
        docsByDevice(viewStore.getState("product") ?? {}),
    );
    return result;
}

async function startSession(services) {
    const { session, commandRouter, registerMemory, waveform, debug, workspace } = services;
    try {
        await session.createSession({ role: workspace.role });
        // ⚠️ **产品视图默认是空产品**（v0.77 双栏工作面的口径）：启动时
        // 内核里那块 default 板 / mock 工程**不算用户的产品** ⇒ ⇒ 产品树
        // 不显示它（否则「默认不加载任何板子」这条就落不了地）。
        //
        // ⚠️ **只动 `product` 视图状态**：板级内存图视图（`memory-map`）
        // 照旧吃内核推来的 `memory.map`，两个视图各管各的。
        viewStore.setState("product", (prev) => ({
            ...prev,
            loaded: false,
            locked: true,
            selected: null,
            board: null,
            tree: null,
            docs: {},
            docsBase: {},
        }));
        publishViewState("product");
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
                // ⚠️ **器件中间层**（STAGE-43 · v0.66）⇒ ⇒ **必须**在这儿
                // 一起交进去 ⇒ ⇒ 否则 `resubscribeAllStreams` **看不到**它
                // ⇒ ⇒ 那两条流**永远不订阅** ⇒ ⇒ 产品视图那两张卡**永远是
                // 空态**（⚠️ 这正是真机门禁第一次跑就抓到的那个坑）。
                devices: services.devices,
                // ⚠️ **模板清单流**（v0.76 · STAGE-52）—— 同形：装载 / 卸载
                // 清掉内核主题集合 ⇒ `resubscribeAllStreams` 看不到它就不重订
                // ⇒ 模板库清单**永远停在空态**（r12/r13 真机：30s 等不到）。
                templates: services.templates,
            },
            project,
        );
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
 * 装载工程，并把**内核侧订阅**都强制重订。
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
 * 4. `resubscribeAllStreams()` —— 强制重订其余的**流**订阅。同样的道理：
 *    `LoadProject` 把内核的主题集合清空了，探针流 / console 流 / 内存图流
 *    不重订就**静默停掉**。
 *
 * ⚠️ **第 4 步是助手**（v0.31 · 裁决 F）⇒ **装载**与**卸载**共用同一套
 * ⇒ 卸载路径不会漏掉任何一条流。
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
export async function loadProjectIntoKernel(services, project, options = {}) {
    const { registerMemory, waveform, debug, console: consoleBridge } = services;
    if (!consoleBridge) {
        // ⚠️ **不静默**：console 接线漏了 ⇒ 终端永远是空的，而页面上看不出
        // 任何异常 ⇒ 这是最难查的一类故障。宁可当场抛错。
        throw new Error(
            "loadProjectIntoKernel 缺少 console 接线：console 流必须被强制重订" +
            "（LoadProject 清掉了内核的主题集合）",
        );
    }
    // ⚠️ **记下当前工程** ⇒ 「重新装载工程」按钮能再发一次同一个工程
    // （板级配置是**待生效**的 ⇒ 宿主在下一次 `LoadProject` 时才应用）。
    //
    // ⚠️ **v0.88 · `remember: false`**（用户拍板方案 B）：产品视图「配板」
    //   用的是一次 `images: []` 的空装载 —— 那是**为了让板生效**的内部装载，
    //   **不是**用户装了新程序 ⇒ ⇒ 它**不许**把「用户当前工程」的记忆覆盖成
    //   一份空工程。
    //
    //   ⚠️ 覆盖的后果（真机 + 门禁双重现场）：配完板再点「重新装载工程」⇒
    //   重发的是一份 `images: []` ⇒ **既不会重装固件、也不报任何错**
    //   ⇒ 按钮形同虚设；同时「板已应用 + 现有固件」这条装载路径被抹掉
    //   ⇒ ISA 门禁（`E_PLT-20/21`）在重装这一步**永远走不到**。
    if (options.remember !== false) setLoadedProject(project);
    // ⚠️ **新工程 = 新程序 ⇒ 运行态归零**（v0.63 批）⇒ ⇒ 顶栏那个按钮回到
    //   「运行」（用户拍板的那条语义：「**当加载之后顶栏的运行应该显示运行**」）
    //   ⇒ ⇒ 「跑过没有」这一格也归零 ⇒ ⇒ 不是上一段程序的历史。
    setRunState({ ...initialRunState(), started: false });
    waveform.reset();
    // ⚠️ 新工程 = 新程序 ⇒ **上一个程序的反汇编**留在面板上就是错的
    // （同一个 D-45 的坑：状态必须跟着工程一起重建）。
    // `debug` 是可选的——不是每个调用点都装了调试面。
    debug?.reset?.();
    // ⚠️ 新程序 ⇒ 上一个程序的 console 字节不算数（同一个坑）。
    consoleBridge.reset();
    // ⚠️ 器件实例状态**跟着工程重建**（STAGE-43 · v0.66 · 同一个坑）⇒ ⇒
    // 上一块板的实例清单不算数。
    services.devices?.reset?.();
    // ⚠️ 模板清单**跟着工程重建**（v0.76 · STAGE-52 · 同一个坑）⇒ ⇒
    // 上一会话的清单不算数。
    services.templates?.reset?.();
    // ⚠️ **D-178（v0.72 批）**：装载**失败**时**也要重订一次流** ⇒ ⇒
    // 否则前端**学不到**内核顶上去的修订号。
    //
    // ⚠️ **根因链**：内核在「挑平台描述」那一步就会把**待生效的板应用**
    // （修订号 **+1**）⇒ ⇒ 那之后才失败（写不进总线 / 符号索引重建失败）
    // ⇒ ⇒ 内核**已经**是新修订号 ⇒ ⇒ 而**失败路径不回 `memory.map`** ⇒ ⇒
    // 前端**唯一**能学到新修订号的途径是「重订 ⇒ 内核强推一次」⇒ ⇒
    // 不重订 ⇒ ⇒ 前端停在旧修订号 ⇒ ⇒ **下一次改板报 `E_PLT-13`**
    // ⇒ ⇒ 板换不回去、零提示。
    //
    // ⚠️ **失败也要重订**（不是「成功才重订」）⇒ ⇒ **重订在 catch 里**也
    // 跑一遍 ⇒ ⇒ 原错误**原样抛出**（不改调用点现有的错误语义）⇒ ⇒
    // 「装载失败」这件事**照旧**被看见。
    //
    // ⚠️ **重订**失败**不掩盖**装载错误 ⇒ ⇒ 各流**各自**兜住自己的错误
    // （`resubscribeAllStreams` 内部就是那样做的）⇒ ⇒ 这里**不**追加。
    let loadError = null;
    try {
        await registerMemory.loadProject(project);
    } catch (err) {
        loadError = err;
    }
    // ⚠️ **重订部分抽成助手**（裁决 F · v0.31）⇒ 装载与卸载**共用**同一套
    // ⇒ **不写第二套**。
    //
    // ⚠️ `skipRegisters` = 真：M-10 的 `loadProject()` **自己**就强制重订
    // 了寄存器流（第 3 步）⇒ 这里不再重复发一次。
    const errors = await resubscribeAllStreams(services, { skipRegisters: true });
    // ⚠️ **原错误原样抛出** ⇒ ⇒ 调用点的错误语义**一字未变**。
    if (loadError) {
        throw loadError;
    }
    // ⚠️ **装载成功 ⇒ 上一会话的最后一条错误不再代表当前状态**。
    //
    // 空产品启动时会先 `LoadProject` 撞一次 `E_PLT-22`（无生效板，这是
    // v0.76 的预期拒绝）⇒ 全局 `error` 订阅把那条写进了 `debug.error`
    // ⇒ 之后从模版装板成功、甚至 `debug.reset()` 也按「保留内核最后一份
    // 报告」的规矩不清它 ⇒ 调试视图会一直挂「没有生效板」的假状态。
    // 错误位只在**这一次装载失败**时由 `loadError` 抛出并保留；成功
    // 就把它收掉。
    viewStore.setState("debug", (prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), error: null },
    }));
    publishViewState("debug");
    return {
        probeError: errors.probeError,
        consoleError: errors.consoleError,
        memoryMapError: errors.memoryMapError,
        // ⚠️ 器件中间层（STAGE-43 · v0.66）—— 与其它流**同形**地报出来。
        deviceError: errors.deviceError,
        // ⚠️ 模板清单流（v0.76 · STAGE-52）—— 同形报出来。
        templateError: errors.templateError,
    };
}

/**
 * 强制重订**全部流**（STAGE-16 · v0.31 · 裁决 F）。
 *
 * ⚠️ **为什么必须是「强制」**：内核侧的订阅会随**会话**一起失效
 * （`LoadProject` 与 `UnloadProject` 都会清掉内核的主题集合 ⇒ 新会话
 * = 新订阅）⇒ 而前端的「已订阅」标志可能还立着 ⇒ 不重订就**静默停流**
 * （台账 D-45 / D-46 的同一个坑）。
 *
 * ⚠️ **要重订的四条流**（前端的**事件流**）：
 *   - 状态增量流 `sim.state.delta`（M-10 的寄存器/PC 生产载体；
 *     `sim.registers` 的波形流只是兼容输入，不再订阅）；
 *   - 探针波形流 `waveform.history`（内核侧主题是 `sim.waveform`）；
 *   - console 流 `sim.console`；
 *   - 内存图流 `memory.map`。
 *
 * ⚠️ **每一条各自兜住**：一条流订阅不上**不该**牵连其它流 ⇒ 失败作为
 * `registerError` / `probeError` / `consoleError` / `memoryMapError`
 * 返回 ⇒ **不静默**：调用点负责把它显示出来。
 *
 * ⚠️ **装载与卸载共用这一套**（裁决 F）⇒ 卸载之后不会漏掉任何一条流。
 *
 * @param {{registerMemory?: object, waveform: object, console?: object, memoryMap?: object}} services
 * @param {{skipRegisters?: boolean}} [options] — `skipRegisters` = 真时**不**
 *   重订寄存器流（调用点自己已经重订过了）
 * @returns {Promise<{registerError: Error|null, probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>}
 */
export async function resubscribeAllStreams(services, options = {}) {
    const { skipRegisters = false } = options;
    /** @type {{registerError: Error|null, probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, deviceError: Error|null}} */
    const errors = {
        registerError: null,
        probeError: null,
        consoleError: null,
        memoryMapError: null,
        // ⚠️ 器件中间层的两条流（STAGE-43 · v0.66）⇒ ⇒ **各自兜住**（不牵连别的流）。
        deviceError: null,
        // ⚠️ 模板清单流（v0.76 · STAGE-52 · 裁决 M）—— 同形兜住。
        templateError: null,
    };
    if (!skipRegisters) {
        try {
            // ⚠️ **寄存器流 + 状态增量流**都归 M-10 ⇒ 它的 `resubscribe()`
            // 是这两条流的**唯一**重订入口。
            await services.registerMemory?.resubscribe?.();
        } catch (err) {
            errors.registerError = err;
        }
    }
    try {
        await services.waveform.subscribeProbeStream();
    } catch (err) {
        errors.probeError = err;
    }
    try {
        await services.console.subscribeConsoleStream();
    } catch (err) {
        errors.consoleError = err;
    }
    try {
        await services.memoryMap?.subscribeMemoryMapStream?.();
    } catch (err) {
        errors.memoryMapError = err;
    }
    // ⚠️ **器件中间层**（STAGE-43 · v0.66）—— 与内存图流**同形**：装载 /
    // 卸载都会清掉内核的主题集合 ⇒ ⇒ 不重订就**静默**停在旧数据上。
    try {
        await services.devices?.subscribeDeviceStreams?.();
    } catch (err) {
        errors.deviceError = err;
    }
    // ⚠️ 模板清单流（v0.76 · STAGE-52）—— 装载 / 卸载清掉内核主题集合 ⇒
    // 不重订就**静默**停在旧清单上（与 device.types 同一个坑）。
    try {
        await services.templates?.subscribeTemplateStreams?.();
    } catch (err) {
        errors.templateError = err;
    }
    return errors;
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
 * `sim.state.delta`.
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void}}
 */
/**
 * 断点列表的**唯一落点**（STAGE-35 · v0.50 批 · 裁决 K）。
 *
 * ⚠️ **视图层只读 + 发命令** ⇒ ⇒ 它**不碰**服务层、**不持有**第二份列表
 * ⇒ ⇒ 「一个约束一个落点」。
 *
 * ⚠️ **元素是地址数组**（不是 `{id, addr}`）⇒ ⇒ 因为**断点 id 就等于地址**
 * （裁决 J）⇒ ⇒ id **可推导** ⇒ ⇒ **不需要第二个计数器状态**。
 *
 * ⚠️ **它是「本会话由界面请求过的断点」**，**不是**内核断点集的镜像
 * （裁决 O）—— 内核**没有**「列出断点」的事件（`DebugMeta` 是死 DTO）
 * ⇒ ⇒ 所以界面上**如实标注**这一点。
 *
 * @param {(prev: number[]) => number[]} mutator
 * @returns {number[]} 写进去的列表
 */
function updateBreakpoints(mutator) {
    const next = updateDebugState((prev) => {
        const list = Array.isArray(prev?.debug?.breakpoints) ? prev.debug.breakpoints : [];
        return {
            ...prev,
            debug: { ...(prev.debug ?? {}), breakpoints: mutator(list) },
        };
    });
    return next?.breakpoints ?? [];
}

/**
 * 改**调试视图状态**并重渲染（契约 v9 · 符号标签的落点）。
 *
 * ⚠️ **「有哪些断点」只有一个落点**（`debug.breakpoints`，既有判据 A-7）
 *   ⇒ ⇒ 符号名**只是附注**（`debug.bpSymbols`：地址 → 名字）⇒ ⇒ 列表的
 *   **成员**仍由 `breakpoints` 决定 ⇒ ⇒ 不是第二个真相。
 *
 * @param {(prev: object) => object} mutator
 * @returns {object} 写进去的 `debug` 子树
 */
/** 空输入 ⇒ `undefined`；非空按十六进制解析，解析不了 ⇒ `NaN`（交给归一器报错）。 */
function parseOptionalFilterAddr(raw) {
    const text = String(raw ?? "").trim();
    if (!text) return undefined;
    const parsed = parseAddrInput(text);
    return parsed.ok ? parsed.value : Number.NaN;
}

function updateDebugState(mutator) {
    const next = viewStore.setState("debug", (prev) => mutator(prev));
    publishViewState("debug");
    return next?.debug ?? {};
}

/**
 * 记一个「界面请求过的断点」。
 *
 * ⚠️ **已经在列表里就不重复记** ⇒ ⇒ 内核会拒「该地址已存在」（M-08 的
 * 第二条规则）⇒ ⇒ 前端**不该**去触发那个错误。
 *
 * @param {number} addr
 * @returns {number[]} 写进去的列表
 */
export function requestBreakpoint(addr) {
    // 新加的断点默认启用（与 `Command.setBreakpoint` 的 `enabled: true` 同拍）。
    updateDebugState((prev) => ({
        ...prev,
        debug: {
            ...(prev.debug ?? {}),
            bpEnabled: { ...(prev.debug?.bpEnabled ?? {}), [String(addr)]: true },
        },
    }));
    return updateBreakpoints((list) => (
        list.includes(addr) ? list : [...list, addr]
    ));
}

/**
 * 撤掉一个「界面请求过的断点」。
 *
 * ⚠️ **符号标签一起撤掉** ⇒ ⇒ 列表里不会留下一个**已经没有断点**的名字
 *   ⇒ ⇒ 标签与列表**始终一致**。
 *
 * @param {number} addr
 * @returns {number[]} 写进去的列表
 */
export function withdrawBreakpoint(addr) {
    updateDebugState((prev) => {
        const symbols = { ...(prev?.debug?.bpSymbols ?? {}) };
        const enabled = { ...(prev?.debug?.bpEnabled ?? {}) };
        const hits = { ...(prev?.debug?.bpHits ?? {}) };
        const key = String(Number(addr ?? 0) >>> 0);
        delete symbols[key];
        delete enabled[key];
        delete hits[key];
        return {
            ...prev,
            debug: { ...(prev.debug ?? {}), bpSymbols: symbols, bpEnabled: enabled, bpHits: hits },
        };
    });
    return updateBreakpoints((list) => list.filter((a) => a !== addr));
}

/**
 * 给一个断点**贴符号名标签**（契约 v9 §5.3 · C-10）。
 *
 * ⚠️ **标签是附注** ⇒ ⇒ 「有哪些断点」仍由 `debug.breakpoints` 决定
 *   ⇒ ⇒ 既有判据（断点列表是**地址数组**）**一个字不用改**。
 *
 * @param {number} addr
 * @param {string} symbol
 */
export function labelBreakpointSymbol(addr, symbol) {
    updateDebugState((prev) => ({
        ...prev,
        debug: {
            ...(prev.debug ?? {}),
            bpSymbols: {
                ...(prev.debug?.bpSymbols ?? {}),
                [String(Number(addr ?? 0) >>> 0)]: String(symbol),
            },
        },
    }));
}

/**
 * 条件表达式的**线上形态**（契约 v9 · C-9）。
 *
 * ⚠️ **空 ⇒ `null`**（**不是**空串）⇒ ⇒ 「无条件断点」在线上是
 *   `condition: null` ⇒ ⇒ 内核那边就是 `Option::None` ⇒ ⇒ 既有断点语义
 *   一字不变。
 *
 * ⚠️ **非空 ⇒ 原样发出去** ⇒ ⇒ 前端**不解析**条件表达式（那是内核的
 *   判据 ⇒ ⇒ `E_DBG-02`）⇒ ⇒ 前端只显示它的原文。
 *
 * @param {unknown} raw
 * @returns {string|null}
 */
export function normalizeCondition(raw) {
    if (raw === null || raw === undefined) return null;
    const text = String(raw);
    return text === "" ? null : text;
}


/**
 * 内存读 / 转储失败 → 顶部 toast 正文。
 *
 * ⚠️ 写的是内核原文（`code` + `message`）⇒ ⇒ 前端不吞、不改写错误原因。
 *
 * @param {unknown} err
 * @returns {string}
 */
function memoryToastText(err) {
    const { code, message } = splitErrorCode(err);
    return code ? `${code}：${message || code}` : String(err?.message ?? err);
}

/**
 * 把**内核原文**写进**某一位**（契约 v9 · C-13）。
 *
 * ⚠️ **写的是内核给的原文**（`event.value.message`）⇒ ⇒ **不吞、不改写**
 *   ⇒ ⇒ 真机断言能逐字看到内核说了什么。
 *
 * ⚠️ **空串 = 清掉那一位** ⇒ ⇒ 重试之前先清 ⇒ ⇒ 旧提示不挂着吓人。
 *
 * @param {"cond"|"symbol"|"dump"|null} slot
 * @param {string} message
 */
function writeErrorSlot(slot, message) {
    const text = String(message ?? "");
    if (slot === "cond") {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), condError: text },
        }));
        publishViewState("debug");
    } else if (slot === "symbol") {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), symbolError: text },
        }));
        publishViewState("debug");
    } else if (slot === "dump") {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), dumpError: text },
        }));
        publishViewState("debug");
    }
}

/**
 * M-11 调试面的接线（反汇编 / trace 导出 / trace 开关 / 停下后的**自动读回**）。
 *
 * ⚠️ **`services` 是可选的**（v0.67 批）⇒ ⇒ 旧调用点（只传 `commandRouter`）
 *   **照样能链接**⇒ ⇒ 最坏是「**没有自动读回**」（降级）⇒ ⇒ **不会白屏**。
 *
 * @param {object} commandRouter — M-05
 * @param {{registerMemory?: object|null}} [services]
 * @returns {object}
 */
export function wireDebug(commandRouter, services = {}) {
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
     * 内核断点表的**只读回执**（契约 v13 · `debug.breakpoints`）。
     *
     * ⚠️ 这是断点列表的**唯一数据源**：`ListBreakpoints` 与
     * `SetBreakpointEnabled` 的回复都走这里 ⇒ ⇒ 前端不再自己猜「内核有
     * 哪些断点」。
     */
    commandRouter.subscribe("debug.breakpoints", (event) => {
        const entries = Array.isArray(event.value?.breakpoints) ? event.value.breakpoints : [];
        const addresses = [];
        const symbols = {};
        const enabled = {};
        const hits = {};
        for (const item of entries) {
            const bp = item?.breakpoint ?? item ?? {};
            const addr = Number(bp.addr ?? 0) >>> 0;
            addresses.push(addr);
            enabled[String(addr)] = bp.enabled !== false;
            hits[String(addr)] = Number(item?.hit_count ?? 0);
            if (bp.source_hint) symbols[String(addr)] = String(bp.source_hint);
        }
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                breakpoints: addresses,
                bpSymbols: symbols,
                bpEnabled: enabled,
                bpHits: hits,
            },
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
    /**
     * 观察表达式轮询回执（契约 v14 · `debug.watch`）。
     * 值变化由内核计算，前端只显示。
     */
    commandRouter.subscribe("debug.watch", (event) => {
        const values = Array.isArray(event.value?.values) ? event.value.values : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), watchValues: values },
        }));
        publishViewState("debug");
    });

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
     * The kernel's **halt report** (the `Halted` event).
     *
     * ⚠️ **这是「停下」的**唯一落点**（STAGE-35 · v0.50 批 · 裁决 L）——
     * 脚本口 `window.riscv_kernel.lastHalt()` 与调试视图的「命中断点」提示
     * **读同一份** ⇒ ⇒ **不订阅两次**（那会是两份状态 ⇒ ⇒ 第二个真相）。
     *
     * ⚠️ **`reason === "Breakpoint"`** 是「命中断点」的唯一依据（裁决 M）
     * ⇒ ⇒ 前端**不自己判断**「跑没跑到那个地址」。
     *
     * ⚠️ **复位时保留**（与 `debug.trace` / `debug.error` 同一条规矩）：
     * 它是**内核最后一次的报告**，不是程序数据 ⇒ ⇒ 跨复位保留。
     */
    commandRouter.subscribe("halted", (event) => {
        const reason = event.value?.reason ?? null;
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                halt: {
                    reason: event.value?.reason ?? null,
                    pc: Number(event.value?.pc ?? 0) >>> 0,
                },
            },
        }));
        // ⚠️ **连续运行的续发落点**（STAGE-37 · v0.52 批 · 裁决 C）。
        //
        // ⚠️ **落在** `halted` **这一个订阅上** ⇒ ⇒ 不新订阅一次 ⇒ ⇒
        // 「内核的停下报告」**仍然只有一份** ⇒ ⇒ 不是第二个真相。
        //
        // ⚠️ **必须由订阅驱动**（不能写成 `await` 循环）—— 命令的 promise
        // 只 resolve **第一个**带 reqId 的事件（那是 `Tick`）⇒ ⇒ `Halted`
        // **只能**从订阅拿到（F8）。
        //
        // ⚠️ **判据在纯函数里**（`nextRunAction`）⇒ ⇒ 「该不该续」这件事
        // Node 侧**零 DOM** 就能验 ⇒ ⇒ 这里只是一次**调用**。
        const run = getRunState();
        const action = nextRunAction({ mode: run.mode, reason });
        // ⚠️ **在途计数 −1**（STAGE-38 · v0.53 批 · 裁决 B）⇒ ⇒ 一次执行
        // ⇒ ⇒ 一次停下报告 ⇒ ⇒ 这里**就是**那个退休点（**只有一处** ⇒ ⇒
        // 不是第二个真相）。
        const next = runStateAfter(run, reason);
        const busy = Math.max(0, next.busy - 1);
        if (action === RUN_ACTION_CONTINUE) {
            // ⚠️ ⚠️ **先减后加在同一个值里算完**（裁决 C · 施工期订正 2）
            // ⇒ ⇒ 「内核让出控制权」的那一瞬间计数**不**会掉到 0 ⇒ ⇒
            // 界面**不会**把活值闪出来一下 ⇒ ⇒ **顺序是硬约束**。
            // ⚠️ **「跑过没有」那一格带上**（`run-loop.js` 只搬三格）。
            setRunState({ ...next, busy: busy + 1, started: run.started === true });
            // ⚠️ **「用户要的那次停」作废**（v0.67 批）⇒ ⇒ 用户点过暂停**又**
            //   点了运行 ⇒ ⇒ 那个「等一次停」的意图**已经被覆盖**⇒ ⇒ 不能留着
            //   去匹配**后面**某次停下 ⇒ ⇒ 否则会冒出来一次**没人要**的读回。
            setPausePending(false);
            // ⚠️ **不 await** ⇒ ⇒ 这个订阅回调**不等**内核 ⇒ ⇒ 事件处理
            // 不会卡住 ⇒ ⇒ 错误也不往这里抛（抛了就是**未处理的拒绝**）。
            void Promise.resolve(commandRouter.dispatchCommand(Command.continueRun()))
                .catch(() => {
                    // ⚠️ **续发失败 ⇒ 退出连续模式** ⇒ ⇒ 界面不再显示
                    // 「在跑」⇒ ⇒ 状态位不会骗人。
                    //
                    // ⚠️ **计数也要退回来**⇒ ⇒ 否则内核**根本没跑**而计数说
                    // 「在跑」⇒ ⇒ 活值会**永远**藏着 ⇒ ⇒ 界面在骗人。
                    bumpRunBusy(-1);
                    transitionRunState(stopRunState(getRunState()));
                });
        } else {
            // ⚠️ **真停**（断点 / 观察点 / 陷阱 / 用户暂停）⇒ ⇒ 退出连续模式
            // ⇒ ⇒ **不续发** ⇒ ⇒ 连续模式在断点后**不会**继续跑
            // ⇒ ⇒ 活值**恢复显示**（`isRunning()` 变成假）。
            // ⚠️ **「跑过没有」那一格带上**（`run-loop.js` 只搬三格）。
            setRunState({ ...next, busy, started: run.started === true });
            // ⚠️ ⚠️ **自动读回**（v0.67 批）⇒ ⇒ 程序**真停**在这儿 ⇒ ⇒ 把
            //   用户正在看的那几样东西**重新取回来**（寄存器 + 内存单元 +
            //   转储）⇒ ⇒ **内存单元表跟着刷新**（用户报的那个现象）。
            //
            // ⚠️ **只在「真停」这一支里做** ⇒ ⇒ 连续运行里**每个窗口**都会来的
            //   那次 `EndOfExecution` **不会**触发 ⇒ ⇒ 真机门禁的脚本口**不会**
            //   因为每个批次多发两条命令而**慢到不可用**。
            //
            // ⚠️ **判据在纯函数里**（`shouldAutoReadback`）⇒ ⇒ Node 侧**零
            //   DOM** 就能验 ⇒ ⇒ 这里只是一次**调用**。
            const stopReason = readbackReasonOf(reason);
            // ⚠️ **用户点过暂停** ⇒ ⇒ 这次 `EndOfExecution` **就是**用户要的
            //   那次停（暂停**只停续发**，正在跑的那一个窗口跑完才停）⇒ ⇒ 也
            //   该读回。
            const pauseHit = reason === HALT_REASON_END_OF_EXECUTION && consumePausePending();
            if (stopReason !== null || pauseHit) {
                // ⚠️ **不 await** ⇒ ⇒ 这个订阅回调**不等**内核 ⇒ ⇒ 事件处理
                // 不会卡住 ⇒ ⇒ 错误也不往这里抛（抛了就是**未处理的拒绝**）。
                void performReadback(
                    // ⚠️ **带上 `commandRouter`** ⇒ ⇒ 「读这一段」的转储结果要
                    //   **重发一条** `DumpMemory` ⇒ ⇒ 而那条命令**只能**从这里
                    //   发 ⇒ ⇒ `wireDebug` 自己手里就有它 ⇒ ⇒ 不必让调用方
                    //   多传一份。
                    { ...services, commandRouter },
                    stopReason ?? READBACK_REASON.USER_PAUSE,
                    { auto: true },
                ).catch(() => {
                    // ⚠️ 读回失败**已经写进** `debug.readback.error` ⇒ ⇒ 这里
                    // 只是**不让它变成未处理的拒绝** ⇒ ⇒ 不吞（界面看得见）。
                });
            }
        }
        publishViewState("debug");
    });

    /**
     * The kernel's error replies (M-05's error path — the `Error` event
     * is published to the `error` topic **and** rejects the pending
     * command). The debug view shows the last one; there is no second
     * error channel.
     *
     * ⚠️ **契约 v9**：内核的错误**还要**按位显示（`E_DBG-02` ⇒ 条件那一位 /
     * 符号那一位；`E_DBG-03` ⇒ 转储那一位）⇒ ⇒ **写的是内核原文**
     * （`event.value.message`）⇒ ⇒ **不吞、不改写**。
     */
    commandRouter.subscribe("error", (event) => {
        const code = event.value?.code ?? "E_PROTO-08";
        const message = event.value?.message ?? "";
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                error: {
                    code,
                    message,
                },
            },
        }));
        // ⚠️ **按位显示**：谁发的命令 ⇒ 错误落在谁那一位 ⇒ ⇒ 一次错误
        //   **消费掉**这个标记 ⇒ ⇒ 不会串位。
        const slot = getPendingErrorSlot();
        setPendingErrorSlot(null);
        if (slot) writeErrorSlot(slot, message);
        publishViewState("debug");
    });

    /**
     * 符号查询的**答复**（契约 v9 · `debug.symbol`）。
     *
     * ⚠️ **状态写入必须在订阅回调里** ⇒ ⇒ **不是**在 `await
     *   dispatchCommand(...)` 之后 ⇒ ⇒ M-05 的 `handleFrame` 是**先
     *   resolve 命令的 promise、再 publish 事件** ⇒ ⇒ 「await 回来」比
     *   「订阅回调」**更晚** ⇒ ⇒ 写在 await 之后会落在错的那一格上
     *   （既有 `case "disasm"` 就是这个模式）。
     *
     * ⚠️ **这是一次查询的答复**（推导量）⇒ ⇒ 住视图状态、**不持久化**
     *   ⇒ ⇒ 不是内核状态的第二份副本。
     */
    commandRouter.subscribe("debug.symbol", (event) => {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), symbol: event.value ?? null },
        }));
        publishViewState("debug");
    });

    /**
     * 整段转储的**答复**（契约 v9 · `debug.dump`）。
     *
     * ⚠️ **落点在 `debug` 视图状态**（规格 §5.2：`debug.dump` · v0.65 批随
     *   「寄存器 / 内存」视图并进调试视图而改落点）⇒ ⇒ **不**另存一份 ⇒ ⇒
     *   调试视图「内存窗口」显示的**就是**这一份。
     *
     * ⚠️ **同样在订阅回调里写**（理由同 `debug.symbol`）。
     */
    commandRouter.subscribe("debug.dump", (event) => {
        const addr = Number(event.value?.addr ?? 0) >>> 0;
        const bytes = Array.isArray(event.value?.bytes) ? event.value.bytes : [];
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: {
                ...(prev.debug ?? {}),
                dump: { addr, bytes: bytes.map((b) => Number(b) & 0xff) },
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
                // ⚠️ **符号查询的答复也清掉**（契约 v9）⇒ ⇒ 它是**上一个程序**
                //   的符号 ⇒ ⇒ 留着就是一份**错**的答案。
                //
                // ⚠️ **转储结果也清掉**（同一个道理 ⇒ ⇒ 它是**上一个程序**的
                //   内存）⇒ ⇒ 与符号**同一笔**写、**同一拍**发布 ⇒ ⇒ 视图
                //   **不会**看见「一半清了、一半没清」的中间态。
                debug: {
                    ...(prev.debug ?? {}),
                    export: null,
                    symbol: null,
                    dump: null,
                    // ⚠️ **断点 / 符号标签也必须清**：内核在 `LoadProject` /
                    //   `UnloadProject` 里调 `clear_debug_state()`，但契约
                    //   **没有**「列出断点」的事件 ⇒ ⇒ 前端不在这里清，就会
                    //   保留上一程序的假断点；用户再点同一地址时又会被
                    //   `requestBreakpoint` 去重拦住，命令根本发不出去。
                    breakpoints: [],
                    bpSymbols: {},
                    bpEnabled: {},
                    bpHits: {},
                    watchValues: [],
                    watchNextId: 1,
                    // ⚠️ **读回的那一份也清掉**（v0.67 批）⇒ ⇒ 基准与「上次
                    //   读回」说的都是**上一个程序**⇒ ⇒ 留着就是一份**不存在的**
                    //   对比⇒ ⇒ 用户会看到**上一趟**的高亮。
                    readback: null,
                },
            }));
            publishViewState("debug");
            // ⚠️ **「用户要的那次停」也作废**（v0.67 批）⇒ ⇒ 复位之后正在跑的
            //   那个窗口**不算**用户要的那次停⇒ ⇒ 否则复位之后会冒出来一次
            //   **没人要**的读回。
            setPausePending(false);
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
     * 「为什么停、停在哪儿」。⇒ `Halted { reason, pc }` 由 `wireDebug`
     * 记进**视图状态** `debug.halt`（STAGE-35 · v0.50 批 · 裁决 L）。
     *
     * ⚠️ **这里只读，不再订阅一次** ⇒ ⇒ 脚本口与调试视图**读同一份**
     * ⇒ ⇒ 「停下」**只有一个真相**（两份状态 = 第二个真相 ⇒ 会漂）。
     *
     * @returns {{reason: string|null, pc: number}|null}
     */
    function lastHalt() {
        return viewStore.getState("debug")?.debug?.halt ?? null;
    }

    window.riscv_kernel = {
        /** 最近一次「停下」的原因与 PC（`Halted` 事件）；还没有就是 `null`。 */
        lastHalt,
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
         *
         * ⚠️ **在途计数 +1**（STAGE-38 · v0.53 批 · F9 / F10）⇒ ⇒ 脚本口
         * 跑的那**一个**窗口也算「运行中」⇒ ⇒ 活值**也**隐藏 ⇒ ⇒ 否则
         * 「运行中」只看 `debug.run.mode` 就会漏掉这一种。
         *
         * ⚠️ **退休点在 `halted` 订阅**（裁决 B）⇒ ⇒ 这里**只**加不减 ⇒ ⇒
         * 减的地方**只有一处** ⇒ ⇒ 不是第二个真相。
         */
        continueRun: () => {
            bumpRunBusy(+1);
            return dispatch(Command.continueRun(), 30000).catch((err) => {
                // ⚠️ **发不出去 ⇒ 计数退回来**⇒ ⇒ 否则内核**根本没跑**而
                // 计数说「在跑」⇒ ⇒ 活值会**永远**藏着 ⇒ ⇒ 界面在骗人。
                bumpRunBusy(-1);
                throw err;
            });
        },
    };
}

// ---------------------------------------------------------------------------
// 装载**仓外**的镜像（真实 RTOS 的 ELF 不进仓库）
// ---------------------------------------------------------------------------

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
 * 「按**已在手上**的字节 → 发 `LoadProject`」的入口（STAGE-32 · v0.47 批）。
 *
 * ⚠️ **这是 UI 那条路的落点**：用户在**文件选择器**里选了固件 ⇒ ⇒ 浏览器
 * 把字节读出来 ⇒ ⇒ 视图层把它们放进 `riscv:command` 信号 ⇒ ⇒ app 层
 * （**这里**）把字节变成工程并装载。
 *
 * ⚠️ **与 `loadImageFromUrl` 共用同一条装载路径**（`loadProjectIntoKernel`）
 * ⇒ ⇒ **不写第二套**（规格 §5 的台账边界）⇒ ⇒ 重订流、清残留这些语义
 * **只有一处**。
 *
 * ⚠️ **装完就停**（规格 §3 裁决 D）⇒ ⇒ 这里⛔ **不**发 `ContinueRun`
 * ⇒ ⇒ 跑不跑由用户自己点「继续」。
 *
 * ⚠️ **内核不联网**（`ImageSource::Url` 在宿主里报 `E_LDR-01`）⇒ ⇒ 字节
 * 必须是**真的字节**（不是 base64、不是字符串）⇒ ⇒ 契约层要的形状是
 * `{"Bytes":[...]}`。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {number[]|Uint8Array} bytes — 镜像字节
 * @param {{name?: string, kind?: string, id?: string, projectName?: string, boardName?: string}} [options]
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, byteLength: number, project: object}>}
 */
export async function loadImageFromBytes(services, bytes, options = {}) {
    const project = buildImageProject(
        options.name ? `/local/${options.name}` : "/local/image.bin",
        bytes,
        {
            imageName: options.name ?? "image.bin",
            kind: options.kind ?? "Elf",
            id: options.id ?? "proj-user-image",
            name: options.projectName ?? options.name ?? "image.bin",
            boardName: options.boardName ?? "MainBoard",
            toolchain: "user file (browser)",
        },
    );
    const result = await loadProjectIntoKernel(services, project);
    return { ...result, byteLength: Array.from(bytes ?? []).length, project };
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
 * ⚠️ 这条链**没有 UI 入口**：真实 RTOS 的镜像**在仓外**（不进仓库），
 * 浏览器要拿到它就得**自己**把字节取回来——只能由脚本
 * （`tools/browser-verify.mjs`）驱动。
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
    // ⚠️ **顶栏那条全局运行条**（STAGE-34 · v0.49 批）⇒ ⇒ 三个按钮直接调
    // `handleCommand` ⇒ ⇒ 与调试视图那份**共用同一条命令路径**。
    wireTopbarRunbar(services);
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
