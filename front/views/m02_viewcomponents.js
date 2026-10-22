// M-02 视图组件库 (View component library).
//
// Four MVP views (LLD §3.4):
//   - productTreeView   — 产品树/原理图 + M-14 内存映射表 (FR-01/02/03)
//   - debugView         — 反汇编区 + 寄存器/内存面板 (FR-05/08/09 占位)
//   - waveformView      — 波形 UI (FR-10/14, MVP 仅通道选择 + 时基)
//   - terminalView      — 串口终端回显 (NFR-04 XSS 防护)
//
// Each component implements the three-function shape from LLD §2.1.1:
//   - render(storeState)
//   - onCommand(ev)     → requestId
//   - onDelta(delta)
//
// Render dispatch comes from M-01 `riscv:render` CustomEvents.
// In MVP we do NOT subscribe to M-05 event flow (M-07 transport not
// wired yet) — components render with empty state and update locally.

import * as viewStore from "./m03_viewstore.js";
import { escapeHtml } from "../lib/escape.js";
import {
    describeMatch,
    matchSourceFile,
    renderSourceText,
} from "./components/source-view.js";

// ---------------------------------------------------------------------------
// ViewComponent shape (LLD §2.1.1)
// ---------------------------------------------------------------------------

/**
 * @typedef {object} StoreState
 * @property {string} viewId
 * @property {number} timeVirt
 * @property {number} version
 * @property {object} sim
 * @property {object} wave
 * @property {object} term
 * @property {object} events
 */

/**
 * @typedef {object} ViewComponent
 * @property {string} id
 * @property {string[]} channels
 * @property {(state: object) => string|Node} render
 * @property {(ev: Event) => string|undefined} onCommand
 * @property {(delta: object) => void} onDelta
 * @property {(mount: HTMLElement) => void} mountFor      // bind listeners
 */

/** @type {Map<string, ViewComponent>} */
const components = new Map();

/** Register a view component. Idempotent. */
export function register(c) {
    components.set(c.id, c);
    return c;
}

/** @returns {ViewComponent|undefined} */
export function get(id) {
    return components.get(id);
}

/** Test / dev only. */
export function _all() {
    return [...components.values()];
}

/** Reset the registry. */
export function reset() {
    components.clear();
}

// ---------------------------------------------------------------------------
// XSS-safe text + DOM helpers (NFR-04).
// ---------------------------------------------------------------------------

// The helper used to live here; it now has a single implementation in
// `front/lib/escape.js` (FRONTEND_Review.md #9). Re-exported so existing
// importers keep working.
export { escapeHtml };

// ---------------------------------------------------------------------------
// Command signal — the view layer never talks to the service layer
// directly (the same rule the demo seeder follows: M-02 only emits a
// signal, `app.js` turns it into state).
// ---------------------------------------------------------------------------

/** Monotonic request id counter (the ViewComponent `onCommand` return). */
let requestCounter = 0;

/**
 * Emit the `riscv:command` signal and return the request id.
 *
 * @param {string} action
 * @param {string} viewId
 * @param {object|null} [params] — command parameters (address / count /
 *   source / format / offset …). They travel **in the signal**: the view
 *   layer must not talk to the service layer, and the DOM is not
 *   reachable from the bootstrap's listener (the signal is dispatched on
 *   `window`, so `event.target` is the window, not the button).
 * @returns {string} the request id
 */
function emitCommand(action, viewId, params = null) {
    requestCounter += 1;
    const requestId = `cmd-${action}-${requestCounter}`;
    window.dispatchEvent(new CustomEvent("riscv:command", {
        detail: { action, requestId, viewId, params },
    }));
    return requestId;
}

// ---------------------------------------------------------------------------
// productTreeView — shows the M-14 mapping table (FR-01/02/03)
// ---------------------------------------------------------------------------

const productTreeView = register({
    id: "product",
    channels: ["sim"],
    /** Render to body. Pure: same storeState → same DOM string. */
    render(state) {
        const mappings = state?.sim?.memoryMap ?? [];
        const tree = state?.tree ?? { root: { name: "未加载", children: [] } };
        return `
            <div class="product-grid">
                <section class="product-tree">
                    <h3>产品树</h3>
                    ${renderTree(tree.root)}
                </section>
                <section class="memory-map">
                    <h3>内存映射 (M-14)</h3>
                    ${renderMapTable(mappings)}
                </section>
            </div>
            <p class="muted">
                提示：MVP 阶段内存映射来自一次性 seed；要刷新请调用
                <code>window.riscv_demo.seedBus()</code>（dev only）。
            </p>
        `;
    },
    onCommand(ev) {
        // User action: ask for the demo memory map to be re-seeded.
        //
        // The button carries `data-action` itself and lives *outside* the
        // <table>, so it has no `[data-region-base]` ancestor — requiring
        // one made the button permanently dead (FRONTEND_Review.md #3).
        const target = /** @type {HTMLElement} */ (ev.target);
        if (target?.matches?.("[data-action='seed-demo']")) {
            seedBus("product");
            return "cmd-seed-demo";
        }
        return undefined;
    },
    onDelta(_delta) {
        // No-op until M-18 hooks land.
    },
    mountFor(el) {
        el.addEventListener("click", (ev) => productTreeView.onCommand(ev));
    },
});

function renderTree(node) {
    if (!node) return "";
    const children = (node.children ?? [])
        .map(renderTree)
        .join("");
    const id = node.component_id
        ? `<span class="muted">(${escapeHtml(node.component_id)})</span>`
        : "";
    return `<li>${escapeHtml(node.name)} ${id}${children ? `<ul>${children}</ul>` : ""}</li>`;
}

function renderMapTable(mappings) {
    if (mappings.length === 0) {
        return `<p class="muted">尚无映射 — 调用 <code>seedBus()</code> demo。</p>`;
    }
    return `
        <table class="map-table">
            <thead>
                <tr><th>区间</th><th>类型</th><th>size</th><th>详情</th></tr>
            </thead>
            <tbody>
                ${mappings.map((m) => `
                    <tr data-region-base="${m.base}">
                        <td><code>0x${m.base.toString(16).padStart(8, "0")}</code></td>
                        <td>${escapeHtml(m.type)}</td>
                        <td><code>0x${m.size.toString(16)}</code></td>
                        <td><pre>${escapeHtml(m.detail ?? "")}</pre></td>
                    </tr>
                `).join("")}
            </tbody>
        </table>
        <button data-action="seed-demo">刷新 demo 映射</button>
    `;
}

// ---------------------------------------------------------------------------
// debugView — 反汇编面板 (M-11) + 寄存器/内存 (FR-05/08/09)
// ---------------------------------------------------------------------------

const debugView = register({
    id: "debug",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        const debug = state?.debug ?? {};
        // The disassembly entries come from the kernel's `Disasm` event.
        const entries = Array.isArray(sim.disasm) ? sim.disasm : [];
        // The PC comes from the **existing** state source (M-10 pushes
        // `sim.pc` from `sim.registers` / `sim.state.delta`) — the panel
        // must not invent a second one.
        const pc = Number(sim.pc ?? 0) >>> 0;
        const exportState = debug.export ?? null;
        const traceState = debug.trace ?? null;
        const error = debug.error ?? null;
        // The continuation is offered only while there is **more** to
        // fetch (`next_offset < total`); the offset unit is the **entry
        // count** (STAGE-5 §3.1).
        const canContinue = exportState !== null
            && Number(exportState.next_offset ?? 0) < Number(exportState.total ?? 0);
        return `
            <div class="debug-toolbar">
                <button type="button" data-action="step">单步</button>
                <button type="button" data-action="step10">单步 ×10</button>
                <button type="button" data-action="continue">继续</button>
                <button type="button" data-action="pause">暂停</button>
                <button type="button" data-action="reset">复位</button>
                <button type="button" data-action="read-regs">读寄存器</button>
                <button type="button" data-action="read-mem">读内存 @0x100</button>
                <button type="button" data-action="disasm" data-addr="${pc}" data-count="16">反汇编</button>
                <button type="button" data-action="export-trace" data-source="Execution" data-format="Csv" data-offset="0" data-limit="0">导出执行轨迹 (CSV)</button>
                <button type="button" data-action="export-trace" data-source="Waveform" data-format="Vcd" data-offset="0" data-limit="0">导出波形 (VCD)</button>
                ${canContinue ? `<button type="button" data-action="export-trace" data-source="${escapeHtml(String(exportState.source))}" data-format="${escapeHtml(String(exportState.format))}" data-offset="${Number(exportState.next_offset)}" data-limit="0" data-continue="1">继续导出</button>` : ""}
                <button type="button" data-action="trace-enable">开启 trace 记录</button>
                <button type="button" data-action="trace-disable">关闭 trace 记录</button>
            </div>
            ${traceState?.enabled === true
                ? `<p class="trace-notice" data-trace-notice>trace 记录已开启 —— 仿真会明显降速（每条指令与访存都要记录）。</p>`
                : ""}
            ${error
                ? `<p class="debug-error" data-debug-error>${escapeHtml(String(error.code))}：${escapeHtml(String(error.message))}</p>`
                : ""}
            <div class="debug-grid">
                <section class="disasm">
                    <h3>反汇编 (M-11)</h3>
                    <p class="muted">助记符是 <strong>ISA 助记符</strong>：压缩指令保留 <code>c.</code> 前缀，不做伪指令别名改写（裁决 C）。</p>
                    <table class="disasm-table">
                        <thead><tr><th>地址</th><th>机器码</th><th>助记符</th><th>宽度</th><th>源码</th></tr></thead>
                        <tbody>
                            ${entries.map((entry) => renderDisasmRow(entry, pc)).join("")
                                || `<tr><td colspan="5" class="muted">尚无反汇编 — 点击「反汇编」向内核请求一段（M-11）。</td></tr>`}
                        </tbody>
                    </table>
                </section>
                <section class="trace-export">
                    <h3>trace 导出 (M-11 / M-19)</h3>
                    ${exportState
                        ? `<p class="muted" data-trace-progress>${escapeHtml(String(exportState.source))} / ${escapeHtml(String(exportState.format))} · 条目 ${Number(exportState.offset)} → ${Number(exportState.next_offset)} / ${Number(exportState.total)}</p>`
                        : `<p class="muted">尚未导出 — 选一个数据源与格式发起导出。</p>`}
                    ${exportState?.overflow_marked === true
                        ? `<p class="trace-overflow" data-trace-overflow>⚠️ 导出窗口<strong>不是完整历史</strong> —— 源环溢出过、丢过最旧条目（本次导出从第 ${Number(exportState.offset)} 条开始，更早的内容已不在环里）。</p>`
                        : ""}
                    <pre class="trace-chunk" data-trace-chunk>${escapeHtml(String(exportState?.chunk ?? ""))}</pre>
                </section>
                <section class="regmem">
                    <h3>寄存器 / 内存</h3>
                    <div class="grid-2">
                        <div>
                            <h4>寄存器</h4>
                            <div class="reg-grid">${
                                (sim.gpr ?? Array(32).fill(0)).map((v, i) =>
                                    `<div><span class="muted">x${i}</span><code>${(v >>> 0).toString(16).padStart(8, "0")}</code></div>`
                                ).join("")
                            }</div>
                        </div>
                        <div>
                            <h4>PC</h4>
                            <code class="pc">0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}</code>
                            <h4>虚拟时间</h4>
                            <code class="timeVirt">${state?.timeVirt ?? 0} ps</code>
                        </div>
                    </div>
                </section>
            </div>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        // The command's parameters travel **with the signal** — the
        // bootstrap's listener runs on `window` and cannot see the button.
        return emitCommand(action, "debug", {
            addr: el.dataset.addr,
            count: el.dataset.count,
            source: el.dataset.source,
            format: el.dataset.format,
            offset: el.dataset.offset,
            limit: el.dataset.limit,
        });
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => debugView.onCommand(ev));
    },
});

/**
 * 「**没有**行号」时的**如实**措辞（K-3）。
 *
 * ⚠️ **不许**显示空、**不许**显示 `0`、**不许**显示 `null`——那三种
 * 形态读起来都像「有行号但值不对」，而真相是「**根本没有**行号」。
 */
export const NO_SOURCE_LINE_LABEL = "无行号";

/**
 * 一条反汇编条目的源码位置的**两个部分**（K-3）。
 *
 * ⚠️ **两个都空**才是「无行号」；只有一个有 ⇒ **如实**（别编一个出来）。
 * ⚠️ `line` 是 **1 起**的行号 ⇒ `0` 与负数都**不算**「有行号」。
 *
 * @param {{file?: string|null, line?: number|null}|null|undefined} entry
 * @returns {{file: string, line: number|null}}
 */
export function sourceLocationParts(entry) {
    const rawFile = entry?.file;
    const file = rawFile === null || rawFile === undefined ? "" : String(rawFile).trim();
    const rawLine = entry?.line;
    const lineNum = rawLine === null || rawLine === undefined ? null : Number(rawLine);
    const line = lineNum !== null && Number.isFinite(lineNum) && lineNum > 0 ? lineNum : null;
    return { file, line };
}

/**
 * 把一条反汇编条目的**源码位置**渲染成一行文本。
 *
 * ⚠️ **绝不编造**（K-3）：`file` / `line` 是契约 v4 的两个 `Option` ⇒
 * JS 侧是 `null` / `undefined` ⇒ **两个都空**才是「无行号」；只有一个
 * 有 ⇒ **如实显示那一个**，**不编**另一半出来。
 *
 * @param {{file?: string|null, line?: number|null}|null|undefined} entry
 * @returns {string}
 */
export function formatSourceLocation(entry) {
    const { file, line } = sourceLocationParts(entry);
    if (file && line !== null) return `${file}:${line}`;
    // ⚠️ 只有文件名 ⇒ 如实说「这个文件有，但**行号**没有」。
    if (file) return `${file}（${NO_SOURCE_LINE_LABEL}）`;
    // ⚠️ 只有行号 ⇒ 如实说「**行号**有，但**文件名**没有」。
    if (line !== null) return `（无文件）:${line}`;
    return NO_SOURCE_LINE_LABEL;
}

/**
 * One row of the disassembly table.
 *
 * ⚠️ The mnemonic is shown **verbatim** (HTML-escaped, but otherwise
 * untouched): the kernel renders **ISA** mnemonics — compressed
 * instructions keep their `c.` prefix and there are **no
 * pseudo-instruction aliases** (ruling C). The front end must not
 * "helpfully" rewrite `c.addi sp,-16` into `addi sp,sp,-16`.
 *
 * The row the PC points at is highlighted (V-12).
 *
 * ⚠️ **逐条显示源码位置**（K-3）：`file` / `line` 来自内核的 DWARF 行表
 * （契约 v4 的两个可选字段）⇒ 查不到就是「**无行号**」，**不编造**。
 *
 * @param {{addr?: number, code?: number, mnemonic?: string, width?: number, file?: string|null, line?: number|null}} entry
 * @param {number} pc
 * @returns {string}
 */
function renderDisasmRow(entry, pc) {
    const addr = Number(entry?.addr ?? 0) >>> 0;
    const width = Number(entry?.width ?? 4);
    const code = Number(entry?.code ?? 0) >>> 0;
    const isPc = addr === (Number(pc ?? 0) >>> 0);
    const addrText = `0x${addr.toString(16).padStart(8, "0")}`;
    // The machine code is padded to the instruction's **own** width: a
    // 16-bit compressed instruction shows 4 hex digits, a 32-bit one 8.
    const codeText = `0x${code.toString(16).padStart(width === 2 ? 4 : 8, "0")}`;
    const sourceText = formatSourceLocation(entry);
    // ⚠️ **机器可读**的两个 `data-*`：真机段要断言「`file` 是个真源码
    // 路径」「`line` 大于 0」⇒ 从渲染出来的**文本**里猜是不行的。
    const { file, line } = sourceLocationParts(entry);
    return `
        <tr class="disasm-row${isPc ? " is-pc" : ""}" data-disasm-addr="${addrText}"${isPc ? ' data-pc="1"' : ""}>
            <td><code>${addrText}</code></td>
            <td><code>${codeText}</code></td>
            <td class="disasm-mnemonic">${escapeHtml(entry?.mnemonic ?? "")}</td>
            <td>${width}</td>
            <td class="disasm-source" data-disasm-file="${escapeHtml(file)}" data-disasm-line="${line === null ? "" : String(line)}">${escapeHtml(sourceText)}</td>
        </tr>
    `;
}

// ---------------------------------------------------------------------------
// regmemView — the "寄存器/内存" view (M-10 data).
//
// The panel existed in M-01's view registry from the start, but no
// component was registered for it — the tab rendered an empty body.
// M-10 now feeds the store, so the view has something to show.
// ---------------------------------------------------------------------------

const regmemView = register({
    id: "regmem",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        const gpr = sim.gpr ?? Array(32).fill(0);
        const mem = sim.mem ?? {};
        const cells = Object.entries(mem);
        return `
            <section class="regmem-view">
                <h3>寄存器 / 内存 (M-10)</h3>
                <div class="debug-toolbar">
                    <button type="button" data-action="read-regs">读寄存器</button>
                    <button type="button" data-action="read-mem">读内存 @0x100</button>
                </div>
                <div class="grid-2">
                    <div>
                        <h4>通用寄存器</h4>
                        <div class="reg-grid">${
                            gpr.map((v, i) =>
                                `<div><span class="muted">x${i}</span><code>${(v >>> 0).toString(16).padStart(8, "0")}</code></div>`
                            ).join("")
                        }</div>
                    </div>
                    <div>
                        <h4>PC / 虚拟时间</h4>
                        <code class="pc">0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}</code>
                        <code class="timeVirt">${state?.timeVirt ?? 0} ps</code>
                        <h4>内存单元</h4>
                        ${cells.length === 0
                            ? `<p class="muted">尚无读回的内存单元 — 点击「读内存 @0x100」。</p>`
                            : `<table class="map-table">
                                <thead><tr><th>地址</th><th>值</th></tr></thead>
                                <tbody>${cells.map(([addr, value]) => `
                                    <tr><td><code>${escapeHtml(addr)}</code></td>
                                    <td><code>0x${(value >>> 0).toString(16).padStart(8, "0")}</code></td></tr>
                                `).join("")}</tbody>
                            </table>`}
                    </div>
                </div>
            </section>
        `;
    },
    onCommand(ev) {
        const action = ev.target?.closest?.("[data-action]")?.dataset?.action;
        if (!action) return undefined;
        return emitCommand(action, "regmem");
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => regmemView.onCommand(ev));
    },
});

// ---------------------------------------------------------------------------
// waveformView — 探针通道列表 (D-46).
// ---------------------------------------------------------------------------

const waveformView = register({
    id: "waveform",
    channels: ["wave"],
    render(state) {
        const channels = state?.wave?.channels ?? [];
        return `
            <section class="waveform">
                <h3>波形 (M-19)</h3>
                ${channels.length === 0
                    ? `<p class="muted">尚无探针信号 — 固件驱动外设后（M-15 + M-18 管道已接通）会列出通道。</p>`
                    : `<ul class="channel-list">
                        ${channels.map((c) => `
                            <li>
                                <span class="channel-label">${escapeHtml(c.label || c.id)}</span>
                                <span class="muted">${c.edges ?? 0} edges</span>
                                ${c.lastValue === null || c.lastValue === undefined
                                    ? ""
                                    : `<span class="muted">· 最新值 ${escapeHtml(String(c.lastValue))}</span>`}
                            </li>`).join("")}
                       </ul>`}
            </section>
        `;
    },
    onCommand(_ev) {},
    onDelta(_delta) {},
    mountFor(_el) {},
});

// ---------------------------------------------------------------------------
// sourceView — 源码视图（STAGE-7 裁决 E / F）。
//
// ⚠️ **源码不进内核**（裁决 E）：文件由浏览器原生 `<input type="file">`
// 读成文本 ⇒ 存进 **M-03 视图状态** ⇒ **不发**给内核、**不新增**任何
// 契约命令。整个视图里只有一条命令会离开前端：`resolve-line`（问内核
// 「这个地址对应哪一行」）—— 那条命令**早就存在**（契约 v3 的
// `ResolveLine`），本批只是把它接上。
//
// ⚠️ **装载源码用的是浏览器原生能力**（`<input type="file">`），不是 File
// System Access API：后者只在 Chromium 系可用，会让「零依赖、跨浏览器」
// 的前端出现一条只在部分浏览器工作的路径（裁决 E）。
// ---------------------------------------------------------------------------

const sourceView = register({
    id: "source",
    channels: ["sim"],
    render(state) {
        const src = state?.source ?? {};
        const tree = src.tree ?? {};
        const keys = Object.keys(tree);
        // PC 只能来自**既有**的状态源（M-10 推进的 `sim.pc`）—— 前端不许
        // 自己造一个状态源（与反汇编面板同一条规矩）。
        const pc = Number(state?.sim?.pc ?? 0) >>> 0;
        // 内核回的 `SourceLocation { addr, file, line }`。
        const location = src.location ?? null;
        // 用户在**多个候选**里选定的那个（裁决 F：不静默挑第一个）。
        const selected = src.selected ?? null;

        // 匹配结果在**渲染时**算（纯函数）⇒ 状态里不存第二份真相。
        const match = location?.file ? matchSourceFile(tree, location.file) : null;
        // ⚠️ **三种「没有答案」必须分开**（V-B 检视 VB-5）：
        //   - 还没问              ⇒ `location == null`
        //   - 问了，但**没有 DWARF** ⇒ `file === "" && line === 0`
        //   - 问了，但**地址没命中** ⇒ `file === "" && line === 0xFFFFFFFF`
        // 之前这三种**渲染成同一句「尚未解析」** ⇒ 用户点完按钮看到没变化
        // ⇒ 以为没点生效 ⇒ 无限点下去。内核其实**已经回答了**。
        // （内核的两种形状见 `kernel_host::source_location_event`。）
        const answered = location != null;
        const fileMissing = answered && !location.file;
        const noDwarf = fileMissing && Number(location.line ?? 0) === 0;
        const missed = fileMissing && Number(location.line ?? 0) === 0xFFFFFFFF;
        // ⚠️ **三态如实区分**（裁决 J）：「没有源码树」/「匹配不到」/
        // 「匹配到多个」—— 不合并成一个「无源码」。
        const statusText = keys.length === 0
            ? "尚未装载源码 — 用上面的入口选一个目录或若干文件。"
            : (location?.file
                ? describeMatch(match)
                : (noDwarf
                    ? "该程序没有 DWARF 行表 — 重新编译时加上 -g 才有源码行。"
                    : (missed
                        ? "该地址不在 DWARF 行表内 — 换一个地址试试。"
                        : "尚未解析 — 点击「解析当前 PC 的源码行」。")));
        const activeKey = selected ?? (match?.kind === "unique" ? match.key : null);
        const text = activeKey ? (tree[activeKey] ?? "") : "";
        const line = Number(location?.line ?? 0);

        return `
            <section class="source-view">
                <h3>源码 (M-11 · DWARF 行号)</h3>
                <div class="debug-toolbar">
                    <label class="source-input">
                        装载目录
                        <input type="file" webkitdirectory multiple data-source-input="dir">
                    </label>
                    <label class="source-input">
                        装载文件
                        <input type="file" multiple data-source-input="files">
                    </label>
                    <button type="button" data-action="resolve-line" data-addr="${pc}">解析当前 PC 的源码行</button>
                </div>
                <p class="muted" data-source-summary>
                    源码树：${keys.length} 个文件${keys.length > 0 ? `（${escapeHtml(keys.slice(0, 3).join("、"))}${keys.length > 3 ? " …" : ""}）` : ""}
                </p>
                <p class="source-status" data-source-status>${escapeHtml(statusText)}</p>
                ${location
                    ? (location.file
                        ? `<p class="source-location" data-source-location>${escapeHtml(String(location.file))}:${Number(location.line ?? 0)}（地址 0x${(Number(location.addr ?? 0) >>> 0).toString(16).padStart(8, "0")}）</p>`
                        // ⚠️ 内核**回答了**但没有文件名 ⇒ **不**显示
                        // `:4294967295` 这种无意义行号（V-B 检视 VB-5）。
                        : `<p class="source-location" data-source-location>（地址 0x${(Number(location.addr ?? 0) >>> 0).toString(16).padStart(8, "0")} — 内核没有给出文件名）</p>`)
                    : `<p class="muted" data-source-location>尚未解析 — 点击「解析当前 PC 的源码行」。</p>`}
                ${match?.kind === "ambiguous"
                    ? `<ul class="source-candidates" data-source-candidates>${match.candidates.map((key) => `
                        <li><button type="button" data-action="pick-candidate" data-key="${escapeHtml(key)}">${escapeHtml(key)}</button></li>
                    `).join("")}</ul>`
                    : ""}
                ${activeKey
                    ? `<pre class="source-code" data-source-code>${renderSourceText(text, line)}</pre>`
                    : `<pre class="source-code muted" data-source-code>选中一个源码文件后显示内容。</pre>`}
            </section>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        if (action === "resolve-line") {
            // 问内核「这个地址对应哪一行」（契约 v3 的 `ResolveLine`）。
            return emitCommand("resolve-line", "source", { addr: el.dataset.addr });
        }
        if (action === "pick-candidate") {
            // 多个候选 ⇒ **让用户选**（裁决 F：⛔ 不静默挑第一个）。
            // 这是**纯前端**的选择 ⇒ 不发命令。
            viewStore.setState("source", (prev) => ({
                ...prev,
                source: { ...(prev.source ?? {}), selected: el.dataset.key ?? null },
            }));
            publishSourceState();
            return "cmd-pick-candidate";
        }
        return undefined;
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => sourceView.onCommand(ev));
        el.addEventListener("change", (ev) => {
            const input = ev.target?.closest?.("[data-source-input]");
            if (!input) return;
            void loadSourceFiles(input.files);
        });
    },
});

/**
 * 读用户选中的文件 ⇒ 存进 M-03 视图状态。
 *
 * ⚠️ **不进内核**（裁决 E）：这里**不发**任何命令 / 帧。
 *
 * @param {FileList|ArrayLike|null|undefined} fileList
 */
async function loadSourceFiles(fileList) {
    const files = [...(fileList ?? [])];
    if (files.length === 0) return;
    /** @type {Record<string, string>} */
    const tree = {};
    for (const file of files) {
        // 目录选择器给的是相对路径（`webkitRelativePath`）；多文件选择器
        // 只有文件名。
        const key = file.webkitRelativePath || file.name;
        try {
            tree[key] = await file.text();
        } catch (_err) {
            // 读不出来就**跳过**（不假装读到了）。
        }
    }
    viewStore.setState("source", (prev) => ({
        ...prev,
        source: {
            ...(prev.source ?? {}),
            // 新选的**并入**已有的树（不丢用户已经装好的文件）。
            tree: { ...(prev.source?.tree ?? {}), ...tree },
        },
    }));
    publishSourceState();
}

/**
 * 让源码视图按**当前状态**重渲染。
 *
 * ⚠️ 走的是 M-01 的 `riscv:render` 事件（与 `app.js` 的
 * `publishViewState` 同一条路）⇒ 视图层不直接摸 DOM。
 */
function publishSourceState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "source", type: "update", state: viewStore.getState("source") },
    }));
}

// ---------------------------------------------------------------------------
// terminalView — UART echo placeholder, XSS-safe.
// ---------------------------------------------------------------------------

/**
 * console 字节数组的**分块大小**（每次 `String.fromCharCode` 展开的字节数）。
 *
 * ⚠️ **为什么要分块**：`String.fromCharCode(...bytes)` 把**每个字节**当成
 * 一个**实参**传给函数——几万个字节会**爆调用栈**（实参个数与栈帧都有限）。
 * 真实 RTOS 的 console 一次能吐几万个字节 ⇒ 必须分块。
 *
 * 4096 是个安全的取值：远低于任何引擎的实参上限，而块数又少到不值得
 * 为它做更细的调优。
 */
export const CONSOLE_DECODE_CHUNK = 4096;

/**
 * 把 console 的**字节数组**解成字符串。
 *
 * ⚠️ **字节，不是字符**：guest 可能吐出任意字节（ANSI 转义、`\r`、半个
 * UTF-8 序列）⇒ 这里做的是「字节 → 码位」的一一映射（`String.fromCharCode`），
 * **不**做 UTF-8 解码——**字节流才是真相**（与 Spike 的逐字节比对比的就是它）。
 *
 * ⚠️ **字符串原样返回**：既有的 `onDelta({ termDelta: { bytes: "<x>&y" } })`
 * 走的是字符串路径（**既有渲染语义不许丢**）⇒ 这里不碰它。
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

const terminalView = register({
    id: "terminal",
    channels: ["term"],
    render(state) {
        const buf = state?.term?.bytes ?? "";
        // ⚠️ **草稿住在视图状态里**（K-1）：`applyRenderEvent` 每次都
        // `body.innerHTML = html` ⇒ **输入框元素被重建** ⇒ 只存在 DOM 里
        // 的草稿会**每来一个 console 字节就清一次** ⇒ 根本没法打字。
        const draft = state?.term?.draft ?? "";
        return `
            <section class="terminal">
                <h3>串口终端 (M-15 UART)</h3>
                <pre class="terminal-out" data-terminal-out>${escapeHtml(buf)}</pre>
                <div class="terminal-input-row">
                    <input type="text" class="terminal-input" data-terminal-input value="${escapeHtml(draft)}" placeholder="输入后按「发送」——字节会进 guest 的 UART0">
                    <button type="button" class="terminal-send" data-action="console-send">发送</button>
                </div>
                <p class="muted">输入框里的文本<strong>按 UTF-8 编码</strong>成字节，发给 guest 的 UART0（契约 v5 的 <code>WriteConsole</code>）；<strong>回显来自 guest</strong>，平台不做本地回显。本批只送原始字节，不模拟终端控制序列。</p>
            </section>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (action !== "console-send") return undefined;
        return sendTerminalInput();
    },
    /** Append new bytes to the terminal buffer in-place (T1 fast path). */
    onDelta(delta) {
        if (!delta?.termDelta) return;
        // ⚠️ **字节数组也要能吃**：契约 v4 的 `Console` 事件的载荷是
        // `bytes: [104, 105]`（JSON **字节数组**，不是字符串）⇒ 这里解码成
        // 字符串再喂给 store（store 里的 `term.bytes` 一直是**字符串**）。
        //
        // ⚠️ 解码是**分块**的（`decodeConsoleBytes`）：不分块的话几万个字节
        // 会爆调用栈。
        const bytes = decodeConsoleBytes(delta.termDelta.bytes ?? "");
        // The store is the single source of truth: the DOM is rewritten
        // from the accumulated value instead of appending to whatever the
        // DOM currently holds. Appending to the DOM would duplicate the
        // bytes whenever a re-render rebuilds the body from the store
        // (FRONTEND_Review.md R-07).
        const next = (viewStore.getState("terminal")?.term?.bytes ?? "") + bytes;
        viewStore.setState("terminal", (prev) => ({
            ...prev,
            term: {
                ...(prev.term ?? {}),
                bytes: next,
            },
        }));
        const out = document.querySelector("[data-terminal-out]");
        if (!out) return;
        // `textContent` does not parse HTML — escaping here would print a
        // literal "&lt;" (FRONTEND_Review.md #4).
        out.textContent = next;
        out.scrollTop = out.scrollHeight;
    },
    mountFor(el) {
        // ⚠️ **事件委托**（K-1）：监听器绑在**容器**上，**不**绑在输入框上
        // ——输入框元素每次重渲染都会被重建（`body.innerHTML = html`）⇒
        // 绑在它身上的监听器**会丢**（点第二次就没反应了）。
        el.addEventListener("click", (ev) => terminalView.onCommand(ev));
        el.addEventListener("input", (ev) => {
            const input = ev.target?.closest?.("[data-terminal-input]");
            if (!input) return;
            // 草稿进**视图状态**（不是 DOM）⇒ 重渲染也不会丢。
            setTerminalDraft(input.value ?? "");
        });
        el.addEventListener("keydown", (ev) => {
            const input = ev.target?.closest?.("[data-terminal-input]");
            if (!input) return;
            if (ev.key !== "Enter") return;
            // 「Enter 键发送」是**可选**的加分项（K-1 §7）；送的仍然是
            // **原始字节**（不追加换行——那不是终端控制序列）。
            ev.preventDefault?.();
            sendTerminalInput();
        });
    },
});

/**
 * 把终端草稿写进 **M-03 视图状态**（`term.draft`）。
 *
 * ⚠️ **不在这里重渲染**：每敲一个键就重渲染会把输入框重建一次（焦点
 * 都没了）⇒ 草稿的**写入**与**重渲染**必须分开。
 *
 * @param {unknown} text
 */
function setTerminalDraft(text) {
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), draft: String(text ?? "") },
    }));
}

/**
 * 让终端视图按**当前状态**重渲染。
 *
 * ⚠️ 走的是 M-01 的 `riscv:render` 事件（与源码视图的
 * `publishSourceState` 同一条路）⇒ 视图层不直接摸 DOM。
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
 * 把输入框里的文本**按 UTF-8 编码**成字节 ⇒ 发 `console-send` 信号
 * ⇒ 清空输入框。
 *
 * ⚠️ **字节，不是字符**（K-1）：guest 的 UART 收的是**字节**（契约 v5 的
 * `WriteConsole { bytes: Vec<u8> }`）⇒ 这里用 `TextEncoder` 做 UTF-8 编码
 * （`"A"` ⇒ `[65]`、`"你"` ⇒ `[228, 189, 160]`），**不**送字符串（内核会
 * 按 `E_PROTO-08` 拒掉）。
 *
 * ⚠️ **先发命令，再清草稿**：清早了就把要发的内容一起清掉了。
 *
 * ⚠️ **视图层不许直接调服务层**（本模块的既有规矩）：这里只发信号，
 * `app.js` 把它变成真的命令。
 *
 * @returns {string} the request id
 */
function sendTerminalInput() {
    const input = document.querySelector("[data-terminal-input]");
    const text = String(input?.value ?? "");
    const bytes = Array.from(new TextEncoder().encode(text));
    const requestId = emitCommand("console-send", "terminal", { bytes });
    setTerminalDraft("");
    publishTerminalState();
    return requestId;
}

// ---------------------------------------------------------------------------
// Render dispatcher — receives M-01 `riscv:render` CustomEvents.
// ---------------------------------------------------------------------------

/**
 * @param {{ viewId: string, type: string, state: object }} detail
 */
function applyRenderEvent(detail) {
    const c = components.get(detail.viewId);
    if (!c) return;
    // Locate the per-view mount body and re-render into it.
    const body = document.querySelector(`[data-view-body="${detail.viewId}"]`);
    if (!body) return;
    let html = "";
    try {
        html = c.render(detail.state ?? {});
    } catch (err) {
        html = `<div class="view-error">渲染失败：${escapeHtml(err?.message ?? String(err))}</div>`;
        console.warn(`[M-02 ${detail.viewId}] render error:`, err);
    }
    body.innerHTML = html;
}

/**
 * Bind a view component to its DOM panel the first time it is mounted.
 */
function attachMount(viewId) {
    const c = components.get(viewId);
    if (!c) return;
    const body = document.querySelector(`[data-view-body="${viewId}"]`);
    if (!body || body.dataset.mounted === "1") return;
    body.dataset.mounted = "1";
    c.mountFor(body);
}

// ---------------------------------------------------------------------------
// Bootstrap
// ---------------------------------------------------------------------------

/** @returns {Promise<void>} */
export async function mountViewComponents() {
    // Wire render events to all registered components.
    window.addEventListener("riscv:render", (ev) => {
        const detail = ev.detail;
        if (!detail || !detail.viewId) return;
        attachMount(detail.viewId);
        applyRenderEvent(detail);
    });
}

// ---------------------------------------------------------------------------
// Dev-only seed signal — the production layer never builds demo data
// itself, it only asks for it (FRONTEND_Review.md #15).
// ---------------------------------------------------------------------------

/**
 * Ask the dev layer to (re)seed the demo data.
 *
 * M-02 must not know how demo data is produced, so it only emits the
 * `riscv:seed` signal; the dev wiring in app.js turns that signal into
 * store state. The demo data itself lives in `front/dev/demo-state.js`.
 *
 * @param {string} [viewId]
 */
export function seedBus(viewId = "product") {
    window.dispatchEvent(new CustomEvent("riscv:seed", { detail: { viewId } }));
}
