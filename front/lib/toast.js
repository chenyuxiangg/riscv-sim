// 顶部渐隐 toast —— 轻量、非模态的即时反馈（内存读越限这类「点一下、立刻
// 知道没读成」的场景）。
//
// ## 与 `lib/alert.js` 的分工
//
// - `alert.js`：居中模态告警，带「知道了」按钮，用于需要用户确认的拦截
//   （运行中读内存等）；
// - 本模块：顶部居中的轻提示，**没有按钮**，停留一会儿自动渐隐消失，用于
//   命令被内核拒绝这类「看到原因即可、不必点确认」的反馈。
//
// ⚠️ **状态用属性** `data-toast-state` 表达（`open` / `fading` / `""`），
//    不用 `style` / `hidden` ⇒ ⇒ `front/tests/dom-stub.mjs` 只支持
//    `classList` / `dataset` / `getAttribute` ⇒ ⇒ Node 侧零浏览器就能测。
//
// ⚠️ **骨架住在 `front/index.html` 顶层** ⇒ ⇒ 视图重渲染（`body.innerHTML`
//    重建）不会把它销毁，理由同 alert.js 裁决 J。

/** 提示停留多久之后**开始渐隐**（毫秒）。 */
export const TOAST_HOLD_MS = 3000;

/** 渐隐本身用多久（毫秒）。 */
export const TOAST_FADE_MS = 300;

/** 状态：显示中。 */
export const TOAST_STATE_OPEN = "open";

/** 状态：正在渐隐。 */
export const TOAST_STATE_FADING = "fading";

/** 状态：已关闭。 */
export const TOAST_STATE_CLOSED = "";

/** toast 骨架的选择器（**唯一**落点）。 */
const TOAST_SELECTOR = "[data-toast]";

/**
 * 「开始渐隐」那个定时器。
 * @type {any}
 */
let holdTimer = null;

/**
 * 「彻底关掉」那个定时器。
 * @type {any}
 */
let fadeTimer = null;

/**
 * 当前的 `document`（没有 DOM 时是 `null`）。
 *
 * @returns {Document|null}
 */
function currentDocument() {
    return typeof globalThis !== "undefined" && globalThis.document
        ? globalThis.document
        : null;
}

/**
 * 读当前 toast 状态（判据用）。
 *
 * @returns {string} `"open"` / `"fading"` / `""`（没有 toast 也是 `""`）
 */
export function toastState() {
    const doc = currentDocument();
    const toast = doc?.querySelector?.(TOAST_SELECTOR) ?? null;
    if (!toast) return TOAST_STATE_CLOSED;
    return String(toast.getAttribute?.("data-toast-state") ?? TOAST_STATE_CLOSED);
}

/**
 * 清掉两个定时器（**无条件**）⇒ ⇒ toast 坏掉时这里照旧能清，门禁变红而
 * 不是卡死。
 */
function clearTimers() {
    if (holdTimer !== null) {
        globalThis.clearTimeout?.(holdTimer);
        holdTimer = null;
    }
    if (fadeTimer !== null) {
        globalThis.clearTimeout?.(fadeTimer);
        fadeTimer = null;
    }
}

/**
 * 写 toast 状态（**唯一**写状态的地方）。
 *
 * @param {string} state
 * @returns {string} 写进去的那一份
 */
function setState(state) {
    const doc = currentDocument();
    const toast = doc?.querySelector?.(TOAST_SELECTOR) ?? null;
    toast?.setAttribute?.("data-toast-state", state);
    return state;
}

/**
 * 弹出顶部 toast。
 *
 * ⚠️ **反复调用会重置计时** ⇒ ⇒ 连着两次失败 ⇒ ⇒ 提示重新开始计时，不会被
 *    第二次触发提前关掉。
 *
 * @param {string} text — 提示正文
 * @param {{holdMs?: number, fadeMs?: number}} [opts]
 * @returns {string} 写进去的状态
 */
export function showToast(text, opts = {}) {
    const doc = currentDocument();
    const toast = doc?.querySelector?.(TOAST_SELECTOR) ?? null;
    if (!toast) return TOAST_STATE_CLOSED;

    const holdMs = Number.isFinite(opts.holdMs) ? Number(opts.holdMs) : TOAST_HOLD_MS;
    const fadeMs = Number.isFinite(opts.fadeMs) ? Number(opts.fadeMs) : TOAST_FADE_MS;

    const textEl = doc?.querySelector?.("[data-toast-text]") ?? null;
    if (textEl) textEl.textContent = String(text ?? "");

    clearTimers();
    setState(TOAST_STATE_OPEN);

    holdTimer = globalThis.setTimeout?.(() => {
        holdTimer = null;
        setState(TOAST_STATE_FADING);
        fadeTimer = globalThis.setTimeout?.(() => {
            fadeTimer = null;
            setState(TOAST_STATE_CLOSED);
        }, fadeMs);
        // ⚠️ Node 的定时器会让事件循环活着 ⇒ ⇒ 前端门禁不能被一个「等着渐隐」
        //    的定时器挂住。
        fadeTimer?.unref?.();
    }, holdMs);
    holdTimer?.unref?.();
    return TOAST_STATE_OPEN;
}

/**
 * **立即**关闭 toast。
 *
 * @returns {string} 写进去的状态
 */
export function hideToast() {
    clearTimers();
    return setState(TOAST_STATE_CLOSED);
}
