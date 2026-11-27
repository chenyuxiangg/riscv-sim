#!/usr/bin/env python3
"""生成「AI 接入层（MCP）」运行视图 SVG。

    python3 tools/build_mcp_view.py        # → docs/architecture/mcp-view.svg

渲染 PNG（Chrome headless）：

    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless \
      --screenshot=docs/architecture/mcp-view.png --window-size=1700,1060 \
      "file://$PWD/docs/architecture/mcp-view.svg"

⚠️ 图上的事实来源（不要在描述里加没验证过的东西）：
   - riscv-sim 容器：镜像 v0.1.1、0.0.0.0:8876、healthy（ali_hy 实测）
   - 会话池：max_sessions=8 / session_ttl=900s / max_run_ms=5000（config.rs 常量）
   - sim-mcp：**已与 sim 同容器**（单镜像 riscv_sim）
"""
from html import escape
from unicodedata import east_asian_width

W, H = 1700, 1060


def _tw(s, size):
    """估算字宽：CJK 全角 ≈ size，其余 ≈ 0.55 size（够用来防溢出）。"""
    return sum(size if east_asian_width(c) in "WF" else size * 0.55 for c in s)


def _wrap(s, max_px, size):
    """按估算宽度折行；不切词，只在超过预算时断开。"""
    out, cur = [], ""
    for c in s:
        if _tw(cur + c, size) > max_px and cur:
            out.append(cur)
            cur = c
        else:
            cur += c
    if cur:
        out.append(cur)
    return out


def rect(x, y, w, h, fill, stroke, dash=None, r=10, sw=1.4, op=1):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" ry="{r}" '
            f'fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{d} opacity="{op}"/>')


def text(x, y, s, size=12, weight=400, fill="#1c2430", anchor="start", mono=False, op=1):
    fam = "ui-monospace,SFMono-Regular,Menlo,monospace" if mono else \
          '-apple-system,BlinkMacSystemFont,PingFang SC,Helvetica Neue,sans-serif'
    return (f'<text x="{x}" y="{y}" font-family="{fam}" font-size="{size}" '
            f'font-weight="{weight}" fill="{fill}" text-anchor="{anchor}" opacity="{op}">{escape(s)}</text>')


def card(x, y, w, h, title, lines, fill="#ffffff", stroke="#cfd6df", dash=None,
         title_fill="#1c2430", mono_lines=(), size=11, pad=13, title_size=12.5, line_h=17):
    """卡片自带折行 + 缩字号：装不下就缩到能装下（最小 9），绝不溢出边框。"""
    avail = w - pad * 2
    head = 22 + line_h  # 标题到首行
    budget = h - head - 8

    def layout(sz):
        wrapped = []
        for i, line in enumerate(lines):
            parts = _wrap(line, avail, sz)
            for j, part in enumerate(parts):
                wrapped.append((part, i if j == 0 else -1))
        return wrapped, sz

    wrapped, sz = layout(size)
    need = len(wrapped) * line_h
    while need > budget and sz > 9:
        sz -= 0.5
        wrapped, sz = layout(sz)
    if need > budget:
        print(f"⚠ 卡片放不下：{title!r}（需 {len(wrapped)} 行 / 只有 {budget // line_h} 行）")

    out = [rect(x, y, w, h, fill, stroke, dash)]
    out.append(text(x + pad, y + 22, title, title_size, 600, title_fill))
    yy = y + head
    for k, (part, orig) in enumerate(wrapped):
        is_first = orig == 0
        out.append(text(x + pad, yy, part, sz, 400,
                        "#243043" if is_first else "#4a5666",
                        mono=(orig in mono_lines)))
        yy += line_h
    return "\n".join(out)


def arrow(x1, y1, x2, y2, label=None, dash=None, color="#5b6b7f", lx=None, ly=None, anchor="middle"):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    out = [f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" '
           f'stroke-width="1.6" marker-end="url(#arrow)"{d}/>']
    if label:
        out.append(text(lx if lx is not None else x1, ly if ly is not None else y1,
                        label, 10.5, 500, "#5b6b7f", anchor=anchor))
    return "\n".join(out)


svg = []
svg.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">')
svg.append('''<defs>
<marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
  <path d="M0,0 L10,5 L0,10 z" fill="#5b6b7f"/>
</marker>
<pattern id="dots" width="16" height="16" patternUnits="userSpaceOnUse">
  <circle cx="1" cy="1" r="1" fill="#e6eaef"/>
</pattern>
</defs>''')
svg.append(f'<rect width="{W}" height="{H}" fill="#f5f7fa"/>')
svg.append(f'<rect width="{W}" height="{H}" fill="url(#dots)" opacity="0.9"/>')

# ------------------------------------------------------------------ 标题
svg.append(text(40, 56, "运行视图 · AI 接入层：MCP 部署在 ali_hy，只经隧道", 25, 700))
svg.append(text(40, 84, "实线 = 已在跑（单镜像 riscv_sim：服务端 + sim-mcp 0.1.0，ali_hy 实测通过）；虚线 = 仅剩「还没做」的登记项。", 12.5, 400, "#61708a"))
svg.append(text(40, 102, "AI 只看得见工具，看不见 8876，也拿不到 SIM_TOKEN —— 这条边界是整张图的重点。", 12.5, 400, "#61708a"))
svg.append(text(W - 40, 56, "front/ · crates/{kernel,contract,server} · sim-mcp 已合体", 11.5, 400, "#8a97a8", anchor="end", mono=True))

# ------------------------------------------------------------------ A. 你的 Mac
MX, MY, MW, MH = 40, 118, 424, 618
svg.append(rect(MX, MY, MW, MH, "#ffffff", "#a9b9cd", sw=1.8))
svg.append(text(MX + 20, MY + 28, "A · 你的 Mac（客户端 + 隧道）", 14.5, 700, "#243043"))

svg.append(card(MX + 18, MY + 44, MW - 36, 132, "AI 客户端（任选一个）", [
    "Claude Code / Cursor / 自研 agent 都行",
    "Streamable HTTP（或本地 stdio 桥）",
    "只持 SIM_MCP_TOKEN：也只能调 9 个工具",
    "对仿真器一无所知：不拼 curl、不认识 8876",
    "换客户端只改 config，工具面不动",
], fill="#eef3f9", stroke="#a9b9cd"))

svg.append(card(MX + 18, MY + 188, MW - 36, 108, "本地回环端口 · 隧道口", [
    "ssh -f -N -L 18877:127.0.0.1:8877 ali_hy",
    "只有隧道活着，本机 18877 才存在（无监听=隧道死了）",
    "断开的表现是「连接被拒绝」，不是「仿真器坏了」",
], fill="#eef3f9", stroke="#a9b9cd", mono_lines=(0,), size=10.5))

svg.append(card(MX + 18, MY + 308, MW - 36, 104, "这里刻意没有的东西", [
    "没有 SIM_TOKEN（server 令牌不下发到客户端侧）",
    "没有 8876 直连（那条是浏览器老路径，与 AI 无关）",
    "没有 Docker 引擎 —— Mac 上跑不了镜像，验证只在 ali_hy",
], fill="#fff7e8", stroke="#e8c98a"))

svg.append(card(MX + 18, MY + 424, MW - 36, 96, "验收线（已通过）", [
    "装 8 字节 RawHex 固件 → advance 2 次",
    "→ 工具回 pc=8 · x5=3，中途没被纠正",
], fill="#fbfcfe", stroke="#cdd5e0"))

svg.append(card(MX + 18, MY + 520, MW - 36, 82, "隧道给的是「先验身份」", [
    "能开隧道 = 有这台机器 SSH key 的人",
    "⇒ 工具面冻结前，先别开安全组",
], fill="#fbfbfc", stroke="#c8ced8", dash="6 4"))

# A → B 连线
svg.append(arrow(MX + MW, MY + 120, 552, MY + 120, color="#3f6fa8"))
svg.append(text(MX + MW + 44, MY + 104, "① tools/call", 10.8, 600, "#3f6fa8", anchor="middle"))
svg.append(text(MX + MW + 44, MY + 136, "HTTP · Bearer", 9.8, 400, "#5b6b7f", anchor="middle", mono=True))

# ------------------------------------------------------------------ B. ali_hy
BX, BY, BW, BH = 552, 118, 736, 618
svg.append(rect(BX, BY, BW, BH, "#ffffff", "#7c8ea6", sw=1.8))
svg.append(text(BX + 20, BY + 28, "B · ali_hy　8.155.139.119（单镜像双进程：sim + mcp 同一容器）", 14.5, 700, "#243043"))

# sim-mcp（虚线：待建）
svg.append(rect(BX + 18, BY + 44, BW - 36, 292, "#faf9ff", "#8f74c9", sw=1.8, r=10))
svg.append(text(BX + 32, BY + 68, "同容器进程 sim-mcp（0.1.0 · 只绑 127.0.0.1:8877）", 13, 700, "#4a3f66"))

svg.append(card(BX + 32, BY + 80, 328, 104, "入口与鉴权", [
    "只绑回环 127.0.0.1:8877，公网没有这个口",
    "Bearer <MCP token>；退化 /mcp/<token>",
    "一条令牌，无角色、无过期（不是 OAuth）",
], fill="#ffffff", stroke="#c3cedb", size=10.5))

svg.append(card(BX + 376, BY + 80, 328, 104, "会话归属表（隔离的唯一手段）", [
    "client_id → session_id，一客户端 1 个",
    "boot 只建自己的，空闲 5 min 主动 DELETE",
    "对客户端永不暴露 /api/sessions 列表",
], fill="#ffffff", stroke="#c3cedb", size=10.5))

svg.append(card(BX + 32, BY + 196, 328, 128, "工具层 · 9 个（写细分 / 读合并）", [
    "boot · load_image · advance · run · poll_job",
    "inspect(kind) · disasm · breakpoints · events",
    "读合并、写细分；只认 Command 变体（不认路由）",
    "返回人读摘要 + 机器字段（不倒 32 个寄存器）",
], fill="#ffffff", stroke="#c3cedb", size=10.5, dash="6 4", mono_lines=(0, 1)))

svg.append(card(BX + 376, BY + 196, 328, 128, "闸门与审计", [
    "破坏性工具默认不注册（要显式开环境变量）",
    "max_run_ms 封顶 5000，客户端不能抬价",
    "每调用一行：client / tool / 耗时 / 错误码",
    "没有它，远程出事定位不到「谁把会话搞没」",
], fill="#ffffff", stroke="#c3cedb", size=10.5))

# riscv-sim（实线：已跑）
svg.append(rect(BX + 18, BY + 360, BW - 36, 238, "#f7fbf7", "#a9c8a9", sw=1.8, r=10))
svg.append(text(BX + 32, BY + 384, "同容器进程 riscv-sim（v0.1.1 二进制 · 0.0.0.0:9876）", 13, 700, "#2f5a34"))

svg.append(card(BX + 32, BY + 396, 328, 88, "入口与令牌", [
    "0.0.0.0:8876（安全组已放行 · 浏览器老路径）",
    "x-auth-token；WS 例外 ?token=；SIM_TOKEN 不进客户端",
], fill="#ffffff", stroke="#a9c8a9", size=10.5, mono_lines=(0,)))

svg.append(card(BX + 376, BY + 396, 328, 88, "会话池与配额", [
    "max_sessions 8（MCP 划 4 · ttl 900s）",
    "忙 ⇒ 409 E_SESS-02；list 能列全部，MCP 必须吞掉",
], fill="#ffffff", stroke="#a9c8a9", size=10.5))

svg.append(card(BX + 32, BY + 496, 672, 86, "内核与事件环", [
    "同一份 crates/kernel 的 native 目标 · CLINT / PLIC / UART / GPIO / Timer",
    "EventRing seq ⇒ events(since_seq) 增量补拉；run 回 job ⇒ /jobs/{job} 轮询",
], fill="#eef7ee", stroke="#a9c8a9", size=10.5))

# sim-mcp → riscv-sim 两条
svg.append(f'<line x1="{BX + BW // 2 - 60}" y1="{BY + 336}" x2="{BX + BW // 2 - 60}" y2="{BY + 360}" stroke="#3f6fa8" stroke-width="1.6" marker-end="url(#arrow)"/>')
svg.append(text(BX + BW // 2 - 52, BY + 356, "② 回环 9876 · SIM_TOKEN", 10.5, 600, "#3f6fa8"))
svg.append(f'<line x1="{BX + BW // 2 + 130}" y1="{BY + 360}" x2="{BX + BW // 2 + 130}" y2="{BY + 336}" stroke="#8a6b3c" stroke-width="1.6" marker-end="url(#arrow)" stroke-dasharray="5 4"/>')
svg.append(text(BX + BW // 2 + 138, BY + 356, "③ 应答 / events / job", 10.5, 600, "#8a6b3c"))

# ------------------------------------------------------------------ C. 端口与凭据
CX2, CY, CW2, CH2 = 1312, 118, 348, 618
svg.append(rect(CX2, CY, CW2, CH2, "#ffffff", "#c3cedb", sw=1.8))
svg.append(text(CX2 + 18, CY + 28, "C · 端口与凭据矩阵", 14.5, 700, "#243043"))

svg.append(card(CX2 + 16, CY + 44, CW2 - 32, 128, "三个端口，三种暴露面", [
    "8876  0.0.0.0  安全组已开  浏览器直连（人看）",
    "8877  127.0.0.1 仅回环  未开安全组（AI 走隧道）",
    "9876  容器内  不发布  只在 docker 网络里",
], fill="#fbfcfe", stroke="#cdd5e0", size=10.2, mono_lines=(0, 1, 2)))

svg.append(card(CX2 + 16, CY + 184, CW2 - 32, 128, "两条令牌，两种权限", [
    "SIM_TOKEN（fc63f4c6…） 全权：建/删/查所有会话",
    "⇒ 只给 sim-mcp 进程，永不进客户端",
    "SIM_MCP_TOKEN  只能调 9 个工具，且只见自己的会话",
], fill="#fff7e8", stroke="#e8c98a", size=10.2))

svg.append(card(CX2 + 16, CY + 324, CW2 - 32, 112, "为什么先走隧道", [
    "工具面没冻结前不开口子，少一次公网暴露",
    "隧道 = 零配置鉴权边界（先验 SSH 身份）",
    "日后要开：安全组加 8877 一条规则，代码不改",
], fill="#fbfbfc", stroke="#c8ced8", dash="6 4", size=10.5))

svg.append(card(CX2 + 16, CY + 448, CW2 - 32, 140, "还没做（如实登记）", [
    "多客户端并发未压测（共享 8 个会话额度）",
    "令牌 → 角色 / 配额：没做，现在一条令牌通吃",
    "MCP 仍是 python 旁挂进程，没并进 Rust 单二进制",
    "远程审计日志没有轮转与保留策略",
], fill="#ffffff", stroke="#c3cedb", size=10.2))

# ------------------------------------------------------------------ D. 契约带
svg.append(rect(40, 754, 1620, 92, "#f6f3fd", "#b9a8dd", sw=1.8))
svg.append(text(60, 782, "唯一接缝 · crates/contract（schema v19）—— 工具面的唯一来源，也是升版时唯一要谈的地方", 14.5, 700, "#3f2f66"))
svg.append(text(60, 806, "Command 信封 20 个变体 ⇒ 9 个工具：写操作细分（可审计、可分级），读操作合并（省上下文）；MCP 层不重新设计能力面。", 11.4, 400, "#4a3f66"))
svg.append(text(60, 828, "server 升 schema ⇒ sim-mcp 的 contract 冒烟必须变红。工具面对 HTTP 路由无知：传输可换（stdio / Streamable HTTP / 内嵌 Rust），换个壳不用重写工具。", 11.4, 400, "#4a3f66"))

# ------------------------------------------------------------------ 底部三块
svg.append(card(40, 862, 520, 150, "一、四条不变量（破了就等于没有这一层）", [
    "① 凭据两层：客户端拿不到 SIM_TOKEN",
    "② 会话归属：看不见别人的会话，也不许 list",
    "③ 破坏性默认关：reset / restore 不注册",
    "④ 审计一行：谁、调了什么、耗时、错误码",
    "破了任一条，远程 MCP 就退回「一条共享令牌 + 直接开 8876」",
], fill="#ffffff", stroke="#c3cedb", size=10.8))

svg.append(card(590, 862, 520, 150, "二、长任务与「仿真器坏了」的误判", [
    "run 同步等 ⇒ 必然撞 5000ms 预算 ⇒ agent 误判内核挂死",
    "所以 run 只回 job_id，poll_job 去拉 /jobs/{job}",
    "事件用 events(since_seq) 补，不靠长连 WS（隧道会断）",
    "隧道断开 = 连接错误；仿真器异常 = 工具回结构化错误码",
    "一容器两进程 ⇒ 任一退出整容器重启（不做进程级自愈）",
], fill="#ffffff", stroke="#c3cedb", size=10.8))

svg.append(card(1140, 862, 520, 150, "三、现在到哪一步", [
    "已合体：riscv_sim 单镜像（192MB，两进程一容器，healthy）",
    "已验收：隧道内 check_mcp ⇒ 装固件 · advance(2) · pc=8 · x5=3",
    "审计已生效：docker logs riscv-sim 一行一调用（client/tool/耗时/码）",
    "待做：多客户端并发压测、MCP 并进 Rust 单二进制",
], fill="#ffffff", stroke="#c3cedb", size=10.8))

svg.append('</svg>')
open("docs/architecture/mcp-view.svg", "w").write("\n".join(svg))
print("wrote docs/architecture/mcp-view.svg")
