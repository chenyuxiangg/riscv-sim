# M-13 RISC-V ISS（RV32IMC）· 代码审查报告

> **审查对象**：`crates/kernel/src/m13_iss/`（5 个文件，2581 行：`alu.rs` 78 / `decoder.rs` 146 / `executor.rs` 651 / `mod.rs` 296 / `tests_rv32i.rs` 1410）
> **审查基准**：`docs/detailed-design/M-13_RISC-V_ISS_RV32IMC.html`（LLD §4.13）、`docs/reviews/CROSS_MODULE_DEBT.md`（D-04 / D-11 / D-16 / D-23）
> **审查日期**：2026-10-20
> **审查方式**：全文件通读 + 调用方交叉核对（M-12 / M-17 / M-08 / M-09 / M-20 / M-23 / M-22 / M-06）+ 指令语义逐条对拍 RISC-V 非特权规范 v20260120（RV32I 2.1）
> **门禁状态**：`cargo test --workspace` 277 passed / 0 failed（基线 203）；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：M-13 由「只有 ADDI 一条指令」扩到 **RV32I 全集（41 条指令 = LLD §3.3 的 48 条清单中实现 40 条 + FENCE.I）**，三条台账欠账（D-04 / D-11 / D-16）全部闭环。审查中另发现并修复 **1 项实现缺陷**（立即数符号扩展位宽错误，影响全部负立即数）。剩余 8 项为下一波（M 扩展 / C 扩展 / CSR / 陷阱投递）的工作，不在本轮范围。

---

## 一、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 处置 |
| --- | --- | --- | --- | --- | --- |
| 1 | **阻断** | `executor.rs` / `mod.rs`（原 `:97` / `:161`） | **D-04：x0 未硬连线为 0**。两处裸写 `state.gpr[d.rd] = result`，裸读 `state.gpr[d.rs1]`，无任何守卫 | x0 写能污染后续全部读取 x0 的语义。第一条 Spike golden 用例即红。**已修复** | ✅ 引入 `read_gpr()` / `write_gpr()`（`executor.rs:76` / `:89`），写侧丢弃 `rd == 0`，读侧恒返回 0；全模块仅这两个函数触碰寄存器文件（`grep gpr\[` 证实生产代码零旁路） |
| 2 | **高** | `mod.rs`（原 `:140`） | **D-11：`BusExecutor` 复制 `Executor` 的 ADDI 语义**，`bus` 字段挂 `#[allow(dead_code)]` 从未被读；两条路径的非法指令语义还不同（`Trapped` vs `Err`） | LOAD/STORE 落地后会**静默绕过总线**（丢 stats 与 M-18 hook）。**已修复** | ✅ 引入 `MemoryAccess` 端口（`executor.rs:102`）+ 唯一执行路径 `execute_one()`（`executor.rs:441`）。`BusExecutor` 删除，改为只做访存适配的 `BusMemory`（`mod.rs:166`）。两条路径的非法指令语义统一为 `Trapped { IllegalInstruction }` |
| 3 | **中** | `decoder.rs`（原 `Decoded`） | **D-16：`Decoded` 无指令长度字段，PC 恒 `+4`** | RV32IMC 落地前必须先改结构，否则解码结构要返工。**已修复** | ✅ 新增 `pub len: u8`（`decoder.rs:43`）与 `LEN_32` / `LEN_16` 常量；解码器按 `raw & 0b11` 判定宽度（32 位编码低位恒 `0b11`，压缩编码恒不为）；PC 步进改为按 `d.len` |
| 4 | **高** | `decoder.rs` `immediate()` | **本轮新发现：S / B / J 型立即数符号扩展位宽少算一位**（原写 11 / 12 / 20，正确为 12 / 13 / 21） | 符号位（S 型 `imm[11]`、B 型 `imm[12]`、J 型 `imm[20]`）在左移时被移出 32 位，**所有负立即数都会算错偏移量**。例：B 型 `-8` 被解成 `-384`、J 型 `+0x10_0000`（越界值）被解成 `-0x10_0000` | ✅ 已修正为 12 / 13 / 21 位；补 `branch_immediate_survives_every_field` / `jal_immediate_survives_every_field` 逐字段用例锁定 |
| 5 | **中** | LLD §3.1 vs §3.3 | **LLD 自身矛盾**：§3.1 立即数表写「U 型 `imm[31:12] = inst[31:12]`，低 12 位补 0」，§3.3 执行列却写 LUI 的执行动作是 `regFile[rd] = imm<<12` | 两者不可同真：若 `imm` 已含左移，再 `<<12` 即二次左移（`lui x1, 0x12345` 会得到 `0x2345_0000` 而非 `0x1234_5000`） | 本实现按 §3.1 立即数表（U 型立即数即最终值，低 12 位为 0），LUI 直接 `rd = imm`、AUIPC `rd = pc + imm`。**理由**：§3.1 是立即数拼接的规范定义，§3.3 执行列是笔误。建议回写 LLD 修正 §3.3 |
| 6 | **中** | `executor.rs` `MisalignedPolicy`（`:175`） | **LLD §3.6(2) 要求非对齐访存「允许、不产生异常」（与 Spike 对齐）」，本轮契约要求「非对齐访存 → `Trapped { MisalignedAccess }`」 | RISC-V 规范允许二选一，两者都合法。硬编码任一方都会在另一方落地时返工 | **未二选一**：做成 `MisalignedPolicy` 开关（`Trap` / `Allow`），默认 `Trap`（满足本轮契约），`Iss::with_misaligned_policy()`（`mod.rs:79`）可切到 `Allow`（满足 LLD §3.6(2)）。Spike 对拍波次落地时翻转默认值即可，无需改结构 |
| 7 | **中** | `executor.rs` `execute_one()` | **LLD §3.2 要求键未命中 → E_ISS-01 平台错误码**，本实现统一为 `StepOutcome::Trapped { IllegalInstruction }` | E_ISS-01 是平台错误码，而非法指令本质是指令级同步异常（mcause = 2）。陷阱投递链路（mepc/mcause/mtval → mtvec）尚未接通，`StepOutcome::Trapped` 是当前唯一能表达「指令级同步异常」的通道 | 按本轮契约统一为 `Trapped`。**理由**：D-11 要求两条路径的非法指令语义一致，`Trapped` 是唯一自洽的选择。陷阱投递接通后，`Trapped` → E_ISS-01 的转换应在 M-12 侧完成（见 §四 #3） |
| 8 | **低** | `executor.rs` `TrapCause`（`:44`） | `TrapCause::IntegerDivideByZero` 为预留变体，本轮无任何产出路径（M 扩展属下一波） | 死变体。但本轮契约明确要求为 Wave 2 预留 | 保留，文档已注明「Reserved for the `M` extension」。M 扩展落地时直接产出，无需改枚举形状 |
| 9 | **低** | `executor.rs` / `mod.rs` | **陷阱投递链路未接通**：`StepOutcome::Trapped` 只带 `TrapCause`，不更新 `mepc` / `mcause` / `mtval`，也不跳转 `mtvec` | `SimState.csrs` 是 `Vec<(u16, u32)>` 契约字段，但 ISS 尚未读写它。M-16 / M-12 的中断交付也未接线 | 列为下一波。本轮不伪造 CSR 语义 |
| 10 | **低** | `executor.rs` / `mod.rs` | **通道不对称**：未对齐访存走 `Trapped`，而总线访问失败（未映射 / 外设错误）走 `Err(PlatformError { family: Iss, scenario: 1 })` | 两者都是指令级访问异常，但一个走陷阱、一个走平台错误，调用方要分两条路处理 | 保留现状并在此登记。**理由**：总线错误携带诊断消息（地址 + M-14 原始错误），`TrapCause` 是无载荷枚举，强行塞进去会丢信息。待 #9 的陷阱投递接通后，两者应统一为 `Trapped { OutOfRange }` + `mtval` 记地址 |
| 11 | **低** | `executor.rs` `read_gpr` / `write_gpr` | **x0 槽位不被擦洗**：ISS 丢弃对 x0 的写入，但不会把被外部写脏的 `state.gpr[0]` 改回 0 | 架构语义正确（x0 恒读作 0），但下游若直接读 `state.gpr[0]`（寄存器面板 / M-20 快照 / M-10 内存服务）会看到脏值 | ISS 侧不擦洗（写忽略是架构语义，擦洗是消毒，两回事）。**跨模块契约点**：建议在 `SimState.gpr` 的文档里写明「x0 槽位恒为 0 的保证由写入方负责」——需契约层决策，本次未改契约 |
| 12 | **低** | `decoder.rs` `Decoded` | **新增 `pub len` 字段对外部字面量构造是破坏性变更** | `Decoded` 的构造点全在 `m13_iss` 内部（已全库检索确认：仅 `m11_symbol` 的**文档注释**提到 `Decoded`，非构造点；`m17_backend` 只用 `Iss`） | 影响为零，保留加法式变更 |
| 13 | **低** | `executor.rs` `classify()` | **指令集仍有缺口**：M 扩展（8 条）、C 扩展（26 条）、CSR 指令（6 条）、MRET / WFI 未实现，命中即判非法指令 | LLD §3.3 把 FENCE 列入 RV32I，已实现（空操作）；其余属 Wave 2 | 下一波。压缩指令（`len == 2`）已显式判为非法指令陷阱，不会被误当作 32 位指令执行 |

---

## 二、值得肯定的设计（LLD 评审项落地情况）

| 评审项 / LLD 条目 | 设计点 | 落地情况 |
| --- | --- | --- |
| 13-5 | 解码键 = `opcode \| funct3 \| funct7`，两级分派 | ✅ `classify()`（`executor.rs:280`）按同一键空间实现：`Op` 枚举 + `classify` 匹配，键未命中即非法指令。键空间与 LLD §3.2 伪代码一一对应 |
| §3.1 | 立即数按 I / S / B / U / J 型拼接并符号扩展 | ✅ `decoder.rs` 的 `immediate()`，按 opcode 选格式；OP-IMM 移位指令特判为 5 位 shamt（LLD §3.3） |
| §3.1（评审 13-3） | 未对齐检查先于宽度判定与取指访存 | ✅ `mod.rs:97` / `:131`：`pc & 0x1 != 0` 先于取指判陷阱 |
| §3.3 | x0 写忽略 | ✅ `write_gpr()` 写侧丢弃；读侧恒 0 |
| §3.6(2) | 非对齐访存 | ⚠️ 做成策略开关，默认 `Trap`（见 §一 #6） |
| §3.6(5) | FENCE / WFI | ✅ FENCE / FENCE.I 实现为空操作；WFI 依赖 CSR，留待下一波 |
| §3.6(10) / 评审 13-8 | 每条指令至少一条正向用例 + 边界用例 | ✅ 41 条指令逐条有用例；边界覆盖负立即数、移位量 ≥ 32、SLT 跨符号边界、分支跳 / 不跳、B/J 型立即数逐字段、未对齐访存、非法码陷阱 |
| §2.1 | 指令为 `(gpr, mem, decoded)` 的纯函数，执行器不读宿主时钟 | ✅ `execute_one()` 是纯函数；`execution_is_deterministic` 用例断言同一程序两次运行结果完全一致 |
| D-11 | 单一执行路径 | ✅ `execute_one()` 是唯一的指令语义实现；`Executor<F, G>` 与 `BusMemory` 只是两个 `MemoryAccess` 适配器，语义不可能漂移 |

---

## 三、与 LLD 的偏离及理由（汇总）

| LLD 条目 | LLD 要求 | 本实现 | 理由 |
| --- | --- | --- | --- |
| §3.6(2) | 非对齐访存允许、不产生异常（对齐 Spike） | 默认陷阱，`MisalignedPolicy::Allow` 可切 | 本轮契约明确要求陷阱；RISC-V 规范允许二选一。做成开关而非二选一，两边需求都保住了 |
| §3.3（U 型执行列） | LUI 执行动作 `regFile[rd] = imm<<12` | `rd = imm`（立即数已含左移） | LLD §3.1 立即数表与 §3.3 执行列自相矛盾，按 §3.1 走。**建议回写 LLD** |
| §3.2 | 键未命中 → E_ISS-01 平台错误码 | `Trapped { IllegalInstruction }` | 陷阱投递链路未接通（#9），`StepOutcome::Trapped` 是当前唯一能表达指令级同步异常的通道；D-11 要求两条路径语义一致 |
| §3.3（48 条 RV32I） | 48 条含 CSR 指令 / ECALL / EBREAK / MRET / WFI | 实现 40 条（占 48 条的 83%）+ FENCE.I；CSR / MRET / WFI 未实现 | CSR 指令 / MRET / WFI 依赖尚未接通的 CSR 文件与陷阱投递（#9），硬实现只会做出「假语义」 |
| §2.1 | `Decoded` 含 `cat` / `compressed` / `compressedRaw` 字段 | 只实现 `len`；`compressed` 由 `len == 2` 表达 | `cat` / `compressedRaw` 服务于 M-11 反汇编与 mtval 记录，属 M-11 / 压缩扩展落地时的字段；本轮不引入无消费者的字段 |

---

## 四、剩余风险与下一波

1. **陷阱投递（最高优先）**：`StepOutcome::Trapped` 目前只带原因，不更新 `mepc` / `mcause` / `mtval`，不跳 `mtvec`。固件一旦依赖 ECALL 处理（`ecall` → `trap` → `mepc` → `mret`），当前实现无法闭环。M-16 / M-12 的中断交付同属此链。
2. **Spike 对拍（FR-04 必做）**：本轮已具备对拍前提（x0 正确、语义单一、立即数拼接有逐字段用例）。对拍前需先决定 `MisalignedPolicy` 的默认值——Spike 默认允许非对齐，届时应切到 `Allow`。
3. **M 扩展**：`Op` / `TrapCause::IntegerDivideByZero` 已预留位置；`MUL` 的 funct7 = `0000001` 当前判为非法指令（有用例锁定），Wave 2 落地时改 `classify()` 即可。
4. **C 扩展**：`Decoded.len` 已能区分 2 / 4 字节，压缩编码当前显式判为非法指令；C 落地还需改取指宽度（`pc % 4 == 2` 时取半字）与解码器展开层。
5. **跨模块契约点**：`SimState.gpr[0]` 恒 0 的保证由谁负责（§一 #11）——需契约层决策，本次未改契约。
6. **`Decoded` 与 `Op` 的公开面**：`classify()` / `Op` 已公开，供 M-11 反汇编复用；M-11 若要按 `funct3` / `funct7` 直接反汇编，需自行读 `Decoded` 字段而非依赖 `Op`（`Op` 只覆盖 RV32I 基础集）。

---

## 五、闭环核对（2026-10-20 复查）

模块规模 456 → **2581 行**（生产代码 456 → 1171 行，测试 0 → 1410 行）；M-13 用例 2 → **77 条**；`cargo test --workspace` 203 → **277 passed / 0 failed**。**D-04 / D-11 / D-16 三条台账欠账 100% 闭环**，本轮新发现的立即数符号扩展缺陷（#4）亦已闭环。

| 欠账 / 项 | 原严重度 | 状态 | 落地证据 |
| --- | --- | --- | --- |
| **D-04** x0 硬连线 | 阻断 | ✅ 闭环 | `read_gpr()` / `write_gpr()`（`executor.rs:76` / `:89`）；生产代码内 `gpr[` 仅出现在这两个函数（`grep` 核实）。用例：`addi_to_x0_leaves_x0_zero`、`addi_reading_x1_cannot_write_x0`、`bus_path_also_keeps_x0_zero`、`reading_x0_yields_zero_even_if_register_file_is_corrupted`、`load_into_x0_is_discarded` |
| **D-11** 统一执行路径 | 高 | ✅ 闭环 | `MemoryAccess` 端口（`executor.rs:102`）+ 唯一 `execute_one()`（`executor.rs:441`）；`BusExecutor` 已删除，替换为只做访存适配的 `BusMemory`（`mod.rs:166`），`#[allow(dead_code)]` 一并消除。用例：`illegal_instruction_reports_the_same_outcome_on_both_routes`（两条路径 outcome 必须相等）、`load_and_store_go_through_the_bus`（`bus.stats().reads` 证明访存确实经总线） |
| **D-16** 指令长度字段 | 中 | ✅ 闭环 | `Decoded.len`（`decoder.rs:43`）+ `LEN_32` / `LEN_16`；PC 按 `d.len` 步进。用例：`decode_reports_four_bytes_for_a_32bit_encoding`、`decode_reports_two_bytes_for_a_compressed_encoding`、`pc_advances_by_the_decoded_instruction_length`、`compressed_encoding_traps_and_leaves_the_pc_alone` |
| **D-23 第 3 项** 门禁缺口 | 低 | ✅ 闭环 | `addi x0, x0, 42` → `gpr[0] == 0` 的断言已进回归集（`addi_to_x0_leaves_x0_zero`），三条既有门禁对此用例全有感知 |
| #4 立即数符号扩展位宽 | 高 | ✅ 闭环 | S / B / J 型改为 12 / 13 / 21 位。用例：`branch_immediate_survives_every_field`（11 个偏移量）、`jal_immediate_survives_every_field`（9 个偏移量）、`decode_store_splices_the_immediate`（含 ±2047 / -2048 极值） |
| #5 LLD U 型矛盾 | 中 | ⚠️ **按 §3.1 实现，LLD 待回写** | 本报告 §三 已记录；需协调者决定是否回写 `docs/detailed-design/M-13_*.html` §3.3 的 LUI 执行列 |
| #6 非对齐访存策略 | 中 | ⚠️ **默认 Trap，LLD §3.6(2) 的 Allow 已就位** | `MisalignedPolicy`（`executor.rs:175`）+ `Iss::with_misaligned_policy()`（`mod.rs:79`）。用例：`misaligned_word_access_traps`、`misaligned_halfword_access_traps`、`misaligned_store_traps`、`byte_accesses_are_never_misaligned`、`allow_policy_performs_the_misaligned_access` |
| #7 非法指令错误码 | 中 | ⚠️ **按契约走 TrapOutcome，LLD §3.2 的 E_ISS-01 待陷阱投递接通后转换** | 统一在 `execute_one()` 内，两条路径一致。转换点建议放在 M-12 侧 |
| #8 `IntegerDivideByZero` 预留 | 低 | ✅ 闭环（预留态） | `TrapCause`（`executor.rs:44`）已声明并注明用途；M 扩展落地时直接产出 |
| #9 陷阱投递链路 | 低 | ❌ **未闭环（下一波）** | `StepOutcome::Trapped` 不更新 CSR、不跳 `mtvec`。依赖 CSR 文件与 M-16 / M-12 中断交付接线，**非本轮范围** |
| #10 通道不对称 | 低 | ❌ **未闭环（下一波）** | 未对齐走 `Trapped`、总线错误走 `Err(Iss, 1)`。待 #9 落地后统一为 `Trapped { OutOfRange }` + `mtval` 记地址 |
| #11 x0 槽位擦洗 | 低 | ❌ **未闭环（跨模块，需契约层决策）** | ISS 侧按架构语义「写忽略」实现，不做消毒。`SimState.gpr[0]` 的恒零保证需在契约文档中明确责任方——**本次未改契约** |
| #12 `Decoded` 字段追加 | 低 | ✅ 闭环 | 全库检索确认无外部构造点（`m11_symbol` 仅文档提及） |
| #13 指令集缺口 | 低 | ❌ **未闭环（下一波）** | M 扩展 / C 扩展 / CSR 指令 / MRET / WFI 命中即判非法指令（有用例锁定 `MUL` funct7 判非法） |

**统计**：模块内部项 13 条中 **9 条完全闭环**、**2 条按 LLD 冲突项以「策略开关 + 报告登记」方式带条件闭环**（#6 / #7）、**1 条待 LLD 回写**（#5）、**3 条明确移交下一波**（#9 / #10 / #13），**1 条跨模块待契约层决策**（#11）。
