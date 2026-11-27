#!/usr/bin/env python3
"""把 ACR 上的标签收敛成「只有带 mcp 的单镜像」。

口径（用户定）：
  - 镜像名固定 `riscv_sim`；
  - 只有单镜像（服务端 + sim-mcp）是发布物；
  - `latest` 永远等于最新 release，由 workflow 在打 tag 时更新；
  - 每个 `<git tag>` 保留自己那一份（不覆盖、不共用滚动别名）。

于是这里要删的是**不合规的历史标签**：
  `mcp-latest`（滚动别名）、`*-mcp`（双轨时代的后缀）、以及单镜像之前那批纯服务端
  的 `v0.1.0 / v0.1.1 / v0.1.2` 和它们的 `sha-*`。

保留规则（不写死版本号，避免下次发版还要改脚本）：
  `latest` · 所有 `v<major>.<minor>.<patch>` 中 **≥ 单镜像起点** 的 · 本次构建的 sha

用法：
  ACR_USERNAME=… ACR_PASSWORD=… python3 tools/prune_acr_tags.py <当前 sha 短号>
  （CI 里 secrets 已有，本地跑需要自己的账号密码；这是唯一有删除动作的脚本）

⚠️ registry v2 只能删 manifest，删一个 digest 会带走指向它的**所有**标签 ⇒ 先取
   每个标签的 digest，与保留集共用就跳过。ACR 个人版若拒绝删除（405/400），脚本
   只打印清单并退出 0 —— 发布流水线不因为「清理失败」变红。
"""
import base64
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

REG = "crpi-d990qy21vgojj2m7.cn-heyuan.personal.cr.aliyuncs.com"
REPO = "zyzs/riscv_sim"
SINGLE_IMAGE_SINCE = (0, 1, 3)  # v0.1.3 起才是一个镜像两个进程
ACCEPT = ",".join([
    "application/vnd.oci.image.manifest.v1+json",
    "application/vnd.docker.distribution.manifest.v2+json",
])


def token(scope="pull,push,delete"):
    # 删除 manifest 需要 token 里带 `delete` 这个 action：只申请 pull,push 时
    # registry 回 401（detail 里 Action=delete）—— 这是实测出来的，不是猜的。
    # 万一某个版本的 token 服务不认 `delete`，退回 pull,push（至少还能读）。
    user, pw = os.environ["ACR_USERNAME"], os.environ["ACR_PASSWORD"]
    basic = base64.b64encode(f"{user}:{pw}".encode()).decode()
    url = "https://dockerauth-cn-heyuan.aliyuncs.com/auth?" + urllib.parse.urlencode({
        "service": "registry.aliyuncs.com:cn-heyuan:26842",
        "scope": f"repository:{REPO}:{scope}",
    })
    req = urllib.request.Request(url, headers={"Authorization": f"Basic {basic}"})
    try:
        return json.load(urllib.request.urlopen(req, timeout=30))["token"]
    except urllib.error.HTTPError as e:
        if scope.endswith(",delete"):
            print(f"⚠️ 带 delete 的 scope 被拒（HTTP {e.code}），退回 pull,push")
            return token("pull,push")
        raise


def api(path, tok=None, method="GET", accept=ACCEPT):
    req = urllib.request.Request(f"https://{REG}{path}", method=method)
    if tok:
        req.add_header("Authorization", f"Bearer {tok}")
    if accept:
        req.add_header("Accept", accept)
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.status, r.headers, r.read()


def release_tag(t):
    if not (t.startswith("v") and t.count(".") == 2):
        return None
    try:
        return tuple(int(x) for x in t[1:].split("."))
    except ValueError:
        return None


def main():
    cur_sha = f"sha-{sys.argv[1]}" if len(sys.argv) > 1 else None
    try:
        tok = token()
    except urllib.error.HTTPError as e:
        print(f"⚠️ 取 token 失败 HTTP {e.code} {e.read()[:160]!r}：跳过清理")
        return 0
    _, _, body = api(f"/v2/{REPO}/tags/list", tok=tok, accept=None)
    tags = json.loads(body)["tags"]

    digests = {}
    for t in tags:
        _, hdrs, _ = api(f"/v2/{REPO}/manifests/{t}", tok=tok)
        digests[t] = hdrs.get("Docker-Content-Digest")

    keep = {"latest"} | ({cur_sha} if cur_sha in digests else set())
    for t in tags:
        v = release_tag(t)
        if v and v >= SINGLE_IMAGE_SINCE:
            keep.add(t)
    kept_digests = {digests[t] for t in keep if t in digests}

    print("保留：", sorted(keep))
    stale = sorted(t for t in tags if t not in keep)
    failures = 0
    done = set()
    for t in stale:
        d = digests[t]
        if d in kept_digests:
            print(f"跳过 {t}（digest 与保留项共用 {d[:19]}）")
            continue
        if d in done:
            print(f"跳过 {t}（同 digest 已删）")
            continue
        try:
            st, _, _ = api(f"/v2/{REPO}/manifests/{d}", tok=tok, method="DELETE")
            print(f"删除 {t} → HTTP {st}")
            done.add(d)
        except urllib.error.HTTPError as e:
            failures += 1
            print(f"删除 {t} 失败 → HTTP {e.code} {e.read()[:160]!r}")

    _, _, body = api(f"/v2/{REPO}/tags/list", tok=tok, accept=None)
    print("清理后：", json.loads(body)["tags"])
    if failures:
        print(f"⚠️ {failures} 个标签没删掉（个人版可能不支持 DELETE manifest，"
              f"需在 ACR 控制台手删；不影响发布）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
