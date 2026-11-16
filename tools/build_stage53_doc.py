#!/usr/bin/env python3
"""STAGE-53 设计文档构建脚本（M-13 v1.1 版式）。

数据驱动：正文里的每一张表 / 每一段代码 / 每一个章节都先落成 Python 结构，
再统一渲染成单文件 HTML。改内容只改结构，不碰渲染代码。

用法：
    python3 tools/build_stage53_doc.py
产物：
    docs/detailed-design/STAGE-53_V077_No_Region_Budget.html
"""

from __future__ import annotations

import html
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "detailed-design" / "STAGE-53_V077_No_Region_Budget.html"

DOC = {
    "title": "STAGE-53 · V077 · 预算概念退役（段自己的 size 就是上限）",
    "subtitle": "把 region_budget_bytes / max_region_bytes 两个“预算”概念从契约、内核、前端一并拿掉",
    "version": "v0.77 · 契约描述格式 PLATFORM_SCHEMA_VERSION 5 → 6 · 信封 SCHEMA_VERSION 15 → 16",
}

# ---------------------------------------------------------------------------
# 表格（数据）
# ---------------------------------------------------------------------
TABLES: dict[str, dict] = {
    "t1": {
        "caption": "表 1 · 16 MiB 住在哪（四个落点，全部是“默认值”）",
        "widths": ["18%", "30%", "20%", "32%"],
        "cols": ["落点", "位置", "当前值", "性质"],
        "rows": [
            ["M-14 总线护栏", "crates/kernel/src/m14_bus/mod.rs:79", "DEFAULT_MAX_REGION_BYTES = 16 MiB", "**真正的源头**：D-10 的防 OOM 默认值"],
            ["描述字段（声明期）", "crates/contract/src/platform.rs:256", "region_budget_bytes: u32（必填）", "把总线默认值搬到板上"],
            ["描述字段（运行期）", "crates/contract/src/platform.rs:257-259", "max_region_bytes: Option<u32>（缺省 = 上一行的值）", "同一件事的第二个旋钮"],
            ["前端内置模板", "front/views/components/product-workspace.js:85 / 110 / 326", "0x01000000（tpl-min / tpl-soc / defaultDraft）", "手抄上两行的常量"],
        ],
    },
    "t2": {
        "caption": "表 2 · 这条默认值是怎么把人卡死的（因果链，逐环可验）",
        "widths": ["8%", "38%", "54%"],
        "cols": ["环", "发生了什么", "落点证据"],
        "rows": [
            ["①", "tpl-soc 的 RAM 只有 2 MiB，而 osz.elf 要 0x80000000 起 64 MiB+ 的地址", "elf program headers：0x83c00000 / 0x84000000 两段"],
            ["②", "把段长度改成 64 MiB ⇒ 超过预算 16 MiB", "V-7：size > region_budget_bytes"],
            ["③", "SetMemoryMap 跑 validate() ⇒ E_PLT-07 ⇒ 什么都不动", "kernel_host/mod.rs:3038"],
            ["④", "草稿永远是“未生效”，界面只给页脚一行“内存图流不可用”", "app.js:2035-2040、index.html:143"],
            ["⑤", "同样的描述存成档案 ⇒ 从模版加载仍被同一条拒", "app.js:1803-1840、1819-1826"],
        ],
    },
    "t3": {
        "caption": "表 3 · 本批裁决（A~F）",
        "widths": ["6%", "24%", "38%", "32%"],
        "cols": ["裁决", "内容", "为什么这么定", "代价 / 落点"],
        "rows": [
            ["A", "**预算概念整体退役**：删 region_budget_bytes 与 max_region_bytes 两个字段、删 V-7、删 MemoryMapOp::SetBudget", "预算不是板的属性，是段的属性；段的 size 本来就是容量，再给一个独立旋钮必然出现“预算 < 段长”的自相矛盾板", "契约 + 内核 + 前端三处同批改"],
            ["B", "**M-14 不再有全局默认**：`max_region_bytes` 字段与 `DEFAULT_MAX_REGION_BYTES` 一并删除，上限**取自各段自己的 size**", "只删描述字段会把 16 MiB 留在 Bus 里：装得进校验、装不进运行期（E_BUS-09），问题只是从装载期挪到运行期", "M-14 内部，契约零影响"],
            ["C", "**E_PLT-07 保留号、标 retired**；`assign_bases` 那处“缺基址段装不下”改报 `E_PLT-05`（溢出）", "码表不重排，历史日志/文档里的 E_PLT-07 仍可解释；装不下本来就是地址空间溢出", "error_code_check.py 清单标 retired"],
            ["D", "**E_BUS-09 保留、语义收紧**：从“超过宿主预算”改成“某段给的数据（init / 快照页）超过**该段自己声明的 size**”", "护栏不能凭空消失，但它的判据必须跟着段走，不能再来自一个板上旋钮", "m14_bus 六处 raise 点逐个改判据"],
            ["E", "**版本号**：描述 5 → 6（少两个键）、信封 15 → 16（少一个 MemoryMapOp 变体）；描述侧 SUPPORTED = [1..6] **跨版本接受**", "老档案（带两个预算键）被 serde 忽略即可读入 ⇒ 用户手里那块“16 MiB 预算 + 64 MiB 段”的板**自动变合法**，不需要迁移脚本", "一次联调；发行说明写“内核与前端必须同批升级”"],
            ["F", "**前端一并收口**：删三处常量、删预算输入框、**裁掉三个没挂载的渲染函数**、补错误横幅、存完刷新档案列表", "今次卡住用户的不是代码 bug，是“入口在死代码里 + 错误只写页脚”", "前端净减代码；无新视图"],
        ],
    },
    "t4": {
        "caption": "表 4 · 契约层字段处置（PlatformDescription）",
        "widths": ["20%", "10%", "12%", "58%"],
        "cols": ["字段", "处置", "类型", "说明"],
        "rows": [
            ["region_budget_bytes", "**删除**", "u32（必填）", "声明期预算；V-7 的唯一输入；删后无任何“每段上限”概念"],
            ["max_region_bytes", "**删除**", "Option<u32>", "运行期预算；缺省时回落成上一行 ⇒ 两者一起删才干净"],
            ["schema_version", "保留", "u32", "本批写入值 = 6"],
            ["id / name / priority", "保留", "String / String / i32", "板身份，不动"],
            ["regions", "保留", "Vec<RegionSpec>", "RAM/ROM 靠 size 表达容量，语义不变"],
            ["description / roles / mcu", "保留", "可选", "v0.70~v0.75 引入，不动"],
        ],
    },
    "t5": {
        "caption": "表 5 · 错误码处置",
        "widths": ["12%", "10%", "18%", "60%"],
        "cols": ["码", "值", "处置", "说明"],
        "rows": [
            ["E_PLT-07", "7", "**retired（保留号）**", "原有 raise 点两处：V-7（契约，删）、assign_bases（改成 E_PLT-05）；常量保留、加 `#[deprecated]` 注释，码表不重排"],
            ["E_PLT-05", "5", "**扩大使用面**", "地址空间装不下（base + size 溢出 / assign_bases 找不到空隙）—— 既有语义，不新增码"],
            ["E_PLT-06", "6", "不动", "段间重叠"],
            ["E_PLT-04", "4", "不动", "size == 0"],
            ["E_BUS-09", "9", "**语义收紧**", "测什么：RAM 的物理占用（页）/ ROM 与外设窗口的声明大小；上限来源从“宿主预算”改成**该段自己的 size**"],
        ],
    },
    "t6": {
        "caption": "表 6 · 版本号与兼容矩阵",
        "widths": ["22%", "14%", "14%", "50%"],
        "cols": ["版本量", "现在", "本批", "兼容策略"],
        "rows": [
            ["PLATFORM_SCHEMA_VERSION（描述形状）", "5", "6", "写入 6；读取接受 [1..6]"],
            ["SUPPORTED_PLATFORM_SCHEMA_VERSIONS", "[1..5]", "[1..6]", "老档案（schema ≤ 5，带两个预算键）逐字可读"],
            ["SCHEMA_VERSION（信封）", "15", "16", "少一个 MemoryMapOp 变体属于线上破坏性变更"],
            ["PLATFORM_RULES_VERSION（板规则）", "3", "3", "规则版本不跟着形状走，不动"],
            ["SIM_STATE_SCHEMA_VERSION（快照）", "2", "2", "快照里没有预算字段，不动"],
        ],
    },
    "t7": {
        "caption": "表 7 · 老档案兼容矩阵（读出后落到哪）",
        "widths": ["26%", "20%", "22%", "32%"],
        "cols": ["档案里的内容", "读出结果", "是否可装载", "说明"],
        "rows": [
            ["schema 5，预算 16 MiB，RAM 2 MiB", "忽略预算键，按 6 读入", "✅", "与今天行为等价"],
            ["schema 5，预算 16 MiB，RAM 64 MiB", "忽略预算键，按 6 读入", "✅ **（本批的核心收益）**", "今天被 E_PLT-07 拒；改后直接合法"],
            ["schema 5，max_region_bytes 显式值", "忽略该键", "✅", "运行期上限已改为段自己 size"],
            ["schema 6（本批写的）", "正常读入", "✅", "明细里没有预算键"],
            ["schema < 5 的老档案", "按各自版本读入（仅忽略两个预算键）", "✅", "其余键集差异按既有 V-2 规则"],
        ],
    },
    "t8": {
        "caption": "表 8 · 内核落点清单（一处落点一条）",
        "widths": ["34%", "30%", "36%"],
        "cols": ["文件:行", "今天是什么", "本批改成"],
        "rows": [
            ["crates/contract/src/platform.rs:739-756", "V-7：size > region_budget_bytes ⇒ E_PLT-07", "**整段删除**"],
            ["crates/contract/src/platform.rs:256", "region_budget_bytes 字段", "删除（含 serde 键）"],
            ["crates/contract/src/platform.rs:257-272", "max_region_bytes 字段与 document", "删除"],
            ["crates/contract/src/platform.rs:54 / 1765 / 2133", "常量定义、用例断言、码表条目", "常量标 retired、用例改写、码表标 retired"],
            ["crates/contract/src/protocol.rs:526", "MemoryMapOp::SetBudget 变体", "删除"],
            ["crates/contract/src/protocol.rs:490 / 2979-2985", "示例与往返用例", "删除 / 改成“该变体不存在”的负向用例"],
            ["crates/kernel/src/kernel_host/mod.rs:5029-5036", "SetBudget 合成两个预算字段", "删除该分支"],
            ["crates/kernel/src/kernel_host/platform.rs:559", "bus.set_max_region_bytes(desc.max_region_bytes ?? desc.region_budget_bytes)", "整段删除（Bus 不再有全局上限）"],
            ["crates/kernel/src/kernel_host/platform.rs:841", "assign_bases 装不下 ⇒ E_PLT-07", "改报 E_PLT-05"],
            ["crates/kernel/src/m14_bus/mod.rs:79", "DEFAULT_MAX_REGION_BYTES = 16 MiB", "删除常量"],
            ["crates/kernel/src/m14_bus/mod.rs:762 / 810-830", "max_region_bytes 字段与 getter/setter/构造器", "删除（含文档）"],
            ["crates/kernel/src/m14_bus/mod.rs:841-870", "map_ram / map_rom 用全局 cap", "上限改用各段自己的 size"],
            ["crates/kernel/src/m14_bus/mod.rs:973-980", "外设窗口 size > cap ⇒ E_BUS-09", "判据改为 size > 地址空间余量（窗口大小来自 M-15 注册表，不来自描述）"],
            ["crates/kernel/src/m14_bus/mod.rs:1003-1015 / 605-615 / 1368 / 1676 / 2325", "其余 E_BUS-09 raise 点", "判据统一改成“相对本段 size”"],
        ],
    },
    "t9": {
        "caption": "表 9 · M-14 新判据（按段类型，逐条可测）",
        "widths": ["14%", "26%", "26%", "34%"],
        "cols": ["段类型", "测什么", "上限取自", "越过时报什么"],
        "rows": [
            ["RAM（惰性页）", "物化页字节数", "**该段自己的 size**", "写越段尾 ⇒ E_BUS-01（未映射）；init 大于 size ⇒ E_BUS-09"],
            ["ROM", "声明大小", "**该段自己的 size**", "init 大于 size ⇒ E_BUS-09"],
            ["外设窗口", "声明大小", "M-15 注册表的 window_bytes（固定值）", "与注册表不符 ⇒ 既有码；不存在“按板上预算卡”这一步"],
            ["快照还原", "某段的页数换算字节", "**该段自己的 size**", "超了 ⇒ E_BUS-09（坏快照/坏数据）"],
        ],
    },
    "t10": {
        "caption": "表 10 · 前端落点清单",
        "widths": ["34%", "30%", "36%"],
        "cols": ["文件:行", "今天是什么", "本批改成"],
        "rows": [
            ["front/views/components/product-workspace.js:85", "tpl-min 的 region_budget_bytes: 0x01000000", "删除该键"],
            ["front/views/components/product-workspace.js:110", "tpl-soc 的同一常量", "删除该键"],
            ["front/views/components/product-workspace.js:326", "defaultDraft() 的同一常量", "删除该键"],
            ["front/views/components/memory-map.js:2469-2474", "data-mm-budget 输入框（死代码）", "随渲染函数一起删除"],
            ["front/views/components/memory-map.js:2351 / 1679 / 2237", "renderMemoryMapPanel / renderArchivePanel / renderTemplateLibrary", "**整函数删除**（视图注册表里没有 memory-map，三者无调用点）"],
            ["front/views/components/memory-map.js:521 / 527", "draftToDescription / 描述归一化写预算", "不再写这两个键；老档案里若有 ⇒ 提交前丢弃"],
            ["front/lib/message.js:186 / 200", "PLATFORM_SCHEMA_VERSION = 5、SUPPORTED = [1..5]", "改成 6 / [1..6]"],
            ["front/app.js:2035-2040", "catch 里一律 setNote('内存图流不可用')", "按错误码分类 + 工作面横幅（表 11）"],
            ["front/app.js:1836-1839", "另存为成功后不刷新列表", "落库成功后调 refreshProductArchive"],
        ],
    },
    "t11": {
        "caption": "表 11 · 错误呈现矩阵（替代“页脚一行小字”）",
        "widths": ["18%", "26%", "56%"],
        "cols": ["错误类", "界面表现", "文案要点"],
        "rows": [
            ["描述校验类（E_PLT-04/05/06/12/20）", "工作面顶部红色横幅 + 保留页脚", "“这份板没通过校验：<码> <原因>。草稿已保留，改完再点「重新载入」。”"],
            ["版本/形状类（schema 不在 [1..6]）", "横幅 + 引导", "“这块板是格式 vN，当前只接受 [1..6] —— 从模板重新导入一份。”"],
            ["修订号冲突（E_PLT-13）", "横幅 + 刷新建议", "“板级配置已在别处被改过（修订号 N ≠ M），先看当前生效图再重提交。”"],
            ["无板装载（E_PLT-22）", "横幅 + 模板库入口", "既有文案，不动"],
            ["未知系统（E_PLT-17）", "横幅 + 差额区间", "既有文案（会说差在哪一段），不动"],
        ],
    },
    "t12": {
        "caption": "表 12 · 判据矩阵（内核 K-*，cargo test）",
        "widths": ["8%", "44%", "48%"],
        "cols": ["编号", "判据", "怎么算过"],
        "rows": [
            ["K-1", "不带两个预算键的描述能 serialize→deserialize 往返", "逐字段相等；键集不含 budget 两键"],
            ["K-2", "schema 5 老描述（带两个预算键）能被读入", "读入成功，两键被忽略，其余字段逐字保留"],
            ["K-3", "64 MiB RAM 段的板 validate() 通过", "不再出现 E_PLT-07"],
            ["K-4", "同一块板建图成功（Bus 无 16 MiB 墙）", "map_ram 返回 Ok；不再出现 E_BUS-09"],
            ["K-5", "seg size 恰好 4 GiB−base 的极限值", "V-5 溢出判据仍在（E_PLT-05），不是 E_PLT-07"],
            ["K-6", "init 字节数大于本段 size", "E_BUS-09（新判据仍能报）"],
            ["K-7", "缺基址的外设段在地址空间装不下", "E_PLT-05（**不是** E_PLT-07）"],
            ["K-8", "PatchMemoryMap 载荷里出现 SetBudget", "serde 拒（不存在的变体）⇒ 信封错误码"],
            ["K-9", "E_PLT-07 常量仍存在但零 raise 点", "全仓 grep 只剩定义与 retired 注释"],
            ["K-10", "存量 52 条涉及预算的用例逐条处置", "每条在 §9 登记为“改写 / 退役”，无静默删除"],
        ],
    },
    "t13": {
        "caption": "表 13 · 判据矩阵（前端 N-*，node:test）",
        "widths": ["8%", "44%", "48%"],
        "cols": ["编号", "判据", "怎么算过"],
        "rows": [
            ["N-1", "三处内置模板不再带预算键", "JSON 里两个键都不存在"],
            ["N-2", "defaultDraft() 不再带预算键", "同上"],
            ["N-3", "三个死渲染函数已删除", "grep 零命中（含导出名）"],
            ["N-4", "校验失败出横幅", "页面上存在 `[data-board-error-banner]` 且码在文案里"],
            ["N-5", "另存为成功后档案列表当次刷新", "store 里 2 条 ⇒ 弹层里也 2 条"],
            ["N-6", "提交载荷不含两个预算键", "发出的 SetMemoryMap 描述键集精确固定"],
            ["N-7", "老档案（schema 5 带预算）导入后可提交", "载入 → 提交成功，载荷已丢两键"],
        ],
    },
    "t14": {
        "caption": "表 14 · 真机段（B-*，browser-verify.mjs）",
        "widths": ["8%", "44%", "48%"],
        "cols": ["编号", "判据", "怎么算过"],
        "rows": [
            ["B-1", "tpl-soc 段长度改成 128 MiB ⇒ 重新载入成功", "修订号 +1、无 E_PLT-07、脏标记清零"],
            ["B-2", "装载 osz.elf", "不再出 E_PLT-17（板够大之后）"],
            ["B-3", "从模版加载一份老档案", "装载成功，页面无预算字段"],
            ["B-4", "构造两段重叠 ⇒ 重新载入", "横幅出现 E_PLT-06，草稿保留"],
        ],
    },
    "t15": {
        "caption": "表 15 · 变异自检（改坏一处，必须精确变红）",
        "widths": ["6%", "34%", "26%", "34%"],
        "cols": ["编号", "变异", "期望红", "说明"],
        "rows": [
            ["T-1", "把 Bus 的上限退回常量 16 MiB", "K-4、K-5、B-1", "证明“默认值真的没了”"],
            ["T-2", "V-7 判据留着（只删字段）", "K-3", "证明校验期不再有预算"],
            ["T-3", "SUPPORTED 去掉 5", "K-2、B-3", "证明老档案兼容靠的是名单，不是巧合"],
            ["T-4", "把 SetBudget 变体加回", "K-8", "证明线上形状变了"],
            ["T-5", "前端模板表写回预算键", "N-1、N-6", "证明前端没有第二个真相"],
            ["T-6", "assign_bases 仍报 E_PLT-07", "K-7", "证明码的处置落到实点"],
        ],
    },
    "t16": {
        "caption": "表 16 · 破坏性变更（发行说明逐条抄）",
        "widths": ["8%", "34%", "58%"],
        "cols": ["#", "变更", "影响与要求"],
        "rows": [
            ["1", "描述少两个键（region_budget_bytes / max_region_bytes）", "老前端读到新描述会因缺必填字段失败 ⇒ **内核与前端必须同批升级**"],
            ["2", "信封少一个变体（MemoryMapOp::SetBudget）", "老前端若发该操作 ⇒ serde 拒绝；当前前端无发送点（死代码），风险为零"],
            ["3", "E_PLT-07 退役", "码表不变、号不重排；断言该码的旧脚本会恒红 ⇒ 逐条改判 E_PLT-05"],
            ["4", "E_BUS-09 判据收紧", "原来“超预算”的用例改判“超本段 size”"],
            ["5", "板档案", "无需迁移；老档案直接可读（表 7）"],
            ["6", "快照（SIM_STATE）", "不含预算字段，零影响"],
        ],
    },
    "t17": {
        "caption": "表 17 · 验收清单（施工完逐条勾）",
        "widths": ["6%", "94%"],
        "cols": ["#", "项"],
        "rows": [
            ["1", "契约：两字段删除、V-7 删除、SetBudget 删除、版本 6/16、SUPPORTED [1..6]"],
            ["2", "内核：Bus 无全局上限、E_BUS-09 判据跟段走、assign_bases 改报 E_PLT-05"],
            ["3", "前端：三处常量删除、三个死函数删除、错误横幅、档案刷新"],
            ["4", "`cargo test` 全绿；`npm run test:front` 全绿；`tools/error_code_check.py` 零红"],
            ["5", "`tools/stage14_check.py` / `tools/stage16_check.py` 的键名清单同步（删 budget 两键）"],
            ["6", "真机四段（表 14）全过；变异六条（表 15）精确变红且逐字节还原"],
            ["7", "USER_GUIDE 的 V-7 说明改成“预算已退役”并写明段 size 即容量"],
        ],
    },
    "t18": {
        "caption": "表 18 · 风险与遗留",
        "widths": ["10%", "32%", "58%"],
        "cols": ["等级", "风险", "处置"],
        "rows": [
            ["中", "防 OOM 从“板级预算”退化为“段 size 即上限”", "RAM 惰性分页 + 单段 size 由用户显式声明 ⇒ 上限仍存在；后续批可加“总占用告警”"],
            ["低", "老脚本断言 E_PLT-07", "表 5 明确 retired；门禁脚本同批改"],
            ["低", "`memory-map` 视图彻底消失后想再要独立板编辑面", "本批明确不做；需要时按新需求单独立项（不要复活死代码）"],
            ["中", "内核/前端跨版本混跑", "表 16 第 1 条硬约束：发行说明写死“必须同批升级”"],
        ],
    },
}

# ---------------------------------------------------------------------------
# 代码块（数据）
# ---------------------------------------------------------------------
CODE: dict[str, dict] = {
    "c1": {
        "title": "描述形状：少两个键（示意，最终键集以 PlatformDescription 的 serde 形状 + A-2 键集用例为准）",
        "text": """// 今天（PLATFORM_SCHEMA_VERSION = 5）
{
  "schema_version": 5,
  "id": "tpl-soc",
  "name": "带中断的 SoC",
  "priority": 100,
  "region_budget_bytes": 16777216,   // ← 删
  "max_region_bytes": null,          // ← 删
  "regions": [ { "kind": "ram", "base": 2147483648, "size": 2097152 }, ... ]
}

// 本批（PLATFORM_SCHEMA_VERSION = 6）
{
  "schema_version": 6,
  "id": "tpl-soc",
  "name": "带中断的 SoC",
  "priority": 100,
  "regions": [ { "kind": "ram", "base": 2147483648, "size": 134217728 }, ... ]
}""",
    },
    "c2": {
        "title": "V-7（删）与 assign_bases（改码）",
        "text": """// crates/contract/src/platform.rs:739-756 —— 整段删除
for region in &self.regions {
    if let Some(size) = region.declared_size() {
        if size > self.region_budget_bytes {          // ← 唯一输入已不存在
            return Err(E_PLT_07_OVER_BUDGET, ...);     // ← retired
        }
    }
}

// crates/kernel/src/kernel_host/platform.rs:841 —— 改报既有码
let Some(base) = placed else {
    return Err(PlatformError::new(
        PLATFORM_FAMILY,
        E_PLT_05_OVERFLOW,          // 原 E_PLT_07_OVER_BUDGET
        format!("区域（缺基址）的大小 {:#x} 在地址空间里装不下", size),
    ));
};""",
    },
    "c3": {
        "title": "M-14：上限从“板上旋钮”改成“段自己的 size”",
        "text": """// crates/kernel/src/m14_bus/mod.rs —— 删除
pub const DEFAULT_MAX_REGION_BYTES: u64 = 16 * 1024 * 1024;  // ← 16 MiB 的真身
struct Bus { max_region_bytes: u64, ... }                    // ← 字段删除
pub fn with_max_region_bytes(u64) -> Self                    // ← 构造器删除
pub fn set_max_region_bytes(&mut self, u64)                  // ← setter 删除

// 判据改成“入参自己的 size”
pub fn map_ram(&mut self, base: u32, size: u32, init: &[u8]) -> Result<(), PlatformError> {
    let cap = u64::from(size);            // 原：self.max_region_bytes
    ...
}

// 初始化数据超过本段容量 ⇒ 仍是 E_BUS-09（防坏数据，不防“板太小”）
let pages = init.len().div_ceil(MEM_PAGE_BYTES);
if (pages as u64) * MEM_PAGE_BYTES as u64 > u64::from(size) { return Err(E_BUS_09); }""",
    },
    "c4": {
        "title": "前端：三个渲染函数整段删除（视图注册表里从来没有它们）",
        "text": """// front/views/components/memory-map.js
- export function renderMemoryMapPanel(state)   // 2351 行，无任何 import
- export function renderArchivePanel(state)     // 1679 行，仅被上面那个调用
- function renderTemplateLibrary(state)         // 2237 行，仅被上面那个调用

// 保留的纯函数（app.js / m02 真正在用的）
export { draftToDescription, planMemoryMapUpdate, splitErrorCode, addRegion, parseNumber, ... }""",
    },
}

# ---------------------------------------------------------------------------
# 章节（数据）
# ---------------------------------------------------------------------


def body(*items):
    return list(items)


def p(text):
    return ("p", text)


def tbl(key, caption_override=None):
    return ("tbl", (key, caption_override))


def code(key):
    return ("code", key)


def flow():
    return ("flow", None)


def lead(text):
    return ("lead", text)


def checklist(items):
    return ("check", items)


SECTIONS = [
    {
        "id": "s1",
        "title": "§1 一句话：这一批到底改了什么",
        "body": body(
            lead("**“预算”这个概念整体退役**：不再有 `region_budget_bytes`、不再有 `max_region_bytes`、不再有 V-7、不再有 `MemoryMapOp::SetBudget`。**一段内存能有多大，只由它自己的 `size` 说了算。**"),
            p("今天的三层事实必须一起改，缺一层都不叫根治：① 描述里的声明期预算（V-7 的唯一输入）；② 同一个概念的第二个旋钮 `max_region_bytes`；③ **M-14 总线里那个 16 MiB 的真身** `DEFAULT_MAX_REGION_BYTES`。只删 ①② 会把失败从装载期挪到运行期（E_BUS-09），用户看到的仍是“装不进”。"),
            p("收益：那块“16 MiB 预算 + 64 MiB 段”的板不再自相矛盾——它本来就是同一个字段的两种说法。用户不必再算预算，也不会再出现“改得动、存不下、装不进”的三段式卡点。"),
        ),
    },
    {
        "id": "s2",
        "title": "§2 现状：16 MiB 住在哪，怎么把人卡死",
        "body": body(
            p("四个落点全是“默认值”，没有一个是硬件事实或产品约束："),
            tbl("t1"),
            p("典型踩坑链路（上一轮实测，逐环可验）："),
            tbl("t2"),
            p("注意第 ③ 环：`SetMemoryMap` / `PatchMemoryMap` 都会跑 `description.validate()`（`kernel_host/mod.rs:3038 / 3152`），所以拒绝发生在**写总线之前**——`self.platform` 一个字都没动，草稿因此永远停在“未生效”。"),
        ),
    },
    {
        "id": "s3",
        "title": "§3 裁决（A~F）",
        "body": body(
            tbl("t3"),
            p("六条里 **B 是最容易被漏掉的一条**：如果只做 A，`Bus` 仍会用 16 MiB 去卡 128 MiB 的 RAM 段，`E_BUS-09` 会在建图那一刻把整批改动抵消掉。判据见 §5。"),
        ),
    },
    {
        "id": "s4",
        "title": "§4 契约层：字段、错误码、版本",
        "body": body(
            p("字段处置（只删不加，避免“删一个补一个”变成等价改造）："),
            tbl("t4"),
            code("c1"),
            p("错误码处置——**码表不重排**，这是硬约束（历史日志、旧文档、旧脚本都按号解释）："),
            tbl("t5"),
            p("版本号与兼容矩阵："),
            tbl("t6"),
            p("老档案读出后落到哪（不需要迁移脚本，靠 SUPPORTED 名单 + serde 忽略未知键）："),
            tbl("t7"),
        ),
    },
    {
        "id": "s5",
        "title": "§5 内核与 M-14：上限跟段走",
        "body": body(
            p("落点清单（一条一处，不允许“两个地方各判一半”）："),
            tbl("t8"),
            code("c2"),
            code("c3"),
            p("**M-14 的新判据**（表 9）是这批的核心语义变化：同一个错误码 `E_BUS-09` 保留，但它量的是“这段自己的容量够不够”，不再是“宿主的预算够不够”。"),
            tbl("t9"),
            p("为什么保留 `E_BUS-09` 而不是删掉：它守的是**坏数据**（init 字节数 > 段 size、坏快照），那件事与“板上有没有预算”无关，删了就是丢掉一条真实护栏。"),
        ),
    },
    {
        "id": "s6",
        "title": "§6 前端：删常量、删死代码、补两处体验",
        "body": body(
            tbl("t10"),
            code("c4"),
            p("上一轮用户实际撞到的两个体验缺口，同批修掉（不是“顺手优化”，它们和预算问题共用同一条失败路径）："),
            tbl("t11"),
        ),
    },
    {
        "id": "s7",
        "title": "§7 新链路（流程图）",
        "body": body(
            p("改完之后，从“用户改段长度”到“真正跑起来”只有一条链，**没有任何地方再问“预算是多少”**："),
            flow(),
            p("老档案走的是同一条链：读入时忽略两个预算键 ⇒ 之后与 schema 6 完全同构，不存在第二条分支。"),
        ),
    },
    {
        "id": "s8",
        "title": "§8 判据矩阵（内核 / 前端 / 真机）",
        "body": body(
            tbl("t12"),
            tbl("t13"),
            tbl("t14"),
        ),
    },
    {
        "id": "s9",
        "title": "§9 存量用例处置（不许静默删）",
        "body": body(
            p("`region_budget_bytes` 全仓 **173 处 / 30 文件**（`contract/platform.rs` 35、`memory-map.js` 15 全是死代码、kernel 测试 ~50、docs ~50、`tools/stage14/16_check.py` 2）。处置口径三条："),
            checklist([
                "**随死代码一起删**：`memory-map.js` 那 15 处（函数整段不存在了，用例一并退役并登记）；",
                "**改写判据**：kernel 里凡是断言 `E_PLT-07` 的，改判 `E_PLT-05`（装不下）或改为“不再报错”（够大就装得进）；",
                "**只改措辞**：docs 里的历史记录不回改（历史就该是历史的），只在 USER_GUIDE 与 PROGRESS 里写“已退役”。",
            ]),
            p("门禁脚本两处必须同批改，否则 CI 直接红：`tools/stage14_check.py:137`、`tools/stage16_check.py:158` 的键名清单里删掉 `region_budget_bytes`。"),
        ),
    },
    {
        "id": "s10",
        "title": "§10 变异自检",
        "body": body(
            p("每条变异都必须**精确**变红（不多不少），改回后逐字节还原、md5 一致："),
            tbl("t15"),
        ),
    },
    {
        "id": "s11",
        "title": "§11 破坏性变更与验收",
        "body": body(
            p("发行说明直接抄这张表："),
            tbl("t16"),
            tbl("t17"),
        ),
    },
    {
        "id": "s12",
        "title": "§12 风险与遗留",
        "body": body(
            tbl("t18"),
            p("登记一条明确的不做：**独立板级内存图编辑面（#/memory-map）本批不复活**。三个渲染函数删掉之后，如果需要“独立板编辑面”，那是新需求，按新视图立项，不要从死代码里捞。"),
        ),
    },
]

# ---------------------------------------------------------------------------
# 渲染
# ---------------------------------------------------------------------

FLOW_SVG = """<svg class="flow-svg" viewBox="0 0 1020 300" role="img" aria-label="新的装载与建图链路">
  <defs>
    <marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0 0L10 5L0 10z" fill="#2f5bd6"/>
    </marker>
  </defs>
  <g font-family="Noto Sans SC, PingFang SC, sans-serif" font-size="13">
    <rect x="8" y="40" width="150" height="66" rx="8" fill="#f1f4f9" stroke="#d8dee9"/>
    <text x="83" y="66" text-anchor="middle" fill="#1f2937" font-weight="700">用户改段长度</text>
    <text x="83" y="86" text-anchor="middle" fill="#4b5563" font-size="11.5">只写前端草稿</text>

    <line x1="158" y1="73" x2="196" y2="73" stroke="#2f5bd6" stroke-width="1.6" marker-end="url(#ar)"/>

    <rect x="196" y="40" width="150" height="66" rx="8" fill="#fff" stroke="#d8dee9"/>
    <text x="271" y="66" text-anchor="middle" fill="#1f2937" font-weight="700">点「重新载入」</text>
    <text x="271" y="86" text-anchor="middle" fill="#4b5563" font-size="11.5">SetMemoryMap</text>

    <line x1="346" y1="73" x2="384" y2="73" stroke="#2f5bd6" stroke-width="1.6" marker-end="url(#ar)"/>

    <rect x="384" y="32" width="180" height="82" rx="8" fill="#fff" stroke="#d8dee9"/>
    <text x="474" y="56" text-anchor="middle" fill="#1f2937" font-weight="700">validate()</text>
    <text x="474" y="76" text-anchor="middle" fill="#4b5563" font-size="11.5">V-4 零长 / V-5 溢出</text>
    <text x="474" y="94" text-anchor="middle" fill="#4b5563" font-size="11.5">V-6 重叠 · 无 V-7</text>

    <line x1="564" y1="73" x2="602" y2="73" stroke="#2f5bd6" stroke-width="1.6" marker-end="url(#ar)"/>

    <rect x="602" y="32" width="190" height="82" rx="8" fill="#fff" stroke="#d8dee9"/>
    <text x="697" y="56" text-anchor="middle" fill="#1f2937" font-weight="700">建图（M-14）</text>
    <text x="697" y="76" text-anchor="middle" fill="#4b5563" font-size="11.5">上限 = 该段自己的 size</text>
    <text x="697" y="94" text-anchor="middle" fill="#4b5563" font-size="11.5">没有 16 MiB 默认</text>

    <line x1="792" y1="73" x2="830" y2="73" stroke="#2f5bd6" stroke-width="1.6" marker-end="url(#ar)"/>

    <rect x="830" y="40" width="180" height="66" rx="8" fill="#eaf0fb" stroke="#b9c8ea"/>
    <text x="920" y="66" text-anchor="middle" fill="#1e3a8a" font-weight="700">装载 / 运行</text>
    <text x="920" y="86" text-anchor="middle" fill="#1e3a8a" font-size="11.5">修订号 +1 · 自动落库</text>

    <rect x="196" y="190" width="300" height="76" rx="8" fill="#fdf3f3" stroke="#f0c9c9"/>
    <text x="346" y="214" text-anchor="middle" fill="#8b2b2b" font-weight="700">校验失败？</text>
    <text x="346" y="234" text-anchor="middle" fill="#8b2b2b" font-size="11.5">E_PLT-04/05/06 —— 横幅 + 草稿保留</text>
    <text x="346" y="252" text-anchor="middle" fill="#8b2b2b" font-size="11.5">内核一个字都不动</text>
    <line x1="474" y1="114" x2="474" y2="168" stroke="#c25a5a" stroke-width="1.4" stroke-dasharray="4 3" marker-end="url(#ar)"/>
    <line x1="474" y1="168" x2="400" y2="190" stroke="#c25a5a" stroke-width="1.4" stroke-dasharray="4 3" marker-end="url(#ar)"/>

    <rect x="560" y="190" width="330" height="76" rx="8" fill="#fff8ea" stroke="#efd9a8"/>
    <text x="725" y="214" text-anchor="middle" fill="#7a5a12" font-weight="700">老档案（schema ≤ 5）</text>
    <text x="725" y="234" text-anchor="middle" fill="#7a5a12" font-size="11.5">读入时忽略两个预算键</text>
    <text x="725" y="252" text-anchor="middle" fill="#7a5a12" font-size="11.5">之后与 schema 6 同构，无第二分支</text>
    <line x1="697" y1="114" x2="697" y2="190" stroke="#b8912f" stroke-width="1.4" stroke-dasharray="4 3" marker-end="url(#ar)"/>
  </g>
</svg>"""

FLOW_MOBILE = """
<div class="flow-mobile">
  <div class="flow-step"><span class="num">1</span><span class="txt"><b>用户改段长度</b> —— 只写前端草稿，内核不知道</span></div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><span class="num">2</span><span class="txt"><b>点「重新载入」</b> —— SetMemoryMap / PatchMemoryMap</span></div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><span class="num">3</span><span class="txt"><b>validate()</b> —— V-4 零长 / V-5 溢出 / V-6 重叠；<b>不再有 V-7</b></span></div>
  <div class="flow-branch">失败 ⇒ E_PLT-04/05/06 ⇒ 横幅 + 草稿保留；内核一个字都不动</div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><span class="num">4</span><span class="txt"><b>建图（M-14）</b> —— 每段上限 = 该段自己的 size，<b>没有 16 MiB 默认</b></span></div>
  <div class="flow-branch">老档案（schema ≤ 5）读入时忽略两个预算键，之后与 schema 6 同构</div>
  <div class="flow-arrow">↓</div>
  <div class="flow-step"><span class="num">5</span><span class="txt"><b>装载 / 运行</b> —— 修订号 +1，成功即自动落库到本机板档案</span></div>
</div>
"""

CSS = """:root { --ink:#1f2937; --ink-2:#4b5563; --brand:#1e3a8a; --brand-2:#2f5bd6;
  --line:#d8dee9; --head-bg:#f1f4f9; --zebra:#f8fafc; --bg:#ffffff; --code-bg:#0f172a; }
* { box-sizing:border-box; }
html { scroll-behavior:smooth; scroll-padding-top:20px; }
@media (prefers-reduced-motion: reduce) { html { scroll-behavior:auto; } }
body { margin:0; background:var(--bg); color:var(--ink);
  font-family:'Noto Sans SC','PingFang SC','Microsoft YaHei',system-ui,sans-serif;
  font-size:15px; line-height:1.75; }
.wrap { width:100%; max-width:1120px; margin:0 auto; padding:16px; }
.doc-title { font-size:26px; line-height:1.4; font-weight:900; color:var(--brand);
  margin:8px 0 4px; text-align:center; }
.subtitle { text-align:center; color:var(--ink-2); font-size:14px; margin:0 0 4px; }
.version { text-align:center; color:#6b7280; font-size:12.5px; margin:0 0 20px; }
.h-sec { font-size:21px; font-weight:800; color:var(--brand);
  border-bottom:2px solid var(--line); padding-bottom:6px; margin:34px 0 14px; scroll-margin-top:16px; }
.h-sub { font-size:17px; font-weight:700; color:#111827; margin:24px 0 10px; }
.body { margin:0 0 10px; text-align:justify; }
.lead { margin:0 0 10px; font-size:15.5px; }
.tbl-wrap { overflow-x:auto; margin:12px 0 16px; }
.doc-table { width:100%; border-collapse:collapse; table-layout:fixed; font-size:13px; line-height:1.6; background:#fff; }
.doc-table th { background:var(--head-bg); color:#111827; font-weight:700;
  border:1px solid var(--line); padding:7px 8px; vertical-align:middle; }
.doc-table td { border:1px solid var(--line); padding:6px 8px; vertical-align:middle; word-break:break-word; }
.doc-table tr.row1 { background:var(--zebra); }
.caption { text-align:left; font-size:12.5px; color:var(--ink-2); margin:14px 0 4px; font-weight:700; }
.code { background:var(--code-bg); color:#e2e8f0; font-family:'JetBrains Mono','Cascadia Mono',Consolas,monospace;
  font-size:12.5px; line-height:1.6; padding:12px 14px; border-radius:6px; overflow-x:auto;
  white-space:pre; margin:10px 0 16px; }
.code-title { font-size:13px; font-weight:700; color:#111827; margin:16px 0 4px; }
.flow-svg { width:100%; height:auto; }
.flow-mobile { display:none; }
.flow-step { display:flex; gap:10px; margin:8px 0; align-items:flex-start; }
.flow-step .num { flex:0 0 26px; height:26px; border-radius:50%; background:var(--brand);
  color:#fff; font-weight:700; font-size:13px; display:flex; align-items:center; justify-content:center; }
.flow-step .txt { font-size:14px; }
.flow-branch { margin:6px 0 6px 36px; font-size:13.5px; color:var(--ink-2); }
.flow-arrow { text-align:center; color:#9aa1af; font-size:16px; line-height:1; margin:2px 0; }
.list { margin:8px 0 14px; padding-left:20px; }
.list li { margin:5px 0; font-size:14.5px; }
.nav-toggle { margin:14px 0; }
.nav-toggle summary { cursor:pointer; font-weight:700; color:var(--brand);
  padding:9px 12px; border:1px solid var(--line); border-radius:6px; background:var(--head-bg); list-style:none; }
.nav-toggle summary::-webkit-details-marker { display:none; }
.nav-box { display:flex; flex-direction:column; gap:2px; padding:10px 4px 2px; }
.nav-l1 { color:var(--brand); font-weight:700; text-decoration:none; padding:3px 4px; }
.nav-l1:hover { color:var(--brand-2); text-decoration:underline; }
@media (max-width:767px) {
  .flow-svg { display:none; }
  .flow-mobile { display:block; }
  .doc-table { font-size:12px; }
  .wrap { padding:12px; }
}
"""


def inline_md(text: str) -> str:
    """只支持 **加粗** 与 `代码`，够用且不会引入第二套语法。"""
    out = html.escape(text, quote=False)
    # **bold**
    parts = []
    bold = False
    for chunk in out.split("**"):
        if bold:
            parts.append(f"<b>{chunk}</b>")
        else:
            parts.append(chunk)
        bold = not bold
    out = "".join(parts)
    # `code`
    segs = out.split("`")
    res = []
    for i, seg in enumerate(segs):
        if i % 2 == 1:
            res.append(f"<code>{seg}</code>")
        else:
            res.append(seg)
    return "".join(res)


def render_table(key: str, caption: str | None = None) -> str:
    t = TABLES[key]
    widths = t["widths"]
    head = "".join(f'<th style="width:{w}">{inline_md(c)}</th>' for w, c in zip(widths, t["cols"]))
    rows = []
    for i, row in enumerate(t["rows"]):
        cls = ' class="row1"' if i % 2 == 1 else ""
        cells = "".join(f"<td>{inline_md(c)}</td>" for c in row)
        rows.append(f"<tr{cls}>{cells}</tr>")
    cap = caption or t["caption"]
    return (
        f'<p class="caption">{inline_md(cap)}</p>'
        f'<div class="tbl-wrap"><table class="doc-table">'
        f"<thead><tr>{head}</tr></thead><tbody>{''.join(rows)}</tbody>"
        f"</table></div>"
    )


def render_code(key: str) -> str:
    c = CODE[key]
    return (
        f'<p class="code-title">{inline_md(c["title"])}</p>'
        f'<pre class="code">{html.escape(c["text"], quote=False)}</pre>'
    )


def render_section(sec: dict) -> str:
    out = [f'<h2 class="h-sec" id="{sec["id"]}">{inline_md(sec["title"])}</h2>']
    for kind, payload in sec["body"]:
        if kind == "p":
            out.append(f'<p class="body">{inline_md(payload)}</p>')
        elif kind == "lead":
            out.append(f'<div class="lead">{inline_md(payload)}</div>')
        elif kind == "tbl":
            key, cap = payload
            out.append(render_table(key, cap))
        elif kind == "code":
            out.append(render_code(payload))
        elif kind == "flow":
            out.append(f'<figure class="fig">{FLOW_SVG}</figure>{FLOW_MOBILE}')
        elif kind == "check":
            items = "".join(f"<li>{inline_md(x)}</li>" for x in payload)
            out.append(f'<ul class="list">{items}</ul>')
        else:  # pragma: no cover
            raise ValueError(kind)
    return "\n".join(out)


def nav() -> str:
    links = "".join(
        f'<a class="nav-l1" href="#{s["id"]}">{inline_md(s["title"])}</a>' for s in SECTIONS
    )
    return (
        '<details class="nav-toggle"><summary>目录（12 节 · 18 张表）</summary>'
        f'<div class="nav-box">{links}</div></details>'
    )


def main() -> None:
    sections = "\n".join(render_section(s) for s in SECTIONS)
    page = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(DOC["title"])}</title>
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect x='4' y='4' width='24' height='24' rx='3' fill='%231e3a8a'/%3E%3Crect x='9' y='9' width='14' height='14' rx='1.5' fill='%23f8fafc'/%3E%3C/svg%3E">
<style>
{CSS}</style>
</head>
<body>
<div class="wrap">
<h1 class="doc-title">{html.escape(DOC["title"])}</h1>
<p class="subtitle">{html.escape(DOC["subtitle"])}</p>
<p class="version">{html.escape(DOC["version"])}</p>
{nav()}
{sections}
</div>
</body>
</html>
"""
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(page, encoding="utf-8")
    tables = page.count('<table class="doc-table">')
    print(f"写出 {OUT.relative_to(ROOT)}：{len(page.splitlines())} 行 · {tables} 张表 · {len(page)} 字节")


if __name__ == "__main__":
    main()
