# STAGE-6 · 黄金对拍的 CSR / 陷阱覆盖 —— v0.21 批冻结规格

> **状态**：冻结（v0.21，2026-10-22）
> **上游依据**：台账 **D-84**（`mip` 无生产者）· **D-85**（黄金语料对 CSR / `mret` / 陷阱投递零覆盖）· **D-90**（`riscv-golden` 的第二套 ELF 符号读取器）
> **本批新增台账**：**D-91**（`misa` 未实现）· **D-92**（未实现 CSR 的写未被忽略）· **D-93**（`mret` 不屏蔽 `mepc[1:0]`）· **D-94**（S 扩展缺失 ⇒ `rv32si` 全部不可行）
> **本批留账**：**D-86**（M-13 取指恒读 4 字节）—— 只订正规格与记录方案，**本批不改实现**（裁决 H）

---

## 0. 本批要解决的问题

v0.19（M-16 中断控制器）与 v0.20（M-11 符号 / 反汇编 / trace 导出）两批落地后，
平台**第一次**有了 CSR 指令族、`mret`、陷阱投递与 `mtvec` 向量。但这两批的
**正确性从未被跨实现验证过**：

- 黄金语料 `tests/golden/corpus.txt` 的 51 例**全是** `rv32ui` / `rv32um` / `rv32uc`
  ⇒ **零条 CSR 指令、零条 `mret`、零条陷阱投递**；
- 台账 D-85 早就写明语料排除 `rv32mi` / `rv32si` 的理由是「需要 trap delivery 和
  `mret`，**那正是 M-16 在第 5 阶段落的**」——v0.19 **就是**那个第 5 阶段，
  **触发条件早已达成**，语料却没扩。

⇒ 本批的目标：**把已落地的能力放进跨实现对拍**，并且**让对拍把平台的语义缺口照出来**
（而不是「补语料」这种只增加数量的活）。

### 0.1 本批的实测基线（**写规格时的证据，不是推演**）

参考侧 = Spike（`--isa=rv32imc_zicclsm_zifencei`），候选侧 = `riscv-golden`。
上游 env（`~/riscv-sim-golden/riscv-tests/env/p/`）原样编译，**上游 `.S` 一行未改**。

**表 A —— 裁决落地前（基线）**：

| # | 用例 | Spike | 我们的 runner | 差异根因（已用探针钉死） |
| --- | --- | --- | --- | --- |
| 1 | `breakpoint` | PASS | **FAIL 2** | 未实现的 trigger CSR（`tdata1` 等）**被我们存进去了**；Spike 是**写忽略、读回 0** ⇒ 该用例的「不支持就跳过」探针不跳 ⇒ **D-92** |
| 2 | `csr` | PASS | PASS | ✅ 已一致 |
| 3 | `illegal` | PASS | **NOVERDICT** | S 模式语义（`csrwi mip, MIP_SSIP` 写被平台规则吞掉 ⇒ 挂死）⇒ 见裁决 K |
| 4 | `instret_overflow` | PASS | PASS | ✅ 已一致 |
| 5~8 | `lh/lw/sh/sw-misaligned` | PASS | PASS | ✅ 已一致（`MisalignedPolicy::Allow` 与 Zicclsm 参考侧同值） |
| 9 | `ma_addr` | PASS | PASS | ✅ 已一致 |
| 10 | `ma_fetch` | PASS | **FAIL 8** | `misa` 未实现（`misa.C` 探测走错分支）⇒ **D-91** |
| 11 | `mcsr` | PASS | **FAIL 2** | **`misa` 未实现** ⇒ `misa[31:30]` 读 0（应报 MXL=1）⇒ **D-91** |
| 12 | `pmpaddr` | PASS | PASS | ✅ 已一致（**当时**我们把写存进去了，恰好与 Spike 同值） |
| 13 | `sbreak` | PASS | PASS | ✅ 已一致（EBREAK → cause 3 + `mepc`） |
| 14 | `scall` | PASS | PASS | ✅ 已一致（ECALL → cause 11） |
| 15 | `shamt` | PASS | PASS | ✅ 已一致（非法指令 → cause 2） |
| 16 | `zicntr` | **FAIL 2** | PASS | **参考侧**不支持（Spike 自己也判不出）⇒ 见裁决 K |

**表 B —— 裁决 D / E 落地后（**实测**，本批的目标状态）**：

| # | 用例 | Spike | 我们的 runner | 变化 |
| --- | --- | --- | --- | --- |
| 1 | `breakpoint` | PASS | **PASS** | ✅ **解锁**（D-92：trigger CSR 写忽略 ⇒ 「不支持就跳过」探针跳过） |
| 2 | `csr` | PASS | PASS | ✅ 不变 |
| 3 | `illegal` | PASS | **NOVERDICT** | ❌ 不变 ⇒ **pending**（裁决 K） |
| 4 | `instret_overflow` | PASS | PASS | ✅ 不变 |
| 5~8 | `lh/lw/sh/sw-misaligned` | PASS | PASS | ✅ 不变 |
| 9 | `ma_addr` | PASS | PASS | ✅ 不变 |
| 10 | `ma_fetch` | PASS | **PASS** | ✅ **解锁**（D-91：`misa` 有 C 位 ⇒ 与 Spike 同分支） |
| 11 | `mcsr` | PASS | **PASS** | ✅ **解锁**（D-91：MXL = 1） |
| 12 | `pmpaddr` | PASS | **FAIL 1** | ❌ **新分歧** ⇒ **pending**（裁决 K：无 PMP ⇒ 写忽略后读回 0，与 Spike 的 PMP 语义**相反** ⇒ **D-94**） |
| 13 | `sbreak` | PASS | PASS | ✅ 不变 |
| 14 | `scall` | PASS | PASS | ✅ 不变 |
| 15 | `shamt` | PASS | PASS | ✅ 不变 |
| 16 | `zicntr` | **FAIL 2** | PASS | ❌ 不变 ⇒ **pending**（参考侧不支持） |

⇒ **13 例两侧一致**（`active`）+ **3 例两侧不一致**（`pending`）⇒ 语料 **51 → 64**。

⚠️ **`pmpaddr` 的新分歧是「修对了才暴露」的**：裁决 D 之前，我们把 `pmpaddr0`
的写**存进去**、读回非 0，**恰好**与 Spike 同值 ⇒ 假一致；裁决 D 之后两边
都「写忽略」，但 **Spike 实现了 PMP（读回非 0）、我们没有（读回 0）** ⇒ 真分歧
暴露出来。⇒ **这就是「让对拍把语义缺口照出来」的实例**。

⚠️ **`ma_fetch` 的覆盖差异（诚实标注 · 2026-10-2x 复核订正）**：两侧判决都是 PASS，
但**走的不是同一条分支**。⚠️ **本节原先的归因是错的**，两路独立检视 + 复核探针
把它钉死了：

| 事实 | 实测值 | 谁测的 |
| --- | --- | --- |
| 参考侧（Spike，默认 `--priv=MSU`）的 `misa` | **0x40141104** = MXL=1 + I + M + C + **S(bit18)** + **U(bit20)** | `csrr misa` 探针 + `--log-commits` 双向确认 |
| 我们的 `misa` | **0x40001104** = MXL=1 + I + M + C | 同上 |
| **差异位** | **恰好 bit18(S) 与 bit20(U)** 两位 | 位运算 |
| IALIGN 字段（bits 29:26） | **两侧都是 0** | 位运算 |

⇒ **原文「Spike 的 `misa` 带 IALIGN=32（bit 29）」是错的**：Spike 的 bit29 也是 0，
两侧 IALIGN 字段**逐位相同** ⇒ 那里**没有**覆盖差异。
⇒ **真正让两侧分叉的是 `misa` 的写语义**：上游 `ma_fetch.S` 用
`csrci misa, 1 << ('c'-'a')` 探测「C 位能不能被关掉」——参考侧的 `misa` 是
**WARL 可写**（实测：`csrci misa, 4` 之后读回 `0x40141100`，C 位真的被清掉），
我们按裁决 E 把 `misa` 做成**完全只读**（写被忽略，读回仍是 `0x40001104`）⇒
**同一个探针在两侧取相反分支**：参考侧继续走后面的断言段，我们提前跳到 `pass`
⇒ **判决一致是真的，覆盖不等价**。⇒ 归入 **D-95**（`misa` 写语义分歧）。

⚠️ **三条「空洞的一致」（判决相同，但两侧**没做同一件事**）**——语料把它们标成
`active` 时必须**带上这个标注**，否则覆盖率会被高估**：

| 用例 | 为什么是「空洞」 | 判别式（怎么证明的） |
| --- | --- | --- |
| `rv32mi/csr` | 用例按 `misa[20]`（**U** 位）分支：参考侧有 U ⇒ **进入 U 模式段**（`csrwi mcounteren` / `sret` 跳用户态 / 用户态读 `sstatus` 必须陷入）；我们没有 U ⇒ **直接跳 `finish` 提前 PASS** ⇒ 那三段特权级路径**一次都没跑** | 插桩上游 `rv64si/csr.S`：参考侧进入 U 模式段，我们不进；陷阱进入次数参考侧 5 次 / 我们 1 次 |
| `rv32mi/ma_fetch` | `misa` 写语义分歧（见上）⇒ 参考侧进 IALIGN 段，我们跳过 | 插桩：参考侧命中 IALIGN 段标记，我们在「清 C 检查点」就跳 `pass` |
| `rv32mi/instret_overflow` | 平台**没有计数器** ⇒ `minstret` 写被忽略、读回恒 0 ⇒ 用例的 `TEST_CASE(2/3/4)`（期望值都是 0）**恒真** ⇒ 它断言的不是「我们的计数器行为与参考侧一致」，而是「我们没有计数器」 | 参考侧 `csrwi minstret, 0` **写生效**（`c2818_minstret 0x00000000`），我们写被忽略 |

⇒ 规格自己的原则是「**空洞的一致比分歧更危险**」（裁决 I）——这三条必须**逐条标注**，
标注写在 `tests/golden/README.md` 的语料说明里（**D-96**）。

**三条探针（两侧各跑一次，退出码即测量值；⚠️ **裁决落地前**的基线）**：

| 探针 | Spike | 我们（**当时**） | 结论 |
| --- | --- | --- | --- |
| `csrr misa; srli 30` | **1** | 0 | ⇒ `misa` 缺失（**D-91**） |
| `csrw tdata1, 0x123; csrr tdata1` | **0**（写忽略） | 0x123（存进去了） | ⇒ 未实现 CSR 的写未忽略（**D-92**） |
| `csrc mstatus, MPP; csrr mstatus` | **0** | 0 | ⇒ ⚠️ **这条推演后来被推翻了**（见下） |

⚠️ **第三条探针的结论是错的**（推翻于 2026-10-2x 复核）：它测的是**陷阱上下文之外**、
且**没有写**的 `MPP`，读到的 0 说明不了任何事——`MPP` 是 **WARL** 字段，在**陷入**
时会被写成 M。⇒ 在**陷阱处理程序里**读 `MPP`：

| 探针 | Spike | 我们（**当时**） | 结论 |
| --- | --- | --- | --- |
| 陷入后在 handler 里 `csrr mstatus` | **0x0018_0080**（`MPP = 3`） | **0x0000_0080**（`MPP = 0`） | ⇒ **真实分歧**：陷入时 `MPP` 没被置 M ⇒ 与 M-13 LLD §5.2.4（"`MPP←M`"）和参考侧都不符 ⇒ **D-97**（✅ v0.21 已闭环） |
| `csrwi mstatus, 0` 之后 `csrr mstatus` | `MPP = 0`（写生效：参考侧支持 U ⇒ 0 是合法 WARL 值） | `MPP = 0` | ⇒ 这一条两侧**确实**一致 |

### 0.2 一个必须先纠正的前提

上游 env 的 `mret` 之前有 `csrwi mstatus, 0` ⇒ 按 RISC-V 规范会把 `MPP` 清成
U-mode ⇒ 看起来 `mret` 会掉出 M 模式。**实测不是**：上游 `RVTEST_RV32M` 的 `init`
宏 = `RVTEST_ENABLE_MACHINE`（`csrs mstatus, MSTATUS_MPP` ⇒ `MPP = 3`），且它在
`csrwi mstatus, 0` **之后**执行 ⇒ `mret` 回到 M 模式。
⚠️ **复核补充**：探针实测 `mret` 返回后**仍能读 `mstatus`**（若已掉出 M 模式，
这条读会陷入）⇒ **两侧 `mret` 后都还在 M 模式** ⇒ 这条前提成立。

⚠️ **但「不实现 `MPP` WARL ⇒ 不构成差异」的结论是错的**（推翻于 2026-10-2x 复核）：
`mstatus` 在本平台是**普通存储**（`csrw mstatus, 0xFFFFFFFF` 读回 `0xFFFFFFFF`，
参考侧读回 `0xAA797E80` ⇒ 掩码掉了保留位）⇒ **确实是分歧**（**D-98**）。
⇒ v0.21 的处置：**只**把**陷入时的 `MPP ← M`** 补上（LLD §5.2.4 的明文要求，
且参考侧也这么做）⇒ **陷入上下文**的 `MPP` 两侧一致了；
**`mstatus` 的整体 WARL 掩码**与 **`mret` 对 `MPP` 的处理**留账（**D-98**）——
参考侧 `mret` 后 `MPP = 0`（它支持 U ⇒ 最小特权级 = U），本平台 `mret` 不动
`MPP`（我们只实现 M ⇒ 最小特权级 = M）⇒ 这是**能力差异**，与 `misa` 的 S/U 位
同类，不是缺陷。

---

## §1. 范围与切分

### 1.1 本批做什么

| 编号 | 内容 | 台账 |
| --- | --- | --- |
| A | `mip` 生产者：M-16 的挂起集 → guest 可读的 `mip` | **D-84** |
| B | 黄金 env 升级（CSR-capable 变体）+ `rv32mi` 语料扩容 + harness 适配 | **D-85** |
| C | `riscv-golden` 的 `find_symbol` 切到 M-11 | **D-90** |
| D | `misa` 实现（MXL + 扩展位） | **D-91**（新登记） |
| E | 未实现 CSR 的**写被忽略、读回 0** | **D-92**（新登记） |
| F | runner 遇陷阱**继续执行**（陷阱是程序的一部分） | 支撑 D-85 |

### 1.2 本批不做什么（**留账**）

| 编号 | 内容 | 台账 | 理由 |
| --- | --- | --- | --- |
| H | M-13 取指宽度（先 2 后 4） | **D-86** | 裁决 H：见 §2 裁决 H |
| G | `mret` 屏蔽 `mepc[1:0]`（IALIGN=32） | **D-93**（新登记） | 只影响 `ma_fetch`（已 pending）；改了会动 `mret` 语义 ⇒ 另批 |
| — | S 扩展（`sret` / `satp` / S 模式 / 软件中断） | **D-94**（新登记） | 范围过大；`rv32si` 全部因此不可行 |
| — | PMP（`pmpcfg0` / `pmpaddr0`） | 记入 D-94 同族 | 范围过大 |
| — | cycle / instret 计数器 | 记入 D-94 同族 | 范围过大 |

---

## §2. 裁决（覆盖 LLD 措辞，须先改 `docs/`）

### 裁决 A · runner 遇陷阱 ⇒ **继续执行**，不是 ERROR

`crates/kernel/src/bin/riscv-golden.rs` 的主循环原本对任何 `StepOutcome::Trapped`
直接 `return Verdict::Error("trap@…")`。

⚠️ **ISS 的 `enter_trap` 已经完成了投递**：`mcause` / `mepc` / `mtval` / `mstatus`
写好，PC 重定向到 `mtvec`（direct）或 `mtvec + 4 × (cause & 7)`（vectored）。
⇒ **陷阱是程序的一部分**，不是平台故障。

**口径**：
- 陷阱 ⇒ **继续跑**，让 guest 的 trap vector 处理它；
- trap vector 自己再陷入 ⇒ 反复投递 ⇒ 转满 `cfg.budget` ⇒ `Inconclusive`
  （NOVERDICT）——**这正是「程序发散」应有的判决**；
- ⚠️ **不得**把陷阱重新变成「静默 PASS」：guest 的 trap vector 会把非预期陷阱
  变成 `TESTNUM |= 1337` ⇒ 判决落在 `tohost` 上 ⇒ **由 guest 自己判 FAIL**。

### 裁决 B · `mip` = M-16 **挂起队列**的投影，**M-17 写侧落盘**

- **来源**：M-16 的 `pending`（挂起集）⇒ **不是** `raised`（断言线集）。
  `mip` 的位义是「中断挂起」；`pending` 天然是**电平**语义（保持到 `clear`）。
- **落盘**：**M-17 写侧投影**（`IssExecutor::step` 内），两个刷新点：
  ① 量子边界 `poll()` **之后**；② `irq_clear` 副作用（`clear_lines_by_mask`）**之后**。
- **时机**：量子边界（1 µs = **1000 条指令**）⇒ guest 读 `mip` 得到的是
  **上一个量子边界的挂起快照** ⇒ **必须写进规格与台账**（不假装是精确值）。
- **只在值变化时写** ⇒ 没有中断的 guest 的 CSR 文件**不会**多出 `(0x344, 0)` 条目
  ⇒ 保住 `SimState::is_empty()` 的语义。

⚠️ **偏离台账 D-84 的字面措辞**（原文：「要动 M-13 的 CSR **读**路径」）。
**理由**（必须记在台账里）：
1. `csr_read` 是 `&SimState` 上的**纯函数**，M-13 对 M-16 **零依赖** ⇒ 读侧投影要么给
   `SimState` 加字段（= CSR 的**第二落点**，违反 STAGE-4 裁决 A），要么给 `csr_read`
   加参数（≈10 个调用点）⇒ 侵入面远大于写侧。
2. `irq_clear` 的平台副作用本来就落在 **M-17** ⇒ **同型落点就是 M-17**。
3. ⚠️ **硬理由**：平台的 `mip` 写**不能**经过 M-13 的「刚写了哪个 CSR」回报
   （STAGE-4 §646）——否则 M-17 会把「写了 `mip`」当成「写了 `irq_clear`」
   ⇒ **清错线**。写侧投影天然规避。

### 裁决 C · `mie` 与 `mip` 共用**同一张位号表**

RISC-V 里 `mie` 与 `mip` 对**同一中断源用同一 bit 号**（MEIE/MEIP=11、MTIE/MTIP=7）。
本平台的线号 → 位号映射**已经存在**（`mie_bit_for_line`）⇒ **映射表唯一**。

⚠️ 但函数名里的 `mie` 会让它同时服务两个寄存器 ⇒ **一码多义**（本仓一贯反对）。
⇒ **裁决**：契约层把 `mie_bit_for_line` 改名 **`std_irq_bit_for_line`**（语义：
「线号 → RISC-V 标准中断位」），**保留 `mie_bit_for_line` 作别名**（不破坏外部调用）；
新增 `pending_lines_to_mip_mask(lines: &[u16]) -> u32`（挂在**同一张表**上）。

⚠️ **本平台的线语义**（契约层唯一真相，**不是**「线 1 = MEIP」）：

| 线号 | 来源 | 标准位 |
| --- | --- | --- |
| 1 | UART 收就绪（外部） | **11（MEIP）** |
| 2 | UART 发空（外部） | **11（MEIP）**（与线 1 **共享**） |
| 3 | 定时器比较 | **7（MTIP）** |

⇒ 线 1 与线 2 **折叠进同一位** ⇒ guest 读 `mip` **分辨不出**是哪条 UART 线挂起；
线号只在交付时经 `mcause = IRQ_BASE | line` 给出。**必须有用例钉住这个共享位。**

### 裁决 D · 「**已实现 CSR 表**」——未实现的 CSR：**写被忽略、读回 0**

⚠️ **实测证据**（探针 2）：Spike 对未实现 CSR 的写**被忽略**（`tdata1` 写 0x123
读回 0）；我们**存进去了**（读回 0x123）⇒ 语义不一致 ⇒ **D-92**。

**口径**（契约层 `csr.rs` 新增一张表）：

| 规则 | 内容 |
| --- | --- |
| 读 | CSR **未实现** ⇒ 返回 **0**，**不陷阱**（STAGE-4 §2.2 的既有裁决，**不变**） |
| 写 | CSR **未实现** ⇒ **写被忽略**（不进 CSR 文件）⇒ 之后读回 **0** |
| 写 | CSR **已实现且只读**（`misa` / `mhartid` / `cycle` / `instret` …）⇒ 写被忽略 |
| 写 | CSR **已实现且可写** ⇒ 写入 CSR 文件 |
| 写 | `mip` ⇒ **guest 写被忽略**（平台规则，既有）；**平台自己**写（裁决 B）⇒ 生效 |

**「已实现」清单**（契约层唯一真相）：
`mstatus`(0x300) · `misa`(0x301) · `medeleg`? **否** · `mideleg`? **否** ·
`mie`(0x304) · `mtvec`(0x305) · `mscratch`(0x340) · `mepc`(0x341) ·
`mcause`(0x342) · `mtval`(0x343) · `mip`(0x344，平台写) ·
`irq_clear`(0x7C0，平台自定义)。

⚠️ **`mscratch` 必须在内**：上游 `rv64si/csr.S` 测它的 12 条 RMW 语义 ⇒
不实现 ⇒ 写被忽略 ⇒ 那条用例的语义就错了。

⚠️ **`medeleg` / `mideleg` 不在内** ⇒ 写被忽略、读回 0。
⚠️ **原文写「与 Spike 一致（Spike 同样不实现 delegation ⇒ 写忽略）」是错的**，
实测（复核于 2026-10-2x）：参考侧跑的是**默认 `--priv=MSU`** ⇒ 它**实现了**
delegation ⇒ `csrw medeleg, 0x123; csrr medeleg` 在参考侧读回 **0x22**
（写被按掩码裁剪后存下），我们读回 **0** ⇒ **两侧不等** ⇒ 归入 **D-94**。
⚠️ 顺带说明：本裁决里「**未实现 ⇒ 写忽略、读回 0**」这条**平台规则**本身也
**不等于参考侧**——参考侧对**根本不存在**的 CSR 会陷入非法指令（`csrr cycle`、
`csrw mhartid`），我们一律读回 0 不陷入 ⇒ 归入 **D-92 / D-94**。

### 裁决 E · `misa` 报 **MXL=1** + 已实现扩展位（**只读**）

`misa`(0x301) 的架构定义：bit 31:30 = MXL（`01` = RV32），bit 0..31 = 扩展字母位。

**本批实现**：`MXL = 1`（RV32）+ **如实**填已实现扩展（`I` / `M` / `C`）
⇒ **不**填 `A`（原子未实现）、**不**填 `F`/`D`（浮点未实现）、
**不**填 `S`/`U`（特权级未实现）。
⇒ `misa` **只读**（写被忽略，裁决 D）。

⚠️ **诚实标注（复核订正于 2026-10-2x）**：参考侧的 `misa`（默认 `--priv=MSU`）=
**0x40141104** = MXL=1 + I + M + C + **S** + **U**；我们 = **0x40001104** =
MXL=1 + I + M + C。⇒ **两侧不逐位相同**，**差异恰好是 bit18(S) 与 bit20(U)**
——因为参考侧实现了 S/U 特权级而我们没有 ⇒ 如实不填是**诚实**的。
⇒ 上游 `mcsr.S` 恰好只查 MXL（`srl 30`）⇒ 兼容；
⚠️ **但 `rv64si/csr.S` 会查 `misa` 的 U 位（bit20）** ⇒ 参考侧**有** U ⇒
**不跳过** U 模式段（`csrwi mcounteren` / `sret` 跳用户态 / 用户态读 `sstatus`
必须陷入）⇒ 我们（无 U）**直接跳 `finish` 提前 PASS** ⇒ **两侧走的是不同分支**
⇒ **判决一致是真的，覆盖不等价**。
⇒ ⚠️ 原文「与 Spike（`misa` 无 F/U 位）**同分支** ⇒ 一致 ✅」是**错的**：参考侧
**有 U 位**（F 位倒确实两侧都没有 ⇒ F 段两边都跳过，那半句成立）。
⇒ 该用例必须**标注**为「空洞的一致」（**D-96**），不得当作特权级覆盖的证据。
⚠️ 另有一处语义分歧：`misa` 的**写**语义——参考侧 WARL 可写（`csrci misa, 4`
真的清掉 C 位），我们完全只读 ⇒ **D-95**。

### 裁决 F · 黄金 env **另起一个 CSR 变体**，**不动**现有 51 例的 env

现有 `tests/golden/env/riscv_test.h` 是**零 CSR** 的替代实现（176 行）。本批
**不**改它（51 例的判决**零影响**）⇒ 新增 `tests/golden/env/csr/` 目录：

```
tests/golden/env/
├── riscv_test.h        # 现有：零 CSR 变体（51 例，**不动**）
├── link.ld             # 现有（**不动**）
└── csr/                # 新增：CSR-capable 变体
    ├── riscv_test.h    # 陷阱向量 + mtvec 编程 + mret 收尾 + ECALL 判决
    └── encoding.h      # 上游测试体与 test_macros.h 依赖的常量
```

⇒ **harness 的 include 路径**：CSR 变体的 `-I` 必须**排在** `-I$ENV_DIR` **之前**
⇒ `riscv_test.h` 解析到变体，而 `encoding.h` 由变体目录提供
（上游 env 用 `#include "../encoding.h"` ⇒ 相对 `env/csr/` ⇒ `env/encoding.h`
⇒ ⚠️ **本批的 `encoding.h` 放 `tests/golden/env/encoding.h`（`csr/` 的上一级）**，
与上游的目录关系一致 ⇒ 变体里写 `#include "../encoding.h"`）。

⚠️ **`link.ld` 不用改**：上游 `env/p/link.ld` 与本项目的 `tests/golden/env/link.ld`
**逐行等价**（同布局、同 `.tohost` 对齐）⇒ 两边链接结果一致。

### 裁决 G · 语料清单**增一列「env 变体」**

格式：`<suite> <test> <march> <status> [env] [reason]`

- 第 5 列 = env 变体（`csr`）；**缺省** = 现有零 CSR 变体；
- 第 6 列起 = `pending` 的理由 + 台账号（**`pending` 必须带**）。

⚠️ **既有 51 行不动**（第 5/6 列为空）⇒ 解析兼容。

### 裁决 H · D-86（M-13 取指恒读 4 字节）**本批不改**，只订正规格 + 记录方案

**现状**：M-13 取指恒 `read_fetch(pc, BusWidth::Word)`（`m13_iss/mod.rs`），
而 M-11 的反汇编取指是「**先 2 后 4**」⇒ 映射区**末端**只有 2 字节的压缩指令时，
**反汇编能解出来、访客取指却失败**（`E_BUS-01`）⇒ **调试器显示的指令访客执行不了**。

**本批不改的理由**：
1. LLD §3.5 的「取指恒 4 字节」是**有理由的设计决策**（**1 次总线事务**，不翻倍
   32 位指令的事务数、不污染总线统计）⇒ 推翻它要重新权衡；
2. 替代方案「4 字节失败后回退 2 字节」需要 **M-14 的跨界错误可区分** +
   **失败访问的 hook 语义**（探针 / 观察点会看到那次失败的访问）⇒ 影响面超出本批；
3. ⚠️ **改取指宽度会让本批的黄金对拍测量失效**（总线事务数变了）⇒ 本批的基线必须
   建立在**当前**取指语义上。

⇒ **处置**：本批**登记为留账**，把「可行方案 + 各自代价」写进台账
⇒ 留给 **D-30**（M-12 独立模块级 review，含 `run_until` 无界自旋守卫）那一轮
一并处理。⇒ **本批的 AGENTS.md / REVIEWS.md 必须写明「D-86 未闭环」**。

### 裁决 I · `rv32mi` 语料的 `active` / `pending` 划分

**判据**（不是「看起来能不能跑」，而是**两侧实测**）：

| 状态 | 用例 | 依据 |
| --- | --- | --- |
| **active** | `breakpoint` `csr` `instret_overflow` `lh-misaligned` `lw-misaligned` `sh-misaligned` `sw-misaligned` `ma_addr` `ma_fetch` `mcsr` `sbreak` `scall` `shamt` | **13 例**（表 B 实测：裁决 D/E 落地后两侧一致） |
| **pending** | `illegal` | 需 S 模式语义（`csrwi mip, MIP_SSIP` 写被忽略 ⇒ 挂死）⇒ **D-94**。实测：Spike `PASS` / 我们 `NOVERDICT` |
| **pending** | `pmpaddr` | 平台**无 PMP** ⇒ `pmpaddr0` 写忽略后读回 0，与 Spike 的 PMP 语义**相反** ⇒ **D-94**。实测：Spike `PASS` / 我们 `FAIL 1` |
| **pending** | `zicntr` | **参考侧**（Spike）自己也判不出（`FAIL 2`，因为该 ISA 配置下 Spike 没有 Zicntr、读 `cycle` 会陷入）⇒ 平台无责。实测：Spike `FAIL 2` / 我们 `PASS` |

⚠️ **`rv32si` 6 例全部 `pending`**（S 扩展未实现 ⇒ **D-94**）。实测：
`csr` → `FAIL 22`、`sbreak` / `scall` → `FAIL 669`、`wfi` → `FAIL 668`、
`dirty` **连汇编都过不了**（缺 `SATP_MODE` / `SATP_MODE_SV32`）、
`ma_fetch` 两侧**都 PASS**（但是**空洞的一致**——两侧都没走到它要测的 S 模式路径
⇒ **不能**提升为 `active`）。

⚠️ **pending 行为什么不进 `corpus.txt`**：harness 的 `pending` 规则**只**容忍
「候选侧跑不了」（`ERROR` / `NOVERDICT`）。上表里 `pmpaddr`（我们 `FAIL 1`）与
`zicntr`（我们 `PASS`）**跑得了、只是分歧** ⇒ 把它们列进去就得把规则放宽成
「任何分歧都算预期缺口」——那正是这条规则当初要堵的洞。
⇒ **改为**：pending 清单 + 逐例实测分歧写进 `tests/golden/README.md`，台账 **D-94** 跟踪。
（`rv32si/ma_fetch` 那一行尤其说明了为什么：**空洞的一致**比分歧更危险。）

⚠️ **本批结束后语料从 51 例扩到 64 例**（51 + 13）⇒ 门禁数字**全部重算**。

---

## §3. 契约层新增形状（**lane 照此实现，不得自行发明**）

### 3.1 `crates/contract/src/csr.rs`

```rust
/// 线号 → RISC-V 标准中断位（`mie` 与 `mip` **共用同一张表**）。
pub fn std_irq_bit_for_line(line: u16) -> Option<u32>;

/// 旧名保留为别名（不破坏外部调用点）。
pub fn mie_bit_for_line(line: u16) -> Option<u32>;   // = std_irq_bit_for_line

/// 挂起线集合 → `mip` 位掩码（挂在同一张表上）。
pub fn pending_lines_to_mip_mask(lines: &[u16]) -> u32;

/// 该 CSR 是否**已实现**（决定「写被忽略」还是「写入文件」）。
pub fn csr_is_implemented(csr: u16) -> bool;

/// 该 CSR 是否**只读**（`misa` 等；与地址规则 `>= 0x800` 并集）。
pub fn csr_is_read_only(csr: u16) -> bool;            // = is_csr_read_only || misa
```

⚠️ **`mip` 的写规则**（裁决 D）：**guest 写被忽略**（既有平台规则），
**平台写**由 M-17 直接调 `csr_write`（不经 M-13 的执行臂）⇒ 不冲突。

### 3.2 M-13 侧（`crates/kernel/src/m13_iss/`）

- `csr_write` 的**调用点**（执行臂）加一条守卫：**未实现 ⇒ 忽略写**（裁决 D）。
  ⚠️ **平台自己的写**（`mip`）必须走**另一条入口**（`csr_write` 本身无守卫）⇒
  **平台的 `mip` 写不会被忽略**。
- `misa` 的**取值方式**：复位时把 `misa` 放进 CSR 文件（`MXL=1` + `I`/`M`/`C`），
  或在 `csr_read` 里特判。⚠️ **两处只能选一处** ⇒ 裁决（**§3.4 的最终决定**）：
  **在 `csr_read` 里特判**（**不**预置进 CSR 文件）—— 预置会让复位后
  `SimState::csrs` **非空** ⇒ 破坏 `EngineSnapshotDto::is_empty()` 的
  「空快照」判定 ⇒ 那个判定是**公开契约 API**（`contract/src/snapshot.rs`），
  语义变化必须显式记录，不能顺手改掉。
  ⇒ ⚠️ **本节原先写的是「裁决：预置进 CSR 文件」，那是过期文字**（与 §3.4 矛盾），
  **实现跟的是 §3.4**（`m13_iss/csr.rs` 的 `csr_read` 特判 `misa`）⇒ 已订正。

### 3.3 M-16 / M-17 侧（D-84）

- M-16 的 `InterruptPort` trait 新增 **`fn pending_lines(&self) -> Vec<u16>`**
  （返回**挂起线**的编号集合）。
  ⇒ **三个实现都要改**：`InterruptController` / `SharedInterruptPort` / `NullInterruptPort`
  ⇒ 漏一个就**编译不过**（这是好事）。
- **位掩码的换算留在契约层**：`pending_lines_to_mip_mask(&lines)`（契约层
  `std_irq_bit_for_line` 那张表）⇒ **M-16 不知道位号**，M-17 才知道 `mip` 的
  掩码 ⇒ 「线号空间 ↔ `mie`/`mip` 位号空间」仍然只有**一个**换算点。
- ⚠️ **本节原先规定的是 `fn mip_bits(&self) -> u32`**（M-16 自己算掩码）。
  实现**改成了 `pending_lines()`** ⇒ 理由：让**换算表**留在契约层（只有契约层
  知道 `std_irq_bit_for_line`），M-16 只报「哪些线挂起了」这个**它真正知道**的
  事实 ⇒ 少一个知道位号的落点。**偏离已登记，规格已订正**。
- M-17 的 `IssExecutor::step`：**两处**刷新（裁决 B）。

### 3.4 ⚠️ 三个必须回答的副作用问题

| 问题 | 口径 |
| --- | --- |
| `misa` 预置 ⇒ `SimState::csrs` 非空 ⇒ `is_empty()` 变 false？ | ⚠️ **必须评估并写进台账**。`is_empty()` 是**公开契约 API**（`contract/src/snapshot.rs`）⇒ 语义变化要**显式记录**。⚠️ 备选：`misa` 不预置，改在 `csr_read` 里特判 ⇒ **`csr_read` 多一个特判**（但「没有 ⇒ 返回 0」的语义**不变**）⇒ **本批选这条路**（改动面小、不动 `is_empty` 语义）⇒ 裁决 D 的预置方案**作废**。 |
| `mip` 写进 CSR 文件 ⇒ 会话快照的**两个落点**（段 1 的 `mip` vs 段 5 的 pending） | 段 5 权威；段 1 的 `mip` 是**派生缓存** ⇒ 还原后**下一个量子边界**就重新投影（还原把 `last_poll_ps` 归零 ⇒ 虚拟时间 ≥ 1 µs 时即**第一条指令**，否则等到跨过量子边界）⇒ 窗口 ≤ 1 个量子，与**运行期**的陈旧窗口**同一个口径** ⇒ **写进台账**。⚠️ **不新增**「`mip` 必须等于段 5」的会话校验（量子内瞬态会误伤）。 |
| 前端会不会显示 `mip`？ | **不会**（`ReadRegisters` 只读 GPR；前端零 CSR 引用）⇒ **无 UI 影响**。 |

---

## §4. 黄金参照与验收判据

### 4.1 黄金参照

- **参考侧**：Spike（`--isa=rv32imc_zicclsm_zifencei`），**与现有 51 例同配置**；
- **候选侧**：`riscv-golden`；
- **上游源码**：`~/riscv-sim-golden/riscv-tests/isa/rv32mi/*.S`（**一行不改**）；
- **env**：本批的 CSR 变体（`tests/golden/env/csr/`）。

### 4.2 验收清单（**逐条可测**）

| # | 验收项 | 判据 |
| --- | --- | --- |
| 1 | `mip` 生产者 | guest 读 `mip` 能看到**挂起线**（位 11 = 线 1∨2、位 7 = 线 3）⇒ 端到端用例 |
| 2 | `mip` 与 `mie` 位号一致 | 同一线的 `mip` 位号 == `mie` 位号（位 11 / 位 7） |
| 3 | 线 1/2 共享位 | 线 1 与线 2 **折叠进同一位**（guest 分辨不出）⇒ 用例钉住 |
| 4 | `mip` 只在变化时写 | 无中断的 guest 的 CSR 文件**不含** `(0x344, …)` 条目 |
| 5 | guest 写 `mip` 无效 | guest 写 `mip` ⇒ 平台值**不变** |
| 6 | `misa` 报 MXL | `misa[31:30] == 1`（RV32） |
| 7 | `misa` 只读 | guest 写 `misa` ⇒ 读回**不变** |
| 8 | 未实现 CSR 写忽略 | 写 `tdata1` ⇒ 读回 **0**（与 Spike 同） |
| 9 | 未实现 CSR 读不陷阱 | 读 `mip`/`mhartid` 等 ⇒ 返回 **0**，**不陷阱** |
| 10 | `mscratch` 可用 | 12 条 RMW 语义正确（`csrrs`/`csrrc` 在 `rs1==x0` 时不写） |
| 11 | 陷阱是程序的一部分 | runner 遇陷阱**继续** ⇒ guest 的 trap vector 能跑 |
| 12 | 发散 ⇒ NOVERDICT | trap vector 自己再陷入 ⇒ 预算耗尽 ⇒ `Inconclusive`（**不是** ERROR、**不是** PASS） |
| 13 | 语料扩容 | `corpus.txt` 含 **13 条 `rv32mi` active**（`env` 列 = `csr`）⇒ 全表 **64 行**（51 旧 + 13 新）。⚠️ **本批不把 pending 行放进 `corpus.txt`**（原措辞「+ 4 条 pending」与 #14 的 64/64 自相矛盾，且实测三例的 pending 行**不满足** harness 的严格 pending 规则，见裁决 I）⇒ pending 清单与**逐例实测分歧**写进 `tests/golden/README.md`（§ "Cases that are *not* in the corpus"），由台账 **D-94** 跟踪 |
| 14 | 语料全绿 | `tools/golden-run.sh --keep-going` ⇒ **64/64**（51 旧 + 13 新）判决逐例一致 |
| 15 | 金丝雀仍有效 | `--self-check` 三个对照两侧一致（**语料扩容不得让金丝雀变弱**） |
| 16 | 旧语料零影响 | 51 条旧用例的判决**逐例不变** |
| 17 | `riscv-golden` 用 M-11 查符号 | `find_symbol` **删掉** ⇒ 符号来自 M-11 ⇒ `tohost` 仍能找到 ⇒ 判决不变 |
| 18 | D-86 留账 | 台账写明「本批未闭环」+ 可行方案 + 代价 |
| 19 | 新台账 | **D-91 / D-92 / D-93 / D-94** 登记，处置写明 |
| 20 | 数字全重算 | AGENTS.md / REVIEWS.md / PROGRESS.md 的行数、用例数、语料数、门禁数字**从当前树重算** |

---

## §5. 分 lane 与文件所有权（并行写，串行验）

| Lane | 内容 | **独占文件** |
| --- | --- | --- |
| **0**（串行，地基） | 契约层 `csr.rs` + M-13 的 CSR 规则（裁决 D/E）+ runner 陷阱策略（裁决 A）+ D-90 + 本规格 | `crates/contract/src/csr.rs` · `crates/contract/src/lib.rs` · `crates/kernel/src/m13_iss/csr.rs` · `crates/kernel/src/m13_iss/executor.rs` · `crates/kernel/src/m13_iss/tests_csr.rs` · `crates/kernel/src/bin/riscv-golden.rs` · `docs/detailed-design/STAGE-6_*.md` |
| **A** | D-84：`mip` 生产者 | `crates/kernel/src/m16_interrupt/mod.rs` · `crates/kernel/src/m16_interrupt/tests.rs` · `crates/kernel/src/m17_backend/mod.rs` · `crates/kernel/tests/intc_end_to_end.rs` |
| **B** | D-85：CSR env + 语料 + harness | `tests/golden/env/csr/*` · `tests/golden/env/encoding.h` · `tests/golden/corpus.txt` · `tools/golden-run.sh` · `tests/golden/README.md` |

⚠️ **Lane 0 必须先落**（契约层与 M-13 的 CSR 规则是 Lane A/B 的地基）。
⚠️ **Lane A 与 Lane B 的文件零交集** ⇒ 可并行。
⚠️ **每 lane 独立 `CARGO_TARGET_DIR`**（共享 target 目录会互相替换产物 ⇒ 假象）。

---

## §6. 与台账的关系

| 台账 | 处置 |
| --- | --- |
| **D-84** | ✅ 本批闭环（裁决 B）⇒ **并记录「读侧 → 写侧」的偏离与理由** |
| **D-85** | ✅ 本批闭环（裁决 F/G/I） |
| **D-90** | ✅ 本批闭环 |
| **D-86** | ⏳ **留账**（裁决 H）⇒ 记录可行方案与代价 |
| **D-91** | 🆕 **新登记**（`misa` 未实现）⇒ 本批闭环（裁决 E） |
| **D-92** | 🆕 **新登记**（未实现 CSR 的写未忽略）⇒ 本批闭环（裁决 D） |
| **D-93** | 🆕 **新登记**（`mret` 不屏蔽 `mepc[1:0]`）⇒ **留账** |
| **D-94** | 🆕 **新登记**（S 扩展 / PMP / 计数器缺失）⇒ **留账**（`rv32si` 全部 pending 的根因） |

---

## §7. 冻结边界

1. **契约层升版？** ❌ **不升** —— 本批**不新增**任何契约命令 / 事件 / DTO
   （`csr.rs` 的新增全是 `pub fn`，不动线格式）⇒ `schema_version` 保持 **3**。
2. **前端零改动** —— 本批不碰 `front/`（裁决：前端不显示 CSR ⇒ 无 UI 影响）。
3. **不改现有 51 例的 env** ⇒ 旧语料的判决**逐例不变**（验收 #16）。
4. **不改上游 `.S`** —— 上游源码**只读**；本批的改动全在**本项目**的 env / 语料 /
   契约 / 内核里。
5. **不动 M-13 的取指宽度**（裁决 H）⇒ 黄金对拍的基线保持在当前取指语义上。
6. **不实现 S 扩展 / PMP / 计数器**（D-94）。

---

## §8. 收口判据

- `cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --no-fail-fast --lib --tests`（**数字从当前树重算**）
- `tools/check-front.sh`（**数字从当前树重算**）
- `tools/golden-run.sh --self-check` ⇒ 三个对照两侧一致
- `tools/golden-run.sh --keep-going` ⇒ **64/64** 判决逐例一致
- `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
- `node tools/browser-verify.mjs`（真 Chrome）
- **反向变异自检**（本批的判别式至少覆盖：裁决 A / B / D / E / F / G / I）
- **两路独立检视**（实现正确性 / 判别力与门禁）
- 数字口径**从当前树重算** ⇒ AGENTS.md / REVIEWS.md / PROGRESS.md 同步
