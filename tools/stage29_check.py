#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-29 规格自检（七项 + 七个子项，与 stage28_check.py 同构）。

用法：python3 tools/stage29_check.py docs/detailed-design/STAGE-29_V044_Contract_V8_Admin_Carrier_Code_Landing.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（A-1~A-8 / B-1~B-6 / C-1~C-8 / T-1~T-4 / X-1~X-4）
    7. 关键约束字面量钉住（含已废止字面量的禁列）
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F16 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.1 的 **A-1 行是唯一权威处**）
       ＋ 7e. 跨层一致性：§1.2 的 **F 表逐行**核对该行点名的**文件**与**标识符**
                （原文从规格里读 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红）
       ＋ 7f. **D-129 的落点必须在位**（契约层两个 DTO + 两个事件 + 一个码常量 +
                `SCHEMA_VERSION` = 8 / 宿主 `admin_enabled` 开关 / 前端两个 topic +
                两张码表）
       ＋ 7g. **场景码落位的落点必须在位**（`error_families!` 族数仍 15 /
                `E_CTX_05_ADMIN_CHANNEL_DISABLED` = 5 / `E_CTX-05` **只在**镜像表里 /
                11 个前端自有码**都在**自有表里）
       ＋ 7h. **两条裁决的字面量**（「不新增错误族」与「只搬 `E_CTX-05`」）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7c / 7f / 7g 都是「逐条判」、不是「数全局次数」** —— 这正是 **STAGE-26 §8 订正 8** 的教训：
    上一批的 7c 数的是 §8 整节的「已改成」**出现次数** ⇒ ⇒ §8 开头那句散文里的
    「已改成什么」**顶替**了某一条订正 ⇒ ⇒ 删掉一条真的「已改成」也**不红**
    ⇒ ⇒ **零判别力** ⇒ ⇒ 本脚本**不许**重犯同一个错。

⚠️ **7e 的口径是「标识符级」而不是「结论级」** —— §1.2 的 F 表记录的是「**冻结时刻**」
    的实测状态 ⇒ ⇒ 本批**会改掉其中几条的「结论」**（F3 / F12 / F16）⇒ ⇒ 但**「落点」
    （文件与标识符）不变** ⇒ ⇒ 7e 钉的是**落点** ⇒ ⇒ 结论的变化记在 **§8**。
⚠️ **7e 读的是 §1.2 表格的第 3 格（落点列）** —— 第 2 格是**散文**（「事实」），
    散文里的反引号**不参与**判定（否则散文里提到的旧字面量会造成假红）。

⚠️ **7d 与 stage28 的 7d 口径不同**：stage28 那批**不升版** ⇒ ⇒ 判「紧跟的数字 == 原值」；
    **本批要升 `SCHEMA_VERSION`（7 → 8）** ⇒ ⇒ 若还用「紧跟的数字」口径，§2.4 里那句
    「`SCHEMA_VERSION` **7 → 8**」会被读成「值是 7」⇒ ⇒ **假红** ⇒ ⇒ 因此本项改成
    **只判 §5.1 的 A-1 行**（那里是**唯一**把四个版本号都写出来的地方）⇒ ⇒
    一个数**只服务一个口径** ⇒ ⇒ 判据 = 「A-1 行里，每个常量名紧跟的那个数字 == 期望值」。

⚠️ **7f / 7g 在本批施工完成之前是红的** ⇒ ⇒ 这是**诚实**的（本批的落点还没落地）⇒ ⇒
    **批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-29_V044_Contract_V8_Admin_Carrier_Code_Landing.md")
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
#:    * **F3**：前端码表化**可能**让 `TRANSPORT_CHANNEL_ERROR_CODE` /
#:      `HOST_ERROR_CODE` 这两个导出名消失 ⇒ ⇒ 允许台账留痕兜底。
#:    * **F7**：F7 的落点引的是**修前**的裸字面量形态
#:      （`throw sessionError("E_CTX-11", "会话不存在")`）⇒ ⇒ 而本批的裁决 E
#:      **要求**把那处裸字面量**删掉**（改走 `FRONT_ERROR_CODES`）⇒ ⇒
#:      ⇒ **两者不可兼得** ⇒ ⇒ 落点由**台账留痕**兜底（D-129 的销账条目里
#:      **逐字**记着那个修前形态）⇒ ⇒ 而**源文件**里**不许**再留一份
#:      「为了过检查而存在的注释」⇒ ⇒ 否则那个检查**永远绿着**⇒ ⇒ **零判别力**。
FALLBACK_LEDGER_ROWS = {"F3", "F7"}


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


def _body_lines():
    """规格的**正文**（**去掉 §9 变异自检那一节**）。

    ⚠️ **为什么必须去掉 §9**：§9 的变异表**逐字引用**变异内容 ⇒ ⇒ 里面
    **故意**出现「不存在的验收编号」与「故意写错的版本号」⇒ ⇒ 若不排除 ⇒ ⇒
    **§9 自己把门禁弄红** ⇒ ⇒ ⇒ ⇒ **零判别力** ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒
    ⇒ ⇒ **判据**：§9 记的是「**做过什么**」，**不是**「规格现在说什么」⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ 排除它。
    """
    out = []
    in_s9 = False
    for line in lines:
        if re.match(r"^##\s+§9\b", line):
            in_s9 = True
            continue
        if in_s9:
            if re.match(r"^##\s+§\d+", line):
                in_s9 = False
            else:
                continue
        out.append(line)
    return out


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
    "A": [f"A-{i}" for i in range(1, 9)],   # A-1~A-8
    "B": [f"B-{i}" for i in range(1, 7)],   # B-1~B-6
    "C": [f"C-{i}" for i in range(1, 9)],   # C-1~C-8
    "T": [f"T-{i}" for i in range(1, 5)],   # T-1~T-4
    "X": [f"X-{i}" for i in range(1, 5)],   # X-1~X-4
}
for grp, want in ACCEPT_EXPECT.items():
    got = sorted((x for x in defined if x.startswith(grp + "-")),
                 key=lambda x: int(x.split("-")[1]))
    if got != want:
        err(f"[验收] {grp} 组编号不是 {want[0]}~{want[-1]} 连续：{got}")
for sub in sorted(sub_ids):
    if sub not in defined:
        err(f"[验收] 子编号 {sub}b 的**母号** {sub} 不在 §5 的验收里")
# ⚠️⚠️ **§9 的变异表必须排除在这条之外** —— 它**逐字引用**变异内容
#    ⇒ ⇒ 若不排除 ⇒ ⇒ §9 自己会把门禁弄红 ⇒ ⇒ ⇒ ⇒ ⇒ ⇒ **零判别力**
body_lines = _body_lines()
for m in re.finditer(r"\b([ABCTX]-\d+)\b", "\n".join(body_lines)):
    if m.group(1) not in defined:
        line_no = "\n".join(body_lines)[:m.start()].count("\n") + 1
        err(f"[验收] 第 {line_no} 行出现未在 §5 定义的验收编号 {m.group(1)}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    # ---- 契约层落点（D-129）----
    "crates/contract/src/protocol.rs",
    "crates/contract/src/error.rs",
    "crates/contract/src/numstr.rs",
    "LogEntryDto",
    "AdminDiagDto",
    "AdminLogs",
    "AdminDiag",
    "E_CTX_05_ADMIN_CHANNEL_DISABLED",
    "u64_decimal",
    # ---- 内核落点 ----
    "crates/kernel/src/kernel_host",
    "crates/kernel/src/m21_logging",
    "crates/kernel/src/m22_hms",
    "admin_enabled",
    "DiagnosticKit",
    "LogEntry",
    # ---- 前端与工具落点 ----
    "front/lib/message.js",
    "front/services/m05_commandrouter.js",
    "front/services/m23_boardstore.js",
    "tools/error_code_check.py",
    "FRONT_ERROR_CODES",
    "MIRRORED_ERROR_CODES",
    "admin.logs",
    "admin.diag",
    # ---- 台账与批次 ----
    "D-129",
    "D-132",
    "STAGE-29",
    "STAGE-28",
    # ---- 纪律 ----
    "一个约束一个落点",
    "加码不加族",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    不是「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ⚠️ **「不升版」这条**在本批**不成立**（本批**要**升 `SCHEMA_VERSION`）⇒ ⇒
#    禁列必须**只**盯住**不该升**的那三个 ⇒ ⇒ 否则 §0.2 的散文会假红。
for line in lines:
    if not line.startswith("#"):
        continue
    if "升版" in line and "不" not in line and "SIM_STATE_SCHEMA_VERSION" in line:
        err(f"[禁列] 结论性标题里出现「升版」+ `SIM_STATE_SCHEMA_VERSION`"
            f"（本批**不**升那一个）：{line.strip()}")

# ---- 7b. §1.2 的事实表编号列必须是 F1~F16 连续 ---------------------------
sec12 = section(r"^###\s+§1\.2(?!\.1)\b")
rows12 = table_rows(sec12)
facts = []
for row in rows12:
    if not row:
        continue
    m = re.fullmatch(r"F(\d+)", row[0])
    if m:
        facts.append("F" + m.group(1))
want_facts = [f"F{i}" for i in range(1, 17)]
if facts != want_facts:
    err(f"[事实] §1.2 的实测事实表**编号列**不是 F1~F16 连续（无缺号/无重复/无越界）：{facts}")

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
        missing_changed = []
        for idx, line in enumerate(sec8):
            m = re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", line)
            if not m:
                continue
            body_lines_of = []
            for follow in sec8[idx + 1:]:
                if re.match(r"^#{2,4}\s*.*?订正\s*(\d+)", follow):
                    break
                body_lines_of.append(follow)
            if "已改成" not in "\n".join(body_lines_of):
                missing_changed.append(m.group(1))
        if missing_changed:
            err(
                f"[订正] §8 的这些订正**自己那段里**没有「已改成」：{missing_changed}"
                " ⇒ ⇒ 逐条判（不是数全局次数）"
            )

# ---- 7d. **四个版本号的值**（§5.1 的 A-1 行是唯一权威处） ----------------
# ⚠️ **本批要升 `SCHEMA_VERSION`（7 → 8）** ⇒ ⇒ 「紧跟的数字」口径在 §2.4 的
#    「`SCHEMA_VERSION` **7 → 8**」那句话上会**读成 7** ⇒ ⇒ 假红 ⇒ ⇒
#    因此本项**只判 §5.1 的 A-1 行**（那里是**唯一**把四个版本号都写出来的地方）。
VERSION_PINS = [
    ("SCHEMA_VERSION", "8"),
    ("SIM_STATE_SCHEMA_VERSION", "2"),
    ("PLATFORM_SCHEMA_VERSION", "1"),
    ("PLATFORM_RULES_VERSION", "3"),
]
a1_row = None
for row in table_rows(sec5):
    if row and re.fullmatch(r"A-1", row[0]):
        a1_row = " | ".join(row)
        break
if a1_row is None:
    err("[禁列] §5 里找不到 **A-1** 行 ⇒ ⇒ 7d 的权威处**静默失效** ⇒ ⇒ 必须红")
else:
    for const, want in VERSION_PINS:
        # ⚠️ `SCHEMA_VERSION` 是 `SIM_STATE_SCHEMA_VERSION` 与
        #    `PLATFORM_SCHEMA_VERSION` 的**子串** ⇒ ⇒ 必须排除被字母 / 数字 /
        #    下划线包住的那次命中 ⇒ ⇒ 否则会拿别的常量的值来判。
        pat = re.compile(r"(?<![A-Za-z0-9_])" + re.escape(const) + r"(?![A-Za-z0-9_])")
        m = pat.search(a1_row)
        if m is None:
            err(f"[禁列] §5 的 A-1 行没有写出 {const} 的值"
                f"（本批必须是 {want}）⇒ ⇒ 本项会恒绿 ⇒ ⇒ 必须红")
            continue
        win = a1_row[m.end():m.end() + 24]
        dm = re.search(r"(?<![A-Za-z0-9_-])(\d)", win)
        if dm is None:
            err(f"[禁列] §5 的 A-1 行没有把 {const} 的值写出来"
                f"（本批必须是 {want}）⇒ ⇒ 本项会恒绿 ⇒ ⇒ 必须红")
        elif dm.group(1) != want:
            err(f"[禁列] §5 的 A-1 行把 {const} 的值写成 {dm.group(1)}"
                f"（本批必须是 {want}）")

# ---- 7e. 跨层一致性：§1.2 的 F 表**逐行**核对文件与标识符 ----------------
# ⚠️ **原文与路径都从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红。
#: ⚠️ **口径是「标识符级」而不是「结论级」**（见文件头的说明）⇒ ⇒ 本批**改掉
#:    F3 / F12 / F16 的「结论」**不红（它们改的是**结论**、**落点不变**）。
#: ⚠️ **读的是第 3 格（落点列）** —— 第 2 格是**散文**，不参与判定。
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
    # ⚠️ **落点那一格**（第 3 格）⇒ ⇒ 第 2 格是散文，不参与。
    if len(row) < 3:
        err(f"[跨层] §1.2 的 {fid} 行只有 {len(row)} 格 ⇒ ⇒ 抽不出「落点」那一格")
        continue
    where = row[2]
    toks = [t for t in re.findall(r"`([^`]+)`", where)
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
                continue
        err(f"[跨层] {fid} 引用的标识符 {tok} 在它点名的源文件里**逐字找不到**"
            f"（台账兜底也不在位）⇒ ⇒ 规格与实现漂移了")

# ---- 7f. D-129 的落点必须在位 --------------------------------------------
_CONTRACT_DIR = REPO_ROOT / "crates/contract/src"
_HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
_FRONT_MSG_REL = "front/lib/message.js"

if not _CONTRACT_DIR.is_dir():
    err("[落地] 找不到 crates/contract/src ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
    contract_rs = []
else:
    contract_rs = sorted(_CONTRACT_DIR.glob("*.rs"))

# ---- 7f-① 契约层：两个 DTO + 两个事件 + 一个码常量 + `SCHEMA_VERSION` = 8 ----
def _find_in_rs(needle):
    hits = []
    for path in contract_rs:
        try:
            txt = path.read_text(encoding="utf-8")
        except OSError:
            continue
        if needle in txt:
            hits.append(path)
    return hits


for dto in ("LogEntryDto", "AdminDiagDto"):
    if not _find_in_rs(dto):
        err(f"[落地] 契约层没有 `{dto}` ⇒ ⇒ D-129 的落点不在位")

_protocol = _CONTRACT_DIR / "protocol.rs"
if not _protocol.exists():
    err("[落地] 找不到 crates/contract/src/protocol.rs ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
else:
    ptxt = _protocol.read_text(encoding="utf-8")
    for ev in ("AdminLogs", "AdminDiag"):
        if not re.search(rf"\b{ev}\b", ptxt):
            err(f"[落地] 契约层的事件里没有 `{ev}` ⇒ ⇒ D-129 的落点不在位")
    # ⚠️ **`AdminOp` 不许加新变体**（裁决 H）⇒ ⇒ 变体数必须仍是 4。
    am = re.search(r"pub enum AdminOp\s*\{(.*?)\n\}", ptxt, re.S)
    if am is None:
        err("[落地] 契约层的 `AdminOp` 定义没找到 ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
    else:
        variants = re.findall(r"(?m)^\s{4}([A-Z][A-Za-z]*)\s*(?:\(|,|\{)", am.group(1))
        variants = [v for v in variants if v != "Custom"]
        if len(variants) != 3:
            err(f"[落地] `AdminOp` 的变体数是 {len(variants)}（期望 3 个既有 + Custom）"
                f"：{variants} ⇒ ⇒ 裁决 H 说「不加新变体」")

_error_rs = _CONTRACT_DIR / "error.rs"
if not _error_rs.exists():
    err("[落地] 找不到 crates/contract/src/error.rs ⇒ ⇒ 7f 检查**静默失效**⇒ ⇒ 必须红")
else:
    etxt = _error_rs.read_text(encoding="utf-8")
    cm = re.search(r"pub const E_CTX_05_ADMIN_CHANNEL_DISABLED:\s*u16\s*=\s*(\d+)\s*;", etxt)
    if cm is None:
        err("[落地] 契约层没有 `E_CTX_05_ADMIN_CHANNEL_DISABLED` ⇒ ⇒ 裁决 C 的落点不在位")
    elif cm.group(1) != "5":
        err(f"[落地] `E_CTX_05_ADMIN_CHANNEL_DISABLED` 的值是 {cm.group(1)}，"
            f"而码 `E_CTX_05` 的场景号是 05")

_lib_rs = _CONTRACT_DIR / "lib.rs"
if not _lib_rs.exists():
    err("[落地] 找不到 crates/contract/src/lib.rs ⇒ ⇒ 7f 检查**静默失效**⇒ ⇒ 必须红")
else:
    ltxt = _lib_rs.read_text(encoding="utf-8")
    sm = re.search(r"pub const SCHEMA_VERSION:\s*u32\s*=\s*(\d+)\s*;", ltxt)
    if sm is None:
        err("[落地] 契约层的 `SCHEMA_VERSION` 定义没找到 ⇒ ⇒ 7f 检查**静默失效**⇒ ⇒ 必须红")
    elif sm.group(1) != "8":
        err(f"[落地] 契约层的 `SCHEMA_VERSION` 是 {sm.group(1)}，本批必须是 8")

# ---- 7f-② 内核：宿主有 `admin_enabled` 开关 ------------------------------
_host = REPO_ROOT / _HOST_REL
if not _host.exists():
    err(f"[落地] 找不到 {_HOST_REL} ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
else:
    htxt = _host.read_text(encoding="utf-8")
    if not re.search(r"\badmin_enabled\b", htxt):
        err("[落地] 宿主没有 `admin_enabled` 开关 ⇒ ⇒ 裁决 G 的落点不在位")

# ---- 7f-③ 前端：两个 topic + 两张码表 + `SCHEMA_VERSION` = 8 -------------
_front = REPO_ROOT / _FRONT_MSG_REL
if not _front.exists():
    err(f"[落地] 找不到 {_FRONT_MSG_REL} ⇒ ⇒ 7f 检查**静默失效** ⇒ ⇒ 必须红")
else:
    ftxt = _front.read_text(encoding="utf-8")
    for topic in ("admin.logs", "admin.diag"):
        if topic not in ftxt:
            err(f"[落地] 前端镜像没有 topic `{topic}` ⇒ ⇒ D-129 的落点不在位")
    for tbl in ("FRONT_ERROR_CODES", "MIRRORED_ERROR_CODES"):
        if not re.search(rf"\b{tbl}\b", ftxt):
            err(f"[落地] 前端镜像没有 `{tbl}` ⇒ ⇒ 裁决 E 的落点不在位")
    fsm = re.search(r"export const SCHEMA_VERSION\s*=\s*(\d+)\s*;", ftxt)
    if fsm is None:
        err("[落地] 前端镜像的 `SCHEMA_VERSION` 定义没找到 ⇒ ⇒ 7f 检查**静默失效**⇒ ⇒ 必须红")
    elif fsm.group(1) != "8":
        err(f"[落地] 前端镜像的 `SCHEMA_VERSION` 是 {fsm.group(1)}，本批必须是 8")

# ---- 7g. 场景码落位的落点必须在位 ----------------------------------------
# ---- 7g-① `error_families!` 的族数必须仍是 15（**加码不加族**）------------
if _error_rs.exists():
    # ⚠️ **只数 `error_families!` 的「调用块」里的族** —— `error.rs` 里还有**模块级
    #    文档**里的一段**示例**（`/// error_families! { /// doc Context => "CTX", }`）
    #    ⇒ ⇒ 若扫全文 ⇒ ⇒ **示例里的那个族会被算进去** ⇒ ⇒ 假红 ⇒ ⇒ ⇒ ⇒
    #    ⇒ ⇒ **判据**：族数 = **调用块**（`^error_families! {` … `^}`）里的族数。
    fam_blk = re.search(r"(?m)^error_families!\s*\{(.*?)^\}", etxt, re.S)
    if fam_blk is None:
        err("[落地] `error_families!` 的**调用块**没找到 ⇒ ⇒ 7g-① 检查**静默失效**⇒ ⇒ 必须红")
        fams = []
    else:
        fams = re.findall(r"=>\s*\"([A-Z]{3,5})\"", fam_blk.group(1))
    if len(fams) != 15:
        err(f"[落地] `error_families!` 的族数是 {len(fams)}（本批**不新增族** ⇒ ⇒ 必须是 15）"
            f"：{fams}")
    for tag in ("CTX", "TRX", "STORE"):
        if tag not in fams:
            err(f"[落地] `error_families!` 里没有 `{tag}` 族 ⇒ ⇒ 族被删了")

# ---- 7g-② 前端两张码表的内容对不对 ---------------------------------------
FRONT_OWN_CODES = [
    "E_CTX-01", "E_CTX-06", "E_CTX-11",
    "E_TRX-01", "E_TRX-02", "E_TRX-03", "E_TRX-04", "E_TRX-05",
    "E_STORE-01", "E_STORE-02", "E_STORE-03",
]
if _front.exists():
    def _table_block(name):
        m = re.search(rf"\b{name}\b\s*=\s*Object\.freeze\(\{{(.*?)\}}\)", ftxt, re.S)
        return m.group(1) if m else None

    own = _table_block("FRONT_ERROR_CODES")
    if own is None:
        err("[落地] 前端镜像的 `FRONT_ERROR_CODES` 表体没找到 ⇒ ⇒ 7g-② 检查**静默失效**⇒ ⇒ 必须红")
    else:
        for code in FRONT_OWN_CODES:
            if code not in own:
                err(f"[落地] `FRONT_ERROR_CODES` 里没有前端自有码 `{code}`"
                    f" ⇒ ⇒ 裁决 E 的落点不在位")
        # ⚠️ **`E_CTX-05` 是契约层的码**（裁决 C）⇒ ⇒ **不许**在前端自有表里再定义一遍
        #    ⇒ ⇒ 两处定义 ⇒ ⇒ 「唯一定义」判红。
        if "E_CTX-05" in own:
            err("[落地] `FRONT_ERROR_CODES` 里**也**定义了 `E_CTX-05`"
                " ⇒ ⇒ 它是契约层的码（裁决 C）⇒ ⇒ 两处定义 ⇒ ⇒ 「唯一定义」被破了")

    mir = _table_block("MIRRORED_ERROR_CODES")
    if mir is None:
        err("[落地] 前端镜像的 `MIRRORED_ERROR_CODES` 表体没找到 ⇒ ⇒ 7g-② 检查**静默失效**⇒ ⇒ 必须红")
    elif "E_CTX-05" not in mir:
        err("[落地] `MIRRORED_ERROR_CODES` 里没有 `E_CTX-05`"
            " ⇒ ⇒ 裁决 C 的落点不在位")

# ---- 7h. 两条裁决的字面量必须写在 §7 里 -----------------------------------
sec7 = "\n".join(section(r"^##\s+§7\b"))
for kw in ("不新增错误族", "只搬"):
    if kw not in sec7:
        err(f"[钉住] §7 没有写清「{kw}」这条口径")
if "E_CTX-05" not in sec7:
    err("[钉住] §7 没有写清「只搬 `E_CTX-05`」⇒ ⇒ 裁决 C 的字面量缺失")

print(f"检查项：7（+ 7b/7c/7d/7e/7f/7g/7h）  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
