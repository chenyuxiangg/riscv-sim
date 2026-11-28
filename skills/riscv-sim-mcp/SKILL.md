---
name: riscv-sim-mcp
description: 通过 sim-mcp（11 个 MCP 工具）驱动远端 RISC-V 仿真器 riscv_sim：装固件、单步推进、跑到停点、调陷阱交付策略、读寄存器/内存/console、反汇编、设断点、回放事件。当用户要求在 RISC-V 仿真器里跑固件或调试、问 pc/寄存器/x5/t_ps/断点/反汇编/串口输出、问「AI 怎么驱动仿真器」，或提到 riscv_sim、sim-mcp、MCP 仿真、RISC-V 调试时使用。不用于编译固件，也不替代 HTTP 契约调试。
---

# 用 sim-mcp 驱动 RISC-V 仿真器

目标：把一个**正在跑**的仿真器当外设来用 —— 装固件、一步一步走、在停点上看状态。
你不是在写代码，而是在**观测**一台机器；每个判断都要有 `inspect` / `disasm` / `events`
的读数支撑，不要凭想象解释执行结果。

## 1. 先打通连接（不通就别往下走）

生产入口就一条（443，按域名分 vhost，不带端口）：

```
URL   https://rsimmcp.lczyzs.xyz/mcp
HEAD  Authorization: Bearer <你自己的账号令牌>
```

- **令牌是账号级的，一人一把**：由运维在服务器上用账号库 CLI 发（`accounts.py
  add-account` / `add-token`，明文只打印一次）。`tok-xxxxxx` 是它的 id，不是令牌本身。
  **不要**索要/复用别人的令牌；容器 env 里那个 `SIM_MCP_TOKEN` 是进程内部兜底，
  不是给客户端用的全权凭据。
- 客户端不支持自定义 header ⇒ 用 URL 形态 `https://rsimmcp.lczyzs.xyz/mcp/<令牌>`。
- 先探一次健康：`GET https://rsimmcp.lczyzs.xyz/healthz`（免鉴权），期望
  `{"ok":true,"tools":11,"base":"http://127.0.0.1:9876"}`。`base` 不是 `:9876` ⇒ MCP 上游指错，
  每个工具都会 502，别怀疑工具。
- `401` 是令牌问题（`E_SESS-07` / `E_MCP-08`）：令牌被吊销、抄错、或拿了别人的。
- `502` 是反代到不了 ali_hy（端口/安全组/容器在重启），交给运维，不要反复重试。
- 本机调试才用隧道：`ssh -f -N -L 18877:127.0.0.1:8877 ali_hy` ⇒
  `http://127.0.0.1:18877/mcp`（只在 ali_hy 本机回环有效，已不对公网开放）。

命令行自查（不依赖任何客户端）：

```
python3 scripts/sim_call.py --url https://rsimmcp.lczyzs.xyz/mcp --token "$SIM_MCP_TOKEN" --list
python3 scripts/sim_call.py ... --tool boot --json '{}'
```

## 2. 铁律（破了结论就不可信）

1. **一个客户端一个会话，`boot` 一定要带 `board`。** 首次不传板，会话就没有生效板，
   之后 `load_image` 一直回 `E_PLT-22`，而 `boot` 还一直「复用成功」—— 看着像工具坏了。
   板在建会话时定死；再给一个不同的 `board` 会重建会话（sim-mcp **0.1.1** 起；
   0.1.0 是忽略，只能等空闲回收）。`boot` 本身幂等：同板复用回 `reused: true`。
   全机会话池只有 8 个、单账号 2 个，空闲 5 分钟（MCP sweeper）/ 15 分钟（服务端 TTL）会被回收 —— 但别指望它，收尾自己关（见铁律 4）。**不要反复 boot 来「重试」**，
   也不要指望有工具去看别人的会话 —— 看不到，这是设计（越权与不存在同回 `404 E_SESS-01`）。
   会话在**服务端内存**：容器重启/升级后旧 `session_id` 全部失效（回 404），重新 `boot` 即可，
   不要当成 bug。
2. **装固件只有一条路：客户端把文件字节推到 `POST /images`（`--upload`）。**
   不要用 `hex` / `b64` / `path` 去装真文件 —— 它们要么烧上下文，要么依赖“文件已经在
   服务器上”这种本 skill 无法验证的前提：
   - `hex`：只留给 8 字节基线自检（§4），不是装固件的手段；
   - `b64`：几十 KB 的 ELF 编成上百 K 字符，等于先烧几万 token，还容易截断；
   - `path`：读的是服务器上的镜像目录（账号模式下是 `/data/images/<你的账号>/`，相对名
     `prog.elf` 会被按账号根解析；v0.1.13 起），你没法确认里面到底是不是用户那个文件。
   文件从**你这侧**读、原始字节直接 POST，是这条路唯一的正确形态。
   `name` 的扩展名决定解析：`.bin` = RawHex（一律从 0 起），`.elf` = ELF（真实 entry、多段），
   `.hex` 文本（Intel HEX）会被 `E_PROTO-08` 拒。

3. **要验异常路径（野指针 / 非法指令 / 缺页）先调 `stop_policy {stop_on_trap:false, max_trap_events:N, stall_guard:32}`**
   —— 否则陷阱交付即停，handler 一条指令都跑不到，后面全丢。它是**会话级粘性**
   命令，换工程 / 卸载回默认；中断不进这条逻辑。`stall_guard`（B-2）可选：同一
   `(cause, mepc)` 连陷 N 次就报 `E_ENG-04` 中止（跨窗口累计，比配额早得多）——
   handler 死循环时回体是 `stop_reason:"error"` + `error_code`，别读成 guest 停点。

4. **任务收口必调 `close_session`，不要把会话留给 sweeper。** 一轮调试/一次交付结束就关：
   单账号只有 **2 条**配额，同账号的多个令牌、浏览器页面共用这一个池子；开着不关，
   下一个任务很可能直接 `429 E_SESS-05`。命令行走 `python3 scripts/sim_call.py --tool xxx --close`
   （或 `--upload fw.elf --close`），代码里放 `finally` —— 异常、超时、中途放弃也要关。
   它是幂等的，多关一次只是回 `closed:false`，不会误删别人的会话。

5. **`advance` 是「恰好 n 条」，`run` 是「跑到停点」。** 想复现就只用 `advance`；
   `run` 默认 5 秒墙钟，撞钟回 `504`，但 **body 仍是完整停点报告** —— 读它，不是报失败。
   `run` 的 `stop_reason = "budget"` 意思是**指令预算用尽**（前缀模式缺省 1e6 条，回体
   `budget_exhausted:true` 会明说），不是死锁 —— 给 `steps` 再来。

## 3. 工具面：写细分、读合并

| 工具 | 用途 | 关键参数 |
|---|---|---|
| `boot` | 建/复用本客户端会话（**带 board**） | `board`：`qemu-virt` / `default` / `minimal` |
| `POST /images` | **装固件唯一入口**（HTTP，不是 MCP 工具） | 头 `x-image-name`，body 原始字节 |
| `load_image` | 只用于 8 字节基线自检 | `hex`（**不装真文件**） |
| `advance` | 恰好走 n 条 | `n`（1..100000） |
| `run` | 跑到停点 | `steps` / `timeout_ms` / `prefix`（console 命中）/ `until`（breakpoint\|halt）/ `background` |
| `poll_job` | 读异步作业 | `job`（`run(background=true)` 回体里的） |
| `stop_policy` | 会话级陷阱交付策略（**R-1**）+ 异常连环守卫（**B-2**） | `stop_on_trap`（true=交付即停，默认）；`false` 时 `max_trap_events` **必填**；可选 `stall_guard`（≥2，同一 (cause,mepc) 连陷 N 次 ⇒ `E_ENG-04`） |
| `inspect` | 读状态/串口/内存 | `kind`=state\|console\|memory；console 用**绝对 offset**（默认 4 KiB，回 `base/total/next/lost`，按 `next` 续读，窗口丢了从 `base` 重来）；`addr` `len` `format`(u32\|asc) `limit` |
| `disasm` | 反汇编 | `addr` `count` |
| `breakpoints` | 断点表 | `op`=list\|set\|clear\|enable（set 三选一：`addr`/`symbol`/`condition`） |
| `events` | 增量拉事件 | `since`=`next`，`limit` |
| `keepalive` | 续空闲窗口（长思考/人机来回时用） | 无参；客户端 `ping` 同效；最多延长 3600s，收尾仍要 `close_session` |
| `close_session` | 主动释放自己的会话（幂等） | 无参；跑完就还配额，不必等空闲回收 |

`close_session` **始终注册**（正常生命周期，不是破坏性）：只删本客户端映射的那一条，
浏览器页面 / 别的令牌开的会话不碰；没有会话时按幂等回「无需释放」。
**用完不关 = 占着配额**（单账号 2 条），所以每个任务收尾都要调一次 —— 见 §2 铁律 4。

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
close_session                                    → 会话已删，配额立即可用（**收尾必做**）
```

顺序很关键：`close_session` 放**最后**。先关再读，后续每个工具都会回 `404 E_SESS-01`。
这是**验收基线**：跑出 `pc=8 · x5=3` 说明链路（隧道→MCP→server→内核）全通。
哪一跳不对，先证链路，再怀疑固件。

## 4.5 上传 ELF（**唯一的装载路径**）

MCP 是远端进程，看不见你的磁盘 —— 「文件」永远从**你这侧**推过去，一条路，没有分支：

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
| 域名整体 `502`（连 `/healthz` 也 502） | 反代到不了 ali_hy：8877 未监听 / 安全组没放 nginx 源 IP / 容器在重启 |
| `401` | 令牌被吊销、抄错、或拿了别人的；去运维要自己的令牌 |
| 刚换过容器，`session 404` | 会话是内存态，重启即清空 —— 重新 `boot` |
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
`riscv64-unknown-elf-gcc -nostdlib`），产出 `.elf` 或裸 `.bin`，再用 `--upload` 推上去。
（装固件不走 `load_image`，只有 8 字节基线用 `hex`。）
8 字节最小样例：`addi x5,x0,2` + `addi x5,x5,1` = `9302200093821200`。
