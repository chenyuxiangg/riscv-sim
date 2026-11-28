#!/usr/bin/env python3
"""sim-mcp —— 把 riscv_sim 的 HTTP 面收成 11 个 MCP 工具（只依赖标准库）。

设计口径（与 docs/architecture/mcp-view.svg 对齐）：
  1. 凭据：客户端持**自己的令牌**（服务端账号库里那把）；MCP 原样转给服务端，
     服务端是唯一的判据。`SIM_TOKEN` 只作 MCP→服务端的应急凭据与兼容模式。
  2. 会话归属：`client_id → session_id` 一张表，boot 只建自己的，永不 list 别人的。
  3. 破坏性工具默认不注册；`SIM_MCP_ALLOW_DESTRUCTIVE=1` 才出现。
  4. 审计一行一调用（client / tool / 耗时 / 错误码）。

传输：MCP Streamable HTTP 的 JSON 子集（POST /mcp，回 application/json）。
  - 不做服务端 SSE 推送（GET /mcp 回 405）⇒ 事件靠 `events(since_seq)` 拉。
  - 支持 `Authorization: Bearer <token>`，也支持退化形态 `/mcp/<token>`。
  - **文件上行**：`POST /images`（原始字节 + `x-image-name`，同一令牌）。ELF 不进
    工具参数 —— base64 上百 KB 会把上下文烧光，字节流走 HTTP 才是零成本。

启动（容器内）：
    SIM_BASE=http://host.docker.internal:8876 \
    SIM_TOKEN=<server 令牌> SIM_MCP_TOKEN=<给客户端的那条> \
    python3 server.py
"""
from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import unquote, urlparse

# --------------------------------------------------------------------------- 配置
SIM_BASE = os.environ.get("SIM_BASE", "http://127.0.0.1:8876").rstrip("/")
SIM_TOKEN = os.environ.get("SIM_TOKEN", "").strip()
MCP_TOKEN = os.environ.get("SIM_MCP_TOKEN", "").strip()
ALLOW_DESTRUCTIVE = os.environ.get("SIM_MCP_ALLOW_DESTRUCTIVE", "0") == "1"
ALLOW_REMOTE = os.environ.get("SIM_MCP_ALLOW_REMOTE", "0") == "1"
HOST = os.environ.get("SIM_MCP_HOST", "127.0.0.1")
PORT = int(os.environ.get("SIM_MCP_PORT", "8877"))
IDLE_SECS = int(os.environ.get("SIM_MCP_IDLE_SECS", "300"))
MAX_RUN_MS = int(os.environ.get("SIM_MCP_MAX_RUN_MS", "5000"))
MAX_STEPS = int(os.environ.get("SIM_MCP_MAX_STEPS", "100000"))
# 文件上传的字节上限（客户端推 ELF 走 HTTP，不是 MCP 参数 —— 一条 base64 参数
# 会把上下文烧光：100 KB 的 ELF 编成 133 K 个字符）。默认跟服务端一样 16 MiB。
MAX_UPLOAD = int(os.environ.get("SIM_MCP_MAX_UPLOAD", str(16 * 1024 * 1024)))
PROTOCOL = os.environ.get("SIM_MCP_PROTOCOL", "2025-06-18")


def die(msg: str) -> None:
    print(f"sim-mcp: {msg}", file=sys.stderr)
    raise SystemExit(1)


if not SIM_TOKEN:
    die("SIM_TOKEN 没设：本进程要拿它访问仿真器，客户端不该看见它")
if MCP_TOKEN and MCP_TOKEN == SIM_TOKEN:
    die("SIM_MCP_TOKEN 与 SIM_TOKEN 相同：两层凭据退化成一层，配置错了")
if HOST not in ("127.0.0.1", "localhost", "::1") and not ALLOW_REMOTE:
    die(f"拒绝绑定 {HOST}：默认只回环（要对外请显式 SIM_MCP_ALLOW_REMOTE=1）")

# --------------------------------------------------------------------------- 仿真器客户端


class SimError(Exception):
    def __init__(self, code: str, detail: str, status: int):
        self.code, self.detail, self.status = code, detail, status
        super().__init__(f"{code}: {detail}")


# 每个请求线程「当前是谁」的令牌。工具函数保持 `(client, args)` 签名不变，
# 真正发 HTTP 时从这里取 —— 这样几十个 `t_*` 一个都不用改。
_TOKEN_CTX = threading.local()


def _current_token() -> str:
    return getattr(_TOKEN_CTX, "token", "") or SIM_TOKEN


def _http(method: str, path: str, *, body=None, raw=None, ctype=None, timeout=20.0,
          extra_headers: dict | None = None, token: str | None = None):
    url = SIM_BASE + path
    data = raw
    headers = {"x-auth-token": token or _current_token()}
    if extra_headers:
        headers.update(extra_headers)
    if body is not None:
        data = json.dumps(body).encode()
        headers["content-type"] = "application/json"
    elif raw is not None:
        headers["content-type"] = ctype or "application/octet-stream"
    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, _decode(resp.read()), dict(resp.headers)
    except urllib.error.HTTPError as err:
        payload = _decode(err.read())
        # run 撞墙钟会回 504，但 body 里仍是完整停点报告 ⇒ 上层当结果用
        if err.code == 504 and isinstance(payload, dict) and "stop_reason" in payload:
            return 504, payload, dict(err.headers)
        code = payload.get("code") if isinstance(payload, dict) else None
        detail = payload.get("detail") if isinstance(payload, dict) else None
        raise SimError(code or f"E_HTTP-{err.code}", detail or str(payload), err.code)
    except urllib.error.URLError as err:
        raise SimError("E_MCP-01", f"仿真器不可达（{SIM_BASE}）：{err.reason}", 502)


def _decode(blob: bytes):
    if not blob:
        return {}
    try:
        return json.loads(blob)
    except json.JSONDecodeError:
        return {"raw": blob.decode("utf-8", "replace")}


# --------------------------------------------------------------------------- 契约命令通道
# ⚠️ `SetStopPolicy` 是**契约命令**（不是 REST 资源），走 `POST …/command`：整条
#    `Message` 进内核，服务端不翻译。信封的 `schema_version` 从 `/api/version`
#    读 ⇒ ⇒ MCP 里**不**留第二份硬编码版本号。
_SCHEMA_LOCK = threading.Lock()
_SCHEMA_CACHE: dict = {}


def _schema_version() -> int:
    with _SCHEMA_LOCK:
        cur = _SCHEMA_CACHE.get("v")
    if cur:
        return cur
    _, body, _ = _http("GET", "/api/version")
    v = int(body.get("schema_version") or 0)
    if v <= 0:
        raise SimError("E_MCP-10", "/api/version 没给 schema_version", 502)
    with _SCHEMA_LOCK:
        _SCHEMA_CACHE["v"] = v
    return v


def _frames_events(frames: list):
    """把应答帧剥到 `(变体名, 载荷)`；不懂的帧跳过。"""
    for frame in frames or []:
        inner = (frame.get("payload") or {}).get("payload") or {}
        if isinstance(inner, dict) and len(inner) == 1 and next(iter(inner)) in ("Event", "Command"):
            inner = next(iter(inner.values())) or {}
        if not isinstance(inner, dict) or len(inner) != 1:
            continue
        kind, value = next(iter(inner.items()))
        yield kind, value


def _contract_command(sid: str, payload: dict, req_id: int = 1) -> dict:
    """发一条契约命令；内核回的 `Error` 事件**当异常抛**（不默默吞掉）。"""
    msg = {
        "schema_version": _schema_version(),
        "direction": "Command",
        "seq": req_id,
        "payload": {"Command": payload},
    }
    _, body, _ = _http("POST", f"/api/sessions/{sid}/command",
                       body={"message": msg, "reqId": req_id})
    frames = body.get("frames") or []
    for kind, value in _frames_events(frames):
        if kind == "Error":
            code = (value or {}).get("code") or "E_MCP-10"
            detail = (value or {}).get("message") or "内核拒绝了这条命令"
            raise SimError(code, detail, 400)
    return body


# --------------------------------------------------------------------------- 会话归属
ABI = ["zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1",
       "a2", "a3", "a4", "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7",
       "s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6"]


class Ownership:
    """client_id → 会话。只建自己的、只删自己的，永不 list 全池。"""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._map: dict[str, dict] = {}

    def boot(self, client: str, board: str | None, token: str | None = None) -> dict:
        # ⚠️ 令牌随会话一起记着：TTL 回收时**必须用这把会话自己的令牌**去删，
        #    否则 MCP 的 sweeper 会拿 SIM_TOKEN（`ops`）去删别人的会话 ⇒ 404
        #    ⇒ 会话一直占着配额直到服务端 TTL。
        token = token or _current_token()
        with self._lock:
            cur = self._map.get(client)
        if cur:
            status, state, _ = _http("GET", f"/api/sessions/{cur['sid']}/state")
            if status == 200:
                if board and board != cur["board"]:
                    # 板是建会话时定死的，改不了 ⇒ 换板只能重建。只动自己的会话。
                    # 不这么做的话：首次 boot 没给 board ⇒ 会话永远没板，load_image
                    # 一直回 E_PLT-22，而 boot 一直「复用成功」，看着像工具坏了。
                    print(json.dumps({"audit": "rebind", "from": cur["board"],
                                      "to": board}), flush=True)
                    self._drop(cur)
                    with self._lock:
                        self._map.pop(client, None)
                    cur = None
                else:
                    cur["used"] = time.time()
                    return {"session_id": cur["sid"], "board": cur["board"], "reused": True}
            else:
                with self._lock:
                    self._map.pop(client, None)
        body = {"board": board} if board else {}
        status, created, _ = _http("POST", "/api/sessions", body=body)
        entry = {"sid": created["session_id"], "board": created.get("board"),
                 "used": time.time(), "token": token}
        with self._lock:
            old = self._map.get(client)
            self._map[client] = entry
        if old and old["sid"] != entry["sid"]:
            self._drop(old)
        return {"session_id": entry["sid"], "board": entry["board"], "reused": False}

    def sid(self, client: str) -> str:
        with self._lock:
            entry = self._map.get(client)
        if not entry:
            raise SimError("E_MCP-02", "这个客户端还没有会话：先调 boot", 409)
        entry["used"] = time.time()
        return entry["sid"]

    def _drop(self, entry: dict) -> None:
        try:
            _http("DELETE", f"/api/sessions/{entry['sid']}", token=entry.get("token"))
        except SimError:
            pass

    def sweep(self) -> None:
        now = time.time()
        with self._lock:
            stale = [(k, v) for k, v in self._map.items() if now - v["used"] > IDLE_SECS]
            for key, _ in stale:
                self._map.pop(key, None)
        for _, entry in stale:
            print(json.dumps({"audit": "idle-drop", "session": entry["sid"]}), flush=True)
            self._drop(entry)


OWNER = Ownership()


def _sweeper() -> None:
    while True:
        time.sleep(30)
        try:
            OWNER.sweep()
        except Exception as err:  # noqa: BLE001 - 清理线程不许把进程带走
            print(json.dumps({"audit": "sweep-error", "detail": str(err)}), flush=True)


# --------------------------------------------------------------------------- 工具实现


def _regs_text(regs) -> str:
    if not isinstance(regs, list):
        return ""
    shown = [f"x{i}({ABI[i]})={v}" for i, v in enumerate(regs[:32]) if v]
    return " ".join(shown)


def t_boot(client: str, args: dict) -> dict:
    board = args.get("board") or None
    info = OWNER.boot(client, board, _current_token())
    verb = "复用" if info["reused"] else "新建"
    return {
        "text": f"{verb}会话 {info['session_id']}（板 {info['board']}）",
        "data": info,
    }


def t_load_image(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    name = args.get("name")
    path = args.get("path")
    if path:
        # ⚠️ 给了 path 就别替客户端编名字：服务端会按文件名推 name
        #    （`osz_red.elf` 报成 `fw.bin` ⇒ 摘要和落盘名都对不上）。
        body: dict = {"path": path}
        if name:
            body["name"] = name
        status, body, _ = _http("POST", f"/api/sessions/{sid}/image", body=body)
    else:
        name = name or "fw.bin"
        blob_hex = (args.get("hex") or "").replace("0x", "").replace(" ", "").replace("\n", "")
        blob_b64 = (args.get("b64") or "").strip()
        if not blob_hex and not blob_b64:
            raise SimError("E_MCP-03",
                           "三选一：hex（原始字节的十六进制）/ b64（小文件，<20KB）/ path", 400)
        try:
            blob = (base64.b64decode(blob_b64, validate=False) if blob_b64
                    else bytes.fromhex(blob_hex))
        except (ValueError, base64.binascii.Error) as err:
            raise SimError("E_MCP-03", f"hex/b64 解不开：{err}", 400)
        status, body, _ = _http("POST", f"/api/sessions/{sid}/image", raw=blob,
                                ctype="application/octet-stream",
                                extra_headers={"x-image-name": name})
    seg = ", ".join(f"[{s['base']:#x}+{s['size']}]" for s in body.get("segments", []))
    return {
        "text": f"装载 {body.get('name')}（{body.get('kind')}）entry={body.get('entry', 0):#x} 段 {seg}",
        "data": body,
    }


def t_advance(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    n = int(args.get("n", 1))
    if n < 1 or n > MAX_STEPS:
        raise SimError("E_MCP-04", f"n 必须在 1..{MAX_STEPS}", 400)
    status, body, _ = _http("POST", f"/api/sessions/{sid}/step", body={"n": n}, timeout=30)
    return {
        "text": (f"执行 {body.get('executed')} 条 · pc={body.get('pc')} · "
                 f"t_ps={body.get('t_ps')} · 停因 {body.get('stop_reason')}"),
        "data": body,
    }


def t_run(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    req: dict = {}
    if args.get("steps") is not None:
        req["steps"] = int(args["steps"])
    if args.get("timeout_ms") is not None:
        req["timeout_ms"] = min(int(args["timeout_ms"]), MAX_RUN_MS)
    if args.get("prefix"):
        req["prefix"] = args["prefix"]
    if args.get("until"):
        req["until"] = args["until"]
    if not req:
        req["timeout_ms"] = MAX_RUN_MS
    background = bool(args.get("background"))
    if background:
        req["run_async"] = True
        status, body, _ = _http("POST", f"/api/sessions/{sid}/run", body=req, timeout=15)
        job = body.get("job") or body.get("job_id") or body.get("id")
        return {"text": f"已提交异步作业 {job}（之后用 poll_job 取结果）", "data": body}
    status, body, _ = _http("POST", f"/api/sessions/{sid}/run", body=req, timeout=MAX_RUN_MS / 1000 + 15)
    note = "（撞墙钟已熔断，但停点信息如下）" if status == 504 else ""
    # 平台主动中止（E_ENG-03 无进展 / E_ENG-04 异常连环）⇒ 回体带
    # `stop_reason="error"` + `error_code`：这不是 guest 停点，必须显式说清。
    error = body.get("error_code")
    tail = (f"；内核报错 {error}（平台中止，不是 guest 停）"
            if error else "")
    return {
        "text": (f"{note}执行 {body.get('executed')} 条 · pc={body.get('pc')} · "
                 f"t_ps={body.get('t_ps')} · 停因 {body.get('stop_reason')}{tail}"),
        "data": body,
    }


def t_stop_policy(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    if args.get("stop_on_trap") is None:
        raise SimError("E_MCP-07",
                       "要显式给 stop_on_trap（true=陷阱交付即停；false=继续跑，但要配额）", 400)
    stop_on_trap = bool(args["stop_on_trap"])
    quota = args.get("max_trap_events")
    stall_guard = args.get("stall_guard")
    if stall_guard is not None:
        stall_guard = int(stall_guard)
        if stall_guard < 2:
            raise SimError("E_MCP-07", "stall_guard 必须 >= 2（1 等于「每次都算连环」）", 400)
        if stop_on_trap:
            raise SimError("E_MCP-07",
                           "stall_guard 只在 stop_on_trap=false 时有用"
                           "（true 时第一个陷阱就停了，没有连环可数）", 400)
    if quota is not None:
        quota = int(quota)
        if quota < 1:
            raise SimError("E_MCP-07", "max_trap_events 必须 >= 1", 400)
    if not stop_on_trap and quota is None:
        raise SimError("E_MCP-07",
                       "stop_on_trap=false 必须同时给 max_trap_events"
                       "（否则 handler 死循环会烧穿预算）", 400)
    body = _contract_command(sid, {"SetStopPolicy": {
        "stop_on_trap": stop_on_trap,
        "max_trap_events": quota,
        "stall_guard": stall_guard,
    }})
    if stop_on_trap:
        text = "停止策略：陷阱**交付即停**（默认 · 与改动前逐字节一致）"
    else:
        text = (f"停止策略：陷阱**不**停，最多跑 {quota} 个后停"
                "（只算同步异常；中断不进这条逻辑）")
        if stall_guard is not None:
            text += (f"；连环守卫：同一 (cause, mepc) 连陷 {stall_guard} 次"
                     "即报 E_ENG-04 中止这一批"
                     "（跨窗口累计 ⇒ 比配额早得多，也更准）")
    return {
        "text": text + " · 会话级粘性：换工程 / 卸载回默认",
        "data": {"stop_on_trap": stop_on_trap, "max_trap_events": quota,
                 "stall_guard": stall_guard, "frames": body.get("frames")},
    }


def t_poll_job(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    job = args.get("job")
    if not job:
        raise SimError("E_MCP-05", "缺 job：run(background=true) 的返回值里有它", 400)
    status, body, _ = _http("GET", f"/api/sessions/{sid}/jobs/{job}")
    return {"text": f"作业 {job}：{json.dumps(body, ensure_ascii=False)[:300]}", "data": body}


def t_inspect(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    kind = args.get("kind") or "state"
    if kind == "state":
        _, body, _ = _http("GET", f"/api/sessions/{sid}/state")
        return {
            "text": f"pc={body.get('pc')} t_ps={body.get('t_ps')} | {_regs_text(body.get('registers'))}",
            "data": body,
        }
    if kind == "console":
        offset = int(args.get("offset", 0))
        limit = int(args.get("limit", 1024))
        status, body, _ = _http("GET", f"/api/sessions/{sid}/console?offset={offset}&limit={limit}")
        blob = body.get("bytes") or []
        text = bytes(blob).decode("utf-8", "replace") if isinstance(blob, list) else str(blob)
        return {"text": f"console[{body.get('base', offset)}+{len(blob)}] {text!r}", "data": body}
    if kind == "memory":
        addr = args.get("addr")
        if addr is None:
            raise SimError("E_MCP-06", "memory 要给 addr", 400)
        length = int(args.get("len", 64))
        fmt = args.get("format")
        query = f"addr={int(addr)}&len={length}" + (f"&format={fmt}" if fmt else "")
        _, body, _ = _http("GET", f"/api/sessions/{sid}/memory?{query}")
        blob = bytes(body.get("bytes") or [])
        lines = [f"{int(addr) + i:#010x}  " + " ".join(f"{b:02x}" for b in blob[i:i + 8])
                 for i in range(0, len(blob), 8)]
        return {"text": "\n".join(lines) or "（空）", "data": body}
    raise SimError("E_MCP-06", f"未知 kind {kind!r}；可用 state|console|memory", 400)


def t_disasm(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    addr = args.get("addr")
    if addr is None:
        raise SimError("E_MCP-06", "disasm 要给 addr", 400)
    count = int(args.get("count", 8))
    _, body, _ = _http("GET", f"/api/sessions/{sid}/disasm?addr={int(addr)}&count={count}")
    items = body.get("instructions") or body.get("items") or []
    lines = [f"{it.get('addr', 0):#010x}: {it.get('text') or it.get('asm') or it}" for it in items]
    return {"text": "\n".join(lines) or json.dumps(body, ensure_ascii=False)[:300], "data": body}


def t_breakpoints(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    op = args.get("op") or "list"
    base = f"/api/sessions/{sid}/breakpoints"
    if op == "list":
        _, body, _ = _http("GET", base)
    elif op == "set":
        req = {}
        if args.get("addr") is not None:
            req["addr"] = int(args["addr"])
        elif args.get("symbol"):
            req["symbol"] = args["symbol"]
        elif args.get("condition"):
            req["condition"] = args["condition"]
        else:
            raise SimError("E_MCP-07", "set 要给 addr / symbol / condition 之一", 400)
        _, body, _ = _http("POST", base, body=req)
    elif op == "clear":
        if args.get("id") is None:
            raise SimError("E_MCP-07", "clear 要给 id（断点表里的 id）", 400)
        _, body, _ = _http("DELETE", f"{base}/{int(args['id'])}")
    elif op == "enable":
        if args.get("id") is None:
            raise SimError("E_MCP-07", "enable 要给 id 和 enabled", 400)
        _, body, _ = _http("PUT", f"{base}/{int(args['id'])}/enabled",
                           body={"enabled": bool(args.get("enabled", True))})
    else:
        raise SimError("E_MCP-07", f"未知 op {op!r}；可用 list|set|clear|enable", 400)
    table = body.get("breakpoints", body)
    return {"text": f"断点表（{len(table) if isinstance(table, list) else '?'} 条）："
                    f"{json.dumps(table, ensure_ascii=False)[:300]}", "data": body}


def _compact_frame(frame: dict) -> dict:
    """事件帧压成「够 agent 判断」的形状。

    ⚠️ 原始帧每个 SimStateDelta 都带 32 个寄存器 ⇒ 64 条就能把上下文吃光。
    这里只留 seq / 种类 / 少量关键字段；要看细节就再调 inspect。
    """
    payload = frame.get("payload") or {}
    inner = payload.get("payload") or {}
    # 契约帧多包一层方向：{"Event": {"SimStateDelta": ...}} ⇒ 剥掉再判种类
    if isinstance(inner, dict) and len(inner) == 1 and next(iter(inner)) in ("Event", "Command"):
        inner = next(iter(inner.values())) or {}
    kind, value = next(iter(inner.items())) if isinstance(inner, dict) and inner else ("?", None)
    out = {"seq": frame.get("seq"), "kind": kind}
    if isinstance(value, dict):
        if kind == "SimStateDelta":
            out.update({k: value.get(k) for k in ("pc", "t_ps") if k in value})
            changed = [r for r in value.get("registers", []) if r.get("value")]
            out["regs"] = {f"x{r['index']}": r["value"] for r in changed[:8]}
        elif "bytes" in value:
            out["bytes"] = len(value["bytes"])
            text = value.get("text")
            if text:
                out["text"] = text[:120]
        else:
            out["brief"] = json.dumps(value, ensure_ascii=False)[:160]
    return out


def t_events(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    since = int(args.get("since", 0))
    limit = int(args.get("limit", 64))
    _, body, _ = _http("GET", f"/api/sessions/{sid}/events?since={since}&limit={limit}")
    frames = body.get("frames") or []
    nxt = body.get("next") or (frames[-1].get("seq", since) + 1 if frames else since)
    compact = [_compact_frame(f) for f in frames]
    kinds = sorted({c["kind"] for c in compact})
    return {
        "text": (f"{len(compact)} 条事件（since {since}，下次 since={nxt}）"
                 + (f"：{kinds}" if kinds else "（空环）")),
        "data": {"next": nxt, "frames": compact},
    }


def t_destructive_reset(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    _, body, _ = _http("POST", f"/api/sessions/{sid}/reset")
    return {"text": f"已复位到最近一次装载点：{json.dumps(body, ensure_ascii=False)[:200]}",
            "data": body}


def t_drop_session(client: str, args: dict) -> dict:
    sid = OWNER.sid(client)
    _http("DELETE", f"/api/sessions/{sid}")
    with OWNER._lock:
        OWNER._map.pop(client, None)
    return {"text": f"已删除自己的会话 {sid}（不影响别人的）", "data": {"session_id": sid}}


def t_close_session(client: str, args: dict) -> dict:
    """主动释放本客户端自己的会话（**幂等**）：跑完就还配额，不必等 TTL。

    ⚠️ 只动 `OWNER` 里本客户端映射的那一条 —— 浏览器页面 / 别的令牌开的会话
       **不碰**（同一账号共用一个配额池，闭掉别人的等于把对方正在看的页面砸掉）。
    """
    with OWNER._lock:
        entry = OWNER._map.get(client)
    if not entry:
        # 没有映射（还没 boot，或 MCP 进程重启丢了内存态）⇒ 什么都不删，
        # 但把「已经没什么可释放」说清楚，调用方不必把它当失败重试。
        return {"text": "这个客户端当前没有在跟踪的会话（无需释放）",
                "data": {"session_id": None, "closed": False}}
    sid = entry["sid"]
    try:
        status, _, _ = _http("DELETE", f"/api/sessions/{sid}", token=entry.get("token"))
    except SimError as err:
        # 404 = 服务端已经没了（TTL 回收 / 别人删了）⇒ 与「刚删掉」等价，按幂等成功回。
        if err.status != 404:
            raise
        status = 404
    with OWNER._lock:
        OWNER._map.pop(client, None)
    gone = status in (200, 202, 204, 404)
    return {
        "text": (f"已释放会话 {sid}，配额立即可用" if status != 404
                 else f"会话 {sid} 已经不存在（TTL 回收或已关闭），无需再释放"),
        "data": {"session_id": sid, "closed": gone, "status": status},
    }


# --------------------------------------------------------------------------- 工具清单
TOOLS = [
    {
        "name": "boot",
        "description": ("建立/复用本客户端自己的仿真会话（幂等）。返回 session_id 与板名。"
                        "每个客户端最多 1 个会话；空闲回收：MCP 侧 5 分钟 sweeper、"
                        "服务端 15 分钟 TTL。跑完想立刻还配额就调 close_session。"),
        "inputSchema": {
            "type": "object",
            "properties": {"board": {"type": "string",
                                     "description": "板模板 id：qemu-virt（默认）/ default / minimal"}},
        },
        "fn": t_boot,
    },
    {
        "name": "load_image",
        "description": ("装载固件：hex（**原始字节**的十六进制，不是 Intel HEX 文件）/ "
                        "b64（小文件，几十 KB 以内；大 ELF 走 POST /images 上行）/ "
                        "path（服务器白名单）。8 字节最小固件即可跑。"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "hex": {"type": "string", "description": "原始字节的十六进制，如 9302200093821200"},
                "path": {"type": "string", "description": "服务器侧白名单路径（/data/images 下）"},
                "b64": {"type": "string", "description": "文件字节的 base64（小文件用；大文件走 POST /images）"},
                "name": {"type": "string", "description": "镜像名，决定 kind；.bin=RawHex，.elf=ELF"},
            },
        },
        "fn": t_load_image,
    },
    {
        "name": "advance",
        "description": ("恰好执行 n 条指令（没有墙钟）。返回 executed / pc / t_ps / stop_reason。"
                        "会话忙时仿真器回 E_SESS-02（409），等 200ms 重试，不要重建会话。"),
        "inputSchema": {
            "type": "object",
            "properties": {"n": {"type": "integer", "minimum": 1}},
            "required": ["n"],
        },
        "fn": t_advance,
    },
    {
        "name": "run",
        "description": (f"跑到停点：steps 或 timeout_ms（默认且上限 {MAX_RUN_MS}ms，粗超会被夹）。"
                        "长任务用 background=true 拿 job，再用 poll_job；否则会撞墙钟。"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "steps": {"type": "integer"},
                "timeout_ms": {"type": "integer"},
                "prefix": {"type": "string", "description": "console 出现该字节串即停"},
                "until": {"type": "string", "description": "breakpoint | halt"},
                "background": {"type": "boolean", "description": "true ⇒ 返回 job，不阻塞"},
            },
        },
        "fn": t_run,
    },
    {
        "name": "stop_policy",
        "description": ("会话级陷阱交付策略（R-1）＋异常连环守卫（B-2）："
                        "stop_on_trap=true 交付即停（默认），"
                        "false 则陷阱继续跑、按 max_trap_events 配额停（**必须**同时给配额）。"
                        "可选 stall_guard=N：同一 (cause, mepc) 连续陷 N 次就报 E_ENG-04 中止"
                        "（跨窗口累计；handler 没让程序走出故障点 ⇒ 比配额早停、且更省指令）。"
                        "只作用同步异常（野指针 / 非法指令 / 缺页），中断不进这条逻辑；"
                        "换工程 / 卸载回默认。"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "stop_on_trap": {"type": "boolean", "description": "true=交付即停（默认）；false=继续跑"},
                "max_trap_events": {"type": "integer", "description": "配额（一次批处理窗口内最多几个陷阱）；stop_on_trap=false 时必填"},
                "stall_guard": {"type": "integer", "description": "连环守卫阈值（>=2）：同一 (cause, mepc) 连陷 N 次 ⇒ E_ENG-04；只在 stop_on_trap=false 时有效，省略=关"},
            },
            "required": ["stop_on_trap"],
        },
        "fn": t_stop_policy,
    },
    {
        "name": "poll_job",
        "description": "读一条异步作业（run(background=true) 的返回里有 job）。",
        "inputSchema": {
            "type": "object",
            "properties": {"job": {"type": "string"}},
            "required": ["job"],
        },
        "fn": t_poll_job,
    },
    {
        "name": "inspect",
        "description": ("读状态：kind=state（pc/t_ps/32 个寄存器）、console（offset/limit）、"
                        "memory（addr/len/format=u32|asc）。读操作合并成一个工具，省上下文。"),
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["state", "console", "memory"]},
                "addr": {"type": "integer"},
                "len": {"type": "integer"},
                "offset": {"type": "integer"},
                "limit": {"type": "integer"},
                "format": {"type": "string", "enum": ["u32", "asc"]},
            },
            "required": ["kind"],
        },
        "fn": t_inspect,
    },
    {
        "name": "disasm",
        "description": "反汇编：addr + count 条。",
        "inputSchema": {
            "type": "object",
            "properties": {"addr": {"type": "integer"}, "count": {"type": "integer"}},
            "required": ["addr"],
        },
        "fn": t_disasm,
    },
    {
        "name": "breakpoints",
        "description": "断点表：op=list / set（addr|symbol|condition 三选一）/ clear(id) / enable(id,enabled)。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "op": {"type": "string", "enum": ["list", "set", "clear", "enable"]},
                "addr": {"type": "integer"},
                "symbol": {"type": "string"},
                "condition": {"type": "string"},
                "id": {"type": "integer"},
                "enabled": {"type": "boolean"},
            },
            "required": ["op"],
        },
        "fn": t_breakpoints,
    },
    {
        "name": "events",
        "description": ("增量拉事件：since=上次的 next_seq。EventRing 的 seq 单调，"
                        "所以不需要长连 WS（隧道会断）。"),
        "inputSchema": {
            "type": "object",
            "properties": {"since": {"type": "integer"}, "limit": {"type": "integer"}},
        },
        "fn": t_events,
    },
    {
        "name": "close_session",
        "description": ("主动释放本客户端自己的会话（幂等）。跑完/交付完就还配额，"
                        "不必等空闲回收（MCP 侧 5 分钟 sweeper、服务端 15 分钟 TTL）。"
                        "只删本客户端映射的那一条 —— 浏览器页面或别的令牌开的会话不碰。"
                        "释放后再用就先 boot（会新建一条）。"),
        "inputSchema": {"type": "object", "properties": {}},
        "fn": t_close_session,
    },
]

if ALLOW_DESTRUCTIVE:
    TOOLS.append({
        "name": "reset",
        "description": "复位到最近一次成功装载点（破坏性：丢失当前执行进度）。",
        "inputSchema": {"type": "object", "properties": {}},
        "fn": t_destructive_reset,
    })
    TOOLS.append({
        "name": "drop_session",
        "description": "删除本客户端自己的会话（破坏性）。",
        "inputSchema": {"type": "object", "properties": {}},
        "fn": t_drop_session,
    })

BY_NAME = {tool["name"]: tool for tool in TOOLS}


# --------------------------------------------------------------------------- MCP 传输
def _audit(client: str, tool: str, ms: int, ok: bool, code: str = "") -> None:
    print(json.dumps({"audit": "call", "client": client, "tool": tool,
                      "ms": ms, "ok": ok, "code": code}, ensure_ascii=False), flush=True)


def _rpc_result(msg_id, result) -> dict:
    return {"jsonrpc": "2.0", "id": msg_id, "result": result}


def _rpc_error(msg_id, code: int, message: str) -> dict:
    return {"jsonrpc": "2.0", "id": msg_id, "error": {"code": code, "message": message}}


def handle_rpc(msg: dict, client: str) -> dict | None:
    method = msg.get("method")
    msg_id = msg.get("id")
    if method == "initialize":
        asked = (msg.get("params") or {}).get("protocolVersion") or PROTOCOL
        return _rpc_result(msg_id, {
            "protocolVersion": PROTOCOL if asked != PROTOCOL else asked,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {"name": "sim-mcp", "version": "0.1.5"},
            "instructions": ("先 boot，再 load_image(hex=原始字节的十六进制)，然后 advance(n)。"
                             "读状态用 inspect(kind=state)。长任务用 run(background=true)+poll_job。"),
        })
    if method in ("notifications/initialized", "notifications/cancelled"):
        return None
    if method == "ping":
        return _rpc_result(msg_id, {})
    if method == "tools/list":
        return _rpc_result(msg_id, {"tools": [
            {k: v for k, v in tool.items() if k != "fn"} for tool in TOOLS
        ]})
    if method != "tools/call":
        return _rpc_error(msg_id, -32601, f"method not found: {method}")

    params = msg.get("params") or {}
    name = params.get("name")
    args = params.get("arguments") or {}
    tool = BY_NAME.get(name)
    if not tool:
        return _rpc_error(msg_id, -32602, f"unknown tool: {name}")
    t0 = time.monotonic()
    try:
        out = tool["fn"](client, args)
        ms = int((time.monotonic() - t0) * 1000)
        _audit(client, name, ms, True)
        return _rpc_result(msg_id, {
            "content": [{"type": "text", "text": out["text"]}],
            "structuredContent": {"ok": True, "summary": out["text"], "data": out["data"]},
            "isError": False,
        })
    except SimError as err:
        ms = int((time.monotonic() - t0) * 1000)
        _audit(client, name, ms, False, err.code)
        return _rpc_result(msg_id, {
            "content": [{"type": "text", "text": f"{err.code}: {err.detail}"}],
            "structuredContent": {"ok": False, "code": err.code, "detail": err.detail,
                                  "upstream_status": err.status},
            "isError": True,
        })
    except Exception as err:  # noqa: BLE001 - 一条工具的异常不许打断整条连接
        ms = int((time.monotonic() - t0) * 1000)
        _audit(client, name, ms, False, "E_MCP-99")
        return _rpc_result(msg_id, {
            "content": [{"type": "text", "text": f"E_MCP-99: {err}"}],
            "structuredContent": {"ok": False, "code": "E_MCP-99", "detail": str(err)},
            "isError": True,
        })


# 令牌 → 租户键（账号 id）的缓存。**同一个账号的多把令牌落到同一个键** ⇒
# 它们看到同一批会话（「一个账号多个令牌」的口径就是这么来的）。
_CLIENTS: dict[str, str] = {}
_CLIENTS_LOCK = threading.Lock()


def _resolve_client(token: str) -> tuple[bool, str]:
    """令牌是否有效 + 它代表哪个租户。

    判据在**服务端**（`GET /api/whoami`）：MCP 不再维护第二套账号库。
    服务端没有账号库（兼容模式）时退回令牌哈希 —— 与 v0.1.9 的 `client_id`
    逐字节一致。
    """
    if not token:
        return False, ""
    digest = hashlib.sha256(token.encode()).hexdigest()
    if MCP_TOKEN and hmac.compare_digest(token, MCP_TOKEN):
        # 服务凭据：给本机/运维用，按它自己的哈希分桶（与 v0.1.9 相同）。
        return True, "c-" + digest[:8]
    with _CLIENTS_LOCK:
        cached = _CLIENTS.get(digest)
    if cached:
        return True, cached
    try:
        _, body, _ = _http("GET", "/api/whoami", token=token, timeout=5)
    except SimError:
        return False, ""
    key = "c-" + digest[:8]
    if isinstance(body, dict) and body.get("accounts"):
        key = str(body.get("account") or key)
    with _CLIENTS_LOCK:
        _CLIENTS[digest] = key
    return True, key


class Handler(BaseHTTPRequestHandler):
    server_version = "sim-mcp/0.1.4"
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt: str, *args) -> None:  # 默认 access log 太吵，审计另有
        return

    def _json(self, status: int, payload, extra: dict | None = None) -> None:
        blob = json.dumps(payload, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(blob)))
        for key, value in (extra or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(blob)

    def _token(self, path: str) -> str:
        auth = self.headers.get("authorization", "")
        if auth.lower().startswith("bearer "):
            return auth[7:].strip()
        header = self.headers.get("x-mcp-token", "").strip()
        if header:
            return header
        parts = path.strip("/").split("/")
        return unquote(parts[1]) if len(parts) > 1 and parts[0] == "mcp" else ""

    def do_GET(self) -> None:  # noqa: N802 - http.server 的命名
        path = urlparse(self.path).path
        if path == "/healthz":
            # 只报「我在跑」是不够的：上游地址配错时，进程活着但每个工具都 502。
            # 探针要能抓到这种「起得来、干不了活」的状态。
            try:
                with urllib.request.urlopen(SIM_BASE + "/healthz", timeout=2) as resp:
                    upstream_ok = resp.status == 200
                detail = ""
            except Exception as err:  # noqa: BLE001
                upstream_ok = False
                detail = str(err)
            self._json(200 if upstream_ok else 503, {
                "ok": upstream_ok, "tools": len(TOOLS), "base": SIM_BASE,
                "destructive": ALLOW_DESTRUCTIVE,
                **({} if upstream_ok else {"upstream_error": detail}),
            })
            return
        if path.startswith("/mcp"):
            self.send_response(405)
            self.send_header("allow", "POST, DELETE")
            self.send_header("content-length", "0")
            self.end_headers()
            return
        self._json(404, {"error": "not found"})

    def do_DELETE(self) -> None:  # noqa: N802
        token = self._token(urlparse(self.path).path)
        _TOKEN_CTX.token = token
        ok, _ = _resolve_client(token)
        if not ok:
            self._json(401, {"code": "E_MCP-08", "detail": "unauthorized"})
            return
        self.send_response(200)
        self.send_header("content-length", "0")
        self.end_headers()

    def _upload(self) -> None:
        """POST /images —— 把客户端手上的 ELF/裸 bin 推给仿真器并装载。

        为什么是 HTTP 端点而不是 `load_image(b64=...)`：
          - 100 KB 的 ELF base64 之后是 133 K 个字符，塞进 MCP 参数就是**烧上下文**，
            而且大多数客户端根本没法把「本地文件」变成参数；
          - 但字节流走 HTTP 是零 token 成本：客户端读文件 → 直接 POST。
        所以：小文件用 `load_image(b64)`，真文件用这里。

        鉴权只看 `SIM_MCP_TOKEN`（客户端那条）；`SIM_TOKEN` 仍只在本进程里。
        目标会话必须是**这个客户端自己的**（先 boot），不替别人装。
        """
        length = int(self.headers.get("content-length", "0"))
        if length <= 0:
            self._json(400, {"code": "E_MCP-08", "detail": "空 body：This is a raw-bytes upload"})
            return
        if length > MAX_UPLOAD:
            self._json(413, {"code": "E_MCP-09",
                             "detail": f"太大：{length} > 上限 {MAX_UPLOAD} 字节"})
            return
        name = (self.headers.get("x-image-name") or "fw.elf").strip()
        token = self._token(urlparse(self.path).path)
        _TOKEN_CTX.token = token
        ok, client = _resolve_client(token)
        if not ok:
            self._json(401, {"code": "E_MCP-08", "detail": "unauthorized"})
            return
        t0 = time.monotonic()
        try:
            sid = OWNER.sid(client)
            blob = self.rfile.read(length)
            status, body, _ = _http("POST", f"/api/sessions/{sid}/image", raw=blob,
                                    ctype="application/octet-stream",
                                    extra_headers={"x-image-name": name}, timeout=60)
            ms = int((time.monotonic() - t0) * 1000)
            print(json.dumps({"audit": "upload", "client": client, "name": name,
                              "bytes": len(blob), "ms": ms, "ok": True}), flush=True)
            self._json(status, {"ok": True, "session_id": sid, "bytes": len(blob), **body})
        except SimError as err:
            print(json.dumps({"audit": "upload", "client": client, "name": name,
                              "ms": int((time.monotonic() - t0) * 1000),
                              "ok": False, "code": err.code}), flush=True)
            self._json(err.status, {"ok": False, "code": err.code, "detail": err.detail})

    def do_POST(self) -> None:  # noqa: N802
        parsed = urlparse(self.path)
        if parsed.path.startswith("/images") or parsed.path.startswith("/upload"):
            token = self._token(parsed.path)
            _TOKEN_CTX.token = token
            ok, _ = _resolve_client(token)
            if not ok:
                # 读掉 body 再回，否则客户端写不进去、连接被重置
                int(self.headers.get("content-length", "0")) and self.rfile.read(
                    int(self.headers.get("content-length", "0")))
                self._json(401, {"code": "E_MCP-08", "detail": "unauthorized"})
                return
            self._upload()
            return
        if not parsed.path.startswith("/mcp"):
            self._json(404, {"error": "not found"})
            return
        token = self._token(parsed.path)
        _TOKEN_CTX.token = token
        ok, client = _resolve_client(token)
        if not ok:
            self._json(401, {"code": "E_MCP-08", "detail": "unauthorized"})
            return
        length = int(self.headers.get("content-length", "0"))
        try:
            msg = json.loads(self.rfile.read(length) or b"{}")
        except json.JSONDecodeError:
            self._json(400, _rpc_error(None, -32700, "parse error"))
            return
        replies = []
        batch = msg if isinstance(msg, list) else [msg]
        for one in batch:
            reply = handle_rpc(one, client)
            if reply is not None:
                replies.append(reply)
        if not replies:
            self.send_response(202)
            self.send_header("content-length", "0")
            self.end_headers()
            return
        body = replies if isinstance(msg, list) else replies[0]
        self._json(200, body, {"mcp-session-id": client})


def main() -> None:
    threading.Thread(target=_sweeper, daemon=True).start()
    srv = ThreadingHTTPServer((HOST, PORT), Handler)
    print(json.dumps({"audit": "start", "listen": f"{HOST}:{PORT}", "base": SIM_BASE,
                      "tools": [tool["name"] for tool in TOOLS],
                      "destructive": ALLOW_DESTRUCTIVE}, ensure_ascii=False), flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        srv.shutdown()


if __name__ == "__main__":
    main()
