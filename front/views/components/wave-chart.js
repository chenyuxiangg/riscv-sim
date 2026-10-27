// front/views/components/wave-chart.js —— 波形图的**纯函数** + 卡片片段
// （STAGE-36 · v0.51 批 · 裁决 I）。
//
// ⚠️ **这个模块不 import 波形库加载器** —— 加载器里那个跨源 `import()` 在 Node 里
//   会失败 ⇒ ⇒ 若这里 import 它 ⇒ ⇒ Node 侧的判据就没法「零依赖」地跑纯函数
//   ⇒ ⇒ ⇒ **加载与构造分开**（加载器在 app 层用，构造在这里）。
//
// ⚠️ **纯函数**：不碰 DOM、不读全局 ⇒ ⇒ Node 侧零 DOM 就能测「画的是不是真数据」
//   ⇒ ⇒ 与 v0.31 批「差量合成剥成纯函数」同形。
//   ⚠️ **唯一的例外**是 `resolveWaveTheme` —— 它**只读** CSS 自定义属性、不写任何
//   状态 ⇒ ⇒ 它是**唯一的 DOM 读取口**（见下面「为什么文字色要单独解决」）。

import { escapeHtml } from "../../lib/escape.js";
import { chartableChannels, WAVE_SAMPLE_LIMIT } from "../../lib/wave-samples.js";

/**
 * 降级提示的文案（**唯一落点**）。
 *
 * ⚠️ **为什么要有它**：波形库不可达时，界面**必须**把这件事说出来 ⇒ ⇒
 *   「静默地什么都不画」比「明确说画不出来」**更糟**（用户会以为是数据没了）。
 */
export const WAVE_CHART_DEGRADED_TEXT = "波形库不可用";

/**
 * 图表主题的**槽位 → CSS 自定义属性**映射。
 *
 * ⚠️ **为什么颜色不直接写在这个模块里**（STAGE-36 施工期订正）：
 *   echarts 画在 **canvas** 上 ⇒ ⇒ canvas 里的文字**不受 CSS 管** ⇒ ⇒
 *   echarts 默认的文字色是**深灰**（约 `#333`）⇒ ⇒ 而插槽底色是 `--panel`
 *   （`#111a30`，深底）⇒ ⇒ ⇒ **深字压深底 ⇒ ⇒ 坐标轴标签 / 图例文字几乎看不见**
 *   ⇒ ⇒ 「图画出来了」但「读不出来」⇒ ⇒ 等于没画。
 *   ⇒ ⇒ 所以颜色**必须**由 echarts **显式**指定 ⇒ ⇒ 而指定成什么 ⇒ ⇒ 又不能
 *   在 JS 里另写一份颜色字面量（那会变成**第二个真相**）⇒ ⇒ ⇒ **只能从 `:root`
 *   的既有令牌读** ⇒ ⇒ ⇒ 本表就是「哪个槽位读哪个令牌」的**唯一**定义处。
 *
 * ⚠️ **注意**：`--panel2` / `--tx` 等是**既有**令牌 ⇒ ⇒ 本表**不新增**令牌
 *   ⇒ ⇒ `:root` **一个字都不用动**。
 */
export const WAVE_THEME_VARS = {
    /** 轴标签 / 图例文字（次级文字色）。 */
    text: "--tx2",
    /** 轴名字（主文字色，比标签亮一点）。 */
    title: "--tx",
    /** 轴线 / 分割线（网格线）。 */
    line: "--line",
    /** 提示框底色。 */
    tooltipBg: "--panel2",
    /** 提示框文字色。 */
    tooltipText: "--tx",
};

/**
 * 从 DOM 元素上解析出图表主题。
 *
 * ⚠️ **为什么要运行时解析**：`getComputedStyle(el).getPropertyValue("--tx2")`
 *   拿到的是 `:root` 里那个令牌的**值** ⇒ ⇒ ⇒ **令牌是唯一真相** ⇒ ⇒
 *   换主题时图**自动**跟着走 ⇒ ⇒ JS 里**零**颜色字面量。
 *
 * ⚠️ **读不到就返回空对象** ⇒ ⇒ `buildWaveOption` 不设颜色 ⇒ ⇒ echarts 用它
 *   自己的默认值 ⇒ ⇒ **不会抛** ⇒ ⇒ Node 侧（无 DOM）也安全。
 *
 * @param {Element|Document} [el] —— 解析的来源元素（默认 `document.documentElement`）
 * @returns {{text?:string, title?:string, line?:string, tooltipBg?:string, tooltipText?:string}}
 */
export function resolveWaveTheme(el) {
    const out = {};
    const root = el
        ?? (typeof document !== "undefined" ? document.documentElement : null);
    if (!root) return out;
    if (typeof getComputedStyle !== "function") return out;
    let cs = null;
    try {
        cs = getComputedStyle(root);
    } catch (_err) {
        return out;
    }
    if (!cs || typeof cs.getPropertyValue !== "function") return out;
    for (const [slot, varName] of Object.entries(WAVE_THEME_VARS)) {
        let v = "";
        try {
            v = String(cs.getPropertyValue(varName) ?? "").trim();
        } catch (_err) {
            v = "";
        }
        if (v) out[slot] = v;
    }
    return out;
}

/**
 * 现在**能不能**画图（量到尺寸才画）。
 *
 * ⚠️ **为什么剥成纯函数**（STAGE-36 施工期订正 2）：面板不可见时
 *   `.hidden { display: none }` ⇒ ⇒ 容器量到的尺寸是 **0** ⇒ ⇒ 此时
 *   `echarts.init` 会建一个 **0×0** 的实例 ⇒ ⇒ 而且**它绑在容器上、
 *   echarts 不会自己重画** ⇒ ⇒ 等用户切到波形视图 ⇒ ⇒ 图**永远是空的**
 *   ⇒ ⇒ ⇒ 所以**量不到尺寸就不画** ⇒ ⇒ 等面板显示出来、下一轮渲染再画。
 *
 * ⚠️ **为什么是纯函数**：这个判断**必须能被单独验** ⇒ ⇒ 剥出来 ⇒ ⇒
 *   Node 侧零 DOM 就能测 ⇒ ⇒ 与 `mergeEdges` / `buildWaveOption` 同形 ⇒ ⇒
 *   ⇒ 不然这条护栏只能靠「真机恰好撞上那个时序」才有人管 ⇒ ⇒ **零判别力**。
 *
 * ⚠️ **`undefined` / `NaN` 也算「量不到」** ⇒ ⇒ `!(w > 0)` 这种写法把
 *   「量到 0」与「量不到」**归成同一件事** ⇒ ⇒ 替身环境（dom-stub）里
 *   `clientWidth` 是 `undefined` ⇒ ⇒ 语义**一致**。
 *
 * @param {number} width —— 容器量到的宽
 * @param {number} height —— 容器量到的高
 * @returns {boolean}
 */
export function canDrawChart(width, height) {
    return width > 0 && height > 0;
}

/**
 * 一个通道在图例里的名字。
 *
 * ⚠️ **两个数**（STAGE-36 裁决 O）：`shown` = **图上画出来的条数**、
 *   `total` = **边沿总数** ⇒ ⇒ 长跑时两者**必然不同** ⇒ ⇒ 必须**分开标注**
 *   ⇒ ⇒ 界面上看不出「图被截断了」＝ 骗人。
 *
 * @param {string} label
 * @param {number} shown
 * @param {number} total
 * @returns {string}
 */
export function seriesName(label, shown, total) {
    return `${label}（${shown}/${total}）`;
}

/**
 * 通道条目 → echarts option。
 *
 * ⚠️ **纯函数**：只读入参、不碰 DOM、不读全局。
 *
 * ⚠️ **x 轴是 value 轴**（STAGE-36 裁决 J）—— 时间是**连续量** ⇒ ⇒ 用
 *   `category` 轴会让不等长的间隔**等距**画出来 ⇒ ⇒ 时序失真 ⇒ ⇒ 图是假的。
 *
 * ⚠️ **阶梯线**（STAGE-36 裁决 K）—— M-19 §4.19 说波形是**阶梯**
 *   （staircase，由边沿回放得出）⇒ ⇒ `step: "end"` ⇒ ⇒ **不是**斜线。
 *
 * ⚠️ **有图例**（STAGE-36 施工期订正）—— `seriesName` 里那**两个数**
 *   （显示条数 / 总数）**只有图例才看得见** ⇒ ⇒ 没有图例 ⇒ ⇒ 「分开标注」
 *   这条裁决就**落不了地** ⇒ ⇒ ⇒ 图例是**必需**的、不是装饰。
 *
 * @param {Array<{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}>} channels
 * @param {object} [theme] —— `resolveWaveTheme` 的结果（**可选** ⇒ ⇒ 缺省时
 *   不设颜色 ⇒ ⇒ echarts 用它自己的默认值 ⇒ ⇒ Node 侧零 DOM 也能调）
 * @returns {object} echarts option
 */
export function buildWaveOption(channels, theme) {
    const list = chartableChannels(channels);
    const th = theme && typeof theme === "object" ? theme : {};
    const text = String(th.text ?? "");
    const title = String(th.title ?? text);
    const line = String(th.line ?? "");
    const tipBg = String(th.tooltipBg ?? "");
    const tipText = String(th.tooltipText ?? text);

    const option = {
        // ⚠️ **不开动画** ⇒ ⇒ 边沿每来一批就重画一次 ⇒ ⇒ 动画会一直闪。
        animation: false,
        // ⚠️ `top` 给图例留位置 ⇒ ⇒ 图例压住曲线就是「看不清」。
        grid: { left: 58, right: 20, top: 44, bottom: 62 },
        tooltip: { trigger: "axis" },
        xAxis: {
            type: "value",
            name: "虚拟时间 (ps)",
            nameLocation: "middle",
            nameGap: 30,
            scale: true,
        },
        yAxis: { type: "value", name: "值", scale: true },
        // ⚠️ FR-10 要求波形视图能**缩放 / 平移** ⇒ ⇒ 这两个就是它。
        dataZoom: [
            { type: "inside", xAxisIndex: 0 },
            { type: "slider", xAxisIndex: 0 },
        ],
        // ⚠️ **图例必需** ⇒ ⇒ 那两个数（显示条数 / 总数）**在这里**看得见。
        legend: { type: "scroll", top: 0, left: 0, right: 0 },
        series: list.map((c) => ({
            name: seriesName(c.label || c.id, c.samples.length, c.edges),
            type: "line",
            step: "end",
            showSymbol: false,
            symbol: "none",
            lineStyle: { width: 1.5 },
            emphasis: { focus: "series" },
            // ⚠️ **数据就是样本本身**（`[t, v]`）⇒ ⇒ 图上画的是**真边沿**。
            data: c.samples.map((s) => [s.t, s.v]),
        })),
    };

    // ⚠️ **颜色全部来自 CSS 令牌**（`resolveWaveTheme` 运行时解析）⇒ ⇒
    //   ⇒ 这里**一个颜色字面量都没有** ⇒ ⇒ 令牌是**唯一**真相。
    if (text) {
        option.textStyle = { color: text };
        option.xAxis.axisLabel = { color: text };
        option.yAxis.axisLabel = { color: text };
        option.legend.textStyle = { color: text };
    }
    if (title) {
        option.xAxis.nameTextStyle = { color: title };
        option.yAxis.nameTextStyle = { color: title };
    }
    if (line) {
        option.xAxis.axisLine = { lineStyle: { color: line } };
        option.yAxis.axisLine = { lineStyle: { color: line } };
        option.xAxis.splitLine = { lineStyle: { color: line } };
        option.yAxis.splitLine = { lineStyle: { color: line } };
    }
    if (tipBg) {
        option.tooltip.backgroundColor = tipBg;
        option.tooltip.borderColor = line || tipBg;
        option.tooltip.textStyle = { color: tipText };
    }
    return option;
}

/**
 * 波形图卡片的 HTML 片段（按图表状态分支）。
 *
 * ⚠️ **降级分支与正常分支**是**两个不同的 DOM** ⇒ ⇒ 降级时**没有**图表插槽
 *   ⇒ ⇒ 不会有「一个空白的框在那儿装样子」。
 *
 * @param {{status?:string, channels?:Array, limit?:number}} chart
 * @returns {string}
 */
export function waveChartHtml(chart) {
    const status = String(chart?.status ?? "idle");
    const channels = Array.isArray(chart?.channels) ? chart.channels : [];
    const limit = Number(chart?.limit ?? WAVE_SAMPLE_LIMIT);

    if (status === "degraded") {
        return `
            <div class="wave-chart wave-chart-degraded" data-wave-chart data-wave-chart-status="degraded">
                <p class="wave-degraded">
                    <span class="wave-degraded-title">${WAVE_CHART_DEGRADED_TEXT}</span>
                    —— 外部波形库没加载到，图画不出来。
                </p>
                <p class="muted">通道列表仍在下面 ⇒ ⇒ 调试闭环不受影响；联网后刷新即可恢复。</p>
            </div>`;
    }

    const drawable = chartableChannels(channels).length;
    return `
        <div class="wave-chart" data-wave-chart data-wave-chart-status="${escapeHtml(status)}">
            <div class="wave-chart-slot" data-wave-chart-slot></div>
            <p class="muted">每个通道最多显示最近 ${limit} 条边沿；通道列表里的「N edges」是总数。</p>
            <p class="muted">${drawable > 0
                ? `${drawable} 个通道有边沿可画。`
                : "还没有可画的边沿 —— 先跑起来，固件驱动外设引脚才会有。"}</p>
        </div>`;
}
