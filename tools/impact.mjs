#!/usr/bin/env node
// 按**影响面**选测：改了哪些文件 ⇒ 只跑哪几段门禁 / 哪几个前端用例 / 哪几个 crate。
//
// ## 为什么要有这个工具
//
// 收口时全量门禁约 4~15 分钟、前端 758 条用例、内核几百条单测。改一行注释也跑
// 一遍全量，代价高到没有人愿意跑 ⇒ ⇒ 于是「改完就跑一遍」变成了「改完不跑，
// 等收口再说」⇒ ⇒ 红点攒到收口才暴露。选测把「改完立刻验」这件事变得**可负担**。
//
// ## 两条硬规矩
//
// ① **不准漏**：选测只许**缩小**必跑集合，**不许**把「可能被影响的东西」排除在
//    外。映射不出来 / 扇出太大 / 属于共享底座 ⇒ ⇒ **一律收敛到全量**。所以
//    「全量红、选测绿」这种情况只可能来自映射表写错，不可能来自「这条判据本来
//    就不在必跑集合里」。
// ② **前端用例的集合是算出来的，不是抄出来的**：按 `front/tests/*.test.mjs`
//    的**静态 import 闭包**推导 —— 改了 `front/lib/x.js`，只有（直接或间接）
//    import 了它的用例被选中。用手工清单迟早会和真实依赖脱节。
//
// ## 与门禁的关系
//
// 本工具只产出「计划」，**不执行**判别：
//   - `tools/browser-verify.mjs --impact` 读本工具的计划，只跑计划里的段；
//   - `tools/check-front.sh --impact` 读计划，只跑计划里的用例文件。
// 两者都**保留**全量入口：收口判据仍然是「全量 0 ✗」，选测只用于定位期
// （与 R-1 的同一条口径）。
//
// ## 用法
//
//   node tools/impact.mjs --base origin/main            # 人类可读的计划
//   node tools/impact.mjs --base origin/main --json      # 给脚本用
//   node tools/impact.mjs --front-tests --base origin/main  # 只打印前端用例文件
//   node tools/impact.mjs --paths front/lib/trace-export.js # 模拟一组改动（演练）
//   node tools/impact.mjs --selftest                     # 映射规则自检

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");

/** 门禁的 31 段（与 `tools/browser-verify.mjs` 的 EXPECTED_SECTION_NAMES 一一对应）。 */
const ALL_SECTIONS = Object.freeze([
    "landing", "render", "kernelExec", "source", "waveform", "waveChart",
    "loadImage", "osz", "terminalScreen", "memoryMap", "unknownSystem",
    "platformSchemaV2", "mcuConcept", "boardArchive", "boardDelta",
    "boardOrdering", "uiLayout", "breakpointUi", "liveReadout", "runLoop",
    "addrInput", "symbolUi", "memDump", "symbolBreakpoint", "uartEcho",
    "traceSwitch", "traceExport", "debugDedup", "deviceRegistry", "readback",
    "staleSurfaces",
]);

/**
 * **窄映射表**：只有「改动面确实很窄」的路径才写在这里。
 *
 * ⚠️ 规则：**能窄才窄**。任何一条判断上没把握的路径，**不要**进这张表 ——
 *   落到 `classify()` 的默认分支（全量）反而更安全。表的收益是「少跑」，成本
 *   是「漏跑」；漏跑一次就会让门禁的结论失去意义，所以宁可少写几条。
 *
 * 格式：`[相对仓根的 glob, {sections?: string[], doctest?: boolean}]`
 *   - `sections: []` ⇒ ⇒ 本文件改动**不需要**跑真机门禁（只影响 doc/单测）；
 *   - 缺省 `sections` ⇒ ⇒ **全量**。
 */
const NARROW = [
    // ---- 文档：只影响文档台账检查 ----
    ["docs/**", { sections: [], docsOnly: true }],

    // ---- 门禁工具自身：改了它就得跑**全量**（它是判据本身） ----
    ["tools/browser-verify.mjs", { sections: null }],

    // ---- 纯逻辑模块：各自只被一小撮段用到 ----
    ["front/lib/trace-export.js", {
        sections: ["traceExport"],
    }],
    ["front/lib/trace-switch.js", {
        sections: ["traceSwitch", "debugDedup"],
    }],
    ["front/lib/run-loop.js", {
        sections: ["runLoop", "liveReadout", "debugDedup"],
    }],
    ["front/lib/waveform-data.js", {
        sections: ["waveform", "waveChart"],
    }],
];

/** 「共享底座」——碰了就全量（不是靠枚举，而是靠这条兜底）。 */
const FANOUT = [
    "front/app.js",
    "front/index.html",
    "front/views/m01_workbench.js",
    "front/views/m02_viewcomponents.js",
    "front/views/m03_viewstore.js",
    "front/lib/message.js",
    "front/lib/command-router.js",
    "crates/kernel/**",
    "crates/golden/**",
    "Cargo.toml",
    "Cargo.lock",
];

/** 把 glob 转成锚定的正则（只支持 `**` 与 `*` 两种通配）。 */
function globToRegExp(glob) {
    const esc = glob.replace(/[.+^${}()|[\]\\]/g, "\\$&");
    const body = esc
        .replace(/\*\*/g, "\u0000DS\u0000")
        .replace(/\*/g, "[^/]*")
        .split("\u0000DS\u0000").join(".*");
    return new RegExp(`^${body}$`);
}

const NARROW_RULES = NARROW.map(([glob, plan]) => ({ re: globToRegExp(glob), plan }));
const FANOUT_RULES = FANOUT.map((glob) => globToRegExp(glob));

/**
 * 读「相对 `base` 的改动文件」。**包含**工作区未提交与未跟踪文件 ——
 * 选测是给「改完立刻验」用的，没提交的改动恰恰是最需要验的那一批。
 *
 * @param {string} base — 例如 `origin/main`
 * @returns {string[]} 相对仓根的文件路径
 */
function changedFiles(base) {
    const out = new Set();
    const git = (args) => {
        const r = spawnSync("git", args, { cwd: ROOT, encoding: "utf8" });
        if (r.status !== 0) {
            throw new Error(`git ${args.join(" ")} 失败：${String(r.stderr ?? "").trim()}`);
        }
        return String(r.stdout ?? "").split("\n").map((s) => s.trim()).filter(Boolean);
    };
    // 已提交的差异（三点：base 与 HEAD 的公共祖先到 HEAD）
    for (const f of git(["diff", "--name-only", `${base}...HEAD`])) out.add(f);
    // 工作区（未暂存 + 已暂存）
    for (const f of git(["diff", "--name-only", "HEAD"])) out.add(f);
    // 未跟踪
    for (const f of git(["ls-files", "--others", "--exclude-standard"])) out.add(f);
    return [...out];
}

/**
 * 判断一个改动文件对**真机门禁**的影响。
 *
 * @param {string} file — 相对仓根
 * @returns {{sections: string[]|null, reason: string, docsOnly?: boolean}}
 *   `null` 表示**全量**。
 */
function classify(file) {
    if (FANOUT_RULES.some((re) => re.test(file))) {
        return { sections: null, reason: "共享底座（扇出到全部段）" };
    }
    const hit = NARROW_RULES.find((r) => r.re.test(file));
    if (hit) {
        const p = hit.plan;
        return {
            sections: p.sections === null ? null : (p.sections ?? null),
            reason: `窄映射（${file}）`,
            docsOnly: p.docsOnly === true,
        };
    }
    // 认不出来 ⇒ ⇒ 全量（规矩①：宁可多跑）
    return { sections: null, reason: `未在窄映射表里（${file}）⇒ ⇒ 保守跑全量` };
}

/** 列出 `front/tests/*.test.mjs` / `tools/**`;
 * 解析一个文件的**本地 import 目标**（相对路径 / 绝对路径，忽略 node: 与包名）。 */
function localImports(file) {
    const text = readFileSync(file, "utf8");
    const out = [];
    // ⚠️ **不要**写「`import ... from` + 任意字符 + 回溯」那种正则 —— 在一两千行
    //  的大文件上会**灾难性回溯**（实测直接把工具挂住）。这里只抓 `from "x"`
    //  与 `import("x")` 两种**定界清晰**的形态。
    const specs = [];
    for (const m of text.matchAll(/\bfrom\s*(["'])([^"'\n]+)\1/g)) specs.push(m[2]);
    for (const m of text.matchAll(/\bimport\s*\(\s*(["'])([^"'\n]+)\1/g)) specs.push(m[2]);
    for (const spec of specs) {
        if (/^(node:|data:)/.test(spec)) continue;
        if (!spec.startsWith(".") && !spec.startsWith("/")) continue;
        const target = resolve(dirname(file), spec);
        // ⚠️ 只认**文件**：`import "../views"` 这类目录引用不参与闭包。
        if (existsSync(target) && statSync(target).isFile()) out.push(target);
    }
    // ⚠️ 前端用例大量走 `importFront("lib/xxx.js")`（先 `pathToFileURL` 再 `import`）
    //   —— 那种写法**不是**静态 import ⇒ ⇒ 上面那轮抓不到 ⇒ ⇒ 这里把**所有**
    //   长得像模块路径的字符串字面量也当依赖候选（相对 `front/` 或本文件目录）。
    for (const m of text.matchAll(/(["'])([\w./-]+\.js)\1/g)) {
        const spec = m[2];
        for (const base of [join(ROOT, "front"), dirname(file)]) {
            const target = resolve(base, spec);
            if (existsSync(target) && statSync(target).isFile()) {
                out.push(target);
                break;
            }
        }
    }
    return out;
}

/**
 * 前端用例的**影响闭包**：
 *   改了哪些 front 文件 ⇒ 哪些 `front/tests/*.test.mjs` 会（直接或间接）读到它。
 *
 * ⚠️ 两条都算：
 *   ① **import 闭包**（真依赖）；
 *   ② **正文里出现了该路径字符串**（有些用例是「读源码文本断言」的，不 import
 *      也依赖它 —— 例如 grep 某个函数名还在不在）。
 *
 * @param {string[]} files — 相对仓根的改动文件
 * @returns {string[]} 需要跑的用例文件（相对仓根）
 */
function frontTestsFor(files) {
    const testDir = join(ROOT, "front", "tests");
    if (!existsSync(testDir)) return [];
    const tests = readdirSync(testDir)
        .filter((n) => n.endsWith(".test.mjs"))
        .map((n) => join(testDir, n));

    const changed = files
        .filter((f) => f.startsWith("front/"))
        .map((f) => resolve(ROOT, f));
    if (changed.length === 0) return [];

    const selected = new Set();
    for (const test of tests) {
        // ② 源码文本里点名了这个文件
        const text = readFileSync(test, "utf8");
        const mentioned = files.some((f) => f.startsWith("front/") && text.includes(f));
        // ① import 闭包（BFS）
        let depends = false;
        const seen = new Set([test]);
        const queue = [test];
        while (queue.length > 0 && !depends) {
            const cur = queue.pop();
            for (const dep of localImports(cur)) {
                if (seen.has(dep)) continue;
                if (changed.includes(dep)) { depends = true; break; }
                seen.add(dep);
                if (dep.startsWith(join(ROOT, "front"))) queue.push(dep);
            }
        }
        if (mentioned || depends) selected.add(relative(ROOT, test));
    }
    return [...selected].sort();
}

/**
 * 改了哪些文件 ⇒ 哪些 cargo 包（`crates/<pkg>` ⇒ `-p <pkg>`）。
 *
 * @param {string[]} files
 * @returns {string[]}
 */
function cargoPackagesFor(files) {
    const pkgs = new Set();
    for (const f of files) {
        const m = /^crates\/([^/]+)\//.exec(f);
        if (m) pkgs.add(m[1]);
    }
    return [...pkgs].sort();
}

/**
 * 汇总成一份**计划**。
 *
 * @param {string[]} files — 相对仓根的改动文件
 * @returns {{full: boolean, sections: string[], reasons: string[], frontTests: string[],
 *           cargo: string[], docs: boolean}}
 */
function planFor(files) {
    const reasons = [];
    let full = false;
    let docs = false;
    const sections = new Set();

    if (files.length === 0) {
        return { full: false, sections: [], reasons: ["无改动"], frontTests: [], cargo: [], docs: false };
    }
    for (const f of files) {
        const c = classify(f);
        if (c.docsOnly) docs = true;
        if (c.sections === null) {
            full = true;
            reasons.push(c.reason);
            continue;
        }
        for (const s of c.sections) sections.add(s);
    }
    // ⚠️ **一个 section 都没命中、又不是纯文档/工具改动** ⇒ ⇒ 也收敛到全量：
    //   「没有任何段覆盖这块代码」意味着**覆盖缺口**，不是「不用跑」。
    if (!full && sections.size === 0 && !docs) {
        full = true;
        reasons.push("改动没有命中任何段（覆盖缺口 ⇒ 保守跑全量）");
    }
    return {
        full,
        sections: full ? [...ALL_SECTIONS] : [...sections].sort(),
        reasons,
        frontTests: frontTestsFor(files),
        cargo: cargoPackagesFor(files),
        docs,
    };
}

/** 人类可读地打印计划。 */
function printPlan(plan, files) {
    console.log(`改动 ${files.length} 个文件`);
    if (files.length > 0 && files.length <= 12) {
        for (const f of files) console.log(`  · ${f}`);
    } else if (files.length > 12) {
        for (const f of files.slice(0, 8)) console.log(`  · ${f}`);
        console.log(`  · … 其余 ${files.length - 8} 个`);
    }
    console.log("");
    if (plan.full) {
        console.log("真机门禁：**全量**（31 段）");
        console.log(`  理由：${plan.reasons.join(" / ")}`);
    } else {
        console.log(`真机门禁：${plan.sections.length} 段 — ${plan.sections.join(", ")}`);
    }
    console.log(`前端用例：${plan.frontTests.length} 个文件`
        + (plan.frontTests.length ? ` — ${plan.frontTests.map((p) => p.replace("front/tests/", "")).join(", ")}` : ""));
    console.log(`内核单测：${plan.cargo.length ? `cargo test -p ${plan.cargo.join(" -p ")}` : "（无改动）"}`);
    if (plan.docs) console.log("文档台账：需要跑 tools/doc_check.py");
}

const argv = process.argv.slice(2);
const opts = { base: "origin/main", json: false, frontTests: false, selftest: false, paths: null };
for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === "--base") opts.base = String(argv[++i]);
    else if (a === "--json") opts.json = true;
    else if (a === "--front-tests") opts.frontTests = true;
    else if (a === "--selftest") opts.selftest = true;
    else if (a === "--paths") {
        opts.paths = [];
        while (i + 1 < argv.length && !argv[i + 1].startsWith("--")) opts.paths.push(argv[++i]);
    } else if (a === "--help" || a === "-h") {
        console.log("用法：node tools/impact.mjs [--base <rev>] [--json] [--front-tests] [--paths <file...>] [--selftest]");
        process.exit(0);
    } else {
        console.error(`未知参数：${a}`);
        process.exit(1);
    }
}

if (opts.selftest) {
    // 映射规则的**不变量**（与具体改动无关）：
    // ① 共享底座碰了就是全量；② 文档只影响 doc；③ 认不出来的文件收敛全量。
    const cases = [
        ["front/app.js", true],
        ["front/views/m02_viewcomponents.js", true],
        ["crates/kernel/src/lib.rs", true],
        ["docs/progress/PROGRESS.md", false],
        ["front/lib/trace-export.js", false],
        ["some/rand0m.cfg", true],
    ];
    let ok = true;
    for (const [file, expectFull] of cases) {
        const p = planFor([file]);
        if (p.full !== expectFull) {
            ok = false;
            console.error(`✗ selftest: ${file} 期望 full=${expectFull}，实得 ${p.full}`);
        }
    }
    const doc = planFor(["docs/progress/PROGRESS.md"]);
    if (!(doc.docs === true && doc.sections.length === 0)) {
        ok = false;
        console.error("✗ selftest: 纯文档改动应当 docs=true 且不跑段");
    }
    const narrow = planFor(["front/lib/trace-export.js"]);
    if (!(narrow.full === false && narrow.sections.includes("traceExport"))) {
        ok = false;
        console.error("✗ selftest: trace-export.js 应当窄映射到 traceExport");
    }
    if (!ok) process.exit(1);
    console.log("selftest：映射规则 8 条不变量全部成立");
    process.exit(0);
}

const files = opts.paths ?? changedFiles(opts.base);
const plan = planFor(files);

if (opts.frontTests) {
    for (const f of plan.frontTests) console.log(f);
    process.exit(0);
}
if (opts.json) {
    console.log(JSON.stringify({ files, ...plan }, null, 2));
    process.exit(0);
}
printPlan(plan, files);
