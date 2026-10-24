# STAGE-21 · v0.37 · PMP 批 —— 物理内存保护 / 条目优先级 / MPRV 口径

> **本规格是冻结件。** 改它必须走「统筹方独占修改 + 合回 main」的路，
> 并同步改 `tools/stage21_check.py`（七项自检）⇒ ⇒ 规格与自检脚本**一起**变。
>
> **契约层新增面 = 零**（`SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、
> `PLATFORM_SCHEMA_VERSION` 仍 **1**）⇒ ⇒ **零**新 DTO、**零**新命令、**零**新事件、
> **零**新场景码 ⇒ ⇒ 前端镜像**不用改版本号** ⇒ ⇒ 不制造 D-143 的存量问题。

## §0 本批的定位：D-94 的**第 ② 件事**

台账 `docs/reviews/REVIEWS.md` 的 D-94 写的是「RV32IMC 缺三样特权能力」，
第 19 批（v0.36）做掉了第 ① 件事（Zicntr 计数器），本批做第 ② 件事：**PMP**。

### §0.1 本批**做**什么（**五件**）

1. **PMP 寄存器组**：`pmpcfg0..3` + `pmpaddr0..15`（**20 个 CSR**，共 16 个条目）。
2. **地址匹配**：NAPOT / NA4 / TOR 三种模式（OFF = 不启用），**粒度 4 字节**。
3. **权限判定**：R / W / X 三位 + 优先级（**最低编号的命中条目**说了算）。
4. **特权口径**：M 模式旁路（`MPRV=0`）与 `MPRV=1` 的受检口径；`L` 位把权限
   **在 M 模式也强制执行**；取指同样受 `X` 约束。
5. **拒绝 ⇒ 架构陷阱**：取指 = `mcause 1`、读 = `mcause 5`、写 = `mcause 7`，
   `mtval` = 出错地址。

### §0.2 本批**明确不做**的事（**五件**）

1. **不做 S/U 模式**（D-94 的第 ③ 件事）⇒ ⇒ 本批的 PMP 只能在 **M 模式 + `MPRV=1`**
   下被观测到 ⇒ ⇒ 判据必须显式开 `MPRV`。
2. **不做页表 / `satp`** ⇒ ⇒ PMP 判的是**物理地址**。
3. **不做 64 个条目**：参考侧（Spike RV32）只实现 **16** 条 ⇒ ⇒ 本批对齐 **16** 条
   ⇒ ⇒ `pmpcfg4..15` 与 `pmpaddr16..63` **不实现**（读回 0、写被忽略）。
4. **不改 `tests/golden/env/`**：那份 env 是上游 riscv-tests 的**逐字镜像**
   （`tools/golden-run.sh` 的 `check_env_constants` 会逐字比对）⇒ ⇒ env 里那句
   「本平台无 PMP ⇒ 写被忽略 ⇒ 全放行」的注释**必须留着**（它会变成过期文字）
   ⇒ ⇒ 偏差登记在 `tests/golden/README.md`（裁决 M）。
5. **不做 PMA / 设备属性**：MMIO 的可读可写性仍是平台的事，不归 PMP。

### §0.3 为什么**必须**做真实现（不是「一排不起作用的寄存器」）

勘察批（`docs/reviews/V035_D94_Survey.md`）已经点破：**光放宽 CSR 规则就能让
`rv32mi/pmpaddr` 过 ⇒ ⇒ 那不是真修法**。本批的判据（§4.2）要求**参考侧与
本平台跑出同一判决**，而该判决**必须依赖 PMP 的真实行为** ⇒ ⇒ 寄存器组、匹配、
优先级、陷阱四件事**一件都不能少**。

## §1 参考侧语义：**实测**（不是推断）

### §1.1 探针方法论（**前两轮翻车的教训** ⇒ 这里是方法，不是结论**）

1. **每个观测一个独立 ELF** ⇒ ⇒ 观测值不进「打包过的退出码」。
2. **退出码就是观测值**：参考侧的 PASS 路径**静默退出码 0**；FAIL 路径退出码 =
   `TESTNUM` ⇒ ⇒ 布尔观测直接读退出码；数值观测（读回值）用 `TESTNUM` 承载。
3. **不猜「被拒」的原因**：「没匹配」与「匹配了但没权限」行为**完全相同**
   ⇒ ⇒ 每次只让**一个变量**动，其余**固定且已证明**。
4. **先证明「匹配」再测「权限」**：用一条**已证明匹配**的编码做基准。
5. ⚠️ **一个能同时解释「匹配」与「权限」的构造是不合格的** ⇒ ⇒ 「变量没分开」时
   结论一律作废（§8 记了两次翻车）。

### §1.2 十九条**实测**事实

| 编号 | 事实 | 探针 |
| --- | --- | --- |
| F1 | `pmpcfg` 字节布局：**`L` = bit 7、`A` = bit 4:3、`X` = bit 2、`W` = bit 1、`R` = bit 0**；`A`：OFF = `0b00`、TOR = `0b01`、NA4 = `0b10`、NAPOT = `0b11` | `p11` 四变体 + 规范 §3.7.6 |
| F2 | RV32 的 `pmpcfg{i}` 装 **4 个 8 位条目**（字节 0..3 = 条目 4i..4i+3）⇒ ⇒ **条目 1 在 `pmpcfg0` 的字节 1** | §8 订正 1 |
| F3 | `pmpaddr` 存的是**物理地址右移 2 位**（地址 bit 33:2）⇒ ⇒ 粒度 4 字节 | `matrix3` A 组全对、B 组全拒 |
| F4 | NAPOT：令 `n` = `pmpaddr` 末尾连续 1 的个数 ⇒ 区间 = `[ (pmpaddr & ~((1<<n)-1)) << 2 , +2^(n+3) )` 字节 | `matrix3` / `matrix9` / `matrix11`（4 KiB、256 KiB、1 MiB 三点） |
| F5 | NA4：区间 = `[ pmpaddr<<2 , pmpaddr<<2 + 3 ]`（4 字节） | `matrix4` N4 组 |
| F6 | TOR：区间 = `[ pmpaddr[i-1]<<2 , pmpaddr[i]<<2 )`（**半开**） | `matrix3` T3 组 |
| F7 | **优先级：最低编号的「命中」条目决定成败**（不是「任一放行即可」，也不是「首个非 OFF 条目」） | `matrix16` 五行 |
| F8 | **无条目命中 ⇒ 拒**（只要存在 ≥1 个 `A≠OFF` 的条目） | `NOMATCH-out` |
| F9 | **`R`=0 且 `W`=1 是保留组合 ⇒ 写被拒**（`0x1a`/`0x1e` 写均被拒；`0x1b`（`R`∧`W`）放行） | `matrix2` W2 组 |
| F10 | M 模式的 load/store：`MPRV=0` ⇒ **旁路**；`MPRV=1` ⇒ **受检** | `matrix4` M0 组 |
| F11 | **`MPRV` 不影响取指** | `matrix5`（`MPRV=1` 时取指仍按 `X` 判定） |
| F12 | **`L`=1 ⇒ 该条目在 M 模式也强制执行**（`L`=0 ⇒ M 模式旁路） | `matrix4` L 组 |
| F13 | **`L`=1 ⇒ 写 `pmpcfg[i]` 与 `pmpaddr[i]` 被忽略**（锁到复位为止） | `matrix10`：`pmpcfg0` 读回 `0x9f`、`pmpaddr0` 读回 `0x7fffffff` |
| F14 | **取指受 `X` 约束** ⇒ 无 `X` ⇒ `mcause 1`（instruction access fault），`mtval` = 取指地址 | `matrix5` 四变体（`0x88`/`0x89`/`0x8a` 被拒、`0x8c` 放行） |
| F15 | 参考侧实现 **16 条**：`pmpcfg0..3` + `pmpaddr0..15`；`pmpaddr16` 与 `pmpcfg4` 读回 **0** | `matrix8` |
| F16 | `pmpaddr` **逐字存储**（写 `0x20000123` 读回同值，不做 NAPOT 归一化） | `matrix12` |
| F17 | 拒绝时的 cause：**取指 1 / 读 5 / 写 7**，`mtval` = 出错地址 | 各矩阵的 `mcause` / `mtval` 列 |
| F18 | 参考侧 PASS 路径**静默退出码 0**；FAIL 路径退出码 = `TESTNUM` | `p11` 两路径 |
| F19 | env 的 `INIT_PMP`（`pmpaddr0 = 0x7fffffff`、`pmpcfg0 = 0x1f`）⇒ `n`=31 ⇒ 区间 = `[0, 2^34)` ⇒ **覆盖整个 32 位空间** ⇒ 参考侧一直在「一条全放行条目」下跑 | 由 F4 推导 + `p11` 基线（`pmpcfg0` 读回 `0x1f`） |

### §1.3 **两条被推翻的旧结论**（必须写进规格，否则会被再犯）

| 旧结论 | 错在哪 | 正确结论 |
| --- | --- | --- |
| 「规范里 `X`=bit0、`R`=bit2，与上游 `encoding.h` 的 `R`=bit0 冲突」 | 规范 §3.7.6 的字节布局就是 **`R`=bit0、`W`=bit1、`X`=bit2** | 上游 `encoding.h` **与规范一致**，**没有冲突**（F1） |
| 「多条目命中时**任一放行即可**（最宽松胜）」 | 探针的 `pmpcfg1` **配的是条目 4..7**（F2）⇒ ⇒ 条目 1 从未被配置 ⇒ ⇒ 测的其实只有条目 0 | **最低编号的命中条目说了算**（F7） |

## §2 契约层设计

### §2.1 形状：**零新 DTO**（裁决 A）

PMP 的状态**就是 CSR 文件里的那些寄存器** ⇒ ⇒ 不需要新的快照字段
⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **不升** ⇒ ⇒ 前端镜像**不用改**。

### §2.2 新增常量（裁决 B：唯一落点在契约层）

`crates/contract/src/csr.rs` 新增 **20 个** CSR 地址常量：

| 常量 | 地址 | 条数 |
| --- | --- | --- |
| `CSR_PMPCFG0` .. `CSR_PMPCFG3` | `0x3A0` .. `0x3A3` | 4 |
| `CSR_PMPADDR0` .. `CSR_PMPADDR15` | `0x3B0` .. `0x3BF` | 16 |

外加：

- `PMP_ENTRY_COUNT = 16`（裁决 C：对齐参考侧，见 F15）。
- `PMP_GRAINULARITY_SHIFT = 2`（F3：地址右移 2 位）。
- `PMP_CFG_A_MASK` / `PMP_CFG_A_SHIFT` / `PMP_CFG_R` / `PMP_CFG_W` / `PMP_CFG_X`
  / `PMP_CFG_L`（F1：位常量，**不写魔法数**）。

### §2.3 **纯函数**：解码 / 区间 / 匹配 / 判定（裁决 D）

契约层提供**四个纯函数**（只读入参、不碰状态 ⇒ ⇒ 内核与契约测试都能用）：

| 函数 | 职责 | 依据 |
| --- | --- | --- |
| `pmp_decode_cfg(byte) -> PmpCfg` | 解出 `L` / `A` / `R` / `W` / `X` | F1 |
| `pmp_region(entry, prev_addr, cur_addr) -> Option<(u32, u32)>` | 算出条目的 `(base, size)`；`A`=OFF ⇒ `None` | F4 / F5 / F6 |
| `pmp_entry_matches(entry, addr, size) -> bool` | 条目是否**覆盖访问的全部字节** | 规范 §3.7.6 |
| `pmp_access_allowed(...) -> bool` | 整条判定链（优先级 + 权限 + 特权口径） | F7 ~ F12 |

⚠️ **`pmp_region` 的半开 / 闭区间必须逐字按 F4/F5/F6 写**（NAPOT 与 TOR 是**半开**，
NA4 是**闭**）⇒ ⇒ 这三个区间的差一位就是一条判别力。

### §2.4 `csr_is_implemented` 的新增项（裁决 E）

`pmpcfg0..3` 与 `pmpaddr0..15` **必须**被 `csr_is_implemented` 认作已实现
⇒ ⇒ 否则 `csr_write_is_ignored` 会把写吞掉 ⇒ ⇒ **PMP 变成一排不起作用的寄存器**
⇒ ⇒ 这条是「真实现」的**底线**。

⚠️ `is_csr_read_only` **不用改**：`0x3A0..0x3BF` 落在 `0x300..0x3FF`，
既不是 `0xC00` 掩码命中的只读段，也不命中 `0xA00`/`0xB00` 的只读切片
⇒ ⇒ 默认**可写** ⇒ ⇒ 与 F13（`L`=1 时写被忽略）是**两件事**，不要混。

## §3 内核设计

### §3.1 状态：**零新增**（裁决 F）

PMP 的配置**住在 CSR 文件里**（`pmpcfg0..3` + `pmpaddr0..15`）⇒ ⇒ **零新增状态**
⇒ ⇒ 快照/恢复天然一致。

### §3.2 判定的**唯一落点**（裁决 G）

`crates/kernel/src/m13_iss/pmp.rs`（新文件）提供**一个**入口：

```
pub(crate) fn pmp_allows(state, addr, size, kind) -> bool
```

⇒ ⇒ 三条访存路径（load / store / 取指）**都调它** ⇒ ⇒ 「一个约束一个落点」。

落点（**三个调用点**）：

| 访存 | 调用点 |
| --- | --- |
| 读 | `crates/kernel/src/m13_iss/executor.rs` 的 load 分支（`mem.read` 之前） |
| 写 | `crates/kernel/src/m13_iss/executor.rs` 的 store 分支（`mem.write` 之前） |
| 取指 | `crates/kernel/src/m13_iss/mod.rs` 的取指分支（`read_fetch_instruction` 之前） |

### §3.3 判定链（裁决 H）

1. **找决定条目**：按条目编号从小到大，找**第一个覆盖访问全部字节**的条目
   （`A`≠OFF 且区间命中）⇒ ⇒ **命中即定**（F7）。
2. **没命中**：若存在 ≥1 个 `A`≠OFF 的条目 ⇒ ⇒ **拒**（F8）；否则 ⇒ 放行
   （「PMP 未实现 ⇒ 全部放行」）。
3. **特权口径**（F10 / F11 / F12）：
   - 访存是 load/store：M 模式且 `MPRV`=0 ⇒ **旁路**；
   - 访存是取指：`MPRV` **不参与**（F11）；
   - 决定条目的 `L`=1 ⇒ **在 M 模式也强制执行**（F12）；
   - 决定条目的 `L`=0 且当前是 M 模式（load/store 还要求 `MPRV`=0）⇒ ⇒ **放行**。
4. **权限位**：读要 `R`、写要 `W`、取指要 `X`；**`R`=0 且 `W`=1 ⇒ 写被拒**
   （保留组合，F9）。

### §3.4 热路径：**一次掩码判断**（裁决 I）

M 模式 + `MPRV`=0 是**绝大多数访存**的形态 ⇒ ⇒ 这条路径必须是 O(1)：

- 先看**四个 `pmpcfg` 字节里有没有 `L`=1**（4 次 CSR 读 + 掩码）⇒ ⇒ 没有 ⇒ **放行**；
- 有 ⇒ ⇒ 走完整扫描（慢路径，但只在有锁条目时才会发生）。

⇒ ⇒ **不重建条目数组、不缓存第二份状态** ⇒ ⇒ 与 v0.36 的「影子是推导量」同源。

### §3.5 拒绝 ⇒ 架构陷阱（裁决 J）

| 访存 | cause | `mcause` | `mtval` |
| --- | --- | --- | --- |
| 取指 | `TrapCause::InstructionAccessFault` | 1 | 取指地址 |
| 读 | `TrapCause::LoadAccessFault` | 5 | 出错地址 |
| 写 | `TrapCause::StoreAccessFault` | 7 | 出错地址 |

⚠️ **与未映射访问同 cause**（未映射读也是 5、未映射写也是 7）⇒ ⇒ 区分靠
「地址是否映射」或 `mtval` ⇒ ⇒ 判据必须用**映射内**的地址（§4.2）。

### §3.6 **PMP 检查在未映射检查之前**（裁决 K）

规范把 PMP 放在物理地址一级 ⇒ ⇒ 顺序：PMP ⇒ 未映射 ⇒ 设备属性。
⇒ ⇒ 落点：三个调用点都在**访存之前**判定（PMP 先，未映射后）⇒ ⇒
「未映射且被 PMP 拒」的地址，cause 与 `mtval` 与「仅被 PMP 拒」**相同**
⇒ ⇒ 两侧一致（参考侧同样是 PMP 先）。

## §4 工具层设计

### §4.1 默认 Golden ISA：**不动**（裁决 N）

本批**不新增** ISA 扩展 ⇒ ⇒ `tools/golden-run.sh` 的默认 ISA 串
`rv32imc_zicsr_zicclsm_zifencei_zicntr` **保持不变** ⇒ ⇒ `zicsr` 已在里面
⇒ ⇒ 写 `pmpcfg`/`pmpaddr` 的指令**可以编出来**。

### §4.2 语料：**新增一条** `local/pmp_deny`（裁决 L）

`tests/golden/local/pmp_deny.S`（**自包含**，不引上游源码）：

- 条目 0（`pmpcfg0` 字节 0）= NAPOT 4 KiB @ `0x80040000`，**无权限**
  ⇒ `pmpaddr0 = 0x200101ff`、`pmpcfg0` 字节 0 = `0x18`；
- 条目 1（`pmpcfg0` 字节 1）= NAPOT 1 MiB @ `0x80000000`，`R`+`W`+`X`
  ⇒ `pmpaddr1 = 0x2001ffff`、`pmpcfg0` 字节 1 = `0x1f`
  ⇒ ⇒ **整条 `pmpcfg0` = `0x1f18`**（⚠️ 打包，见 F2）；
- `mstatus.MPRV` = 1；
- **控制组**：在 `0x80002000`（条目 1 内）写后读回 ⇒ 必须成功
  ⇒ ⇒ 没有它，「全拒」的实现也会绿；
- **观测点**：写 `0x80040000`（条目 0 命中、无权限 ⇒ 拒）⇒ guest 自查
  `mcause == 7`、`mtval == 0x80040000`、`mepc == 那条 store` ⇒ 全对才 PASS。

⚠️ **`0x80040000` 在两侧的 RAM 窗口内**（`0x8000_0000` + 4 MiB）⇒ ⇒
被拒**只可能**因 PMP ⇒ ⇒ 与 `local/unmapped_store.S`（`0x90000000`，两侧都未映射）
**可区分**。

⚠️ **`rv32mi/pmpaddr` 不入库**（裁决 O）：参考侧那条用例走「trivially passes」
早退路 ⇒ ⇒ 是**空洞** ⇒ ⇒ 语料靠 `local/pmp_deny` 这条**自包含**用例。

### §4.3 `tests/golden/README.md`：**登记 env 注释的偏差**（裁决 M）

仓内 env 是上游**逐字镜像** ⇒ ⇒ 那句「本平台无 PMP ⇒ 写被忽略 ⇒ 全放行」
**不能改** ⇒ ⇒ 但它**过期**了 ⇒ ⇒ 在 README 里写明：

- env 的 `INIT_PMP` 在本平台**会生效**（不再被忽略）；
- 但它配的区间**覆盖整个 32 位空间**（F19）⇒ ⇒ 行为**仍是全放行**
  ⇒ ⇒ **既有 68 条语料的判决不变**。

## §5 验收

### §5.1 A 组 · 契约层

| 编号 | 验收项 |
| --- | --- |
| A-1 | `pmpcfg0..3` 与 `pmpaddr0..15` 的地址常量与 F1/F2/F3 一致 |
| A-2 | `csr_is_implemented` 认这 20 个 CSR（裁决 E） |
| A-3 | `pmp_decode_cfg` 解出的 `L`/`A`/`R`/`W`/`X` 与 F1 一致（逐位） |
| A-4 | `pmp_region` 对 NAPOT / NA4 / TOR 三种模式给出的区间与 F4/F5/F6 **逐字**一致（含半开与闭） |
| A-5 | `pmp_access_allowed` 的优先级判定与 F7 一致（低编号命中条目说了算） |
| A-6 | `R`=0 ∧ `W`=1 ⇒ 写被拒（F9） |
| A-7 | 契约层**零新 DTO**：快照 DTO 的字段集合未变 |

### §5.2 B 组 · 内核

| 编号 | 验收项 |
| --- | --- |
| B-1 | 写 `pmpcfg0`/`pmpaddr0` 后**读回逐字相同**（F16） |
| B-2 | `MPRV`=0 时 M 模式访存**不受 PMP 约束**（F10） |
| B-3 | `MPRV`=1 时 M 模式访存**受 PMP 约束**（F10） |
| B-4 | 取指**不受 `MPRV` 影响**，但**受 `X` 约束**（F11 + F14） |
| B-5 | `L`=1 ⇒ M 模式也强制执行（F12） |
| B-6 | `L`=1 ⇒ 写 `pmpcfg`/`pmpaddr` 被忽略（F13） |
| B-7 | 拒绝 ⇒ cause 1/5/7 + `mtval` = 出错地址（F17） |
| B-8 | 无条目命中 ⇒ 拒（F8） |
| B-9 | 热路径是 O(1) 掩码判断，不重建条目数组（裁决 I） |

### §5.3 C 组 · 工具层

| 编号 | 验收项 |
| --- | --- |
| C-1 | `local/pmp_deny` 入 `tests/golden/corpus.txt`，参考侧判 **PASS** |
| C-2 | 同一条语料在**未实现 PMP** 的平台上判 **FAIL 3** ⇒ ⇒ 判据有判别力 |
| C-3 | `tests/golden/README.md` 登记 env 注释的偏差（裁决 M） |
| C-4 | 既有 68 条语料**判决不变**（F19 ⇒ 本批对它们零影响） |
| C-5 | 默认 Golden ISA 串**未变**（裁决 N） |

### §5.4 T 组 · 台账与文档

| 编号 | 验收项 |
| --- | --- |
| T-1 | 台账 D-94 末列追加本批处置（闭环第 ② 件事，**整体仍未闭环** ⇒ 不参与减法） |
| T-2 | 台账登记「与参考侧/规范的**有意偏离**」条目（若存在） |
| T-3 | 收口文档三处同步：README / AGENTS.md / PROGRESS.md |
| T-4 | 规模数字用 `tools/count_scale.py` 从当前树**重算** |

### §5.5 X 组 · 跨层与判别力

| 编号 | 验收项 |
| --- | --- |
| X-1 | 变异 M1（关掉判定）⇒ 判据语料**变红**（参考侧 PASS / 本平台 FAIL 3） |
| X-2 | 变异 M2（把优先级改成「任一放行即可」）⇒ 判别力**红**（必须能区分 F7） |
| X-3 | 变异 M3（把 `pmpaddr` 刻度改回「原始地址」）⇒ 判别力**红**（必须能区分 F3） |
| X-4 | 三条变异的**红集互不相同** |

## §6 门禁

| 门禁 | 命令 |
| --- | --- |
| 格式 | `cargo fmt --all -- --check` |
| 静态 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 单元/集成 | `cargo test --workspace --no-fail-fast --lib --tests` |
| 前端 | `tools/check-front.sh` |
| 黄金对拍 | `tools/golden-run.sh --self-check` 与 `tools/golden-run.sh --check-mem --keep-going` |
| WASM | `tools/build-wasm.sh` 与 `node tools/wasm-smoke.mjs` |
| 浏览器 | `node tools/browser-verify.mjs` |
| 真实 RTOS | `tools/boot-osz.sh` |
| 规格自检 | `python3 tools/stage21_check.py docs/detailed-design/STAGE-21_V037_PMP.md` |
| 台账口径 | `python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md` 与 `--self-test` |
| 文档 | `python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt` |
| 归属口径 | `python3 tools/front_test_attrib.py` 与 `--selftest` |

⚠️ 判「全绿」必须看**退出码** + `grep -cE "^test result: FAILED"` 为 **0**
⇒ ⇒ 汇总的失败数在 `awk -F'[ ;]'` 的 **`$7`**（不是 `$6`）。

## §7 裁决

### 裁决 A · 本批范围 = D-94 的 ②

PMP 是 D-94 拆出的第 2 件事 ⇒ ⇒ 本批做 PMP，S 模式顺延为 STAGE-22 及再往后。

### 裁决 B · 契约层**零升版**

`SCHEMA_VERSION` 仍 7、`SIM_STATE_SCHEMA_VERSION` 仍 2、`PLATFORM_SCHEMA_VERSION` 仍 1
⇒ ⇒ 前端镜像不改 ⇒ ⇒ 不制造 D-143 的存量问题。

### 裁决 C · 条目数 = **16**（对齐参考侧）

参考侧只实现 16 条（F15）⇒ ⇒ 本批也做 16 条 ⇒ ⇒ 扩到 64 条是**纯增量**，
但会与参考侧分歧 ⇒ ⇒ **不做**。

### 裁决 D · 解码 / 区间 / 匹配 / 判定 = **契约层纯函数**

四个纯函数都在契约层（§2.3）⇒ ⇒ 内核只负责「取状态 + 调用 + 陷阱化」
⇒ ⇒ 「一个约束一个落点」。

### 裁决 E · `csr_is_implemented` **必须**补入 PMP CSR

否则写被忽略 ⇒ ⇒ PMP 变成一排不起作用的寄存器 ⇒ ⇒ 这条是底线。

### 裁决 F · PMP 状态**零新增**

住在 CSR 文件里 ⇒ ⇒ 快照天然一致。

### 裁决 G · 判定的**唯一入口**

`pmp_allows` 一个函数，三条访存路径都调它。

### 裁决 H · 判定链的**顺序**钉死

命中条目 → 特权口径 → 权限位 ⇒ ⇒ 三步的顺序不可换（换了就是另一个语义）。

### 裁决 I · 热路径 = **一次掩码判断**

M 模式 + `MPRV`=0 ⇒ ⇒ 看四个 `pmpcfg` 字节有没有 `L`=1 ⇒ ⇒ 没有就放行
⇒ ⇒ 不重建条目数组、不缓存第二份状态。

### 裁决 J · 拒绝 ⇒ 架构陷阱（cause 1/5/7）

`mtval` = 出错地址 ⇒ ⇒ 与未映射同 cause ⇒ ⇒ 靠「是否映射」区分。

### 裁决 K · **PMP 检查在未映射检查之前**

规范顺序 ⇒ ⇒ 落点：三个调用点都在访存之前判 PMP。

### 裁决 L · 语料 = **新增 `local/pmp_deny`**；`rv32mi/pmpaddr` **不入库**

参考侧那条是空洞（走早退路）⇒ ⇒ 靠自包含用例判别。

### 裁决 M · env **逐字不动**，偏差登记在 `tests/golden/README.md`

`golden-run.sh` 的 `check_env_constants` 会逐字比对上游 ⇒ ⇒ 改 env = 硬错误。

### 裁决 N · 默认 Golden ISA **不动**

本批不新增 ISA 扩展 ⇒ ⇒ `zicsr` 已在默认串里。

### 裁决 O · `rv32mi/pmpaddr` 的处置 = **不入库**（空洞）

与 v0.36 批对 `rv32mi/zicntr` 的处置**同形** ⇒ ⇒ 语料靠 `local/pmp_deny`。

## §8 施工期订正

### 订正 1 · **RV32 的 `pmpcfg` 一个寄存器装 4 个条目**（初稿判错 ⇒ 已改）

初稿把「条目 1」写成 `pmpcfg1` ⇒ ⇒ **错**：`pmpcfg1` 装的是**条目 4..7**
⇒ ⇒ 探针里**条目 1 从未被配置** ⇒ ⇒ 前面若干测量其实**只测了条目 0**。

⇒ ⇒ 现象：一度测出「条目 0 不命中时仍由条目 0 拍板」这种**不可能**的语义。
⇒ ⇒ 修正后（条目 1 打进 `pmpcfg0` 的字节 1）重测 ⇒ ⇒ 得到**规范语义**
（最低编号的命中条目说了算，F7）。

⇒ ⇒ **教训**：「`pmpcfg1` = 第二个条目」是**想当然**；CSR 编号与**条目编号**
是两套编号 ⇒ ⇒ 任何涉及多条目的探针，**必须先证明第二个条目真的生效了**
（判别式：**换一个地址，让两个条目的区间不重叠**）。

### 订正 2 · **`pmpaddr` 的刻度 = 地址右移 2 位**（初稿判错 ⇒ 已改）

初稿把 `pmpaddr` 当**原始地址**用 ⇒ ⇒ 于是「NAPOT 4 KiB @ `0x80000000`」写成了
`pmpaddr0 = 0x80000ff8` ⇒ ⇒ 该编码**根本没命中**目标地址 ⇒ ⇒ 「被拒」被误读成
「匹配了但没权限」⇒ ⇒ **变量没分开**。

⇒ ⇒ 修正：`pmpaddr = addr >> 2`（F3）⇒ ⇒ 用两套刻度做对照
（解释 A = `addr>>2`、解释 B = 原始地址）⇒ ⇒ A 全对、B 全拒 ⇒ ⇒ 定案 A。

⇒ ⇒ **教训**：**「被拒」不能推出「匹配上了」** ⇒ ⇒ 每次测权限前，
必须先用一个**已证明匹配**的编码做基准。

### 订正 3 · **写权限需要 `R` ∧ `W`**（不是「`W` 单独有效」）

初稿按规范表「`R`=0、`W`=1 ⇒ 写 Yes」设计 ⇒ ⇒ 实测该组合**被拒** ⇒ ⇒
查规范原文才发现：**该组合是保留的**（规范 §3.7.6 明写「`R`=0 和 `W`=1 的组合被保留」）
⇒ ⇒ 保留组合 ⇒ ⇒ 实现按「保留 ⇒ 不产生写权限」处理（F9）。

### 订正 4 · `pmpcfg` 的**位序**与上游 `encoding.h` **一致**（初稿判错 ⇒ 已改）

初稿记的是「规范 `X`=bit0 / `R`=bit2 ⇒ 与上游 `PMP_R 0x01` 冲突」⇒ ⇒ 实测
**bit0 就是 `R`**（`0x19` 放行读、`0x1c` 被拒）⇒ ⇒ 与上游 `encoding.h` **一致**
⇒ ⇒ **没有冲突** ⇒ ⇒ 仓内 `tests/golden/env/encoding.h` **一个字都不用改**
（本来也不该改——`check_env_constants` 会逐字比对上游）。

### 订正 5 · **取指是受 PMP 约束的**（初稿判错 ⇒ 已改）

初稿据一次探测得出「参考侧不对取指做 PMP 检查」⇒ ⇒ 那次探测**踩了订正 1 的坑**
（条目 1 没生效）⇒ ⇒ 修正打包后重测 ⇒ ⇒ **无 `X` ⇒ `mcause 1`**（F14）
⇒ ⇒ 本平台**按规范实现取指检查**（`X` 位因此**不是摆设**）。

## §9 变异自检（**每 lane 至少一条对照 0 红**）

### §9.1 纪律（**沿用，不得放宽**）

1. 必须能编译；期望集合 **AND** 语义；记录**完整红集**。
2. 还原用 `cp` + `md5` + `cmp` + **`touch`（当前时间，不是 `touch -r`）**
   ⇒ ⇒ 禁 `git checkout -- <file>`。
3. 备份名用**相对路径**做键；变异期间不改其它被编译源文件；必须有超时。
4. 每 lane 至少一条**对照 0 红**。
5. ⚠️ **变异必须打在被测路径真正走的那条分支上**；
   ⚠️ **「自检脚本绿了」不算判别力证据**。

### §9.2 计划中的三条变异

| 编号 | 变异 | 期望红 |
| --- | --- | --- |
| M1 | 关掉判定（`pmp_allows` 恒真） | 判据语料红（参考侧 PASS / 本平台 FAIL 3） |
| M2 | 优先级改成「任一命中条目放行即可」 | 红（必须能区分 F7） |
| M3 | `pmpaddr` 刻度改回「原始地址」 | 红（必须能区分 F3） |

## §10 风险与已知边界

1. **PMP 在本平台只能被 `MPRV=1` 观测到**（D-94 的第 ③ 件事没做）⇒ ⇒
   语料必须显式开 `MPRV`。
2. **既有 68 条语料对本批零影响**（F19）⇒ ⇒ 本批**不改变**任何既有判决 ⇒ ⇒
   判别力全部由新增的 `local/pmp_deny` 承担 ⇒ ⇒ **那条用例必须真红**。
3. **与未映射访问同 cause** ⇒ ⇒ 单看 cause 分不出 PMP 拒绝与未映射 ⇒ ⇒
   判据用**映射内**地址（§4.2）。
4. **热路径只覆盖「M 模式 + `MPRV`=0 + 无锁条目」** ⇒ ⇒ 有锁条目时退化为线性扫描
   ⇒ ⇒ 性能可接受，但**不是常数时间**。
5. **`pmpcfg4..15` 与 `pmpaddr16..63` 不实现** ⇒ ⇒ 写被忽略、读回 0（WARL）。

## §11 复现命令

### §11.1 参考侧探针（本批的**唯一**事实来源）

```bash
# 探针在仓外（/tmp/v037-probe），每个观测一个独立 ELF
cd /tmp/v037-probe && ./run_matrix.sh matrix16.txt   # 优先级（F7）
cd /tmp/v037-probe && SRC=p14_fetch.S ./run_matrix.sh matrix5.txt   # 取指（F14）
```

### §11.2 判据（两侧同程序）

```bash
cd /Users/zyzs/lczyzs/code/riscv_sim
printf 'local pmp_deny rv32i_zicsr_zifencei active csr\n' > /tmp/v037-probe/manifest_pmp.txt
GOLDEN_MANIFEST=/tmp/v037-probe/manifest_pmp.txt \
  perl tools/watchdog.pl 900 tools/golden-run.sh --keep-going
# ⇒ 期望：spike=PASS  ours=FAIL 3（本批实现前）⇒ 本批实现后 ours=PASS
```

### §11.3 全量门禁

```bash
perl tools/watchdog.pl 1800 cargo test --workspace --no-fail-fast --lib --tests
perl tools/watchdog.pl 1800 cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
perl tools/watchdog.pl 3600 tools/golden-run.sh --check-mem --keep-going
python3 tools/stage21_check.py docs/detailed-design/STAGE-21_V037_PMP.md
python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md
python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')
python3 tools/count_scale.py
```
