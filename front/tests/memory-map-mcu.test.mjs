// v0.75 · STAGE-51 —— **MCU 概念**前端侧门禁（N-1 / N-3 / N-4 / N-5）。
//
// ⚠️ 本套件只盯**前端**的「展示与配置」语义（规格 §4.4 N 组）：
//
//   N-1  `mcu` 段**往返**：`normalizeMcu` 深复制、去多余键、向量表数值化
//   N-3  MCU 区**只展示**：草稿 MCU + 生效板 MCU 两行都出来，**没有任何
//        MCU 的编辑入口**（没有 `data-action` / 输入框）
//   N-4  加载固件后编辑面**不自动实例化**生效板描述（用户拍板：板级视图
//        用户自己配，内置板只作模板参考）⇒ 出现「当前生效板（自动选择 · 只读）」
//        横幅 + 「从内置模版开始」按钮
//   N-5  基址统一 **16 进制**显示（草稿里仍是十进制字符串，采集两种写法都认）
//
// ⚠️ N-2（前端**零处**判 isa）是「不存在性」断言 ⇒ 由 `tools/stage51_check.py`
//    grep 守住（前端源码**不出现** `E_PLT-20` / `mcu.isa` 判据），这里不假造
//    「测不了的东西」。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
const dom = installDom(indexHtml);

// ⚠️ 关掉 `app.js` 的自启动（与 `memory-map.test.mjs` 同一条理由）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const mm = await importFront("views/components/memory-map.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

// ---------------------------------------------------------------------------
// 现场
// ---------------------------------------------------------------------------

/** 装一套干净的「板级内存图编辑器」现场（同 memory-map.test.mjs 的手法）。 */
function setup(hash = "#/memory-map") {
    m03.reset();
    m01.assembleWorkspace("product");
    m01.route(hash);
}

/** 编辑器当前的草稿（走 `currentDraft` —— 渲染与采集同一个基底）。 */
function draft() {
    return mm.currentDraft(m03.getState("memory-map"));
}

/** 假内核（同 memory-map.test.mjs 的 fakeKernel 形状）。 */
function fakeKernel() {
    const subs = new Map();
    const topics = new Set();
    return {
        topics,
        async subscribeMemoryMapStream() {
            topics.add("memory.map");
            const handlers = subs.get("memory.map") ?? new Set();
            handlers.add(() => {});
            subs.set("memory.map", handlers);
        },
        async subscribe(topic, callback) {
            topics.add(topic);
            if (!subs.has(topic)) subs.set(topic, new Set());
            if (typeof callback === "function") subs.get(topic).add(callback);
            return () => subs.get(topic).delete;
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
        dispatchCommand() {
            return [];
        },
    };
}

/**
 * 一份「**带 mcu 的生效板**」——与契约层 `PlatformDescription` 逐字同形
 * （v0.75 新段：`mcu` 在 `pins` 之后，`vector_table` 表项数 16）。
 */
const MCU_BOARD = {
    schema_version: 4,
    id: "qemu-virt",
    name: "QEMU virt 板",
    priority: 100,
    auto_selectable: true,
    region_budget_bytes: 16777216,
    regions: [
        { kind: "ram", base: 2147483648, size: 16777216 },
        { kind: "peripheral", base: 1073741824, device: { device: "uart16550", id: "UART0", irq_line: 10 } },
        { kind: "peripheral", base: 1073742848, device: { device: "plic", id: "PLIC", irq_line: 11 } },
        { kind: "peripheral", base: 1073743872, device: { device: "gpio", id: "GPIO0", irq_line: 12 } },
    ],
    mcu: {
        isa: "rv32imac",
        vendor: "qemu",
        model: "virt",
        vector_table: { base: 2147483648, entries: 16 },
    },
};

/** 让面板按当前视图状态重渲染一次（重入当前视图也会派发 render 事件）。 */
function rerender() {
    m01.route("#/memory-map");
}

// ---------------------------------------------------------------------------
// N-1 · Mcu 往返
// ---------------------------------------------------------------------------

describe("板级内存图 · MCU 段（规格 §4.4 N 组）", () => {
    test("N-1 normalizeMcu 深复制 + 去多余键 + 向量表数值化（往返逐字不变）", () => {
        const once = mm.normalizeMcu({
            isa: "rv32imac",
            vendor: "qemu",
            model: "virt",
            vector_table: { base: "2147483648", entries: "16" },
        });
        assert.equal(once.isa, "rv32imac", "isa 必须原样保留（字符串）");
        assert.equal(once.vendor, "qemu", "vendor 必须是字符串才保留");
        assert.equal(once.model, "virt", "model 必须是字符串才保留");
        assert.equal(once.vector_table.base, 2147483648, "向量表基址必须数值化（>>>0）");
        assert.equal(once.vector_table.entries, 16, "表项数必须数值化");
        assert.ok(!("pins" in once), "多余键必须被丢掉（只留 mcu 该有的）");

        // 往返：归一化两次 ⇒ 语义不变（幂等）。
        assert.deepEqual(mm.normalizeMcu(once), once, "normalizeMcu 必须是幂等的（往返安全）");
    });

    test("N-1 缺省：没有 mcu ⇒ None（老描述不带键 ⇒ 前端不凭空造）；isa 空串 ⇒ 原样透传", () => {
        assert.equal(mm.normalizeMcu(undefined), null, "无 mcu ⇒ null（不造默认）");
        // ⚠️ 前端是**形状归一化**，**不判 isa 语义**（N-2）⇒ `isa` 空串也**透传**，
        //    语义判定留给契约 V-23（词法非法 ⇒ E_PLT-21）⇒ 这里不重抄第二份词法。
        assert.deepEqual(mm.normalizeMcu({ isa: "" }), { isa: "" }, "isa 空串 ⇒ 原样透传（前端不判）");
        assert.equal(mm.normalizeMcu({ isa: "rv32imac", vendor: "", model: "" }).vendor, undefined,
            "空串 vendor / model 必须被丢（与 serde 的 skip 同形）");
    });

    test("N-1 草稿带 mcu ⇒ 提交出去的描述**真的带** mcu（往返不进草稿时丢失）", () => {
        const d = mm.defaultDraft();
        d.mcu = { isa: "rv32imac", vendor: "qemu", model: "virt", vector_table: { base: 2147483648, entries: 16 } };
        const desc = mm.draftToDescription(d);
        assert.equal(desc.mcu.isa, "rv32imac", "提交出去的描述必须带上 mcu.isa");
        assert.equal(desc.mcu.vector_table.entries, 16, "向量表必须原样带出");
    });

    // -----------------------------------------------------------------------
    // N-3 · 只展示
    // -----------------------------------------------------------------------

    test("N-3 MCU 区**只展示**：草稿 MCU 与生效板 MCU 都出来，且**没有编辑入口**", () => {
        setup();
        m03.setState("memory-map", (prev) => ({
            ...prev,
            draft: {
                ...prev.draft,
                mcu: { isa: "rv32ima", vendor: "vendor-x", model: "model-y" },
            },
        }));
        rerender();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(MCU_BOARD, false, true);
        rerender();

        assert.ok($("[data-mcu-display='1']"), "MCU 展示卡片必须在");
        const text = $("[data-mcu-display='1']").textContent;
        assert.ok(text.includes("rv32ima"), "草稿 MCU 的 isa 必须显示出来");
        assert.ok(text.includes("vendor-x"), "草稿 MCU 的 vendor 必须显示出来");
        assert.ok(text.includes("rv32imac"), "生效板 MCU 的 isa 必须显示出来");
        assert.ok(text.includes("只展示 · 不编辑"), "必须自标「只展示 · 不编辑」");

        // ⚠️ **零编辑入口**：MCU 卡片里**不许**有输入框 / data-action 按钮。
        const card = $("[data-mcu-display='1']");
        assert.equal(card.querySelectorAll("input").length, 0, "MCU 区不能有任何输入框（只展示）");
        assert.equal(card.querySelectorAll("[data-action]").length, 0, "MCU 区不能有任何动作按钮（只展示）");
    });

    test("N-3 无 mcu 时给出空态文案（不假装有）", () => {
        setup();
        assert.ok($("[data-mcu-none='1']"), "没声明 MCU ⇒ 必须给「未声明 MCU」空态");
    });

    // -----------------------------------------------------------------------
    // N-4 · 不自动实例化 + 模板入口
    // -----------------------------------------------------------------------

    test("N-4 加载固件（推生效板）⇒ 编辑面**不自动实例化**生效板描述", () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        // ⚠️ 生效板**带着 4 个器件** ⇒ 旧行为会把它们实例化成草稿。
        router.emitMemoryMap(MCU_BOARD, false, true);

        // ① 横幅：当前生效板（自动选择 · 只读）。
        const banner = $("[data-mmap-effective-banner]");
        assert.ok(banner, "加载固件后必须出现「当前生效板（自动选择 · 只读）」横幅");
        assert.ok(banner.textContent.includes("QEMU virt 板"), "横幅必须说出生效板的名字");

        // ② 草稿**不是**生效板的实例化：默认草稿只有 1 条 RAM，没有外设窗口。
        const d = draft();
        assert.equal(d.regions.length, 1, "草稿必须是默认草稿（1 条 RAM）——生效板描述**不许**被实例化进来");
        assert.equal(
            d.regions.filter((r) => r.kind === "peripheral").length, 0,
            "草稿不能带出生效板的 4 个器件（CLINT/PLIC/UART0/GPIO0 都不许自动冒出来）",
        );
        assert.equal(d.mcu, undefined, "草稿不能自动带出生效板的 mcu（用户自己配置）");

        // ③ 模板入口：按钮存在。
        const tpl = $("[data-action='load-current'][data-template-entry='1']");
        assert.ok(tpl, "「从内置模版开始」按钮必须在（模板是入口，不是默认选）");
    });

    test("N-4 用户动过草稿 ⇒ 横幅消失（不再提示）", () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(MCU_BOARD, false, true);
        m03.setState("memory-map", (prev) => ({ ...prev, draft: { ...prev.draft, name: "我自己改的板" } }));
        rerender();
        assert.equal($("[data-mmap-effective-banner]"), null,
            "草稿被用户动过 ⇒ 「自动选择」横幅不该再提示（那是在教用户做事）");
    });

    test("N-4c 显式重建草稿（内容=默认）⇒ 横幅**仍在**（提示是给默认草稿的）", () => {
        setup();
        const router = fakeKernel();
        app.wireMemoryMap(router);
        router.emitMemoryMap(MCU_BOARD, false, true);
        // ⚠️ 点「重建草稿」= 把 draft 设成默认草稿对象 ⇒ **内容仍是默认** ⇒
        // 「自动选择 · 只读」提示必须还在（B-166a 真机段等它）。
        m03.setState("memory-map", (prev) => ({ ...prev, draft: mm.defaultDraft() }));
        rerender();
        assert.ok($("[data-mmap-effective-banner]"),
            "重建草稿后内容=默认 ⇒ 横幅必须还在");
    });

    // -----------------------------------------------------------------------
    // N-5 · 基址 16 进制
    // -----------------------------------------------------------------------

    test("N-5 基址统一 16 进制显示（草稿十进制字符串不变 ⇒ 提交口径不变）", () => {
        setup();
        m03.setState("memory-map", (prev) => ({
            ...prev,
            draft: { ...prev.draft, regions: [{ kind: "ram", base: "2147483648", size: "8388608" }] },
        }));

        const input = $("[data-mmap-region-field='base']");
        assert.ok(input, "RAM 段的基址输入框必须在");
        assert.equal(input.value, "0x80000000", "基址必须以 16 进制显示（2147483648 ⇒ 0x80000000）");
        // ⚠️ 草稿里**仍是十进制字符串**（采集时两种写法都认）⇒ 显示改了，数据没改。
        assert.equal(draft().regions[0].base, "2147483648", "草稿里的 base 不许被显示层改写");

        // 纯函数：formatBase 的边界。
        assert.equal(mm.formatBase("0x0"), "0x0", "0 ⇒ 0x0（有基址就是 0）");
        assert.equal(mm.formatBase(""), "", "缺 base（装配期分配）⇒ 空串，不显示 0x0");
        assert.equal(mm.formatBase("0x80000000"), "0x80000000", "已是 16 进制 ⇒ 原样");
        assert.equal(mm.formatBase(16), "0x10", "数字 16 ⇒ 0x10");
    });

    test("N-5 采集时 16 进制输入也认（parseNumber 两种写法都认）", () => {
        const n = mm.parseNumber("0x80000000");
        assert.equal(n, 2147483648, "0x 写法必须能采集回来（往返安全）");
    });

    // -----------------------------------------------------------------------
    // N-6 · 改 MCU 必须触发整份替换（否则「换 isa 提交」永远 mode="none"）
    // -----------------------------------------------------------------------

    test("N-6 只换 mcu.isa ⇒ plan 走整份替换（命令真的发得出去）", () => {
        const base = JSON.parse(JSON.stringify(MCU_BOARD));
        const draft = JSON.parse(JSON.stringify(MCU_BOARD));
        draft.mcu = { ...draft.mcu, isa: "rv64ima" };
        const plan = mm.planMemoryMapUpdate(base, draft);
        assert.equal(plan.mode, "replace",
            "换 isa 提交必须整份替换（差量契约表达不了 mcu）");
    });

    test("N-6b 去掉 mcu ⇒ 也是改动（整份替换）", () => {
        const base = JSON.parse(JSON.stringify(MCU_BOARD));
        const draft = JSON.parse(JSON.stringify(MCU_BOARD));
        delete draft.mcu;
        const plan = mm.planMemoryMapUpdate(base, draft);
        assert.equal(plan.mode, "replace", "去掉 mcu 也是改动，必须能提交出去");
    });

    test("N-6c 同样的 mcu、键序不同 ⇒ 不算改动（不假触发整份）", () => {
        const base = JSON.parse(JSON.stringify(MCU_BOARD));
        const draft = JSON.parse(JSON.stringify(MCU_BOARD));
        draft.mcu = {
            vector_table: { base: 2147483648, entries: 16 },
            model: "virt",
            vendor: "qemu",
            isa: "rv32imac",
        };
        const plan = mm.planMemoryMapUpdate(base, draft);
        assert.notEqual(plan.mode, "replace",
            "同样的 MCU、键序不同 ⇒ 不许假触发整份替换");
    });

    test("N-6d 既没有 mcu 的两份 ⇒ 不算改动（缺省 = 没有 MCU）", () => {
        const base = { id: "b", name: "B", priority: "1", regions: [] };
        const draft = { id: "b", name: "B", priority: "1", regions: [] };
        const plan = mm.planMemoryMapUpdate(base, draft);
        assert.notEqual(plan.mode, "replace", "两边都没有 mcu ⇒ 不假触发");
    });
});
