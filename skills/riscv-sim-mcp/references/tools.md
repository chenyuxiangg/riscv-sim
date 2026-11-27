# 工具面细节（sim-mcp 0.1.1）

所有工具都是 `tools/call`，返回 `structuredContent = {summary, data}`：`summary` 是给人读的
一行，`data` 是给判断用的结构。失败时 `isError: true`，`content[0].text` 带错误码。

## boot

- 参数：`board`（`qemu-virt` 默认 / `default` / `minimal`）
- 返回：`{session_id, board, reused}`
- 幂等：同一客户端、同一个 `board` 再调一次拿到同一个 `session_id`，`reused:true`
- **板是建会话时定死的**：给一个和当前不同的 `board` 会重建会话（0.1.1 起，审计里留
  `rebind` 一行）；不传 board 首次建会话 ⇒ 会话没板 ⇒ 后续 `load_image` 回 `E_PLT-22`
- 会话池 8 个，空闲 `SIM_MCP_IDLE_SECS`（默认 300s）被回收；回收是服务端主动 DELETE，
  客户端不用管

## load_image

- **本 skill 只允许一种装载入口：`POST /images`（见下节）。** 这个工具只留 `hex`
  （8 字节基线自检），协议上还存在 `b64` / `path` 两个参数，但**不要用**：`b64` 烧上下文、
  `path` 依赖「文件已经在服务器上」这个本 skill 无法核实的前提。
- 返回：`{name, kind, entry, segments:[{base,size}]}`
- `name` 的扩展名决定解析：`.bin` ⇒ RawHex，`.elf` ⇒ ELF；**`.hex` 文本会被
  `E_PROTO-08` 拒**（那是 Intel HEX 格式，不是原始字节）
- 装载后会复位到 `entry`，`t_ps` 归零

## 文件上行 `POST /images`（不是 MCP 工具）

```
POST <base>/images
Authorization: Bearer <SIM_MCP_TOKEN>
x-image-name: fw.elf
content-type: application/octet-stream

<文件原始字节>
```

- 回体：`{ok, session_id, bytes, name, kind, entry, segments:[{base,size}], pc}`
- 装完**即复位到 entry**；不用再调 `load_image`
- 目标会话必须是本客户端的（先 `boot`）；没有会话回 `409 / E_MCP-02`
- 上限 `SIM_MCP_MAX_UPLOAD`（默认 16 MiB）：超了 `413 / E_MCP-09`；空 body `400 / E_MCP-08`
- 为什么不用工具参数：100 KB 的 ELF base64 之后是 133 K 字符，等于把上下文烧光；
  而且多数客户端没法把本地文件变成参数。字节流走 HTTP 是零成本路径。

## advance

- 参数：`n`（1..`SIM_MCP_MAX_STEPS`，默认 100000）
- 返回：`{executed, pc, t_ps, stop_reason, total_executed, console?:{base,bytes,total}}`
- 语义是**恰好 n 条**，没有墙钟，可复现；`stop_reason` 可能是 `steps` / `trap` /
  `breakpoint` / `halt` —— 只有 `steps` 才代表「目标达成了」
- 会话忙 ⇒ `409`（`E_SESS-02`）；等 200ms 重试，别重建会话

## run

- 参数（都不给 ⇒ 什么都不推，只是空跑一次 5s 墙钟，所以至少给一个）：
  - `steps`：最多走多少条
  - `timeout_ms`：墙钟上限，**被夹到 `SIM_MCP_MAX_RUN_MS`（默认 5000）**
  - `prefix`：console 出现该字节串就停
  - `until`：`breakpoint` 或 `halt`
  - `background`：`true` ⇒ 立刻回 `job`（`202`），不阻塞
- 返回：同步 `{executed, pc, t_ps, stop_reason, ...}`；异步 `{job, ...}`
- **撞墙钟回 `504`，但 body 是完整停点报告** —— 按成功读，别当失败

## stop_policy

- 参数：`stop_on_trap`（**必填**）、`max_trap_events`（配额，`stop_on_trap=false` 时**必填**）
- `true`（默认）⇒ 同步异常交付即停，与改动前逐字节一致（AC1 基线）
- `false` ⇒ 陷阱交付后**继续跑** handler，最多 N 个后停（配额按**一次批处理窗口**计）
- `false` 不给配额：内核 `E_PROTO-08`；MCP 侧提前拦，给你 `E_MCP-07`
- **会话级粘性**：一次调用管到换工程 / 卸载（那时回默认）；`advance` / `run` 都吃它
- 事件流里会出现 `TrapSummary {count, first_cause, first_mepc, first_mtvec_target}`
  —— 只在非默认策略下发；`first_mtvec_target` 是 `enter_trap` 真正跳去的地址
- 中断（定时器 / 外设）**不进**这条逻辑：它们在指令边界交付，批次不停

## poll_job

- 参数：`job`（`run(background=true)` 回体里的 id）
- 返回：作业对象，含状态与（终态时的）停点报告
- 轮询间隔 300–500ms 起步；拿到终态就停手，不要空转

## inspect

- `kind=state` ⇒ `{executed, pc, registers[32], t_ps, ...}`（`registers[5]` 就是 `x5`）
- `kind=console` ⇒ `{base, bytes[], total}`，`offset` / `limit`（默认 1024）
- `kind=memory` ⇒ `{addr, bytes[]}`；`addr` 必填，`len` 默认 64，`format`=`u32`|`asc`
- `kind` 必填，取值只能是这三种；`memory` 缺 `addr` ⇒ `E_MCP-06`

## disasm

- 参数：`addr`（必填）、`count`（默认 8）
- 返回：`{instructions:[{addr,text}]}`（工具层已排成 `0x00000000: addi x5,x0,2` 的行）
- 用来判断「某段地址到底是指令还是空洞」—— 空内存反汇编会得到 0 或非法指令

## breakpoints

- `op=list` 无参
- `op=set` 三选一：`addr`（十进制）、`symbol`、`condition`（只取第一个命中的字段）
- `op=clear` 要 `id`（断点表里的 id，不是地址）
- `op=enable` 要 `id` + `enabled`
- 返回：`{breakpoints:[...]}`（工具层会把条数与前几条摘要打在 `summary` 里）

## events

- 参数：`since`（上次的 `next`，首次 0）、`limit`（默认 64）
- 返回：`{next, frames:[{seq, kind, pc?, t_ps?, regs?, bytes?, text?, brief?}]}`
- 帧已被**压平**：`SimStateDelta` 只留 pc/t_ps 与最多 8 个非零寄存器；
  console 帧只留字节数与 120 字文本。要全量寄存器就 `inspect`
- `seq` 单调，所以增量拉取不需要长连 WS（隧道会断，正好用这个）

## reset / drop_session（默认不注册）

- 只有服务端 `SIM_MCP_ALLOW_DESTRUCTIVE=1` 才出现在 `tools/list`
- `reset`：复位到最近一次成功装载点（丢当前进度）
- `drop_session`：删自己那条会话，不影响别人

## 错误码对照

| 码 | 出处 | 含义 / 动作 |
|---|---|---|
| `E_MCP-03` | load_image | 没给 hex（本 skill 唯一的调用形态），或 hex 不是合法十六进制 |
| `E_MCP-04` | advance | `n` 越界（不在 1..MAX_STEPS） |
| `E_MCP-05` | poll_job | 缺 `job` |
| `E_MCP-06` | inspect / disasm | `memory` 缺 `addr`，或 `kind` 不认识 |
| `E_MCP-07` | breakpoints | `op` 不认识，或 `set` 三选一都没给 / `clear` 缺 `id` |
| `E_MCP-08` | `POST /images` | body 空（发的是 JSON 不是字节流） |
| `E_MCP-09` | `POST /images` | 超过 `SIM_MCP_MAX_UPLOAD`（默认 16 MiB） |
| `E_SESS-02` | 服务端 | 会话忙（409），等 200ms 重试 |
| `E_PROTO-08` | 服务端 | 不接受的文件形态（`.hex` 文本） |
| `E_PLT-22` | 服务端 | 会话没有生效板：`boot` 没带 `board` |
| `502 / 503` | MCP | 上游不通：看 `/healthz` 的 `upstream_error` |
