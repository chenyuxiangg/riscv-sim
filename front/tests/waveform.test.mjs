// D-46 —— 前端接真实探针流的门禁。
//
// 第 1 阶段结束时波形视图是**空的**：前端既没订阅内核的 `sim.waveform`，
// 也没有把探针流落进视图状态；就算收到了，`app.js` 里的过滤器写的也是
// 「只认 `probe.` 开头的 id」——而**真内核**的通道 id 是
// `"{外设 id}.{信号名}"`（`GPIO0.OUT`），会被**全部过滤掉**。
//
// 这个套件盯住三件事：
//
//   1. 过滤器认的是「**不是**寄存器流」，不是「是 `probe.` 前缀」；
//   2. 边沿是**累加**的（内核每次只推增量，覆盖的话视图上永远只有最后
//      一批的边沿数）；
//   3. 内核侧的 `sim.waveform` 订阅**会被强制重订**（`LoadProject` 会清
//      掉内核的主题集合 —— 和 D-45 是同一个坑）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
// The DOM globals must exist **before** the front-end modules are
// imported — `app.js` reads `document.getElementById("status")` at
// module scope.
installDom(indexHtml);

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");

/**
 * A command router double: records the commands and lets the test push
 * events on demand.
 *
 * @returns {{sent: object[], subscribe: Function, emit: Function, dispatchCommand: Function}}
 */
function fakeRouter() {
    /** @type {Map<string, Set<Function>>} */
    const subs = new Map();
    /** @type {object[]} */
    const sent = [];
    return {
        sent,
        subscribe(topic, handler) {
            if (!subs.has(topic)) subs.set(topic, new Set());
            subs.get(topic).add(handler);
            return () => subs.get(topic).delete(handler);
        },
        emit(topic, event) {
            for (const fn of subs.get(topic) ?? []) fn(event);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            return { value: {} };
        },
    };
}

/**
 * Build a `WaveformDelta` event value.
 *
 * @param {Array<{id: string, label?: string, edges: Array<{t_ps: number, value: number}>}>} streams
 * @returns {object}
 */
function waveformEvent(streams) {
    return {
        variant: "WaveformDelta",
        topic: "waveform.delta",
        value: {
            schema_version: 1,
            streams: streams.map((s) => ({
                id: s.id,
                label: s.label ?? s.id,
                edges: s.edges,
            })),
        },
    };
}

/**
 * 建一套干净的接线。
 *
 * ⚠️ 视图状态是**模块级单例**（M-03），用例之间是共享的。所以每个用例
 * 都要先 `reset()`——否则上一个用例留下的通道会漏进下一个用例的断言里
 * （「寄存器流不进波形视图」那条就是这么假绿过一次）。
 *
 * @returns {{router: object, waveform: object}}
 */
function setup() {
    const router = fakeRouter();
    const waveform = app.wireWaveform(router);
    waveform.reset();
    return { router, waveform };
}

/** The channels currently in the waveform view's state. */
function channels() {
    return m03.getState("waveform")?.wave?.channels ?? [];
}

// ---------------------------------------------------------------------------

describe("探针流 · 通道落进视图状态", () => {
    test("真内核的通道 id（`GPIO0.OUT`）进得了视图状态", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: [] },
            ]),
        );
        assert.equal(channels().length, 1, "探针通道必须出现");
        assert.equal(channels()[0].id, "GPIO0.OUT");
    });

    test("mock 的通道 id（`probe.led0`）也认（前端不绑定具体通道名）", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "probe.led0", label: "LED0", edges: [{ t_ps: 1, value: 1 }] },
            ]),
        );
        assert.equal(channels().length, 1);
        assert.equal(channels()[0].id, "probe.led0");
    });

    test("寄存器流（`cpu.gpr.x1` / `cpu.pc`）不进波形视图", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "cpu.gpr.x1", label: "x1", edges: [{ t_ps: 1, value: 42 }] },
                { id: "cpu.pc", label: "pc", edges: [{ t_ps: 1, value: 4 }] },
            ]),
        );
        assert.deepEqual(
            channels(),
            [],
            "寄存器流归 M-10 消费，不该出现在波形视图里",
        );
    });

    test("边沿是**累加**的（内核每次只推增量）", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: [{ t_ps: 1, value: 1 }] },
            ]),
        );
        assert.equal(channels()[0].edges, 1);
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: [{ t_ps: 2, value: 0 }] },
            ]),
        );
        assert.equal(
            channels()[0].edges,
            2,
            "第二批边沿必须**累加**上去，不能覆盖上一批",
        );
        assert.equal(channels()[0].lastValue, 0, "最新值跟着最后一条边沿走");
    });

    test("reset 之后旧通道不算数（新会话 = 新命名空间）", () => {
        const { router, waveform } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: [{ t_ps: 1, value: 1 }] },
            ]),
        );
        assert.equal(channels().length, 1);
        waveform.reset();
        assert.deepEqual(channels(), [], "reset 之后通道必须清空");
    });
});

describe("探针流 · 内核侧订阅", () => {
    test("订阅的是 `sim.waveform` 主题", async () => {
        const { router, waveform } = setup();
        await waveform.subscribeProbeStream();
        assert.equal(router.sent.length, 1);
        assert.deepEqual(router.sent[0], { Subscribe: { topic: "sim.waveform" } });
    });

    test("订阅是**强制**的（每次调用都发一条命令，不靠本地标志）", async () => {
        const { router, waveform } = setup();
        await waveform.subscribeProbeStream();
        await waveform.subscribeProbeStream();
        assert.equal(
            router.sent.length,
            2,
            "重订必须真的再发一条命令 —— 内核侧的订阅会随 LoadProject 一起失效",
        );
    });
});

describe("波形视图 · 渲染", () => {
    test("通道列表渲染出来了（含标签与边沿数）", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([
                { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: [{ t_ps: 1, value: 1 }] },
            ]),
        );
        const view = m02.get("waveform");
        const html = view.render(m03.getState("waveform"));
        assert.ok(html.includes("GPIO0.OUT"), `通道标签没渲染出来：${html}`);
        assert.ok(html.includes("1 edges"), `边沿数没渲染出来：${html}`);
    });

    test("没有通道时给出的是「尚无探针信号」而不是空白", () => {
        const { router } = setup();
        router.emit(
            "waveform.delta",
            waveformEvent([{ id: "cpu.gpr.x1", label: "x1", edges: [] }]),
        );
        const view = m02.get("waveform");
        const html = view.render(m03.getState("waveform"));
        assert.ok(html.includes("尚无探针信号"), `占位文案丢了：${html}`);
    });
});
