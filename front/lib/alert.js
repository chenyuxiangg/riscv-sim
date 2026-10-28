// STAGE-38 · v0.53 批 —— **告警弹窗**（居中模态 · 5 秒后渐隐 · 「知道了」立即关闭）。
//
// ## 为什么要有这个模块
//
// 用户要求：「如果用户在运行时查看内存，需要报弹窗告警，弹窗需要能渐隐消失」。
//
// ⚠️ **仓里没有 toast 设施**（只有 `setNote()` 那一行页脚、`.banner`、
//    `.trace-notice`）⇒ ⇒ 这是**新建**的，不是改造某个既有设施。
//
// ⚠️ **本模块是本仓第一个碰 DOM 的 `front/lib/` 模块** ⇒ ⇒ 理由：弹窗是
//    **全局**的（任何视图下触发都看得见）⇒ ⇒ 它**不属于**任何一个视图
//    ⇒ ⇒ 放进 `front/views/components/` 反而会**谎报**它的归属。
//
// ## 状态怎么表达（裁决 I）
//
// 状态用**属性** `data-alert-state` 表达，取值：
//
// - `"open"`   ⇒ 显示中；
// - `"fading"` ⇒ 正在渐隐（CSS 过渡把不透明度降到 0）；
// - `""`       ⇒ 已关闭。
//
// ⚠️ **不用 `style` / `hidden` 属性** ⇒ ⇒ `front/tests/dom-stub.mjs` 只支持
//    `classList` / `dataset` / `getAttribute` / `setAttribute` ⇒ ⇒ 用属性
//    ⇒ ⇒ Node 侧**零浏览器**就能测。
//
// ⚠️ **骨架住在 `front/index.html` 的顶层**（裁决 J）⇒ ⇒ 视图重渲染
//    （`body.innerHTML = html`）**不会**把它销毁。

/** 弹窗显示多久之后**开始渐隐**（毫秒）。 */
export const ALERT_HOLD_MS = 5000;

/** 渐隐本身用多久（毫秒）。 */
export const ALERT_FADE_MS = 400;

/** 状态：显示中。 */
export const ALERT_STATE_OPEN = "open";

/** 状态：正在渐隐。 */
export const ALERT_STATE_FADING = "fading";

/** 状态：已关闭。 */
export const ALERT_STATE_CLOSED = "";

/** 弹窗骨架的选择器（**唯一**落点）。 */
const OVERLAY_SELECTOR = "[data-alert-overlay]";

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
 * 「知道了」按钮**只接一次**监听 ⇒ ⇒ 反复弹窗不会叠加监听器。
 * @type {boolean}
 */
let dismissWired = false;

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
 * 读当前弹窗状态（判据用）。
 *
 * @returns {string} `"open"` / `"fading"` / `""`（没有弹窗也是 `""`）
 */
export function alertState() {
    const doc = currentDocument();
    const overlay = doc?.querySelector?.(OVERLAY_SELECTOR) ?? null;
    if (!overlay) return ALERT_STATE_CLOSED;
    return String(overlay.getAttribute?.("data-alert-state") ?? ALERT_STATE_CLOSED);
}

/**
 * 清掉两个定时器（**无条件**）。
 *
 * ⚠️ **清定时器不许依赖「被测的那个按钮」** ⇒ ⇒ 弹窗坏掉时这里**照旧**
 *    能清 ⇒ ⇒ 门禁**变红**而不是**卡死**。
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
 * 写弹窗状态（**唯一**写状态的地方）。
 *
 * @param {string} state
 * @returns {string} 写进去的那一份
 */
function setState(state) {
    const doc = currentDocument();
    const overlay = doc?.querySelector?.(OVERLAY_SELECTOR) ?? null;
    overlay?.setAttribute?.("data-alert-state", state);
    return state;
}

/**
 * 把「知道了」按钮的监听接上（**只接一次**）。
 */
function ensureDismissWired() {
    if (dismissWired) return;
    const doc = currentDocument();
    const btn = doc?.querySelector?.("[data-alert-dismiss]") ?? null;
    if (!btn || typeof btn.addEventListener !== "function") return;
    dismissWired = true;
    btn.addEventListener("click", () => {
        // ⚠️ **「知道了」= 立即关闭**（不渐隐）⇒ ⇒ 用户明确按了按钮 ⇒ ⇒
        //   不用再等那 400 ms。
        clearTimers();
        setState(ALERT_STATE_CLOSED);
    });
}

/**
 * 打开告警弹窗。
 *
 * ⚠️ **反复调用会重置计时** ⇒ ⇒ 连着点两次「读内存」⇒ ⇒ 弹窗**重新**
 *    开始计时 ⇒ ⇒ 不会被第二次点击提前关掉。
 *
 * @param {string} text — 弹窗正文
 * @param {{title?: string, holdMs?: number, fadeMs?: number}} [opts]
 * @returns {string} 写进去的状态
 */
export function showAlert(text, opts = {}) {
    const doc = currentDocument();
    const overlay = doc?.querySelector?.(OVERLAY_SELECTOR) ?? null;
    if (!overlay) return ALERT_STATE_CLOSED;

    const title = String(opts.title ?? "提示");
    const holdMs = Number.isFinite(opts.holdMs) ? Number(opts.holdMs) : ALERT_HOLD_MS;
    const fadeMs = Number.isFinite(opts.fadeMs) ? Number(opts.fadeMs) : ALERT_FADE_MS;

    const titleEl = doc?.querySelector?.("[data-alert-title]") ?? null;
    const textEl = doc?.querySelector?.("[data-alert-text]") ?? null;
    if (titleEl) titleEl.textContent = title;
    if (textEl) textEl.textContent = String(text ?? "");

    ensureDismissWired();
    clearTimers();
    setState(ALERT_STATE_OPEN);

    holdTimer = globalThis.setTimeout?.(() => {
        holdTimer = null;
        setState(ALERT_STATE_FADING);
        fadeTimer = globalThis.setTimeout?.(() => {
            fadeTimer = null;
            setState(ALERT_STATE_CLOSED);
        }, fadeMs);
        // ⚠️ Node 的定时器会让事件循环活着 ⇒ ⇒ 前端门禁不能被一个
        //    「等着渐隐」的定时器挂住。
        fadeTimer?.unref?.();
    }, holdMs);
    // ⚠️ 同上（见上）。
    holdTimer?.unref?.();
    return ALERT_STATE_OPEN;
}

/**
 * **立即**关闭告警弹窗（「知道了」按钮走的就是这条）。
 *
 * @returns {string} 写进去的状态
 */
export function hideAlert() {
    clearTimers();
    return setState(ALERT_STATE_CLOSED);
}
