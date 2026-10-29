#!/usr/bin/env python3
"""STAGE-41 冻结规格（v0.56 · 终端屏幕仿真批 · D-161）的自检。

用法：python3 tools/stage41_check.py docs/detailed-design/STAGE-41_V056_Terminal_Screen.md

⚠️ **七项 + 子项**（与 `tools/stage40_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~J 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F11 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续
       ＋ 7d. **四个版本号的值**（§5.1 的表是唯一权威处）
       ＋ 7e. 屏幕仿真模块的**关键落点**（`createTerminalScreen` /
          `renderTerminalScreen` / `TAB_WIDTH` / 那个文件本身）
       ＋ 7f. 台账必须登记本批（`v0.56` + `STAGE-41` + `D-161`）
       ＋ 7g. 脚本必须自指存在
       ＋ 7h. §9 的**变异编号** `T1 ~ T7` + `T-C1` + `BV-M1` + `BV-C1`
          **齐全**，且每条都有**非空的实测红集**（⛔ 不许占位）
       ＋ 7i. 裁决 B 的**措辞**：必须出现「LF 顺带 CR」那个锚点，且**不许**
          把「LF 顶格」写成**事实**

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**（裁决 J）：
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-15）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-67 ~ B-76，接 STAGE-40 的 B-66）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-2）
⇒ ⇒ **前端那一组用 `N` 而不是 `B`** —— `B` 是**真机输出**的编号空间，
   同一个输出里 ID 必须**全局唯一** ⇒ ⇒ 复用 `B-1 ~ B-15` 会与前几批**撞名**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-41_V056_Terminal_Screen.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约层零升版** ⇒ ⇒ 全不动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "8",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

SCREEN_REL = "front/views/components/terminal-screen.js"
VIEWS_REL = "front/views/m02_viewcomponents.js"
APP_REL = "front/app.js"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~J = 10 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(10)]


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

# --- 4. 裁决编号 A~J 无悬空引用 --------------------------------------------
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*\*\*([A-J])\*\*\s*\|", line.strip())
    if m:
        defined.add(m.group(1))
for d in DECISION_LETTERS:
    if d not in defined:
        err(f"[裁决] §2 缺裁决 {d}")
text = "\n".join(lines)
for d in DECISION_LETTERS:
    for m in re.finditer(rf"裁决\s*{d}\b", text):
        if d not in defined:
            err(f"[裁决] 正文引用了未定义的裁决 {d}")

# --- 5. 章节 §0~§10 完整 ---------------------------------------------------
for sec in range(0, 11):
    if not re.search(rf"^## §{sec}\b", text, re.M):
        err(f"[章节] 缺 §{sec}")

# --- 6. 验收编号交叉引用 ---------------------------------------------------
#: ⚠️ **本规格必须定义**的验收编号（三个编号空间）。
EXPECT_ACCEPT = {
    "N": list(range(1, 16)),      # N-1 ~ N-15（前端 Node 门禁 · 每批范围）
    "B": list(range(67, 77)),     # B-67 ~ B-76（真机门禁 · 跨批次唯一）
    "X": list(range(1, 3)),       # X-1 ~ X-2（交叉 · 每批范围）
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-66 也算「已定义」。
    "B": set(range(1, 77)),
    "X": set(EXPECT_ACCEPT["X"]),
}
for group, nums in EXPECT_ACCEPT.items():
    for n in nums:
        if not re.search(rf"^\|\s*\*\*{group}-{n}\*\*\s*\|", text, re.M):
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

# --- 7b. §1.2 事实表编号 F1~F11 连续 ---------------------------------------
facts = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*\*{0,2}(F\d+)\*{0,2}\s*\|", line.strip())
    if m:
        facts.add(m.group(1))
for i in range(1, 12):
    if f"F{i}" not in facts:
        err(f"[事实] §1.2 缺 F{i}")
for f in facts:
    n = int(f[1:])
    if not 1 <= n <= 11:
        err(f"[事实] §1.2 出现越界的事实编号 {f}")

# --- 7c. §8 施工期订正：不是占位 + 编号连续 --------------------------------
sec8_text = ""
sec8_at = re.search(r"^## §8\b", text, re.M)
sec9_at = re.search(r"^## §9\b", text, re.M)
if sec8_at and sec9_at:
    sec8_text = text[sec8_at.start(): sec9_at.start()]
corrections = re.findall(r"^### 订正 (\d+)", sec8_text, re.M)
for i, c in enumerate(corrections, 1):
    if int(c) != i:
        err(f"[订正] §8 的订正编号不连续：第 {i} 条写成「订正 {c}」")
if len(corrections) < 1:
    err("[订正] §8 一条施工期订正都没有")
if "待实测" in sec8_text or "待补" in sec8_text:
    err("[订正] §8 里还有占位（待实测 / 待补）")

# --- 7d. 四个版本号 --------------------------------------------------------
for name, value in EXPECT_VERSIONS.items():
    if not re.search(rf"\*{{0,2}}`?{name}`?\*{{0,2}}\s*仍?\s*\*{{0,2}}{re.escape(value)}\*{{0,2}}", text):
        err(f"[版本] §5.1 没钉住 {name} = {value}")

# --- 7e. 屏幕仿真模块的关键落点 --------------------------------------------
screen_src = read(SCREEN_REL)
if screen_src is None:
    err(f"[源码] 找不到 {SCREEN_REL}")
else:
    for token in ("createTerminalScreen", "renderTerminalScreen", "TAB_WIDTH"):
        if token not in screen_src:
            err(f"[源码] {SCREEN_REL} 里没有 {token}")
views_src = read(VIEWS_REL)
if views_src is None:
    err(f"[源码] 找不到 {VIEWS_REL}")
else:
    for token in ("terminal-screen.js", "resetTerminalScreen", "console-raw-toggle"):
        if token not in views_src:
            err(f"[源码] {VIEWS_REL} 里没有 {token}")
app_src = read(APP_REL)
if app_src is None:
    err(f"[源码] 找不到 {APP_REL}")
elif "resetTerminalScreen" not in app_src:
    err(f"[源码] {APP_REL} 里没有 resetTerminalScreen（换工程时屏幕没清）")

# --- 7f. 台账必须登记本批 --------------------------------------------------
ledger = read(LEDGER_REL)
if ledger is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    for token in ("v0.56", "STAGE-41", "D-161"):
        if token not in ledger:
            err(f"[台账] 台账里没有 {token}")

# --- 7g. 脚本自指存在 ------------------------------------------------------
self_path = REPO_ROOT / "tools" / "stage41_check.py"
if not self_path.exists():
    err("[脚本] tools/stage41_check.py 不存在")

# --- 7h. §9 变异编号齐全 + 每条都有非空红集 --------------------------------
EXPECT_MUTATIONS = ["T1", "T2", "T3", "T4", "T5", "T6", "T7", "T-C1", "BV-M1", "BV-C1"]
sec9_text = ""
sec10_at = re.search(r"^## §10\b", text, re.M)
sec9_at = re.search(r"^## §9\b", text, re.M)
if sec9_at and sec10_at:
    sec9_text = text[sec9_at.start(): sec10_at.start()]
for mut in EXPECT_MUTATIONS:
    if not re.search(rf"\b{re.escape(mut)}\b", sec9_text):
        err(f"[变异] §9 缺变异 {mut}")
# 红集表：每一行都必须有**非空**的红集（⛔ 不许占位）
red_rows = re.findall(r"^\|\s*(T\d|T-C\d|BV-M\d|BV-C\d)\s*\|(.*)$", sec9_text, re.M)
seen = set()
for mut, rest in red_rows:
    seen.add(mut)
    cell = rest.split("|")[0].strip()
    if not cell or cell.startswith("_") or "待实测" in cell:
        err(f"[变异] §9.3 的 {mut} 红集是占位（{cell!r}）")
    elif cell.startswith("（零红）") and mut not in ("T-C1", "BV-C1"):
        err(f"[变异] §9.3 的 {mut} 是零红 ⇒ ⇒ 它没有判别力")
for mut in EXPECT_MUTATIONS:
    if mut not in seen:
        err(f"[变异] §9.3 的红集表里缺 {mut}")
# 对照必须零红
for ctl in ("T-C1", "BV-C1"):
    m = re.search(rf"^\|\s*{re.escape(ctl)}\s*\|(.*)$", sec9_text, re.M)
    if m and "零红" not in m.group(1):
        err(f"[变异] 对照 {ctl} 必须是零红")

# --- 7i. 裁决 B 的措辞 -----------------------------------------------------
#
# ⚠️ **只看活着的正文**（排除 §8）：§8 是「**记做过什么**」⇒ ⇒ 那里**逐字**
#   引用旧措辞（「LF 顶格」当事实的那一版）⇒ ⇒ 扫它 ⇒ ⇒ **门禁被文档自己
#   触发** ⇒ ⇒ **零判别力**。
if "LF 顺带 CR" not in text:
    err("[措辞] 规格里没有「LF 顺带 CR」那个锚点（裁决 B 的口径变了？）")
# ⚠️ **凡是讲 LF 的事实行 ⇒ ⇒ 必须带上它的依据**（ONLCR / 回列首 / 顺带 CR）
#   ⇒ ⇒ 「LF 顶格」这件事**只能是裁决**（建模选择）⇒ ⇒ 不能是一条没有依据的
#   「事实」⇒ ⇒ 这条检查盯的是**事实表的每一行**，不是全文的关键词。
for line in body:
    if line is None:
        continue
    st = line.strip()
    if not re.match(r"^\|\s*\*{0,2}F\d+\*{0,2}\s*\|", st):
        continue
    if "顶格" in st and not any(k in st for k in ("ONLCR", "CR+LF", "回列首")):
        err(f"[措辞] 事实表里有一行讲「顶格」却没带依据（ONLCR / CR+LF）：{st[:120]}")
# ⚠️ **§8 必须记下「LF 顶格」从事实降级成裁决**这件事（订正 1）
if "LF 顶格" not in sec8_text:
    err("[措辞] §8 没有记「LF 顶格」降级成裁决这条订正")

# ---------------------------------------------------------------------------
print(f"检查 {spec}")
if fail:
    for f in fail:
        print("✗", f)
    print(f"⇒ {len(fail)} 处不过")
    sys.exit(1)
print("✓ 全部通过")
