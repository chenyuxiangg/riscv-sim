#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-48 冻结规格（v0.72 · 宿主不再把器件实例塌缩成固定角色）的自检。

用法：python3 tools/stage48_check.py docs/detailed-design/STAGE-48_V072_Device_Instance_Table.md

⚠️ **十项**（与 `tools/stage47_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
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
       ＋ 7d. **四个版本号的值**（**本批升版** ⇒ ⇒ `SCHEMA_VERSION` = 11，其余三个**不动**）
          **且** **前端镜像**与契约层**逐字相同**
       ＋ 7e. **跨层源代码锚点**（实例表 / 连线编译 / 报告 DTO / 内核门禁 /
          前端那几条 / 真机段的段名）
       ＋ 7f. 台账必须登记本批（`v0.72` + `STAGE-48` + `D-177`）
       ＋ 7g. 脚本必须自指存在
    8. **契约层升版**（`SCHEMA_VERSION` = 11，其余三个**一个都不动**）⇒ ⇒
       前端镜像**也**必须一样
    9. **理由词表**：**恰好 6 个值** + **线上形态是 kebab-case** + **不是场景码**
       （⇒ ⇒ `error.rs` 里**不许**有它 ⇒ ⇒ `error_code_check.py` 零改动）
    10. **上一批的破坏性改动没有被回退**（STAGE-47 的 `E_PLT_17` 判定**必须**
        仍然在 ⇒ ⇒ 且 `SCHEMA_VERSION` 的**升版**不许把**另外三个**版本号带走）

⚠️ **编号空间**（与 STAGE-47 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-5）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-13）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-4）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-146 ~ B-150，接 v0.71 批的 B-145）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-48_V072_Device_Instance_Table.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **本批**（STAGE-48 · v0.72）**契约层升版** ⇒ ⇒ `SCHEMA_VERSION` 10 → 11。
#:
#: ⚠️ ⚠️ **v0.73 批（STAGE-49）把 `PLATFORM_SCHEMA_VERSION` 1 → 2 ·
#:    `PLATFORM_RULES_VERSION` 3 → 4**（描述格式一次升版）⇒ ⇒ ⇒ 那两个
#:    **批次局部**的期望值**已经作废** ⇒ ⇒ ⇒ **不许**在这里钉它们
#:    （⇒ ⇒ 否则**下一批**的合法升版会把**上一批**的门禁**永久打红**）。
#:
#: ⚠️ **判别力保留**：这里仍然钉**本批之后一直没动**的那两个（**信封版本**
#:    与**会话快照版本**）⇒ ⇒ ⇒ 有人误改它们**照样红** ⇒ ⇒ ⇒ 而「描述
#:    格式版本 / 规则集版本」现在由 **STAGE-49 那一批**的
#:    `tools/stage49_check.py` 守着（⇒ ⇒ **一个约束一个落点**：每个版本号
#:    由**最新**那一批的门禁守）。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "11",
    "SIM_STATE_SCHEMA_VERSION": "2",
}

#: ⚠️ **v0.73 批升过版的那两个**（⇒ ⇒ **不在**上面的期望值里 ⇒ ⇒ ⇒
#:    7d / 8 / 10 那些检查**只**作用于上面那两个）。
EXPECT_VERSIONS_FROZEN_BY_LATER_BATCH = [
    "PLATFORM_SCHEMA_VERSION",
    "PLATFORM_RULES_VERSION",
]

LIB_REL = "crates/contract/src/lib.rs"
MODEL_REL = "crates/contract/src/model.rs"
PROTOCOL_REL = "crates/contract/src/protocol.rs"
ERROR_REL = "crates/contract/src/error.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
HOST_PLATFORM_REL = "crates/kernel/src/kernel_host/platform.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
KTEST_REL = "crates/kernel/src/kernel_host/tests_v072_bridge.rs"
MIRROR_REL = "front/lib/message.js"
WRLIB_REL = "front/lib/bridge-report.js"
MMAP_REL = "front/views/components/memory-map.js"
TEST_REL = "front/tests/bridge-report.test.mjs"
BROWSER_REL = "tools/browser-verify.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"
PREV_TEST_REL = "crates/kernel/src/kernel_host/tests_v071_unknown_system.rs"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**。
EXPECT_ACCEPT = {
    "A": list(range(1, 6)),       # A-1 ~ A-5（契约与跨层）
    "K": list(range(1, 15)),      # K-1 ~ K-14（内核）
    "N": list(range(1, 5)),       # N-1 ~ N-4（前端 Node）
    "B": list(range(146, 151)),   # B-146 ~ B-150（真机门禁 · 跨批次唯一）
    "X": list(range(1, 4)),       # X-1 ~ X-3（交叉）
}
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-145 也算「已定义」。
    "B": set(range(1, 151)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ **变异编号空间**（§5 · T-1 ~ T-7）。
MUTATION_IDS = list(range(1, 8))

#: ⚠️ **理由词表**（恰好 6 个值 · **线上形态是 kebab-case**）。
EXPECT_REASONS = [
    "source-unknown",
    "source-type",
    "target-unknown",
    "target-type",
    "source-out-of-range",
    "overridden",
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
wr_lib = read(WRLIB_REL) or ""
mmap_js = read(MMAP_REL) or ""
test_js = read(TEST_REL) or ""
browser = read(BROWSER_REL) or ""
ledger = read(LEDGER_REL) or ""
prev_test = read(PREV_TEST_REL) or ""

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
    # ⚠️ **实例表**（裁决 A / B）
    (host_platform, "pub struct DeviceInstanceTable"),
    (host_platform, "pub enum DeviceHandle"),
    (host_platform, "pub fn uart(&self, id: &str)"),
    (host_platform, "pub fn plic(&self, id: &str)"),
    # ⚠️ **连线编译**（裁决 C / D）
    (host_platform, "pub fn compile_bridge("),
    (host_platform, "pub struct BridgeCompile"),
    (host_platform, "fn check_wire("),
    # ⚠️ **宿主消费实例表**（裁决 E / F）
    (host_rs, "self.devices.uart(&wire.from_device)"),
    (host_rs, "self.devices.plic(&wire.to_device)"),
    (host_rs, "pub fn uart_receive_on("),
    (host_rs, "bridge_report: self.bridge_report.clone()"),
    # ⚠️ **契约层的新 DTO**（裁决 G / H）
    (model_rs, "pub struct BridgeReportDto"),
    (model_rs, "pub struct DroppedWireDto"),
    (model_rs, "pub enum WireDropReason"),
    (protocol_rs, "bridge_report: crate::model::BridgeReportDto"),
    # ⚠️ **内核的门禁**（K-1 ~ K-13）
    (ktest, "fn k1_the_second_uarts_wire_really_carries_the_interrupt"),
    (ktest, "fn k2_the_control_a_gpio_source_cannot_be_raised"),
    (ktest, "fn k3_the_default_bridge_still_claims_source_10"),
    (ktest, "fn k4_a_shadowed_wire_is_replaced_not_appended"),
    (ktest, "fn k5_a_wire_to_a_missing_device_is_rejected"),
    (ktest, "fn k6_on_a_valid_description_only_two_reasons_are_reachable"),
    (ktest, "fn k7_a_shadowed_wire_is_reported_as_overridden"),
    (ktest, "fn k9_the_report_is_recomputed_when_the_board_changes"),
    (ktest, "fn k11_the_four_fallback_reasons_are_still_reachable"),
    (ktest, "fn k12_every_real_report_keeps_the_identity"),
    (ktest, "fn k13_the_host_really_holds_a_handle_for_the_second_uart"),
    # ⚠️ **前端那几条**（N-1 ~ N-4）
    (wr_lib, "export function normalizeBridgeReport"),
    (wr_lib, "export function wireDropReasonLabel"),
    (wr_lib, "export function describeBridgeReport"),
    (mmap_js, "export function renderBridgeReport"),
    (test_js, "N-1"),
    (test_js, "N-2"),
    (test_js, "N-3"),
    (test_js, "N-4"),
    # ⚠️ **真机段**（B-146 ~ B-150）
    (browser, "bridgeReport"),
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
for needle in ("D-177", "STAGE-48", "v0.72"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage48_check.py" not in text:
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

# --- 9. 理由词表：恰好 6 个 + 线上形态 + **不是场景码** ---------------------
if "pub enum WireDropReason" not in model_rs:
    err("[词表] 契约层没有 `WireDropReason`")
else:
    variants = re.findall(r"^\s{4}([A-Z][A-Za-z]+),$", model_rs, re.M)
    # ⚠️ **只数 WireDropReason 那段**（枚举体到下一个 `}`）。
    seg = model_rs.split("pub enum WireDropReason", 1)[1]
    seg = seg.split("\n}", 1)[0]
    names = re.findall(r"^\s{4}([A-Z][A-Za-z]+),$", seg, re.M)
    if len(names) != len(EXPECT_REASONS):
        err(f"[词表] 理由词表有 {len(names)} 个值（{names}）⇒ ≠ 规格的 "
            f"{len(EXPECT_REASONS)} 个")
for reason in EXPECT_REASONS:
    if reason not in text:
        err(f"[词表] 规格里没有写理由词 `{reason}`")
    if reason not in wr_lib:
        err(f"[词表] 前端镜像里没有 `{reason}` ⇒ ⇒ 界面**显示不出来**")
    if reason not in ktest:
        err(f"[词表] 内核门禁里没有 `{reason}` ⇒ ⇒ 那条理由**没人测**")
if "WireDropReason" in error_rs:
    err("[词表] `error.rs` 里出现了 `WireDropReason` ⇒ ⇒ 理由词被当成了"
        "**场景码** ⇒ ⇒ 本批**零新场景码**这条被破坏了")
# ⚠️ ⚠️ **不要**断言「`E_PLT_18` 不存在」—— 那是一条**批次局部**的性质
#   （「**本批**零新场景码」）⇒ ⇒ **后续批次**（v0.73 · STAGE-49）**合法地**
#   新增了 `E_PLT_18`（角色声明）/ `E_PLT_19`（引脚表）⇒ ⇒ ⇒ 那条断言会**误红**
#   ⇒ ⇒ ⇒ **判据必须是「不随批次失效的那条性质」** ⇒ ⇒ 这里查的是：
#   **理由词表的值没有变成场景码**（`source-unknown` 那种词**不许**出现在
#   `error.rs` 里）⇒ ⇒ ⇒ 这条**永远成立**。
for reason in EXPECT_REASONS:
    if reason in error_rs:
        err(f"[词表] `error.rs` 里出现了理由词 `{reason}` ⇒ ⇒ 理由词被当成了"
            "**场景码** ⇒ ⇒ 一码两义 ⇒ 禁止")
# ⚠️ **下一批（v0.73）的两个新码**是**合法**的（各有唯一定义处）⇒ ⇒ 这里只查
#   它们**没有被定义成 `E_PLT_16` 的别名**（那才是「一码两义」）。
for new_code in ("E_PLT_18_ROLES_INVALID", "E_PLT_19_PINS_INVALID"):
    if re.search(rf"pub const {new_code}[^=]*=\s*E_PLT_16", error_rs):
        err(f"[新码] `{new_code}` 被定义成 `E_PLT_16` 的别名 ⇒ ⇒ 一码两义")

# --- 10. 上一批的破坏性改动没有被回退（STAGE-47）------------------------------
if "E_PLT_17_BOARD_MISMATCH" not in error_rs:
    err("[回退] 契约层没有 `E_PLT_17_BOARD_MISMATCH` ⇒ ⇒ 上一批（STAGE-47）"
        "那个场景码**被回退了**")
if "E_PLT_17_BOARD_MISMATCH" not in host_rs:
    err("[回退] 内核侧没有引用 `E_PLT_17_BOARD_MISMATCH` ⇒ ⇒ 上一批那个判定"
        "**被回退了**")
if "fn k1_image_inside_the_user_board_loads" not in prev_test:
    err("[回退] 上一批的内核门禁 `tests_v071_unknown_system.rs` **不在**了 ⇒ ⇒ "
        "「未知系统」那条判定**没人守**")
# ⚠️ **本批**（STAGE-48）**不许被带走**的那些版本号 —— ⚠️ **v0.73 批**
#    （STAGE-49）**合法地**升了 `PLATFORM_SCHEMA_VERSION` / `PLATFORM_RULES_VERSION`
#    ⇒ ⇒ ⇒ 那两个**不再**由本脚本守（⇒ ⇒ 由 `tools/stage49_check.py` 守
#    ⇒ ⇒ ⇒ **一个约束一个落点**）⇒ ⇒ ⇒ 这里只守**真的没动**的那些。
for name in ("SIM_STATE_SCHEMA_VERSION",):
    m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", lib)
    if not m:
        m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)",
                      read("crates/contract/src/platform.rs") or "")
        m = m or re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", model_rs)
    if not m:
        err(f"[回退] 读不到 {name}")
    elif m.group(1) != EXPECT_VERSIONS[name]:
        err(f"[回退] {name} = {m.group(1)} ⇒ ⇒ 本批**不许**动它")
# ⚠️ **那两个被下一批合法升版的** ⇒ ⇒ 只**确认它们还在**（⇒ ⇒ 有人**删掉**
#    它们 ⇒ ⇒ 门禁会红）⇒ ⇒ ⇒ **不**钉具体值（⇒ ⇒ 那是**下一批**的门禁的活）。
for name in EXPECT_VERSIONS_FROZEN_BY_LATER_BATCH:
    found = False
    for src_ in (lib, model_rs, protocol_rs, platform_rs):
        if re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_):
            found = True
            break
    if not found:
        err(f"[回退] 契约层没有 {name} ⇒ ⇒ 那个版本号**被删掉了**")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-48 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-48 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · K {len(EXPECT_ACCEPT['K'])}"
      f" · N {len(EXPECT_ACCEPT['N'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])}")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / （PLATFORM / PLATFORM_RULES 已由 **STAGE-49 批**升版 ⇒ ⇒ 归它守）")
print(f"  理由词表：{len(EXPECT_REASONS)} 个值")
sys.exit(0)
