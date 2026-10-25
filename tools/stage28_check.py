#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-28 规格自检（七项 + 七个子项，与 stage27_check.py 同构）。

用法：python3 tools/stage28_check.py docs/detailed-design/STAGE-28_V043_Store_Code_Clock_Visibility.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（A-1~A-8 / T-1~T-4 / X-1~X-6）
    7. 关键约束字面量钉住（含已废止字面量的禁列）
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F12 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. 三个版本号的值必须还是原值（本批**不升**）
       ＋ 7e. 跨层一致性：§1.2 的 **F 表逐行**核对该行点名的**文件**与**标识符**
                （原文从规格里读 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红）
       ＋ 7f. **D-135 的两个落点必须在位**（诊断包的显式时钟字段 / `judge_window` 改走
                `classify_window_with_clock` / 旧函数签名一字未改）
       ＋ 7g. **D-157 的拆号必须在位**（旧常量删干净 / 新码在位 / 两个触发点各用各的 /
                前端不撞内核的号）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7c / 7f / 7g 都是「逐条判」、不是「数全局次数」** —— 这正是 **STAGE-26 §8 订正 8** 的教训：
    上一批的 7c 数的是 §8 整节的「已改成」**出现次数** ⇒ ⇒ §8 开头那句散文里的
    「已改成什么」**顶替**了某一条订正 ⇒ ⇒ 删掉一条真的「已改成」也**不红**
    ⇒ ⇒ **零判别力** ⇒ ⇒ 本脚本**不许**重犯同一个错（**裁决 N**）。

⚠️ **7e 的口径是「标识符级」而不是「结论级」** —— §1.2 的 F 表记录的是「**冻结时刻**」的
    实测状态 ⇒ ⇒ 本批**会改掉其中几条的「结论」**（F2 / F7 / F11）⇒ ⇒ 但**「落点」
    （文件与标识符）不变** ⇒ ⇒ 7e 钉的是**落点** ⇒ ⇒ 结论的变化记在 **§8**。
⚠️ **7e 的「修前事实」要留痕**：F7 / F11 那两行的标识符（`E_STORE_01_ARCHIVE_FAILED`）
    **本批会删掉** ⇒ ⇒ 修掉它必然把它从源文件里删掉 ⇒ ⇒ 「只查源文件」这条判据与
    「修复」**互斥** ⇒ ⇒ 允许**台账留痕兜底**（与 stage27 的 `FALLBACK_LEDGER_ROWS` 同构）。

⚠️ **7f / 7g 在本批施工完成之前是红的** ⇒ ⇒ 这是**诚实**的（本批的落点还没落地）⇒ ⇒
    **批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-28_V043_Store_Code_Clock_Visibility.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7e 的兜底落点）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **哪些行允许「台账兜底」** —— 判据是「**这条事实是修前的，修掉它就必然
#:    把它从源文件里删掉**」⇒ ⇒ 允许查「源文件**或** 台账」。
FALLBACK_LEDGER_ROWS = {"F7", "F11"}


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
    # ---- 内核落点（D-135）----
    "crates/kernel/src/m22_hms",
    "crates/kernel/src/kernel_host",
    "crates/kernel/src/m23_datastore",
    "DiagnosticKit",
    "kit_to_json",
    "classify_window_with_clock",
    "classify_window",
    "judge_window",
    "Hms::new",
    # ---- 前端与工具落点（D-157）----
    "front/services/m23_boardstore.js",
    "front/lib/message.js",
    "protocolError",
    "tools/error_code_check.py",
    "E_STORE_01_ARCHIVE_FAILED",
    "E_STORE-04",
    "E_STORE-05",
    # ---- 台账与批次 ----
    "D-135",
    "D-157",
    "STAGE-28",
    "STAGE-27",
    # ---- 纪律 ----
    "一个约束一个落点",
    "不升",
    "protocolError",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    不是「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ⚠️ **D-157 的做法 A 必须是「错的」**（裁决 D）⇒ ⇒ 判据落在 §2.3 上。
sec23 = "\n".join(section(r"^###\s+§2\.3\b"))
if "A" not in sec23 or "证伪" not in sec23:
    err("[钉住] §2.3 没有写清「D-157 的做法 A 被实测证伪」")
for mark in ("A′", "B′", "C′"):
    if mark not in sec23:
        err(f"[钉住] §2.3 没有给出替代做法 {mark}")

# ⚠️ **号段规则**（裁决 G）必须说清 ⇒ ⇒ 判据落在 §2.4 上。
sec24 = "\n".join(section(r"^###\s+§2\.4\b"))
if "不许用同一个号" not in sec24:
    err("[钉住] §2.4 没有写清「前端自产码与内核自产码不许用同一个号」")
for kw in ("豁免", "逐条判"):
    if kw not in sec24:
        err(f"[钉住] §2.4 没有写清「{kw}」这条口径")

# ---- 禁列（已废止 / 不许出现的形态） -------------------------------------
# ⚠️ **「不做触发器」**这条（§0.2 第 4 条）⇒ ⇒ 判据落在**结论性标题**上 ⇒ ⇒
#    标题里出现「实现触发器」= 把没做的事说成做了 ⇒ ⇒ 红。
for line in lines:
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

# ---- 7e. 跨层一致性：§1.2 的 F 表**逐行**核对文件与标识符 ----------------
# ⚠️ **原文与路径都从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红。
#: ⚠️ **口径是「标识符级」而不是「结论级」**（见文件头的说明）⇒ ⇒ 本批**改掉
#:    F2 / F7 / F11 的「结论」**不红（它们改的是**结论**、**落点不变**）。
#: ⚠️ **「修前的事实」允许台账兜底**（F7 / F11）⇒ ⇒ 修掉它必然把它从源文件里删掉
#:    ⇒ ⇒ 「只查源文件」与「修复」**互斥**。
PATH_TOKEN_RE = re.compile(r"^[A-Za-z0-9_./-]+\.(?:rs|js|mjs|py|md|html|txt|sh)$")
#: ⚠️ **token 的合法性**：不许含箭头、不许含 Markdown 粗体标记、不许含省略号
#:    ⇒ ⇒ 这三样出现 ⇒ ⇒ **那不是标识符**（是散文）⇒ ⇒ 不参与判定。
TOKEN_BAD = ("⇒", "**", "…")


def _norm(s: str) -> str:
    """比对方去掉 Markdown 粗体标记与转义竖线。"""
    return s.replace("*", "").replace("\\|", "|").strip()


_ledger_text = None
for row in rows12:
    if not row:
        continue
    m = re.fullmatch(r"(F\d+)", row[0])
    if not m:
        continue
    fid = m.group(1)
    # ⚠️ **事实那一格**（第 2 格）⇒ ⇒ 「来源」那一格是散文，不参与。
    if len(row) < 2:
        err(f"[跨层] §1.2 的 {fid} 行只有 {len(row)} 格 ⇒ ⇒ 抽不出「事实」那一格")
        continue
    fact = row[1]
    toks = [t for t in re.findall(r"`([^`]+)`", fact)
            if t.strip() and not any(b in t for b in TOKEN_BAD)]
    paths = [t for t in toks if "/" in t and PATH_TOKEN_RE.match(t)]
    idents = [t for t in toks if t not in paths]
    if not paths:
        err(f"[跨层] §1.2 的 {fid} 行里抽不出**源文件路径** ⇒ ⇒ 7e 检查**静默失效** ⇒ ⇒ 必须红")
        continue
    blobs = []
    missing_paths = []
    for rel in paths:
        target = REPO_ROOT / rel
        if not target.exists():
            missing_paths.append(rel)
            continue
        blobs.append(target.read_text(encoding="utf-8"))
    if missing_paths:
        for rel in missing_paths:
            err(f"[跨层] 找不到 {fid} 指名的源文件 {rel} ⇒ ⇒ 跨层一致性检查**静默失效** ⇒ ⇒ 必须红")
    if not blobs:
        continue
    blob = "\n".join(blobs)
    for tok in idents:
        if tok in blob:
            continue
        # ⚠️ **台账兜底**（只对「修前的事实」开放 ⇒ ⇒ 见 FALLBACK_LEDGER_ROWS）。
        if fid in FALLBACK_LEDGER_ROWS:
            if _ledger_text is None:
                ledger = REPO_ROOT / LEDGER_REL
                if not ledger.exists():
                    err(f"[跨层] 找不到台账 {LEDGER_REL} ⇒ ⇒ 兜底检查**静默失效** ⇒ ⇒ 必须红")
                    _ledger_text = ""
                else:
                    _ledger_text = _norm(ledger.read_text(encoding="utf-8"))
            if tok in _ledger_text:
                # ⇒ ⇒ 原文在台账的**留痕**里在位 ⇒ ⇒ 这是「修前事实」的**正确落点**。
                continue
        err(f"[跨层] {fid} 引用的标识符 {tok} 在它点名的源文件里**逐字找不到**"
            f"（台账兜底也不在位）⇒ ⇒ 规格与实现漂移了")

# ---- 7f. D-135 的两个落点必须在位 ----------------------------------------
#: ⚠️ **落点从规格里读**（§3.0 的 lane 表）⇒ ⇒ 规格改了、本检查自动跟着改。
_KIT_REL = "crates/kernel/src/m22_hms/mod.rs"
_HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
_WATCHDOG_REL = "crates/kernel/src/kernel_host/watchdog.rs"

_kit = REPO_ROOT / _KIT_REL
_host = REPO_ROOT / _HOST_REL
_watchdog = REPO_ROOT / _WATCHDOG_REL
for rel in (_KIT_REL, _HOST_REL, _WATCHDOG_REL):
    if not (REPO_ROOT / rel).exists():
        err(f"[落地] 找不到 {rel} ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")

if _kit.exists():
    kit_txt = _kit.read_text(encoding="utf-8")
    # ---- 7f-① `DiagnosticKit` 有一个**显式**的布尔「时钟可用」字段 ----
    km = re.search(r"pub\s+struct\s+DiagnosticKit\s*\{(.*?)\n\}", kit_txt, re.S)
    if km is None:
        err("[落地] `DiagnosticKit` 的定义没找到 ⇒ ⇒ 7f-① 检查**静默失效** ⇒ ⇒ 必须红")
    else:
        body = km.group(1)
        fields = re.findall(r"(?m)^\s*pub\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([^,]+),", body)
        clock_bools = [
            (name, typ) for name, typ in fields
            if "bool" in typ and "clock" in name.lower()
        ]
        if len(clock_bools) == 0:
            err("[落地] `DiagnosticKit` 里**没有**「显式的布尔时钟可用字段」"
                "（字段名含 clock 且类型是 bool）⇒ ⇒ D-135 的落点不在位")
        elif len(clock_bools) > 1:
            err(f"[落地] `DiagnosticKit` 里有 {len(clock_bools)} 个布尔时钟字段"
                f"（{clock_bools}）⇒ ⇒ 「一个约束一个落点」被破了")
        # ⚠️ **不许用数值 0 兼职**（裁决 K）⇒ ⇒ 名字含 clock 的字段里**不许**
        #    有**数值型**的另一个 ⇒ ⇒ 那会让人以为 0 就是「不可用」。
        clock_numeric = [
            (name, typ) for name, typ in fields
            if "clock" in name.lower() and "bool" not in typ
        ]
        if clock_numeric:
            err(f"[落地] `DiagnosticKit` 里有**数值型**的时钟字段 {clock_numeric}"
                " ⇒ ⇒ ⛔ 不许用数值 0 兼职「时钟不可用」")

if _host.exists():
    host_txt = _host.read_text(encoding="utf-8")
    # ---- 7f-② `judge_window` 改走 `classify_window_with_clock` ----
    jm = re.search(r"(?s)fn\s+judge_window\b.*?\n\}", host_txt)
    if jm is None:
        err("[落地] `judge_window` 的定义没找到 ⇒ ⇒ 7f-② 检查**静默失效** ⇒ ⇒ 必须红")
    else:
        jbody = jm.group(0)
        if "classify_window_with_clock(" not in jbody:
            err("[落地] `judge_window` **没有**改走 `classify_window_with_clock`"
                " ⇒ ⇒ D-135 的宿主侧接线没落地")
        # ⚠️ **裸的 `classify_window(`** ⇒ ⇒ 它是**旧**那个（`classify_window_with_clock(`
        #    里的 `classify_window` 后面跟的是 `_` ⇒ ⇒ **不会**被这条误抓）。
        if re.search(r"(?<![\w_])classify_window\s*\(", jbody):
            err("[落地] `judge_window` 里**还在**调旧的 `classify_window`"
                " ⇒ ⇒ 宿主侧接线没落地")

if _watchdog.exists():
    wd_txt = _watchdog.read_text(encoding="utf-8")
    # ---- 7f-③ 旧函数仍在、且**签名一字未改** ----
    want_sig = "pub fn classify_window(elapsed_ms: u64, stalled: bool) -> WindowOutcome"
    if want_sig not in wd_txt:
        err(f"[落地] `classify_window` 的签名**一字未改**这条不成立"
            f"（{_WATCHDOG_REL} 里找不到 `{want_sig}`）")
    if "pub fn classify_window_with_clock(" not in wd_txt:
        err(f"[落地] `classify_window_with_clock` 不在 {_WATCHDOG_REL} 里 ⇒ ⇒ 7f 检查**静默失效**")

# ---- 7g. D-157 的拆号必须在位 --------------------------------------------
_STORE_REL = "crates/kernel/src/m23_datastore/mod.rs"
_HMS_REL = "crates/kernel/src/m22_hms/mod.rs"
_BOARDSTORE_REL = "front/services/m23_boardstore.js"

#: ⚠️ **旧码**（本批必须删干净）⇒ ⇒ **只此一处**定义。
OLD_STORE_CONST = "E_STORE_01_ARCHIVE_FAILED"

#: ⚠️ **新码的场景号**（内核那两个触发点）⇒ ⇒ **只此一处**定义。
NEW_STORE_NUMS = {"04", "05"}

#: ⚠️ **前端占的那三个**（内核不许撞）⇒ ⇒ **只此一处**定义。
FRONT_STORE_NUMS = {"01", "02", "03"}

for rel in (_STORE_REL, _HMS_REL, _BOARDSTORE_REL):
    if not (REPO_ROOT / rel).exists():
        err(f"[落地] 找不到 {rel} ⇒ ⇒ 7g 检查**静默失效** ⇒ ⇒ 必须红")

# ---- 7g-① 旧常量在全 `crates/**` 里**零命中** ----
crates_dir = REPO_ROOT / "crates"
if crates_dir.is_dir():
    hits = []
    for path in sorted(crates_dir.rglob("*.rs")):
        rel = path.relative_to(REPO_ROOT).as_posix()
        try:
            body = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if OLD_STORE_CONST in body:
            hits.append(rel)
    if hits:
        err(f"[落地] 内核**仍然**定义着旧常量 {OLD_STORE_CONST}：{hits}"
            f" ⇒ ⇒ 「旧码要删干净」这条不成立")

# ---- 7g-② 内核 STORE 族：新码在位、且不与前端撞号 ----
if (REPO_ROOT / _STORE_REL).exists():
    store_txt = (REPO_ROOT / _STORE_REL).read_text(encoding="utf-8")
    defined_nums = set(re.findall(r"(?m)^\s*pub\s+const\s+E_STORE_(\d{2})_[A-Z0-9_]*\s*:", store_txt))
    if not defined_nums:
        err(f"[落地] {_STORE_REL} 里**一个** `E_STORE_NN_*` 常量都没有"
            f" ⇒ ⇒ 7g-② 检查**静默失效** ⇒ ⇒ 必须红")
    missing_new = NEW_STORE_NUMS - defined_nums
    if missing_new:
        err(f"[落地] 内核**新码不在位**：{_STORE_REL} 里缺场景号 {sorted(missing_new)}"
            f" ⇒ ⇒ D-157 的拆号没落地")
    clash = FRONT_STORE_NUMS & defined_nums
    if clash:
        err(f"[落地] 内核 STORE 场景号 {sorted(clash)} 与前端占的"
            f" {sorted(FRONT_STORE_NUMS)} **重叠** ⇒ ⇒ 撞号没让开")

# ---- 7g-③ 两个触发点**各用各的**码 ----
if (REPO_ROOT / _HMS_REL).exists():
    hms_txt = (REPO_ROOT / _HMS_REL).read_text(encoding="utf-8")
    used = set(re.findall(r"\bE_STORE_(\d{2})_[A-Z0-9_]*\b", hms_txt))
    missing_used = NEW_STORE_NUMS - used
    if missing_used:
        err(f"[落地] {_HMS_REL} 里**没有**用到场景号 {sorted(missing_used)}"
            f" ⇒ ⇒ 两个触发点没各用各的码")
    if len(used & NEW_STORE_NUMS) < 2:
        err(f"[落地] {_HMS_REL} 里用到的 STORE 码少于两个（{sorted(used)}）"
            f" ⇒ ⇒ 两个触发点还共用一个码")

# ---- 7g-④ 前端**不许**撞内核的新号 ----
front_dir = REPO_ROOT / "front"
if front_dir.is_dir():
    front_hits = []
    for path in sorted(front_dir.rglob("*")):
        if not path.is_file() or path.suffix not in (".js", ".mjs"):
            continue
        rel = path.relative_to(REPO_ROOT).as_posix()
        # ⚠️ `front/tests/` 与 `front/dev/` **不算**（测试 / 模拟内核的替身）。
        if rel.startswith("front/tests/") or rel.startswith("front/dev/"):
            continue
        try:
            body = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        for num in NEW_STORE_NUMS:
            if f"E_STORE-{num}" in body:
                front_hits.append(f"{rel}: E_STORE-{num}")
    if front_hits:
        err(f"[落地] 前端运行时用到了内核的新号：{front_hits}"
            f" ⇒ ⇒ 撞号没让开")

print(f"检查项：7（+ 7b/7c/7d/7e/7f/7g）  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
