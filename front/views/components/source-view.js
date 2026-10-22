// M-02 · 源码视图的**纯逻辑**（STAGE-7 裁决 E / F）。
//
// ⚠️ 这个模块**只**导出纯函数：不碰 DOM、不碰服务层、不发命令 ⇒
// `front/tests/source-view.test.mjs`（node:test）可以直接测它。
//
// # 裁决 E：源码**不进内核**
//
// 源码文本是**调试器客户端的资产**，不是仿真状态 ⇒ 前端本地读文件 ⇒
// 存进 M-03 视图状态 ⇒ **不发**给内核、**不新增**任何契约命令。
//
// # 裁决 F：路径**尾部逐段匹配**，唯一命中才用
//
// DWARF 里的 `file` 是**编译期路径**（可能是 `../../src/main.c` 或
// `/tmp/proj/src/main.c`）；本地源码树的 key 是**相对路径**（如
// `src/main.c`）。两者不一定逐字相等 ⇒ 规则是：
//
// 1. 两边都按 `/` 切段；
// 2. **先正规化**（丢掉 `.` 段、按栈消解 `..` 段）；
// 3. 从**尾部**逐段比，**全部相同** ⇒ 命中候选；
// 4. **唯一命中** ⇒ 直接用它；
// 5. **多个命中** ⇒ **列出候选让用户选**（⛔ 不静默挑第一个）；
// 6. **零命中** ⇒ 显示「匹配不到」。
//
// ⚠️ 第 3 条的「尾部」比的是**较短的那一段**：DWARF 给的是**绝对路径**
// （`/tmp/proj/src/main.c`，8 段）而本地 key 是**相对路径**
// （`src/main.c`，2 段）⇒ 比的是本地 key 的**全部** 2 段与 DWARF 路径的
// **尾部** 2 段。反过来（DWARF 给相对路径、本地 key 更长）也一样。
// 换句话说：**两串段序列，短的那串必须是长的那串的后缀**。
//
// # 三态必须**如实区分**（裁决 J）
//
// 「没有源码树」/「有源码树但匹配不到」/「匹配到多个」——**不合并**成
// 一个「无源码」。

import { escapeHtml } from "../../lib/escape.js";

/**
 * 路径正规化：丢掉 `.` 段、按栈消解 `..` 段。
 *
 * ⚠️ **消解不掉的（前导）`..` 段被丢弃**：尾部匹配只需要尾部；留着它们
 * 会让 `../../src/main.c` 永远匹配不到 `src/main.c`（两边的段数不等）。
 *
 * @param {unknown} path
 * @returns {string} 正规化后的路径（`/` 分隔）
 */
export function normalizePath(path) {
    const raw = String(path ?? "").replace(/\\/g, "/");
    const absolute = raw.startsWith("/");
    const out = [];
    for (const seg of raw.split("/")) {
        if (seg === "" || seg === ".") continue;
        if (seg === "..") {
            // 能消解就消解（前一段被顶掉）；消解不掉就**丢弃**。
            if (out.length > 0) out.pop();
            continue;
        }
        out.push(seg);
    }
    return (absolute ? "/" : "") + out.join("/");
}

/**
 * 源码树的 key 列表（`Map` 或普通对象都接受）。
 *
 * @param {Map<string, string>|object|null|undefined} tree
 * @returns {string[]}
 */
function treeKeys(tree) {
    if (!tree) return [];
    if (typeof tree.get === "function") return [...tree.keys()];
    if (typeof tree === "object") return Object.keys(tree);
    return [];
}

/**
 * @typedef {{kind: "none", reason: "no-tree"|"no-match"}
 *   | {kind: "unique", key: string}
 *   | {kind: "ambiguous", candidates: string[]}} SourceMatch
 */

/**
 * 把 DWARF 的路径匹配到本地源码树的一个 key 上。
 *
 * ⚠️ **三态如实区分**（裁决 J / F）：
 * - `{ kind: "none", reason: "no-tree" }` —— **根本没有源码树**；
 * - `{ kind: "none", reason: "no-match" }` —— 有源码树，但**匹配不到**；
 * - `{ kind: "unique", key }` —— 唯一命中；
 * - `{ kind: "ambiguous", candidates }` —— 多个命中（**列出候选**，
 *   ⛔ **不静默挑第一个**）。
 *
 * @param {Map<string, string>|object|null|undefined} tree — 源码树
 * @param {unknown} dwarfPath — DWARF 里的编译期路径
 * @returns {SourceMatch}
 */
export function matchSourceFile(tree, dwarfPath) {
    const keys = treeKeys(tree);
    if (keys.length === 0) {
        // 没有源码树 ⇒ 与「匹配不到」**不是**一回事。
        return { kind: "none", reason: "no-tree" };
    }
    const target = normalizePath(dwarfPath);
    const targetSegs = target.split("/").filter(Boolean);
    if (targetSegs.length === 0) {
        return { kind: "none", reason: "no-match" };
    }
    const candidates = [];
    for (const key of keys) {
        const keySegs = normalizePath(key).split("/").filter(Boolean);
        if (keySegs.length === 0) continue;
        // 尾部逐段比：比**较短的那一段**（短的必须是长的后缀）。
        const count = Math.min(keySegs.length, targetSegs.length);
        const keyOffset = keySegs.length - count;
        const targetOffset = targetSegs.length - count;
        let same = true;
        for (let i = 0; i < count; i += 1) {
            if (keySegs[keyOffset + i] !== targetSegs[targetOffset + i]) {
                same = false;
                break;
            }
        }
        if (same) candidates.push(key);
    }
    if (candidates.length === 0) return { kind: "none", reason: "no-match" };
    if (candidates.length === 1) return { kind: "unique", key: candidates[0] };
    // 多个命中 ⇒ 列出候选（排序 ⇒ 结果确定）。⛔ 不静默挑第一个。
    return { kind: "ambiguous", candidates: [...candidates].sort() };
}

/**
 * 三态 → 如实的提示文案。
 *
 * @param {SourceMatch|null|undefined} match
 * @returns {string}
 */
export function describeMatch(match) {
    if (!match || match.kind === "none") {
        if (match?.reason === "no-tree") {
            return "尚未装载源码 — 用上面的入口选一个目录或若干文件。";
        }
        return "源码树里匹配不到这个文件（按路径尾部逐段比）。";
    }
    if (match.kind === "unique") return `命中：${match.key}`;
    return `匹配到 ${match.candidates.length} 个候选 — 请选一个（不替你挑）。`;
}

/**
 * 渲染源码文本，**当前行高亮**。
 *
 * ⚠️ 文本走 `escapeHtml`（NFR-04）：源码里出现 `<` / `&` 不得变成标记。
 *
 * ⚠️ 高亮用**内联样式**（而不是一条 CSS 规则）：`front/style.css` **不在
 * 本 lane 的文件所有权清单内** ⇒ 不去动它 ⇒ 高亮色与反汇编面板的 PC 行
 * 保持一致（同一个琥珀色）。等样式表归谁管时，再把它挪进 `.src-line
 * .is-current` 规则里（类名已经留好了）。
 *
 * @param {unknown} text — 源码文本
 * @param {number} currentLine — 当前行号（1 起；0 / 非法 ⇒ 不高亮）
 * @returns {string} HTML 片段
 */
export function renderSourceText(text, currentLine) {
    const lines = String(text ?? "").split("\n");
    const current = Number(currentLine);
    const wanted = Number.isFinite(current) && current > 0 ? current : 0;
    return lines
        .map((line, idx) => {
            const no = idx + 1;
            const isCurrent = wanted !== 0 && no === wanted;
            const style = isCurrent ? ' style="background: rgba(255, 193, 7, 0.14)"' : "";
            return (
                `<div class="src-line${isCurrent ? " is-current" : ""}" data-src-line="${no}"${style}>` +
                `<span class="src-line-no">${no}</span>` +
                `<span class="src-line-text">${escapeHtml(line)}</span>` +
                `</div>`
            );
        })
        .join("");
}
