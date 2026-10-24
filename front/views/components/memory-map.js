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

import { escapeHtml } from "../../lib/escape.js";

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/**
 * 描述格式版本（`crates/contract/src/platform.rs` 的
 * `PLATFORM_SCHEMA_VERSION`）。
 *
 * ⚠️ **这与契约信封的 `schema_version`（现在是 6）是两个不同的东西**：前者管
 * 「内存图描述长什么样」，后者管「消息信封长什么样」。
 */
export const PLATFORM_SCHEMA_VERSION = 1;

/** 契约信封的版本（只用于**文案**提醒，不是描述的版本号）。 */
export const CONTRACT_SCHEMA_VERSION = 6;

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
    return {
        ...draft,
        regions: regions.map((region) => {
            if (String(region?.kind) === "peripheral") {
                return { ...region, device: { ...(region.device ?? {}) } };
            }
            return { ...region };
        }),
    };
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
 * 把内核给的**当前生效的图**复制成一份草稿。
 *
 * @param {object|null|undefined} description
 * @returns {object|null}
 */
export function draftFromDescription(description) {
    if (!description || typeof description !== "object") return null;
    const regions = Array.isArray(description.regions) ? description.regions : [];
    return {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: String(description.id ?? ""),
        name: String(description.name ?? ""),
        priority: String(Number(description.priority ?? 0)),
        auto_selectable: description.auto_selectable === true,
        region_budget_bytes: String(Number(description.region_budget_bytes ?? 0)),
        regions: regions.map((region) => regionToDraft(region)),
    };
}

/**
 * 草稿 → **线上描述**（提交给内核的那一份）。
 *
 * ⚠️ 这是**唯一**一处「草稿 → 契约载荷」的转换：逐字段重建，**不**透传
 * 草稿里多出来的键 ⇒ 草稿的内部字段**不可能**漏进契约。
 *
 * ⚠️ **外设窗口没有 `size`** ⇒ 这里**绝不**补一个（补了就是编数据）。
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
    return {
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
        <section class="memory-map-archive">
            <h4>板档案</h4>
            <p class="muted" data-mmap-archive-status="${escapeHtml(status)}">${escapeHtml(archiveStatusText(status))}</p>
            <p class="muted" data-mmap-validated="${accepted ? "1" : "0"}">${accepted
                ? "当前草稿已被内核接受（校验通过）—— 可以保存进板档案。"
                : "当前草稿尚未被内核接受 —— 先提交，校验通过后才能保存。"}</p>
            <table class="map-table" data-mmap-archive="1">
                <thead><tr><th>板名</th><th>id</th><th></th><th></th></tr></thead>
                <tbody>${table}</tbody>
            </table>
            <div class="debug-toolbar">
                <button type="button" data-action="save-memory-map"${disabled}>保存当前板</button>
                <button type="button" data-action="save-memory-map-as"${disabled}>另存为板</button>
                <button type="button" data-action="new-memory-map">新板</button>
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
// 渲染：编辑器面板
// ---------------------------------------------------------------------------

/**
 * 一个顶层的文本 / 数值输入框。
 *
 * @param {object} options
 * @returns {string}
 */
function field({ field: name, value, label, hex = null, extra = "" }) {
    const hint = hex === null
        ? ""
        : ` <span class="muted" data-mm-hex-hint>≈ ${escapeHtml(hex)}</span>`;
    return (
        `<label class="mmap-field"><span>${escapeHtml(label)}</span>` +
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
            <td><button type="button" data-action="remove-region" data-mmap-index="${index}">删除</button></td>
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
            <td><button type="button" data-action="remove-region" data-mmap-index="${index}">删除</button></td>
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

    const pendingBox = pending
        ? `<p class="mmap-pending" data-memory-map-pending="1">` +
          `待生效 —— 重新装载工程后应用（板级配置已存下，但当前会话的镜像还在跑：` +
          `改板只换总线、不动固件）。</p>`
        : "";

    const regions = Array.isArray(draft.regions) ? draft.regions : [];
    const memoryRows = [];
    const peripheralRows = [];
    regions.forEach((region, index) => {
        if (String(region?.kind) === "peripheral") peripheralRows.push(renderPeripheralRow(region, index));
        else memoryRows.push(renderMemoryRow(region, index));
    });

    // ⚠️ **已有板的 id 只读**（STAGE-15 裁决 E）：主键 = 描述的 `id`
    // ⇒ 改 id 就是「另一块板」⇒ **改 id 的入口不存在**（`selectedId` 为空
    // = 正在起一块**新**板 ⇒ 那时 id 才可编辑）。
    const selectedId = state?.selectedId ?? null;
    const idExtra = selectedId === null || selectedId === undefined ? "" : "readonly";

    return `
        <section class="memory-map-editor">
            <h3>板级内存图（M-14 · 板级配置）</h3>
            ${pendingBox}
            ${errorBox}
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
                    label: "region_budget_bytes（字节）",
                    value: draft.region_budget_bytes ?? "0",
                    hex: hex32(parseNumber(draft.region_budget_bytes) ?? 0),
                    extra: "data-mm-budget",
                })}
            </div>
            <p class="muted" data-mmap-budget-note>
                ⚠️ <strong>V-7</strong>：任一 RAM / ROM 段的<strong>声明</strong> size
                不得超过 <code>region_budget_bytes</code>（超出 ⇒ 内核报 <code>E_PLT-07</code>）；
                同一个数还是<strong>运行期物理占用</strong>的上界 ⇒ 想换 1 GiB RAM
                就必须把它一起抬到 1 GiB。
            </p>
            <h4>RAM / ROM 段</h4>
            <table class="map-table" data-mmap-memory-rows>
                <thead><tr><th>类型</th><th>基址</th><th>大小</th><th></th></tr></thead>
                <tbody>${memoryRows.join("") || `<tr><td colspan="4" class="muted">还没有 RAM / ROM 段。</td></tr>`}</tbody>
            </table>
            <div class="debug-toolbar">
                <button type="button" data-action="add-ram">+ RAM 段</button>
                <button type="button" data-action="add-rom">+ ROM 段</button>
            </div>
            <h4>外设窗口</h4>
            <p class="muted" data-mmap-device-range-note>
                器件参数的合法区间**由内核校验**（越界 ⇒ <code>E_PLT-12</code>）——
                前端**不写第二份区间表**（那会是第二个真相）⇒ 区间值只在内核的消息里给出。
            </p>
            <table class="map-table" data-mmap-peripheral-rows>
                <thead><tr><th>基址</th><th>器件类型</th><th>器件参数</th><th></th></tr></thead>
                <tbody>${peripheralRows.join("") || `<tr><td colspan="4" class="muted">还没有外设窗口。</td></tr>`}</tbody>
            </table>
            <div class="debug-toolbar">
                <button type="button" data-action="add-peripheral">+ 外设窗口</button>
            </div>
            <div class="debug-toolbar">
                <button type="button" data-action="submit-memory-map">提交板级内存图（SetMemoryMap）</button>
                <button type="button" data-action="reload-project">重新装载工程</button>
                <button type="button" data-action="load-current">从当前生效的图载入</button>
                <button type="button" data-action="new-draft">重建草稿</button>
            </div>
            ${renderArchivePanel(state)}
            <h4>当前生效的内存图</h4>
            ${renderEffectiveSummary(memoryMap)}
        </section>
    `;
}
