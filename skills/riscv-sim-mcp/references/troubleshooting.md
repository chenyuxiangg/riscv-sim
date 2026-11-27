# 排查清单（从外往里，一层一层剥）

## 1. 连不上（`connection refused` / `000`）

- 8877 **只在宿主的 127.0.0.1 上**，公网没有这个端口 —— 直接打 `http://<公网IP>:8877`
  必然失败，这是正确的。
- 隧道：`ssh -f -N -L 18877:127.0.0.1:8877 ali_hy`。开两次会抢端口，
  用 `lsof -ti tcp:18877` 看有没有已经活着；macOS 上别用 `pkill`（会报
  `Cannot get process list`），要杀用 `lsof -ti tcp:18877 | xargs kill`。
- 隧道活着但 `healthz` 不通 ⇒ 容器没起：`docker ps --filter name=riscv-sim`。

## 2. 通了但 `/healthz` 说上游错

```json
{"ok": false, "tools": 9, "base": "http://127.0.0.1:9876", "upstream_error": "..."}
```

- `base` 必须是**容器内** `127.0.0.1:9876`。出现 `:8876` ⇒ MCP 的去向指到了宿主端口，
  容器里没人监听 ⇒ 所有工具都会 502，而 `/healthz` 的 HTTP 状态还可能仍是 200（有的版本）。
  这是配置问题（`SIM_BASE` / `SIM_MCP_BASE` 写错），不是工具问题。
- `503` = MCP 进程活着但仿真器不通。等容器 `healthy` 再试；`docker logs riscv-sim` 能看到
  两行日志（`server listening` 与 MCP 的审计行）。

## 3. 401 / 403

- `Authorization: Bearer <SIM_MCP_TOKEN>` 少了、拼错、或用了 `SIM_TOKEN`。
  用 `SIM_TOKEN` 打 MCP 也会 401 —— 它只在 MCP 进程的 env 里有效。
- 退化形态 `/mcp/<token>` 路径里的 token 要**整段**（不要 URL 编码出错）。

## 4. 工具回 409

`E_SESS-02`：会话忙，上一次 `run` 还在跑。等 200ms 重试即可。
**不要**重建会话、不要 `drop_session` —— 那等于把正在跑的任务砸掉。

## 5. 工具回 502 / 504

- `502`：MCP→server 这一跳断了。看 `/healthz` 的 `upstream_error`。
- `504`：`run` 撞墙钟。**body 里是完整停点报告**（executed / pc / stop_reason 都在），
  按成功解析；要更长的跑动就 `background=true` + `poll_job`。

## 5.5 回 `E_PLT-22`「没有生效板」

`boot` 时没给 `board`，会话建起来了但没有板，装载自然无处落。
先 `boot {"board":"qemu-virt"}`：0.1.1 起会**重建**会话；0.1.0 只会复用，
只能等 5 分钟空闲回收，或开闸 `drop_session`。别把这条当成工具坏了。

## 6. 装上了但跑不动

| 观察 | 可能原因 |
|---|---|
| `executed=1` 后 `stop_reason=trap`，`pc` 不变 | 内存里没有指令（固件没装上/装错段），`disasm` 确认 |
| `executed=0` | `n` 被服务端夹住，或入口不在当前 pc |
| `x5` 恒为 0 | 只跑了 1 条（第 2 条才写 3），或装的是 `.hex` 文本被当原始字节 |
| `load_image` 回 `E_PROTO-08` | 给了 Intel HEX 文本；改成裸 `.bin` 或 `.elf` |
| `entry` 不是期望值 | `.bin` 是 RawHex，没有 ELF 头 ⇒ 一律从 0 开始；要真实入口就给 `.elf` |

## 7. 找不到破坏性工具

`reset` / `drop_session` 默认**不注册**（`SIM_MCP_ALLOW_DESTRUCTIVE=0`）。
需要时让服务端把它设成 `1` 并重启 MCP。**不要**为了复位去直连 `:9876` 的 HTTP 端点 ——
那需要 `SIM_TOKEN`，把两层凭据合成一层就是把闸门拆了。

## 8. 会话凭空消失

空闲 5 分钟被服务端回收（`idle-drop` 会写进审计）。这不是错误：再 `boot` 一次即可，
返回 `reused:false` 说明确实新建了。会话池 8 个，多客户端并发时先看有没有人占着。
