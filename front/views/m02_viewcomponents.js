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
import { refreshAll } from "./m01_workbench.js";

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

export function escapeHtml(s) {
    return String(s)
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
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
        // User action: toggle a memory-mapped region's readonly flag.
        const target = /** @type {HTMLElement} */ (ev.target);
        const row = target.closest?.("[data-region-base]");
        if (row && target.matches("[data-action='seed-demo']")) {
            seedDemoBus();
            refreshAll();
            return `cmd-seed-demo`;
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
        return `<p class="muted">尚无映射 — 调用 <code>seedDemoBus()</code> demo。</p>`;
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
// debugView — disasm + regs/mem stub (FR-05/08/09, MVP stub only).
// ---------------------------------------------------------------------------

const debugView = register({
    id: "debug",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        return `
            <div class="debug-grid">
                <section class="disasm">
                    <h3>反汇编 (M-11)</h3>
                    <p class="muted">DWARF 解析属 M-5（v0.3 决议，已纳入 MVP）。</p>
                    <table class="disasm-table">
                        <thead><tr><th>PC</th><th>指令</th><th>字</th></tr></thead>
                        <tbody>
                            ${(sim.disasm ?? []).map((row) => `
                                <tr>
                                    <td><code>0x${row.pc.toString(16).padStart(8, "0")}</code></td>
                                    <td>${escapeHtml(row.inst ?? "?")}</td>
                                    <td><code>0x${row.raw.toString(16).padStart(8, "0")}</code></td>
                                </tr>
                            `).join("") || `<tr><td colspan="3" class="muted">尚无指令（M-13 跑通后会填充）</td></tr>`}
                        </tbody>
                    </table>
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
        const action = ev.target?.closest?.("[data-action]")?.dataset?.action;
        if (action === "step") {
            // Will route to M-05 dispatchCommand once transport lands.
            console.info("[M-02 debugView] step requested (no-op until M-07)");
        }
        return undefined;
    },
    onDelta(_delta) {},
    mountFor(_el) {},
});

// ---------------------------------------------------------------------------
// waveformView — placeholder for FR-10/14. Renders channel list.
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
                    ? `<p class="muted">尚无探针信号 — M-15 + M-18 接入后会列出通道。</p>`
                    : `<ul class="channel-list">
                        ${channels.map((c) => `
                            <li>
                                <span class="channel-label">${escapeHtml(c.label || c.id)}</span>
                                <span class="muted">${c.edges ?? 0} edges</span>
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
        const out = document.querySelector("[data-terminal-out]");
        if (!out) return;
        const bytes = delta.termDelta.bytes ?? "";
        out.textContent = (out.textContent ?? "") + escapeHtml(bytes);
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
// Dev-only demo seeder — populates productTreeView with one RAM region
// emulating the M-14 kernel side. The real binding lands once M-07
// transport is wired up.
// ---------------------------------------------------------------------------

/**
 * Build a fake `sim.memoryMap` and a tiny product tree that mirrors
 * what `m14_bus::Bus::regions()` would return after a typical
 * "intelligent light" load.
 */
export function buildDemoStoreState() {
    return {
        timeVirt: 0,
        version: 1,
        sim: {
            pc: 0x0,
            gpr: Array(32).fill(0),
            memoryMap: [
                { base: 0x0000_0000, size: 0x1_0000, type: "RAM", detail: "[0, 64 KiB) firmware image" },
                { base: 0x2000_0000, size: 0x1000, type: "PERIPHERAL", detail: "UART0 (id=1, 4 KiB)" },
                { base: 0x4000_0000, size: 0x1000, type: "PERIPHERAL", detail: "GPIO0 (id=2, 4 KiB)" },
            ],
            disasm: [],
        },
        wave: { channels: [] },
        term: { bytes: "" },
        events: {},
        tree: {
            root: {
                name: "MainBoard",
                component_id: "rv-sim",
                children: [
                    { name: "GPIO0", component_id: "GPIO0", children: [] },
                    { name: "UART0", component_id: "UART0", children: [] },
                ],
            },
        },
    };
}

/** Convenience exported handle — also reachable from `window.riscv_demo`. */
export function seedBus() {
    window.dispatchEvent(new CustomEvent("riscv:seed", { detail: buildDemoStoreState() }));
}
