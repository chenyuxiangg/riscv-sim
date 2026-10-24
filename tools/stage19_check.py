#!/usr/bin/env python3
"""STAGE-19 规格自检（七项，与 stage18_check.py 同构）。

用法：python3 tools/stage19_check.py docs/detailed-design/STAGE-19_V034_OneConstraintOneLanding_Convergence.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）——
⚠️ **不要**在这里再写一份朴素 `split("|")` ⇒ ⇒ 本批的 D-149 就是**修这个**
⇒ ⇒ 本脚本自己**必须**是那个「只有一处定义」的样本。
sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

⚠️ **验收分组刻意不用 D**：台账编号（D-125 / D-126 / D-149）与验收编号**形态相同**
⇒ 本规格的工具层验收组用 **T**（tool）、跨层组用 **X**（cross）⇒ ⇒ **两套编号不会撞**。

⚠️ **本批的两个版本都不升**（裁决 B）⇒ ⇒ 第 7 项的「禁列」必须钉住
`SCHEMA_VERSION = 8` 与 `PLATFORM_SCHEMA_VERSION = 2` ⇒ ⇒ 一旦出现，
说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。

⚠️ **本批新增** `E_PLT_16`（wiring 非法）与 **V-18 / V-19**（那两条规则）⇒ ⇒ 它们
在「必列」里，而**下一个**码 `E_PLT_17` 与**下一条**规则 `V-20` 在「禁列」里。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402,F811  (与上面那句 import 同源，只为醒目)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-19_V034_OneConstraintOneLanding_Convergence.md")
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
    "E_PLT_16",              # 裁决 C：本批新增的场景码（wiring 非法）
    "V-18",                  # 裁决 C：本批新增的校验规则（引用器件必须存在）
    "V-19",                  # 裁决 C：本批新增的校验规则（source 必须小于 sources）
    "E_PLT_08",              # V-9：两个器件共用一个 id（不得改）
    "E_PLT_15",              # v0.33 的码（不得改）
    "wiring",                # 裁决 B / D：连线段的字段名
    "WiringSpec",            # 裁决 B：连线的形状
    "WiringFrom",            # 裁决 B：连线的形状
    "WiringTo",              # 裁决 B：连线的形状
    "ram_window",            # 裁决 I：「取哪段 RAM」这条规则的**唯一**落点
    "PLATFORM_RULES_VERSION",  # 裁决 F：本批的规则集版本（2 → 3）
    "PLATFORM_SCHEMA_VERSION",  # 描述形状版本（**不升**）
    "SCHEMA_VERSION",        # 契约信封版本（**不升**）
    "DEFAULT_WIRING",        # 裁决 E：默认连线的唯一落点
    "irq_line",              # 裁决 D：语义**不动**的那个字段
    "split_row",             # 裁决 H：表格切分的唯一落点
    "tools/doc_check.py",    # 裁决 H：切分口径的定义处
    "tools/fixtures/stage/",  # 裁决 H：判别力夹具
    "--print-memory",        # 裁决 I：候选侧打印内存窗口
    "base:size",             # 裁决 I：打印格式（十进制）
    "tools/stage14_check.py",  # 裁决 H：四个待收敛的脚本
    "tools/stage15_check.py",
    "tools/stage16_check.py",
    "tools/stage17_check.py",
    "tools/golden-run.sh",   # 裁决 I：参考侧
    "tools/boot-osz.sh",     # 裁决 I：参考侧
    "crates/kernel/src/bin/riscv-golden.rs",  # 裁决 I：候选侧
    "crates/kernel/src/bin/osz-boot.rs",      # 裁决 I：候选侧
    "crates/contract/src/platform.rs",        # 裁决 B / C / I 的落点
    "crates/contract/src/error.rs",           # 裁决 C：新场景码的落点
    "crates/kernel/src/kernel_host/mod.rs",   # 裁决 E：连线生效处
    "front/lib/message.js",                   # 裁决 F：镜像的唯一落点
    "front/views/components/memory-map.js",   # 裁决 G：逐字段重建
    "逐字段重建",              # 裁决 G
    "一个约束一个落点",         # 裁决 L
    "D-125",                  # 本批闭环的第一条
    "D-126",                  # 本批闭环的第二条
    "D-149",                  # 本批闭环的第三条
    "STAGE-20",               # I2C 那批顺延到的规格号
    "施工期订正",               # 施工期订正记录节
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",          # 本批**不升**契约信封版本（裁决 B）
    "PLATFORM_SCHEMA_VERSION = 2",  # 本批**不升**描述形状版本（裁决 B）
    "E_PLT_17",                    # 本批只到 `E_PLT_16` ⇒ 下一个码**不该**出现
    "V-20",                        # 本批只到 `V-19` ⇒ 下一条规则**不该**出现
    "E-PLT-16",                    # 仓内族名写法是 `E_PLT-16`（不是这个）
    "E_PLAT_16",
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
