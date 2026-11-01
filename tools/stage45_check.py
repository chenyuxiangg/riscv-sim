#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-45 冻结规格（v0.69 · 陈旧面收尾批：D-176 做法 C+B · D-173 做法 A · D-174 做法 B）的自检。

用法：python3 tools/stage45_check.py docs/detailed-design/STAGE-45_V069_Stale_Surfaces_Closure.md

⚠️ **十项**（与 `tools/stage44_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
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
       ＋ 7d. **四个版本号的值**（§2 的表是唯一权威处 ⇒ ⇒ `SCHEMA_VERSION` = 10）
       ＋ 7e. **跨层源代码锚点**（前端侧：新模块的四个导出 / 视图层的两个钩子 /
          app 层的两条命令落点；工具侧：`detect_mask_mismatch` + `selftest_mask`）
       ＋ 7f. 台账必须登记本批（`v0.69` + `STAGE-45` + `D-176` / `D-173` / `D-174`）
       ＋ 7g. 脚本必须自指存在
    8. **零内核改动**（裁决 A-3）—— `crates/` 下**零 diff** ⇒ ⇒ 契约与内核行为
       **一字不变**（⇒ ⇒ 用 git 查**工作区**与 `HEAD` 的差异）
    9. **零新场景码**（裁决 A-2）—— 契约层 `error.rs` 里**不得**出现 `E_PLT_17`
   10. **稳定钩子的移出**（裁决 J / N）—— `data-embedded-time` **必须**不在
       `front/views/m02_viewcomponents.js` 里 ⇒ ⇒ **且** `tools/stage35_check.py`
       的 `EXPECT_HOOKS` 里**也**没有它（⇒ ⇒ 两处**都**改了 ⇒ ⇒ 不会「只改一处」）

⚠️ **编号空间**（与 STAGE-44 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约层（Rust）⇒ ⇒ **每批范围**（A-1 ~ A-3）
    * **K 组** = 内核（Rust）⇒ ⇒ **本批为空**（零内核改动 ⇒ ⇒ **如实写空**）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-12）
    * **L 组** = 台账口径工具（Python）⇒ ⇒ **每批范围**（L-1 ~ L-2）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-130 ~ B-135，接 v0.68 批的 B-129）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）
⇒ ⇒ **K 组不叫 B**：同一个输出里 ID 必须全局唯一 ⇒ ⇒ 复用 `B` 会与真机段
   **撞名** ⇒ ⇒ 内核那组另起一个字母。
⇒ ⇒ **L 组**同理：那两条判据落在 **Python 工具**里 ⇒ ⇒ 塞进 A / N / B 任一组
   都是**张冠李戴** ⇒ ⇒ 单起一个字母。
"""
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-45_V069_Stale_Surfaces_Closure.md")
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
FOLLOW_REL = "front/lib/disasm-follow.js"
VIEWS_REL = "front/views/m02_viewcomponents.js"
APP_REL = "front/app.js"
TEST_REL = "front/tests/stale-surfaces.test.mjs"
LANDING_REL = "front/tests/embedded-landing.test.mjs"
ATTRIB_REL = "tools/front_test_attrib.py"
STAGE35_REL = "tools/stage35_check.py"
BROWSER_REL = "tools/browser-verify.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**（三个「每批范围」+ 一个「跨批次唯一」）。
EXPECT_ACCEPT = {
    "A": list(range(1, 4)),       # A-1 ~ A-3（契约与跨层）
    # ⚠️ **K 组本批为空**（零内核改动）⇒ ⇒ **不列**（列了就会要求规格里出现
    #    一个**并不存在**的 K 组判据 ⇒ ⇒ 那是**逼人编**）。
    "N": list(range(1, 13)),      # N-1 ~ N-12（前端 Node）
    "L": list(range(1, 3)),       # L-1 ~ L-2（台账口径工具）
    "B": list(range(130, 136)),   # B-130 ~ B-135（真机门禁 · 跨批次唯一）
    "X": list(range(1, 4)),       # X-1 ~ X-3（交叉）
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(),
    "N": set(EXPECT_ACCEPT["N"]),
    "L": set(EXPECT_ACCEPT["L"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-129 也算「已定义」。
    "B": set(range(1, 136)),
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
# 反向：正文引用了未定义的验收编号
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
error_rs = read(ERROR_REL) or ""
follow_js = read(FOLLOW_REL) or ""
views = read(VIEWS_REL) or ""
app_js = read(APP_REL) or ""
test_js = read(TEST_REL) or ""
landing_test = read(LANDING_REL) or ""
attrib = read(ATTRIB_REL) or ""
stage35 = read(STAGE35_REL) or ""
browser = read(BROWSER_REL) or ""

# 7d. 四个版本号
for name, want in EXPECT_VERSIONS.items():
    # ⚠️ **四个版本号住在三个文件里** ⇒ ⇒ 逐个找（找不到就是红 ⇒ 不许猜）。
    m = None
    for src in (lib, model_rs, read(PLATFORM_REL) or ""):
        m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src)
        if m:
            break
    if not m:
        err(f"[版本] 契约层读不到 {name}")
        continue
    if m.group(1) != want:
        err(f"[版本] {name} = {m.group(1)} ≠ 规格的 {want}")
    if name not in text:
        err(f"[版本] 规格里没有提到 {name}")

# 7e. 跨层源代码锚点（前端侧）
JS_ANCHORS = [
    # ⚠️ **新模块**（裁决 L）⇒ ⇒ 四个导出**必须**在 ⇒ ⇒ 「新能力住新模块」
    #   这条纪律**结构上**可查。
    (follow_js, "export function segmentBaseAddr"),
    (follow_js, "export function isStaleSegment"),
    (follow_js, "export function shouldFollowDisasm"),
    (follow_js, "export function disasmStatusText"),
    (follow_js, "export function followPcLabel"),
    # ⚠️ **两个新 DOM 钩子**（裁决 I）
    (views, "data-disasm-status"),
    (views, "data-disasm-stale"),
    (views, "data-disasm-follow"),
    # ⚠️ **app 层的两处落点**（裁决 D / F）
    (app_js, 'case "disasm-follow"'),
    (app_js, "shouldFollowDisasm"),
    (app_js, "Command.disasm(pc, DEFAULT_DISASM_COUNT)"),
    # ⚠️ **前端用例的编号锚点**
    (landing_test, "N-1"),
    (landing_test, "N-2"),
    (test_js, "N-3"),
    (test_js, "N-4"),
    (test_js, "N-5"),
    (test_js, "N-6"),
    (test_js, "N-7"),
    (test_js, "N-8"),
    (test_js, "N-9"),
    (test_js, "N-10"),
    (test_js, "N-11"),
    (test_js, "N-12"),
    # ⚠️ **真机段的钩子与段名**（裁决 M）
    (browser, "BV_STALE_SURFACES_PROBE"),
    (browser, "runStaleSurfacesSection"),
    (browser, '"staleSurfaces"'),
    # ⚠️ **工具侧的两个落点**（裁决 K · D-174 做法 B）
    (attrib, "def detect_mask_mismatch"),
    (attrib, "def selftest_mask"),
]
for src, needle in JS_ANCHORS:
    if src is None:
        err(f"[锚点] 文件缺失（{needle}）")
    elif needle not in src:
        err(f"[锚点] 缺 {needle}")

# 7b. 事实表编号连续
facts = sorted({int(m.group(1)) for m in re.finditer(r"\|\s*F-(\d+)\s*\|", text)})
if facts != list(range(1, len(facts) + 1)) or not facts:
    err(f"[事实] §1 的事实编号 {facts} 不是 F-1 ~ F-{len(facts)} 连续")

# 7f. 台账登记
ledger = read(LEDGER_REL) or ""
for needle in ("D-176", "D-173", "D-174", "STAGE-45", "v0.69"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage45_check.py" not in text:
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
            # ⚠️ **最后一条是「对照」** ⇒ ⇒ 它的红集**理应为空** ⇒ ⇒ 判据是
            # 「**明写**零红」而不是「红集非空」⇒ ⇒ 否则对照自己会被红。
            if not any("零红" in c for c in cells):
                err(f"[订正] T-{tid} 是**对照** ⇒ ⇒ 必须**明写**「零红」")
            continue
        if not any(re.search(r"红\s*[1-9]", c) for c in cells):
            err(f"[订正] T-{tid} 的实测红集是空的 ⇒ ⇒ 那条变异零判别力")

# --- 8. 零内核改动（裁决 A-3）----------------------------------------------
try:
    diff = subprocess.run(
        ["git", "diff", "--name-only", "HEAD", "--", "crates/"],
        cwd=REPO_ROOT, capture_output=True, text=True, check=False)
    touched = [l for l in diff.stdout.splitlines() if l.strip()]
    if touched:
        err(f"[零内核改动] `crates/` 下有改动：{touched} ⇒ ⇒ 本批**零内核改动**")
except Exception as exc:  # pragma: no cover
    err(f"[零内核改动] 查 git 失败：{exc}")

# --- 9. 零新场景码（裁决 A-2）----------------------------------------------
for m in re.finditer(r"E_PLT_(\d+)", error_rs):
    if int(m.group(1)) >= 17:
        err(f"[零新码] 契约层出现了 E_PLT_{m.group(1)}（本批**不**新增判据 ⇒ ⇒ 零新场景码）")

# --- 10. 稳定钩子的移出（裁决 J / N）---------------------------------------
if "data-embedded-time" in views:
    err("[钩子] `front/views/m02_viewcomponents.js` 里还有 `data-embedded-time`"
        " ⇒ ⇒ 落地视图那份**重复的**虚拟时间**没删掉**（裁决 J）")
# ⚠️ **只查 `EXPECT_HOOKS` 那个元组** ⇒ ⇒ 那份文件的**注释里**提到这个钩子
#    （本批把「为什么移出」写在了那儿）⇒ ⇒ 全文搜会**误报**。
m_hooks = re.search(r"EXPECT_HOOKS\s*=\s*\((.*?)\n\)", stage35, re.S)
if not m_hooks:
    err("[钩子] `tools/stage35_check.py` 里读不到 `EXPECT_HOOKS`")
else:
    hooks = set(re.findall(r'"([^"]+)"', m_hooks.group(1)))
    if "data-embedded-time" in hooks:
        err("[钩子] `tools/stage35_check.py` 的稳定钩子清单里还有 `data-embedded-time`"
            " ⇒ ⇒ 那条门禁**会红** ⇒ ⇒ 必须**两处**都改（裁决 N）")
    # ⚠️ **其余钩子一个都不许少** ⇒ ⇒ STAGE-35 那张清单**不许**被清空。
    for keep in ("data-embedded-pc", "data-embedded-link", "data-disasm-addr", "reg-grid"):
        if keep not in hooks:
            err(f"[钩子] STAGE-35 的稳定钩子清单里少了 {keep} ⇒ ⇒ 本批**只**许移出那一个")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-45 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-45 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · N {len(EXPECT_ACCEPT['N'])}"
      f" · L {len(EXPECT_ACCEPT['L'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])} · K 0（零内核改动）")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM={EXPECT_VERSIONS['PLATFORM_SCHEMA_VERSION']}"
      f" / PLATFORM_RULES={EXPECT_VERSIONS['PLATFORM_RULES_VERSION']}")
sys.exit(0)
