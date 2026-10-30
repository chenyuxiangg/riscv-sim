// 固件镜像加载器（STAGE-32 · v0.47 批）。
//
// ⚠️ **为什么要有它**：契约层与内核侧**早就够了**——契约的
// `ImageSource::Bytes` 能表达「浏览器取字节、内核不联网」，宿主按
// `ImageKind` 分派到 M-06 的 ELF / RAW 解析⇒ ⇒ 缺的**只是前端的一个
// 入口**⇒ ⇒ 本模块**只**做「把用户选的文件变成一个工程」。
//
// ⚠️ **本模块是纯逻辑**（`front/views/components/` 的一贯做法）：
//   - `imageKindFor(name)` —— 扩展名 ⇒ 契约层的 `ImageKind`（两个变体）
//   - `buildImageProjectFromBytes(bytes, name, options)` —— 造工程
//   - `renderImageLoadStatus(state)` —— 渲染**当前固件的状态块**（产品视图
//     用；⛔ **不含入口** ⇒ ⇒ 入口只有页头那一处）
//   - `renderImageLoaderControl(options)` —— 渲染**控件本体**（**唯一入口**
//     在页头 ⇒ ⇒ `accept` 列表与按钮语义**只有一处**）
//   - `readImageFile(file)` —— 读字节（浏览器原生 `File.arrayBuffer()`）
//   - `pendingImage()` / `loadImageFileIntoPending(file)` —— 待加载镜像
//   - `imageLoadDecision(state)` —— **点「加载固件」时该怎么办**的纯判定
//   - `describeImageLoadShort(state)` —— 短版状态说明（页头那条窄状态条）
//
// ⚠️ **v0.58 批：入口收敛成**一个**（用户定案「只应该有一个入口」）**
// ⇒ ⇒ 原来有两个（页头「加载固件」+ 产品视图面板里的「加载」）⇒ ⇒ 两个入口
// 共用**同一份** `pending` ⇒ ⇒ 用户在**一处**装完、到**另一处**再点 ⇒ ⇒
// 「没有待加载的文件」⇒ ⇒ 而那次装载**其实成功了** ⇒ ⇒ 界面在骗人。
// ⇒ ⇒ 现在：**产品视图那块面板降级成纯状态显示**（不再有入口）⇒ ⇒
// **入口只有页头那一个**。
//
// ⚠️ **「装完就停」是用户的定案**（规格 §3 裁决 D）⇒ ⇒ 本模块**不**发任何
// 「继续」命令⇒ ⇒ 跑不跑由用户自己点。
//
// ⚠️ **镜像类型只认契约层有的两个变体**（`Elf` / `RawHex`）⇒ ⇒ M-06 那
// 第三种 IHEX（`Kind::Hex`）**在契约层表达不了**⇒ ⇒ 本模块**不假装支持**
// （规格 §8 订正 1）。

import { escapeHtml } from "../../lib/escape.js";
import * as viewStore from "../m03_viewstore.js";

/**
 * 契约层 `ImageKind` 的**两个**变体（`crates/contract/src/model.rs` 的
 * `enum ImageKind { Elf, RawHex }`）。
 *
 * ⚠️ **前端镜像不另立一份常量表**——但这两个是**契约的字面量**（serde
 * 外部标签），前端必须写出来⇒ ⇒ 它们只在这一处定义⇒ ⇒ 视图层与服务层
 * 都从这里取。
 */
export const IMAGE_KINDS = Object.freeze({
    Elf: "Elf",
    RawHex: "RawHex",
});

/**
 * 加载入口的**位置标记**（v0.58 批起**只有一个入口** ⇒ ⇒ 只剩页头这一处）。
 *
 * ⚠️ **为什么还留着这个标记**：真机门禁按**位置**取元素 ⇒ ⇒ 标记是门禁
 * 选择器的**锚点**⇒ ⇒ 它必须**只写在一处**⇒ ⇒ 这里就是那一处。
 *
 * ⚠️ **v0.58 批之前**这里有**两个**取值（`topbar` / `product`）⇒ ⇒ 两个
 * 入口**各有一个** `<input type="file">` ⇒ ⇒ 门禁用不带位置限定的选择器会
 * **命中页头那个**⇒ ⇒ 断言还在跑、断的却不是它以为的那个东西 ⇒ ⇒ 现在
 * **只有一个**入口 ⇒ ⇒ 「按位置取」这条规矩**仍然留着**（不是历史包袱：
 * 它让门禁的选择器**无歧义**）。
 *
 * @type {string}
 */
export const IMAGE_ENTRY_FROM = "topbar";

/**
 * 文件扩展名 ⇒ 契约层的 `ImageKind`。
 *
 * ⚠️ **口径必须与 M-06 一致**（规格 §3 裁决 F + §8 订正 1）：M-06 的
 * `kind_from_extension` 把 `.elf` / `.out` 判成 ELF，`.hex` / `.ihex`
 * 判成 **IHEX**（第三种）——而**契约层只有两个变体**⇒ ⇒ IHEX **表达不了**
 * ⇒ ⇒ 这里**不发明第三种**⇒ ⇒ `.hex` 落到 `RawHex`（**如实登记为已知
 * 边界**，不假装支持 IHEX）。
 *
 * ⚠️ **未知扩展名 ⇒ `RawHex`**（不是猜 ELF）——与 M-06 的 `detect_kind`
 * 同一原则（`D-21`）：猜 ELF 会让每个无扩展名的文件都报「bad ELF magic」。
 *
 * @param {string} name — 文件名（含扩展名）
 * @returns {"Elf"|"RawHex"}
 */
export function imageKindFor(name) {
    const text = String(name ?? "");
    const dot = text.lastIndexOf(".");
    const ext = dot >= 0 ? text.slice(dot + 1).toLowerCase() : "";
    if (ext === "elf" || ext === "out") return IMAGE_KINDS.Elf;
    return IMAGE_KINDS.RawHex;
}

/**
 * 读一个用户选的文件 ⇒ 字节数组。
 *
 * ⚠️ **用浏览器原生 `File.arrayBuffer()`**（不是 `FileReader`，不是
 * `fetch`）⇒ ⇒ 零依赖 ⇒ ⇒ 也**不需要**任何 HTTP 服务（CORS 与端口都
 * 不存在）⇒ ⇒ 这是「用户点一下就能加载」的关键。
 *
 * ⚠️ **不截断**：`arrayBuffer()` 拿的是**完整**响应体（镜像有几百 KiB：
 * osz 的 ELF 是 282 280 字节）⇒ ⇒ 不能用「读一部分就停」的方式取。
 *
 * @param {File|Blob} file
 * @returns {Promise<number[]>} 字节数组（普通数组 ⇒ 可直接进 JSON）
 */
export async function readImageFile(file) {
    if (!file || typeof file.arrayBuffer !== "function") {
        throw new Error("不是一个可读的文件");
    }
    const buf = await file.arrayBuffer();
    return Array.from(new Uint8Array(buf));
}

/**
 * 用**字节**造一个「只含一个镜像」的工程（`model::Project` 的线上形态）。
 *
 * ⚠️ **与 `buildImageProject` 同形**（`front/app.js`）——那一份按 URL 取
 * 字节；这一份**字节已经在手上**⇒ ⇒ 本模块**不**重复实现「取字节」那一段。
 *
 * ⚠️ **`source` 是外部标签枚举**：`model::ImageSource::Bytes(Vec<u8>)` 的
 * serde 形态是 `{"Bytes":[...]}`（**不是** base64、**不是**字符串）。
 *
 * ⚠️ **`isa` 必须是 `"RV32IMC"`**：契约层的 `ISA` 常量，别的值会被内核判
 * 「工程 ISA 与内核支持的不一致」。
 *
 * @param {number[]} bytes — 镜像字节
 * @param {string} name — 文件名（进工程名 ⇒ 装完能看出装的是哪个）
 * @param {{id?: string, projectName?: string, kind?: string, boardName?: string, toolchain?: string}} [options]
 * @returns {object} 一个 `model::Project` 形状的值
 */
export function buildImageProjectFromBytes(bytes, name, options = {}) {
    const fileName = String(name ?? "").trim() || "image.bin";
    const kind = options.kind ?? imageKindFor(fileName);
    return {
        id: options.id ?? "proj-user-image",
        name: options.projectName ?? fileName,
        tree: {
            root: {
                name: options.boardName ?? "MainBoard",
                component_id: null,
                children: [],
            },
        },
        images: [{
            name: fileName,
            kind,
            source: { Bytes: Array.from(bytes ?? []) },
        }],
        version: {
            schema: 1,
            isa: "RV32IMC",
            toolchain: options.toolchain ?? "user file (browser)",
        },
    };
}

/**
 * 加载状态的**一句话说明**（三种状态分得开）。
 *
 * ⚠️ **「没加载过」与「加载失败」必须分得开**——两者都读起来像「没有
 * 镜像」⇒ ⇒ 用户会以为自己没选文件。
 *
 * @param {{status?: string, name?: string|null, byteLength?: number|null, error?: string|null}} [state]
 * @returns {string}
 */
export function describeImageLoad(state) {
    const status = String(state?.status ?? "idle");
    if (status === "reading") return "正在读取固件……（读完才能点「加载固件」）";
    if (status === "loading") return "正在读取并装载固件…";
    if (status === "error") {
        const msg = String(state?.error ?? "未知原因");
        return `装载失败：${msg}`;
    }
    if (status === "ok") {
        const name = String(state?.name ?? "固件");
        const bytes = Number(state?.byteLength ?? 0);
        return `已装载「${name}」（${bytes} 字节）—— 点「继续」开始运行。`;
    }
    // ⚠️ **「选好了但还没装」与「什么都没选」必须分得开**（施工期订正 4）：
    // 两者都是 `idle` ⇒ 只看状态字面量分不出来 ⇒ ⇒ 用「有没有文件名」分开
    // ⇒ ⇒ 用户选完文件能**看见**自己选了什么（可操作性），真机门禁也
    // 有一个「字节真的读到了」的观测点。
    if (state?.name) {
        const name = String(state.name);
        const bytes = Number(state.byteLength ?? 0);
        return `已选择「${name}」（${bytes} 字节）—— 点「加载固件」装进内核。`;
    }
    return "尚未装载自己的固件——启动时装的是内建 demo 固件（40 字节）。";
}

/**
 * 写「镜像加载」这块视图状态。
 *
 * ⚠️ **它住在产品视图的状态里**（不进契约、不进事件）⇒ ⇒ 刷新即失
 * （与规格 §1.1 一致）。
 *
 * ⚠️ **视图层与 app 层共用这两个函数**（视图层写「读好了」，app 层写
 * 「装好了 / 装失败」）⇒ ⇒ 它们**住在这里**（这块功能的归属地）⇒ ⇒
 * **不写第二份**。
 *
 * @param {{status?: string, name?: string|null, byteLength?: number|null, error?: string|null}} patch
 */
export function setImageLoadState(patch) {
    viewStore.setState("product", (prev) => ({
        ...prev,
        imageLoad: { ...(prev?.imageLoad ?? {}), ...patch },
    }));
}

/**
 * 让产品视图按当前状态重渲染一次。
 *
 * ⚠️ 走的是 **M-01 的 `riscv:render` 事件**（与 app 层那几处同一套路）
 * ⇒ ⇒ 视图层不直接摸 DOM。
 */
export function publishProductState() {
    window.dispatchEvent(new CustomEvent("riscv:render", {
        detail: {
            viewId: "product",
            type: "update",
            state: viewStore.getState("product"),
        },
    }));
}

/**
 * **待加载的镜像**（字节 + 文件名 + 类型）。
 *
 * ⚠️ **住在视图状态里**（`product` 视图的 `imageLoad.pending`）⇒ ⇒
 * **单一真相** ⇒ ⇒ 视图状态一重置（`m03.reset()` / 换工作台）它就没了
 * ⇒ ⇒ **不会**出现「上一个会话的字节还挂着」那种状态。
 *
 * ⚠️ **为什么必须先存**：视图层只能发信号（`emitCommand`），而命令的载荷
 * **在信号里**⇒ ⇒ 字节必须先**存**在某处才能在点击「加载」时取到。
 *
 * ⚠️ **为什么不是「点击时读文件」**：点击时 `input.files` 已经是空的了
 * （浏览器把文件交给 `change` 事件后就清掉了）⇒ ⇒ 必须在 `change` 里
 * 读。
 *
 * ⚠️ **STAGE-33：这段从视图层搬到了这里**——页头那个入口在 **app 层**
 * （页头不属于任何视图面板 ⇒ ⇒ 视图层管不到）⇒ ⇒ 两边共用**同一段**实现
 * ⇒ ⇒ **不写第二套**（规格 §3 裁决 D）。
 *
 * @returns {{bytes: number[], name: string, kind: string}|null}
 */
export function pendingImage() {
    return viewStore.getState("product")?.imageLoad?.pending ?? null;
}

/**
 * 读用户选的文件 ⇒ 存成「待加载的镜像」。
 *
 * ⚠️ **读失败如实显示**（规格 §3 裁决 J）⇒ ⇒ 不静默。
 *
 * ⚠️ **两个入口共用这一段**（页头那个入口与产品视图那个入口）⇒ ⇒ 读字节
 * 与存 pending 的行为**必然一致**。
 *
 * @param {File|Blob} file
 * @returns {Promise<void>}
 */
export async function loadImageFileIntoPending(file) {
    // ⚠️⚠️ **先写「读取中」**（v0.59 批 · 施工期实测撞到）⇒ ⇒ **这是竞态，
    // 必须在**第一个 `await` **之前**写。
    //
    // ⚠️ **竞态的机制**：读字节是**异步**的（`await file.arrayBuffer()`）⇒ ⇒
    // 在字节到手之前，状态里**还留着上一次**的值（`status` 仍是 `"ok"`）⇒ ⇒
    // 用户在这期间点「加载固件」⇒ ⇒ `imageLoadDecision` 判成「**已装载**」⇒ ⇒
    // **这一下被吞掉**（界面还如实说「已装载，无需重复加载」）⇒ ⇒ **界面在
    // 骗人**（用户明明刚选了新文件，界面说不用装）。
    //
    // ⚠️ **门禁实测复现**：真机门禁「选文件 ⇒ 立刻点加载」⇒ ⇒ 偶发「装载没
    // 发生」（PC 没回去）⇒ ⇒ 根因就是这个竞态 ⇒ ⇒ **不是门禁的时序问题，是
    // 产品缺陷** ⇒ ⇒ 所以**产品侧要修**（⛔ 不许只在门禁侧绕过去 ⇒ ⇒ 那是
    // 把缺陷原样留给用户）。
    //
    // ⚠️ **为什么名字与字节数也可以先写**：这两个是**同步**就知道的（`File`
    // 的 `name` / `size`）⇒ ⇒ 先写出来只是让界面**早一点**显示「在读哪个」
    // ⇒ ⇒ `pending`（真字节）仍然要等读完了才有。
    setImageLoadState({
        status: "reading",
        name: String(file?.name ?? "image.bin"),
        byteLength: Number(file?.size ?? 0),
        error: null,
    });
    publishProductState();
    try {
        const bytes = await readImageFile(file);
        const name = String(file?.name ?? "image.bin");
        setImageLoadState({
            status: "idle",
            name,
            byteLength: bytes.length,
            error: null,
            pending: { bytes, name, kind: imageKindFor(name) },
        });
        publishProductState();
    } catch (err) {
        setImageLoadState({
            status: "error",
            error: `读取文件失败：${err?.message ?? err}`,
            pending: null,
        });
        publishProductState();
    }
}

/**
 * 加载状态的**一句话说明**（短版 · 给页头那条窄状态条用）。
 *
 * ⚠️ **与 `describeImageLoad` 同源**（都读同一份 `state`）⇒ ⇒ 不是第二个
 * 真相 ⇒ ⇒ 只是**长短**不同。
 *
 * @param {{status?: string, name?: string|null, byteLength?: number|null, error?: string|null}} [state]
 * @returns {string}
 */
export function describeImageLoadShort(state) {
    const status = String(state?.status ?? "idle");
    if (status === "reading") return "正在读取固件…";
    if (status === "loading") return "装载中…";
    if (status === "error") return `装载失败：${String(state?.error ?? "未知原因")}`;
    if (status === "ok") return `已装载「${String(state?.name ?? "固件")}」`;
    if (state?.name) {
        return `已选「${String(state.name)}」（${Number(state.byteLength ?? 0)} 字节）`;
    }
    return "未选固件";
}

/**
 * 点「加载固件」时**该怎么办**的**纯判定**（v0.58 批）。
 *
 * ⚠️ **为什么要有它**：这一段是**用户报的缺陷**的落点——装完一次之后再点
 * 一次，界面报「装载失败：没有待加载的文件」⇒ ⇒ 而**上一次其实成功
 * 了**⇒ ⇒ 界面在骗人 ⇒ ⇒ 根因是「装完就把 `pending` 清掉」+「按钮一直
 * 可点」⇒ ⇒ 于是**第二次点击**落进了「没有文件」那一格。
 *
 * ⚠️ **四格必须分得开**（每格一个**不同的**后果）：
 *   - `loading` —— **正在装** ⇒ ⇒ **忽略**这次点击（幂等 ⇒ ⇒ 连点两下
 *     **不会**发两条 `LoadProject`）
 *   - `load` —— 有待加载的文件 ⇒ ⇒ 装它
 *   - `already-loaded` —— **上次装成功了** ⇒ ⇒ **不报失败**（如实说
 *     「已装载，无需重复加载」）⇒ ⇒ ⛔ **不许**把它当成「没有文件」
 *   - `no-file` —— **真的**没选过文件 ⇒ ⇒ 报错（⛔ 不静默）
 *
 * ⚠️ **判定顺序是硬约束**：`loading` **先**判 ⇒ ⇒ 否则连点两下时第二下会
 * 看到「还有 `pending`」（成功前它一直在）⇒ ⇒ 于是**发第二条**
 * `LoadProject` ⇒ ⇒ 内核被连着换两次工程。
 *
 * @param {{status?: string, name?: string|null, pending?: object|null}} [state]
 * @returns {{kind: "loading"|"load"|"already-loaded"|"no-file", pending?: object, name?: string}}
 */
export function imageLoadDecision(state) {
    const status = String(state?.status ?? "idle");
    if (status === "loading") return { kind: "loading" };
    // ⚠️ **读取中**（v0.59 批）⇒ ⇒ 字节**还在读**⇒ ⇒ ⛔ **不许**把它判成
    // 「已装载」⇒ ⇒ 否则用户刚选的新文件那一下**被吞掉**⇒ ⇒ 界面在骗人。
    //
    // ⚠️ **必须排在 `pending` 那一格**之前：上一次装载**失败**时 `pending`
    // 是**保留**的（用户点一下就能重试）⇒ ⇒ 读取期间它还是**旧的**那份 ⇒ ⇒
    // 先判 `pending` 就会拿**旧字节**去装。
    if (status === "reading") return { kind: "reading" };
    const pending = state?.pending ?? null;
    if (pending) return { kind: "load", pending };
    if (status === "ok") {
        return { kind: "already-loaded", name: String(state?.name ?? "固件") };
    }
    return { kind: "no-file" };
}

/**
 * 渲染加载入口的**控件本体**（文件选择器 + 按钮 [+ 一条短状态]）。
 *
 * ⚠️ **v0.58 批：这是**唯一**那个入口**（页头）⇒ ⇒ `accept` 列表、按钮
 * 语义、`data-image-load-from` 的取值**都只有一处**⇒ ⇒ **不写第二套**。
 *
 * ⚠️ **按钮文案**：**「加载固件」**（与用户要找的字逐字一致）⇒ ⇒ 页头那条
 * 状态条紧挨着它 ⇒ ⇒ 装载结果**就在按钮旁边**看得见。
 *
 * @param {{label?: string, buttonText?: string, withBrief?: boolean}} [options]
 * @returns {string}
 */
export function renderImageLoaderControl(options = {}) {
    const label = options.label ?? "选择固件镜像";
    const buttonText = options.buttonText ?? "加载固件";
    const brief = options.withBrief
        ? `<span class="muted image-loader-brief" data-image-brief data-image-brief-status="idle">未选固件</span>`
        : "";
    return `
        <span class="image-loader-control">
            <label class="source-input">
                ${escapeHtml(label)}
                <input type="file" accept=".elf,.out,.hex,.ihex,.bin,.raw"
                       data-image-input data-image-load-from="${escapeHtml(IMAGE_ENTRY_FROM)}">
            </label>
            <button type="button" data-action="load-image"
                    data-image-load-from="${escapeHtml(IMAGE_ENTRY_FROM)}">${escapeHtml(buttonText)}</button>
            ${brief}
        </span>
    `;
}

/**
 * 渲染**当前固件的状态块**（产品视图用）。
 *
 * ⚠️ **v0.58 批：这里**不再有入口**⇒ ⇒ 只剩「现在装的是哪个固件」这个
 * **状态**⇒ ⇒ 入口**只有页头那一个**⇒ ⇒ 用户在产品视图里看到的是
 * 「当前固件是什么 / 上次装载成没成」，**不是**第二个加载按钮。
 *
 * ⚠️ **状态写在面板上**（不只在页脚）——规格 §8 订正 3：失败必须**在界面
 * 上看得见**⇒ ⇒ 这里有一块带 `data-image-load-status` 的区域。
 *
 * ⚠️ **机器可读的标记**：真机门禁按**数据属性**取值，不靠文案。
 *
 * @param {{status?: string, name?: string|null, byteLength?: number|null, error?: string|null}} [state]
 * @returns {string}
 */
export function renderImageLoadStatus(state) {
    const status = String(state?.status ?? "idle");
    const text = describeImageLoad(state);
    const errorBlock = status === "error"
        ? `<p class="image-load-error" data-image-load-error="1">${escapeHtml(text)}</p>`
        : "";
    return `
        <section class="image-loader">
            <h3>当前固件</h3>
            <p class="muted" data-image-load-status="${escapeHtml(status)}">${escapeHtml(text)}</p>
            ${errorBlock}
            <p class="muted">
                ⚠️ **加载入口只有页头那一个**（「加载固件」）⇒ ⇒ 在**任何视图**
                都能装载；支持 <code>.elf</code> / <code>.out</code>（按 ELF 解析）
                与 <code>.hex</code> / <code>.bin</code> / <code>.raw</code>
                （按原始字节解析）⇒ ⇒ 文件在**浏览器里**读成字节后交给内核
                ⇒ ⇒ **不需要**任何 HTTP 服务；⚠️ 装完**不自动跑**⇒ ⇒ 请自己
                点「继续」。
            </p>
        </section>
    `;
}
