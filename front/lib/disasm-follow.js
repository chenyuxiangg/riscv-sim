// STAGE-45 · v0.69 批 —— 「**反汇编跟随 / 陈旧标注**」的**纯函数**。
//
// ## 为什么要有这个模块
//
// 台账 **D-176**：调试视图的「反汇编」窗口在**程序跑过之后**仍是**旧 PC**
// 那一段 ⇒ ⇒ 界面**在骗人**（用户看不出来它已经过期）。
//
// ⚠️ **根因链**（逐条读源码，不是推断）：
//
//   ① 前端**只有一处**发 `Disasm`（`front/app.js` 的 `case "disasm"`）⇒ ⇒
//      而它**只**由反汇编卡那个按钮触发 ⇒ ⇒ 程序跑过之后**没有人**再请求
//      ⇒ ⇒ 表里那份是**请求那一刻**的 PC 那一段；
//   ② 契约里**没有**「PC 变了 ⇒ 推一份新反汇编」这种事件 ⇒ ⇒ 前端**只能**
//      自己再发一条命令 ⇒ ⇒ **不是**内核的锅；
//   ③ 它与 v0.67 批修掉的「寄存器 / 内存两处陈旧」**不是同一件事** ——
//      读回是「把一个**已知**的值取回来」（寄存器 / 内存单元 / 转储都**有
//      地址**）⇒ ⇒ 而反汇编是「按**新的** PC **重新求一段**」⇒ ⇒ 读回
//      **解决不了**它。
//
// ## 做法：C（不骗人）+ B（可自动跟随）
//
// - **C** —— 把「这段是哪个 PC 时的」「当前 PC 是多少」**如实标出来** ⇒ ⇒
//   用户**看得见**它过期了；
// - **B** —— 加一个「**跟随 PC**」开关（**默认关**）⇒ ⇒ 打开时**停下后**
//   按**新 PC** 自动重取一段 ⇒ ⇒ **不顶掉**用户手动选的那段（默认关 ⇒ ⇒
//   它本来就不会顶）。
//
// ⚠️ **两个量都**不是**前端另存的**：
//
// - **段起点** = `entries[0].addr` ⇒ ⇒ **内核给的**（内核 `disasm_core` 的
//   第一条 `entry.addr` 就是请求的地址）⇒ ⇒ 前端**不**记住「我请求了哪个
//   地址」⇒ ⇒ **契约层零升版**（`Disasm` 事件**不必**带 `addr`）；
// - **当前 PC** = M-10 推的活值 ⇒ ⇒ 与顶栏那份**同一个源**。
//
// ⇒ ⇒ 「这段是旧 PC 那段」这件事**由数据本身回答** ⇒ ⇒ 前端**不猜**。
//
// ⚠️ **判定剥成纯函数**（与 `front/lib/readback.js` / `front/lib/run-loop.js`
//   同一手法）⇒ ⇒ **Node 侧零 DOM 就能验** ⇒ ⇒ 「该不该跟随」「陈不陈旧」
//   这两件事不必拖一整条链路才测得到。
//
// ⚠️ **新能力一律住新模块**（v0.63 批的白屏纪律 ⇒ ⇒ 前端门禁
//   `debug-view-dedup.test.mjs` 有一条用例钉着「**既有模块的导出集合一个
//   字都没改**」）⇒ ⇒ 加一个导出就会让浏览器缓存里的旧副本**链接不上**
//   ⇒ ⇒ **整个页面白屏** ⇒ ⇒ 这里**新开**一个模块。

import { READBACK_REASON } from "./readback.js";

/**
 * 「跟随 PC」的**默认值**。
 *
 * ⚠️ **默认关** ⇒ ⇒ 用户**不主动要求**就不动他手动选的那一段 ⇒ ⇒
 * 「用户选的那段被顶掉」这个副作用**默认不会发生**。
 *
 * @type {boolean}
 */
export const FOLLOW_PC_DEFAULT = false;

/**
 * **会触发跟随**的那些「停下」原因。
 *
 * ⚠️ **白名单，不是黑名单** ⇒ ⇒ 不知道的一律**不跟随**（与 `run-loop.js`
 *   的「不猜」纪律同形）⇒ ⇒ 将来 `performReadback` 多了一个新 reason ⇒ ⇒
 *   **默认不跟随** ⇒ ⇒ 不会**默默地**开始顶掉用户那一段。
 *
 * ⚠️ **手动读回不在里面** ⇒ ⇒ 用户点「读寄存器」**不是**「程序停下了」⇒ ⇒
 *   那时**不该**重取反汇编。
 *
 * @type {ReadonlySet<string>}
 */
export const FOLLOWABLE_REASONS = Object.freeze(new Set([
    READBACK_REASON.BREAKPOINT,
    READBACK_REASON.WATCHPOINT,
    READBACK_REASON.TRAP,
    READBACK_REASON.USER_PAUSE,
    READBACK_REASON.STEP,
]));

/**
 * **八位十六进制**（界面上的地址一律这个形态）。
 *
 * @param {number} addr
 * @returns {string}
 */
function hex8(addr) {
    return (Number(addr ?? 0) >>> 0).toString(16).padStart(8, "0");
}

/**
 * 这一段反汇编的**起点地址**（**推导量** ⇒ ⇒ 不另存）。
 *
 * ⚠️ **取第一条的地址** —— 内核 `disasm_core` 从请求地址起逐条解码 ⇒ ⇒
 *   第一条的 `addr` **就是**请求的那个地址 ⇒ ⇒ 「这段是哪一段」**由数据
 *   自己说**。
 *
 * ⚠️ **空 ⇒ ⇒ `null`** —— 内核取指失败时返回的是**空序列**（不是错误）⇒ ⇒
 *   那时**没有**「这一段」⇒ ⇒ **不猜**一个起点。
 *
 * @param {unknown} entries — 内核给的反汇编条目
 * @returns {number|null} 起点地址（或 `null`）
 */
export function segmentBaseAddr(entries) {
    if (!Array.isArray(entries) || entries.length === 0) return null;
    const addr = Number(entries[0]?.addr);
    if (!Number.isFinite(addr)) return null;
    return addr >>> 0;
}

/**
 * 这一段**陈旧**了吗（段起点 ≠ 当前 PC）。
 *
 * ⚠️ **「有内容」是前提** ⇒ ⇒ 空表**不算**陈旧 ⇒ ⇒ 「没有东西」与「有过期
 * 的东西」**分得开**。
 *
 * ⚠️ **当前 PC 拿不到 ⇒ ⇒ 不算陈旧** ⇒ ⇒ **不猜**（那会把「读不到」说成
 * 「过期」）。
 *
 * @param {unknown} entries — 内核给的反汇编条目
 * @param {unknown} pc — 当前 PC（M-10 推的活值）
 * @returns {boolean}
 */
export function isStaleSegment(entries, pc) {
    const base = segmentBaseAddr(entries);
    if (base === null) return false;
    // ⚠️ **`null` / `undefined` 要单独挡** ⇒ ⇒ `Number(null)` 是 **0**
    //   （不是 `NaN`）⇒ ⇒ 不挡的话「读不到当前 PC」会被当成「PC = 0」⇒ ⇒
    //   于是「读不到」被说成「过期」⇒ ⇒ **在骗人**。
    if (pc === null || pc === undefined) return false;
    const current = Number(pc);
    if (!Number.isFinite(current)) return false;
    return base !== (current >>> 0);
}

/**
 * 该不该在**这次读回之后**按新 PC 重取一段反汇编？
 *
 * ⚠️ **两个条件都要满足**：
 *   ① 用户**打开了**「跟随 PC」（`followPc === true`）⇒ ⇒ 默认关 ⇒ ⇒
 *      **平时零开销**；
 *   ② 这次读回**是**一次「停下」⇒ ⇒ 手动读回**不算**。
 *
 * ⚠️ **判据在纯函数里** ⇒ ⇒ Node 侧**零 DOM** 就能验 ⇒ ⇒ 这里只是一次
 *   **调用**。
 *
 * @param {{followPc?: boolean, reason?: string|null}} input
 * @returns {boolean}
 */
export function shouldFollowDisasm(input) {
    if (input?.followPc !== true) return false;
    const reason = typeof input?.reason === "string" ? input.reason : null;
    if (reason === null) return false;
    return FOLLOWABLE_REASONS.has(reason);
}

/**
 * 反汇编状态行（**做法 C** 的文案）。
 *
 * ⚠️ **三态分得开**：
 *   - **无内容** ⇒ ⇒ 「尚无反汇编」；
 *   - **有内容且段起点 == 当前 PC** ⇒ ⇒ 「这一段就是当前 PC 那段」；
 *   - **有内容且段起点 ≠ 当前 PC** ⇒ ⇒ 「**已陈旧**」⇒ ⇒ **不骗人**。
 *
 * ⚠️ **「陈旧」两个字只出现在第三态** ⇒ ⇒ 判据可以**逐字**查（不许用
 *   「包含某个模糊词」那种判据）。
 *
 * @param {{entries?: unknown, pc?: unknown, followPc?: boolean}} input
 * @returns {{text: string, stale: boolean, base: number|null, count: number}}
 */
export function disasmStatusText(input) {
    const entries = Array.isArray(input?.entries) ? input.entries : [];
    const base = segmentBaseAddr(entries);
    const pc = Number(input?.pc ?? 0) >>> 0;
    if (base === null) {
        return {
            text: "尚无反汇编 —— 点上面的「反汇编」向内核请求一段（M-11）。",
            stale: false,
            base: null,
            count: 0,
        };
    }
    const count = entries.length;
    const stale = isStaleSegment(entries, pc);
    const head = `这段是 0x${hex8(base)} 起的 ${count} 条`;
    const cur = `当前 PC 0x${hex8(pc)}`;
    if (!stale) {
        return { text: `${head} · ${cur}`, stale: false, base, count };
    }
    const hint = input?.followPc === true
        ? ""
        : " · 勾上「跟随 PC」可自动跟上";
    return {
        text: `${head} · ${cur} —— **已陈旧**（程序已经跑到别处去了）${hint}`,
        stale: true,
        base,
        count,
    };
}

/**
 * 「跟随 PC」复选框的**标签**（**状态 + 点了会发生什么**）。
 *
 * ⚠️ **两个都写** ⇒ ⇒ 用户不用猜「勾上会发生什么」⇒ ⇒ 与 v0.50 批那个
 *   trace 开关**同一手法**。
 *
 * @param {{followPc?: boolean}} input
 * @returns {string}
 */
export function followPcLabel(input) {
    return input?.followPc === true
        ? "跟随 PC：已开启（停下后自动重取一段 · 点击关闭）"
        : "跟随 PC：已关闭（停下后不自动重取 · 点击开启）";
}
