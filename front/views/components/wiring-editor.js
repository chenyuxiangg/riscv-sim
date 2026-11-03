// v0.74 批 · STAGE-50 · **D-177 的 D** —— **连线编辑面**（界面上画线）。
//
// ## 为什么要有这个模块
//
// v0.34 批（D-125）把「UART 的 RX 中断连到 PLIC 的哪个源」搬进了**数据**
// （平台描述的 `wiring` 段）⇒ ⇒ 但**界面画不出线** ⇒ ⇒ 「从界面提交的板
// 永远没有 `wiring`」是 v0.34 ~ v0.73 一直挂着的**已知边界**。
//
// v0.74 批把这个边界**拆掉**：面板上多一张「器件连线」卡 ⇒ ⇒ 用户可以
// **加一根 / 删一根** ⇒ ⇒ 草稿里就有 ⇒ ⇒ 提交回去还在。
//
// ## ⚠️ **前端不判合法性**（裁决 L / M）
//
// 「这个事件名合法吗」「这个引脚号在不在范围内」「这个源号有没有超出那台
// PLIC 的源数」——**全部**由**内核**的 V-18 / V-19 裁决（既有码 `E_PLT-16`）
// ⇒ ⇒ 本模块**不写第二个校验器** ⇒ ⇒ 前端只**呈现**与**收集**。
//
// ⚠️ **事件名是自由文本 + 建议列表**（不是 `<select>`）：内核的词表**会长大**
// ⇒ ⇒ 若前端用 `<select>` 锁死清单 ⇒ ⇒ 内核新增一个事件时**前端表达不了**
// ⇒ ⇒ ⇒ 那就是前端**约束**了词表（第二个真相源）⇒ ⇒ ⇒ **不许**。
// ⇒ ⇒ 这里用 `<input list="…">`：清单**只是建议**，用户**可以**手输别的名字
// ⇒ ⇒ ⇒ 合法性**只**由内核判。
//
// ## ⚠️ **引脚数来自内核**（裁决 F · X-3）
//
// 「这个 GPIO 有多少个引脚」⇒ ⇒ **内核**知道（契约层 `GPIO_PIN_COUNT` 是
// 唯一落点）⇒ ⇒ 内核的器件类型表里**带**这个数（`DeviceTypeInfo.pin_count`）
// ⇒ ⇒ 前端**查**它、**不抄** ⇒ ⇒ 界面上那句「32 个引脚」是**内核说的**。
//
// ## ⚠️ **新能力一律住新模块**（v0.63 批的白屏纪律）⇒ ⇒ 不给既有模块加导出。

import { escapeHtml } from "../../lib/escape.js";
import { WIRING_EVENTS } from "../../lib/message.js";

/**
 * 板上的**器件实例**（`id` + 种类）—— 连线的**源**与**目标**都从这里挑。
 *
 * ⚠️ **只读草稿**（不改它）⇒ ⇒ 纯函数。
 *
 * @param {object} draft
 * @returns {Array<{id: string, kind: string}>}
 */
export function wiringDevices(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const out = [];
    for (const region of regions) {
        if (String(region?.kind ?? "") !== "peripheral") continue;
        const device = region?.device ?? {};
        const id = String(device.id ?? "");
        if (id.length === 0) continue;
        out.push({ id, kind: String(device.device ?? "") });
    }
    return out;
}

/**
 * 板上的 **PLIC**（连线的**目标**只能是 PLIC —— V-19）。
 *
 * ⚠️ **前端不判「这根线合不合法」**（裁决 L）⇒ ⇒ 这里只是**把候选摆出来**
 * ⇒ ⇒ 用户仍然可以手输别的 id（⇒ ⇒ 内核判）。
 *
 * @param {object} draft
 * @returns {Array<{id: string, kind: string}>}
 */
export function wiringPlics(draft) {
    return wiringDevices(draft).filter((one) => one.kind === "plic");
}

/**
 * 某个器件种类**有多少个引脚**（**查内核的类型表** · 裁决 F / X-3）。
 *
 * ⚠️ **查不到 ⇒ `null`**（不是 0、不是猜一个）⇒ ⇒ 界面**不显示**那句提示
 * ⇒ ⇒ **绝不编一个数字**（编了就成第二个真相源）。
 *
 * ⚠️ **前端不抄那个数** ⇒ ⇒ 判据由 `front/tests/wiring-editor.test.mjs`
 *   的**跨层**那条守着（读 Rust 源码比对）。
 *
 * @param {Array<object>|null|undefined} deviceTypes — 内核的器件类型表
 * @param {string} kind — 器件种类（`"gpio"` …）
 * @returns {number|null}
 */
export function pinCountOf(deviceTypes, kind) {
    const list = Array.isArray(deviceTypes) ? deviceTypes : [];
    const hit = list.find((one) => String(one?.kind ?? "") === String(kind ?? ""));
    const n = Number(hit?.pin_count);
    return Number.isFinite(n) && n > 0 ? n : null;
}

/**
 * 某个器件**实例**的引脚数（按 id 查 ⇒ 先找器件 ⇒ 再查它的种类）。
 *
 * @param {Array<object>|null|undefined} deviceTypes
 * @param {object} draft
 * @param {string} deviceId
 * @returns {number|null}
 */
export function pinCountOfDevice(deviceTypes, draft, deviceId) {
    const device = wiringDevices(draft).find((one) => one.id === String(deviceId ?? ""));
    if (!device) return null;
    return pinCountOf(deviceTypes, device.kind);
}

/**
 * 事件名 → 它的**建议**信息（**镜像**那份 · 只用于界面上的提示）。
 *
 * ⚠️ **查不到 ⇒ `null`**（不是「不许用」）⇒ ⇒ 界面**不提示**、**不禁用**
 * ⇒ ⇒ 用户手输的事件名照样能提交（⇒ ⇒ 合法性**只**由内核判）。
 *
 * @param {string} event
 * @returns {{name: string, source_kind: string, needs_pin: boolean}|null}
 */
export function wiringEventInfo(event) {
    const name = String(event ?? "");
    return WIRING_EVENTS.find((one) => one.name === name) ?? null;
}

/**
 * 加一根**线**（草稿层面）。
 *
 * ⚠️ **新行给的是「能提交的默认值」**：源 = 板上**第一个**器件 · 事件 = 与
 *   那个器件**种类**相配的那个（⇒ ⇒ 用户少改一处）· 目标 = 板上**第一个**
 *   PLIC · 源号 = 0。
 *
 * ⚠️ **不给「空行」**：空行 ⇒ ⇒ 提交时描述**必然**被内核拒（`E_PLT-16`）
 *   ⇒ ⇒ 用户会以为板坏了 ⇒ ⇒ ⇒ 新行**必须**是**完整**的一行。
 *
 * @param {object} draft
 * @returns {object} 新草稿
 */
export function addWiring(draft) {
    const next = cloneWiring(draft);
    const devices = wiringDevices(next);
    const source = devices[0] ?? null;
    const target = wiringPlics(next)[0] ?? null;
    const event = source
        ? (WIRING_EVENTS.find((one) => one.source_kind === source.kind) ?? WIRING_EVENTS[0])
        : (WIRING_EVENTS[0] ?? { name: "" });
    const wire = {
        from: { device: source?.id ?? "", event: String(event?.name ?? "") },
        to: { device: target?.id ?? "", source: 0 },
    };
    // ⚠️ **引脚级事件 ⇒ 默认带上 `pin`（0）**：这根线**天生**要引脚号 ⇒ ⇒
    //   缺了它内核**必然**拒（V-18）⇒ ⇒ ⇒ 默认就把它**放在那儿**。
    if (event?.needs_pin) wire.from.pin = 0;
    next.wiring = [...(Array.isArray(next.wiring) ? next.wiring : []), wire];
    return next;
}

/**
 * 删掉**一根**线（草稿层面）。
 *
 * @param {object} draft
 * @param {number} index
 * @returns {object} 新草稿
 */
export function removeWiring(draft, index) {
    const next = cloneWiring(draft);
    const list = Array.isArray(next.wiring) ? next.wiring : [];
    if (!Number.isInteger(index) || index < 0 || index >= list.length) return next;
    next.wiring = list.filter((_, i) => i !== index);
    if (next.wiring.length === 0) delete next.wiring;
    return next;
}

/**
 * 把面板上的**连线输入框**采集成草稿。
 *
 * ⚠️ **DOM 是真相**（与段表同一手法）⇒ ⇒ 用户刚敲进去的引脚号**不许**丢。
 *
 * ⚠️ **引脚号留空 ⇒ 该键不带**（不是 0）⇒ ⇒ 与契约层 serde 的
 *   `skip_serializing_if = "Option::is_none"` **同形**⇒ ⇒ 老描述**逐字不变**。
 *
 * @param {Element} root — 面板元素
 * @param {object} draft — 当前草稿
 * @returns {object} 采集后的草稿
 */
export function collectWiring(root, draft) {
    const next = cloneWiring(draft);
    if (!root || typeof root.querySelectorAll !== "function") return next;
    const list = Array.isArray(next.wiring) ? next.wiring : [];
    for (const el of root.querySelectorAll("[data-wiring-field]")) {
        const index = Number(el.getAttribute("data-wiring-index"));
        const wire = list[index];
        if (!wire) continue;
        const field = String(el.getAttribute("data-wiring-field") ?? "");
        const raw = el.value;
        if (field === "from.device") {
            wire.from.device = String(raw ?? "");
        } else if (field === "from.event") {
            wire.from.event = String(raw ?? "");
        } else if (field === "from.pin") {
            const n = parseNumber(raw);
            if (n === null) delete wire.from.pin;
            else wire.from.pin = n;
        } else if (field === "to.device") {
            wire.to.device = String(raw ?? "");
        } else if (field === "to.source") {
            const n = parseNumber(raw);
            wire.to.source = n === null ? 0 : n;
        }
    }
    if (list.length === 0) {
        delete next.wiring;
    } else {
        next.wiring = list;
    }
    return next;
}

/**
 * 渲染**连线编辑面**。
 *
 * @param {object} options
 * @param {object} options.draft — 当前草稿
 * @param {Array<object>|null} options.deviceTypes — 内核的器件类型表
 * @returns {string}
 */
export function renderWiringEditor({ draft, deviceTypes = null } = {}) {
    const wires = Array.isArray(draft?.wiring) ? draft.wiring : [];
    const devices = wiringDevices(draft);
    const plics = wiringPlics(draft);
    const rows = wires.map((wire, index) => renderWiringRow({
        wire,
        index,
        devices,
        plics,
        deviceTypes,
    })).join("");
    const body = rows
        ? rows
        : `<tr><td colspan="6" class="muted">还没有器件连线（没有连线时，内核用它的默认连线）。</td></tr>`;
    return `
        <div class="card">
            <h3>器件连线<span class="tag">${wires.length} 根</span></h3>
            <p class="muted" data-wiring-note>
                一根线 = <strong>源器件</strong>的<strong>某个事件</strong> ⇒ ⇒ PLIC 的
                <strong>某个源</strong>。事件名、引脚号、源号**是否合法由内核判**
                （越界 ⇒ <code>E_PLT-16</code>）——前端**不**写第二份校验器。
            </p>
            <table class="map-table" data-wiring-rows>
                <thead><tr><th>源器件</th><th>事件</th><th>引脚</th><th>目标 PLIC</th><th>源号</th><th></th></tr></thead>
                <tbody>${body}</tbody>
            </table>
            <div class="row">
                <button type="button" class="btn" data-action="add-wiring">+ 连线</button>
            </div>
            <datalist id="wiring-device-options">
                ${devices.map((one) => `<option value="${escapeHtml(one.id)}"></option>`).join("")}
            </datalist>
            <datalist id="wiring-plic-options">
                ${plics.map((one) => `<option value="${escapeHtml(one.id)}"></option>`).join("")}
            </datalist>
            <datalist id="wiring-event-options">
                ${WIRING_EVENTS.map((one) => `<option value="${escapeHtml(one.name)}"></option>`).join("")}
            </datalist>
        </div>
    `;
}

/**
 * 一根线的行。
 *
 * @param {object} options
 * @returns {string}
 */
function renderWiringRow({ wire, index, devices, plics, deviceTypes }) {
    const from = wire?.from ?? {};
    const to = wire?.to ?? {};
    const info = wiringEventInfo(from.event);
    // ⚠️ **引脚数来自内核**（裁决 F / X-3）⇒ ⇒ 源器件的**种类** ⇒ ⇒ 查它的
    //   类型表 ⇒ ⇒ 查不到 ⇒ ⇒ **不显示**（绝不编一个数字）。
    const deviceKind = devices.find((one) => one.id === String(from.device ?? ""))?.kind ?? "";
    const pinCount = pinCountOf(deviceTypes, deviceKind);
    const pinNote = pinCount === null
        ? ""
        : `<span class="muted" data-wiring-pin-note>（${escapeHtml(String(pinCount))} 个引脚）</span>`;
    const eventNote = info?.needs_pin
        ? `<span class="muted" data-wiring-event-note>（这个事件要引脚号）</span>`
        : "";
    return `
        <tr data-wiring-row="${index}">
            <td>
                <input type="text" data-wiring-field="from.device" data-wiring-index="${index}"
                    list="wiring-device-options" value="${escapeHtml(String(from.device ?? ""))}">
            </td>
            <td>
                <input type="text" data-wiring-field="from.event" data-wiring-index="${index}"
                    list="wiring-event-options" value="${escapeHtml(String(from.event ?? ""))}">
                ${eventNote}
            </td>
            <td>
                <input type="text" inputmode="numeric" data-wiring-field="from.pin" data-wiring-index="${index}"
                    value="${escapeHtml(from.pin === undefined || from.pin === null ? "" : String(from.pin))}">
                ${pinNote}
            </td>
            <td>
                <input type="text" data-wiring-field="to.device" data-wiring-index="${index}"
                    list="wiring-plic-options" value="${escapeHtml(String(to.device ?? ""))}">
                ${plics.length === 0 ? `<span class="muted">（板上没有 PLIC）</span>` : ""}
            </td>
            <td>
                <input type="text" inputmode="numeric" data-wiring-field="to.source" data-wiring-index="${index}"
                    value="${escapeHtml(String(to.source ?? 0))}">
            </td>
            <td>
                <button type="button" class="btn ghost" data-action="remove-wiring"
                    data-wiring-index="${index}">删除</button>
            </td>
        </tr>
    `;
}

/**
 * 拷贝一份草稿（**深复制** `wiring` ⇒ ⇒ 改副本不改原草稿）。
 *
 * @param {object} draft
 * @returns {object}
 */
function cloneWiring(draft) {
    const next = { ...(draft ?? {}) };
    if (Array.isArray(draft?.wiring)) {
        next.wiring = draft.wiring.map((wire) => ({
            from: { ...(wire?.from ?? {}) },
            to: { ...(wire?.to ?? {}) },
        }));
    }
    return next;
}

/**
 * 解析一个**数字**输入（空 ⇒ `null`）。
 *
 * ⚠️ 与 `memory-map.js` 那份**同款**（两个真相 ⇒ ⇒ 不许）⇒ ⇒ **行为一致**
 *   就好（这里**不**抄一份数值策略：它是**纯解析**，不是校验器）。
 *
 * @param {unknown} raw
 * @returns {number|null}
 */
function parseNumber(raw) {
    if (raw === null || raw === undefined) return null;
    const text = String(raw).trim();
    if (text.length === 0) return null;
    const n = Number(text);
    if (!Number.isFinite(n)) return null;
    return Math.trunc(n);
}
