#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-35 规格自检（七项 + 九个子项，与 stage34_check.py 同构）。

用法：python3 tools/stage35_check.py docs/detailed-design/STAGE-35_V050_View_Content_Breakpoint_UI.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-16 / B-19~B-26 / C-1~C-4）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F14 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5 的表是唯一权威处）
       ＋ 7e. **稳定钩子逐条落地**（裁决 A）：§5.1 的每个钩子**必须**出现在仓内源码里
       ＋ 7f. **断点接线必须落地**：`front/app.js` 里**必须**出现 `SetBreakpoint` /
                `ClearBreakpoint` / `debug.halt` / `debug.breakpoints`
       ＋ 7g. **断点列必须落地**：`front/views/m02_viewcomponents.js` 里**必须**出现
                `data-bp-toggle` 与 `debug.breakpoints`
       ＋ 7h. **台账必须登记本批**：台账里**必须**出现 `v0.50` 与 `STAGE-35`
       ＋ 7i. `tools/stage35_check.py` 自己**必须存在**（自指）
       ＋ 7j. §9 的**变异编号** `T1 ~ T8` + `T-C1` **齐全**（`stage34_check.py`
                **没有**这一项 ⇒ ⇒ 那是它的缺口 ⇒ ⇒ **不要**照抄）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一** ⇒ ⇒ 本批从
   **B-19** 起（STAGE-33 用了 B-6~B-11 · STAGE-34 用了 B-12~B-18）⇒ ⇒ **不重用**。
   **A 组与 C 组**是**每批范围**（各自的规格文档 + 各自的 Node 套件 ⇒ ⇒ 落在不同的输出里）。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-35_V050_View_Content_Breakpoint_UI.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7h 用）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

APP_REL = "front/app.js"
VIEW_REL = "front/views/m02_viewcomponents.js"
MEMORY_MAP_REL = "front/views/components/memory-map.js"
SOURCE_REL = "front/views/components/source-view.js"
STYLE_REL = "front/style.css"

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约层零升版** ⇒ ⇒ 全不动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "8",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

#: ⚠️ **稳定钩子**（7e 用）—— §5.1 的表是唯一权威处 ⇒ ⇒ 这里只钉**字面量**。
#: ⇒ ⇒ **每一条都必须在仓内源码里出现** ⇒ ⇒ 换形态**不许**把判据的取法换掉。
EXPECT_HOOKS = (
    "data-view-body",
    "data-embedded-pc",
    "data-embedded-time",
    "data-embedded-entry",
    "data-embedded-link",
    "reg-grid",
    "channel-list",
    "disasm-row",
    "data-disasm-addr",
    "data-region-base",
    "data-mmap-",
    "data-terminal-out",
    "data-trace-notice",
    "data-trace-overflow",
    "data-trace-download-button",
)


def err(msg):
    fail.append(msg)


def read(rel):
    p = REPO_ROOT / rel
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# 1~3：表格列数 / 标题跳级 / 围栏配平
# ---------------------------------------------------------------------------


def split_fences(src):
    """返回 (围栏外的行, 围栏内的行标记)。围栏块内容整块剔除。"""
    out = []
    in_fence = False
    for line in src:
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            out.append(None)
            continue
        out.append(None if in_fence else line)
    return out


body = split_fences(lines)

# --- 1. 表格列数一致 -------------------------------------------------------
start = None
cols = None
for i, line in enumerate(body, 1):
    if line is None:
        continue
    s = line.strip()
    if s.startswith("|") and s.endswith("|"):
        c = len(split_row(s))
        if start is None:
            start, cols = i, c
        elif c != cols:
            err(f"[表格] 第 {i} 行列数 {c} ≠ 首行 {cols}（表起于第 {start} 行）")
    else:
        start, cols = None, None

# --- 2. 标题层级不跳级 -----------------------------------------------------
prev = 0
for i, line in enumerate(body, 1):
    if line is None:
        continue
    m = re.match(r"^(#{1,6})\s+\S", line)
    if not m:
        continue
    lvl = len(m.group(1))
    if prev and lvl > prev + 1:
        err(f"[标题] 第 {i} 行从 h{prev} 跳到 h{lvl}")
    prev = lvl

# --- 3. 代码围栏配平 -------------------------------------------------------
n_fences = sum(1 for l in lines if l.lstrip().startswith("```"))
if n_fences % 2 != 0:
    err(f"[围栏] 围栏数 {n_fences} 为奇数 ⇒ 未配平")

# --- 4. 裁决编号 A~O 无悬空引用 --------------------------------------------
DECISIONS = [chr(ord("A") + i) for i in range(15)]  # A..O
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*([A-O])\s*\|", line.strip())
    if m:
        defined.add(m.group(1))
for d in DECISIONS:
    if d not in defined:
        err(f"[裁决] §2 缺裁决 {d}")
text = "\n".join(l for l in lines)
for d in DECISIONS:
    for m in re.finditer(rf"裁决\s*{d}\b", text):
        if d not in defined:
            err(f"[裁决] 正文引用了未定义的裁决 {d}")

# --- 5. 章节 §0~§10 完整 ---------------------------------------------------
for sec in range(0, 11):
    if not re.search(rf"^## §{sec}\b", text, re.M):
        err(f"[章节] 缺 §{sec}")

# --- 6. 验收编号交叉引用 ---------------------------------------------------
#: ⚠️ **本规格必须定义**的验收编号。
EXPECT_ACCEPT = {
    # ⚠️ **A 是每批范围**（各自的规格文档 + 各自的 Node 套件 ⇒ ⇒ 落在不同的输出里）。
    "A": list(range(1, 17)),   # A-1 ~ A-16
    # ⚠️ **B 从 19 起**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一**
    #    （STAGE-33 用了 B-6 ~ B-11 · STAGE-34 用了 B-12 ~ B-18）⇒ ⇒ **不重用**。
    "B": list(range(19, 27)),  # B-19 ~ B-26
    "C": list(range(1, 5)),    # C-1 ~ C-4
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前两批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "B": set(range(6, 27)),    # B-6 ~ B-26（B-6~B-11 属 STAGE-33 · B-12~B-18 属 STAGE-34）
    "C": set(EXPECT_ACCEPT["C"]),
}
for group, nums in EXPECT_ACCEPT.items():
    for n in nums:
        if not re.search(rf"^\|\s*{group}-{n}\s*\|", text, re.M):
            err(f"[验收] §3 缺 {group}-{n}")
# 反向：正文引用了未定义的验收编号
#
# ⚠️ **必须排除 §8**（施工期订正那一节）：它**逐字引用旧编号**（「规格原文」
#    那一段）⇒ ⇒ 那是「**记做过什么**」⇒ ⇒ 不是「**规格现在说什么**」⇒ ⇒
#    扫它 ⇒ ⇒ **门禁被文档自己触发** ⇒ ⇒ **零判别力**。
sec8_at = re.search(r"^## §8\b", text, re.M)
live_text = text[: sec8_at.start()] if sec8_at else text
for group in EXPECT_ACCEPT:
    for m in re.finditer(rf"\b({group}-\d+)\b", live_text):
        tag = m.group(1)
        num = int(tag.split("-")[1])
        if num not in KNOWN_ACCEPT[group]:
            err(f"[验收] 引用了未定义的 {tag}")

# --- 7. 关键约束字面量钉住 -------------------------------------------------
for literal in (
    "断点",
    "SetBreakpoint",
    "ClearBreakpoint",
    "稳定钩子",
    "卡片",
    "零升版",
    "视图内容形态",
):
    if literal not in text:
        err(f"[字面量] 规格里缺 {literal}")

# --- 7b. §1.2 事实表 F1~F14 连续 -------------------------------------------
f_rows = re.findall(r"^\|\s*(F\d+)\s*\|", text, re.M)
expected_f = [f"F{i}" for i in range(1, 15)]
if f_rows != expected_f:
    err(f"[事实表] §1.2 的 F 编号不是 F1~F14 连续（实测 {f_rows}）")

# --- 7c. §8 施工期订正 ------------------------------------------------------
sec8 = re.search(r"^## §8 .*$", text, re.M)
if not sec8:
    err("[订正] 缺 §8")
else:
    tail = text[sec8.end():]
    stop = re.search(r"^## §9\b", tail, re.M)
    sec8_body = tail[:stop.start()] if stop else tail
    if "施工期订正" in sec8_body and "（施工中每改规格一字" in sec8_body \
            and not re.search(r"^### 订正 1 ", sec8_body, re.M):
        err("[订正] §8 只有占位、没有真的订正条目")
    corr = re.findall(r"^### 订正 (\d+) · (.+)$", sec8_body, re.M)
    if corr:
        nums = [int(n) for n, _ in corr]
        if nums != list(range(1, len(nums) + 1)):
            err(f"[订正] §8 的订正编号不连续（实测 {nums}）")
        # ⚠️ **逐条判**：每条自己那段里都必须有「已改成」
        blocks = re.split(r"^### 订正 \d+ · ", sec8_body, flags=re.M)[1:]
        for idx, blk in enumerate(blocks, 1):
            if "已改成" not in blk:
                err(f"[订正] 第 {idx} 条订正自己那段里没有「已改成」")

# --- 7d. 四个版本号的值 ----------------------------------------------------
for name, value in EXPECT_VERSIONS.items():
    m = re.findall(rf"`{name}`\s*\|\s*\*\*(\d+)\*\*", text)
    if not m:
        err(f"[版本号] 规格里找不到 {name} 的值")
    elif m[-1] != value:
        err(f"[版本号] {name} 期望 {value} ⇒ 实测 {m[-1]}")

# --- 7e. 稳定钩子逐条落地（裁决 A） -----------------------------------------
#: ⚠️ **钩子落在哪个文件**是**判据的取法** ⇒ ⇒ 换形态**不许**把它换掉。
HOOK_FILES = (
    VIEW_REL, MEMORY_MAP_REL, SOURCE_REL,
    "front/views/components/image-loader.js",
)
hook_sources = {rel: (read(rel) or "") for rel in HOOK_FILES}
all_hooks = "\n".join(hook_sources.values())
for hook in EXPECT_HOOKS:
    if hook not in all_hooks:
        err(f"[钩子] 稳定钩子 `{hook}` 在仓内源码里**找不到**"
            "（换形态把判据的取法换掉了 ⇒ ⇒ 判据会悄悄变）")

# --- 7f. 断点接线必须落地 ---------------------------------------------------
app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    for literal in ("SetBreakpoint", "ClearBreakpoint", "debug.halt", "debug.breakpoints"):
        if literal not in app_src:
            err(f"[落点] {APP_REL} 里没有 `{literal}`"
                "（断点接线 / 状态落点没落地）")

# --- 7g. 断点列必须落地 -----------------------------------------------------
view_src = read(VIEW_REL)
if view_src is None:
    err(f"[落点] 找不到 {VIEW_REL}")
else:
    for literal in ("data-bp-toggle", "debug.breakpoints"):
        if literal not in view_src:
            err(f"[落点] {VIEW_REL} 里没有 `{literal}`"
                "（反汇编表的断点列没落地）")

# --- 7h. 台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.50" not in ledger_src:
        err("[台账] 台账里没有 v0.50 的批次记录")
    if "STAGE-35" not in ledger_src:
        err("[台账] 台账里没有 STAGE-35 的引用")

# --- 7j. §9 的变异编号 T1 ~ T8 + T-C1 齐全 -------------------------------
#: ⚠️ **§10 第 7 项**要求「§9 的变异编号 `T1 ~ T8` + `T-C1` **齐全**」⇒ ⇒
#: ⇒ 这里**补上**这一项（`stage34_check.py` **没有** ⇒ ⇒ 那是它的一个缺口，
#: ⇒ ⇒ **不要**照抄）。
mut_rows = re.findall(r"^\|\s*(T\d+|T-C1)\s*\|", text, re.M)
expect_muts = [f"T{i}" for i in range(1, 9)] + ["T-C1"]
if mut_rows != expect_muts:
    err(f"[变异] §9 的变异编号不是 T1~T8 + T-C1（实测 {mut_rows}）")

# --- 7i. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage35_check.py"
if not self_path.exists():
    err("[自指] tools/stage35_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
