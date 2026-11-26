# syntax=docker/dockerfile:1.6
#
# 单容器 / 单端口：**页面与 API 同一个 origin**（`--front-dir`）。
#
#   docker build -t riscv-sim-server .          # 或 ./deploy.sh（推荐）
#
# 为什么只有一个容器：
#   - `front/` 是**零构建**的 ES module（没有 npm 依赖、没有打包步骤）⇒ 直接
#     `COPY front` 就是可用的页面；
#   - 服务端用 `--front-dir` 自己托管它 ⇒ 不需要 nginx、不需要 CORS 白名单、
#     用户在浏览器里只需要一个地址。
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
FROM debian:bookworm-slim AS runtime

# curl 只给 HEALTHCHECK 用（keep-alive 的编排系统靠它判断「能不能收流量」）。
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# 非 root：容器里跑的是**内核宿主**，能执行任意固件 ⇒ 出事时不许它还是 root。
RUN useradd --system --uid 10001 --shell /usr/sbin/nologin sim \
 && mkdir -p /app /data/images \
 && chown -R sim:sim /app /data

WORKDIR /app

COPY --from=build /src/target/release/riscv_sim_server /usr/local/bin/riscv_sim_server
COPY docker/entrypoint.sh /usr/local/bin/sim-entrypoint
COPY front /app/front

RUN chmod 0755 /usr/local/bin/riscv_sim_server /usr/local/bin/sim-entrypoint \
 && chown -R sim:sim /app/front

USER sim

ENV SIM_BIND=0.0.0.0 \
    SIM_PORT=9876 \
    SIM_FRONT_DIR=/app/front \
    SIM_IMAGE_ROOT=/data/images

EXPOSE 9876

# 探针打 `/healthz`：它不校验令牌（编排系统没有令牌，也不该有）。
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${SIM_PORT}/healthz" >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/sim-entrypoint"]
