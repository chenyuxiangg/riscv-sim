#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-38 规格自检（七项 + 十一个子项，与 stage37_check.py 同构）。

用法：python3 tools/stage38_check.py docs/detailed-design/STAGE-38_V053_Running_Readout.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-20 / B-42~B-54 / C-1~C-6）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F16 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.7 的表是唯一权威处）
       ＋ 7e. `front/lib/run-loop.js` 存在且导出 `isKernelRunning` / `bumpBusy` / `liveValue`
       ＋ 7f. `front/lib/alert.js` 存在且导出 `showAlert` / `hideAlert` / `alertState`
       ＋ 7g. `front/index.html` 里弹窗骨架**存在**且**不在视图正文里**
       ＋ 7h. `front/app.js` 的 `case "read-mem"` 分支里**有**运行中拒绝的判据
       ＋ 7i. 台账必须登记本批（`v0.53` + `STAGE-38`）
       ＋ 7j. 脚本必须自指存在
       ＋ 7k. §9 的**变异编号** `T1 ~ T13` + `T-C1` **齐全**（`stage37_check.py`
                有这一项 ⇒ ⇒ **不要**照抄它的编号范围）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一** ⇒ ⇒ 本批从
   **B-42** 起（STAGE-33 用了 B-6~B-11 · STAGE-34 用了 B-12~B-18 · STAGE-35 用了
   B-19~B-26 · STAGE-36 用了 B-27~B-33 · STAGE-37 用了 B-34~B-41）⇒ ⇒ **不重用**。
   **A 组与 C 组**是**每批范围**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-38_V053_Running_Readout.md")
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
ALERT_REL = "front/lib/alert.js"
ECHARTS_REL = "front/lib/echarts-loader.js"

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
    "A": list(range(1, 21)),  # A-1 ~ A-20
    # ⚠️ **B 从 42 起**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一**
    #    （STAGE-33 用了 B-6 ~ B-11 · STAGE-34 用了 B-12 ~ B-18 · STAGE-35 用了
    #    B-19 ~ B-26 · STAGE-36 用了 B-27 ~ B-33 · STAGE-37 用了 B-34 ~ B-41）
    #    ⇒ ⇒ **不重用**。
    "B": list(range(42, 55)),  # B-42 ~ B-54
    "C": list(range(1, 7)),    # C-1 ~ C-6
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "B": set(range(6, 55)),    # B-6 ~ B-54
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
    "运行中",
    "活值",
    "固定符号",
    "拒绝",
    "弹窗",
    "渐隐",
    "知道了",
    "零升版",
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
                ("isKernelRunning", "bumpBusy", "liveValue"),
                "运行中判据与活值口径的纯函数")

require_exports(ALERT_REL,
                ("showAlert", "hideAlert", "alertState"),
                "告警弹窗")

require_exports(ECHARTS_REL,
                ("loadECharts", "setChartSource", "getChartSource"),
                "波形库加载器")

index_src = read(INDEX_REL)
if index_src is None:
    err(f"[落点] 找不到 {INDEX_REL}")
else:
    if "data-alert-overlay" not in index_src:
        err(f"[落点] {INDEX_REL} 里没有告警弹窗的骨架"
            "（运行中读内存的告警就没有落点）")
    else:
        # ⚠️ **必须在视图挂载容器之外**（裁决 J）⇒ ⇒ 否则视图重渲染会把它销毁。
        #
        # ⚠️ **容器是 `<main id="app">`** —— 视图正文（`[data-view-body=...]`）
        #    是 M-01 在**运行时**建出来的 ⇒ ⇒ `index.html` 里**找不到**那个
        #    字符串 ⇒ ⇒ 判据必须盯**真正**会被重建的那块。
        #
        # ⚠️ **先把 HTML 注释剔掉**⇒ ⇒ 注释里会**逐字提到** `<main id="app">`
        #    （那段注释就在讲弹窗骨架住在顶层）⇒ ⇒ 不剔的话判据会命中**注释**
        #    里的那个位置 ⇒ ⇒ **假红**。
        bare = re.sub(r"<!--.*?-->", "", index_src, flags=re.S)
        overlay_at = bare.find("data-alert-overlay")
        main_at = bare.find("<main")
        main_end = bare.find("</main>")
        if main_at == -1 or main_end == -1:
            err(f"[落点] {INDEX_REL} 里找不到视图挂载容器 `<main>`"
                " ⇒ ⇒ 判据「弹窗不在视图正文里」**没法判**")
        elif main_at < overlay_at < main_end:
            err(f"[落点] {INDEX_REL} 里告警弹窗的骨架**在 `<main>` 里面**"
                " ⇒ ⇒ 视图重渲染会把它销毁（裁决 J / F12）")

app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    for literal in ("debug.run", "isKernelRunning", "showAlert"):
        if literal not in app_src:
            err(f"[落点] {APP_REL} 里没有 `{literal}`"
                "（运行中隐藏活值 / 读内存告警的落点没落地）")

# --- 7h. `case "read-mem"` 分支里**有**运行中拒绝的判据 ----------------------
# ⚠️ **这是本批最容易被「改错」的一条**：拒绝必须落在**那个分支里** ⇒ ⇒
#    光判「界面上有弹窗」的话，「拒绝执行但仍然发命令」照样绿。
if app_src is not None:
    m = re.search(r'case\s+"read-mem"\s*:(.*?)(?=\n\s*case\s+"|\n\s*\}\s*$)',
                  app_src, re.S)
    if not m:
        err(f"[落点] {APP_REL} 里找不到 `case \"read-mem\":` 分支")
    else:
        branch = m.group(1)
        if "isRunning" not in branch and "isKernelRunning" not in branch:
            err(f"[落点] {APP_REL} 的 `case \"read-mem\"` 分支里**没有**运行中判据"
                " ⇒ ⇒ 运行中读内存不会被拒绝")
        if "readMemory" not in branch:
            err(f"[落点] {APP_REL} 的 `case \"read-mem\"` 分支里**没有**读内存"
                " ⇒ ⇒ 「读内存」这条命令彻底没了")

# --- 7i. 台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.53" not in ledger_src:
        err("[台账] 台账里没有 v0.53 的批次记录")
    if "STAGE-38" not in ledger_src:
        err("[台账] 台账里没有 STAGE-38 的引用")
    # ⚠️ **两处都出现**才算登记 ⇒ ⇒ 「v0.53 批的候选」那几行里只有
    #    「v0.53」、候选清单那几行里只有「STAGE-38」⇒ ⇒ **单看一个词会假绿**。
    #
    # ⚠️⚠️ **但「同一行有这两个词」本身也会假绿**（STAGE-38 · v0.53 批 ·
    #    施工期订正 8）：「§2.1 候选清单」那一行**恰好**同时写了
    #    「v0.53 批的候选」与「I2C 实际落 STAGE-38 及再往后」⇒ ⇒ 在**本批
    #    还没登记**的时候，这条判据就已经是绿的 ⇒ ⇒ **零判别力**。
    # ⇒ ⇒ **必须排除「候选清单」那一行** ⇒ ⇒ 只认「**批次登记行**」。
    ledger_lines = ledger_src.splitlines()
    registered = [
        ln for ln in ledger_lines
        if "v0.53" in ln and "STAGE-38" in ln and "候选" not in ln
    ]
    if not registered:
        err("[台账] 台账里**没有一行同时**提到 v0.53 与 STAGE-38"
            "（⇒ ⇒ 「本批已登记」这件事还没有**一处**落点）")
        err("[台账] ⚠️ **「候选清单」那一行不算登记**"
            "（它同时写了「v0.53 批的候选」与「STAGE-38」）⇒ ⇒ "
            "必须另有一行**批次登记行**同时提到这两个词")

# --- 7k. §9 的变异编号 T1 ~ T12 + T-C1 齐全 -------------------------------
mut_rows = re.findall(r"^\|\s*(T\d+|T-C1)\s*\|", text, re.M)
expect_muts = [f"T{i}" for i in range(1, 14)] + ["T-C1"]
if mut_rows != expect_muts:
    err(f"[变异] §9 的变异编号不是 T1~T13 + T-C1（实测 {mut_rows}）")

# --- 7j. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage38_check.py"
if not self_path.exists():
    err("[自指] tools/stage38_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
