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
 *   echarts 默认文字色偏深灰 ⇒ ⇒ 而插槽底色由令牌给（`--panel2`）⇒ ⇒
 *   深字压深底时坐标轴标签 / 图例文字会几乎看不见
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
/**
 * 有多少条通道会被补**一个**保持点（判据与 `holdToEnd` 完全一致）。
 *
 * ⚠️ 存在的理由：图上的点 = **真样本** + **保持点**，两者必须能分开报
 *   ⇒ ⇒ 门禁的「点数 = 样本数」那条判据才不会因为保持点变假 ⇒ ⇒ 界面得
 *   把「补了几个」单独说出来（`data-wave-chart-hold-points`）。
 *
 * @param {Array<{samples:Array<{t:number,v:number}>}>} channels
 * @returns {number}
 */
export function holdPointCount(channels) {
    const list = Array.isArray(channels) ? channels : [];
    let tEnd = Number.NEGATIVE_INFINITY;
    for (const c of list) {
        const sm = Array.isArray(c?.samples) ? c.samples : [];
        if (sm.length) tEnd = Math.max(tEnd, sm[sm.length - 1].t);
    }
    if (!Number.isFinite(tEnd)) return 0;
    let n = 0;
    for (const c of list) {
        const sm = Array.isArray(c?.samples) ? c.samples : [];
        if (sm.length && sm[sm.length - 1].t < tEnd) n += 1;
    }
    return n;
}

/**
 * 给一条通道的样本补**右端保持点**（见 `buildWaveOption` 里的长注释）。
 *
 * ⚠️ 只在「确实还没画到最右」时补；已经到最右（或一个样本都没有）⇒ 原样返回。
 * ⚠️ **不新增跳变**：`v` 取最后一个样本的值 ⇒ 绘画上是水平延续，不是新边沿。
 *
 * @param {Array<{t:number,v:number}>} samples
 * @param {number} tEnd —— 全局最右样本时间（ps）；可能为 `-Infinity`
 * @returns {Array<[number,number]>}
 */
export function holdToEnd(samples, tEnd) {
    const pts = samples.map((s) => [s.t, s.v]);
    if (pts.length === 0 || !Number.isFinite(tEnd)) return pts;
    const last = pts[pts.length - 1];
    if (last[0] < tEnd) pts.push([tEnd, last[1]]);
    return pts;
}

export function buildWaveOption(channels, theme) {
    const list = chartableChannels(channels);
    const th = theme && typeof theme === "object" ? theme : {};
    const text = String(th.text ?? "");
    const title = String(th.title ?? text);
    const line = String(th.line ?? "");
    const tipBg = String(th.tooltipBg ?? "");
    const tipText = String(th.tooltipText ?? text);

    // ⚠️ **保持点（hold-to-end）** —— 通道只在**电平变化**时才产生样本
    //   （注册表按值去重）⇒ ⇒ 一条「抬起来就没再落下」的线，其最后样本就是
    //   那一次上升沿 ⇒ ⇒ `step:"end"` 只在**样本之间**保持 ⇒ ⇒ 最后一个
    //   样本之后**什么都不画** ⇒ ⇒ 屏幕上只剩一小段、看着像「断了」。
    //   ⇒ ⇒ 给每条通道补**一个**右端保持点：`t` = 全局最右样本、`v` = 该通道
    //   最后的值。它**不是**编造的边沿（不新增跳变），只是「电平一直保持到
    //   这一刻」的显式表达 —— 图上点数 = 样本数 + 0/1，测试钉住这一点。
    const tEnd = list.reduce(
        (max, c) => (c.samples.length ? Math.max(max, c.samples[c.samples.length - 1].t) : max),
        Number.NEGATIVE_INFINITY,
    );

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
            // ⚠️ **数据就是样本本身**（`[t, v]`）⇒ ⇒ 图上画的是**真边沿**；
            //   末尾那一个「保持点」见上面的说明（不新增跳变）。
            data: holdToEnd(c.samples, tEnd),
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
 * 中断通道的**分组**（核中断 / 外设中断）。
 *
 * ⚠️ 判据是**流 id 前缀**（`irq.core.*` / `irq.dev.*`），不是 label 文本 ——
 *   label 会随语言/格式改，id 不会。
 *
 * @param {Array<{id:string,label:string,edges:number,samples:Array}>} channels
 * @returns {Array<{key:string,title:string,channels:Array}>}
 */
export function groupIrqChannels(channels) {
    const core = [];
    const dev = [];
    for (const c of channels || []) {
        const id = String(c?.id ?? "");
        if (id.startsWith("irq.core.")) core.push(c);
        else if (id.startsWith("irq.dev.")) dev.push(c);
    }
    return [
        { key: "core", title: "核中断", channels: core },
        { key: "dev", title: "外设中断", channels: dev },
    ].filter((g) => g.channels.length > 0);
}

/**
 * 默认筛选：**只勾核中断**（2026-11-18 用户拍板）。
 *
 * ⚠️ 非中断通道（`GPIO0.OUT` 之类）默认**可见** —— 它们本来就在图上，
 *   不该因为这次加筛选而消失。
 *
 * @param {Array<{id:string}>} channels
 * @returns {Object<string, boolean>}
 */
export function defaultIrqSelection(channels) {
    const sel = {};
    for (const c of channels || []) {
        const id = String(c?.id ?? "");
        sel[id] = !id.startsWith("irq.dev.");
    }
    return sel;
}

/**
 * 平台变更后**合并**筛选状态：已有的选择保留，新通道按默认值补上。
 *
 * ⚠️ 这就是「产品视图加器件后不丢选择」的落点：键是**流 id**，
 *   不是通道号。
 *
 * @param {Object<string, boolean>|null} prev
 * @param {Array<{id:string}>} channels
 * @returns {Object<string, boolean>}
 */
export function mergeIrqSelection(prev, channels) {
    const def = defaultIrqSelection(channels);
    if (!prev || typeof prev !== "object") return def;
    const out = { ...prev };
    for (const [id, on] of Object.entries(def)) {
        if (!(id in out)) out[id] = on;
    }
    return out;
}

/**
 * 按筛选结果过滤通道（未出现在选择表里的通道**保留**）。
 *
 * @param {Array} channels
 * @param {Object<string, boolean>|null} selected
 * @returns {Array}
 */
export function visibleChannels(channels, selected) {
    if (!selected || typeof selected !== "object") return channels || [];
    return (channels || []).filter((c) => selected[String(c?.id ?? "")] !== false);
}

/**
 * 通道筛选区的 HTML（复选框 + 全选/仅核/清空）。
 *
 * @param {Array} channels
 * @param {Object<string, boolean>|null} selected
 * @returns {string}
 */
export function channelFilterHtml(channels, selected) {
    const groups = groupIrqChannels(channels);
    if (groups.length === 0) return "";
    const row = (c) => {
        const id = String(c?.id ?? "");
        const on = selected?.[id] !== false;
        return `<label class="wave-filter-item">
            <input type="checkbox" data-action="toggle-wave-channel" data-id="${escapeHtml(id)}"${on ? " checked" : ""}>
            <span>${escapeHtml(String(c?.label ?? id))}</span>
            <span class="muted">${Number(c?.edges ?? 0)} edges</span>
        </label>`;
    };
    return `<div class="wave-filter" data-wave-filter>
        <div class="row">
            <h4>通道筛选</h4>
            <button type="button" data-action="wave-select" data-scope="all">全选</button>
            <button type="button" data-action="wave-select" data-scope="core">仅核</button>
            <button type="button" data-action="wave-select" data-scope="none">清空</button>
        </div>
        ${groups.map((g) => `<fieldset class="wave-filter-group">
            <legend>${escapeHtml(g.title)}</legend>
            ${g.channels.map(row).join("")}
        </fieldset>`).join("")}
    </div>`;
}

/**
 * 波形图卡片的 HTML 片段（按图表状态分支）。
 *
 * ⚠️ **降级分支与正常分支**是**两个不同的 DOM** ⇒ ⇒ 降级时**没有**图表插槽
 *   ⇒ ⇒ 不会有「一个空白的框在那儿装样子」。
 *
 * @param {{status?:string, channels?:Array, limit?:number, selected?:Object}} chart
 * @returns {string}
 */
export function waveChartHtml(chart) {
    const status = String(chart?.status ?? "idle");
    const channels = Array.isArray(chart?.channels) ? chart.channels : [];
    const limit = Number(chart?.limit ?? WAVE_SAMPLE_LIMIT);
    const selected = chart?.selected ?? null;

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

    const visible = visibleChannels(channels, selected);
    const drawable = chartableChannels(visible).length;
    return `
        <div class="wave-chart" data-wave-chart data-wave-chart-status="${escapeHtml(status)}">
            ${channelFilterHtml(channels, selected)}
            <div class="wave-chart-slot" data-wave-chart-slot></div>
            <p class="muted">每个通道最多显示最近 ${limit} 条边沿；通道列表里的「N edges」是总数。</p>
            <p class="muted">${drawable > 0
                ? `${drawable} 个通道有边沿可画。`
                : "还没有可画的边沿 —— 先跑起来，固件驱动外设引脚才会有。"}</p>
        </div>`;
}
