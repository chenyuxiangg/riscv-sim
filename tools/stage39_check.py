#!/usr/bin/env python3
"""STAGE-39 冻结规格（v0.54 · 调试器基础能力批）的自检。

用法：python3 tools/stage39_check.py docs/detailed-design/STAGE-39_V054_Debugger_Basics.md

⚠️ **七项 + 子项**（与 `tools/stage38_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~L 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1 ~ A-16 / B-55 ~ B-60 / C-1 ~ C-3）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F16 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.4 的表是唯一权威处）
       ＋ 7e. `front/app.js` 的 `case "read-mem"` 分支**不得**出现字面量 `0x100`
       ＋ 7f. `front/app.js` 的 `case "set-breakpoint"` 分支**有** `params.addr`
       ＋ 7g. `front/views/m02_viewcomponents.js` 里有 `data-bp-addr-input` 与 `data-mem-addr-input`
       ＋ 7h. `front/lib/message.js` 的 `SCHEMA_VERSION` **仍**是 8（**前端镜像零改动**）
       ＋ 7i. 台账必须登记本批（`v0.54` + `STAGE-39`）
       ＋ 7j. 脚本必须自指存在
       ＋ 7k. §9 的**变异编号** `T1 ~ T6` + `T-C1` **齐全**

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一** ⇒ ⇒ 本批从
   **B-55** 起（STAGE-33 用了 B-6~B-11 · STAGE-34 用了 B-12~B-18 · STAGE-35 用了
   B-19~B-26 · STAGE-36 用了 B-27~B-33 · STAGE-37 用了 B-34~B-41 · STAGE-38 用了
   B-42~B-54）⇒ ⇒ **不重用**。
   **A 组与 C 组**是**每批范围**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-39_V054_Debugger_Basics.md")
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

APP_REL = "front/app.js"
VIEWS_REL = "front/views/m02_viewcomponents.js"
MESSAGE_REL = "front/lib/message.js"
LEDGER_REL = "docs/reviews/REVIEWS.md"


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

# --- 4. 裁决编号 A~L 无悬空引用 --------------------------------------------
DECISIONS = [chr(ord("A") + i) for i in range(12)]  # A..L
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*([A-L])\s*\|", line.strip())
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
    "A": list(range(1, 17)),  # A-1 ~ A-16
    # ⚠️ **B 从 55 起**：**B 组落在同一个真机门禁输出里** ⇒ ⇒ **跨批次唯一**
    #    ⇒ ⇒ **不重用**。
    "B": list(range(55, 61)),  # B-55 ~ B-60
    "C": list(range(1, 4)),    # C-1 ~ C-3
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "B": set(range(6, 61)),    # B-6 ~ B-60
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
    "任意地址",
    "视图状态",
    "事件委托",
    "不预校验",
    "零升版",
    "参数",
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
def case_branch(src, case_name):
    """抽出 `case "<case_name>":` 到**下一个** `case "` 之间的那段文本。"""
    m = re.search(rf'case\s+"{re.escape(case_name)}"\s*:', src)
    if not m:
        return None
    nxt = re.search(r'\n\s*case\s+"', src[m.end():])
    return src[m.end(): m.end() + nxt.start()] if nxt else src[m.end():]


app_src = read(APP_REL)
if app_src is None:
    err(f"[落点] 找不到 {APP_REL}")
else:
    # 7e：读内存分支里**不得**出现写死的 `0x100`
    branch = case_branch(app_src, "read-mem")
    if branch is None:
        err(f"[落点] {APP_REL} 里没有 `case \"read-mem\"`")
    elif "0x100" in branch:
        err(f"[落点] {APP_REL} 的 `case \"read-mem\"` 分支里**还有**字面量 `0x100`"
            " ⇒ ⇒ 读内存的地址**仍然写死** ⇒ ⇒ 「任意地址」没落地")
    elif "params" not in branch:
        err(f"[落点] {APP_REL} 的 `case \"read-mem\"` 分支里**没有**读 `params`"
            " ⇒ ⇒ 地址 / 宽度**不是**从界面来的")

    # 7f：断点分支必须从 `params` 取地址
    bp_branch = case_branch(app_src, "set-breakpoint")
    if bp_branch is None:
        err(f"[落点] {APP_REL} 里没有 `case \"set-breakpoint\"`")
    elif "params" not in bp_branch:
        err(f"[落点] {APP_REL} 的 `case \"set-breakpoint\"` 分支里**没有**读 `params`"
            " ⇒ ⇒ 断点地址**不是**从界面来的")

views_src = read(VIEWS_REL)
if views_src is None:
    err(f"[落点] 找不到 {VIEWS_REL}")
else:
    # 7g：两个输入框必须在视图层
    for hook in ("data-bp-addr-input", "data-mem-addr-input"):
        if hook not in views_src:
            err(f"[落点] {VIEWS_REL} 里没有 `{hook}`"
                " ⇒ ⇒ 界面**给不出**那个地址")

message_src = read(MESSAGE_REL)
if message_src is None:
    err(f"[落点] 找不到 {MESSAGE_REL}")
else:
    # 7h：前端镜像**零改动** ⇒ 版本号仍是 8
    m = re.search(r"SCHEMA_VERSION\s*=\s*(\d+)", message_src)
    if not m:
        err(f"[版本号] {MESSAGE_REL} 里找不到 `SCHEMA_VERSION`")
    elif m.group(1) != "8":
        err(f"[版本号] {MESSAGE_REL} 的 `SCHEMA_VERSION` 是 {m.group(1)}"
            " ⇒ ⇒ 本批**契约层零升版** ⇒ ⇒ 前端镜像**不许**动")

# --- 7i. 台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.54" not in ledger_src:
        err("[台账] 台账里没有 v0.54 的批次记录")
    if "STAGE-39" not in ledger_src:
        err("[台账] 台账里没有 STAGE-39 的引用")
    # ⚠️ **必须排除「候选清单」那一行** —— 它同时写了「v0.54 批的候选」
    #    与「STAGE-39」⇒ ⇒ 不排除它就是**零判别力**（STAGE-38 的教训）。
    registered = [
        ln for ln in ledger_src.splitlines()
        if "v0.54" in ln and "STAGE-39" in ln and "候选" not in ln
    ]
    if not registered:
        err("[台账] 台账里**没有一行同时**提到 v0.54 与 STAGE-39"
            "（⇒ ⇒ 「本批已登记」这件事还没有**一处**落点）")
        err("[台账] ⚠️ **「候选清单」那一行不算登记**"
            "（它同时写了「v0.54 批的候选」与「STAGE-39」）⇒ ⇒ "
            "必须另有一行**批次登记行**同时提到这两个词")

# --- 7k. §9 的变异编号 T1 ~ T6 + T-C1 齐全 ---------------------------------
mut_rows = re.findall(r"^\|\s*(T\d+|T-C1)\s*\|", text, re.M)
expect_muts = [f"T{i}" for i in range(1, 7)] + ["T-C1"]
if mut_rows != expect_muts:
    err(f"[变异] §9 的变异编号不是 T1~T6 + T-C1（实测 {mut_rows}）")

# --- 7j. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage39_check.py"
if not self_path.exists():
    err("[自指] tools/stage39_check.py 不存在")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
