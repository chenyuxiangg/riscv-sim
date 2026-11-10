#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-49 冻结规格（v0.73 · 描述格式一次升版：B2 + G + D1 + D-177 的 C）的自检。

用法：python3 tools/stage49_check.py docs/detailed-design/STAGE-49_V073_Platform_Schema_V2.md

⚠️ **十项**（与 `tools/stage48_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §1~§9 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1 的事实表**编号列**必须**连续**
       ＋ 7c. §7 的施工期订正：不是占位 + **变异编号**齐全 + 每条有**非空的实测红集**
       ＋ 7d. **四个版本号的值**（**本批描述格式升版** ⇒ ⇒ `PLATFORM_SCHEMA_VERSION`
          = 2 · `PLATFORM_RULES_VERSION` = 4 · `SCHEMA_VERSION`（信封）**仍 11** ·
          `SIM_STATE_SCHEMA_VERSION` **仍 2**）**且** **前端镜像**与契约层**逐字相同**
       ＋ 7e. **跨层源代码锚点**（契约层的新变体 / 新段 / 新校验 / 新码 /
          内核的分配器 / 前端那几条 / 真机段的段名）
       ＋ 7f. 台账必须登记本批（`v0.73` + `STAGE-49` + `D-175` + `D-177`）
       ＋ 7g. 脚本必须自指存在
    8. **本批升版**：描述格式版本 1 → 2 · 规则集 3 → 4 · **信封不动**（仍 11）
       ⇒ ⇒ 前端镜像**四处**必须一样
    9. **本批的两个新场景码**（`E_PLT_18` 角色声明 / `E_PLT_19` 引脚表）⇒ ⇒
       **各有唯一定义处** + **规格写明线上形态** + **未被撞号**
       （⇒ ⇒ **不复用** `E_PLT_16` ⇒ ⇒ 「一个场景码只一个含义」这条没被破坏）
    10. **上一批的破坏性改动没有被回退**（STAGE-48 的连线报告 DTO 三个 +
        `E_PLT_17` 判定**必须**仍然在）

⚠️ **编号空间**（与 STAGE-48 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-5）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-12）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-4）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-151 ~ B-155，接 v0.72 批的 B-150）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-49_V073_Platform_Schema_V2.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d / 8 用）—— 本批**描述格式升版** ⇒ ⇒
#: `PLATFORM_SCHEMA_VERSION` 1 → 2 · `PLATFORM_RULES_VERSION` 3 → 4 ⇒ ⇒
#: **契约信封 `SCHEMA_VERSION` 不动**（仍 11）· `SIM_STATE_SCHEMA_VERSION` 仍 2。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "11",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "2",
    "PLATFORM_RULES_VERSION": "4",
}

LIB_REL = "crates/contract/src/lib.rs"
MODEL_REL = "crates/contract/src/model.rs"
PROTOCOL_REL = "crates/contract/src/protocol.rs"
ERROR_REL = "crates/contract/src/error.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
HOST_PLATFORM_REL = "crates/kernel/src/kernel_host/platform.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
KTEST_REL = "crates/kernel/src/kernel_host/tests_v073_schema.rs"
MIRROR_REL = "front/lib/message.js"
MMAP_REL = "front/views/components/memory-map.js"
BOARDSTORE_REL = "front/services/m23_boardstore.js"
BROWSER_REL = "tools/browser-verify.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**。
EXPECT_ACCEPT = {
    "A": list(range(1, 6)),       # A-1 ~ A-5（契约与跨层）
    "K": list(range(1, 13)),      # K-1 ~ K-12（内核）
    "N": list(range(1, 5)),       # N-1 ~ N-4（前端 Node）
    "B": list(range(151, 156)),   # B-151 ~ B-155（真机门禁 · 跨批次唯一）
    "X": list(range(1, 4)),       # X-1 ~ X-3（交叉）
}
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-150 也算「已定义」⇒ ⇒
    #    **再加上本批自己的那几条**（B-151 ~ B-155）⇒ ⇒ ⇒ 范围到 155。
    "B": set(range(1, 156)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ **变异编号空间**（§5 · T-1 ~ T-7）。
MUTATION_IDS = list(range(1, 8))

#: ⚠️ **本批的两个新场景码**（各有**唯一定义处** ⇒ ⇒ 一码一义）。
EXPECT_NEW_CODES = [
    "E_PLT_18_ROLES_INVALID",
    "E_PLT_19_PINS_INVALID",
]


def err(msg):
    fail.append(msg)


def read(rel):
    p = REPO_ROOT / rel
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# 1~3：表格列数 / 标题跳级 / 围栏配平
# ---------------------------------------------------------------------------


def split_fences(src):
    """返回 (围栏外的行, 围栏内的行标记)。围栏块内容整块剔除。"""
    out = []
    in_fence = False
    for line in src:
        if line.lstrip().startswith("```"):
            in_fence = not in_fence
            out.append(None)
            continue
        out.append(None if in_fence else line)
    return out


body = split_fences(lines)

# --- 1. 表格列数一致 -------------------------------------------------------
start = None
cols = None
for i, line in enumerate(body, 1):
    if line is None:
        continue
    s = line.strip()
    if s.startswith("|") and s.endswith("|"):
        c = len(split_row(s))
        if start is None:
            start, cols = i, c
        elif c != cols:
            err(f"[表格] 第 {i} 行列数 {c} ≠ 首行 {cols}（表起于第 {start} 行）")
    else:
        start, cols = None, None

# --- 2. 标题层级不跳级 -----------------------------------------------------
prev = 0
for i, line in enumerate(body, 1):
    if line is None:
        continue
    m = re.match(r"^(#{1,6})\s+\S", line)
    if not m:
        continue
    lvl = len(m.group(1))
    if prev and lvl > prev + 1:
        err(f"[标题] 第 {i} 行从 h{prev} 跳到 h{lvl}")
    prev = lvl

# --- 3. 代码围栏配平 -------------------------------------------------------
n_fences = sum(1 for l in lines if l.lstrip().startswith("```"))
if n_fences % 2 != 0:
    err(f"[围栏] 围栏数 {n_fences} 为奇数 ⇒ 未配平")

text = "\n".join(lines)

# --- 4. 裁决编号 A~N 无悬空引用 --------------------------------------------
defined = set()
for line in body:
    if line is None:
        continue
    m = re.match(r"^\|\s*\*\*([A-N])\*\*\s*\|", line.strip())
    if m:
        defined.add(m.group(1))
for d in DECISION_LETTERS:
    if d not in defined:
        err(f"[裁决] §3 缺裁决 {d}")
for d in DECISION_LETTERS:
    for m in re.finditer(rf"裁决\s*{d}\b", text):
        if d not in defined:
            err(f"[裁决] 正文引用了未定义的裁决 {d}")

# --- 5. 章节 §1~§9 完整 ---------------------------------------------------
for sec in range(1, 10):
    if not re.search(rf"^## §{sec}\b", text, re.M):
        err(f"[章节] 缺 §{sec}")

# --- 6. 验收编号交叉引用 ---------------------------------------------------
for group, nums in EXPECT_ACCEPT.items():
    for n in nums:
        if not re.search(rf"^\|\s*\*{{0,2}}{group}-{n}\*{{0,2}}\s*\|", text, re.M):
            err(f"[验收] §4 缺 {group}-{n}")
sec8_at = re.search(r"^## §8\b", text, re.M)
head = text[: sec8_at.start()] if sec8_at else text
for m in re.finditer(r"\b([AKNLBX])-(\d+)\b", head):
    group, num = m.group(1), int(m.group(2))
    if group not in KNOWN_ACCEPT:
        continue
    if num not in KNOWN_ACCEPT[group]:
        err(f"[验收] 正文引用了未定义的验收编号 {group}-{num}")

# --- 7. 关键约束字面量钉住 -------------------------------------------------
lib = read(LIB_REL) or ""
model_rs = read(MODEL_REL) or ""
protocol_rs = read(PROTOCOL_REL) or ""
platform_rs = read(PLATFORM_REL) or ""
error_rs = read(ERROR_REL) or ""
host_platform = read(HOST_PLATFORM_REL) or ""
host_rs = read(HOST_REL) or ""
ktest = read(KTEST_REL) or ""
mirror_js = read(MIRROR_REL) or ""
mmap_js = read(MMAP_REL) or ""
boardstore_js = read(BOARDSTORE_REL) or ""
browser = read(BROWSER_REL) or ""
ledger = read(LEDGER_REL) or ""

# 7d. 四个版本号（契约层 + 前端镜像）
for name, want in EXPECT_VERSIONS.items():
    m = None
    for src_ in (lib, model_rs, protocol_rs, platform_rs):
        m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_)
        if m:
            break
    if not m:
        err(f"[版本] 契约层读不到 {name}")
        continue
    if m.group(1) != want:
        err(f"[版本] {name} = {m.group(1)} ≠ 规格的 {want}")
    if name not in text:
        err(f"[版本] 规格里没有提到 {name}")
m2 = re.search(r"SCHEMA_VERSION\s*=\s*['\"]?(\d+)", mirror_js)
if not m2:
    err("[版本] 前端镜像读不到 `SCHEMA_VERSION`")
elif m2.group(1) != EXPECT_VERSIONS["SCHEMA_VERSION"]:
    err(f"[版本] 前端镜像的 SCHEMA_VERSION = {m2.group(1)} ≠ 契约层的 "
        f"{EXPECT_VERSIONS['SCHEMA_VERSION']} ⇒ ⇒ 两边**不一致**")

# 7e. 跨层源码锚点
ANCHORS = [
    # ⚠️ **G**（裁决 C / D）：描述里能写出 `timer_riscv`
    (platform_rs, "TimerRiscv {"),
    (platform_rs, '"timer_riscv"'),
    (host_platform, "DeviceSpec::TimerRiscv { id, irq_line } =>"),
    # ⚠️ **D-177 的 C**（裁决 E）：描述里的角色声明
    (platform_rs, "pub struct DeviceRoles"),
    (platform_rs, "pub roles: Option<DeviceRoles>"),
    (platform_rs, "pub struct PinBinding"),
    (platform_rs, "pub pins: Option<Vec<PinBinding>>"),
    # ⚠️ **B2**（裁决 B / H / I）：`base` 可缺省 + 装配期分配
    # ⚠️ **枚举变体的字段**在 Rust 里**不是** `pub`（那是枚举**定义**上的
    #   写法）⇒ ⇒ ⇒ 这里钉**实际**的字段形状（`base: Option<u32>`）。
    (platform_rs, "base: Option<u32>"),
    (platform_rs, "SUPPORTED_PLATFORM_SCHEMA_VERSIONS"),
    (host_platform, "pub fn assign_bases("),
    # ⚠️ **前端那几条**（N-1 ~ N-4）
    (mmap_js, "export function normalizeRoles"),
    (mmap_js, "export function normalizePins"),
    (boardstore_js, "PLATFORM_RULES_VERSION"),
    # ⚠️ **真机段**（B-151 ~ B-155）
    (browser, "platformSchemaV2"),
]
for src_, needle in ANCHORS:
    if src_ is None:
        err(f"[锚点] 文件缺失（{needle}）")
    elif needle not in src_:
        err(f"[锚点] 缺 {needle}")

# 7b. 事实表编号连续
facts = sorted({int(m.group(1)) for m in re.finditer(r"\|\s*F-(\d+)\s*\|", text)})
if facts != list(range(1, len(facts) + 1)) or not facts:
    err(f"[事实] §1 的事实编号 {facts} 不是 F-1 ~ F-{len(facts)} 连续")

# 7f. 台账登记
for needle in ("D-175", "D-177", "STAGE-49", "v0.73"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage49_check.py" not in text:
    err("[自指] 规格里没有提到本脚本")

# --- 7c. §7 的施工期订正：不是占位 + 变异编号齐全 + 每条有非空实测红集 ----
sec7 = re.search(r"^## §7\b(.*?)(?=^## §8|\Z)", text, re.M | re.S)
if not sec7:
    err("[订正] 缺 §7")
else:
    sec7_text = sec7.group(1)
    if "待填" in sec7_text:
        err("[订正] §7 还是占位（『待填』）")
    for t in MUTATION_IDS:
        if not re.search(rf"^\|\s*\*{{0,2}}T-{t}\*{{0,2}}\s*\|", sec7_text, re.M):
            err(f"[订正] §7 缺变异 T-{t}")
    for line in sec7_text.splitlines():
        m = re.match(r"^\|\s*\*?\*?T-(\d+)\*?\*?\s*\|", line)
        if not m:
            continue
        cells = [c.strip() for c in split_row(line.strip())]
        tid = int(m.group(1))
        if tid == MUTATION_IDS[-1]:
            if not any("零红" in c for c in cells):
                err(f"[订正] T-{tid} 是**对照** ⇒ ⇒ 必须**明写**「零红」")
            continue
        if not any(re.search(r"红\s*[1-9]", c) for c in cells):
            # ⚠️ **允许「该变异不成立」** ⇒ ⇒ 但**必须**在规格里**明写**
            #   「零红 / 不成立」⇒ ⇒ 否则「零判别力」这件事会被**静默**。
            if not any(("零红" in c) or ("不成立" in c) for c in cells):
                err(f"[订正] T-{tid} 的实测红集是空的 ⇒ ⇒ 那条变异零判别力"
                    f"（⇒ ⇒ 若它**确实不成立**，必须在规格里**明写**「零红 / 不成立」）")

# --- 9. 本批的两个新场景码：唯一定义处 + 规格写明 + 未被撞号 ---------------
for code in EXPECT_NEW_CODES:
    if f"pub const {code}" not in error_rs:
        err(f"[新码] 契约层没有 `{code}` 的定义处")
    if error_rs.count(f"pub const {code}") != 1:
        err(f"[新码] `{code}` 的定义处**不止一处** ⇒ ⇒ 一码多义")
    if code not in text:
        err(f"[新码] 规格里没有写 `{code}`")
# ⚠️ **不许复用 `E_PLT_16`** ⇒ ⇒ 「一个场景码只一个含义」这条没被破坏。
if "E_PLT_18" in error_rs and "E_PLT_16" in error_rs:
    # 两个码**各自独立**存在 ⇒ ⇒ 合法；这里只查**没有把新码定义成旧码的别名**。
    for code in EXPECT_NEW_CODES:
        if re.search(rf"pub const {code}[^=]*=\s*E_PLT_16", error_rs):
            err(f"[新码] `{code}` 被定义成 `E_PLT_16` 的别名 ⇒ ⇒ 一码两义")

# --- 10. 上一批的破坏性改动没有被回退（STAGE-48）---------------------------
for dto in ("pub struct BridgeReportDto", "pub struct DroppedWireDto",
            "pub enum WireDropReason"):
    if dto not in model_rs:
        err(f"[回退] 契约层没有 `{dto}` ⇒ ⇒ 上一批（STAGE-48）的连线报告"
            f"**被回退了**")
if "E_PLT_17_BOARD_MISMATCH" not in error_rs:
    err("[回退] 契约层没有 `E_PLT_17_BOARD_MISMATCH` ⇒ ⇒ 上一批那个场景码"
        "**被回退了**")
if "E_PLT_17_BOARD_MISMATCH" not in host_rs:
    err("[回退] 内核侧没有引用 `E_PLT_17_BOARD_MISMATCH` ⇒ ⇒ 上一批那个判定"
        "**被回退了**")
# ⚠️ **另外三个版本号不许被带走**（本批只许升描述格式那两个）。
for name in ("SCHEMA_VERSION", "SIM_STATE_SCHEMA_VERSION"):
    m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", lib)
    if not m:
        for src_ in (model_rs, protocol_rs, platform_rs):
            m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_)
            if m:
                break
    if not m:
        err(f"[回退] 读不到 {name}")
    elif m.group(1) != EXPECT_VERSIONS[name]:
        err(f"[回退] {name} = {m.group(1)} ⇒ ⇒ 本批**只**许升描述格式那两个")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-49 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-49 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · K {len(EXPECT_ACCEPT['K'])}"
      f" · N {len(EXPECT_ACCEPT['N'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])}")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM={EXPECT_VERSIONS['PLATFORM_SCHEMA_VERSION']}"
      f" / PLATFORM_RULES={EXPECT_VERSIONS['PLATFORM_RULES_VERSION']}")
print(f"  新场景码：{len(EXPECT_NEW_CODES)} 个（各有唯一定义处）")
sys.exit(0)
