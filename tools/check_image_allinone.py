#!/usr/bin/env python3
"""单镜像冒烟：一个容器里两个进程，两边都得能用。

    python3 tools/check_image_allinone.py riscv-sim:allinone

断言：
    1. 不给 SIM_MCP_TOKEN 时只是服务端（老行为）—— 这一条由 check_image_front.py 守
    2. 给了 SIM_MCP_TOKEN ⇒ 9876 与 8877 都 /healthz 200
    3. 前端模块图完整（少文件就白屏的那次事故）
    4. MCP 的 tools/list 至少 10 个工具，且 boot / stop_policy 在
    5. MCP 的 /healthz 报的是**容器内**上游（127.0.0.1:9876），不是宿主端口

为什么单独一个脚本：合成镜像比单进程镜像多两个失效面 —— 「页面能开但 AI 连不上」
和「MCP 连的是宿主端口（容器里没人监听）」。两边都探才叫冒烟。
"""
import json
import sys
import urllib.request

from check_image_front import crawl, sh, wait_health

NAME = "sim-allinone-smoke"
SIM_PORT = 18899
MCP_PORT = 18877
SIM_TOKEN = "smoke-token-not-a-secret"
MCP_TOKEN = "smoke-mcp-token-not-a-secret"


def mcp_post(port: int, body: dict) -> dict:
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/mcp",
        data=json.dumps(body).encode(),
        method="POST",
        headers={"content-type": "application/json",
                 "accept": "application/json, text/event-stream",
                 "authorization": f"Bearer {MCP_TOKEN}"},
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: check_image_allinone.py <image>", file=sys.stderr)
        return 2
    image = sys.argv[1]

    sh("docker", "rm", "-f", NAME)
    run = sh(
        "docker", "run", "-d", "--name", NAME,
        "-p", f"127.0.0.1:{SIM_PORT}:9876",
        "-p", f"127.0.0.1:{MCP_PORT}:8877",
        "-e", "SIM_TOKEN=" + SIM_TOKEN,
        "-e", "SIM_MCP_TOKEN=" + MCP_TOKEN,
        "-e", "SIM_BOARD=qemu-virt",
        image,
    )
    if run.returncode != 0:
        print("容器起不来：\n" + run.stderr, file=sys.stderr)
        return 1

    try:
        if not wait_health(f"http://127.0.0.1:{SIM_PORT}"):
            print("仿真器 /healthz 不通", file=sys.stderr)
            print(sh("docker", "logs", "--tail", "40", NAME).stdout, file=sys.stderr)
            return 1
        if not wait_health(f"http://127.0.0.1:{MCP_PORT}"):
            print("sim-mcp /healthz 不通（上游配错时这里会是 503）", file=sys.stderr)
            print(sh("docker", "logs", "--tail", "40", NAME).stdout, file=sys.stderr)
            return 1

        # 1) 前端模块图
        seen, bad = crawl(f"http://127.0.0.1:{SIM_PORT}")
        print(f"模块图：{len(seen)} 个文件，坏掉 {len(bad)} 个")
        for path, why in bad[:10]:
            print(f"  ✗ {path} ← {why}", file=sys.stderr)
        if bad:
            return 1

        # 2) MCP 的两个口
        health = json.loads(urllib.request.urlopen(
            f"http://127.0.0.1:{MCP_PORT}/healthz", timeout=5).read())
        print("mcp /healthz →", json.dumps(health, ensure_ascii=False))
        if not health.get("ok"):
            print("MCP 自称不健康（或上游不通）", file=sys.stderr)
            return 1
        if ":9876" not in str(health.get("base", "")):
            print(f"MCP 上游不是容器内 9876：{health.get('base')!r}", file=sys.stderr)
            return 1

        tools = mcp_post(MCP_PORT, {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
        names = [t["name"] for t in tools["result"]["tools"]]
        print(f"tools：{len(names)} 个 → {names}")
        if len(names) < 10 or "boot" not in names or "stop_policy" not in names:
            print("工具面不对", file=sys.stderr)
            return 1

        print("✅ 单镜像冒烟通过：页面模块图完整 + MCP 10 工具 + 上游指向容器内 9876")
        return 0
    finally:
        sh("docker", "rm", "-f", NAME)


if __name__ == "__main__":
    sys.exit(main())
