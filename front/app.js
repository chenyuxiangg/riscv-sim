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
    downloadExportChunk,
    mountViewComponents,
    resetTerminalScreen,
    seedBus,
    get as getViewComponent,
} from "./views/m02_viewcomponents.js";
import * as viewStore from "./views/m03_viewstore.js";
import { mountSession } from "./services/m04_session.js";
import { mountCommandRouter } from "./services/m05_commandrouter.js";
import * as transportApi from "./services/m07_transport.js";
import { mountRegisterMemory } from "./services/m10_registermemory.js";
import { Command, TRACE_FORMAT, TRACE_SOURCE, WRITE_SIZE } from "./lib/message.js";
// ⚠️ 只导入**纯函数**（错误码拆分、草稿转换、「已被内核接受」推导量）⇒
// 不引入服务层依赖，也不构成循环依赖。
import {
    currentDraft,
    describeMemoryMapError,
    draftFromDescription,
    draftToDescription,
    isDraftAccepted,
    planMemoryMapUpdate,
    splitErrorCode,
} from "./views/components/memory-map.js";
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
import { buildDemoStoreState, seedDemoState } from "./dev/demo-state.js";
// ⚠️ **波形边沿的纯函数**（STAGE-36 · v0.51 批）⇒ ⇒ 累加与截断**剥成纯函数**
// ⇒ ⇒ Node 侧零 DOM 就能测 ⇒ ⇒ 与 v0.31 批「差量合成剥成纯函数」同形。
import { mergeEdges, WAVE_SAMPLE_LIMIT } from "./lib/wave-samples.js";
// ⚠️ **波形库的加载器**（STAGE-36 · v0.51 批）⇒ ⇒ 本批**第一个**外部依赖
// ⇒ ⇒ URL 是**全仓唯一落点**（住加载器里）⇒ ⇒ 加载失败**不抛** ⇒ ⇒ 界面走降级。
import {
    getChartSource,
    loadECharts,
    setChartSource,
} from "./lib/echarts-loader.js";
// ⚠️ **连续运行的纯函数**（STAGE-37 · v0.52 批）⇒ ⇒ 「该不该续发」**剥成纯函数**
// ⇒ ⇒ Node 侧零 DOM 就能测 ⇒ ⇒ 与 STAGE-36 的「边沿合并剥成纯函数」同形。
import {
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
// ⚠️ **trace 记录开关的判定**（纯函数）⇒ ⇒ 「点一下该发什么」只有一处说了算
// ⇒ ⇒ 视图层渲染的按钮标签与这里发出的命令**引同一处** ⇒ ⇒ 不会对不上。
import { nextTraceEnabled, traceSwitchState, TRACE_SWITCH_OFF } from "./lib/trace-switch.js";
// ⚠️ **「把整条轨迹翻完」的分页收集**（纯逻辑）⇒ ⇒ 「下载轨迹到本地」
//   顺着 `next_offset` 一块一块翻 ⇒ ⇒ 翻完才是**完整**的一份。
import { collectTraceChunks } from "./lib/trace-export.js";
// ⚠️ **告警弹窗**（STAGE-38 · v0.53 批）⇒ ⇒ 运行中点「读内存」= 拒绝 + 弹窗。
// ⇒ ⇒ 本仓**第一个**碰 DOM 的 lib 模块（弹窗是**全局**的，不属于任何一个视图）。
import { showAlert } from "./lib/alert.js";

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
 * 它 ⇒ ⇒ 事件只能挂在这里（而 app 层**已经**在管页头 —— 见 `wireDevSeeding`）。
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
    // ⚠️ **寄存器视图也要跟着变**（裁决 M）⇒ ⇒ 它读的是**同一份** `debug.run`
    // ⇒ ⇒ 不是第二个真相。
    publishViewState("regmem");
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

// ---------------------------------------------------------------------------
// 板档案（STAGE-15 · v0.30 · D-136 / D-140）
//
// ⚠️ **落库门槛**（裁决 B）：只有**被内核接受过**的描述才允许落库 ⇒ 库里
// 不会有「永远装不上的板」⇒ 前端**不写第二份校验器**（延续裁决 E）。
//
// ⚠️ **保存 ≠ 生效**（裁决 C）：「保存 / 另存为 / 选用 / 删除」**都不改内核
// 状态**（不发契约命令）⇒ 板要生效仍需「提交板级内存图」。
// ---------------------------------------------------------------------------

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
 * 板档案在**视图状态**里的四个字段（读的时候把缺省值补上 ⇒ 调用方不必判空）。
 *
 * ⚠️ 这四个字段住在 M-03 的「memory-map」视图状态里（**前端自己的东西**）
 * ⇒ 不进契约、不进事件。
 *
 * @param {object} [state] — 「memory-map」视图状态（不给就读当前状态）
 * @returns {{archive: object[], selectedId: string|null, archiveStatus: string, acceptedDescription: object|null}}
 */
export function boardState(state) {
    const src = state ?? viewStore.getState("memory-map") ?? {};
    return {
        archive: Array.isArray(src.archive) ? src.archive : [],
        selectedId: src.selectedId ?? null,
        archiveStatus: src.archiveStatus ?? "ok",
        acceptedDescription: src.acceptedDescription ?? null,
    };
}

/**
 * 把板档案的四个字段**初始化**进视图状态（缺什么补什么 ⇒ 不覆盖已有值）。
 *
 * @returns {object} 初始化后的视图状态
 */
export function ensureBoardState() {
    const prev = viewStore.getState("memory-map") ?? {};
    const next = { ...prev, ...boardState(prev) };
    viewStore.setState("memory-map", next);
    return next;
}

/**
 * 写板档案的视图状态并重渲染一次。
 *
 * @param {object} patch
 */
function setBoardState(patch) {
    viewStore.setState("memory-map", (prev) => ({ ...prev, ...patch }));
    publishViewState("memory-map");
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
 * 板档案服务缺席时的**显式**降级（不静默）。
 *
 * @param {unknown} error
 */
function setArchiveUnavailable(error) {
    setBoardState({ archiveStatus: "error" });
    setNote(`板档案服务不可用：${error?.message ?? error}`);
}

/**
 * 把一条档案写进**持久化存储**。
 *
 * ⚠️ 没有存储 ⇒ **明确降级**为「只存在于本会话」（返回 `unsupported`），
 * 不假装存下来了；写失败 ⇒ 返回 `error`（调用方据此**不**改档案）。
 *
 * ⚠️ **失败时把原因带回去**（`reason`）⇒ 上层**能说清**失败在哪 ⇒
 * **不静默**（STAGE-18 裁决 I：升级失败要显示失败原因）。
 *
 * @param {{boardStore?: object|null}} services
 * @param {{id: string, description: object, rules_version?: number}} entry
 * @returns {Promise<{status: "ok"|"unsupported"|"error", reason: string|null}>}
 */
async function persistBoard(services, entry) {
    const store = services?.boardStore ?? null;
    if (!store) return { status: "unsupported", reason: null };
    try {
        await store.put(entry);
        return { status: "ok", reason: null };
    } catch (err) {
        const reason = err?.message ?? String(err);
        setBoardState({ archiveStatus: "error" });
        setNote(`板档案落库失败：${reason}`);
        return { status: "error", reason };
    }
}

/**
 * 「保存当前板」⇒ 把**当前草稿**写进板档案（键 = 草稿的 `id`）。
 *
 * ⚠️ **不改内核状态**（不发契约命令）⇒ 板要生效仍需「提交」。
 *
 * @param {object} services
 */
async function saveCurrentBoard(services) {
    ensureBoardState();
    if (!isDraftAccepted(viewStore.getState("memory-map"))) {
        setNote("当前草稿还没被内核接受过 —— 先提交，校验通过后才能保存");
        return;
    }
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        setArchiveUnavailable(error);
        return;
    }
    const state = boardState();
    const description = draftToDescription(currentDraft(viewStore.getState("memory-map")));
    const entry = api.boardEntry(description);
    if (!entry) {
        setNote("板 id 为空 —— 这块板存不进档案（先给板 id）");
        return;
    }
    // ⚠️ **新板**（没选中档案里的一块）⇒ id 不能与已有档案撞。
    // ⇒ 这不是「描述合法性」（那是内核的事）⇒ 是**档案的主键唯一性**
    //   （裁决 E：「前端唯一会判的事：档案里 id 不重复」）。
    if (state.selectedId === null && api.isBoardIdTaken(state.archive, entry.id)) {
        setNote(`板 id「${entry.id}」已被占用 —— 改个 id，或先「选用」那块板再保存（那是覆盖）`);
        return;
    }
    const merged = api.mergeBoard(state.archive, entry);
    const persisted = await persistBoard(services, entry);
    if (persisted.status === "error") return;
    setBoardState({
        archive: merged.archive,
        selectedId: entry.id,
        archiveStatus: persisted.status === "ok" ? "ok" : "unsupported",
    });
    setNote(persisted.status === "ok"
        ? `已保存板「${entry.id}」`
        : `板「${entry.id}」已存入本会话（板档案只存在于本会话）`);
}

/**
 * 「另存为板」⇒ 以当前草稿为模板**新建**一块板（新 `id` + 新 `name`）。
 *
 * ⚠️ **只改 `id` / `name`** ⇒ 其余字段与**已通过校验**的那份逐字相同 ⇒
 * 合法性必然相同 ⇒ **不需要第二次提交、也不需要第二次校验**。
 *
 * ⚠️ **不改内核状态**（不发契约命令）。
 *
 * @param {object} services
 * @param {{id?: string, name?: string}} [params]
 */
async function saveBoardAs(services, params) {
    ensureBoardState();
    if (!isDraftAccepted(viewStore.getState("memory-map"))) {
        setNote("当前草稿还没被内核接受过 —— 先提交，校验通过后才能保存");
        return;
    }
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        setArchiveUnavailable(error);
        return;
    }
    const state = boardState();
    const newId = String(params?.id ?? "").trim();
    const newName = String(params?.name ?? "").trim();
    if (newId === "") {
        setNote("另存为需要一个新的板 id —— 填了再点");
        return;
    }
    if (newName === "") {
        setNote("另存为需要一个新的板名 —— 填了再点");
        return;
    }
    // ⚠️ **id 唯一**（裁决 E）：已被占用 ⇒ 明确拒绝，不静默覆盖。
    if (api.isBoardIdTaken(state.archive, newId)) {
        setNote(`板 id「${newId}」已被占用 —— 换一个没被占用的 id`);
        return;
    }
    const description = draftToDescription(currentDraft(viewStore.getState("memory-map")));
    const entry = api.boardEntry({ ...description, id: newId, name: newName });
    if (!entry) {
        setNote("板 id 为空 —— 这块板存不进档案");
        return;
    }
    const merged = api.mergeBoard(state.archive, entry);
    const persisted = await persistBoard(services, entry);
    if (persisted.status === "error") return;
    // ⚠️ **选中项不变**：草稿一个字都没改 ⇒ 它仍代表原来那块板。
    setBoardState({
        archive: merged.archive,
        archiveStatus: persisted.status === "ok" ? "ok" : "unsupported",
    });
    setNote(persisted.status === "ok"
        ? `已另存为板「${newId}」（原板仍在档案里）`
        : `板「${newId}」已存入本会话（板档案只存在于本会话）`);
}

/**
 * 「选用」档案里的一块 ⇒ **载入草稿**（**不改内核状态**）。
 *
 * ⚠️ 载入**不是**接受：被载入的草稿**还要重新提交**才被内核接受 ⇒
 * 「保存 / 另存为」在重新提交前是**禁用**的（落库门槛，裁决 B）。
 *
 * ⚠️ **规则版本落后 ⇒ 明确提示**（STAGE-18 · D-143 · 裁决 I）：提示里**同时**
 * 给出「这块板的规则版本」与「当前规则版本」⇒ ⇒ **不静默**。
 * ⚠️ **提示之后仍然可以选用**（草稿照常载入）⇒ ⇒ 前端**不判**合法性
 * （合法性**只**由内核那一处裁决 ⇒ 不写第二份校验器）。
 *
 * @param {string} id
 */
async function selectBoard(id) {
    ensureBoardState();
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        setArchiveUnavailable(error);
        return;
    }
    const state = boardState();
    const entry = api.findBoard(state.archive, String(id ?? ""));
    if (!entry) {
        setNote(`板档案里没有 id「${String(id ?? "")}」这一块`);
        return;
    }
    const draft = draftFromDescription(entry.description);
    if (!draft) {
        setNote("这块板的描述读不出来 —— 档案可能坏了");
        return;
    }
    // ⚠️ **规则版本判定在载入之前**（但**不拦**载入）⇒ 提示与「已选用」一起
    // 说 ⇒ 用户既看得见差在哪，也照样能把板载进草稿。
    const notice = api.boardRulesNotice(entry);
    setBoardState({ draft, selectedId: entry.id });
    setNote(notice
        ? `已选用板「${entry.id}」—— ⚠️ ${notice}（尚未提交 —— 要生效请点「提交板级内存图」）`
        : `已选用板「${entry.id}」（尚未提交 —— 要生效请点「提交板级内存图」）`);
}

/**
 * 「删除」档案里的一块 ⇒ **不改内核状态**。
 *
 * @param {object} services
 * @param {string} id
 */
async function deleteBoard(services, id) {
    ensureBoardState();
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        setArchiveUnavailable(error);
        return;
    }
    const state = boardState();
    const dropped = api.dropBoard(state.archive, String(id ?? ""));
    if (!dropped.changed) {
        setNote(`板档案里没有 id「${String(id ?? "")}」这一块`);
        return;
    }
    const store = services?.boardStore ?? null;
    if (store) {
        try {
            await store.remove(String(id ?? ""));
        } catch (err) {
            setBoardState({ archiveStatus: "error" });
            setNote(`板档案删除失败：${err?.message ?? err}`);
            return;
        }
    }
    setBoardState({
        archive: dropped.archive,
        // ⚠️ 被删的那块不再是「选中项」⇒ 选中清空（草稿还在，id 重新可编辑）。
        selectedId: state.selectedId === String(id ?? "") ? null : state.selectedId,
        archiveStatus: store ? "ok" : "unsupported",
    });
    setNote(store
        ? `已删除板「${String(id ?? "")}」`
        : `已删除板「${String(id ?? "")}」（板档案只存在于本会话）`);
}

/**
 * 内核**真的接受**了一份描述之后 ⇒ 把档案里对应的那一条**升级**到当前规则版本。
 *
 * ⚠️ **升级条件 = 内核真的接受过**（STAGE-18 · D-143 · 裁决 I 的最后一条）
 * ⇒ ⇒ 不引入「前端说它没问题」这种第二判据 ⇒ ⇒ **只**在提交**成功**之后
 * 调它。
 *
 * ⚠️ **升级动作 = 重写那一条**（带上当前 `rules_version`）⇒ ⇒ **不发任何
 * 契约命令**（纯前端动作）。
 *
 * ⚠️ **升级失败不静默**（裁决 I）：写库失败 ⇒ 把原因带回去 ⇒ 上层显示它
 * ⇒ ⇒ **描述仍然可用**（只是条目还是旧的 ⇒ 下次还会提示）。
 *
 * ⚠️ **按 `id` 找那一条**（不按描述逐字段比）：板档案的**主键就是 `id`**
 * ⇒ 「那块板」= 那个 id ⇒ ⇒ 草稿 → 描述的往返**本来就不是逐字节相同**的
 * （实测：器件会被补上 `irq_line: 0`、可选的 `max_region_bytes` 会掉）
 * ⇒ ⇒ 若按字段相同来判 ⇒ **永远升级不了**（静默失效）⇒ ⇒ 按 id 判。
 *
 * @param {{boardStore?: object|null}} services
 * @param {object|null|undefined} description — 内核刚接受的那份描述
 * @returns {Promise<{status: "upgraded"|"current"|"skipped"|"error"|"unavailable", version?: number, reason: string|null, persisted?: "ok"|"unsupported"}>}
 */
async function upgradeBoardRules(services, description) {
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        return {
            status: "unavailable",
            reason: `板档案服务不可用：${error?.message ?? error}`,
        };
    }
    const id = api.boardId(description);
    if (id === null) return { status: "skipped", reason: null };
    const state = boardState();
    const entry = api.findBoard(state.archive, id);
    // ⚠️ 档案里**没有**这一块 ⇒ 没什么可升级（它还没被保存进档案）。
    if (!entry) return { status: "skipped", reason: null };
    // ⚠️ **已经是当前版本** ⇒ 不重写（不制造无谓的写）。
    if (api.checkBoardRules(entry).ok) return { status: "current", reason: null };
    const upgraded = { ...entry, rules_version: api.PLATFORM_RULES_VERSION };
    const persisted = await persistBoard(services, upgraded);
    if (persisted.status === "error") {
        return { status: "error", reason: persisted.reason };
    }
    const merged = api.mergeBoard(state.archive, upgraded);
    setBoardState({ archive: merged.archive });
    return {
        status: "upgraded",
        version: api.PLATFORM_RULES_VERSION,
        reason: null,
        persisted: persisted.status,
    };
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
        // ⚠️ **产品树**也要换成新工程的（否则还显示 demo 的树）。
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
            Command.exportTrace(source, format, offset, 0),
        );
        return ev?.value ?? {};
    });
    if (!result.content) return null;
    return downloadExportChunk({ source, format, chunk: result.content });
}

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
                // ⚠️ **先退出连续模式**（裁决 O）⇒ ⇒ 否则下一轮 `Continue`
                // 立刻把这一步盖掉 ⇒ ⇒ 「单步」在连续模式下**不可用**。
                //
                // ⚠️ **顺带记下「跑过」**（v0.63 批）⇒ ⇒ 程序离开过起点 ⇒ ⇒
                //   顶栏那个按钮从「运行」切成「继续」。
                setRunState(markStarted(stopRunState(getRunState())));
                await commandRouter.dispatchCommand(Command.stepN(1));
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
                await commandRouter.dispatchCommand(Command.stepN(steps));
                break;
            }
            case "step10":
                // ⚠️ **与「单步」同一条路**（同裁决 O）⇒ ⇒ 10 条也是「走一段
                // 然后停」⇒ ⇒ 连续模式必须让位。
                transitionRunState(stopRunState(getRunState()));
                await commandRouter.dispatchCommand(Command.stepN(10));
                break;
            case "continue":
                // ⚠️ **「运行」= 连续运行**（裁决 A）⇒ ⇒ 进连续模式 ⇒ 发
                // `Continue`（内核那边仍然是「跑一个窗口」）⇒ ⇒ **前端**在
                // `halted` 订阅里**接着发下一条**（裁决 C）。
                //
                // ⚠️ **幂等**（裁决 I）：已经在连续模式 ⇒ ⇒ **不重发** ⇒ ⇒
                // 连点两下不会变成两个并行窗口。
                if (getRunState().mode === RUN_MODE_RUNNING) break;
                // ⚠️ **连续运行 ⇒ ⇒ 程序离开过起点** ⇒ ⇒ 「跑过没有」置真。
                setRunState({ ...startRunState(), started: true });
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
                            console: consoleWiring,
                            memoryMap: memoryMapWiring,
                        },
                        loadedProject ?? buildMockProject(),
                    );
                    resetProbeError = result.probeError;
                    resetConsoleError = result.consoleError;
                    resetMemoryMapError = result.memoryMapError;
                }
                break;
            case "read-regs": {
                // ⚠️ **上一次读回的 GPR 快照**（STAGE-35 · v0.50 批 · 裁决 F）
                // ⇒ ⇒ **在读之前**存 ⇒ ⇒ 视图层可以拿「这次的值」和「上一次
                // 的值」比 ⇒ ⇒ 高亮变化。
                //
                // ⚠️ **落点在 `regmem` 视图状态里** ⇒ ⇒ **只有一份** ⇒ ⇒
                // 视图层**只读**（分层不破）⇒ ⇒ 不另造一个计数器状态。
                const prevGpr = viewStore.getState("regmem")?.sim?.gpr ?? null;
                viewStore.setState("regmem", (state) => ({
                    ...state,
                    regmem: {
                        ...(state.regmem ?? {}),
                        prevGpr: Array.isArray(prevGpr) ? prevGpr.slice() : null,
                    },
                }));
                await registerMemory.readRegisters();
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
                await registerMemory.readMemory(readAddr, readSize);
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
                requestBreakpoint(addr);
                // ⚠️ **内核错误落在「条件」那一位** ⇒ ⇒ 先记目标位、再发命令。
                pendingErrorSlot = "cond";
                writeErrorSlot("cond", "");
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: null,
                    enabled: true,
                    condition,
                }));
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
                requestBreakpoint(addr);
                if (symbol) labelBreakpointSymbol(addr, symbol);
                pendingErrorSlot = "cond";
                writeErrorSlot("cond", "");
                await commandRouter.dispatchCommand(Command.setBreakpoint({
                    id: addr,
                    addr,
                    source_hint: symbol,
                    enabled: true,
                    condition,
                }));
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
                pendingErrorSlot = "symbol";
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
                pendingErrorSlot = "dump";
                writeErrorSlot("dump", "");
                await commandRouter.dispatchCommand(Command.dumpMemory(addr, len));
                break;
            }
            case "clear-breakpoint": {
                // 撤掉断点 ⇒ ⇒ id 同样**就是地址**（裁决 J）⇒ ⇒ 前端列表
                // 只存地址就够 ⇒ ⇒ `ClearBreakpoint { id }` 传的是地址。
                const addr = toInt(detail.params?.addr, 0) >>> 0;
                withdrawBreakpoint(addr);
                await commandRouter.dispatchCommand(Command.clearBreakpoint(addr));
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
            case "download-trace-local": {
                // ⚠️ **「下载当前这份导出的全部内容」**（v0.63 批）⇒ ⇒ 源 / 格式
                //   **跟着当前导出状态走**（默认 = 执行轨迹 CSV）⇒ ⇒ 「导出波形
                //   (VCD)」之后点它 ⇒ ⇒ 下的是**整份波形**⇒ ⇒ 波形那条路**没有**
                //   被这次去重**砍掉**。
                //
                // ⚠️ **默认值与视图层那个按钮的文案逐字一致**（`Execution` /
                //   `Csv`）⇒ ⇒ 按钮上写的与实际下的**不可能**对不上。
                const current = viewStore.getState("debug")?.debug?.export ?? null;
                const source = String(current?.source ?? TRACE_SOURCE.Execution);
                const format = String(current?.format ?? TRACE_FORMAT.Csv);
                try {
                    const result = await downloadWholeTrace(services, { source, format });
                    if (result) {
                        setNote(`已下载 ${result.filename}（内核报告 ${result.total ?? "?"} 条）`);
                    }
                } catch (err) {
                    updateDebugState((prev) => ({
                        ...prev,
                        debug: {
                            ...(prev.debug ?? {}),
                            error: {
                                code: err?.code ?? "E_CTX-06",
                                message: `下载轨迹失败：${err?.message ?? String(err)}`,
                            },
                        },
                    }));
                    return;
                }
                break;
            }
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
            case "submit-memory-map": {
                // 提交板级内存图（契约 v6 的 `SetMemoryMap` / **v7 的
                // `PatchMemoryMap`**）。
                //
                // ⚠️ **走哪条路由前端自己判**（裁决 D）：元数据（`id` /
                // `name` / `priority` / `auto_selectable`）变了 ⇒ **整份
                // 替换**；否则 ⇒ **差量**（只发改动的那几段）。
                //
                // ⚠️ **差量的基底 = 最近一次 `memory.map` 事件带回来的生效
                // 图**（裁决 I）⇒ 与宿主作用的对象**是同一个东西**。
                //
                // ⚠️ **前端不自写校验**（裁决 E）：契约层那个唯一校验器在内核
                // 侧 ⇒ 校验失败由内核报 `E_PLT-*` ⇒ 这里**如实显示**它。
                const description = detail?.params?.description ?? null;
                // ⚠️ 板档案的四个字段**先**初始化（缺省值就是规格里的初值）
                // ⇒ 「失败时 `acceptedDescription` 不动」这条断言看得见的是
                //   `null`（初值），而不是「字段压根不存在」。
                ensureBoardState();
                const state = viewStore.getState("memory-map") ?? {};
                const base = state.memoryMap?.description ?? null;
                // ⚠️ **期望修订号**（裁决 B）：**从没收到过** `memory.map`
                // 事件 ⇒ `null`（不检查）；否则带上事件带来的那个。
                const expectedRevision = state.memoryMap
                    ? Number(state.memoryMap.revision ?? 0)
                    : null;
                const plan = planMemoryMapUpdate(base, description);
                if (plan.mode === "none") {
                    // ⚠️ **什么都没改 ⇒ 不发命令**（免得白涨一次修订号）。
                    setNote("板级内存图没有改动 —— 不用提交");
                    return;
                }
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
                    setMemoryMapError(null);
                    // ⚠️ **成功后**才记「这份描述被内核接受过」（裁决 B 的落库
                    // 门槛）⇒ 失败时**不动** `acceptedDescription`（失败分支
                    // 只做 `setMemoryMapError`）⇒ 库里不会有没校验过的板。
                    //
                    // ⚠️ 记的是**草稿那一份**（差量合成结果逐字段等于草稿 ⇒
                    // 裁决 I 的推论）⇒ 落库门槛**一字不改**。
                    setBoardState({ acceptedDescription: description ?? null });
                } catch (err) {
                    // ⚠️ **校验失败必须在界面上显示**（验收 C-5）⇒ 码与文案
                    // 写进「memory-map」视图状态 ⇒ 面板上有一块
                    // `data-memory-map-error`。
                    //
                    // ⚠️ **修订号冲突（`E_PLT-13`）不许吞掉**（验收 C-8）⇒
                    // 附带一句「图已被改，请刷新后再提交」。
                    const { code, message } = splitErrorCode(err);
                    setMemoryMapError({
                        code,
                        message: describeMemoryMapError(code, message),
                    });
                    throw err;
                }
                // ⚠️ **升级**（STAGE-18 · D-143 · 裁决 I / J）：内核**真的接受**
                // 之后 ⇒ 把档案里对应的那一条**重写一次**（带上当前
                // `rules_version`）⇒ ⇒ **有提示**（不静默迁移）。
                //
                // ⚠️ **落点在 `catch` 之后** ⇒ 走到这里**只可能**是内核接受
                // 了（失败那条路 `catch` 里 `throw` 掉了）⇒ ⇒ **不引入**「前端
                // 说它没问题」这种第二判据（裁决 I 的最后一条）。
                //
                // ⚠️ **不发任何契约命令** ⇒ 升级是**纯前端**动作（重写库里的
                // 那一条）。
                const upgrade = await upgradeBoardRules(services, description);
                if (upgrade.status === "upgraded") {
                    setNote(upgrade.persisted === "unsupported"
                        ? `已升级到当前规则版本 ${upgrade.version}（板档案只存在于本会话）`
                        : `已升级到当前规则版本 ${upgrade.version}`);
                    return;
                }
                if (upgrade.status === "error") {
                    // ⚠️ **失败不静默**（裁决 I）⇒ 显示原因 ⇒ 描述**仍然可用**
                    // （只是条目还是旧的 ⇒ 下次还会提示）。
                    setNote(`板档案升级失败：${upgrade.reason} —— 描述仍然可用（这块板的条目还是旧的，下次还会提示）`);
                    return;
                }
                break;
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
                consoleWiring?.reset?.();
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
            case "save-memory-map":
                // 「保存当前板」⇒ 落库（**不改内核状态**）。
                await saveCurrentBoard(services);
                return;
            case "save-memory-map-as":
                // 「另存为板」⇒ 新建一块板落库（**不改内核状态**）。
                await saveBoardAs(services, detail?.params);
                return;
            case "select-memory-map":
                // 「选用」⇒ 把档案里那块板载进草稿（**不改内核状态**）。
                await selectBoard(detail?.params?.id);
                return;
            case "delete-memory-map":
                // 「删除」⇒ 从档案里删一块（**不改内核状态**）。
                await deleteBoard(services, detail?.params?.id);
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
    consoleWiring.reset();
    await registerMemory.loadProject(project);
    // ⚠️ **重订部分抽成助手**（裁决 F · v0.31）⇒ 装载与卸载**共用**同一套
    // ⇒ **不写第二套**。
    //
    // ⚠️ `skipRegisters` = 真：M-10 的 `loadProject()` **自己**就强制重订
    // 了寄存器流（第 3 步）⇒ 这里不再重复发一次。
    const errors = await resubscribeAllStreams(services, { skipRegisters: true });
    return {
        probeError: errors.probeError,
        consoleError: errors.consoleError,
        memoryMapError: errors.memoryMapError,
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
 * ⚠️ **要重订的五条流**（前端的**事件流**）：
 *   - 寄存器流 `sim.registers`（M-10 那一条路，顺带管状态增量流
 *     `sim.state.delta` —— 两条流都由 M-10 消费）；
 *   - 状态增量流 `sim.state.delta`（同上）；
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
    /** @type {{registerError: Error|null, probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}} */
    const errors = {
        registerError: null,
        probeError: null,
        consoleError: null,
        memoryMapError: null,
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
    return errors;
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
     * ⚠️ **STAGE-36 · v0.51 批**：条目里**多了一个** `samples`（**能画图的
     * 边沿**）⇒ ⇒ 此前内核推来的边沿**只**被用来累加计数与取末值、**样本本身
     * 被丢掉** ⇒ ⇒ 「真图」缺数据源 ⇒ ⇒ 本批把**样本留住**。
     *
     * ⚠️ **两个数不是同一个数**（裁决 B）：
     *   - `edges`   = **边沿总数**（内核口径，累加，**不受**保留上限影响）；
     *   - `samples` = **保留下来能画图的边沿**（每通道最近 `WAVE_SAMPLE_LIMIT` 条）。
     *
     * @type {Map<string, {id: string, label: string, edges: number, samples: Array<{t:number,v:number}>, lastValue: number|null}>}
     */
    const channels = new Map();

    /**
     * 波形图的**状态**（STAGE-36 · 裁决 H）。
     *
     * ⚠️ 住**视图状态** `wave.chart` ⇒ ⇒ 视图层**纯渲染**（按 `status` 分支）
     * ⇒ ⇒ app 层**写**、视图层**只读** ⇒ ⇒ 分层不破。
     *
     * @param {"idle"|"loading"|"ok"|"degraded"} status
     * @param {string} [reason]
     */
    function setChartState(status, reason) {
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                chart: { status, ...(reason ? { reason } : {}) },
            },
        }));
        publishViewState("waveform");
    }

    /**
     * **懒加载**波形库（裁决 E）。
     *
     * ⚠️ **为什么懒**：echarts 的 ESM 产物约一兆 ⇒ ⇒ 启动就拉 ⇒ ⇒ 首屏白等
     * ⇒ ⇒ 只在**真的有边沿可画**的时候才拉。
     *
     * ⚠️ **失败不抛** ⇒ ⇒ 状态落 `degraded` ⇒ ⇒ 界面显示「波形库不可用」
     * ⇒ ⇒ **通道列表仍在**（裁决 F）⇒ ⇒ 核心调试闭环不受影响。
     */
    function ensureWaveChartLoaded() {
        const status = viewStore.getState("waveform")?.wave?.chart?.status;
        if (status === "ok" || status === "loading") return;
        setChartState("loading");
        loadECharts().then((mod) => {
            setChartState(
                mod ? "ok" : "degraded",
                mod ? undefined : "波形库没加载到（离线或 CDN 不可达）",
            );
        });
    }

    commandRouter.subscribe("waveform.history", (event) => {
        const streams = (event.value?.streams ?? []).filter(
            (s) => !String(s.id ?? "").startsWith(REGISTER_STREAM_PREFIX),
        );
        if (streams.length === 0) return;
        for (const stream of streams) {
            const id = String(stream.id ?? "");
            if (!id) continue;
            const prev = channels.get(id) ?? null;
            // ⚠️ **累加与截断是纯函数**（裁决 C）⇒ ⇒ 只读 `prev`、只写返回值
            // ⇒ ⇒ 视图状态里那份**不被改**。
            const merged = mergeEdges(prev, stream, WAVE_SAMPLE_LIMIT);
            // ⚠️ **「最新值」是通道列表一直在用的字段** ⇒ ⇒ 它**不是**样本里的
            // 东西 ⇒ ⇒ 这里单独从原始流取 ⇒ ⇒ 不动 `mergeEdges` 的契约。
            const raw = Array.isArray(stream.edges) ? stream.edges : [];
            merged.lastValue = raw.length > 0
                ? (raw[raw.length - 1].value ?? prev?.lastValue ?? null)
                : (prev?.lastValue ?? null);
            channels.set(id, merged);
        }
        viewStore.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                channels: [...channels.values()],
            },
        }));
        publishViewState("waveform");
        // ⚠️ **有边沿了才去拉波形库**（懒加载 ⇒ ⇒ 启动不拉那一兆）。
        ensureWaveChartLoaded();
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
        /**
         * 换波形库来源并**重新加载**（STAGE-36 · 裁决 G）。
         *
         * ⚠️ **为什么要有它**：降级路径**必须可达** ⇒ ⇒ 不能只靠「碰巧没网」
         * ⇒ ⇒ 真机门禁**主动**触发降级 ⇒ ⇒ ⇒ **降级逻辑永远有人管**。
         *
         * @param {string} next —— `"off"` = 关掉；给 URL = 换源；给空 = 回到默认。
         * @returns {string} 换之后的来源。
         */
        setChartSource(next) {
            const source = setChartSource(next);
            setChartState("loading");
            loadECharts().then((mod) => {
                setChartState(
                    mod ? "ok" : "degraded",
                    mod ? undefined : "波形库没加载到（离线或 CDN 不可达）",
                );
            });
            return source;
        },
        /** 当前波形库来源。 */
        chartSource() {
            return getChartSource();
        },
        /**
         * 各通道的**计数快照**（脚本口用）。
         *
         * ⚠️ **为什么要有它**（STAGE-36 · 判别力）：真机门禁要拿
         *   「**图上画了几个点**」与「**视图状态里留了几条样本**」比对 ⇒ ⇒
         *   两者必须是**两个独立来源** ⇒ ⇒ ⇒ 只给**数字**、不给样本本身 ⇒ ⇒
         *   门禁读到的是**状态**、不是界面自己的说法。
         *
         * ⚠️ **`edges` 与 `samples` 是两个数**（裁决 B）⇒ ⇒ 两个都报 ⇒ ⇒
         *   「图被截断了」这件事在真机上也看得见。
         *
         * @returns {Array<{id:string,label:string,edges:number,samples:number}>}
         */
        channelStats() {
            return Array.from(channels.values()).map((c) => ({
                id: c.id,
                label: c.label,
                edges: c.edges,
                samples: c.samples.length,
            }));
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
            // ⚠️ **屏幕缓存也要一起清**（STAGE-41 · v0.56）：终端视图那边
            //   有一份**增量**的屏幕实例 ⇒ ⇒ 字节流清了而它没清 ⇒ ⇒
            //   下一个程序的字节会被**接在上一段工程的屏幕后面**⇒ ⇒ 显示错乱。
            resetTerminalScreen();
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
 * Wire the kernel's **board memory map** (contract v6's `MemoryMap` event,
 * **v7 added `loaded` / `revision`**) into the product view's M-14 table and
 * the board-memory-map editor.
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
 * ⚠️ **`revision` 也住在这份缓存里**（v0.31 · 裁决 A）⇒ 提交时原样作为
 * `expected_revision` 带上 ⇒ 并发丢更新**看得见**（`E_PLT-13`）。
 *
 * ⚠️ **主题字面量**：`"memory.map"` 必须与 `front/lib/message.js` 的
 * `EVENT_TOPIC.MemoryMap` 逐字一致（与 `wireConsole` 同一个口径）。
 *
 * ⚠️ **乱序护栏**（STAGE-17 · v0.32 · D-146 · 裁决 F / G）：事件**乱序
 * 到达**时（迟到的旧事件），**更旧**的那份**不许覆盖**较新那份 ⇒ 状态留在
 * 较新那份 ⇒ 差量**算在较新的那张图上**（而不是算在旧图上）。
 *
 * @param {object} commandRouter
 * @returns {{reset: () => void, subscribeMemoryMapStream: () => Promise<object>, get: () => object|null, discardedCount: () => number}}
 */
export function wireMemoryMap(commandRouter) {
    /**
     * **当前生效**的图（`{description, pending, loaded, revision}`）——唯一的那一份。
     *
     * ⚠️ **`revision` 住在这个对象里**（裁决 A · v0.31）：它是内核事件带来的
     * ⇒ 与「当前生效的图」**同一份事实** ⇒ **不另设**一个可以独立翻转的
     * 标志位 ⇒ 提交时原样作为 `expected_revision` 带上。
     *
     * @type {{description: object|null, pending: boolean, loaded: boolean, revision: number}|null}
     */
    let current = null;

    /**
     * **乱序丢弃计数**（STAGE-17 · D-146 · 裁决 F：「丢弃要可观测」）。
     *
     * ⚠️ **只记数、不记内容** ⇒ 它**不是**「当前生效的图」的第二份副本
     * ⇒ 不构成第二个真相源。
     *
     * ⚠️ **只增不减**：`reset()`（新会话）**不**清它 ⇒ 清掉等于把「丢弃
     * 发生过」的证据悄悄抹掉 ⇒ 「不静默」这条承诺就破了。
     *
     * @type {number}
     */
    let discardedOutOfOrder = 0;

    commandRouter.subscribe("memory.map", (event) => {
        const next = {
            description: event.value?.description ?? null,
            pending: event.value?.pending === true,
            loaded: event.value?.loaded === true,
            revision: Number(event.value?.revision ?? 0),
        };
        // ⚠️ **乱序护栏**（裁决 F）：`revision` **严格更旧**的事件 ⇒ **不覆盖**
        // 已有状态 ⇒ 状态留在较新那份 ⇒ 差量**算在较新的那张图上**。
        //
        // ⚠️ **判据是「严格更旧」不是「不等」**（裁决 G）：`UnloadProject`
        // 清订阅 ⇒ 前端重订 ⇒ 内核**立即**推一次当前图 ⇒ 那次推送的
        // `revision` 与手上那份**相同**（卸载**不改图** ⇒ 修订号不 +1）⇒
        // 「不等就挡」会让卸载后的前端**停在旧状态**。
        //
        // ⚠️ **护栏不改提交路径的其它行为**（裁决 H）：基底被判为旧时，提交
        // **照常**发命令 ⇒ 内核的 `expected_revision` 检查**照常兜底**。
        if (current !== null && next.revision < current.revision) {
            discardedOutOfOrder += 1;
            return;
        }
        current = next;
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
        /**
         * **被丢弃的乱序（旧）事件数**（STAGE-17 · D-146 · 裁决 F）。
         *
         * ⚠️ 「不静默」靠**这个数** ⇒ 判定不靠文案、靠**数**。
         *
         * ⚠️ **不**随 `reset()` 清零（见上面那条注释：清掉等于抹掉证据）。
         *
         * @returns {number}
         */
        discardedCount() {
            return discardedOutOfOrder;
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
        delete symbols[String(Number(addr ?? 0) >>> 0)];
        return {
            ...prev,
            debug: { ...(prev.debug ?? {}), bpSymbols: symbols },
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
 * 「**下一次**内核错误显示在哪**一位**」（契约 v9）。
 *
 * ⚠️ **为什么需要它**：错误事件**只带码与消息**，**不带**「该显示在哪一位」
 *   ⇒ ⇒ 发命令时**先记下目标位** ⇒ ⇒ 错误到达时**按位显示**（内核原文，
 *   不吞、不改写）。
 *
 * ⚠️ **注意**：`E_DBG-02`（条件表达式非法）与 `E_DBG-04`（符号名为空）**已经
 *   是两个不同的码**了（整合批拆的 ⇒ ⇒ 「一场景一码」）⇒ ⇒ 但**位**还是
 *   **要记** ⇒ ⇒ 因为**同一个码**也可能从不同的入口来（地址断点 / 符号
 *   候选）⇒ ⇒ 位是**发命令时**决定的 ⇒ ⇒ 不能靠码反推。
 *
 * ⚠️ **它是「下一次」** ⇒ ⇒ 一次错误消费掉 ⇒ ⇒ 不会串位。
 *
 * @type {"cond"|"symbol"|"dump"|null}
 */
let pendingErrorSlot = null;

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
        viewStore.setState("regmem", (prev) => ({
            ...prev,
            regmem: { ...(prev.regmem ?? {}), dumpError: text },
        }));
        publishViewState("regmem");
    }
}

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
        const slot = pendingErrorSlot;
        pendingErrorSlot = null;
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
     * ⚠️ **落点在 `regmem` 视图状态**（规格 §5.2：`regmem.dump`）⇒ ⇒
     *   **不**另存一份 ⇒ ⇒ 「寄存器 / 内存」视图显示的**就是**这一份。
     *
     * ⚠️ **同样在订阅回调里写**（理由同 `debug.symbol`）。
     */
    commandRouter.subscribe("debug.dump", (event) => {
        const addr = Number(event.value?.addr ?? 0) >>> 0;
        const bytes = Array.isArray(event.value?.bytes) ? event.value.bytes : [];
        viewStore.setState("regmem", (prev) => ({
            ...prev,
            regmem: {
                ...(prev.regmem ?? {}),
                dump: { addr, bytes: bytes.map((b) => Number(b) & 0xff) },
            },
        }));
        publishViewState("regmem");
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
                debug: { ...(prev.debug ?? {}), export: null, symbol: null },
            }));
            publishViewState("debug");
            // ⚠️ **转储结果也清掉**（同一个道理 ⇒ ⇒ 它是**上一个程序**的内存）。
            viewStore.setState("regmem", (prev) => ({
                ...prev,
                regmem: { ...(prev.regmem ?? {}), dump: null },
            }));
            publishViewState("regmem");
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
 * 把「**波形库来源**」暴露给页面上的脚本（STAGE-36 · v0.51 批 · 裁决 G）。
 *
 * ⚠️ **这不是「测试后门」**，是**能力协商**：离线环境、或用户不想为一个图
 * 拉一兆字节的时候 ⇒ ⇒ 关掉波形图 ⇒ ⇒ 界面如实显示「波形库不可用」
 * ⇒ ⇒ **通道列表仍在** ⇒ ⇒ 核心调试闭环不受影响。
 *
 * ⚠️ **为什么必须有它**（判别力）：降级路径**必须可达** ⇒ ⇒ 真机门禁
 * **主动**触发降级 ⇒ ⇒ ⇒ **降级逻辑永远有人管** ⇒ ⇒ 不能只靠「碰巧没网」。
 *
 * @param {{setChartSource: (next: string) => string, chartSource: () => string}} waveform
 * @returns {void}
 */
export function exposeWaveChartCommands(waveform) {
    window.riscv_waveform = {
        /** 当前波形库来源（默认是那个 CDN URL）。 */
        chartSource: () => waveform.chartSource(),
        /**
         * 换波形库来源（`"off"` = 关掉波形图；给 URL = 换源；给空 = 回到默认）。
         *
         * @param {string} next
         * @returns {string} 换之后的来源。
         */
        setChartSource: (next) => waveform.setChartSource(next),
        /**
         * 各通道的**计数快照**（`edges` = 边沿总数 / `samples` = 留着的样本数）。
         *
         * ⚠️ **只读、只给数字** ⇒ ⇒ 门禁拿它跟「界面上画了几个点」比对 ⇒ ⇒
         *   两个**独立来源**互相印证 ⇒ ⇒ 界面上报错了就会红。
         *
         * @returns {Array<{id:string,label:string,edges:number,samples:number}>}
         */
        channelStats: () => waveform.channelStats(),
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
    // 板档案（STAGE-15 · v0.30）：建存储实例 + **恢复档案列表**。
    //
    // ⚠️ **不自动重新提交**（裁决 D）：刷新后板**还在**，但**要重新提交**才
    // 生效 ⇒ 自动提交在「已装载镜像」时会变成「挂起待生效」⇒ 用户刷新
    // 一次就多一次「改了板没反应」的困惑 ⇒ 这里**不发任何命令**。
    const board = await createBoardStoreInstance();
    ensureBoardState();
    if (board.store) {
        try {
            const entries = await board.store.list();
            setBoardState({
                archive: Array.isArray(entries) ? entries : [],
                archiveStatus: "ok",
            });
        } catch (err) {
            setBoardState({ archiveStatus: "error" });
            setNote(`板档案读取失败：${err?.message ?? err}`);
        }
    } else {
        // ⚠️ **显式**降级（不静默）：面板上明说「板档案只存在于本会话」。
        setBoardState({ archiveStatus: board.status });
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
