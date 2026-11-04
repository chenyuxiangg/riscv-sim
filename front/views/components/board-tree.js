// 产品树 = **草稿的派生视图**（STAGE-44 · v0.68 · 裁决 A~G）。
//
// # 这个模块管什么
//
// 产品视图里那张「产品树」**不再是装饰**：它是**器件实例的编辑面**。
//
// ⚠️ **树 = 草稿**（裁决 A）⇒ ⇒ 树与板**同源** ⇒ ⇒ 「产品树与板是两条
// 互不相通的数据线」那条根因**治掉**。
//
// ⚠️ **树只管器件实例**（裁决 B）⇒ ⇒ **RAM / ROM 段**仍在「内存映射
// (M-14)」那张卡里编辑 ⇒ ⇒ **同一个量不显示两处**（同一个量只显示一处）
// ⇒ ⇒ 两个视图**各管一半、共用同一份草稿**。
//
// ⚠️ **参数表单复用编辑器那套**（裁决 D）⇒ ⇒ 字段名与钩子
// （`data-mmap-device-field` / `data-mmap-index`）**逐字同形** ⇒ ⇒
// [`collectDraft`] 采得到 ⇒ ⇒ **一份草稿、一个采集器**。
//
// ⚠️ **「待生效」取自内核的逐实例状态**（裁决 F）⇒ ⇒ 前端**不**自己推
// ⇒ ⇒ 与内核**同一口径**。

import { escapeHtml } from "../../lib/escape.js";

/**
 * 树根节点的**折叠键**（沿用 v0.50 批那条「折叠状态住视图状态」的机制）。
 *
 * ⚠️ **键 = 一个常量** ⇒ ⇒ 折叠的是**器件区**这一层。
 */
export const TREE_ROOT_KEY = "root";
import {
    DEVICE_LABELS,
    availableDeviceKinds,
    deviceField,
    deviceKindSelect,
    deviceParams,
    regionField,
} from "./memory-map.js";

/**
 * 草稿里的**器件实例**（= 外设窗口）。
 *
 * ⚠️ **判据 = `region.kind === "peripheral"`** ⇒ ⇒ RAM / ROM 段**不进树**
 * ⇒ ⇒ 树**不重复**内存映射表那张表的内容（裁决 B）。
 *
 * @param {object} draft — 草稿
 * @returns {Array<{index: number, region: object}>}
 */
export function draftDevices(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const out = [];
    regions.forEach((region, index) => {
        if (String(region?.kind) === "peripheral") out.push({ index, region });
    });
    return out;
}

/**
 * 一个器件实例是不是**待生效**（还没在内核当前生效的图里）。
 *
 * ⚠️ **判据来自内核**（裁决 F）⇒ ⇒ 逐实例清单里**没有**这一条 ⇒ ⇒ 它
 * **不在**当前生效的图里 ⇒ ⇒ 待生效。
 *
 * ⚠️ **内核还没报过实例**（清单为 `null`）⇒ ⇒ **一律当已绑定** ⇒ ⇒ 不
 * 假装用户在改的东西「没生效」（那会误导）。
 *
 * @param {Array<object>|null} instances — 内核报的逐实例清单
 * @param {object} region — 草稿里的那一段
 * @returns {boolean}
 */
export function isPending(instances, region) {
    if (!Array.isArray(instances)) return false;
    const id = String(region?.device?.id ?? "");
    const base = String(region?.base ?? "");
    return !instances.some((inst) => (
        String(inst?.id ?? "") === id && String(inst?.base ?? "") === base
    ));
}

/**
 * 一个器件节点的 HTML（含**参数表单**与**删除**按钮）。
 *
 * ⚠️ **参数表单用编辑器那套字段**（裁决 D）⇒ ⇒ 采集器**一个**。
 *
 * @param {object} options
 * @returns {string}
 */
function renderDeviceNode({ index, region, kinds, pending }) {
    const device = region?.device ?? {};
    const kind = String(device.device ?? "gpio");
    const id = String(device.id ?? "");
    const base = String(region.base ?? "0");
    const paramFields = (paramList(kind)).map((p) => deviceField({
        index,
        name: p.name,
        label: p.label,
        value: device[p.name] ?? "0",
    })).join("");
    return `
        <div class="node board-device"
             data-device-node="${escapeHtml(id)}"
             data-device-node-kind="${escapeHtml(kind)}"
             data-device-node-pending="${pending ? "1" : "0"}">
            <div class="row board-device-head">
                <span class="caret leaf" aria-hidden="true">·</span>
                <span class="name">${escapeHtml(id || "(未命名器件)")}</span>
                <span class="badge" data-device-node-badge>${escapeHtml(DEVICE_LABELS[kind] ?? kind)}</span>
                ${pending ? `<span class="tag" data-device-node-pending-tag="1">待生效</span>` : ""}
                <button type="button" class="btn ghost danger"
                        data-action="remove-device" data-mmap-index="${index}"
                        title="从草稿删除这个器件实例">删除</button>
            </div>
            <div class="board-device-fields">
                ${regionField({ index, name: "base", label: "基址 base", value: base })}
                ${deviceKindSelect(index, kind, kinds)}
                ${deviceField({ index, name: "id", label: "器件 id", value: id })}
                ${paramFields}
            </div>
        </div>
    `;
}

/**
 * 某类型的**参数清单**（名字 + 标签）。
 *
 * ⚠️ **与编辑器同源**（[`deviceParams`]）⇒ ⇒ **不写第二份参数表**（裁决 D）
 * ⇒ ⇒ 「每个类型有哪些参数」这件事**只有一处**。
 *
 * @param {string} kind
 * @returns {Array<{name: string, label: string}>}
 */
function paramList(kind) {
    return deviceParams(kind);
}

/**
 * 产品树那张卡的 HTML。
 *
 * @param {object} options
 * @param {object} options.draft — 草稿（**唯一那一份**）
 * @param {Array<object>|null} options.deviceTypes — 内核的类型表
 * @param {Array<object>|null} options.instances — 内核的逐实例状态
 * @returns {string}
 */
export function renderBoardTree({ draft, deviceTypes, instances, collapsed }) {
    const kinds = availableDeviceKinds(deviceTypes);
    const devices = draftDevices(draft);
    const boardName = String(draft?.name ?? "板");
    if (kinds.length === 0 && Array.isArray(deviceTypes)) {
        // ⚠️ **交集为空** ⇒ ⇒ 内核**不支持**前端能写的那些类型 ⇒ ⇒ 明说，
        // 不给一个空下拉让用户对着空气点。
        return `
            <section class="card">
                <h3>产品树<span class="tag">板定义</span></h3>
                <p class="muted" data-board-tree="unsupported">
                    内核**不支持**任何可实例化的器件类型（内核报告的类型：${escapeHtml(
        (deviceTypes ?? []).map((t) => String(t?.kind ?? "")).join("、") || "（无）",
    )}）⇒ ⇒ 先换内核版本，或扩器件类型。
                </p>
            </section>
        `;
    }
    const nodes = devices
        .map((d) => renderDeviceNode({
            index: d.index,
            region: d.region,
            kinds,
            pending: isPending(instances, d.region),
        }))
        .join("");
    // ⚠️ **折叠**（沿用 v0.50 批那条能力）：折叠的是**器件区**（`.kids`）
    // ⇒ ⇒ 折叠状态**仍住视图状态**（`product.collapsed`）⇒ ⇒ 不是 DOM 私藏。
    const isCollapsed = collapsed instanceof Set ? collapsed.has(TREE_ROOT_KEY) : false;
    const kids = isCollapsed ? "" : `<div class="kids">${nodes}</div>`;
    const empty = devices.length === 0
        ? `<p class="muted" data-board-tree="empty">这块板上**还没有器件实例** ⇒ ⇒ 点「+ 器件」加一个（内存段在下面那张「内存映射 (M-14)」表里加）。</p>`
        : "";
    return `
        <section class="card">
            <h3>产品树<span class="tag">板定义 · 器件的编辑面</span></h3>
            <p class="muted">
                ⚠️ 树 = **当前草稿**的派生视图 ⇒ ⇒ 与下面那张内存映射表**共用同一份草稿** ⇒ ⇒
                在这里加 / 删 / 改器件，那张表**同步变**；提交**只有一条路径**。
            </p>
            <div class="tree" data-board-tree>
                <div class="node">
                    <span class="caret${devices.length > 0 ? "" : " leaf"}"
                          data-action="toggle-tree" data-tree-key="${TREE_ROOT_KEY}"
                          role="button" tabindex="0" aria-expanded="${isCollapsed ? "false" : "true"}"
                          title="${devices.length > 0 ? (isCollapsed ? "展开" : "折叠") : "（没有器件可折叠）"}">${isCollapsed ? "▶" : "▼"}</span>
                    <span class="name">${escapeHtml(boardName)}</span>
                    <span class="sub">(板 · ${devices.length} 个器件实例)</span>
                </div>
                ${kids}
            </div>
            ${empty}
            <div class="debug-toolbar">
                <button type="button" data-action="add-device">+ 器件</button>
                <button type="button" data-action="submit-board">提交板级内存图</button>
            </div>
        </section>
    `;
}
