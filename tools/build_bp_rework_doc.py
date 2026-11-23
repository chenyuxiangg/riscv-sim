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
SUBTITLE = "RISC-V 仿真平台 · 调试服务层 · 详细设计定稿 v0.7"
VERSION = ("版本 v0.7 · 2026-11-23 · 基线 main 1b62c53（v0.137 地址文法归属订正）"
           "（A→E + CFI + 栈回溯 + 真相候选列表 + 断点恢复放过 + 符号联想 已合入）"
           "· 输入方式按 kind 三值落库 · CFI / 栈回溯口径见 §12 · "
           "局部变量候选列表见 §13 · 恢复放过与符号联想见 §14 · 偏离见 §11")

KPI = [
    ("3", "种输入方式", "地址 · 符号 · 条件（落库为 kind 三值，可筛选可还原）"),
    ("2", "种运行时语义", "地址命中 · 谓词命中"),
    ("1", "张权威表", "by_id 唯一数据源 + 两个派生索引"),
    ("2", "个求值时刻", "无内存读→取指前；含内存读→store 边界"),
    ("2", "条 CFI 口径", "变量取主行 · 回溯按 pc 逐行（同一节区、两个口径）"),
    ("32", "帧回溯上限", "返回地址 0 = 栈底；再往上说「被截断」，不猜"),
    ("128", "条变量上限", "超了报「列表不完整」，不静默截断"),
    ("4", "个字符起联", "符号档 ≥4 字符自动联想（防抖 180 ms），Tab 退役"),
    ("6", "秒报错寿命", "校验类错误到点自己消失，不再永久挂在面板上"),
    ("1", "次放过", "停在断点上后，恢复只放过这一条一次（否则原地重停）"),
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
         "这套写法早就在用：<code>WatchTarget::parse</code> 接受 <code>[…]</code>、"
         "<code>@…</code>、<code>AddressBase</code> + <code>i32</code> 偏移 ⇒ ⇒ "
         "v0.2 的扩展是<b>复用</b>它，不是另造一套。"
         "<br>v0.137 起这份地址文法的<b>归属变了</b>：原先住在 <code>watch_expr.rs</code>，"
         "但它只有条件文法在用 ⇒ ⇒ 已并回 <code>cond.rs</code> 的 <code>mod addr</code>，"
         "条件断点与地址表达式共用同一份，地址解析<b>只有一个落点</b>。",
         "本文所有「既有实现」指针都指向那里。"),
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
                 "寄存器位置 ⇒ 取指前；栈位置 ⇒ store 边界；<b>可见性 = 整帧可见</b>"],
                ["<code>&g_counter</code> / <code>&main</code>", "符号的地址", "否", "取指前", "取地址"],
                ["<code>[x1+8]</code> / <code>@sp-4</code>", "读该地址的 word", "是", "store 边界",
                 "沿用地址文法既有写法（<code>cond.rs</code> 的 <code>mod addr</code>）"],
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
                 "<code>AddressSpec.offset: i32</code>"
                 "（<code>cond.rs</code> 的 <code>mod addr</code> ⇒ ⇒ v0.137 从 "
                 "<code>watch_expr.rs</code> 并回，两处文法合一）",
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
         "<code>watch_expr</code> 里<b>裸数字就是内存地址</b>"
         "（<code>cond.rs</code> 的 <code>mod addr</code> · <code>parse_base_with</code> 的裸数字分支）。"
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
    "h": "10 测试设计（钉错误机制 · 按批落地）",
    "blocks": [
        ("p", "下列用例钉的是「错误机制」，不是覆盖率。首批之后已逐批写成真用例，"
              "编号见 §12 / §13（宿主 S-15 ~ S-23、前端 C-10f / C-14 系列 / C-15 系列）。"
              "回归基线（v0.132 后）：后端 1998 passed / 0 failed（20 套件）、"
              "前端 791/791、fmt / clippy rc=0。"),
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
                 "误抄地址文法「裸数字=内存地址」"],
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
                 "1998 / 791 基线被破；fmt/clippy 不干净"],
            ],
        }),
    ],
})

# ── 11 开放项 ───────────────────────────────────────────────────────────
SECTIONS.append({
    "id": "s11",
    "h": "11 开放决定与交付批次",
    "blocks": [
        ("callout", "A→E 实施期定稿（取代本节原先的开放项）",
         "<b>① 局部变量生命周期</b> ⇒ <b>一律当整帧可见</b>（用户 2026-11-22 拍板）："
         "不看 <code>DW_AT_location</code> 的位置区间列表，只认单点位置（<code>DW_OP_fbreg</code> / "
         "<code>DW_OP_regN</code>）；<code>loclist</code> 变量整条跳过，不编位置。"
         "<b>② 降级态</b> ⇒ 维持「求值期失败按未命中」；注册期读不到只打标记、不拒。"
         "<b>③ 读观察点</b>（监控 load）⇒ <b>维持不做</b>（用户 2026-11-22 拍板）：含内存读的条件"
         "只在 store 边界判定，<b>读到匹配值不触发</b>；停因仍复用 <code>HaltReason::Watchpoint</code>、"
         "<code>trigger_addr</code> 指向触发指令。产出的<b>文案债务</b>已在条件输入栏写明（见 §12 表 20）。"
         "<b>④ 跨帧 unwind</b> ⇒ <b>已落地收窄版</b>（用户拍板）：只做<b>栈回溯列表</b>"
         "（上层函数名 + pc），<b>不解析上层帧变量</b>；<code>CFA</code> 不再假设 <code>s0</code>，"
         "改为读 <code>.debug_frame</code> 的 CIE/FDE（见 §12）。"
         "<b>⑤ <code>trigger_addr</code></b> ⇒ 已进停止回执结构（只在 store 边界命中时给值）。"
         "<b>⑥ store 谓词与 M-10 观察点同时成立</b> ⇒ <b>条件断点优先</b>（用户拍板）："
         "停因归条件断点、<code>trigger_addr</code> 指向那条 store，M-10 的命中事实仍记录在 "
         "<code>last_watchpoint_hit</code>（不抹掉、不改变停因）。"
         "<b>⑦ 真相候选列表</b> ⇒ <b>只读面板</b>（用户 2026-11-22 拍板）：回执只给「名字 + "
         "位置」（人读文本 <code>[s0-24]</code> / <code>a0</code>），<b>不带当前值、不带类型、"
         "不做条件输入补全</b>；口径见 §13。"
         "<b>⑧ 停在断点上后的恢复</b> ⇒ <b>内核放过这一条一次</b>（用户 2026-11-22 拍板）："
         "钩子仍留在<b>取指位置</b>，但恢复（「继续」/「单步」）的第一条指令不再判断点；"
         "否则同一个 pc 上立刻二次命中、机器卡死（见 §14 表 22）。"
         "<b>⑨ 符号档的联想与报错寿命</b> ⇒ <b>≥4 字符自动联想、Tab 退役、报错 6 秒过期</b>"
         "（用户 2026-11-22 拍板，见 §14 表 23）；<b>两条细节同时定稿</b>："
         "报错寿命取 <code>6000 ms</code>、<b>全族统一</b>不按错误码区分；回车 <b>两步语义保留</b>"
         "（先填名、再按一次才加断点）。"),
        ("caption", "表 18 · 实施期刻意偏离（代码为准，逐条有落地位置）"),
        ("table", {
            "widths": ["28%", "44%", "28%"],
            "head": ["项", "定稿做法", "落地位置 / 理由"],
            "rows": [
                ["条件文法不认 <code>@</code>",
                 "只认方括号 <code>[...]</code>；<code>@x2+4</code> 没有右边界 ⇒ 在表达式里歧义",
                 "<code>m08_breakpoint/cond.rs</code> 的词法"],
                ["<code>AddressBase::Symbol</code> 只在绑定期存在",
                 "符号基址在注册期落定成数字 / 寄存器 / 帧偏移 ⇒ 求值期不需要符号表",
                 "<code>cond.rs::bind_spec</code>"],
                ["「记基线供界面展示」未做",
                 "界面没有基线栏 ⇒ 不留一个没人读的字段（只报触发地址）",
                 "<code>m17_backend::trial_eval_breakpoint</code> 注释"],
                ["局部变量位置只认单点",
                 "区间列表（loclist）一律跳过 ⇒ 与「整帧可见」同一条口径",
                 "<code>m11_symbol/locals.rs</code>"],
                ["<code>CFA</code> 不再假设 <code>s0(x8)</code>（v0.129 替掉）",
                 "解析 <code>.debug_frame</code> 的 CIE/FDE，变量按<b>主行</b>（覆盖地址最多的一行）取值；"
                 "读不到 CFI ⇒ <b>保留函数区间、丢掉变量</b>（不退回旧假设）",
                 "<code>m11_symbol/cfi.rs</code> + <code>locals.rs</code>"],
                ["<code>DW_CFA_def_cfa_expression</code> 不认",
                 "CFA 由表达式算的 CIE/FDE 整条按「读不到 CFI」处理（不猜）",
                 "<code>cfi.rs::run_cfi</code> 的 <code>0x0f</code> 分支"],
                ["栈回溯只给 pc + 函数名",
                 "上层帧<b>变量</b>不解析（名字只在当前帧有效）；<code>frames[0]</code> 是当前 pc",
                 "<code>CommandPayload::Backtrace</code> / <code>EventPayload::Backtrace</code>"],
                ["回溯只追 CFA 与 RA 两条规则",
                 "其余 callee-saved 不逐帧恢复 ⇒ 深帧若 CFA 基于 <code>s0</code> 类基址寄存器，"
                 "用的是<b>当前帧</b>的寄存器值（已知近似）",
                 "<code>kernel_host::on_backtrace</code> 注释"],
                ["返回地址 0 视为栈底",
                 "「爬不上去」与「到顶了」分开：<code>ra == 0</code> 是正常终点（不算截断）；"
                 "其余四类成因才报 <code>truncated</code>",
                 "<code>on_backtrace</code> 的 <code>if ra == 0 { break; }</code>"],
                ["内存条件只在 store 边界判定",
                 "读到匹配值<b>不触发</b>（用户拍板维持现状）⇒ 界面必须写明，否则用户会以为「读没生效」",
                 "<code>debug-panel.js</code> 的 <code>data-bp-store-only-note</code>"],
                ["两态空文案拆成三态",
                 "无 DWARF / 该帧没有这个变量 / 名字压根不存在 ⇒ 三条不同的下一步",
                 "<code>names::ResolveError</code> + <code>cond::resolve_in</code>"],
                ["变量列表只给「名字 + 位置」",
                 "值要<b>按位置拼地址</b>再读内存 / 读寄存器、类型要顺 "
                 "<code>DW_AT_type → DW_TAG_base_type</code> 再走一遍 DIE ⇒ 两者都<b>不做</b>"
                 "（用户拍板），契约里没这两个字段",
                 "<code>CommandPayload::ListLocals</code> / "
                 "<code>EventPayload::Locals</code> 的注释"],
                ["变量列表是<b>快照</b>，不随 pc 自动刷新",
                 "只在点「列出」时发一条命令；pc 变了列表还是上一次的 ⇒ 要看新帧再点一次"
                 "（不偷偷重发、不猜用户想看哪帧）",
                 "<code>debug-panel.js</code> 的 <code>data-action=\"list-locals\"</code>"],
            ],
        }),
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
                 "DWARF 通用 DIE 走查；变量表 + 帧基址；寄存器 / 栈偏移定位", "档 4"],
                ["E", "收口批（偏离回写 + 界面上下文）",
                 "<code>E_DBG-02</code> 三态文案；<code>BreakpointInfo.scope_function</code>；"
                 "断点行「所在函数」；条件 placeholder 点出变量名", "档 4 收尾"],
                ["F", "CFI 批（真 CFA）",
                 "M-06 放行 <code>.debug_frame</code>；新模块 <code>cfi.rs</code>；"
                 "变量位置改由 CFI 主行算出，删 <code>FrameBaseSlot</code>", "档 4 深化"],
                ["G", "栈回溯批",
                 "CFI 补 <b>RA 规则</b> + 按 pc 取行；<code>Backtrace</code> 命令 / 回执；"
                 "调试面板「调用栈」卡；条件栏补「按写入判定」文案", "档 4 收尾"],
                ["H", "真相候选列表批",
                 "<code>ListLocals</code> 命令 / <code>Locals</code> 回执；"
                 "调试面板「局部变量」卡（名字 + 位置、两态文案互斥）；"
                 "<code>LocalLocation::describe()</code> 收敛位置写法", "档 4 收尾"],
                ["I", "断点恢复放过批（v0.134 · 本批前的 Bug 修）",
                 "<code>SelfIssBackend::resume_skip_pc</code>：停住时记 pc、恢复放过一条、"
                 "reset / restore 清位；查询口不动、放过的一条不重复计命中", "档 4 收尾"],
                ["J", "符号输入批（v0.135）",
                 "符号档 ≥4 字符自动联想（防抖 180 ms · 组合框下拉）；Tab 不再触发查询；"
                 "校验类报错 6 秒存活期；空 / 短文不发 <code>ResolveSymbol</code>", "档 4 收尾"],
            ],
        }),
        ("callout", "下一步",
         "A→E + CFI 批 + 栈回溯批 + 真相候选列表批均已落地（每批独立门禁 + 台账，"
         "<code>--no-ff</code> 合 main、删本地分支）。剩下的都<b>不是欠做</b>，是<b>拍板不做</b>："
         "变量<b>带值</b>（要按位置拼地址再读）、变量<b>带类型</b>（顺 "
         "<code>DW_AT_type</code> 走 DIE）、<b>条件输入补全</b>（点行不填名字）、"
         "上层帧变量、读观察点（<code>[..] == v</code> 读到即停）。"
         "任一要动，都是<b>新一笔拍板</b>，不在这条链上自动续。"),
    ],
})

SECTIONS.append({
    "id": "s12",
    "h": "12 CFI 与栈回溯（v0.129 / v0.130 落地）",
    "blocks": [
        ("p", "本节回写两批实施期口径。共同前提：局部变量与栈回溯用的是<b>同一份</b> "
              "<code>.debug_frame</code>，但<b>取值口径相反</b> —— 变量要「整帧可见」⇒ 取<b>主行</b>；"
              "回溯要「此刻这条指令」⇒ 按 <b>pc 逐行</b>。两个口径各持一个 <code>CfiTable</code> 实例，"
              "不共享。"),
        ("caption", "表 19 · <code>.debug_frame</code> 解析口径（v0.129）"),
        ("table", {
            "widths": ["26%", "46%", "28%"],
            "head": ["项", "定稿做法", "落地位置 / 理由"],
            "rows": [
                ["节区放行",
                 "M-06 的 <code>DEBUG_SECTION_WHITELIST</code> 7 → 8，放行 <code>.debug_frame</code>",
                 "<code>m06_elf/elf.rs</code> + 测试的 <code>EXPECTED_WHITELIST</code>"],
                ["记录格式",
                 "DWARF32；CIE 版本 v1 / v3 / v4；增补串只认空串或 <code>z</code> 打头"
                 "（<code>eh</code> / <code>zR</code> / <code>zPLR</code> 整条 CIE 不认）",
                 "<code>cfi.rs::Cie::parse</code>"],
                ["CIE 索引键",
                 "用<b>记录起点</b>（FDE 的 <code>cie_pointer</code> 从节区开头数）；"
                 "曾用「body 偏移」⇒ 0 条 FDE 解析成功，被「真夹具两条 FDE」用例抓住",
                 "<code>cfi.rs::CfiTable::parse</code>"],
                ["变量取哪一行",
                 "变量位置用<b>主行</b>（覆盖地址最多的一行，同宽取后者）⇒ 与「整帧可见」一致",
                 "<code>cfi.rs::body_cfa</code> → <code>locals.rs</code>"],
                ["读不到 CFI",
                 "函数区间<b>照收</b>（断点仍能显示「所在函数」）、栈上变量一个<b>不留</b>；不退回 "
                 "<code>CFA == s0</code>",
                 "<code>locals.rs</code> + 宿主 S-15"],
            ],
        }),
        ("callout", "为什么不能沿用「主行」做回溯",
         "主行是<b>整帧</b>口径：函数入口、函数体、尾声三段的 CFA / 返回地址位置各不相同。"
         "回溯要回答「<b>当前 pc 这一条</b>的返回地址存在哪儿」⇒ 必须用 <code>row_at(pc)</code> 取覆盖 pc 的那一行"
         "（FDE 取最小覆盖区间）。用主行回爬会在序言 / 尾声读到<b>错误的返回地址</b>，且错得很安静。"),
        ("caption", "表 20 · 栈回溯（<code>Backtrace</code>）口径与截断语义（v0.130）"),
        ("table", {
            "widths": ["24%", "50%", "26%"],
            "head": ["项", "定稿做法", "落点 / 判别力"],
            "rows": [
                ["命令",
                 "<code>CommandPayload::Backtrace</code>（unit 变体 · <b>只读</b>，不动任何状态）",
                 "前端 <code>Command.backtrace()</code>"],
                ["回执",
                 "<code>EventPayload::Backtrace { frames, truncated }</code>；"
                 "<code>StackFrame { pc, function }</code>，无名字时 <code>function</code> 缺省",
                 "<code>contract/src/model.rs</code>"],
                ["帧顺序",
                 "<code>frames[0]</code> 是<b>当前</b> pc；之后每帧是「上一层函数的返回地址」",
                 "前端首行标 <code>#0</code>，不排序不补帧"],
                ["返回地址怎么取",
                 "这一行给 <code>CFA = reg + offset</code> 与 <code>ra_offset</code> ⇒ 读 "
                 "<code>[CFA + ra_offset]</code> 的 4 字节",
                 "宿主 <code>on_backtrace</code> + <code>row_at(pc)</code>"],
                ["栈底",
                 "<code>ra == 0</code> ⇒ <b>正常终点</b>，<code>truncated = false</code>",
                 "S-16（真夹具 + 内存写 0）"],
                ["<code>truncated</code> 的四类成因",
                 "① pc 不在任何 FDE 里；② 这一行没给 RA 规则；③ 返回地址读不出（越界 / 总线拒绝）；"
                 "④ 帧数到上限（32）或返回地址落回已出现过的 pc（判环）",
                 "S-17 / S-18 / S-19"],
                ["上层帧变量",
                 "<b>不做</b>（用户拍板收窄）：每一帧只有 pc + 函数名；要看上层变量需逐帧恢复 "
                 "callee-saved 并另备一份变量位置",
                 "契约里<b>没有</b>变量字段（不是省了没填）"],
                ["内存条件文案（债务偿还）",
                 "条件档写明「条件里的内存读取按<b>写入</b>判定：只有 store 写进匹配值才停，"
                 "读到匹配值不触发」；地址 / 符号档不挂",
                 "C-10f；<code>data-bp-store-only-note</code>"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s13",
    "h": "13 局部变量的候选列表（v0.132 落地）",
    "blocks": [
        ("p", "批 E 收口时用户拍板「两态空态只做轻档」，把「当前帧有哪些变量」推到另立批。"
              "到 v0.130 收官时，变量表与 CFI 都已就位，缺的只是<b>把已有的真相摆出来</b> —— "
              "用户写条件时仍不知道这个帧里能写哪些名字。本节回写这批的落地口径："
              "<b>一条只读命令 + 一张只读卡</b>，复用 §12 的 DWARF / CFI 结果，"
              "<b>不新增任何解析</b>，也不回写任何状态。"),
        ("caption", "表 21 · 局部变量候选列表（<code>ListLocals</code>）口径（v0.132）"),
        ("table", {
            "widths": ["22%", "52%", "26%"],
            "head": ["项", "定稿做法", "落点 / 判别力"],
            "rows": [
                ["命令",
                 "<code>CommandPayload::ListLocals</code>（unit 变体 · <b>只读</b>）："
                 "按<b>当前 pc</b> 取它所在的帧，不动任何状态",
                 "前端只在点「列出」时发一条（C-15c）"],
                ["回执",
                 "<code>EventPayload::Locals { function, vars, truncated }</code>；"
                 "每个变量是 <code>LocalVarInfo { name, location }</code>",
                 "<code>contract/src/model.rs</code>"],
                ["<code>location</code> 谁渲染",
                 "<b>内核</b>渲染成人读文本；前端<b>不</b>二次解释、不拼自己的格式、"
                 "不建第二份寄存器名表",
                 "<code>locals.rs::LocalLocation::describe()</code>；名字表唯一位于 "
                 "<code>render::reg_name</code>"],
                ["位置写法（唯一四种）",
                 "寄存器 ⇒ ABI 名（<code>a0</code>，不写 <code>x10</code>）；栈上 ⇒ "
                 "<code>[base±offset]</code>；偏移 0 ⇒ <code>[s0]</code>（不写 <code>+0</code>）；"
                 "正偏移必带 <code>+</code>",
                 "locals 单测：<code>s0</code> / <code>a0</code> / <code>[s0-24]</code> / "
                 "<code>[sp+16]</code> / <code>[s0]</code> 逐字断言"],
                ["「没有名字」≠「没有变量」（两态空态）",
                 "<code>function</code> 缺省 ⇒ pc 不在<b>任何</b>已知函数里（不编名字）；"
                 "<code>function</code> 有值而 <code>vars</code> 空 ⇒ 区间收了、位置算不出来"
                 "（读不到 CFI）⇒ 前端两条文案<b>互斥</b>",
                 "S-23 / S-22；<code>data-locals-no-function</code> / "
                 "<code>data-locals-empty</code>（C-15b）"],
                ["上限与截断",
                 "128 条上限；超了置 <code>truncated</code> 并在卡上说「列表不完整」"
                 "（<b>不静默截</b>）；空列表<b>不是</b>截断",
                 "<code>on_list_locals::MAX_VARS</code>；S-22 / S-23 均断言 "
                 "<code>truncated = false</code>"],
                ["不带值、不带类型",
                 "面板只回答「这个名字此刻在哪儿」；值要按位置拼地址再读内存 / 读寄存器，"
                 "类型要顺 <code>DW_AT_type</code> 再走一遍 DIE ⇒ 都<b>另立批</b>，"
                 "契约里<b>没有</b>这两个字段（不是省了没填）",
                 "契约注释 + 本批遗留问题"],
                ["不做条件输入补全",
                 "点变量行<b>不</b>把名字填进条件框；卡上的 "
                 "<code>data-locals-hint</code> 直接写明「点行不填条件（本批只读）」",
                 "C-15d（钉住「不填、不加断点」）"],
                ["快照语义",
                 "列表是<b>一次查询的结果</b>，不随 pc 变化自动刷新；要看新帧再点一次"
                 "（不偷偷重发、不猜用户想看哪一帧）",
                 "<code>debug-wiring.js</code> 的 <code>debug.locals</code> 订阅原样存"],
                ["真夹具实测行",
                 "停在 <code>main</code>（pc 0x48）⇒ <code>x [s0-24]</code>、"
                 "<code>y [s0-20]</code>；换到 <code>helper</code>（pc 0x10）⇒ "
                 "<code>a [s0-36]</code>、<code>t [s0-20]</code>",
                 "S-20 / S-21（走真实 ELF 装载路径）"],
            ],
        }),
        ("svg", "locals"),
        ("mobile_flow", [
            ("1", "点「列出」⇒ 发一条 <code>ListLocals</code>（只读，不改状态）"),
            ("2", "pc 不落在任何函数区间 ⇒ <code>function</code> 缺省，"
                  "文案「当前 pc 不在任何已知函数里」"),
            ("3", "区间在、但读不到 CFI ⇒ <code>function</code> 有值、<code>vars</code> 空，"
                  "文案「X 里没有可展示的变量」"),
            ("4", "区间 + CFI 都在 ⇒ 列出「名字 + 位置」（≤128 条，超了标截断）"),
        ]),
        ("callout", "为什么位置必须由内核渲染",
         "同一个变量有三种写法：寄存器有别名（<code>s0</code> = <code>x8</code> = <code>fp</code>）、"
         "栈偏移有正负号、偏移 0 写不写 <code>+0</code>。前端各拼一套必然分叉 —— 面板显示 "
         "<code>[x8+-24]</code>、条件框只认 <code>[s0-24]</code>，用户抄不进去。"
         "所以 <code>describe()</code> 是<b>唯一落点</b>，且复用条件文法用的那张 "
         "<code>reg_name</code> 表：<b>显示的名字</b>与<b>能写进条件框的名字</b>同源。"),
    ],
})

# ── 14 断点恢复放过 + 符号联想（v0.134 / v0.135 落地） ─────────────────────
SECTIONS.append({
    "id": "s14",
    "h": "14 断点恢复放过与符号输入的联想（v0.134 / v0.135）",
    "blocks": [
        ("p", "本节回写三个<b>行为级</b>修订，都不动契约、不动断点表结构："
              "<b>① 停在断点上之后的恢复</b>（v0.134 · Bug）；"
              "<b>② 符号档的自动联想与下拉</b>（v0.135）；"
              "<b>③ 校验类报错的存活期</b>（v0.135）。"),
        ("callout", "为什么「停在断点上」必须放过一条",
         "断点判定挂在<b>取指之前</b>（§3 表 3）⇒ 命中时那条指令<b>没有执行</b>、"
         "<code>pc</code> <b>没有前进</b>（K-13 钉住）。断点表里只有地址 / 使能 / 条件 / 命中计数"
         "——<b>没有</b>「这一条刚报过」这个状态。于是恢复的第一轮又从<b>同一个 <code>pc</code></b> "
         "判一次 ⇒ 立刻以 <code>Breakpoint</code> 再停一次、指令数一动不动 ⇒ "
         "用户感受就是「断住之后继续不了」（实测：首次停 <code>pc=0x0c</code>；再「继续」、"
         "「单步」、再「继续」三次读数全是 <code>pc=0x0c / Breakpoint / executed=3</code>）。"),
        ("caption", "表 22 · 断点恢复放过一次（v0.134）"),
        ("table", {
            "widths": ["24%", "50%", "26%"],
            "head": ["项", "定稿做法", "落点 / 判别力"],
            "rows": [
                ["钩子位置",
                 "<b>不动</b>：判定仍在<b>取指之前</b>（用户 2026-11-22 明确口径）⇒ 停住时"
                 "该条指令未执行、<code>pc</code> 等于断点地址",
                 "<code>m17_backend::IssExecutor::step</code> 的 (1) 段"],
                ["放过位住哪儿",
                 "后端字段 <code>SelfIssBackend::resume_skip_pc: Option&lt;u32&gt;</code>"
                 "（跨批次）——执行器每批次新建 ⇒ 挂执行器等于每批都丢",
                 "与 <code>last_poll_ps</code> 同一条理由"],
                ["谁写、谁读",
                 "停住时写（<code>step()</code> 命中分支 + <code>step_n</code> 取指前预判）；"
                 "恢复的第一条指令读一次（<b>只读接口</b>给宿主报告用，不消费）",
                 "<code>resume_skip_applies()</code> / <code>halt_on_breakpoint_before_fetch()</code>"],
                ["消费时机",
                 "<b>判过即清</b>（只放过一条）⇒ 指令执行后 <code>pc</code> 前进，后续轮次照常参与判定",
                 "K-14 第二次「继续」停在下一条断点"],
                ["<code>reset</code> / <code>restore</code>",
                 "清位：<code>pc</code> 被换掉之后，旧的放过位不许再吞新程序的断点",
                 "<code>reset</code> / <code>restore</code> / <code>restore_full</code> 三处"],
                ["命中计数",
                 "被放过的这一条<b>不</b>计数（同一次停只算一次）⇒ 用户看到的命中数不再虚涨",
                 "K-14：<code>id=1</code> 的 <code>hit_count</code> 仍为 1"],
                ["哪些停点会放过",
                 "地址断点与观察式条件断点（<code>kind == Condition</code>）<b>共用</b>同一条规则"
                 "——谓词在同一个 <code>pc</code> 上反复为真同样会卡死",
                 "按 <code>pc</code> 生效（不按条目）"],
                ["已知边角",
                 "停住时若在<b>同一 <code>pc</code></b> 上新加一条断点，恢复的那一条也会被放过一次"
                 "（与 GDB「先走一步」同义；未做「按条目放过」）",
                 "本批遗留问题，未扩表结构"],
                ["用例",
                 "<b>K-14</b>（0x0c + 0x10 双断点：第二次「继续」停 0x10、<code>0x100</code> 读到 49、"
                 "计数仍 1）· <b>K-15</b>（<code>StepN{1}</code> 后 <code>pc</code> 前进且 store 生效）",
                 "<code>tests_v057_debug.rs</code>"],
            ],
        }),
        ("caption", "表 23 · 符号档的自动联想与报错存活期（v0.135）"),
        ("table", {
            "widths": ["24%", "50%", "26%"],
            "head": ["项", "定稿做法", "落点 / 判别力"],
            "rows": [
                ["触发方式",
                 "<b>边打字边联想</b>：输入<b>≥4 个字符</b>（含 4）才发查询；"
                 "<b>Tab 不再触发</b>（用户明确口径：Tab 交回浏览器焦点切换）",
                 "<code>SYMBOL_SUGGEST_MIN_CHARS = 4</code>；C-3 / C-3b"],
                ["防抖",
                 "停手 <b>180 ms</b> 再发一条 <code>ResolveSymbol</code>（连续打字只留最后一次）",
                 "<code>SYMBOL_SUGGEST_DEBOUNCE_MS = 180</code>；C-3"],
                ["呈现",
                 "<b>组合框</b>：输入框 <code>role='combobox'</code> + <code>aria-expanded</code>，"
                 "候选<b>浮在输入框下方</b>竖排、可滚动；收起时<b>不占位</b>"
                 "（<code>data-bp-suggest='closed'</code>）",
                 "<code>debug-panel.js</code> + <code>style.css</code> 的 <code>.bp-suggest</code>"],
                ["开 / 收判据",
                 "纯函数：输入 ≥4、<b>回执的名字与当前输入逐字相同</b>、且没被 Esc / 选中收过"
                 "⇒ 打开；敲下一个字符的瞬间旧列表收起，等新回执",
                 "<code>symbolSuggestOpen()</code>；C-3c"],
                ["键位",
                 "↑↓ 在打开的下拉里换一条（列表关着 ⇒ 先按当前输入重查一次）；"
                 "回车 = 选中高亮那条（填名字 + 收起）；<b>再按一次回车</b>才是「加断点」；Esc = 收起"
                 "（<b>两步语义保留</b>，用户 2026-11-22 拍板）",
                 "C-3b / C-3c（Tab 一律不查）"],
                ["候选点击",
                 "<b>不变</b>：点候选直接下断点（<code>addr</code> + <code>source_hint</code> 逐字来自候选）",
                 "C-7（既有用例一字未改）"],
                ["空 / 短文不发",
                 "<4 个字符<b>根本不发</b> <code>ResolveSymbol</code> ⇒ 「符号名为空」"
                 "那条错误不再由正常输入触发（命令本身仍不校验，内核真拒时原文照显）",
                 "<code>scheduleSymbolSuggest()</code> 的早退"],
                ["报错存活期",
                 "校验类报错（<code>E_DBG-04</code> / <code>E_DBG-02</code> / <code>E_DBG-03</code>）"
                 "在输入位与顶部横幅上都只活 <b>6 秒</b>，到点自己消失；输入框里一敲字仍<b>立即</b>清；"
                 "内容被新错误顶掉时旧定时器<b>不清</b>。"
                 "<b>6 秒为定稿数值</b>（用户 2026-11-22 拍板）⇒ <b>全族统一</b>、不按错误码区分寿命",
                 "<code>ERROR_TTL_MS = 6000</code>；C-13 过期用例"],
                ["原文不变",
                 "错误码与内核原文到达时<b>一字不改</b>（过期只是不显示）⇒ 真机断言仍逐字可比",
                 "C-13（逐字相等）"],
            ],
        }),
        ("svg", "suggest"),
        ("mobile_flow", [
            ("1", "符号档输入框：敲字时高亮位归零、旧下拉就地收起（不重渲染，不打断输入）"),
            ("2", "不足 4 个字符 ⇒ 什么都不发（连「符号名为空」都不可能触发）"),
            ("3", "≥4 个字符 ⇒ 停手 180 ms 发一条 <code>ResolveSymbol</code> ⇒ 回执到了下拉打开"),
            ("4", "Esc / 选中一条 ⇒ 收起；再敲字 ⇒ 重新打开并重查"),
        ]),
        ("callout", "为什么联想必须由「输入」触发，而不是 Tab",
         "Tab 是<b>键盘焦点</b>的语义：用户按 Tab 通常是想换到下一个控件。把查询绑在 Tab 上，"
         "等于让「换焦点」变成「发一条命令」——而查询失败（空名字 ⇒ <code>E_DBG-04</code>）又会"
         "在面板上留下一条不会消失的红字。改成<b>输入驱动 + 最小 4 字符</b>之后，"
         "查询只在「用户确实写出了可查的前缀」时发生，失败路径基本消失；剩下的失败（极端名字）"
         "由<b>6 秒存活期</b>兜底。"),
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


LOCALS_SVG = """
<svg class="flow-svg" viewBox="0 0 1020 420" xmlns="http://www.w3.org/2000/svg"
  role="img" aria-label="当前帧局部变量列表的三种回执形态">
  <defs>
    <marker id="l1" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/>
    </marker>
    <marker id="l2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#9aa1af"/>
    </marker>
  </defs>

  <rect x="360" y="12" width="300" height="56" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="510" y="36" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">点「列出」⇒ ListLocals</text>
  <text x="510" y="56" text-anchor="middle" font-size="11.5" fill="#4b5563">只读 · 按当前 pc 取帧 · 不动任何状态</text>
  <line x1="510" y1="68" x2="510" y2="86" stroke="#1e3a8a" stroke-width="2" marker-end="url(#l1)"/>

  <rect x="60" y="86" width="900" height="42" rx="6" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="510" y="112" text-anchor="middle" font-size="12.5" fill="#4b5563">判别只看两件事：pc 落在函数区间里吗 · 那个函数的变量位置算得出来吗</text>

  <!-- 形态一：没有函数区间 -->
  <rect x="40" y="146" width="300" height="76" rx="8" fill="#fff7ed" stroke="#fdba74"/>
  <text x="190" y="173" text-anchor="middle" font-size="13" font-weight="700" fill="#9a3412">没有函数区间</text>
  <text x="190" y="195" text-anchor="middle" font-size="11.5" fill="#9a3412">pc 不在任何已知函数里</text>
  <line x1="342" y1="184" x2="376" y2="184" stroke="#9aa1af" stroke-width="1.8" marker-end="url(#l2)"/>
  <rect x="380" y="146" width="600" height="76" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="400" y="173" font-size="13" font-weight="700" fill="#1f2937">function 缺省 · vars 空</text>
  <text x="400" y="195" font-size="12" fill="#4b5563">前端文案「当前 pc 不在任何已知函数里」</text>
  <text x="400" y="213" font-size="11.5" fill="#6b7280">S-23 ｜ 不编一个名字</text>

  <!-- 形态二：区间在、读不到 CFI -->
  <rect x="40" y="234" width="300" height="76" rx="8" fill="#fff7ed" stroke="#fdba74"/>
  <text x="190" y="261" text-anchor="middle" font-size="13" font-weight="700" fill="#9a3412">区间在 · 没有 CFI</text>
  <text x="190" y="283" text-anchor="middle" font-size="11.5" fill="#9a3412">位置算不出来 ⇒ 变量一个不留</text>
  <line x1="342" y1="272" x2="376" y2="272" stroke="#9aa1af" stroke-width="1.8" marker-end="url(#l2)"/>
  <rect x="380" y="234" width="600" height="76" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="400" y="261" font-size="13" font-weight="700" fill="#1f2937">function 有值 · vars 空</text>
  <text x="400" y="283" font-size="12" fill="#4b5563">前端文案「X 里没有可展示的变量（没有调试信息，或读不到 CFI）」</text>
  <text x="400" y="301" font-size="11.5" fill="#6b7280">S-22 ｜ 空列表不是截断</text>

  <!-- 形态三：能列 -->
  <rect x="40" y="322" width="300" height="76" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="190" y="349" text-anchor="middle" font-size="13" font-weight="700" fill="#166534">区间 + CFI 都在</text>
  <text x="190" y="371" text-anchor="middle" font-size="11.5" fill="#166534">位置由 describe() 落成人读文本</text>
  <line x1="342" y1="360" x2="376" y2="360" stroke="#1e3a8a" stroke-width="2" marker-end="url(#l1)"/>
  <rect x="380" y="322" width="600" height="76" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="400" y="349" font-size="13" font-weight="700" fill="#166534">列表：名字 + 位置</text>
  <text x="400" y="371" font-size="12" fill="#166534">main ⇒ x [s0-24] · y [s0-20]   ／   helper ⇒ a [s0-36] · t [s0-20]</text>
  <text x="400" y="389" font-size="11.5" fill="#166534">S-20 / S-21 ｜ ≤128 条，超了标 truncated</text>

  <text x="40" y="414" font-size="11.5" fill="#6b7280">
图注：前两种都是合法回答 —— 「不是任何函数」与「函数在、变量读不到」必须分开说，不能合并成一句「没有数据」。
  </text>
</svg>
"""

SUGGEST_SVG = """
<svg class="flow-svg" viewBox="0 0 1020 420" xmlns="http://www.w3.org/2000/svg"
  role="img" aria-label="符号输入框的联想状态：不足四字符、达到四字符、回执到达、收起">
  <defs>
    <marker id="g1" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/>
    </marker>
    <marker id="g2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#9aa1af"/>
    </marker>
  </defs>

  <rect x="360" y="12" width="300" height="56" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="510" y="36" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">符号档：在输入框里敲字</text>
  <text x="510" y="56" text-anchor="middle" font-size="11.5" fill="#4b5563">Tab 不再触发查询（旧交互退役）</text>
  <line x1="510" y1="68" x2="510" y2="86" stroke="#1e3a8a" stroke-width="2" marker-end="url(#g1)"/>

  <rect x="60" y="86" width="900" height="42" rx="6" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="510" y="112" text-anchor="middle" font-size="12.5" fill="#4b5563">判别只看两件事：字符够不够 4 个 · 回执里的名字是不是当前这串输入</text>

  <rect x="40" y="146" width="300" height="76" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="190" y="173" text-anchor="middle" font-size="13" font-weight="700" fill="#4b5563">不足 4 个字符</text>
  <text x="190" y="195" text-anchor="middle" font-size="11.5" fill="#6b7280">不下拉、不发查询</text>
  <line x1="342" y1="184" x2="376" y2="184" stroke="#9aa1af" stroke-width="1.8" marker-end="url(#g2)"/>
  <rect x="380" y="146" width="600" height="76" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="400" y="173" font-size="13" font-weight="700" fill="#1f2937">只有草稿在增长</text>
  <text x="400" y="195" font-size="12" fill="#4b5563">「符号名为空：无法查询符号」不可能由正常输入触发</text>
  <text x="400" y="213" font-size="11.5" fill="#6b7280">C-3b ｜ Tab 也不发</text>

  <rect x="40" y="234" width="300" height="76" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="190" y="261" text-anchor="middle" font-size="13" font-weight="700" fill="#166534">≥4 个字符</text>
  <text x="190" y="283" text-anchor="middle" font-size="11.5" fill="#166534">停手 180 ms 才发</text>
  <line x1="342" y1="272" x2="376" y2="272" stroke="#1e3a8a" stroke-width="2" marker-end="url(#g1)"/>
  <rect x="380" y="234" width="600" height="76" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="400" y="261" font-size="13" font-weight="700" fill="#166534">ResolveSymbol { name }</text>
  <text x="400" y="283" font-size="12" fill="#166534">连续打字只留最后一次（防抖）⇒ 回执 SymbolResolved</text>
  <text x="400" y="301" font-size="11.5" fill="#166534">C-3 ｜ 一条查询，不多发</text>

  <rect x="40" y="322" width="300" height="76" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="190" y="349" text-anchor="middle" font-size="13" font-weight="700" fill="#1e3a8a">回执的名字 == 当前输入</text>
  <text x="190" y="371" text-anchor="middle" font-size="11.5" fill="#1e3a8a">组合框展开（listbox）</text>
  <line x1="342" y1="360" x2="376" y2="360" stroke="#1e3a8a" stroke-width="2" marker-end="url(#g1)"/>
  <rect x="380" y="322" width="600" height="76" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="400" y="349" font-size="13" font-weight="700" fill="#1e3a8a">↓↑ 换一条 · 回车选中 · Esc 收起</text>
  <text x="400" y="371" font-size="12" fill="#4b5563">选中只填名字并收起 ⇒ 再按一次回车才「加断点」（组合框常规语义）</text>
  <text x="400" y="389" font-size="11.5" fill="#6b7280">C-3c ｜ 收起后再敲字自动重开</text>

  <text x="40" y="414" font-size="11.5" fill="#6b7280">
图注：连线上的「灰」表示什么都不发生（只有草稿变化）；「蓝」表示真的发了一条命令或真的画出了下拉。
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
            out.append({"table": TABLE_SVG, "timing": TIMING_SVG, "locals": LOCALS_SVG,
                        "suggest": SUGGEST_SVG}[val])
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
.kpis { display:grid; grid-template-columns:repeat(auto-fit,minmax(215px,1fr)); gap:10px; margin:6px 0 22px; }
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
不要手改 HTML。本文为设计定稿（v0.7）：A→E + CFI + 栈回溯 + 真相候选列表 + 断点恢复放过 + 符号联想
均已落地并合入 main（基线 1b62c53），文中「测试设计」列的用例、§12 / §13 / §14 的用例已在内核 / 前端用例中实现。
v0.7 只订正一处<b>归属</b>：地址文法（<code>AddressBase</code> / <code>AddressSpec</code> /
<code>parse_address_syntax</code>）已于 v0.137 从 <code>watch_expr.rs</code> 并回
<code>cond.rs</code> 的 <code>mod addr</code> —— 文中所有「既有实现」指针随之改指，语义与文法口径不变。</p>
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
