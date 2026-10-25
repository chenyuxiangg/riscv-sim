# STAGE-26 · v0.41 · 三条欠账并行批 —— D-154（`mtval`）+ D-151（`time`）+ D-96（空洞探测器）+ D-155 销账

> **冻结日期**：2026-10-25
> **上一版**：STAGE-25（v0.40 · PMP 特权口径 · D-153 闭环）
> **本批的形状**：**三条 lane 并行**（用户 2026-10-25 选定**方案甲**）⇒ ⇒ **一条 lane 一组文件** ⇒ ⇒ 合并时只有**一个**文件被两条 lane 同时改（`m13_iss/executor.rs`）⇒ ⇒ 其余**零冲突**。
> **规格编号 = STAGE-26**：STAGE-24 已被 I2C 那批**预留**，STAGE-25 = v0.40 ⇒ ⇒ 本批**不**顺延 STAGE-24。
> **作者**：agent (mavis)

---

## §0 本批的定位：三条欠账一次并行 + 一条销账

### §0.1 本批**做**什么（**四件**）

| 编号 | 欠账 | 主题 | 落点 | 契约层新增面 |
| --- | --- | --- | --- | --- |
| **Lane A** | **D-154** | PMP 拒绝时上报的 `mtval` 与参考侧不一致 | 契约层 `crates/contract/src/csr.rs` + 内核 `crates/kernel/src/m13_iss/pmp.rs` + `crates/kernel/src/m13_iss/executor.rs` + 语料 `local/pmp_straddle.S` | **一个函数**（既有函数签名**不改**） |
| **Lane B** | **D-151** | `time` / `timeh` 恒读 0 ⇒ 真值要接 M-15 的 CLINT | 契约层 `crates/contract/src/port.rs` + 内核 `crates/kernel/src/m13_iss/csr.rs` + `crates/kernel/src/m13_iss/executor.rs` + `crates/kernel/src/m13_iss/mod.rs` + `crates/kernel/src/m15_peripheral/clint.rs` + `crates/kernel/src/m17_backend/mod.rs` + `crates/kernel/src/kernel_host/platform.rs` | **一条只读端口签名**（无 DTO） |
| **Lane C** | **D-96** | 三条「空洞的一致」没有**机器可检**的探测器 | `tools/vacuity_check.py`（新） + `crates/kernel/src/bin/riscv-golden.rs` | **零** |
| **统筹方** | **D-155** | v0.38 规格里「`mret` 的使能位方向与特权规范一致」那句**没有实测支撑** | 规范 `STAGE-22` 补一条事实 + 台账改判 | **零**（平台一个字都不改） |

### §0.2 本批**明确不做**的事（**七件**）

1. **不实现 I2C / 新器件类型**（⇒ ⇒ 那批仍落在 **STAGE-24**）。
2. **不把 `mtime` 塞进 `SimState`**（⇒ ⇒ 台账 D-151 的**做法 C** ⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **2 → 3** ⇒ ⇒ 破坏性 ⇒ ⇒ **不做**，见**裁决 F**）。
3. **不给 `time` 新增黄金语料**（⇒ ⇒ 两侧的 `time` **不可比** ⇒ ⇒ 会假红 ⇒ ⇒ 判别力**全部**落在 Rust 单测，见**裁决 J**）。
4. **不改未映射访存的 `mtval` 口径**（⇒ ⇒ 那是「出错的那次访问」⇒ ⇒ 两个口径**不许**揉成一个，见**裁决 E**）。
5. **不动取指路径的 `mtval`**（⇒ ⇒ 取指 4 字节对齐 ⇒ ⇒ 永不跨界，见**裁决 D**）。
6. **不升级任何版本常量**（⇒ ⇒ `SCHEMA_VERSION` 仍 **7** / `SIM_STATE_SCHEMA_VERSION` 仍 **2** / `PLATFORM_SCHEMA_VERSION` 仍 **1**，见**裁决 A**）。
7. **不重写 611 个 commit 的 author**（⇒ ⇒ 用户早已定案：**保持现状，不重写历史**）。

### §0.3 为什么本批**必须改实现**（而不是「只测一测」）

* **D-154**：平台报「**访问的有效地址**」，参考侧报「**出错那一片的起始地址**」⇒ ⇒ **两个不同的数** ⇒ ⇒ 只测不改 ⇒ ⇒ 差异**永远在**。
* **D-151**：平台的 `csr_read` 对 `time` / `timeh` 有**显式恒 0 特判** ⇒ ⇒ 真值**拿不到** ⇒ ⇒ 只测不改 ⇒ ⇒ `csrr time` **永远读 0** ⇒ ⇒ 护栏挡的是畸形载荷，**不是**真值通道。
* **D-96**：今天「空洞的一致」靠**人读规格**发现 ⇒ ⇒ 机器**看不见** ⇒ ⇒ 只测不改 ⇒ ⇒ 工具层**零新增**。
* **D-155**：平台与参考侧**逐位一致** ⇒ ⇒ **不是**平台 bug ⇒ ⇒ **平台一个字都不改** ⇒ ⇒ 只补一条**实测事实**。

---

## §1 事实：**实测**与**原文**（不是推断）

### §1.1 证据来源

| 来源 | 怎么取的 | 能证明什么 |
| --- | --- | --- |
| **v0.39 批的仓外探针**（`/tmp/v039-probe/`） | Spike + 自包含 ELF，两侧同跑 | `mtval` 的**参考侧真值**（F1） |
| **v0.40 批的仓外探针**（`/tmp/v040-mret-probe/`） | 三个 `mstatus` 初值变体，退出码通道 | `mret` 后 `MIE` / `MPIE` 的**两侧实测**（F11） |
| **riscv-privileged v1.13 原文**（用户 2026-10-25 提供） | `mutool draw -F txt` 抽取 | **MRET 使能位的规范条文**（F12） |
| **本仓当前树** | `grep` / 读源码 | 落点、调用点、口径（F2 ~ F10、F13） |

⚠️ **观测通道的带宽**（本项目的通用纪律）：**探针走进程退出码只有 8 位** ⇒ ⇒ 32 位量**必须**拆成 4 次单字节；**逐位测之前必须先清源 CSR**；**绝不用两个聚合观测的差反推一个值** ⇒ ⇒ 本批的 F11 只观测 `mstatus` 的**低字节**（`MIE` = bit 3 / `MPIE` = bit 7 **都在低字节里**）⇒ ⇒ 那一个通道**够用**。

### §1.2 事实表（F1 ~ F13）

| 编号 | 事实 | 来源 |
| --- | --- | --- |
| **F1** | 参考侧在 PMP 拒绝时 `mtval` = **出错那一片的起始地址**（未对齐 4B 读 @ `0x8003_FFFE`，尾部两字节落进 DENY 区间 ⇒ ⇒ 参考侧 `mtval` = **`0x8004_0000`**） | v0.39 批实测 |
| **F2** | 同一场景两侧的 `mcause` **相同**（都是 **5**）⇒ ⇒ **只看 `mcause` 的判据分不出 D-154 这一条** | v0.39 批实测 |
| **F3** | 平台当前报 `mtval` = **访问的有效地址**（`0x8003_FFFE`）⇒ ⇒ 差**一个偏移量** | 本仓源码（`executor.rs` 的 `enter_trap(state, cause.mcause(), addr)`） |
| **F4** | 取指窗口是 **4 字节**（指令最大长度）⇒ ⇒ **永远对齐** ⇒ ⇒ **永不跨界** ⇒ ⇒ 取指的 `mtval` 两侧**必然一致** | 本仓源码 + 架构对齐规则 |
| **F5** | 参考侧的 `time` 在**黄金配置**下**恒读 0** ⇒ ⇒ 与平台当前的恒 0 **逐位一致** ⇒ ⇒ D-151 **不是**对拍缺口 | v0.36 批实测 |
| **F6** | 黄金语料里**没有一条**查 `time` / `mcycle`（`rv32mi/zicntr` **不入库**）⇒ ⇒ D-151 **不会**让既有语料翻红 | `tests/golden/corpus.txt` |
| **F7** | `time`（U）与 `stime`（S）是**同一个 CSR 地址** `0xC01`；`timeh`（U）与 `stimeh`（S）是**同一个** `0xC81` ⇒ ⇒ **一次接线**覆盖两个特权级 | 架构 CSR 地址映射 |
| **F8** | `time` / `timeh` 走**计数器使能链**（`mcounteren.TM` / `scounteren.TM` / `ucounteren.TM`）⇒ ⇒ S / U 模式要过链，缺一 ⇒ ⇒ **cause 2** | 本仓源码（`m13_iss/csr.rs` 的 `counteren_bit` / `counter_access_allowed`） |
| **F9** | 平台的 `csr_read` 对 `time` / `timeh` 有**显式恒 0 特判**（**不是**「文件里没有 ⇒ 读回 0」那条巧合）⇒ ⇒ 它挡的是「**畸形载荷把 `time` 塞进 CSR 文件 ⇒ 读回非 0**」 | 本仓源码（`m13_iss/csr.rs` 第 136 行附近） |
| **F10** | 黄金 runner（`crates/kernel/src/bin/riscv-golden.rs`）**没有** CLINT ⇒ ⇒ D-151 落地后，**黄金配置**下平台侧 `time` **仍然恒 0** ⇒ ⇒ 与参考侧**仍然逐位一致** | 本仓源码（`grep` 无 CLINT） |
| **F11** | `mret` 之后 `mstatus` 的 `MIE` / `MPIE`：参考侧与平台**逐位一致**（`MIE`=1/`MPIE`=0 ⇒ 两侧 `0x80`；`MIE`=0/`MPIE`=1 ⇒ 两侧 `0x88`；`MIE`=0/`MPIE`=0 ⇒ 两侧 `0x80`）⇒ ⇒ **平台没有 bug** | v0.40 批实测（三个变体） |
| **F12** | **riscv-privileged v1.13 §3.1.6.1 / Table 52** 原文：「MRET then in mstatus/mstatush sets MPV=0, MPP=0, **MIE=MPIE, and MPIE=1**」⇒ ⇒ **规范方向 = 平台方向** ⇒ ⇒ v0.38 规格里那句「与特权规范一致」**成立** | 规范原文（用户提供的 PDF） |
| **F13** | 参考侧可用 `spike -l --log-commits` 给出**逐条提交日志** ⇒ ⇒ 两侧的**执行指令数**与**陷阱序列**都是**可观测**的 ⇒ ⇒ D-96 的探测器有**数据源** | 工具链既有能力 |

⚠️ **F12 的版本陷阱**：本机另有一份 `riscv-privileged-spec-v1.7中文版.pdf`（在 `~/riscv-sim-golden` 附近）⇒ ⇒ 那是 **2015 年的早期草案**（那一版指令还叫 `ERET` / `MRTS`，**没有** MRET 的使能位条文）⇒ ⇒ **不能**用它判方向 ⇒ ⇒ 「本地有 PDF」≠「它是权威版」。

### §1.3 本批**推翻**的旧结论

| 旧结论 | 现在 | 依据 |
| --- | --- | --- |
| 「`mtval` 的差异只是**一个偏移量** ⇒ 算一下就行」 | ⛔ **不许**用「有效地址 + 偏移」重算 ⇒ ⇒ **必须**让判定本身把失败片带出来（两个真相源） | **裁决 C** |
| 「`time` 可以按 `SimState` 的虚拟时间折算」 | ⛔ **虚拟时间 ≠ 周期数** ⇒ ⇒ 与参考侧**语义不同** ⇒ ⇒ 对拍会**假红** ⇒ ⇒ 台账 D-151 做法 A **不做** | 台账 D-151 行 |
| 「`time` 的值放进 `SimState` 最省事」 | ⛔ ⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **升版** ⇒ ⇒ 前端镜像**必须**同步 ⇒ ⇒ 制造 D-143 那类存量问题 ⇒ ⇒ **做法 B** | 台账 D-151 行 / **裁决 F** |
| 「`stime` / `stimeh` 要**另做**一次」 | ⛔ `stime` 与 `time` **同一个地址**（F7）⇒ ⇒ **一次接线**覆盖两个特权级 | **裁决 I** |
| 「v0.38 说『与特权规范一致』⇒ ⇒ 平台可能偏离规范」 | ⛔ **不偏离** ⇒ ⇒ 平台与参考侧、平台与规范**三者一致** ⇒ ⇒ D-155 **销账**（**不是**改判成偏差） | F11 + F12 |
| 「空洞的一致**靠人读规格**发现就行」 | ⛔ ⇒ ⇒ 要做成**机器可检**的探测器 ⇒ ⇒ 但探测器**查不了**「每条断言是否忠实于参考侧语义」⇒ ⇒ **边界必须写在输出里** | **裁决 L** |

---

## §2 契约层设计

### §2.1 版本：**三个都不升**（裁决 A）

| 常量 | 值 | 本批 |
| --- | --- | --- |
| `SCHEMA_VERSION` | **7** | **不动** |
| `SIM_STATE_SCHEMA_VERSION` | **2** | **不动** |
| `PLATFORM_SCHEMA_VERSION` | **1** | **不动** |

⚠️ **为什么加一条 trait / 一个函数不算升版**：那两条都是**仓内 Rust 符号**，**不是** DTO、**不是**线格式 ⇒ ⇒ 前端镜像**不用**改版本号 ⇒ ⇒ **不制造 D-143 那类存量问题**（v0.40 批 `pmp_access_allowed` 改第五个参数时**同理**）。

### §2.2 D-154：加**一个新函数**，既有签名**一字不改**（裁决 B）

```rust
// crates/contract/src/csr.rs —— 新增
/// The **first failing piece** of an access, or `None` when the access is
/// permitted.
///
/// ⚠️ **既有 `pmp_access_allowed` 的签名一个字都不改**（v0.40 裁决 F 的延续）
/// ⇒ ⇒ 既有调用点**零改动** ⇒ ⇒ 契约层**零升版**。
pub fn pmp_fault_piece_addr(
    entries: &[(PmpCfg, u32)],
    addr: u32,
    size: u32,
    access: PmpAccess,
    privilege: u32,
) -> Option<u32>
```

**判据**：返回 **`Some(失败片的起始地址)`**（不是末尾、不是有效地址）；放行时返回 **`None`**。

⚠️ **一个约束一个落点**：既有 `pmp_access_allowed` **由新函数实现**（`pmp_fault_piece_addr(...).is_none()`）⇒ ⇒ 两条路径**走的是同一条链** ⇒ ⇒ 零漂移；⛔ **不许**把切片逻辑再写一份。

### §2.3 D-151：契约层 `port.rs` 加**一条只读端口签名**（裁决 F / G）

```rust
// crates/contract/src/port.rs —— 新增
/// Driven port: the platform's **wall clock** (M-13 ← M-15 CLINT).
///
/// ⚠️ **只读**、**无 DTO**、**不升版**（台账 D-151 做法 B）。
/// ⚠️ **单位 = 平台时钟 tick**（不是 ps、不是指令数）—— 与 CLINT 的
/// `mtime_cycles()` **同源** ⇒ ⇒ 两侧是**同一份**读数，不是各算一份。
pub trait TimeSource {
    /// The platform's clock reading, in the platform's clock ticks.
    fn time_ticks(&self) -> u64;
}
```

⚠️ **不许**把它做成「契约层的一个 DTO」⇒ ⇒ 那会**升版** ⇒ ⇒ 前端镜像**必须**同步 ⇒ ⇒ **不做**。
⚠️ **不许**让它带**写**语义 ⇒ ⇒ `time` 是**只读** CSR ⇒ ⇒ 端口也**只读**。

### §2.4 `csr_read` 的签名**不改**（裁决 K）

* **既有** `csr_read(state, csr)`：**签名一个字都不改**（它是 M-17 调试读 CSR 的**唯一入口** ⇒ ⇒ 改它会波及 10 处）。
* **新增** `csr_read_with_time(state, csr, time_ticks: u64) -> u32`：**「`time` 报什么」这条规则只许有这一个落点**。
* `csr_read(state, csr)` = `csr_read_with_time(state, csr, 0)`（**薄封装**，不是第二个真相源）。
* ⚠️ **护栏不许删**（**裁决 H**）：`time` / `timeh` 的值**永远不从 CSR 文件读** ⇒ ⇒ 改后规则 = 「**有真值源就报真值，没有就报 0**」⇒ ⇒ 「畸形载荷把 `time` 塞进文件 ⇒ 读回非 0」这条**仍然被挡**。

---

## §3 内核设计

### §3.0 落点（**一个约束一个落点**）

| 规则 | 唯一落点 | 调用方 |
| --- | --- | --- |
| PMP 的**逐片判定** | 契约层 `pmp_access_allowed` / `pmp_fault_piece_addr` | 内核只供给状态数据 |
| PMP 的**有效特权级 / 豁免** | 契约层 `pmp_effective_privilege` / `pmp_exempt` | 内核 `m13_iss/pmp.rs` |
| 内核的 PMP **判定入口** | `m13_iss/pmp.rs` 的 `pmp_fault_addr`（`pmp_allows` 由它实现） | `executor.rs` 的 load / store / fetch 三条路径 |
| `time` 的**读值规则** | `m13_iss/csr.rs` 的 `csr_read_with_time` | 执行器六条 CSR 指令 + M-17 调试读 |
| 平台**时钟**的真值 | M-15 的 `Clint::mtime_cycles()` | 经 `TimeSource` 端口 |

### §3.1 Lane A：内核接上「失败片」

* `m13_iss/pmp.rs` 新增 `pub(crate) fn pmp_fault_addr(state, addr, size, access) -> Option<u32>`；**`pmp_allows` 由它实现**（`.is_none()`）⇒ ⇒ 判定**仍然只有一个入口**。
* ⚠️ **热路径不许被改慢**：M 模式豁免那一支**先返回 `None`**（与既有 `pmp_allows` 的早退**同形**）。
* `executor.rs` 的 **load / store** 两条路径：把 `enter_trap(state, cause.mcause(), addr)` 的 `addr` 换成**失败片地址**。
* ⚠️ **取指路径一个字都不动**（**裁决 D**）。
* ⚠️ **未映射访存那一支的 `mtval` 一个字都不动**（**裁决 E**）。

### §3.2 Lane B：把时钟接进 ISS

* `m15_peripheral/clint.rs`：`impl TimeSource for Clint` ⇒ `time_ticks()` = `mtime_cycles()`（**周期**）。
* `m13_iss/mod.rs`：`Iss` 持有 `Option<Rc<dyn TimeSource>>`；`Iss::step_one`（闭包那条路）与 `Iss::step_one_via_bus`（总线那条路）**传同一个源** ⇒ ⇒ 两条派发路径**同口径**（D-11 的延续）。
  * ⚠️ `Iss` 的 `Debug`：**手写**（`dyn TimeSource` 没有 `Debug` 约束 ⇒ ⇒ 与 `Peripheral` 的先例**同形** ⇒ ⇒ 契约层**不加** `Debug` 超约束）。
* `m13_iss/executor.rs`：`execute_one` **末尾**加一个参数 `time: Option<&dyn TimeSource>`；六条 CSR 指令的**读**那一支改调 `csr_read_with_time(state, number, time_ticks)`。
  * ⚠️ **`executor.rs` 的 import 块不许动** ⇒ ⇒ 用**全路径** `riscv_sim_contract::port::TimeSource` ⇒ ⇒ 与 Lane A 的 hunk **零重叠**。
* `m17_backend/mod.rs`：后端**转发**时间源；调试读 CSR 走 `csr_read_with_time` ⇒ ⇒ **调试视图与 guest 视图同源**。
* `kernel_host/`：把 CLINT 的 `Rc<RefCell<Clint>>` **适配**成 `Rc<dyn TimeSource>`（适配器用 **`try_borrow`** ⇒ ⇒ 争用时**不 panic**、降级为 0 ⇒ ⇒ 与 `BoundPeripheral::instance_id` **同形**）⇒ ⇒ 装进 `Iss`。

### §3.3 Lane C：探测器的落点

* **新工具** `tools/vacuity_check.py`（**统筹方**不碰它 ⇒ ⇒ 归 Lane C）。
* **平台侧的观测源**：`crates/kernel/src/bin/riscv-golden.rs` 增加「**提交指令数**」的统计输出 ⇒ ⇒ 与 Lane A / Lane B **零文件冲突**。
* ⚠️ **不许**改 `tools/stage26_check.py` / `tools/doc_check.py` / 其它既有工具 ⇒ ⇒ **文件所有权按文件名切**。

---

## §4 工具层与语料设计

### §4.1 Lane A 的语料改动：`local/pmp_straddle.S` 补 `mtval` 断言

* 在 `trap_in_scenario_a` 里加 `csrr t3, mtval`，与**期望值 `0x8004_0000`** 比较；不等 ⇒ ⇒ `TESTNUM = 10` ⇒ ⇒ `RVTEST_FAIL`。
* ⚠️ **必须在 A + B 之后做**（⇒ ⇒ 否则**立刻红** ⇒ ⇒ 那是**预期的**「改前红」，是判别力证据）。
* ⚠️ **必要对照**（判别力清单 ②）：断言的是**失败片的起始**（`0x8004_0000`），**不是**有效地址（`0x8003_FFFE`）⇒ ⇒ 「断言写反了」与「实现写反了」**分得开**。
* ⚠️ **`mcause` 断言不许删**（它是「判决对不对」的主判据；`mtval` 只是「报得准不准」）。
* ⚠️ **控制组不许拆**：完全落在允许区间内的未对齐读**不陷** ⇒ ⇒ `mtval` 无从谈起 ⇒ ⇒ **不能**用它当对照。
* ⚠️ **失败编号表要补一行**：`TESTNUM = 10`（`mtval` 不是失败片的起始）。

### §4.2 Lane B 的判别力：**全部落在 Rust 单测**（裁决 J）

| 编号 | 判别式 | 为什么不能落黄金语料 |
| --- | --- | --- |
| ① | `csrr time` 的值**随虚拟时间推进而变**（当前恒 0 ⇒ 必红） | 参考侧在黄金配置下恒 0（F5）⇒ ⇒ 两侧**不可比** |
| ② | 写 `time` **不生效**（只读）⇒ ⇒ 写后读回不变 | 同上 |
| ③ | 与 CLINT 的 `mtime` 读数**逐字相同**（两侧**同源**，不是各算一份） | 同上 |
| ④ | `timeh` 与 `time` 的**高位配对一致** | 同上 |
| ⑤ | S / U 模式的**使能链**（F8）**不受影响** | 同上 |

⚠️ **本批的明示取舍**：D-151 落地后，「`time` 恒读 0 与参考侧逐位一致」这个**前提消失**（在**有 CLINT** 的宿主上）⇒ ⇒ 属「**与参考侧不再可比**」的改动 ⇒ ⇒ **如实登记**，**不假装**还对得上。
⚠️ **黄金配置下**仍然一致（F10）⇒ ⇒ 既有语料**零翻红**（F6）。

### §4.3 Lane C 的探测器：判别力**边界**必须写在输出里（裁决 L / M）

* **能**查：**集合关系** —— 语料声明的 ISA ⊆ 参考侧实现的 ISA；两侧的**提交指令数**与**陷阱序列**是否**同量级**（F13）⇒ ⇒ 「**差一个数量级**」⇒ ⇒ 报「**空洞一致**」。
* **不能**查：「每条断言是否**忠实于**参考侧语义」⇒ ⇒ ⛔ **不许**让「探测器绿」被当成「语料有判别力」⇒ ⇒ **工具的输出必须带这条边界声明**。
* ⚠️ **阈值必须写死且可解释**（例如「参考侧提交数 ≥ N 而平台侧 = 0」⇒ ⇒ 「差一个数量级」）⇒ ⇒ ⛔ **不许**用「差异大就报」这种不可复现的口径。
* ⚠️ **必须自带一个坏夹具**（故意造一条两侧提交数差一个数量级的假语料）⇒ ⇒ 探测器**必须**变红 ⇒ ⇒ 证明它**不是恒绿**。

---

## §5 验收

### §5.1 A 组 · 契约层

* **A-1** `pmp_fault_piece_addr` 存在，签名 = `(entries, addr, size, access, privilege) -> Option<u32>`；`pmp_access_allowed` 的**签名一字未改**。
* **A-2** `pmp_access_allowed(...)` == `pmp_fault_piece_addr(...).is_none()`（**同一条链** ⇒ ⇒ 零漂移）。
* **A-3** `pmp_fault_piece_addr` 在「失败片的起始」上返回 `Some`（未对齐 4B 读 @ `0x8003_FFFE` ⇒ ⇒ `Some(0x8004_0000)`），在放行时返回 `None`。
* **A-4** `port.rs` 有 `TimeSource` 端口签名，方法 = `fn time_ticks(&self) -> u64`；**无 DTO**、**无版本号**。
* **A-5** 三个版本常量**逐字未变**（`7` / `2` / `1`）。

### §5.2 B 组 · 内核

* **B-1** `m13_iss/pmp.rs` 有 `pmp_fault_addr`；`pmp_allows` 由它实现；判定**仍然只有一个入口**。
* **B-2** `executor.rs` 的 load / store 两条路径把**失败片地址**交给 `enter_trap` 的 `mtval`。
* **B-3** **取指路径的 `mtval` 一个字未改**；**未映射访存**那一支的 `mtval` **一个字未改**。
* **B-4** `Clint` 实现 `TimeSource`；`time_ticks()` == `mtime_cycles()`（**周期**）。
* **B-5** `csr_read` 的**签名未改**；`csr_read_with_time` 存在；**「`time` 报什么」只有一个落点**。
* **B-6** `time` / `timeh` 的值**永远不从 CSR 文件读**（畸形载荷那条护栏**仍在**）。
* **B-7** `Iss` 的**两条**派发路径**传同一个时间源**；`Debug` **手写**（契约层**不加** `Debug` 超约束）。
* **B-8** 宿主把 CLINT 适配成 `Rc<dyn TimeSource>` 装进 `Iss`；适配器用 **`try_borrow`**（**不 panic**）。
* **B-9** M-17 的**调试读 CSR** 与 guest 读 `time` **同源**。
* **B-10** §4.2 的**五条**判别式各有**至少一条** Rust 单测钉住。

### §5.3 C 组 · 工具层

* **C-1** `tools/vacuity_check.py` 存在，**可执行**、**有 `--help`**、**退出码**有意义。
* **C-2** 它的输出**带边界声明**（「不能查每条断言是否忠实于参考侧语义」）。
* **C-3** 它的**阈值写死且可解释**（⛔ 不得是「差异大就报」）。
* **C-4** 它**自带坏夹具**，且坏夹具**必须让它变红**。
* **C-5** `local/pmp_straddle.S` 补了 `mtval` 断言（`0x8004_0000`），且 `mcause` 断言**仍在**。
* **C-6** `tests/golden/README.md` 的 `pmp_straddle` 那一节**同步**了新断言（只许动**那一节**）。

### §5.4 T 组 · 台账与文档

* **T-1** **D-155 改判为已闭环**（依据 = F11 + F12）⇒ ⇒ **平台一个字未改**。
* **T-2** `STAGE-22_V038_Smode.md` 的事实表**补一条**（编号沿用 **`F42`**）⇒ ⇒ 只**补实测**，**不改** v0.38 的结论。
* **T-3** **D-154 / D-151 / D-96 三行**改判为已闭环，正文**按实测改写**并**保留原表述留痕**。
* **T-4** 台账 §0.1 的三个数、**真实未闭环**、§1.3 标记区**按口径重算**（一个数只服务一个口径）。
* **T-5** `README.md` / `AGENTS.md` / `docs/progress/PROGRESS.md` **同步**（规模数字**从当前树重算**）。

### §5.5 X 组 · 跨层与判别力

* **X-1** **三条 lane 的文件所有权零重叠**（唯一例外：`m13_iss/executor.rs`，两处 hunk 相距 ≥ 200 行）。
* **X-2** **D-154 的必要对照**：断言「有效地址」的那条**必须同时变红** ⇒ ⇒ 「断言写反」与「实现写反」**分得开**。
* **X-3** **D-151 的判别力全部有单测钉住**（⛔ 不靠黄金语料 ⇒ ⇒ 因为两侧不可比）。
* **X-4** **D-96 的探测器有坏夹具** ⇒ ⇒ 证明它**不是恒绿**。
* **X-5** **五条真变异的红集互不相同**；每条 lane **至少一条**「语义不变的对照变异」**0 红**。
* **X-6** **三个版本常量**在整批之后**仍然**是 `7` / `2` / `1` ⇒ ⇒ 前端镜像**零改动**。

---

## §6 门禁

| # | 门禁 | 命令 |
| --- | --- | --- |
| 1 | 格式 | `cargo fmt --all -- --check` |
| 2 | 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 3 | Rust 测试 | `cargo test --workspace --no-fail-fast --lib --tests` |
| 4 | 前端 | `tools/check-front.sh` |
| 5 | 黄金自检 | `tools/golden-run.sh --self-check` |
| 6 | 黄金对拍 | `tools/golden-run.sh --check-mem --keep-going` |
| 7 | WASM | `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` |
| 8 | 真浏览器 | `node tools/browser-verify.mjs` |
| 9 | 真实 RTOS | `tools/boot-osz.sh` |
| 10 | 规格自检 | `python3 tools/stage26_check.py …` … `python3 tools/stage18_check.py …` |
| 11 | 台账 | `python3 tools/ledger_check.py --ledger` + `--self-test` |
| 12 | 文档 | `python3 tools/doc_check.py --baseline` |
| 13 | 前端归属 | `python3 tools/front_test_attrib.py` + `--selftest` |
| 14 | 规模 | `python3 tools/count_scale.py` |

⚠️ **macOS 没有 `timeout`** ⇒ ⇒ 挂死保护一律走 `tools/watchdog.pl <秒> <cmd>`（退出码 **124 = HANG**，**不算红也不算绿**，与「对照必须 0 红」**分开记**）。
⚠️ **并发跑 cargo 会造成假 NOVERDICT** ⇒ ⇒ **一次只跑一个 cargo**。
⚠️ **本机 8 核 / 8 GiB** ⇒ ⇒ 三条 lane **并行**时**不要**同时跑全量 `cargo test` ⇒ ⇒ **门禁复验由统筹方在合并后串行跑**。

---

## §7 裁决

### 裁决 A · 三个版本常量**都不动**

加一条 trait / 一个函数**不是**改 DTO、**不是**改线格式 ⇒ ⇒ `SCHEMA_VERSION` 仍 **7**、`SIM_STATE_SCHEMA_VERSION` 仍 **2**、`PLATFORM_SCHEMA_VERSION` 仍 **1** ⇒ ⇒ 前端镜像**零改动** ⇒ ⇒ 不制造 D-143 那类存量问题。

### 裁决 B · D-154 走「**加一个新函数**」的形态

既有 `pmp_access_allowed` 的**签名一个字不改** ⇒ ⇒ 既有调用点**零改动** ⇒ ⇒ 契约层**零升版**；新函数额外回答「**哪一片不允许**」⇒ ⇒ 「一个约束一个落点」。

### 裁决 C · 失败片地址**由判定本身产出**

⛔ **不许**用「有效地址 + 偏移」重算失败片 ⇒ ⇒ 那等于**重新实现**一次切片 ⇒ ⇒ 两个真相源 ⇒ ⇒ **必须**让判定把失败片带出来。

### 裁决 D · 只改 **load / store** 两条路径；**取指路径一个字都不动**

取指窗口是 4 字节（F4）⇒ ⇒ **永不跨界** ⇒ ⇒ 取指的 `mtval` 两侧**必然一致** ⇒ ⇒ 改它是**多出来的风险**。

### 裁决 E · 未映射访存的 `mtval` 口径**不动**

那一条报的是「**出错的那次访问**」⇒ ⇒ 与「PMP 失败片」是**两个口径** ⇒ ⇒ ⛔ **不许**顺手揉成一个。

### 裁决 F · D-151 走**做法 B**（只读端口）

台账 D-151 的**做法 A**（虚拟时间折算）**语义不同** ⇒ ⇒ 对拍会**假红**；**做法 C**（塞进 `SimState`）**升版 + 破坏性** ⇒ ⇒ 只剩**做法 B**：契约层 `port.rs` 加**一条只读端口签名** ⇒ ⇒ 零 DTO、零升版。

### 裁决 G · `time` 的真值源 = **M-15 的 CLINT**，单位 = **平台时钟 tick**

⛔ 不是 ps、不是指令数 ⇒ ⇒ 与 `Clint::mtime_cycles()` **同源** ⇒ ⇒ 两侧是**同一份**读数，不是各算一份。

### 裁决 H · `time` 的「**显式恒 0**」护栏**不许删**

改后规则 = 「**有真值源就报真值，没有就报 0**」⇒ ⇒ **永远不从 CSR 文件读** ⇒ ⇒ 「畸形载荷把 `time` 塞进文件 ⇒ 读回非 0」那条**仍然被挡**（与 `misa` / `mhartid` 是同一道防线）。

### 裁决 I · `time` / `timeh` / `stime` / `stimeh` 是**同一个地址** ⇒ ⇒ **一次接线**

F7 ⇒ ⇒ `0xC01` / `0xC81` 覆盖 U 与 S 两个特权级 ⇒ ⇒ ⛔ **不许**做两次（台账 D-151 落地陷阱 ③ 明写「一次把两个特权级的 `time` 规划掉」）。

### 裁决 J · D-151 是「**与参考侧不再可比**」的改动 ⇒ ⇒ **明示取舍**

`time` 的值由**平台**定义 ⇒ ⇒ 参考侧在黄金配置下恒 0（F5）⇒ ⇒ 本批之后**有 CLINT 的宿主**上两侧**不再可比** ⇒ ⇒ **如实登记**；⚠️ 判别力**全部**落在 Rust 单测 ⇒ ⇒ ⛔ **不许**新增一条查 `time` 的黄金语料（那会**假红**）。

### 裁决 K · `csr_read` 的**签名不改** ⇒ ⇒ 加 `csr_read_with_time`

`csr_read` 是 M-17 调试读 CSR 的**唯一入口** ⇒ ⇒ 改它会波及 10 处 ⇒ ⇒ 加一个新函数，`csr_read` = 它的 `time = 0` 薄封装 ⇒ ⇒ 「`time` 报什么」这条规则**仍然只有一个落点**。

### 裁决 L · D-96 的判别力**边界必须写在工具输出里**

探测器**能**查**集合关系**（提交数 / 陷阱序列的同量级）；**不能**查「每条断言是否忠实于参考侧语义」⇒ ⇒ ⛔ **不许**让「探测器绿」被当成「语料有判别力」⇒ ⇒ 输出里**必须**带这条声明。

### 裁决 M · 探测器的**阈值写死且可解释**

⛔ 不许用「差异大就报」⇒ ⇒ 判据必须能**逐字复述**（例如「参考侧提交数 ≥ N 而平台侧 = 0」）⇒ ⇒ 且**必须**自带坏夹具证明它**不是恒绿**。

### 裁决 N · D-155 在本批**销账**，**平台一个字都不改**

F11（两侧逐位一致）+ F12（v1.13 Table 52 的规范方向 = 平台方向）⇒ ⇒ v0.38 那句「与特权规范一致」**成立** ⇒ ⇒ **不是**偏差、**不是** bug ⇒ ⇒ 只**补一条实测事实**（`F42`）⇒ ⇒ 台账改判。

---

## §8 施工期订正

> ⚠️ **本节在施工期逐条追加**（每条写清「**已改成什么**」）。⚠️ **没有**订正也要写「**无**」⇒ ⇒ 不得留空。

*（施工期追加）*

---

## §9 变异自检

### §9.1 真变异（期望**红**）

> ⚠️ 每条变异的**锚点必须逐字匹配且出现恰好 1 次**⇒ ⇒ 否则**放弃**（不猜）。
> ⚠️ **还原纪律**：`cp` 备份（键 = 相对路径把 `/` 换成 `__`）⇒ ⇒ 还原用 `cp` ⇒ ⇒ `md5` + `cmp` 逐字节校验 ⇒ ⇒ **`touch <file>`（当前时间，不是 `touch -r`）** ⇒ ⇒ 重编 ⇒ ⇒ 复跑基准 ⇒ ⇒ **禁** `git checkout -- <file>`。

| 编号 | 落点 | 变异 | 期望 |
| --- | --- | --- | --- |
| （施工期填） | | | |

### §9.2 对照变异（语义不变 ⇒ **必须 0 红**）

| 编号 | 落点 | 等价改写 | 期望 |
| --- | --- | --- | --- |
| （施工期填） | | | |

### §9.3 判别力**边界**的自查

* ⚠️ 「**自检脚本绿了**」**不算**判别力证据 ⇒ ⇒ 每条真变异都要有**独立**的观测（红集**内容**、不只是「有红」）。
* ⚠️ 「**真台账跑绿不代表口径对**」⇒ ⇒ 切分口径错成朴素写法时真台账仍 0 红 ⇒ ⇒ 只有**专门夹具**能暴露。
* ⚠️ 「**某个规则没被语料钉住**」**≠**「门禁不会红」⇒ ⇒ 登记覆盖缺口前**先查现有测试树**。

---

## §10 已知边界

1. **D-151 之后，「`time` 恒读 0」在有 CLINT 的宿主上不再成立** ⇒ ⇒ 与参考侧**不再可比**（**裁决 J**）⇒ ⇒ 这是**明示取舍**，不是缺陷。
2. **黄金配置下（无 CLINT）两侧仍然一致**（F10）⇒ ⇒ 黄金对拍**零翻红**；⚠️ 但这也意味着**黄金对拍抓不到** `time` 的任何回归 ⇒ ⇒ 判别力**只能**来自 Rust 单测。
3. **`time` 的单位是「平台时钟 tick」** ⇒ ⇒ 与参考侧的 `time` **不是同一个量纲** ⇒ ⇒ ⛔ 永远不要拿两边的 `time` **直接比大小**。
4. **D-154 只覆盖 load / store** ⇒ ⇒ 取指的 `mtval` **不在本批范围**（它**也不需要**改，F4）。
5. **未映射访存的 `mtval`**（**裁决 E**）与 PMP 失败片是**两个口径** ⇒ ⇒ 本批**没有**统一它们 ⇒ ⇒ 也没有**统一它们**的义务。
6. **D-96 的探测器查不了「每条断言是否忠实于参考侧语义」**（**裁决 L**）⇒ ⇒ 它的绿**不等于**语料有判别力。
7. **`spike -l --log-commits` 的输出是参考侧的**逐条**提交日志** ⇒ ⇒ 探测器的阈值必须按「**提交数**」而不是「**日志行数**」定 ⇒ ⇒ 两者在有压缩指令时**不等**。
8. **本批不新增器件类型** ⇒ ⇒ I2C 那批仍在 **STAGE-24**。

---

## §11 复现命令

```bash
# ---- 仓内门禁 ----
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast --lib --tests
tools/check-front.sh
tools/golden-run.sh --self-check
tools/golden-run.sh --check-mem --keep-going
tools/build-wasm.sh && node tools/wasm-smoke.mjs
node tools/browser-verify.mjs
tools/boot-osz.sh
python3 tools/stage26_check.py docs/detailed-design/STAGE-26_V041_Mtval_Time_Vacuity.md
python3 tools/stage25_check.py docs/detailed-design/STAGE-25_V040_PMP_Privilege.md
python3 tools/ledger_check.py --ledger docs/reviews/REVIEWS.md
python3 tools/ledger_check.py --self-test
python3 tools/doc_check.py --baseline tools/doc_check_baseline.txt $(git ls-files '*.md')
python3 tools/front_test_attrib.py
python3 tools/count_scale.py

# ---- 判别式单跑（参考侧绿 / 平台侧改前红）----
perl tools/watchdog.pl 600 tools/golden-run.sh --filter local/pmp_straddle --keep-going

# ---- 本批的新工具 ----
python3 tools/vacuity_check.py --help
```
