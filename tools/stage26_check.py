#!/usr/bin/env python3
"""STAGE-26 规格自检（七项 + 四个子项，与 stage25_check.py 同构）。

用法：python3 tools/stage26_check.py docs/detailed-design/STAGE-26_V041_Mtval_Time_Vacuity.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（A-1~A-5 / B-1~B-10 / C-1~C-6 / T-1~T-5 / X-1~X-6）
    7. 关键约束字面量钉住（含已废止字面量的禁列）＋ **三条 lane 的落点**
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F13 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + 每条都有「已改成」
       ＋ 7d. 三个版本号的值必须还是原值（本批**不升**）
       ＋ 7e. 跨层一致性：§1.2 的 F1 / F3 两行里的地址必须与语料两处逐字相同

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **本批的三个版本都不升**（裁决 A）⇒ ⇒ 第 7d 项按「**找到常量名 ⇒ 看紧跟其后的
那个数字**」判定 ⇒ ⇒ ⛔ **不要**拿「`SCHEMA_VERSION = 8`」这种**规格里根本不存在的
形态**当禁列 ⇒ ⇒ 那是**零判别力的死护栏**（上一批刚踩过）。

⚠️ **7e 的地址来源**：§1.2 的 **F1 行**（参考侧报的那个数）与 **F3 行**（平台报的那个
数）是地址的**唯一来源** ⇒ ⇒ 地址**从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了
地址，语料两处必须跟着改。
⚠️ **那两处文件此刻可能还不存在**（另一条 lane 正在写）⇒ ⇒ **文件不存在就报红**
（检查静默失效 == 门禁恒绿）。

⚠️ **本批的 §8 在施工期之前是空的** ⇒ ⇒ 7c 会红（这是**诚实**的：规格还没写完）⇒ ⇒
**批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-26_V041_Mtval_Time_Vacuity.md")
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
# ⚠️ **正文**（不含 §7 的裁决标题行）里引用的裁决编号 ⇒ ⇒ 必须都在表内。
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
    "A": [f"A-{i}" for i in range(1, 6)],      # A-1~A-5
    "B": [f"B-{i}" for i in range(1, 11)],     # B-1~B-10
    "C": [f"C-{i}" for i in range(1, 7)],      # C-1~C-6
    "T": [f"T-{i}" for i in range(1, 6)],      # T-1~T-5
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
    # ---- 契约层落点（裁决 B / F / G / K）----
    "pmp_fault_piece_addr",
    "pmp_access_allowed",
    "TimeSource",
    "time_ticks",
    "csr_read_with_time",
    "crates/contract/src/csr.rs",
    "crates/contract/src/port.rs",
    # ---- 内核落点（裁决 B / D / E / G / H / K）----
    "crates/kernel/src/m13_iss/pmp.rs",
    "crates/kernel/src/m13_iss/executor.rs",
    "crates/kernel/src/m13_iss/csr.rs",
    "pmp_fault_addr",
    "pmp_allows",
    "mtime_cycles",
    "Clint",
    # ---- 关键地址与 CSR 号 ----
    "0x8004_0000",
    "0x8003_FFFE",
    "0xC01",
    "0xC81",
    # ---- 三条 lane 的工具落点 ----
    "tools/vacuity_check.py",
    "local/pmp_straddle.S",
    "tests/golden/corpus.txt",
    "tests/golden/README.md",
    # ---- 台账与批次 ----
    "D-151",
    "D-154",
    "D-155",
    "D-96",
    "STAGE-26",
    "STAGE-25",
    "STAGE-24",
    # ---- 规范原文（裁决 N）----
    "Table 52",
    # ---- 纪律 ----
    "一个约束一个落点",
    "取指",
    "不升",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    **不是**「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ⚠️ **「有真值源就报真值，没有就报 0」这条护栏**必须说清 ⇒ ⇒ 判据落在
#    **提到 `time` 读值的那一段**上 ⇒ ⇒ 少了它 = 护栏被悄悄删了。
if "永远不从 CSR 文件读" not in text:
    err("[钉住] 规格没有写清「`time` / `timeh` **永远不从 CSR 文件读」这条护栏")

# ⚠️ **「不许用有效地址 + 偏移重算」**这条（裁决 C）必须说清。
if "有效地址 + 偏移" not in text:
    err("[钉住] 规格没有写清「⛔ 不许用『有效地址 + 偏移』重算失败片」")

# ⚠️ **「D-151 是与参考侧不再可比的改动」**这条明示取舍（裁决 J）必须说清。
if "不再可比" not in text:
    err("[钉住] 规格没有写清「D-151 之后 `time` **与参考侧不再可比**」这条明示取舍")

# ---- 禁列（已废止 / 不许出现的形态） -------------------------------------
# ⚠️ **D-151 的做法 C**（把 `mtime` 塞进 `SimState`）**不许**被写成「要做」⇒ ⇒
#    禁的是「**升版**」这个动作在结论性标题里出现 ⇒ ⇒ §1.3 / §8 **豁免**
#    （那两处正是记录「被推翻的旧做法」的地方）。
_sec_no = None
for line in lines:
    m = re.match(r"^#{2,6}\s+§(\d+)(?:\.(\d+))?", line)
    if m:
        _sec_no = (int(m.group(1)),
                   int(m.group(2)) if m.group(2) else None)
    if not line.startswith("#"):
        continue
    if _sec_no in ((1, 3), (8, None)):
        continue
    if "做法 C" in line:
        err(f"[禁列] 结论性标题里出现「做法 C」（旧做法只许出现在 §1.3 / §8）：{line.strip()}")

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
    #    **不参与**判定（否则会一路读到下一个数字 —— 上一版就栽在这儿：
    #    读到的是「D-143」里的 1 ⇒ ⇒ 假红）。
    #: ⚠️ **独立数字**：紧跟字母 / 数字 / 下划线 / 连字符的数字**不算**
    #    （`D-143` 的 143 是**欠账编号**，不是版本号）。
    #: ⚠️ **不许判空**：每个常量**至少有一处**必须把值写出来 ⇒ ⇒ 否则
    #    本项会**恒绿**（零判别力）⇒ ⇒ 那也是红。
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

# ---- 7b. §1.2 的实测事实表：编号列必须**恰好是 F1~F13** ------------------
# ⚠️ **删掉一条实测事实**也是「把规格改软」的一种 ⇒ ⇒ 事实表本身要钉住。
sec12 = section(r"^###\s+§1\.2(?!\.1)\b")
facts = []
for row in table_rows(sec12):
    if not row:
        continue
    m = re.fullmatch(r"F(\d+)", row[0])
    if m:
        facts.append("F" + m.group(1))
want_facts = [f"F{i}" for i in range(1, 14)]
if facts != want_facts:
    err(f"[事实] §1.2 的实测事实表**编号列**不是 F1~F13 连续（无缺号/无重复/无越界）：{facts}")

# ---- 7c. §8 施工期订正：不是占位 + 编号连续 + 每条都有「已改成什么」 ------
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
        # ⚠️ **必须逐条判，不能数全局次数**（实测出来的坑，见 STAGE-26 §8 订正 8）：
        #    §8 开头那句提示语里就有一个「**已改成什么**」⇒ ⇒ **全局计数**会把它
        #    当成某一条订正的「已改成」⇒ ⇒ **删掉某一条真的「已改成」也可能不红**
        #    ⇒ ⇒ **零判别力** ⇒ ⇒ 判据落在「**每一条订正自己的那段里**」。
        missing_changed = []
        for idx, line in enumerate(sec8):
            m = re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", line)
            if not m:
                continue
            # 这一条订正的正文 = 从它的标题到**下一条订正标题**（或 §8 末尾）为止。
            body = []
            for follow in sec8[idx + 1 :]:
                if re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", follow):
                    break
                body.append(follow)
            if "已改成" not in "\n".join(body):
                missing_changed.append(m.group(1))
        if missing_changed:
            err(
                f"[订正] §8 的这些订正**自己那段里**没有「已改成」：{missing_changed}"
                " ⇒ ⇒ 逐条判（不是数全局次数）"
            )

# ---- 7e. 跨层一致性：§1.2 的 F1 / F3 两行里的地址必须与语料两处逐字相同 -
# ⚠️ **规格里 §1.2 的 F1 行（参考侧报的那个数）与 F3 行（平台报的那个数）**是
#    地址的**唯一来源** ⇒ ⇒ 地址**从规格里读**，⛔ **不写死**在本脚本里。
#: ⚠️ **RAM 窗口的基址**是 `0x8000_0000` ⇒ ⇒ 小于它的十六进制字面量是
#:    `pmpaddr` 的**编码值**（= 基址 >> 2），**不是地址** ⇒ ⇒ 不参与跨层比对。
RAM_BASE = 0x8000_0000
ADDR_RE = re.compile(r"0[xX][0-9a-fA-F_]+")
rows12 = table_rows(sec12)
source_rows = [r for r in rows12 if r and r[0] in ("F1", "F3")]
if not source_rows:
    err("[跨层] §1.2 的事实表里找不到 F1 行 / F3 行（形状变了？）")
required_addrs = []
for row in source_rows:
    line = "| " + " | ".join(row) + " |"
    for m in ADDR_RE.finditer(line):
        norm = m.group(0).lower().replace("_", "")
        try:
            val = int(norm, 16)
        except ValueError:
            continue
        if val >= RAM_BASE:
            required_addrs.append(norm)
required_addrs = sorted(set(required_addrs))
if not required_addrs:
    err("[跨层] §1.2 的 F1 / F3 两行里抽不出**地址字面量**"
        "（占位未填？或事实表的形状变了？）")

#: ⚠️ **两处语料**（语料源码 + golden README）⇒ ⇒ 缺一不可。
#:    ⚠️ **不进 `corpus.txt`** —— 那是**登记清单**，不承载地址字面量。
CROSS_LAYER_FILES = [
    "tests/golden/local/pmp_straddle.S",
    "tests/golden/README.md",
]


def _norm_addr(s: str) -> str:
    return s.replace("_", "").lower()


_repo_root = Path(__file__).resolve().parent.parent
_spec_abs = spec if spec.is_absolute() else (Path.cwd() / spec)
_cross_targets = [(_spec_abs, "规格本身")] + [
    (_repo_root / rel, rel) for rel in CROSS_LAYER_FILES]
for _p, _label in _cross_targets:
    if not _p.exists():
        # ⚠️ **文件没了 ⇒ ⇒ 跨层检查静默失效** ⇒ ⇒ 与「找不到候选清单块」同形
        err(f"[跨层] 找不到 {_label}（{_p}）⇒ ⇒ 跨层一致性检查**静默失效** ⇒ ⇒ 必须红")
        continue
    _body = _norm_addr(_p.read_text(encoding="utf-8"))
    for _lit in required_addrs:
        if _lit not in _body:
            err(f"[跨层] {_label} 缺少 §1.2 的 F1 / F3 里的地址字面量 {_lit}")

print(f"检查项：7（+ 7b/7c/7d/7e）  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
