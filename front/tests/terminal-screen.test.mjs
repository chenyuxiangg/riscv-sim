// front/tests/terminal-screen.test.mjs —— 串口终端的**屏幕仿真**门禁
// （STAGE-41 · v0.56 批 · 判据 N-1 ~ N-14）。
//
// ## 这套用例盯的是什么
//
// ⚠️ **触发点是用户反馈**：「往串口界面输入 <code>cyx</code>，输出却是好几行
//   红字」⇒ ⇒ 根因**不在字节流**：guest 的 shell 本来就是**行编辑器**
//   （`SHELL_CLEAN_LINE` = `"\033[2K\r"`，每敲一个键把整行重画一遍）
//   ⇒ ⇒ 字节流里同一个提示符出现 N 次是**对的**⇒ ⇒ 真实终端显示成**一行**
//   ⇒ ⇒ **错的是前端把字节流原样当文本显示**（`\r` 渲染成换行、`ESC[2K`
//   渲染成字面量 `[2K`）。
//
// ⇒ ⇒ 这套用例盯两件事：
//   1. **屏幕上**是「一行 `OSZ$ cyx`」，不是一堆重画叠出来的乱码；
//   2. **字节流仍是原文**（真相没丢）⇒ ⇒ 切到「原始字节」档看得到。

import { test, describe } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { installDom } from "./dom-stub.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const frontDir = resolve(here, "..");

const indexHtml = (await import("node:fs")).readFileSync(resolve(frontDir, "index.html"), "utf8");
const dom = installDom(indexHtml);

// ⚠️ **关掉 `app.js` 的自启动**（与 `console.test.mjs` 同一条理由）：视图状态是
//   **模块级单例** ⇒ 自启动的 `reset()` 可能落在任意两个用例之间。
globalThis.__RVSIM_NO_BOOTSTRAP__ = true;

const importFront = (rel) => import(pathToFileURL(resolve(frontDir, rel)).href);

const app = await importFront("app.js");
const m01 = await importFront("views/m01_workbench.js");
const m02 = await importFront("views/m02_viewcomponents.js");
const m03 = await importFront("views/m03_viewstore.js");
const screen = await importFront("views/components/terminal-screen.js");

await m02.mountViewComponents();

const $ = (selector) => dom.document.querySelector(selector);

/** 终端视图当前显示的文本。 */
function terminalText() {
    const out = $("[data-terminal-out]");
    return out ? String(out.textContent ?? "") : "";
}

/** store 里的**原始字节流**（真相）。 */
function storedBytes() {
    return String(m03.getState("terminal")?.term?.bytes ?? "");
}

// ---------------------------------------------------------------------------
// 夹具（**字面量**，不从被测实现反推）
// ---------------------------------------------------------------------------

/** osz 启动后 console 上的那 31 个字节（与 `console.test.mjs` 同一串）。 */
const OSZ_BOOT = "==== Enter Main ====\n\x1b[2K\rOSZ$ ";

/**
 * osz 的 shell 敲 `cyx` 三个键时**逐键重画**出来的字节。
 *
 * ⚠️ **逐字抄自 OSZ 的 `shell.c`**：`inner_shell_reset_line()` 打的是
 *   `SHELL_CLEAN_LINE`（`"\033[2K\r"`）+ `SHELL_NAME`（`"OSZ$ "`）
 *   + 当前缓冲区 ⇒ ⇒ 每敲一个键就是**一整段**。
 */
const OSZ_TYPED = "\x1b[2K\rOSZ$ c" + "\x1b[2K\rOSZ$ cy" + "\x1b[2K\rOSZ$ cyx";

/** 上面那串文本的**字节数组**形态（契约 v4 的线上形态）。 */
function bytesOf(text) {
    return Array.from(text, (ch) => ch.charCodeAt(0));
}

// ---------------------------------------------------------------------------
// 假内核（学真内核的样子：没订阅就不推）
// ---------------------------------------------------------------------------

function fakeKernel() {
    /** @type {Map<string, Set<Function>>} */
    const subs = new Map();
    /** @type {object[]} */
    const sent = [];
    const topics = new Set();

    return {
        sent,
        subscribe(topic, handler) {
            if (!subs.has(topic)) subs.set(topic, new Set());
            subs.get(topic).add(handler);
            return () => subs.get(topic).delete(handler);
        },
        async dispatchCommand(payload) {
            sent.push(payload);
            if (payload && typeof payload === "object" && "Subscribe" in payload) {
                topics.add(payload.Subscribe.topic);
            }
            return { value: {} };
        },
        emitConsole(bytes) {
            if (!topics.has("sim.console")) return false;
            const event = { variant: "Console", topic: "sim.console", value: { bytes } };
            for (const fn of subs.get("sim.console") ?? []) fn(event);
            return true;
        },
    };
}

/**
 * 装一套干净的接线（视图状态是**模块级单例** ⇒ 每个用例都重置）。
 *
 * @returns {Promise<{router: object, consoleWiring: object}>}
 */
async function setup() {
    m01.assembleWorkspace("embedded");
    m01.route("#/terminal");
    m03.setState("terminal", { term: { bytes: "" } });
    m02.resetTerminalScreen();
    const router = fakeKernel();
    const consoleWiring = app.wireConsole(router);
    await consoleWiring.subscribeConsoleStream();
    return { router, consoleWiring };
}

// ---------------------------------------------------------------------------
// N-1 ~ N-8：屏幕仿真（纯函数）
// ---------------------------------------------------------------------------

describe("屏幕仿真 · 纯函数", () => {
    test("N-1：osz 逐键重画 ⇒ 屏幕上**只有一行**（用户那条反馈的正面判据）", () => {
        const out = screen.renderTerminalScreen(OSZ_BOOT + OSZ_TYPED);
        assert.equal(out, "==== Enter Main ====\nOSZ$ cyx", "屏幕上必须是**一行** `OSZ$ cyx`");
        assert.ok(!out.includes("[2K"), `转义的字面量漏进了屏幕：${JSON.stringify(out)}`);
        assert.ok(!out.includes("\r"), `回车漏进了屏幕：${JSON.stringify(out)}`);
        assert.equal(out.split("\n").length, 2, "两行（启动横幅 + 一行命令）——不是四行重画");
    });

    test("N-2：LF 顺带 CR（host 端 tty 的 ONLCR）⇒ 下一行**顶格**", () => {
        // ⚠️ 没有 ONLCR 的话，"abc" 之后的 `\n` 会让下一行**缩进三格**
        //   ⇒ ⇒ 没有任何真实终端是那个样子 ⇒ ⇒ LF 必须顺带 CR。
        assert.equal(screen.renderTerminalScreen("abc\ndef"), "abc\ndef");
        assert.equal(screen.renderTerminalScreen("abc\ndef").split("\n")[1], "def", "第二行必须顶格");
    });

    test("N-3：CR 回列首并**覆盖**（不是追加）", () => {
        assert.equal(screen.renderTerminalScreen("abc\rX"), "Xbc");
        assert.equal(screen.renderTerminalScreen("abc\rXy"), "Xyc");
    });

    test("N-4：擦除行内的三档（EL 0 / 1 / 2）", () => {
        assert.equal(screen.renderTerminalScreen("abcdef\x1b[2K"), "", "EL 2 ⇒ 整行清空");
        assert.equal(screen.renderTerminalScreen("abcdef\x1b[K"), "abcdef", "EL 0 ⇒ 光标在行尾 ⇒ 不擦");
        assert.equal(screen.renderTerminalScreen("abcdef\x1b[3D\x1b[K"), "abc", "EL 0 ⇒ 光标到行尾之间被擦掉");
        // ⚠️ EL 1 是**含光标格**的（规范口径）⇒ ⇒ "abcdef" 的光标退到第 3 格
        //   ⇒ ⇒ 前 4 格（a b c d）被擦成空白 ⇒ ⇒ 剩下 "    ef"。
        assert.equal(screen.renderTerminalScreen("abcdef\x1b[3D\x1b[1K"), "    ef", "EL 1 ⇒ 行首到光标（含）被擦掉");
    });

    test("N-5：退格与 CSI 光标左右移动（OSZ 的 shell 都用）", () => {
        assert.equal(screen.renderTerminalScreen("abc\b\bX"), "aXc", "BS ⇒ 光标左移一格再写");
        assert.equal(screen.renderTerminalScreen("abc\x1b[1DX"), "abX", "CSI D ⇒ 光标左移一格");
        // ⚠️ CSI C 是**移动光标**而不是插入字符 ⇒ ⇒ 光标跨过的那一格仍是空白
        //   ⇒ ⇒ 屏幕上 "abc" 与 "X" 之间有**一格空白**（真实终端也是这个样子）。
        assert.equal(screen.renderTerminalScreen("abc\x1b[1CX"), "abc X", "CSI C ⇒ 光标右移一格（跨过的那格是空白）");
    });

    test("N-6：SGR 与**不认识**的序列被吞掉（绝不能变成屏幕上的字面量）", () => {
        assert.equal(screen.renderTerminalScreen("\x1b[0mok\x1b[1;31m"), "ok", "SGR 被吞掉");
        assert.equal(screen.renderTerminalScreen("\x1b[99Zok"), "ok", "不认识的 CSI 被吞掉");
        assert.equal(screen.renderTerminalScreen("\x1b]0;title\x07ok"), "ok", "OSC 被吞掉");
        assert.equal(screen.renderTerminalScreen("\x1bP junk \x1b\\ok"), "ok", "DCS 被吞掉");
    });

    test("N-7：空字节流 ⇒ 空屏（不许出现 \"undefined\"）", () => {
        assert.equal(screen.renderTerminalScreen(""), "");
        assert.equal(screen.renderTerminalScreen(undefined), "");
        assert.equal(screen.renderTerminalScreen(null), "");
    });

    test("N-8：28 万字节**不爆调用栈**且屏幕长度正确", () => {
        const big = "A".repeat(280000);
        const out = screen.renderTerminalScreen(big);
        assert.equal(out.length, 280000, "一个字符都不能少");
    });

    test("N-16：渲染时**去掉行尾空格**（屏幕上的空格格是空白）", () => {
        assert.equal(screen.renderTerminalScreen("OSZ$ "), "OSZ$", "提示符那个尾随空格要去掉");
        assert.equal(screen.renderTerminalScreen("a  \nb"), "a\nb", "行尾空格要去掉");
        assert.equal(screen.renderTerminalScreen("a  b"), "a  b", "**行中间**的空格要留着");
    });
});

// ---------------------------------------------------------------------------
// N-9 ~ N-14：视图接线
// ---------------------------------------------------------------------------

describe("终端视图 · 显示的是「屏幕」", () => {
    test("N-9：视图里显示的是**屏幕**，不是字节流原样", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf(OSZ_BOOT + OSZ_TYPED));
        const text = terminalText();
        assert.equal(text, "==== Enter Main ====\nOSZ$ cyx", "屏幕上必须是那一行命令");
        assert.ok(!text.includes("[2K"), `转义字面量还在：${JSON.stringify(text)}`);
        assert.ok(!text.includes("\x1b"), `ESC 还在：${JSON.stringify(text)}`);
    });

    test("N-10：**字节流仍是原文**（真相没丢）", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf(OSZ_BOOT + OSZ_TYPED));
        const raw = storedBytes();
        assert.equal(raw, OSZ_BOOT + OSZ_TYPED, "store 里的字节流必须逐字不变");
        assert.equal(raw.split("\x1b[2K").length - 1, 4, "四次重画（启动 + 三个键）都在字节流里");
        assert.ok(raw.includes("\r"), "回车也必须在字节流里");
    });

    test("N-11：「原始字节」档切得回去，且**不发任何契约命令**", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf(OSZ_BOOT + OSZ_TYPED));
        const before = router.sent.length;
        // 点「原始字节」⇒ ⇒ 视图显示未经解释的字节流。
        $("[data-action=console-raw-toggle]").dispatchEvent(new Event("click"));
        assert.equal(terminalText(), OSZ_BOOT + OSZ_TYPED, "原始字节档必须显示原文");
        assert.equal(router.sent.length, before, "显示开关**不发**契约命令");
        // 再点一次 ⇒ ⇒ 回到屏幕。
        $("[data-action=console-raw-toggle]").dispatchEvent(new Event("click"));
        assert.equal(terminalText(), "==== Enter Main ====\nOSZ$ cyx", "切回来必须还是屏幕");
    });

    test("N-12：增量那条路与「从零重建」**逐字一致**（连续三次事件之后仍然一致）", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf(OSZ_BOOT));
        router.emitConsole(bytesOf("\x1b[2K\rOSZ$ c"));
        router.emitConsole(bytesOf("\x1b[2K\rOSZ$ cy"));
        router.emitConsole(bytesOf("\x1b[2K\rOSZ$ cyx"));
        const shown = terminalText();
        const rebuilt = screen.renderTerminalScreen(storedBytes());
        assert.equal(shown, rebuilt, "增量算出来的屏幕必须与从零重建逐字相同");
        assert.equal(shown, "==== Enter Main ====\nOSZ$ cyx");
    });

    test("N-13：新程序 ⇒ 屏幕**清空**（不继承上一段工程）", async () => {
        const { router, consoleWiring } = await setup();
        router.emitConsole(bytesOf(OSZ_BOOT + OSZ_TYPED));
        assert.ok(terminalText().includes("cyx"), "夹具：先让屏幕上有一行命令");
        consoleWiring.reset();
        router.emitConsole(bytesOf("second program\n"));
        const text = terminalText();
        // ⚠️ 末尾那个 `\n` 是**光标已经换到下一行**⇒ ⇒ 屏幕上是两行
        //   （第二行是空的）⇒ ⇒ 文本以 `\n` 结尾。
        assert.equal(text, "second program\n", "新工程的屏幕里不许有上一个工程的内容");
        assert.ok(!text.includes("cyx"), "上一个工程的字不许残留");
    });

    test("N-14：guest 打印 HTML ⇒ 仍然**不被解析**（NFR-04 口径不变）", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf("<b>hi</b>"));
        assert.equal(terminalText(), "<b>hi</b>", "终端里必须是字面文本");
        const body = $('[data-view-body="terminal"]');
        assert.equal(body.querySelectorAll("b").length, 0, "标签被解析成了元素");
    });

    test("N-15：store 里的字节流被**替换**（不是追加）⇒ ⇒ 屏幕**跟着真相走**", async () => {
        const { router } = await setup();
        router.emitConsole(bytesOf("first program\n"));
        assert.ok(terminalText().includes("first program"), "夹具：先让屏幕上有第一段的内容");
        // ⚠️ **不是追加**：store 里的字节流被**换掉**了 ⇒ ⇒ 增量那份缓存
        //   里的「已吃掉的前缀」不再对得上 ⇒ ⇒ 屏幕必须**从零重建**。
        m03.setState("terminal", { term: { bytes: "second program\n" } });
        router.emitConsole(bytesOf("more\n"));
        const text = terminalText();
        assert.ok(!text.includes("first program"), "被换掉的那一段不许留在屏幕上");
        assert.ok(text.includes("second program"), "屏幕必须跟着 store 的真相走");
        assert.ok(text.includes("more"), "新到的字节也必须在屏幕上");
    });
});
