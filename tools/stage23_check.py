#!/usr/bin/env python3
"""STAGE-23 规格自检（七项，与 stage22_check.py 同构）。

用法：python3 tools/stage23_check.py docs/detailed-design/STAGE-23_V039_PMP_Per_Piece.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）
       ＋ 7b. §1.2 的实测事实表必须是 **F1~F15 连续**（删掉一条实测事实
          也是「把规格改软」⇒ ⇒ 事实表本身要钉住）
       ＋ 7c. §8 的施工期订正必须是 **订正 1~6 连续**且每条都有「已改成什么」

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **本批的三个版本都不升**（裁决 A）⇒ ⇒ 第 7 项的「禁列」必须钉住
`SCHEMA_VERSION = 8`、`SIM_STATE_SCHEMA_VERSION = 3` 与 `PLATFORM_SCHEMA_VERSION = 2`
⇒ ⇒ 一旦出现，说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。

⚠️ **契约层的签名不许变**（裁决 H）⇒ ⇒ 第 7 项的「禁列」必须钉住
「按片传入」那种签名形态 ⇒ ⇒ 签名一变就是**破坏性的 API 变更** ⇒ ⇒
「要不要升契约版本」这个问题会被逼出来 ⇒ ⇒ 而本批的结论是「**不升**」。

⚠️ **参考侧是「逐片」**（F11）⇒ ⇒ 三种读法（只按起始地址 / 整段覆盖 / 逐片）
在 F9 + F10 这一对上的判决各不相同 ⇒ ⇒ 第 7 项的「must」必须把**两条判别式
的观测值**都钉住 ⇒ ⇒ 少钉一条就可能悄悄滑回「整段覆盖」。

⚠️ **台账 D-152 原文的两条前提都被证伪**（订正 2 / 订正 3）⇒ ⇒ 「只按起始地址」
与「整段覆盖」这两个**旧读法**只允许出现在「**被推翻的旧结论**」里（§1.3 / §8）
⇒ ⇒ §2.3 的正文**不许**把它们写成结论。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-23_V039_PMP_Per_Piece.md")
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []


def err(msg):
    fail.append(msg)


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
for v in sorted(verdicts):
    # ⚠️ **只认「裁决 X」这种带前缀的引用** ⇒ 规格正文里到处是「条目 0」这种
    #    **单字母加粗** ⇒ 粗匹配会把悬空检查变成假绿。
    if not re.search(rf"裁决[^。\n|]{{0,12}}?{v}\b", body):
        err(f"[裁决] 裁决 {v} 在正文无引用（悬空）")

# ---- 5. 章节完整 §0~§11 ---------------------------------------------------
heads = [l for l in lines if re.match(r"^##\s+§\d+", l)]
nums = sorted(int(re.search(r"§(\d+)", h).group(1)) for h in heads)
if nums != list(range(0, 12)):
    err(f"[章节] §0~§11 不完整，实得 {nums}")

# ---- 6. 验收编号交叉引用（§5 内每个编号都被 §5 定义） ---------------------
sec5 = []
grab = False
for line in lines:
    if line.startswith("## §5"):
        grab = True
        continue
    if line.startswith("## §6"):
        grab = False
    if grab:
        sec5.append(line)
sec5_text = "\n".join(sec5)
ids = set(re.findall(r"\b([ABCTX]-\d+)\b", sec5_text))
defined = set(re.findall(r"^\|\s*\*?\*?([ABCTX]-\d+)\*?\*?\s*\|", sec5_text, re.M))
undef = ids - defined
if undef:
    err(f"[验收] §5 内出现未定义的验收编号 {sorted(undef)}")
for grp in "ABCTX":
    got = sorted(x for x in defined if x.startswith(grp + "-"))
    if not got:
        err(f"[验收] §5 缺 {grp} 组验收")
    if got:
        nums_g = sorted(int(x.split("-")[1]) for x in got)
        if nums_g != list(range(1, len(nums_g) + 1)):
            err(f"[验收] {grp} 组编号不连续：{got}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    # ---- 判据语料（裁决 K / L）----
    "local/pmp_lock_tor.S",
    "local/pmp_straddle.S",
    "tests/golden/README.md",
    "tests/golden/corpus.txt",
    "tests/golden/env/link.ld",
    "0x8000_1000",   # tohost 所在的页 ⇒ 锁住的区间必须避开它
    # ---- 契约层落点（裁决 B / C / D / E / G / H）----
    "crates/contract/src/csr.rs",
    "pmp_access_allowed",
    "PmpRegion::covers",
    "PMP_GRAINULARITY_SHIFT",
    "PMP_ENTRY_COUNT",
    "pmp_permits",
    "pmp_region",
    # ---- 内核落点（裁决 I / J）----
    "crates/kernel/src/m13_iss/pmp.rs",
    "crates/kernel/src/m13_iss/executor.rs",
    "pmp_allows",
    "legalize_write",
    # ---- 版本常量（裁决 A）----
    "SCHEMA_VERSION",
    "SIM_STATE_SCHEMA_VERSION",
    "PLATFORM_SCHEMA_VERSION",
    # ---- 实测事实的关键字面量（§1.2）----
    "0x2001_1234",   # F1：被冻住的旧值
    "0x2001_5678",   # F1 / F2：尝试写入的新值 / 不锁时的落地值
    "0x2001_BEEF",   # F3：锁住条目自己的旧值
    "0x2001_CAFE",   # F3：尝试写入但没落地的值
    "0x0000_8F0F",   # F7：pmpcfg 逐字节 + L 粘滞的读回值
    "0xCCCC_CCCC",   # F8：未对齐但完全在区间内 ⇒ 放行
    "0xBBBB_AAAA",   # F9：跨界放行时的混合值 ⇒ 证明确实跨了界
    "0x800F_FFFE",   # F9：跨界读的起始地址
    "0x8003_FFFE",   # F10：尾部落进 DENY 的跨界读的起始地址
    "0x8004_0000",   # F10：DENY 区域的下界
    "0x8010_0000",   # F9：两个 ALLOW 条目的边界
    "0x2001_ffff",   # 1 MiB NAPOT 的 pmpaddr
    "0x2005_ffff",   # 第二个 1 MiB NAPOT 的 pmpaddr
    "0x2001_01ff",   # 4 KiB NAPOT 的 pmpaddr
    # ---- 事实与订正的编号 ----
    "F11",           # 「参考侧唯一钉在逐片」
    "F12",           # 平台侧三处分歧、两个方向
    "F15",           # 分歧被隔离到 PMP 判定规则
    # ⚠️ **F11 的结论本身**要钉住 —— 只钉「F11」这个**编号**是**不够**的
    #    （实测：把全文的「逐片」换成「整段覆盖」⇒ 旧钉法**抓不到** ⇒ ⇒
    #    零判别力）⇒ ⇒ 这里钉的是**标题**与**判决表**本身。
    "参考侧唯一钉在「逐片」",
    "只按起始地址 ⇒ (绿, 绿)",
    "整段覆盖 ⇒ (红, 红)",
    "逐片 ⇒ (红, 绿)",
    # ⚠️ **§2 那两节的标题**也要钉 —— 它们是「这条规则是什么」的**结论性**
    #    位置 ⇒ ⇒ 散文里写错一个字不要紧，**标题**写错就是「改软」。
    "§2.2 判定改成**逐片**",
    "§2.3 逐片的**决定链**",
    "D-152",
    "D-94",
    "D-143",
    "D-153",        # 旁支分歧：PMP 的特权口径（MPP + S 模式）
    "D-154",        # 旁支分歧：PMP 拒绝时上报的 mtval
    "STAGE-23",
    "STAGE-24",      # I2C 的规格号顺延
    # ---- 纪律 ----
    "一个约束一个落点",
    "施工期订正",
    "只引规范",
    "逐片",
]
text = "\n".join(lines)
# ⚠️ **字面量比对一律大小写不敏感** —— 约束是「**哪个**字面量出现」，
#    **不是**「用哪种大小写写」⇒ ⇒ 十六进制的大小写不该造成假红。
text_lc = text.lower()
for m in must:
    if m.lower() not in text_lc:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    # ⚠️ **签名不许变**（裁决 H）⇒ ⇒ 「按片传入」那种形态不该出现
    "per_piece: &[",                  # 假想的新签名形态
    "entries_per_piece",
    # ⚠️ **参考侧是「逐片」** ⇒ ⇒ 这两个**旧读法**只允许出现在「被推翻的旧结论」里
    "参考侧只按起始地址判 ⇒ ⇒ 参考侧",  # 半截的旧结论不许当结论写
    "整段覆盖判 ⇒ ⇒ 参考侧",
    # ⚠️ **写错的地址**（初稿把 b12 的地址少写了一位十六进制数字）⇒ ⇒
    #    钉住**正确**地址的同时把**错误**地址钉进禁列 ⇒ ⇒ 否则「少一位」
    #    这种错不会被任何自检发现 —— 它看起来仍像一个合法的地址。
    "0x8000_FFFE",   # ❌ 错：b12 的起始地址是 0x800F_FFFE
    "0x8001_0000",   # ❌ 错：两个 ALLOW 条目的边界是 0x8010_0000
]
for f in forbidden:
    if f.lower() in text_lc:
        err(f"[禁列] 规格出现已废止字面量 {f}")

# ---- 7d. 三个版本号的值必须还是原值（**不升版**） ------------------------
# ⚠️ **不能**用「`SCHEMA_VERSION = 8`」这种字面量去禁 —— 规格里那三个版本号
#    **不是**用「= N」的形式写的（写的是「仍 **7**」）⇒ ⇒ 那种禁列**永远匹配
#    不上** ⇒ ⇒ **零判别力**（**实测**：把「仍 **7**」改成「仍 **8**」⇒ 旧
#    禁列**抓不到**）⇒ ⇒ 改成「**找到常量名 ⇒ 看紧跟其后的那个数字**」。
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

# ---- 7b. §1.2 的实测事实表：F1~F15 必须**连续**且齐全 --------------------
# ⚠️ **删掉一条实测事实**也是「把规格改软」的一种 ⇒ ⇒ 事实表本身要钉住。
facts = []
grab = False
for line in lines:
    if line.startswith("### §1.2"):
        grab = True
        continue
    if grab and line.startswith("### "):
        grab = False
    if grab:
        m = re.match(r"^\|\s*\**\s*(F\d+)\s*\**\s*\|", line)
        if m:
            facts.append(m.group(1))
want = [f"F{i}" for i in range(1, 16)]
if facts != want:
    err(f"[事实] §1.2 的实测事实表不是 F1~F15 连续：{facts}")

# ---- 7c. §8 施工期订正：编号必须 **1..6 连续**且每条都有「已改成什么」 ----
# ⚠️ **施工期订正是规格被实测推翻的账** ⇒ ⇒ 删掉一条 = 把已被推翻的判据
# 悄悄留回规格里 ⇒ ⇒ 必须钉住。
corrections = []
grab = False
for line in lines:
    if line.startswith("## §8"):
        grab = True
        continue
    if grab and line.startswith("## "):
        grab = False
    if grab:
        m = re.match(r"^###\s*订正\s*(\d+)\s*·", line)
        if m:
            corrections.append(int(m.group(1)))
want_corr = list(range(1, 7))
if corrections != want_corr:
    err(f"[订正] §8 的施工期订正编号不是 1..6 连续：{corrections}")

sec8 = []
grab = False
for line in lines:
    if line.startswith("## §8"):
        grab = True
        continue
    if grab and line.startswith("## "):
        grab = False
    if grab:
        sec8.append(line)
sec8_text = "\n".join(sec8)
if sec8_text.count("已改成什么") < 6:
    err(f"[订正] §8 的「已改成什么」不足 6 条（实测 {sec8_text.count('已改成什么')}）")

# ⚠️ **§1.3 的「被推翻的旧结论」必须四条齐全** ⇒ ⇒ 那是「台账原文被推翻」的账
sec13 = []
grab = False
for line in lines:
    if line.startswith("### §1.3"):
        grab = True
        continue
    if grab and line.startswith("### "):
        grab = False
    if grab:
        sec13.append(line)
if len(sec13) < 4:
    err(f"[旧结论] §1.3 的被推翻旧结论不足 4 条（实测 {len(sec13)} 行）")

# ---- 7e. 跨层一致性：同一个地址字面量在**四处**必须逐字相同 ----------------
# ⚠️ **本批实测抓到的真缺口**：b12 的起始地址被写成 `0x8000_FFFE`
#    （**少一位**十六进制数字）⇒ ⇒ 而同一个地址要**手抄**到**四处**
#    ⇒ ⇒ **规格** / **语料源码的注释** / **语料登记的注释** / **golden README**
#    ⇒ ⇒ 结果是**三处抄错、一处（代码里的那条指令）是对的**
#    ⇒ ⇒ ⇒ 「**手抄同一个字面量到多处**」这件事**本身**就是缺陷
#    ⇒ ⇒ ⇒ **跨层逐字比对**必须进门禁（否则下一次照样抄错）。
#: ⚠️ **归一化**：汇编里写的是 `0x800ffffe`（无下划线、小写）而散文里写的是
#:    `0x800F_FFFE`（有下划线、大写）⇒ ⇒ 比的是**归一化后**的形状。
def _norm_addr(s: str) -> str:
    return s.replace("_", "").lower()


CROSS_LAYER_FILES = [
    "docs/detailed-design/STAGE-23_V039_PMP_Per_Piece.md",
    "tests/golden/local/pmp_straddle.S",
    "tests/golden/corpus.txt",
    "tests/golden/README.md",
]
#: ⚠️ **必须出现在全部四处**的那几个地址（两条判别式的起始地址）。
CROSS_LAYER_REQUIRED = ["0x800F_FFFE", "0x8003_FFFE"]
#: ⚠️ **一处都不许出现**的那两个地址（**抄错**的形态 ⇒ ⇒ 出现即红）。
CROSS_LAYER_FORBIDDEN = ["0x8000_FFFE", "0x8001_0000"]

_repo_root = Path(__file__).resolve().parent.parent
for _rel in CROSS_LAYER_FILES:
    _p = _repo_root / _rel
    if not _p.exists():
        # ⚠️ **文件没了 ⇒ ⇒ 跨层检查静默失效** ⇒ ⇒ 与「找不到候选清单块」同形
        err(f"[跨层] 找不到 {_rel} ⇒ ⇒ 跨层一致性检查**静默失效** ⇒ ⇒ 必须红")
        continue
    _body = _norm_addr(_p.read_text(encoding="utf-8"))
    for _lit in CROSS_LAYER_REQUIRED:
        if _norm_addr(_lit) not in _body:
            err(f"[跨层] {_rel} 缺少关键地址字面量 {_lit}")
    for _lit in CROSS_LAYER_FORBIDDEN:
        if _norm_addr(_lit) in _body:
            err(f"[跨层] {_rel} 出现**抄错的**地址字面量 {_lit}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
