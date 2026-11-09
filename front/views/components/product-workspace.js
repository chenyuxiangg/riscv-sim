// 产品视图（M-02 · product）的工作面渲染：左侧产品树 + 右侧器件内存窗口。
//
// # 这个模块管什么
//
// 产品视图从「树 + 内存映射表 + 一个跳到别的视图的按钮」改成了**双栏工作面**：
//
//   - **左栏**：产品树（核 / 存储段 / 外设），默认**不加载任何板子**，
//     只给「创建」与「从模版加载」两个入口；带 lock / unlock；
//   - **右栏**：器件内存窗口。默认**空白**，居中显示「内存展示区」；
//     点中某个器件后才显示它的基址、各偏移与偏移含义（核只显示 CSR）；
//   - **lock / unlock 只门控编辑**：锁定时字段只读；「改完必须重新载入才
//     生效」是**提交语义**，与锁**无关**（锁着也能重载）。
//
// # 为什么渲染是纯的
//
// M-02 的 `applyRenderEvent` 每次都用 `body.innerHTML = html` 重建面板 ⇒
// 任何「住在 DOM 里」的草稿都会被下一次重渲染抹掉 ⇒ 草稿一律住在 M-03 的
// 视图状态里（`board` / `docs` / `docsBase` / `selected` / `locked`），
// 本模块只做**读**，写入都在事件处理里（`m02_viewcomponents.js`）。
//
// # 描述层 vs 契约层
//
// `board`（草稿）是**描述形状**（`schema_version / id / name / regions / ...`），
// 提交前由 `memory-map.js` 的 `draftToDescription` 归一化成线上载荷。
// 寄存器表的偏移 / 含义是**前端文档层**（`device-docs.js`）⇒ 不进契约。

import {
    DEVICE_DOCS,
    buildDeviceNodes,
    cloneRegisterDoc,
    hasDeviceDoc,
    hexAddress,
    normalizeText,
    sameRegisterDoc,
} from "./device-docs.js";

import { PLATFORM_SCHEMA_VERSION } from "../../lib/message.js";
import { parseErrorCode, renderMemoryMapTable } from "./memory-map.js";
import { renderImageLoadStatus } from "./image-loader.js";

/** 创建面板里可勾选的器件（顺序即展示顺序）。 */
export const CREATE_CATALOG = Object.freeze([
    { kind: "ram", label: "RAM 主存", base: "0x80000000", size: "0x00100000" },
    { kind: "rom", label: "ROM 固件", base: "0x00010000", size: "0x00080000" },
    { kind: "uart16550", label: "UART16550 串口", base: "0x40000000" },
    { kind: "gpio", label: "GPIO", base: "0x40001000" },
    { kind: "clint", label: "CLINT 本地中断", base: "0x02000000" },
    { kind: "plic", label: "PLIC 平台中断", base: "0x0C000000" },
]);

/** 器件 id 的前缀（`uart16550` → `UART0`）。 */
const DEVICE_ID_PREFIX = Object.freeze({
    uart16550: "UART",
    gpio: "GPIO",
    clint: "CLINT",
    plic: "PLIC",
});

/** 器件类型的默认参数（与契约 `DeviceSpec` 的参数表一致）。 */
const DEVICE_PARAM_DEFAULTS = Object.freeze({
    uart16550: { irq_line: 10, baud_rate: 115200 },
    gpio: {},
    clint: { freq_hz: 10000000, harts: 1 },
    plic: { sources: 32, contexts: 1 },
});

/** 内置模版（不落库 ⇒ 用户第一次点开就有东西可载）。 */
export const BUILTIN_TEMPLATES = Object.freeze([
    {
        id: "tpl-min",
        name: "最小系统（RAM + UART + GPIO）",
        description: Object.freeze({
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "tpl-min",
            name: "最小系统",
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 0x01000000,
            regions: Object.freeze([
                Object.freeze({ kind: "ram", base: 0x80000000, size: 0x00100000 }),
                Object.freeze({
                    kind: "peripheral",
                    base: 0x40000000,
                    device: Object.freeze({ device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 }),
                }),
                Object.freeze({
                    kind: "peripheral",
                    base: 0x40001000,
                    device: Object.freeze({ device: "gpio", id: "GPIO0" }),
                }),
            ]),
        }),
    },
    {
        id: "tpl-soc",
        name: "带中断的 SoC（RAM/ROM + CLINT + PLIC + UART）",
        description: Object.freeze({
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "tpl-soc",
            name: "带中断的 SoC",
            priority: 100,
            auto_selectable: false,
            region_budget_bytes: 0x01000000,
            regions: Object.freeze([
                Object.freeze({ kind: "rom", base: 0x00010000, size: 0x00080000 }),
                Object.freeze({ kind: "ram", base: 0x80000000, size: 0x00200000 }),
                Object.freeze({
                    kind: "peripheral",
                    base: 0x02000000,
                    device: Object.freeze({ device: "clint", id: "CLINT0", freq_hz: 10000000, harts: 1 }),
                }),
                Object.freeze({
                    kind: "peripheral",
                    base: 0x0C000000,
                    device: Object.freeze({ device: "plic", id: "PLIC0", sources: 32, contexts: 1 }),
                }),
                Object.freeze({
                    kind: "peripheral",
                    base: 0x40000000,
                    device: Object.freeze({ device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 }),
                }),
            ]),
        }),
    },
]);

/** HTML 转义（面板里的每一处动态文本都要过它）。 */
function escapeHtml(text) {
    return String(text ?? "")
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;")
        .replace(/'/g, "&#39;");
}

/**
 * **生效中**的描述：内核推来的那份（`memoryMap.description`）。
 *
 * @param {object} state
 * @returns {object|null}
 */
export function effectiveDescription(state) {
    return state?.memoryMap?.description ?? null;
}

/**
 * **草稿**描述：用户正在改的那份；没有草稿就退回生效那份（只读展示）。
 *
 * @param {object} state
 * @returns {object|null}
 */
export function boardDraft(state) {
    return state?.board?.description ?? effectiveDescription(state);
}

/**
 * 某个节点当前的**寄存器草稿**（没有就取内置表的拷贝）。
 *
 * ⚠️ 内置表本身**不被修改**（`cloneRegisterDoc` 深拷贝）。
 *
 * @param {object} state
 * @param {object} node
 * @returns {{unit: "B"|"bit", regs: Array<object>}}
 */
export function docFor(state, node) {
    const key = String(node?.key ?? "");
    const stored = state?.docs?.[key];
    if (stored && Array.isArray(stored.regs)) return stored;
    return cloneRegisterDoc(node?.deviceKind);
}

/**
 * 某个节点**已生效**的寄存器描述（重载后的基线；没有就取内置表）。
 *
 * @param {object} state
 * @param {object} node
 * @returns {{unit: "B"|"bit", regs: Array<object>}}
 */
export function baseDocFor(state, node) {
    const key = String(node?.key ?? "");
    const stored = state?.docsBase?.[key];
    if (stored && Array.isArray(stored.regs)) return stored;
    return cloneRegisterDoc(node?.deviceKind);
}

/**
 * 草稿里的某段（按节点下标取）。
 *
 * @param {object|null} description
 * @param {object} node
 * @returns {object|null}
 */
export function draftRegion(description, node) {
    const index = node?.regionIndex;
    if (!Number.isInteger(index)) return null;
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    return regions[index] ?? null;
}

/**
 * 在**生效描述**里找到与草稿某一段**同一个器件**的那一段。
 *
 * ⚠️ 内核会把段按 `base` 重排后推回来 ⇒ 下标不可信 ⇒ 用什么对齐：
 *
 *   - 外设：`device.id`（器件身份，唯一）；
 *   - 存储段：同 `kind` 的第 n 个（同一块板里同类型存储段很少，且它们的
 *     `base` 是用户可能正在改的字段，不能拿来当键）。
 *
 * @param {object|null} description — 生效描述
 * @param {object|null} draft — 草稿描述（提供下标与次序）
 * @param {object} node
 * @returns {object|null}
 */
export function matchRegion(description, draft, node) {
    const index = node?.regionIndex;
    if (!Number.isInteger(index)) return null;
    const draftRegions = Array.isArray(draft?.regions) ? draft.regions : [];
    const region = draftRegions[index] ?? null;
    if (!region) return null;
    const effRegions = Array.isArray(description?.regions) ? description.regions : [];
    if (region.kind === "peripheral") {
        const id = String(region?.device?.id ?? "");
        if (!id) return null;
        return effRegions.find((r) => String(r?.device?.id ?? "") === id) ?? null;
    }
    const nth = draftRegions.filter((r) => r?.kind === region.kind).indexOf(region);
    const effSame = effRegions.filter((r) => r?.kind === region.kind);
    return effSame[nth] ?? null;
}

/**
 * 某个节点有没有未生效的改动。
 *
 * @param {object} state
 * @param {object} node
 * @returns {{base: boolean, doc: boolean}} `base` 对核永远 `false`（核没有基址）
 */
export function nodeDirty(state, node) {
    const eff = effectiveDescription(state);
    const draft = boardDraft(state);
    let baseDirty = false;
    if (node?.regionIndex !== null && node?.regionIndex !== undefined) {
        const a = draftRegion(draft, node);
        // ⚠️ **按器件身份对齐**，不按数组下标：内核会把段**排序**后推回来
        // （同一份板，装载前 UART 在前、装载后 GPIO 在前）⇒ 按下标比会把
        // 「什么都没改」判成「每段都改了」。
        const b = matchRegion(eff, draft, node);
        if (a || b) {
            baseDirty = normalizeText(a?.base) !== normalizeText(b?.base)
                || Number(a?.size ?? 0) !== Number(b?.size ?? 0);
        }
    }
    const docDirty = !sameRegisterDoc(docFor(state, node), baseDocFor(state, node));
    return { base: baseDirty, doc: docDirty };
}

/**
 * 整个产品有没有未生效的改动（底栏与侧栏据此显示提示与按钮）。
 *
 * @param {object} state
 * @returns {{count: number, dirty: boolean, keys: string[]}}
 */
export function productDirty(state) {
    const nodes = buildDeviceNodes(state);
    const keys = [];
    let count = 0;
    for (const node of nodes) {
        const d = nodeDirty(state, node);
        if (d.base) count += 1;
        if (d.doc) count += 1;
        if (d.base || d.doc) keys.push(node.key);
    }
    return { count, dirty: keys.length > 0, keys };
}

/**
 * 造一份**可提交的**板描述草稿（创建面板用）。
 *
 * ⚠️ 数值用字符串（草稿形状，`draftToDescription` 提交时归一化）⇒ 与
 * 「内存图编辑器」同一个约定。
 *
 * @param {string} name — 产品 / 板名
 * @param {string[]} kinds — 勾选的器件类型
 * @param {string} [id] — 板 id（缺省从名字派生）
 * @returns {object} 描述草稿
 */
export function createBoardDraft(name, kinds, id) {
    const chosen = Array.isArray(kinds) ? kinds : [];
    const boardName = String(name ?? "").trim() || "自定义产品";
    const boardId = String(id ?? "").trim() || `board-${Date.now().toString(36)}`;
    const regions = [];
    for (const kind of chosen) {
        const item = CREATE_CATALOG.find((entry) => entry.kind === kind);
        if (!item) continue;
        if (kind === "ram" || kind === "rom") {
            // 多个同类型存储段：第二个起基址往后挪一段，避免自我重叠。
            const same = regions.filter((r) => r.kind === kind).length;
            const base = (Number.parseInt(String(item.base), 16) + same * Number.parseInt(String(item.size), 16)) >>> 0;
            regions.push({ kind, base: hexAddress(base), size: item.size });
            continue;
        }
        const same = regions.filter((r) => r?.device?.device === kind).length;
        // ⚠️ 器件 id 是**人读的短名**（`UART0`，不是 `UART165500`）：内核与
        // 固件侧都按 `UART0` / `GPIO0` 认这块板上的器件。
        const device = { device: kind, id: `${DEVICE_ID_PREFIX[kind] ?? kind.toUpperCase()}${same}` };
        for (const [key, value] of Object.entries(DEVICE_PARAM_DEFAULTS[kind] ?? {})) {
            device[key] = String(value);
        }
        const base = (Number.parseInt(String(item.base), 16) + same * 0x1000) >>> 0;
        regions.push({ kind: "peripheral", base: hexAddress(base), device });
    }
    return {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: boardId,
        name: boardName,
        priority: "100",
        auto_selectable: false,
        region_budget_bytes: "0x01000000",
        regions,
    };
}

/**
 * 产品树 → `Project.tree`（装载工程用）。
 *
 * @param {object|null} description
 * @returns {object} `{root: {name, component_id, children}}`
 */
export function projectTreeFrom(description) {
    const name = String(description?.name ?? "MainBoard");
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    const children = [];
    for (const region of regions) {
        if (String(region?.kind ?? "") !== "peripheral") continue;
        const id = String(region?.device?.id ?? "");
        if (!id) continue;
        children.push({ name: id, component_id: id, children: [] });
    }
    return {
        root: {
            name,
            component_id: "rv-sim",
            children: [
                { name: "RV32IMC", component_id: "rv32imc", children },
            ],
        },
    };
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

/** 器件类型 → 图标化的类型标签。 */
function kindBadge(node) {
    if (node.kind === "core") return "核 · 内部寄存器";
    if (node.kind === "memory") return node.deviceKind === "rom" ? "ROM · 只读段" : "RAM · 读写段";
    return `${node.deviceKind || "外设"} · 寄存器窗口`;
}

/** 左栏一棵树的节点行。 */
function renderTreeNode(state, node, selected, locked) {
    const dirty = nodeDirty(state, node);
    const active = node.key === selected;
    const addr = node.kind === "core" ? "CSR" : hexAddress(node.base);
    const marks = [];
    if (dirty.base) marks.push("基址改");
    if (dirty.doc) marks.push("偏移改");
    return `
        <button type="button" class="pw-node ${active ? "is-active" : ""}" data-action="select-device" data-device-key="${escapeHtml(node.key)}">
            <span class="pw-node-name">${escapeHtml(node.label)}</span>
            <span class="pw-node-meta">${escapeHtml(node.kind === "core" ? node.id : addr)}</span>
            ${marks.length ? `<span class="pw-node-dirty">${escapeHtml(marks.join(" / "))}</span>` : ""}
            <span class="pw-node-kind">${escapeHtml(kindBadge(node))}</span>
        </button>
    `;
}

/** 左栏：树 + 修订号 + 三个入口。 */
function renderSidebar(state, nodes, locked, dirty) {
    const loaded = state?.loaded === true;
    const meta = state?.memoryMap ?? null;
    const revision = Number(meta?.revision ?? 0);
    const tree = state?.tree?.root ?? null;
    const treeHtml = nodes.length
        ? nodes.map((node) => renderTreeNode(state, node, state?.selected ?? null, locked)).join("")
        : `<p class="pw-empty-hint">${loaded ? "这块板还没有任何器件 —— 换一块板，或重新创建。" : "尚未加载任何板子。"}</p>`;
    return `
        <aside class="pw-side">
            <header class="pw-side-head">
                <div>
                    <h3>产品树</h3>
                    <p class="pw-side-sub">${escapeHtml(tree?.name ?? "（空）")}</p>
                </div>
                <button type="button" class="pw-lock ${locked ? "is-locked" : ""}" data-action="toggle-lock" ${loaded ? "" : "disabled"}>
                    ${locked ? "已锁定 · 点此解锁" : "已解锁 · 点此锁定"}
                </button>
            </header>
            <div class="pw-tree" data-product-tree>${treeHtml}</div>
            <footer class="pw-side-foot">
                <p class="pw-revision">
                    <span>修订号 <b>${revision}</b></span>
                    ${meta?.pending ? `<span class="pw-pending">待生效（重新载入后应用）</span>` : ""}
                    ${dirty.dirty ? `<span class="pw-dirty-count">${dirty.count} 处未生效</span>` : ""}
                </p>
                <div class="pw-side-actions">
                    <button type="button" class="pw-btn" data-action="open-create">创建</button>
                    <button type="button" class="pw-btn" data-action="open-templates">从模版加载</button>
                    <button type="button" class="pw-btn" data-action="save-archive" ${loaded ? "" : "disabled"}>另存为档案</button>
                    <button type="button" class="pw-btn pw-btn-primary" data-action="reload-product" ${loaded ? "" : "disabled"}>重新载入</button>
                </div>
                <p class="pw-tip">${locked ? "锁定：字段只读。解锁后可改基址 / 偏移 / 含义。" : "解锁：可直接改基址 / 偏移 / 含义，改完点「重新载入」才生效。"}</p>
                ${renderImageLoadStatus(state?.imageLoad ?? null)}
                <details class="pw-m14">
                    <summary>当前生效内存映射 (M-14)</summary>
                    ${renderMemoryMapTable(state?.memoryMap ?? null)}
                </details>
            </footer>
        </aside>
    `;
}

/** 右栏空态：居中「内存展示区」。 */
function renderWindowEmpty() {
    return `
        <div class="pw-window-empty">
            <div class="pw-window-plate">内存展示区</div>
            <p>点左侧产品树里的器件，这里显示它的基址、各偏移与偏移含义。</p>
        </div>
    `;
}

/** 寄存器表（核的 CSR 与外设的偏移窗口共用一张表）。 */
function renderRegisterTable(state, node, locked) {
    const doc = docFor(state, node);
    const baseDoc = baseDocFor(state, node);
    const unit = doc.unit === "bit" ? "位宽 (bit)" : "位宽 (B)";
    const rows = Array.isArray(doc.regs) ? doc.regs : [];
    const body = rows.length
        ? rows.map((row, index) => {
            const baseRow = Array.isArray(baseDoc.regs) ? baseDoc.regs[index] : null;
            const dirty = baseRow ? !sameRegisterDoc({ unit: doc.unit, regs: [row] }, { unit: baseDoc.unit, regs: [baseRow] }) : true;
            return `
                <tr class="${dirty ? "is-dirty" : ""}">
                    <td><input type="text" class="pw-input pw-mono" data-reg-field="offset" data-reg-index="${index}" value="${escapeHtml(row.offset)}" ${locked ? "disabled" : ""}></td>
                    <td><input type="text" class="pw-input" data-reg-field="name" data-reg-index="${index}" value="${escapeHtml(row.name)}" ${locked ? "disabled" : ""}></td>
                    <td><input type="text" class="pw-input" data-reg-field="meaning" data-reg-index="${index}" value="${escapeHtml(row.meaning)}" ${locked ? "disabled" : ""}></td>
                    <td><input type="text" class="pw-input pw-mono pw-narrow" data-reg-field="width" data-reg-index="${index}" value="${escapeHtml(row.width)}" ${locked ? "disabled" : ""}></td>
                </tr>
            `;
        }).join("")
        : `<tr><td colspan="4" class="pw-table-empty">这份描述还没有寄存器说明。</td></tr>`;
    return `
        <table class="pw-table">
            <colgroup>
                <col style="width:14%"><col style="width:22%"><col style="width:50%"><col style="width:14%">
            </colgroup>
            <thead>
                <tr>
                    <th>${node.kind === "core" ? "CSR 地址" : "偏移"}</th>
                    <th>寄存器名</th>
                    <th>含义</th>
                    <th>${unit}</th>
                </tr>
            </thead>
            <tbody>${body}</tbody>
        </table>
    `;
}

/** 右栏：器件详情。 */
function renderWindowDetail(state, node, locked) {
    const dirty = nodeDirty(state, node);
    const draft = boardDraft(state);
    const region = draftRegion(draft, node);
    const unitLabel = docFor(state, node).unit === "bit" ? "CSR 偏移" : "偏移";
    const baseField = node.kind === "core"
        ? `<div class="pw-field"><span>基址</span><b class="pw-mono">无（核内部寄存器，不经总线）</b></div>`
        : `<label class="pw-field"><span>内存基址</span>
             <input type="text" class="pw-input pw-mono" data-base-input="1" value="${escapeHtml(hexAddress(region?.base ?? node.base))}" ${locked ? "disabled" : ""}>
           </label>`;
    const sizeField = node.kind === "memory"
        ? `<label class="pw-field"><span>段长度</span>
             <input type="text" class="pw-input pw-mono" data-size-input="1" value="${escapeHtml(hexAddress(region?.size ?? node.size))}" ${locked ? "disabled" : ""}>
           </label>`
        : "";
    return `
        <div class="pw-window-detail">
            <header class="pw-window-head">
                <div>
                    <h3>${escapeHtml(node.label)} <span class="pw-badge">${escapeHtml(node.kind === "core" ? "核" : node.deviceKind)}</span></h3>
                    <p>${escapeHtml(node.note)}</p>
                </div>
                ${dirty.base || dirty.doc ? `<span class="pw-dirty-flag">有未生效的改动</span>` : `<span class="pw-clean-flag">与生效一致</span>`}
            </header>
            <div class="pw-fields">${baseField}${sizeField}</div>
            <section class="pw-regs">
                <div class="pw-regs-head">
                    <h4>${node.kind === "core" ? "内部寄存器（CSR）" : `${escapeHtml(unitLabel)}与含义`}</h4>
                    <span class="muted">${locked ? "锁定中 —— 解锁后可编辑" : "可直接编辑；改完点「重新载入」才生效"}</span>
                </div>
                ${node.kind === "memory" ? `<p class="pw-tip">这段是连续内存，没有固定的寄存器偏移表；装固件后按地址直接访问。</p>` : ""}
                ${node.kind === "core" || node.kind === "peripheral" || hasDeviceDoc(node.deviceKind)
                    ? renderRegisterTable(state, node, locked)
                    : `<p class="pw-tip">这个器件类型没有内置寄存器说明。</p>`}
            </section>
            <footer class="pw-window-foot">
                <span class="muted">修改保存为草稿；点「重新载入」把草稿送进内核并重新装载工程。</span>
                <div class="pw-side-actions">
                    <button type="button" class="pw-btn" data-action="discard-product">放弃修改</button>
                    <button type="button" class="pw-btn pw-btn-primary" data-action="reload-product">重新载入</button>
                </div>
            </footer>
        </div>
    `;
}

/** 创建弹层。 */
function renderCreateDialog(state, locked) {
    const items = CREATE_CATALOG.map((item) => `
        <label class="pw-check">
            <input type="checkbox" data-create-kind="${escapeHtml(item.kind)}" value="1">
            <span>${escapeHtml(item.label)}</span>
            <b class="pw-mono">${escapeHtml(item.base)}</b>
        </label>
    `).join("");
    return `
        <div class="pw-scrim" data-dialog="create">
            <div class="pw-dialog" role="dialog" aria-modal="true">
                <h3>创建产品</h3>
                <p class="muted">从零搭一块板：勾选器件，填产品名，然后直接装载。</p>
                <label class="pw-field"><span>产品名</span><input type="text" class="pw-input" data-create-name value="自定义产品"></label>
                <div class="pw-check-grid">${items}</div>
                <footer class="pw-dialog-foot">
                    <button type="button" class="pw-btn" data-action="close-dialog">取消</button>
                    <button type="button" class="pw-btn pw-btn-primary" data-action="confirm-create">创建并装载</button>
                </footer>
            </div>
        </div>
    `;
}

/** 模版弹层（内置模版 + 本机板档案）。 */
function renderTemplateDialog(state) {
    const archive = Array.isArray(state?.archive) ? state.archive : [];
    const builtin = BUILTIN_TEMPLATES.map((tpl) => `
        <button type="button" class="pw-template" data-action="use-template" data-template-id="${escapeHtml(tpl.id)}" data-template-src="builtin">
            <b>${escapeHtml(tpl.name)}</b>
            <span class="muted">${escapeHtml(tpl.description.regions.length)} 个器件 · 内置</span>
        </button>
    `).join("");
    const saved = archive.length
        ? archive.map((entry) => `
            <button type="button" class="pw-template" data-action="use-template" data-template-id="${escapeHtml(entry?.id)}" data-template-src="archive">
                <b>${escapeHtml(entry?.description?.name ?? entry?.id)}</b>
                <span class="muted">${escapeHtml(entry?.id ?? "")} · 本机板档案</span>
            </button>
        `).join("")
        : `<p class="pw-tip">本机还没有保存过的板档案。</p>`;
    return `
        <div class="pw-scrim" data-dialog="templates">
            <div class="pw-dialog" role="dialog" aria-modal="true">
                <h3>从模版加载</h3>
                <p class="muted">选一块板直接装载；装载会替换当前产品树。</p>
                <h4>内置模版</h4>
                <div class="pw-template-list">${builtin}</div>
                <h4>本机板档案</h4>
                <div class="pw-template-list">${saved}</div>
                <footer class="pw-dialog-foot">
                    <button type="button" class="pw-btn" data-action="close-dialog">取消</button>
                </footer>
            </div>
        </div>
    `;
}

/** 另存为档案弹层（把当前产品复制成库里另一条档案）。 */
function renderArchiveDialog(state) {
    const description = boardDraft(state) ?? {};
    const currentId = String(description.id ?? "board");
    return `
        <div class="pw-scrim" data-dialog="archive">
            <div class="pw-dialog" role="dialog" aria-modal="true">
                <h3>另存为档案</h3>
                <p class="muted">把当前产品（含偏移与偏移含义）复制成本机数据库里的另一条档案；当前产品不受影响。</p>
                <label class="pw-field"><span>档案 id</span>
                    <input type="text" class="pw-input pw-mono" data-archive-id value="${escapeHtml(`${currentId}-copy`)}">
                </label>
                <label class="pw-field"><span>档案名</span>
                    <input type="text" class="pw-input" data-archive-name value="${escapeHtml(`${description.name ?? "产品"} 副本`)}">
                </label>
                <footer class="pw-dialog-foot">
                    <button type="button" class="pw-btn" data-action="close-dialog">取消</button>
                    <button type="button" class="pw-btn pw-btn-primary" data-action="confirm-save-as">保存到数据库</button>
                </footer>
            </div>
        </div>
    `;
}

/** 读档案弹层里填的 id / 名称（渲染层只读 DOM，不写状态）。 */
export function readArchiveDraft(root) {
    const idEl = root?.querySelector?.("[data-archive-id]");
    const nameEl = root?.querySelector?.("[data-archive-name]");
    return {
        id: String(idEl?.value ?? "").trim(),
        name: String(nameEl?.value ?? "").trim(),
    };
}

/**
 * 产品视图的整页 HTML。
 *
 * @param {object} state — product 视图状态
 * @returns {string}
 */
export function renderProductWorkspace(state) {
    const nodes = buildDeviceNodes(state);
    const locked = state?.locked !== false;
    const dirty = productDirty(state);
    const selectedKey = state?.selected ?? null;
    const selected = nodes.find((node) => node.key === selectedKey) ?? null;
    const windowHtml = selected ? renderWindowDetail(state, selected, locked) : renderWindowEmpty();
    const error = state?.error ?? null;
    const parsed = error ? parseErrorCode(error.code) : null;
    const dialog = state?.dialog ?? null;
    const notice = state?.notice ?? null;
    return `
        <section class="pw">
            ${renderSidebar(state, nodes, locked, dirty)}
            <main class="pw-window">
                ${error
        ? `<div class="pw-error" data-memory-map-error data-error-code="${escapeHtml(error.code)}" data-error-family="${escapeHtml(parsed?.family ?? "")}" data-error-scenario="${escapeHtml(parsed?.scenario ?? "")}"><b>${escapeHtml(error.code)}</b><span>${escapeHtml(error.message)}</span></div>`
        : ""}
                ${notice ? `<p class="pw-tip" data-product-notice>${escapeHtml(notice)}</p>` : ""}
                ${windowHtml}
            </main>
            ${dialog === "create" ? renderCreateDialog(state, locked) : ""}
            ${dialog === "templates" ? renderTemplateDialog(state) : ""}
            ${dialog === "archive" ? renderArchiveDialog(state) : ""}
        </section>
    `;
}

/**
 * 把创建面板的勾选结果读成器件类型列表（渲染层只读 DOM，不写状态）。
 *
 * @param {Element|Document} root
 * @returns {string[]}
 */
export function readCreateKinds(root) {
    const list = Array.from(root?.querySelectorAll?.("[data-create-kind]") ?? []);
    return list.filter((el) => el.checked).map((el) => String(el.dataset.createKind));
}

/**
 * 从模版列表里找到一份描述。
 *
 * @param {object} state
 * @param {string} id
 * @param {"builtin"|"archive"} src
 * @returns {object|null}
 */
export function findTemplate(state, id, src) {
    return findTemplateEntry(state, id, src)?.description ?? null;
}

/**
 * 同 `findTemplate`，但**连寄存器文档一起**返回（档案里存了偏移 / 含义）。
 *
 * @param {object} state
 * @param {string} id
 * @param {"builtin"|"archive"} src
 * @returns {{description: object, registers: object}|null} 内置模版没有额外文档 ⇒ `registers: {}`
 */
export function findTemplateEntry(state, id, src) {
    const key = String(id ?? "");
    if (src === "builtin") {
        const tpl = BUILTIN_TEMPLATES.find((entry) => entry.id === key);
        return tpl ? { description: cloneDescription(tpl.description), registers: {} } : null;
    }
    const entry = (Array.isArray(state?.archive) ? state.archive : []).find((row) => String(row?.id) === key);
    if (!entry) return null;
    return {
        description: cloneDescription(entry.description),
        registers: JSON.parse(JSON.stringify(entry?.registers ?? {})),
    };
}

/**
 * 深拷贝一份描述（模版是 `Object.freeze` 的共享常量 ⇒ 草稿必须另起一份）。
 *
 * @param {object|null} description
 * @returns {object|null}
 */
export function cloneDescription(description) {
    if (!description || typeof description !== "object") return null;
    return JSON.parse(JSON.stringify(description));
}
