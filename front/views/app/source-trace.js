// App 层 · 源码树 / 波形库来源脚本口族（从 app.js 抽出）。
//
// 本模块先收编只依赖视图状态（M-03）与公共视图派发辅助的源码树读写、
// 波形库来源暴露；依赖命令路由 / 运行态计数的脚本命令口在后续批次迁入。
import * as viewStore from "../m03_viewstore.js";
import { publishViewState } from "./shared.js";

/**
 * 把一棵源码树**规整**成「路径 → 内容」的对象。
 *
 * ⚠️ **接受 `Map`**（任务书指定的形态）、**也**接受普通对象与
 * `[key, value]` 对的数组 ⇒ 调用方不必为了喂它而改自己的数据结构。
 *
 * @param {Map<string, string>|Record<string, string>|Iterable<[string, string]>|null|undefined} tree
 * @returns {Record<string, string>}
 */
export function normalizeSourceTree(tree) {
    /** @type {Record<string, string>} */
    const out = {};
    if (!tree) return out;
    // `Map`（有 `get` / `forEach`）。
    if (typeof tree.get === "function" && typeof tree.forEach === "function") {
        tree.forEach((value, key) => {
            out[String(key)] = String(value ?? "");
        });
        return out;
    }
    // `[key, value]` 对的数组。
    if (Array.isArray(tree)) {
        for (const pair of tree) {
            if (!Array.isArray(pair) || pair.length < 2) continue;
            out[String(pair[0])] = String(pair[1] ?? "");
        }
        return out;
    }
    // 普通对象。
    if (typeof tree === "object") {
        for (const [key, value] of Object.entries(tree)) {
            out[String(key)] = String(value ?? "");
        }
    }
    return out;
}

/**
 * 把一棵源码树写进 **M-03 视图状态**（**不进内核**）。
 *
 * ⚠️ **并入**已有的树，不丢用户已经装好的文件（与文件选择器那条路
 * 同一口径）。
 *
 * @param {Map<string, string>|Record<string, string>|Iterable<[string, string]>} tree
 * @returns {number} 树里的文件数
 */
export function setSourceTree(tree) {
    const normalized = normalizeSourceTree(tree);
    viewStore.setState("source", (prev) => ({
        ...prev,
        source: {
            ...(prev.source ?? {}),
            tree: { ...(prev.source?.tree ?? {}), ...normalized },
        },
    }));
    publishViewState("source");
    return Object.keys(normalized).length;
}

/**
 * 把可编程的源码树入口暴露到 `window.riscv_source`。
 *
 * ⚠️ **这是给真机脚本用的可编程入口，不是界面按钮**：
 * 真机脚本（`tools/browser-verify.mjs`）没法操作文件选择器 ⇒ 需要一条
 * **可编程**的路把源码树送进前端。
 *
 * @returns {void}
 */
export function exposeSourceLoader() {
    window.riscv_source = {
        setTree: (tree) => setSourceTree(tree),
    };
}

/**
 * 把「**波形库来源**」暴露给页面上的脚本（STAGE-36 · v0.51 批 · 裁决 G）。
 *
 * ⚠️ **这不是「测试后门」**，是**能力协商**：离线环境、或用户不想为一个图
 * 拉一兆字节的时候 ⇒ ⇒ 关掉波形图 ⇒ ⇒ 界面如实显示「波形库不可用」
 * ⇒ ⇒ **通道列表仍在** ⇒ ⇒ 核心调试闭环不受影响。
 *
 * ⚠️ **为什么必须有它**（判别力）：降级路径**必须可达** ⇒ ⇒ 真机门禁
 * **主动**触发降级 ⇒ ⇒ **降级逻辑永远有人管** ⇒ ⇒ 不能只靠「碰巧没网」。
 *
 * @param {{setChartSource: (next: string) => string, chartSource: () => string}} waveform
 * @returns {void}
 */
export function exposeWaveChartCommands(waveform) {
    window.riscv_waveform = {
        /** 当前波形库来源（默认是那个 CDN URL）。 */
        chartSource: () => waveform.chartSource(),
        /**
         * 换波形库来源（`"off"` = 关掉波形图；给 URL = 换源；给空 = 回到默认）。
         *
         * @param {string} next
         * @returns {string} 换之后的来源。
         */
        setChartSource: (next) => waveform.setChartSource(next),
        /**
         * 各通道的**计数快照**（`edges` = 边沿总数 / `samples` = 留着的样本数）。
         *
         * ⚠️ **只读、只给数字** ⇒ ⇒ 门禁拿它跟「界面上画了几个点」比对 ⇒ ⇒
         *   两个**独立来源**互相印证 ⇒ ⇒ 界面上报错了就会红。
         *
         * @returns {Array<{id:string,label:string,edges:number,samples:number}>}
         */
        channelStats: () => waveform.channelStats(),
    };
}
