#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""STAGE-52 冻结规格（v0.76 · 模板导入不兜底 + 引脚复用/pinmux）的自检。

用法：python3 tools/stage52_check.py docs/detailed-design/STAGE-52_V076_Pin_Mux_And_Template.md

⚠️ **十二项**（与 `tools/stage50_check.py` / `tools/stage51_check.py` 同构 ⇒ ⇒
   **不要**照抄它们的编号范围）：
    1. Markdown 表格列数一致（切分复用 `tools/doc_check.py` 的那一份）
    2. 标题层级不跳级
    3. 代码围栏配平
    4. 裁决编号 A~N 无悬空引用
    5. 章节 §1~§9 完整
    6. 验收编号交叉引用无未定义项
    7. 关键约束字面量钉住
       ＋ 7b. §1 的事实表编号列连续
       ＋ 7c. §7 的施工期订正：不是占位 + 变异编号齐全 + 每条有非空实测红集
       ＋ 7d. 版本号（本批**信封升版** ⇒ ⇒ `SCHEMA_VERSION` = 12 ·
          `SIM_STATE_SCHEMA_VERSION` 仍 2 · `PLATFORM_SCHEMA_VERSION` /
          `PLATFORM_RULES_VERSION` 走**跨批性质断言**）**且**前端镜像逐字相同
       ＋ 7e. 跨层源码锚点（契约词表 / 模板协议 / 内核 pinmux 路由 /
          前端模板库与只读卡 / 真机段）
       ＋ 7f. 台账必须登记本批（`v0.76` + `STAGE-52` + `D-175`）
       ＋ 7g. 脚本必须自指存在
    8. **本批升版**：描述格式版本 4 → 5 · 规则集 6 → 7 · **信封 11 → 12**
       ⇒ ⇒ 前端镜像四处必须一样
    9. **新场景码**：`E_PLT_22_NO_BOARD` / `E_PLT_23_PINMUX_INVALID`
       **唯一落点**在契约层 · `E_PLT-19` 语义扩展（引脚规格静态非法）
    10. **上一批的破坏性改动没有被回退**（STAGE-51 的 `mcu` + `McuSpec` +
        `isa_gate_error` + `E_PLT_20` / `E_PLT_21` 必须仍然在）
    11. **B3 摘除**：`auto_selectable` / `choose_description` 从契约层与
        内核调用点**彻底删除**（残留 ⇒ 红）
    12. **模板协议**：命令 / 事件四条 + `TOPIC_PLATFORM_TEMPLATES` 与前端
        `EVENT_TOPIC.PlatformTemplates` 逐字同

⚠️ **编号空间**：
    * **A 组** = 契约与跨层 ⇒ ⇒ **每批范围**（A-1 ~ A-7）
    * **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（K-1 ~ K-10）
    * **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（N-1 ~ N-5）
    * **B 组** = 真机门禁 ⇒ ⇒ **跨批次唯一**（B-168 ~ B-174，接 v0.75 批的 B-167）
    * **X 组** = 交叉 ⇒ ⇒ **每批范围**（X-1 ~ X-3）

⚠️ **变异编号空间 = T-1 ~ T-7**（`T-7` 是**对照** ⇒ ⇒ 必须**零红**）+ **N-4**
（前端顺手整改变异）⇒ ⇒ 与 `tools/mutation_v076.py` 的 `MUTATIONS` 表
**逐条对应**。
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from doc_check import split_row  # noqa: E402  (必须在 sys.path 调整之后)

spec = Path(sys.argv[1] if len(sys.argv) > 1 else
            "docs/detailed-design/STAGE-52_V076_Pin_Mux_And_Template.md")
if not spec.exists():
    print(f"✗ 找不到规格文件 {spec}")
    sys.exit(1)
lines = spec.read_text(encoding="utf-8").splitlines()
fail = []

REPO_ROOT = Path(__file__).resolve().parent.parent

#: ⚠️ 版本号的期望 —— **本批信封也升**（11 → 12 ⇒ ⇒ 协议形状变了必须升版）
#: ⇒ ⇒ `SCHEMA_VERSION` = 12 · `SIM_STATE_SCHEMA_VERSION` 仍 2 ⇒ ⇒ 这两个
#: 钉死是安全的；`PLATFORM_SCHEMA_VERSION` / `PLATFORM_RULES_VERSION` 走
#: **跨批性质断言**（∈ SUPPORTED / 只许升 ⇒ ⇒ 下一批合法升版不误红）。
EXPECT_VERSIONS = {
    "SCHEMA_VERSION": "12",
    "SIM_STATE_SCHEMA_VERSION": "2",
    "PLATFORM_SCHEMA_VERSION": None,
    "PLATFORM_RULES_VERSION": None,
}

LIB_REL = "crates/contract/src/lib.rs"
MODEL_REL = "crates/contract/src/model.rs"
PROTOCOL_REL = "crates/contract/src/protocol.rs"
ERROR_REL = "crates/contract/src/error.rs"
PLATFORM_REL = "crates/contract/src/platform.rs"
SESSION_REL = "crates/contract/src/session.rs"
HOST_REL = "crates/kernel/src/kernel_host/mod.rs"
GPIO_REL = "crates/kernel/src/m15_peripheral/gpio.rs"
PINMUX_REL = "crates/kernel/src/m15_peripheral/pinmux.rs"
KTEST_REL = "crates/kernel/src/kernel_host/tests_v076_templates.rs"
KTEST_RUNTIME_REL = "crates/kernel/src/kernel_host/tests_v076_pinmux_runtime.rs"
MIRROR_REL = "front/lib/message.js"
MMAP_REL = "front/views/components/memory-map.js"
M02_REL = "front/views/m02_viewcomponents.js"
APP_REL = "front/app.js"
TSVC_REL = "front/services/m76_templates.js"
NTEST_REL = "front/tests/memory-map-templates.test.mjs"
BROWSER_REL = "tools/browser-verify.mjs"
SMOKE_REL = "tools/wasm-smoke.mjs"
LEDGER_REL = "docs/reviews/REVIEWS.md"

DECISION_LETTERS = [chr(ord("A") + i) for i in range(14)]

EXPECT_ACCEPT = {
    "A": list(range(1, 8)),        # A-1 ~ A-7
    "K": list(range(1, 11)),       # K-1 ~ K-10
    "N": list(range(1, 6)),        # N-1 ~ N-5
    "B": list(range(168, 175)),    # B-168 ~ B-174
    "X": list(range(1, 4)),        # X-1 ~ X-3
}
KNOWN_ACCEPT = {
    "A": set(EXPECT_ACCEPT["A"]),
    "K": set(EXPECT_ACCEPT["K"]),
    "N": set(EXPECT_ACCEPT["N"]),
    # ⛔ **B 是跨批次唯一的** ⇒ ⇒ 前几批的 B-1 ~ B-167 也算「已定义」。
    "B": set(range(1, 175)),
    "X": set(EXPECT_ACCEPT["X"]),
}

#: ⚠️ 变异编号（§5 · T-1 ~ **T-7**；`T-7` 是对照 ⇒ ⇒ 零红）+ N-4。
MUTATION_IDS = list(range(1, 8))


def err(msg):
    fail.append(msg)


def read(rel):
    p = REPO_ROOT / rel
    if not p.exists():
        return None
    return p.read_text(encoding="utf-8")


def split_fences(src):
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
host_rs = read(HOST_REL) or ""
gpio_rs = read(GPIO_REL) or ""
pinmux_rs = read(PINMUX_REL) or ""
ktest = read(KTEST_REL) or ""
ktest_runtime = read(KTEST_RUNTIME_REL) or ""
mirror_js = read(MIRROR_REL) or ""
mmap_js = read(MMAP_REL) or ""
m02_js = read(M02_REL) or ""
app_js = read(APP_REL) or ""
tsvc_js = read(TSVC_REL) or ""
ntest_js = read(NTEST_REL) or ""
browser = read(BROWSER_REL) or ""
smoke_js = read(SMOKE_REL) or ""
ledger = read(LEDGER_REL) or ""

# 7d. 版本号（契约层 + 前端镜像）
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
        if name == "PLATFORM_SCHEMA_VERSION":
            supported = re.search(
                r"SUPPORTED_PLATFORM_SCHEMA_VERSIONS:\s*&\[u32\]\s*=\s*&\[([^\]]*)\]",
                platform_rs,
            )
            listed = [int(x) for x in re.findall(r"\d+", supported.group(1))] if supported else []
            if int(m.group(1)) not in listed:
                err(f"[版本] {name} = {m.group(1)} ∉ 可读版本列表 {listed} ⇒ ⇒ 性质破坏")
        else:
            if int(m.group(1)) < 7:
                err(f"[版本] {name} = {m.group(1)} < 7 ⇒ ⇒ 只许升不许降")
    elif m.group(1) != want:
        err(f"[版本] {name} = {m.group(1)} ≠ 规格的 {want}")
    if name not in text:
        err(f"[版本] 规格里没有提到 {name}")
m2 = re.search(r"SCHEMA_VERSION\s*=\s*['\"]?(\d+)", mirror_js)
if not m2:
    err("[版本] 前端镜像读不到 `SCHEMA_VERSION`")
elif m2.group(1) != EXPECT_VERSIONS["SCHEMA_VERSION"]:
    err(f"[版本] 前端镜像的 SCHEMA_VERSION = {m2.group(1)} ≠ 契约层的 "
        f"{EXPECT_VERSIONS['SCHEMA_VERSION']} ⇒ ⇒ 两边**不一致**")
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
ANCHORS = [
    # ⚠️ 契约：PinSpec 系列（裁决 E / F / G / L）
    (platform_rs, "pub struct PinSpec"),
    (platform_rs, "pub struct PinFunction"),
    (platform_rs, "pub struct PinIrq"),
    (platform_rs, "pub enum PinDirection"),
    (platform_rs, "pub fn parse_gpio_pin_name("),
    (platform_rs, "pub const KINDS"),
    (model_rs, "pub struct PlatformTemplateInfo"),
    # ⚠️ 契约：模板协议四条（裁决 M）
    (protocol_rs, "ListPlatformTemplates"),
    (protocol_rs, "GetPlatformTemplate"),
    (protocol_rs, "PlatformTemplates"),
    (protocol_rs, "PlatformTemplate"),
    # ⚠️ 新码唯一落点
    (error_rs, "E_PLT_22_NO_BOARD"),
    (error_rs, "E_PLT_23_PINMUX_INVALID"),
    # ⚠️ 内核：模板主题 + 清单 helper + 无板引导
    (host_rs, 'TOPIC_PLATFORM_TEMPLATES'),
    (host_rs, "platform_templates_event("),
    (host_rs, "请先从模板导入"),
    # ⚠️ 内核：pinmux 路由（2.5 分支）+ GPIO 锁存解耦 + pinmux 器件
    (host_rs, "pinmux_routes"),
    (gpio_rs, "self.pending_irqs |= changed & PIN_COUNT_MASK"),
    (pinmux_rs, "fn current_function("),
    (pinmux_rs, "fn set_function_count("),
    # ⚠️ 前端：模板库服务 / 渲染 / 只读卡 / load-template 接线
    (tsvc_js, "export function wireTemplates("),
    (tsvc_js, "async loadTemplate(id)"),
    (mmap_js, "function renderTemplateLibrary("),
    (mmap_js, "function renderPinsCard("),
    (mmap_js, 'data-pins-display'),
    (mmap_js, "已废弃（v0.76 · B3）"),
    (m02_js, 'case "load-template"'),
    (app_js, "templatesWiring"),
    (mirror_js, 'PlatformTemplates: "platform.templates"'),
    # ⚠️ 前端：模板测试 + 真机段 + 冒烟配板段
    (ntest_js, "data-tpl-library"),
    (browser, "templateLibrary"),
    (smoke_js, "ListPlatformTemplates"),
]
for src_, needle in ANCHORS:
    if src_ is None:
        err(f"[锚点] 文件缺失（{needle}）")
    elif needle not in src_:
        err(f"[锚点] 缺 {needle}")

# 7b. §2 现状勘察表必须有实质内容（本规格事实表不带 F- 编号 ⇒ ⇒ 改数行数）
sec2 = re.search(r"^## §2\b(.*?)(?=^## §3|\Z)", text, re.M | re.S)
if not sec2:
    err("[事实] 缺 §2 现状勘察")
else:
    fact_rows = [l for l in sec2.group(1).splitlines()
                 if l.strip().startswith("|") and not re.match(r"^\|\s*[-:]+\s*\|", l.strip())
                 and not re.match(r"^\|\s*事实\s*\|", l.strip())]
    if len(fact_rows) < 3:
        err(f"[事实] §2 现状勘察表太少（{len(fact_rows)} 行事实）⇒ ⇒ 勘察不是走过场")

# 7f. 台账登记
for needle in ("D-175", "STAGE-52", "v0.76"):
    if needle not in ledger:
        err(f"[台账] 台账里没有 {needle}")

# 7g. 脚本自指
if "stage52_check.py" not in text:
    err("[自指] 规格里没有提到本脚本")

# --- 7c. §7 的施工期订正 --------------------------------------------------
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
        if not any(re.search(r"[KNXA]-\d", c) for c in cells):
            if not any(("零红" in c) or ("不成立" in c) for c in cells):
                err(f"[订正] T-{tid} 的实测红集是空的 ⇒ ⇒ 那条变异零判别力"
                    f"（若它**确实不成立**，必须在规格里**明写**「零红 / 不成立」）")
    # ⚠️ N-4（前端顺手整改）也要有实测红集。
    for line in sec7_text.splitlines():
        if not re.match(r"^\|\s*\*?\*?N-4\*?\*?\s*\|", line):
            continue
        cells = [c.strip() for c in split_row(line.strip())]
        if not any(re.search(r"[KNXA]-\d", c) for c in cells):
            if not any(("零红" in c) or ("不成立" in c) for c in cells):
                err(f"[订正] N-4 的实测红集是空的 ⇒ ⇒ 那条变异零判别力")

# --- 8. 本批升版（文本侧确认）---------------------------------------------
for needle in ("4 → 5", "6 → 7", "11 → 12"):
    if needle not in text:
        err(f"[升版] 规格里没有写明 {needle}")

# --- 9. 新场景码 ----------------------------------------------------------
if "E_PLT_22_NO_BOARD" not in text:
    err("[新码] 规格里没有提到 `E_PLT_22_NO_BOARD`")
if "E_PLT_23_PINMUX_INVALID" not in text:
    err("[新码] 规格里没有提到 `E_PLT_23_PINMUX_INVALID`")

# --- 10. 上一批的破坏性改动没有被回退（STAGE-51）---------------------------
for needle in ("pub struct McuSpec", "McuIsaInfo", "isa_gate_error",
               "E_PLT_20_FW_ISA_MISMATCH", "E_PLT_21"):
    if needle not in (platform_rs + host_rs + error_rs):
        err(f"[回退] 缺 `{needle}` ⇒ ⇒ 上一批（STAGE-51）的破坏性改动**被回退了**")
# ⚠️ 可读版本列表必须收着 1 / 2 / 3 / 4（存量板档案零风险）。
m = re.search(r"SUPPORTED_PLATFORM_SCHEMA_VERSIONS:\s*&\[u32\]\s*=\s*&\[([^\]]*)\]",
              platform_rs)
if not m:
    err("[回退] 契约层读不到 `SUPPORTED_PLATFORM_SCHEMA_VERSIONS`")
else:
    listed = [int(x) for x in re.findall(r"\d+", m.group(1))]
    for old in (1, 2, 3, 4):
        if old not in listed:
            err(f"[回退] 可读版本列表 {listed} 里没有 {old} ⇒ ⇒ 存量板档案会被拒")

# --- 11. B3 摘除：auto_selectable / choose_description 必须彻底删除 ---------
# ⚠️ **豁免注释行**（契约层版本注释 / 前端 B3 决议注释**合法保留**——它们
#   是「已删除」的**证据**）⇒ ⇒ 只查**代码行**。
# ⚠️ 判据 = **字段声明**（`pub auto_selectable:`）/ **键赋值**（`auto_selectable:`）
# / **函数调用**（`choose_description(`）⇒ ⇒ 测试代码里的老键**字面量**（
# 老档案兼容测试 a2）与文档注释里的提及**合法保留**。
for src_, name in ((platform_rs, "契约层"), (model_rs, "契约层"), (mmap_js, "前端")):
    if "pub auto_selectable" in src_ or "auto_selectable:" in src_:
        err(f"[B3] {name}还有 `auto_selectable` 字段/键（B3 摘除不彻底）")
for src_, name in ((platform_rs, "契约层"), (host_rs, "内核")):
    if "fn choose_description" in src_ or "choose_description(" in src_:
        err(f"[B3] {name}还有 `choose_description` 函数/调用（自动选档链路没删干净）")

# --- 12. 模板协议主题逐字同 ------------------------------------------------
host_topic = re.search(r'TOPIC_PLATFORM_TEMPLATES:\s*&str\s*=\s*"([^"]+)"', host_rs)
js_topic = re.search(r"PlatformTemplates:\s*['\"]([^'\"]+)['\"]", mirror_js)
if not host_topic:
    err("[模板] 内核读不到 TOPIC_PLATFORM_TEMPLATES")
if not js_topic:
    err("[模板] 前端读不到 EVENT_TOPIC.PlatformTemplates")
elif host_topic and js_topic and host_topic.group(1) != js_topic.group(1):
    err(f"[模板] 主题串不一致：内核 '{host_topic.group(1)}' ≠ 前端 '{js_topic.group(1)}'")

# ---------------------------------------------------------------------------
# 结论
# ---------------------------------------------------------------------------
if fail:
    print(f"STAGE-52 规格自检：{len(fail)} 条失败")
    for f in fail:
        print(f"  ✗ {f}")
    sys.exit(1)

print("STAGE-52 规格自检：全过")
print(f"  裁决 {len(defined)} 条 · 变异 {len(MUTATION_IDS)} 条")
print(f"  验收：A {len(EXPECT_ACCEPT['A'])} · K {len(EXPECT_ACCEPT['K'])}"
      f" · N {len(EXPECT_ACCEPT['N'])} · B {len(EXPECT_ACCEPT['B'])}"
      f" · X {len(EXPECT_ACCEPT['X'])}")
print(f"  版本号：SCHEMA_VERSION={EXPECT_VERSIONS['SCHEMA_VERSION']}"
      f" / SIM_STATE={EXPECT_VERSIONS['SIM_STATE_SCHEMA_VERSION']}"
      f" / PLATFORM=性质断言（∈ SUPPORTED）"
      f" / PLATFORM_RULES=性质断言（只许升）")
print("  新场景码：E_PLT-22（无板装载）· E_PLT-23（pinmux 非法）—— 唯一落点已钉")
sys.exit(0)
