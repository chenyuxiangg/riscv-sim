/**
 * **外设中断号编辑面** —— 每个中断外设一行，直接改它接 PLIC 的源号。
 *
 * ⚠️ 这里**只渲染**：输入框沿用板级内存图里那套 `data-mmap-device-field`
 * 采集协议（`data-mmap-index` = 段序号）⇒ ⇒ 提交时由
 * `collectMemoryMapDraft` 统一写回 `region.device.irq_line`。
 *
 * ⚠️ 只列**接 PLIC 的**器件（UART16550 / GPIO）： Timer 的 `irq_line`
 * 仍是内部比较线，不出现在这张表里。
 */

import { escapeHtml } from "../../lib/escape.js";

const LABELS = Object.freeze({
    uart16550: "UART16550",
    gpio: "GPIO",
});

const IRQ_KINDS = Object.freeze(["uart16550", "gpio"]);

/**
 * 一行中断号的说明文字（语义写在这里，不散落到模板里）。
 *
 * @param {string} kind
 * @returns {string}
 */
function noteFor(kind) {
    return kind === "uart16550"
        ? "RX 与 TX-empty 共用这一个源号（真实 16550 只有一根 IRQ 线）"
        : "整块 GPIO 一个源号：任一引脚有事件就抬这个源号";
}

/**
 * @param {{draft?: object|null}} args
 * @returns {string}
 */
export function renderIrqEditor({ draft } = {}) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const rows = [];

    regions.forEach((region, index) => {
        if (!region || region.kind !== "peripheral") return;
        const device = region.device ?? {};
        const kind = String(device.device ?? "");
        if (!IRQ_KINDS.includes(kind)) return;
        const value = Number.isFinite(Number(device.irq_line))
            ? Number(device.irq_line)
            : 11;
        rows.push(`
            <tr>
                <td>${escapeHtml(String(device.id ?? ""))}</td>
                <td>${escapeHtml(LABELS[kind] ?? kind)}</td>
                <td>
                    <input class="input" type="number" min="1" step="1"
                        data-mmap-device-field="irq_line"
                        data-mmap-index="${index}"
                        value="${value}">
                </td>
                <td class="muted">${escapeHtml(noteFor(kind))}</td>
            </tr>`);
    });

    return `
        <div class="card" data-irq-editor>
            <h3>外设中断号<span class="tag">${rows.length} 个</span></h3>
            <p class="muted">
                每个外设一个 PLIC 源号（<code>1..sources</code>，<code>0</code> 保留）——
                同一块板内不允许重复；装载时生效。
            </p>
            <table class="map-table" data-irq-rows>
                <thead><tr><th>器件</th><th>类型</th><th>PLIC 源号</th><th>说明</th></tr></thead>
                <tbody>${rows.join("") || `<tr><td colspan="4" class="muted">还没有中断外设（UART / GPIO）。</td></tr>`}</tbody>
            </table>
        </div>
    `;
}
