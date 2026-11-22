// 源码视图面板（M-02 · source）—— 拆分自 m02_viewcomponents.js。
//
// 管 `sourceView` 注册组件。源码不进内核（裁决 E）：文件由浏览器原生
// `<input type="file">` 读成文本 ⇒ 存进 M-03 视图状态 ⇒ 不发给内核、
// 不新增契约命令。整个视图里只有一条命令会离开前端：`resolve-line`。

import * as viewStore from "../m03_viewstore.js";
import { escapeHtml } from "../../lib/escape.js";
import {
    describeMatch,
    matchSourceFile,
    renderSourceColumns,
} from "./source-view.js";
import { register, emitCommand } from "../component-registry.js";

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
        // 用户自填的「编译期前缀 ⇒ 本地前缀」映射表（仅本会话）。
        const remaps = Array.isArray(src.remaps) ? src.remaps : [];

        // 匹配结果在**渲染时**算（纯函数）⇒ 状态里不存第二份真相。
        const match = location?.file ? matchSourceFile(tree, location.file, remaps) : null;
        // ⚠️ **三种「没有答案」必须分开**（VB-5）：
        //   - 还没问              ⇒ `location == null`
        //   - 问了，但**没有 DWARF** ⇒ `file === "" && line === 0`
        //   - 问了，但**地址没命中** ⇒ `file === "" && line === 0xFFFFFFFF`
        const answered = location != null;
        const fileMissing = answered && !location.file;
        const noDwarf = fileMissing && Number(location.line ?? 0) === 0;
        const missed = fileMissing && Number(location.line ?? 0) === 0xFFFFFFFF;
        // ⚠️ **三态如实区分**（裁决 J）：「没有源码树」/「匹配不到」/「匹配到多个」。
        const statusText = keys.length === 0
            ? "尚未装载源码"
            : (location?.file
                ? describeMatch(match)
                : (noDwarf
                    ? "没有 DWARF 行表"
                    : (missed
                        ? "不在 DWARF 行表内"
                        : "尚未解析")));
        const activeKey = selected ?? (match?.kind === "unique" ? match.key : null);
        const line = Number(location?.line ?? 0);

        return `
            <section class="source-view">
                <div class="card">
                    <div class="row"><h3>源码</h3></div>
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
                    <div class="source-remaps" data-source-remaps>
                        <div class="row">
                            <h4>路径前缀映射</h4>
                            <button type="button" data-action="remap-add">+ 添加映射</button>
                        </div>
                        <p class="muted">编译期前缀 → 本地前缀</p>
                        ${remaps.length === 0
                            ? `<p class="muted">尚无映射</p>`
                            : remaps.map((r) => `
                                <div class="remap-row" data-remap-row data-id="${escapeHtml(String(r.id))}">
                                    <input type="text" data-remap-field="from" data-id="${escapeHtml(String(r.id))}" value="${escapeHtml(String(r.from ?? ""))}" placeholder="编译期前缀，如 /home/user/osz_build/osz-main" spellcheck="false" autocomplete="off">
                                    <span class="note">⇒</span>
                                    <input type="text" data-remap-field="to" data-id="${escapeHtml(String(r.id))}" value="${escapeHtml(String(r.to ?? ""))}" placeholder="本地前缀（留空 = 剥目录）" spellcheck="false" autocomplete="off">
                                    <button type="button" data-action="remap-remove" data-id="${escapeHtml(String(r.id))}">删除</button>
                                </div>`).join("")}
                    </div>
                    <p class="source-status" data-source-status>${escapeHtml(statusText)}</p>
                    ${location
                        ? (location.file
                            ? `<p class="source-location" data-source-location>${escapeHtml(String(location.file))}:${Number(location.line ?? 0)}（地址 0x${(Number(location.addr ?? 0) >>> 0).toString(16).padStart(8, "0")}）</p>`
                            // ⚠️ 内核**回答了**但没有文件名 ⇒ **不**显示
                            // `:4294967295` 这种无意义行号（VB-5）。
                            : `<p class="source-location" data-source-location>（地址 0x${(Number(location.addr ?? 0) >>> 0).toString(16).padStart(8, "0")}，无文件名）</p>`)
                        : `<p class="muted" data-source-location>尚未解析</p>`}
                    ${match?.kind === "ambiguous"
                        ? `<ul class="source-candidates" data-source-candidates>${match.candidates.map((key) => `
                            <li><button type="button" data-action="pick-candidate" data-key="${escapeHtml(key)}">${escapeHtml(key)}</button></li>
                        `).join("")}</ul>`
                        : ""}
                    ${renderSourceColumns({ tree, activeKey, currentLine: line })}
                </div>
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
        if (action === "remap-add") {
            addRemap();
            return "cmd-remap-add";
        }
        if (action === "remap-remove") {
            removeRemap(el.dataset.id);
            return "cmd-remap-remove";
        }
        return undefined;
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => sourceView.onCommand(ev));
        el.addEventListener("change", (ev) => {
            const remapInput = ev.target?.closest?.("[data-remap-field]");
            if (remapInput) {
                updateRemap(remapInput.dataset.id, remapInput.dataset.remapField, remapInput.value);
                return;
            }
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
 * 当前会话里的路径映射表（纯前端资产 ⇒ 不进内核、不持久化）。
 *
 * @returns {Array<{id: number|string, from: string, to: string}>}
 */
function currentRemaps() {
    const prev = viewStore.getState("source")?.source?.remaps;
    return Array.isArray(prev) ? prev : [];
}

/**
 * 把映射表写回 M-03 并触发源码视图重渲染。
 *
 * @param {Array<{id: number|string, from: string, to: string}>} remaps
 */
function writeRemaps(remaps) {
    viewStore.setState("source", (prev) => ({
        ...prev,
        source: { ...(prev.source ?? {}), remaps },
    }));
    publishSourceState();
}

/** 追加一条空映射。 */
function addRemap() {
    const remaps = currentRemaps();
    const nextId = remaps.reduce((max, r) => Math.max(max, Number(r.id) || 0), 0) + 1;
    writeRemaps([...remaps, { id: nextId, from: "", to: "" }]);
}

/**
 * 按 id 删一条映射。
 *
 * @param {string|number} id
 */
function removeRemap(id) {
    writeRemaps(currentRemaps().filter((r) => String(r.id) !== String(id)));
}

/**
 * 更新某条映射的 `from` / `to` 字段。
 *
 * @param {string|number} id
 * @param {string} field — `"from"` | `"to"`
 * @param {string} value
 */
function updateRemap(id, field, value) {
    const remaps = currentRemaps().map((r) =>
        String(r.id) === String(id) ? { ...r, [field]: String(value ?? "") } : r,
    );
    writeRemaps(remaps);
}

/**
 * 让源码视图按**当前状态**重渲染。
 *
 * ⚠️ 走的是 M-01 的 `riscv:render` 事件 ⇒ 视图层不直接摸 DOM。
 */
function publishSourceState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "source", type: "update", state: viewStore.getState("source") },
    }));
}
