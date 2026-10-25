# STAGE-22 · v0.38 · S 模式批 —— 特权级 / S-U 模式 CSR / 委托 / 陷阱路由

> **状态**：冻结。**规格号 STAGE-22**（STAGE-21 已被 v0.37 的 PMP 批占用）。
> **主题**：台账 **D-94** 拆出的**第 ③ 件事** —— S 扩展。
> **实测来源**：仓外探针 `/tmp/v038-probe`，**十轮、约 90 个独立 ELF**。

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
6. **契约层的 `mstatus` WARL 修正**（**这是台账点名的前置**）：实测出参考侧的**可写位**掩码是 `0x007E_79AA`，**不是**现有的 `0x807F_FFEA` ⇒ ⇒ **契约层现有的掩码是个超集，是个真 bug**。⚠️ 写全 1 之后**读回**的是 `0x807E_79AA` ⇒ ⇒ 两者**只差 `SD` 那一位**（`SD` 是**推导位**，F40）。

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

**事实三**（F9 / F28）：参考侧 `sret` 的落点**由 `sstatus.SPP` 决定** ⇒ ⇒ 实测 `SPP` = 0 时，`sret` 从 S 模式**落到 U 模式**（实测：`sret` 之后 `sstatus` 与 `mstatus` **都**变成非法指令 ⇒ 既不是 S 也不是 M ⇒ **是 U**，F9）；而委托陷阱会把 `SPP` 置 1（F28）⇒ ⇒ 此时 `sret` **回到 S 模式** ⇒ ⇒ ⚠️ `SPP` 现在**也是客体可写的**（F8）⇒ ⇒ 同一个机制。

**事实四**（F21）：上游 `rv32si/scall` 与 `rv32si/csr` **都要 `sret` 进 U 模式** ⇒ ⇒ 台账点名的三个上游用例里，**两个结构性依赖 U 模式**。

⇒ ⇒ **结论：S 模式与 U 模式在本平台上是一个不可分的单位** ⇒ ⇒ 只做 S 模式 ⇒ 参考侧的 `sret` 落 U、我们落空 ⇒ ⇒ 任何碰到 `sret` 的用例**必然分歧** ⇒ ⇒ 且 `rv32si/scall` / `rv32si/csr` **永远进不了语料** ⇒ ⇒ ⇒ **本批把 U 模式一起做掉**。

⇒ ⇒ **附带收益**（F3 + F24）：本平台的 `misa` 已经是 `0x4000_1104`（**X 位已置**），参考侧是 `0x4014_1104` ⇒ ⇒ **差的就是 S 位与 U 位** ⇒ ⇒ 本批做完 ⇒ **`misa` 与参考侧逐位相同** ⇒ ⇒ 台账 **D-96**（`rv32mi/csr` 因 U 位缺失而只跑一半）**顺带闭环**。

---

## §1 参考侧语义：**实测**（不是推断）

### §1.1 探针方法论

1. **每个观测一个独立 ELF** ⇒ 退出码**结构上**就是观测值 ⇒ 不存在「打包 bug」。
2. **观测通道 = Spike 的退出码**：探针写 `(value << 1) | 1` 到 `tohost` ⇒ Spike 退出码 = `value`。⚠️ 退出码只有 8 位 ⇒ 更大的值要**取字节**再报。
   ⚠️ **这条通道会把 32 位量截断** ⇒ ⇒ 初稿的三个结论（`sstatus` 的写掩码 / 读视图 / `MPP` 的保留值语义）**都是**被它坑到的 ⇒ ⇒ 现在所有 32 位量都**拆成 4 次单字节观测**再拼回来（round13）⇒ ⇒ 并且把「**写掩码**」与「**读视图**」**分开测**（写完去**另一个** CSR 读 ⇒ 得到**写掩码**；在 M 模式把位立起来再读 ⇒ 得到**读视图**）。
3. **模式探测器**：`sstatus` 是 **S 模式** CSR ⇒ S 模式可读、M 模式也可读、**U 模式非法** ⇒ ⇒ 「读 `sstatus` 是否合法」能区分 S 与 U，但**区分不了 M 与 S** ⇒ ⇒ 区分 M 与 S 要用 `mstatus`（M 模式可读、S/U 非法）。
4. ⚠️ **非委托陷阱在 S 模式无法观测**（F20）：环境的陷阱向量读 `mcause`（M 模式 CSR）⇒ S 模式读它 ⇒ 非法指令 ⇒ 再次陷阱 ⇒ **挂死** ⇒ ⇒ **S 模式下只有被委托的陷阱可观测** ⇒ ⇒ 探针一律把 `medeleg` 拉满。
5. ⚠️ **不能把「被拒」推出「匹配上了」**（沿用 v0.37 的纪律）。
6. 每个探针**自带对照**：先证明「能读到」再问「读到的是什么」。

### §1.2 四十一条**实测**事实

> 记号 `F1`~`F41`。每条都注明**探针来源**（仓外 `/tmp/v038-probe/roundN.py` 里的探针名）。
>
> ⚠️ **F26~F41 是施工期补测的**（规格冻结后追加）⇒ ⇒ 它们的原文与判据见 **§8 施工期订正**：订正 9 / 10 讲 `MPP`，订正 11 讲 `sstatus`，订正 12 讲 `medeleg`，订正 13 讲陷阱入口，订正 14 讲 `SD` 的推导语义，订正 15 讲 `sstatus` 写掩码的**逐位**实测。
>
> ⚠️ **round13 的观测名带 `_b0`~`_b3` 后缀**（四个字节各一次独立观测，再拼回 32 位）⇒ ⇒ 正文里写**不带后缀**的聚合名（例：`r13_m_wall_read_mstatus` = `r13_m_wall_read_mstatus_b0`~`_b3`）。

| # | 事实 | 探针 |
| --- | --- | --- |
| **F1** | Spike 的 ISA 串**不接受 `s`**：`--isa=rv32imc_..._s` ⇒ `error: bad --isa option ... unsupported extension: s`。⇒ **S 模式由 `--priv` 开关，不是 ISA 串** | `run.sh`（手工扫 `--isa`） |
| **F2** | Spike 的 `--priv` **只接受 `m` / `mu` / `msu`** ⇒ `--priv=ms` 报 `bad --priv option ms` ⇒ ⇒ **不存在「有 S 无 U」** | `run.sh`（手工扫 `--priv`） |
| **F3** | 参考侧 `misa` = **`0x4014_1104`**（逐字节实测：byte0=`0x04`、byte1=`0x11`、byte2=`0x14`、byte3=`0x40`）⇒ **S 位(18) 与 U 位(20) 都置**，**X 位(30) 也置**（无条件） | `r5_misa_byte0..3` |
| **F4** | `misa` 随 `--priv` 变：`m` ⇒ `0x0000_1104`；`mu` ⇒ `0x0010_1104`；`msu` ⇒ `0x4014_1104` | `p01_misa.S` |
| **F5** | `mstatus.MPP` 是 **WARL**，合法值 **{0=U, 1=S, 3=M}**；写**保留值 2** ⇒ ⇒ **该字段合法化到 0**（U），**同一次写里的其它位照常落进去** ⇒ ⇒ **不是**「保留旧值」，**也不是**「整次写被丢弃」。证据：清零后写 `0x1002`（`MPP = 2` 且 `SIE = 1`）⇒ 读回 **`0x0000_0002`**；**先写 `0x1800`（`MPP = 3`）再写 `0x1002`** ⇒ 读回**仍是 `0x0000_0002`** ⇒ ⇒ 旧值被**覆盖成 0**。⚠️ **`MPP` 字段是 bits 12:11 ⇒ ⇒ `MPP = 2` 写的是 `0x1000`**（`0x1800` 是 `MPP = 3`，**合法** ⇒ ⇒ 拿它测「保留值」**测不到** ⇒ ⇒ 这就是当初测错的根因） | `r13_mpp2_warl`（T8）、`r13_mpp2_after_mpp3`（T9） |
| **F6** | 参考侧 `mstatus` 的**可写位掩码 = `0x007E_79AA`** ⇒ ⇒ 写 `0xFFFFFFFF` 之后**读回** **`0x807E_79AA`**（byte3=`0x80`、byte2=`0x7E`、byte1=`0x79`、byte0=`0xAA`）⇒ ⇒ ⚠️ 两者**只差 `SD` 那一位**（bit 31 是**推导位**，F40）⇒ ⇒ ⇒ **契约层现有的 `0x807F_FFEA` 是超集**（多出 bit 0/2/4/6/9/10/15/16 —— 正是 RV64-only 与 XS 这些「扩展不存在就不该可写」的位；另 bit 31 **不是可写位**而是**推导位**，F40） | `r4_mstatus_warl_*`、`r5_mstatus_byte1/2` |
| **F7** | `mstatus.SPP`（bit 8）**在 M 模式可写**，且与 `sstatus.SPP` 是**同一个字段**（M 模式写 `mstatus` 后 `sstatus` 读到 1） | `r4_m_spp_then_read_sstatus`、`r4_m_spp_then_s_mode_sstatus` |
| **F8** | `sstatus` 的**客体写掩码** = **`0x000C_6122`**（`SIE` \| `SPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR`）⇒ ⇒ **S 模式可以写 `sstatus.SPP`**（S 模式写 `sstatus = 0x100` ⇒ 读回 **`0x0000_0100`**）⇒ ⇒ **与特权规范不同**（特权规范里 `SPP` 只在 M 模式可写）⇒ ⇒ 本平台**对齐参考侧**（黄金对拍的口径就是参考侧） | `r15_bit8_SPP`（逐位实测，F41）、`r13_s_write_spp_only`（T11） |
| **F9** | `sret` 从 S 模式**落到 U 模式**（**`SPP` = 0 时**）：之后 `sstatus`（cause 2）与 `mstatus`（cause 2）**都非法** ⇒ 既不是 M 也不是 S ⇒ ⇒ ⚠️ `SPP` = 1 时落 **S** 模式（F28）⇒ ⇒ 落点**由 `SPP` 决定** | `r4_sret_landing`、`r4_sret_landing_mstatus` |
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
| **F26** | S 模式**写** M 模式 CSR ⇒ **cause 2**（不是「被忽略」）：`csrw mstatus` / `csrw mcounteren` / `csrw misa` 全部 cause 2 ⇒ ⇒ **按级可访问性对读与写同形**。⇒ S/U 模式读写**未实现**的 CSR（`0x0C1` / `fcsr`）⇒ 参考侧也是 **cause 2** ⇒ ⇒ 我们读回 0 ⇒ ⇒ **既有分歧类别**（本批修） | `r7_s_write_mstatus`、`r7_s_write_mcounteren`、`r7_s_write_misa`、`r7_s_read_misa`、`r7_s_write_unimpl`、`r7_s_read_fcsr`、`r7_u_read_sstatus` |
| **F27** | 参考侧 `misa` 在**黄金门禁的 ISA 串**下（`rv32imc_zicsr_zicclsm_zifencei_zicntr`，**不带 `--priv`**）**仍是 `0x4014_1104`** ⇒ ⇒ 门禁跑的就是这个参考侧 ⇒ ⇒ 我们置上 S+U 位**不会**与门禁分歧 | `r8m_b0..3` |
| **F28** | **委托陷阱之后 `sstatus.SPP` = 1** ⇒ ⇒ 陷阱入口对 `sstatus` 的写**不受客体的写掩码约束**（走 `mstatus` 的**原始位操作**）⇒ ⇒ `sret` 从陷阱处理器**回到 S 模式**（而不是 U 模式）⇒ ⇒ ⚠️ 与 F8 **不矛盾**：F8 说的是「客体**能**写 `SPP`」，这条说的是「**平台**也**能**写 `SPP`」⇒ ⇒ 两者是**同一条机制**的两面 | `r9_spp_after_trap` |
| **F29** | `sstatus` 的**读视图** = **`0x800D_E162`**（`SIE` \| `SPIE` \| `UBE` \| `SPP` \| `FS[14:13]` \| `XS[16:15]` \| `SUM` \| `MXR` \| `SD`）⇒ ⇒ **含 `SD`**、含 `SUM`、含 `MXR` ⇒ ⇒ 与特权规范的「S 级可见字段全集」**不同** ⇒ ⇒ 本平台**对齐参考侧**。⚠️ 视图里多出的 `UBE`(bit 6) 与 `XS`(bit 15) 在参考侧**恒为 0**（`mstatus` 的写掩码不含这两处）⇒ ⇒ **不可观测** ⇒ ⇒ 取「在视图里」是为了与参考侧的掩码**逐字一致**（F38 的 `0x800C_6122` = 视图 ∧ `mstatus` 的**可写位**（`0x007E_79AA`）+ **推导出的 `SD`**） | `r13_m_wall_read_sstatus`（T2） |
| **F30** | M 模式写 `sstatus` = `0x22`（`SIE` \| `SPIE`）⇒ 之后 S 模式读 `sstatus` 读回 **`0x22`**（该次写的两位都在写掩码里 ⇒ ⇒ 逐字保留）；S 模式 `csrs sstatus, 2` ⇒ 读回 **2** ⇒ ⇒ **`sstatus` 在 M / S 两级是同一个存储**，且 `SIE` 在 S 模式**可写** ⇒ ⇒ `sstatus` 是 `mstatus` 的**位视图**，不是第二份存储 | `r10_m_write_sstatus`、`r10_s_write_sie` |
| **F31** | `sie` / `sip` / `satp` 在 S 模式的初值**都是 0** | `r10_sie_read`、`r10_sip_read`、`r10_satp_read` |
| **F32** | 汇编器在 `-march=rv32i_zicsr_zifencei` 下把 **`scall` / `sbreak` 汇编成 `ecall` / `ebreak`**（`0x00000073` / `0x00100073`）⇒ ⇒ **不需要新的 `SCALL` / `SBREAK` 操作码** ⇒ ⇒ 现有 `Op::Ecall` / `Op::Ebreak` 只需**按当前特权级选 cause**（11 / 9 / 8，cause 3 恒定）⇒ ⇒ 但 **`sret` 必须新解码**（`0x10200073`，现有解码器把 `0x102` 留给「非法」） | 工具链 `objdump` 逐条 dump |
| **F33** | `sstatus` 的**写掩码在 S 模式与 M 模式相同** ⇒ ⇒ 参考侧**不按特权裁剪** `sstatus` 的写掩码（M 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `mstatus` = **`0x800C_61A2`**；S 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `sstatus` = **`0x800C_6122`** ⇒ ⇒ 两者**只差 `MPIE`（bit 7）**）⇒ ⇒ ⚠️ ⚠️ **但这两个聚合值**推不出**写掩码** ⇒ ⇒ 那多出来的 `MPIE = 1` **不是 `sstatus` 的写带进去的**（它是参考侧 env 初始化留下的）⇒ ⇒ ⇒ 写掩码的真值见 **F41** ⇒ ⇒ 详见**订正 15** | `r13_s_wall_from_m_read_mstatus`（T3）、`r13_s_wall_from_s_read_sstatus`（T4） |
| **F34** | `mstatus` 写全 1 之后的**读回值** = **`0x807E_79AA`** ⇒ ⇒ 与 F6 **逐位一致** ⇒ ⇒ ⚠️ ⚠️ **但「读回值」不等于「可写位掩码」**（`SD` 是**推导位**，F40）⇒ ⇒ ⇒ **可写位掩码是 `0x007E_79AA`** ⇒ ⇒ 详见**订正 14** | `r13_m_wall_read_mstatus`（T1） |
| **F35** | `medeleg` 的**合法位集合** = **`0x0008_B3FE`**（**per-bit WARL**）：写 `medeleg = 0xFFFFFFFF` ⇒ 读回 **`0x0008_B3FE`**；写 `0x0000_FFFF` ⇒ 读回 **`0x0000_B3FE`** ⇒ ⇒ 两者都是「请求值 ∧ **同一个合法位集合**」 | `r13_medeleg_wall`（T5） |
| **F36** | `medeleg` 写 **`0xB109`**（env 实际写的值）⇒ 参考侧存 **`0xB108`**（bit 0「指令地址未对齐」被丢）⇒ ⇒ F35 的**直接推论**（`0xB109` ∧ `0x0008_B3FE` = `0xB108`） | `r13_medeleg_wall`（T5） |
| **F37** | S 模式读 `mstatus` ⇒ **cause 2**（**复核 F10**）——**且**因为该陷阱**未委托**（`medeleg` 的 bit 2 未置）⇒ ⇒ 落 **`mtvec`** 且**在 M 模式处理** ⇒ ⇒ 这正是 **F20** 那个挂死机理的**机制** | `r13_s_read_mstatus`（T10） |
| **F38** | `sstatus` 的**写掩码**与**读视图**的**逐位**推导：M 模式写 `mstatus = 0xFFFFFFFF` ⇒ 读回 `mstatus` = **`0x807E_79AA`**、读回 `sstatus` = **`0x800C_6122`**；M 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `mstatus` = **`0x800C_61A2`**；S 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `sstatus` = **`0x800C_6122`**；S 模式写 `sstatus = 0x100` ⇒ 读回 **`0x0000_0100`** ⇒ ⇒ 写掩码与读视图**分别**得到，且 **M / S 两级的写掩码相同** ⇒ ⇒ ⚠️ ⚠️ **但这些是聚合观测** ⇒ ⇒ 它们**分不清「某一位是可写的」与「某一位本来就在那里」** ⇒ ⇒ ⇒ **写掩码的真值必须逐位实测**（F41）⇒ ⇒ 详见**订正 15** | `r13_m_wall_read_mstatus`（T1）、`r13_m_wall_read_sstatus`（T2）、`r13_s_wall_from_m_read_mstatus`（T3）、`r13_s_wall_from_s_read_sstatus`（T4）、`r13_s_write_spp_only`（T11） |
| **F39** | `mstatus.MPP` 写保留值 2 后的**完整**读回值 = **`0x0000_0002`**（含「先写 `MPP = 3` 再写」的情形）⇒ ⇒ **F5 的完整依据** | `r13_mpp2_warl`（T8）、`r13_mpp2_after_mpp3`（T9） |
| **F40** | `mstatus.SD`（bit 31）是**推导位** = (`FS` == 3) ∨ (`XS` == 3)，**不可直接写**：只写 `FS = 3`（`0x6000`、**不碰 bit 31**）⇒ 读回 **`0x8000_6000`**（`SD` 被**推成 1**）；只写 `FS = 1`（`0x2000`）⇒ 读回 **`0x0000_2000`**（`SD` = 0）；**直接写 bit 31**（`0x8000_0000`）⇒ 读回 **`0x0000_0000`**（**直接写无效**）；先写 `FS = 3` 再写 `FS = 0` ⇒ 读回 **`0x0000_0000`**（`SD` 跟着 `FS` 走）；写全 1 ⇒ 读回 **`0x807E_79AA`**（`FS = 3` ⇒ `SD = 1`，复核）⇒ ⇒ ⇒ **可写位掩码因此是 `0x007E_79AA`**（bit 31 不在里面） | `r14_fs3_only`、`r14_fs1_only`、`r14_sd_only`、`r14_fs3_then_fs0`、`r14_wall`（仓外 `/tmp/v038-probe/round14.py`） |
| **F41** | `sstatus` 的**客体写掩码** = **`0x000C_6122`**（`SIE` \| `SPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR`）。⚠️ **测法必须逐位**：**先把 `mstatus` 清零**（`csrwi mstatus, 0`）⇒ ⇒ 再**单独**写 `sstatus` 的**每一位** ⇒ ⇒ 读回 `mstatus` 看那一位落没落 ⇒ ⇒ **落**：`SIE`(1) / `SPIE`(5) / `SPP`(8) / `FS0`(13) / `FS1`(14) / `SUM`(18) / `MXR`(19)；**不落**：`MIE`(3) / `UBE`(6) / `MPIE`(7) / `MPP0`(11) / `MPP1`(12) / `XS0`(15) / `XS1`(16) / `MPRV`(17) / `TVM`(20) / `TW`(21) / `TSR`(22) / `SD`(31)（`SD` 是推导位，F40）⇒ ⇒ ⇒ **每次观测都没有额外位** ⇒ ⇒ ⇒ ⚠️ **上一版的 `0x804C_61A2` 多了 `MPIE`(7) 与 `TSR`(22) 两位**（差 `0x8040_0080`）⇒ ⇒ **写掩码与读视图恰好对称**（读视图 `0x800D_E162` 去掉 `UBE`(6) / `XS`(15:16) / 推导位 `SD`(31)） | `r15_bit1_SIE`、`r15_bit5_SPIE`、`r15_bit8_SPP`、`r15_bit13_FS0`、`r15_bit14_FS1`、`r15_bit18_SUM`、`r15_bit19_MXR`（**落**）与 `r15_bit3_MIE`、`r15_bit6_UBE`、`r15_bit7_MPIE`、`r15_bit11_MPP0`、`r15_bit12_MPP1`、`r15_bit15_XS0`、`r15_bit16_XS1`、`r15_bit17_MPRV`、`r15_bit20_TVM`、`r15_bit21_TW`、`r15_bit22_TSR`、`r15_bit31_SD`（**不落**）（仓外 `/tmp/v038-probe/round15.py`） |

### §1.3 **八条被推翻的旧结论**（必须写进规格，否则会被再犯）

1. ❌ **旧**：「`mstatus.MPP` 写保留值 2 ⇒ 合法化到 0（U）」⇒ ✅ **实测（F5 + F39）**：**该字段合法化到 0**，**同一次写里的其它位照常落进去**（`SIE` 会落进去）⇒ ⇒ ⚠️ 「**保留旧值**」（订正 9 的口径）**与**「**整次写被丢弃**」（初稿的口径）**两种读法都被推翻** ⇒ ⇒ 这是**掩码表达不了**的语义 ⇒ ⇒ 必须写成**函数**（§2.3）⇒ ⇒ 见**订正 10**。
2. ❌ **旧**：「契约层现有的 `MSTATUS_WARL_MASK`（`0x807F_FFEA`）是对的」⇒ ✅ **实测（F6 + F34 + F40）**：参考侧的**可写位**是 **`0x007E_79AA`**（写全 1 之后**读回** `0x807E_79AA` ⇒ ⇒ 两者**只差 `SD` 那一位**）⇒ ⇒ **契约层现有掩码是超集** ⇒ ⇒ guest 现在能写 `mstatus` 的 RV64-only 位（UIE/HIE/UPIE/HPIE/VS/XS）而参考侧会丢掉 ⇒ ⇒ **真 bug，本批修**。
3. ❌ **旧**：「S 模式 = 只有 S 就行，U 模式是另一件事」⇒ ✅ **实测（F2 + F8 + F21）**：参考侧**不存在「有 S 无 U」**，且 `rv32si/scall` / `rv32si/csr` **结构性依赖 U 模式** ⇒ ⇒ S 与 U 在本平台是**一个不可分的单位**（§0.3）。
4. ❌ **旧**：「`misa` 不可比，因为平台没有 S/U 位」⇒ ✅ **实测（F3 + F24）**：本平台的 X 位**本来就置着** ⇒ ⇒ 本批做完 **`misa` 逐位相同** ⇒ ⇒ **D-96 顺带闭环**。
5. ❌ **旧**：「`sstatus` 的客体写掩码 = `0x22`、读视图 = `0x0000_0122`」⇒ ✅ **实测（F8 / F29 / F33 / F38 / F41）**：写掩码 = **`0x000C_6122`**、读视图 = **`0x800D_E162`** ⇒ ⇒ ⚠️ **根因**：旧的这两个值都来自**被 8 位退出码截断**的观测 ⇒ ⇒ 「写掩码 `0x22`」其实是**聚合值的低字节**、「读视图 `0x122`」是**低 16 位** ⇒ ⇒ ⇒ 本轮把两者**分开测**（写完去**另一个** CSR 读 ⇒ 得到**写掩码**；在 M 模式把位立起来再读 ⇒ 得到**读视图**）⇒ ⇒ ⇒ 由此推出**两条与特权规范的差异**（§10）⇒ ⇒ 见**订正 11**（⚠️ **它的掩码值本身在订正 15 被再次推翻**）。
6. ❌ **旧**：「上游 env 的 `medeleg` 掩码 = `0x11A9`（位 `{0,3,5,7,8,12}`）」⇒ ✅ **实测（F35 / F36）**：参考侧的 `medeleg` 是 **per-bit WARL**，合法位集合 = **`0x0008_B3FE`** ⇒ ⇒ ⚠️ **根因**：初稿把 `CAUSE_LOAD_PAGE_FAULT` / `CAUSE_STORE_PAGE_FAULT` 当成 **5 / 7**，实为 **13 / 15** ⇒ ⇒ 初稿写的 `0x11A9` **错** ⇒ ⇒ 正确值是 **`0xB109`**（env 实际写的值）⇒ ⇒ 参考侧再把它按合法位集合收敛成 **`0xB108`**（bit 0「指令地址未对齐」被丢）⇒ ⇒ 见**订正 12**。
7. ❌ **旧**：「`mstatus.SD` 是**存储位**」⇒ ✅ **实测（F40）**：**推导位** = (`FS` == 3) ∨ (`XS` == 3)，**不可直接写** ⇒ ⇒ ⚠️ 「写全 1 之后读回 `0x807E_79AA`」这条**本身没错**（`FS = 3` ⇒ `SD = 1`）⇒ ⇒ 错的是**把 bit 31 当成「可直接写的位」** ⇒ ⇒ ⇒ **可写位掩码应当是 `0x007E_79AA`** ⇒ ⇒ ⇒ ⚠️ **为什么之前测不出来**：前面几轮探针**全是「写全 1」** ⇒ ⇒ 那种情形下 `SD` 恒为 1 ⇒ ⇒ 「推导」与「存储」两种读法**给不出不同结果** ⇒ ⇒ 平台按**纯存储**实现也照样过 ⇒ ⇒ **假绿** ⇒ ⇒ 见**订正 14**。
8. ❌ **旧**：「`sstatus` 的客体写掩码 = `0x804C_61A2`（= 读视图 + `MPIE` 一位）」⇒ ✅ **实测（F41）**：**`0x000C_6122`** ⇒ ⇒ ⚠️ **错在从聚合值的差反推掩码**：上一轮的写掩码是从两个**聚合观测**的差推出来的 —— 「M 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `mstatus` = `0x800C_61A2`」与「读视图 = `0x800C_6122`」两者**差一位 `MPIE`（bit 7）** ⇒ ⇒ 很自然地推成「写掩码 = `0x804C_61A2`」⇒ ⇒ ⇒ **但那多出来的 `MPIE = 1` 不是 `sstatus` 的写带进去的** ⇒ ⇒ 它是**参考侧 env 初始化留下的**（env 在 `csrwi mstatus, 0` 之后又 `csrs mstatus, MSTATUS_MPP` ⇒ ⇒ `MPIE` 早就是 1）⇒ ⇒ ⇒ 而且我**又**把 `MPIE` OR 进去一次 ⇒ ⇒ ⇒ **错了两位**（`0x804C_61A2` 与真值差 `0x8040_0080` = bit 22 `TSR` + bit 7 `MPIE`）⇒ ⇒ ⇒ ⚠️ **通用判据六**：**凡是从两个聚合观测的差推出一个掩码，必须用逐位实测复核** ⇒ ⇒ **聚合观测分不清「某一位是可写的」与「某一位本来就在那里」** ⇒ ⇒ 见**订正 15**。

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
| `CSR_PRIV` | `0x0C0` | **裁决 E**（自定义只读区） |
| `MSTATUS_SIE_BIT` | `1` | 规范 |
| `MSTATUS_SPIE_BIT` | `5` | 规范 |
| `MSTATUS_SPP_BIT` | `8` | **F7** |
| `MSTATUS_MPP_SHIFT` | `11` | 规范 |
| `MSTATUS_MPP_U` / `_S` / `_M` | `0` / `1` / `3` | **F5** |
| `PRIV_U` / `PRIV_S` / `PRIV_M` | `0` / `1` / `3` | **裁决 G**（与 `MPP` 同构 ⇒ 一份字面量） |
| `MSTATUS_WARL_MASK` | **`0x007E_79AA`** | **F6 + F40**（实测替换旧的 `0x807F_FFEA`）⚠️ **`SD` 是推导位 ⇒ ⇒ 不在可写位里** ⇒ ⇒ 写全 1 之后**读回** `0x807E_79AA`（`FS = 3` ⇒ `SD = 1`）⇒ ⇒ 掩码与读回值**只差 `SD` 那一位** |
| `MSTATUS_FS_MASK` | **`0x3 << 13`** | 规范（`FS` 字段 = bits 14:13） |
| `MSTATUS_XS_MASK` | **`0x3 << 15`** | 规范（`XS` 字段 = bits 16:15） |
| `MSTATUS_SD_BIT` | **`1 << 31`** | **F40**（`SD` = **推导位** = (`FS` == 3) ∨ (`XS` == 3)） |
| `SSTATUS_WARL_MASK` | **`0x000C_6122`** | **F8 / F41**（客体的写掩码：`SIE` \| `SPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR`）⚠️ **上一版的 `0x804C_61A2` 多算了 `TSR`（22）与 `MPIE`（7）两位** ⇒ ⇒ 见**订正 15** |
| `SSTATUS_VIEW_MASK` | **`0x800D_E162`** | **F29 / F38**（读视图：`SIE` \| `SPIE` \| `UBE` \| `SPP` \| `FS[14:13]` \| `XS[16:15]` \| `SUM` \| `MXR` \| `SD`） |
| `MEDELEG_WRITABLE_MASK` | **`0x0008_B3FE`** | **F35 / F36**（`medeleg` 的合法位集合 · per-bit WARL） |
| `CAUSE_ILLEGAL_INSTRUCTION` | `2` | 规范（F26） |
| `CAUSE_BREAKPOINT` | `3` | 规范（F19） |
| `CAUSE_USER_ECALL` | `8` | 规范（F19） |
| `CAUSE_SUPERVISOR_ECALL` | `9` | 规范（F19） |
| `CAUSE_MACHINE_ECALL` | `11` | 规范（F19） |
| `COUNTEREN_CY` / `_TM` / `_IR` | `0` / `1` / `2` | 规范（`mcounteren` / `scounteren` / `ucounteren` 的位） |

⚠️ **一个约束一个落点**：内核**只引用**这些常量，**不**另写一份字面量（同 v0.37 的 `pmpcfg` 纪律）。

### §2.3 `mstatus` 的 WARL：**函数**而不是掩码（裁决 C）

⚠️ **实测（F5 + F39）推翻了「掩码就够了」的旧模型**：写保留值 2 时，参考侧**把该字段合法化到 0**，而**同一次写里的其它位照常落进去** ⇒ ⇒ 一个 `& mask` **表达不了**这个语义 ⇒ ⇒ 必须写成**函数**：

```rust
/// `mstatus` 的 WARL 合法化。
///
/// ⚠️ **不是掩码** —— 掩码表达不了「保留值 ⇒ 该字段合法化到 0」。
pub fn mstatus_warl(new: u32) -> u32;
```

规则（**顺序钉死**）：

1. `masked = new & MSTATUS_WARL_MASK`（**实测掩码**，F6 / F40）。
2. 取 `masked` 的 MPP 字段（bits 12:11）。
3. 若该字段 ∉ **{0, 1, 3}**（保留值 = **2**，F5）⇒ ⇒ **该字段合法化到 0** ⇒ ⇒ **同一次写里的其它位照常落进去**。
4. 否则 ⇒ `MPP` 取新值。
5. 取 `masked` 的 `FS`（bits 14:13）与 `XS`（bits 16:15）。
6. `SD`（bit 31）是**推导位** ⇒ ⇒ **不可直接写** ⇒ ⇒ `SD` = (`FS` == 3) ∨ (`XS` == 3) ⇒ ⇒ 由 `FS` / `XS` **推出来** ⇒ ⇒ 因此它**不在 `MSTATUS_WARL_MASK` 里**（F40）。
7. 返回「掩码后的新值」+「合法化后的 `MPP`」+「**推导出来的 `SD`**」。

⚠️ **为什么没有 `old` 参数**：**合法化到 0** 的语义**不依赖旧值** ⇒ ⇒ 留一个**读不到用处**的参数就是**谎言** ⇒ ⇒ 函数**只吃新值** ⇒ ⇒ ⚠️ 而且留着还会被 clippy 的 `-D warnings` 拦下（未使用的参数）。

⚠️ **凡是写 `mstatus` 的路径都要让 `SD` 跟着被推导** ⇒ ⇒ 只做 `& mask` 的路径**会漏掉它** ⇒ ⇒ 客体的 `sstatus` 写**也要走这条** ⇒ ⇒ 这正是黄金语料 `local/smode_csr` **第 12 条检查**的落点（把 `FS` 写成 3 ⇒ 读回 `sstatus` 的 bit 31 必须是 1）。

⚠️ **「保留旧值」与「整次写被丢弃」两种读法都错**（**订正 10** · **F5 + F39**）：实测写 `0x1002`（`MPP = 2` 且 `SIE = 1`）后的**完整**读回值 = **`0x0000_0002`** ⇒ ⇒ 「先写 `MPP = 3` 再写」也**仍是 `0x0000_0002`** ⇒ ⇒ 旧值被**覆盖成 0** ⇒ ⇒ 「保留旧值」会把 `MPP` 留在 3（读回 `0x1802`）、「整次丢弃」会把 `SIE` 也吞掉 ⇒ ⇒ **两者都与参考侧逐位不等**。

⚠️ **`MPP` 字段是 bits 12:11** ⇒ ⇒ `MPP = 2` 写的是 **`0x1000`**（**不是** `0x1800` —— 那是 `MPP = 3`，**合法** ⇒ ⇒ 拿它测「保留值」**测不到**）⇒ ⇒ 这就是初稿的 F5 证据为什么不足的原因（**订正 9**）。

⚠️ **`mstatus.SPP` 在 M 模式可写**（F7）⇒ ⇒ 它在掩码里（bit 8 在 `0x79` 内）⇒ ⇒ 由这条规则一并处理。

⚠️ **掩码与读回值只差 `SD` 那一位**（**订正 14** · F40）：写全 1 ⇒ 掩码给出 `0x007E_79AA`、而 `FS = 3` ⇒ ⇒ 推导出的 `SD = 1` ⇒ ⇒ **读回** `0x807E_79AA` ⇒ ⇒ ⇒ 这两条**不矛盾**，差的是「**可写位**」与「**最终值**」。

### §2.4 `sstatus` 的独立写掩码（裁决 D）

- `sstatus` 的**客体写**掩码 = **`0x000C_6122`**（`SIE` \| `SPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR`）⇒ ⇒ **S 模式可以写 `sstatus.SPP`**（F8 / F41）⇒ ⇒ 且**写掩码在 S 模式与 M 模式相同**（F33）⇒ ⇒ 参考侧**不按特权裁剪**。
  ⚠️ **写掩码 = 读视图去掉 `UBE`（6）/ `XS`（15:16）/ 推导位 `SD`（31）** ⇒ ⇒ `0x800D_E162` & `~0x0001_8040` = **`0x000C_6122`** ⇒ ⇒ 而 `UBE` 与 `XS` 在参考侧**恒为 0**（`mstatus` 的可写位掩码不含它们）⇒ ⇒ **不可观测** ⇒ ⇒ ⇒ **可观测的部分恰好对称**：读视图里**可观测**的那些位（`SIE` / `SPIE` / `SPP` / `FS[14:13]` / `SUM` / `MXR`）**正好**就是写掩码 ⇒ ⇒ 这是 **F38 与 F41 的交叉印证**（两边独立测出来 ⇒ ⇒ 对上）。
  ⚠️ **写掩码里没有 `SD` 那一位**（`0x000C_6122` 的 bit 31 是 0）⇒ ⇒ 而且 `SD` 本来就是**推导位**（F40）⇒ ⇒ ⇒ **客体的 `sstatus` 写也必须走 `mstatus_warl`**（§2.3）⇒ ⇒ 只做 `& mask` 的路径会漏掉 `SD`（`FS = 3` 时 `SD` 该被推成 1）。
- `sstatus` 的**读**是 `mstatus` 的**位视图**，视图掩码 = **`0x800D_E162`**（`SIE` \| `SPIE` \| `UBE` \| `SPP` \| `FS[14:13]` \| `XS[16:15]` \| `SUM` \| `MXR` \| `SD`）⇒ ⇒
  - `SPP` 与 `mstatus.SPP` **同一个字段**（F7）⇒ ⇒ M 模式写 `mstatus.SPP` 后，S 模式读 `sstatus.SPP` 能看到；
  - 视图**含 `SD`**、含 `SUM`、含 `MXR`（F29）⇒ ⇒ ⚠️ 视图里多出的 `UBE`(bit 6) 与 `XS`(bits 15:16) 在参考侧**恒为 0**（`mstatus` 的可写位掩码不含这两处）⇒ ⇒ **不可观测** ⇒ ⇒ 取「在视图里」是为了与参考侧的掩码**逐字一致**；
  - M 模式写 `sstatus` = `0x22` ⇒ S 模式读回 `0x22`（F30）⇒ ⇒ **同一存储**，不是第二份。
- ⚠️ `sstatus` 在 **M 模式可读**（F11）⇒ ⇒ M 模式读到的值是 S 级状态。
- ⚠️ **平台自己的写**（陷阱入口写 `SPP` / `SPIE` / `SIE`、`sret` 写 `SPP`）**不受客体掩码约束** ⇒ ⇒ 走 `mstatus` 的**原始位操作** ⇒ ⇒ ⇒ 这就是 F28（委托陷阱后 `sstatus.SPP` = 1）的实现方式（**F28 的实现方式没变**）。

⚠️ **与特权规范的两条差异（明示取舍，登记在 §10 与 `tests/golden/README.md`）**：

1. 参考侧**允许 S 模式写 `sstatus.SPP`**（F8）⇒ ⇒ 而特权规范里 `SPP` **只在 M 模式可写** ⇒ ⇒ 本平台**对齐参考侧**（黄金对拍的口径就是参考侧）。
2. `SUM` 与 `MXR` **出现在 `sstatus` 的读视图里**（F29）⇒ ⇒ 而特权规范的「S 级可见字段全集」里**没有**这两位 ⇒ ⇒ 本平台**对齐参考侧**。

### §2.5 特权级：**住在 CSR 文件里**（裁决 E）

⚠️ **为什么不是 `SimState` 的新字段**：`SimState` 加字段 ⇒ `SIM_STATE_SCHEMA_VERSION` 2 → 3 ⇒ ⇒ 旧快照全部作废 ⇒ ⇒ **制造存量问题**（D-143 同族）⇒ ⇒ 本批**不造**。

⇒ ⇒ **沿用 v0.37 已确立的形状**（「PMP 状态住在 CSR 文件里 ⇒ 快照天然一致」）：特权级存进 `SimState.csrs` 的**一个伪 CSR 条目**：

- 地址：**`CSR_PRIV = 0x0C0`**（**自定义只读区** 0x000–0x0FF ⇒ 架构上 guest **写它是非法的**）。
- 值：特权级编码（**0 = U、1 = S、3 = M**，与 `mstatus.MPP` 的编码**同构** ⇒ ⇒ 一份字面量）。

**两条保护（缺一不可）**：

| 方向 | 行为 | 理由 |
| --- | --- | --- |
| guest **写** | **忽略**（不落到文件） | ⚠️ **必须靠 `csr_is_read_only` 显式拦下**（**订正 6**）—— 只靠「与未实现 CSR 同形」是**假的**：裁决 F 要求 `CSR_PRIV` 进 `csr_is_implemented` ⇒ ⇒ 那时 `!csr_is_implemented` 那一条不再成立 ⇒ ⇒ 客体的写会真的落进文件 ⇒ ⇒ **guest 能改自己的特权级** |
| guest **读** | **读回 0** | 与「未实现 CSR 读回 0」**同形** ⇒ ⇒ **不引入新的分歧类别**（平台本来就读未实现 CSR 回 0） |

⚠️ **读回 0 的实现落点**：`csr_read` 对 `CSR_PRIV` **特判**返回 0 ⇒ ⇒ **平台自己读特权级不能走 `csr_read`** ⇒ ⇒ 走 `priv_get`（**直接读 CSR 文件的那个条目**）⇒ ⇒ **两个落点，两条路径，互不干扰**。

⚠️ **`CSR_PRIV` 必须进 `csr_is_implemented`** ⇒ ⇒ 否则 `m12_engine` 的 `csrs.retain(csr_is_implemented)` 会把它**丢掉** ⇒ ⇒ **快照还原后特权级归零** ⇒ ⇒ 快照**不往返**。⇒ ⇒ 这是「一个约束一个落点」的**硬依赖**，验收项 A-3 专门盯它。

⚠️ **`CSR_PRIV` 必须进 `csr_is_read_only`** ⇒ ⇒ 客体的写被拦下（**订正 6**）⇒ ⇒ 而**平台自己的写**走 `csr_write_raw`，不受这条影响（与 `mip` 同一条纪律）。

### §2.6 `csr_is_implemented` 的新增项（裁决 F）

补入 §2.2 的 **14 个**新 CSR **+** `CSR_PRIV` ⇒ ⇒ 否则：

- 新 CSR 的写会被**吞掉**（`csr_write_is_ignored`）⇒ ⇒ S 模式 CSR **一排不起作用的寄存器**（v0.37 的原话，裁决 E 的同一条纪律）。
- `CSR_PRIV` 会被快照的 retain **丢掉**（§2.5）。

⚠️ **`csr_is_read_only` 也要补 `CSR_PRIV`**（**订正 6**）⇒ ⇒ 否则客体的写会真的落进那个条目 ⇒ ⇒ guest 能改自己的特权级 ⇒ ⇒ 这是**另一条**护栏，**不是**上一条。

---

## §3 内核设计

### §3.0 落点（**一个约束一个落点**）

| 落点 | 干什么 |
| --- | --- |
| `crates/contract/src/csr.rs` | 全部常量 + `mstatus_warl` + `csr_is_implemented` + `csr_is_read_only`（**唯一**落点） |
| `crates/kernel/src/m13_iss/csr.rs` | CSR 的读写路径（`medeleg` 的写**必须**过 `MEDELEG_WRITABLE_MASK`）、按级可访问性、伪 CSR `CSR_PRIV` 的读写保护、特权级的读写（`priv_get` / `priv_set`） |
| `crates/kernel/src/m13_iss/executor.rs` | 特权指令（`mret` / `sret`）、CSR 指令的按级检查、陷阱入口的分级、**新解码 `Op::Sret`（`0x102`）** |
| `crates/kernel/src/m13_iss/mod.rs` | 特权级状态的对外读��（`priv_get` / `priv_set`） |
| `crates/kernel/src/m11_symbol/render.rs` | `Op::Sret` 的**两处**渲染（反汇编文本 + 助记符） |
| `tests/golden/env/smode/riscv_test.h` | 新 env 变体（**新**） |
| `tests/golden/local/smode_csr.S` | 自包含判据语料（**新**） |
| `tests/golden/local/mstatus_warl.S` | 自包含判据语料（**新**，**订正 9**）⇒ ⇒ 承载 `MPP` 保留值的语义 |
| `tests/golden/corpus.txt` | 语料清单（新增四条） |
| `tools/golden-run.sh` | env 变体白名单加 `smode`（**唯一**允许的第三种取值） |
| `tests/golden/README.md` | 入库 / 不入库 / 已知偏差的登记 |

### §3.1 特权级模型（裁决 G）

- 三级：**M（3）/ S（1）/ U（0）** ⇒ ⇒ 编码与 `mstatus.MPP` 同构 ⇒ ⇒ **一份字面量**。
- 存放：`state.csrs` 里的 `CSR_PRIV` 条目（§2.5）⇒ ⇒ 快照天然一致。
- 初值：**M**（复位后即机器态）⇒ ⇒ 与 `mret` 的 `MPP` 初值一致。

### §3.2 CSR 的**按级可访问性**（裁决 H）

**唯一入口** = 一张**按级查表**，三条路径（取指无关；读 / 写 / 特权指令）都问它：

⚠️ **要问两问**（**订正 5**）：① 这个 CSR **实现了吗**？② 实现了的话，**当前级别**能访问吗？

**第一问 · 实现了吗**

| | M 模式 | S 模式 | U 模式 |
| --- | --- | --- | --- |
| **未实现**的 CSR | 读回 **0** / 写被忽略（既有行为，D-92） | **cause 2**（F26） | **cause 2**（F26） |

**第二问 · 实现了的话，按族分**

| CSR 族 | M 模式 | S 模式 | U 模式 |
| --- | --- | --- | --- |
| M 模式 CSR（`mstatus` / `misa` / `mepc` / `medeleg` / `mcounteren` …） | ✅ | ❌ **非法指令**（F10） | ❌ 非法指令 |
| S 模式 CSR（`sstatus` / `stvec` / `sepc` / …） | ✅（F11） | ✅ | ❌ 非法指令 |
| U 模式 CSR（`cycle` / `time` / `instret`） | ✅ | ✅ **但要过使能链**（F19） | ✅ **但要过使能链** |

⚠️ **M 模式读 S 模式 CSR 不陷阱**（F11）⇒ ⇒ **不是**「统一按级拒」⇒ ⇒ 查表必须**逐 CSR 族**写死，不能用「特权级 ≤ 当前级」的粗判。

⚠️ **读与写同形**（F26）：S 模式**写** M 模式 CSR 也是 cause 2（不是「被忽略」）⇒ ⇒ 六条 CSR 指令的读写路径用**同一张表**。

⚠️ **计数器的使能链**（裁决 L）：S 模式需 `mcounteren` **且** `scounteren`；U 模式再需 `ucounteren` ⇒ ⇒ 缺一 ⇒ **cause 2**。位与计数器的对应：`cycle`/`cycleh` ⇒ `CY`(0)、`time`/`timeh` ⇒ `TM`(1)、`instret`/`instreth` ⇒ `IR`(2)。M 模式**不需要**使能链。

### §3.3 陷阱入口（裁决 I）

**唯一的陷阱入口**（沿用 v0.20 的 `enter_trap` 形状）分成两级：

| 陷入时的特权级 | 委托 | 落点 | **处理时的特权级** | 状态 CSR |
| --- | --- | --- | --- | --- |
| M | — | `mtvec` | **M**（特权级 ← M） | `mcause` / `mepc` / `mtval` |
| S | `medeleg[cause]` = 1 | `stvec` | **S**（特权级 ← S） | `scause` / `sepc` / `stval` |
| S | `medeleg[cause]` = 0 | `mtvec` | **M**（特权级 ← M） | `mcause` / `mepc` / `mtval` |
| U | `medeleg[cause]` = 1 | `stvec` | **S**（特权级 ← S） | `scause` / `sepc` / `stval` |
| U | `medeleg[cause]` = 0 | `mtvec` | **M**（特权级 ← M） | `mcause` / `mepc` / `mtval` |

⚠️ **「处理时的特权级」这一列是必须的**（**订正 13** · F37）：**未委托** ⇒ ⇒ 落 `mtvec` ⇒ ⇒ **特权级 ← M**；**已委托** ⇒ ⇒ 落 `stvec` ⇒ ⇒ **特权级 ← S** ⇒ ⇒ 这正是 **F20** 那个挂死机理的**机制**（S 模式读 `mstatus` ⇒ cause 2 ⇒ 而该陷阱**未委托** ⇒ ⇒ 落 `mtvec` ⇒ ⇒ **在 M 模式处理** ⇒ ⇒ 陷阱向量读 `mcause` 是**合法**的）。

⚠️ **为什么 M 模式不受 `medeleg` 影响**：委托只作用于 S/U 模式下发生的陷阱 ⇒ ⇒ M 模式的陷阱**永远**走 `mtvec` ⇒ ⇒ 这条**不需要**额外实现。

⚠️ **委托陷阱的状态位**（F28）：
- `sstatus.SPP` ← **陷入前的级别**（S 模式陷入 ⇒ `SPP` = 1 ⇒ ⇒ `sret` 回到 **S** 模式；U 模式陷入 ⇒ `SPP` = 0 ⇒ ⇒ 回到 **U** 模式）⇒ ⇒ 这**不受**客体写掩码约束（**平台自己的写**）。
- `sstatus.SPIE` ← 旧的 `SIE`；`sstatus.SIE` ← 0。
- 未委托的陷阱（落 `mtvec`）⇒ ⇒ `mstatus.MPP` ← 陷入前的级别。

⚠️ **中断 cause 的委托判定**：中断 cause 带 **`IRQ_BASE` 高位**（`0x8000_0000`）⇒ ⇒ 判委托**先剥掉高位**，且**只有 `cause < 64`** 才查 `medeleg` ⇒ ⇒ ⇒ 委托位表查的是**架构 cause 号**，不是那个带高位的值。

⚠️ **向量模式的位移**：偏移 = `4 × (cause & 0x7)` ⇒ ⇒ 与**既有 `mtvec` 实现的口径一致**（`crates/kernel/src/m13_iss/csr.rs` 的 `enter_trap` 用的就是 `base + 4 × (cause & 0x7)`）⇒ ⇒ **两个向量表同口径** ⇒ ⇒ ⚠️ **不是** `cause & 0x3`。⚠️ `stvec` 的 MODE 字段仍是 **1 位 WARL**（F13）⇒ ⇒ 那 1 位只决定「**直接 / 向量**」，**不**决定索引能取几项 ⇒ ⇒ 索引口径跟 `mtvec` 取 `cause & 0x7`。

### §3.4 `mret` / `sret`（裁决 J）

| 指令 | 可用级别 | 行为 |
| --- | --- | --- |
| `mret` | **仅 M**（F23：`mepc` 在 S 模式不可访问 ⇒ ⇒ S 模式**根本回不去 M 模式**） | PC ← `mepc`（IALIGN 掩码）；特权级 ← **旧的 `MPP`**；`MPP` ← **U(0)**；**`MIE` ← 旧的 `MPIE`**；**`MPIE` ← 1** |
| `sret` | **仅 S** | PC ← `sepc`（IALIGN 掩码）；特权级 ← (`SPP` ? S : U)；`SPP` ← 0；**`SIE` ← 旧的 `SPIE`**；**`SPIE` ← 1** |

⚠️ **`mret` 的特权级来自「旧的 `MPP`」** ⇒ ⇒ 必须**先读旧值再清** ⇒ ⇒ `MPP` ← 0 的那一步不能影响这一次读取。

⚠️ **两处方向相反是**对的**，不要「统一」**（**订正 13**）：**陷阱入口**那侧是「`MPIE` ← `MIE`，然后 `MIE` ← 0」（`sstatus` 同理：`SPIE` ← `SIE`，然后 `SIE` ← 0）⇒ ⇒ **返回**那侧是「`MIE` ← **旧 `MPIE`**、`MPIE` ← **1**」（`sret` 同理）⇒ ⇒ 这与**特权规范一致**、也与**本仓既有实现一致** ⇒ ⇒ ⚠️ 初稿把陷阱入口那侧的箭头照抄后改了常量 ⇒ ⇒ 那是**笔误**。

⚠️ **`sret` 的落点由 `SPP` 决定**（**实测**口径）：`SPP` = 0 ⇒ **U 模式**（F9：`sret` 之后 `sstatus` 与 `mstatus` **都**变成非法指令）；`SPP` = 1 ⇒ **S 模式**（F28：委托陷阱把 `SPP` 置 1 ⇒ `sret` 回到 S 模式）⇒ ⇒ 我们的规则与参考侧**同口径** ⇒ ⇒ **两边一致，无需登记偏差**。⚠️ 但**谁能把 `SPP` 写成 1** 与特权规范不同（见 §10）⇒ ⇒ 那一条**登记**、这条**不登记**。

⚠️ **但从陷阱处理器 `sret` 会回到 S 模式**（F28）⇒ ⇒ 因为**平台自己的写**把 `SPP` 置成了 1 ⇒ ⇒ ⇒ 两条路径的 `SPP` 初值不同 ⇒ ⇒ **不是矛盾**。

⇒ ⇒ **这是「把 U 模式一起做」最直接的收益**（§0.3）。

### §3.5 指令语义（裁决 K）

| 指令 | M 模式 | S 模式 | U 模式 |
| --- | --- | --- | --- |
| `ecall` | cause **11** | cause **9** | cause **8** |
| `ebreak` / `sbreak` | cause **3** | cause **3** | cause **3** |
| `mret` | ✅ | ❌ 非法指令 | ❌ 非法指令 |
| `sret` | ❌ 非法指令 | ✅ | ❌ 非法指令 |

⚠️ **实测依据**（F19）：`scall` 在 S 模式 ⇒ cause 9；`ebreak` 与 `sbreak` 在 S 模式 ⇒ **都是** cause 3 ⇒ ⇒ **`sbreak` 不是「非法指令」**。

⚠️ **`scall` / `sbreak` 不需要新操作码**（**订正 8** · F32）：工具链在语料的 march（`rv32i_zicsr_zifencei`）下把 `scall` / `sbreak` **汇编成 `ecall` / `ebreak`** ⇒ ⇒ 上游语料的二进制里**没有** `SCALL` / `SBREAK` 操作码 ⇒ ⇒ ⇒ 现有 `Op::Ecall` / `Op::Ebreak` **按当前特权级选 cause** 就够（11 / 9 / 8；`ebreak` 恒 3）⇒ ⇒ **少一整族新操作码**。

⚠️ **`sret` 必须新解码**：`0x10200073` ⇒ ⇒ 现有解码器把 `(funct3=000, imm=0x102)` 留给「非法」⇒ ⇒ 本批加 `Op::Sret`。⚠️ **同时** `m11_symbol/render.rs` 的**两处**渲染要补 `sret`（反汇编文本 + 助记符表）⇒ ⇒ 否则符号服务会把 `sret` 打成未知。

### §3.6 计数器的使能链（裁决 L）

- **M 模式**：直接可读。
- **S 模式**：需要 `mcounteren.CY` **且** `scounteren.CY`（F19）。
- **U 模式**：需要 `mcounteren.CY` **且** `scounteren.CY` **且** `ucounteren.CY`。
- 缺任一 ⇒ ⇒ **非法指令**（不是「读回 0」）⇒ ⇒ **判别力来自「能不能读」，不是「读到什么」**。

---

## §4 工具层设计

### §4.1 新 env 变体 `smode`（裁决 M）

⚠️ **为什么必须新开一个变体，而不是改 `csr`**（**订正 1 / 2**）：

- `csr` env 的判决路径是 **`ecall` → 陷阱向量 → `tohost`**。
- ⚠️ **初稿以为「S 模式这条路走不通」**（F20）—— **判错了**：F20 那条探针的前提是 `medeleg` 被**拉满**（`0xFFFF`）；而**上游 env 的 `medeleg` 掩码是 `0xB109`**（位 `{0,3,8,12,13,15}`）⇒ ⇒ **位 9 不在其中** ⇒ ⇒ S 模式的 `ecall`（cause 9）**落到 `mtvec` 的 M 级陷阱向量** ⇒ ⇒ 那里读 `mcause` 是**合法**的 ⇒ ⇒ **不死循环、也不挂死**。
- ⇒ ⇒ **真正的理由是「假绿」**：`csr` env 的 M 级陷阱向量把 `CAUSE_USER_ECALL`(8) / `CAUSE_SUPERVISOR_ECALL`(9) / `CAUSE_MACHINE_ECALL`(11) **一律当成 PASS** ⇒ ⇒ 于是 **`rv32si/scall` 在 `sret` 错误地落在 S 模式时**：`ecall` 的 cause 是 **9** ⇒ ⇒ 被当成 PASS ⇒ ⇒ **两侧都报 PASS ⇒ ⇒ 这条语料的判别力为零**。
- ⇒ ⇒ **改陷阱向量也不够**：S 级处理器里的 `RVTEST_PASS` 又是一次 `ecall` ⇒ cause 9 ⇒ 又被当成 PASS ⇒ ⇒ **假绿照旧**。
- ⇒ ⇒ **唯一能拆掉假绿的办法是「判决不经 `ecall`」** ⇒ ⇒ 即 **直接写 `tohost`**。

⇒ ⇒ ⇒ **新开 `tests/golden/env/smode/riscv_test.h`** ⇒ ⇒ `csr` env **一字不动** ⇒ ⇒ **M 模式语料零回归**（而且「一字不动」这条约束**有结构性理由**：M 级语料因 `__MACHINE_MODE` 把 `stvec_handler` **别名**成 `mtvec_handler` ⇒ ⇒ `la t0, stvec_handler` = 0 ⇒ ⇒ **`medeleg` 一次都没被写过** ⇒ ⇒ **M 级语料的委托面是零** ⇒ ⇒ 见**订正 4**）。

`smode` env 的形状：

| 项 | 取值 | 理由 |
| --- | --- | --- |
| 判决 | **直接写 `tohost`**（PASS 写 1、FAIL 写 `(k<<1)\|1`） | 拆掉「假绿」（订正 2） |
| `mtvec` → M 级陷阱向量 | **严格**：任何不是客体预期的陷阱 ⇒ 写**失败**判决 ⇒ ⇒ **cause 9 不再被当成 PASS** | 同上 |
| `stvec` | → 测试自己的 `stvec_handler` | S 级陷阱的落点 |
| `medeleg` | **上游的掩码 `0xB109`**（位 `{0,3,8,12,13,15}`） | 与上游 env **逐字同形** ⇒ ⇒ 两侧同一份程序 ⇒ ⇒ 委托面**是**活的（cause 3 / cause 8）⇒ ⇒ ⚠️ **参考侧会把它按合法位集合收敛成 `0xB108`**（bit 0 被丢，F36）⇒ ⇒ 我们也**必须**过 `MEDELEG_WRITABLE_MASK` ⇒ ⇒ 两侧才**逐位相同** |
| `mideleg` | 由 `RVTEST_ENABLE_SUPERVISOR` 设 | 沿用上游 |
| `mstatus.MPP` | `init` 钩子设成 **S** ⇒ 复位后的 `mret` **进 S 模式** | 沿用上游 `RVTEST_RV32S` |

### §4.2 语料（裁决 N）

| 条目 | march | env | 状态 | 理由 |
| --- | --- | --- | --- | --- |
| `rv32si/sbreak` | `rv32i_zicsr_zifencei` | `smode` | **active** | 参考侧 PASS（F21）⇒ ⇒ 它**不需要 U 模式** ⇒ ⇒ 可入库 |
| `rv32si/scall` | 同上 | — | **不入库** | 需要 U 模式 ⇒ ⇒ 本批**实现了** U 模式 ⇒ ⇒ **可入库** ⇒ ⇒ 改判（见下） |
| `rv32si/csr` | 同上 | — | **不入库** | 同上 ⇒ ⇒ 改判（见下） |
| `local/smode_csr` | `rv32i_zicsr_zifencei` | `smode` | **新增**（自包含） | 承载 S 模式 CSR 读写 + 按级可访问性 + 委托路由的判别力 |
| `local/mstatus_warl` | `rv32i_zicsr_zifencei` | `csr` | **新增**（自包含） | 承载 `MPP` 保留值的**真实语义**（**只丢 `MPP` 字段、其它位照落**）⇒ ⇒ **变异 M1 的黄金侧判别力** |

⚠️ **`rv32si/scall` 与 `rv32si/csr` 的处置**：本批**实现了 U 模式** ⇒ ⇒ 它们的前提**已被满足** ⇒ ⇒ ⇒ **两条都入库**（`active`）⇒ ⇒ 台账点名的三个上游用例**全部入库** ⇒ ⇒ ⇒ 唯一**需要新写**的是 `local/smode_csr`（自包含，测上游测不到的东西：按级可访问性）。

⚠️ **委托面在 M 级语料与 S 级语料之间不同**（**订正 4**）：`rv32mi/csr` 与 `rv32mi/scall` 在 `__MACHINE_MODE` 下把 `stvec_handler` **别名**成 `mtvec_handler` ⇒ ⇒ 符号 `stvec_handler` 不存在 ⇒ ⇒ 上游 env 的 `la t0, stvec_handler; beqz t0, 1f; … csrw medeleg, t0` **直接跳走** ⇒ ⇒ **`medeleg` 一次都没被写过** ⇒ ⇒ **M 级语料的委托面是零** ⇒ ⇒ 而 `rv32si/*` 不定义 `__MACHINE_MODE` ⇒ ⇒ `medeleg` = `0xB109` **会**被写上 ⇒ ⇒ 委托面**是**活的（cause 3 / cause 8）⇒ ⇒ 这两条才是本批委托语义的真正判据。

⚠️ **`rv32si/dirty` / `ma_fetch` / `wfi`** ⇒ ⇒ **不入库**（要分页 / 要 S 级中断），理由登记在 `tests/golden/README.md`。

⚠️ **`rv32si/*` 的 march 串**：**不带 `s`**（F22：GCC 15 拒绝 `-march=..._s`）⇒ ⇒ 与既有语料的 march 串同形 ⇒ ⇒ **语料清单的 march 列不用新增取值**。

### §4.3 `tests/golden/README.md`（裁决 O）

- 登记 `rv32si/sbreak` / `scall` / `csr` 三条**入库**、其余三条**不入库**及各自理由。
- 登记 `local/smode_csr` 一节（含失败模式编号的含义）。
- 登记**已知偏差**：`satp` 不可比（F17）、`misa` 的 X 位口径、**`sstatus` 的两条与特权规范的差异**（S 模式可写 `SPP`；`SUM` / `MXR` 出现在读视图里 ⇒ ⇒ §10 第 9 条）。

---

## §5 验收

### §5.1 A 组 · 契约层

| 编号 | 验收内容 |
| --- | --- |
| A-1 | `mstatus_warl` 的逐条事实断言：写 `0xFFFFFFFF` ⇒ 读回 **`0x807E_79AA`**（F6 / F34）⇒ ⇒ ⚠️ 那是**读回值**，**可写位掩码**是 `0x007E_79AA`（F40） |
| A-2 | `mstatus_warl` 的 **MPP 保留值**语义（F5 + F39）：旧值 0 + 写 `0x1002`（`MPP = 2` 且 `SIE = 1`）⇒ 结果 **`0x0002`**；旧值 3 + 写 `0x1002` ⇒ 结果 **`0x0002`**（**不是** `0x1802`）⇒ ⇒ 判据是「**该字段合法化到 0**」 |
| A-3 | `CSR_PRIV` 在 `csr_is_implemented` 里 ⇒ ⇒ 快照往返后**特权级不丢** |
| A-4 | `csr_is_implemented` 认 §2.2 的 14 个新 CSR ⇒ ⇒ 它们的写**不被吞掉** |
| A-5 | `CSR_MISA_VALUE` 含 **S 位(18) 与 U 位(20)** ⇒ ⇒ 值 = **`0x4014_1104`** ⇒ ⇒ 与参考侧**逐位相同**（F3） |
| A-6 | `SSTATUS_WARL_MASK` = **`0x000C_6122`**（F41）⇒ ⇒ **含 `SPP`** ⇒ ⇒ 参考侧**允许 S 模式写 `sstatus.SPP`** ⇒ ⇒ ⚠️ **负向断言**：`MPIE`（7）与 `TSR`（22）**不在掩码里**（上一版的 `0x804C_61A2` 就是多算了这两位 ⇒ ⇒ **订正 15**） |
| A-7 | 三个版本常量**都不动**：`SCHEMA_VERSION = 7`、`SIM_STATE_SCHEMA_VERSION = 2`、`PLATFORM_SCHEMA_VERSION = 1` |
| A-8 | `CSR_PRIV` 的读写保护：guest 写 ⇒ **文件里那个条目不变**；guest 读 ⇒ **读回 0** |
| A-9 | `misa` 在**黄金门禁的 ISA 串**下与参考侧**逐位相同**（F27）⇒ ⇒ 门禁跑的那个参考侧就是 `0x4014_1104` |
| A-10 | `CSR_PRIV` 在 `csr_is_read_only` 里 ⇒ ⇒ 客体的写被**地址规则之外的那一条**拦下（**订正 6**） |
| A-11 | `SSTATUS_VIEW_MASK` = **`0x800D_E162`** ⇒ ⇒ 视图**含 `SD`**、含 `SUM`、含 `MXR`（F29 / F38） |
| A-12 | `MEDELEG_WRITABLE_MASK` = **`0x0008_B3FE`**（F35 / F36）⇒ ⇒ `medeleg` 的写**必须**先过这个掩码 ⇒ ⇒ 写 `0xB109` ⇒ 存 **`0xB108`** |
| A-13 | `mstatus_warl` 的 **`SD` 推导**（F40）：只写 `FS = 3` ⇒ 结果 **`0x8000_6000`**；只写 `FS = 1` ⇒ 结果 **`0x0000_2000`**；直接写 `0x8000_0000` ⇒ 结果 **`0x0000_0000`** ⇒ ⇒ ⇒ `SD` **不可直接写**，它由 `FS` / `XS` **推出来** |

### §5.2 B 组 · 内核

| 编号 | 验收内容 |
| --- | --- |
| B-1 | `mret` ⇒ 特权级取**旧 `MPP`**（`MPP` = 3 ⇒ M；`MPP` = 1 ⇒ S；`MPP` = 0 ⇒ U）、`MPP` ← 0、**`MIE` ← 旧 `MPIE`**、**`MPIE` ← 1**、PC ← `mepc`（IALIGN 掩码） |
| B-2 | `sret` ⇒ 特权级 ← (`SPP` ? S : U)、`SPP` ← 0、**`SIE` ← 旧 `SPIE`**、**`SPIE` ← 1**、PC ← `sepc` |
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
| B-14 | 委托路由：`medeleg[cause]` = 0 ⇒ ⇒ 陷阱落 `mtvec`（`mcause`）、**处理时的特权级 ← M**；= 1 ⇒ ⇒ 落 `stvec`（`scause`）、**处理时的特权级 ← S**（§3.3 · F37） |
| B-15 | `mret` 在 S 模式 ⇒ **cause 2**（`mepc` 不可访问 ⇒ 非法指令，F23） |
| B-16 | **未实现**的 CSR 在 S 模式 ⇒ **cause 2**（F26）⇒ ⇒ 而在 M 模式 ⇒ 读回 0 / 写被忽略（D-92 的既有行为**不变**） |
| B-17 | 委托陷阱之后 `sstatus.SPP` = **1**（F28）⇒ ⇒ `sret` 从陷阱处理器**回到 S 模式** |
| B-18 | `sstatus` 在 M / S 两级是**同一存储**（F30）⇒ ⇒ M 模式写 `sstatus`（`0x22`）⇒ S 模式读回 `0x22` |
| B-19 | `mret` 的特权级来自**旧的 `MPP`** ⇒ ⇒ `MPP` = 1 ⇒ ⇒ 落到 **S 模式**；`MPP` = 0 ⇒ ⇒ 落到 **U 模式** |
| B-20 | `ecall` 在 **U 模式** ⇒ cause **8**（F19 / §3.5） |
| B-21 | `mstatus` 的 `MPP` 保留值语义（F5 + F39）⇒ ⇒ 写保留值 2 ⇒ ⇒ **该字段合法化到 0**、`SIE` **落进去** ⇒ ⇒ **不是**「保留旧值」，**也不是**「整次丢弃」 |
| B-22 | 内核侧：S 模式经 `sstatus` 把 `FS` 写成 **3** ⇒ ⇒ 读回 `sstatus` 的 **bit 31 是 1**（`SD` 由 `FS` 推导，F40）⇒ ⇒ ⇒ **客体的 `sstatus` 写也必须走 `mstatus_warl`**（只做 `& mask` 会漏掉 `SD`） |

### §5.3 C 组 · 工具层

| 编号 | 验收内容 |
| --- | --- |
| C-1 | `tests/golden/env/smode/riscv_test.h` 存在，且 `csr` env **一字未改** |
| C-2 | `rv32si/sbreak` / `rv32si/scall` / `rv32si/csr` 三条**入库**（`active`）⇒ ⇒ 参考侧 PASS、本平台 PASS |
| C-3 | `local/smode_csr` 存在且**自包含**（不引上游源码） |
| C-4 | 语料清单的 march 列**没有新增取值**（F22） |
| C-5 | `tests/golden/README.md` 登记了三条入库 + 三条不入库 + 已知偏差 |
| C-6 | `local/mstatus_warl` 存在且**自包含** ⇒ ⇒ 变异 **M1** 在**黄金侧**有判别力（**订正 9 / 订正 10**） |

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
| X-4 | ⚠️ **本批最强的一条**：`rv32mi/csr` 的 U 模式段**两侧都真的跑了**（**不是**假绿）⇒ ⇒ **判别式**：变异 **M2**（让 S/U 模式读 M 模式 CSR 不判非法）**必须**让 `rv32mi/csr` 变红 ⇒ ⇒ 若它**不红** ⇒ ⇒ 说明 U 模式那段**根本没跑** ⇒ ⇒ **D-96 没有闭环**（**订正 3**） |

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

### 裁决 C · `mstatus` 的 WARL 是**函数**（`mstatus_warl(new)`）

因为「保留值 ⇒ **该字段合法化到 0**」（F5 + F39）**掩码表达不了** ⇒ ⇒ 必须写成**函数** ⇒ ⇒ ⚠️ **签名只吃新值**（**合法化到 0 的语义不依赖旧值** ⇒ ⇒ 留一个 `old` 参数就是**谎言** ⇒ ⇒ 还会被 clippy 的 `-D warnings` 拦下）⇒ ⇒ 落在契约层。

### 裁决 D · `sstatus` 有**独立的写掩码** `0x000C_6122`

客体的写掩码 = **`0x000C_6122`**（`SIE` \| `SPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR`；F8 / F41）⇒ ⇒ **含 `SPP`** ⇒ ⇒ **与参考侧同形**（⚠️ 由此产生的**两条与特权规范的差异**见 §10）⇒ ⇒ 且 `sstatus` 的读是 `mstatus` 的位视图（视图掩码 `0x800D_E162`，F29 / F38；`SPP` 共享，F7）。⚠️ **上一版的 `0x804C_61A2` 多算了 `MPIE` 与 `TSR` 两位** ⇒ ⇒ 见**订正 15**。

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

### 订正 1 · §4.1 「S 模式的判决不能经陷阱」——**理由不成立**（⇒ 改成「假绿」）

- **初稿原文**：「⚠️ **在 S 模式这条路走不通**（F20）：`ecall` 在 S 模式 ⇒ cause 9 ⇒ **要么**被委托回 S 级处理器（⇒ **死循环**，因为判决又是一次 `ecall`）**要么**落到 M 级陷阱向量（⇒ 那里读 `mcause` ⇒ **非法指令** ⇒ **挂死**）。⇒ ⇒ **S 模式的判决必须是「直接写 `tohost`」**」
- **依据**：**F20 那条探针的前提是 `medeleg` 被拉满（`0xFFFF`）** ⇒ ⇒ 委托位全置 ⇒ cause 9 也被委托 ⇒ 才死循环。而**上游 env 的 `medeleg` 掩码是 `0xB109`**（位 `{0,3,8,12,13,15}` ⇒ ⇒ **位 9 不在其中**）⇒ ⇒ S 模式的 `ecall`（cause 9）**落到 `mtvec` 的 M 级陷阱向量** ⇒ ⇒ 那里读 `mcause` 是**合法**的（M 模式）⇒ ⇒ **不死循环、也不挂死**。⇒ ⇒ **初稿的理由是拿「探针的 `medeleg` 拉满」当成了「env 的 `medeleg`」** ⇒ ⇒ **判错了**。⚠️ **这条订正当时引用的掩码字面量是 `0x11A9`（位 `{0,3,5,7,8,12}`）—— 那个值在订正 12 被推翻**（初稿把两个页错误 cause 当成 5 / 7）⇒ ⇒ **但「位 9 不在其中」这个论证在订正后的位集合上仍然成立**。
- **已改成什么**：**判决仍然改成「直接写 `tohost`」** ⇒ ⇒ 但**理由换掉了**（见订正 2）。⇒ ⇒ §4.1 的表格与正文按此改写。

### 订正 2 · §4.1 「新 env 的真正理由」——**不是「走不通」，是「假绿」**

- **初稿原文**：「⇒ ⇒ **S 模式的判决必须是「直接写 `tohost`」** ⇒ ⇒ 与 `csr` env 的形状**不同** ⇒ ⇒ ⇒ **新开 `tests/golden/env/smode/riscv_test.h`**」
- **依据**：**上游 `csr` env 的 M 级陷阱向量把 `CAUSE_USER_ECALL`(8) / `CAUSE_SUPERVISOR_ECALL`(9) / `CAUSE_MACHINE_ECALL`(11) 三种 `ecall` 一律当成「PASS」**（`trap_vector` 的三个 `beq`）。⇒ ⇒ 于��� **`rv32si/scall` 在 `sret` 错误地落在 S 模式时**：`ecall` 的 cause 是 **9** ⇒ ⇒ 落到 `mtvec` ⇒ ⇒ 被当成 PASS ⇒ ⇒ **两侧都报 PASS ⇒ ⇒ 这条语料的判别力为零**。⇒ ⇒ 而且**改陷阱向量也不够**：S 级处理器里的 `RVTEST_PASS` 又是一次 `ecall` ⇒ cause 9 ⇒ 又被当成 PASS ⇒ ⇒ **假绿照旧**。⇒ ⇒ **唯一能拆掉假绿的办法是「判决不��� `ecall`」** ⇒ ⇒ 即**直接写 `tohost`**。
- **已改成什么**：**新 env 的理由改成「假绿」** ⇒ ⇒ 新的 `smode` env 的 M 级陷阱向量**严格**：任何不是「客体预期的陷阱」都写**失败**判决 ⇒ ⇒ `cause 9` ⇒ ⇒ **红**。⇒ ⇒ 配合「直接写 `tohost`」⇒ ⇒ `rv32si/scall` 才真的能判别 `sret` 的落点。

### 订正 3 · §0.3 / §1.3 的「D-96 顺带闭环」——**机制要说清，否则会被当成空话**

- **初稿原文**：「⇒ ⇒ **附带收益**（F3 + F24 + F27）：本平台的 `misa` 已经是 `0x4000_1104`（**X 位已置**），参考侧是 `0x4014_1104` ⇒ ⇒ **差的就是 S 位与 U 位** ⇒ ⇒ 本批做完 ⇒ ⇒ **`misa` 与参考侧逐位相同** ⇒ ⇒ 台账 **D-96**（`rv32mi/csr` 因 U 位缺失而只跑一半）**顺带闭环**。

⚠️ **「闭环」的准确含义**（施工期补测 · 订正 3）：`rv32mi/csr` 在**参考侧**其实**真的进了 U 模式**并跑了 `TEST_CASE(14)` / `(15)`（两次都应当是非法指令 ⇒ cause 2 ⇒ 参考侧真的处理了它们）；而**我们侧**因为 `misa` 的 U 位**没置** ⇒ ⇒ `beqz a0, finish` **直接跳走** ⇒ ⇒ **那段根本没跑** ⇒ ⇒ **两侧都报 PASS ⇒ ⇒ 这一行今天是「假绿」** ⇒ ⇒ **零判别力**。⇒ ⇒ 本批置上 U 位 **且** 真的实现 U 模式 ⇒ ⇒ 那段才真的跑起来 ⇒ ⇒ **D-96 才算闭环**（判别式见 §5.5 的 **X-4**）。」
- **依据**：`rv32mi/csr` 在**参考侧**其实**真的进了 U 模式**（`misa` 的 U 位置着 ⇒ `beqz a0, finish` **不**跳）⇒ ⇒ 它真的跑了 `TEST_CASE(14)`（U 模式的 `cycle` 读）与 `TEST_CASE(15)`（U 模式的 `sstatus` 读）⇒ ⇒ 两次都应当**非法指令（cause 2）**⇒ ⇒ 参考侧真的处理了它们。而**我们侧**因为 `misa` 的 U 位**没置** ⇒ ⇒ `beqz a0, finish` **直接跳到 `finish`** ⇒ ⇒ **那段根本没跑** ⇒ ⇒ **两侧都报 PASS ⇒ ⇒ 这一行今天是「假绿」** ⇒ ⇒ **零判别力**。⇒ ⇒ 这才是 D-96 的原文「只跑一半」的**准确含义**：参考侧跑满，我们侧只跑一半，而门禁看不出来。
- **已改成什么**：§0.3 的「附带收益」补上这段机制；§5.5 加一条**强判别力**验收 **X-4**：**变异 M2（让 S/U 模式读 M 模式 CSR 不判非法）必须让 `rv32mi/csr` 变红** ⇒ ⇒ 若它**不红** ⇒ ⇒ 说明 U 模式那段**根本没跑** ⇒ ⇒ 那就还是假绿。

### 订正 4 · §4.2 「三条全部入库」——**要写清「为什么 `csr` env 可以一字不动」**

- **初稿原文**：「⇒ ⇒ 本批**实现了 U 模式** ⇒ ⇒ 它们的前提**已被满足** ⇒ ⇒ ⇒ **两条都入库**（`active`）⇒ ⇒ 台账点名的三个上游用例**全部入库**」
- **依据**：`rv32mi/csr` 与 `rv32mi/scall` 在 `__MACHINE_MODE` 下把 `stvec_handler` **别名**成 `mtvec_handler` ⇒ ⇒ **符号 `stvec_handler` 根本不存在** ⇒ ⇒ 上游 env 里那句 `la t0, stvec_handler; beqz t0, 1f; … csrw medeleg, t0` ⇒ ⇒ **`beqz` 直接跳走** ⇒ ⇒ **`medeleg` 一次都没被写过** ⇒ ⇒ **M 级语料的委托面是零** ⇒ ⇒ 这正是 `csr` env 能**一字不动**的原因（而 `rv32si/*` 不定义 `__MACHINE_MODE` ⇒ `stvec_handler` 是真符号 ⇒ ⇒ `medeleg` = `0xB109` 会被写上 ⇒ ⇒ 委托面**是**活的）。⚠️ **这条订正当时引用的掩码字面量是 `0x11A9` —— 那个值在订正 12 被推翻** ⇒ ⇒ 「`medeleg` 会被写上」这个结论**不受影响**。
- **已改成什么**：§4.2 补上这段 ⇒ ⇒ **「`csr` env 一字不动」这条约束从「怕回归」升级为「有结构性理由」**。

### 订正 5 · §3.2 「按级可访问性」——**要区分「未实现」与「实现但级别不对」**

- **初稿原文**：「| M 模式 CSR（…） | ✅ | ❌ **非法指令**（F10） | ❌ 非法指令 |」——只有一张按**族**分的表。
- **依据**：**F26** 补测出**未实现**的 CSR 在 S 模式**也是 cause 2**（参考侧），而**我们侧读回 0** ⇒ ⇒ ⇒ 规则必须**分两问**：① 这个 CSR **实现了吗**？② 实现了的话，**当前级别**能访问吗？⇒ ⇒ **M 模式**：未实现 ⇒ 读回 0 / 写被忽略（既有行为，D-92）；**S / U 模式**：未实现 ⇒ **cause 2**（F26）。
- **已改成什么**：§3.2 的表改成**两问**（族 + 实现与否）；§5.2 加验收 **B-16**。

### 订正 6 · §2.5 「`CSR_PRIV` 的读写保护」——**只靠「未实现 CSR 同形」不够，它必须被显式列为只读**

- **初稿原文**：「| guest **写** | **忽略**（不落到文件） | 与「未实现 CSR 的写被忽略」**同形** |」
- **依据**：契约层的 `csr_write_is_ignored` = `csr_is_read_only(csr) \|\| csr == CSR_MIP \|\| !csr_is_implemented(csr)` ⇒ ⇒ `CSR_PRIV` 若只加进 `csr_is_implemented`（裁决 F 要求）⇒ ⇒ **`!csr_is_implemented` 那一条不成立** ⇒ ⇒ **客体的写会真的落进文件** ⇒ ⇒ **guest 能改自己的特权级** ⇒ ⇒ 「同形」这条理由**在实现层是假的**。
- **已改成什么**：**`CSR_PRIV` 必须同时进 `csr_is_read_only`** ⇒ ⇒ 写被地址规则之外的那一条显式拦下 ⇒ ⇒ 读仍走「读回 0」那条 ⇒ ⇒ 两条保护各自有**独立落点**（不再靠「同形」）。

### 订正 7 · §2.4 「`sstatus` 的读是 `mstatus` 的位视图」——**视图掩码要钉死**（⚠️ **这个掩码在订正 11 被再次推翻**）

- **初稿原文**：「- `sstatus` 的**读**是 `mstatus` 的**位视图**：SPP 与 `mstatus.SPP` **同一个字段**（F7）⇒ ⇒ M 模式写 `mstatus.SPP` 后，S 模式读 `sstatus.SPP` 能看到。」
- **依据**：**F29 / F30** 补测出：M 模式写 `sstatus`（`0x22`）⇒ S 模式读回 `0x22`（⇒ **同一存储**）；M 模式把 `mstatus.SD` 置 1 ⇒ S 模式读 `sstatus` 的 `SD` 位**仍是 0**（⇒ 「视图不含 `SD`」）；S 模式 `csrs sstatus, 2` ⇒ 读回 2（⇒ `SIE` 可写）；委托陷阱后 `sstatus.SPP` = **1**（F28 ⇒ **平台的写不受客体掩码约束**）。
- **已改成什么**：**`SSTATUS_VIEW_MASK = 0x0000_0122`**（`SIE` \| `SPIE` \| `SPP`）钉进契约层；`sstatus` 的**客体写**掩码仍是 `0x22`；**平台自己的写**（陷阱入口 / `sret`）走 `mstatus` 的原始位操作 ⇒ ⇒ `SPP` 由平台写 ⇒ ⇒ `sret` 从陷阱处理器**回到 S 模式**（F28）。⚠️ ⚠️ **这条订正钉进去的两个掩码都在订正 11 被实测推翻** ⇒ ⇒ 「视图不含 `SD`」与「写掩码 `0x22`」**两个都是被 8 位退出码截断的观测** ⇒ ⇒ **现值见 §2.2**。

### 订正 8 · §3.5 「指令语义」——**`scall` / `sbreak` 不需要新操作码**

- **初稿原文**：「| `ecall` | cause **11** | cause **9** | cause **8** |」——只提 `ecall`，**没提 `scall` / `sbreak` 的编码归属**。
- **依据**：**F32**：工具链在 `-march=rv32i_zicsr_zifencei` 下把 `scall` / `sbreak` **汇编成 `ecall` / `ebreak`** ⇒ ⇒ 上游 `rv32si/scall` / `rv32si/sbreak` 的**二进制里根本没有 `SCALL` / `SBREAK` 操作码** ⇒ ⇒ ⇒ 现有 `Op::Ecall` / `Op::Ebreak` **按当前特权级选 cause** 就够了 ⇒ ⇒ **少一整族新操作码** ⇒ ⇒ 但 **`sret`（`0x10200073`）必须新解码**。
- **已改成什么**：§3.5 的表补上「`scall` / `sbreak` **在语料的 march 下就是 `ecall` / `ebreak`**」这一行 ⇒ ⇒ §3.0 的落点表补「解码器新增 `Op::Sret`（`0x102`）」与「`m11_symbol/render.rs` 补 `sret` 的两处渲染」。

### 订正 9 · §1.2 / §2.3 的「`MPP` 保留值 ⇒ **整次写被丢弃**」——**错在两处**（⚠️ **这条订正本身在订正 10 被再次推翻**）

- **初稿原文**：「**写保留值 2 ⇒ 整次写被丢弃、字段保留旧值**（不是合法化到 0）⇒ 证据：清零后写 2 读回 **0**；旧值是 3 时写 2 读回 **3**」以及 §2.3 的「若该字段 ∉ {0,1,3} ⇒ ⇒ **整次写被丢弃** ⇒ ⇒ **返回 `old`**」。
- **依据**：**两条证据都不成立**——① `MPP` 字段是 **bits 12:11** ⇒ `MPP = 2` 写的是 **`0x1000`**，而初稿用的 **`0x1800` 是 `MPP = 3`（合法）** ⇒ ⇒ 拿它测「保留值」**测不到**（`r6_mpp2_with_sie` 实际证明的是「`MPP = 3` 合法」）；② 「整次丢弃」被**补测直接推翻**：清零后写 **`0x1002`**（`MPP = 2` 且 `SIE = 1`）⇒ 参考侧读回低字节 **`0x02`** ⇒ ⇒ **`SIE` 落进去了** ⇒ ⇒ 只有 `MPP` 字段被**保留成旧值**。⚠️ **这条订正自己也没对**：那次观测走的是 **8 位退出码**通道 ⇒ ⇒ 只看得见**低字节** ⇒ ⇒ 「先写 MPP=3 再写」那次读到的 `0x02` 是**截断值**，**不是**完整值 ⇒ ⇒ ⇒ 「保留旧值」这个结论**站不住** ⇒ ⇒ 本轮用不截断的通道补测 ⇒ ⇒ 见**订正 10**。
- **已改成什么**：§1.2 的 F5 改写 + 新增一条补测事实（**本轮已把它改成 F39**）；§2.3 的规则改成「**该字段保留旧值、同一次写里的其它位照常落进去**」；§1.3 的第 1 条同步；验收 **A-2** 改成断言 `0x0002` / `0x1802`；§9.2 的 **M1** 改成「把保留值改成**整次丢弃**」（**当时旧口径才是变异体**）；变异 M1 另配一条**自包含语料** `local/mstatus_warl`（§4.2）⇒ ⇒ 黄金侧也有判别力。⚠️ ⚠️ **这条订正写的口径本身在订正 10 被再次推翻** ⇒ ⇒ 现值见 §2.3。

---

### 订正 10 · F5 + §2.3 + 订正 9 + 验收 A-2 + 变异 M1 ——「`MPP` 保留值 ⇒ **保留旧值**」⇒ 改成「**合法化到 0**」

- **初稿原文**：「`mstatus.MPP` 写保留值 2 ⇒ ⇒ **该字段保留旧值**（不是合法化到 0）⇒ ⇒ 同一次写里的其它位照常落进去」以及 §2.3 的「若该字段 ∉ {0,1,3} ⇒ ⇒ **该字段保留旧值**」与函数签名 `mstatus_warl(old: u32, new: u32) -> u32`。
- **依据**：**F5 + F39** —— 参考侧写保留值 2 后的**完整**读回值 = **`0x0000_0002`**：清零后写 `0x1002`（`MPP = 2` 且 `SIE = 1`）⇒ 读回 `0x0000_0002`；**先写 `0x1800`（`MPP = 3`）再写 `0x1002`** ⇒ 读回**仍是 `0x0000_0002`** ⇒ ⇒ 旧值被**覆盖成 0** ⇒ ⇒ 「保留旧值」被推翻。⚠️ **根因**：旧的那次观测走的是 **8 位退出码**通道 ⇒ ⇒ 只看得见**低字节**（`0x02`）⇒ ⇒ 把**截断值**当成了完整值 ⇒ ⇒ 结论错了 ⇒ ⇒ 本轮改用**不截断**的通道（4 次单字节观测拼回 32 位）重测。
- **已改成什么**：§1.2 的 F5 改写 + 新增 **F39**；§2.3 的规则改成「**该字段合法化到 0**、同一次写里的其它位照常落进去」；函数签名收敛成 **`mstatus_warl(new: u32) -> u32`**（**合法化到 0 的语义不依赖旧值** ⇒ ⇒ 留一个**读不到用处**的 `old` 参数就是**谎言** ⇒ ⇒ 还会被 clippy 的 `-D warnings` 拦下）；§1.3 的第 1 条同步；验收 **A-2** 改成断言 `0x0002` / `0x0002`（**不是** `0x1802`）、**B-21** 同步；§9.2 的 **M1** 改成「把『合法化到 0』改成『保留旧值』」（**现在旧口径才是变异体**）。

### 订正 11 · F8 + F29 + §2.4 + 订正 7 + 验收 A-6 / A-11 —— `sstatus` 的写掩码与读视图改成**实测值**

- **初稿原文**：「`sstatus` 的**客体写**掩码 = **`0x22`**（`SIE` \| `SPIE`）⇒ ⇒ **`SPP` 写不进去**（F8）」与「视图掩码 = **`0x0000_0122`**（`SIE` \| `SPIE` \| `SPP`）⇒ ⇒ 视图**不含 `SD`**（F29）」。
- **依据**：**F8 / F29 / F33 / F38** —— 客体写掩码 = **`0x804C_61A2`**（`SIE` \| `SPIE` \| `MPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR` \| `SD`）、读视图 = **`0x800D_E162`**（`SIE` \| `SPIE` \| `UBE` \| `SPP` \| `FS[14:13]` \| `XS[16:15]` \| `SUM` \| `MXR` \| `SD`），且**写掩码在 S 模式与 M 模式相同**。⚠️ **根因**：旧的两个值都来自**被 8 位退出码截断**的观测 ⇒ ⇒ 「写掩码 `0x22`」其实是**聚合值的低字节**、「读视图 `0x122`」是**低 16 位** ⇒ ⇒ ⇒ 本轮把两者**分开测**（写完去**另一个** CSR 读 ⇒ 得到**写掩码**；在 M 模式把位立起来再读 ⇒ 得到**读视图**）。⚠️ ⚠️ **这条订正钉进去的写掩码 `0x804C_61A2` 本身在订正 15 被再次推翻**（真值是 `0x000C_6122`）⇒ ⇒ **历史叙述保留** ⇒ ⇒ **现值见 §2.2**。
- **已改成什么**：§1.2 的 F8 / F29 / F33 改写 + 新增 **F38**；§2.2 的两个常量改成 **`0x804C_61A2`** / **`0x800D_E162`**（⚠️ **前一个值在订正 15 被再次推翻**）；§2.4 整节改写并写明**两条与特权规范的差异**；§1.3 补第 5 条；验收 **A-6** / **A-11** 同步；§10 补两条**明示取舍**。

### 订正 12 · F35 + F36 + §2.2 —— `medeleg` 的掩码：`0x11A9` ⇒ 改成**合法位集合 `0x0008_B3FE`**

- **初稿原文**：「上游 env 的 `medeleg` 掩码是 **`0x11A9`**（位 `{0,3,5,7,8,12}`）」（§4.1 / §4.2）。
- **依据**：**F35 / F36** —— 参考侧的 `medeleg` 是 **per-bit WARL**：写 `0xFFFFFFFF` ⇒ 读回 **`0x0008_B3FE`**；写 `0x0000_FFFF` ⇒ 读回 **`0x0000_B3FE`** ⇒ ⇒ 两者都是「请求值 ∧ **同一个合法位集合**」⇒ ⇒ 合法位集合 = **`0x0008_B3FE`**。⚠️ **初稿错的根因**：把 `CAUSE_LOAD_PAGE_FAULT` / `CAUSE_STORE_PAGE_FAULT` 当成 **5 / 7**，实为 **13 / 15** ⇒ ⇒ 初稿写的 `0x11A9` **错** ⇒ ⇒ 正确值是 **`0xB109`**（env 实际写的值）⇒ ⇒ 参考侧再把它按合法位集合收敛成 **`0xB108`**（bit 0「指令地址未对齐」被丢）。
- **已改成什么**：§2.2 新增常量 **`MEDELEG_WRITABLE_MASK` = `0x0008_B3FE`**；§1.2 新增 **F35 / F36**；§4.1 / §4.2 里 env 的 `medeleg` 值改成 **`0xB109`**（位 `{0,3,8,12,13,15}`）⇒ ⇒ 并注明**参考侧会把它收敛成 `0xB108`**；§1.3 补第 6 条；验收新增 **A-12**。

### 订正 13 · F37 + §3.3 + §3.4 + §5.2 的 B-1 / B-2 —— 陷阱入口表补「**处理时的特权级**」、使能位方向、向量偏移口径

- **初稿原文**：§3.3 的陷阱入口表**没有「处理时的特权级」这一列**；向量偏移写成 `4 × (cause & 0x3)`（并称「与 `mtvec` 现有实现的口径一致」）；§3.4 把 `mret` / `sret` 的使能位方向写成「`MPIE` ← `MIE`；`MIE` ← 1」与「`SPIE` ← `SIE`；`SIE` ← 1」；§5.2 的 B-1 写「`mret` ⇒ 特权级 M」。
- **依据**：**F37** —— S 模式读 `mstatus` ⇒ **cause 2**（复核 F10）⇒ ⇒ **且**因为该陷阱**未委托**（`medeleg` 的 bit 2 未置）⇒ ⇒ 落 **`mtvec`** 且**在 M 模式处理** ⇒ ⇒ 这正是 **F20** 那个挂死机理的**机制** ⇒ ⇒ 陷阱入口表必须写清**处理时的特权级**（未委托 ⇒ 落 `mtvec` ⇒ 特权级 ← **M**；已委托 ⇒ 落 `stvec` ⇒ 特权级 ← **S**）。⚠️ **向量偏移**：既有 `mtvec` 实现用的是 **`cause & 0x7`**（`crates/kernel/src/m13_iss/csr.rs`）⇒ ⇒ 初稿的 `0x3` 与那句「与既有实现一致」**两个都错**。⚠️ **使能位方向**：返回那侧是「`MIE` ← **旧 `MPIE`**、`MPIE` ← **1**」⇒ ⇒ 与**特权规范一致**、与**本仓既有实现一致** ⇒ ⇒ 初稿是把**陷阱入口那侧**的箭头照抄后改了常量 ⇒ ⇒ 那是**笔误**。⚠️ **B-1 与 B-19 自相矛盾**（一个说「特权级 M」、一个说「来自旧的 `MPP`」）⇒ ⇒ 以 B-19 为准 ⇒ ⇒ B-1 改成「特权级取**旧 `MPP`**」。
- **已改成什么**：§3.3 的表补「**处理时的特权级**」列 + 向量偏移改成 **`cause & 0x7`** + 补「中断 cause **先剥高位**、且 `cause < 64` 才算委托」；§3.4 的 `mret` / `sret` 方向改正并加「**两处方向相反是对的**，不要「统一」」；§5.2 的 **B-1** / **B-2** 同步（**B-14** 补上处理时的特权级）；§1.2 新增 **F37**。

### 订正 14 · F40 + §2.2 + §2.3 + 验收 A-13 / B-22 ——「`mstatus` 的可写掩码 = `0x807E_79AA`」⇒ ⇒ 改成「**可写位** = `0x007E_79AA`」+「`SD` **由 `FS`/`XS` 推导**」⇒ ⇒ **写全 1 之后的读回值仍是 `0x807E_79AA`** ⇒ ⇒ 两者**不矛盾**

- **初稿原文**：「参考侧 `mstatus` 的**真实可写掩码 = `0x807E_79AA`**（写 `0xFFFFFFFF` 后读回：byte3=`0x80`、byte2=`0x7E`、byte1=`0x79`、byte0=`0xAA`）」以及 §2.2 的「`MSTATUS_WARL_MASK` = **`0x807E_79AA`**」⇒ ⇒ 即**把 bit 31 当成了「可直接写的位」**。
- **依据**：**F40** —— 只写 `FS = 3`（`0x6000`、**不碰 bit 31**）⇒ 参考侧读回 **`0x8000_6000`** ⇒ ⇒ `SD` 被**推成 1**；只写 `FS = 1`（`0x2000`）⇒ 读回 **`0x0000_2000`** ⇒ ⇒ `SD` = 0；**直接写 bit 31**（`0x8000_0000`）⇒ 读回 **`0x0000_0000`** ⇒ ⇒ **直接写无效**；先写 `FS = 3` 再写 `FS = 0` ⇒ 读回 **`0x0000_0000`** ⇒ ⇒ `SD` 跟着 `FS` 走；写全 1 ⇒ 读回 **`0x807E_79AA`**（`FS = 3` ⇒ `SD = 1`，复核）⇒ ⇒ ⇒ **`SD` 是推导位** = (`FS` == 3) ∨ (`XS` == 3)，**不可直接写** ⇒ ⇒ ⇒ **可写位掩码是 `0x007E_79AA`**（bit 31 不在里面）。
  ⚠️ **为什么之前测不出来**：前面几轮探针**全是「写全 1」** ⇒ ⇒ 那种情形下 `FS = 3` ⇒ ⇒ `SD` **恒为 1** ⇒ ⇒ 「推导」与「存储」两种读法**给不出不同结果** ⇒ ⇒ 平台按**纯存储**实现也照样过 ⇒ ⇒ **假绿**（**零判别力**）⇒ ⇒ 而黄金语料 `local/smode_csr` 的**第 12 条检查**（把 `FS` 写成 3 之后 S 模式读 `sstatus` 要看到 `SD`）**依赖**这条推导 ⇒ ⇒ 不改就会红 ⇒ ⇒ ⇒ 这也印证了 §1.3 第 7 条那条通用判据：「凡结论来自**写全 1** 再读回，就必须被拆成**可写位**与**读回值**两问」。
- **已改成什么**：§2.2 的 `MSTATUS_WARL_MASK` 改成 **`0x007E_79AA`**（依据 F6 + F40）⇒ ⇒ 并**新增**三个常量 `MSTATUS_FS_MASK`（`0x3 << 13`）、`MSTATUS_XS_MASK`（`0x3 << 15`）、`MSTATUS_SD_BIT`（`1 << 31`）；§2.3 的规则**追加第 6 条**（`SD` 是推导位 ⇒ ⇒ 不在掩码里）⇒ ⇒ 返回值改成「掩码后的新值 + 合法化后的 `MPP` + **推导出来的 `SD`**」⇒ ⇒ 并加一条 ⚠️「凡是写 `mstatus` 的路径都要让 `SD` 跟着被推导，客体的 `sstatus` 写也要走这条」；§1.2 的 **F6** 与 **F34** 改成「**可写位掩码** vs **读回值**」两问（**F34** 那条「`F6` 不需要订正」的判断**只对读回值成立**）；§1.3 补第 7 条；验收新增 **A-13** 与 **B-22**；§9.2 新增变异 **M4**。

### 订正 15 · F8 / F38 / F41 + §2.2 / §2.4 / 裁决 D / 验收 A-6 —— 「`sstatus` 的客体写掩码 = `0x804C_61A2`」⇒ ⇒ 改成「**`0x000C_6122`**」⇒ ⇒ **根因 = 从聚合值的差反推掩码** ⇒ ⇒ **通用判据六**

- **初稿原文**：「`sstatus` 的**客体写**掩码 = **`0x804C_61A2`**（`SIE` \| `SPIE` \| `MPIE` \| `SPP` \| `FS[14:13]` \| `SUM` \| `MXR` \| `SD`）」（F8 / F33，订正 11）以及「客体的写掩码 = `0x804C_61A2` ⇒ ⇒ **含 `MPIE`** ⇒ ⇒ **是 `读视图 + MPIE 一位`**」那个**从差值反推**出来的结论。
- **依据**：**F41** —— **逐位实测**（**先把 `mstatus` 清零** ⇒ ⇒ 再**单独**写 `sstatus` 的**每一位** ⇒ ⇒ 读回 `mstatus` 看那一位落没落）⇒ ⇒ **落**：`SIE`(1) / `SPIE`(5) / `SPP`(8) / `FS0`(13) / `FS1`(14) / `SUM`(18) / `MXR`(19)；**不落**：`MIE`(3) / `UBE`(6) / `MPIE`(7) / `MPP0`(11) / `MPP1`(12) / `XS0`(15) / `XS1`(16) / `MPRV`(17) / `TVM`(20) / `TW`(21) / `TSR`(22) / `SD`(31) ⇒ ⇒ ⇒ **每次观测都没有额外位** ⇒ ⇒ ⇒ **写掩码 = `0x000C_6122`** ⇒ ⇒ ⇒ 与上一版的 `0x804C_61A2` 差 **`0x8040_0080`** = **bit 22（`TSR`）+ bit 7（`MPIE`）**。
  ⚠️ **为什么上一版错了**：那个值是**从两个聚合观测的差反推**出来的 —— 「M 模式写 `sstatus = 0xFFFFFFFF` ⇒ 读回 `mstatus` = `0x800C_61A2`」与「读视图 = `0x800C_6122`」两者**差一位 `MPIE`（bit 7）** ⇒ ⇒ ⇒ **但那多出来的 `MPIE = 1` 不是 `sstatus` 的写带进去的** ⇒ ⇒ 它是**参考侧 env 初始化留下的**（env 在 `csrwi mstatus, 0` 之后又 `csrs mstatus, MSTATUS_MPP` ⇒ ⇒ `MPIE` 早就是 1）⇒ ⇒ ⇒ 而且我**又**把 `MPIE` OR 进去一次 ⇒ ⇒ ⇒ **错了两位** ⇒ ⇒ ⇒ ⚠️ **聚合观测分不清「某一位是可写的」与「某一位本来就在那里」** ⇒ ⇒ ⇒ ⇒ **通用判据六**：**凡是从两个聚合观测的差推出一个掩码，必须用逐位实测复核**。
  ⚠️ **交叉印证**：**写掩码 = 读视图去掉 `UBE`(6) / `XS`(15:16) / 推导位 `SD`(31)** ⇒ ⇒ `0x800D_E162` & `~0x0001_8040` = `0x000C_6122` ⇒ ⇒ 而 `UBE` 与 `XS` 在参考侧**恒为 0**（不可观测）⇒ ⇒ ⇒ **可观测的部分恰好对称** ⇒ ⇒ 这条对称性是 **F38（视图侧）与 F41（写侧）两边独立测出来**的 ⇒ ⇒ ⇒ 互相印证。
- **已改成什么**：§1.2 的 **F8** 改写（删掉「含 `MPIE`」）+ **新增 F41**；**F33** 与 **F38** 补一句「聚合观测推不出写掩码 ⇒ ⇒ 真值必须逐位实测」；§1.3 补第 8 条（即**通用判据六**）；§2.2 的 `SSTATUS_WARL_MASK` 改成 **`0x000C_6122`**（依据 F8 / F41）⇒ ⇒ 并注明「上一版多算了 `TSR` 与 `MPIE` 两位」；§2.4 的写掩码改成新值并补「**写掩码 = 读视图去掉 `UBE` / `XS` / `SD` ⇒ ⇒ 可观测的部分恰好对称**」这条交叉印证；裁决 **D** 改成新值与新字段列表；验收 **A-6** 改成新值并补**负向断言**（`MPIE`(7) 与 `TSR`(22) **不在掩码里**）；§9.2 的 **M1b** 落点里的实测值改成新值。

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

### §9.2 计划中的五条变异

| 编号 | 落点 | 预期红 | 目的 |
| --- | --- | --- | --- |
| **M1** | **内核**的 `mstatus` 写路径：把「`MPP` 保留值 ⇒ **该字段合法化到 0**」改成「**该字段保留旧值**」（**订正 10**：旧口径现在才是变异体） | 内核 + 黄金（**必须含 `local/mstatus_warl`**） | 验 `MPP` 的 WARL 真的被实现 ⇒ ⇒ 黄金侧由 `local/mstatus_warl` 承担 |
| **M1b** | **契约层** `SSTATUS_WARL_MASK`：把实测值 `0x000C_6122` 改回旧的 `0x22`（**订正 11**：即「`SPP` 写不进去」那个**被推翻**的口径） | 契约层 + 内核 + 黄金（**必须含 `local/smode_csr`**） | 验 `sstatus` 的**写掩码**真的被实现 ⇒ ⇒ 黄金侧由 `local/smode_csr` 承担 ⇒ ⇒ **这是契约层那条 lane 自己的变异** |
| **M2** | 内核：S/U 模式读 M 模式 CSR **不判非法指令** | 内核 + 黄金（**必须含 `rv32mi/csr`** ⇒ ⇒ **X-4**） | 验按级可访问性真的被实现 ⇒ ⇒ **顺带验 D-96 真的闭环** |
| **M3** | 内核：`sret` 落 **S 模式**（而不是 U） | 内核 + 黄金（**必须含 `rv32si/scall`**） | 验 `sret` 的落点真的被实现 ⇒ ⇒ **这条的判别力来自 `smode` env 的「严格陷阱向量」**（**订正 2**） |
| **M4** | 内核：`mstatus` 的写路径**不推导 `SD`**（即只做 `& mask` ⇒ ⇒ `SD` 恒为 0） | 内核 + 黄金（**必须含 `local/smode_csr`**） | 验「`SD` 的推导真的被实现」（**订正 14** · F40）⇒ ⇒ 黄金侧由 `local/smode_csr` 的**第 12 条检查**承担 |

⇒ ⇒ **五条红集必须互不相同** ⇒ ⇒ 否则说明有两条走的是同一条路径 ⇒ ⇒ 判别力不成立。

⚠️ **M1b 与 M4 的黄金侧语料相同**（都是 `local/smode_csr`）⇒ ⇒ ⇒ 这两条靠「**红集里有没有契约层**」区分开：**M1b 的红集含契约层**（它落在契约层 ⇒ ⇒ 契约层那条用例**会红**）、**M4 的红集不含契约层**（它落在内核 ⇒ ⇒ 契约层那条用例**不会红**）⇒ ⇒ ⇒ 两者的**内核红集也不同**：M1b 是「`sstatus` 的写掩码被改回 `0x22`」⇒ ⇒ 红的检查是**第 5 条 / 第 11 条**那类；M4 是「`SD` 不再被推导」⇒ ⇒ 红的检查是**第 12 条** ⇒ ⇒ ⇒ **落点不同 ⇒ ⇒ 现象不同 ⇒ ⇒ 红集不同**。

⚠️ **M1 为什么落在内核而不是契约层**（**统筹方裁决**）：契约层的 `mstatus_warl` 已按 **F39** 收敛成 **`mstatus_warl(new: u32) -> u32`** —— 「合法化到 0」的语义**不依赖旧值** ⇒ ⇒ 函数里**拿不到旧值** ⇒ ⇒ 「保留旧值」这个变异体在**契约层**无法**字面**实现（最多只能落成「保留值不被合法化」⇒ ⇒ 观察值是 `MPP = 2` 而不是 `MPP = 3` ⇒ ⇒ 判别力**等价**但**不是同一条语义**）⇒ ⇒ ⇒ **统筹方裁定：M1 落在内核的 `mstatus` 写路径** —— 那里**拿得到旧值**（写之前先读一次 `mstatus`）⇒ ⇒ 变异体就是**字面的「保留旧值」**（把 `MPP` 字段用旧值填回）⇒ ⇒ **不动契约层** ⇒ ⇒ 符合 §9.1 第 5 条「变异期间不改其它被编译的源文件」⇒ ⇒ ⚠️ **代价**：契约层那条用例**不会红** ⇒ ⇒ 所以**另加 M1b** 让**契约层**也有一条自己的变异 ⇒ ⇒ 两条的红集因此**互不相同**（M1b 的红集**含契约层**、M1 的**不含**；两者的黄金侧语料**也不同**）。

⚠️ **M2 与 M3 的红集里各带一条「只有本批才有」的语料** ⇒ ⇒ 这是**判别力清单**的核心：⇒
- **M2 的专属**：`rv32mi/csr`（今天**不**红 ⇒ 因为 U 模式那段根本没跑 ⇒ ⇒ 做完本批它**才**有判别力）；
- **M3 的专属**：`rv32si/scall`（今天**不**存在 ⇒ ⇒ 本批新入库 ⇒ ⇒ 它是 `sret` 落点的**唯一**判据）。
- **M4 的专属**：`local/smode_csr` 的**第 12 条检查**（`SD` 的推导）⇒ ⇒ 这条检查**依赖 F40** ⇒ ⇒ 内核不推导 `SD` 就红 ⇒ ⇒ 它是 M4 的**唯一**黄金侧判据。

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
| 5 | **伪 CSR `CSR_PRIV` 落在 guest 可见的地址空间** | 读回 0 / 写被忽略 ⇒ ⇒ 与「未实现 CSR」**同形** ⇒ ⇒ 不引入新的分歧类别；验收 A-8 / A-10 钉住 |
| 6 | **`sstatus` 的读视图 = `0x800D_E162`**（**含 `SD` / `SUM` / `MXR` / `FS` / `XS`**；F29 / F38）⇒ ⇒ 与规范的「S 级可见字段全集」**不同** | 视图掩码 **钉死**在契约层 ⇒ ⇒ 语料观测不到 ⇒ ⇒ 登记在 `tests/golden/README.md`；**不**登记成台账欠账 |
| 7 | **`mret` 的 `MPP` 只在实现 U 模式时置 U(0)** | 与参考侧同形（参考侧也实现 U）⇒ ⇒ 不是偏差；验收 B-1 / B-19 钉住 |
| 8 | **`sie` / `sip` 的写掩码 = `mideleg`**（F18）⇒ ⇒ 委托位之外的位**写不进去** | 与参考侧同形 ⇒ ⇒ 不是偏差；验收 B-11 钉住 |
| 9 | ⚠️ **与特权规范的两条差异（明示取舍）**：① 参考侧**允许 S 模式写 `sstatus.SPP`**（F8 / F38）⇒ ⇒ 而特权规范里 `SPP` **只在 M 模式可写**；② `SUM` 与 `MXR` **出现在 `sstatus` 的读视图里**（F29）⇒ ⇒ 而特权规范的「S 级可见字段全集」里**没有**这两位 | **本平台对齐参考侧**（黄金对拍的口径就是参考侧）⇒ ⇒ **明示取舍**，登记在 `tests/golden/README.md` 的已知偏差表 ⇒ ⇒ **不**登记成台账欠账；验收 A-6 / A-11 钉住 |
| 10 | **`medeleg` 是 per-bit WARL**：合法位集合 = `0x0008_B3FE`（F35 / F36）⇒ ⇒ env 写的 `0xB109` 在参考侧存成 `0xB108` | 我们的写路径**必须**过 `MEDELEG_WRITABLE_MASK` ⇒ ⇒ 否则与参考侧**逐位不等**；验收 A-12 钉住 |
| 11 | **本平台不实现向量扩展** ⇒ ⇒ `mstatus.XS` **恒为 0** ⇒ ⇒ `SD` 的推导**实际只看 `FS`** ⇒ ⇒ 但口径**仍按规范**算 `XS`（与参考侧同形） | `MSTATUS_XS_MASK` 照样参与推导（`XS` 恒 0 ⇒ ⇒ `XS == 3` 恒不成立）⇒ ⇒ **不因「本平台没有向量」而简化公式** ⇒ ⇒ 与参考侧**同形**；验收 A-13 钉住 |

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
python3 round7.py    # 写侧的按级检查（cause 2）—— F26
python3 round8.py    # 黄金 ISA 串下的 misa —— F27
python3 round9.py    # 委托陷阱后的 sstatus 形状 —— F28（以及当时的 F29，已被订正 11 推翻）
python3 round10.py   # sstatus 的读视图（⚠️ 这次走的是 8 位通道 ⇒ 被订正 11 推翻）/ sie / sip / satp —— F30 / F31
python3 round11.py   # MPP 保留值的真实语义 —— 推翻初稿的「整次丢弃」（⚠️ 这次走的是 8 位通道 ⇒ 结论在订正 10 被再推翻）
python3 round12.py   # 第一次用**不截断**的通道重测：sstatus 的写掩码 / 读视图 / MPP 合法化
python3 round13.py   # 逐位判定：mstatus / sstatus / medeleg 的写掩码与读视图 + MPP 的 WARL —— F34~F39
python3 round14.py   # `mstatus.SD` 是「存储位」还是「由 FS/XS 推导出来的脏位」—— F40
python3 round15.py   # `sstatus` 客体写掩码的**逐位**实测：先清 mstatus 再单独写每一位 —— F41
```

⚠️ **探针不进主代码树**（既有纪律）：探针脚本与它的产物都在**仓外** `/tmp/v038-probe` ⇒ ⇒ 主代码树里**只留**这份规格的结论 ⇒ ⇒ 本轮的复现命令就是 `cd /tmp/v038-probe && python3 round13.py`。

⚠️ **F32 的取证方式**（不是探针，是工具链）：

```bash
printf 'ecall\nebreak\nscall\nsbreak\nsret\nmret\nwfi\n' > /tmp/enc.S
riscv-none-elf-gcc -march=rv32i_zicsr_zifencei -mabi=ilp32 -c /tmp/enc.S -o /tmp/enc.o
riscv-none-elf-objdump -d /tmp/enc.o
# ⇒ `scall` 被汇编成 `ecall`(0x00000073)、`sbreak` 被汇编成 `ebreak`(0x00100073)
```

### §11.2 判据（两侧同程序）

```bash
# ⇒ 期望：spike = PASS   ours = PASS（本批实现前 ours = FAIL 3）
GOLDEN_MANIFEST=... tools/golden-run.sh --keep-going
```

### §11.3 全量门禁

见 §6。

