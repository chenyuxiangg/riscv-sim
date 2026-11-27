# 调试剧本

每个剧本＝一串工具调用 + 一条**判据**。判据不成立就停手排查，不要接着往下推。

## R1 · 链路基线（每次开工先跑）

```
boot {}
load_image {"hex":"9302200093821200","name":"fw8.bin"}
advance {"n":2}
inspect {"kind":"state"}
```

判据：`executed=2 · pc=8 · t_ps=2000`，`registers[5]=3`。
不成立 ⇒ 先看 `stop_reason`：`trap` 多半是固件装空了，`steps` 才对。

## R2 · 断点 + 跑到断点

```
breakpoints {"op":"set","addr":8}          → 记下 id
run {"until":"breakpoint","timeout_ms":3000}
inspect {"kind":"state"}
breakpoints {"op":"list"}
```

判据：`stop_reason=breakpoint` 且 `pc=8`。
`pc` 不是 8 ⇒ 断点没落在已装载的段上（先 `disasm {"addr":8,"count":4}` 看有没有指令）。

## R3 · 按串口输出停机（固件 printf 调试）

```
run {"prefix":"FAIL:","timeout_ms":5000}
inspect {"kind":"console","offset":0,"limit":2048}
```

判据：`stop_reason` 命中 prefix，且 console 里能看到那段字节。
console 是环形（`total` 可能大于 `limit`）：要早期输出就从小 `offset` 开始读。

## R4 · 长跑别撞墙钟

```
run {"background":true,"steps":1000000}
poll_job {"job":"<上一步的 job>"}          → 重复到终态
```

判据：作业终态出现，且 `executed` 与预期量级一致。
`poll_job` 空转超过 3 次仍无进展 ⇒ 用 `inspect {"kind":"state"}` 确认它是不是卡在某个 `pc`。

## R5 · 定位「pc 为什么停这儿」

```
disasm {"addr":0,"count":16}                  # 入口段有没有指令
inspect {"kind":"memory","addr":0,"len":32}   # 原始字节对不对
inspect {"kind":"state"}                      # pc / t_ps / 寄存器
```

判据：`disasm` 出来的指令与固件编译结果一致。不一致 ⇒ 装错文件（`.hex` 文本当二进制、
或 `name` 扩展名导致 kind 判错）。

## R6 · 事件回放（不重连 WS）

```
events {"since":0,"limit":32}        → 记 next
events {"since":<next>,"limit":32}   → 继续
```

判据：`seq` 单调，帧里能看到 `ImageLoaded` / `SimStateDelta` / `BreakpointHit` 之类。
重复从 0 拉只会重复烧上下文，没有新信息。

## R7 · 复位再来一遍（需破坏性闸门）

```
tools/list                # 先确认有 reset
reset {}
load_image {"hex":"...","name":"fw.bin"}
```

判据：`tools/list` 里没有 `reset` 就**不要**尝试，直接告诉用户开
`SIM_MCP_ALLOW_DESTRUCTIVE=1` 并重启 MCP 进程。
