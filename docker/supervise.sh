#!/bin/bash
# 单镜像双进程：仿真器（前台）+ sim-mcp（后台）。
#
# ⚠️ 为什么在一台容器里跑两个进程：MCP 只是仿真器 HTTP 面的一层适配，
#    分层部署要两个镜像、两份令牌、两套重启策略；合成一个之后「一条
#    docker run / 一个 compose 服务」就把两件事都办了。
# ⚠️ 但失败域也合了：任一进程退出 ⇒ 整个容器退出 ⇒ 交给 restart 策略
#    重启。不做「进程级自愈」，因为半死不活的容器比直接重启更难查。
set -u

: "${SIM_TOKEN:?SIM_TOKEN is required: the container binds 0.0.0.0, so the server refuses to start without a token}"

# 没给 MCP 令牌 ⇒ 就是老行为：只起服务端。
if [ -z "${SIM_MCP_TOKEN:-}" ]; then
    exec /usr/local/bin/sim-entrypoint
fi

if [ "${SIM_MCP_TOKEN}" = "${SIM_TOKEN}" ]; then
    echo "sim-supervise: SIM_MCP_TOKEN 与 SIM_TOKEN 相同 ⇒ 两层凭据退化，拒绝启动" >&2
    exit 1
fi

# 同容器内走回环私口（9876），不是宿主发布的 8876。
# ⚠️ 变量名是 SIM_BASE —— sim-mcp 读的就是它；写成 SIM_MCP_BASE 会静默指向
#    8876（宿主端口），容器内那个口根本没监听，MCP 一路 502。
export SIM_BASE="${SIM_BASE:-http://127.0.0.1:${SIM_PORT:-9876}}"

python3 -u /opt/sim-mcp/server.py &
mcp_pid=$!
/usr/local/bin/sim-entrypoint &
srv_pid=$!

trap 'kill -TERM "$srv_pid" "$mcp_pid" 2>/dev/null || true' TERM INT

wait -n
rc=$?
echo "sim-supervise: 有一个进程退出（rc=$rc）⇒ 整个容器退出" >&2
kill -TERM "$srv_pid" "$mcp_pid" 2>/dev/null || true
wait 2>/dev/null || true
exit 1
