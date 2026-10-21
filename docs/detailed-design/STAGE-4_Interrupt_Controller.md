# STAGE-4 中断控制器落地规格（`M-16`）

> **状态**：**冻结**（本批实现必须逐条照此落地；实现与本文件冲突时，**先改本文件**再改实现）。
> **批次**：v0.19「M-16 中断控制器落地批」。
> **权威上位**：`docs/detailed-design/M-16_InterruptController.html`（LLD v1.4，本批更新为 v1.5）·
> `docs/architecture/RISC-V_Architecture_Design.html`（AD v2.6.2）· `docs/module-design-spec/RISC-V_Module_Design_Specification.html`。
> **台账**：`docs/reviews/REVIEWS.md` §1.2（**D-26 / D-59 / D-63 / D-64 / D-72 / D-74**）。
>
> **前置**：`STAGE-2_Contract_V2.md`（契约层 v2 冻结规格）· `STAGE-3_Session_Snapshot.md`（会话快照六段载荷）。

---

## 1. 目标与范围

### 1.1 目标

把 **M-16 中断控制器**从 **stub**（168 行 / 6 测试，无 CSR 管道）变成**有生产者、有消费者、可端到端验证**的模块，并**一次闭环**围着它转的六条台账欠账。

**完成的形状**（一句话）：

```
外设 request(line) → 挂起队列 → 量子边界采样（拉 mie / mstatus.MIE）
  → 优先级仲裁 → 登记交付 → M-13 取指边界拉取
  → 写 mepc / mcause / mtval、跳 mtvec → handler → clear → 恢复执行
```

### 1.2 裁决（实现必须照此；每条都有**理由**，不是「这么写就这样写」）

#### 裁决 A —— CSR 存储用**通用 CSR 文件**，不加专用字段 → **契约层不升 `schema_version`**

`SimState.csrs: Vec<(u16, u32)>`（契约层**既有**字段）承载全部 CSR 值。`mtval` = CSR `0x343`，**不是** `SimState` 的新字段。

**理由**：

1. 台账 **D-64** 的字面建议是「`SimState` 增 mtval 字段」，但它写于「CSR 管道尚未接线」之时。按**今天**的事实重判：契约层**已经有**一个通用的 CSR 通道，且它**随快照落盘**（`EngineSnapshotDto.state.csrs` → 会话载荷段 1）——**专用字段要的那件事，通用通道已经在做了**。
2. 加专用字段 = 在契约层刚升到 v2（`3126f54`）之后**马上**再塞一个**专用**词汇 → 契约层同时存在「CSR 编号空间」与「平台状态字段」两套词汇，**一个约束两个落点**。
3. 不加字段 ⇒ **不需要 `schema_version = 3`** ⇒ **前端 `front/lib/message.js` 的 JS 镜像不需要改**（§8 文档关系表里这一格因此为空）。

**代价（必须承认）**：`csrs` 是 `Vec<(u16,u32)>` 的**线性扫描**，CSR 访问是 O(n)。n ≤ 16，可接受；**将来**要换 `BTreeMap` 是一次**契约层**变更（另批）。

#### 裁决 B —— `TrapCause` 拆成**三个**变体，`MisalignedAccess` **移除**

| 变体 | mcause | 触发点 |
| --- | --- | --- |
| `InstructionAddressMisaligned` | **0** | 取指地址未对齐 |
| `LoadAddressMisaligned` | **4** | 读访存地址未对齐 |
| `StoreAddressMisaligned` | **6** | 写访存地址未对齐 |

**理由**：RISC-V 手册的 mcause 异常码 **0 / 4 / 6** 是三个**不同**的码。实现侧今天**一个** `MisalignedAccess` 变体**同时**覆盖取指未对齐、读未对齐、写未对齐 —— 台账 **D-63** 说的就是这件事（「取指未对齐与访存未对齐共用它（mcause 4/6）」）。

**这是内核 crate 内部的 API 变更，不是契约层变更**（`TrapCause` 在 `crates/kernel/src/m13_iss/executor.rs`，不在 `crates/contract/`）—— 契约层「公开 API 只增不减」的约束**不适用**。全仓消费点只有 M-13 内部与 M-17，**同批改完**。

**顺带**：`TrapCause::IntegerDivideByZero`（台账 **D-66** 记的「全仓从未被构造」的死变体）**本批不动** —— 它是**公开 API 移除**（要决策），与本批无关；但本批的 `TrapCause` 改动**不得**顺手把它删掉（**混进两件事 = 判别式会被稀释**）。

#### 裁决 C —— `pause()` 语义 = **下一条指令边界停**；`StepOutcome::HostPause` **删除**

- `pause()` 的语义定为「**在下一条指令边界停下**」（不是「立刻停下」—— 本平台的宿主调用是**同步**的，宿主**不可能**在一条指令执行的中途插进来，所以「立刻」是**做不到**的语义，写进文档就是骗人）。
- 实现落点：**M-17 层**的 `pause_requested` 标志 —— `pause()` 置位，`step_n` / `continue_until_event` 在**指令边界**查它，命中就停下（`HaltReason::UserPause`）。
- `StepOutcome::HostPause` **删除**：它是 **M-13 的**枚举，而 M-13 **不可能**知道宿主要暂停 —— 一个「M-13 产生的宿主暂停」在架构上就是错的。台账 **D-72** 的处置是「**要么变成活分支、要么被删**」→ **删**。

**这条同时闭环 D-59**（`pause()` 的语义要定）。

#### 裁决 D —— 使能的**唯一真相**是 CSR `mie`；仲裁条件 = 挂起 ∧ `mie` ∧ `mstatus.MIE`

- **中断线编号**（本平台 line 空间）：

  | line | 名称 | `mie` 位号 | 优先级 |
  | --- | --- | --- | --- |
  | 1 | UART rx-ready | **11**（`MEIE`） | 1 |
  | 2 | UART tx-empty | **11**（`MEIE`） | 1 |
  | 3 | timer-compare | **7**（`MTIE`） | 2（最高） |

- **两个位号空间**（line 空间 1/2/3 与 RISC-V `mie` 位号 11/7）**必须显式映射**（`mie_bit_for_line`），**不得**混用。映射表是**代码里的唯一真相**，两处（M-16 刷新使能、M-13 写 `mip`）都从它取。
- **M-16 内部的 `enabled_mask: u32` 按 line 索引**（bit L = line L 使能）；**量子边界**从 CSR `mie` 刷新：`enabled_mask` 的 bit L := `mie` 的 bit `mie_bit_for_line(L)`。**没有** mie 位映射的 line（未来扩展）刷新时**保持原值** —— **不静默清零**（清零 = 静默关中断）。
- **全局使能**：`mstatus.MIE`（bit **3**）。仲裁要求它为 1。
- **交付前**的两层判定（LLD 评审 16-10）：per-line `enabled_mask` **AND** `mstatus.MIE`。
- ⚠️ **与真实 RISC-V 的差异**（`mie` 用平台 line 位号而非标准 `MEIE`/`MTIE` 位）→ 登记进 **M-16 LLD §3.5 差异对照表**。

#### 裁决 E —— CLINT 的 `mtime` / `mtimecmp` **MMIO 通路推迟到 P1**；本批只做**控制器内部**的定时器中断

LLD §3.4 要求固件经 **M-14 MMIO 区间**（CLINT 语义）访问 `mtime` / `mtimecmp`。本批**不**做，理由：

1. 落它要**同时**动 **M-14 的区域类型**（新增 `RegionType::Clint` + 一条派发路径）与**契约层的区域 kind**（`M14MapDto.region.kind` 现在是 `String`，但 `SessionSnapshotDto::validate()` 按 `"ram"` / `"rom"` / `"peripheral"` 分类做检查）—— **契约层刚升 v2 不到一批**，再动它会把本批从「落地一个模块」拉成「改会话载荷的形状」。
2. **固件侧今天没有消费者**（demo 固件不写 `mtimecmp`）→ MMIO 通路**今天没有活影响** —— 与 D-64 当初「没有生产者就不加字段」的判断**同型**。
3. **定时器中断的语义本批照样闭环**：`mtime` 随虚拟时间推进、`mtimecmp` 由**宿主 API** 设置、`mtime ≥ mtimecmp` 时按**电平**语义挂起 line 3 → **仲裁 → 交付 → 陷入**的整条链**可端到端验证**。缺的只是「固件自己写 `mtimecmp`」这一步。

**登记为台账 **D-82**（P1）**：CLINT MMIO 通路（`mtime`/`mtimecmp` 经 M-14 区域暴露给固件）。

#### 裁决 F —— `M16IntcDto` 扩展用 `#[serde(default)]` → 旧载荷仍可加载，且**语义正确**

- 会话载荷段 5（`M16IntcDto`）从 `{ raised }` 扩到 `{ raised, pending, priorities, enabled_mask, active_irq, mtime_ps, mtimecmp_ps }`。
- **新增字段全部 `#[serde(default)]`** → **旧载荷**（stub 时代捕获的「一个中断都没有」的会话）**仍能反序列化**，且默认值（`pending` 空 / `enabled_mask` 0 / `active_irq` 无 / `mtime` 0）**正是**那个时代的真实状态 —— **不只是「能读」，是「读出来是对的」**。
- ⇒ **会话载荷 `schema_version` 保持 1**（`SESSION_SNAPSHOT_SCHEMA_VERSION`）—— 加默认值的字段是**加法**，且旧载荷语义正确，不是破坏性变更。
- ⚠️ **虚拟时间字段走十进制字符串**（契约 v2 §2.2）：`mtime_ps` / `mtimecmp_ps` 用 `#[serde(with = "crate::numstr::u64_decimal")]`。**不得**用 `u64` JSON 数字（2⁵³ ps ≈ 2.5 小时即可越界）。

#### 裁决 G —— 平台自定义 CSR `0xBFF0`（`irq_clear`）让 guest 能清除中断线

- 真实 M 模式的 `mip` 对 `MEIP` / `MTIP` 是**只读**的 → guest **不能**经 `mip` 清中断。
- 本平台提供 **`irq_clear` = CSR `0xBFF0`**：**写入一个线号位掩码**（bit L = 清除 line L）→ M-16 清除对应挂起项并复位 `active_irq`。
- 这是**平台自定义 CSR**（非 RISC-V 标准）→ 登记进 **M-16 LLD §3.5 差异对照表**。
- **CSR 地址 `0xBFF0` 的低两位为 0**（对齐要求满足），落在 `0xB000..0xBFFF` 的**只读**区间（bits[11:10] == `0b11`）—— ⚠️ **所以自定义 CSR 必须放在一个**可写**区间**：`0xBFF0` 的 bits[11:10] = `0b11` ⇒ 按标准规则**只读**。**修正**：本平台的**只读规则**按 RISC-V 标准（地址 bits[11:10] == `0b11` ⇒ 写被忽略），因此 `irq_clear` **不能用 `0xBFF0`**。

  **最终裁定**：`irq_clear` = **CSR `0x7C0`**。它落在 RISC-V 规定的**可写自定义区** `0x7C0..0x7FF` 里（地址 bit 11 = 0 ⇒ 可写），**不与任何标准 CSR 冲突**（RV32 未占用 `0x7C0`）。

  ⚠️ **判别式**：「`irq_clear` 的写**真的**清掉了挂起项」必须有一条**能变红**的用例；只断言「CSR 写成功」是**恒真断言**。

#### 裁决 H —— **没有**「CSR 地址必须 4 字节对齐」这条规则（本条是被**自己的测试**改出来的）

本规格的第一稿写过一条：「CSR 地址低两位非零 ⇒ **非法指令**（cause 2）」。**实现时的契约层用例当场把它打红**：

```
csr 0x305 must be aligned
```

**反例**：`mtvec` 的**标准编号就是 `0x305`** —— 低两位是 `0b01`。也就是说，那条规则会让平台**自己的向量表 CSR 变成非法指令**，也就是**任何中断都跳不进 handler**。

**查证**：RISC-V 的 CSR 地址空间按 **bit 11** 分成「可写 / 只读」四个区（§2.1 的表），**没有**任何低位对齐要求；标准 CSR 编号里 `0x305`（`mtvec`）、`0x306`（`mcounteren`）的低两位都不是 0。

**裁定**：**删掉**那条对齐规则。合法性只由「地址 bit 11 ⇒ 只读」与「平台白名单（`mip`）」两条决定。

⚠️ **方法论沉淀**：本条是「**规格也会写错**」的一个实例 —— 规格不是圣经，**能被一条反例打红的规格就该被打红**。反例的形状是「**平台自己要用的那个值**」（`mtvec` 0x305）**违反了规格自己写的规则**」。⇒ 本批的判别式里必须有一条**把标准编号钉死**的用例（`crates/contract/src/csr.rs::tests::mtvec_is_a_legal_csr_number`）。

### 1.3 台账闭环清单（本批负责闭环的）

| 条目 | 内容 | 本批怎么闭环 |
| --- | --- | --- |
| **D-26** | M-18 的 ch=2（中断脉冲）无生产者 | M-16 的 `request()` 每次登记挂起时**调一次脉冲回调** → 宿主把它接到 M-18 的 `sample_irq_pulse` |
| **D-59** | `pause()` 语义要定 | 裁决 C：下一条指令边界停 |
| **D-63** | 取指未对齐与访存未对齐共用 `TrapCause` | 裁决 B：拆三个变体（mcause 0 / 4 / 6） |
| **D-64** | `mtval` 全平台不存在 | 裁决 A：落 CSR `0x343`（通用 CSR 文件）；陷阱投递时按 cause 写入（§3.5） |
| **D-72** | `StepOutcome::HostPause` 死接口 | 裁决 C：删除该变体，pause 落到 M-17 的标志位 |
| **D-74** | M-16 模块头的「量子边界」措辞 | 按 D-57 新口径（「量子边界是**抢占点**，不是**停止条件**」）重写模块头与相关措辞 |
| **D-82**（新） | CLINT 的 `mtime`/`mtimecmp` MMIO 通路 | 裁决 E：本批不做，登记为 P1 |

**本批不做（明确留账）**：D-13（字段可见性）· D-30（M-12 独立模块级 review）· D-66（三处死接口的删/留决策）· D-75（版本协商不对称，随 P1）· D-25（UART RX/CTS 未实现）。

---

## 2. 契约层形状（`crates/contract/`）

### 2.1 CSR 编号与常量（**新增**，落 `crates/contract/src/model.rs` 或新文件 `csr.rs`）

```
mstatus = 0x300     mcause = 0x342     mip = 0x344
mie    = 0x304     mtval = 0x343     mtime = 0xBFF8
mtvec  = 0x305     mepc  = 0x341     irq_clear = 0x7C0（平台自定义）
```

| 常量 | 值 | 含义 |
| --- | --- | --- |
| `CSR_MSTATUS` | `0x300` | 全局状态 |
| `CSR_MIE` | `0x304` | 中断使能（位号 = RISC-V 标准位） |
| `CSR_MTVEC` | `0x305` | 向量表基址 + 模式 |
| `CSR_MEPC` | `0x341` | 陷阱返回地址 |
| `CSR_MCAUSE` | `0x342` | 陷阱原因 |
| `CSR_MTVAL` | `0x343` | 陷阱附加信息（**D-64**） |
| `CSR_MIP` | `0x344` | 中断挂起（**只读**） |
| `CSR_IRQ_CLEAR` | `0x7C0` | **平台自定义**：写线号位掩码清中断（裁决 G） |
| `MSTATUS_MIE_BIT` | `3` | 全局中断使能位 |
| `MIE_MEIE_BIT` | `11` | 机器外部中断使能位 |
| `MIE_MTIE_BIT` | `7` | 机器定时器中断使能位 |
| `IRQ_BASE` | `0x8000_0000` | 中断位（`mcause[31]`） |

**只读规则**（RISC-V 标准）：**地址 bit 11 置位 ⇒ 写被忽略**。

| 地址区间 | 权限 | 访问 |
| --- | --- | --- |
| `0x000..0x3FF` | 用户级 | 读 / 写 |
| `0x400..0x7FF` | 监管级 | 读 / 写（含**自定义**区 `0x7C0..0x7FF`） |
| `0x800..0xBFF` | 用户级 | **只读** |
| `0xC00..0xFFF` | 机器 / 调试 | **只读** |

⇒ `mtime`（`0xBFF8`）**只读**；`irq_clear`（`0x7C0`，落在可写的自定义区）**可写**。

⚠️ **平台规则叠加在地址规则之上**：`mip`（`0x344`）按地址规则**是可写的**（`0x344 < 0x800`），但 RISC-V 规定 `MEIP` / `MTIP` 在 M 模式下**只读**——只有平台能拉起它们，guest 自己写 `mip` **不许**伪造出一个挂起中断。⇒ `mip` 的写**被忽略**（平台规则）。

⚠️ **没有「CSR 地址必须 4 字节对齐」这条规则**——见**裁决 H**。

### 2.2 `SimState.csrs` 的访问语义

- 存储：`Vec<(u16, u32)>`，**同号 CSR 至多一条**（写入覆盖，**不追加**）。
- 读：找不到 ⇒ 返回 **0**（**不是**陷阱 —— 未实现 CSR 读 0 是 RV32 的标准行为）。
- 写：找到 ⇒ 覆盖；找不到 ⇒ **追加**（保持 **升序**插入，避免同一 CSR 出现两条）。
- 快照：`EngineSnapshotDto.state.csrs` 原样落盘 / 还原（**既有能力，不需改**）。

### 2.3 `M16IntcDto` 扩展（会话载荷 **段 5**）

```rust
pub struct M16IntcDto {
    /// 旧字段：已拉起的 IRQ 线（升序去重）—— **保留**。
    #[serde(default)]
    pub raised: Vec<u16>,
    /// 挂起队列（按 (request_time_ps, insert_seq) 升序）。
    #[serde(default)]
    pub pending: Vec<PendingIrqDto>,
    /// line → 优先级（按 line 升序）。
    #[serde(default)]
    pub priorities: Vec<IrqPriorityDto>,
    /// 使能掩码（**按 line 索引**：bit L = line L）。
    #[serde(default)]
    pub enabled_mask: u32,
    /// 当前已交付未清除的中断线（`0xFFFF` = 无）。
    #[serde(default)]
    pub active_irq: u16,
    /// 定时器计数（虚拟时间等价值）—— **十进制字符串**。
    #[serde(default, with = "crate::numstr::u64_decimal")]
    pub mtime_ps: u64,
    /// 定时器比较值 —— **十进制字符串**。
    #[serde(default, with = "crate::numstr::u64_decimal")]
    pub mtimecmp_ps: u64,
}

pub struct PendingIrqDto {
    pub line: u16,
    pub prio: u8,
    /// 挂起时刻（虚拟时间）—— **十进制字符串**。
    #[serde(with = "crate::numstr::u64_decimal")]
    pub request_time_ps: u64,
    /// 插入序（确定性排序的次键）。
    pub insert_seq: u64,
    /// 触发模式：`0` = 电平，`1` = 边沿。
    pub trigger_type: u8,
    /// 请求源类型：`"uart"` / `"gpio"` / `"timer"` / `"platform"`。
    pub source_type: String,
}

pub struct IrqPriorityDto {
    pub line: u16,
    pub prio: u8,
}
```

⚠️ `with = "crate::numstr::u64_decimal"` 与 `default` **同时**用时，`default` 生效于**字段缺失**（`#[serde(default)]`），`with` 负责**序列化/反序列化** —— 两者**不冲突**。

### 2.4 `SessionSnapshotDto::validate()` 的新增检查（段 5）

⚠️ **编号从 11 起**——STAGE-3 §5 的 1~10 是**既有**的十项，本批的检查**追加**在后面（不重编号：重编号会让 STAGE-3 §5 的表与实现对不上）。

| # | 检查 | 拒绝码 |
| --- | --- | --- |
| 11 | `pending`：**同一条 line 不重复**（同线去重是 M-16 的不变式）**且**按 `(request_time_ps, insert_seq)` **严格升序** | `E_SNAP_06` |
| 12 | `priorities` 的 `line` **升序且不重复** | `E_SNAP_06` |
| 13 | `active_irq` 要么是 `NO_ACTIVE_IRQ`（无），要么**必须**出现在 `pending` 里 | `E_SNAP_06` |
| 14 | `pending` 里的每条 line 都**必须**在 `raised` 里（不变式是**单向**的：`pending ⊆ raised`） | `E_SNAP_06` |

⚠️ **旧载荷必须仍然通过**：`pending` 空 + `active_irq` = `NO_ACTIVE_IRQ` ⇒ 检查 11~14 **全部通过**。**默认 `active_irq` 必须是 `NO_ACTIVE_IRQ`（`0xFFFF`）**，不能是 0 —— 否则旧载荷会「有一个 active 的 line 0」而不通过检查 13。⇒ `active_irq` 的 `Default` 要**专门**实现（**不能**用 `#[derive(Default)]` 的 0）。这是本批**最容易踩**的一个坑。

⚠️ **检查 14 的方向是单向的**：`pending ⊆ raised`，**不是**相等。反向（`raised ⊆ pending`）**不成立**——stub 时代的载荷就是「拉起了若干行、队列是空的」，那是那个时代的**真实状态**，必须能过校验。

---

## 3. 行为规格（M-16 本体）

### 3.1 数据结构

```rust
pub struct InterruptController {
    /// 挂起队列（按 (request_time_ps, insert_seq) 升序）。
    pending: Vec<PendingIrq>,
    /// 插入序计数器（确定性排序的次键）。
    insert_seq: u64,
    /// line → 优先级。
    priorities: BTreeMap<u16, u8>,
    /// 使能掩码（按 line 索引）。
    enabled_mask: u32,
    /// 当前已交付未清除的中断线（`0xFFFF` = 无）。
    active_irq: u16,
    /// 已拉起的线（level 状态；边沿线用完即清）。
    raised: BTreeSet<u16>,
    /// 定时器计数 / 比较值。
    mtime_ps: u64,
    mtimecmp_ps: u64,
    /// 交付交接结构（LLD 评审 16-4：M-13 拉取式）。
    pending_delivery: Option<PendingIntr>,
    /// 中断脉冲回调（→ M-18 `sample_irq_pulse`）。
    pulse_sink: Option<Box<dyn FnMut(u64, u16)>>,
}
```

**确定性**：挂起队列的排序键是 **(request_time_ps, insert_seq)**，**不依赖宿主时钟**、**不依赖 HashMap 迭代序**（`BTreeMap` / `Vec` 都有稳定序）。

### 3.2 `request(line, now_ps, source_type, trigger_type)`

1. **同线去重**（LLD 评审 16-2）：`pending` 里已有该 line ⇒ **直接返回**，**不追加**（不堆积）。
2. 登记挂起项：`(line, prio = priorities[line] 或默认, request_time_ps = now_ps, insert_seq = ++insert_seq, trigger_type, source_type)`。
3. 插到队列里**保持升序**（`(request_time_ps, insert_seq)`）。
4. **脉冲回调**：`pulse_sink(now_ps, line)` → M-18（**D-26**）。
5. **电平线**（timer-compare）：`raised.insert(line)`；**边沿线**（rx/tx）：`raised` 只在 `clear` 时清。

### 3.3 `clear(line)`

- 移除 `pending` 里该 line 的挂起项。
- `raised.remove(line)`。
- 若 `active_irq == line` ⇒ `active_irq = 0xFFFF`。

### 3.4 量子边界采样与仲裁（`poll(now_ps, mie, mstatus)`）

**顺序**（每一步都必须可单独判别）：

1. **刷新使能**：对每个**已知** line（有 `mie_bit_for_line` 映射的）⇒ `enabled_mask` 的 bit L := `mie` 的对应位。**没有**映射的 line ⇒ **保持原值**（不静默清零）。
2. **定时器轮询**：`mtime_ps := now_ps`；若 `mtime_ps >= mtimecmp_ps` 且 `mtimecmp_ps != 0` ⇒ 按**电平**语义 `request(timer-compare)`（**同线去重** ⇒ 不会堆积）。
3. **单级嵌套防护**（LLD 评审 16-6）：`active_irq != 0xFFFF` ⇒ **直接返回 `None`**（不覆盖已交付的中断）。
4. **候选筛选**：`pending` 里**挂起 ∧ `enabled_mask` 的 bit L 为 1`** 的项。
5. **全局使能**：`mstatus.MIE` 为 0 ⇒ **返回 `None`**（保持挂起，不丢）。
6. **仲裁**：取**最高优先级**；同优先级按 `(request_time_ps, insert_seq)` **升序**取第一个（确定性）。
7. **登记交付**：`pending_delivery = Some(PendingIntr { line, cause: IRQ_BASE | line })`；`active_irq = line`。

### 3.5 交付与陷入（M-13 侧）

**拉取式**（LLD 评审 16-4）：M-13 在**取指边界**查 `pending_intr()`，有就 `take_intr()` 并执行陷入。

**陷入入口统一走 M-13 的一个公开函数**（异常路径与中断路径**共用**）：

```rust
pub fn enter_trap(state: &mut SimState, cause: u32, mtval: u32) -> u32
```

它做四件事（顺序固定）：

1. `mepc := state.pc`
2. `mcause := cause`
3. `mtval := mtval`
4. `mstatus`：先 `MPIE := MIE`，再 `MIE := 0`（**关中断**，单级语义）
5. `state.pc := mtvec`（**直接模式**：base；**向量模式**：base + 4 × (cause & 0x7)）

**`mtval` 的取值规则**（**D-64** 的落点）：

| cause | `mtval` |
| --- | --- |
| 非法指令（2） | 32 位指令：**原指令字**；**压缩指令**：**原码**（`Decoded::compressed_raw`） |
| 取指地址未对齐（0） | **故障地址** |
| 读/写地址未对齐（4 / 6） | **故障地址** |
| ECALL（11）/ EBREAK（3） | **0** |
| 中断（`IRQ_BASE｜line`） | **0**（中断没有故障地址） |

⚠️ **压缩指令的 `mtval`**：压缩指令的 `mtval` 按 RISC-V 规范应是**16 位原码**（零扩展到 32 位）。`Decoded::compressed_raw` 已有这个信息（台账 D-64 说「信息齐备」）→ **本批把它接上**。

### 3.6 M-13 的 CSR 指令与 `mret`

| 指令 | funct3 | 语义 |
| --- | --- | --- |
| `CSRRW` | `001` | `rd = old`；`csr = rs1`（`rd == x0` ⇒ **不读**旧值） |
| `CSRRS` | `010` | `rd = old`；`rs1 != x0` ⇒ `csr |= rs1`（`rs1 == x0` ⇒ **只读**，不写） |
| `CSRRC` | `011` | `rd = old`；`rs1 != x0` ⇒ `csr &= !rs1` |
| `CSRRWI` | `101` | `rd = old`；`csr = zimm`（rs1 字段当 u5） |
| `CSRRSI` | `110` | `rd = old`；`zimm != 0` ⇒ `csr |= zimm` |
| `CSRRCI` | `111` | `rd = old`；`zimm != 0` ⇒ `csr &= !zimm` |
| `MRET` | `000` + `imm = 0x302` | `pc := mepc`；`MIE := MPIE`；`MPIE := 1` |

- **只读 CSR 的写被忽略**（地址 bit 11 置位；外加平台规则：`mip` 也只读）。
- **未知 CSR 的读 = 0**（不产生陷阱）。
- ⚠️ **没有「未对齐 CSR 地址 ⇒ 非法指令」这条规则**（裁决 H）。
- `MRET` 的编码是 `0x3020_0073` ⇒ `funct3 == 0` 且 `imm == 0x302`。⚠️ **当前 `classify` 对 `funct3 == 0` 的 SYSTEM 编码按 `imm` 分派 `Ecall`(imm=0) / `Ebreak`(imm=1)** → `MRET` 今天落到 `None`（非法指令）→ 本批**加一个 `Op::Mret`** 分支，**不动** `Ecall` / `Ebreak` 的既有分派。
- `SRET` / `WFI` / `SFENCE.VMA` ⇒ **非法指令**（MVP 只有 M 模式）。

### 3.7 `pause()`（裁决 C）

- M-17 增加 `pause_requested: bool`。
- `pause()` ⇒ `pause_requested = true` + `last_stop = Some(UserPause)`。
- `step_n` / `continue_until_event` 在**指令边界**查它 ⇒ 命中就停下。
- 批次开始时**清零**（一次 pause 只影响一个批次 —— 与 `watch_hit` 的「本批次专属」口径**同型**）。

### 3.8 定时器中断（裁决 E 的范围内部分）

- `mtime_ps` 随**虚拟时间**推进（量子边界采样时 `mtime_ps := now_ps`）。
- `mtimecmp_ps` 由**宿主 API** 设置（`set_timer_compare(mtimecmp_ps)`）。
- `mtime_ps >= mtimecmp_ps`（且 `mtimecmp_ps != 0`）⇒ 按**电平**语义挂起 line 3。
- **电平语义**：`mtimecmp` 被改写（变大）之前，该线**持续**成立 → 同线去重 ⇒ 不会堆积；改写后 ⇒ 不再成立 → `clear` 时移除。

---

## 4. 接线点（**一条线接到哪里**，逐条列明）

| # | 从 | 到 | 机制 | 落点 |
| --- | --- | --- | --- | --- |
| 1 | 外设（M-15） | M-16 | `InterruptSink::raise(line)` | 契约层**既有**端口；M-16 实现它 |
| 2 | M-12 | M-16 | **量子边界**采样回调（**M-12 内部接缝**，与 `PeripheralTicker` 同型） | `m12_engine` 新增 `InterruptPoller` 接缝；`set_interrupt_poller` 安装；M-17 装配 |
| 3 | M-16 | M-13 | **拉取式**交付（LLD 评审 16-4） | `m16_interrupt` 暴露 `pending_intr()` / `take_intr()`；M-17 的 `IssExecutor` 在取指边界查 |
| 4 | M-13 | M-16 | `irq_clear` CSR 写 ⇒ 清挂起 | M-13 的 CSR 写路径需要一个**副作用回调**；M-17 装配 |
| 5 | M-16 | M-18 | **中断脉冲**（D-26） | M-16 的 `pulse_sink` 回调；宿主接到 `ProbeRegistry::sample_irq_pulse` |
| 6 | 宿主 | M-16 | `raise_irq` / `lower_irq` / `set_timer_compare` | `kernel_host` **既有** `raise_irq` / `lower_irq` **扩语义**（现在是「直接进 `raised` 集合」） |
| 7 | 宿主 | M-16 | `clear_irq(line)`（新） | 调试器/固件清中断 |

⚠️ **接缝 #2 与 #3 的边界**：`M-12` **不依赖** M-13，**也不依赖** M-16 —— 它只认一个**回调式接缝**（与 `PeripheralTicker` / `InstructionExecutor` **同型**）。**依赖方向单向**：M-17 → M-12 / M-13 / M-16。

⚠️ **接缝 #4 的落点**：M-13 的执行器是**纯函数**（`(gpr, mem, decoded)`），它**不该**直接认识 M-16。所以 `irq_clear` 的**副作用**由 **M-17** 承担：M-17 在 CSR 写之后检查「刚写的 CSR 是不是 `irq_clear`」，是就调 `intc.clear(line)`。**M-13 只管把值写进 `csrs`**，副作用在 M-17 —— **一个约束一个落点**。

### 4.1 跨 lane 的 Rust 签名（**冻结** —— 三路施工对着这一份写）

#### M-16（`crates/kernel/src/m16_interrupt/`）对外暴露

```rust
/// 一条待交付的中断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingIntr {
    /// 中断线号。
    pub line: u16,
    /// `mcause` 的值（中断 = `IRQ_BASE | line`）。
    pub cause: u32,
}

/// 触发模式：电平。
pub const TRIGGER_LEVEL: u8 = 0;
/// 触发模式：边沿。
pub const TRIGGER_EDGE: u8 = 1;

impl InterruptController {
    pub fn new() -> Self;

    // —— 请求 / 清除 ——
    /// 登记一条挂起请求（**同线去重**）。返回 `false` 表示该线**已在**
    /// 挂起队列里（本次请求被折叠）。
    pub fn request(
        &mut self,
        line: u16,
        now_ps: u64,
        source_type: &str,
        trigger_type: u8,
    ) -> Result<bool, PlatformError>;

    /// 清除一条线：移除挂起项 + 撤销线电平 + 复位 `active_irq`。
    pub fn clear(&mut self, line: u16);

    // —— 量子边界采样与仲裁 ——
    /// 采样 + 仲裁。返回**本次登记的交付**（`None` = 本轮不交付）。
    pub fn poll(
        &mut self,
        now_ps: u64,
        mie: u32,
        mstatus: u32,
    ) -> Result<Option<PendingIntr>, PlatformError>;

    // —— 拉取式交付（评审 16-4）——
    /// 已登记但尚未被取走的交付。
    pub fn pending_intr(&self) -> Option<PendingIntr>;
    /// 取走交付（**取走即消失**）。
    pub fn take_intr(&mut self) -> Option<PendingIntr>;

    // —— 配置 ——
    pub fn set_priority(&mut self, line: u16, prio: u8);
    pub fn priority_of(&self, line: u16) -> u8;
    pub fn set_enabled_mask(&mut self, mask: u32);
    pub fn enabled_mask(&self) -> u32;
    pub fn set_timer_compare(&mut self, mtimecmp_ps: u64);
    pub fn timer_compare(&self) -> u64;
    pub fn mtime(&self) -> u64;

    // —— 观测 ——
    /// 挂起队列（规范序）。
    pub fn pending_lines(&self) -> Vec<u16>;
    pub fn active_irq(&self) -> u16;
    /// 已断言的线（升序）。
    pub fn raised_lines(&self) -> Vec<u16>;

    // —— 中断脉冲回调（D-26）——
    /// 安装「中断脉冲」回调：`FnMut(now_ps, line)`。宿主把它接到
    /// M-18 的 `sample_irq_pulse`。
    pub fn set_pulse_sink(&mut self, sink: Box<dyn FnMut(u64, u16)>);

    // —— 会话状态 ——
    pub fn export_state(&self) -> M16IntcDto;
    pub fn load_state(&mut self, state: &M16IntcDto);
}
```

#### M-13（`crates/kernel/src/m13_iss/`）对外暴露

```rust
/// 陷入的**原始** `mcause` 值（异常：小数字；中断：`IRQ_BASE | line`）。
pub fn enter_trap(state: &mut SimState, cause: u32, mtval: u32) -> u32;
```

`TrapCause` 的三个变体（**替换** `MisalignedAccess`）：
`InstructionAddressMisaligned`（0）· `LoadAddressMisaligned`（4）· `StoreAddressMisaligned`（6）。

#### M-12（`crates/kernel/src/m12_engine/`）新增接缝

```rust
/// 量子边界的**中断采样**回调（M-12 内部接缝，与 `PeripheralTicker` 同型）。
///
/// 签名：`FnMut(now_ps, &mut SimState) -> Result<(), PlatformError>`。
/// `SimState` 传进去是因为回调要从 CSR 文件里读 `mie` / `mstatus`。
pub struct InterruptPoller { /* ... */ }

impl InterruptPoller {
    pub fn new<F>(f: F) -> Self
    where F: FnMut(u64, &mut SimState) -> Result<(), PlatformError> + 'static;
    pub fn poll(&mut self, now_ps: u64, state: &mut SimState) -> Result<(), PlatformError>;
}

/// 装到引擎上（**唯一**的安装点）。
pub fn set_interrupt_poller(&mut self, poller: InterruptPoller);
```

⚠️ **接缝在 M-12 内部**，**不是契约端口**（与 `PeripheralTicker` / `InstructionExecutor` 同型）—— 依赖方向单向：M-17 → M-12 / M-13 / M-16，M-12 **不依赖** M-16。

#### M-17（`crates/kernel/src/m17_backend/`）接线

```rust
impl<'a> IssExecutor<'a> {
    /// 多带两个引用：中断控制器 + 脉冲/副作用回调。
    pub fn new(
        iss: &'a mut Iss,
        bus: &'a mut Bus,
        breakpoints: &'a BreakpointTable,
        watch_hit: Rc<RefCell<Option<Watchpoint>>>,
        intc: &'a mut dyn InterruptPort,     // ← 新增
    ) -> Self;
}

/// M-17 需要的**中断控制器**能力（**内核内部** trait，由 M-16 实现）。
pub trait InterruptPort {
    fn pending_intr(&self) -> Option<PendingIntr>;
    fn take_intr(&mut self) -> Option<PendingIntr>;
    fn clear(&mut self, line: u16);
}
```

⚠️ `InterruptPort` 是**内核 crate 内部**的 trait（放 `m16_interrupt` 里），**不进契约层** —— 契约层已经有 `InterruptSink`（raise / lower），再加一个契约端口就是**第二个真相**。

---

## 5. 错误码

**本批不新增错误码族**。本批涉及：

| 码 | 族 | 落点 | 何时 |
| --- | --- | --- | --- |
| `E_INT_01_CONTROLLER_BUSY` | `INT` | 宿主 | `RefCell` 争用（**既有**） |
| `E_ISS_03` | `ISS` | M-13 | 陷入处理失败（LLD §6.1「交付/陷入失败 → M-13 上报」） |

⚠️ **D-70**（LLD 错误码表只收录 6 族 / 29 码，代码实际发 11 族 / 36 码）是**文档补齐**，与本批的实现**并行但独立**（文档 lane，零文件交集）。

---

## 6. 验收

### 6.1 契约层（`crates/contract/`）

1. `M16IntcDto` 的**旧形状**（只有 `raised`）能反序列化 ⇒ 新字段取默认值。
2. `active_irq` 的默认是 `0xFFFF`（**不是 0**）。
3. `mtime_ps` / `mtimecmp_ps` 序列化出来是**十进制字符串**（不是 JSON 数字）。
4. `validate()` 拒：重复 line / 顺序错乱 / `active_irq` 不在 `pending` 里。
5. `validate()` **接受**旧形状载荷（`pending` 空 + `active_irq = 0xFFFF`）。
6. `SessionSnapshotDto` 往返（JSON → 结构 → JSON）**字段名与顺序**稳定。

### 6.2 M-16 本体

7. `request()` 同线去重（连请求两次 ⇒ 挂起队列里 **1** 条）。
8. 仲裁取**最高优先级**；同优先级按 `(request_time, insert_seq)` 取**最早**。
9. **未使能的高优先级不阻塞**已使能的低优先级（LLD 评审 16-1）。
10. `mstatus.MIE == 0` ⇒ 仲裁返回 `None`，**挂起不丢**。
11. `active_irq != 无` ⇒ 再次仲裁返回 `None`（单级嵌套防护）。
12. `clear(line)` 移除挂起项 + 复位 `active_irq`。
13. **量子边界**刷新使能：`mie` 的 bit 11 → line 1/2 的使能位；bit 7 → line 3。
14. **没有** mie 位映射的 line ⇒ 刷新时**保持原值**（不静默清零）。
15. 定时器：`mtime >= mtimecmp` ⇒ 挂起 line 3（电平语义，不堆积）。
16. `export_state()` / `load_state()` 往返 ⇒ 挂起队列 / 优先级 / 使能 / active / mtime 全部还原。
17. `load_state()` 是**整体替换**（旧的不该残留）。

### 6.3 M-13

18. 六条 CSR 指令的读写语义（含 `rd == x0` 不读、`rs1 == x0` 不写）。
19. **只读** CSR（`0x344` `mip` / `0xBFF8` `mtime`）的写**被忽略**。
20. **只读** CSR（`mip` / `mtime`）的写**被忽略**；`irq_clear`（`0x7C0`）的写**生效**。
21. 未知 CSR 的读 = **0**，**不陷阱**；`mtvec`（`0x305`，低位**不是** 0）**可读可写**。
22. `MRET`：`pc := mepc`；`MIE := MPIE`；`MPIE := 1`。
23. **取指未对齐** ⇒ cause **0**；**读未对齐** ⇒ cause **4**；**写未对齐** ⇒ cause **6**（**D-63**）。
24. 陷阱写 `mtval`：非法指令 ⇒ 指令字（压缩 ⇒ 原码）；未对齐 ⇒ 故障地址（**D-64**）。
25. 陷入时 `mepc := pc`、`mstatus.MIE := 0`、`MPIE := 旧 MIE`。

### 6.4 接线与端到端

26. **量子边界**才仲裁（一条指令执行中间**不**仲裁）。
27. 中断交付 ⇒ `pc` 跳到 `mtvec`；`mcause == 0x8000_0000 | line`。
28. `irq_clear` CSR 写 ⇒ 挂起项被清、`active_irq` 复位（**不是**恒真断言）。
29. `MRET` 之后回到 `mepc`，且能**再次**收到同一中断（清 → 再请求 → 再交付）。
30. **脉冲探针**：一次 `request()` ⇒ M-18 收到**恰好一条** `irq_pulse` 边沿（**D-26**）。
31. `pause()` ⇒ 批次在**下一条指令边界**停（**D-59**）。
32. **黄金对拍**：`tools/golden-run.sh --keep-going` **51/51** 不变（语料**无** CSR 指令 ⇒ CSR 实现**不得**改变既有判决）。

### 6.5 门禁

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast          # 全绿
tools/check-front.sh                            # 前端零改动 ⇒ 用例数不变
tools/golden-run.sh --keep-going                # 51/51
tools/build-wasm.sh && node tools/wasm-smoke.mjs
```

---

## 7. 本批**不做**什么

1. **完整 PLIC 上下文保存 / 恢复 / 嵌套中断**（P1，LLD §1.3 预留）。
2. **软件中断（SWI）/ 外部中断细分**（P1）。
3. **多级优先级组 / 阈值**（P1）。
4. **S 模式中断**（P2）。
5. **CLINT 的 `mtime` / `mtimecmp` MMIO 通路**（裁决 E → 台账 **D-82**，P1）。
6. **UART 的 RX / CTS**（台账 D-25）⇒ line 1 / line 2 今天**没有固件侧生产者**（宿主 `raise_irq` 可以手动拉起）。
7. **M-11 符号表 / DWARF**（下一批）。
8. **D-75**（版本协商不对称，随 P1）。
9. **D-13 / D-30**（Wave 2 另批）。
10. **契约层升 `schema_version = 3`**（裁决 A / F ⇒ **不需要**）。

---

## 8. 文档关系表

| 文档 | 本批的关系 |
| --- | --- |
| `docs/detailed-design/M-16_InterruptController.html` | **LLD v1.4 → v1.5**：模块头措辞（**D-74**）· §3.5 差异表补「`mie` 用平台 line 位号」与「`irq_clear` 平台 CSR」· §3.4 标注 MMIO 通路为 P1 |
| `docs/reviews/REVIEWS.md` | §1.2 闭环 **D-26 / D-59 / D-63 / D-64 / D-72 / D-74**；新登记 **D-82**；§2.2 优先级表更新；§3.x 检视归档 |
| `docs/progress/PROGRESS.md` | v0.19.0 变更摘要 |
| `AGENTS.md` | v0.19 判据块 + M-13 / M-16 模块状态行 |
| `docs/detailed-design/STAGE-2_Contract_V2.md` | **不改**（本批**不升** `schema_version`） |
| `docs/detailed-design/STAGE-3_Session_Snapshot.md` | **不改**（段 5 的形状扩了，但载荷 `schema_version` 不变、**旧载荷仍可加载**） |
| `docs/architecture/RISC-V_Architecture_Design.html` | §3.4 表16 错误码族补齐（**D-70**，文档 lane） |
| `front/lib/message.js` | **不改**（契约层没升版） |
