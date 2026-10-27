// front/lib/wave-samples.js —— 波形边沿的**纯函数**（M-19 / STAGE-36 · v0.51 批）。
//
// ⚠️ **为什么把它从 app 层剥出来**（STAGE-36 裁决 C）：
//   边沿的累加与截断是**纯数据**的事 ⇒ ⇒ 剥成纯函数 ⇒ ⇒ **Node 侧零 DOM 就能测**
//   ⇒ ⇒ 与 v0.31 批「差量合成剥成纯函数」**同形** ⇒ ⇒ 「两条路径变成同一个纯函数的
//   两种调用 ⇒ ⇒ 等价性变成一条断言」那个先例一样。
//
// ⚠️ **这个模块零副作用**：不碰 DOM、不读全局、不写任何模块级状态
//   ⇒ ⇒ 全部状态由**调用方**拿着 ⇒ ⇒ 没有「第二个真相」。
//
// ⚠️ **两个数不是同一个数**（STAGE-36 裁决 B）：
//   - `edges`  = **边沿总数**（内核口径，**累加**，**不受**保留上限影响）；
//   - `samples` = **保留下来能画图的边沿**（每通道**最近 N 条**，超出丢最旧的）。
//   ⇒ ⇒ 界面上必须**分开标注** ⇒ ⇒ 长跑时这两个数**必然不同**。

import { psToNumber } from "./message.js";

/**
 * 每通道保留的边沿样本上限。
 *
 * ⚠️ **为什么有上限**：内核每通道的环有它自己的容量（M-19 §1.1：2×10⁵ 事件）
 * ⇒ ⇒ 前端**不能**把全部边沿都留在内存里 ⇒ ⇒ 长跑时前端只留**最近**的一段。
 *
 * ⚠️ **为什么是 512**：够画出一段有意义的时序，又不至于让前端的内存占用随运行时长
 * 线性增长 ⇒ ⇒ 且它**小于**内核的容量 ⇒ ⇒ 「图上的点数」与「边沿总数」**天然不同**
 * ⇒ ⇒ 界面上必须分开标注（裁决 B / 裁决 O）。
 */
export const WAVE_SAMPLE_LIMIT = 512;

/**
 * 把内核推来的一批边沿并进一个通道条目。
 *
 * ⚠️ **纯函数**：只读 `prev`，只写返回值 ⇒ ⇒ 原来的 `prev` **不被改** ⇒ ⇒
 * 调用方（app 层）自己决定要不要替换掉 Map 里那一份。
 *
 * @param {{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}|null} prev
 *   —— 上一轮的通道条目（第一次是 `null`）。
 * @param {{id:string,label:string,edges:Array<{t_ps:string|number,value:number}>}} stream
 *   —— 内核推来的一个流（契约层 `model::WaveformStream` 的线上形态）。
 * @param {number} [limit]
 *   —— 保留多少条样本（缺省 `WAVE_SAMPLE_LIMIT`）。
 * @returns {{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}}
 */
export function mergeEdges(prev, stream, limit = WAVE_SAMPLE_LIMIT) {
    const id = String(stream?.id ?? prev?.id ?? "");
    const label = String(stream?.label ?? prev?.label ?? id);
    const incoming = Array.isArray(stream?.edges) ? stream.edges : [];

    // ⚠️ **`edges` 是总数**（内核口径）⇒ ⇒ **累加**，**不受**保留上限影响。
    const total = (Number(prev?.edges) || 0) + incoming.length;

    // ⚠️ **`t_ps` 必须走 `psToNumber`**（STAGE-36 裁决 D）—— 契约层 §2.2 规定
    //   虚拟时间在线上是**十进制字符串**，而 `psToNumber` 对「不是十进制整数字符串」
    //   与「超过 2^53」都**抛** ⇒ ⇒ ⛔ **不许** `Number(str)` 硬转（那会把
    //   `"1e+21"` 静默变成 1e21 ⇒ ⇒ 时间轴上是一条**假边沿**）。
    // ⇒ ⇒ 一条边沿的 `t_ps` 非法 ⇒ ⇒ **丢弃那一条**并继续（**不让整条流崩掉**）
    // ⇒ ⇒ 但 `edges` 计数**照加**（那是内核说的边沿数）。
    const samples = [...(prev?.samples ?? [])];
    for (const edge of incoming) {
        let t;
        try {
            t = psToNumber(edge?.t_ps);
        } catch (_err) {
            continue; // ⚠️ 这一条没有可用的时间戳 ⇒ ⇒ 不进样本
        }
        samples.push({ t, v: Number(edge?.value ?? 0) });
    }

    // ⚠️ **保留最近的** ⇒ ⇒ 超出上限时丢**最旧**的（STAGE-36 裁决 B）。
    // ⇒ ⇒ 与内核侧「截断保留最新」的语义一致（STAGE-36 事实 F6）。
    const kept = samples.length > limit ? samples.slice(samples.length - limit) : samples;

    return { id, label, edges: total, samples: kept };
}

/**
 * 从通道条目里取出「可画图」的通道（**至少有一条样本**的）。
 *
 * ⚠️ **为什么要这一步**：一个通道可能**只有名字、没有边沿**（内核订阅时会推一次
 * 空边沿的波形，通道就在里面）⇒ ⇒ 那条通道**没有点可画** ⇒ ⇒ 不该出现在图上
 * ⇒ ⇒ 但它**仍然要在通道列表里**（用户要知道这条信号存在）。
 *
 * @param {Array<{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}>} channels
 * @returns {Array<{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}>}
 */
export function chartableChannels(channels) {
    const list = Array.isArray(channels) ? channels : [];
    return list.filter((c) => Array.isArray(c?.samples) && c.samples.length > 0);
}
