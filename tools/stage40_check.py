#!/usr/bin/env python3
"""STAGE-40 冻结规格（v0.55 · 串口 RX 中断堵死修复批 · D-158）的自检。

用法：python3 tools/stage40_check.py docs/detailed-design/STAGE-40_V055_Interrupt_Release.md

⚠️ **七项 + 子项**（与 `tools/stage39_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~V 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F20 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.1 的表是唯一权威处）
       ＋ 7e. `front/lib/message.js` 的 `SCHEMA_VERSION` **仍**是 8（**前端镜像零改动**）
       ＋ 7f. **四处修法**的源码落点（M-16 的撤线 / M-16 的 `release_active_line` /
          M-13 的 trap-return 出参 / M-17 的接线 / 宿主第 ② 步的撤销 /
          宿主第 ③ 步的 `None` 分支）
       ＋ 7g. 前端**行结束符**的四档常量与默认档
       ＋ 7h. 台账必须登记本批（`v0.55` + `STAGE-40`）
       ＋ 7i. 脚本必须自指存在
       ＋ 7j. §9 的**变异编号** `T1 ~ T12` + `T-C1` + `T-C2` + `BV-M1` + `BV-C1`
          **齐全**，且每条都有**非空的实测红集**（⛔ 不许占位）
       ＋ 7k. 裁决 C 的**正反两面**：必须出现「`mret` 不清」那个锚点，且**不许**
          出现「mret … 清 …」的**肯定式**表述

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **编号空间**（裁决 S）：
    * **A 组** = Rust 门禁 ⇒ ⇒ **每批范围**（A-1 ~ A-15）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-8）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-61 ~ B-66，接 STAGE-39 的 B-60）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-2）
⇒ ⇒ **前端那一组刻意用 `N` 而不是 `B`** —— `B` 是**真机输出**的编号空间，
   同一个输出里 ID 必须**全局唯一** ⇒ ⇒ 复用 `B-1 ~ B-8` 会与 STAGE-32 的
   `B-1 ~ B-5` **撞名**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-40_V055_Interrupt_Release.md")
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

M16_REL = "crates/kernel/src/m16_interrupt/mod.rs"
M13_ISS_REL = "crates/kernel/src/m13_iss/mod.rs"
M13_EXEC_REL = "crates/kernel/src/m13_iss/executor.rs"
M17_REL = "crates/kernel/src/m17_backend/mod.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
VIEWS_REL = "front/views/m02_viewcomponents.js"
MESSAGE_REL = "front/lib/message.js"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~V = 22 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(22)]


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

# --- 4. 裁决编号 A~V 无悬空引用 --------------------------------------------
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*([A-V])\s*\|", line.strip())
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
#: ⚠️ **本规格必须定义**的验收编号（裁决 S 的四个编号空间）。
EXPECT_ACCEPT = {
    "A": list(range(1, 16)),   # A-1 ~ A-15（Rust 门禁 · 每批范围）
    "N": list(range(1, 9)),    # N-1 ~ N-8（前端 Node 门禁 · 每批范围）
    "B": list(range(61, 67)),  # B-61 ~ B-66（真机门禁 · 跨批次唯一）
    "X": list(range(1, 3)),    # X-1 ~ X-2（交叉 · 每批范围）
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-60 也算「已定义」。
    "B": set(range(1, 67)),
    "X": set(EXPECT_ACCEPT["X"]),
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
    "电平语义",
    "服务中",
    "行结束符",
    "零升版",
    "释放",
    "claim",
):
    if literal not in text:
        err(f"[字面量] 规格里缺 {literal}")

# --- 7b. §1.2 事实表 F1~F20 连续 -------------------------------------------
f_rows = re.findall(r"^\|\s*(F\d+)\s*\|", text, re.M)
expected_f = [f"F{i}" for i in range(1, 21)]
if f_rows != expected_f:
    err(f"[事实表] §1.2 的 F 编号不是 F1~F20 连续（实测 {f_rows}）")

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

# --- 7e~7h：落点必须真的在仓内 -------------------------------------------
def need(rel, literals, tag):
    src = read(rel)
    if src is None:
        err(f"[落点] 找不到 {rel}")
        return None
    for lit in literals:
        if lit not in src:
            err(f"[落点] {rel} 里没有 `{lit}`"
                f" ⇒ ⇒ {tag}")
    return src


# 7e：前端镜像**零改动** ⇒ 版本号仍是 8
message_src = read(MESSAGE_REL)
if message_src is None:
    err(f"[落点] 找不到 {MESSAGE_REL}")
else:
    m = re.search(r"SCHEMA_VERSION\s*=\s*(\d+)", message_src)
    if not m:
        err(f"[版本号] {MESSAGE_REL} 里找不到 `SCHEMA_VERSION`")
    elif m.group(1) != "8":
        err(f"[版本号] {MESSAGE_REL} 的 `SCHEMA_VERSION` 是 {m.group(1)}"
            " ⇒ ⇒ 本批**契约层零升版** ⇒ ⇒ 前端镜像**不许**动")

# 7f：四处修法的源码落点
need(M16_REL, [
    "pub fn release_active_line",
    "self.active_irq = NO_ACTIVE_IRQ;",
    "fn release_active_line(&mut self) {}",
    "self.clear(IRQ_TIMER_COMPARE);",
], "M-16 的「释放服务中标记」与「定时器撤线」两处修法**没有落点**")
need(M13_ISS_REL, [
    "last_trap_return",
    "take_last_trap_return",
], "M-13 的 trap-return 出参**没有落点** ⇒ ⇒ 「服务结束」这件事没人报")
need(M13_EXEC_REL, [
    "returned_from_trap",
], "M-13 的执行器**没有** trap-return 出参 ⇒ ⇒ 接线点不存在")
need(M17_REL, [
    "take_last_trap_return",
    "release_active_line",
], "M-17 的**接线**没有落点 ⇒ ⇒ M-13 报的事**没人接**")
need(HOST_REL, [
    "p.set_pending(wire.source, false);",
    "clear(IRQ_UART_RX_READY);",
], "宿主 `sync_interrupts` 的两处修法（第 ② 步撤销 / 第 ③ 步 `None` 分支）**没有落点**")

# 7g：前端**行结束符**的四档常量与默认档
views_src = read(VIEWS_REL)
if views_src is None:
    err(f"[落点] 找不到 {VIEWS_REL}")
else:
    for lit in (
        "TERMINAL_EOL",
        'TERMINAL_EOL_DEFAULT = "cr"',
        "crlf",
        "data-eol",
    ):
        if lit not in views_src:
            err(f"[落点] {VIEWS_REL} 里没有 `{lit}`"
                " ⇒ ⇒ 前端**行结束符**那四档没有落点")

# 7h：台账必须登记本批 --------------------------------------------------
ledger_src = read(LEDGER_REL)
if ledger_src is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    if "v0.55" not in ledger_src:
        err("[台账] 台账里没有 v0.55 的批次记录")
    if "STAGE-40" not in ledger_src:
        err("[台账] 台账里没有 STAGE-40 的引用")
    # ⚠️ **必须排除「候选清单」那一行** —— 它同时写了「v0.55 批的候选」
    #    与「STAGE-40」⇒ ⇒ 不排除它就是**零判别力**（STAGE-38 的教训）。
    registered = [
        ln for ln in ledger_src.splitlines()
        if "v0.55" in ln and "STAGE-40" in ln and "候选" not in ln
    ]
    if not registered:
        err("[台账] 台账里**没有一行同时**提到 v0.55 与 STAGE-40"
            "（⇒ ⇒ 「本批已登记」这件事还没有**一处**落点）")
        err("[台账] ⚠️ **「候选清单」那一行不算登记**"
            "（它同时写了「v0.55 批的候选」与「STAGE-40」）⇒ ⇒ "
            "必须另有一行**批次登记行**同时提到这两个词")

# --- 7i. 自检脚本自指 ------------------------------------------------------
self_path = REPO_ROOT / "tools/stage40_check.py"
if not self_path.exists():
    err("[自指] tools/stage40_check.py 不存在")

# --- 7j. §9 的变异编号 T1 ~ T12 + T-C1 + T-C2 + BV-M1 + BV-C1 齐全 -------------
mut_rows = re.findall(r"^\|\s*(T\d+|T-C1|T-C2|BV-M1|BV-C1)\s*\|", text, re.M)
expect_muts = ([f"T{i}" for i in range(1, 13)]
                + ["T-C1", "T-C2", "BV-M1", "BV-C1"])
if sorted(mut_rows) != sorted(expect_muts):
    err(f"[变异] §9 的变异编号不是 T1~T12 + T-C1 + T-C2 + BV-M1 + BV-C1"
        f"（实测 {mut_rows}）")
elif len(set(mut_rows)) != len(mut_rows):
    err(f"[变异] §9 的变异编号有**重号**（实测 {mut_rows}）")
else:
    # ⚠️ **每条变异都必须写出「实测红集」** ⇒ ⇒ 且**不许**是占位 ⇒ ⇒
    #    「规格引用了一条不存在的判据」比「没有那条判据」更坏（它让人以为有）。
    PLACEHOLDERS = ("待测", "待补", "待定", "TBD", "—", "-")
    for line in text.split("\n"):
        m = re.match(r"^\|\s*(T\d+|T-C1|T-C2|BV-M1|BV-C1)\s*\|", line)
        if not m:
            continue
        cells = split_row(line)
        if len(cells) < 3:
            err(f"[变异] {m.group(1)} 那行**列数不够**（实测 {len(cells)}）")
            continue
        red = cells[2].strip()
        if not red:
            err(f"[变异] {m.group(1)} 的**实测红集**是空的"
                " ⇒ ⇒ 那条变异**没人管**")
        elif red in PLACEHOLDERS:
            err(f"[变异] {m.group(1)} 的**实测红集**是占位：{red!r}")

# --- 7k. 裁决 C 的正反两面 --------------------------------------------------
if "`mret` 不清" not in text:
    err("[裁决 C] 规格里没有「`mret` 不清」那个锚点"
        " ⇒ ⇒ 裁决 C 的**正面表述**丢了")
# ⚠️ **肯定式表述不许出现** —— 「mret … 清 …」且**没有**否定词 ⇒ ⇒
#    那就是「本批替 guest 在 trap 返回时清了架构挂起位」⇒ ⇒ 与裁决 C 冲突。
NEGATIONS = ("不清", "不许", "不碰", "不做", "不替")
for m in re.finditer(r"mret[^\n]{0,30}清", text):
    seg = m.group(0)
    if not any(neg in seg for neg in NEGATIONS):
        err(f"[裁决 C] 出现了「mret … 清 …」的**肯定式**表述：{seg!r}"
            " ⇒ ⇒ 裁决 C 说本批**不做**那件事")

# ---------------------------------------------------------------------------

print(f"检查项：7  文件 1  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
