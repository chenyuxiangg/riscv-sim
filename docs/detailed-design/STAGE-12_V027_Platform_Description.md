# STAGE-12 · v0.27 冻结规格 · 内存图通用化 B 批（D-123-B）

> **状态**：**草案**（待合 `main` 冻结）—— 本批的范围、裁决、验收清单与门禁以本文档为准。
> **上游**：`STAGE-11`（v0.26 · 内存图通用化 **A 批** · D-123-A）—— RAM 已改页级懒分配 ⇒ **一段 128 MiB 的 RAM 登记成本 ≈ 0** ⇒ 本批换描述时不会换出一份 OOM。
> **台账**：`docs/reviews/REVIEWS.md`（**D-123** · **D-82** · D-10 · D-13）。

---

## §0 范围

### 0.1 本批做（7 件）

| # | 做什么 | 落点 | 台账 |
| --- | --- | --- | --- |
| 1 | **平台描述格式**（JSON）与其 **DTO + 校验器** | 契约层新文件 `crates/contract/src/platform.rs` | D-123-B |
| 2 | **两份内置描述**（`default` / `qemu-virt`），**逐字复现**今天的两档 | `crates/kernel/src/kernel_host/platforms/*.json` | D-123-B |
| 3 | **宿主从描述建图**（`map_memory` 不再硬编码档位） | `crates/kernel/src/kernel_host/` | D-123-B |
| 4 | **器件工厂**（描述 → 实例），**一个 match、一个落点** | `crates/kernel/src/kernel_host/platform.rs` | D-123-B |
| 5 | **自动选档判据**描述化（逐字等价于今天） | `crates/kernel/src/kernel_host/mod.rs` | D-123-B |
| 6 | **第三份演示描述**（新平台）⇒ 证明「换描述、宿主零改动」 | `crates/kernel/src/kernel_host/platforms/minimal.json` | D-123-B |
| 7 | **D-82 闭环**：补 guest 指令级端到端判别力 + **订正台账措辞** | `crates/kernel/tests/` | D-82 |

### 0.2 本批**明确不做**的事（10 条）

| # | 不做 | 为什么 |
| --- | --- | --- |
| 1 | **契约层升版本** | 平台描述**不是会话载荷**（不进 `SessionSnapshotDto`）⇒ `schema_version` 不动 ⇒ 前端镜像不动 |
| 2 | **新增 MemoryMap 命令 / 事件** | 那要升契约版本 + 改前端 ⇒ 属 P1（见 §0.4 的欠账登记） |
| 3 | **前端内存映射表接真实数据** | 前端 `sim.memoryMap` 今天就是 **dev 假数据**（`front/dev/demo-state.js:29-33`，与两档**都不一致**）⇒ 它**不是**验收基准；接真实数据要新增契约事件（不做 #2） |
| 4 | **M-14 改动** | 描述的**消费者只有宿主** ⇒ M-14 零改动（裁决 F） |
| 5 | **器件之间的信号连线路由化** | 「UART 的 RX 就绪 → PLIC 的源 10」是**连线**不是地址 ⇒ 描述覆盖「有什么、在哪」，不覆盖「谁连谁」（裁决 G ⇒ 登记为欠账） |
| 6 | **`riscv-golden.rs` / `tools/*.sh` 的地址副本** | 那是**参考侧对拍**的职责（要与 Spike 对齐，不是宿主内存图）⇒ 本批不动（裁决 J ⇒ 登记为欠账） |
| 7 | **ROM 懒分配** | ROM 内容是镜像的一部分（输入）⇒ 必然在内存里（STAGE-11 同款理由） |
| 8 | **描述的运行时热加载 / 描述编辑界面** | 只提供**一次性加载**（文件路径 ⇒ 宿主启动时读一次） |
| 9 | **D-94 / D-87 / D-89 / D-114 / D-115 / M-21 / M-22 / M-23** | 与本批无交集 |
| 10 | **`MemoryProfile` 类型的保留** | 它是**公开 API**（`KernelHost::memory_profile()`，5 个调用点）⇒ 保留为**兼容视图**（裁决 C） |

### 0.3 为什么现在做（A 批是前置）

- v0.26 之前：宿主 `map_ram` 是 `vec![0u8; size]` ⇒ **声明 128 MiB = 真吃 128 MiB** ⇒ 「换一份内存图描述」在 A 批之前等于「换一份 OOM」⇒ **换不了**。
- v0.26 之后：登记成本 ≈ 0（只有写过的页占物理内存）⇒ 换描述是**纯数据操作** ⇒ B 批才成立。
- ⇒ **顺序不可交换**：A 是 B 的**成本前提**，B 是 A 的**兑现**。

### 0.4 本批**新登记**的欠账（4 条，收口时进台账）

| 临时编号 | 内容 | 为什么本批不做 |
| --- | --- | --- |
| **D-124**（候选） | 契约层**没有** MemoryMap 命令 / 事件 ⇒ 前端拿不到**真实**内存图（今天渲染的是 dev 假数据） | 要升契约版本 + 改前端 + 改 `front/lib/message.js` 镜像 ⇒ 独立一批 |
| **D-125**（候选） | **器件连线**（UART RX → PLIC 源 10）仍由宿主逻辑写死 ⇒ 「换平台」若改连线，**仍要改宿主代码** | 连线要变成描述 ⇒ 宿主 `sync_interrupts` 要改写成描述驱动 ⇒ 影响面比地址大得多 |
| **D-126**（候选） | 参考侧地址**三份副本**（`crates/kernel/src/bin/riscv-golden.rs:89/119` · `tools/golden-run.sh:56` · `tools/boot-osz.sh:45`）与宿主描述**手工对齐** | 参考侧要对齐 **Spike**，不是对齐宿主 ⇒ 抽公共常量要跨 crate / 跨语言（shell）⇒ 独立一批 |
| **D-127**（候选） | 契约层 `crates/contract/src/csr.rs:404` 的「三个空间」表格把 `irq_clear` 列在「internal **line**」一栏 ⇒ **与实现不符**（M-17 的 `clear_lines_by_mask` 判据是 `mask & (1 << line) != 0` ⇒ 值是**位掩码**）⇒ 两个值长得像（清 line 1 写 `2`、清 line 3 写 `8`）⇒ 极易踩错 | 改契约层注释要连带复核「三个空间」那张表的其余表述 ⇒ 与本批的契约层改动分开做更清楚 |

---

## §1 平台描述格式

### 1.1 为什么是 JSON（裁决 A）

- **全仓唯一的格式先例就是 JSON + serde_json**（`crates/kernel/Cargo.toml:20` 已直接依赖 `serde_json = "1.0"`；`crates/contract/Cargo.toml:13-18` 也有）⇒ **零新增依赖**。
- 引入 TOML / YAML 会是**全仓首个**该类依赖 ⇒ 要动 workspace 依赖表 + 锁文件 + WASM 目标 ⇒ 收益不抵成本。
- 前端是**原生 ES modules、零构建**（`AGENTS.md` NFR-05）⇒ 同一份 JSON 将来能被前端直接读。
- 代价：**JSON 不能写注释** ⇒ 补偿：可选的 `description` 字段承载人读说明（§1.3）。

### 1.2 顶层形状

```json
{
  "schema_version": 1,
  "id": "qemu-virt",
  "name": "QEMU virt",
  "priority": 20,
  "auto_selectable": true,
  "region_budget_bytes": 134217728,
  "regions": [ ... ]
}
```

### 1.3 字段表（**键集精确固定**）

| 字段 | 类型 | 必填 | 语义 |
| --- | --- | --- | --- |
| `schema_version` | 整数 | ✅ | 描述格式版本，**当前唯一合法值 = 1** |
| `id` | 字符串 | ✅ | 平台标识（`default` / `qemu-virt` / `minimal` / …）⇒ **平台的主键** |
| `name` | 字符串 | ✅ | 人读名称 |
| `priority` | 整数 | ✅ | **自动选档**时候选顺序，**小者优先**（裁决 E） |
| `auto_selectable` | 布尔 | ❌（**默认 `true`**） | 这个描述**是否参与自动选档**（裁决 E）⇒ 演示平台设 `false` ⇒ 只能被**显式**指定 |
| `region_budget_bytes` | 整数 | ✅ | **每区域**护栏预算（字节）⇒ 语义与 M-14 的 `max_region_bytes` 一致（§1.5 裁决 J'） |
| `regions` | 数组 | ✅ | 区域列表，**允许乱序**（校验时按 `base` 排序） |
| `description` | 字符串 | ❌ | 人读说明（JSON 不能写注释的补偿） |

### 1.4 区域条目（**内部 tag 判别**）

`regions` 的每一条用 **`kind` 作内部 tag**：

```json
{ "kind": "ram",       "base": 0,          "size": 1048576 }
{ "kind": "rom",       "base": 4096,       "size": 65536 }
{ "kind": "peripheral", "base": 1073741824,
  "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } }
```

| `kind` | 字段 | 说明 |
| --- | --- | --- |
| `"ram"` | `base` · `size` | 普通内存（懒分配） |
| `"rom"` | `base` · `size` | 只读内存（**写被静默丢弃**） |
| `"peripheral"` | `base` · `device` | 外设窗口；**`size` 不在描述里**（裁决 C） |

⚠️ **`kind` 的取值集合与 `RegionDto.kind` 完全一致**（`"ram"` / `"rom"` / `"peripheral"`）⇒ 描述与快照拓扑**同一套词汇** ⇒ 描述 → 宿主 → `export_map()` 的输出不需要任何翻译。

⚠️ **外设条目不带 `size`**：窗口大小是**器件的寄存器图**（`GPIO_WINDOW` / `UART_WINDOW` / `CLINT_WINDOW` / `PLIC_WINDOW` 都在 M-15 各器件模块里）⇒ 写进描述就是**第二个真相源** ⇒ 器件一改寄存器图，描述就静默过期 ⇒ **由器件自己报**（`Peripheral::size()`，契约层 `port.rs:191`）。

### 1.5 器件条目（`device` 内部 tag）

```json
{ "device": "gpio",      "id": "GPIO0" }
{ "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 }
{ "device": "clint",     "id": "CLINT", "freq_hz": 10000000, "harts": 1 }
{ "device": "plic",      "id": "PLIC",  "sources": 64, "contexts": 16 }
```

| `device` | 字段 | 对应今天宿主里的构造调用 |
| --- | --- | --- |
| `"gpio"` | `id` | `GpioBank::new(id, base)`（`kernel_host/mod.rs:855-858`） |
| `"uart16550"` | `id` · `irq_line` · `baud_rate` | `Uart16550::new(id, base, baud_rate, irq_line)`（`:870-875`） |
| `"clint"` | `id` · `freq_hz` · `harts` | `Clint::with_frequency(id, base, harts, freq_hz)`（`:879-884`） |
| `"plic"` | `id` · `sources` · `contexts` | `Plic::with_capacity(id, base, sources, contexts)`（`:888-893`） |

⚠️ **参数逐字对应今天**（`irq_line = 1` = `IRQ_UART_RX_READY` · `baud_rate = 115_200` = `UART0_BAUD_RATE` · `freq_hz = 10_000_000` = `QEMU_VIRT_FREQ_HZ` · `harts = 1` · `sources = 64` · `contexts = 16`）。

### 1.6 校验规则（**一场景一码**）

| # | 规则 | 场景码 | 备注 |
| --- | --- | --- | --- |
| V-1 | `schema_version != 1` | `E_PLT-01` | |
| V-2 | `id` 为空 | `E_PLT-02` | 平台主键不能空 |
| V-3 | `regions` 为空 | `E_PLT-03` | 一个区域都没有 = 没有内存图 |
| V-4 | RAM / ROM 的 `size == 0` | `E_PLT-04` | 与 M-14 的 `E_BUS-06` 同义但**不同码**（不同层的约束） |
| V-5 | `base + size` 溢出 u32 | `E_PLT-05` | 与 M-14 的 `E_BUS-04` 同义但**不同码** |
| V-6 | 两个区域**重叠** | `E_PLT-06` | 按 `base` 排序后逐条比；⚠️ **外设条目的口径见下方** |
| V-7 | RAM / ROM 的**声明大小** > `region_budget_bytes` | `E_PLT-07` | ⚠️ 见下方**重要说明** |
| V-8 | 未知器件种类 / 缺参数 / 参数类型错 | serde 错误 | 反序列化阶段就失败 |
| V-9 | 外设器件的 `id` 与另一条重复 | `E_PLT-08` | 同名器件 ⇒ 恢复时按 id 反查会歧义 |
| V-10 | **宿主侧**：请求的**平台 id 不存在** | `E_PLT-09` | 契约层拥有（施工期补） |
| V-11 | **宿主侧**：描述**文件读不了** | `E_PLT-10` | 契约层拥有（施工期补） |
| V-12 | **宿主侧**：描述**文件不是合法 JSON / 形态非法** | `E_PLT-11` | 契约层拥有（施工期补） |

⚠️ **V-7 为什么必须存在（一个真发现）**：v0.26 把 RAM 的护栏从「**声明大小**」改成了「**物理占用**」（`RamBackend` 懒分配 ⇒ `map_ram` 时物化页数 = 0）⇒ **「声明一段超大 RAM」在 M-14 的运行时护栏里已经拦不住了** ⇒ 描述层是**唯一**还能拦「声明期超大」的地方 ⇒ 它必须拦。
⚠️ 但 V-7（**声明期**）与 M-14 的 `E_BUS_09`（**运行期**，按物理占用）是**两个不同语义、不同阶段**的约束 ⇒ **不同码**（不合并）⇒ 且 V-7 的码**不叫** `E_BUS_09`（避免「一码多义」）。

⚠️ **V-7 与今天的行为逐字一致**：Default 档预算 16 MiB（RAM 1 MiB + 4 MiB ✓）；QemuVirt 档预算 128 MiB（RAM 128 MiB **恰好相等** ⇒ 允许）⇒ 判据是「**超过**才拒」（`>` 不是 `>=`），与 M-14 一致。

⚠️ **V-6 对「外设条目」的口径（施工期补，原文是空白）**：描述里的外设条目**没有窗口大小**（裁决 D）⇒ 做不出**完整区间比较** ⇒ 采用**保守口径**：
- **外设的基址落在某个已声明的 RAM / ROM 区间内** ⇒ 算**重叠**；
- **两个外设条目同基址** ⇒ 算**重叠**。
⇒ 这与 M-14 建图时报 `E_BUS-03`（地址已被占用）的行为**一致** ⇒ 描述层提前拒掉，不让冲突走到建图那一步。
⇒ ⚠️ **为什么不是「按区间比」**：那要求描述里有窗口大小 ⇒ 违反裁决 D（窗口大小是器件的寄存器图，写进描述就是第二个真相源）⇒ 宁可**保守拒**，不可**放过冲突**。

---

## §2 落点（谁改什么）

### 2.1 契约层（Lane A）

**新文件** `crates/contract/src/platform.rs`：

```rust
/// 平台描述的格式版本（描述**不是**会话载荷 ⇒ 不与 `SESSION_SNAPSHOT_SCHEMA_VERSION` 联动）。
pub const PLATFORM_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformDescription {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub priority: u32,
    pub region_budget_bytes: u32,
    pub regions: Vec<RegionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegionSpec {
    Ram { base: u32, size: u32 },
    Rom { base: u32, size: u32 },
    Peripheral { base: u32, device: DeviceSpec },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "device", rename_all = "snake_case")]
pub enum DeviceSpec {
    Gpio { id: String },
    Uart16550 { id: String, irq_line: u16, baud_rate: u32 },
    Clint { id: String, freq_hz: u64, harts: u32 },
    Plic { id: String, sources: u32, contexts: u32 },
}
```

⚠️ **内部 tag 的名字是 `device` 而不是 `kind`**（`kind` 已被区域层占用）⇒ 线上键名是 `{"device": "uart16550", ...}` ⇒ **与 §1.5 的示例逐字一致**。

**校验器**：`PlatformDescription::validate(&self) -> Result<(), PlatformError>`，**V-1 ~ V-9 全部落在这里** ⇒ **一个约束一个落点**（宿主与前端都调它，不各写一份）。
⚠️ **V-10 ~ V-12 不落在 `validate` 里**（它们是**宿主侧**的三类失败：平台 id 不存在 / 描述文件读不了 / 描述文件形态非法 ⇒ 宿主在做**装载**时才知道）⇒ 但**它们的场景码仍归契约层所有**（`crates/contract/src/error.rs`）⇒ 宿主只**引用**、**不自建** ⇒ **约束的落点唯一**。

⚠️ **错误码族**：新增第 **15 个**错误族 `ErrorFamily::Platform`（tag `"PLT"`，**3 字符** ⇒ 符合契约层「tag 是 3–5 字符」的约束）+ 场景码 `1..11`（线上 `E_PLT-01` .. `E_PLT-11`）⇒ ⚠️ **十一条规则（V-1 ~ V-12，其中 V-8 落在 serde 层）对应十一个码**。

⚠️ **新增一族要改两处**（契约层的 `error_families!` 宏是「族」的真相源，但**族标签表 `TAGS` 是另一处**）：
- `crates/contract/src/error.rs:119` 的 `error_families! { … }` 里加**一行** `Platform => "PLT",`；
- 同文件的**族标签表** `TAGS` 里加**一行**（`error.rs:412-429` 的完整性检查会逐族比对 `as_str()` 与表里的 tag ⇒ **漏一处就红**）。
⇒ ⚠️ 这两处是**同一个约束的两个落点**（枚举 + 查表），不是两个约束 ⇒ 契约层已用一条**完整性用例**兜住（**`every_family_tag_is_pinned`**，在 `crates/contract/src/error.rs` 的测试模块里 ⇒ 它逐族比对 `as_str()` 与 `TAGS`、并断言 `TAGS.len() == ALL_FAMILIES.len()`）⇒ 本批**不新增**这条用例，靠它兜住。

⚠️ **前端零改动**（**实测**）：前端**不镜像错误族** —— `front/lib/message.js:468` 的 `Event.error(code, message)` 把错误码当**自由字符串**透传，前端测试里出现的 `E_BUS-01` / `E_CTX-05` 也只是**字面量**（`front/tests/mock-kernel.test.mjs:189`）⇒ 新增一族**不需要**动前端 ⇒ **前端门禁 312 用例必须仍然全绿，且前端源码一行不改**。

⚠️ **契约层「不升版本」与「新增错误族」不矛盾**：契约版本（`schema_version`）管的是**消息形状**；错误族是**错误码词汇表**⇒ 两者是**两条不同的约束** ⇒ 前者不动、后者新增 ⇒ **前端镜像不受影响**（前端只镜像命令/事件的形状）。

### 2.2 宿主（Lane B）

| 改动 | 文件 | 说明 |
| --- | --- | --- |
| **新增** 平台子模块 | `crates/kernel/src/kernel_host/platform.rs` | 描述的**加载**（内置 / 文件路径）、**器件工厂**、**建图** |
| **新增** 内置描述 | `crates/kernel/src/kernel_host/platforms/{default,qemu-virt,minimal}.json` | `include_str!` 编译进二进制 |
| **改** `map_memory` | `crates/kernel/src/kernel_host/mod.rs:811-906` | 从「`match profile` 硬编码」改成「读描述 → 逐条建区域 → 实例化器件」 |
| **改** `choose_memory_profile` | `mod.rs:918-936` | 判据描述化（§4 裁决 E） |
| **改** `new_with_profile` | `mod.rs:783-805` | 保留为**兼容入口**（裁决 C） |
| **新增** `new_with_platform` | `mod.rs` | 显式按 `id` 或按文件路径建图 |

⚠️ **`map_memory` 改造后的形状**（一个约束一个落点）：

```
描述 → 校验（契约层） → 逐条区域：
    ram        → bus.map_ram(base, size, &[])
    rom        → bus.map_rom(base, size, &[])
    peripheral → 工厂造实例 → bus.map_peripheral_instance(base, 实例.size(), 实例)
护栏预算 → bus.set_max_region_bytes(描述.region_budget_bytes)
```

⚠️ **`map_memory` 里不再出现任何地址常量、任何器件构造调用、任何 `match profile`** ⇒ 「换描述、宿主零改动」在**源码层面**成立（判别式 P-6：源码里 `map_memory` 不得出现 `0x` 字面量）。

### 2.3 M-14（**零改动**）

- 描述的**消费者只有宿主** ⇒ M-14 不需要知道描述的存在。
- M-14 的 `RegionType` / `RegionDto.kind` / `load_map` 的 kind 分流**一律不动** ⇒ 描述的 `kind` 词汇表**恰好**是 M-14 已有的三个值 ⇒ 不新增分类分支。
- ⚠️ **例外**：M-14 的 `REGION_KIND_RAM` / `_ROM` / `_PERIPHERAL` 三个常量**定义在 M-14**（`m14_bus/mod.rs:99/102/105`）而不是契约层 ⇒ 描述层要用同样的三个字符串 ⇒ **要么**契约层引用 M-14（反向依赖，不行）**要么** M-14 引用契约层（正确）⇒ ⇒ **本批把三个常量上移到契约层**，M-14 改为 `pub use` ⇒ **一个约束一个落点**（同一批里收拢，不留到下一批）。
  - ⚠️ 这会**动 M-14 的源码**（与「零改动」冲突）⇒ 裁决：**「零改动」指的是 M-14 的**行为**不变**（区域模型、派发路径、护栏语义、快照导出全部一字不变）；只允许**常量归属**上移（M-14 内部 `pub use` 别名，调用点字面量不变 ⇒ **既有断言一条不改**）。

### 2.4 器件工厂（新增，Lane B）

```rust
/// 描述 → 器件实例。**一个 match、一个落点**。
///
/// ⚠️ **返回「两种视图」**（施工期订正）：总线侧要的是**类型擦除**的
/// `Rc<RefCell<dyn Peripheral>>`，而宿主侧要的是**有类型**的句柄
///（`self.uart0: Option<Rc<RefCell<Uart16550>>>`）⇒ ⚠️ `RefCell<T>` 是
/// **不变**的 ⇒ `RefCell<dyn Peripheral>` **降不回** `RefCell<Uart16550>`
/// ⇒ 一个返回值**不可能**同时满足两边 ⇒ ⇒ 工厂返回**两样东西**：
/// ① 擦除视图（挂总线）② 有类型句柄（宿主用）。
/// ⇒ 器件种类的 `match` 仍然**只有一处** ✓
fn build_device(spec: &DeviceSpec, base: u32) -> Result<BuiltDevice, PlatformError>
```

⚠️ **契约先行**：工厂返回**两种视图**（① **擦除视图** `Rc<RefCell<dyn Peripheral>>` —— 契约层 `port.rs` 的 trait 对象，挂总线用；② **有类型句柄** —— 宿主自己用，如 `Rc<RefCell<Uart16550>>`）⇒ 工厂**不认识** M-15 的具体类型，除了**构造**那一刻。
⚠️ **为什么必须是两种视图**：`RefCell<T>` 是**不变**的 ⇒ `RefCell<dyn Peripheral>` **降不回** `RefCell<Uart16550>` ⇒ 一个返回值**不可能**同时满足总线侧与宿主侧 ⇒ ⇒ 拆成两样东西 ⇒ 而**器件种类的 `match` 仍然只有一处** ✓（约束的落点没增加）。
⚠️ **新增一种器件种类 ⇒ 必须改这一个 match** ⇒ 这是**不可消除**的边界 ⇒ 规格明说：**平台（内存图）零代码，器件种类要代码**。

### 2.5 「一个约束一个落点」对照表（裁决 H）

| 约束 | 唯一落点 | 本批**不得**新增的第二处 |
| --- | --- | --- |
| 描述的**校验** | 契约层 `PlatformDescription::validate` | 宿主不得自写一份校验 |
| 描述的**格式版本** | 契约层 `PLATFORM_SCHEMA_VERSION` | 不得与 `SESSION_SNAPSHOT_SCHEMA_VERSION` 联动 |
| 区域 kind 的**词汇表** | 契约层（由 M-14 `pub use`） | M-14 不得留一份字面量 |
| 器件种类 → 构造 | 宿主 `platform.rs` 的 `build_device` | 不得在 `map_memory` 里再写一个 match |
| 外设**窗口大小** | 器件自己的 `Peripheral::size()` | 描述里**不得**出现窗口大小 |
| 每区域**预算** | 描述的 `region_budget_bytes` → `bus.set_max_region_bytes` | 宿主不得按 `id` 硬编码预算 |

---

## §3 裁决（施工期不可自行更改）

### 裁决 A · 描述格式是 JSON

见 §1.1。**代价**：不能写注释 ⇒ 补偿是可选的 `description` 字段。

### 裁决 B · 描述 DTO 进**契约层**新文件 `platform.rs`

- **为什么不是内核内部**：`PlatformDescription` 是**跨 crate 的数据形状**（kernel 读；将来的 server crate 读；前端将来镜像）⇒ 与 `SessionSnapshotDto` 同性质 ⇒ 归契约层。
- **为什么校验也在契约层**：校验规则**只能有一份**（前端将来也要校验用户手写的描述）⇒ §2.5 对照表第一行。

### 裁决 C · `MemoryProfile` **保留**为兼容视图（不删公开 API）

- `KernelHost::memory_profile()` 是**公开 API**，**5 个调用点**（1 生产 `crates/kernel/src/bin/osz-boot.rs:230` + 4 测试）。
- 形态：`MemoryProfile` 变成 **`Default` / `QemuVirt` 两个变体的兼容枚举**，`memory_profile()` **按当前生效描述的 `id`** 返回 ⇒ `default` ⇒ `Default`、`qemu-virt` ⇒ `QemuVirt`、**其它 id ⇒ `Default`**（⚠️ 新平台的 `memory_profile()` 会返回 `Default` ⇒ 诚实但信息量低 ⇒ 新增 `platform_id()` 返回**真实 id 字符串** ⇒ 既有 4 条测试断言**一字不改**）。
- ⚠️ `new_with_profile(profile)` 保留 ⇒ 按 `profile` 选内置描述 ⇒ **13 个调用点一字不改**。

### 裁决 D · 描述里**不写**外设窗口大小

见 §1.4 ⇒ 由 `Peripheral::size()` 决定 ⇒ 没有第二个真相源。

### 裁决 E · 自动选档判据（逐字等价于今天）

**今天**（`mod.rs:918-936`）：镜像的每个地址区间 `(lo, hi)` **逐个过**；能被 Default 的**高段**窗口完整覆盖 ⇒ 继续下一个区间；**只**被 QemuVirt 的**高段**窗口完整覆盖 ⇒ **立即**选 QemuVirt；两档都覆盖不了 ⇒ 跳过；全部区间过完 ⇒ Default。

⚠️ **今天有两条隐含结构，必须原样搬过来**：① **区间在外层**（不是「描述在外层、找第一个能装下的」）② 判据只看**高段**窗口。

**描述化后**（描述多了一个 `auto_selectable` 字段，见 §1.3）：

1. 候选描述 = 全部 `auto_selectable == true` 的内置描述，按 `priority` **小者优先**（`default` = 10 · `qemu-virt` = 20）；
2. **区间在外层**逐个过：对每个区间 `(lo, hi)`，找出**能完整覆盖它**的候选描述（`lo >= base && hi <= base + size`，在它的**全部 RAM 区域**里找）里 `priority` **最小**的那个；
   - **找不到** ⇒ 跳过这个区间（= 今天的 `continue`）；
   - 找到的是 **`default`** ⇒ 继续下一个区间（= 今天「被 Default 覆盖 ⇒ 不 return」）；
   - 找到的是**别的** ⇒ **立即**选它（= 今天的 `return QemuVirt`）；
3. 全部区间过完 ⇒ 回落 `default`。

⚠️ **等价性论证**（必须钉住，B-9）：
- 镜像落在 Default 高段 ⇒ 只有 `default` 覆盖 ⇒ 选 `default` ✓（今天同）
- 镜像落在 QemuVirt 高段（`0x8040_0000` 以上）⇒ `default` **不**覆盖、`qemu-virt` 覆盖 ⇒ 选 `qemu-virt` ✓（今天同）
- 镜像落在两档之外（如 `0x9000_0000`）⇒ 无候选 ⇒ 回落 `default` ✓（今天同 ⇒ 随后由 M-14 报未映射）
- 镜像落在**低段**（`0x0..0x1000`）⇒ `default` 的**低段** RAM 覆盖它 ⇒ 选 `default` ✓（今天：`inside(default_high)` 为假 ⇒ `continue` ⇒ Default ⇒ 同）
- 镜像落在**低段但超出 1 MiB**（如 `0x0..0x1000_01`）⇒ `default` 与 `qemu-virt` 的低段**都是 1 MiB** ⇒ 都覆盖不了 ⇒ 无候选 ⇒ 回落 `default` ⇒ 随后报未映射 ✓（**与今天逐字一致**：今天它落在两档**高段**之外 ⇒ `continue` ⇒ Default ⇒ 同样报未映射）

⚠️ **`auto_selectable` 存在的唯一理由（这是本裁决的关键）**：`minimal` 的低段是 **2 MiB**（比 `default` 的 1 MiB 大）⇒ 若它参与自动选档，就会覆盖上面那条「超出 1 MiB」的镜像 ⇒ 选到 `minimal` ⇒ **今天绝不会发生** ⇒ ⇒ `minimal` 必须 `auto_selectable: false` ⇒ 它**只被显式指定**。
⇒ ⚠️ **这就是把「今天只查高段」这个差异显式承载下来的地方** —— 靠**数据里的一个布尔**，而不是靠代码里的一份 id 名单（名单是「代码知道哪个平台是演示平台」⇒ 换个人写描述就会破）。

⚠️ **两份真实平台描述都是 `auto_selectable: true`** ⇒ 它们之间**没有**低段差异（低段都是 1 MiB）⇒ 唯一有低段差异的 `minimal` 被排除 ⇒ ⇒ 整体**逐字等价** ✓

### 裁决 F · 内置描述 `include_str!` 编译进二进制 + 运行时文件路径入口

- **为什么必须编译进去**：WASM 产物是**单文件**（wasm-pack，`tools/build-wasm.sh`）⇒ 浏览器里**没有文件系统** ⇒ 运行时读文件不可能 ⇒ 内置描述**必须**在编译期进产物。
- **两条路径共用同一个解析器 + 同一个校验器**（§2.5 第一行）⇒ 没有两份语义。
- ⚠️ **「零改动」的诚实口径**：
  - **运行时路径**（`new_with_platform_file(path)`）⇒ 换平台**一字不改**；
  - **内置**描述 ⇒ 要加**一行** `include_str!` 登记（**数据登记**，不是逻辑改动）⇒ 规格不把它叫「零改动」，叫「**零逻辑改动**」。
- ⚠️ 描述文件放**内核 crate 内**（`crates/kernel/src/kernel_host/platforms/`）⇒ 不进仓库根的「仓外产物」禁区（那是外部工具产物，不是平台配置）。

### 裁决 G · **连线**不进描述（明确裁剪 ⇒ 登记 D-125）

「UART 的 RX 就绪 → PLIC 的源 10」（`kernel_host/mod.rs:976-990`）是**器件之间的信号连线**，不是地址空间。
⇒ 描述覆盖「**有什么、在哪、参数是多少**」⇒ **不覆盖「谁连谁」**。
⇒ **诚实的后果**：换平台若要改连线，**仍要改宿主代码** ⇒ 「零逻辑改动」的边界止于地址与器件参数。
⇒ 登记为 **D-125**。

### 裁决 H · D-82：**不做** `RegionType::Clint`、**不改**契约层 kind 值域；改为**补判别力 + 订正台账**

见 §4（下一节）。

### 裁决 I · 契约层**不升版本**、**不加** MemoryMap 命令 / 事件

⇒ 前端**零改动**（`front/lib/message.js` 加一个错误族常量除外，见 §2.1）⇒ 前端门禁 312 用例必须仍全绿。

### 裁决 J · 参考侧的地址副本**不动**（登记 D-126）

`crates/kernel/src/bin/riscv-golden.rs:89/119` · `tools/golden-run.sh:56` · `tools/boot-osz.sh:45` 是**参考侧对拍**的职责（要对齐 **Spike**）⇒ 不属「宿主内存图」⇒ 本批不动 ⇒ 登记 **D-126**。

---

## §4 D-82 的定性（**订正台账措辞**）

### 4.1 台账原文（D-82，`REVIEWS.md:302`）

> 「CLINT 的 `mtime` / `mtimecmp` **固件侧 MMIO 通路**推迟到 P1：LLD §3.4 要求固件经 **M-14 MMIO 区间**访问 `mtime` / `mtimecmp`（CLINT 语义），本批**不做** —— 落它要**同时**动 M-14 的区域类型（新增 `RegionType::Clint` + 一条派发路径）与**契约层的区域 kind**（`M14MapDto.region.kind` 是 `String`，但 `SessionSnapshotDto::validate()` 按 `"ram"` / `"rom"` / `"peripheral"` 分类做检查）⇒ 契约层刚升 v2 不到一批，再动它会把本批从「落地一个模块」拉成「改会话载荷的形状」」

### 4.2 实测结论（**台账原文与今天的代码不符**）

| 台账原文的判断 | 实测（2026-10-24） | 结论 |
| --- | --- | --- |
| 「要新增 `RegionType::Clint` + 一条派发路径」 | CLINT **早已挂在总线上**（`kernel_host/mod.rs:885`），M-14 的 `PeripheralBound` 派发路径**已经是通用的**（`m14_bus/mod.rs:2388-2410`），且传入的 `offset` **已是 region 相对偏移**（`mod.rs:2143`） | ❌ **不必要** ⇒ 新增专用路径 = **第二条派发路径** ⇒ 违反「一个约束一个落点」 |
| 「契约层 `validate()` 按 ram/rom/peripheral 分类做检查」 | 契约层**只做一次 `kind == "ram"` 的字面量比较**（`session.rs:808`）；**分类逻辑实际住在 M-14 的 `load_map`**（`m14_bus/mod.rs:1328-1381`） | ⚠️ **台账措辞与代码不符** ⇒ 要订正 |
| 「固件侧 MMIO 通路没做」 | **已通**：guest 写 `mtimecmp` → `Clint::write` → 批次边界 `sync_interrupts`（`kernel_host/mod.rs:1900` / `:1936`）→ `M-16::set_timer_compare` → `poll()` 挂起定时器线 3 | ✅ **已通** |
| 「今天没有活影响 / 固件侧没有消费者」 | **osz 实测有消费者**：`hal_clint_timer_get_time_stamp` 反汇编出 `lui a5,0x200c; lw a6,-8(a5)` ⇒ 地址 `0x200bff8` / `0x200bffc` ⇒ **正是**我们 CLINT 模型的 `mtime` 偏移（`0xBFF8` / `0xBFFC`，`clint.rs:88`） | ⚠️ **台账这条已过时** |

### 4.3 D-82 的真实剩余缺口

**不是**「通路不存在」，而是「**通路没有 guest 指令级的端到端判别力**」：
- 现有的 `the_clint_mtimecmp_reaches_the_interrupt_controller`（`tests_osz_wiring.rs:264`）是**宿主侧**写总线（`host.bus_mut().write(...)`）⇒ 它证明「写总线 → CLINT 存下 → 同步 → M-16 拿到比较值」⇒ **不覆盖**「guest 指令写 → 定时器线挂起 → 交付 → 陷入」。
- M-16 侧有单测（`m16_interrupt/tests.rs:348` 的电平语义）⇒ 两端各测一半 ⇒ **中间那一段没有一条用例**。

### 4.4 本批怎么做（裁决 H）

**补一条 guest 指令级的端到端用例**（固件机器码**用仓外交叉工具链汇编产出 + `objdump` 核对**，先例见 `crates/kernel/tests/intc_end_to_end.rs:65-73`；工具链在 `~/riscv-sim-golden/toolchain/bin/riscv-none-elf-{as,objdump}`，**仓外**，不进仓库）：

```asm
        lui   x5, 0x84000        # x5 = 0x8400_0000（固件基址，QEMU virt 高段 RAM）
        addi  x5, x5, 0x28      # x5 = handler 的地址（0x28 == `handler` 的偏移，见下）
        lui   x6, 0x2004        # x6 = 0x0200_4000 = CLINT 的 mtimecmp（⚠️ **立即数是 0x2004**，不是 0x20040 —— 见下方字节表的「真缺陷」说明）
        li    x7, 1             # x7 = 1 周期
        sw    x7, 0(x6)         # 写 mtimecmp 低 32 位（高半保持 0 ⇒ 值 = 1 周期）
        li    x8, 0x80          # x8 = 1 << 7 = **MTIE 位号 7**
        csrrw x0, mie, x8       # mie 使能定时器线（line 3）
        csrrsi x0, mstatus, 8   # mstatus.MIE（bit 3）= 1
        csrrw x0, mtvec, x5     # mtvec = handler（直接模式）
loop:   jal   x0, loop         # 自旋，等中断
handler:
        csrr  x9,  mcause       # x9 = guest 看到的 mcause
        csrr  x10, mepc         # x10 = guest 看到的 mepc
        csrr  x11, mtval        # x11 = guest 看到的 mtval
        addi  x12, x12, 1       # 计数：handler 跑了几遍
        csrrwi x0, 0x7c0, 8 # 清 line 3（**位掩码 bit 3 = 1 << 3 = 8**；⚠️ **CSR 号是 0x7c0**，不是 0x7c1 —— 见下方字节表的「真缺陷」说明）
        mret
```

⚠️ **汇编命令与核对方式**（工具链在**仓外** `~/riscv-sim-golden/toolchain/bin`，不进仓库）：

```bash
riscv-none-elf-as -march=rv32im_zicsr -mabi=ilp32 d82.s -o d82.o
riscv-none-elf-objdump -d d82.o          # 逐条核对
riscv-none-elf-objcopy -O binary --only-section=.text d82.o d82.bin
```

⚠️ ⚠️ **`-march` 必须带 `zicsr`**（否则 `csrrw` / `csrr` / `csrrsi` / `csrrwi` 全部报 `unrecognized opcode ... extension 'zicsr' required`）⇒ 用 `rv32im`（**不带 `c`**）⇒ 每条指令 4 字节 ⇒ 偏移好算。
⚠️ ⚠️ **不要用 `-march=rv32imc`**：压缩指令会被汇编器自动生成（`li` → `c.li`、`jal` → `c.j`）⇒ **指令长度变成 2 字节** ⇒ `handler` 的偏移不再是 4 的倍数、也不再是 0x28 ⇒ **偏移常量与固件布局耦合** ⇒ 改一条指令就要重算。
⚠️ ⚠️ **不要用 `.option pic`**：那会让 `la` 展开成 `auipc` + **`lw`**（GOT 重定位）⇒ 未链接时是 `auipc t0,0x0` + `lw t0,0(t0)` ⇒ **从地址 0 读数据** ⇒ 完全错误。

**已核对的字节**（`objdump` 逐条对过，16 条 × 4 字节 = **64 字节**，小端）：

| 偏移 | 指令 | 机器码 |
| --- | --- | --- |
| 0x00 | `lui x5, 0x84000` | `0x840002b7` |
| 0x04 | `addi x5, x5, 0x28` | `0x02828293` |
| 0x08 | `lui x6, 0x2004` | `0x02004337` |
| 0x0c | `li x7, 1` | `0x00100393` |
| 0x10 | `sw x7, 0(x6)` | `0x00732023` |
| 0x14 | `li x8, 0x80` | `0x08000413` |
| 0x18 | `csrrw x0, mie, x8` | `0x30441073` |
| 0x1c | `csrrsi x0, mstatus, 8` | `0x30046073` |
| 0x20 | `csrrw x0, mtvec, x5` | `0x30529073` |
| **0x24** | `jal x0, 0x24`（**`loop`**） | `0x0000006f` |
| **0x28** | `csrr x9, mcause`（**`handler` 入口**） | `0x342024f3` |
| 0x2c | `csrr x10, mepc` | `0x34102573` |
| 0x30 | `csrr x11, mtval` | `0x343025f3` |
| 0x34 | `addi x12, x12, 1` | `0x00160613` |
| 0x38 | `csrrwi x0, 0x7c0, 8` | `0x7c045073` |
| 0x3c | `mret` | `0x30200073` |

⚠️ ⚠️ **`0x38` 那条的 CSR 号是 `0x7c0`（不是 `0x7c1`）** —— 施工期第五次订正修掉的**真缺陷**：原字节表写的是 `csrrwi x0, 0x7c1, 8` / `0x7c145073` ⇒ **CSR 号错了一位** ⇒ 后果是**假绿**：
- 契约层的 `irq_clear` 是 **`CSR_IRQ_CLEAR = 0x7C0`**（`crates/contract/src/csr.rs:171`）；
- `0x7C1` **不在** `csr_is_implemented` 的名单里（`csr.rs` 的 `matches!` 只列了 `CSR_IRQ_CLEAR` = `0x7C0`）⇒ 按 `csr_write_is_ignored`（`csr.rs:565`）⇒ **guest 写 `0x7C1` 被静默忽略**；
- ⇒ **定时器线永不清** ⇒ handler 里的 `mret` 落回 `mepc` 后**立刻再次陷入** ⇒ 判别式 4「handler 计数器 x12 ≥ 1」**照样能过** ⇒ **用例看起来绿，其实根本没测到「清线」这条通路** ⇒ 这正是「假绿」的定义；
- ⇒ 修法：CSR 号改回 **`0x7c0`** ⇒ `csrrwi x0, 0x7c0, 8` ⇒ 机器码 **`0x7c045073`**（`csr` = `0x7c0` · `funct3` = 5（`csrrwi`）· `zimm` = 8）⇒ **已用仓外交叉工具链重新汇编 + `objdump` 逐条核对 + `objcopy` 逐字节核对**（`riscv-none-elf-as -march=rv32im_zicsr -mabi=ilp32` ⇒ `objdump -d` ⇒ `objcopy -O binary --only-section=.text` ⇒ 仍是 16 条 × 4 字节 = 64 字节）。
- ⚠️ **顺带一提**：`csrrwi` 的 `zimm` 只有 **5 位** ⇒ 位掩码 `8`（`1 << 3`）放得进，但 `1 << 7` = 128 就**放不进** ⇒ 清**线 7** 用 `csrrwi` 是**做不到**的 ⇒ 用例注释里要写明这条限制（v0.24 那条 `csrrwi x0, irq_clear, 2` 同理）。

⚠️ ⚠️ **`0x08` 那条的立即数是 `0x2004`（不是 `0x20040`）** —— 施工期第七次订正修掉的**真缺陷**（**三个缺陷里最致命的一个**）：原字节表写 `lui x6, 0x20040` / `0x20040337` ⇒ **`x6` 拿到的是 `0x2004_0000`，不是 `0x0200_4000`** ⇒ 后果是**固件根本到不了定时器**：
- **`lui` 的立即数是 20 位、并且要左移 12 位**（`x = imm[19:0] << 12`）⇒ 要得到 `0x0200_4000` 需要的立即数是 **`0x0200_4000 >> 12 = 0x02004`** ⇒ 写成 `0x20040` 是**多写了一个 0**（把「地址的高位」当成立即数）⇒ `0x20040 << 12 = 0x2004_0000`；
- `0x2004_0000` **不在 `qemu-virt` 描述的任何区域里**（`ram@0/1 MiB` · `ram@0x8000_0000/128 MiB` · `clint@0x0200_0000` · `plic@0x0C00_0000` · `uart@0x1000_0000` · `gpio@0x4000_0000`）⇒ guest 的 `sw x7, 0(x6)` 落到**未映射地址** ⇒ 触发**架构陷阱 cause 7（store access fault）**（D-99 的判别力）⇒ **guest 在第 5 条指令就死了**，压根跑不到「写 `mtimecmp` → 使能 `mie` → 陷入」那条路；
- ⚠️ **而且那时 `mtvec` 还没设**（`mtvec` 在第 9 条指令才写，而 store 在第 5 条）⇒ 陷阱跳到 `mtvec = 0` ⇒ 落进低段 RAM 的全零区 ⇒ 执行非法指令 ⇒ **再次陷入** ⇒ **无限陷阱循环** ⇒ ⇒ 后果是「陷入确实发生了，但 `mcause` 是 store fault 的 `7`、不是定时器中断的 `0x8000_0007`」⇒ **判别式 1 会红**（这正是它该红的理由）。
- ⇒ 修法：立即数改成 **`0x2004`** ⇒ 机器码 **`0x02004337`**（小端字节 `37 43 00 02`）⇒ **已用仓外交叉工具链重新汇编 + `objdump` 逐条核对 + `objcopy` 逐字节核对**（`objdump` 的反汇编行也自证：`sw t2,0(t1) # 2004000` ⇒ 目标地址 `0x0200_4000`）⇒ 仍是 16 条 × 4 字节 = 64 字节。
- ⚠️ **教训（别再犯）**：`lui` 的立即数**不是**地址的高 20 位，而是「地址右移 12 位」⇒ 写 `lui` 之前先算 `addr >> 12` ⇒ **不许拿「看着像地址的一段」直接填**。

⚠️ **`handler` 在偏移 `0x28`** ⇒ `addi x5, x5, 0x28` 里的常量**与固件布局耦合**（`loop` 之前正好 10 条指令 × 4 字节）⇒ 改固件条数**必须**同步改这个常量 ⇒ 用例注释里要写明这一点。

⚠️ **`loop` 的绝对地址 = `0x8400_0024`**、`handler` 的绝对地址 = `0x8400_0028`（固件基址 `0x8400_0000` + 偏移）⇒ 这两个值**由固件布局决定**⇒ 断言时写**字面量**（不许写成 `FW_BASE + 0x24` 那种从常量反推的形式 —— 那跟实现同源，改实现时会跟着变，变成恒绿）。

⚠️ **`irq_clear` 的值是「位掩码」，不是「线号」**（M-17 `clear_lines_by_mask`，`m17_backend/mod.rs:349-354`：判据是 `mask & (1 << line) != 0`）⇒ 定时器线 3 ⇒ 写 **`8`**（`1 << 3`），**不是 3**。
⚠️ 对照：v0.24 那条 UART 用例写的是 `2`（清 line 1 ⇒ `1 << 1 = 2`）⇒ 两个值**长得像**但含义不同 ⇒ **这是最容易踩的坑**，用例注释里要写明。
⚠️ ⚠️ **顺带发现（已登记 D-127）**：契约层 `crates/contract/src/csr.rs:404` 的「三个空间」表格把 `irq_clear` 列在「internal **line**」一栏 ⇒ **与实现不符**（实现是位掩码）⇒ 该表述要订正。

⚠️ **三个架构字面量（不许从实现反推）**：
- `mcause == 0x8000_0007`（`arch_mcause_for_line(IRQ_TIMER_COMPARE)`，契约层 `csr.rs:843-846`：**IRQ_BASE 0x8000_0000 | 架构 cause 7 = 机器定时器中断**）；
- `mie` 的值 = `1 << 7` = **0x80**（`std_irq_bit_for_line(3) = 7`，契约层 `csr.rs:346-352`）；
- ⚠️ **内部线号 3**（`IRQ_TIMER_COMPARE`）与**架构 cause 码 7** 是**两个不同的空间**，断言时只能用 **7**，写 3 就是错的。

**判别式**（每条都要能变红）：
1. guest 陷入的 `mcause == 0x8000_0007`；
2. `mepc == 0x8400_0024`（**`loop` 指令的绝对地址**，架构无关但**由固件布局决定** ⇒ 写**字面量**，不许写成 `FW_BASE + 0x24`）；
3. `mtval == 0`（定时器中断没有错因值 ⇒ M-17 调 `enter_trap(state, cause, 0)`，`m17_backend/mod.rs:464`）；
4. handler 的计数器（x12）≥ 1 ⇒ 真的进了 handler，不是停在别处；
5. ⚠️ **变异可红**：把固件的 `sw x7, 0(x6)` 去掉 ⇒ **必须红**（陷入不发生）⇒ 证明这条用例**真的在测那条通路**，不是恒绿；
5b. ⚠️ **变异可红（施工期第五次订正新增）**：把固件的 `li x7, 1` 改成 `li x7, 0`（`mtimecmp = 0`）⇒ **必须红**（陷入不发生）⇒ 理由：`mtimecmp == 0` 在 CLINT 侧是「无期限」、在 M-16 侧是「**定时器关闭**」（`poll()` 的判据是 `mtimecmp_ps != 0 && mtime_ps >= mtimecmp_ps`，`m16_interrupt/mod.rs:400`）⇒ 定时器**永不到期** ⇒ ⇒ 这条比第 5 条**更严格**：它证明「**比较值真的被喂进了 M-16**」（而不只是「guest 写总线这条路通」）。
6. ⚠️ **变异可红（施工期第五次订正新增）**：把固件的 `csrrwi x0, 0x7c0, 8` 的 **CSR 号改回 `0x7c1`**（即原字节表那版）⇒ **必须红** ⇒ 证明用例**真的在测「清线」这条通路** ⇒ ⚠️ **这条判别式是上面那条「CSR 号错一位 = 假绿」的护栏** ⇒ 没有它，那条缺陷就查不出来。
   ⇒ ⚠️ **「红」的标准是什么**：CSR 号改成 `0x7c1` ⇒ 写被**静默忽略**（`csr_write_is_ignored`，`csr.rs:565`）⇒ **定时器线永不被清** ⇒ M-16 的**挂起队列里线 3 一直在**、**`raised` 里线 3 一直在**、`active_irq` 一直指着 3 ⇒ ⇒ **判据是「M-16 的导出状态里线 3 已被清掉」**（`host.intc_state()` 是公开 API ⇒ `raised` / `pending` / `active_irq` 三个字段都能看）⇒ **CSR 号写错 ⇒ 线 3 仍在 ⇒ 红** ⇒ ✅ 这才是「清线真的发生了」的判别式。
   ⇒ ⚠️ **为什么不用「陷入次数」当判据**：定时器是**电平**语义（M-16 `poll()`：`now_ps >= mtimecmp_ps` ⇒ 请求线 3）⇒ 清掉线之后比较值**仍然**是 100 000 ps、而 `now_ps` **只增不减** ⇒ **下一个量子边界 `poll()` 又会请求线 3** ⇒ **反复陷入是平台的正常行为** ⇒ ⇒ **「陷入恰好一次」不成立** ⇒ **不许**把「x12 == 1」当判据（会恒红）。

⚠️ ⚠️ **判别式 4 的「x12 ≥ 1」为什么够用（写死，别推）**：判别式 4 只证明「**确实进了 handler**」（不是停在别处）⇒ 它与「清线有没有生效」是**两件独立的事** ⇒ 「清线」由**判别式 6**（M-16 导出状态）单独负责 ⇒ ⇒ **两条判据各管一件事，不互相顶替**。

⚠️ **时序（必须写进用例注释，否则一定写错）**：

| 事实 | 数值 | 出处 |
| --- | --- | --- |
| 每条指令推进的虚拟时间 | `INSTRUCTION_COST_PS` = **1 000 ps**（1 ns） | `m12_engine/mod.rs:81`（= `TIME_QUANTUM_PS / QUANTUM_INSTR_QUOTA`） |
| 时间量子 | `TIME_QUANTUM_PS` = **1 µs** = **1 000 条指令** | `m12_engine/mod.rs:69` |
| 一个量子的指令数 | `QUANTUM_INSTR_QUOTA` = **1 000 条** | `m12_engine/mod.rs:75` |
| M-16 的 `poll()` 采样点 | **量子边界**（每 1 000 条指令一次） | M-17 取指边界采样（`m17_backend/mod.rs:425`） |
| CLINT 的频率 | 10 MHz ⇒ **1 周期 = 100 000 ps = 100 ns** | `PS_PER_SEC / freq_hz`（`clint.rs:183`：`PS_PER_SEC = 1e12`） |
| `mtimecmp = 1` 周期 ⇒ M-16 的比较值 | **100 000 ps** | `clint.mtimecmp_ps()`（`clint.rs:238`） |

⇒ **推进 100 000 ps 的虚拟时间 ⇒ 100 条指令**（`100 000 ps / 1 000 ps`）⇒ **但 `poll()` 只在量子边界采样** ⇒ **实际至少要跨过 1 个量子（1 000 条指令）**，比较值才可能被看到。
⇒ ⇒ **用例第 2 条 `StepN` 跑 ≥ 2 000 条即可**（2 个量子，给足余量）⇒ 规格原文写的「100 000 条」**多算了 1 000 倍**（那是把 100 ns 误当成 100 µs 算出来的）⇒ 施工期第五次订正订正。

⚠️ ⚠️ **单位订正（施工期第五次订正）**：原文写「10 MHz ⇒ 1 周期 = 100 000 ps = **100 µs**」⇒ **错**：`100 000 ps = 100 ns = 0.1 µs` ⇒ 由此推出的「要推进 100 µs ⇒ 100 个量子 ⇒ 100 000 条指令」**整体偏大 1 000 倍**。
⇒ **判据（写死）**：`PS_PER_SEC = 1_000_000_000_000` ps/s · `freq_hz = 10_000_000` ⇒ `ps_per_cycle = PS_PER_SEC / freq_hz = 100 000 ps`（`clint.rs:183` 的实现就是这一行）⇒ **断言里不许手算**，照抄实现那一行的算式。

⚠️ ⚠️ **`sync_interrupts` 在批次边界调用**（`kernel_host/mod.rs:1963` 跑之前 / `:1999` 跑之后）⇒ guest 在批次内写下的 `mtimecmp` ⇒ **本批次内**定时器不会来 ⇒ **下一批次**才被喂给 M-16。
⇒ ⇒ **用例必须发**两条** `StepN`**：
- **第 1 条**（跑够写 `mtimecmp` + 使能 `mie` + `mstatus.MIE` + `mtvec`，然后进自旋）⇒ 跑完时 `sync_interrupts` 把 `mtimecmp` 喂进 M-16；
- **第 2 条**（跑 ≥ **2 000** 条 ⇒ 至少跨过 2 个量子边界）⇒ 期间 `poll()` 在量子边界看到 `now_ps >= mtimecmp_ps`（100 000 ps）⇒ 挂起线 3 ⇒ 拉取式交付 ⇒ `enter_trap` ⇒ guest 陷入。

⚠️ **一条 `StepN` 是不够的**（比较值在批次内还没进 M-16）⇒ 用例注释里要写明「**为什么是两条**」——否则后来的人会把它合并成一条，然后看到用例变红而不知道为什么。

⚠️ **`mtimecmp == 0` 表示「无期限」**（宿主把它原样传给 M-16，M-16 里 `0` 也表示「定时器关闭」）⇒ 固件**必须写 1**，不能写 0。

### 4.5 台账怎么改

- **D-82**：从「⏳ 已登记未做（P1 · 范围裁剪）」改成「✅ **已闭环**（v0.27 · STAGE-12）」，并**订正**其技术方案描述（按 §4.2 的三行）⇒ **闭环口径 = guest 指令级端到端判别力用例 + 台账订正**。
- ⚠️ 按「**闭环不回填**」的记账口径，它**仍计入未闭环数**（但要**减 1**）⇒ 收口时重算。

---

## §5 验收清单

### 5.1 Lane A · 契约层（A-1 ~ A-8）

- [ ] **A-1** `PLATFORM_SCHEMA_VERSION == 1`；`schema_version != 1` ⇒ `E_PLT-01`
- [ ] **A-2** 描述的**键集精确固定**（顶层**恰好 7 个键**：`schema_version` / `id` / `name` / `priority` / `auto_selectable` / `region_budget_bytes` / `regions`；⚠️ `description` 带 `skip_serializing_if` ⇒ **不带说明时不出现**、带说明时才是 8 个 ⇒ 断言要写「**7 个 / 带说明时 8 个**」，不能笼统写「8 个」）⇒ ⚠️ **`auto_selectable` 是裁决 E 新增的顶层字段**（`#[serde(default = "default_true")]` ⇒ 缺省为 `true` ⇒ **它总是出现在序列化结果里**、解析时缺了也合法）⇒ 另加一条用例钉住「缺省 `auto_selectable` 的描述**按 `true`** 参与自动选档」⇒ 另断言 `regions[0]` 的键集合按 `kind` 分别是 `{kind, base, size}` / `{kind, base, device}`；断言器件条目的键集合按 `device` 分派。先例：`crates/contract/src/session.rs:2431` 的「CLINT 状态的键集必须精确固定」
- [ ] **A-3** V-1 ~ V-12 **十二条校验规则**各自**一个独立的场景码**（`E_PLT-01` .. `E_PLT-08` + serde 错误 + `E_PLT-09` .. `E_PLT-11`）⇒ 每条一条用例；⚠️ **十二条规则对应十一个码**（V-8 落在 serde 层 ⇒ 用 serde 自己的错误、不占场景码）⇒ ⚠️ **V-10 ~ V-12 是宿主侧的三类失败，但码归契约层所有**（宿主只引用、不自建）⇒ 契约层 `crates/contract/src/error.rs` 里 `E_PLT_09_UNKNOWN_PLATFORM` / `E_PLT_10_PLATFORM_FILE_UNREADABLE` / `E_PLT_11_PLATFORM_FILE_MALFORMED` 三条**由契约层拥有** ⇒ 断言要写「**十二条规则 / 十一个码**」，不能笼统写「十二条码」
- [ ] **A-4** 描述**往返**（serialize → deserialize ⇒ 逐字段相等），含边界值（`region_budget_bytes` = `u32::MAX`）⇒ ⚠️ **裁决 J'' · 数值策略**：`region_budget_bytes` 取 **`u32`**（不是 `u64`）⇒ ① 它是**每区域**的字节预算，而地址空间只有 4 GiB ⇒ 超过 `u32::MAX` 的预算**没有意义** ⇒ ② 用 `u32` ⇒ **超出范围在 serde 反序列化阶段就失败**（归 V-8 的 serde 错误）⇒ **不需要**新增一条校验规则（也不需要契约层的十进制字符串策略 `numstr.rs` —— 那是给**虚拟时间**这类真会超 2^53 的量用的）⇒ 用例钉住「`u32::MAX` 的预算能往返；`u32::MAX + 1` 在 serde 阶段被拒」
- [ ] **A-5** `RegionSpec` / `DeviceSpec` 的**内部 tag 键名**逐字是 `"kind"` / `"device"`（不是 `type`、不是 `t`）
- [ ] **A-6** 错误族 `ErrorFamily::Platform` 新增 ⇒ 契约层错误族清单从 **14 → 15**；⚠️ **两处都补**：`error_families!` 宏的**一行** + 族标签表 `TAGS` 的**一行** ⇒ 靠契约层既有的完整性用例（**`every_family_tag_is_pinned`**，在 `crates/contract/src/error.rs` 的测试模块里）兜住 ⇒ **不新增**这条用例
- [ ] **A-7** `RegionDto.kind` 的三个常量（`REGION_KIND_*`）**上移到契约层**，M-14 改为 `pub use` ⇒ **M-14 内部调用点的字面量一条不改**
- [ ] **A-8** 契约层**不升版本**（`SESSION_SNAPSHOT_SCHEMA_VERSION` 仍为 3）

### 5.2 Lane B · 宿主 + 描述（B-1 ~ B-12）

- [ ] **B-1** `default` 描述建出的内存图与**今天 Default 档逐条一致**：3 个区域（`ram@0/1 MiB` · `peripheral@0x4000_0000 GPIO0` · `ram@0x8000_0000/4 MiB`）+ 预算 16 MiB
- [ ] **B-2** `qemu-virt` 描述建出的内存图与**今天 QemuVirt 档逐条一致**：6 个区域（`ram@0/1 MiB` · `ram@0x8000_0000/128 MiB` · `peripheral GPIO0@0x4000_0000` · `peripheral UART0@0x1000_0000` · `peripheral CLINT@0x0200_0000` · `peripheral PLIC@0x0C00_0000`）+ 预算 128 MiB
- [ ] **B-3** 外设区域的**窗口大小**由器件自己报 ⇒ 描述里**没有** `size` 字段 ⇒ 断言 `UART0` 窗口 == 8 字节、`CLINT` == 64 KiB、`PLIC` == `0x21_0000`、`GPIO0` == 16 字节（**架构字面量**）
- [ ] **B-4** **护栏**：两份描述的 `region_budget_bytes` ⇒ `bus.max_region_bytes()` 逐字相等（16 MiB / 128 MiB）
- [ ] **B-5** **V-7 生效**：描述声明一段 **超过**预算的 RAM ⇒ 建图前就被拒（`E_PLT-07`）⇒ **不留任何区域**
- [ ] **B-6** **零逻辑改动**：`map_memory` 的源码里**不得出现**任何地址字面量（`0x`）、任何器件构造调用、任何 `match profile` ⇒ 用例**读源码**断言（`include_str!` 读 `mod.rs` 文本）
- [ ] **B-7** **换描述、宿主零改动**：`minimal` 描述（低段 2 MiB + 高段 16 MiB @ `0x1000_0000`，与两档**都不同**）⇒ 显式建图 ⇒ 内存图**逐条**等于描述 ⇒ **且没有任何一行宿主代码为它而写**
- [ ] **B-8** **运行时路径**：`new_with_platform_file(path)` 从一个**仓外临时文件**读描述 ⇒ 建图成功 ⇒ 与同名内置描述**结果一致**
- [ ] **B-9** **自动选档**（裁决 E）**六条**等价性：Default 高段 ⇒ `default`；QemuVirt 高段 ⇒ `qemu-virt`；两档之外 ⇒ 回落 `default`；低段 ⇒ `default`；**低段超出 1 MiB** ⇒ 回落 `default`（⇒ 随后报未映射）；`minimal` **永不**被自动选中（`auto_selectable: false`）
    > ⚠️ **「区间在外层」这条结构也要钉住**：镜像**有多个区间**时，判据是「逐个区间过、遇到只被更高优先级描述覆盖的区间就**立即**选它」⇒ 用例要有一个**多区间**的镜像（一个区间落 default 高段、另一个落 QemuVirt 高段）⇒ 结果必须是 `qemu-virt`。
- [ ] **B-10** **兼容 API**：`memory_profile()` 对 `default`/`qemu-virt` 分别返回 `Default`/`QemuVirt`；新增 `platform_id()` 返回**真实 id**；`new_with_profile` 两条路径与今天**逐字等价**
- [ ] **B-11** **描述坏了要报**：`schema_version` 不对 / 区域重叠 / 预算超了 ⇒ 各自**不同的码**（不是同一个）
- [ ] **B-12** **M-14 行为一字不变**：既有 `m14_bus` 的**全部**用例（含 v0.26 的 14 条验收）**一条不改、一条不红**

### 5.3 Lane C · D-82 + 集成（C-1 ~ C-4）

- [ ] **C-1** guest 指令级：写 `mtimecmp` + 使能 MTIE ⇒ **真的陷入**，`mcause == 0x8000_0007`（架构字面量）
- [ ] **C-2** **变异可红**：固件里去掉 `sw t1, 0(t0)` ⇒ **必须红**（陷入不发生）⇒ 证明用例不恒绿
- [ ] **C-3** 陷入时 `mepc` 落在自旋指令、`mtval == 0`
- [ ] **C-4** **17 条既有测试锚点**逐条改到位（清单见 §6.3）⇒ **既有断言的语义不变**（改的是「取值来源」：从常量改成从描述查；**不是**把断言删掉或放松）

---

## §6 门禁

### 6.1 九项（与既有口径一致）

| # | 门禁 | 命令 |
| --- | --- | --- |
| 1 | 格式 | `cargo fmt --all -- --check` |
| 2 | 静态 | `cargo clippy --workspace --all-targets --offline -- -D warnings` |
| 3 | 单测 + 集成 | `cargo test --workspace --no-fail-fast --lib --tests --offline` |
| 4 | 前端 | `tools/check-front.sh` |
| 5 | 黄金对拍 | `tools/golden-run.sh --self-check` + `--keep-going` |
| 6 | WASM | `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` |
| 7 | 浏览器真机 | `node tools/browser-verify.mjs` |
| 8 | 真实 RTOS | `tools/boot-osz.sh` ⇒ **VERDICT MATCH** |
| 9 | 变异自检 | 每 lane 至少一条对照 **0 红** |

### 6.2 本批**额外**要跑的（2 项）

- **S-1 源码级判别**：B-6 的「`map_memory` 里没有地址字面量」在**整合后的树**上再验一次（防止 lane 合并时把硬编码塞回去）。
- **S-2 描述往返**：把两份内置描述读出来 → 解析 → **重新序列化** ⇒ 与仓内文件**逐字节相等**（⇒ 描述文件是**规范形式**，不会因为 serde 的字段顺序漂移）。

### 6.3 既有测试锚点（17 条，逐条处置）

| # | 位置 | 改法 |
| --- | --- | --- |
| 1 | `crates/kernel/tests/v026_memory_map.rs:189` | 区域数 6 ⇒ 从 `qemu-virt` 描述查 |
| 2 | 同上 `:200` | RAM 区域数 2 ⇒ 从描述查 |
| 3 | 同上 `:213/219` | `base`/`size`/`base+size` ⇒ 从描述查 |
| 4 | 同上 `:229` | 低段 `size` ⇒ 从描述查 |
| 5 | 同上 `:251` | Default 区域数 3 ⇒ 从描述查 |
| 6 | 同上 `:263` | Default 高段 `size` ⇒ 从描述查 |
| 7 | 同上 `:277/285` | 护栏预算 ⇒ 从描述查 |
| 8 | `crates/kernel/tests/v026_payload_slimming.rs:71` | `assert_qemu_virt_layout` ⇒ 从描述查 |
| 9 | `crates/kernel/src/kernel_host/tests_osz_wiring.rs:184/191/199` | 「一个 128 MiB RAM」⇒ 从描述查 |
| 10 | `crates/kernel/src/kernel_host/tests_session_snapshot.rs:389-400` | 默认档 3 区域 + base/kind/target ⇒ 从描述查 |
| 11 | 同上 `:830/846/921` | 3 段 / 4 段拓扑 ⇒ 从描述查 |
| 12 | `crates/kernel/tests/session_snapshot.rs:226-228` | 两块 RAM 的 `(base,size)` ⇒ 从描述查 |
| 13 | `tests_osz_wiring.rs:137/214/226` · `tests_v024.rs:239` | `memory_profile()` ⇒ 保留（裁决 C ⇒ **不改**） |
| 14 | `tests_osz_wiring.rs:141/145/205/206/207` · `tests_v024.rs:39/41/43/54/56/58` | 外设寄存器地址 ⇒ 从描述取 base + 偏移 |
| 15 | `tests_m11.rs:38` · `d46_probe_pipeline.rs:146/309/479` · `session_snapshot.rs:157/227` · `v026_uart_tx_ring.rs:58` | `DEFAULT_RAM_*` / `DEFAULT_GPIO_BASE` ⇒ 从描述取 |
| 16 | `v026_memory_map.rs:35/38/44` · `v026_payload_slimming.rs:41/44` | **自抄副本** ⇒ ⚠️ **保留为「架构字面量」**（不从描述取 —— 它们的作用是「不与实现同源」）⇒ 但**要加注释**说明它与描述的对应关系 |
| 17 | `kernel_host/mod.rs:918-935` · `:1799` | 挑档判据 ⇒ 描述化（裁决 E） |

⚠️ **第 16 条的纪律**：那两份**自抄副本**（`0x8000_0000` / 128 MiB）**故意不从被测实现反推** ⇒ 描述化之后它们**仍然**是独立字面量 ⇒ **保留**。⚠️ 但要**加一行注释**标明它与 `qemu-virt` 描述的对应关系（否则读者会以为它是重复）。

---

## §7 破坏性变更清单（6 条）

| # | 变更 | 影响面 | 缓解 |
| --- | --- | --- | --- |
| 1 | 契约层**新增**错误族 `ErrorFamily::Platform`（**14 → 15** 族） | 契约层错误族清单 + 族标签表 `TAGS` | 同批补**两处**（宏 + 表）；⚠️ **前端零改动**（前端不镜像错误族） |
| 2 | `RegionDto.kind` 的三个常量**归属**上移到契约层（M-14 改 `pub use`） | M-14 内部（字面量不变） | 既有断言一条不改 |
| 3 | `MemoryProfile` 的**语义**从「两档」变成「当前描述 id 的兼容投影」 | `memory_profile()` 5 个调用点 | 保留旧取值（裁决 C）⇒ **既有 4 条断言不改** |
| 4 | `map_memory` 的**签名**从 `profile` 改成「描述引用」 | 仅宿主内部（2 个调用点） | 私有函数 ⇒ 外部不可达 |
| 5 | **新增** `Platform` 错误族的场景码 `E_PLT-01` .. `E_PLT-11` | 契约层错误码清单 | §5.1 A-6 |
| 6 | **不升** `SESSION_SNAPSHOT_SCHEMA_VERSION`（仍 3） | — | ⚠️ 描述**不进会话载荷** ⇒ 前端镜像不动 |

⚠️ **不破坏的**：`SessionSnapshotDto` 的形状、快照的 `E-SNAP-05` 行为、`RegionDto` 的字段集、`M14MapDto` 的形状 ⇒ **会话载荷向后兼容**。

---

## §8 与台账的关系（收口时回填）

| 条目 | 本批做什么 | 闭环口径 |
| --- | --- | --- |
| **D-123**（内存图通用化） | **B 批**（描述格式 + 描述驱动）落地 ⇒ A 批 + B 批**都**完成 | **闭环** ⇒ 按「闭环不回填」口径仍计入未闭环数，但**减 1** |
| **D-82**（CLINT 固件侧 MMIO） | 补 guest 指令级端到端判别力 + **订正**台账措辞（§4.2） | **闭环** ⇒ 减 1 |
| **D-10**（每区域上限护栏） | 语义**再修订**一次：新增「**声明期**」检查（V-7 / `E_PLT-07`），运行期仍是「物理占用」（`E_BUS_09`） | **修订**（不是闭环）⇒ 不参与减法 |
| **D-13**（字段可见性） | 已闭环（v0.26） | 不动 |
| **（新）D-124** | 契约层无 MemoryMap 命令 / 事件 ⇒ 前端拿不到真实内存图 | 登记未做 |
| **（新）D-125** | 器件连线仍写死在宿主 ⇒ 换平台改连线仍要改代码 | 登记未做 |
| **（新）D-126** | 参考侧地址三份副本与宿主描述手工对齐 | 登记未做 |
