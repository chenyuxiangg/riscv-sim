# sim-mcp —— 只依赖标准库的镜像（python:3.12-alpine 即可）
#
#   docker build -t sim-mcp:0.1.0 mcp/
#
# 运行形态（只经隧道，所以宿主侧绑 127.0.0.1）：
#   docker run -d --name sim-mcp --restart unless-stopped --network host \
#     --security-opt no-new-privileges:true \
#     -e SIM_BASE=http://127.0.0.1:8876 \
#     -e SIM_TOKEN=<server 令牌> \
#     -e SIM_MCP_TOKEN=<给客户端的那条> \
#     sim-mcp:0.1.0
#
# ⚠️ 用 `--network host` 是有意的：本服务只监听 127.0.0.1:8877，宿主上端口不发布，
#    也就不会被 `-p` 顺手摊到公网。桥接网络下 127.0.0.1 指的是容器自己，必须改绑
#    0.0.0.0 + 设 SIM_MCP_ALLOW_REMOTE=1，暴露面反而更难守。
FROM python:3.12-alpine

WORKDIR /app
# ⚠️ 显式给读权限：COPY 会保留源文件 mode（本地 600 会让非 root 起不来）
COPY --chmod=0444 server.py /app/server.py

RUN adduser -D -u 10001 mcp
USER mcp

ENV PYTHONUNBUFFERED=1 \
    SIM_MCP_HOST=127.0.0.1 \
    SIM_MCP_PORT=8877

EXPOSE 8877

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD python3 -c "import urllib.request as u; u.urlopen('http://127.0.0.1:8877/healthz', timeout=2)"

ENTRYPOINT ["python3", "-u", "/app/server.py"]
