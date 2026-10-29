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
import { WRITE_SIZE } from "../lib/message.js";
// ⚠️ **「运行中」的判据与活值的隐藏口径**（STAGE-38 · v0.53 批）⇒ ⇒
// 「内核真的在执行吗」与「运行中显示什么」**各只有一处** ⇒ ⇒ 顶栏 /
// 调试视图 / 寄存器视图**共用**同一套口径 ⇒ ⇒ 不会一个藏了一个没藏。
import { isKernelRunning, liveValue } from "../lib/run-loop.js";
// ⚠️ **波形图的纯函数 + 片段**（STAGE-36 · v0.51 批）⇒ ⇒ option 的构造
// **不碰 DOM** ⇒ ⇒ Node 侧零 DOM 就能测「画的是不是真数据」。
import { buildWaveOption, canDrawChart, resolveWaveTheme, waveChartHtml } from "./components/wave-chart.js";
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
    collectDraft,
    countInvalidFields,
    currentDraft,
    defaultDraft,
    draftFromDescription,
    draftToDescription,
    removeRegion,
    renderMemoryMapPanel,
    renderMemoryMapTable,
    setDeviceKind,
} from "./components/memory-map.js";
import {
    loadImageFileIntoPending,
    pendingImage,
    publishProductState,
    renderImageLoader,
    setImageLoadState,
} from "./components/image-loader.js";

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
// productTreeView — shows the M-14 mapping table (FR-01/02/03)
// ---------------------------------------------------------------------------

const productTreeView = register({
    id: "product",
    channels: ["sim"],
    /** Render to body. Pure: same storeState → same DOM string. */
    render(state) {
        // ⚠️ **真数据**（STAGE-14 ⑦ · 裁决 F）：内存映射来自内核的
        // `memory.map` 事件（**当前生效**的板级内存图）⇒ 不再是 demo 的
        // 假数据。
        //
        // ⚠️ **「没有数据」与「数据是空的」必须分得开**：没收到事件 ⇒
        // 「尚无内存图数据」；收到了但 0 段 ⇒ 「当前生效的内存图为空」。
        const memoryMap = state?.memoryMap ?? null;
        const tree = state?.tree ?? { root: { name: "未加载", children: [] } };
        // ⚠️ **折叠状态住视图状态**（裁决 C）⇒ ⇒ 这里**只读** ⇒ ⇒ 视图层
        // 不持有第二份（与断点列表同一条规矩）。
        const collapsed = new Set(Array.isArray(state?.collapsed) ? state.collapsed : []);
        return `
            <div class="cards">
                <section class="card">
                    <div class="row"><h3>产品树</h3><span class="tag">点 ▸ / ▼ 折叠 · 展开</span></div>
                    <div class="tree" data-product-tree>${renderTree(tree.root, collapsed, [], 0)}</div>
                </section>
                <section class="card">
                    <div class="row"><h3>内存映射 (M-14)</h3><span class="tag">当前生效</span></div>
                    ${renderMemoryMapTable(memoryMap)}
                    <div class="debug-toolbar">
                        <button type="button" data-action="go-memory-map">编辑板级内存图</button>
                        <button type="button" data-action="seed-demo">重新 seed demo 状态（dev only）</button>
                    </div>
                </section>
            </div>
            ${renderImageLoader(state?.imageLoad)}
            <p class="muted">
                内存映射表显示的是内核 <code>memory.map</code> 事件里的
                <strong>当前生效</strong>的板级内存图；没有数据时如实显示空态，不显示假数据。
            </p>
        `;
    },
    onCommand(ev) {
        // User action: ask for the demo product tree / sim state to be re-seeded.
        //
        // The button carries `data-action` itself and lives *outside* the
        // <table>, so it has no `[data-region-base]` ancestor — requiring
        // one made the button permanently dead (FRONTEND_Review.md #3).
        const target = /** @type {HTMLElement} */ (ev.target);
        if (target?.matches?.("[data-action='seed-demo']")) {
            seedBus("product");
            return "cmd-seed-demo";
        }
        // 「编辑板级内存图」⇒ 跳到板级内存图编辑器（独立工作面）。
        const el = ev.target?.closest?.("[data-action]");
        if (el?.dataset?.action === "go-memory-map") {
            route("#/memory-map");
            return "cmd-go-memory-map";
        }
        // 折叠 / 展开产品树的一个节点（STAGE-35 · v0.50 批 · 裁决 C）。
        //
        // ⚠️ **折叠状态住视图状态** ⇒ ⇒ 这里写的是 `product.collapsed`
        // ⇒ ⇒ 切走视图再切回来**折叠还在** ⇒ ⇒ ⛔ 不在 DOM 上私藏。
        if (el?.dataset?.action === "toggle-tree") {
            const key = String(el.dataset.treeKey ?? "");
            if (!key) return undefined;
            const prev = viewStore.getState("product")?.collapsed ?? [];
            const set = new Set(Array.isArray(prev) ? prev : []);
            if (set.has(key)) set.delete(key); else set.add(key);
            viewStore.setState("product", (state) => ({ ...state, collapsed: [...set] }));
            publishProductState();
            return "cmd-toggle-tree";
        }
        // 「加载」⇒ **读用户选的文件** ⇒ ⇒ 发一条 `load-image` 信号。
        //
        // ⚠️ **视图层不碰服务层**（本模块的既有规矩）⇒ ⇒ 这里只发信号，
        // 真正发命令的是 app 层（它持有服务接线）。
        //
        // ⚠️ **字节在信号里**：文件选择器的 `files` 拿不到（点击时它是
        // 空的——浏览器只把文件交给 `change` 事件）⇒ ⇒ 必须在 `change`
        // 里**先把字节读出来**再存下来（见 `mountFor`）⇒ ⇒ 这里读的是
        // 那份**已读好的字节**。
        if (el?.dataset?.action === "load-image") {
            const pending = pendingImage();
            if (!pending) {
                setImageLoadState({
                    status: "error",
                    error: "没有待加载的文件 —— 请先用上面的选择器选一个固件镜像",
                    pending: null,
                });
                publishProductState();
                return "cmd-load-image-no-file";
            }
            setImageLoadState({ status: "loading", error: null });
            publishProductState();
            return emitCommand("load-image", "product", {
                bytes: pending.bytes,
                name: pending.name,
                kind: pending.kind,
            });
        }
        return undefined;
    },
    onDelta(_delta) {
        // No-op until M-18 hooks land.
    },
    mountFor(el) {
        el.addEventListener("click", (ev) => productTreeView.onCommand(ev));
        // ⚠️ **文件选择器的 `change`**：浏览器把选中的文件**只**交给这个
        // 事件（点击时 `input.files` 是空的）⇒ ⇒ 必须在这里读字节。
        el.addEventListener("change", (ev) => {
            const input = ev.target?.closest?.("[data-image-input]");
            if (!input) return;
            const file = input.files?.[0] ?? null;
            if (!file) return;
            void loadImageFileIntoPending(file);
        });
    },
});

// ---------------------------------------------------------------------------
// 固件加载（STAGE-32 · v0.47 批）—— 视图层这一侧
// ---------------------------------------------------------------------------

// ⚠️ **`pendingImage()` 与 `loadImageFileIntoPending()` 已经搬走**
// （STAGE-33 · v0.48 批）⇒ ⇒ 它们现在住在
// `components/image-loader.js` ⇒ ⇒ **页头那个入口也共用同一段**（页头在
// **app 层** ⇒ ⇒ 视图层够不着 ⇒ ⇒ 要共用就只能**提到共享组件**）⇒ ⇒
// **不写第二套**（规格 §3 裁决 D）。
//
// ⚠️ 视图层这边**只**剩「点按钮 ⇒ 发信号」那一步（`onCommand` 的
// `load-image` 分支）⇒ ⇒ 载荷（字节 / 文件名 / 类型）从共享组件那份
// pending 里取。

/**
 * 产品树节点的**稳定键**（折叠状态的元素）。
 *
 * ⚠️ **键 = 路径**（每一段是「组件 id，没有就退回名字」）⇒ ⇒ 同一个键
 * 在**重渲染之间不变** ⇒ ⇒ 折叠状态住在视图状态里才找得回来。
 *
 * ⚠️ **同名兄弟不串**：键里带上**序号** ⇒ ⇒ 两个都叫 `UART0` 的节点
 * 是**两个不同的键** ⇒ ⇒ 折叠其中一个不会连带另一个。
 *
 * @param {string[]} path — 祖先的键（从根算起）
 * @param {object|null|undefined} node
 * @param {number} index — 在同一层里的序号
 * @returns {string}
 */
function treeNodeKey(path, node, index) {
    const label = node?.component_id
        ? String(node.component_id)
        : String(node?.name ?? "");
    return [...path, `${label}#${index}`].join("›");
}

/**
 * 渲染**可折叠**的产品树。
 *
 * ⚠️ **折叠状态住视图状态**（STAGE-35 · v0.50 批 · 裁决 C）⇒ ⇒ **不是**
 * DOM 私藏 ⇒ ⇒ 切走视图再切回来**折叠还在** ⇒ ⇒ ⛔ 不用 `hidden` class
 * 私藏（那样状态就只存在于 DOM 里 ⇒ ⇒ 重渲染即丢）。
 *
 * @param {object|null|undefined} node
 * @param {Set<string>} collapsed — 已折叠节点的键集合
 * @param {string[]} path — 祖先的键
 * @param {number} index — 在同一层里的序号
 * @returns {string} HTML 片段
 */
function renderTree(node, collapsed, path = [], index = 0) {
    if (!node) return "";
    const key = treeNodeKey(path, node, index);
    const children = Array.isArray(node.children) ? node.children : [];
    const hasKids = children.length > 0;
    const isCollapsed = collapsed.has(key);
    const kids = hasKids && !isCollapsed
        ? `<div class="kids">${children
            .map((child, i) => renderTree(child, collapsed, [...path, key], i))
            .join("")}</div>`
        : "";
    const id = node.component_id
        ? `<span class="sub">(${escapeHtml(node.component_id)})</span>`
        : "";
    return `
        <div class="node">
            <span class="caret${hasKids ? "" : " leaf"}" data-action="toggle-tree" data-tree-key="${escapeHtml(key)}"
                role="button" tabindex="0" aria-expanded="${hasKids ? (isCollapsed ? "false" : "true") : "false"}"
                title="${hasKids ? (isCollapsed ? "展开" : "折叠") : "（叶子节点）"}">${isCollapsed ? "▶" : "▼"}</span>
            <span class="name">${escapeHtml(node.name ?? "")}</span>
            ${id}
        </div>
        ${kids}
    `;
}

// ---------------------------------------------------------------------------
// memoryMapView — 板级内存图编辑器（STAGE-14 · v0.29）。
//
// ⚠️ **编辑器是独立工作面**：路由 `#/memory-map`、面板 `[data-view-panel="memory-map"]`。
// 它**不**塞进产品树视图（规格 §3.3）。
//
// ⚠️ **草稿住 M-03 视图状态**（规格 §3.3 / 裁决 D）：草稿是**前端自己的东西**
// ⇒ 不进契约、不进事件 ⇒ 刷新页面即失（D-136，规格已如实登记）。
//
// ⚠️ **视图层不碰服务层**：提交只发 `riscv:command` 信号（`emitCommand`），
// 由 `app.js` 的 `handleCommand` 变成真正的 M-05 命令。
// ---------------------------------------------------------------------------

/** 编辑器面板的 DOM（挂载时记下 ⇒ 采集与渲染看的是**同一份**东西）。 */
let memoryMapPanel = null;

/**
 * 内存图视图状态里的**当前草稿**（渲染与采集共用同一个基底）。
 *
 * @returns {object}
 */
function memoryMapDraft() {
    return currentDraft(viewStore.getState("memory-map"));
}

/**
 * 把编辑器面板的最新输入采集成草稿，写回 M-03 视图状态。
 *
 * ⚠️ **不重渲染**：重渲染会重建 DOM ⇒ 输入框失焦、正在敲的字丢掉。
 * 草稿在视图状态里 ⇒ 下次重渲染时值还在。
 *
 * @param {Element} root — 面板元素
 * @returns {object} 采集到的草稿
 */
function syncMemoryMapDraft(root) {
    const draft = collectDraft(root, memoryMapDraft());
    viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
    return draft;
}

/**
 * 让内存图编辑器按当前视图状态重渲染一次。
 */
function publishMemoryMapState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: {
            viewId: "memory-map",
            type: "update",
            state: viewStore.getState("memory-map"),
        },
    }));
}

const memoryMapView = register({
    id: "memory-map",
    channels: ["sim"],
    render(state) {
        return renderMemoryMapPanel(state);
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        const index = Number(el?.dataset?.mmapIndex ?? -1);
        const root = memoryMapPanel;
        switch (action) {
            case "add-ram":
            case "add-rom":
            case "add-peripheral": {
                // ⚠️ 先采集再改：用户正在敲的数**不许**被这次点击冲掉。
                const base = root ? collectDraft(root, memoryMapDraft()) : memoryMapDraft();
                const kind = action === "add-ram" ? "ram" : action === "add-rom" ? "rom" : "peripheral";
                const draft = addRegion(base, kind);
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                publishMemoryMapState();
                return `cmd-${action}`;
            }
            case "remove-region": {
                const base = root ? collectDraft(root, memoryMapDraft()) : memoryMapDraft();
                const draft = removeRegion(base, index);
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                publishMemoryMapState();
                return "cmd-remove-region";
            }
            case "load-current": {
                const current = viewStore.getState("memory-map")?.memoryMap?.description ?? null;
                const draft = draftFromDescription(current) ?? defaultDraft();
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                publishMemoryMapState();
                return "cmd-load-current";
            }
            case "new-draft": {
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft: defaultDraft() }));
                publishMemoryMapState();
                return "cmd-new-draft";
            }
            case "submit-memory-map": {
                const root2 = root;
                const draft = root2 ? collectDraft(root2, memoryMapDraft()) : memoryMapDraft();
                // ⚠️ 有栏没看懂 ⇒ **不提交**（提交一份内核看不懂的描述只会换来
                // 一个更费解的错）。
                const invalid = root2 ? countInvalidFields(root2) : 0;
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                if (invalid > 0) {
                    publishMemoryMapState();
                    return "cmd-submit-memory-map-invalid";
                }
                // ⚠️ 载荷形状 = 契约的 `PlatformDescription`（**逐字同形**）。
                return emitCommand("submit-memory-map", "memory-map", {
                    description: draftToDescription(draft),
                });
            }
            case "reload-project":
                // 「重新装载工程」⇒ 宿主在下次 LoadProject 时应用待生效的板级配置。
                return emitCommand("reload-project", "memory-map");
            case "unload-project":
                // 「卸载工程」⇒ 契约 v7 的 `UnloadProject`（D-139）。
                //
                // ⚠️ **只发这一个信号**：真正发命令的是 app 层（它持有服务
                // 接线）⇒ 视图层**不**自己碰内核。
                return emitCommand("unload-project", "memory-map");
            case "save-memory-map": {
                // 「保存当前板」⇒ 把**当前草稿**写进板档案（键 = 草稿的 id）。
                //
                // ⚠️ **不改内核状态** ⇒ 不发契约命令 ⇒ 只发 `riscv:command`
                // 信号给 app.js（它持有板档案存储）。
                // ⚠️ **先采集再发**：用户正在敲的内容以 DOM 为准。
                const draft = root ? collectDraft(root, memoryMapDraft()) : memoryMapDraft();
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                return emitCommand("save-memory-map", "memory-map");
            }
            case "save-memory-map-as": {
                // 「另存为板」⇒ 以当前草稿为模板**新建**一块板（新 id + 新 name）。
                //
                // ⚠️ 新 id / 新 name 从**面板上的两个输入框**读（不用 `prompt()`
                // —— 浏览器真机门禁没法驱动 `prompt`）。
                const draft = root ? collectDraft(root, memoryMapDraft()) : memoryMapDraft();
                viewStore.setState("memory-map", (prev) => ({ ...prev, draft }));
                const newId = root?.querySelector?.("[data-mmap-new-id]")?.value ?? "";
                const newName = root?.querySelector?.("[data-mmap-new-name]")?.value ?? "";
                return emitCommand("save-memory-map-as", "memory-map", {
                    id: String(newId ?? ""),
                    name: String(newName ?? ""),
                });
            }
            case "select-memory-map": {
                // 「选用」档案里的一块 ⇒ **载入草稿**（`draftFromDescription`）
                // ⇒ `selectedId` 跟着变。
                //
                // ⚠️ **不发契约命令**（内核状态不变）⇒ 但**要发** `riscv:command`
                // 信号：档案与 `selectedId` 住在 app.js 那一侧的视图状态里。
                const id = String(el?.dataset?.mmapTargetId ?? "");
                return emitCommand("select-memory-map", "memory-map", { id });
            }
            case "delete-memory-map": {
                // 「删除」档案里的一块 ⇒ 不改内核状态。
                const id = String(el?.dataset?.mmapTargetId ?? "");
                return emitCommand("delete-memory-map", "memory-map", { id });
            }
            case "new-memory-map": {
                // 「新板」⇒ 清空当前编辑（草稿回到默认、**取消选中**）。
                //
                // ⚠️ **不发命令**：它只改**草稿**与 `selectedId`（都在视图状态里）
                // ⇒ 落库与否是「保存」那一步的事。
                viewStore.setState("memory-map", (prev) => ({
                    ...prev,
                    draft: defaultDraft(),
                    selectedId: null,
                }));
                publishMemoryMapState();
                return "cmd-new-memory-map";
            }
            case "focus-region": {
                // 点比例条上的**一段** ⇒ 焦点跳到下面段表的**那一行**
                // （STAGE-35 · v0.50 批）—— 横幅上写着的「点击任一分段即可在
                // 下方编辑」**必须是真的**（通用判据：界面文案是能力声明 ⇒ ⇒
                // 声明的必须是现在真有的）。
                //
                // ⚠️ **只挪焦点，不改草稿** ⇒ ⇒ 用户的输入不会被这次点击冲掉。
                const row = root?.querySelector?.(`[data-mmap-row="${index}"]`) ?? null;
                const field = row?.querySelector?.("input, select") ?? null;
                if (field && typeof field.focus === "function") {
                    field.focus();
                    row?.scrollIntoView?.({ block: "nearest" });
                }
                return "cmd-focus-region";
            }
            default:
                return undefined;
        }
    },
    onDelta(_delta) {},
    mountFor(el) {
        memoryMapPanel = el;
        el.addEventListener("click", (ev) => memoryMapView.onCommand(ev));
        // ⚠️ 输入 ⇒ 采集进草稿（**不重渲染** ⇒ 不失焦）。
        el.addEventListener("input", () => {
            if (memoryMapPanel) syncMemoryMapDraft(memoryMapPanel);
        });
        // ⚠️ 换器件类型 ⇒ 参数集变了 ⇒ 必须重渲染（否则还挂着旧类型的参数栏）。
        el.addEventListener("change", (ev) => {
            if (!memoryMapPanel) return;
            const changed = ev.target?.closest?.("[data-mm-device-kind]");
            syncMemoryMapDraft(memoryMapPanel);
            if (changed) publishMemoryMapState();
        });
    },
});

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
                <div class="cards">
                    <div class="card kpi">
                        <span class="k">当前 PC</span>
                        <code class="v pc" data-embedded-pc>0x${pc.toString(16).padStart(8, "0")}</code>
                    </div>
                    <div class="card kpi">
                        <span class="k">虚拟时间</span>
                        <code class="v timeVirt" data-embedded-time>${escapeHtml(String(timeVirt))} ps</code>
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
export const ADDR_INPUT_ERROR = "地址格式不对：请填十六进制（最多 8 位，可带 0x 前缀）。";

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
 * 读**当前草稿**（断点地址 / 读内存地址 / 宽度）。
 *
 * @returns {{bpAddr: string, memAddr: string, memSize: string}}
 */
export function debuggerDraft() {
    const s = viewStore.getState(DEBUGGER_STATE_KEY) ?? {};
    const size = String(s.memSize ?? WRITE_SIZE.Word);
    return {
        bpAddr: String(s.bpAddr ?? ""),
        memAddr: String(s.memAddr ?? ""),
        memSize: Object.prototype.hasOwnProperty.call(WRITE_SIZE, size) ? size : WRITE_SIZE.Word,
    };
}

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
 * @param {string} action
 * @param {Element|null} el — 被点的那个按钮
 * @returns {{addr?: number|string, size?: string, invalid?: boolean}|null}
 */
function resolveAddrParams(action, el) {
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
        return { addr: parsed.value };
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
        if (target?.closest?.("[data-mem-addr-input]")) {
            setDebuggerDraft({ memAddr: target.value ?? "" });
            setAddrError(target.closest("[data-addr-input]"), "[data-mem-addr-error]", "");
        }
    });
    el.addEventListener("change", (ev) => {
        const target = ev.target?.closest?.("[data-mem-size]");
        if (!target) return;
        setDebuggerDraft({ memSize: target.value ?? WRITE_SIZE.Word });
    });
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
        // ⚠️ **运行中 ⇒ ⇒ 活值隐藏**（STAGE-38 · v0.53 批 · 裁决 D）
        // ⇒ ⇒ 寄存器 / PC / 虚拟时间一律显示**固定符号 `-`** ⇒ ⇒ 界面
        // **不闪**（那些数字每秒要跳 5 次 · F6）⇒ ⇒ 停止后**自动**恢复。
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
        const exportState = debug.export ?? null;
        const traceState = debug.trace ?? null;
        const error = debug.error ?? null;
        // ⚠️ **断点列表只有一个落点**（STAGE-35 · v0.50 批 · 裁决 K）⇒ ⇒
        // 这里**只读**视图状态 ⇒ ⇒ 视图层**不持有**第二份列表。
        const breakpoints = Array.isArray(debug.breakpoints) ? debug.breakpoints : [];
        // ⚠️ **「停下」也只有一个落点**（裁决 L）⇒ ⇒ 这里读 `debug.halt`。
        const halt = debug.halt ?? null;
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
                <button type="button" data-action="disasm" data-addr="${pc}" data-count="16">反汇编</button>
                <button type="button" data-action="export-trace" data-source="Execution" data-format="Csv" data-offset="0" data-limit="0">导出执行轨迹 (CSV)</button>
                <button type="button" data-action="export-trace" data-source="Waveform" data-format="Vcd" data-offset="0" data-limit="0">导出波形 (VCD)</button>
                ${canContinue ? `<button type="button" data-action="export-trace" data-source="${escapeHtml(String(exportState.source))}" data-format="${escapeHtml(String(exportState.format))}" data-offset="${Number(exportState.next_offset)}" data-limit="0" data-continue="1">继续导出</button>` : ""}
                <button type="button" data-action="trace-enable">开启 trace 记录</button>
                <button type="button" data-action="trace-disable">关闭 trace 记录</button>
            </div>
            ${halt?.reason === "Breakpoint"
                // ⚠️ **这句话只在内核这么说的时候出现**（裁决 M）⇒ ⇒ 前端
                // **不自己判断**「跑没跑到那个地址」⇒ ⇒ 判据 = 内核的
                // `Halted.reason`。
                ? `<p class="bp-hit" data-bp-hit>命中断点 @ <code>0x${(Number(halt.pc ?? 0) >>> 0).toString(16).padStart(8, "0")}</code></p>`
                : ""}
            ${traceState?.enabled === true
                ? `<p class="trace-notice" data-trace-notice>trace 记录已开启 —— 仿真会明显降速（每条指令与访存都要记录）。</p>`
                : ""}
            ${error
                ? `<p class="debug-error" data-debug-error>${escapeHtml(String(error.code))}：${escapeHtml(String(error.message))}</p>`
                : ""}
            <div class="debug-grid">
                <section class="card disasm">
                    <div class="row"><h3>反汇编 (M-11)</h3><span class="tag">在「断点」列点一下即可下断点</span></div>
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
                    ${breakpoints.length === 0
                        ? `<p class="muted">尚无断点 — 在左边反汇编表的「断点」列点「加断点」，或在上面直接填地址。</p>`
                        : `<div class="bp-list" data-bp-list>${breakpoints.map((raw) => {
                            const addr = Number(raw ?? 0) >>> 0;
                            const addrText = `0x${addr.toString(16).padStart(8, "0")}`;
                            return `
                                <div class="bp-row" data-bp-row data-bp-addr="${addrText}">
                                    <code class="addr">${addrText}</code>
                                    <span class="note">断点 id = 地址</span>
                                    <button type="button" data-action="clear-breakpoint" data-addr="${addrText}">撤掉</button>
                                </div>`;
                        }).join("")}</div>`}
                    <p class="muted">这里列的是<strong>本会话由界面请求过的断点</strong>（界面回显）—— 内核没有「列出断点」的事件，所以它<strong>不是</strong>内核断点集的镜像；用脚本口设的断点在这里看不到。</p>
                </section>
                <section class="card trace-export">
                    <div class="row"><h3>trace 导出 (M-11 / M-19)</h3></div>
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
                <section class="card regmem">
                    <div class="row"><h3>寄存器 / 内存</h3></div>
                    ${renderMemoryReadRow({ draft: draft.memAddr, size: draft.memSize })}
                    <div class="grid-2">
                        <div>
                            <h4>寄存器</h4>
                            <div class="reg-grid">${renderRegGrid(sim.gpr ?? Array(32).fill(0), previousGprSnapshot(), running)}</div>
                        </div>
                        <div>
                            <h4>PC</h4>
                            <code class="pc">${liveValue(running, `0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}`)}</code>
                            <h4>虚拟时间</h4>
                            <code class="timeVirt">${liveValue(running, `${state?.timeVirt ?? 0} ps`)}</code>
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
        // ⚠️ **任意地址**（STAGE-39 · v0.54 批 · 裁决 B）⇒ ⇒ 地址有两个
        // 来源：按钮**自带** `data-addr`（反汇编行的旧按钮 ⇒ ⇒ **一字不变**）
        // 或**同屏**的地址输入框 ⇒ ⇒ 用户给什么地址就下在什么地址。
        const addrParams = resolveAddrParams(action, el);
        // ⚠️ **解析不出来 ⇒ ⇒ 不发命令**（裁决 F）⇒ ⇒ 「点了没反应」是最差
        // 的 UX ⇒ ⇒ 输入框旁边**就地**有提示 ⇒ ⇒ 这里**直接返回**。
        if (addrParams?.invalid === true) return undefined;
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
// regmemView — the "寄存器/内存" view (M-10 data).
//
// The panel existed in M-01's view registry from the start, but no
// component was registered for it — the tab rendered an empty body.
// M-10 now feeds the store, so the view has something to show.
// ---------------------------------------------------------------------------

/**
 * 「**内核真的在执行**吗」（STAGE-38 · v0.53 批 · 裁决 A / M）。
 *
 * ⚠️ **只有一个落点**：视图状态 `debug.run`（app 层写）⇒ ⇒ 调试视图与
 * 寄存器视图**读同一份** ⇒ ⇒ 不是第二个真相。
 *
 * ⚠️ **两个视图都走这一个函数** ⇒ ⇒ 「运行中」的口径**只有一处** ⇒ ⇒
 * 「运行中显示 `-`」这件事**不会**只落在一个视图上。
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
 * ⚠️ **变化高亮**（STAGE-35 · v0.50 批 · 裁决 F）：与**上一次读回**的 GPR
 * 快照比 ⇒ ⇒ 变大加 `.up` · 变小加 `.down` · 没变**不加类**。
 *
 * ⚠️ **没有上一次快照** ⇒ ⇒ **一律不加类**（⛔ 不猜「那一定是 0」）
 * ⇒ ⇒ 第一次读回时**不**满屏绿红。
 *
 * ⚠️ **`running` = 运行中**（STAGE-38 · v0.53 批 · 裁决 D）⇒ ⇒ 运行中
 * 每格只显示**固定符号 `-`**（不是占位文案、不是冻结旧值）⇒ ⇒ 停止后
 * 调同一个函数就**自动**恢复最新值。
 *
 * ⚠️ **运行中不算变化高亮** ⇒ ⇒ 高亮说的是「这个值变了」⇒ ⇒ 值都没显示
 * ⇒ ⇒ 高亮没有意义（而且会让那一格看起来像「有值」）。
 *
 * @param {number[]} gpr
 * @param {number[]|null} prevGpr
 * @param {boolean} [running]
 * @returns {string}
 */
export function renderRegGrid(gpr, prevGpr, running = false) {
    const values = Array.isArray(gpr) ? gpr : Array(32).fill(0);
    const prev = Array.isArray(prevGpr) ? prevGpr : null;
    return values.map((v, i) => {
        const cur = Number(v ?? 0) >>> 0;
        let cls = "";
        if (running !== true && prev && prev.length === values.length) {
            const old = Number(prev[i] ?? 0) >>> 0;
            if (cur !== old) cls = cur > old ? "up" : "down";
        }
        const clsAttr = cls ? ` class="${cls}"` : "";
        const text = liveValue(running, cur.toString(16).padStart(8, "0"));
        return `<div><span class="muted">x${i}</span><code${clsAttr}>${text}</code></div>`;
    }).join("");
}

/**
 * **上一次读回的 GPR 快照**（视图层**只读** · 裁决 F）。
 *
 * ⚠️ **落点在 `regmem` 视图状态里** ⇒ ⇒ app 层在 `read-regs` **之前**存
 * ⇒ ⇒ 视图层**不自己存**（否则又是第二份）。
 *
 * @returns {number[]|null}
 */
function previousGprSnapshot() {
    return viewStore.getState("regmem")?.regmem?.prevGpr ?? null;
}

const regmemView = register({
    id: "regmem",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        const gpr = sim.gpr ?? Array(32).fill(0);
        const mem = sim.mem ?? {};
        const cells = Object.entries(mem);
        // ⚠️ **运行中 ⇒ ⇒ 活值隐藏**（STAGE-38 · v0.53 批 · 裁决 D）
        // ⇒ ⇒ 与**调试视图那份**是**同一个口径**（同一个函数）⇒ ⇒ 不会
        // 一个藏了一个没藏。
        const running = kernelIsRunning();
        // ⚠️ **草稿住共享命名空间**（STAGE-39 · v0.54 批 · 裁决 G）⇒ ⇒ 与
        // 调试视图**同一份** ⇒ ⇒ 两个入口**一个真相**。
        const draft = debuggerDraft();
        return `
            <section class="regmem-view">
                <div class="card">
                    <div class="row"><h3>寄存器 / 内存 (M-10)</h3><span class="tag">变化高亮 = 与上一次读回相比</span></div>
                    ${renderMemoryReadRow({ draft: draft.memAddr, size: draft.memSize })}
                    <div class="debug-toolbar">
                        <button type="button" data-action="read-regs">读寄存器</button>
                    </div>
                    <div class="grid-2">
                        <div>
                            <h4>通用寄存器</h4>
                            <div class="reg-grid">${renderRegGrid(gpr, previousGprSnapshot(), running)}</div>
                        </div>
                        <div>
                            <h4>PC / 虚拟时间</h4>
                            <code class="pc">${liveValue(running, `0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}`)}</code>
                            <code class="timeVirt">${liveValue(running, `${state?.timeVirt ?? 0} ps`)}</code>
                            <h4>内存单元</h4>
                            ${cells.length === 0
                                ? `<p class="muted">尚无读回的内存单元 — 在上面填一个地址，点「读内存」。</p>`
                                : `<table class="map-table">
                                    <thead><tr><th>地址</th><th>值</th></tr></thead>
                                    <tbody>${cells.map(([addr, value]) => `
                                        <tr><td><code>${escapeHtml(addr)}</code></td>
                                        <td><code>${liveValue(running, `0x${(value >>> 0).toString(16).padStart(8, "0")}`)}</code></td></tr>
                                    `).join("")}</tbody>
                                </table>`}
                        </div>
                    </div>
                </div>
            </section>
        `;
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        // ⚠️ **任意地址**（STAGE-39 · v0.54 批 · 裁决 B）⇒ ⇒ 地址与宽度都
        // 从**同屏**的输入框来 ⇒ ⇒ 参数**随信号走**（视图层不碰服务层）。
        const addrParams = resolveAddrParams(action, el);
        if (addrParams?.invalid === true) return undefined;
        return emitCommand(action, "regmem", { ...(addrParams ?? {}) });
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => regmemView.onCommand(ev));
        mountAddrInputListeners(el);
    },
});

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
                    <div class="row"><h3>波形 (M-19)</h3><span class="tag">${channels.length} 个通道</span></div>
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
        // ⚠️ **TX / RX 是两个不同的源**（STAGE-35 · v0.50 批 · 裁决 G）⇒ ⇒
        // **分别标注方向** ⇒ ⇒ 不许把两个数混成一个。
        const txBytes = Number(state?.term?.txBytes ?? 0);
        const rxBytes = Number(state?.term?.rxBytes ?? 0);
        return `
            <section class="terminal">
                <div class="card">
                    <div class="row"><h3>串口终端 (M-15 UART)</h3><span class="tag">收发字节分别计数</span></div>
                    <div class="term-scroll">
                        <pre class="terminal-out" data-terminal-out>${escapeHtml(buf)}</pre>
                    </div>
                    <div class="term-meta">
                        <span class="dir">TX（guest → 终端，内核 console 事件累计）</span><span class="n" data-term-tx>${txBytes}</span>
                        <span class="dir">RX（终端 → guest，前端发出累计）</span><span class="n" data-term-rx>${rxBytes}</span>
                    </div>
                    <div class="terminal-input-row">
                        <input type="text" class="terminal-input" data-terminal-input value="${escapeHtml(draft)}" placeholder="输入后按「发送」——字节会进 guest 的 UART0">
                        <div class="terminal-eol" role="group" aria-label="行结束符">${renderTerminalEolButtons(eol)}</div>
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
        // ⚠️ **计数也要就地更新**：这条路径**不重渲染**（只写 `textContent`）
        // ⇒ ⇒ 不改计数的话 TX 会**一直停在旧值**。
        const txEl = document.querySelector("[data-term-tx]");
        if (txEl) {
            txEl.textContent = String(Number(viewStore.getState("terminal")?.term?.txBytes ?? 0));
        }
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
 * `riscv:seed` signal; the dev wiring in app.js turns that signal into
 * store state. The demo data itself lives in `front/dev/demo-state.js`.
 *
 * @param {string} [viewId]
 */
export function seedBus(viewId = "product") {
    window.dispatchEvent(new CustomEvent("riscv:seed", { detail: { viewId } }));
}
