// 器件类型注册表 + 逐实例状态的**渲染**（STAGE-43 · v0.66 · 台账 D-175）。
//
// # 这个模块管什么
//
// 产品视图的两张卡：
//   - 「器件类型（内核支持）」—— **内核实现了哪些类型**：窗口大小 ·
//     寄存器**偏移表** · 实例参数的合法区间；
//   - 「器件实例（逐实例）」—— **每个实例**绑到了哪个基址（`base: null`
//     ⇒ **尚未生效**）。
//
// # 数据从哪来
//
// ⚠️ **只从内核来**（契约 v10 的两条只读通道）⇒ ⇒ 前端**不**维护自己的
// 器件类型清单、**不**写第二份参数区间表 ⇒ ⇒ 没有第二个真相源。
//
// # 空态分得开
//
// ⚠️ `null`（还没收到事件）vs `[]`（收到但一个都没有）—— **两句话不一样**
// ⇒ ⇒ 「还没连上内核」与「内核说没有器件」**不**长得一样。

import { escapeHtml } from "../../lib/escape.js";

/**
 * 十六进制（`0x` + 8 位）—— 与内存映射表那一份**同形**。
 *
 * @param {number} value
 * @returns {string}
 */
function hex32(value) {
    return `0x${(Number(value) >>> 0).toString(16).padStart(8, "0")}`;
}

/**
 * 「器件类型（内核支持）」那张卡。
 *
 * ⚠️ **空态两句话**：「还没收到内核的类型表」vs「内核说没有类型」。
 *
 * @param {Array<object>|null} types — `DeviceTypeInfo` 列表（`null` = 还没收到）
 * @returns {string} HTML 片段
 */
export function renderDeviceTypeCard(types) {
    if (types === null || types === undefined) {
        return `
            <section class="card">
                <h3>器件类型<span class="tag">内核支持</span></h3>
                <p class="muted" data-device-types="no-data">
                    尚未收到器件类型表 —— 内核还没有推送（订阅 <code>device.types</code> 后自动更新）。
                </p>
            </section>
        `;
    }
    if (!Array.isArray(types) || types.length === 0) {
        return `
            <section class="card">
                <h3>器件类型<span class="tag">内核支持</span></h3>
                <p class="muted" data-device-types="empty">内核报告：没有任何器件类型。</p>
            </section>
        `;
    }
    const rows = types
        .map((type) => {
            const kind = String(type?.kind ?? "");
            const windowBytes = Number(type?.window_bytes ?? 0);
            const registers = Array.isArray(type?.registers) ? type.registers : [];
            const params = Array.isArray(type?.params) ? type.params : [];
            const regText = registers.length > 0
                ? registers
                    .map((reg) => `${String(reg?.name ?? "")}@${hex32(Number(reg?.offset ?? 0))}`)
                    .join(" · ")
                : "（无）";
            const paramText = params.length > 0
                ? params
                    .map((param) => {
                        const min = param?.min === null || param?.min === undefined ? null : Number(param.min);
                        const max = param?.max === null || param?.max === undefined ? null : Number(param.max);
                        const lo = min === null ? "−" : String(min);
                        const hi = max === null ? "∞" : String(max);
                        return `${String(param?.name ?? "")}∈[${lo}, ${hi}]`;
                    })
                    .join(" · ")
                : "（无）";
            return `
                <tr data-device-type="${escapeHtml(kind)}"
                    data-device-window="${String(windowBytes)}">
                    <td><code>${escapeHtml(kind)}</code></td>
                    <td>${escapeHtml(hex32(windowBytes))}</td>
                    <td class="device-regs">${escapeHtml(regText)}</td>
                    <td class="device-params">${escapeHtml(paramText)}</td>
                </tr>
            `;
        })
        .join("");
    return `
        <section class="card">
            <h3>器件类型<span class="tag">内核支持 · ${types.length} 种</span></h3>
            <p class="muted">
                ⚠️ **寄存器只定义偏移**（不含基址）⇒ ⇒ 集成时绑定到**任意基址**；
                参数合法区间**由内核给出** ⇒ ⇒ 前端**不**自己判。
            </p>
            <table class="map-table" data-device-types="rows">
                <thead><tr><th>类型</th><th>窗口</th><th>寄存器（偏移）</th><th>参数合法区间</th></tr></thead>
                <tbody>${rows}</tbody>
            </table>
        </section>
    `;
}

/**
 * 「器件实例（逐实例）」那张卡。
 *
 * ⚠️ **逐实例**（裁决 B）—— 每个实例一行：id · 类型 · **绑定的基址** ·
 * 窗口大小。
 *
 * ⚠️ **待生效的实例**（`base === null`）**显式**标出来 —— 不是空着
 * （规格 §5 的 N-3）。
 *
 * @param {Array<object>|null} instances — `DeviceInstanceInfo` 列表
 * @returns {string} HTML 片段
 */
export function renderDeviceInstanceCard(instances) {
    if (instances === null || instances === undefined) {
        return `
            <section class="card">
                <h3>器件实例<span class="tag">逐实例</span></h3>
                <p class="muted" data-device-instances="no-data">
                    尚未收到器件实例状态 —— 内核还没有推送（订阅 <code>device.instances</code> 后自动更新）。
                </p>
            </section>
        `;
    }
    if (!Array.isArray(instances) || instances.length === 0) {
        return `
            <section class="card">
                <h3>器件实例<span class="tag">逐实例</span></h3>
                <p class="muted" data-device-instances="empty">内核报告：当前没有器件实例。</p>
            </section>
        `;
    }
    const rows = instances
        .map((inst) => {
            const id = String(inst?.id ?? "");
            const kind = String(inst?.kind ?? "");
            const bound = inst?.base !== null && inst?.base !== undefined;
            const baseText = bound ? hex32(Number(inst.base)) : "待生效";
            const windowBytes = Number(inst?.window_bytes ?? 0);
            return `
                <tr data-device-instance="${escapeHtml(id)}"
                    data-device-instance-kind="${escapeHtml(kind)}"
                    data-device-instance-base="${bound ? String(Number(inst.base)) : ""}"
                    data-device-instance-pending="${bound ? "0" : "1"}"
                    data-device-window="${String(windowBytes)}">
                    <td><code>${escapeHtml(id)}</code></td>
                    <td>${escapeHtml(kind)}</td>
                    <td>${bound ? escapeHtml(baseText) : `<span class="tag" data-device-pending="1">待生效</span>`}</td>
                    <td>${escapeHtml(hex32(windowBytes))}</td>
                </tr>
            `;
        })
        .join("");
    return `
        <section class="card">
            <h3>器件实例<span class="tag">逐实例 · ${instances.length} 个</span></h3>
            <table class="map-table" data-device-instances="rows">
                <thead><tr><th>实例 id</th><th>类型</th><th>基址</th><th>窗口</th></tr></thead>
                <tbody>${rows}</tbody>
            </table>
        </section>
    `;
}
