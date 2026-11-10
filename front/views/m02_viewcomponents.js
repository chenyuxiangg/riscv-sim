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
// ⚠️ **M-02 → M-01 是文档化的依赖方向**（M-02 LLD §9.1：「M-02（视图组件库）
// 依赖 M-01（工作台装配与路由）」，且同段写明 M-02「**不自行实现路由**」）。
// ⇒ 视图组件**请求**跳转，路由与能力位复校验仍然只有 M-01 一处。
//
// ⚠️ 这里**没有**循环依赖：M-01 只依赖 M-03（视图状态），不依赖 M-02。
import { route } from "./m01_workbench.js";
import { escapeHtml } from "../lib/escape.js";
// ⚠️ **宽度三档只用契约镜像的那一份**（STAGE-39 · v0.54 批 · 裁决 D）⇒ ⇒
// `Byte` / `Half` / `Word` **不新造**枚举 ⇒ ⇒ 前端与内核说的是同一组词。
//
// ⚠️ **转储长度上限也只用契约镜像的那一份**（契约 v9 · 裁决 K）⇒ ⇒ 界面上的
// 「≤ 4096」不是**抄**出来的字面量 ⇒ ⇒ 它跟着镜像走 ⇒ ⇒ 夹具一改就红。
import { DUMP_MAX_BYTES, TRACE_FORMAT, TRACE_SOURCE, WRITE_SIZE } from "../lib/message.js";
// ⚠️ **「运行中」的判据与活值的隐藏口径**（STAGE-38 · v0.53 批）⇒ ⇒
// 「内核真的在执行吗」与「运行中显示什么」**各只有一处** ⇒ ⇒ 顶栏 /
// 调试视图 / 寄存器视图**共用**同一套口径 ⇒ ⇒ 不会一个藏了一个没藏。
import { isKernelRunning, liveValue } from "../lib/run-loop.js";
// ⚠️ **「读回」的纯函数**（v0.67 批）⇒ ⇒ 「变了几个」**现算**⇒ ⇒ 状态与显示
// **不可能**对不上 ⇒ ⇒ 视图层**不存**第二份。
import { READBACK_REASON_LABEL, diffRegisters } from "../lib/readback.js";
// ⚠️ **「反汇编跟随 / 陈旧标注」的纯函数**（STAGE-45 · v0.69 批 · 台账 D-176）
// ⇒ ⇒ 「这段是哪个 PC 时的」「当前 PC 是多少」「该不该自动重取」三件事
// **各只有一处** ⇒ ⇒ 视图层与 app 层**引同一处** ⇒ ⇒ 不会一个说陈旧、
// 另一个说没陈旧。
import {
    disasmStatusText,
    followPcLabel,
} from "../lib/disasm-follow.js";
// ⚠️ **trace 记录开关的判定**（纯函数）⇒ ⇒ 视图层渲染按钮/状态行与 app 层
// 发命令**引同一处** ⇒ ⇒ 「按钮写关闭、命令发开启」这种漂移**结构上不可能**。
import {
    TRACE_SWITCH_OFF,
    TRACE_SWITCH_ON,
    traceSwitchState,
} from "../lib/trace-switch.js";
// ⚠️ **波形图的纯函数 + 片段**（STAGE-36 · v0.51 批）⇒ ⇒ option 的构造
// **不碰 DOM** ⇒ ⇒ Node 侧零 DOM 就能测「画的是不是真数据」。
import { buildWaveOption, canDrawChart, resolveWaveTheme, waveChartHtml } from "./components/wave-chart.js";
// ⚠️ **终端屏幕仿真**（STAGE-41 · v0.56）：guest 的 shell 是**行编辑器**
// （每敲一个键就把整行重画一遍）⇒ ⇒ 字节流里同一个提示符出现 N 次 ⇒ ⇒
// **真实终端**只显示一行 ⇒ ⇒ 本平台此前把字节流**原样**塞进 `<pre>`
// ⇒ ⇒ `\r` 被渲染成换行 ⇒ ⇒ 用户看到一堆重画叠出来的乱码。
import { createTerminalScreen, renderTerminalScreen } from "./components/terminal-screen.js";
// ⚠️ **波形库加载器的同步读**（STAGE-36）⇒ ⇒ 视图层**只读**已加载好的模块
// ⇒ ⇒ **加载**由 app 层做（那是它的活）⇒ ⇒ 视图层**不发起**跨源 import。
import { getECharts } from "../lib/echarts-loader.js";
import {
    describeMatch,
    matchSourceFile,
    renderSourceColumns,
} from "./components/source-view.js";
import {
    addRegion,
    parseNumber,
} from "./components/memory-map.js";
import {
    buildDeviceNodes,
    cloneRegisterDoc,
} from "./components/device-docs.js";
import {
    boardDraft,
    cloneDescription,
    createBoardDraft,
    effectiveDescription,
    findTemplateEntry,
    projectTreeFrom,
    readArchiveDraft,
    readCreateKinds,
    renderProductWorkspace,
} from "./components/product-workspace.js";
// ⚠️ **连线编辑面**（v0.74 · STAGE-50 · D-177 的 D）—— **新能力住新模块**
// ⇒ ⇒ 视图层只**调用**它（加一根 / 删一根）⇒ ⇒ 形状**只有那一处**。
import {
    publishProductState,
    renderImageLoadStatus,
} from "./components/image-loader.js";
// ⚠️ **器件类型注册表 + 逐实例状态**的渲染（STAGE-43 · v0.66 · D-175）⇒ ⇒
// 两张卡的数据**只**来自内核（前端不抄第二份类型清单、不写第二份区间表）。
import {
    renderDeviceInstanceCard,
    renderDeviceTypeCard,
} from "./components/device-registry.js";
// ⚠️ **产品树 = 草稿的派生视图**（STAGE-44 · v0.68 · 裁决 A）⇒ ⇒ 树与板**同源**。
import { renderBoardTree } from "./components/board-tree.js";

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
// productTreeView — 双栏工作面：左侧产品树 + 右侧器件内存窗口（FR-01/02/03）
//
// ⚠️ **板级内存图不再是独立工作面**（本轮重构推翻 STAGE-14 §3.3 的
// 「编辑器是独立工作面」）：基址 / 偏移 / 含义都在产品视图右栏里改 ⇒
// `#/memory-map` 路由、面板与编辑器渲染代码一并删除。
//
// ⚠️ **默认空树**：启动不装载任何板子 ⇒ 左栏只有「创建」「从模版加载」两个
// 入口，右栏居中显示「内存展示区」。
// ---------------------------------------------------------------------------

/**
 * 让产品视图按**当前状态**重渲染一次。
 *
 * ⚠️ 走 M-01 的 `riscv:render` 事件（与终端 / 源码视图同一条路）⇒ 视图层
 * 不直接摸 DOM。
 */
function publishTwoPaneState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: { viewId: "product", type: "update", state: viewStore.getState("product") },
    }));
}

/** 当前 product 视图状态（永远返回对象，不返回 undefined）。 */
function productState() {
    return viewStore.getState("product") ?? {};
}

/**
 * 写 product 视图状态；`publish` 决定要不要立刻重渲染。
 *
 * ⚠️ **输入过程中不重渲染**（`body.innerHTML = html` 会重建输入框 ⇒ 焦点与
 * 光标位置全丢）⇒ 逐键输入只写状态，失焦（`change`）/按钮再重渲染。
 *
 * @param {object|((prev: object) => object)} patch
 * @param {boolean} [publish]
 */
function setProductState(patch, publish = true) {
    viewStore.setState("product", (prev) => {
        const base = prev ?? {};
        return typeof patch === "function" ? patch(base) : { ...base, ...patch };
    });
    if (publish) publishTwoPaneState();
}

/**
 * 取**可编辑**的板描述草稿；没有草稿就从「生效中的描述」拷一份。
 *
 * @returns {object|null}
 */
function ensureBoardDraft() {
    const state = productState();
    if (state.board?.description) return state.board.description;
    const eff = effectiveDescription(state);
    const clone = cloneDescription(eff);
    if (!clone) return null;
    setProductState({ board: { description: clone } }, false);
    return clone;
}

/** 选中节点的 node 对象（找不到 ⇒ `null`）。 */
function selectedNode() {
    const state = productState();
    const key = state.selected ?? null;
    if (!key) return null;
    return buildDeviceNodes(state).find((node) => node.key === key) ?? null;
}

/**
 * 往草稿的某一段写一个数值字段（基址 / 段长度）。
 *
 * ⚠️ 解析不出来 ⇒ **不写**（保留用户上一次的合法值）；数值合法性最终仍由
 * 内核的校验器裁决（前端不写第二份校验器）。
 *
 * @param {object} node
 * @param {"base"|"size"} field
 * @param {unknown} raw
 */
function writeRegionNumber(node, field, raw) {
    const index = node?.regionIndex;
    if (!Number.isInteger(index)) return;
    const value = parseNumber(raw);
    if (value === null) return;
    const draft = ensureBoardDraft();
    const region = Array.isArray(draft?.regions) ? draft.regions[index] : null;
    if (!region) return;
    region[field] = value;
    setProductState({ board: { description: draft } }, false);
}

/**
 * 改当前选中外设的**器件参数**（例如 UART 的 PLIC 源号 / 波特率）。
 *
 * ⚠️ 只写草稿；合法性（源号范围 / 是否重复）仍由内核装载时裁决。
 *
 * @param {string} name
 * @param {unknown} raw
 */
function writeDeviceParam(name, raw) {
    const node = selectedNode();
    const index = node?.regionIndex;
    if (!Number.isInteger(index)) return;
    const draft = ensureBoardDraft();
    const region = Array.isArray(draft?.regions) ? draft.regions[index] : null;
    if (!region || !region.device) return;
    region.device[String(name)] = String(raw ?? "");
    setProductState({ board: { description: draft } }, false);
}

/**
 * 改当前选中器件的一行寄存器描述（写进 `docs` 草稿）。
 *
 * @param {string} field — offset / name / meaning / width
 * @param {number} index — 行号
 * @param {unknown} raw
 */
function writeRegisterField(field, index, raw) {
    const node = selectedNode();
    if (!node) return;
    const state = productState();
    const key = node.key;
    const current = state.docs?.[key] ?? cloneRegisterDoc(node.deviceKind);
    const rows = Array.isArray(current.regs) ? current.regs.slice() : [];
    const row = rows[index];
    if (!row) return;
    const value = field === "width" ? parseNumber(raw) : String(raw ?? "");
    if (field === "width" && value === null) return;
    rows[index] = { ...row, [field]: value };
    setProductState({
        docs: { ...(state.docs ?? {}), [key]: { unit: current.unit, regs: rows } },
    }, false);
}

const productTreeView = register({
    id: "product",
    channels: ["sim"],
    /** Render to body. Pure: same storeState → same DOM string. */
    render(state) {
        return renderProductWorkspace(state ?? {});
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        const state = productState();
        switch (action) {
            // ---- 视图本地状态（不发命令） --------------------------------
            case "select-device": {
                setProductState({ selected: el.dataset.deviceKey ?? null, dialog: null });
                return "cmd-select-device";
            }
            case "toggle-lock": {
                if (state.loaded !== true) return undefined;
                setProductState({ locked: state.locked === false });
                return "cmd-toggle-lock";
            }
            case "open-create": {
                setProductState({ dialog: "create" });
                return "cmd-open-create";
            }
            case "open-templates": {
                setProductState({ dialog: "templates" });
                // 模版列表 = 内置 + 本机板档案 ⇒ 档案要现取（app.js 读板档案服务）。
                emitCommand("list-templates", "product", null);
                return "cmd-open-templates";
            }
            case "open-archives": {
                setProductState({ dialog: "archives" });
                emitCommand("list-templates", "product", null);
                return "cmd-open-archives";
            }
            case "open-add-device": {
                if (state.loaded !== true) return undefined;
                setProductState({ dialog: "add-device" });
                return "cmd-open-add-device";
            }
            case "add-device": {
                if (state.loaded !== true) return undefined;
                const kind = el?.dataset?.addKind;
                const draft = ensureBoardDraft();
                if (!kind || !draft) return undefined;
                const next = addRegion(draft, kind);
                setProductState({ board: { description: next }, dialog: null });
                publishTwoPaneState();
                return "cmd-add-device";
            }
            case "use-product-gap": {
                const node = selectedNode();
                const from = Number(el?.dataset?.gapFrom);
                if (!node || !Number.isFinite(from)) return undefined;
                const draft = ensureBoardDraft();
                const index = node.regionIndex;
                const region = Array.isArray(draft?.regions) ? draft.regions[index] : null;
                if (!region) return undefined;
                region.base = from;
                setProductState({ board: { description: draft } });
                publishTwoPaneState();
                return "cmd-use-product-gap";
            }
            case "close-dialog": {
                setProductState({ dialog: null });
                return "cmd-close-dialog";
            }
            // ---- 创建 / 模版 / 重新载入 ----------------------------------
            case "confirm-create": {
                const body = el.closest?.(".pw-dialog") ?? document;
                const nameInput = body.querySelector?.("[data-create-name]");
                const kinds = readCreateKinds(body);
                if (kinds.length === 0) {
                    setNoteInProduct("至少勾一个器件 —— 空白板没有可展示的器件。");
                    return undefined;
                }
                const description = createBoardDraft(nameInput?.value ?? "", kinds);
                setProductState({ dialog: null }, false);
                emitCommand("create-product", "product", { description });
                return "cmd-create-product";
            }
            case "use-template": {
                // ⚠️ 连**寄存器文档**一起取：档案里存了偏移 / 含义 ⇒ 装载时
                // 要按器件身份把它们搬回视图状态的 `docs`（否则「从模版加载」
                // 出来的东西跟保存时不一样）。
                const entry = findTemplateEntry(state, el.dataset.templateId, el.dataset.templateSrc);
                if (!entry) {
                    setNoteInProduct("模版没找到 —— 板档案可能已被删除。");
                    return undefined;
                }
                setProductState({ dialog: null }, false);
                emitCommand("load-archive", "product", {
                    description: entry.description,
                    registers: entry.registers,
                });
                return "cmd-load-template";
            }
            case "save-archive": {
                if (state.loaded === false) {
                    setNoteInProduct("先创建或从模版装载一块板，再另存为档案。");
                    return undefined;
                }
                setProductState({ dialog: "archive" });
                return undefined;
            }
            case "confirm-save-as": {
                const draft = readArchiveDraft(el.closest("[data-dialog]") ?? document);
                setProductState({ dialog: null }, false);
                emitCommand("save-board-as", "product", draft);
                return "cmd-save-board-as";
            }
            case "delete-product-archive": {
                const id = String(el?.dataset?.archiveId ?? "");
                if (!id) return undefined;
                emitCommand("delete-product-archive", "product", { id });
                return "cmd-delete-product-archive";
            }
            case "reload-product": {
                const draft = boardDraft(state);
                emitCommand("reload-product", "product", { description: cloneDescription(draft) });
                return "cmd-reload-product";
            }
            case "discard-product": {
                // 放弃草稿 = 回到「生效中的描述」；寄存器基线也回到内置表。
                setProductState({ board: null, docs: {}, docsBase: {} });
                return "cmd-discard-product";
            }
            default:
                return undefined;
        }
    },
    onDelta(_delta) {
        // No-op until M-18 hooks land.
    },
    mountFor(el) {
        // ⚠️ **事件委托**：`applyRenderEvent` 每次重建 `body.innerHTML` ⇒
        // 监听器必须绑在容器上，不能绑在按钮 / 输入框上。
        el.addEventListener("click", (ev) => productTreeView.onCommand(ev));
        el.addEventListener("input", (ev) => {
            const input = ev.target;
            if (!input?.matches) return;
            if (input.matches("[data-base-input]")) {
                const node = selectedNode();
                if (node) writeRegionNumber(node, "base", input.value);
                return;
            }
            if (input.matches("[data-size-input]")) {
                const node = selectedNode();
                if (node) writeRegionNumber(node, "size", input.value);
                return;
            }
            if (input.matches("[data-reg-field]")) {
                writeRegisterField(
                    String(input.dataset.regField ?? ""),
                    Number(input.dataset.regIndex ?? -1),
                    input.value,
                );
                return;
            }
            if (input.matches("[data-device-param]")) {
                writeDeviceParam(String(input.dataset.deviceParam ?? ""), input.value);
            }
        });
        // 失焦 / 提交时重渲染一次：把草稿归一化显示，并刷新「未生效」标记。
        el.addEventListener("change", (ev) => {
            if (ev.target?.matches?.("[data-base-input],[data-size-input],[data-reg-field],[data-device-param]")) {
                publishTwoPaneState();
            }
        });
    },
});

/**
 * 在产品视图的错误条上放一句话（视图本地提示，不冒充内核错误）。
 *
 * ⚠️ 用 `error` 字段会带 `data-memory-map-error` 钩子 ⇒ 会被当成「内核拒绝
 * 的描述」⇒ 所以这里只写 `notice`，由渲染层单独显示。
 *
 * @param {string} text
 */
function setNoteInProduct(text) {
    setProductState({ notice: String(text ?? "") });
}

/**
 * 把一段描述装进产品视图（装载 / 重载成功后调用）。
 *
 * @param {object} description — 生效的描述
 * @param {{resetDocs?: boolean}} [options]
 */
export function adoptProductDescription(description, options = {}) {
    const state = productState();
    const keepSelected = options.resetDocs === true ? null : (state.selected ?? null);
    const docs = options?.docs && typeof options.docs === "object"
        ? JSON.parse(JSON.stringify(options.docs))
        : {};
    setProductState({
        loaded: true,
        justApplied: false,
        board: { description: cloneDescription(description) },
        tree: state.tree ?? { root: projectTreeFrom(description).root },
        selected: keepSelected,
        ...(options.resetDocs === true ? { docs, docsBase: JSON.parse(JSON.stringify(docs)) } : {}),
    });
}

/**
 * 一段描述在**生效描述**里是否已经落位（按器件身份 + 基址逐段比对）。
 *
 * ⚠️ 用来挡住「事件还没到就扶正基线」的假动作：只有内核真把这份板推出
 * 来了，才把草稿换成它。
 *
 * @param {object|null} description — 生效描述
 * @param {object|null} draft — 刚提交的那份
 * @returns {boolean}
 */
function boardReflectedIn(description, draft) {
    const effRegions = Array.isArray(description?.regions) ? description.regions : [];
    const draftRegions = Array.isArray(draft?.regions) ? draft.regions : [];
    if (draftRegions.length === 0) return false;
    for (const region of draftRegions) {
        const found = region.kind === "peripheral"
            ? effRegions.find((r) => String(r?.device?.id ?? "") === String(region?.device?.id ?? ""))
            : effRegions.filter((r) => r?.kind === region.kind)[
                draftRegions.filter((r) => r?.kind === region.kind).indexOf(region)
            ];
        if (!found) return false;
        if (String(found.base ?? "") !== String(region.base ?? "")) return false;
        if (region.kind !== "peripheral" && String(found.size ?? "") !== String(region.size ?? "")) return false;
    }
    return true;
}

/**
 * 节点的**身份**（跨「内核重排段」仍然指同一个器件）。
 *
 * @param {object|null} node
 * @param {object|null} description
 * @returns {string|null}
 */
function nodeIdentity(node, description) {
    if (!node) return null;
    if (node.kind === "core") return "core";
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    const region = regions[node.regionIndex] ?? null;
    if (!region) return null;
    if (region.kind === "peripheral") return `id:${region?.device?.id ?? ""}`;
    const nth = regions.filter((r) => r?.kind === region.kind).indexOf(region);
    return `kind:${region.kind}:${nth}`;
}

/**
 * 「重新载入」已经生效 ⇒ 把**内核推回来的那份**扶正成新基线。
 *
 * ⚠️ 用内核那份而不是我们提交的那份：内核会把段**按 base 重排** ⇒ 不扶正
 * 就会出现「刚刚载入成功，界面上却写着 N 处未生效」这种自相矛盾的画面
 * （这是真机验收里抓到的）。
 *
 * ⚠️ 只有 `boardReflectedIn` 成立才动 ⇒ 事件没到时什么都不改，等事件来。
 *
 * @returns {boolean} 扶正了吗
 */
export function syncProductBaseline() {
    const state = productState();
    if (state.justApplied !== true) return false;
    const eff = effectiveDescription(state);
    const draft = boardDraft(state);
    if (!eff || !boardReflectedIn(eff, draft)) return false;
    const nodes = buildDeviceNodes(state);
    const nextNodes = buildDeviceNodes({ loaded: true, board: { description: eff }, tree: state.tree });
    const current = nodes.find((node) => node.key === state.selected) ?? null;
    const identity = nodeIdentity(current, draft);
    // ⚠️ **寄存器草稿按器件身份搬家**：`docs` 是按节点 key（`r<i>`）索引的，
    // 而内核重排段之后 key 会变 ⇒ 不搬家就又变成「改了含义一重载就没了」。
    const relocated = {};
    for (const node of nodes) {
        const id = nodeIdentity(node, draft);
        if (!id) continue;
        const target = nextNodes.find((n) => nodeIdentity(n, eff) === id);
        if (!target || target.key === node.key) continue;
        relocated[node.key] = target.key;
    }
    const moveDocs = (source) => {
        const out = {};
        for (const [key, value] of Object.entries(source ?? {})) {
            out[relocated[key] ?? key] = value;
        }
        return out;
    };
    const nextDocs = moveDocs(state.docs);
    const nextSelected = identity ? (selectedKeyFor(eff, identity) ?? state.selected) : state.selected;
    setProductState({
        justApplied: false,
        loaded: true,
        board: { description: cloneDescription(eff) },
        selected: nextSelected,
        docs: nextDocs,
        docsBase: JSON.parse(JSON.stringify(nextDocs)),
    });
    return true;
}

/**
 * 扶正之后，按**身份**把选中项搬到新下标上（内核重排后 `r<i>` 会变）。
 *
 * @param {object|null} description
 * @param {string} identity
 * @returns {string|null}
 */
function selectedKeyFor(description, identity) {
    const nodes = buildDeviceNodes({
        loaded: true,
        board: { description },
        tree: productState().tree,
    });
    for (const node of nodes) {
        if (nodeIdentity(node, description) === identity) return node.key;
    }
    return null;
}

/**
 * 点「重新载入」成功之后：把当前草稿扶正成**新基线**。
 *
 * ⚠️ 「生效」与 lock **无关**：锁定状态下也能重载（lock 只门控编辑）。
 */
export function markProductApplied() {
    const state = productState();
    setProductState({
        justApplied: true,
        docsBase: JSON.parse(JSON.stringify(state.docs ?? {})),
    }, false);
    // 事件已经到了（本函数跑在 `LoadProject` 之后）⇒ 立刻扶正；没到就等
    // 事件来（`app.js` 的内存图回落会再调一次）。
    syncProductBaseline();
}

// ---------------------------------------------------------------------------
// 固件加载（STAGE-32 · v0.47 批 → **v0.58 批：入口收敛成一个**）—— 视图层这一侧
// ---------------------------------------------------------------------------

// ⚠️ **`pendingImage()` 与 `loadImageFileIntoPending()` 已经搬走**
// （STAGE-33 · v0.48 批）⇒ ⇒ 它们现在住在
// `components/image-loader.js` ⇒ ⇒ **页头那个入口也共用同一段**（页头在
// **app 层** ⇒ ⇒ 视图层够不着 ⇒ ⇒ 要共用就只能**提到共享组件**）⇒ ⇒
// **不写第二套**（规格 §3 裁决 D）。
//
// ⚠️ **v0.58 批**：视图层这边**连入口也没有了**⇒ ⇒ 产品视图里那块面板
// 降级成**纯状态显示**（`renderImageLoadStatus`）⇒ ⇒ 装载入口**只有页头
// 那一处**（用户定案「只应该有一个入口」）⇒ ⇒ 视图层**不再**发
// `load-image` 信号 ⇒ ⇒ `onCommand` 里**没有**那个分支了。
//
// ⚠️ **为什么两个入口会咬人**（根因，值得记着）：两个入口共用**同一份**
// `pending` ⇒ ⇒ 用户在**一处**装完（成功 ⇒ ⇒ `pending` 被清掉）⇒ ⇒ 到
// **另一处**再点 ⇒ ⇒ 撞上「没有待加载的文件」⇒ ⇒ 而那次装载**其实成功
// 了**⇒ ⇒ 界面在骗人。⇒ ⇒ 一个功能**只留一个入口**，状态就不会有两套。

// ---------------------------------------------------------------------------
// embeddedView — 落地视图（D-33 · STAGE-10 裁决 C）。
//
// ⚠️ **为什么是「补组件」而不是「从落地视图列表去掉」**：去掉 ⇒ 嵌入式角色
// **没有**落地视图 ⇒ tab 条上的「嵌入式」变成死标签（点了落到 fallback）
// ⇒ 得同时改 tab 条与 `ROLE_DEFAULT_VIEW` ⇒ 改动面更大、用户拿到的信息更少
// （STAGE-10 §1 裁决 C）。
//
// ⚠️ **它不是第二个寄存器表**：这里是**概览 + 入口** —— 连接状态、当前 PC、
// 虚拟时间，加上**三个**子视图的入口。寄存器与内存的明细在**调试视图**的
// 「寄存器窗口」与「内存窗口」里（v0.65 批 · 「寄存器 / 内存」那个独立
// 视图已撤销 ⇒ ⇒ 入口也少一个）。
// ---------------------------------------------------------------------------

/**
 * 落地视图的**三个子视图入口**（裁决 C 的最小内容之一 · v0.65 批减一个）。
 *
 * ⚠️ **这里只列入口**：某个入口当前**不可达**（能力位不满足）时，由 M-01 的
 * 兜底页处理（E_CTX-05）⇒ 前端不再另造一份「可达性」真相。
 *
 * @type {ReadonlyArray<{view: string, label: string}>}
 */
export const EMBEDDED_VIEW_ENTRIES = [
    { view: "debug", label: "调试视图" },
    { view: "waveform", label: "波形视图" },
    { view: "source", label: "源码视图" },
];

const embeddedView = register({
    id: "embedded",
    channels: ["sim"],
    render(state) {
        // PC **只**来自既有状态源（M-10 推进的 `sim.pc`）⇒ 落地视图**不造**
        // 第二个状态源（与调试视图同一条规矩）。
        //
        // ⚠️ **虚拟时间**（`timeVirt`）**不在这里**（STAGE-45 · v0.69 批 ·
        //   台账 **D-173** 做法 A）⇒ ⇒ 顶栏那份是**全屏都在**的那一份 ⇒ ⇒
        //   落地视图**不再显示第二份** ⇒ ⇒ 「同一个量只显示一处」这条规矩
        //   **在落地视图上终于成立**。
        const pc = Number(state?.sim?.pc ?? 0) >>> 0;
        // 连接状态来自 **M-07 的传输层状态**（由 `app.js` 推进视图状态）⇒
        // 视图层**不**直接摸服务层。
        const link = state?.link ?? {};
        const connected = link.connected === true;
        return `
            <section class="embedded-view">
                <h3>嵌入式工作台概览</h3>
                <p class="link-status" data-embedded-link data-connected="${connected ? "1" : "0"}">
                    内核连接：<strong>${connected ? "已连接" : "未连接"}</strong>${connected
                        ? ""
                        : `<span class="muted">（${escapeHtml(String(link.lastError ?? "内核传输层未就绪"))}）</span>`}
                </p>
                <div class="cards">
                    <div class="card kpi">
                        <span class="k">当前 PC</span>
                        <code class="v pc" data-embedded-pc>0x${pc.toString(16).padStart(8, "0")}</code>
                    </div>
                    <div class="card kpi">
                        <span class="k">内核连接</span>
                        <span class="v">${connected ? "已连接" : "未连接"}</span>
                    </div>
                </div>
                <section class="card">
                    <div class="row"><h3>子视图</h3><span class="tag">${EMBEDDED_VIEW_ENTRIES.length} 个入口</span></div>
                    <ul class="embedded-entries">
                        ${EMBEDDED_VIEW_ENTRIES.map((entry) => `
                            <li>
                                <button type="button" data-action="go-view" data-embedded-entry="${escapeHtml(entry.view)}" data-view-target="${escapeHtml(entry.view)}">${escapeHtml(entry.label)}</button>
                            </li>`).join("")}
                    </ul>
                </section>
            </section>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        if (el?.dataset?.action !== "go-view") return undefined;
        const target = String(el.dataset.viewTarget ?? "").trim();
        if (!target) return undefined;
        // ⚠️ **路由归 M-01**（M-02 LLD §9：M-02 → M-01 单向依赖，M-02
        // 「不自行实现路由」）⇒ 组件**请求**跳转，不自己改 `location.hash`，
        // 也不自己做能力位复校验（M-01 的 `route()` 两件都做了）。
        route(`#/${target}`);
        return `cmd-go-view-${target}`;
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => embeddedView.onCommand(ev));
    },
});

// ---------------------------------------------------------------------------
// trace 导出 → **下载**（D-88 · STAGE-10 裁决 D）。
//
// ⚠️ **为什么不是「改 LLD 措辞」**：M-11 LLD 写下的承诺是「分块下载」⇒
// 兑现承诺比改文档便宜（STAGE-10 §1 裁决 D）。
//
// ⚠️ **文件名由内容派生**（源 + 格式）⇒ 不是写死的同一个名字：
// `Execution` + `Csv` ⇒ `trace-execution.csv`；`Waveform` + `Vcd` ⇒
// `trace-waveform.vcd`。
// ---------------------------------------------------------------------------

/**
 * 把文件名的一段**消毒**成安全字符集。
 *
 * ⚠️ 源 / 格式来自内核（契约字段）⇒ 不能假定它们一定是字母数字 ⇒
 * 文件名里只留 `[a-z0-9_-]`，其余一律换成 `-`（NFR-04 的同一道防线：
 * 不把不可信的值原样带进文件系统）。
 *
 * ⚠️ **`.` 也不留**：扩展名的那个点由 `exportFileName` 自己拼 ⇒ 段里再带点
 * 就可能出现 `..`（路径穿越的形状）。
 *
 * @param {string} text
 * @returns {string}
 */
function sanitizeFileNamePart(text) {
    return String(text)
        .replace(/[^a-z0-9_-]+/g, "-")
        .replace(/^-+|-+$/g, "");
}

/**
 * trace 卡片那个下载按钮的**文案**（**跟着当前这份导出走**）。
 *
 * ⚠️ **为什么文案要跟着变**：v0.63 批把「导出波形 (VCD)」搬到了**波形视图**⇒ ⇒
 *   用户在那边导出的波形**也要能落盘**⇒ ⇒ 下载按钮**不能**写死「轨迹 CSV」⇒ ⇒
 *   它**跟着当前这份导出的源 / 格式走**（默认 = 执行轨迹 CSV）⇒ ⇒ 「导出波形」
 *   之后点它 ⇒ ⇒ 下的是**整份波形**（不是「只能下载轨迹」）。
 *
 * ⚠️ **默认值与 app 层逐字一致**（`Execution` / `Csv`）⇒ ⇒ 按钮上写的与
 *   实际下的**不可能**对不上。
 *
 * @param {unknown} source — 导出源（契约的 `TraceSource`）
 * @param {unknown} format — 导出格式（契约的 `TraceFormat`）
 * @returns {string}
 */
export function exportDownloadLabel(source, format) {
    const src = String(source ?? "").trim() || TRACE_SOURCE.Execution;
    const fmt = String(format ?? "").trim() || TRACE_FORMAT.Csv;
    const label = src === TRACE_SOURCE.Waveform
        ? "波形"
        : (src === TRACE_SOURCE.Execution ? "轨迹" : "导出内容");
    return `下载${label}到本地 (${fmt.toUpperCase()})`;
}

/**
 * 从导出状态派生**文件名**（源 + 格式）。
 *
 * ⚠️ 这是**纯函数** ⇒ 门禁可以直接断言「文件名由内容派生」，不需要真的
 * 下载一次。
 *
 * @param {unknown} source — 导出源（契约的 `TraceSource`，如 `Execution`）
 * @param {unknown} format — 导出格式（契约的 `TraceFormat`，如 `Csv`）
 * @returns {string} 例如 `trace-execution.csv`
 */
export function exportFileName(source, format) {
    const src = sanitizeFileNamePart(String(source ?? "").trim().toLowerCase()) || "trace";
    const fmt = sanitizeFileNamePart(String(format ?? "").trim().toLowerCase()) || "txt";
    return `trace-${src}.${fmt}`;
}

/**
 * 导出格式 → MIME 类型（`Blob` 的 `type`）。
 *
 * @param {unknown} format
 * @returns {string}
 */
export function exportMimeType(format) {
    switch (String(format ?? "").trim().toLowerCase()) {
        case "csv": return "text/csv;charset=utf-8";
        case "json": return "application/json;charset=utf-8";
        case "vcd": return "text/plain;charset=utf-8";
        default: return "text/plain;charset=utf-8";
    }
}

/**
 * 把当前导出块**下载**成文件（D-88）。
 *
 * 做法（STAGE-10 §1 裁决 D）：`Blob` → `URL.createObjectURL` →
 * `<a download="…">` → 点击 → `URL.revokeObjectURL`。
 *
 * ⚠️ **`download` 属性用 `setAttribute` 设置**（NFR-04）：文件名**不**拼进
 * HTML 字符串 ⇒ 它永远不经过标记解析器。
 *
 * ⚠️ **释放是「点击之后」**（`setTimeout(..., 0)`）：同步释放会把**还没开始**
 * 的下载打断（Firefox）；而默认动作（真正的下载）在事件派发**之后**发生 ⇒
 * 下一个宏任务里释放是安全的（不泄漏，也不打断下载）。
 *
 * ⚠️ **视图层不碰服务层**：这里只用浏览器原生能力，不发任何契约命令。
 *
 * @param {{source?: unknown, format?: unknown, chunk?: unknown}|null|undefined} exportState
 * @param {{document?: Document, URL?: typeof URL, Blob?: typeof Blob}} [deps] — 注入点（测试用）
 * @returns {{filename: string, url: string, href: string, download: string, content: string, anchor: object}|null}
 *   — `null` 表示没有可下载的内容。
 */
export function downloadExportChunk(exportState, deps = {}) {
    if (!exportState) return null;
    const doc = deps.document ?? globalThis.document;
    const urlApi = deps.URL ?? globalThis.URL;
    const BlobCtor = deps.Blob ?? globalThis.Blob;
    if (!doc || !urlApi || !BlobCtor) return null;
    const filename = exportFileName(exportState.source, exportState.format);
    const content = String(exportState.chunk ?? "");
    const blob = new BlobCtor([content], { type: exportMimeType(exportState.format) });
    const url = urlApi.createObjectURL(blob);
    const anchor = doc.createElement("a");
    anchor.setAttribute("href", url);
    anchor.setAttribute("download", filename);
    // ⚠️ 这是**机器可读**的标记：真机 / 门禁按它取下载动作，别去猜文案。
    anchor.setAttribute("data-trace-download", filename);
    doc.body?.appendChild(anchor);
    anchor.click?.();
    anchor.parentNode?.removeChild?.(anchor);
    setTimeout(() => urlApi.revokeObjectURL(url), 0);
    return { filename, url, href: url, download: filename, content, anchor };
}

// ---------------------------------------------------------------------------
// 任意地址输入 — 断点 / 读内存 (STAGE-39 · v0.54 批)
//
// ⚠️ **一条约束一个落点**（裁决 E）：「把用户输入的文本解析成一个地址」
// **只允许有一处实现** ⇒ ⇒ 断点入口与读内存入口**共用**它 ⇒ ⇒ 前端
// **不写第二份校验器**。
//
// ⚠️ **前端只挡「压根不是一个数」**（裁决 E）：**不**预校验对齐、**不**查
// 地址是否已映射、**不**查那个地址上有没有断点 ⇒ ⇒ 那些都是**内核的判据**
// ⇒ ⇒ 奇数地址**照发** ⇒ ⇒ 由内核回错误。
//
// ⚠️ **契约与内核早就够用了**（F1 / F2 / F4）：`Breakpoint.addr` 与
// `ReadMemory.addr` **早有** ⇒ ⇒ 本节**零契约变更**（裁决 L）。
// ---------------------------------------------------------------------------

/**
 * 地址输入的**唯一提示文案**（STAGE-39 · v0.54 批）。
 *
 * ⚠️ **一处定义** ⇒ ⇒ 两个入口**说同一句话** ⇒ ⇒ 用户不用学两套。
 *
 * @type {string}
 */
export const ADDR_INPUT_ERROR = "请先填十六进制地址（最多 8 位，可带 0x 前缀）。";

/**
 * 「**读内存**」与「**读这一段**」这两个入口的**差别说明**（v0.64 批）。
 *
 * ⚠️ **为什么要有这句**：用户反馈「**读内存按钮点击后没有反应，我希望可以读取
 *   指定数量的内存**」⇒ ⇒ 真机实测发现两件事：① 「读内存」**填了地址就真的能
 *   读出来**（不是坏了）⇒ ⇒ 问题是**第一次点**（地址框默认空 ⇒ ⇒ 静默失败 ⇒ ⇒
 *   只留一行**贴在按钮右边的小字** ⇒ ⇒ 看着就是「没反应」）；② 「读**指定数量**」
 *   那条路**早就存在**（契约的 `DumpMemory`，≤ 4096 字节）⇒ ⇒ 只是**没人知道
 *   那个叫「转储」的入口就是它** ⇒ ⇒ ⇒ 所以这句把**两个入口的差别**讲明白。
 *
 * ⚠️ **两种读法**语义**真的不同**，不是同一个动作的两种叫法：
 *   - **读内存** = 读**一个字**（1 / 2 / 4 字节）⇒ ⇒ **走总线** ⇒ ⇒ 探针会动
 *     ⇒ ⇒ 结果进「内存单元」表；
 *   - **读这一段** = 读**你指定的字节数**（≤ 4096）⇒ ⇒ **调试视角**（不计总线
 *     统计、不触发探针）⇒ ⇒ 结果是十六进制 + ASCII 表。
 *
 * ⚠️ **不许出现写死的地址字面量**（STAGE-39 订正 4 · A-8）⇒ ⇒ 这句里**没有**
 *   任何具体地址 ⇒ ⇒ 「默认空」那个决定**不变**（读内存是**真的任意地址入口**，
 *   不是一条写死的快捷方式）⇒ ⇒ 而**地址框会记住你上次读的那个**（草稿住
 *   视图状态）⇒ ⇒ 只有**第一次**需要填。
 *
 * ⚠️ **这是 HTML 片段**（不是纯文本）⇒ ⇒ 渲染时**不许**再 `escapeHtml`
 *   ⇒ ⇒ 否则 `**…**` 会**原样显示**（界面上出现一串 markdown 记号）⇒ ⇒ 要强调
 *   的地方用 `<strong>`。⚠️ **里面没有任何用户数据** ⇒ ⇒ 不转义是安全的。
 *
 * @type {string}
 */
export const MEMORY_READ_HINT = "<strong>读内存</strong> 读<strong>一个字</strong>"
    + "（走总线 ⇒ ⇒ 探针会动）；"
    + `<strong>读这一段</strong> 读<strong>你指定的字节数</strong>（≤ ${DUMP_MAX_BYTES} 字节，调试视角 ⇒ ⇒ 不动总线）。`
    + "地址框会<strong>记住你上次填的那个</strong> ⇒ ⇒ 只有第一次要填。";

/**
 * 「读**这一段**」那个按钮的**文案**（v0.64 批）。
 *
 * ⚠️ **为什么不再叫「转储」**：`DumpMemory` 是契约里的词 ⇒ ⇒ 但用户看到的是
 *   「**我要读指定数量的内存**」⇒ ⇒ 「读这一段」说的是**他要做的事**⇒ ⇒ 而
 *   「转储」说的是**实现**⇒ ⇒ 界面上说**目的**、文档里说**实现**。
 *
 * @type {string}
 */
export const MEMORY_DUMP_BUTTON_LABEL = "读这一段";

/**
 * 把用户输入的文本解析成一个地址（**唯一落点** · 裁决 E）。
 *
 * ⚠️ **只挡「压根不是一个数」**：最多 8 位十六进制、可带 `0x` 前缀 ⇒ ⇒
 * 解析不出来 ⇒ ⇒ **格式不对**；其余（对齐 / 是否已映射 / 是否已有断点）
 * **一律放行**给内核 ⇒ ⇒ 前端**不写第二份校验器**。
 *
 * ⚠️ **按十六进制解析**（裁决 A）：界面上所有地址都是 `0x…` 的写法 ⇒ ⇒
 * 裸数字也按十六进制读 ⇒ ⇒ 与显示口径一致。
 *
 * @param {unknown} text
 * @returns {{ok: true, value: number}|{ok: false, error: string}}
 */
export function parseAddrInput(text) {
    const raw = String(text ?? "").trim();
    const body = /^0[xX]/.test(raw) ? raw.slice(2) : raw;
    if (!/^[0-9a-fA-F]{1,8}$/.test(body)) {
        return { ok: false, error: ADDR_INPUT_ERROR };
    }
    return { ok: true, value: Number.parseInt(body, 16) >>> 0 };
}

/**
 * **断点地址**输入行（共享片段 · 裁决 K）。
 *
 * ⚠️ **一个片段两处用**：调试视图的「断点」卡 ⇒ ⇒ 片段**只有一份**
 * ⇒ ⇒ 改一次两处都改。
 *
 * @param {{draft?: string, error?: string}} [opts]
 * @returns {string}
 */
export function renderBreakpointAddrRow({ draft = "", error = "" } = {}) {
    return `
        <div class="addr-input" data-addr-input="breakpoint">
            <input type="text" class="addr-field" data-bp-addr-input value="${escapeHtml(draft)}"
                placeholder="断点地址（十六进制）" spellcheck="false" autocomplete="off">
            <button type="button" data-action="set-breakpoint">加断点</button>
            <span class="addr-error" data-bp-addr-error>${escapeHtml(error)}</span>
        </div>`;
}

/**
 * **读内存**的地址 + 宽度输入行（共享片段 · 裁决 D / K）。
 *
 * ⚠️ **宽度只用契约的 `WRITE_SIZE`**（三档）⇒ ⇒ 不新造枚举 ⇒ ⇒ 前端与
 * 内核说的是同一组词。
 *
 * @param {{draft?: string, size?: string, error?: string}} [opts]
 * @returns {string}
 */
export function renderMemoryReadRow({ draft = "", size = WRITE_SIZE.Word, error = "" } = {}) {
    const sizes = [WRITE_SIZE.Byte, WRITE_SIZE.Half, WRITE_SIZE.Word];
    const current = Object.prototype.hasOwnProperty.call(WRITE_SIZE, size) ? size : WRITE_SIZE.Word;
    return `
        <div class="addr-input" data-addr-input="memory">
            <input type="text" class="addr-field" data-mem-addr-input value="${escapeHtml(draft)}"
                placeholder="内存地址（十六进制）" spellcheck="false" autocomplete="off">
            <select class="addr-field" data-mem-size>
                ${sizes.map((s) => `<option value="${s}"${s === current ? " selected" : ""}>${s}</option>`).join("")}
            </select>
            <button type="button" data-action="read-mem">读内存</button>
            <span class="addr-error" data-mem-addr-error>${escapeHtml(error)}</span>
        </div>`;
}

// ---------------------------------------------------------------------------
// 契约 v9 · 调试器增强批（符号查询 + 条件断点 + 整段转储）
//
// ⚠️ **DOM 契约逐字照规格 §5.1 / §5.2** ⇒ ⇒ `data-cond-input` /
//   `data-bp-cond-input` / `data-bp-cond-error` / `data-symbol-input` /
//   `data-bp-symbol-input` / `data-action="lookup-symbol"` /
//   `data-bp-symbol-error` / `data-bp-symbol-candidates` /
//   `data-dump-input` / `data-dump-addr-input` / `data-dump-len-input` /
//   `data-action="dump-memory"` / `data-dump-error` / `data-mem-dump`。
//
// ⚠️ **视图层是纯渲染**：不碰命令、不发请求、不解析条件表达式 ⇒ ⇒
//   条件**是不是合法**由**内核**说（前端只显示它的原文）。
// ---------------------------------------------------------------------------

/**
 * **条件表达式**输入行（共享片段 · 契约 v9 §5.1 · 裁决 O）。
 *
 * ⚠️ **一个条件框，地址断点与符号断点共用** ⇒ ⇒ 不为符号断点单开一个。
 *
 * ⚠️ **前端不解析条件** ⇒ ⇒ 这里**只**是一个文本框 ⇒ ⇒ 合法性由内核判
 *   （`E_DBG-02`）⇒ ⇒ 前端显示内核原文。
 *
 * @param {{draft?: string, error?: string}} [opts]
 * @returns {string}
 */
export function renderBreakpointCondRow({ draft = "", error = "" } = {}) {
    return `
        <div class="cond-input" data-cond-input="breakpoint">
            <input type="text" class="addr-field" data-bp-cond-input value="${escapeHtml(draft)}"
                placeholder="条件（可选），例如 x1 == 5" spellcheck="false" autocomplete="off">
            <span class="addr-error" data-bp-cond-error>${escapeHtml(error)}</span>
        </div>`;
}

/**
 * 一条符号候选的**可点按钮**（契约 v9 §5.1 · 裁决 N）。
 *
 * ⚠️ **点一下直接下断点** ⇒ ⇒ `<select>` 的「先选中、再点按钮」那**两步**
 *   被省掉 ⇒ ⇒ 多一步零收益。
 *
 * ⚠️ **形状钉死**（真机门禁按**具体值**断言）：`class="symbol-candidate"` ·
 *   `data-addr` = `0x` + **8 位小写**十六进制 · `data-symbol` = 符号名 ·
 *   按钮文本 = `<名字> @ <地址>`。
 *
 * ⚠️ **没有地址的符号**（`addr === null`）**不能**下断点 ⇒ ⇒ 如实显示成
 *   一行字（**不编**一个地址出来）⇒ ⇒ 它的 class **不是**
 *   `symbol-candidate` ⇒ ⇒ 「可点的候选个数」不被它污染。
 *
 * @param {{name?: string, addr?: number|null, size?: number|null}} symbol
 * @returns {string}
 */
export function renderSymbolCandidate(symbol) {
    const name = String(symbol?.name ?? "");
    const addr = symbol?.addr;
    if (typeof addr !== "number" || !Number.isFinite(addr)) {
        return `<span class="muted symbol-candidate-na" data-bp-symbol-name="${escapeHtml(name)}">${escapeHtml(name)}（无地址，不能下断点）</span>`;
    }
    const addrText = `0x${(addr >>> 0).toString(16).padStart(8, "0")}`;
    return `<button type="button" class="symbol-candidate" data-action="set-symbol-breakpoint"
        data-addr="${addrText}" data-symbol="${escapeHtml(name)}">${escapeHtml(name)} @ ${addrText}</button>`;
}

/**
 * 候选区（契约 v9 §5.1）。
 *
 * ⚠️ **精确命中也在列表里，且排第一** ⇒ ⇒ 用户填了全名 ⇒ ⇒ 列表里就是
 *   那一条 ⇒ ⇒ 点它。
 *
 * ⚠️ **「没有这个符号」有两态**（C-6）：`total == 0` ⇒ ⇒ 这个镜像**没有
 *   符号表**（可能被 strip 了）；`total > 0` ⇒ ⇒ 这个镜像**共有 N 个符号**
 *   ⇒ ⇒ 两态**必须能区分**（否则用户分不清「镜像没符号」与「名字写错了」）。
 *
 * @param {{name?: string, symbol?: object|null, candidates?: object[], total?: number}|null} result
 * @returns {string}
 */
export function renderSymbolCandidates(result) {
    if (!result) return "";
    const exact = result.symbol ?? null;
    const candidates = Array.isArray(result.candidates) ? result.candidates : [];
    const total = Number(result.total ?? 0);
    if (!exact && candidates.length === 0) {
        const why = total === 0
            ? "这个镜像没有符号表（可能被 strip 了）"
            : `这个镜像共有 ${total} 个符号`;
        return `<p class="muted" data-bp-symbol-empty>没有这个符号 —— ${escapeHtml(why)}</p>`;
    }
    const items = [];
    if (exact) items.push(exact);
    for (const c of candidates) items.push(c);
    return items.map((s) => renderSymbolCandidate(s)).join("");
}

/**
 * **符号查询**入口（共享片段 · 契约 v9 §5.1 · 裁决 L / M / N）。
 *
 * ⚠️ **与地址框分开的另一个入口**（裁决 L）⇒ ⇒ 「符号」框**只**收符号名、
 *   「地址」框**只**收十六进制地址 ⇒ ⇒ 不做「一个框自动分流」。
 *
 * ⚠️ **查询按显式动作触发**（裁决 M：「查找」按钮 / 回车）⇒ ⇒ 一次输入 =
 *   一次查询 ⇒ ⇒ 没有「边打字边查」的乱序问题。
 *
 * @param {{draft?: string, error?: string, result?: object|null}} [opts]
 * @returns {string}
 */
export function renderSymbolLookupRow({ draft = "", error = "", result = null } = {}) {
    const query = result ? String(result.name ?? "") : "";
    return `
        <div class="symbol-input" data-symbol-input="breakpoint">
            <input type="text" class="addr-field" data-bp-symbol-input value="${escapeHtml(draft)}"
                placeholder="符号名（例如 main）" spellcheck="false" autocomplete="off">
            <button type="button" data-action="lookup-symbol">查找</button>
            <span class="addr-error" data-bp-symbol-error>${escapeHtml(error)}</span>
            <div class="symbol-candidates" data-bp-symbol-candidates${query ? ` data-bp-symbol-query="${escapeHtml(query)}"` : ""}>${renderSymbolCandidates(result)}</div>
        </div>`;
}

/**
 * **整段转储**的输入行（共享片段 · 契约 v9 §5.2 · 裁决 D / P）。
 *
 * ⚠️ **上限的字面量来自契约镜像**（`DUMP_MAX_BYTES`）⇒ ⇒ 界面上的
 *   「≤ 4096」不是抄的 ⇒ ⇒ 夹具一改，界面跟着改。
 *
 * @param {{addrDraft?: string, lenDraft?: string, error?: string}} [opts]
 * @returns {string}
 */
export function renderMemoryDumpRow({ addrDraft = "", lenDraft = "256", error = "" } = {}) {
    return `
        <div class="dump-input" data-dump-input="memory">
            <input type="text" class="addr-field" data-dump-addr-input value="${escapeHtml(addrDraft)}"
                placeholder="转储起始地址（十六进制）" spellcheck="false" autocomplete="off">
            <input type="text" class="addr-field" data-dump-len-input value="${escapeHtml(lenDraft)}"
                placeholder="字节数（≤ ${DUMP_MAX_BYTES}）" spellcheck="false" autocomplete="off">
            <button type="button" data-action="dump-memory">${escapeHtml(MEMORY_DUMP_BUTTON_LABEL)}</button>
            <span class="addr-error" data-dump-error>${escapeHtml(error)}</span>
        </div>
        <p class="muted mem-read-hint" data-mem-read-hint>${MEMORY_READ_HINT}</p>`;
}

/**
 * 转储结果（**偏移 | 十六进制 | ASCII** 三列 · 契约 v9 §5.2）。
 *
 * ⚠️ **形状钉死**（真机门禁按**具体值**断言）：
 *   - 外层是 `<div class="mem-dump" data-mem-dump>` ⇒ ⇒ 里面是
 *     `<table class="dump-table">`；
 *   - **偏移列**：**8 位小写十六进制**、**不带** `0x` 前缀（相对转储起点）；
 *   - **十六进制列**：每字节 **2 位小写**十六进制、**空格分隔**、**每行 16
 *     字节**（最后一行可以不足 16 个）；
 *   - **ASCII 列**：`0x20`..`0x7e` **原样**，其余一律 `.`；
 *   - ⚠️ **字节序按地址递增**（低地址在前）。
 *
 * ⚠️ **两个计数写进容器**（`data-mem-dump-rows` / `data-mem-dump-bytes`）
 *   ⇒ ⇒ 判据能断言**具体数字**（不是「非空」）。
 *
 * @param {{addr?: number, bytes?: number[]}|null} dump
 * @returns {string}
 */
export function renderMemoryDump(dump) {
    const bytes = dump && Array.isArray(dump.bytes) ? dump.bytes : [];
    const rows = [];
    for (let i = 0; i < bytes.length; i += 16) {
        const chunk = bytes.slice(i, i + 16);
        const hex = chunk.map((b) => (Number(b) & 0xff).toString(16).padStart(2, "0")).join(" ");
        const ascii = chunk
            .map((b) => {
                const v = Number(b) & 0xff;
                return v >= 0x20 && v <= 0x7e ? String.fromCharCode(v) : ".";
            })
            .join("");
        const off = i.toString(16).padStart(8, "0");
        rows.push(`<tr class="dump-row" data-dump-off="${off}">
            <td class="dump-off">${off}</td>
            <td class="dump-hex">${escapeHtml(hex)}</td>
            <td class="dump-ascii">${escapeHtml(ascii)}</td>
        </tr>`);
    }
    const body = rows.length === 0
        ? `<p class="muted" data-mem-dump-empty>尚无转储 — 在上面填起始地址与字节数，点「${escapeHtml(MEMORY_DUMP_BUTTON_LABEL)}」。</p>`
        : `<table class="dump-table">
            <thead><tr><th>偏移</th><th>十六进制</th><th>ASCII</th></tr></thead>
            <tbody>${rows.join("")}</tbody>
        </table>`;
    return `<div class="mem-dump" data-mem-dump data-mem-dump-rows="${rows.length}" data-mem-dump-bytes="${bytes.length}">${body}</div>`;
}

/**
 * ⚠️ **草稿的共享命名空间**（STAGE-39 · v0.54 批 · 裁决 G）。
 *
 * ⚠️ **为什么不是某个视图自己的状态**：断点入口在**调试视图**、读内存
 * 入口在**调试视图与寄存器视图两处** ⇒ ⇒ 两处**必须**看到同一份草稿
 * ⇒ ⇒ 放在一个**共享**的命名空间里 ⇒ ⇒ **一个真相**。
 *
 * ⚠️ **`setState` 不校验 viewId** ⇒ ⇒ 这个键**不是**一个视图 ⇒ ⇒ 不会
 * 被工作台派发渲染 ⇒ ⇒ 写草稿**不触发重渲染** ⇒ ⇒ 输入框不被重建 ⇒ ⇒
 * 焦点不丢。
 *
 * @type {string}
 */
export const DEBUGGER_STATE_KEY = "debugger";

/**
 * 「**按步数单步**」那个输入框的**默认步数**（v0.63 批）。
 *
 * ⚠️ **默认 10**：用户拍板「在反汇编窗口中增加一个输入框和一个按钮，用于支持
 *   用户指定执行多少步」⇒ ⇒ 这条路**取代**了原先那个写死的「单步 ×10」⇒ ⇒
 *   默认值取它 ⇒ ⇒ 习惯不变。
 *
 * @type {string}
 */
export const DEFAULT_STEP_COUNT = "10";

/**
 * 读**当前草稿**（断点地址 / 读内存地址 / 宽度 / 条件 / 符号 / 转储 / 步数）。
 *
 * ⚠️ **条件、符号、转储的草稿也住这里**（契约 v9）⇒ ⇒ 渲染会重建 DOM ⇒ ⇒
 *   草稿住在 DOM 里会被清空（本仓踩过这个坑，D-119）⇒ ⇒ 草稿**必须**在
 *   视图状态里。
 *
 * @returns {{bpAddr: string, bpCond: string, bpSymbol: string, memAddr: string, memSize: string, dumpAddr: string, dumpLen: string}}
 */
export function debuggerDraft() {
    const s = viewStore.getState(DEBUGGER_STATE_KEY) ?? {};
    const size = String(s.memSize ?? WRITE_SIZE.Word);
    return {
        bpAddr: String(s.bpAddr ?? ""),
        bpCond: String(s.bpCond ?? ""),
        bpSymbol: String(s.bpSymbol ?? ""),
        memAddr: String(s.memAddr ?? ""),
        memSize: Object.prototype.hasOwnProperty.call(WRITE_SIZE, size) ? size : WRITE_SIZE.Word,
        dumpAddr: String(s.dumpAddr ?? ""),
        dumpLen: String(s.dumpLen ?? DEFAULT_DUMP_LEN),
        // ⚠️ **步数草稿也住这里**（v0.63 批）⇒ ⇒ 渲染会重建 DOM ⇒ ⇒ 草稿住在
        //   DOM 里会被清空 ⇒ ⇒ 用户填的步数**必须**在视图状态里。
        stepCount: String(s.stepCount ?? DEFAULT_STEP_COUNT),
    };
}

/**
 * 转储长度的**默认草稿**（字节）。
 *
 * ⚠️ **只在这里写一次** ⇒ ⇒ 界面上的默认值与门禁里的默认值是**同一个数**
 * ⇒ ⇒ 不是两个真相。
 *
 * @type {string}
 */
export const DEFAULT_DUMP_LEN = "256";

/**
 * 写**草稿**（**不重渲染** · 裁决 G）。
 *
 * ⚠️ **不在这里重渲染**：每敲一个键就重渲染 ⇒ ⇒ 输入框被重建 ⇒ ⇒ 焦点
 * 没了 ⇒ ⇒ 没法连续打字（terminal 的 `term.draft` 同一惯例 · F10）。
 *
 * @param {{bpAddr?: string, memAddr?: string, memSize?: string}} patch
 */
function setDebuggerDraft(patch) {
    viewStore.setState(DEBUGGER_STATE_KEY, (prev) => ({ ...prev, ...patch }));
}

/**
 * 就地写**错误提示**（**不重渲染** · 裁决 F）。
 *
 * ⚠️ **提示必须看得见**：「点了没反应」是最差的 UX ⇒ ⇒ 解析不出来就在
 * 那个输入框**旁边**写一行字 ⇒ ⇒ 不重渲染 ⇒ ⇒ 输入框不被重建 ⇒ ⇒ 焦点
 * 也不丢。
 *
 * @param {Element|null} box — 输入框所在的容器
 * @param {string} selector — 错误位的选择器
 * @param {string} message — 空串 = 清掉
 */
function setAddrError(box, selector, message) {
    const el = box?.querySelector?.(selector);
    if (!el) return;
    el.textContent = message ?? "";
}

/**
 * 把「点一下」翻译成「一条命令的地址参数」（**两个来源** · 裁决 B）。
 *
 * 1. **按钮自带 `data-addr`** —— 反汇编行 / 断点列表的**旧**按钮 ⇒ ⇒
 *    **原样传** ⇒ ⇒ 那条路的**行为一个字不变**；
 * 2. **按钮旁边有地址输入框** —— 本批新增的入口 ⇒ ⇒ 读那个输入框 ⇒ ⇒
 *    **用户给什么地址就下在什么地址**。
 *
 * ⚠️ **解析不出来 ⇒ ⇒ 不发命令**（裁决 F）⇒ ⇒ 返回 `{invalid: true}`
 * ⇒ ⇒ 调用方**直接返回** ⇒ ⇒ **一条命令都不发**。
 *
 * ⚠️ **契约 v9 增加了三条动作**（`lookup-symbol` / `set-symbol-breakpoint` /
 *   `dump-memory`）⇒ ⇒ 它们的参数**也从界面来** ⇒ ⇒ 同一条路。
 *
 * @param {string} action
 * @param {Element|null} el — 被点的那个按钮
 * @returns {{addr?: number|string, size?: string, name?: string, symbol?: string|null, condition?: string|null, len?: number, invalid?: boolean}|null}
 */
function resolveAddrParams(action, el) {
    // --- 契约 v9 的三条新动作（各有各的界面来源）--------------------------
    if (action === "set-symbol-breakpoint") {
        // ⚠️ 候选按钮自带 `data-addr` + `data-symbol` ⇒ ⇒ **原样带上去**
        // ⇒ ⇒ 条件**另外**从「断点」卡那个条件框里取（裁决 O：两个入口
        // 共用同一套条件输入）。
        return {
            addr: el?.dataset?.addr,
            symbol: el?.dataset?.symbol ? String(el.dataset.symbol) : null,
            condition: readBreakpointCondition(el),
        };
    }
    if (action === "lookup-symbol") {
        // ⚠️ **符号框只收符号名**（裁决 L）⇒ ⇒ 不做「一个框自动分流」。
        const box = el?.closest?.("[data-symbol-input]");
        const name = String(box?.querySelector?.("[data-bp-symbol-input]")?.value ?? "");
        return { name };
    }
    if (action === "dump-memory") {
        const box = el?.closest?.("[data-dump-input]");
        if (!box) return null;
        const parsed = parseAddrInput(box.querySelector("[data-dump-addr-input]")?.value ?? "");
        if (!parsed.ok) {
            setAddrError(box, "[data-dump-error]", parsed.error);
            return { invalid: true };
        }
        // ⚠️ **前端不判长度上限**（统筹方裁定 · 2026-10-30）⇒ ⇒ **一律发
        //   命令** ⇒ ⇒ 让**内核**拒（`E_DBG-03`）⇒ ⇒ 前端把**内核原文**
        //   显示在 `[data-dump-error]` 上。
        //
        // ⚠️ **为什么**：内核是唯一真相 ⇒ ⇒ 前端不写第二份校验器 ⇒ ⇒ 前端
        //   预校验会让「内核那条上限路径」**永远走不到** ⇒ ⇒ 真机门禁那条
        //   断言（越界 ⇒ ⇒ `E_DBG-03` 原文上屏）就**永远红不了** ⇒ ⇒ 那是
        //   **假绿**。
        //
        // ⚠️ 这里**只做「类型归一」**（线上字段是 `u32` ⇒ ⇒ 非数与负数一律
        //   归成 `0`）⇒ ⇒ 那是**域**的归一，不是**上限**的校验 ⇒ ⇒ `0` 照样
        //   由内核拒（`len == 0` ⇒ `E_DBG-03`）⇒ ⇒ 错误路径**可达**。
        const rawLen = String(box.querySelector("[data-dump-len-input]")?.value ?? "").trim();
        const parsedLen = Number(rawLen);
        const len = Number.isFinite(parsedLen) ? Math.max(0, Math.trunc(parsedLen)) : 0;
        setAddrError(box, "[data-dump-error]", "");
        return { addr: parsed.value, len };
    }
    if (action !== "set-breakpoint" && action !== "read-mem") return null;
    // --- 来源 1：按钮自带地址（旧按钮的路，一字不变）----------------------
    if (el?.dataset?.addr) return { addr: el.dataset.addr };
    // --- 来源 2：同屏的地址输入框 ----------------------------------------
    const box = el?.closest?.("[data-addr-input]");
    if (!box) return null;
    if (action === "set-breakpoint") {
        const parsed = parseAddrInput(box.querySelector("[data-bp-addr-input]")?.value ?? "");
        if (!parsed.ok) {
            setAddrError(box, "[data-bp-addr-error]", parsed.error);
            return { invalid: true };
        }
        setAddrError(box, "[data-bp-addr-error]", "");
        // ⚠️ **条件框对地址断点也通用**（裁决 O）⇒ ⇒ 地址入口与符号入口
        // 共用「断点」卡里那**一个**条件框。
        return { addr: parsed.value, condition: readBreakpointCondition(el) };
    }
    const parsed = parseAddrInput(box.querySelector("[data-mem-addr-input]")?.value ?? "");
    if (!parsed.ok) {
        setAddrError(box, "[data-mem-addr-error]", parsed.error);
        return { invalid: true };
    }
    setAddrError(box, "[data-mem-addr-error]", "");
    const rawSize = box.querySelector("[data-mem-size]")?.value ?? WRITE_SIZE.Word;
    const size = Object.prototype.hasOwnProperty.call(WRITE_SIZE, rawSize)
        ? rawSize
        : WRITE_SIZE.Word;
    return { addr: parsed.value, size };
}

/**
 * 读**「断点」卡里那个条件框**的内容（契约 v9 · 裁决 O）。
 *
 * ⚠️ **只认「断点」卡里的那一个条件框** ⇒ ⇒ 地址入口与符号入口共用它
 *   ⇒ ⇒ 不是两个条件框、不是两个真相。
 *
 * ⚠️ **反汇编行那个「加断点」按钮不在断点卡里** ⇒ ⇒ 它拿不到条件 ⇒ ⇒
 *   发出去的是**无条件**断点（那条入口是「点一下就下断点」的快捷方式，
 *   不是规格说的那两个入口之一）。
 *
 * ⚠️ **空串 ⇒ `null`**（C-9：不是空串）⇒ ⇒ 「无条件」在线上是 `null`。
 *   ⚠️ **纯空格的串照发** ⇒ ⇒ 前端**不解析**条件表达式（那是内核的判据）
 *   ⇒ ⇒ 内核回 `E_DBG-02` ⇒ ⇒ 前端显示它的原文。
 *
 * @param {Element|null} el — 被点的那个按钮
 * @returns {string|null}
 */
function readBreakpointCondition(el) {
    const card = el?.closest?.(".breakpoint-card");
    const input = card?.querySelector?.("[data-bp-cond-input]");
    const raw = String(input?.value ?? "");
    return raw === "" ? null : raw;
}

/**
 * 把**输入事件**接上（**事件委托** · 裁决 G / F11）。
 *
 * ⚠️ **监听器绑在容器上，不绑在输入框上** —— 输入框元素每次重渲染都会
 * 被重建 ⇒ ⇒ 绑在它身上的监听器**会丢**（点第二次就没反应了）。
 *
 * ⚠️ **写草稿不重渲染** ⇒ ⇒ 输入框不被重建 ⇒ ⇒ 焦点不丢。
 *
 * ⚠️ **重新输入 ⇒ ⇒ 旧提示立刻作废** ⇒ ⇒ 提示不会一直挂着吓人。
 *
 * @param {HTMLElement} el — 视图的挂载容器
 */
function mountAddrInputListeners(el) {
    el.addEventListener("input", (ev) => {
        const target = ev.target;
        if (target?.closest?.("[data-bp-addr-input]")) {
            setDebuggerDraft({ bpAddr: target.value ?? "" });
            setAddrError(target.closest("[data-addr-input]"), "[data-bp-addr-error]", "");
            return;
        }
        // ⚠️ **条件框的草稿也住视图状态**（契约 v9 · D-119）⇒ ⇒ 渲染重建
        // DOM 时草稿不丢。
        if (target?.closest?.("[data-bp-cond-input]")) {
            setDebuggerDraft({ bpCond: target.value ?? "" });
            setAddrError(target.closest("[data-cond-input]"), "[data-bp-cond-error]", "");
            return;
        }
        // ⚠️ **符号框同理** ⇒ ⇒ 一次输入 = 一次查询（裁决 M）⇒ ⇒ 草稿住
        // 状态、查询由**显式动作**触发。
        if (target?.closest?.("[data-bp-symbol-input]")) {
            setDebuggerDraft({ bpSymbol: target.value ?? "" });
            setAddrError(target.closest("[data-symbol-input]"), "[data-bp-symbol-error]", "");
            return;
        }
        if (target?.closest?.("[data-dump-addr-input]")) {
            setDebuggerDraft({ dumpAddr: target.value ?? "" });
            setAddrError(target.closest("[data-dump-input]"), "[data-dump-error]", "");
            return;
        }
        if (target?.closest?.("[data-dump-len-input]")) {
            setDebuggerDraft({ dumpLen: target.value ?? "" });
            setAddrError(target.closest("[data-dump-input]"), "[data-dump-error]", "");
            return;
        }
        if (target?.closest?.("[data-mem-addr-input]")) {
            setDebuggerDraft({ memAddr: target.value ?? "" });
            setAddrError(target.closest("[data-addr-input]"), "[data-mem-addr-error]", "");
            return;
        }
        // ⚠️ **步数框的草稿也住视图状态**（v0.63 批）⇒ ⇒ 与其它输入框同一
        //   规矩 ⇒ ⇒ 重渲染不丢 ⇒ ⇒ 用户填的步数还在。
        if (target?.closest?.("[data-step-count-input]")) {
            setDebuggerDraft({ stepCount: target.value ?? "" });
        }
    });
    el.addEventListener("change", (ev) => {
        const target = ev.target?.closest?.("[data-mem-size]");
        if (!target) return;
        setDebuggerDraft({ memSize: target.value ?? WRITE_SIZE.Word });
    });
    // ⚠️ **回车 = 「查找」**（契约 v9 · 裁决 M：查询按**显式动作**触发 ⇒ ⇒
    //   「查找」按钮 / 回车 ⇒ ⇒ 一次输入 = 一次查询 ⇒ ⇒ 没有「边打字边查」
    //   的乱序问题）。
    el.addEventListener("keydown", (ev) => {
        if (ev?.key !== "Enter") return;
        const target = ev.target?.closest?.("[data-bp-symbol-input]");
        if (!target) return;
        ev.preventDefault?.();
        const params = resolveAddrParams("lookup-symbol", target) ?? {};
        emitCommand("lookup-symbol", "debug", { name: params.name ?? "" });
    });
}

// ---------------------------------------------------------------------------
// debugView — 反汇编面板 (M-11) + 断点 + trace + 寄存器窗口 / 内存窗口
//
// ⚠️ **「寄存器 / 内存」不再是独立视图**（v0.65 批）⇒ ⇒ 它原本那张卡
//   **拆成两张**：**寄存器窗口**（寄存器格 + PC）与**内存窗口**（读内存 +
//   读这一段 + 内存单元）⇒ ⇒ 同一个动作**只出现在一处**。
// ---------------------------------------------------------------------------

/**
 * trace 开关按钮上的**标签**（**状态 + 点了会发生什么**）。
 *
 * ⚠️ **两者都写**：只写状态 ⇒ 用户不知道点下去会发生什么（那正是「点了没
 *   反应」那种体验的来源）；只写动作 ⇒ 用户不知道现在是什么状态 ⇒ ⇒
 *   **两个都写在按钮上** ⇒ ⇒ 用户不用猜。
 *
 * @param {{state: string}} sw — {@link traceSwitchState} 的结果
 * @returns {string}
 */
function traceToggleLabel(sw) {
    if (sw.state === TRACE_SWITCH_ON) return "trace 记录：已开启（点击关闭）";
    if (sw.state === TRACE_SWITCH_OFF) return "trace 记录：已关闭（点击开启）";
    return "trace 记录：状态未查询（点击开启）";
}

/**
 * trace 开关的**状态行**（**常驻** —— 开与关都在）。
 *
 * ⚠️ **为什么常驻**：此前只有「开着」才有那行降速提示 ⇒ ⇒ 「关着」与
 *   「从没点过」在界面上**一模一样** ⇒ ⇒ 用户没法判断自己到底开没开 ⇒ ⇒
 *   「点了没反应」。⇒ ⇒ 三种状态（未查询 / 已开启 / 已关闭）**都**显示。
 *
 * ⚠️ **条数只能当「快照」说**：`TraceState.len` 是内核**回复那一刻**的环大小
 *   ⇒ ⇒ 之后仿真继续跑、环继续涨 ⇒ ⇒ 界面上的那个数**不会自己更新**⇒ ⇒
 *   说成「已记录 N 条」就是**误导**（用户会以为没在录）⇒ ⇒ 如实说「内核上次
 *   回报时环里 N 条」⇒ ⇒ **实时**总数在「导出执行轨迹」的进度行里（那里是活值）。
 *
 * @param {{state: string, on: boolean, len: number, dropped: number}} sw
 * @returns {string}
 */
function renderTraceStatus(sw) {
    const len = String(sw.len);
    const dropped = String(sw.dropped);
    if (sw.state === TRACE_SWITCH_ON) {
        return `<span class="trace-state-main">trace 记录：<strong>已开启</strong> · 仿真会明显降速（每条指令与访存都要记录）。</span>`
            + `<span class="muted trace-state-hint">点上面的开关可<strong>关闭</strong>（关闭后内核零开销）。内核上次回报时环里 <strong>${escapeHtml(len)}</strong> 条 · 溢出丢弃 <strong>${escapeHtml(dropped)}</strong> 条 ⇒ 条数随仿真增长，「导出执行轨迹」里看最新总数。</span>`;
    }
    if (sw.state === TRACE_SWITCH_OFF) {
        return `<span class="trace-state-main">trace 记录：<strong>已关闭</strong> · 内核不记录任何指令与访存（零开销）。</span>`
            + `<span class="muted trace-state-hint">点上面的开关可<strong>开启</strong>（每条指令与访存都要记录 ⇒ 仿真会明显降速）。内核上次回报时环里 <strong>${escapeHtml(len)}</strong> 条 · 溢出丢弃 <strong>${escapeHtml(dropped)}</strong> 条 ⇒ 这 ${escapeHtml(len)} 条仍可导出。</span>`;
    }
    return `<span class="trace-state-main">trace 记录：<strong>状态未查询</strong></span>`
        + `<span class="muted trace-state-hint">点上面的开关：向内核确认当前状态，并<strong>开启</strong>记录（每条指令与访存都要记录 ⇒ 仿真会明显降速）。</span>`;
}

const debugView = register({
    id: "debug",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        const debug = state?.debug ?? {};
        // ⚠️ **运行中 ⇒ ⇒ 活值隐藏**（STAGE-38 · v0.53 批 · 裁决 D）
        // ⇒ ⇒ 寄存器 / PC 一律显示**固定符号 `-`** ⇒ ⇒ 界面
        // **不闪**（那些数字每秒要跳 5 次 · F6）⇒ ⇒ 停止后**自动**恢复。
        // ⚠️ **虚拟时间不在这里**（v0.65 批）⇒ ⇒ 它在**顶栏**⇒ ⇒ 这里
        //   不再显示第二份 ⇒ ⇒ 「同一个量只显示一处」这条规矩不破。
        const running = kernelIsRunning();
        // ⚠️ **草稿住共享命名空间**（STAGE-39 · v0.54 批 · 裁决 G）⇒ ⇒
        // 输入框里的内容**重渲染不丢** ⇒ ⇒ 焦点也不丢（不重建 ⇒ 不丢）。
        const draft = debuggerDraft();
        // The disassembly entries come from the kernel's `Disasm` event.
        const entries = Array.isArray(sim.disasm) ? sim.disasm : [];
        // The PC comes from the **existing** state source (M-10 pushes
        // `sim.pc` from `sim.registers` / `sim.state.delta`) — the panel
        // must not invent a second one.
        const pc = Number(sim.pc ?? 0) >>> 0;
        // ⚠️ **「跟随 PC」是用户意图**（STAGE-45 · v0.69 批 · 台账 D-176）⇒ ⇒
        //   它住**视图状态** `debug.followPc`（**默认关**）⇒ ⇒ 视图层**只读**
        //   （分层不破）⇒ ⇒ app 层写、视图层渲染 ⇒ ⇒ 只有一个落点。
        const followPc = debug.followPc === true;
        // ⚠️ **状态行是现算的**（`front/lib/disasm-follow.js`）⇒ ⇒ 「这段是
        //   哪个 PC 时的」由**内核给的数据**回答（段起点 = `entries[0].addr`）
        //   ⇒ ⇒ 前端**不**记住「我请求了哪个地址」⇒ ⇒ **不会**与界面显示
        //   的那段对不上。
        const disasmStatus = disasmStatusText({ entries, pc, followPc });
        // ⚠️ **内存单元是 M-10 推的那份**（`sim.mem`）⇒ ⇒ 与读内存那条路
        //   落的是**同一个缓存** ⇒ ⇒ 不是第二份。
        const cells = Object.entries(sim.mem ?? {});
        // ⚠️ **转储结果住在 M-03 视图状态**（契约 v9 §5.2）⇒ ⇒ `debug.dump`
        //   ⇒ ⇒ **不**另存一份 ⇒ ⇒ 这里是**唯一**一个显示它的地方。
        const dump = debug.dump ?? null;
        const dumpError = String(debug.dumpError ?? "");
        const exportState = debug.export ?? null;
        // ⚠️ **开关状态是推导量**（`front/lib/trace-switch.js`）⇒ ⇒ 视图层
        // （按钮 + 状态行）与 app 层（发什么命令）**引同一处** ⇒ ⇒ 两者
        // **不可能**对不上。
        const traceSwitch = traceSwitchState(debug.trace ?? null);
        const error = debug.error ?? null;
        // ⚠️ **断点列表只有一个落点**（STAGE-35 · v0.50 批 · 裁决 K）⇒ ⇒
        // 这里**只读**视图状态 ⇒ ⇒ 视图层**不持有**第二份列表。
        const breakpoints = Array.isArray(debug.breakpoints) ? debug.breakpoints : [];
        // ⚠️ **符号名标签**（契约 v9 §5.3）⇒ ⇒ 只是一个「地址 → 名字」的
        //   **附注** ⇒ ⇒ 「有哪些断点」这件事**仍然只有** `breakpoints`
        //   这**一个**落点（既有判据 A-7 不许被改软）。
        const bpSymbols = debug.bpSymbols && typeof debug.bpSymbols === "object"
            ? debug.bpSymbols
            : {};
        // ⚠️ **符号查询的答复**（一次查询的答复 ⇒ ⇒ 推导量 ⇒ ⇒ 只住视图
        //   状态、**不持久化**）。
        const symbolResult = debug.symbol ?? null;
        // ⚠️ **内核原文**（不吞、不改写）⇒ ⇒ 这两位显示的是内核给的
        //   **原文消息**（不是前端改写过的措辞）。
        const condError = String(debug.condError ?? "");
        const symbolError = String(debug.symbolError ?? "");
        // ⚠️ **「停下」也只有一个落点**（裁决 L）⇒ ⇒ 这里读 `debug.halt`。
        const halt = debug.halt ?? null;
        // ⚠️ **「读回」也只有一个落点**（v0.67 批）⇒ ⇒ 基准住
        //   `debug.readback.baseline` ⇒ ⇒ app 层在**运行 / 单步开始前**存
        //   ⇒ ⇒ 视图层**只读**（分层不破）⇒ ⇒ 「什么时候会高亮」有了一个
        //   **能写在界面上**的答案。
        const readback = debug.readback ?? null;
        const regBaseline = readback?.baseline ?? null;
        // ⚠️ **变了几个 = 现算**⇒ ⇒ 不另存一份 ⇒ ⇒ 数字与格子里那些高亮
        //   **必然**来自同一组数⇒ ⇒ 界面不会自相矛盾。
        const regChanged = diffRegisters(regBaseline, sim.gpr ?? []).length;
        // ⚠️ **调试视图的工具条整块撤掉**（v0.63 批 · 用户拍板）⇒ ⇒ 那十一个
        //   按钮**每一个**都有别的落点 ⇒ ⇒ 同一个动作在两处出现 ⇒ ⇒ 用户得
        //   猜「哪个才是真的」⇒ ⇒ 落点分别是：「单步 / 继续 / 暂停 / 复位」在
        //   **顶栏**（全局运行条）·「读寄存器」在**寄存器窗口**（v0.65 批：
        //   「寄存器 / 内存视图」这个独立视图没了 ⇒ ⇒ 它并进了调试视图）
        //   ·「反汇编」在**反汇编卡片** ·「导出波形」在**波形视图**
        //   ·「trace 记录」与「下载轨迹」在**trace 卡片** ⇒ ⇒ 这里**不再有**
        //   工具条。
        return `
            ${halt?.reason === "Breakpoint"
                // ⚠️ **这句话只在内核这么说的时候出现**（裁决 M）⇒ ⇒ 前端
                // **不自己判断**「跑没跑到那个地址」⇒ ⇒ 判据 = 内核的
                // `Halted.reason`。
                ? `<p class="bp-hit" data-bp-hit>命中断点 @ <code>0x${(Number(halt.pc ?? 0) >>> 0).toString(16).padStart(8, "0")}</code></p>`
                : ""}
            ${error
                ? `<p class="debug-error" data-debug-error>${escapeHtml(String(error.code))}：${escapeHtml(String(error.message))}</p>`
                : ""}
            <div class="debug-grid">
                <section class="card disasm">
                    <div class="row">
                        <h3>反汇编 (M-11)</h3>
                        <span class="tag">在「断点」列点一下即可下断点</span>
                        <button type="button" data-action="disasm" data-addr="${pc}" data-count="16">反汇编</button>
                    </div>
                    <div class="card-actions" data-disasm-follow-box>
                        <label class="follow-pc">
                            <input type="checkbox" data-disasm-follow${followPc ? " checked" : ""}>
                            <span data-disasm-follow-label>${escapeHtml(followPcLabel({ followPc }))}</span>
                        </label>
                    </div>
                    <p class="muted" data-disasm-status data-disasm-stale="${disasmStatus.stale ? "1" : "0"}">${escapeHtml(disasmStatus.text)}</p>
                    <div class="card-actions" data-step-count-box>
                        <label class="step-count">
                            步数
                            <input type="number" min="1" step="1" value="${escapeHtml(draft.stepCount)}" data-step-count-input>
                        </label>
                        <button type="button" data-action="step-n">按步数单步</button>
                        <span class="muted">从当前 PC 起走这么多条指令（顶栏「单步」走一条）。</span>
                    </div>
                    <p class="muted">助记符是 <strong>ISA 助记符</strong>：压缩指令保留 <code>c.</code> 前缀，不做伪指令别名改写（裁决 C）。</p>
                    <table class="disasm-table">
                        <thead><tr><th>地址</th><th>机器码</th><th>助记符</th><th>宽度</th><th>源码</th><th>断点</th></tr></thead>
                        <tbody>
                            ${entries.map((entry) => renderDisasmRow(entry, pc, breakpoints)).join("")
                                || `<tr><td colspan="6" class="muted">尚无反汇编 — 点击「反汇编」向内核请求一段（M-11）。</td></tr>`}
                        </tbody>
                    </table>
                </section>
                <section class="card breakpoint-card">
                    <div class="row"><h3>断点</h3><span class="tag">${breakpoints.length} 个</span></div>
                    ${renderBreakpointAddrRow({ draft: draft.bpAddr })}
                    ${renderBreakpointCondRow({ draft: draft.bpCond, error: condError })}
                    ${renderSymbolLookupRow({ draft: draft.bpSymbol, error: symbolError, result: symbolResult })}
                    ${breakpoints.length === 0
                        ? `<p class="muted">尚无断点 — 在左边反汇编表的「断点」列点「加断点」，或在上面直接填地址。</p>`
                        : `<div class="bp-list" data-bp-list>${breakpoints.map((raw) => {
                            const addr = Number(raw ?? 0) >>> 0;
                            const addrText = `0x${addr.toString(16).padStart(8, "0")}`;
                            // ⚠️ **符号名标签另起一个 `.note`**（形状钉死）⇒ ⇒
                            // 既有那行「断点 id = 地址」**保持不变** ⇒ ⇒ 两个
                            // `.note` 并列。
                            const symbol = bpSymbols[String(addr)] ?? null;
                            return `
                                <div class="bp-row" data-bp-row data-bp-addr="${addrText}">
                                    <code class="addr">${addrText}</code>
                                    ${symbol ? `<span class="note">${escapeHtml(String(symbol))}</span>` : ""}
                                    <span class="note">断点 id = 地址</span>
                                    <button type="button" data-action="clear-breakpoint" data-addr="${addrText}">撤掉</button>
                                </div>`;
                        }).join("")}</div>`}
                    <p class="muted">这里列的是<strong>本会话由界面请求过的断点</strong>（界面回显）—— 内核没有「列出断点」的事件，所以它<strong>不是</strong>内核断点集的镜像；用脚本口设的断点在这里看不到。</p>
                </section>
                <section class="card trace-export">
                    <div class="row"><h3>trace 记录 / 导出 (M-11 / M-19)</h3></div>
                    <div class="card-actions">
                        <button type="button" data-action="trace-toggle" data-trace-switch="${traceSwitch.state}" aria-pressed="${traceSwitch.state === TRACE_SWITCH_ON ? "true" : "false"}">${escapeHtml(traceToggleLabel(traceSwitch))}</button>
                        <button type="button" data-action="download-trace-local" data-trace-download-button title="把**整份**导出翻完并落盘（不是第一块）">${escapeHtml(exportDownloadLabel(exportState?.source, exportState?.format))}</button>
                    </div>
                    <p class="trace-state" data-trace-state data-trace-switch="${traceSwitch.state}"${traceSwitch.state === TRACE_SWITCH_ON ? " data-trace-notice" : ""}>${renderTraceStatus(traceSwitch)}</p>
                    ${exportState
                        ? `<p class="muted" data-trace-progress>${escapeHtml(String(exportState.source))} / ${escapeHtml(String(exportState.format))} · 条目 ${Number(exportState.offset)} → ${Number(exportState.next_offset)} / ${Number(exportState.total)}</p>`
                        : `<p class="muted">尚未导出 — 选一个数据源与格式发起导出。</p>`}
                    ${exportState?.overflow_marked === true
                        ? `<p class="trace-overflow" data-trace-overflow>⚠️ 导出窗口<strong>不是完整历史</strong> —— 源环溢出过、丢过最旧条目（本次导出从第 ${Number(exportState.offset)} 条开始，更早的内容已不在环里）。</p>`
                        : ""}
                    <pre class="trace-chunk" data-trace-chunk>${escapeHtml(String(exportState?.chunk ?? ""))}</pre>
                </section>
                <section class="card regmem-registers" data-window="registers">
                    <div class="row">
                        <h3>寄存器窗口</h3>
                        <span class="tag">变化高亮 = 与本次运行 / 单步开始前相比</span>
                        <button type="button" data-action="read-regs">读寄存器</button>
                    </div>
                    <div class="reg-grid">${renderRegGrid(sim.gpr ?? Array(32).fill(0), regBaseline, running)}</div>
                    <p class="muted" data-readback-state>${renderReadbackState(readback, regChanged)}</p>
                    <h4>PC</h4>
                    <code class="pc">${liveValue(running, `0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}`)}</code>
                </section>
                <section class="card regmem-memory" data-window="memory">
                    <div class="row"><h3>内存窗口</h3><span class="tag">读回的字列在下面</span></div>
                    <p class="muted" data-memory-readback-note>程序<strong>停下</strong>时（断点 / 观察点 / 陷阱 / 暂停 / 单步走完）这里会<strong>自动重读</strong>下面这些单元。</p>
                    ${renderMemoryReadRow({ draft: draft.memAddr, size: draft.memSize })}
                    ${renderMemoryDumpRow({ addrDraft: draft.dumpAddr, lenDraft: draft.dumpLen, error: dumpError })}
                    ${renderMemoryDump(dump)}
                    <h4>内存单元</h4>
                    ${cells.length === 0
                        ? `<p class="muted">尚无读回的内存单元 — 在上面填一个地址，点「读内存」。</p>`
                        : `<table class="map-table" data-mem-cells>
                            <thead><tr><th>地址</th><th>值</th></tr></thead>
                            <tbody>${cells.map(([addr, value]) => `
                                <tr><td><code>${escapeHtml(addr)}</code></td>
                                <td><code>${liveValue(running, `0x${(value >>> 0).toString(16).padStart(8, "0")}`)}</code></td></tr>
                            `).join("")}</tbody>
                        </table>`}
                </section>
            </div>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        // ⚠️ **按步数单步**（v0.63 批 · 用户拍板「在反汇编窗口中增加一个输入框
        //   和一个按钮，用于支持用户指定执行多少步」）⇒ ⇒ 步数**从界面来**
        //   ⇒ ⇒ 与地址那条路**同一手法**（参数随信号走 ⇒ ⇒ 视图层不碰服务层）。
        //
        // ⚠️ **归一在 app 层**（`normalizeStepCount`）⇒ ⇒ 视图层**不写第二份
        //   校验器** ⇒ ⇒ 输入框里是什么就发什么。
        if (action === "step-n") {
            const box = el?.closest?.("[data-step-count-box]");
            const raw = String(box?.querySelector?.("[data-step-count-input]")?.value ?? "");
            return emitCommand("step-n", "debug", { steps: raw });
        }
        // ⚠️ **任意地址**（STAGE-39 · v0.54 批 · 裁决 B）⇒ ⇒ 地址有两个
        // 来源：按钮**自带** `data-addr`（反汇编行的旧按钮 ⇒ ⇒ **一字不变**）
        // 或**同屏**的地址输入框 ⇒ ⇒ 用户给什么地址就下在什么地址。
        const addrParams = resolveAddrParams(action, el);
        // ⚠️ **解析不出来 ⇒ ⇒ 不发命令**（裁决 F）⇒ ⇒ 「点了没反应」是最差
        // 的 UX ⇒ ⇒ 输入框旁边**就地**有提示 ⇒ ⇒ 这里**直接返回**。
        if (addrParams?.invalid === true) return undefined;
        // ⚠️ **符号查询**（契约 v9）⇒ ⇒ 参数只有「名字」⇒ ⇒ **不**带地址
        //   ⇒ ⇒ 一次输入 = 一次查询（裁决 M）。
        if (action === "lookup-symbol") {
            return emitCommand("lookup-symbol", "debug", { name: addrParams?.name ?? "" });
        }
        // The command's parameters travel **with the signal** — the
        // bootstrap's listener runs on `window` and cannot see the button.
        return emitCommand(action, "debug", {
            ...(addrParams ?? {}),
            addr: addrParams?.addr ?? el.dataset.addr,
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
        mountAddrInputListeners(el);
        // ⚠️ **「跟随 PC」复选框走 `change`**（不是 `click`）⇒ ⇒ `click` 事件
        //   触发时 `checked` **还没变** ⇒ ⇒ 读它会拿到**旧值** ⇒ ⇒ 必须用
        //   `change`。
        //
        // ⚠️ **`mountFor` 只会被调一次**（`body.dataset.mounted` 守卫）⇒ ⇒
        //   这个监听器**不会**重复挂 ⇒ ⇒ 勾一次只发一条命令。
        el.addEventListener("change", (ev) => {
            const box = ev.target?.closest?.("[data-disasm-follow]");
            if (!box) return;
            const follow = box.checked === true;
            emitCommand("disasm-follow", "debug", { follow });
        });
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
 * @param {number[]} breakpoints — **本会话由界面请求过的**断点地址（只读）
 * @returns {string}
 */
function renderDisasmRow(entry, pc, breakpoints = []) {
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
    // ⚠️ **断点列**（STAGE-35 · v0.50 批）：按钮文案随「这个地址**是不是**
    //   已有断点」变 ⇒ ⇒ **按钮自己**就说清了当前状态 ⇒ ⇒ 真机门禁
    //   **不用**去猜「有没有断点」。
    const on = breakpoints.some((a) => (Number(a ?? 0) >>> 0) === addr);
    const bpButton = `
        <button type="button" class="bp-toggle${on ? " on" : ""}" data-bp-toggle="${addrText}"
            data-action="${on ? "clear-breakpoint" : "set-breakpoint"}" data-addr="${addrText}"
            title="${on ? "撤掉这个断点" : "在这个地址下断点"}">${on ? "断点 ●" : "加断点"}</button>`;
    return `
        <tr class="disasm-row${isPc ? " is-pc" : ""}" data-disasm-addr="${addrText}"${isPc ? ' data-pc="1"' : ""}>
            <td><code>${addrText}</code></td>
            <td><code>${codeText}</code></td>
            <td class="disasm-mnemonic">${escapeHtml(entry?.mnemonic ?? "")}</td>
            <td>${width}</td>
            <td class="disasm-source" data-disasm-file="${escapeHtml(file)}" data-disasm-line="${line === null ? "" : String(line)}">${escapeHtml(sourceText)}</td>
            <td class="disasm-bp">${bpButton}</td>
        </tr>
    `;
}

// ---------------------------------------------------------------------------
// 寄存器 / 内存 —— **共享片段**（v0.65 批）。
//
// ⚠️ **为什么不是两个视图**：此前「寄存器 / 内存」是一个**独立视图**，而
//   「调试视图」里又有一张**同名**的卡片 ⇒ ⇒ 同一件事在**两处**⇒ ⇒ 用户
//   得猜「哪个才是真的」⇒ ⇒ 那是 v0.63 批「调试视图去重」那一族问题的
//   最后一个残留 ⇒ ⇒ 现在**只有一个视图**（调试视图），它里面的
//   **寄存器窗口**与**内存窗口**是**两张卡**。
// ---------------------------------------------------------------------------

/**
 * 「**内核真的在执行**吗」（STAGE-38 · v0.53 批 · 裁决 A / M）。
 *
 * ⚠️ **只有一个落点**：视图状态 `debug.run`（app 层写）⇒ ⇒ 调试视图的
 * **寄存器窗口**与**内存窗口**读**同一份** ⇒ ⇒ 不是第二个真相。
 *
 * ⚠️ **两个窗口都走这一个函数** ⇒ ⇒ 「运行中」的口径**只有一处** ⇒ ⇒
 * 「运行中显示 `-`」这件事**不会**只落在一个窗口上。
 *
 * @returns {boolean}
 */
function kernelIsRunning() {
    return isKernelRunning(viewStore.getState("debug")?.debug?.run);
}

/**
 * 渲染**寄存器格**（`.reg-grid` 里的 32 个格子）。
 *
 * ⚠️ **稳定钩子**（裁决 A）：`.reg-grid > div` **仍是 32 个** ⇒ ⇒ 真机门禁
 * 与前端用例的取法**一字不改**。
 *
 * ⚠️ **变化高亮**（v0.67 批 · **口径变更**）：与**本次运行 / 单步开始前**的
 * GPR 快照比 ⇒ ⇒ 变大加 `.up` · 变小加 `.down` · 没变**不加类**。
 *
 * ⚠️ **口径为什么变**（v0.67 批）：原先比的是「**上一次读回**」⇒ ⇒ 那一次
 *   「读回」取自 `sim.gpr` —— 而 `sim.gpr` 是 M-10 **被内核持续推送改写**的
 *   **活值**⇒ ⇒ 基准与显示值**同源**⇒ ⇒ 高亮几乎永不出现，且时机取决于内核
 *   的推送节奏 ⇒ ⇒ **用户「不知道什么时候会高亮」**（用户原话）。
 *   ⇒ ⇒ 新口径 = 「**与本次运行 / 单步开始前相比**」⇒ ⇒ 捕获时机**唯一**。
 *
 * ⚠️ **没有基准** ⇒ ⇒ **一律不加类**（⛔ 不猜「那一定是 0」）
 * ⇒ ⇒ 第一次读回时**不**满屏绿红。
 *
 * ⚠️ **`running` = 运行中**（STAGE-38 · v0.53 批 · 裁决 D）⇒ ⇒ 运行中
 * 每格只显示**固定符号 `-`**（不是占位文案、不是冻结旧值）⇒ ⇒ 停止后
 * 调同一个函数就**自动**恢复最新值。
 *
 * ⚠️ **运行中不算变化高亮** ⇒ ⇒ 高亮说的是「这个值变了」⇒ ⇒ 值都没显示
 * ⇒ ⇒ 高亮没有意义（而且会让那一格看起来像「有值」）。
 *
 * ⚠️ **长度不齐也照样比**（缺的那位按 0 参与）⇒ ⇒ 与 `diffRegisters`
 * **同一个口径** ⇒ ⇒ 「变了几个」那个数与格子里那些高亮**必然**一致。
 *
 * @param {number[]} gpr
 * @param {number[]|null} prevGpr — 基准（**本次运行 / 单步开始前**那一份）
 * @param {boolean} [running]
 * @returns {string}
 */
export function renderRegGrid(gpr, prevGpr, running = false) {
    const values = Array.isArray(gpr) ? gpr : Array(32).fill(0);
    const prev = Array.isArray(prevGpr) ? prevGpr : null;
    return values.map((v, i) => {
        const cur = Number(v ?? 0) >>> 0;
        let cls = "";
        if (running !== true && prev && prev.length > 0) {
            const old = Number(prev[i] ?? 0) >>> 0;
            if (cur !== old) cls = cur > old ? "up" : "down";
        }
        const clsAttr = cls ? ` class="${cls}"` : "";
        const text = liveValue(running, cur.toString(16).padStart(8, "0"));
        return `<div><span class="muted">x${i}</span><code${clsAttr}>${text}</code></div>`;
    }).join("");
}

/**
 * 「**读回**」状态那一行（v0.67 批）。
 *
 * ⚠️ **回答的是用户真正会问的那个问题**：「**什么时候会高亮**？」⇒ ⇒ 这一行
 *   **必须**把「高亮的基准是什么」与「上次什么时候读过」**都**说出来 ⇒ ⇒ 用户
 *   不必猜。
 *
 * ⚠️ **三种状态分得开**：① **还没跑过**（没有基准）⇒ ⇒ 说清「跑一次就有了」；
 *   ② **基准已记下、还没读回**（刚点完运行）⇒ ⇒ 说清「停下就会读回」；
 *   ③ **读回过了** ⇒ ⇒ 说清「**上次**是**什么时候**因为**什么**读回的」+ 变了几个。
 *
 * @param {object|null|undefined} readback — `debug.readback` 那一份
 * @param {number} changedCount — 这一趟变了几个（**现算**的）
 * @returns {string}
 */
export function renderReadbackState(readback, changedCount = 0) {
    const count = Number.isFinite(Number(changedCount)) ? Math.max(0, Number(changedCount)) : 0;
    if (!readback || !Array.isArray(readback.baseline)) {
        return "尚未运行 —— 点「运行」或「单步」时会记下基准，停下后自动读回并标出这一趟变了的寄存器。";
    }
    if (readback.reason === null || readback.reason === undefined) {
        return "基准已记下（本次运行 / 单步开始前）—— 停下之后会自动读回，并标出这一趟变了的寄存器。";
    }
    const label = READBACK_REASON_LABEL[readback.reason] ?? "读回";
    const how = readback.auto === true ? "自动读回" : "手动读回";
    const failed = readback.error ? " · ⚠️ 上次读回有失败" : "";
    return `上次${how}：${label} · 这一趟变了 <strong data-readback-changed-count>${count}</strong> 个寄存器${failed}`;
}

// ---------------------------------------------------------------------------
// waveformView — 探针通道列表 (D-46).
// ---------------------------------------------------------------------------

const waveformView = register({
    id: "waveform",
    channels: ["wave"],
    /**
     * **持久图表宿主**（STAGE-36 · 裁决 L）。
     *
     * ⚠️ **为什么要有它**：视图每次渲染都把 `body.innerHTML` 整个换掉 ⇒ ⇒
     * 视图里的 DOM **每次都被重建** ⇒ ⇒ echarts 实例挂在**容器元素**上 ⇒ ⇒
     * 容器没了实例也就没了 ⇒ ⇒ 每次都重建 ⇒ ⇒ **会闪**。
     *
     * ⇒ ⇒ 所以这里持有**一个** `<div>`（**不是**用户状态 ⇒ ⇒ 不住视图状态），
     * 每次渲染把它**搬**进新插槽 ⇒ ⇒ echarts 实例**始终绑在同一个元素上**
     * ⇒ ⇒ **不重建、不闪**。
     *
     * @type {HTMLElement|null}
     */
    _chartHost: null,
    /**
     * **已初始化的 echarts 实例**（同一个宿主上只有一个 ⇒ ⇒ 复用，不重建）。
     *
     * @type {object|null}
     */
    _chart: null,
    /**
     * **上次绘制时的宿主尺寸**（`"宽x高"`）⇒ ⇒ 变了就 `resize()`。
     *
     * @type {string|null}
     */
    _chartSize: null,
    render(state) {
        const wave = state?.wave ?? {};
        const channels = wave.channels ?? [];
        return `
            <section class="waveform">
                <div class="card">
                    <div class="row">
                        <h3>波形 (M-19)</h3>
                        <span class="tag">${channels.length} 个通道</span>
                        <button type="button" data-action="export-trace" data-source="Waveform" data-format="Vcd" data-offset="0" data-limit="0">导出波形 (VCD)</button>
                    </div>
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
                    <p class="muted">通道列表里的边沿数是**总数**；下面的图只画每个通道最近的一段。</p>
                    <p class="muted">导出的波形内容显示在「<strong>调试视图 → trace 记录 / 导出</strong>」卡片里（下载也在那儿）。</p>
                </div>
                <div class="card">
                    <div class="row"><h3>波形图</h3><span class="tag">M-19 · 边沿回放</span></div>
                    ${waveChartHtml({
                        status: wave.chart?.status ?? "idle",
                        channels,
                    })}
                </div>
            </section>
        `;
    },
    /**
     * 渲染**之后**做的事（STAGE-36 · 裁决 M）。
     *
     * ⚠️ **这一步只做两件事**：把**持久宿主**搬进插槽 + （波形库就绪时）
     * 初始化 / 更新图表 ⇒ ⇒ **不碰**任何别的视图的状态。
     *
     * @param {HTMLElement} body
     * @param {object} state
     */
    afterRender(body, state) {
        const slot = body?.querySelector?.("[data-wave-chart-slot]") ?? null;
        if (!slot) return; // ⚠️ 降级态**没有**插槽 ⇒ ⇒ 什么都不用做。

        // ⚠️ **搬宿主**：把上一次那个 `<div>` 搬进这次的插槽 ⇒ ⇒
        // ⇒ 容器还是同一个 ⇒ ⇒ echarts 实例**不用重建**。
        //
        // ⚠️⚠️ **插槽必须留下**（STAGE-36 施工期订正）—— `front/style.css` §16
        //   用 `.wave-chart-slot > *` 给宿主**绝对定位铺满** ⇒ ⇒ 把插槽删掉
        //   ⇒ ⇒ 宿主既没有 `min-height` 也没有定位参照 ⇒ ⇒ **0 高**
        //   ⇒ ⇒ 画出来是 0 高 ⇒ ⇒ **看不见** ⇒ ⇒ 正是那段 CSS 注释里说的
        //   「静默地什么都不画」⇒ ⇒ ⇒ 所以宿主**进插槽**、插槽**留着**。
        if (!waveformView._chartHost) {
            waveformView._chartHost = document.createElement("div");
            waveformView._chartHost.className = "wave-chart-host";
            waveformView._chartHost.setAttribute("data-wave-chart-host", "");
        }
        slot.appendChild(waveformView._chartHost);

        // ⚠️ **波形库还没好** ⇒ ⇒ 这一轮**不画** ⇒ ⇒ 等 app 层把状态写成
        // 「ok」并重新渲染 ⇒ ⇒ 那一轮才画 ⇒ ⇒ （降级时状态是「degraded」
        // ⇒ ⇒ 插槽压根不存在 ⇒ ⇒ 上面就 return 了）。
        const echarts = getECharts();
        if (!echarts) return;

        const host = waveformView._chartHost;
        const w = host.clientWidth;
        const h = host.clientHeight;
        // ⚠️⚠️ **量不到尺寸就不许 `init`**（STAGE-36 施工期订正）—— 面板没显示时
        //   `.hidden { display: none }` ⇒ ⇒ `clientWidth/Height` 是 **0**
        //   ⇒ ⇒ 此时 `echarts.init` 会量到 0 ⇒ ⇒ 画出来是 0 高 ⇒ ⇒
        //   ⇒ **而且它绑在容器上、之后不会自己重画** ⇒ ⇒ 等用户切到波形视图
        //   ⇒ ⇒ 图**永远是空的** ⇒ ⇒ ⇒ 所以量到 0 就**这一轮不画**
        //   ⇒ ⇒ ⇒ 等面板显示出来（下一次渲染会再来一轮）再画。
        //
        // ⚠️ **判据本身是纯函数**（`canDrawChart`）⇒ ⇒ Node 侧能单独验
        //   ⇒ ⇒ 不然这条护栏只能靠「真机恰好撞上那个时序」才有人管。
        if (!canDrawChart(w, h)) return;

        const channels = state?.wave?.channels ?? [];
        // ⚠️ **颜色从 CSS 令牌来**（`resolveWaveTheme`）⇒ ⇒ echarts 画在
        //   canvas 上 ⇒ ⇒ canvas 里的文字**不受 CSS 管** ⇒ ⇒ 默认的深灰字压
        //   深底 ⇒ ⇒ 看不见 ⇒ ⇒ 必须**显式**把令牌的颜色喂给 echarts。
        const theme = resolveWaveTheme(
            typeof document !== "undefined" ? document.documentElement : null,
        );
        const option = buildWaveOption(channels, theme);
        if (!waveformView._chart) {
            waveformView._chart = echarts.init(host);
        } else if (waveformView._chartSize !== `${w}x${h}`) {
            // ⚠️ **尺寸变了要 resize** ⇒ ⇒ 否则 echarts 一直按旧尺寸画
            // ⇒ ⇒ 窗口拉宽 / 侧栏收起 ⇒ ⇒ 图画不满。
            waveformView._chart.resize();
        }
        waveformView._chartSize = `${w}x${h}`;
        waveformView._chart.setOption(option, true);

        // ⚠️ **自报**的两个属性（真机门禁读它们）⇒ ⇒ 「界面上画了几个点」是
        // **界面自己说的** ⇒ ⇒ 真机判据还要拿它跟**视图状态里的样本数**比对
        // ⇒ ⇒ ⇒ 「界面说的」与「数据里的」**必须一致** ⇒ ⇒ 有判别力。
        const drawable = option.series.length;
        const points = option.series.reduce((n, s) => n + s.data.length, 0);
        host.setAttribute("data-wave-chart-points", String(points));
        host.setAttribute("data-wave-chart-series", String(drawable));
        host.setAttribute(
            "data-wave-chart-status",
            String(state?.wave?.chart?.status ?? "idle"),
        );
        // ⚠️ **自报**：真正喂给 echarts 的**文字色** ⇒ ⇒ 真机门禁拿它跟
        //   `--tx2` 的**计算值**比对 ⇒ ⇒ 两个**独立来源**（CSS 令牌 vs 界面
        //   自报）互相印证 ⇒ ⇒ 「令牌真的变成了图上的颜色」这件事**有人管**
        //   ⇒ ⇒ ⇒ 少了这一步，文字色那条订正就是**没人验**的。
        host.setAttribute(
            "data-wave-chart-text-color",
            String(option?.textStyle?.color ?? ""),
        );
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        // ⚠️ **导出波形**（v0.63 批 · 用户拍板「导出波形按钮可以放到波形视图」）
        //   ⇒ ⇒ 参数**随信号走**（与调试视图那条路同一手法）⇒ ⇒ 视图层**不碰**
        //   服务层 ⇒ ⇒ 导出结果落在**调试视图的 trace 卡片**里（那里是导出内容
        //   的**唯一**落点 ⇒ ⇒ 不是第二个真相）。
        if (action === "export-trace") {
            return emitCommand("export-trace", "waveform", {
                source: el.dataset.source,
                format: el.dataset.format,
                offset: el.dataset.offset,
                limit: el.dataset.limit,
            });
        }
        return undefined;
    },
    onDelta(_delta) {},
    mountFor(el) {
        // ⚠️ **监听器绑在容器上**（事件委托）⇒ ⇒ 每次渲染重建 DOM 也不会丢。
        el.addEventListener("click", (ev) => waveformView.onCommand(ev));
    },
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
        const line = Number(location?.line ?? 0);

        return `
            <section class="source-view">
                <div class="card">
                    <div class="row"><h3>源码 (M-11 · DWARF 行号)</h3><span class="tag">左列文件 · 右列代码</span></div>
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
                    ${renderSourceColumns({ tree, activeKey, currentLine: line })}
                    <p class="muted">源码<strong>不进内核</strong>（裁判 E）：文件由浏览器读成文本 ⇒ 存进视图状态 ⇒ <strong>不发</strong>任何命令。本视图里唯一离开前端的命令是「解析当前 PC 的源码行」。</p>
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

/**
 * console 字节流的**字节数**。
 *
 * ⚠️ **按字节数，不按字符数**（STAGE-35 · v0.50 批 · 裁决 G）：guest 可能吐出
 * 任意字节 ⇒ 字符数 ≠ 字节数 ⇒ ⇒ 计数必须是**字节数**。
 *
 * ⚠️ **字节数组**（契约 v4 的 `Console` 载荷）⇒ 直接取长度；**字符串**
 * （既有的 `termDelta` 旧路径）⇒ 按 UTF-8 编码后取长度。
 *
 * @param {unknown} raw — 字节数组或字符串
 * @returns {number}
 */
export function consoleByteCount(raw) {
    if (Array.isArray(raw)) return raw.length;
    if (typeof raw === "string") return new TextEncoder().encode(raw).length;
    return 0;
}

/**
 * 终端的**行结束符**四档（STAGE-40 · v0.55 · 裁决 G）。
 *
 * ⚠️ **行结束符是 guest 的属性，不是平台的属性** —— 实测：osz 的
 * `inner_shell_char_is_special` 逐条比对 `127 / 9 / 13 / 27 / 91`
 * ⇒ ⇒ **只认 CR（13）判行结束**，**不认 LF（10）**；而多数 POSIX shell
 * 认 LF（也认 CR）⇒ ⇒ 平台**不猜** ⇒ ⇒ 做成**用户可选**。
 *
 * ⚠️ **默认是 CR**：真实串口终端按回车发的就是 **CR**（0x0D）⇒ ⇒ 默认档
 * 与「真实终端」一致 ⇒ ⇒ 也与 osz 一致。
 *
 * ⚠️ **四档都是「追加的字节」**（`none` 是空数组）⇒ ⇒ 「不追加」这个
 * 选项**保留**了 v0.5x 批之前的行为（K-1 §7 的「只送原始字节」）。
 *
 * @type {Readonly<Record<string, number[]>>}
 */
export const TERMINAL_EOL = Object.freeze({
    cr: [13],
    lf: [10],
    crlf: [13, 10],
    none: [],
});

/** 行结束符的**默认档**（= CR）。 */
export const TERMINAL_EOL_DEFAULT = "cr";

/**
 * 行结束符按钮的**四档**（渲染与文案都用它 ⇒ ⇒ **一处定义**）。
 *
 * ⚠️ **不用 `<select>`**：`front/tests/dom-stub.mjs` 的 `<select>.value`
 * 读的是**元素自己的 `value` 属性**（不是选中项）⇒ ⇒ `<select>` 的
 * 「非默认项」在 Node 门禁里**测不到** ⇒ ⇒ 用**按钮组**（`data-eol`）⇒ ⇒
 * 两层都可测。
 *
 * @type {ReadonlyArray<{id: string, label: string, title: string}>}
 */
export const TERMINAL_EOL_OPTIONS = Object.freeze([
    Object.freeze({ id: "cr", label: "CR", title: "0x0D —— 真实串口终端按回车发的就是这个；osz 只认它" }),
    Object.freeze({ id: "lf", label: "LF", title: "0x0A —— 多数 POSIX shell 认它" }),
    Object.freeze({ id: "crlf", label: "CRLF", title: "0x0D 0x0A —— Windows 风格" }),
    Object.freeze({ id: "none", label: "不加", title: "只送原始字节（不追加行结束符）" }),
]);

/**
 * 从视图状态里读**当前的行结束符**（读不到 ⇒ 默认档）。
 *
 * ⚠️ **判据用 `hasOwnProperty` 而不是 `TERMINAL_EOL[x]`** —— 后者会把
 * `"constructor"` 之类当成合法取值。
 *
 * @param {object} [state]
 * @returns {string}
 */
function currentTerminalEol(state) {
    const raw = state?.term?.eol ?? viewStore.getState("terminal")?.term?.eol;
    return Object.prototype.hasOwnProperty.call(TERMINAL_EOL, raw) ? raw : TERMINAL_EOL_DEFAULT;
}

/**
 * 渲染行结束符的**按钮组**。
 *
 * ⚠️ **当前档带 `data-eol-selected="true"`** ⇒ ⇒ 判据可以按属性读
 * （不用读文案 ⇒ ⇒ 改文案不会让判据变红）。
 *
 * @param {string} eol
 * @returns {string}
 */
function renderTerminalEolButtons(eol) {
    return TERMINAL_EOL_OPTIONS.map((opt) => {
        const selected = opt.id === eol;
        return `<button type="button" class="terminal-eol-btn${selected ? " is-selected" : ""}" data-eol="${opt.id}"${selected ? ' data-eol-selected="true"' : ""} title="${escapeHtml(opt.title)}">${opt.label}</button>`;
    }).join("");
}

/**
 * 切换行结束符（**住在视图状态里** ⇒ ⇒ 重渲染不丢）。
 *
 * @param {unknown} eol
 * @returns {string} the selected id
 */
function setTerminalEol(eol) {
    const next = Object.prototype.hasOwnProperty.call(TERMINAL_EOL, eol)
        ? eol
        : TERMINAL_EOL_DEFAULT;
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), eol: next },
    }));
    publishTerminalState();
    return next;
}

// ---------------------------------------------------------------------------
// 终端屏幕（STAGE-41 · v0.56 · 裁决 D ~ F）
// ---------------------------------------------------------------------------

/**
 * 增量喂字节的**屏幕实例**（模块级单例）。
 *
 * ⚠️ **为什么要增量**：长会话里 console 事件可能来很多次 ⇒ ⇒ 每来一次就
 *   **从零重扫**整条历史是 O(n²) ⇒ ⇒ 这里只处理**新到的那些字节**。
 *   ⚠️ **它不是真相**：真相是 store 里的 `term.bytes` ⇒ ⇒ 本实例只是
 *   「快速路径」的缓存 ⇒ ⇒ 任何时候都能从真相**重建**（裁决 E）。
 */
let terminalScreen = createTerminalScreen();

/**
 * 增量实例**已经吃掉的**原始字节流（用来核对「真的是追加」）。
 *
 * @type {string}
 */
let terminalScreenRaw = "";

/**
 * 把终端屏幕**重置**（换工程 / 清会话时用）。
 *
 * ⚠️ **必须与 store 的 `term.bytes` 清空**成对发生 ⇒ ⇒ 否则增量实例还拿着
 *   上一段工程的屏幕 ⇒ ⇒ 显示错乱。
 */
export function resetTerminalScreen() {
    terminalScreen = createTerminalScreen();
    terminalScreenRaw = "";
}

/**
 * 终端**显示什么**。
 *
 * ⚠️ **默认是「屏幕」**（把字节流按终端语义解释过的结果）⇒ ⇒ 切到
 *   「原始字节」档才是字节流本身 ⇒ ⇒ 两条路都留着。
 *
 * @param {string} raw 原始字节流
 * @param {boolean} rawMode 是否「原始字节」档
 * @returns {string}
 */
function terminalDisplayText(raw, rawMode) {
    return rawMode ? String(raw ?? "") : renderTerminalScreen(raw);
}

/**
 * 现在是不是「原始字节」档（**读视图状态** ⇒ ⇒ 重渲染不丢）。
 *
 * @returns {boolean}
 */
function isRawMode() {
    return viewStore.getState("terminal")?.term?.raw === true;
}

/**
 * 屏幕文本（**增量**那条路 —— 判据 T-11 盯它与「从零重建」的一致性）。
 *
 * ⚠️ **只处理新到的字节** ⇒ ⇒ 长会话里不是 O(n²) ⇒ ⇒ 但**必须**核对
 *   「store 里的字节流确实是**追加**的」⇒ ⇒ 不是就**从零重建**（裁决 E）
 *   ⇒ ⇒ 增量实例与真相**不会**悄悄分叉。
 *
 * @param {string} nextRaw store 里的**全部**字节流
 * @returns {string}
 */
function terminalScreenText(nextRaw) {
    const raw = String(nextRaw ?? "");
    if (!raw.startsWith(terminalScreenRaw)) {
        // 不是追加 ⇒ ⇒ 从零重建（**真相是 store 的字节流**，不是这个缓存）。
        resetTerminalScreen();
    }
    terminalScreen.write(raw.slice(terminalScreenRaw.length));
    terminalScreenRaw = raw;
    return terminalScreen.text();
}

/**
 * 切「原始字节 / 终端屏幕」两档（**不发契约命令**）。
 *
 * @returns {boolean} 切完之后是不是「原始字节」档
 */
function toggleTerminalRaw() {
    const next = !(viewStore.getState("terminal")?.term?.raw === true);
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: { ...(prev.term ?? {}), raw: next },
    }));
    publishTerminalState();
    return next;
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
        // ⚠️ **行结束符的选择也住在视图状态里**（STAGE-40 · v0.55 · 裁决 I）
        // ⇒ ⇒ 与草稿同一个真相 ⇒ ⇒ 重渲染不丢。
        const eol = currentTerminalEol(state);
        // ⚠️ **原始字节档**（STAGE-41 · v0.56 · 裁决 F）：默认显示**屏幕**
        //   （仿真后的）⇒ ⇒ 切到「原始字节」才看未经解释的字节流 ⇒ ⇒
        //   「字节流才是真相」这条**没有丢**，只是换了个入口。
        const rawMode = state?.term?.raw === true;
        // ⚠️ **TX / RX 是两个不同的源**（STAGE-35 · v0.50 批 · 裁决 G）⇒ ⇒
        // **分别标注方向** ⇒ ⇒ 不许把两个数混成一个。
        const txBytes = Number(state?.term?.txBytes ?? 0);
        const rxBytes = Number(state?.term?.rxBytes ?? 0);
        return `
            <section class="terminal">
                <div class="card">
                    <div class="row"><h3>串口终端 (M-15 UART)</h3><span class="tag">收发字节分别计数</span></div>
                    <div class="term-scroll">
                        <pre class="terminal-out" data-terminal-out>${escapeHtml(terminalDisplayText(buf, rawMode))}</pre>
                    </div>
                    <div class="term-meta">
                        <span class="dir">TX（guest → 终端，内核 console 事件累计）</span><span class="n" data-term-tx>${txBytes}</span>
                        <span class="dir">RX（终端 → guest，前端发出累计）</span><span class="n" data-term-rx>${rxBytes}</span>
                    </div>
                    <div class="terminal-input-row">
                        <input type="text" class="terminal-input" data-terminal-input value="${escapeHtml(draft)}" placeholder="输入后按「发送」——字节会进 guest 的 UART0">
                        <div class="terminal-eol" role="group" aria-label="行结束符">${renderTerminalEolButtons(eol)}</div>
                        <button type="button" class="terminal-raw-btn${rawMode ? " is-selected" : ""}" data-action="console-raw-toggle" data-raw-selected="${rawMode ? "true" : "false"}" title="切换「原始字节」与「终端屏幕」两种显示">原始字节</button>
                        <button type="button" class="terminal-send" data-action="console-send">发送</button>
                    </div>
                    <p class="muted">输入框里的文本<strong>按 UTF-8 编码</strong>成字节，发给 guest 的 UART0（契约 v5 的 <code>WriteConsole</code>）；<strong>回显来自 guest</strong>，平台不做本地回显。<strong>发送时会追加一个行结束符</strong>（默认 <code>CR</code> = 0x0D —— 真实串口终端按回车发的就是它；osz 这类固件的 shell 也只认它）；选「不加」则只送原始字节。</p>
                </div>
            </section>
        `;
    },
    onCommand(ev) {
        const actionEl = ev.target?.closest?.("[data-action]");
        const action = actionEl?.dataset?.action;
        if (action === "console-send") return sendTerminalInput();
        // ⚠️ **原始字节档**（STAGE-41 · v0.56）：切显示口径 ⇒ ⇒ **不发**任何
        //   契约命令 ⇒ ⇒ 与板档案那套按钮同一个口径（界面上的显示开关
        //   一律不发命令）。
        if (action === "console-raw-toggle") return toggleTerminalRaw();
        // ⚠️ **行结束符按钮**（STAGE-40 · v0.55）：它**没有** `data-action`
        // ⇒ ⇒ 不与上面的分支抢同一个动作名 ⇒ ⇒ 两套按钮可以并存。
        const eolEl = ev.target?.closest?.("[data-eol]");
        if (eolEl) return setTerminalEol(eolEl.dataset.eol);
        return undefined;
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
        // ⚠️ **TX 计数**（STAGE-35 · v0.50 批 · 裁决 G）：这是**内核 console
        // 事件**累计的字节数 ⇒ ⇒ 与 RX **不是同一个源** ⇒ ⇒ 分别标注。
        const txDelta = consoleByteCount(delta.termDelta.bytes ?? "");
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
                txBytes: Number(prev.term?.txBytes ?? 0) + txDelta,
            },
        }));
        const out = document.querySelector("[data-terminal-out]");
        // ⚠️ **显示口径**（STAGE-41 · v0.56）：默认「终端屏幕」，切到
        //   「原始字节」档才显示未经解释的字节流。
        const rawMode = isRawMode();
        // ⚠️ **计数也要就地更新**：这条路径**不重渲染**（只写 `textContent`）
        // ⇒ ⇒ 不改计数的话 TX 会**一直停在旧值**。
        const txEl = document.querySelector("[data-term-tx]");
        if (txEl) {
            txEl.textContent = String(Number(viewStore.getState("terminal")?.term?.txBytes ?? 0));
        }
        if (!out) return;
        // `textContent` does not parse HTML — escaping here would print a
        // literal "&lt;" (FRONTEND_Review.md #4).
        //
        // ⚠️ **显示的是「屏幕」，不是字节流**（STAGE-41 · v0.56 · 裁决 A）：
        //   guest 的 shell 每敲一个键就把整行重画一遍 ⇒ ⇒ 字节流里同一个
        //   提示符出现 N 次 ⇒ ⇒ **原样显示**会把 N 次重画**堆成 N 行**
        //   （`\r` 被渲染成换行 + `ESC[2K` 的 `[2K` 变成字面量）⇒ ⇒ 用户
        //   看到的是乱码而不是那一行命令。
        out.textContent = rawMode ? next : terminalScreenText(next);
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
 * 把输入框里的文本**按 UTF-8 编码**成字节 ⇒ 追加**行结束符** ⇒ 发
 * `console-send` 信号 ⇒ 清空输入框。
 *
 * ⚠️ **字节，不是字符**（K-1）：guest 的 UART 收的是**字节**（契约 v5 的
 * `WriteConsole { bytes: Vec<u8> }`）⇒ 这里用 `TextEncoder` 做 UTF-8 编码
 * （`"A"` ⇒ `[65]`、`"你"` ⇒ `[228, 189, 160]`），**不**送字符串（内核会
 * 按 `E_PROTO-08` 拒掉）。
 *
 * ⚠️ **追加的行结束符**（STAGE-40 · v0.55 · 裁决 G）：默认为 **CR
 * （0x0D）**⇒ ⇒ 真实串口终端按回车发的就是它 ⇒ ⇒ osz 这类只认 CR 的
 * shell 才能把命令**执行**掉（只送原始字节 ⇒ ⇒ 命令永远停在缓冲区里
 * ⇒ ⇒ 用户看不到任何输出）。
 *
 * ⚠️ **RX 计数含追加的行结束符**（裁决 J）：RX 的口径是「**前端发出**的
 * 字节数」⇒ ⇒ 追加的 CR **确实发出去了** ⇒ ⇒ 计入。
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
    const payload = Array.from(new TextEncoder().encode(text));
    // ⚠️ **行结束符取自视图状态**（不是 DOM）⇒ ⇒ **一个真相**。
    const eol = currentTerminalEol();
    const bytes = payload.concat(TERMINAL_EOL[eol]);
    // ⚠️ **RX 计数**（STAGE-35 · v0.50 批 · 裁决 G）：这是**前端发出**的
    // 字节数（TX 是内核推过来的 ⇒ ⇒ **两个不同的源** ⇒ ⇒ 分别标注）。
    //
    // ⚠️ **计的是「发出」不是「内核确认收到」** —— `WriteConsole` 没有回执
    // 事件 ⇒ ⇒ 界面上就这么说，不假装知道内核收没收到。
    viewStore.setState("terminal", (prev) => ({
        ...prev,
        term: {
            ...(prev.term ?? {}),
            rxBytes: Number(prev.term?.rxBytes ?? 0) + bytes.length,
        },
    }));
    const requestId = emitCommand("console-send", "terminal", { bytes });
    setTerminalDraft("");
    publishTerminalState();
    return requestId;
}

// ---------------------------------------------------------------------------
// Render dispatcher — receives M-01 `riscv:render` CustomEvents.
// ---------------------------------------------------------------------------

/**
 * 上一次**写进 DOM 的 HTML**（STAGE-38 · v0.53 批 · 裁决 K）。
 *
 * ⚠️ **按 DOM 容器本身做键**（`WeakMap`）⇒ ⇒ 容器被换掉（视图被重新挂载）
 * ⇒ ⇒ 查不到旧值 ⇒ ⇒ **第一轮一定重建** ⇒ ⇒ 不会出现「容器是空的却
 * 因为「内容一样」而跳过渲染」这种**空面板**。
 *
 * @type {WeakMap<object, string>}
 */
const lastRenderedHtml = new WeakMap();

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
    // ⚠️ **同内容不重建**（STAGE-38 · v0.53 批 · 裁决 K）
    // ⇒ ⇒ 连续运行每一轮都 `publishViewState("debug")`（F7）⇒ ⇒ 而运行中
    // 活值**全被隐藏** ⇒ ⇒ 渲染结果**逐字相同**（F8）⇒ ⇒ 这一次**不必**
    // 把整个视图正文换掉 ⇒ ⇒ 主线程**不再**被占满 ⇒ ⇒ 界面**不闪**。
    //
    // ⚠️ **`afterRender` 照旧调用**（裁决 L）⇒ ⇒ 波形图那一张图**不受**
    // 去重影响 ⇒ ⇒ 图**照样**跟着数据更新。
    const unchanged = lastRenderedHtml.get(body) === html;
    if (!unchanged) {
        lastRenderedHtml.set(body, html);
        body.innerHTML = html;
    }
    // ⚠️ **可选**的渲染后钩子（STAGE-36 · 裁决 M）。
    //
    // ⚠️ **为什么需要它**：图表（echarts）需要一个**真实 DOM 容器**才能初始化
    // ⇒ ⇒ 而 `body.innerHTML = html` 之后容器才存在 ⇒ ⇒ 渲染**之前**拿不到。
    //
    // ⚠️ **为什么它是可选的**：只有**声明了**它的视图会被调用 ⇒ ⇒ 既有视图
    // **零改动**、零回归。
    //
    // ⚠️ **为什么用 try/catch**：钩子里做的事（初始化图表）失败**不许**把
    // 整个渲染搞崩 ⇒ ⇒ 视图已经画出来了 ⇒ ⇒ 失败只该让**那一张图**没有。
    if (typeof c.afterRender === "function") {
        try {
            c.afterRender(body, detail.state ?? {});
        } catch (err) {
            console.warn(`[M-02 ${detail.viewId}] afterRender error:`, err);
        }
    }
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
 * `riscv:seed` signal; the dev bridge in app.js turns that signal into
 * store state. The demo data itself lives in `front/dev/demo-state.js`.
 *
 * @param {string} [viewId]
 */
export function seedBus(viewId = "product") {
    window.dispatchEvent(new CustomEvent("riscv:seed", { detail: { viewId } }));
}
