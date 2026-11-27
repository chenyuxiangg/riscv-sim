#!/usr/bin/env python3
"""容器探针：仿真器 + （开了 MCP 时的）sim-mcp 都得能收流量才算健康。

只用标准库：运行层是 python:3.12-slim，没有 curl，也不该为了探针再装一遍。
"""
import os
import sys
import urllib.request

PORTS = [("sim", os.environ.get("SIM_PORT", "9876"))]
if os.environ.get("SIM_MCP_TOKEN"):
    PORTS.append(("sim-mcp", os.environ.get("SIM_MCP_PORT", "8877")))

for name, port in PORTS:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/healthz", timeout=2) as resp:
            if resp.status != 200:
                print(f"{name} :{port} 回 {resp.status}", file=sys.stderr)
                sys.exit(1)
    except Exception as err:  # noqa: BLE001 - 探针只关心「通不通」
        print(f"{name} :{port} 不通：{err}", file=sys.stderr)
        sys.exit(1)

print("ok: " + ", ".join(f"{n}:{p}" for n, p in PORTS))
