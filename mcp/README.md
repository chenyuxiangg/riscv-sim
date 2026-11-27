# sim-mcp · AI 接入层

把 `riscv_sim` 的 HTTP 面收成 **10 个 MCP 工具**，让 AI（Claude Code / Cursor / 自研 agent）
能驱动一个真的在跑的仿真器。对应图：`docs/architecture/mcp-view.svg`。

## 它是什么，不是什么

- **是**：一层薄适配。工具只翻译 `Command` 信封的变体，不重新设计能力面。
- **不是**：第二套 API。仿真器仍然是唯一真相，工具层不认识 HTTP 路由之外的语义。
- **不是**：公网服务。默认只绑 `127.0.0.1:8877`，靠 SSH 隧道进来。

## 两条令牌，别混

| 令牌 | 谁持有 | 能干什么 |
|---|---|---|
| `SIM_TOKEN` | **只有本进程**（env） | 建/删/查所有会话、跑任意固件 |
| `SIM_MCP_TOKEN` | 客户端 | 只能调 10 个工具，且只见自己的会话 |

启动时若两条令牌相同 ⇒ 直接拒绝启动（那等于把两层凭据退化成一层）。

## 工具面（写细分 / 读合并）

`boot` · `load_image` · `advance` · `run` · `stop_policy` · `poll_job` · `inspect` · `disasm` · `breakpoints` · `events`
（`reset` / `drop_session` 属破坏性，需 `SIM_MCP_ALLOW_DESTRUCTIVE=1` 才注册）

`run` 只回 `job`，长任务用 `poll_job` —— 因为 `max_run_ms` 默认 5000，同步等必然撞墙钟。

`stop_policy` 是**会话级粘性命令**（契约 v20 · R-1）：`stop_on_trap=false` 让同步异常
（野指针 / 非法指令 / 缺页）交付后继续跑 handler，**必须**同时给 `max_trap_events`
配额；换工程 / 卸载回默认。看异常路径的标准套路：

```
stop_policy {"stop_on_trap": false, "max_trap_events": 1024, "stall_guard": 32}
run          {"prefix": "[SZTEST] end", "steps": 8000000}
```

`stall_guard`（B-2 · 异常连环守卫）是可选阈值：同一 `(mcause, mepc)` 连续陷到 N 次
（N ≥ 2）就报 `E_ENG-04` 中止这一批 —— 意思是 handler 一直在把程序送回故障点，
**跨批处理窗口累计**（窗口边界不清零），所以比配额早得多、也省得多（配额 1024 ×
每条 handler 十几条 = 十几万条指令）。只对同步异常生效，中断不进。

停在统计阈值后，`run` 回体的 `stop_reason` 与停点一起给出；**平台中止**（`E_ENG-03`
无进展 / `E_ENG-04` 异常连环）回体是 `stop_reason="error"` + `error_code`，不要读成
`halt` —— 那不是 guest 停点。事件流里有 `TrapSummary {count, first_cause, first_mepc,
first_mtvec_target}`（默认策略下**不发**）。

## 客户端手上的 ELF 怎么装（文件上行）

工具参数里塞文件是错的：100 KB 的 ELF base64 之后是 133 K 字符，上下文先烧光。所以
**字节流走 HTTP**，同一个令牌：

```bash
curl -X POST http://127.0.0.1:18877/images \
  -H "Authorization: Bearer $SIM_MCP_TOKEN" \
  -H "x-image-name: fw.elf" \
  --data-binary @fw.elf
```

回体 `{ok, bytes, kind, entry, segments, session_id}`，装完即复位到 `entry`。

**装载只有这一条路**（skill 也这么要求）：`load_image` 只在 8 字节基线自检时用 `hex`。
协议里的 `b64` / `path` 还留着（历史兼容），但不要拿它们装真文件 —— 一个烧上下文，
一个依赖「文件已经在服务器上」。工具数仍是 10 个。

## 起了它之后怎么验

```bash
# 本机（或隧道之后）—— 一条命令就是验收线
python3 tools/check_mcp.py --url http://127.0.0.1:18877/mcp --token "$SIM_MCP_TOKEN"
```

期望：`装固件 → advance(2) → pc=8 · x5=3`（8 字节 RawHex：`addi x5,x0,2` + `addi x5,x5,1`）。

## 客户端配置（Streamable HTTP）

```json
{
  "mcpServers": {
    "riscv-sim": {
      "type": "http",
      "url": "http://127.0.0.1:18877/mcp",
      "headers": { "Authorization": "Bearer <SIM_MCP_TOKEN>" }
    }
  }
}
```

客户端不支持自定义 header 时，用退化形态：`http://127.0.0.1:18877/mcp/<SIM_MCP_TOKEN>`。
隧道：`ssh -f -N -L 18877:127.0.0.1:8877 ali_hy`。

## 不变量（破了就等于没这一层）

1. 客户端永远拿不到 `SIM_TOKEN`；
2. 一个客户端一个会话，永不 `list` 别人的；
3. 破坏性工具默认不注册；
4. 每次调用一行审计（client / tool / 耗时 / 错误码）。
