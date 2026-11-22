// App 层 · 调试视图状态写入辅助与归一化纯函数（从 app.js 下沉）。
//
// ⚠️ 这一层是运行态族 / 调试接线族 / 门面命令分发共用的**基础层**：
// 只写 `debug` 视图状态并发布、做地址 / 条件 / 错误文案的归一化，
// 不反向依赖运行态或调试接线族 ⇒ ⇒ 避免子模块之间循环依赖。

import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";
import { parseAddrInput } from "../m02_viewcomponents.js";
import { splitErrorCode } from "../components/memory-map.js";

/**
 * 改**调试视图状态**并重渲染（契约 v9 · 符号标签的落点）。
 *
 * ⚠️ **「有哪些断点」只有一个落点**（`debug.breakpoints`，既有判据 A-7）
 *   ⇒ ⇒ 符号名**只是附注**（`debug.bpSymbols`：地址 → 名字）⇒ ⇒ 列表的
 *   **成员**仍由 `breakpoints` 决定 ⇒ ⇒ 不是第二个真相。
 *
 * @param {(prev: object) => object} mutator
 * @returns {object} 写进去的 `debug` 子树
 */
export function updateDebugState(mutator) {
    const next = viewStore.setState("debug", (prev) => mutator(prev));
    publishViewState("debug");
    return next?.debug ?? {};
}

/** 空输入 ⇒ `undefined`；非空按十六进制解析，解析不了 ⇒ `NaN`（交给归一器报错）。 */
export function parseOptionalFilterAddr(raw) {
    const text = String(raw ?? "").trim();
    if (!text) return undefined;
    const parsed = parseAddrInput(text);
    return parsed.ok ? parsed.value : Number.NaN;
}

/**
 * 条件表达式的**线上形态**（契约 v9 · C-9）。
 *
 * ⚠️ **空 ⇒ `null`**（**不是**空串）⇒ ⇒ 「无条件断点」在线上是
 *   `condition: null` ⇒ ⇒ 内核那边就是 `Option::None` ⇒ ⇒ 既有断点语义
 *   一字不变。
 *
 * ⚠️ **非空 ⇒ 原样发出去** ⇒ ⇒ 前端**不解析**条件表达式（那是内核的
 *   判据 ⇒ ⇒ `E_DBG-02`）⇒ ⇒ 前端只显示它的原文。
 *
 * @param {unknown} raw
 * @returns {string|null}
 */
export function normalizeCondition(raw) {
    if (raw === null || raw === undefined) return null;
    const text = String(raw);
    return text === "" ? null : text;
}

/**
 * 内存读 / 转储失败 → 顶部 toast 正文。
 *
 * ⚠️ 写的是内核原文（`code` + `message`）⇒ ⇒ 前端不吞、不改写错误原因。
 *
 * @param {unknown} err
 * @returns {string}
 */
export function memoryToastText(err) {
    const { code, message } = splitErrorCode(err);
    return code ? `${code}：${message || code}` : String(err?.message ?? err);
}

/**
 * 把**内核原文**写进**某一位**（契约 v9 · C-13）。
 *
 * ⚠️ **写的是内核给的原文**（`event.value.message`）⇒ ⇒ **不吞、不改写**
 *   ⇒ ⇒ 真机断言能逐字看到内核说了什么。
 *
 * ⚠️ **空串 = 清掉那一位** ⇒ ⇒ 重试之前先清 ⇒ ⇒ 旧提示不挂着吓人。
 *
 * @param {"bp"|"dump"|null} slot
 * @param {string} message
 */
export function writeErrorSlot(slot, message) {
    const text = String(message ?? "");
    if (text) scheduleSlotExpiry(slot, text);
    if (slot === "bp") {
        // ⚠️ STAGE-55：地址 / 符号 / 条件**共用一个提示位** ⇒ ⇒ 因为界面上
        //   只有一个输入框、一个按钮 ⇒ ⇒ 错在哪儿由**内核原文**说。
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), bpError: text },
        }));
        publishViewState("debug");
    } else if (slot === "dump") {
        viewStore.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), dumpError: text },
        }));
        publishViewState("debug");
    }
}


// ---------------------------------------------------------------------------
// 报错**会过期**（v0.135 批 · Bug）
//
// ⚠️ **为什么**：校验类错误（`E_DBG-04` 符号名为空、`E_DBG-02` 条件非法）
//   是「你这一下没填对」的即时反馈 ⇒ ⇒ 它**不该永久挂在面板上**：用户改完
//   输入之后，那行红字还在，看起来像「平台坏了」。⇒ ⇒ 统一给一个
//   **存活时间**，到点自己消失（输入框里一敲字仍会立即清 ⇒ 两条路都通）。
//
// ⚠️ **只过期「显示」**：错误码与原文到达时**一字不改**（真机断言仍逐字
//   可比），过期只是**不显示了**。
// ---------------------------------------------------------------------------

/** 调试面板里那两处错误提示的**存活时长**（毫秒）。 */
export const ERROR_TTL_MS = 6000;

/** 存活时长（用例可临时调小；生产值见 {@link ERROR_TTL_MS}）。 */
let errorTtlMs = ERROR_TTL_MS;

/**
 * **用例专用**：把存活时长调小（默认 6000 ms ⇒ 门禁不可能真等 6 秒）。
 *
 * @param {number} ms
 */
export function setErrorTtlForTest(ms) {
    errorTtlMs = Number(ms) > 0 ? Number(ms) : ERROR_TTL_MS;
}

/** 位 → `debug` 子树里的字段名（**唯一落点**）。 */
const SLOT_FIELD = { bp: "bpError", dump: "dumpError" };

/**
 * 排一个「到点自己清掉」的定时器（**内容没变过才清**）。
 *
 * @param {() => boolean} stillThere — 真 ⇒ 当前显示的仍是这一条
 * @param {() => void} clear — 清掉并重渲染
 */
function scheduleExpiry(stillThere, clear) {
    const timer = setTimeout(() => {
        if (!stillThere()) return;
        clear();
    }, errorTtlMs);
    // ⚠️ Node 侧（门禁）不让一个待触发的定时器把进程吊住。
    timer?.unref?.();
}

/** 排一个输入位的过期定时器。 */
function scheduleSlotExpiry(slot, text) {
    const field = SLOT_FIELD[slot];
    if (!field) return;
    scheduleExpiry(
        () => String(viewStore.getState("debug")?.debug?.[field] ?? "") === text,
        () => writeErrorSlot(slot, ""),
    );
}

/**
 * 写调试面板**顶部那条**错误横幅（并排一个过期定时器）。
 *
 * ⚠️ 同一位置只有一条：后来的错误**顶掉**前一条，前一条的定时器到点发现
 *   「内容已经不是它」⇒ ⇒ **不清**（不会把新错误顺手抹掉）。
 *
 * @param {string} code
 * @param {string} message
 */
export function setDebugError(code, message) {
    const snapshot = { code: String(code), message: String(message) };
    updateDebugState((prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), error: snapshot },
    }));
    scheduleExpiry(
        () => {
            const cur = viewStore.getState("debug")?.debug?.error ?? null;
            return Boolean(cur)
                && String(cur.code ?? "") === snapshot.code
                && String(cur.message ?? "") === snapshot.message;
        },
        () => updateDebugState((prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), error: null },
        })),
    );
}

/** 立刻清掉顶部错误横幅（用例 / 换工程时用）。 */
export function clearDebugError() {
    updateDebugState((prev) => ({
        ...prev,
        debug: { ...(prev.debug ?? {}), error: null },
    }));
}
