// App 层 · 板档案持久化、产品板装载、内核装载底座与流重订（从 app.js 下沉）。
//
// ⚠️ 单向依赖：视图状态（M-03）、公共视图派发辅助、state 下沉访问器、
// run-state 运行态族与纯函数 lib；不回引门面 ⇒ ⇒ 不形成子模块 ↔ 门面循环。

import * as viewStore from "../m03_viewstore.js";
import { setNote, publishViewState } from "./shared.js";
import { setLoadedProject } from "./state.js";
import { setRunState } from "./run-state.js";
import { initialRunState } from "../../lib/run-loop.js";
import { Command } from "../../lib/message.js";
import { buildMockProject } from "../../dev/mock-project.js";
import {
    boardErrorHint,
    draftToDescription,
    planMemoryMapUpdate,
    splitErrorCode,
} from "../components/memory-map.js";
import { docsByDevice, docsForKeyed } from "../components/device-docs.js";
import { projectTreeFrom } from "../components/product-workspace.js";
import {
    adoptProductDescription,
    markProductApplied,
} from "../m02_viewcomponents.js";

/**
 * 板档案服务（M-23）的**懒加载**句柄。
 *
 * ⚠️ **为什么是动态 import 而不是静态 import**：板档案服务是**另一条
 * lane** 的产出（规格 §3.3 冻结了 API 形状）⇒ 前端**不因为它缺席就
 * 白屏**（D-01 / D-02 的教训：缺一个模块 ⇒ 整个页面空白，而且页面上看不出
 * 任何异常）⇒ 缺席时**显式**降级为「板档案不可用」，**不静默假装成功**。
 *
 * @type {Promise<{api: object|null, error: Error|null}>|null}
 */
let boardStoreModulePromise = null;

/**
 * 取板档案服务模块（**只加载一次**）。
 *
 * @returns {Promise<{api: object|null, error: Error|null}>}
 */
export function loadBoardStoreModule() {
    if (!boardStoreModulePromise) {
        boardStoreModulePromise = import("../../services/m23_boardstore.js")
            .then((api) => ({ api, error: null }))
            .catch((error) => ({ api: null, error }));
    }
    return boardStoreModulePromise;
}

/**
 * 建板档案的**存储实例**（持久化层）。
 *
 * ⚠️ **不支持就明确说**（裁决 B / C-10）：`{supported: false}` ⇒ 状态
 * `unsupported` ⇒ 界面明说「板档案只存在于本会话」⇒ **不静默假装成功**。
 *
 * @returns {Promise<{store: object|null, status: string, reason: string|null}>}
 */
export async function createBoardStoreInstance() {
    const { api, error } = await loadBoardStoreModule();
    if (!api) {
        return {
            store: null,
            status: "error",
            reason: `板档案服务不可用：${error?.message ?? error}`,
        };
    }
    const backend = api.createIndexedDbBackend();
    if (!backend || backend.supported !== true) {
        return {
            store: null,
            status: "unsupported",
            reason: String(backend?.reason ?? "本浏览器没有可用的持久化存储"),
        };
    }
    return { store: api.createBoardStore(backend), status: "ok", reason: null };
}

/**
 * 删除产品视图「档案管理」里的一条板档案。
 *
 * ⚠️ 只动本机档案，不改当前产品、不改内核状态；删除后当前产品仍可继续用。
 *
 * @param {object} services
 * @param {string} id
 */
export async function deleteProductArchive(services, id) {
    const state = viewStore.getState("product") ?? {};
    const archive = Array.isArray(state.archive) ? state.archive : [];
    const target = String(id ?? "");
    if (!archive.some((row) => String(row?.id) === target)) {
        setNote(`板档案里没有 id「${target}」这一块`);
        return;
    }
    const store = services?.boardStore ?? null;
    if (store) {
        try {
            await store.remove(target);
        } catch (err) {
            setNote(`板档案删除失败：${err?.message ?? err}`);
            return;
        }
    }
    patchProductBoard({ archive: archive.filter((row) => String(row?.id) !== target) });
    setNote(store
        ? `已删除板档案「${target}」`
        : `已删除板档案「${target}」（板档案只存在于本会话）`);
}

/**
 * 产品视图这一侧的板档案状态（住「product」视图状态，与板级内存图视图
 * 那套 `memory-map` 的字段**各住一处**，互不混淆）。
 *
 * @param {object} patch
 */
export function patchProductBoard(patch) {
    viewStore.setState("product", (prev) => ({ ...prev, ...patch }));
    publishViewState("product");
}

/**
 * 重新读一次板档案列表（「从模版加载」弹层的数据源）。
 *
 * ⚠️ 读失败**不静默**：状态写 `error` 并显示原因（板档案只是**模版来源**，
 * 读不出来不影响已经装上的产品）。
 *
 * @param {{boardStore?: object|null}} services
 */
export async function refreshProductArchive(services) {
    const store = services?.boardStore ?? null;
    if (!store || store.supported === false) {
        patchProductBoard({ archive: [], archiveStatus: "unsupported" });
        return;
    }
    try {
        const entries = await store.list();
        patchProductBoard({
            archive: Array.isArray(entries) ? entries : [],
            archiveStatus: "ok",
        });
    } catch (err) {
        patchProductBoard({ archiveStatus: "error" });
        setNote(`板档案读取失败：${err?.message ?? err}`);
    }
}

/**
 * 把一份「内核接受过的描述 + 寄存器文档」写进**本机数据库**（板档案）。
 *
 * ⚠️ **落库门槛**（沿用规格裁决 B）：只有内核接受过的描述才允许落库 ⇒
 * 调用点都在 `SetMemoryMap` 成功之后；本函数**不**自己判描述合法性。
 *
 * @param {{boardStore?: object|null}} services
 * @param {object} description — 内核接受过的描述
 * @param {object} registers — 按器件身份索引的寄存器文档（可为 `{}`）
 * @returns {Promise<{saved: boolean, id: string|null, reason: string|null}>}
 */
export async function writeProductArchive(services, description, registers) {
    const store = services?.boardStore ?? null;
    if (!store || store.supported === false) {
        return { saved: false, id: null, reason: "unsupported" };
    }
    const { api } = await loadBoardStoreModule();
    if (!api) return { saved: false, id: null, reason: "module" };
    const entry = api.boardEntry(description, registers);
    if (!entry) return { saved: false, id: null, reason: "no-id" };
    try {
        await store.put(entry);
    } catch (err) {
        setNote(`板档案写入失败：${err?.message ?? err}`);
        return { saved: false, id: entry.id, reason: "write" };
    }
    await refreshProductArchive(services);
    return { saved: true, id: entry.id, reason: null };
}

/**
 * 把一份板描述**装进内核**（产品视图的「创建 / 从模版加载 / 重新载入」共用）。
 *
 * 三步绑在一个函数里 ⇒ 调用点不会漏掉后两步：
 * 1. `SetMemoryMap` / `PatchMemoryMap` —— 路由仍由 `planMemoryMapUpdate` 判；
 * 2. `LoadProject` —— 宿主在这次装载时应用**待生效**的板；
 * 3. 视图状态扶正 —— 树、板草稿、寄存器基线与「已装载」标志。
 *
 * ⚠️ **基址真提交内核，偏移 / 含义是前端文档层**（`DeviceSpec` 没有寄存器
 * 偏移字段）⇒ 第 3 步把 `docs` 扶正成新基线。
 *
 * @param {object} services
 * @param {object} rawDescription — 草稿形状的描述（数值可能是字符串）
 * @param {{fresh?: boolean, registers?: object}} [options]
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>}
 */
export async function applyProductBoard(services, rawDescription, options = {}) {
    const {
        commandRouter,
        registerMemory,
        waveform,
        debug,
        console: consoleBridge,
        memoryMap: memoryMapBridge,
    } = services;
    const description = draftToDescription(rawDescription ?? {});
    const product = viewStore.getState("product") ?? {};
    const base = product.memoryMap?.description ?? null;
    // ⚠️ **期望修订号**：从没收到过 `memory.map` 事件 ⇒ `null`（不检查）。
    const expectedRevision = product.memoryMap
        ? Number(product.memoryMap.revision ?? 0)
        : null;
    const plan = planMemoryMapUpdate(base, description, {
        pending: product.memoryMap?.pending === true,
    });
    if (plan.mode !== "none") {
        try {
            if (plan.mode === "replace") {
                await commandRouter.dispatchCommand(
                    Command.setMemoryMap(description, expectedRevision),
                );
            } else {
                await commandRouter.dispatchCommand(
                    Command.patchMemoryMap(plan.ops, expectedRevision),
                );
            }

            // ⚠️ 只有**内核接受过**的描述才算「被接受过」（板档案落库门槛）。
            // ⚠️ 接受成功 ⇒ **清掉**上一次的横幅（否则旧错会一直挂着）。
            viewStore.setState("product", (prev) => ({
                ...prev,
                acceptedDescription: description,
                error: null,
            }));
        } catch (err) {
            const { code, message } = splitErrorCode(err);
            // ⚠️ **v0.77（STAGE-53 · 表 11）**：校验失败要在**工作面横幅**上
            //    说出来（用户当时的实感是「弹层关了，什么都没发生」）。
            const hint = boardErrorHint(code);
            const detail = hint ? `${hint}${message ? `（${message}）` : ""}` : message;
            viewStore.setState("product", (prev) => ({
                ...prev,
                error: { code, message: detail || code },
            }));
            setNote(`内存图流不可用：${code} ${message}`);
            throw err;
        }
    }
    // 树跟着描述走：核节点固定，外设节点 = 每段外设的器件 id。
    const tree = projectTreeFrom(description);
    viewStore.setState("product", (prev) => ({ ...prev, tree }));
    // ⚠️ 从**档案**装载时把寄存器文档一起搬回视图状态（按器件身份 → 当前
    // 节点 key）：档案里存了偏移 / 含义 ⇒ 装出来的东西必须和保存时一致。
    const docs = options.fresh === true
        ? docsForKeyed(options.registers ?? {}, description, tree)
        : undefined;
    adoptProductDescription(description, { resetDocs: options.fresh === true, docs });
    // ⚠️ `images: []` 是**如实**的：产品视图这一批还没有固件装载面。
    const project = {
        id: `proj-${description.id || "board"}`,
        name: description.name || "产品",
        tree,
        images: [],
        version: { schema: 1, isa: "RV32IMC", toolchain: "front (product view)" },
    };
    const result = await loadProjectIntoKernel(
        {
            commandRouter,
            registerMemory,
            waveform,
            debug,
            console: consoleBridge,
            memoryMap: memoryMapBridge,
            devices: services.devices,
            templates: services.templates,
        },
        project,
        // ⚠️ **v0.88**：这是「让板生效」的内部空装载 ⇒ 保留「用户当前工程」
        //   （含固件）⇒ 「重新装载工程」才真的重装固件（方案 B）。
        { remember: false },
    );
    markProductApplied();
    // ⚠️ **内核接受过 ⇒ 顺手写进本机数据库**（板档案）：重新载入、创建、
    // 从模版加载三条通路都走这里 ⇒ 库里的板永远等于「最近一次真装上的那份」
    // （带偏移 / 含义）。写库失败只提示、不回滚装载。
    await writeProductArchive(
        services,
        description,
        docsByDevice(viewStore.getState("product") ?? {}),
    );
    return result;
}

/**
 * Bring the session up: create it, load the mock project, subscribe to
 * the register + probe streams and probe the link with a `Nop` round-trip.
 *
 * Never throws — a missing kernel (e.g. the front-end gate running
 * under Node, where `Worker` does not exist) must not take the whole
 * workbench down.
 *
 * @param {{session: object, commandRouter: object, registerMemory: object, waveform: object, workspace: object}} services
 */
export async function startSession(services) {
    const { session, commandRouter, registerMemory, waveform, debug, workspace } = services;
    try {
        await session.createSession({ role: workspace.role });
        // ⚠️ **产品视图默认是空产品**（v0.77 双栏工作面的口径）：启动时
        // 内核里那块 default 板 / mock 工程**不算用户的产品** ⇒ ⇒ 产品树
        // 不显示它（否则「默认不加载任何板子」这条就落不了地）。
        //
        // ⚠️ **只动 `product` 视图状态**：板级内存图视图（`memory-map`）
        // 照旧吃内核推来的 `memory.map`，两个视图各管各的。
        viewStore.setState("product", (prev) => ({
            ...prev,
            loaded: false,
            locked: true,
            selected: null,
            board: null,
            tree: null,
            docs: {},
            docsBase: {},
        }));
        publishViewState("product");
        const project = buildMockProject();
        // 装载工程**并且**把内核侧订阅都强制重订。
        //
        // 为什么必须是「强制」：`LoadProject` 会清掉内核侧的主题集合
        // （新工程 = 新会话，FR-13），而 `ensureSubscribed()` 是**幂等**
        // 的——本地标志可能还写着「已订阅」（会话绑定
        // `createSession` → `bindSession` → 传输状态变化会触发一次），
        // 实际内核侧的订阅却已经随旧工程一起没了。结果就是寄存器推送
        // **静默停掉**，视图停在旧值上，不报任何错。
        //
        // 这个缺陷在 mock 内核时代就存在，是第 1 阶段前端接上**真内核**
        // 之后、被真机断言「x1 必须是 42」逼出来的。
        const { probeError, consoleError, memoryMapError } = await loadProjectIntoKernel(
            {
                commandRouter,
                registerMemory,
                waveform,
                debug,
                console: services.console,
                memoryMap: services.memoryMap,
                // ⚠️ **器件中间层**（STAGE-43 · v0.66）⇒ ⇒ **必须**在这儿
                // 一起交进去 ⇒ ⇒ 否则 `resubscribeAllStreams` **看不到**它
                // ⇒ ⇒ 那两条流**永远不订阅** ⇒ ⇒ 产品视图那两张卡**永远是
                // 空态**（⚠️ 这正是真机门禁第一次跑就抓到的那个坑）。
                devices: services.devices,
                // ⚠️ **模板清单流**（v0.76 · STAGE-52）—— 同形：装载 / 卸载
                // 清掉内核主题集合 ⇒ `resubscribeAllStreams` 看不到它就不重订
                // ⇒ 模板库清单**永远停在空态**（r12/r13 真机：30s 等不到）。
                templates: services.templates,
            },
            project,
        );
        await commandRouter.dispatchCommand(Command.nop());
        if (probeError) {
            // ⚠️ **内核是好的**——工程装好了、寄存器流也重订了、连 Nop
            // 往返都成功了。只是**探针流**订阅不上（波形视图会是空的）。
            //
            // 这里**不能**说「内核未就绪」——那会把排查引到错误的方向
            // （D-46 检视 V-B）。
            setNote(`内核已就绪，但探针流不可用：${probeError?.message ?? probeError}`);
            return;
        }
        if (consoleError) {
            // 同理：console 流订阅不上 ⇒ 终端永远是空的，但内核是好的。
            setNote(`内核已就绪，但 console 流不可用：${consoleError?.message ?? consoleError}`);
            return;
        }
        if (memoryMapError) {
            // 同理：内存图流订阅不上 ⇒ 内存映射表是空的，但内核是好的。
            setNote(`内核已就绪，但内存图流不可用：${memoryMapError?.message ?? memoryMapError}`);
            return;
        }
        setNote("WASM 内核已就绪（Nop 往返成功）");
    } catch (err) {
        setNote(`内核未就绪：${err?.message ?? err}`);
    }
}

/**
 * 装载工程，并把**内核侧订阅**都强制重订。
 *
 * 四步绑在一个函数里（而不是散落在调用点的「记得再订阅一次」注释里）：
 * 调用点只管调它，不会有人忘记后两步。
 *
 * 1. `waveform.reset()` —— 新工程 = 新会话，上一个会话的探针通道
 *    不该继续挂在视图上；
 * 2. `console.reset()` —— 新程序 = 新 console，上一个程序打到串口上的
 *    字节不该留在屏幕上（与 D-45 同一个坑：状态必须跟着工程一起重建）；
 * 3. `registerMemory.loadProject()` —— 装载工程 + 强制重订寄存器流
 *    （D-45 的教训：内核侧的订阅会随工程一起失效）；
 * 4. `resubscribeAllStreams()` —— 强制重订其余的**流**订阅。同样的道理：
 *    `LoadProject` 把内核的主题集合清空了，探针流 / console 流 / 内存图流
 *    不重订就**静默停掉**。
 *
 * ⚠️ **第 4 步是助手**（v0.31 · 裁决 F）⇒ **装载**与**卸载**共用同一套
 * ⇒ 卸载路径不会漏掉任何一条流。
 *
 * ⚠️ **`console` 接线是必需的**（不是可选参数）：漏传它 ⇒ console 流
 * 永远订阅不上 ⇒ 终端**静默**是空的，而页面上看不出任何异常。宁可
 * 抛错也不要静默。
 *
 * @param {{commandRouter: object, registerMemory: object, waveform: object, debug?: object, console: object, memoryMap?: object}} services
 * @param {object} project — `model::Project` shaped value
 * @returns {Promise<{probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>} —
 *   `probeError` 非空表示**探针流**重订失败；`consoleError` 非空表示
 *   **console 流**重订失败；`memoryMapError` 非空表示**内存图流**重订失败。
 *   **工程已经装载、寄存器流已经重订**（前 3 步成功）
 *
 * ⚠️ **第 4 步失败不许牵连前 3 步**（D-46 检视 V-B）。几步串在一起时，
 * 第 4 步抛错会让整个 `loadProjectIntoKernel` 失败——而前 3 步**已经
 * 成功了**：工程装好了、寄存器流也重订了。调用方若因此判定「内核未就绪」，
 * 就会连带跳过 Nop 往返、产品树渲染，提示语还会**误导**（说内核没就绪，
 * 其实内核好得很，只是探针流订阅不上）。
 *
 * 所以第 4 步**各自单独**兜住，失败作为 `probeError` / `consoleError` 返回
 * ——**不静默**：调用点负责把它显示出来。
 *
 * ⚠️ **两条流各自兜住**（v0.23 · console）：探针流订阅不上**不该**牵连
 * console 流（反之亦然）——它们是两条独立的流。
 *
 * ⚠️ **导出是为了让它可测**（D-46 检视 V-B）。此前它是**私有**函数，
 * 前端门禁里**一条用例都碰不到它**——把第 3 步（探针流重订）删掉，
 * `tools/check-front.sh` 仍然 **152/152 全绿**：D-46 最核心的不变量
 * 「`LoadProject` 后探针流被强制重订」**根本没有被守**。
 * 与 D-45 同一个坑——「我记得要重订」写在注释里，不如用一条用例钉住。
 */
export async function loadProjectIntoKernel(services, project, options = {}) {
    const { registerMemory, waveform, debug, console: consoleBridge } = services;
    if (!consoleBridge) {
        // ⚠️ **不静默**：console 接线漏了 ⇒ 终端永远是空的，而页面上看不出
        // 任何异常 ⇒ 这是最难查的一类故障。宁可当场抛错。
        throw new Error(
            "loadProjectIntoKernel 缺少 console 接线：console 流必须被强制重订" +
            "（LoadProject 清掉了内核的主题集合）",
        );
    }
    // ⚠️ **记下当前工程** ⇒ 「重新装载工程」按钮能再发一次同一个工程
    // （板级配置是**待生效**的 ⇒ 宿主在下一次 `LoadProject` 时才应用）。
    //
    // ⚠️ **v0.88 · `remember: false`**（用户拍板方案 B）：产品视图「配板」
    //   用的是一次 `images: []` 的空装载 —— 那是**为了让板生效**的内部装载，
    //   **不是**用户装了新程序 ⇒ ⇒ 它**不许**把「用户当前工程」的记忆覆盖成
    //   一份空工程。
    //
    //   ⚠️ 覆盖的后果（真机 + 门禁双重现场）：配完板再点「重新装载工程」⇒
    //   重发的是一份 `images: []` ⇒ **既不会重装固件、也不报任何错**
    //   ⇒ 按钮形同虚设；同时「板已应用 + 现有固件」这条装载路径被抹掉
    //   ⇒ ISA 门禁（`E_PLT-20/21`）在重装这一步**永远走不到**。
    if (options.remember !== false) setLoadedProject(project);
    // ⚠️ **新工程 = 新程序 ⇒ 运行态归零**（v0.63 批）⇒ ⇒ 顶栏那个按钮回到
    //   「运行」（用户拍板的那条语义：「**当加载之后顶栏的运行应该显示运行**」）
    //   ⇒ ⇒ 「跑过没有」这一格也归零 ⇒ ⇒ 不是上一段程序的历史。
    setRunState({ ...initialRunState(), started: false });
    waveform.reset();
    // ⚠️ 新工程 = 新程序 ⇒ **上一个程序的反汇编**留在面板上就是错的
    // （同一个 D-45 的坑：状态必须跟着工程一起重建）。
    // `debug` 是可选的——不是每个调用点都装了调试面。
    debug?.reset?.();
    // ⚠️ 新程序 ⇒ 上一个程序的 console 字节不算数（同一个坑）。
    consoleBridge.reset();
    // ⚠️ 器件实例状态**跟着工程重建**（STAGE-43 · v0.66 · 同一个坑）⇒ ⇒
    // 上一块板的实例清单不算数。
    services.devices?.reset?.();
    // ⚠️ 模板清单**跟着工程重建**（v0.76 · STAGE-52 · 同一个坑）⇒ ⇒
    // 上一会话的清单不算数。
    services.templates?.reset?.();
    // ⚠️ **D-178（v0.72 批）**：装载**失败**时**也要重订一次流** ⇒ ⇒
    // 否则前端**学不到**内核顶上去的修订号。
    //
    // ⚠️ **根因链**：内核在「挑平台描述」那一步就会把**待生效的板应用**
    // （修订号 **+1**）⇒ ⇒ 那之后才失败（写不进总线 / 符号索引重建失败）
    // ⇒ ⇒ 内核**已经**是新修订号 ⇒ ⇒ 而**失败路径不回 `memory.map`** ⇒ ⇒
    // 前端**唯一**能学到新修订号的途径是「重订 ⇒ 内核强推一次」⇒ ⇒
    // 不重订 ⇒ ⇒ 前端停在旧修订号 ⇒ ⇒ **下一次改板报 `E_PLT-13`**
    // ⇒ ⇒ 板换不回去、零提示。
    //
    // ⚠️ **失败也要重订**（不是「成功才重订」）⇒ ⇒ **重订在 catch 里**也
    // 跑一遍 ⇒ ⇒ 原错误**原样抛出**（不改调用点现有的错误语义）⇒ ⇒
    // 「装载失败」这件事**照旧**被看见。
    //
    // ⚠️ **重订**失败**不掩盖**装载错误 ⇒ ⇒ 各流**各自**兜住自己的错误
    // （`resubscribeAllStreams` 内部就是那样做的）⇒ ⇒ 这里**不**追加。
    let loadError = null;
    try {
        await registerMemory.loadProject(project);
    } catch (err) {
        loadError = err;
    }
    // ⚠️ **重订部分抽成助手**（裁决 F · v0.31）⇒ 装载与卸载**共用**同一套
    // ⇒ **不写第二套**。
    //
    // ⚠️ `skipRegisters` = 真：M-10 的 `loadProject()` **自己**就强制重订
    // 了寄存器流（第 3 步）⇒ 这里不再重复发一次。
    const errors = await resubscribeAllStreams(services, { skipRegisters: true });
    // ⚠️ **原错误原样抛出** ⇒ ⇒ 调用点的错误语义**一字未变**。
    if (loadError) {
        throw loadError;
    }
    // ⚠️ **装载成功 ⇒ 上一会话的最后一条错误不再代表当前状态**。
    //
    // 空产品启动时会先 `LoadProject` 撞一次 `E_PLT-22`（无生效板，这是
    // v0.76 的预期拒绝）⇒ 全局 `error` 订阅把那条写进了 `debug.error`
    // ⇒ 之后从模版装板成功、甚至 `debug.reset()` 也按「保留内核最后一份
    // 报告」的规矩不清它 ⇒ 调试视图会一直挂「没有生效板」的假状态。
    // 错误位只在**这一次装载失败**时由 `loadError` 抛出并保留；成功
    // 就把它收掉。
    viewStore.setState("debug", (prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), error: null },
    }));
    publishViewState("debug");
    return {
        probeError: errors.probeError,
        consoleError: errors.consoleError,
        memoryMapError: errors.memoryMapError,
        // ⚠️ 器件中间层（STAGE-43 · v0.66）—— 与其它流**同形**地报出来。
        deviceError: errors.deviceError,
        // ⚠️ 模板清单流（v0.76 · STAGE-52）—— 同形报出来。
        templateError: errors.templateError,
    };
}

/**
 * 强制重订**全部流**（STAGE-16 · v0.31 · 裁决 F）。
 *
 * ⚠️ **为什么必须是「强制」**：内核侧的订阅会随**会话**一起失效
 * （`LoadProject` 与 `UnloadProject` 都会清掉内核的主题集合 ⇒ 新会话
 * = 新订阅）⇒ 而前端的「已订阅」标志可能还立着 ⇒ 不重订就**静默停流**
 * （台账 D-45 / D-46 的同一个坑）。
 *
 * ⚠️ **要重订的四条流**（前端的**事件流**）：
 *   - 状态增量流 `sim.state.delta`（M-10 的寄存器/PC 生产载体；
 *     `sim.registers` 的波形流只是兼容输入，不再订阅）；
 *   - 探针波形流 `waveform.history`（内核侧主题是 `sim.waveform`）；
 *   - console 流 `sim.console`；
 *   - 内存图流 `memory.map`。
 *
 * ⚠️ **每一条各自兜住**：一条流订阅不上**不该**牵连其它流 ⇒ 失败作为
 * `registerError` / `probeError` / `consoleError` / `memoryMapError`
 * 返回 ⇒ **不静默**：调用点负责把它显示出来。
 *
 * ⚠️ **装载与卸载共用这一套**（裁决 F）⇒ 卸载之后不会漏掉任何一条流。
 *
 * @param {{registerMemory?: object, waveform: object, console?: object, memoryMap?: object}} services
 * @param {{skipRegisters?: boolean}} [options] — `skipRegisters` = 真时**不**
 *   重订寄存器流（调用点自己已经重订过了）
 * @returns {Promise<{registerError: Error|null, probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null}>}
 */
export async function resubscribeAllStreams(services, options = {}) {
    const { skipRegisters = false } = options;
    /** @type {{registerError: Error|null, probeError: Error|null, consoleError: Error|null, memoryMapError: Error|null, deviceError: Error|null}} */
    const errors = {
        registerError: null,
        probeError: null,
        consoleError: null,
        memoryMapError: null,
        // ⚠️ 器件中间层的两条流（STAGE-43 · v0.66）⇒ ⇒ **各自兜住**（不牵连别的流）。
        deviceError: null,
        // ⚠️ 模板清单流（v0.76 · STAGE-52 · 裁决 M）—— 同形兜住。
        templateError: null,
    };
    if (!skipRegisters) {
        try {
            // ⚠️ **寄存器流 + 状态增量流**都归 M-10 ⇒ 它的 `resubscribe()`
            // 是这两条流的**唯一**重订入口。
            await services.registerMemory?.resubscribe?.();
        } catch (err) {
            errors.registerError = err;
        }
    }
    try {
        await services.waveform.subscribeProbeStream();
    } catch (err) {
        errors.probeError = err;
    }
    try {
        await services.console.subscribeConsoleStream();
    } catch (err) {
        errors.consoleError = err;
    }
    try {
        await services.memoryMap?.subscribeMemoryMapStream?.();
    } catch (err) {
        errors.memoryMapError = err;
    }
    // ⚠️ **器件中间层**（STAGE-43 · v0.66）—— 与内存图流**同形**：装载 /
    // 卸载都会清掉内核的主题集合 ⇒ ⇒ 不重订就**静默**停在旧数据上。
    try {
        await services.devices?.subscribeDeviceStreams?.();
    } catch (err) {
        errors.deviceError = err;
    }
    // ⚠️ 模板清单流（v0.76 · STAGE-52）—— 装载 / 卸载清掉内核主题集合 ⇒
    // 不重订就**静默**停在旧清单上（与 device.types 同一个坑）。
    try {
        await services.templates?.subscribeTemplateStreams?.();
    } catch (err) {
        errors.templateError = err;
    }
    return errors;
}
