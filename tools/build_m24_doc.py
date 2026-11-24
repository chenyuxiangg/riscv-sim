#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 M-24（对外操作接口宿主 riscv-sim-server）的 HTML 单文件详细设计。

数据驱动：正文、表格、流程图全部在 `SECTIONS` / `FIG_*` 里，渲染器只管排版
⇒ 改内容不动版式。版式与结构对齐《M-13 RISC-V ISS 模块详细设计》。

产物：docs/detailed-design/M-24_SimApiServer.html
用法：python3 tools/build_m24_doc.py [输出路径]
"""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

OUT_DEFAULT = Path("docs/detailed-design/M-24_SimApiServer.html")

TITLE = "M-24 对外操作接口宿主（riscv-sim-server）模块详细设计"
SUBTITLE = "RISC-V 仿真平台 · 模块详细设计系列 · 服务端形态（P1）落地设计 · 回应《Sim_Agent_API_Requirements v0.1》"
VERSION = ("HTML 版 v1.3（2026-11-24 方案定稿；P0-1～P0-5 已落地；契约 v19 装载回执；run 停因唯一实现 run_until）· 需求基线 docs/requirements/"
           "Sim_Agent_API_Requirements_v0.1.html · 代码基线 sim feature/m24-p0-5（契约 schema v19 / 内核 0.1.0）")

KPI = [
    ("15", "条需求承接", "R-01～R-15 逐条有落点，无悬空条目"),
    ("1", "处内核增量", "装载成功多发一条 ImageLoaded 回执（契约 v19）"),
    ("1", "个新错误族", "契约层新增 SESS 族 7 个场景码"),
    ("1/会话", "专属线程", "KernelHost 为 !Send，靠线程边界隔离"),
]

FIG1 = """
<figure class="fig">
<svg class="flow-svg" viewBox="0 0 1120 500" role="img" aria-label="M-24 分层与线程模型">
  <defs>
    <marker id="ar1" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/></marker>
    <marker id="ar2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#b45309"/></marker>
  </defs>
  <rect x="20" y="60" width="190" height="110" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="34" y="86" font-size="13" font-weight="700" fill="#1f2937">Client</text>
  <text x="34" y="108" font-size="11.5" fill="#4b5563">Agent / CI / curl</text>
  <text x="34" y="128" font-size="11.5" fill="#4b5563">Python / Rust</text>
  <text x="34" y="150" font-size="11.5" fill="#6b7280">不接触 wasm 产物</text>

  <rect x="250" y="30" width="400" height="170" rx="10" fill="#f1f4f9" stroke="#1e3a8a"/>
  <text x="266" y="54" font-size="13" font-weight="700" fill="#1e3a8a">M-24 · HTTP/WS 接入层（tokio 多线程）</text>
  <rect x="266" y="66" width="168" height="46" rx="6" fill="#ffffff" stroke="#d8dee9"/>
  <text x="278" y="84" font-size="11.5" font-family="monospace" fill="#1f2937">router（axum）</text>
  <text x="278" y="102" font-size="11" fill="#4b5563">/api/version · /api/boot</text>
  <rect x="446" y="66" width="184" height="46" rx="6" fill="#ffffff" stroke="#d8dee9"/>
  <text x="458" y="84" font-size="11.5" font-family="monospace" fill="#1f2937">SessionRegistry</text>
  <text x="458" y="102" font-size="11" fill="#4b5563">会话表 · TTL · 配额</text>
  <rect x="266" y="122" width="364" height="62" rx="6" fill="#ffffff" stroke="#d8dee9"/>
  <text x="278" y="142" font-size="11.5" fill="#1f2937">错误映射 error_to_response（码不改写，只补 status）</text>
  <text x="278" y="162" font-size="11.5" fill="#1f2937">路径白名单 · token 校验 · 请求 id</text>

  <rect x="250" y="220" width="400" height="162" rx="10" fill="#f8fafc" stroke="#d8dee9" stroke-dasharray="5 4"/>
  <text x="266" y="244" font-size="12.5" font-weight="700" fill="#374151">会话线程（每会话 1 条 std::thread）</text>
  <rect x="266" y="256" width="172" height="112" rx="8" fill="#ffffff" stroke="#b45309"/>
  <text x="278" y="276" font-size="11.5" font-weight="700" fill="#b45309">Session A</text>
  <text x="278" y="296" font-size="11" fill="#4b5563">KernelHost（!Send）</text>
  <text x="278" y="314" font-size="11" fill="#4b5563">ConsoleRing · 断点表</text>
  <text x="278" y="332" font-size="11" fill="#4b5563">ctrl 原子位</text>
  <text x="278" y="352" font-size="11" fill="#6b7280">命令串行</text>
  <rect x="446" y="256" width="184" height="112" rx="8" fill="#ffffff" stroke="#b45309"/>
  <text x="458" y="276" font-size="11.5" font-weight="700" fill="#b45309">Session B</text>
  <text x="458" y="296" font-size="11" fill="#4b5563">KernelHost（!Send）</text>
  <text x="458" y="314" font-size="11" fill="#4b5563">ConsoleRing · 断点表</text>
  <text x="458" y="332" font-size="11" fill="#4b5563">ctrl 原子位</text>
  <text x="458" y="352" font-size="11" fill="#6b7280">与 A 零共享</text>
  <rect x="250" y="398" width="400" height="80" rx="8" fill="#fffbeb" stroke="#fcd34d"/>
  <text x="266" y="420" font-size="11.5" fill="#92400e">跨会话共享（只读/全局）：板模板库 · 配额 · 日志</text>
  <text x="266" y="440" font-size="11.5" fill="#92400e">错误码表 · 量子常量</text>
  <text x="266" y="462" font-size="11" fill="#6b7280">不得共享：寄存器/内存/console/断点/停顿态</text>

  <rect x="700" y="30" width="390" height="180" rx="10" fill="#ffffff" stroke="#d8dee9"/>
  <text x="716" y="54" font-size="13" font-weight="700" fill="#1f2937">内核（crates/kernel · 不改动）</text>
  <text x="716" y="80" font-size="11.5" font-family="monospace" fill="#1f2937">KernelHost::handle_message(&amp;Message)</text>
  <text x="716" y="100" font-size="11.5" fill="#4b5563">→ Vec&lt;EventPayload&gt;（一进多出）</text>
  <text x="716" y="126" font-size="11.5" fill="#4b5563">M-12 引擎 · M-13 ISS · M-14 总线</text>
  <text x="716" y="146" font-size="11.5" fill="#4b5563">M-08 断点 · M-17 后端 · M-18/M-19</text>
  <text x="716" y="172" font-size="11.5" fill="#6b7280">门禁在内核判，M-24 只透出</text>

  <rect x="700" y="240" width="390" height="120" rx="10" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="716" y="264" font-size="13" font-weight="700" fill="#1f2937">契约层（crates/contract · 只增不改）</text>
  <text x="716" y="288" font-size="11.5" fill="#4b5563">SCHEMA_VERSION = 18（消息形状不变）</text>
  <text x="716" y="308" font-size="11.5" fill="#4b5563">新增 ErrorFamily::Session = "SESS"</text>
  <text x="716" y="328" font-size="11.5" fill="#4b5563">E_SESS-01..07 常量 · 前端镜像表 +2</text>
  <text x="716" y="350" font-size="11" fill="#6b7280">命令/事件/DTO 一个字段都不加</text>

  <line x1="210" y1="115" x2="248" y2="115" stroke="#1e3a8a" stroke-width="1.5" marker-end="url(#ar1)"/>
  <text x="212" y="106" font-size="10.5" fill="#1e3a8a">HTTP/WS</text>
  <line x1="450" y1="200" x2="420" y2="218" stroke="#1e3a8a" stroke-width="1.5" marker-end="url(#ar1)"/>
  <text x="432" y="214" font-size="10.5" fill="#1e3a8a">mpsc</text>
  <line x1="700" y1="120" x2="652" y2="120" stroke="#b45309" stroke-width="1.5" marker-end="url(#ar2)"/>
  <text x="642" y="112" font-size="10.5" fill="#b45309">Message</text>
  <line x1="652" y1="150" x2="698" y2="150" stroke="#b45309" stroke-width="1.5" marker-end="url(#ar2)"/>
  <text x="638" y="166" font-size="10.5" fill="#b45309">事件</text>
</svg>
<div class="figcap">图 1 · 分层与线程模型：HTTP 层只持 Send 句柄，内核状态留在会话线程内</div>
</figure>
"""

FIG2 = """
<figure class="fig">
<svg class="flow-svg" viewBox="0 0 1120 430" role="img" aria-label="run 的分段推进与停因">
  <defs>
    <marker id="ar3" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/></marker>
    <marker id="ar4" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#b45309"/></marker>
  </defs>
  <rect x="40" y="36" width="210" height="58" rx="8" fill="#f1f4f9" stroke="#1e3a8a"/>
  <text x="56" y="60" font-size="12" font-weight="700" fill="#1e3a8a">POST …/run</text>
  <text x="56" y="80" font-size="11" fill="#4b5563">解析 RunSpec · 占位 Running</text>

  <line x1="250" y1="65" x2="296" y2="65" stroke="#1e3a8a" marker-end="url(#ar3)"/>
  <rect x="300" y="36" width="250" height="58" rx="8" fill="#ffffff" stroke="#d8dee9"/>
  <text x="316" y="60" font-size="12" fill="#1f2937">一个量子窗口</text>
  <text x="316" y="80" font-size="11" fill="#6b7280">Continue → 10 000 条指令</text>

  <line x1="550" y1="65" x2="596" y2="65" stroke="#1e3a8a" marker-end="url(#ar3)"/>
  <rect x="600" y="36" width="230" height="58" rx="8" fill="#ffffff" stroke="#d8dee9"/>
  <text x="616" y="60" font-size="12" fill="#1f2937">收事件 · 落 console</text>
  <text x="616" y="80" font-size="11" fill="#6b7280">先 append，再看停因</text>

  <line x1="830" y1="65" x2="876" y2="65" stroke="#1e3a8a" marker-end="url(#ar3)"/>
  <rect x="880" y="36" width="200" height="58" rx="8" fill="#fffbeb" stroke="#fcd34d"/>
  <text x="896" y="60" font-size="12" font-weight="700" fill="#92400e">按固定次序判停因</text>
  <text x="896" y="80" font-size="11" fill="#92400e">表 13 · 六种停因</text>

  <path d="M980 94 L980 150 L425 150 L425 94" stroke="#b45309" stroke-width="1.5" fill="none" marker-end="url(#ar4)"/>
  <text x="640" y="142" font-size="11" fill="#b45309">不停 ⇒ 继续下一个量子</text>

  <rect x="600" y="180" width="230" height="80" rx="8" fill="#ffffff" stroke="#1e3a8a"/>
  <text x="616" y="204" font-size="12" font-weight="700" fill="#1e3a8a">停：置 Idle</text>
  <text x="616" y="224" font-size="11" fill="#4b5563">flush console → 组 StopReport</text>
  <text x="616" y="244" font-size="11" fill="#6b7280">console 先于响应落缓冲</text>

  <line x1="715" y1="260" x2="715" y2="300" stroke="#1e3a8a" marker-end="url(#ar3)"/>
  <rect x="560" y="304" width="310" height="86" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="576" y="326" font-size="12" fill="#1f2937">响应 {stop_reason, executed, pc, t_ps}</text>
  <text x="576" y="346" font-size="11" fill="#4b5563">timeout ⇒ 504 + stop_reason=timeout</text>
  <text x="576" y="366" font-size="11" fill="#6b7280">其余 200；meta 回显量子与耗时</text>

  <rect x="40" y="180" width="330" height="210" rx="8" fill="#f8fafc" stroke="#d8dee9"/>
  <text x="56" y="204" font-size="12" font-weight="700" fill="#374151">判停因的四个数据来源</text>
  <text x="56" y="230" font-size="11.5" fill="#4b5563">① 内核事件：Halted{reason}</text>
  <text x="56" y="252" font-size="11.5" fill="#4b5563">② ctrl 原子位：pause / cancel</text>
  <text x="56" y="274" font-size="11.5" fill="#4b5563">③ 墙钟 deadline（每窗口查一次）</text>
  <text x="56" y="296" font-size="11.5" fill="#4b5563">④ console 前缀 + 指令计数</text>
  <text x="56" y="322" font-size="11" fill="#6b7280">四个来源都不改内核语义</text>
  <text x="56" y="344" font-size="11" fill="#6b7280">都只读宿主可见量</text>
</svg>
<div class="figcap">图 2 · run 的分段推进：一个量子一次 Continue，停因在第 5.2 节的固定次序上判定</div>
</figure>
"""

FIG3 = """
<figure class="fig">
<svg class="flow-svg" viewBox="0 0 1120 360" role="img" aria-label="会话建立与装载时序">
  <defs>
    <marker id="ar5" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
      <path d="M0,0 L10,5 L0,10 z" fill="#1e3a8a"/></marker>
  </defs>
  <text x="40" y="30" font-size="12" fill="#4b5563" font-family="monospace">Client</text>
  <text x="470" y="30" font-size="12" fill="#4b5563" font-family="monospace">M-24 HTTP 层</text>
  <text x="780" y="30" font-size="12" fill="#4b5563" font-family="monospace">会话线程 / 内核</text>
  <line x1="40" y1="42" x2="1080" y2="42" stroke="#d8dee9"/>

  <line x1="140" y1="70" x2="500" y2="70" stroke="#1e3a8a" marker-end="url(#ar5)"/>
  <text x="146" y="64" font-size="11" fill="#1f2937" font-family="monospace">POST /api/sessions</text>
  <line x1="500" y1="86" x2="140" y2="86" stroke="#9aa1af" stroke-dasharray="4 3"/>
  <text x="150" y="102" font-size="10.5" fill="#4b5563" font-family="monospace">201 {session_id}</text>

  <line x1="140" y1="130" x2="500" y2="130" stroke="#1e3a8a" marker-end="url(#ar5)"/>
  <text x="146" y="124" font-size="11" fill="#1f2937" font-family="monospace">PUT …/board {template}</text>
  <line x1="500" y1="130" x2="800" y2="126" stroke="#b45309" marker-end="url(#ar5)"/>
  <text x="540" y="120" font-size="10.5" fill="#b45309" font-family="monospace">GetPlatformTemplate → SetMemoryMap</text>
  <line x1="500" y1="148" x2="140" y2="148" stroke="#9aa1af" stroke-dasharray="4 3"/>
  <text x="150" y="164" font-size="10.5" fill="#4b5563" font-family="monospace">{revision}（或 409/404）</text>

  <line x1="140" y1="196" x2="500" y2="196" stroke="#1e3a8a" marker-end="url(#ar5)"/>
  <text x="146" y="190" font-size="11" fill="#1f2937" font-family="monospace">POST …/image (octet-stream)</text>
  <line x1="500" y1="196" x2="800" y2="192" stroke="#b45309" marker-end="url(#ar5)"/>
  <text x="540" y="186" font-size="10.5" fill="#b45309" font-family="monospace">LoadProject(Project{images})</text>
  <line x1="500" y1="214" x2="140" y2="214" stroke="#9aa1af" stroke-dasharray="4 3"/>
  <text x="150" y="230" font-size="10.5" fill="#4b5563" font-family="monospace">{entry,segments,isa} / 422</text>

  <line x1="140" y1="262" x2="500" y2="262" stroke="#1e3a8a" marker-end="url(#ar5)"/>
  <text x="146" y="256" font-size="11" fill="#1f2937" font-family="monospace">POST …/run {prefix,timeout_ms}</text>
  <line x1="500" y1="262" x2="800" y2="258" stroke="#b45309" marker-end="url(#ar5)"/>
  <text x="540" y="252" font-size="10.5" fill="#b45309" font-family="monospace">Continue × N（分段）</text>
  <line x1="500" y1="282" x2="140" y2="282" stroke="#9aa1af" stroke-dasharray="4 3"/>
  <text x="150" y="298" font-size="10.5" fill="#4b5563" font-family="monospace">{stop_reason,executed,pc}</text>

  <line x1="140" y1="322" x2="1080" y2="322" stroke="#d8dee9"/>
  <text x="48" y="322" font-size="10.5" fill="#6b7280" font-family="monospace">GET …/console?offset=0</text>
</svg>
<div class="figcap">图 3 · 会话建立 → 配板 → 装载 → 运行 → 取 console（会话内命令严格串行）</div>
</figure>
"""

SECTIONS = []

# ---------------------------------------------------------------- 1
SECTIONS.append({
    "id": "sec1",
    "h": "1 功能概述",
    "subs": [("sec1-1", "1.1 模块定位"), ("sec1-2", "1.2 需求承接与约束"),
             ("sec1-3", "1.3 范围、优先级与不做")],
    "blocks": [
        ("h3", "sec1-1", "1.1 模块定位"),
        ("p", "M-24 是 riscv_sim 的<b>进程级对外操作入口</b>：把内核已有的能力（装载 / 运行 / 步进 / 断点 / 寄存器 / 内存 / 反汇编 / console 字节流）以稳定的 HTTP + WebSocket 形态暴露，"
              "使 Agent / CI 不必 <code>import front/assets/pkg/riscv_sim_kernel.js</code>，也不必知道 <code>front/</code> 的目录结构。"),
        ("p", "它<b>不是第 24 个内核模块</b>：M-13 仍然解释指令，M-12 仍然持有虚拟时钟，M-14 仍然管理映射，M-08 仍然持有断点表。M-24 只做四件事 —— "
              "<b>传输（HTTP/WS）</b>、<b>会话（表 + 生命周期 + 隔离）</b>、<b>缓冲（console 的绝对 offset）</b>、<b>映射（内核码 ↔ HTTP 状态）</b>。"
              "浏览器形态里住在 M-04/M-05/M-07 的三块职责，在服务端形态里由 M-24 承担；内核与契约层不动。"),
        ("p", "三条设计红线（与需求的「非目标」逐条对齐）："),
        ("list", [
            "<b>不替代浏览器工作台</b>：前端仍是主交互面，M-24 是第二个入口，两者共用同一份内核与契约。",
            "<b>不在 API 层改内核语义</b>：无板不装载、镜像落在板外即拒、ISA 门禁不过即拒 —— 一律原样透出，不做「贴心兜底」。",
            "<b>不给同一个故障造第二个码</b>：错误码只在契约层拥有，M-24 只补 HTTP 状态与 detail。",
        ]),
        ("h3", "sec1-2", "1.2 需求承接与约束"),
        ("p", "需求文档 15 条全部有落点，没有悬空条目。下表是「需求 → 设计落点」的对照，也是后续评审的检查清单。"),
        ("caption", "表 1 · 需求承接矩阵（R-01～R-15）"),
        ("table", {
            "widths": ["9%", "24%", "47%", "10%", "10%"],
            "head": ["编号", "需求要点", "本设计的落点", "设计章节", "优先级"],
            "rows": [
                ["R-01", "进程级入口与版本协商", "<code>riscv-sim-server</code> 二进制；<code>GET /api/version</code> 返回 kernel / schema_version / contract_version；启动打印实际绑定地址", "§3.1 §5.1", "P0"],
                ["R-02", "会话生命周期与隔离", "会话表 + 每会话专属线程 + TTL 回收；命令串行、跨会话并行", "§2.2 §5.5", "P0"],
                ["R-03", "板级配置（无板不装载）", "<code>PUT …/board</code> 三来源；无板装载原样回 <code>E_PLT-22</code>；模板只读端点", "§3.3 §5.1", "P0"],
                ["R-04", "镜像装载", "<code>POST …/image</code> 支持 octet-stream 与本地路径；<b>魔数优先</b>判 kind；IHEX 明确拒", "§3.3", "P0"],
                ["R-05", "运行与停止（四停因）", "服务端用 <code>Continue</code> 分片推进；停因含 budget / prefix / breakpoint / halt / timeout / paused", "§5.2", "P0"],
                ["R-06", "console 增量、单调、原样", "<code>ConsoleRing</code> 绝对 offset 只追加；溢出显式报丢弃区间；WS 与拉取共用同一份 offset", "§5.3", "P0"],
                ["R-07", "状态 / 内存 / 符号 / 反汇编", "读 <code>ReadRegisters</code> / <code>DumpMemory</code> / <code>ResolveSymbol</code> / <code>Disasm</code>；<code>pc</code> 与 <code>gpr[32]</code> 分开", "§3.1", "P0"],
                ["R-08", "断点与观察点", "断点表以内核为准（<code>ListBreakpoints</code>），命中随 run 的停因一起返回", "§3.1", "P0"],
                ["R-09", "错误契约（码 + detail）", "统一错误体；12 个码固定 HTTP 映射；不新造同义词", "§3.4", "P0"],
                ["R-10", "确定性：同输入同输出", "预算按指令条数；量子常量随响应回显；不许随机初值、不许墙钟参与结果", "§5.2", "P0"],
                ["R-11", "超时与熔断", "每窗口查 deadline；到点停下并回 504；会话仍可 <code>reset</code> 后再用", "§5.4", "P0"],
                ["R-12", "CI / 脚本友好", "<code>POST /api/boot</code> 为<b>对外</b>判定口径；<code>osz-boot</code> 保留为<b>离线</b> runner（无 daemon 依赖），两条入口共用内核同一份 <code>run_window</code>", "§3.2 §5.2 附录 A", "P0"],
                ["R-13", "事件流（WS）", "复用 M-07 的 <code>{type,seq,sessionId,role,reqId,payload}</code> 帧；背压显式报丢弃区间", "§3.6", "P1"],
                ["R-14", "快照 / 复位 / trace", "复用 <code>Snapshot</code>/<code>Restore</code>/<code>SetTraceEnabled</code>/<code>ExportTrace</code>；复位的实现是重发 <code>LoadProject</code>", "§5.5", "P1"],
                ["R-15", "安全边界", "默认只绑回环；镜像路径白名单 + 规范化；非回环需显式开关与 token", "§3.4 §5.4", "P0"],
            ],
        }),
        ("callout", "约束", "R-03 的「不得回退到 v0.76 之前的自动选档」是一条<b>结构约束</b>：M-24 的配板动作必须是客户端可见的一次命令，"
                  "服务启动参数 <code>--board</code> 也只能在<b>建会话时显式执行一次</code>，并在会话元数据里记下 <code>board_source=startup-default</code>，"
                  "使「这块板从哪来」可追溯 —— 而不是在装载路径里偷偷补一块。"),
        ("h3", "sec1-3", "1.3 范围、优先级与不做"),
        ("caption", "表 2 · 范围与优先级"),
        ("table", {
            "widths": ["10%", "62%", "28%"],
            "head": ["档位", "内容", "验收口径"],
            "rows": [
                ["P0（必需）", "进程入口 / 会话 / 配板 / 装载 / 运行 / console / 状态内存符号 / 断点 / 错误契约 / 确定性 / 超时 / <code>/api/boot</code>", "AC-01～AC-10 全绿"],
                ["P1（调试完整体验）", "WS 事件流 / 快照与恢复 / trace 分页 / <code>async:true</code> 异步 run", "前端 server-mode 可用；快照回放一致"],
                ["P2（后续）", "远程鉴权体系 / 进程级硬隔离 / 多租户配额与计量", "不进入本批排期"],
                ["明确不做", "替代浏览器工作台 · 支持 IHEX · 同一会话多板并行 · API 层自动选板或改地址 · 自动重试装载", "评审逐条确认"],
            ],
        }),
    ],
})

# ---------------------------------------------------------------- 2
SECTIONS.append({
    "id": "sec2",
    "h": "2 数据描述",
    "subs": [("sec2-1", "2.1 核心数据结构"), ("sec2-2", "2.2 会话状态机"),
             ("sec2-3", "2.3 数据流与缓冲")],
    "blocks": [
        ("h3", "sec2-1", "2.1 核心数据结构"),
        ("p", "七个结构，其中只有两个跨线程：<code>SessionHandle</code>（Send，HTTP 层持有）与 <code>KernelHost</code>（!Send，会话线程持有）。"
              "其余结构都住在会话线程里或 HTTP 层的请求上下文中，不共享可变状态。"),
        ("caption", "表 3 · 核心数据结构（字段级）"),
        ("table", {
            "widths": ["17%", "20%", "18%", "45%"],
            "head": ["结构", "字段", "类型", "语义与约束"],
            "rows": [
                ["<b>Config</b>", "bind / port", "IpAddr / u16", "默认 <code>127.0.0.1:9876</code>；<code>--port 0</code> 由内核分配并打印实际值"],
                ["", "board", "Option&lt;String&gt;", "模板 id；<b>只在建会话时显式配一次</b>，不做装载期兜底"],
                ["", "session_ttl", "Duration", "空闲回收阈值，默认 600 s；扫表周期 10 s"],
                ["", "max_sessions", "usize", "默认 8；超限 <code>429</code> 并回剩余额度"],
                ["", "max_console", "usize", "默认 1 MiB；溢出<b>显式</b>报丢弃区间，不静默丢"],
                ["", "allow_image_root", "Vec&lt;PathBuf&gt;", "镜像路径白名单；仓库根始终在列，其余显式追加"],
                ["", "allow_remote / token", "bool / Option&lt;String&gt;", "非回环绑定必须两者同时具备，否则启动失败"],
                ["", "quantum", "u32", "== <code>DEFAULT_INSTRUCTION_BUDGET</code>（10 000）；运行分片粒度，随响应回显"],
                ["<b>SessionHandle</b>", "id", "SessionId（hex16）", "随机生成、不可猜测；不复用"],
                ["", "tx", "mpsc::Sender&lt;Job&gt;", "命令面入口；<b>只投递，不等待</b>"],
                ["", "ctrl", "Arc&lt;AtomicU8&gt;", "控制面：<code>Run/Pause/Cancel</code>；立即生效，不排队"],
                ["", "state", "AtomicU8", "<code>New/Boarded/Loaded/Idle/Running/Poisoned/Closed</code>"],
                ["", "created_at / last_seen", "Instant", "TTL 与 <code>GET /api/sessions</code> 的来源"],
                ["<b>ConsoleRing</b>", "base / total", "u64 / u64", "<code>base</code> = 缓冲里最旧字节的<b>绝对 offset</b>；<code>total</code> = 已产生字节总数"],
                ["", "buf / max / dropped", "Vec&lt;u8&gt; / usize / Vec&lt;(u64,u64)&gt;", "回收从尾部裁；每次裁剪记一段丢弃区间，供响应头回放"],
                ["<b>RunSpec</b>", "steps / prefix / timeout / until / chunk", "Option&lt;u64&gt; / Option&lt;Vec&lt;u8&gt;&gt; / Option&lt;Duration&gt; / Until / u32", "四者至少一项；<code>until</code> ∈ {any, breakpoint, halt}；<code>chunk</code> 缺省取 <code>quantum</code>"],
                ["<b>StopReport</b>", "reason / executed / pc / t_ps / console / hit / meta", "&amp;'static str / u64 / u32 / String / Range / Option&lt;Hit&gt; / Meta", "<code>t_ps</code> 按契约数值策略走<b>十进制字符串</b>；<code>meta</code> 回显量子、窗口数、overshoot 上限"],
                ["<b>ErrorBody</b>", "code / detail / status / request_id", "String / String / u16 / String", "<code>code</code> 必须来自契约层；<code>detail</code> 非空（禁 500 空错误）"],
                ["<b>ImageReport</b>", "kind / entry / segments / total_bytes / isa / sha256", "String / u32 / Vec&lt;(u32,u32)&gt; / u64 / String / String", "<code>segments</code> 用 JSON 数字，<code>sha256</code> 供 CI 固定夹具"],
            ],
        }),
        ("h3", "sec2-2", "2.2 会话状态机"),
        ("p", "状态只由会话线程写（HTTP 层读），因此「谁改状态」只有一个落点。非法迁移一律在<b>动手之前</b>拒绝，机器状态不动 —— 与内核既有的次序纪律同款。"),
        ("caption", "表 4 · 状态迁移与拒绝码"),
        ("table", {
            "widths": ["13%", "26%", "13%", "48%"],
            "head": ["现态", "事件", "次态", "判据 / 拒绝"],
            "rows": [
                ["New", "<code>PUT board</code>", "Boarded", "模板存在 ⇒ 200；未知模板 ⇒ <code>404 + E_PLT-22</code>"],
                ["Boarded", "<code>POST image</code>", "Loaded", "镜像合法且不越板 ⇒ 200；越板 ⇒ <code>422 + E_PLT-17</code>"],
                ["New", "<code>POST image</code>", "—", "无板 ⇒ <code>409 + E_PLT-22</code>，detail 保留「先从模板导入 default / minimal / qemu-virt」"],
                ["Loaded", "<code>PUT board</code>", "Loaded", "允许换板，但<b>不自动重载</b>；返回 <code>pending:true</code>，需再发一次 image 才生效"],
                ["Loaded / Idle", "<code>run</code> / <code>step</code>", "Running", "占用命令面；同一时刻只允许一个推进者"],
                ["Running", "<code>run</code> / <code>step</code>", "—", "忙 ⇒ <code>409 + E_SESS-02</code>，<b>不排队</b>、不交错执行"],
                ["Running", "停因成立", "Idle", "见 §5.2 表 13；停后仍可再 run"],
                ["Running", "<code>POST pause</code>", "Idle", "置 ctrl 位；推进者在<b>下一个量子边界</b>停（≤ 1 个量子）"],
                ["任意", "<code>DELETE</code> / TTL 到期", "Closed", "释放线程与缓冲；之后任何访问 ⇒ <code>404 + E_SESS-01</code>"],
                ["任意", "会话线程 panic", "Poisoned", "捕获后标记中毒；后续命令 ⇒ <code>500 + E_SESS-03</code>（带 detail），并立即可回收"],
                ["Closed", "任意访问", "—", "一律 <code>404 + E_SESS-01</code>，不复用 id"],
            ],
        }),
        ("h3", "sec2-3", "2.3 数据流与缓冲"),
        ("p", "一条命令的完整路径：HTTP handler 组 <code>Message</code> → <code>mpsc</code> 投递给会话线程 → <code>KernelHost::handle_message</code> 执行 → "
              "事件分批返回 → <b>先</b>把 <code>Console</code> 字节 append 进 ring、<b>再</b>按停因组响应。"
              "「先日记账、后作结论」是 R-06 与 R-05 一致性的关键：响应里说的字节数必须此刻已经可读。"),
        ("svg", "FIG1"),
        ("caption", "表 5 · 缓冲与通道"),
        ("table", {
            "widths": ["16%", "16%", "22%", "20%", "26%"],
            "head": ["对象", "持有者", "生命周期", "上界", "溢出 / 满时行为"],
            "rows": [
                ["ConsoleRing", "会话线程", "会话全程", "<code>--max-console</code>", "从尾部裁，记丢弃区间；读时回 <code>409 + E_SESS-04</code>"],
                ["命令面队列", "会话线程", "会话全程", "深度 1（串行）", "已有推进者 ⇒ 立即 409，不入队"],
                ["ctrl 原子位", "HTTP 层写 / 线程读", "会话全程", "1 字节", "只置位不清零；新 run 开始时复位为 Run"],
                ["配额信号量", "进程全局", "进程全程", "<code>--max-sessions</code>", "无可用额度 ⇒ 429 + 剩余额度，不排队"],
                ["WS 订阅表", "会话线程", "连接期", "每连接 1 份", "客户端跟不上 ⇒ 丢帧并显式上报 seq 区间"],
            ],
        }),
    ],
})

# ---------------------------------------------------------------- 3
SECTIONS.append({
    "id": "sec3",
    "h": "3 接口设计",
    "subs": [("sec3-1", "3.1 端点总表"), ("sec3-2", "3.2 运行与一把梭的字段"),
             ("sec3-3", "3.3 镜像与配板的字段"), ("sec3-4", "3.4 错误体与状态映射"),
             ("sec3-5", "3.5 契约增量（只增不改）"), ("sec3-6", "3.6 事件流与 M-07 帧")],
    "blocks": [
        ("h3", "sec3-1", "3.1 端点总表"),
        ("p", "路径一律 <code>/api/sessions/{id}/…</code>；下表把需求文档 §3.1 的 15 条建议形态落到可实现的字段与返回码上。"),
        ("caption", "表 6 · 端点清单"),
        ("table", {
            "widths": ["29%", "18%", "38%", "6%", "9%"],
            "head": ["方法 / 路径", "作用", "关键载荷 → 返回", "成功码", "优先级"],
            "rows": [
                ["<code>GET /api/version</code>", "版本与 schema 协商", "→ <code>{kernel, schema_version, contract_version}</code>", "200", "P0"],
                ["<code>GET /healthz</code>", "就绪探针", "→ <code>{ok, sessions, uptime_ms}</code>", "200/503", "P0"],
                ["<code>POST /api/sessions</code>", "新建会话", "<code>{board?}</code> → <code>{session_id, board?, schema_version}</code>", "201", "P0"],
                ["<code>GET /api/sessions</code>", "列出会话", "→ 数组（含 board / image / state / last_seen）", "200", "P0"],
                ["<code>DELETE /api/sessions/{id}</code>", "回收", "→ 无体", "204", "P0"],
                ["<code>PUT …/board</code>", "配板", "<code>{template|description, expected_revision?}</code> → <code>{revision, board_id, regions}</code>", "200", "P0"],
                ["<code>GET /api/board/templates</code>", "模板清单", "→ 只读元数据（id / name / priority / device_count）", "200", "P0"],
                ["<code>GET /api/board/templates/{id}</code>", "模板详情", "→ 完整 <code>PlatformDescription</code>", "200/404", "P0"],
                ["<code>POST …/image</code>", "装载 ELF/BIN", "<code>octet-stream</code> 或 <code>{path,kind}</code> → <code>{entry, segments, isa, sha256}</code>", "200", "P0"],
                ["<code>POST …/run</code>", "跑到停点", "<code>{steps|prefix, timeout_ms, until}</code> → <code>{stop_reason, executed, pc, console}</code>", "200/504", "P0"],
                ["<code>POST …/step</code>", "精确 n 条", "<code>{n}</code> → <code>{pc, executed}</code>", "200", "P0"],
                ["<code>POST …/pause</code> · <code>…/reset</code>", "暂停 / 复位", "→ <code>{state}</code>；复位 = 重发 <code>LoadProject(last_project)</code>", "200/202", "P0"],
                ["<code>GET …/console</code>", "取 console 字节", "<code>?offset&limit</code> → 原始字节 + <code>X-Console-Total</code>", "200", "P0"],
                ["<code>GET …/state</code> · <code>…/memory</code> · <code>…/symbols</code> · <code>…/disasm</code>", "调试读取面", "见 §3.1 说明", "200", "P0"],
                ["<code>/…/breakpoints</code>", "断点增删查", "<code>POST {addr|symbol}</code> · <code>DELETE /{id}</code> · <code>GET</code> + 启停开关", "200/201", "P0"],
                ["<code>POST /api/boot</code>", "一把梭（唯一判定口径）", "<code>{path, prefix, steps, timeout_ms, write_to?}</code> → console 原始字节 + meta 头", "200/504", "P0"],
                ["<code>GET …/events</code>（升级 WS）", "事件流", "M-07 帧原样转发", "101", "P1"],
                ["<code>…/snapshot</code> · <code>…/restore</code> · <code>…/trace</code>", "闭环调试", "复用既有命令", "200", "P1"],
            ],
        }),
        ("note", "<b>读接口的语义要点：</b><code>state</code> 返回 <code>{pc, gpr[32], t_ps}</code> —— <b>pc 单独一个字段</b>，不在 <code>gpr</code> 里，且 <code>gpr[0]</code> 恒 0（需求里踩过的坑就在这）；"
              "<code>memory</code> 的 <code>len ≤ 4096</code>（契约 <code>DUMP_MAX_BYTES</code>），越界原样拒；<code>symbols</code> 精确命中与候选并存（候选上限 16）；"
              "<code>disasm</code> 允许短于 <code>count</code>（未映射即止，与内核一致）。"),
        ("h3", "sec3-2", "3.2 运行与一把梭的字段"),
        ("caption", "表 7 · <code>run</code> 与 <code>/api/boot</code> 的请求 / 响应字段"),
        ("table", {
            "widths": ["12%", "16%", "11%", "9%", "52%"],
            "head": ["端点", "字段", "类型", "必填", "语义 / 默认"],
            "rows": [
                ["run", "steps", "u64", "否", "指令预算（按<b>条数</b>，不按墙钟）；与 <code>prefix</code> 至少给一个"],
                ["run", "prefix", "string(UTF-8)", "否", "console 字节流里出现该串即停；按<b>字节</b>匹配，不做转义"],
                ["run", "timeout_ms", "u64", "否", "墙钟熔断，默认 <code>--default-timeout</code>（30 000）；只作为熔断，不参与结果"],
                ["run", "until", "enum", "否", "<code>any</code>（默认）/ <code>breakpoint</code> / <code>halt</code>"],
                ["run", "async", "bool", "否", "P0 只接受 <code>false</code>；<code>true</code> ⇒ <code>422 + E_PROTO-08</code>（P1 再开）"],
                ["run", "stop_reason", "enum", "—", "<code>budget / prefix / breakpoint / halt / timeout / paused</code>"],
                ["run", "executed", "u64", "—", "本次请求执行的条数 = 前窗口差；量子对齐，<code>meta.quantum</code> 回显"],
                ["run", "console", "{from,to,total}", "—", "本次新增字节的绝对区间；断言「to-from == 实际字节数」"],
                ["run", "meta", "object", "—", "<code>{quantum, windows, overshoot_max, wall_ms, board_revision}</code>"],
                ["boot", "path", "string", "二选一", "服务端路径；受 <code>--allow-image-root</code> 约束"],
                ["boot", "image", "octet-stream", "二选一", "请求体直传（免 200 KB 级 JSON 数字数组）"],
                ["boot", "prefix / steps / timeout_ms", "同上", "否", "缺省 <code>prefix</code> 为 <code>OSZ$&nbsp;</code>"],
                ["boot", "write_to", "string", "否", "显式落盘；不在 <code>--allow-out-root</code> 内 ⇒ <code>403</code>"],
                ["boot", "响应体", "application/octet-stream", "—", "<b>只放 console 字节</b>；统计信息走 <code>X-Sim-Meta</code> 头（ASCII JSON）"],
            ],
        }),
        ("callout", "为什么 <code>boot</code> 的统计走头不走体", "需求 R-12 要求「返回体只放 console 字节」，这样与 Spike 的 <code>cmp</code> 不必再写 sed 剥前缀。"
                  "把 <code>stop_reason / executed / pc</code> 放进响应头，既保住字节流纯净，又让脚本一眼拿到判决信息。"),
        ("h3", "sec3-3", "3.3 镜像与配板的字段"),
        ("caption", "表 8 · <code>image</code> 与 <code>board</code> 的字段与判定"),
        ("table", {
            "widths": ["13%", "20%", "14%", "53%"],
            "head": ["端点", "字段 / 输入", "类型", "判定与错误"],
            "rows": [
                ["board", "template", "string", "内置模板 id；不存在 ⇒ <code>404 + E_PLT-22</code>（与「无板」同码同口径）"],
                ["board", "description", "<code>PlatformDescription</code>", "整份替换（不是 patch）；非法描述 ⇒ <code>422 + E_PLT-0x</code>"],
                ["board", "expected_revision", "u64?", "给了且与当前不符 ⇒ <code>409 + E_PLT-13</code>，什么都不动"],
                ["board", "响应 revision / pending", "u64 / bool", "修订号单调；<code>pending</code> 表示已配但尚未重载生效"],
                ["image", "请求体", "octet-stream", "≤ <code>--max-image-bytes</code>（默认 16 MiB 对齐 M-06 限值）"],
                ["image", "path", "string", "<b>规范化后</b>必须落在白名单目录内；否则 <code>403 + E_STORE-07</code>；<code>..</code> 穿越一律拒"],
                ["image", "kind", "enum?", "<code>elf</code> / <code>bin</code>；省略则<b>魔数优先</b>、扩展名兜底（与 M-06 <code>detect_kind_with_hint</code> 同口径）"],
                ["image", ".hex / .ihex", "—", "<code>400 + E_PROTO-08</code>，detail「IHEX 在产品入口即被拒」；不静默降级为 raw"],
                ["image", "返回 entry / segments", "u32 / Vec&lt;(u32,u32)&gt;", "与 <code>readelf -l</code> 一致；<code>entry</code> 是 ELF <code>e_entry</code>，raw 为 0"],
                ["image", "返回 isa", "string", "供与板 <code>mcu.isa</code> 门禁对照；门禁不过由内核回 <code>E_PLT-20</code> ⇒ 422"],
            ],
        }),
        ("h3", "sec3-4", "3.4 错误体与状态映射"),
        ("p", "统一错误体 <code>{code, detail, status, request_id}</code>；<code>status</code> 同时出现在 HTTP 状态行与体内，便于只看体的客户端判错。"
              "<b>码一律使用契约层既有的族</b>：新增场景码只有两处 —— SESS 族进契约层（见 §3.5），<code>E_STORE-07</code> 住本 crate。"),
        ("caption", "表 9 · 错误码与 HTTP 状态映射（含新增会话族）"),
        ("table", {
            "widths": ["8%", "20%", "38%", "34%"],
            "head": ["HTTP", "code", "触发场景", "处置 / detail 要点"],
            "rows": [
                ["400", "<code>E_PROTO-08</code>", "载荷形状不对、未知字段、<code>async:true</code>、IHEX", "指出具体字段；不回显整份载荷"],
                ["401", "<code>E_SESS-07</code>", "远程模式缺 / 错 <code>X-Auth-Token</code>", "只说「需要 X-Auth-Token」，不回显期望值"],
                ["403", "<code>E_STORE-07</code>", "镜像路径越出白名单、<code>write_to</code> 越界", "只报「不在允许目录内」，不回显真实路径"],
                ["404", "<code>E_SESS-01</code>", "会话不存在 / 已回收 / TTL 到期", "含 <code>session_id</code> 与建议「重新 POST /api/sessions」"],
                ["404", "<code>E_PLT-22</code>", "未知板模板 id", "保留「先从模板导入 default / minimal / qemu-virt」"],
                ["409", "<code>E_PLT-22</code>", "无板直接装载", "原样透出内核中文引导，不改写"],
                ["409", "<code>E_PLT-13</code>", "修订号不符", "回当前 <code>revision</code>，让客户端重取"],
                ["409", "<code>E_SESS-02</code>", "会话正忙（已有推进者）", "回当前状态与建议 <code>pause</code> 或等待"],
                ["409", "<code>E_SESS-04</code>", "请求的 console offset 早于缓冲窗口", "回 <code>base</code> 与被丢弃区间"],
                ["413", "<code>E_PROTO-08</code>", "请求体超限（<code>tower_http::limit</code>，默认 32 MiB）", "不给解析器制造 OOM 的机会"],
                ["422", "<code>E_PLT-17</code>", "镜像落在板外", "detail 保留「第一个落不在的区间」"],
                ["422", "<code>E_LDR-01/02/03</code>", "ELF 畸形 / 超限 / 段溢出", "原样透出解析器定位信息"],
                ["422", "<code>E_PLT-20</code>", "ISA 门禁不过（RV64 vs RV32、板不含 c）", "点明是位宽还是压缩扩展"],
                ["429", "<code>E_SESS-05</code>", "并发会话或额度超限", "回剩余额度与建议回收哪一侧"],
                ["504", "<code>E_SESS-06</code>", "墙钟超时（guest 已停下）", "体里同时给 <code>stop_reason=timeout</code> 与已执行条数"],
                ["500", "<code>E_SESS-03</code> / 其他", "会话线程 panic、内部不变量破坏", "<b>必须</b>带 detail；禁止空错误；可立即回收"],
            ],
        }),
        ("h3", "sec3-5", "3.5 契约增量（只增不改）"),
        ("p", "全量核对过契约 v18：装载 / 运行 / 步进 / 断点 / 观察点 / 寄存器 / 内存转储 / 符号 / 反汇编 / 快照 / trace / 模板 / 板图 —— "
              "<b>本需求需要的命令一条不缺</b>。因此契约层<b>不动任何命令</b>：只加一个错误族，外加一个事件变体（装载回执）。"
              "回执是<b>用户拍板</b>加的：没有它，客户端只能从 <code>MemoryMap.loaded</code> 这个<b>状态位</code>推断「装载发生过」，"
              "而段/入口/ISA 事实要么再查一次、要么在宿主侧重解析一遍 —— <b>报告与事实就有两个来源</b>。"),
        ("caption", "表 10 · 契约层与镜像代码的增量清单"),
        ("table", {
            "widths": ["22%", "16%", "12%", "34%", "16%"],
            "head": ["变更项", "位置", "改动量", "理由（判据 D-132 · C）", "版本影响"],
            "rows": [
                ["新增 <code>ErrorFamily::Session = \"SESS\"</code>", "contract/error.rs 的 <code>error_families!</code>", "1 行", "会话不存在 / 忙 / 中毒 / 窗口丢失 / 额度 / 超时 —— 这些码<b>前端 server-mode 要按码分派</b>（判据②成立）", "族清单 +1；<code>SCHEMA_VERSION</code> 随之 18 → 19（与下一行同批）"],
                ["新增 <code>EventPayload::ImageLoaded</code>", "contract/protocol.rs 的 <code>EventPayload</code>", "1 变体（7 字段）", "R-04 要在装载回执里给 entry / segments / isa —— 由<b>内核</b>在装载路径末尾发（与真正 <code>load_bytes</code> 的那份解析同源）⇒ 报告即事实", "线上<b>多一个事件变体</code> ⇒ <code>SCHEMA_VERSION</code> 18 → 19"],
                ["新增 <code>model::ImageSegment</code>", "contract/model.rs", "1 DTO（base / size）", "只报段<b>形状</b>，不重复传字节（字节仍是 <code>MemoryDump</code> 的事）", "随 v19"],
                ["<code>E_SESS-01</code> 会话不存在", "同上", "1 常量", "R-02 / R-09 明确要求", "前端 <code>RUST_ERROR_CODE_REFS</code> +1"],
                ["<code>E_SESS-02</code> 会话忙", "同上", "1 常量", "「忙」必须与「不存在」分开，否则客户端无法决定重试还是重建", "+1"],
                ["<code>E_SESS-03</code> 会话中毒", "同上", "1 常量", "线程 panic 后不可继续，与业务拒绝不同类", "+1"],
                ["<code>E_SESS-04</code> console 窗口丢失", "同上", "1 常量", "偏移早于缓冲窗口时，客户端必须重新对齐", "+1"],
                ["<code>E_SESS-05 / 06</code> 额度 / 超时", "同上（P0 内实现）", "2 常量", "R-11 的 429 与 504 需要可判别的码", "+2"],
                ["<code>E_SESS-07</code> 未授权", "同上", "1 常量", "<code>--allow-remote</code> 下缺 token（P2 前的最小闸门）", "+1"],
                ["<code>E_STORE-07</code> 路径越出白名单", "<b>server crate</b> 内部常量", "1 常量", "不跨 crate 共享、前端不分派 ⇒ 按判据 C <b>住模块内</b>", "契约不变"],
                ["<code>KernelHost::on_load_project</code> 末尾", "kernel/kernel_host/project.rs", "1 段（发回执）", "回执必须在<b>全部步骤成功之后</b>发；段与第五步 <code>load_bytes</code> 用<b>同一份</code> <code>parsed</code>", "内核语义不变：多一条事件，不改既有行为"],
                ["<code>platform::template_infos()</code>", "kernel/kernel_host/platform.rs", "1 公开纯函数", "模板清单元数据（<code>device_count</code> 口径）只留<b>一个定义点</b>：内核事件与 <code>GET /api/board/templates</code> 共用", "无"],
            ],
        }),
        ("callout", "一处与需求原文的偏差", "需求 R-15 写的 <code>403 + E_IO-xx</code> 在契约层没有对应族（现只有 <code>STORE</code> 承担持久化/IO）。"
                  "本设计把它落到 <code>E_STORE-07</code>：<b>不新造 IO 族</b>，与「同一个故障不出现两个码」同一条纪律。评审时可确认是否接受。"),
        ("h3", "sec3-6", "3.6 事件流与 M-07 帧"),
        ("p", "WS 端点直接复用前端已有的帧编解码（<code>front/lib/frame.js</code>），<code>payload</code> 就是冻结的契约消息 —— 这样 server-mode 的前端分支只需换 URL，不必改解码。"),
        ("caption", "表 11 · 帧字段与职责归属"),
        ("table", {
            "widths": ["14%", "20%", "30%", "36%"],
            "head": ["字段", "产生方", "语义", "约束"],
            "rows": [
                ["type", "M-24", "<code>command</code> / <code>event</code> / <code>resume</code>", "与 <code>FRAME_TYPE</code> 逐字一致"],
                ["seq", "M-24", "会话内<b>传输</b>序号，单调 +1", "与契约 <code>Message.seq</code> 各自独立，互不覆盖"],
                ["sessionId", "M-24", "会话绑定", "与 URL 里的 id 必须一致，否则断连"],
                ["role", "M-24", "能力面（NFR-04）", "取自会话创建时的 role，运行期不变"],
                ["reqId", "M-24", "命令/回复关联", "REST 请求的 <code>X-Request-Id</code> 与它同源"],
                ["crc", "M-24", "<b>载荷</b>的 CRC-32", "覆盖契约消息，不含会话绑定字段（同 M-07 定义）"],
                ["payload", "契约层", "冻结的 <code>Message</code>", "不做二次包装；错误事件也走它"],
                ["订阅主题", "客户端", "<code>sim.console</code> / <code>sim.state.delta</code> / <code>debug.trace</code> / <code>sim.waveform</code>", "订阅在会话上生效；不订阅不发"],
                ["背压", "M-24", "客户端跟不上时丢帧", "<b>必须</b>发一条 <code>{dropped:{from,to}}</code> 通知，禁静默断流"],
            ],
        }),
    ],
})

# ---------------------------------------------------------------- 4
SECTIONS.append({
    "id": "sec4",
    "h": "4 关键函数",
    "subs": [],
    "blocks": [
        ("p", "14 个函数构成 M-24 的全部实现面。判据是「单一职责、可单测」：凡是能与 IO 分离的判定都做成纯函数，HTTP 层只负责搬运。"),
        ("caption", "表 12 · 关键函数与错误面"),
        ("table", {
            "widths": ["26%", "30%", "26%", "18%"],
            "head": ["函数", "签名（简）", "职责", "可能返回的错误"],
            "rows": [
                ["<code>Config::parse</code>", "<code>(args: &amp;[String]) -&gt; Result&lt;Config, String&gt;</code>", "手写解析，不用 clap（与 <code>osz-boot</code> 同风格，零新依赖）", "用法错误 ⇒ 退出码 1"],
                ["<code>guard_bind</code>", "<code>(cfg) -&gt; Result&lt;(), String&gt;</code>", "非回环必须同时给 <code>--allow-remote</code> 与 token", "启动失败 ⇒ 退出码 2"],
                ["<code>router</code>", "<code>(state: AppState) -&gt; Router</code>", "装配全部端点 + 请求 id 中间件", "—"],
                ["<code>SessionRegistry::create</code>", "<code>(&amp;self, board: Option&lt;String&gt;) -&gt; Result&lt;SessionHandle&gt;</code>", "配额检查 → 起线程 → 绑板 → 注册", "<code>429 + E_SESS-05</code>；<code>404 + E_PLT-22</code>"],
                ["<code>SessionRegistry::sweep</code>", "<code>(&amp;self, now: Instant)</code>", "TTL 回收：置 ctrl=Cancel → join → 摘表", "—"],
                ["<code>SessionHandle::submit</code>", "<code>(&amp;self, job: Job) -&gt; Result&lt;oneshot::Receiver&gt;</code>", "投递命令面；<b>忙则立即拒</b>", "<code>409 + E_SESS-02</code>"],
                ["<code>session_main</code>", "<code>(host, rx, ctrl, ring)</code>", "线程主循环：收 Job → 执行 → 组响应 → 记账", "捕获 panic ⇒ 标 Poisoned"],
                ["<code>run_window</code>", "<code>(host, spec, deadline, ctrl) -&gt; StopReport</code>", "分片推进 + 停因判定（本设计核心）；<b>住内核</b>，服务端与离线 runner 共用这<b>一份</b>实现", "透出内核码"],
                ["<code>judge_stop</code>", "<code>(obs: &amp;Obs) -&gt; Option&lt;StopReason&gt;</code>", "<b>纯函数</b>：按固定次序判停因，可单测", "—"],
                ["<code>ConsoleRing::append</code>", "<code>(&amp;mut self, bytes: &amp;[u8]) -&gt; Range&lt;u64&gt;</code>", "只追加；裁剪时记丢弃区间", "—"],
                ["<code>ConsoleRing::read</code>", "<code>(&amp;self, off: u64, limit: usize) -&gt; ReadResult</code>", "绝对偏移读；越界区分「未来」与「已丢」", "<code>409 + E_SESS-04</code>"],
                ["<code>error_to_response</code>", "<code>(err: PlatformError, req: &amp;str) -&gt; Response</code>", "<b>纯映射</b>：族/场景 → HTTP 状态；不改码、不吞 detail", "—"],
                ["<code>resolve_board</code>", "<code>(req: BoardReq, host) -&gt; Result&lt;BoardReply&gt;</code>", "三来源归一：模板 / 整份描述 / 启动默认", "<code>404/422/409</code>"],
                ["<code>boot_once</code>", "<code>(cfg, req) -&gt; Result&lt;BootReply&gt;</code>", "建临时会话 → 配板 → 装载 → run → 取字节 → 回收", "全量透出 + 504"],
            ],
        }),
        ("p", "两个函数值得单独说。其一 <code>judge_stop</code>：它把「六种停因」收敛成一个纯函数，输入是一份只读观测（内核事件、ctrl 位、deadline 判定、前缀命中、指令计数），"
              "输出是唯一停因 —— 这样「先停哪个」这件事只有一个落点，不会在循环里散成五处 <code>break</code>。"
              "其二 <code>error_to_response</code>：它是<b>纯映射表</code></b>，唯一允许的「加工」是补 HTTP 状态与 <code>request_id</code>。"),
    ],
})

# ---------------------------------------------------------------- 5
SECTIONS.append({
    "id": "sec5",
    "h": "5 处理流程",
    "subs": [("sec5-1", "5.1 会话建立与配板"), ("sec5-2", "5.2 运行：分段推进与停因"),
             ("sec5-3", "5.3 console 的一致性"), ("sec5-4", "5.4 超时、熔断与回收"),
             ("sec5-5", "5.5 并发、隔离与复位")],
    "blocks": [
        ("h3", "sec5-1", "5.1 会话建立与配板"),
        ("p", "建会话是一个<b>串行化的装配</b>：配额 → 线程 → 板 → 注册。任一步失败都要把已经拿到的资源还回去（线程 join + 配额归还），否则「建会话失败」会漏出孤儿线程。"),
        ("svg", "FIG3"),
        ("list", [
            "配额检查通过才起线程；线程启动后立刻进入等待态（队列空），不会有「半初始化会话」被别的请求看见。",
            "<code>--board</code> 的默认板在<b>建会话时</b>执行一次 <code>PutBoard</code>，并在会话元数据记 <code>board_source=\"startup-default\"</code>；客户端自己配的记 <code>\"client\"</code>。",
            "模板来源走 <code>GetPlatformTemplate</code> 取整份描述再 <code>SetMemoryMap</code> —— 与前端同一条路径，不另抄模板数据。",
            "装配失败一律不注册；<code>POST /api/sessions</code> 返回错误体，客户端拿到的是 404/409，而不是一个「建好了但没板」的会话。",
        ]),
        ("h3", "sec5-2", "5.2 运行：分段推进与停因"),
        ("p", "「跑到什么时候停」由服务端判定，客户端不再切 <code>StepN</code>。一个窗口 = 一次 <code>Continue</code>（量子 10 000 条）；每个窗口结束做四件事：取 console、记账、判停因、查 deadline。"),
        ("svg", "FIG2"),
        ("code", """// run_window 的核心循环（伪码，逐字对应表 13 的判定次序）
fn run_window(host, spec, deadline, ctrl) -> StopReport {
    let before = host.total_executed();
    let mut windows = 0u64;
    loop {
        // ① 内核先跑一个量子窗口（不在服务端切更小的片）
        let ev = host.handle_message(&Message::command(seq, Continue));
        let halted = ev.iter().find_map(|e| as_halted(e));       // HaltReason

        // ② 先记账：console 进 ring，事件进 fanout（响应之前必须可读）
        ring.append(&host.drain_console());   // 取走本窗口产生的字节
        windows += 1;

        // ③ 再判停因（纯函数，固定次序）
        let obs = Obs { halted, ctrl, deadline, ring, spec,
                        executed: host.total_executed() };
        if let Some(reason) = judge_stop(&obs) {
            return StopReport::new(reason, host.total_executed() - before,
                                   host.pc(), host.now_ps(), ring);
        }
        if let Err(err) = first_kernel_error(&ev) { return StopReport::failed(err); }
    }
}"""),
        ("caption", "表 13 · 停因判定次序（判据固定，因此结果确定）"),
        ("table", {
            "widths": ["8%", "16%", "30%", "46%"],
            "head": ["次序", "停因", "判据", "为什么排在这一位"],
            "rows": [
                ["1", "<code>halt</code>", "内核 <code>Halted{EndOfExecution|Trap}</code> 或执行终结", "宿主不可继续，必须优先，否则会把「已终结」误报成「预算到」"],
                ["2", "<code>breakpoint</code>", "<code>Halted{Breakpoint|Watchpoint}</code> 且 <code>until=any</code>", "用户显式停点，语义高于数量与时间"],
                ["3", "<code>paused</code>", "<code>ctrl</code> 位为 Pause", "用户意图，必须停；与停点的区别只在「谁下的」"],
                ["4", "<code>timeout</code>", "<code>now &gt;= deadline</code>", "墙钟只作熔断，停在<b>已完成的窗口边界</b>"],
                ["5", "<code>prefix</code>", "ring 中出现 <code>prefix</code> 字节串", "内容停点，比数量更「有意义」"],
                ["6", "<code>budget</code>", "<code>executed &gt;= steps</code>", "数量停点，最后判定"],
            ],
        }),
        ("note", "<b>量子对齐与 overshoot：</b><code>prefix</code> 与 <code>timeout</code> 只能停在窗口边界，因此实际执行条数可能比「理论停点」多出至多一个量子（10 000 条）。"
              "本设计把这个上界写进 <code>meta.overshoot_max</code> 并回显量子常量 —— 使「同一个镜像、同一组参数」在两次运行之间<b>逐字节可比</b>，"
              "也让 AC-01 的 <code>executed</code> 有一个说得清的可复现区间，而不是一个靠运气的数。"),
        ("note", "<b>两个入口、一份判定：</b>停止条件（预算 / 前缀 / 墙钟 / 停因次序）只实现一次 —— 就是住内核的 <code>run_window</code>。"
              "<code>osz-boot</code> 保留为<b>离线</b>入口（不起 daemon、不占端口、不需要 token），服务端 <code>run</code> 与 <code>/api/boot</code> 走同一函数，"
              "差别只在传输形态。判据是 T-14：同 ELF、同参数，两侧 <code>stop_reason</code> 与 console 字节必须一致；次序表被改歪时，表 19 的 <code>judge_stop</code> 先红。"),
        ("h3", "sec5-3", "5.3 console 的一致性"),
        ("p", "console 是<b>字节</b>不是字符串：ANSI 序列、<code>\\r</code>、半截 UTF-8 一律不加工。ring 用绝对 offset，只追加、不改写；裁剪只从尾部发生，并在丢弃表里留证据。"),
        ("code", """// append：只追加，裁剪时记账（不静默丢）
fn append(&mut self, bytes: &[u8]) -> Range<u64> {
    let from = self.total;
    self.buf.extend_from_slice(bytes);
    self.total += bytes.len() as u64;
    while self.buf.len() > self.max {
        let cut = self.buf.len() - self.max;
        self.buf.drain(..cut);              // 只从头部裁
        let dropped_from = self.base;
        self.base += cut as u64;
        self.dropped.push((dropped_from, self.base));
    }
    from..self.total
}

// read：三种结果必须可区分，不许都变成「空」
fn read(&self, off: u64, limit: usize) -> ReadResult {
    if off < self.base {
        return ReadResult::Lost { base: self.base, dropped: self.dropped.clone() };
    }
    if off > self.total { return ReadResult::Future { total: self.total }; }
    let start = (off - self.base) as usize;
    let end = (start + limit).min(self.buf.len());
    ReadResult::Bytes { bytes: self.buf[start..end].to_vec(), total: self.total }
}""" ),
        ("caption", "表 14 · 读取语义（三种「没有字节」必须分开）"),
        ("table", {
            "widths": ["22%", "30%", "18%", "30%"],
            "head": ["情形", "响应", "客户端该做什么", "禁止的做法"],
            "rows": [
                ["<code>off &lt; base</code>", "<code>409 + E_SESS-04</code>，回 base 与丢弃区间", "从 <code>base</code> 重新对齐，或改用 WS 增量", "不要返回空 200 冒充「没有输出」"],
                ["<code>base ≤ off &lt; total</code>", "200 + 原始字节 + <code>X-Console-Total</code>", "按 <code>to</code> 续拉；分页拼接与一次拉取等价", "不要在此处做换行/转义美化"],
                ["<code>off == total</code>", "200 + 空体 + <code>X-Console-Total</code>", "等待下一轮 or 用 WS 订阅", "不要返回 404（会话是好的）"],
                ["<code>off &gt; total</code>", "200 + 空体 + 头里带 <code>X-Console-Total</code>", "把 offset 收敛到 total", "不要 500"],
                ["溢出已发生", "头里带 <code>X-Console-Dropped</code>", "记入日志，必要时扩大 <code>--max-console</code>", "不要静默丢且不报"],
            ],
        }),
        ("h3", "sec5-4", "5.4 超时、熔断与回收"),
        ("p", "超时只做一件事：让推进中的 run <b>停在窗口边界</b>并返回 504。它不改内核时钟、不改寄存器、不重置会话 —— 所以「超时后同会话还能继续用」是结构保证，不是靠重试凑出来的。"),
        ("caption", "表 15 · 超时与资源边界"),
        ("table", {
            "widths": ["18%", "26%", "26%", "30%"],
            "head": ["场景", "判定点", "动作", "会话后续状态"],
            "rows": [
                ["单请求超时", "每窗口一次 <code>Instant::elapsed</code>", "停 → 504 → <code>stop_reason=timeout</code>", "Idle，可 <code>reset</code> 后再 run"],
                ["无进展死循环", "窗口照常完成（10 000 条）", "不触发内核看门狗（那属 M-22）", "仍由墙钟兜住，不无限等"],
                ["会话额度超限", "建会话 / run 前置检查", "429 + 剩余额度", "会话不变，客户端可回收别的会话"],
                ["并发会话超限", "注册表插入时", "429，不排队", "已有会话不受影响"],
                ["进程收到 SIGTERM", "信号处理", "置全部 ctrl=Cancel → join 线程 → 退出", "不留孤儿线程"],
                ["会话 DELETE", "HTTP 层", "置 ctrl → join → 摘表 → 释放 ring", "id 不复用，再访问 404"],
                ["TTL 到期", "10 s 扫表", "同 DELETE，记一条回收日志", "同上"],
            ],
        }),
        ("h3", "sec5-5", "5.5 并发、隔离与复位"),
        ("p", "为什么是「一会话一线程」而不是共享内核：<code>KernelHost</code> 内部是 <code>Rc/RefCell</code>（wasm 侧还有 <code>thread_local</code> 单会话约束），"
              "<b>不是 Send</b>。用 <code>tokio::Mutex</code> 持锁跨 await 编译不过；用 <code>spawn_blocking</code> 又会让状态留在池线程里、下次请求换个线程就找不到。"
              "专属线程是唯一既通用又不需要改内核的形态。"),
        ("caption", "表 16 · 共享与不共享（会话隔离的落点）"),
        ("table", {
            "widths": ["26%", "26%", "48%"],
            "head": ["对象", "归属", "说明"],
            "rows": [
                ["内核状态（寄存器/内存/时钟/断点/console）", "会话线程私有", "跨会话<b>零共享</b>；一侧跑飞不影响另一侧"],
                ["板模板库", "进程只读", "只读缓存；每个会话拿到的是<b>描述副本</b>，改板不影响别人"],
                ["并发与预算", "进程全局", "全局上限保护宿主；单会话额度另记"],
                ["日志 / 指标", "进程全局", "按会话 id 打标签，便于事后定位是哪一侧"],
                ["复位（reset）", "会话内", "重发 <code>LoadProject(last_project)</code>：等价前端「复位＝重新装载当前工程」，不新增 <code>Reset</code> 命令"],
                ["快照 / 恢复（P1）", "会话内", "<code>Snapshot</code> 取、<code>Restore</code> 送；跨会话传快照属于「另一个会话的装载」"],
            ],
        }),
        ("callout", "为什么不做线程抢占", "硬杀线程会让 <code>KernelHost</code> 处于半事务状态（总线写到一半、事件队列未清），下次用它会得到「看起来没坏其实坏了」的会话。"
                  "本设计把「跑飞」收敛到<b>一个量子（≤10 000 条）的延迟</b>，用 deadline 兜底；真要硬隔离，P2 用<b>进程</b>而不是线程（<code>--isolation=process</code>），那时才允许 kill。"),
    ],
})

# ---------------------------------------------------------------- 6
SECTIONS.append({
    "id": "sec6",
    "h": "6 异常处理",
    "subs": [("sec6-1", "6.1 错误码与处置"), ("sec6-2", "6.2 边界与降级")],
    "blocks": [
        ("h3", "sec6-1", "6.1 错误码与处置"),
        ("p", "下表按「谁拥有这个码」分类。<b>M-24 从不新造内核码</b>；它自己的码只有会话族与一个路径码，且都在契约层或本 crate 里唯一定义。"),
        ("caption", "表 17 · 错误码处置矩阵"),
        ("table", {
            "widths": ["16%", "10%", "22%", "22%", "30%"],
            "head": ["code", "HTTP", "拥有方", "典型触发", "处置"],
            "rows": [
                ["<code>E_PROTO-01</code>", "400", "契约 / 内核", "schema 版本不匹配", "回版本与期望值，让客户端对齐"],
                ["<code>E_PROTO-08</code>", "400", "契约 / M-24", "信封或字段形态非法", "指出字段名；不回显整份载荷"],
                ["<code>E_SESS-01</code>", "404", "契约 / M-24", "会话不存在", "建议重建；不暴露内部句柄"],
                ["<code>E_SESS-02</code>", "409", "契约 / M-24", "会话忙", "回状态与建议动作"],
                ["<code>E_SESS-03</code>", "500", "契约 / M-24", "线程 panic、不变量破坏", "带 detail；自动标 Poisoned 并等待回收"],
                ["<code>E_SESS-04</code>", "409", "契约 / M-24", "console offset 早于 base", "回 base 与丢弃区间"],
                ["<code>E_SESS-05</code>", "429", "契约 / M-24", "额度超限", "回剩余额度"],
                ["<code>E_SESS-06</code>", "504", "契约 / M-24", "墙钟超时", "回 stop_reason 与已执行条数"],
                ["<code>E_STORE-07</code>", "403", "M-24 内部", "路径越出白名单", "只说「不在允许目录内」"],
                ["<code>E_PLT-22</code>", "404/409", "契约 / 内核", "未知模板 / 无板装载", "原样透出中文引导文案"],
                ["<code>E_PLT-13</code>", "409", "契约 / 内核", "修订号不符", "回当前 revision"],
                ["<code>E_PLT-17</code>", "422", "契约 / 内核", "镜像落在板外", "detail 带第一个落不进的区间"],
                ["<code>E_PLT-20</code>", "422", "契约 / 内核", "ISA 门禁不过", "点明位宽 / 压缩扩展哪一条"],
                ["<code>E_LDR-01/02/03</code>", "422", "内核 M-06", "ELF 畸形 / 超限 / 段溢出", "原样透出解析定位"],
            ],
        }),
        ("h3", "sec6-2", "6.2 边界与降级"),
        ("list", [
            "<b>装载失败不改变机器状态</b>：与内核次序纪律一致 —— 解析、门禁、建图都在写入之前完成，失败时寄存器与内存一个字节不动。",
            "<b>配板 pending 不自动重载</b>：换了板但没重装镜像时，返回 <code>pending:true</code>；不替客户端做「顺手重装载」，否则故障现场被抹掉。",
            "<b>超时后再 run</b>：允许；<code>reset</code> 也只是重发一次 <code>LoadProject</code>，语义与前端「复位」同口径。",
            "<b>WS 断流</b>：连接断即退订；不保留「断线期间的事件」缓冲（P1 只保证 <code>console</code> 可从 REST 补齐）。",
            "<b>请求体过大</b>：<code>413 + E_PROTO-08</code>（<code>tower_http::limit</code>，默认 32 MiB）+ <code>E_PROTO-08</code>；不给解析器制造 OOM 的机会。",
            "<b>token 缺失（远程模式）</b>：<code>401 + E_SESS-07</code>，detail 只说「需要 X-Auth-Token」，不回显期望值。",
        ]),
    ],
})

# ---------------------------------------------------------------- 7
SECTIONS.append({
    "id": "sec7",
    "h": "7 测试设计与可维护性",
    "subs": [("sec7-1", "7.1 验收用例"), ("sec7-2", "7.2 单元测试与可维护性")],
    "blocks": [
        ("h3", "sec7-1", "7.1 验收用例"),
        ("p", "AC-01～AC-10 直接沿用需求文档的判定，数值用上一轮实测的黄金值；T-11 之后是本设计新增的结构性用例。"),
        ("caption", "表 18 · 验收用例"),
        ("table", {
            "widths": ["8%", "34%", "58%"],
            "head": ["#", "动作", "期望"],
            "rows": [
                ["AC-01", "<code>POST /api/boot {path: osz.elf, prefix:\"OSZ$ \", timeout_ms:30000}</code>", "<code>stop_reason=prefix</code>；console <b>31 字节</b>；含 <code>==== Enter Main ====</code> 与 <code>OSZ$ </code>；<code>pc ≈ 0x80006a7c</code>"],
                ["AC-02", "平台 console 与 Spike stdout 做 <code>cmp</code>", "逐字节相同（MATCH）"],
                ["AC-03", "不配板直接 <code>POST image</code>", "<code>409 + E_PLT-22</code>，detail 保留模板引导"],
                ["AC-04", "装载后 <code>GET state</code>", "<code>pc = 0x80000000</code>；<code>gpr[0]=0</code>；索引与 ABI 一致"],
                ["AC-05", "<code>step {n:1}</code> 连两次", "pc 每条都有变化（或明确 <code>halt</code>）"],
                ["AC-06", "<code>main</code> 首条下断点后 <code>run</code>", "<code>stop_reason=breakpoint</code>；地址与符号解析一致"],
                ["AC-07", "死循环镜像 <code>run {timeout_ms:3000}</code>", "约 3 s 返回 <code>timeout</code> + 504；随后 <code>reset</code> 仍可用"],
                ["AC-08", "两会话并行跑同一镜像", "各得 31 字节；删 A 后 B 仍可 run"],
                ["AC-09", "console 分两页取（<code>limit=16</code>）", "拼接与一次取全一致；<code>X-Console-Total</code> 恒 31"],
                ["AC-10", "同参数连跑 3 次", "console、<code>stop_reason</code>、最终 pc 三次一致"],
                ["T-11", "<code>POST …/run</code> 与正在跑的 run 重叠", "<code>409 + E_SESS-02</code>；不乱序、不交错"],
                ["T-12", "<code>pause</code> 打在长 run 上", "≤ 1 个量子内停；<code>stop_reason=paused</code>"],
                ["T-13", "console 溢出后按旧 offset 取", "<code>409 + E_SESS-04</code>，回 base 与丢弃区间"],
                ["T-14", "同一 ELF 双入口对拍（<code>osz-boot</code> × <code>/api/boot</code>）", "<code>stop_reason</code> 与 console 字节一致；不进默认门禁（需仓外 ELF）"],
            ],
        }),
        ("h3", "sec7-2", "7.2 单元测试与可维护性"),
        ("p", "凡是「判定」都做成纯函数，用表驱动单测；凡是「装配」都用集成测试跑真内核。<b>协议形状</b>不进单测 —— 契约层自己的 golden JSON 已经在管这件事。"),
        ("caption", "表 19 · 单元测试清单"),
        ("table", {
            "widths": ["30%", "44%", "26%"],
            "head": ["被测对象", "用例要点", "可证伪性"],
            "rows": [
                ["<code>judge_stop</code>", "六种停因两两同时成立时，取次序在前的那个", "把次序表改一位 ⇒ 期望用例变红"],
                ["<code>ConsoleRing::read</code>", "Lost / Bytes / Empty / Future 四种结果", "把 <code>off &lt; base</code> 改成返回空 ⇒ 变红"],
                ["<code>ConsoleRing::append</code>", "裁剪后 <code>base/total/dropped</code> 三者自洽", "裁剪不记 dropped ⇒ 变红"],
                ["<code>error_to_response</code>", "14 个码逐条断言 status + code", "漏一个映射 ⇒ 变红"],
                ["<code>guard_bind</code>", "四个组合（回环/非回环 × 有/无 token）", "放开任一条 ⇒ 变红"],
                ["<code>Config::parse</code>", "缺值、坏值、端口 0、重复参数", "解析 panic ⇒ 变红"],
                ["<code>resolve_board</code>", "模板 / 描述 / 默认 / 未知 id", "未知 id 回落成默认 ⇒ 变红"],
                ["<code>boot_once</code>", "任一步失败都要回收临时会话", "资源泄漏用例（线程数 +1）⇒ 变红"],
            ],
        }),
        ("p", "可维护性:三件事坚持住。① <b>量子常量只有一个来源</b>（<code>DEFAULT_INSTRUCTION_BUDGET</code>），M-24 不抄第二份数；"
              "② <b>错误映射是表</b>，不是 if-else 链；③ <b>停因判定只有一个落点</b>：<code>run_window</code> 住内核，服务端与离线 runner 都调它 —— "
              "两条入口各有自己的传输形态，但<b>不各写一套停因次序</b>。"),
    ],
})

# ---------------------------------------------------------------- 8
SECTIONS.append({
    "id": "sec8",
    "h": "8 质量属性落实",
    "subs": [],
    "blocks": [
        ("caption", "表 20 · 质量属性与落实手段"),
        ("table", {
            "widths": ["14%", "22%", "40%", "24%"],
            "head": ["属性", "目标", "手段", "证据"],
            "rows": [
                ["确定性", "同输入同输出", "预算按条数；量子常量回显；无随机初值；墙钟只熔断", "AC-10 + 快照回放"],
                ["隔离性", "一会话跑飞不伤别人", "专属线程 + 会话内串行 + 全局配额", "AC-08 / T-11"],
                ["吞吐", "冷启动 < 2 s；单会话 1.5M 指令可接受", "无重解析；console 增量取；分片不复制状态", "AC-01 计时"],
                ["可控性", "跑飞杀得掉", "窗口间 deadline + ctrl 位；P2 进程隔离", "AC-07"],
                ["可观测", "出问题能定位", "<code>/healthz</code> + 会话表 + 请求 id + 回收日志", "运维演练"],
                ["安全性", "默认可信边界", "只绑回环 / 白名单 / 拒穿越 / 远程需 token", "<code>/etc/passwd</code> 用例"],
                ["兼容性", "不破坏既有形态", "内核零改动；契约只加错误族；前端 Worker 形态不变", "全量单测 + 前端门禁"],
                ["可测性", "判定可单测", "纯函数 + 表驱动 + 真内核集成", "表 19"],
            ],
        }),
    ],
})

# ---------------------------------------------------------------- 9
SECTIONS.append({
    "id": "sec9",
    "h": "9 实施排期与决策记录",
    "subs": [("sec9-1", "9.1 分阶段落地"), ("sec9-2", "9.2 五项决策与落地取舍")],
    "blocks": [
        ("h3", "sec9-1", "9.1 分阶段落地"),
        ("p", "按「每一步都能被脚本验证」切片，避免一口气做完再联调。每片都自带一条验收，红即回滚。"),
        ("caption", "表 21 · 里程碑"),
        ("table", {
            "widths": ["8%", "26%", "38%", "28%"],
            "head": ["片", "内容", "完成标志", "依赖"],
            "rows": [
                ["P0-1", "server 骨架：Config / router / version / healthz / 错误映射", "<code>curl /api/version</code> 与内核版本一致", "契约 v18"],
                ["P0-2", "会话表 + 专属线程 + 配额 + TTL", "两会话并存、删除互不影响", "P0-1"],
                ["P0-3", "配板 + 装载（octet-stream / path / 模板）", "osy.elf 装载 entry=0x80000000", "P0-2 + 模板端点"],
                ["P0-4", "run / step / pause / reset + console ring（已落地：内核 <code>run_until</code> 是停因唯一实现，服务端只做请求折算与 504 包装）", "AC-01 / AC-05 / AC-07 / AC-09", "P0-3"],
                ["P0-5", "状态 / 内存 / 符号 / 反汇编 / 断点（已落地：读面全走内核既有命令，符号类断点先 <code>ResolveSymbol</code> 再注册）", "AC-04 / AC-06", "P0-4"],
                ["P0-6", "<code>/api/boot</code>；<code>osz-boot</code> 换用内核 <code>run_window</code>（CLI 与退出码不变）", "一条 curl 完成 osz 判定，与 Spike MATCH；<code>--print-memory</code> 链路保持；顺带修掉 <code>E_PLT-22</code> 断链", "P0-5"],
                ["P1", "WS 事件流 + 快照 + trace + 异步 run", "前端 server-mode 分支可用", "P0-6"],
            ],
        }),
        ("h3", "sec9-2", "9.2 五项决策与落地取舍"),
        ("p", "需求文档 §08 的五项会改变实现形态，五项<b>已全部拍板</b>；实施期又追加一项（D6 装载回执，见表 22）。其中 D1–D4 按本设计的建议通过；"
              "D5 改为保留 <code>osz-boot</code> 作为<b>离线</b>入口，因此额外的落地约束是「停因判定必须共处一份实现」，见 §5.2 的末尾说明。"),
        ("caption", "表 22 · 决策点与裁决"),
        ("table", {
            "widths": ["6%", "16%", "34%", "12%", "32%"],
            "head": ["#", "决策点", "选项", "建议", "理由"],
            "rows": [
                ["D1", "传输形态", "① REST + WS 混合；② 纯 WS 单通道", "①（已拍板）", "CI 要 <code>curl</code> 一次拿结果；流才用 WS，且帧可复用 M-07"],
                ["D2", "镜像投递", "① 只路径；② 只上传；③ 两者", "③（已拍板）", "Agent 传字节、CI 传路径都合理；路径必须白名单"],
                ["D3", "<code>run</code> 语义", "① 同步到停点；② 立即返回 + 回调", "①（已拍板；② 留 P1）", "CI 的「一次调用拿到结果」不可退让"],
                ["D4", "默认板", "① 不提供；② 启动参数显式配", "②（已拍板）", "显式、可追溯，且不绕过「无板不装载」"],
                ["D5", "<code>/api/boot</code> 与 CLI", "① CLI 保留为<b>离线</b> runner（不起 daemon）；② CLI 变薄客户端（依赖 daemon）", "①（已拍板）", "离线验证不能依赖常驻进程；代价是停因判定必须共用内核<b>一处</b>，否则两套实现会漂，而两边 debug 都「看着对」"],
                ["D6", "装载回执", "① 内核发 <code>ImageLoaded</code>（契约升 19）；② 宿主侧再解析一遍合成回执", "①（已拍板）", "解析只在装载路径做<b>一次</b>：回执是「真的装了什么」的证据；宿主侧再解析=两个来源，且多一次 ELF 解析开销"],
            ],
        }),
        ("callout", "本设计对需求的唯一主动偏离", "R-15 的 <code>E_IO-xx</code> 在契约层无族，改落 <code>E_STORE-07</code>（见 §3.5）。"
                  "若受理方更倾向「新开 IO 族」，代价是前端镜像表与族清单各加一行 —— 但会多出一个与 STORE 语义重叠的族，不推荐。"),
        ("h3", "sec9-3", "附录 A 最小可脚本化序列"),
        ("code", """# 1) 版本协商（打印实际绑定地址与 schema）
curl -s http://127.0.0.1:9876/api/version

# 2) 建会话 + 配板 + 装镜像（任一步失败都带 code / detail / status）
SID=$(curl -s -X POST http://127.0.0.1:9876/api/sessions | jq -r .session_id)
curl -s -X PUT  "http://127.0.0.1:9876/api/sessions/$SID/board" -d '{"template":"qemu-virt"}'
curl -s -X POST "http://127.0.0.1:9876/api/sessions/$SID/image" \\
     -H 'Content-Type: application/octet-stream' --data-binary @output/image/osz.elf

# 3) 跑到 shell，拿停因
curl -s -X POST "http://127.0.0.1:9876/api/sessions/$SID/run" \\
     -d '{"prefix":"OSZ$ ","timeout_ms":30000}'
# → {"stop_reason":"prefix","executed":1500000,"pc":2147506812,
#    "console":{"from":0,"to":31,"total":31},
#    "meta":{"quantum":10000,"windows":150,"overshoot_max":10000}}

# 4) 取 console 原始字节（直接与 spike stdout 做 cmp）
curl -s "http://127.0.0.1:9876/api/sessions/$SID/console?offset=0" > platform.bin
cmp platform.bin spike.bin && echo MATCH

# 5) 一把梭（唯一判定口径）
curl -s -X POST http://127.0.0.1:9876/api/boot \\
     -d '{"path":"output/image/osz.elf","prefix":"OSZ$ ","timeout_ms":30000}' \\
     -o console.bin -D meta.txt   # meta 全部在 X-Sim-Meta / X-Stop-Reason 头里"""),
        ("p", "落成之后，客户端侧的承诺照需求执行：<code>tools/sim/run_on_platform.mjs</code>（import wasm 的过渡方案）退役，"
              "文档里的运行验证命令统一改成 API 形态；<code>osz-boot</code> 保留同名 CLI、退出码与 <code>--print-memory</code> 语义，"
              "作为<b>离线</b>入口（不要求先起 server），内部换用与 M-24 相同的 <code>run_window</code> ⇒ ⇒ 两个入口、一份判定。"),
    ],
})

# ---------------------------------------------------------------- renderer
def esc(t: str) -> str:
    return html.escape(t, quote=False)


def render_table(spec: dict) -> str:
    widths = spec["widths"]
    cols = "".join(f'<col style="width:{w}">' for w in widths)
    head = "".join(f"<th>{c}</th>" for c in spec["head"])
    rows = []
    for i, row in enumerate(spec["rows"]):
        cls = ' class="row1"' if i % 2 else ""
        cells = "".join(
            f'<td data-label="{html.escape(strip_tags(spec["head"][j]) if j < len(spec["head"]) else "", quote=True)}">{c}</td>'
            for j, c in enumerate(row)
        )
        rows.append(f"<tr{cls}>{cells}</tr>")
    return (
        '<div class="tbl-wrap"><table class="doc-table wide">'
        f"<colgroup>{cols}</colgroup><thead><tr>{head}</tr></thead>"
        f"<tbody>{''.join(rows)}</tbody></table></div>"
    )


def strip_tags(t: str) -> str:
    return re.sub(r"<[^>]+>", "", t)


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
        elif kind == "code":
            out.append(f'<pre class="code">{esc(val)}</pre>')
        elif kind == "list":
            items = "".join(f"<li>{i}</li>" for i in val)
            out.append(f'<ul class="list">{items}</ul>')
        elif kind == "callout":
            tag, text = (b[1], b[2]) if len(b) >= 3 else (val[0], val[1])
            out.append(
                f'<div class="callout"><span class="callout-tag">{tag}</span>'
                f'<span class="callout-txt">{text}</span></div>'
            )
        elif kind == "h3":
            out.append(f'<h3 id="{b[1]}" class="h-sub3">{b[2]}</h3>')
        elif kind == "svg":
            out.append({"FIG1": FIG1, "FIG2": FIG2, "FIG3": FIG3}[val])
        elif kind == "note":
            out.append(f'<div class="note">{val}</div>')
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
.h-sub3 { font-size:15.5px; font-weight:700; color:#374151; margin:22px 0 8px; scroll-margin-top:16px; }
.body { margin:0 0 10px; text-align:justify; }
.tbl-wrap { overflow-x:auto; margin:12px 0 16px; }
.doc-table { width:100%; border-collapse:collapse; table-layout:fixed; font-size:13px; line-height:1.6; background:#fff; }
.doc-table th { background:var(--head-bg); color:#111827; font-weight:700;
  border:1px solid var(--line); padding:7px 8px; vertical-align:middle; }
.doc-table td { border:1px solid var(--line); padding:6px 8px; vertical-align:middle; word-break:break-word; }
.doc-table tr.row1 { background:var(--zebra); }
.caption { text-align:left; font-size:12.5px; color:var(--ink-2); margin:14px 0 4px; font-weight:700; }
.code { background:var(--code-bg); color:#e2e8f0; font-family:'JetBrains Mono','Cascadia Mono',Consolas,monospace;
  font-size:12.5px; line-height:1.62; padding:12px 14px; border-radius:6px; overflow-x:auto;
  white-space:pre; margin:10px 0 16px; }
.fig { margin:14px 0 6px; }
.fig .figcap { text-align:center; font-size:12.5px; color:var(--ink-2); margin-top:4px; }
.flow-svg { width:100%; height:auto; display:block; }
.list { margin:8px 0 14px; padding-left:20px; }
.list li { margin:5px 0; font-size:14.5px; }
.callout { display:flex; gap:8px; align-items:flex-start; background:#fffbeb; border:1px solid #fcd34d;
  border-left-width:4px; border-radius:6px; padding:9px 12px; margin:12px 0 16px; }
.callout-tag { flex:0 0 auto; font-weight:800; color:#92400e; font-size:13px; }
.callout-txt { font-size:13.5px; color:#3f2d0a; }
.note { background:var(--head-bg); border:1px solid var(--line); border-radius:6px;
  padding:9px 12px; margin:12px 0 16px; font-size:13.5px; color:var(--ink-2); }
.note b { color:#111827; }
.nav-toggle { margin:14px 0 4px; }
.nav-toggle summary { cursor:pointer; font-weight:700; color:var(--brand);
  padding:9px 12px; border:1px solid var(--line); border-radius:6px; background:var(--head-bg); list-style:none; }
.nav-toggle summary::-webkit-details-marker { display:none; }
.nav-box { display:flex; flex-direction:column; gap:2px; padding:10px 4px 2px; }
.nav-l1 { color:var(--brand); font-weight:700; text-decoration:none; padding:3px 4px; }
.nav-l2 { color:var(--ink-2); text-decoration:none; padding:3px 4px 3px 18px; font-size:13.5px; }
.nav-l1:hover,.nav-l2:hover { color:var(--brand-2); text-decoration:underline; }
.foot { margin:40px 0 8px; padding-top:10px; border-top:1px solid var(--line);
  font-size:12.5px; color:#6b7280; }
@media (min-width:768px) {
  .wrap { display:grid; grid-template-columns:250px minmax(0,1fr); gap:26px; align-items:start; }
  .doc-head { grid-column:1 / -1; }
  .nav-toggle { position:sticky; top:16px; max-height:calc(100vh - 32px); overflow:auto; }
  .nav-toggle summary { pointer-events:none; }
}
@media (max-width:767px) {
  .doc-table.wide thead { display:none; }
  .doc-table.wide, .doc-table.wide tbody, .doc-table.wide tr, .doc-table.wide td { display:block; width:100%; }
  .doc-table.wide tr { border:1px solid var(--line); margin:10px 0; border-radius:6px; background:#fff; }
  .doc-table.wide tr.row1 { background:var(--zebra); }
  .doc-table.wide td { border:none; border-bottom:1px dashed var(--line); padding:6px 10px 6px 104px;
    position:relative; text-align:left !important; }
  .doc-table.wide td:last-child { border-bottom:none; }
  .doc-table.wide td::before { content:attr(data-label); position:absolute; left:10px; top:7px;
    font-weight:700; color:var(--brand); font-size:12px; width:88px; }
  .kpis { grid-template-columns:repeat(2,1fr); }
  .wrap { padding:12px; }
  .doc-title { font-size:20px; }
  .h-sec { font-size:18px; }
}
@media print {
  .nav-toggle { display:none; }
  .wrap { display:block; max-width:none; padding:0; }
  .doc-table, .fig, .code { break-inside:avoid; }
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
<header class="doc-head">
<h1 class="doc-title">{esc(TITLE)}</h1>
<p class="subtitle">{esc(SUBTITLE)}</p>
<p class="version">{esc(VERSION)}</p>
<div class="kpis">{kpis}</div>
</header>
{nav}
<main>
{body}
<p class="foot">本文件由 <b>tools/build_m24_doc.py</b> 数据驱动生成；改内容请改脚本后重跑，不要手改 HTML。<br>
冻结后的规格若需 DOCX/PDF 副本，另走导出，不以手改的 HTML 为准。</p>
</main>
</div>
</body>
</html>
"""


def main() -> int:
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else OUT_DEFAULT
    nav_items = []
    for s in SECTIONS:
        nav_items.append(f'<a class="nav-l1" href="#{s["id"]}">{s["h"]}</a>')
        for sid, st in s.get("subs", []):
            nav_items.append(f'<a class="nav-l2" href="#{sid}">{st}</a>')
    nav = (
        '<nav class="nav-toggle"><details open><summary>目录</summary>'
        f'<div class="nav-box">{"".join(nav_items)}</div></details></nav>'
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
