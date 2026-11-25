#!/bin/sh
# 容器入口：把环境变量翻成 `riscv_sim_server` 的参数。
#
# ⚠️ **令牌是硬要求**：容器里一定绑 `0.0.0.0`，而服务端的 `guard_bind` 拒绝
#    「非回环但没有令牌」的启动 ⇒ 这里先自己说不清，别让用户对着一条
#    「refusing to bind 0.0.0.0」猜半天。
#
# ⚠️ **不做字符串拼接**（`$ARGS` 那套）：路径里有空格就会裂开。用 `set --`
#    逐个追加，每个值都是独立的一个 argv。
set -eu

: "${SIM_TOKEN:?SIM_TOKEN is required: the container binds 0.0.0.0, so the server refuses to start without a token}"

set -- riscv_sim_server \
    --bind "${SIM_BIND:-0.0.0.0}" \
    --port "${SIM_PORT:-9876}" \
    --allow-remote \
    --token "$SIM_TOKEN"

# 页面（`--front-dir`）：给了目录就是「一个端口」。没有就只服务 API。
if [ -n "${SIM_FRONT_DIR:-}" ]; then
    if [ -f "${SIM_FRONT_DIR}/index.html" ]; then
        set -- "$@" --front-dir "$SIM_FRONT_DIR"
    else
        echo "sim-entrypoint: SIM_FRONT_DIR=${SIM_FRONT_DIR} has no index.html, serving API only" >&2
    fi
fi

# 镜像路径白名单：容器里它指向挂载卷（用户放进来的 `.bin` / `.elf`）。
if [ -n "${SIM_IMAGE_ROOT:-}" ]; then
    mkdir -p "$SIM_IMAGE_ROOT" 2>/dev/null || true
    set -- "$@" --allow-image-root "$SIM_IMAGE_ROOT"
fi

[ -n "${SIM_BOARD:-}" ] && set -- "$@" --board "$SIM_BOARD"
[ -n "${SIM_CORS_ORIGINS:-}" ] && set -- "$@" --cors-origins "$SIM_CORS_ORIGINS"
[ -n "${SIM_MAX_RUN_MS:-}" ] && set -- "$@" --max-run-ms "$SIM_MAX_RUN_MS"
[ -n "${SIM_MAX_SESSIONS:-}" ] && set -- "$@" --max-sessions "$SIM_MAX_SESSIONS"
[ -n "${SIM_MAX_CONSOLE:-}" ] && set -- "$@" --max-console "$SIM_MAX_CONSOLE"
[ -n "${SIM_MAX_IMAGE_BYTES:-}" ] && set -- "$@" --max-image-bytes "$SIM_MAX_IMAGE_BYTES"
[ -n "${SIM_SESSION_TTL:-}" ] && set -- "$@" --session-ttl "$SIM_SESSION_TTL"

exec "$@"
