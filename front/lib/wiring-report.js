// v0.72 批 · STAGE-48 · D-177 A —— **连线报告**的纯函数。
//
// ## 为什么要有这个模块
//
// 内核在「**接受描述的那一刻**」就会说清「你写的 N 根线里有 M 根当前抬不了」
// ⇒ ⇒ 用户提交一份带 `wiring` 的板 ⇒ ⇒ **立刻**知道哪几根线没通电
// ⇒ ⇒ 「描述被接受但线被静默丢弃」这件事**不再是静默的**。
//
// ⚠️ **前端不自己判**（裁决 L）：「这根线抬不抬得了」是**内核**的判定
// ⇒ ⇒ 前端**只**把内核给的数字与理由**原样**显示 ⇒ ⇒ **不写第二份
// 校验器**（那会是第二个真相源）。
//
// ⚠️ **显示层**允许把内核给的那个**稳定词**配一句白话（与
// `describeMemoryMapError` 给 `E_PLT-13` 配白话**同一个手法**）⇒ ⇒
// 但 **白话不许替代原词** ⇒ ⇒ 界面上**原词一定在** ⇒ ⇒ 用户能与内核的
// 原文对得上 ⇒ ⇒ **未知词**（内核将来加了新的）**原样显示**、**不编**。
//
// ⚠️ **纯函数**（与 `front/lib/run-loop.js` / `front/lib/readback.js` 同一
// 手法）⇒ ⇒ **Node 侧零 DOM 就能验** ⇒ ⇒ 「报告怎么显示」不必拖一整条
// 链路才测得到。
//
// ⚠️ **新能力一律住新模块**（v0.63 批的白屏纪律）⇒ ⇒ 不给既有模块加导出。

/**
 * 理由词 → 白话。
 *
 * ⚠️ **只有内核给过的 6 个词才有白话** ⇒ ⇒ 别的词**原样显示** ⇒ ⇒
 * 「内核加了新理由」这件事**看得见**（而不是被前端悄悄编掉）。
 *
 * @type {Readonly<Record<string, string>>}
 */
export const WIRE_DROP_REASON_TEXT = Object.freeze({
    "source-unknown": "源器件不在板上的器件实例里",
    "source-type": "源器件不是 UART（宿主只从 UART 抬 rx_ready）",
    "target-unknown": "目标器件不在板上的器件实例里",
    "target-type": "目标器件不是 PLIC（宿主只往 PLIC 递源）",
    "source-out-of-range": "源号超出该 PLIC 的源数",
    overridden: "被同键的后一条连线顶掉了（一个键一根线）",
});

/**
 * 归一化**内核给的**连线报告（**不判定** · 只做形状整理）。
 *
 * ⚠️ **缺省那档**（老内核 / 老事件**没有**这个字段）⇒ ⇒ 返回 `null`
 * ⇒ ⇒ 视图**不炸**（显示「内核没有给出连线报告」那档）⇒ ⇒ **N-4**。
 *
 * ⚠️ **恒等式不在这儿判**：内核侧保证 `declared == live + dropped.length`
 * ⇒ ⇒ 前端**不另算**（那会是第二个真相源）⇒ ⇒ 只**显示**内核给的数。
 *
 * @param {object|null|undefined} raw — 内核给的 `wiring_report`
 * @returns {{declared: number, live: number, dropped: Array<{from_device: string, event: string, to_device: string, source: number, reason: string}>}|null}
 */
export function normalizeWiringReport(raw) {
    if (!raw || typeof raw !== "object") {
        return null;
    }
    const dropped = Array.isArray(raw.dropped) ? raw.dropped : [];
    return {
        declared: Number(raw.declared ?? 0),
        live: Number(raw.live ?? 0),
        dropped: dropped.map((one) => ({
            from_device: String(one?.from_device ?? ""),
            event: String(one?.event ?? ""),
            to_device: String(one?.to_device ?? ""),
            source: Number(one?.source ?? 0),
            reason: String(one?.reason ?? ""),
        })),
    };
}

/**
 * 一个理由词的**标签**（**原词** + 一句白话）。
 *
 * ⚠️ **原词一定在**（`code`）⇒ ⇒ 界面显示的白话**替代不了**内核的原文
 * ⇒ ⇒ 用户能拿它去对内核的日志 / 报告。
 *
 * ⚠️ **未知词 ⇒ 白话就是原词** ⇒ ⇒ **不编** ⇒ ⇒ 内核将来加了新理由时，
 * 前端**如实显示那个词**。
 *
 * @param {string} reason — 内核给的理由词
 * @returns {{code: string, text: string}}
 */
export function wireDropReasonLabel(reason) {
    const code = String(reason ?? "");
    return { code, text: WIRE_DROP_REASON_TEXT[code] ?? code };
}

/**
 * 连线报告的**一行摘要**。
 *
 * ⚠️ **数字全部来自内核**（`declared` / `live`）⇒ ⇒ 前端**不重算**。
 *
 * @param {object|null} report — 归一化后的报告（`null` ⇒ 没有报告）
 * @returns {string}
 */
export function describeWiringReport(report) {
    if (!report) {
        return "内核没有给出连线报告（这份内核可能还没有连线报告这条通道）。";
    }
    const declared = Number(report.declared ?? 0);
    const live = Number(report.live ?? 0);
    const dropped = Array.isArray(report.dropped) ? report.dropped.length : 0;
    if (declared === 0) {
        return "连线：这块板没有写连线（0 根）。";
    }
    if (dropped === 0) {
        return `连线：写了 ${declared} 根，全部通电（${live} 根在用）。`;
    }
    return `连线：写了 ${declared} 根，其中 ${dropped} 根当前抬不了（${live} 根在用）。`;
}

/**
 * 一根**抬不了**的线 → **一行文字**（原词 + 白话 + 两端 + 源号）。
 *
 * ⚠️ **两端与源号都原样显示** ⇒ ⇒ 用户能照着改描述。
 *
 * @param {object} one — 归一化后的一根丢线
 * @returns {string}
 */
export function describeDroppedWire(one) {
    const { code, text } = wireDropReasonLabel(one?.reason);
    const from = String(one?.from_device ?? "");
    const to = String(one?.to_device ?? "");
    const event = String(one?.event ?? "");
    const source = Number(one?.source ?? 0);
    return `${from} · ${event} → ${to} · 源 ${source} · ${code} —— ${text}`;
}
