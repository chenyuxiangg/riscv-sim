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
        bridge: [],
    };
}

/** 把草稿放进「memory-map」视图状态（**唯一那一份**）。 */
function setDraft(draft) {
    m03.setState("memory-map", (prev) => ({ ...prev, draft }));
    return draft;
}

/** 把一份草稿放进「product」视图状态（双栏工作面的树来源）。 */
function setProductBoard(draft) {
    m03.setState("product", (prev) => ({
        ...prev,
        loaded: true,
        board: { description: JSON.parse(JSON.stringify(draft)) },
    }));
    return draft;
}

/** 双栏工作面左树里的器件项 key（按顺序）。 */
function productDeviceKeys() {
    return Array.from(body("product").querySelectorAll("[data-device-key]"))
        .map((el) => el.getAttribute("data-device-key"));
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

describe("STAGE-44 · 产品树 = 板的派生视图（双栏工作面）", () => {
    // ⚠️ **v0.77 产品视图重构**：产品视图从「树 + M-14 表 + 编辑器」改成
    // 「左树 + 右器件内存窗口」⇒ ⇒ 树上**不再有**「+ 器件 / 删除 / 类型下拉 /
    // 折叠 / 提交」这些编辑面（编辑改成「创建 / 从模版加载」+ lock/unlock +
    // 「重新载入」，覆盖在 `tests/product-view.test.mjs`）⇒ ⇒ 原来钉那些
    // 按钮的 N-2 / N-2b / N-3 / N-4 / N-5 / N-6 / N-7 / N-8 / N-9 **随界面
    // 一起退役**（不是静默删，是这条重构明确换掉了那块交互）。
    //
    // 留下的是那条**不变式**：**树 = 草稿的派生视图**（不读 `Project.tree`）。

    test("N-1 树显示的是**草稿里的器件实例**，不再是 `Project.tree` 那棵 mock 树", () => {
        setProductBoard(boardDraft());
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
        const keys = productDeviceKeys();
        // 核 + 三段（RAM / GPIO0 外设 / RAM）⇒ ⇒ 4 个节点，全部来自草稿。
        assert.deepEqual(keys, ["core", "r0", "r1", "r2"], "树显示的不是草稿里的器件实例");
        const text = body("product").textContent ?? "";
        assert.ok(text.includes("GPIO"), "草稿里的外设器件没上树");
        assert.ok(!text.includes("rv-sim"), "树还在显示 `Project.tree` 那棵 mock 树");
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
            bridge: [],
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

});
