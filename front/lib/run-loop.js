// STAGE-37 · v0.52 批 —— **连续运行**的纯函数。
//
// ## 为什么要有这个模块
//
// 「点一次运行 ⇒ 连续跑到断点」这件事，**判定**与**执行**必须分开：
//
// - **判定**（本模块）：拿到内核的一次「停下报告」，问一句「接下来该干什么」
//   ⇒ ⇒ **纯函数** ⇒ ⇒ Node 侧**零 DOM** 就能测 ⇒ ⇒ 与 STAGE-31 批的
//   「差量合成剥成纯函数」同一手法。
// - **执行**（`front/app.js`）：按判定结果发命令、写视图状态。
//
// ⚠️ **为什么不写在 app 层直接判**：那样判据就得靠「整条链路跑一遍」才测得到
// ⇒ ⇒ 一旦中间某个环节坏了，判据**分不清**是哪一环坏的 ⇒ ⇒ 本模块让
// **四种停下原因 × 两种模式**的组合**逐一**可验。
//
// ## 两个数不是同一个数（裁决 B / F5 同形）
//
// - `windows` = 前端**已经发出去多少轮** `Continue`；
// - 内核那边「一个窗口」= 10 000 条指令 = 10 µs 虚拟时间（**10 个量子**）。
//
// ⇒ ⇒ 「跑了几轮」是**前端的账**，内核不知道 ⇒ ⇒ 不显示就不编造。

/**
 * 运行模式：`idle` = 没在连续跑；`running` = 正在连续跑。
 *
 * @type {string}
 */
export const RUN_MODE_IDLE = "idle";

/**
 * 运行模式：正在连续跑。
 *
 * @type {string}
 */
export const RUN_MODE_RUNNING = "running";

/**
 * 判定结果：**续发**一条 `Continue`。
 *
 * ⚠️ 这不是「停下」——它是内核**让出控制权**（跑满一个指令预算窗口）后，
 * 前端**接着**跑下一轮。
 *
 * @type {string}
 */
export const RUN_ACTION_CONTINUE = "continue";

/**
 * 判定结果：**真停**了（断点 / 观察点 / 陷阱 / 用户暂停）。
 *
 * @type {string}
 */
export const RUN_ACTION_STOP = "stop";

/**
 * 判定结果：**什么都不做**。
 *
 * ⚠️ 什么时候是它：① 压根没在连续跑（`mode === "idle"`）⇒ ⇒ 内核让出控制权
 * 是**正常的**，不是「该续跑」；② 停下原因**不认识** ⇒ ⇒ **不猜**。
 *
 * @type {string}
 */
export const RUN_ACTION_NONE = "none";

/**
 * 内核的停下原因：**跑满一个指令预算窗口**（让出控制权）。
 *
 * ⚠️ **契约层用它兼表两件事**（「程序真的跑完了」与「只是跑满了一个窗口」）
 * ⇒ ⇒ 前端**分不清** ⇒ ⇒ 本批把「分不清」这件事**显式化**：连续模式里
 * 它 = **续发**，非连续模式里它 = **什么都不做**（裁决 D）。
 *
 * @type {string}
 */
export const HALT_REASON_END_OF_EXECUTION = "EndOfExecution";

/**
 * 内核的停下原因：命中断点。
 *
 * @type {string}
 */
export const HALT_REASON_BREAKPOINT = "Breakpoint";

/**
 * 内核的停下原因：命中观察点。
 *
 * @type {string}
 */
export const HALT_REASON_WATCHPOINT = "Watchpoint";

/**
 * 内核的停下原因：陷阱。
 *
 * @type {string}
 */
export const HALT_REASON_TRAP = "Trap";

/**
 * 内核的停下原因：用户暂停。
 *
 * @type {string}
 */
export const HALT_REASON_USER_PAUSE = "UserPause";

/**
 * **真停**的四种原因（裁决 D）。
 *
 * ⚠️ 这四种都会让连续运行**结束** ⇒ ⇒ 前端**必须**逐个认得 ⇒ ⇒ 少认一种
 * ⇒ ⇒ 连续运行在那种停下上**停不下来**。
 */
const STOP_REASONS = Object.freeze(new Set([
    HALT_REASON_BREAKPOINT,
    HALT_REASON_WATCHPOINT,
    HALT_REASON_TRAP,
    HALT_REASON_USER_PAUSE,
]));

/**
 * 初始的运行状态。
 *
 * @returns {{mode: string, windows: number}}
 */
export function initialRunState() {
    return { mode: RUN_MODE_IDLE, windows: 0 };
}

/**
 * 拿到一次「停下报告」之后，问一句「接下来该干什么」。
 *
 * ⚠️ **判据表**（裁决 D）—— 四种真停原因 × 两种模式，**逐一**可验：
 *
 * | `mode` | `reason` | 结果 |
 * | --- | --- | --- |
 * | `running` | `EndOfExecution` | `continue` |
 * | `idle` | `EndOfExecution` | `none` |
 * | 任意 | `Breakpoint` / `Watchpoint` / `Trap` / `UserPause` | `stop` |
 * | 任意 | 其它 / 未知 / `null` | `none` |
 *
 * ⚠️ **第二行是最容易写成「空转」的一格**：`idle` 模式下收到
 * `EndOfExecution` 必须返回 `none` ⇒ ⇒ 否则「永远续发」的实现也会绿
 * ⇒ ⇒ **防空转**（判别力清单第 1 条）。
 *
 * ⚠️ **第四行是「不猜」**：不认识的原因 ⇒ ⇒ **不续也不停** ⇒ ⇒ 宁可不动，
 * 也不要按错误的语义往下跑。
 *
 * @param {{mode?: string, reason?: string|null}} input
 * @returns {string} `continue` / `stop` / `none`
 */
export function nextRunAction(input) {
    const mode = input?.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const reason = typeof input?.reason === "string" ? input.reason : null;
    if (reason === HALT_REASON_END_OF_EXECUTION) {
        return mode === RUN_MODE_RUNNING ? RUN_ACTION_CONTINUE : RUN_ACTION_NONE;
    }
    if (reason !== null && STOP_REASONS.has(reason)) {
        return RUN_ACTION_STOP;
    }
    return RUN_ACTION_NONE;
}

/**
 * 把一次「停下报告」落到**运行状态**上。
 *
 * ⚠️ **`stop` 时 `windows` 保留**（不归零）⇒ ⇒ 用户看得见「这一趟连续跑了
 * 多少轮才停」⇒ ⇒ 归零的话那个数**每次停下都被抹掉** ⇒ ⇒ 白记。
 *
 * ⚠️ **`none` 时整个状态原样返回** ⇒ ⇒ 「不认识就不动」在状态上也是
 * **真的不动**（不是「动了但看起来一样」）。
 *
 * @param {{mode?: string, windows?: number}} state
 * @param {string|null} reason
 * @returns {{mode: string, windows: number}}
 */
export function runStateAfter(state, reason) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const mode = prev.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    const action = nextRunAction({ mode, reason });
    if (action === RUN_ACTION_CONTINUE) {
        return { mode: RUN_MODE_RUNNING, windows: windows + 1 };
    }
    if (action === RUN_ACTION_STOP) {
        return { mode: RUN_MODE_IDLE, windows };
    }
    return { mode, windows };
}

/**
 * 进入连续运行模式。
 *
 * ⚠️ **windows 归零**：这是一趟**新的**连续运行 ⇒ ⇒ 上一趟跑了多少轮
 * 与这一趟无关 ⇒ ⇒ 归零。
 *
 * @returns {{mode: string, windows: number}}
 */
export function startRunState() {
    return { mode: RUN_MODE_RUNNING, windows: 0 };
}

/**
 * 退出连续运行模式（**保留** `windows`）。
 *
 * @param {{mode?: string, windows?: number}} [state]
 * @returns {{mode: string, windows: number}}
 */
export function stopRunState(state) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    return { mode: RUN_MODE_IDLE, windows };
}
