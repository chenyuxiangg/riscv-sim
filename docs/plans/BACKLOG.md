# 待办台账（R-1 批之后的延期项）

> 只记**已经想清楚、但当前不做**的事；每一条写清「为什么现在不做」和「什么时候该做」。
> 日期：2026-11-27 · 来源：需求单 v2 讨论（R-1 落地后）。

## B-1 · `stop_causes`（同步异常的 cause 过滤）

**要什么**：`SetStopPolicy` 增加可选 `stop_causes: Option<Vec<u32>>`，只在
`stop_on_trap = false` 时有意义 —— 给 `[5,6,7]` 表示「野指针 / 取指 / load 类故障仍然停，
其余（尤其 `3` Breakpoint / `11` Ecall）放过去继续跑」。缺省 `None` = 现状（全停）。

**为什么现在不做**：用户 2026-11-27 判断「现在用不到」。OSZ 板上框架目前没有用
`ebreak` / `ecall` 做 semihosting、syscall 桩或 trap-and-emulate 的用例；先扩契约面
只会多一条没人走的分支。

**什么时候该做**：OSZ 报「有固件用 ebreak 做自定义服务，但默认策略把 `run` 掐断」时。
**注意时间成本**：R-1 已把 `SCHEMA_VERSION` 升到 20；这批合入后再加字段就是 21，
前端镜像 / MCP / skill 都要再跟一遍。能塞进 v20 的窗口只有合入前。

## B-2 · `stall_guard`（异常连环的提前收尾）

**要什么**：`stop_on_trap = false` 时，连续 N 次（建议 64）出现**同 `mcause` + 同
`mepc`** ⇒ 判「异常连环」报 `E_ENG-04`（带 cause / mepc / count）中止，而不是烧到
`max_trap_events`。计数**跨批处理窗口**持久（窗口边界不清零），否则一次 `run` 切窗
就失效。

**为什么现在不做**：P0 的验收（AC1/AC3/AC4）用配额就能过；`stall_guard` 是第二道防线，
且它的误报口径（合法地反复陷在同一处，如自陷式打点日志）要跟 B-1 一起定。

**什么时候该做**：R-2 收口后，或 OSZ 反馈「配额给小了误杀、给大了烧穿预算」。

## B-3 · R-2 · wasm 后端 CLINT `mtime` 64 位读偶发异常

**要什么**：先在 wasm 侧写「100 万次连读 `0x200BFF8`」的复现脚本判因
（宿主喂的 `now_ps` 跳变 vs guest 两次 32 位读未锁存），再补锁存 / 原子快照。

**为什么现在不做**：与 R-1 独立（需求单明确「可独立合入」），且未复现就没有判据。

## B-4 · `run` 的 `budget_exhausted` 之外，把「默认 1e6」写进 USER_GUIDE

**要什么**：部署约定 `POST /images` 的镜像落在**容器内** `/data/images`（volume
`riscv-sim-data → /data`），宿主没这个路径 ⇒ 要 `scp` 再 `docker cp`。写进
`docs/USER_GUIDE.md` 与 skill 的 troubleshooting。

**为什么现在不做**：属文档债，与代码批解耦，下一批文档整理时一起补。
