#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-30 规格自检（七项 + 九个子项，与 stage29_check.py 同构）。

用法：python3 tools/stage30_check.py docs/detailed-design/STAGE-30_V045_Role_Gate_D131.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-5 / B-1~B-8 / C-1~C-5 / T-1~T-5 / X-1~X-4）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F15 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.1 的 **A-1 行是唯一权威处**）
       ＋ 7e. 跨层一致性：§1.2 的 **F 表逐行**核对该行点名的**文件**与**标识符**
       ＋ 7f. **D-131 的落点必须在位**（M-22 的 `set_role` / 宿主 `with_role` + `set_role`
                ⇒ ⇒ 且 **`admin_enabled: bool` 那个字段必须不在**）
       ＋ 7g. **D-130 的落点必须仍在位**（`DegradeExecutor` / `install_executor` /
                `fn degrade`）—— 本批**推后** D-130 ⇒ ⇒ 那三个落点**一个字不许动**
       ＋ 7h. **两条裁决的字面量**（逐条判，收窄到那一条裁决**自己那段**）
       ＋ 7i. **台账与规格的「真实未闭环」必须一致**（跨文件一致性）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7c / 7f / 7g / 7h 都是「逐条判」、不是「数全局次数」** —— 这正是 **STAGE-26 §8 订正 8** 的教训：
    数全局次数会被**开头那句散文**顶替 ⇒ ⇒ 删掉一条真的断言也**不红**
    ⇒ ⇒ **零判别力** ⇒ ⇒ 本脚本**不许**重犯同一个错。

⚠️ **7e 的口径是「标识符级」而不是「结论级」** —— §1.2 的 F 表记录的是「**冻结时刻**」
    的实测状态 ⇒ ⇒ 本批**会改掉其中几条的「结论」**（F5 / F6 / F8）⇒ ⇒ 但**「落点」**
    （文件与标识符）不变 ⇒ ⇒ 7e 钉的是**落点** ⇒ ⇒ 结论的变化记在 **§8**。

⚠️ **7f 在本批施工完成之前是红的** ⇒ ⇒ 这是**诚实**的（本批的落点还没落地）⇒ ⇒
    **批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-30_V045_Role_Gate_D131.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7e 的兜底落点 + 7i 的另一个来源）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **哪些行允许「台账兜底」** —— 判据是「**这条事实是修前的，修掉它就必然
#:    把它从源文件里删掉**」⇒ ⇒ 允许查「源文件**或** 台账」。
#:    * **F5**：宿主侧会从 `Hms::new()` 改成 `Hms::with_role(...)` ⇒ ⇒ `Hms::new()`
#:      这个**标识符**可能从宿主文件里消失 ⇒ ⇒ 允许台账留痕兜底。
#:    * **F6**：`admin_enabled: bool` 那个**字段**本批**要删掉** ⇒ ⇒ 它必然从宿主
#:      文件里消失 ⇒ ⇒ 允许台账留痕兜底（台账要**逐字**记着修前形态）。
#:    * **F8**：`with_admin_channel` 本批**要删掉** ⇒ ⇒ 同上。
FALLBACK_LEDGER_ROWS = {"F5", "F6", "F8"}

#: ⚠️ **本批的「真实未闭环」期望值**（7i 用）—— 本批**不销账** ⇒ ⇒ 不减。
EXPECT_OPEN_DEBT = "62"


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
    ⇒ ⇒ **判据**：§9 记的是「**做过什么**」，**不是**「规格现在说什么」⇒ ⇒ 排除它。
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


def _norm(s: str) -> str:
    """比对方去掉 Markdown 粗体标记与转义竖线。"""
    return s.replace("*", "").replace("\\|", "|").strip()


def _find_accept_line(code):
    """在 §5 的**所有行**（表格行 + 列表项）里找以某个验收编号开头的那一行。

    ⚠️ **不许只扫表格** —— §5 的形状可以是**列表** ⇒ ⇒ 只扫表格 ⇒ ⇒
    **抽不出那一行** ⇒ ⇒ 7d / 7i 的权威处**静默失效** ⇒ ⇒ 必须红 ⇒ ⇒
    ⇒ ⇒ 所以两种形状都认 ⇒ ⇒ **形状不许决定判据是否有效**。
    ⚠️ 返回的是**归一化**后的行（去粗体）⇒ ⇒ 数字提取不受 `**` 干扰。
    """
    pat = re.compile(
        r"^(?:[-*+]\s+|\d+[.)]\s+|\|\s*)?\**\s*" + re.escape(code) + r"\b")
    for line in sec5:
        s = line.strip()
        if pat.match(s):
            return _norm(s)
    return None


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

# ---- 5. 章节完整 §0~§10 --------------------------------------------------
heads = [l for l in lines if re.match(r"^##\s+§\d+", l)]
nums = sorted(int(re.search(r"§(\d+)", h).group(1)) for h in heads)
if nums != list(range(0, 11)):
    err(f"[章节] §0~§10 不完整，实得 {nums}")

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
    "A": [f"A-{i}" for i in range(1, 6)],   # A-1~A-5
    "B": [f"B-{i}" for i in range(1, 9)],   # B-1~B-8
    "C": [f"C-{i}" for i in range(1, 6)],   # C-1~C-5
    "T": [f"T-{i}" for i in range(1, 6)],   # T-1~T-5
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
body_lines = _body_lines()
for m in re.finditer(r"\b([ABCTX]-\d+)\b", "\n".join(body_lines)):
    if m.group(1) not in defined:
        line_no = "\n".join(body_lines)[:m.start()].count("\n") + 1
        err(f"[验收] 第 {line_no} 行出现未在 §5 定义的验收编号 {m.group(1)}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    # ---- 落点文件 ----
    "crates/kernel/src/m22_hms/mod.rs",
    "crates/kernel/src/kernel_host/mod.rs",
    "crates/kernel/src/kernel_host/tests_admin_channel.rs",
    "crates/contract/src/admin.rs",
    "crates/server/src/main.rs",
    # ---- 标识符 ----
    "set_role",
    "with_role",
    "Hms::set_role",
    "Role",
    "admin_enabled",
    "DegradeExecutor",
    "install_executor",
    "HOST_SESSION_ID",
    "LogEntryDto",
    "query_recent",
    # ---- 判据短语（T3 / C-2 / C-4 的字面量）----
    "搜不到任何日志文本",
    "恒等于 0",
    # ---- 台账与批次 ----
    "D-130",
    "D-131",
    "STAGE-30",
    # ---- 纪律 ----
    "一个约束一个落点",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    不是「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# ---- 7b. §1.2 的事实表编号列必须是 F1~F15 连续 ---------------------------
sec12 = section(r"^###\s+§1\.2(?!\.1)\b")
rows12 = table_rows(sec12)
facts = []
for row in rows12:
    if not row:
        continue
    m = re.fullmatch(r"F(\d+)", row[0])
    if m:
        facts.append("F" + m.group(1))
want_facts = [f"F{i}" for i in range(1, 16)]
if facts != want_facts:
    err(f"[事实] §1.2 的实测事实表**编号列**不是 F1~F15 连续（无缺号/无重复/无越界）：{facts}")

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
        # ⚠️ **必须逐条判，不能数全局次数**（STAGE-26 §8 订正 8 的教训）
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
# ⚠️ 本批**不升任何版本** ⇒ ⇒ 但 §0 / §3 的散文里也出现了版本号 ⇒ ⇒
#    「紧跟的数字」口径会被散文顶替 ⇒ ⇒ 因此本项**只判 §5.1 的 A-1 行**
#    （那里是**唯一**把四个版本号都写出来的地方）⇒ ⇒ 一个数**只服务一个口径**。
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
    # ⚠️ **不许只扫表格** —— §5 的形状可以是**列表** ⇒ ⇒ 只扫表格 ⇒ ⇒
    #    **抽不出那一行** ⇒ ⇒ 7d 的权威处**静默失效** ⇒ ⇒ 必须红 ⇒ ⇒
    #    ⇒ ⇒ 兜底：扫 §5 的**所有行**（表格行 + 列表项）⇒ ⇒ 两种形状都认。
    a1_row = _find_accept_line("A-1")
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
#: ⚠️ **口径是「标识符级」而不是「结论级」** ⇒ ⇒ 本批**改掉 F5 / F6 / F8 的
#:    「结论」**不红（它们改的是**结论**、**落点不变**或已被台账兜底）。
#: ⚠️ **读的是第 3 格（落点列）** —— 第 2 格是**散文**，不参与判定。
PATH_TOKEN_RE = re.compile(r"^[A-Za-z0-9_./-]+\.(?:rs|js|mjs|py|md|html|txt|sh)$")
#: ⚠️ **token 的合法性**：不许含箭头、不许含 Markdown 粗体标记、不许含省略号
TOKEN_BAD = ("⇒", "**", "…")

_ledger_text = None
for row in rows12:
    if not row:
        continue
    m = re.fullmatch(r"(F\d+)", row[0])
    if not m:
        continue
    fid = m.group(1)
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
        err(f"[跨层] §1.2 的 {fid} 行点名的标识符 {tok!r} 在源文件里找不到"
            f" ⇒ ⇒ 规格与代码漂移")

# ---- 7f. **D-131 的落点必须在位** ----------------------------------------
# ⚠️ 本批施工完成之前这一项是**红的** ⇒ ⇒ 那是**诚实**的 ⇒ ⇒ 批末必须绿。
M22_REL = "crates/kernel/src/m22_hms/mod.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
m22_text = (REPO_ROOT / M22_REL).read_text(encoding="utf-8")
host_text = (REPO_ROOT / HOST_REL).read_text(encoding="utf-8")
if "pub fn set_role" not in m22_text:
    err(f"[落点] {M22_REL} 里没有 `pub fn set_role`"
        " ⇒ ⇒ D-131 的 M-22 侧落点没落地 ⇒ ⇒ 必须红")
if "pub fn with_role" not in host_text:
    err(f"[落点] {HOST_REL} 里没有 `pub fn with_role`"
        " ⇒ ⇒ 宿主侧的角色构造入口没落地 ⇒ ⇒ 必须红")
if "pub fn set_role" not in host_text:
    err(f"[落点] {HOST_REL} 里没有 `pub fn set_role`"
        " ⇒ ⇒ 宿主侧的角色写入口没落地 ⇒ ⇒ 必须红")
# ⚠️⚠️ **禁列**：那个**独立布尔字段**必须**不在**宿主文件里 ——
#    ⚠️ 精确匹配 `admin_enabled: bool`（**带类型标注**）⇒ ⇒ 派生只读量
#    `admin_enabled()` **不在**禁列范围里（那个要保留）。
if re.search(r"admin_enabled\s*:\s*bool", host_text):
    err(f"[禁列] {HOST_REL} 里还有 `admin_enabled: bool` 这个**独立布尔字段**"
        " ⇒ ⇒ 角色与布尔两个状态仍能打架 ⇒ ⇒ 安全方向的漏洞没堵 ⇒ ⇒ 必须红")

# ---- 7g. **D-130 的落点必须仍在位**（本批推后 ⇒ ⇒ 一个字不许动） --------
for tok in ("pub trait DegradeExecutor", "pub fn install_executor", "pub fn degrade"):
    if tok not in m22_text:
        err(f"[推后] {M22_REL} 里没有 {tok!r}"
            " ⇒ ⇒ 本批**推后** D-130 ⇒ ⇒ 那些落点**一个字不许动** ⇒ ⇒ 必须红")

# ---- 7h. **两条裁决的字面量**（逐条判，收窄到那一条裁决**自己那段**） ----
# ⚠️ **不许扫全篇** —— §9 的变异表**逐字引用**了其中一句 ⇒ ⇒ 扫全篇会**自己红**。
VERDICT_PIN = {
    "B": "role 的唯一存储留在",
    "C": "独立布尔字段删除",
}
sec7_lines = section(r"^##\s+§7\b")
cur = None
seg = {}
for line in sec7_lines:
    m = re.match(r"^###\s*裁决\s*([A-N])\s*·", line)
    if m:
        cur = m.group(1)
        seg.setdefault(cur, [])
        # ⚠️⚠️ **标题行本身也要收进那段** —— 裁决的**关键短语常在标题里**
        #    （例如「独立布尔字段删除」只出现在裁决 C 的标题上）⇒ ⇒
        #    若只收正文 ⇒ ⇒ 那条短语**永远找不到** ⇒ ⇒ **7h 恒红** ⇒ ⇒
        #    ⇒ ⇒ **零判别力**（不是「判据红」而是「判据坏」）⇒ ⇒ 必须收。
        seg[cur].append(line)
        continue
    if cur:
        seg[cur].append(line)
for vid, phrase in VERDICT_PIN.items():
    if vid not in seg:
        err(f"[钉住] §7 里找不到裁决 {vid} 那一段 ⇒ ⇒ 7h 的检查**静默失效** ⇒ ⇒ 必须红")
        continue
    if phrase not in _norm("\n".join(seg[vid])):
        err(f"[钉住] 裁决 {vid} **自己那段里**没有 {phrase!r}"
            " ⇒ ⇒ 裁决被改软 ⇒ ⇒ 必须红")

# ---- 7i. **台账与规格的「真实未闭环」必须一致** -------------------------
ledger_path = REPO_ROOT / LEDGER_REL
if not ledger_path.exists():
    err(f"[一致] 找不到台账 {LEDGER_REL} ⇒ ⇒ 7i 的检查**静默失效** ⇒ ⇒ 必须红")
else:
    ledger_text = ledger_path.read_text(encoding="utf-8")
    # ⚠️⚠️ **台账里有 8 处「真实未闭环数 = … = N**」**（历史批次各留一行）⇒ ⇒
    #    取**第一个**会读到**历史值**（实测读到 65）⇒ ⇒ **假红** ⇒ ⇒
    #    ⇒ ⇒ 口径 = **最后一条**以 `> **真实未闭环数 = ` **开头**的行
    #    （那一行是**当前**口径；后面的 `> v0.19 / v0.2 …` 那种是**历史段落**
    #    ⇒ ⇒ 行首形态不同 ⇒ ⇒ 不会被选中）⇒ ⇒ 一个数**只服务一个口径**。
    ledger_hits = []
    for line in ledger_text.splitlines():
        if not line.startswith("> **真实未闭环数 = "):
            continue
        m = re.search(r"=\s*(\d+)\*\*", line)
        if m:
            ledger_hits.append(m.group(1))
    if not ledger_hits:
        err("[一致] 台账里找不到「> **真实未闭环数 = … = N**」那个**当前**落点"
            " ⇒ ⇒ 7i 的检查**静默失效** ⇒ ⇒ 必须红")
    else:
        ledger_n = ledger_hits[-1]
        if ledger_n != EXPECT_OPEN_DEBT:
            err(f"[一致] 台账的「真实未闭环」是 {ledger_n}，"
                f"本批的期望是 {EXPECT_OPEN_DEBT} ⇒ ⇒ 必须红")
        # ⚠️ **规格 §5.4 的 T-3 行**必须写同一个数 ⇒ ⇒ 两处**互为对照**。
        t3 = _find_accept_line("T-3")
        if t3 is None:
            err("[一致] §5 里找不到 **T-3** 行 ⇒ ⇒ 7i 的规格侧**静默失效** ⇒ ⇒ 必须红")
        else:
            mm = re.search(r"仍\s*(\d+)", t3)
            if mm is None:
                err("[一致] §5 的 T-3 行没有把「真实未闭环」的数字写出来"
                    " ⇒ ⇒ 7i 的规格侧**静默失效** ⇒ ⇒ 必须红")
            elif mm.group(1) != ledger_n:
                err(f"[一致] 规格 §5.4 的 T-3 行写的是 {mm.group(1)}，"
                    f"台账写的是 {ledger_n} ⇒ ⇒ 两处**不一致** ⇒ ⇒ 必须红")

# ---- 7j. **判据短语必须钉在指定的那一行**（收窄断言范围） ---------------
# ⚠️⚠️ **为什么必须有这一项** —— 实测教训：§9 的 **T3** 变异把 §5.3 的 **C-2 行**
#    里的「搜不到任何日志文本」改成「搜不到日志**标题**」⇒ ⇒ **自检脚本零红**
#    ⇒ ⇒ **零判别力**！根因 = 那个短语在规格里**出现 4 次**（§3.4 也有一处）⇒ ⇒
#    7 的「全局必须提到」那条判据**命中了别处** ⇒ ⇒ **改一处抓不到**。
# ⇒ ⇒ **修法**：把「**哪些短语必须出现在哪一行**」**逐行**钉住 ⇒ ⇒
#    收窄到「那段文档自己的块」⇒ ⇒ 判据**只**看那一行 ⇒ ⇒ 改那一行必红。
# ⚠️ 这正是「**文档改动类断言必须把检查范围收窄到那段文档自己的块**」那条纪律。
ACCEPT_PHRASE_PINS = {
    "C-2": "搜不到任何日志文本",
    "C-4": "恒等于 0",
}
for code, phrase in ACCEPT_PHRASE_PINS.items():
    row = _find_accept_line(code)
    if row is None:
        err(f"[钉住] §5 里找不到 **{code}** 行 ⇒ ⇒ 7j 的检查**静默失效** ⇒ ⇒ 必须红")
        continue
    if phrase not in row:
        err(f"[钉住] §5 的 {code} 行里没有 {phrase!r}"
            f" ⇒ ⇒ 该判据被改软（**只**看那一行）⇒ ⇒ 必须红")

# ---- 输出 -----------------------------------------------------------------
print(f"检查项：7（+ 7b/7c/7d/7e/7f/7g/7h/7i）  失败 {len(fail)}")
for f in fail:
    print(f"  ✗ {f}")
if not fail:
    print("  ✓ 全部通过")
    sys.exit(0)
sys.exit(1)
