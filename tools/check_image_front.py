#!/usr/bin/env python3
"""镜像冒烟：起容器 → 抓前端模块图 → 少一个文件就失败。

为什么需要它（2026-11-26 真实事故）：
    `.dockerignore` 排除了 `front/dev`，而 `front/app.js` 里是**静态** import
    `./dev/mock-project.js`。ES module 的链接期是「全有或全无」⇒ 镜像里少一个
    文件 ⇒ **app.js 一行都不执行** ⇒ 页面只剩静态 HTML，用户看到
    「脚本没启动，请硬刷新」——刷新永远没用，因为文件是真缺。

    本机 `--front-dir front` 起服务时，开发目录在，所以「本地全绿 + 镜像白屏」；
    只有把**镜像**当真跑一遍才看得见。这个脚本就是那道关。

用法：
    python3 tools/check_image_front.py riscv-sim:probe

退出码：0 = 模块图完整；1 = 有 404 / 非 JS content-type / 页面起不来。
"""
import collections
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request

NAME = "sim-front-smoke"
PORT = 18899
TOKEN = "smoke-token-not-a-secret"

IMPORT_RE = re.compile(r'from\s+"([^"]+)"|import\s*\(\s*"([^"]+)"\s*\)|import\s+"([^"]+)"')


def sh(*args):
    return subprocess.run(args, capture_output=True, text=True)


def wait_health(base, timeout=25.0):
    end = time.time() + timeout
    while time.time() < end:
        try:
            with urllib.request.urlopen(base + "/healthz", timeout=2) as r:
                if r.status == 200:
                    return True
        except Exception:
            time.sleep(0.4)
    return False


def crawl(base, entry="app.js"):
    seen, bad, q = set(), [], collections.deque([entry])
    while q:
        path = q.popleft()
        if path in seen:
            continue
        seen.add(path)
        try:
            with urllib.request.urlopen(urllib.parse.urljoin(base, path), timeout=8) as r:
                body = r.read().decode("utf-8", "ignore")
                ctype = r.headers.get("content-type") or ""
        except urllib.error.HTTPError as e:          # 404 / 403 …
            bad.append((path, f"HTTP {e.code}"))
            continue
        except Exception as e:                        # 连接失败
            bad.append((path, repr(e)))
            continue
        if "javascript" not in ctype:
            bad.append((path, "content-type: " + (ctype or "<none>")))
        for m in IMPORT_RE.finditer(body):
            target = m.group(1) or m.group(2) or m.group(3) or ""
            if target.startswith("./") or target.startswith("../") or target.endswith(".js"):
                q.append(urllib.parse.urljoin(path, target))
    return seen, bad


def main():
    if len(sys.argv) < 2:
        print("usage: check_image_front.py <image>", file=sys.stderr)
        return 2
    image = sys.argv[1]

    sh("docker", "rm", "-f", NAME)
    run = sh(
        "docker", "run", "-d", "--name", NAME,
        "-p", f"127.0.0.1:{PORT}:9876",
        "-e", "SIM_TOKEN=" + TOKEN,
        "-e", "SIM_BOARD=qemu-virt",
        image,
    )
    if run.returncode != 0:
        print("容器起不来：\n" + run.stderr, file=sys.stderr)
        return 1

    base = f"http://127.0.0.1:{PORT}"
    try:
        if not wait_health(base):
            logs = sh("docker", "logs", "--tail", "30", NAME)
            print("健康检查没通过：\n" + logs.stdout + logs.stderr, file=sys.stderr)
            return 1
        page = urllib.request.urlopen(base + "/", timeout=8).read().decode("utf-8", "ignore")
        if "app.js" not in page:
            print("index.html 里没有 app.js —— 页面不是这一份", file=sys.stderr)
            return 1
        seen, bad = crawl(base)
        print(f"模块图：{len(seen)} 个文件，坏掉 {len(bad)} 个")
        for path, why in bad[:20]:
            print(f"  ✗ {path}  ←  {why}", file=sys.stderr)
        if bad:
            print(
                "\n镜像里少了模块图里的文件。多半是 .dockerignore 把某个 front/ 目录排掉了，"
                "\n而前端有静态 import 指着它 ⇒ 整张图链接失败 ⇒ app.js 不执行。",
                file=sys.stderr,
            )
            return 1
        print("前端模块图完整（每个静态 import 都能取到、且 content-type 是 JS）")
        return 0
    finally:
        sh("docker", "rm", "-f", NAME)


if __name__ == "__main__":
    sys.exit(main())
