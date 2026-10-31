// **trace 记录开关**的纯函数（状态推导 + 下一个值）。
//
// ## 为什么要有这个模块
//
// 「这个开关现在是开还是关」「点一下该发什么」必须**只有一处**说了算：
//
// - **视图层**（`front/views/m02_viewcomponents.js`）要据此渲染**按钮**与
//   **状态行**（用户看得见的那两样）；
// - **app 层**（`front/app.js`）要据此发 `SetTraceEnabled`（内核收到的那一样）。
//
// ⇒ ⇒ 两边各写一份 ⇒ ⇒ 迟早漂移（按钮写「关闭」、命令发的是「开启」）⇒ ⇒
// 拆成**纯函数** ⇒ ⇒ 两边**引同一处** ⇒ ⇒ 漂移**结构上不可能**。
//
// ⚠️ **与 `front/lib/run-loop.js` 同一手法**：判定与执行分开 ⇒ ⇒ Node 侧
//   **零 DOM** 就能测 ⇒ ⇒ 「按钮点了没反应」这类问题**判得出是哪一环坏的**。
//
// ## 不引入第二份真相
//
// 本模块**只读**内核回报的那份状态（视图状态里的 `debug.trace` =
// `TraceState { enabled, len, dropped }`）⇒ ⇒ 它是**推导量**，不是另一份
// 存储 ⇒ ⇒ 内核永远是唯一真相。
//
// ⚠️ **「未查询」是一个真状态**：内核**只在那条命令的回复里**报告开关状态
//   （`SetTraceEnabled` 是回复事件，不是推送流）⇒ ⇒ 一次都没点过 ⇒ 前端
//   **不知道**开关是开是关 ⇒ ⇒ 那就**如实说不知道**（`unknown`）⇒ ⇒
//   **不猜**（内核的默认值是「开」，但那是内核的内部事实，界面不替它说）。

/**
 * 开关状态：**还没问过内核**。
 *
 * ⚠️ 这不是「关闭」—— 它是「**不知道**」⇒ ⇒ 界面必须**如实说不知道**，
 *   不能拿「关闭」冒充（那会让用户以为「记录是关着的」，而内核其实在录）。
 *
 * @type {string}
 */
export const TRACE_SWITCH_UNKNOWN = "unknown";

/**
 * 开关状态：**开着**（内核在记录指令与访存）。
 *
 * @type {string}
 */
export const TRACE_SWITCH_ON = "on";

/**
 * 开关状态：**关着**（内核零开销，不记录任何东西）。
 *
 * @type {string}
 */
export const TRACE_SWITCH_OFF = "off";

/**
 * 从内核的那份报告推导出开关状态。
 *
 * @param {unknown} trace — `TraceState`（`{ enabled, len, dropped }`）或 `null`
 * @returns {{state: string, on: boolean, len: number, dropped: number}}
 */
export function traceSwitchState(trace) {
    const known = Boolean(trace)
        && typeof trace === "object"
        && typeof (/** @type {any} */ (trace).enabled) === "boolean";
    if (!known) {
        return {
            state: TRACE_SWITCH_UNKNOWN,
            on: false,
            len: 0,
            dropped: 0,
        };
    }
    const raw = /** @type {any} */ (trace);
    const len = Number(raw.len);
    const dropped = Number(raw.dropped);
    return {
        state: raw.enabled === true ? TRACE_SWITCH_ON : TRACE_SWITCH_OFF,
        on: raw.enabled === true,
        len: Number.isFinite(len) ? Math.max(0, Math.trunc(len)) : 0,
        dropped: Number.isFinite(dropped) ? Math.max(0, Math.trunc(dropped)) : 0,
    };
}

/**
 * 「点一下 ⇒ 发出去的值」。
 *
 * ⚠️ **状态未知 ⇒ 第一次点 = 开启**：用户点「开启 trace 记录」⇒ ⇒ 发的就是
 *   `true` ⇒ ⇒ 内核回报之后状态就**准**了（内核的报告是唯一真相）⇒ ⇒
 *   第一次点击**顺带**完成「查询」。
 *
 * ⚠️ **状态已知 ⇒ 发它的反面**（开 ⇄ 关）。
 *
 * @param {unknown} trace — `TraceState` 或 `null`
 * @returns {boolean} `SetTraceEnabled` 的 `enabled`
 */
export function nextTraceEnabled(trace) {
    const sw = traceSwitchState(trace);
    if (sw.state === TRACE_SWITCH_UNKNOWN) return true;
    return !sw.on;
}
