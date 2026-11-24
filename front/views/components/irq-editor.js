/**
 * **外设中断号汇总面** —— 一屏列出所有接 PLIC 的器件，提前暴露源号冲突。
 *
 * ⚠️ **只读**（v0.143）：值仍在右栏「器件参数」里改（`data-device-param` 那
 * 一路采集）⇒ ⇒ 这里**不再**放第二个输入框 ⇒ ⇒ 同一个量**只有一个编辑面**
 * （两个输入框各存一份 DOM 值 ⇒ 提交时后写的那份覆盖前一份 ⇒ 用户改一处、
 * 另一处的旧值把改动冲掉）。
 *
 * ⚠️ **判据与内核逐字同口径**（`contract/platform.rs` V-15 / V-15b）：
 * - 参与判定的器件 = `uart16550` / `gpio` / `timer_riscv`（三个外设一条规则）；
 * - 越界：有 PLIC 时 `irq_line` 必须落在 `1..sources-1`（`0` 是保留号；
 *   `>= sources` 不存在那个源）⇒ `E_PLT-12`；
 * - 重复：同一块板上一个源号只能属于一个外设 ⇒ `E_PLT-16`。
 * ⇒ ⇒ 这里**只做提示**，不阻拦、不改值：判定权仍在内核（前端不猜）。
 *
 * ⚠️ **行键 = 产品树的节点键**（`r<段序号>`）：草稿就是产品树那一份 ⇒ 同一
 * 份 `regions` ⇒ 点一行能直接选中右栏那个器件。
 *
 * ⚠️ **钩子名是 `data-irq-key`，不是 `data-device-key`**：后者是**产品树**
 * 节点的钩子（`[data-device-key]` 就是「树里有几个节点」的判据）⇒ ⇒ 汇总面
 * 复用它会**冒充树节点**（一屏多出 N 个假节点）。
 */

import { escapeHtml } from "../../lib/escape.js";

const LABELS = Object.freeze({
    uart16550: "UART16550",
    gpio: "GPIO",
    timer_riscv: "定时器 TimerRiscv",
});

/** 参与中断号判定的器件类型（与内核 V-15 那条 match 三分支**同集合**）。 */
const IRQ_KINDS = Object.freeze(["uart16550", "gpio", "timer_riscv"]);

/**
 * 一行中断号的说明文字（语义写在这里，不散落到模板里）。
 *
 * @param {string} kind
 * @returns {string}
 */
function noteFor(kind) {
    if (kind === "uart16550") return "RX / TX 共用一根中断线";
    if (kind === "timer_riscv") return "内部比较线，仍占一个 PLIC 源号";
    return "整块 GPIO 共用一个源号";
}

/**
 * 草稿里那张 PLIC 的源号上界（没有 PLIC ⇒ `null` ⇒ 不判越界）。
 *
 * ⚠️ V-27 保证**至多一个** PLIC ⇒ 取它自己的 `sources`（不是「多块里最小」）。
 *
 * @param {any[]} regions
 * @returns {number|null}
 */
function plicSources(regions) {
    for (const region of regions) {
        if (!region || region.kind !== "peripheral") continue;
        const device = region.device ?? {};
        if (String(device.device ?? "") !== "plic") continue;
        const n = Number(device.sources);
        if (Number.isFinite(n)) return n;
    }
    return null;
}

/**
 * 把草稿里的中断外设读成行（含冲突判定）。
 *
 * @param {object|null} draft
 * @returns {{rows: Array<object>, conflictCount: number, sources: number|null}}
 */
export function irqRows(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const sources = plicSources(regions);
    /** @type {Array<object>} */
    const rows = [];
    regions.forEach((region, index) => {
        if (!region || region.kind !== "peripheral") return;
        const device = region.device ?? {};
        const kind = String(device.device ?? "");
        if (!IRQ_KINDS.includes(kind)) return;
        const raw = device.irq_line;
        const line = Number(raw);
        rows.push({
            index,
            key: `r${index}`,
            id: String(device.id ?? ""),
            kind,
            line: Number.isFinite(line) ? line : (kind === "gpio" ? 11 : 0),
            hasLine: raw !== undefined && raw !== null && String(raw) !== "",
            range: false,
            duplicateOf: null,
        });
    });

    // ---- 越界：只在有 PLIC 时判（无 PLIC ⇒ irq_line 无意义但无害）--------
    if (sources !== null) {
        const upper = sources > 0 ? sources - 1 : 0;
        for (const row of rows) {
            row.range = row.line === 0 || (sources > 0 && row.line >= sources) || upper < 1;
        }
    }

    // ---- 重复：一个源号只能属于一个外设（两行都标）----------------------
    for (let i = 0; i < rows.length; i += 1) {
        for (let j = i + 1; j < rows.length; j += 1) {
            if (rows[i].line !== rows[j].line) continue;
            if (!rows[i].hasLine || !rows[j].hasLine) continue;
            if (rows[i].duplicateOf === null) rows[i].duplicateOf = rows[j].id;
            if (rows[j].duplicateOf === null) rows[j].duplicateOf = rows[i].id;
        }
    }

    const conflictCount = rows.filter((r) => r.range || r.duplicateOf !== null).length;
    return { rows, conflictCount, sources };
}

/**
 * 一行的状态文案（冲突优先：重复比越界更具体）。
 *
 * @param {object} row
 * @returns {string}
 */
function statusFor(row) {
    if (row.duplicateOf !== null) return `重复：与 ${row.duplicateOf} 同号`;
    if (row.range) return "越界：不在 PLIC 的源号范围内";
    if (!row.hasLine) return "未填写";
    return "—";
}

/**
 * 中断号汇总面的 HTML（没有草稿 / 没有中断外设时也给出一个占位）。
 *
 * @param {{draft?: object|null, selectedKey?: string|null}} args
 * @returns {string}
 */
export function renderIrqEditor({ draft, selectedKey } = {}) {
    const { rows, conflictCount, sources } = irqRows(draft);
    const rangeText = sources === null
        ? "板上没有 PLIC：源号只登记、不判范围"
        : `PLIC 源号 1 – ${Math.max(sources - 1, 0)}（0 是保留号）`;
    const body = rows.length
        ? rows.map((row) => {
            const conflict = row.duplicateOf !== null ? "duplicate" : (row.range ? "range" : "none");
            const active = String(selectedKey ?? "") === row.key;
            return `
            <tr class="${active ? "is-active" : ""}"
                data-irq-row data-irq-key="${escapeHtml(row.key)}"
                data-irq-conflict="${conflict}" data-irq-line="${row.line}">
                <td>
                    <button type="button" class="pw-link" data-action="select-irq-device"
                            data-irq-key="${escapeHtml(row.key)}">${escapeHtml(row.id || "(未命名)")}</button>
                </td>
                <td>${escapeHtml(LABELS[row.kind] ?? row.kind)}</td>
                <td class="pw-mono">${escapeHtml(String(row.line))}</td>
                <td class="${conflict === "none" ? "muted" : "pw-irq-bad"}">${escapeHtml(statusFor(row))}</td>
                <td class="muted">${escapeHtml(noteFor(row.kind))}</td>
            </tr>`;
        }).join("")
        : `<tr><td colspan="5" class="pw-table-empty">暂无中断外设（UART / GPIO / 定时器）</td></tr>`;
    return `
        <section class="pw-irq" data-irq-editor>
            <div class="pw-regs-head">
                <h4>外设中断号</h4>
                <span class="muted" data-irq-sources>${escapeHtml(rangeText)}</span>
                <span class="${conflictCount ? "pw-irq-bad" : "muted"}" data-irq-conflict-count="${conflictCount}">
                    ${rows.length} 个${conflictCount ? ` · ${conflictCount} 处冲突` : ""}
                </span>
            </div>
            <p class="pw-tip">值在右栏「器件参数 · PLIC 源号」里改；同一块板上一个源号只能属于一个外设。</p>
            <table class="pw-table pw-table-irq">
                <colgroup>
                    <col style="width:18%"><col style="width:20%"><col style="width:12%">
                    <col style="width:26%"><col style="width:24%">
                </colgroup>
                <thead><tr><th>器件</th><th>类型</th><th>PLIC 源号</th><th>状态</th><th>说明</th></tr></thead>
                <tbody data-irq-rows>${body}</tbody>
            </table>
        </section>
    `;
}
