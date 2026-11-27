#!/usr/bin/env python3
"""一个工具，直连 sim-mcp 的 Streamable HTTP：查健康、列工具、调任意一个工具。

    export SIM_MCP_URL=http://127.0.0.1:18877/mcp
    export SIM_MCP_TOKEN=<MCP 令牌>
    python3 scripts/sim_call.py --health
    python3 scripts/sim_call.py --list
    python3 scripts/sim_call.py --tool boot --json '{"board":"qemu-virt"}'
    python3 scripts/sim_call.py --tool advance --json '{"n":2}'
    python3 scripts/sim_call.py --tool inspect --json '{"kind":"state"}' --raw

只依赖标准库：客户端 SDK 版本各异，这条路径是「协议本身」的最低公共分母。
"""
import argparse
import json
import os
import sys
import urllib.error
import urllib.request

DEFAULT_URL = os.environ.get("SIM_MCP_URL", "http://127.0.0.1:18877/mcp")
DEFAULT_TOKEN = os.environ.get("SIM_MCP_TOKEN", "")


def http(url: str, token: str, body: dict | None = None, method: str = "POST"):
    req = urllib.request.Request(url, method=method)
    req.add_header("authorization", f"Bearer {token}")
    if body is None:
        req.add_header("accept", "application/json")
    else:
        req.data = json.dumps(body).encode()
        req.add_header("content-type", "application/json")
        req.add_header("accept", "application/json, text/event-stream")
    with urllib.request.urlopen(req, timeout=40) as resp:
        blob = resp.read()
        return json.loads(blob) if blob else {}


class Mcp:
    def __init__(self, url: str, token: str):
        self.url, self.token, self.seq = url, token, 0

    def rpc(self, method: str, params: dict | None = None):
        self.seq += 1
        return http(self.url, self.token,
                    {"jsonrpc": "2.0", "id": self.seq, "method": method, "params": params or {}})

    def handshake(self):
        init = self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                       "clientInfo": {"name": "sim_call", "version": "1"}})
        self.rpc("notifications/initialized")
        return init["result"]

    def call(self, name: str, args: dict) -> dict:
        reply = self.rpc("tools/call", {"name": name, "arguments": args})
        if "error" in reply:
            raise SystemExit(f"RPC error on {name}: {json.dumps(reply['error'], ensure_ascii=False)}")
        result = reply["result"]
        if result.get("isError"):
            raise SystemExit(f"tool {name} 失败: {result['content'][0]['text']}")
        return result.get("structuredContent") or {}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default=DEFAULT_URL)
    ap.add_argument("--token", default=DEFAULT_TOKEN)
    ap.add_argument("--health", action="store_true", help="探 /healthz（含上游探测）")
    ap.add_argument("--list", action="store_true", help="列工具")
    ap.add_argument("--tool", help="工具名")
    ap.add_argument("--json", default="{}", help="工具参数（JSON）")
    ap.add_argument("--raw", action="store_true", help="原样打印 structuredContent")
    a = ap.parse_args()

    if not a.token:
        raise SystemExit("缺令牌：--token 或 $SIM_MCP_TOKEN（别用 SIM_TOKEN）")

    if a.health:
        base = a.url.rsplit("/mcp", 1)[0] if a.url.endswith("/mcp") else a.url
        print(json.dumps(http(f"{base}/healthz", a.token, method="GET"), indent=2, ensure_ascii=False))
        return 0

    mcp = Mcp(a.url, a.token)
    info = mcp.handshake()
    print(f"# {info['serverInfo']['name']} {info['serverInfo'].get('version','')} "
          f"· MCP {info['protocolVersion']}", file=sys.stderr)

    if a.list:
        for t in (x["name"] for x in mcp.rpc("tools/list")["result"]["tools"]):
            print(t)
        return 0

    if not a.tool:
        raise SystemExit("要么 --list / --health，要么 --tool <名字>")

    out = mcp.call(a.tool, json.loads(a.json))
    if a.raw or not out.get("summary"):
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        print(out["summary"])
        data = out.get("data")
        if data is not None:
            print(json.dumps(data, ensure_ascii=False)[:1200])
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except urllib.error.HTTPError as err:
        print(f"HTTP {err.code}: {err.read()[:300]!r}", file=sys.stderr)
        raise SystemExit(1)
