#!/usr/bin/env python3
"""STAGE-14 规格自检（七项，与 stage13_check.py 同构）。

用法：python3 tools/stage14_check.py docs/detailed-design/STAGE-14_V029_Board_Memory_Map.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住（含已废止字面量的禁列）
"""
import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-14_V029_Board_Memory_Map.md")
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
verdicts = set()
for line in lines:
    m = re.match(r"^\|\s*\*\*([A-N])\*\*\s*\|", line)
    if m:
        verdicts.add(m.group(1))
expected = set("ABCDEFGHIJKLMN")
missing = expected - verdicts
if missing:
    err(f"[裁决] 裁决表缺 {sorted(missing)}")
body = "\n".join(l for l in lines if not re.match(r"^\|\s*\*\*[A-N]\*\*\s*\|", l))
for v in sorted(verdicts):
    # 正文里三种写法都算引用：**F**、「裁决 F」、「裁决 I 与 L」
    if not (re.search(rf"\*\*{v}\*\*", body)
            or re.search(rf"裁决[^。\n|]{{0,8}}?{v}\b", body)):
        err(f"[裁决] 裁决 {v} 在正文无引用（悬空）")

# ---- 5. 章节完整 §0~§10 ---------------------------------------------------
heads = [l for l in lines if re.match(r"^##\s+§\d+", l)]
nums = sorted(int(re.search(r"§(\d+)", h).group(1)) for h in heads)
if nums != list(range(0, 11)):
    err(f"[章节] §0~§10 不完整，实得 {nums}")

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
# 台账编号（D-124 / D-136 / D-9 等）与验收编号（A-1 / B-2 / C-3）形态相同 ⇒ 排除。
# ⚠️ 本规格 §5 **没有 D 组验收**（只有 A / B / C 三组）⇒ 全部 `D-\d+` 都是台账编号。
ids = set(re.findall(r"\b([ABCD]-\d+)\b", sec5_text)) - set(re.findall(r"\b(D-\d+)\b", sec5_text))
defined = set(re.findall(r"^\|\s*([ABCD]-\d+)\s*\|", sec5_text, re.M))
undef = ids - defined
if undef:
    err(f"[验收] §5 内出现未定义的验收编号 {sorted(undef)}")
for grp in "ABC":
    got = sorted(x for x in defined if x.startswith(grp + "-"))
    if not got:
        err(f"[验收] §5 缺 {grp} 组验收")
    # 每组编号必须是连续的 1..N
    if got:
        nums_g = sorted(int(x.split("-")[1]) for x in got)
        if nums_g != list(range(1, len(nums_g) + 1)):
            err(f"[验收] {grp} 组编号不连续：{got}")

# ---- 7. 关键约束字面量钉住（防规格被悄悄改软） --------------------------
must = [
    "SetMemoryMap",              # 命令名
    "EventPayload::MemoryMap",   # 事件变体
    "memory.map",                # topic
    "PlatformDescription",       # 零新 DTO：复用已有描述 DTO
    "零新 DTO",
    "E_PLT-07",                  # 校验用的族 + 码
    "region_budget_bytes",       # 预算与 size 的耦合
    "image_loaded",              # 「已装载」判定
    "pending_platform",          # 待生效配置字段
    "rebuild_platform",          # 立即生效的落点
    "choose_platform",           # 自动选档（B-5 钉「逐字不变」）
    "SCHEMA_VERSION",            # 契约升版
    "pending: bool",             # 事件载荷形状
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "E-PLT-07",   # 仓内族名写法是 `E_PLT-07`
    "E_CTX-05",   # 契约层 CTX 族没有具名场景码 05
    "E_PLAT-07",
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
