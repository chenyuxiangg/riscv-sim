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
// 3. 从**尾部**逐段比，**全部相同** ⇒ 命中候选；本地 key 可先丢掉任意
//    前导段（目录选择器带的选中文件夹名，与编译机 `comp_dir` 尾部本就不同）；
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
//
// # v0.50（STAGE-35）：**两栏形态** —— 左「文件列表」＋ 右「代码区」
//
// ⚠️ 本批**只调形态**（事实 F14）⇒ ⇒ 既有导出的**语义与签名一个字都不改**
//   （`normalizePath` / `matchSourceFile` / `describeMatch` /
//   `renderSourceText`）⇒ ⇒ 真机门禁与前端用例**零改动**（裁决 A：
//   稳定钩子一字不动）。
//
// ⚠️ **新增的导出只是「渲染片段」**（与 `front/views/components/memory-map.js`
//   **同一套**house pattern）：视图层（M-02）负责把它们拼进视图正文 ⇒ ⇒
//   本模块仍然**不碰 DOM、不碰服务层、不发命令**。
//
// ⚠️ **选中语义沿用既有的那条**（裁决 F）：「用户在多个候选里选定的那个」
//   （`source.selected`）优先于「唯一命中的那个」⇒ ⇒ 「匹配到多个」时
//   **没有任何一项**被标成选中（⛔ 不静默挑第一个）。

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
 * 本地 key 的任意后缀与 DWARF 路径的后缀逐段相同 ⇒ 命中。
 *
 * ⚠️ **本地 key 允许丢弃前导段**：目录选择器给的 `webkitRelativePath` 带
 * 选中文件夹名作为第一段（如 `osz/targets/.../main.c`），而 DWARF 绝对
 * 路径的 `comp_dir` 尾部是编译机上的目录名（如 `…/osz-main/targets/...`），
 * 这两个前导段本来就不该相等 ⇒ ⇒ 从 key 的每个后缀起，再按「较短串是
 * 较长串后缀」的规则比；丢掉的只是本地这边的前导前缀，目标侧一段不丢。
 *
 * @param {string[]} keySegs — 已正规化、切段后的本地 key
 * @param {string[]} targetSegs — 已正规化、切段后的 DWARF 路径
 * @returns {boolean}
 */
function keySuffixMatches(keySegs, targetSegs) {
    for (let start = 0; start < keySegs.length; start += 1) {
        const cand = keySegs.slice(start);
        const count = Math.min(cand.length, targetSegs.length);
        const candOffset = cand.length - count;
        const targetOffset = targetSegs.length - count;
        let same = true;
        for (let i = 0; i < count; i += 1) {
            if (cand[candOffset + i] !== targetSegs[targetOffset + i]) {
                same = false;
                break;
            }
        }
        if (same) return true;
    }
    return false;
}

/**
 * 路径串 → 正规化后的段序列（丢空段 / `.`、消解可消 `..`）。
 *
 * @param {unknown} path
 * @returns {string[]}
 */
function segmentsOf(path) {
    return normalizePath(path).split("/").filter(Boolean);
}

/**
 * 一条用户映射：把 DWARF 编译期前缀 `from` 替换成本地前缀 `to`。
 *
 * - `from` 为空 ⇒ 不参与重写；
 * - `to` 为空 ⇒ 剥掉 `from` 后只留 DWARF 里的相对路径；
 * - 段级比较（正规化之后），不拼字符串。
 *
 * @typedef {{from: string, to: string}} PathRemap
 */

/**
 * 对一条 DWARF 段序列套用映射；前缀不匹配 ⇒ 返回 `null`。
 *
 * @param {string[]} dwarfSegs
 * @param {string[]} fromSegs
 * @param {string[]} toSegs
 * @returns {string[]|null}
 */
function applyRemap(dwarfSegs, fromSegs, toSegs) {
    if (fromSegs.length === 0 || fromSegs.length > dwarfSegs.length) return null;
    for (let i = 0; i < fromSegs.length; i += 1) {
        if (dwarfSegs[i] !== fromSegs[i]) return null;
    }
    return [...toSegs, ...dwarfSegs.slice(fromSegs.length)];
}

/**
 * 把 DWARF 路径按映射表重写出若干候选段序列（原始路径始终在最前）。
 *
 * @param {string[]} dwarfSegs
 * @param {Array<PathRemap>|null|undefined} remaps
 * @returns {string[][]}
 */
function remappedVariants(dwarfSegs, remaps) {
    const variants = [dwarfSegs];
    for (const remap of remaps ?? []) {
        const fromSegs = segmentsOf(remap?.from);
        const toSegs = segmentsOf(remap?.to);
        const mapped = applyRemap(dwarfSegs, fromSegs, toSegs);
        if (mapped) variants.push(mapped);
    }
    return variants;
}

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
 * @param {Array<PathRemap>|null|undefined} [remaps] — 用户自填的前缀映射表
 * @returns {SourceMatch}
 */
export function matchSourceFile(tree, dwarfPath, remaps) {
    const keys = treeKeys(tree);
    if (keys.length === 0) {
        // 没有源码树 ⇒ 与「匹配不到」**不是**一回事。
        return { kind: "none", reason: "no-tree" };
    }
    const targetSegs = segmentsOf(dwarfPath);
    if (targetSegs.length === 0) {
        return { kind: "none", reason: "no-match" };
    }
    const variants = remappedVariants(targetSegs, remaps);
    const candidates = [];
    for (const key of keys) {
        const keySegs = segmentsOf(key);
        if (keySegs.length === 0) continue;
        if (variants.some((targetSegs_) => keySuffixMatches(keySegs, targetSegs_))) {
            candidates.push(key);
        }
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
            return "尚未装载源码";
        }
        return "匹配不到该文件";
    }
    if (match.kind === "unique") return `命中：${match.key}`;
    return `匹配到 ${match.candidates.length} 个候选，请选择`;
}

/**
 * 渲染源码文本，**当前行高亮**。
 *
 * ⚠️ 文本走 `escapeHtml`（NFR-04）：源码里出现 `<` / `&` 不得变成标记。
 *
 * ⚠️ 高亮色收在样式表的 `.src-line.is-current` 规则里（令牌派生），
 *    本模块**不写颜色字面量** ⇒ ⇒ 换主题时高亮跟着令牌走。
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
            return (
                `<div class="src-line${isCurrent ? " is-current" : ""}" data-src-line="${no}">` +
                `<span class="src-line-no">${no}</span>` +
                `<span class="src-line-text">${escapeHtml(line)}</span>` +
                `</div>`
            );
        })
        .join("");
}

/**
 * 源码树里某个 key 的**文本**（`Map` 与普通对象都支持）。
 *
 * ⚠️ **源码文本不进内核**（裁决 E）⇒ ⇒ 它只是**用户在前端装载的资产** ⇒ ⇒
 * 这里读的是**本地那份**，逐字显示。
 *
 * @param {Map<string, string>|object|null|undefined} tree — 源码树
 * @param {string|null|undefined} key — 文件 key
 * @returns {string} 文本（读不到 ⇒ 空串）
 */
function treeText(tree, key) {
    if (!tree || key == null) return "";
    if (typeof tree.get === "function") {
        const value = tree.get(key);
        return typeof value === "string" ? value : "";
    }
    if (typeof tree === "object") {
        const value = tree[key];
        return typeof value === "string" ? value : "";
    }
    return "";
}

/**
 * 渲染**左栏**：文件列表。
 *
 * ⚠️ **每一项都是一个按钮**（`.fi`）⇒ ⇒ 点它 = **选中那个文件**。
 *   - `data-source-key` —— **这一批新增的钩子**（谁的 key）；
 *   - `data-action="pick-candidate"` + `data-key` —— **复用既有的那个动作**
 *     ⇒ ⇒ 视图层**一个字符都不改**就能选中（那条分支写的是
 *     `source.selected = el.dataset.key` ⇒ ⇒ 与「用户在候选里选一个」
 *     **同一条落点**、**同一个语义**）。
 *
 * ⚠️ **选中态**用 `.on`（CSS 契约：`front/style.css` 的 `.filelist .fi.on`）。
 * ⚠️ ⛔ **源码行上不做断点**（裁决 H）：需要「行 → 地址」映射，而 M-11 的
 *   行表**只覆盖带 DWARF 的程序** ⇒ ⇒ 这里**不许**出现断点入口。
 *
 * ⚠️ **顺序 = 源码树自己的顺序**（装载顺序）⇒ ⇒ **不另加一条排序规则**
 *   （本批只调形态 · 事实 F14）⇒ ⇒ 同一个树 ⇒ 同一个列表。
 *
 * @param {Map<string, string>|object|null|undefined} tree — 源码树
 * @param {string|null|undefined} activeKey — **当前选中**的那个 key
 *   （null ⇒ **没有任何一项**被标成选中 ⛔ 不静默挑第一个）
 * @returns {string} HTML 片段
 */
export function renderSourceFileList(tree, activeKey) {
    const keys = treeKeys(tree);
    const active = activeKey == null ? null : String(activeKey);
    if (keys.length === 0) {
        return (
            `<div class="filelist" id="source-filelist" data-source-filelist>` +
            `<p class="muted">尚未装载源码</p>` +
            `</div>`
        );
    }
    const items = keys.map((key) => {
        const on = key === active ? " on" : "";
        const attr = escapeHtml(key);
        return (
            `<button type="button" class="fi${on}"` +
            ` data-source-key="${attr}"` +
            ` data-action="pick-candidate" data-key="${attr}"` +
            `>${attr}</button>`
        );
    });
    return (
        `<div class="filelist" id="source-filelist" data-source-filelist>` +
        `${items.join("")}` +
        `</div>`
    );
}

/**
 * 渲染源码视图的**两栏正文**：左「文件列表」＋ 右「代码区」。
 *
 * ⚠️ **类名契约**（`front/style.css` 已经写好 ⇒ ⇒ **不新造第二套类名**）：
 *   `.srcwrap`（两栏容器）· `.card`（卡片）· `.filelist` / `.filelist .fi` /
 *   `.filelist .fi.on`（文件列表）· `.code`（代码区）。
 *
 * ⚠️ **选中语义**（裁决 F）：`activeKey` 由调用方按**既有那条规则**算出来
 *   （用户在多个候选里选定的那个优先 ⇒ 否则唯一命中的那个 ⇒ 否则**没有**）
 *   ⇒ ⇒ 本函数**只负责把它标出来**，**不自己挑**（⛔ 不静默挑第一个）。
 *
 * ⚠️ **代码区**显示**当前选中**（或唯一匹配）的那份源码文本，**逐字**
 *   （走 `escapeHtml` ⇒ 源码里的 `<` / `&` 不会变成标记 · NFR-04）。
 *
 * ⚠️ **PC 行高亮**沿用 `renderSourceText` 的那条（`.src-line.is-current`）：
 *   行号为 **0**（内核：没有 DWARF）或 **0xFFFFFFFF**（内核：地址没命中
 *   行表）时**不高亮任何一行** ⇒ ⇒ 内核的**两种「没有答案」不合并**。
 *
 * @param {object} options
 * @param {Map<string, string>|object|null|undefined} options.tree — 源码树
 * @param {string|null|undefined} options.activeKey — 当前选中的 key
 * @param {number|undefined} options.currentLine — 当前行号（1 起；0 / 非法 ⇒ 不高亮）
 * @returns {string} HTML 片段
 */
export function renderSourceColumns(options) {
    const tree = options?.tree ?? null;
    const active = options?.activeKey == null ? null : String(options.activeKey);
    const line = Number(options?.currentLine ?? 0);
    // ⚠️ **选中态只有一处**：`activeKey` 为空 ⇒ **没有**文件被标成选中
    //   ⇒ ⇒ 代码区显示「先选一个」而不是**擅自**显示某一份。
    const code = active
        ? `<div class="code" data-source-code>${renderSourceText(treeText(tree, active), line)}</div>`
        : `<div class="code muted" data-source-code>选择源码文件查看内容</div>`;
    return `
        <div class="srcwrap">
            <div class="card">
                <h3>文件</h3>
                ${renderSourceFileList(tree, active)}
            </div>
            <div class="card">
                <h3>代码</h3>
                ${code}
            </div>
        </div>
    `;
}
