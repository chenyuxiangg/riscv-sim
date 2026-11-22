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
