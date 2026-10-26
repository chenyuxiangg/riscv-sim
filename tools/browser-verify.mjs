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
import { createReadStream, existsSync, mkdtempSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { loadElf, findFirstSymbol } from "./elf-symtab.mjs";
import {
    UI_IMAGE_ELF_MARKER,
    buildUiImageElf,
    checkEncoderAgainstDemoFirmware,
} from "./ui-image-fixture.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const frontDir = join(repoRoot, "front");

/** A missing precondition (no Chrome / no python3) — not a test failure. */
const EXIT_PRECONDITION = 127;
/** The verification itself failed. */
const EXIT_FAILED = 1;

// ---------------------------------------------------------------------------
// The section registry (STAGE-17 · 裁决 I / J · D-147)
// ---------------------------------------------------------------------------

/**
 * The **section registry** —— 判定聚合的**唯一**来源。
 *
 * ⚠️ **为什么不是一串手写的展开**（D-147）：v0.31 批把差量段的断言**忘了
 * 并进**聚合 ⇒ 那段的断言**永远不会红** ⇒ 门禁形同虚设 ⇒ ⇒ 现在**每一段
 * 跑完就登记自己** ⇒ 判定聚合**只**从注册表取 ⇒ ⇒ 「新加一段忘了并进」
 * 这件事**结构上不可能**（让 bug **写不出来**，而不是靠 review 抓）。
 *
 * ⚠️ **顺序即登记顺序** ⇒ 报告也按登记顺序打印（不重排可读顺序）。
 *
 * ⚠️ **skipped 的段也要登记**（空数组 + `skipped: true`）⇒ ⇒ 「没跑」与
 * 「跑了但没登记」**分得开**（前者是显式的跳过，后者才是漏）。
 *
 * @type {{name: string, title: string, skippedTitle: string|null, checks: {name: string, ok: boolean, detail: string}[], skipped: boolean, reason: string|null}[]}
 */
const SECTION_REGISTRY = [];

/**
 * The **expected** section names（裁决 J）—— **故意写死**。
 *
 * ⚠️ **这张表是本批对 D-147 的结构性护栏**：跑完断言「注册表里的段名集合
 * == 这张表」⇒ 不等就**红** ⇒ ⇒ 兜住「跑过了但没登记」与「登记了但不在
 * 期望表里」两种漏。
 *
 * ⚠️ **加段时必须同时改这张表** ⇒ 漏了就红 ⇒ ⇒ 这是**故意的摩擦**（不是
 * 缺陷）：一张写死的表把「加一段」这件事变成一次**必须显式**的动作。
 *
 * ⚠️ **段名要稳定**（前端 / 文档 / 报告都靠它）⇒ ⇒ 改名 = 改契约。
 */
const EXPECTED_SECTION_NAMES = Object.freeze([
    "landing",
    "render",
    "kernelExec",
    "source",
    "waveform",
    "loadImage",
    "osz",
    "memoryMap",
    "boardArchive",
    "boardDelta",
    "boardOrdering",
]);

/**
 * Register one section's checks（裁决 I）。
 *
 * @param {string} name — the section name (must be in {@link EXPECTED_SECTION_NAMES})
 * @param {{name: string, ok: boolean, detail: string}[]} checks
 * @param {{title?: string, skippedTitle?: string|null, skipped?: boolean, reason?: string|null}} [opts]
 * @returns {void}
 */
function registerSection(name, checks, opts = {}) {
    SECTION_REGISTRY.push({
        name: String(name),
        title: String(opts.title ?? name),
        skippedTitle: opts.skippedTitle ?? null,
        checks: Array.isArray(checks) ? checks : [],
        skipped: opts.skipped === true,
        reason: opts.reason ?? null,
    });
}

/**
 * The registry-vs-expected-table diff（裁决 J）。
 *
 * @returns {{missing: string[], extra: string[], duplicated: string[], ok: boolean}}
 */
function registryDiff() {
    const actual = SECTION_REGISTRY.map((s) => s.name);
    const missing = EXPECTED_SECTION_NAMES.filter((n) => !actual.includes(n));
    const extra = actual.filter((n) => !EXPECTED_SECTION_NAMES.includes(n));
    const duplicated = actual.filter((n, i) => actual.indexOf(n) !== i);
    return {
        missing,
        extra,
        duplicated,
        ok: missing.length === 0 && extra.length === 0 && duplicated.length === 0,
    };
}

/**
 * 跑一段 ⇒ **异常也变成一条判负的断言**（而不是把整趟跑挂掉）。
 *
 * ⚠️ **为什么**：门禁的价值在于「**跑完**」⇒ 一段抛异常时，后面的段**还没
 * 跑**⇒ ⇒ 报告就少了 ⇒ ⇒ 看不到「哪一段炸了、后面的段是什么状态」⇒ ⇒
 * ⇒ 这里把异常**如实**变成一条红 ⇒ ⇒ 后面那些段照跑。
 *
 * @param {string} name — the section name (for the message)
 * @param {() => Promise<{skipped: boolean, reason?: string, checks: object[]}>} fn
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runSection(name, fn) {
    try {
        return await fn();
    } catch (err) {
        return {
            skipped: false,
            checks: [{
                name: `这一段（${name}）抛异常了`,
                ok: false,
                detail: String(err?.message ?? err).slice(0, 300),
            }],
        };
    }
}

/**
 * The canonical JSON of a value（**键序无关**）—— 用来比「逐字段相同」。
 *
 * ⚠️ **为什么要排序**：JS 对象的键序取决于**构造顺序** ⇒ 直接
 * `JSON.stringify` 比较两个「内容相同但键序不同」的对象会判不等 ⇒ ⇒
 * 字段顺序**不进判据**（顺序是**线上形状**的事，不是**图的内容**的事）。
 *
 * ⚠️ **数组的顺序仍然进判据**（一般值如此）⇒ ⇒ ⚠️ **但内存图的段列表另有口径**
 * ⇒ ⇒ 见 {@link canonicalMemoryMap}（裁决 B：**`regions` 按基址排序**）⇒ ⇒
 * 「两条路径产出的图必须一致」这条判据在**两层**（宿主层 + 工具层）必须是
 * **同一个**口径 ⇒ ⇒ 否则两层会给出**不同的**判决。
 *
 * @param {unknown} value
 * @returns {string}
 */
function canonicalJson(value) {
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
}

/**
 * The canonical JSON of a **memory-map description**（裁决 B）。
 *
 * ⚠️ **两处**归一化：① **对象键**递归排序（键序不进判据）② **`regions`
 * 按基址排序**（段的**列表顺序**也是**线上形状**的事，不是**图的内容**的事）。
 *
 * ⚠️ **为什么必须归一化列表顺序**：整份路径保留**调用方给的顺序**，差量路径
 * 保留**基座的顺序**（命中**原地替换** / 未命中**末尾追加**）⇒ ⇒ 同一组改动
 * 在两条路径上的**列表顺序天然不同**⇒ ⇒ 不归一化 ⇒ 真机层（T-4）会因
 * 「同图不同序」**假红**。
 *
 * ⚠️ **与宿主层同一个口径** ⇒ ⇒ 宿主层的等价性用例（`crates/kernel/src/
 * kernel_host/tests_board_delta_equivalence.rs`）用的也是「键序无关 +
 * `regions` 按基址排序」⇒ ⇒ 两层的判决**必须一致** ⇒ ⇒ 否则一层绿、另一层
 * 红，两边都说不清是哪儿错了。
 *
 * @param {unknown} description — a `PlatformDescription`（线上形状）
 * @returns {string}
 */
function canonicalMemoryMap(description) {
    if (!description || typeof description !== "object") {
        return canonicalJson(description);
    }
    const copy = { ...description };
    if (Array.isArray(copy.regions)) {
        copy.regions = [...copy.regions].sort((a, b) => {
            const ab = Number(a?.base ?? 0);
            const bb = Number(b?.base ?? 0);
            return ab === bb ? 0 : ab - bb;
        });
    }
    return canonicalJson(copy);
}

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

// ---------------------------------------------------------------------------
// Page probes (STAGE-17 · 裁决 K · D-147)
// ---------------------------------------------------------------------------

/**
 * Evaluate an expression in the page **and await its value**.
 *
 * ⚠️ **这是本脚本唯一的「取回值」入口**（STAGE-17 · 裁决 K）：helper 内部
 * 已经是 `awaitPromise: true` ⇒ ⇒ 「用 Promise 却忘了 await」在**语法层面**
 * 不可能（那是 helper 的内部细节，调用点根本拿不到一个未 await 的 Promise）。
 *
 * ⚠️ **同步探针也走它**（形态统一 ⇒ **零**心智负担）：`awaitPromise` 对
 * 非 Promise 的返回值是无害的。
 *
 * ⚠️ **副作用型的调用不要走它**（比如 `location.hash = ...` / `btn.click()`）
 * —— 那些**不取回值** ⇒ 用 {@link runEffect}。
 *
 * @param {object} cdp
 * @param {string} expression — the expression to evaluate in the page
 * @param {number} [timeoutMs]
 * @returns {Promise<unknown>} the value the expression produced
 * @throws {Error} when the page threw (the message carries the description)
 */
async function awaitEval(cdp, expression, timeoutMs = 15000) {
    const res = await cdp.send("Runtime.evaluate", {
        expression,
        awaitPromise: true,
        returnByValue: true,
    }, timeoutMs);
    if (res.exceptionDetails) {
        const err = res.exceptionDetails.exception?.description
            ?? res.exceptionDetails.text
            ?? "未知错误";
        throw new Error(`页面求值失败：${String(err).slice(0, 300)}`);
    }
    return res.result?.value ?? null;
}

/**
 * Evaluate an expression in the page for its **side effect** only.
 *
 * ⚠️ **返回值被丢弃** ⇒ ⇒ 用它就等于声明「我不要这个值」⇒ ⇒ 「忘了 await
 * 一个 Promise」这件事在这条路上**也**不成立（副作用型的调用本来就不看值）。
 *
 * @param {object} cdp
 * @param {string} expression
 * @param {number} [timeoutMs]
 * @returns {Promise<void>}
 */
async function runEffect(cdp, expression, timeoutMs = 15000) {
    await cdp.send("Runtime.evaluate", { expression, returnByValue: true }, timeoutMs);
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
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await runEffect(cdp, BV_CAPTURE_COMMANDS_EXPRESSION);

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        const value = await awaitEval(cdp, BV_MEMORY_MAP_PROBE);
        probe = value ?? null;
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
    await runEffect(cdp, `(() => {
        const btn = document.querySelector('[data-action="submit-memory-map"]');
        if (!btn) return "no-button";
        btn.click();
        return "clicked";
    })()`);
    await sleep(400);
    const cmdsText = await awaitEval(cdp, 'JSON.stringify(window.__bvCmds ?? [])');
    let cmds = [];
    try {
        cmds = JSON.parse(String(cmdsText ?? "[]"));
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
    await runEffect(cdp, `window.dispatchEvent(new CustomEvent("riscv:command", {
        detail: {
            action: "submit-memory-map",
            requestId: "bv-mm-board",
            viewId: "memory-map",
            params: { description: ${JSON.stringify(BV_BOARD)} },
        },
    }))`);

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
        const pendingNow = await awaitEval(cdp, `(() => {
            const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
            return Boolean(el);
        })()`);
        if (pendingNow === true) {
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
    await runEffect(cdp, `window.dispatchEvent(new CustomEvent("riscv:command", {
        detail: { action: "reset", requestId: "bv-mm-reset", viewId: "memory-map", params: null },
    }))`);

    // -- ⑤ 等产品视图的内存映射表变成新图 ----------------------------------
    //
    // ⚠️ 切到 `#/product` 之前**先等**：视图是懒挂载的（面板在切过去时才
    // 建出来）。
    const mapDeadline = Date.now() + opts.timeoutMs;
    let rows = [];
    while (Date.now() < mapDeadline) {
        await runEffect(cdp, 'location.hash = "#/product";');
        const p = await awaitEval(cdp, BV_MEMORY_MAP_PROBE);
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
    const pendingAfter = await awaitEval(cdp, `(() => {
        const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
        return Boolean(el);
    })()`);
    add(
        "装载后「待生效」横幅**消失**（用户的板已经变成当前生效的图）",
        pendingAfter === false,
        pendingAfter === false
            ? "横幅没了 ⇒ user_board == 当前生效的图"
            : "横幅还在 ⇒ 用户的板没真正生效",
    );

    // -- ⑥ 新 RAM 段：写进去 ⇒ 读得回来 ------------------------------------
    let writeOk = true;
    let writeWhy = "";
    try {
        await awaitEval(cdp,
            `window.riscv_kernel.writeMemory(${BV_NEW_RAM_BASE}, "Word", ${BV_NEW_RAM_VALUE})`,
            30000);
    } catch (err) {
        writeOk = false;
        writeWhy = String(err?.message ?? err).slice(0, 200);
    }
    add(
        "新 RAM 段**写**得进去",
        writeOk,
        writeOk
            ? `写入 ${BV_NEW_RAM_VALUE} @ 0x${BV_NEW_RAM_BASE.toString(16)}`
            : `写失败：${writeWhy}`,
    );

    const readReply = await awaitEval(cdp,
        `window.riscv_kernel.readMemory(${BV_NEW_RAM_BASE}, "Word")`,
        30000);
    const readValue = readReply?.value ?? null;
    add(
        "新 RAM 段**读**得回来（写-读往返相等）",
        readValue === BV_NEW_RAM_VALUE,
        `读回 ${readValue === null ? "null" : `0x${Number(readValue).toString(16)}`}`
        + `（期望 0x${BV_NEW_RAM_VALUE.toString(16)}）`,
    );

    // -- ⑦ 写新 RAM **之外** ⇒ 失败（size 真的是那个 size） -------------------
    const beyond = BV_NEW_RAM_BASE + BV_NEW_RAM_SIZE;
    let beyondFailed = false;
    try {
        await awaitEval(cdp,
            `window.riscv_kernel.writeMemory(${beyond}, "Word", ${BV_NEW_RAM_VALUE})`,
            30000);
    } catch (_err) {
        beyondFailed = true;
    }
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
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await runEffect(cdp, BV_CAPTURE_COMMANDS_EXPRESSION);

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        const value = await awaitEval(cdp, BV_BOARD_ARCHIVE_PROBE);
        probe = value ?? null;
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
    await runEffect(cdp, `(() => {
        const btn = document.querySelector('[data-action="submit-memory-map"]');
        if (!btn) return "no-button";
        btn.click();
        return "clicked";
    })()`);

    const pendingDeadline = Date.now() + opts.timeoutMs;
    let pendingSeen = false;
    while (Date.now() < pendingDeadline) {
        const pendingNow = await awaitEval(cdp, `(() => {
            const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
            return Boolean(el);
        })()`);
        if (pendingNow === true) {
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
        afterSubmit = await awaitEval(cdp, BV_BOARD_ARCHIVE_PROBE);
        if (afterSubmit && afterSubmit.saveDisabled === false) break;
        await sleep(200);
    }
    add(
        "草稿**已被内核接受** ⇒ 「保存当前板」**解禁**（落库门槛成立）",
        afterSubmit?.saveDisabled === false,
        `saveDisabled = ${JSON.stringify(afterSubmit?.saveDisabled ?? null)}`,
    );

    // -- ③ 点「保存当前板」⇒ 档案里多一块 ----------------------------------
    const submittedCanonical = String(await awaitEval(cdp, `(() => {
        const cmds = Array.isArray(window.__bvCmds) ? window.__bvCmds : [];
        const cmd = cmds.find((c) => String(c.action || "") === "submit-memory-map");
        if (!cmd) return "";
        const canon = ${BV_CANONICAL_JSON};
        return canon(cmd.params && cmd.params.description ? cmd.params.description : null);
    })()`) ?? "");
    add(
        "捕到**提交出去**的那份描述（后面拿它跟存下来的那份逐字段比）",
        submittedCanonical.length > 0,
        submittedCanonical ? `规范 JSON ${submittedCanonical.length} 字节` : "没捕到提交命令",
    );

    await runEffect(cdp, `(() => {
        const btn = document.querySelector('[data-action="save-memory-map"]');
        if (!btn) return "no-button";
        btn.click();
        return "clicked";
    })()`);

    let saved = null;
    const savedDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < savedDeadline) {
        const value = await awaitEval(cdp, BV_BOARD_ARCHIVE_PROBE);
        saved = value ?? null;
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
        const p = await awaitEval(cdp, PROBE_EXPRESSION);
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

    const storedText = await awaitEval(cdp, `new Promise((resolve) => {
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
        })`);
    let stored = null;
    try {
        stored = JSON.parse(String(storedText ?? "{}"));
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

// ---------------------------------------------------------------------------
// STAGE-16 · 板级差量 / 卸载（v0.31 · T-1 / T-2 / T-3）
// ---------------------------------------------------------------------------

/**
 * 被测的第二块 RAM 段（差量改的就是它）。
 *
 * ⚠️ 只改**大小**，**元数据一字不动** ⇒ 前端才走**差量**那条路（裁决 D）。
 */
const BV_DELTA_RAM_BASE = 2147483648;
const BV_DELTA_RAM_SIZE = 4194304; // 4 MiB（原 8 MiB）

/**
 * 装一个**线上的**命令捕获器：记录每一条发进 Worker 的传输帧。
 *
 * ⚠️ **为什么必须看线上帧**：`submit-memory-map` 这条**应用层动作**在「整份
 * 替换」与「差量」两条路上**完全一样** ⇒ 只看应用层信号**分不出**走了哪条
 * ⇒ ⇒ 必须看到**契约命令名**（`SetMemoryMap` 还是 `PatchMemoryMap`）⇒
 * ⇒ 那是**线上帧的载荷**里才有的东西。
 *
 * ⚠️ **只插桩，不改前端源码**（与 `BV_CAPTURE_COMMANDS_EXPRESSION` 同一套路）。
 * ⚠️ **刷新页面后钩子会丢** ⇒ 每次刷新之后**重新装一次**。
 */
const BV_WIRE_HOOK_EXPRESSION = `(() => {
  if (!Array.isArray(window.__bvWire)) window.__bvWire = [];
  if (!Array.isArray(window.__bvIn)) window.__bvIn = [];
  if (!window.__bvWireHooked) {
    window.__bvWireHooked = true;
    const orig = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (msg) {
      try { window.__bvWire.push(JSON.stringify(msg)); } catch (e) {}
      // ⚠️ **顺手把 worker 实例留下来**（STAGE-17 · T-4 / T-6 要用）：
      //   · 读**入向**帧（内核 → 主机）——「当前生效的图」就在里面；
      //   · 往 worker **注入**一个事件帧（乱序到达那一条）。
      //
      // ⚠️ **入向的监听器是在 transport 之后注册的** ⇒ 内核的帧**先**到
      // 前端（状态推进）**再**到这里的捕获 ⇒ ⇒ 「捕获到的最后一帧」就是
      // 「前端已经处理过的最后一帧」⇒ ⇒ 两者不会错位。
      try {
        if (window.__bvWorker !== this) {
          window.__bvWorker = this;
          this.addEventListener("message", (ev) => {
            try { window.__bvIn.push(JSON.stringify(ev.data)); } catch (e) {}
          });
        }
      } catch (e) {}
      return orig.apply(this, arguments);
    };
  }
  return "hooked";
})()`;

/**
 * 注入帧的**哨兵 seq**（负数）。
 *
 * ⚠️ 注入的帧**也会**被上面的入向捕获器看见（它就挂在同一个 worker 上）
 * ⇒ ⇒ 读「内核说的生效图」时必须**跳过**它 ⇒ ⇒ 用一个**不可能**由传输层
 * 产生的 seq（传输层的 seq 是从 1 起的单调计数）把它标出来。
 */
const BV_INJECTED_FRAME_SEQ = -1;

/**
 * 工具自己发给内核的**命令帧**的 seq。
 *
 * ⚠️ **必须是正数**：契约信封的 `seq` 是 `u64` ⇒ 负数会让内核**整个信封**
 * 解析失败（`E_PROTO-08`）⇒ ⇒ 这里用一个**传输层不可能发到的**大数。
 */
const BV_TOOL_COMMAND_SEQ = 900001;

/**
 * 读**最后一条**内核推回来的 `memory.map` 事件（内核说的当前生效的图）。
 *
 * ⚠️ **注入的帧被跳过**（哨兵 seq）⇒ ⇒ 这里读到的**永远是内核说的**那份。
 *
 * ⚠️ **生效图 = 事件载荷的 `description`**（不是 DOM 上的投影）⇒ ⇒ 比的
 * 是**图本身**。
 */
const BV_LAST_MEMORY_MAP_EXPRESSION = `(() => {
  const frames = Array.isArray(window.__bvIn) ? window.__bvIn : [];
  let last = null;
  for (const raw of frames) {
    let frame = null;
    try { frame = JSON.parse(String(raw)); } catch (e) { continue; }
    const seq = Number(frame && frame.seq);
    if (!Number.isFinite(seq) || seq < 0) continue;
    const msg = (frame && frame.payload) ? frame.payload : null;
    const body = (msg && msg.payload) ? msg.payload : null;
    const ev = (body && body.Event) ? body.Event : null;
    if (!ev || !ev.MemoryMap) continue;
    last = {
      description: ev.MemoryMap.description ?? null,
      pending: ev.MemoryMap.pending === true,
      loaded: ev.MemoryMap.loaded === true,
      revision: Number(ev.MemoryMap.revision ?? 0),
    };
  }
  return last;
})()`;

/**
 * 读**最后一条**内核推回来的**错误**事件（内核拒了什么）。
 *
 * ⚠️ **读的是内核的码**（`E_PLT-13` / `E_PLT-07` …）⇒ ⇒ 不靠文案匹配。
 */
const BV_LAST_ERROR_EVENT_EXPRESSION = `(() => {
  const frames = Array.isArray(window.__bvIn) ? window.__bvIn : [];
  let last = null;
  for (const raw of frames) {
    let frame = null;
    try { frame = JSON.parse(String(raw)); } catch (e) { continue; }
    const seq = Number(frame && frame.seq);
    if (!Number.isFinite(seq) || seq < 0) continue;
    const msg = (frame && frame.payload) ? frame.payload : null;
    const body = (msg && msg.payload) ? msg.payload : null;
    const ev = (body && body.Event) ? body.Event : null;
    if (!ev || !ev.Error) continue;
    last = {
      code: String(ev.Error.code ?? ""),
      message: String(ev.Error.message ?? ""),
    };
  }
  return last;
})()`;

/**
 * 最后一次「提交板级内存图」命令带的那份描述（= 用户配的那张板）。
 *
 * ⚠️ **这是草稿转成的线上描述**（七键、没有内核补上的缺省）⇒ ⇒ 拿它做
 * **整份**提交 ⇒ ⇒ 内核会自己补缺省 ⇒ ⇒ 与差量路径的结果可比。
 */
const BV_LAST_SUBMITTED_DESCRIPTION_EXPRESSION = `(() => {
  const cmds = Array.isArray(window.__bvCmds) ? window.__bvCmds : [];
  let last = null;
  for (const c of cmds) {
    if (String((c && c.action) ? c.action : "") !== "submit-memory-map") continue;
    const params = (c && c.params) ? c.params : null;
    last = (params && params.description) ? params.description : null;
  }
  return last;
})()`;

/**
 * 板级内存图的**错误码**（内核拒了就会出现在这儿）。
 *
 * ⚠️ **读的是 `data-error-code`**（内核的码）⇒ ⇒ 不靠文案匹配。
 */
const BV_MEMORY_MAP_ERROR_PROBE = `(() => {
  const el = document.querySelector('[data-view-body="memory-map"] [data-memory-map-error]');
  return {
    present: Boolean(el),
    code: el ? String(el.getAttribute("data-error-code") ?? "") : "",
    text: el ? String(el.textContent ?? "").trim() : "",
  };
})()`;

/** 读修订号 / 待生效横幅 / 卸载按钮 / 已捕到的线上帧。 */
const BV_DELTA_PROBE = `(() => {
  const rev = document.querySelector('[data-mmap-revision]');
  const pending = document.querySelector('[data-view-body="memory-map"] [data-memory-map-pending]');
  const unload = document.querySelector('[data-action="unload-project"]');
  return {
    revision: rev ? String(rev.textContent ?? "").trim() : null,
    pending: Boolean(pending),
    unloadPresent: Boolean(unload),
    wire: Array.isArray(window.__bvWire) ? window.__bvWire.slice() : [],
  };
})()`;

/** 线上帧里有没有某个契约命令名。 */
function bvWireHas(wire, name) {
  return wire.some((s) => String(s ?? "").includes(name));
}

/** 线上帧里 `PatchMemoryMap` 那一条的载荷（取第一条）。 */
function bvWireFind(wire, name) {
  for (const s of wire) {
    const str = String(s ?? "");
    if (str.includes(name)) return str;
  }
  return "";
}

/**
 * 板级差量 / 卸载那一段。
 *
 * ⚠️ **必须排在板档案段之后**：这一段会**卸载工程**（⇒ 内核重启、用户的板
 * 不存在）⇒ 放在前面会把板档案段要用的板换掉。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runBoardDeltaSection(cdp, opts) {
    /** @type {{name: string, ok: boolean, detail: string}[]} */
    const checks = [];
    const add = (name, ok, detail) => checks.push({ name, ok: Boolean(ok), detail: String(detail) });

    // -- ① 切到编辑器视图 + 装线上钩子 --------------------------------------
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await runEffect(cdp, BV_WIRE_HOOK_EXPRESSION);
    // ⚠️ **命令捕获器也得装**（板档案段刷新过页面 ⇒ 钩子没了）⇒ ⇒ T-4 要
    // 从「提交出去的那条命令」里拿**用户配的那张板**（草稿转成的线上描述）。
    await runEffect(cdp, BV_CAPTURE_COMMANDS_EXPRESSION);

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        probe = await awaitEval(cdp, BV_DELTA_PROBE);
        if (probe?.unloadPresent) break;
        await sleep(250);
    }
    if (!probe?.unloadPresent) {
        return {
            skipped: false,
            checks: [{
                name: "「卸载工程」按钮挂上了（`#/memory-map` 有 `data-action=\"unload-project\"`）",
                ok: false,
                detail: "等不到卸载按钮（前端引导没起来？）",
            }],
        };
    }
    add(
        "「卸载工程」按钮挂上了（`#/memory-map` 有 `data-action=\"unload-project\"`）",
        true,
        "卸载按钮在",
    );

    // -- ② 点「卸载工程」⇒ 命令真的发到内核 --------------------------------
    await runEffect(cdp, `(() => {
        const btn = document.querySelector('[data-action="unload-project"]');
        if (!btn) return "no-button";
        btn.click();
        return "clicked";
    })()`);

    let unloadSeen = false;
    const unloadDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < unloadDeadline) {
        probe = await awaitEval(cdp, BV_DELTA_PROBE);
        if (probe && bvWireHas(probe.wire, "UnloadProject")) {
            unloadSeen = true;
            break;
        }
        await sleep(200);
    }
    add(
        "「卸载工程」**真的**发出 `UnloadProject` 命令（线上帧里看得见）",
        unloadSeen,
        unloadSeen ? "线上帧里有 UnloadProject" : "线上帧里没有 UnloadProject",
    );
    // ⚠️ 等命令**往返**（异步）⇒ 否则下一步的提交可能排在卸载前面。
    await sleep(600);

    // -- ③ 提交一张板 ⇒ **必须立即生效**（卸载的判别力） ---------------------
    //
    // ⚠️ **判据不是「按钮点过了」而是「改板立即生效」**：卸载之前提交只会
    // 变成「待生效」（宿主对已装载的会话只记下、不重建）⇒ ⇒ 提交后**生效图
    // 真的变成那张板** ⇒ 就证明 `image_loaded` 真的被清掉了 ⇒ ⇒ 这才是
    // 「卸载 ⇒ 改板立即生效」的端到端判别式（D-139 判别力 ⑤）。
    await runEffect(cdp, `window.dispatchEvent(new CustomEvent("riscv:command", {
        detail: {
            action: "submit-memory-map",
            requestId: "bv-delta-board",
            viewId: "memory-map",
            params: { description: ${JSON.stringify(BV_BOARD)} },
        },
    }))`);

    let effectiveRows = [];
    let appliedImmediately = false;
    const applyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < applyDeadline) {
        await runEffect(cdp, 'location.hash = "#/product";');
        const p = await awaitEval(cdp, BV_MEMORY_MAP_PROBE);
        effectiveRows = Array.isArray(p?.rows) ? p.rows : [];
        const hasNewRam = effectiveRows.some(
            (r) => String(r.base) === String(BV_NEW_RAM_BASE) && String(r.kind) === "ram",
        );
        if (hasNewRam && effectiveRows.length === BV_BOARD.regions.length) {
            appliedImmediately = true;
            break;
        }
        await sleep(250);
    }
    add(
        "卸载之后改板**立即生效**（生效图真的变成那张板 ⇒ `image_loaded` 被清了）",
        appliedImmediately,
        appliedImmediately
            ? `生效图 ${effectiveRows.length} 行，含新 RAM 段`
            : `生效图没变成那张板（${effectiveRows.length} 行）`,
    );

    // ⚠️ 「待生效」横幅**必须不出现**（立即生效 ⇒ 没有待生效的那一份）。
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await sleep(300);
    probe = await awaitEval(cdp, BV_DELTA_PROBE);
    add(
        "立即生效 ⇒ **没有**「待生效」横幅",
        probe?.pending === false,
        `pending = ${JSON.stringify(probe?.pending ?? null)}`,
    );

    const revisionBefore = probe?.revision ?? null;
    add(
        "面板上**看得见修订号**（`data-mmap-revision`）",
        revisionBefore !== null && /^[0-9]+$/.test(String(revisionBefore)),
        `revision = ${JSON.stringify(revisionBefore)}`,
    );

    // -- ④ 改一段 RAM 的大小（元数据**一字不动**）⇒ 走**差量** ---------------
    //
    // ⚠️ **先记下「改动前」的生效图**（T-4 第 ⑥ 条的对照量）⇒ ⇒ 它必须
    // 在**差量提交之前**取 ⇒ ⇒ 取的是**内核推回来的**那份（线上帧里的
    // `memory.map` 事件）⇒ ⇒ 不是 DOM 上的投影。
    const e0 = await awaitEval(cdp, BV_LAST_MEMORY_MAP_EXPRESSION);
    //
    // ⚠️ **走真 UI 动作**（不是手工拼一份描述发过去）：
    //   ① 点「从当前生效的图载入」⇒ 编辑器的草稿 = **当前生效的图**；
    //   ② 把那一段的 size 输入框改成 4 MiB；
    //   ③ 点「提交」。
    //
    // ⚠️ **为什么不能手工拼**：拼出来的那份描述**会漏掉内核补上的缺省**
    // （`auto_selectable` 缺省是 **true**）⇒ 与「当前生效的图」的元数据
    // **对不上** ⇒ 前端按裁决 D 判成「整份替换」⇒ **差量这条路根本没被
    // 走到** ⇒ 门禁会**假红**（而且是「测的不是那条路」的红）。
    //
    // ⚠️ **走真 UI 还有一层好处**：编辑器那一份是**七字段**（没有
    // `max_region_bytes`），而「当前生效的图」是**八字段**（内核恒定写出）
    // ⇒ 这一段**顺带**验了「缺省 ≡ null」的归一化。
    const editResult = await awaitEval(cdp, `(() => {
        const loadBtn = document.querySelector('[data-action="load-current"]');
        if (!loadBtn) return "no-load-current";
        loadBtn.click();
        const rows = Array.from(document.querySelectorAll("[data-mmap-row]"));
        let target = null;
        for (const row of rows) {
            const baseInput = row.querySelector('[data-mmap-region-field="base"]');
            if (!baseInput) continue;
            const base = parseInt(String(baseInput.value ?? "").trim(), 10);
            if (base === ${BV_DELTA_RAM_BASE}) { target = row; break; }
        }
        if (!target) return "no-row";
        const sizeInput = target.querySelector('[data-mmap-region-field="size"]');
        if (!sizeInput) return "no-size-input";
        sizeInput.value = String(${BV_DELTA_RAM_SIZE});
        sizeInput.dispatchEvent(new Event("input", { bubbles: true }));
        const btn = document.querySelector('[data-action="submit-memory-map"]');
        if (!btn) return "no-button";
        btn.click();
        return "clicked";
    })()`);
    add(
        "编辑器能**载入当前生效的图**并改那一段的大小（真 UI 动作）",
        editResult === "clicked",
        `编辑结果 = ${JSON.stringify(editResult)}`,
    );

    let patchFrame = "";
    const patchDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < patchDeadline) {
        probe = await awaitEval(cdp, BV_DELTA_PROBE);
        const wire = probe?.wire ?? [];
        if (bvWireHas(wire, "PatchMemoryMap")) {
            patchFrame = bvWireFind(wire, "PatchMemoryMap");
            break;
        }
        await sleep(200);
    }
    add(
        "只改一段 ⇒ 命令**走差量**（线上帧里是 `PatchMemoryMap`，**不是** `SetMemoryMap`）",
        patchFrame.length > 0,
        patchFrame ? `帧长 ${patchFrame.length} 字节` : "线上帧里没有 PatchMemoryMap",
    );
    add(
        "差量的载荷里**只有那一段**（一个 `UpsertRegion`，基址 = 被改的那段）",
        patchFrame.includes("UpsertRegion") && patchFrame.includes(String(BV_DELTA_RAM_BASE)),
        patchFrame ? patchFrame.slice(0, 220) : "没有差量帧",
    );

    // -- ⑤ 生效图里那一段**真的变了** ---------------------------------------
    let patchedRows = [];
    const patchedDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < patchedDeadline) {
        await runEffect(cdp, 'location.hash = "#/product";');
        const p = await awaitEval(cdp, BV_MEMORY_MAP_PROBE);
        patchedRows = Array.isArray(p?.rows) ? p.rows : [];
        const row = patchedRows.find((r) => String(r.base) === String(BV_DELTA_RAM_BASE));
        if (row && String(row.size) === String(BV_DELTA_RAM_SIZE)) break;
        await sleep(250);
    }
    const patchedRow = patchedRows.find((r) => String(r.base) === String(BV_DELTA_RAM_BASE));
    add(
        "生效图里**那一段真的变了**（大小 = 4 MiB）",
        Boolean(patchedRow) && String(patchedRow.size) === String(BV_DELTA_RAM_SIZE),
        `行 = ${JSON.stringify(patchedRow ?? null)}`,
    );

    // -- ⑥ 修订号 **+1** ----------------------------------------------------
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await sleep(300);
    probe = await awaitEval(cdp, BV_DELTA_PROBE);
    const revisionAfter = probe?.revision ?? null;
    add(
        "修订号**真的 +1**（并发可见性的凭据）",
        Number(revisionAfter) === Number(revisionBefore) + 1,
        `${JSON.stringify(revisionBefore)} → ${JSON.stringify(revisionAfter)}`,
    );

    // =================================================================
    // T-4（STAGE-17 · §4.2 · D-145 的**真机层**）· 差量 ≡ 整份
    // =================================================================
    //
    // ⚠️ **六步**（规格 §4.2 的原文口径）：
    //   ① 真前端把「当前生效的图」载入编辑器 ⇒ 改一段 ⇒ 提交 ⇒ ⇒ 走**差量**；
    //   ② 记下生效图 **E1**；
    //   ③ 把**同一张板**（含那处改动）**整份**提交一次 ⇒ ⇒ 走**整份**；
    //   ④ 记下生效图 **E2**；
    //   ⑤ 断言：E1 与 E2 **逐字段相同**；
    //   ⑥ 断言：E1 与「改动前的图」**不相同**。
    //
    // ⚠️ **① 已经做完了**（就是上面第 ④ 步那个「只改一段 ⇒ 走差量」）⇒
    // 这里从 **②** 接着做。
    //
    // ⚠️ **「生效图」从哪来**：**内核推回来的 `memory.map` 事件**（线上帧
    // 里那个 `MemoryMap` 载荷的 `description`）⇒ ⇒ 那是**内核说的**当前
    // 生效的图，不是 DOM 上的投影 ⇒ ⇒ 「逐字段相同」比的是**图本身**。
    //
    // ⚠️ **③ 为什么走线上帧**：前端的路由规则（裁决 D）是「**元数据**变了
    // ⇒ 整份；否则 ⇒ 差量」⇒ ⇒ **同一张板**在前端那里**只可能**走差量 ⇒
    // ⇒ 「整份路径」只能由**别的入口**触发 ⇒ 这里直接把 `SetMemoryMap`
    // 命令**发到 worker**（**用前端自己的契约镜像** `front/lib/frame.js` +
    // `front/lib/message.js` ⇒ **不另写一份线上形状**）⇒ ⇒ 内核走的是
    // **它自己的整份路径**（`on_set_memory_map`）⇒ ⇒ 两条路径比的是
    // **同一个内核**的两条路。
    //
    // ⚠️ **E2 用的是「用户配的那张板」**（草稿转成的七键描述）⇒ ⇒ 内核
    // 自己补缺省（`auto_selectable` / `max_region_bytes`）⇒ ⇒ 与 E1
    // （差量合成 ⇒ 也是内核补过缺省的）**可比**。
    const draftDescription = await awaitEval(cdp, BV_LAST_SUBMITTED_DESCRIPTION_EXPRESSION);
    const revisionNow = Number(revisionAfter);
    // ⚠️ **E1 = 差量路径的生效图**（= 整份提交**之前**内核说的那张）。
    const e1 = await awaitEval(cdp, BV_LAST_MEMORY_MAP_EXPRESSION);

    const wholeSubmit = await awaitEval(cdp, `(async () => {
        const { encodeFrame } = await import("./lib/frame.js");
        const { Command, commandMessage } = await import("./lib/message.js");
        if (!window.__bvWorker) return { ok: false, why: "no-worker" };
        const seq = ${BV_TOOL_COMMAND_SEQ};
        const payload = commandMessage(seq, Command.setMemoryMap(
            ${JSON.stringify(draftDescription)},
            ${revisionNow},
        ));
        const frame = encodeFrame({
            type: "command",
            seq: seq,
            sessionId: null,
            role: null,
            reqId: null,
            payload: payload,
        });
        window.__bvWorker.postMessage(frame);
        return { ok: true, seq: seq };
    })()`);
    add(
        "整份路径的命令**真的发到内核**了（线上帧里是 `SetMemoryMap`）",
        Boolean(wholeSubmit?.ok),
        wholeSubmit?.ok
            ? `已发 SetMemoryMap（期望修订号 ${revisionNow}）`
            : `发不出去：${JSON.stringify(wholeSubmit?.why ?? null)}`,
    );

    // 等内核把整份路径的结果推回来（E2）。
    let e2 = null;
    const e2Deadline = Date.now() + opts.timeoutMs;
    while (Date.now() < e2Deadline) {
        const now = await awaitEval(cdp, BV_LAST_MEMORY_MAP_EXPRESSION);
        if (now && Number(now.revision) > revisionNow) {
            e2 = now;
            break;
        }
        await sleep(200);
    }
    const e1Canonical = canonicalMemoryMap(e1?.description ?? null);
    const e2Canonical = canonicalMemoryMap(e2?.description ?? null);
    const wholeErr = await awaitEval(cdp, BV_LAST_ERROR_EVENT_EXPRESSION);
    add(
        "T-4：差量路径与整份路径的**生效图逐字段相同**（规范 JSON · 键序无关）",
        Boolean(e2) && e1Canonical === e2Canonical && e1Canonical !== "null",
        e2
            ? `E1 ${e1Canonical.length} 字节 / E2 ${e2Canonical.length} 字节 ⇒ ${e1Canonical === e2Canonical ? "相同" : "**不同**"}`
            : `整份路径没回来（内核没回事件？内核最后的错误：`
              + `${JSON.stringify(wholeErr ?? null)}；送出去的描述：`
              + `${draftDescription ? `${canonicalMemoryMap(draftDescription).length} 字节` : "**没捕到提交命令**"}）`,
    );
    add(
        "T-4：生效图与**改动前**那张**不相同**（⇒ 差量**真的**生效了，不是两边都没变）",
        canonicalMemoryMap(e0?.description ?? null) !== e1Canonical
            && canonicalMemoryMap(e0?.description ?? null) !== "null",
        `改动前 ${canonicalMemoryMap(e0?.description ?? null).length} 字节 vs 改动后 ${e1Canonical.length} 字节`,
    );

    // -- ⑦ 刷新页面 ⇒ 重新引导成功 + **板档案还在**（回归守卫） --------------
    await runEffect(cdp, "location.reload();");
    await sleep(1200);
    let landing = [];
    const landingDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < landingDeadline) {
        const p = await awaitEval(cdp, PROBE_EXPRESSION);
        landing = p ? landingChecks(p) : [];
        if (landing.length > 0 && landing.every((c) => c.ok)) break;
        await sleep(250);
    }
    add(
        "刷新后前端**重新引导**成功（落地页检查全过）",
        landing.length > 0 && landing.every((c) => c.ok),
        landing.length === 0 ? "落地页检查没跑起来" : `落地页 ${landing.filter((c) => c.ok).length}/${landing.length}`,
    );

    // ⚠️ **必须 `awaitPromise`**：`indexedDB.open` 走的是 Promise ⇒ 不 await
    // 的话这里拿到的是**一个还没 resolve 的 Promise 对象** ⇒ 「档案空了」
    // 与「探测失败」会**长得一模一样**（一条假红）。
    const archive = await awaitEval(cdp, `(() => {
        return new Promise((resolve) => {
                let req;
                try {
                    // ⚠️ **不带版本号**：带版本号打开会与前端自己那次打开
                    // 撞版本（versionchange ⇒ blocked）⇒ 探测直接失败。
                    req = indexedDB.open("riscv-sim-board-store");
                } catch (e) {
                    resolve({ ok: false, why: "open-throws", ids: [] });
                    return;
                }
                req.onerror = () => resolve({ ok: false, why: "open-error", ids: [] });
                req.onblocked = () => resolve({ ok: false, why: "open-blocked", ids: [] });
                req.onsuccess = () => {
                    const db = req.result;
                    try {
                        const tx = db.transaction("boards", "readonly");
                        const store = tx.objectStore("boards");
                        const all = store.getAll();
                        all.onsuccess = () => {
                            const ids = (all.result || []).map((e) => String(e && e.id || ""));
                            resolve({ ok: true, why: "ok", ids });
                        };
                        all.onerror = () => resolve({ ok: false, why: "getAll-error", ids: [] });
                    } catch (e) {
                        resolve({ ok: false, why: "tx-throws", ids: [] });
                    }
                };
            });
        })()`);
    add(
        "刷新后**板档案还在**（本批没把上一批的能力弄坏）",
        Boolean(archive?.ok) && Array.isArray(archive?.ids) && archive.ids.length > 0,
        archive?.ok
            ? `库里 ${JSON.stringify(archive?.ids ?? [])}`
            : `档案探测**失败**（why = ${JSON.stringify(archive?.why ?? null)}）—— 不等于「库里空了」`,
    );

    return { skipped: false, checks };
}

// ---------------------------------------------------------------------------
// STAGE-17 · 乱序到达（T-6 · D-146 的**真机层**）
// ---------------------------------------------------------------------------

/**
 * 走**真 UI**改一段的大小并提交（返回改了什么）。
 *
 * ⚠️ **走真 UI 动作**（载入当前生效的图 → 改一栏 → 点提交）⇒ ⇒ 前端
 * **自己**算差量、自己算期望修订号 ⇒ ⇒ 这条路验的才是「真前端」。
 *
 * ⚠️ **改的是第一段带 size 的 RAM/ROM**（外设窗口**没有** size 栏 ⇒
 * 跳过）⇒ ⇒ 改成**一半**（保证一定变小 ⇒ 差量非空 ⇒ 前端**真的**会发
 * 命令）。
 */
function bvEditFirstRamSizeExpression() {
    return `(() => {
        const loadBtn = document.querySelector('[data-action="load-current"]');
        if (!loadBtn) return { ok: false, why: "no-load-current" };
        loadBtn.click();
        const rows = Array.from(document.querySelectorAll("[data-mmap-row]"));
        let target = null;
        for (const row of rows) {
            const sizeInput = row.querySelector('[data-mmap-region-field="size"]');
            if (!sizeInput) continue;
            target = { row: row, sizeInput: sizeInput };
            break;
        }
        if (!target) return { ok: false, why: "no-ram-row" };
        const baseInput = target.row.querySelector('[data-mmap-region-field="base"]');
        const before = Number(String(target.sizeInput.value ?? "").trim());
        const after = Math.max(1, Math.floor(before / 2));
        if (!Number.isFinite(before) || before <= 1 || after === before) {
            return { ok: false, why: "cannot-halve", before: before };
        }
        target.sizeInput.value = String(after);
        target.sizeInput.dispatchEvent(new Event("input", { bubbles: true }));
        const btn = document.querySelector('[data-action="submit-memory-map"]');
        if (!btn) return { ok: false, why: "no-submit" };
        btn.click();
        return {
            ok: true,
            base: baseInput ? String(baseInput.value ?? "") : "",
            before: before,
            after: after,
        };
    })()`;
}

/**
 * 乱序到达那一段（STAGE-17 · §5 的 **T-6**）。
 *
 * ⚠️ **依赖 Lane B 的前端护栏**（`wireMemoryMap` 的「**严格更旧**才挡」）：
 * 没有那条护栏时 ⇒ 前端会被更旧的事件覆盖 ⇒ 提交带的是**更旧**的修订号
 * ⇒ 真内核**必然**拒（`E_PLT-13`）⇒ ⇒ **这条 lane 单独跑时 T-6 必然红**
 * ⇒ ⇒ 它在**整合批**（三条 lane 合上之后）才会变绿。
 *
 * ⚠️ **注入的是「线上帧」**：走的是**内核 → 主机**那条路（worker 的
 * `message` 事件）⇒ ⇒ 用**前端自己的契约镜像**造那个帧（`frame.js` 的
 * `encodeFrame` + `message.js` 的 `Event.memoryMap`）⇒ ⇒ **不另写一份
 * 线上形状** ⇒ ⇒ 帧的 `crc` 也是前端自己算的。
 *
 * ⚠️ **注入的是「更旧」的事件**：修订号**严格小于**当前那个 ⇒ ⇒ 前端若
 * 没有护栏，它就会拿这份更旧的修订号去提交 ⇒ ⇒ 内核拒（`E_PLT-13`）。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runBoardOrderingSection(cdp, opts) {
    /** @type {{name: string, ok: boolean, detail: string}[]} */
    const checks = [];
    const add = (name, ok, detail) => checks.push({ name, ok: Boolean(ok), detail: String(detail) });

    // -- ① 切到编辑器视图 + **重装**线上钩子（上一页刷新过 ⇒ 钩子丢了） ----
    await runEffect(cdp, 'location.hash = "#/memory-map";');
    await runEffect(cdp, BV_WIRE_HOOK_EXPRESSION);
    await runEffect(cdp, BV_CAPTURE_COMMANDS_EXPRESSION);

    let probe = null;
    const readyDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < readyDeadline) {
        probe = await awaitEval(cdp, BV_DELTA_PROBE);
        if (probe?.unloadPresent) break;
        await sleep(250);
    }
    if (!probe?.unloadPresent) {
        return {
            skipped: false,
            checks: [{
                name: "板级内存图编辑器挂上了（`#/memory-map`）",
                ok: false,
                detail: "等不到编辑器视图（前端引导没起来？）",
            }],
        };
    }
    add("板级内存图编辑器挂上了（`#/memory-map`）", true, "编辑器在");

    // -- ② 先真提交一次 ⇒ 拿到一个**非零**的修订号 --------------------------
    //
    // ⚠️ **为什么先提交一次**：要注入的「更旧」事件必须**严格更旧** ⇒ ⇒
    // 当前修订号得 **> 0** ⇒ ⇒ 先让内核接受一次变更。
    //
    // ⚠️ **必须真的改一处** ⇒ 否则前端的计划是「什么都没改」⇒ **不发
    // 命令** ⇒ ⇒ 白跑一趟。
    const firstEdit = await awaitEval(cdp, bvEditFirstRamSizeExpression());
    add(
        "先真提交一次（改一段的大小 ⇒ 差量非空 ⇒ 前端**真的**发命令）",
        Boolean(firstEdit?.ok),
        firstEdit?.ok
            ? `基址 ${firstEdit.base} 的大小 ${firstEdit.before} → ${firstEdit.after}`
            : `没改成：${JSON.stringify(firstEdit?.why ?? null)}`,
    );
    if (!firstEdit?.ok) {
        return { skipped: false, checks };
    }

    // 等内核把这次提交的结果推回来（修订号 +1）。
    let afterFirst = null;
    const firstDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < firstDeadline) {
        const now = await awaitEval(cdp, BV_LAST_MEMORY_MAP_EXPRESSION);
        if (now && Number(now.revision) >= 1) {
            afterFirst = now;
            break;
        }
        await sleep(200);
    }
    const revisionBefore = Number(afterFirst?.revision ?? -1);
    add(
        "内核接受了那一次提交（内核说的修订号 **+1**）",
        revisionBefore >= 1,
        `内核的修订号 = ${revisionBefore}`,
    );

    // -- ③ 注入一个**更旧**的 `memory.map` 事件 ----------------------------
    //
    // ⚠️ **注入的是事件帧**（`type: "event"`）⇒ ⇒ 走的是**内核 → 主机**
    // 那条路 ⇒ ⇒ 前端收到它的方式与收到内核的真事件**完全一样** ⇒ ⇒
    // 「更旧的事件先到/后到」这件事被**真的**演练了一遍。
    //
    // ⚠️ **描述用当前生效的那份**（只有修订号更旧）⇒ ⇒ 判据落在**修订号**
    // 上（D-146 的判别式）⇒ ⇒ 不会顺手把前端的图换成别的。
    const injectSeq = BV_INJECTED_FRAME_SEQ;
    const injected = await awaitEval(cdp, `(async () => {
        const { encodeFrame } = await import("./lib/frame.js");
        const { Event, eventMessage } = await import("./lib/message.js");
        if (!window.__bvWorker) return { ok: false, why: "no-worker" };
        const payload = eventMessage(
            ${injectSeq},
            Event.memoryMap(
                ${JSON.stringify(afterFirst?.description ?? null)},
                ${afterFirst?.pending === true},
                ${afterFirst?.loaded === true},
                ${Math.max(0, revisionBefore - 1)},
            ),
        );
        const frame = encodeFrame({
            type: "event",
            seq: ${injectSeq},
            sessionId: null,
            role: null,
            reqId: null,
            payload: payload,
        });
        window.__bvWorker.dispatchEvent(new MessageEvent("message", { data: frame }));
        return { ok: true, revision: ${Math.max(0, revisionBefore - 1)} };
    })()`);
    add(
        "注入了一个**更旧**的 `memory.map` 事件（修订号 **严格更旧**）",
        Boolean(injected?.ok),
        injected?.ok
            ? `注入的修订号 = ${injected.revision}（当前 ${revisionBefore}）`
            : `注不进去：${JSON.stringify(injected?.why ?? null)}`,
    );
    if (!injected?.ok) {
        return { skipped: false, checks };
    }
    await sleep(300);

    // -- ④ 再真提交一次（改另一处 ⇒ 走差量）⇒ 看内核的答复 ----------------
    const secondEdit = await awaitEval(cdp, bvEditFirstRamSizeExpression());
    add(
        "乱序之后**再真提交一次**（真 UI 动作 ⇒ 走差量）",
        Boolean(secondEdit?.ok),
        secondEdit?.ok
            ? `基址 ${secondEdit.base} 的大小 ${secondEdit.before} → ${secondEdit.after}`
            : `没改成：${JSON.stringify(secondEdit?.why ?? null)}`,
    );

    // 等内核答复（接受 ⇒ 修订号 +1；拒 ⇒ 只有一条错误事件、修订号不动）。
    let kernelRevision = revisionBefore;
    const secondDeadline = Date.now() + opts.timeoutMs;
    while (Date.now() < secondDeadline) {
        const now = await awaitEval(cdp, BV_LAST_MEMORY_MAP_EXPRESSION);
        if (now && Number(now.revision) > kernelRevision) {
            kernelRevision = Number(now.revision);
            break;
        }
        await sleep(200);
    }
    const errProbe = await awaitEval(cdp, BV_MEMORY_MAP_ERROR_PROBE);
    const errCode = String(errProbe?.code ?? "");
    add(
        "T-6：乱序之后真提交 ⇒ **真内核接受**（**不出现** `E_PLT-13`，且内核的修订号 +1）",
        errCode !== "E_PLT-13" && kernelRevision === revisionBefore + 1,
        `错误区：${errProbe?.present ? `码 ${JSON.stringify(errCode)}` : "没有错误"}；`
        + `内核修订号 ${revisionBefore} → ${kernelRevision}`
        + (errCode === "E_PLT-13"
            ? " ⇒ **前端把基底守在了更旧的那份上** ⇒ 护栏缺失"
            : " ⇒ 内核接受了这次提交"),
    );

    return { skipped: false, checks };
}

// ---------------------------------------------------------------------------
// STAGE-32 · v0.47 · 固件加载 UI（真机 B 组）
// ---------------------------------------------------------------------------

/**
 * 用 CDP 的 `DOM.setFileInputFiles` **真**给一个 `<input type="file">` 选文件。
 *
 * ⚠️ **这是 B-1 的判别力来源**：真机门禁**不许**用脚本口
 * （`window.riscv_image.loadFromUrl`）代替界面路径 ⇒ ⇒ 否则那条判据恒绿
 * ⇒ ⇒ 这里走的是**浏览器自己的文件选择器**（CDP 驱动）⇒ ⇒ 走的是
 * `change` 事件 ⇒ ⇒ 与用户点出来的**完全同一条路**。
 *
 * @param {object} cdp
 * @param {string} selector — 文件输入框的选择器
 * @param {string} filePath — **绝对**路径
 * @returns {Promise<void>}
 */
async function selectFileForInput(cdp, selector, filePath) {
    await cdp.send("DOM.enable");
    const doc = await cdp.send("DOM.getDocument", { depth: -1, pierce: true });
    const rootId = doc?.root?.nodeId;
    if (!rootId) throw new Error("拿不到 DOM 根节点");
    const found = await cdp.send("DOM.querySelector", { nodeId: rootId, selector });
    if (!found?.nodeId) throw new Error(`页面里找不到文件输入框：${selector}`);
    await cdp.send("DOM.setFileInputFiles", { files: [filePath], nodeId: found.nodeId });
}

/** 加载器面板的探针（DOM 现状）。 */
const IMAGE_LOADER_PROBE = `(() => {
  const status = document.querySelector("[data-image-load-status]");
  const err = document.querySelector("[data-image-load-error]");
  const input = document.querySelector("[data-image-input]");
  const btn = document.querySelector('[data-action="load-image"]');
  // ⚠️ **文件输入框里真的躺着那个文件吗** —— 这是 B-1 的**正向**判据：
  // 只断言「CDP 调用没报错」是**弱**的 ⇒ ⇒ 换成「浏览器的文件输入框里
  // **真的有**那个文件（名字 + 大小）」⇒ ⇒ 走脚本口代替界面路径时
  // **这里必然是空** ⇒ ⇒ 那条判据才不是恒绿。
  const files = input && input.files
    ? Array.from(input.files).map((f) => ({
        name: String(f.name ?? ""),
        size: Number(f.size ?? 0),
      }))
    : [];
  return {
    status: status ? String(status.getAttribute("data-image-load-status") || "") : null,
    text: status ? String(status.textContent || "").trim() : "",
    error: err ? String(err.textContent || "").trim() : null,
    hasInput: !!input,
    inputType: input ? String(input.getAttribute("type") || "") : null,
    accept: input ? String(input.getAttribute("accept") || "") : null,
    hasButton: !!btn,
    files,
  };
})()`;

/** 寄存器 / PC 的探针（regmem 视图）。 */
const UI_IMAGE_REG_PROBE = `(() => {
  const cells = Array.from(document.querySelectorAll('[data-view-body="regmem"] .reg-grid > div'));
  const val = (i) => {
    const code = cells[i] ? cells[i].querySelector("code") : null;
    return code ? String(code.textContent || "").trim() : "";
  };
  const pc = document.querySelector('[data-view-body="regmem"] code.pc');
  return {
    count: cells.length,
    x1: val(1),
    x3: val(3),
    pc: pc ? String(pc.textContent || "").trim() : "",
  };
})()`;

/** 终端输出 + 波形通道列表的探针。 */
const UI_IMAGE_TERM_WAVE_PROBE = `(() => {
  const term = document.querySelector("[data-terminal-out]");
  const items = Array.from(document.querySelectorAll('[data-view-body="waveform"] .channel-list > li'));
  return {
    terminal: term ? String(term.textContent || "") : "",
    channels: items.map((li) => String(li.textContent || "").trim()),
  };
})()`;

/**
 * 等加载器面板进入某个状态。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @param {(state: object) => boolean} predicate
 * @returns {Promise<object|null>} 最后一个看到的状态（等不到就是最后一个）
 */
async function waitForImageState(cdp, opts, predicate) {
    const deadline = Date.now() + opts.timeoutMs;
    let last = null;
    while (Date.now() < deadline) {
        last = await awaitEval(cdp, IMAGE_LOADER_PROBE);
        if (last && predicate(last)) return last;
        await sleep(200);
    }
    return last;
}

/**
 * 从波形通道列表里取 `GPIO0.OUT` 的**边沿数**。
 *
 * ⚠️ **找不到就返回 -1**（不是 0）⇒ ⇒ 「通道根本不在列表里」与「通道在
 * 但没有边沿」**分得开**⇒ ⇒ 否则 B-4 会在「波形视图整个空了」时**假绿**。
 *
 * @param {string[]} channels
 * @returns {number}
 */
function gpioOutEdges(channels) {
    const list = Array.isArray(channels) ? channels : [];
    for (const item of list) {
        const text = String(item);
        if (!text.includes("GPIO0.OUT")) continue;
        const m = text.match(/(\d+)\s+edges/);
        return m ? Number(m[1]) : -1;
    }
    return -1;
}

/**
 * 「文件真的被选进输入框了」的**留痕**探针。
 *
 * ⚠️ **为什么不能直接读 `input.files`**：视图每次重渲染都把
 * `body.innerHTML` 整个换掉 ⇒ ⇒ **input 元素被重建** ⇒ ⇒ 新元素的
 * `files` **是空的** ⇒ ⇒ 读它必然得到 `[]` ⇒ ⇒ 那条判据**恒红**（实测
 * 踩过）⇒ ⇒ 改成在 **`change` 事件里**留痕（那时文件**还在**）⇒ ⇒ 留痕
 * 挂在 `document` 上 ⇒ ⇒ 元素被重建也不影响它。
 */
const BV_IMAGE_PICK_PROBE = `(() => {
  const rec = window.__bv_image_pick ?? null;
  if (!rec) return null;
  return { name: String(rec.name ?? ""), size: Number(rec.size ?? 0) };
})()`;

/**
 * 在页面里装一个 **`change` 事件的捕获监听器**（把选中的文件留痕）。
 *
 * ⚠️ **用捕获阶段**（`capture: true`）⇒ ⇒ 它在视图层那个监听器**之前**跑
 * ⇒ ⇒ 留痕的是「浏览器真的把文件交给了 `change`」这件事 ⇒ ⇒ 走脚本口
 * 代替界面路径时**根本不会有 `change`** ⇒ ⇒ 留痕恒为空 ⇒ ⇒ 那条判据
 * 才不是恒绿。
 *
 * @param {object} cdp
 * @returns {Promise<void>}
 */
async function installImagePickRecorder(cdp) {
    await runEffect(cdp, [
        'window.__bv_image_pick = null;',
        'document.addEventListener("change", (ev) => {',
        '  const t = ev.target;',
        '  const f = t && t.files ? t.files[0] : null;',
        '  if (f) window.__bv_image_pick = { name: String(f.name || ""), size: Number(f.size || 0) };',
        '}, true);',
    ].join("\n"));
}

/**
 * 固件加载 UI 那一段（STAGE-32 · B 组）：**真**在界面上选文件 → 装载 →
 * 跑起来 → 内核真的换了镜像。
 *
 * ⚠️ **必须排在 osz 段之前**：这一段把当前镜像换成夹具 ⇒ ⇒ 放在 osz 段
 * 后面会让 osz 段看到的是夹具而不是它自己装的那张板。
 *
 * ⚠️ **判据不许用脚本口代替界面路径**（规格 §6 判别力清单第 6 条）⇒ ⇒
 * 选文件走 CDP 的 `DOM.setFileInputFiles`（浏览器的**真**文件选择器）⇒ ⇒
 * 走的是 `change` 事件 ⇒ ⇒ 与用户点出来的完全同一条路。
 *
 * @param {object} cdp
 * @param {{timeoutMs: number}} opts
 * @returns {Promise<{skipped: boolean, reason?: string, checks: object[]}>}
 */
async function runLoadImageSection(cdp, opts) {
    const checks = [];
    const tmpDir = mkdtempSync(join(tmpdir(), "riscv-ui-image-"));
    try {
        // -- 编码器自检（先钉住「夹具本身是对的」）------------------------
        // ⚠️ 编码器写错时**不报错** ⇒ ⇒ 夹具会「跑得好好的，只是没做它该做
        // 的事」⇒ ⇒ 这里拿仓内那份**已被内核执行过**的 demo 固件当标尺。
        const encoder = checkEncoderAgainstDemoFirmware();
        checks.push({
            name: "夹具自检 · 编码器逐字节复现 demo 固件（U/I/S/J/R 五种形式）",
            ok: encoder.ok,
            detail: encoder.detail,
        });
        if (!encoder.ok) return { skipped: false, checks };

        const elfBytes = buildUiImageElf();
        const elfPath = join(tmpDir, "ui-image.elf");
        writeFileSync(elfPath, elfBytes);
        // 一个**故意坏掉**的 ELF（magic 不对）⇒ ⇒ 内核必须拒它（B-5）。
        const badPath = join(tmpDir, "broken.elf");
        writeFileSync(badPath, Buffer.from("this file is definitely not an ELF image\n"));

        // -- 「装载前」的波形基线（demo 固件的那条边沿还在）-----------------
        // ⚠️ **必须先取基线**：B-4 的判据是「**前后对照**」⇒ ⇒ 只看装载后
        // 的边沿数是 0 **不够**（万一它**一直是** 0 呢）⇒ ⇒ 必须同时证明
        // 「装载前 ≥ 1」。
        await runEffect(cdp, 'location.hash = "#/waveform";');
        await sleep(500);
        const waveBefore = await awaitEval(cdp, UI_IMAGE_TERM_WAVE_PROBE);
        const edgesBefore = gpioOutEdges(waveBefore?.channels);

        // -- 装载器在产品视图里 ⇒ 先切过去 ------------------------------
        await runEffect(cdp, 'location.hash = "#/product";');
        await sleep(500);

        // -- B-1 · **真**用界面的文件选择器选文件 -------------------------
        // ⚠️ **留痕监听器必须先装**（否则 `change` 里看到的东西就没了）。
        await installImagePickRecorder(cdp);
        let pickerError = null;
        try {
            await selectFileForInput(cdp, "[data-image-input]", elfPath);
        } catch (err) {
            pickerError = String(err?.message ?? err).slice(0, 300);
        }
        checks.push({
            name: "B-1 · 界面上真选了文件（CDP DOM.setFileInputFiles ⇒ 走 change 事件）",
            ok: pickerError === null,
            detail: pickerError ?? `已选入 ${basename(elfPath)}（${elfBytes.length} 字节）`,
        });
        if (pickerError) return { skipped: false, checks };

        // -- B-1a · **正向**判据：`change` 事件里**真的有**那个文件 -------
        // ⚠️ **这一条才是 B-1 的判别力来源**：上面那条只断言「CDP 调用没
        // 报错」（**弱**）⇒ ⇒ 换成「浏览器把那个文件交给了 `change` 事件」
        // ⇒ ⇒ **走脚本口**代替界面路径时**根本没有 `change`** ⇒ ⇒ 判据不是
        // 恒绿。
        let pickedFile = null;
        const pickDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < pickDeadline) {
            pickedFile = await awaitEval(cdp, BV_IMAGE_PICK_PROBE);
            if (pickedFile) break;
            await sleep(200);
        }
        checks.push({
            name: "B-1a · **`change` 事件里真的有那个文件**（名字 + 大小 ⇒ ⇒ 走的是界面路径）",
            ok: pickedFile?.name === basename(elfPath) && pickedFile?.size === elfBytes.length,
            detail: `change 事件留痕：${JSON.stringify(pickedFile)}`,
        });

        // -- 面板必须显示「已选择」+ 字节数（字节真的读到了）--------------
        const picked = await waitForImageState(cdp, opts,
            (s) => s.status === "error" || s.text.includes("ui-image.elf"));
        checks.push({
            name: "B-1b · 面板显示已选文件与**真实字节数**（字节数 == 文件大小）",
            ok: picked?.status === "idle"
                && picked?.text.includes("ui-image.elf")
                && picked?.text.includes(String(elfBytes.length)),
            detail: picked?.status === "error"
                ? `读文件就失败了：${picked.error ?? ""}`
                : `面板：${JSON.stringify(picked?.text ?? "")}（文件 ${elfBytes.length} 字节）`,
        });

        // -- 点「加载」----------------------------------------------------
        await runEffect(cdp,
            'document.querySelector(\'[data-action="load-image"]\').click();');
        const loaded = await waitForImageState(cdp, opts,
            (s) => s.status === "ok" || s.status === "error");
        checks.push({
            name: "B-1c · 装载成功（面板状态 = ok，且显示文件名与字节数）",
            ok: loaded?.status === "ok"
                && loaded?.text.includes("ui-image.elf")
                && loaded?.text.includes(String(elfBytes.length)),
            detail: `面板状态 ${JSON.stringify(loaded?.status ?? null)}：${JSON.stringify(loaded?.text ?? "")}`,
        });
        if (loaded?.status !== "ok") return { skipped: false, checks };

        // -- B-3 · 终端面板必须**先挂上**，console 字节才进得了 DOM ---------
        // ⚠️ console 字节是**流**：内核推 → 前端 `onDelta` 写进 store **并**
        // 直接写 `[data-terminal-out]` ⇒ ⇒ 面板**没挂上**时那一步会跳过
        // ⇒ ⇒ 与 osz 段同一条规矩：**先切到终端视图再跑**。
        await runEffect(cdp, 'location.hash = "#/terminal";');
        await sleep(500);

        // -- 跑起来（「装完就停」是用户的定案 ⇒ ⇒ 这里**自己**发步进）-----
        // ⚠️ 装完那一步**没有**自动跑（那是 A-8 的判据）⇒ ⇒ 这里发的是
        // 「用户点继续/单步」的动作。
        await runEffect(cdp,
            'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step10" } }));');
        await sleep(600);
        await runEffect(cdp,
            'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step10" } }));');

        let term = null;
        const termDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < termDeadline) {
            term = await awaitEval(cdp, UI_IMAGE_TERM_WAVE_PROBE);
            if (String(term?.terminal ?? "").includes(UI_IMAGE_ELF_MARKER)) break;
            await sleep(250);
        }
        checks.push({
            name: `B-3 · 终端跑出固件自己的输出（判据字面量 ${JSON.stringify(UI_IMAGE_ELF_MARKER)}）`,
            ok: String(term?.terminal ?? "").includes(UI_IMAGE_ELF_MARKER),
            detail: `终端：${JSON.stringify(String(term?.terminal ?? "").slice(0, 200))}`,
        });

        // -- B-2 · 内核真的换了镜像 ---------------------------------------
        await runEffect(cdp, 'location.hash = "#/regmem";');
        await sleep(500);
        let regs = null;
        const regDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < regDeadline) {
            regs = await awaitEval(cdp, UI_IMAGE_REG_PROBE);
            if (regs && regs.x3 === "00000123") break;
            await sleep(250);
        }
        checks.push({
            name: "B-2 · 内核真的换了镜像（x3 = 夹具的特征值，不是 demo 的 0x2a）",
            ok: regs?.x3 === "00000123" && regs?.x1 !== "0000002a",
            detail: `x1=${JSON.stringify(regs?.x1 ?? "")} x3=${JSON.stringify(regs?.x3 ?? "")} pc=${JSON.stringify(regs?.pc ?? "")}`,
        });

        // -- B-4 · 波形上 demo 固件的那条边沿**没了** ----------------------
        // ⚠️ **判据是「边沿数」，不是「通道在不在」**：通道是**板子上的器件**
        // （qemu-virt 也有 GPIO0）⇒ ⇒ 「通道列表里没有 GPIO0.OUT」这条判据
        // 在换档之后**必然为真**⇒ ⇒ 是**假绿**⇒ ⇒ 真正的判据是
        // 「**边沿**没了」⇒ ⇒ demo 固件那条边沿是装载**前**产生的 ⇒ ⇒
        // 装载后必须为 0。
        await runEffect(cdp, 'location.hash = "#/waveform";');
        await sleep(500);
        const waveAfter = await awaitEval(cdp, UI_IMAGE_TERM_WAVE_PROBE);
        const edgesAfter = gpioOutEdges(waveAfter?.channels);
        checks.push({
            name: "B-4 · 波形上 demo 固件的 GPIO0.OUT **边沿**没了（装载前 ≥ 1 ⇒ 装载后 = 0）",
            ok: edgesBefore >= 1 && edgesAfter === 0,
            detail: `GPIO0.OUT 边沿数：装载前 ${edgesBefore} ⇒ 装载后 ${edgesAfter}；`
                + `通道列表：${JSON.stringify(waveAfter?.channels ?? [])}`,
        });

        // -- B-5 · 装载失败时界面显示内核的错误码 --------------------------
        await runEffect(cdp, 'location.hash = "#/product";');
        await sleep(400);
        let badPickError = null;
        try {
            await selectFileForInput(cdp, "[data-image-input]", badPath);
        } catch (err) {
            badPickError = String(err?.message ?? err).slice(0, 300);
        }
        if (badPickError) {
            checks.push({
                name: "B-5 · 装载失败时界面显示内核的错误码",
                ok: false,
                detail: `第二个文件选不进去：${badPickError}`,
            });
            return { skipped: false, checks };
        }
        await waitForImageState(cdp, opts, (s) => s.status === "error" || s.text.includes("broken.elf"));
        await runEffect(cdp,
            'document.querySelector(\'[data-action="load-image"]\').click();');
        const failed = await waitForImageState(cdp, opts, (s) => s.status === "error");
        const failText = `${failed?.text ?? ""} ${failed?.error ?? ""}`;
        checks.push({
            name: "B-5 · 装载失败时界面显示**内核原文**（含错误码 E_LDR-01）",
            ok: failed?.status === "error" && /E_LDR-\d{2}/.test(failText),
            detail: `面板状态 ${JSON.stringify(failed?.status ?? null)}：${JSON.stringify(failText.slice(0, 300))}`,
        });

        return { skipped: false, checks };
    } finally {
        rmSync(tmpDir, { recursive: true, force: true });
    }
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
        const imageReady = await awaitEval(cdp,
            'typeof window.riscv_image === "object" && window.riscv_image !== null');
        if (imageReady === true) {
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
    await runEffect(cdp, 'location.hash = "#/terminal";');
    await sleep(300);

    const artifact = await startArtifactServer(dirname(elfPath));
    try {
        const elfUrl = `http://127.0.0.1:${artifact.port}/${basename(elfPath)}`;

        // -- 装载：按 URL 取字节 → LoadProject → 强制重订 console 订阅 -----
        let loadReply = null;
        try {
            loadReply = await awaitEval(cdp,
                `window.riscv_image.loadFromUrl(${JSON.stringify(elfUrl)})`,
                120000);
        } catch (err) {
            return {
                skipped: false,
                checks: [{
                    name: "装载 osz 的 ELF（按 URL 取字节 → LoadProject）",
                    ok: false,
                    detail: String(err?.message ?? err).slice(0, 300),
                }],
            };
        }
        const byteLength = Number(loadReply?.byteLength ?? 0);
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
        let runReply = null;
        try {
            runReply = await awaitEval(cdp,
                `window.riscv_image.runUntilConsole("OSZ$ ", { timeoutMs: ${OSZ_RUN_TIMEOUT_MS} })`,
                OSZ_RUN_TIMEOUT_MS + 30000);
        } catch (err) {
            return {
                skipped: false,
                checks: [...breakpoint.checks, {
                    name: "跑到 osz 的 shell 提示符",
                    ok: false,
                    detail: String(err?.message ?? err).slice(0, 300),
                }],
            };
        }
        const batches = Number(runReply?.batches ?? 0);
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
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
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
    return await awaitEval(cdp, PROBE_EXPRESSION);
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
    try {
        await awaitEval(cdp,
            `window.riscv_kernel.setBreakpoint(${scheduler.value}, 1)`,
            30000);
    } catch (err) {
        checks.push({
            name: "在调度器符号上下断点",
            ok: false,
            detail: String(err?.message ?? err).slice(0, 300),
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
        try {
            await awaitEval(cdp, "window.riscv_kernel.continueRun()", 60000);
        } catch (_err) {
            break;
        }
        rounds += 1;
        halt = await awaitEval(cdp, "window.riscv_kernel.lastHalt()");
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
    await awaitEval(cdp, "window.riscv_kernel.clearBreakpoint(1)", 30000);

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
        const value = await awaitEval(cdp, DISASM_ROWS_EXPRESSION);
        return Array.isArray(value) ? value : [];
    }

    /**
     * 让面板反汇编一段（**生产路径**：M-02 发信号 → `app.js` 发命令）。
     * @param {number} addr
     */
    async function requestDisasm(addr) {
        await runEffect(cdp, `window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "disasm", params: { addr: ${addr}, count: 16 } } }));`);
    }

    // -- 第 1 轮：**按钮**（用当前 PC 当起点）------------------------------
    await runEffect(cdp, 'location.hash = "#/debug";');
    await sleep(300);
    await runEffect(cdp, `document.querySelector('[data-action="disasm"]')?.click();`);
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
    const sent = await awaitEval(cdp, sendTerminalTextExpression("A"));
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
        let value = null;
        try {
            value = await awaitEval(cdp,
                `window.riscv_kernel.readMemory(${UART_LSR_ADDR}, "Byte")`,
                30000);
        } catch (_err) {
            break;
        }
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
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            landing = probe ? landingChecks(probe) : [];
            if (landing.length > 0 && landing.every((c) => c.ok)) break;
            await sleep(250);
        }
        // ⚠️ **登记**（裁决 I）：判定聚合**只**从注册表取 ⇒ ⇒ 「新加一段
        // 忘了并进」这件事**结构上不可能**。
        registerSection("landing", landing, { title: "落地页检查" });

        // -- switch to a view that has a component and check the render path --
        // ⚠️ **落地视图（`embedded`）的正文检查在 `landingChecks` 里**（D-33 ·
        // 裁决 C：落地视图现在**有** M-02 组件了 ⇒ 它的正文必须非空且有实质
        // 内容）。这里切到 `regmem` 是为了**渲染路径**本身（白屏回归守卫，
        // FRONTEND_Review.md #20）⇒ 后续那些需要切视图的步骤**不动**。
        await runEffect(cdp, 'location.hash = "#/regmem";');
        let render = [];
        const renderDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < renderDeadline) {
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            render = probe ? renderChecks(probe) : [];
            if (render.length > 0 && render.every((c) => c.ok)) break;
            await sleep(250);
        }
        registerSection("render", render, { title: "渲染路径检查（切到 #/regmem）" });

        // -- drive the REAL kernel -------------------------------------------
        //  Send a `step` command through the view layer's signal and check
        //  that the register view shows the value the **WASM kernel**
        //  actually computed (`addi x1, x0, 42` → x1 = 0x2a).
        await runEffect(cdp,
            'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step" } }));');
        let kernelExec = [];
        const execDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < execDeadline) {
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            kernelExec = probe ? kernelExecChecks(probe) : [];
            if (kernelExec.length > 0 && kernelExec.every((c) => c.ok)) break;
            await sleep(250);
        }
        registerSection("kernelExec", kernelExec, { title: "真内核执行检查（发一条 step 命令）" });

        // -- the source view really asks for the CURRENT PC (VB-7) ------------
        //  ⚠️ 必须在**内核已经单步过**之后：当前 PC 是那一步推进的。
        //  这条链路是「源码视图收到 `sim.pc` ⇒ 按钮的 `data-addr`」——
        //  V-B 检视的 VB-1 断的就是它（源码视图收不到 `sim.pc` ⇒ 按钮
        //  **恒问地址 0**），而 Node 门禁 243 条**全绿**（用例手工注入
        //  了 `sim.pc`）⇒ 只有真机断言抓得到。
        await runEffect(cdp, 'location.hash = "#/source";');
        let source = [];
        const sourceDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < sourceDeadline) {
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            source = probe ? sourceChecks(probe) : [];
            if (source.length > 0 && source.every((c) => c.ok)) break;
            await sleep(250);
        }

        // -- the button really dispatches `resolve-line` with that address ----
        //  ⚠️ **等上面全绿之后才点**：那时按钮上的 `data-addr` 已经等于当
        //  前 PC，否则点下去抓到的是 0（那是时序问题，不是被测缺陷）。
        await runEffect(cdp, CAPTURE_RESOLVE_LINE_EXPRESSION);
        await runEffect(cdp, CLICK_RESOLVE_LINE_EXPRESSION);
        let sourceCmd = null;
        const sourceCmdDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < sourceCmdDeadline) {
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            sourceCmd = probe ? sourceCommandCheck(probe) : null;
            if (sourceCmd && sourceCmd.ok) break;
            await sleep(250);
        }
        source.push(sourceCmd ?? {
            name: "点按钮真的发出 resolve-line 命令（地址等于当前 PC）",
            ok: false,
            detail: "没取到命令信号",
        });
        registerSection("source", source, { title: "源码视图检查（切到 #/source）" });

        // -- drive the probe pipeline (D-46) ----------------------------------
        //  The demo firmware writes `GPIO0.OUT` on its 8th instruction, so
        //  the program has to run that far. The single `step` above only
        //  ran one instruction (it is the one the `x1 == 42` assertion
        //  checks), so push a `step10` on top of it — 1 + 10 instructions
        //  is past the GPIO write.
        await runEffect(cdp,
            'window.dispatchEvent(new CustomEvent("riscv:command", { detail: { action: "step10" } }));');
        await runEffect(cdp, 'location.hash = "#/waveform";');
        let waveform = [];
        const waveDeadline = Date.now() + opts.timeoutMs;
        while (Date.now() < waveDeadline) {
            const value = await awaitEval(cdp, PROBE_EXPRESSION);
            probe = value ?? null;
            waveform = probe ? waveformChecks(probe) : [];
            if (waveform.length > 0 && waveform.every((c) => c.ok)) break;
            await sleep(250);
        }
        registerSection("waveform", waveform, { title: "探针流检查（切到 #/waveform）" });

        // -- 固件加载 UI（STAGE-32 · v0.47 · B 组）------------------------
        // ⚠️ **必须排在 osz 段之前**：这一段把当前镜像换成夹具 ⇒ ⇒ 放在
        // osz 后面，osz 段会看到的是夹具而不是它自己装的那张板。
        //
        // ⚠️ **必须排在波形段之后**：波形段断言的是 **demo 固件**的
        // `GPIO0.OUT` 边沿 ⇒ ⇒ 这一段把镜像换掉之后，那条边沿就没了。
        const loadImage = await runSection("loadImage", () => runLoadImageSection(cdp, opts));
        registerSection("loadImage", loadImage.checks, {
            title: "固件加载 UI 检查（界面上真选文件 → 装载 → 跑起来 → 内核真的换了镜像）",
        });

        // -- the REAL RTOS (osz): load the out-of-repo ELF and SEE its console -
        // ⚠️ 这一段在**没有 osz ELF** 的机器上必须**跳过**（不是判负）——
        // osz 的产物在**仓外**（不进仓库）。
        const osz = await runSection("osz", () => runOszSection(cdp, opts));
        registerSection("osz", osz.checks, {
            title: "真实 RTOS 检查（osz：装载仓外 ELF → 跑 → 终端里的 console）",
            skippedTitle: "真实 RTOS 检查（osz）",
            skipped: osz.skipped,
            reason: osz.reason,
        });

        // -- the board memory map (STAGE-14 · C-8) --------------------------
        // ⚠️ **必须排在 osz 之后**：用户的板一旦配过就**一直**保持 ⇒ 放在
        // osz 前面会把 osz 要用的那张板换掉。
        const memoryMap = await runSection("memoryMap", () => runMemoryMapSection(cdp, opts));
        registerSection("memoryMap", memoryMap.checks, {
            title: "板级内存图检查（配一张板 → 映射表变新图 → 新 RAM 段可写可读）",
        });

        // -- the board ARCHIVE (STAGE-15 · T-3) ------------------------------
        // ⚠️ **必须排在板级内存图段之后**：这一段会**提交一张新板**
        // （落库门槛要求「已被内核接受」）⇒ 放在前面会把 osz 段要用的
        // 那张板换掉。
        const boardArchive = await runSection("boardArchive", () => runBoardArchiveSection(cdp, opts));
        registerSection("boardArchive", boardArchive.checks, {
            title: "板档案检查（提交 → 保存 → 刷新页面 → 板还在且逐字段相同）",
        });

        // -- the board DELTA + UNLOAD (STAGE-16 · T-1 / T-2 / T-3) ------------
        // ⚠️ **必须排在板档案段之后**：这一段会**卸载工程** ⇒ 内核重启、用户
        // 的板不存在 ⇒ 放在前面会把板档案段要用的板换掉。
        const boardDelta = await runSection("boardDelta", () => runBoardDeltaSection(cdp, opts));
        registerSection("boardDelta", boardDelta.checks, {
            title: "板级差量与卸载检查（卸载 → 改板立即生效 → 只改一段走差量 → 修订号 +1 → 差量 ≡ 整份）",
        });

        // -- the board ORDERING (STAGE-17 · T-6) --------------------------------
        // ⚠️ **必须排在差量段之后**：这一段会**注入一个更旧的内存图事件**
        // ⇒ ⇒ 前面的段看到的必须是**没被污染过**的那份图。
        const boardOrdering = await runSection("boardOrdering", () => runBoardOrderingSection(cdp, opts));
        registerSection("boardOrdering", boardOrdering.checks, {
            title: "乱序到达检查（注入更旧的 memory.map 事件 → 真提交 → 真内核接受）",
        });

        // -- report ------------------------------------------------------------
        //
        // ⚠️ **按登记顺序打印**（裁决 I）⇒ ⇒ 报告的可读顺序 = 段的运行顺序。
        for (const section of SECTION_REGISTRY) {
            console.log("");
            if (section.skipped) {
                console.log(`${section.skippedTitle ?? section.title}：skipped（${section.reason}）`);
                continue;
            }
            console.log(`${section.title}：`);
            for (const check of section.checks) {
                console.log(`  ${check.ok ? "✓" : "✗"} ${check.name} — ${check.detail}`);
            }
        }
        if (consoleErrors.length > 0) {
            console.log("");
            console.log("控制台错误：");
            for (const err of consoleErrors) console.log(`  ✗ ${err}`);
        }

        // -- the verdict --------------------------------------------------------
        //
        // ⚠️ **聚合只从注册表取**（裁决 I）⇒ ⇒ **不存在**手写的展开串 ⇒ ⇒
        // 「新加一段忘了并进」**结构上不可能**。
        const failed = SECTION_REGISTRY.flatMap((s) => s.checks).filter((c) => !c.ok);
        // ⚠️ **期望段名表**（裁决 J）：登记的段名集合必须**等于**那张表 ⇒ 不等
        // 就**红** ⇒ ⇒ 兜住「跑过了但没登记」与「登记了但不在期望表里」。
        const diff = registryDiff();
        console.log("");
        console.log("注册表一致性检查（期望段名表 vs 实际登记）：");
        console.log(`  ${diff.ok ? "✓" : "✗"} 登记的段名集合 == 期望段名表`
            + `（期望 ${EXPECTED_SECTION_NAMES.length} 段，实际 ${SECTION_REGISTRY.length} 段）`
            + (diff.missing.length > 0 ? `；**该有却没有**：${diff.missing.join(" / ")}` : "")
            + (diff.extra.length > 0 ? `；**多出来没有**：${diff.extra.join(" / ")}` : "")
            + (diff.duplicated.length > 0 ? `；**重复登记**：${diff.duplicated.join(" / ")}` : ""));
        const ok = failed.length === 0 && consoleErrors.length === 0 && diff.ok;
        console.log("");
        console.log(ok
            ? "browser-verify: 通过（真机挂载 + 渲染路径 + 真内核执行 + 源码视图 + 探针流 + 板级内存图 + 板档案 + 板级差量/卸载 + 乱序到达 + 注册表一致 + 控制台）"
            : `browser-verify: 失败（${failed.length} 项断言未通过，${consoleErrors.length} 条控制台错误，`
              + `注册表一致性 ${diff.ok ? "通过" : "**不通过**"}）`);
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
