#!/usr/bin/env python3
"""STAGE-20 规格自检（七项，与 stage19_check.py 同构）。

用法：python3 tools/stage20_check.py docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。

⚠️ **验收分组刻意不用 D**：台账编号（D-94）与验收编号**形态相同**
⇒ 本规格的工具层验收组用 **T**（tool）、跨层组用 **X**（cross）⇒ ⇒ **两套编号不会撞**。

⚠️ **本批的两个版本都不升**（裁决 B）⇒ ⇒ 第 7 项的「禁列」必须钉住
`SCHEMA_VERSION = 8`、`SIM_STATE_SCHEMA_VERSION = 3` 与 `PLATFORM_SCHEMA_VERSION = 2`
⇒ ⇒ 一旦出现，说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md")
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
    # ⚠️ **只认「裁决 X」这种带前缀的引用** ⇒ 规格正文里到处是「生效图 **A**」
    #    这种**单字母加粗** ⇒ 粗匹配会把悬空检查变成假绿。
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
    # ---- 契约层：计数器 CSR 的常量（裁决 C：唯一落点在契约层） ----
    "CSR_MCOUNTINHIBIT",
    "CSR_MCYCLE",
    "CSR_MINSTRET",
    "CSR_MCYCLEH",
    "CSR_MINSTRETH",
    "CSR_CYCLE",
    "CSR_TIME",
    "CSR_INSTRET",
    "CSR_CYCLEH",
    "CSR_TIMEH",
    "CSR_INSTRETH",
    "counter_pair",        # 裁决 C：半部配对的唯一落点
    "counter_shadow",      # 裁决 C / D：影子映射的唯一落点
    "is_csr_read_only",    # 裁决 E：只读地址规则（修 bug）
    "csr_is_implemented",  # §2.4：计数器必须被实现，否则写被忽略
    "csr_write_is_ignored",
    "SCHEMA_VERSION",          # 裁决 B：契约信封版本（**不升**）
    "SIM_STATE_SCHEMA_VERSION",  # 裁决 B：快照版本（**不升**）
    "PLATFORM_SCHEMA_VERSION",   # 描述形状版本（**不升**）
    # ---- 内核：落点 ----
    "crates/contract/src/csr.rs",          # 裁决 C / E
    "crates/kernel/src/m13_iss/csr.rs",    # 裁决 D：影子读
    "crates/kernel/src/m13_iss/executor.rs",  # 裁决 F：自增的唯一落点
    "execute_one",        # 裁决 F / G
    "wrote_csr",          # 裁决 H：抑制规则的零新增状态通道
    # ---- 工具层 ----
    "tools/golden-run.sh",                 # 裁决 M：默认 ISA
    "tools/stage20_check.py",              # 裁决 O：本批的规格自检
    "tests/golden/README.md",              # 裁决 L：回填三处过期文字
    "tests/golden/corpus.txt",             # 裁决 L：语料仍是 68
    "rv32imc_zicsr_zicclsm_zifencei_zicntr",  # 裁决 M：默认 ISA 串
    # ---- 用例与台账 ----
    "rv32mi/instret_overflow",   # 裁决 L：出空洞清单
    "rv32mi/zicntr",             # 裁决 L：不入库
    "D-94",                      # 本批闭环的那一条
    # ---- 纪律 ----
    "一个约束一个落点",            # 裁决 C
    "施工期订正",                  # 施工期订正记录节
    "STAGE-21",                   # 裁决 O：I2C 那批顺延到的规格号
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",             # 本批**不升**契约信封版本（裁决 B）
    "SIM_STATE_SCHEMA_VERSION = 3",   # 本批**不升**快照版本（裁决 B）
    "PLATFORM_SCHEMA_VERSION = 2",   # 本批**不升**描述形状版本（裁决 B）
    "E_PLT_17",                      # 本批不新增场景码 ⇒ 下一个码**不该**出现
    "V-20",                          # 本批不新增校验规则 ⇒ 下一条规则**不该**出现
    "E-PLT-17",                      # 仓内族名写法是 `E_PLT_17`（不是这个）
    "E_PLAT_17",
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
