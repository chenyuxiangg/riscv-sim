// 「寄存器 / 内存」并进**调试视图**那一批的前端门禁（v0.65 批）。
//
// ⚠️ **用户直接提的需求**：「UI 的 regmem 页面内容和 debug 页面的寄存器/内存
//   窗口内容有重复 ⇒ ⇒ 把 regmem 的内容迁移到 debug 页面 ⇒ ⇒ debug 页面的
//   寄存器/内存窗口拆分为**寄存器窗口**和**内存窗口** ⇒ ⇒ 虚拟时间在顶栏
//   有了 ⇒ ⇒ 窗口里的可以删除」。
//
// ⇒ ⇒ 本批**不是**「删一个功能」⇒ ⇒ 是「**把同一件事从两处收到一处**」⇒ ⇒
//   判据必须**双向**：
//
//   ① **旧的那处真的没了**（视图、导航项、落地入口、状态落点、服务层推送）
//   ② **新的那处内容完整**（四样东西一件没少）
//
// ⚠️ **只断言一半 ⇒ ⇒ 会空转**：「按钮整个不见了」时「那里没有重复按钮」那条
//   也会绿 ⇒ ⇒ 每条判据都成对写（那边**没有** ＋ 这边**有**）。
//
// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎只支持
//   简单复合选择器 ⇒ ⇒ 一律用**扁平选择器 + 元素作用域的 `querySelectorAll`**。

import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ **关掉 `app.js` 的自启动**（同 `view-content.test.mjs`）：M-03 的视图状态
// 是**模块级单例** ⇒ ⇒ bootstrap 的异步副作用会落在任意两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const m10 = await importFront("services/m10_registermemory.js");
const { WRITE_SIZE } = await importFront("lib/message.js");

await m02.mountViewComponents();

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

/** 路由到某个视图并渲染一次，返回它的正文容器。 */
function show(viewId) {
    m01.assembleWorkspace("embedded");
    m01.route(`#/${viewId}`);
    const body = dom.document.querySelector(`[data-view-body="${viewId}"]`);
    assert.ok(body, `视图 ${viewId} 的正文容器没挂上`);
    return body;
}

/** 调试视图里的某一张窗口卡（`data-window` 定位 ⇒ **判据抓位置**）。 */
function windowCard(body, name) {
    const card = body.querySelector(`[data-window="${name}"]`);
    assert.ok(card, `调试视图里没有「${name}」窗口`);
    return card;
}

/** 侧边栏的导航项。 */
const sidebarItems = () =>
    [...dom.document.querySelectorAll("[data-view-tab]")]
        .filter((el) => el.closest("[data-sidebar]"));

// ---------------------------------------------------------------------------

describe("v0.65 · 「寄存器 / 内存」那个**独立视图**没了", () => {
    test("M-1 · 视图层**没有**注册那个视图了（不是「隐藏了」，是**没了**）", () => {
        // ⚠️ **先钉住「视图层确实还注册着别的视图」** ⇒ ⇒ 否则「一个视图都
        //   没注册」时这条断言**恒真** ⇒ ⇒ **假绿**。
        assert.ok(m02.get("debug"), "调试视图必须注册着（否则本套件没有对象）");
        assert.ok(m02.get("waveform"), "波形视图必须注册着");
        assert.equal(
            m02.get("regmem"),
            undefined,
            "「寄存器 / 内存」那个独立视图**还在** ⇒ ⇒ 内容又变成两处了",
        );
    });

    test("M-2 · 侧边栏**没有**那个导航项了", () => {
        // ⚠️ **先装配工作台**（侧边栏是装配时渲染的）⇒ ⇒ 否则「一个项都没有」
        //   是「没装配」的形态，不是「撤销了那个视图」的形态 ⇒ ⇒ **假绿**。
        show("debug");
        const items = sidebarItems();
        // ⚠️ **先钉住件数** ⇒ ⇒ 免得「侧边栏一个项都没有」时这个循环空转。
        assert.ok(items.length >= 7, `侧边栏一个导航项都没有（实际 ${items.length}）`);
        const hashes = items.map((el) => el.getAttribute("data-view-tab"));
        assert.ok(
            !hashes.includes("regmem"),
            `侧边栏还有「寄存器 / 内存」那个入口（${hashes.join(",")}）`,
        );
    });

    test("M-3 · 落地视图**没有**指向它的入口了（不是死按钮）", () => {
        const body = show("embedded");
        const targets = [...body.querySelectorAll("[data-embedded-entry]")]
            .map((el) => el.getAttribute("data-embedded-entry"));
        assert.ok(targets.length >= 3, `落地视图的入口一个都没有（实际 ${targets.length}）`);
        assert.ok(
            !targets.includes("regmem"),
            "落地视图还有指向已撤销视图的死入口",
        );
    });

    test("M-4 · 视图状态里**没有**那个视图的孤儿状态", () => {
        m03.reset();
        // ⚠️ **判据钉的是「状态落在哪儿」** ⇒ ⇒ 旧落点必须**整个不存在**
        //   （不是「空着」）⇒ ⇒ 否则会留一份**没人渲染**的孤儿状态。
        assert.equal(
            m03.getState("regmem"),
            undefined,
            "视图状态里还留着「regmem」那一份（孤儿状态 ⇒ ⇒ 没人渲染它）",
        );
    });
});

// ---------------------------------------------------------------------------

describe("v0.65 · 调试视图拆成**寄存器窗口**与**内存窗口**", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("M-5 · 两张窗口卡**都在**（各一个），且**互不串**", () => {
        const body = show("debug");
        // ⚠️ **先钉住「窗口卡存在」** ⇒ ⇒ 否则「一个都没有」时后面全是空转。
        const registers = windowCard(body, "registers");
        const memory = windowCard(body, "memory");
        assert.notEqual(registers, memory, "两个窗口是同一张卡");
        assert.equal(
            body.querySelectorAll('[data-window="registers"]').length,
            1,
            "「寄存器窗口」不止一张卡",
        );
        assert.equal(
            body.querySelectorAll('[data-window="memory"]').length,
            1,
            "「内存窗口」不止一张卡",
        );
    });

    test("M-6 · 寄存器窗口：寄存器格 32 个 + PC + 「读寄存器」按钮", () => {
        const body = show("debug");
        const win = windowCard(body, "registers");
        const grid = win.querySelector(".reg-grid");
        assert.ok(grid, "寄存器窗口里没有 .reg-grid");
        assert.equal(
            grid.querySelectorAll("div").length,
            32,
            "寄存器格必须**仍是 32 个**（稳定钩子 · 判别力不许因为搬家而丢）",
        );
        assert.ok(win.querySelector("code.pc"), "寄存器窗口里没有 PC");
        assert.equal(
            win.querySelectorAll('[data-action="read-regs"]').length,
            1,
            "寄存器窗口里没有「读寄存器」按钮（或不止一个）",
        );
    });

    test("M-7 · 内存窗口：读内存入口 + 读这一段入口 + 内存单元表", () => {
        const body = show("debug");
        const win = windowCard(body, "memory");

        // -- 读内存（一个字 · 走总线）----------------------------------
        const box = win.querySelector('[data-addr-input="memory"]');
        assert.ok(box, "内存窗口里没有读内存的入口");
        assert.ok(box.querySelector("[data-mem-addr-input]"), "必须有地址输入框");
        const select = box.querySelector("[data-mem-size]");
        assert.ok(select, "必须有宽度选择");
        assert.deepEqual(
            [...select.querySelectorAll("option")].map((o) => o.getAttribute("value")),
            [WRITE_SIZE.Byte, WRITE_SIZE.Half, WRITE_SIZE.Word],
            "宽度三档必须**逐字**是契约的 `WRITE_SIZE`",
        );
        assert.ok(
            box.querySelector('[data-action="read-mem"]'),
            "必须有「读内存」按钮",
        );

        // -- 读这一段（指定字节数 · 调试视角）---------------------------
        // ⚠️ **这一条最关键**：「读这一段」此前**只在**「寄存器 / 内存」那个
        //   视图里 ⇒ ⇒ 迁移之后调试视图**必须**有它 ⇒ ⇒ 否则「迁移」变成了
        //   「删除一个用户看得见的能力」⇒ ⇒ 那是最坏的一种改法。
        const dump = win.querySelector('[data-dump-input="memory"]');
        assert.ok(dump, "内存窗口里没有「读这一段」的入口");
        assert.ok(dump.querySelector("[data-dump-addr-input]"), "必须有起始地址输入框");
        assert.ok(dump.querySelector("[data-dump-len-input]"), "必须有字节数输入框");
        assert.ok(
            dump.querySelector('[data-action="dump-memory"]'),
            "必须有「读这一段」按钮",
        );
    });

    test("M-8 · 两个窗口**各归其位**（寄存器格不在内存窗口、读内存不在寄存器窗口）", () => {
        const body = show("debug");
        const registers = windowCard(body, "registers");
        const memory = windowCard(body, "memory");
        // ⚠️ **双向断言** ⇒ ⇒ 只查一边的话，「整个搬错了窗口」也会绿。
        assert.equal(
            registers.querySelectorAll(".reg-grid").length,
            1,
            "寄存器窗口里没有寄存器格",
        );
        assert.equal(
            memory.querySelectorAll(".reg-grid").length,
            0,
            "寄存器格跑进内存窗口里去了",
        );
        assert.equal(
            registers.querySelectorAll('[data-action="read-mem"]').length,
            0,
            "「读内存」跑进寄存器窗口里去了",
        );
        assert.equal(
            registers.querySelectorAll('[data-action="dump-memory"]').length,
            0,
            "「读这一段」跑进寄存器窗口里去了",
        );
        assert.equal(
            memory.querySelectorAll('[data-action="read-regs"]').length,
            0,
            "「读寄存器」跑进内存窗口里去了",
        );
    });

    test("M-9 · 调试视图里**没有**虚拟时间了（顶栏有 ⇒ ⇒ 同一个量只显示一处）", () => {
        const body = show("debug");
        // ⚠️ **反向判据**：这条钉的是「**这里没有**」⇒ ⇒ 顶栏那份是**唯一**那
        //   一份 ⇒ ⇒ 「虚拟时间只显示一处」这条规矩在界面上成立。
        assert.equal(
            body.querySelectorAll(".timeVirt").length,
            0,
            "调试视图里还在显示虚拟时间（顶栏有 ⇒ ⇒ 同一个量显示两处）",
        );
        assert.ok(
            !String(body.innerHTML).includes("timeVirt"),
            "调试视图里还有 timeVirt 的残留（活值隐藏那条判据依赖它）",
        );
    });
});

// ---------------------------------------------------------------------------

describe("v0.65 · 状态落点与推送范围", () => {
    beforeEach(() => {
        m03.reset();
    });

    test("M-10 · 转储结果与错误**落在 `debug` 视图状态**里", async () => {
        const handlers = new Map();
        const router = {
            subscribe: (topic, handler) => {
                handlers.set(topic, handler);
                return () => handlers.delete(topic);
            },
            dispatchCommand: async () => ({ value: null }),
        };
        app.wireDebug(router);
        assert.ok(handlers.has("debug.dump"), "app 层没有订阅 debug.dump 主题");

        handlers.get("debug.dump")({ value: { addr: 0x1000, bytes: [1, 2, 3] } });
        const dump = m03.getState("debug")?.debug?.dump;
        assert.ok(dump, "转储结果没有落进 debug.dump");
        assert.equal(Number(dump.addr), 0x1000);
        assert.deepEqual(dump.bytes, [1, 2, 3]);
        // ⚠️ **旧落点必须空** ⇒ ⇒ 没有第二份 ⇒ ⇒ 界面上显示的就是这一份。
        assert.equal(
            m03.getState("regmem"),
            undefined,
            "转储结果还在往旧的视图状态写（⇒ ⇒ 有第二份）",
        );
    });

    test("M-11 · M-10 的**默认** `viewIds` 里没有那个视图（服务层也不往那儿推）", () => {
        const src = readFileSync(resolve(frontDir, "services/m10_registermemory.js"), "utf8");
        const m = /const viewIds = options\.viewIds \?\? (\[[^\]]*\])/.exec(src);
        assert.ok(m, "没找到 M-10 的默认 viewIds");
        // ⚠️ **这里刻意不写含单引号的正则** —— `tools/front_test_attrib.py` 的
        //   掩码（`strip_code`）**不认识正则字面量** ⇒ ⇒ 一个带 `'` 的正则会被
        //   当成字符串开头 ⇒ ⇒ **后面若干条用例整段被抹掉** ⇒ ⇒ 那个文件的
        //   「测试」列**被少算** ⇒ ⇒ 源文件里写的是双引号 ⇒ ⇒ 直接 `JSON.parse`
        //   就行（台账 **D-174**）。
        assert.deepEqual(
            JSON.parse(m[1]),
            ["debug"],
            "M-10 的默认 viewIds 应当只剩 debug（⇒ ⇒ 快照只往一处推）",
        );
    });

    test("M-12 · 生产用的那份列表**没有**那个视图（唯一真相在 app 层）", () => {
        assert.deepEqual(
            [...app.REGISTER_MEMORY_VIEW_IDS],
            ["debug", "source", "embedded"],
            "生产用的 viewIds 应当是这三个（⇒ ⇒ 视图数与用途一一对应）",
        );
    });
});
