#!/usr/bin/env python3
"""生成部署视图 SVG（精确坐标，不靠文生图猜文字）。"""
from html import escape

W, H = 1600, 1000

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
         title_fill="#1c2430", mono_lines=(), size=11.5, pad=14, title_size=13):
    out = [rect(x, y, w, h, fill, stroke, dash)]
    out.append(text(x + pad, y + 24, title, title_size, 600, title_fill))
    yy = y + 24 + 20
    for i, line in enumerate(lines):
        out.append(text(x + pad, yy, line, size, 400,
                        "#4a5666" if i else "#243043",
                        mono=(i in mono_lines)))
        yy += 18
    return "\n".join(out)

def arrow(x1, y1, x2, y2, label=None, dash=None, color="#5b6b7f", lx=None, ly=None, anchor="middle"):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    out = [f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" '
           f'stroke-width="1.6" marker-end="url(#arrow)"{d}/>']
    if label:
        out.append(text(lx if lx is not None else x, ly if ly is not None else y,
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

# Title
svg.append(text(60, 62, "部署视图 · 单容器单端口（v1.12）", 24, 700))
svg.append(text(60, 88, "实线 = 已实现并验证；虚线 = 未做 / 外部依赖。镜像构建需在有 Docker 引擎的机器上执行。", 12.5, 400, "#61708a"))

# ---------------------------------------------------------------- 浏览器
svg.append(card(60, 150, 330, 170, "用户浏览器", [
    "页面 front/（零构建 ES module）",
    "GET / · /app.js · /services/*",
    "同源探测 GET /api/version ⇒ 自动 server 模式",
    "WS 事件流：…/events/ws?token=",
    "REST：x-auth-token 头",
]) )

# ---------------------------------------------------------------- 宿主机
svg.append(rect(440, 120, 1100, 880, "#fbfcfe", "#aeb9c8", dash="10 6", sw=1.6))
svg.append(text(462, 146, "宿主机 · Docker Engine", 13.5, 700, "#3c4a5c"))

# deploy.sh
svg.append(card(470, 170, 330, 132, "部署入口", [
    "./deploy.sh            (up，带缓存)",
    "./deploy.sh rebuild    (--no-cache)",
    ".env：SIM_TOKEN（随机，chmod 600）",
    "SIM_HOST_PORT=8876 · SIM_BOARD=qemu-virt",
], fill="#f2f7ff", stroke="#b8cdf0", mono_lines=(0,))) 

# 镜像
svg.append(card(830, 170, 300, 140, "镜像（本地 tag + 已发布）", [
    "riscv-sim-server:latest（本机构建）",
    "ACR: …/zyzs/riscv_sim · v0.1.0 / sha-*",
    "rust:1-bookworm → debian:bookworm-slim",
    "只装 ca-certificates + curl · amd64 36MB",
    "本机无 Docker 引擎 ⇒ 未本机构建；由 CI 构建",
], fill="#fff7e8", stroke="#e8c98a", size=11.2))

# 端口映射
svg.append(card(1160, 170, 340, 140, "端口映射（compose 发布）", [
    "宿主机 8876  →  容器 9876",
    "默认发布在所有网卡（0.0.0.0:8876）",
    "⚠ 谁能访问 8876 就等于谁能执行内核",
], fill="#fdeeee", stroke="#e3adad", size=11.2))

# ---------------------------------------------------------------- 容器
svg.append(rect(470, 360, 1030, 470, "#ffffff", "#7c8ea6", sw=1.8))
svg.append(text(492, 388, "容器 · riscv-sim-server（USER sim，uid 10001，非 root）", 13.5, 700, "#243043"))

# entrypoint
svg.append(card(492, 404, 986, 78, "docker/entrypoint.sh", [
    "env → argv：SIM_TOKEN 必填（否则拒绝启动）；set -- 逐个追加，不做字符串拼接",
], fill="#f6f8fb", stroke="#cdd5e0", size=11.5))

# process
svg.append(card(492, 500, 986, 62, "进程 riscv_sim_server（唯一宿主）", [
    "--bind 0.0.0.0 --port 9876 --allow-remote --token *** --front-dir /app/front",
], fill="#eef3f9", stroke="#a9b9cd", mono_lines=(1,), size=11.2))

# HTTP 面
svg.append(card(492, 582, 470, 150, "HTTP 面（同一端口）", [
    "GET /<路径>  静态资源 · Cache-Control: no-store",
    "GET /healthz  公开（探针用，不校验令牌）",
    "/api/**  require_token：只认 x-auth-token",
    "  唯一例外：…/events/ws 升级可带 ?token=",
    "  （浏览器 WebSocket 不能带自定义头）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

# 静态资源
svg.append(card(492, 752, 470, 60, "内置页面 /app/front", [
    "index.html · app.js · services/* · views/* · style.css",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

# 会话线程 + 内核
svg.append(card(986, 582, 492, 150, "会话线程池（每会话一线程）", [
    "KernelHost 不是 Send ⇒ 靠线程边界隔离",
    "同一时刻一个推进者；忙 ⇒ 409 E_SESS-02",
    "作业：Job::Debug（REST） / Job::Contract（…/command）",
    "事件环 + console 环（绝对 offset）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(986, 752, 234, 60, "内核 riscv_sim_kernel", [
    "native · 同一 crate",
], fill="#eefaf1", stroke="#a8d3b6", size=11.0))
svg.append(card(1244, 752, 234, 60, "契约 crate", [
    "schema v19 · Message 信封",
], fill="#f6f1ff", stroke="#c6b6e6", size=11.0))

# 健康检查 + 卷
svg.append(card(470, 850, 320, 88, "HEALTHCHECK（镜像自带）", [
    "每 30s：curl /healthz",
    "3 次失败 ⇒ unhealthy（deploy.sh 会打印日志）",
], fill="#f6f8fb", stroke="#cdd5e0", size=11.0))
svg.append(card(810, 850, 320, 88, "卷 sim-data → /data", [
    "/data/images = --allow-image-root（白名单根）",
    "重启容器后仍在；数据卷不随 down 删除",
], fill="#f6f8fb", stroke="#cdd5e0", size=11.0))
svg.append(card(1150, 850, 350, 88, "未做 / 后续（虚线）", [
    "反向代理 + TLS + 来源限流",
    "SQLite：板 / 工程状态持久化",
], fill="#fbfbfc", stroke="#c8ced8", dash="6 4", size=11.0))

# ---------------------------------------------------------------- 连线
svg.append(arrow(390, 210, 432, 210, "HTTP/WS", lx=412, ly=228, anchor="middle"))
svg.append(arrow(800, 245, 828, 245))
svg.append(arrow(1130, 245, 1158, 245))
svg.append(arrow(1330, 320, 1330, 360, "8876 → 9876", lx=1352, ly=345, anchor="start"))
svg.append(arrow(800, 291, 800, 360, None))
svg.append(text(792, 318, "启动容器", 10.5, 500, "#5b6b7f", anchor="end"))
svg.append(arrow(970, 815, 970, 850, None, dash="5 3"))
svg.append(text(976, 836, "挂载", 10.5, 500, "#5b6b7f"))
svg.append(arrow(880, 542, 880, 582, None, dash="5 3"))
svg.append(text(886, 566, "同源", 10.5, 500, "#5b6b7f"))

# 图例
svg.append(text(60, 360, "读法", 12.5, 700))
svg.append(text(60, 384, "1. 浏览器只认一个地址：http://host:8876/?token=…", 11.5, 400, "#5a6878"))
svg.append(text(60, 404, "2. 服务端把页面与 API 一起回 → 同源，无 CORS、无第二个进程", 11.5, 400, "#5a6878"))
svg.append(text(60, 424, "3. 内核在容器里（native），命令与事件都回到这一份会话状态", 11.5, 400, "#5a6878"))

svg.append('</svg>')
open("docs/architecture/deploy-view.svg", "w", encoding="utf-8").write("\n".join(svg))
print("wrote docs/architecture/deploy-view.svg")
