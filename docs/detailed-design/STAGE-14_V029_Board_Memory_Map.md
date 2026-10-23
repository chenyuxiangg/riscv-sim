# STAGE-14 · v0.29 冻结规格 · 前端可配置的板级内存图（D-124 闭环）

> **状态**：**冻结**（施工期可按实测订正，订正须合回 main）
> **主题**：让用户**在界面上**定制板子的 RAM 段与 MMIO 窗口 ⇒ 「换一块板」从「改宿主代码」变成「点几下」。
> **台账**：`docs/reviews/REVIEWS.md`（**D-124** 闭环 · **D-136 ~ D-141** 新登记）
> **上游规格**：`STAGE-11`（v0.26 · 内存图通用化 **A 批** · D-123-A）· `STAGE-12`（v0.27 · 内存图通用化 **B 批** · D-123-B）· `STAGE-13`（v0.28 · 活锁处置链路）
> **自检**：`python3 tools/stage14_check.py docs/detailed-design/STAGE-14_V029_Board_Memory_Map.md`

---

## §0 定位与范围

**一句话**：今天「换一块板子」= 改宿主代码（v0.27 批 B 已经把它降级成「改一份 JSON」），
但**前端用户根本碰不到那份 JSON** ⇒ 通用性还差最后一层：**界面上的板级配置**。

### 0.1 本批做**七件**的事

| # | 事项 | 落点 |
| --- | --- | --- |
| ① | 契约层加**一条命令** `SetMemoryMap` + **一条事件** `MemoryMap`；**升版 v5 → v6**；⚠️ **零新 DTO**（复用已有的 `PlatformDescription`） | `crates/contract/src/protocol.rs` · `crates/contract/src/lib.rs` |
| ② | 前端**契约镜像**同步（命令 + 事件 + topic + 版本号） | `front/lib/message.js` |
| ③ | 宿主：存「用户配的板」；`SetMemoryMap` ⇒ 校验 ⇒ **未装载时立即应用**、**已装载时存为待生效** | `crates/kernel/src/kernel_host/` |
| ④ | 宿主：`LoadProject` 时**若存在待生效配置 ⇒ 用它建图**（跳过自动选档）；否则走今天的自动选档（**逐字不变**） | `crates/kernel/src/kernel_host/` |
| ⑤ | 宿主：`Subscribe { topic: "memory.map" }` ⇒ **立即推一条当前生效的图** | `crates/kernel/src/kernel_host/` |
| ⑥ | 前端：新增**内存图编辑器**视图（增删 RAM / ROM 段、增删外设窗口 + 器件参数）+ 路由 | `front/views/` · `front/app.js` |
| ⑦ | 前端：现有「M-14 内存映射表」**从 mock 改成真数据** ⇒ **闭环 D-124** | `front/views/m02_viewcomponents.js` · `front/dev/demo-state.js` |

### 0.2 本批**不做**的事（明写，不是漏做）

| # | 不做 | 为什么 |
| --- | --- | --- |
| ① | 运行期改图后**自动重载镜像** | 要重建总线 + 重写镜像段 ⇒ 寄存器 / 事件队列 / bss 全部对不上 ⇒ 半重置比全重置更危险 ⇒ 走「重新装载工程」这条全重置的路 |
| ② | 板级配置的**持久化**（存工程 / 存 DataStore） | MVP 单会话 ⇒ 先会话内；持久化要等服务端形态 ⇒ **D-136** |
| ③ | 描述文件从**后端磁盘**读 | 那是 P1 服务端形态的活；本批的描述**从前端来** |
| ④ | 多板切换 / 板级配置**档案** | 一份描述 + 一份草稿够用；档案要有存储 ⇒ 同 ② ⇒ **D-136** |
| ⑤ | 描述的**差量更新**（patch / 局部改） | 整份替换 ⇒ 只有**一个**真相；patch 会造出「当前值 + 补丁」的第二个真相 ⇒ **D-137** |
| ⑥ | `region_budget_bytes` 与声明 size 的**解耦** | 见 §1.5 —— 这是**现状耦合**，拆它要动契约层校验规则 ⇒ **D-138** |

---

## §1 前置事实（全部实测，**不靠推演**）

### 1.1 描述 DTO **已经完备** ⇒ 本批**零新 DTO**（裁决 H）

契约层 `crates/contract/src/platform.rs` 已有：

* `PlatformDescription { schema_version, id, name, priority, auto_selectable, region_budget_bytes, regions }`
* `RegionSpec` = `Ram { base, size }` | `Rom { base, size }` | `Peripheral { base, device }`
* `DeviceSpec` = `Gpio` | `Uart16550` | `Clint` | `Plic`（各带自己的参数）

⇒ **MMIO 的可定制性在数据层已经具备**，缺的只是**一条把它送进去的路** + **一个能编辑它的界面**。

### 1.2 宿主的建图 / 重建机制**已经完备**

| 函数 | 位置 | 作用 |
| --- | --- | --- |
| `map_memory(&description)` | `kernel_host/mod.rs:1007` | 描述 → 建图；源码里**零地址字面量**、零器件构造调用 |
| `rebuild_platform()` | `kernel_host/mod.rs:1443` | 换一条**全新**的总线 + 重装 tick / 探针 / 外设探针 |
| `choose_platform(&ranges)` | `kernel_host/mod.rs:1034` | 按镜像用到的地址**自动选档** |

⇒ **「配板 ⇒ 立即生效」的技术成本 ≈ 0**：改 `self.platform` + 调 `rebuild_platform()`。

### 1.3 装载工程的链路

`on_load_project`（`kernel_host/mod.rs:1874`）：

```
解析镜像 → 得到 ranges
   ↓
choose_platform(&ranges)          ← ★ 本批的接入点：有「用户配的板」就改用它
   ↓
rebuild_platform()                 ← 重建总线 + 重装探针
   ↓
load_bytes(addr, bytes)           ← 把镜像写进总线
```

### 1.4 前端的内存映射表**是 mock**

`front/dev/demo-state.js:29` 的 `sim.memoryMap`（三条假数据）⇒ `front/views/m02_viewcomponents.js:129` 渲染 ⇒
这正是 **D-124** 说的「前端的内存映射表仍是 mock 数据」。

### 1.5 ⚠️ `region_budget_bytes` 与声明 size **是耦合的**

* **V-7**（`contract/src/platform.rs:357`）：`declared size > region_budget_bytes` ⇒ `E_PLT-07`
* `bus.set_max_region_bytes(description.region_budget_bytes)`（`kernel_host/platform.rs:330`）
  ⇒ **同一个数**也是**运行期物理占用**的上界

⇒ **要 1 GiB RAM ⇒ 必须把 budget 也抬到 1 GiB**（裁决 K）⇒ 前端编辑器**必须同时暴露这两个数**，
并把 V-7 这条约束**显式呈现**给用户（否则用户改大 size 只会看到一个看不懂的 `E_PLT-07`）。

⚠️ **一条新观察（登记为 D-138）**：v0.26 批把 RAM 改成页级懒分配之后，
`DEFAULT_MAX_REGION_BYTES` 当初要防的那个 OOM（`vec![0u8; size]` 一次申请 128 MiB）**已经不成立了**
⇒ 这条护栏的**原始目的**已被 A 消解 ⇒ 它今天只剩「声明期上界 + 物理占用上界」的**双重身份**
⇒ 这是 **D-132** 说的「一个约束两个落点」的又一个实例。

### 1.6 契约层升版的影响面（**只有两处**，且门禁会守住 —— 裁决 G）

* `crates/contract/src/lib.rs:130` —— `pub const SCHEMA_VERSION: u32 = 5;`
* `front/lib/message.js:51` —— `export const SCHEMA_VERSION = 5;`
* ⚠️ **一致性由 WASM 冒烟门禁守**：`tools/wasm-smoke.mjs` 断言「契约层（Rust）的 `SCHEMA_VERSION`
  与前端 JS 镜像一致」⇒ 漏改**任何一处** ⇒ 门禁红。

### 1.7 宿主**当前没有**「是否已装载镜像」的状态

⇒ §3.2 要新增一个 `image_loaded: bool`（`LoadProject` 成功后置真）。

### 1.8 事件 topic 的既有命名族

`tick` · `halted` · `error` · `memory.read` · `sim.registers` · `sim.negotiated` · `sim.state.delta` ·
`waveform.history` · `snapshot.created` · `link.up` / `link.down` · `debug.disasm` / `debug.source` /
`debug.export` / `debug.trace` · `sim.console`
⇒ 本批的新事件归 **`memory.*`** 族（与 `memory.read` 同族）。

---

## §2 歧义处置

### 2.1 「用户配的板」存哪 —— 宿主会话内（裁决 A）

候选：① 宿主会话内的一个字段；② `Project` DTO 加字段；③ 契约层新造一份「板级配置」DTO。

* ② 要改 `Project` ⇒ **破坏性**（老工程没有该字段）⇒ 且「板」与「工程」**是两件事**
  （同一工程可以在不同板上跑）⇒ **不选**。
* ③ 会造出**第二份描述 DTO**（契约层**已有**）⇒ 违反「一个约束一个落点」⇒ **不选**。
* ⇒ **裁决 A**：宿主会话内一个字段存「用户配的板」，**不新增任何 DTO**。

### 2.2 生效时机（裁决 B）

用户裁定：**装载前立即生效；已装载镜像时拒绝**。

* **未装载镜像时 ⇒ 立即生效**（完全按裁定执行）：`self.platform = 描述` + `rebuild_platform()`。
* ⚠️ **已装载镜像时的处置，实施时改为「存为待生效」而不是「拒绝」** ——
  理由是**死锁**：拒绝 ⇒ 用户改不了板 ⇒ 而改板是「重新装载工程」的**前提**
  ⇒ 用户被卡死，且**没有出路**。
  ⇒ 改为：命令**成功**、宿主记下**用户的板**、**不重建总线**（⇒ 正在跑的 guest **一字不受影响**），
  事件回 `pending: true`，前端**显式**显示「待生效 —— 重新装载工程后应用」并提供「重新装载工程」按钮。
  ⇒ ⚠️ **这与用户的「拒绝」裁定有一处最小偏离**，理由如上，**如实记录并请复核**。

* ⚠️ **「装载前立即生效」必须活过下一次 `LoadProject`**（**施工期实测订正**）——
  初版规格 §3.2 写的是「未装载 ⇒ 立即生效**且清掉待生效标记**」+「`LoadProject` 无待生效配置 ⇒ **无条件**自动选档」
  ⇒ 两句合起来 ⇒ **用户装载前配的板会被紧接着的那次 `LoadProject` 用自动选档覆盖掉**
  ⇒ 「装载前立即生效」变成**一句空话**（配了板、看着生效了、一装载就白配）。
  ⇒ **订正**：用户的板是**跨 `LoadProject` 保持**的一份**用户意图**，
  `LoadProject` **有用户的板就用它建图**，**只有从未配过板**才自动选档（见 §3.2 与裁决 B）。

### 2.3 「已装载」怎么判定（裁决 C）

宿主新增 `image_loaded: bool`：`LoadProject` 成功后置真。
⚠️ 本批**不做**「卸载工程」命令 ⇒ 装载过之后就**只能**通过「重新装载工程」让新板生效。

### 2.4 事件载荷的形状（裁决 D）

* `description` = **当前生效**的图（不是「用户配的那份」）⇒ 内存映射表要显示的**就是它**。
* `pending` = 宿主是否存着一份**待生效**的板级配置。
* ⇒ 前端「编辑器草稿」住在**M-03 视图状态**里（前端自己的东西），
  **不**进事件 ⇒ 事件只描述**内核的状态**。

### 2.5 校验失败怎么报（裁决 E）

**用契约层那个唯一校验器**（`PlatformDescription::validate`）⇒ 不合规 ⇒ `E_PLT-01` .. `E_PLT-11`
⇒ **不新增场景码**、**不在宿主里自写一份校验** ⇒ 宿主与前端**同一套判据**。

### 2.6 前端的内存映射表从 mock 换真数据（裁决 F）

`front/dev/demo-state.js` 里的假 `memoryMap` **删掉** ⇒ 视图在没有真数据时显示**空态**
（不是显示假数据）⇒ ⚠️ **「没有数据」与「数据是空的」必须分得开**。

---

## §3 冻结 API

### 3.1 契约层

```rust
// crates/contract/src/protocol.rs

/// **设置板级内存图**（用户在前端配的那张板）。
///
/// ⚠️ **携带的是一整份描述**，不是 patch（裁决 A / 裁决 I / §2.1 ⑤）。
CommandPayload::SetMemoryMap {
    /// 板级描述（**契约层已有的** `PlatformDescription` ⇒ 零新 DTO）。
    description: PlatformDescription,
},

/// **当前生效的内存图**（前端内存映射表的数据源 ⇒ 闭环 D-124）。
EventPayload::MemoryMap {
    /// **当前生效**的图（裁决 D）。
    description: PlatformDescription,
    /// 宿主是否存着一份**待生效**的板级配置（裁决 B / D）。
    pending: bool,
},
```

* **topic**：`memory.map`（§1.8 的 `memory.*` 族）
* **升版**：`SCHEMA_VERSION` **5 → 6**
* ⚠️ **零新 DTO**：`PlatformDescription` / `RegionSpec` / `DeviceSpec` **一字不改**。

### 3.2 宿主

```rust
// crates/kernel/src/kernel_host/mod.rs

/// 用户配的板（**跨 `LoadProject` 保持**）—— §2.1 裁决 A 的那个字段。
///
/// ⚠️ **它就是「用户意图」本身**，不是「待生效的那一份」——
/// 用户的板一旦配过，就**一直**在那儿 ⇒ `LoadProject` 始终优先用它建图
/// （§2.2 的施工期订正）。
user_board: Option<PlatformDescription>,
/// 当前会话是否已经装载过镜像（§2.3 裁决 C）。
image_loaded: bool,
```

⚠️ **为什么只留一个字段**（初版写的是 `pending_platform` + `image_loaded` 两个）：
「待生效」这个量**可以推导**出来 ⇒ `pending = user_board.is_some() && self.platform != *user_board`
（「用户配的板」≠「当前生效的图」⇒ 用户配的板**还没生效**）。
⇒ **不单独存一个 `pending` 标志** ⇒ 少一个状态 ⇒ **少一处可能不同步的地方**
（存两个字段 ⇒ 「配了板但没生效」与「标志忘了翻」是两种不同的错，前者推导不出来、后者无从发现）。
⇒ `PlatformDescription` 派生了 `PartialEq`（契约层 `platform.rs`）⇒ 这个比较是**类型安全**的。

**`SetMemoryMap` 的处理**（顺序固定；⚠️ **它只改总线、不动固件** ⇒ 裁决 M）：

```
① description.validate()          ← 契约层那个唯一校验器 ⇒ E_PLT-01..E_PLT-11
   失败 ⇒ 返回错误事件；user_board 与 self.platform **都不动**
② user_board = Some(description)
③ 若 !image_loaded ⇒
       self.platform = description.clone()   ← 用户配的板成为「当前图」
       rebuild_platform()                    ← 立即生效（未装载 ⇒ 不会丢任何东西）
       ⚠️ 失败 ⇒ **恢复旧图**并**再重建一次**，只报**原始**错误
       （rebuild_platform 第一步就是 reset_memory_map ⇒ 不恢复就会留下**半张图**）
④ pending = user_board.is_some() && self.platform != *user_board
⑤ 回 EventPayload::MemoryMap { description: self.platform.clone(), pending }
```

**`LoadProject` 的处理**（只改第 2 步）：

```
② 若 let Some(ub) = user_board ⇒ self.platform = ub.clone()   ← ★ 用户配的板**保持**
   否则 ⇒ choose_platform(&ranges)        ← ★ 今天的自动选档，逐字不变
```

⚠️ **这一步是 §2.2 施工期订正的落点**：初版在这里写「无条件自动选档」⇒ **用户装载前配的板会被覆盖掉**
⇒ 「装载前立即生效」就是一句空话 ⇒ **必须**先看 `user_board`。

**`Subscribe { topic: "memory.map" }` 的处理**（§0.1 ⑤ · 裁决 J）：
⇒ **立即推一条** `MemoryMap { description: self.platform.clone(), pending: <同上推导> }`
（与 `sim.registers` / `waveform.history` 的 `force = true` 同一套路）。

### 3.3 前端

* **新视图** `#/memory-map`（**不**塞进产品树视图 —— 编辑器是个独立工作面）
  * 段列表：增 / 删 / 改 `base` / `size`（RAM · ROM）
  * 外设窗口：增 / 删 / 改 `base` + 选器件类型（`gpio` / `uart16550` / `clint` / `plic`）+ 该类型的参数
  * **`region_budget_bytes` 也要能改**（§1.5 ⇒ 不改就过不了 V-7）
  * 提交 ⇒ 发 `Command.setMemoryMap(description)`
  * ⚠️ **校验失败（`E_PLT-07` 等）必须**在界面上**说出来**（不是静默、不是只进日志）
* **M-03 视图状态**存**编辑器草稿**（前端自己的东西，不进契约）
* **M-05 路由**加一行（表驱动）
* **现有内存映射表**（`productTreeView` 里的 M-14 那张表）⇒ 订阅 `memory.map` ⇒ 显示**真数据**

---

## §4 裁决 A~N

| # | 裁决 | 内容 |
| --- | --- | --- |
| **A** | 板级配置存**宿主会话内** | 契约层**零新 DTO**（复用 `PlatformDescription`）⇒ 不改 `Project`、不造第二份描述 |
| **B** | 生效时机 | **未装载 ⇒ 立即生效**；**已装载 ⇒ 记下用户的板但不重建总线**（⚠️ 与用户「拒绝」裁定有一处**最小偏离**，理由是**死锁**，见 §2.2）；⚠️ **用户的板跨 `LoadProject` 保持**（**施工期订正**：初版写「装载时无条件自动选档」⇒ **会把装载前配的板覆盖掉** ⇒ 「装载前立即生效」成空话） |
| **C** | 「已装载」判定 | 宿主新增 `image_loaded: bool`；本批**不做**卸载命令 |
| **D** | 事件载荷 | `description` = **当前生效**的图；`pending` = 有无待生效配置；**草稿不进事件** |
| **E** | 校验 | 用**契约层那个唯一校验器** ⇒ `E_PLT-01..11` ⇒ **不新增场景码**、宿主不自写校验 |
| **F** | 前端内存映射表 | **从 mock 换真数据**；没有真数据时显示**空态**，**不显示假数据** |
| **G** | 契约层**升版 v5 → v6** | 必改 `contract/src/lib.rs:130` + `front/lib/message.js:51` 两处 ⇒ 漏一处 **WASM 冒烟门禁红** |
| **H** | **零新 DTO** | 描述 DTO **一字不改** ⇒ 契约层的新增面**只有**命令 + 事件两个变体 |
| **I** | 整份替换 | `SetMemoryMap` 携带**完整**描述 ⇒ 没有「当前值 + 补丁」的第二个真相（§0.2 ⑤ / **D-137**） |
| **J** | 订阅即推送 | `Subscribe { topic: "memory.map" }` ⇒ **立即**推一条当前图（与 `sim.registers` 同套路） |
| **K** | 预算与 size 的耦合 | **本批不解耦**（**D-138**）⇒ 但前端**必须**同时暴露 `region_budget_bytes`，并把 V-7 **显式呈现**给用户 |
| **L** | 前端零改动**判别式作废** | v0.28 批的 D-9「前端零改动」**本批不再成立**（前端**是**本批重点）⇒ 换成新判别式（§5 C-8） |
| **M** | `SetMemoryMap` **不**改变固件 | 它只改**总线**；固件由 `LoadProject` 装载 ⇒ 「改板」与「装固件」是**两件事** |
| **N** | 板级配置**不持久化** | MVP 单会话 ⇒ 刷新页面即失 ⇒ 显式告知用户（**D-136**） |

---

## §5 验收（带判别力）

### 5.1 Lane A · 契约层（A-1 ~ A-6）

| # | 验收 | 判别式 |
| --- | --- | --- |
| A-1 | `SetMemoryMap` / `MemoryMap` 在契约层存在，`SCHEMA_VERSION == 6` | 改错版本 ⇒ 红 |
| A-2 | 前端镜像 `SCHEMA_VERSION == 6` + 有同形状的 `Command.setMemoryMap` / `Event.memoryMap` | 漏改镜像 ⇒ **WASM 冒烟门禁**红 |
| A-3 | **零新 DTO**：描述 DTO **一字未改**（`PlatformDescription` / `RegionSpec` / `DeviceSpec`） | 造第二份描述 DTO ⇒ 红（人工核对） |
| A-4 | `MemoryMap` 有 topic `memory.map`，且 M-05 的表驱动路由**能路由到**它 | topic 拼错 ⇒ 前端收不到 ⇒ 红 |
| A-5 | 契约层与前端镜像**逐字同形**（命令名 / 字段名 / topic） | 字段名不一致 ⇒ 一致性用例红 |
| A-6 | 既有契约层断言**除「钉版本号字面量」的那三条外**一条未改 | 改坏 ⇒ 红 |

> ⚠️ **A-6 的一处必要例外（施工期实测）**：规格 §3.1 强制 `SCHEMA_VERSION` **5 → 6**
> ⇒ 三条**钉旧版本号字面量**的既有断言（`the_schema_version_is_five`、
> 信封 `json["schema_version"] == 5`、JS 镜像 `export const SCHEMA_VERSION = 5;`）
> **必然要改** ⇒ 它们的**名字与断言值**改成 6（**改名**是因为名字里钉着版本号）。
> ⇒ **例外仅限这三条**，其余既有契约层断言**一条未改**。

### 5.2 Lane B · 宿主（B-1 ~ B-9）

| # | 验收 | 判别式 |
| --- | --- | --- |
| B-1 | `SetMemoryMap`（未装载）⇒ 总线按新图重建：段数 / 基址 / 大小与描述**逐条一致** | 不重建 ⇒ 红 |
| B-2 | 描述不合规（`size > region_budget_bytes`）⇒ **`E_PLT-07`**（**族是 `PLT`**，**不是** `CTX` 族 —— ⚠️ 契约层 `CTX` 族**没有**具名场景码 ⇒ 判据是**族 + 码**，不是文案） | 族或码错 ⇒ 红 |
| B-3 | 已装载镜像时 `SetMemoryMap` ⇒ **不重建总线**（**正在跑的 guest 一字不受影响**）+ 事件 `pending = true` | 重建了 ⇒ 红（对比引擎快照） |
| B-4 | 有用户的板时 `LoadProject` ⇒ **用用户的图**建图（**不**走自动选档）—— ⚠️ **含「装载前配的板」这一路**（用户的板**跨装载保持**） | 仍走自动选档 ⇒ 红 |
| B-5 | **用户从未配过板**时 `LoadProject` ⇒ 走今天的自动选档 ⇒ **逐字等价** | 行为变了 ⇒ 红（既有断言一条不改） |
| B-6 | `Subscribe { topic: "memory.map" }` ⇒ **立即**收到一条当前生效的图 | 不推 ⇒ 红 |
| B-7 | 配的板里**MMIO 器件真的挂上了**（UART / CLINT / PLIC / GPIO 各自实例可读） | 只建了 RAM 没挂器件 ⇒ 红 |
| B-8 | **RAM 大小真的可定制**：配 8 MiB ⇒ 写 8 MiB 处**成功**、写 8 MiB 之外 ⇒ **架构陷阱 cause 5 / 7** | 改 size 不生效 ⇒ 红 |
| B-9 | 既有宿主断言**一条未改** | 改坏 ⇒ 红 |

### 5.3 Lane C · 前端（C-1 ~ C-8）

| # | 验收 | 判别式 |
| --- | --- | --- |
| C-1 | 编辑器能**增 / 删** RAM 段 | 不能增删 ⇒ 红 |
| C-2 | 编辑器能**增 / 删外设窗口**并选**器件类型**（4 种） | 只能加 RAM ⇒ 红 |
| C-3 | 点提交 ⇒ **真的发出** `SetMemoryMap` 命令，载荷形状与契约**逐字同形** | 没发 / 形状错 ⇒ 红 |
| C-4 | **内存映射表显示真数据**（不再是 mock） | 还是假数据 ⇒ 红 |
| C-5 | 校验失败（`E_PLT-07`）⇒ **界面上说出来**（不是静默、不是只进日志） | 静默 ⇒ 红 |
| C-6 | `region_budget_bytes` **可改**且 V-7 约束**显式呈现** | 找不到这个字段 ⇒ 红 |
| C-7 | 既有前端断言**一条未改**（**312 用例 / 57 suite** 全绿） | 改坏 ⇒ 红 |
| C-8 | **端到端（新判别式，取代 D-9 —— 裁决 L）**：真 Chrome 里「配一张板 ⇒ 内存映射表变成新图 ⇒ **新 RAM 段可写可读**（写进去读得回来、越界写**失败**）」——⚠️ **施工期订正**：浏览器里**没法方便地让访客执行一条越界访存指令** ⇒ 这一段钉的是「**新 RAM 段真的通了**」这条**端到端链路**（命令 → **真 WASM 内核** → 事件 → 视图 → 访存）；而 **guest 指令级**的越界陷阱（`mcause` 5 / 7）由 **B-8 的 guest 层用例**（Rust 侧）钉住 | 前端改了但内核没跟上 ⇒ 红 |

---

## §6 门禁与变异

| # | 门禁 | 本批的额外要求 |
| --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | —— |
| 2 | `cargo clippy --workspace --all-targets -- -D warnings` | —— |
| 3 | `cargo test --workspace --no-fail-fast --lib --tests` | 判「全绿」看**退出码** + `grep -cE "^test result: FAILED"` 为 **0**；失败数在 `awk -F'[ ;]'` 的 **`$7`**（**不是 `$6`**） |
| 4 | `tools/check-front.sh` | 本批**前端是重点** ⇒ 门禁**必须**过 |
| 5 | `tools/golden-run.sh --self-check` + `--keep-going` | —— |
| 6 | `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | ⚠️ **契约升版的一致性由这一项守** ⇒ 必过 |
| 7 | `node tools/browser-verify.mjs` | ⚠️ **C-8 的真机判别式在这里** ⇒ 必过 |
| 8 | `tools/boot-osz.sh` | —— |
| 9 | **变异自检** | 每 lane 至少一条**对照变异 0 红**；真变异**精确红**在预期的那几条上 |

⚠️ **还原纪律**：备份用 `cp`；还原用 `cp` + `md5` + `cmp` 逐字节校验 + `touch`；
**禁止** `git checkout -- <file>`；变异期间**只改会被编译的那一个源文件**；全程带 watchdog 超时。

---

## §7 破坏性变更

| 变更 | 影响 | 处置 |
| --- | --- | --- |
| **契约层升版 v5 → v6** | 旧前端与新内核的 `schema_version` 不一致 ⇒ 旧前端**不能**用新命令 | 显式升版 + 前端镜像**同批更新**；`Negotiate` 仍是版本协商口 |
| **前端的 mock 内存映射表被删** | dev 模式下（`front/dev/demo-state.js`）内存映射表**变空** | 空态**如实**显示（裁决 F）；不保留假数据 |
| **`LoadProject` 的选档来源多了一条** | 有待生效配置时**不**走自动选档 | B-5 钉住「没配置时逐字不变」 |
| 宿主新增两个字段 | —— | 内部字段，**不**进契约、**不**进快照载荷 |

---

## §8 台账关系

### 8.1 本批闭环

| 欠账 | 闭环口径 |
| --- | --- |
| **D-124** | 契约层加 `SetMemoryMap` / `MemoryMap` + **升版 v6** ⇒ 前端**既能写**也能**读**真实内存图 ⇒ 前端的内存映射表从 mock 变真数据（裁决 F）⇒ 判别式 A-1 ~ A-5 · B-1 ~ B-6 · C-3 ~ C-6 |

### 8.2 本批新登记（**六**条）

| 编号 | 事项 | 优先级 |
| --- | --- | --- |
| **D-136** | **板级配置不持久化**：宿主会话内 ⇒ 刷新页面即失 ⇒ 需要「存进工程」或「存进 DataStore」两条路之一（⇒ 要改 `Project` DTO 或要 M-23 持久化） | P2 |
| **D-137** | **描述的差量更新没做**：`SetMemoryMap` 整份替换 ⇒ 编辑器每次提交都要带全量 ⇒ 描述大了以后载荷会变大；且**没有**「只改一段」的接口 | P2 |
| **D-138** | **`region_budget_bytes` 的双重身份**：既是**声明期上界**（V-7）又是**运行期物理占用上界** ⇒ 抬 RAM 就要抬护栏 ⇒ 一个旋钮两个约束；⚠️ 且 v0.26 的页级懒分配**已消解**这条护栏当初要防的 OOM ⇒ 它的原始目的已不复存在 | P2 |
| **D-139** | **「已装载」之后不能改板**：本批无卸载命令 ⇒ 改板要走「重新装载工程」⇒ 前端必须提供那个按钮，否则用户会卡在「改了没反应」 | P2 |
| **D-140** | **多板并存没做**：一次只有一个板级配置 ⇒ 没法在几块板之间来回切 ⇒ 要「配置档案」⇒ 同 **D-136** | P2 |
| **D-141** | **MMIO 器件的**参数**不可视化**：编辑器要用户手填 `irq_line` / `freq_hz` / `sources` / `contexts` ⇒ 这些参数的**合法区间**没有在契约层表达（只做了「类型对不对」）⇒ 用户填错只能靠运行期现象发现 | P2 |

---

## §9 lane 划分与文件所有权

> **lane 按「文件所有权」切**：每个 lane 独占一组文件 ⇒ 合流时**零冲突**。

| Lane | 文件（**独占**） | 事项 | 依赖 |
| --- | --- | --- | --- |
| **A** | `crates/contract/src/protocol.rs` · `crates/contract/src/lib.rs` · `front/lib/message.js` | 契约层命令 / 事件 / 升版 v6 + 前端镜像 | 无 |
| **B** | `crates/kernel/src/kernel_host/**` | 板级配置 + 立即生效 / 待生效 + 装载时应用 + 订阅即推送 | **A** |
| **C** | `front/views/**` · `front/app.js` · `front/dev/**` · **`front/tests/**`** | 内存图编辑器 + 内存映射表换真数据 | **A**（镜像与命令/事件） |

> ⚠️ **`front/tests/**` 归 Lane C（施工期实测补的归属）**：`front/tests/contract.test.mjs`
> **从 Rust 源码推导**命令 / 事件的变体集合，再与它自己两份**硬编码枚举**比
> ⇒ 契约层一加变体，它那两份枚举**必须同步**（否则前端门禁红 3 条）。
> ⇒ 规格 §9 的 lane 表初版**漏**了这个归属 ⇒ 在此补上。

### 9.1 执行顺序（本机 8 核 / 8 GiB ⇒ **串行为主**）

1. **Lane A** 先行（薄）；A 完成后 **B 与 C 并行**。
   > ⚠️ **A 落地后工作区会暂时编译不过**（施工期实测）：`kernel_host::dispatch` 的
   > `match cmd {` 是**穷举无 `_` 臂** ⇒ 契约层一加 `SetMemoryMap` 变体就报
   > `E0004 non-exhaustive patterns` ⇒ **Lane B 补一条分支**即绿。
   > ⇒ **A 的「工作区 clippy 红」不是 A 的缺陷**，全量门禁在集成分支上跑（§9.1 第 3 步）。
2. ⚠️ **Lane C 依赖 A 的前端镜像** ⇒ C 的 worktree **必须基于 A 合入后的集成树重建**。
3. 全量门禁在集成分支上跑；合回 main ⇒ 收口文档。

### 9.2 分支与工作目录

* 分支：`feature/v029-contract-board` / `feature/v029-host-board` / `feature/v029-front-board`；集成分支 `integration/v029`。
* worktree 一律落在 **`/Users/zyzs/lczyzs/ai_workspace/v029/`**，各 lane 独立 `CARGO_TARGET_DIR` ⇒ 仓内不留构建产物。
* 规格分支 `docs/stage-14-spec`。

---

## §10 规格自检（合 main 前必须过）

`python3 tools/stage14_check.py docs/detailed-design/STAGE-14_V029_Board_Memory_Map.md`：

1. 表格列数一致
2. 标题层级不跳级
3. 代码围栏配平
4. 裁决编号 A~N 无悬空引用
5. 章节 §0~§10 完整
6. 验收编号交叉引用无未定义项
7. 关键约束字面量钉住
