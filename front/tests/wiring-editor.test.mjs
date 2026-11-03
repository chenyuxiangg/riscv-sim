// v0.74 批 · STAGE-50 · **D-177 的 D** —— 「**连线编辑面**」（界面上画线）。
//
// ## 这一套盯住什么
//
// 规格 `docs/detailed-design/STAGE-50_V074_Pin_Level_Routing.md` §4 里与
// **前端连线编辑面**有关的那几条：
//
// - **N-3**：前端**能画线**（加一根 / 删一根 ⇒ ⇒ 草稿里有 ⇒ ⇒ 提交回去还在）；
// - **N-4**：界面上画的线**真的进** `wiring` 段（⇒ ⇒ 线上形态逐字相同）；
// - **N-2**：前端**零处**自己判「描述格式 / 引脚号对不对」（⇒ ⇒ 合法性**只**
//   由内核判）；
// - **X-3**：引脚数**只有一处**（契约层 `GPIO_PIN_COUNT`）⇒ ⇒ 前端**查
//   内核**（**不抄**）；
// - **X-2 / 跨层**：前端镜像的**事件词表**与契约层**逐字相同**（⇒ ⇒ 两侧
//   漂了会红）⇒ ⇒ ⇒ 「一个约束一个落点」在**前端这一侧**也成立。
//
// ## ⚠️ **为什么「界面上画线」要有真判据**
//
// v0.34 ~ v0.73 一直挂着一条**已知边界**：「从界面提交的板**永远没有**
// `wiring`」⇒ ⇒ 本批把它**拆掉** ⇒ ⇒ ⇒ 判据必须落在「**真的能画**」与
// 「**画出来的线真的进契约**」上（⇒ ⇒ 光看面板上有个按钮**不够**）。
//
// ## ⚠️ **断言值是写死的字面量**，不从被测实现反推。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
const dom = installDom(indexHtml);

// ⚠️ **关掉 `app.js` 的自启动**（与既有套件同一条理由）：M-03 的视图状态是
// **模块级单例** ⇒ bootstrap 的异步 reset 可能落在任意两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const message = await importFront("lib/message.js");
const mm = await importFront("views/components/memory-map.js");
const we = await importFront("views/components/wiring-editor.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

/**
 * 装一套干净的「板级内存图编辑器」现场。
 *
 * ⚠️ **视图状态是模块级单例** ⇒ 每个用例都重来一遍。
 *
 * @param {Array<object>|null} [deviceTypes] — 内核的器件类型表
 */
function setup(deviceTypes = null) {
    m03.reset();
    m01.assembleWorkspace("product");
    // ⚠️ **类型表要赶在路由之前**放进视图状态 ⇒ ⇒ 路由那次渲染**才**看得见它
    //   （⇒ ⇒ 「内核报没报类型表」这件事**看得见**）。
    if (deviceTypes) {
        m03.setState("memory-map", (prev) => ({ ...prev, deviceTypes }));
    }
    m01.route("#/memory-map");
}

/** 编辑器当前的草稿。 */
function draft() {
    return mm.currentDraft(m03.getState("memory-map"));
}

/**
 * 给板加一个**指定种类**的器件（走界面那条路 ⇒ ⇒ 与真人操作同形）。
 *
 * @param {string} kind — 器件种类（`"gpio"` / `"plic"` …）
 */
function addDevice(kind) {
    $("[data-action='add-peripheral']").dispatchEvent(new Event("click"));
    // ⚠️ **改「最后一行」**（刚加的那一个）⇒ ⇒ 否则改的是**第一行**（⇒ ⇒
    //   两次调用会把**同一个**器件换两次类型 ⇒ ⇒ 板上只剩一个器件）。
    const selects = $$("[data-mm-device-kind]");
    const select = selects[selects.length - 1];
    select.value = kind;
    select.dispatchEvent(new Event("change"));
}

/**
 * 内核的器件类型表（**带 `pin_count`** —— 裁决 F：引脚数**来自内核**）。
 */
const KERNEL_TYPES = [
    { kind: "gpio", window_bytes: 16, registers: [], params: [], pin_count: 32 },
    { kind: "uart16550", window_bytes: 16, registers: [], params: [] },
    { kind: "plic", window_bytes: 16, registers: [], params: [] },
];

// ---------------------------------------------------------------------------

describe("连线编辑面 · **N-3**（能画线）", () => {
    test("N-3 面板上**有**连线编辑面（一张卡 + 一个「+ 连线」入口）", () => {
        setup();
        assert.ok($("[data-wiring-rows]"), "面板上没有连线那张表");
        assert.ok($("[data-action='add-wiring']"), "面板上没有「+ 连线」按钮");
    });

    test("N-3 点「+ 连线」⇒ ⇒ 草稿里**多一根**", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        assert.equal(draft().wiring, undefined, "还没有线时草稿里不该有 `wiring`");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        const wires = draft().wiring ?? [];
        assert.equal(wires.length, 1, "点「+ 连线」没有加出线来");
        assert.equal(typeof wires[0].from, "object");
        assert.equal(typeof wires[0].to, "object");
    });

    test("N-3 点「删除」⇒ ⇒ 草稿里**少一根**", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        assert.equal((draft().wiring ?? []).length, 2, "两根线没加上");
        const button = $$("[data-action='remove-wiring']").find(
            (b) => Number(b.getAttribute("data-wiring-index")) === 0,
        );
        assert.ok(button, "第一根线上没有删除按钮");
        button.dispatchEvent(new Event("click"));
        assert.equal((draft().wiring ?? []).length, 1, "点「删除」没有删掉那一根");
    });

    test("N-3 删掉**最后一根** ⇒ ⇒ 草稿里**不再有** `wiring` 这个键", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        $("[data-action='remove-wiring']").dispatchEvent(new Event("click"));
        assert.equal("wiring" in draft(), false, "删光了还留着 `wiring` 键（缺省优先：不该塞空数组）");
    });

    test("N-3 新加的那根线**默认就是完整的一行**（不是空行）", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        const wire = (draft().wiring ?? [])[0];
        assert.ok(wire, "没有新线");
        assert.notEqual(wire.from.device, "", "新线的源器件不能是空的（空行 ⇒ 提交必被内核拒）");
        assert.notEqual(wire.from.event, "", "新线的事件名不能是空的");
        assert.notEqual(wire.to.device, "", "新线的目标不能是空的");
        assert.equal(typeof wire.to.source, "number");
    });
});

describe("连线编辑面 · **N-4**（画出来的线真的进 `wiring` 段）", () => {
    test("N-4 提交时**真的发出**那根线（载荷形状逐字相同）", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        // ⚠️ **改面板上的输入框**（DOM 是真相）⇒ ⇒ 用户敲什么就发什么。
        const event = $("[data-wiring-field='from.event']");
        event.value = "pin_irq";
        event.dispatchEvent(new Event("input"));
        const pin = $("[data-wiring-field='from.pin']");
        pin.value = "3";
        pin.dispatchEvent(new Event("input"));

        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='submit-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        assert.ok(detail, "点提交没有发出命令");
        const wiring = detail.params.description.wiring;
        assert.ok(Array.isArray(wiring), "提交的那份描述里没有 `wiring` 段");
        assert.equal(wiring.length, 1);
        // ⚠️ **器件 id 从草稿里读**（不写死）⇒ ⇒ 板上那两台器件叫什么，线
        //   就指着谁（⇒ ⇒ 判据钉的是「**关系**」，不是某个名字）。
        const devices = we.wiringDevices(draft());
        const gpio = devices.find((d) => d.kind === "gpio");
        const plic = devices.find((d) => d.kind === "plic");
        assert.ok(gpio && plic, "板上该有一台 GPIO 与一台 PLIC");
        assert.deepEqual(
            wiring[0],
            {
                from: { device: gpio.id, event: "pin_irq", pin: 3 },
                to: { device: plic.id, source: 0 },
            },
            "线上形态必须逐字相同（`from` / `to` / `pin`）",
        );
    });

    test("N-4 引脚号**留空** ⇒ ⇒ 那个键**不带**（不是 0、不是 null）", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        const pin = $("[data-wiring-field='from.pin']");
        pin.value = "";
        pin.dispatchEvent(new Event("input"));

        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='submit-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        const wire = (detail?.params?.description?.wiring ?? [])[0];
        assert.ok(wire, "没有提交出线来");
        assert.equal("pin" in wire.from, false, "留空时那个键**不许**出现");
    });

    test("N-4 没有连线时提交 ⇒ ⇒ 那份描述**不带** `wiring`（缺省优先）", () => {
        setup();
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='submit-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        // ⚠️ **v0.74 批 · 施工期订正**：命令详情是 `{ action, requestId, viewId,
        //   params }`（`emitCommand` 的形状）⇒ ⇒ 这里**必须**读 `params`
        //   ⇒ ⇒ 原来读 `payload` ⇒ ⇒ 恒为 `undefined` ⇒ ⇒ 断言拿 `{}` 比
        //   ⇒ ⇒ **恒真通过**（⇒ ⇒ 正是「测试因错误原因通过」那一类）。
        const description = detail?.params?.description ?? null;
        assert.ok(description, "没有连线时提交也该有 `description`（否则下面的判据没东西可断）");
        assert.equal("wiring" in description, false, "没有连线时不该凭空多一个 `wiring` 键");
    });

    test("N-4 越界的引脚号**原样送到内核**（前端**不**替它裁）", () => {
        setup();
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        const pin = $("[data-wiring-field='from.pin']");
        pin.value = "300";
        pin.dispatchEvent(new Event("input"));

        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='submit-memory-map']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        const wire = (detail?.params?.description?.wiring ?? [])[0];
        assert.equal(
            wire.from.pin, 300,
            "越界的引脚号必须**原样**送到内核（前端裁了 ⇒ ⇒ 越界就永远判不出来 ⇒ ⇒ 第二个真相源）",
        );
    });
});

describe("连线编辑面 · **N-2**（前端不判合法性）", () => {
    test("N-2 前端**零处**自己判「引脚号在不在范围内」", () => {
        const sources = [
            ["wiring-editor", readFileSync(resolve(frontDir, "views/components/wiring-editor.js"), "utf8")],
            ["memory-map", readFileSync(resolve(frontDir, "views/components/memory-map.js"), "utf8")],
        ];
        // ⚠️ **先把注释去掉**再扫 ⇒ ⇒ 否则「注释里提了一句 `GPIO_PIN_COUNT`」
        //   也会红（⇒ ⇒ 那种红**不判别力**）⇒ ⇒ ⇒ 只扫**代码**。
        for (const [label, raw] of sources) {
            const src = raw.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
            assert.ok(
                !/pin\s*(?:>=|<=|>|<)\s*\d/.test(src),
                `${label}：前端**不许**拿引脚号跟数字比（越界由内核的 V-18 判 ⇒ ⇒ 既有码 E_PLT-16）`,
            );
            assert.ok(
                !/GPIO_PIN_COUNT|\bpin_count\s*[:=]\s*\d/.test(src),
                `${label}：前端**不许**抄一份引脚数（真相在契约层的 GPIO_PIN_COUNT）`,
            );
        }
    });

    test("N-2 前端**零处**自己判「描述格式对不对」", () => {
        const src = readFileSync(resolve(frontDir, "views/components/wiring-editor.js"), "utf8");
        assert.ok(
            !/schema_version\s*!==|schema_version\s*==\s*\d/.test(src),
            "前端**不许**自己判描述的格式版本对不对（合法性只由内核判）",
        );
    });
});

describe("连线编辑面 · **X-3**（引脚数来自内核，不抄）", () => {
    test("X-3 界面上那句「N 个引脚」**来自内核的类型表**", () => {
        setup(KERNEL_TYPES);
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        const note = $("[data-wiring-pin-note]");
        assert.ok(note, "引脚号旁边没有那句提示");
        assert.match(note.textContent, /32/, "提示里的引脚数必须就是内核给的那个");
    });

    test("X-3 内核**没报**类型表 ⇒ ⇒ 界面**不显示**那个提示（绝不编一个数）", () => {
        setup(null);
        addDevice("gpio");
        addDevice("plic");
        $("[data-action='add-wiring']").dispatchEvent(new Event("click"));
        assert.equal(
            $("[data-wiring-pin-note]"), null,
            "内核没给引脚数时界面**不许**编一个出来",
        );
    });

    test("X-3 引脚数**查**内核的类型表（`pin_count`）", () => {
        assert.equal(we.pinCountOf(KERNEL_TYPES, "gpio"), 32);
        assert.equal(we.pinCountOf(KERNEL_TYPES, "uart16550"), null, "没有引脚的器件 ⇒ ⇒ `null`");
        assert.equal(we.pinCountOf(null, "gpio"), null);
    });
});

describe("连线编辑面 · **跨层**（镜像 == 契约层源码）", () => {
    test("前端镜像的**事件词表** == 契约层源码里那张表", () => {
        const platformRs = resolve(repoRoot, "crates/contract/src/platform.rs");
        const src = readFileSync(platformRs, "utf8");

        // ⚠️ **逐字读 Rust 源码**（⇒ ⇒ 不是「跟一个常量比」那种恒真写法）。
        const constOf = (name) => {
            const m = src.match(new RegExp(`pub const ${name}: &str = "([^"]+)";`));
            assert.ok(m, `契约层读不到 ${name}`);
            return m[1];
        };
        const table = src.match(/pub const WIRING_EVENTS: &\[WiringEventInfo\] = &\[([\s\S]*?)\n\];/);
        assert.ok(table, "契约层读不到 `WIRING_EVENTS` 那张表");
        const entries = [...table[1].matchAll(/WiringEventInfo\s*\{([\s\S]*?)\}/g)].map((m) => m[1]);
        const expected = entries.map((body) => {
            const name = body.match(/name:\s*([A-Z_0-9]+|WIRING_EVENT_[A-Z_0-9]+)/)[1];
            const sourceKind = body.match(/source_kind:\s*"([^"]+)"/)[1];
            const needsPin = body.match(/needs_pin:\s*(true|false)/)[1] === "true";
            const literal = name.match(/^"([^"]+)"$/);
            return {
                name: literal ? literal[1] : constOf(name),
                source_kind: sourceKind,
                needs_pin: needsPin,
            };
        });

        assert.deepEqual(
            message.WIRING_EVENTS.map((one) => ({ ...one })),
            expected,
            "前端镜像的事件词表必须与契约层**逐字相同**（两侧漂了 ⇒ ⇒ 界面上的建议会骗人）",
        );
    });

    test("前端镜像的**四个版本号**与契约层**逐字相同**（X-1）", () => {
        const contractSrc = readFileSync(resolve(repoRoot, "crates/contract/src/lib.rs"), "utf8");
        const platformSrc = readFileSync(resolve(repoRoot, "crates/contract/src/platform.rs"), "utf8");
        const expectations = [
            ["SCHEMA_VERSION", contractSrc],
            ["PLATFORM_SCHEMA_VERSION", platformSrc],
            ["PLATFORM_RULES_VERSION", platformSrc],
        ];
        for (const [name, src] of expectations) {
            const m = src.match(new RegExp(`pub const ${name}:\\s*u32\\s*=\\s*(\\d+)`));
            assert.ok(m, `契约层必须定义 ${name}`);
            assert.equal(Number(message[name]), Number(m[1]), `${name}：前端镜像必须与契约层**逐字相同**`);
        }
    });
});
