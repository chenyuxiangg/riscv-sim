#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-31 规格自检（七项 + 九个子项，与 stage30_check.py 同构）。

用法：python3 tools/stage31_check.py docs/detailed-design/STAGE-31_V046_Degrade_Mechanism_D130.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用（§7 每条都有标题；正文引用的编号都在表内）
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（A-1~A-3 / B-1~B-7 / C-1~C-6 / T-1~T-4 / X-1~X-6）
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F1~F15 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + 编号连续 + **每条自己那段里**都有「已改成」
       ＋ 7d. **四个版本号的值**（§5.1 的 **A-1 行是唯一权威处**）
       ＋ 7e. 跨层一致性：§1.2 的 **F 表逐行**核对该行点名的**文件**与**标识符**
       ＋ 7f. **D-130 的落点必须在位**（M-22 的 trait / `install_executor` / `degrade` /
                新增的 `DegradeAttempt`；宿主侧**必须**出现 `install_executor` 与
                `set_degrade_policy` 的**调用**）
       ＋ 7g. 台账 **D-130 末列**必须**逐个**点名三档够不着的动作
                （`SnapshotRestore` / `Pause` / `Disconnect`）**与**本批的新标记
       ＋ 7h. **宿主只有一个降级入口**：`hms.degrade(` 在 `kernel_host/` **整个目录**
                里**必须 0 次**
       ＋ 7i. M-22 的**五组旧措辞**必须**一个都不剩**（逐组禁列）
       ＋ 7j. 台账与规格的「真实未闭环」必须一致（跨文件一致性）+ D-130 **不销账**

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **7c / 7f / 7g / 7h / 7i 都是「逐条判」、不是「数全局次数」** —— 这正是
    **STAGE-26 §8 订正 8** 的教训：数全局次数会被**开头那句散文**顶替 ⇒ ⇒
    删掉一条真的断言也**不红** ⇒ ⇒ **零判别力** ⇒ ⇒ 本脚本**不许**重犯同一个错。

⚠️ **7g 的口径是「本批专属的新标记」** —— 「三档动作的名字」在 v0.45 批就**已经**
   出现在台账里了 ⇒ ⇒ 只查名字 ⇒ ⇒ **零判别力** ⇒ ⇒ 所以额外查**本批才可能有**
   的三个标记（`v0.46` / `DegradeAttempt` / `E-HMS-01` 带连字符的写法）。

⚠️ **7f 在本批施工完成之前是红的** ⇒ ⇒ 那是**诚实**的（本批的落点还没落地）⇒ ⇒
    **批末必须绿**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-31_V046_Degrade_Mechanism_D130.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **台账路径**（7g / 7j 用）⇒ ⇒ **只此一处**定义。
LEDGER_REL = "docs/reviews/REVIEWS.md"

M22_REL = "crates/kernel/src/m22_hms/mod.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
HOST_DIR_REL = "crates/kernel/src/kernel_host"

#: ⚠️ **本批的「真实未闭环」期望值**（7j 用）—— 本批**不销账** ⇒ ⇒ 不减。
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
    **抽不出那一行** ⇒ ⇒ 7d 的权威处**静默失效** ⇒ ⇒ 必须红 ⇒ ⇒
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
    "A": [f"A-{i}" for i in range(1, 4)],   # A-1~A-3
    "B": [f"B-{i}" for i in range(1, 8)],   # B-1~B-7
    "C": [f"C-{i}" for i in range(1, 7)],   # C-1~C-6
    "T": [f"T-{i}" for i in range(1, 5)],   # T-1~T-4
    "X": [f"X-{i}" for i in range(1, 7)],   # X-1~X-6
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
    "crates/kernel/src/m18_probe/registry.rs",
    "crates/kernel/src/m19_waveform/pipeline.rs",
    "crates/kernel/src/m19_waveform/channel.rs",
    "crates/contract/src/error.rs",
    "crates/contract/src/lib.rs",
    # ---- 标识符 ----
    "pub trait DegradeExecutor",
    "DegradeAttempt",
    "DegradeOutcome",
    "Skipped",
    "install_executor",
    "set_degrade_policy",
    "sample_cfg",
    "overflow_policy",
    "OverflowPolicy",
    "PlatformError",
    "SCHEMA_VERSION",
    # ---- 台账与批次 ----
    "D-130",
    "STAGE-31",
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
    m = re.fullmatch(r"(F\d+)", row[0])
    if m:
        facts.append(m.group(1))
want_facts = [f"F{i}" for i in range(1, 16)]
if facts != want_facts:
    err(f"[事实] §1.2 的事实表编号不是 F1~F15 连续：{facts}")

# ---- 7c. §8 的施工期订正 -------------------------------------------------
sec8 = section(r"^##\s+§8\b")
if not sec8:
    err("[订正] §8 不存在或是空的")
else:
    corr = []
    cur = None
    seg = {}
    for line in sec8:
        m = re.match(r"^###\s*订正\s*(\d+)\s*·", line)
        if m:
            cur = m.group(1)
            corr.append(cur)
            seg.setdefault(cur, [])
            seg[cur].append(line)
            continue
        if cur:
            seg[cur].append(line)
    want_corr = [str(i) for i in range(1, len(corr) + 1)]
    if corr != want_corr:
        err(f"[订正] §8 的订正编号不连续：{corr}")
    for c in corr:
        blob = "\n".join(seg[c])
        if "**规格原文**" not in blob:
            err(f"[订正] 订正 {c} 缺「**规格原文**」")
        if "**实测**" not in blob:
            err(f"[订正] 订正 {c} 缺「**实测**」")
        if "**已改成**" not in blob:
            err(f"[订正] 订正 {c} 缺「**已改成**」")
        # ⚠️ **占位检查只查「已改成」那一段** ⇒ ⇒ 「规格原文」段**合法地**
        #    要**引用**当初的占位措辞 ⇒ ⇒ 全篇禁词会误伤 ⇒ ⇒ 收窄口径。
        changed = blob.split("**已改成**", 1)
        if len(changed) == 2 and ("待实测" in changed[1] or "待补" in changed[1]):
            err(f"[订正] 订正 {c} 的「已改成」段还是占位（待实测 / 待补）")

# ---- 7d. 四个版本号的值（§5.1 的 A-1 行是唯一权威处） -------------------
a1 = _find_accept_line("A-1")
if a1 is None:
    err("[版本] §5 里找不到 A-1 那一行 ⇒ ⇒ 版本号判据**静默失效** ⇒ ⇒ 必须红")
else:
    for name, val in (("SCHEMA_VERSION", "8"),
                      ("SIM_STATE_SCHEMA_VERSION", "2"),
                      ("PLATFORM_SCHEMA_VERSION", "1"),
                      ("PLATFORM_RULES_VERSION", "3")):
        if name not in a1:
            err(f"[版本] A-1 行里没有 {name} ⇒ ⇒ 版本号判据**静默失效**")
            continue
        if not re.search(re.escape(name) + r"\s*=\s*" + val + r"\b", a1):
            err(f"[版本] A-1 行里 {name} 的值不是 {val} ⇒ ⇒ 那一行写着：{a1!r}")

# ---- 7e. 跨层一致性：§1.2 的 F 表**逐行**核对文件与标识符 ----------------
# ⚠️ **原文与路径都从规格里读**，⛔ **不写死**在本脚本里 ⇒ ⇒ 规格改了、源没改 ⇒ ⇒ 红。
#: ⚠️ **口径是「标识符级」而不是「结论级」** ⇒ ⇒ 本批**会改掉其中几条的「结论」**
#:    ⇒ ⇒ 但**「落点」**（文件与标识符）不变 ⇒ ⇒ 7e 钉的是**落点** ⇒ ⇒ 结论的变化记在 **§8**。
PATH_TOKEN_RE = re.compile(r"^[A-Za-z0-9_./-]+\.(?:rs|js|mjs|py|md|html|txt|sh)$")
#: ⚠️ **token 的合法性**：不许含箭头、不许含 Markdown 粗体标记、不许含省略号
TOKEN_BAD = ("⇒", "**", "…")

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
        err(f"[跨层] §1.2 的 {fid} 行点名的标识符 {tok!r} 在源文件里找不到"
            " ⇒ ⇒ 规格与代码漂移")

# ---- 7f. **D-130 的落点必须在位** ----------------------------------------
m22_path = REPO_ROOT / M22_REL
host_path = REPO_ROOT / HOST_REL
m22_text = m22_path.read_text(encoding="utf-8") if m22_path.exists() else ""
host_text = host_path.read_text(encoding="utf-8") if host_path.exists() else ""
if not m22_text:
    err(f"[落点] 找不到 {M22_REL} ⇒ ⇒ 跨层检查**静默失效** ⇒ ⇒ 必须红")
if not host_text:
    err(f"[落点] 找不到 {HOST_REL} ⇒ ⇒ 跨层检查**静默失效** ⇒ ⇒ 必须红")
for tok in ("pub trait DegradeExecutor", "pub fn install_executor",
            "pub fn degrade", "pub enum DegradeAttempt", "pub enum DegradeOutcome"):
    if tok not in m22_text:
        err(f"[落点] {M22_REL} 里没有 {tok!r}"
            " ⇒ ⇒ D-130 的落点没落地 ⇒ ⇒ 必须红")
# ⚠️ 宿主侧**必须**出现执行器的安装与默认策略的设置 ⇒ ⇒ 「宿主永远装了执行器」
#    与「默认由策略表承载」这两条不变式**逐字**钉住。
if "install_executor" not in host_text:
    err(f"[落点] {HOST_REL} 里没有 `install_executor`"
        " ⇒ ⇒ 宿主**零注入点** ⇒ ⇒ 降级仍走不通派发路径 ⇒ ⇒ 必须红")
if not re.search(r"set_degrade_policy\s*\(\s*Severity::Fatal\s*,\s*DegradeAction::Terminate\s*\)",
                  host_text):
    err(f"[落点] {HOST_REL} 里没有「默认策略 = Fatal → Terminate」的设置"
        " ⇒ ⇒ 默认行为**不再**由策略承载 ⇒ ⇒ 必须红")

# ---- 7g. 台账 **D-130 末列**必须点名三档够不着的动作 + 本批的新标记 ----
ledger_path = REPO_ROOT / LEDGER_REL
ledger_text = ledger_path.read_text(encoding="utf-8") if ledger_path.exists() else ""
if not ledger_text:
    err(f"[台账] 找不到台账 {LEDGER_REL} ⇒ ⇒ 判据**静默失效** ⇒ ⇒ 必须红")
else:
    d130_row = None
    for line in ledger_text.splitlines():
        if line.startswith("| **D-130**"):
            d130_row = line
            break
    if d130_row is None:
        err("[台账] 台账里找不到 D-130 那一行 ⇒ ⇒ 判据**静默失效** ⇒ ⇒ 必须红")
    else:
        cells = [c.strip() for c in split_row(d130_row.strip())]
        if len(cells) != 4:
            err(f"[台账] D-130 那一行有 {len(cells)} 格（应 4 格）"
                " ⇒ ⇒ 抽不出「说明」那一格 ⇒ ⇒ 判据**静默失效** ⇒ ⇒ 必须红")
            body_cell = d130_row
        else:
            body_cell = cells[3]
        for act in ("SnapshotRestore", "Pause", "Disconnect"):
            if act not in body_cell:
                err(f"[台账] D-130 的说明格里没有点名 `{act}`"
                    " ⇒ ⇒ 三档够不着的动作**没有逐条**写进台账 ⇒ ⇒ 必须红")
        # ⚠️ **本批专属的新标记** —— 「三档动作的名字」v0.45 批就已经有了 ⇒ ⇒
        #    只查名字 ⇒ ⇒ **零判别力** ⇒ ⇒ 必须查**本批才可能有**的东西。
        for mark in ("v0.46", "DegradeAttempt", "E_HMS-01"):
            if mark not in body_cell:
                err(f"[台账] D-130 的说明格里没有本批的新标记 {mark!r}"
                    " ⇒ ⇒ 台账**没按 C 做法级别**更新 ⇒ ⇒ 必须红")

# ---- 7h. **宿主只有一个降级入口** ----------------------------------------
# ⚠️ **逐字判**：不是「数全局次数」⇒ ⇒ 收窄到**宿主目录**这一个范围。
host_dir = REPO_ROOT / HOST_DIR_REL
if not host_dir.is_dir():
    err(f"[入口] 找不到 {HOST_DIR_REL} 目录 ⇒ ⇒ 判据**静默失效** ⇒ ⇒ 必须红")
else:
    hits = []
    for p in sorted(host_dir.rglob("*.rs")):
        txt = p.read_text(encoding="utf-8")
        n = txt.count("hms.degrade(")
        if n:
            hits.append(f"{p.relative_to(REPO_ROOT)} × {n}")
    if hits:
        err(f"[入口] 宿主目录里还有 `hms.degrade(` 的直接调用：{'; '.join(hits)}"
            " ⇒ ⇒ 降级**有两个入口**（策略分支 + 宿主直做）⇒ ⇒ 会重复降级 ⇒ ⇒ 必须红")

# ---- 7i. M-22 的**五组旧措辞**必须一个都不剩 ----------------------------
# ⚠️ **逐组判** —— 不是「数全局次数」⇒ ⇒ 五组各自都必须**零**出现。
OLD_WORDINGS = [
    "降级动作本批只实现 `Terminate`",
    "本批**未实现**",
    "本批唯一有实现的动作",
    "只有 `Terminate` 有实现",
    "本批只落 `Terminate`",
]
if m22_text:
    for old in OLD_WORDINGS:
        if old in m22_text:
            err(f"[禁列] {M22_REL} 里还有旧措辞 {old!r}"
                " ⇒ ⇒ 文档还在说「只实现了一个动作」⇒ ⇒ 口径与实现**不一致** ⇒ ⇒ 必须红")

# ---- 7j. 台账与规格的「真实未闭环」必须一致 + D-130 不销账 ---------------
if ledger_text:
    # ⚠️ **只取最后一条**「> **真实未闭环数 = 」开头的行（历史行在后面 ⇒ 不取）。
    open_lines = [l for l in ledger_text.splitlines()
                  if l.startswith("> **真实未闭环数 = ")]
    if not open_lines:
        err("[台账] 台账里找不到「> **真实未闭环数 = 」那一行"
            " ⇒ ⇒ 跨文件一致性判据**静默失效** ⇒ ⇒ 必须红")
    else:
        last = open_lines[-1]
        m = re.search(r"=\s*(\d+)\*\*", last)
        if not m:
            err(f"[台账] 最后一条「真实未闭环数」行里抽不出数字 ⇒ ⇒ 判据**静默失效**")
        elif m.group(1) != EXPECT_OPEN_DEBT:
            err(f"[台账] 真实未闭环数 = {m.group(1)} ≠ 期望 {EXPECT_OPEN_DEBT}"
                " ⇒ ⇒ 本批**不销账** ⇒ ⇒ 那个数必须不变")
    # ⚠️ **D-130 不销账** ⇒ ⇒ 标记必须仍是「未闭环」。
    for line in ledger_text.splitlines():
        if line.startswith("| **D-130**"):
            if "⏳" not in line or "未闭环" not in line:
                err("[台账] D-130 的标记不是「⏳ **未闭环**」"
                    " ⇒ ⇒ 本批**不销账** ⇒ ⇒ 标记不许变 ⇒ ⇒ 必须红")
            if "✅" in line and "已闭环" in line:
                err("[台账] D-130 那一行出现了「✅ **已闭环**」"
                    " ⇒ ⇒ 本批**不销账** ⇒ ⇒ 必须红")
            break

print(f"检查项：7（+ 7b/7c/7d/7e/7f/7g/7h/7i/7j）  失败 {len(fail)}")
for m in fail:
    print(f"  ✗ {m}")
sys.exit(1 if fail else 0)
