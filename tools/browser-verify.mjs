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
//     不再是 mock)；
//   - the console is clean: no `console.error`, no uncaught exception.
//
// What it does NOT verify
// -----------------------
//   - it is **not** a gate: it needs a local Chrome and a local HTTP
//     server, so it is deliberately *not* part of `tools/check-front.sh`;
//   - it does not replace the Node gate — the Node gate checks the
//     contract / module behaviour, this one checks the wiring in a
//     browser;
//   - it does not check the waveform view: the real probe pipeline
//     (M-18 / M-19) is not wired in stage 1, so the waveform view is
//     legitimately empty.
//
// Prerequisites
// -------------
//   - Node >= 22 (the script uses the global `WebSocket` and `fetch`);
//   - a local Chrome / Chromium (see `CHROME_PATH` below);
//   - python3 (to serve `front/` over HTTP — ES modules and Workers
//     need a same-origin HTTP(S) origin, `file://` is blocked);
//   - **the WASM kernel artifact** — `tools/build-wasm.sh` must have been
//     run (the front end now mounts the real kernel, not the mock).
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
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
  return {
    hasApp: Boolean(app),
    panelCount: panels.length,
    appChildren: app ? app.childElementCount : 0,
    activePanel: active ? active.getAttribute("data-view-panel") : null,
    title: heading ? String(heading.textContent || "").trim() : "",
    bodyChars: body ? String(body.innerHTML || "").trim().length : 0,
    regCells: regCells.length,
    regX1: cellText(1),
    regX2: cellText(2),
    pc: String((document.querySelector('[data-view-body="regmem"] .pc') || {}).textContent || "").trim(),
    status: String((document.getElementById("status") || {}).textContent || "").trim(),
    note: String((document.getElementById("kernel-note") || {}).textContent || "").trim(),
  };
})()`;

/**
 * Landing-page checks: the workbench really came up.
 *
 * The landing view of the embedded role is the `embedded` panel, which
 * has **no** M-02 component in the MVP (it is a placeholder) — so the
 * body of the landing view is legitimately empty. The render path is
 * therefore checked on a view that *does* have a component (see
 * `renderChecks`).
 *
 * @param {object} probe
 * @returns {{name: string, ok: boolean, detail: string}[]}
 */
function landingChecks(probe) {
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
            name: "该视图正文非空（不是白屏）",
            ok: probe.bodyChars > 0,
            detail: `bodyChars=${probe.bodyChars}`,
        },
        {
            name: "寄存器视图渲染出 32 个寄存器格",
            ok: probe.regCells === 32,
            detail: `regCells=${probe.regCells}`,
        },
    ];
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
        // The landing panel of the embedded role is a placeholder without
        // an M-02 component, so the body check runs on `regmem` — the
        // white-screen check (FRONTEND_Review.md #20).
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
        if (consoleErrors.length > 0) {
            console.log("");
            console.log("控制台错误：");
            for (const err of consoleErrors) console.log(`  ✗ ${err}`);
        }

        const failed = [...landing, ...render, ...kernelExec].filter((c) => !c.ok);
        const ok = failed.length === 0 && consoleErrors.length === 0;
        console.log("");
        console.log(ok
            ? "browser-verify: 通过（真机挂载 + 渲染路径 + 真内核执行 + 控制台）"
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
