#!/usr/bin/env python3
"""STAGE-27 规格自检（七项 + 四个子项，与 stage26_check.py 同构）。

用法：python3 tools/stage27_check.py docs/detailed-design/STAGE-27_V042_Log_Health_Contract_Gate.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（A-1~A-8 / B-1~B-8 / C-1~C-4 / T-1~T-4 / X-1~X-6）
    7. 关键约束字面量钉住（含已废止字面量的禁列）＋ **三条 lane 的落点**
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F12 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. 三个版本号的值必须还是原值（本批**不升**）
       ＋ 7e. 跨层一致性：§1.2 的 **F1 / F2** 两行里**引号里的原文**必须与它们
                指名的那个源文件**逐字相同**（规格改了、源没改 ⇒ ⇒ 红）
       ＋ 7f. **D-128 的 LLD 订正真的落地**：§2.4 指名的那个 LLD 里，
                **旧措辞必须消失**、**新措辞要求的片段必须逐个在位**（规格 ≠ 落地 ⇒ ⇒ 红）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7c 是「逐条判」、不是「数全局次数」** —— 这正是 **STAGE-26 §8 订正 8** 的教训：
    上一批的 7c 数的是 §8 整节的「已改成」**出现次数** ⇒ ⇒ §8 开头那句散文里的
    「已改成什么」**顶替**了某一条订正 ⇒ ⇒ 删掉一条真的「已改成」也**不红**
    ⇒ ⇒ **零判别力** ⇒ ⇒ 本脚本**不许**重犯同一个错（**裁决 N**）。

⚠️ **本批的三个版本都不升**（裁决 A）⇒ ⇒ 第 7d 项按「**找到常量名 ⇒ 看紧跟其后的
    那个数字**」判定 ⇒ ⇒ ⛔ **不要**拿「`SCHEMA_VERSION = 8`」这种**规格里根本不存在的
    形态**当禁列 ⇒ ⇒ 那是**零判别力的死护栏**。

⚠️ **7e 的来源**：§1.2 的 **F1 行**（契约层那张表的原文）与 **F2 行**（实现侧那句注释的
    原文）是那两段文字的**唯一来源** ⇒ ⇒ 原文**从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒
    规格改了、源没改 ⇒ ⇒ 门禁红（⇒ ⇒ 规格与实现**漂移**看得见）。
⚠️ **那两个文件此刻可能还不存在** ⇒ ⇒ **文件不存在就报红**（检查静默失效 == 门禁恒绿）。

⚠️ **本批的 §8 在施工期之前是空的** ⇒ ⇒ 7c 会红（这是**诚实**的：规格还没写完）⇒ ⇒
    **批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-27_V042_Log_Health_Contract_Gate.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []


def err(msg):
    fail.append(msg)


def section(header_pat):
    """取出某小节的正文（不含标题行）⇒ ⇒ 到**同级或更高级**的下一个标题为止。"""
    out = []
    grab = False
    base = 0
    for line in lines:
        m = re.match(r"^(#{1,6})\s+", line)
        if m:
            lvl = len(m.group(1))
            if grab:
                if lvl <= base:
                    break
            elif re.match(header_pat, line):
                grab = True
                base = lvl
                continue
        if grab:
            out.append(line)
    return out


def table_rows(sec_lines):
    """取出小节里的表格行（每行切成格子）。"""
    rows = []
    for line in sec_lines:
        s = line.strip()
        if s.startswith("|") and s.endswith("|"):
            rows.append([c.strip().strip("*").strip() for c in split_row(s)])
    return rows


# ---- 1. Markdown 表格列数一致 --------------------------------------------
in_fence = False
table_start = None
table_cols = None
for i, line in enumerate(lines, 1):
    if line.lstrip().startswith("```"):
        in_fence = not in_fence
        continue
    if in_fence:
        continue
    if line.strip().startswith("|") and line.strip().endswith("|"):
        cols = len(split_row(line.strip()))
        if table_start is None:
            table_start, table_cols = i, cols
        elif cols != table_cols:
            err(f"[表格] 第 {i} 行列数 {cols} ≠ 首行 {table_cols}（表起于第 {table_start} 行）")
    else:
        table_start, table_cols = None, None

# ---- 2. 标题层级不跳级 ---------------------------------------------------
in_fence = False
prev = 0
for i, line in enumerate(lines, 1):
    if line.lstrip().startswith("```"):
        in_fence = not in_fence
        continue
    if in_fence:
        continue
    m = re.match(r"^(#{1,6})\s+\S", line)
    if not m:
        continue
    lvl = len(m.group(1))
    if prev and lvl > prev + 1:
        err(f"[标题] 第 {i} 行从 h{prev} 跳到 h{lvl}")
    prev = lvl

# ---- 3. 代码围栏配平 -----------------------------------------------------
n_fence = sum(1 for l in lines if l.lstrip().startswith("```"))
if n_fence % 2 != 0:
    err(f"[围栏] 围栏数 {n_fence} 为奇数 ⇒ 未配平")

# ---- 4. 裁决编号 A~N 无悬空引用 ------------------------------------------
verdicts = set()
for line in lines:
    m = re.match(r"^###\s*裁决\s*([A-N])\s*·", line)
    if m:
        verdicts.add(m.group(1))
expected = set("ABCDEFGHIJKLMN")
missing = expected - verdicts
if missing:
    err(f"[裁决] 裁决表缺 {sorted(missing)}")
extra = verdicts - expected
if extra:
    err(f"[裁决] 出现表外的裁决编号 {sorted(extra)}")
body = "\n".join(l for l in lines if not re.match(r"^###\s*裁决\s*[A-N]", l))
body_refs = set(re.findall(r"裁决\s*([A-Z])\b", body))
for v in sorted(body_refs):
    if v not in expected:
        err(f"[裁决] 正文引用了**表外**的裁决编号 {v}")
    elif v not in verdicts:
        err(f"[裁决] 正文引用了**没有标题**的裁决 {v}（悬空引用）")

# ---- 5. 章节完整 §0~§11 ---------------------------------------------------
heads = [l for l in lines if re.match(r"^##\s+§\d+", l)]
nums = sorted(int(re.search(r"§(\d+)", h).group(1)) for h in heads)
if nums != list(range(0, 12)):
    err(f"[章节] §0~§11 不完整，实得 {nums}")

# ---- 6. 验收编号交叉引用（§5 内每个编号都被 §5 定义） ---------------------
sec5 = section(r"^##\s+§5\b")
sec5_text = "\n".join(sec5)
defined = set()
sub_ids = set()
for line in sec5:
    m = re.match(r"^\s*(?:[-*+]\s+|\d+[.)]\s+|\|\s*)?\**\s*([ABCTX]-\d+)([a-z])?\b", line)
    if m:
        if m.group(2):
            sub_ids.add(m.group(1))
        else:
            defined.add(m.group(1))
ids = set(re.findall(r"\b([ABCTX]-\d+)\b", sec5_text))
undef = ids - defined
if undef:
    err(f"[验收] §5 内出现未定义的验收编号 {sorted(undef)}")
# ⚠️ **每个组的号段是钉死的** ⇒ ⇒ 「改软」规格（少写一条验收）也会红。
ACCEPT_EXPECT = {
    "A": [f"A-{i}" for i in range(1, 9)],      # A-1~A-8
    "B": [f"B-{i}" for i in range(1, 9)],      # B-1~B-8
    "C": [f"C-{i}" for i in range(1, 5)],      # C-1~C-4
    "T": [f"T-{i}" for i in range(1, 5)],      # T-1~T-4
    "X": [f"X-{i}" for i in range(1, 7)],      # X-1~X-6
}
for grp, want in ACCEPT_EXPECT.items():
    got = sorted((x for x in defined if x.startswith(grp + "-")),
                 key=lambda x: int(x.split("-")[1]))
    if got != want:
        err(f"[验收] {grp} 组编号不是 {want[0]}~{want[-1]} 连续：{got}")
for sub in sorted(sub_ids):
    if sub not in defined:
        err(f"[验收] 子编号 {sub}b 的**母号** {sub} 不在 §5 的验收里")
# ⚠️ **§5 之外不许出现未定义的验收编号** ⇒ ⇒ 别处引用一个不存在的号 ⇒ 红。
for m in re.finditer(r"\b([ABCTX]-\d+)\b", "\n".join(lines)):
    if m.group(1) not in defined:
        line_no = "\n".join(lines)[:m.start()].count("\n") + 1
        err(f"[验收] 第 {line_no} 行出现未在 §5 定义的验收编号 {m.group(1)}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    # ---- 契约层落点（D-127 / D-132）----
    "irq_clear",
    "clear_lines_by_mask",
    "crates/contract/src/csr.rs",
    "crates/contract/src/error.rs",
    "error_families!",
    "tools/error_code_check.py",
    # ---- 内核落点（D-133 / D-134 / D-135）----
    "crates/kernel/src/m21_logging",
    "crates/kernel/src/m22_hms",
    "crates/kernel/src/kernel_host/clock.rs",
    "crates/kernel/src/kernel_host/watchdog.rs",
    "query_recent",
    "time_virt_ps",
    "AdminDiagnosis",
    "log_ref",
    "retained",
    "available",
    "classify_window_with_clock",
    "classify_window",
    "HostInstant",
    "HostClock",
    # ---- 文档落点 ----
    "docs/detailed-design/M-21_Logging.html",
    "tests/golden/README.md",
    "rv32mi/breakpoint",
    "tdata1",
    "tselect",
    # ---- 台账与批次 ----
    "D-127",
    "D-128",
    "D-132",
    "D-133",
    "D-134",
    "D-135",
    "D-156",
    "STAGE-27",
    "STAGE-26",
    # ---- 纪律 ----
    "一个约束一个落点",
    "不升",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    不是「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ⚠️ **D-135 的核心护栏**必须说清 ⇒ ⇒ 判据落在「**提到 `available` 的那一段**」上。
if "available" not in text:
    err("[钉住] 规格没有写清 `available` 这条护栏")

# ⚠️ **「不许用数值 0 兼职」**这条（裁决 G）必须说清。
if "兼职" not in text:
    err("[钉住] 规格没有写清「⛔ 不许用数值 0 兼职『时钟不可用』」")

# ⚠️ **D-156 的做法 A 必须是「错的」**（裁决 K）⇒ ⇒ 判据落在 §4.1 上。
sec41 = "\n".join(section(r"^###\s+§4\.1\b"))
if "A" not in sec41 or "错" not in sec41:
    err("[钉住] §4.1 没有写清「D-156 的做法 A 是错的」")
for mark in ("A′", "B′", "C′"):
    if mark not in sec41:
        err(f"[钉住] §4.1 没有给出替代做法 {mark}")

# ⚠️ **「只登记、不搬」**这条（裁决 D）必须说清。
if "只登记" not in text or "不搬" not in text:
    err("[钉住] 规格没有写清「D-132 只登记、不搬」")

# ---- 禁列（已废止 / 不许出现的形态） -------------------------------------
# ⚠️ **「不做触发器」**这条（裁决 L）必须说清 ⇒ ⇒ 判据落在**结论性标题**上 ⇒ ⇒
#    标题里出现「实现触发器」= 把没做的事说成做了 ⇒ ⇒ 红。
_sec_no = None
for line in lines:
    m = re.match(r"^#{2,6}\s+§(\d+)(?:\.(\d+))?", line)
    if m:
        _sec_no = (int(m.group(1)),
                   int(m.group(2)) if m.group(2) else None)
    if not line.startswith("#"):
        continue
    if "实现触发器" in line and "不" not in line:
        err(f"[禁列] 结论性标题里出现「实现触发器」（本批**不**实现）：{line.strip()}")

# ---- 7b. §1.2 的事实表编号列必须是 F1~F12 连续 ---------------------------
sec12 = section(r"^###\s+§1\.2(?!\.1)\b")
rows12 = table_rows(sec12)
facts = []
for row in rows12:
    if not row:
        continue
    m = re.fullmatch(r"F(\d+)", row[0])
    if m:
        facts.append("F" + m.group(1))
want_facts = [f"F{i}" for i in range(1, 13)]
if facts != want_facts:
    err(f"[事实] §1.2 的实测事实表**编号列**不是 F1~F12 连续（无缺号/无重复/无越界）：{facts}")

# ---- 7c. §8 施工期订正：不是占位 + 编号连续 + **逐条**都有「已改成」 -----
sec8 = section(r"^##\s+§8\b")
sec8_text = "\n".join(sec8)
if "<!-- CORRECTIONS -->" in sec8_text or not sec8_text.strip():
    err("[订正] §8 还是占位（`<!-- CORRECTIONS -->`）⇒ ⇒ 施工期订正尚未落账")
else:
    corrections = []
    for line in sec8:
        m = re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", line)
        if m:
            corrections.append(int(m.group(1)))
    if not corrections:
        # 兜底：§8 写成表格时，第一列是编号。
        for line in sec8:
            m = re.match(r"^\|\s*(\d+)\s*\|", line)
            if m:
                corrections.append(int(m.group(1)))
    if not corrections:
        err("[订正] §8 里没有可识别的「订正」条目（形状变了？）")
    else:
        want_corr = list(range(1, len(corrections) + 1))
        if sorted(corrections) != want_corr:
            err(f"[订正] §8 的施工期订正编号不是 1..{len(corrections)} 连续：{sorted(corrections)}")
        # ⚠️ **必须逐条判，不能数全局次数**（STAGE-26 §8 订正 8 的教训）：
        #    §8 开头那句提示语里就有一个「**已改成什么**」⇒ ⇒ **全局计数**会把它
        #    当成某一条订正的「已改成」⇒ ⇒ **删掉某一条真的「已改成」也可能不红**
        #    ⇒ ⇒ **零判别力** ⇒ ⇒ 判据落在「**每一条订正自己的那段里**」。
        missing_changed = []
        for idx, line in enumerate(sec8):
            m = re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", line)
            if not m:
                continue
            # 这一条订正的正文 = 从它的标题到**下一条订正标题**（或 §8 末尾）为止。
            body_lines = []
            for follow in sec8[idx + 1:]:
                if re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", follow):
                    break
                body_lines.append(follow)
            if "已改成" not in "\n".join(body_lines):
                missing_changed.append(m.group(1))
        if missing_changed:
            err(
                f"[订正] §8 的这些订正**自己那段里**没有「已改成」：{missing_changed}"
                " ⇒ ⇒ 逐条判（不是数全局次数）"
            )

# ---- 7d. 三个版本号的值必须还是原值（**不升版**） ------------------------
# ⚠️ **不能**用「`SCHEMA_VERSION = 8`」这种字面量去禁 —— 规格里那三个版本号
#    **不是**用「= N」的形式写的（写的是「仍 **7**」）⇒ ⇒ 那种禁列**永远匹配
#    不上** ⇒ ⇒ **零判别力** ⇒ ⇒ 改成「**找到常量名 ⇒ 看紧跟其后的那个数字**」。
#: ⚠️ **一个数只服务一个口径**：这三个值由本项**独占**判定 ⇒ ⇒ 禁列里
#:    **不再**重复列它们（重复 ⇒ ⇒ 两个落点 ⇒ ⇒ 改一处漏一处）。
VERSION_PINS = [
    ("SCHEMA_VERSION", "7"),
    ("SIM_STATE_SCHEMA_VERSION", "2"),
    ("PLATFORM_SCHEMA_VERSION", "1"),
]
for const, want in VERSION_PINS:
    # ⚠️ `SCHEMA_VERSION` 是 `SIM_STATE_SCHEMA_VERSION` 与
    #    `PLATFORM_SCHEMA_VERSION` 的**子串** ⇒ ⇒ 必须排除被字母 / 数字 /
    #    下划线包住的那次命中 ⇒ ⇒ 否则会拿别的常量的值来判。
    pat = re.compile(r"(?<![A-Za-z0-9_])" + re.escape(const) + r"(?![A-Za-z0-9_])")
    # ⚠️ **窗口**：只判「紧跟在名字后面**一小段**里写的那个值」⇒ ⇒
    #    「`SIM_STATE_SCHEMA_VERSION` **升版**」那种**不写数字**的说法
    #    **不参与**判定。
    #: ⚠️ **独立数字**：紧跟字母 / 数字 / 下划线 / 连字符的数字**不算**
    #:    （`D-143` 的 143 是**欠账编号**，不是版本号）。
    #: ⚠️ **不许判空**：每个常量**至少有一处**必须把值写出来 ⇒ ⇒ 否则
    #:    本项会**恒绿**（零判别力）⇒ ⇒ 那也是红。
    WINDOW = 24
    n_stated = 0
    for m in pat.finditer(text):
        win = text[m.end():m.end() + WINDOW]
        dm = re.search(r"(?<![A-Za-z0-9_-])(\d)", win)
        if not dm:
            continue
        n_stated += 1
        if dm.group(1) != want:
            line_no = text[:m.start()].count("\n") + 1
            err(f"[禁列] 第 {line_no} 行把 {const} 的值写成 {dm.group(1)}"
                f"（本批**不升** ⇒ 必须是 {want}）")
    if n_stated == 0:
        err(f"[禁列] 规格没有在**任何一处**把 {const} 的值（{want}）写出来"
            f"⇒ ⇒ 本项会恒绿 ⇒ ⇒ 必须红")

# ---- 7e. 跨层一致性：§1.2 的 F1 / F2 两行里**引号里的原文** -------------
# ⚠️ **原文从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红。
#: ⚠️ **每行两样东西**：① 它指名的**源文件**（`crates/...`）② 它引用的**原文**（「…」）。
#:    两者**都**从那一行里抽 ⇒ ⇒ **不写死** ⇒ ⇒ 规格是唯一来源。
_repo_root = Path(__file__).resolve().parent.parent
FILE_RE = re.compile(r"(crates/[A-Za-z0-9_./-]+\.(?:rs|py))")
QUOTE_RE = re.compile(r"「([^」]{8,})」")

#: ⚠️ **台账路径**（F1 的兜底落点）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **哪些行允许「台账兜底」** —— 判据是「**这条事实是修前的，修掉它就必然
#:    把它从源文件里删掉**」⇒ ⇒ 它的原文的**正确落点**是台账的**留痕**
#:    （本批 T-1 要求「正文按实测改写并保留原表述留痕」）⇒ ⇒ 查「源文件
#:    **或** 台账」。
#:    · **F1** = 修前的事实（那张表里 `irq_clear` 曾被列在 internal line 栏）
#:      ⇒ ⇒ **D-127 的修复必然把那一行从源文件里删掉** ⇒ ⇒ 「只查源文件」
#:      这条判据与「修复」**互斥** ⇒ ⇒ ⇒ 允许兜底。
#:      ⚠️ 源文件那一侧的保证**另有**用例钉住（Lane B 的
#:      `the_three_spaces_table_keeps_irq_clear_out_of_the_line_column`
#:      逐字读源文件）⇒ ⇒ 这里放宽**不丢**判别力。
#:    · **F2** = 实现侧**当前**的注释 ⇒ ⇒ **只查源文件** ⇒ ⇒ **不放宽**。
FALLBACK_LEDGER_ROWS = {"F1"}


def _norm(s: str) -> str:
    """比对方去掉 Markdown 粗体标记与转义竖线。"""
    return s.replace("*", "").replace("\\|", "|").strip()


for row in rows12:
    if not row or row[0] not in ("F1", "F2"):
        continue
    raw = "| " + " | ".join(row) + " |"
    fm = FILE_RE.search(raw)
    if not fm:
        err(f"[跨层] §1.2 的 {row[0]} 行里抽不出**源文件路径**")
        continue
    rel = fm.group(1)
    qm = QUOTE_RE.search(raw)
    if not qm:
        err(f"[跨层] §1.2 的 {row[0]} 行里抽不出**引号里的原文**")
        continue
    target = _repo_root / rel
    if not target.exists():
        # ⚠️ **文件没了 ⇒ ⇒ 跨层检查静默失效** ⇒ ⇒ 与「找不到候选清单块」同形
        err(f"[跨层] 找不到 {row[0]} 指名的源文件 {rel} ⇒ ⇒ 跨层一致性检查**静默失效** ⇒ ⇒ 必须红")
        continue
    body_txt = _norm(target.read_text(encoding="utf-8"))
    quoted = _norm(qm.group(1))
    if quoted in body_txt:
        continue
    # ⚠️ **台账兜底**（只对「修前的事实」开放 ⇒ ⇒ 见 FALLBACK_LEDGER_ROWS）。
    if row[0] in FALLBACK_LEDGER_ROWS:
        ledger = _repo_root / LEDGER_REL
        if not ledger.exists():
            err(f"[跨层] 找不到台账 {LEDGER_REL} ⇒ ⇒ 兜底检查**静默失效** ⇒ ⇒ 必须红")
            continue
        if quoted in _norm(ledger.read_text(encoding="utf-8")):
            # ⇒ ⇒ 原文在台账的**留痕**里在位 ⇒ ⇒ 这是「修前事实」的**正确落点**。
            continue
    err(f"[跨层] {row[0]} 引用的原文在 {rel} 里**逐字找不到**"
        f"（台账兜底也不在位）⇒ ⇒ 规格与实现漂移了")

# ---- 7f. D-128 的 LLD 订正**真的落地**（规格 ≠ 落地 ⇒ ⇒ 必须红）--------------
#: ⚠️ **这一项钉的是「落地」**：规格里写了「改成有界长度」⇒ ⇒ **LLD 没改** ⇒ ⇒
#:    门禁必须红 ⇒ ⇒ 否则「D-128 已闭环」这句话**没有证据**。
#: ⚠️ **路径与措辞都从规格里读**（§2.4）⇒ ⇒ **规格是唯一来源** ⇒ ⇒ 规格改了、
#:    本检查自动跟着改 ⇒ ⇒ 不会「规格与脚本各说一套」。
#: ⚠️ **禁列 = 「被替换掉的那段旧措辞」** ⇒ ⇒ 它是**历史**（与「规格现在说什么」
#:    无关）⇒ ⇒ 硬编码在这一处是安全的 ⇒ ⇒ 「不许再出现」这条约束**只此一处**。
_sec24 = "\n".join(section(r"^###\s+§2\.4\b"))
_lld_path_m = re.search(r"(docs/[A-Za-z0-9_./-]+\.html)", _sec24)
_quoted24 = re.findall(r"「([^」]{6,})」", _sec24)
if _lld_path_m is None:
    err("[跨层] §2.4 里抽不出 LLD 的路径 ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
elif len(_quoted24) < 2:
    err("[跨层] §2.4 里抽不出「旧措辞 → 新措辞」两段「…」原文 ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
else:
    _lld_rel = _lld_path_m.group(1)
    _old24 = _norm(_quoted24[0])
    _new24 = _norm(_quoted24[1])
    _lld_file = _repo_root / _lld_rel
    if not _lld_file.exists():
        err(f"[跨层] 找不到 {_lld_rel} ⇒ ⇒ D-128 的落地检查**静默失效** ⇒ ⇒ 必须红")
    else:
        _lld_txt = _norm(_lld_file.read_text(encoding="utf-8"))
        if _old24 in _lld_txt:
            err(f"[跨层] {_lld_rel} 里**旧措辞还在**（「{_old24}」）⇒ ⇒ D-128 只改了规格、没改落地")
        # ⚠️ **正列 = 新措辞里的代码片段**（逐个）⇒ ⇒ 规格是唯一来源。
        for _tok in re.findall(r"`([^`]+)`", _new24):
            if _tok not in _lld_txt:
                err(f"[跨层] {_lld_rel} 里找不到新措辞要求的「{_tok}」⇒ ⇒ D-128 的订正没落到地面")
        # ⚠️ **两个中文要点词**必须**同时**在规格与 LLD 里 ⇒ ⇒ 规格改了措辞 ⇒ ⇒
        #    本检查**显式报红**（而不是静默地失效）。
        for _kw in ("有界长度", "UTF-8 字符边界"):
            if _kw not in _new24:
                err(f"[跨层] §2.4 的新措辞里没有「{_kw}」⇒ ⇒ 规格措辞变了 ⇒ ⇒ 请同步 7f 的要点词")
            elif _kw not in _lld_txt:
                err(f"[跨层] {_lld_rel} 里没有「{_kw}」⇒ ⇒ D-128 的订正没落到地面")

print(f"检查项：7（+ 7b/7c/7d/7e/7f）  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
