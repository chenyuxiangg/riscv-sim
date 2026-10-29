// 视图内容形态 + 断点 UI 的前端门禁（STAGE-35 · v0.50 批 · A-1 ~ A-16）。
//
// ⚠️ **分层**：这一套验**结构与状态落点**（Node 侧）；**真交互**（真点断点、
// 真在内核停下、真撤掉）由 `tools/browser-verify.mjs` 的 `breakpointUi` 段
// （B-19 ~ B-26）负责 ⇒ ⇒ 两边合起来才是完整的判据。
//
// ⚠️ **本批的第一约束是「稳定钩子一字不动」**（裁决 A）⇒ ⇒ A-4 / A-5 盯的
// 就是「换形态没有把判据的取法换掉」。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
// 简单复合选择器 ⇒ ⇒ 一律用**扁平选择器 + 元素作用域的 `querySelectorAll`**
// （`body.querySelectorAll(".card")` 是**按元素作用域**的 ⇒ ⇒ 合法）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（D-46 检视 V-B · probe D2）：`app.js` 是
// 引导入口——**导入即启动**，它的 `bootstrap()` 是异步的 ⇒ ⇒ 会在任意两个
// 用例之间动 M-03 的模块级单例 ⇒ ⇒ 套件不依赖时序运气。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const memoryMap = await importFront("views/components/memory-map.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ⚠️ **面板是 M-01 在挂载时**用 DOM 调用**造出来的**（不是 index.html 里的
// 静态标记）⇒ ⇒ 不挂工作台就**没有** `[data-view-body]` ⇒ ⇒ 断言会「视图
// 正文容器没挂上」⇒ ⇒ 这里显式挂一次。
await m01.mountWorkbench();
await new Promise((r) => setTimeout(r, 0));

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

/** 视图正文（渲染后）。 */
function body(viewId) {
    const el = dom.document.querySelector(`[data-view-body="${viewId}"]`);
    assert.ok(el, `视图 ${viewId} 的正文容器没挂上`);
    return el;
}

/** 路由到某个视图并让它渲染一次。 */
function show(viewId) {
    m01.route(`#/${viewId}`);
    return body(viewId);
}

/** 八个视图（F14 ⇒ 不增视图、不删视图）。 */
const VIEW_IDS = [
    "debug", "embedded", "regmem", "terminal",
    "waveform", "memory-map", "product", "source",
];

// ⚠️ **反汇编条目**（内核的 `debug.disasm` 事件的载荷形状）。
const DISASM = [
    { addr: 0x00, code: 0x00000013, mnemonic: "nop", width: 4, file: null, line: null },
    { addr: 0x04, code: 0x00000001, mnemonic: "addi", width: 4, file: null, line: null },
    { addr: 0x20, code: 0x0000006f, mnemonic: "jal", width: 4, file: null, line: null },
];

/** 把反汇编塞进**调试视图状态**（唯一真相 ⇒ 视图只读）。 */
function seedDisasm(entries = DISASM) {
    m03.setState("debug", (prev) => ({
        ...prev,
        sim: { ...(prev.sim ?? {}), disasm: entries.map((e) => ({ ...e })) },
    }));
}

/** 抓 `riscv:command` 信号（视图层只发信号 ⇒ 那是视图层的出口）。 */
function captureCommands(fn) {
    const captured = [];
    const handler = (ev) => captured.push(ev.detail);
    dom.window.addEventListener("riscv:command", handler);
    try {
        fn();
    } finally {
        dom.window.removeEventListener("riscv:command", handler);
    }
    return captured;
}

/** 一个只记录派发载荷的假 commandRouter。 */
function fakeServices(extra = {}) {
    const dispatched = [];
    return {
        dispatched,
        services: {
            commandRouter: {
                dispatchCommand: async (payload) => {
                    dispatched.push(payload);
                    return { value: null };
                },
                subscribe: () => () => {},
            },
            registerMemory: { readRegisters: async () => {} },
            waveform: {},
            debug: {},
            console: {},
            memoryMap: {},
            ...extra,
        },
    };
}

// ---------------------------------------------------------------------------

describe("STAGE-35 · 视图内容形态（卡片化）", () => {
    test("A-1 · 八个视图的正文**都非空**（8 / 8）", () => {
        // ⚠️ **先钉住件数** ⇒ ⇒ 否则「视图一个都没挂上」时这个 `for`
        // **一次都不执行** ⇒ ⇒ 用例**空转通过** ⇒ ⇒ **假绿**。
        assert.equal(VIEW_IDS.length, 8, "本批的视图清单必须是 8 个");
        for (const id of VIEW_IDS) {
            const el = show(id);
            assert.ok(
                el.children.length > 0,
                `视图 ${id} 的正文是空的（children.length === 0）`,
            );
            assert.ok(
                String(el.innerHTML).trim().length > 0,
                `视图 ${id} 的正文 innerHTML 是空的`,
            );
        }
    });

    test("A-2 · 每个视图正文里**至少一个** `.card`（内容形态确实卡片化）", () => {
        for (const id of VIEW_IDS) {
            const el = show(id);
            const cards = el.querySelectorAll(".card");
            assert.ok(
                cards.length >= 1,
                `视图 ${id} 的正文里没有 .card（卡片化没落地）`,
            );
        }
    });

    test("A-3 · 嵌入式总览是 `.cards` 网格，里面**至少 3 个** `.kpi`", () => {
        const el = show("embedded");
        const grids = el.querySelectorAll(".cards");
        assert.ok(grids.length >= 1, "嵌入式总览里没有 .cards 网格");
        const kpis = el.querySelectorAll(".kpi");
        assert.ok(kpis.length >= 3, `嵌入式总览的 .kpi 只有 ${kpis.length} 个（至少要 3 个）`);
        // ⚠️ **KPI 必须真的在那个网格里** ⇒ ⇒ 逐个查祖先。
        const inGrid = kpis.filter((k) => k.closest(".cards"));
        assert.equal(
            inGrid.length,
            kpis.length,
            "有 .kpi 不在 .cards 网格里（KPI 卡片没嵌进网格）",
        );
    });

    test("A-4 · 寄存器视图的 `.reg-grid > div` **仍是 32 个**（稳定钩子 · 裁决 A）", () => {
        const el = show("regmem");
        const grid = el.querySelector(".reg-grid");
        assert.ok(grid, "寄存器视图里没有 .reg-grid");
        const cells = grid.querySelectorAll("div");
        assert.equal(cells.length, 32, `.reg-grid 里的格子应当是 32 个，实际 ${cells.length}`);
    });

    test("A-5 · 波形视图的 `.channel-list > li` **仍在**（稳定钩子 · 裁决 A）", () => {
        m03.setState("waveform", (prev) => ({
            ...prev,
            wave: {
                ...(prev.wave ?? {}),
                channels: [{ id: "uart0.tx", label: "UART0 TX", edges: 3, lastValue: 7 }],
            },
        }));
        const el = show("waveform");
        const list = el.querySelector(".channel-list");
        assert.ok(list, "波形视图里没有 .channel-list");
        const items = list.querySelectorAll("li");
        assert.equal(items.length, 1, `通道行应当是 1 条，实际 ${items.length}`);
    });
});

describe("STAGE-35 · 断点 UI", () => {
    test("A-6 · 调试视图的反汇编表**有断点列**（每个 `tr.disasm-row` 里有一个 `[data-bp-toggle]`）", () => {
        seedDisasm();
        const el = show("debug");
        const rows = el.querySelectorAll("tr.disasm-row");
        assert.equal(rows.length, 3, `反汇编行应当是 3 条，实际 ${rows.length}`);
        for (const row of rows) {
            const toggles = row.querySelectorAll("[data-bp-toggle]");
            assert.equal(toggles.length, 1, "有一行没有断点按钮（断点列没落地）");
        }
    });

    test("A-7 · 断点列表**只有一处**落点：视图状态 `debug.breakpoints`（视图层不持有第二份）", async () => {
        seedDisasm();
        // ⚠️ **先钉住「列表真的被渲染出来了」** ⇒ ⇒ 否则「列表是空的」
        // 会让「视图层没存第二份」这条断言**恒真** ⇒ ⇒ **假绿**。
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), breakpoints: [0x20] },
        }));
        let el = show("debug");
        let rows = el.querySelectorAll("[data-bp-row]");
        assert.equal(rows.length, 1, `断点列表应当有 1 行，实际 ${rows.length}`);
        assert.equal(
            rows[0].getAttribute("data-bp-addr"),
            "0x00000020",
            "断点列表那一行的地址不对",
        );

        // ⚠️ **行为判据**：视图状态一改，列表**必须跟着改** ⇒ ⇒ 若视图层
        // 自己存了一份（第二个真相），这里会**继续显示旧地址** ⇒ ⇒ 变红。
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), breakpoints: [0x30] },
        }));
        el = show("debug");
        rows = el.querySelectorAll("[data-bp-row]");
        assert.equal(rows.length, 1, `换了一个断点地址之后列表应当还是 1 行，实际 ${rows.length}`);
        assert.equal(
            rows[0].getAttribute("data-bp-addr"),
            "0x00000030",
            "视图状态改了但列表还是旧地址 ⇒ ⇒ 视图层存了第二份（第二个真相）",
        );

        // ⚠️ **app 层是写的那一方** ⇒ ⇒ 视图层**不写** debug 视图状态。
        const { services } = fakeServices();
        await app.handleCommand({ action: "set-breakpoint", params: { addr: "0x00000040" } }, services);
        assert.deepEqual(
            m03.getState("debug")?.debug?.breakpoints,
            [0x30, 0x40],
            "app 层没有把断点写进 debug.breakpoints（唯一落点没接上）",
        );
        const m02Src = readFileSync(resolve(frontDir, "views/m02_viewcomponents.js"), "utf8");
        assert.ok(
            !/setState\(\s*"debug"/.test(m02Src),
            "m02 里写了 debug 视图状态 ⇒ ⇒ 视图层持有第二份断点列表 ⇒ ⇒ 第二个真相",
        );
        assert.ok(
            m02Src.includes("debug.breakpoints"),
            "m02 里连读都没读 debug.breakpoints（断点列表没接上唯一落点）",
        );
    });

    test("A-8 · 「加断点」发的是 `SetBreakpoint` 且 **`id === addr`**（裁决 J）", async () => {
        seedDisasm();
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), breakpoints: [] },
        }));
        const el = show("debug");
        const btn = el.querySelector('[data-bp-toggle="0x00000020"]');
        assert.ok(btn, "0x20 那行没有断点按钮");
        const signals = captureCommands(() => btn.click());
        assert.equal(signals.length, 1, `点一下应当只发一条命令信号，实际 ${signals.length}`);
        assert.equal(signals[0].action, "set-breakpoint", "点「加断点」发的 action 不对");

        // ⚠️ **命令的最终形状**要在 **app 层**验 ⇒ ⇒ 视图发信号、app 发命令。
        const { services, dispatched } = fakeServices();
        await app.handleCommand(signals[0], services);
        assert.equal(dispatched.length, 1, `应当派发一条命令，实际 ${dispatched.length}`);
        const bp = dispatched[0]?.SetBreakpoint;
        assert.ok(bp, `派发的不是 SetBreakpoint：${JSON.stringify(dispatched[0])}`);
        assert.equal(Number(bp.addr), 0x20, "断点地址不对");
        assert.equal(
            Number(bp.id),
            Number(bp.addr),
            `断点 id 必须是地址本身（裁决 J），实际 id=${bp.id} addr=${bp.addr}`,
        );
        assert.equal(bp.enabled, true, "断点必须是启用的");
    });

    test("A-9 · 「撤掉断点」发的是 `ClearBreakpoint { id }` 且 **`id === addr`**", async () => {
        seedDisasm();
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), breakpoints: [0x20] },
        }));
        const el = show("debug");
        // ⚠️ **按钮文案要说清当前状态** ⇒ ⇒ 已有断点时那个按钮就是「撤掉」。
        const btn = el.querySelector('[data-bp-toggle="0x00000020"]');
        assert.ok(btn, "0x20 那行没有断点按钮");
        assert.match(
            String(btn.textContent),
            /断点/,
            `已有断点时按钮文案应当说「断点」，实际 ${btn.textContent}`,
        );
        const signals = captureCommands(() => btn.click());
        assert.equal(signals.length, 1, `点一下应当只发一条命令信号，实际 ${signals.length}`);
        assert.equal(signals[0].action, "clear-breakpoint", "点「撤掉断点」发的 action 不对");

        const { services, dispatched } = fakeServices();
        await app.handleCommand(signals[0], services);
        assert.equal(dispatched.length, 1, `应当派发一条命令，实际 ${dispatched.length}`);
        const clr = dispatched[0]?.ClearBreakpoint;
        assert.ok(clr, `派发的不是 ClearBreakpoint：${JSON.stringify(dispatched[0])}`);
        assert.equal(
            Number(clr.id),
            0x20,
            `ClearBreakpoint 的 id 必须是地址本身（裁决 J），实际 ${clr.id}`,
        );
    });

    test("A-15 · 停下原因**只有一个落点**：脚本口 `lastHalt()` 与视图状态 `debug.halt` 读同一份（裁决 L）", () => {
        // ⚠️ **脚本口不许自己订阅一次** ⇒ ⇒ 那是**第二份状态** ⇒ ⇒ 会漂。
        const fake = { subscribe: () => () => {}, dispatchCommand: async () => ({ value: null }) };
        app.exposeScriptCommands(fake);
        assert.ok(dom.window.riscv_kernel, "脚本口没有挂上 window.riscv_kernel");
        assert.equal(dom.window.riscv_kernel.lastHalt(), null, "还没有停下过，不该有值");

        // 内核报了「命中断点」⇒ ⇒ 落进**视图状态** `debug.halt`。
        m03.setState("debug", (prev) => ({
            ...prev,
            debug: { ...(prev.debug ?? {}), halt: { reason: "Breakpoint", pc: 0x20 } },
        }));
        const halt = m03.getState("debug")?.debug?.halt;
        assert.ok(halt, "视图状态里没有 debug.halt");
        assert.equal(halt.reason, "Breakpoint", "停下原因不对");
        assert.equal(Number(halt.pc), 0x20, "停下时的 PC 不对");
        // ⚠️ **同一个落点** ⇒ ⇒ 脚本口读到的**就是**那一份。
        assert.deepEqual(
            dom.window.riscv_kernel.lastHalt(),
            { reason: "Breakpoint", pc: 0x20 },
            "脚本口 lastHalt() 与视图状态 debug.halt 不是同一份（第二个真相）",
        );
    });

    test("A-15b · 内核的 `Halted` 事件 ⇒ ⇒ 视图状态 `debug.halt`（app 层一处订阅）", () => {
        const handlers = new Map();
        const fake = {
            subscribe: (topic, handler) => {
                handlers.set(topic, handler);
                return () => handlers.delete(topic);
            },
            dispatchCommand: async () => ({ value: null }),
        };
        app.wireDebug(fake);
        assert.ok(handlers.has("halted"), "app 层没有订阅 halted 主题");
        handlers.get("halted")({ value: { reason: "Breakpoint", pc: 0x20 } });
        const halt = m03.getState("debug")?.debug?.halt;
        assert.ok(halt, "halted 事件没有落进 debug.halt");
        assert.equal(halt.reason, "Breakpoint");
        assert.equal(Number(halt.pc), 0x20);
    });

    test("A-14 · 源码视图**没有**断点按钮（裁决 H 的边界在界面上成立）", () => {
        m03.setState("source", (prev) => ({
            ...prev,
            source: { ...(prev.source ?? {}), tree: { "main.c": "int main(){return 0;}\n" } },
        }));
        const el = show("source");
        assert.equal(
            el.querySelectorAll("[data-bp-toggle]").length,
            0,
            "源码视图里出现了断点按钮（裁决 H 说这一批不做）",
        );
    });
});

describe("STAGE-35 · 产品树折叠 / 内存图比例条 / 终端收发计数", () => {
    test("A-10 · 产品树**可折叠**，且折叠状态住**视图状态**（不是 DOM 私藏）", () => {
        m03.setState("product", (prev) => ({
            ...prev,
            tree: {
                root: {
                    name: "MainBoard",
                    component_id: "rv-sim",
                    children: [
                        { name: "GPIO0", component_id: "GPIO0", children: [] },
                        { name: "UART0", component_id: "UART0", children: [] },
                    ],
                },
            },
            collapsed: [],
        }));
        const el = show("product");
        const carets = el.querySelectorAll(".caret");
        // ⚠️ **先钉住件数** ⇒ ⇒ 免得「一个 caret 都没有」时这条断言空转。
        assert.ok(carets.length >= 1, "产品树里没有可折叠的节点");
        const before = el.querySelectorAll(".kids").length;
        assert.ok(before >= 1, "产品树一开始就没有子节点（折叠无从谈起）");

        const key = carets[0].getAttribute("data-tree-key");
        assert.ok(key, "可折叠节点没有 data-tree-key");
        captureCommands(() => carets[0].click());

        // ⚠️ **状态住在视图状态里** ⇒ ⇒ 不是 DOM 私藏。
        const collapsed = m03.getState("product")?.collapsed;
        assert.ok(Array.isArray(collapsed), "视图状态里没有 collapsed 数组");
        assert.ok(collapsed.includes(key), "折叠状态没落进视图状态");
        // 折叠后子节点不再渲染。
        const after = body("product").querySelectorAll(".kids").length;
        assert.equal(after, before - 1, "折叠之后子节点还在（折叠没生效）");

        // ⚠️ **切走再切回来**折叠还在 ⇒ ⇒ 这才是「住视图状态」的证据。
        show("waveform");
        const back = show("product");
        assert.equal(
            back.querySelectorAll(".kids").length,
            after,
            "切走再切回来折叠状态丢了（状态其实藏在 DOM 里）",
        );
    });

    test("A-11 · 内存图比例条的**分段数 == 段表行数**（且每段宽度按**真地址区间**算）", () => {
        // ⚠️ **先造出真数据** ⇒ ⇒ 0 段 == 0 行那条断言**恒真** ⇒ ⇒ 假绿。
        let draft = memoryMap.defaultDraft();
        draft = memoryMap.addRegion(draft, "ram");
        draft = memoryMap.addRegion(draft, "rom");
        m03.setState("memory-map", (prev) => ({ ...prev, draft }));
        const el = show("memory-map");
        const segs = el.querySelectorAll(".seg");
        const rows = el.querySelectorAll("[data-mmap-row]");
        assert.ok(rows.length >= 2, `段表应当至少有 2 行，实际 ${rows.length}`);
        assert.equal(
            segs.length,
            rows.length,
            `比例条分段数 ${segs.length} 与段表行数 ${rows.length} 对不上`,
        );
        // ⚠️ **宽度按真地址区间算** ⇒ ⇒ 逐段核对比例。
        const regions = draft.regions ?? [];
        const lo = Math.min(...regions.map((r) => Number(r.base)));
        const hi = Math.max(...regions.map((r) => Number(r.base) + Number(r.size)));
        const span = hi - lo;
        assert.ok(span > 0, "这块板的地址跨度是 0（算不出比例）");
        for (let i = 0; i < segs.length; i += 1) {
            const style = String(segs[i].getAttribute("style") ?? "");
            const m = /flex:\s*([0-9.eE+-]+)/.exec(style);
            assert.ok(m, `第 ${i} 段没有宽度（style=${style}）`);
            const expected = Number(regions[i].size) / span;
            const actual = Number(m[1]);
            assert.ok(
                Math.abs(actual - expected) < 1e-9,
                `第 ${i} 段的比例 ${actual} 不是按真地址区间算的（期望 ${expected}）`,
            );
        }
    });

    test("A-12 · 终端**显示 TX / RX 计数**，且**分别标注方向**（裁决 G）", () => {
        m03.setState("terminal", (prev) => ({
            ...prev,
            term: { ...(prev.term ?? {}), bytes: "", draft: "", txBytes: 0, rxBytes: 0 },
        }));
        const el = show("terminal");
        const tx = el.querySelector("[data-term-tx]");
        const rx = el.querySelector("[data-term-rx]");
        assert.ok(tx && rx, "终端里没有 TX / RX 计数");
        const text = String(el.textContent);
        assert.ok(/guest\s*→\s*终端/.test(text), "TX 没有标注方向（guest → 终端）");
        assert.ok(/终端\s*→\s*guest/.test(text), "RX 没有标注方向（终端 → guest）");

        // ⚠️ **两个计数不是同一个源** ⇒ ⇒ 内核推来的字节只涨 TX。
        m02.get("terminal").onDelta({ termDelta: { bytes: [104, 105, 33] } });
        assert.equal(
            Number(dom.document.querySelector("[data-term-tx]").textContent),
            3,
            "内核推了 3 个字节，TX 应当是 3",
        );
        assert.equal(
            Number(dom.document.querySelector("[data-term-rx]").textContent),
            0,
            "RX 不该被内核的字节影响（两个不是同一个源）",
        );

        // 前端发出去的字节只涨 RX。
        const input = dom.document.querySelector("[data-terminal-input]");
        input.value = "hi";
        const send = dom.document.querySelector('[data-action="console-send"]');
        assert.ok(send, "终端里没有发送按钮");
        captureCommands(() => send.click());
        assert.equal(
            Number(dom.document.querySelector("[data-term-rx]").textContent),
            3,
            "前端发出 3 个字节（\"hi\" 两个 **+** 默认档的行结束符 CR 一个），RX 应当是 3",
        );
        assert.equal(
            Number(dom.document.querySelector("[data-term-tx]").textContent),
            3,
            "RX 不该影响 TX（两个不是同一个源）",
        );
    });

    test("A-13 · **ABI 名零出现**（界面里没有 `sp` / `ra` / `gp` 之类别名 · 裁决 E）", () => {
        const el = show("regmem");
        const text = String(el.textContent);
        // ⚠️ **完整的 ABI 别名表** ⇒ ⇒ 只查 `sp` / `ra` 两个是**半截**判据。
        const abi = [
            "zero", "ra", "sp", "gp", "tp",
            "t0", "t1", "t2",
            "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11",
            "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7",
        ];
        for (const name of abi) {
            assert.ok(
                !new RegExp(`\\b${name}\\b`).test(text),
                `寄存器视图里出现了 ABI 别名「${name}」`
                + "⇒ ⇒ 契约层没有那个字段（无数据源）⇒ ⇒ 那是编造",
            );
        }
    });

    test("A-16 · 寄存器变化高亮：快照住 `regmem.prevGpr`（app 层在 `read-regs` **之前**存）", async () => {
        const before = Array.from({ length: 32 }, (_, i) => i + 1);
        const after = before.slice();
        after[1] = 0; // x1 变小 ⇒ `.down`
        after[2] = 999; // x2 变大 ⇒ `.up`
        m03.setState("regmem", (prev) => ({
            ...prev,
            sim: { ...(prev.sim ?? {}), gpr: before.slice() },
            regmem: { ...(prev.regmem ?? {}), prevGpr: null },
        }));
        const { services } = fakeServices({
            registerMemory: {
                readRegisters: async () => {
                    m03.setState("regmem", (prev) => ({
                        ...prev,
                        sim: { ...(prev.sim ?? {}), gpr: after.slice() },
                    }));
                },
            },
        });
        await app.handleCommand({ action: "read-regs" }, services);
        // ⚠️ **快照是「读之前」的那一份** ⇒ ⇒ 不是读之后那份。
        assert.deepEqual(
            m03.getState("regmem")?.regmem?.prevGpr,
            before,
            "prevGpr 不是读之前的那份 GPR 快照",
        );
        const el = show("regmem");
        const codes = el.querySelectorAll("code");
        const cellOf = (i) => {
            const div = el.querySelector(".reg-grid").querySelectorAll("div")[i];
            return div.querySelector("code");
        };
        assert.ok(cellOf(0), "寄存器格子里没有 code");
        assert.equal(cellOf(0).getAttribute("class") ?? "", "", "x1 没变却带上了高亮类");
        assert.equal(cellOf(1).getAttribute("class"), "down", "x1 变小了却没有 .down");
        assert.equal(cellOf(2).getAttribute("class"), "up", "x2 变大了却没有 .up");
        assert.ok(codes.length >= 32, "寄存器格子数量不对");
    });
});
