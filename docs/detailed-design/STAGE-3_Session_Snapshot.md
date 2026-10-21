# 会话级快照载荷规格（`STAGE-3`）

> **状态**：**冻结**（本文件是本轮施工的唯一依据；实现与本文件不一致即为缺陷）
> **来源**：台账 **D-36**（快照不含外设 / INTC 状态）+ **D-37**（契约 DTO 与 M-20/M-23
> 冻结的 `Snapshot` 结构不一致）+ **D-41**（`SimState` 缺内存页 / 外设状态）
> **基线**：`main` = `14bf6bc`（契约层 v2 落地后）
> **分支**：`feature/session-snapshot`
>
> ⚠️ **体例同 `STAGE-1_Wasm_Bridge.md` / `STAGE-2_Contract_V2.md`**。
> **本轮是契约层 v2 之后的第一个「会话级」扩展**——契约层已解冻（`schema_version = 2`），
> 可以新增 DTO，但**已冻结的 `EngineSnapshotDto` 不得改形状**。

---

## 1. 为什么现在做

台账 §2.2 的优先级表里，**5b 是唯一未完成的条目**：

> **5b** | **D-36 + D-37 + D-41：快照落盘对齐 LLD** | ~3d | 外设 / 中断控制器状态序列化
> （M-15 / M-16，D-36）+ M-20 按 LLD §2.1.1 组装完整 `Snapshot{cpMeta, m13State,
> m14Map, m15Periph}` + M-23 改吃该结构（D-37）+ M-13 / M-14 / M-15 补状态导出与载入接口（D-41）

**这三条是「持久化一个会话」的真正剩余工作量**。v0.11.2 加的 `EngineSnapshotDto`
只覆盖 **M-12/M-17 段**（队列 + 平台状态 + 插入序 + 虚拟时间 + 指令预算）——
**UART 的 RX/TX FIFO、定时器的 `mtime`/`mtimecmp`、GPIO 的方向 / 输出锁存、
已拉起的 IRQ 行，全都不在快照里**。今天落盘的东西**还原不了一个会话**。

### 1.1 本轮要闭合的三条台账

| 台账 | 卡在哪 | 本轮处置 |
| --- | --- | --- |
| **D-36** | 快照不含外设 / INTC 状态；M-15 / M-16 没有状态导出接口 | ✅ §3.3 / §3.4 新增 `export_state` / `load_state` |
| **D-37** | 契约 DTO 与 M-20/M-23 冻结的 `Snapshot` 结构不一致；M-20 / M-23 未接线 | ✅ §3.1 会话级载荷 DTO + §3.5 M-20 组装 + §3.6 M-23 改吃新结构 |
| **D-41** | M-13 / M-14 / M-15 缺状态导出与载入接口 | ✅ §3.2 / §3.3 |

### 1.2 ⚖️ 裁决：LLD 内部两处矛盾（本轮处置）

**M-20 / M-23 两份 LLD 的 §2.1.1 本身不自洽**——它们定义的 `Snapshot` 结构与它们
自己的正文打架。本轮**以「详细设计 §2.1.1」为准**（2026-10-20 裁决的延续），
但**该节内部**有两处必须处置：

#### 裁决 A —— `Snapshot` 结构漏列了 **M-12/M-17 段**

两份 LLD 的**正文**都写：

> 契约层 `riscv_sim_contract::snapshot::EngineSnapshotDto` 只覆盖本结构的
> **M-12/M-17 段**（调度器待处理事件队列 + 平台状态 + 插入序计数器 + 虚拟时间 +
> 每量子指令预算），**不含** cpMeta / m14Map / m15Periph。它由 M-17 的
> `snapshot_full` / `restore_full` 提供……**落盘格式契约归属：本结构优先于契约层 DTO。**

**但结构清单里没有它的位置**：

```c
struct Snapshot {
  Checkpoint cpMeta;            // 检查点元数据
  M13State  m13State;           // M-13 指令级状态序列化（寄存器/内存视图）
  M14Map    m14Map;             // M-14 总线/内存映射状态
  M15Periph m15Periph;          // M-15 外设模型状态
};
```

**没有它，落盘的东西还原不了一个引擎**（事件队列 / 插入序 / 指令预算全丢）。
**裁定**：结构补第 2 段 `m12Engine`（承载 `EngineSnapshotDto`），
并**同步订正两份 LLD 的结构清单**——正文已经要求它了，清单是漏列。

#### 裁决 B —— 「寄存器」有**两个真相**的风险

LLD 说 `m13State` = 「M-13 指令级状态序列化（**寄存器**/内存视图）」，
而 CPU 寄存器（`gpr` / `pc` / `csrs`）**已经在** `m12Engine.state`（平台状态
`SimState`）里了。**再存一份就是两个真相**——而「同一个量有两处落点」正是本项目
反复踩的坑（D-73 的自指断言、D-53 的静默降级）。

**裁定**：`m13State` **只承载内存视图（内存页）**；**寄存器不重复存**，
其唯一落点仍是 `m12Engine.state`。DTO 的 doc 里写明这条边界。

> ⚠️ 这是**措辞订正**，不是删字段：LLD 的「寄存器/内存视图」读作
> 「M-13 的**架构**状态视角」，其中寄存器部分由 `m12Engine.state` 承载。
> 两份 LLD 的 §2.1.1 注释同步订正。

#### 裁决 C —— M-16 的状态段

D-36 点名「**已拉起的 IRQ 行**」不在快照里。M-16 的 `InterruptController`
**有**一份真实状态（`raised: BTreeSet<u16>`），虽然它还是 stub。

**裁定**：会话载荷**加第 6 段 `m16Intc`**。理由：M-16 的状态**有生产者**
（`raise` / `lower`），不是「契约里有、没人产」的半截状态。
⚠️ **但 M-16 的 CSR 管道仍未接线**（它还是 stub）——所以段 6 在当前树里**恒为空**，
这是**如实反映现状**，不是缺陷。

---

## 2. 会话级载荷的总形状

```
SessionSnapshotDto {
  schema_version : u32                  // 落盘格式版本（会话载荷层）
  cp_meta        : CheckpointMetaDto    // 段 0 · M-20
  m12_engine     : EngineSnapshotDto    // 段 1 · M-12/M-17（复用 v2 已冻结 DTO）
  m13_state      : M13StateDto          // 段 2 · 内存页（RAM 内容）
  m14_map        : M14MapDto            // 段 3 · 内存映射拓扑
  m15_periph     : M15PeriphDto         // 段 4 · 外设内部状态
  m16_intc       : M16IntcDto           // 段 5 · 已拉起的 IRQ 行
}
```

**与 LLD §2.1.1 的差异只有两处**（且都是「补上正文已要求的东西」）：
1. 补 `m12Engine` 段（裁决 A）
2. 补 `m16Intc` 段（裁决 C）

`m13State` 的内容按裁决 B 收窄为「内存页」。

---

## 3. 逐段规格

### 3.1 会话级载荷（契约层 · 新文件 `crates/contract/src/session.rs`）

```rust
/// 会话级快照载荷的格式版本。
///
/// ⚠️ **与 `SNAPSHOT_SCHEMA_VERSION`（引擎段）是两个版本号**：
/// 外层管「会话载荷」的整体形状，内层管「M-12/M-17 段」的形状。
/// 改内层不一定要改外层（内层有自己的迁移链），改外层一定要改。
pub const SESSION_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
```

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `schema_version` | `u32` | 会话载荷格式版本 |
| `cp_meta` | `CheckpointMetaDto` | 段 0 |
| `m12_engine` | `EngineSnapshotDto` | 段 1（**v2 已冻结 DTO，不改形状**） |
| `m13_state` | `M13StateDto` | 段 2 |
| `m14_map` | `M14MapDto` | 段 3 |
| `m15_periph` | `M15PeriphDto` | 段 4 |
| `m16_intc` | `M16IntcDto` | 段 5 |

#### `CheckpointMetaDto`（段 0）

对应 LLD 的 `Checkpoint` 结构：

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `id` | `u32` | 检查点 id（单调） |
| `t_virt` | `u64` | 触发时虚拟时间 —— **十进制字符串**（§4） |
| `t_host` | `u64` | 触发时宿主时间（ms）—— **十进制字符串**（§4） |
| `base_seq` | `u32` | 基线序列号（后续输入事件起点） |
| `version` | `u32` | 落盘 schema 版本（= `SESSION_SNAPSHOT_SCHEMA_VERSION`） |

#### `M13StateDto`（段 2 · 内存页）

```rust
pub struct M13StateDto {
    /// 内存页：guest 可见的 RAM 内容。
    pub mem_pages: Vec<MemPageDto>,
}

pub struct MemPageDto {
    /// 页起始物理地址（页对齐）。
    pub base: u32,
    /// 页内容（长度 = 实际字节数，**不补齐到页边界**——末页可以短）。
    pub bytes: Vec<u8>,
}
```

> ⚠️ **寄存器不在这段**（裁决 B）：`gpr` / `pc` / `csrs` 的唯一落点是
> `m12_engine.state`（平台状态 `SimState`）。

#### `M14MapDto`（段 3 · 内存映射拓扑）

```rust
pub struct M14MapDto {
    /// 区域表（按 base 升序）。
    pub regions: Vec<RegionDto>,
}

pub struct RegionDto {
    pub base: u32,
    pub size: u32,
    /// 区域类型：`ram` / `rom` / `peripheral`（**小写字符串**，不是枚举序号）。
    pub kind: String,
    /// 目标标识：`ram` / `rom` → 空串；`peripheral` → 外设 id（如 `GPIO0`）。
    pub target: String,
}
```

> ⚠️ **`PeripheralBound`（活实例）不可序列化**——只记**外设 id**，恢复时按 id 重新绑定。
> ⚠️ **ROM 的内容不在拓扑里**——ROM 内容随固件镜像走，恢复时由固件重新装载
> （固件是会话的**输入**，不是状态）。**RAM 内容在段 2。**

#### `M15PeriphDto`（段 4 · 外设内部状态）

```rust
pub struct M15PeriphDto {
    pub uarts: Vec<UartStateDto>,
    pub gpios: Vec<GpioStateDto>,
    pub timers: Vec<TimerStateDto>,
}

pub struct UartStateDto {
    pub id: String,          // 外设 id
    pub tx: Option<u8>,      // 待发字节
    pub rx: Option<u8>,      // 已收字节
    pub status: u32,         // 状态寄存器
    pub irq_line: u16,
    pub now_ps: u64,         // ⚠️ 十进制字符串
}

pub struct GpioStateDto {
    pub id: String,
    pub directions: u32,     // 1 = 输出
    pub outputs: u32,
    pub inputs: u32,
    pub irq_lines: Vec<u16>, // 每 pin 一条
    pub pending_irqs: u32,
    pub now_ps: u64,         // ⚠️ 十进制字符串
}

pub struct TimerStateDto {
    pub id: String,
    pub irq_line: u16,
    pub mtime: u64,          // ⚠️ 十进制字符串
    pub mtimecmp: u64,       // ⚠️ 十进制字符串
}
```

#### `M16IntcDto`（段 5 · 已拉起的 IRQ 行）

```rust
pub struct M16IntcDto {
    /// 当前拉起的 IRQ 行（升序、去重）。
    pub raised: Vec<u16>,
}
```

### 3.2 M-14 的导出 / 载入接口

```rust
impl Bus {
    /// 导出内存页（RAM 内容）——段 2 的生产者。
    pub fn export_pages(&self) -> Vec<MemPageDto>;

    /// 回灌内存页（RAM 内容）——段 2 的消费者。
    /// ⚠️ **必须先 `load_map`**：页要写进已存在的区域里。
    pub fn load_pages(&mut self, pages: &[MemPageDto]) -> Result<(), PlatformError>;

    /// 导出内存映射拓扑——段 3 的生产者。
    pub fn export_map(&self) -> M14MapDto;

    /// 回灌内存映射拓扑——段 3 的消费者。
    /// ⚠️ **只重建 RAM / ROM 区域**（内容置零；ROM 随固件走、RAM 在段 2）。
    /// ⚠️ **外设区域原地保留**（既不删除、也不重建）——外设实例是会话的
    /// **输入**（固件绑定出来的活实例），不是状态。
    /// 删掉它们会让恢复后的总线**没有外设**，固件再跑一步就发散。
    pub fn load_map(&mut self, map: &M14MapDto) -> Result<(), PlatformError>;
}
```

### 3.3 M-15 的导出 / 载入接口

```rust
impl PeripheralRegistry {
    /// 导出全部外设的内部状态——段 4 的生产者。
    pub fn export_state(&self) -> M15PeriphDto;

    /// 回灌全部外设的内部状态——段 4 的消费者。
    /// ⚠️ **按 id 匹配**：找不到对应外设 → 报错（`E_PER-05` 一类的既有码），
    /// **不静默跳过**——静默跳过 = 丢状态 = 还原出来的会话不一样。
    pub fn load_state(&mut self, state: &M15PeriphDto) -> Result<(), PlatformError>;
}
```

### 3.4 M-16 的导出 / 载入接口

```rust
impl InterruptController {
    /// 导出已拉起的 IRQ 行——段 5 的生产者。
    pub fn export_state(&self) -> M16IntcDto;

    /// 回灌已拉起的 IRQ 行——段 5 的消费者。
    pub fn load_state(&mut self, state: &M16IntcDto);
}
```

### 3.5 M-20 的组装（`SnapshotManager`）

```rust
impl SnapshotManager {
    /// 组装**完整**会话载荷（六段全齐）。
    ///
    /// ⚠️ **六段由调用方提供**——M-20 是**记录器**，不是**采集器**。
    /// 采集（`bus.export_pages()` / `registry.export_state()` / …）由
    /// **内核宿主**做，因为它同时持有总线、外设注册表与中断控制器。
    /// 这样 M-20 **不依赖** M-14 / M-15 / M-16，三段可以各自演进。
    pub fn capture_session(
        &mut self,
        cp_meta: CheckpointMetaDto,
        engine: EngineSnapshotDto,
        m13_state: M13StateDto,
        m14_map: M14MapDto,
        m15_periph: M15PeriphDto,
        m16_intc: M16IntcDto,
    ) -> SessionSnapshotDto;
}
```

### 3.6 M-23 改吃新结构

```rust
pub trait DataStore {
    /// 持久化**会话级**快照载荷（D-37：不再吃 partial 的 `SnapshotRecord`）。
    fn put_snapshot(&mut self, s: &SessionSnapshotDto) -> Result<(), PlatformError>;
    /// 读取会话级快照载荷。
    fn get_snapshot(&self, id: u64) -> Result<Option<SessionSnapshotDto>, PlatformError>;
}
```

> ⚠️ **旧签名 `put_snapshot(&mut self, s: &SnapshotRecord)` 必须消失**——
> 否则「两个落盘格式并存」= 「哪个是真的说不清」。
> `SnapshotRecord`（内存态记录）**保留**，但它**不再**是落盘载体。

### 3.7 恢复顺序（**不变量**）

```
1. M-14 load_map(map)        // 先重建区域拓扑
2. M-14 load_pages(pages)     // 再把 RAM 内容写进去（需要区域已存在）
3. M-15 load_state(periph)   // 外设内部状态
4. M-16 load_state(intc)     // 已拉起的 IRQ 行
5. M-17 restore_full(engine) // 最后还原引擎段（队列 / 寄存器 / 虚拟时间）
```

⚠️ **顺序不可换**：页写不进不存在的区域；外设状态写不进未绑定的实例；
引擎段最后还原——它会**覆盖** `gpr` / `pc` / 虚拟时间。

---

## 4. 数值策略

**沿用契约层 v2 §2 的规则**：凡是「随仿真时长增长」的量（虚拟时间），线上表示为
**十进制字符串**（`crate::numstr::u64_decimal`）。

本轮新增的虚拟时间字段（**共 5 个**）：

| 字段 | 所在 |
| --- | --- |
| `t_virt` | `CheckpointMetaDto` |
| `t_host` | `CheckpointMetaDto` |
| `now_ps` | `UartStateDto` |
| `now_ps` | `GpioStateDto` |
| `mtime` | `TimerStateDto` |
| `mtimecmp` | `TimerStateDto` |

（`t_host` 是**宿主时间**（ms），但它同样随时间增长 → 同样走字符串。）

其余 `u64`（`base_seq` / `instruction_budget` / `next_seq` / `event_queue_head`）
保持 **JSON 数字**。

---

## 5. 校验（`validate`）

会话载荷必须**先校验后应用**（与 `EngineSnapshotDto::validate()` 同口径）：

| # | 检查 | 拒绝码 |
| --- | --- | --- |
| 1 | `schema_version` == `SESSION_SNAPSHOT_SCHEMA_VERSION` | `E_SNAP-05`（不支持的状态 schema 版本） |
| 2 | `cp_meta.version` == `SESSION_SNAPSHOT_SCHEMA_VERSION` | `E_SNAP-05` |
| 3 | 内层 `m12_engine.schema_version` == `SNAPSHOT_SCHEMA_VERSION` | `E_SNAP-05` |
| 4 | 内层 `m12_engine.state.schema_version` == `SIM_STATE_SCHEMA_VERSION` | `E_SNAP-05`（**落点在内层**，见下） |
| 5 | `m14_map.regions`：**按 base 升序且不重叠** | **`E_SNAP-06`**（会话载荷畸形） |
| 6 | `m13_state.mem_pages`：**按 base 升序且不重叠**；每页的 `bytes` 长度 ≤ 该区域大小 | **`E_SNAP-06`** |
| 7 | `m15_periph`：**同一类型内 id 不重复** | **`E_SNAP-06`** |
| 8 | `m16_intc.raised`：**升序且不重复** | **`E_SNAP-06`** |
| 9 | 内层 `m12_engine.validate()` 全部通过（队列到期 ≥ 虚拟时间 等） | 内层自己的码 |

> ⚠️ **第 4 项的落点唯一 —— 外层不重复检查**。
> 内层 `EngineSnapshotDto::validate()`（`crates/contract/src/snapshot.rs`）**已经**
> 用**同一个码** `E_SNAP-05` 拒 `state.schema_version`。外层再写一遍就是
> **「一个约束两个落点」**：删掉任一处行为都不变 → 两处都测不出判别力
> （D-73 的自指断言、D-53 的静默降级同源）。**内层是唯一落点。**
>
> ⚠️ **第 3 项相反：外层要查**——内层对引擎段版本用的是**另一个码**
> `E_SNAP_01_UNSUPPORTED_SCHEMA`，外层这处决定了对外面包报出哪个码。
> 「**码相同 → 归内层；码不同 → 外层保留**」是这两项分岔的**唯一理由**。

> ⚠️ **不借用 `E_SNAP-02`**：那个码的原义是「**事件队列**畸形」，拿它报
> 「区域表不排序」就是**一码多义**——前端拿到 `E_SNAP-02` 无法判断该查队列还是查内存图。
> 本轮**新开一个场景码** `E_SNAP-06`（会话载荷畸形），**一场景一码**
> （与 D-69 的 `E_PP`/`E_PER`、D-32 的 `E_PROTO-02` 同一个教训）。

⚠️ **校验必须发生在任何状态被写入之前**——半个应用完的快照比被拒绝的快照更糟。

---

## 6. 验收清单（合入前逐条核对）

| # | 验收项 | 判别式 |
| --- | --- | --- |
| 1 | 会话载荷的六段齐全，形状**精确**固定 | 契约层单测：JSON 精确形状 |
| 2 | **6 个虚拟时间字段**是十进制字符串 | 契约层单测：逐字段形状 |
| 3 | 浮点被拒 | 契约层单测 |
| 4 | `validate()` 的 9 类拒绝各自可复现 | 契约层单测（表驱动） |
| 5 | M-14 的 `export_map` ↔ `load_map` 往返一致 | 内核单测 |
| 6 | M-14 的 `export_pages` ↔ `load_pages` 往返一致 | 内核单测 |
| 7 | M-15 的 `export_state` ↔ `load_state` 往返一致 | 内核单测 |
| 8 | M-16 的 `export_state` ↔ `load_state` 往返一致 | 内核单测 |
| 9 | **组装出来的载荷能还原出一个等价会话**（端到端） | 集成测试：跑 N 步 → 快照 → 还原 → 再跑 M 步 → 与「不打断」跑 M 步的**最终状态逐字节相同** |
| 10 | M-23 落盘往返（JSON 落盘 → 读回 → 逐字段相等） | 集成测试 |
| 11 | `load_map` 未先调用时 `load_pages` **报错**（不是静默） | 内核单测 |
| 12 | M-15 的 `load_state` 遇到未知 id **报错**（不是静默跳过） | 内核单测 |
| 13 | 恢复后事件队列 / 插入序 / 指令预算与快照一致 | 集成测试 |
| 14 | 三关门禁全绿 | `fmt` / `clippy` / `cargo test --workspace --no-fail-fast` |

---

## 7. 不做什么

| 项 | 理由 |
| --- | --- |
| **M-16 的 CSR 管道**（`mip` / `mie` / `mcause` / `mepc`） | M-16 还是 stub，本轮只做**状态导出**（它有真实状态）；CSR 接线随 **M-16 落地**做 |
| **输入事件流 / `eventStream` / `replayCursor` / `CrashKit`** | LLD M-20 §1.3 的 MVP 范围里另有其物，但它们是**独立**的数据结构（`InputEvent` / `CrashKit`），与「快照载荷」是两件事；本轮只做载荷 |
| **ROM 内容进快照** | ROM 内容随固件镜像走（固件是输入，不是状态）；恢复时由固件重新装载 |
| **压缩快照**（`recordingFormat`） | LLD 明写 **P1 预留** |
| **`DebugMeta` 进快照** | 2026-10-20 裁决：项目级独立存储（键 `pdebug:`+projectId），**不随快照回滚** |
| **M-11 符号表 / DWARF** | 独立批次（10 人日，风险最高） |
| **会话快照的**协议入口**（`SaveSession` / `LoadSession` 命令） | 加一个**命令变体**就是**协议层**改动：v2 的 JS 镜像 `decodeMessage` 遇到未知变体会抛 `E_PROTO-08`，所以加变体必须同时 bump `SCHEMA_VERSION` —— 那是**另一次**契约升版，不属本批。本轮把会话快照做到**宿主内部 API**可达（`KernelHost::capture_session` / `restore_session`），协议入口留给下一批 |

---

## 8. 与既有文档的关系

| 文档 | 本轮动作 |
| --- | --- |
| `docs/detailed-design/M-20_Snapshot.html` §2.1.1 | 补 `m12Engine` 段 + `m16Intc` 段；`m13State` 的「寄存器/内存视图」措辞按裁决 B 订正；补 v1.4 裁决注记 |
| `docs/detailed-design/M-23_DataStore.html` §2.1.1 | 同上（两份 LLD 的 `Snapshot` 必须**逐字一致**） |
| `docs/module-design-spec/RISC-V_Module_Design_Specification.html` 表12 | `Snapshot` 行的关键字段补 `m12Engine` / `m16Intc` |
| `docs/reviews/REVIEWS.md` | D-36 / D-37 / D-41 闭环 + 数字口径重算 + §3.9 归档 |
| `crates/contract/src/snapshot.rs` | **不改形状**（v2 已冻结）；只在其模块头补一句与本规格的关系说明 |
