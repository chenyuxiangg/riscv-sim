#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 STAGE-54（v0.79 中断波形通道）的 HTML 单文件设计文档。

数据驱动：所有正文/表格都在 `SECTIONS` 里，渲染器只管排版 ⇒ 改内容不动版式。
产物：docs/detailed-design/STAGE-54_V079_IRQ_Waveform_Channels.html
用法：python3 tools/build_stage54_doc.py [输出路径]
"""

from __future__ import annotations

import html
import sys
from pathlib import Path

OUT_DEFAULT = Path("docs/detailed-design/STAGE-54_V079_IRQ_Waveform_Channels.html")

TITLE = "STAGE-54 · v0.79 中断波形通道（每条中断一个 0/1 电平通道）"
SUBTITLE = "RISC-V 仿真平台 · 波形可观测性改造 · 冻结规格"
VERSION = "版本 v1.0 · 2026-11-18 · 分支 feature/irq-waveform-channels · 基线 1cd8b05"

# ── 摘要卡片 ────────────────────────────────────────────────────────────
KPI = [
    ("1", "条废弃通道", "「中断脉冲」把线号当电平画，已删除"),
    ("3", "条核中断通道", "software / timer / external"),
    ("1", "条/台设备", "按板上 irq_line 现算，加删器件自动跟随"),
    ("0", "处契约变更", "WaveformStream 本来就带 label"),
]

# ── 章节数据 ────────────────────────────────────────────────────────────
SECTIONS = []

SECTIONS.append({
    "id": "s1",
    "h": "1 为什么要改：那条波形画的不是电平",
    "blocks": [
        ("p", "现场现象：波形面板上只有一条 <b>M16.irq_pulse</b>，纵轴 1～3，常驻在 <b>3</b>，"
              "偶发短暂掉到 1。用户据此判断「波形初值是 3」——但 3 不是电平，是<b>中断线号</b>。"),
        ("code", "第一帧实测\n{\"t_ps\":\"2089000000\",\"value\":3}   // value = line，不是 level"),
        ("p", "成因：M-16 在每次 <i>request()</i> 时调 <i>sample_irq_pulse(now_ps, u64::from(line))</i>，"
              "M-18 的脉冲通道（<i>pulse=true</i>）把 <i>line</i> 原样当采样值推给 M-19，"
              "M-19 按「值变才发边沿」记账 ⇒ 于是「3 一直没有变化」看起来像恒定的电平，"
              "而 UART 收到一帧时冒出 line=1 的脉冲。"),
        ("callout", "结论", "这条通道同时丢失了两件用户真正要的信息：① 有没有中断（0/1）；"
                            "② 是哪一根线（核中断 / 哪台设备）。一条通道装不下两个维度 ⇒ 必须拆。"),
        ("caption", "表 1 · 旧通道 vs 新通道"),
        ("table", {
            "widths": ["16%", "26%", "26%", "32%"],
            "head": ["", "旧：中断脉冲", "新：0/1 电平通道", "用户得到什么"],
            "rows": [
                ["值域", "中断线号（1/3/4…），数值无上界",
                 "0 或 1（单比特，M-19 按值去重）",
                 "一眼看到「有/无」，不再被线号误导"],
                ["通道数", "1 条（全部中断挤在一起）", "3 条核 + 每台已接线设备 1 条",
                 "能分辨是哪根线、哪台设备"],
                ["分辨率", "只有「收到了一个事件」", "每条通道各自抬/落",
                 "能看出中断挂起多久、何时被清"],
                ["筛选", "无（就一条）", "分组复选框 + 全选/仅核/清空",
                 "只留感兴趣的线，图例不糊"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s2",
    "h": "2 概念空间：三套编号不是一套",
    "blocks": [
        ("p", "这次改动最容易踩的坑是「用一个数字表达两个身份」。本平台的编号分成三套，"
              "互相之间<b>只映射、不合并</b>。"),
        ("caption", "表 2 · 三套编号各自的含义"),
        ("table", {
            "widths": ["18%", "14%", "26%", "42%"],
            "head": ["编号空间", "取值（本平台）", "谁在用", "能不能拿来当通道身份"],
            "rows": [
                ["L2 平台内部线号", "1 / 3 / 4（线 2 已退役空缺）", "M-16 的 pending 位、CLINT msip / mtimecmp 接线",
                 "<b>不能</b>：只有 3 个值，装不下设备"],
                ["RISC-V cause", "中断 3 / 7 / 11（bit31=1）", "mcause、mip 的 MSIP/MTIP/MEIP 位",
                 "<b>不能</b>：cause 11 与设备源 11 同数字，会撞"],
                ["PLIC 源号", "板上 <i>irq_line</i>，1..1023", "PLIC 网关、设备请求",
                 "<b>不能直接</b>：u8 的 ch 装不下 1023"],
            ],
        }),
        ("callout", "对齐口径", "线号 1↔MEIP(11)、3↔MTIP(7)、4↔MSIP(3) 是<b>固定映射</b>："
                               "核只认 mcause/mip，设备只认 PLIC 源号，中间由宿主翻译。"),
        ("caption", "表 3 · 现行映射表（与代码常量一一对应）"),
        ("table", {
            "widths": ["20%", "14%", "18%", "18%", "24%"],
            "head": ["语义", "L2 线号", "mip 位", "mcause", "优先级"],
            "rows": [
                ["timer（CLINT mtimecmp）", "IRQ_TIMER_COMPARE = 3", "MTIP 7", "0x8000_0007", "2（最高）"],
                ["external（PLIC）", "IRQ_UART_RX_READY = 1（语义=外部）", "MEIP 11", "0x8000_000B", "1"],
                ["software（CLINT msip）", "IRQ_SOFTWARE = 4", "MSIP 3", "0x8000_0003", "0"],
                ["（退役）UART tx-empty", "线 2 空缺、不重排", "—", "—", "—"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s3",
    "h": "3 通道表：核 3 条 + 每台设备 1 条",
    "blocks": [
        ("caption", "表 4 · 中断波形通道全集"),
        ("table", {
            "widths": ["12%", "22%", "22%", "10%", "28%"],
            "head": ["组", "流 id", "图例标签", "值", "边沿从哪来"],
            "rows": [
                ["核", "<i>irq.core.software</i>", "software · IRQ 3", "0/1", "M-16 request() 抬 / clear() 落"],
                ["核", "<i>irq.core.timer</i>", "timer · IRQ 7", "0/1", "M-16 poll() 抬 / set_timer_compare() 落"],
                ["核", "<i>irq.core.external</i>", "external · MEIP", "0/1", "宿主 sync_interrupts()：PLIC 有无可交付源"],
                ["外设", "<i>irq.dev.{id}</i>", "{id} · IRQ {source}", "0/1", "宿主在 Plic::set_source_level 处同步采样"],
            ],
        }),
        ("callout", "为什么 external 不写 IRQ 11",
         "cause 11 与某台设备的 PLIC 源 11 是同一个数字（tpl-soc 的 GPIO0 恰好就是源 11）。"
         "写成 <i>external · MEIP</i> 才能在界面上不与设备通道混名。"),
        ("caption", "表 5 · 设备通道的粒度（按用户拍板）"),
        ("table", {
            "widths": ["26%", "22%", "52%"],
            "head": ["粒度", "结论", "理由 / 代价"],
            "rows": [
                ["每台已接线设备一条", "采用", "板级声明里 <i>irq_line</i> 就是 PLIC 源号，天然一一对应"],
                ["UART RX / TX 拆两条", "不做", "真 16550 只有一根 IRQ（同 MEIE 位）⇒ 物理上分不开；"
                                        "要分只能另开器件级二级通道，本批明确不做"],
                ["PLIC 32 个源全建（含空线）", "不做", "空线永远不动，图例只会更糊"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s4",
    "h": "4 通道号：内部键，不是身份",
    "blocks": [
        ("p", "M-18 的通道号是 <i>u8</i>，只作内部键使用。对外身份是<b>流 id 与图例标签</b>，"
              "前端与断言都不解析这个数字。"),
        ("caption", "表 6 · 通道号分配"),
        ("table", {
            "widths": ["18%", "30%", "52%"],
            "head": ["区间", "用途", "说明"],
            "rows": [
                ["0～4", "既有器件通道", "TIM0.CMP / GPIO0.OUT / 空闲 / KEY0.IN / UART0.TX"],
                ["16 / 17 / 18", "核中断", "software / timer / external"],
                ["19 + 声明序号", "设备通道", "按板上中断表顺序编号，随平台重建"],
            ],
        }),
        ("callout", "为什么不用 cause / 源号当 ch",
         "① 会撞号：cause 11 与源 11、cause 3 与源 3；② <i>u8</i> 装不下 PLIC 源号的 1023；"
         "③ 设备增删后源号会重排 ⇒ 数字当身份历史会错位。"),
    ],
})

SECTIONS.append({
    "id": "s5",
    "h": "5 采样路径：事件驱动，不轮询",
    "blocks": [
        ("p", "单比特通道只在 0↔1 变化时产生边沿（M-19 按值去重）⇒ 重复请求不会刷边沿，"
              "长时间挂起也只占一个点。"),
        ("svg", "flow"),
        ("mobile_flow", [
            ("1", "设备请求：UART/GPIO 拉高自身 IRQ 电平"),
            ("2", "PLIC 网关：set_source_level(src, 1) ⇒ 同步写设备通道 = 1"),
            ("3", "宿主裁决：deliverable_source() 有可交付源 ⇒ external 通道 = 1"),
            ("4", "M-16 登记：request(line) ⇒ 该线通道 = 1，等 M-17 在量子边界仲裁"),
            ("5", "清除：clear() ⇒ 该线通道 = 0；PLIC 源落下不 retract（Renode 口径）"),
        ]),
        ("caption", "表 7 · 谁在什么时候写哪条通道"),
        ("table", {
            "widths": ["34%", "18%", "48%"],
            "head": ["事件", "写谁", "写什么 / 注意"],
            "rows": [
                ["M-16 request() 真的登记挂起", "对应线通道", "写 1；被掩码丢弃的请求不算"],
                ["M-16 clear()", "对应线通道", "写 0；含 timer 撤线、软中断落、外部无可交付源"],
                ["Plic::set_source_level(src, level)", "该设备通道", "写 level；这是<b>网关电平</b>，不是核视角"],
                ["设备改板 / 重装工程", "整表重建", "install_probe_pipeline() 按 peripheral_irqs 现算"],
            ],
        }),
        ("callout", "两条通道允许不同步",
         "外设通道画的是「设备有没有在请求 PLIC」，external 画的是「核有没有看到 MEIP」。"
         "PLIC 源落下不会 retract 已送进 PLIC 的请求 ⇒ 设备通道先落、external 后落，这是正确行为。"),
    ],
})

SECTIONS.append({
    "id": "s6",
    "h": "6 动态适应：产品视图加器件，波形自己长出来",
    "blocks": [
        ("p", "用户要求「在产品视图增加器件实例并分配中断号之后，波形视图动态适应」——"
              "这条链路本来就是现成的，本批只是把中断通道挂到它上面。"),
        ("caption", "表 8 · 重建链路（从上到下，一次改板全走完）"),
        ("table", {
            "widths": ["30%", "34%", "36%"],
            "head": ["步骤", "入口", "结果"],
            "rows": [
                ["1 重装内存图", "rebuild_platform() → reset_memory_map()", "清掉旧段与旧外设实例"],
                ["2 重编译中断表", "map_memory() → compile_peripheral_irqs()", "peripheral_irqs 变成新板的设备→源号"],
                ["3 重装 tick", "install_peripheral_ticker()", "新器件实例拿到时间驱动"],
                ["4 重装探针管道", "install_probe_pipeline()", "中断通道按新表重建（多/少几条）"],
                ["5 装外设探针", "attach_peripheral_probes()", "器件级信号（GPIO0.OUT 等）重新挂上"],
            ],
        }),
        ("caption", "表 9 · 前端如何不丢勾选"),
        ("table", {
            "widths": ["26%", "34%", "40%"],
            "head": ["状态", "键", "行为"],
            "rows": [
                ["用户勾选", "流 id（如 irq.dev.uart0）", "按 id 保留；不按序号、不按位置"],
                ["新出现的通道", "默认值", "核中断默认勾，外设默认不勾"],
                ["消失的通道", "—", "静默丢弃，不报错"],
                ["非中断通道", "—", "保持可见（GPIO0.OUT 等不受筛选影响）"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s7",
    "h": "7 前端交互",
    "blocks": [
        ("caption", "表 10 · 筛选条元素"),
        ("table", {
            "widths": ["20%", "26%", "54%"],
            "head": ["元素", "位置", "行为"],
            "rows": [
                ["分组", "核中断 / 外设中断", "按流 id 前缀分组，不靠标签文本猜"],
                ["复选框", "每条通道一项", "显示通道标签 + 边沿条数（N edges）"],
                ["快捷按钮", "全选 / 仅核 / 清空", "同步刷新 DOM 复选框状态"],
                ["默认", "打开即生效", "核 3 条勾选；外设全部不勾"],
            ],
        }),
        ("caption", "表 11 · 重画策略"),
        ("table", {
            "widths": ["24%", "34%", "42%"],
            "head": ["动作", "做什么", "为什么"],
            "rows": [
                ["勾选变化", "只重算 ECharts option", "不重新 dump 边沿，避免把历史刷掉"],
                ["新数据到达", "按当前可见集重画", "隐藏通道不参与渲染，但不丢数据"],
                ["自报属性", "同步 data-wave-chart-*", "门禁脚本与回归用例可断言"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s8",
    "h": "8 改动清单",
    "blocks": [
        ("caption", "表 12 · 文件级改动"),
        ("table", {
            "widths": ["40%", "60%"],
            "head": ["文件", "改了什么"],
            "rows": [
                ["crates/kernel/src/m18_probe/channel.rs", "删 CH_IRQ_PULSE；新增 16/17/18 与设备基址 19"],
                ["crates/kernel/src/m18_probe/registry.rs", "删 sample_irq_pulse（脉冲语义整体退役）"],
                ["crates/kernel/src/m16_interrupt/mod.rs", "新增 clear_sink：clear() 时发 0 边沿"],
                ["crates/kernel/src/kernel_host/mod.rs", "标签常量、irq_ch_for_line、irq_src_channels、"
                                                          "sample_irq_source、waveform_event 补标签"],
                ["front/views/components/wave-chart.js", "分组/默认选择/合并/可见集/筛选区 HTML"],
                ["front/views/m02_viewcomponents.js", "筛选状态、_redrawWaveChart、两个命令分支"],
                ["front/style.css", ".wave-filter 样式（限高滚动，不撑爆面板）"],
                ["front/tests/wave-irq-filter.test.mjs", "新增 8 条纯函数用例"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s9",
    "h": "9 测试矩阵",
    "blocks": [
        ("caption", "表 13 · 验证结果（本批实测）"),
        ("table", {
            "widths": ["26%", "26%", "16%", "32%"],
            "head": ["层", "用例", "结果", "钉住什么"],
            "rows": [
                ["M-18", "single_bit_level_channel_coalesces_same_value", "通过",
                 "0→1 发、1→1 不发、1→0 发、0→0 不发"],
                ["M-16 → M-18", "one_request_produces_exactly_one_level_edge", "通过",
                 "一次 request() 恰好一条 0→1，值为 1 而不是线号"],
                ["D-46", "中断通道集合与标签格式", "通过", "核 3 条必在；设备标签必须是「名 · IRQ 号」"],
                ["前端", "wave-irq-filter.test.mjs", "8/8", "分组 / 默认只勾核 / 合并保选 / 筛选区 HTML"],
                ["全量", "cargo test --workspace", "通过", "lib 1449、contract 281、d46 15/15"],
                ["门禁", "check-front.sh + stage14/16 + error_code + doc_check", "通过", "静态服务与文档口径"],
            ],
        }),
    ],
})

SECTIONS.append({
    "id": "s10",
    "h": "10 欠账与后续",
    "blocks": [
        ("list", [
            "<b>M-16 / M-18 的 HTML 详设</b>：本批已在原位加了 v0.79 修订说明（旧表述保留并标注取代），"
            "彻底改写要等这两份文档下一次整体改版。",
            "<b>STAGE-4 的 D-26</b>：同上，已补「v0.79 取消中间信号」一行。",
            "<b>门禁工具改造 R-1~R-4</b>（分段重跑 / 失败即 dump / 采集通道 health check / wasm mtime 前置）仍欠。",
            "<b>器件级二级通道</b>（UART RX/TX 分开）：本批明确不做，若将来要分，需在器件层开子通道。",
        ]),
        ("callout", "一条经验", "「一个数字能表达几个身份」必须在设计期问清楚。"
                              "线号、cause、PLIC 源号、通道号，四者只要混用一个，界面上就会出现"
                              "「3 到底是哪个 3」这种谁也无法自证的问题。"),
    ],
})

# ── SVG 流程图（宽屏） ──────────────────────────────────────────────────
FLOW_SVG = """
<svg class="flow-svg" viewBox="0 0 1040 400" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="中断波形采样路径">
  <defs>
    <marker id="ar" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/>
    </marker>
    <marker id="ar2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#9aa1af"/>
    </marker>
  </defs>
  <!-- 设备行 -->
  <rect x="20" y="40" width="200" height="76" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="120" y="70" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">外设（UART / GPIO）</text>
  <text x="120" y="94" text-anchor="middle" font-size="12.5" fill="#4b5563">拉高自身 IRQ 电平</text>
  <text x="120" y="112" text-anchor="middle" font-size="12.5" fill="#4b5563">（RX || TX，整块一个源）</text>

  <line x1="222" y1="78" x2="316" y2="78" stroke="#1e3a8a" stroke-width="2" marker-end="url(#ar)"/>
  <text x="269" y="66" text-anchor="middle" font-size="12" fill="#1e3a8a">请求</text>

  <!-- PLIC 网关 -->
  <rect x="318" y="34" width="220" height="92" rx="8" fill="#fff7ed" stroke="#fdba74"/>
  <text x="428" y="62" text-anchor="middle" font-size="15" font-weight="700" fill="#9a3412">PLIC 网关</text>
  <text x="428" y="86" text-anchor="middle" font-size="12.5" fill="#4b5563">set_source_level(src, level)</text>
  <text x="428" y="104" text-anchor="middle" font-size="12.5" fill="#4b5563">⇒ 设备通道写 level</text>
  <text x="428" y="120" text-anchor="middle" font-size="12" fill="#9a3412">（网关电平，不 retract）</text>

  <line x1="540" y1="78" x2="626" y2="78" stroke="#1e3a8a" stroke-width="2" marker-end="url(#ar)"/>
  <text x="583" y="66" text-anchor="middle" font-size="12" fill="#1e3a8a">可交付源</text>

  <!-- 核 */
  <rect x="628" y="30" width="222" height="104" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="739" y="58" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">hart（M 模式，3 根输入）</text>
  <text x="739" y="82" text-anchor="middle" font-size="12.5" fill="#4b5563">MEIP ← PLIC ／ MTIP ← CLINT</text>
  <text x="739" y="102" text-anchor="middle" font-size="12.5" fill="#4b5563">MSIP ← CLINT</text>
  <text x="739" y="122" text-anchor="middle" font-size="12" fill="#1e3a8a">external / timer / software 三条通道</text>

  <line x1="850" y1="78" x2="936" y2="78" stroke="#1e3a8a" stroke-width="2" marker-end="url(#ar)"/>

  <!-- 波形 */
  <rect x="938" y="40" width="86" height="76" rx="8" fill="#f0fdf4" stroke="#86efac"/>
  <text x="981" y="70" text-anchor="middle" font-size="14" font-weight="700" fill="#166534">M-19</text>
  <text x="981" y="90" text-anchor="middle" font-size="12" fill="#166534">波形回放</text>
  <text x="981" y="108" text-anchor="middle" font-size="12" fill="#166534">0/1 阶梯</text>

  <!-- 下半：M-16 登记 + 清除 -->
  <rect x="318" y="200" width="220" height="92" rx="8" fill="#eef2fb" stroke="#c7d2e5"/>
  <text x="428" y="228" text-anchor="middle" font-size="15" font-weight="700" fill="#1e3a8a">M-16 中断控制器</text>
  <text x="428" y="252" text-anchor="middle" font-size="12.5" fill="#4b5563">request(line) ⇒ 线通道 = 1</text>
  <text x="428" y="270" text-anchor="middle" font-size="12.5" fill="#4b5563">clear() ⇒ 线通道 = 0</text>
  <text x="428" y="286" text-anchor="middle" font-size="12" fill="#9a3412">（clear_sink：v0.79 新增）</text>

  <path d="M739,136 L739,180 L540,246" fill="none" stroke="#9aa1af" stroke-width="1.8" marker-end="url(#ar2)"/>
  <text x="668" y="212" text-anchor="middle" font-size="12" fill="#6b7280">登记 pending</text>

  <path d="M428,294 L428,330 L739,330 L739,136" fill="none" stroke="#9aa1af" stroke-width="1.6" stroke-dasharray="6 4" marker-end="url(#ar2)"/>
  <text x="586" y="322" text-anchor="middle" font-size="12" fill="#6b7280">clear() 撤线 ⇒ 发 0 边沿</text>

  <line x1="120" y1="118" x2="120" y2="246" stroke="#9aa1af" stroke-width="1.2" stroke-dasharray="5 4"/>
  <path d="M120,246 L316,246" fill="none" stroke="#9aa1af" stroke-width="1.2" stroke-dasharray="5 4" marker-end="url(#ar2)"/>
  <text x="222" y="238" text-anchor="middle" font-size="12" fill="#6b7280">电平变化 → 设备通道</text>

  <text x="20" y="378" font-size="12" fill="#6b7280">图注：实线 = 请求路径；虚线 = 清除/回写路径；橙色 = PLIC 网关（设备视角）；蓝色 = 核视角。</text>
</svg>
"""


def esc(t: str) -> str:
    return html.escape(t, quote=False)


def render_table(spec: dict) -> str:
    widths = spec["widths"]
    cols = "".join(f'<col style="width:{w}">' for w in widths)
    head = "".join(f"<th>{c}</th>" for c in spec["head"])
    rows = []
    for i, row in enumerate(spec["rows"]):
        cls = ' class="row1"' if i % 2 else ""
        cells = "".join(f"<td>{c}</td>" for c in row)
        rows.append(f"<tr{cls}>{cells}</tr>")
    return (
        '<div class="tbl-wrap"><table class="doc-table wide">'
        f"<colgroup>{cols}</colgroup><thead><tr>{head}</tr></thead>"
        f"<tbody>{''.join(rows)}</tbody></table></div>"
    )


def render_blocks(blocks) -> str:
    out = []
    for b in blocks:
        kind, val = b[0], b[1] if len(b) > 1 else None
        if kind == "p":
            out.append(f'<p class="body">{val}</p>')
        elif kind == "caption":
            out.append(f'<p class="caption">{val}</p>')
        elif kind == "table":
            out.append(render_table(val))
        elif kind == "code":
            out.append(f'<pre class="code">{esc(val)}</pre>')
        elif kind == "list":
            items = "".join(f"<li>{i}</li>" for i in val)
            out.append(f'<ul class="list">{items}</ul>')
        elif kind == "callout":
            out.append(
                f'<div class="callout"><span class="callout-tag">{val[0]}</span>'
                f'<span class="callout-txt">{val[1]}</span></div>'
            )
        elif kind == "svg" and val == "flow":
            out.append(FLOW_SVG)
        elif kind == "mobile_flow":
            steps = "".join(
                f'<div class="flow-step"><span class="num">{n}</span><span class="txt">{t}</span></div>'
                for n, t in val
            )
            out.append(f'<div class="flow-mobile">{steps}</div>')
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
.doc-title { font-size:26px; line-height:1.4; font-weight:900; color:var(--brand);
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
.code { background:var(--code-bg); color:#e2e8f0; font-family:'JetBrains Mono','Cascadia Mono',Consolas,monospace;
  font-size:12.5px; line-height:1.6; padding:12px 14px; border-radius:6px; overflow-x:auto;
  white-space:pre; margin:10px 0 16px; }
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
        f'<div class="kpi-d">{d}</div></div>'
        for n, t, d in KPI
    )
    return f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(TITLE)}</title>
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect x='4' y='4' width='24' height='24' rx='3' fill='%231e3a8a'/%3E%3Crect x='9' y='9' width='14' height='14' rx='1.5' fill='%23f8fafc'/%3E%3C/svg%3E">
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
<p class="foot">本文件由 <b>tools/build_stage54_doc.py</b> 数据驱动生成；改内容请改脚本后重跑，
不要手改 HTML。冻结规格的 Markdown 副本见 STAGE-54_V079_IRQ_Waveform_Channels.md。</p>
</div>
</body>
</html>
"""


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT_DEFAULT
    nav_items = "".join(
        f'<a class="nav-l1" href="#{s["id"]}">{s["h"]}</a>' for s in SECTIONS
    )
    nav = (
        '<details class="nav-toggle" open><summary>目录</summary>'
        f'<div class="nav-box">{nav_items}</div></details>'
    )
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
