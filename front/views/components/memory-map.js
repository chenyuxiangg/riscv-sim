// M-02 · 板级内存图（STAGE-14 · v0.29）的**纯逻辑**。
//
// 这个模块**只**导出纯函数与纯渲染：不碰服务层、不发命令 ⇒
// `front/tests/memory-map.test.mjs`（node:test）可以直接测它。
//
// # 真相源：内核的 `memory.map` 事件
//
// 描述 DTO（`PlatformDescription`）是**契约层已有**的 DTO ⇒ 零新 DTO。
// 事件载荷里的 `description` 是**当前生效**的图（裁决 D）⇒ 前端内存映射表
// 显示的就是它。
//
// # 线上形状**不是** Rust 枚举的默认形状
//
// `crates/contract/src/platform.rs` 用了 serde 的**内部标签**：
//
//   区域：`{"kind":"ram","base":…,"size":…}`
//         `{"kind":"rom","base":…,"size":…}`
//         `{"kind":"peripheral","base":…,"device":{…}}`
//   器件：`{"device":"gpio","id":"GPIO0"}`
//         `{"device":"uart16550","id":"UART0","irq_line":10,"baud_rate":115200}`
//         `{"device":"clint","id":"CLINT","freq_hz":10000000,"harts":1}`
//         `{"device":"plic","id":"PLIC","sources":32,"contexts":1}`
//
// ⇒ 草稿**直接用这个形状** ⇒ 提交时不需要任何形状转换（转换就是第二个真相
// 的来源）。
//
// # ⚠️ 外设窗口**没有** `size`
//
// `RegionSpec::Peripheral` **故意**没有 `size` 字段（窗口大小由器件的寄存器
// 映射自报 —— 写进描述就会多出一份会过期的真相）⇒ 内存映射表的 size 列
// 对外设行**没有数据源** ⇒ 如实显示「由器件自报」，**绝不编一个数字**，
// 外设行也**不挂** `data-region-size`。
//
// # ⚠️ 校验**不在**前端
//
// 契约层那个唯一校验器（`PlatformDescription::validate` ⇒ E_PLT-01..11）
// 是**内核**与**前端**的同一套判据（裁决 E）⇒ 前端**不自写一份校验**
// ⇒ 编辑器只**呈现**约束（V-7），最终由内核裁决。
//
// # 板档案（STAGE-15 · v0.30 · D-136 / D-140）
//
// 板档案的**存储**在 `front/services/m23_boardstore.js`（服务层，另一条
// lane 的产出）⇒ 本模块**只**做两件纯的事：
//   - 「当前草稿**已被内核接受过**」这个**推导量**（`isDraftAccepted`）；
//   - 板档案区块的**渲染**（`renderArchivePanel`）。
//
// ⚠️ 「已被内核接受」是**推导量**（`acceptedDescription` 与当前草稿
// **逐字段相同**）⇒ 不是标志位 ⇒ 不需要「一改输入就手工清标志」那种
// 失效逻辑（那是第二个真相）。
//
// ⚠️ **落库门槛**（STAGE-15 裁决 B）：只有被内核接受过的描述才允许落库
// ⇒ 库里不会有**永远装不上的板** ⇒ 前端**仍然不写第二份校验器**。

//
// # 器件连线 `wiring`（STAGE-19 · v0.34 · D-125 · 裁决 G）
//
// 「UART 的 RX 中断连到 PLIC 的哪个源」这条约束**进数据**（平台描述里的
// **可选** `wiring` 段）⇒ 换一块板改连线**不用改代码**。
//
// ⚠️ **前端只做「透传」**（裁决 G）：编辑器**不暴露** `wiring` 的 UI ⇒ 「从
// 界面提交的板永远没有 `wiring`」是**已知边界**（不假装界面支持）⇒ 这里
// **只**保证「描述里有 ⇒ 草稿里有 ⇒ 提交回去还在」。
//
// ⚠️ **逐字段重建**（裁决 G）：`from.device` / `from.event` / `to.device` /
// `to.source` **逐个重建** ⇒ 草稿里多出来的键**不可能**漏进契约 ⇒ 「逐
// 字段重建」这条纪律**不许破**。
//
// ⚠️ **形状**：`[{ from: { device, event }, to: { device, source } }]`（契约
// 层 `WiringSpec` / `WiringFrom` / `WiringTo`，规格 §2.1 钉死名字）⇒ 草稿
// 里就按这个形状存（**数组**，不是对象）。
//
// ⚠️ **空 / 缺省 ⇒ 线上描述**不带** `wiring` 键**（不是 `null`、不是空数组）
// ⇒ 与契约层 serde 的 `skip_serializing_if = "Option::is_none"` **同形** ⇒
// 没有 `wiring` 的板往返后**仍然**没有 `wiring`（不给所有板都塞一个空数组）
// ⇒ 内核回传的那份与前端提交的那份**逐字段相同** ⇒ 「已被内核接受」这个
// 推导量**不会**因为多一个 `wiring: null` 而永远为假。
//
//
// # 地址比例条（STAGE-35 · v0.50 批 · 裁决 D）
//
// 视图正文改成**卡片形态**（`.cards` / `.card` / `.banner` / `.empty` /
// `.kpi` / `.chip` / `.legend` —— 全部**复用** `front/style.css` 里已有的类，
// **不新造类名**）⇒ ⇒ 段表上方多一条**地址比例条**。
//
// ⚠️ **每段的宽度按它的真实地址区间算**（`size / 整图跨度`）⇒ ⇒ 那是**真数据**
// 的投影 ⇒ ⇒ ⛔ **不用固定百分比** ⇒ ⇒ 比例写进 `style="flex: <比例>"`（那是
// `flex-grow` ⇒ ⇒ 各段的**相对宽度**正好是地址长度之比）。
//
// ⚠️ **稳定钩子一个字符都不动**（裁决 A）⇒ ⇒ `data-mmap-*` / `data-action` /
// 段表的**行结构**与**字段名**全部原样 ⇒ ⇒ 真机门禁与前端用例的**取法**
// 不变。

import { escapeHtml } from "../../lib/escape.js";
import { PLATFORM_SCHEMA_VERSION } from "../../lib/message.js";

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/**
 * 描述格式版本 —— **从契约镜像层 re-export**
 * （`front/lib/message.js` 的 `PLATFORM_SCHEMA_VERSION`，
 *  镜像 `crates/contract/src/platform.rs` 的同名常量）。
 *
 * ⚠️ **这与契约信封的 `schema_version`（现在是 6）是两个不同的东西**：前者管
 * 「内存图描述长什么样」，后者管「消息信封长什么样」。
 *
 * ⚠️ **为什么 re-export 而不是就地定义**（STAGE-15 裁决 K）：本模块的
 * 兄弟模块 `front/services/m23_boardstore.js` 也要用同一个值 ⇒ 两处各定义
 * 一份就是**两个真相**（会漂移）⇒ 常量住在**中立层** `front/lib/` ⇒
 * 服务层与视图层**都从那里 import** ⇒ **一个定义处**，而依赖方向依然单向
 * （两边都只依赖 `front/lib/`，**互不依赖**）。
 */
export { PLATFORM_SCHEMA_VERSION };

/**
 * 契约信封的版本（只用于**文案**提醒，不是描述的版本号）。
 *
 * ⚠️ **v0.31（契约 v7）**：它跟着 `front/lib/message.js` 的
 * `SCHEMA_VERSION` 走 ⇒ 契约层升版时**这里也要跟着改**（否则面板上的
 * 文案会撒谎）。
 *
 * ⚠️ **v0.44（契约 v8）**：管理员通道的**回复侧**（`AdminLogs` /
 * `AdminDiag`）⇒ ⇒ 信封版本 **7 → 8** ⇒ ⇒ 文案跟着改。
 *
 * ⚠️ **v0.57（契约 v9）**：调试器增强批（符号查询 + 条件断点 + 整段转储）
 *   ⇒ ⇒ 信封版本 **8 → 9** ⇒ ⇒ 文案跟着改。
 */
export const CONTRACT_SCHEMA_VERSION = 9;

/** 器件类型（`DeviceSpec` 的四个变体，snake_case 线上名）。 */
export const DEVICE_KINDS = Object.freeze(["gpio", "uart16550", "clint", "plic"]);

/** 区域类型（`RegionSpec` 的三个变体，snake_case 线上名）。 */
export const REGION_KINDS = Object.freeze(["ram", "rom", "peripheral"]);

/**
 * 外设窗口的**摆放长度**（4 KiB）—— 摆放与比例条折算用**同一个**值。
 *
 * ⚠️ **描述里没有外设窗口的 `size`**（`RegionSpec::Peripheral` **故意**没有那个
 * 字段 ⇒ 窗口大小由器件自报）⇒ ⇒ 但比例条**必须**给外设段一个长度
 * （给它 0 ⇒ 那一段在条上**看不见** ⇒ 图例里的「外设」永远不出现）⇒ ⇒
 * 折算成**本模块摆放外设窗口时用的那个窗口**（`nextFreeBase` 里的同一个常量）
 * ⇒ ⇒ **一个落点**。
 *
 * ⚠️ **它不是器件自报的真窗口**（那条**没有数据源**）⇒ ⇒ 界面上**不显示**这个
 * 数字（那会是第二个真相）⇒ ⇒ 它只当**折算用**。
 */
export const PERIPHERAL_WINDOW_BYTES = 0x1000;

/**
 * 每种器件的**默认参数**（新增一个外设窗口时的初值）。
 *
 * ⚠️ 这些数字**只是初值**，不构成任何合法性判据（合法性由内核的校验器
 * 裁决 ⇒ 裁决 E）。
 */
const DEVICE_DEFAULTS = Object.freeze({
    gpio: { id: "GPIO0" },
    uart16550: { id: "UART0", irq_line: 10, baud_rate: 115200 },
    clint: { id: "CLINT", freq_hz: 10000000, harts: 1 },
    plic: { id: "PLIC", sources: 32, contexts: 1 },
});

/** 器件类型 → 显示名。 */
const DEVICE_LABELS = Object.freeze({
    gpio: "GPIO",
    uart16550: "UART16550",
    clint: "CLINT",
    plic: "PLIC",
});

/**
 * 每种器件**可编辑**的参数（除 `id` 之外的）。
 *
 * @type {Readonly<Record<string, ReadonlyArray<{name: string, label: string}>>>}
 */
const DEVICE_PARAMS = Object.freeze({
    gpio: [],
    uart16550: [
        { name: "irq_line", label: "中断号 irq_line" },
        { name: "baud_rate", label: "波特率 baud_rate" },
    ],
    clint: [
        { name: "freq_hz", label: "频率 freq_hz" },
        { name: "harts", label: "核数 harts" },
    ],
    plic: [
        { name: "sources", label: "中断源数 sources" },
        { name: "contexts", label: "上下文数 contexts" },
    ],
});

// ---------------------------------------------------------------------------
// 基础工具
// ---------------------------------------------------------------------------

/**
 * 一个 u32 十六进制串（`0x` + 8 位小写）。
 *
 * @param {unknown} value
 * @returns {string}
 */
export function hex32(value) {
    const n = Number(value ?? 0) >>> 0;
    return `0x${n.toString(16).padStart(8, "0")}`;
}

/**
 * 解析用户填的数（十进制或 `0x` 十六进制）。
 *
 * ⚠️ **解析不出来就返回 `null`**（不是 `NaN`、不是 `0`）：前端**不猜**用户的
 * 意图 ⇒ 非法输入**不写进草稿** ⇒ 提交给内核的一定是合法的数 ⇒ 语义上的
 * 合法性（V-1..V-9）仍由内核的**唯一**校验器裁决（裁决 E）。
 *
 * @param {unknown} raw
 * @returns {number|null}
 */
export function parseNumber(raw) {
    const text = String(raw ?? "").trim();
    if (text === "") return null;
    const n = /^0[xX]/.test(text)
        ? Number.parseInt(text.slice(2), 16)
        : Number(text);
    return Number.isFinite(n) ? n : null;
}

/**
 * 错误码 → **族 + 场景码**。
 *
 * ⚠️ 契约的错误码形状是 `E_<FAMILY>-<SCENARIO>`（族标签见
 * `crates/contract/src/error.rs`）⇒ 判别**族 + 码**（如 `PLT` + `07`），
 * **不靠文案**。
 *
 * @param {unknown} code
 * @returns {{code: string, family: string, scenario: string}}
 */
export function parseErrorCode(code) {
    const text = String(code ?? "");
    const m = /^E_([A-Z]+)-(\d+)$/.exec(text);
    return {
        code: text,
        family: m ? m[1] : "",
        scenario: m ? m[2] : "",
    };
}

/**
 * 把一次命令失败拆成 **码 + 文案**。
 *
 * ⚠️ M-05 的 `protocolError(code, message)` 把文案拼成了 `"{code}: {msg}"`
 * ⇒ 这里把前缀**摘掉**，让界面显示「码」与「文案」两件事（码另有
 * `data-error-code` / `data-error-family` / `data-error-scenario` 钩子）。
 *
 * @param {unknown} err
 * @returns {{code: string, message: string}}
 */
export function splitErrorCode(err) {
    const code = String(err?.code ?? "");
    let message = String(err?.message ?? "");
    const prefix = code ? `${code}: ` : "";
    if (prefix && message.startsWith(prefix)) message = message.slice(prefix.length);
    return { code, message };
}

// ---------------------------------------------------------------------------
// 器件
// ---------------------------------------------------------------------------

/**
 * 某个器件类型的**可编辑参数**清单。
 *
 * @param {unknown} kind
 * @returns {ReadonlyArray<{name: string, label: string}>}
 */
export function deviceParams(kind) {
    return DEVICE_PARAMS[String(kind ?? "")] ?? [];
}

/**
 * 造一个某类型器件的**默认**描述（草稿形状：数值是**字符串**）。
 *
 * @param {unknown} kind
 * @param {string} [keepId] — 保留已有器件 id（换类型时用）
 * @returns {object}
 */
export function newDevice(kind, keepId) {
    const name = String(kind ?? "gpio");
    const defaults = DEVICE_DEFAULTS[name] ?? DEVICE_DEFAULTS.gpio;
    const id = keepId === undefined || keepId === null || keepId === ""
        ? defaults.id
        : String(keepId);
    const device = { device: name, id };
    for (const param of deviceParams(name)) device[param.name] = String(defaults[param.name]);
    return device;
}

// ---------------------------------------------------------------------------
// 草稿
// ---------------------------------------------------------------------------

/**
 * 一份**默认草稿**（一张最小可用的板）。
 *
 * ⚠️ 草稿里的数值是**字符串**（用户输入的原样）⇒ 重渲染不会把用户正在敲的
 * `0x8000` 变成 `32768`；提交时才解析成数字。
 *
 * ⚠️ 默认值选得**合法**（V-7：段 size ≤ `region_budget_bytes`）⇒ 用户一打开
 * 编辑器就能提交一个内核会接受的描述。
 *
 * @returns {object}
 */
export function defaultDraft() {
    return {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "custom",
        name: "自定义板",
        priority: "100",
        // ⚠️ 用户配的板**不该**参与自动选档（契约层自己的 user-board 样例
        // 也是 `false`）⇒ 显式写出来，不靠 serde 的缺省。
        auto_selectable: false,
        region_budget_bytes: "8388608",
        regions: [
            { kind: "ram", base: "2147483648", size: "8388608" },
        ],
    };
}

/**
 * 深复制一份草稿（改草稿**不许**改到内核给的那份）。
 *
 * @param {object} draft
 * @returns {object}
 */
export function cloneDraft(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const next = {
        ...draft,
        regions: regions.map((region) => {
            if (String(region?.kind) === "peripheral") {
                return { ...region, device: { ...(region.device ?? {}) } };
            }
            return { ...region };
        }),
    };
    // ⚠️ `wiring` 也**深复制**（STAGE-19）：它是**对象数组** ⇒ 浅拷贝会让
    // 「改草稿」改到内核给的那份 ⇒ ⇒ 与 `regions` 同一条纪律。
    if (Array.isArray(next.wiring)) next.wiring = normalizeWiringList(next.wiring);
    return next;
}

/**
 * 一个区域 → **草稿形状**（数值转字符串）。
 *
 * @param {object} region
 * @returns {object}
 */
export function regionToDraft(region) {
    const kind = String(region?.kind ?? "ram");
    const base = Number(region?.base ?? 0) >>> 0;
    if (kind === "peripheral") {
        const device = region?.device ?? {};
        const kindName = String(device.device ?? "gpio");
        const draftDevice = { device: kindName, id: String(device.id ?? "") };
        for (const param of deviceParams(kindName)) {
            draftDevice[param.name] = String(device[param.name] ?? 0);
        }
        return { kind: "peripheral", base: String(base), device: draftDevice };
    }
    return {
        kind: kind === "rom" ? "rom" : "ram",
        base: String(base),
        size: String(Number(region?.size ?? 0) >>> 0),
    };
}

/**
 * 一组器件连线 → **规范形状**（草稿形状 == 线上形状）。
 *
 * ⚠️ **唯一落点**（STAGE-19 裁决 G）：线上 → 草稿、草稿 → 线上**共用这一个
 * 函数** ⇒ ⇒ 「连线长什么样」这条约束**只有一处定义**（不会漂）。
 *
 * ⚠️ **逐字段重建**：`from.device` / `from.event` / `to.device` /
 * `to.source` **逐个重建** ⇒ ⇒ 草稿里多出来的键**不可能**漏进契约 ⇒ ⇒
 * 「逐字段重建」这条纪律**不许破**。
 *
 * ⚠️ **形状**：`[{ from: { device, event }, to: { device, source } }]`（契约
 * 层 `WiringSpec` / `WiringFrom` / `WiringTo`，规格 §2.1 钉死名字）。
 *
 * ⚠️ **不是数组 ⇒ 当作没有连线**（缺省）⇒ ⇒ 前端**不猜**。
 *
 * ⚠️ **不判合法性**：连线引用的器件存不存在、`to.source` 在不在范围内 ⇒ ⇒
 * **只**由内核那一处裁决（`E_PLT_16`）⇒ ⇒ 前端**不写第二个校验器**。
 *
 * @param {unknown} wiring — 线上的 `wiring` 段 / 草稿里的 `wiring`
 * @returns {Array<{from: {device: string, event: string}, to: {device: string, source: number}}>}
 */
export function normalizeWiringList(wiring) {
    if (!Array.isArray(wiring)) return [];
    return wiring.map((entry) => ({
        from: {
            device: String(entry?.from?.device ?? ""),
            event: String(entry?.from?.event ?? ""),
        },
        to: {
            device: String(entry?.to?.device ?? ""),
            source: Number(entry?.to?.source ?? 0) >>> 0,
        },
    }));
}

/**
 * 把内核给的**当前生效的图**复制成一份草稿。
 *
 * ⚠️ **`wiring` 也读进来**（STAGE-19 · 裁决 G）：描述里有 ⇒ 草稿里有 ⇒ ⇒
 * 提交回去还在；⚠️ **空 / 缺省 ⇒ 草稿里**不带**这个键**。
 *
 * @param {object|null|undefined} description
 * @returns {object|null}
 */
export function draftFromDescription(description) {
    if (!description || typeof description !== "object") return null;
    const regions = Array.isArray(description.regions) ? description.regions : [];
    const draft = {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: String(description.id ?? ""),
        name: String(description.name ?? ""),
        priority: String(Number(description.priority ?? 0)),
        auto_selectable: description.auto_selectable === true,
        region_budget_bytes: String(Number(description.region_budget_bytes ?? 0)),
        regions: regions.map((region) => regionToDraft(region)),
    };
    // ⚠️ `wiring`（STAGE-19 · 裁决 G）：描述里有 ⇒ **草稿里带上** ⇒ ⇒
    // 提交回去还在（否则「板档案里那块板」一提交就把连线丢了）。
    //
    // ⚠️ **空 / 缺省 ⇒ 草稿里**不带**这个键**（不给所有板都塞一个空数组）。
    const wiring = normalizeWiringList(description.wiring);
    if (wiring.length > 0) draft.wiring = wiring;
    return draft;
}

/**
 * 草稿 → **线上描述**（提交给内核的那一份）。
 *
 * ⚠️ 这是**唯一**一处「草稿 → 契约载荷」的转换：逐字段重建，**不**透传
 * 草稿里多出来的键 ⇒ 草稿的内部字段**不可能**漏进契约。
 *
 * ⚠️ **外设窗口没有 `size`** ⇒ 这里**绝不**补一个（补了就是编数据）。
 *
 * ⚠️ **`wiring` 逐字段重建**（STAGE-19 · 裁决 G）：`from.device` /
 * `from.event` / `to.device` / `to.source` **逐个重建** ⇒ ⇒ 草稿里多出来的
 * 键**漏不进**契约；⚠️ **空 / 缺省 ⇒ 这个键**不带**（不是 `null`）。
 *
 * @param {object} draft
 * @returns {object}
 */
export function draftToDescription(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    const num = (raw) => {
        const n = parseNumber(raw);
        return n === null ? 0 : Math.max(0, Math.trunc(n)) >>> 0;
    };
    const description = {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: String(draft?.id ?? ""),
        name: String(draft?.name ?? ""),
        priority: num(draft?.priority),
        auto_selectable: draft?.auto_selectable === true,
        region_budget_bytes: num(draft?.region_budget_bytes),
        regions: regions.map((region) => {
            const kind = String(region?.kind ?? "ram");
            if (kind === "peripheral") {
                const device = region?.device ?? {};
                const kindName = String(device.device ?? "gpio");
                const wire = { device: kindName, id: String(device.id ?? "") };
                for (const param of deviceParams(kindName)) wire[param.name] = num(device[param.name]);
                return { kind: "peripheral", base: num(region?.base), device: wire };
            }
            return {
                kind: kind === "rom" ? "rom" : "ram",
                base: num(region?.base),
                size: num(region?.size),
            };
        }),
    };
    // ⚠️ `wiring`（STAGE-19 · 裁决 G）：**逐字段重建**（同一个
    // `normalizeWiringList`）⇒ ⇒ 草稿里多出来的键**不可能**漏进契约。
    //
    // ⚠️ **空 / 缺省 ⇒ 线上描述**不带**这个键**（不是 `null`、不是空数组）⇒ ⇒
    // 与契约层 serde 的 `skip_serializing_if = "Option::is_none"` **同形** ⇒ ⇒
    // 没有 `wiring` 的板往返后**仍然**没有 `wiring`。
    const wiring = normalizeWiringList(draft?.wiring);
    if (wiring.length > 0) description.wiring = wiring;
    return description;
}

/**
 * 下一个**摆放**用的基址（纯摆放辅助，**不是**校验器）。
 *
 * ⚠️ 它**只**算「新段放哪儿比较顺眼」；真正的重叠判定是内核的
 * `E_PLT-06`（裁决 E）。
 *
 * @param {object} draft
 * @param {"ram"|"rom"|"peripheral"} kind
 * @returns {string}
 */
export function nextFreeBase(draft, kind) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];
    let top = 0;
    for (const region of regions) {
        const base = parseNumber(region?.base) ?? 0;
        const size = parseNumber(region?.size) ?? 0;
        const end = String(region?.kind) === "peripheral"
            ? (base + PERIPHERAL_WINDOW_BYTES) >>> 0
            : (base + size) >>> 0;
        if (end > top) top = end;
    }
    const aligned = (top + 0xfff) >>> 0;
    if (kind === "rom") return String(aligned);
    if (kind === "peripheral") return String(aligned);
    return String(Math.max(aligned, 0x8000_0000) >>> 0);
}

/**
 * 草稿 + 一个新段 → **新草稿**（不修改原草稿）。
 *
 * @param {object} draft
 * @param {"ram"|"rom"|"peripheral"} kind
 * @returns {object}
 */
export function addRegion(draft, kind) {
    const next = cloneDraft(draft ?? defaultDraft());
    const base = nextFreeBase(next, kind);
    if (kind === "peripheral") {
        next.regions.push({ kind: "peripheral", base, device: newDevice("gpio") });
        return next;
    }
    const size = kind === "rom" ? "1048576" : "8388608";
    next.regions.push({ kind, base, size });
    return next;
}

/**
 * 草稿 − 一个段 → **新草稿**。
 *
 * @param {object} draft
 * @param {number} index
 * @returns {object}
 */
export function removeRegion(draft, index) {
    const next = cloneDraft(draft ?? defaultDraft());
    next.regions = next.regions.filter((_r, i) => i !== Number(index));
    return next;
}

/**
 * 草稿 + 换一种器件类型 → **新草稿**。
 *
 * ⚠️ 换类型 ⇒ 参数集**换**了 ⇒ 该类型的参数回到默认值。
 *
 * ⚠️ **器件 id 的处置**（`keepId` 给了就用给的）：用户**自己起**的 id 保留；
 * 还挂着**上一类型的默认值**的话 ⇒ 跟着换（`GPIO0` 挂在 UART 上会让人以为
 * 看错了）。
 *
 * @param {object} draft
 * @param {number} index
 * @param {string} kind
 * @param {string} [keepId] — 保留的器件 id；不给 ⇒ 按上面的规则自己判
 * @returns {object}
 */
export function setDeviceKind(draft, index, kind, keepId) {
    const next = cloneDraft(draft ?? defaultDraft());
    const region = next.regions[Number(index)];
    if (!region || String(region.kind) !== "peripheral") return next;
    if (!DEVICE_KINDS.includes(String(kind))) return next;
    let keep = keepId;
    if (keep === undefined) {
        const oldKind = String(region.device?.device ?? "");
        const oldDefaultId = DEVICE_DEFAULTS[oldKind]?.id ?? null;
        const currentId = String(region.device?.id ?? "");
        keep = currentId !== "" && currentId !== oldDefaultId ? currentId : undefined;
    }
    region.device = newDevice(kind, keep);
    return next;
}

/**
 * 视图状态 ⇒ **当前生效的草稿**。
 *
 * ⚠️ 渲染与「采集」必须用**同一个**基底 ⇒ 否则「显示的」与「提交的」会
 * 不是同一份东西。
 *
 * 优先级：用户已编辑的草稿 → 当前生效的图 → 一份默认草稿。
 *
 * @param {object} state — 「memory-map」视图状态
 * @returns {object}
 */
export function currentDraft(state) {
    return state?.draft
        ?? draftFromDescription(state?.memoryMap?.description)
        ?? defaultDraft();
}

// ---------------------------------------------------------------------------
// 规范 JSON 与「已被内核接受」这个**推导量**（STAGE-15 · v0.30）
// ---------------------------------------------------------------------------

/**
 * 规范 JSON：**键排序**后的序列化（递归）。
 *
 * ⚠️ **为什么不用 `JSON.stringify` 直接比**：对象键的插入顺序不同 ⇒ 同一份
 * 数据能序列化出两个不同的串 ⇒ 「逐字段相同」会被误判成「不同」。
 *
 * @param {unknown} value
 * @returns {unknown}
 */
function canonicalize(value) {
    if (Array.isArray(value)) return value.map(canonicalize);
    if (value && typeof value === "object") {
        const out = {};
        for (const key of Object.keys(value).sort()) out[key] = canonicalize(value[key]);
        return out;
    }
    return value;
}

/**
 * 规范 JSON 串（键排序后序列化）。
 *
 * @param {unknown} value
 * @returns {string}
 */
export function canonicalJson(value) {
    return JSON.stringify(canonicalize(value));
}

/**
 * 两份描述**逐字段相同**吗。
 *
 * @param {unknown} a
 * @param {unknown} b
 * @returns {boolean}
 */
export function sameDescription(a, b) {
    if (a === null || a === undefined) return false;
    if (b === null || b === undefined) return false;
    return canonicalJson(a) === canonicalJson(b);
}

/**
 * 当前草稿**已被内核接受过**吗（**推导量**，不是标志位）。
 *
 * ⚠️ **判据只有一条**：`acceptedDescription`（内核接受过的那份描述快照）
 * **非空**，且与**当前草稿**转成的描述**逐字段相同**。
 *
 * ⇒ 用户改一笔草稿 ⇒ 推导量自动变假 ⇒ **不需要**「一改输入就手工清标志」
 * 那种失效逻辑（那是**第二个真相**：标志与内容各说各话）。
 *
 * ⚠️ 这条是 STAGE-15 裁决 B（落库门槛）的**前端侧落点**：只有被内核接受过
 * 的描述才允许落库 ⇒ 库里不会有**永远装不上的板**（前端**不写第二份
 * 校验器**，延续 STAGE-14 裁决 E）。
 *
 * @param {object} state — 「memory-map」视图状态
 * @returns {boolean}
 */
export function isDraftAccepted(state) {
    const accepted = state?.acceptedDescription ?? null;
    if (!accepted) return false;
    return sameDescription(accepted, draftToDescription(currentDraft(state)));
}

// ---------------------------------------------------------------------------
// 差量更新（STAGE-16 · v0.31 · D-137）
//
// ⚠️ **差量的基底 = 最近一次 `memory.map` 事件带回来的生效图**（裁决 I）
// ⇒ 与宿主作用的对象**是同一个东西** ⇒ 不需要额外的「草稿基座」状态
// ⇒ 少一个可能不同步的地方。
//
// ⚠️ **按 `base` 定位 ⇒ 与数组顺序无关**（台账判别力 ③）。
//
// ⚠️ **元数据变了走整份替换**（裁决 D）：`id` / `name` / `priority` /
// `auto_selectable` 是**板的身份** ⇒ 改身份属于「换一块板」。
// ---------------------------------------------------------------------------

/**
 * 一个段的**定位键**（`base`）。
 *
 * ⚠️ 契约层保证**生效图里不可能有两个同 `base` 的段**（V-6 重叠检查）
 * ⇒ 按 `base` 定位**永远唯一**。
 *
 * @param {unknown} region
 * @returns {number}
 */
export function regionBase(region) {
    return Number(region?.base ?? 0) >>> 0;
}

/**
 * 两份描述的**元数据**（板的身份）一样吗。
 *
 * ⚠️ **只比这四个字段**（裁决 D）：`schema_version` 两边恒等（前端**只**
 * 写 `PLATFORM_SCHEMA_VERSION`）⇒ 不必比；`description`（备注）**前端
 * 不暴露** ⇒ 也不必比。
 *
 * @param {object|null|undefined} a
 * @param {object|null|undefined} b
 * @returns {boolean}
 */
export function sameMemoryMapMetadata(a, b) {
    if (!a || !b) return false;
    return String(a.id ?? "") === String(b.id ?? "")
        && String(a.name ?? "") === String(b.name ?? "")
        && (Number(a.priority ?? 0) >>> 0) === (Number(b.priority ?? 0) >>> 0)
        && (a.auto_selectable === true) === (b.auto_selectable === true);
}

/**
 * 两份描述的**器件连线**（`wiring`）一样吗。
 *
 * ⚠️ **用同一个归一化函数比**（STAGE-19 · 验收 C-5 的第 2 条）：两边都过
 * `normalizeWiringList`（**唯一落点**）⇒ ⇒ 「同样的连线、**键序不同**」
 * **不会**被判成不同 ⇒ ⇒ 不会**假触发**整份替换 ⇒ ⇒ 「一个约束一个落点」
 * 这条纪律在**第二处**也成立。
 *
 * ⚠️ **缺省 = 没有连线**：两边都缺省 ⇒ 一样；一边缺省、一边有 ⇒ 不同 ⇒ ⇒
 * 「删掉连线」也是**改动**（也得提交得出去）。
 *
 * ⚠️ **列表顺序算数据的一部分**（契约层是 `Vec`）⇒ 顺序不同 ⇒ 不同 ⇒ ⇒
 * ⚠️ 但界面**没有** `wiring` 的编辑器 ⇒ 顺序**改不了** ⇒ ⇒ 不会假触发。
 *
 * ⚠️ **不判合法性**：连线引用的器件存不存在、`source` 在不在范围内 ⇒ ⇒
 * **只**由内核那一处裁决 ⇒ ⇒ 前端**不写第二个校验器**。
 *
 * @param {object|null|undefined} a
 * @param {object|null|undefined} b
 * @returns {boolean}
 */
export function sameWiring(a, b) {
    return canonicalJson(normalizeWiringList(a?.wiring))
        === canonicalJson(normalizeWiringList(b?.wiring));
}

/**
 * 运行期上界（`max_region_bytes`）的**归一化值**。
 *
 * ⚠️ **缺省与 `null` 是同一件事**（契约层：`None` ⇒ 等于声明期那个）
 * ⇒ 归一化成 `null` 才能比较 ⇒ 否则「基底没有这个键」与「草稿没有这个
 * 键」会被判成「不同」⇒ 差量**永远不空**。
 *
 * @param {object|null|undefined} description
 * @returns {number|null}
 */
function normalizedMaxRegionBytes(description) {
    const raw = description?.max_region_bytes;
    if (raw === null || raw === undefined) return null;
    return Number(raw) >>> 0;
}

/**
 * **差量**：草稿 vs 生效图 ⇒ 一串差量操作。
 *
 * ⚠️ **按 `base` 定位**（不是按数组下标）⇒ 段**重排**不产生任何操作
 * ⇒ 命中**同一条**（台账判别力 ③）。
 *
 * ⚠️ **「内容不同」比字段，不比对象引用** ⇒ 用既有那份
 * {@link sameDescription}（键序无关）⇒ **不另写第二份比较**。
 *
 * ⚠️ **外设段的 `spec` 是完整的段对象**（含 `kind`）⇒ 线上形状就是
 * `{"UpsertRegion": {"spec": {"kind": "peripheral", ...}}}`。
 *
 * @param {object|null|undefined} baseDescription — **当前生效的图**
 * @param {object|null|undefined} draftDescription — 草稿转成的线上描述
 * @returns {Array<object>} 差量操作（外部标签：键 = Rust 变体名）
 */
export function diffMemoryMapOps(baseDescription, draftDescription) {
    /** @type {Array<object>} */
    const ops = [];
    const baseRegions = Array.isArray(baseDescription?.regions) ? baseDescription.regions : [];
    const draftRegions = Array.isArray(draftDescription?.regions) ? draftDescription.regions : [];

    /** 生效图：按 `base` 索引。 */
    const baseByBase = new Map();
    for (const region of baseRegions) baseByBase.set(regionBase(region), region);

    /** 草稿里出现过的 `base`。 */
    const draftBases = new Set();
    for (const region of draftRegions) {
        const base = regionBase(region);
        draftBases.add(base);
        const current = baseByBase.get(base);
        // ⚠️ **不在生效图里** 或 **内容不同** ⇒ 插入 / 替换。
        if (!current || !sameDescription(current, region)) {
            ops.push({ UpsertRegion: { spec: region } });
        }
    }
    // ⚠️ 生效图里有、草稿里没有的段 ⇒ 删掉。
    for (const region of baseRegions) {
        const base = regionBase(region);
        if (!draftBases.has(base)) ops.push({ RemoveRegion: { base } });
    }

    // ⚠️ **预算**：两个字段任一不同 ⇒ `SetBudget`。
    const baseBudget = Number(baseDescription?.region_budget_bytes ?? 0) >>> 0;
    const draftBudget = Number(draftDescription?.region_budget_bytes ?? 0) >>> 0;
    const baseMax = normalizedMaxRegionBytes(baseDescription);
    const draftMax = normalizedMaxRegionBytes(draftDescription);
    if (baseBudget !== draftBudget || baseMax !== draftMax) {
        ops.push({
            SetBudget: {
                region_budget_bytes: draftBudget,
                max_region_bytes: draftMax,
            },
        });
    }
    return ops;
}

/**
 * 一次提交**该怎么走**：整份替换 / 差量 / 什么都不发。
 *
 * ⚠️ **判据住在前端**（裁决 D）：只有前端知道草稿的元数据 ⇒ 规则**只写
 * 在这一处** ⇒ 宿主不做这个判断。
 *
 * ⚠️ **没有基底**（还没收到过 `memory.map` 事件）⇒ **整份替换**（没有
 * 差量可算）。
 *
 * ⚠️ **差量为空**（什么都没改）⇒ `mode = "none"` ⇒ 前端**不发命令**
 * ⇒ 免得白涨一次修订号。
 *
 * ⚠️ **`wiring` 变了 ⇒ 也走**整份**（STAGE-19 §9 第 1 条 · 验收 C-5）：
 * 差量契约里**没有** `wiring` 操作 ⇒ ⇒ 差量路径**表达不了** ⇒ ⇒ 「元数据
 * 变了 ⇒ **整份**」这条判据**同样**管 `wiring` ⇒ ⇒ `wiring` 的改动**必须**
 * 能提交出去。
 *
 * @param {object|null|undefined} baseDescription — **当前生效的图**
 * @param {object|null|undefined} draftDescription — 草稿转成的线上描述
 * @returns {{mode: "replace"|"patch"|"none", ops: Array<object>}}
 */
export function planMemoryMapUpdate(baseDescription, draftDescription) {
    if (!baseDescription) return { mode: "replace", ops: [] };
    if (!sameMemoryMapMetadata(baseDescription, draftDescription)) {
        return { mode: "replace", ops: [] };
    }
    // ⚠️ **`wiring` 变了 ⇒ 也走**整份**（STAGE-19 §9 第 1 条 · 验收 C-5）：
    // 差量契约里**没有** `wiring` 操作 ⇒ ⇒ 差量路径**表达不了** ⇒ ⇒
    // 「元数据变了 ⇒ **整份**」这条判据**同样**管 `wiring` ⇒ ⇒ `wiring` 的
    // 改动**必须**能提交出去（否则它**永远**提交不出去）。
    //
    // ⚠️ **同一个归一化函数**（`sameWiring` ⇒ `normalizeWiringList`）⇒ ⇒
    // 「同样的连线、键序不同」**不**算改动 ⇒ ⇒ 不会假触发整份替换。
    if (!sameWiring(baseDescription, draftDescription)) {
        return { mode: "replace", ops: [] };
    }
    const ops = diffMemoryMapOps(baseDescription, draftDescription);
    if (ops.length === 0) return { mode: "none", ops: [] };
    return { mode: "patch", ops };
}

/**
 * 板级内存图的**错误文案**（内核的原文 + 前端的一句白话）。
 *
 * ⚠️ **`E_PLT-13`（修订号冲突）必须说清下一步**（规格 §4.3）：用户的
 * 提交是基于一张**已经被改过**的图 ⇒ 告诉他「刷新后再提交」⇒ 否则用户
 * 只会看到一串看不懂的码。
 *
 * @param {string} code — 错误码（如 `"E_PLT-13"`）
 * @param {string} message — 内核给的消息
 * @returns {string}
 */
export function describeMemoryMapError(code, message) {
    const text = String(message ?? "");
    if (String(code ?? "") === "E_PLT-13") {
        return `${text}${text ? " —— " : ""}图已被改，请刷新后再提交。`;
    }
    return text;
}

// ---------------------------------------------------------------------------
// 内存映射表：描述 → 表格行
// ---------------------------------------------------------------------------

/**
 * 描述 → **内存映射表的行**。
 *
 * ⚠️ **显式转换**（不是让渲染函数直接吃事件载荷）：内部标签 `kind`
 * （`ram` / `rom` / `peripheral`）与器件标签 `device`
 * （`gpio` / `uart16550` / `clint` / `plic`）在这里映射成表里的
 * 「类型」与「详情」两列。
 *
 * ⚠️ **外设行的 `size` 是 `null`**（载荷里没有这个字段）⇒ 表格那一列
 * 如实显示「由器件自报」，**绝不编一个数字**。
 *
 * @param {object|null|undefined} description
 * @returns {Array<{base: number, size: number|null, kind: string, type: string, detail: string}>}
 */
export function descriptionToRows(description) {
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    return regions.map((region) => {
        const kind = String(region?.kind ?? "");
        const base = Number(region?.base ?? 0) >>> 0;
        if (kind === "peripheral") {
            const device = region?.device ?? {};
            const kindName = String(device.device ?? "");
            const params = deviceParams(kindName).filter(
                (p) => device[p.name] !== undefined && device[p.name] !== null,
            );
            const detail = [
                `${String(device.id ?? "?")}（${kindName || "?"}）`,
                ...params.map((p) => `${p.name}=${device[p.name]}`),
            ].join(" · ");
            // ⚠️ `size: null` —— 外设窗口的大小**没有数据源**。
            return { base, size: null, kind, type: "PERIPHERAL", detail };
        }
        const size = Number(region?.size ?? 0) >>> 0;
        const label = kind === "rom" ? "ROM" : "RAM";
        return {
            base,
            size,
            kind,
            type: label,
            detail: `[${hex32(base)}, ${hex32((base + size) >>> 0)})`,
        };
    });
}

/**
 * 内存映射表的行（**当前生效的图**）。
 *
 * ⚠️ `data-region-base` / `data-region-size` 是**十进制字符串**（脚本按数值
 * 比对 ⇒ 十六进制要多一层解析）⇒ 外设行**不挂** `data-region-size`
 * （外设窗口的 size 由器件自报，声明里**没有**）。
 *
 * @param {object[]} rows
 * @returns {string}
 */
export function renderMapRows(rows) {
    return rows.map((row) => {
        const sizeAttr = row.size === null
            ? ""
            : ` data-region-size="${String(row.size)}"`;
        return `
            <tr data-region-base="${String(row.base)}" data-region-kind="${escapeHtml(row.kind)}"${sizeAttr}>
                <td><code>${escapeHtml(hex32(row.base))}</code></td>
                <td>${escapeHtml(row.type)}</td>
                <td>${row.size === null ? "由器件自报" : escapeHtml(hex32(row.size))}</td>
                <td>${escapeHtml(row.detail)}</td>
            </tr>
        `;
    }).join("");
}

/**
 * 当前生效的内存图 → **M-14 内存映射表**（产品视图用）。
 *
 * ⚠️ **「没有数据」与「数据是空的」必须分得开**（裁决 F）：
 * - 没收到 `memory.map` 事件 ⇒ 「尚无数据」；
 * - 收到了但 0 段 ⇒ 「图是空的」。
 *
 * @param {object|null} memoryMap — `{description, pending}` 或 `null`
 * @returns {string}
 */
export function renderMemoryMapTable(memoryMap) {
    if (!memoryMap || !memoryMap.description) {
        return (
            `<p class="muted" data-memory-map-table="no-data">` +
            `尚无内存图数据 —— 内核还没有推送当前生效的内存图（订阅 <code>memory.map</code> 后自动更新）。` +
            `</p>`
        );
    }
    const rows = descriptionToRows(memoryMap.description);
    if (rows.length === 0) {
        return `<p class="muted" data-memory-map-table="empty">当前生效的内存图为空（0 段）。</p>`;
    }
    return `
        <table class="map-table" data-memory-map-table="rows">
            <thead><tr><th>区间</th><th>类型</th><th>size</th><th>详情</th></tr></thead>
            <tbody>${renderMapRows(rows)}</tbody>
        </table>
    `;
}

/**
 * 当前生效的内存图的**一句话摘要**（编辑器面板用）。
 *
 * ⚠️ 编辑器**不重复**渲染 M-14 那张表 ⇒ `data-region-*` 钩子在整页里
 * **只有一份**（产品视图那张表）⇒ 脚本按行核对时不会数到两套行。
 *
 * @param {object|null} memoryMap
 * @returns {string}
 */
export function renderEffectiveSummary(memoryMap) {
    if (!memoryMap || !memoryMap.description) {
        return `<p class="muted" data-mmap-effective="none">尚无内存图数据 —— 内核还没有推送当前生效的内存图。</p>`;
    }
    const rows = descriptionToRows(memoryMap.description);
    if (rows.length === 0) {
        return `<p class="muted" data-mmap-effective="empty">当前生效的内存图为空（0 段）。</p>`;
    }
    const budget = Number(memoryMap.description.region_budget_bytes ?? 0) >>> 0;
    const counts = rows.reduce((acc, row) => {
        acc[row.type] = (acc[row.type] ?? 0) + 1;
        return acc;
    }, {});
    const parts = Object.keys(counts).sort().map((k) => `${counts[k]} ${k}`).join(" · ");
    return (
        `<p class="muted" data-mmap-effective="summary">` +
        `当前生效：${rows.length} 段（${escapeHtml(parts)}）· 预算 ${escapeHtml(String(budget))} 字节` +
        `（${escapeHtml(hex32(budget))}）· 板 id ${escapeHtml(String(memoryMap.description.id ?? ""))}` +
        `</p>`
    );
}

// ---------------------------------------------------------------------------
// 板档案（STAGE-15 · v0.30 · D-136 / D-140）
// ---------------------------------------------------------------------------

/**
 * 板档案的**持久化状态** → 一句话说明。
 *
 * ⚠️ 三种状态**都要说出来**（不静默）：`ok` = 存在本机浏览器里；
 * `unsupported` = **板档案只存在于本会话**；`error` = 落库失败。
 *
 * @param {string} status
 * @returns {string}
 */
function archiveStatusText(status) {
    if (status === "unsupported") {
        return "板档案只存在于本会话 —— 本浏览器没有可用的持久化存储 ⇒ 刷新即失。";
    }
    if (status === "error") {
        return "板档案落库失败 —— 当前只存在于本会话（原因见页脚提示）。";
    }
    return "板档案存在本机浏览器的持久化存储里 —— 刷新后还在（但**要重新提交**才生效）。";
}

/**
 * 板档案表的**一行**（一块板）。
 *
 * ⚠️ **钩子挂在行上**（`data-mmap-board-id` / `data-mmap-board-name`）⇒
 * 门禁脚本按**数据属性**取值，**不靠文案**。
 *
 * ⚠️ 按钮上的 id 用**另一个**属性名（`data-mmap-target-id`）⇒ 这样
 * `[data-mmap-board-id]` 在整页里**一块板只命中一次**（行）⇒ 数板的时候
 * 不会把按钮也数进去。
 *
 * @param {object} entry — `{id, description}`
 * @param {string|null} selectedId — 当前选中的板 id
 * @returns {string}
 */
export function renderArchiveRow(entry, selectedId) {
    const id = String(entry?.id ?? "");
    const name = String(entry?.description?.name ?? "");
    const selected = selectedId !== null && selectedId !== undefined
        && String(selectedId) === id;
    return `
        <tr data-mmap-board-row="${escapeHtml(id)}" data-mmap-board-id="${escapeHtml(id)}" data-mmap-board-name="${escapeHtml(name)}"${selected ? ` data-mmap-board-selected="1"` : ""}>
            <td data-mmap-board-cell="name">${escapeHtml(name)}</td>
            <td data-mmap-board-cell="id"><code>${escapeHtml(id)}</code></td>
            <td><button type="button" data-action="select-memory-map" data-mmap-target-id="${escapeHtml(id)}">选用</button></td>
            <td><button type="button" data-action="delete-memory-map" data-mmap-target-id="${escapeHtml(id)}">删除</button></td>
        </tr>
    `;
}

/**
 * 板档案区块（表 + 工具条 + 新板输入 + 持久化状态 + 校验状态）。
 *
 * ⚠️ **落库门槛的呈现**：「保存 / 另存为」在草稿**没被内核接受过**时
 * `disabled`，并有一句明说的话 ⇒ 用户不会以为自己存下来了。
 *
 * ⚠️ **前端不显示参数区间常量**（那会是第二份真相）⇒ 只在器件参数旁写
 * 一句「区间由内核校验（`E_PLT-12`）」。
 *
 * ⚠️ **它自己就是一张卡**（STAGE-35 · 裁决 B ⇒ 复用 `.card`）⇒ ⇒ 面板那边
 * 直接把它放进卡片流里（**不再**套一层）。
 *
 * @param {object} state — 「memory-map」视图状态
 * @returns {string}
 */
export function renderArchivePanel(state) {
    const archive = Array.isArray(state?.archive) ? state.archive : [];
    const selectedId = state?.selectedId ?? null;
    const status = String(state?.archiveStatus ?? "ok");
    const accepted = isDraftAccepted(state);

    const rows = archive.map((entry) => renderArchiveRow(entry, selectedId)).join("");
    const table = archive.length === 0
        ? `<tr><td colspan="4" class="muted" data-mmap-archive-empty="1">还没有板档案 —— 提交一块板之后可以「保存当前板」。</td></tr>`
        : rows;

    const disabled = accepted ? "" : " disabled";

    return `
        <section class="card memory-map-archive">
            <h3>板档案<span class="tag">本机浏览器</span></h3>
            <p class="muted" data-mmap-archive-status="${escapeHtml(status)}">${escapeHtml(archiveStatusText(status))}</p>
            <p class="muted" data-mmap-validated="${accepted ? "1" : "0"}">${accepted
                ? "当前草稿已被内核接受（校验通过）—— 可以保存进板档案。"
                : "当前草稿尚未被内核接受 —— 先提交，校验通过后才能保存。"}</p>
            <table class="map-table" data-mmap-archive="1">
                <thead><tr><th>板名</th><th>id</th><th></th><th></th></tr></thead>
                <tbody>${table}</tbody>
            </table>
            <div class="row">
                <button type="button" class="btn" data-action="save-memory-map"${disabled}>保存当前板</button>
                <button type="button" class="btn" data-action="save-memory-map-as"${disabled}>另存为板</button>
                <button type="button" class="btn ghost" data-action="new-memory-map">新板</button>
            </div>
            <div class="grid-2">
                <label class="mmap-field"><span>新板 id</span>
                    <input type="text" data-mmap-new-id value=""></label>
                <label class="mmap-field"><span>新板名</span>
                    <input type="text" data-mmap-new-name value=""></label>
            </div>
            <p class="muted" data-mmap-archive-note>
                「保存 / 另存为」只写**本机浏览器**的板档案，**不改内核状态** ——
                板要生效仍需「提交板级内存图」。已存在的板**不能改 id**（id 是它的主键）
                ⇒ 想换一块板请用「另存为板」。
            </p>
        </section>
    `;
}

// ---------------------------------------------------------------------------
// DOM 采集：DOM → 草稿
// ---------------------------------------------------------------------------

/**
 * 从**活的 DOM** 把编辑器的值读回草稿。
 *
 * ⚠️ **为什么读 DOM 而不是只读草稿**：输入框里的内容是**用户输入的事实**
 * （草稿可能因为一次重渲染而落后）⇒ 提交前以 DOM 为准。
 *
 * ⚠️ **副作用**（有意的）：读不出来的数字栏会被标上 `data-mmap-invalid="1"`
 * ⇒ 提交前能**如实**告诉用户「这一栏没看懂」，而不是悄悄提交旧值。
 *
 * @param {Element|null|undefined} root — 面板容器
 * @param {object} draft — 当前草稿（作为基底）
 * @returns {object} 一份新的草稿
 */
export function collectDraft(root, draft) {
    const next = cloneDraft(draft ?? defaultDraft());
    if (!root || typeof root.querySelectorAll !== "function") return next;

    const readNumber = (el, current) => {
        const n = parseNumber(el.value);
        if (n === null) {
            el.setAttribute("data-mmap-invalid", "1");
            return current;
        }
        el.removeAttribute("data-mmap-invalid");
        return String(n);
    };

    for (const el of root.querySelectorAll("[data-mmap-field]")) {
        const field = el.getAttribute("data-mmap-field");
        const raw = el.value;
        if (field === "id" || field === "name") {
            next[field] = String(raw ?? "");
        } else if (field === "priority") {
            next.priority = readNumber(el, next.priority);
        } else if (field === "region_budget_bytes") {
            next.region_budget_bytes = readNumber(el, next.region_budget_bytes);
        }
    }

    for (const el of root.querySelectorAll("[data-mmap-region-field]")) {
        const index = Number(el.getAttribute("data-mmap-index"));
        const region = next.regions[index];
        if (!region) continue;
        if (el.getAttribute("data-mmap-region-field") === "base") {
            region.base = readNumber(el, region.base);
        } else if (String(region.kind) !== "peripheral") {
            region.size = readNumber(el, region.size);
        }
    }

    // ⚠️ 器件类型**先**处理：换类型 ⇒ 参数集换了 ⇒ 旧参数必须被丢掉。
    //
    // ⚠️ **器件 id 先从 DOM 读出来**（用户输入的事实）⇒ 换类型时才知道
    // 「这个 id 是用户自己起的」还是「只是上一类型的默认值」。
    /** @type {Map<number, string>} */
    const rawIds = new Map();
    for (const el of root.querySelectorAll("[data-mmap-device-field='id']")) {
        rawIds.set(Number(el.getAttribute("data-mmap-index")), String(el.value ?? ""));
    }
    /** 换过器件类型的段（那些段的 id 由**新类型**说了算）。 */
    const switched = new Set();
    for (const el of root.querySelectorAll("[data-mm-device-kind]")) {
        const index = Number(el.getAttribute("data-mmap-index"));
        const region = next.regions[index];
        if (!region || String(region.kind) !== "peripheral") continue;
        const kind = String(el.value ?? "");
        if (!DEVICE_KINDS.includes(kind)) continue;
        if (String(region.device?.device ?? "") === kind) continue;
        const rawId = rawIds.get(index) ?? "";
        const oldDefaultId = DEVICE_DEFAULTS[String(region.device?.device ?? "")]?.id ?? null;
        // ⚠️ 用户**自己起**的 id 保留；还挂着上一类型的默认值 ⇒ 跟着换。
        const keepId = rawId !== "" && rawId !== oldDefaultId ? rawId : undefined;
        next.regions[index] = setDeviceKind(next, index, kind, keepId).regions[index];
        switched.add(index);
    }

    for (const el of root.querySelectorAll("[data-mmap-device-field]")) {
        const index = Number(el.getAttribute("data-mmap-index"));
        const region = next.regions[index];
        if (!region || String(region.kind) !== "peripheral") continue;
        const device = region.device ?? (region.device = newDevice("gpio"));
        const field = el.getAttribute("data-mmap-device-field");
        if (field === "id") {
            // ⚠️ 换过类型 ⇒ id 由**新类型**说了算（上面已定）⇒ 不拿旧输入覆盖。
            if (switched.has(index)) continue;
            device.id = rawIds.get(index) ?? String(el.value ?? "");
            continue;
        }
        // ⚠️ 只认**当前器件类型**的参数 ⇒ 换类型后残留的旧参数栏被忽略。
        if (!deviceParams(device.device).some((p) => p.name === field)) continue;
        device[field] = readNumber(el, device[field]);
    }

    return next;
}

/**
 * 面板里**没看懂**的数字栏（`data-mmap-invalid="1"`）。
 *
 * @param {Element|null|undefined} root
 * @returns {number}
 */
export function countInvalidFields(root) {
    if (!root || typeof root.querySelectorAll !== "function") return 0;
    return root.querySelectorAll("[data-mmap-invalid]").length;
}

// ---------------------------------------------------------------------------
// 地址比例条（STAGE-35 · v0.50 批 · 裁决 D）
//
// ⚠️ **每段的宽度按它的真实地址区间算** ⇒ ⇒ 那是**真数据**的投影 ⇒ ⇒
// ⛔ **不用固定百分比**。
//
// ⚠️ **比例写进 `flex`**（`style="flex: <比例>"`）⇒ ⇒ `flex` 的 `flex-basis`
// 是 `0%` ⇒ ⇒ 浏览器按 `flex-grow` 的**比值**分配整条 ⇒ ⇒ 各段的**相对宽度**
// 正好是「这段的地址长度 : 那段的地址长度」⇒ ⇒ 比例之和**不必**是 1。
//
// ⚠️ **整图跨度** = 所有段的 `base + size` 的最大值 **减** 最小的 `base`
// ⇒ ⇒ 段**之间**的空洞（地址空间里没人用的那部分）**不占**宽度 ⇒ ⇒ 那是对的：
// 条画的是**段**，不是**地址空间**。
// ---------------------------------------------------------------------------

/**
 * 段类型 → 比例条上的**类**（裁决 B：**不新造类名** ⇒ 只用 `.seg` 那四个）。
 *
 * ⚠️ **「保留」不是契约层的一种段**（`RegionSpec` 只有 `ram` / `rom` /
 * `peripheral`）⇒ ⇒ 它是**兜底**：任何不认识（或将来的）类型都落到它 ⇒ ⇒
 * 图例上的四色与这条映射**一一对应**。
 *
 * @param {unknown} kind
 * @returns {"ram"|"rom"|"per"|"reserved"}
 */
export function segmentClass(kind) {
    const k = String(kind ?? "");
    if (k === "ram") return "ram";
    if (k === "rom") return "rom";
    if (k === "peripheral") return "per";
    return "reserved";
}

/**
 * 比例条的**图例项**（四种颜色各一项 —— 顺序固定）。
 *
 * ⚠️ **四种都在**（哪怕当前图里只用得上两种）⇒ ⇒ 用户看得见「这一条还有
 * 两种颜色没出现」⇒ ⇒ 不会以为「没出现 = 不支持」。
 */
export const SEGMENT_LEGEND = Object.freeze([
    Object.freeze({ cls: "ram", label: "RAM" }),
    Object.freeze({ cls: "rom", label: "ROM" }),
    Object.freeze({ cls: "per", label: "外设窗口" }),
    Object.freeze({ cls: "reserved", label: "保留" }),
]);

/**
 * 比例条上面那句提示（**原型原文**）。
 *
 * ⚠️ **只在真的有分段时出现** ⇒ ⇒ 没有段时那句话是**假的**（没东西可点）⇒ ⇒
 * 那时给的是 `.empty` 空态。
 */
export const SEGMENT_BAR_HINT = "内存布局做成横向条，按地址比例着色；点击任一分段即可在下方编辑。";

/**
 * 一段在比例条上显示的**名字**。
 *
 * ⚠️ **描述里没有段的名字**（`RegionSpec` 只有 `kind` / `base` / `size` /
 * `device`）⇒ ⇒ 名字**由已有的字段推**：外设段用**器件 id**（那是它唯一的
 * 身份），RAM / ROM 段用**类型名** ⇒ ⇒ **不编**一个描述里没有的名字。
 *
 * @param {object} region — 段（草稿形状）
 * @param {string} kind — 段的类型
 * @returns {string}
 */
function segmentLabel(region, kind) {
    if (kind === "peripheral") {
        const id = String(region?.device?.id ?? "");
        if (id !== "") return id;
        const devKind = String(region?.device?.device ?? "");
        return devKind !== "" ? devKind.toUpperCase() : "外设";
    }
    if (kind === "rom") return "ROM";
    if (kind === "ram") return "RAM";
    return "保留";
}

/**
 * 草稿 → 比例条的**布局**（分段 + 整图跨度）。
 *
 * ⚠️ **一段 ↔ 一行**（验收 A-11）：下标 `index` 就是 `data-mmap-row` 用的那个
 * 下标 ⇒ ⇒ 比例条上的第 i 段**就是**段表里的第 i 行 ⇒ ⇒ 谁对谁都不会错位。
 *
 * ⚠️ **总跨度为 0**（全挤在同一个基址、且都没有大小）⇒ ⇒ **等分**（退化成
 * 一条均匀的条）⇒ ⇒ 不是「宽度全 0」那种看不见的条。
 *
 * @param {object} draft — 草稿
 * @returns {{span: number, segments: Array<{index:number,kind:string,cls:string,label:string,base:number,size:number,end:number,ratio:number,title:string}>}}
 */
export function memoryMapLayout(draft) {
    const regions = Array.isArray(draft?.regions) ? draft.regions : [];

    /** @type {Array<{kind: string, base: number, size: number}>} */
    const intervals = regions.map((region) => {
        const kind = String(region?.kind ?? "ram");
        const base = parseNumber(region?.base) ?? 0;
        // ⚠️ **外设段没有 `size`** ⇒ 用**摆放窗口**折算（见
        // `PERIPHERAL_WINDOW_BYTES` 的注释）。
        const size = kind === "peripheral"
            ? PERIPHERAL_WINDOW_BYTES
            : (parseNumber(region?.size) ?? 0);
        return { kind, base, size };
    });

    // ⚠️ **整图跨度** = max(base + size) − min(base)。
    let lo = Infinity;
    let hi = -Infinity;
    for (const it of intervals) {
        if (it.base < lo) lo = it.base;
        const end = it.base + it.size;
        if (end > hi) hi = end;
    }
    const span = intervals.length === 0 ? 0 : Math.max(0, hi - lo);
    const equalShare = intervals.length > 0 ? 1 / intervals.length : 0;

    const segments = intervals.map((it, index) => {
        const end = it.base + it.size;
        const label = segmentLabel(regions[index], it.kind);
        // ⚠️ **外设段不显示字节数**（窗口大小**由器件自报** ⇒ 描述里没有 ⇒
        // 报一个数就是编数据）。
        const title = it.kind === "peripheral"
            ? `${label} · 外设窗口 · ${hex32(it.base)} 起（窗口大小由器件自报）`
            : `${label} · ${hex32(it.base)} – ${hex32(end >>> 0)} · ${it.size} 字节`;
        return {
            index,
            kind: it.kind,
            cls: segmentClass(it.kind),
            label,
            base: it.base,
            size: it.size,
            end,
            ratio: span > 0 ? it.size / span : equalShare,
            title,
        };
    });

    return { span, segments };
}

/**
 * 一个比例 → **`flex` 里那一个数**（十进制，**不带科学计数法**）。
 *
 * ⚠️ **为什么不用 `toFixed(2)`**：那会把很小的比例**抹成 0** ⇒ 那一段在条上
 * **消失** ⇒ ⇒ 留 12 位有效数字，必要时把指数写法展开。
 *
 * @param {unknown} value
 * @returns {string}
 */
function formatFlexRatio(value) {
    const n = Number(value);
    if (!Number.isFinite(n) || n <= 0) return "0";
    let text = String(Number(n.toPrecision(12)));
    if (text.includes("e") || text.includes("E")) {
        text = n.toFixed(24).replace(/0+$/, "").replace(/\.$/, "");
    }
    return text;
}

/**
 * 比例条上的**一段**。
 *
 * ⚠️ **`data-mmap-seg` 的下标与 `data-mmap-row` 的下标一一对应** ⇒ ⇒ 段表里
 * 的第 i 行就是条上的第 i 段 ⇒ ⇒ 将来接「点分段 → 选中下面那一行」时**不用
 * 再算一遍映射**。
 *
 * @param {object} segment
 * @returns {string}
 */
function renderSegment(segment) {
    return (
        `<div class="seg ${segment.cls}" style="flex: ${formatFlexRatio(segment.ratio)}" ` +
        `data-mmap-seg="${segment.index}" data-mmap-seg-base="${segment.base}" ` +
        `data-mmap-index="${segment.index}" data-action="focus-region" ` +
        `role="button" tabindex="0" ` +
        `title="${escapeHtml(segment.title)}">${escapeHtml(segment.label)}</div>`
    );
}

/**
 * **地址比例条**（裁决 D）。
 *
 * ⚠️ **没有段 ⇒ 空态**（`.empty`）⇒ ⇒ 不是一条空的条（那看着像坏了）。
 *
 * @param {Array<object>} segments
 * @returns {string}
 */
export function renderSegmentBar(segments) {
    const list = Array.isArray(segments) ? segments : [];
    if (list.length === 0) {
        return (
            `<div class="empty" data-mmap-segbar="empty">` +
            `<p>还没有分段 —— 先加一段 RAM / ROM 或一个外设窗口；有了之后这里会按` +
            `**真实地址区间**画成横向比例条。</p>` +
            `</div>`
        );
    }
    return (
        `<div class="segbar" data-mmap-segbar="1">` +
        list.map(renderSegment).join("") +
        `</div>`
    );
}

/**
 * 比例条的**图例**（四种颜色各一项）。
 *
 * ⚠️ **`data-mmap-legend-item` 挂在每一项上** ⇒ ⇒ 「四种都在」这条判据
 * **不靠文案**（也不靠后代选择器 ⇒ ⇒ 前端用例的 DOM 替身**支持**它）。
 *
 * @returns {string}
 */
export function renderSegmentLegend() {
    const items = SEGMENT_LEGEND.map((item) => (
        `<span class="li" data-mmap-legend-item="${escapeHtml(item.cls)}">` +
        `<span class="sw ${item.cls}"></span>${escapeHtml(item.label)}</span>`
    )).join("");
    return `<div class="legend" data-mmap-legend="1">${items}</div>`;
}

/**
 * 比例条那一片 **chip**：**整图跨度**（比例条的那把尺子）。
 *
 * ⚠️ **显示它** ⇒ ⇒ 用户看得见「宽度是按多长的地址区间算的」⇒ ⇒ 比例条
 * **不是**一张没有依据的画。
 *
 * @param {number} span — 整图跨度（字节）
 * @returns {string}
 */
export function renderSpanChip(span) {
    return (
        `<div class="chip">` +
        `<span class="ic">址</span>` +
        `<span class="name">整图跨度</span>` +
        `<span class="sub">最小 base → 最大 base+size</span>` +
        `<span class="vals">${escapeHtml(hex32(span))}</span>` +
        `</div>`
    );
}

// ---------------------------------------------------------------------------
// 渲染：编辑器面板
// ---------------------------------------------------------------------------

/**
 * 一个顶层的文本 / 数值输入框。
 *
 * ⚠️ **`row` 是复用**（裁决 B：不新造类名 ⇒ 用 `front/style.css` 里已有的
 * `.card .row`）⇒ ⇒ 标签与输入框**同一行**、中间有间隙（`.mmap-field` 在
 * 现在的样式表里**没有**规则 ⇒ 不给它加类名就只是行内文字）。
 *
 * @param {object} options
 * @returns {string}
 */
function field({ field: name, value, label, hex = null, extra = "" }) {
    const hint = hex === null
        ? ""
        : ` <span class="muted" data-mm-hex-hint>≈ ${escapeHtml(hex)}</span>`;
    return (
        `<label class="mmap-field row"><span>${escapeHtml(label)}</span>` +
        `<input type="text" inputmode="numeric" data-mmap-field="${escapeHtml(name)}" ` +
        `${extra} value="${escapeHtml(String(value))}">${hint}</label>`
    );
}

/**
 * 一个段的 `base` / `size` 输入框。
 *
 * @param {object} options
 * @returns {string}
 */
function regionField({ index, name, label, value, extra = "" }) {
    return (
        `<label class="mmap-field"><span>${escapeHtml(label)}</span>` +
        `<input type="text" inputmode="numeric" data-mmap-region-field="${escapeHtml(name)}" ` +
        `data-mmap-index="${index}" ${extra} value="${escapeHtml(String(value))}"></label>`
    );
}

/**
 * 一个器件参数输入框。
 *
 * @param {object} options
 * @returns {string}
 */
function deviceField({ index, name, label, value }) {
    return (
        `<label class="mmap-field"><span>${escapeHtml(label)}</span>` +
        `<input type="text" inputmode="numeric" data-mmap-device-field="${escapeHtml(name)}" ` +
        `data-mmap-index="${index}" value="${escapeHtml(String(value))}"></label>`
    );
}

/**
 * 器件类型的下拉框。
 *
 * @param {number} index
 * @param {string} current
 * @returns {string}
 */
function deviceKindSelect(index, current) {
    const options = DEVICE_KINDS.map((kind) => {
        const selected = kind === current ? " selected" : "";
        return `<option value="${escapeHtml(kind)}"${selected}>${escapeHtml(DEVICE_LABELS[kind] ?? kind)}</option>`;
    }).join("");
    // ⚠️ `value` 属性只对**测试替身**有意义（替身没有 `<select>` 的选中
    // 语义）；真浏览器里由 `selected` 属性决定。两者给的是同一个值。
    return (
        `<select data-mm-device-kind data-mmap-index="${index}" value="${escapeHtml(current)}">${options}</select>`
    );
}

/**
 * 一个外设窗口的行。
 *
 * @param {object} region
 * @param {number} index
 * @returns {string}
 */
function renderPeripheralRow(region, index) {
    const device = region?.device ?? {};
    const kindName = String(device.device ?? "gpio");
    const paramFields = deviceParams(kindName).map((p) => deviceField({
        index,
        name: p.name,
        label: p.label,
        value: device[p.name] ?? "0",
    })).join("");
    return `
        <tr data-mmap-row="${index}" data-mmap-row-kind="peripheral">
            <td>${regionField({ index, name: "base", label: "基址 base", value: region.base ?? "0", extra: "data-mm-base" })}</td>
            <td>${deviceKindSelect(index, kindName)}</td>
            <td>
                ${deviceField({ index, name: "id", label: "器件 id", value: device.id ?? "" })}
                ${paramFields}
            </td>
            <td><button type="button" class="btn ghost" data-action="remove-region" data-mmap-index="${index}">删除</button></td>
        </tr>
    `;
}

/**
 * 一个 RAM / ROM 段的行。
 *
 * @param {object} region
 * @param {number} index
 * @returns {string}
 */
function renderMemoryRow(region, index) {
    const label = region.kind === "rom" ? "ROM" : "RAM";
    return `
        <tr data-mmap-row="${index}" data-mmap-row-kind="${escapeHtml(label.toLowerCase())}">
            <td>${label}</td>
            <td>${regionField({ index, name: "base", label: "基址 base", value: region.base ?? "0", extra: "data-mm-base" })}</td>
            <td>${regionField({ index, name: "size", label: "大小 size", value: region.size ?? "0", extra: "data-mm-size" })}</td>
            <td><button type="button" class="btn ghost" data-action="remove-region" data-mmap-index="${index}">删除</button></td>
        </tr>
    `;
}

/**
 * 渲染**内存图编辑器**面板。
 *
 * @param {object} state — 「memory-map」视图状态
 * @returns {string}
 */
export function renderMemoryMapPanel(state) {
    const memoryMap = state?.memoryMap ?? null;
    const error = state?.error ?? null;
    const pending = memoryMap?.pending === true;
    // ⚠️ **草稿是前端自己的东西**（规格 §3.3 / 裁决 D）：不进契约、不进事件。
    const draft = currentDraft(state);

    let errorBox = "";
    if (error) {
        const parsed = parseErrorCode(error.code);
        errorBox =
            `<p class="mmap-error" data-memory-map-error="1" ` +
            `data-error-code="${escapeHtml(parsed.code)}" ` +
            `data-error-family="${escapeHtml(parsed.family)}" ` +
            `data-error-scenario="${escapeHtml(parsed.scenario)}">` +
            `${escapeHtml(parsed.code)}：${escapeHtml(String(error.message ?? ""))}</p>`;
    }

    // ⚠️ **「待生效」是横幅**（原型与真机门禁都叫它「横幅」⇒ ⇒ 复用
    // `.banner`，不新造类名）。
    const pendingBox = pending
        ? `<p class="banner mmap-pending" data-memory-map-pending="1">` +
          `待生效 —— 重新装载工程后应用（板级配置已存下，但当前会话的镜像还在跑：` +
          `改板只换总线、不动固件）。</p>`
        : "";

    // ⚠️ **修订号**（裁决 A · v0.31）：它是**内核事件带回来的**⇒ 与
    // 「当前生效的图」住在**同一个对象**里（不是另设的标志位）⇒
    // 提交时原样带上 ⇒ 并发丢更新看得见（`E_PLT-13`）。
    //
    // ⚠️ **没收到过事件 ⇒ 显示 0**（不许显示 `undefined` / 空）。
    const revision = Number(memoryMap?.revision ?? 0);
    const loaded = memoryMap?.loaded === true;
    const revisionBox =
        `<p class="muted" data-mmap-revision-line>` +
        `内存图修订号：<span data-mmap-revision>${escapeHtml(String(revision))}</span>` +
        ` · <span data-mmap-loaded="${loaded ? "1" : "0"}">${loaded ? "已装载工程" : "未装载工程"}</span>` +
        `</p>`;

    const regions = Array.isArray(draft.regions) ? draft.regions : [];
    const memoryRows = [];
    const peripheralRows = [];
    regions.forEach((region, index) => {
        if (String(region?.kind) === "peripheral") peripheralRows.push(renderPeripheralRow(region, index));
        else memoryRows.push(renderMemoryRow(region, index));
    });

    // ⚠️ **比例条的分段**（STAGE-35 · 裁决 D）：**按真地址区间**算，不是
    // 写死的百分比 ⇒ ⇒ 分段数 == 段表行数（验收 A-11）。
    const layout = memoryMapLayout(draft);
    const segments = layout.segments;
    const memoryCount = segments.filter((seg) => seg.kind !== "peripheral").length;
    const peripheralCount = segments.length - memoryCount;

    // ⚠️ **提示条只在真的有分段时出现** ⇒ ⇒ 没有段时那句话是**假的**（没东西
    // 可点）⇒ ⇒ 那时给的是 `.empty` 空态。
    const hintBox = segments.length > 0
        ? `<p class="banner" data-mmap-seg-hint="1">${escapeHtml(SEGMENT_BAR_HINT)}</p>`
        : "";

    // ⚠️ **已有板的 id 只读**（STAGE-15 裁决 E）：主键 = 描述的 `id`
    // ⇒ 改 id 就是「另一块板」⇒ **改 id 的入口不存在**（`selectedId` 为空
    // = 正在起一块**新**板 ⇒ 那时 id 才可编辑）。
    const selectedId = state?.selectedId ?? null;
    const idExtra = selectedId === null || selectedId === undefined ? "" : "readonly";

    return `
        <section class="memory-map-editor">
            <div class="cards" style="grid-template-columns: 1fr;">
                <div class="card">
                    <h3>板级内存图<span class="tag">M-14 · 板级配置</span></h3>
                    ${pendingBox}
                    ${errorBox}
                    <div class="row">
                        <div class="kpi"><span class="k">分段数</span><span class="v">${segments.length}</span></div>
                        <div class="kpi"><span class="k">RAM / ROM 段</span><span class="v">${memoryCount}</span></div>
                        <div class="kpi"><span class="k">外设窗口</span><span class="v">${peripheralCount}</span></div>
                    </div>
                    ${revisionBox}
                    <p class="muted">
                        描述格式版本固定为 <code>${PLATFORM_SCHEMA_VERSION}</code>
                        （<code>PLATFORM_SCHEMA_VERSION</code>）——它与契约信封的
                        <code>schema_version</code>（${CONTRACT_SCHEMA_VERSION}）是**两件事**，不要混。
                        <code>auto_selectable</code> 固定为 <code>false</code>（用户配的板不参与自动选档）。
                    </p>
                    <div class="grid-2">
                        ${field({ field: "id", label: "板 id", value: draft.id ?? "", extra: idExtra })}
                        ${field({ field: "name", label: "板名", value: draft.name ?? "" })}
                        ${field({ field: "priority", label: "优先级 priority", value: draft.priority ?? "0" })}
                        ${field({
                            field: "region_budget_bytes",
                            label: "每段的声明大小上限（region_budget_bytes，字节）",
                            value: draft.region_budget_bytes ?? "0",
                            hex: hex32(parseNumber(draft.region_budget_bytes) ?? 0),
                            extra: "data-mm-budget",
                        })}
                    </div>
                    <p class="muted" data-mmap-budget-note>
                        ⚠️ <strong>V-7</strong>：任一 RAM / ROM 段的<strong>声明</strong> size
                        不得超过 <code>region_budget_bytes</code>（超出 ⇒ 内核报 <code>E_PLT-07</code>）——
                        它是<strong>每段的声明大小上限</strong>；运行期的占用上限由内核
                        按各段的声明值另算，<strong>界面只提供这一个旋钮</strong>。
                    </p>
                </div>

                <div class="card">
                    <h3>地址布局<span class="tag">按真实地址区间着色</span></h3>
                    ${hintBox}
                    ${renderSegmentBar(segments)}
                    ${renderSegmentLegend()}
                    ${renderSpanChip(layout.span)}
                </div>

                <div class="card">
                    <h3>RAM / ROM 段<span class="tag">${memoryCount} 段</span></h3>
                    <table class="map-table" data-mmap-memory-rows>
                        <thead><tr><th>类型</th><th>基址</th><th>大小</th><th></th></tr></thead>
                        <tbody>${memoryRows.join("") || `<tr><td colspan="4" class="muted">还没有 RAM / ROM 段。</td></tr>`}</tbody>
                    </table>
                    <div class="row">
                        <button type="button" class="btn" data-action="add-ram">+ RAM 段</button>
                        <button type="button" class="btn" data-action="add-rom">+ ROM 段</button>
                    </div>
                </div>

                <div class="card">
                    <h3>外设窗口<span class="tag">${peripheralCount} 个</span></h3>
                    <p class="muted" data-mmap-device-range-note>
                        器件参数的合法区间**由内核校验**（越界 ⇒ <code>E_PLT-12</code>）——
                        前端**不写第二份区间表**（那会是第二个真相）⇒ 区间值只在内核的消息里给出。
                    </p>
                    <table class="map-table" data-mmap-peripheral-rows>
                        <thead><tr><th>基址</th><th>器件类型</th><th>器件参数</th><th></th></tr></thead>
                        <tbody>${peripheralRows.join("") || `<tr><td colspan="4" class="muted">还没有外设窗口。</td></tr>`}</tbody>
                    </table>
                    <div class="row">
                        <button type="button" class="btn" data-action="add-peripheral">+ 外设窗口</button>
                    </div>
                </div>

                ${renderArchivePanel(state)}

                <div class="card">
                    <h3>提交与工程</h3>
                    <div class="row">
                        <button type="button" class="btn primary" data-action="submit-memory-map">提交板级内存图（整份替换 / 差量）</button>
                        <button type="button" class="btn" data-action="unload-project">卸载工程</button>
                        <button type="button" class="btn" data-action="reload-project">重新装载工程</button>
                        <button type="button" class="btn" data-action="load-current">从当前生效的图载入</button>
                        <button type="button" class="btn ghost" data-action="new-draft">重建草稿</button>
                    </div>
                </div>

                <div class="card">
                    <h3>当前生效的内存图</h3>
                    ${renderEffectiveSummary(memoryMap)}
                </div>
            </div>
        </section>
    `;
}
