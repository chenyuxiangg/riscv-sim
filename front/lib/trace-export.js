// 「把**整条**轨迹翻完」的分页收集（纯逻辑 · 不碰 DOM · 不碰服务层）。
//
// ## 为什么要有这个模块
//
// 内核的 `ExportTrace` 是**分块**的（一块 `EXPORT_DEFAULT_LIMIT` = 10 000 条）
// ⇒ ⇒ 「下载轨迹到本地」这个动作**不能**只发一条命令 ⇒ ⇒ 它要**顺着
// `next_offset` 一块一块翻完** ⇒ ⇒ 直到 `next_offset == total`。
//
// ⇒ ⇒ 这段「翻页」是**判定 + 协议** ⇒ ⇒ 抽成纯逻辑 ⇒ ⇒ Node 侧**零 DOM**就能测
//     （传一个假的 `fetchChunk` 进去即可）⇒ ⇒ 与 `front/lib/run-loop.js` 同一手法。
//
// ## 三条终止条件（缺一条就可能死循环）
//
// ① `next_offset >= total` ⇒ ⇒ 到末尾了；
// ② `next_offset <= offset` ⇒ ⇒ 内核**没有推进**（病态）⇒ ⇒ 停；
// ③ 块数超上限 ⇒ ⇒ 停。
//
// ⚠️ **上限是「保护」不是「策略」**：轨迹环的上限是 200 000 条、一块 10 000 条
//   ⇒ ⇒ 正常最多 **20** 块 ⇒ ⇒ 这个上限离得很远 ⇒ ⇒ 它只用来挡病态。

/**
 * 一次下载**最多**翻多少块（保护用，见上）。
 *
 * @type {number}
 */
export const TRACE_EXPORT_MAX_CHUNKS = 256;

/**
 * 把**整条**轨迹翻完 ⇒ ⇒ 拼成一份完整文本。
 *
 * ⚠️ **续导协议**：下一块的起点 = 上一块回包的 `next_offset` ⇒ ⇒ 一直翻到
 *   `next_offset == total` ⇒ ⇒ 累计条目数**恰好** == `total`（不重不漏）。
 *
 * @param {(offset: number) => Promise<{chunk?: unknown, next_offset?: unknown, total?: unknown}>} fetchChunk
 *   — 取一块的函数（由调用方注入 ⇒ ⇒ 本模块**不认识**服务层）
 * @param {{maxChunks?: number}} [opts]
 * @returns {Promise<{content: string, chunks: number, total: number, nextOffset: number, truncated: boolean}>}
 */
export async function collectTraceChunks(fetchChunk, opts = {}) {
    const maxChunks = Number(opts.maxChunks ?? TRACE_EXPORT_MAX_CHUNKS);
    /** @type {string[]} */
    const parts = [];
    let offset = 0;
    let total = 0;
    let nextOffset = 0;
    let chunks = 0;
    let truncated = false;
    for (;;) {
        const value = await fetchChunk(offset);
        const chunk = String(value?.chunk ?? "");
        if (chunk) parts.push(chunk);
        chunks += 1;
        const rawNext = Number(value?.next_offset ?? offset);
        const rawTotal = Number(value?.total ?? rawNext);
        nextOffset = Number.isFinite(rawNext) ? rawNext : offset;
        total = Number.isFinite(rawTotal) ? rawTotal : nextOffset;
        if (nextOffset >= total) break;      // ① 到末尾
        if (nextOffset <= offset) {          // ② 内核没推进（病态）
            truncated = true;
            break;
        }
        if (chunks >= maxChunks) {           // ③ 超上限
            truncated = true;
            break;
        }
        offset = nextOffset;
    }
    return { content: parts.join(""), chunks, total, nextOffset, truncated };
}
