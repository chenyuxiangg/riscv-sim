// v0.78 —— 中断波形通道：分组、默认筛选、平台变更合并。
//
// 用户拍板（2026-11-18）：
//   1. 波形要展示软中断 / 定时器 / 所有已接线外设；
//   2. 前端要能筛选，**默认只勾核中断**；
//   3. 产品视图新增器件后波形自动适应，且**不丢已有勾选**。
//
// 这三条都落在 `wave-chart.js` 的**纯函数**上，所以这里零 DOM 也能钉死。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const url = pathToFileURL(resolve(here, "../views/components/wave-chart.js")).href;

const {
    groupIrqChannels,
    defaultIrqSelection,
    mergeIrqSelection,
    visibleChannels,
    channelFilterHtml,
} = await import(url);

const CHANNELS = [
    { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: 3, samples: [] },
    { id: "irq.core.software", label: "software · IRQ 3", edges: 2, samples: [] },
    { id: "irq.core.timer", label: "timer · IRQ 7", edges: 9, samples: [] },
    { id: "irq.core.external", label: "external · MEIP", edges: 1, samples: [] },
    { id: "irq.dev.uart0", label: "uart0 · IRQ 10", edges: 7, samples: [] },
    { id: "irq.dev.gpio0", label: "gpio0 · IRQ 11", edges: 4, samples: [] },
];

describe("中断通道分组", () => {
    test("按流 id 前缀分成核 / 外设，非中断通道不进组", () => {
        const groups = groupIrqChannels(CHANNELS);
        assert.equal(groups.length, 2);
        assert.equal(groups[0].title, "核中断");
        assert.deepEqual(
            groups[0].channels.map((c) => c.id),
            ["irq.core.software", "irq.core.timer", "irq.core.external"],
        );
        assert.equal(groups[1].title, "外设中断");
        assert.deepEqual(
            groups[1].channels.map((c) => c.id),
            ["irq.dev.uart0", "irq.dev.gpio0"],
        );
    });
});

describe("默认筛选：只勾核中断", () => {
    test("核中断默认开，外设默认关，非中断通道保持可见", () => {
        const sel = defaultIrqSelection(CHANNELS);
        assert.equal(sel["irq.core.software"], true);
        assert.equal(sel["irq.core.timer"], true);
        assert.equal(sel["irq.core.external"], true);
        assert.equal(sel["irq.dev.uart0"], false);
        assert.equal(sel["irq.dev.gpio0"], false);
        assert.equal(sel["GPIO0.OUT"], true);
    });

    test("默认筛选后图上只有核中断 + GPIO，没有外设中断", () => {
        const sel = defaultIrqSelection(CHANNELS);
        const visible = visibleChannels(CHANNELS, sel).map((c) => c.id);
        assert.deepEqual(visible, [
            "GPIO0.OUT",
            "irq.core.software",
            "irq.core.timer",
            "irq.core.external",
        ]);
    });

    test("显式勾上外设后它出现在图上", () => {
        const sel = { ...defaultIrqSelection(CHANNELS), "irq.dev.uart0": true };
        const visible = visibleChannels(CHANNELS, sel).map((c) => c.id);
        assert.ok(visible.includes("irq.dev.uart0"));
    });
});

describe("平台变更：选择按流 id 保留", () => {
    test("新增器件补进选择表，已有勾选不变", () => {
        const prev = { ...defaultIrqSelection(CHANNELS), "irq.dev.uart0": true };
        const after = CHANNELS.concat([
            { id: "irq.dev.gpio1", label: "gpio1 · IRQ 12", edges: 0, samples: [] },
        ]);
        const merged = mergeIrqSelection(prev, after);
        assert.equal(merged["irq.dev.uart0"], true, "用户的勾选必须保留");
        assert.equal(merged["irq.dev.gpio1"], false, "新器件按默认（不勾）");
    });

    test("没有历史选择时就是默认", () => {
        const merged = mergeIrqSelection(null, CHANNELS);
        assert.equal(merged["irq.core.timer"], true);
        assert.equal(merged["irq.dev.uart0"], false);
    });
});

describe("筛选区 HTML", () => {
    test("两组都在，复选框按选择表勾选", () => {
        const html = channelFilterHtml(CHANNELS, defaultIrqSelection(CHANNELS));
        assert.match(html, /核中断/);
        assert.match(html, /外设中断/);
        assert.match(html, /data-action="toggle-wave-channel"/);
        assert.match(html, /data-id="irq.dev.uart0"/);
        // 外设默认不勾：它那一条的 input 不带 checked
        const uartRow = html.slice(html.indexOf('data-id="irq.dev.uart0"'));
        const before = html.slice(0, html.indexOf('data-id="irq.dev.uart0"'));
        const inputStart = before.lastIndexOf("<input");
        const inputEnd = html.indexOf(">", inputStart);
        const tag = html.slice(inputStart, inputEnd);
        assert.ok(!tag.includes("checked"), `外设默认不勾：${tag}`);
        assert.ok(uartRow.length > 0);
    });

    test("没有中断通道时不渲染筛选区", () => {
        const html = channelFilterHtml(
            [{ id: "GPIO0.OUT", label: "GPIO0.OUT", edges: 1, samples: [] }],
            null,
        );
        assert.equal(html, "");
    });
});
