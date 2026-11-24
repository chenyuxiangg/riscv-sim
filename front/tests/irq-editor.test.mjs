// 外设中断号汇总面（v0.143）的门禁：
//
//   1. **真的挂在产品视图上**（不是死导入）——右栏下面那张 `[data-irq-editor]`；
//   2. **行键 = 产品树节点键**（`r<段序号>`）——点一行真的选中右栏那个器件；
//   3. **冲突口径与内核一致**（`contract/platform.rs` V-15 / V-15b）：
//      `uart16550` / `gpio` / `timer_riscv` 三个外设一条规则，
//      越界（0 或 >= PLIC sources）与重复（同板同号）都要提示；
//   4. **只读**：面板里不许再有第二个 `irq_line` 输入框（同一个量一个编辑面）。
//
// ⚠️ 断言都走 data-* 钩子（文案会变，钩子不该变）。

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
const pw = await importFront("views/components/product-workspace.js");
const irq = await importFront("views/components/irq-editor.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

/** 一份最小板：RAM + UART（源 10）+ GPIO（源 11，缺省写法）+ 定时器（源 12）。 */
const BOARD = {
    schema_version: 1,
    id: "user-board",
    name: "我的板",
    priority: 100,
    auto_selectable: false,
    regions: [
        { kind: "ram", base: 0x80000000, size: 0x00100000 },
        {
            kind: "peripheral",
            base: 0x40000000,
            device: { device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 },
        },
        {
            kind: "peripheral",
            base: 0x40001000,
            device: { device: "gpio", id: "GPIO0", irq_line: 11 },
        },
        {
            kind: "peripheral",
            base: 0x02004000,
            device: { device: "timer_riscv", id: "TIMER0", irq_line: 12 },
        },
    ],
};

/** 装一块「已装载」的板（左树 + 右窗都有内容）。 */
function setupLoaded(extra = {}) {
    m03.reset();
    m01.assembleWorkspace("product");
    m03.setState("product", {
        loaded: true,
        locked: false,
        selected: null,
        board: { description: JSON.parse(JSON.stringify(BOARD)) },
        docs: {},
        docsBase: {},
        tree: pw.projectTreeFrom(BOARD),
        memoryMap: null,
        ...extra,
    });
    m01.route("#/product");
}

/** 装一个「还没选板」的空产品。 */
function setupEmpty() {
    m03.reset();
    m01.assembleWorkspace("product");
    m03.setState("product", {
        loaded: false,
        locked: true,
        selected: null,
        board: null,
        docs: {},
        docsBase: {},
        tree: null,
        memoryMap: null,
    });
    m01.route("#/product");
}

describe("外设中断号汇总面 · 挂在产品视图上", () => {
    test("已装载产品 ⇒ 右栏出现 `[data-irq-editor]`，行数 = 中断外设数", () => {
        setupLoaded();
        assert.ok($("[data-irq-editor]"), "产品视图里没有中断号汇总面（又变回死导入了）");
        const rows = $$("[data-irq-row]");
        assert.equal(rows.length, 3, `中断外设应有 3 行（UART / GPIO / 定时器），实际 ${rows.length}`);
        const keys = Array.from(rows).map((r) => r.getAttribute("data-irq-key"));
        assert.deepEqual(keys, ["r1", "r2", "r3"], `行键不是产品树节点键：${JSON.stringify(keys)}`);
    });

    test("空产品 ⇒ 不渲染汇总面（对着空板列中断号没意义）", () => {
        setupEmpty();
        assert.equal($("[data-irq-editor]"), null, "空产品不该有中断号汇总面");
    });

    test("定时器**也**进表（与内核 V-15 那条 match 同集合）", () => {
        setupLoaded();
        const ids = $$("[data-irq-row]").map((r) => r.textContent);
        assert.ok(ids.some((t) => t.includes("TIMER0")), `表里少了定时器：${JSON.stringify(ids)}`);
    });

    test("点一行 ⇒ 真的选中右栏那个器件", () => {
        setupLoaded();
        // ⚠️ 选择器写成「两个属性同一个元素」——dom-stub 不支持 `[a] [b]` 这种
        //   属性选择器后面的后代组合子。
        const btn = $("[data-action='select-irq-device'][data-irq-key='r3']");
        assert.ok(btn, "中断号那一行没有「跳到器件」的入口");
        btn.dispatchEvent(new Event("click"));
        assert.equal(m03.getState("product").selected, "r3", "点中断号行没选中那个器件");
        assert.equal($("[data-base-input]").value, "0x02004000", "右栏不是定时器的基址");
    });
});

describe("外设中断号汇总面 · 口径与内核一致", () => {
    /** 一份带 PLIC（sources 16）的草稿。 */
    const withPlic = (regions) => ({
        regions: [
            { kind: "peripheral", base: 0x0c000000, device: { device: "plic", id: "PLIC0", sources: 16, contexts: 1 } },
            ...regions,
        ],
    });

    test("重复：同板同号 ⇒ 两行都标 duplicate，且写明跟谁撞", () => {
        const { rows, conflictCount } = irq.irqRows({
            regions: [
                { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 10 } },
                { kind: "peripheral", device: { device: "gpio", id: "GPIO0", irq_line: 10 } },
            ],
        });
        assert.equal(conflictCount, 2, "两行都该算冲突");
        assert.equal(rows[0].duplicateOf, "GPIO0");
        assert.equal(rows[1].duplicateOf, "UART0");
        const html = irq.renderIrqEditor({
            draft: {
                regions: [
                    { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 10 } },
                    { kind: "peripheral", device: { device: "gpio", id: "GPIO0", irq_line: 10 } },
                ],
            },
        });
        assert.equal((html.match(/data-irq-conflict="duplicate"/g) ?? []).length, 2, html);
    });

    test("越界：0 与 >= PLIC sources 都算（取值 1..sources-1）", () => {
        const zero = irq.irqRows(withPlic([
            { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 0 } },
        ]));
        assert.equal(zero.rows[0].range, true, "源号 0 是保留号，应判越界");
        const top = irq.irqRows(withPlic([
            { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 16 } },
        ]));
        assert.equal(top.rows[0].range, true, "源号 16 超出 1..15，应判越界");
        const ok = irq.irqRows(withPlic([
            { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 15 } },
        ]));
        assert.equal(ok.rows[0].range, false, "源号 15 合法，不该判越界");
    });

    test("没有 PLIC ⇒ 不判越界（裁决 G：无 PLIC 的板不非法）", () => {
        const { rows } = irq.irqRows({
            regions: [
                { kind: "peripheral", device: { device: "uart16550", id: "UART0", irq_line: 40 } },
            ],
        });
        assert.equal(rows[0].range, false, "没有 PLIC 时不该判范围");
    });

    test("行里没有第二个 irq_line 输入框（同一个量一个编辑面）", () => {
        setupLoaded();
        const html = $("[data-irq-editor]").innerHTML;
        assert.ok(!html.includes("data-device-param"), "汇总面里出现了第二个参数输入框");
        assert.ok(!html.includes("data-mmap-device-field"), "汇总面里出现了第二套采集字段");
    });

    test("视图层真的**调用**它（不是只 import）——防再变回死导入", () => {
        const source = readFileSync(resolve(frontDir, "views/components/product-workspace.js"), "utf8");
        assert.match(source, /renderIrqEditor\(\{/, "产品视图里没有 renderIrqEditor 的调用点");
    });
});
