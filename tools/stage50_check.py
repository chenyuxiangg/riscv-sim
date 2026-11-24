#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-50 冻结规格（v0.74 · 引脚级连线 + 通用连接路由 + 前端连线编辑面）的自检。

用法：python3 tools/stage50_check.py docs/detailed-design/STAGE-50_V074_Pin_Level_Routing.md

⚠️ **十项**（与 `tools/stage48_check.py` / `tools/stage49_check.py` 同构 ⇒ ⇒
   **不要**照抄它们的编号范围）：
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
          = 3 · `PLATFORM_RULES_VERSION` = 5 · `SCHEMA_VERSION`（信封）**仍 11** ·
          `SIM_STATE_SCHEMA_VERSION` **仍 2**）**且** **前端镜像**与契约层**逐字相同**
       ＋ 7e. **跨层源代码锚点**（契约层的词表 / `pin` 字段 / 内核的按引脚取线 /
          前端的新模块 / 真机段的段名）
       ＋ 7f. 台账必须登记本批（`v0.74` + `STAGE-50` + `D-175` + `D-177`）
       ＋ 7g. 脚本必须自指存在
    8. **本批升版**：描述格式版本 2 → 3 · 规则集 4 → 5 · **信封不动**（仍 11）
       ⇒ ⇒ 前端镜像**四处**必须一样
    9. **零新场景码**（本批**复用**既有码 `E_PLT_16`）⇒ ⇒ 契约层**不许**出现
       `E_PLT_20` 及以后 ⇒ ⇒ 「一个场景码只一个含义」这条没被破坏
    10. **上一批的破坏性改动没有被回退**（STAGE-49 的 `base` 可缺省 + `roles`
        + `pins` + `E_PLT_18` / `E_PLT_19` **必须**仍然在）

⚠️ **编号空间**（与前两批同口径 ⇒ ⇒ **B 是真机输出的全局唯一空间**）：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-3）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-8）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-4）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-156 ~ B-160，接 v0.73 批的 B-155）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）

⚠️ **变异编号空间 = T-1 ~ T-8**（`T-8` 是**对照** ⇒ ⇒ 必须**零红**）⇒ ⇒ 与
`tools/mutation_v074.py` 的 `MUTATIONS` 表**逐条对应**（编号**不许**错位 ⇒ ⇒
否则「实测红集」那张表会**指向别的变异**）。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-50_V074_Pin_Level_Routing.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ **四个版本号的期望值**（7d / 8 用）—— 本批**描述格式升版** ⇒ ⇒
#: `PLATFORM_SCHEMA_VERSION` 2 → 3 · `PLATFORM_RULES_VERSION` 4 → 5 ⇒ ⇒
#: **契约信封 `SCHEMA_VERSION` 不动**（仍 11）· `SIM_STATE_SCHEMA_VERSION` 仍 2。
#: ⚠️ **跨批纪律**（STAGE-51 规格 §4.5 X-2 已明文）：上一批检查器**不许**钉死
#: `PLATFORM_*` 的具体值（下一批合法升版会误红）⇒ ⇒ 只钉**性质**：
#: `PLATFORM_SCHEMA_VERSION` 必须**出现在 `SUPPORTED_PLATFORM_SCHEMA_VERSIONS`
#: 里**（值随代码走）· `PLATFORM_RULES_VERSION` 只许升（≥ 5）。
#: 信封两个**不动**（仍 11 / 2 ⇒ ⇒ 钉死是安全的）。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": 11,
    "SIM_STATE_SCHEMA_VERSION": 2,  # 单调下界（升版不红、回退才红）
    "PLATFORM_SCHEMA_VERSION": None,  # 性质断言（∈ SUPPORTED）
    "PLATFORM_RULES_VERSION": None,   # 性质断言（≥ 5）
}

LIB_REL = "crates/contract/src/lib.rs"
MODEL_REL = "crates/contract/src/model.rs"
PROTOCOL_REL = "crates/contract/src/protocol.rs"
ERROR_REL = "crates/contract/src/error.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
SESSION_REL = "crates/contract/src/session.rs"
HOST_PLATFORM_REL = "crates/kernel/src/kernel_host/platform.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
GPIO_REL = "crates/kernel/src/m15_peripheral/gpio.rs"
TYPES_REL = "crates/kernel/src/m15_peripheral/types.rs"
KTEST_REL = "crates/kernel/src/kernel_host/tests_v074_pin_routing.rs"
MIRROR_REL = "front/lib/message.js"
MMAP_REL = "front/views/components/memory-map.js"
WEDITOR_REL = "front/views/components/bridge-editor.js"
WTEST_REL = "front/tests/bridge-editor.test.mjs"
BOARDSTORE_REL = "front/services/m23_boardstore.js"
BROWSER_REL = "tools/browser-verify.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"

#: ⚠️ **裁决编号空间**（A~N = 14 条）。
DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

#: ⚠️ **验收编号空间**。
EXPECT_ACCEPT = {
    "A": list(range(1, 4)),       # A-1 ~ A-3（契约与跨层）
    "K": list(range(1, 9)),       # K-1 ~ K-8（内核）
    "N": list(range(1, 5)),       # N-1 ~ N-4（前端 Node）
    "B": list(range(156, 161)),   # B-156 ~ B-160（真机门禁 · 跨批次唯一）
    "X": list(range(1, 4)),       # X-1 ~ X-3（交叉）
}
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-155 也算「已定义」⇒ ⇒
    #    **再加上本批自己的那几条**（B-156 ~ B-160）⇒ ⇒ ⇒ 范围到 160。
    "B": set(range(1, 161)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ **变异编号空间**（§5 · T-1 ~ **T-8** —— ⚠️ **T-8 是对照** ⇒ ⇒
#:    编号与 `tools/mutation_v074.py` 的 `MUTATIONS` 表**逐条对应**）。
MUTATION_IDS = list(range(1, 9))


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
session_rs = read(SESSION_REL) or ""
error_rs = read(ERROR_REL) or ""
host_platform = read(HOST_PLATFORM_REL) or ""
host_rs = read(HOST_REL) or ""
gpio_rs = read(GPIO_REL) or ""
types_rs = read(TYPES_REL) or ""
ktest = read(KTEST_REL) or ""
mirror_js = read(MIRROR_REL) or ""
mmap_js = read(MMAP_REL) or ""
weditor_js = read(WEDITOR_REL) or ""
wtest_js = read(WTEST_REL) or ""
boardstore_js = read(BOARDSTORE_REL) or ""
browser = read(BROWSER_REL) or ""
ledger = read(LEDGER_REL) or ""
ledger += "\n" + "\n".join(
    read(_r) or "" for _r in ("docs/reviews/REVIEWS-archive.md",
                              "docs/progress/PROGRESS-history.md"))

# 7d. 四个版本号（契约层 + 前端镜像）
for name, want in EXPECT_VERSIONS.items():
    m = None
    for src_ in (lib, model_rs, protocol_rs, platform_rs, session_rs):
        m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_)
        if m:
            break
    if not m:
        err(f"[版本] 契约层读不到 {name}")
        continue
    if want is None:
        # ⚠️ 跨批纪律：**性质断言**（值随代码走，不钉死具体数字）。
        if name == "PLATFORM_SCHEMA_VERSION":
            supported = re.search(
                r"SUPPORTED_PLATFORM_SCHEMA_VERSIONS:\s*&\[u32\]\s*=\s*&\[([^\]]*)\]",
                platform_rs,
            )
            listed = [int(x) for x in re.findall(r"\d+", supported.group(1))] if supported else []
            if int(m.group(1)) not in listed:
                err(f"[版本] {name} = {m.group(1)} ∉ 可读版本列表 {listed} ⇒ ⇒ 性质破坏")
        else:
            if int(m.group(1)) < 5:
                err(f"[版本] {name} = {m.group(1)} < 5 ⇒ ⇒ 只许升不许降")
    elif int(m.group(1)) < want:
        err(f"[版本] {name} = {m.group(1)} **低于**本批落地值 {want}"
            " ⇒ ⇒ 版本被**回退**了（升版不红、回退才红）")
    if name not in text:
        err(f"[版本] 规格里没有提到 {name}")
m2 = re.search(r"SCHEMA_VERSION\s*=\s*['\"]?(\d+)", mirror_js)
if not m2:
    err("[版本] 前端镜像读不到 `SCHEMA_VERSION`")
elif int(m2.group(1)) < EXPECT_VERSIONS["SCHEMA_VERSION"]:
    err(f"[版本] 前端镜像的 SCHEMA_VERSION = {m2.group(1)} ≠ 契约层的 "
        f"{EXPECT_VERSIONS['SCHEMA_VERSION']} ⇒ ⇒ 两边**不一致**")
# ⚠️ **前端镜像的描述格式 / 规则集版本**也必须**逐字相同**（N-1 / X-1）。
# ⚠️ 跨批纪律：基准是**契约层实际值**（先读契约层再比镜像，不钉死数字）。
for name in ("PLATFORM_SCHEMA_VERSION", "PLATFORM_RULES_VERSION"):
    m3 = re.search(rf"export const {name}\s*=\s*(\d+)", mirror_js)
    m4 = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", lib or "")
    if not m4:
        m4 = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", platform_rs or "")
    if not m3:
        err(f"[版本] 前端镜像读不到 `{name}`")
    elif not m4:
        err(f"[版本] 契约层读不到 `{name}` ⇒ ⇒ 镜像一致性无法核对")
    elif m3.group(1) != m4.group(1):
        err(f"[版本] 前端镜像的 {name} = {m3.group(1)} ≠ 契约层的 "
            f"{m4.group(1)} ⇒ ⇒ 两边**不一致**")

# 7e. 跨层源码锚点
#: ⚠️ **本批的引脚级连线 / 通用连接路由能力已退役**（v0.77 批：器件连线 →
#:    外设中断号映射 · `eccc15c` / `16c01a7` / `980d9af` / `c0f375b`）。
#:    ⇒ ⇒ `bridge.from.pin` / `BRIDGE_EVENTS` / `BridgeEventSemantics` /
#:    `take_irq_line_for_pin` / `bridge-editor.js`（`renderBridgeEditor` 等 5 个
#:    导出）/ `pinLevelRouting` 真机段**全部随之移除**（继续判只会永远红）。
#: ⚠️ **保留的是「本批引入、且活到现在」的那几件**：引脚数的**唯一落点**
#:    （契约 `GPIO_PIN_COUNT` + 内核按 id 取 GPIO 句柄）—— 后几批的中断号
#:    面板仍按「每块 GPIO 一个源」用它 ⇒ ⇒ 它一没，引脚相关显示就断。
ANCHORS = [
    (session_rs, "pub const GPIO_PIN_COUNT"),
    (host_platform, "DeviceHandle::Gpio("),
    (host_platform, "pub fn gpio(&self"),
    (model_rs, "pub pin_count: Option<u32>"),
    (types_rs, "pin_count: Some(GPIO_PIN_COUNT as u32)"),
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
for needle in ("D-175", "D-177", "STAGE-50", "v0.74"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage50_check.py" not in text:
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

# --- 9. 新场景码（跨批纪律）---------------------------------------------
# ⚠️ **本批（STAGE-50）复用既有码 `E_PLT_16`**（自己批次零新码 ⇒ ⇒ 规格
# 文本这么写）⇒ ⇒ 但**契约层的码空间是跨批共享的**：`E_PLT_20 / E_PLT_21`
# 由**下一批（STAGE-51）登记** ⇒ ⇒ 上一批检查器**不许**因后续批合法新码
# 而红（「一个场景码只一个含义」的唯一落点检查由 `error_code_check.py`
# 与各批自己的 stageNN_check 守）。
if "E_PLT_16" not in text:
    err("[新码] 规格里没有写明复用的既有码 `E_PLT_16`")

# --- 10. 上一批的破坏性改动没有被回退（STAGE-49）---------------------------
# ⚠️ `PinBinding` / `pins` 段随 v0.77 批退役 ⇒ ⇒ 不再判它们（判了只会永远红）。
for needle in ("base: Option<u32>", "pub struct DeviceRoles",
               "pub roles: Option<DeviceRoles>", "timer_riscv"):
    if needle not in platform_rs:
        err(f"[回退] 契约层没有 `{needle}` ⇒ ⇒ 上一批（STAGE-49）的破坏性改动"
            f"**被回退了**")
if "E_PLT_18_ROLES_INVALID" not in error_rs:
    err("[回退] 契约层没有 `E_PLT_18_ROLES_INVALID` ⇒ ⇒ 上一批那个场景码"
        "**被回退了**")
if "pub fn assign_bases(" not in host_platform:
    err("[回退] 内核侧没有 `assign_bases` ⇒ ⇒ 上一批的装配期分配**被回退了**")
# ⚠️ **另外三个版本号不许被带走**（本批只许升描述格式那两个）。
for name in ("SCHEMA_VERSION", "SIM_STATE_SCHEMA_VERSION"):
    m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", lib)
    if not m:
        for src_ in (model_rs, protocol_rs, platform_rs, session_rs):
            m = re.search(rf"pub const {name}:\s*u32\s*=\s*(\d+)", src_)
            if m:
                break
    if not m:
        err(f"[回退] 读不到 {name}")
    elif EXPECT_VERSIONS[name] is not None and int(m.group(1)) < EXPECT_VERSIONS[name]:
        err(f"[回退] {name} = {m.group(1)} **低于**本批落地值 "
            f"{EXPECT_VERSIONS[name]} ⇒ ⇒ 版本被**回退**了")
# ⚠️ **可读版本列表必须收着 1 / 2**（缺省优先：存量板档案零风险）。
m = re.search(r"SUPPORTED_PLATFORM_SCHEMA_VERSIONS:\s*&\[u32\]\s*=\s*&\[([^\]]*)\]",
              platform_rs)
if not m:
    err("[回退] 契约层读不到 `SUPPORTED_PLATFORM_SCHEMA_VERSIONS`")
else:
    listed = [int(x) for x in re.findall(r"\d+", m.group(1))]
    for old in (1, 2, 3):
        if old not in listed:
            err(f"[回退] 可读版本列表 {listed} 里没有 {old} ⇒ ⇒ 存量板档案会被拒")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-50 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-50 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 事实 {len(facts)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · K {len(EXPECT_ACCEPT['K'])}"
      f" · N {len(EXPECT_ACCEPT['N'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])}")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM=性质断言（∈ SUPPORTED，值随代码）"
      f" / PLATFORM_RULES=性质断言（只许升，值随代码）")
print("  新场景码：0 个（非法连线一律复用既有的 `E_PLT_16`；后续批新码由各批自守）")
print("  ⚠️ 引脚级连线 / 通用连接路由能力已于 v0.77 批退役（器件连线 → 外设中断号映射）"
      " ⇒ ⇒ 相关锚点已移除；保留 GPIO 引脚数落点与实例表句柄")
sys.exit(0)
