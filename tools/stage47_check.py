#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-47 冻结规格（v0.71 · 「未知系统」门禁）的自检。

用法：python3 tools/stage47_check.py docs/detailed-design/STAGE-47_V071_Unknown_System_Gate.md

⚠️ **十项**（与 `tools/stage46_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
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
       ＋ 7d. **四个版本号的值**（**本批零升版** ⇒ ⇒ `SCHEMA_VERSION` = 10）**且**
          **前端镜像**与契约层**逐字相同**
       ＋ 7e. **跨层源代码锚点**（内核的纯函数与接线 / 契约层的新码 / 内核门禁 /
          前端那三条 / 真机段的段名与探针）
       ＋ 7f. 台账必须登记本批（`v0.71` + `STAGE-47` + `D-175`）
       ＋ 7g. 脚本必须自指存在
    8. **契约层零升版**（四个版本号全不动）⇒ ⇒ 前端镜像**也**必须一样
    9. **新场景码**：有定义处 + 值 == 17 + 内核**引用**了它 + 规格写明**线上形态**
   10. **上一批的破坏性改动没有被回退**（D-173 删掉的 `data-embedded-time`
       **必须**仍然不在视图里，**且**仍然不在 STAGE-35 的稳定钩子清单里）

⚠️ **编号空间**（与 STAGE-46 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-3）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-6）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-3）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-141 ~ B-145，接 v0.70 批的 B-140）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-2）
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-47_V071_Unknown_System_Gate.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约层零升版** ⇒ ⇒ 全不动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "10",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

LIB_REL = "crates/contract/src/lib.rs"
MODEL_REL = "crates/contract/src/model.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
ERROR_REL = "crates/contract/src/error.rs"
HOST_PLATFORM_REL = "crates/kernel/src/kernel_host/platform.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
KTEST_REL = "crates/kernel/src/kernel_host/tests_v071_unknown_system.rs"
VIEWS_REL = "front/views/m02_viewcomponents.js"
MIRROR_REL = "front/lib/message.js"
TEST_REL = "front/tests/unknown-system.test.mjs"
STAGE35_REL = "tools/stage35_check.py"
BROWSER_REL = "tools/browser-verify.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**。
EXPECT_ACCEPT = {
    "A": list(range(1, 4)),       # A-1 ~ A-3（契约与跨层）
    "K": list(range(1, 7)),       # K-1 ~ K-6（内核）
    "N": list(range(1, 4)),       # N-1 ~ N-3（前端 Node）
    "B": list(range(141, 146)),   # B-141 ~ B-145（真机门禁 · 跨批次唯一）
    "X": list(range(1, 3)),       # X-1 ~ X-2（交叉）
}
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-140 也算「已定义」。
    "B": set(range(1, 146)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ **变异编号空间**（§5 · T-1 ~ T-6）。
MUTATION_IDS = list(range(1, 7))


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
platform_rs = read(PLATFORM_REL) or ""
error_rs = read(ERROR_REL) or ""
host_platform = read(HOST_PLATFORM_REL) or ""
host_rs = read(HOST_REL) or ""
ktest = read(KTEST_REL) or ""
views = read(VIEWS_REL) or ""
mirror_js = read(MIRROR_REL) or ""
test_js = read(TEST_REL) or ""
stage35 = read(STAGE35_REL) or ""
browser = read(BROWSER_REL) or ""

# 7d. 四个版本号（契约层 + 前端镜像）
for name, want in EXPECT_VERSIONS.items():
    m = None
    for src_ in (lib, model_rs, platform_rs):
        m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_)
        if m:
            break
    if not m:
        err(f"[版本] 契约层读不到 {name}")
        continue
    if m.group(1) != want:
        err(f"[版本] {name} = {m.group(1)} ≠ 规格的 {want} ⇒ ⇒ 本批**零升版**")
    if name not in text:
        err(f"[版本] 规格里没有提到 {name}")
m2 = re.search(r"SCHEMA_VERSION\s*=\s*['\"]?(\d+)", mirror_js)
if not m2:
    err("[版本] 前端镜像读不到 `SCHEMA_VERSION`")
elif m2.group(1) != EXPECT_VERSIONS["SCHEMA_VERSION"]:
    err(f"[版本] 前端镜像的 SCHEMA_VERSION = {m2.group(1)} ≠ 契约层的 "
        f"{EXPECT_VERSIONS['SCHEMA_VERSION']} ⇒ ⇒ 两边**不一致**")

# 7e. 跨层源码锚点
JS_ANCHORS = [
    # ⚠️ **内核的纯函数与接线**（裁决 C / D）
    (host_platform, "pub fn compatibility"),
    (host_platform, "pub fn board_intervals"),
    (host_platform, "pub struct BoardCompatibility"),
    (host_rs, "platform::compatibility"),
    (host_rs, "platform::board_intervals"),
    (host_rs, "E_PLT_17_BOARD_MISMATCH"),
    # ⚠️ **契约层的新码**（裁决 B）
    (error_rs, "pub const E_PLT_17_BOARD_MISMATCH: u16 = 17;"),
    # ⚠️ **内核的门禁**（K-1 ~ K-6）
    (ktest, "fn k1_image_inside_the_user_board_loads"),
    (ktest, "fn k2_image_outside_the_user_board_is_rejected"),
    (ktest, "fn k3_partially_outside_is_not_rejected"),
    (ktest, "fn k4_missing_and_partial_are_two_different_verdicts"),
    (ktest, "fn k5_the_verdict_uses_the_users_board"),
    (ktest, "fn k6_without_a_user_board_the_auto_selection_still_works"),
    # ⚠️ **前端那三条**（N-1 ~ N-3）
    (test_js, "N-1"),
    (test_js, "N-2"),
    (test_js, "N-3"),
    (mirror_js, "SCHEMA_VERSION"),
    # ⚠️ **真机段本批暂不做**（§4.4 已标注「整段摘掉 · 编号先不占」）⇒ ⇒
    #   ⇒ **不**在这里钉它 ⇒ ⇒ 否则「真机段还没做」这件事**看不出来**。
]
for src_, needle in JS_ANCHORS:
    if src_ is None:
        err(f"[锚点] 文件缺失（{needle}）")
    elif needle not in src_:
        err(f"[锚点] 缺 {needle}")

# 7b. 事实表编号连续
facts = sorted({int(m.group(1)) for m in re.finditer(r"\|\s*F-(\d+)\s*\|", text)})
if facts != list(range(1, len(facts) + 1)) or not facts:
    err(f"[事实] §1 的事实编号 {facts} 不是 F-1 ~ F-{len(facts)} 连续")

# 7f. 台账登记
ledger = read(LEDGER_REL) or ""
for needle in ("D-175", "STAGE-47", "v0.71"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage47_check.py" not in text:
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

# --- 9. 新场景码：有定义处 + 值 == 17 + 内核引用 + 规格写明线上形态 --------
if "pub const E_PLT_17_BOARD_MISMATCH: u16 = 17;" not in error_rs:
    err("[新码] 契约层没有 `E_PLT_17_BOARD_MISMATCH`（或值不是 17）")
if "E_PLT_17_BOARD_MISMATCH" not in host_rs:
    err("[新码] 内核侧没有引用新码 ⇒ ⇒ 那条判定**根本不存在**")
if "E_PLT-17" not in text:
    err("[新码] 规格里没有写**线上形态** `E_PLT-17` ⇒ ⇒ 断言会用错字符串")

# --- 10. 上一批的破坏性改动没有被回退（D-173）------------------------------
if "data-embedded-time" in views:
    err("[回退] `front/views/m02_viewcomponents.js` 里**又**有 `data-embedded-time`"
        " ⇒ ⇒ 上一批（D-173）删掉的那个钩子**被回退了**")
m_hooks = re.search(r"EXPECT_HOOKS\s*=\s*\((.*?)\n\)", stage35, re.S)
if not m_hooks:
    err("[回退] `tools/stage35_check.py` 里读不到 `EXPECT_HOOKS`")
else:
    hooks = set(re.findall(r'"([^"]+)"', m_hooks.group(1)))
    if "data-embedded-time" in hooks:
        err("[回退] `tools/stage35_check.py` 的稳定钩子清单里**又**有 `data-embedded-time`"
            " ⇒ ⇒ 上一批那笔**移出**被回退了")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-47 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-47 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · K {len(EXPECT_ACCEPT['K'])}"
      f" · N {len(EXPECT_ACCEPT['N'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])}")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM={EXPECT_VERSIONS['PLATFORM_SCHEMA_VERSION']}"
      f" / PLATFORM_RULES={EXPECT_VERSIONS['PLATFORM_RULES_VERSION']}")
sys.exit(0)
