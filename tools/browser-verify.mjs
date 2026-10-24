#!/usr/bin/env node
// Browser smoke check — a **real** Chrome against the real front end.
//
// Why this script exists
// ----------------------
// `tools/check-front.sh` boots the front end against a DOM *stub*. The
// stub covers state and behaviour, but it cannot cover **mount order**:
// "the first render event arrives before the view components subscribed"
// leaves every view body empty while the stub stays green
// (FRONTEND_Review.md #20, 2026-10-20). Only a real browser exposes
// that class of defect.
//
// The previous round verified in Chrome with a throwaway script under
// `/tmp` — nobody could reproduce the result. This file is the
// committed, dependency-free version of it.
//
// What it verifies
// ----------------
//   - the page really boots: the workbench container has children, the
//     active view has a readable title and a **non-empty body** (the
//     white-screen check);
//   - the service chain really works: the transport reports "connected"
//     and the footer reports "WASM 内核已就绪（Nop 往返成功）";
//   - the **real** kernel really executes: a `step` command makes the
//     register view show `x1 = 0x2a` — the value the WASM kernel
//     computed from the demo firmware (第 1 阶段起前端挂的是真内核，
//     不再是 mock);
//   - the **real** probe pipeline really works: after the demo firmware
//     drives `GPIO0.OUT`, the waveform view lists that channel **with
//     its edge count** (D-46) — not "some content";
//   - the **real** source view really works: after the kernel stepped,
//     the "解析当前 PC 的源码行" button carries **the current PC** in
//     its `data-addr` — not `0` (V-B's VB-1 broke exactly that link
//     while the Node gate stayed green);
//   - the **real RTOS** really runs in the browser: the out-of-repo osz
//     ELF is fetched by URL, loaded, run to its shell prompt, and the
//     terminal view shows `==== Enter Main ====` / `OSZ$` — concrete
//     literals, not "some content" (v0.23 · Lane G). **Skipped, not
//     failed**, when the ELF is not on this machine;
//   - the console is clean: no `console.error`, no uncaught exception.
//
// What it does NOT verify
// -----------------------
//   - it is **not** a gate: it needs a local Chrome and a local HTTP
//     server, so it is deliberately *not* part of `tools/check-front.sh`;
//   - it does not replace the Node gate — the Node gate checks the
//     contract / module behaviour, this one checks the wiring in a
//     browser.
//
// Prerequisites
// -------------
//   - Node >= 22 (the script uses the global `WebSocket` and `fetch`);
//   - a local Chrome / Chromium (see `CHROME_PATH` below);
//   - python3 (to serve `front/` over HTTP — ES modules and Workers
//     need a same-origin HTTP(S) origin, `file://` is blocked);
//   - **the WASM kernel artifact** — `tools/build-wasm.sh` must have been
//     run (the front end now mounts the real kernel, not the mock);
//   - **the osz ELF** (optional) — the real-RTOS section is **skipped**
//     when it is absent. Path: `$OSZ_ELF`, else `/tmp/osz-artifacts/osz.elf`.
//
// Usage
// -----
//   # 1) the script starts the HTTP server itself if nothing answers
//   #    on the port already:
//   node tools/browser-verify.mjs
//
//   # 2) or run the server by hand and let the script reuse it:
//   python3 -m http.server 8080 --directory front
//   node tools/browser-verify.mjs --no-serve
//
//   # 3) point it at another port / origin:
//   node tools/browser-verify.mjs --serve-port 8090 --url http://localhost:8090/
//
//   # 4) use a specific browser binary:
//   CHROME_PATH=/path/to/chrome node tools/browser-verify.mjs
//
// Exit codes
// ----------
//   0  — every assertion passed and the console was clean
//   1  — an assertion failed, or the console reported an error
//   127 — a precondition is missing (no Chrome, no python3) — this is
//         *not* a verification failure

import { spawn, spawnSync } from "node:child_process";
import { createReadStream, existsSync, mkdtempSync, rmSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadElf, findFirstSymbol } from "./elf-symtab.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const frontDir = join(repoRoot, "front");

/** A missing precondition (no Chrome / no python3) — not a test failure. */
const EXIT_PRECONDITION = 127;
/** The verification itself failed. */
const EXIT_FAILED = 1;

/**
 * Console noise that is *not* a front-end defect: the browser asks for
 * `/favicon.ico`, the dev server has none, and the failed request shows
 * up as a console error. The previous review hit the same thing.
 */
const IGNORED_CONSOLE = [/favicon\.ico/i];

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

/**
 * Parse the command line.
 *
 * @param {string[]} argv
 * @returns {{url: string, cdpPort: number, servePort: number, serve: boolean, timeoutMs: number}}
 */
function parseArgs(argv) {
    const opts = {
        url: "http://localhost:8080/",
        cdpPort: 9222,
        servePort: null,
        serve: true,
        timeoutMs: 20000,
    };
    for (let i = 0; i < argv.length; i += 1) {
        const arg = argv[i];
        if (arg === "--url") opts.url = argv[++i];
        else if (arg === "--cdp-port") opts.cdpPort = Number(argv[++i]);
        else if (arg === "--serve-port") opts.servePort = Number(argv[++i]);
        else if (arg === "--timeout-ms") opts.timeoutMs = Number(argv[++i]);
        else if (arg === "--no-serve") opts.serve = false;
        else if (arg === "--help" || arg === "-h") {
            printUsage();
            process.exit(0);
        } else {
            throw new Error(`未知参数：${arg}`);
        }
    }
    if (opts.servePort === null) {
        // The server follows the URL unless it was pinned explicitly.
        const port = new URL(opts.url).port;
        opts.servePort = port ? Number(port) : 80;
    }
    return opts;
}

/** Print the usage banner. */
function printUsage() {
    console.log([
        "用法：node tools/browser-verify.mjs [选项]",
        "",
        "  --url <url>          要打开的页面（默认 http://localhost:8080/）",
        "  --serve-port <port>  HTTP 服务端口（默认取自 --url）",
        "  --cdp-port <port>    Chrome 远程调试端口（默认 9222）",
        "  --timeout-ms <ms>    等待前端就绪的超时（默认 20000）",
        "  --no-serve           不自己起 HTTP 服务（要求服务已在跑）",
    ].join("\n"));
}

/** Sleep. @param {number} ms */
function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Preconditions
// ---------------------------------------------------------------------------

/**
 * Locate an executable on `PATH`.
 *
 * @param {string} name
 * @returns {string|null}
 */
function which(name) {
    const direct = spawnSync("which", [name], { encoding: "utf8" });
    if (direct.status === 0) {
        const found = String(direct.stdout ?? "").trim();
        if (found) return found;
    }
    const viaSh = spawnSync("sh", ["-c", `command -v ${name}`], { encoding: "utf8" });
    if (viaSh.status === 0) {
        const found = String(viaSh.stdout ?? "").trim();
        if (found) return found;
    }
    return null;
}

/**
 * Find a Chrome / Chromium binary.
 *
 * An explicit `CHROME_PATH` wins and is **not** silently ignored — if
 * it points at nothing, the script reports a missing precondition
 * instead of quietly running a different browser.
 *
 * @returns {string|null} the binary path, or `null` when none was found
 */
function resolveChrome() {
    const fromEnv = process.env.CHROME_PATH;
    if (fromEnv) {
        return existsSync(fromEnv) ? fromEnv : null;
    }
    const candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    for (const candidate of candidates) {
        if (existsSync(candidate)) return candidate;
    }
    for (const name of ["google-chrome", "chromium"]) {
        const found = which(name);
        if (found) return found;
    }
    return null;
}

/**
 * Is the front-end server answering?
 *
 * @param {string} url
 * @returns {Promise<boolean>}
 */
async function probeServer(url) {
    try {
        const res = await fetch(url, { method: "GET" });
        return res.ok;
    } catch {
        return false;
    }
}

/**
 * Make sure the front end is served over HTTP.
 *
 * A running server is reused; otherwise `python3 -m http.server` is
 * started for this run (`file://` is blocked by the browser — ES
 * modules and Workers need a same-origin HTTP origin).
 *
 * @param {{url: string, servePort: number, serve: boolean}} opts
 * @returns {Promise<{child: object|null}>} the server process to clean up
 */
async function ensureServer(opts) {
    if (await probeServer(opts.url)) {
        console.log(`· 前端服务已在运行：${opts.url}`);
        return { child: null };
    }
    if (!opts.serve) {
        const err = new Error(
            `前端服务不可达：${opts.url}（--no-serve 表示由你自己起服务）`,
        );
        err.precondition = true;
        throw err;
    }
    const python = which("python3");
    if (!python) {
        const err = new Error(
            "找不到 python3，无法起同源 HTTP 服务（ES module 与 Worker 都要求 HTTP）",
        );
        err.precondition = true;
        throw err;
    }
    console.log(`· 启动 HTTP 服务：${python} -m http.server ${opts.servePort} --directory front`);
    const child = spawn(
        python,
        ["-m", "http.server", String(opts.servePort), "--directory", frontDir],
        { cwd: repoRoot, stdio: ["ignore", "ignore", "pipe"] },
    );
    child.stderr?.on("data", (chunk) => {
        // The dev server logs every request — keep it out of the report.
        void chunk;
    });
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
        if (await probeServer(opts.url)) return { child };
        if (child.exitCode !== null) {
            throw new Error(`HTTP 服务退出了（code=${child.exitCode}）`);
        }
        await sleep(150);
    }
    child.kill("SIGTERM");
    throw new Error(`HTTP 服务没能在 10s 内就绪（端口 ${opts.servePort}）`);
}

// ---------------------------------------------------------------------------
// Chrome DevTools Protocol
// ---------------------------------------------------------------------------

/**
 * A minimal CDP client on top of the global `WebSocket`.
 */
class CdpClient {
    /**
     * @param {WebSocket} socket
     */
    constructor(socket) {
        this.socket = socket;
        this.nextId = 1;
        /** @type {Map<number, {resolve: Function, reject: Function, method: string}>} */
        this.pending = new Map();
        /** @type {Map<string, Set<Function>>} */
        this.listeners = new Map();
        socket.addEventListener("message", (ev) => this.#onMessage(String(ev.data)));
    }

    /**
     * @param {string} raw
     */
    #onMessage(raw) {
        let msg;
        try {
            msg = JSON.parse(raw);
        } catch {
            return;
        }
        if (typeof msg.id === "number") {
            const entry = this.pending.get(msg.id);
            if (!entry) return;
            this.pending.delete(msg.id);
            if (msg.error) {
                entry.reject(new Error(`${entry.method} 失败：${msg.error.message}`));
            } else {
                entry.resolve(msg.result);
            }
            return;
        }
        const handlers = this.listeners.get(msg.method);
        if (!handlers) return;
        for (const handler of [...handlers]) {
            try {
                handler(msg.params ?? {});
            } catch (err) {
                console.warn("CDP 事件处理失败：", err);
            }
        }
    }

    /**
     * Send one CDP command and wait for its answer.
     *
     * @param {string} method
     * @param {object} [params]
     * @param {number} [timeoutMs]
     * @returns {Promise<object>}
     */
    send(method, params = {}, timeoutMs = 15000) {
        const id = this.nextId;
        this.nextId += 1;
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(`${method} 超时（${timeoutMs}ms）`));
            }, timeoutMs);
            this.pending.set(id, {
                method,
                resolve: (value) => {
                    clearTimeout(timer);
                    resolve(value);
                },
                reject: (err) => {
                    clearTimeout(timer);
                    reject(err);
                },
            });
            this.socket.send(JSON.stringify({ id, method, params }));
        });
    }

    /**
     * Subscribe to a CDP event.
     *
     * @param {string} method
     * @param {(params: object) => void} handler
     * @returns {() => void} the unsubscribe fn
     */
    on(method, handler) {
        let set = this.listeners.get(method);
        if (!set) {
            set = new Set();
            this.listeners.set(method, set);
        }
        set.add(handler);
        return () => set.delete(handler);
    }

    /** Close the socket. */
    close() {
        try {
            this.socket.close();
        } catch {
            // already closed
        }
    }
}

/**
 * Launch headless Chrome and wait for its DevTools endpoint.
 *
 * @param {string} chromePath
 * @param {number} port
 * @param {string} userDataDir
 * @returns {Promise<{child: object, version: object}>}
 */
async function launchChrome(chromePath, port, userDataDir) {
    const baseArgs = [
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-background-networking",
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${userDataDir}`,
        "--window-size=1280,900",
        "about:blank",
    ];
    let child = null;
    let stderr = "";
    for (const headless of ["--headless=new", "--headless"]) {
        child = spawn(chromePath, [headless, ...baseArgs], { stdio: ["ignore", "ignore", "pipe"] });
        stderr = "";
        child.stderr?.on("data", (chunk) => {
            stderr += String(chunk);
        });
        const version = await waitForDevTools(port, 8000, child);
        if (version) return { child, version };
        child.kill("SIGTERM");
    }
    throw new Error(
        `Chrome 起不来（${chromePath}）。最后一段 stderr：\n${stderr.trim().slice(-800)}`,
    );
}

/**
 * Poll the DevTools `/json/version` endpoint.
 *
 * @param {number} port
 * @param {number} timeoutMs
 * @param {object} child — the Chrome process (used to detect an early exit)
 * @returns {Promise<object|null>}
 */
async function waitForDevTools(port, timeoutMs, child) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        if (child && child.exitCode !== null) return null;
        try {
            const res = await fetch(`http://127.0.0.1:${port}/json/version`);
            if (res.ok) return await res.json();
        } catch {
            // not up yet
        }
        await sleep(120);
    }
    return null;
}

/**
 * The page target's DevTools WebSocket URL.
 *
 * @param {number} port
 * @returns {Promise<string|null>}
 */
async function findPageTarget(port) {
    const res = await fetch(`http://127.0.0.1:${port}/json/list`);
    const targets = await res.json();
    const page = (Array.isArray(targets) ? targets : [])
        .find((t) => t && t.type === "page" && t.webSocketDebuggerUrl);
    return page ? page.webSocketDebuggerUrl : null;
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/** The DOM probe — evaluated in the page. */
const PROBE_EXPRESSION = `(() => {
  const app = document.getElementById("app");
  const panels = Array.from(document.querySelectorAll("[data-view-panel]"));
  const active = panels.find((p) => !p.classList.contains("hidden")) || null;
  const heading = active ? active.querySelector("h2") : null;
  const body = active ? active.querySelector("[data-view-body]") : null;
  const regCells = Array.from(document.querySelectorAll('[data-view-body="regmem"] .reg-grid > div'));
  const cellText = (i) => {
    const code = regCells[i] ? regCells[i].querySelector("code") : null;
    return code ? String(code.textContent || "").trim() : "";
  };
  const waveItems = Array.from(document.querySelectorAll('[data-view-body="waveform"] .channel-list > li'));
  // -- 终端视图（真实 RTOS 的 console）--
  // ⚠️ 用 data-* 选择器取：**不靠文案匹配**去找元素。
  const termOut = document.querySelector("[data-terminal-out]");
  // -- 源码视图（STAGE-7 裁决 E / F）--
  // ⚠️ 全部用 data-* 选择器取：**不靠文案匹配**去找元素。
  const sourceBody = document.querySelector('[data-view-body="source"]');
  const resolveButtons = Array.from(document.querySelectorAll('[data-action="resolve-line"]'));
  const resolveBtn = resolveButtons[0] || null;
  const sourceStatusEl = document.querySelector("[data-source-status]");
  // -- 落地视图（embedded · D-33）--
  // ⚠️ 用 data-* 选择器取：**不靠文案匹配**去找元素。
  const embeddedBody = document.querySelector('[data-view-body="embedded"]');
  const embeddedPc = document.querySelector('[data-view-body="embedded"] [data-embedded-pc]');
  const embeddedLink = document.querySelector('[data-view-body="embedded"] [data-embedded-link]');
  const embeddedEntries = Array.from(
    document.querySelectorAll('[data-view-body="embedded"] [data-embedded-entry]'),
  );
  return {
    hasApp: Boolean(app),
    panelCount: panels.length,
    appChildren: app ? app.childElementCount : 0,
    activePanel: active ? active.getAttribute("data-view-panel") : null,
    title: heading ? String(heading.textContent || "").trim() : "",
    bodyChars: body ? String(body.innerHTML || "").trim().length : 0,
    // -- 落地视图（embedded）--
    embeddedBodyChars: embeddedBody ? String(embeddedBody.innerHTML || "").trim().length : 0,
    embeddedPcText: embeddedPc ? String(embeddedPc.textContent || "").trim() : "",
    embeddedLinkText: embeddedLink ? String(embeddedLink.textContent || "").trim() : "",
    embeddedLinkConnected: embeddedLink ? String(embeddedLink.getAttribute("data-connected") ?? "") : "",
    embeddedEntryCount: embeddedEntries.length,
    embeddedEntryTargets: embeddedEntries.map((el) =>
      String(el.getAttribute("data-embedded-entry") || "")),
    regCells: regCells.length,
    regX1: cellText(1),
    regX2: cellText(2),
    pc: String((document.querySelector('[data-view-body="regmem"] .pc') || {}).textContent || "").trim(),
    waveChannels: waveItems.length,
    waveLabel0: waveItems[0]
      ? String((waveItems[0].querySelector(".channel-label") || {}).textContent || "").trim()
      : "",
    waveItemText0: waveItems[0] ? String(waveItems[0].textContent || "").trim() : "",
    // -- 终端视图 --
    termText: termOut ? String(termOut.textContent || "") : "",
    // -- 源码视图 --
    sourceBtnCount: resolveButtons.length,
    sourceAddr: resolveBtn ? String(resolveBtn.getAttribute("data-addr") ?? "").trim() : "",
    sourceStatusText: sourceStatusEl ? String(sourceStatusEl.textContent || "").trim() : "",
    sourceBodyChars: sourceBody ? String(sourceBody.innerHTML || "").trim().length : 0,
    resolveLineCmds: Array.isArray(window.__bvResolveLineCmds)
      ? window.__bvResolveLineCmds.map((v) => String(v))
      : [],
    status: String((document.getElementById("status") || {}).textContent || "").trim(),
    note: String((document.getElementById("kernel-note") || {}).textContent || "").trim(),
  };
})()`;

/**
 * 在页面里挂一个**命令捕获器**：记录每一条 `riscv:command` 信号里的
 * `resolve-line` 地址。
 *
 * ⚠️ 这只是**运行时插桩**，不改前端源码：视图层发命令的方式就是往
 * `window` 上抛 `riscv:command`（M-02 的 `emitCommand`）⇒ 在这里监听
 * 就能看到「按钮点下去到底发了什么」。
 */
const CAPTURE_RESOLVE_LINE_EXPRESSION = `(() => {
  if (!Array.isArray(window.__bvResolveLineCmds)) window.__bvResolveLineCmds = [];
  if (!window.__bvResolveLineHooked) {
    window.__bvResolveLineHooked = true;
    window.addEventListener("riscv:command", (ev) => {
      const detail = ev && ev.detail ? ev.detail : {};
      if (detail.action !== "resolve-line") return;
      window.__bvResolveLineCmds.push(String((detail.params || {}).addr ?? ""));
    });
  }
  return window.__bvResolveLineCmds.length;
})()`;

/**
 * 点一下「解析当前 PC 的源码行」按钮（用 `data-*` 选择器找，不靠文案）。
 */
const CLICK_RESOLVE_LINE_EXPRESSION = `(() => {
  const btn = document.querySelector('[data-action="resolve-line"]');
  if (!btn) return false;
  btn.click();
  return true;
})()`;

/**
 * 反汇编面板的**逐条源码位置**（K-3 / E-6）。
 *
 * ⚠️ **用 `data-*` 取，不从渲染文本里猜**：文件名里可能带 `:`，而
 * 「真源码路径 + 行号」是**两个**独立的值（`file` / `line`）。
 */
const DISASM_ROWS_EXPRESSION = `(() => {
  const rows = Array.from(document.querySelectorAll('[data-view-body="debug"] tr.disasm-row'));
  return rows.map((r) => {
    const cell = r.querySelector(".disasm-source");
    return {
      addr: String(r.getAttribute("data-disasm-addr") || ""),
      file: cell ? String(cell.getAttribute("data-disasm-file") || "") : "",
      line: cell ? String(cell.getAttribute("data-disasm-line") || "") : "",
      text: cell ? String(cell.textContent || "") : "",
    };
  });
})()`;

/**
 * 在终端视图的**输入框**里敲一段文本并点「发送」（K-1 / E-8）。
 *
 * ⚠️ 走的是**生产路径**：输入框 → `input` 事件 → 草稿进视图状态 →
 * 「发送」按钮 → `riscv:command` 信号 → `WriteConsole` 命令。
 *
 * @param {string} text — 要送的文本（**按 UTF-8 编码**成字节）
 * @returns {string} the expression to evaluate in the page
 */
function sendTerminalTextExpression(text) {
    return `(() => {
  const input = document.querySelector("[data-terminal-input]");
  const btn = document.querySelector('[data-action="console-send"]');
  if (!input || !btn) return { ok: false, reason: "输入框或发送按钮不在" };
  input.value = ${JSON.stringify(text)};
  input.dispatchEvent(new Event("input", { bubbles: true }));
  btn.click();
  return { ok: true, draft: String(input.value) };
})()`;
}

/**
 * Landing-page checks: the workbench really came up.
 *
 * ⚠️ **落地视图就是 `embedded` 面板**（嵌入式角色的落地视图）⇒ 它的正文
 * **必须**有实质内容（D-33 · STAGE-10 裁决 C：以前这里没有 M-02 组件 ⇒
 * 「标题在、正文空」）。
 *
 * ⚠️ **判别力**：只断言「正文非空」对**白屏**是绿的 ⇒ 这里落到**具体内容**
 * —— PC 的十六进制值 + 四个子视图入口的 `data-*` 标记 + 连接状态。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function landingChecks(probe) {
    const entryTargets = Array.isArray(probe.embeddedEntryTargets) ? probe.embeddedEntryTargets : [];
    return [
        {
            name: "工作台容器 #app 存在",
            ok: probe.hasApp === true,
            detail: `hasApp=${probe.hasApp}`,
        },
        {
            name: "工作台容器挂出了子节点",
            ok: probe.appChildren > 0,
            detail: `appChildren=${probe.appChildren}`,
        },
        {
            name: "视图面板已挂载",
            ok: probe.panelCount > 0,
            detail: `panelCount=${probe.panelCount}`,
        },
        {
            name: "当前视图有可读的标题文本",
            ok: typeof probe.title === "string" && probe.title.length > 0,
            detail: `active=${probe.activePanel} title="${probe.title}"`,
        },
        {
            name: "传输层已连接",
            ok: typeof probe.status === "string" && probe.status.includes("已连接"),
            detail: `status="${probe.status}"`,
        },
        {
            name: "WASM 内核已就绪（Nop 往返）",
            ok: typeof probe.note === "string" && probe.note.includes("WASM 内核已就绪"),
            detail: `note="${probe.note}"`,
        },
        {
            name: "落地视图（embedded）正文非空（不是白屏）",
            ok: Number(probe.embeddedBodyChars ?? 0) > 0,
            detail: `embeddedBodyChars=${probe.embeddedBodyChars}`,
        },
        {
            name: "落地视图正文有实质内容（PC 十六进制 + 四个子视图入口）",
            ok: /^0x[0-9a-fA-F]{8}$/.test(String(probe.embeddedPcText ?? "").trim())
                && Number(probe.embeddedEntryCount ?? 0) >= 4
                && ["debug", "regmem", "waveform", "source"].every((t) => entryTargets.includes(t)),
            detail: `pc="${probe.embeddedPcText}" entries=${probe.embeddedEntryCount} targets=[${entryTargets.join(",")}]`,
        },
        {
            name: "落地视图显示内核连接状态（连接 / 未连接）",
            ok: ["0", "1"].includes(String(probe.embeddedLinkConnected ?? ""))
                && typeof probe.embeddedLinkText === "string"
                && (probe.embeddedLinkText.includes("已连接") || probe.embeddedLinkText.includes("未连接")),
            detail: `connected="${probe.embeddedLinkConnected}" text="${probe.embeddedLinkText}"`,
        },
    ];
}

/**
 * **真内核执行**检查：demo 固件真的跑起来了。
 *
 * 这是本脚本与 mock 时代的**本质区别**：以前只能验「内核已就绪」，
 * 现在能验「WASM 内核真的执行了指令」——发一条 `step` 命令，然后读
 * 寄存器视图里 x1 的值。
 *
 * demo 固件第一条指令是 `addi x1, x0, 42`，所以单步一次之后 x1 必须是
 * `0000002a`。这条断言一旦变红，只有三种可能：内核没接上、命令没派发、
 * 或者波形增量没推到视图——**不是**「mock 还在」这种含混的失败。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function kernelExecChecks(probe) {
    return [
        {
            name: "WASM 内核执行了 demo 固件（x1 = 42）",
            ok: probe.regX1 === "0000002a",
            detail: `x1="${probe.regX1}"`,
        },
        {
            name: "PC 随单步前进（0x00000004）",
            ok: probe.pc === "0x00000004",
            detail: `pc="${probe.pc}"`,
        },
    ];
}

/**
 * Render-path checks: the view body is really filled in.
 *
 * This is the check the DOM stub cannot make: "the first render event
 * arrives before the view component subscribed" leaves every view body
 * empty while the Node gate stays green (FRONTEND_Review.md #20).
 *
 * ⚠️ **「落地视图正文非空」这一条住在 `landingChecks`**（D-33 · 裁决 C）：
 * 它查的是**落地视图**（`embedded`）的正文 ⇒ 断言落点跟着落地视图走，
 * 不再挂在 `regmem` 上（`regmem` 的正文由「32 个寄存器格」那条覆盖）。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function renderChecks(probe) {
    return [
        {
            name: "路由切到了有组件的视图（regmem）",
            ok: probe.activePanel === "regmem",
            detail: `active=${probe.activePanel}`,
        },
        {
            name: "该视图有标题文本",
            ok: typeof probe.title === "string" && probe.title.length > 0,
            detail: `title="${probe.title}"`,
        },
        {
            name: "寄存器视图渲染出 32 个寄存器格",
            ok: probe.regCells === 32,
            detail: `regCells=${probe.regCells}`,
        },
    ];
}

/**
 * **探针流**检查：波形视图真的列出了真内核推来的通道（D-46）。
 *
 * 这是「波形视图不再是空的」的真机证据：demo 固件写 GPIO0.OUT → M-15
 * 报边沿 → M-18 判沿 → 宿主推契约事件 → 前端落进视图状态 → 视图列出
 * 通道。
 *
 * ⚠️ 断言的是**通道名与边沿数**，不是「有内容」：只断言「非空」的话，
 * 页面里随便塞点假数据也能绿（这正是第 1 阶段那版 mock `probe.*` 流的
 * 问题）。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function waveformChecks(probe) {
    return [
        {
            name: "波形视图列出了 GPIO0.OUT 通道",
            ok: probe.waveChannels >= 1 && probe.waveLabel0 === "GPIO0.OUT",
            detail: `channels=${probe.waveChannels} label0="${probe.waveLabel0}"`,
        },
        {
            name: "该通道带着固件产生的边沿",
            ok: /\b1 edges\b/.test(probe.waveItemText0 ?? ""),
            detail: `item0="${probe.waveItemText0}"`,
        },
    ];
}

/**
 * **源码视图**检查：这条链路必须真的通到「当前 PC」（VB-7）。
 *
 * ⚠️ **断言的是具体值**（D-45 的教训）：「解析当前 PC 的源码行」按钮的
 * `data-addr` 必须**等于当前 PC 的十进制值**。
 *
 * 为什么要这么严：V-B 检视查出的 VB-1 是**生产路径真的断了**——源码视图
 * 收不到 `sim.pc`（M-10 挂载时没把 `source` 列进去）⇒ 按钮**恒问地址 0**。
 * 而门禁 243 条**全绿**，因为用例**手工注入**了 `sim.pc`。只断言「按钮
 * 存在」或「地址非空」都抓不到它——**0 也是非空**。
 *
 * ⚠️ **期望值拿不到就让这条检查失败**（不许退化成弱断言）：当前 PC 取自
 * 寄存器视图渲染出来的那行十六进制文本（`0x00000004`）⇒ 换成十进制才是
 * `data-addr` 里的形态。取不到 ⇒ `expectedAddr` 为空 ⇒ 判红。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function sourceChecks(probe) {
    const pcHex = String(probe.pc ?? "").trim();
    const pcNum = /^0x[0-9a-fA-F]+$/.test(pcHex) ? Number.parseInt(pcHex, 16) : Number.NaN;
    // `>>> 0` 与视图侧 `Number(...) >>> 0` 的口径一致（32 位无符号）。
    const expectedAddr = Number.isFinite(pcNum) ? String(pcNum >>> 0) : "";
    const addr = String(probe.sourceAddr ?? "").trim();

    return [
        {
            name: "路由切到了源码视图（#/source）",
            ok: probe.activePanel === "source",
            detail: `active=${probe.activePanel}`,
        },
        {
            name: "源码视图有标题文本",
            ok: typeof probe.title === "string" && probe.title.length > 0,
            detail: `title="${probe.title}"`,
        },
        {
            name: "源码视图正文非空（不是白屏）",
            ok: Number(probe.sourceBodyChars ?? 0) > 0,
            detail: `sourceBodyChars=${probe.sourceBodyChars}`,
        },
        {
            name: "「解析当前 PC 的源码行」按钮存在",
            ok: probe.sourceBtnCount === 1,
            detail: `buttons=${probe.sourceBtnCount} data-addr="${addr}"`,
        },
        {
            name: "按钮的 data-addr 等于当前 PC（不是 0）",
            ok: expectedAddr !== "" && expectedAddr !== "0" && addr === expectedAddr,
            detail: `pc="${pcHex}" expected="${expectedAddr}" data-addr="${addr}"`,
        },
        {
            name: "未装载源码时如实显示「未装载源码」",
            ok: typeof probe.sourceStatusText === "string"
                && probe.sourceStatusText.includes("未装载源码"),
            detail: `status="${probe.sourceStatusText}"`,
        },
    ];
}

/**
 * 源码视图的**命令腿**：点按钮 ⇒ 真的发出 `resolve-line` 命令，且命令里
 * 带的地址**等于当前 PC**。
 *
 * ⚠️ 这一条查的是「按钮 → `riscv:command` 信号」那一段：按钮渲染对了但
 * `onCommand` 没把地址带出去的话，前一条断言照样绿。
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}}
 */
function sourceCommandCheck(probe) {
    const pcHex = String(probe.pc ?? "").trim();
    const pcNum = /^0x[0-9a-fA-F]+$/.test(pcHex) ? Number.parseInt(pcHex, 16) : Number.NaN;
    const expectedAddr = Number.isFinite(pcNum) ? String(pcNum >>> 0) : "";
    const cmds = Array.isArray(probe.resolveLineCmds) ? probe.resolveLineCmds : [];
    return {
        name: "点按钮真的发出 resolve-line 命令（地址等于当前 PC）",
        ok: expectedAddr !== "" && expectedAddr !== "0"
            && cmds.length > 0 && cmds[0] === expectedAddr,
        detail: `cmds=${JSON.stringify(cmds)} expected="${expectedAddr}"`,
    };
}

// ---------------------------------------------------------------------------
// 真实 RTOS（osz）—— 仓外镜像的装载与 console 的可见性
// ---------------------------------------------------------------------------

/**
 * osz 的 ELF 默认在哪（**在仓外**，不进仓库）。
 *
 * ⚠️ 路径来自 `$OSZ_ELF`，其次 `/tmp/osz-artifacts/osz.elf`。
 */
const OSZ_ELF_DEFAULT = "/tmp/osz-artifacts/osz.elf";

/**
 * osz 的**调度器**符号候选（**按优先级**排）。
 *
 * ⚠️ **不要把某一个名字写死成唯一答案**（那会变成「恒绿」的断言）：
 * 先**按名字去符号表里查**，找到**第一个**存在的就用它；一个都没有 ⇒
 * 判负（并说明没找到哪些候选）。
 */
const SCHEDULER_SYMBOL_CANDIDATES = [
    "first_schedule",
    "schedule",
    "os_preemp_reschedule",
    "os_preemp_schedule_with_pm",
];

/** UART0（ns16550）的基址。 */
const UART0_BASE = 0x10000000;
/** ns16550 的 LSR 偏移（`0x5`）。 */
const UART_LSR_OFFSET = 0x5;
/** LSR 的寄存器地址。 */
const UART_LSR_ADDR = UART0_BASE + UART_LSR_OFFSET;
/** LSR 的 **bit0 = RX_VALID**（接收 FIFO 里有字节可读）。 */
const LSR_RX_VALID = 0x1;

/**
 * 跑 osz 到提示符的**墙钟上限**（毫秒）。
 *
 * ⚠️ 真实 RTOS 是**死循环**（跑完提示符也不会自己停）⇒ 必须有上限，
 * 否则浏览器测试会永远挂着。
 */
const OSZ_RUN_TIMEOUT_MS = 60000;

/** 断点命中后，等 DOM 里的 PC 更新上来的**上限**（毫秒）。 */
const PC_SETTLE_TIMEOUT_MS = 5000;

/**
 * 一个**带 CORS 头**的静态服务（给**仓外**的 osz ELF 用）。
 *
 * ⚠️ **为什么不能直接用前端的 HTTP 服务**：osz 的 ELF 在**仓外**
 * （不进仓库），而前端的服务只服务 `front/` 目录 ⇒ 页面 `fetch` 不到它。
 *
 * ⚠️ **为什么需要 CORS 头**：产物服务与页面**不同源**（端口不同）⇒ 浏览器
 * 的同源策略会拦掉 `fetch`。加 `Access-Control-Allow-Origin: *` 即可
 * （这是一次性的本地复验设施，不是产品代码）。
 *
 * @param {string} root — 要服务的目录
 * @returns {Promise<{port: number, close: () => void}>}
 */
function startArtifactServer(root) {
    return new Promise((resolvePromise, rejectPromise) => {
        const server = createServer((req, res) => {
            const name = decodeURIComponent((req.url ?? "/").split("?")[0])
                .replace(/^\/+/, "");
            // ⚠️ **只服务该目录下的单个文件**（不做目录穿越）。
            if (!name || name.includes("/") || name.includes("..")) {
                res.writeHead(400);
                res.end("bad path");
                return;
            }
            const file = join(root, name);
            if (!existsSync(file) || !statSync(file).isFile()) {
                res.writeHead(404);
                res.end("not found");
                return;
            }
            res.writeHead(200, {
                "Content-Type": "application/octet-stream",
                "Content-Length": String(statSync(file).size),
                "Access-Control-Allow-Origin": "*",
            });
            createReadStream(file).pipe(res);
        });
        server.on("error", rejectPromise);
        server.listen(0, "127.0.0.1", () => {
            const addr = server.address();
            resolvePromise({
                port: addr.port,
                close: () => server.close(),
            });
        });
    });
}

/**
 * **终端视图**的检查：真实 RTOS 的 console 真的显示出来了。
 *
 * ⚠️ **断言值是具体字面量**，不是「非空」：只断言「终端里有内容」抓不到
 * 「跑的是 demo 固件而不是 osz」这种错（demo 固件根本不打 console ⇒
 * 终端会是空的，但将来某个别的固件可能会打点东西）。
 *
 * ⚠️ **断言 ANSI 转义还在**：osz 的 console 含 `\x1b[2K`（清行）⇒ 终端
 * **不许过滤**它。（`\r` 不在这里断言：浏览器把 HTML 里的裸 `\r` 规范化成
 * `\n` 那是 HTML 解析器的行为，不是前端过滤；字节级的 `\r` 由 Node 门禁
 * 盯。）
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function oszConsoleChecks(probe) {
    const text = String(probe.termText ?? "");
    const shown = JSON.stringify(text.slice(0, 200));
    return [
        {
            name: "终端视图显示 osz 的启动行（==== Enter Main ====）",
            ok: text.includes("==== Enter Main ===="),
            detail: `termText=${shown}`,
        },
        {
            name: "终端视图显示 osz 的提示符（OSZ$）",
            ok: text.includes("OSZ$"),
            detail: `termText=${shown}`,
        },
        {
            name: "console 的 ANSI 转义原样显示（\\x1b[2K 没被过滤）",
            ok: text.includes("\x1b[2K"),
            detail: `termText=${shown}`,
        },
    ];
}

// ---------------------------------------------------------------------------
// The board memory map (STAGE-14 · acceptance C-8)
// ---------------------------------------------------------------------------

/**
 * 这一段配置的那张板。
 *
 * ⚠️ **必须是一份合法的描述**（契约层 V-1 ~ V-9）：`schema_version` 是
 * **描述自己的版本 1**（**不是**契约的 6）· `id` 非空 · 每段的声明
 * `size` ≤ `region_budget_bytes`（V-7）· 两段**不重叠**（V-6）。
 *
 * ⚠️ **为什么第一段在 0**：demo 固件落在基址 0 ⇒ 这张板必须装得下它，
 * 否则「重新装载工程」会失败 ⇒ 用户的板就永远生效不了（那正是
 * 「用户的板跨装载保持」这条要防的坑的**反面**）。
 *
 * ⚠️ **为什么第二段在 0x8000_0000**：它就是**被测的那一段** —— 8 MiB 的
 * 新 RAM，写进去读得回来、越界写不进去。
 */
const BV_BOARD = {
    schema_version: 1,
    id: "browser-verify-board",
    name: "browser-verify 板",
    priority: 999,
    region_budget_bytes: 8388608,
    regions: [
        { kind: "ram", base: 0, size: 1048576 },
        { kind: "ram", base: 2147483648, size: 8388608 },
    ],
};
/** 被测的新 RAM 段的基址（= 0x8000_0000）。 */
const BV_NEW_RAM_BASE = 2147483648;
/** 被测的新 RAM 段的大小（8 MiB）。 */
const BV_NEW_RAM_SIZE = 8388608;
/** 写进新 RAM 的那个值（0xA5A5_A5A5 ⇒ 肉眼可辨）。 */
const BV_NEW_RAM_VALUE = 0xa5a5a5a5;

/**
 * 读产品视图里那张内存映射表的**行**（按 `data-*`，**不靠文案**）。
 *
 * ⚠️ `data-region-size` **外设行没有**（外设窗口的大小由器件自报，声明里
 * **没有** ⇒ 挂了就是编数据）⇒ 这里用 `null` 如实表示「没有」。
 */
const BV_MEMORY_MAP_PROBE = `(() => {
  const rows = Array.from(
    document.querySelectorAll('[data-view-body="product"] [data-region-base]'),
  );
  const editor = document.querySelector('[data-view-body="memory-map"]');
  const submit = document.querySelector('[data-action="submit-memory-map"]');
  const pending = document.querySelector("[data-memory-map-pending]");
  return {
    rowCount: rows.length,
    rows: rows.map((el) => ({
      base: String(el.getAttribute("data-region-base") || ""),
      kind: String(el.getAttribute("data-region-kind") || ""),
      size: el.getAttribute("data-region-size") === null
        ? null
        : String(el.getAttribute("data-region-size")),
    })),
    editorPresent: Boolean(editor),
    submitPresent: Boolean(submit),
    pendingVisible: Boolean(pending),
  };
})()`;

/**
 * 在页面里挂一个**命令捕获器**：记录每一条 `riscv:command` 信号。
 *
 * ⚠️ 与 `CAPTURE_RESOLVE_LINE_EXPRESSION` 同一套路 —— 只是**运行时插桩**，
 * 不改前端源码。
 */
const BV_CAPTURE_COMMANDS_EXPRESSION = `(() => {
  if (!Array.isArray(window.__bvCmds)) window.__bvCmds = [];
  if (!window.__bvCmdsHooked) {
    window.__bvCmdsHooked = true;
    window.addEventListener("riscv:command", (ev) => {
      const detail = (ev && ev.detail) ? ev.detail : {};
      window.__bvCmds.push({
        action: String(detail.action || ""),
        viewId: String(detail.viewId || ""),
        params: detail.params ?? null,
      });
    });
  }
  return true;
})()`;

/**
 * 板级内存图那一段（v0.29 批 · 验收 **C-8** 的端到端判别式）。
 *
 * 五步：
 *   ① 编辑器的「提交」按钮**真的**发出 `set-memory-map` 命令（UI 层 ⇒ C-3）；
 *   ② 这条命令走**真内核**（WASM 产物）⇒ 内存映射表变成新图
 *      （**命令 → 内核 → 事件 → 视图**的整条链 ⇒ C-4）；
 *   ③ 新 RAM 段**写-读往返**成功（证明那段真被映射了、且可写）；
 *   ④ 写新 RAM **之外** ⇒ 失败（证明 size 真的是那个 size）。
 *
 * ⚠️ **这一段必须排在 osz 段之后**：用户的板一旦配过就**一直**保持（宿主里
 * 那个字段存的是「用户意图」，不随一次会话结束而消失）⇒ 放在 osz 前面会把
 * osz 段要用的那张板换掉 ⇒ osz 的控制台就没了。
 *
 * ⚠️ **越界写判的是「失败」不是「报错文案」**：内核对未映射访问报的是
 * `E_BUS-01`，而**判据是「这次写没成功」** ⇒ 不靠 message 文本。
 *
 * ⚠️ **guest 指令级的越界陷阱（`mcause` 5 / 7）不在这一段**：浏览器里没法
 * 方便地让访客执行一条越界访存指令 ⇒ 那条由 **B-8 的 guest 层用例**（Rust
 * 侧）钉住；这一段钉的是「**新 RAM 段真的通了**」这条端到端链路。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runMemoryMapSection(cdp, opts) {
    /** @type {{name: string, ok: boolean, detail: string}[]} */
    const checks = [];
    const add = (name, ok, detail) => checks.push({ name, ok: Boolean(ok), detail: String(detail) });

    // -- ① 切到编辑器视图，等它挂上 ----------------------------------------
    await cdp.send("Runtime.evaluate", {
        expression: 'location.hash = "#/memory-map";',
        returnByValue: true,
    });
    await cdp.send("Runtime.evaluate", {
        expression: BV_CAPTURE_COMMANDS_EXPRESSION,
        returnByValue: true,
    });

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: BV_MEMORY_MAP_PROBE,
            returnByValue: true,
        });
        probe = res.result?.value ?? null;
        if (probe?.submitPresent) break;
        await sleep(250);
    }
    if (!probe?.submitPresent) {
        return {
            skipped: false,
            checks: [{
                name: "板级内存图编辑器挂上了（# 提交按钮在）",
                ok: false,
                detail: "等不到编辑器视图（前端引导没起来？）",
            }],
        };
    }
    add("板级内存图编辑器挂上了（`#/memory-map` 有提交按钮）", true, "submit 按钮在");

    // -- ② 点提交 ⇒ 命令**真的**发出去了 -----------------------------------
    await cdp.send("Runtime.evaluate", {
        expression: `(() => {
            const btn = document.querySelector('[data-action="submit-memory-map"]');
            if (!btn) return "no-button";
            btn.click();
            return "clicked";
        })()`,
        returnByValue: true,
    });
    await sleep(400);
    const cmdsRes = await cdp.send("Runtime.evaluate", {
        expression: 'JSON.stringify(window.__bvCmds ?? [])',
        returnByValue: true,
    });
    let cmds = [];
    try {
        cmds = JSON.parse(String(cmdsRes.result?.value ?? "[]"));
    } catch {
        cmds = [];
    }
    const memoryMapCmd = cmds.find((c) => String(c.action ?? "") === "submit-memory-map");
    const cmdDescription = memoryMapCmd?.params?.description ?? null;
    add(
        "编辑器的提交按钮**真的**发出 `SetMemoryMap` 命令（载荷带描述）",
        Boolean(memoryMapCmd) && Boolean(cmdDescription) && Array.isArray(cmdDescription.regions),
        `捕到 ${cmds.length} 条命令；memory-map 那条 = ${JSON.stringify(memoryMapCmd?.action ?? null)}`
        + `，载荷 regions ${Array.isArray(cmdDescription?.regions) ? cmdDescription.regions.length : "无"}`,
    );

    // -- ③ 把**被测的那张板**经**真内核**送进去 ----------------------------
    //
    // ⚠️ 走的是**视图层发命令的那条通道**（`riscv:command`）⇒ 走的是
    // app → M-05 → 传输 → **真 WASM 内核** ⇒ 整条链都在。
    await cdp.send("Runtime.evaluate", {
        expression: `window.dispatchEvent(new CustomEvent("riscv:command", {
            detail: {
                action: "submit-memory-map",
                requestId: "bv-mm-board",
                viewId: "memory-map",
                params: { description: ${JSON.stringify(BV_BOARD)} },
            },
        }))`,
        returnByValue: true,
    });

    // ⚠️ **必须等这条命令真的往返一趟**再发下一条（`reset`）。
    //
    // ⚠️ **为什么**：命令是**异步**的（Worker → WASM → 回事件）⇒ 若不等，
    // `reset` 可能**先**到内核 ⇒ 那时装载用的是**旧**的图（用户还没配的
    // 那张）⇒ 而 `set-memory-map` **后**到 ⇒ 用户的板变成「待生效」⇒
    // 内存映射表**永远不变** ⇒ 症状是「明明配了板却没反应」。
    //
    // ⚠️ **判据**：此刻镜像**已经装载过**（osz 段装过）⇒ 宿主对新板**只
    // 记下、不重建** ⇒ 回的事件里 `pending = true` ⇒ 编辑器上会出现
    // **「待生效」**横幅 ⇒ 那条横幅**出现**就证明命令**往返完成**了。
    const pendingDeadline = Date.now() + opts.timeoutMs;
    let pendingSeen = false;
    while (Date.now() < pendingDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: `(() => {
                const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
                return Boolean(el);
            })()`,
            returnByValue: true,
        });
        if (res.result?.value === true) {
            pendingSeen = true;
            break;
        }
        await sleep(200);
    }
    add(
        "命令**往返完成**（内核回了「待生效」⇒ 用户的板已记下、正在跑的访客不受影响）",
        pendingSeen,
        pendingSeen
            ? "看到了「待生效」横幅"
            : "等不到「待生效」横幅 ⇒ 命令可能没发出去或内核没回",
    );
    await sleep(300);

    // -- ④ 重新装载工程 ⇒ 用户的板**跨装载保持** ⇒ 新图生效 ----------------
    //
    // ⚠️ 用 **demo 工程**重载（不是 osz 那个）：osz 的镜像装不进这张板 ⇒
    // 装它会失败 ⇒ 用户的板就永远生效不了。
    await cdp.send("Runtime.evaluate", {
        expression: `window.dispatchEvent(new CustomEvent("riscv:command", {
            detail: { action: "reset", requestId: "bv-mm-reset", viewId: "memory-map", params: null },
        }))`,
        returnByValue: true,
    });

    // -- ⑤ 等产品视图的内存映射表变成新图 ----------------------------------
    //
    // ⚠️ 切到 `#/product` 之前**先等**：视图是懒挂载的（面板在切过去时才
    // 建出来）。
    const mapDeadline = Date.now() + opts.timeoutMs;
    let rows = [];
    while (Date.now() < mapDeadline) {
        await cdp.send("Runtime.evaluate", {
            expression: 'location.hash = "#/product";',
            returnByValue: true,
        });
        const res = await cdp.send("Runtime.evaluate", {
            expression: BV_MEMORY_MAP_PROBE,
            returnByValue: true,
        });
        const p = res.result?.value ?? null;
        rows = Array.isArray(p?.rows) ? p.rows : [];
        const hasNewRam = rows.some(
            (r) => String(r.base) === String(BV_NEW_RAM_BASE) && String(r.kind) === "ram",
        );
        if (hasNewRam && rows.length === BV_BOARD.regions.length) break;
        await sleep(250);
    }

    const newRamRow = rows.find((r) => String(r.base) === String(BV_NEW_RAM_BASE)) ?? null;
    add(
        "内存映射表变成了新图（新 RAM 段在表里，基址与大小逐条一致）",
        Boolean(newRamRow)
        && String(newRamRow.kind) === "ram"
        && String(newRamRow.size) === String(BV_NEW_RAM_SIZE)
        && rows.length === BV_BOARD.regions.length,
        `表里 ${rows.length} 行：${JSON.stringify(rows)}`,
    );

    // ⚠️ **「待生效」横幅应当消失** —— 用户的板**已经**变成当前生效的图
    // ⇒ `pending` 是**推导量**（`user_board ≠ self.platform`）⇒ 两者相等
    // ⇒ 推导出来就是「假」⇒ 横幅**必须**没了。
    //
    // ⚠️ **判据是横幅的**有无**，不是文案** ⇒ 不靠 message 文本。
    const pendingAfter = await cdp.send("Runtime.evaluate", {
        expression: `(() => {
            const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
            return Boolean(el);
        })()`,
        returnByValue: true,
    });
    add(
        "装载后「待生效」横幅**消失**（用户的板已经变成当前生效的图）",
        pendingAfter.result?.value === false,
        pendingAfter.result?.value === false
            ? "横幅没了 ⇒ user_board == 当前生效的图"
            : "横幅还在 ⇒ 用户的板没真正生效",
    );

    // -- ⑥ 新 RAM 段：写进去 ⇒ 读得回来 ------------------------------------
    const writeRes = await cdp.send("Runtime.evaluate", {
        expression: `window.riscv_kernel.writeMemory(${BV_NEW_RAM_BASE}, "Word", ${BV_NEW_RAM_VALUE})`,
        awaitPromise: true,
        returnByValue: true,
    }, 30000);
    const writeOk = !writeRes.exceptionDetails;
    add(
        "新 RAM 段**写**得进去",
        writeOk,
        writeOk
            ? `写入 ${BV_NEW_RAM_VALUE} @ 0x${BV_NEW_RAM_BASE.toString(16)}`
            : `写失败：${String(writeRes.exceptionDetails?.exception?.description
                ?? writeRes.exceptionDetails?.text ?? "?").slice(0, 200)}`,
    );

    const readRes = await cdp.send("Runtime.evaluate", {
        expression: `window.riscv_kernel.readMemory(${BV_NEW_RAM_BASE}, "Word")`,
        awaitPromise: true,
        returnByValue: true,
    }, 30000);
    const readValue = readRes.result?.value?.value ?? null;
    add(
        "新 RAM 段**读**得回来（写-读往返相等）",
        readValue === BV_NEW_RAM_VALUE,
        `读回 ${readValue === null ? "null" : `0x${Number(readValue).toString(16)}`}`
        + `（期望 0x${BV_NEW_RAM_VALUE.toString(16)}）`,
    );

    // -- ⑦ 写新 RAM **之外** ⇒ 失败（size 真的是那个 size） -------------------
    const beyond = BV_NEW_RAM_BASE + BV_NEW_RAM_SIZE;
    const beyondRes = await cdp.send("Runtime.evaluate", {
        expression: `window.riscv_kernel.writeMemory(${beyond}, "Word", ${BV_NEW_RAM_VALUE})`,
        awaitPromise: true,
        returnByValue: true,
    }, 30000);
    const beyondFailed = Boolean(beyondRes.exceptionDetails);
    add(
        "写新 RAM **之外** ⇒ 失败（那段的大小真的是那个大小）",
        beyondFailed,
        beyondFailed
            ? `写 0x${beyond.toString(16)}（超出段尾）被拒`
            : `写 0x${beyond.toString(16)} 居然成功了 ⇒ size 没生效`,
    );

    return { skipped: false, checks };
}

/**
 * 板档案（STAGE-15 · 验收 **T-3**）的探针：**只读数据属性**，不靠文案。
 *
 * ⚠️ 持久化状态从 `data-mmap-archive-status` 读 ⇒ 前端若把它写成
 * `"unsupported"`（本机没有 IndexedDB）这一段就会红 ⇒ **不静默**。
 */
const BV_BOARD_ARCHIVE_PROBE = `(() => {
  const rows = Array.from(
    document.querySelectorAll('[data-view-body="memory-map"] [data-mmap-board-id]'),
  );
  const status = document.querySelector('[data-view-body="memory-map"] [data-mmap-archive-status]');
  const saveBtn = document.querySelector('[data-action="save-memory-map"]');
  const validated = document.querySelector('[data-view-body="memory-map"] [data-mmap-validated]');
  return {
    rowCount: rows.length,
    rows: rows.map((el) => ({
      id: String(el.getAttribute("data-mmap-board-id") || ""),
      name: String(el.getAttribute("data-mmap-board-name") || ""),
    })),
    status: status ? String(status.getAttribute("data-mmap-archive-status") || "") : "",
    savePresent: Boolean(saveBtn),
    saveDisabled: saveBtn ? Boolean(saveBtn.disabled) : null,
    validated: validated ? String(validated.getAttribute("data-mmap-validated") || "") : "",
  };
})()`;

/**
 * **规范 JSON**（键排序）—— 用来比较「提交出去的描述」与「存下来的描述」。
 *
 * ⚠️ 为什么要排序：JS 对象的键序取决于**构造顺序** ⇒ 直接
 * `JSON.stringify` 比较两个「内容相同但键序不同」的对象会判不等。
 */
const BV_CANONICAL_JSON = `(value) => {
  const canon = (v) => {
    if (Array.isArray(v)) return v.map(canon);
    if (v && typeof v === "object") {
      const out = {};
      for (const k of Object.keys(v).sort()) out[k] = canon(v[k]);
      return out;
    }
    return v;
  };
  return JSON.stringify(canon(value));
}`;

/**
 * 板级配置**持久化**那一段（v0.30 批 · STAGE-15 · 验收 **T-3**）。
 *
 * ⚠️ **这一段是本批唯一的「真 IndexedDB」覆盖**：默认门禁（node:test）只
 * 覆盖纯逻辑层与内存 backend ⇒ IndexedDB 适配层**只能**在这里被验到
 * （前端零依赖 ⇒ 不能引 `fake-indexeddb`）。
 *
 * 四步：
 *   ① 提交一张板 ⇒ 等内核**往返完成**（「待生效」横幅出现）⇒ 「保存当前板」
 *      **解禁**（证明「已被内核接受」这个**推导量**成立）；
 *   ② 点「保存当前板」⇒ 板档案里**多一块**（按 `data-mmap-board-id` 判）；
 *   ③ **刷新页面**（`Page.reload`）⇒ 前端重新引导 ⇒ 板档案**从 IndexedDB
 *      读回来**；
 *   ④ 板**还在**，且存下来的描述与提交出去的那份**逐字段相同**。
 *
 * ⚠️ **必须排在板级内存图段之后**：用户的板是**黏的**（宿主里那个字段存的是
 * 用户意图）⇒ 放前面会把 osz 段要用的那张板换掉。
 *
 * ⚠️ **第 ④ 步直接读 IndexedDB**，不读 DOM ⇒ 验的是「**真的存下来了**」，
 * 不是「界面上画出来了」⇒ 两者不是一回事。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runBoardArchiveSection(cdp, opts) {
    /** @type {{name: string, ok: boolean, detail: string}[]} */
    const checks = [];
    const add = (name, ok, detail) => checks.push({ name, ok: Boolean(ok), detail: String(detail) });

    // -- ① 切到编辑器视图，等它挂上 ----------------------------------------
    await cdp.send("Runtime.evaluate", {
        expression: 'location.hash = "#/memory-map";',
        returnByValue: true,
    });
    await cdp.send("Runtime.evaluate", {
        expression: BV_CAPTURE_COMMANDS_EXPRESSION,
        returnByValue: true,
    });

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: BV_BOARD_ARCHIVE_PROBE,
            returnByValue: true,
        });
        probe = res.result?.value ?? null;
        if (probe?.savePresent) break;
        await sleep(250);
    }
    if (!probe?.savePresent) {
        return {
            skipped: false,
            checks: [{
                name: "板档案区块挂上了（`#/memory-map` 有「保存当前板」按钮）",
                ok: false,
                detail: "等不到板档案区块（前端引导没起来？）",
            }],
        };
    }
    add("板档案区块挂上了（`#/memory-map` 有「保存当前板」按钮）", true, "保存按钮在");
    add(
        "持久化状态是「可用」（不是 `unsupported` ⇒ 本机有 IndexedDB）",
        probe.status === "ok",
        `data-mmap-archive-status = ${JSON.stringify(probe.status)}`,
    );

    // -- ② 提交一张板 ⇒ 等内核**往返完成** --------------------------------
    //
    // ⚠️ **落库门槛**：只有**内核接受过**的描述才允许落库（STAGE-15 裁决 B）
    // ⇒ 「保存当前板」在草稿**没被接受过**时必须是**禁用**的。
    //
    // ⚠️ **判据**：此刻镜像**已经装载过**（板级内存图段装过）⇒ 宿主对新板
    // **只记下、不重建** ⇒ 回的事件里 `pending = true` ⇒ 「待生效」横幅出现
    // ⇒ 那条横幅**出现**就证明命令**往返完成**了（也证明内核**接受**了）。
    await cdp.send("Runtime.evaluate", {
        expression: `(() => {
            const btn = document.querySelector('[data-action="submit-memory-map"]');
            if (!btn) return "no-button";
            btn.click();
            return "clicked";
        })()`,
        returnByValue: true,
    });

    const pendingDeadline = Date.now() + opts.timeoutMs;
    let pendingSeen = false;
    while (Date.now() < pendingDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: `(() => {
                const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
                return Boolean(el);
            })()`,
            returnByValue: true,
        });
        if (res.result?.value === true) {
            pendingSeen = true;
            break;
        }
        await sleep(200);
    }
    add(
        "命令**往返完成**（内核回了「待生效」⇒ 这份描述**已被内核接受**）",
        pendingSeen,
        pendingSeen ? "看到了「待生效」横幅" : "等不到「待生效」横幅 ⇒ 命令没发出去或内核没回",
    );

    // ⚠️ 「保存当前板」此刻必须**解禁** ⇒ 证明「已被内核接受」这个推导量成立。
    let afterSubmit = null;
    const saveDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < saveDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: BV_BOARD_ARCHIVE_PROBE,
            returnByValue: true,
        });
        afterSubmit = res.result?.value ?? null;
        if (afterSubmit && afterSubmit.saveDisabled === false) break;
        await sleep(200);
    }
    add(
        "草稿**已被内核接受** ⇒ 「保存当前板」**解禁**（落库门槛成立）",
        afterSubmit?.saveDisabled === false,
        `saveDisabled = ${JSON.stringify(afterSubmit?.saveDisabled ?? null)}`,
    );

    // -- ③ 点「保存当前板」⇒ 档案里多一块 ----------------------------------
    const submittedRes = await cdp.send("Runtime.evaluate", {
        expression: `(() => {
            const cmds = Array.isArray(window.__bvCmds) ? window.__bvCmds : [];
            const cmd = cmds.find((c) => String(c.action || "") === "submit-memory-map");
            if (!cmd) return "";
            const canon = ${BV_CANONICAL_JSON};
            return canon(cmd.params && cmd.params.description ? cmd.params.description : null);
        })()`,
        returnByValue: true,
    });
    const submittedCanonical = String(submittedRes.result?.value ?? "");
    add(
        "捕到**提交出去**的那份描述（后面拿它跟存下来的那份逐字段比）",
        submittedCanonical.length > 0,
        submittedCanonical ? `规范 JSON ${submittedCanonical.length} 字节` : "没捕到提交命令",
    );

    await cdp.send("Runtime.evaluate", {
        expression: `(() => {
            const btn = document.querySelector('[data-action="save-memory-map"]');
            if (!btn) return "no-button";
            btn.click();
            return "clicked";
        })()`,
        returnByValue: true,
    });

    let saved = null;
    const savedDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < savedDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: BV_BOARD_ARCHIVE_PROBE,
            returnByValue: true,
        });
        saved = res.result?.value ?? null;
        if (saved && saved.rowCount > 0) break;
        await sleep(200);
    }
    const savedRows = Array.isArray(saved?.rows) ? saved.rows : [];
    add(
        "点「保存当前板」⇒ 板档案里**多了一块**",
        savedRows.length > 0,
        `档案 ${savedRows.length} 行：${JSON.stringify(savedRows.map((r) => r.id))}`,
    );

    // -- ④ 刷新页面 ⇒ 板**还在**（逐字段相同） ------------------------------
    //
    // ⚠️ **直接读 IndexedDB**，不读 DOM ⇒ 验的是「**真的存下来了**」。
    const reloadLoaded = new Promise((resolve) => {
        const off = cdp.on("Page.loadEventFired", () => {
            off();
            resolve(true);
        });
    });
    await cdp.send("Page.reload", { ignoreCache: false });
    await Promise.race([reloadLoaded, sleep(opts.timeoutMs)]);

    // ⚠️ 刷新后前端要**重新引导**（Worker / WASM 内核也重启了）⇒ 等它起来。
    const bootDeadline = Date.now() + opts.timeoutMs;
    let booted = false;
    while (Date.now() < bootDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: PROBE_EXPRESSION,
            returnByValue: true,
        });
        const p = res.result?.value ?? null;
        const landing = p ? landingChecks(p) : [];
        if (landing.length > 0 && landing.every((c) => c.ok)) {
            booted = true;
            break;
        }
        await sleep(250);
    }
    add(
        "刷新后前端**重新引导**成功（落地页检查全过）",
        booted,
        booted ? "落地页检查全过" : "刷新后前端没起来",
    );

    const storedRes = await cdp.send("Runtime.evaluate", {
        expression: `new Promise((resolve) => {
            const canon = ${BV_CANONICAL_JSON};
            let req;
            try {
                req = indexedDB.open("riscv-sim-board-store");
            } catch (err) {
                resolve(JSON.stringify({ error: String(err) }));
                return;
            }
            req.onerror = () => resolve(JSON.stringify({ error: "open failed" }));
            req.onsuccess = () => {
                const db = req.result;
                try {
                    const tx = db.transaction("boards", "readonly");
                    const store = tx.objectStore("boards");
                    const all = store.getAll();
                    all.onsuccess = () => {
                        const entries = Array.isArray(all.result) ? all.result : [];
                        resolve(JSON.stringify({
                            count: entries.length,
                            ids: entries.map((e) => String((e && e.id) || "")),
                            // ⚠️ **比的是 e.description**（描述本身），**不是**整条
                            // entry（entry 是 id 加 description 的**包装**）⇒
                            // 拿包装去比描述 ⇒ 永远不等 ⇒ **假红**。
                            entries: entries.map((e) => ({
                                id: String((e && e.id) || ""),
                                canonical: canon((e && e.description) ? e.description : null),
                            })),
                        }));
                    };
                    all.onerror = () => resolve(JSON.stringify({ error: "getAll failed" }));
                } catch (err) {
                    resolve(JSON.stringify({ error: String(err) }));
                }
            };
        })`,
        awaitPromise: true,
        returnByValue: true,
    });
    let stored = null;
    try {
        stored = JSON.parse(String(storedRes.result?.value ?? "{}"));
    } catch {
        stored = null;
    }
    const storedIds = Array.isArray(stored?.ids) ? stored.ids : [];
    add(
        "刷新后 IndexedDB 里**那块板还在**",
        storedIds.length > 0 && storedIds.length === savedRows.length,
        `库里 ${storedIds.length} 条：${JSON.stringify(storedIds)}`,
    );

    // ⚠️ **逐字段相同**：存下来的描述 vs 提交出去的那份 ⇒ 用**规范 JSON**比。
    //
    // ⚠️ **按 `id` 配对**（不是「任意一条相等就算过」）⇒ 多块板时不会拿
    // 另一块板的描述来顶替（那正是「拿别的地址的行号顶替」那类错误）。
    const storedEntries = Array.isArray(stored?.entries) ? stored.entries : [];
    const submittedId = (() => {
        try {
            return String(JSON.parse(submittedCanonical)?.id ?? "");
        } catch {
            return "";
        }
    })();
    const paired = storedEntries.find((e) => String(e?.id ?? "") === submittedId) ?? null;
    const sameField = Boolean(paired) && paired.canonical === submittedCanonical;
    add(
        "存下来的描述与**提交出去的那份逐字段相同**（规范 JSON 比，按 id 配对）",
        sameField,
        sameField
            ? `板「${submittedId}」的两份规范 JSON 完全一致`
            : `提交 id = ${JSON.stringify(submittedId)}，规范 JSON ${submittedCanonical.slice(0, 100)}；`
              + `库存 = ${JSON.stringify(storedEntries.map((e) => ({ id: e?.id, c: String(e?.canonical ?? "").slice(0, 100) })))}`,
    );

    return { skipped: false, checks };
}

/**
 * 真实 RTOS 那一段：装载仓外的 ELF → 跑起来 → 终端里看得见 console。
 *
 * ⚠️ **ELF 不在就跳过，不是判负**：osz 的产物**在仓外**（不进仓库）⇒
 * 这一段在没装产物的机器上必须**跳过**（整体退出码仍是 0）。但**有**
 * 产物却跑不出 console ⇒ **判负**。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runOszSection(cdp, opts) {
    const elfPath = process.env.OSZ_ELF || OSZ_ELF_DEFAULT;
    if (!existsSync(elfPath)) {
        return {
            skipped: true,
            reason: `没有 osz ELF（${elfPath}；可用 $OSZ_ELF 指定）`,
            checks: [],
        };
    }
    console.log(`· osz ELF：${elfPath}`);

    // -- 符号表：按**名字**取调度器符号的地址（E-7）----------------------
    // ⚠️ **不许写死地址**（写死就成了恒绿的断言）。这里用仓内的零依赖
    // ELF32 解析器（`tools/elf-symtab.mjs`）读 `.symtab` ⇒ 不依赖用户
    // 机器上有没有 `nm`。
    /** @type {{name: string, value: number}|null} */
    let scheduler = null;
    let elfEntry = 0;
    try {
        const elf = loadElf(elfPath);
        elfEntry = elf.entry;
        scheduler = findFirstSymbol(elf, SCHEDULER_SYMBOL_CANDIDATES);
    } catch (err) {
        return {
            skipped: false,
            checks: [{
                name: "解析 osz 的符号表（零依赖 ELF32 解析器）",
                ok: false,
                detail: String(err?.message ?? err).slice(0, 300),
            }],
        };
    }
    if (!scheduler) {
        return {
            skipped: false,
            checks: [{
                name: "在符号表里按**名字**找到 osz 的调度器符号",
                ok: false,
                detail: `候选一个都没找到：${SCHEDULER_SYMBOL_CANDIDATES.join(" / ")}`,
            }],
        };
    }
    console.log(
        `· 调度器符号：${scheduler.name} = 0x${scheduler.value.toString(16)}`
        + `（ELF 入口 0x${elfEntry.toString(16)}）`,
    );

    // -- 等页面上的装载入口就绪（bootstrap 是异步的）----------------------
    const readyDeadline = Date.now() + opts.timeoutMs;
    let ready = false;
    while (Date.now() < readyDeadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: 'typeof window.riscv_image === "object" && window.riscv_image !== null',
            returnByValue: true,
        });
        if (res.result?.value === true) {
            ready = true;
            break;
        }
        await sleep(250);
    }
    if (!ready) {
        return {
            skipped: false,
            checks: [{
                name: "页面上的装载入口就绪（window.riscv_image）",
                ok: false,
                detail: "等不到（前端引导没起来？）",
            }],
        };
    }

    // -- 切到终端视图（终端面板要挂上，DOM 里才有 [data-terminal-out]）----
    await cdp.send("Runtime.evaluate", {
        expression: 'location.hash = "#/terminal";',
        returnByValue: true,
    });
    await sleep(300);

    const artifact = await startArtifactServer(dirname(elfPath));
    try {
        const elfUrl = `http://127.0.0.1:${artifact.port}/${basename(elfPath)}`;

        // -- 装载：按 URL 取字节 → LoadProject → 强制重订 console 订阅 -----
        const loadRes = await cdp.send("Runtime.evaluate", {
            expression: `window.riscv_image.loadFromUrl(${JSON.stringify(elfUrl)})`,
            awaitPromise: true,
            returnByValue: true,
        }, 120000);
        if (loadRes.exceptionDetails) {
            const err = loadRes.exceptionDetails.exception?.description
                ?? loadRes.exceptionDetails.text
                ?? "未知错误";
            return {
                skipped: false,
                checks: [{
                    name: "装载 osz 的 ELF（按 URL 取字节 → LoadProject）",
                    ok: false,
                    detail: String(err).slice(0, 300),
                }],
            };
        }
        const byteLength = Number(loadRes.result?.value?.byteLength ?? 0);
        console.log(`· 已装载 osz：${byteLength} 字节`);

        // =================================================================
        // E-7（K-5）· 在 osz 的**调度器符号**上下断点
        //
        // ⚠️⚠️ **断点必须在「跑之前」下**：osz 的调度器（`first_schedule`
        // 之类）在 **shell 提示符出现之前**就执行过了 ⇒ 先跑到提示符再下
        // 断点，那个符号**永远命中不了** ⇒ 这一段**必须**在「跑到提示符」
        // **之前**跑。
        // =================================================================
        const breakpoint = await runBreakpointProbe(cdp, opts, scheduler);

        // -- 跑到提示符（一条一条 Continue，最多 60 秒）----------------------
        const runRes = await cdp.send("Runtime.evaluate", {
            expression:
                `window.riscv_image.runUntilConsole("OSZ$ ", { timeoutMs: ${OSZ_RUN_TIMEOUT_MS} })`,
            awaitPromise: true,
            returnByValue: true,
        }, OSZ_RUN_TIMEOUT_MS + 30000);
        if (runRes.exceptionDetails) {
            const err = runRes.exceptionDetails.exception?.description
                ?? runRes.exceptionDetails.text
                ?? "未知错误";
            return {
                skipped: false,
                checks: [...breakpoint.checks, {
                    name: "跑到 osz 的 shell 提示符",
                    ok: false,
                    detail: String(err).slice(0, 300),
                }],
            };
        }
        const batches = Number(runRes.result?.value?.batches ?? 0);
        console.log(`· osz 跑了 ${batches} 批 Continue`);

        // =================================================================
        // E-6（K-4）· 反汇编面板出现 osz 的**真** file 与 line
        // =================================================================
        const disasm = await runDisasmSourceProbe(cdp, opts, scheduler, elfEntry);

        // =================================================================
        // E-8（K-6）· 送一个字节 ⇒ guest 的 UART LSR 的 bit0 置位
        // =================================================================
        const uart = await runUartRxProbe(cdp, opts);

        // -- 断言**终端视图里**出现了那两段字面量 ---------------------------
        const deadline = Date.now() + opts.timeoutMs;
        let consoleChecks = [];
        let probe = null;
        while (Date.now() < deadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            consoleChecks = probe ? oszConsoleChecks(probe) : [];
            if (consoleChecks.length > 0 && consoleChecks.every((c) => c.ok)) break;
            await sleep(250);
        }
        return {
            skipped: false,
            checks: [
                ...breakpoint.checks,
                ...disasm.checks,
                ...uart.checks,
                ...consoleChecks,
            ],
        };
    } finally {
        artifact.close();
    }
}

/**
 * 页面里取一份探针快照（DOM 现状）。
 *
 * @param {object} cdp
 * @returns {Promise<object|null>}
 */
async function pageProbe(cdp) {
    const res = await cdp.send("Runtime.evaluate", {
        expression: PROBE_EXPRESSION,
        returnByValue: true,
    });
    return res.result?.value ?? null;
}

/**
 * E-7（K-5）· 在调度器符号上下断点 ⇒ 命中时 PC 落在那个符号的地址。
 *
 * ⚠️ **必须用符号名取地址**（写死就成恒绿）——地址是**外面**用零依赖
 * ELF32 解析器按名字查出来的。
 *
 * ⚠️ **断点必须在跑之前下**：osz 的调度器在 shell 提示符之前就跑过了。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @param {{name: string, value: number}} scheduler
 * @returns {Promise<{checks: object[]}>}
 */
async function runBreakpointProbe(cdp, opts, scheduler) {
    /** @type {object[]} */
    const checks = [];
    const expectedHex = `0x${scheduler.value.toString(16).padStart(8, "0")}`;

    checks.push({
        name: "在符号表里按**名字**找到 osz 的调度器符号（不许写死地址）",
        ok: true,
        detail: `${scheduler.name} = 0x${scheduler.value.toString(16)}`
            + `（候选：${SCHEDULER_SYMBOL_CANDIDATES.join(" / ")}）`,
    });
    checks.push({
        name: "断点地址对齐（RISC-V 压缩指令 2 字节对齐）",
        ok: scheduler.value % 2 === 0,
        detail: `addr=0x${scheduler.value.toString(16)}`,
    });

    // -- 下断点 ------------------------------------------------------------
    const setRes = await cdp.send("Runtime.evaluate", {
        expression: `window.riscv_kernel.setBreakpoint(${scheduler.value}, 1)`,
        awaitPromise: true,
        returnByValue: true,
    }, 30000);
    if (setRes.exceptionDetails) {
        const err = setRes.exceptionDetails.exception?.description
            ?? setRes.exceptionDetails.text
            ?? "未知错误";
        checks.push({
            name: "在调度器符号上下断点",
            ok: false,
            detail: String(err).slice(0, 300),
        });
        return { checks };
    }
    checks.push({
        name: "在调度器符号上下断点",
        ok: true,
        detail: `addr=0x${scheduler.value.toString(16)} id=1`,
    });

    // -- 跑到命中（一条一条 Continue）---------------------------------------
    // ⚠️ 每跑一批都要看一次「内核报告的停在哪」——那是**权威**的 PC；
    // DOM 里的 PC 是寄存器流推到视图的**投影**（要晚一点）。
    const deadline = Date.now() + opts.timeoutMs;
    let halt = null;
    let rounds = 0;
    while (Date.now() < deadline && rounds < 400) {
        const runRes = await cdp.send("Runtime.evaluate", {
            expression: "window.riscv_kernel.continueRun()",
            awaitPromise: true,
            returnByValue: true,
        }, 60000);
        if (runRes.exceptionDetails) break;
        rounds += 1;
        const haltRes = await cdp.send("Runtime.evaluate", {
            expression: "window.riscv_kernel.lastHalt()",
            returnByValue: true,
        });
        halt = haltRes.result?.value ?? null;
        if (halt && Number(halt.pc) === scheduler.value) break;
    }
    console.log(`· 断点跑了 ${rounds} 批 Continue`);

    checks.push({
        name: "断点命中时内核报告的 PC 落在那个符号的地址上",
        ok: Boolean(halt) && Number(halt.pc) === scheduler.value,
        detail: `halt=${JSON.stringify(halt)} expected="${expectedHex}"`,
    });
    checks.push({
        name: "停下原因是「断点」（不是陷阱 / 跑完）",
        ok: Boolean(halt) && halt.reason === "Breakpoint",
        detail: `reason=${halt?.reason ?? "(无)"}`,
    });

    // -- DOM 里的 PC（用户看到的那一行）也要落在同一个地址 -----------------
    let domPc = "";
    const settleDeadline = Date.now() + PC_SETTLE_TIMEOUT_MS;
    while (Date.now() < settleDeadline) {
        const probe = await pageProbe(cdp);
        domPc = String(probe?.pc ?? "").trim();
        if (domPc === expectedHex) break;
        await sleep(150);
    }
    checks.push({
        name: "视图里的 PC 也落在那个符号的地址上（寄存器流推到视图）",
        ok: domPc === expectedHex,
        detail: `pc="${domPc}" expected="${expectedHex}"`,
    });

    // -- 撤掉断点（后面的 E-6 / E-8 不该再被它打断）------------------------
    await cdp.send("Runtime.evaluate", {
        expression: "window.riscv_kernel.clearBreakpoint(1)",
        awaitPromise: true,
        returnByValue: true,
    }, 30000);

    return { checks };
}

/**
 * E-6（K-4）· 反汇编面板出现 osz 的**真** file 与 line。
 *
 * ⚠️ **断言特征，不断言具体路径**（路径会随 osz 的构建目录变）：
 * `file` 必须**含 `.c`**，`line` 必须是**大于 0 的整数**。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @param {{name: string, value: number}} scheduler
 * @param {number} elfEntry — ELF 的入口地址（兜底用的反汇编起点）
 * @returns {Promise<{checks: object[]}>}
 */
async function runDisasmSourceProbe(cdp, opts, scheduler, elfEntry) {
    /** @type {object[]} */
    const checks = [];
    /** @type {object[]} */
    let rows = [];

    /**
     * 读一次面板上的反汇编行。
     * @returns {Promise<object[]>}
     */
    async function readRows() {
        const res = await cdp.send("Runtime.evaluate", {
            expression: DISASM_ROWS_EXPRESSION,
            returnByValue: true,
        });
        const value = res.result?.value;
        return Array.isArray(value) ? value : [];
    }

    /**
     * 让面板反汇编一段（**生产路径**：M-02 发信号 → `app.js` 发命令）。
     * @param {number} addr
     */
    async function requestDisasm(addr) {
        await cdp.send("Runtime.evaluate", {
            expression: `window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "disasm", params: { addr: ${addr}, count: 16 } } }));`,
            returnByValue: true,
        });
    }

    // -- 第 1 轮：**按钮**（用当前 PC 当起点）------------------------------
    await cdp.send("Runtime.evaluate", {
        expression: 'location.hash = "#/debug";',
        returnByValue: true,
    });
    await sleep(300);
    await cdp.send("Runtime.evaluate", {
        expression: `document.querySelector('[data-action="disasm"]')?.click();`,
        returnByValue: true,
    });
    let deadline = Date.now() + opts.timeoutMs;
    while (Date.now() < deadline) {
        rows = await readRows();
        if (rows.length > 0) break;
        await sleep(200);
    }
    const pcRows = rows.slice();

    // -- 第 2 轮（兜底）：PC 那一段没有行表时，改从**已知有源码的符号**
    //    （调度器函数）开始反汇编 ---------------------------------------------
    if (!pcRows.some((r) => r.file !== "" && r.line !== "")) {
        await requestDisasm(scheduler.value);
        deadline = Date.now() + opts.timeoutMs;
        while (Date.now() < deadline) {
            rows = await readRows();
            if (rows.some((r) => r.file !== "" && r.line !== "")) break;
            await sleep(200);
        }
    }

    const all = [...pcRows, ...rows];
    const withSource = all.filter((r) => r.file !== "" && r.line !== "");
    const good = withSource.find(
        (r) => r.file.includes(".c") && Number.isInteger(Number(r.line)) && Number(r.line) > 0,
    );

    checks.push({
        name: "反汇编面板出现带**真源码路径**的条目（file 含 .c）",
        ok: Boolean(good),
        detail: good
            ? `file="${good.file}" addr=${good.addr} line=${good.line}`
            : `面板上 ${all.length} 条，没有一条带 .c 的源码路径：`
                + JSON.stringify(all.slice(0, 3)),
    });
    checks.push({
        name: "该条目的行号是**大于 0 的整数**（不是 0 / null）",
        ok: Boolean(good)
            && Number.isInteger(Number(good.line))
            && Number(good.line) > 0,
        detail: good ? `line=${good.line}` : "没有带行号的条目",
    });

    return { checks };
}

/**
 * E-8（K-6）· 送一个字节 ⇒ guest 的 UART **RX 就绪**（LSR 的 bit0）。
 *
 * ⚠️ **断言要具体**：读 guest 的 UART0 的 LSR（`0x1000_0005`）⇒ 断言
 * `value & 0x1 === 1`。
 *
 * ⚠️ **发送走终端输入框**（生产路径）——这同时是 K-1 的真机证据。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{checks: object[]}>}
 */
async function runUartRxProbe(cdp, opts) {
    /** @type {object[]} */
    const checks = [];

    // -- 送一个字节（"A" = 0x41）-------------------------------------------
    const sendRes = await cdp.send("Runtime.evaluate", {
        expression: sendTerminalTextExpression("A"),
        awaitPromise: true,
        returnByValue: true,
    });
    const sent = sendRes.result?.value ?? null;
    checks.push({
        name: "在终端输入框敲一个字节并点「发送」",
        ok: Boolean(sent?.ok),
        detail: JSON.stringify(sent),
    });

    // -- 读 guest 的 LSR ---------------------------------------------------
    // ⚠️ 读的是**外设寄存器**（UART0 是内存映射的）⇒ 用契约的
    // `ReadMemory` 读 `0x1000_0005`。
    let lsr = null;
    const deadline = Date.now() + opts.timeoutMs;
    while (Date.now() < deadline) {
        const res = await cdp.send("Runtime.evaluate", {
            expression: `window.riscv_kernel.readMemory(${UART_LSR_ADDR}, "Byte")`,
            awaitPromise: true,
            returnByValue: true,
        });
        if (res.exceptionDetails) break;
        const value = res.result?.value ?? null;
        if (value && typeof value.value === "number") {
            lsr = value.value >>> 0;
            // ⚠️ **RX_VALID 置位**才算数 ⇒ 没置位就再读一次（读 LSR 本身
            // 不会清掉它，但多读一次更稳）。
            if ((lsr & LSR_RX_VALID) === 1) break;
        }
        await sleep(150);
    }

    checks.push({
        name: `读得到 guest 的 UART LSR（0x${UART_LSR_ADDR.toString(16)}）`,
        ok: lsr !== null,
        detail: lsr === null ? "读不到（ReadMemory 失败？）" : `LSR=0x${lsr.toString(16)}`,
    });
    checks.push({
        name: "LSR 的 bit0（RX_VALID）置位（字节进了 guest 的接收 FIFO）",
        ok: lsr !== null && (lsr & LSR_RX_VALID) === 1,
        detail: lsr === null
            ? "读不到 LSR"
            : `LSR=0x${lsr.toString(16)} (bit0=${lsr & 1})`,
    });

    return { checks };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

/**
 * Run the whole verification.
 *
 * @returns {Promise<number>} the process exit code
 */
async function main() {
    const opts = parseArgs(process.argv.slice(2));

    // The front end now mounts the **real** WASM kernel, so the artifact
    // is a hard precondition. Missing it is *not* a verification failure
    // (the wasm build is a separate, deliberate step) — it is the same
    // class as "no Chrome".
    const wasmGlue = join(frontDir, "assets/pkg/riscv_sim_kernel.js");
    const wasmBin = join(frontDir, "assets/pkg/riscv_sim_kernel_bg.wasm");
    if (!existsSync(wasmGlue) || !existsSync(wasmBin)) {
        console.error([
            "error: 找不到 WASM 内核产物。",
            `  期望：${wasmGlue}`,
            `  期望：${wasmBin}`,
            "  先构建：tools/build-wasm.sh（或 tools/build-wasm.sh --release）",
        ].join("\n"));
        return EXIT_PRECONDITION;
    }
    console.log("· WASM 内核：front/assets/pkg/");

    const chromePath = resolveChrome();
    if (!chromePath) {
        console.error([
            "error: 找不到 Chrome / Chromium。",
            process.env.CHROME_PATH
                ? `  CHROME_PATH 指向的路径不存在：${process.env.CHROME_PATH}`
                : "  可以用 CHROME_PATH 指定浏览器路径，例如：",
            ...(process.env.CHROME_PATH
                ? []
                : ["    CHROME_PATH=\"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome\" \\",
                   "      node tools/browser-verify.mjs"]),
            "  试过的路径：CHROME_PATH（设置了就只试它）、/Applications/Google Chrome.app/…、",
            "  /Applications/Chromium.app/…、PATH 上的 google-chrome / chromium。",
        ].join("\n"));
        return EXIT_PRECONDITION;
    }
    console.log(`· 浏览器：${chromePath}`);

    const { child: server } = await ensureServer(opts);

    const userDataDir = mkdtempSync(join(tmpdir(), "riscv-browser-verify-"));
    let chrome = null;
    let cdp = null;
    try {
        chrome = await launchChrome(chromePath, opts.cdpPort, userDataDir);
        console.log(`· DevTools：${chrome.version.Browser}`);

        const targetUrl = await findPageTarget(opts.cdpPort);
        if (!targetUrl) {
            console.error("error: Chrome 没有暴露 page target");
            return EXIT_FAILED;
        }

        const socket = new WebSocket(targetUrl);
        await new Promise((resolve, reject) => {
            socket.addEventListener("open", resolve, { once: true });
            socket.addEventListener("error", () => reject(new Error("CDP WebSocket 连接失败")), { once: true });
        });
        cdp = new CdpClient(socket);

        // -- console / exception collection ----------------------------------
        /** @type {string[]} */
        const consoleErrors = [];
        cdp.on("Runtime.consoleAPICalled", (params) => {
            if (params.type !== "error") return;
            const text = (params.args ?? [])
                .map((a) => String(a.value ?? a.description ?? a.type ?? ""))
                .join(" ");
            if (IGNORED_CONSOLE.some((re) => re.test(text))) return;
            consoleErrors.push(`console.error: ${text}`);
        });
        cdp.on("Runtime.exceptionThrown", (params) => {
            const details = params.exceptionDetails ?? {};
            const text = details.exception?.description ?? details.text ?? "unknown";
            consoleErrors.push(`未捕获异常: ${text}`);
        });

        await cdp.send("Page.enable");
        await cdp.send("Runtime.enable");

        // -- navigate --------------------------------------------------------
        const loaded = new Promise((resolve) => {
            const off = cdp.on("Page.loadEventFired", () => {
                off();
                resolve(true);
            });
        });
        await cdp.send("Page.navigate", { url: opts.url });
        await Promise.race([loaded, sleep(opts.timeoutMs)]);

        // -- wait for the front end to come up --------------------------------
        let landing = [];
        let probe = null;
        const deadline = Date.now() + opts.timeoutMs;
        while (Date.now() < deadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            landing = probe ? landingChecks(probe) : [];
            if (landing.length > 0 && landing.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- switch to a view that has a component and check the render path --
        // ⚠️ **落地视图（`embedded`）的正文检查在 `landingChecks` 里**（D-33 ·
        // 裁决 C：落地视图现在**有** M-02 组件了 ⇒ 它的正文必须非空且有实质
        // 内容）。这里切到 `regmem` 是为了**渲染路径**本身（白屏回归守卫，
        // FRONTEND_Review.md #20）⇒ 后续那些需要切视图的步骤**不动**。
        await cdp.send("Runtime.evaluate", {
            expression: 'location.hash = "#/regmem";',
            returnByValue: true,
        });
        let render = [];
        const renderDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < renderDeadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            render = probe ? renderChecks(probe) : [];
            if (render.length > 0 && render.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- drive the REAL kernel -------------------------------------------
        //  Send a `step` command through the view layer's signal and check
        //  that the register view shows the value the **WASM kernel**
        //  actually computed (`addi x1, x0, 42` → x1 = 0x2a).
        await cdp.send("Runtime.evaluate", {
            expression:
                'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step" } }));',
            returnByValue: true,
        });
        let kernelExec = [];
        const execDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < execDeadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            kernelExec = probe ? kernelExecChecks(probe) : [];
            if (kernelExec.length > 0 && kernelExec.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- the source view really asks for the CURRENT PC (VB-7) ------------
        //  ⚠️ 必须在**内核已经单步过**之后：当前 PC 是那一步推进的。
        //  这条链路是「源码视图收到 `sim.pc` ⇒ 按钮的 `data-addr`」——
        //  V-B 检视的 VB-1 断的就是它（源码视图收不到 `sim.pc` ⇒ 按钮
        //  **恒问地址 0**），而 Node 门禁 243 条**全绿**（用例手工注入
        //  了 `sim.pc`）⇒ 只有真机断言抓得到。
        await cdp.send("Runtime.evaluate", {
            expression: 'location.hash = "#/source";',
            returnByValue: true,
        });
        let source = [];
        const sourceDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < sourceDeadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            source = probe ? sourceChecks(probe) : [];
            if (source.length > 0 && source.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- the button really dispatches `resolve-line` with that address ----
        //  ⚠️ **等上面全绿之后才点**：那时按钮上的 `data-addr` 已经等于当
        //  前 PC，否则点下去抓到的是 0（那是时序问题，不是被测缺陷）。
        await cdp.send("Runtime.evaluate", {
            expression: CAPTURE_RESOLVE_LINE_EXPRESSION,
            returnByValue: true,
        });
        await cdp.send("Runtime.evaluate", {
            expression: CLICK_RESOLVE_LINE_EXPRESSION,
            returnByValue: true,
        });
        let sourceCmd = null;
        const sourceCmdDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < sourceCmdDeadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            sourceCmd = probe ? sourceCommandCheck(probe) : null;
            if (sourceCmd && sourceCmd.ok) break;
            await sleep(250);
        }
        source.push(sourceCmd ?? {
            name: "点按钮真的发出 resolve-line 命令（地址等于当前 PC）",
            ok: false,
            detail: "没取到命令信号",
        });

        // -- drive the probe pipeline (D-46) ----------------------------------
        //  The demo firmware writes `GPIO0.OUT` on its 8th instruction, so
        //  the program has to run that far. The single `step` above only
        //  ran one instruction (it is the one the `x1 == 42` assertion
        //  checks), so push a `step10` on top of it — 1 + 10 instructions
        //  is past the GPIO write.
        await cdp.send("Runtime.evaluate", {
            expression:
                'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step10" } }));',
            returnByValue: true,
        });
        await cdp.send("Runtime.evaluate", {
            expression: 'location.hash = "#/waveform";',
            returnByValue: true,
        });
        let waveform = [];
        const waveDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < waveDeadline) {
            const res = await cdp.send("Runtime.evaluate", {
                expression: PROBE_EXPRESSION,
                returnByValue: true,
            });
            probe = res.result?.value ?? null;
            waveform = probe ? waveformChecks(probe) : [];
            if (waveform.length > 0 && waveform.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- the REAL RTOS (osz): load the out-of-repo ELF and SEE its console -
        // ⚠️ 这一段在**没有 osz ELF** 的机器上必须**跳过**（不是判负）——
        // osz 的产物在**仓外**（不进仓库）。
        const osz = await runOszSection(cdp, opts);

        // -- the board memory map (STAGE-14 · C-8) --------------------------
        // ⚠️ **必须排在 osz 之后**：用户的板一旦配过就**一直**保持 ⇒ 放在
        // osz 前面会把 osz 要用的那张板换掉。
        const memoryMap = await runMemoryMapSection(cdp, opts);

        // -- the board ARCHIVE (STAGE-15 · T-3) ------------------------------
        // ⚠️ **必须排在板级内存图段之后**：这一段会**提交一张新板**
        // （落库门槛要求「已被内核接受」）⇒ 放在前面会把 osz 段要用的
        // 那张板换掉。
        const boardArchive = await runBoardArchiveSection(cdp, opts);

        // -- report ------------------------------------------------------------
        console.log("");
        console.log("落地页检查：");
        for (const check of landing) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        console.log("渲染路径检查（切到 #/regmem）：");
        for (const check of render) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        console.log("真内核执行检查（发一条 step 命令）：");
        for (const check of kernelExec) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        console.log("源码视图检查（切到 #/source）：");
        for (const check of source) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        console.log("探针流检查（切到 #/waveform）：");
        for (const check of waveform) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        if (osz.skipped) {
            console.log(`真实 RTOS 检查（osz）：skipped（${osz.reason}）`);
        } else {
            console.log("真实 RTOS 检查（osz：装载仓外 ELF → 跑 → 终端里的 console）：");
            for (const check of osz.checks) {
                console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
            }
        }
        console.log("");
        console.log("板级内存图检查（配一张板 → 映射表变新图 → 新 RAM 段可写可读）：");
        for (const check of memoryMap.checks) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        console.log("");
        console.log("板档案检查（提交 → 保存 → 刷新页面 → 板还在且逐字段相同）：");
        for (const check of boardArchive.checks) {
            console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
        }
        if (consoleErrors.length > 0) {
            console.log("");
            console.log("控制台错误：");
            for (const err of consoleErrors) console.log(`  ✗ ${err}`);
        }

        const failed = [...landing, ...render, ...kernelExec, ...source, ...waveform, ...osz.checks,
            ...memoryMap.checks, ...boardArchive.checks]
            .filter((c) => !c.ok);
        const ok = failed.length === 0 && consoleErrors.length === 0;
        console.log("");
        console.log(ok
            ? "browser-verify: 通过（真机挂载 + 渲染路径 + 真内核执行 + 源码视图 + 探针流 + 板级内存图 + 板档案 + 控制台）"
            : `browser-verify: 失败（${failed.length} 项断言未通过，${consoleErrors.length} 条控制台错误）`);
        return ok ? 0 : EXIT_FAILED;
    } finally {
        cdp?.close();
        if (chrome?.child) chrome.child.kill("SIGTERM");
        if (server) server.kill("SIGTERM");
        try {
            rmSync(userDataDir, { recursive: true, force: true });
        } catch {
            // best effort
        }
    }
}

try {
    process.exit(await main());
} catch (err) {
    if (err?.precondition) {
        console.error(`error: ${err?.message ?? err}`);
        process.exit(EXIT_PRECONDITION);
    }
    console.error(`error: ${err?.message ?? err}`);
    process.exit(EXIT_FAILED);
}
