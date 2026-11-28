#!/usr/bin/env python3
"""账号库（`accounts.json`）的离线管理工具（M-25）。

服务端只**读**这个文件；建账号、发令牌、吊销令牌都在这里做 —— 目的是让
「谁能进来」这件事永远不需要过 HTTP，也就没有「远程建账号」这个攻击面。

用法（默认路径 `/data/accounts.json`，容器里在数据卷上）：

    # 从现有 SIM_TOKEN 起步：把老令牌落成 admin 账号的第一把令牌
    python3 tools/accounts.py init --name admin --password '***' \
        --token "$SIM_TOKEN"

    # 加一个人（浏览器登录用密码；MCP 用下面发的令牌）
    python3 tools/accounts.py add-account --name alice --password '***'

    # 给某个账号再发一把令牌（一个账号可以有多把）
    python3 tools/accounts.py add-token --name alice

    # 吊销某一把（记录保留，`revoked: true`）
    python3 tools/accounts.py revoke-token --id tok-1a2b3c

    # 看一眼现状（**不回显**令牌与哈希）
    python3 tools/accounts.py list

密码派生与服务端**同一套参数**（`pbkdf2_sha256$<iter>$<salt>$<hash>`，
base64url 无填充），所以这里写的文件服务端直接能验。
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import secrets
import sys
import tempfile
import time

DEFAULT_PATH = os.environ.get("SIM_ACCOUNTS", "/data/accounts.json")
DEFAULT_ITERATIONS = 200_000
ROLES = ("admin", "user")


def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).decode().rstrip("=")


def hash_token(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()


def encode_password(password: str, iterations: int) -> str:
    salt = secrets.token_bytes(16)
    digest = hashlib.pbkdf2_hmac("sha256", password.encode(), salt, iterations, 32)
    return f"pbkdf2_sha256${iterations}${b64url(salt)}${b64url(digest)}"


def new_token() -> str:
    return b64url(secrets.token_bytes(32))


def new_account_id(existing: set[str]) -> str:
    while True:
        candidate = "acc-" + secrets.token_hex(3)
        if candidate not in existing:
            return candidate


def new_token_id(existing: set[str]) -> str:
    while True:
        candidate = "tok-" + secrets.token_hex(3)
        if candidate not in existing:
            return candidate


def load(path: str, *, must_exist: bool) -> dict:
    if not os.path.isfile(path):
        if must_exist:
            raise SystemExit(f"{path} 不存在：先跑 `init`")
        return {"version": 1, "accounts": []}
    with open(path, "r", encoding="utf-8") as handle:
        store = json.load(handle)
    if store.get("version") != 1:
        raise SystemExit(f"{path} 的 version 不是 1，本工具不认识")
    store.setdefault("accounts", [])
    return store


def save(path: str, store: dict) -> None:
    """原子写 + 0600。**半写的 accounts.json 会让服务端起不来**。"""
    directory = os.path.dirname(os.path.abspath(path)) or "."
    os.makedirs(directory, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=directory, prefix=".accounts-", suffix=".json")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(store, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
        os.chmod(tmp, 0o600)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def all_token_ids(store: dict) -> set[str]:
    return {token["id"] for account in store["accounts"] for token in account["tokens"]}


def find_account(store: dict, name: str) -> dict:
    for account in store["accounts"]:
        if account["name"].lower() == name.lower():
            return account
    raise SystemExit(f"没有账号 {name!r}")


def stamp() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


# --------------------------------------------------------------------------- 子命令


def cmd_init(args: argparse.Namespace) -> int:
    if os.path.isfile(args.path) and not args.force:
        raise SystemExit(f"{args.path} 已存在：加 --force 覆盖（会清掉现有账号与令牌）")
    store = {"version": 1, "accounts": []}
    account = {
        "id": new_account_id(set()),
        "name": args.name,
        "role": "admin",
        "password": encode_password(args.password, args.iterations) if args.password else None,
        "tokens": [],
        "created": stamp(),
    }
    token = None
    if args.token:
        token = args.token.strip()
        account["tokens"].append(
            {
                "id": new_token_id(set()),
                "hint": token[:4],
                "hash": hash_token(token),
                "created": stamp(),
                "revoked": False,
            }
        )
    elif args.mint_token:
        token = new_token()
        account["tokens"].append(
            {
                "id": new_token_id(set()),
                "hint": token[:4],
                "hash": hash_token(token),
                "created": stamp(),
                "revoked": False,
            }
        )
    store["accounts"].append(account)
    save(args.path, store)
    print(f"写入 {args.path}：账号 {account['name']}（{account['id']}，admin）")
    if token and not args.token:
        print(f"令牌（只显示这一次）：{token}")
    if args.password:
        print("密码已写入（明文不落盘，只存 pbkdf2 派生值）")
    elif token:
        print("⚠️ 这个账号没有密码：只能走令牌，不能浏览器登录")
    return 0


def cmd_add_account(args: argparse.Namespace) -> int:
    store = load(args.path, must_exist=True)
    if any(account["name"].lower() == args.name.lower() for account in store["accounts"]):
        raise SystemExit(f"账号 {args.name!r} 已存在（同名会让人分不清谁是谁）")
    account = {
        "id": new_account_id({a["id"] for a in store["accounts"]}),
        "name": args.name,
        "role": args.role,
        "password": encode_password(args.password, args.iterations) if args.password else None,
        "tokens": [],
        "created": stamp(),
    }
    token = None
    if not args.no_token:
        token = new_token()
        account["tokens"].append(
            {
                "id": new_token_id(all_token_ids(store)),
                "hint": token[:4],
                "hash": hash_token(token),
                "created": stamp(),
                "revoked": False,
            }
        )
    store["accounts"].append(account)
    save(args.path, store)
    print(f"加了账号 {args.name}（{account['id']}，{args.role}）")
    if token:
        print(f"令牌（只显示这一次）：{token}")
    if not args.password:
        print("⚠️ 没给密码：这个账号只能用令牌（MCP / 令牌链接）")
    return 0


def cmd_add_token(args: argparse.Namespace) -> int:
    store = load(args.path, must_exist=True)
    account = find_account(store, args.name)
    token = args.token.strip() if args.token else new_token()
    hint = token[:4]
    if any(t["hash"] == hash_token(token) for t in account["tokens"]):
        raise SystemExit("这把令牌已经在这个账号名下了（重复写只会多一条没用的记录）")
    record = {
        "id": new_token_id(all_token_ids(store)),
        "hint": hint,
        "hash": hash_token(token),
        "created": stamp(),
        "revoked": False,
    }
    account["tokens"].append(record)
    save(args.path, store)
    print(f"给 {account['name']} 加了令牌 {record['id']}（hint {hint}）")
    if not args.token:
        print(f"令牌（只显示这一次）：{token}")
    return 0


def cmd_revoke_token(args: argparse.Namespace) -> int:
    store = load(args.path, must_exist=True)
    for account in store["accounts"]:
        for token in account["tokens"]:
            if token["id"] == args.id:
                token["revoked"] = True
                save(args.path, store)
                print(f"已吊销 {args.id}（{account['name']} / hint {token.get('hint', '?')}）")
                return 0
    raise SystemExit(f"没有令牌 {args.id!r}（用 `list` 看现有 id）")


def cmd_list(args: argparse.Namespace) -> int:
    store = load(args.path, must_exist=True)
    print(f"{args.path}: {len(store['accounts'])} 个账号")
    for account in store["accounts"]:
        live = [t for t in account["tokens"] if not t.get("revoked")]
        print(
            f"  {account['name']:<16} {account['id']:<12} role={account['role']:<6} "
            f"password={'yes' if account.get('password') else 'no '} "
            f"tokens={len(live)}/{len(account['tokens'])}"
        )
        for token in account["tokens"]:
            mark = "revoked" if token.get("revoked") else "live   "
            print(f"      {token['id']:<12} {mark} hint={token.get('hint', '?')}")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="账号库（accounts.json）管理")
    parser.add_argument("--path", default=DEFAULT_PATH, help=f"文件路径（默认 {DEFAULT_PATH}）")
    sub = parser.add_subparsers(dest="command", required=True)

    init = sub.add_parser("init", help="新建一份账号库（可把现有 SIM_TOKEN 收进来）")
    init.add_argument("--name", default="admin")
    init.add_argument("--password", default=None, help="浏览器登录密码（不给就只能用令牌）")
    init.add_argument("--token", default=None, help="已有令牌（例如现在的 SIM_TOKEN）")
    init.add_argument("--mint-token", action="store_true", help="顺手发一把新令牌")
    init.add_argument("--iterations", type=int, default=DEFAULT_ITERATIONS)
    init.add_argument("--force", action="store_true")
    init.set_defaults(func=cmd_init)

    add = sub.add_parser("add-account", help="加一个账号（默认顺手发一把令牌）")
    add.add_argument("--name", required=True)
    add.add_argument("--password", default=None)
    add.add_argument("--role", choices=ROLES, default="user")
    add.add_argument("--no-token", action="store_true", help="先不发令牌")
    add.add_argument("--iterations", type=int, default=DEFAULT_ITERATIONS)
    add.set_defaults(func=cmd_add_account)

    token = sub.add_parser("add-token", help="给已有账号再发一把令牌")
    token.add_argument("--name", required=True)
    token.add_argument("--token", default=None, help="写死一把（不给就随机生成）")
    token.set_defaults(func=cmd_add_token)

    revoke = sub.add_parser("revoke-token", help="吊销一把令牌（记录保留）")
    revoke.add_argument("--id", required=True)
    revoke.set_defaults(func=cmd_revoke_token)

    listing = sub.add_parser("list", help="列账号与令牌（不回显明文）")
    listing.set_defaults(func=cmd_list)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
