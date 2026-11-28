# syntax=docker/dockerfile:1.6
#
# 单镜像：**服务端 + sim-mcp 两个进程**，一条 `docker run` 起两件事。
#
#   docker build -t riscv-sim .          # 或 ./deploy.sh（推荐）
#
# 为什么只有一个容器：
#   - `front/` 是**零构建**的 ES module（没有 npm 依赖、没有打包步骤）⇒ 直接
#     `COPY front` 就是可用的页面；
#   - 服务端用 `--front-dir` 自己托管它 ⇒ 不需要 nginx、不需要 CORS 白名单、
#     用户在浏览器里只需要一个地址；
#   - sim-mcp 只是服务端 HTTP 面的一层适配（`mcp/server.py`，纯标准库）⇒ 与它
#     同容器、走回环 `127.0.0.1:9876`，省掉第二个镜像、第二份令牌、第二套重启策略。
#
# ⚠️ 两个进程的生死由 `docker/supervise.sh` 管：任一退出 ⇒ 整个容器退出 ⇒ 交给
#    `--restart`。不做进程级自愈 —— 半死不活的容器比直接重启更难查。
#
# ⚠️ WASM 内核**不**打进镜像（默认形态）：本镜像的页面走 **server 模式**
#    （内核在容器里，native）。要离线单机模式请先 `tools/build-wasm.sh`，
#    并把 `front/assets/pkg` 留在构建上下文里（`.dockerignore` 默认不排除它）。

# ---------------------------------------------------------------------------
# 1) 构建服务端二进制
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src

# 依赖声明与源码一起拷：本仓的 crate 就是工作区成员，没有「只拷 manifest
# 再 stub 源码」的缓存戏法（那套要求三层目录镜像，改一次 manifest 就废）。
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --locked --release -p riscv_sim_server \
 && { strip target/release/riscv_sim_server 2>/dev/null || true; }

# ---------------------------------------------------------------------------
# 2) 运行层
# ---------------------------------------------------------------------------
# ⚠️ 运行层用 `python:3.12-slim-bookworm` 而不是 `debian:bookworm-slim` + apt：
#    mcp/server.py 是**纯标准库**，镜像里只需要一个 python3；用 python 基础镜像
#    就省掉 `apt-get update`。这不只是省事：2026-11-27 在 ali_hy 上
#    `apt-get update` 因 bookworm 仓库签名公钥缺失直接失败（NO_PUBKEY），
#    用 apt 的镜像连构建都起不来；不碰 apt 就没有这个外部依赖。
#    两个镜像同为 bookworm，构建产物（glibc 链接的服务端二进制）兼容。
FROM python:3.12-slim-bookworm AS runtime

# 非 root：容器里跑的是**内核宿主**，能执行任意固件 ⇒ 出事时不许它还是 root。
RUN useradd --system --uid 10001 --shell /usr/sbin/nologin sim \
 && mkdir -p /app /data/images \
 && chown -R sim:sim /app /data

WORKDIR /app

COPY --from=build /src/target/release/riscv_sim_server /usr/local/bin/riscv_sim_server
COPY docker/entrypoint.sh /usr/local/bin/sim-entrypoint
COPY docker/supervise.sh /usr/local/bin/sim-supervise
COPY docker/healthcheck.py /usr/local/bin/sim-healthcheck
COPY mcp/server.py /opt/sim-mcp/server.py
# M-25 账号库的离线 CLI：运维用 `docker exec … python3 /opt/sim-tools/accounts.py`
# 建账号 / 发令牌（服务端本身只读账号库，不写）。
COPY tools/accounts.py /opt/sim-tools/accounts.py
COPY front /app/front

# ⚠️ server.py 显式 0444：源文件在开发机上是 600，COPY 会保留 mode ⇒ 非 root 的
#    sim 用户读不到，容器会以「Permission denied」重启循环（真见过一次）。
RUN chmod 0755 /usr/local/bin/riscv_sim_server /usr/local/bin/sim-entrypoint \
       /usr/local/bin/sim-supervise /usr/local/bin/sim-healthcheck \
 && chmod 0444 /opt/sim-mcp/server.py \
 && chmod 0755 /opt/sim-tools/accounts.py \
 && chown -R sim:sim /app/front /opt/sim-mcp /opt/sim-tools

USER sim

ENV SIM_BIND=0.0.0.0 \
    SIM_PORT=9876 \
    SIM_FRONT_DIR=/app/front \
    SIM_IMAGE_ROOT=/data/images \
    SIM_MCP_PORT=8877 \
    SIM_MCP_HOST=0.0.0.0 \
    SIM_MCP_ALLOW_REMOTE=1

# 8877 是 MCP：容器内必须绑 0.0.0.0（端口映射打到容器 IP，不是回环），
# 但**宿主侧只发布 127.0.0.1** ⇒ 公网仍然没有这个端口，AI 走 SSH 隧道进来。
EXPOSE 9876 8877

# 探针打两边 `/healthz`（都不校验令牌：编排系统没有令牌，也不该有）。
# 只探服务端等于允许「页面能开、AI 连不上」的状态；MCP 的 /healthz 还会探上游，
# 配错地址时回 503 —— 「起得来、干不了活」也能被抓住。
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD ["python3", "/usr/local/bin/sim-healthcheck"]

ENTRYPOINT ["/usr/local/bin/sim-supervise"]
