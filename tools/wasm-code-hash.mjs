#!/usr/bin/env node
/**
 * **WASM 产物的源码指纹**（v0.83 · 门禁提速）。
 *
 * ⚠️ **为什么需要这个**：门禁原来用 **mtime** 判「wasm 是不是比源码旧」⇒ ⇒
 *   改一行**测试**、改一句**注释**、甚至 `touch` 一下，都会触发一次
 *   `cargo build --target wasm32`（实测**约 2 分钟**）⇒ ⇒ 每趟门禁固定白花。
 *
 * ⚠️ **指纹的口径 = 真正会进产物的那部分代码**：
 *   ① 去掉 `#[cfg(test)]` 模块（以及紧跟的 `mod tests { … }` 整块）；
 *   ② 去掉行注释（`//…`，含 `///`）；
 *   ③ 去掉空行。
 *   ⇒ ⇒ 于是「只改测试 / 只改注释」的改动**指纹不变** ⇒ 不重建；
 *   「改了真代码」的改动**指纹变** ⇒ 重建（判据没有被放宽）。
 *
 * ⚠️ **两种用法**：
 *   · `node tools/wasm-code-hash.mjs` ⇒ 打印 JSON（`{相对路径: 指纹}`）；
 *   · `node tools/wasm-code-hash.mjs --check <stamp.json>` ⇒ 与构建时的
 *     指纹表比对，打印**变了的文件**（门禁用这个）。
 *
 * ⚠️ 指纹表由 `tools/build-wasm.sh` 在**构建成功后**写入
 *   `front/assets/pkg/.wasm-code.json` ⇒ ⇒ 产物的「我是由这版代码构建的」
 *   这件事**有据可查**，不再靠时间戳猜。
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)));

/**
 * 去掉 `#[cfg(test)]` 的整个模块块（含其中的 `mod tests { … }`）。
 *
 * ⚠️ **按行扫描 + 花括号计数**：`#[cfg(test)]` 下一行通常是 `mod tests {`，
 *   也可能是 `#[cfg(test)]` 直接修饰 `fn`（那种只跳一行）⇒ ⇒ 两种都覆盖。
 *
 * @param {string[]} lines
 * @returns {string[]}
 */
function stripTestModules(lines) {
    const out = [];
    for (let i = 0; i < lines.length; i += 1) {
        const line = lines[i];
        if (!/^\s*#\[cfg\(test\)\]/.test(line)) {
            out.push(line);
            continue;
        }
        // 找到块起点：`#[cfg(test)]` 后面第一个含 `{` 的行。
        let j = i;
        let depth = 0;
        let seenBrace = false;
        for (; j < lines.length; j += 1) {
            const cur = lines[j];
            const opens = (cur.match(/\{/g) ?? []).length;
            const closes = (cur.match(/\}/g) ?? []).length;
            if (opens > 0) seenBrace = true;
            depth += opens - closes;
            if (seenBrace && depth <= 0) break;
            if (!seenBrace && /;/.test(cur)) break; // 属性修饰 fn 声明（无块）
        }
        i = j; // 整个块吃掉
    }
    return out;
}

/**
 * 单个 `.rs` 文件的**代码指纹**。
 *
 * @param {string} text
 * @returns {string}
 */
function codeHash(text) {
    const lines = stripTestModules(text.split("\n"));
    const kept = lines
        .map((line) => line.replace(/\/\/.*$/, "").trimEnd())
        .filter((line) => line.trim().length > 0);
    return createHash("sha256").update(kept.join("\n")).digest("hex").slice(0, 16);
}

/**
 * 全部 `crates/**​/*.rs` 的指纹表。
 *
 * @returns {Record<string, string>}
 */
function fingerprintAll() {
    const cratesDir = join(repoRoot, "crates");
    const table = {};
    if (!existsSync(cratesDir)) return table;
    const walk = (dir) => {
        for (const entry of readdirSync(dir, { withFileTypes: true })) {
            const full = join(dir, entry.name);
            if (entry.isDirectory()) {
                walk(full);
            } else if (entry.isFile() && entry.name.endsWith(".rs")) {
                table[relative(repoRoot, full)] = codeHash(readFileSync(full, "utf8"));
            }
        }
    };
    walk(cratesDir);
    return table;
}

const argv = process.argv.slice(2);
if (argv[0] === "--check") {
    const stampPath = argv[1];
    if (!stampPath || !existsSync(stampPath)) {
        console.error("检查失败：没有构建指纹表（先跑一次 tools/build-wasm.sh）");
        process.exit(2);
    }
    const stamp = JSON.parse(readFileSync(stampPath, "utf8"));
    const now = fingerprintAll();
    const changed = [];
    const added = [];
    for (const [file, hash] of Object.entries(now)) {
        if (!(file in stamp)) added.push(file);
        else if (stamp[file] !== hash) changed.push(file);
    }
    const removed = Object.keys(stamp).filter((f) => !(f in now));
    console.log(JSON.stringify({ changed, added, removed }));
} else {
    console.log(JSON.stringify(fingerprintAll()));
}
