// STAGE-46 · v0.70 批 —— **基址规划面板**（台账 **D-175** 候选 **B** 的第一半）的前端门禁（N 组）。
//
// 这个套件盯**一条用户能自己走通的路**：
//
//   用户在编辑器里配一块板 ⇒ ⇒ **基址规划面板**列出**当前草稿里还没被占用**的
//   地址区间 ⇒ ⇒ 挑一条 ⇒ ⇒ 它被填进「目标行」那一行的基址 ⇒ ⇒ 提交 ⇒ ⇒
//   **真内核接受**（那个空隙**不撞**）。
//
// ⚠️ **四条不变式**：
//   ① **用户自己规划，平台提供入口** ⇒ ⇒ 基址那格**仍可手填** ⇒ ⇒ 平台**不替
//      用户决定**落在哪 ⇒ ⇒ 面板**只**给「哪里空着」这个**信息** ＋ 一个**入口**；
//   ② **段起点是推导量** ⇒ ⇒ 空隙 = **规划范围** − **占用并集** ⇒ ⇒ 前端**不**
//      另存一份「哪里空着」的真相；
//   ③ **外设窗口大小查内核类型表** ⇒ ⇒ `window_bytes` **只有一个真相** ⇒ ⇒
//      内核**还没报**时**如实标注「窗口大小未知」** ⇒ ⇒ **不假装**知道；
//   ④ **冲突判定仍然只由内核判**（`E_PLT_06`）⇒ ⇒ 面板**只显示**占用区间
//      ⇒ ⇒ 前端**不写第二份校验器**。
//
// ⚠️ **真交互**（真点「用这个」、真提交、真内核接受）由
//   `tools/browser-verify.mjs` 的 `staleSurfaces` 之后的 **`basePlanner` 段**
//   （**B-136 ~ B-140**）负责 ⇒ ⇒ 两边合起来才是完整的判据。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const mm = await importFront("views/components/memory-map.js");

await m02.mountViewComponents();
await m01.mountWorkbench();
await new Promise((r) => setTimeout(r, 0));

/** 内核的器件类型表（**窗口大小的那份真相**）。 */
const TYPES = [
    { kind: "gpio", window_bytes: 64, registers: [], params: [] },
    { kind: "uart16550", window_bytes: 4096, registers: [], params: [] },
];

/** 一块板：两段 RAM + 一个 GPIO0 外设窗口（**彼此不重叠**）。 */
function boardDraft() {
    return {
        schema_version: 1,
        id: "board-a",
        name: "测试板",
        priority: "100",
        auto_selectable: "false",
        region_budget_bytes: "8388608",
        regions: [
            { kind: "ram", base: "0", size: "1048576" },
            { kind: "peripheral", base: "1073741824", device: { device: "gpio", id: "GPIO0" } },
            { kind: "ram", base: "2147483648", size: "8388608" },
        ],
        wiring: [],
    };
}

/** 把草稿（+ 可选的类型表）放进「memory-map」视图状态（**唯一那一份**）。 */
function setDraft(draft, deviceTypes = TYPES) {
    // ⚠️ **视图状态是模块级单例** ⇒ ⇒ **连 `plan` 一起重置** ⇒ ⇒ 否则上一条
    //   用例留下的规划范围会**漏进**这一条 ⇒ ⇒ 那条断言测的就不是它自己。
    m03.setState("memory-map", (prev) => ({ ...prev, draft, deviceTypes, plan: null }));
    return draft;
}

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

/** 面板里的空隙行。 */
function gapRows(view = body("memory-map")) {
    return Array.from(view.querySelectorAll("[data-mmap-plan-gap]"));
}

/** 两段区间**重叠**了吗（半开区间）。 */
function overlaps(a, b) {
    return a.from < b.to && b.from < a.to;
}

// ---------------------------------------------------------------------------

describe("STAGE-46 · 基址规划 · 纯函数", () => {
    test("N-1 · 空隙 = 规划范围 − 占用并集 ⇒ 每条都**不与占用重叠**、且**互不重叠**", () => {
        const draft = boardDraft();
        const occupied = mm.occupiedIntervals(draft, TYPES);
        const range = { from: 0, to: 0x4000_0000 };
        const gaps = mm.planBaseGaps(draft, TYPES, range);
        assert.ok(gaps.length > 0, "规划范围里一条空隙都没有 ⇒ ⇒ 那条断言会**恒真**（假绿）");
        for (const gap of gaps) {
            assert.ok(gap.size > 0, `有一条空隙的字节数是 ${gap.size}（不是可用区间）`);
            assert.ok(gap.from >= range.from && gap.to <= range.to,
                `空隙 ${gap.from}..${gap.to} 跑出了规划范围 ${range.from}..${range.to}`);
            for (const it of occupied) {
                assert.ok(
                    !overlaps(gap, { from: it.base, to: it.end }),
                    `空隙 ${gap.from}..${gap.to} 与占用 ${it.base}..${it.end} 重叠了`,
                );
            }
        }
        for (let i = 0; i < gaps.length; i += 1) {
            for (let j = i + 1; j < gaps.length; j += 1) {
                assert.ok(
                    !overlaps(gaps[i], gaps[j]),
                    `第 ${i} 与第 ${j} 条空隙互相重叠了`,
                );
            }
        }
    });

    test("N-2 · **必要对照**：草稿里已有**重叠段** ⇒ ⇒ 空隙按**并集**算 ⇒ ⇒ 重叠部分**不被**算成空隙", () => {
        const draft = boardDraft();
        // ⚠️ **故意**让第 2 段压到第 0 段上面（[0, 1 MiB) 那一块）。
        draft.regions[2].base = "524288";
        // ⚠️ **对照区** = 「干净时**空着**、重叠后**被压住**」的那一块 ⇒ ⇒
        //   段2 压到 `524288` ⇒ ⇒ 它一路盖到 `524288 + 8 MiB = 8912896` ⇒ ⇒
        //   [1 MiB, 8912896) 这一块**从空隙变成了占用** ⇒ ⇒ 这是一对**真能
        //   分辨**的样本（不是「两种情况都不空」那种恒假对照）。
        const probe = { from: 1048576, to: 8912896 };
        const gaps = mm.planBaseGaps(draft, TYPES, { from: 0, to: 0x4000_0000 });
        for (const gap of gaps) {
            assert.ok(
                !overlaps(gap, probe),
                `空隙 ${gap.from}..${gap.to} 落进了**已被压住**的那块 `
                + `⇒ ⇒ 说明占用没按**并集**算`,
            );
        }
        // ⚠️ **正向对照**：没有重叠时，那块**是**空隙（⇒ ⇒ 上面那条不是恒真）。
        const clean = mm.planBaseGaps(boardDraft(), TYPES, { from: 0, to: 0x4000_0000 });
        assert.ok(
            clean.some((g) => overlaps(g, probe)),
            "没有重叠时那块**不该**是空隙 ⇒ ⇒ 上面那条断言就**恒假**了",
        );
    });

    test("N-3 · 外设占用用**内核给的 `window_bytes`**（不是前端抄的那份）", () => {
        const draft = boardDraft();
        const withKernel = mm.occupiedIntervals(draft, TYPES);
        const peripheral = withKernel.find((it) => it.kind === "peripheral");
        assert.ok(peripheral, "草稿里那个外设段不见了");
        assert.equal(peripheral.size, 64, "外设占用长度**不是**内核给的 `window_bytes`");
        // ⚠️ **必要对照**：**改了内核那个数** ⇒ ⇒ 占用区间**必须跟着变**。
        const bigger = mm.occupiedIntervals(draft, [
            { kind: "gpio", window_bytes: 4096, registers: [], params: [] },
        ]);
        assert.equal(
            bigger.find((it) => it.kind === "peripheral").size,
            4096,
            "改了内核的 `window_bytes` 而占用区间**没跟着变** ⇒ ⇒ 前端用的是**抄的那份**",
        );
    });

    test("N-4 · 内核类型表**没到** ⇒ ⇒ 退回占位长度 **且** `windowKnown` 为假（**不假装**知道）", () => {
        const draft = boardDraft();
        const occupied = mm.occupiedIntervals(draft, null);
        const peripheral = occupied.find((it) => it.kind === "peripheral");
        assert.ok(peripheral, "草稿里那个外设段不见了");
        assert.equal(peripheral.windowKnown, false, "内核还没报窗口大小却说是**知道**的 ⇒ ⇒ 在骗人");
        assert.ok(peripheral.size > 0, "退化后也得有个长度（否则那段在图上不见了）");
        // ⚠️ **正向对照**：内核**报了** ⇒ ⇒ `windowKnown` 为真。
        assert.equal(
            mm.occupiedIntervals(draft, TYPES).find((it) => it.kind === "peripheral").windowKnown,
            true,
            "内核报了窗口大小却标成**不知道** ⇒ ⇒ 判据反了",
        );
    });

    test("N-5 · 面板**列出了**空隙 ⇒ ⇒ 每条有**起止地址 + 可用字节**", () => {
        setDraft(boardDraft());
        const view = show("memory-map");
        const plan = view.querySelector("[data-mmap-plan]");
        assert.ok(plan, "编辑器里没有「基址规划」面板");
        const rows = gapRows(view);
        assert.ok(rows.length > 0, "面板一条空隙都没列 ⇒ ⇒ 那条断言会**恒真**（假绿）");
        for (const row of rows) {
            const cells = row.querySelectorAll("td");
            assert.ok(cells.length >= 3, `空隙行只有 ${cells.length} 列（起 / 止 / 字节）`);
            for (const cell of cells) {
                assert.ok(
                    String(cell.textContent || "").trim().length > 0,
                    "空隙行里有一格是空的 ⇒ ⇒ 用户看不见那个区间",
                );
            }
            const use = row.querySelector('[data-action="use-base-gap"]');
            assert.ok(use, "空隙行没有「用这个」入口 ⇒ ⇒ 面板只是个说明");
        }
    });

    test("N-8 · **规划范围**改了 ⇒ ⇒ 空隙**跟着变**", () => {
        const draft = boardDraft();
        const wide = mm.planBaseGaps(draft, TYPES, { from: 0, to: 0x4000_0000 });
        const narrow = mm.planBaseGaps(draft, TYPES, { from: 0, to: 0x0010_0000 });
        assert.notDeepEqual(
            wide.map((g) => [g.from, g.to]),
            narrow.map((g) => [g.from, g.to]),
            "改了规划范围而空隙**没跟着变** ⇒ ⇒ 面板显示的不是这个范围里的空隙",
        );
        for (const gap of narrow) {
            assert.ok(gap.to <= 0x0010_0000, `窄范围里混进了 ${gap.from}..${gap.to}`);
        }
    });

    test("N-9 · 范围里**没有空隙** ⇒ ⇒ 面板**如实说**（不是显示一个空列表）", () => {
        const draft = boardDraft();
        // ⚠️ **故意**把范围设成正好被占满的那一小块。
        const occupied = mm.occupiedIntervals(draft, TYPES);
        const tight = { from: occupied[0].base, to: occupied[0].end };
        assert.equal(
            mm.planBaseGaps(draft, TYPES, tight).length,
            0,
            "这个范围**不该**有空隙（前提不成立 ⇒ ⇒ 后面那条断言就**恒假**了）",
        );
        setDraft(draft);
        m03.setState("memory-map", (prev) => ({
            ...prev,
            plan: { range: tight, target: 0 },
        }));
        const view = show("memory-map");
        const note = view.querySelector("[data-mmap-plan-note]");
        assert.ok(note, "面板没有那行说明");
        assert.ok(
            view.querySelector("[data-mmap-plan-empty]"),
            `范围里没有空隙却没说：${JSON.stringify(note.textContent)}`,
        );
    });

    test("N-10 · 比例条的外设段宽度**按内核给的窗口大小**算（裁决 I）", () => {
        const draft = boardDraft();
        const layout = mm.memoryMapLayout(draft, TYPES);
        const peripheral = layout.segments.find((seg) => seg.kind === "peripheral");
        assert.ok(peripheral, "比例条上那个外设段不见了");
        assert.equal(peripheral.size, 64, "比例条上的外设段**不是**按内核的窗口大小算的");
        // ⚠️ **必要对照**：内核**还没报** ⇒ ⇒ 退回占位长度（⇒ ⇒ 那个数**变了**）。
        const fallback = mm.memoryMapLayout(draft, null);
        assert.equal(
            fallback.segments.find((seg) => seg.kind === "peripheral").size,
            mm.PERIPHERAL_WINDOW_BYTES,
            "内核还没报时**不该**用占位长度 ⇒ ⇒ 判据反了",
        );
    });
});

describe("STAGE-46 · 基址规划 · 视图层交互", () => {
    test("N-6 · 点「用这个」⇒ ⇒ **目标行**的基址**真的**变了", () => {
        setDraft(boardDraft());
        const view = show("memory-map");
        const rows = gapRows(view);
        assert.ok(rows.length > 0, "面板一条空隙都没列 ⇒ ⇒ 后面无从谈起");
        const first = rows[0];
        const gapFrom = Number(first.getAttribute("data-mmap-plan-gap"));
        const use = first.querySelector('[data-action="use-base-gap"]');
        assert.ok(use, "空隙行没有「用这个」按钮");
        // ⚠️ **在 `pointerdown` 阶段触发**（真实浏览器里「刚敲完范围就点它」那个
        //   时序陷阱 ⇒ ⇒ `blur` → `change` → 重渲染 ⇒ ⇒ 按钮**被换掉** ⇒ ⇒
        //   `click` 根本到不了）。
        use.dispatchEvent(new Event("pointerdown", { bubbles: true }));
        const draft = m03.getState("memory-map")?.draft;
        assert.ok(draft, "视图状态里没有草稿");
        assert.equal(
            String(draft.regions[0].base),
            String(gapFrom),
            "点「用这个」之后目标行的基址**没变** ⇒ ⇒ 入口没生效",
        );
    });

    test("N-7 · **目标行**换一行 ⇒ ⇒ 填给的是**那一行**（不是永远第 0 行）", () => {
        setDraft(boardDraft());
        m03.setState("memory-map", (prev) => ({
            ...prev,
            plan: { range: null, target: 2 },
        }));
        const view = show("memory-map");
        const rows = gapRows(view);
        assert.ok(rows.length > 0, "面板一条空隙都没列");
        const gapFrom = Number(rows[0].getAttribute("data-mmap-plan-gap"));
        const use = rows[0].querySelector('[data-action="use-base-gap"]');
        assert.ok(use, "空隙行没有「用这个」按钮");
        use.dispatchEvent(new Event("pointerdown", { bubbles: true }));
        const draft = m03.getState("memory-map")?.draft;
        assert.ok(draft, "视图状态里没有草稿");
        assert.equal(
            String(draft.regions[2].base),
            String(gapFrom),
            "目标行是第 2 行，却把基址填到了别处 ⇒ ⇒ 目标行**没生效**",
        );
        assert.notEqual(
            String(draft.regions[0].base),
            String(gapFrom),
            "第 0 行**也被**改了 ⇒ ⇒ 目标行被**忽略**了（永远填第 0 行）",
        );
    });
});
