#!/usr/bin/env python3
"""STAGE-21 规格自检（七项，与 stage20_check.py 同构）。

用法：python3 tools/stage21_check.py docs/detailed-design/STAGE-21_V037_PMP.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）
       ＋ 7b. §1.2 的实测事实表必须是 **F1~F21 连续**（删掉一条实测事实
          也是「把规格改软」⇒ ⇒ 事实表本身要钉住）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **验收分组刻意不用 D**：台账编号（D-94）与验收编号**形态相同**
⇒ 本规格的工具层验收组用 **T**（tool）、跨层组用 **X**（cross）⇒ ⇒ **两套编号不会撞**。

⚠️ **本批的两个版本都不升**（裁决 B）⇒ ⇒ 第 7 项的「禁列」必须钉住
`SCHEMA_VERSION = 8`、`SIM_STATE_SCHEMA_VERSION = 3` 与 `PLATFORM_SCHEMA_VERSION = 2`
⇒ ⇒ 一旦出现，说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。

⚠️ **条目数钉死 16**（裁决 C，对齐参考侧）⇒ ⇒ 「64 条」的痕迹
（`CSR_PMPADDR16` / `CSR_PMPADDR63` / `PMP_ENTRY_COUNT = 64`）一旦出现 ⇒ ⇒ 红。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-21_V037_PMP.md")
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
    # ---- 契约层：PMP 的常量与纯函数（裁决 B / C / D） ----
    "CSR_PMPCFG0",
    "CSR_PMPCFG3",
    "CSR_PMPADDR0",
    "CSR_PMPADDR15",
    "PMP_ENTRY_COUNT",
    "PMP_GRAINULARITY_SHIFT",
    "pmp_decode_cfg",
    "pmp_region",
    "PmpRegion",
    "covers",           # §8 订正 8：落点是 PmpRegion::covers，不是 pmp_entry_matches
    "pmp_cfg_warl",     # F20：pmpcfg 的写入要合法化
    "pmp_access_allowed",
    "csr_is_implemented",     # 裁决 E：不补 ⇒ PMP 成一排不起作用的寄存器
    "csr_write_is_ignored",
    "is_csr_read_only",
    "SCHEMA_VERSION",          # 裁决 B：契约信封版本（**不升**）
    "SIM_STATE_SCHEMA_VERSION",  # 裁决 B：快照版本（**不升**）
    "PLATFORM_SCHEMA_VERSION",   # 描述形状版本（**不升**）
    # ---- 内核：落点 ----
    "crates/contract/src/csr.rs",        # 裁决 B / D
    "crates/kernel/src/m13_iss/pmp.rs",  # 裁决 G：判定的唯一入口
    "crates/kernel/src/m13_iss/executor.rs",  # 裁决 K：三条访存路径
    "pmp_allows",
    "MPRV",                              # 裁决 H：特权口径
    "mstatus.MPRV",
    "MSTATUS_MPRV_BIT",                  # F10 / F11：位号钉死
    "fetch_pmp_trap",                    # §8 订正 9：两条派发路径共用取指判定
    "TrapCause::LoadAccessFault",        # 裁决 J：cause 5
    "TrapCause::StoreAccessFault",       # 裁决 J：cause 7
    "TrapCause::InstructionAccessFault",  # 裁决 J：cause 1
    # ---- 工具层 ----
    "tools/golden-run.sh",                 # 裁决 N：默认 ISA 不动
    "tools/stage21_check.py",              # 本批的规格自检
    "tests/golden/README.md",              # 裁决 M：env 偏差登记
    "tests/golden/corpus.txt",             # 裁决 L：语料
    "tests/golden/local/pmp_deny.S",       # 裁决 L：判据语料
    "rv32imc_zicsr_zicclsm_zifencei_zicntr",  # 裁决 N：默认 ISA 串
    # ---- 实测事实与判据的关键字面量 ----
    "pmpcfg0..3",
    "pmpaddr0..15",
    "0x1f18",        # 判据的打包值（条目 0 = 0x18、条目 1 = 0x1f）
    "0x200101ff",    # 判据的条目 0 地址编码
    "0x2001ffff",    # 判据的条目 1 地址编码
    "0x9f",          # F20：pmpcfg 的保留位清零后读回的值
    "0x1a",          # F20：保留组合（R=0 ∧ W=1）的写法
    "rv32mi/pmpaddr",   # 裁决 O：空洞，不入库
    "local/pmp_deny",
    "D-94",              # 本批闭环的那一条
    # ---- 纪律 ----
    "一个约束一个落点",     # 裁决 D / G
    "施工期订正",           # 施工期订正记录节
    "STAGE-22",            # 裁决 A：S 模式顺延到的规格号
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",             # 本批**不升**契约信封版本（裁决 B）
    "SIM_STATE_SCHEMA_VERSION = 3",   # 本批**不升**快照版本（裁决 B）
    "PLATFORM_SCHEMA_VERSION = 2",    # 本批**不升**描述形状版本（裁决 B）
    "E_PLT_17",                      # 本批不新增场景码 ⇒ 下一个码**不该**出现
    "CSR_PMPADDR16",                 # 裁决 C：只做 16 条 ⇒ 64 条的痕迹不该出现
    "CSR_PMPADDR63",
    "CSR_PMPCFG4",
    "PMP_ENTRY_COUNT = 64",           # 裁决 C：条目数钉死 16
    "2^(n+1)",                        # 订正 2：NAPOT 尺寸公式错一位就是一条判别力
    "2^(n+2)",
    "PMP_GRAINULARITY_SHIFT = 0",      # 订正 2：刻度错一位就是一条判别力
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

# ---- 7b. §1.2 的实测事实表：F1~F21 必须**连续**且齐全 --------------------
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
        m = re.match(r"^\|\s*(F\d+)\s*\|", line)
        if m:
            facts.append(m.group(1))
want = [f"F{i}" for i in range(1, 22)]
if facts != want:
    err(f"[事实] §1.2 的实测事实表不是 F1~F21 连续：{facts}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
