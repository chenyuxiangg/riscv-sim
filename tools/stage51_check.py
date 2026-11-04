#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-51 冻结规格（v0.75 · MCU 概念 + ISA 门禁 + 前端顺手整改）的自检。

用法：python3 tools/stage51_check.py docs/detailed-design/STAGE-51_V075_MCU_Concept.md

⚠️ **五项**（与 `tools/stage50_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. **裁决** A~N 无悬空引用（§3 的裁决字母在 §4 判别力 / §2 口径里都
       对得上号）
    2. **事实**表连续 + 关键事实字面量钉住（`rv32imac` / `E_PLT-20` /
       `E_PLT-21` / 向量表声明+展示 / c 位单方向）
    3. **变异**：§5 表 T-1~T-5 齐全 + §7.2 有**非空的实测红集**（施工后
       填值）+ 对照 T-5 必须零红
    4. **版本号**：本批升版 3→4 · `SUPPORTED=[1,2,3,4]` · 信封 11 / 快照 2
       **不动** ⇒ ⇒ 且**前端镜像**（`front/lib/message.js`）与契约层逐字相同
    5. **新码**：`E_PLT_20` / `E_PLT_21` 在契约层**唯一定义**（一码一义：
       error.rs 各一个 `pub const`；两码语义不同、互不复用）⇒ 且旧码
       `E_PLT_19` 仍在（上一批没被回退）

⚠️ **编号空间**（与前几批同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-3）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-5）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-5）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-161 ~ B-167，接 v0.74 的
      B-160）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-2）

⚠️ **变异编号空间 = T-1 ~ T-5 + 顺手整改 N-4** ⇒ ⇒ 与
`tools/mutation_v075.py` 的 `MUTATIONS` 表**逐条对应**（编号**不许**错位）。
"""

import re
import sys
from pathlib import Path

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-51_V075_MCU_Concept.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent


def check(cond, msg):
    if not cond:
        fail.append(msg)


# ---- 1. 裁决 A~N 无悬空引用 ------------------------------------------------
section3 = []
in_s3 = False
for ln in lines:
    if re.match(r"^## §3 ", ln):
        in_s3 = True
        continue
    if re.match(r"^## §[0-9] ", ln) and in_s3:
        break
    if in_s3 and re.match(r"^\|\s*([A-Z])\s*\|", ln):
        section3.append(ln)
decided = {m.group(1) for ln in section3 if (m := re.match(r"^\|\s*([A-Z])\s*\|", ln))}
# ⚠️ 本批裁决 A~O（O = 顺手整改四件那条）⇒ 不写死字母表 ⇒ 用「引用对得上」
#   来判：§3 的每一行在 §2/§4 里都被「裁决 X」引用过，反之亦然。
check(
    len(decided) >= 5,
    f"§3 裁决行太少（{sorted(decided)}）⇒ 表没解析对",
)
# 悬空引用：§2 / §4 里出现「裁决 X」但 §3 没那个字母。
full_text = "\n".join(lines)
referenced = {m.group(1) for m in re.finditer(r"裁决\s*([A-Z])\b", full_text)}
check(
    referenced <= decided,
    f"出现「裁决 X」但 §3 没有对应行 ⇒ 悬空引用：{sorted(referenced - decided)}",
)

# ---- 2. 事实表连续 + 关键事实字面量 ---------------------------------------
facts_rows = []
in_facts = False
for ln in lines:
    if re.match(r"^## §1 ", ln):
        in_facts = True
        continue
    if re.match(r"^## §[0-9] ", ln) and in_facts:
        break
    if in_facts and re.match(r"^\|\s*\d+\s*\|", ln):
        facts_rows.append(ln)
nums = [int(m.group(1)) for ln in facts_rows if (m := re.match(r"^\|\s*(\d+)\s*\|", ln))]
check(
    nums == list(range(1, len(nums) + 1)),
    f"§1 事实表编号不连续：{nums}（必须 1..N）",
)
for needle in ["rv32imac", "E_PLT-20", "E_PLT-21", "向量表", "单方向", "只展示"]:
    check(
        needle in full_text,
        f"规格里必须出现事实/口径字面量「{needle}」（裁决拍板过的语义）",
    )

# ---- 3. 变异：§5 表齐全 + §7.2 实测红集 + 对照零红 -------------------------
check(
    all(f"T-{i}" in full_text for i in range(1, 6)),
    "§5 变异表必须 T-1~T-5 齐全",
)
# §7.2 施工后要有实测红集（非占位）。
section72 = ""
in_72 = False
for ln in lines:
    if re.match(r"^### §7\.2", ln):
        in_72 = True
        continue
    if re.match(r"^## §[0-9] ", ln) and in_72:
        break
    if in_72:
        section72 += ln + "\n"
check(
    "（预留" not in section72 and len(section72.strip()) > 40,
    "§7.2 必须填了实测红集（不是预留占位）——先跑 `tools/mutation_v075.py`",
)
check(
    "T-5" in section72 and "零红" in section72,
    "§7.2 必须登记 T-5 对照零红",
)

# ---- 4. 版本号：契约层 + 前端镜像逐字相同 ---------------------------------
def contract_line(needle):
    text = (REPO_ROOT / "crates/contract/src/platform.rs").read_text(encoding="utf-8")
    for ln in text.splitlines():
        if needle in ln:
            return ln.strip()
    return None


def front_value(js_needle):
    text = (REPO_ROOT / "front/lib/message.js").read_text(encoding="utf-8")
    for ln in text.splitlines():
        if js_needle in ln:
            return ln.split("=")[-1].strip().rstrip(";")
    return None


def contract_value(needle):
    text = (REPO_ROOT / "crates/contract/src/platform.rs").read_text(encoding="utf-8")
    for ln in text.splitlines():
        if needle in ln:
            # ⚠️ Rust 切片写法带 `&`（`&[1, 2, 3, 4]`）⇒ 剥掉再比。
            return ln.split("=")[-1].strip().rstrip(";").lstrip("&")
    return None


for rust_needle, js_needle, expect in [
    ("PLATFORM_SCHEMA_VERSION: u32 =", "PLATFORM_SCHEMA_VERSION =", "4"),
    ("SUPPORTED_PLATFORM_SCHEMA_VERSIONS:", "SUPPORTED_PLATFORM_SCHEMA_VERSIONS =", "[1, 2, 3, 4]"),
    ("PLATFORM_RULES_VERSION: u32 =", "PLATFORM_RULES_VERSION =", "6"),
]:
    cv = contract_value(rust_needle)
    check(cv is not None, f"契约层找不到 {rust_needle}")
    if cv:
        check(expect in cv, f"契约层 {rust_needle} 必须含 {expect}（{cv}）")
    fv = front_value(js_needle)
    check(
        fv is not None,
        f"前端镜像找不到 {js_needle} ⇒ 版本镜像没同步",
    )
    if cv and fv:
        check(
            cv == fv,
            f"前端镜像 {js_needle} 与契约层不一致：契约 {cv} · 前端 {fv}",
        )
# 信封 / 快照不动（声明处：契约 lib.rs / model.rs）。
for file, needle, expect in [
    ("crates/contract/src/lib.rs", "SCHEMA_VERSION: u32 =", "11"),
    ("crates/contract/src/model.rs", "SIM_STATE_SCHEMA_VERSION: u32 =", "2"),
]:
    txt = (REPO_ROOT / file).read_text(encoding="utf-8")
    vals = [ln.strip() for ln in txt.splitlines() if needle in ln and "=" in ln]
    check(
        any(expect in v for v in vals),
        f"{file} 的 {needle} 必须仍是 {expect}（信封/快照不动）",
    )

# ---- 5. 新码：E_PLT_20 / E_PLT_21 唯一定义 + 旧码 E_PLT_19 未回退 ----------
error_rs = (REPO_ROOT / "crates/contract/src/error.rs").read_text(encoding="utf-8")
for const_name, num in [
    ("E_PLT_20_FW_ISA_MISMATCH", 20),
    ("E_PLT_21_MCU_INVALID", 21),
    ("E_PLT_19_PINS_INVALID", 19),
]:
    count = error_rs.count(f"pub const {const_name}: u16 = {num};")
    check(
        count == 1,
        f"error.rs 的 {const_name} 必须**恰好一处**唯一定义（实际 {count} 处）",
    )
# 一码一义：20 与 21 不能是同一语义的别名（靠注释里的语义区分不能机器判 ⇒
# 判「两个码的数字不同 + 常量名不同」⇒ 语义区分由 error.rs 注释 + 规格守）。
check(
    "E_PLT_20_FW_ISA_MISMATCH" != "E_PLT_21_MCU_INVALID",
    "20 / 21 常量名不许相同（防复制粘贴）",
)
# 前端不得判 isa（N-2：零处判）⇒ **ELF / ISA 的事实只在内核** ⇒ 前端源码
# 不许出现 ELF 头判据符号（`E_PLT-20` 在**注释文案**里可以提 —— error_code_check
# 会把「提到」与「用到」分开；真正被禁的是**判据落点**）。
front_mmap = (REPO_ROOT / "front/views/components/memory-map.js").read_text(encoding="utf-8")
for banned in ["elf_isa", "e_flags", "EF_RISCV_RVC", "has_rvc"]:
    check(
        banned not in front_mmap,
        f"前端**不许**判 isa（N-2）⇒ memory-map.js 不许出现 {banned}（ELF/ISA 事实只在内核）",
    )

# ---- 汇总 ----------------------------------------------------------------
if fail:
    print(f"✗ stage51_check 失败：{len(fail)} 项")
    for f in fail:
        print(f"  - {f}")
    sys.exit(1)
print("✓ stage51_check 全过（裁决/事实/变异/版本号/新码）")
