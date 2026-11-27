---
name: riscv-sim-mcp
description: 通过 sim-mcp（9 个 MCP 工具）驱动远端 RISC-V 仿真器 riscv_sim：装固件、单步推进、跑到停点、读寄存器/内存/console、反汇编、设断点、回放事件。当用户要求在 RISC-V 仿真器里跑固件或调试、问 pc/寄存器/x5/t_ps/断点/反汇编/串口输出、问「AI 怎么驱动仿真器」，或提到 riscv_sim、sim-mcp、MCP 仿真、RISC-V 调试时使用。不用于编译固件，也不替代 HTTP 契约调试。
---

# 用 sim-mcp 驱动 RISC-V 仿真器

目标：把一个**正在跑**的仿真器当外设来用 —— 装固件、一步一步走、在停点上看状态。
你不是在写代码，而是在**观测**一台机器；每个判断都要有 `inspect` / `disasm` / `events`
的读数支撑，不要凭想象解释执行结果。

## 1. 先打通连接（不通就别往下走）

隧道 + 两条令牌：

```
ssh -f -N -L 18877:127.0.0.1:8877 ali_hy     # 只开一次，重复开会抢端口
URL   http://127.0.0.1:18877/mcp
HEAD  Authorization: Bearer <SIM_MCP_TOKEN>
```

- 客户端不支持自定义 header ⇒ 用退化形态 `http://127.0.0.1:18877/mcp/<SIM_MCP_TOKEN>`。
- 令牌来源：用户给的 `SIM_MCP_TOKEN`（或服务器上 `/root/riscv-sim/mcp-token`）。
  **永远不要**索要、打印或使用 `SIM_TOKEN` —— 那是服务端自己的凭据，客户端拿到就等于拿到全机。
- 先探一次健康：`GET http://127.0.0.1:18877/healthz`，期望 `ok:true`，且 `base` 里带
  **容器内**端口 `:9876`。`base` 指到 `:8876` 说明 MCP 连的是宿主端口 ⇒ 每个工具都会 502，
  这时别怀疑工具，直接报「MCP 上游指错」。
- `503` + `upstream_error` = MCP 活着但仿真器不通（容器在重启，或 server 崩了）。

命令行自查（不依赖任何客户端）：

```
python3 scripts/sim_call.py --url http://127.0.0.1:18877/mcp --token "$SIM_MCP_TOKEN" --list
python3 scripts/sim_call.py ... --tool boot --json '{}'
```

## 2. 三条铁律（破了结论就不可信）

1. **一个客户端一个会话，`boot` 一定要带 `board`。** 首次不传板，会话就没有生效板，
   之后 `load_image` 一直回 `E_PLT-22`，而 `boot` 还一直「复用成功」—— 看着像工具坏了。
   板在建会话时定死；再给一个不同的 `board` 会重建会话（sim-mcp **0.1.1** 起；
   0.1.0 是忽略，只能等空闲回收）。`boot` 本身幂等：同板复用回 `reused: true`。
   全机会话池只有 8 个，空闲 5 分钟由服务端自动回收。**不要反复 boot 来「重试」**，
   也不要指望有工具去看别人的会话 —— 看不到，这是设计。
2. **装的方式按文件大小选，别把 ELF 塞进工具参数。** 三种入口：
   - `hex` —— 原始字节的十六进制，**不是 Intel HEX 文件**（`.hex` 文本会被 `E_PROTO-08` 拒）；
   - `b64` —— 几十 KB 以内的小文件；再大就是烧上下文（100 KB 的 ELF base64 后 133 K 字符）；
   - **`POST /images` 上行** —— 真文件走这条：读本地文件、原始字节直接推，零 token。
   另有 `path`（服务器 `/data/images` 白名单下的文件）。`name` 的扩展名决定解析：
   `.bin` = RawHex（一律从 0 起），`.elf` = ELF（带真实 entry 与多段）。
3. **`advance` 是「恰好 n 条」，`run` 是「跑到停点」。** 想复现就只用 `advance`；
   `run` 默认 5 秒墙钟，撞钟回 `504`，但 **body 仍是完整停点报告** —— 读它，不是报失败。

## 3. 工具面：写细分、读合并

| 工具 | 用途 | 关键参数 |
|---|---|---|
| `boot` | 建/复用本客户端会话（**带 board**） | `board`：`qemu-virt` / `default` / `minimal` |
| `load_image` | 装固件并复位到入口 | `hex` / `b64`（小文件）/ `path`；`name` |
| `POST /images` | **上行真文件**（HTTP，不是工具） | 头 `x-image-name`，body 原始字节 |
| `advance` | 恰好走 n 条 | `n`（1..100000） |
| `run` | 跑到停点 | `steps` / `timeout_ms` / `prefix`（console 命中）/ `until`（breakpoint\|halt）/ `background` |
| `poll_job` | 读异步作业 | `job`（`run(background=true)` 回体里的） |
| `inspect` | 读状态/串口/内存 | `kind`=state\|console\|memory；`addr` `len` `format`(u32\|asc) `offset` `limit` |
| `disasm` | 反汇编 | `addr` `count` |
| `breakpoints` | 断点表 | `op`=list\|set\|clear\|enable（set 三选一：`addr`/`symbol`/`condition`） |
| `events` | 增量拉事件 | `since`=`next`，`limit` |

`reset` / `drop_session` 属**破坏性**，只有服务端设了 `SIM_MCP_ALLOW_DESTRUCTIVE=1` 才注册。
工具列表里没有它们，就是闸门关着 —— 告诉用户去开，**别绕过 MCP 直连 `:9876`**。

字段形状、返回体细节、错误码：读 [references/tools.md](references/tools.md)。

## 4. 标准循环（先跑通这条，再谈调试）

```
boot {"board":"qemu-virt"}                      → session_id
load_image hex=9302200093821200 name=fw8.bin    → entry=0x0，段 [0x0+8]
advance n=2                                     → executed=2 · pc=8 · t_ps=2000 · stop_reason=steps
inspect kind=state                              → x5(t0)=3
events since=0                                  → next=4，含 ImageLoaded / SimStateDelta
```

这是**验收基线**：跑出 `pc=8 · x5=3` 说明链路（隧道→MCP→server→内核）全通。
哪一跳不对，先证链路，再怀疑固件。

## 4.5 上传 ELF（客户端手上的文件）

MCP 是远端进程，看不见你的磁盘 —— 所以「文件」永远从**你这侧**推过去：

```
python3 scripts/sim_call.py --upload ./fw.elf          # 默认 name 取文件名
python3 scripts/sim_call.py --upload ./build/app.bin --name app.bin
```

等价于 `POST <base>/images`，头 `Authorization: Bearer <MCP令牌>` + `x-image-name: fw.elf`，
body 就是文件字节。回体 `{ok, bytes, kind, entry, segments, session_id}`，**装完即复位到
entry**，不用再调 `load_image`。

- 目标必须是**你自己的会话**：先 `boot {"board":"qemu-virt"}`，否则 409。
- `413 / E_MCP-09` = 超过 `SIM_MCP_MAX_UPLOAD`（默认 16 MiB）。
- `400 / E_MCP-08` = body 空（别把 JSON 当字节流传了）。
- 装完先 `inspect kind=state` 对一眼 `pc`，再 `disasm` 确认那段真的是你编的指令。

## 5. 长任务与并发

- 超过约 5 秒的跑动：`run background=true` 拿 `job`，用 `poll_job` 轮询到终态。
- `409 / E_SESS-02`（会话忙）= 上次请求还在跑。**等 200ms 重试**，不要重建会话、不要 drop。
- `events` 用游标：把返回的 `next` 当下次的 `since`。每次都从 0 拉就是重复烧上下文。
- 事件帧已被压平（`seq/kind/pc/t_ps/regs`）。要细节用 `inspect` 补，别指望事件里有全量寄存器。

## 6. 常见误判（这些踩过）

| 现象 | 真相 |
|---|---|
| 每个工具都 `502`，但 `/healthz` 200 | `base` 指错端口（宿主 8876），MCP 连了没人监听的地址 |
| 装入后 `pc` 不动、`executed=0` | 入口段没覆盖当前 pc；先 `disasm` 看那段是不是指令，再看 `stop_reason` |
| `x5` 一直是 0 | 只跑了 1 条，或把 `.hex` 文本当二进制装了 |
| `load_image` 回 `E_PLT-22`「没有生效板」 | `boot` 没带 `board`；0.1.1 起再 boot 一次带 board 就会重建会话 |
| `run` 回 `504` | 撞墙钟，**body 有完整停点**；改 `background` 或 `steps` 分片 |
| 没有 `reset` 工具 | 破坏性闸门关着（默认），不是 MCP 坏了 |
| 上传回 `409` | 还没 `boot`，或会话被别人占着（一客户端一会话） |
| 上传后 `pc=0` 但固件是 ELF | 传的是 `.bin` 名字 + ELF 内容（或反过来），解析按扩展名走 |

排查清单：[references/troubleshooting.md](references/troubleshooting.md)；上传细节见
[references/tools.md](references/tools.md) 的「文件上行」。

## 7. 调试剧本

单步走到断点、按 console 输出停机、读内存/反汇编定位、事件回放：
[references/recipes.md](references/recipes.md)。每个剧本都带「怎么判定成功」。

## 8. 固件怎么来

MCP 只负责**装**，不负责编。要新固件就在仓库里编（`cargo build` 或
`riscv64-unknown-elf-gcc -nostdlib`），产出 `.elf` 或裸 `.bin` 再喂 `load_image`。
8 字节最小样例：`addi x5,x0,2` + `addi x5,x5,1` = `9302200093821200`。
