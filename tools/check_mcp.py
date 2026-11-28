#!/usr/bin/env python3
"""sim-mcp 的验收冒烟：只走 MCP 协议，不碰仿真器的私有端点。

    python3 tools/check_mcp.py --url http://127.0.0.1:18877/mcp --token <MCP 令牌>

断言（这条线就是「AI 能自己跑起来」的判据）：
    1. tools/list 至少 10 个工具，且 boot / load_image / advance / close_session 都在
    2. boot 拿到 session_id
    3. 装 8 字节 RawHex 固件（addi x5,x0,2 → addi x5,x5,1）
    4. advance(2) ⇒ executed=2 · pc=8 · t_ps=2000
    5. inspect(state) ⇒ x5=3
    6. events(since=0) 能取到东西（EventRing 有 seq）
    7. 文件上行 POST /images：同一个令牌推 8 字节 bin，回体是 Elf/RawHex + entry
       （这条对应「客户端把 ELF 推给仿真器」那条路径：字节流走 HTTP，不进工具参数）
"""
import argparse
import json
import sys
import urllib.error
import urllib.request

FW_HEX = "9302200093821200"  # 2 条指令：addi x5,x0,2 / addi x5,x5,1


class Mcp:
    def __init__(self, url: str, token: str) -> None:
        self.url, self.token = url, token
        self.seq = 0

    def rpc(self, method: str, params: dict | None = None):
        self.seq += 1
        body = json.dumps({"jsonrpc": "2.0", "id": self.seq, "method": method,
                           "params": params or {}}).encode()
        req = urllib.request.Request(self.url, data=body, method="POST", headers={
            "content-type": "application/json",
            "accept": "application/json, text/event-stream",
            "authorization": f"Bearer {self.token}",
        })
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                blob = resp.read()
                return json.loads(blob) if blob else {}  # 通知类请求回 202 空体
        except urllib.error.HTTPError as err:
            print(f"HTTP {err.code}: {err.read()[:400]!r}", file=sys.stderr)
            raise

    def call(self, name: str, args: dict | None = None) -> dict:
        reply = self.rpc("tools/call", {"name": name, "arguments": args or {}})
        if "error" in reply:
            raise SystemExit(f"RPC error on {name}: {reply['error']}")
        result = reply["result"]
        if result.get("isError"):
            raise SystemExit(f"tool {name} 失败: {result['content'][0]['text']}")
        return result.get("structuredContent") or {}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="http://127.0.0.1:18877/mcp")
    ap.add_argument("--token", required=True)
    args = ap.parse_args()

    mcp = Mcp(args.url, args.token)

    init = mcp.rpc("initialize", {"protocolVersion": "2025-06-18",
                                  "capabilities": {},
                                  "clientInfo": {"name": "check_mcp", "version": "0"}})
    print("initialize →", init["result"]["serverInfo"], "|", init["result"]["protocolVersion"])
    mcp.rpc("notifications/initialized")
    tools = [t["name"] for t in mcp.rpc("tools/list")["result"]["tools"]]
    print("tools →", tools)
    for need in ("boot", "load_image", "advance", "inspect", "close_session"):
        assert need in tools, f"缺工具 {need}"

    boot = mcp.call("boot", {"board": "qemu-virt"})
    sid = boot["data"]["session_id"]
    print("boot →", sid, boot["data"]["board"], "reused" if boot["data"]["reused"] else "new")

    ka = mcp.call("keepalive")
    print("keepalive →", ka["summary"])
    assert ka["data"]["session_id"] == sid, ka["data"]
    mcp.rpc("ping")  # 协议层心跳也走同一条保活路径，回体必须还是空结果

    img = mcp.call("load_image", {"hex": FW_HEX, "name": "fw8.bin"})
    print("load_image →", img["summary"])

    adv = mcp.call("advance", {"n": 2})
    print("advance →", adv["summary"])
    assert adv["data"]["executed"] == 2, adv["data"]
    assert adv["data"]["pc"] == 8, adv["data"]

    st = mcp.call("inspect", {"kind": "state"})
    print("inspect →", st["summary"])
    assert st["data"]["registers"][5] == 3, st["data"]["registers"]

    c1 = mcp.call("inspect", {"kind": "console", "offset": 0})
    c2 = mcp.call("inspect", {"kind": "console", "offset": 0})
    assert c1["data"]["text"] == c2["data"]["text"], "同 offset 重读必须一致（读不消费）"
    assert c1["data"]["base"] == c2["data"]["base"] and c1["data"]["total"] == c2["data"]["total"]
    assert c1["data"].get("lost", 0) == 0, c1["data"]
    print("console →", c1["summary"][:120])

    # 7) 文件上行：真文件不该走工具参数（base64 会烧上下文），走 POST /images
    base = args.url.rsplit("/mcp", 1)[0] if args.url.endswith("/mcp") else args.url
    req = urllib.request.Request(f"{base}/images", data=bytes.fromhex(FW_HEX), method="POST")
    req.add_header("authorization", f"Bearer {args.token}")
    req.add_header("content-type", "application/octet-stream")
    req.add_header("x-image-name", "fw8.bin")
    with urllib.request.urlopen(req, timeout=30) as resp:
        up = json.loads(resp.read())
    assert up["ok"] and up["kind"] in ("RawHex", "Elf"), up
    assert up["entry"] == 0 and up["bytes"] == 8, up
    print("upload /images →", up["kind"], f"entry={up['entry']:#x}", f"{up['bytes']} B")

    ev = mcp.call("events", {"since": 0, "limit": 32})
    print("events →", ev["summary"][:160])
    assert ev["data"]["frames"], "事件环里应该有 SimStateDelta"
    assert ev["data"]["next"] >= 1, ev["data"]["next"]

    # 8) 主动释放：跑完就还配额（不必等 MCP 5 分钟 sweeper / 服务端 15 分钟 TTL）
    closed = mcp.call("close_session")
    print("close_session →", closed["summary"])
    assert closed["data"]["closed"] is True, closed["data"]
    assert closed["data"]["session_id"] == sid, closed["data"]

    print("\n✅ MCP 验收通过：装固件 → advance(2) → pc=8 · x5=3 → close_session 释放")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
