# 契约层 v2 规格（`schema_version = 2`）

> **状态**：**冻结**（本文件是本轮施工的唯一依据；实现与本文件不一致即为缺陷）
> **来源**：台账 **D-32**（契约缺口 7 项）+ **D-34**（命名歧义）+ **D-39 / D-40**（快照校验）
> + **D-57 / D-58**（契约端口文档与实现口径偏离）
> **基线**：`main` = `2793e0e`（v0.16 M+C 扩展落地后）
> **分支**：`feature/contract-schema-v2`
>
> ⚠️ **本文件是「接口冻结」文档**——体例同 `STAGE-1_Wasm_Bridge.md`。
> 契约层自 v0.10 起一直是**冻结**的（`schema_version = 1`）；本轮是**第一次**升版，
> 因此**破坏性变更是允许的**，但必须**逐条写明**。

---

## 1. 为什么现在做

契约层冻结期间积压了 **9 条**台账，其中 **6 条**都卡在「契约层不升版就改不了」：

| 台账 | 卡在契约层的哪一点 | 本批处置 |
| --- | --- | --- |
| **D-32** | 契约缺口 7 项本身 | ✅ 本批全部闭环（§3~§7） |
| **D-34** | `Waveform` / `WaveformDelta` 命名歧义 | ✅ 本批闭环（§6） |
| **D-39** | `validate()` 少一条检查 | ✅ 本批闭环（§8） |
| **D-40** | 空快照无法被显式拒绝 | ✅ 本批闭环（§8） |
| **D-57** | `continue_until_event` 的端口文档 | ✅ 本批闭环（§9.1） |
| **D-58** | `Peripheral::tick` 的端口文档 | ✅ 本批闭环（§9.2） |
| **D-36** | 快照 DTO 不含外设 / INTC 状态 | ⏳ **留给 D-37 批**（依赖 M-15 / M-16 的状态导出接口） |
| **D-41** | 契约 `SimState` 缺「内存页 / 外设状态」 | ⏳ **留给 D-37 批**（与 D-36 同批，避免「契约里有、没人产」的半截状态） |
| **D-64** | `SimState` 缺 `mtval` 字段 | ⏳ **留给 M-16 批**（见 §9.3 的理由） |

### 1.1 契约层「只增不改」规则的边界

`schema_version = 1` 时代，契约层遵守「**公开 API 只增不减**」。本轮升到 **2**，
以下**破坏性变更**被**允许**（每一处都在本文件里写明）：

1. §4 的命令 / 事件**新增**（新增本身是加法，但会让「变体集合」变化 → 前端镜像必须跟）。
2. §5 的**数值策略**变更：虚拟时间字段的**线上表示**从 JSON 数字改成**十进制字符串**。
3. §6 的**改名**：`model::Waveform` → `model::WaveformHistory`；
   `EventPayload::WaveformDelta` → `EventPayload::WaveformHistory`；topic `waveform.delta`
   → `waveform.history`。
4. §7 的 `ErrorFamily` 增一个**族**（`Protocol`）。

**破坏性变更之外的一切仍然是加法**：不删字段、不改字段语义、不改既有事件的载荷形状。

---

## 2. 数值策略：为什么虚拟时间必须走字符串

### 2.1 问题

契约里的虚拟时间量是 `u64`（皮秒）。Rust 侧 `serde_json` 把 `u64` 序列化成
**JSON 数字**；JS 侧 `JSON.parse` 把 JSON 数字解析成 **IEEE-754 double**，
**只能精确表示到 2⁵³**。

```
2^53 ps = 9 007 199 254 740 992 ps ≈ 9 007 s ≈ 2.5 小时仿真时间
```

量子是 **1 µs**，所以**一次连续仿真 2.5 小时就会越过 2⁵³**——这不是理论值，
是可达的。越过之后 `elapsed_ps` / `t_ps` 会**静默丢精度**（差值变成 1 000 000 之类
的非整数量），波形时间轴会出现**跳变**。

### 2.2 规则

> **凡是「随仿真时长增长」的量（虚拟时间），在线上表示为十进制字符串。**
> 其余 `u64`（计数器、序列号、信号值）保持 **JSON 数字**。

**走字符串的字段（8 个）**：

| 字段 | 类型 | 所在 |
| --- | --- | --- |
| `virtual_time_ps` | `u64` | `model::SimState` |
| `elapsed_ps` | `u64` | `protocol::EventPayload::Tick` |
| `t_ps` | `u64` | `model::Edge` |
| `t_ps` | `u64` | `model::EdgeEvent` |
| `t_ps` | `u64` | `model::TraceEvent` |
| `t_virt` | `u64` | `model::WaveformDelta` |
| `at_ps` | `u64` | `snapshot::ScheduledEvent` |
| `at_ps` | `u64` | `snapshot::EventQueueEntry` |

**保持数字的字段（其余全部 `u64`）**，连同各自的**可达上界**（写进契约文档）：

| 字段 | 可达上界 | 结论 |
| --- | --- | --- |
| `Message.seq` | 2⁵³ 条消息 ≈ 285 年 @1000 msg/s | 安全 |
| `SimState.event_queue_head` | 2⁵³ 次入队 | 安全 |
| `SimState.seed` | 由宿主给定，写入即定值 | 安全（宿主自行取值） |
| `Edge.value` / `EdgeEvent.value` | 模拟信号值，MVP 内 ≤ 32 位 | 安全 |
| `WaveformDelta.seq_start/seq_end`、`DropRange.seq_start/seq_end` | 同 `seq` | 安全 |
| `EventPayload::SnapshotCreated.id` | 快照计数 | 安全 |

### 2.3 Rust 侧实现口径

**Rust 类型不变**（仍是 `u64`）——只改**线上表示**。用 `#[serde(with = ...)]`
挂在字段上，**kernel 内部零改动**（`u64` 的算术、比较、赋值全部照旧）。

新增契约内私有辅助模块 `crates/contract/src/numstr.rs`：

```rust
/// 以十进制字符串传输一个 `u64`（见 §2.2 的数值策略）。
pub mod u64_decimal {
    pub fn serialize<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error>;
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error>;
}
```

- **序列化**：一律输出十进制字符串（`"9007199254740992"`）。
- **反序列化**：**接受字符串**，也**接受数字**（兼容旧生产者）——但数字形态本身
  已经丢过精度，所以文档要写明「数字形态只保证 ≤ 2⁵³ 精确」。

### 2.4 JS 侧实现口径

`front/lib/message.js` 增两个助手：

```js
/** 把一个虚拟时间量编码成线上表示（十进制字符串）。 */
export function ps(value) { return String(value); }

/**
 * 把线上表示的虚拟时间读回 JS number。
 * 超过 2^53 时**抛错**（E_PROTO-04）而不是静默丢精度。
 */
export function psToNumber(text) { ... }
```

⚠️ **`psToNumber` 必须抛错而不是四舍五入**——静默丢精度是本条要消灭的**正是**那种行为。

⚠️ **错误码是 `E_PROTO-04`（输入超限），不是 `E_PROTO-02`**（规格订正 · 收口自查）。
初稿写的是 `E_PROTO-02`，那是**错的**：`E_PROTO-02` 在 LLD 里的原义是
「**未知字段** → 忽略、**仅日志**」，而「未知字段」在契约里恰恰是**不抛错**的
（只增不改原则）。拿一个「仅日志」的码去**抛错**，语义是反的。

「数值超出消费者能精确表示的范围」对应的是 **`E_PROTO-04`（输入超限）**——
输入的**大小**超过了这个消费者能处理的**上限**。为此 `E_PROTO-04` 的口径
在 LLD 里被明确拓宽为「输入超限（大小/复杂度上限）**或数值超出消费者可精确
表示的范围**」。

⚠️ **反序列化侧不受影响**：契约的 `numstr::u64_decimal` 接受数字形态（≤ 2⁵³
精确），**不抛错**——那只是精度**已经**丢失的旧生产者的形态，不是「超限」。
「超限」是**消费者侧**的判断，发生在 JS 侧。

---

## 3. 契约缺口 7 项 · 总览

| # | 缺口（台账原文） | 本批处置 | 章节 |
| --- | --- | --- | --- |
| 1 | 无寄存器快照**事件 / 命令** | 加 `CommandPayload::ReadRegisters` + `EventPayload::Registers` | §3.1 / §4.2 |
| 2 | 无**写寄存器**命令 | 加 `CommandPayload::WriteRegister` | §3.2 |
| 3 | 无 `negotiate` | 加 `CommandPayload::Negotiate` + `EventPayload::Negotiated` | §3.3 / §4.3 |
| 4 | `u64` 超 2⁵³ 精度 | 虚拟时间走十进制字符串 | §2 |
| 5 | LLD 的 `E_PROTO` 族在契约 `ErrorFamily` 无对应 | 加 `ErrorFamily::Protocol`（`"PROTO"`） | §7 |
| 6 | `crc` 未实现 | 传输帧加 `crc` 字段（**前端 `frame.js`**，不是契约信封） | §7.2 |
| 7 | 无 `simStateDelta` 变体 | 加 `EventPayload::SimStateDelta` | §4.4 |

---

## 4. 命令 / 事件新增（**逐字规格**）

> 变体名、形状（unit / newtype / struct）、字段名**逐字**给出——前端镜像与 Rust
> 必须按此拼写（`front/tests/contract.test.mjs` 会拿 Rust 源码与 JS 镜像对拍）。

### 4.1 `CommandPayload` 新增 3 个变体

```rust
    /// Read a range of general-purpose registers (M-10).
    ///
    /// The reply is [`EventPayload::Registers`]. `index` + `count`
    /// are clamped to the 32-register file; the reply always reports
    /// the **actual** start index it used.
    ReadRegisters {
        /// Index of the first register to read (0 = x0).
        index: u32,
        /// How many consecutive registers to read.
        count: u32,
    },
    /// Write one general-purpose register (M-10).
    ///
    /// Writing x0 is discarded (D-04). The reply is
    /// [`EventPayload::Registers`] carrying the register's new value.
    WriteRegister {
        /// Index of the register to write (0..=31).
        index: u32,
        /// The 32-bit value to write.
        value: u32,
    },
    /// Negotiate the protocol schema version and the session
    /// capabilities (M-05).
    ///
    /// The reply is [`EventPayload::Negotiated`]. The host sends the
    /// **lowest** schema version it can speak; the kernel answers with
    /// the version it settled on.
    Negotiate {
        /// The lowest schema version the host can speak.
        min_schema_version: u32,
        /// Capability bits the host offers (M-05 `capBits`).
        cap_bits: u64,
    },
```

### 4.2 `EventPayload` 新增 `Registers`

```rust
    /// A batch of general-purpose register values (M-10).
    ///
    /// This is the **first-class** register carrier: before v2 the
    /// front-end had to read registers out of the edge-encoded
    /// waveform streams (the contract had no register event), which
    /// conflated "signal history" with "register snapshot".
    Registers {
        /// Index of the first register in `values` (0 = x0).
        index: u32,
        /// The register values, in ascending index order.
        values: Vec<u32>,
    },
```

### 4.3 `EventPayload` 新增 `Negotiated`

```rust
    /// The negotiation result (M-05).
    Negotiated {
        /// The schema version the kernel settled on.
        schema_version: u32,
        /// The capability bits the kernel grants.
        cap_bits: u64,
        /// Kernel build / ISA descriptor (e.g. `"riscv_sim_kernel/0.1.0 RV32IMC"`).
        kernel: String,
    },
```

### 4.4 `EventPayload` 新增 `SimStateDelta`

```rust
    /// An incremental simulation-state update (M-02 `onDelta`).
    ///
    /// The LLD's `simStateDelta` has been referenced by M-02 / M-03 /
    /// M-10 since v0.11 but the contract never carried it — the
    /// front-end's register view was rebuilt from waveform edges.
    SimStateDelta {
        /// The registers that changed, ascending index order.
        registers: Vec<DeltaRegister>,
        /// The new program counter, when it changed.
        pc: Option<u32>,
        /// Virtual time of the update — **decimal string** (§2.2).
        t_ps: u64,
    },
```

配套新 DTO（放 `model.rs`）：

```rust
/// One changed register inside a [`crate::protocol::EventPayload::SimStateDelta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaRegister {
    /// Register index (0..=31).
    pub index: u32,
    /// The new value.
    pub value: u32,
}
```

### 4.5 前端 topic 映射

⚠️ **这里有两个不同的 topic 命名空间，不要混**（规格订正 · 收口自查）：

| 命名空间 | 谁用 | 取值风格 | 作用 |
| --- | --- | --- | --- |
| **订阅 topic** | 宿主发给内核的 `Subscribe { topic }` | `sim.registers` / `sim.waveform` | 内核据此决定**推什么** |
| **事件 topic** | 前端 `message.js` 的 `EVENT_TOPIC`（变体 → 路由键） | `tick` / `halted` / `memory.read` | 前端据此决定**派给谁** |

两者是**各自独立**的，历史上就并存（v1 的 `EVENT_TOPIC.WaveformDelta` 是
`waveform.delta`，而内核的波形订阅 topic 是 `sim.waveform`）。

**事件 topic 映射**（`front/lib/message.js` 的 `EVENT_TOPIC`）：

| 事件变体 | 事件 topic |
| --- | --- |
| `Registers` | `sim.registers` |
| `Negotiated` | `sim.negotiated` |
| `SimStateDelta` | `sim.state.delta` |
| `WaveformHistory`（改名后） | `waveform.history`（原 `waveform.delta`） |

⚠️ **`Registers` 的事件 topic 故意取 `sim.registers`**——它与**订阅 topic**
里的 `sim.registers` **同名**。这样一来，一条 `Registers` 回复事件（无论是
`ReadRegisters` 的回复还是内核的推送）都会被派到 M-10 订阅的那个 topic 上，
M-10 的处理器**自动**把它落到视图状态里，而不必再写一条专门的旁路。
这是**有意的重合**，不是笔误。

⚠️ `Negotiated` / `SimStateDelta` 的 `sim.*` 风格与 `tick` / `halted` 这类
短名**不统一**——这是事实记录，不是本批要统一的东西（统一命名是独立的一
笔欠账，不在契约 v2 的范围内）。

---

## 5. 命令 / 事件的**内核接线**语义

| 命令 | 内核行为 | 回复事件 |
| --- | --- | --- |
| `ReadRegisters { index, count }` | 读 M-13 的 `gpr`；`index`/`count` **钳到** `[0, 32)`；`index` 钳到 `0..31`；`count = 0` → **回复空区间**（`values: []`） | `Registers { index, values }`（`index` = **实际**起点，`values.len()` = 实际条数） |
| `WriteRegister { index, value }` | 写 M-13 的 `gpr`；`index == 0` → **丢弃**（D-04）但**仍回复**（值恒为 0）；`index > 31` → **拒绝**（`E_DBG-01`） | `Registers { index, values }`（`values.len() == 1`） |
| `Negotiate { min_schema_version, cap_bits }` | 宿主 `min_schema_version > SCHEMA_VERSION` → **拒绝**（`E_PROTO-01`）；否则回 `SCHEMA_VERSION` 与内核授予的 `cap_bits` | `Negotiated { schema_version, cap_bits, kernel }` |

⚠️ **`WriteRegister` 的 `index > 31` 是拒绝而不是钳位**——写钳位会**静默**写到别的寄存器。
`ReadRegisters` 则是**钳位**（读越界没有副作用）。

⚠️ **`count = 0` 必须回复空区间，不能不回复**（规格订正 · 收口自查）。
初稿写的是「`count = 0` → 不回复」，那是**错的**：M-05 的命令路由按
`reqId` 配对请求与回复，**没有回复**与**消息丢失**在协议上**不可区分**——
`dispatchCommand` 会在 500 ms 后以 `E_CTX-06`（命令超时）拒绝。
于是一个完全合法的请求（「读 0 个寄存器」）会变成一条**假超时**，
用户看到的是「内核没应答」而不是「读到了空区间」。

M-05 的回复模型只有两种：一条配对的回复，或一条 `Error`。**「不回复」不在模型里**，
它等价于传输故障。所以 `count = 0` 回复 `Registers { index, values: [] }`：
请求被**回答**了，区间是**空**的——两件事都说清楚了。

⚠️ **错误码用契约常量，不要裸字符串**：

| 码 | 契约常量 | 含义 |
| --- | --- | --- |
| `E_DBG-01` | `error::E_DBG_01_INVALID_REGISTER_INDEX` | 寄存器下标越界（**写**才拒绝） |
| `E_PROTO-01` | `error::E_PROTO_01_VERSION_MISMATCH` | 协议版本不匹配 |

`ErrorFamily::Debug`（`"DBG"`）是契约**早已声明**的调试服务族（M-08 / M-09 / M-10），
M-09 也已声明 `FAMILY = ErrorFamily::Debug`；但 LLD 的错误码表此前**只列了**
`CTX / BUS / ISS / PP / PROTO` 五族，**没有 DBG 的场景码**。本批把
`E_DBG-01` 固化进契约（族 + 场景码一起），并要求在 LLD 错误码表补 DBG 一行
——**族与场景码必须同时有落点**，只给族等于只给了一半（D-32 缺口 5 的教训）。

---

## 6. D-34 命名歧义的处置

**现状**：`model.rs` 里同时有 `Waveform`（history 形：`schema_version + streams[]`）
与 `WaveformDelta`（live 批次形：`seq_start/seq_end/t_virt/entries`），而
`EventPayload::WaveformDelta` 装的是**前者**——名字对不上内容。

**处置**（两个名字各归其位，**事件变体随内容改名**）：

| v1 | v2 | 说明 |
| --- | --- | --- |
| `model::Waveform` | **`model::WaveformHistory`** | history 形（wire 上真正在用的） |
| `model::WaveformDelta` | `model::WaveformDelta`（不变） | live 批次形（M-19 → M-03 推送） |
| `EventPayload::WaveformDelta(Waveform)` | **`EventPayload::WaveformHistory(WaveformHistory)`** | 变体名随**内容**改 |
| topic `waveform.delta` | **`waveform.history`** | 跟随变体名 |

> ⚠️ `model::WaveformDelta`（live 批次形）**本批不上线**——M-19 的
> `DeltaSink::push_delta` 路径尚未接进契约事件（见 D-50）。它继续留在契约里
> 作为 M-11 / D-50 的落点，**不是死代码**（有 `push_delta` 的实现方与测试）。

---

## 7. 错误族与传输帧

### 7.1 `ErrorFamily::Protocol`（对应 LLD 的 `E_PROTO-*`）

```rust
    /// Protocol-level errors: schema-version negotiation, envelope
    /// validation, transport-frame integrity (M-05, M-07).
    ///
    /// This is the family the LLD's `E_PROTO-01`..`E_PROTO-08` codes
    /// belong to. Before v2 the contract had **no** family for them
    /// (the transport family is `TRX`), so the LLD's codes had no
    /// contract-side home.
    Protocol,
```

`as_str()` → **`"PROTO"`**（与 LLD 的 `E_PROTO-0x` 逐字一致）。

⚠️ **与既有的 `Transport`（`"TRX"`）并存**：两个族**职责不同**——
`TRX` 是**传输适配器**自身的故障（连接、断线、超时），`PROTO` 是**协议层**
的故障（版本不协商、信封畸形、CRC 不符）。

**族与场景码一起落地**：契约的 `error` 模块同时给出
`E_PROTO_01_VERSION_MISMATCH` … `E_PROTO_08_BAD_MESSAGE` 八个场景常量
（含义逐字抄自 LLD 第 6 章的错误码表）。只给族不给场景码，等于只给了一半落点——
内核仍然只能把 `"E_PROTO-01"` 当裸字符串拼，那正是这类拼写漂移的来源。

另加 `E_DBG_01_INVALID_REGISTER_INDEX`（`ErrorFamily::Debug`）：LLD 错误码表
此前没有 DBG 族，但契约的 `ErrorFamily::Debug` 文档本就点名 M-08 / M-09 / M-10，
M-09 也已声明 `FAMILY = ErrorFamily::Debug`——本批把它的**第一个场景码**定下来。

### 7.2 传输帧的 `crc`（**属于 `front/lib/frame.js`，不属于契约信封**）

LLD（M-07 §2.1.1）把 `crc` 放在 **`TransportFrame`** 上，不在契约的 `Message` 上：

```
TransportFrame { type, seq, sessionId, role, reqId, crc, payload }
```

**规格**：

- **算法**：**CRC-32**（IEEE 802.3 / zlib 多项式 `0xEDB88320`，反射式）。
- **覆盖范围**：帧的 **`payload`**（即契约消息）的**规范 JSON 序列化**。
  - 规范 JSON = `JSON.stringify(payload)`，**键按字典序**（`JSON.stringify` 的
    插入序不稳定 → 必须先**递归排序键**）。
  - 只覆盖 payload：`sessionId` / `role` / `reqId` 是**会话绑定信息**，由 M-04
    负责；`seq` 由 M-05 的缺口检测负责。CRC 的职责是「**载荷在传输中没被损坏**」。
- **表示**：**8 位小写十六进制**字符串（无 `0x` 前缀）。
- **生产**：`encodeFrame` 计算并写入。
- **消费**：`decodeFrame` 校验；不符 → 抛 **`E_PROTO-06`**。
- **兼容**：**没有 `crc` 字段的帧照常接受**（旧生产者；E_PROTO-02 仅日志）——
  「只增不改」原则。有 `crc` 但**算错** → **拒绝**（E_PROTO-06）。
- ⚠️ **两端都在 JS 里**：内核只认契约消息，帧是**前端**的包裹层，
  所以 `frame.js` 一处实现即覆盖 worker 与主线程两侧。

---

## 8. 快照 DTO 的两条小修（D-39 / D-40）

### 8.1 D-39：`validate()` 增加「队列最早到期时刻 ≥ 当前虚拟时间」

`EngineSnapshotDto::validate()` 增一条检查：事件队列里**最早**的 `at_ps`
必须 **≥** `state.virtual_time_ps`。否则报 `E_SNAP_02`（队列畸形码，复用，
不新开场景码）。

### 8.2 D-40：`EngineSnapshotDto::is_empty()`

加一个 `pub fn is_empty(&self) -> bool`，让宿主能**显式拒绝**空快照
（`default()` 造出来的「合法但空」的快照）。**不加** `Default` 的破坏性移除
（那是 breaking change，本批不做）。

---

## 9. 契约端口文档与实现口径的订正（D-57 / D-58）

### 9.1 D-57：`ExecutionBackend::continue_until_event` 的文档

**现状**：契约写「Run until the next breakpoint, watchpoint, trap, or **quantum
boundary**」；实现跑到**指令预算**耗尽（D-07 确立的「唯一主循环 + 预算」语义）。

**处置**：**改契约文档**（不改实现）。理由：改实现会推翻 D-07 刚闭环的
`d07_time_driven.rs` 31 条用例所钉的语义；且「量子边界」在 D-07 里是
**抢占点**而不是**停止条件**。

**新措辞**：

> Run until the next breakpoint, watchpoint, or trap, **or until the
> engine's instruction budget for this run window is exhausted**.
> The quantum boundary is a **preemption point** (the scheduler may
> yield there), **not** a stop condition.

### 9.2 D-58：`Peripheral::tick` 的文档

**现状**：契约写「Called once per quantum (1µs by default)」；实现是
**每条指令一次**（D-46 为让探针边沿带上精确到单条指令的时间戳而改）。

**处置**：**改契约文档**（不改实现）。理由：降回每量子一次会**牺牲边沿时间戳
精度**——那正是 D-46 接线换来的东西。

**新措辞**：

> Called on **every instruction boundary** (and on every quantum
> boundary). The default quantum is 1 µs, but the tick is **not**
> quantised to it: the probe pipeline needs edge timestamps that are
> exact to a single instruction, so the tick runs at instruction
> granularity. Implementations must therefore be **cheap** and
> **idempotent within one virtual instant**.

⚠️ 「同一虚拟时刻被 tick 两次」是 D-07 修掉的缺陷（指令边界跳过刚被事件
tick 过的外设）——新措辞里的「idempotent within one virtual instant」要与之相容。

### 9.3 D-64（mtval）**本批不做**的理由

D-64 说「随 M-16 一并做」。本批**刻意不加** `SimState.mtval` 字段：

- M-16 与陷阱投递**都还没接线**，加了就是**永远为 0 的字段**——
  与 **D-66**（`Decoded.compressed` / `fetch_size()` / `IntegerDivideByZero`
  三个死接口）**同类**；
- 字段一旦进契约就是**公开 API**，事后删除又是一次破坏性变更。

**待 M-16 落地时，与 D-63（取指未对齐的 mcause）一并做**：那时字段有生产者，
一次到位。

---

## 10. 兼容与迁移

- **宿主 ↔ 内核**：两端的 `schema_version` 必须**同时**升到 2。
  `decodeMessage` 的既有规则保持：**低于**支持版本 → 拒（`E_PROTO-01`）；
  **高于** → 放行（前向兼容）。
- **旧前端 ↔ 新内核**：旧前端会看到 `schema_version: 2`（高于它支持的 1）
  → 按前向兼容**放行**，但它**不认识**新命令（`ReadRegisters` 等）→
  未知变体按 `E_PROTO-08` 拒（这是正确行为，不是回归）。
- **新前端 ↔ 旧内核**：新前端 `SCHEMA_VERSION = 2`，旧内核回 `1` → **拒**
  （`E_PROTO-01`）。**这是有意的**：虚拟时间的字符串表示必须两侧一致，
  否则波形时间轴会错位。
- **快照落盘**：§2 的数值策略会改变 `EngineSnapshotDto` 的 JSON 形状
  （时间字段变字符串）→ **旧的快照文件读不进来了**。契约层已有
  `SNAPSHOT_SCHEMA_VERSION` 机制，**必须 bump**（见 §11 的验收清单）。

---

## 11. 验收清单（合入前逐条核对）

| # | 验收项 | 怎么验 |
| --- | --- | --- |
| 1 | `SCHEMA_VERSION == 2` | 契约层单测 + JS 镜像常量 |
| 2 | 3 个新命令 / 3 个新事件在 **Rust 与 JS 两侧**逐字一致 | `front/tests/contract.test.mjs`（现成的对拍门禁） |
| 3 | 虚拟时间字段的**线上表示是十进制字符串** | 契约层 JSON 形状断言 + JS 镜像断言 |
| 4 | `psToNumber` 超过 2⁵³ **抛错**而不是静默截断；错误码是 **`E_PROTO-04`**（输入超限），**不是** `E_PROTO-02`（那个是「仅日志」） | JS 单测 |
| 4b | **`ReadRegisters { count: 0 }` 回复 `values: []`，不是「不回复」**——不回复与消息丢失不可区分，M-05 会超时拒绝 | 内核侧用例 + 前端 mock 用例 |
| 5 | `ErrorFamily::Protocol.as_str() == "PROTO"` | 契约层单测 |
| 5b | **`E_PROTO_01`..`E_PROTO_08` 与 `E_DBG_01` 的场景常量已固化**，渲染出的码与 LLD 逐字一致 | 契约层单测（`error::tests`） |
| 5c | **LLD 错误码表补 DBG 一行**（`E_DBG-01` = 寄存器下标越界） | 文档核对 |
| 6 | 传输帧 `crc` 正确计算；算错 → `E_PROTO-06`；**缺失** → 放行 | JS 单测 |
| 7 | `Waveform` → `WaveformHistory` 改名完成，topic 同步 | 全仓 grep 无残留 + 前后端对拍 |
| 8 | `validate()` 拒绝「队列到期时刻 < 当前虚拟时间」 | 契约层单测 |
| 9 | `is_empty()` 存在且能识别 `default()` 快照 | 契约层单测 |
| 10 | `continue_until_event` / `Peripheral::tick` 的端口文档已订正 | 文档核对 |
| 11 | `SNAPSHOT_SCHEMA_VERSION` 已 bump | 契约层单测 |
| 12 | 全部门禁绿 | `cargo fmt` / `cargo clippy` / `cargo test --workspace --no-fail-fast` / `tools/check-front.sh` / `tools/golden-run.sh --keep-going` |
