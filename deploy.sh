#!/usr/bin/env bash
# 一键部署：构建镜像 → 起容器 → 等健康检查 → 打印可用地址。
#
#   ./deploy.sh                # 首次：生成 .env（含随机令牌）→ 构建 → 起 → 等健康
#   ./deploy.sh rebuild        # 全量重建镜像（--no-cache；依赖换了/缓存可疑时用）
#   ./deploy.sh pull           # 从 registry 拉现成镜像再起（需 SIM_IMAGE，服务器用）
#   ./deploy.sh down           # 停并删容器（数据卷保留）
#   ./deploy.sh logs           # 跟日志（Ctrl-C 退出）
#   ./deploy.sh ps             # 看容器与健康状态
#
# 常用变量（写进 .env 或在命令行前给）：
#   SIM_HOST_PORT=8876  宿主机端口        SIM_BOARD=qemu-virt   默认板
#   SIM_MAX_RUN_MS=60000  单次 run 上限   SIM_MAX_SESSIONS=8    并发会话
#   SIM_TOKEN=...       固定令牌（默认随机生成）  SIM_PLATFORM=linux/amd64  交叉构建
#   SIM_IMAGE=...       镜像全名（pull 时必填；默认本地 tag riscv-sim-server:latest）
#   SIM_DATA_VOLUME=... 数据卷名（默认 sim-data，compose 自己建）
#
# ⚠️ **令牌是必须的**：容器绑 `0.0.0.0`，服务端拒绝「非回环但没令牌」的启动。
#    页面地址要带 `?token=`（API 的 `require_token` 只认 `x-auth-token` 头，
#    页面负责把它从 URL 里取出来带上）。
set -euo pipefail

cd "$(dirname "$0")"

say() { printf '%s\n' "$*"; }
die() { printf 'deploy: %s\n' "$*" >&2; exit 1; }

compose() {
    if docker compose version >/dev/null 2>&1; then
        docker compose -f compose.yml "$@"
    elif command -v docker-compose >/dev/null 2>&1; then
        docker-compose -f compose.yml "$@"
    else
        die "docker compose 不可用（需要 Docker Desktop / docker-ce + compose v2）"
    fi
}

need_docker() {
    command -v docker >/dev/null 2>&1 || die "找不到 docker（先装 Docker Desktop 或 docker-ce）"
    docker info >/dev/null 2>&1 || die "docker 没起来（Docker Desktop 打开了吗？）"
}

random_token() {
    if command -v openssl >/dev/null 2>&1; then
        openssl rand -hex 24
    else
        head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'
    fi
}

ensure_env() {
    if [ -f .env ]; then
        return
    fi
    local token
    token="$(random_token)"
    cat > .env <<EOF
# 由 ./deploy.sh 生成（不要提交；.gitignore 已经忽略 .env）
SIM_HOST_PORT=${SIM_HOST_PORT:-8876}
SIM_BOARD=${SIM_BOARD:-qemu-virt}
SIM_MAX_RUN_MS=${SIM_MAX_RUN_MS:-60000}
SIM_MAX_SESSIONS=${SIM_MAX_SESSIONS:-8}
SIM_MAX_CONSOLE=${SIM_MAX_CONSOLE:-1048576}
SIM_SESSION_TTL=${SIM_SESSION_TTL:-900}
# 访问令牌：页面用 ?token=<这个值> 打开；改它要 ./deploy.sh rebuild
SIM_TOKEN=${SIM_TOKEN:-$token}
# AI 接入层令牌（sim-mcp）：客户端只拿这条，SIM_TOKEN 不下发
SIM_MCP_TOKEN=${SIM_MCP_TOKEN:-$(random_token)}
SIM_MCP_PORT=${SIM_MCP_PORT:-8877}
EOF
    chmod 600 .env
    say "==> 已生成 .env（令牌在里面，权限 600）"
}

# 等容器「健康」，而不是「起来了」：起得来的进程可能在 panic 之后仍然存在。
wait_healthy() {
    local container="$1" waited=0
    while [ "$waited" -lt 90 ]; do
        local status
        status="$(docker inspect -f '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "$container" 2>/dev/null || echo missing)"
        case "$status" in
            healthy)
                say "==> 容器健康（${waited}s）"
                return 0
                ;;
            unhealthy|missing)
                say ""
                say "!! 容器没健康起来（status=${status}），下面是最近的日志："
                docker logs --tail 40 "$container" 2>&1 || true
                return 1
                ;;
        esac
        sleep 3
        waited=$((waited + 3))
    done
    say "!! 90s 内没等到健康检查通过，下面是最近的日志："
    docker logs --tail 40 "$container" 2>&1 || true
    return 1
}

print_url() {
    local port token
    port="$(sed -n 's/^SIM_HOST_PORT=//p' .env | head -1)"
    token="$(sed -n 's/^SIM_TOKEN=//p' .env | head -1)"
    port="${port:-8876}"
    say ""
    say "页面（带令牌，点开即用）："
    say "  http://localhost:${port}/?token=${token}"
    say ""
    say "只想要地址、自己填令牌： http://localhost:${port}/"
    say "接口自检： curl -s -H 'x-auth-token: ${token}' http://localhost:${port}/api/version"
    say "健康探针： curl -s http://localhost:${port}/healthz"
    say ""
    say "⚠️ 令牌等于「谁能执行这个内核」⇒ 别贴到群里，也别把地址直接公开。"
}

build_image() {
    local platform="${SIM_PLATFORM:-}" extra=""
    [ "${1:-}" = "--no-cache" ] && extra="--no-cache"
    if [ -n "$platform" ] && docker buildx version >/dev/null 2>&1; then
        say "==> 交叉构建（${platform}）"
        docker buildx build $extra --load --platform "$platform" \
            -t "${SIM_IMAGE:-riscv-sim-server:latest}" .
    else
        say "==> 构建镜像（本机架构）"
        docker build $extra -f Dockerfile -t "${SIM_IMAGE:-riscv-sim-server:latest}" .
    fi
}

cmd="${1:-up}"
case "$cmd" in
    up)
        need_docker
        ensure_env
        # 带缓存构建：没改代码时这一步只是「检查完就说 up to date」（几秒）。
        # 要真正全量重建（依赖换了、缓存可疑）用 `./deploy.sh rebuild`。
        build_image
        compose up -d --remove-orphans
        container="${SIM_CONTAINER:-riscv-sim-server}"
        wait_healthy "$container" || exit 1
        print_url
        ;;
    rebuild)
        need_docker
        ensure_env
        build_image --no-cache
        compose up -d --remove-orphans
        container="${SIM_CONTAINER:-riscv-sim-server}"
        wait_healthy "$container" || exit 1
        print_url
        ;;
    pull)
        need_docker
        ensure_env
        : "${SIM_IMAGE:?pull 需要 SIM_IMAGE=完整镜像地址，例如 crpi-xxx.cn-heyuan.personal.cr.aliyuncs.com/zyzs/riscv_sim:latest}"
        say "==> 拉取 ${SIM_IMAGE}"
        docker pull "$SIM_IMAGE"
        docker image inspect --format '摘要：{{index .RepoDigests 0}}' "$SIM_IMAGE" || true
        # --no-build：镜像就在本地（刚从 registry 拉的）⇒ 不走 Dockerfile。
        SIM_IMAGE="$SIM_IMAGE" compose up -d --no-build --remove-orphans
        container="${SIM_CONTAINER:-riscv-sim-server}"
        wait_healthy "$container" || exit 1
        print_url
        ;;
    down)
        need_docker
        compose down
        say "==> 已停止（数据卷保留；要连数据一起删：docker volume rm <卷名>）"
        ;;
    logs)
        need_docker
        compose logs -f --tail 50
        ;;
    ps|status)
        need_docker
        compose ps
        ;;
    -h|--help|help)
        sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
        ;;
    *)
        die "不认识的子命令 ${cmd}（可用：up / rebuild / pull / down / logs / ps）"
        ;;
esac
