# STAGE-29 · 契约层 v8 批 · 管理员跨进程载体 + 场景码落位收口

> ⚠️ **冻结规格** —— 本文件在**开工前**冻结；施工期的偏差一律记在 **§8 施工期订正**，**不回改正文**。
> ⚠️ 本规格由**统筹方独占修改**；lane 只报「**实测事实 + 规格原文 + 行号**」，不直接改本文件。
> ⚠️ **规格号 = STAGE-29**（`STAGE-24` 仍被「新增器件类型（如 I2C）」预留）；**批次号 = 第 28 批**（v0.44）。

---

## §0 本批的定位：一次升版，两件同链的事

### §0.1 主题

本批做台账 **D-129**（日志 / 健康信号的**跨进程载体缺失**）⇒ ⇒ 契约层**加 DTO 与回复事件** ⇒ ⇒ **必须升契约版本** ⇒ ⇒ 于是把**同一条链上**的另外两件事**搭进这一次升版**：

1. **场景码落位收口**（v0.43 批留下的一条边界）—— 跨边界的 `E_CTX-05` 落进契约层 ⇒ ⇒ 两侧共用。
2. **场景码工具补盲区**（实测新发现）—— 前端自产码**没有定义处** ⇒ ⇒ 「唯一定义」与「前端 × 内核撞号」两条判据对它们**结构上无法生效** ⇒ ⇒ **恒绿 = 零判别力**。

⇒ ⇒ **本批的主题 = 契约层 v8 批**：把「**内核 ↔ 前端**」这条边界上三件一直含糊的事，一次性钉死。

### §0.2 硬约束

- **只升 `SCHEMA_VERSION`**（**7 → 8**）⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **仍 2**、`PLATFORM_SCHEMA_VERSION` **仍 1**、`PLATFORM_RULES_VERSION` **仍 3** ⇒ ⇒ **零**新命令（`CommandPayload::Admin` **已存在**）· **零**新错误族 · **零**新会话快照段。
- ⚠️ **前端镜像必须同批更新**（`front/lib/message.js` 的 `SCHEMA_VERSION` 7 → 8 + 两个 topic + 两个载荷构造器 + 两张码表）⇒ ⇒ **两个文件同一个 commit**。
- ⚠️ **默认行为一字不变**（裁决 G）⇒ ⇒ 宿主对 `Admin` 命令的**默认**回应**仍然**是 `E_CTX-05` ⇒ ⇒ 零回归风险。
- ⚠️ **不引入 `Role`**（那是 D-131）⇒ ⇒ 契约层**零**新增角色概念。

---

## §1 事实：**实测**与**原文**（不是推断）

### §1.1 台账原文（**要改的那几条**）

- **D-129**（⏳ **未闭环**）：「日志 / 健康信号的**跨进程载体缺失**」—— 契约层**已经**有 `CommandPayload::Admin { op: AdminOp }` 与 `AdminOp::DrainLogs` ⇒ ⇒ **空壳入口在，缺的是「日志条目的 DTO」与「回复事件」** ⇒ ⇒ 宿主对该命令只能一律返回 `E_CTX-05`「管理通道未启用」⇒ ⇒ **管理员通道接不上**。做法 **A/B/C/D** = 契约层新增 `LogEntryDto` + `AdminDiagDto` · 新增**回复事件** `AdminLogs`（`direction = Event`）⇒ ⇒ **一条命令至少一个事件**（前端 Promise 才有落点）· 前端 `front/lib/message.js` **镜像**同形状 + M-05 的表驱动路由**加一行** · 宿主 `dispatch` 把 `AdminOp::DrainLogs` 从「一律 `E_CTX-05`」改成「回 `AdminLogs` 事件」。**工作量估计** ≈ **2~3 人日**。⚠️ **陷阱** ③ 前端镜像**必须**与 Rust 侧**逐字同形**（本仓前端有一份 JS 镜像 ⇒ **两个真相**的风险点）⇒ 需要一条一致性用例。
- **D-132**（✅ 已闭环 · v0.42）：判据 =「**跨模块共享**（别的 crate 要引用它）**或** **前端要按码分派**（前端拿到这个码要决定做什么）⇒ ⇒ 满足**其一** ⇒ ⇒ 住契约层；不满足 ⇒ ⇒ 住模块内」⇒ ⇒ **B 归类** = **只登记、不搬** ⇒ ⇒ **零搬迁**。⚠️ **一条待订正**：宏的**既有**文档写的是「two modules need to name must live in the contract layer」=「**且**」⇒ ⇒ 与本批判据**不一致** ⇒ ⇒ **以模块级文档为准** ⇒ ⇒ 宏文档**下一批订正**。
- **v0.43 批留下的一条边界**：「前端自产码**散落各族** ⇒ ⇒ 要治它需要「**前端自有族**」⇒ ⇒ **加族就要升版** ⇒ ⇒ 留给下一批」⇒ ⇒ ⚠️ **本批实测推翻这条**（见 F3 / F4 / F5 / D）。

### §1.2 实测事实表

> ⚠️ **§1.2 记录的是冻结时刻状态**；7e 钉**落点**、**不钉结论** ⇒ ⇒ 本批会改掉其中几条的「结论」⇒ ⇒ 那部分记在 **§8**。

| 编号 | 事实 | 落点（文件 + 标识符） |
| --- | --- | --- |
| **F1** | 契约层 `error_families!` 声明 **15 个族**；其中 `Context => "CTX"` · `Transport => "TRX"` · `Storage => "STORE"` **三个族已声明、但契约层零场景码** | `crates/contract/src/error.rs`：`error_families!` / `Context => "CTX"` / `Transport => "TRX"` / `Storage => "STORE"` |
| **F2** | 契约层 `pub const E_*` 共 **40** 个（`E_PROTO_01_VERSION_MISMATCH` … `E_PLT_16_WIRING_INVALID`）⇒ ⇒ **没有** `E_CTX_*` / `E_TRX_*` / `E_STORE_*` 任一个 | `crates/contract/src/error.rs`：`pub const E_PROTO_01_VERSION_MISMATCH` / `pub const E_PLT_16_WIRING_INVALID` |
| **F3** | 前端**服务层**真正用到（注释不算）而 Rust 侧**没有定义**的码共 **12 个**：`E_CTX-01` `E_CTX-05` `E_CTX-06` `E_CTX-11` / `E_TRX-01` `E_TRX-02` `E_TRX-03` `E_TRX-04` `E_TRX-05` / `E_STORE-01` `E_STORE-02` `E_STORE-03` | `front/services/m04_session.js` · `front/services/m05_commandrouter.js` · `front/services/m07_transport.js` · `front/services/m10_registermemory.js` · `front/services/m23_boardstore.js` · `front/kernel-worker.js` · `TRANSPORT_CHANNEL_ERROR_CODE` · `HOST_ERROR_CODE` |
| **F4** | `E_CTX-05` 是**唯一跨边界**的码 ⇒ ⇒ **两侧同义**：内核 `crates/kernel/src/kernel_host/mod.rs` 的 `dispatch` 里回的就是它；前端 `front/services/m05_commandrouter.js` 里只是**兜底默认码** ⇒ ⇒ 两侧**含义相同** | `crates/kernel/src/kernel_host/mod.rs`：`fn dispatch(&mut self, cmd: CommandPayload) -> Vec<EventPayload>`；`front/services/m05_commandrouter.js`：`failPending` |
| **F5** | `E_CTX-06` 在内核 `crates/kernel/src/m22_hms/mod.rs` 里**只出现在测试**（`heartbeat_signal("E_CTX-06")`）⇒ ⇒ **内核生产代码不用它** ⇒ ⇒ 它是**纯前端码**（M-05 命令超时 / 传输断开） | `crates/kernel/src/m22_hms/mod.rs`：`heartbeat_signal("E_CTX-06")`；`front/services/m05_commandrouter.js`：`reject(protocolError("E_CTX-06", …))` |
| **F6** | `tools/error_code_check.py` 的**码集合只从 Rust 侧定义来**（`parse_definitions` 只扫 `crates/**`）⇒ ⇒ 前端自产码只落进「**被点名但无定义**」⇒ ⇒ 「前端 × 内核撞号」判据**对它们恒绿** ⇒ ⇒ **零判别力** | `tools/error_code_check.py`：`def parse_definitions` / `def judge_front_kernel_collisions` |
| **F7** | 前端那 12 个码**全是裸字面量字符串**（**无定义处**）⇒ ⇒ D-120 的「**唯一定义**」不变式对它们**结构上无法生效** | `front/services/m23_boardstore.js`：`protocolError("E_STORE-01", …)`；`front/services/m04_session.js`：`throw sessionError("E_CTX-11", "会话不存在")` |
| **F8** | 契约层**已有** `CommandPayload::Admin { op: AdminOp }` 与 `AdminOp::{DrainLogs, KillGuest, PersistProject, Custom}` ⇒ ⇒ **空壳入口在** | `crates/contract/src/protocol.rs`：`pub enum CommandPayload` / `pub enum AdminOp` / `DrainLogs` / `KillGuest` / `PersistProject` |
| **F9** | 宿主 `dispatch` 对 `CommandPayload::Admin { .. }` **一律**回 `EventPayload::Error`（码是 `E_CTX-05`）⇒ ⇒ **管理员通道接不上** | `crates/kernel/src/kernel_host/mod.rs`：`CommandPayload::Admin { op } => self.on_admin(op)`；`fn on_admin(&mut self, op: AdminOp) -> Vec<EventPayload>` |
| **F10** | 宿主**没有** `Role` 概念（契约层与 `kernel_host` 里 `Role` 零命中）；`KERNEL_CAP_BITS = 0b111` 是 **L1/L2/L3 调试能力分级**，与 admin 无关 ⇒ ⇒ 「会话的**能力面**」这句措辞**没有实现支撑** | `crates/kernel/src/kernel_host/mod.rs`：`pub const KERNEL_CAP_BITS: u64 = 0b111;` |
| **F11** | M-21 的 `LogEntry` 有 **8** 个字段（`time_virt_ps` · `time_host_ms` · `session_id` · `level` · `ctx` · `err_code` · `msg` · `truncated`），查询 API 是 `query(&LogQuery) -> Vec<LogEntry>` 与 `query_recent(&LogQuery, n) -> Vec<LogEntry>`；契约层的十进制字符串策略 `numstr::u64_decimal` **现成** | `crates/kernel/src/m21_logging/mod.rs`：`pub struct LogEntry` / `pub fn query(` / `pub fn query_recent(`；`crates/contract/src/numstr.rs`：`pub mod u64_decimal` |
| **F12** | 前端镜像 `front/lib/message.js` 已有 `SCHEMA_VERSION` / `EVENT_TOPIC`（**15** 个 topic）/ `protocolError(code, message)` / `Command.admin(op)` / `Event` 构造器组 ⇒ ⇒ **挂载点现成** | `front/lib/message.js`：`export const SCHEMA_VERSION` / `export const EVENT_TOPIC` / `export function protocolError` / `admin: (op) => ({ Admin: { op } })` |
| **F13** | 前端 M-05 的回包匹配是**按 `reqId`**（`pending` Map + 传输帧的关联号）⇒ ⇒ **不是**台账说的「**表驱动路由**」⇒ ⇒ **新增一个事件变体不需要改路由** | `front/services/m05_commandrouter.js`：`if (event.reqId !== null && pending.has(event.reqId)) {` |
| **F14** | `DiagnosticKit` 有 **4** 个字段（`err_code` · `snapshot_summary` · `input_seq_tail` · `host_clock_available`）⇒ ⇒ **没有 u64 时间字段** ⇒ ⇒ 「虚拟时间走十进制字符串」**只**影响 `LogEntryDto` | `crates/kernel/src/m22_hms/mod.rs`：`pub struct DiagnosticKit` |
| **F15** | `error_families!` 宏的**既有**文档写的是「two modules need to name must live in the contract layer」=「**且**」⇒ ⇒ 与 D-132 判据（「**或**」）**不一致** ⇒ ⇒ **v0.42 批明确记「下一批订正」** | `crates/contract/src/error.rs`：`error_families!`（宏的文档注释） |
| **F16** | 版本常量：契约层 `SCHEMA_VERSION`（**7**，本批要升）/ `SIM_STATE_SCHEMA_VERSION`（**2**）/ `PLATFORM_SCHEMA_VERSION`（**1**）；前端镜像也有 `SCHEMA_VERSION` | `crates/contract/src/lib.rs`：`pub const SCHEMA_VERSION: u32` / `crates/contract/src/model.rs`：`pub const SIM_STATE_SCHEMA_VERSION: u32 = 2;` / `crates/contract/src/platform.rs`：`pub const PLATFORM_SCHEMA_VERSION: u32 = 1;` / `front/lib/message.js`：`export const SCHEMA_VERSION` |

---

## §2 契约层设计

### §2.1 两个 DTO（**只镜像形状，不搬内核类型**）

```rust
// crates/contract/src/admin.rs（新增文件）
/// 一条日志条目（跨进程形状）—— 字段与 M-21 的 `LogEntry` **一一对应**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntryDto {
    /// 虚拟时间（ps）—— **十进制字符串**（契约 v2 §2.2 的 `u64_decimal` 策略）。
    #[serde(with = "crate::numstr::u64_decimal")]
    pub time_virt_ps: u64,
    /// 宿主单调时间（ms）—— **十进制字符串**（裁决 J：与虚拟时间**同族**处理）。
    #[serde(with = "crate::numstr::u64_decimal")]
    pub time_host_ms: u64,
    /// 会话关联标签（MVP 恒为 0）。
    pub session_id: u32,
    /// 级别（`info` / `warn` / `error` / `fatal`）。
    pub level: String,
    /// 「模块-流程点」标识。
    pub ctx: String,
    /// 完整场景码，**可空**。
    pub err_code: Option<String>,
    /// 消息文本。
    pub msg: String,
    /// 任一字段被截断时为 true。
    pub truncated: bool,
}

/// 管理员诊断包（跨进程形状）—— 字段与 M-22 的 `DiagnosticKit` **一一对应**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminDiagDto {
    pub err_code: Option<String>,
    pub snapshot_summary: String,
    pub input_seq_tail: Vec<String>,
    /// **宿主单调时钟可用吗**（v0.43 · D-135 · 显式布尔，**不是**缺键）。
    pub host_clock_available: bool,
}
```

⚠️ **裁决 I**：这不是「把内核类型搬进契约层」，而是**在契约层定一份形状** ⇒ ⇒ 内核负责转换（M-21 的 `LogEntry` → `LogEntryDto`、M-22 的 `DiagnosticKit` → `AdminDiagDto`）⇒ ⇒ **契约层不依赖内核**。

### §2.2 两个回复事件

```rust
// crates/contract/src/protocol.rs 的 EventPayload 里新增
/// 管理员通道：导出的日志条目（回复 `CommandPayload::Admin { op: AdminOp::DrainLogs }`）。
AdminLogs { entries: Vec<crate::admin::LogEntryDto> },
/// 管理员通道：管理员诊断包（回复 `CommandPayload::Admin { op: AdminOp::Custom("diag") }`）。
AdminDiag { diag: crate::admin::AdminDiagDto },
```

⚠️ **一条命令至少一个事件** ⇒ ⇒ 前端 Promise 才有落点（台账 D-129 的 B 做法）。
⚠️ **`AdminDiag` 挂在 `AdminOp::Custom("diag")` 上**（裁决 H）⇒ ⇒ `AdminOp` **不加新变体** ⇒ ⇒ 契约层**零**新命令。

### §2.3 一个码常量（**唯一的跨边界码**）

```rust
// crates/contract/src/error.rs
/// 管理通道未启用（admin op 不在当前会话的能力面内）—— **内核与前端共用**。
pub const E_CTX_05_ADMIN_CHANNEL_DISABLED: u16 = 5;
```

⇒ ⇒ **CTX 族**已声明（F1）⇒ ⇒ **加码不加族** ⇒ ⇒ `error_families!` **一个字不改**。
⇒ ⇒ 内核 `kernel_host/mod.rs` 的 `"E_CTX-05"` 字面量改成**引用这个常量** ⇒ ⇒ 前端 `m05_commandrouter.js` 的兜底默认值也引用**镜像常量** ⇒ ⇒ 「**一个约束一个落点**」。

### §2.4 版本与宏文档

- `SCHEMA_VERSION` **7 → 8**；`SIM_STATE_SCHEMA_VERSION` **仍 2**；`PLATFORM_SCHEMA_VERSION` **仍 1**；`PLATFORM_RULES_VERSION` **仍 3**。
- ⚠️ **宏文档订正**（F15 · D-132 留的「下一批订正」）：`error_families!` 的文档从「two modules need to name」=「**且**」改成「**跨模块共享 _或_ 前端要按码分派**」⇒ ⇒ **行为零变更**（只改散文）。

---

## §3 内核设计

### §3.1 宿主侧的新增

```rust
// crates/kernel/src/kernel_host/mod.rs
/// 管理通道**是否启用**（裁决 G）。
///
/// ⚠️ **默认关** ⇒ ⇒ 宿主对 `Admin` 命令的**默认**回应**仍然**是 `E_CTX-05`
/// ⇒ ⇒ **默认行为一字不变** ⇒ ⇒ 零回归风险。
pub admin_enabled: bool,
```

- `KernelHost` 的构造路径加一个 `with_admin_channel(bool)`（或等价）⇒ ⇒ **默认 `false`**。
- `dispatch` 的 `Admin` 分支改成：
  - `admin_enabled == false` ⇒ ⇒ **原样**回 `EventPayload::Error { code: E_CTX_05_ADMIN_CHANNEL_DISABLED, … }` ⇒ ⇒ **一字不变**。
  - `admin_enabled == true` 且 `op == DrainLogs` ⇒ ⇒ 回 `EventPayload::AdminLogs { entries }`，条目取自 **M-21 的 `query`**（虚拟时间序）。
  - `admin_enabled == true` 且 `op == Custom("diag")` ⇒ ⇒ 回 `EventPayload::AdminDiag { diag }`。
  - 其余 op（`KillGuest` / `PersistProject`）⇒ ⇒ **仍回 `E_CTX-05`** ⇒ ⇒ **不假装实现了**（裁决 H）⇒ ⇒ 登记为已知边界。

### §3.2 M-21 / M-22 的转换

- M-21 ⇒ ⇒ 加一个 `LogEntry → LogEntryDto` 的转换（**住在 M-21**，因为**日志条目是它拥有的形状**）⇒ ⇒ 契约层不依赖内核。
- M-22 ⇒ ⇒ `DiagnosticKit → AdminDiagDto` 的转换（**住在 M-22**）。
- ⚠️ **日志条数的上界**：管理员通道一次导出**必须**有上界 ⇒ ⇒ 否则一次 `DrainLogs` 就能把整个日志环搬过进程边界 ⇒ ⇒ 上界**写死且可解释**（与 `RETAINED_BAND_CAPACITY = 256` 同族的口径）⇒ ⇒ 超出的部分**明确告知被截断**（**不是静默**）。

### §3.3 内核侧的字面量收口

- `kernel_host/mod.rs` 的 `"E_CTX-05"` ⇒ ⇒ 改成 `riscv_sim_contract::error::E_CTX_05_ADMIN_CHANNEL_DISABLED` 的**字符串形态**（该常量是 `u16`，故用 `format!` 或在契约层加一个 `&str` 形态的常量 ⇒ ⇒ **裁决**：契约层**只**给 `u16` 常量（与其它码**同形**），内核用 `format!("E_CTX-05")` 之类**同形**写法 ⇒ ⇒ ⚠️ **不要**在契约层搞第二份字符串常量 ⇒ ⇒ **一个约束一个落点**）。

---

## §4 前端与工具设计

### §4.1 前端镜像（`front/lib/message.js`）

- `SCHEMA_VERSION` **7 → 8**。
- `EVENT_TOPIC` 加两个 topic：`AdminLogs: "admin.logs"` · `AdminDiag: "admin.diag"`。
- `Event` 构造器加两个载荷：`adminLogs(entries)` · `adminDiag(diag)`。
- ⚠️ **M-05 路由表不需要改**（F13 ⇒ 回包按 `reqId` 匹配）⇒ ⇒ **台账 D-129 的「C 前端 M-05 的表驱动路由加一行」已被实测推翻 ⇒ ⇒ 订正**。
- **两张码表**（裁决 E）：
  - `FRONT_ERROR_CODES` —— **前端自有**的码（11 个：`E_CTX-01` `E_CTX-06` `E_CTX-11` / `E_TRX-01`..`E_TRX-05` / `E_STORE-01`..`E_STORE-03`）⇒ ⇒ 它们的**定义处在前端** ⇒ ⇒ 裸字面量**判红**。
  - `MIRRORED_ERROR_CODES` —— **契约层镜像**（本批只 `E_CTX-05`）⇒ ⇒ **不得**在 `FRONT_ERROR_CODES` 里再定义一遍（⇒ ⇒ 两处定义 ⇒ ⇒ 「唯一定义」判红）。

### §4.2 工具（`tools/error_code_check.py`）

- **补盲区 ①**：「定义处」的口径**扩展到前端常量表** ⇒ ⇒ `FRONT_ERROR_CODES` 的每个条目是那些码的**定义处** ⇒ ⇒ `MIRRORED_ERROR_CODES` 的条目**不是**定义处（那是契约层）⇒ ⇒ 「唯一定义」对前端码**真正生效**。
- **补盲区 ②**：码集合**并上前端自产码**（现在只在「被点名但无定义」里）⇒ ⇒ 「前端 × 内核撞号」判据**对前端码也生效**。
- **新断言 ⑤**：「前端**服务层**用到的码**必须**有定义处」⇒ ⇒ **平台改前红**（12 个裸字面量）⇒ ⇒ 落地后**绿** ⇒ ⇒ **「平台改前红、改后绿」这条判别力链成立**。
- ⚠️ **判红必须逐条判、不能数全局条数**（沿用 v0.43 批的纪律）⇒ ⇒ 红集里**每个码一行**。
- ⚠️ **自带坏夹具** ⇒ ⇒ 坏夹具**必须让它变红** ⇒ ⇒ **红集逐字比对**。
- ⚠️ **退出码语义不变**（0 = 没报出问题 / 1 = 报了问题 / 2 = 工具自身出错）。

### §4.3 一致性用例（D-129 陷阱 ③）

契约层导出 `LogEntryDto` / `AdminDiagDto` 的 JSON ⇒ ⇒ 前端镜像能解析**同一份载荷** ⇒ ⇒ **一条跨层用例**（Rust 侧导出 → 前端侧解析，或 Rust 侧断言 JSON 形状 + 前端侧断言解析结果**逐字相同**）。

---

## §5 验收

### §5.1 契约层（A 组）

| 编号 | 验收 |
| --- | --- |
| **A-1** | `SCHEMA_VERSION` 是 **8**；`SIM_STATE_SCHEMA_VERSION` **仍 2**；`PLATFORM_SCHEMA_VERSION` **仍 1**；`PLATFORM_RULES_VERSION` **仍 3** |
| **A-2** | 契约层有 `LogEntryDto` 与 `AdminDiagDto`，字段与 §2.1 **逐字一致** |
| **A-3** | `EventPayload` 有 `AdminLogs` 与 `AdminDiag` 两个变体；`AdminOp` **没有**新变体 |
| **A-4** | 契约层有 `E_CTX_05_ADMIN_CHANNEL_DISABLED = 5`；`error_families!` **一个字未改**（族数仍 15） |
| **A-5** | `LogEntryDto` 的两个时间字段的 **JSON 形态是字符串**（不是数字） |
| **A-6** | `error_families!` 的宏文档**不再**出现「two modules need to name」；**出现**「或」那条判据 |
| **A-7** | 契约层**零** `Role` 概念；**零**新命令变体 |
| **A-8** | `AdminDiagDto` 的字段与 M-22 `DiagnosticKit` **一一对应**（不多不少） |

### §5.2 内核侧（B 组）

| 编号 | 验收 |
| --- | --- |
| **B-1** | `admin_enabled == false`（默认）⇒ ⇒ `Admin` 命令**仍**回 `E_CTX-05` ⇒ ⇒ **默认行为一字不变** |
| **B-2** | `admin_enabled == true` + `DrainLogs` ⇒ ⇒ 回**恰好一个** `AdminLogs` 事件（**不是** `E_CTX-05`） |
| **B-3** | `AdminLogs` 的条目**条数与顺序**与 M-21 环里的一致（虚拟时间序） |
| **B-4** | `admin_enabled == true` + `Custom("diag")` ⇒ ⇒ 回**恰好一个** `AdminDiag` 事件 |
| **B-5** | `KillGuest` / `PersistProject` ⇒ ⇒ **仍**回 `E_CTX-05`（**不假装实现了**） |
| **B-6** | 一次 `DrainLogs` 的条目数**有上界**；超出的部分**明确告知被截断**（**不是静默**） |

### §5.3 前端与工具（C 组）

| 编号 | 验收 |
| --- | --- |
| **C-1** | 前端 `SCHEMA_VERSION` 是 **8**；`EVENT_TOPIC` 有 `admin.logs` 与 `admin.diag`；`Event` 构造器有两个新载荷 |
| **C-2** | 前端**没有**裸字面量场景码（前端服务层里每个码都来自常量表） |
| **C-3** | `FRONT_ERROR_CODES` 与 `MIRRORED_ERROR_CODES` **不重叠**（`E_CTX-05` 只在镜像表里） |
| **C-4** | 工具的「唯一定义」判据**对前端码生效**（前端码有定义处 ⇒ 不红） |
| **C-5** | 工具的新断言「前端服务层用到的码必须有定义处」⇒ ⇒ **改前红、改后绿** |
| **C-6** | 工具的「前端 × 内核撞号」判据**对前端码也生效**（造一棵树 ⇒ ⇒ 抓得住） |
| **C-7** | 契约层与前端镜像的 DTO 形状**逐字相同**（一条跨层一致性用例） |
| **C-8** | 前端能订阅 `admin.logs` / `admin.diag` 并拿到事件（最小接线） |

### §5.4 工具判别力（T 组）

| 编号 | 验收 |
| --- | --- |
| **T-1** | 工具的**坏夹具**必须让它变红，且红集**逐字**相符（沿用 v0.43 的做法） |
| **T-2** | 工具的**好夹具**必须让它 **0 红** |
| **T-3** | 「平台改前红 / 改后绿」在**真仓**上实测过（改前 = 前端裸字面量；改后 = 前端常量表） |
| **T-4** | 工具的**退出码语义**未变（0 / 1 / 2） |

### §5.5 边界（X 组）

| 编号 | 验收 |
| --- | --- |
| **X-1** | 契约层**零**新增 `Role` 概念；**零**新命令变体 |
| **X-2** | `error_families!` 的族数**仍 15**（**加码不加族**） |
| **X-3** | 宿主对 `Admin` 命令的**默认**回应**一字未变** |
| **X-4** | `KillGuest` / `PersistProject` **仍未实现**（明确登记，不假装） |

---

## §6 门禁

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --no-fail-fast --lib --tests`
4. `tools/check-front.sh`
5. `tools/golden-run.sh --self-check` + `--keep-going` + `--keep-going --check-mem`
6. `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
7. `node tools/browser-verify.mjs`
8. `tools/boot-osz.sh`
9. `python3 tools/stage29_check.py docs/detailed-design/STAGE-29_V044_Contract_V8_Admin_Carrier_Code_Landing.md`
10. `python3 tools/stage28_check.py docs/detailed-design/STAGE-28_V043_Store_Code_Clock_Visibility.md`
11. `python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md` + `--self-test`
12. `python3 tools/error_code_check.py` + `--self-test`
13. `python3 tools/front_test_attrib.py` + `--selftest`
14. `python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')`
15. `python3 tools/count_scale.py`
16. **变异自检**（每条 lane 至少一条「对照变异 0 红」）

⚠️ **「十六项」是**本批**的清单**（与 v0.42 / v0.43 批同口径）⇒ ⇒ **新增**的是第 9 项（`stage29_check.py`）⇒ ⇒ 其余十五项与上批**逐字相同**。

---

## §7 裁决

### 裁决 A · 本批升契约版本 **v7 → v8**

D-129 加 DTO 与事件 ⇒ ⇒ **必须**升契约版本 ⇒ ⇒ `SCHEMA_VERSION` **7 → 8**。

⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **仍 2** · `PLATFORM_SCHEMA_VERSION` **仍 1** · `PLATFORM_RULES_VERSION` **仍 3**。

### 裁决 B · **不新增错误族**

`error_families!` **一个字不改** ⇒ ⇒ 「加码不加族」守住。

### 裁决 C · **只搬 `E_CTX-05`**

`E_CTX-05` 是**唯一跨边界**的码（F4）⇒ ⇒ 契约层定义 `E_CTX_05_ADMIN_CHANNEL_DISABLED` ⇒ ⇒ 内核与前端**共用** ⇒ ⇒ 「一个约束一个落点」。

### 裁决 D · 其余 11 个前端码**留在前端**

「场景码住在拥有该场景的模块里」⇒ ⇒ 现状**正确** ⇒ ⇒ **零搬迁**。

⚠️ **v0.43 批台账与 README 里「前端自产码散落各族 ⇒ 要加族」那条结论被本批实测推翻 ⇒ ⇒ 订正**。

### 裁决 E · 前端自产码要有**定义处**

前端加**两张码表**（`FRONT_ERROR_CODES` = 前端自有 · `MIRRORED_ERROR_CODES` = 契约层镜像）⇒ ⇒ 裸字面量**判红** ⇒ ⇒ 「唯一定义」对前端码**真正生效**。

### 裁决 F · 工具补盲区（两条）

码集合并上前端自产码 + 「定义处」口径扩到前端常量表 + 新断言「前端服务层用到的码**必须**有定义处」⇒ ⇒ **平台改前红、改后绿**。

### 裁决 G · D-129 的闸门 = 宿主侧显式开关

`admin_enabled`，**默认关** ⇒ ⇒ **默认行为一字不变** ⇒ ⇒ 「谁有资格开」= **D-131**。

### 裁决 H · 本批只实现 `DrainLogs` 与 `Custom("diag")` 两个 op

`AdminOp` **不加新变体** ⇒ ⇒ `KillGuest` / `PersistProject` **仍未实现** ⇒ ⇒ **不假装实现了**。

### 裁决 I · `AdminDiagDto` = `DiagnosticKit` 的 4 个字段逐字镜像

**不加不减** ⇒ ⇒ **不是**把内核类型搬进契约层，而是**在契约层定一份形状** ⇒ ⇒ 转换住在 M-22。

### 裁决 J · `LogEntryDto` 的两个时间字段都走 `u64_decimal`

十进制字符串 ⇒ ⇒ 「同族处理」比「只把虚拟时间转字符串」更一致 ⇒ ⇒ 契约层现成策略。

### 裁决 K · M-05 路由表**不需要改**

F13 ⇒ ⇒ 回包按 `reqId` 匹配 ⇒ ⇒ 前端只需 topic 表 + 载荷构造器。

⚠️ **台账 D-129 的「C 加一行路由」已被实测推翻 ⇒ ⇒ 订正**。

### 裁决 L · **不引入 `Role`**

那是 D-131 ⇒ ⇒ 契约层**零**新增角色概念 ⇒ ⇒ `AdminOp` 文档里「Always gated by an admin role check」那句**本批不兑现** ⇒ ⇒ ⚠️ **改措辞**（零行为）。

### 裁决 M · 宏文档订正

F15 · D-132 留的「下一批订正」⇒ ⇒ 「two modules need to name」（=「且」）改成「**或**」⇒ ⇒ **行为零变更**（只改散文）。

### 裁决 N · 契约层新增面清单

= 2 个 DTO（`LogEntryDto` / `AdminDiagDto`）+ 2 个事件变体（`AdminLogs` / `AdminDiag`）+ 1 个码常量（`E_CTX_05_ADMIN_CHANNEL_DISABLED`）+ `SCHEMA_VERSION` 7 → 8 ⇒ ⇒ **零**新命令 · **零**新族 · **零**新快照段。

⚠️ **前端镜像必须同批更新**（`SCHEMA_VERSION` + 两个 topic + 两个载荷构造器 + 两张码表）⇒ ⇒ **两个文件同一个 commit**。

---

## §8 施工期订正

> ⚠️ **本节记录「规格冻结之后**实测**到的事实与规格原文不符」的地方** ⇒ ⇒ 每一条都给出：规格原文 ⇒ ⇒ 实测事实 ⇒ ⇒ **已改成**什么 ⇒ ⇒ 落在哪个 commit。
> ⚠️ **判别力的口径**：一条订正**不算**判别力证据 ⇒ ⇒ 「规格与现实对不上」这件事**本身**只有被**工具或用例钉住**才算数 ⇒ ⇒ 每条订正后面都写明「谁钉住了它」。

### 订正 1 · §2.1 的 `level` 字段：示例级别清单与内核现实不符

- **规格原文**（§2.1 的 `LogEntryDto`）：「级别（`info` / `warn` / `error` / `fatal`）」。
- **实测事实**：内核 M-21 的 `LogLevel`（`crates/kernel/src/m21_logging/mod.rs`）是 **五个**变体 `Trace` / `Debug` / `Info` / `Warn` / `Error`，`as_str()` 给的是**大写**拼写 `"TRACE"` / `"DEBUG"` / `"INFO"` / `"WARN"` / `"ERROR"` ⇒ ⇒ **没有 `fatal` 这一级** ⇒ ⇒ 而且 `LogEntry.err_code` 存的是**普通 `String`**（**空串**表示没有码）⇒ ⇒ 与 DTO 的 `Option<String>` 不是同一件事。
- **已改成**：契约层 `crates/contract/src/admin.rs` 的字段文档**已改成**写明内核自己的五级拼写、**删掉** `fatal`、并写明「空串 ⇒ `None`」这条语义由内核的转换承担 ⇒ ⇒ 跨层唯一真相 `crates/contract/fixtures/admin-dto.json` 的样例等级**已改成** `INFO`（原来写的是与内核不符的 `info`）⇒ ⇒ Rust 侧的样例条目同步。
- **谁钉住了它**：契约层单测 `admin::tests::the_logs_event_matches_the_canonical_fixture`（序列化结果与 fixture 逐字相符 ⇒ ⇒ 拼写漂了它就红）。

### 订正 2 · §4.2 的「平台改前红（12 个裸字面量）」应是 **11** 个

- **规格原文**（§4.2 新断言 ⑤）：「⇒ ⇒ **平台改前红**（12 个裸字面量）」。
- **实测事实**：那 12 个是 §1.2 F3 的「前端服务层用到而 **Rust 侧没有定义**」的码 ⇒ ⇒ 其中 `E_CTX-05` 在**契约层落地之后**（本批 Lane A）**已经有了 Rust 侧的定义处**（`crates/contract/src/error.rs` 的 `E_CTX_05_ADMIN_CHANNEL_DISABLED`）⇒ ⇒ 于是「服务层用到但**没有**定义处」的码变成 **11 个**（就是前端自有的那 11 个：`E_CTX-01` / `E_CTX-06` / `E_CTX-11` / `E_TRX-01..05` / `E_STORE-01..03`）⇒ ⇒ 工具的「改前红」条数**已改成** 11。
- ⚠️ **F3 那个 12 仍然是事实**（它是**冻结时刻**的状态）⇒ ⇒ §1.2 开头那句「§1.2 记录的是冻结时刻状态」**继续有效** ⇒ ⇒ 两处**不矛盾**：F3 说的是「冻结时刻 Rust 侧没有定义」，本条说的是「**现在**没有定义」。
- **谁钉住了它**：工具 `tools/error_code_check.py` 的新断言 ⑤ 的「改前红 11 条 / 改后红 0 条」实测 + 坏夹具的**红集逐字比对**。

### 订正 3 · §3.3 的「同形写法」表述含歧义：族名与号**都**该从契约层推导

- **规格原文**（§3.3）：「内核用 `format!("E_CTX-05")` 之类**同形**写法 ⇒ ⇒ ⚠️ **不要**在契约层搞第二份字符串常量」。
- **实测事实**：「同形写法」这句话**只**约束了**场景号**，对**族名**没有约束 ⇒ ⇒ 若内核照字面写 `format!("E_CTX-05")`，那个 `"E_CTX"` 字面量就是**族名的第二处落点** ⇒ ⇒ 与「一个约束一个落点」相悖。
- **已改成**：内核侧那个码字符串**已改成**由契约层的 `error::CONTEXT_FAMILY.as_str()`（值 `"CTX"`）与 `error::E_CTX_05_ADMIN_CHANNEL_DISABLED`（值 `5`）**拼出** ⇒ ⇒ **族名与场景号各只有一个落点** ⇒ ⇒ 契约层**仍然只给 `u16`**（**没有**第二份字符串常量）⇒ ⇒ 渲染结果与旧的 `"E_CTX-05"` **逐字相同**。
- **谁钉住了它**：内核侧用例（B-1 断言默认路径回**逐字相同**的那个码串）+ 契约层单测 `error::tests::the_admin_channel_code_is_the_only_context_scenario_code`（族数仍 15）。

### 订正 4 · 规格没提「升版要连带处理**四处写死版本号的历史断言**」

- **规格原文**：§5.1 的 A-1 只写了四个版本号**该是什么**，**没写**「仓里有四处断言写死了旧版本号、升版时它们会红」。
- **实测事实**：`SCHEMA_VERSION` 在契约层被**四处**断言（`protocol.rs` 两处 · `platform.rs` 两处）⇒ ⇒ 升版时四条全红 ⇒ ⇒ 其中 `platform.rs` 的 `s19_a6_the_rules_version_is_three_and_the_other_two_versions_are_untouched` 更麻烦：它的**名字**声称「另两个版本未动」⇒ ⇒ 而 v0.44 **故意**动了契约信封版本（裁决 A）⇒ ⇒ 那个名字**变成一句假话** ⇒ ⇒ 只改数字不改名，等于留一个**说谎的名字**。
- **已改成**：按仓里**上一批升版（v6 → v7）的既有做法**（改名 + 改字面量）处理 ⇒ ⇒ `the_schema_version_is_seven` **已改成** `the_schema_version_is_eight`（字面量 8）· `the_envelope_carries_the_current_schema_version` 的字面量**已改成** 8 · `s16_a1_the_contract_schema_version_is_seven` **已改成** `s16_a1_the_contract_schema_version_is_eight`（字面量 8）· `s19_a6_...` **已改成** `s19_a6_the_rules_version_is_three_and_the_description_version_is_still_one` 并**收窄**为只钉「规则集版本 3」与「描述格式版本 1」⇒ ⇒ 契约信封版本改由 `protocol::tests::the_schema_version_is_eight` **单独**钉 ⇒ ⇒ 覆盖**没少**（只是**不钉两遍**）。
- **谁钉住了它**：那四条用例**自己**（它们断言的就是字面量）⇒ ⇒ 另有 `platform.rs` 那张三个版本常量的对照表，其表头与首行**已改成** `v0.44` 与 `8`。

### 订正 5 · §7 裁决 L 的措辞订正**已兑现**：但「旧句原文」**不许**留在文件里

- **规格原文**（裁决 L）：「`AdminOp` 文档里「Always gated by an admin role check on it」那句**本批不兑现** ⇒ ⇒ ⚠️ **改措辞**（零行为）」。
- **实测事实**：那句承诺了一个平台上**根本不存在**的检查（F10：宿主与契约层 `Role` 零命中）⇒ ⇒ 改成「由**接收方自己的开关**把守（宿主的 `admin_enabled`，默认关）」+「**谁**有资格开是另一条台账 D-131」⇒ ⇒ **行为零变更**（只改散文）。
- ⚠️ **附带的一条通用判据**（本批踩了**两次**）：**断言的字面量会命中自己** ⇒ ⇒ ① 若用**整篇文件**做子串检查 ⇒ ⇒ 断言的**第一个参数**自己就在文件里 ⇒ ⇒ 「文件里没有旧句」这条**永远不成立**；② 若**文档里引用了旧句原文**（哪怕是为了说明「这句被删了」）⇒ ⇒ 同一条检查**照样**命中 ⇒ ⇒ ⇒ ⇒ **判据**：文档改动类断言**必须**把检查范围**收窄到那段文档自己的块** ⇒ ⇒ **且**那段文档**不许**引用旧句原文（要么不提，要么**不提原文**）。
- **已改成**：契约层 `error_families!` 的宏文档与 `AdminOp` 的枚举文档**已改成**按「跨模块共享**或**前端要按码分派」与「由接收方自己的开关把守」写 ⇒ ⇒ 两处**都刻意不引用旧句原文** ⇒ ⇒ 各配一条**收窄到该文档块**的单测（`error::tests::the_family_macros_documentation_states_the_or_criterion` 与 `protocol::tests::the_admin_op_documentation_does_not_promise_a_role_check`）⇒ ⇒ 收窄时**还要**跳过夹在文档块与条目之间的**属性行**（`#[derive(...)]`）。
- **谁钉住了它**：那两条契约层单测（**改坏散文 ⇒ 它们就红**）。

### 订正 6 · §1.2 的 F3 与「前端自有码」的边界：镜像码**不等于**前端定义处

- **规格原文**（裁决 E）：「前端加**两张码表**（`FRONT_ERROR_CODES` = 前端自有 · `MIRRORED_ERROR_CODES` = 契约层镜像）⇒ ⇒ 裸字面量**判红**」。
- **实测事实**：前端服务层用到的码共 **21** 个（实测），其中**前端自有**的是 **11** 个，**其余 10** 个（`E_CTX-03` `E_DBG-01` `E_PLT-13` `E_PROTO-01/02/04/06/08` 与 `E_CTX-05`）的**定义处在 Rust 侧**（契约层或内核 crate）⇒ ⇒ `E_CTX-03` 尤其容易误判：它**不是**前端的，它是**内核 crate** 定义的（`crates/kernel/src/m11_symbol/trace.rs` 的 `E_CTX_03_ILLEGAL_EXPORT_FORMAT`）⇒ ⇒ 「CTX 族已声明但契约层零场景码」这句话在 v0.44 **之前**成立，**之后**只对 `E_CTX-01/06/11` 成立。
- ⚠️ **连带的一条工具口径**：工具的「唯一定义」判据要认出「前端常量表的条目」是**定义处**，**但**它们**绝不能**被混进「内核定义」那一组 ⇒ ⇒ 否则「前端 × 内核撞号」判据会把前端自产码当成**前端撞内核** ⇒ ⇒ **假红** ⇒ ⇒ `MIRRORED_ERROR_CODES` **不是**定义处（契约层才是）。
- **已改成**：契约层**只**定义 `E_CTX-05` 这**一个**跨边界码（裁决 C），其余 11 个前端码**留在前端**（裁决 D）⇒ ⇒ 工具的「定义处」口径**已改成**分成「Rust 侧定义」与「前端常量表定义」**两组分别记账**。
- **谁钉住了它**：工具 `tools/error_code_check.py` 的新断言 ⑤（「前端服务层用到的码**必须**有定义处」）+ 断言 ④（「前端 × 内核撞号」对前端码**也**生效）+ 坏夹具的**红集逐字比对**。

---

## §9 变异自检

> ⚠️ 每条变异的**锚点必须逐字匹配且出现恰好 1 次**⇒ ⇒ 否则**放弃**（不猜）。
> ⚠️ **还原纪律**：`cp` 备份（键 = 相对路径把 `/` 换成 `__`）⇒ ⇒ 还原用 `cp` ⇒ ⇒ `md5` + `cmp` 逐字节校验 ⇒ ⇒ **`touch <file>`（当前时间，不是 `touch -r`）** ⇒ ⇒ 重编 ⇒ ⇒ 复跑基准 ⇒ ⇒ **禁** `git checkout -- <file>`。
> ⚠️⚠️ **每条变异之前先还原到基线** ⇒ ⇒ 否则变异会**累积** ⇒ ⇒ 红集自然**递增** ⇒ ⇒ 「红集互不相同」那条**恒成立** ⇒ ⇒ ⇒ **零判别力** ⇒ ⇒ **必须逐条独立**。
> ⚠ **本表的「期望」是**逐条**实测**出来的**（不是推断）⇒ ⇒ 变异**打偏**时「期望」本身会假 ⇒ ⇒ ⇒ ⇒ ⇒ **零判别力** ⇒ ⇒ ⇒ ⇒ ⇒ **必须先探针**。
> ⚠ **「开工前」的那一版实测**与**「施工后在最终规格上」的重测**结果**可能不同** ⇒ ⇒ 本批 **T5** 就是这样：开工前红 1 条 ⇒ ⇒ 落地后重测红 **2** 条 ⇒ ⇒ ⇒ ⇒ **因为规格本身变了**（§8 填了十条订正 · §9.3 填了九条边界 ⇒ ⇒ F16 那一行的形状没变、但**跟着变的是断言集合**）⇒ ⇒ ⇒ ⇒ ⇒ **所以「期望」必须跟着规格一起复核** ⇒ ⇒ ⇒ ⇒ ⇒ **开工前测一次不够**。

### §9.1 真变异（期望**红**）

| 编号 | 落点 | 变异 | 期望（**完整红集**） |
| --- | --- | --- | --- |
| **T1** | 规格 **§5.1 的 A-4 行** | 验收编号 `A-4` 改成 `A-9` | **红 1 条**（A 组编号不连续） |
| **T2** | 规格 **§1.2 的 F4 行（落点列）** | `fn dispatch` 改错一个字符（`fn dispatch` → `fn dispatcH`） | **红 1 条**（跨层检查找不到那个标识符） |
| **T3** | 规格 **§5.1 的 A-1 行** | `SCHEMA_VERSION` 的值写成 **9** | **红 1 条**（禁列） |
| **T4** | 规格 §7 | 删掉「**裁决 H**」那条的标题（正文仍引用它） | **红 2 条**（裁决表缺一条 + 悬空引用） |
| **T5** | 规格 **§1.2 的 F 表** | **F16 那一行多一列** | **红 2 条**（① 表格列数 ② 7e 抽不出「落点」那一格 ⇒ ⇒ 跨层检查**静默失效** ⇒ ⇒ 必须红） |

> ⚠️ **期望红集是「施工后重测」出来的**：上表**开工前**的那一版实测里 **T5** 只红 **1** 条 ⇒ ⇒ 而本批**落地后**在**最终规格**上重测 ⇒ ⇒ **T5 红 2 条** ⇒ ⇒ ⇒ **多出来的那一条是 7e**：F16 那一行多一列 ⇒ ⇒ 「落点」那一格**被挤走** ⇒ ⇒ 7e 抽不出源文件路径 ⇒ ⇒ **跨层检查静默失效** ⇒ ⇒ 它**必须红** ⇒ ⇒ ⇒ ⇒ **一条变异触发两条断言**是常事 ⇒ ⇒ ⇒ **表里必须记完整红集**（不是条数）⇒ ⇒ ⇒ ⇒ **只记条数会漏掉那一条**。

### §9.2 对照变异（语义不变 ⇒ **必须 0 红**）

| 编号 | 落点 | 等价改写 | 期望 |
| --- | --- | --- | --- |
| **T-C1** | 规格 §1.2 的 **F1** 行 | 「**已声明**」→「**已宣告**」 | **0 红** |
| **T-C2** | 规格 §5.1 的 **A-2** | 「**逐字一致**」→「**逐字相同**」 | **0 红** |
| **T-C3** | 规格 §7 裁决 C | 「**共用**」→「**共享**」 | **0 红** |

### §9.3 判别力**边界**的自查

> ⚠️ **本节回答一个问题**：本批的门禁**抓不到**什么？⇒ ⇒ 「全绿」**不等于**「平台对」⇒ ⇒ 边界必须**写下来**，否则下一个人会误读「十六项全绿」。

1. ⚠️ **规格自检（`stage29_check.py`）只检**规格文本** ⇒ ⇒ 它**不**证明平台真的做到了 ⇒ ⇒ 平台的判别力**全部**来自**三处**：**契约层 Rust 单测** · **内核 Rust 单测** · **前端 node 测试 + 工具夹具** ⇒ ⇒ ⇒ **别拿 §9 的红绿当平台行为的证据**。
2. ⚠️ **工具的「前端服务层」扫描**覆盖 `front/services/` + `front/lib/` + `front/kernel-worker.js` ⇒ ⇒ **不覆盖**视图层（`front/views/**`）与 `front/dev/` ⇒ ⇒ **视图层里的裸字面量结构上抓不到** ⇒ ⇒ 本批**手工**改掉了 `front/views/components/memory-map.js` 里那个写死的版本号（它会被拼进给人看的说明文字 ⇒ ⇒ 不改就是**撒谎**）⇒ ⇒ 但**没有**工具钉住它 ⇒ ⇒ **已知覆盖缺口**。
3. ⚠️ **wasm（浏览器内）形态下管理通道默认关** ⇒ ⇒ **浏览器真机门禁碰不到它** ⇒ ⇒ **真机层对 admin 的覆盖是空的** ⇒ ⇒ （§10 第 7 条已登记）⇒ ⇒ 本批**没有任何真机层证据**。
4. ⚠️ **「谁有资格开管理通道」没做**（D-131）⇒ ⇒ 闸门只是**一个布尔开关** ⇒ ⇒ 「谁能开」这件事**没有**用例能覆盖 ⇒ ⇒ **结构上不可测**（平台里**没有**那个概念可供测）⇒ ⇒ 而且 `admin_enabled` **仓内没有任何调用点** ⇒ ⇒ 管理通道**实际不可达**。
5. ⚠️ **`AdminOp::Custom(String)` 的 `"diag"` 是字符串约定不是类型** ⇒ ⇒ 一个**拼错**的 op 会**静默**回 `E_CTX-05` ⇒ ⇒ ⇒ **没有**用例能区分「拼错了」与「这个 op 没实现」⇒ ⇒ 这是本批**有意**保留的缺口（§10 第 4 条）⇒ ⇒ 若将来加第二个 admin op，**必须**换掉这个约定。
6. ⚠️ **`E_CTX-05` 的「两侧同义」靠的是同一个字符串** ⇒ ⇒ 而**跨层 fixture 里没有那个码**（fixture 钉的是两个 DTO 的形状）⇒ ⇒ ⇒ 若某一侧偷偷改了那个码串，**跨层一致性用例抓不到** ⇒ ⇒ ⇒ 真正钉住它的是**别处**：内核侧那条「渲染结果与旧串逐字相同」的断言 + 前端常量表那条 + 规格自检的 7g-② ⇒ ⇒ ⇒ **三处**才够 ⇒ ⇒ **一处不算**。
7. ⚠️ **日志导出的上界取值（256）是写死的** ⇒ ⇒ 用例钉的是「**有条数上界**」这个**性质**，**不是**「恰好 256 条」这个**数值** ⇒ ⇒ 钉数值是**耦合**⇒ ⇒ 数值一改语义不变 ⇒ ⇒ ⇒ 那是**对照**不是**判别** ⇒ ⇒ ⇒ 本批**刻意不**钉数值。
8. ⚠️ **「截断明确告知」的实现方式是「宿主往 M-21 环里写一条 WARN 级说明 + 一个计数」** ⇒ ⇒ 用例钉的是「**截断发生时环里多了一条说明**」⇒ ⇒ 但「**人是否真的看得到**」**不在**机器可测的范围 ⇒ ⇒ ⇒ 「不静默」这条**只做到可观测**，**没做到可读性**。
9. ⚠️ **台账兜底（`FALLBACK_LEDGER_ROWS`）是「原文与源码都可能找得到」** ⇒ ⇒ 它**放宽**了 7e ⇒ ⇒ ⇒ 代价是：**一个落点可以靠台账活着** ⇒ ⇒ ⇒ 所以台账那一侧**必须**逐字记着那个形态（订正 8 已记）⇒ ⇒ ⇒ **放宽与收紧是一对**，不能只做一半。

### 订正 7 · §4.1 的「**两张**码表」不够 ⇒ ⇒ 实测是**三张**

- **规格原文**（§4.1 · 裁决 E）：「前端加**两张码表**（`FRONT_ERROR_CODES` = 前端自有 · `MIRRORED_ERROR_CODES` = 契约层镜像）⇒ ⇒ 裸字面量**判红**」。
- **实测事实**：前端服务层**用到**的码里，**只有 11 个是前端自产的** ⇒ ⇒ 其余那些是 **Rust 侧定义**的码（契约层或内核 crate）⇒ ⇒ 而 C-2 要求「前端服务层**不许**留裸字面量」⇒ ⇒ 那些 Rust 侧码在前端**也必须有名字可引** ⇒ ⇒ ⇒ ⇒ **两张表装不下** ⇒ ⇒ ⇒ ⇒ 拆成**三张**：`FRONT_ERROR_CODES`（11 个前端自有码 = **定义处**）· `MIRRORED_ERROR_CODES`（`E_CTX-05`）· `RUST_ERROR_CODE_REFS`（其余 Rust 侧码的**引用名**，**不是**定义处）。
- ⚠️ **裁决 E 的**意图**（前端服务层零裸字面量）**一字未动** ⇒ ⇒ 变的只是**表的张数** ⇒ ⇒ 而**多出来的那张**恰恰是「不是定义处」的那张 ⇒ ⇒ 「唯一定义」不变式**不受影响**。
- **已改成**：前端镜像**已改成**三张表 ⇒ ⇒ 工具的「定义处」口径**只**认 `FRONT_ERROR_CODES`（**唯一**那张）⇒ ⇒ `MIRRORED_ERROR_CODES` 与 `RUST_ERROR_CODE_REFS` **都**不是定义处 ⇒ ⇒ 契约层 / 内核 crate 的码**不许**进 `FRONT_ERROR_CODES`（进去就触发「重复定义」）。
- **谁钉住了它**：工具的新断言 ⑤（「前端服务层用到的码必须有定义处」）+ 断言 ④（「前端 × 内核撞号」）+ 前端用例 C-2 / C-3 + 规格自检的 7g-②（两张表的表体必须找得到）。

### 订正 8 · §1.2 的 F7 落点与裁决 E **互斥** ⇒ ⇒ 落点改由**台账留痕**兜底

- **规格原文**（§1.2 F7 的落点列）：`front/services/m04_session.js`：`throw sessionError("E_CTX-11", "会话不存在")`。
- **实测事实**：那正是 C-2 **要求删掉**的裸字面量 ⇒ ⇒ **两者不可兼得** ⇒ ⇒ 而 7e 的跨层检查是**对文件原文做子串匹配、不区分注释与代码** ⇒ ⇒ ⇒ 若不处置 ⇒ ⇒ 规格要么**逼着**前端留裸字面量（破 C-2），要么**逼着**前端留一份「**为了过检查而存在的注释**」（⇒ ⇒ 那个检查**永远绿着** ⇒ ⇒ **零判别力**）。
- **已改成**：走 STAGE-27 **已有**的机制（`FALLBACK_LEDGER_ROWS` = 台账兜底）⇒ ⇒ **F7 已加入**那张名单 ⇒ ⇒ F7 的落点**由台账留痕**兜底（D-129 的销账条目里**逐字**记着那个修前形态）⇒ ⇒ 而**源文件里不再留**那份「为了过检查而存在的注释」（只留一句「这里原先写的是裸字面量」的说明，**不带**旧句原文）⇒ ⇒ ⇒ 那个检查**真的**依赖台账 ⇒ ⇒ **有判别力**。
- **谁钉住了它**：规格自检的 7e（`F7` 不在台账留痕里 ⇒ ⇒ **红**）⇒ ⇒ 与「台账写错字」这条失败路径**逐条**分开。

### 订正 9 · 交底时给 lane 的**三个实测数字**订正

- **「前端服务层用到 21 个码」** ⇒ ⇒ **口径错了** ⇒ ⇒ 实测：**代码里用到 16 个**（**含注释**被提到的是 20 个）⇒ ⇒ 而「用到的 16 个」里**只有 11 个是前端自产的** ⇒ ⇒ 其余 5 个是 Rust 侧定义（`E_CTX-03` · `E_CTX-05` · `E_DBG-01` · `E_PLT-13` · `E_PROTO-01/02/04/06/08` 中的被用到者）⇒ ⇒ ⚠️ **`E_CTX-03` 尤其容易误判** ⇒ ⇒ 它的定义处在**内核 crate**（`crates/kernel/src/m11_symbol/trace.rs`）⇒ ⇒ **不是**契约层 ⇒ ⇒ 也**不是**前端的。
- **`RETAINED_BAND_CAPACITY` 在 M-21** ⇒ ⇒ **写错了** ⇒ ⇒ 实测它在 **M-22**（`crates/kernel/src/m22_hms/mod.rs`）⇒ ⇒ 内核侧导出上界取 **256**、与**它**同族。
- **内核侧那个码串「自己拼一次」** ⇒ ⇒ **不必** ⇒ ⇒ 宿主**本来就有一个**统一的渲染点（`error_from`，宿主所有错误码共用的那一个）⇒ ⇒ **已改成**复用它 ⇒ ⇒ 于是宿主里 `format!("E_{}-{:02}")` **只有一处** ⇒ ⇒ 族名与场景号**各**只有一个落点。

### 订正 10 · Lane C 的 C-2 判据**首版是空的** ⇒ ⇒ 被它自己的变异抓出来

- **实测事实**：Lane C 写的「服务层无裸字面量」那条判据，用正则去抓**引号里裹着码**的写法 ⇒ ⇒ 首版的引号字符类**漏了外层捕获组** ⇒ ⇒ `\1` **没有组可回引** ⇒ ⇒ 退化成「匹配一个字面的 1」⇒ ⇒ ⇒ **一条都匹配不到** ⇒ ⇒ ⇒ **恒绿** ⇒ ⇒ ⇒ **零判别力**。
- **怎么抓出来的**：**不是**靠人读 ⇒ ⇒ 是靠它自己的**变异自检**（MT4「把服务层退回裸字面量」）⇒ ⇒ **首轮 0 红** ⇒ ⇒ ⇒ 「变异打在被测路径真正走的分支上」这条纪律**救了它** ⇒ ⇒ 修好捕获组后**红 1 条**。
- **已改成**：那条判据**已改成**带捕获组的形式 ⇒ ⇒ 并在源码里**写明**这个坑。
- ⚠️ **一条可复用的判据**：**「自检脚本绿了」不是判别力证据** ⇒ ⇒ **「变异打上去它红不红」才是** ⇒ ⇒ 而「变异 0 红」有**两种**含义（① 这条路**没人测** ② 这条路**测了但判据是空的**）⇒ ⇒ ⇒ **两种都要查** ⇒ ⇒ ⇒ 前者查「测试树里有没有覆盖那条分支」，后者查「判据本身是不是恒真」。
- **谁钉住了它**：Lane C 的 MT4 变异（改坏 ⇒ ⇒ **红 1 条**）。

---

## §10 已知边界

1. ⚠️ **「谁有资格开管理通道」没做** ⇒ ⇒ 宿主侧只有一个**布尔开关** ⇒ ⇒ **角色 / 令牌鉴权 = D-131**（本批不碰）。
2. ⚠️ **`KillGuest` / `PersistProject` 仍未实现** ⇒ ⇒ 仍回 `E_CTX-05` ⇒ ⇒ **不假装实现了**（裁决 H）。
3. ⚠️ **管理员通道的日志导出有上界** ⇒ ⇒ 超出部分**明确告知被截断** ⇒ ⇒ ⚠️ **上界的取值**由本批写死 ⇒ ⇒ 将来若调小，**语义不变**（只是更早截断）。
4. ⚠️ **`AdminDiag` 走 `AdminOp::Custom("diag")`** ⇒ ⇒ `Custom(String)` 这个扩展点是**已存在**的 ⇒ ⇒ 契约层**零**新命令 ⇒ ⇒ ⚠️ 但它的**字符串**是**约定**而不是**类型** ⇒ ⇒ 将来若有第二个 admin op，仍要靠约定。
5. ⚠️ **前端自产码仍在「前端自有」这张表里** ⇒ ⇒ 契约层**不**定义它们 ⇒ ⇒ 跨进程时**前端自产的码仍然不可见** ⇒ ⇒ 但那些码**本来就是前端场景** ⇒ ⇒ 不构成缺口。
6. ⚠️ **本批是「边界收口」类** ⇒ ⇒ **平台不会因此「更强」** ⇒ ⇒ 判别力的提升**全部**来自「原本会红的地方现在被钉住」⇒ ⇒ ⚠️ **别拿「功能变多了」来汇报本批**。
7. ⚠️ **wasm 侧**：管理通道在 wasm（浏览器内）形态下**默认关** ⇒ ⇒ 浏览器真机门禁**不会**碰它 ⇒ ⇒ 真机层对 admin 的覆盖**是空的** ⇒ ⇒ 登记为已知边界。

---

## §11 复现命令

```bash
# ---- 仓内门禁 ----
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast --lib --tests
tools/check-front.sh
tools/golden-run.sh --self-check
tools/golden-run.sh --keep-going
tools/golden-run.sh --keep-going --check-mem
tools/build-wasm.sh && node tools/wasm-smoke.mjs
node tools/browser-verify.mjs
tools/boot-osz.sh
python3 tools/stage29_check.py docs/detailed-design/STAGE-29_V044_Contract_V8_Admin_Carrier_Code_Landing.md
python3 tools/stage28_check.py docs/detailed-design/STAGE-28_V043_Store_Code_Clock_Visibility.md
python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md
python3 tools/ledger_check.py --self-test
python3 tools/error_code_check.py
python3 tools/error_code_check.py --self-test
python3 tools/front_test_attrib.py
python3 tools/front_test_attrib.py --selftest
python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')
python3 tools/count_scale.py
```
