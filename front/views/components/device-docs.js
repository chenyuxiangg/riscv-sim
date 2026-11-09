// 器件寄存器描述（前端描述层）—— 产品视图「器件内存窗口」的数据源。
//
// # 这一层是什么
//
// 内核契约里 `DeviceSpec` 只有 `id` + 类型 + 参数，**没有**「偏移 / 偏移含义」
// （`crates/contract/src/platform.rs`）。产品视图右侧要展示「基址 + 各偏移 +
// 偏移含义」，所以这里放一份**前端描述层**：
//
//   - 内置一份寄存器说明表（核的 CSR、UART16550 / GPIO / CLINT / PLIC 的
//     寄存器窗口），字段 `offset / name / meaning / width`；
//   - 用户可以在解锁后改这三列，改动只落在**草稿**里（M-03 视图状态）；
//   - 「重新载入」时**基址**照常提交内核（`SetMemoryMap`），偏移与含义是
//     文档层 ⇒ 跟着这次载入一起生效（刷新界面用），**不进契约**。
//
// ⚠️ **不碰内核**：本模块是纯前端。核的 CSR 表也在这里（核没有内存基址 ⇒
// 不显示 base，只显示内部寄存器）。
//
// ⚠️ **数值在草稿里是字符串**（用户输入的原样）⇒ 重渲染不会把用户正在敲的
// `0x004` 变成 `0x04`；比较时才归一化。

/**
 * 一个寄存器描述的**类型**（草稿形状）。
 *
 * @typedef {object} RegisterDoc
 * @property {"B"|"bit"} unit — 位宽单位（存储/外设 = 字节，CSR = 位）
 * @property {Array<{offset: string, name: string, meaning: string, width: number}>} regs
 */

/**
 * 内置寄存器说明表。
 *
 * ⚠️ 数据来源：UART16550 / SiFive 风格 GPIO / CLINT / PLIC 的公开寄存器布局，
 * 以及 RISC-V 机器态 CSR。**这是说明性数据**（文档层），不参与内核校验 ⇒
 * 用户改了也不会让仿真行为变化。
 *
 * @type {Readonly<Record<string, Readonly<{label: string, note: string, unit: "B"|"bit", regs: ReadonlyArray<object>}>>>}
 */
export const DEVICE_DOCS = Object.freeze({
    core: Object.freeze({
        label: "RV32IMC 核",
        note: "内部寄存器（CSR）—— 不经总线，没有内存基址",
        unit: "bit",
        regs: Object.freeze([
            { offset: "0x300", name: "mstatus", meaning: "机器态全局状态（MIE / MPIE / MPP）", width: 32 },
            { offset: "0x304", name: "mie", meaning: "机器中断使能（MEIE / MTIE / MSIE）", width: 32 },
            { offset: "0x305", name: "mtvec", meaning: "陷阱入口基址与模式", width: 32 },
            { offset: "0x340", name: "mscratch", meaning: "机器态暂存（陷阱处理上下文）", width: 32 },
            { offset: "0x341", name: "mepc", meaning: "异常返回地址", width: 32 },
            { offset: "0x342", name: "mcause", meaning: "最近一次陷阱的原因码", width: 32 },
            { offset: "0x343", name: "mtval", meaning: "陷阱附加信息（出错地址 / 指令）", width: 32 },
            { offset: "0x344", name: "mip", meaning: "机器中断挂起位", width: 32 },
            { offset: "0xB00", name: "mcycle", meaning: "已执行周期计数（低 32 位）", width: 32 },
            { offset: "0xB02", name: "minstret", meaning: "已退休指令计数（低 32 位）", width: 32 },
            { offset: "0xF11", name: "mvendorid", meaning: "实现厂商 ID（只读）", width: 32 },
            { offset: "0xF12", name: "marchid", meaning: "微架构 ID（只读）", width: 32 },
        ]),
    }),
    uart16550: Object.freeze({
        label: "UART16550",
        note: "串口控制器（8 个 8 位寄存器）",
        unit: "B",
        regs: Object.freeze([
            { offset: "0x000", name: "RBR / THR", meaning: "接收缓冲 / 发送保持（读 / 写同一地址）", width: 1 },
            { offset: "0x004", name: "IER", meaning: "中断使能（RX / TX / 错误）", width: 1 },
            { offset: "0x008", name: "IIR / FCR", meaning: "中断标识（读）/ FIFO 控制（写）", width: 1 },
            { offset: "0x00C", name: "LCR", meaning: "线路控制（字长 / 停止位 / 校验）", width: 1 },
            { offset: "0x010", name: "MCR", meaning: "调制解调器控制（DTR / RTS / 回环）", width: 1 },
            { offset: "0x014", name: "LSR", meaning: "线路状态（DR / THRE / 错误位）", width: 1 },
            { offset: "0x018", name: "MSR", meaning: "调制解调器状态", width: 1 },
            { offset: "0x01C", name: "SCR", meaning: "暂存寄存器（软件自定义）", width: 1 },
        ]),
    }),
    gpio: Object.freeze({
        label: "GPIO",
        note: "GPIO 控制器（SiFive 风格）",
        unit: "B",
        regs: Object.freeze([
            { offset: "0x000", name: "INPUT_VAL", meaning: "引脚输入电平（只读）", width: 4 },
            { offset: "0x004", name: "INPUT_EN", meaning: "输入使能", width: 4 },
            { offset: "0x008", name: "OUTPUT_EN", meaning: "输出使能", width: 4 },
            { offset: "0x00C", name: "OUTPUT_VAL", meaning: "输出电平", width: 4 },
            { offset: "0x010", name: "PUE", meaning: "上拉使能", width: 4 },
            { offset: "0x014", name: "DS", meaning: "驱动强度", width: 4 },
            { offset: "0x018", name: "RISE_IE", meaning: "上升沿中断使能", width: 4 },
            { offset: "0x01C", name: "RISE_IP", meaning: "上升沿中断挂起", width: 4 },
            { offset: "0x020", name: "FALL_IE", meaning: "下降沿中断使能", width: 4 },
            { offset: "0x024", name: "FALL_IP", meaning: "下降沿中断挂起", width: 4 },
            { offset: "0x028", name: "IOF_SEL", meaning: "引脚功能复用选择", width: 4 },
            { offset: "0x02C", name: "OUT_XOR", meaning: "输出取反", width: 4 },
        ]),
    }),
    clint: Object.freeze({
        label: "CLINT",
        note: "核本地中断器（定时与软中断）",
        unit: "B",
        regs: Object.freeze([
            { offset: "0x0000", name: "MSIP0", meaning: "hart0 软中断挂起（写 1 触发）", width: 4 },
            { offset: "0x4000", name: "MTIMECMP0", meaning: "hart0 定时器比较值（低半）", width: 8 },
            { offset: "0x4004", name: "MTIMECMP0_HI", meaning: "hart0 定时器比较值（高半）", width: 4 },
            { offset: "0xBFF8", name: "MTIME", meaning: "全局定时器计数（低半）", width: 8 },
        ]),
    }),
    plic: Object.freeze({
        label: "PLIC",
        note: "平台级中断控制器",
        unit: "B",
        regs: Object.freeze([
            { offset: "0x0000", name: "PRIORITY0", meaning: "中断源 0 的优先级基址", width: 4 },
            { offset: "0x1000", name: "PENDING", meaning: "待处理中断位图（只读）", width: 4 },
            { offset: "0x2000", name: "ENABLE", meaning: "各上下文的中断使能位图", width: 4 },
            { offset: "0x200000", name: "THRESHOLD", meaning: "上下文 0 的优先级阈值", width: 4 },
            { offset: "0x200004", name: "CLAIM", meaning: "认领中断 / 完成中断（读 / 写）", width: 4 },
        ]),
    }),
});

/** 器件类型是否有一份内置寄存器说明。 */
export function hasDeviceDoc(kind) {
    return Object.prototype.hasOwnProperty.call(DEVICE_DOCS, String(kind ?? ""));
}

/**
 * 一份**全新的**草稿（深拷贝内置表 ⇒ 改草稿不许改到内置表）。
 *
 * @param {string} kind
 * @returns {RegisterDoc}
 */
export function cloneRegisterDoc(kind) {
    const doc = DEVICE_DOCS[String(kind ?? "")];
    if (!doc) return { unit: "B", regs: [] };
    return {
        unit: doc.unit,
        regs: doc.regs.map((r) => ({
            offset: r.offset,
            name: r.name,
            meaning: r.meaning,
            width: Number(r.width),
        })),
    };
}

/** 数值/十六进制串归一化（只用于比较与相等判定）。 */
export function normalizeText(value) {
    return String(value ?? "").trim().toLowerCase().replace(/[_\s]/g, "");
}

/**
 * 两份寄存器表是否**内容相同**（行序参与比较）。
 *
 * @param {RegisterDoc|null|undefined} a
 * @param {RegisterDoc|null|undefined} b
 * @returns {boolean}
 */
export function sameRegisterDoc(a, b) {
    const left = Array.isArray(a?.regs) ? a.regs : [];
    const right = Array.isArray(b?.regs) ? b.regs : [];
    if (left.length !== right.length) return false;
    for (let i = 0; i < left.length; i += 1) {
        const x = left[i];
        const y = right[i];
        if (normalizeText(x?.offset) !== normalizeText(y?.offset)) return false;
        if (normalizeText(x?.name) !== normalizeText(y?.name)) return false;
        if (String(x?.meaning ?? "").trim() !== String(y?.meaning ?? "").trim()) return false;
        if (Number(x?.width) !== Number(y?.width)) return false;
    }
    return true;
}

/**
 * 某一行是否有改动（相对生效那份）。
 *
 * @param {object} row
 * @param {object|null|undefined} base
 * @returns {boolean}
 */
export function isRegisterRowDirty(row, base) {
    if (!base) return true;
    return normalizeText(row?.offset) !== normalizeText(base.offset)
        || normalizeText(row?.name) !== normalizeText(base.name)
        || String(row?.meaning ?? "").trim() !== String(base?.meaning ?? "").trim()
        || Number(row?.width) !== Number(base?.width);
}

/**
 * 描述里的**每段** → 器件列表项（树 node）。
 *
 * ⚠️ **key 用「段下标」不用基址**：基址是可编辑字段 ⇒ 用基址当 key 会让
 * 「用户改了基址」这一瞬间**选中节点丢失、草稿对不上号**。段下标在同一次
 * 装载内稳定 ⇒ key = `r<下标>`（核固定是 `core`）。
 *
 * ⚠️ **判据是段里的 `kind` 与 `device`**（`descriptionToRows` 用的是同一条
 * 路）⇒ 不猜、不按器件名反查。
 *
 * @param {object|null|undefined} description — 板级内存图描述
 * @returns {Array<object>} node：`{key, kind, deviceKind, regionIndex, base, size, id, label, note}`
 */
export function regionNodes(description) {
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    return regions.map((region, regionIndex) => {
        const regionKind = String(region?.kind ?? "");
        const base = Number(region?.base ?? 0) >>> 0;
        if (regionKind === "peripheral") {
            const deviceKind = String(region?.device?.device ?? "");
            const id = String(region?.device?.id ?? "");
            const label = DEVICE_DOCS[deviceKind]?.label ?? (deviceKind || "外设");
            return {
                key: `r${regionIndex}`,
                kind: "peripheral",
                deviceKind,
                regionIndex,
                base,
                size: null,
                id: id || label,
                label,
                note: DEVICE_DOCS[deviceKind]?.note ?? "外设窗口（寄存器映射）",
            };
        }
        const size = Number(region?.size ?? 0) >>> 0;
        const label = regionKind === "rom" ? "ROM" : "RAM";
        return {
            key: `r${regionIndex}`,
            kind: "memory",
            deviceKind: regionKind === "rom" ? "rom" : "ram",
            regionIndex,
            base,
            size,
            id: `${label}@${hexAddress(region?.base)}`,
            label,
            note: regionKind === "rom" ? "固件段（只读）" : "主存段（连续可读写）",
        };
    });
}

/**
 * 树里有没有一个「核」节点（`Project.tree` 的 MCU 节点）。
 *
 * ⚠️ 只看 `component_id` / `name` 里有没有 `rv32` / `core` / `mcu` 这类字样 ⇒
 * 找不到就**不加**核节点（不凭空调一个根节点出来）。
 *
 * @param {object|null|undefined} tree — `state.tree`
 * @returns {string|null} 核节点的显示名
 */
export function coreNodeName(tree) {
    const root = tree?.root ?? null;
    if (!root) return null;
    const children = Array.isArray(root.children) ? root.children : [];
    for (const child of children) {
        const text = `${child?.component_id ?? ""} ${child?.name ?? ""}`.toLowerCase();
        if (/rv32|riscv|core|mcu|cpu/.test(text)) {
            return String(child.name ?? child.component_id ?? "核");
        }
    }
    return null;
}

/** 一个 u32 的十六进制串（`0x` + 8 位小写）。 */
export function hexAddress(value) {
    const n = Number(value ?? 0) >>> 0;
    return `0x${n.toString(16).padStart(8, "0")}`;
}

/**
 * 视图状态 → 左栏产品树的节点表（供渲染与命令查找共用）。
 *
 * 节点顺序：核（若有）→ 存储段（ram / rom）→ 外设（peripheral）。
 * `key` 是**稳定标识**（草稿按它索引）⇒ 基址变了 key 不变。
 *
 * @param {{memoryMap?: object|null, tree?: object|null}} state — product 视图状态
 * @returns {Array<object>}
 */
export function buildDeviceNodes(state) {
    // ⚠️ **没装载产品 ⇒ 树是空的**：启动不自动装载（`loaded === false`），
    // 而内核里还装着 default 板、`memory.map` 也会推它 ⇒ 不加这道闸，空产品
    // 会显示一块用户没选过的板（「默认不加载任何板子」就落不了地）。
    if (state?.loaded === false) return [];
    const description = state?.board?.description ?? state?.memoryMap?.description ?? null;
    const nodes = [];
    const core = coreNodeName(state?.tree);
    if (core) {
        nodes.push({
            key: "core",
            kind: "core",
            deviceKind: "core",
            regionIndex: null,
            label: core,
            id: core,
            base: null,
            size: null,
            note: DEVICE_DOCS.core.note,
        });
    }
    nodes.push(...regionNodes(description));
    return nodes;
}

/**
 * 产品视图**是不是空的**（没装载产品 ⇒ 树为空、右窗空态）。
 *
 * @param {{loaded?: boolean, memoryMap?: object|null}} state
 * @returns {boolean}
 */
export function isProductEmpty(state) {
    if (state?.loaded === false) return true;
    const description = state?.board?.description ?? state?.memoryMap?.description ?? null;
    if (!description) return true;
    return buildDeviceNodes(state).length === 0;
}

/**
 * 一个节点在**档案里的身份键**（寄存器文档的索引）。
 *
 * ⚠️ 不能用节点 key（`r<i>`）：内核会把段**排序**后推回来 ⇒ 下标会变 ⇒
 * 文档会错位（「改了含义一重载就没了」）。身份键按器件本身取：
 *
 * | 节点 | 身份键 |
 * | --- | --- |
 * | 核 | `core`（一块板只有一个核） |
 * | 外设 | `device:<device.id>`（`UART0` / `GPIO0`） |
 * | 存储段 | `mem:<kind>:<同一个 kind 的第 n 个>` |
 *
 * @param {object|null|undefined} node — `buildDeviceNodes` 产出的节点
 * @param {object|null|undefined} description — 该节点所在的那份描述
 * @returns {string|null}
 */
export function nodeDocKey(node, description) {
    if (!node) return null;
    if (node.kind === "core") return "core";
    const index = node.regionIndex;
    if (!Number.isInteger(index)) return null;
    const regions = Array.isArray(description?.regions) ? description.regions : [];
    const region = regions[index] ?? null;
    if (!region) return null;
    if (region.kind === "peripheral") {
        const id = String(region?.device?.id ?? "");
        return id ? `device:${id}` : null;
    }
    const nth = regions.filter((r) => r?.kind === region.kind).indexOf(region);
    return `mem:${String(region.kind ?? "")}:${nth}`;
}

/**
 * 视图状态 → **可落库的寄存器文档**（按身份键索引）。
 *
 * ⚠️ 只导出**用户改过的那几份**（`state.docs` 里有的）⇒ 没动过的器件靠
 * 内置表回落 ⇒ 库里不会存一份「内置表的快照」（内置表改了，档案跟着变，
 * 不会两份真相不一致）。
 *
 * @param {object} state — product 视图状态
 * @returns {Object<string, {unit?: string, regs: Array<object>}>}
 */
export function docsByDevice(state) {
    const description = state?.board?.description ?? state?.memoryMap?.description ?? null;
    const nodes = buildDeviceNodes(state);
    const out = {};
    for (const node of nodes) {
        const key = nodeDocKey(node, description);
        if (!key) continue;
        const stored = state?.docs?.[node.key];
        if (stored && Array.isArray(stored.regs)) out[key] = JSON.parse(JSON.stringify(stored));
    }
    return out;
}

/**
 * **可落库的寄存器文档** → 视图状态里的 `docs` / `docsBase`（按节点 key
 * 索引，`r<i>` 由**当前这份描述**重新算）。
 *
 * @param {Object<string, object>|null|undefined} registers — 档案条目里的 `registers`
 * @param {object|null|undefined} description — 要装的那份描述
 * @param {object|null|undefined} tree — `state.tree`（决定有没有核节点）
 * @returns {Object<string, object>}
 */
export function docsForKeyed(registers, description, tree) {
    const nodes = buildDeviceNodes({ loaded: true, board: { description }, tree });
    const out = {};
    if (!registers || typeof registers !== "object") return out;
    for (const node of nodes) {
        const key = nodeDocKey(node, description);
        if (!key) continue;
        const doc = registers[key];
        if (doc && Array.isArray(doc.regs)) out[node.key] = JSON.parse(JSON.stringify(doc));
    }
    return out;
}
