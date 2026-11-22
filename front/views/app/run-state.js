// App 层 · 连续运行状态、寄存器基准 / 读回与顶栏运行条同步（从 app.js 下沉）。
//
// ⚠️ 单向依赖：视图状态（M-03）、公共视图派发辅助、debug-state 基础层与
// 纯函数 lib；不回引门面 ⇒ ⇒ 不形成子模块 ↔ 门面循环。
//
// ⚠️ 顶栏按钮的命令下发**不经本模块直接调门面 `handleCommand`**：
// `wireTopbarRunbar(services, dispatch)` 的 `dispatch` 由门面 bootstrap 注入
// （`(detail) => handleCommand(detail, services)`）⇒ ⇒ 子模块只持有回调、
// 不持有门面引用。

import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";
import { updateDebugState } from "./debug-state.js";
import { Command } from "../../lib/message.js";
import {
    RUN_MODE_IDLE,
    RUN_MODE_RUNNING,
    bumpBusy,
    isKernelRunning,
    liveValue,
} from "../../lib/run-loop.js";
import { runActionLabel } from "../../lib/run-action.js";
import { captureBaseline, mergeReadbackState } from "../../lib/readback.js";
import { shouldFollowDisasm } from "../../lib/disasm-follow.js";

/** How many instructions a disassembly request asks for by default. */
const DEFAULT_DISASM_COUNT = 16;

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
export function captureGprBaseline(services) {
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
export async function performReadback(services, reason, options = {}) {
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
 *   再要一条退休路径 ⇒ ⇒ 两个真相）⇒ ⇒ 退休点**只有一处**（`halted` 订阅）。
 *
 * @param {number} delta — `+1` / `-1`
 * @returns {{mode: string, windows: number, busy: number}}
 */
export function bumpRunBusy(delta) {
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
export function transitionRunState(next, prev = getRunState()) {
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
        ? `运行中 · ${run.windows} 轮`
        : `已停止 · ${run.windows} 轮`;
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
 *   那类判据会被**重复触发**带偏 ⇒ ⇒ 所以**先退旧的、再挂新的** ⇒ ⇒ 恰好一个。
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
 * ⚠️ **三个按钮直接走注入的 `dispatch`** ⇒ ⇒ 与调试视图那份**共用同一条
 * 命令处理路径** ⇒ ⇒ **不新增**命令处理分支 ⇒ ⇒ 一条路径。`dispatch` 由门面
 * bootstrap 注入，本子模块不回引门面。
 *
 * ⚠️ **状态订阅按 services 重新订**：每次 bootstrap 的 `services` 都是**新
 * 对象** ⇒ ⇒ 若用「只订一次」的标志 ⇒ ⇒ 新 services **收不到**更新 ⇒ ⇒
 * 顶栏数字会**冻住** ⇒ ⇒ 所以改成「先退订旧的、再订新的」⇒ ⇒ **恰好一个**
 * 订阅。
 *
 * @param {object} services
 * @param {(detail: object) => void} [dispatch] — 门面注入的命令分发回调
 */
export function wireTopbarRunbar(services, dispatch) {
    const bar = document.querySelector("[data-topbar-runbar]");
    if (!bar) return;
    const emit = dispatch ?? (() => {});
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
        emit({ action: "continue" });
    });
    bind('[data-topbar-action="step"]', () => {
        emit({ action: "step" });
    });
    bind('[data-topbar-action="step-over"]', () => {
        emit({ action: "step-over" });
    });
    bind('[data-topbar-action="reset"]', () => {
        emit({ action: "reset" });
    });
    // ⚠️ **「暂停」按钮**（STAGE-37 · v0.52 批 · 裁决 G）⇒ ⇒ 与调试视图
    // 那份**共用同一个 action** ⇒ ⇒ **一条路径** ⇒ ⇒ 不写第二套。
    //
    // ⚠️ **v0.63 批**：调试视图里那份**撤掉**了 ⇒ ⇒ 暂停的**唯一**入口是这里
    //   ⇒ ⇒ 界面**不再有**同一个动作的第二个入口。
    bind('[data-topbar-action="pause"]', () => {
        emit({ action: "pause" });
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
