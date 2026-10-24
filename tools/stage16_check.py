#!/usr/bin/env python3
"""STAGE-16 规格自检（七项，与 stage15_check.py / stage14_check.py 同构）。

用法：python3 tools/stage16_check.py docs/detailed-design/STAGE-16_V031_Board_Delta_Unload_Budget.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §0~§11 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / **T**）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **验收分组刻意不用 D**：台账编号（D-45 / D-137 / D-139 / D-145 …）与验收编号
**形态相同** ⇒ 本规格的工具层验收组用 **T**（tool）⇒ ⇒ **两套编号不会撞**。
"""
import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-16_V031_Board_Delta_Unload_Budget.md")
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
# ⚠️ 裁决写成**标题**（`### 裁决 X · ...`）⇒ 不是表格行 ⇒ 与 stage15 的
#    表格写法不同但**语义一样**（一条裁决一个标题 ⇒ 标题即落点）。
verdicts = set()
for line in lines:
    m = re.match(r"^###\s*裁决\s*([A-N](?:2)?)\s*·", line)
    if m:
        verdicts.add(m.group(1))
expected = set("ABCDEFGHIJKLMN") | {"F2"}
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
    "E_PLT_13",                    # 本批新增的场景码（修订号冲突）
    "E_PLT_14",                    # 本批新增的场景码（段找不到）
    "E_PLT_13_REVISION_CONFLICT",  # 常量名
    "E_PLT_14_REGION_NOT_FOUND",   # 常量名
    "max_region_bytes",            # D-138 拆出来的字段
    "MemoryMapOp",                 # 差量操作枚举
    "PatchMemoryMap",              # 差量命令
    "UnloadProject",               # D-139 的卸载命令
    "expected_revision",           # 修订号检查
    "revision",                    # 事件带的修订号
    "loaded",                      # 事件带的「有没有装载镜像」
    "map_revision",                # 宿主侧的修订号计数器
    "validate()",                  # 唯一校验器
    "PlatformDescription",         # 复用已有描述 DTO
    "RegionSpec",                  # 差量定位的键
    "base",                        # 按 base 定位（与顺序无关）
    "外部标签",                      # serde 的嵌套限制（裁决 H）
    "SCHEMA_VERSION",              # 契约版本（升到 7）
    "PLATFORM_SCHEMA_VERSION",     # 描述格式版本（不升）
    "E_PLT_07",                    # 声明期护栏（不能删）
    "E_BUS_09",                    # 运行期护栏（不合并）
    "set_max_region_bytes",        # 宿主那一行
    "region_budget_bytes",         # 声明期预算
    "image_loaded",                # 卸载要清的那一位
    "user_board",                  # 用户的板（卸载**不**清）
    "topics.clear",                # 卸载必然清订阅
    "resubscribeAllStreams",       # 卸载路径复用的重订助手
    "front/lib/message.js",        # 契约镜像（唯一真相）
    "D-137",
    "D-138",
    "D-139",
    "D-145",                       # 本批新登记的第一条
    "D-146",                       # 本批新登记的第二条
    "data-mmap-revision",           # 冻结的 DOM 钩子（修订号）
    "data-action=\"unload-project\"",  # 冻结的 DOM 钩子（卸载按钮）
    "data-mmap-budget-note",        # 既有钩子（措辞要改）
    "STAGE-17",                    # I2C 那批的规格号让位
    "施工期订正",                   # 施工期订正记录节
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    'rename = "declared_size_budget_bytes"',   # 改名留给下一批（裁决 F2）
    "E-PLT-13",       # 仓内族名写法是 `E_PLT-13`
    "E_PLAT_13",
    "E_PLT_15",       # 本批只加到 14
    "SCHEMA_VERSION = 8",   # 本批只升到 7
    "PLATFORM_SCHEMA_VERSION = 2",   # 描述格式版本**不升**（裁决 E）
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
