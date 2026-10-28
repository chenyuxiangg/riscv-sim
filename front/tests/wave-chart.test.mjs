// 波形真图（echarts）的前端门禁（STAGE-36 · v0.51 批 · A-1 ~ A-16）。
//
// ⚠️ **分层**：这一套验**纯函数与渲染形态**（Node 侧）；**真交互**（真的画出
//   图、真降级、真恢复）由 `tools/browser-verify.mjs` 的 `waveChart` 段
//   （B-27 ~ B-32）负责 ⇒ ⇒ 两边合起来才是完整的判据。
//
// ⚠️ **本批的第一约束是「稳定钩子一字不动」**（裁决 A）⇒ ⇒ A-14 / A-15 盯的
//   就是「真图是**另一张**卡片，**波形视图**的通道列表一个字都不许改」。
//
// ⚠️ **两个数不是同一个数**（裁决 B）：`edges` = 边沿**总数**（内核口径，
//   累加、不受上限影响）、`samples` = **画在图上的条数**（每通道最近 512 条）
//   ⇒ ⇒ A-2 / A-3 / A-16 盯的就是「这两个数不许混成一个」。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器 + 元素作用域的 `querySelectorAll`**
//   + `closest()`。
//
// ⚠️ **判据必须正向**：先钉住「被观测的对象存在」（集合非空），再逐条断言它
//   的内容 ⇒ ⇒ 集合为空时不会「零断言假绿」。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

import { WAVE_SAMPLE_LIMIT, mergeEdges } from "../lib/wave-samples.js";
import {
    ECHARTS_SOURCE_OFF,
    ECHARTS_URL,
    getChartSource,
    getECharts,
    loadECharts,
    setChartSource,
} from "../lib/echarts-loader.js";
import {
    WAVE_CHART_DEGRADED_TEXT,
    buildWaveOption,
    canDrawChart,
    waveChartHtml,
} from "../views/components/wave-chart.js";
import { _all } from "../views/m02_viewcomponents.js";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

// ⚠️ **DOM 全局必须先于前端模块存在**（同 `ui-layout.test.mjs` /
// `view-content.test.mjs`）⇒ ⇒ 这里只需要一个「能把 HTML 片段变成元素树」
// 的替身 ⇒ ⇒ 断言全在**元素作用域**里做。
const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ---------------------------------------------------------------------------
// 「有没有真的去 import」的**可观测探针**（A-13 用 · v0.53 批）
// ---------------------------------------------------------------------------

/**
 * 全局计数器的名字。
 *
 * ⚠️ **为什么需要它**：加载器的「关掉」这条路上**不发请求**这件事，
 *   在外面**看不到** ⇒ ⇒ 以前只能靠「再要一次还是同一个 promise」间接推断
 *   ⇒ ⇒ 那是**实现细节** ⇒ ⇒ 判据会随实现重构而误红 ⇒ ⇒ 换个**直接**的量：
 *   被 import 的模块**自己**把计数器加一 ⇒ ⇒ 计数器不动就是「没 import 过」。
 */
const IMPORT_COUNTER = "__v053ChartImportCount";

/**
 * 一个**能加载成功**、且在被 import 的那一刻**自增计数器**的来源。
 *
 * ⚠️ 用 `data:` URL ⇒ ⇒ 零网络、零夹具文件、零第三方依赖。
 * ⚠️ Node 的 ESM 加载器支持 `data:` URL ⇒ ⇒ 动态 `import()` 能真的加载它。
 */
const COUNTING_SOURCE =
    `data:text/javascript,globalThis.${IMPORT_COUNTER}`
    + "=(globalThis." + IMPORT_COUNTER + "||0)+1;"
    + "export default { init: () => {} };";

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

/**
 * 造一条内核推来的边沿。
 *
 * ⚠️ **`t_ps` 在线上是十进制字符串**（契约层 §2.2 · STAGE-36 事实 F3）⇒ ⇒
 * 这里一律用字符串喂 ⇒ ⇒ 「非法形态」才有意义。
 *
 * @param {number} t —— 虚拟时间（ps）
 * @param {number} value —— 信号值
 * @returns {{t_ps: string, value: number}}
 */
function edge(t, value) {
    return { t_ps: String(t), value };
}

/**
 * 造一条流（契约层 `model::WaveformStream` 的线上形态）。
 *
 * @param {string} id —— 通道 id
 * @param {Array<object>} edges —— 边沿
 * @returns {{id: string, label: string, edges: Array<object>}}
 */
function stream(id, edges) {
    return { id, label: id, edges };
}

/**
 * 造一个通道条目（**视图状态**里的形状，STAGE-36 §5.1.4）。
 *
 * @param {string} id —— 通道 id
 * @param {number} edges —— 边沿**总数**
 * @param {Array<{t: number, v: number}>} samples —— 保留下来能画图的样本
 * @returns {{id: string, label: string, edges: number, samples: Array<object>}}
 */
function channel(id, edges, samples) {
    return { id, label: id, edges, samples };
}

/**
 * 渲染**波形视图**并把它变成元素树。
 *
 * ⚠️ **为什么走 `render(state)` 而不是静态扫源码**：真机门禁判的是**渲染出来
 *   的东西** ⇒ ⇒ Node 侧也得判**渲染出来的东西** ⇒ ⇒ 源码文本只能当补充，
 *   不能当主判据（源码里有 ≠ 界面上有）。
 *
 * @returns {object} 元素树的根
 */
function renderWaveView() {
    const view = _all().find((c) => c.id === "waveform");
    assert.ok(view, "波形视图组件必须注册着（m02 的注册表里找不到 id=waveform）");
    const html = view.render({
        wave: {
            channels: [channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }])],
            chart: { status: "ok" },
        },
    });
    const root = dom.document.createElement("div");
    root.innerHTML = html;
    return root;
}

// ---------------------------------------------------------------------------
// A-1 ~ A-4 · 边沿累加（纯函数 · 零 DOM）
// ---------------------------------------------------------------------------

describe("波形样本 · 边沿累加与截断（A-1 ~ A-4）", () => {
    test("A-1 · mergeEdges(null, stream) 给出通道条目，样本是 {t, v} 且 t 是数字", () => {
        const ch = mergeEdges(
            null,
            stream("GPIO0.OUT", [edge(10, 1), edge(20, 0)]),
        );
        assert.equal(ch.id, "GPIO0.OUT", "通道 id 必须带过来");
        assert.equal(ch.label, "GPIO0.OUT", "通道名必须带过来");
        assert.equal(ch.edges, 2, "edges 是这条流的边沿条数");
        assert.equal(ch.samples.length, 2, "每条合法边沿一条样本");
        assert.deepEqual(
            ch.samples,
            [{ t: 10, v: 1 }, { t: 20, v: 0 }],
            "样本的形状必须是 {t, v}",
        );
        assert.equal(
            typeof ch.samples[0].t,
            "number",
            "t 必须是**数字**（不是线上那个十进制字符串）",
        );
    });

    test("A-2 · 累加时 edges 是总数、samples.length 是 min(总数, 512) —— 两个数独立", () => {
        const first = mergeEdges(
            null,
            stream("GPIO0.OUT", [edge(10, 0), edge(20, 1), edge(30, 0)]),
        );
        assert.equal(first.edges, 3, "第一批的 edges");
        assert.equal(first.samples.length, 3, "第一批的样本数");

        const second = mergeEdges(
            first,
            stream("GPIO0.OUT", [edge(40, 1), edge(50, 0)]),
        );
        assert.equal(second.edges, 5, "edges 必须**累加**成总数（不是覆盖）");
        assert.equal(
            second.samples.length,
            Math.min(second.edges, WAVE_SAMPLE_LIMIT),
            "样本条数 = min(总数, 上限) —— 上限是 512",
        );
        assert.equal(second.samples.length, 5);
        assert.deepEqual(
            second.samples.map((s) => s.t),
            [10, 20, 30, 40, 50],
            "两批样本必须接在一起（时间轴不许跳）",
        );
        // ⚠️ **纯函数**（裁决 C）：入参 `prev` 不许被就地改掉。
        assert.equal(first.edges, 3, "prev 不能被就地改掉（纯函数）");
        assert.equal(first.samples.length, 3, "prev 的样本不能被就地改掉（纯函数）");
    });

    test("A-3 · 600 条边沿 ⇒ 只留最近 512 条（第一条是原第 89 条），但 edges 仍是 600", () => {
        const edges = [];
        for (let i = 1; i <= 600; i += 1) edges.push(edge(i * 10, i));
        const ch = mergeEdges(null, stream("GPIO0.OUT", edges));
        assert.equal(ch.edges, 600, "edges 是总数，不受保留上限影响");
        assert.equal(
            ch.samples.length,
            WAVE_SAMPLE_LIMIT,
            "样本上限必须真的生效",
        );
        assert.equal(ch.samples.length, 512, "上限 = 512 条");
        assert.deepEqual(
            ch.samples[0],
            { t: 890, v: 89 },
            "留下的是**最近**的 512 条（第一条 = 原第 89 条）",
        );
        assert.deepEqual(
            ch.samples[ch.samples.length - 1],
            { t: 6000, v: 600 },
            "最后一条必须是最新那条",
        );
    });

    test("A-4 · 非法 t_ps 的那条边沿被丢弃，但 edges 仍 +1，其它边沿不受影响", () => {
        const ch = mergeEdges(
            null,
            stream("GPIO0.OUT", [
                edge(10, 0),
                // ⛔ 不是十进制整数字符串 ⇒ ⇒ `psToNumber` 抛 ⇒ ⇒ 这条被丢弃。
                //    （`Number("1e+21")` 会静默变成 1e21 ⇒ ⇒ 那是一条**假边沿**。）
                { t_ps: "1e+21", value: 1 },
                edge(30, 0),
            ]),
        );
        assert.equal(
            ch.edges,
            3,
            "edges 仍按内核口径 +1（那条边沿也是内核报的一条边沿）",
        );
        assert.equal(ch.samples.length, 2, "非法 t_ps 的那条**不进样本**");
        assert.deepEqual(
            ch.samples,
            [{ t: 10, v: 0 }, { t: 30, v: 0 }],
            "其它边沿不受影响（一条坏边沿不许把整条流带崩）",
        );
    });
});

// ---------------------------------------------------------------------------
// A-5 ~ A-10 · option 与卡片片段（纯函数 · 零 DOM）
// ---------------------------------------------------------------------------

describe("波形图 · option 与卡片片段（A-5 ~ A-10、A-16）", () => {
    test("A-5 · buildWaveOption 的 series 数据**逐条**等于喂进去的样本", () => {
        const samples = [{ t: 0, v: 0 }, { t: 10, v: 1 }, { t: 20, v: 0 }];
        const opt = buildWaveOption([channel("GPIO0.OUT", 3, samples)]);
        assert.equal(opt.series.length, 1, "一个可画通道 ⇒ 一条 series");
        const data = opt.series[0].data;
        assert.ok(Array.isArray(data), "series.data 必须是数组");
        assert.equal(
            data.length,
            samples.length,
            "图上的点数 = 样本数（**不是**边沿总数）",
        );
        for (let i = 0; i < samples.length; i += 1) {
            assert.deepEqual(
                data[i],
                [samples[i].t, samples[i].v],
                `第 ${i} 个点必须是该样本的 [t, v]`,
            );
        }
        assert.deepEqual(data, [[0, 0], [10, 1], [20, 0]], "整体逐条相等");
    });

    test("A-6 · 多通道 ⇒ series 的数量与顺序 = 通道的数量与顺序", () => {
        const chans = [
            channel("GPIO0.OUT", 2, [{ t: 10, v: 1 }, { t: 20, v: 0 }]),
            channel("UART0.TX", 1, [{ t: 30, v: 1 }]),
            channel("SPI0.SCK", 3, [{ t: 40, v: 0 }, { t: 50, v: 1 }, { t: 60, v: 0 }]),
        ];
        const opt = buildWaveOption(chans);
        assert.equal(opt.series.length, chans.length, "series 数量 = 通道数量");
        for (let i = 0; i < chans.length; i += 1) {
            assert.ok(
                opt.series[i].name.startsWith(chans[i].label),
                `第 ${i} 条 series 必须是通道 ${chans[i].label}（顺序不许乱）`,
            );
            assert.deepEqual(
                opt.series[i].data,
                chans[i].samples.map((s) => [s.t, s.v]),
                `第 ${i} 条 series 的数据必须是**对应通道**的样本`,
            );
        }
    });

    test("A-7 · x 轴是 value 轴（⛔ 不是 category 轴）", () => {
        const opt = buildWaveOption([
            channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }]),
        ]);
        assert.ok(opt.xAxis, "option 必须有 xAxis");
        assert.equal(
            opt.xAxis.type,
            "value",
            "时间轴必须是 value 轴 —— category 轴会把不等长间隔画成等距（时序失真）",
        );
    });

    test("A-8 · 每条 series 都是阶梯线（step = end）", () => {
        const opt = buildWaveOption([
            channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }]),
            channel("UART0.TX", 1, [{ t: 20, v: 0 }]),
        ]);
        assert.equal(opt.series.length, 2, "先钉住被观测的集合非空");
        for (let i = 0; i < opt.series.length; i += 1) {
            assert.equal(
                opt.series[i].step,
                "end",
                `第 ${i} 条 series 必须是阶梯线（M-19 §4.19 的 staircase）`,
            );
        }
    });

    test("A-9 · 空通道 ⇒ series 是空数组（**不抛**）", () => {
        let opt = null;
        assert.doesNotThrow(() => {
            opt = buildWaveOption([]);
        }, "空通道不许抛");
        assert.ok(opt, "必须返回一个 option");
        assert.ok(Array.isArray(opt.series), "series 必须是数组");
        assert.equal(opt.series.length, 0, "没有可画通道 ⇒ 没有 series");
    });

    test("A-10 · 降级文案：degraded 含「波形库不可用」，ok 不含", () => {
        const degraded = waveChartHtml({ status: "degraded", channels: [] });
        assert.ok(
            degraded.includes(WAVE_CHART_DEGRADED_TEXT),
            `降级时必须说出「波形库不可用」：${degraded}`,
        );
        assert.ok(
            degraded.includes('data-wave-chart-status="degraded"'),
            "降级分支必须自报状态",
        );

        const ok = waveChartHtml({
            status: "ok",
            channels: [channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }])],
        });
        assert.ok(
            ok.includes('data-wave-chart-status="ok"'),
            `正常分支必须自己声明状态：${ok}`,
        );
        assert.ok(
            !ok.includes(WAVE_CHART_DEGRADED_TEXT),
            `正常时不该出现降级文案：${ok}`,
        );
    });

    test("A-16 · 显示条数与总数**分开标注**（series 名里两个数）", () => {
        // ⚠️ 故意让两个数**不等**（样本 3 条、边沿总数 7 条）⇒ ⇒ 只带一个数
        //   的实现必然红。
        const ch = channel("GPIO0.OUT", 7, [
            { t: 10, v: 0 },
            { t: 20, v: 1 },
            { t: 30, v: 0 },
        ]);
        const opt = buildWaveOption([ch]);
        assert.equal(opt.series.length, 1, "先钉住被观测的 series 存在");
        const name = String(opt.series[0].name);
        assert.ok(
            name.startsWith("GPIO0.OUT"),
            `series 名必须带通道名：${name}`,
        );
        const pair = /（(\d+)\/(\d+)）/.exec(name);
        assert.ok(
            pair,
            `series 名必须同时带「显示条数/总数」两个数（例如 GPIO0.OUT（3/7））：${name}`,
        );
        assert.equal(
            pair[1],
            String(ch.samples.length),
            "第一个数是**画在图上的条数**",
        );
        assert.equal(pair[2], String(ch.edges), "第二个数是**边沿总数**");
    });
});

// ---------------------------------------------------------------------------
// A-11 ~ A-13 · 波形库加载器（失败不抛 · 来源可覆盖）
// ---------------------------------------------------------------------------

describe("波形库加载器 · 加载失败与来源切换（A-11 ~ A-13）", () => {
    test("A-11 · 加载失败**不抛**：loadECharts() 返回 null 而不是炸掉整条流", async () => {
        // ⚠️ 显式回到默认来源 ⇒ ⇒ 这条判的是「默认来源加载不了」的语义。
        setChartSource("");
        const mod = await loadECharts();
        assert.equal(
            mod,
            null,
            "Node 的 ESM 加载器不支持 https: 方案 ⇒ ⇒ 必须返回 null 而不是抛",
        );
        assert.equal(getECharts(), null, "没加载好时不得留下半个实例");
    });

    test("A-12 · URL 唯一落点：那个 CDN URL 字面量在 echarts-loader.js 里恰好出现一次", () => {
        const src = readFileSync(
            resolve(frontDir, "lib/echarts-loader.js"),
            "utf8",
        );
        const hits = src.split(ECHARTS_URL).length - 1;
        assert.equal(
            hits,
            1,
            `CDN URL 字面量必须**恰好出现一次**（实际 ${hits} 次）—— `
            + "第二个来源就是「第二个真相」",
        );
    });

    test("A-13 · setChartSource(\"off\") ⇒ 不发请求（返回 null）、来源回读是 off、且能改回默认", async () => {
        // ⚠️ **「不发网络请求」怎么才能被观测到**（STAGE-38 · v0.53 批 · 施工期订正 7）：
        //   以前这条判据写的是「再要一次还是**同一个** promise」⇒ ⇒ 那钉的是
        //   **实现细节**（旧实现在「关掉」这条路上把 `pending` 留住 ⇒ ⇒ 复用它）
        //   ⇒ ⇒ 而 v0.53 批把「关掉」**提到最前**（修真缺陷：在途的旧加载会复活
        //   已关掉的库）⇒ ⇒ 那个实现细节**必然**被打破 ⇒ ⇒ 判据会红。
        //   ⇒ ⇒ **换成直接可观测的量**：一个 `data:` 模块在被 import 的那一刻
        //   把全局计数器加一 ⇒ ⇒ 计数器不动 ⇒ ⇒ 就是「一次都没 import 过」。
        //   ⇒ ⇒ **这条判据比原来那条强**：原来那条钉的是「槽位被留住」，
        //   现在这条钉的是「**真的没发请求**」—— 正是用例名里承诺的那件事。
        globalThis[IMPORT_COUNTER] = 0;

        // ⚠️ **先让一个「真能加载」的源加载成功** ⇒ ⇒ 计数 = 1 ⇒ ⇒
        //   「没有 import」这条判据才有**基线**（否则 0 也可能是「压根没触发」）。
        setChartSource(COUNTING_SOURCE);
        assert.ok(
            await loadECharts(),
            "对照：一个可加载的来源必须真的加载到东西",
        );
        assert.equal(
            globalThis[IMPORT_COUNTER],
            1,
            "对照：这一次加载确实 import 了一次（计数 = 1）",
        );

        // ⚠️ **关掉**
        setChartSource(ECHARTS_SOURCE_OFF);
        assert.equal(getChartSource(), "off", "来源回读必须是 off");

        const first = loadECharts();
        assert.equal(await first, null, "关掉 ⇒ 不加载（返回 null）");
        assert.equal(getECharts(), null, "关掉 ⇒ 什么都没加载");
        assert.equal(
            globalThis[IMPORT_COUNTER],
            1,
            "关掉之后不许再去 import（否则就是「关了没关」）",
        );

        // ⚠️ **对照**：换一个**真能加载**的来源 ⇒ ⇒ 加载器确实会去加载 ⇒ ⇒
        //   上面的 null 是「关掉了」的结果，不是「这个加载器永远不加载」。
        const local = pathToFileURL(
            resolve(frontDir, "lib/echarts-loader.js"),
        ).href;
        setChartSource(local);
        const loaded = await loadECharts();
        assert.ok(loaded, "换一个可加载的来源 ⇒ 必须真的加载到东西");

        // ⇒ ⇒ 「关掉再打开」不是单向：能改回默认值。
        setChartSource("");
        assert.equal(getChartSource(), ECHARTS_URL, "必须能改回默认来源");
    });
});

// ---------------------------------------------------------------------------
// A-14 ~ A-15 · 波形视图的渲染（稳定钩子）
// ---------------------------------------------------------------------------

describe("波形视图 · 渲染的稳定钩子（A-14 ~ A-15）", () => {
    test("A-14 · 通道列表仍是 <li> + 「N edges」文本（⛔ 不含任何新加的计数后缀）", () => {
        const root = renderWaveView();
        const lists = root.querySelectorAll(".channel-list");
        assert.equal(
            lists.length,
            1,
            "通道列表必须还在 —— 真图是**另一张**卡片，不是把列表改掉",
        );
        const items = lists[0].querySelectorAll("li");
        assert.equal(items.length, 1, "通道列表里必须有一个条目（先钉住非空）");
        const text = items[0].textContent.replace(/\s+/g, " ").trim();
        assert.equal(
            text,
            "GPIO0.OUT 1 edges",
            "通道条目的文本必须一字不动 —— 任何新加的计数后缀"
            + "（例如「（1/1）」）都是破坏稳定钩子",
        );
    });

    test("A-15 · 真图是另一张卡片：输出里有 [data-wave-chart-slot]，且它不在 .channel-list 里", () => {
        const root = renderWaveView();
        const slots = root.querySelectorAll("[data-wave-chart-slot]");
        assert.ok(slots.length >= 1, "波形图插槽必须存在（先钉住被观测的对象）");
        const slot = slots[0];
        const lists = root.querySelectorAll(".channel-list");
        assert.equal(lists.length, 1, "通道列表必须还在");
        assert.equal(
            slot.closest(".channel-list"),
            null,
            "波形图插槽不许塞在通道列表里（两块必须分得开）",
        );
        assert.equal(
            lists[0].querySelectorAll("[data-wave-chart-slot]").length,
            0,
            "通道列表里不许有图表插槽",
        );
    });
});

// ---------------------------------------------------------------------------
// A-17 ~ A-19 · 施工期订正的判别力（宿主搬移 · 主题颜色 · 图例）
// ---------------------------------------------------------------------------

describe("施工期订正 · 宿主搬移 / 主题颜色 / 图例（A-17 ~ A-19）", () => {
    test("A-17 · 持久图表宿主**搬进插槽**：插槽**留着**、宿主是插槽的**子节点**", () => {
        // ⚠️ **为什么这条存在**（STAGE-36 施工期订正 1）：第一版实现是「宿主
        //   append 到插槽的**父节点**、再把插槽 removeChild 掉」⇒ ⇒ 插槽没了
        //   ⇒ ⇒ `front/style.css` §16 的 `.wave-chart-slot > *` 定位与
        //   `min-height` **都不生效** ⇒ ⇒ 宿主 **0 高** ⇒ ⇒ 图**看不见**。
        const view = _all().find((c) => c.id === "waveform");
        assert.ok(view, "波形视图组件必须注册着");
        const state = {
            wave: {
                channels: [channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }])],
                chart: { status: "ok" },
            },
        };
        const root = dom.document.createElement("div");
        root.innerHTML = view.render(state);

        // ⚠️ **先钉住被观测的对象存在**（插槽必须在）⇒ ⇒ 集合为空时不会
        //   「零断言假绿」。
        const slots = root.querySelectorAll("[data-wave-chart-slot]");
        assert.equal(slots.length, 1, "波形图插槽必须存在（先钉住非空）");
        const slot = slots[0];

        view.afterRender(root, state);

        // ⚠️ **插槽必须还在**：它承载 `min-height` 与 `.wave-chart-slot > *`
        //   的定位参照 ⇒ ⇒ 删掉它 ⇒ ⇒ 宿主 0 高 ⇒ ⇒ 图看不见。
        const after = root.querySelectorAll("[data-wave-chart-slot]");
        assert.equal(
            after.length,
            1,
            "插槽必须**留着**（删掉它 ⇒ 宿主 0 高 ⇒ 图看不见）",
        );
        assert.equal(
            after[0],
            slot,
            "必须是**同一个**插槽（不是重建了一个）",
        );

        // ⚠️ **宿主必须在插槽里面**（样式靠 `.wave-chart-slot > *` 定位它）。
        const hosts = slot.querySelectorAll("[data-wave-chart-host]");
        assert.equal(
            hosts.length,
            1,
            "持久图表宿主必须**在插槽里面**（先钉住被观测的对象存在）",
        );
        assert.equal(
            hosts[0].parentNode,
            slot,
            "宿主的父节点必须是插槽（不是插槽的父节点）",
        );
    });

    test("A-18 · 主题颜色**真的落进 option**（轴标签 / 图例 / 轴线 / 提示框）", () => {
        // ⚠️ **为什么这条存在**（STAGE-36 施工期订正 3）：echarts 画在 canvas 上
        //   ⇒ ⇒ canvas 里的文字**不受 CSS 管** ⇒ ⇒ 默认的深灰字压深底
        //   ⇒ ⇒ 看不见 ⇒ ⇒ 颜色必须**显式**喂给它。
        const theme = {
            text: "#a8b4cc",
            title: "#e6edf7",
            line: "#232f4d",
            tooltipBg: "#0d1526",
            tooltipText: "#e6edf7",
        };
        const opt = buildWaveOption(
            [channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }])],
            theme,
        );
        assert.equal(opt.textStyle?.color, theme.text, "全局文字色");
        assert.equal(opt.xAxis?.axisLabel?.color, theme.text, "x 轴标签色");
        assert.equal(opt.yAxis?.axisLabel?.color, theme.text, "y 轴标签色");
        assert.equal(opt.xAxis?.nameTextStyle?.color, theme.title, "x 轴名色");
        assert.equal(opt.yAxis?.nameTextStyle?.color, theme.title, "y 轴名色");
        assert.equal(
            opt.xAxis?.axisLine?.lineStyle?.color,
            theme.line,
            "x 轴线色",
        );
        assert.equal(
            opt.xAxis?.splitLine?.lineStyle?.color,
            theme.line,
            "x 轴分割线色",
        );
        assert.equal(
            opt.tooltip?.backgroundColor,
            theme.tooltipBg,
            "提示框底色",
        );
        assert.equal(
            opt.tooltip?.textStyle?.color,
            theme.tooltipText,
            "提示框文字色",
        );

        // ⚠️ **不传 theme ⇒ ⇒ 不设颜色** ⇒ ⇒ 「颜色写死在本模块里」这种做法
        //   **结构上不可能** ⇒ ⇒ 令牌才是唯一真相。
        const plain = buildWaveOption(
            [channel("GPIO0.OUT", 1, [{ t: 10, v: 1 }])],
        );
        assert.equal(
            plain.textStyle,
            undefined,
            "不传主题就不许有全局文字色（不许硬编颜色）",
        );
        assert.equal(
            plain.xAxis?.axisLabel,
            undefined,
            "不传主题就不许有轴标签色（不许硬编颜色）",
        );
    });

    test("A-19 · **图例存在**（那两个数靠它才看得见）", () => {
        // ⚠️ **为什么这条存在**（STAGE-36 施工期订正 4）：`seriesName()` 把
        //   「显示条数 / 总数」写进 series 的 `name` ⇒ ⇒ 而 `name` **只有图例
        //   与提示框才显示** ⇒ ⇒ 没有图例 ⇒ ⇒ 「分开标注」在界面上**一个字都
        //   看不见** ⇒ ⇒ ⇒ 裁决 O 落不了地。
        const opt = buildWaveOption(
            [channel("GPIO0.OUT", 3, [{ t: 10, v: 1 }])],
            { text: "#a8b4cc" },
        );
        assert.ok(opt.legend, "必须有图例（那两个数靠它才看得见）");
        assert.equal(
            opt.legend.textStyle?.color,
            "#a8b4cc",
            "图例文字色也要跟着主题走（否则也是深字压深底）",
        );
        assert.equal(
            opt.legend.type,
            "scroll",
            "通道多了图例不许把图挤没（可滚动）",
        );
    });
});

// ---------------------------------------------------------------------------
// A-20 · 「量不到尺寸就不画」这条护栏（纯函数 ⇒ Node 侧能单独验）
// ---------------------------------------------------------------------------

describe("量不到尺寸就不画（A-20）", () => {
    test("A-20 · canDrawChart：量到正数才画，量到 0 / 量不到都**不画**", () => {
        // ⚠️ **为什么这条必须单独有判据**（STAGE-36 施工期订正 2）：面板不可见
        //   时容器量到的尺寸是 **0** ⇒ ⇒ 此时 `echarts.init` 会建一个 **0×0**
        //   的实例 ⇒ ⇒ 而且它**绑在容器上、echarts 不会自己重画** ⇒ ⇒
        //   ⇒ 等用户切到波形视图 ⇒ ⇒ 图**永远是空的** ⇒ ⇒ ⇒ 这条护栏
        //   ⇒ ⇒ **必须有判据**，否则它只是「看起来对」。
        assert.equal(canDrawChart(932, 258), true, "量到正数 ⇒ 画");
        assert.equal(canDrawChart(0, 258), false, "宽是 0 ⇒ 不画");
        assert.equal(canDrawChart(932, 0), false, "高是 0 ⇒ 不画");
        assert.equal(canDrawChart(0, 0), false, "两个都是 0 ⇒ 不画");
        // ⚠️ **量不到**（替身环境里 `clientWidth` 是 `undefined`）⇒ ⇒ 与
        //   「量到 0」**同义** ⇒ ⇒ 不许画。
        assert.equal(canDrawChart(undefined, undefined), false, "量不到 ⇒ 不画");
        assert.equal(canDrawChart(Number.NaN, 258), false, "量到 NaN ⇒ 不画");
    });
});
