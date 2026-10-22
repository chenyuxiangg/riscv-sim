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
                        <thead><tr><th>地址</th><th>机器码</th><th>助记符</th><th>宽度</th></tr></thead>
                        <tbody>
                            ${entries.map((entry) => renderDisasmRow(entry, pc)).join("")
                                || `<tr><td colspan="4" class="muted">尚无反汇编 — 点击「反汇编」向内核请求一段（M-11）。</td></tr>`}
                        </tbody>
                    </table>
                </section>
                <section class="trace-export">
                    <h3>trace 导出 (M-11 / M-19)</h3>
                    ${exportState
                        ? `<p class="muted" data-trace-progress>${escapeHtml(String(exportState.source))} / ${escapeHtml(String(exportState.format))} · 条目 ${Number(exportState.offset)} → ${Number(exportState.next_offset)} / ${Number(exportState.total)}</p>`
                        : `<p class="muted">尚未导出 — 选一个数据源与格式发起导出。</p>`}
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
 * @param {{addr?: number, code?: number, mnemonic?: string, width?: number}} entry
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
    return `
        <tr class="disasm-row${isPc ? " is-pc" : ""}" data-disasm-addr="${addrText}"${isPc ? ' data-pc="1"' : ""}>
            <td><code>${addrText}</code></td>
            <td><code>${codeText}</code></td>
            <td class="disasm-mnemonic">${escapeHtml(entry?.mnemonic ?? "")}</td>
            <td>${width}</td>
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
// terminalView — UART echo placeholder, XSS-safe.
// ---------------------------------------------------------------------------

const terminalView = register({
    id: "terminal",
    channels: ["term"],
    render(state) {
        const buf = state?.term?.bytes ?? "";
        return `
            <section class="terminal">
                <h3>串口终端 (M-15 UART)</h3>
                <pre class="terminal-out" data-terminal-out>${escapeHtml(buf)}</pre>
                <p class="muted">MVP 阶段 UART 行为留到 M-15（FIQ 第 3 步）；此处只显示回显缓冲。</p>
            </section>
        `;
    },
    onCommand(_ev) {},
    /** Append new bytes to the terminal buffer in-place (T1 fast path). */
    onDelta(delta) {
        if (!delta?.termDelta) return;
        const bytes = delta.termDelta.bytes ?? "";
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
    mountFor(el) {},
});

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
