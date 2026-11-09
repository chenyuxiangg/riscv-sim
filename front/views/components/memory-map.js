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
 */
export const CONTRACT_SCHEMA_VERSION = 8;

/** 器件类型（`DeviceSpec` 的四个变体，snake_case 线上名）。 */
export const DEVICE_KINDS = Object.freeze(["gpio", "uart16550", "clint", "plic"]);

/** 区域类型（`RegionSpec` 的三个变体，snake_case 线上名）。 */
export const REGION_KINDS = Object.freeze(["ram", "rom", "peripheral"]);

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
            ? (base + 0x1000) >>> 0
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

