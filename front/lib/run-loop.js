// STAGE-37 · v0.52 批 —— **连续运行**的纯函数。
// STAGE-38 · v0.53 批 —— **「运行中」的判据**与**活值的隐藏口径**（同一模块）。
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
// ## v0.53 批加的两件事（同一个模块，同一条理由）
//
// - **「运行中」的判据**（`isKernelRunning`）：连续模式**或**有在途的
//   `Continue` ⇒ ⇒ 脚本口跑的那**一个**窗口也盖得住（F9 / F10）。
// - **活值的隐藏口径**（`liveValue`）：运行中一律显示**固定符号 `-`**
//   ⇒ ⇒ 三处（调试视图 / 寄存器视图 / 顶栏）用**同一个词** ⇒ ⇒ 不是三份实现。
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
 * 顶栏那个运行按钮的**两种文案**。
 *
 * ⚠️ **为什么要有两种**：用户拍板「**当加载之后顶栏的运行应该显示运行，点击停
 *   之后需要切换为继续（文本切换）**」⇒ ⇒ 那条按钮**同一个动作**（继续往下跑）
 *   但**两种语义**（「从头跑」/「接着跑」）⇒ ⇒ 按钮上必须**说出当下是哪种**⇒ ⇒
 *   用户点之前就知道自己点了会得到什么。
 *
 * ⚠️ **文案只有这一处** ⇒ ⇒ 顶栏按钮上显示的与判据说看到的**天然对得上**⇒ ⇒
 *   不是两处各写一遍。
 *
 * @type {string}
 */
export const RUN_LABEL_RUN = "运行";

/**
 * 顶栏那个运行按钮的**第二种文案**（接着跑）。
 *
 * @type {string}
 */
export const RUN_LABEL_CONTINUE = "继续";

/**
 * 「**这个程序已经跑起来过**吗」（`started`）。
 *
 * ⚠️ **它与 `mode` 是两件事**：`mode` 说「**此刻**前端还想连续跑吗」⇒ ⇒
 *   暂停之后 `mode` 变回 `idle`；而 `started` 说「**这个程序离开过起点吗**」⇒ ⇒
 *   暂停之后它**仍然是真**⇒ ⇒ 顶栏那个按钮因此从「运行」切成「继续」。
 *
 * ⚠️ **它不是「跑了几轮」**（那是 `windows`）⇒ ⇒ 也不说「跑到哪儿了」（那是
 *   PC）⇒ ⇒ 它只回答**「接着跑」还是「从头跑」**这一个问题。
 *
 * @param {{started?: boolean}|undefined} state
 * @returns {boolean}
 */
export function startedOf(state) {
    return state?.started === true;
}

/**
 * 顶栏那个运行按钮**此刻该显示哪句话**（纯函数 ⇒ ⇒ Node 侧零 DOM 就能测）。
 *
 * ⚠️ **判据只有一个**：`started` ⇒ ⇒ 「跑过 ⇒ ⇒ 继续；没跑过 ⇒ ⇒ 运行」⇒ ⇒
 *   与「此刻在不在跑」**无关**（在跑的时候按钮被禁用/不可点 ⇒ ⇒ 不影响）。
 *
 * @param {{started?: boolean}|undefined} state
 * @returns {string} `运行` 或 `继续`
 */
export function runActionLabel(state) {
    return startedOf(state) ? RUN_LABEL_CONTINUE : RUN_LABEL_RUN;
}

/**
 * 初始的运行状态。
 *
 * ⚠️ **`busy` = 在途的 `Continue` 计数**（STAGE-38 · v0.53 批 · 裁决 B）
 * ⇒ ⇒ 初值是 0（没有在途的执行）。
 *
 * ⚠️ **`started` = 还没跑起来过**（新工程 / 复位之后）⇒ ⇒ 顶栏那个按钮显示
 *   「运行」（用户拍板的那条语义）。
 *
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function initialRunState() {
    return { mode: RUN_MODE_IDLE, windows: 0, busy: 0, started: false };
}

/**
 * 记下「**这个程序跑起来过了**」（单步 / 按步数单步 / 连续运行都用它）。
 *
 * ⚠️ **只改 `started`** ⇒ ⇒ 模式、轮数、在途计数**逐字不动** ⇒ ⇒ 这条函数
 *   **不碰**连续运行的语义 ⇒ ⇒ 与 `bumpBusy` 同一手法。
 *
 * @param {{mode?: string, windows?: number, busy?: number, started?: boolean}} [state]
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function markStarted(state) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const mode = prev.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    return { mode, windows, busy: busyOf(prev), started: true };
}

/**
 * 在途计数（**只读**· 读的时候补默认值）。
 *
 * ⚠️ **不是布尔**：它是**个数** ⇒ ⇒ 两个 `Continue` 同时在途就是 2
 * ⇒ ⇒ 一次 `Halted` 只抵一个 ⇒ ⇒ 计数**不会**被提前抵消。
 *
 * @param {{busy?: number}|undefined} state
 * @returns {number}
 */
export function busyOf(state) {
    const raw = state?.busy;
    return Number.isFinite(raw) ? Math.max(0, Number(raw)) : 0;
}

/**
 * 「**内核真的在执行**吗」（STAGE-38 · v0.53 批 · 裁决 A）。
 *
 * ⚠️ **两个来源、但只有一个判据**：
 *
 * - `mode === "running"` ⇒ ⇒ 连续模式**正在跑**（哪怕此刻内核刚好让出
 *   控制权、在两个窗口之间 ⇒ ⇒ 界面**不该**把活值闪出来一下 ⇒ ⇒ 裁决 C）；
 * - `busy > 0` ⇒ ⇒ 有在途的 `Continue`（脚本口 `window.riscv_kernel
 *   .continueRun()` 跑的那**一个**窗口就是这种 ⇒ ⇒ F9 / F10）。
 *
 * ⚠️ **为什么不能只看 `mode`**：脚本口那个窗口**不是**连续模式 ⇒ ⇒
 * 只看 `mode` 的话，脚本跑的时候界面照样闪 ⇒ ⇒ 零判别力。
 *
 * @param {{mode?: string, busy?: number}|undefined} state
 * @returns {boolean}
 */
export function isKernelRunning(state) {
    const mode = state?.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    return mode === RUN_MODE_RUNNING || busyOf(state) > 0;
}

/**
 * 在途计数 ±1（STAGE-38 · v0.53 批 · 裁决 B）。
 *
 * ⚠️ **永远不小于 0** ⇒ ⇒ 「多来一次 `Halted`」不会把计数推成负数
 * ⇒ ⇒ 负数会让 `busy > 0` 这个判据**假假**。
 *
 * ⚠️ **只改 `busy`** ⇒ ⇒ 模式与轮数**逐字不动** ⇒ ⇒ 这条函数**不碰**
 * 连续运行的语义。
 *
 * ⚠️ **`started` 也逐字搬过去** ⇒ ⇒ 「跑过没有」与「在途计数」**互不相干**
 * ⇒ ⇒ 两条护栏各自独立。
 *
 * @param {{mode?: string, windows?: number, busy?: number, started?: boolean}|undefined} state
 * @param {number} delta — 通常是 `+1` 或 `-1`
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function bumpBusy(state, delta) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const mode = prev.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    const step = Number.isFinite(delta) ? Number(delta) : 0;
    return {
        mode,
        windows,
        busy: Math.max(0, busyOf(prev) + step),
        started: startedOf(prev),
    };
}

/**
 * 活值被隐藏时的**固定符号**（STAGE-38 · v0.53 批 · 裁决 D）。
 *
 * ⚠️ **是符号不是文案** ⇒ ⇒ 用户一眼就知道「这里没有值」⇒ ⇒
 * 不是「暂无数据」那种**占位文案**（用户拍定的形态）。
 *
 * @type {string}
 */
export const LIVE_HIDDEN_SYMBOL = "-";

/**
 * 活值的显示口径（STAGE-38 · v0.53 批 · 裁决 D）。
 *
 * ⚠️ **三处（调试视图 / 寄存器视图 / 顶栏）共用这一个函数** ⇒ ⇒
 * 「界面上显示的」与「判据说看到的」**天然对得上** ⇒ ⇒ 不是三份实现。
 *
 * ⚠️ **不是冻结旧值**：运行中返回 `-`（**没有**旧值）⇒ ⇒ 停止后
 * 调同一个函数就**自动**恢复最新值 ⇒ ⇒ 不需要「恢复」这一步。
 *
 * @param {boolean} running — 「内核真的在执行」吗
 * @param {unknown} text — 不运行时要显示的那一份
 * @returns {string}
 */
export function liveValue(running, text) {
    return running === true ? LIVE_HIDDEN_SYMBOL : String(text);
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
 * ⚠️ **`busy` 一律保留**（STAGE-38 · v0.53 批 · 裁决 B）⇒ ⇒ 在途计数
 * **不由**这条函数管（它由「在途 `Continue`」那条线管）⇒ ⇒ 这里**只**搬它。
 *
 * @param {{mode?: string, windows?: number, busy?: number, started?: boolean}} state
 * @param {string|null} reason
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function runStateAfter(state, reason) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const mode = prev.mode === RUN_MODE_RUNNING ? RUN_MODE_RUNNING : RUN_MODE_IDLE;
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    const busy = busyOf(prev);
    const started = startedOf(prev);
    const action = nextRunAction({ mode, reason });
    if (action === RUN_ACTION_CONTINUE) {
        return { mode: RUN_MODE_RUNNING, windows: windows + 1, busy, started };
    }
    if (action === RUN_ACTION_STOP) {
        return { mode: RUN_MODE_IDLE, windows, busy, started };
    }
    return { mode, windows, busy, started };
}

/**
 * 进入连续运行模式。
 *
 * ⚠️ **windows 归零**：这是一趟**新的**连续运行 ⇒ ⇒ 上一趟跑了多少轮
 * 与这一趟无关 ⇒ ⇒ 归零。
 *
 * ⚠️ **busy 也归零**（STAGE-38 · v0.53 批）⇒ ⇒ 新的连续运行**接管**了
 * 在途计数 ⇒ ⇒ 上一趟残留的在途数**不作数**。
 *
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function startRunState() {
    return { mode: RUN_MODE_RUNNING, windows: 0, busy: 0, started: true };
}

/**
 * 退出连续运行模式（**保留** `windows` 与 `busy`）。
 *
 * ⚠️ **`busy` 也保留**（STAGE-38 · v0.53 批 · 裁决 A）⇒ ⇒ 「暂停」只是
 * **停续发**⇒ ⇒ 正在跑的那**一个**窗口还在跑 ⇒ ⇒ 那段时间**仍然**算
 * 「运行中」⇒ ⇒ 活值**不**会提前闪出来。
 *
 * @param {{mode?: string, windows?: number, busy?: number}} [state]
 * @returns {{mode: string, windows: number, busy: number, started: boolean}}
 */
export function stopRunState(state) {
    const prev = state && typeof state === "object" ? state : initialRunState();
    const windows = Number.isFinite(prev.windows) ? Number(prev.windows) : 0;
    return {
        mode: RUN_MODE_IDLE,
        windows,
        busy: busyOf(prev),
        started: startedOf(prev),
    };
}

/**
 * 「**按步数单步**」的步数归一（纯函数 ⇒ ⇒ Node 侧零 DOM 就能测）。
 *
 * ⚠️ **只做「类型归一」，不做「业务上限校验」**：契约里 `StepN { n }` 的 `n`
 *   是 `u32` ⇒ ⇒ 前端能发的**只能**是 `0 … 4294967295` 的整数 ⇒ ⇒ 这一段是
 *   **域归一**（非数 / 负数 / 小数 ⇒ ⇒ 归成 **1**；太大 ⇒ ⇒ 归到 `u32` 上界）
 *   ⇒ ⇒ 与「前端不预校验内核的业务上限」那条规矩**不冲突**（内核那条上限是
 *   `DUMP_MAX_BYTES` 那种**业务**上限 ⇒ ⇒ 那种仍由内核拒）。
 *
 * ⚠️ **0 归成 1 而不是 0**：走 0 条指令 = **什么都不做** ⇒ ⇒ 用户点了按钮却
 *   看不到任何变化 ⇒ ⇒ 那正是「点了没反应」⇒ ⇒ 归成 1（真的走一条）。
 *
 * @param {unknown} raw — 界面输入框里的原始串
 * @returns {number} `1 … 4294967295` 的整数
 */
export function normalizeStepCount(raw) {
    const value = Number(String(raw ?? "").trim());
    if (!Number.isFinite(value)) return 1;
    const truncated = Math.trunc(value);
    if (truncated < 1) return 1;
    return Math.min(truncated, 0xffff_ffff);
}
