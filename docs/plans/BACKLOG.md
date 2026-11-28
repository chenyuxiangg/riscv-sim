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

## B-2 · `stall_guard`（异常连环的提前收尾）—— **已落地**

**要什么**：`stop_on_trap = false` 时，连续 N 次**同 `mcause` + 同 `mepc`** ⇒ 判
「异常连环」报 `E_ENG-04`（带 cause / mepc / count）中止，而不是烧到
`max_trap_events`。计数**跨批处理窗口**持久（窗口边界不清零），否则一次 `run` 切窗
就失效。

**怎么做的**：`SetStopPolicy` 加第三个字段 `stall_guard: Option<u32>`（`serde(default)`，
不升 schema）；`TrapPolicy` 记 `last_trap_key` / `stall_count`；`step()` 在记账后、配额
前判守卫；`reset_trap_window()` **不清**连环账；`set_stop_policy` 重置。宿主 `dispatch`
拒两种误用（`stop_on_trap=true` 时给、阈值 < 2）⇒ `E_PROTO-08`；MCP 同一套判据 ⇒
`E_MCP-07`。`run` 回体把平台中止翻成 `stop_reason="error"` + `error_code`（此前只会
说 `halt`）。

**剩下的**：默认值仍建议给（工具面文案写 `32` 起步，代码不设默认 ⇒ 缺省关）；
B-1 的 `stop_causes` 落地后可把守卫限定到 fault 类 cause，进一步压误报（合法地反复
陷同一处：自陷式打点 / 轮询 semihosting）。

## B-3 · R-2 · wasm 后端 CLINT `mtime` 64 位读偶发异常 —— **已做（判因+补丁）**

**判因结论**：`now_ps` 只有一处写入（`m17_backend::step` 每条退休指令 +1000 ps），
没有跳时入口 ⇒ 「宿主喂的 `now_ps` 跳变」不成立；结构性入口只剩**两次独立重算**
（`read_word` 对 `0xBFF8`/`0xBFFC` 各调一次 `mtime_cycles()`）。已改为
**读低半时锁存高半**（`mtime_hi_latch`，`Cell<Option<u32>>`），并在
`set_mtime_cycles` / `restore_session_state` 里作废锁存。

**复现情况（诚实记录）**：本机重编的 wasm 产物（schema 20）上
`tools/r2_mtime_probe.mjs` 跑 100 万次（修前/修后）都是单调、最大单步差 1 cycle；
把 OSZ 的 `osz_red.elf` 在 wasm 里跑完整套件（8 pass / 1 crash）时 `ms=` 全是 0/1，
**没有**出现 `0x12A7D7FBECD0` 那个量级。⇒ OSZ 报告的那一次异常值**未在本机复现**，
补丁是按「结构性根因」修的（撕裂读窗口存在，只是概率极低）。

**待办（需要 OSZ 侧配合）**：要他们出事那版的 wasm 产物（或 commit）+ 板子 + 三条
读的相邻时间戳，否则只能停在「结构已消掉、症状未复现」。

**另外发现（与本项无关，记在这里免得丢）**：`Continue` 一个窗口 = **恰好一个指令预算**
（`DEFAULT_INSTRUCTION_BUDGET`，实测 10 000 条 = 10 000 000 ps），窗口跑满后
**无条件**回 `Halted{EndOfExecution}`（`execute.rs` 里那句「`Continue` 跑到预算
上限也是一个合法的停止条件，契约里最接近的取值是 `EndOfExecution`」）。实测同一
状态连发三次 `Continue`：ps 20M → 30M → 40M、pc 一路在走 ⇒ **机器没停**，
`Halted` 只是「本窗口预算用尽」。

⇒ 这是**语义重载**（`EndOfExecution` 同时表示「程序跑完」和「窗口到点」），不是引擎
停滞。代价：
- 浏览器侧一次「运行」只覆盖 10k 条，得驱动方自己再发（前端按「窗口结束」处理，
  见 `front/tests/{stale-surfaces,readback}.test.mjs`）；
- 服务端 `run` 用显式 `steps` 分块绕开了它，但 `stop_reason="end"` 也可能只是预算用尽；
- 第三方驱动（`agent-bridge`/MCP）若按「没 halt 就继续」写循环，会在 10k 条处误判「执行结束」。

两种收口（需统筹方拍板）：① 契约加「本窗口预算用尽」的非终止语义（`HaltReason`
加变体 ⇒ `SCHEMA_VERSION` 升 21，OSZ 侧要同步）；② v20 冻结期不动契约，只把
「`Continue` = 一个预算窗口」写成驱动层不变量 + 文档口径，谁用谁循环。

## B-4 · `run` 的 `budget_exhausted` 之外，把「默认 1e6」写进 USER_GUIDE

**要什么**：部署约定 `POST /images` 的镜像落在**容器内** `/data/images`（volume
`riscv-sim-data → /data`），宿主没这个路径 ⇒ 要 `scp` 再 `docker cp`。写进
`docs/USER_GUIDE.md` 与 skill 的 troubleshooting。

**为什么现在不做**：属文档债，与代码批解耦，下一批文档整理时一起补。
