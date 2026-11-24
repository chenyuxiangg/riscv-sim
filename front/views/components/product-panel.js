// 产品视图面板（M-02 · product / embedded）—— 拆分自 m02_viewcomponents.js。
//
// 管两个注册组件：
//   - productTreeView  — 双栏工作面：左侧产品树 + 右侧器件内存窗口（FR-01/02/03）
//   - embeddedView     — 落地概览视图（连接状态 / 当前 PC + 三个子视图入口）
//
// 本模块只**读**视图状态、在事件处理里写草稿；渲染纯函数住在
// `product-workspace.js` 等既有组件库里。

import * as viewStore from "../m03_viewstore.js";
// ⚠️ **M-02 → M-01 是文档化的依赖方向**（M-02 LLD §9.1）：视图组件**请求**
// 跳转，路由与能力位复校验仍然只有 M-01 一处。这里**没有**循环依赖：
// M-01 只依赖 M-03，不依赖 M-02。
import { route } from "../m01_workbench.js";
import { escapeHtml } from "../../lib/escape.js";
import { addRegion, parseNumber } from "./memory-map.js";
import { buildDeviceNodes, seedRegisterDoc } from "./device-docs.js";
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
} from "./product-workspace.js";
import { register, emitCommand } from "../component-registry.js";

// ---------------------------------------------------------------------------
// productTreeView — 双栏工作面：左侧产品树 + 右侧器件内存窗口（FR-01/02/03）
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
    const current = state.docs?.[key] ?? seedRegisterDoc(node.deviceKind, state?.deviceTypes);
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
            // ⚠️ **中断号汇总面的行**（v0.143）：键是树节点键，但用的是自己的钩子
            //   （`data-irq-key`）⇒ ⇒ 不冒充树节点 ⇒ 选中语义与树**同一个动作**。
            case "select-irq-device": {
                setProductState({ selected: el.dataset.irqKey ?? null, dialog: null });
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
                    setNoteInProduct("请至少勾选一个器件");
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
                    setNoteInProduct("模版未找到，档案可能已删除");
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
                    setNoteInProduct("请先创建或装载一块板");
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
            case "reload-project": {
                if (state.loaded !== true) return undefined;
                emitCommand("reload-project", "product", null);
                return "cmd-reload-project";
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
    // 而内核重排段之后 key 会变 ⇒ ⇒ 不搬家就又变成「改了含义一重载就没了」。
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
// ⇒ 得同时改 tab 条与 `ROLE_DEFAULT_VIEW` ⇒ 改动面更大、用户拿到的信息更少。
//
// ⚠️ **它不是第二个寄存器表**：这里是**概览 + 入口** —— 连接状态、当前 PC、
// 虚拟时间，加上**三个**子视图的入口。寄存器与内存的明细在**调试视图**的
// 「寄存器窗口」与「内存窗口」里。
// ---------------------------------------------------------------------------

/**
 * 落地视图的**三个子视图入口**（裁决 C 的最小内容之一）。
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
                        : `<span class="muted">（${escapeHtml(String(link.lastError ?? "未就绪"))}）</span>`}
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
                    <div class="row"><h3>子视图</h3><span class="tag">${EMBEDDED_VIEW_ENTRIES.length} 个</span></div>
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
