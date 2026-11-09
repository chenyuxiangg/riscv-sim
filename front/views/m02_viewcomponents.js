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
import {
    describeMatch,
    matchSourceFile,
    renderSourceText,
} from "./components/source-view.js";
import { parseNumber } from "./components/memory-map.js";
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
function publishProductState() {
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
    if (publish) publishProductState();
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
                emitCommand("load-template", "product", {
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
            }
        });
        // 失焦 / 提交时重渲染一次：把草稿归一化显示，并刷新「未生效」标记。
        el.addEventListener("change", (ev) => {
            if (ev.target?.matches?.("[data-base-input],[data-size-input],[data-reg-field]")) {
                publishProductState();
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
// embeddedView — 落地视图（D-33 · STAGE-10 裁决 C）。
//
// ⚠️ **为什么是「补组件」而不是「从落地视图列表去掉」**：去掉 ⇒ 嵌入式角色
// **没有**落地视图 ⇒ tab 条上的「嵌入式」变成死标签（点了落到 fallback）
// ⇒ 得同时改 tab 条与 `ROLE_DEFAULT_VIEW` ⇒ 改动面更大、用户拿到的信息更少
// （STAGE-10 §1 裁决 C）。
//
// ⚠️ **它不是第二个寄存器表**：这里是**概览 + 入口** —— 连接状态、当前 PC、
// 虚拟时间，加上四个子视图的入口。寄存器明细仍然只在 `regmem` 视图里。
// ---------------------------------------------------------------------------

/**
 * 落地视图的**四个子视图入口**（裁决 C 的最小内容之一）。
 *
 * ⚠️ **这里只列入口**：某个入口当前**不可达**（能力位不满足）时，由 M-01 的
 * 兜底页处理（E_CTX-05）⇒ 前端不再另造一份「可达性」真相。
 *
 * @type {ReadonlyArray<{view: string, label: string}>}
 */
export const EMBEDDED_VIEW_ENTRIES = [
    { view: "debug", label: "调试视图" },
    { view: "regmem", label: "寄存器 / 内存" },
    { view: "waveform", label: "波形视图" },
    { view: "source", label: "源码视图" },
];

const embeddedView = register({
    id: "embedded",
    channels: ["sim"],
    render(state) {
        // PC / 虚拟时间**只**来自既有状态源（M-10 推进的 `sim.pc` 与
        // `timeVirt`）⇒ 落地视图**不造**第二个状态源（与调试视图同一条规矩）。
        const pc = Number(state?.sim?.pc ?? 0) >>> 0;
        const timeVirt = state?.timeVirt ?? 0;
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
                <div class="grid-2">
                    <div>
                        <h4>当前 PC</h4>
                        <code class="pc" data-embedded-pc>0x${pc.toString(16).padStart(8, "0")}</code>
                    </div>
                    <div>
                        <h4>虚拟时间</h4>
                        <code class="timeVirt" data-embedded-time>${escapeHtml(String(timeVirt))} ps</code>
                    </div>
                </div>
                <h4>子视图</h4>
                <ul class="embedded-entries">
                    ${EMBEDDED_VIEW_ENTRIES.map((entry) => `
                        <li>
                            <button type="button" data-action="go-view" data-embedded-entry="${escapeHtml(entry.view)}" data-view-target="${escapeHtml(entry.view)}">${escapeHtml(entry.label)}</button>
                        </li>`).join("")}
                </ul>
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
                        <thead><tr><th>地址</th><th>机器码</th><th>助记符</th><th>宽度</th><th>源码</th></tr></thead>
                        <tbody>
                            ${entries.map((entry) => renderDisasmRow(entry, pc)).join("")
                                || `<tr><td colspan="5" class="muted">尚无反汇编 — 点击「反汇编」向内核请求一段（M-11）。</td></tr>`}
                        </tbody>
                    </table>
                </section>
                <section class="trace-export">
                    <h3>trace 导出 (M-11 / M-19)</h3>
                    ${exportState
                        ? `<p class="muted" data-trace-progress>${escapeHtml(String(exportState.source))} / ${escapeHtml(String(exportState.format))} · 条目 ${Number(exportState.offset)} → ${Number(exportState.next_offset)} / ${Number(exportState.total)}</p>`
                        : `<p class="muted">尚未导出 — 选一个数据源与格式发起导出。</p>`}
                    ${exportState?.overflow_marked === true
                        ? `<p class="trace-overflow" data-trace-overflow>⚠️ 导出窗口<strong>不是完整历史</strong> —— 源环溢出过、丢过最旧条目（本次导出从第 ${Number(exportState.offset)} 条开始，更早的内容已不在环里）。</p>`
                        : ""}
                    <pre class="trace-chunk" data-trace-chunk>${escapeHtml(String(exportState?.chunk ?? ""))}</pre>
                    ${exportState
                        ? `<div class="trace-download-row">
                            <button type="button" data-action="download-trace" data-trace-download-button>下载 ${escapeHtml(exportFileName(exportState.source, exportState.format))}</button>
                            <span class="muted" data-trace-download-name>${escapeHtml(exportFileName(exportState.source, exportState.format))}</span>
                        </div>`
                        : ""}
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
        // ⚠️ **下载**（D-88）：纯浏览器动作 ⇒ **不发**契约命令。
        // 导出块取自**视图状态**（唯一真相）⇒ 与面板上显示的是同一块内容。
        if (action === "download-trace") {
            const exportState = viewStore.getState("debug")?.debug?.export ?? null;
            const result = downloadExportChunk(exportState);
            return result ? `cmd-download-trace-${result.filename}` : undefined;
        }
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
    const sourceText = formatSourceLocation(entry);
    // ⚠️ **机器可读**的两个 `data-*`：真机段要断言「`file` 是个真源码
    // 路径」「`line` 大于 0」⇒ 从渲染出来的**文本**里猜是不行的。
    const { file, line } = sourceLocationParts(entry);
    return `
        <tr class="disasm-row${isPc ? " is-pc" : ""}" data-disasm-addr="${addrText}"${isPc ? ' data-pc="1"' : ""}>
            <td><code>${addrText}</code></td>
            <td><code>${codeText}</code></td>
            <td class="disasm-mnemonic">${escapeHtml(entry?.mnemonic ?? "")}</td>
            <td>${width}</td>
            <td class="disasm-source" data-disasm-file="${escapeHtml(file)}" data-disasm-line="${line === null ? "" : String(line)}">${escapeHtml(sourceText)}</td>
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
        const text = activeKey ? (tree[activeKey] ?? "") : "";
        const line = Number(location?.line ?? 0);

        return `
            <section class="source-view">
                <h3>源码 (M-11 · DWARF 行号)</h3>
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
                ${activeKey
                    ? `<pre class="source-code" data-source-code>${renderSourceText(text, line)}</pre>`
                    : `<pre class="source-code muted" data-source-code>选中一个源码文件后显示内容。</pre>`}
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

const terminalView = register({
    id: "terminal",
    channels: ["term"],
    render(state) {
        const buf = state?.term?.bytes ?? "";
        // ⚠️ **草稿住在视图状态里**（K-1）：`applyRenderEvent` 每次都
        // `body.innerHTML = html` ⇒ **输入框元素被重建** ⇒ 只存在 DOM 里
        // 的草稿会**每来一个 console 字节就清一次** ⇒ 根本没法打字。
        const draft = state?.term?.draft ?? "";
        return `
            <section class="terminal">
                <h3>串口终端 (M-15 UART)</h3>
                <pre class="terminal-out" data-terminal-out>${escapeHtml(buf)}</pre>
                <div class="terminal-input-row">
                    <input type="text" class="terminal-input" data-terminal-input value="${escapeHtml(draft)}" placeholder="输入后按「发送」——字节会进 guest 的 UART0">
                    <button type="button" class="terminal-send" data-action="console-send">发送</button>
                </div>
                <p class="muted">输入框里的文本<strong>按 UTF-8 编码</strong>成字节，发给 guest 的 UART0（契约 v5 的 <code>WriteConsole</code>）；<strong>回显来自 guest</strong>，平台不做本地回显。本批只送原始字节，不模拟终端控制序列。</p>
            </section>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (action !== "console-send") return undefined;
        return sendTerminalInput();
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
 * 把输入框里的文本**按 UTF-8 编码**成字节 ⇒ 发 `console-send` 信号
 * ⇒ 清空输入框。
 *
 * ⚠️ **字节，不是字符**（K-1）：guest 的 UART 收的是**字节**（契约 v5 的
 * `WriteConsole { bytes: Vec<u8> }`）⇒ 这里用 `TextEncoder` 做 UTF-8 编码
 * （`"A"` ⇒ `[65]`、`"你"` ⇒ `[228, 189, 160]`），**不**送字符串（内核会
 * 按 `E_PROTO-08` 拒掉）。
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
    const bytes = Array.from(new TextEncoder().encode(text));
    const requestId = emitCommand("console-send", "terminal", { bytes });
    setTerminalDraft("");
    publishTerminalState();
    return requestId;
}

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
