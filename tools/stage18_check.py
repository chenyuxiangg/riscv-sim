#!/usr/bin/env python3
"""STAGE-18 规格自检（七项，与 stage17_check.py / stage16_check.py / stage15_check.py / stage14_check.py 同构）。

用法：python3 tools/stage18_check.py docs/detailed-design/STAGE-18_V033_Device_Identity_Board_Rules_Ledger_Gate.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~O 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / T / X）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **验收分组刻意不用 D**：台账编号（D-142 / D-143 / D-144 / D-148）与验收编号
    **形态相同** ⇒ 本规格的工具层验收组用 **T**（tool）、跨层组用 **X**（cross）
    ⇒ ⇒ **两套编号不会撞**。

⚠️ **本批契约层零升版**（裁决 O）⇒ ⇒ 第 7 项的「禁列」必须钉住
    `SCHEMA_VERSION = 8` 与 `PLATFORM_SCHEMA_VERSION = 2` ⇒ ⇒ 一旦出现，
    说明有人偷偷给契约层升了版 ⇒ ⇒ **规格与实现就此脱节**。

⚠️ **本批新增** `E_PLT_15`（器件 `id` 为空）与 **V-17**（那条规则）⇒ ⇒ 它们
    在「必列」里，而**下一个**码 `E_PLT_16` 与**下一条**规则 `V-18` 在「禁列」里。
"""
import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-18_V033_Device_Identity_Board_Rules_Ledger_Gate.md")
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

# ---- 4. 裁决编号 A~O 无悬空引用 ------------------------------------------
# ⚠️ 裁决写成**标题**（`### 裁决 X · ...`）⇒ 一条裁决一个标题 ⇒ 标题即落点。
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
    # ⚠️ **只认「裁决 X」这种带前缀的引用**。
    # ⚠️ **不要**加 `re.search(rf"\*\*{v}\*\*", body)` 那个粗匹配 —— 规格正文里
    #    到处是「生效图 **A**」这种**单字母加粗** ⇒ ⇒ 粗匹配会把**裁决 A / B / C**
    #    的悬空检查**全部变成假绿**（实测：删掉「裁决 A」的唯一引用后仍报 0 失败）。
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
# ⚠️ 本规格的验收分组是 A / B / C / T / X —— **没有 D 组** ⇒ 全部 `D-\d+` 都是台账编号。
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
    "E_PLT_15",               # 裁决 B：本批新增的场景码（器件 id 为空）
    "V-17",                   # 裁决 A：本批新增的校验规则
    "E_PLT_08",               # V-9：两个器件共用一个 id（裁决 C：不得改）
    "E_PLT_02",               # V-2：描述的 id 为空（裁决 C：不得改）
    "is_empty()",             # 裁决 D：不 trim
    "PLATFORM_RULES_VERSION",  # 裁决 E：本批新增的合法性规则集版本
    "PLATFORM_SCHEMA_VERSION",  # 描述形状版本（**不升**）
    "SCHEMA_VERSION",         # 契约信封版本（**不升**）
    "rules_version",          # 裁决 F：规则版本落在条目容器里
    "boardRulesVersion",      # 裁决 G：老条目隐含版本 1
    "checkBoardRules",        # 裁决 H：两个 reason 不共用
    "checkBoardVersion",      # 裁决 H：唯一现行护栏（不能删）
    'reason:"rules"',         # 裁决 H：规则口径的 reason
    "validate()",             # 裁决 A：唯一校验器
    "diffMemoryMapOps",       # 裁决 K：真前端差量算法
    "planMemoryMapUpdate",    # 裁决 K：真前端选路
    "UnloadProject",          # 裁决 L：下沉那段的前置（先卸载才立即生效）
    "UpsertRegion",           # 裁决 L：差量操作的三种形态之一
    "RemoveRegion",           # 裁决 L：差量操作的三种形态之一
    "逐字段相同",              # 裁决 L：等价性的判据
    "tools/wasm-smoke.mjs",   # 裁决 K：D-148 的落点
    "tools/ledger_check.py",  # 裁决 N：D-144 的落点
    "crates/contract/src/platform.rs",   # 裁决 A / E 的落点
    "crates/contract/src/error.rs",      # 裁决 B 的落点
    "front/lib/message.js",              # 裁决 E：镜像的唯一落点
    "front/services/m23_boardstore.js",  # 裁决 F / G / H 的落点
    "front/app.js",                      # 裁决 I / J 的落点
    "front/views/components/memory-map.js",  # 裁决 K：真前端算法
    "D-142",                  # 本批闭环的第一条
    "D-143",                  # 本批闭环的第二条
    "D-144",                  # 本批闭环的第三条
    "D-148",                  # 本批闭环的第四条
    "STAGE-19",               # I2C 那批顺延到的规格号
    "施工期订正",               # 施工期订正记录节
    "不 trim",                 # 裁决 D 的字面口径
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "SCHEMA_VERSION = 8",        # 本批**不升**契约信封版本（裁决 O）
    "PLATFORM_SCHEMA_VERSION = 2",  # 本批**不升**描述形状版本（裁决 O）
    "E_PLT_16",                  # 本批只到 `E_PLT_15` ⇒ 下一个码**不该**出现
    "V-18",                      # 本批只到 `V-17` ⇒ 下一条规则**不该**出现
    "E-PLT-15",                  # 仓内族名写法是 `E_PLT-15`（不是这个）
    "E_PLAT_15",
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
