#!/usr/bin/env python3
"""起前端静态服务（**不发缓存**）—— 本仓「打开前端」的**唯一**落点。

## 为什么不用 `python3 -m http.server`

⚠️ **`http.server` 只发 `Last-Modified`、不发 `Cache-Control`** ⇒ ⇒ 浏览器
按「**启发式新鲜度**」缓存 ES module ⇒ ⇒ **磁盘上代码已改、浏览器里跑的还是
旧的** ⇒ ⇒ 症状像「**改了没生效 / 功能没改**」⇒ ⇒ 会把人引到「**代码写错了**」
那条错路上（实测 2026-10-31 · v0.60 批：改了调试视图的 trace 开关后页面仍是
旧的两个按钮 ⇒ ⇒ 差点把「缓存」当成「实现跑偏」去改内核）。

⇒ ⇒ 本脚本给每个响应加 **`Cache-Control: no-store`**（外加 `Pragma` /
`Expires` 两个老浏览器兜底）⇒ ⇒ 浏览器**必须**每次回源 ⇒ ⇒ 「改了没生效」
这件事**在开发期结构上不可能**。

## 零依赖

只用标准库（与本仓「零依赖 · 零构建」的取向一致）⇒ ⇒ 不用装任何东西。

## 用法

```bash
python3 tools/serve-front.py              # 服务 front/ 到 http://127.0.0.1:8080
python3 tools/serve-front.py --port 9000  # 换端口
python3 tools/serve-front.py --host 0.0.0.0   # 允许局域网访问（默认只听本机）
python3 tools/serve-front.py --self-check     # 自检：起一个临时服务验响应头
```

⚠️ **换端口 = 换 origin = 换缓存键** ⇒ ⇒ 万一浏览器还拿着旧副本，**换端口
打开一次**就一定拿到新的（这是分辨「缓存」与「代码写错」最省事的一招）。

⚠️ **只给入口 URL 加 cache-bust 查询串没用**（`index.html?v=2`）：相对路径
`import` 的子模块 URL 不变 ⇒ ⇒ 它们**照旧**吃缓存 ⇒ ⇒ 必须靠**响应头**或
**换 origin**。
"""

from __future__ import annotations

import argparse
import functools
import http.server
import os
import socketserver
import sys
import threading
import urllib.request

#: 仓库根目录（本文件在 `tools/` 下 ⇒ 上一级就是仓库根）。
REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

#: 默认服务的目录（相对仓库根）—— 前端源码树。
DEFAULT_DIR = "front"

#: 默认端口（与 README / 用户指南里写的那个一致 ⇒ 换端口是「排查手段」）。
DEFAULT_PORT = 8080

#: 默认只听本机（⚠️ 前端不需要对外暴露 ⇒ 别默认绑 `0.0.0.0`）。
DEFAULT_HOST = "127.0.0.1"

#: `.mjs` / `.wasm` 的 MIME —— 标准库的 `guess_type` 在不同 Python 版本上
#: 不一致（`.wasm` 常常落到 `application/octet-stream` ⇒ ⇒ Worker 的
#: `WebAssembly.instantiateStreaming` 会拒）。
EXTRA_TYPES = {
    ".js": "text/javascript",
    ".mjs": "text/javascript",
    ".wasm": "application/wasm",
    ".json": "application/json",
    ".map": "application/json",
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
}


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    """静态文件服务 + **强制不发缓存**。"""

    def end_headers(self) -> None:
        # ⚠️ **`no-store` 是主力**：`no-cache` 只是「回源前先问一次」，
        # `no-store` 是「**既不存、也不许用已存的**」⇒ ⇒ 开发期要的是后者。
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate, max-age=0")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()

    def guess_type(self, path):  # noqa: D102 - 覆盖标准库签名
        ext = os.path.splitext(path)[1].lower()
        if ext in EXTRA_TYPES:
            return EXTRA_TYPES[ext]
        return super().guess_type(path)

    def end_headers_for_404(self) -> None:  # pragma: no cover - 占位
        """（保留位：将来若要改 404 页的措辞，从这里下手。）"""


def make_server(host: str, port: int, directory: str) -> socketserver.TCPServer:
    """造一个服务 `directory` 的 HTTP 服务。"""
    handler = functools.partial(NoCacheHandler, directory=directory)
    return socketserver.TCPServer((host, port), handler)


def self_check() -> int:
    """自检：起一个临时服务 ⇒ 验**响应头**与**内容**都对 ⇒ ⇒ 打印 PASS / FAIL。

    ⚠️ **验的是「浏览器会不会拿到旧副本」这件事本身** ⇒ ⇒ 不是「文件能不能
    取到」（那个 `http.server` 本来就能）⇒ ⇒ 判据 = 响应里有 `no-store`。
    """
    directory = os.path.join(REPO_ROOT, DEFAULT_DIR)
    if not os.path.isdir(directory):
        print(f"FAIL: 找不到前端目录 {directory}")
        return 1
    server = make_server("127.0.0.1", 0, directory)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        base = f"http://127.0.0.1:{port}"
        ok = True

        with urllib.request.urlopen(f"{base}/index.html", timeout=10) as resp:
            html = resp.read().decode("utf-8")
            cctl = resp.headers.get("Cache-Control", "")
            ctype = resp.headers.get("Content-Type", "")
        print(f"  · index.html: Cache-Control={cctl!r} Content-Type={ctype!r}")
        if "no-store" not in cctl:
            print("  ✗ 响应里没有 no-store ⇒ 浏览器会吃缓存 ⇒ ⇒ 这一版没意义")
            ok = False
        if "text/html" not in ctype:
            print("  ✗ index.html 的 Content-Type 不对")
            ok = False
        if "<title>" not in html:
            print("  ✗ index.html 内容不完整")
            ok = False

        with urllib.request.urlopen(f"{base}/app.js", timeout=10) as resp:
            cctl = resp.headers.get("Cache-Control", "")
            ctype = resp.headers.get("Content-Type", "")
            js = resp.read().decode("utf-8")
        print(f"  · app.js:     Cache-Control={cctl!r} Content-Type={ctype!r}")
        if "no-store" not in cctl:
            print("  ✗ JS 响应里没有 no-store")
            ok = False
        if "javascript" not in ctype:
            print("  ✗ .js 的 Content-Type 不是 text/javascript（ES module 会被拒）")
            ok = False
        if "import" not in js:
            print("  ✗ app.js 内容不完整")
            ok = False

        # ⚠️ **换 origin ⇒ 换缓存键** ⇒ ⇒ 换端口一定能拿到新副本 ⇒ ⇒
        #   这条是「用户说『改了没生效』」时的**最省事**的分辨法 ⇒ ⇒
        #   把它写进自检的输出里（不是断言，是提示）。
        print(f"  · 换端口提示：新 origin = 新缓存键 ⇒ ⇒ 换端口打开一次即可绕过旧缓存"
              f"（本次自检用的是 {base}）")
    finally:
        server.shutdown()
        server.server_close()

    if ok:
        print("PASS: 前端静态服务自检全过（响应头带 no-store ⇒ 浏览器不会吃缓存）")
        return 0
    print("FAIL: 前端静态服务自检失败")
    return 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="起前端静态服务（不发缓存）")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help="端口（默认 8080）")
    parser.add_argument("--host", default=DEFAULT_HOST, help="监听地址（默认 127.0.0.1）")
    parser.add_argument("--dir", default=DEFAULT_DIR, help="要服务的目录（相对仓库根，默认 front）")
    parser.add_argument("--self-check", action="store_true", help="自检响应头后退出")
    args = parser.parse_args(argv)

    if args.self_check:
        return self_check()

    directory = os.path.abspath(os.path.join(REPO_ROOT, args.dir))
    if not os.path.isdir(directory):
        print(f"找不到目录：{directory}")
        return 1
    try:
        server = make_server(args.host, args.port, directory)
    except OSError as err:
        print(f"起服务失败（端口 {args.port} 被占？）：{err}")
        print("  ⇒ 换端口即可（换端口 = 换 origin = 换缓存键 ⇒ 顺带绕开旧缓存）")
        return 1
    print(f"前端静态服务（**不发缓存**）：http://{args.host}:{args.port}/  ← 目录 {directory}")
    print("  ⇒ 改了前端代码**刷新即可**看到（浏览器不会吃缓存）")
    print("  ⇒ Ctrl-C 停止")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n已停止")
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
