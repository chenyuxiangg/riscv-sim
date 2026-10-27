#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-37 规格自检（七项 + 十一个子项，与 stage36_check.py 同构）。

用法：python3 tools/stage37_check.py docs/detailed-design/STAGE-37_V052_Continuous_Run.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-12 / B-34~B-41 / C-1~C-5）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F16 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.4 的表是唯一权威处）
       ＋ 7e. `front/lib/run-loop.js` 存在且导出三个纯函数
       ＋ 7f. `front/app.js` 里 `debug.run` 与续发落点存在
       ＋ 7g. `front/index.html` 里 `data-topbar-action="pause"` 存在
       ＋ 7h. `front/app.js` 的 `case "pause"` 分支**不**发 `Pause` 命令（F6 残留）
       ＋ 7i. 台账必须登记本批（`v0.52` + `STAGE-37`）
       ＋ 7j. 脚本必须自指存在
       ＋ 7k. §9 的**变异编号** `T1 ~ T9` + `T-C1` **齐全**（`stage36_check.py`
                有这一项 ⇒ ⇒ **不要**照抄它的编号范围）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一** ⇒ ⇒ 本批从
   **B-34** 起（STAGE-33 用了 B-6~B-11 · STAGE-34 用了 B-12~B-18 · STAGE-35 用了
   B-19~B-26 · STAGE-36 用了 B-27~B-33）⇒ ⇒ **不重用**。**A 组与 C 组**是**每批范围**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-37_V052_Continuous_Run.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7i 用）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

APP_REL = "front/app.js"
INDEX_REL = "front/index.html"
RUNLOOP_REL = "front/lib/run-loop.js"

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
    "A": list(range(1, 13)),  # A-1 ~ A-12
    # ⚠️ **B 从 34 起**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一**
    #    （STAGE-33 用了 B-6 ~ B-11 · STAGE-34 用了 B-12 ~ B-18 · STAGE-35 用了
    #    B-19 ~ B-26 · STAGE-36 用了 B-27 ~ B-33）⇒ ⇒ **不重用**。
    "B": list(range(34, 42)),  # B-34 ~ B-41
    "C": list(range(1, 6)),    # C-1 ~ C-5
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "B": set(range(6, 42)),    # B-6 ~ B-41
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
    "连续运行",
    "单步",
    "指令预算",
    "让出控制权",
    "零升版",
    "残留",
):
    if literal not in text:
        err(f"[字面量] 规格里缺 {literal}")

# --- 7b. §1.2 事实表 F1~F16 连续 -------------------------------------------
f_rows = re.findall(r"^\|\s*(F\d+)\s*\|", text, re.M)
expected_f = [f"F{i}" for i in range(1, 17)]
if f_rows != expected_f:
    err(f"[事实表] §1.2 的 F 编号不是 F1~F16 连续（实测 {f_rows}）")

# --- 7c. §8 施工期订正 ------------------------------------------------------
sec8 = re.search(r"^## §8 .*$", text, re.M)
if not sec8:
    err("[订正] 缺 §8")
else:
    tail = text[sec8.end():]
    stop = re.search(r"^## §9\b", tail, re.M)
    sec8_body = tail[:stop.start()] if stop else tail
    corr = re.findall(r"^### 订正 (\d+) · (.+)$", sec8_body, re.M)
    if not corr:
        err("[订正] §8 只有占位、没有真的订正条目")
    else:
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

# --- 7e/7f/7g/7h：落点必须真的在仓内 ---------------------------------------
def require_exports(rel, names, tag):
    src = read(rel)
    if src is None:
        err(f"[落点] 找不到 {rel}")
        return
    for name in names:
        if name not in src:
            err(f"[落点] {rel} 里没有导出 `{name}`（{tag}）")


require_exports(RUNLOOP_REL,
                ("nextRunAction", "runStateAfter", "initialRunState"),
                "连续运行的纯函数")

app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    for literal in ("debug.run", "nextRunAction"):
        if literal not in app_src:
            err(f"[落点] {APP_REL} 里没有 `{literal}`"
                "（连续运行的落点没落地 ⇒ ⇒ 「运行」还是只走一个窗口）")

index_src = read(INDEX_REL)
if index_src is None:
    err(f"[落点] 找不到 {INDEX_REL}")
else:
    if 'data-topbar-action="pause"' not in index_src:
        err(f"[落点] {INDEX_REL} 里没有顶栏的「暂停」按钮"
            "（连续模式没有真实出口 ⇒ ⇒ 真机门禁会被拖死）")

# --- 7h. `case "pause"` 分支**不**发 `Pause` 命令 --------------------------
# ⚠️ **这是本批最容易「改错」的一条**（F6 实测：残留的暂停会吃掉下一次运行
#    的**一整轮**）⇒ ⇒ 判据必须落在「那个分支里**没有** `Command.pause()`」
#    ⇒ ⇒ 光判「状态变了」的话，「停续发 **且** 发 `Pause`」照样绿。
if app_src is not None:
    m = re.search(r'case\s+"pause"\s*:(.*?)(?=\n\s*case\s+"|\n\s*\}\s*$)',
                  app_src, re.S)
    if not m:
        err(f"[落点] {APP_REL} 里找不到 `case \"pause\":` 分支")
    elif "Command.pause()" in m.group(1):
        err(f"[落点] {APP_REL} 的 `case \"pause\"` 分支里发了 `Command.pause()`"
            " ⇒ ⇒ 内核会留下 pending 暂停 ⇒ ⇒ 下一次「运行」会被吃掉一整轮"
            "（F6 实测）⇒ ⇒ 界面上的「暂停」**只能**停续发")

# --- 7i. 台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.52" not in ledger_src:
        err("[台账] 台账里没有 v0.52 的批次记录")
    if "STAGE-37" not in ledger_src:
        err("[台账] 台账里没有 STAGE-37 的引用")

# --- 7k. §9 的变异编号 T1 ~ T9 + T-C1 齐全 -------------------------------
mut_rows = re.findall(r"^\|\s*(T\d+|T-C1)\s*\|", text, re.M)
expect_muts = [f"T{i}" for i in range(1, 10)] + ["T-C1"]
if mut_rows != expect_muts:
    err(f"[变异] §9 的变异编号不是 T1~T9 + T-C1（实测 {mut_rows}）")

# --- 7j. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage37_check.py"
if not self_path.exists():
    err("[自指] tools/stage37_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
