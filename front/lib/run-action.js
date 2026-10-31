// 「**跑过没有**」那一格 ＋ 顶栏那个按钮的「**运行 / 继续**」文案
// （v0.63 批 · 调试视图去重批）。
//
// ## 为什么**另起一个模块**（本批最贵的一条教训）
//
// ⚠️ **既有模块的导出集合一个字都不许改** —— ES module 的**链接期**是
//   **全有或全无**的 ⇒ ⇒ 「**新的** `app.js`」+「**浏览器缓存里的旧**
//   `run-loop.js`」⇒ ⇒ 只要 `app.js` 从旧文件里 import 了一个**旧文件没有
//   的名字** ⇒ ⇒ **整个 `app.js` 都不执行** ⇒ ⇒ 页面**只剩静态 HTML**
//   ⇒ ⇒ 状态停在 `index.html` 里写死的「未连接」⇒ ⇒ 用户看到的是
//   **白屏 + 未连接**，而真相是「**脚本根本没启动**」。
//
// ⇒ ⇒ 所以**新能力一律放新模块** ⇒ ⇒ 缓存里那份旧 `run-loop.js` **照样能
//   链接** ⇒ ⇒ 最坏情况只是「**新功能没有**」（**降级**）⇒ ⇒ **不会白屏**。
//
// ⚠️ **本模块是纯函数**（零 DOM）⇒ ⇒ Node 侧就能测 ⇒ ⇒ 与
//   `front/lib/run-loop.js` 同一手法。
//
// ⚠️ **「跑过没有」这一格由 app 层自己搬**（`front/app.js` 的
//   `transitionRunState`）⇒ ⇒ `run-loop.js` 那几个纯函数只搬
//   `mode` / `windows` / `busy` ⇒ ⇒ 它们**一个字都不用改** ⇒ ⇒ 旧缓存
//   那份**逐字可用**。

/**
 * 顶栏那个运行按钮的**两种文案**。
 *
 * ⚠️ **为什么有两种**：用户拍板「**当加载之后顶栏的运行应该显示运行，点击停
 *   之后需要切换为继续（文本切换）**」⇒ ⇒ 那条按钮**同一个动作**（继续往下
 *   跑）但**两种语义**（「从头跑」/「接着跑」）⇒ ⇒ 按钮上必须**说出当下是
 *   哪种**⇒ ⇒ 用户点之前就知道自己点了会得到什么。
 *
 * ⚠️ **文案只有这一处** ⇒ ⇒ 顶栏按钮上显示的与判据说看到的**天然对得上**
 *   ⇒ ⇒ 不是两处各写一遍。
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
 *   暂停之后 `mode` 变回 `idle`；而 `started` 说「**这个程序离开过起点吗**」
 *   ⇒ ⇒ 暂停之后它**仍然是真**⇒ ⇒ 顶栏那个按钮因此从「运行」切成「继续」。
 *
 * ⚠️ **它不是「跑了几轮」**（那是 `windows`）⇒ ⇒ 也不说「跑到哪儿了」
 *   （那是 PC）⇒ ⇒ 它只回答「**接着跑**还是**从头跑**」这一个问题。
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
 *   与「此刻在不在跑」**无关**（在跑的时候那个按钮不该被点 ⇒ ⇒ 不影响）。
 *
 * @param {{started?: boolean}|undefined} state
 * @returns {string} `运行` 或 `继续`
 */
export function runActionLabel(state) {
    return startedOf(state) ? RUN_LABEL_CONTINUE : RUN_LABEL_RUN;
}

/**
 * 记下「**这个程序跑起来过了**」（单步 / 按步数单步 / 连续运行都用它）。
 *
 * ⚠️ **只加那一格** ⇒ ⇒ `mode` / `windows` / `busy` **逐字不动** ⇒ ⇒ 这条
 *   函数**不碰**连续运行的语义 ⇒ ⇒ 与 `bumpBusy` 同一手法。
 *
 * ⚠️ **它**不**自己调 `stopRunState`** —— 那个函数只搬三格 ⇒ ⇒ 调它会把
 *   `started` 弄丢 ⇒ ⇒ 「先停续发、再记跑过」这两步**由 app 层按顺序做**
 *   （`markStarted(stopRunState(...))`）⇒ ⇒ 顺序是 app 层看得见的。
 *
 * @param {{mode?: string, windows?: number, busy?: number, started?: boolean}} [state]
 * @returns {{mode?: string, windows?: number, busy?: number, started: boolean}}
 */
export function markStarted(state) {
    return { ...(state ?? {}), started: true };
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
