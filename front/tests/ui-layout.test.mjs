// UI 布局门禁（STAGE-34 · v0.49 批 · 侧边栏 + 顶栏运行条）。
//
// ⚠️ **分层**：这一套验**结构与接线**（Node 侧）；**真交互**（真点侧边栏、
// 真点顶栏按钮、真窄屏）由 `tools/browser-verify.mjs` 的 `uiLayout` 段
// （B-12 ~ B-18）负责 ⇒ ⇒ 两边合起来才是完整的判据。
//
// ⚠️ **本批的核心教训**（STAGE-34 §8 订正 4）：改 DOM 结构 ⇒ ⇒ 判据的
// 取法可能**悄悄**失效 ⇒ ⇒ 所以这里与真机门禁都**按位置取值**
// （`data-view-tab` / `data-sidebar` / `data-topbar-action`）。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = readFileSync(resolve(frontDir, "index.html"), "utf8");
const styleSrc = readFileSync(resolve(frontDir, "style.css"), "utf8");
const appSrc = readFileSync(resolve(frontDir, "app.js"), "utf8");
const runStateSrc = readFileSync(resolve(frontDir, "views/app/run-state.js"), "utf8");
// 顶栏运行条按钮接线已迁入 run-state.js ⇒ ⇒ 按钮 action 指纹在门面 + 该子模块
// 合并文本里找；`handleCommand` 的 `case` 分支仍在门面。
const appAndRunStateSrc = `${appSrc}\n${runStateSrc}`;
const m01Src = readFileSync(resolve(frontDir, "views/m01_workbench.js"), "utf8");

// ⚠️ **DOM 全局必须先于前端模块存在**（`app.js` 在模块作用域读
// `document.getElementById("status")`）⇒ ⇒ 同 `frontend.test.mjs`。
const dom = installDom(indexHtml);

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");

const tick = () => new Promise((r) => setTimeout(r, 0));

/** 等应用**挂载完成**（同 `frontend.test.mjs` 的做法）。 */
const waitForMount = async (maxTicks = 64) => {
    for (let i = 0; i < maxTicks; i += 1) {
        const status = dom.document.getElementById("status");
        const text = String(status?.textContent ?? "");
        if (text && !text.includes("正在挂载")) return true;
        await tick();
    }
    return false;
};

const $ = (selector) => dom.document.querySelector(selector);
const $$ = (selector) => dom.document.querySelectorAll(selector);

// ⚠️ **不许用后代组合器**（`[a] [b]`）⇒ ⇒ `dom-stub` 的选择器引擎**只支持
// 简单复合选择器**（`tag` / `#id` / `.class` / `[attr]` / `[attr="v"]`）⇒ ⇒
// 组合器会**静默返回 0 个** ⇒ ⇒ 用例会以「侧边栏 0 项」的形态**假红**。
// ⇒ ⇒ 改成**扁平选择器 + `closest()` 过滤**（`closest()` 是 stub 支持的）。
const sidebarItems = () =>
    [...$$("[data-view-tab]")].filter((el) => el.closest("[data-sidebar]"));

await waitForMount();

// ---------------------------------------------------------------------------

describe("STAGE-34 · 左侧分组导航（侧边栏）", () => {
    test("A-1 · 每个已挂载的视图**都有分组**（6 个一个不缺）", () => {
        const ws = m01.currentWorkspace();
        // ⚠️ **v0.65 批**：「寄存器 / 内存」那个独立视图撤销了 ⇒ ⇒ 侧边栏
        //   从 **8 项**变 **6 项**（板级内存图并入产品视图） ⇒ ⇒ 不是「少一个功能」，是「少一个重复入口」。
        assert.ok(ws.views.length >= 6, `应当至少挂载 6 个视图，实际 ${ws.views.length}`);
        for (const v of ws.views) {
            assert.ok(
                v.group === "run" || v.group === "platform",
                `视图 ${v.hash} 的分组是 ${JSON.stringify(v.group)}（应当是 run / platform 之一）`,
            );
        }
    });

    test("A-2 · 分组计数正确：运行与调试 4 项 / 平台 2 项", () => {
        const ws = m01.currentWorkspace();
        const run = ws.views.filter((v) => v.group === "run").map((v) => v.hash);
        const platform = ws.views.filter((v) => v.group === "platform").map((v) => v.hash);
        assert.equal(run.length, 4, `run 组应当 4 项，实际 ${run.length}（${run.join(",")}）`);
        assert.equal(platform.length, 2, `platform 组应当 2 项，实际 ${platform.length}（${platform.join(",")}）`);
        // ⚠️ **逐项点名**：只判个数 ⇒ ⇒ 「换了两个视图但个数不变」会**假绿**。
        assert.deepEqual([...run].sort(), ["debug", "embedded", "terminal", "waveform"]);
        assert.deepEqual([...platform].sort(), ["product", "source"]);
    });

    test("A-3 · 侧边栏渲染出 6 个导航项，每项带 data-view-tab 与**内联 SVG 图标**", () => {
        const items = sidebarItems();
        assert.equal(items.length, 6, `侧边栏应当有 6 个导航项，实际 ${items.length}`);
        const withIcon = [...items].filter((el) => el.querySelector("svg"));
        assert.equal(withIcon.length, items.length, "每个导航项都应当带一个内联 SVG 图标");
        // ⚠️ **图标是真 SVG 元素**（`createElementNS` 造的）⇒ ⇒ 它的
        // `tagName` 带命名空间 ⇒ ⇒ 不能是 HTML 的 `svg` 标签。
        const first = withIcon[0].querySelector("svg");
        assert.ok(first, "导航项里没有 SVG 元素");
    });

    test("A-4 · 导航项带正确的 href ⇒ ⇒ 真浏览器点它就会切路由", () => {
        const items = sidebarItems();
        // ⚠️ **先钉住件数** ⇒ ⇒ 否则「侧边栏一个项都没有」时这个 `for`
        // **一次都不执行** ⇒ ⇒ 用例**空转通过** ⇒ ⇒ **假绿**
        // （STAGE-34 §8 订正 6：变异自检 T1 实测到的就是这个）。
        assert.equal(items.length, 6, `侧边栏应当有 6 个导航项，实际 ${items.length}`);
        for (const el of items) {
            const hash = el.getAttribute("data-view-tab");
            assert.equal(
                el.getAttribute("href"),
                `#/${hash}`,
                `导航项 ${hash} 的 href 应当是 #/${hash}`,
            );
        }
    });

    test("A-4b · 路由切过去 ⇒ ⇒ 对应面板显示、其余隐藏", () => {
        m01.route("#/waveform");
        const visible = $$("[data-view-panel]").filter((p) => !p.classList.contains("hidden"));
        assert.equal(visible.length, 1, `应当恰好有一个面板可见，实际 ${visible.length}`);
        assert.equal(visible[0].getAttribute("data-view-panel"), "waveform");
    });

    test("A-5 · 当前视图对应的导航项带 aria-current=\"page\"，其余**不带**", () => {
        const items = sidebarItems();
        const current = [...items].filter((el) => el.getAttribute("aria-current") === "page");
        assert.equal(current.length, 1, `应当恰好有一个高亮项，实际 ${current.length}`);
        assert.equal(current[0].getAttribute("data-view-tab"), "waveform");
        // ⚠️ **换视图后高亮要跟着走** ⇒ ⇒ 否则「高亮」是**摆件**。
        m01.route("#/terminal");
        const after = sidebarItems()
            .filter((el) => el.getAttribute("aria-current") === "page");
        assert.equal(after.length, 1, `切换后应当恰好有一个高亮项，实际 ${after.length}`);
        assert.equal(after[0].getAttribute("data-view-tab"), "terminal");
    });
});

describe("STAGE-34 · 顶栏全局运行条", () => {
    test("A-6 · 运行条存在，显示 PC（hex）与虚拟时间，**不显示**指令退休计数", () => {
        const bar = $("[data-topbar-runbar]");
        assert.ok(bar, "顶栏没有运行条（data-topbar-runbar）");
        const pc = $("[data-topbar-pc-value]");
        const time = $("[data-topbar-time-value]");
        assert.ok(pc && time, "运行条上缺 PC 或虚拟时间");
        assert.match(String(pc.textContent), /^0x[0-9a-f]{8}$/, `PC 应当是 8 位 hex，实际 ${pc.textContent}`);
        assert.match(String(time.textContent), /\d/, `虚拟时间应当含数字，实际 ${time.textContent}`);
        // ⚠️ **不显示指令退休计数**（契约层的 SimState 没有那个字段 ⇒ ⇒
        // 无数据来源 ⇒ ⇒ 不编造）⇒ ⇒ 界面上不许出现那种数字。
        const text = String(bar.textContent);
        assert.ok(!/inst|指令/i.test(text), `运行条上出现了「指令计数」⇒ ⇒ 那是没有数据来源的编造：${text}`);
    });

    test("A-6b · 顶栏 PC / 虚拟时间**来自 M-10 快照**（⇒ ⇒ 不是写死的常量）", () => {
        // ⚠️ **判据要问用户真正会问的那个问题**：顶栏那个数字**是不是真的**
        // ⇒ ⇒ 喂一份**已知的**快照进去 ⇒ ⇒ 界面必须**逐字复现**它。
        // ⇒ ⇒ 若有人把 PC 写死成常量 ⇒ ⇒ 这里立刻红。
        const snap = { pc: 0x1234abcd, timeVirt: 123456 };
        const fake = {
            registerMemory: {
                getSnapshot: () => snap,
                subscribe: () => () => {},
            },
        };
        const out = app.syncTopbarStats(fake);
        assert.ok(out, "syncTopbarStats 没有返回读到的状态");
        assert.equal(out.pc, 0x1234abcd, "读到的 PC 与快照不一致");
        assert.equal(out.timeVirt, 123456, "读到的虚拟时间与快照不一致");
        assert.equal(
            String($("[data-topbar-pc-value]").textContent),
            "0x1234abcd",
            "顶栏显示的 PC 与快照不一致",
        );
        assert.equal(
            $("[data-topbar-time-value]").getAttribute("data-topbar-time-ps"),
            "123456",
            "顶栏挂的原始 ps 与快照不一致",
        );
    });

    test("A-7 · 四个按钮的 action 取值与 app.js 的命令分支**逐字对应**", () => {
        // ⚠️ **「暂停」是第四个按钮**（STAGE-37 · v0.52 批 · 裁决 G）⇒ ⇒
        // 「运行」变成连续运行之后**必须**有一个真实出口 ⇒ ⇒ 加按钮就得
        // 同步扩这条判据（否则新按钮**零覆盖**）。
        for (const action of ["run", "step", "pause", "reset"]) {
            const btn = $(`[data-topbar-action="${action}"]`);
            assert.ok(btn, `运行条上缺 ${action} 按钮`);
        }
        // ⚠️ **逐字对应**：按钮的 `data-topbar-action` 与 app.js 里
        // `handleCommand` 的 `case` 分支**必须是同一个词** ⇒ ⇒ 顶栏点一下
        // 才会发出**对**的命令。
        const actionMap = {
            run: "continue",
            step: "step",
            pause: "pause",
            reset: "reset",
        };
        for (const [attr, cmd] of Object.entries(actionMap)) {
            assert.ok(
                appAndRunStateSrc.includes(`{ action: "${cmd}" }`),
                `app.js 里没有把 ${attr} 按钮接到 action: "${cmd}"`,
            );
            assert.ok(
                new RegExp(`case "${cmd}":`).test(appSrc),
                `app.js 的命令分支里没有 case "${cmd}"`,
            );
        }
    });

    test("A-7b · 顶栏按钮**走** handleCommand ⇒ ⇒ 与视图层共用同一条路径", () => {
        // ⚠️ **不许自己写一套** ⇒ ⇒ 顶栏必须调**那个**函数 ⇒ ⇒
        // 「一个约束一个落点」。搬迁后命令分发经注入回调进门面 `handleCommand`：
        // 门面 bootstrap 把 `handleCommand` 作为 dispatch 传给 `wireTopbarRunbar`，
        // 子模块按钮只 `emit` action ⇒ ⇒ 两边合起来仍是同一条路径。
        assert.ok(
            /wireTopbarRunbar\([^,]+,\s*\([^)]*\)\s*=>\s*handleCommand\(/.test(appSrc),
            "门面没有把 handleCommand 作为分发器注入顶栏运行条 ⇒ ⇒ 那是另写了一套路径",
        );
        for (const cmd of ["continue", "step", "pause", "reset"]) {
            assert.ok(
                runStateSrc.includes(`{ action: "${cmd}" }`),
                `运行条子模块没有把按钮接到 action: "${cmd}"`,
            );
        }
    });

    test("A-9 · 每个视图面板有 .vhead 外框（标题 + .hint），标题文字与登记表逐字相同", () => {
        const ws = m01.currentWorkspace();
        for (const v of ws.views) {
            const panel = $(`[data-view-panel="${v.hash}"]`);
            assert.ok(panel, `视图 ${v.hash} 的面板没挂上`);
            const head = panel.querySelector(".vhead");
            assert.ok(head, `视图 ${v.hash} 的面板没有 .vhead 外框`);
            const heading = head.querySelector("h2");
            assert.ok(heading, `视图 ${v.hash} 的 .vhead 里没有 h2 标题`);
            assert.equal(
                String(heading.textContent).trim(),
                v.name,
                `视图 ${v.hash} 的面板标题与登记表不一致`,
            );
            const hint = head.querySelector(".hint");
            assert.ok(hint && String(hint.textContent).trim().length > 0,
                `视图 ${v.hash} 的 .vhead 里没有 .hint 提示`);
        }
    });
});

describe("STAGE-34 · 设计令牌", () => {
    test("A-8 · 令牌在 :root 里逐字等于规格 §5.1 的值（hs-fpv 中性浅色风）", () => {
        const expect = {
            "--bg": "#f4f5f7", "--bg2": "#ffffff", "--panel": "#ffffff",
            "--panel2": "#f5f5f5", "--line": "#e5e5e5", "--line2": "#eeeeee",
            "--tx": "#171717", "--tx2": "#525252", "--tx3": "#737373",
            "--accent": "#171717", "--amber": "#d97706", "--red": "#dc2626",
            "--green": "#16a34a", "--line-strong": "#d4d4d4",
            "--r": "12px", "--r-md": "10px", "--r-sm": "8px",
            "--r-xs": "6px", "--r-pill": "999px",
            // ⚠️ **派生色也要逐字钉住**（STAGE-34 §8 订正 5）：这 6 个原先散落在
            // 组件里逐字写死 ⇒ ⇒ 换主题会漏掉 ⇒ ⇒ 收进 `:root` 后逐字钉住。
            "--on-accent": "#ffffff", "--on-green": "#ffffff",
            "--red-soft": "#dc2626", "--amber-soft": "#b45309",
            "--term-bg": "#18181b", "--term-fg": "#e4e4e7",
        };
        const root = styleSrc.match(/:root\s*\{([\s\S]*?)\n\}/);
        assert.ok(root, "style.css 里没有 :root 块");
        for (const [token, value] of Object.entries(expect)) {
            assert.ok(
                new RegExp(`${token}\\s*:\\s*${value}\\s*;`).test(root[1]),
                `:root 里缺 ${token}: ${value}`,
            );
        }
    });

    test("A-8b · **旧令牌零残留**（换名不换值 ⇒ ⇒ 留两套命名会是第二个真相）", () => {
        for (const old of ["--ink", "--ink2", "--teal", "--bg3"]) {
            assert.ok(
                !new RegExp(`${old}\\s*:`).test(styleSrc),
                `style.css 里还有旧令牌 ${old}`,
            );
        }
    });

    test("A-8c · 颜色字面量**只出现在 :root 里**（组件里不散落）", () => {
        const root = styleSrc.match(/:root\s*\{([\s\S]*?)\n\}/);
        assert.ok(root, "style.css 里没有 :root 块");
        const outside = styleSrc.slice(0, root.index) + styleSrc.slice(root.index + root[0].length);
        // ⚠️ 只查**六位 hex**（`#rrggbb`）⇒ ⇒ 免得把 `#app` 这类 id 选择器
        // 误判成颜色。
        const hits = outside.match(/#[0-9a-fA-F]{6}\b/g) ?? [];
        assert.deepEqual(hits, [], `:root 之外还有颜色字面量：${hits.join(" ")}`);
    });
});

describe("STAGE-34 · 既有门禁不许被放松", () => {
    test("M-01 仍然不许用 innerHTML 拼面板（这条**一字不改**）", () => {
        assert.ok(
            !/\.innerHTML\s*=/.test(m01Src),
            "M-01 又用 innerHTML 拼内容了（#10 的回归）",
        );
    });

    test("侧边栏图标走 createElementNS（SVG 在真浏览器里只能这样造）", () => {
        assert.ok(
            /createElementNS\(/.test(m01Src),
            "侧边栏图标没有用 createElementNS ⇒ ⇒ 真浏览器里它不渲染",
        );
    });
});
