#!/usr/bin/env python3
"""STAGE-17 规格自检（七项，与 stage16_check.py / stage15_check.py / stage14_check.py 同构）。

用法：python3 tools/stage17_check.py docs/detailed-design/STAGE-17_V032_Delta_Equivalence_Ordering_Gate.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **验收分组刻意不用 D**：台账编号（D-145 / D-146 / D-147 / D-148）与验收编号
**形态相同** ⇒ 本规格的工具层验收组用 **T**（tool）⇒ ⇒ **两套编号不会撞**。

⚠️ **本批零契约变更**（裁决 L）⇒ ⇒ 第 7 项的「禁列」必须钉住
    `SCHEMA_VERSION = 8` 与 `PLATFORM_SCHEMA_VERSION = 2` ⇒ ⇒ 一旦出现，
    说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。
"""
import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-17_V032_Delta_Equivalence_Ordering_Gate.md")
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
        cols = len([c for c in line.strip().strip("|").split("|")])
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
# ⚠️ 裁决写成**标题**（`### 裁决 X · ...`）⇒ 一条裁决一个标题 ⇒ 标题即落点。
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
    if not (re.search(rf"\*\*{v}\*\*", body)
            or re.search(rf"裁决[^。\n|]{{0,12}}?{v}\b", body)):
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
# ⚠️ 本规格的验收分组是 A / B / C / T —— **没有 D 组** ⇒ 全部 `D-\d+` 都是台账编号。
ids = set(re.findall(r"\b([ABCT]-\d+)\b", sec5_text))
defined = set(re.findall(r"^\|\s*\*?\*?([ABCT]-\d+)\*?\*?\s*\|", sec5_text, re.M))
undef = ids - defined
if undef:
    err(f"[验收] §5 内出现未定义的验收编号 {sorted(undef)}")
for grp in "ABCT":
    got = sorted(x for x in defined if x.startswith(grp + "-"))
    if not got:
        err(f"[验收] §5 缺 {grp} 组验收")
    if got:
        nums_g = sorted(int(x.split("-")[1]) for x in got)
        if nums_g != list(range(1, len(nums_g) + 1)):
            err(f"[验收] {grp} 组编号不连续：{got}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    "compose_memory_map",     # 裁决 A：宿主内部的纯函数（落点）
    "PlatformDescription",    # 复用的描述 DTO
    "MemoryMapOp",            # 差量操作枚举
    "SetMemoryMap",           # 整份路径（等价性的另一半）
    "PatchMemoryMap",         # 差量路径
    "UnloadProject",          # 裁决 G：同号事件照收的来源
    "E_PLT_06",               # 非法操作（重叠）
    "E_PLT_07",               # 整份路径也要拒的那一类
    "E_PLT_14",               # 定位不到（不静默忽略）
    "validate()",             # 唯一校验器（仍留在调用点）
    "map_revision",           # 宿主侧的修订号计数器（单调）
    "revision",               # 事件带的修订号（乱序判据）
    "expected_revision",      # 裁决 H：提交路径照常兜底
    "user_board",             # 等价性用例的共同基座
    "wireMemoryMap",          # 裁决 F：护栏的唯一落点
    "严格更旧",                # 裁决 F/G：判据是「严格更旧才挡」
    "逐字段相同",              # 裁决 B：等价性的判据
    "awaitEval",              # 裁决 K：统一探测 helper
    "awaitPromise: true",     # 裁决 K：helper 内部语义
    "registerSection",        # 裁决 I：注册表即聚合源
    "SCHEMA_VERSION",         # 契约信封版本（不升）
    "PLATFORM_SCHEMA_VERSION",  # 描述格式版本（不升）
    'data-action="unload-project"',  # v0.31 冻结的 DOM 钩子（本批不新增）
    "data-mmap-revision",     # v0.31 冻结的 DOM 钩子（本批不新增）
    "crates/kernel/src/kernel_host/mod.rs",  # 宿主落点
    "front/services/m07_transport.js",       # 前端前提（Worker 只建一次）
    "tools/browser-verify.mjs",              # 门禁落点
    "D-145",                  # 本批闭环的第一条
    "D-146",                  # 本批闭环的第二条
    "D-147",                  # 本批闭环的第三条
    "D-148",                  # 本批新登记
    "STAGE-18",               # I2C 那批顺延到的规格号
    "施工期订正",               # 施工期订正记录节
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",       # 本批**不升**契约版本（裁决 L）
    "PLATFORM_SCHEMA_VERSION = 2",  # 本批**不升**描述格式版本（裁决 L）
    "E_PLT_15",                 # 本批**不新增**场景码（§0.3 / 裁决 L）
    "E_PLT_16",                 # 同上
    "E-PLT-15",                 # 仓内族名写法是 `E_PLT-15`（不是这个）
    "E_PLAT_15",
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
