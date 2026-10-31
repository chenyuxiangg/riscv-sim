// STAGE-44 · v0.68 批 —— **产品树 = 板的编辑面**的前端门禁（N 组）。
//
// 这个套件盯**一条用户能自己走通的路**：
//
//   在产品树的树上**加一个器件** ⇒ ⇒ 草稿里多一段 ⇒ ⇒ **板级内存图编辑器
//   里那张「外设窗口」表同步多一行** ⇒ ⇒ 点「提交」⇒ ⇒ 发的是**同一个**
//   `submit-memory-map` ⇒ ⇒ 真内核接受 ⇒ ⇒ 逐实例清单多一条。
//
// ⚠️ **三条不变式**：
//   ① **树 = 草稿的派生视图**（不再读 `Project.tree`）⇒ ⇒ 树与板**同源**；
//   ② **树与编辑器那张表共用同一份草稿** ⇒ ⇒ 同一个量**不可能不同步**；
//   ③ **类型下拉只列内核已实现的类型** ⇒ ⇒ 前端**不再自说自话**。
//
// ⚠️ **真交互**（真点、真提交、真内核接受）由 `tools/browser-verify.mjs` 的
// `boardTree` 段（B-118 ~ B-120）负责 ⇒ ⇒ 两边合起来才是完整的判据。

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
const memoryMap = await importFront("views/components/memory-map.js");
const boardTree = await importFront("views/components/board-tree.js");

await m02.mountViewComponents();
await m01.mountWorkbench();
await new Promise((r) => setTimeout(r, 0));

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

/** 一块板（草稿）：两段 RAM + 一个 GPIO0 外设窗口。 */
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

/** 把草稿放进「memory-map」视图状态（**唯一那一份**）。 */
function setDraft(draft) {
    m03.setState("memory-map", (prev) => ({ ...prev, draft }));
    return draft;
}

/** 树里的器件节点。 */
function deviceNodes(viewId = "product") {
    return Array.from(body(viewId).querySelectorAll("[data-device-node]"));
}

/** 板级内存图编辑器里那张「外设窗口」表的行。 */
function peripheralRows() {
    const view = body("memory-map");
    const rows = Array.from(view.querySelectorAll("[data-mmap-row-kind=\"peripheral\"]"));
    return rows;
}

/** 树上的器件 id（按顺序）。 */
function treeDeviceIds() {
    return deviceNodes().map((el) => el.getAttribute("data-device-node"));
}

/** 编辑器表里的器件 id（按顺序）。 */
function tableDeviceIds() {
    return peripheralRows().map((el) => {
        const input = el.querySelector("[data-mmap-device-field=\"id\"]");
        return input ? input.getAttribute("value") : null;
    });
}

// ---------------------------------------------------------------------------
// N 组
// ---------------------------------------------------------------------------

describe("STAGE-44 · 产品树 = 板的编辑面", () => {
    test("N-1 树显示的是**草稿里的器件实例**，不再是 `Project.tree` 那棵 mock 树", () => {
        setDraft(boardDraft());
        // ⚠️ **故意**塞一棵 mock 树进视图状态 ⇒ ⇒ 树**不许**显示它。
        m03.setState("product", (prev) => ({
            ...prev,
            tree: {
                root: {
                    name: "MainBoard",
                    component_id: "rv-sim",
                    children: [{ name: "RV32IMC", component_id: "rv32imc", children: [] }],
                },
            },
        }));
        show("product");
        const ids = treeDeviceIds();
        assert.deepEqual(ids, ["GPIO0"], "树显示的不是草稿里的器件实例");
        const text = body("product").textContent ?? "";
        assert.ok(!text.includes("rv-sim"), "树还在显示 `Project.tree` 那棵 mock 树");
        assert.ok(!text.includes("RV32IMC"), "树还在显示 `Project.tree` 那棵 mock 树");
    });

    test("N-2 树上「+ 器件」⇒ 草稿里多一段 ⇒ **编辑器那张表同步多一行**", () => {
        setDraft(boardDraft());
        show("product");
        const before = treeDeviceIds().length;
        const btn = body("product").querySelector("[data-action=\"add-device\"]");
        assert.ok(btn, "树上没有「+ 器件」按钮");
        btn.click();
        // ⚠️ **树**变了。
        assert.equal(treeDeviceIds().length, before + 1, "树上没多出一个器件节点");
        // ⚠️ **编辑器那张表**（同一份草稿）也变了 ⇒ ⇒ 同一个量**不可能不同步**。
        show("memory-map");
        assert.equal(peripheralRows().length, before + 1, "编辑器那张表没同步多一行");
    });

    test("N-2b 新加的器件**不与已有的重名**（id 与基址都不重）", () => {
        setDraft(boardDraft());
        show("product");
        const btn = body("product").querySelector("[data-action=\"add-device\"]");
        btn.click();
        const ids = treeDeviceIds();
        assert.equal(new Set(ids).size, ids.length, `器件 id 撞了：${ids.join("、")}`);
        const bases = deviceNodes().map((el) => {
            const input = el.querySelector("[data-mmap-region-field=\"base\"]");
            return input ? input.getAttribute("value") : null;
        });
        assert.equal(new Set(bases).size, bases.length, `基址撞了：${bases.join("、")}`);
    });

    test("N-3 树上「删除」⇒ **编辑器那张表同步少一行**", () => {
        setDraft(boardDraft());
        show("product");
        const btn = body("product").querySelector("[data-action=\"remove-device\"]");
        assert.ok(btn, "树上没有「删除」按钮");
        btn.click();
        assert.equal(treeDeviceIds().length, 0, "树上没删掉那个器件");
        show("memory-map");
        assert.equal(peripheralRows().length, 0, "编辑器那张表没同步少一行");
    });

    test("N-4 树与编辑器那张表**显示同一份草稿**（逐字段同形）", () => {
        setDraft(boardDraft());
        show("product");
        show("memory-map");
        assert.deepEqual(tableDeviceIds(), treeDeviceIds(), "树与编辑器那张表不是同一份草稿");
    });

    test("N-5 树上的提交按钮发的是**同一个** `submit-memory-map`", () => {
        setDraft(boardDraft());
        show("product");
        const seen = [];
        const handler = (ev) => seen.push(ev.detail);
        dom.window.addEventListener("riscv:command", handler);
        try {
            const btn = body("product").querySelector("[data-action=\"submit-board\"]");
            assert.ok(btn, "树上没有「提交板级内存图」按钮");
            btn.click();
        } finally {
            dom.window.removeEventListener("riscv:command", handler);
        }
        assert.equal(seen.length, 1, `树上提交发了 ${seen.length} 条命令（应恰好一条）`);
        assert.equal(seen[0].action, "submit-memory-map", "树上提交发的不是 `submit-memory-map`");
        assert.ok(
            Array.isArray(seen[0].params?.description?.regions),
            "提交载荷里没有描述（regions）",
        );
    });

    test("N-6 「待生效」取自**内核的逐实例状态**（不是前端自己算）", () => {
        setDraft(boardDraft());
        // ⚠️ 内核说：GPIO0 **不在**已绑定清单里 ⇒ ⇒ 待生效。
        m03.setState("product", (prev) => ({
            ...prev,
            deviceInstances: [{ id: "UART0", kind: "uart16550", base: 1024, window_bytes: 8 }],
        }));
        m03.setState("memory-map", (prev) => ({
            ...prev,
            deviceInstances: [{ id: "UART0", kind: "uart16550", base: 1024, window_bytes: 8 }],
        }));
        show("product");
        const node = deviceNodes()[0];
        assert.equal(node.getAttribute("data-device-node-pending"), "1", "没标成待生效");
        // ⚠️ **内核说它在** ⇒ ⇒ 不许标待生效。
        m03.setState("product", (prev) => ({
            ...prev,
            deviceInstances: [{ id: "GPIO0", kind: "gpio", base: 1073741824, window_bytes: 16 }],
        }));
        m03.setState("memory-map", (prev) => ({
            ...prev,
            deviceInstances: [{ id: "GPIO0", kind: "gpio", base: 1073741824, window_bytes: 16 }],
        }));
        show("product");
        assert.equal(
            deviceNodes()[0].getAttribute("data-device-node-pending"),
            "0",
            "内核说它已绑定，界面却标成待生效",
        );
    });

    test("N-7 类型下拉**只列内核已实现的类型**（内核没有的选不出来）", () => {
        setDraft(boardDraft());
        // ⚠️ 内核只实现了 gpio（**没有** uart16550 / clint / plic）。
        const kernelTypes = [{ kind: "gpio", window_bytes: 16, registers: [], params: [] }];
        m03.setState("product", (prev) => ({ ...prev, deviceTypes: kernelTypes }));
        m03.setState("memory-map", (prev) => ({ ...prev, deviceTypes: kernelTypes }));
        show("product");
        const select = body("product").querySelector("[data-mm-device-kind]");
        assert.ok(select, "树上没有器件类型下拉");
        const options = Array.from(select.querySelectorAll("option")).map((o) => o.getAttribute("value"));
        assert.deepEqual(options, ["gpio"], `下拉里出现了内核没有的类型：${options.join("、")}`);
    });

    test("N-8 内核的类型表还没到 ⇒ 下拉退回前端那份（兜底）", () => {
        setDraft(boardDraft());
        m03.setState("product", (prev) => ({ ...prev, deviceTypes: null }));
        m03.setState("memory-map", (prev) => ({ ...prev, deviceTypes: null }));
        show("product");
        const select = body("product").querySelector("[data-mm-device-kind]");
        const options = Array.from(select.querySelectorAll("option")).map((o) => o.getAttribute("value"));
        assert.deepEqual(
            options,
            [...memoryMap.DEVICE_KINDS],
            "内核的类型表还没到，下拉不该是空的",
        );
    });

    test("N-10 用户的板**待生效**时 ⇒ 提交**不许**被「没有改动」吞掉", () => {
        // ⚠️ **根因**（STAGE-44 施工期实测撞见）：差量的**基底 = 生效图**，
        // 而**正在被改的那份**是**用户的板**（它 ≠ 生效图）⇒ ⇒ 把草稿改回
        // 生效图那样 ⇒ ⇒ 差量算出来是「没有改动」⇒ ⇒ **命令根本不发**
        // ⇒ ⇒ **用户的板永远停在旧的那份**。
        const base = {
            schema_version: 1,
            id: "board-a",
            name: "测试板",
            priority: "100",
            auto_selectable: "false",
            region_budget_bytes: "8388608",
            regions: [{ kind: "ram", base: "0", size: "1048576" }],
            wiring: [],
        };
        // ⚠️ 用户的板**挂着**（≠ 生效图）⇒ ⇒ 草稿被改回生效图那样。
        const plan = memoryMap.planMemoryMapUpdate(
            base,
            { ...base, regions: [{ kind: "ram", base: "0", size: "1048576" }] },
            { pending: true },
        );
        assert.equal(plan.mode, "replace", "待生效时提交被算成「没有改动」⇒ ⇒ 命令不发 ⇒ ⇒ 用户的板停在旧的那份");
        // ⚠️ **没有**待生效 ⇒ ⇒ 差量那条路**照旧**（不回归）。
        const plan2 = memoryMap.planMemoryMapUpdate(
            base,
            { ...base, regions: [{ kind: "ram", base: "0", size: "2097152" }] },
            { pending: false },
        );
        assert.equal(plan2.mode, "patch", "没有待生效时不该一律整份（差量那条路被弄丢了）");
    });

    test("N-9 折叠仍**住视图状态**（沿用 v0.50 批那条机制）", () => {
        setDraft(boardDraft());
        m03.setState("product", (prev) => ({ ...prev, collapsed: [] }));
        show("product");
        const caret = body("product").querySelector(".caret[data-tree-key]");
        assert.ok(caret, "树根节点没有可折叠的 caret");
        const key = caret.getAttribute("data-tree-key");
        caret.click();
        const collapsed = m03.getState("product")?.collapsed;
        assert.ok(Array.isArray(collapsed) && collapsed.includes(key), "折叠状态没落进视图状态");
        assert.equal(deviceNodes().length, 0, "折叠之后器件节点还在");
    });
});
