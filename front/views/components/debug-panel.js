// 调试视图面板（M-02 · debug）—— 拆分自 m02_viewcomponents.js。
//
// 管 `debugView` 注册组件，以及它复用的一族渲染片段 / 纯函数：
// 地址解析、断点 / 条件 / 符号查询 / 整段转储输入行、转储结果、反汇编行、
// 寄存器格与读回状态行；另含 trace 导出块的下载工具。

import * as viewStore from "../m03_viewstore.js";
import { escapeHtml } from "../../lib/escape.js";
// ⚠️ **宽度三档只用契约镜像的那一份**（STAGE-39 · 裁决 D）⇒ ⇒
// `Byte` / `Half` / `Word` **不新造**枚举；转储长度上限也只用契约镜像的那一份。
import { DUMP_MAX_BYTES, WRITE_SIZE } from "../../lib/message.js";
// ⚠️ **「运行中」的判据与活值的隐藏口径**（STAGE-38）⇒ ⇒ 顶栏 / 调试视图 /
// 寄存器视图**共用**同一套口径。
import { isKernelRunning, liveValue } from "../../lib/run-loop.js";
// ⚠️ **「读回」的纯函数**（v0.67）⇒ ⇒ 「变了几个」现算，状态与显示不可能对不上。
import { READBACK_REASON_LABEL, diffRegisters } from "../../lib/readback.js";
// ⚠️ **「反汇编跟随 / 陈旧标注」的纯函数**（STAGE-45 · 台账 D-176）⇒ ⇒
// 视图层与 app 层引同一处。
import {
    disasmStatusText,
    followPcLabel,
} from "../../lib/disasm-follow.js";
// ⚠️ **trace 记录开关的判定**（纯函数）⇒ ⇒ 视图层渲染按钮/状态行与 app 层
// 发命令引同一处。
import {
    TRACE_SWITCH_OFF,
    TRACE_SWITCH_ON,
    traceSwitchState,
} from "../../lib/trace-switch.js";
import { register, emitCommand } from "../component-registry.js";

// ---------------------------------------------------------------------------
// trace 导出 → **下载**（D-88 · STAGE-10 裁决 D）。
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
 * @param {unknown} source — 导出源（契约的 `TraceSource`）
 * @param {unknown} format — 导出格式（契约的 `TraceFormat`）
 * @returns {string}
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
// **只允许有一处实现** ⇒ 断点入口与读内存入口**共用**它 ⇒ 前端
// **不写第二份校验器**。
//
// ⚠️ **前端只挡「压根不是一个数」**（裁决 E）：**不**预校验对齐、**不**查
// 地址是否已映射、**不**查那个地址上有没有断点 ⇒ 那些都是**内核的判据**
// ⇒ 奇数地址**照发** ⇒ 由内核回错误。
// ---------------------------------------------------------------------------

/**
 * 地址输入的**唯一提示文案**（STAGE-39 · v0.54 批）。
 *
 * @type {string}
 */
export const ADDR_INPUT_ERROR = "请先填十六进制地址";

/**
 * 「**读内存**」与「**读这一段**」这两个入口的**差别说明**（v0.64 批）。
 *
 * ⚠️ **这是 HTML 片段**（不是纯文本）⇒ 渲染时**不许**再 `escapeHtml`
 * ⇒ 否则 `**…**` 会**原样显示**。⚠️ **里面没有任何用户数据** ⇒ ⇒ 不转义是安全的。
 *
 * @type {string}
 */
export const MEMORY_READ_HINT = "<strong>读内存</strong> 单字；"
    + `<strong>读这一段</strong> 按字节数（≤ ${DUMP_MAX_BYTES} 字节）。`;

/**
 * 「读**这一段**」那个按钮的**文案**（v0.64 批）。
 *
 * @type {string}
 */
export const MEMORY_DUMP_BUTTON_LABEL = "读这一段";

/**
 * 把用户输入的文本解析成一个地址（**唯一落点** · 裁决 E）。
 *
 * ⚠️ **只挡「压根不是一个数」**：最多 8 位十六进制、可带 `0x` 前缀 ⇒
 * 解析不出来 ⇒ **格式不对**；其余（对齐 / 是否已映射 / 是否已有断点）
 * **一律放行**给内核 ⇒ ⇒ 前端**不写第二份校验器**。
 *
 * ⚠️ **按十六进制解析**（裁决 A）：界面上所有地址都是 `0x…` 的写法 ⇒
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
 * 断点的**输入方式**三值 —— **与契约 `Breakpoint.kind` 的变体逐个对齐**
 * （STAGE-55 · 表 13）⇒ ⇒ 前端不另造一套名字。
 *
 * ⚠️ 三值是**落库**的（随 `DebugMeta.breakpoints` 走）⇒ ⇒ 它只是
 *   「这个断点是**怎么**建起来的」的来源标记 ⇒ ⇒ 运行时仍只有两种命中
 *   语义（地址命中 / 谓词命中）。
 *
 * @type {{Address: string, Symbol: string, Condition: string}}
 */
export const BREAKPOINT_KIND = {
    Address: "Address",
    Symbol: "Symbol",
    Condition: "Condition",
};

/**
 * 三种类型在界面上的**中文标签**（唯一落点 · 表 14「来源标签」）。
 *
 * @type {Record<string, string>}
 */
export const BREAKPOINT_KIND_LABEL = {
    [BREAKPOINT_KIND.Address]: "地址",
    [BREAKPOINT_KIND.Symbol]: "符号",
    [BREAKPOINT_KIND.Condition]: "条件",
};

/**
 * 按类型切换的 placeholder（唯一落点）。
 *
 * @type {Record<string, string>}
 */
const BREAKPOINT_KIND_PLACEHOLDER = {
    [BREAKPOINT_KIND.Address]: "断点地址（十六进制），例如 0x2000",
    [BREAKPOINT_KIND.Symbol]: "符号名，例如 main（Tab 补全前 5 条）",
    // ⚠️ **档 4 之后**条件里可以写**当前帧局部变量名**（收口批）：placeholder
    //    点出这条，用户才知道「变量名」是合法写法（不是只有寄存器）。
    [BREAKPOINT_KIND.Condition]: "条件表达式，例如 x == 5 / [0x2000] > 3",
};

/**
 * **归一化**一个输入方式：不认识的值一律回到 `Address`。
 *
 * ⚠️ 旧草稿 / 旧链接 / 旧载荷里没有这个字段 ⇒ ⇒ **默认地址断点**
 *   ⇒ ⇒ 这一条与契约的 `#[serde(default)]` **同口径**（旧数据不会变味）。
 *
 * @param {unknown} raw
 * @returns {string}
 */
export function normalizeBreakpointKind(raw) {
    const text = String(raw ?? "");
    return Object.prototype.hasOwnProperty.call(BREAKPOINT_KIND_LABEL, text)
        ? text
        : BREAKPOINT_KIND.Address;
}

/**
 * **断点输入行**：类型下拉 + **单输入框** + 一个按钮（STAGE-55 · 表 13/14）。
 *
 * ⚠️ **收敛的是入口，不是能力**：三种类型共用一条输入行 ⇒ ⇒ 下拉切换时
 *   placeholder、候选区与错误位**都**跟着切 ⇒ ⇒ 不存在「地址框旁边还挂着
 *   一个条件框」这种把地址当表达式提交的口子。
 *
 * ⚠️ 符号类型的候选区**只在这一种类型下渲染** ⇒ ⇒ 条件类型**不做**符号
 *   补全（表 13）。
 *
 * @param {{kind?: string, draft?: string, error?: string, symbolResult?: object|null, tabIndex?: number}} [opts]
 * @returns {string}
 */
export function renderBreakpointAddRow({
    kind = BREAKPOINT_KIND.Address,
    draft = "",
    error = "",
    symbolResult = null,
    tabIndex = -1,
} = {}) {
    const k = normalizeBreakpointKind(kind);
    const options = Object.values(BREAKPOINT_KIND)
        .map((value) => `<option value="${value}"${value === k ? " selected" : ""}>${BREAKPOINT_KIND_LABEL[value]}</option>`)
        .join("");
    const candidates = k === BREAKPOINT_KIND.Symbol
        ? `<div class="symbol-candidates" data-bp-symbol-candidates>${renderSymbolCandidates(symbolResult, tabIndex)}</div>`
        : "";
    return `
        <div class="addr-input bp-add" data-addr-input="breakpoint" data-bp-add data-bp-kind="${k}">
            <select class="addr-field bp-kind" data-bp-kind-input aria-label="断点类型">${options}</select>
            <input type="text" class="addr-field" data-bp-input value="${escapeHtml(draft)}"
                placeholder="${escapeHtml(BREAKPOINT_KIND_PLACEHOLDER[k])}" spellcheck="false" autocomplete="off">
            <button type="button" data-action="set-breakpoint">加断点</button>
            <span class="addr-error" data-bp-error>${escapeHtml(error)}</span>
            ${candidates}
        </div>`;
}

/**
 * **读内存**的地址 + 宽度输入行（共享片段 · 裁决 D / K）。
 *
 * ⚠️ **宽度只用契约的 `WRITE_SIZE`**（三档）⇒ ⇒ 不新造枚举。
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
// ⚠️ **视图层是纯渲染**：不碰命令、不发请求、不解析条件表达式 ⇒ ⇒
// 条件**是不是合法**由**内核**说（前端只显示它的原文）。
// ---------------------------------------------------------------------------

/**
 * Tab 补全能循环的候选条数（**前端截前 5** · 表 13）。
 *
 * @type {number}
 */
export const BREAKPOINT_SYMBOL_TAB_LIMIT = 5;

/**
 * 一条符号候选的**可点按钮**（契约 v9 §5.1 · 裁决 N）。
 *
 * ⚠️ **形状钉死**：`class="symbol-candidate"` · `data-addr` = `0x` + **8 位小写**
 * 十六进制 · `data-symbol` = 符号名 · 按钮文本 = `<名字> @ <地址>`。
 *
 * ⚠️ **没有地址的符号**（`addr === null`）**不能**下断点 ⇒ ⇒ 如实显示成
 * 一行字（**不编**一个地址出来）⇒ 它的 class **不是** `symbol-candidate`。
 *
 * @param {{name?: string, addr?: number|null, size?: number|null}} symbol
 * @param {{index?: number, active?: boolean}} [opts] — Tab 循环的当前位
 * @returns {string}
 */
export function renderSymbolCandidate(symbol, { index = -1, active = false } = {}) {
    const name = String(symbol?.name ?? "");
    const addr = symbol?.addr;
    if (typeof addr !== "number" || !Number.isFinite(addr)) {
        return `<span class="muted symbol-candidate-na" data-bp-symbol-name="${escapeHtml(name)}">${escapeHtml(name)}（无地址）</span>`;
    }
    const addrText = `0x${(addr >>> 0).toString(16).padStart(8, "0")}`;
    return `<button type="button" class="symbol-candidate${active ? " is-active" : ""}" data-action="set-symbol-breakpoint"
        data-addr="${addrText}" data-symbol="${escapeHtml(name)}"${active ? " aria-current=\"true\"" : ""}${index >= 0 ? ` data-bp-tab-index="${index}"` : ""}>${escapeHtml(name)} @ ${addrText}</button>`;
}

/**
 * 候选区（契约 v9 §5.1 · STAGE-55 表 13「候选**前 5 条**」）。
 *
 * ⚠️ **精确命中也在列表里，且排第一** ⇒ 用户填了全名 ⇒ 列表里就是那一条。
 *
 * ⚠️ **前端只截前 5**（后端上限 16）⇒ ⇒ 截的是**补全候选**（Tab 能循环的
 *   那几条）⇒ ⇒ 与「一共查到几条」是两回事，`total` 照原样显示。
 *
 * @param {{name?: string, symbol?: object|null, candidates?: object[], total?: number}|null} result
 * @param {number} [tabIndex] — Tab 循环的当前位（`-1` = 还没开始循环）
 * @returns {string}
 */
export function renderSymbolCandidates(result, tabIndex = -1) {
    if (!result) return "";
    const exact = result.symbol ?? null;
    const candidates = Array.isArray(result.candidates) ? result.candidates : [];
    const total = Number(result.total ?? 0);
    if (!exact && candidates.length === 0) {
        const why = total === 0
            ? "没有符号表"
            : `共有 ${total} 个符号`;
        return `<p class="muted" data-bp-symbol-empty>没有这个符号 —— ${escapeHtml(why)}</p>`;
    }
    const items = [];
    if (exact) items.push(exact);
    for (const c of candidates) items.push(c);
    const shown = items.slice(0, BREAKPOINT_SYMBOL_TAB_LIMIT);
    return shown
        .map((s, i) => renderSymbolCandidate(s, { index: i, active: i === tabIndex }))
        .join("");
}

/**
 * **「为什么停」的那一行横幅**（内核的 `Halted` 回执）。
 *
 * ⚠️ **两种停因各说各的**：`"Breakpoint"` = 地址断点 / 取指前的谓词命中 ⇒
 *   「命中断点」；`"Watchpoint"` = 观察类命中（内存观察点 / store 边界的条件
 *   谓词）⇒ ⇒ 不也叫「命中断点」（那是**另一件事**）。
 *
 * ⚠️ **`triggerAddr` 只在 store 边界出现**（STAGE-55 批 C）⇒ ⇒ 有就如实报出
 *   「是哪条 store 触发的」，没有就不编（其余停因没有这个字段）。
 *
 * @param {{reason?: string|null, pc?: number, triggerAddr?: number}|null} stop
 * @returns {string}
 */
function stopBanner(stop) {
    const reason = stop?.reason ?? null;
    if (reason !== "Breakpoint" && reason !== "Watchpoint") return "";
    const hex = (v) => `0x${(Number(v ?? 0) >>> 0).toString(16).padStart(8, "0")}`;
    const label = reason === "Breakpoint" ? "命中断点" : "观察命中";
    const trigger = stop?.triggerAddr === null || stop?.triggerAddr === undefined
        ? ""
        : ` · 触发于 <code data-bp-trigger>${hex(stop.triggerAddr)}</code>`;
    return `<p class="bp-hit" data-bp-hit>${label} @ <code>${hex(stop.pc)}</code>${trigger}</p>`;
}

/**
 * **断点列表的一行**（STAGE-55 · 表 14「断点列表」）。
 *
 * ⚠️ 一行必须自己说清**四件事**：来源标签（地址 / 符号 / 条件）、
 *   它凭什么命中（地址 or 表达式原文）、命中次数、启停状态。
 *
 * ⚠️ **行内的 id 是内核给的**（`data-bp-id`）⇒ ⇒ 启停 / 撤销都**按 id**
 *   走 ⇒ ⇒ 谓词条目没有地址，拿地址当句柄会当场坏掉。
 *
 * ⚠️ **降级标记**（批 C 的 `degraded`）来自内核回执的**顶层字段**：注册期
 *   这条表达式求不出来（读内存失败等）⇒ ⇒ 内核仍收下它，但打上标记 ⇒
 *   这里如实显示，没有就不显示（**不编**状态）。
 *
 * ⚠️ **所在函数**（收口批 · `BreakpointInfo.scope_function`）：地址类条目的
 *   条件是按**哪个帧**解析的 ⇒ ⇒ 展示出来，用户才知道「变量名」指的是谁；
 *   观察式条目没有地址 ⇒ 没有这一栏（不编）。
 *
 * @param {{id?: number, addr?: number, kind?: string, condition?: string|null, symbol?: string|null, enabled?: boolean, hits?: number, degraded?: boolean, scopeFunction?: string|null}} entry
 * @returns {string}
 */
export function renderBreakpointRow(entry) {
    const id = Number(entry?.id ?? 0) >>> 0;
    const kind = normalizeBreakpointKind(entry?.kind);
    const addr = Number(entry?.addr ?? 0) >>> 0;
    const addrText = `0x${addr.toString(16).padStart(8, "0")}`;
    const on = entry?.enabled !== false;
    const hits = Number(entry?.hits ?? 0);
    const condition = entry?.condition ? String(entry.condition) : "";
    const symbol = entry?.symbol ? String(entry.symbol) : "";
    const scopeFunction = entry?.scopeFunction ? String(entry.scopeFunction) : "";
    const where = kind === BREAKPOINT_KIND.Condition
        ? `<code class="cond" data-bp-cond-text>${escapeHtml(condition)}</code>`
        : `<code class="addr">${addrText}</code>`;
    return `
        <div class="bp-row" data-bp-row data-bp-id="${id}" data-bp-addr="${addrText}" data-bp-kind="${kind}" data-bp-enabled="${on ? "true" : "false"}">
            <span class="tag bp-kind-tag" data-bp-kind-tag>${BREAKPOINT_KIND_LABEL[kind]}</span>
            ${where}
            ${symbol ? `<span class="note" data-bp-symbol>${escapeHtml(symbol)}</span>` : ""}
            ${scopeFunction ? `<span class="note" data-bp-scope>所在函数 ${escapeHtml(scopeFunction)}</span>` : ""}
            ${entry?.degraded === true ? `<span class="tag bp-degraded" data-bp-degraded>降级</span>` : ""}
            <span class="note" data-bp-hits>命中 ${hits} 次</span>
            <button type="button" data-action="toggle-breakpoint" data-id="${id}" aria-pressed="${on ? "true" : "false"}">${on ? "禁用" : "启用"}</button>
            <button type="button" data-action="clear-breakpoint" data-id="${id}">撤掉</button>
        </div>`;
}

/**
 * **整段转储**的输入行（共享片段 · 契约 v9 §5.2 · 裁决 D / P）。
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
 * ⚠️ **形状钉死**：外层 `<div class="mem-dump" data-mem-dump>`；偏移列 8 位
 * 小写十六进制不带 `0x`；十六进制列每字节 2 位、空格分隔、每行 16 字节；
 * ASCII 列 `0x20..0x7e` 原样，其余 `.`；字节序按地址递增。
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
        ? `<p class="muted" data-mem-dump-empty>尚无转储</p>`
        : `<table class="dump-table">
            <thead><tr><th>偏移</th><th>十六进制</th><th>ASCII</th></tr></thead>
            <tbody>${rows.join("")}</tbody>
        </table>`;
    return `<div class="mem-dump" data-mem-dump data-mem-dump-rows="${rows.length}" data-mem-dump-bytes="${bytes.length}">${body}</div>`;
}

/**
 * ⚠️ **草稿的共享命名空间**（STAGE-39 · 裁决 G）。
 *
 * ⚠️ **为什么不是某个视图自己的状态**：断点入口在**调试视图**、读内存
 * 入口在**调试视图与寄存器视图两处** ⇒ ⇒ 两处**必须**看到同一份草稿 ⇒ ⇒
 * 放在一个**共享**的命名空间里 ⇒ ⇒ **一个真相**。
 *
 * ⚠️ `setState` 不校验 viewId** ⇒ ⇒ 这个键**不是**一个视图 ⇒ ⇒ 不会
 * 被工作台派发渲染 ⇒ ⇒ 写草稿**不触发重渲染** ⇒ ⇒ 输入框不被重建。
 *
 * @type {string}
 */
export const DEBUGGER_STATE_KEY = "debugger";

/**
 * 「**按步数单步**」那个输入框的**默认步数**（v0.63 批）。默认 10。
 *
 * @type {string}
 */
export const DEFAULT_STEP_COUNT = "10";

/**
 * 读**当前草稿**（断点类型与那**一个**输入框 / 读内存地址 / 宽度 / 转储 / 步数）。
 *
 * ⚠️ 草稿住视图状态（不是 DOM）⇒ 渲染会重建 DOM ⇒ 草稿住在 DOM 里会被
 *   清空（D-119）⇒ 草稿**必须**在视图状态里。
 *
 * ⚠️ **STAGE-55**：地址 / 条件 / 符号的**三份草稿合成一份**（`bpDraft`）
 *   ⇒ ⇒ 与界面上的**一个输入框**一一对应 ⇒ ⇒ 不存在「框里有一份、
 *   状态里还有另一份」的两套内容。
 *
 * @returns {{bpKind: string, bpDraft: string, bpTabIndex: number, memAddr: string, memSize: string, dumpAddr: string, dumpLen: string}}
 */
export function debuggerDraft() {
    const s = viewStore.getState(DEBUGGER_STATE_KEY) ?? {};
    const size = String(s.memSize ?? WRITE_SIZE.Word);
    return {
        bpKind: normalizeBreakpointKind(s.bpKind),
        bpDraft: String(s.bpDraft ?? ""),
        bpTabIndex: Number.isInteger(s.bpTabIndex) ? s.bpTabIndex : -1,
        memAddr: String(s.memAddr ?? ""),
        memSize: Object.prototype.hasOwnProperty.call(WRITE_SIZE, size) ? size : WRITE_SIZE.Word,
        dumpAddr: String(s.dumpAddr ?? ""),
        dumpLen: String(s.dumpLen ?? DEFAULT_DUMP_LEN),
        // ⚠️ **步数草稿也住这里**（v0.63）⇒ 重渲染不丢。
        stepCount: String(s.stepCount ?? DEFAULT_STEP_COUNT),
        // 契约 v14：观察表达式草稿（同一规矩：重渲染不丢）。
        watchExpr: String(s.watchExpr ?? ""),
    };
}

/**
 * 转储长度的**默认草稿**（字节）。只在这里写一次。
 *
 * @type {string}
 */
export const DEFAULT_DUMP_LEN = "256";

/**
 * 写**草稿**（**不重渲染** · 裁决 G）。
 *
 * @param {{bpAddr?: string, memAddr?: string, memSize?: string}} patch
 */
function setDebuggerDraft(patch) {
    viewStore.setState(DEBUGGER_STATE_KEY, (prev) => ({ ...prev, ...patch }));
}

/**
 * 就地写**错误提示**（**不重渲染** · 裁决 F）。
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
 * 1. **按钮自带 `data-addr`** —— 反汇编行 / 断点列表的**旧**按钮 ⇒ 原样传；
 * 2. **按钮旁边有地址输入框** —— 本批新增的入口 ⇒ 读那个输入框。
 *
 * ⚠️ **解析不出来 ⇒ ⇒ 不发命令**（裁决 F）⇒ 返回 `{invalid: true}`。
 *
 * @param {string} action
 * @param {Element|null} el — 被点的那个按钮
 * @returns {{addr?: number|string, size?: string, name?: string, symbol?: string|null, condition?: string|null, len?: number, invalid?: boolean}|null}
 */
function resolveAddrParams(action, el) {
    // --- 契约 v9 的三条新动作（各有各的界面来源）--------------------------
    if (action === "set-symbol-breakpoint") {
        // 候选按钮自带地址与名字 ⇒ ⇒ 这一条**不看输入框**。
        return {
            addr: el?.dataset?.addr,
            symbol: el?.dataset?.symbol ? String(el.dataset.symbol) : null,
            kind: BREAKPOINT_KIND.Symbol,
        };
    }
    if (action === "lookup-symbol") {
        // ⚠️ STAGE-55：符号查询与断点共用一个框（`data-bp-input`）。
        const box = el?.closest?.("[data-bp-add]") ?? el?.closest?.("[data-addr-input]");
        const name = String(box?.querySelector?.("[data-bp-input]")?.value ?? "");
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
        //   命令** ⇒ 让**内核**拒（`E_DBG-03`）⇒ 前端把**内核原文**显示在
        //   `[data-dump-error]` 上。
        const rawLen = String(box.querySelector("[data-dump-len-input]")?.value ?? "").trim();
        const parsedLen = Number(rawLen);
        const len = Number.isFinite(parsedLen) ? Math.max(0, Math.trunc(parsedLen)) : 0;
        setAddrError(box, "[data-dump-error]", "");
        return { addr: parsed.value, len };
    }
    if (action !== "read-mem") return null;
    // --- 来源 1：按钮自带地址（旧按钮的路，一字不变）----------------------
    if (el?.dataset?.addr) return { addr: el.dataset.addr };
    // --- 来源 2：同屏的地址输入框 ----------------------------------------
    const box = el?.closest?.("[data-addr-input]");
    if (!box) return null;
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
 * 把「点一下加断点」翻译成一条命令的载荷（STAGE-55 · 表 13「提交载荷」）。
 *
 * ⚠️ **三种类型只有一个入口** ⇒ ⇒ 载荷按**当前下拉**组装：
 *   地址 ⇒ `{kind, addr}`；符号 ⇒ `{kind, name}`（名字**由内核**解析成
 *   地址，前端**不编**地址）；条件 ⇒ `{kind, condition}`（表达式原文照发，
 *   **前端不解析**）。
 *
 * ⚠️ **只有地址类型在前端挡「压根不是一个数」**（既有裁决 E 不变）⇒ ⇒
 *   符号 / 条件的合法与否**一律由内核说**。
 *
 * @param {Element|null} el — 被点的那个按钮
 * @returns {{kind: string, addr?: number, name?: string, condition?: string, invalid?: boolean}|null}
 */
function resolveAddBreakpoint(el) {
    const box = el?.closest?.("[data-bp-add]") ?? el?.closest?.("[data-addr-input]");
    if (!box) return null;
    const kind = normalizeBreakpointKind(box.querySelector("[data-bp-kind-input]")?.value);
    const text = String(box.querySelector("[data-bp-input]")?.value ?? "").trim();
    if (kind === BREAKPOINT_KIND.Address) {
        const parsed = parseAddrInput(text);
        if (!parsed.ok) {
            setAddrError(box, "[data-bp-error]", parsed.error);
            return { invalid: true };
        }
        setAddrError(box, "[data-bp-error]", "");
        return { kind, addr: parsed.value };
    }
    setAddrError(box, "[data-bp-error]", "");
    if (kind === BREAKPOINT_KIND.Condition) return { kind, condition: text };
    return { kind, name: text };
}

/**
 * 符号补全能循环的那几条候选（**前 5**、**必须有地址**）。
 *
 * ⚠️ 与渲染**同一条口径**（表 13「候选前 5 条」「无地址的候选渲染为
 *   不可点」）⇒ ⇒ 能 Tab 选中的，就是点得动的。
 *
 * @param {{symbol?: object|null, candidates?: object[]}|null} result
 * @returns {object[]}
 */
function tabCandidates(result) {
    if (!result) return [];
    const items = [];
    if (result.symbol) items.push(result.symbol);
    for (const c of Array.isArray(result.candidates) ? result.candidates : []) items.push(c);
    return items
        .filter((s) => typeof s?.addr === "number" && Number.isFinite(s.addr))
        .slice(0, BREAKPOINT_SYMBOL_TAB_LIMIT);
}

/**
 * 把**输入事件**接上（**事件委托** · 裁决 G / F11）。
 *
 * ⚠️ **监听器绑在容器上，不绑在输入框上** —— 输入框元素每次重渲染都会
 * 被重建 ⇒ 绑在它身上的监听器**会丢**。写草稿不重渲染 ⇒ 输入框不被重建。
 *
 * @param {HTMLElement} el — 视图的挂载容器
 */
function mountAddrInputListeners(el) {
    el.addEventListener("input", (ev) => {
        const target = ev.target;
        if (target?.closest?.("[data-bp-input]")) {
            // ⚠️ STAGE-55：**一个框**管三种类型 ⇒ 敲字就退出 Tab 循环
            //   （`bpTabIndex = -1`）⇒ ⇒ 下一次 Tab 是一轮**新的**前缀查询。
            setDebuggerDraft({ bpDraft: target.value ?? "", bpTabIndex: -1 });
            setAddrError(target.closest("[data-bp-add]") ?? target.closest("[data-addr-input]"), "[data-bp-error]", "");
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
        if (target?.closest?.("[data-step-count-input]")) {
            setDebuggerDraft({ stepCount: target.value ?? "" });
        }
        if (target?.closest?.("[data-watch-expr-input]")) {
            setDebuggerDraft({ watchExpr: target.value ?? "" });
        }
    });
    el.addEventListener("change", (ev) => {
        const kindBox = ev.target?.closest?.("[data-bp-kind-input]");
        if (kindBox) {
            // ⚠️ 表 14：**切换类型 ⇒ 清空草稿与错误位**（免得把地址当表达式
            //   提交出去）⇒ ⇒ 草稿写在一个地方，重渲染由 app 层发。
            emitCommand("bp-kind", "debug", { kind: normalizeBreakpointKind(kindBox.value) });
            return;
        }
        const target = ev.target?.closest?.("[data-mem-size]");
        if (!target) return;
        setDebuggerDraft({ memSize: target.value ?? WRITE_SIZE.Word });
    });
    // ⚠️ **Tab = 符号补全 / ↑↓ = 换一条 / 回车 = 点「加断点」**
    //   （表 13「Tab 触发前缀补全，候选前 5 条；↑↓ 选择、回车落断」·
    //   表 14「Enter 等价点击按钮」）。
    el.addEventListener("keydown", (ev) => {
        const box = ev.target?.closest?.("[data-bp-add]") ?? ev.target?.closest?.("[data-addr-input]");
        const input = box?.querySelector?.("[data-bp-input]");
        if (!input || ev.target !== input) return;
        const kind = normalizeBreakpointKind(box?.querySelector?.("[data-bp-kind-input]")?.value);
        if (ev?.key === "Enter") {
            ev.preventDefault?.();
            box.querySelector("[data-action=\"set-breakpoint\"]")?.click?.();
            return;
        }
        if (kind !== BREAKPOINT_KIND.Symbol) return;
        if (ev?.key !== "Tab" && ev?.key !== "ArrowDown" && ev?.key !== "ArrowUp") return;
        ev.preventDefault?.();
        const draft = debuggerDraft();
        const list = tabCandidates(viewStore.getState("debug")?.debug?.symbol ?? null);
        if (list.length === 0) {
            // 还没有候选（或一条都没有）⇒ ⇒ 这一下是**发起前缀查询**。
            setDebuggerDraft({ bpTabIndex: -1 });
            emitCommand("lookup-symbol", "debug", { name: String(input.value ?? "") });
            return;
        }
        const back = ev?.key === "ArrowUp";
        const step = back ? -1 : 1;
        const index = draft.bpTabIndex < 0
            ? 0
            : (draft.bpTabIndex + step + list.length) % list.length;
        const picked = list[index];
        input.value = String(picked?.name ?? "");
        setDebuggerDraft({ bpDraft: input.value, bpTabIndex: index });
        // ⚠️ 高亮要**看得见** ⇒ ⇒ 让 app 层重渲染一次（草稿已写进状态，
        //   这一条**不清草稿**——它与「切换类型」不是同一条动作）。
        emitCommand("bp-tab", "debug", { index });
    });
}

/**
 * trace 开关按钮上的**标签**（**状态 + 点了会发生什么**）。
 *
 * @param {{state: string}} sw — {@link traceSwitchState} 的结果
 * @returns {string}
 */
function traceToggleLabel(sw) {
    if (sw.state === TRACE_SWITCH_ON) return "trace 记录：已开启 · 点击关闭";
    if (sw.state === TRACE_SWITCH_OFF) return "trace 记录：已关闭 · 点击开启";
    return "trace 记录：状态未查询";
}

/**
 * trace 开关的**状态行**（**常驻** —— 开与关都在）。
 *
 * @param {{state: string, on: boolean, len: number, dropped: number}} sw
 * @returns {string}
 */
function renderTraceStatus(sw) {
    const len = String(sw.len);
    const dropped = String(sw.dropped);
    if (sw.state === TRACE_SWITCH_ON) {
        return `<span class="trace-state-main">trace 记录：<strong>已开启</strong> · 降速</span>`
            + `<span class="muted trace-state-hint">环里 <strong>${escapeHtml(len)}</strong> 条 · 溢出丢弃 <strong>${escapeHtml(dropped)}</strong> 条</span>`;
    }
    if (sw.state === TRACE_SWITCH_OFF) {
        return `<span class="trace-state-main">trace 记录：<strong>已关闭</strong></span>`
            + `<span class="muted trace-state-hint">环里 <strong>${escapeHtml(len)}</strong> 条 · 溢出丢弃 <strong>${escapeHtml(dropped)}</strong> 条</span>`;
    }
    return `<span class="trace-state-main">trace 记录：<strong>状态未查询</strong></span>`
        + `<span class="muted trace-state-hint">点开关向内核确认状态。</span>`;
}

const debugView = register({
    id: "debug",
    channels: ["sim"],
    render(state) {
        const sim = state?.sim ?? {};
        const debug = state?.debug ?? {};
        // ⚠️ **运行中 ⇒ ⇒ 活值隐藏**（STAGE-38 · 裁决 D）⇒ 寄存器 / PC 一律
        // 显示**固定符号 `-`** ⇒ 界面**不闪**；停止后**自动**恢复。
        const running = kernelIsRunning();
        // ⚠️ **草稿住共享命名空间**（裁决 G）⇒ 输入框内容重渲染不丢、焦点不丢。
        const draft = debuggerDraft();
        // The disassembly entries come from the kernel's `Disasm` event.
        const entries = Array.isArray(sim.disasm) ? sim.disasm : [];
        // The PC comes from the **existing** state source — the panel must
        // not invent a second one.
        const pc = Number(sim.pc ?? 0) >>> 0;
        // ⚠️ **「跟随 PC」是用户意图**（D-176）⇒ 住视图状态 `debug.followPc`
        // （默认关）⇒ 视图层**只读**，app 层写、视图层渲染。
        const followPc = debug.followPc === true;
        // ⚠️ **状态行是现算的**（disasm-follow.js）。
        const disasmStatus = disasmStatusText({ entries, pc, followPc });
        // ⚠️ **内存单元是 M-10 推的那份**（`sim.mem`）⇒ 与读内存那条路
        // 落的是**同一个缓存** ⇒ 不是第二份。
        const cells = Object.entries(sim.mem ?? {});
        // ⚠️ **转储结果住在 M-03 视图状态**（契约 v9 §5.2）⇒ `debug.dump`。
        const dump = debug.dump ?? null;
        const dumpError = String(debug.dumpError ?? "");
        const exportState = debug.export ?? null;
        // ⚠️ **开关状态是推导量**（trace-switch.js）⇒ 视图层与 app 层引同一处。
        const traceSwitch = traceSwitchState(debug.trace ?? null);
        const error = debug.error ?? null;
        // ⚠️ **断点表只有一个落点**（STAGE-55）⇒ `debug.bpEntries` 是内核
        //   `ListBreakpoints` 回执的**原样条目**（id / 地址 / kind / 表达式 /
        //   启停 / 命中）⇒ ⇒ 视图层只读，**不持有**第二份。
        const bpEntries = Array.isArray(debug.bpEntries) ? debug.bpEntries : [];
        // ⚠️ 反汇编的「断点」列只关心**有地址的**条目 ⇒ ⇒ 谓词条目没有地址，
        //   不该在反汇编表上占一格。
        const bpAddrs = bpEntries
            .filter((e) => normalizeBreakpointKind(e?.kind) !== BREAKPOINT_KIND.Condition)
            .map((e) => Number(e?.addr ?? 0) >>> 0);
        // 契约 v14：观察表达式的轮询回执（`WatchValue` 列表）。
        const watchValues = Array.isArray(debug.watchValues) ? debug.watchValues : [];
        const symbolResult = debug.symbol ?? null;
        const bpError = String(debug.bpError ?? "");
        const halt = debug.halt ?? null;
        const readback = debug.readback ?? null;
        const regBaseline = readback?.baseline ?? null;
        // ⚠️ **变了几个 = 现算**⇒ 不另存一份 ⇒ 数字与格子高亮必然来自同一组数。
        const regChanged = diffRegisters(regBaseline, sim.gpr ?? []).length;
        return `
            ${stopBanner(halt)}
            ${error
                ? `<p class="debug-error" data-debug-error>${escapeHtml(String(error.code))}：${escapeHtml(String(error.message))}</p>`
                : ""}
            <div class="debug-grid">
                <section class="card disasm">
                    <div class="row">
                        <h3>反汇编</h3>
                        <span class="tag">点断点列下断点</span>
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
                    </div>
                    <table class="disasm-table">
                        <thead><tr><th>地址</th><th>机器码</th><th>助记符</th><th>宽度</th><th>源码</th><th>断点</th></tr></thead>
                        <tbody>
                            ${entries.map((entry) => renderDisasmRow(entry, pc, bpAddrs)).join("")
                                || `<tr><td colspan="6" class="muted">尚无反汇编</td></tr>`}
                        </tbody>
                    </table>
                </section>
                <section class="card breakpoint-card">
                    <div class="row"><h3>断点</h3><span class="tag">${bpEntries.length} 个</span><button type="button" data-action="list-breakpoints">刷新内核断点表</button></div>
                    ${renderBreakpointAddRow({
                        kind: draft.bpKind,
                        draft: draft.bpDraft,
                        error: bpError,
                        symbolResult,
                        tabIndex: draft.bpTabIndex,
                    })}
                    ${bpEntries.length === 0
                        ? `<p class="muted">尚无断点</p>`
                        : `<div class="bp-list" data-bp-list>${bpEntries.map((entry) => renderBreakpointRow(entry)).join("")}</div>`}
                </section>
                <section class="card watch-card">
                    <div class="row"><h3>观察表达式</h3><span class="tag">${watchValues.length} 项</span><button type="button" data-action="list-watch-exprs">刷新</button></div>
                    <div class="card-actions">
                        <input type="text" value="${escapeHtml(draft.watchExpr)}" placeholder="pc / x1 / 0x80000000 / @x2+4" spellcheck="false" autocomplete="off" data-watch-expr-input>
                        <button type="button" data-action="set-watch-expr">添加观察</button>
                    </div>
                    ${watchValues.length === 0
                        ? `<p class="muted">尚无观察项</p>`
                        : `<div class="watch-list">${watchValues.map((item) => {
                            const value = item?.value;
                            const text = value === null || value === undefined
                                ? "—"
                                : `0x${(Number(value) >>> 0).toString(16).padStart(8, "0")}`;
                            const error = item?.error ? `<span class="note">${escapeHtml(String(item.error))}</span>` : "";
                            return `<div class="watch-row" data-watch-row data-watch-id="${Number(item?.id ?? 0)}" data-watch-changed="${item?.changed === true ? "true" : "false"}">
                                <code>${escapeHtml(String(item?.expr ?? ""))}</code>
                                <code class="addr">${text}</code>
                                ${item?.changed === true ? `<span class="tag">变化</span>` : ""}
                                ${error}
                                <button type="button" data-action="clear-watch-expr" data-id="${Number(item?.id ?? 0)}">删除</button>
                            </div>`;
                        }).join("")}</div>`}
                </section>
                <section class="card trace-export">
                    <div class="row"><h3>trace 记录 / 导出</h3></div>
                    <div class="card-actions">
                        <button type="button" data-action="trace-toggle" data-trace-switch="${traceSwitch.state}" aria-pressed="${traceSwitch.state === TRACE_SWITCH_ON ? "true" : "false"}">${escapeHtml(traceToggleLabel(traceSwitch))}</button>
                        <button type="button" data-action="export-trace" data-source="Execution" data-format="Csv" data-offset="0" data-limit="0" data-whole="1" title="导出执行轨迹 CSV">导出执行轨迹 (CSV)</button>
                        <button type="button" data-action="export-trace" data-source="Execution" data-format="Json" data-offset="0" data-limit="0" data-whole="1" title="导出执行轨迹 JSON">导出执行轨迹 (JSON)</button>
                    </div>
                    <p class="trace-state" data-trace-state data-trace-switch="${traceSwitch.state}"${traceSwitch.state === TRACE_SWITCH_ON ? " data-trace-notice" : ""}>${renderTraceStatus(traceSwitch)}</p>
                    <div class="trace-filter" data-trace-filter>
                        <input type="text" data-trace-time-from placeholder="起始时间 ps（含）" spellcheck="false" autocomplete="off">
                        <input type="text" data-trace-time-to placeholder="结束时间 ps（含）" spellcheck="false" autocomplete="off">
                        <input type="text" data-trace-addr-from placeholder="起始地址（十六进制，含）" spellcheck="false" autocomplete="off">
                        <input type="text" data-trace-addr-to placeholder="结束地址（十六进制，含）" spellcheck="false" autocomplete="off">
                    </div>
                    <p class="note" data-trace-filter-error></p>
                    ${exportState
                        ? `<p class="muted" data-trace-progress>${escapeHtml(String(exportState.source))} / ${escapeHtml(String(exportState.format))} · 条目 ${Number(exportState.offset)} → ${Number(exportState.next_offset)} / ${Number(exportState.total)}</p>`
                        : `<p class="muted">尚未导出</p>`}
                    ${exportState?.overflow_marked === true
                        ? `<p class="trace-overflow" data-trace-overflow>⚠️ 导出窗口不是完整历史（从第 ${Number(exportState.offset)} 条起）。</p>`
                        : ""}
                    <pre class="trace-chunk" data-trace-chunk>${escapeHtml(String(exportState?.chunk ?? ""))}</pre>
                </section>
                <section class="card regmem-registers" data-window="registers">
                    <div class="row">
                        <h3>寄存器窗口</h3>
                        <span class="tag">变化高亮 · 本次运行基准</span>
                        <button type="button" data-action="read-regs">读寄存器</button>
                    </div>
                    <div class="reg-grid">${renderRegGrid(sim.gpr ?? Array(32).fill(0), regBaseline, running)}</div>
                    <p class="muted" data-readback-state>${renderReadbackState(readback, regChanged)}</p>
                    <h4>PC</h4>
                    <code class="pc">${liveValue(running, `0x${(sim.pc ?? 0).toString(16).padStart(8, "0")}`)}</code>
                </section>
                <section class="card regmem-memory" data-window="memory">
                    <div class="row"><h3>内存窗口</h3></div>
                    <p class="muted" data-memory-readback-note>停下时自动重读</p>
                    ${renderMemoryReadRow({ draft: draft.memAddr, size: draft.memSize })}
                    ${renderMemoryDumpRow({ addrDraft: draft.dumpAddr, lenDraft: draft.dumpLen, error: dumpError })}
                    ${renderMemoryDump(dump)}
                    <h4>内存单元</h4>
                    ${cells.length === 0
                        ? `<p class="muted">尚无读回的内存单元</p>`
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
        // ⚠️ **按步数单步**（v0.63）⇒ 步数**从界面来**；归一在 app 层。
        if (action === "step-n") {
            const box = el?.closest?.("[data-step-count-box]");
            const raw = String(box?.querySelector?.("[data-step-count-input]")?.value ?? "");
            return emitCommand("step-n", "debug", { steps: raw });
        }
        if (action === "set-watch-expr") {
            const input = el?.closest?.(".card")?.querySelector?.("[data-watch-expr-input]");
            return emitCommand("set-watch-expr", "debug", { expr: String(input?.value ?? "") });
        }
        if (action === "clear-watch-expr") {
            return emitCommand("clear-watch-expr", "debug", { id: el?.getAttribute?.("data-id") });
        }
        if (action === "list-watch-exprs") {
            return emitCommand("list-watch-exprs", "debug", {});
        }
        // ⚠️ **STAGE-55**：加断点的载荷按**当前下拉**组装（三种类型一个入口）
        //   ⇒ ⇒ 解析不出来时**一条命令都不发**（提示位已就地写）。
        if (action === "set-breakpoint") {
            // ⚠️ **反汇编行上的那个「加断点」**是**地址类**的快捷入口 ⇒ ⇒ 它
            //   不在新增输入行里（没有下拉、没有单框）⇒ ⇒ 不能走「读草稿」
            //   那条路，直接拿行上的地址 ⇒ ⇒ 两种入口发的是**同一条命令**。
            if (el?.hasAttribute?.("data-bp-toggle")) {
                return emitCommand("set-breakpoint", "debug", {
                    kind: BREAKPOINT_KIND.Address,
                    addr: el.dataset.addr,
                });
            }
            const params = resolveAddBreakpoint(el);
            if (!params || params.invalid === true) return undefined;
            return emitCommand("set-breakpoint", "debug", params);
        }
        // ⚠️ **任意地址**（裁决 B）：地址有两个来源。
        const addrParams = resolveAddrParams(action, el);
        if (addrParams?.invalid === true) return undefined;
        if (action === "lookup-symbol") {
            return emitCommand("lookup-symbol", "debug", { name: addrParams?.name ?? "" });
        }
        const traceBox = el?.closest?.("[data-trace-filter]")?.parentElement
            ?? el?.closest?.(".trace-export");
        const traceFilterParams = {
            filterTimeFrom: traceBox?.querySelector?.("[data-trace-time-from]")?.value,
            filterTimeTo: traceBox?.querySelector?.("[data-trace-time-to]")?.value,
            filterAddrFrom: traceBox?.querySelector?.("[data-trace-addr-from]")?.value,
            filterAddrTo: traceBox?.querySelector?.("[data-trace-addr-to]")?.value,
        };
        return emitCommand(action, "debug", {
            ...(addrParams ?? {}),
            addr: addrParams?.addr ?? el.dataset.addr,
            // ⚠️ **按 id 的动作**（STAGE-55 的启停 / 撤销）⇒ ⇒ 行上的 id
            //   是内核给的句柄 ⇒ ⇒ 原样带出去。
            id: el.dataset.id,
            count: el.dataset.count,
            source: el.dataset.source,
            format: el.dataset.format,
            offset: el.dataset.offset,
            limit: el.dataset.limit,
            // ⚠️ **v0.89**：`data-whole="1"` ⇒ 这次「导出」要把**整份**落盘。
            whole: el.dataset.whole,
            ...traceFilterParams,
        });
    },
    onDelta(_delta) {},
    mountFor(el) {
        el.addEventListener("click", (ev) => debugView.onCommand(ev));
        mountAddrInputListeners(el);
        // ⚠️ **寄存器写入复用格内编辑框**：不在别处另开输入口，失焦即写。
        //   `focusout` 会冒泡（`blur` 不会），所以监听挂在视图正文容器上。
        el.addEventListener("focusout", (ev) => {
            const input = ev.target?.closest?.("[data-reg-edit]");
            if (!input || input.hasAttribute("disabled")) return;
            const original = String(input.getAttribute("data-reg-original") ?? "");
            const raw = String(input.value ?? "").trim();
            if (raw === original) return;
            const parsed = parseAddrInput(raw);
            if (!parsed.ok) {
                // 格式不对就还原，不把一条注定被内核拒掉的命令发出去。
                input.value = original;
                return;
            }
            emitCommand("write-register", "debug", {
                index: Number(input.getAttribute("data-reg-edit")),
                value: parsed.value,
            });
        });
        // ⚠️ **「跟随 PC」复选框走 `change`**（不是 `click`）⇒ `click` 触发时
        // `checked` **还没变** ⇒ 必须用 `change`。`mountFor` 只会被调一次。
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
 * ⚠️ **不许**显示空、**不许**显示 `0`、**不许**显示 `null`。
 */
export const NO_SOURCE_LINE_LABEL = "无行号";

/**
 * 一条反汇编条目的源码位置的**两个部分**（K-3）。
 *
 * ⚠️ **两个都空**才是「无行号」；只有一个有 ⇒ **如实**（别编一个出来）。
 * `line` 是 **1 起**的行号 ⇒ `0` 与负数都**不算**「有行号」。
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
 * JS 侧是 `null` / `undefined` ⇒ **两个都空**才是「无行号」。
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
 * instructions keep their `c.` prefix and there are no pseudo-instruction
 * aliases (ruling C).
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
    const codeText = `0x${code.toString(16).padStart(width === 2 ? 4 : 8, "0")}`;
    const sourceText = formatSourceLocation(entry);
    const { file, line } = sourceLocationParts(entry);
    // ⚠️ **断点列**：按钮文案随「这个地址**是不是**已有断点」变。
    const on = breakpoints.some((a) => (Number(a ?? 0) >>> 0) === addr);
    const bpButton = `
        <button type="button" class="bp-toggle${on ? " on" : ""}" data-bp-toggle="${addrText}"
            data-action="${on ? "clear-breakpoint" : "set-breakpoint"}" data-addr="${addrText}"
            data-id="${addrText}"
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

/**
 * 「**内核真的在执行**吗」（STAGE-38 · 裁决 A / M）。
 *
 * ⚠️ **只有一个落点**：视图状态 `debug.run`（app 层写）⇒ 寄存器窗口与
 * 内存窗口读**同一份**。
 *
 * @returns {boolean}
 */
function kernelIsRunning() {
    return isKernelRunning(viewStore.getState("debug")?.debug?.run);
}

/**
 * 渲染**寄存器格**（`.reg-grid` 里的 32 个格子）。
 *
 * ⚠️ **变化高亮**（v0.67 · 口径变更）：与**本次运行 / 单步开始前**的 GPR
 * 快照比 ⇒ 变大加 `.up` · 变小加 `.down` · 没变**不加类**。没有基准 ⇒
 * 一律不加类。`running` 时每格只显示固定符号 `-`。
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
        // 运行中不重建输入框：值不可写，也避免抢走运行面板的焦点。
        if (running === true) {
            return `<div><span class="muted">x${i}</span><code${clsAttr}>${text}</code></div>`;
        }
        // 复用寄存器格里的编辑框：失焦即写；x0 恒等于 0，只读。
        const disabled = i === 0 ? " disabled" : "";
        const hex = cur.toString(16).padStart(8, "0");
        return `<div><span class="muted">x${i}</span><code${clsAttr}>`
            + `<input class="reg-edit" data-reg-edit="${i}" data-reg-original="${hex}"`
            + ` value="${hex}" spellcheck="false" autocomplete="off"${disabled}>`
            + `</code></div>`;
    }).join("");
}

/**
 * 「**读回**」状态那一行（v0.67）。
 *
 * ⚠️ 三种状态分得开：① 还没跑过；② 基准已记下、还没读回；③ 读回过了
 * （上次什么时候因为什么读回 + 变了几个）。
 *
 * @param {object|null|undefined} readback — `debug.readback` 那一份
 * @param {number} changedCount — 这一趟变了几个（**现算**的）
 * @returns {string}
 */
export function renderReadbackState(readback, changedCount = 0) {
    const count = Number.isFinite(Number(changedCount)) ? Math.max(0, Number(changedCount)) : 0;
    if (!readback || !Array.isArray(readback.baseline)) {
        return "尚未运行";
    }
    if (readback.reason === null || readback.reason === undefined) {
        return "基准已记下";
    }
    const label = READBACK_REASON_LABEL[readback.reason] ?? "读回";
    const how = readback.auto === true ? "自动" : "手动";
    const failed = readback.error ? " · ⚠️ 有失败" : "";
    return `${how}读回：${label} · 变了 <strong data-readback-changed-count>${count}</strong> 个寄存器${failed}`;
}
