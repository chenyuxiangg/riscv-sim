// 波形视图面板（M-02 · waveform）—— 拆分自 m02_viewcomponents.js。
//
// 管 `waveformView` 注册组件：探针通道列表 + 持久宿主上的 echarts 边沿回放。

import { escapeHtml } from "../../lib/escape.js";
// ⚠️ **波形图的纯函数 + 片段**（STAGE-36）⇒ option 构造不碰 DOM ⇒
// Node 侧零 DOM 就能测「画的是不是真数据」。
import {
    buildWaveOption,
    holdPointCount,
    canDrawChart,
    groupIrqChannels,
    mergeIrqSelection,
    resolveWaveTheme,
    visibleChannels,
    waveChartHtml,
} from "./wave-chart.js";
// ⚠️ **波形库加载器的同步读**（STAGE-36）⇒ 视图层只读已加载好的模块 ⇒
// **加载**由 app 层做，视图层不发起跨源 import。
import { getECharts } from "../../lib/echarts-loader.js";
import { register, emitCommand } from "../component-registry.js";

const waveformView = register({
    id: "waveform",
    channels: ["wave"],
    /**
     * **持久图表宿主**（STAGE-36 · 裁决 L）。
     *
     * ⚠️ 视图每次渲染都把 `body.innerHTML` 整个换掉 ⇒ 视图里的 DOM 每次都被
     * 重建 ⇒ echarts 实例挂在容器元素上、容器没了实例也就没了 ⇒ 每次都重建
     * **会闪**。所以这里持有**一个** `<div>`（不是用户状态 ⇒ 不住视图状态），
     * 每次渲染把它**搬**进新插槽 ⇒ 实例始终绑在同一个元素上 ⇒ 不重建、不闪。
     *
     * @type {HTMLElement|null}
     */
    _chartHost: null,
    /**
     * **已初始化的 echarts 实例**（同一个宿主上只有一个 ⇒ 复用，不重建）。
     *
     * @type {object|null}
     */
    _chart: null,
    /**
     * **上次绘制时的宿主尺寸**（`"宽x高"`）⇒ ⇒ 变了就 `resize()`。
     *
     * @type {string|null}
     */
    _chartSize: null,
    /** 中断通道筛选状态（键 = 流 id ⇒ ⇒ 平台变更不丢选择）。 */
    _selected: null,
    /** 上一次的通道表（重画时不重新取 view state）。 */
    _lastChannels: [],
    render(state) {
        const wave = state?.wave ?? {};
        const channels = wave.channels ?? [];
        const selected = mergeIrqSelection(waveformView._selected, channels);
        waveformView._selected = selected;
        waveformView._lastChannels = channels;
        return `
            <section class="waveform">
                <div class="card">
                    <div class="row">
                        <h3>波形 (M-19)</h3>
                        <span class="tag">${channels.length} 个通道</span>
                        <button type="button" data-action="export-trace" data-source="Waveform" data-format="Vcd" data-offset="0" data-limit="0" data-whole="1">导出波形 (VCD)</button>
                    </div>
                    ${channels.length === 0
                        ? `<p class="muted">尚无探针信号 — 固件驱动外设后（M-15 + M-18 管道已接通）会列出通道。</p>`
                        : `<ul class="channel-list">
                            ${channels.map((c) => `
                                <li>
                                    <span class="channel-label">${escapeHtml(c.label || c.id)}</span>
                                    <span class="muted">${c.edges ?? 0} edges</span>
                                    ${c.lastValue === null || c.lastValue === undefined
                                        ? ""
                                        : `<span class="muted">· 最新值 ${escapeHtml(String(c.lastValue))}</span>`}
                                </li>`).join("")}
                           </ul>`}
                    <p class="muted">通道列表里的边沿数是**总数**；下面的图只画每个通道最近的一段。</p>
                    <p class="muted">导出的波形内容显示在「<strong>调试视图 → trace 记录 / 导出</strong>」卡片里（下载也在那儿）。</p>
                </div>
                <div class="card">
                    <div class="row"><h3>波形图</h3><span class="tag">M-19 · 边沿回放</span></div>
                    ${waveChartHtml({
                        status: wave.chart?.status ?? "idle",
                        channels,
                        selected,
                    })}
                </div>
            </section>
        `;
    },
    /**
     * 渲染**之后**做的事（STAGE-36 · 裁决 M）。
     *
     * ⚠️ 这一步只做两件事：把**持久宿主**搬进插槽 +（波形库就绪时）初始化 /
     * 更新图表 ⇒ **不碰**任何别的视图的状态。
     *
     * @param {HTMLElement} body
     * @param {object} state
     */
    afterRender(body, state) {
        const slot = body?.querySelector?.("[data-wave-chart-slot]") ?? null;
        if (!slot) return; // ⚠️ 降级态**没有**插槽 ⇒ ⇒ 什么都不用做。

        // ⚠️ **搬宿主**：把上一次那个 `<div>` 搬进这次的插槽 ⇒ 容器还是同一个
        // ⇒ echarts 实例**不用重建**。
        //
        // ⚠️⚠️ **插槽必须留下**（STAGE-36 施工期订正）—— `front/style.css` §16
        //   用 `.wave-chart-slot > *` 给宿主**绝对定位铺满** ⇒ 把插槽删掉 ⇒
        //   宿主既没有 `min-height` 也没有定位参照 ⇒ **0 高** ⇒ 画出来是 0 高
        //   ⇒ **看不见**。所以宿主**进插槽**、插槽**留着**。
        if (!waveformView._chartHost) {
            waveformView._chartHost = document.createElement("div");
            waveformView._chartHost.className = "wave-chart-host";
            waveformView._chartHost.setAttribute("data-wave-chart-host", "");
        }
        slot.appendChild(waveformView._chartHost);

        // ⚠️ **波形库还没好** ⇒ 这一轮**不画** ⇒ 等 app 层把状态写成「ok」
        // 并重新渲染 ⇒ 那一轮才画（降级时状态是「degraded」⇒ 插槽压根不存在）。
        const echarts = getECharts();
        if (!echarts) return;

        const host = waveformView._chartHost;
        const w = host.clientWidth;
        const h = host.clientHeight;
        // ⚠️⚠️ **量不到尺寸就不许 `init`**（施工期订正）—— 面板没显示时
        //   `.hidden { display: none }` ⇒ `clientWidth/Height` 是 **0** ⇒
        //   此时 `echarts.init` 会量到 0 ⇒ 画出来是 0 高 ⇒ 而且它绑在容器上、
        //   之后不会自己重画 ⇒ 图**永远是空的**。量到 0 就**这一轮不画**。
        //
        // ⚠️ **判据本身是纯函数**（`canDrawChart`）⇒ Node 侧能单独验。
        if (!canDrawChart(w, h)) return;

        // ⚠️ 图上只画**筛选后**的通道（默认只画核中断）。
        const channels = visibleChannels(state?.wave?.channels ?? [], waveformView._selected);
        // ⚠️ **颜色从 CSS 令牌来**（resolveWaveTheme）⇒ echarts 画在 canvas 上
        // ⇒ canvas 里的文字**不受 CSS 管** ⇒ 必须显式把令牌颜色喂给 echarts。
        const theme = resolveWaveTheme(
            typeof document !== "undefined" ? document.documentElement : null,
        );
        const option = buildWaveOption(channels, theme);
        if (!waveformView._chart) {
            waveformView._chart = echarts.init(host);
        } else if (waveformView._chartSize !== `${w}x${h}`) {
            // ⚠️ **尺寸变了要 resize** ⇒ 否则 echarts 一直按旧尺寸画。
            waveformView._chart.resize();
        }
        waveformView._chartSize = `${w}x${h}`;
        waveformView._chart.setOption(option, true);

        // ⚠️ **自报**的两个属性（真机门禁读它们）⇒ 「界面上画了几个点」是
        // **界面自己说的** ⇒ 真机判据还要拿它跟**视图状态里的样本数**比对。
        const drawable = option.series.length;
        // ⚠️ **点数口径**：`data-wave-chart-points` 只报**真样本**（= 视图状态
        //   里的 `samples` 之和）⇒ 补出来的**保持点**单独报 `data-wave-chart-hold-points`。
        const points = channels.reduce(
            (n, c) => n + (Array.isArray(c?.samples) ? c.samples.length : 0),
            0,
        );
        host.setAttribute("data-wave-chart-points", String(points));
        host.setAttribute("data-wave-chart-hold-points", String(holdPointCount(channels)));
        host.setAttribute("data-wave-chart-series", String(drawable));
        host.setAttribute(
            "data-wave-chart-status",
            String(state?.wave?.chart?.status ?? "idle"),
        );
        // ⚠️ **自报**：真正喂给 echarts 的**文字色** ⇒ 真机门禁拿它跟
        // `--tx2` 的计算值比对。
        host.setAttribute(
            "data-wave-chart-text-color",
            String(option?.textStyle?.color ?? ""),
        );
    },
    /**
     * 用**当前筛选**重画图表（勾选是纯前端动作，不重新取 view state）。
     *
     * ⚠️ 同时刷新自报属性（`data-wave-chart-points/series`）⇒ 筛选之后**仍有判别力**。
     */
    _redrawWaveChart() {
        const chart = waveformView._chart;
        if (!chart) return;
        const theme = resolveWaveTheme(
            typeof document !== "undefined" ? document.documentElement : null,
        );
        const visible = visibleChannels(
            waveformView._lastChannels ?? [],
            waveformView._selected,
        );
        chart.setOption(buildWaveOption(visible, theme), true);
        const host = waveformView._chartHost;
        if (host) {
            const points = visible.reduce(
                (n, c) => n + (Array.isArray(c?.samples) ? c.samples.length : 0),
                0,
            );
            host.setAttribute("data-wave-chart-points", String(points));
            host.setAttribute("data-wave-chart-hold-points", String(holdPointCount(visible)));
            host.setAttribute("data-wave-chart-series", String(visible.length));
        }
    },
    onCommand(ev) {
        const el = ev.target?.closest?.("[data-action]");
        const action = el?.dataset?.action;
        if (!action) return undefined;
        // ⚠️ 单通道勾选：`el.checked` 已被浏览器翻过 ⇒ 直接写进选择表再重画。
        if (action === "toggle-wave-channel") {
            const id = String(el.dataset?.id ?? "");
            if (!id) return undefined;
            const sel = { ...(waveformView._selected ?? {}) };
            sel[id] = el.checked !== false;
            waveformView._selected = sel;
            waveformView._redrawWaveChart();
            return undefined;
        }
        // ⚠️ 三个快捷：全选 / 仅核 / 清空（只作用于**中断通道**）。
        if (action === "wave-select") {
            const scope = String(el.dataset?.scope ?? "all");
            const sel = { ...(waveformView._selected ?? {}) };
            for (const group of groupIrqChannels(waveformView._lastChannels ?? [])) {
                for (const c of group.channels) {
                    const id = String(c.id);
                    sel[id] =
                        scope === "all" ? true : scope === "none" ? false : group.key === "core";
                }
            }
            waveformView._selected = sel;
            const root = el.closest?.("[data-wave-chart]");
            const boxes = root?.querySelectorAll?.("[data-action='toggle-wave-channel']");
            for (const box of boxes ?? []) {
                box.checked = sel[String(box.dataset?.id ?? "")] !== false;
            }
            waveformView._redrawWaveChart();
            return undefined;
        }
        // ⚠️ **导出波形**（v0.63）⇒ 参数**随信号走**；导出结果落在**调试视图**
        // 的 trace 卡片里（那里是导出内容的**唯一**落点 ⇒ 不是第二个真相）。
        if (action === "export-trace") {
            return emitCommand("export-trace", "waveform", {
                source: el.dataset.source,
                format: el.dataset.format,
                offset: el.dataset.offset,
                limit: el.dataset.limit,
                // ⚠️ **v0.89**：界面上**只有**「导出」入口；点「导出」＝ 把
                // **整份**翻完并落盘；`whole` 就是这个意思。
                whole: el.dataset.whole,
            });
        }
        return undefined;
    },
    onDelta(_delta) {},
    mountFor(el) {
        // ⚠️ **监听器绑在容器上**（事件委托）⇒ 每次渲染重建 DOM 也不会丢。
        el.addEventListener("click", (ev) => waveformView.onCommand(ev));
    },
});
