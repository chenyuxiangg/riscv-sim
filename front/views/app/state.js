// App 层 · 跨命令 / 接线 / 装载族共享的模块级可变状态（从 app.js 下沉）。
//
// ⚠️ 这些状态**不进视图状态**（M-03），是 app 层这一层自己的过路记忆：
// 暂停过路标记、当前工程、下一条错误该显示在哪一位。各族经这里的
// getter / setter / 消费函数访问，不直接共享 `let` 绑定 ⇒ ⇒ 子模块之间
// 不互相引用可变绑定，避免门面 ↔ 子模块循环依赖。

/**
 * 「**用户点了暂停、正在跑的那一个窗口还没结束**」这个**过路状态**
 * （v0.67 批）。
 *
 * ⚠️ **为什么需要它**：界面上的「暂停」**不发** `Pause` 命令（裁决 E）⇒ ⇒ 它
 *   只是**停续发**⇒ ⇒ 正在跑的那**一个**窗口**跑完**才停 ⇒ ⇒ 那次停下**就是**
 *   用户要的那次停 ⇒ ⇒ 但从外面看，「用户点过暂停」这件事**只存在于这一小段
 *   窗口里** ⇒ ⇒ 不记它 ⇒ ⇒ 就分不出「这次 `EndOfExecution` 是用户要的那次停」
 *   与「连续运行里每个窗口都会来的那次」⇒ ⇒ **两者长得一模一样**。
 *
 * ⚠️ **只在两个时刻之间为真**：点「暂停」时**置真**、下一次「停下报告」时
 *   **消费掉** ⇒ ⇒ 不会一直挂着 ⇒ ⇒ 不会把**后面**某次停下也当成用户要的。
 *
 * ⚠️ **不是用户状态** ⇒ ⇒ 不进视图状态 ⇒ ⇒ 只活在这两个时刻之间。
 *
 * @type {boolean}
 */
let pausePending = false;

/** @returns {boolean} 现在是否在等「用户要的那次停」。 */
export function isPausePending() {
    return pausePending;
}

/** @param {boolean} value */
export function setPausePending(value) {
    pausePending = Boolean(value);
}

/**
 * **消费**一次「用户要的那次停」（v0.67 批）。
 *
 * ⚠️ **消费即清零** ⇒ ⇒ 只可能触发**一次**读回 ⇒ ⇒ 不会连着两次停下都当成
 *   「用户要的那次」⇒ ⇒ 不会连着读回两次。
 *
 * @returns {boolean} 刚才是不是真的在等
 */
export function consumePausePending() {
    if (!pausePending) return false;
    pausePending = false;
    return true;
}

/**
 * **当前装载着的工程**（`model::Project` 的线上形态）。
 *
 * ⚠️ 「重新装载工程」按钮需要它：板级配置是**待生效**的，宿主在**下一次**
 * `LoadProject` 时才应用 ⇒ 前端必须能**再发一次同一个工程**。
 *
 * ⚠️ 它由 `loadProjectIntoKernel` 记下 ⇒ **每一条**装载路径（启动、
 * 「reset」命令、装载仓外镜像）都会被记到 ⇒ 不会漏。
 *
 * @type {object|null}
 */
let loadedProject = null;

/** @returns {object|null} 当前装载着的工程。 */
export function getLoadedProject() {
    return loadedProject;
}

/** @param {object|null} project */
export function setLoadedProject(project) {
    loadedProject = project;
}

/**
 * 下一条内核错误**该显示在哪一位**（契约 v9 · C-13）。
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

/** @returns {"cond"|"symbol"|"dump"|null} 下一条错误的目标位。 */
export function getPendingErrorSlot() {
    return pendingErrorSlot;
}

/** @param {"cond"|"symbol"|"dump"|null} slot */
export function setPendingErrorSlot(slot) {
    pendingErrorSlot = slot;
}
