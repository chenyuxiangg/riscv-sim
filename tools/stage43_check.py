#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-43 冻结规格（v0.66 · 内核侧中间层批：器件类型注册表 + 逐实例状态关联）的自检。

用法：python3 tools/stage43_check.py docs/detailed-design/STAGE-43_V066_Device_Type_Registry_Instance_State.md

⚠️ **十项**（与 `tools/stage42_check.py` 同构 ⇒ ⇒ **不要**照抄它的编号范围）：
    1. Markdown 表格列数一致（**切分复用 `tools/doc_check.py` 的那一份** ⇒ ⇒
       ⛔ 不许自己再写一份朴素 `split("|")` ⇒ ⇒ **一个约束一个落点**）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §1~§9 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1.2 的事实表**编号列**必须是 **F-1 ~ F-13 连续**
       ＋ 7c. §8 的施工期订正：不是占位 + **变异编号**齐全 + 每条有**非空的实测红集**
       ＋ 7d. **四个版本号的值**（§2.5 的表是唯一权威处 ⇒ ⇒ `SCHEMA_VERSION` = 10）
       ＋ 7e. **跨层源代码锚点**（Rust 侧两条命令 / 两条事件 / 四个 DTO；
          前端镜像的构造器 / topic；产品视图的 DOM 钩子）
       ＋ 7f. 台账必须登记本批（`v0.66` + `STAGE-43` + `D-175`）
       ＋ 7g. 脚本必须自指存在
    8. **类型注册表与契约层变体集合的关系**（裁决 H）—— 规格里写明的那条
       「可实例化 ⊆ 已实现」必须能在两侧源码里看到（`DeviceSpec::KINDS`
       与 `DEVICE_TYPES`）
    9. **零新场景码**（裁决 M）—— 契约层 `error.rs` 里 `E_PLT_` 的编号
       **不得**出现 `E_PLT_17`（本批不新增判据）

⚠️ **编号空间**（与 STAGE-42 同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约层（Rust）⇒ ⇒ **每批范围**（A-1 ~ A-3）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-8）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-6）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-115 ~ B-117，接 v0.65 批的 B-114）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-2）
⇒ ⇒ **K 组不叫 B**：同一个输出里 ID 必须全局唯一 ⇒ ⇒ 复用 `B` 会与真机段
   **撞名** ⇒ ⇒ 内核那组另起一个字母。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-43_V066_Device_Type_Registry_Instance_State.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d 用）—— 本批**契约信封升版** ⇒ ⇒ 只有它动。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "10",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": "1",
    "PLATFORM_RULES_VERSION": "3",
}

PROTOCOL_REL = "crates/contract/src/protocol.rs"
MODEL_REL = "crates/contract/src/model.rs"
ERROR_REL = "crates/contract/src/error.rs"
LIB_REL = "crates/contract/src/lib.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
TYPES_REL = "crates/kernel/src/m15_peripheral/types.rs"
MIRROR_REL = "front/lib/message.js"
VIEWS_REL = "front/views/m02_viewcomponents.js"
REGISTRY_REL = "front/views/components/device-registry.js"
DEVICES_REL = "front/services/m15_devices.js"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**（三个「每批范围」+ 一个「跨批次唯一」）。
EXPECT_ACCEPT = {
    "A": list(range(1, 4)),       # A-1 ~ A-3（契约层）
    "K": list(range(1, 9)),       # K-1 ~ K-8（内核）
    "N": list(range(1, 7)),       # N-1 ~ N-6（前端 Node）
    "B": list(range(115, 118)),   # B-115 ~ B-117（真机门禁 · 跨批次唯一）
    "X": list(range(1, 3)),       # X-1 ~ X-2（交叉）
}
#: ⚠️ **反向引用允许**的编号全集 —— 比 `EXPECT_ACCEPT` 多出**前几批已定义**的那些。
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-114 也算「已定义」。
    "B": set(range(1, 118)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ **变异编号空间**（§6.6 · T-1 ~ T-5）。
MUTATION_IDS = list(range(1, 6))


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
            err(f"[验收] §6 缺 {group}-{n}")
# 反向：正文引用了未定义的验收编号
sec8_at = re.search(r"^## §8\b", text, re.M)
head = text[: sec8_at.start()] if sec8_at else text
for m in re.finditer(r"\b([AKNBX])-(\d+)\b", head):
    group, num = m.group(1), int(m.group(2))
    if group not in KNOWN_ACCEPT:
        continue
    if num not in KNOWN_ACCEPT[group]:
        err(f"[验收] 正文引用了未定义的验收编号 {group}-{num}")

# --- 7. 关键约束字面量钉住 -------------------------------------------------
lib = read(LIB_REL) or ""
model_rs = read(MODEL_REL) or ""
platform_rs = read(PLATFORM_REL) or ""
protocol = read(PROTOCOL_REL) or ""
model = read(MODEL_REL) or ""
platform = read(PLATFORM_REL) or ""
types_rs = read(TYPES_REL) or ""
mirror = read(MIRROR_REL) or ""
views = read(VIEWS_REL) or ""
registry = read(REGISTRY_REL) or ""
devices_js = read(DEVICES_REL) or ""

# 7d. 四个版本号
for name, want in EXPECT_VERSIONS.items():
    # ⚠️ **四个版本号住在三个文件里** ⇒ ⇒ 逐个找（找不到就是红 ⇒ 不许猜）。
    m = None
    for src in (lib, model_rs, platform_rs):
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

# 7e. 跨层源代码锚点
RUST_ANCHORS = [
    (protocol, "CommandPayload::ListDeviceTypes"),
    (protocol, "CommandPayload::ListDeviceInstances"),
    (protocol, "EventPayload::DeviceTypes"),
    (protocol, "EventPayload::DeviceInstances"),
    (model, "pub struct DeviceTypeInfo"),
    (model, "pub struct DeviceRegisterInfo"),
    (model, "pub struct DeviceParamInfo"),
    (model, "pub struct DeviceInstanceInfo"),
    (platform, "pub const KINDS"),
    (types_rs, "pub const DEVICE_TYPES"),
    (types_rs, "pub fn device_type_infos"),
]
for src, needle in RUST_ANCHORS:
    if src is None:
        err(f"[锚点] 源码文件缺失（{needle}）")
    elif needle not in src:
        err(f"[锚点] Rust 侧缺 {needle}")

JS_ANCHORS = [
    (mirror, "listDeviceTypes"),
    (mirror, "listDeviceInstances"),
    (mirror, "deviceTypes"),
    (mirror, "deviceInstances"),
    (mirror, 'DeviceTypes: "device.types"'),
    (mirror, 'DeviceInstances: "device.instances"'),
    (views, "renderDeviceTypeCard"),
    (views, "renderDeviceInstanceCard"),
    (registry, 'data-device-types="rows"'),
    (registry, 'data-device-instances="rows"'),
    (devices_js, "wireDevices"),
]
for src, needle in JS_ANCHORS:
    if src is None:
        err(f"[锚点] 前端文件缺失（{needle}）")
    elif needle not in src:
        err(f"[锚点] 前端侧缺 {needle}")

# 7b. 事实表编号连续（F-1 ~ F-13）
facts = sorted({int(m.group(1)) for m in re.finditer(r"\|\s*F-(\d+)\s*\|", text)})
if facts != list(range(1, 14)):
    err(f"[事实] §1.2 的事实编号 {facts} ≠ F-1 ~ F-13 连续")

# 7f. 台账登记
ledger = read(LEDGER_REL) or ""
if "D-175" not in ledger:
    err("[台账] 台账里没有 D-175")
if "STAGE-43" not in ledger:
    err("[台账] 台账里没有 STAGE-43")
if "v0.66" not in ledger:
    err("[台账] 台账里没有 v0.66")

# 7g. 脚本自指
if "stage43_check.py" not in text:
    err("[自指] 规格里没有提到本脚本")

# --- 7c. §8 的施工期订正：不是占位 + 变异编号齐全 + 每条有非空实测红集 ----
sec8 = re.search(r"^## §8\b(.*?)(?=^## §9|\Z)", text, re.M | re.S)
if not sec8:
    err("[订正] 缺 §8")
else:
    sec8_text = sec8.group(1)
    if "待填" in sec8_text:
        err("[订正] §8 还是占位（『待填』）")
    for t in MUTATION_IDS:
        if not re.search(rf"^\|\s*\*{{0,2}}T-{t}\*{{0,2}}\s*\|", sec8_text, re.M):
            err(f"[订正] §8 缺变异 T-{t}")
    # 每条变异的**实测红集**必须非空 ⇒ ⇒ 「红 0 条」不算数。
    #
    # ⚠️ **红集在哪一格**：那张表有四列（编号 / 变异 / 实测红集 / 红集归属）
    # ⇒ ⇒ **不能**只读最后一格 ⇒ ⇒ **扫全部格子**找「红 N 条」那个形态。
    for line in sec8_text.splitlines():
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

# --- 8. 裁决 H：两侧集合的关系 --------------------------------------------
m = re.search(r"pub const KINDS:\s*\[&'static str;\s*\d+\]\s*=\s*\[([^\]]*)\]", platform)
if not m:
    err("[裁决 H] 契约层读不到 `DeviceSpec::KINDS`")
else:
    rust_kinds = set(re.findall(r'"([^"]+)"', m.group(1)))
    m2 = re.search(r"pub const DEVICE_TYPES:[^=]*=\s*&\[(.*?)\n\];", types_rs, re.S)
    if not m2:
        err("[裁决 H] 内核侧读不到 `DEVICE_TYPES`")
    else:
        kernel_kinds = set(re.findall(r'kind:\s*"([^"]+)"', m2.group(1)))
        missing = rust_kinds - kernel_kinds
        if missing:
            err(f"[裁决 H] 描述能写但内核没实现：{sorted(missing)}")
        if "timer_riscv" not in kernel_kinds:
            err("[裁决 H] 内核注册表里没有 timer_riscv（裁决 J：它必须被如实报出来）")

# --- 9. 零新场景码（裁决 M）------------------------------------------------
error_rs = read(ERROR_REL) or ""
for m in re.finditer(r"E_PLT_(\d+)", error_rs):
    if int(m.group(1)) >= 17:
        err(f"[零新码] 契约层出现了 E_PLT_{m.group(1)}（本批**不**新增判据 ⇒ ⇒ 零新场景码）")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-43 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-43 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM_SCHEMA={EXPECT_VERSIONS['PLATFORM_SCHEMA_VERSION']}"
      f" / PLATFORM_RULES={EXPECT_VERSIONS['PLATFORM_RULES_VERSION']}")
sys.exit(0)
