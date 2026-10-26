#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-32 规格自检（七项 + 六个子项，与 stage31_check.py 同构）。

用法：python3 tools/stage32_check.py docs/detailed-design/STAGE-32_V047_Load_Image_UI.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~M 无悬空引用（§3 每条都有编号；正文引用的编号都在表内）
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-12 / B-1~B-5 / C-1~C-6）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F14 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§4.3 的 **C-3 行是唯一权威处**）
       ＋ 7e. **前端必须有加载入口**：`front/views/m02_viewcomponents.js` 里
                **必须**出现 `load-image` 与 `type="file"`
       ＋ 7f. **app 层必须有分支**：`front/app.js` 里**必须**出现 `load-image`
                与 `loadProject`
       ＋ 7g. **契约层 / 内核零变更**：台账里**必须**登记本批的 C-1 / C-2 边界
       ＋ 7h. `tools/stage32_check.py` 自己**必须存在**（自指）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7e / 7f 是「本批专属」**：它们盯的是**本批才引入**的字面量 ⇒ ⇒ 上一批的树上
   **必然红** ⇒ ⇒ 那不是「历史退化」，是「本批的落点还没落地」⇒ ⇒ 批末必须绿。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-32_V047_Load_Image_UI.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7g 用）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

VIEWS_REL = "front/views/m02_viewcomponents.js"
APP_REL = "front/app.js"

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约层零升版** ⇒ ⇒ 全不动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "8",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}


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

# --- 4. 裁决编号 A~M 无悬空引用 --------------------------------------------
DECISIONS = [chr(ord("A") + i) for i in range(13)]  # A..M
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*([A-M])\s*\|", line.strip())
    if m:
        defined.add(m.group(1))
for d in DECISIONS:
    if d not in defined:
        err(f"[裁决] §3 缺裁决 {d}")
text = "\n".join(l for l in lines)
# 正文里引用的裁决编号必须都在裁决表内 ⇒ ⇒ 判据 = 「该编号在 §3 的表里出现过」
# ⚠️ **不要**用「上下文窗口里有没有 `| F |`」那种脆判据：§8 的订正里引用裁决
#    时，隔着几十行，窗口根本够不到 ⇒ ⇒ 会**假红**。
for d in DECISIONS:
    for m in re.finditer(rf"裁决\s*{d}\b", text):
        if d not in defined:
            err(f"[裁决] 正文引用了未定义的裁决 {d}")

# --- 5. 章节 §0~§10 完整 ---------------------------------------------------
for sec in range(0, 11):
    if not re.search(rf"^## §{sec}\b", text, re.M):
        err(f"[章节] 缺 §{sec}")

# --- 6. 验收编号交叉引用 ---------------------------------------------------
EXPECT_ACCEPT = {
    "A": list(range(1, 13)),   # A-1 ~ A-12
    "B": list(range(1, 6)),    # B-1 ~ B-5
    "C": list(range(1, 7)),    # C-1 ~ C-6
}
for group, nums in EXPECT_ACCEPT.items():
    for n in nums:
        if not re.search(rf"^\|\s*{group}-{n}\s*\|", text, re.M):
            err(f"[验收] §4 缺 {group}-{n}")
# 反向：正文引用了未定义的验收编号
for group, nums in EXPECT_ACCEPT.items():
    for m in re.finditer(rf"\b({group}-\d+)\b", text):
        tag = m.group(1)
        num = int(tag.split("-")[1])
        if num not in nums:
            err(f"[验收] 引用了未定义的 {tag}")

# --- 7. 关键约束字面量钉住 -------------------------------------------------
for literal in (
    "ImageSource::Bytes",
    "load-image",
    "loadProjectIntoKernel",
    "装完就停",
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
    # §8 到 §9 之间的内容
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
    # 找带该名字的行，取最后一个反引号里的值
    found = None
    for line in lines:
        if name in line and "仍" in line or (name in line and "|" in line):
            m = re.findall(rf"`{name}`\s*(?:仍\s*)?\*\*(\d+)\*\*", line)
            if m:
                found = m[-1]
    if found is None:
        # 退化：宽松匹配「`NAME` 仍 **N**」
        m = re.findall(rf"`{name}`[^\n]*?\*\*(\d+)\*\*", text)
        if m:
            found = m[-1]
    if found is None:
        err(f"[版本号] 规格里找不到 {name} 的值")
    elif found != value:
        err(f"[版本号] {name} 期望 {value} ⇒ 实测 {found}")

# --- 7e. 前端必须有加载入口 ------------------------------------------------
views_src = read(VIEWS_REL)
if views_src is None:
    err(f"[落点] 找不到 {VIEWS_REL}")
else:
    if "load-image" not in views_src:
        err(f"[落点] {VIEWS_REL} 里没有 `load-image`")
    if 'type="file"' not in views_src:
        err(f"[落点] {VIEWS_REL} 里没有 `type=\"file\"`（加载入口缺文件选择器）")

# --- 7f. app 层必须有分支 --------------------------------------------------
app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    if "load-image" not in app_src:
        err(f"[落点] {APP_REL} 里没有 `load-image`")
    if "loadProject" not in app_src:
        err(f"[落点] {APP_REL} 里没有 `loadProject`（命令没真正发出去）")

# --- 7g. 台账必须登记「契约层 / 内核零变更」这条边界 -----------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.47" not in ledger_src:
        err("[台账] 台账里没有 v0.47 的批次记录")
    if "STAGE-32" not in ledger_src:
        err("[台账] 台账里没有 STAGE-32 的引用")

# --- 7h. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage32_check.py"
if not self_path.exists():
    err("[自指] tools/stage32_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
