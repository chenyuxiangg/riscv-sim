# 第 1 阶段 · WASM 桥接口规格（冻结）

> **状态**：冻结（2026-10-21）。本文件是**第 1 阶段**两侧实现的唯一权威副本：
> Rust 侧（`crates/kernel/src/kernel_host/` + `wasm_api.rs`）与前端侧
> （`front/kernel-worker.js`）都按本文实现；任何一方要改形状，必须先改本文。
>
> **范围**：第 1 阶段 = WASM 导出层 + 前端接真实内核 + 真机复验。
> 契约层（`crates/contract/**`）**冻结不变**——本文描述的一切都在契约之内。

---

## 1. 分层：谁拥有哪一层

```
前端（浏览器）                                        内核（WASM）
┌────────────────────────────────┐        ┌───────────────────────────────┐
│ M-05 命令路由 (front/services) │        │ kernel_host（协议装配层）      │
│   ↓ 契约 Message               │        │   ↓ 契约 Message              │
│ M-07 传输适配器 (front/services)│        │ M-12/M-13/M-14/M-08/M-17     │
│   ↓ 传输帧 TransportFrame        │        │                               │
│ kernel-worker.js（传输宿主）     │──────▶ │ wasm_api::process_message()   │
└────────────────────────────────┘        └───────────────────────────────┘
```

**分层口径**（与 `front/lib/frame.js` 头注释一致）：

| 层 | 形态 | 归属 | 谁实现 |
| --- | --- | --- | --- |
| M-07 传输帧 | `{type, seq, sessionId, role, reqId, payload}` | 前端传输层 | `front/lib/frame.js` + `front/kernel-worker.js` |
| M-05 契约消息 | `riscv_sim_contract::protocol::Message` | 契约（冻结） | Rust `protocol.rs` + `front/lib/message.js` 镜像 |

**内核只认契约消息**。传输帧是前端的东西，内核**不解析**、**不认识**。
这条边界一旦模糊，`reqId` / `sessionId` 就会渗进契约层——契约冻结，不允许。

---

## 2. WASM 导出面

产物：`tools/build-wasm.sh` → `front/assets/pkg/riscv_sim_kernel.js`（`--target web`，带命名导出的原生 ESM）。

| 导出 | 签名 | 语义 |
| --- | --- | --- |
| `kernel_version()` | `() => string` | 内核版本串（诊断 / 检测陈旧 `.wasm`） |
| `protocol_schema_version()` | `() => number` | 契约 `SCHEMA_VERSION` |
| `ping(text)` | `(string) => string` | 回声，验字符串编解码 |
| **`process_message(json)`** | `(string) => string` | **本阶段的桥** |

### 2.1 `process_message(json)` 契约

- **入参**：一个契约 `Message` 的 JSON 文本，`direction` 必须是 `"Command"`。
- **出参**：一个 JSON **数组**，元素是契约 `Message`，`direction` 必须是 `"Event"`。
- **不抛异常、不 panic**：WASM 里 panic = 整个 Worker 崩。畸形输入必须变成
  一个 `Error` 事件（`E_PROTO-08`），而不是把 Worker 带走。
- **出参数组长度 ≥ 1**：每条命令至少回一个事件（`Nop` → `Tick`）。
- **`seq` 由内核分配**：出参 `Message.seq` 是内核自己的单调计数器，从 `1` 起，
  每产出一条事件 +1。**不复用**入参的 `seq`（那是前端 M-05 的序号空间）。
- **会话状态**：内核在 WASM 实例内**保留一个会话**（寄存器 / 内存图 / 断点表 /
  订阅表）。同一 Worker 生命周期内的多条命令共享它。

### 2.2 契约消息的 JSON 形状（serde 默认表示）

```jsonc
// 命令：结构体变体
{"schema_version":1,"direction":"Command","seq":1,"payload":{"Command":{"StepN":{"n":1}}}}
// 命令：单元变体（serde 把无字段变体序列化成裸字符串）
{"schema_version":1,"direction":"Command","seq":2,"payload":{"Command":"Nop"}}
// 事件
{"schema_version":1,"direction":"Event","seq":7,"payload":{"Event":{"Tick":{"elapsed_ps":1000,"pc":4}}}}
```

JS 镜像见 `front/lib/message.js`（`decodeMessage()` 是它的逆）。

---

## 3. 内核宿主（`kernel_host`）——命令 → 事件的**规范映射**

`kernel_host` **不是** 23 个详细设计模块之一，它是它们的**装配层**
（对应前端 `front/dev/mock-kernel.js` 那个「真内核的对应物」）。
详见 `crates/kernel/src/kernel_host/mod.rs` 的模块级文档。

### 3.1 规范映射表

下表是**规范**：每条命令产生的**事件序列（有序）**。「首个事件」= 携带
`reqId` 的那个（M-05 用它做命令关联，见 `front/services/m05_commandrouter.js`）。

| 命令 | 事件序列（有序） |
| --- | --- |
| `Nop` | `Tick` |
| `LoadProject` | `Tick`；若已订阅 `sim.registers` → `WaveformDelta`（**全量**寄存器 + pc） |
| `Continue` | `Tick`；若已订阅 `sim.registers` → `WaveformDelta`（**增量**：仅脏寄存器 + pc）；若停止 → `Halted`；若已订阅 `sim.waveform` → `WaveformDelta`（探针流） |
| `StepN{n}` | 同 `Continue`（把 `Continue` 换成「执行 n 条」） |
| `Pause` | `Tick`；`Halted{UserPause}` |
| `SetBreakpoint` | `Tick`；地址非法 / 重复注册 → `Error{E_CTX-07}`（**取代** `Tick`） |
| `ClearBreakpoint{id}` | `Tick`（id 不存在 → **幂等**，仍是 `Tick`，不报错） |
| `SetWatchpoint` | `Tick` |
| `ClearWatchpoint{id}` | `Tick`（同上，幂等） |
| `ReadMemory` | `MemoryRead{addr,size,value}`；未映射 → `Error{E_BUS-01}`（**取代**） |
| `WriteMemory` | `Tick`；未映射 → `Error{E_BUS-01}`（**取代**） |
| `Subscribe{topic}` | `Tick`；`topic=="sim.registers"` → `WaveformDelta`（**全量**）；`topic=="sim.waveform"` → `WaveformDelta`（探针流） |
| `Unsubscribe{topic}` | `Tick`（幂等） |
| `Snapshot` | `SnapshotCreated{id}`（id 单调递增） |
| `Restore{state}` | `Tick`；状态不合法 → `Error`（**取代**） |
| `Admin{op}` | `Error{E_CTX-05}`（管理通道未启用） |

**「取代」= 该条命令只产出这一个事件**（错误事件本身即首个事件，携带 `reqId`）。

### 3.2 寄存器波形流

契约里**没有**寄存器快照事件（D-32 缺口），所以寄存器值只能走
`EventPayload::WaveformDelta` 的边沿编码流——这是契约内**唯一**的载体。
流 id 与 mock 内核**逐字一致**，前端视图零改动：

| 流 id | label | value |
| --- | --- | --- |
| `cpu.gpr.x{i}`（i = 0..31） | `x{i}` | `gpr[i]` |
| `cpu.pc` | `pc` | `pc` |

- **全量**（`LoadProject` / `Subscribe`）：32 条 GPR + 1 条 pc。
- **增量**（`Continue` / `StepN`）：仅**脏**寄存器（值变过）+ pc（PC 变过才带）。

### 3.2b 宿主侧硬性要求：装载工程之后必须**强制**重新订阅

`LoadProject` 会**清空内核侧的主题集合**（新工程 = 新会话，FR-13）。
于是：内核侧的订阅死了，而**宿主本地**的「已订阅」标志还立着。

所以宿主**不能**用幂等的 `ensureSubscribed()`（它看到本地标志为真就什么
都不发），必须用**强制**的重新订阅。本仓库把它落在 M-10 的
`loadProject(project)` 里——**装载 + 强制重新订阅**绑成一个动作，调用点
不会漏掉后半步：

```js
await registerMemory.loadProject(project);   // 不是 dispatch + ensureSubscribed
```

> **这个缺陷在 mock 内核时代就存在**（mock 的 `reset()` 同样清主题），
> 一直是「静默陈旧」——寄存器推送停掉、视图停在旧值、不报任何错。
> 它是第 1 阶段前端接上**真内核**、并把「x1 必须是 42」写进真机断言
> （`tools/browser-verify.mjs`）之后才暴露的：**PC 会前进、x1 不动**。
>
> 这条也说明了为什么真机断言必须断言**值**，而不能只断言「内核已就绪」。

### 3.3 探针波形流

第 1 阶段**不接** M-18/M-19 探针管道（真实探针接线是后续阶段），因此
`sim.waveform` 订阅下**不产生**探针流。波形视图在第 1 阶段为空——这是**已知
限制**，登记为台账欠账，不允许用假数据糊弄。

### 3.4 默认内存图（demo 平台）

契约的 `Project` **不带内存图描述**（产品树只是 UI 数据），所以内核自带一张
默认图。`LoadProject` 时建立：

| 区 | 基址 | 大小 | 类型 |
| --- | --- | --- | --- |
| RAM | `0x0000_0000` | 1 MiB | 复位向量 + 镜像 + 数据 |
| RAM | `0x8000_0000` | 4 MiB | 常规 RISC-V 内存（与 golden 对拍侧一致） |

镜像落点：
- `ImageKind::Elf` → M-06 解析出的各段，按段自己的 `paddr` 落；`entry` 作复位向量。
- `ImageKind::RawHex` → M-06 `load_raw_at(bytes, 0)`，即**基址 0**（契约的
  `ImageRef` 没有基址字段，raw 镜像的基址由内核定为 0 = 复位向量）。

**无镜像时**：不报错，`LoadProject` 仍然成功（PC = 0，取指必然 trap）。

### 3.4b demo 固件（第 1 阶段的演示镜像）

前端 `front/dev/demo-firmware.js` 挂一个 **40 字节的 RV32I 手写小程序**
（`RawHex` 镜像，落在基址 0）：

| addr | word | 指令 |
| --- | --- | --- |
| 0x00 | `0x02A00093` | `addi x1, x0, 42` |
| 0x04 | `0x00700113` | `addi x2, x0, 7` |
| 0x08 | `0x002081B3` | `add  x3, x1, x2` |
| 0x0c | `0x10302023` | `sw  x3, 0x100(x0)` |
| 0x10 | `0x0100006F` | `jal x0, 0x20` |
| 0x14 | `0x00000013` | `nop` |
| 0x18 | `0x00000013` | `nop` |
| 0x1c | `0x00000013` | `nop` |
| 0x20 | `0x00100213` | `addi x4, x0, 1` ← **断点靶点** |
| 0x24 | `0x0000006F` | `jal x0, 0` ← 自环 |

**编码订正（2026-10-20）**：本规格初稿把 0x0c 处的 `sw` 写成
`0x00302023`——那是 `sw x3, 0(x0)`，S 型立即数的 **imm[11:5]** 段漏了。
`sw x3, 0x100(x0)` 的正确编码是 `0x10302023`（imm = 0x100 → imm[11:5] = 8
在 bit[31:25]，imm[4:0] = 0 在 bit[11:7]）。前端「读 0x100」的动作依赖这一条。
前端门禁里有一条**解码级**断言（把 S 型立即数解回来比对 `0x100`），
不是只比对字面值。

### 3.5 断点语义（M-08）

- 地址必须 **2 字节对齐**（RV32IMC：RVC 指令半字对齐）→ 否则 `E_CTX-07`。
- 同一地址**重复注册** → `E_CTX-07`。
- `ClearBreakpoint{id}`：按 **id** 删（契约的 `ClearBreakpoint` 只有 `id` 字段，
  没有 `addr`）。**id 不存在 → 幂等成功**。
- 命中判定：`hitCheck(pc)` 是**唯一**命中入口，挂在**取指之前**（M-08 LLD
  §3.2「pre=1 阶段」）。禁用的断点不命中。

### 3.6 停止原因

| 情形 | `HaltReason` |
| --- | --- |
| 命中断点 | `Breakpoint` |
| 命中观察点 | `Watchpoint` |
| 非法指令 / 总线错误 | `Trap` |
| 宿主 `Pause` | `UserPause` |
| 跑满指令预算（`Continue` 的上限） | `EndOfExecution` |

---

## 4. 前端传输宿主（`front/kernel-worker.js`）

```js
// 1. 初始化：import WASM，等待 init()
// 2. 收到传输帧 {type:"command", seq, sessionId, role, reqId, payload}
// 3. const msgs = JSON.parse(process_message(JSON.stringify(frame.payload)))
// 4. 对每个 msg 包成传输帧回传：
//    { type:"event", seq: msg.seq, sessionId: frame.sessionId,
//      role: frame.role, reqId: (i === 0 ? frame.reqId : null),
//      payload: msg }
// 5. 首个事件带 reqId（命令关联），其余带 null（推送类事件）
```

- `sessionId` / `role` **原样回传**——内核不解释它们（会话归属是 M-04 的事）。
- 传输帧的 `seq` = 事件的 `Message.seq`（与 mock 内核同口径）。
- WASM 初始化失败 → Worker 抛错，M-07 的 `error` 事件把状态置为 `closed`。

---

## 5. 前端改动清单

| 文件 | 改动 |
| --- | --- |
| `front/kernel-worker.js` | **新增**：WASM 传输宿主 |
| `front/app.js` | 挂载 URL 从 `./dev/mock-kernel-worker.js` 换成 `./kernel-worker.js`；就绪文案从「mock 内核已就绪」换成「WASM 内核已就绪」 |
| `front/services/m07_transport.js` | 默认 `workerUrl` → `"./kernel-worker.js"`；label → `"wasm worker"` |
| `front/dev/demo-firmware.js` | **新增**：demo 固件字节（RV32I 手写编码，带反汇编注释） |
| `front/dev/mock-project.js` | `images` 挂上 demo 固件（`RawHex` + `Bytes`） |
| `front/dev/mock-kernel*.js` | **保留**为测试替身（协议语义参照物），不再是 App 默认路径 |
| `tools/browser-verify.mjs` | 断言从「mock 内核已就绪」改成「WASM 内核已就绪」 |

---

## 6. 已知限制（第 1 阶段）

1. **波形视图为空**——真实探针管道（M-18/M-19 ↔ M-14 总线 hook）未接。
2. **`Over` / `Out` / `Line` 单步退化为指令级**——需要 DWARF 行表与调用栈（M-11）。
3. **条件断点未实现**——冻结契约的 `Breakpoint` 没有 `cond` 字段（D-32 缺口）。
4. **观察点只做地址匹配**——`Watchpoint` 有 `access` / `size` 字段，可精确匹配。
5. **管理通道未启用**——`Admin` 一律 `E_CTX-05`。
