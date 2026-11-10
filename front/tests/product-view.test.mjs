// 产品视图（双栏工作面）的门禁 —— 本轮重构的验收：
//
//   1. **默认空树**：没装载产品 ⇒ 左栏只有「创建 / 从模版加载」，右栏居中
//      「内存展示区」，**不**把内核里那块 default 板当成用户的产品；
//   2. **点器件才出窗口**：右栏要显示该器件的基址 / 各偏移与含义（核只显示
//      CSR，没有基址）；
//   3. **lock 只门控编辑**：锁定时字段只读；解锁后可写，写完只是**草稿**；
//   4. **生效与 lock 无关**：锁定状态下点「重新载入」照样发命令（送板 +
//      再装载工程）；改完不点就不生效；
//
// ⚠️ 断言都走 **data-* 钩子**（文案会变，钩子不该变）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const dom = installDom(readFileSync(resolve(frontDir, "index.html"), "utf8"));

// ⚠️ 关掉自启动（与其它 app 层用例同一条理由：视图状态是模块级单例）。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const pw = await importFront("views/components/product-workspace.js");
const docs = await importFront("views/components/device-docs.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

/** 一份最小板（RAM + UART + GPIO）—— 与内置模版同形，但字面量写死。 */
const BOARD = {
    schema_version: 1,
    id: "user-board",
    name: "我的板",
    priority: 100,
    auto_selectable: false,
    region_budget_bytes: 0x01000000,
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
            device: { device: "gpio", id: "GPIO0" },
        },
    ],
};

/** 装一块「已装载」的板（左树 + 右窗都有内容）。 */
function setupLoaded(extra = {}) {
    m03.reset();
    m01.assembleWorkspace("product");
    m03.setState("product", {
        loaded: true,
        locked: true,
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

/** 点某个节点（走真实的事件委托）。 */
function clickNode(key) {
    const node = $(`[data-device-key='${key}']`);
    assert.ok(node, `左树里没有节点 ${key}`);
    node.dispatchEvent(new Event("click"));
}

/** 在某输入框里敲一个值（input 事件 = 逐键写状态，不重渲染）。 */
function typeInto(el, value) {
    el.value = value;
    el.dispatchEvent(new Event("input"));
}

describe("产品视图 · 生效内存映射不再占左栏", () => {
    test("M-14 折叠表已删除，只留左树与右器件窗口", () => {
        setupLoaded();
        assert.equal($(".pw-m14"), null, "左栏不该再有 M-14 折叠表");
        assert.ok(
            !dom.document.body.textContent.includes("当前生效内存映射"),
            "页面不该再出现「当前生效内存映射」",
        );
    });
});

describe("产品视图 · 默认空态", () => {
    test("没装载产品 ⇒ 左栏只有两个入口，右栏居中「内存展示区」", () => {
        setupEmpty();
        const html = $('[data-view-body="product"]').innerHTML;
        assert.ok(html.includes("尚未加载任何板子"), "空态没有说「尚未加载任何板子」");
        assert.ok(html.includes("内存展示区"), "右栏没有「内存展示区」");
        assert.ok($("[data-action='open-create']"), "缺少「创建」入口");
        assert.ok($("[data-action='open-templates']"), "缺少「从模版加载」入口");
        assert.equal($$("[data-device-key]").length, 0, "空产品不许渲染器件");
        assert.ok($("[data-action='reload-product']").hasAttribute("disabled"), "空产品不该能「重新载入」");
    });

    test("内核推了 default 板的 `memory.map` ⇒ **不**把它当成用户的产品", () => {
        setupEmpty();
        // 内核还装着 default 板 ⇒ 事件照收，但产品视图仍是空树。
        m03.setState("product", {
            memoryMap: { description: { ...BOARD, id: "default", name: "Default" }, revision: 0, pending: false, loaded: true },
        });
        m01.route("#/product");
        assert.equal($$("[data-device-key]").length, 0, "「有图但没装载」不许冒出器件树");
        assert.ok($('[data-view-body="product"]').innerHTML.includes("尚未加载任何板子"));
    });
});

describe("产品视图 · 点器件才出窗口", () => {
    test("默认不选 ⇒ 右栏还是「内存展示区」（不是第一个器件）", () => {
        setupLoaded();
        assert.ok($(".pw-window-empty"), "没点器件时右栏不该有详情");
        assert.ok($('[data-view-body="product"]').innerHTML.includes("内存展示区"));
    });

    test("点 RAM 段 ⇒ 显示内存基址 + 段长度", () => {
        setupLoaded();
        clickNode("r0");
        const base = $("[data-base-input]");
        assert.ok(base, "RAM 段没有内存基址");
        assert.equal(base.value, "0x80000000");
        const size = $("[data-size-input]");
        assert.ok(size, "RAM 段没有段长度");
        assert.equal(size.value, "0x00100000");
    });

    test("点外设 ⇒ 显示基址 + 偏移表（含偏移含义）", () => {
        setupLoaded();
        clickNode("r1");
        assert.equal($("[data-base-input]").value, "0x40000000");
        const rows = $$("[data-reg-index]");
        assert.ok(rows.length > 0, "外设没有偏移表");
        const meanings = $$("[data-reg-field='meaning']").map((el) => el.value);
        assert.ok(
            meanings.some((text) => text.includes("接收") || text.includes("发送")),
            `偏移含义不是寄存器说明表里的内容：${JSON.stringify(meanings)}`,
        );
    });

    test("点核 ⇒ **没有**内存基址，只有 CSR 表", () => {
        setupLoaded();
        clickNode("core");
        assert.equal($("[data-base-input]"), null, "核不该有内存基址");
        const first = $("[data-reg-field='name']");
        assert.ok(first, "核没有 CSR 表");
        assert.equal(first.value, "mstatus", "CSR 表头一项应当是 mstatus");
        const header = $(".pw-regs-head h4").textContent;
        assert.ok(header.includes("CSR"), `核的表头必须是 CSR：${header}`);
    });
});

describe("产品视图 · lock 只门控编辑", () => {
    test("锁定时字段只读；解锁后可编辑", () => {
        setupLoaded();
        clickNode("r1");
        assert.ok($("[data-base-input]").hasAttribute("disabled"), "锁定时基址必须只读");
        assert.ok($("[data-reg-field='meaning']").hasAttribute("disabled"), "锁定时偏移含义必须只读");

        $("[data-action='toggle-lock']").dispatchEvent(new Event("click"));
        assert.equal(m03.getState("product").locked, false, "点「解锁」后状态没翻转");
        assert.equal($("[data-base-input]").hasAttribute("disabled"), false, "解锁后基址仍只读");
        assert.equal($("[data-reg-field='meaning']").hasAttribute("disabled"), false, "解锁后偏移含义仍只读");
    });

    test("改基址 ⇒ 只写草稿（`state.board`），生效描述不动", () => {
        setupLoaded({ locked: false });
        clickNode("r1");
        typeInto($("[data-base-input]"), "0x40002000");

        const state = m03.getState("product");
        assert.equal(state.board.description.regions[1].base, 0x40002000, "草稿没记下新基址");
        assert.equal(pw.effectiveDescription(state), null, "生效描述不该被草稿改掉");
        // 左侧与右窗都要有「未生效」标记（判定靠钩子，不靠文案）。
        assert.ok($(".pw-dirty-flag"), "右窗没有「有未生效的改动」标记");
        assert.ok($(".pw-dirty-count"), "左栏没有未生效计数");
    });

    test("改偏移含义 ⇒ 写进 `docs` 草稿 ⇒ 标记为未生效", () => {
        setupLoaded({ locked: false });
        clickNode("r1");
        const meaning = $("[data-reg-field='meaning']");
        typeInto(meaning, "改过的含义");
        const state = m03.getState("product");
        assert.equal(state.docs.r1.regs[0].meaning, "改过的含义", "含义没写进 docs 草稿");
        assert.ok($(".pw-dirty-flag"), "改了含义但没有「未生效」标记");
    });

    test("「放弃修改」⇒ 草稿与 docs 一起回到基线", () => {
        setupLoaded({ locked: false });
        clickNode("r1");
        typeInto($("[data-base-input]"), "0x40002000");
        $("[data-action='discard-product']").dispatchEvent(new Event("click"));
        const state = m03.getState("product");
        assert.equal(state.board, null, "放弃修改没有清掉草稿");
        assert.deepEqual(state.docs, {});
    });
});

describe("产品视图 · 生效必须「重新载入」（与 lock 无关）", () => {
    test("**锁定**状态下点「重新载入」⇒ 照样发出命令、并带上草稿", () => {
        setupLoaded({ locked: true });
        const draft = JSON.parse(JSON.stringify(BOARD));
        draft.regions[1].base = 0x40002000;
        m03.setState("product", { board: { description: draft } });

        let detail = null;
        const listener = (ev) => { if (ev.detail?.action === "reload-product") detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        clickNode("r1");
        // 右窗底栏那个「重新载入」（锁着也必须是可点的）。
        const buttons = $$("[data-action='reload-product']");
        assert.ok(buttons.length >= 1);
        buttons[buttons.length - 1].dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        assert.ok(detail, "锁定时点「重新载入」没有发命令（生效不该被 lock 挡住）");
        assert.equal(detail.params.description.regions[1].base, 0x40002000, "命令里带的不是草稿");
    });

    test("app 层：`reload-product` ⇒ 先送板再 `LoadProject`，之后草稿成为基线", async () => {
        setupEmpty();
        const sent = [];
        const router = {
            async dispatchCommand(payload) { sent.push(payload); return { value: {} }; },
            subscribe() { return () => {}; },
        };
        const services = {
            commandRouter: router,
            registerMemory: { async loadProject(project) { sent.push({ __project: project }); } },
            waveform: { reset() {}, async subscribeProbeStream() {} },
            console: { reset() {}, async subscribeConsoleStream() {} },
            debug: { reset() {} },
            memoryMap: null,
            boardStore: null,
        };
        const draft = JSON.parse(JSON.stringify(BOARD));
        draft.regions[0].size = 0x00200000;
        m03.setState("product", { board: { description: draft } });

        await app.handleCommand(
            { action: "reload-product", params: { description: pw.cloneDescription(draft) } },
            services,
        );

        const set = sent.find((c) => c && "SetMemoryMap" in c);
        assert.ok(set, "没有送板（`SetMemoryMap`）");
        assert.equal(set.SetMemoryMap.description.regions[0].size, 0x00200000, "送出去的不是改过的那份");
        const project = sent.find((c) => c && c.__project);
        assert.ok(project, "送板之后没有 `LoadProject`（板级配置要下一次装载才生效）");
        assert.equal(project.__project.tree.root.children[0].children.length, 2, "工程树没带上外设");

        const state = m03.getState("product");
        assert.equal(state.loaded, true, "成功之后必须标成已装载");
        assert.ok($("[data-device-key]"), "成功之后左树应当有器件");
    });

    test("什么都没改 ⇒ **一条命令都不发**（不白涨修订号）", async () => {
        setupLoaded();
        const sent = [];
        const router = {
            async dispatchCommand(payload) { sent.push(payload); return { value: {} }; },
            subscribe() { return () => {}; },
        };
        const services = {
            commandRouter: router,
            registerMemory: { async loadProject(project) { sent.push({ __project: project }); } },
            waveform: { reset() {}, async subscribeProbeStream() {} },
            console: { reset() {}, async subscribeConsoleStream() {} },
            debug: { reset() {} },
            memoryMap: null,
        };
        m03.setState("product", {
            memoryMap: { description: JSON.parse(JSON.stringify(BOARD)), revision: 3, pending: false, loaded: true },
        });
        await app.handleCommand(
            { action: "reload-product", params: { description: JSON.parse(JSON.stringify(BOARD)) } },
            services,
        );
        assert.equal(sent.find((c) => c && "SetMemoryMap" in c), undefined, "没改却发了整份替换");
        assert.equal(sent.find((c) => c && "PatchMemoryMap" in c), undefined, "没改却发了差量");
    });
});

describe("产品视图 · 重新载入后不许自相矛盾（真机抓到的回归）", () => {
    test("内核把段重排后推回来 ⇒ 基线用**内核那份**，不再是「N 处未生效」", () => {
        // 前置：用户改过 UART 基址并点过「重新载入」（状态里已标 justApplied）。
        const draft = JSON.parse(JSON.stringify(BOARD));
        draft.regions[1].base = 0x40002000;
        m03.reset();
        m01.assembleWorkspace("product");
        m03.setState("product", {
            loaded: true,
            locked: false,
            selected: "r1",
            board: { description: draft },
            docs: { r1: { unit: "B", regs: [{ offset: "0x00", name: "THR", meaning: "改过的含义", width: 1 }] } },
            docsBase: { r1: { unit: "B", regs: [{ offset: "0x00", name: "THR", meaning: "接收缓冲 / 发送保持", width: 1 }] } },
            tree: pw.projectTreeFrom(draft),
            justApplied: true,
            // ⚠️ 内核推回来的那份：段**按 base 排序**（GPIO 在前、UART 在后）。
            memoryMap: {
                revision: 3,
                pending: false,
                loaded: true,
                description: {
                    ...JSON.parse(JSON.stringify(BOARD)),
                    regions: [
                        { kind: "ram", base: 0x80000000, size: 0x00100000 },
                        { kind: "peripheral", base: 0x40001000, device: { device: "gpio", id: "GPIO0" } },
                        { kind: "peripheral", base: 0x40002000, device: { device: "uart16550", id: "UART0", irq_line: 10, baud_rate: 115200 } },
                    ],
                },
            },
        });
        m01.route("#/product");
        // 扶正之前：基址要按**器件身份**对齐（UART 在下标 1、生效图里在 2）
        // ⇒ 「基址没改」必须判成没改（只有含义草稿是真的没生效）。
        const uartNode = docs.buildDeviceNodes(m03.getState("product")).find((n) => n.key === "r1");
        assert.equal(pw.nodeDirty(m03.getState("product"), uartNode).base, false, "基址按身份对齐后不该报脏");

        const synced = m02.syncProductBaseline();
        assert.equal(synced, true, "内核那份已经落位 ⇒ 必须扶正基线");
        const state = m03.getState("product");
        assert.equal(state.justApplied, false, "扶正后要清掉「刚提交」标志");
        assert.equal(state.board.description.regions[1].device.id, "GPIO0", "基线没有换成内核重排后的那份");
        assert.deepEqual(pw.productDirty(state), { count: 0, dirty: false, keys: [] });
        // 选中项按**身份**搬到新下标：UART 现在是 r2。
        assert.equal(state.selected, "r2", "选中项没跟着器件走（还停在下标上）");
        m01.route("#/product");
        assert.equal($("[data-base-input]").value, "0x40002000", "扶正后右窗显示的不是 UART 的基址");
        assert.equal($(".pw-dirty-flag"), null, "扶正后右窗还挂着「未生效」");
        // ⚠️ 寄存器草稿也要跟着器件搬家（否则「改了含义一重载就没了」）。
        assert.ok(state.docs.r2, "docs 还挂在旧下标 r1 上");
        assert.equal(state.docs.r1, undefined, "旧下标上的草稿没有搬走");
        assert.equal(state.docs.r2.regs[0].meaning, "改过的含义", "搬过去的草稿内容不对");
        assert.equal(state.docsBase.r2.regs[0].meaning, "改过的含义", "docsBase 没跟上");
    });
});

describe("产品视图 · 板档案落在数据库里（偏移 / 含义随档持久化）", () => {
    /** 一份「已装载 + 改过 UART 含义」的现场（草稿与基线不一致）。 */
    function setupEdited() {
        setupLoaded({
            docs: { r1: { unit: "B", regs: [{ offset: "0x00", name: "THR", meaning: "改过的含义", width: 1 }] } },
            docsBase: { r1: { unit: "B", regs: [{ offset: "0x00", name: "THR", meaning: "接收缓冲 / 发送保持", width: 1 }] } },
        });
    }

    /** 假档案库（`put` 记账、`list` 回读 ⇒ 与真 store 同一形状）。 */
    function fakeStore() {
        const rows = [];
        return {
            rows,
            supported: true,
            async list() { return JSON.parse(JSON.stringify(rows)); },
            async put(entry) { rows.push(JSON.parse(JSON.stringify(entry))); },
            async remove() {},
        };
    }

    /** app 层要的那套服务桩（只到「装载完成」为止）。 */
    function servicesWith(store) {
        const sent = [];
        const router = {
            async dispatchCommand(payload) { sent.push(payload); return { value: {} }; },
            subscribe() { return () => {}; },
        };
        return {
            sent,
            commandRouter: router,
            registerMemory: { async loadProject(project) { sent.push({ __project: project }); } },
            waveform: { reset() {}, async subscribeProbeStream() {} },
            console: { reset() {}, async subscribeConsoleStream() {} },
            debug: { reset() {} },
            memoryMap: null,
            boardStore: store,
        };
    }

    test("寄存器文档按**器件身份**索引：段被重排也不串位（`docsByDevice` → `docsForKeyed` 往返）", () => {
        setupEdited();
        const loaded = m03.getState("product");
        const byDevice = docs.docsByDevice(loaded);
        assert.ok(byDevice["device:UART0"], "草稿没有按器件 id 归档");
        assert.equal(byDevice["device:UART0"].regs[0].meaning, "改过的含义");

        // 内核重排后的描述（GPIO 在前、UART 在后）⇒ 身份不变、下标变了
        const reordered = {
            ...JSON.parse(JSON.stringify(BOARD)),
            regions: [BOARD.regions[0], BOARD.regions[2], BOARD.regions[1]],
        };
        const keyed = docs.docsForKeyed(byDevice, reordered, pw.projectTreeFrom(reordered));
        assert.deepEqual(Object.keys(keyed), ["r2"], "重排后 UART 应当落在 r2");
        assert.equal(keyed.r2.regs[0].meaning, "改过的含义", "搬过去的内容不对");
    });

    test("视图层：点「另存为档案」⇒ 弹层出现，确认后发 `save-board-as`（带 id / 名称）", () => {
        setupLoaded();
        m01.route("#/product");
        const saveBtn = $('[data-action="save-archive"]');
        assert.ok(saveBtn, "左栏没有「另存为档案」入口");
        assert.equal(saveBtn.hasAttribute("disabled") && saveBtn.getAttribute("disabled") !== "", false, "已装载时这个入口不该禁用");

        saveBtn.dispatchEvent(new Event("click"));
        assert.ok($('[data-dialog="archive"]'), "点了按钮没有弹层");
        const idEl = $("[data-archive-id]");
        assert.equal(idEl.value, "user-board-copy", "默认 id 没按当前板派生");

        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        idEl.value = "board-copy";
        $("[data-archive-name]").value = "副本板";
        $('[data-action="confirm-save-as"]').dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);

        assert.ok(detail, "确认之后没有发命令");
        assert.equal(detail.action, "save-board-as");
        assert.equal(detail.params.id, "board-copy");
        assert.equal(detail.params.name, "副本板");
    });

    test("app 层：重新载入成功 ⇒ **顺手写进本机数据库**，条目带偏移 / 含义", async () => {
        setupEdited();
        const store = fakeStore();
        const services = servicesWith(store);
        const draft = JSON.parse(JSON.stringify(BOARD));
        draft.regions[1].base = 0x40002000;
        m03.setState("product", { board: { description: draft } });

        await app.handleCommand(
            { action: "reload-product", params: { description: pw.cloneDescription(draft) } },
            services,
        );

        assert.equal(store.rows.length, 1, "重新载入之后没有落库");
        const entry = store.rows[0];
        assert.equal(entry.id, "user-board", "落库的 id 不对");
        assert.deepEqual(
            Object.keys(entry).sort(),
            ["description", "id", "registers", "rules_version"],
            "条目字段集合不对",
        );
        assert.deepEqual(
            entry.description.regions.map((r) => r.base),
            [0x80000000, 0x40002000, 0x40001000],
            "落库的描述不是刚提交的那份",
        );
        assert.equal(
            entry.registers["device:UART0"].regs[0].meaning,
            "改过的含义",
            "偏移 / 含义没跟着落库",
        );
        // 列表刷新过 ⇒ 下次打开「从模版加载」能看见它
        assert.deepEqual(m03.getState("product").archive.map((row) => row.id), ["user-board"]);
    });

    test("app 层：`save-board-as` 写一条**副本**（当前产品 id 不变），id 占用则拒绝", async () => {
        setupLoaded();
        const store = fakeStore();
        store.rows.push({ id: "taken", description: JSON.parse(JSON.stringify(BOARD)), rules_version: 1 });
        m03.setState("product", { archive: JSON.parse(JSON.stringify(store.rows)) });
        const services = servicesWith(store);

        await app.handleCommand(
            { action: "save-board-as", params: { id: "taken", name: "想覆盖别人的板" } },
            services,
        );
        assert.equal(store.rows.length, 1, "id 已被占用却照样写了（会静默盖掉别人的板）");

        await app.handleCommand(
            { action: "save-board-as", params: { id: "board-copy", name: "副本板" } },
            services,
        );
        assert.equal(store.rows.length, 2, "另存为没有落库");
        const copy = store.rows[1];
        assert.equal(copy.id, "board-copy");
        assert.equal(copy.description.name, "副本板", "改了名却没落到描述里");
        assert.equal(copy.description.regions.length, BOARD.regions.length, "副本不是当前板");
        assert.equal(
            m03.getState("product").board.description.id,
            "user-board",
            "另存为把当前产品的 id 也改掉了",
        );
    });

    test("从模版加载档案 ⇒ 把档案里的寄存器文档搬回视图状态", async () => {
        setupEmpty();
        const store = fakeStore();
        const services = servicesWith(store);
        const registers = {
            "device:UART0": { unit: "B", regs: [{ offset: "0x00", name: "THR", meaning: "档案里的含义", width: 1 }] },
        };
        await app.handleCommand(
            {
                action: "load-archive",
                params: { description: JSON.parse(JSON.stringify(BOARD)), registers },
            },
            services,
        );
        const state = m03.getState("product");
        assert.equal(state.loaded, true);
        assert.deepEqual(state.docs, { r1: registers["device:UART0"] }, "档案文档没有搬回视图状态");
        assert.deepEqual(state.docsBase, state.docs, "基线没跟上（会立刻显示「未生效」）");
    });
});

describe("产品视图 · 器件参数与基址空隙进右栏", () => {
    test("外设右栏显示 PLIC 源号；改它只写草稿", () => {
        setupLoaded({ locked: false });
        clickNode("r1");
        const irq = $("[data-device-param='irq_line']");
        assert.ok(irq, "外设右栏没有 PLIC 源号输入框");
        assert.equal(irq.value, "10");
        const label = irq.closest(".pw-field").querySelector("span");
        assert.equal(label.textContent, "PLIC 源号", "中断号在 UI 上必须叫 PLIC 源号");

        typeInto(irq, "12");
        const state = m03.getState("product");
        assert.equal(state.board.description.regions[1].device.irq_line, "12", "源号没写进草稿");
        assert.ok($(".pw-dirty-flag"), "改了源号但没有「未生效」标记");
    });

    test("RAM 右栏有地址空隙；点「用这个」把基址填过去", () => {
        setupLoaded({ locked: false });
        clickNode("r0");
        assert.ok($("[data-product-gaps]"), "RAM 右栏没有地址空隙块");
        const buttons = $$("[data-action='use-product-gap']");
        assert.ok(buttons.length > 0, "没有可用的空隙");
        assert.equal(buttons[0].hasAttribute("disabled"), false, "解锁后「用这个」不该禁用");

        buttons[0].dispatchEvent(new Event("click"));
        const state = m03.getState("product");
        assert.equal(
            state.board.description.regions[0].base,
            Number(buttons[0].getAttribute("data-gap-from")),
            "点了「用这个」基址没填进草稿",
        );
    });
});

describe("产品视图 · 添加器件", () => {
    test("点「添加器件」⇒ 弹层列出 RAM / ROM；添加后树里多一个器件节点", () => {
        setupLoaded({ locked: false });
        const open = $("[data-action='open-add-device']");
        assert.ok(open, "产品树头部没有「添加器件」入口");
        open.dispatchEvent(new Event("click"));
        assert.ok($('[data-dialog="add-device"]'), "没有添加器件弹层");
        const rom = $("[data-action='add-device'][data-add-kind='rom']");
        assert.ok(rom, "弹层里没有 ROM");
        const before = m03.getState("product").board.description.regions.length;
        rom.dispatchEvent(new Event("click"));
        const after = m03.getState("product").board.description;
        assert.equal(after.regions.length, before + 1, "添加 ROM 后器件数没加一");
        assert.equal(after.regions.at(-1).kind, "rom", "新加的器件不是 ROM");
        assert.equal(
            Number(after.regions.at(-1).base) % 4096,
            0,
            `新器件的基址没有按 4 KiB 对齐：${after.regions.at(-1).base}`,
        );
        assert.equal(m03.getState("product").dialog, null, "添加成功后弹层没关");
    });
    test("添加外设（GPIO）⇒ 多一个 peripheral 段，器件类型是 gpio", () => {
        setupLoaded({ locked: false });
        $("[data-action='open-add-device']").dispatchEvent(new Event("click"));
        $("[data-action='add-device'][data-add-kind='gpio']").dispatchEvent(new Event("click"));
        const regions = m03.getState("product").board.description.regions;
        const added = regions.at(-1);
        assert.equal(added.kind, "peripheral", "外设器件没有落成 peripheral 段");
        assert.equal(added.device.device, "gpio", "器件类型没有落成 gpio");
        assert.match(String(added.device.id), /^GPIO/, "器件 id 没有走 GPIO 前缀");
        assert.equal(regions.filter((r) => r.device?.id === added.device.id).length, 1, "器件 id 重复");
    });
});

describe("产品视图 · 档案管理子窗", () => {
    const ARCHIVED = {
        id: "archived-board",
        description: { ...JSON.parse(JSON.stringify(BOARD)), id: "archived-board", name: "档案板" },
        rules_version: 1,
        registers: {},
    };

    test("点「档案管理」⇒ 弹层列出档案，可载入", () => {
        setupLoaded({ archive: [ARCHIVED] });
        const btn = $("[data-action='open-archives']");
        assert.ok(btn, "左栏没有「档案管理」入口");
        btn.dispatchEvent(new Event("click"));
        assert.ok($('[data-dialog="archives"]'), "点了按钮没有档案管理弹层");
        const load = $("[data-action='use-template'][data-template-src='archive']");
        assert.ok(load, "档案行没有「载入」");
        assert.equal(load.getAttribute("data-template-id"), "archived-board");
    });

    test("点「删除」⇒ 发出 `delete-product-archive`（带 id）", () => {
        setupLoaded({ archive: [ARCHIVED] });
        m01.route("#/product");
        $("[data-action='open-archives']").dispatchEvent(new Event("click"));
        let detail = null;
        const listener = (ev) => { detail = ev.detail; };
        dom.window.addEventListener("riscv:command", listener);
        $("[data-action='delete-product-archive']").dispatchEvent(new Event("click"));
        dom.window.removeEventListener("riscv:command", listener);
        assert.ok(detail, "删除档案没有发命令");
        assert.equal(detail.action, "delete-product-archive");
        assert.equal(detail.params.id, "archived-board");
    });

    test("app 层：删除档案 ⇒ 只动本机档案，当前产品不变", async () => {
        setupLoaded({ archive: [ARCHIVED] });
        const rows = [JSON.parse(JSON.stringify(ARCHIVED))];
        const store = {
            supported: true,
            async list() { return rows; },
            async put(entry) { rows.push(entry); },
            async remove(id) {
                const at = rows.findIndex((row) => row.id === id);
                if (at >= 0) rows.splice(at, 1);
            },
        };
        await app.handleCommand(
            { action: "delete-product-archive", params: { id: "archived-board" } },
            { boardStore: store },
        );
        const state = m03.getState("product");
        assert.deepEqual(state.archive, [], "档案列表没刷新");
        assert.equal(state.board.description.id, "user-board", "删除档案把当前产品也改掉了");
        assert.equal(rows.length, 0, "持久化层没删掉");
    });
});
