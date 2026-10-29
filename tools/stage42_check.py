#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-42 冻结规格（v0.57 · 调试器增强批：符号查询 + 条件断点表达式 + 整段转储）的自检。

用法：python3 tools/stage42_check.py docs/detailed-design/STAGE-42_V057_Debugger_Enhancements.md

⚠️ **八项 + 子项**（与 `tools/stage41_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~Q 无悬空引用
    5. 章节 §1~§9 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F-1 ~ F-15 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续
       ＋ 7d. **四个版本号的值**（§2.8 的表是唯一权威处 ⇒ ⇒ `SCHEMA_VERSION` = 9）
       ＋ 7e. **跨层源代码锚点**（Rust 侧两条命令 / 两条事件 / `condition`
          字段 / 两个新码 / 两个上限常量；前端镜像的构造器 / topic /
          常量；界面 DOM 钩子）
       ＋ 7f. 台账必须登记本批（`v0.57` + `STAGE-42`）
       ＋ 7g. 脚本必须自指存在
       ＋ 7h. §9 的**变异编号**齐全，且每条都有**非空的实测红集**
       ＋ 7i. 条件表达式的**语法锚点**（§4.1 / §4.3 / §4.4）
    8. **跨层夹具**（`crates/contract/fixtures/debug-limits.json`）的两个
       上限值必须与规格逐字相符 ⇒ ⇒ 「两个真相结构上不可能」

⚠️ **编号空间**（与 STAGE-41 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约层（Rust）⇒ ⇒ **每批范围**（A-1 ~ A-8）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-15）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-13）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-77 ~ B-87，接 STAGE-41 的 B-76）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）
⇒ ⇒ **K 组不叫 B**：同一个输出里 ID 必须全局唯一 ⇒ ⇒ 复用 `B` 会与真机段
   **撞名** ⇒ ⇒ 内核那组另起一个字母。
"""
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-42_V057_Debugger_Enhancements.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约信封升版** ⇒ ⇒ 只有它动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "9",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

#: ⚠️ **两个上限的期望值**（第 8 项用）—— 与跨层夹具逐字相符。
EXPECT_LIMITS = {
    "dump_max_bytes": 4096,
    "symbol_candidate_limit": 16,
}

PROTOCOL_REL = "crates/contract/src/protocol.rs"
MODEL_REL = "crates/contract/src/model.rs"
ERROR_REL = "crates/contract/src/error.rs"
LIB_REL = "crates/contract/src/lib.rs"
FIXTURE_REL = "crates/contract/fixtures/debug-limits.json"
MIRROR_REL = "front/lib/message.js"
VIEWS_REL = "front/views/m02_viewcomponents.js"
APP_REL = "front/app.js"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~Q = 17 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(17)]

#: ⚠️ **验收编号空间**（三个「每批范围」+ 一个「跨批次唯一」）。
EXPECT_ACCEPT = {
    "A": list(range(1, 9)),       # A-1 ~ A-8（契约层）
    "K": list(range(1, 16)),      # K-1 ~ K-15（内核）
    "N": list(range(1, 14)),      # N-1 ~ N-13（前端 Node）
    "B": list(range(77, 88)),     # B-77 ~ B-87（真机门禁 · 跨批次唯一）
    "X": list(range(1, 4)),       # X-1 ~ X-3（交叉）
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-76 也算「已定义」。
    "B": set(range(1, 88)),
    "X": set(EXPECT_ACCEPT["X"]),
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

text = "\n".join(lines)

# --- 4. 裁决编号 A~P 无悬空引用 --------------------------------------------
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*\*\*([A-Q])\*\*\s*\|", line.strip())
    if m:
        defined.add(m.group(1))
for d in DECISION_LETTERS:
    if d not in defined:
        err(f"[裁决] §3 缺裁决 {d}")
for d in DECISION_LETTERS:
    for m in re.finditer(rf"裁决\s*{d}\b", text):
        if d not in defined:
            err(f"[裁决] 正文引用了未定义的裁决 {d}")

# --- 5. 章节 §1~§9 完整 ---------------------------------------------------
for sec in range(1, 10):
    if not re.search(rf"^## §{sec}\b", text, re.M):
        err(f"[章节] 缺 §{sec}")

# --- 6. 验收编号交叉引用 ---------------------------------------------------
for group, nums in EXPECT_ACCEPT.items():
    for n in nums:
        if not re.search(rf"^\|\s*\*{{0,2}}{group}-{n}\*{{0,2}}\s*\|", text, re.M):
            err(f"[验收] §6 缺 {group}-{n}")
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

# --- 7b. §1.2 事实表编号 F-1 ~ F-15 连续 -----------------------------------
facts = []
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*\*{0,2}(F-\d+)\*{0,2}\s*\|", line.strip())
    if m:
        facts.append(m.group(1))
for i in range(1, 16):
    if f"F-{i}" not in facts:
        err(f"[事实] §1.2 缺 F-{i}")

# --- 7c. §8 施工期订正：不是占位 + 编号连续 --------------------------------
sec8_text = ""
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
# ⚠️ **按「表行」判**（不是「名字与值相邻」）——§2.8 的表是
#    「常量 | 旧 | 新 | 理由」四列 ⇒ ⇒ 名字与新值**不同格** ⇒ ⇒
#    「相邻才匹配」的正则会**永远匹配不上**（工具自己把自己判红）。
for name, value in EXPECT_VERSIONS.items():
    m = re.search(rf"^\|\s*`?{re.escape(name)}`?\s*\|(.*)$", text, re.M)
    if not m:
        err(f"[版本] §2.8 的表里没有 {name} 那一行")
        continue
    cells = [c.strip().strip("*` ").strip() for c in m.group(1).split("|")]
    if len(cells) < 2:
        err(f"[版本] §2.8 的 {name} 那一行列数不够（至少要有「旧」「新」两格）")
        continue
    old, new = cells[0], cells[1]
    if new != value:
        err(f"[版本] §2.8 的 {name} 新值是 {new!r} ≠ {value!r}")
    # ⚠️ **另外三个版本号一个都不许升** ⇒ ⇒ 「旧」与「新」必须**逐字相同**。
    if name != "SCHEMA_VERSION" and old != new:
        err(f"[版本] §2.8 的 {name} 竟然从 {old!r} 变成 {new!r} ⇒ ⇒ 本批**只**升信封版本")

# --- 7e. 跨层源代码锚点 ---------------------------------------------------
#: (相对路径, 必须出现的字面量元组)
ANCHORS = [
    (PROTOCOL_REL, ("ResolveSymbol", "DumpMemory", "SymbolResolved", "MemoryDump")),
    (MODEL_REL, ("condition",)),
    (ERROR_REL, ("E_DBG_02_CONDITION_INVALID", "E_DBG_03_DUMP_TOO_LARGE")),
    (LIB_REL, ("DUMP_MAX_BYTES", "SYMBOL_CANDIDATE_LIMIT")),
    (MIRROR_REL, ("resolveSymbol", "dumpMemory", "symbolResolved", "memoryDump",
                  "debug.symbol", "debug.dump",
                  "DUMP_MAX_BYTES", "SYMBOL_CANDIDATE_LIMIT")),
    (VIEWS_REL, ("data-bp-symbol-input", "data-bp-cond-input",
                 "data-bp-symbol-candidates", "data-dump-addr-input", "data-mem-dump")),
    (APP_REL, ("lookup-symbol", "set-symbol-breakpoint", "dump-memory")),
]
for rel, tokens in ANCHORS:
    src = read(rel)
    if src is None:
        err(f"[源码] 找不到 {rel}")
        continue
    for token in tokens:
        if token not in src:
            err(f"[源码] {rel} 里没有 {token}")

# --- 7f. 台账必须登记本批 --------------------------------------------------
ledger = read(LEDGER_REL)
if ledger is None:
    err(f"[台账] 找不到 {LEDGER_REL}")
else:
    for token in ("v0.57", "STAGE-42"):
        if token not in ledger:
            err(f"[台账] 台账里没有 {token}")

# --- 7g. 脚本自指存在 ------------------------------------------------------
self_path = REPO_ROOT / "tools" / "stage42_check.py"
if not self_path.exists():
    err("[脚本] tools/stage42_check.py 不存在")

# --- 7h. §9 变异编号齐全 + 每条都有非空红集 --------------------------------
EXPECT_MUTATIONS = ["T1", "T2", "T3", "T4", "T5", "T6", "T7", "T8",
                     "T-C1", "BV-M1", "BV-C1"]
sec9_text = ""
sec10_at = re.search(r"^## §10\b", text, re.M)
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

# --- 7i. 条件表达式的语法锚点 ----------------------------------------------
# ⚠️ **规格的表格里 `||` 必须写成 `\|\|`**（GitHub 的表格切分会把裸管道
#    当列分隔 ⇒ ⇒ 渲染就散了）⇒ ⇒ 搜索前**先把转义去掉** ⇒ ⇒ 两种写法
#    都能被找到。
plain_text = text.replace("\\|", "|")
for literal in ("x1 == 5", "a0 != 0", "pc >= 0x80001000", "x1 == 5 && x2 != 0",
                "x1 == 5 || x2 == 7", "!(x1 == 0)", "s0 == sp"):
    if literal not in plain_text:
        err(f"[语法] 规格里缺条件表达式例子 {literal!r}")
# ⚠️ **v1 不支持内存读** ⇒ ⇒ 「mem[」这个写法**不许**出现在规格的语法里
#    （⇒ ⇒ 它会让人以为支持）。
if "mem[" in text:
    err("[语法] 规格里出现了 `mem[` ⇒ ⇒ v1 不支持内存读（裁决 G）")

# --- 8. 跨层夹具 -----------------------------------------------------------
fixture = read(FIXTURE_REL)
if fixture is None:
    err(f"[夹具] 找不到 {FIXTURE_REL}")
else:
    try:
        data = json.loads(fixture)
    except json.JSONDecodeError as exc:
        err(f"[夹具] {FIXTURE_REL} 不是合法 JSON：{exc}")
        data = {}
    for key, value in EXPECT_LIMITS.items():
        if key not in data:
            err(f"[夹具] {FIXTURE_REL} 缺 {key}")
        elif data[key] != value:
            err(f"[夹具] {FIXTURE_REL} 的 {key} = {data[key]!r} ≠ {value!r}")
    # 规格里也要写同样的值（两边都由夹具判定 ⇒ ⇒ 规格文字不能自相矛盾）
    if str(EXPECT_LIMITS["dump_max_bytes"]) not in text:
        err("[夹具] 规格里没写 dump 上限")
    if str(EXPECT_LIMITS["symbol_candidate_limit"]) not in text:
        err("[夹具] 规格里没写候选上限")

# ---------------------------------------------------------------------------
print(f"检查 {spec}")
if fail:
    for f in fail:
        print("✗", f)
    print(f"⇒ {len(fail)} 处不过")
    sys.exit(1)
print("✓ 全部通过")
