// 板级内存图 · **纯逻辑**门禁（STAGE-14 ~ STAGE-19 的契约侧纪律）。
//
// ⚠️ **编辑器与面板的用例已随视图一起删除**（本轮重构：板级内存图不再是
// 独立工作面 —— 基址 / 偏移 / 含义都搬到产品视图的右栏，见
// `product-view.test.mjs`）。这里只剩**没有视图也能成立**的那部分：
//
//   - 草稿 → 线上描述（字段不多不少、外设没有 `size`）；
//   - 提交走哪条路由（整份 / 差量 / 什么都不发）；
//   - 差量操作的形状（`UpsertRegion` / `RemoveRegion` / `SetBudget`）；
//   - 错误码拆分、数字解析、规范 JSON。
//
// ⚠️ 这几条是**内核契约的镜像**（`crates/contract/src/platform.rs`）
// ⇒ 不因为界面搬家而变化 ⇒ 一条都不许放宽。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const mm = await importFront("views/components/memory-map.js");

/** 一份「用户配的板」——形状与契约层的 `PlatformDescription` 逐字同形。 */
const USER_BOARD = {
    schema_version: 1,
    id: "user-board",
    name: "用户板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 2147483648, size: 8388608 },
        {
            kind: "peripheral",
            base: 1073741824,
            device: { device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 },
        },
    ],
};

/** 草稿形状（数值是字符串）——与产品视图右栏写出来的那一份同形。 */
const USER_DRAFT = {
    schema_version: 1,
    id: "user-board",
    name: "用户板",
    priority: "100",
    auto_selectable: false,
    region_budget_bytes: "8388608",
    regions: [
        { kind: "ram", base: "0x80000000", size: "8388608" },
        {
            kind: "peripheral",
            base: "0x40000000",
            device: { device: "uart16550", id: "UART0", irq_line: "10", baud_rate: "115200" },
        },
    ],
};

describe("板级内存图 · 描述形状（契约逐字同形）", () => {
    test("草稿 ⇒ 线上描述：字段不多不少（草稿的内部字段漏不进契约）", () => {
        const dirty = { ...USER_DRAFT, __uiExtra: "不许漏进去", regions: [...USER_DRAFT.regions] };
        const description = mm.draftToDescription(dirty);
        assert.equal(description.__uiExtra, undefined, "草稿内部字段漏进了契约");
        assert.equal(description.schema_version, 1);
        assert.equal(description.id, "user-board");
        assert.equal(description.region_budget_bytes, 8388608);
        assert.equal(typeof description.region_budget_bytes, "number");
    });

    test("外设窗口的线上形状**没有** `size`", () => {
        const description = mm.draftToDescription(USER_DRAFT);
        const peripheral = description.regions.find((r) => r.kind === "peripheral");
        assert.ok(peripheral, "外设段丢了");
        assert.equal("size" in peripheral, false, "外设窗口被补了一个编出来的 size");
    });

    test("器件的线上形状是 serde 内部标签 `device`", () => {
        const description = mm.draftToDescription(USER_DRAFT);
        const peripheral = description.regions.find((r) => r.kind === "peripheral");
        assert.equal(peripheral.device.device, "uart16550");
        assert.equal(peripheral.device.irq_line, 10);
        assert.equal(typeof peripheral.device.irq_line, "number");
    });

    test("非法数字**不写进草稿**（前端不猜用户的意图）", () => {
        assert.equal(mm.parseNumber("0x80"), 128);
        assert.equal(mm.parseNumber("128"), 128);
        assert.equal(mm.parseNumber(""), null);
        assert.equal(mm.parseNumber("abc"), null);
    });

    test("错误码按**族 + 码**解析（不靠文案）", () => {
        const split = mm.splitErrorCode({ code: "E_PLT-07", message: "E_PLT-07: 段超预算" });
        assert.equal(split.code, "E_PLT-07");
        assert.equal(split.message, "段超预算");
        assert.deepEqual(mm.parseErrorCode("E_PLT-07"), {
            code: "E_PLT-07", family: "PLT", scenario: "07",
        });
    });

    test("规范 JSON **按键排序** ⇒ 键序不同的同内容描述判为**相同**", () => {
        const a = { id: "x", name: "y" };
        const b = { name: "y", id: "x" };
        assert.equal(mm.canonicalJson(a), mm.canonicalJson(b));
        assert.equal(mm.sameDescription(a, b), true);
    });
});

describe("板级内存图 · 提交选路（整份 / 差量 / 不发）", () => {
    test("没有基底 ⇒ 整份替换（没有差量可算）", () => {
        assert.equal(mm.planMemoryMapUpdate(null, USER_BOARD).mode, "replace");
    });

    test("元数据（id）变了 ⇒ 整份替换", () => {
        const changed = { ...USER_BOARD, id: "another-board" };
        assert.equal(mm.planMemoryMapUpdate(USER_BOARD, changed).mode, "replace");
    });

    test("`wiring` 变了 ⇒ 整份替换（差量契约里没有 wiring 操作）", () => {
        const base = { ...USER_BOARD, wiring: [] };
        const changed = { ...USER_BOARD, wiring: [{ from: { device: "UART0", event: "TxReady" }, to: { device: "GPIO0", source: "Irq" } }] };
        assert.equal(mm.planMemoryMapUpdate(base, changed).mode, "replace");
    });

    test("只改一段的 size ⇒ 差量，且载荷里**只有**那一段的 op", () => {
        const changed = {
            ...USER_BOARD,
            regions: [
                { ...USER_BOARD.regions[0], size: 16777216 },
                USER_BOARD.regions[1],
            ],
        };
        const plan = mm.planMemoryMapUpdate(USER_BOARD, changed);
        assert.equal(plan.mode, "patch");
        assert.equal(plan.ops.length, 1);
        assert.ok(plan.ops[0].UpsertRegion, "差量里没有 UpsertRegion");
        assert.equal(plan.ops[0].UpsertRegion.spec.size, 16777216);
    });

    test("删一段 ⇒ `RemoveRegion`，base 正确", () => {
        const changed = { ...USER_BOARD, regions: [USER_BOARD.regions[0]] };
        const plan = mm.planMemoryMapUpdate(USER_BOARD, changed);
        assert.equal(plan.mode, "patch");
        assert.equal(plan.ops.length, 1);
        assert.equal(plan.ops[0].RemoveRegion.base, 1073741824);
    });

    test("改预算 ⇒ `SetBudget`（两个预算字段都带上）", () => {
        const changed = { ...USER_BOARD, region_budget_bytes: 16777216 };
        const plan = mm.planMemoryMapUpdate(USER_BOARD, changed);
        assert.equal(plan.mode, "patch");
        assert.deepEqual(plan.ops[0].SetBudget, {
            region_budget_bytes: 16777216,
            max_region_bytes: null,
        });
    });

    test("纯重排（内容没变）⇒ **不发命令**（不白涨修订号）", () => {
        const reordered = {
            ...USER_BOARD,
            regions: [USER_BOARD.regions[1], USER_BOARD.regions[0]],
        };
        assert.equal(mm.planMemoryMapUpdate(USER_BOARD, reordered).mode, "none");
    });

    test("什么都没改 ⇒ `none`", () => {
        assert.equal(mm.planMemoryMapUpdate(USER_BOARD, mm.draftToDescription(USER_DRAFT)).mode, "none");
    });
});
