// 终端视图面板（M-02 · terminal）—— 拆分自 m02_viewcomponents.js。
//
// 管 `terminalView` 注册组件：guest UART 回显 + 逐键直输；含 console 字节
// 解码 / 计数、行结束符档位、自研终端屏幕缓存与 xterm 适配。

import * as viewStore from "../m03_viewstore.js";
import { escapeHtml } from "../../lib/escape.js";
// ⚠️ **终端屏幕仿真**（STAGE-41）：guest 的 shell 是**行编辑器**（每敲一个
// 键就把整行重画一遍）⇒ 字节流里同一个提示符出现 N 次 ⇒ 真实终端只显示
// 一行 ⇒ 不能把字节流原样塞进 `<pre>`（`\r` 被渲染成换行 ⇒ 一堆乱码）。
import { createTerminalScreen, renderTerminalScreen } from "./terminal-screen.js";
// ⚠️ **终端仿真库（xterm.js）加载器**：与 echarts 同一条懒加载 / 失败降级
// 口径。视图层**只同步读** `getXterm()`；**加载**由 app 层发起。
import { getXterm } from "../../lib/xterm-loader.js";
import { register, emitCommand } from "../component-registry.js";

/**
 * console 字节数组的**分块大小**（每次 `String.fromCharCode` 展开的字节数）。
 *
 * ⚠️ **为什么要分块**：`String.fromCharCode(...bytes)` 把**每个字节**当成
 * 一个**实参**传给函数——几万个字节会**爆调用栈**。真实 RTOS 的 console 一次
 * 能吐几万个字节 ⇒ 必须分块。4096 是个安全取值。
 */
export const CONSOLE_DECODE_CHUNK = 4096;

/**
 * 把 console 的**字节数组**解成字符串。
 *
 * ⚠️ **字节，不是字符**：guest 可能吐出任意字节（ANSI 转义、`\r`、半个
 * UTF-8 序列）⇒ 这里做「字节 → 码位」一一映射（`String.fromCharCode`），
 * **不**做 UTF-8 解码——**字节流才是真相**。
 *
 * ⚠️ **字符串原样返回**：既有的 `onDelta({ termDelta: { bytes: "<x>&y" } })`
 * 走字符串路径（**既有渲染语义不许丢**）⇒ 这里不碰它。
 *
 * @param {unknown} bytes — 字节数组（`[104, 105]`）或字符串
 * @returns {string}
 */
export function decodeConsoleBytes(bytes) {
    if (typeof bytes === "string") return bytes;
    if (!Array.isArray(bytes)) return "";
    const parts = [];
    for (let i = 0; i < bytes.length; i += CONSOLE_DECODE_CHUNK) {
        const chunk = bytes.slice(i, i + CONSOLE_DECODE_CHUNK);
        parts.push(String.fromCharCode(...chunk));
    }
    return parts.join("");
}

/**
 * console 字节流的**字节数**。
 *
 * ⚠️ **按字节数，不按字符数**（裁决 G）：guest 可能吐出任意字节 ⇒ 字符数 ≠
 * 字节数 ⇒ 计数必须是**字节数**。字节数组直接取长度；字符串按 UTF-8 编码后取长度。
 *
 * @param {unknown} raw — 字节数组或字符串
 * @returns {number}
 */
export function consoleByteCount(raw) {
    if (Array.isArray(raw)) return raw.length;
    if (typeof raw === "string") return new TextEncoder().encode(raw).length;
    return 0;
}

/**
 * 终端的**行结束符**四档（STAGE-40 · 裁决 G）。
 *
 * ⚠️ **行结束符是 guest 的属性，不是平台的属性**：osz 的 shell 只认 CR（13），
 * 多数 POSIX shell 认 LF（也认 CR）⇒ 平台**不猜** ⇒ 做成**用户可选**。
 * **默认是 CR**：真实串口终端按回车发的就是 CR（0x0D）。
 *
 * @type {Readonly<Record<string, number[]>>}
 */
export const TERMINAL_EOL = Object.freeze({
    cr: [13],
    lf: [10],
    crlf: [13, 10],
    none: [],
});

/** 行结束符的**默认档**（= CR）。 */
export const TERMINAL_EOL_DEFAULT = "cr";

/**
 * 行结束符按钮的**四档**（渲染与文案都用它 ⇒ **一处定义**）。
 *
 * ⚠️ **不用 `<select>`**：`front/tests/dom-stub.mjs` 的 `<select>.value`
 * 读的是元素自己的 `value` 属性 ⇒ 「非默认项」在 Node 门禁里**测不到** ⇒
 * 用**按钮组**（`data-eol`）⇒ 两层都可测。
 *
 * @type {ReadonlyArray<{id: string, label: string, title: string}>}
 */
export const TERMINAL_EOL_OPTIONS = Object.freeze([
    Object.freeze({ id: "cr", label: "CR", title: "0x0D 回车" }),
    Object.freeze({ id: "lf", label: "LF", title: "0x0A" }),
    Object.freeze({ id: "crlf", label: "CRLF", title: "0x0D 0x0A" }),
    Object.freeze({ id: "none", label: "不加", title: "不追加行结束符" }),
]);

/**
 * 从视图状态里读**当前的行结束符**（读不到 ⇒ 默认档）。
 *
 * ⚠️ **判据用 `hasOwnProperty` 而不是 `TERMINAL_EOL[x]`** —— 后者会把
 * `"constructor"` 之类当成合法取值。
 *
 * @param {object} [state]
 * @returns {string}
 */
function currentTerminalEol(state) {
    const raw = state?.term?.eol ?? viewStore.getState("terminal")?.term?.eol;
    return Object.prototype.hasOwnProperty.call(TERMINAL_EOL, raw) ? raw : TERMINAL_EOL_DEFAULT;
}

/**
 * 渲染行结束符的**按钮组**。
 *
 * ⚠️ **当前档带 `data-eol-selected="true"`** ⇒ 判据可以按属性读（不用读文案）。
 *
 * @param {string} eol
 * @returns {string}
 */
function renderTerminalEolButtons(eol) {
    return TERMINAL_EOL_OPTIONS.map((opt) => {
        const selected = opt.id === eol;
        return `<button type="button" class="terminal-eol-btn${selected ? " is-selected" : ""}" data-eol="${opt.id}"${selected ? ' data-eol-selected="true"' : ""} title="${escapeHtml(opt.title)}">${opt.label}</button>`;
    }).join("");
}

/**
 * 切换行结束符（**住在视图状态里** ⇒ 重渲染不丢）。
 *
 * @param {unknown} eol
 * @returns {string} the selected id
 */
function setTerminalEol(eol) {
    const next = Object.prototype.hasOwnProperty.call(TERMINAL_EOL, eol)
        ? eol
        : TERMINAL_EOL_DEFAULT;
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), eol: next },
    }));
    publishTerminalState();
    return next;
}

// ---------------------------------------------------------------------------
// 终端屏幕（STAGE-41 · 裁决 D ~ F）
// ---------------------------------------------------------------------------

/**
 * 增量喂字节的**屏幕实例**（模块级单例）。
 *
 * ⚠️ **为什么要增量**：长会话里 console 事件可能来很多次 ⇒ 每来一次就**从零
 * 重扫**整条历史是 O(n²) ⇒ 这里只处理**新到的那些字节**。
 * ⚠️ **它不是真相**：真相是 store 里的 `term.bytes` ⇒ 本实例只是「快速路径」
 * 的缓存 ⇒ 任何时候都能从真相**重建**（裁决 E）。
 */
let terminalScreen = createTerminalScreen();

/**
 * 增量实例**已经吃掉的**原始字节流（用来核对「真的是追加」）。
 *
 * @type {string}
 */
let terminalScreenRaw = "";

/**
 * 把终端屏幕**重置**（换工程 / 清会话时用）。
 *
 * ⚠️ **必须与 store 的 `term.bytes` 清空**成对发生 ⇒ 否则增量实例还拿着
 * 上一段工程的屏幕 ⇒ 显示错乱。
 */
export function resetTerminalScreen() {
    terminalScreen = createTerminalScreen();
    terminalScreenRaw = "";
}

/**
 * 终端**显示什么**。
 *
 * ⚠️ **默认是「屏幕」**（把字节流按终端语义解释过的结果）⇒ 切到「原始字节」
 * 档才是字节流本身 ⇒ 两条路都留着。
 *
 * @param {string} raw 原始字节流
 * @param {boolean} rawMode 是否「原始字节」档
 * @returns {string}
 */
function terminalDisplayText(raw, rawMode) {
    return rawMode ? String(raw ?? "") : renderTerminalScreen(raw);
}

/**
 * 现在是不是「原始字节」档（**读视图状态** ⇒ 重渲染不丢）。
 *
 * @returns {boolean}
 */
function isRawMode() {
    return viewStore.getState("terminal")?.term?.raw === true;
}

/**
 * 屏幕文本（**增量**那条路 —— 判据 T-11 盯它与「从零重建」的一致性）。
 *
 * ⚠️ **只处理新到的字节** ⇒ 长会话里不是 O(n²)；但**必须核对**「store 里的
 * 字节流确实是**追加**的」⇒ 不是就**从零重建**（裁决 E）⇒ 增量实例与真相
 * **不会**悄悄分叉。
 *
 * @param {string} nextRaw store 里的**全部**字节流
 * @returns {string}
 */
function terminalScreenText(nextRaw) {
    const raw = String(nextRaw ?? "");
    if (!raw.startsWith(terminalScreenRaw)) {
        // 不是追加 ⇒ ⇒ 从零重建（**真相是 store 的字节流**，不是这个缓存）。
        resetTerminalScreen();
    }
    terminalScreen.write(raw.slice(terminalScreenRaw.length));
    terminalScreenRaw = raw;
    return terminalScreen.text();
}

/**
 * 切「原始字节 / 终端屏幕」两档（**不发契约命令**）。
 *
 * @returns {boolean} 切完之后是不是「原始字节」档
 */
function toggleTerminalRaw() {
    const next = !(viewStore.getState("terminal")?.term?.raw === true);
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), raw: next },
    }));
    publishTerminalState();
    return next;
}

const terminalView = register({
    id: "terminal",
    channels: ["term"],
    /**
     * **持久宿主**（与波形图同一个裁决 L）：视图每次重渲染都把 `body.innerHTML`
     * 整个换掉 ⇒ xterm 实例绑在宿主元素上 ⇒ 宿主元素搬走、实例不重建。
     *
     * @type {HTMLElement|null}
     */
    _host: null,
    /** 已初始化的 xterm 实例（库没就绪是 `null` ⇒ 走自研仿真器降级路径）。 */
    _term: null,
    /** Fit 自适应附加件实例。 */
    _fit: null,
    /** xterm 当前已同步的 store 字节流（用于切档 / 重渲染后的全量对齐）。 */
    _syncedRaw: null,
    render(state) {
        const buf = state?.term?.bytes ?? "";
        // ⚠️ **行结束符的选择也住在视图状态里**（裁决 I）⇒ 与既有口径同一真相。
        const eol = currentTerminalEol(state);
        // ⚠️ **原始字节档**（裁决 F）：默认显示**屏幕**（仿真后的）⇒ 切到
        // 「原始字节」才看未经解释的字节流 ⇒ 两条路都留着。
        const rawMode = state?.term?.raw === true;
        // ⚠️ **TX / RX 是两个不同的源**（裁决 G）⇒ 分别标注方向，不许混成一个。
        const txBytes = Number(state?.term?.txBytes ?? 0);
        const rxBytes = Number(state?.term?.rxBytes ?? 0);
        return `
            <section class="terminal">
                <div class="card">
                    <div class="row"><h3>串口终端</h3><span class="tag">字节进 UART0</span></div>
                    <div class="term-scroll" data-xterm-slot>
                        <pre class="terminal-out" data-terminal-out>${escapeHtml(terminalDisplayText(buf, rawMode))}</pre>
                    </div>
                    <div class="term-meta">
                        <span class="dir">TX（guest → 终端）</span><span class="n" data-term-tx>${txBytes}</span>
                        <span class="dir">RX（终端 → guest）</span><span class="n" data-term-rx>${rxBytes}</span>
                    </div>
                    <div class="terminal-toolbar">
                        <div class="terminal-eol" role="group" aria-label="行结束符">${renderTerminalEolButtons(eol)}</div>
                        <button type="button" class="terminal-raw-btn${rawMode ? " is-selected" : ""}" data-action="console-raw-toggle" data-raw-selected="${rawMode ? "true" : "false"}" title="切换原始字节 / 终端屏幕">原始字节</button>
                    </div>
                    <p class="muted">回显来自 guest，平台不做本地回显。</p>
                </div>
            </section>
        `;
    },
    onCommand(ev) {
        const actionEl = ev.target?.closest?.("[data-action]");
        const action = actionEl?.dataset?.action;
        // ⚠️ **原始字节档**：切显示口径 ⇒ **不发**任何契约命令。
        if (action === "console-raw-toggle") return toggleTerminalRaw();
        // ⚠️ **行结束符按钮**：它**没有** `data-action` ⇒ 不与上面的分支抢。
        const eolEl = ev.target?.closest?.("[data-eol]");
        if (eolEl) return setTerminalEol(eolEl.dataset.eol);
        return undefined;
    },
    /**
     * 渲染**之后**：把持久宿主搬进插槽，（库就绪时）初始化 / 复用 xterm
     * 实例；库没就绪这一轮继续走降级 `<pre>` 仿真器。
     *
     * @param {HTMLElement} body
     * @param {object} state
     */
    afterRender(body, state) {
        const slot = body?.querySelector?.("[data-xterm-slot]") ?? null;
        if (!slot) return;

        if (!terminalView._host) {
            terminalView._host = document.createElement("div");
            terminalView._host.className = "xterm-host";
            terminalView._host.setAttribute("data-xterm-host", "");
        }
        slot.appendChild(terminalView._host);

        const lib = getXterm();
        const pre = slot.querySelector("[data-terminal-out]");
        const rawMode = state?.term?.raw === true;

        if (!lib) {
            // ⚠️ 库还没好（加载由 app 层触发）：这一轮继续走降级 `<pre>` 仿真器。
            terminalView._host.hidden = true;
            if (pre) pre.hidden = false;
            return;
        }

        if (!terminalView._term) {
            const term = new lib.Terminal({
                theme: resolveXtermTheme(),
                fontFamily: readMonoFont(),
                fontSize: 12,
                scrollback: 5000,
                convertEol: false,
                cursorBlink: true,
            });
            const fit = new lib.FitAddon();
            term.open(terminalView._host);
            terminalView._fit = fit;
            fit.fit();
            // ⚠️ **逐键直输**：xterm 把按键（含控制字节 / CSI 序列）原样给回
            //   onData ⇒ 这里映射成字节就发；行编辑 / 回显都是 guest 的事。
            term.onData((data) => {
                dispatchTerminalBytes(terminalInputBytes(data, currentTerminalEol()));
            });
            terminalView._term = term;
            terminalView._syncedRaw = null;
        }

        // 屏幕档下：xterm 里的内容必须与 store 全量对齐。首次初始化回灌历史
        // 缓冲；从「原始字节」档切回屏幕档时，期间只进了降级 `<pre>` 的字节
        // 也要补回来 ⇒ 按 store 全量重写一遍（`_syncedRaw` 记下已同步的流）。
        if (!rawMode && terminalView._syncedRaw !== String(state?.term?.bytes ?? "")) {
            const buf = String(state?.term?.bytes ?? "");
            terminalView._term.clear();
            if (buf) terminalView._term.write(decodeConsoleBytes(buf));
            terminalView._syncedRaw = buf;
        }
        try { terminalView._fit?.fit(); } catch (_err) { /* 尺寸量不到时忽略 */ }

        // 原始字节档 ⇒ 降级 pre 可见、xterm 藏起来；屏幕档反之。
        terminalView._host.hidden = rawMode;
        if (pre) pre.hidden = !rawMode;
    },
    /** Append new bytes to the terminal buffer in-place (T1 fast path). */
    onDelta(delta) {
        if (!delta?.termDelta) return;
        // ⚠️ **字节数组也要能吃**：契约 v4 的 `Console` 事件载荷是
        // `bytes: [104, 105]`（JSON **字节数组**）⇒ 解码成字符串再喂给
        // store（store 里的 `term.bytes` 一直是**字符串**）。
        const bytes = decodeConsoleBytes(delta.termDelta.bytes ?? "");
        // ⚠️ **TX 计数**：这是**内核 console 事件**累计的字节数 ⇒ 与 RX
        // **不是同一个源** ⇒ 分别标注。
        const txDelta = consoleByteCount(delta.termDelta.bytes ?? "");
        // The store is the single source of truth: the DOM is rewritten
        // from the accumulated value instead of appending to whatever the
        // DOM currently holds.
        const next = (viewStore.getState("terminal")?.term?.bytes ?? "") + bytes;
        viewStore.setState("terminal", (prev) => ({
            ...prev,
            term: {
                ...(prev.term ?? {}),
                bytes: next,
                txBytes: Number(prev.term?.txBytes ?? 0) + txDelta,
            },
        }));
        // ⚠️ **计数也要就地更新**：这条路径**不重渲染** ⇒ 不改计数的话
        // TX 会**一直停在旧值**。
        const txEl = document.querySelector("[data-term-tx]");
        if (txEl) {
            txEl.textContent = String(Number(viewStore.getState("terminal")?.term?.txBytes ?? 0));
        }
        const rawMode = isRawMode();
        // xterm 就绪且在屏幕档 ⇒ 增量喂给它；否则写降级 pre（既有仿真器路径）。
        if (terminalView._term && !rawMode) {
            terminalView._term.write(bytes);
            terminalView._syncedRaw = next;
            return;
        }
        const out = document.querySelector("[data-terminal-out]");
        if (!out) return;
        // `textContent` does not parse HTML — escaping here would print a
        // literal "&lt;" (FRONTEND_Review.md #4).
        out.textContent = rawMode ? next : terminalScreenText(next);
        out.scrollTop = out.scrollHeight;
    },
    mountFor(el) {
        // ⚠️ **事件委托**（K-1）：监听器绑在**容器**上 ⇒ 重渲染重建的子元素
        //   不会把监听器丢掉。直输模式下键盘输入由 xterm 自己接管，这里只剩
        //   按钮 / EOL 组的点击委托。
        el.addEventListener("click", (ev) => terminalView.onCommand(ev));
    },
});

/**
 * 从 CSS 令牌解析 xterm 主题（终端内部不受 CSS 管 ⇒ 颜色必须显式喂；
 * 读不到就留空，用 xterm 默认值；Node 侧无 `getComputedStyle` 也安全）。
 *
 * @returns {object}
 */
function resolveXtermTheme() {
    const out = {};
    if (typeof document === "undefined" || typeof getComputedStyle !== "function") return out;
    let cs = null;
    try { cs = getComputedStyle(document.documentElement); } catch (_err) { return out; }
    const read = (name) => {
        try { return String(cs.getPropertyValue(name) ?? "").trim(); } catch (_err) { return ""; }
    };
    const bg = read("--term-bg");
    const fg = read("--term-fg");
    if (bg) out.background = bg;
    if (fg) {
        out.foreground = fg;
        out.cursor = fg;
        out.cursorAccent = bg || "#18181b";
    }
    return out;
}

/** 读等宽字体栈（`--mono` 令牌；读不到退回通用 monospace）。 */
function readMonoFont() {
    if (typeof document === "undefined" || typeof getComputedStyle !== "function") return "monospace";
    try {
        const v = String(getComputedStyle(document.documentElement).getPropertyValue("--mono") ?? "").trim();
        return v || "monospace";
    } catch (_err) {
        return "monospace";
    }
}

/**
 * 让终端视图按**当前状态**重渲染。
 *
 * ⚠️ 走的是 M-01 的 `riscv:render` 事件 ⇒ 视图层不直接摸 DOM。
 */
function publishTerminalState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "terminal", type: "update", state: viewStore.getState("terminal") },
    }));
    // 重渲染把 `.terminal-out` **重建**了 ⇒ 滚动位置回到顶部 ⇒ 把它推回
    // 底部（与 `onDelta` 的既有行为一致）。
    const out = document.querySelector("[data-terminal-out]");
    if (out) out.scrollTop = out.scrollHeight;
}

/**
 * 把 xterm `onData` 收到的一段按键文本映射成要发出去的**字节**。
 *
 * ⚠️ **字节，不是字符**（K-1）：guest 的 UART 收的是**字节**（契约 v5 的
 * `WriteConsole { bytes: Vec<u8> }`）⇒ 用 `TextEncoder` 做 UTF-8 编码，
 * **不**送字符串。
 *
 * ⚠️ **回车映射行结束符**：直输模式没有整行草稿 ⇒ 回车（`\r` / `\n`）
 * 只发当前档的 EOL 字节；其余按键逐帧直发。
 *
 * ⚠️ **控制字节原样透传**：`\x03`（Ctrl-C）、`\x7f`（退格）、方向键的
 * CSI 序列都不做本地解释 ⇒ 由 guest 的 shell 自己处理。
 *
 * @param {unknown} data xterm onData 回传的按键文本
 * @param {string} eol 当前行结束符档（cr / lf / crlf / none）
 * @returns {number[]}
 */
export function terminalInputBytes(data, eol) {
    const text = String(data ?? "");
    if (text === "\r" || text === "\n") {
        const table = Object.prototype.hasOwnProperty.call(TERMINAL_EOL, eol)
            ? TERMINAL_EOL[eol]
            : TERMINAL_EOL[TERMINAL_EOL_DEFAULT];
        return [...table];
    }
    return Array.from(new TextEncoder().encode(text));
}

/**
 * 把已经算好的字节数组发进 guest 的 UART0（直输模式：调用方负责把按键
 * 映射成字节）。
 *
 * ⚠️ **RX 计数**（裁决 G）：计「前端发出」的字节数；`WriteConsole` 没有
 * 回执 ⇒ 不假装知道内核收没收到。
 *
 * ⚠️ **视图层不许直接调服务层**：这里只发信号，`app.js` 把它变成真的命令。
 *
 * @param {number[]} bytes
 * @returns {string|undefined} the request id（空字节时不发命令）
 */
export function dispatchTerminalBytes(bytes) {
    const payload = Array.isArray(bytes) ? bytes : [];
    if (payload.length === 0) return undefined;
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: {
            ...(prev.term ?? {}),
            rxBytes: Number(prev.term?.rxBytes ?? 0) + payload.length,
        },
    }));
    // ⚠️ **计数就地更新**（与 `onDelta` 的 TX 同一条快路径）：这条链路不重渲染
    //   ⇒ 不刷元素的话 RX 会一直停在旧值。
    const rxEl = document.querySelector("[data-term-rx]");
    if (rxEl) {
        rxEl.textContent = String(Number(viewStore.getState("terminal")?.term?.rxBytes ?? 0));
    }
    return emitCommand("console-send", "terminal", { bytes: payload });
}

/**
 * 新会话 / 清会话时把 xterm 实例一并清空（与 `resetTerminalScreen` 成对）。
 *
 * ⚠️ **必须与 store 的 `term.bytes` 清空成对发生** ⇒ 否则实例还拿着上一段
 * 工程的屏幕 ⇒ 显示错乱。
 */
export function resetTerminalXterm() {
    if (terminalView._term) {
        try { terminalView._term.clear(); } catch (_err) { /* 实例未就绪时忽略 */ }
    }
    terminalView._syncedRaw = null;
}
