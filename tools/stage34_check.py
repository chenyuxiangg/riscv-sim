#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-34 规格自检（七项 + 九个子项，与 stage33_check.py 同构）。

用法：python3 tools/stage34_check.py docs/detailed-design/STAGE-34_V049_UI_Visual_Layout.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~K 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-9 / B-12~B-18 / C-1~C-3）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F12 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5 的表是唯一权威处）
       ＋ 7e. **侧边栏与顶栏运行条必须落地**：`front/index.html` 里**必须**出现
                `data-sidebar` 与 `data-topbar-runbar`
       ＋ 7f. **视图分组元数据必须落地**：`front/views/m01_workbench.js` 里**必须**
                出现 `group` 与两个分组取值（`run` / `platform`）
       ＋ 7g. **顶栏运行条接线必须落地**：`front/app.js` 里**必须**出现
                `data-topbar-runbar` 与顶栏那三个命令
       ＋ 7h. **设计令牌逐字落地**：`front/style.css` 的 `:root` 里**必须**出现
                §5.1 的每个令牌值，且**旧令牌零残留**
       ＋ 7i. **台账必须登记本批**：台账里**必须**出现 `v0.49` 与 `STAGE-34`
       ＋ 7j. `tools/stage34_check.py` 自己**必须存在**（自指）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7e ~ 7h 是「本批专属」**：它们盯的是**本批才引入**的字面量 ⇒ ⇒ 上一批的树上
   **必然红** ⇒ ⇒ 那不是「历史退化」，是「本批的落点还没落地」⇒ ⇒ 批末必须绿。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-34_V049_UI_Visual_Layout.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7i 用）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

INDEX_REL = "front/index.html"
APP_REL = "front/app.js"
WORKBENCH_REL = "front/views/m01_workbench.js"
STYLE_REL = "front/style.css"

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约层零升版** ⇒ ⇒ 全不动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "8",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

#: ⚠️ **设计令牌**（7h 用）—— §5.1 的表是唯一权威处 ⇒ ⇒ 这里只钉**值**。
#: ⚠️ **旧令牌零残留**：换名不换值 ⇒ ⇒ 留两套命名会是「第二个真相」。
EXPECT_TOKENS = {
    "--bg": "#0b1020",
    "--bg2": "#0e1628",
    "--panel": "#111a30",
    "--panel2": "#0d1526",
    "--line": "#232f4d",
    "--line2": "#1a2340",
    "--tx": "#e6edf7",
    "--tx2": "#a8b4cc",
    "--tx3": "#6b7a99",
    "--accent": "#22d3ee",
    "--amber": "#f5a524",
    "--red": "#f87171",
    "--green": "#34d399",
    "--r": "10px",
}

#: ⚠️ **旧令牌名**（7h 用）—— 本批**一次性换掉** ⇒ ⇒ 这些名字**必须零残留**。
RETIRED_TOKENS = ("--ink", "--ink2", "--teal", "--bg3")


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

# --- 4. 裁决编号 A~K 无悬空引用 --------------------------------------------
DECISIONS = [chr(ord("A") + i) for i in range(11)]  # A..K
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*([A-K])\s*\|", line.strip())
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
    "A": list(range(1, 10)),   # A-1 ~ A-9
    # ⚠️ **B 从 12 起**：STAGE-33 那一批已经用了 `B-6 ~ B-11` ⇒ ⇒
    #    **编号空间是跨批次共享的** ⇒ ⇒ 新批次**不能**从 1 重新起
    #    （否则同一个门禁输出里会出现**同号不同义**的断言）。
    "B": list(range(12, 19)),  # B-12 ~ B-18
    "C": list(range(1, 4)),    # C-1 ~ C-3
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**上一批已定义**的那些。
#: ⇒ ⇒ 本规格**正文里提到** `B-6 ~ B-11`（那是 STAGE-33 定义的、且 C-1 要验
#: 它们**仍然成立**）**是合法的** ⇒ ⇒ 不许把它报成「未定义」。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "B": set(range(6, 19)),    # B-6 ~ B-18（B-6 ~ B-11 属 STAGE-33）
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
    "侧边栏",
    "data-view-tab",
    "aria-current",
    "hash 路由",
    "设计令牌",
    "内联 SVG",
    "零升版",
):
    if literal not in text:
        err(f"[字面量] 规格里缺 {literal}")

# --- 7b. §1.2 事实表 F1~F12 连续 -------------------------------------------
f_rows = re.findall(r"^\|\s*(F\d+)\s*\|", text, re.M)
expected_f = [f"F{i}" for i in range(1, 13)]
if f_rows != expected_f:
    err(f"[事实表] §1.2 的 F 编号不是 F1~F12 连续（实测 {f_rows}）")

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
    m = re.findall(rf"`{name}`\s*\|\s*\*\*(\d+)\*\*", text)
    if not m:
        err(f"[版本号] 规格里找不到 {name} 的值")
    elif m[-1] != value:
        err(f"[版本号] {name} 期望 {value} ⇒ 实测 {m[-1]}")

# --- 7e. 侧边栏与顶栏运行条必须落地 ----------------------------------------
index_src = read(INDEX_REL)
if index_src is None:
    err(f"[落点] 找不到 {INDEX_REL}")
else:
    for literal in ("data-sidebar", "data-topbar-runbar"):
        if literal not in index_src:
            err(f"[落点] {INDEX_REL} 里没有 `{literal}`"
                "（侧边栏 / 顶栏运行条没有挂载点）")

# --- 7f. 视图分组元数据必须落地 --------------------------------------------
wb_src = read(WORKBENCH_REL)
if wb_src is None:
    err(f"[落点] 找不到 {WORKBENCH_REL}")
else:
    if "group" not in wb_src:
        err(f"[落点] {WORKBENCH_REL} 里没有 `group`"
            "（视图没分组 ⇒ ⇒ 侧边栏没法按组渲染）")
    for grp in ("run", "platform"):
        if f'"{grp}"' not in wb_src:
            err(f"[落点] {WORKBENCH_REL} 里没有分组取值 `{grp}`")

# --- 7g. 顶栏运行条接线必须落地 --------------------------------------------
app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    if "data-topbar-runbar" not in app_src:
        err(f"[落点] {APP_REL} 里没有 `data-topbar-runbar`"
            "（顶栏运行条没人挂）")
    for cmd in ("step", "continue", "reset"):
        if f'"{cmd}"' not in app_src:
            err(f"[落点] {APP_REL} 里没有命令 `{cmd}`"
                "（顶栏运行条要发的命令不在）")

# --- 7h. 设计令牌逐字落地 + 旧令牌零残留 -----------------------------------
style_src = read(STYLE_REL)
if style_src is None:
    err(f"[落点] 找不到 {STYLE_REL}")
else:
    root_at = re.search(r":root\s*\{(.*?)\}", style_src, re.S)
    if not root_at:
        err("[令牌] front/style.css 里没有 `:root` 块（设计令牌没有落点）")
    else:
        root_body = root_at.group(1)
        for token, value in EXPECT_TOKENS.items():
            if f"{token}:" not in root_body or value not in root_body:
                err(f"[令牌] `:root` 里缺 `{token}: {value}`")
    for old in RETIRED_TOKENS:
        if re.search(rf"{re.escape(old)}\s*:", style_src):
            err(f"[令牌] `front/style.css` 里还有**旧令牌** `{old}`"
                "（换名不换值 ⇒ ⇒ 留两套命名会是第二个真相）")

# --- 7i. 台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.49" not in ledger_src:
        err("[台账] 台账里没有 v0.49 的批次记录")
    if "STAGE-34" not in ledger_src:
        err("[台账] 台账里没有 STAGE-34 的引用")

# --- 7j. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage34_check.py"
if not self_path.exists():
    err("[自指] tools/stage34_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
