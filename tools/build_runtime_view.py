#!/usr/bin/env python3
"""生成运行视图 SVG（精确坐标；文字不靠模型猜，全部来自仓里的事实）。

    python3 tools/build_runtime_view.py      # → docs/architecture/runtime-view.svg

渲染 PNG（Chrome headless）：

    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless \
      --screenshot=docs/architecture/runtime-view.png --window-size=1700,1160 \
      "file://$PWD/docs/architecture/runtime-view.svg"
"""
from html import escape

W, H = 1700, 1160


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
svg.append(text(40, 58, "运行视图 · 一份内核，两种跑法", 25, 700))
svg.append(text(40, 86, "实线 = 已实现并验证；虚线 = 未做 / 外部依赖。左右两侧共享同一个内核 crate 与同一份契约，区别只在「内核跑在谁的进程里」。",
                12.5, 400, "#61708a"))
svg.append(text(W - 40, 58, "front/ · crates/{kernel,contract,server}", 11.5, 400, "#8a97a8", anchor="end", mono=True))

# ------------------------------------------------------------------ 浏览器面板
PX_L, PY, PW, PH = 40, 118, 640, 690
svg.append(rect(PX_L, PY, PW, PH, "#ffffff", "#a9b9cd", sw=1.8))
svg.append(text(PX_L + 20, PY + 28, "浏览器 · 前端 front/（零构建 ES module）", 14.5, 700, "#243043"))

CX = PX_L + 20
CW = PW - 40

svg.append(card(CX, 160, CW, 98, "视图层 · views/", [
    "m01 工作台 · m02 视图组件 · m03 视图态（8 个视图，注册表驱动）",
    "产品 / 嵌入式 / 调试 / 寄存器·内存 / 波形 / 终端 / 源码 / 板级内存图",
    "渲染是纯函数式的：视图状态进 store，组件只读",
], fill="#eef3f9", stroke="#a9b9cd", size=11.2))

svg.append(card(CX, 272, CW, 152, "服务层 · services/", [
    "m04 会话 · m05 命令路由（契约 Message 的唯一出口）",
    "m07 传输（两档适配器）· m07_servermode 同源探测",
    "m07_servercommand（命令 POST）· m10 寄存器/内存 · m15 设备",
    "m23 板库（IndexedDB）· m76 模板",
    "⚠ 服务层不认识「PC / 寄存器」语义，只搬 Message",
    "⇒ 换内核不动前端，换前端不动内核",
], fill="#eef3f9", stroke="#a9b9cd", size=11.2))

svg.append(card(CX, 438, CW, 116, "状态与持久化", [
    "视图态：内存里的 store（切视图/改筛选即时生效）",
    "板档案：IndexedDB「riscv-sim-board-store」（跨刷新/跨重启留着）",
    "工程与固件字节：内存 ⇒ 刷新即失（源码树也只在前端）",
    "⇒ 所以「重启浏览器回来」只回来板，不回来固件",
], fill="#fbfcfe", stroke="#cdd5e0", size=11.2))

svg.append(card(CX, 568, CW, 98, "m07_transport · 选哪一档", [
    "mountTransport()：显式 ?server= ⇒ 档 B；否则同源 GET /api/version 探测",
    "档 A（离线）：module Worker + WASM 内核 ｜ 档 B（服务端）：WebSocket + REST",
    "两档对外是同一组 API（sendCommand / onMessage / onStatus）",
], fill="#eef3f9", stroke="#a9b9cd", size=11.2))

svg.append(card(CX, 680, CW, 108, "档 A 的落点 · kernel-worker.js", [
    "new Worker('./kernel-worker.js', { type: 'module' })",
    "instantiate → front/assets/pkg/riscv_sim_kernel_bg.wasm（wasm-bindgen 胶水）",
    "WASM 与 native 是同一份源码（crates/kernel，wasm32-unknown-unknown）",
], fill="#fff7e8", stroke="#e8c98a", mono_lines=(1,), size=11.0))

svg.append(arrow(CX + CW // 2, 666, CX + CW // 2, 680, color="#b98a3c"))

# ------------------------------------------------------------------ 服务端面板
PX_R, PW_R = 1020, 640
svg.append(rect(PX_R, PY, PW_R, PH, "#ffffff", "#7c8ea6", sw=1.8))
svg.append(text(PX_R + 20, PY + 28, "服务进程 riscv_sim_server（可选形态）", 14.5, 700, "#243043"))

RX = PX_R + 20
RW = PW_R - 40

svg.append(card(RX, 160, RW, 134, "HTTP 面 · axum router", [
    "GET /<路径> 静态 front（--front-dir，一律 no-store）",
    "GET /api/version · GET /healthz 公开（同源探测 / 容器探针）",
    "/api/** require_token：只认 x-auth-token",
    "唯一例外：…/events/ws 可带 ?token=（浏览器 WS 不能带自定义头）",
    "无板装载 ⇒ 409 E_PLT-22（「没有生效板」不是建会话失败）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(RX, 308, RW, 98, "会话注册表 · SessionRegistry", [
    "配额 SIM_MAX_SESSIONS=8 · TTL SIM_SESSION_TTL=900s（到期回收）",
    "一个会话 = 一条专属线程 + 一个内核实例 + 一个事件环",
    "GET /api/sessions 回 {quota:{max,used}, sessions[...]}",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(RX, 420, RW, 116, "会话线程 · Job", [
    "Job::Debug（REST 老面：run/step/pause/reset/console…）",
    "Job::Contract（契约面：POST …/command 吃整条 Message）",
    "单推进者：忙时立即 409 E_SESS-02，不排队（延迟才可预测）",
    "run(prefix, until, t_ps) · snapshot / restore（进程内）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(RX, 550, RW, 98, "内核 riscv_sim_kernel（native）", [
    "同一 crate 的另一种编译目标（不是另一套实现）",
    "CLINT / PLIC / UART / GPIO / Timer 的行为级等价模型在进程内",
    "执行预算窗口 10µs ⇒ 让出线程，HTTP 不被一条 run 卡死",
], fill="#eef7ee", stroke="#a9c8a9", size=11.0))

svg.append(card(RX, 662, RW, 126, "事件环 · EventRing", [
    "events：定容 VecDeque，seq 从 0 单调递增、按键去重的基础",
    "应答首帧：seq 从 2^40 起、不进环 ⇒ 与 WS 环永不撞号",
    "console 环 / trace 环各自定容，溢出丢最旧（面板会标「不是完整历史」）",
    "取用途径：WS 推流（持续）/ GET …/events（从 since 拉补）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

# ------------------------------------------------------------------ 中间的连线
svg.append(arrow(684, 470, 1016, 470, color="#3f6fa8"))
svg.append(text(850, 458, "① 命令（前端 → 服务端）", 11.2, 600, "#3f6fa8", anchor="middle"))
svg.append(text(850, 492, "POST …/run|step|command · x-auth-token", 10.0, 400, "#5b6b7f", anchor="middle", mono=True))

svg.append(arrow(1016, 600, 684, 600, color="#8a6b3c"))
svg.append(text(850, 588, "② 事件（服务端 → 前端）", 11.2, 600, "#8a6b3c", anchor="middle"))
svg.append(text(850, 622, "WS …/events/ws?token= · 帧 {seq, sessionId}", 10.0, 400, "#5b6b7f", anchor="middle", mono=True))

svg.append(card(700, 240, 300, 128, "档位怎么定", [
    "1) URL 里有 ?server= ⇒ 档 B",
    "2) 有 ?token= 且同源 ⇒ 档 B",
    "3) 否则 GET /api/version 真回版本三件套 ⇒ 档 B",
    "4) 探测失败 / 超时 / 畸形 ⇒ 档 A（不抛错）",
], fill="#fbfbfc", stroke="#c8ced8", dash="6 4", size=10.8, title_size=12))

svg.append(text(850, 400, "同一个页面，两种身份", 11.5, 600, "#61708a", anchor="middle"))
svg.append(text(850, 420, "档 A 时它是「带内核的本机」；档 B 时它只是「遥控器」", 10.5, 400, "#7b8798", anchor="middle"))

# ------------------------------------------------------------------ 契约带
svg.append(rect(40, 830, 1620, 108, "#f6f3fd", "#b9a8dd", sw=1.8))
svg.append(text(60, 858, "唯一接缝 · crates/contract（schema v19）", 14.5, 700, "#3f2f66"))
svg.append(text(60, 882, "Message 信封：kind + reqId + payload ⇒ 命令（前端→内核）/ 事件（内核→前端）/ 应答（reqId 关联回原命令）", 11.4, 400, "#4a3f66"))
svg.append(text(60, 902, "同一份事件只有一条路：首帧走 HTTP 回体（seq ≥ 2^40），后续帧进环走 WS ⇒ 不重复投递、也不丢", 11.4, 400, "#4a3f66"))
svg.append(text(60, 922, "内核不认识 HTTP / JSON 路由；宿主不认识「PC、寄存器」的语义 ⇒ 契约是两者之间唯一的约定，也是升版时唯一要谈的地方", 11.4, 400, "#4a3f66"))

# ------------------------------------------------------------------ 底部三块
svg.append(card(40, 960, 520, 126, "一、时间与推进（内核侧）", [
    "run(prefix, until, t_ps)：跑到某条件 / 某时刻 / 跑满预算",
    "停止原因：halt · steps · budget（预算跑满 = 没等到那一行）",
    "单推进者 + 10µs 窗口 ⇒ 长 run 也不会把 HTTP / 其它会话饿死",
    "reset = 重新装载当前工程（不是换回内置 demo）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(590, 960, 520, 126, "二、数据落在哪", [
    "板档案 → 浏览器 IndexedDB（服务端没有它的一份）",
    "固件/镜像字节 → 前端内存（刷新即失）；容器里是 /data 卷 + 白名单根",
    "事件/轨迹 → 进程内定容环（重启即失，不落盘）",
    "服务端 SQLite 持久化 ⇒ 未做（下一次发布才谈）",
], fill="#ffffff", stroke="#c3cedb", size=11.0))

svg.append(card(1140, 960, 520, 126, "三、还没做 / 如实登记", [
    "令牌 → 角色/配额：没做 ⇒ 现在放公网只靠「一条共享令牌」",
    "跨进程快照恢复：够不着（只支持同进程内 restore）",
    "工程与板的服务端持久化：未做 ⇒ 容器重启不记得你的工程",
    "WASM 内核默认不进镜像 ⇒ 容器里只有档 B（server 模式）",
], fill="#fbfbfc", stroke="#c8ced8", dash="6 4", size=11.0))

svg.append('</svg>')
open("docs/architecture/runtime-view.svg", "w").write("\n".join(svg))
print("wrote docs/architecture/runtime-view.svg")
