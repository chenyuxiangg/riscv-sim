// 任意地址断点 + 任意地址读内存（STAGE-39 · v0.54 批）的前端门禁（A-1 ~ A-16）。
//
// ⚠️ **本批的第一约束是「零契约变更」**（裁决 L）⇒ ⇒ A-16 钉住四个版本号
//   与内核 `hit_check` 的签名**一个字没改** ⇒ ⇒ 「界面没把它们露出来」
//   这个根因（§1.3）落地了 ⇒ ⇒ 不是「偷偷升了契约」换来的。
//
//   ⚠️ **v0.57 批（契约 v9）例外**：契约层**真的**升版了（8 → 9，符号查询
//   + 条件断点 + 整段转储）⇒ ⇒ A-16 的版本字面量随之更新、判据**改名**
//   （原名「逐字未改」已不成立）⇒ ⇒ **断言一条都没删** ⇒ ⇒ M-08 的
//   `hit_check` 签名那条**继续**有效。
//
// ⚠️ **第二约束是「前端只挡『压根不是一个数』」**（裁决 E）⇒ ⇒ A-5 钉住
//   「奇数地址**照样发**」⇒ ⇒ 前端**不许**抢内核的判据（对齐 / 是否已映射
//   都由内核说）⇒ ⇒ **不写第二份校验器**。
//
// ⚠️ **第三约束是「两个语义不许混」**（裁决 I）⇒ ⇒ A-12（运行中读内存
//   **拒绝 + 弹窗**）与 A-13（运行中加断点**仍然发**）**成对**出现 ⇒ ⇒
//   把「运行中」也套到断点上 ⇒ ⇒ A-13 立刻红。
//
// ⚠️ **判据必须正向**：先钉住「被观测的对象**存在**」（输入框在、按钮在、
//   宽度三档在），再逐条断言它的**内容** ⇒ ⇒ 对象整个没了时不会「零断言
//   假绿」（v0.53 批的教训）。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器** + `closest()`。
//
// ⚠️ **真机**（真的在那个地址停下、真的读回那一行）由
//   `tools/browser-verify.mjs` 的 `addrInput` 段（B-55 ~ B-60）负责。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, resolve, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");
const repoRoot = resolve(frontDir, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（与 `run-live.test.mjs` 同一条理由）：
// M-03 的视图状态是**模块级单例** ⇒ bootstrap 的异步副作用可能落在任意
// 两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const { WRITE_SIZE } = await importFront("lib/message.js");

// M-02 的渲染监听器只挂一次（模块级单例）。
await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 替身与工具
// ---------------------------------------------------------------------------

/**
 * 一个**够用**的命令路由器替身：记下发出去的命令。
 *
 * @returns {{sent: object[], dispatchCommand: (p: object) => Promise<object>}}
 */
function fakeRouter() {
    /** @type {object[]} */
    const sent = [];
    return {
        sent,
        async dispatchCommand(payload) {
            sent.push(payload);
            return { value: {} };
        },
    };
}

/**
 * 寄存器 / 内存服务的替身：只记「读了几次、读的什么」。
 *
 * @returns {{calls: any[], readMemory: (addr: number, size: string) => Promise<object>}}
 */
function fakeRegisterMemory() {
    /** @type {any[]} */
    const calls = [];
    return {
        calls,
        async readMemory(addr, size) {
            calls.push([addr, size]);
            return { value: {} };
        },
    };
}

/**
 * 把视图挂上去并渲染出来（**生产路径**：工作台路由 ⇒ `riscv:render` ⇒
 * `applyRenderEvent` ⇒ 视图挂监听器 + 写 `innerHTML`）。
 *
 * ⚠️ **不是**直接调 `view.render()` 塞字符串 ⇒ ⇒ 那样**测不到**事件委托
 *   与「重渲染后仍能点」⇒ ⇒ A-14 会变成空转。
 *
 * @param {string} hash
 * @returns {object} 视图的 body 元素
 */
function mountView(hash) {
    m01.assembleWorkspace("embedded");
    m01.route(hash);
    const body = dom.document.querySelector(`[data-view-body="${hash.slice(2)}"]`);
    assert.ok(body, `视图体未挂载：${hash}`);
    return body;
}

/**
 * 收集窗口上的 `riscv:command` 信号。
 *
 * @returns {{signals: object[], stop: () => void}}
 */
function collectSignals() {
    /** @type {object[]} */
    const signals = [];
    const handler = (ev) => signals.push(ev.detail);
    dom.window.addEventListener("riscv:command", handler);
    return { signals, stop: () => dom.window.removeEventListener("riscv:command", handler) };
}

/**
 * 读一个视图 body 里**某个容器**的输入框。
 *
 * @param {object} body
 * @param {string} boxSelector — 容器选择器
 * @param {string} inputSelector — 输入框选择器
 * @returns {object}
 */
function boxInput(body, boxSelector, inputSelector) {
    const box = body.querySelector(boxSelector);
    assert.ok(box, `容器不存在：${boxSelector}`);
    const input = box.querySelector(inputSelector);
    assert.ok(input, `输入框不存在：${inputSelector}`);
    return input;
}

/** 递归列出某个目录下的所有 `.js` 文件。 */
function jsFiles(dir) {
    const out = [];
    for (const name of readdirSync(dir)) {
        const full = join(dir, name);
        if (statSync(full).isDirectory()) out.push(...jsFiles(full));
        else if (name.endsWith(".js")) out.push(full);
    }
    return out;
}

// ---------------------------------------------------------------------------
// A-1 ~ A-2：界面**给得出**那个地址
// ---------------------------------------------------------------------------

describe("任意地址入口（视图层）", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("A-1 · 调试视图的断点卡**有**地址输入框与「加断点」按钮", () => {
        const body = mountView("#/debug");
        const box = body.querySelector("[data-addr-input=\"breakpoint\"]");
        assert.ok(box, "断点卡里必须有地址输入的容器");
        assert.ok(
            box.querySelector("[data-bp-addr-input]"),
            "断点卡里必须有地址输入框（data-bp-addr-input）",
        );
        assert.ok(
            box.querySelector("[data-action=\"set-breakpoint\"]"),
            "断点卡里必须有「加断点」按钮",
        );
        assert.ok(
            box.querySelector("[data-bp-addr-error]"),
            "断点卡里必须有提示位（data-bp-addr-error）",
        );
    });

    test("A-2 · 草稿**住视图状态** ⇒ ⇒ 重渲染**不丢**（落点唯一）", () => {
        const body = mountView("#/debug");
        // -- 写草稿（模拟用户敲了字）------------------------------------
        const input = boxInput(body, "[data-addr-input=\"breakpoint\"]", "[data-bp-addr-input]");
        input.value = "0x2000";
        input.dispatchEvent(new Event("input", { bubbles: true }));

        assert.equal(
            m02.debuggerDraft().bpAddr,
            "0x2000",
            "草稿必须**住视图状态**（不是只存在 DOM 里）",
        );

        // -- 重渲染 ⇒ ⇒ 草稿还在 ------------------------------------------
        dom.window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: { viewId: "debug", type: "update", state: m03.getState("debug") ?? {} },
        }));
        const after = boxInput(
            dom.document.querySelector('[data-view-body="debug"]'),
            "[data-addr-input=\"breakpoint\"]",
            "[data-bp-addr-input]",
        );
        assert.equal(after.value, "0x2000", "重渲染后草稿**不许丢**");
    });
});

// ---------------------------------------------------------------------------
// A-3 ~ A-5：断点入口
// ---------------------------------------------------------------------------

describe("任意地址断点", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("A-3 · 点「加断点」⇒ ⇒ `params.addr` = **用户输入的那个**", async () => {
        const body = mountView("#/debug");
        const input = boxInput(body, "[data-addr-input=\"breakpoint\"]", "[data-bp-addr-input]");
        input.value = "0x2000";

        const router = fakeRouter();
        const { signals, stop } = collectSignals();
        dom.window.addEventListener("riscv:command", (ev) => {
            void app.handleCommand(ev.detail, { commandRouter: router });
        });
        body.querySelector("[data-addr-input=\"breakpoint\"]")
            .querySelector("[data-action=\"set-breakpoint\"]")
            .click();
        stop();

        assert.equal(signals.length, 1, "点一次「加断点」必须发**一条**命令");
        assert.equal(signals[0].action, "set-breakpoint", "动作名必须是 set-breakpoint");
        assert.equal(
            signals[0].params.addr,
            0x2000,
            "地址必须是**用户输入的那个**（⛔ 不是 0x100、⛔ 不是 0）",
        );
        // ⚠️ **信号只是「起点」** ⇒ ⇒ 还要钉住 **app 层把它原样交给内核**
        //   ⇒ ⇒ 否则「界面发对了、app 层又写死成别的地址」这一种坏法会漏网。
        const sent = router.sent.filter((p) => p?.SetBreakpoint);
        assert.equal(sent.length, 1, "app 层必须把这条命令**真的发到内核**");
        assert.equal(
            sent[0].SetBreakpoint.addr,
            0x2000,
            "内核收到的地址必须**逐字**是用户输入的那个",
        );
    });

    test("A-4 · 输入**解析不出来** ⇒ ⇒ **不发**命令 **且** 有提示", () => {
        const body = mountView("#/debug");
        const box = body.querySelector("[data-addr-input=\"breakpoint\"]");
        const input = box.querySelector("[data-bp-addr-input]");
        input.value = "zzz";

        const { signals, stop } = collectSignals();
        box.querySelector("[data-action=\"set-breakpoint\"]").click();
        stop();

        assert.equal(signals.length, 0, "解析不出来 ⇒ ⇒ **一条命令都不发**");
        const err = box.querySelector("[data-bp-addr-error]");
        assert.ok(err, "必须有提示位");
        assert.ok(
            String(err.textContent ?? "").length > 0,
            "解析不出来 ⇒ ⇒ 必须**看得见**提示（「点了没反应」是最差的 UX）",
        );
    });

    test("A-5 · 输入**奇数**地址 ⇒ ⇒ **照样发**命令（前端不预校验对齐）", () => {
        const body = mountView("#/debug");
        const box = body.querySelector("[data-addr-input=\"breakpoint\"]");
        const input = box.querySelector("[data-bp-addr-input]");
        input.value = "0x101";

        const { signals, stop } = collectSignals();
        box.querySelector("[data-action=\"set-breakpoint\"]").click();
        stop();

        assert.equal(signals.length, 1, "奇数地址**照样发**（对齐是**内核**的判据）");
        assert.equal(signals[0].params.addr, 0x101, "地址必须逐字是用户输入的那个");
    });
});

// ---------------------------------------------------------------------------
// A-6 ~ A-10：读内存入口
// ---------------------------------------------------------------------------

describe("任意地址读内存", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("A-6 · 内存视图**有**地址输入框与宽度选择（三档齐全）", () => {
        const body = mountView("#/regmem");
        const box = body.querySelector("[data-addr-input=\"memory\"]");
        assert.ok(box, "内存视图必须有地址输入的容器");
        assert.ok(box.querySelector("[data-mem-addr-input]"), "必须有地址输入框");
        const select = box.querySelector("[data-mem-size]");
        assert.ok(select, "必须有宽度选择（data-mem-size）");
        const options = select.querySelectorAll("option");
        assert.equal(options.length, 3, "宽度必须**三档**（Byte / Half / Word）");
        assert.deepEqual(
            options.map((o) => o.getAttribute("value")),
            [WRITE_SIZE.Byte, WRITE_SIZE.Half, WRITE_SIZE.Word],
            "三档必须**逐字**是契约的 `WRITE_SIZE`（不新造枚举）",
        );
    });

    test("A-7 · 点「读内存」⇒ ⇒ `params.addr` + `params.size` 都是界面给的", async () => {
        const body = mountView("#/regmem");
        const box = body.querySelector("[data-addr-input=\"memory\"]");
        box.querySelector("[data-mem-addr-input]").value = "0x3000";
        box.querySelector("[data-mem-size]").value = WRITE_SIZE.Half;

        const router = fakeRouter();
        const regmem = fakeRegisterMemory();
        const { signals, stop } = collectSignals();
        dom.window.addEventListener("riscv:command", (ev) => {
            void app.handleCommand(ev.detail, { commandRouter: router, registerMemory: regmem });
        });
        box.querySelector("[data-action=\"read-mem\"]").click();
        stop();

        assert.equal(signals.length, 1, "点一次「读内存」必须发**一条**命令");
        assert.equal(signals[0].action, "read-mem", "动作名必须是 read-mem");
        assert.equal(signals[0].params.addr, 0x3000, "地址必须是用户输入的那个");
        assert.equal(signals[0].params.size, WRITE_SIZE.Half, "宽度必须是**选的那个**");
        // ⚠️ **信号只是「起点」** ⇒ ⇒ 还要钉住 **app 层把它原样交给服务**
        //   ⇒ ⇒ 否则「界面发对了、app 层又写死成 0x100」这一种坏法会漏网。
        assert.deepEqual(
            regmem.calls,
            [[0x3000, WRITE_SIZE.Half]],
            "服务层收到的**地址与宽度**必须逐字是界面给的那两个",
        );
    });

    test("A-8 · 视图层与 app 层**零命中**写死的 `0x100`", () => {
        const files = [
            ...jsFiles(resolve(frontDir, "views")),
            resolve(frontDir, "app.js"),
        ];
        // ⚠️ **必须按「完整的那个数」匹配** ⇒ ⇒ `0x1000`（外设窗口大小）
        //   **不是** `0x100` ⇒ ⇒ 用十六进制数字边界 ⇒ ⇒ 不误伤。
        const literal = /(?<![0-9a-fA-F])0x100(?![0-9a-fA-F])/;
        const hits = [];
        for (const file of files) {
            const src = readFileSync(file, "utf8");
            if (literal.test(src)) hits.push(file.replace(repoRoot, "."));
        }
        assert.deepEqual(hits, [], "「任意地址」落地 ⇒ ⇒ 写死的 `0x100` 必须**零命中**");
    });

    test("A-9 · 宽度**只**用契约的 `WRITE_SIZE`（前端不新造枚举）", () => {
        const src = readFileSync(resolve(frontDir, "views/m02_viewcomponents.js"), "utf8");
        assert.ok(
            src.includes('from "../lib/message.js"'),
            "视图层必须**从契约镜像**导入 `WRITE_SIZE`",
        );
        assert.ok(
            /WRITE_SIZE\.Byte[\s\S]{0,400}WRITE_SIZE\.Half[\s\S]{0,400}WRITE_SIZE\.Word/.test(src),
            "三档必须**逐字**用 `WRITE_SIZE` 的成员（不新造字符串枚举）",
        );
    });

    test("A-10 · 输入**解析不出来** ⇒ ⇒ **不发**命令 **且** 有提示", () => {
        const body = mountView("#/regmem");
        const box = body.querySelector("[data-addr-input=\"memory\"]");
        box.querySelector("[data-mem-addr-input]").value = "nope";

        const { signals, stop } = collectSignals();
        box.querySelector("[data-action=\"read-mem\"]").click();
        stop();

        assert.equal(signals.length, 0, "解析不出来 ⇒ ⇒ **一条命令都不发**");
        const err = box.querySelector("[data-mem-addr-error]");
        assert.ok(err, "必须有提示位");
        assert.ok(
            String(err.textContent ?? "").length > 0,
            "解析不出来 ⇒ ⇒ 必须**看得见**提示",
        );
    });
});

// ---------------------------------------------------------------------------
// A-11 ~ A-14：回显 / 回归 / 委托
// ---------------------------------------------------------------------------

describe("回显、回归与委托", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("A-11 · 断点列表显示的地址**就是**用户输入的那个", async () => {
        const body = mountView("#/debug");
        const box = body.querySelector("[data-addr-input=\"breakpoint\"]");
        // ⚠️ 裸的十六进制（没有 `0x` 前缀）⇒ ⇒ 解析出来必须是 0x2000 ⇒ ⇒
        // 列表里显示的**不是**原文「2000」而是那个**地址**。
        box.querySelector("[data-bp-addr-input]").value = "2000";

        const router = fakeRouter();
        const { signals, stop } = collectSignals();
        dom.window.addEventListener("riscv:command", (ev) => {
            void app.handleCommand(ev.detail, { commandRouter: router });
        });
        box.querySelector("[data-action=\"set-breakpoint\"]").click();
        stop();

        assert.equal(signals.length, 1, "必须发出**一条**命令");
        const state = m03.getState("debug");
        assert.deepEqual(
            state.debug.breakpoints,
            [0x2000],
            "断点必须下在**用户输入的那个地址**上（不是 0、不是原文）",
        );
        const html = m02.get("debug").render(state);
        assert.ok(
            html.includes('data-bp-addr="0x00002000"'),
            "断点列表必须显示**那个地址**（机器可读的属性）",
        );
    });

    test("A-12 · ⚠️ 回归：运行中点「读内存」⇒ ⇒ **不发**命令 **且** 弹窗", async () => {
        const router = fakeRouter();
        const regmem = fakeRegisterMemory();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        await app.handleCommand(
            { action: "read-mem", params: { addr: 0x100, size: WRITE_SIZE.Word } },
            { commandRouter: router, registerMemory: regmem },
        );

        assert.equal(
            regmem.calls.length,
            0,
            "运行中读内存必须被**拒绝**（v0.53 批的语义不许被绕过）",
        );
        const alert = await importFront("lib/alert.js");
        assert.equal(alert.alertState(), "open", "必须弹窗告诉用户");
    });

    test("A-13 · ⚠️ 回归：运行中点「加断点」⇒ ⇒ **仍然发**命令", async () => {
        const router = fakeRouter();
        await app.handleCommand({ action: "continue" }, { commandRouter: router });

        await app.handleCommand(
            { action: "set-breakpoint", params: { addr: 0x2000 } },
            { commandRouter: router },
        );

        // ⚠️ **「运行中」不许套到断点上** ⇒ ⇒ 断点命令必须**真的发出去**。
        const sent = router.sent.filter((p) => p?.SetBreakpoint);
        assert.equal(sent.length, 1, "运行中加断点必须**仍然发**命令（两个语义不同）");
        assert.equal(
            sent[0].SetBreakpoint.addr,
            0x2000,
            "地址必须**逐字**是用户给的那个",
        );
    });

    test("A-14 · 监听走**事件委托** ⇒ ⇒ 重渲染后**仍能**点", () => {
        const body = mountView("#/debug");
        const input = boxInput(body, "[data-addr-input=\"breakpoint\"]", "[data-bp-addr-input]");
        input.value = "0x3000";
        // ⚠️ **事件委托**：监听器绑在**容器**上 ⇒ ⇒ 这里派发到输入框，
        //   冒泡到容器 ⇒ ⇒ 容器上的监听器接得住。
        input.dispatchEvent(new Event("input", { bubbles: true }));
        assert.equal(m02.debuggerDraft().bpAddr, "0x3000", "输入必须写进草稿");

        // -- 重渲染 ⇒ ⇒ **输入框元素被换掉** --------------------------------
        dom.window.dispatchEvent(new CustomEvent("riscv:render", {
            detail: { viewId: "debug", type: "update", state: m03.getState("debug") ?? {} },
        }));
        const body2 = dom.document.querySelector('[data-view-body="debug"]');
        const box2 = body2.querySelector("[data-addr-input=\"breakpoint\"]");
        assert.notEqual(
            box2.querySelector("[data-bp-addr-input]"),
            input,
            "重渲染后输入框**必须**是新的元素（否则这条判据是空转）",
        );

        const { signals, stop } = collectSignals();
        box2.querySelector("[data-action=\"set-breakpoint\"]").click();
        stop();

        assert.equal(signals.length, 1, "重渲染后**仍然点得动**（委托 ⇒ ⇒ 监听器没丢）");
        assert.equal(signals[0].params.addr, 0x3000, "地址仍然是用户输入的那个");
    });
});

// ---------------------------------------------------------------------------
// A-15 ~ A-16：一个落点 / 零契约变更
// ---------------------------------------------------------------------------

describe("一个落点与契约版本", () => {
    test("A-15 · 「解析地址」这件事**只有一处**落点", () => {
        const files = [
            ...jsFiles(resolve(frontDir, "views")),
            resolve(frontDir, "app.js"),
        ];
        // ⚠️ **用「解析规则本身」当指纹** ⇒ ⇒ 抄一份到别处就会被抓住 ⇒ ⇒
        //   只钉「函数名」的话，第二份实现换个名字就溜过去了。
        const fingerprint = "/^[0-9a-fA-F]{1,8}$/";
        const hits = [];
        for (const file of files) {
            const src = readFileSync(file, "utf8");
            if (src.includes(fingerprint)) hits.push(file.replace(repoRoot, "."));
        }
        assert.deepEqual(
            hits,
            ["./front/views/m02_viewcomponents.js"],
            "「解析地址」**只有一处**落点（前端不写第二份校验器）",
        );
        assert.equal(typeof m02.parseAddrInput, "function", "那个落点必须是导出的纯函数");
    });

    test("A-16 · 契约层与内核：版本随契约 v9 升版，M-08 的 `hit_check` **一字不变**", () => {
        // ⚠️ **原名是「契约层与内核逐字未改」** ⇒ ⇒ 那是 **v0.54 批**的
        //   批次约束（本批零契约变更）⇒ ⇒ 契约 v9 **改了**契约层 ⇒ ⇒ 那句
        //   话已不成立 ⇒ ⇒ 按本仓先例**收窄 + 改名**（断言一条都不删）。
        //
        // ⚠️ **M-08 的 `hit_check` 签名那条断言**（本批**真的**没动它 ⇒ ⇒
        //   它**继续**有效）⇒ ⇒ 逐字保留。
        const contract = readFileSync(resolve(repoRoot, "crates/contract/src/lib.rs"), "utf8");
        assert.ok(
            contract.includes("pub const SCHEMA_VERSION: u32 = 9;"),
            "契约的 `SCHEMA_VERSION` 必须**是 9**（契约 v9：符号查询 + 整段转储）",
        );
        const mirror = readFileSync(resolve(frontDir, "lib/message.js"), "utf8");
        assert.ok(
            mirror.includes("SCHEMA_VERSION = 9"),
            "前端镜像的 `SCHEMA_VERSION` 必须**是 9**（与契约层同批升版）",
        );
        const kernel = readFileSync(
            resolve(repoRoot, "crates/kernel/src/m08_breakpoint/mod.rs"),
            "utf8",
        );
        assert.ok(
            kernel.includes("pub fn hit_check(&self, pc: u32) -> Option<Breakpoint>"),
            "M-08 的 `hit_check` 签名必须**一字不变**",
        );
    });
});
