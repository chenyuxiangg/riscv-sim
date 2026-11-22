#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成「断点窗口重构 v0.2」HTML 详细设计评审稿（取代 v0.1）。

v0.2 相对 v0.1 的订正（见文档第 1 节，全部有代码证据）：
  (1) 取消独立 ObservTable —— M-08 自述「单一数据源、没有第二份状态」，拆表撞不变量；
      改为「by_id 权威 + by_addr / pred_ids 两个派生索引」。
  (2) 取消 R1/R2「一条谓词拆两半求值」—— 改为条目级单一求值时刻：
      无内存读 ⇒ 取指前；含内存读 ⇒ store 边界整条求值。
  (3) 取消注册首判判命中 —— 改为注册期试求值只做「静态错必拒 / 动态错标降级」，不产生停止。
  (4) 变量名语义纠正 —— 全局变量名 = 变量的值（读内存），不是地址常量；
      要地址写 &名字。v0.1 把 g_counter 当地址常量，与「取符号的值」相反。

数据驱动：正文与表格都在 SECTIONS 里，渲染器只管排版。
用法：python3 tools/build_bp_rework_doc.py [输出路径]
产物：docs/detailed-design/STAGE-55_Breakpoint_Window_Rework.html

⚠️ 设计评审稿：基线 main 58eb021，未切 feature 分支、未编码。
   「测试矩阵」全部是计划用例，不代表已通过。
"""

from __future__ import annotations

import html
import sys
from pathlib import Path

OUT_DEFAULT = Path("docs/detailed-design/STAGE-55_Breakpoint_Window_Rework.html")

TITLE = "STAGE-55 · 断点窗口重构（三类输入合一 + 观察式条件断点）"
SUBTITLE = "RISC-V 仿真平台 · 调试服务层 · 详细设计评审稿 v0.2"
VERSION = ("版本 v0.2 重写稿 · 2026-11-22 · 基线 main 58eb021 · 取代 v0.1 "
           "· 输入方式按 kind 三值落库 · 未切分支、未编码")

KPI = [
    ("3", "种输入方式", "地址 · 符号 · 条件（落库为 kind 三值，可筛选可还原）"),
    ("2", "种运行时语义", "地址命中 · 谓词命中"),
    ("1", "张权威表", "by_id 唯一数据源 + 两个派生索引"),
    ("2", "个求值时刻", "无内存读→取指前；含内存读→store 边界"),
]

SECTIONS = []

# ── 1 重写缘由 ──────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s1",
    "h": "1 重写缘由：v0.1 的四处硬伤",
    "blocks": [
        ("p", "v0.1 的问题不是措辞，是结构。四处都撞了既有代码里已经写死的不变量或语义，"
              "逐条订正如下；v0.2 全部按订正后的口径展开。"),
        ("caption", "表 1 · 四处硬伤与订正"),
        ("table", {
            "widths": ["6%", "34%", "30%", "30%"],
            "head": ["#", "v0.1 的写法", "为什么是错的（证据）", "v0.2 改为"],
            "rows": [
                ["1", "另起 <code>ObservTable</code> 存观察式条目",
                 "<code>m08_breakpoint/mod.rs:83-87</code> 自述「单一数据源：注册 / 删除 / 使能只改这两张表，"
                 "<b>没有第二份状态</b>」（M-08 LLD §7.2(1)）。拆表直接破坏该不变量，"
                 "且带来两套 id 空间、两条删除 / 启停路径、列表回执要合并两源",
                 "单表：<code>by_id</code> 权威，<code>by_addr</code> 与 <code>pred_ids</code> 只作派生索引"],
                ["2", "R1 判寄存器半边、R2 判内存半边",
                 "<code>a0 == 5 && [g] == 1</code> 会被拼成「取指前的 a0」+「store 写后的 g」，"
                 "是两个时刻的状态凑成一条谓词，语义不成立",
                 "条目级单一时刻：整条谓词在一个时刻求值一次"],
                ["3", "注册时先判一次，满足则视为命中",
                 "会在一件事都没发生的情况下停住，与「为真即停」的观察式语义冲突，"
                 "触发时机也变得不可预测",
                 "注册期只做<b>试求值校验</b>：静态错必拒、动态错标降级；<b>不判定命中</b>"],
                ["4", "把 <code>g_counter</code> 折叠成地址常量（档 2，不读内存）",
                 "用户口径是「取符号的值（局部变量值、全局变量值、函数地址）」；"
                 "把变量名当地址常量后，<code>g_counter == 3</code> 比的是地址而不是值，正好反了",
                 "<b>变量名 = 变量的值（读内存）；函数名 = 函数地址；要地址写 <code>&名字</code></b>"],
            ],
        }),
        ("callout", "另外修正一处表述",
         "v0.1 把 <code>[x1+8]</code> / <code>@x1+8</code> 写成「新造文法」。"
         "这套写法 <code>watch_expr.rs:75-95</code> 已经在用（<code>WatchTarget::parse</code> "
         "接受 <code>[…]</code>、<code>@…</code>、<code>AddressBase</code> + "
         "<code>i32</code> 偏移）⇒ ⇒ v0.2 的扩展是<b>复用</b>它，不是另造一套。"),
    ],
})

# ── 2 概念澄清 ──────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s2",
    "h": "2 输入方式 3 种落库，运行时语义 2 种",
    "blocks": [
        ("p", "三类输入方式<b>作为一等字段落库</b>（<code>kind</code> 三值），但运行时只兑现<b>两种语义</b>："
              "地址命中 / 谓词命中。符号断点落库后仍标 <code>Symbol</code>，"
              "便于列表筛选、快照还原与排障定位；它的命中路径与地址断点逐字相同。"),
        ("caption", "表 2 · 落库 kind 与运行时映射"),
        ("table", {
            "widths": ["14%", "20%", "26%", "14%", "26%"],
            "head": ["落库 kind", "界面输入", "运行时映射", "入 by_addr", "命中行为"],
            "rows": [
                ["<code>Address</code>", "十六进制地址", "地址匹配", "是",
                 "取指前按地址匹配；命中停在该条指令<b>之前</b>"],
                ["<code>Symbol</code>", "符号名（Tab 补全）",
                 "地址匹配（等价 Address，<b>保留来源</b>）", "是",
                 "与地址断点<b>逐字相同</b>；<code>source_hint</code> 记原始名字"],
                ["<code>Condition</code>", "条件表达式", "谓词求值", "否（<code>addr=0</code> 仅占位）",
                 "观察式：按条目级时机求值，为真即停；<b>不参与地址匹配</b>"],
            ],
        }),
        ("callout", "落库但不改运行时",
         "<code>kind</code> 在这里是<b>来源标记</b>，不是第三种命中逻辑——运行时仍只有地址命中与谓词命中两种。"
         "落库的收益是：列表能筛「符号断点」、快照能还原用户当初怎么下的、排障时能一眼看出"
         "是名字解析错了还是地址错了。代价是 DTO 多一维，必须在 <code>set()</code> 里守住"
         "「<code>Symbol</code> 必须解析出地址、<code>Condition</code> 必须不带真实地址」这条一致性。"),
    ],
})

# ── 3 表结构 ────────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s3",
    "h": "3 表结构：一个权威 + 两个派生索引",
    "blocks": [
        ("p", "既有 <code>BreakpointTable</code> 的权威是 <code>by_addr</code>——因为条目必须有地址。"
              "谓词条目没有地址，所以必须把权威换到 <code>by_id</code>："
              "地址映射降级为索引，谓词条目另有一个 id 集合索引。业务状态仍只有一份。"),
        ("caption", "表 3 · 结构字段与职责"),
        ("table", {
            "widths": ["26%", "14%", "60%"],
            "head": ["字段", "角色", "职责"],
            "rows": [
                ["<code>by_id: BTreeMap&lt;u32, Entry&gt;</code>", "<b>权威</b>",
                 "唯一业务状态。条目、<code>enabled</code>、命中计数、已解析 AST、降级标记全在这里"],
                ["<code>by_addr: BTreeMap&lt;u32, u32&gt;</code>", "派生索引",
                 "地址 → id，只收 <code>Address</code> / <code>Symbol</code> 条目；"
                 "<code>Condition</code> 条目不入此索引。地址重复校验走这里"],
                ["<code>pred_ids: BTreeSet&lt;u32&gt;</code>", "派生索引",
                 "谓词类条目的 id 集合，供每指令遍历；由 <code>by_id</code> 唯一定定，不存额外信息"],
            ],
        }),
        ("svg", "table"),
        ("mobile_table", [
            ("1", "Entry 存于 by_id —— 唯一业务状态（启停 / 计数 / AST / 降级）"),
            ("2", "地址类条目同时在 by_addr 占一个地址槽（addr → id）"),
            ("3", "谓词类条目同时在 pred_ids 占一个 id（无地址槽）"),
            ("4", "写入路径必须同步三处；读取路径只用其中一处，不交叉推断"),
            ("5", "任意时刻：by_addr 与 pred_ids 都必须能由 by_id 完整重算 ⇒ 不一致即内部损坏，按不存在处理，不 panic"),
        ]),
        ("p", "「派生索引」的纪律：任何写路径（set / clear / set_enabled）结束后，"
              "三处必须一致；一致性可用一次只读断言验证（任一 id 在 pred_ids 中 ⇔ "
              "<code>by_id</code> 里该条 <code>kind=Condition</code>）。"
              "不一致时按「不存在」处理，不 panic（沿用现有 WASM 纪律：panic = Worker 崩）。"),
        ("caption", "表 4 · set() 按条目类型分派校验"),
        ("table", {
            "widths": ["16%", "34%", "22%", "28%"],
            "head": ["条目", "校验顺序", "失败码", "表变动"],
            "rows": [
                ["<code>Address</code>", "2 字节对齐 → 地址重复（<code>by_addr</code> 已有）→ id 复用冲突",
                 "E_CTX-07", "失败时<b>三处全不变</b>"],
                ["<code>Symbol</code>", "符号名非空 → 解析出地址 → 再走与 <code>Address</code> 相同的三道地址校验",
                 "E_DBG-04 / E_CTX-07", "解析不出地址则拒绝，<b>不留半生效条目</b>"],
                ["<code>Condition</code>", "语法 → 未定义符号 / 不可定位变量 → 空表达式",
                 "E_DBG-02", "同上"],
                ["<code>Condition</code>", "试求值（仅验证可求值性，见第 5 节）",
                 "不拒绝，标 <code>degraded</code> 并记原因", "注册成功，含降级标记"],
            ],
        }),
        ("callout", "顺序不能变",
         "既有三道地址校验的语义一字未改，条件校验仍排在其后（<code>mod.rs:113-114</code>）⇒ ⇒"
         "「地址非法 + 条件非法」时报的仍是地址的错（<code>E_CTX-07</code>）。"
         "谓词类没有地址，直接进入语法校验。"),
    ],
})

# ── 4 求值模型 ──────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s4",
    "h": "4 求值模型：条目级单一时刻",
    "blocks": [
        ("p", "一条谓词描述的是<b>某一时刻的状态</b>，必须整体在一处求值。"
              "含不含内存读决定这个时刻落在哪里，但绝不把一条谓词拆到两个时刻。"),
        ("caption", "表 5 · 两个求值时刻"),
        ("table", {
            "widths": ["18%", "24%", "28%", "30%"],
            "head": ["时刻", "适用条目", "求值内容", "停止点"],
            "rows": [
                ["取指前 · 地址匹配", "<code>Address</code> / <code>Symbol</code> 条目，无附加条件",
                 "不涉及表达式", "停在<b>该条指令之前</b>"],
                ["取指前 · 地址+条件", "<code>Address</code> / <code>Symbol</code> 条目带附加条件",
                 "地址命中后整条求值；含内存读也当场读总线——该地址命中频率极低，成本可忽略",
                 "同上"],
                ["取指前 · 谓词", "<code>Condition</code> 条目的谓词<b>无</b>内存读（常量 / 寄存器 / pc / 符号地址）",
                 "整条谓词，用当前 gpr + pc 快照", "停在<b>该条指令之前</b>，与地址断点同心智模型"],
                ["store 边界 · 谓词", "<code>Condition</code> 条目的谓词<b>含</b>内存读（变量值 / <code>[…]</code>）",
                 "整条谓词，用该时刻的 gpr + 该时刻的内存", "写已生效、指令已提交；PC 已推进到下一条"],
            ],
        }),
        ("svg", "timing"),
        ("mobile_flow", [
            ("1", "注册：解析 AST，标记 contains_memory_read"),
            ("2", "contains_memory_read = 假 ⇒ 每指令取指前整条求值"),
            ("3", "contains_memory_read = 真 ⇒ 只在 store 成功写后整条求值"),
            ("4", "store 边界：先用 gpr 算出各内存操作数的地址（不算总线访问）"),
            ("5", "地址区间与本次写区间相交才读总线；不相交 ⇒ 谓词值不可能变 ⇒ 跳过"),
            ("6", "相交 ⇒ 读总线取新值，整条谓词求值一次；为真即停"),
            ("7", "命中回执带 trigger_addr（触发那次写所属指令的地址），因为 PC 已推进"),
        ]),
        ("caption", "表 6 · store 边界的三个精确口径"),
        ("table", {
            "widths": ["24%", "76%"],
            "head": ["口径", "说明"],
            "rows": [
                ["写后判新值", "总线写已生效、指令已提交后才求值 ⇒ 读到的是新值，不是旧值"],
                ["区间过滤是等价优化，不是近似",
                 "store 指令不改 GPR（RV32 的 S 型指令没有 rd）⇒ ⇒ 若本次写区间与谓词引用的地址区间不相交，"
                 "谓词读到的每个值都不变 ⇒ 跳过与求值结果必然相同。过滤用的地址由 gpr 现算（只读寄存器，"
                 "不过总线）⇒ 连「先算地址再判」都不产生内存访问"],
                ["引用 pc 的条目不做区间过滤",
                 "提交后 pc 已推进，pc 本身就变了；此时区间过滤不再等价 ⇒ 这类条目按每条 store 都求值（保守）"],
            ],
        }),
        ("callout", "要明写的语义代价",
         "含内存读的条目只在 store 边界求值，意味着它<b>寄存器子式</b>的变化不会单独触发。"
         "例如 <code>a0 == 5 && [g] == 1</code> 里 a0 变成 5 但没有任何 store 发生，这一轮不会停。"
         "这是「只监控 store 类指令」口径的直接后果，属于<b>已接受的取舍</b>，不是缺陷；"
         "要覆盖纯寄存器变化就得每条指令读总线，成本不可接受。"),
    ],
})

# ── 5 注册期校验 ────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s5",
    "h": "5 注册期：静态错必拒、动态错标降级",
    "blocks": [
        ("p", "注册期做一次试求值，但只用来<b>验证这条表达式在当前上下文求不求得出来</b>，"
              "绝不据此判定命中。求值结果只记成基线供界面展示，不产生任何停止事件。"),
        ("caption", "表 7 · 两类错误的分流"),
        ("table", {
            "widths": ["16%", "40%", "20%", "24%"],
            "head": ["类别", "例子", "处置", "理由"],
            "rows": [
                ["静态错", "文法错 / 未知寄存器 / 未定义符号 / 寄存器取地址 <code>&a0</code> / 空表达式",
                 "<b>拒绝注册</b>，E_DBG-02，表不变", "这类错误不会因为运行推进而变好，早拒最省事"],
                ["动态错", "该地址当前未映射 / 栈变量此刻尚无帧 / 掩码全 0 的预置区",
                 "<b>允许注册</b>，标 <code>degraded</code> 并记原因；求值期失败按未命中",
                 "内存还没初始化、还没进函数都是正常过程态，拒绝注册会让用户没法先配好断点"],
                ["基线记录", "试求值得到的数值 / 是否已满足",
                 "只存展示字段，不进命中路径", "避免「什么都没变就停」"],
            ],
        }),
        ("callout", "为什么不判命中",
         "观察式的语义是「状态变化导致命中」。注册时若已满足就停，等于在没有任何变化时停住，"
         "且触发时机变得不可预测。基线只用来让界面显示「当前已满足 / 当前不满足」，帮助用户判断配置对不对。"),
    ],
})

# ── 6 文法 ──────────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s6",
    "h": "6 表达式文法：复用既有词法与寻址",
    "blocks": [
        ("p", "比较 / 逻辑 / 括号 / 短路 / 无符号比较 / 真值≠0 全部沿用 <code>cond.rs</code> 冻结口径"
              "（<code>:5-36</code>）。本批只改两件事：<code>operand</code> 扩充，以及 <code>Ident</code> 的解析顺序。"),
        ("caption", "表 8 · 操作数语义（这一张表决定「写什么得到什么」）"),
        ("table", {
            "widths": ["22%", "22%", "12%", "20%", "24%"],
            "head": ["写法", "含义", "读内存", "求值时刻", "备注"],
            "rows": [
                ["<code>5</code> / <code>0x1000</code>", "字面常量", "否", "取指前", "裸数字仍是常量（见下方分歧说明）"],
                ["<code>a0</code> / <code>x1</code> / <code>sp</code>", "寄存器值", "否", "取指前", "ABI 别名等价某个 xN"],
                ["<code>pc</code>", "当前 PC", "否", "取指前", "含 pc 的条目关闭区间过滤"],
                ["<code>main</code>（函数名）", "函数地址常量", "否", "取指前", "函数没有「值」"],
                ["<code>g_counter</code>（全局变量）", "<b>变量的值</b>", "是", "store 边界", "这是 v0.2 的关键纠正"],
                ["<code>local</code>（局部变量）", "当前帧变量值", "视位置", "见备注",
                 "寄存器位置 ⇒ 取指前；栈位置 ⇒ store 边界"],
                ["<code>&g_counter</code> / <code>&main</code>", "符号的地址", "否", "取指前", "取地址"],
                ["<code>[x1+8]</code> / <code>@sp-4</code>", "读该地址的 word", "是", "store 边界",
                 "沿用 watch_expr 既有写法"],
                ["<code>&[x1+8]</code>", "该地址本身（= x1+8 的结果）", "否", "取指前",
                 "取地址，不读内容"],
                ["<code>&a0</code>", "非法", "—", "—", "寄存器没有可寻址的地址，E_DBG-02"],
            ],
        }),
        ("caption", "表 9 · 文法扩展点与既有实现"),
        ("table", {
            "widths": ["26%", "34%", "40%"],
            "head": ["扩展点", "既有实现", "改动"],
            "rows": [
                ["寻址 <code>基址 ± 偏移</code>", "<code>AddressBase</code>（寄存器 / pc / 数字）+ "
                 "<code>AddressSpec.offset: i32</code>（<code>watch_expr.rs:50-55, 75-95</code>）",
                 "<b>直接复用</b>；仅给 <code>AddressBase</code> 添一个 <code>Symbol</code> 变体"],
                ["<code>[…]</code> / <code>@…</code> 双写法", "同上（<code>strip_prefix('[')</code> / <code>'@'</code>）",
                 "<b>直接复用</b>，不新增第三种壳"],
                ["标识符识别", "<code>cond.rs</code> lexer 已把任意标识符切成 <code>Tok::Ident</code>"
                 "（<code>:390-395</code>）",
                 "只改<b>解析顺序</b>：寄存器别名 → 当前帧局部变量 → 全局符号 → 报未定义"],
                ["寄存器值 / pc", "<code>Operand</code> 已支持", "不变"],
            ],
        }),
        ("callout", "一处刻意的分歧",
         "<code>watch_expr</code> 里<b>裸数字就是内存地址</b>（<code>:89-92</code>）。"
         "条件文法里<b>不能</b>这样——<code>0x1000</code> 必须是比较对象，不能变成「读 0x1000」。"
         "所以条件文法中裸数字恒为字面常量，读内存必须显式写 <code>[…]</code> 或 <code>@…</code>。"
         "这是两套文法各自的语义，需在文档里钉住，避免实现时误抄。"),
        ("caption", "表 10 · 档位与 v0.1 的差异"),
        ("table", {
            "widths": ["10%", "30%", "30%", "30%"],
            "head": ["档", "能力", "求值时刻", "与 v0.1 的差异"],
            "rows": [
                ["档 1", "常量 / 寄存器 / pc", "取指前", "不变"],
                ["档 2", "函数地址、取地址 <code>&名字</code>", "取指前",
                 "v0.1 把全局变量名也算进这一档（错），v0.2 只留函数名与 <code>&</code>"],
                ["档 3", "内存读：全局变量值、<code>[…]</code>、<code>@…</code>", "store 边界",
                 "v0.1 归到档 3 但混有地址常量；v0.2 边界更清"],
                ["档 4", "当前帧局部变量 / 形参", "按位置分流",
                 "v0.1 未区分寄存器位置与栈位置的时刻差异"],
            ],
        }),
    ],
})

# ── 7 契约 ──────────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s7",
    "h": "7 契约与回执",
    "blocks": [
        ("caption", "表 11 · DTO 字段"),
        ("table", {
            "widths": ["24%", "16%", "60%"],
            "head": ["字段", "变更", "说明"],
            "rows": [
                ["<code>kind: BreakpointKind</code>", "新增，<code>#[serde(default)]</code> = <code>Address</code>",
                 "三个值：<code>Address</code> / <code>Symbol</code> / <code>Condition</code>，"
                 "<b>输入方式落库</b>。缺键旧载荷 ⇒ Address，既有地址断点行为一字不变"],
                ["<code>addr: u32</code>", "沿用类型，语义分化",
                 "<code>Address</code> / <code>Symbol</code> = 真实地址；<code>Condition</code> 置 <code>0</code> "
                 "且<b>不入 by_addr 索引</b> ⇒ 不会与真实地址 0 冲突。保持 <code>u32</code> 是为了线上可解析"],
                ["<code>condition: Option&lt;String&gt;</code>", "沿用",
                 "<code>kind=Condition</code> 时为表达式原文；地址类可继续带可选附加条件（兼容现状）"],
                ["<code>source_hint: Option&lt;String&gt;</code>", "沿用",
                 "<code>kind=Symbol</code> 时存原始符号名，与 <code>kind</code> 互为印证；"
                 "仅界面展示，不参与任何判定"],
                ["<code>enabled</code> / <code>id</code>", "沿用", "禁用条目任何路径都不命中、不计数"],
            ],
        }),
        ("caption", "表 12 · 回执与停止原因"),
        ("table", {
            "widths": ["24%", "26%", "50%"],
            "head": ["项", "取值", "理由"],
            "rows": [
                ["<code>HaltReason</code>", "复用 <code>Watchpoint</code>（<code>protocol.rs:939-954</code>）",
                 "观察式本就是 watchpoint 语义；不扩枚举 ⇒ 旧前端 / 旧快照都不用改"],
                ["停止点 PC", "store 边界命中时 PC 已是下一条",
                 "必须另带回 <code>trigger_addr</code>（触发那次写所属指令地址），否则前端无法指出「是这条 store 触发的」"],
                ["<code>trigger_addr</code> 的来源", "执行前记录 <code>exec_pc</code>",
                 "<code>BusAccess</code> 载荷<b>不含 pc</b>（<code>m14_bus/mod.rs</code> 字段只有 addr/width/dir/value/ok/is_fetch）"
                 "⇒ 需在 backend 执行指令前把 pc 存下，hook 记录写时一并带上"],
                ["列表回执", "含 <code>kind</code> / <code>condition</code> 原文 / 命中次数 / "
                 "<code>degraded</code> 原因 / 基线",
                 "让界面能区分三种来源、显示降级提示与当前是否已满足"],
            ],
        }),
        ("callout", "schema 是否升版",
         "<code>kind</code> 带 <code>#[serde(default)]</code> ⇒ 旧载荷 → 新代码无损。"
         "反向（旧代码读新快照）：<code>Symbol</code> 条目会被读成 <code>Address</code>，"
         "但 <code>addr</code> 相同 ⇒ <b>运行时行为一致，只丢来源标记</b>；"
         "真正降级的只有 <code>Condition</code> 条目（被当成 addr=0 的地址断点）。"
         "是否升 <code>SNAPSHOT_SCHEMA_VERSION</code> 取决于能否接受这处降级，"
         "建议先不升并在快照文档写明。"),
    ],
})

# ── 8 前端 ──────────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s8",
    "h": "8 前端交互",
    "blocks": [
        ("p", "现有三段并排输入（<code>debug-panel.js:723-725</code>）收敛为一条："
              "类型下拉 + 单输入框 + 一个按钮。下拉切换时输入框的 placeholder、补全与错误口径随类型切换。"),
        ("caption", "表 13 · 三类输入框"),
        ("table", {
            "widths": ["16%", "24%", "30%", "30%"],
            "head": ["类型", "输入", "表现", "提交载荷"],
            "rows": [
                ["地址断点", "十六进制地址", "普通文本框；无补全；沿用现有地址解析",
                 "<code>kind=Address</code> + <code>addr</code>"],
                ["符号断点", "符号名", "Tab 触发前缀补全，候选<b>前 5 条</b>（后端上限 16，前端截取）；"
                 "↑↓ 选择、回车落断；无地址的候选渲染为不可点",
                 "<code>kind=Symbol</code> + 解析所得 <code>addr</code> + <code>source_hint</code>=原名"],
                ["条件断点", "条件表达式", "表达式文本框；<b>不做</b>符号补全；错误原样透传内核消息",
                 "<code>kind=Condition</code> + <code>condition</code> 原文"],
            ],
        }),
        ("caption", "表 14 · 界面元素"),
        ("table", {
            "widths": ["22%", "30%", "48%"],
            "head": ["元素", "位置", "行为"],
            "rows": [
                ["类型下拉", "输入框左侧", "三项；切换后清空草稿与错误位，避免把地址当表达式提交"],
                ["单输入框", "下拉右侧", "Enter 等价点击按钮"],
                ["按钮", "输入框右侧", "文案统一「加断点」；按当前类型组装载荷"],
                ["候选浮层", "符号类型下挂输入框", "前 5 条；无地址候选不可点"],
                ["错误位", "输入框下方", "原样展示内核消息（含「哪一段不对」），前端不吞不改写"],
                ["断点列表", "卡片下部", "显示来源标签（地址 / 符号 / 条件）、表达式原文、命中次数、降级提示"],
            ],
        }),
        ("callout", "两态空文案",
         "符号查不到时区分「没有符号表 / 无 DWARF 调试信息」与「共有 N 个符号 / 匹配不上」，"
         "沿用现有空态口径，不合并成一句。"),
    ],
})

# ── 9 异常与兼容 ────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s9",
    "h": "9 异常、兼容与回滚",
    "blocks": [
        ("caption", "表 15 · 处置矩阵"),
        ("table", {
            "widths": ["24%", "30%", "46%"],
            "head": ["场景", "处置", "要点"],
            "rows": [
                ["文法 / 未定义符号", "E_DBG-02 拒绝；表不变", "沿用 <code>cond.rs</code> 原样拒纪律"],
                ["注册期动态不可求值", "允许注册，标 <code>degraded</code>", "求值期失败按未命中；界面给提示"],
                ["求值期总线读失败", "该次按未命中，不中断执行", "不 panic（WASM panic = Worker 崩）"],
                ["禁用条目", "任何路径不命中、不计数", "沿用现有语义"],
                ["索引不一致", "按「不存在」处理，不 panic", "与既有 <code>mod.rs:175-179</code> 同口径"],
                ["旧载荷缺 kind", "解析为 Address", "既有地址断点行为不变"],
                ["旧代码读新快照（Symbol）", "读成 Address，<code>addr</code> 相同", "运行时行为一致，只丢来源标记"],
                ["旧代码读新快照（Condition）", "被当作 <code>addr=0</code> 地址断点",
                 "已知降级，文档写明；必要时升 schema 版"],
                ["回滚", "地址类命中链路未改动；R 时机与表结构为纯增量",
                 "回滚需同时清理 <code>pred_ids</code> 与前端草稿态"],
            ],
        }),
    ],
})

# ── 10 测试 ─────────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s10",
    "h": "10 测试设计（计划用例 · 未实现）",
    "blocks": [
        ("p", "下列用例钉的是「错误机制」，不是覆盖率。结果列全部为「计划」。"
              "回归基线：前端 777/777、后端 1899 passed / 0 failed、fmt/clippy rc=0。"),
        ("caption", "表 16 · 计划测试矩阵"),
        ("table", {
            "widths": ["14%", "28%", "30%", "28%"],
            "head": ["层", "用例", "输入 / 步骤", "钉住的错误机制"],
            "rows": [
                ["表", "索引一致性", "set / clear / set_enabled 各路径后校验三处一致",
                 "拆分索引后出现「删了 by_id 忘了 pred_ids」"],
                ["表", "谓词不占地址槽", "注册 <code>addr=0</code> 谓词条目 + 真实 0x0 地址断点",
                 "谓词被误入 by_addr，误伤真实地址 0"],
                ["表", "校验顺序", "地址非法 + 条件非法同时给",
                 "报的必须是 E_CTX-07，不是 E_DBG-02"],
                ["契约", "输入方式落库还原", "注册 Symbol 条目 → 快照序列化 → 反序列化 → ListBreakpoints",
                 "kind 仍是 Symbol、来源不丢"],
                ["契约", "kind 与 addr 一致性", "构造 kind=Symbol 但解析不出地址 / kind=Condition 带真实地址",
                 "前者拒绝注册；后者不得进 by_addr"],
                ["语义", "变量名=值", "<code>g_counter == 3</code> 且 g_counter 存 3",
                 "变量名被当地址常量（v0.1 的错）"],
                ["语义", "取地址", "<code>&g_counter == 0x…</code> 为真",
                 "<code>&</code> 不去读内容"],
                ["语义", "寄存器取地址被拒", "<code>&a0</code>",
                 "应为 E_DBG-02"],
                ["语义", "裸数字是常量", "<code>0x1000</code> 参与比较",
                 "误抄 watch_expr「裸数字=内存地址」"],
                ["时机", "整条单时刻", "<code>a0 == 5 &amp;&amp; [g] == 1</code> 在 store 边界求值",
                 "被拆成取指前 + 写后两半"],
                ["时机", "写后新值", "store 改写 g 后求值",
                 "读到旧值"],
                ["时机", "区间过滤等价", "向不相交地址 store",
                 "多余总线读；或误判为可能命中"],
                ["时机", "引用 pc 关闭过滤", "含 pc 的条目遇任意 store",
                 "用区间过滤错误跳过"],
                ["注册", "已满足不停", "注册时条件已为真的条目",
                 "注册点直接产生 Stop（v0.1 的错）"],
                ["注册", "动态错标降级", "监控未映射地址注册",
                 "被拒注册，用户配不了断点"],
                ["前端", "三类切换", "下拉切换 + 提交",
                 "地址串被当表达式提交；错误位不清空"],
                ["回归", "既有地址链路", "全量门禁",
                 "777 / 1899 基线被破；fmt/clippy 不干净"],
            ],
        }),
    ],
})

# ── 11 开放项 ───────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s11",
    "h": "11 开放决定与交付批次",
    "blocks": [
        ("list", [
            "<b>局部变量生命周期</b>：函数序言 / 尾声区间里寄存器位置变量是否可见、何时销毁——"
            "需 DWARF 解析落地后按实际 DIE 结构再定，属档 4 细节。",
            "<b>降级态是否参与命中</b>：当前口径是「求值期失败按未命中」；也可选择「降级即暂停」"
            "以提醒用户配置有误，但这会让断点不可预期，倾向不做。",
            "<b>读观察点（监控 load）</b>：本批只监控写，读访问观察留后续。",
            "<b>跨帧 unwind</b>：调用者帧变量、栈帧链 / CFA 解析留后续批次。",
            "<b><code>trigger_addr</code> 是否进停止回执结构</b>：也可只在界面层用最近一次写记录推导；"
            "进结构更稳，但扩了回执字段。",
        ]),
        ("caption", "表 17 · 交付批次"),
        ("table", {
            "widths": ["10%", "30%", "36%", "24%"],
            "head": ["子批", "主题", "落点", "档位"],
            "rows": [
                ["A", "前端三类整合 + 单表重构 + 观察式契约",
                 "下拉/单框/切换；by_id 权威化 + 两索引；<code>kind</code> 三值落库；取指前时机", "档 1"],
                ["B", "符号表达式与取地址",
                 "<code>Ident</code> 解析顺序；函数名地址常量；<code>&</code>；<code>AddressBase::Symbol</code>", "档 2"],
                ["C", "内存读与 store 边界",
                 "总线读；地址预计算 + 区间过滤；<code>trigger_addr</code>；降级标记", "档 3"],
                ["D", "当前帧局部变量",
                 "DWARF DIE 解析；寄存器 / 栈偏移定位；两态空态", "档 4"],
            ],
        }),
        ("callout", "下一步",
         "本稿评审通过后：<code>git fetch origin main</code> → 切 <code>feat/breakpoint-window-rework</code> → "
         "按 A→D 实施，每批独立跑门禁并登记台账，收口 <code>--no-ff</code> 合 main、删本地分支。"),
    ],
})

# ── SVG ─────────────────────────────────────────────────────────────────
TABLE_SVG = """
<svg class="flow-svg" viewBox="0 0 1000 380" xmlns="http://www.w3.org/2000/svg"
  role="img" aria-label="断点表单权威与派生索引结构">
  <defs>
    <marker id="a1" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/>
    </marker>
  </defs>

  <rect x="330" y="40" width="340" height="150" rx="10" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="500" y="66" text-anchor="middle" font-size="16" font-weight="700" fill="#1e3a8a">by_id（权威）</text>
  <text x="500" y="90" text-anchor="middle" font-size="12.5" fill="#4b5563">BTreeMap&lt;u32, Entry&gt;</text>
  <text x="500" y="112" text-anchor="middle" font-size="12.5" fill="#4b5563">Entry：bp / kind / AST / enabled / hits / degraded</text>
  <text x="500" y="136" text-anchor="middle" font-size="12" font-weight="700" fill="#9a3412">唯一业务状态</text>
  <text x="500" y="158" text-anchor="middle" font-size="12" fill="#4b5563">必须能完整重算下面两个索引</text>

  <rect x="60" y="230" width="330" height="105" rx="10" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="225" y="256" text-anchor="middle" font-size="14" font-weight="700" fill="#1f2937">by_addr（派生索引）</text>
  <text x="225" y="278" text-anchor="middle" font-size="12.5" fill="#4b5563">addr → id，只收地址类条目</text>
  <text x="225" y="299" text-anchor="middle" font-size="12.5" fill="#4b5563">地址重复校验走这里</text>
  <text x="225" y="320" text-anchor="middle" font-size="12" fill="#6b7280">谓词条目不入此索引</text>

  <rect x="610" y="230" width="330" height="105" rx="10" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="775" y="256" text-anchor="middle" font-size="14" font-weight="700" fill="#1f2937">pred_ids（派生索引）</text>
  <text x="775" y="278" text-anchor="middle" font-size="12.5" fill="#4b5563">BTreeSet&lt;u32&gt;，谓词条目 id</text>
  <text x="775" y="299" text-anchor="middle" font-size="12.5" fill="#4b5563">供每指令 / store 边界遍历</text>
  <text x="775" y="320" text-anchor="middle" font-size="12" fill="#6b7280">不存任何额外信息</text>

  <line x1="420" y1="190" x2="240" y2="228" stroke="#1e3a8a" stroke-width="2" marker-end="url(#a1)"/>
  <line x1="580" y1="190" x2="760" y2="228" stroke="#1e3a8a" stroke-width="2" marker-end="url(#a1)"/>
  <text x="180" y="205" font-size="11.5" fill="#1e3a8a">派生</text>
  <text x="795" y="205" font-size="11.5" fill="#1e3a8a">派生</text>

  <text x="500" y="365" text-anchor="middle" font-size="12" fill="#6b7280">
\U0001F6A8 写路径（set / clear / set_enabled）必须同步三处；读取路径只用其中一处
  </text>
</svg>
"""

TIMING_SVG = """
<svg class="flow-svg" viewBox="0 0 1020 470" xmlns="http://www.w3.org/2000/svg"
  role="img" aria-label="条目级单一求值时刻的两条路径">
  <defs>
    <marker id="t1" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/>
    </marker>
    <marker id="t2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#9aa1af"/>
    </marker>
  </defs>

  <rect x="360" y="16" width="300" height="58" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="510" y="40" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">注册：解析 AST</text>
  <text x="510" y="60" text-anchor="middle" font-size="12" fill="#4b5563">标记 contains_memory_read</text>

  <polygon points="510,96 590,132 510,168 430,132" fill="#fff7ed" stroke="#fdba74"/>
  <text x="510" y="136" text-anchor="middle" font-size="12.5" fill="#9a3412">含内存读？</text>
  <line x1="510" y1="74" x2="510" y2="94" stroke="#1e3a8a" stroke-width="2" marker-end="url(#t1)"/>

  <!-- 左：取指前 -->
  <line x1="432" y1="132" x2="332" y2="200" stroke="#1e3a8a" stroke-width="2" marker-end="url(#t1)"/>
  <text x="330" y="150" font-size="12" font-weight="700" fill="#1e3a8a">否</text>
  <rect x="60" y="200" width="290" height="86" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="205" y="226" text-anchor="middle" font-size="14" font-weight="700" fill="#1e3a8a">取指前求值</text>
  <text x="205" y="248" text-anchor="middle" font-size="12.5" fill="#4b5563">整条谓词，gpr + pc 快照</text>
  <text x="205" y="268" text-anchor="middle" font-size="12.5" fill="#4b5563">停在指令之前（同心智模型）</text>

  <!-- 右：store 边界 -->
  <line x1="588" y1="132" x2="688" y2="200" stroke="#9a3412" stroke-width="2" marker-end="url(#t1)"/>
  <text x="690" y="150" font-size="12" font-weight="700" fill="#9a3412">是</text>
  <rect x="670" y="200" width="310" height="86" rx="8" fill="#fff7ed" stroke="#fdba74"/>
  <text x="825" y="226" text-anchor="middle" font-size="14" font-weight="700" fill="#9a3412">store 边界求值</text>
  <text x="825" y="248" text-anchor="middle" font-size="12.5" fill="#4b5563">整条谓词，写后 gpr + 新内存值</text>
  <text x="825" y="268" text-anchor="middle" font-size="12.5" fill="#4b5563">停在 store 的下一句，带回 trigger_addr</text>

  <rect x="330" y="330" width="360" height="70" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="510" y="356" text-anchor="middle" font-size="14" font-weight="700" fill="#166534">谓词为真 ⇒ 命中暂停</text>
  <text x="510" y="378" text-anchor="middle" font-size="12" fill="#166534">计数 +1；HaltReason 复用 Watchpoint</text>

  <line x1="205" y1="286" x2="205" y2="330" stroke="#9aa1af" stroke-width="1.8" stroke-dasharray="6 4"/>
  <line x1="205" y1="330" x2="328" y2="356" stroke="#9aa1af" stroke-width="1.8" stroke-dasharray="6 4" marker-end="url(#t2)"/>
  <line x1="825" y1="286" x2="825" y2="330" stroke="#9aa1af" stroke-width="1.8" stroke-dasharray="6 4"/>
  <line x1="825" y1="330" x2="692" y2="356" stroke="#9aa1af" stroke-width="1.8" stroke-dasharray="6 4" marker-end="url(#t2)"/>

  <text x="20" y="442" font-size="12" fill="#6b7280">
图注：一条谓词只在一个时刻求值；「含内存读」决定时刻落在取指前还是 store 边界，不拆两半。
  </text>
</svg>
"""


def esc(t: str) -> str:
    return html.escape(t, quote=False)


def render_table(spec: dict) -> str:
    cols = "".join(f'<col style="width:{w}">' for w in spec["widths"])
    head = "".join(f"<th>{c}</th>" for c in spec["head"])
    rows = []
    for i, row in enumerate(spec["rows"]):
        cls = ' class="row1"' if i % 2 else ""
        rows.append(f"<tr{cls}>" + "".join(f"<td>{c}</td>" for c in row) + "</tr>")
    return (
        '<div class="tbl-wrap"><table class="doc-table wide">'
        f"<colgroup>{cols}</colgroup><thead><tr>{head}</tr></thead>"
        f"<tbody>{''.join(rows)}</tbody></table></div>"
    )


def render_blocks(blocks) -> str:
    out = []
    for b in blocks:
        kind = b[0]
        val = b[1] if len(b) > 1 else None
        if kind == "p":
            out.append(f'<p class="body">{val}</p>')
        elif kind == "caption":
            out.append(f'<p class="caption">{val}</p>')
        elif kind == "table":
            out.append(render_table(val))
        elif kind == "list":
            out.append('<ul class="list">' + "".join(f"<li>{i}</li>" for i in val) + "</ul>")
        elif kind == "callout":
            out.append(
                f'<div class="callout"><span class="callout-tag">{b[1]}</span>'
                f'<span class="callout-txt">{b[2]}</span></div>'
            )
        elif kind == "svg":
            out.append(TABLE_SVG if val == "table" else TIMING_SVG)
        elif kind == "mobile_flow":
            out.append('<div class="flow-mobile">' + "".join(
                f'<div class="flow-step"><span class="num">{n}</span><span class="txt">{t}</span></div>'
                for n, t in val) + "</div>")
        elif kind == "mobile_table":
            out.append('<div class="flow-mobile">' + "".join(
                f'<div class="flow-step"><span class="num">{n}</span><span class="txt">{t}</span></div>'
                for n, t in val) + "</div>")
        else:
            raise SystemExit(f"未知块类型：{kind}")
    return "\n".join(out)


CSS = """
:root { --ink:#1f2937; --ink-2:#4b5563; --brand:#1e3a8a; --brand-2:#2f5bd6;
  --line:#d8dee9; --head-bg:#f1f4f9; --zebra:#f8fafc; --bg:#ffffff; --code-bg:#0f172a; }
* { box-sizing:border-box; }
html { scroll-behavior:smooth; scroll-padding-top:20px; }
@media (prefers-reduced-motion: reduce) { html { scroll-behavior:auto; } }
body { margin:0; background:var(--bg); color:var(--ink);
  font-family:'Noto Sans SC','PingFang SC','Microsoft YaHei',system-ui,sans-serif;
  font-size:15px; line-height:1.75; }
.wrap { width:100%; max-width:1120px; margin:0 auto; padding:16px; }
.doc-title { font-size:25px; line-height:1.4; font-weight:900; color:var(--brand);
  margin:8px 0 4px; text-align:center; }
.subtitle { text-align:center; color:var(--ink-2); font-size:14px; margin:0 0 4px; }
.version { text-align:center; color:#6b7280; font-size:12.5px; margin:0 0 18px; }
.kpis { display:grid; grid-template-columns:repeat(4,1fr); gap:10px; margin:6px 0 22px; }
.kpi { background:var(--head-bg); border:1px solid var(--line); border-radius:8px; padding:10px 12px; }
.kpi-n { font-size:22px; font-weight:900; color:var(--brand); line-height:1.2; }
.kpi-t { font-size:13px; font-weight:700; color:#111827; margin-top:2px; }
.kpi-d { font-size:12px; color:var(--ink-2); margin-top:3px; }
.h-sec { font-size:21px; font-weight:800; color:var(--brand);
  border-bottom:2px solid var(--line); padding-bottom:6px; margin:34px 0 14px; scroll-margin-top:16px; }
.body { margin:0 0 10px; text-align:justify; }
.tbl-wrap { overflow-x:auto; margin:12px 0 16px; }
.doc-table { width:100%; border-collapse:collapse; table-layout:fixed; font-size:13px;
  line-height:1.6; background:#fff; }
.doc-table th { background:var(--head-bg); color:#111827; font-weight:700;
  border:1px solid var(--line); padding:7px 8px; vertical-align:middle; }
.doc-table td { border:1px solid var(--line); padding:6px 8px; vertical-align:middle; word-break:break-word; }
.doc-table tr.row1 { background:var(--zebra); }
.caption { text-align:left; font-size:12.5px; color:var(--ink-2); margin:14px 0 4px; font-weight:700; }
.flow-svg { width:100%; height:auto; }
.flow-mobile { display:none; }
.flow-step { display:flex; gap:10px; margin:8px 0; align-items:flex-start; }
.flow-step .num { flex:0 0 26px; height:26px; border-radius:50%; background:var(--brand);
  color:#fff; font-weight:700; font-size:13px; display:flex; align-items:center; justify-content:center; }
.flow-step .txt { font-size:14px; }
.callout { display:flex; gap:8px; align-items:flex-start; background:#fffbeb; border:1px solid #fcd34d;
  border-left-width:4px; border-radius:6px; padding:9px 12px; margin:12px 0 16px; }
.callout-tag { flex:0 0 auto; font-weight:800; color:#92400e; font-size:13px; }
.callout-txt { font-size:13.5px; color:#3f2d0a; }
.list { margin:8px 0 14px; padding-left:20px; }
.list li { margin:5px 0; font-size:14.5px; }
.nav-toggle { margin:14px 0 4px; }
.nav-toggle summary { cursor:pointer; font-weight:700; color:var(--brand);
  padding:9px 12px; border:1px solid var(--line); border-radius:6px; background:var(--head-bg); list-style:none; }
.nav-box { display:flex; flex-direction:column; gap:2px; padding:10px 4px 2px; }
.nav-l1 { color:var(--brand); font-weight:700; text-decoration:none; padding:3px 4px; }
.nav-l1:hover { color:var(--brand-2); text-decoration:underline; }
.foot { margin:40px 0 8px; padding-top:10px; border-top:1px solid var(--line);
  font-size:12.5px; color:#6b7280; }
@media (max-width:767px) {
  .flow-svg { display:none; }
  .flow-mobile { display:block; }
  .doc-table { font-size:12px; }
  .kpis { grid-template-columns:repeat(2,1fr); }
  .wrap { padding:12px; }
  .doc-title { font-size:20px; }
  .h-sec { font-size:18px; }
}
"""


def build(nav: str, body: str) -> str:
    kpis = "".join(
        f'<div class="kpi"><div class="kpi-n">{n}</div><div class="kpi-t">{t}</div>'
        f'<div class="kpi-d">{d}</div></div>' for n, t, d in KPI
    )
    return f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(TITLE)}</title>
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect x='4' y='4' width='24' height='24' rx='3' fill='%231e3a8a'/%3E%3Ccircle cx='12' cy='12' r='5' fill='none' stroke='%23f8fafc' stroke-width='2'/%3E%3C/svg%3E">
<style>{CSS}</style>
</head>
<body>
<div class="wrap">
<h1 class="doc-title">{esc(TITLE)}</h1>
<p class="subtitle">{esc(SUBTITLE)}</p>
<p class="version">{esc(VERSION)}</p>
<div class="kpis">{kpis}</div>
{nav}
{body}
<p class="foot">本文件由 <b>tools/build_bp_rework_doc.py</b> 数据驱动生成；改内容请改脚本后重跑，
不要手改 HTML。本文为设计评审稿，测试矩阵中的用例尚未实现。</p>
</div>
</body>
</html>
"""


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT_DEFAULT
    nav_items = "".join(f'<a class="nav-l1" href="#{s["id"]}">{s["h"]}</a>' for s in SECTIONS)
    nav = ('<details class="nav-toggle" open><summary>目录</summary>'
           f'<div class="nav-box">{nav_items}</div></details>')
    body = "\n".join(
        f'<h2 class="h-sec" id="{s["id"]}">{s["h"]}</h2>\n{render_blocks(s["blocks"])}'
        for s in SECTIONS
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(build(nav, body), encoding="utf-8")
    print(f"generated: {out} ({out.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
