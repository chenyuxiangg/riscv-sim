# 第 1 阶段 · WASM 桥接口规格（冻结）

> **状态**：冻结（2026-10-21 修订——D-46 探针管道闭环，§3.1 / §3.3 / §3.4 /
> §3.4b / §6 同步；**事件形状未变**）。
> **再修订（2026-10-21 · 契约层 v2）**：契约事件变体 `EventPayload::WaveformDelta` →
> **`EventPayload::WaveformHistory`**（台账 D-34：变体名随**内容**改——它装的是 **history 形**）。
> **本表的「命令 → 事件序列」语义未变**，只是变体名改了；契约 v2 的冻结规格见
> `docs/detailed-design/STAGE-2_Contract_V2.md`。
>
> ⚠️ **本文是「第 1 阶段」的桥规格**：命令 → 事件序列的**语义**以本文为准，
> 但**线格式**（`schema_version`、虚拟时间的数值表示、变体集合、错误族、传输帧 `crc`）
> 以 **`docs/detailed-design/STAGE-2_Contract_V2.md` 为准**——契约层已升到
> **`schema_version = 2`**，**冻结已解除**。本文与 v2 规格**冲突时以 v2 规格为准**。
> 第 1 阶段两侧实现的实现依据仍是本文：Rust 侧（`crates/kernel/src/kernel_host/` +
> `wasm_api.rs`）与前端侧（`front/kernel-worker.js`）；**任何一方要改命令 → 事件序列的
> 语义，必须先改本文**。
>
> **范围**：第 1 阶段 = WASM 导出层 + 前端接真实内核 + 真机复验。
> 契约层（`crates/contract/**`）自 **v2** 起**不再冻结**（升版的破坏性变更逐条列明于
> `STAGE-2_Contract_V2.md` §1.1）——本文描述的一切仍在契约之内。

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
| M-07 传输帧 | `{type, seq, sessionId, role, reqId, crc, payload}` | 前端传输层 | `front/lib/frame.js` + `front/kernel-worker.js` |
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
- **`E_PROTO-08` / `E_PROTO-01` 的归属**：契约 v2 起它们**有族可归**——
  `ErrorFamily::Protocol`，`as_str()` → **`"PROTO"`**（与 LLD 的 `E_PROTO-0x` **逐字一致**）。
  契约的 `ErrorFamily` 现在是 **14 个族**：
  `CTX / ISS / ENG / BUS / PER / INT / DBG / OBS / SNAP / LDR / HMS /
  STORE / TRX / PROTO`。
  ⚠️ `PROTO`（**协议层**：版本协商 / 信封畸形 / CRC 不符）与 `TRX`（**传输适配器自身**
  的故障：连接 / 断线 / 超时）**并存不合并**——两个族职责不同，前端要靠它决定是
  「**重连**」还是「**报错**」。
  `EventPayload::Error.code` 仍是 `String` 字段（无族约束），但场景码的**字面量**
  已由契约的 `error` 模块固化（`E_PROTO_01_VERSION_MISMATCH` … `E_PROTO_08_BAD_MESSAGE`）。
- **出参数组长度 ≥ 1**：每条命令至少回一个事件（`Nop` → `Tick`）。
- **`seq` 由内核分配**：出参 `Message.seq` 是内核自己的单调计数器，从 `1` 起，
  每产出一条事件 +1。**不复用**入参的 `seq`（那是前端 M-05 的序号空间）。
- **会话状态**：内核在 WASM 实例内**保留一个会话**（寄存器 / 内存图 / 断点表 /
  订阅表）。同一 Worker 生命周期内的多条命令共享它。

### 2.2 契约消息的 JSON 形状（serde 默认表示）

```jsonc
// 命令：结构体变体（契约 v2：schema_version = 2）
{"schema_version":2,"direction":"Command","seq":1,"payload":{"Command":{"StepN":{"n":1}}}}
// 命令：单元变体（serde 把无字段变体序列化成裸字符串）
{"schema_version":2,"direction":"Command","seq":2,"payload":{"Command":"Nop"}}
// 事件（契约 v2：虚拟时间走十进制字符串 —— 数值策略见 STAGE-2 §2）
{"schema_version":2,"direction":"Event","seq":7,"payload":{"Event":{"Tick":{"elapsed_ps":"1000","pc":4}}}}
```

⚠️ **两处 v2 的线格式变化**（本文的第 1 阶段示例已同步）：

| 项 | v1 | **v2** |
| --- | --- | --- |
| `schema_version` | `1` | **`2`** |
| 虚拟时间（`elapsed_ps` / `t_ps` / `virtual_time_ps` / `at_ps` …） | JSON **数字** | **十进制字符串**（超过 2⁵³ 后 JSON 数字会静默丢精度） |

其余 `u64`（计数器、序列号、信号值）**保持 JSON 数字**。

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
| `LoadProject` | `Tick`。⚠️ **装载即新会话**：内核侧的主题集合被清空（FR-13），所以**不会**产生 `WaveformHistory`——全量寄存器流由**随后的** `Subscribe{sim.registers}` 触发（见 §3.2b） |
| `Continue` | `Tick`；若已订阅 `sim.registers` → `WaveformHistory`（**增量**：仅脏寄存器 + pc）；若已订阅 `sim.waveform` **且本轮有至少一条新边沿** → `WaveformHistory`（**增量**：仅探针边沿，见 §3.3；**无新边沿时不推**，避免空 delta）；若停止 → `Halted`；取指失败 → `Error{E_ISS-01}`（**取代**，见下） |
| `StepN{n}` | 同 `Continue`（把 `Continue` 换成「执行 n 条」）。⚠️ `n` 条跑完**不是**停止，不产生 `Halted` |
| `Pause` | `Tick`；`Halted{UserPause}` |
| `SetBreakpoint` | `Tick`；地址非法 / 重复注册 → `Error{E_CTX-07}`（**取代** `Tick`） |
| `ClearBreakpoint{id}` | `Tick`（id 不存在 → **幂等**，仍是 `Tick`，不报错） |
| `SetWatchpoint` | `Tick` |
| `ClearWatchpoint{id}` | `Tick`（同上，幂等） |
| `ReadMemory` | `MemoryRead{addr,size,value}`；未映射 → `Error{E_BUS-01}`（**取代**） |
| `WriteMemory` | `Tick`；未映射 → `Error{E_BUS-01}`（**取代**） |
| `Subscribe{topic}` | `Tick`；`topic=="sim.registers"` → `WaveformHistory`（**全量**）；`topic=="sim.waveform"` → `WaveformHistory`（**探针通道列表**，见 §3.3） |
| `Unsubscribe{topic}` | `Tick`（幂等） |
| `Snapshot` | `SnapshotCreated{id}`（id 单调递增）。⚠️ 第 1 阶段**只分配 id，不落盘**（M-20 仍是 stub，见 §3.7） |
| `Restore{state}` | `Tick`；`state.schema_version` 不符 → `Error{E_SNAP-05}`（**取代**） |
| `Admin{op}` | `Error{E_CTX-05}`（管理通道未启用） |

**契约 v2 新增的 3 条命令**（已接线；语义逐字取自 `STAGE-2_Contract_V2.md` §5）：

| 命令 | 事件序列（有序） |
| --- | --- |
| `ReadRegisters{index,count}` | `Registers{index,values}`。`index` / `count` **钳到** `[0, 32)`；`index` 是**实际**起点、`values.len()` 是**实际**条数；⚠️ **`count = 0` 照样回复空区间**（`values: []`）——**「不回复」与「消息丢失」不可区分**（M-05 按 `reqId` 配对，零回复 → 500 ms 假超时 `E_CTX-06`） |
| `WriteRegister{index,value}` | `Registers{index,values}`（`values.len() == 1`，值是**写完之后从引擎状态读回来的**实际值）。`index == 0` → **丢弃**（D-04）但**仍回复**（值恒为 0）；`index > 31` → **拒绝**（`Error{E_DBG-01}`，寄存器文件原样不动） |
| `Negotiate{min_schema_version,cap_bits}` | `Negotiated{schema_version,cap_bits,kernel}`；宿主的 `min_schema_version` 高于内核支持的版本 → **拒绝**（`Error{E_PROTO-01}`） |

**「取代」= 该条命令只产出这一个事件**（错误事件本身即首个事件，携带 `reqId`）。

**「取指失败 → Error」的完整序列**：`Continue` / `StepN` 在取指失败（总线错误）时产出
**4 条**事件，依次是 `Error{E_ISS-01}` → `Tick` → `WaveformHistory` → `Halted{Trap}`。
错误事件排在**最前**，因为它要携带命令的 `reqId`——M-05 靠它拒掉命令，而不是
让命令等超时。

> **本表是实现与文档的共同权威**。任何一方与本表不一致时，先改本表，再改代码——
> 反过来做会把已闭环的 D-45（装载后必须强制重订）打回原形。

### 3.2 寄存器波形流

⚠️ **契约层 v2 起，这句话已经不成立**：契约**现在有**寄存器快照事件了
（`EventPayload::Registers`，台账 D-32 缺口 1 已闭环），而且**内核侧已接线**
（`kernel_host::on_read_registers` / `on_write_register`）——见 §3.1 的两条新行。
**本节描述的是第 1 阶段遗留的「边沿编码寄存器流」**：它**仍然**是
`Subscribe{sim.registers}` 的全量推送与 `Continue` / `StepN` 的增量推送的载体
（`EventPayload::WaveformHistory`），**没有**被 `Registers` 事件取代——
两者**并存**：前者是「随仿真推进的脏寄存器流」，后者是「按需读 / 写的一等公民通道」。
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

**D-46 已闭环**：M-18 / M-19 的探针管道**真的接在总线上**了。链路是

```
guest 写外设 → M-14 派发 → M-15 判「状态真变了」→ report(signal)
   → M-18 判沿 → 边沿进 M-19 的环（EdgeSink）
   → 内核宿主取边沿 → 契约 EventPayload::WaveformHistory
```

- **通道 id** = `"{外设 id}.{信号名}"`（M-18 `drain_waveform`）。默认内存图
  里映射了一个 GPIO 外设（`GPIO0`，基址 `0x4000_0000`），所以默认只有一条
  通道：`GPIO0.OUT`。
- **通道列表与边沿走同一个事件形状**（契约冻结，没有单独的「通道列表」
  变体）。`Subscribe{sim.waveform}` 时内核**强制**推一次——哪怕一条边沿
  都还没有。前端据此区分「有这条信号，只是还没变化」与「根本没有这条
  信号」；只靠边沿是表达不出前者的。
- **增量**：`Continue` / `StepN` 推的是**自上次推送以来**的边沿。宿主侧
  必须**累加**边沿数，直接覆盖的话视图上永远只有最后一批。
- **时间戳**：边沿带的是外设上报时刻的虚拟时间。M-17 的步进循环在**每条
  指令前**同步一次外设时钟（M-14 `tick_peripherals`），所以时间轴是真实
  的、单调的——不是恒为 0 的平线。
- **退订期间的边沿不会丢**：边沿一直攒在 M-18 的存储里，重新订阅时的
  强制推送会把它们一并带出来。波形调试器不该因为「用户暂时不想被打扰」
  就丢历史。⚠️ 但「不丢」有**上限**——见下面「两层上限」。
- **调试器写内存也会产生边沿**：`WriteMemory` 写 GPIO 的 `OUT` 就是在改电平，
  内核**当场**推一条波形（不等下一轮 `Run`）。否则用户盯着波形视图却要等
  下一轮才看到自己刚写的那一下。

**⚠️ 两层上限（v0.14.1 检视轮补）**：「不丢」不是「无限」——一条 8 MB 的
事件（实测 8 000 条边沿 ≈ 219 KB）会把 WASM 的 `postMessage` 与前端的
JSON 解析同时压垮。两层各管一段：

| 层 | 位置 | 上限 | 满了怎么办 | 可见性 |
| --- | --- | --- | --- | --- |
| **内存层** | M-18 的每通道边沿存储 | `EDGE_STORE_CAP = 8192` | 丢**最旧的**（成批淘汰） | `dropped_edge_count()` |
| **事件层** | 单条 `WaveformHistory` 事件 | `MAX_WAVEFORM_EDGES_PER_EVENT = 4096` | 保留**最新的** | `waveform_edges_truncated()` |

两层的取舍不同是**有意的**：内存层管「存多少」，事件层管「一条事件多大」。
只做一层，要么内存爆，要么事件太大。**截断都是可见的计数**，不静默丢——
时间轴上出现一个洞时，能查到是哪个上限淘汰的。

> ⚠️ **M-19 的环在生产路径上目前只写不读**。边沿**同时**进两处：M-18 自己的
> 边沿存储（宿主从这里取边沿推契约事件）与 M-19 的环形缓冲（`EdgeSink`
> 端口）。**契约事件取自 M-18**；M-19 的环是为了将来的 CSV/VCD 导出装的，
> 但导出**还没接进契约**（→ 台账 **D-50**）。链路图按这个画，别画成
> 「宿主从 M-19 取边沿」。

> ⚠️ **判别式**：前端过滤寄存器流用的是「id **不是** `cpu.` 开头」，而
> **不是**「id 以 `probe.` 开头」。真内核的通道 id 是 `GPIO0.OUT` 这种
> 形状——写成只认 `probe.` 会把真通道**全部过滤掉**，波形视图永远是空的。

### 3.4 默认内存图（demo 平台）

契约的 `Project` **不带内存图描述**（产品树只是 UI 数据），所以内核自带一张
默认图。`LoadProject` 时建立：

| 区 | 基址 | 大小 | 类型 |
| --- | --- | --- | --- |
| RAM | `0x0000_0000` | 1 MiB | 复位向量 + 镜像 + 数据 |
| RAM | `0x8000_0000` | 4 MiB | 常规 RISC-V 内存（与 golden 对拍侧一致） |
| GPIO | `0x4000_0000` | 16 B | 一个 32 脚 GPIO 组（D-46）——**没有外设就没有信号可观察** |

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
| 0x10 | `0x0040006F` | `jal x0, 0x14` |
| 0x14 | `0x400002B7` | `lui x5, 0x40000` ← x5 = GPIO 基址 |
| 0x18 | `0x00100313` | `addi x6, x0, 1` |
| 0x1c | `0x0062A223` | `sw  x6, 4(x5)` ← **GPIO0.OUT = 1**（D-46 的探针边沿） |
| 0x20 | `0x00100213` | `addi x4, x0, 1` ← **断点靶点** |
| 0x24 | `0x0000006F` | `jal x0, 0` ← 自环 |

**编码订正（2026-10-20）**：本规格初稿把 0x0c 处的 `sw` 写成
`0x00302023`——那是 `sw x3, 0(x0)`，S 型立即数的 **imm[11:5]** 段漏了。
`sw x3, 0x100(x0)` 的正确编码是 `0x10302023`（imm = 0x100 → imm[11:5] = 8
在 bit[31:25]，imm[4:0] = 0 在 bit[11:7]）。前端「读 0x100」的动作依赖这一条。
前端门禁里有一条**解码级**断言（把 S 型立即数解回来比对 `0x100`），
不是只比对字面值。

**D-46 订正（2026-10-21）**：0x10..0x1c 原本是「跳到 0x20 + 三条 nop」，
现改成「跳到 0x14 + 三条 GPIO 设置指令」。**demo 固件必须真的动一下引脚**
——否则探针管道接通了也没有边沿可推，波形视图仍然是空的（只是「空的
原因」换了一个）。`0x1c` 处的 `sw x6, 4(x5)` 写的就是 GPIO 的 `OUT`
寄存器（偏移 `0x04`），M-15 在**电平真变化**时上报 `OUT` 信号。

> ⚠️ **两份固件不许漂移**：前端 `front/dev/demo-firmware.js` 与 Rust 侧
> （`crates/kernel/src/kernel_host/tests.rs` + `crates/kernel/tests/d46_probe_pipeline.rs`）
> 必须**逐字节相同**——否则前端看到的和内核跑的不是同一个程序，而两边
> 各自都有测试，绿着测不同的固件。`d46_probe_pipeline.rs` 里有一条用例
> **直接读那份 JS 比对**。

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

### 3.7 `Snapshot` 的语义（第 1 阶段：**只分配 id**）

`Snapshot` 命令回一条 `SnapshotCreated{id}`，id **单调递增**。

⚠️ **第 1 阶段它只分配 id，不真正捕获快照**：M-20 的 `SnapshotManager` 还是
stub，它的 `SnapshotRecord` 也仍吃 partial 的 `SimState`（台账 **D-37**）。
所以这个 id **指向一个不存在的实体**——后续也不能靠它恢复（契约的
`Restore{state}` 是宿主**直传**状态，不走 id）。

这是**有意留到后续阶段**的（与 §3.3 的波形流同一个写法），不是实现漏了。
真正落盘要等 M-20 / M-23 按各自 LLD §2.1.1 组装完整载荷
（`Snapshot{cpMeta, m13State, m14Map, m15Periph}`）。

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

### 4.1 宿主侧失败 → `Error{E_TRX-04}`

下列三种**宿主侧**（不是内核侧）失败，一律回**一条** `Error` 事件，错误码
**`E_TRX-04`**（M-07 的传输族 `TRX`），并**保留命令的 `reqId`**（这样 M-05
能拒掉在飞的命令，而不是等超时）：

| 情形 | 触发点 |
| --- | --- |
| WASM 初始化失败 | `ensureKernel()` 的 init 抛错 |
| 内核抛错（`process_message` 抛异常） | `process_message` 调用 |
| 内核返回值非法（非 JSON / 非数组 / 空数组） | `validateEventList()` |

- **同一个故障只报一次**：初始化失败时若有命令在飞，由那条命令的错误帧
  承担（带 `reqId`），宿主**不再**补一条启动级帧。
- ⚠️ `E_TRX-04` 在 M-07 里也用于「传输通道错误」（Worker `error` 事件 /
  WebSocket 错误）。**同码多义**是已知问题：前端无法据码区分「通道挂了」
  与「内核算错了」。本阶段不影响功能（两条路径的处置不同：前者把传输置为
  `closed`，后者拒掉单条命令），但 P1 做断线续拉时应当拆码（→ 台账 **D-47**）。

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

1. ~~**波形视图为空**~~——**v0.14 已闭环**（D-46）：真实探针管道
   （M-18/M-19 ↔ M-14 总线 hook）已接，见 §3.3。*（本条保留在列表里是
   为了让「第 1 阶段有哪些限制」一眼可见；带删除线的是已闭环项。）*
2. **`Over` / `Out` / `Line` 单步退化为指令级**——需要 DWARF 行表与调用栈（M-11）。
3. **条件断点未实现**——冻结契约的 `Breakpoint` 没有 `cond` 字段（D-32 缺口）。
4. **观察点只做地址匹配**——`Watchpoint` 有 `access` / `size` 字段，可精确匹配。
5. **管理通道未启用**——`Admin` 一律 `E_CTX-05`。
6. **`Snapshot` 只分配 id，不落盘**——见 §3.7（台账 **D-37**）。
7. **`E_PROTO-08` / `E_PROTO-01` 不属于契约的 `ErrorFamily` 族**——见 §2.1。
