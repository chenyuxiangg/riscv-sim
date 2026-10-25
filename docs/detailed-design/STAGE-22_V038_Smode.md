# STAGE-22 · v0.38 · S 模式批 —— 特权级 / S-U 模式 CSR / 委托 / 陷阱路由

> **状态**：冻结。**规格号 STAGE-22**（STAGE-21 已被 v0.37 的 PMP 批占用）。
> **主题**：台账 **D-94** 拆出的**第 ③ 件事** —— S 扩展。
> **实测来源**：仓外探针 `/tmp/v038-probe`，**六轮、约 60 个独立 ELF**。

---

## §0 本批的定位：D-94 的**第 ③ 件事**

台账 D-94 原文把 S 扩展 / PMP / 计数器拆成三件事：

- ① 计数器 ⇒ **v0.36 已闭环**（STAGE-20）。
- ② PMP ⇒ **v0.37 已闭环**（STAGE-21）。
- ③ **S 模式** ⇒ **本批**。

⇒ ⇒ 本批做完，D-94 **整体**才算闭环 ⇒ ⇒ 按「**一条欠账整体未闭环就不减**」的既有口径（v0.36 批确立），本批**可以**参与减法。

### §0.1 本批**做**什么（**六件**）

1. **特权级模型**：M / S / U 三级。
2. **S 模式 CSR**：`stvec` / `sscratch` / `sepc` / `scause` / `stval` / `sstatus` / `sie` / `sip` / `satp`。
3. **U 模式的进入路径**：S 模式的 `sret` 必须能落到 U 模式（**理由见 §0.3**）。
4. **按级可访问性**：S 模式读 M 模式 CSR ⇒ 非法指令；M 模式可读 S/U 模式 CSR。
5. **委托与陷阱路由**：`medeleg` / `mideleg` 决定 S 模式下的陷阱走 S 级还是 M 级。
6. **契约层的 `mstatus` WARL 修正**（**这是台账点名的前置**）：实测出参考侧的真实可写掩码是 `0x807E_79AA`，**不是**现有的 `0x807F_FFEA` ⇒ ⇒ **契约层现有的掩码是个超集，是个真 bug**。

### §0.2 本批**明确不做**的事（**七件**）

1. **不做分页**：`satp` 只支持 **Bare**（MODE = 0）⇒ ⇒ 我们的 `satp` 与参考侧**不可比**（参考侧接受 Sv32）⇒ ⇒ `satp` **不进语料**。
2. **不做 S 级中断**：S 模式下的软件中断 / 定时中断不交付（本平台无 S 级定时器）。
3. **不实现 `time` / `timeh` 的真值**（那是 D-151，要接 M-15 的 CLINT）。
4. **不做 Hypervisor 扩展**（`hstatus` 等）。
5. **不改默认 Golden ISA 串**（理由见 §4.1 裁决 N）。
6. **不动 `rv32mi/*` 语料的任何一条**（本批只**新增**）。
7. **不把 `misa` 做成逐字节可比的判据**（见 §10）。

### §0.3 为什么 S 模式**必须**连 U 模式一起做（**实测推出的结构性结论**）

这一节是本批**范围**的全部依据 ⇒ ⇒ 它不是「顺手多做」，而是**不做就根本对不上**。

**事实一**（F1）：Spike 1.1.1-dev 的 ISA 串**不接受 `s`** ⇒ `spike --isa=rv32imc_..._s` 直接报 `unsupported extension: s` ⇒ ⇒ S 模式**不由 ISA 串开关**。

**事实二**（F2）：Spike 的 `--priv` **只接受 `m` / `mu` / `msu`** ⇒ ⇒ **不存在「有 S 无 U」的组合** ⇒ ⇒ 参考侧**不可能**只开 S 模式。

**事实三**（F8 / F9）：参考侧 `sstatus.SPP` **恒为 0 且写不进去**（在 S 模式写 `sstatus` 只落 `SIE | SPIE`）⇒ ⇒ `sret` 从 S 模式**总是落到 U 模式**（实测：`sret` 之后 `sstatus` 与 `mstatus` **都**变成非法指令 ⇒ 既不是 S 也不是 M ⇒ **是 U**）。

**事实四**（F21）：上游 `rv32si/scall` 与 `rv32si/csr` **都要 `sret` 进 U 模式** ⇒ ⇒ 台账点名的三个上游用例里，**两个结构性依赖 U 模式**。

⇒ ⇒ **结论：S 模式与 U 模式在本平台上是一个不可分的单位** ⇒ ⇒ 只做 S 模式 ⇒ 参考侧的 `sret` 落 U、我们落空 ⇒ ⇒ 任何碰到 `sret` 的用例**必然分歧** ⇒ ⇒ 且 `rv32si/scall` / `rv32si/csr` **永远进不了语料** ⇒ ⇒ ⇒ **本批把 U 模式一起做掉**。

⇒ ⇒ **附带收益**（F3 + F24）：本平台的 `misa` 已经是 `0x4000_1104`（**X 位已置**），参考侧是 `0x4014_1104` ⇒ ⇒ **差的就是 S 位与 U 位** ⇒ ⇒ 本批做完 ⇒ **`misa` 与参考侧逐位相同** ⇒ ⇒ 台账 **D-96**（`rv32mi/csr` 因 U 位缺失而只跑一半）**顺带闭环**。

---

## §1 参考侧语义：**实测**（不是推断）

### §1.1 探针方法论

1. **每个观测一个独立 ELF** ⇒ 退出码**结构上**就是观测值 ⇒ 不存在「打包 bug」。
2. **观测通道 = Spike 的退出码**：探针写 `(value << 1) | 1` 到 `tohost` ⇒ Spike 退出码 = `value`。⚠️ 退出码只有 8 位 ⇒ 更大的值要**取字节**再报。
3. **模式探测器**：`sstatus` 是 **S 模式** CSR ⇒ S 模式可读、M 模式也可读、**U 模式非法** ⇒ ⇒ 「读 `sstatus` 是否合法」能区分 S 与 U，但**区分不了 M 与 S** ⇒ ⇒ 区分 M 与 S 要用 `mstatus`（M 模式可读、S/U 非法）。
4. ⚠️ **非委托陷阱在 S 模式无法观测**（F20）：环境的陷阱向量读 `mcause`（M 模式 CSR）⇒ S 模式读它 ⇒ 非法指令 ⇒ 再次陷阱 ⇒ **挂死** ⇒ ⇒ **S 模式下只有被委托的陷阱可观测** ⇒ ⇒ 探针一律把 `medeleg` 拉满。
5. ⚠️ **不能把「被拒」推出「匹配上了」**（沿用 v0.37 的纪律）。
6. 每个探针**自带对照**：先证明「能读到」再问「读到的是什么」。

### §1.2 二十五条**实测**事实

> 记号 `F1`~`F25`。每条都注明**探针来源**（仓外 `/tmp/v038-probe/roundN.py` 里的探针名）。

| # | 事实 | 探针 |
| --- | --- | --- |
| **F1** | Spike 的 ISA 串**不接受 `s`**：`--isa=rv32imc_..._s` ⇒ `error: bad --isa option ... unsupported extension: s`。⇒ **S 模式由 `--priv` 开关，不是 ISA 串** | `run.sh`（手工扫 `--isa`） |
| **F2** | Spike 的 `--priv` **只接受 `m` / `mu` / `msu`** ⇒ `--priv=ms` 报 `bad --priv option ms` ⇒ ⇒ **不存在「有 S 无 U」** | `run.sh`（手工扫 `--priv`） |
| **F3** | 参考侧 `misa` = **`0x4014_1104`**（逐字节实测：byte0=`0x04`、byte1=`0x11`、byte2=`0x14`、byte3=`0x40`）⇒ **S 位(18) 与 U 位(20) 都置**，**X 位(30) 也置**（无条件） | `r5_misa_byte0..3` |
| **F4** | `misa` 随 `--priv` 变：`m` ⇒ `0x0000_1104`；`mu` ⇒ `0x0010_1104`；`msu` ⇒ `0x4014_1104` | `p01_misa.S` |
| **F5** | `mstatus.MPP` 是 **WARL**，合法值 **{0=U, 1=S, 3=M}**；**写保留值 2 ⇒ 整次写被丢弃、字段保留旧值**（不是合法化到 0）⇒ 证据：清零后写 2 读回 **0**；旧值是 3 时写 2 读回 **3** | `q1_mpp_write0..3`、`r6_mpp2_with_sie` |
| **F6** | 参考侧 `mstatus` 的**真实可写掩码 = `0x807E_79AA`**（写 `0xFFFFFFFF` 后读回：byte3=`0x80`、byte2=`0x7E`、byte1=`0x79`、byte0=`0xAA`）⇒ ⇒ **契约层现有的 `0x807F_FFEA` 是超集**（多出 bit 0/2/4/6/9/10/15/16 —— 正是 RV64-only 与 XS 这些「扩展不存在就不该可写」的位） | `r4_mstatus_warl_*`、`r5_mstatus_byte1/2` |
| **F7** | `mstatus.SPP`（bit 8）**在 M 模式可写**，且与 `sstatus.SPP` 是**同一个字段**（M 模式写 `mstatus` 后 `sstatus` 读到 1） | `r4_m_spp_then_read_sstatus`、`r4_m_spp_then_s_mode_sstatus` |
| **F8** | `sstatus` 的**可写位 = `0x22`**（`SIE`(1) \| `SPIE`(5)）⇒ `SPP` / `SUM` / `MXR` / `SD` **都写不进去**（写 `0xFFFFFFFF` 读回 `0x22`；单独写 `0x100` 读回 0）⇒ ⇒ **`sstatus.SPP` 恒 0** | `r2_s_sstatus_write`、`r3_sstatus_*` |
| **F9** | `sret` 从 S 模式**落到 U 模式**：之后 `sstatus`（cause 2）与 `mstatus`（cause 2）**都非法** ⇒ 既不是 M 也不是 S | `r4_sret_landing`、`r4_sret_landing_mstatus` |
| **F10** | S 模式读 **M 模式 CSR** ⇒ **非法指令（cause 2）**：`mstatus` / `mscratch` / `mcounteren` / `medeleg` 实测全部 cause 2 | `r2_s_mstatus`、`r2_s_mscratch`、`r2_s_mcounteren`、`r5_s_medeleg` |
| **F11** | **M 模式可以读 S 模式 CSR**（`sstatus` 在 M 模式读回 0，不陷阱）⇒ ⇒ 特权层级是**单向**的 | `r3_m_read_sstatus` |
| **F12** | S 模式可读的 S 模式 CSR：`stvec` / `sscratch` / `sepc` / `scause` / `stval` / `sstatus` / `sie` / `sip` / `satp`（全部实测可读） | `r2_s_read_stvec`、`r2_s_satp`、`r2_s_sie`、`r2_s_sip` 等 |
| **F13** | `stvec` / `mtvec` 的 **MODE 字段是 1 位 WARL**（合法 {0 直接, 1 向量}）：写 2 ⇒ 读 0；写 3 ⇒ 读 1 | `r2_stvec_mode0..3`、`r6_mtvec_mode0..3` |
| **F14** | `stvec` / `sscratch` **可写且逐字读回**（写 `0x1234` 读回 `0x1234`；写 `0xdeadbeef` 读回同值） | `q4_s_write_stvec`、`q5_s_sscratch` |
| **F15** | `sepc` **可写**，且 **bit 0 恒 0**（写 `0x1235` 读回 bit0 = 0）⇒ ⇒ **IALIGN = 32** | `r4_sepc_write_lowbit` |
| **F16** | `scause` 与 `stval` 在参考侧**可写**（写 `0x55` / `0x77` 读回同值）⇒ ⇒ **参考侧不实现「只读」**（与规范原文冲突 ⇒ **以实测为准**） | `r3_scause_writable`、`r4_stval_write`、`r3_scause_w2` |
| **F17** | `satp` **可写**，且 **MODE 字段接受 Sv32（=1）** ⇒ ⇒ 参考侧**有分页** ⇒ ⇒ 我们无分页 ⇒ **`satp` 不可比** | `r3_satp_write`、`r4_m_satp_write` |
| **F18** | `sie` / `sip` 的写**受 `mideleg` 门控**：`mideleg.SSIP` 未置 ⇒ 写 `0x2` 读回 0；置位后 ⇒ 写 `0x2` 读回 2 | `r4_sie_with_mideleg`、`r5_sip_mideleg`、`r3_sie_write` |
| **F19** | `cycle` 在 S 模式**默认非法**（cause 2）⇒ 需要 `mcounteren.CY` **且** `scounteren.CY`（只置 `mcounteren` 仍 cause 2；两条都置 ⇒ 可读） | `r3_cycle_s_mcounteren_only`、`r3_cycle_s_with_counters` |
| **F20** | **非委托陷阱在 S 模式无法观测**：环境陷阱向量读 `mcause` ⇒ S 模式读它 ⇒ 非法 ⇒ 再次陷阱 ⇒ **挂死**（`scall` 在 S 模式首测就是 **124 = HANG**）⇒ ⇒ **S 模式下只有被委托的陷阱可观测** | `q6_s_scall`（首测）、`r2_*`（`medeleg` 拉满后全部可观测） |
| **F21** | 上游 `rv32si/sbreak` / `scall` / `csr` 在参考侧**全部 PASS**（rc = 0）⇒ ⇒ 三条都能跑 | 手工构建 `isa/rv32si/*.S` |
| **F22** | 汇编器：`sbreak` / `scall` / `sret` / `csrr sstatus` 在 `-march=rv32i_zicsr_zifencei` 下**全部可汇编**；但 `-march=..._s` 与 `-march=..._sv32` 被 GCC 15 **拒绝** ⇒ ⇒ **语料的 march 串不需要也不允许带 `s`** | 工具链手工验证 |
| **F23** | `mepc` 在 S 模式**不可访问**（cause 2）⇒ ⇒ **从 S 模式无法 `mret` 回 M 模式** ⇒ ⇒ 离开 S 模式的唯一途径是 `sret` | `r5_mret_from_s_mpp`、`r6_mret_from_s_mpp` |
| **F24** | 本平台现有的 `CSR_MISA_VALUE` = `(1<<30)\|(1<<8)\|(1<<12)\|(1<<2)` = `0x4000_1104` ⇒ ⇒ **X 位已置** ⇒ ⇒ 与参考侧**只差 S 位与 U 位** | `crates/contract/src/csr.rs:104` |
| **F25** | `sstatus` 初始值 0（进 S 模式后未写即读回 0）⇒ ⇒ 我们的 `sstatus` 初值必须也是 0 | `r3_sstatus_fresh` |

### §1.3 **四条被推翻的旧结论**（必须写进规格，否则会被再犯）

1. ❌ **旧**：「`mstatus.MPP` 写保留值 2 ⇒ 合法化到 0（U）」⇒ ✅ **实测（F5）**：**整次写被丢弃、字段保留旧值**。⇒ ⇒ 这是**掩码表达不了**的语义 ⇒ ⇒ 必须写成**函数**（§2.3）。
2. ❌ **旧**：「契约层现有的 `MSTATUS_WARL_MASK`（`0x807F_FFEA`）是对的」⇒ ✅ **实测（F6）**：参考侧是 **`0x807E_79AA`** ⇒ ⇒ **契约层现有掩码是超集** ⇒ ⇒ guest 现在能写 `mstatus` 的 RV64-only 位（UIE/HIE/UPIE/HPIE/VS/XS）而参考侧会丢掉 ⇒ ⇒ **真 bug，本批修**。
3. ❌ **旧**：「S 模式 = 只有 S 就行，U 模式是另一件事」⇒ ✅ **实测（F2 + F8 + F21）**：参考侧**不存在「有 S 无 U」**，且 `rv32si/scall` / `rv32si/csr` **结构性依赖 U 模式** ⇒ ⇒ S 与 U 在本平台是**一个不可分的单位**（§0.3）。
4. ❌ **旧**：「`misa` 不可比，因为平台没有 S/U 位」⇒ ✅ **实测（F3 + F24）**：本平台的 X 位**本来就置着** ⇒ ⇒ 本批做完 **`misa` 逐位相同** ⇒ ⇒ **D-96 顺带闭环**。

---

## §2 契约层设计

### §2.1 版本：**两个都不升**（裁决 A）

| 版本 | 值 | 本批 |
| --- | --- | --- |
| `SCHEMA_VERSION`（契约信封） | **7** | **不动** |
| `SIM_STATE_SCHEMA_VERSION`（快照状态） | **2** | **不动** |
| `PLATFORM_SCHEMA_VERSION`（描述形状） | **1** | **不动** |

⇒ ⇒ **零新 DTO** ⇒ ⇒ 前端镜像 `front/lib/message.js` **一字不改** ⇒ ⇒ **不制造 D-143 的存量问题**。

⚠️ **前端不镜像 `SimState` 的字段**：实测前端只在自己的 **mock** 里造 `gpr`，**不解析真实的 `SimState` DTO** ⇒ ⇒ 即使本批要动 `SimState`，前端也不受影响。⇒ ⇒ 但本批**连 `SimState` 都不动**（见 §2.5）。

### §2.2 新增常量（裁决 B：**唯一落点在契约层**）

| 常量 | 值 | 依据 |
| --- | --- | --- |
| `CSR_SSTATUS` | `0x100` | 规范 |
| `CSR_SIE` | `0x104` | 规范 |
| `CSR_STVEC` | `0x105` | 规范 |
| `CSR_SSCRATCH` | `0x140` | 规范 |
| `CSR_SEPC` | `0x141` | 规范 |
| `CSR_SCAUSE` | `0x142` | 规范 |
| `CSR_STVAL` | `0x143` | 规范 |
| `CSR_SIP` | `0x144` | 规范 |
| `CSR_SATP` | `0x180` | 规范 |
| `CSR_MCOUNTEREN` | `0x306` | 规范 |
| `CSR_MEDELEG` | `0x302` | 规范 |
| `CSR_MIDELEG` | `0x303` | 规范 |
| `CSR_SCOUNTEREN` | `0x906` | 规范 |
| `CSR_UCOUNTEREN` | `0x005` | 规范 |
| `MSTATUS_SIE_BIT` | `1` | 规范 |
| `MSTATUS_SPIE_BIT` | `5` | 规范 |
| `MSTATUS_SPP_BIT` | `8` | **F7** |
| `MSTATUS_MPP_SHIFT` | `11` | 规范 |
| `MSTATUS_MPP_U` / `_S` / `_M` | `0` / `1` / `3` | **F5** |
| `MSTATUS_WARL_MASK` | **`0x807E_79AA`** | **F6（实测替换旧的 `0x807F_FFEA`）** |
| `SSTATUS_WARL_MASK` | **`0x22`** | **F8** |
| `CAUSE_SUPERVISOR_ECALL` | `9` | **F19** |
| `CAUSE_USER_ECALL` | `8` | **F19** |
| `CAUSE_BREAKPOINT` | `3` | **F19** |

⚠️ **一个约束一个落点**：内核**只引用**这些常量，**不**另写一份字面量（同 v0.37 的 `pmpcfg` 纪律）。

### §2.3 `mstatus` 的 WARL：**函数**而不是掩码（裁决 C）

⚠️ **实测（F5）推翻了「掩码就够了」的旧模型**：写保留值 2 时，参考侧**整次写被丢弃、字段保留旧值** ⇒ ⇒ 一个 `& mask` **表达不了**这个语义 ⇒ ⇒ 必须写成**函数**：

```rust
/// `mstatus` 的 WARL 合法化。
///
/// ⚠️ **不是掩码** —— 掩码表达不了「保留值 ⇒ 整次写被丢弃」。
pub fn mstatus_warl(old: u32, new: u32) -> u32;
```

规则（**顺序钉死**）：

1. `masked = new & MSTATUS_WARL_MASK`（**实测掩码**，F6）。
2. 取 `masked` 的 MPP 字段（bits 12:11）。
3. 若该字段 ∉ **{0, 1, 3}**（F5）⇒ ⇒ **整次写被丢弃** ⇒ ⇒ **返回 `old`**（不是返回 `masked`）。
4. 否则 ⇒ 返回 `masked`。

⚠️ **为什么是「整次丢弃」而不是「只丢 MPP 字段」**：实测（F5）里写 `0x1802`（MPP=2 且 SIE=1）后读回 **MPP=3**（旧值）⇒ ⇒ 连同一次写里的 **SIE 也没落进去** ⇒ ⇒ 「整次丢弃」是实测口径，不是推测。

⚠️ **`mstatus.SPP` 在 M 模式可写**（F7）⇒ ⇒ 它在掩码里（bit 8 在 `0x79` 内）⇒ ⇒ 由这条规则一并处理。

### §2.4 `sstatus` 的独立写掩码（裁决 D）

- `sstatus` 的**写**掩码 = **`0x22`**（`SIE` | `SPIE`）⇒ ⇒ **`SPP` 写不进去**（F8）。
- `sstatus` 的**读**是 `mstatus` 的**位视图**：SPP 与 `mstatus.SPP` **同一个字段**（F7）⇒ ⇒ M 模式写 `mstatus.SPP` 后，S 模式读 `sstatus.SPP` 能看到。
- ⚠️ `sstatus` 在 **M 模式可读**（F11）⇒ ⇒ M 模式读到的值是 S 级状态。

### §2.5 特权级：**住在 CSR 文件里**（裁决 E）

⚠️ **为什么不是 `SimState` 的新字段**：`SimState` 加字段 ⇒ `SIM_STATE_SCHEMA_VERSION` 2 → 3 ⇒ ⇒ 旧快照全部作废 ⇒ ⇒ **制造存量问题**（D-143 同族）⇒ ⇒ 本批**不造**。

⇒ ⇒ **沿用 v0.37 已确立的形状**（「PMP 状态住在 CSR 文件里 ⇒ 快照天然一致」）：特权级存进 `SimState.csrs` 的**一个伪 CSR 条目**：

- 地址：**`CSR_PRIV = 0x0C0`**（**自定义只读区** 0x000–0x0FF ⇒ 架构上 guest **写它是非法的**）。
- 值：特权级编码（**0 = U、1 = S、3 = M**，与 `mstatus.MPP` 的编码**同构** ⇒ ⇒ 一份字面量）。

**两条保护（缺一不可）**：

| 方向 | 行为 | 理由 |
| --- | --- | --- |
| guest **写** | **忽略**（不落到文件） | 与「未实现 CSR 的写被忽略」**同形** ⇒ ⇒ guest 改不了自己的特权级 |
| guest **读** | **读回 0** | 与「未实现 CSR 读回 0」**同形** ⇒ ⇒ **不引入新的分歧类别**（平台本来就读未实现 CSR 回 0） |

⚠️ **`CSR_PRIV` 必须进 `csr_is_implemented`** ⇒ ⇒ 否则 `m12_engine` 的 `csrs.retain(csr_is_implemented)` 会把它**丢掉** ⇒ ⇒ **快照还原后特权级归零** ⇒ ⇒ 快照**不往返**。⇒ ⇒ 这是「一个约束一个落点」的**硬依赖**，验收项 A-3 专门盯它。

### §2.6 `csr_is_implemented` 的新增项（裁决 F）

补入 §2.2 的 **14 个**新 CSR **+** `CSR_PRIV` ⇒ ⇒ 否则：

- 新 CSR 的写会被**吞掉**（`csr_write_is_ignored`）⇒ ⇒ S 模式 CSR **一排不起作用的寄存器**（v0.37 的原话，裁决 E 的同一条纪律）。
- `CSR_PRIV` 会被快照的 retain **丢掉**（§2.5）。

---

## §3 内核设计

### §3.0 落点（**一个约束一个落点**）

| 落点 | 干什么 |
| --- | --- |
| `crates/contract/src/csr.rs` | 全部常量 + `mstatus_warl` + `csr_is_implemented`（**唯一**落点） |
| `crates/kernel/src/m13_iss/csr.rs` | CSR 的读写路径、按级可访问性、伪 CSR `CSR_PRIV` 的读写保护 |
| `crates/kernel/src/m13_iss/executor.rs` | 特权指令（`mret` / `sret`）、CSR 指令的按级检查、陷阱入口的分级 |
| `crates/kernel/src/m13_iss/mod.rs` | 特权级状态的读写（`priv_get` / `priv_set`） |
| `tests/golden/env/smode/riscv_test.h` | 新 env 变体（**新**） |
| `tests/golden/local/smode_csr.S` | 自包含判据语料（**新**） |
| `tests/golden/corpus.txt` | 语料清单（新增四条） |
| `tests/golden/README.md` | 入库 / 不入库 / 已知偏差的登记 |

### §3.1 特权级模型（裁决 G）

- 三级：**M（3）/ S（1）/ U（0）** ⇒ ⇒ 编码与 `mstatus.MPP` 同构 ⇒ ⇒ **一份字面量**。
- 存放：`state.csrs` 里的 `CSR_PRIV` 条目（§2.5）⇒ ⇒ 快照天然一致。
- 初值：**M**（复位后即机器态）⇒ ⇒ 与 `mret` 的 `MPP` 初值一致。

### §3.2 CSR 的**按级可访问性**（裁决 H）

**唯一入口** = 一张**按级查表**，三条路径（取指无关；读 / 写 / 特权指令）都问它：

| CSR 族 | M 模式 | S 模式 | U 模式 |
| --- | --- | --- | --- |
| M 模式 CSR（`mstatus` / `misa` / `mepc` / `medeleg` / `mcounteren` …） | ✅ | ❌ **非法指令**（F10） | ❌ 非法指令 |
| S 模式 CSR（`sstatus` / `stvec` / `sepc` / …） | ✅（F11） | ✅ | ❌ 非法指令 |
| U 模式 CSR（`cycle` / `time` / `instret`） | ✅ | ✅ **但要过使能链**（F19） | ✅ **但要过使能链** |

⚠️ **M 模式读 S 模式 CSR 不陷阱**（F11）⇒ ⇒ **不是**「统一按级拒」⇒ ⇒ 查表必须**逐 CSR 族**写死，不能用「特权级 ≤ 当前级」的粗判。

### §3.3 陷阱入口（裁决 I）

**唯一的陷阱入口**（沿用 v0.20 的 `enter_trap` 形状）分成两级：

| 陷入时的特权级 | 委托 | 落点 | 状态 CSR |
| --- | --- | --- | --- |
| M | — | `mtvec` | `mcause` / `mepc` / `mtval` |
| S / U | `medeleg[cause]` = 1 | `stvec` | `scause` / `sepc` / `stval` |
| S / U | `medeleg[cause]` = 0 | `mtvec` | `mcause` / `mepc` / `mtval` |

⚠️ **为什么 M 模式不受 `medeleg` 影响**：委托只作用于 S/U 模式下发生的陷阱 ⇒ ⇒ M 模式的陷阱**永远**走 `mtvec` ⇒ ⇒ 这条**不需要**额外实现。

⚠️ **向量模式的位移**：`stvec` 的 MODE 字段是 **1 位**（F13）⇒ ⇒ 向量模式的偏移 = `4 × (cause & 0x3)`（**不是** `0x7`，因为只有 1 位可表示 ⇒ ⇒ 最多 4 项）。⚠️ 这是**实测**口径（与 `mtvec` 现有实现的口径一致 ⇒ ⇒ 两个向量表**同口径**）。

### §3.4 `mret` / `sret`（裁决 J）

| 指令 | 可用级别 | 行为 |
| --- | --- | --- |
| `mret` | **仅 M**（F23：`mepc` 在 S 模式不可访问 ⇒ ⇒ S 模式**根本回不去 M 模式**） | PC ← `mepc`（IALIGN 掩码）；特权级 ← M；`MPP` ← **U(0)**；`MPIE` ← `MIE`；`MIE` ← 1 |
| `sret` | **仅 S** | PC ← `sepc`（IALIGN 掩码）；特权级 ← (`SPP` ? S : U)；`SPP` ← 0；`SPIE` ← `SIE`；`SIE` ← 1 |

⚠️ **`sret` 落 U 模式是**实测**口径**（F9）：参考侧 `sstatus.SPP` 恒 0（F8）⇒ ⇒ `sret` 总是落 U ⇒ ⇒ 我们的 `sstatus` 写掩码也排除 SPP（F8）⇒ ⇒ **默认也是落 U** ⇒ ⇒ **两边一致，无需登记偏差**。

⇒ ⇒ **这是「把 U 模式一起做」最直接的收益**（§0.3）。

### §3.5 指令语义（裁决 K）

| 指令 | M 模式 | S 模式 | U 模式 |
| --- | --- | --- | --- |
| `ecall` | cause **11** | cause **9** | cause **8** |
| `ebreak` / `sbreak` | cause **3** | cause **3** | cause **3** |
| `mret` | ✅ | ❌ 非法指令 | ❌ 非法指令 |
| `sret` | ❌ 非法指令 | ✅ | ❌ 非法指令 |

⚠️ **实测依据**（F19）：`scall` 在 S 模式 ⇒ cause 9；`ebreak` 与 `sbreak` 在 S 模式 ⇒ **都是** cause 3 ⇒ ⇒ **`sbreak` 不是「非法指令」**。

### §3.6 计数器的使能链（裁决 L）

- **M 模式**：直接可读。
- **S 模式**：需要 `mcounteren.CY` **且** `scounteren.CY`（F19）。
- **U 模式**：需要 `mcounteren.CY` **且** `scounteren.CY` **且** `ucounteren.CY`。
- 缺任一 ⇒ ⇒ **非法指令**（不是「读回 0」）⇒ ⇒ **判别力来自「能不能读」，不是「读到什么」**。

---

## §4 工具层设计

### §4.1 新 env 变体 `smode`（裁决 M）

⚠️ **为什么必须新开一个变体，而不是改 `csr`**：

- `csr` env 的判决路径是 **`ecall` → 陷阱向量 → `tohost`**（注释写明「每一条测试都过一遍陷阱路径」）。
- ⚠️ **在 S 模式这条路走不通**（F20）：`ecall` 在 S 模式 ⇒ cause 9 ⇒ **要么**被委托回 S 级处理器（⇒ **死循环**，因为判决又是一次 `ecall`）**要么**落到 M 级陷阱向量（⇒ 那里读 `mcause` ⇒ **非法指令** ⇒ **挂死**）。
- ⇒ ⇒ **S 模式的判决必须是「直接写 `tohost`」** ⇒ ⇒ 与 `csr` env 的形状**不同** ⇒ ⇒ ⇒ **新开 `tests/golden/env/smode/riscv_test.h`** ⇒ ⇒ `csr` env **一字不动** ⇒ ⇒ **M 模式语料零回归**。

`smode` env 的形状：

| 项 | 取值 | 理由 |
| --- | --- | --- |
| 判决 | **直接写 `tohost`**（PASS 写 1、FAIL 写 `(k<<1)\|1`） | F20 ⇒ 判决不能依赖陷阱 |
| `mtvec` | → M 级陷阱向量（只读 `mcause`） | M 模式可读 `mcause` |
| `stvec` | → 测试自己的 `stvec_handler` | S 级陷阱的落点 |
| `medeleg` | **全 1**（`0xFFFF`） | ⇒ S 模式下的陷阱**都**可观测（F20） |
| `mideleg` | 由 `RVTEST_ENABLE_SUPERVISOR` 设 | 沿用上游 |
| `mstatus.MPP` | `init` 钩子设成 **S** ⇒ 复位后的 `mret` **进 S 模式** | 沿用上游 `RVTEST_RV32S` |

### §4.2 语料（裁决 N）

| 条目 | march | env | 状态 | 理由 |
| --- | --- | --- | --- | --- |
| `rv32si/sbreak` | `rv32i_zicsr_zifencei` | `smode` | **active** | 参考侧 PASS（F21）⇒ ⇒ 它**不需要 U 模式** ⇒ ⇒ 可入库 |
| `rv32si/scall` | 同上 | — | **不入库** | 需要 U 模式 ⇒ ⇒ 本批**实现了** U 模式 ⇒ ⇒ **可入库** ⇒ ⇒ 改判（见下） |
| `rv32si/csr` | 同上 | — | **不入库** | 同上 ⇒ ⇒ 改判（见下） |
| `local/smode_csr` | `rv32i_zicsr_zifencei` | `smode` | **新增**（自包含） | 承载 S 模式 CSR 读写 + 按级可访问性 + 委托路由的判别力 |

⚠️ **`rv32si/scall` 与 `rv32si/csr` 的处置**：本批**实现了 U 模式** ⇒ ⇒ 它们的前提**已被满足** ⇒ ⇒ ⇒ **两条都入库**（`active`）⇒ ⇒ 台账点名的三个上游用例**全部入库** ⇒ ⇒ ⇒ 唯一**需要新写**的是 `local/smode_csr`（自包含，测上游测不到的东西：按级可访问性）。

⚠️ **`rv32si/dirty` / `ma_fetch` / `wfi`** ⇒ ⇒ **不入库**（要分页 / 要 S 级中断），理由登记在 `tests/golden/README.md`。

⚠️ **`rv32si/*` 的 march 串**：**不带 `s`**（F22：GCC 15 拒绝 `-march=..._s`）⇒ ⇒ 与既有语料的 march 串同形 ⇒ ⇒ **语料清单的 march 列不用新增取值**。

### §4.3 `tests/golden/README.md`（裁决 O）

- 登记 `rv32si/sbreak` / `scall` / `csr` 三条**入库**、其余三条**不入库**及各自理由。
- 登记 `local/smode_csr` 一节（含失败模式编号的含义）。
- 登记**已知偏差**：`satp` 不可比（F17）、`misa` 的 X 位口径、U 模式与 S 模式的 `mstatus` 视图差异。

---

## §5 验收

### §5.1 A 组 · 契约层

| 编号 | 验收内容 |
| --- | --- |
| A-1 | `mstatus_warl` 的逐条事实断言：写 `0xFFFFFFFF` ⇒ 读回 **`0x807E_79AA`**（F6） |
| A-2 | `mstatus_warl` 的 **MPP 保留值**语义：旧值 0 + 写 MPP=2 ⇒ 结果 **0**；旧值 3 + 写 `0x1802` ⇒ 结果 **`0x1800`**（**整次丢弃**，F5）⇒ ⇒ **不是**「只丢 MPP 字段」 |
| A-3 | `CSR_PRIV` 在 `csr_is_implemented` 里 ⇒ ⇒ 快照往返后**特权级不丢** |
| A-4 | `csr_is_implemented` 认 §2.2 的 14 个新 CSR ⇒ ⇒ 它们的写**不被吞掉** |
| A-5 | `CSR_MISA_VALUE` 含 **S 位(18) 与 U 位(20)** ⇒ ⇒ 值 = **`0x4014_1104`** ⇒ ⇒ 与参考侧**逐位相同**（F3） |
| A-6 | `SSTATUS_WARL_MASK` = **`0x22`**（F8） |
| A-7 | 三个版本常量**都不动**：`SCHEMA_VERSION = 7`、`SIM_STATE_SCHEMA_VERSION = 2`、`PLATFORM_SCHEMA_VERSION = 1` |
| A-8 | `CSR_PRIV` 的读写保护：guest 写 ⇒ **文件里那个条目不变**；guest 读 ⇒ **读回 0** |

### §5.2 B 组 · 内核

| 编号 | 验收内容 |
| --- | --- |
| B-1 | `mret` ⇒ 特权级 M、`MPP` ← 0、`MPIE` ← `MIE`、`MIE` ← 1、PC ← `mepc`（IALIGN 掩码） |
| B-2 | `sret` ⇒ 特权级 ← (`SPP` ? S : U)、`SPP` ← 0、`SPIE` ← `SIE`、`SIE` ← 1、PC ← `sepc` |
| B-3 | `scall` 在 S 模式 ⇒ **cause 9** 且落在 `stvec`（`medeleg` 置位时） |
| B-4 | `sbreak` 在 S 模式 ⇒ **cause 3** |
| B-5 | `ecall` 在 M 模式 ⇒ cause 11；在 S 模式 ⇒ cause 9；在 U 模式 ⇒ cause 8 |
| B-6 | S 模式读 M 模式 CSR ⇒ **cause 2**（非法指令）—— `mstatus` / `mepc` / `medeleg` / `mcounteren` / `misa` 各一条 |
| B-7 | M 模式读 S 模式 CSR ⇒ **不陷阱**，读回 S 级状态（F11） |
| B-8 | `stvec` / `mtvec` 的 MODE 字段是 **1 位 WARL**（写 2 ⇒ 读 0；写 3 ⇒ 读 1）（F13） |
| B-9 | `sepc` 的 **bit 0 恒 0**（IALIGN = 32）（F15） |
| B-10 | `scause` / `stval` **可写**（F16）⇒ ⇒ 与参考侧同形 |
| B-11 | `sie` / `sip` 的写**受 `mideleg` 门控**（F18） |
| B-12 | `cycle` 在 S 模式 ⇒ 需要 `mcounteren.CY` **且** `scounteren.CY`；缺一 ⇒ **cause 2**（F19） |
| B-13 | `satp` 只支持 **Bare**：MODE 字段恒 0 ⇒ ⇒ 写 `0x1` 读回 **0** |
| B-14 | 委托路由：`medeleg[cause]` = 0 ⇒ ⇒ 陷阱落 `mtvec`（`mcause`）；= 1 ⇒ ⇒ 落 `stvec`（`scause`） |
| B-15 | `mret` 在 S 模式 ⇒ **cause 2**（`mepc` 不可访问 ⇒ 非法指令，F23） |

### §5.3 C 组 · 工具层

| 编号 | 验收内容 |
| --- | --- |
| C-1 | `tests/golden/env/smode/riscv_test.h` 存在，且 `csr` env **一字未改** |
| C-2 | `rv32si/sbreak` / `rv32si/scall` / `rv32si/csr` 三条**入库**（`active`）⇒ ⇒ 参考侧 PASS、本平台 PASS |
| C-3 | `local/smode_csr` 存在且**自包含**（不引上游源码） |
| C-4 | 语料清单的 march 列**没有新增取值**（F22） |
| C-5 | `tests/golden/README.md` 登记了三条入库 + 三条不入库 + 已知偏差 |

### §5.4 T 组 · 台账与文档

| 编号 | 验收内容 |
| --- | --- |
| T-1 | 台账 D-94 的末列追加本批处置（③ 闭环）⇒ ⇒ 按「整体未闭环就不减」的口径，**本批之后 D-94 整体闭环** ⇒ ⇒ **参与减法** |
| T-2 | 台账**新登记**本批抓到的真缺口 |
| T-3 | 台账两个口径各自有名字地分开报（历史行 / 标记区），机器统计只数标记区 |
| T-4 | `README.md` / `AGENTS.md` / `docs/progress/PROGRESS.md` 三处同步，**规模数字从当前树重算** |

### §5.5 X 组 · 跨层与判别力

| 编号 | 验收内容 |
| --- | --- |
| X-1 | `misa` 在两侧**逐位相同**（`0x4014_1104`）⇒ ⇒ 这条**只可能**在实现了 S + U 位时成立 ⇒ ⇒ 是一条**强判别力** |
| X-2 | `rv32mi/csr` 的 U 模式段**两侧都跑**⇒ ⇒ **D-96 的空洞被填上** ⇒ ⇒ 陷阱条目数两侧一致 |
| X-3 | 变异自检的三条**红集互不相同**，且三条对照**全 0 红**（§9） |

---

## §6 门禁

本批的门禁 = 既有**十四项**，一条不减：

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --no-fail-fast --lib --tests`
4. `tools/check-front.sh`（前端 + 归属口径 + 三棵树历史回归）
5. `tools/golden-run.sh --self-check`
6. `tools/golden-run.sh --check-mem --keep-going`
7. `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
8. `node tools/browser-verify.mjs`（真 Chrome）
9. `tools/boot-osz.sh`
10. `python3 tools/stage22_check.py docs/detailed-design/STAGE-22_V038_Smode.md`
11. `python3 tools/stage21_check.py …` / `stage20_check.py` / `stage19_check.py` / `stage18_check.py`
12. `python3 tools/ledger_check.py --ledger …` + `--self-test`
13. `python3 tools/doc_check.py --baseline …`
14. `python3 tools/front_test_attrib.py` + `--selftest`

⚠️ 判「全绿」看**退出码** + `grep -cE "^test result: FAILED"` 为 **0**（汇总的失败数在 `awk -F'[ ;]'` 的 **`$7`**，不是 `$6`）。

---

## §7 裁决

### 裁决 A · 三个版本常量**都不动**

`SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、`PLATFORM_SCHEMA_VERSION` 仍 **1** ⇒ ⇒ 零新 DTO ⇒ ⇒ 前端镜像不动 ⇒ ⇒ 不制造 D-143 存量问题。

### 裁决 B · 新增常量**唯一落点在契约层**

契约层 `crates/contract/src/csr.rs` 持有全部 S/U 模式的 CSR 地址常量与位常量 ⇒ ⇒ 内核**只引用**、**不**另写一份字面量（同 v0.37 的 `pmpcfg` 纪律）。

### 裁决 C · `mstatus` 的 WARL 是**函数**（`mstatus_warl(old, new)`）

因为「保留值 ⇒ 整次丢弃」（F5）**掩码表达不了** ⇒ ⇒ 必须带 `old` 参数 ⇒ ⇒ 落在契约层。

### 裁决 D · `sstatus` 有**独立的写掩码** `0x22`

`SPP` 写不进去（F8）⇒ ⇒ 与参考侧同形 ⇒ ⇒ 且 `sstatus` 的读是 `mstatus` 的位视图（SPP 共享，F7）。

### 裁决 E · 特权级**住在 CSR 文件里**（伪 CSR `CSR_PRIV = 0x0C0`）

沿用 v0.37 的形状（「PMP 状态住在 CSR 文件里 ⇒ 快照天然一致」）⇒ ⇒ **不加 `SimState` 字段** ⇒ ⇒ **不升 `SIM_STATE_SCHEMA_VERSION`** ⇒ ⇒ 旧快照不作废。

### 裁决 F · `csr_is_implemented` **必须**补入 14 个新 CSR **+** `CSR_PRIV`

否则新 CSR 的写被**吞掉**（⇒ S 模式 CSR 一排不起作用的寄存器），且 `CSR_PRIV` 会被快照的 retain **丢掉**（⇒ 特权级归零）。

### 裁决 G · 特权级编码与 `mstatus.MPP` **同构**

`MSTATUS_MPP_U` = 0 / `MSTATUS_MPP_S` = 1 / `MSTATUS_MPP_M` = 3 ⇒ ⇒ 一份字面量 ⇒ ⇒ 伪 CSR 里存的就是这个编码。

### 裁决 H · CSR 的按级可访问性是**一张逐族查表**

**不是**「特权级 ≤ 当前级」的粗判（因为 M 模式可读 S 模式 CSR，F11）。

### 裁决 I · 陷阱入口**分两级**

`medeleg[cause]` 决定 S 模式下的陷阱落 `stvec`（`scause`）还是 `mtvec`（`mcause`）⇒ ⇒ M 模式不受委托影响。

### 裁决 J · `mret` **仅 M**、`sret` **仅 S**

`sret` 落 U 模式是**实测**口径（F9）⇒ ⇒ 与参考侧一致，**无需登记偏差**。

### 裁决 K · `scall` 在 S 模式 ⇒ cause **9**；`ebreak` 与 `sbreak` 在 S 模式 ⇒ 都是 cause **3**（F19）。

### 裁决 L · 计数器的使能链

`mcounteren` → `scounteren` → `ucounteren`，缺一 ⇒ **非法指令**（F19）。

### 裁决 M · **新开 `smode` env 变体**，`csr` env **一字不动**

判决改成**直接写 `tohost`**（F20）⇒ ⇒ M 模式语料**零回归**。

### 裁决 N · `rv32si/sbreak` / `rv32si/scall` / `rv32si/csr` **三条全部入库**

U 模式已实现 ⇒ ⇒ 前提满足 ⇒ ⇒ 另新增一条自包含判据 `local/smode_csr`；march 串**不新增取值**（F22）。

### 裁决 O · 已知偏差登记在 `tests/golden/README.md`

`satp` 不可比（F17）等是「已知且有意」的边界 ⇒ ⇒ **不**登记成台账欠账。

---

## §8 施工期订正

> 规格在施工期被实测推翻的内容**逐条记在这里** ⇒ ⇒ 格式与 STAGE-21 的 §8 一致：**订正 N · 标题（初稿判错 ⇒ 已改）+ 原文 + 依据 + 已改成什么**。
>
> ⚠️ **本节现在是空的** —— 规格冻结时尚未发生施工期订正 ⇒ ⇒ 施工期一旦有，**当场补** ⇒ ⇒ `tools/stage22_check.py` 的连续性检查会盯住它。

---

## §9 变异自检（**每 lane 至少一条对照 0 红**）

### §9.1 纪律（沿用，不得放宽）

1. 变异**必须能编译**。
2. 期望集合用 **AND 语义**（全部命中才算红）。
3. **记录完整红集**（不是「有几条红了」）。
4. **还原用 `cp` 备份 + `md5` 比对 + `cmp` 逐字节校验 + `touch`（当前时间，**不是** `touch -r`）** ⇒ ⇒ 禁 `git checkout -- <file>`。
5. 变异期间**不改其它被编译的源文件**。
6. 每条变异**必须有超时**。
7. **每 lane 至少一条「对照变异（语义不变）0 红」**。

### §9.2 计划中的三条变异

| 编号 | 落点 | 预期红 | 目的 |
| --- | --- | --- | --- |
| **M1** | 契约层 `mstatus_warl`：去掉「保留值 ⇒ 整次丢弃」那一步 | 契约层 + 内核 + 黄金 | 验 MPP 的 WARL 真的被实现 |
| **M2** | 内核：S 模式读 M 模式 CSR **不判非法指令** | 内核 + 黄金 | 验按级可访问性真的被实现 |
| **M3** | 内核：`sret` 落 **S 模式**（而不是 U） | 内核 + 黄金 | 验 `sret` 的落点真的被实现 |

⇒ ⇒ **三条红集必须互不相同** ⇒ ⇒ 否则说明有两条走的是同一条路径 ⇒ ⇒ 判别力不成立。

### §9.3 三条对照变异（语义不变 ⇒ 必须 **0 红**）

| 编号 | 落点 | 对照点 |
| --- | --- | --- |
| **C-A** | 契约层 `mstatus_warl`：掩码的**运算顺序**改写（`new & mask` ⇒ `mask & new`） | 语义等价 ⇒ 0 红 |
| **C-B** | 内核：`sret` 的 `SPP ? S : U` 改写成等价的 `if` 形式 | 语义等价 ⇒ 0 红 |
| **C-C** | 判据语料：两条**独立** CSR 写的**顺序对调** | 语义等价 ⇒ 0 红 |

---

## §10 风险与已知边界

| # | 边界 | 处置 |
| --- | --- | --- |
| 1 | **`satp` 不可比**：参考侧有分页（Sv32），我们只有 Bare（F17） | 不进语料；登记在 `tests/golden/README.md` |
| 2 | **`misa` 的 X 位**：参考侧无条件置 1（F3），本平台也置（F24）⇒ ⇒ **本批之后逐位相同** ⇒ ⇒ **不是**偏差 | 验收 X-1 钉住 |
| 3 | **S 级中断不交付** | 本批不做；`rv32si/wfi` 不入库 |
| 4 | **`mepc` 在 S 模式不可访问** ⇒ ⇒ **S 模式回不去 M 模式**（F23） | 这是**架构事实**，不是缺陷；验收 B-15 钉住 |
| 5 | **非委托陷阱在 S 模式无法观测**（F20）⇒ ⇒ 我们的 S 模式处理器**收不到**未委托的陷阱 | 与参考侧**同形**（同一条 env 限制）⇒ ⇒ 不是偏差 |
| 6 | **伪 CSR `CSR_PRIV` 落在 guest 可见的地址空间** | 读回 0 / 写被忽略 ⇒ ⇒ 与「未实现 CSR」**同形** ⇒ ⇒ 不引入新的分歧类别；验收 A-8 钉住 |

---

## §11 复现命令

### §11.1 参考侧探针（本批的**唯一**事实来源）

```bash
# 探针在仓外（/tmp/v038-probe），每个观测一个独立 ELF
cd /tmp/v038-probe
python3 round1.py    # MPP 的 WARL / S 模式的 CSR / 陷阱
python3 round2.py    # medeleg 拉满后的全部 S 模式可观测项
python3 round3.py    # sstatus 位图 / stval / sie / sip / satp / 使能链
python3 round4.py    # SPP 归属 / sret 落点 / mstatus 的 WARL 掩码
python3 round5.py    # misa 逐字节 / mstatus 高字节 / sip 门控
python3 round6.py    # mret 语义 / mtvec 的 MODE / MPP 保留值
```

### §11.2 判据（两侧同程序）

```bash
# ⇒ 期望：spike = PASS   ours = PASS（本批实现前 ours = FAIL 3）
GOLDEN_MANIFEST=... tools/golden-run.sh --keep-going
```

### §11.3 全量门禁

见 §6。

