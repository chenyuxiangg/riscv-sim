// STAGE-52 · v0.76 批 —— 模板库（裁决 M）+ 引脚只读展示（裁决 E/G）的前端门禁
//
// 本套件钉住规格 §4.3 的 N 组：
//
//   N-1 前端镜像四个版本号与契约层逐字相同；`DEVICE_KINDS` 逐字含 `pinmux`
//   N-2 **模板库**：从模板导入可见 **3 块预设**（default / minimal / qemu-virt ·
//       数据来自内核）⇒ 选一块导入草稿 ⇒ 器件/段都在
//   N-3 **无板状态**：无生效板时界面提示「先导入模板或配置板」（不显示「自动选择」）
//   N-4 **只读展示**：生效板/草稿的引脚规格（功能名 · 方向 · irq 源）只读展示
//       ⇒ 前端**零处**自己判合法性（内核唯一校验器）
//   N-5 `defaultDraft()` 不再含 `auto_selectable` 键
//
// ⚠️ **数据来自内核**（裁决 M）：模板清单**不**在前端硬编码 ⇒ ⇒ 本套件
// 由**假内核**推 `platform.templates`（订阅即推那一条）⇒ ⇒ 卡上显示的
// **就是内核报的那份**。
//
// ⚠️ 前端**不写第二份校验器**（N-4）：`renderPinsCard` **不**判方向枚举、
// **不**判 irq 源越界 ⇒ ⇒ 只透传显示。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const mm = await importFront("views/components/memory-map.js");
const message = await importFront("lib/message.js");
const templates = await importFront("services/m76_templates.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);

// ---------------------------------------------------------------------------
// 假内核（会推模板清单 + 回完整模板描述）
// ---------------------------------------------------------------------------

function fakeKernel() {
    const subs = new Map();
    const sent = [];
    /** @type {Array<object>} 模板描述库（loadTemplate 的回复源） */
    const templateDescriptions = {};
    return {
        sent,
        templateDescriptions,
        subscribe(topic, callback) {
            if (!subs.has(topic)) subs.set(topic, new Set());
            if (typeof callback === "function") subs.get(topic).add(callback);
            return () => subs.get(topic).delete(callback);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            // `GetPlatformTemplate { id }` ⇒ 回完整描述（模拟内核）。
            if (payload && typeof payload === "object" && payload.GetPlatformTemplate) {
                const id = payload.GetPlatformTemplate.id;
                if (!templateDescriptions[id]) {
                    return { value: { error: { code: "E_PLT-22", message: "没有模板" } } };
                }
                return { value: { description: templateDescriptions[id] } };
            }
            return { value: {} };
        },
        emitTemplates(list) {
            const handlers = subs.get("platform.templates");
            if (!handlers || handlers.size === 0) return false;
            const event = {
                variant: "PlatformTemplates",
                topic: "platform.templates",
                value: { templates: list },
            };
            for (const fn of handlers) fn(event);
            return true;
        },
        emitMemoryMap(description, pending = false, loaded = false, revision = 0) {
            const handlers = subs.get("memory.map");
            if (!handlers || handlers.size === 0) return false;
            const event = {
                variant: "MemoryMap",
                topic: "memory.map",
                value: { description, pending, loaded, revision },
            };
            for (const fn of handlers) fn(event);
            return true;
        },
    };
}

// ---------------------------------------------------------------------------
// 固定数据（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** 内核侧会报的模板清单（priority 升序）——与内置 JSON 的数值对齐。 */
const TEMPLATE_LIST = [
    { id: "default", name: "默认板", priority: 10, device_count: 1 },
    { id: "qemu-virt", name: "QEMU virt 板", priority: 20, device_count: 4 },
    { id: "minimal", name: "极简板", priority: 30, device_count: 0 },
];

/** `GetPlatformTemplate` 回的那份完整描述（qemu-virt · 4 器件 + 引脚规格）。 */
const QEMU_VIRT_DESC = {
    schema_version: 5,
    id: "qemu-virt",
    name: "QEMU virt 板",
    priority: 20,
    region_budget_bytes: 16777216,
    regions: [
        { kind: "ram", base: 2147483648, size: 16777216 },
        { kind: "peripheral", base: 1073741312, device: { device: "clint", id: "CLINT", irq_line: 9 } },
        { kind: "peripheral", base: 1073741824, device: { device: "uart16550", id: "UART0", irq_line: 10 } },
        { kind: "peripheral", base: 1073742848, device: { device: "plic", id: "PLIC", irq_line: 11 } },
        { kind: "peripheral", base: 1073743872, device: { device: "gpio", id: "GPIO0", irq_line: 12 } },
    ],
    pins: [
        {
            device: "GPIO0",
            pin: "pin3",
            functions: [
                { name: "gpio_in", direction: "input", irq: { source: 11 } },
                { name: "uart_tx", direction: "output", irq: { source: 3 } },
            ],
        },
    ],
};

// ---------------------------------------------------------------------------

describe("STAGE-52 · 模板库（裁决 M · N 组）", () => {
    test("N-1 `DEVICE_KINDS` 逐字含 `pinmux`（与契约层 DeviceSpec::KINDS 同批）", () => {
        assert.ok(
            mm.DEVICE_KINDS.includes("pinmux"),
            "DEVICE_KINDS 必须含 pinmux（v0.76 新增器件）",
        );
        // ⚠️ 跨层逐字：从契约源码读 `KINDS` 数组 ⇒ ⇒ 前端这份必须**包含**
        // 契约那份（前端只展示内核认识的器件）。
        const modelSrc = readFileSync(
            resolve(repoRoot, "crates/contract/src/platform.rs"),
            "utf8",
        );
        const kindsMatch = /pub const KINDS:\s*\[&'static str;\s*\d+\]\s*=\s*\[(.*?)\];/s.exec(modelSrc);
        assert.ok(kindsMatch, "契约层必须声明 DeviceSpec::KINDS 常量");
        // ⚠️ JS 的 `trim` 不接受删除字符集参数 ⇒ ⇒ 用正则剥掉引号与空白。
        const rustKinds = kindsMatch[1]
            .split(",")
            .map((s) => s.replace(/["\s]/g, "").trim())
            .filter(Boolean);
        for (const kind of rustKinds) {
            assert.ok(
                mm.DEVICE_KINDS.includes(kind),
                `前端 DEVICE_KINDS 缺契约层的 ${kind}`,
            );
        }
    });

    test("N-2 模板库卡渲染内核报的 3 块预设（数据来自内核，不硬编码）", () => {
        m03.reset();
        m01.assembleWorkspace("product");
        m01.route("#/memory-map");
        const router = fakeKernel();
        templates.wireTemplates(router);
        assert.ok(router.emitTemplates(TEMPLATE_LIST), "假内核推清单必须有人接住");

        const card = $("[data-tpl-library='1']");
        assert.ok(card, "模板库卡必须在");
        const entries = card.querySelectorAll("[data-tpl-id]");
        assert.equal(entries.length, 3, "必须显示内核报的 3 块预设");
        const ids = Array.from(entries).map((e) => e.getAttribute("data-tpl-id"));
        assert.deepEqual(ids, ["default", "qemu-virt", "minimal"], "按内核清单原序显示");
        const text = card.textContent;
        assert.ok(text.includes("导入"), "每块预设必须有「导入」入口");
        assert.ok(
            card.querySelectorAll("[data-action='load-template']").length === 3,
            "导入按钮必须带 data-action=load-template",
        );
        // ⚠️ **前端不抄模板数据**（裁决 M）：模板卡上**不**该出现只有
        // 完整描述里才有的器件名（清单只含 id/名称/优先级/器件数）。
        assert.ok(
            !text.includes("CLINT") && !text.includes("UART0"),
            "模板卡只渲染清单字段，不许把完整描述抄进来",
        );
    });

    test("N-2 选一块导入 ⇒ 草稿带出整份描述（器件/引脚规格都在）", async () => {
        m03.reset();
        m01.assembleWorkspace("product");
        m01.route("#/memory-map");
        const router = fakeKernel();
        router.templateDescriptions["qemu-virt"] = QEMU_VIRT_DESC;
        const wiring = templates.wireTemplates(router);

        await app.handleCommand(
            { action: "load-template", params: { id: "qemu-virt" } },
            { commandRouter: router, templates: wiring },
        );

        const d = mm.currentDraft(m03.getState("memory-map"));
        assert.equal(d.id, "qemu-virt", "草稿 id 必须是导入的那块模板");
        assert.equal(
            d.regions.filter((r) => r.kind === "peripheral").length,
            4,
            "导入后草稿必须带 4 个外设器件（CLINT/PLIC/UART0/GPIO0）",
        );
        assert.ok(Array.isArray(d.pins), "导入后草稿必须带引脚规格");
        assert.equal(d.pins[0].device, "GPIO0", "引脚规格跟着模板一起进草稿");
        assert.equal(d.pins[0].functions.length, 2, "同名引脚的两个功能必须都在");
    });

    test("N-2 未知模板 id ⇒ 内核错误原样摆出（不吞）", async () => {
        m03.reset();
        m01.assembleWorkspace("product");
        m01.route("#/memory-map");
        const router = fakeKernel();
        const wiring = templates.wireTemplates(router);

        await app.handleCommand(
            { action: "load-template", params: { id: "no-such" } },
            { commandRouter: router, templates: wiring },
        );

        const state = m03.getState("memory-map");
        assert.equal(
            state.error?.code ?? state.memoryMapError?.code ?? "",
            "E_PLT-22",
            "未知模板必须摆出 E_PLT-22（内核的原样错误）",
        );
    });

    test("N-3 无生效板时界面提示「先导入模板或配置板」（不显示「自动选择」）", () => {
        m03.reset();
        m01.assembleWorkspace("product");
        m01.route("#/memory-map");
        const router = fakeKernel();
        templates.wireTemplates(router);

        const text = dom.document.body.textContent;
        assert.ok(
            text.includes("模板库"),
            "无板状态必须把模板库摆在界面上（B1 引导面）",
        );
        assert.ok(
            text.includes("无板装载会被拒") || text.includes("E_PLT-22"),
            "无板装载的引导文案必须在（E_PLT-22 不静默）",
        );
        assert.ok(
            !text.includes("自动选择"),
            "无板状态不许显示「自动选择」（自动选档已摘除）",
        );
    });
});

describe("STAGE-52 · 引脚只读展示（裁决 E/G · N 组）", () => {
    test("N-4 引脚规格卡：只读展示功能名/方向/irq 源 ⇒ 零编辑入口", () => {
        m03.reset();
        m01.assembleWorkspace("product");
        m01.route("#/memory-map");
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(QEMU_VIRT_DESC, false, true, 7);

        const card = $("[data-pins-display='1']");
        assert.ok(card, "引脚规格卡必须在");
        assert.ok(card.textContent.includes("只展示 · 不编辑"), "必须自标「只展示 · 不编辑」");
        assert.ok(card.textContent.includes("gpio_in"), "功能名必须显示");
        assert.ok(card.textContent.includes("input"), "方向必须显示");
        assert.ok(card.textContent.includes("uart_tx"), "第二个功能也必须显示");
        assert.ok(card.textContent.includes("11"), "irq 源必须显示");
        assert.ok(card.textContent.includes("3"), "第二个功能的 irq 源也必须显示");
        assert.equal(card.querySelectorAll("input").length, 0, "引脚卡不能有任何输入框");
        assert.equal(
            card.querySelectorAll("[data-action]").length,
            0,
            "引脚卡不能有任何动作按钮（只读透传）",
        );
    });

    test("N-4 前端**不判**合法性：非法方向 / 越界源照原样透传显示（内核是唯一校验器）", () => {
        const draft = mm.draftFromDescription({
            schema_version: 5,
            id: "weird-pins",
            name: "怪引脚板",
            priority: 1,
            region_budget_bytes: 4096,
            regions: [{ kind: "ram", base: 0, size: 4096 }],
            pins: [
                { device: "GPIO0", pin: "pin9", functions: [{ name: "whatever", direction: "sideways", irq: { source: 999 } }] },
            ],
        });
        assert.equal(draft.pins[0].functions[0].direction, "sideways", "非法方向照透传");
        assert.equal(draft.pins[0].functions[0].irq.source, 999, "越界源照透传");
        const back = mm.draftToDescription(draft);
        assert.equal(back.pins[0].functions[0].irq.source, 999, "往返后越界源仍在（前端不纠）");
    });

    test("N-5 `defaultDraft()` 不再含 `auto_selectable` 键（B3 决议）", () => {
        const d = mm.defaultDraft();
        assert.ok(!("auto_selectable" in d), "默认草稿不许再带 auto_selectable（废弃字段）");
        const description = mm.draftToDescription(d);
        assert.ok(!("auto_selectable" in description), "提交的描述也不许带 auto_selectable");
        // ⚠️ 老板档案（带旧键）仍能解析导入 ⇒ 前端忽略废弃键（serde 同形）。
        const legacy = mm.draftFromDescription({
            schema_version: 4,
            id: "legacy",
            name: "老板档案",
            priority: 1,
            auto_selectable: true,
            region_budget_bytes: 4096,
            regions: [{ kind: "ram", base: 0, size: 4096 }],
        });
        assert.ok(!("auto_selectable" in legacy), "旧键不透传进草稿");
    });
});
