// v0.67 批 —— 「**读回**」（read-back）的**纯函数**。
//
// ## 为什么要有这个模块
//
// 用户报了两个现象（在调试视图的「寄存器窗口 / 内存窗口」里）：
//
//   ① 寄存器窗口写着「**变化高亮 = 与上一次读回相比**」，但用户**不知道什么时候
//      会高亮** —— 规则不可预期、时机不透明；
//   ② 填好地址「读内存」之后，**运行再暂停**，内存单元表**不联动刷新**，仍停在
//      运行前的旧值。
//
// ⚠️ **两处根因同源**，都在「**什么时候取回内核的值**」这件事上：
//
//   ① 高亮的基准取自 `sim.gpr` —— 而 `sim.gpr` 是 M-10 **被内核持续推送改写**的
//      **活值**（内核每个 `Continue` 窗口结束推一次寄存器波形）⇒ ⇒ **基准与显示
//      值同源** ⇒ ⇒ 高亮几乎永不出现，且出现时机取决于**内核的推送节奏** ⇒ ⇒
//      **时机不透明**；
//   ② 内核**没有**「内存变化」推送 topic ⇒ ⇒ 内存单元表**只**在用户点「读内存」
//      时才更新 ⇒ ⇒ 程序跑过之后它停在**旧值**上，且**没有任何陈旧提示**。
//
// ## 做法：把「读回」变成一个**显式概念**
//
// - **基准** = **本次运行 / 单步开始前**的寄存器值 ⇒ ⇒ 捕获时机**唯一**、可预期；
// - **读回** = 「把用户正在看的那几样东西**重新从内核取回来**」⇒ ⇒ 触发时机
//   **唯一**：**程序停下时**（断点 / 观察点 / 陷阱 / 用户暂停 / 单步结束）；
// - **高亮** = 当前值 **相对基准** ⇒ ⇒ 「什么时候会高亮」这个问题有了一个**能写
//   在界面上**的答案。
//
// ⚠️ **判定剥成纯函数**（与 `front/lib/run-loop.js` 同一手法）⇒ ⇒ **Node 侧零
// DOM 就能验** ⇒ ⇒ 「该不该读回」这件事不必拖一整条链路才测得到。
//
// ⚠️ **不写第二份停下原因表**：真停原因那四种**住在** `run-loop.js` 的
// `nextRunAction` 判据表里 ⇒ ⇒ 这里**引用**它 ⇒ ⇒ 两边不会漂。
//
// ⚠️ **不许给 `run-loop.js` 加导出**（v0.63 批的白屏纪律 ⇒ ⇒ 前端门禁
//   `debug-view-dedup.test.mjs` 有一条用例钉着「**既有模块的导出集合一个字
//   都没改**」）⇒ ⇒ 加一个导出就会让浏览器缓存里的旧副本**链接不上** ⇒ ⇒
//   **整个页面白屏** ⇒ ⇒ 新能力一律住**新模块**。

import {
    HALT_REASON_BREAKPOINT,
    HALT_REASON_END_OF_EXECUTION,
    HALT_REASON_TRAP,
    HALT_REASON_USER_PAUSE,
    HALT_REASON_WATCHPOINT,
    RUN_ACTION_STOP,
    nextRunAction,
} from "./run-loop.js";

/**
 * 这个停下原因**是不是「真停」**。
 *
 * ⚠️ **不另写一张表** ⇒ ⇒ 直接问 `nextRunAction`（「idle 模式下会不会停」）
 * ⇒ ⇒ 「真停原因」那张表**仍然只有 `run-loop.js` 那一份** ⇒ ⇒ 两边不会漂。
 *
 * @param {string|null|undefined} reason
 * @returns {boolean}
 */
function isTrueStopReason(reason) {
    return nextRunAction({ mode: "idle", reason }) === RUN_ACTION_STOP;
}

/**
 * 读回的**原因**（界面显示用 ⇒ ⇒ 一律小写标签，界面上再翻成中文）。
 *
 * ⚠️ **与内核的 `HaltReason` 不是一回事** —— 内核那套说的是「**为什么停**」，
 * 这里这套说的是「**为什么读回**」⇒ ⇒ 「手动读回」在��核那边**没有**对应值。
 *
 * @type {Readonly<Record<string, string>>}
 */
export const READBACK_REASON = Object.freeze({
    /** 基准捕获（运行 / 单步开始前）—— 不是一次读回。 */
    RUN_START: "run-start",
    /** 命中断点。 */
    BREAKPOINT: "breakpoint",
    /** 命中观察点。 */
    WATCHPOINT: "watchpoint",
    /** 陷入。 */
    TRAP: "trap",
    /** 用户点了暂停。 */
    USER_PAUSE: "user-pause",
    /** 单步走完。 */
    STEP: "step",
    /** 用户手动点了「读寄存器」。 */
    MANUAL: "manual",
});

/**
 * 读回原因的**界面短词**（中文）。
 *
 * ⚠️ **只有一处** ⇒ ⇒ 视图层与门禁读**同一份** ⇒ ⇒ 不会出现两处措辞不一致。
 *
 * @type {Readonly<Record<string, string>>}
 */
export const READBACK_REASON_LABEL = Object.freeze({
    [READBACK_REASON.BREAKPOINT]: "断点",
    [READBACK_REASON.WATCHPOINT]: "观察点",
    [READBACK_REASON.TRAP]: "陷阱",
    [READBACK_REASON.USER_PAUSE]: "暂停",
    [READBACK_REASON.STEP]: "单步",
    [READBACK_REASON.MANUAL]: "手动",
});

/**
 * 抓一份**基准**（拷贝，不引用活值）。
 *
 * ⚠️ **必须拷贝** —— 基准是「跑之前」的那一份 ⇒ ⇒ 引用活值的话，跑完之后
 * 基准也跟着变了 ⇒ ⇒ 高亮**恒空**（这正是 v0.66 之前那个缺陷的形态）。
 *
 * ⚠️ **长度不足就补零** —— 基准与当前值**逐位比** ⇒ ⇒ 长度不一致时**多出来的
 * 那些位**按 0 参与比较（不是「跳过」⇒ ⇒ 那会让高位的变化**看不见**）。
 *
 * @param {unknown} gpr — 当前寄存器值（数组）
 * @returns {number[]|null} 基准（拷贝）或 `null`（没有可抓的）
 */
export function captureBaseline(gpr) {
    if (!Array.isArray(gpr)) return null;
    return gpr.map((v) => Number(v ?? 0) >>> 0);
}

/**
 * 哪些寄存器**变了**（相对基准）。
 *
 * ⚠️ **逐位比**（不是「比长度」）⇒ ⇒ 基准比当前值长时，多出来的那些位也算
 * 「变了」⇒ ⇒ 不会因为长度不齐而漏报。
 *
 * @param {number[]|null|undefined} baseline
 * @param {number[]|null|undefined} current
 * @returns {number[]} 变了的下标（升序）
 */
export function diffRegisters(baseline, current) {
    if (!Array.isArray(baseline) || !Array.isArray(current)) return [];
    const changed = [];
    const len = Math.max(baseline.length, current.length);
    for (let i = 0; i < len; i += 1) {
        const now = Number(current[i] ?? 0) >>> 0;
        const before = Number(baseline[i] ?? 0) >>> 0;
        if (now !== before) changed.push(i);
    }
    return changed;
}

/**
 * 内核的**停下原因** ⇒ ⇒ 读回的**原因**。
 *
 * ⚠️ **不认识的 / 没有的 ⇒ ⇒ `null`** ⇒ ⇒ **不猜**（与 `nextRunAction` 同一
 * 纪律：不知道就不动）。
 *
 * @param {string|null|undefined} haltReason — 内核的 `HaltReason` 线上标签
 * @returns {string|null}
 */
export function readbackReasonOf(haltReason) {
    switch (haltReason) {
        case HALT_REASON_BREAKPOINT:
            return READBACK_REASON.BREAKPOINT;
        case "StepOver":
            // stepOver 的返回地址停点不是用户断点，读回原因按「单步」标。
            return READBACK_REASON.STEP;
        case HALT_REASON_WATCHPOINT:
            return READBACK_REASON.WATCHPOINT;
        case HALT_REASON_TRAP:
            return READBACK_REASON.TRAP;
        case HALT_REASON_USER_PAUSE:
            return READBACK_REASON.USER_PAUSE;
        default:
            return null;
    }
}

/**
 * 该不该在**这次停下之后**自动读回？
 *
 * ⚠️ **判据表**（与 `nextRunAction` 那张表同源，但**不是同一张**）：
 *
 * | 内核的停下原因 | 自动读回？ | 为什么 |
 * | --- | --- | --- |
 * | `Breakpoint` / `Watchpoint` / `Trap` / `UserPause` | **是** | **真停** ⇒ ⇒ 程序真的停在那儿 ⇒ ⇒ 值该取回来 |
 * | `EndOfExecution` + **用户刚点过暂停** | **是** | 暂停**只停续发**（STAGE-38 裁决 O）⇒ ⇒ 正在跑的那**一个**窗口跑完才停 ⇒ ⇒ 那次停下**就是**用户要的那次停 |
 * | `EndOfExecution`（**没有**暂停在等） | **否** | 连续运行里**每个窗口**都会来一次 ⇒ ⇒ 每次都读回 ⇒ ⇒ 真机门禁的脚本口会**慢到不可用** ⇒ ⇒ **必须躲开** |
 * | 其它 / 未知 / `null` | **否** | **不猜** |
 *
 * ⚠️ **第二行是最容易写成「空转」的一格**：没有「暂停在等」这个前提时，
 * `EndOfExecution` **必须**返回 `false` ⇒ ⇒ 否则「永远读回」的实现也会绿
 * ⇒ ⇒ **防空转**（判别力清单第 1 条）。
 *
 * @param {{haltReason?: string|null, pausePending?: boolean}} input
 * @returns {boolean}
 */
export function shouldAutoReadback(input) {
    const reason = typeof input?.haltReason === "string" ? input.haltReason : null;
    if (reason === null) return false;
    if (reason === HALT_REASON_END_OF_EXECUTION) {
        return input?.pausePending === true;
    }
    return isTrueStopReason(reason);
}

/**
 * 视图状态里那一份「读回」的**形状**（默认值）。
 *
 * ⚠️ **派生量不另存** ⇒ ⇒ `changed`（变了几个）**不在这里** ⇒ ⇒ 视图层拿
 * 基准与当前值**现算** ⇒ ⇒ 状态与显示**不可能**对不上。
 *
 * @returns {{baseline: number[]|null, baselineAt: number|null, reason: string|null, auto: boolean, at: number|null, error: string|null}}
 */
export function initialReadbackState() {
    return {
        baseline: null,
        baselineAt: null,
        reason: null,
        auto: false,
        at: null,
        error: null,
    };
}

/**
 * 把一份读回状态**并进**已有那份（只改该改的字段）。
 *
 * ⚠️ **基准不会被「读回」清掉** ⇒ ⇒ 用户点「读寄存器」**不会**把正在查看的
 * 对比关系抹掉 ⇒ ⇒ 那正是「读回 ≠ 重新取基准」的语义。
 *
 * @param {object|null|undefined} prev — 已有那一份
 * @param {object} patch — 要改的字段
 * @returns {object} 新的那一份
 */
export function mergeReadbackState(prev, patch) {
    const base = prev && typeof prev === "object" ? prev : initialReadbackState();
    return {
        ...base,
        ...(patch && typeof patch === "object" ? patch : {}),
    };
}
