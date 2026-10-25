#!/usr/bin/env python3
"""STAGE-22 规格自检（七项，与 stage21_check.py 同构）。

用法：python3 tools/stage22_check.py docs/detailed-design/STAGE-22_V038_Smode.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）
       ＋ 7b. §1.2 的实测事实表必须是 **F1~F33 连续**（删掉一条实测事实
          也是「把规格改软」⇒ ⇒ 事实表本身要钉住）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **验收分组刻意不用 D**：台账编号（D-94）与验收编号**形态相同**
⇒ 本规格的工具层验收组用 **T**（tool）、跨层组用 **X**（cross）⇒ ⇒ **两套编号不会撞**。

⚠️ **本批的三个版本都不升**（裁决 A）⇒ ⇒ 第 7 项的「禁列」必须钉住
`SCHEMA_VERSION = 8`、`SIM_STATE_SCHEMA_VERSION = 3` 与 `PLATFORM_SCHEMA_VERSION = 2`
⇒ ⇒ 一旦出现，说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。

⚠️ **`mstatus` 的 WARL 掩码钉死 `0x807E_79AA`**（实测 F6）⇒ ⇒ 旧的
`MSTATUS_WARL_MASK = 0x807F_FFEA` 出现在「赋值形态」上 ⇒ ⇒ **红**
（规格正文里提到旧值本身是**允许**的 —— 它是「被推翻的旧结论」的一部分）。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-22_V038_Smode.md")
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

# ---- 4. 裁决编号 A~O 无悬空引用 ------------------------------------------
verdicts = set()
for line in lines:
    m = re.match(r"^###\s*裁决\s*([A-O])\s*·", line)
    if m:
        verdicts.add(m.group(1))
expected = set("ABCDEFGHIJKLMNO")
missing = expected - verdicts
if missing:
    err(f"[裁决] 裁决表缺 {sorted(missing)}")
extra = verdicts - expected
if extra:
    err(f"[裁决] 出现表外的裁决编号 {sorted(extra)}")
body = "\n".join(l for l in lines if not re.match(r"^###\s*裁决\s*[A-O]", l))
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
    # ---- 契约层：S/U 模式的 CSR 常量与函数（裁决 B / C / D / E / F） ----
    "CSR_SSTATUS",
    "CSR_SIE",
    "CSR_STVEC",
    "CSR_SSCRATCH",
    "CSR_SEPC",
    "CSR_SCAUSE",
    "CSR_STVAL",
    "CSR_SIP",
    "CSR_SATP",
    "CSR_MCOUNTEREN",
    "CSR_SCOUNTEREN",
    "CSR_UCOUNTEREN",
    "CSR_MEDELEG",
    "CSR_MIDELEG",
    "MSTATUS_SIE_BIT",
    "MSTATUS_SPIE_BIT",
    "MSTATUS_SPP_BIT",
    "MSTATUS_MPP_SHIFT",
    "MSTATUS_MPP_U",
    "MSTATUS_MPP_S",
    "MSTATUS_MPP_M",
    "SSTATUS_WARL_MASK",
    "mstatus_warl",
    "csr_is_implemented",
    "csr_write_is_ignored",
    "CSR_PRIV",                 # 裁决 E：特权级住在 CSR 文件里
    "SCHEMA_VERSION",           # 裁决 A：契约信封版本（**不升**）
    "SIM_STATE_SCHEMA_VERSION",  # 裁决 A：快照版本（**不升**）
    "PLATFORM_SCHEMA_VERSION",   # 裁决 A：描述形状版本（**不升**）
    # ---- 内核：落点 ----
    "crates/contract/src/csr.rs",
    "crates/kernel/src/m13_iss/csr.rs",
    "crates/kernel/src/m13_iss/executor.rs",
    "mret",
    "sret",
    "medeleg",
    "mideleg",
    "stvec",
    "scause",
    "sepc",
    # ---- 工具层 ----
    "tools/stage22_check.py",           # 本批的规格自检
    "tools/golden-run.sh",
    "tests/golden/README.md",
    "tests/golden/corpus.txt",
    "tests/golden/env/smode/riscv_test.h",  # 裁决 M：新 env 变体
    "tests/golden/local/smode_csr.S",   # 裁决 N：判据语料
    "rv32i_zicsr_zifencei",             # 裁决 N：语料的 march 串（不带 s）
    # ---- 实测事实的关键字面量 ----
    "0x807E_79AA",   # F6：mstatus 的真实可写掩码
    "0x22",          # F8：sstatus 的写掩码
    "0x4014_1104",   # F3 / F24：misa 的逐位值
    "0x0C0",         # 裁决 E：伪 CSR 的地址
    "rv32si/sbreak", # 裁决 N：入库
    "rv32si/scall",  # 裁决 N：入库
    "rv32si/csr",    # 裁决 N：入库
    "D-94",          # 本批闭环的那一条
    "D-96",          # 顺带闭环的那一条
    "0x0000_0122",   # 订正 7：sstatus 的读视图掩码
    "0x11A9",        # 订正 4：上游 env 的 medeleg 掩码
    "SSTATUS_VIEW_MASK",  # 订正 7
    "X-4",           # 判别力：U 模式段真的跑了
    "CSR_PRIV",      # 裁决 E
    "0x10200073",    # F32：sret 的编码
    "F32",           # 施工期补测的倒数第二条
    "F33",           # 施工期补测的最后一条（推翻初稿的「整次丢弃」）

    # ---- 纪律 ----
    "一个约束一个落点",
    "施工期订正",
    "STAGE-22",
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",             # 本批**不升**契约信封版本（裁决 A）
    "SIM_STATE_SCHEMA_VERSION = 3",   # 本批**不升**快照版本（裁决 A）
    "PLATFORM_SCHEMA_VERSION = 2",    # 本批**不升**描述形状版本（裁决 A）
    "MSTATUS_WARL_MASK = 0x807F_FFEA",  # F6：旧掩码是超集（赋值形态不该出现）
    "rv32imc_zicsr_zicclsm_zifencei_zicntr_s",  # F1：Spike 不接受 ISA 串里的 s
    "MSTATUS_WARL_MASK = 0x807E_79AA",  # 本批**要**改这个常量 ⇒ 赋值形态不该在规格里
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

# ---- 7b. §1.2 的实测事实表：F1~F33 必须**连续**且齐全 --------------------
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
want = [f"F{i}" for i in range(1, 34)]
if facts != want:
    err(f"[事实] §1.2 的实测事实表不是 F1~F33 连续：{facts}")

# ---- 7c. §8 施工期订正：编号必须 **1..8 连续**且每条都有「已改成什么」 ----
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
want_corr = list(range(1, 10))
if corrections != want_corr:
    err(f"[订正] §8 的施工期订正编号不是 1..9 连续：{corrections}")

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
if sec8_text.count("已改成什么") < 9:
    err(f"[订正] §8 的「已改成什么」不足 9 条（实测 {sec8_text.count('已改成什么')}）")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
