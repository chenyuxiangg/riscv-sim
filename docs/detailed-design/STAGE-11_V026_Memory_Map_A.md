# STAGE-11 · v0.26 冻结规格 · 内存图通用化 A 批（D-123-A）

> **状态**：**已冻结**（2026-10-23）—— 本批的范围、裁决、验收清单与门禁以本文档为准。
> **上游**：`STAGE-10`（v0.25 · 取指路径对齐 + 前端补洞）—— 取指宽度语义已定死，是本批的前置。
> **下游**：`STAGE-12`（内存图通用化 **B 批** · D-123-B）—— **本批是它的前置**（B 的「换一份内存图描述、宿主代码零改动」必须先有懒分配，否则换描述就等于换 OOM）。
> **台账**：`docs/reviews/REVIEWS.md`（**D-123** · **D-13** · D-10）。

---

## §0 背景与范围

### 0.1 本批的一句话

**让「登记一段地址空间」不再等于「立刻申请那么多物理内存」**，并把这次变更带来的破坏性影响一次性处理干净。

v0.25 批收口时的架构复盘登记了 **D-123**：内存图是宿主**硬编码的两档**（`MemoryProfile::{Default, QemuVirt}`），换一块板子就得改宿主代码。台账同时写清了**根因不是「内存图形状的必然」，而是 RAM 急分配**：

- `map_ram` 走 `prepare_bytes` 的 `vec![0u8; size]`（`m14_bus/mod.rs:716`）⇒ **整段 RAM 一次申请**；
- 于是 QemuVirt 档的 128 MiB RAM = **128 MiB 物理内存**；
- 又撞上每区域 16 MiB 上限（`DEFAULT_MAX_REGION_BYTES`，`m14_bus/mod.rs:56`，**D-10 的防 OOM 护栏**）⇒ 128 MiB **被迫拆成 8 个 16 MiB 区域**（`kernel_host/mod.rs:795~799`）；
- 才需要一档「128 MiB」的内存图 ⇒ 才出现「两档」。

**连带代价（台账实测）**：`export_pages`（`m14_bus/mod.rs:1182`）把每段 RAM 按 64 KiB（`MEM_PAGE_CHUNK_BYTES`）**整块导出、不跳过全零页** ⇒ **Default 档** 3 region / 80 页 / RAM 载荷 **5.00 MiB** / 整份 JSON **10.00 MiB**；**QemuVirt 档** 13 region / 2 064 页 / RAM 载荷 **129.00 MiB** / 整份 JSON **258.06 MiB** ⇒ **这些字节几乎全是零**（guest 只用了几 KiB，快照却背着 258 MiB）。

### 0.2 本批做的四件事

| # | 事情 | 欠账 | 性质 |
| --- | --- | --- | --- |
| 1 | **RAM 改页级懒分配**（4 KiB 页，写时分配；读没写过的页回 0） | **D-123·A** | 结构性能力（性能与载荷） |
| 2 | **护栏从「声明大小上限」改成「物理占用上限」** | **D-123·A** + **D-10** | 护栏语义转换（**不删**） |
| 3 | **TX 捕获环进会话快照** | 随 A 同批 | 载荷完整性 |
| 4 | **D-13 字段可见性收紧** | **D-13** | 封装（台账措辞需订正，见裁决 D） |

⚠️ **「C 做法」的 C（RAM 区域按需增删）与 A 是一件事的两面**：`region 登记`与`物理页`必须**分层**（裁决 A）—— 登记是 O(1) 且零物理代价，物理页在**写时**增删。

⚠️ **本批是 B 批的前置，不是 B 批本身**：B（内存图由工程/平台描述）留到 `STAGE-12`。本批做完的**效果**是「一个 128 MiB 的 RAM 区域登记成本 ≈ 0」⇒ B 换描述时才不会换出一份 OOM。

### 0.3 本批**明确不做**的事（10 条）

| # | 不做 | 为什么 |
| --- | --- | --- |
| 1 | **D-123·B**：内存图由工程/平台描述 | 下一批（`STAGE-12`）；本批是它的前置 |
| 2 | **D-123·C 独立成批** | C 的实质（登记与物理分层）随 A 一起落地；「按需增删区域」在 A 里表现为「写时增删页」 |
| 3 | **D-94** S 扩展 / PMP / 计数器 | 工作量 ≈ 实现半个特权级 ⇒ 独立一批 |
| 4 | **D-87**（轨迹机器码时间倒流）/ **D-89**（部分写失效粒度） | 与本批无交集 |
| 5 | **D-114** M-17 `step_n` 的同形状活锁 / **D-115** watchdog 处置链路 | 与本批无交集 |
| 6 | **D-82**（CLINT 的固件侧 MMIO 通路） | 留到 B 批顺带做 |
| 7 | **M-21 日志 / M-22 HMS / M-23 持久化** | v0.28 批 |
| 8 | ROM 改懒分配 | ROM 内容是**镜像的一部分**（输入），必然在内存里 ⇒ 懒分配对它没有收益 |
| 9 | 内存图**描述格式**（即便只是 JSON） | 那是 B 批的产物；本批只把「一段 RAM 的成本」变成可描述的量 |
| 10 | 前端内存映射表改造 | 本批**不动**前端（见裁决 D 的实测结论） |

---

## §1 裁决

### 裁决 A · RAM **页级懒分配**（4 KiB 页，写时分配）

**数据结构**：`RamBackend` 从「一块 `Vec<u8>`」改成「**页表**」：

```rust
pub struct RamBackend {
    /// 已物化的页：键 = region 内页号，值 = 4 KiB 载荷。
    ///
    /// ⚠️ **没在表里的页 = 从没被写过** ⇒ 读它回 0，**且不分配**。
    pages: BTreeMap<u32, Box<[u8]>>,
    /// Bound base address (cached for diagnostics).
    base_addr: u32,
}
```

- **页大小 = 4 KiB**（新常量 `MEM_PAGE_BYTES: usize = 4096`，取代 `MEM_PAGE_CHUNK_BYTES` 的地位，见裁决 C）。
- ⚠️ **region 大小不必是 4 KiB 的整数倍**：最后一个不满的页**整页物化**，但只有落在 region 内的那部分字节**可达**（访问先过 region 边界检查 ⇒ `E_BUS-04`）⇒ 最多浪费 4 KiB / region，**换取实现简单**（页号 = `off / 4096`、页内偏移 = `off % 4096`）。

**读**：

- 页**不存在** ⇒ 读回 **0**，**不分配**、**不计数**。
- 页存在 ⇒ 读该页对应字节。

**写**（顺序**不可交换**，写进规格免得下一个人写反）：

1. **先**做 region 边界检查（`off + width <= region.size`）⇒ 越界 ⇒ **`E_BUS-04`，且不得留下任何页**（一次越界写不能凭空物化一页）。
2. **再**做**物理占用上限**检查（见裁决 B）⇒ 超限 ⇒ **`E_BUS-09`，且不得留下任何页**。
3. **再**按需物化页（写时分配），然后写入。

⚠️ **一次访问**可能**跨两页**（⚠️ 本条在 v0.26 施工中被**实测订正**，见下）：

- **原判据是错的**：原文写「页大小 4 096 是 4 的倍数、最大访问宽度 4 字节 ⇒ `off / 4096` 与 `(off + 3) / 4096` **恒相等** ⇒ 永不跨页」。**反例**：`off = 4095` 时 `4095 / 4096 = 0` 而 `4098 / 4096 = 1` ⇒ **一个 4 字节写会跨到下一页**。
- **正确的事实**：最大访问宽度是 **4 字节** < 页大小 ⇒ 一次访问**最多跨两页**（`first = off / 4096`、`last = (off + width - 1) / 4096`，且 `last - first ≤ 1`）。
- **实现因此按「可能跨两页」写**，不依赖「永不跨页」的快捷假设 ⇒ 4 092 处的 4 字节写落在**同一页**内（**A-11** 仍成立并有用例钉住），而 4 095 处的 4 字节写会**正确地**写到两页里。
- ⚠️ **教训**：「宽度 ≤ 4 字节」**推不出**「不跨页」—— 还要看**起点**在页内的位置。规格里的算式必须逐个反例验一遍，不能凭「4 是 4096 的因子」就下结论。

⚠️ **字节写路径**（`write_region_byte`）走**同一条**写时分配路径 ⇒ 不许有「宽写懒分配、字节写急分配」的分叉。

**可观测计数器**（判别力③ 的唯一合法依据）：

- `RamBackend::allocated_pages(&self) -> usize`
- `Bus::ram_allocated_bytes(&self) -> u64`（全总线已物化字节数）

⚠️ **不许靠「读值反推」物理占用** —— 读回 0 与「页存在但内容是 0」**不可区分**（D-107 同形状的坑）⇒ 必须有**计数器**。

⚠️ **统计不变式**：懒分配**只动物理存储**，**不动总线统计**（`reads` / `ram_hits` / `misses` / `writes` 的口径与改动前**逐条相同**）⇒ 既有断言一条都不许删改。

### 裁决 B · 护栏语义转换：**声明大小上限 → 物理占用上限**（**不删**）

⚠️ **D-123 明写**：「护栏**不能删**……A 落地后它可以放宽但**不能去掉**（否则一个畸形镜像就能申请 4 GiB 物理内存）」。

- `Bus::max_region_bytes` 的语义从「**单个 region 的声明大小上限**」改成「**单个 region 相对宿主预算的上限**」。
- 常量**名字与数值都不变**（`DEFAULT_MAX_REGION_BYTES = 16 MiB`）⇒ **不是新护栏，是同一个护栏换了一个被测量的量**。
- ⚠️ **被测量的量按 region 的种类定**（这是本裁决的完整定义，**不是**「一律按物理占用」）：

| region 种类 | 持有内存？ | **被测量的量** | 为什么 |
| --- | --- | --- | --- |
| **RAM** | 懒分配（**只有写过的页**才占） | **物理占用**（已物化页数 × 4096） | 懒分配之后，**声明大小不再是内存成本** ⇒ 按声明大小禁就是禁错了对象 |
| **ROM** | 是（内容是镜像的一部分，**必然**在内存里） | **声明大小**（== 物理占用） | ROM 的声明大小**就是**它的物理占用 ⇒ 两个口径**同一个数** |
| **外设窗口** | **否**（它占用的是**地址区间**，不是内存） | **声明大小** | 它防的是「一个设备声明一个 GiB 的窗口」⇒ 与今天**逐条一致**，既有断言 `the_cap_also_guards_the_peripheral_window` **一条不许删** |

- ⚠️ **D-10 的护栏没有被削弱**（逐条）：

| 路径 | 被测量的量 | 判定 |
| --- | --- | --- |
| `map_ram(base, size, &[])` | 物理占用 **0** | **不检查声明大小** ⇒ 可以声明 128 MiB 的区域（零物理代价） |
| `map_ram(base, size, init)`（`init` 非空） | `ceil(init.len() / 4096)` 页 | 超 cap ⇒ **`E_BUS-09`**（畸形镜像仍被挡） |
| `map_rom(base, size, content)` | **== 声明大小**（ROM 内容必然在内存里） | 超 cap ⇒ **`E_BUS-09`**（**与今天逐条一致**） |
| **外设窗口**（`map_peripheral` / `map_peripheral_instance`） | **== 声明大小** | 超 cap ⇒ **`E_BUS-09`**（**与今天逐条一致**） |
| 运行期写入 | 分配后的页数 | 超 cap ⇒ **`E_BUS-09`** |

⇒ **一个畸形镜像今天被 `E_BUS-09` 挡住，A 落地后仍然被 `E_BUS_09_TOO_LARGE` 挡住** ⇒ 护栏在，只是从「声明多大就禁多少」变成「真占这么多才禁」（**RAM 这一路**）。

⇒ **一句话**：护栏是「**单个 region 相对宿主预算的上限**」；**RAM 按物理占用量，其余按声明大小量** ⇒ **没有任何一路的保护被弄丢**。

⚠️ **错误码的处理**：`E_BUS_09_TOO_LARGE`（`m14_bus/mod.rs:101`）**保留原号**，但**被测量的量**变了 ⇒

- 码的**语义**（「这个 region 超过了每区域上限」）**不变** ⇒ **不新增码**，避免「一码多义」；
- **错误消息必须写清楚被测量的是哪一个量**（RAM 写 "physical" / "allocated"，ROM 与外设窗口写 "size"）；
- ⚠️ **判据不许靠 message 文本**（项目规矩）⇒ 用例只断言 `family == Bus && scenario == 9`；
- 既有断言 E_BUS-09 的用例要**逐条复核**：凡是「**RAM 声明大小**超限」的断言，要么改判据（改成物理占用超限）、要么改成「现在合法了」；**ROM 与外设窗口的断言不许改判据**（它们逐条不变）。

⚠️ **护栏预算是宿主策略**：`Bus::with_max_region_bytes` / `set_max_region_bytes` 的既有文档就写着「use this constructor only when the host has its own budget policy」⇒ A 落地后这句话**才真正成立**：护栏的数值**由宿主按自己的内存图决定**，默认仍是 16 MiB。

⇒ **QemuVirt 档把预算抬到 128 MiB**（= 它声明的 RAM 大小），**Default 档保持默认 16 MiB**（它的两段 RAM 都远小于 16 MiB）。

⚠️ **验收上「护栏只能放宽不能去掉」怎么证**：**默认** `Bus::new()` 的 cap **仍是 16 MiB**（源码级断言 + 行为断言），且「往一个 region 写超过 16 MiB」⇒ `E_BUS-09`；**抬高** cap 后 ⇒ 同样的写**成功**。

### 裁决 C · 快照页：**4 KiB 粒度** + **只装写过的页** + **`schema_version` 2 → 3**

**C-1 粒度**：`MEM_PAGE_CHUNK_BYTES`（64 KiB，`m14_bus/mod.rs:70`）**退役**，改用 `MEM_PAGE_BYTES = 4096`。

- ⚠️ 选 **4 KiB**（物理页）而不是 64 KiB 的理由：4 KiB **恰好是物化粒度** ⇒ 导出的一页 = 物理上真实存在的一页 ⇒ 「导出没跳过任何东西」是**结构上**成立的，而不是靠「64 KiB 里恰好也是零」碰巧成立。
- 64 KiB 会把**一个**物化页塞进 **16 个**导出条目 ⇒ 载荷里 15/16 是零 ⇒ 与今天同形状的病。

**C-2 内容**：`export_pages`（`m14_bus/mod.rs:1182`）⇒ **只导出已物化的页**：

- 条目 = 已物化的页，一页一条；
- `base` = `region.base + 页号 × 4096` ⇒ **落在该 region 的页网格上**（region 的 base 4096 对齐时就是绝对 4096 对齐）；
- `bytes` = 该页在 region 内的**有效字节**（`min(4096, region.size - 页内偏移)`）⇒ **不补齐到页边界**（沿用契约层 `MemPageDto` 的既有约定，见 `crates/contract/src/session.rs:154`）。

⇒ **契约层 `MemPageDto` 的形状不变**（`{ base: u32, bytes: Vec<u8> }`）⇒ **只有语义变** ⇒ 这正是必须升 schema 的理由。

**C-3 还原**：`load_pages`（`m14_bus/mod.rs:1223`）⇒

- **两阶段保留**（先全量只读校验，全过才写一个字节）⇒ 校验不过 ⇒ RAM **原封不动**（STAGE-3 §5 的不变式）；
- 新增**页网格对齐校验**：页必须落在**它所属 region 的页网格**上 ⇒ `page.base % 4096 == region.base % 4096` ⇒ 不对齐 ⇒ `E_BUS-11_PAGE_OUTSIDE_RAM`；
  - ⚠️ **为什么不是「`base % 4096 == 0`」**：导出的一页是 **region 相对**的一页（`base = region.base + 页号 × 4096`）⇒ 若 region 的 base 本身不 4096 对齐，导出的 base 也不会对齐 ⇒ 用绝对对齐判会把**自己导出的东西**判成非法 ⇒ **往返就断了**。
  - ⚠️ **宿主今天用的 base 全部 4096 对齐**（`0x0` · `0x8000_0000` · `QEMU_VIRT_RAM_BASE + i × 16 MiB`）⇒ 在这些 region 上，本规则**退化成**「`base % 4096 == 0`」⇒ 与「绝对对齐」**等价**。
  - ⚠️ **契约层不要求对齐**（它只要求「页升序、不重叠、落在某个 RAM 区域内」）⇒ 本规则是 **M-14 自己的**规则 ⇒ 两层**不冲突**；
- 写入时**按需物化**页（还原出来的页**就是**被写过的页）⇒ 还原后的物理占用 = 载荷里页数 × 4096；
- ⚠️ **还原也要过护栏** ⇒ 一个 region 的物理占用超过 cap ⇒ `E_BUS-09` ⇒ **还原失败**。这是**预期行为**（宿主预算不够就不还原），不是缺陷。

**C-4 版本**：`SESSION_SNAPSHOT_SCHEMA_VERSION` **2 → 3**（`crates/contract/src/session.rs:90`）。

- 旧载荷（`version = 2`）⇒ **E_SNAP-05 显式拒收**（既有校验在 `session.rs:680`，`cp_meta.version` 在 `:687`）⇒ **不许默默换形状**。
- ⚠️ **前端不受影响**：`front/lib/message.js` 镜像的是**协议** `schema_version`（5），**不是**会话快照的 schema ⇒ 契约层升版**不需要**动前端（实测确认）。

**C-5 载荷瘦身的效果是可判定的**（不是「显著变小」这种散文）：

| 场景 | 期望 |
| --- | --- |
| QemuVirt 档新建会话、**一个字都没写** | `mem_pages` **为空**（**0 条**）⇒ 整份 JSON **< 1 MiB**（基线 **258.06 MiB**） |
| 写**恰好一页** | `mem_pages` **恰好 1 条**，`bytes.len() == 4096` |
| 写**两页不相邻** | `mem_pages` **恰好 2 条** |

### 裁决 D · **D-13** 字段可见性收紧（**台账措辞订正**）

- `Region` 的四个字段（`base` / `size` / `kind` / `target`，`m14_bus/mod.rs:403`）⇒ 从 `pub` 改成**模块内可见**。
- `RamBackend.bytes` ⇒ 字段**消失**（变成 `pages`，裁决 A）⇒ 顺带闭环。
- `RomBackend.bytes`（`m14_bus/mod.rs:443`）⇒ 私有 + 一个 `pub fn bytes(&self) -> &[u8]` 访问器。
- ⚠️ **`m14_bus/tests.rs` 是 `m14_bus` 的子模块** ⇒ 子模块**本来就能**访问父模块的私有字段 ⇒ **不需要 `pub`** ⇒ 既有测试**不必改**。
- ⚠️ **实测：`Region` / `RamBackend` 在 `m14_bus` 模块外零使用**（`grep` 全仓命中只有 `src/m14_bus/` 内的）⇒ 收紧**不会**波及其它 crate。

⚠️ **台账 D-13 的措辞是错的，本批订正**。台账原文说「收紧可见性要同步改 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable`」——**实测两条都不成立**：

| 台账说的 | 实测 | 结论 |
| --- | --- | --- |
| `crates/kernel/tests/m06_load_to_bus.rs` 要改 | 该文件里 `.bytes` / `.target` / `RamBackend` / `Region` **零命中** | **不受影响** |
| `front/views/m02_viewcomponents.js` 的 `renderMapTable` 要改 | 它渲染的是 **mock demo 数据**（`m.base` / `m.type` / `m.size` / `m.detail`，`m02_viewcomponents.js:180`），与 Rust 侧 `Region` **无关** | **不受影响** |
| （台账没提）`crates/kernel/src/m14_bus/tests.rs` | 同模块子模块 ⇒ 私有可见性够用 | **不受影响** |

⇒ **本批不碰这两个文件**，并在收口时把台账 D-13 那句改成实测结论。

### 裁决 E · **TX 捕获环进会话快照**

⚠️ **为什么它跟 A 同批**：捕获环的字节是**访客已经发出去、还没被宿主取走**的内容 ⇒ 它**是会话状态** ⇒ 不进快照 ⇒ 一次快照/还原就会**丢字节**（与 v0.24 的 `rx_queue` 同形状的问题，那次已修）。

- `UartStateDto`（`crates/contract/src/session.rs:218`）新增两个字段：

  | 字段 | 类型 | 含义 |
  | --- | --- | --- |
  | `tx_ring` | `Vec<u8>` | 捕获环里**全部**待取字节（**队头在前**，与 `rx_queue` 同一口径） |
  | `tx_dropped` | `u64` | 溢出**丢弃的累计字节数**（生命周期计数，不是寄存器） |

- ⚠️ **既有 `tx` 字段的语义不变**（= 队头，`uart.rs:928` 的 `self.tx.peek()`）⇒ 既有读取方不受影响。
- ⚠️ **不变式**（与 `rx` / `rx_queue` **同构**）：`tx` **必须**等于 `tx_ring` 的**第一个**字节；`tx_ring` 为空 ⇒ `tx` **必须**是 `None` ⇒ 契约层 `validate()` **拒掉**不自洽的载荷。
- ⚠️ **还原**：`restore_session_state` 把 `tx_ring` **整体**灌回捕获环；**`tx` 不单独还原**（它是 `tx_ring` 的冗余表示）⇒ 与 `rx` / `rx_queue` 的还原口径一致。
- ⚠️ **`tx_dropped` 还原时取「覆盖」而不是「累加」** ⇒ 快照携带的是取样瞬间的累计值（快照 = 状态）。**这是本批的一个显式裁决**（若取「累加」，则还原后的计数与被快照的那次不是同一个数）。
- ⚠️ **丢失要看得见**：捕获环满时是「丢最新、保最早」（`uart.rs:324` 的 `TxCapture`）⇒ `tx_dropped` 让这件事**可见**，不是静默的。
- ⚠️ **载荷有界**：捕获环默认 4 KiB ⇒ 每台 UART 最多 4 KiB + 1 个 `u64` ⇒ 快照载荷的增量**有界**。

### 裁决 F · lane 切分：**两条**，**文件所有权零交集**

| lane | 拥有（文件所有权） | 做 |
| --- | --- | --- |
| **Lane A · M-14 总线** | `crates/kernel/src/m14_bus/**` | 裁决 A + B + C-1/C-2/C-3 + D |
| **Lane B · 契约 + 宿主 + 外设** | `crates/contract/src/session.rs` · `crates/kernel/src/m15_peripheral/**` · `crates/kernel/src/kernel_host/**` · `crates/kernel/tests/**` | 裁决 C-4 + E + 裁决 G |

⚠️ **为什么 D-13 归 A 不归 B**：D-13 改的是 `Region` / `RamBackend` 的字段可见性 ⇒ 与 A 的数据结构改造是**同一个文件、同一批改动** ⇒ 拆开只会得到「两个 lane 改同一个文件」的冲突。

⚠️ **行为依赖（不是文件依赖）**：Lane B 的「快照往返」用例依赖 Lane A 的导出格式（4 KiB / 只装写过的页）⇒ 规格已**冻结** ⇒ 两条 lane **可并行施工**，但**串行合并**（**先 A 后 B**）+ 合并后跑全量门禁。

### 裁决 G · **QemuVirt 档：8 个区域 → 1 个 128 MiB 区域**

- `kernel_host/mod.rs:795~799` 的「128 MiB 拆成 8 个 16 MiB 区域」⇒ 改成**一个 128 MiB 的 RAM 区域**。
- ⚠️ **前提是裁决 B**（护栏不再按声明大小卡）⇒ 这是「护栏语义转换真的生效了」的**可见证据**，不是一次顺手的重构。
- ⚠️ **guest 看到的地址空间不变**：osz 的链接脚本要 `ITCM(0x8000_0000,60M)` / `INIT(0x83C0_0000,4M)` / `DTCM(0x8400_0000,64M)` ⇒ 0x8000_0000 起**连续 128 MiB** ⇒ 1 个区域与 8 个连续区域**覆盖同一段地址** ⇒ 访客**看不出区别**。
- ⚠️ **Spike 侧对齐**：参考侧用 `-m2147479552:4096,2147483648:134217728`（`tools/boot-osz.sh:45`）⇒ 同样是 128 MiB 连续 ⇒ 两侧地址空间**本来就一致** ⇒ 改完之后**仍然**一致。
- 宿主同时把护栏预算抬到 **128 MiB**（裁决 B 的「宿主策略预算」）。
- ⇒ 区域数 **13 → 6**（1 低段 + 1 高段 + GPIO + UART + CLINT + PLIC）。

⚠️ **不合并两档**：`MemoryProfile::{Default, QemuVirt}` **本批不动** ⇒ 「两档合一」是 B 批（`STAGE-12`）的事，本批只把它的**成本前提**准备好。

---

## §2 验收清单

### 2.1 Lane A · M-14 总线（A-1 ~ A-12）

- [x] **A-1** 读**没写过**的 RAM ⇒ 回 **0**（**不是**陷阱），且**零页**被物化
      > 证据：`crates/kernel/src/m14_bus/tests.rs` 的
      > `reading_unwritten_ram_returns_zero_and_materializes_no_page` ——
      > 读回 0、`allocated_bytes() == 0`、`allocated_pages() == 0`。
      > 判据用**计数器**（**不是读值反推**）。
- [x] **A-2** 读**没登记**的地址 ⇒ 架构陷阱 **cause 5 / 7**（**不是 0**）
      > 证据（m14_bus 层）：`reading_an_unregistered_address_is_still_unmapped_not_zero`
      > —— 未登记 ⇒ `family == Bus && scenario == 1`（**没变成 0**）、零物化。
      > ⚠️ **架构陷阱那一半在 M-13 侧**（m13_iss 模块，**本批一个字未改**）：
      > `an_unmapped_load_traps_with_cause_five`（`executor.rs:1313`）与
      > `an_unmapped_store_traps_with_cause_seven`（`executor.rs:1345`）
      > **两条仍然全绿** ⇒ **D-99 的判别力没破**（两层没合成一层）。
- [x] **A-3** 写**一页** ⇒ 恰好物化 **1 页**（`allocated_pages() == 1`、`allocated_bytes() == 4096`）
      > 证据：`writing_one_page_materializes_exactly_one_page` ——
      > `pages == 1`、`bytes == 4096`。
- [x] **A-4** 写**两页不相邻** ⇒ 物化 **2** 页；中间那页读回 **0**
      > 证据：`writing_two_non_adjacent_pages_materializes_two_pages` ——
      > `pages == 2`、`bytes == 8192`、中间页读回 0。
- [x] **A-5** 护栏：**默认** cap 仍是 **16 MiB**；往一个 region 写超 16 MiB ⇒ **`E_BUS-09`**；**抬高** cap 后同样的写**成功**
      > ⇒ 「护栏只能放宽、不能去掉」**可证**。
      > 证据：`the_default_physical_occupancy_cap_is_still_sixteen_mib`；
      > 另有两处集成层钉住同一件事：
      > `crates/kernel/tests/m14_m15_m19_wiring.rs` 的
      > `oversized_region_is_rejected_without_allocating`（声明 256 MiB **成功**且
      > `ram_allocated_bytes() == 0`；真占超预算 ⇒ `E_BUS-09` 且**零页留下**）与
      > `the_region_size_cap_is_configurable`（声明免费 → 写第二个页被拒
      > `scenario == 9` → 抬 cap 后同一个写成功 → `ram_allocated_bytes() == 2 * 4096`）。
- [x] **A-6** 单个 region 的**声明**大小可 > 16 MiB：`map_ram(base, 128 MiB, &[])` **成功**且**零物化页**
      > 证据：`a_ram_region_may_declare_more_than_the_cap_and_costs_nothing` ——
      > 声明 128 MiB **成功**、零物化页；写后恰 4096 字节。
- [x] **A-7** **非 RAM** 的护栏行为**逐条不变**：`map_rom(base, 32 MiB, …)` ⇒ **`E_BUS-09`**；**外设窗口**超 cap ⇒ **`E_BUS-09`**（既有断言 `the_cap_also_guards_the_peripheral_window` **一条不许删**）
      > 证据：`non_ram_regions_keep_the_declared_size_cap` ——
      > ROM 32 MiB ⇒ `E_BUS-09`；外设窗口超 cap ⇒ `E_BUS-09`；
      > 既有断言 `oversized_rom_region_is_rejected_before_allocating` 与
      > `the_cap_also_guards_the_peripheral_window` **一条没删**。
- [x] **A-8** `export_pages` 只导出**写过的页**：写 1 页 ⇒ 恰好 1 条、`base` **落在 region 的页网格上**、`bytes.len() == 4096`；不写 ⇒ **0 条**
      > 证据：`export_pages_carries_only_the_materialized_four_kib_pages` ——
      > 不写 ⇒ **0 条**；写 1 页 ⇒ 恰好 1 条、base 落在页网格上、`bytes.len() == 4096`；
      > 写两页不相邻 ⇒ **2 条**。
- [x] **A-9** `load_pages` 仍是**两阶段**：一个越界页 ⇒ **整批拒收**且 RAM **原封不动**
      > 证据：`load_pages_rejects_the_whole_batch_and_leaves_the_ram_untouched` ——
      > 越界页 ⇒ `E_BUS-11`、原值读回不变、页数不变。
      > 另两条 C-3 细则：`load_pages_rejects_a_page_off_the_regions_page_grid`、
      > `load_pages_over_the_physical_occupancy_cap_is_rejected`。
- [x] **A-10** **D-13**：`Region` / `RamBackend` / `RomBackend` 的字段**不再是 `pub`**
      > 用**源码级**守卫断言（扫源码文本），不是「编译不过」这种间接证据。
      > 证据：`region_and_backend_fields_are_not_public_anymore` ——
      > 扫源文本，`pub base:` / `pub size:` / `pub kind:` / `pub target:` / `pub bytes:` **零命中**
      > + 只读访问器 `Region::base()` / `size()` / `kind()` 与 `RomBackend::bytes()` 存在。
- [x] **A-11** 4 092 处的 4 字节写落在**同一页**内（物化 1 页）；⚠️ **并且** 4 095 处的 4 字节写**正确地跨两页**（物化 2 页、两个字节值都对）
      > 证据：`a_four_byte_write_at_4092_stays_inside_one_page` —— 物化 1 页。
      > ⚠️ 规格这条在施工中被**实测订正**（见 §1 裁决 A）：原文那条「一次访问永不跨页」的
      > 算式**不成立**（`off == 4095` 时 4 字节写会跨页）⇒ 实现按「**最多跨两页**」写，
      > **不依赖**那个快捷假设 ⇒ 跨页那一半也正确。
- [x] **A-12** 既有断言**未被删改**：总线统计（`reads` / `ram_hits` / `misses` / `writes`）与改动前**逐条相同**
      > 证据：`the_bus_statistics_are_unchanged_by_the_lazy_pages` ——
      > reads=2 / writes=1 / ram_hits=2 / misses=1 / unmapped_splits=1 / unaligned_splits=0；
      > 既有统计断言**一条没删**。

### 2.2 Lane B · 契约 + 宿主 + 外设（B-1 ~ B-9）

- [x] **B-1** `SESSION_SNAPSHOT_SCHEMA_VERSION == 3`；**旧载荷（version = 2）⇒ `E-SNAP-05`** 显式拒收
      > 证据：`the_session_snapshot_schema_version_is_three`、
      > `a_version_two_session_payload_is_rejected_with_e_snap_05`、
      > `a_version_two_checkpoint_meta_is_rejected_with_e_snap_05`。
      > ⚠️ 既有那条 `the_session_snapshot_schema_version_is_two` **改名**为
      > `..._is_three` 并改断言 3 ⇒ **没删**。
- [x] **B-2** 快照往返：写过的页**逐字节**相同 + **没写过的页仍读回 0**
      > 证据：`crates/kernel/tests/v026_memory_map.rs` 的
      > `a_hand_built_page_snapshot_restores_byte_for_byte`、
      > `a_hand_built_page_snapshot_survives_the_host_round_trip`。
      > ⚠️ **手工构造载荷**（不用 `export_pages()`）⇒ 在新旧两棵 M-14 树上都成立。
- [x] **B-3** QemuVirt 档**空会话**（一个字没写）⇒ `mem_pages` **为空** ⇒ 整份 JSON **< 1 MiB**（基线 **258.06 MiB**）
      > 证据：`crates/kernel/tests/v026_payload_slimming.rs` 的
      > `an_untouched_qemu_virt_session_carries_no_memory_pages`；
      > 另 `one_written_page_shows_up_as_exactly_one_page`
      > （写一个字节 ⇒ 恰好 1 条、`bytes.len() == 4096`、首字节是写进去的值）。
      > ⚠️ **先断言 QemuVirt 布局**再断言载荷 ⇒ 因为 `new_with_profile` 换档失败会
      > **静默退回默认档**（不 panic）⇒ 只断言载荷小的话**降级也会绿**。
- [x] **B-4** QemuVirt 档区域数 **13 → 6**（RAM 从 8 个 16 MiB 变 1 个 128 MiB）
      > 证据：`the_qemuvirt_profile_maps_one_128_mib_ram_region`、
      > `the_default_profile_keeps_its_own_region_layout`、
      > `the_qemuvirt_profile_raises_the_per_region_guardrail_budget`。
      > ⚠️ 断言**不只数区域条数**：同时落在「存在 `base == 0x8000_0000` 且
      > `size == 128 MiB` 的 RAM 区域」与「没有静默降级」上。
- [x] **B-5** TX 捕获环进快照：往 THR 写 3 字节 ⇒ 快照里 `tx_ring` 有 **3** 字节、`tx == tx_ring[0]`；还原后 `drain_tx()` 拿回**同样 3 字节**（逐字节）
      > 证据：`the_tx_capture_ring_travels_into_the_session_snapshot`、
      > `the_tx_capture_ring_survives_a_snapshot_round_trip`、
      > `the_tx_capture_ring_travels_into_the_session_state`、
      > `restoring_a_uart_state_puts_the_whole_tx_ring_back`。
- [x] **B-6** `tx_dropped` 进快照；溢出丢弃**看得见**（不是静默）
      > 证据：`the_dropped_byte_counter_travels_into_the_session_snapshot`、
      > `the_dropped_byte_counter_survives_a_snapshot_round_trip`、
      > `the_dropped_byte_counter_travels_into_the_session_state`、
      > `the_dropped_byte_counter_is_overwritten_not_accumulated_on_restore`。
- [x] **B-7** 契约层 `validate()` **拒掉** `tx != tx_ring[0]` 的载荷（与 `rx` / `rx_queue` 不变式同构）
      > 证据：`a_uart_state_whose_tx_head_disagrees_with_its_ring_is_rejected`、
      > `a_uart_state_with_an_empty_ring_must_have_no_tx_head`（+2 条反面放行）。
      > ⚠️ 契约层那条不变式编号是 **8c**（与既有的 **8b** `rx`/`rx_queue` 同形）。
- [x] **B-8** **osz 对拍仍 `VERDICT MATCH`**（内存图从 8 区域变 1 区域后，guest 看到的地址空间不变）
      > 证据：`tools/boot-osz.sh` 实测 **VERDICT MATCH** ——
      > 两侧 console 字节流 **31 字节逐字节相同**。
- [x] **B-9** **黄金语料 68/68** 仍逐例一致（Default 档未受影响）
      > 证据：`tools/golden-run.sh --self-check` 与 `--keep-going` 全绿，
      > 语料 **68 例**逐例判决一致（mismatch 0 / no-verdict 0 / build-failed 0）。

---

## §3 门禁

### 3.1 常规八项（与历批相同）

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --no-fail-fast --lib --tests`
4. `tools/check-front.sh`
5. `tools/golden-run.sh --self-check`
6. `tools/golden-run.sh --keep-going`
7. `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
8. `node tools/browser-verify.mjs`

（另：**`tools/boot-osz.sh`** —— 与 Spike 逐字节对拍，本批因为动了内存图，**必须跑**。）

### 3.2 本批专属

| # | 门禁 | 覆盖 |
| --- | --- | --- |
| **S-1** | **变异自检**：每条变异**能编译**且**按期望变红**；每条 lane 至少一条**对照**（只改注释）**必须 0 红** | A / B 全部 |
| **S-2** | **载荷瘦身实测**：QemuVirt 档空会话快照 JSON **< 1 MiB**（基线 258.06 MiB） | C-5 / B-3 |
| **S-3** | **osz 对拍 `VERDICT MATCH`** + **黄金 68/68** | B-8 / B-9 / G |

**S-1 的变异清单（至少这六条，每条都要有完整红集记录）**：

| # | 变异 | 期望变红 |
| --- | --- | --- |
| ① | 把「读没写过的页回 0」改成回 **1** | A-1 / A-4 |
| ② | 把写时分配改成**整 region 急分配** | A-1 / A-3 / B-3 |
| ③ | **去掉护栏**（占用上限不检查） | A-5 / A-7 |
| ④ | 把「登记」与「物理页」**合成一层**（没登记的地址读回 0 而不是陷阱） | A-2 |
| ⑤ | TX 环**漏出**快照（`tx_ring` 不填） | B-5 / B-7 |
| ⑥ | **只改注释**（对照） | **0 红** |

**失败项数必须为 0。**

### 3.3 实测结果（v0.26 收口）

**门禁失败项数 = 0。**

| 门禁 | 实测 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过（首轮红过一次：统筹方那两个新文件有格式细节 ⇒ `cargo fmt --all` 修好后重跑通过） |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过（**0 warning**） |
| `cargo test --workspace --no-fail-fast --lib --tests` | **1 292** 条全绿（契约 lib **140** + kernel lib **1 001** + 集成 **134** + `osz-boot` runner **3** + golden runner **14**；基线 1 254 ⇒ **+38**） |
| `tools/check-front.sh` | **312 用例 / 57 suite**，**0 红**（未变） |
| `tools/golden-run.sh --self-check` | 通过（三个金丝雀全 ok） |
| `tools/golden-run.sh --keep-going` | **68/68** 逐例一致（mismatch 0 / no-verdict 0 / build-failed 0） |
| `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | 通过 |
| `node tools/browser-verify.mjs`（真 Chrome） | 通过 |
| `tools/boot-osz.sh`（与 Spike 逐字节对拍） | **VERDICT MATCH** —— 两侧 console 字节流 **31 字节逐字节相同** |

**本批专属三条**：

- **S-1**（变异自检：每条变异能编译且按期望变红；每条 lane 至少一条对照 0 红）✅ ——
  **Lane A 自测 4 条变异 + 1 条对照**（红 5 / 10 / 4 / 7 条，对照 **0** 条）；
  **Lane B 自测 5 条变异 + 1 条对照**（红 12 / 4 / 2 / 2 条，对照 **0** 条）；
  **统筹方另独立复验 2 条 + 1 条对照**：
  ① 「读没写过的页回 0」改成回 **1** ⇒ **5 红**
  （`reading_unwritten_ram_returns_zero_and_materializes_no_page` · `writing_two_non_adjacent_pages_materializes_two_pages` ·
  `export_map_then_load_map_round_trips_the_topology` · `load_map_rebuilds_the_ram_region_and_zeroes_its_content` ·
  `load_pages_writes_a_page_at_its_own_offset_inside_the_region`）；
  ② 「TX 环漏出快照」（`tx_ring` 恒空）⇒ **8 红**（4 条 B-5 + 4 条既有 UART 会话状态）；
  **对照（只改注释）⇒ 0 红**（退出码 0）。
  ⚠️ 三处还原都用 `cp` + `md5` + `cmp` **逐字节**校验（`mod.rs` md5 `d1b9bc04…` · `uart.rs` md5 `abdf9cb8…`）⇒ **IDENTICAL**，**全程未用 `git checkout -- <file>`**；无 124 / HANG。
- **S-2**（载荷瘦身实测：QemuVirt 档空会话快照 JSON **< 1 MiB**）✅ ——
  `an_untouched_qemu_virt_session_carries_no_memory_pages` 实测通过（`mem_pages` **为空**、整份 JSON **< 1 MiB**），
  另 `one_written_page_shows_up_as_exactly_one_page` 实测**恰好 1 条**、`bytes.len() == 4096` ⇒
  相对台账基线 **258.06 MiB** ⇒ **载荷砍掉两个数量级**。
- **S-3**（osz 对拍 `VERDICT MATCH` + 黄金 68/68）✅ —— 详见上表两行。

---

## §4 破坏性变更清单（收口时逐条核对）

| # | 变更 | 影响面 | 处置 |
| --- | --- | --- | --- |
| 1 | `SESSION_SNAPSHOT_SCHEMA_VERSION` **2 → 3** | 旧会话快照载荷 | **E_SNAP-05 显式拒收**（既有机制） |
| 2 | `mem_pages` 语义：**只装写过的页** | 快照体积**大幅缩小**（QemuVirt 档 258.06 MiB → < 1 MiB） | 预期行为 |
| 3 | `UartStateDto` 新增 `tx_ring` / `tx_dropped` | 契约 DTO 形状 | 随 schema 3 一起生效 |
| 4 | `map_ram` 的**声明大小**不再受 16 MiB 限制 | 宿主与工具链 | 预期行为（物理占用仍受限） |
| 5 | `E_BUS_09_TOO_LARGE` 的**被测量**：**RAM** 从声明大小变成**物理占用**（ROM 与外设窗口**不变**，仍按声明大小） | 既有断言 | **RAM 那几条**逐条复核并更新（**判据不靠 message 文本**）；**非 RAM 的断言一条不许动** |
| 6 | `Region` / `RamBackend` / `RomBackend` 字段**不再 `pub`** | crate 公开 API | 实测：模块外零使用 ⇒ 无外部影响 |
| 7 | `MEM_PAGE_CHUNK_BYTES`（64 KiB）**退役** | 模块内 | 换 `MEM_PAGE_BYTES`（4 KiB） |
| 8 | QemuVirt 档区域数 **13 → 6** | 会话快照的 `m14_map` | 预期行为（地址空间不变） |

---

## §5 与台账的关系（收口时回填）

| 条目 | 本批做什么 | 闭环口径 |
| --- | --- | --- |
| **D-123**（内存图通用化） | **A 批**（懒分配 + 护栏转换 + 分层）落地；**B 批**（描述格式）留 `STAGE-12` | **部分闭环**（A 闭环、B 未做）⇒ 按「闭环不回填」口径仍计入未闭环 |
| **D-13**（字段可见性） | 四个字段收紧 + `RamBackend.bytes` 消失 | **闭环**；同时**订正**台账里「要改 `m06_load_to_bus.rs` 与 `renderMapTable`」那句（实测不成立） |
| **D-10**（每区域上限护栏） | 语义转换（声明 → 物理占用），**不删** | **仍然成立**（语义变了，护栏在）⇒ 记一条**修订**而不是闭环 |
| （新）TX 捕获环进快照 | 裁决 E | 随 A 批登记 |

⚠️ **口径提醒**：本批**不新增** D-xxx 条目到「发现」统计里；**D-123** 是「部分闭环」⇒ 按台账既有的「闭环不回填」规则，**真实未闭环数**的减法里**不加**它。
