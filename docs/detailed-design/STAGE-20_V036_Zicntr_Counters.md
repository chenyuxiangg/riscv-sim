# STAGE-20 · v0.36 · 计数器批 —— 硬件计数器 / 影子 CSR / 退休口径

> **状态**：冻结。
> **规格号**：`STAGE-20`（台账 **D-94** 的第 ① 件事）。
> **上一份**：`STAGE-19_V034_OneConstraintOneLanding_Convergence.md`（口径收敛批）。
> **本批不含**：PMP（D-94 的 ②）、S 模式（D-94 的 ③）—— 顺延为 `STAGE-21` 与再往后（裁决 O）。

---

## §0 本批的定位：D-94 的**第 ① 件事**

台账 **D-94** 的原文把「S 扩展 / PMP / 计数器缺失」记成**一条**欠账。第 18 批（v0.35 勘察批）的
`docs/reviews/V035_D94_Survey.md` 把它**拆成三件可独立的事**：

| 件 | 内容 | 体量 | 本批 |
| --- | --- | --- | --- |
| ① | **计数器**（Zicntr：`mcycle` / `minstret` + 影子 `cycle` / `instret`） | 小 | ✅ **本批** |
| ② | **PMP**（`pmpcfg0` / `pmpaddr0`） | 中 | ❌ 顺延 |
| ③ | **S 模式**（`mstatus.MPP` / `mret` 回 S / `sret` / `medeleg`） | 大 | ❌ 顺延 |

本批只做 ①。**动机不是「补一个功能」**，而是：

* 台账 `tests/golden/README.md` 的「空洞的一致」一节里，**`rv32mi/instret_overflow` 被登记为
  空洞**（原文：「the platform has **no counter at all** ⇒ `minstret` writes are ignored and it
  always reads 0 ⇒ the case's `TEST_CASE(2/3/4)` (all expecting 0) are **unconditionally
  satisfied**」）——⇒ **本批之后这一行必须出空洞清单**；
* 第 18 批实测过：**给参考侧开 `zicsr` + `zicntr` 后，全量 68 例仍然 68/68 全绿** ⇒ ⇒
  **参考侧可以安全地带上 Zicntr** ⇒ ⇒ 本批的实现有**可对拍的真判据**。

### 本批**做**什么（**五件**）

1. **契约层**：修 `is_csr_read_only` 的**地址规则 bug**；补齐计数器 CSR 的**常量**与**配对 / 影子
   映射**；把 `mcycle` / `minstret` / `mcycleh` / `minstreth` / `mcountinhibit` 列入
   `csr_is_implemented`。
2. **内核**：实现**退休指令计数**；实现 `mcycle` / `minstret` 的**可写**语义与**影子读**；
   实现「**写计数器抑制本指令自增**」。
3. **工具层**：默认 Golden ISA 从 `rv32imc_zicclsm_zifencei` 改为
   `rv32imc_zicsr_zicclsm_zifencei_zicntr`；回填 `tests/golden/README.md` 的**三处过期文字**。
4. **台账**：**D-94** 行更新；新增「计数器 / `time`」的**欠账**条目。
5. **收口**：`README.md` / `AGENTS.md` / `docs/progress/PROGRESS.md` 三处同步（数字从当前树重算）。

### 本批**明确不做**的事（**五件**）

| 不做 | 为什么 |
| --- | --- |
| `time` / `timeh` 的**真值**（接 M-15 的 CLINT `mtime`） | ISS 的 CSR 读路径拿不到 CLINT ⇒ 要**跨模块接线** ⇒ 单独登记欠账。⚠️ 参考侧 `time` **恒 0**（实测）⇒ **本批读 0 即与参考侧一致** |
| `mcountinhibit` 的**门控效果** | 规范上门控**只作用于 U 模式**读 `cycle` / `instret` / `time` ⇒ 本平台**只有 M 模式** ⇒ **M 模式下按规范就无效果** ⇒ 不是空洞（裁决 J） |
| `rv32mi/zicntr` **入库** | 它只验「读计数器**不陷入**」⇒ 本平台**结构上不可能失败** ⇒ **空洞** ⇒ 不入库（裁决 L） |
| PMP / S 模式 | D-94 的 ② ③ ⇒ 顺延 |
| 契约层**升版** | 本批**零新 DTO** ⇒ `SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、`PLATFORM_SCHEMA_VERSION` 仍 **1**（裁决 B） |

---

## §1 参考侧语义：**实测**（不是推断）

### §1.1 探针方法论（**上一轮翻车的教训** ⇒ 这里是方法，不是结论**）

第 18 批之后，为了定案参考侧的计数器语义，**第一轮**探针（打包法：退出码 = `s0 | (s1 << 15)`）
给出的结论**互相矛盾**：

* 「影子 `instret` 默认读 **0**」；
* 「`mcountinhibit` 清 IR 位后 `instret` 读 **10**、清 CY 位后 `cycle` 读 **0**」；
* 「写 `minstret` 后紧接着读 = **0**」与「隔一条 `nop` 再读 = **1**」**同时**成立。

⇒ 根因不是参考侧语义诡异，而是**打包解读本身**：`andi` / `slli` / `or` 的组合一旦有一处顺序
写错，读出来的「两个 15 位」会被**静默地**解释成一个**看着合理**的数 ⇒ ⇒ **假事实**。

⇒ **本批的探针方法（逐条钉死）**：

1. **每个观测一个独立 ELF** ⇒ 退出码**直接**就是观测值 ⇒ **结构上不可能**再出现打包 bug。
2. 退出码只有 **8 位**（POSIX）⇒ 观测值一律 `andi` 到 8 位 ⇒ 观测程序不到 60 条指令 ⇒
   **绝不截断**。
3. 需要比较的两个量 ⇒ 用**同构布局**的两个 ELF（指令条数与位置逐条相同）⇒ **直接比退出码**，
   **不做差值推导**。
4. 互斥判据 ⇒ 必须有**同构对照**（只差被测的那一位 / 那一个 CSR 号）⇒ 否则「差 1」分不清是
   「被测项生效」还是「别的项生效」。
5. `.tohost` 段必须**同时**定义 `tohost` **与** `fromhost`（否则 Spike 报
   `tohost and fromhost symbols not in ELF` 并挂死）。
6. 退出码 = `tohost >> 1` ⇒ 打包 `tohost = (value << 1) | 1`。

⚠️ **本批的所有参考侧结论都出自这套探针**；§11 给出复现命令与逐条期望值。

### §1.2 十四条**实测**事实

| 编号 | 事实 | 证据（探针 → 退出码） |
| --- | --- | --- |
| **R1** | `mcycle` / `minstret` / `mcycleh` / `minstreth` **可写** | `Q_W_MCYCLE100_MCYCLE` → **100**；`Q_W_MINSTRETH_17` → **17** |
| **R2** | 写一个半部**不改变**另一个半部 | `Q_W_MINSTRET100_MCYCLE` → 基准 **11**（写 `minstret` 不动 `mcycle`） |
| **R3** | **写的那条指令本身不递增** | `Q_W_MINSTRET100_MINSTRET` → **100**（写 100 → 立刻读到 100，不是 101） |
| **R4** | 之后**每条指令 +1** | `Q_W_MINSTRET100_N1` → **101**；`_N3` → **103**；`Q_GAP0/1/3` → **10 / 11 / 13** |
| **R5** | 读到的值 = **本条指令退休前**的值 | `Q_GAP0_MINSTRET` → **10**（第一条 `csrr` 读到 9，第二条读到 10） |
| **R6** | 影子 `cycle` / `instret` = 对应**机器态**计数器的值 | `Q_W_MCYCLE100_CYCLE` → **100**；`Q_W_MINSTRET100_INSTRET` → **100** |
| **R7** | 影子 CSR **只读**（写被忽略） | `Q_W_CYCLEH_17` → **0**；地址规则（§2.2）亦然 |
| **R8** | `mcountinhibit` 是**普通可写存储**（写后读回） | `Q_W_MCOUNTINHIBIT_5` → **5** |
| **R9** | `mcountinhibit` 在 **M 模式下不门控** `cycle` / `instret` | `Q_W_MCOUNTINHIBIT_5_CYCLE` → **10**（= 基准，与 R4 同值） |
| **R10** | `time` / `timeh` **恒 0**（参考侧未接 mtime） | `Q_TIME` → **0**；`Q_TIMEH` → **0** |
| **R11** | **陷阱指令不计入退休计数** | `Q_ECALL_MINSTRET` → **10** vs 同构对照 `Q_GAP1_MINSTRET` → **11**（只差第 6 条是 `ecall` 还是 `nop` ⇒ 差 1 ⇒ `ecall` 不 +1） |
| **R12** | 64 位计数器**整体回绕**（低/高半部一起） | `Q_W_MINSTRET_FF_NOP` → **0**（= 上游 `TEST_CASE(3)` 的期望值，逐字相同） |
| **R13** | 写**高半部**（`minstreth`）**也**抑制该计数器本指令的 +1 | `Q_W_MINSTRETH_ONLY` → **11** vs 同构对照 `Q_W_MCYCLE100_N1_MINSTRET` → **12**（只差写的是 `minstreth` 还是 `mcycle` ⇒ 差 1） |
| **R14** | 抑制是**按计数器**分的（写 `mcycle` 不抑制 `minstret`，反之亦然） | `Q_W_MCYCLE100_N1_MINSTRET` → **12**；`Q_W_MINSTRET100_N1_MCYCLE` → **12**（两者都 = 基准 +3 ⇒ 都没抑制对方） |

⚠️ **R12 与 R13 合起来 ⇒ 参考侧的内部模型是「一个 64 位计数器，`minstret` / `minstreth` 是它的
两半」** ⇒ ⇒ 本批用「**两个 u32 条目 + 进位**」实现它（裁决 I）⇒ ⇒ **与 u64 逐位等价**。

⚠️ **R1 的推论**：`mcycle` 与 `minstret` 是**两个独立**的计数器（不是同一个）——R14 已证。

### §1.3 参考侧与上游用例的**一致性**核对

| 上游用例 | 断言 | 参考侧实测 | 本批实现后 |
| --- | --- | --- | --- |
| `rv64mi/instret_overflow` `TEST_CASE(2)` | `csrwi minstret,0` → `csrr a0,minstret` == **0** | **0**（R3） | **0** ✓ |
| `rv64mi/instret_overflow` `TEST_CASE(3)` | 写 `minstret`=0xffffffff + 写 `minstreth` + `nop` → `csrr a0,minstret` == **0** | **0**（R12） | **0** ✓（回绕） |
| `rv64mi/instret_overflow` `TEST_CASE(4)` | `csrr a0,minstreth` == **0** | **0**（R12：低半部回绕时高半部**一起**回绕） | **0** ✓ |
| `rv64mi/zicntr`（16 个 `TEST_CASE`） | 四种方式读 `cycle` / `instret` / `cycleh` / `instreth` **不陷入** | 不陷入 | 不陷入 ⇒ **但空洞**（裁决 L） |

⚠️ **`TEST_CASE(4)` 只有 u64 模型才过**：若「两半独立存储」**不带进位**，则低半部回绕时高半部停在
`0xffffffff` ⇒ 读回 ≠ 0 ⇒ ⇒ **进位是必须的**（裁决 I）。

---

## §2 契约层设计

### §2.1 形状：**零新 DTO**（裁决 B）

计数器的**唯一落点是 CSR 文件**（`SimState::csrs`，STAGE-4 裁决 A 已定它是 CSR 值的唯一落点，
且随会话快照**免费往返**）⇒ ⇒ **契约层零新 DTO** ⇒ ⇒ **两个版本都不升**：

| 版本 | 值 | 本批 |
| --- | --- | --- |
| `SCHEMA_VERSION`（契约信封） | **7** | **不升** |
| `SIM_STATE_SCHEMA_VERSION`（快照） | **2** | **不升** |
| `PLATFORM_SCHEMA_VERSION`（板描述） | **1** | **不升** |

⇒ ⇒ **前端镜像 `front/lib/message.js` 不用改版本号** ⇒ ⇒ **不制造 D-143 的存量问题**。

### §2.2 只读地址规则：**修 bug**（裁决 E）

⚠️ **现状是一个真 bug**：`crates/contract/src/csr.rs` 的 `is_csr_read_only` 写的是

```rust
pub fn is_csr_read_only(csr: u16) -> bool {
    csr & 0x800 != 0
}
```

而**它自己的文档表**写的是「`0x800..0xBFF` = user **read-only**」—— ⇒ **代码与自己的表矛盾**。

⇒ **正确的规则是区间规则**（特权规范的 CSR 地址映射）。⚠️ **本表在施工期订正过
一次**（见 §8 订正 3）—— 初稿把 `0x800..0x8FF` 判成只读，与规范**相反**：

| 区间 | 规范含义 | 访问 |
| --- | --- | --- |
| `0x000..0x3FF` | 用户级 / 监管级 | 读 / 写 |
| `0x800..0x8FF` | 用户级 custom | 读 / 写 |
| `0x900..0x9FF` | 用户级 custom | **只读** |
| `0xA00..0xA1F` | 机器级标准 | 读 / 写 |
| `0xA20..0xA3F` | 机器级标准 | **只读** |
| `0xA80..0xA9F` | 机器级标准 | 读 / 写 |
| `0xAA0..0xABF` | 机器级标准 | **只读** |
| `0xAC0..0xAFF` | 机器级标准（保留） | 读 / 写 |
| `0xB00..0xB1F` | 机器级 custom | 读 / 写 |
| `0xB20..0xB3F` | 机器级 custom | **只读** |
| `0xB80..0xB9F` | 机器级 custom | 读 / 写 |
| `0xBA0..0xBBF` | 机器级 custom | **只读** |
| `0xBC0..0xBFF` | 机器级 custom | **只读** |
| `0xC00..0xC7F` | 机器级标准 | **只读** |
| `0xC80..0xCFF` | 机器级 custom | **只读** |

⇒ ⇒ **修法**（**区间判定**，不是位判定）：

```rust
pub fn is_csr_read_only(csr: u16) -> bool {
    // bits [11:10] == 0b11 ⇒ read-only.  这一条同时覆盖
    // `0xC00..0xFFF` 与 `0xBC0..0xBFF`。
    if csr & 0xC00 == 0xC00 {
        return true;
    }
    match csr & 0xF00 {
        0x900 => true,                                    // 0x900..=0x9FF
        0xA00 => matches!(csr & 0xFE0, 0xA20 | 0xAA0),    // 0xA20.. / 0xAA0..
        0xB00 => matches!(csr & 0xFE0, 0xB20 | 0xBA0),    // 0xB20.. / 0xBA0..
        _ => false,                                       // 0x800..=0x8FF 可写
    }
}
```

⚠️ **掩码必须是 `0xFE0`**（bits `[11:5]`，0x20 粒度）：`0xA20..=0xA3F` 这 **32** 个地址**跨
bit 8** ⇒ ⇒ 用 `0x0F0`（取不到单一值）或 `0x1F0`（仍跨 bit 8）都会**判错**。

⚠️ **`mtime`（`0xBFF8`）的 bits `[11:10]` 就是 `0b11`** ⇒ ⇒ 它落在 `0xBC0..0xBFF` ⇒ ⇒
**规则自己就拦它** ⇒ ⇒ **不需要**把它挪进平台只读清单（平台清单仍然只有 `misa`）⇒ ⇒
**零行为变化**（验收 **A-6**）。

⇒ ⇒ **本批涉及的 12 个计数器 CSR 逐个落位**：

| CSR | 地址 | 区间 | 本平台 | 规范 |
| --- | --- | --- | --- | --- |
| `mcountinhibit` | `0x320` | 机器级 RW custom | **可写** | 可写 ✓ |
| `mcycle` | `0xB00` | 机器级 RW custom | **可写** | 可写 ✓ |
| `minstret` | `0xB02` | 机器级 RW custom | **可写** | 可写 ✓ |
| `mcycleh` | `0xB80` | 机器级 RW custom | **可写** | 可写 ✓ |
| `minstreth` | `0xB82` | 机器级 RW custom | **可写** | 可写 ✓ |
| `cycle` | `0xC00` | 机器级 RO 标准 | **只读** | 只读 ✓ |
| `time` | `0xC01` | 机器级 RO 标准 | **只读** | 只读 ✓ |
| `instret` | `0xC02` | 机器级 RO 标准 | **只读** | 只读 ✓ |
| `cycleh` | `0xC80` | 机器级 RO custom | **只读** | 只读 ✓ |
| `timeh` | `0xC81` | 机器级 RO custom | **只读** | 只读 ✓ |
| `instreth` | `0xC82` | 机器级 RO custom | **只读** | 只读 ✓ |
| `mtime` | `0xBFF8` | 机器级 RO custom | **只读** | 只读 ✓ |

⚠️ **修这条规则的**行为影响面**：本平台**已实现**的 CSR（`mstatus` `misa` `mie` `mtvec`
`mscratch` `mepc` `mcause` `mtval` `mip` `irq_clear` `mhartid`）**没有一个**落在
`0x800..0x8FF` 或 `0xC00..0xFFF` 之外却被改动 ⇒ ⇒ **零行为变化**（回归项 **A-6**）。

⚠️ **旧写法会把 `mcycle`（`0xB00`）误判为只读** ⇒ ⇒ **不修 ⇒ 本批根本写不进计数器** ⇒ ⇒
**这条 bug 是本批的硬前置**。

### §2.3 新增常量与**两个映射函数**（裁决 C：唯一落点在契约层）

`crates/contract/src/csr.rs` 新增：

```rust
pub const CSR_MCOUNTINHIBIT: u16 = 0x320;
pub const CSR_MCYCLE: u16 = 0xB00;
pub const CSR_MINSTRET: u16 = 0xB02;
pub const CSR_MCYCLEH: u16 = 0xB80;
pub const CSR_MINSTRETH: u16 = 0xB82;
pub const CSR_CYCLE: u16 = 0xC00;
pub const CSR_TIME: u16 = 0xC01;
pub const CSR_INSTRET: u16 = 0xC02;
pub const CSR_CYCLEH: u16 = 0xC80;
pub const CSR_TIMEH: u16 = 0xC81;
pub const CSR_INSTRETH: u16 = 0xC82;
```

⚠️ **两个映射函数**（**内核不许自己再写一份** ⇒ 「一个约束一个落点」）：

```rust
/// 计数器 CSR → (低半部, 高半部)。影子 CSR 不在表里。
pub fn counter_pair(csr: u16) -> Option<(u16, u16)> {
    match csr {
        CSR_MCYCLE | CSR_MCYCLEH => Some((CSR_MCYCLE, CSR_MCYCLEH)),
        CSR_MINSTRET | CSR_MINSTRETH => Some((CSR_MINSTRET, CSR_MINSTRETH)),
        _ => None,
    }
}

/// 影子 CSR → 它跟随的机器态 CSR。`time` / `timeh` **不在**表里。
pub fn counter_shadow(csr: u16) -> Option<u16> {
    match csr {
        CSR_CYCLE => Some(CSR_MCYCLE),
        CSR_CYCLEH => Some(CSR_MCYCLEH),
        CSR_INSTRET => Some(CSR_MINSTRET),
        CSR_INSTRETH => Some(CSR_MINSTRETH),
        _ => None,
    }
}
```

⇒ ⇒ 内核只**引用**这两个函数 ⇒ ⇒ 「计数器是几半」「影子跟谁」这两个问题**只有一处答案**。

⚠️ **`time` / `timeh` 故意不在 `counter_shadow` 里** ⇒ ⇒ 它们**恒读 0**（裁决 K）⇒ ⇒ 与参考侧
（R10）**逐位一致**。

### §2.4 `csr_is_implemented` 的新增项

```rust
| CSR_MCOUNTINHIBIT
| CSR_MCYCLE
| CSR_MINSTRET
| CSR_MCYCLEH
| CSR_MINSTRETH
```

⚠️ **为什么必须加**：`csr_write_is_ignored` 里有 `|| !csr_is_implemented(csr)` ⇒ ⇒ 不加 ⇒ 写
`mcycle` 被忽略 ⇒ ⇒ 计数器**根本写不进去** ⇒ ⇒ 上游 `TEST_CASE(2)` 必 FAIL。

⚠️ **影子 CSR（`cycle` / `instret` / …）**故意**不加** ⇒ ⇒ 它们的写被**只读规则**拦（§2.2）⇒ ⇒
**不需要**在写路径上单独特判 ⇒ ⇒ 也 ⇒ **`wrote_csr` 不会被影子置位**（§3.3 依赖这一点）。

---

## §3 内核设计

### §3.1 计数器的**物理落点**（裁决 C / I）

| 半部 | CSR 地址 | 存哪 |
| --- | --- | --- |
| `mcycle` 低 / 高 | `0xB00` / `0xB80` | CSR 文件的两个 `u32` 条目 |
| `minstret` 低 / 高 | `0xB02` / `0xB82` | CSR 文件的两个 `u32` 条目 |

⇒ ⇒ **一个 64 位计数器 = 两个 CSR 文件条目** ⇒ ⇒ **快照 / 会话往返免费** ⇒ ⇒ **零 DTO**。

⚠️ **为什么不是「一个 u64 字段」**：那要给 `SimState` 加字段 ⇒ ⇒ `SIM_STATE_SCHEMA_VERSION`
**2 → 3** ⇒ ⇒ 契约层升版 + 前端镜像改 + 快照 DTO 改 ⇒ ⇒ **代价大而无收益**（架构上本来就是
「两个半部各占一个 CSR 地址」）。

### §3.2 自增：**唯一的落点**（裁决 F）

**落点**：`crates/kernel/src/m13_iss/executor.rs` 的 `execute_one`（第 775 行起）的**正常完成出口**
（第 1241 行 `Ok(StepOutcome::Advanced)` 之前）。

⚠️ **为什么是这里**（三条理由，逐条可验）：

1. **两条 dispatch 路径都经它** ⇒ ⇒ **唯一落点**：`step_one`（`m13_iss/mod.rs:157`，经
   `execute_with_report`）与 `step_one_via_bus`（`m13_iss/mod.rs:249`，直接调 `execute_one`）⇒ ⇒
   **同一个函数** ⇒ ⇒ 不可能两条路径行为不一致。
2. **陷阱指令天然被排除**（裁决 G）⇒ ⇒ `execute_one` 的陷阱分支是**提前 `return`**（如 `:1075` 的
   `Ecall`、`:1081` 的 `Breakpoint`）⇒ ⇒ **到不了自增那行** ⇒ ⇒ **R11 自动成立**，不需要额外
   的「这是不是陷阱」的判断。
3. **读到的值 = 退休前的值** ⇒ ⇒ CSR 指令在 `execute_one` **内部**读文件（`:1094` 等）⇒ ⇒ 读到的
   值是**自增前**的 ⇒ ⇒ **R5 自动成立**。

⚠️ **自增算法（两半 + 进位 ⇒ 与 u64 逐位等价）**：

```
低半部 = 低半部.wrapping_add(1)
若 低半部 发生回绕（原值 == 0xFFFF_FFFF）:
    高半部 = 高半部.wrapping_add(1)
```

⇒ ⇒ 低半部回绕时高半部**跟着 +1** ⇒ ⇒ **R12 成立** ⇒ ⇒ 上游 `TEST_CASE(4)` 过。

⚠️ **`mcycle` 与 `minstret` 是两个独立计数器** ⇒ ⇒ 两者**都**每条退休指令 +1（无停顿时二者数值
相同 ⇒ ⇒ 与参考侧一致，R1 的推论）。

### §3.3 抑制：**写计数器的那条指令不 +1**（裁决 H）

⚠️ **落点**：**复用现成的 `wrote_csr` 通道** ⇒ ⇒ **零新增状态**、**零新增参数**、**零 DTO**。

`execute_one` 已有形参 `wrote_csr: &mut Option<u16>` ⇒ ⇒ 六个 CSR 写指令分支
（`Csrrw` / `Csrrs` / `Csrrc` / `Csrrwi` / `Csrrsi` / `Csrrci`）**都在写入未被忽略时**置位它 ⇒ ⇒

> `wrote_csr == Some(csr)` ⇒ **本指令写了 `csr`**。

⚠️ **影子 CSR 不会置位它** ⇒ ⇒ 因为 §2.2 把影子判为只读 ⇒ ⇒ `csr_write_is_ignored` 为真 ⇒ ⇒
**不会走到 `csr_write`** ⇒ ⇒ **R7 与「影子写不抑制」自动成立**。

⇒ ⇒ **抑制规则**（按 R13 / R14）：

| 本指令写的 CSR | 抑制哪个计数器本指令的 +1 |
| --- | --- |
| `mcycle`（`0xB00`）/ `mcycleh`（`0xB80`） | **只**抑制 `mcycle` |
| `minstret`（`0xB02`）/ `minstreth`（`0xB82`） | **只**抑制 `minstret` |
| 其它（含全部影子） | **不抑制** |

⚠️ **抑制的粒度是「本指令」** ⇒ ⇒ 抑制**只影响那一条** ⇒ ⇒ 之后照常 +1（`Q_W_MINSTRETH_ONLY_NOP2`
→ **12** 比 `Q_W_MINSTRETH_ONLY` → **11** 多 1 ⇒ 实测证实）。

### §3.4 影子读：**推导量**（裁决 D）

`crates/kernel/src/m13_iss/csr.rs` 的 `csr_read` 加特判（**唯一落点**）：

```
CSR_MISA      → CSR_MISA_VALUE        （既有）
CSR_MHARTID   → CSR_MHARTID_VALUE     （既有）
CSR_CYCLE / CSR_CYCLEH / CSR_INSTRET / CSR_INSTRETH
              → counter_shadow(csr) 指向的文件条目
CSR_TIME / CSR_TIMEH
              → 0                     （裁决 K）
其它          → 文件查找（既有）
```

⇒ ⇒ **影子是推导量** ⇒ ⇒ **不是第二个真相源** ⇒ ⇒ 快照里**不会**同时出现「`mcycle` 的值」与
「`cycle` 的值」两份 ⇒ ⇒ 不会出现「改一个不改另一个」的不同步。

⚠️ **写路径不需要特判**：影子被只读规则拦住 ⇒ ⇒ 写不进文件 ⇒ ⇒ `counter_shadow` 只在读路径用。

### §3.5 `mcountinhibit`：**可写但不门控**（裁决 J）

* 加入 `csr_is_implemented` ⇒ ⇒ **写进去能读回**（R8）。
* **不实现门控效果** ⇒ ⇒ 因为规范上门控**只作用于 U 模式**读 `cycle` / `instret` / `time`
  ⇒ ⇒ 本平台**只有 M 模式** ⇒ ⇒ **M 模式下按规范就无效果** ⇒ ⇒ **不是空洞，是规范正确**。
* ⚠️ **上游 `rv32mi/instret_overflow` 会 `csrci mcountinhibit, 0x4`**（注释说「reset 值未指定」）
  ⇒ ⇒ 本平台**写它不陷入** ⇒ ⇒ 用例能过 ⇒ ⇒ **这条是硬要求**（验收 **B-9**）。

⚠️ **本条必须写进规格**：否则后人看到「`mcountinhibit` 能写却不生效」会以为漏实现。

### §3.6 `time` / `timeh`：**恒 0**（裁决 K）

* 参考侧 **恒 0**（R10）⇒ ⇒ 本批读 0 **与参考侧逐位一致** ⇒ ⇒ 不影响黄金对拍。
* ⚠️ **但功能确实缺失** ⇒ ⇒ **必须登记欠账**（「`time` / `mtime` 需要接 M-15 的 CLINT」）⇒ ⇒
  **不静默**：台账里有 ⇒ ⇒ 下一批能看到。

---

## §4 工具层设计

### §4.1 默认 Golden ISA：加 `zicsr` + `zicntr`（裁决 M）

`tools/golden-run.sh` 的 `ISA="${GOLDEN_ISA:-rv32imc_zicclsm_zifencei}"` ⇒ ⇒ 改为

```
rv32imc_zicsr_zicclsm_zifencei_zicntr
```

⇒ ⇒ **为什么安全**：第 18 批**已实测**——用这个 ISA 跑全量 68 例 ⇒ ⇒ **仍 68/68 全绿**（无
spike ISA 报错）⇒ ⇒ 参考侧多支持两个扩展**不破**已有对拍。

⚠️ **为什么必须加 `zicntr`**（⚠️ **本段在施工期订正过一次**，见 §8 订正 5）：**Zicntr
带来的**影子** CSR（`cycle` / `instret` / `cycleh` / `instreth`）是 **standard** CSR ⇒ ⇒
Spike **按扩展门控**它们 ⇒ ⇒ 没有 `zicntr` ⇒ ⇒ **读 `cycle` 陷入非法指令** ⇒ ⇒
`rv32mi/zicntr` 用例在参考侧**根本跑不完**（实测：旧串下参考侧报 `FAIL 2`，新串下报
`PASS`）⇒ ⇒ **没有真判据**。

⚠️ **而 `zicsr` 的理由要改**（同上）：初稿说「不 `zicsr` ⇒ 参考侧不实现 Zicsr ⇒ ⇒ 读
`cycle` 会陷入 ⇒ ⇒ `instret_overflow` 跑不完」—— ⚠️ **实测逐条不成立**：① Spike 的基
`rv32imc` **已隐含** Zicsr ⇒ ⇒ 旧串下 `csrr` / `csrw` **照常执行**；② `rv32mi/instret_overflow`
在旧串下**跑得完**（参考侧 `PASS`）；③ `mcycle` / `minstret` / `mcountinhibit` 住在
**custom** CSR 区（`0xB00..0xB1F` / `0x200..0x3FF`）⇒ ⇒ Spike **不按扩展门控**它们 ⇒ ⇒
旧串下**可用**（实测：写 `minstret` = `0xFFFFFFFF` 落地、随后回绕读回 `0`）。

⇒ ⇒ **精确口径**：**只有 `zicntr` 是必需的**（它门控的是 **standard** 影子 CSR）；
`zicsr` 保留在串里是**显式点名 `i` 的隐含依赖**（不依赖隐含 ⇒ ⇒ 更不容易被误解），**不是**
「没有它就跑不了」。

⇒ ⇒ **决策不变**：默认 ISA 改成 `rv32imc_zicsr_zicclsm_zifencei_zicntr`（新串已实测全量
**68 / 68** 不破）。

⇒ ⇒ **两个都要**。

### §4.2 语料：**仍是 68 例**（裁决 L）

⚠️ **本批**不**增**语料行（`tests/golden/corpus.txt` 的 `active` 行**一条不动**）：

| 用例 | 处置 | 理由 |
| --- | --- | --- |
| `rv32mi/zicntr` | **不入库** | 只验「读计数器**不陷入**」⇒ 本平台**结构上不可能失败**（读未实现 CSR 也不陷入）⇒ ⇒ **空洞** |
| `rv32mi/instret_overflow` | **已在库**（`active`）⇒ 本批后**出空洞清单** | 它断言**值** ⇒ 平台**可以失败** ⇒ ⇒ 实现后是**真覆盖** |

⇒ ⇒ **语料 68 → 68** ⇒ ⇒ **写死的「68 / 68」数字不用改**。

### §4.3 `tests/golden/README.md`：**回填三处过期文字**（裁决 L）

| 处 | 现状 | 改成 |
| --- | --- | --- |
| 「不入库清单」表里 `rv32mi/zicntr` 行 | 「the **reference side** does not implement Zicntr in this ISA configuration and traps on `cycle`」 | 参考侧**现在实现** Zicntr ⇒ ⇒ **不再 trap** ⇒ ⇒ 理由改为「**空洞**：只验不陷入，本平台结构上不可能失败」 |
| 「空洞的一致」表里 `rv32mi/instret_overflow` 行 | 「the platform has **no counter at all**」 | **本批之后失效** ⇒ ⇒ **移出空洞清单**（并说明它现在断言的是**值**） |
| 「Zicclsm detail」一节里的 ISA 串 | `rv32imc_zicclsm_zifencei` | `rv32imc_zicsr_zicclsm_zifencei_zicntr` + 说明为什么两个都要 |

⚠️ ⚠️ **`rv32mi/instret_overflow` 那一行的移除是本批 README 最重要的改动** ⇒ ⇒ 因为它是**台账里
唯一一条「实现之后变真」的空洞** ⇒ ⇒ 不改 ⇒ ⇒ 台账继续拿空洞当覆盖 ⇒ ⇒ 正是本批要消灭的东西。

---

## §5 验收

### §5.1 A 组 · 契约层

| 编号 | 验收项 |
| --- | --- |
| **A-1** | `is_csr_read_only` 改成**区间规则**：`0xC00..=0xFFF` 与 `0x800..=0x8FF` 只读，其余可写 |
| **A-2** | 12 个计数器 CSR 的**只读 / 可写分类**逐个断言，与 §2.2 的表**逐格相同** |
| **A-3** | `counter_pair` 对 4 个计数器 CSR 返回**正确的半部对**；对影子 CSR 与其它 CSR 返回 `None` |
| **A-4** | `counter_shadow` 对 4 个影子 CSR 返回**正确的机器态 CSR**；对 `time` / `timeh` 与其它返回 `None` |
| **A-5** | `csr_is_implemented` 含 `mcountinhibit` / `mcycle` / `minstret` / `mcycleh` / `minstreth` |
| **A-6** | **回归**：本平台**已实现**的 11 个 CSR 的「可写 / 只读」分类**与修改前逐条相同**（零行为变化） |
| **A-7** | `mcycle` / `minstret` **不被**只读规则拦 ⇒ ⇒ `csr_write_is_ignored` 为**假** |
| **A-8** | 影子 `cycle` / `instret` / `cycleh` / `instreth` / `time` / `timeh` **被**只读规则拦 |

### §5.2 B 组 · 内核

| 编号 | 验收项 |
| --- | --- |
| **B-1** | 初始状态读 `mcycle` / `minstret` / `cycle` / `instret` == **0** |
| **B-2** | 执行 N 条普通指令后读 `minstret` == **N** |
| **B-3** | 读 `cycle` / `instret` 的值与读 `mcycle` / `minstret` **逐位相同**（影子跟随） |
| **B-4** | 写 `mcycle` / `minstret` **立刻**读回 == **写进去的值**（写指令本身不 +1） |
| **B-5** | 写 `minstret` 后隔 **1** 条指令再读 == 写值 **+1**；隔 **3** 条 == 写值 **+3** |
| **B-6** | 写 `mcycle` **不**抑制 `minstret` 的 +1；写 `minstret` **不**抑制 `mcycle` 的 +1 |
| **B-7** | 写 `minstreth`（高半部）**抑制** `minstret` 本指令的 +1，且**只**影响那一条 |
| **B-8** | 低半部回绕时**高半部跟着 +1**（两半 + 进位 ≡ u64） |
| **B-9** | 写 `mcountinhibit` **不陷入**，且**写后能读回** |
| **B-10** | 陷阱指令（`ecall`）**不**计入退休计数；普通指令计入 |

### §5.3 C 组 · 工具层

| 编号 | 验收项 |
| --- | --- |
| **C-1** | `tools/golden-run.sh` 的默认 ISA 含 `zicsr` **与** `zicntr` |
| **C-2** | `GOLDEN_ISA` 覆盖口仍**可用**（能覆盖默认 ISA） |
| **C-3** | 语料仍是 **68** 行 `active`，**无新增行** |
| **C-4** | `tests/golden/README.md` 的「不入库清单」里 `rv32mi/zicntr` 的理由**已改写**（不再说参考侧 trap） |
| **C-5** | `tests/golden/README.md` 的「空洞的一致」一节**不再**列 `rv32mi/instret_overflow` |
| **C-6** | `tests/golden/README.md` 里的 ISA 串与 `tools/golden-run.sh` 的默认 ISA **逐字相同** |

### §5.4 T 组 · 台账与文档

| 编号 | 验收项 |
| --- | --- |
| **T-1** | 台账 **D-94** 行更新：记下「① 计数器已闭环」与「② PMP / ③ S 模式顺延」 |
| **T-2** | 台账新增**欠账**条目：`time` / `timeh` 恒 0（需接 M-15 的 CLINT） |
| **T-3** | 台账 §0.1 的三个合计数按**口径**更新（口径见 §7 裁决 N） |
| **T-4** | `README.md` / `AGENTS.md` / `docs/progress/PROGRESS.md` 三处**同步**，数字**从当前树重算** |

### §5.5 X 组 · 跨层与判别力

| 编号 | 验收项 |
| --- | --- |
| **X-1** | **黄金对拍全量 68 例仍全绿**（改默认 ISA 之后）⇒ ⇒ 参考侧带 `zicsr` + `zicntr` **不破** |
| **X-2** | **`rv32mi/instret_overflow` 两侧都 PASS，且**候选侧**的值**不是空洞**（变异自检证明它能红） |
| **X-3** | **契约层零升版**：`SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、`PLATFORM_SCHEMA_VERSION` 仍 **1** ⇒ ⇒ `front/lib/message.js` **不用改** |
| **X-4** | **端到端判别式**：候选侧读 `minstret` 的值随**实际退休指令数**递增 ⇒ ⇒ 读回**可复现** |
| **X-5** | **两个版本都不升** ⇒ ⇒ 本批**不制造** D-143 的存量问题 |

---

## §6 门禁

| 门禁 | 判据 |
| --- | --- |
| `cargo fmt --all --check` | 退出码 **0** |
| `cargo clippy --workspace --all-targets -- -D warnings` | 退出码 **0** |
| `cargo test --workspace --no-fail-fast --lib --tests` | 退出码 **0** 且 `grep -cE "^test result: FAILED"` 为 **0**（失败数在 `awk -F'[ ;]'` 的 **`$7`**） |
| `tools/check-front.sh` | 退出码 **0**（前端用例 + 归属口径 + 三棵树历史回归） |
| `tools/golden-run.sh --self-check` + `--keep-going` | **68 / 68**，0 mismatch / 0 no-verdict / 0 build-failed |
| `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | 退出码 **0** |
| `node tools/browser-verify.mjs` | 退出码 **0**（真 Chrome；跑前先确认 8080 端口是空的） |
| `tools/boot-osz.sh` | `VERDICT MATCH`（不进默认门禁，按需跑） |
| `python3 tools/stage20_check.py docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md` | **0 红** |
| `python3 tools/stage19_check.py` / `stage18_check.py` | **0 红** |
| `python3 tools/ledger_check.py --ledger` + `--self-test` | **0 红** |
| `python3 tools/doc_check.py --baseline` | **新增失败数 0** |
| `python3 tools/front_test_attrib.py` + `--selftest` | 通过（三棵树历史回归逐字复现） |

---

## §7 裁决

### 裁决 A · 本批范围 = D-94 的 ①

计数器（Zicntr）**只做这一件**。PMP 与 S 模式顺延。理由见 §0。

### 裁决 B · 契约层**零升版**

`SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、`PLATFORM_SCHEMA_VERSION` 仍 **1**。
理由：计数器的落点是 CSR 文件（§3.1）⇒ ⇒ 零新 DTO ⇒ ⇒ 前端镜像不用改 ⇒ ⇒ 不制造 D-143 的
存量问题（验收 **X-3** / **X-5**）。

### 裁决 C · 计数器的**配对与影子映射**只在契约层

`counter_pair` 与 `counter_shadow` 是**唯一**落点（§2.3）⇒ ⇒ 内核**不许**自己再写一份匹配 ⇒ ⇒
「计数器是几半」「影子跟谁」**只有一处答案**。

### 裁决 D · 影子 CSR 是**推导量**

`cycle` / `instret` / `cycleh` / `instreth` 的值**永远**从机器态计数器**算出来**（§3.4）⇒ ⇒
**不进 CSR 文件** ⇒ ⇒ 不是第二个真相源 ⇒ ⇒ 快照里不会出现两份值。

### 裁决 E · 只读地址规则**改成区间规则**

`is_csr_read_only` 从「bit 11」改成「**标准 CSR 地址映射**的区间判定」（§2.2）⇒ ⇒ 旧写法**把
`mcycle` 误判为只读** ⇒ ⇒ **不修则本批无法施工** ⇒ ⇒ 零行为变化由 **A-6** 钉住。

⚠️ **判据是「地址落在哪一段」而不是「某个位」** ⇒ ⇒ 旧实现的 bug 正是「用一位去近似一段」⇒ ⇒
本裁决也顺手把那几段**只读但位判定取不到**的区间（`0x900..0x9FF` / `0xA20..0xA3F` /
`0xAA0..0xABF` / `0xB20..0xB3F` / `0xBA0..0xBBF`）补齐 ⇒ ⇒ 掩码用 `0xFE0`（0x20 粒度）。

### 裁决 F · 自增的**唯一落点** = `execute_one` 的正常完成出口

理由三条见 §3.2 ⇒ ⇒ 两条 dispatch 路径共用 ⇒ ⇒ 陷阱指令天然排除 ⇒ ⇒ 读到的值天然是退休前的值。

### 裁决 G · **陷阱指令不退休**

`ecall` / `ebreak` 等**提前 `return`** ⇒ ⇒ 到不了自增那行 ⇒ ⇒ R11 自动成立（验收 **B-10**）。

### 裁决 H · 写计数器**抑制本指令的 +1**，**按计数器**分

规则表见 §3.3 ⇒ ⇒ **复用 `wrote_csr` 通道** ⇒ ⇒ 零新增状态、零新增参数、零 DTO ⇒ ⇒ 影子写不置位
⇒ ⇒ 天然不抑制。

### 裁决 I · 计数器的内部模型 = **两半 + 进位**（≡ u64）

落点是 CSR 文件的两个 `u32` 条目（§3.1）⇒ ⇒ 与 u64 **逐位等价** ⇒ ⇒ 上游 `TEST_CASE(4)` 过
⇒ ⇒ **不加 `SimState` 字段** ⇒ ⇒ 零 DTO。

### 裁决 J · `mcountinhibit` **可写但不门控**

理由见 §3.5 ⇒ ⇒ **M 模式下按规范无效果** ⇒ ⇒ 不是空洞 ⇒ ⇒ 但**必须写进规格**（§3.5 末段）。

### 裁决 K · `time` / `timeh` **恒 0**，欠账**另行登记**

参考侧恒 0（R10）⇒ ⇒ 本批读 0 与参考侧逐位一致 ⇒ ⇒ 真值需要接 M-15 的 CLINT ⇒ ⇒ 跨模块 ⇒ ⇒
**单独欠账**，**不静默**。

### 裁决 L · `rv32mi/zicntr` **不入库**；`rv32mi/instret_overflow` **出空洞清单**

`rv32mi/zicntr` 只验「不陷入」⇒ ⇒ 空洞 ⇒ ⇒ 不入库 ⇒ ⇒ **语料仍是 68 例** ⇒ ⇒ 写死的「68 / 68」
数字不动。`rv32mi/instret_overflow` 断言**值** ⇒ ⇒ 实现后是**真覆盖** ⇒ ⇒ 必须出空洞清单
（验收 **C-5**）。

### 裁决 M · 默认 Golden ISA 加 `zicsr` + `zicntr`

理由见 §4.1 ⇒ ⇒ **必须加的是 `zicntr`**（它门控 **standard** 影子 CSR ⇒ ⇒ 缺了它读 `cycle`
**陷入** ⇒ ⇒ `rv32mi/zicntr` 在参考侧跑不完 ⇒ ⇒ 没有真判据）；⇒ ⇒ `zicsr` 保留是**显式点名
`i` 的隐含依赖**，**不是**必需（⚠️ 施工期实测订正，见 §8 订正 5）。

### 裁决 N · §0.1 三个合计数的口径

**合计** = +本批**新登记**数；**未闭环** = +本批**新登记**数（**闭环不回填**）；**已闭环** = ±0
（除非回填进 §0.1 模块行）⇒ ⇒ 本批**新登记 1 条**（`time` 欠账）⇒ ⇒ **合计 +1**、**未闭环 +1**、
**已闭环 ±0**。⚠️ **D-94 本身不是本批新登记**（它是 D-94 的 ① 闭环，不新增行）⇒ ⇒ 不进这三个数。

### 裁决 O · 规格号 = **STAGE-20**（I2C 那批顺延为 STAGE-21）

`STAGE-18` / `STAGE-19` 已被勘察批与收敛批占用 ⇒ ⇒ I2C 那批顺延为 **STAGE-21**。

---

## §8 施工期订正

> ⚠️ **本节在施工期逐条追加**（每条写明「现象 → 根因 → 订正 → 影响面」）。**空着 = 还没施工**。

### 订正 1 · `mtime` 的只读**不需要**平台清单（初稿判错 ⇒ 已改）

⚠️ **现象**：初稿把 `mtime`（`0xBFF8`）当成「按地址规则**可写**、要靠平台清单兜住」的 CSR。

⚠️ **根因**：**手算错了** —— `0xBFF8 & 0xC00` 是 **`0xC00`**（不是 `0x0800`）⇒ ⇒ 它的
bits `[11:10]` 就是 `0b11` ⇒ ⇒ 它落在 `0xBC0..0xBFF` 这个**只读**区 ⇒ ⇒ **规则自己就拦它**
⇒ ⇒ 平台清单里那个 `mtime` 条目是**多余的**。

⚠️ **订正**：`csr_is_read_only` 的平台清单**仍然只有 `misa`** ⇒ ⇒ 契约层改动面更小 ⇒ ⇒
**零行为变化**由 **A-6** 钉住。

⚠️ **教训**：运算符优先级与掩码**必须实测**，不能手算 —— 本次是先用 `rustc` 跑了一小段
程序才发现（`0xBFF8 & 0xC00 == 0xC00`）。

### 订正 2 · 掩码粒度：`0x0F0` / `0x1F0` 都**判错**，必须是 `0xFE0`

⚠️ **现象**：按初稿的规则实现后，`is_csr_read_only(0x0A3F)` 判成了**可写**。

⚠️ **根因**：`0xA20..=0xA3F` 是 **32** 个地址，它们**跨 bit 8** ⇒ ⇒ `csr & 0x0F0` 取不到
单一值（`0xA20` 得 `0x020`、`0xA3F` 得 `0x030`）⇒ ⇒ 改用 `0x1F0` 仍**跨 bit 8** ⇒ ⇒ 必须用
`0xFE0`（bits `[11:5]`）⇒ ⇒ 此时 `0xA20..=0xA3F` 一律得 `0xA20`。

⚠️ **订正**：掩码改为 `0xFE0`，匹配值改为 `0xA20 | 0xAA0` 与 `0xB20 | 0xBA0`。

⚠️ **影响面**：只读 / 可写的**最终分类**与 §2.2 的表**逐格一致** ⇒ ⇒ **A-6** 仍成立。

### 订正 3 · 区间表初稿把 `0x800..0x8FF` 判成**只读**（与规范相反）

⚠️ **现象**：初稿的区间表写「`0x800..0x8FF` = 用户级 custom **只读**」。

⚠️ **根因**：初稿沿用了**旧实现**的粗粒度（bit 11 置位 ⇒ `0x800..0xBFF` 一律只读），没有
回到**规范**去核对 ⇒ ⇒ 而规范里 `0x800..0x8FF` 是 **read/write** custom ⇒ ⇒ **写反了**。

⚠️ **订正**：§2.2 的表按规范重写（见上）⇒ ⇒ `0x800..0x8FF` **可写**。

⚠️ **影响面**：本平台**已实现**的 11 个 CSR **无一**落在 `0x800..0x8FF` ⇒ ⇒ 零行为变化 ⇒ ⇒
**A-6** 仍成立。

### 订正 4 · 契约层改动的**唯一**内核回归（在 Lane B 内消化）

⚠️ **现象**：契约层修规则后，内核 lib 有 **1** 条红：
`m13_iss::tests_csr::a_write_to_a_read_only_csr_is_ignored` —— 它拿 `0xb00` 当「只读 CSR」的
样例，而 `0xb00` 现在是 `mcycle`、**可写**了。

⚠️ **根因**：不是内核逻辑坏了 ⇒ ⇒ 是**那条样例失效** ⇒ ⇒ 属「规格 §2.2 的修法生效」的
**必然后果**。

⚠️ **订正**：Lane B 把样例换成真正只读的 CSR（`0xC00` 影子 / `0xBFF8` `mtime`），并**补一条
反向断言**：`mcycle` / `minstret` 的写**不再**被忽略 ⇒ ⇒ 这条反向断言就是本批契约层 bug
修复的**判别力**证据。

⚠️ **影响面**：其余 **1146** 条内核 lib 用例在契约层改动后**全部通过** ⇒ ⇒ 契约层改动对内核
**零回归**。

### 订正 5 · 默认 ISA 的两条理由被实测**逐条推翻**（Lane C 实测 + 统筹方复核）

⚠️ **现象**：初稿 §4.1 与裁决 M 写「不 `zicsr` ⇒ 参考侧不实现 Zicsr ⇒ ⇒ 读 `cycle` 会陷入 ⇒ ⇒
`rv32mi/instret_overflow` 在参考侧**根本跑不完**」与「不 `zicntr` ⇒ ⇒ `minstret` / `mcycle`
在参考侧**不可用** ⇒ ⇒ `csrwi minstret, 0` 会陷入」。

⚠️ **根因**：**把两类 CSR 混为一谈** ⇒ ⇒ **standard** CSR（`cycle` / `instret` / `cycleh` /
`instreth`）Spike **按扩展门控** ⇒ ⇒ 缺 `zicntr` 就**陷入**；⇒ ⇒ 而 **custom** CSR
（`mcycle` / `minstret` / `mcountinhibit`）Spike **不按扩展门控** ⇒ ⇒ 缺 `zicntr` 也**照常可用**；
⇒ ⇒ 而 **Zicsr 的指令**（`csrr` / `csrw` / …）由基 `rv32imc` **隐含** ⇒ ⇒ 旧串下**照常执行**。

⚠️ **实测证据**（两条独立路径测得**同一结论**）：Lane C 用 `spike --log-commits` 逐指令核对，
统筹方用独立探针在**两个 ISA** 下各跑一遍：

| 探针 | 旧串 `rv32imc_zicclsm_zifencei` | 新串 `rv32imc_zicsr_zicclsm_zifencei_zicntr` |
| --- | --- | --- |
| 读 `cycle`（**standard** 影子） | **陷入**（`tohost` = 异常值） | 读到 **9** |
| 读 `minstret`（**custom**） | 读到 **9** | 读到 **9** |
| 写 `minstret`=100 再读（**custom**） | 读回 **100** | 读回 **100** |
| `mcountinhibit`（**custom**） | 可读 | 可读 |

⇒ ⇒ **订正**：§4.1 与裁决 M 改成上面的**精确口径** ⇒ ⇒ **只有 `zicntr` 是必需的**；
`zicsr` 保留的理由改成「显式点名 `i` 的隐含依赖」⇒ ⇒ **决策本身不变**（新串实测全量
**68 / 68**）。

⚠️ **连带订正**：`tests/golden/README.md` 里 Lane C 按初稿理由写的 `zicsr` 段落（「without it
there is no `csrr` / `csrw` ⇒ ⇒ the counter cases cannot run to completion」）**与实测矛盾**
⇒ ⇒ 统筹方改成实测口径；`zicntr` 段落**本来就对**（它说的正是「读 `cycle` 陷入」）⇒ ⇒ 保留。

⚠️ **教训**：**「某个扩展缺失 ⇒ 某个 CSR 不可用」不是一条能想当然的规则** ⇒ ⇒ Spike 对
**standard** CSR 按扩展门控、对 **custom** CSR **不**门控 ⇒ ⇒ 差别**只在地址段**里 ⇒ ⇒
**必须实测**（这条与第 18 批「上一轮探针结论互相矛盾」是同一条纪律的另一个面）。

---

## §9 变异自检（**每 lane 至少一条对照 0 红**）

### §9.1 纪律（**沿用，不得放宽**）

* 变异**必须能编译**；期望集合用 **AND 语义**；记录**完整红集**。
* 还原用 `cp` 备份 + `md5` + `cmp` **逐字节校验** ⇒ ⇒ **`touch <file>`（当前时间，不是 `touch -r`）**
  ⇒ ⇒ 再重建复跑。⛔ **禁** `git checkout -- <file>`。
* 备份名用**相对路径**做键；变异期间**不改其它被编译源文件**；每条变异**必须有超时**
  （macOS 无 `timeout` ⇒ 用仓内 `tools/watchdog.pl <秒> <cmd>`，**124 = HANG**）。
* macOS 新链接二进制首次 `exec` 会被安全评估卡住 ⇒ ⇒ **串行**预热，**勿并行**。
* 并发 cargo ⇒ ⇒ 假 `NOVERDICT` / swap 风暴 ⇒ ⇒ 每 lane **独立** `CARGO_TARGET_DIR`。

### §9.2 计划中的三条变异

| 编号 | 变异 | 期望变红 | 判别力说明 |
| --- | --- | --- | --- |
| **M1** | 把 `execute_one` 的**自增**关掉（不 +1） | `rv32mi/instret_overflow` **红** | 证明该用例**从空洞变成真覆盖**（验收 **X-2**）⇒ ⇒ **不是空洞** |
| **M2** | 把「写计数器抑制本指令 +1」的判定**去掉** | `rv32mi/instret_overflow` **红** | 证明**抑制规则**被真覆盖（R3 / R13） |
| **M3** | 把两半的**进位**去掉（低半部回绕时高半部不动） | `rv32mi/instret_overflow` **红** | 证明 **u64 等价性**被真覆盖（R12 / `TEST_CASE(4)`） |

⚠️ **每条变异都要有「对照 0 红」**：同一变异体在**未变异**的基线上跑 ⇒ **0 红**。

⚠️ ⚠️ **M1 的额外要求**：变异后 `rv32mi/instret_overflow` **必须红** ⇒ ⇒ 这就证明
`rv32mi/instret_overflow` 在**实现后**是**真覆盖** ⇒ ⇒ 与「空洞清单移除」互为证据 ⇒ ⇒ **两处
必须一起改**（验收 **C-5** 与 **X-2**）。

---

## §10 风险与已知边界

| 风险 | 处置 |
| --- | --- |
| **默认 ISA 改动影响已有对拍** | 已实测 68/68 全绿（§4.1）⇒ ⇒ 门禁 **X-1** 再验一次 |
| **`is_csr_read_only` 改动波及面** | 已实现 CSR **无一落在**被改的两个区间 ⇒ ⇒ 回归项 **A-6** 逐条断言 |
| **计数器的绝对值与参考侧不同** | ⚠️ `minstret` 的**复位值未指定** ⇒ ⇒ 参考侧从 5 起、我们从 0 起 ⇒ ⇒ **绝对值不可比** ⇒ ⇒ 黄金对拍只比**判决** ⇒ ⇒ 不影响 |
| **`cycle` 与 `instret` 的语义差**（周期 vs 指令） | 本平台**无停顿模型** ⇒ ⇒ 二者数值相同 ⇒ ⇒ 与参考侧一致 ⇒ ⇒ 若将来引入停顿，**必须**重新区分（**下一批的欠账**） |
| **`time` 恒 0** | 与参考侧一致 ⇒ ⇒ 不影响对拍 ⇒ ⇒ 但**功能缺失** ⇒ ⇒ 台账欠账（验收 **T-2**） |
| **`mcountinhibit` 不门控** | M 模式下按规范无效果 ⇒ ⇒ **不是空洞** ⇒ ⇒ 但写进规格防误判（裁决 J） |

---

## §11 复现命令

### §11.1 参考侧探针（本批的**唯一**事实来源）

⚠️ 探针在**仓外**（`/tmp/v019-probe/`），**不进仓** ⇒ ⇒ 理由：它们是**一次性**的语义定案工具，
不是回归资产 ⇒ ⇒ **回归资产是 §5 的验收项与 §9 的变异自检**。

```bash
# 编译（每个观测一个独立 ELF ⇒ 退出码直接就是观测值）
GCC=~/riscv-sim-golden/toolchain/bin/riscv-none-elf-gcc
$GCC -DQ_MINSTRET -march=rv32i_zicsr_zifencei -mabi=ilp32 -static -mcmodel=medany \
  -fvisibility=hidden -nostdlib -nostartfiles \
  -T tests/golden/env/link.ld q.S -o q_Q_MINSTRET.elf

# 运行（退出码 = 观测值；`.tohost` 段必须同时有 `tohost` 与 `fromhost`）
perl tools/watchdog.pl 20 spike --isa=rv32imc_zicsr_zicclsm_zifencei_zicntr \
  -m2147479552:4096,2147483648:4194304 q_Q_MINSTRET.elf
```

⚠️ **`-m` 的两个数字是十进制**（Spike 的多区域 parser 只接受十进制）⇒ ⇒ `2147479552` =
`0x7FFF_F000`（Spike 的初始栈页，**必须留着**）· `2147483648` = `0x8000_0000`。

### §11.2 本批的门禁

```bash
# 黄金（改默认 ISA 之后）
perl tools/watchdog.pl 3600 tools/golden-run.sh --keep-going

# 契约层 + 内核
cargo test --workspace --no-fail-fast --lib --tests

# 规格自检 / 台账 / 文档
python3 tools/stage20_check.py docs/detailed-design/STAGE-20_V036_Zicntr_Counters.md
python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md
python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')
```
