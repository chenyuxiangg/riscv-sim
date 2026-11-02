#!/usr/bin/env python3
"""前端门禁「测试」列的归属口径 —— **台账 §0.1 的唯一算法**。

用法：python3 tools/front_test_attrib.py [--repo <路径>] [--counts <逐文件用例数文件>]

⚠️ **为什么要它**：台账 §0.1 前端两行（M-01/02/03 与 M-04/05/07/10）的「测试」列
   此前是**逐批人工累加**的 ⇒ ⇒ 同一个数在树变过之后仍留着旧值（实测：
   `disasm.test.mjs` 在 v0.20 批整file记作 20 条归 M-02，后来往该文件里追加了
   「mock 内核」等组时**没回算**）⇒ ⇒ 本脚本把它变成**从当前树重算**。

**判据**（台账 §0.1 v0.17 批新立的口径，原文见 `docs/reviews/REVIEWS.md` §0.1 统计口径段）：
  「前端门禁用例住在 `front/tests/`（**不按模块目录分**），故按**被测源码文件所属的
    目录桶**归属——**桶内文件多者胜**（`front/views` + `front/app.js` +
    `front/index.html` + `front/style.css` → **M-01/02/03**；其余 →
    **M-04/05/07/10**）；该规则与每个套件文件头的自述模块**逐条一致**。」

  1. **粒度 = 套件文件级**：取该套件文件引用到的**被测源码文件集合**；
  2. 按**目录桶**归类 ⇒ **桶内文件多者胜**（**文件数**，不是引用次数）；
  3. 平手 ⇒ **自述**裁决（套件文件头 / describe 组标题里的模块标记）；
  4. **跨层**文件（各 describe 组的判定不一致）⇒ 按 describe 组分别记
     （台账 v0.33 批 `board-rules.test.mjs` 的 7 条 service + 7 条 app 先例）；
  5. 仍判不出 ⇒ 记 TIE（**不静默取一边**）。

**两个必须记住的例外**（都是实测出来的，不是拍脑袋）：
  * `front/lib/message.js` = **契约层的 JS 镜像** ⇒ 前端**每一层**的用例都会用它
    ⇒ ⇒ 它**不专属**任何一层 ⇒ ⇒ **不参与判桶**。
  * `front/dev/` 下的 mock 内核 / demo 固件**按目录归 B 桶**
    （台账 v0.17 批自述：`demo-firmware` 9 条归 M-04/05/07/10）⇒ ⇒ **不**因为
    「它是替身」就排除。

**判别力证据**：本口径在 v0.31 / v0.32 / v0.33 三棵树上**逐字复现**台账当时登记的
历史值（220/194 · 227/194 · 234/201）⇒ ⇒ 口径本身不是新造的。
（复现脚本见 `--selftest`，需要那三棵树的 git 提交号。）
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

BUCKET_A_FILES = {"app.js", "index.html", "style.css"}
BUCKET_A_PREFIX = "views/"

# 跨层共享 ⇒ 不参与判桶
SHARED_FILES = {"lib/message.js"}

# 自述标记 → 桶
SELF_A = ("M-01", "M-02", "M-03", "app 层", "视图")
SELF_B = ("M-04", "M-05", "M-07", "M-10", "service 层", "服务层")

# 历史回归锚点：(提交说明, 期望 A, 期望 B, 期望合计)
# ⚠️ 这三个值**逐字**来自台账 §0.1 的历史登记段 ⇒ 本脚本改坏了就会红。
REGRESSION = [
    ("v0.31", "baed235", 220, 194, 414),
    ("v0.32", "91a452e", 227, 194, 421),
    ("v0.33", "3676c3b", 234, 201, 435),
]


# --------------------------------------------------------------------------- 词法

# ⚠️ **正则字面量的判定**（D-174 做法 A · v0.72 批 STAGE-48）
#
# ⚠️ **为什么要做**：JS 的正则字面量 `/…/` 里**可以**有单引号 ⇒ ⇒ 旧版
# `strip_code` 把那个 `'` 当成**字符串开头** ⇒ ⇒ 一路抹到下一个 `'` ⇒ ⇒
# 中间整段代码（含若干 `test(` 调用）**被抹掉** ⇒ ⇒ 工具**静默少算**。
#
# ⚠️ **怎么区分「除号」与「正则开头」**：ECMAScript 的规则是「`/` 开始正则
# **当且仅当** 前一个 token **不能结束一个表达式**」⇒ ⇒ 可操作化为「看前
# 一个**有效字符**」。
#
# ⚠️ **保守白名单**：只有落在下面这组字符里（或某个关键字后面）⇒ ⇒ 才把
# `/` 当正则开头 ⇒ ⇒ **误判方向是「不掩码」而不是「误掩码」** ⇒ ⇒ 严格
# 优于旧版（旧版是把正则里的一切都当字符串）。
#
# ⚠️ **`}` 不在白名单里**（块结束 vs 对象字面量结束有二义）⇒ ⇒ 保守判成
# 除号 ⇒ ⇒ 与旧版同行为 ⇒ ⇒ **不新增风险**。
_REGEX_OK_PREV_CHARS = set("(,:;=&|?!+-*%~^<>[/")
# ⚠️ **关键字**：这些后面出现 `/` ⇒ ⇒ 是正则开头（`return /re/`）。
_REGEX_OK_PREV_WORDS = {
    "return", "throw", "case", "typeof", "void", "delete",
    "in", "of", "instanceof", "yield", "await", "new", "else", "do",
}
# ⚠️ **标识符字符**（用来切出「上一个词」）。
_IDENT_CHARS = set(
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_$"
)


def _regex_can_start(prev):
    """前一个有效 token ⇒ ⇒ 这里的 `/` **可能**是正则开头吗？

    @param {str|None} prev — 前一个有效 token（单词 / 单字符 / None）
    @returns {bool}
    """
    if prev is None:
        # ⚠️ 文件开头 ⇒ ⇒ `/` 只可能是正则。
        return True
    if prev in _REGEX_OK_PREV_WORDS:
        return True
    if len(prev) == 1 and prev in _REGEX_OK_PREV_CHARS:
        return True
    # ⚠️ **其它一律判成除号**（标识符 / 数字 / `)` / `]` / `}` / 字符串 …）
    # ⇒ ⇒ 保守 ⇒ ⇒ 不掩码 ⇒ ⇒ 与旧版同行为。
    return False


def _scan_regex(text, i, n):
    """从 `/`（下标 `i`）开始扫描一个正则字面量 ⇒ ⇒ 返回**结束 `/` 的下标**。

    ⚠️ **规则**：
      * `\\` 转义下一个字符（`\\/` **不是**结束符）；
      * `[...]` 字符类里的 `/` **不是**结束符；
      * 正则**不能跨行** ⇒ ⇒ 遇到换行就**放弃**（返回 -1 ⇒ 不是正则）。

    @param {str} text — 原文
    @param {int} i — `/` 的下标
    @param {int} n — 长度
    @returns {int} 结束 `/` 的下标；-1 ⇒ 不是正则字面量
    """
    j = i + 1
    in_class = False
    while j < n:
        ch = text[j]
        if ch == "\n":
            # ⚠️ **正则不能跨行** ⇒ ⇒ 这不是正则 ⇒ ⇒ 放弃（回退成除号）。
            return -1
        if ch == "\\":
            j += 2
            continue
        if ch == "[":
            in_class = True
        elif ch == "]":
            in_class = False
        elif ch == "/" and not in_class:
            return j
        j += 1
    return -1


def strip_code(text):
    """抹掉注释、字符串字面量与**正则字面量**（**保留长度** ⇒ 偏移不变）。

    ⚠️ **v0.72（STAGE-48 · D-174 做法 A）**：**正则字面量整段抹掉** ⇒ ⇒
    正则里那个 `'` **不再**被当成字符串开头 ⇒ ⇒ 「后面若干条用例整段被
    抹掉」那个洞**治根**。
    """
    out = list(text)
    i, n = 0, len(text)
    # ⚠️ **前一个有效 token**（决定 `/` 是除号还是正则开头）。
    prev = None
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                out[i] = " "
                i += 1
            # ⚠️ **注释不是 token** ⇒ ⇒ `prev` **不变**（`x /* c */ /re/` 里
            # `/` 仍然是除号）。
            continue
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            while i < n and not (text[i] == "*" and i + 1 < n and text[i + 1] == "/"):
                if text[i] != "\n":
                    out[i] = " "
                i += 1
            for _ in range(2):
                if i < n:
                    out[i] = " "
                    i += 1
            # ⚠️ 同上：`prev` **不变**。
            continue
        elif c in "\"'`":
            q = c
            i += 1
            while i < n:
                if text[i] == "\\":
                    out[i] = " "
                    if i + 1 < n:
                        out[i + 1] = " "
                    i += 2
                    continue
                if text[i] == q:
                    break
                if text[i] != "\n":
                    out[i] = " "
                i += 1
            i += 1
            # ⚠️ **字符串是「表达式可以结束」的位置** ⇒ ⇒ 后面那个 `/` 是
            # 除号（`"a" / 2`）。
            prev = "<string>"
            continue
        elif c == "/":
            # ⚠️ **既不是 `//` 也不是 `/*`** ⇒ ⇒ 可能是**正则字面量**。
            if _regex_can_start(prev):
                end = _scan_regex(text, i, n)
                if end >= 0:
                    # ⚠️ **整段抹掉**（含 flags）—— 只中和那个引号**不够**：
                    # 正则体里常含 `//`（如 `/https?:\\/\\//`）⇒ ⇒ 不整段
                    # 抹的话后面会把 `//` 当注释开头。
                    k = end
                    while k < n and text[k] in _IDENT_CHARS:
                        k += 1
                    for x in range(i, k):
                        if text[x] != "\n":
                            out[x] = " "
                    i = k
                    prev = "<regex>"
                    continue
            # ⚠️ **不是正则** ⇒ ⇒ 除号 ⇒ ⇒ `prev` 记成 `/`（除号之后**可以**
            # 跟一个正则：`a / /re/`）。
            prev = "/"
            i += 1
            continue
        elif c in _IDENT_CHARS:
            j = i
            while j < n and text[j] in _IDENT_CHARS:
                j += 1
            prev = text[i:j]
            i = j
            continue
        elif c.isspace():
            # ⚠️ **空白不是 token** ⇒ ⇒ `prev` **不变**。
            i += 1
            continue
        else:
            prev = c
            i += 1
    return "".join(out)


def match_paren(code, open_idx):
    depth = 0
    for i in range(open_idx, len(code)):
        if code[i] in "({[":
            depth += 1
        elif code[i] in ")}]":
            depth -= 1
            if depth == 0:
                return i
    return -1


def top_level_describes(code, text):
    """顶层 describe 块：(名字, 抹码后的正文, **原文**正文)。"""
    blocks = []
    for mt in re.finditer(r"(^|\n)describe\s*\(", code):
        lp = code.find("(", mt.start(1) + 1)
        if lp < 0:
            continue
        rp = match_paren(code, lp)
        if rp < 0:
            continue
        arrow = code.find("=>", lp)
        if arrow < 0 or arrow > rp:
            continue
        lb = code.find("{", arrow)
        if lb < 0 or lb > rp:
            continue
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        name = text[lp + 1:rp].split(",")[0].strip().strip("\"'`")
        blocks.append((name, code[lb + 1:rb], text[lb + 1:rb]))
    return blocks


def count_tests(code):
    """一条 node:test 用例 = 一次 `test()` 调用。"""
    return len(re.findall(r"(^|[^\w.])test\s*\(", code))


def detect_mask_mismatch(text, authoritative):
    """**掩码后的用例数** vs **权威数** ⇒ ⇒ 不等就是「**静默偏小**」。

    ⚠️ **这是台账 D-174 做法 B 的落点** —— 把「静默偏小」变成「**响亮的
    失败**」⇒ ⇒ 工具**不再**悄悄给出一个偏小的数。

    ⚠️ **两条独立算法**（**必须**保持独立 ⇒ ⇒ 否则交叉验证就消失了）：
      * **静态数** = `count_tests(strip_code(text))` —— 本工具自己的算法；
      * **权威数** = `node --test` 跑出来的 `# tests N`（`run_front_gate_counts`）
        —— **运行时**数出来的。

    ⇒ ⇒ 两边**不等** ⇒ ⇒ 掩码**抹掉了东西** ⇒ ⇒ 那就是本工具的洞
    （`strip_code` 不认识正则字面量 ⇒ ⇒ 正则里那个 `'` 被当成字符串开头
    ⇒ ⇒ 后面若干条用例整段被抹掉）。

    ⚠️ **「权威数」缺了（`None`）⇒ ⇒ 不判** ⇒ ⇒ 「没有基准」**不是**「有洞」
    ⇒ ⇒ 不猜。

    @param {str} text — 套件文件原文
    @param {int|None} authoritative — `node --test` 数出来的用例条数
    @returns {dict|None} `{"static": n, "authoritative": m}` 或 `None`
    """
    if authoritative is None:
        return None
    static = count_tests(strip_code(text))
    auth = int(authoritative)
    if static == auth:
        return None
    return {"static": static, "authoritative": auth}


def selftest_mask():
    """**掩码的自检**（D-174 做法 **A** 的**判别力来源** ＋ 做法 B 的兜底）。

    ⚠️ **v0.72（STAGE-48）**：做法 A（**认正则字面量**）落地 ⇒ ⇒ 含单引号
    正则的文件**不再**被少算 ⇒ ⇒ 断言从「**必须报出差异**」翻成「**掩码数
    必须等于权威数**」⇒ ⇒ **把做法 A 撤掉 ⇒ ⇒ 必红**（判别力**保住**）。

    ⚠️ **必要对照**：一个**不含**单引号正则的文件 ⇒ ⇒ 掩码数**也**等于
    权威数 ⇒ ⇒ 不许过掩码（否则「数对了」是因为「什么都不数」）。

    ⚠️ **做法 B 仍然要能报差异**（兜底）：给它一个**对不上**的权威数 ⇒ ⇒
    它**必须**报出来 ⇒ ⇒ 删掉 `detect_mask_mismatch` ⇒ ⇒ **必红** ⇒ ⇒
    做法 B 那条护栏**不会**被悄悄拆掉。
    """
    # -- ① 含单引号正则 ⇒ ⇒ 掩码**整段抹掉** ⇒ ⇒ 用例数**一个不少** --------
    tricky = (
        "const re = /don't/;\n"
        'test("one", () => {});\n'
        'test("two", () => {});\n'
        'test("three", () => {});\n'
    )
    static_tricky = count_tests(strip_code(tricky))
    ok_tricky = static_tricky == 3
    ok_tricky_no_mismatch = detect_mask_mismatch(tricky, 3) is None
    # -- ② 必要对照：**不含**单引号正则 ⇒ ⇒ 掩码数 == 权威数 ⇒ ⇒ 不报 ------
    easy = (
        'test("one", () => {});\n'
        'test("two", () => {});\n'
    )
    static_easy = count_tests(strip_code(easy))
    ok_easy = static_easy == 2
    ok_easy_no_mismatch = detect_mask_mismatch(easy, 2) is None
    # -- ③ 做法 B **仍然能报差异**（兜底护栏**没被拆**）----------------------
    ok_detects = detect_mask_mismatch(easy, 99) is not None
    print("  %-10s 含单引号正则 ⇒ %s ｜ 不含 ⇒ %s ｜ 仍能报差异 ⇒ %s" % (
        "掩码自检",
        "OK（数对了）" if ok_tricky and ok_tricky_no_mismatch
        else "★红★（数少了 ⇒ 做法 A 没生效）",
        "OK（没误报）" if ok_easy and ok_easy_no_mismatch else "★红★（误报）",
        "OK" if ok_detects else "★红★（报不出差异 ⇒ 做法 B 被拆了）",
    ))
    return ok_tricky and ok_tricky_no_mismatch and ok_easy and ok_easy_no_mismatch and ok_detects


def imports_of(text):
    """源文件 → 该文件在本套件里的**引用形态**（四种写法都认）。"""
    forms = {}

    def add(rel, form):
        forms.setdefault(rel, []).append(form)

    for a, rel in re.findall(
        r"const\s+(\w+)\s*=\s*await\s+importFront\(\s*[\"']([^\"']+)[\"']\s*\)", text
    ):
        add(rel, "member:" + a)
    for names, rel in re.findall(
        r"const\s*\{(.*?)\}\s*=\s*await\s+importFront\(\s*[\"']([^\"']+)[\"']\s*\)", text, re.S
    ):
        for nm in names.split(","):
            nm = nm.strip().split(":")[-1].strip()
            if nm:
                add(rel, "call:" + nm)
    for names, rel in re.findall(
        r"import\s*\{(.*?)\}\s*from\s*[\"']\.\./([^\"']+)[\"']", text, re.S
    ):
        for nm in names.split(","):
            nm = nm.strip().split(" as ")[-1].strip()
            if nm:
                add(rel, "call:" + nm)
    for rel in re.findall(r"resolve\(\s*frontDir\s*,\s*[\"']([^\"']+)[\"']", text):
        add(rel, "path:" + rel)
    return forms


def local_helpers(code):
    """本套件文件里定义的**顶层**本地函数：名字 → 函数体。

    用例常通过本地辅助函数间接碰被测源码 ⇒ 只扫用例正文会漏。
    """
    helpers = {}
    for mt in re.finditer(r"(^|\n)function\s+(\w+)\s*\(", code):
        lb = code.find("{", mt.end())
        if lb < 0:
            continue
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        helpers[mt.group(2)] = code[lb + 1:rb]
    for mt in re.finditer(r"(^|\n)const\s+(\w+)\s*=\s*(?:async\s*)?\([^)]*\)\s*=>\s*\{", code):
        lb = code.find("{", mt.end() - 1)
        rb = match_paren(code, lb)
        if rb < 0:
            continue
        helpers[mt.group(2)] = code[lb + 1:rb]
    return helpers


def referenced_files(body, forms, helpers=None, depth=0, raw=None):
    hit = set()
    for rel, fs in forms.items():
        for f in fs:
            if f.startswith("member:"):
                if re.search(r"\b%s\s*\." % re.escape(f[7:]), body):
                    hit.add(rel)
            elif f.startswith("call:"):
                if re.search(r"\b%s\s*[\(\.]" % re.escape(f[5:]), body):
                    hit.add(rel)
            elif f.startswith("path:"):
                hay = raw if raw is not None else body
                if re.search(r"[\"']%s[\"']" % re.escape(f[5:]), hay):
                    hit.add(rel)
    if helpers and depth < 3:
        for hname, hbody in helpers.items():
            if re.search(r"\b%s\s*\(" % re.escape(hname), body):
                hit |= referenced_files(hbody, forms, helpers, depth + 1, raw)
    return hit


def bucket_of(rel):
    return "A" if (rel in BUCKET_A_FILES or rel.startswith(BUCKET_A_PREFIX)) else "B"


def self_bucket(*texts):
    blob = " ".join(texts)
    a = any(m in blob for m in SELF_A)
    b = any(m in blob for m in SELF_B)
    if a and not b:
        return "A"
    if b and not a:
        return "B"
    return None


def pick_bucket(files, *selftexts):
    if not files:
        return None, {}
    judged = {f for f in files if f not in SHARED_FILES}
    a = sorted(f for f in judged if bucket_of(f) == "A")
    b = sorted(f for f in judged if bucket_of(f) == "B")
    det = {"A": a, "B": b, "shared": sorted(set(files) & SHARED_FILES)}
    if len(a) > len(b):
        return "A", det
    if len(b) > len(a):
        return "B", det
    if not a and not b:
        return None, det
    return self_bucket(*selftexts), det


# --------------------------------------------------------------------------- 主


def attribute(repo, counts):
    """counts = {套件文件名: 用例条数} ⇒ (A, B, TIE, rows, mismatches)

    ⚠️ **mismatches** = 「掩码后的用例数 ≠ 权威数」的那些文件（**D-174 做法 B**）
    ⇒ ⇒ **静默偏小**变成**可报告的差异** ⇒ ⇒ 由 `main()` 变成**红**。
    """
    rows = []
    mismatches = []
    ta = tb = tt = 0
    for name in sorted(counts):
        path = os.path.join(repo, "front", "tests", name)
        with open(path, encoding="utf-8") as fh:
            text = fh.read()
        code = strip_code(text)
        # ⚠️ **掩码检测**（D-174 做法 B）⇒ ⇒ 与「权威数」对账 ⇒ ⇒ 不等就
        #   **记下来** ⇒ ⇒ 由 `main()` 报错（**不再**静默偏小）。
        mismatch = detect_mask_mismatch(text, counts.get(name))
        if mismatch is not None:
            mismatches.append({"file": name, **mismatch})
        forms = imports_of(text)
        helpers = local_helpers(code)
        header = "\n".join(text.splitlines()[:40])
        groups = top_level_describes(code, text)
        total = count_tests(code)

        file_pick, file_det = pick_bucket(
            referenced_files(code, forms, helpers, raw=text), header)

        group_rows = []
        assigned = 0
        for gname, gbody, gtext in groups:
            n = count_tests(gbody)
            if n == 0:
                continue
            assigned += n
            gp, gdet = pick_bucket(
                referenced_files(gbody, forms, helpers, raw=gtext), gname, header)
            group_rows.append((gname, n, gp, gdet))

        picks = {gp for _, _, gp, _ in group_rows if gp in ("A", "B")}
        if len(picks) > 1:
            # 跨层 ⇒ 按组分别记
            for gname, n, gp, gdet in group_rows:
                pick = gp or file_pick or self_bucket(gname, header)
                rows.append({"file": name, "group": gname, "tests": n,
                             "pick": pick, "files": gdet})
                ta += n if pick == "A" else 0
                tb += n if pick == "B" else 0
                tt += n if pick not in ("A", "B") else 0
            top = total - assigned
            if top:
                pick = file_pick or self_bucket(header)
                rows.append({"file": name, "group": "<顶层用例>", "tests": top,
                             "pick": pick, "files": file_det})
                ta += top if pick == "A" else 0
                tb += top if pick == "B" else 0
                tt += top if pick not in ("A", "B") else 0
        else:
            pick = (file_pick or (group_rows[0][2] if group_rows else None)
                    or self_bucket(header))
            rows.append({"file": name, "group": "<整file>", "tests": total,
                         "pick": pick, "files": file_det})
            ta += total if pick == "A" else 0
            tb += total if pick == "B" else 0
            tt += total if pick not in ("A", "B") else 0
    return ta, tb, tt, rows, mismatches


def run_front_gate_counts(repo):
    """逐个跑前端套件，取**权威**的用例条数（TAP 尾部的 `# tests N`）。

    ⚠️ **必须逐文件跑**：`node --test` 整跑虽然也会给总数，但**不按文件分桶**
       ⇒ ⇒ 拿不到「每个文件多少条」⇒ ⇒ 归属算法就没有输入。
    """
    tests_dir = os.path.join(repo, "front", "tests")
    out = {}
    for name in sorted(os.listdir(tests_dir)):
        if not name.endswith(".test.mjs"):
            continue
        proc = subprocess.run(
            ["node", "--test", "--test-reporter=tap",
             os.path.join("front", "tests", name)],
            cwd=repo, capture_output=True, text=True)
        m = re.search(r"^# tests (\d+)", proc.stdout, re.M)
        if not m:
            raise SystemExit("error: 拿不到 %s 的用例数（node --test 失败？）" % name)
        out[name] = int(m.group(1))
    return out


def selftest(repo):
    """判别力回归：在 v0.31 / v0.32 / v0.33 三棵树上**逐字**复现台账历史值。

    ⚠️ 回归用的 worktree 建在**仓外**（`$TMPDIR`）⇒ 不污染仓库目录；
    ⚠️ 跑完**自己收掉**（`git worktree remove`）⇒ ⇒ 可以安全地进默认门禁
       （门禁脚本不该在仓外留垃圾）。
    """
    ok = True
    regress_root = os.path.join(tempfile.gettempdir(), "front-attrib-regress")
    created = []
    try:
        for label, commit, exp_a, exp_b, exp_total in REGRESSION:
            work = os.path.join(regress_root, label)
            if not os.path.isdir(work):
                subprocess.run(["git", "worktree", "add", "--detach", work, commit],
                               cwd=repo, check=True, capture_output=True, text=True)
                created.append(work)
            counts = run_front_gate_counts(work)
            a, b, t, _, mismatches = attribute(work, counts)
            good = (a, b, sum(counts.values())) == (exp_a, exp_b, exp_total)
            # ⚠️ **历史树上的掩码差异也要如实报**（D-174 做法 B）⇒ ⇒ 那三棵树
            #   的**历史值**若与掩码数不符 ⇒ ⇒ **必须登记**（不许悄悄改）。
            if mismatches:
                good = False
                print("  ★红★ %s 上有掩码差异：%s" % (
                    label, ", ".join(
                        "%s(静态 %d / 权威 %d)" % (m["file"], m["static"], m["authoritative"])
                        for m in mismatches)))
            ok = ok and good
            print("  %-8s 期望 %d/%d（合计 %d）  实测 %d/%d（合计 %d）  %s" % (
                label, exp_a, exp_b, exp_total, a, b, sum(counts.values()),
                "OK" if good else "★红★"))
    finally:
        for work in created:
            subprocess.run(["git", "worktree", "remove", "--force", work],
                           cwd=repo, capture_output=True, text=True)
        subprocess.run(["git", "worktree", "prune"], cwd=repo, capture_output=True, text=True)
    return ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default=os.path.dirname(os.path.dirname(
        os.path.abspath(__file__))))
    ap.add_argument("--counts", help="逐文件用例数文件（`名字|条数` 每行一条）")
    ap.add_argument("--selftest", action="store_true",
                    help="判别力回归：在 v0.31/v0.32/v0.33 三棵树上复现台账历史值")
    ap.add_argument("--json", help="把逐文件归属明细写到这个文件")
    args = ap.parse_args()

    if args.selftest:
        print("前端归属口径 · 历史回归（判别力证据）")
        # ⚠️ **掩码检测的自检先跑**（D-174 做法 B）⇒ ⇒ 它**不**需要建
        #   worktree ⇒ ⇒ 快 ⇒ ⇒ 失败**立刻**看得见。
        print("前端归属口径 · 掩码检测自检（D-174 做法 B）")
        mask_ok = selftest_mask()
        ok = selftest(args.repo)
        return 0 if (ok and mask_ok) else 1

    if args.counts:
        counts = {}
        with open(args.counts, encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                name, n = line.split("|")[0], line.split("|")[1]
                if n.isdigit():
                    counts[name] = int(n)
    else:
        counts = run_front_gate_counts(args.repo)

    a, b, t, rows, mismatches = attribute(args.repo, counts)
    # ⚠️ **掩码差异先报**（D-174 做法 B）⇒ ⇒ 它是**工具自己**的洞 ⇒ ⇒
    #   比「归属数对不上」**更根本** ⇒ ⇒ 先说它。
    if mismatches:
        print("★红★ 掩码后的用例数与权威数不符（D-174 · 做法 B）：")
        print("  ⇒ ⇒ 掩码（strip_code）**抹掉了东西** ⇒ ⇒ 那个数**偏小** ⇒ ⇒")
        print("  ⇒ ⇒ 台账「测试」列**不可信** ⇒ ⇒ 请改源码里的正则字面量")
        print("  ⇒ ⇒ （或修 `strip_code` 让它认识正则字面量 —— 那是做法 A）。")
        for m in mismatches:
            print("  %-34s 静态 %3d  权威 %3d  差 %+d" % (
                m["file"], m["static"], m["authoritative"],
                m["static"] - m["authoritative"]))
        print()
    for r in rows:
        if r["pick"] not in ("A", "B"):
            print("TIE/未知: %-28s %-44s %3d" % (r["file"], r["group"][:44], r["tests"]))
    print("前端门禁「测试」列归属（从当前树重算）")
    print("  M-01/02/03      %4d 条" % a)
    print("  M-04/05/07/10   %4d 条" % b)
    print("  TIE（待消解）    %4d 条" % t)
    print("  合计            %4d 条" % (a + b + t))
    if args.json:
        with open(args.json, "w", encoding="utf-8") as fh:
            json.dump(rows, fh, ensure_ascii=False, indent=1)
    # ⚠️ **掩码差异也是红**（D-174 做法 B）⇒ ⇒ 工具**不再**静默偏小。
    return 1 if (t or mismatches) else 0


if __name__ == "__main__":
    sys.exit(main())
