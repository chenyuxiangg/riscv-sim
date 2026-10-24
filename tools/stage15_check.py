#!/usr/bin/env python3
"""STAGE-15 规格自检（七项，与 stage14_check.py 同构）。

用法：python3 tools/stage15_check.py docs/detailed-design/STAGE-15_V030_Board_Persistence.md

七项：
    1. Markdown 表格列数一致
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~K 无悬空引用
    5. 章节 §0~§10 完整
    6. 验收编号交叉引用无未定义项（分组 A / B / C / **T**）
    7. 关键约束字面量钉住（含已废止字面量的禁列）

⚠️ **验收分组刻意不用 D**：台账编号（D-94 / D-136 / D-140 / D-141 / D-142 /
D-143 …）与验收编号形态相同 ⇒ 本规格的工具层验收组用 **T**（tool）。
"""
import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-15_V030_Board_Persistence.md")
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

# ---- 4. 裁决编号 A~J 无悬空引用 ------------------------------------------
verdicts = set()
for line in lines:
    m = re.match(r"^\|\s*\*\*([A-K])\*\*\s*\|", line)
    if m:
        verdicts.add(m.group(1))
expected = set("ABCDEFGHIJK")
missing = expected - verdicts
if missing:
    err(f"[裁决] 裁决表缺 {sorted(missing)}")
body = "\n".join(l for l in lines if not re.match(r"^\|\s*\*\*[A-K]\*\*\s*\|", l))
for v in sorted(verdicts):
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
    "E_PLT_12",              # 本批新增的场景码
    "E_PLT_12_PARAMETER_OUT_OF_RANGE",  # 常量名
    "validate()",            # 唯一校验器
    "PlatformDescription",   # 复用已有描述 DTO
    "零新 DTO",
    "SCHEMA_VERSION",        # 契约版本（不升）
    "PLATFORM_SCHEMA_VERSION",  # 描述格式版本（不升）
    "E_PLT_08",              # 明确「不」复用它
    "m23_boardstore",        # 前端服务层落点
    "IndexedDB",             # 持久化载体
    "riscv-sim-board-store", # 独立库名
    "boardId",               # 板档案主键函数
    "isBoardIdTaken",        # 前端唯一会判的事
    "checkBoardVersion",     # 版本判读（不许静默按新版本解析）
    "irq_line",              # 跨字段规则的主角
    "sources",               # 跨字段规则的依据
    "contexts",
    "harts",
    "freq_hz",
    "baud_rate",
    "4096",                  # harts 上界
    "1024",                  # sources 上界
    "1 000 000 000 000",     # freq_hz 上界（PS_PER_SEC）
    "D-136",
    "D-140",
    "D-141",
    "front/lib/message.js",   # 描述格式版本常量的**单一落点**（裁决 K）
    "store.supported",       # 存储不可用时先判后调
    "施工期订正",              # 施工期订正记录节
]
text = "\n".join(lines)
for m in must:
    if m not in text:
        err(f"[钉住] 规格缺少关键字面量 {m}")

# 已废止 / 不存在的字面量 ⇒ 一旦出现说明规格被回退了
forbidden = [
    "E-PLT-12",       # 仓内族名写法是 `E_PLT-12`
    "E_PLAT_12",
    "ValidateMemoryMap",  # 本批**不加**这条命令（裁决 G）
    "SCHEMA_VERSION = 7",  # 本批**不升**契约版本（裁决 I）
    "BOARD_DESCRIPTION_SCHEMA_VERSION",  # 裁决 K 否掉的「另立同值常量」
]
for f in forbidden:
    if f in text:
        err(f"[禁列] 规格出现已废止字面量 {f}")

print(f"检查项：7  失败 {len(fail)}")
for f in fail:
    print("  ✗", f)
sys.exit(1 if fail else 0)
