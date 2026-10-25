#!/usr/bin/env python3
"""STAGE-25 规格自检（七项，与 stage23_check.py 同构）。

用法：python3 tools/stage25_check.py docs/detailed-design/STAGE-25_V040_PMP_Privilege.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（A-1~A-5 / B-1~B-6 / C-1~C-5 / T-1~T-4 / X-1~X-3）
    7. 关键约束字面量钉住（含已废止字面量的禁列）＋ **五格判别式**（A~E）
       ＋ 7b. §1.2 的实测事实表**编号列**必须是 **F1~F13 连续**（无缺号 / 无重复 / 无越界）
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + 每条都有「已改成什么」
       ＋ 7d. 三个版本号的值必须还是原值（本批**不升**）
       ＋ 7e. 跨层一致性：§1.2.1 **配置表**里的地址必须与语料三处逐字相同

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **本批的三个版本都不升**（裁决 A）⇒ ⇒ 第 7d 项按「**找到常量名 ⇒ 看紧跟其后的
那个数字**」判定 ⇒ ⇒ ⛔ **不要**拿「`SCHEMA_VERSION = 8`」这种**规格里根本不存在的
形态**当禁列 ⇒ ⇒ 那是**零判别力的死护栏**（台账上一批刚踩过）。

⚠️ **`pmp_access_allowed` 的第五个参数变了**（`m_mode_bypass: bool` → `privilege: u32`）
⇒ ⇒ 禁列钉的是**函数签名里的参数声明** ⇒ ⇒ 叙述里写「原来是 `m_mode_bypass: bool`」
是**允许**的 ⇒ ⇒ 禁列必须落在 `fn ...(...)` 的参数表上，不能落在全文关键词上。

⚠️ **「整段覆盖」只许出现在「被推翻的旧结论」里**（§1.3 / §8）⇒ ⇒ 禁列落在
**结论性标题**上，不是全文关键词 ⇒ ⇒ 否则「被否掉的旧说法」也会被误判成「结论」。

⚠️ **7b 只看事实表的「编号」那一列** ⇒ ⇒ 表里还有一列「**探针原号**」（F1~F12、F14、
F15 ⇒ ⇒ **不连续**）⇒ ⇒ ⛔ **不许**去查那一列 ⇒ ⇒ 否则会误报。

⚠️ **7e 的地址来源**：§1.2.1 的**配置表**是地址的**唯一来源** ⇒ ⇒ 地址**从规格里读**，
⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了地址，三处语料必须跟着改。
⚠️ **那三个文件此刻可能还不存在**（另一条 lane 正在写）⇒ ⇒ **文件不存在就报红**
（检查静默失效 == 门禁恒绿）。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-25_V040_PMP_Privilege.md")
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
# ⚠️ **§5 的编号写在**列表项**里（不是表格行）⇒ ⇒ 定义式要同时吃列表项与表格行。
#: ⚠️ **子编号**（`A-2b` / `C-4b` 那种）**不进**连续性判定 ⇒ ⇒ 只取「字母-数字」
#:    那种**完整**的号 ⇒ ⇒ 子号另有一条判定（它的母号必须在）。
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
    "A": [f"A-{i}" for i in range(1, 6)],    # A-1~A-5
    "B": [f"B-{i}" for i in range(1, 7)],    # B-1~B-6
    "C": [f"C-{i}" for i in range(1, 6)],    # C-1~C-5
    "T": [f"T-{i}" for i in range(1, 5)],    # T-1~T-4
    "X": [f"X-{i}" for i in range(1, 4)],    # X-1~X-3
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
    # ---- 契约层落点（裁决 B / C）----
    "pmp_effective_privilege",
    "pmp_exempt",
    "pmp_access_allowed",
    "crates/contract/src/csr.rs",
    # ---- 内核落点（裁决 G / H）----
    "crates/kernel/src/m13_iss/pmp.rs",
    "pmp_allows",
    "CSR_PRIV",
    "0x0C0",
    "PRIV_M",
    "PRIV_S",
    "PmpAccess::Fetch",
    "mcause",
    "mstatus",
    # ---- 公式的关键片段（**钉结论**，词形可以变）----
    "有效特权级",
    "当前特权级",
    "MPRV",
    "MPP",
    "取指",
    "取指永远用当前特权级",
    "有效特权级 == M",
    # ---- 三个版本号**不升**这件事要说清（裁决 A）----
    "不升",
    # ---- 语料与登记（裁决 I / K）----
    "local/pmp_privilege.S",
    "tests/golden/corpus.txt",
    "tests/golden/README.md",
    # ---- 台账与批次 ----
    "D-153",
    "D-154",
    "STAGE-25",
    "STAGE-24",
    "STAGE-23",
    # ---- 纪律 ----
    "一个约束一个落点",
    "逐片",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    **不是**「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ⚠️ **新参数名 `privilege`** ⇒ ⇒ 「`pmp_access_allowed` 的参数变成 `privilege`」
#    这件事必须在规格里说清 ⇒ ⇒ 判据落在**提到那个函数的那一行**上。
_ap_lines = [l for l in lines if "pmp_access_allowed" in l]
if not any("privilege" in l for l in _ap_lines):
    err("[钉住] 规格没有说清 `pmp_access_allowed` 的**新参数名** `privilege`")


def _near(a, b, window=80):
    """a 与 b 在**同一段**里相距不超过 window ⇒ ⇒ 算「挨着」。"""
    pat = (re.escape(a) + r".{0,%d}?" % window + re.escape(b)
           + r"|" + re.escape(b) + r".{0,%d}?" % window + re.escape(a))
    return re.search(pat, text, re.S) is not None


# ⚠️ **位号钉住**：MPRV = bit 17、MPP = bits 12:11 ⇒ ⇒ 挨着写才作数。
if not _near("MPRV", "17"):
    err("[钉住] 规格没有把 `MPRV` 的位号（**bit 17**）写清")
if not _near("MPP", "12:11"):
    err("[钉住] 规格没有把 `MPP` 的位号（**bits 12:11**）写清")

# ---- 禁列（已废止 / 不许出现的形态） -------------------------------------
# ⚠️ **`m_mode_bypass` 只许出现在叙述里** ⇒ ⇒ 禁的是**函数签名里的参数声明**。
_fn_sig = re.compile(r"fn\s+[A-Za-z_][A-Za-z0-9_]*\s*\([^()]*m_mode_bypass\s*:\s*bool",
                     re.S)
if _fn_sig.search(text):
    err("[禁列] 函数签名里仍把 `m_mode_bypass: bool` 当参数（旧签名形态）")
# ⚠️ **`pmp_access_allowed` 的旧第五参数**出现在**它的签名**里 ⇒ ⇒ 同上。
_ap_sig = re.compile(r"pmp_access_allowed\s*\([^()]*m_mode_bypass", re.S)
if _ap_sig.search(text):
    err("[禁列] `pmp_access_allowed` 的签名里仍挂着旧参数 `m_mode_bypass`")

# ⚠️ **「整段覆盖」不许当结论** ⇒ ⇒ 判据落在**结论性标题**上 ⇒ ⇒
#    §1.3（被推翻的旧结论）与 §8（施工期订正）**豁免** ⇒ ⇒ 那两处允许引用它。
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
    if "整段覆盖" in line:
        err(f"[禁列] 结论性标题里出现「整段覆盖」（旧读法只许出现在 §1.3 / §8）：{line.strip()}")

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
    for m in pat.finditer(text):
        tail = text[m.end():]
        # 遇到**另一个**版本常量名就停 ⇒ ⇒ 那不是本常量的值。
        stop = len(tail)
        for other, _ in VERSION_PINS:
            if other == const:
                continue
            om = re.search(r"(?<![A-Za-z0-9_])" + re.escape(other)
                           + r"(?![A-Za-z0-9_])", tail)
            if om and om.start() < stop:
                stop = om.start()
        seg = tail[:stop]
        dm = re.search(r"\d", seg)
        if dm and dm.group(0) != want:
            err(f"[禁列] 规格把 {const} 的值写成 {dm.group(0)}"
                f"（本批**不升** ⇒ 必须是 {want}）")

# ---- 7b. §1.2 的实测事实表：编号列必须**恰好是 F1~F13** ------------------
# ⚠️ **删掉一条实测事实**也是「把规格改软」的一种 ⇒ ⇒ 事实表本身要钉住。
#: ⚠️ **只看「编号」那一列** ⇒ ⇒ 表里还有一列「探针原号」（F1~F12、F14、F15
#:    ⇒ ⇒ **不连续**）⇒ ⇒ ⛔ 那一列**不许**参与判定。
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
# ⚠️ **施工期订正是规格被实测推翻的账** ⇒ ⇒ 删掉一条 = 把已被推翻的判据
#    悄悄留回规格里 ⇒ ⇒ 必须钉住。
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
        n_changed = sec8_text.count("已改成")
        if n_changed < len(corrections):
            err(f"[订正] §8 的「已改成」不足 {len(corrections)} 条（实测 {n_changed}）")

# ---- 五格判别式（§4.1 的表）：A~E 五行都要在，且各自的取值都要在 ---------
# ⚠️ **一格少一行**就是「把判别力改软」⇒ ⇒ 五格必须**成一条程序**（裁决 I）。
sec41 = section(r"^###\s+§4\.1\b")
cells = {}
for row in table_rows(sec41):
    if row and re.fullmatch(r"[A-Z]", row[0]):
        cells[row[0]] = row
WANT_CELLS = ["A", "B", "C", "D", "E"]
if sorted(cells) != WANT_CELLS:
    err(f"[五格] §4.1 的判别式不是 A~E 五格：{sorted(cells)}")
#: ⚠️ **每一格的期望**是钉死的 ⇒ ⇒ 把「放行」写成「拒绝」也会红。
CELL_EXPECT = {"A": "放行", "B": "拒绝", "C": "放行", "D": "拒绝", "E": "拒绝"}
for label in WANT_CELLS:
    row = cells.get(label)
    if not row:
        continue
    if len(row) < 6:
        err(f"[五格] §4.1 的格 {label} 只有 {len(row)} 列（应 ≥ 6：格 / 特权级 / MPRV / MPP / 访问 / 期望）")
        continue
    if not re.search(r"\b(M|S|U)\b", row[1]):
        err(f"[五格] §4.1 的格 {label} 没有**当前特权级**的取值（M / S / U）：{row[1]}")
    if not re.search(r"(0|1|任意)", row[2]):
        err(f"[五格] §4.1 的格 {label} 没有 `MPRV` 的取值：{row[2]}")
    if not re.search(r"(任意|[0-3])", row[3]):
        err(f"[五格] §4.1 的格 {label} 没有 `MPP` 的取值：{row[3]}")
    if not re.search(r"(load|store|fetch)", row[4], re.I):
        err(f"[五格] §4.1 的格 {label} 没有**访问类型**（load / store / fetch）：{row[4]}")
    if not row[5].startswith(CELL_EXPECT[label]):
        err(f"[五格] §4.1 的格 {label} 的期望不是「{CELL_EXPECT[label]}」：{row[5]}")
# ⚠️ **格 E 是取指那一格** ⇒ ⇒ 判据要能认出它。
fetch_cells = [k for k, r in cells.items() if len(r) > 4 and "fetch" in r[4].lower()]
if not fetch_cells:
    err("[五格] §4.1 里没有**取指**（fetch）那一格")
elif "E" in cells and "E" not in fetch_cells:
    err(f"[五格] §4.1 的格 E 不是**取指**那一格（取指在 {fetch_cells}）")

# ---- 7e. 跨层一致性：§1.2.1 配置表里的地址必须与语料三处逐字相同 -------
# ⚠️ **规格里 §1.2.1 的配置表是地址的唯一来源** ⇒ ⇒ 地址**从规格里读**，
#    ⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了地址，三处语料必须跟着改。
#: ⚠️ **RAM 窗口的基址**是 `0x8000_0000` ⇒ ⇒ 小于它的十六进制字面量是
#:    `pmpaddr` 的**编码值**（= 基址 >> 2），**不是地址** ⇒ ⇒ 不参与跨层比对。
RAM_BASE = 0x8000_0000
ADDR_RE = re.compile(r"0[xX][0-9a-fA-F_]+")
#: ⚠️ **区间写法** `[0x80040000, 0x80041000)` 的**第二个**字面量是**区间上界**
#:    ⇒ ⇒ 它不是「条目基址」⇒ ⇒ 剔掉（否则会要求语料里出现一个用不着的字面量）。
RANGE_END_RE = re.compile(r"\[\s*`?0[xX][0-9a-fA-F_]+`?\s*,\s*`?(0[xX][0-9a-fA-F_]+)")
sec121 = section(r"^###\s+§1\.2\.1\b")
required_addrs = []
for row in table_rows(sec121):
    # ⚠️ **只取配置表里**那几行（表里另有「区间」「权限」两列 ⇒ ⇒ 一行一个条目）
    line = "| " + " | ".join(row) + " |"
    low = line.lower()
    if "deny" not in low and "allow" not in low:
        continue
    excluded = {m.group(1).lower().replace("_", "") for m in RANGE_END_RE.finditer(line)}
    for m in ADDR_RE.finditer(line):
        norm = m.group(0).lower().replace("_", "")
        if norm in excluded:
            continue
        try:
            val = int(norm, 16)
        except ValueError:
            continue
        if val >= RAM_BASE:
            required_addrs.append(norm)
required_addrs = sorted(set(required_addrs))
if not required_addrs:
    err("[跨层] §1.2.1 的配置表里抽不出**地址字面量**"
        "（占位未填？或配置表的形状变了？）")

#: ⚠️ **三处语料**（语料源码 + 语料登记 + golden README）⇒ ⇒ 缺一不可。
CROSS_LAYER_FILES = [
    "tests/golden/local/pmp_privilege.S",
    "tests/golden/corpus.txt",
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
            err(f"[跨层] {_label} 缺少 §1.2.1 配置表里的地址字面量 {_lit}")

# ---- 占位注释：§1.2 / §1.2.1 / §1.3 / §8 必须**已经被替换** --------------
PLACEHOLDER_TOKENS = ["FACTS_TABLE", "FACTS_CONFIG", "FACTS_FALSIFIED", "CORRECTIONS"]
for m in re.finditer(r"<!--(.*?)-->", text, re.S):
    _body = m.group(1)
    _tok = [t for t in PLACEHOLDER_TOKENS if t in _body]
    if not _tok and re.search(r"[A-Z][A-Z0-9]*_[A-Z0-9_]+", _body):
        _tok = [m2.group(0) for m2 in re.finditer(r"[A-Z][A-Z0-9]*_[A-Z0-9_]+", _body)]
    if _tok:
        _line_no = text[:m.start()].count("\n") + 1
        err(f"[占位] 第 {_line_no} 行还留着 HTML 注释占位：{_body.strip()}")

print(f"检查项：11（7 + 7b/7c/7d/7e + 五格 + 占位）  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
