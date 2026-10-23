# STAGE-13 · v0.28 批冻结规格 · 活锁处置链路 + M-21 日志 + M-22 HMS

状态：**冻结**（施工期可被实测订正，订正须合回 main）
版本：v0.28 · 2026-10-23
上游规格：STAGE-12（v0.27 · 平台描述）· STAGE-9（v0.24 遗留）· STAGE-4（M-16）· STAGE-5（M-11）
台账：`docs/reviews/REVIEWS.md`（D-114 / D-115 闭环 + 新登记）

---

## §0 本批定位

### 0.1 一句话

**把「活锁变成失败」推进到「失败有处置、有记录、对管理员可见」** —— 并在同一条链路上把
**M-21 日志系统**与**M-22 健康管理 HMS**这两个横切 stub 从「有骨架」推到「按 LLD 可用」。

### 0.2 为什么这四件事必须绑在一批

台账 **D-115** 的原文要求是：

> **待做**：宿主侧加时间预算 + 三次确认 ⇒ 再报 HMS 并记 `EV_WATCHDOG`。

这句话本身就把三样东西串起来了：**宿主看门狗（时间预算 + 三次确认）** → **M-22（报
HMS_FATAL + 幂等标志）** → **M-21（记 `EV_WATCHDOG`）**。而 **D-114**（M-17 `step_n` 的
同形状活锁）是**同一个不变式（M-16 的 `active_irq` 单级防护）的第二个落点** —— VD-F3 的
修复只堵了 M-12 这一个。

⇒ **「无进展守卫」这条不变式在本批有两个落点被堵上（M-12 已有 / M-17 本批补上），
且守卫触发后的处置链路第一次真正存在。**

### 0.3 本批做（八件）

| 编号 | 事项 | 落点 |
| --- | --- | --- |
| ① | **M-21 全量落地**：结构化条目（8 字段）+ 五级分带环 + 截断 + 级别过滤 + query/export + 溢出计数 | `crates/kernel/src/m21_logging/` |
| ② | **M-22 全量落地**：`HealthSignal` / `Severity` / `classify` / 信号流（环形 + 致命严重不丢 + 崩溃冻结）/ 分角色分发 / 降级策略 / 降级重试 / 指标环 / 诊断包 | `crates/kernel/src/m22_hms/` |
| ③ | **D-115**：宿主看门狗 —— 时间预算 + 滑动窗口三次确认 + HMS 同步终止 + `WATCHDOG` 日志 + 幂等 | `crates/kernel/src/kernel_host/` |
| ④ | **D-114**：M-17 `step_n` 的同形状无进展守卫 | `crates/kernel/src/m17_backend/` |
| ⑤ | M-22 诊断包 → **M-23 归档**（通用字节归档接口，M-23 不依赖 M-22） | `crates/kernel/src/m23_datastore/` |
| ⑥ | 契约层 **HMS 场景码**（`E_HMS_01` / `E_HMS_02`）落到契约层（遵守 D-120「场景码住契约层」） | `crates/contract/src/error.rs` |
| ⑦ | 契约层**零新增 DTO / 命令 / 事件**：日志与健康信号是**进程内**通道，前端不订阅（角色边界，见 §2.7）；⚠️ 契约层**已有**的 `Admin` 命令与 `AdminOp` 三个变体**本批不动**（§1.4） | — |
| ⑧ | 文档收口：台账闭环 D-114 / D-115 + 新登记 | `docs/reviews/REVIEWS.md` 等 |

### 0.4 本批明确不做（八件）

| 编号 | 不做的事 | 为什么 | 落账 |
| --- | --- | --- | --- |
| ① | **S 扩展 / PMP / 计数器**（台账 D-94） | 一旦做 S 模式就是半条 RISC-V 特权规范，工作量与本批差一个量级；混批会把「处置链路」这条主线稀释掉 | D-94 保持未做，另批 |
| ② | **M-18 环布局改造**（D-87 访存轨迹带值） | 要改 M-18 的环布局，影响 M-11 导出与前端波形；与本批无耦合 | D-87 保持未做 |
| ③ | **裁决 G 的粒度订正**（D-89 部分写失效） | 是**规格**问题不是实现问题；要改 STAGE-5 的冻结裁决 | D-89 保持未做 |
| ④ | **契约层日志 / 健康 DTO 与命令、事件** | 角色边界：产品 / 嵌入式工作台**不接触**系统日志（LLD M-21 §3.4 / 评审 L1）⇒ 前端不订阅 ⇒ 无需跨进程载体。⚠️ 契约层**已有**的 `Admin` 命令（`AdminOp::DrainLogs` / `KillGuest` / `PersistProject`）本批**不动** —— 缺的是 DTO，不是入口 | §2.7 裁决 |
| ⑤ | **前端任何改动** | 同上。前端零改动是本批的**可验证判别式** | §5 D-9 |
| ⑥ | **M-12 结构改动** | 本批的看门狗**全部落在宿主侧**（台账原话「宿主侧加时间预算 + 三次确认」）⇒ M-12 一行不改 | §4 裁决 F |
| ⑦ | **降级动作的真实执行**（快照恢复 / 暂停 / 断连 / 关 trace） | 分别要 M-20 / M-04 / M-07 / M-19 —— **全部在本批范围外** ⇒ 本批只落 `Terminate` | §4 裁决 H |
| ⑧ | **宿主时间预算的引擎内采样** | 同裁决 F：预算在**宿主侧**测量每批耗时 | §4 裁决 F |

---

## §1 前置事实（勘察结论，2026-10-23）

> 本节每一条都是**实测/读码**得出的，不是从 LLD 转述的。规格的每个裁决都建立在这些事实上。

### 1.1 M-21 现状

* `crates/kernel/src/m21_logging/mod.rs`：**94 行** stub（第三次订正的实测订正）。
* 现有公开类型：`Level { Trace, Info, Warn, Error, Fatal }`、`LogRecord { ts_ms, level,
  source, message }`、`LogBuffer`（单个 `VecDeque`，容量 `MAX_LOG_RECORDS = 4096`）、
  `LOG_FAMILY = ErrorFamily::Context`。
* **与 LLD 的差距**（逐条）：
  1. **缺 `Debug` 级**（LLD 五级：TRACE / DEBUG / INFO / WARN / ERROR）⇒ 现有 `Level` 有
     `Fatal` 而 LLD **没有** `Fatal`（致命属 M-22 的 `Severity`）。
  2. **缺字段**：LLD 的 `logEntry` 是 8 个字段（`time_virt` / `time_host` / `sessionId` /
     `level` / `ctx` / `errCode` / `msg` / `truncated`），现有是 4 个（`ts_ms` / `level` /
     `source` / `message`）⇒ **没有双时间戳、没有会话关联、没有错误码、没有截断标记**。
  3. **不分带**：LLD §3.3 要求「按级别分带」+ 带间独立容量预算；现有是**一个总环**。
  4. **无 `overflowCount`**：LLD §3.3 要求「丢最旧 + 溢出计数」。
  5. **无 `levelFilter`**：LLD §3.1 要求入口先按阈值过滤（O(1) 直接丢）。
  6. **无 `query` / `export` / `setLevel`**：LLD §3.4 管理员通道三件套全缺。
* **零外部引用**：`grep -rn "LogBuffer\|LogRecord" crates/` 除本文件外**零命中** ⇒
  公开 API 可自由重写，不破坏任何调用点。

### 1.2 M-22 现状

* `crates/kernel/src/m22_hms/mod.rs`：**74 行** stub。
* 现有公开类型：`Health { Ok, Degraded, Critical }`、`Hms { state, last_escalation }`、
  `Hms::report(&mut self, kind: &str) -> Result<Health, PlatformError>`、
  `HMS_FAMILY = ErrorFamily::Health`。
* **与 LLD 的差距**：
  1. **没有 `HealthSignal` 结构**（LLD §2.1.1：`src` / `errCode` / `tVirt` / `tHost` /
     `ctx` / `kind` 六字段）。
  2. **没有 `Severity`**（LLD：`FATAL` / `CRITICAL` / `WARNING` / `INFO_NO_DEGRADE` 四值）。
  3. **`report` 的语义与 LLD 完全不同**：现有实现是「每调一次就 `Ok → Degraded → Critical`
     盲升一级」；LLD §3.1 的 `report` 是「**先分级再入流** → 分角色分发 → 查降级策略 →
     降级」，且分级由 `classify` 决定。
  4. **没有 `classify`**：LLD §3.2 的三条规则（watchdog + GUEST_RUN ⇒ FATAL；TRAP ⇒
     INFO_NO_DEGRADE；severityTable 未命中 ⇒ INFO_NO_DEGRADE）全缺。
  5. **没有 `severityTable`**（分级映射，可配置）。
  6. **没有信号流**：无环形带、无「致命 / 严重不丢」的保留带、无崩溃冻结（LLD 22-12）。
  7. **没有分角色分发**（`dispatch`）：ADMIN 收诊断详情 + M-21 日志关联；产品角色收脱敏提示。
  8. **没有 `degradePolicy` / `degrade`**（含 ≤3 次重试 + 终态上报）。
  9. **没有 `collectMetric` / 指标环**（LLD：4096 条）。
  10. **没有 `exportDiagnosticKit`**。
* **零外部引用** ⇒ 公开 API 可自由重写。

### 1.3 M-23 现状

* `crates/kernel/src/m23_datastore/mod.rs` + `memory.rs`：**v0.18 会话快照批已落地**，
  `DataStore` trait 五方法（`put_project` / `get_project` / `put_snapshot` / `get_snapshot` /
  `list_projects`）+ `InMemoryDataStore` 参考实现。
* **缺口**：LLD M-22 §3.5 的 `exportDiagnosticKit` 要 `m23Archive(kit)` ⇒ M-23 **没有归档
  接口** ⇒ 本批补一个**通用字节归档**（§3.4）。

### 1.4 契约层现状

* 错误族（`error_families!` 宏，15 个）：`Context` / `Iss` / `Engine` / `Bus` / `Peripheral` /
  `Interrupt` / `Debug` / `Observability` / `Snapshot` / `Loader` / **`Health`（tag `HMS`，
  M-22 专用，已存在）** / `Storage` / `Transport` / `Protocol` / `Platform`。
* ⇒ **HMS 族已在契约层** ⇒ 本批只需补**场景码常量**，不必新增族。
* ⇒ **M-21 不新增错误族**：LLD M-21 §3.3 明确「不抛错、不阻塞」⇒ 日志写入**没有失败态**
  ⇒ 没有码可加（**没有失败态就没有码**，不是遗漏）。
* **管理员通道的契约入口已存在**（实测，2026-10-23 第一次订正的依据）：
  `CommandPayload::Admin { op: AdminOp }`，`AdminOp` 的三个变体**正好对应本批的三个
  模块** —— `DrainLogs`（Dump the kernel log buffer (M-21)）、`KillGuest`
  （Force-terminate a stuck guest (M-22 watchdog escalation)）、`PersistProject`
  （M-23），外加 `Custom(String)` 预留位。
  **宿主目前对这个命令一律返回 `E_CTX-05`「管理通道未启用」** ⇒ 入口是**预留的空壳**，
  **没有**配套的 DTO 与回复事件。
  ⇒ 本批**不启用**它（理由见 §2.7）。

### 1.5 M-12 / M-17 的看门狗现状

* **M-12 已有无进展守卫**（VD-F3 / D-83 闭环）：`NO_PROGRESS_WINDOW = 1024`，
  连续 1024 次「既没有指令退休、虚拟时间也没有前进、也没有到期事件被消费」⇒ 返回
  `Err(E_ENG_03_NO_PROGRESS)`。口径**明确排除**「中断交付算进展」（VD-F3 实测：把活锁
  放过去）。**M-12 本批一行不改**。
* **M-17 `step_n` 无守卫**：`crates/kernel/src/m17_backend/mod.rs:927` 的
  `fn step_n(&mut self, n: u32)` 循环里，`Ok(ExecOutcome::InterruptTaken)` 分支
  （`:1015`）**不退休指令、不推进虚拟时间** ⇒ 同形状活锁（D-114）。
* **宿主侧没有任何看门狗**：`kernel_host` 里 `grep run_until` **零命中**；驱动路径是
  `step_in_chunks`（`:2049`，按 `DEFAULT_INSTRUCTION_BUDGET = 10 000` 分块）与
  `continue_until_event`（M-17 `:1089` ⇒ `engine.run_until(&mut executor, u64::MAX)`，
  一轮 = 一个指令预算窗口）。
* ⇒ **「检查窗口」的天然粒度 = 引擎的一个批次**（10 000 条指令）⇒ 与 LLD §3.5 的
  「每 ≤10⁴ 条检查」**字面吻合** ⇒ 宿主侧按批次判定即可，**M-12 零改动**。

### 1.6 LLD 内部的矛盾（实测出来的，必须裁决）

见 §2。

### 1.7 M-12 守卫触发后的**重入语义**（实测读码，第二次订正的依据）

* 守卫触发处（`m12_engine/mod.rs`）是 `return Err(PlatformError::new(Engine,
  E_ENG_03_NO_PROGRESS, ..))` ⇒ **只是返回，不改任何引擎状态**（不推进虚拟时间、
  不改寄存器、不动事件队列的游标）。
* ⇒ **重入是安全的**：宿主可以再次调 `run_until`。
* ⚠️ **但 `no_progress` 是 `run_until` 的局部变量** ⇒ **每次重入都从 0 重新计数**。
  ⇒ 宿主做「三次确认」时，实际发生的空转次数是 **3 × `NO_PROGRESS_WINDOW`**，
  **不是** `NO_PROGRESS_WINDOW`。⇒ 这一点必须写进代码注释（否则后来人会以为
  「1024 次就停」）。
* ⚠️ `run_until` 的内层循环 `while done < QUANTUM_INSTR_QUOTA`（1000）在
  `InterruptTaken` 分支**不**增 `done` ⇒ 计数能越过 1000 累积到 1024 ⇒ 守卫可达 ✔。
* ⚠️ `run_until(u64::MAX)` 的退出**只**靠指令预算（`instructions >= budget`）⇒
  一次 `continue_until_event` = **一个 10 000 条指令的窗口** ✔ 与 LLD §3.5 的
  「每 ≤10⁴ 条检查」字面吻合。

---

## §2 歧义处置（九个，逐条给「原文 / 实测 / 裁决」）

> 规矩：**规格由统筹方独占修改**；worker 只报「实测事实 + 原文 + 行号」，不自行改规格。
> 本节的每条裁决都是**先有事实、后有裁决**，不是反过来。

### 2.1 `HMS_FATAL` 不是任何一个符号（裁决 A）

* **原文**：LLD M-12 §3.5「触发后同步终止（评审 12-3）：直接调 `report(HMS_FATAL)`
  （M-22 报告接口，`hms_report` 为其内部封装名，文档统一用 `report`）同步终止内核」。
  台账 D-115 写「报 `HMS_FATAL` + 幂等标志」。
* **实测**：LLD M-22 §3.1 的 `report` 签名是 `Severity report(HealthSignal s)` —— 收的是
  **一条信号**，不是一个致命常量；M-22 §3.2 的 `classify` 里 `FATAL` 是 `Severity` 的
  **枚举成员**。
* **裁决**：`HMS_FATAL` 是 **LLD 的旧命名**，**作废**。本批统一为：
  `Hms::report(HealthSignal { kind: SignalKind::Watchdog, ctx: SignalCtx::GuestRun, .. })`
  ⇒ `classify` 判出 `Severity::Fatal`。
  **禁止**在代码里出现 `HMS_FATAL` 这个标识符（它是散文里的幽灵符号）。
  ⇒ 判别式见 §5 C-1。

### 2.2 `EV_WATCHDOG` **不是**契约事件（裁决 B）

* **原文**：LLD M-12 §3.5「`EV_WATCHDOG` 仅作日志 / 回放记录，**不再入队投递**」。
* **实测**：契约层 `EventPayload` 没有这个变体；LLD 自己就说了「不再入队投递」。
* **裁决**：`EV_WATCHDOG` 是**日志标记**，不是事件类型。
  **禁止**在契约层新增 `EventPayload::EvWatchdog` 变体（加了就变成「入队投递」，
  **违反 LLD 原文**）。
  落地形态：M-21 环里一条 `ctx = "ENG-WATCHDOG"` 的条目 + M-22 的一条
  `SignalKind::Watchdog` 信号。
  ⇒ 规格全篇一律写作 `WATCHDOG`（去掉 `EV_` 前缀，避免被误读成事件）。

### 2.3 M-21 的 `Level` **没有** `Fatal`（裁决 C）

* **原文**：LLD M-21 §2.2 五级 = TRACE / DEBUG / INFO / WARN / ERROR。
* **实测**：现有 stub 的 `Level` 是 `Trace / Info / Warn / Error / Fatal` ⇒ **缺 `Debug`、
  多 `Fatal`**。
* **裁决**：`Fatal` **删**（致命是 M-22 的 `Severity::Fatal`）；补 `Debug`。
  「一个约束一个落点」：级别体系只有一个真相 = M-21 的五级。

### 2.4 M-22 的 `Health` 与 `Severity` 是**两个东西**（裁决 D）

* **原文**：LLD M-22 只有 `Severity`（FATAL / CRITICAL / WARNING / INFO_NO_DEGRADE），
  **没有**「整体健康状态」这个概念。
* **实测**：现有 stub 有 `Health { Ok, Degraded, Critical }`，且 `report` 是「盲升一级」。
* **裁决**：
  * **`Severity` 保留并新建**（LLD 原文），表示**单条信号**的级别。
  * **`Health` 保留但改语义**：它表示**整体健康状态**，由信号流**派生**：
    有 FATAL ⇒ `Critical`；有 CRITICAL / WARNING 而无 FATAL ⇒ `Degraded`；否则 `Ok`。
  * **`report` 不再盲升**：`Health` 由 `Severity` 派生，不再是 `report` 的副作用计数器。
  * **单调性**：`Health` **只降不升**（恢复是显式动作，见 §4 裁决 H），因此
    `Degraded → Ok` 的自动回升**不存在**。

### 2.5 环形缓冲：**五带**还是**三带**？（裁决 E）

* **原文（三处，互不一致）**：
  * LLD M-21 §2.1 `logBuffer`：「WASM 压缩形态 trace 4×10³ / **debug+info 10³** /
    **warn+error 5×10²** 条」
  * LLD M-21 §2.1.1 `RingBuffer` 注释：「trace 100000 / **debug+info 10000** /
    **warn+error 5000**」
  * LLD M-21 §2.2 级别表：**每个级别**一行，各自带容量（WASM：trace 4×10³、debug 10³、
    info 10³、warn 5×10²、error 5×10²）
  * LLD M-21 §1.2 约束：「trace 4×10³、debug/info 10³、warn/error 5×10² 条
    （**共 7×10³ 条**）」+ 「192B 槽 × **7×10³** 条 ≈ 1.34MB」
* **实测推演**：
  * 若「debug+info 10³」= **合计** 1000 ⇒ 总量 = 4000+1000+1000 = 6000 ≠ 7×10³ ✗
  * 若「debug+info 10³」= **各** 1000 ⇒ 总量 = 4000+1000+1000+500+500 = **7000** ✔
    且与 192B×7000×5 带 = 1.34MB 的预算对账**逐字吻合** ✔
* **裁决**：**五级各自一个带**（5 带），WASM 形态容量
  `trace 4000 / debug 1000 / info 1000 / warn 500 / error 500`（合计 7000）；
  服务端 / 导出形态 `100000 / 10000 / 10000 / 5000 / 5000`（合计 130000）。
  §1.2 与 §2.1.1 里「debug+info」「warn+error」的写法按**「各」**读（与 §2.2 的表格
  和总额、预算对账一致）。
  ⇒ 规格全篇按 5 带写；`LevelCapacities` 提供可配置覆盖。

### 2.6 「log() 不分配」在 Rust 下不成立（裁决 M）

* **原文**：LLD M-21 §3.1「`log()` 期间不分配大块内存（槽位预分配于 RingBuffer 实例化
  时）」、§3.3「log() 期间不做格式化、不做 I/O、不分配大块内存（msg 定长槽拷贝）」。
* **实测**：Rust 的 `String` 模型下，「定长槽」要么用 `[u8; N]`（UTF-8 截断会切在多字节
  字符中间），要么用 `String`（每次写入分配）。
* **裁决**：
  * 条目用 `String`（serde 友好、可读、可测）。
  * **长度上界保留**：`ctx ≤ 24` 字节、`err_code ≤ 12` 字节、`msg ≤ 128` 字节，
    **超限按 UTF-8 字符边界截断**（**绝不切半个字符**）并置 `truncated = true`。
  * **「零分配」不承诺**，按「不分配大块内存（单条 ≤ 128B 量级）」**近似满足**，
    并**如实登记**（§8 新欠账）。**不得**在文档里声称「零分配」。
* ⚠️ **「槽位大小」与「有效载荷」的口径差一字节（第三次订正的显式记录）**：
    LLD 的伪码是 C 的定长槽 —— `char ctx[24]` / `char errCode[12]` / `char msg[128]`
    ⇒ C 的 `snprintf` 让**有效载荷**分别是 **23 / 11 / 127** 字节（末字节留给 NUL）。
    本批取**有效载荷 24 / 12 / 128 字节**（Rust 的 `String` 无 NUL）⇒ **比 LLD 各多
    1 字节**。这是**有意偏差**，在此显式记录，**不藏**：规格 §2.6 与 §5.2 的 B-3
    写的是「≤128 / ≤24 / ≤12 字节」，指的是**有效载荷**。
    ⇒ 代价：一条日志比 LLD 的槽位多存 1 字节；收益：不必在 Rust 里为「NUL 位」留坑
    （留了反而会出现「槽位 128 里有效载荷只有 127」的第二个口径）。

### 2.7 日志与健康信号**不上契约层**（裁决 I + L）

* **原文**：LLD M-21 §3.4「产品 / 嵌入式工作台不订阅日志流（L1 边界）」、
  §1.2「仅系统管理员通道」。
* **实测**：前端是产品 / 嵌入式工作台（M-01 / M-02 / M-03）⇒ 它**不该**看到系统日志。
* **裁决**：
  * **契约层零 DTO、零命令、零事件**（本批）。
  * 日志与健康信号是**进程内**通道（M-21 / M-22 / 宿主同进程）。
  * 服务端形态的「跨进程收集」是 **P1** ⇒ 届时再上契约层（**留账**，见 §8）。
  * ⇒ **前端零改动**是本批的**可验证判别式**（§5 D-9）。
* **实测补充（第一次订正）**：契约层**已有** `CommandPayload::Admin { op: AdminOp }`
  与 `AdminOp::{DrainLogs, KillGuest, PersistProject, Custom(String)}`（§1.4）。
  ⇒ 「日志不上契约层」的**准确**表述是：**本批零新增 DTO / 命令 / 事件**，
  而 `Admin` 通道的**空壳入口保持不动**。
  * `AdminOp::DrainLogs` 要真接通，必须先有**日志条目的契约 DTO 与回复事件**
    —— 那正是 **D-129**（P1）。⇒ 本批 `Admin` 命令**仍返回 `E_CTX-05`**（行为不变）。
  * `AdminOp::KillGuest` 的语义（M-22 watchdog escalation）与本批 D-115 重合，
    但本批的终止由**宿主内部**完成（看门狗触发即终止），**不经过**这个命令
    ⇒ 该变体**仍是预留**（真接通需要角色鉴权 ⇒ P1 · D-131）。
* ⇒ 前端**没有** `AdminOp` 的 JS 镜像（实测 `front/lib/message.js` 零命中）
  ⇒ 前端看不到管理员通道 ⇒ 「前端零改动」与角色边界一致。

### 2.8 `role` 从哪来（裁决 J）

* **原文**：LLD M-21 §3.4「三层鉴权：前端路由隐藏 → 内核接口鉴权 → 会话角色（角色来源 =
  M-04 会话创建时从用户登录态派生）」；LLD M-22 §3.3 `dispatch` 按角色分叉。
* **实测**：M-04 是**前端**模块（MVP 的宿主是浏览器 worker）⇒ 内核侧拿不到「用户登录态」。
* **裁决**：
  * `Hms` 持有一个 `Role { Admin, Product }`，由宿主在构造时设定，**默认 `Product`**
    （保守：默认看不见原始日志）。
  * **MVP 是单会话** ⇒ `session_id` 恒为 `0`（诚实登记，§8）。
  * 令牌鉴权三层是 P1（**留账**）。

### 2.9 「崩溃冻结」与「恢复」（裁决 H）

* **原文**：LLD M-22 22-12「崩溃 / 失控时冻结信号流（不再 shift），保证诊断包取证含失控
  前因」；§2.2「恢复 = 回退**崩溃前最近检查点** + 输入重放（22-2）」。
* **实测**：M-20 的恢复通道存在，但本批范围外（§0.4 ⑦）。
* **裁决**：
  * **冻结**本批落地：FATAL 信号入流后，`signals` 不再丢最旧。
  * **恢复**（回退检查点 + 重放）**不做**（要 M-20）⇒ `DegradeAction::SnapshotRestore`
    声明但 `Unsupported`（§4 裁决 H）。

---

## §3 冻结 API

> **签名即契约。** worker 必须按本节实现；实现与本节不符 ⇒ 算实现走样，不是「风格差异」。
> 落点文件：`crates/kernel/src/m21_logging/`、`crates/kernel/src/m22_hms/`、
> `crates/kernel/src/m23_datastore/`、`crates/kernel/src/kernel_host/`。

### 3.1 M-21 日志（`m21_logging`）

```rust
/// 日志级别（五级）。⚠️ **没有 Fatal** —— 致命属 M-22 的 Severity（§2.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info  = 2,
    Warn  = 3,
    Error = 4,
}

impl LogLevel {
    /// "TRACE" / "DEBUG" / "INFO" / "WARN" / "ERROR"
    pub fn as_str(self) -> &'static str;
    /// 五级序（Trace = 0 … Error = 4）—— 过滤判据 `level.rank() >= filter.rank()`
    pub fn rank(self) -> u8;
    /// 该级的默认带容量（WASM 形态，见 §2.5）
    pub fn default_capacity(self) -> usize;
}

/// 各带容量（可配置覆盖；默认 = §2.5 的 WASM 形态）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelCapacities {
    pub trace: usize,
    pub debug: usize,
    pub info: usize,
    pub warn: usize,
    pub error: usize,
}
impl Default for LevelCapacities { /* = §2.5 的 WASM 形态 */ }
impl LevelCapacities {
    /// 服务端 / 导出形态的容量（§2.5）
    pub fn server_profile() -> Self;
    pub fn of(self, level: LogLevel) -> usize;
}

/// 结构化日志条目（LLD M-21 §2.1.1 的 `logEntry`，8 字段）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// 虚拟时间（ps）—— 真相是 M-12 的时钟，调用方传入
    pub time_virt_ps: u64,
    /// 宿主时间（ms，单调）
    pub time_host_ms: u64,
    /// 会话关联标签（MVP 恒为 0，见 §2.8）
    pub session_id: u32,
    pub level: LogLevel,
    /// 「模块-流程点」标识，≤ 24 字节
    pub ctx: String,
    /// 完整场景码，可空，≤ 12 字节
    pub err_code: String,
    /// 消息文本，≤ 128 字节
    pub msg: String,
    /// 任一字段被截断时为 true
    pub truncated: bool,
}

/// 查询过滤（管理员通道）
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    /// 最低级别（含）
    pub min_level: Option<LogLevel>,
    /// 会话过滤
    pub session_id: Option<u32>,
    /// 虚拟时间下界（含）
    pub from_virt_ps: Option<u64>,
    /// 虚拟时间上界（含）
    pub to_virt_ps: Option<u64>,
}

/// 按级别分带的环形缓冲（五带，§2.5）
#[derive(Debug, Clone)]
pub struct LogBuffer {
    session_id: u32,
    filter: LogLevel,
    bands: [VecDeque<LogEntry>; 5],
    overflow_counts: [u64; 5],
    written: u64,      // **入缓冲**的条数（被过滤掉的**不计**）
    dropped_by_filter: u64,
    // ⚠️ **容量必须存住**（第四次订正）：`with_capacities` 要按它判溢出 ⇒
    // 没有它就无法回答「这条是丢了最旧的还是要溢出了」。**私有**字段，
    // 不进公开 API（公开面由 `LevelCapacities` 承载）。
    capacities: LevelCapacities,
}

impl LogBuffer {
    /// 新缓冲（session_id 注入；默认容量 = WASM 形态）
    pub fn new(session_id: u32) -> Self;
    /// 新缓冲（自定义容量）
    pub fn with_capacities(session_id: u32, caps: LevelCapacities) -> Self;

    /// 统一采集入口。**虚拟时间由调用方传入**（真相是 M-12 的时钟，§3.1 注）。
    pub fn log(&mut self, level: LogLevel, ctx: &str, msg: &str, err_code: &str,
               time_virt_ps: u64, time_host_ms: u64);

    /// 管理员检索：**快照读**，不移动游标、不改缓冲状态（LLD 评审 21-2）
    pub fn query(&self, filter: &LogQuery) -> Vec<LogEntry>;
    /// 导出 CSV（分块内容拼成一段；字段与 §3.1 的 8 字段一一对应）
    pub fn export_csv(&self, filter: &LogQuery) -> String;
    /// 导出 JSON（同上）
    pub fn export_json(&self, filter: &LogQuery) -> String;

    /// 运行时级别切换（仅管理员）。返回**旧**阈值。
    pub fn set_level(&mut self, level: LogLevel) -> LogLevel;
    /// 当前阈值
    pub fn level(&self) -> LogLevel;

    /// 总溢出次数（各带之和）
    pub fn overflow_count(&self) -> u64;
    /// 某级的溢出次数
    pub fn overflow_count_of(&self, level: LogLevel) -> u64;
    /// 入缓冲的条数（被过滤掉的不计）
    pub fn records_written(&self) -> u64;
    /// 被级别过滤丢弃的条数
    pub fn dropped_by_filter(&self) -> u64;
    /// 当前驻留条数
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// 清空（保留计数器）
    pub fn clear(&mut self);
    /// 会话标签
    pub fn session_id(&self) -> u32;
}
```

**实现要点（必须满足）**

1. **过滤在入口**：`level.rank() < filter.rank()` ⇒ 直接丢弃，**不**入带、
   **不**计 `written`、**计** `dropped_by_filter`。
2. **截断按 UTF-8 字符边界**：`ctx` / `err_code` / `msg` 逐字段截；
   **任一**字段被截 ⇒ `truncated = true`；**错误码与上下文不因 `msg` 截断而丢失**。
3. **溢出丢最旧**：该带满时写新条 ⇒ 丢该带最旧一条 + 该带 `overflow_count += 1`；
   **其它带不受影响**。
4. **`query` 是纯读**：不移动任何游标、不改任何计数 ⇒ **连续两次 `query` 结果相同**，
   且 `query` 之后 `query` 仍能看到同样的条目。
   ⚠️ **落地口径（第四次订正的实测澄清）**：`VecDeque` **本来就没有游标可动**
   ⇒ 「不移动游标」在 Rust 里的确切含义是「**不取走条目**（`drain` 不用）、
   **不改任何计数**（`written` / `dropped_by_filter` / 各带 `overflow_count`）」。
   ⇒ 判别式按这三条钉：**两次 query 结果相同** + **query 后缓冲内容与全部计数
   一条没变** + **query 后继续写仍能看到旧条目**。
5. **导出不丢字段**：CSV 的表头与每行必须含
   `time_virt_ps,time_host_ms,session_id,level,ctx,err_code,msg,truncated` **全部 8 列**；
   JSON 同理。**虚拟时间必须是真实值**（不许恒 0 —— v0.20「访存轨迹时间戳恒为 0」的
   同形状教训）。
6. **CSV 转义**：`ctx` / `msg` 里的逗号、引号、换行必须正确转义 ⇒ 导出再解析回来必须
   **逐字段等于**原条目（**往返判别式**）。

### 3.2 M-22 HMS（`m22_hms`）

```rust
/// 信号源（LLD M-22 §2.1.1 的 `src`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalSrc { Session, Command, Transport, Engine, Backend, Trap }

/// 信号种类（LLD §2.1.1 的 `kind`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalKind { Heartbeat, State, Error, Timeout, Crash, Conn, Watchdog, Trap }

/// 分级（LLD §2.1.1 的 `Severity`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity { Fatal, Critical, Warning, InfoNoDegrade }

/// 信号上下文（LLD 里 `ctx` 是 uint32；`GUEST_RUN` 是关键取值）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalCtx { GuestRun, Session, Queue, Connection, Other(u32) }

/// 健康信号（LLD §2.1.1 的 `HealthSignal`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthSignal {
    pub src: SignalSrc,
    pub kind: SignalKind,
    /// 完整错误码（可空）
    pub err_code: Option<String>,
    pub t_virt_ps: u64,
    pub t_host_ms: u64,
    pub ctx: SignalCtx,
}

/// 分角色（LLD §3.3）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role { Admin, Product }

/// 降级动作（LLD §3.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeAction { None, SnapshotRestore, Pause, Disconnect, TraceDown, Terminate }

/// 降级结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradeOutcome {
    /// 动作执行成功（第几次重试）
    Ok { action: DegradeAction, tries: u32 },
    /// 动作**未实现**（本批只有 Terminate 有实现）—— **不是失败**（§4 裁决 H）
    Unsupported { action: DegradeAction },
    /// 动作失败且重试用尽（≤3 次）⇒ 终态上报 `E_HMS_01`
    Failed { action: DegradeAction, tries: u32 },
}

/// 脱敏提示（产品 / 嵌入式角色，**不含原始日志**）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeMsg {
    pub severity: Severity,
    pub err_code: Option<String>,
    pub ctx: SignalCtx,
}

/// 诊断详情（管理员角色，含 M-21 日志关联）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminDiagnosis {
    pub severity: Severity,
    pub err_code: Option<String>,
    pub ctx: SignalCtx,
    pub t_virt_ps: u64,
    pub t_host_ms: u64,
    /// M-21 日志关联（**引用**，不是日志内容本身）
    pub log_ref: Vec<LogEntry>,
}

/// 指标项（LLD §3.5）
#[derive(Debug, Clone, PartialEq)]
pub struct Metric { pub t_virt_ps: u64, pub name: String, pub value: f64 }

/// 脱敏诊断包（LLD §3.5 · 评审 P-23：**不含原始日志**）
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticKit {
    pub err_code: Option<String>,
    pub snapshot_summary: String,
    pub input_seq_tail: Vec<String>,
}

/// 整体健康状态（§2.4）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health { Ok, Degraded, Critical }

/// HMS
#[derive(Debug, Clone)]
pub struct Hms {
    role: Role,
    signals: VecDeque<HealthSignal>,
    severities: VecDeque<Severity>,
    retained: VecDeque<(HealthSignal, Severity)>,  // FATAL / CRITICAL 保留带
    frozen: bool,
    severity_table: HashMap<(SignalSrc, String), Severity>,
    degrade_policy: HashMap<Severity, DegradeAction>,
    metrics: VecDeque<Metric>,
    watchdog_fired: bool,
    terminated: bool,
    last_escalation: Option<String>,
    // 降级动作执行器（可注入，宿主装配）
    executor: Option<Box<dyn DegradeExecutor>>,
    // M-23 的归档后端（可注入，宿主装配）—— 诊断包归档走它。
    // ⚠️ **第四次订正的裁定**：归档通道必须是**注入**的，不能让 M-22 自己
    // 造一个后端 —— 那会出现「两个真相」（M-22 造的那个 + 宿主那个）。
    store: Option<Box<dyn DataStore>>,
}

pub trait DegradeExecutor {
    /// 执行一次降级动作
    fn try_degrade(&mut self, action: DegradeAction) -> Result<(), PlatformError>;
}

impl Hms {
    pub fn new() -> Self;                       // role = Product，watchdog_fired = false
    pub fn with_role(role: Role) -> Self;
    pub fn install_executor(&mut self, ex: Box<dyn DegradeExecutor>);
    /// 注入归档后端（宿主装配）—— 不注入则 `archive_diagnostic_kit` 报
    /// 「没有归档通道」而不是静默成功。
    pub fn install_store(&mut self, store: Box<dyn DataStore>);

    /// 上报：**先分级再入流** → 分角色分发 → 查降级策略 → 降级（LLD §3.1）
    pub fn report(&mut self, signal: HealthSignal) -> Severity;
    /// 分级（LLD §3.2 的三条规则）
    pub fn classify(&self, signal: &HealthSignal) -> Severity;

    /// 配分级映射（可配置）
    pub fn set_severity(&mut self, src: SignalSrc, err_code: &str, sev: Severity);
    /// 配降级策略
    pub fn set_degrade_policy(&mut self, sev: Severity, action: DegradeAction);

    /// 整体健康状态（由信号流派生，§2.4）
    pub fn state(&self) -> Health;
    /// 最近一次升级的原因
    pub fn last_escalation(&self) -> Option<&str>;
    /// 幂等标志：watchdog 是否已触发过（一次运行只触发一次）
    pub fn watchdog_fired(&self) -> bool;
    /// 手动置位幂等标志（宿主在触发处置后调用）
    pub fn mark_watchdog_fired(&mut self);
    /// 内核是否已被终止
    pub fn terminated(&self) -> bool;

    /// 信号流（只读快照）
    pub fn signals(&self) -> Vec<HealthSignal>;
    /// 信号流长度
    pub fn signal_count(&self) -> usize;
    /// 保留带（FATAL / CRITICAL 不丢）
    pub fn retained(&self) -> Vec<(HealthSignal, Severity)>;
    /// 信号流是否已冻结（崩溃后不再丢最旧）
    pub fn frozen(&self) -> bool;

    /// 分角色分发结果
    pub fn dispatch(&self, signal: &HealthSignal, severity: Severity) -> DispatchResult;
    /// 采集指标（环形 4096）
    pub fn collect_metric(&mut self, name: &str, value: f64, t_virt_ps: u64);
    /// 指标快照
    pub fn metrics(&self) -> Vec<Metric>;

    /// 导出脱敏诊断包（**不含原始日志**）
    pub fn export_diagnostic_kit(&self, err_code: Option<&str>) -> DiagnosticKit;
    /// 归档诊断包（调 M-23 的 `archive`，§3.4）
    pub fn archive_diagnostic_kit(&mut self, err_code: Option<&str>) -> Result<(), PlatformError>;

    /// 执行降级（重试 ≤3 次；未实现动作 ⇒ `Unsupported`）
    pub fn degrade(&mut self, action: DegradeAction) -> DegradeOutcome;
}

/// 分发结果
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    /// 产品 / 嵌入式角色：脱敏提示
    Notice(NoticeMsg),
    /// 管理员角色：诊断详情 + 日志关联
    Admin(AdminDiagnosis),
}
```

**依赖方向（硬约束）**：`AdminDiagnosis.log_ref` 的类型是 **M-21 的 `LogEntry`**
⇒ **M-22 → M-21 单向依赖**；`archive_diagnostic_kit` 调 **M-23 的 `archive`**
⇒ **M-22 → M-23 单向依赖**。
⇒ **M-21 不依赖 M-22**（日志溢出上报由**宿主**做，见 §3.3 与裁决 K 的同类理由）
⇒ 三个模块之间**无环**。
⇒ **归档通道的形状**（第四次订正的裁定）：`Hms` 持有 `Option<Box<dyn DataStore>>`
（**宿主注入**）⇒ M-22 → M-23 **单向**；⚠️ **不许**让 M-22 自己造一个归档后端
（那会出现「两个真相」）；⚠️ **不许**让 M-23 实现 M-22 的 trait（那是**依赖倒置**）。
⚠️ **Lane C 的编译前提**：它引用 M-21 的 `LogEntry` ⇒ 必须等 **Lane B 的 M-21 落地**
（合入 `integration/v028`）之后才能编译 ⇒ 见 §9.1 的执行顺序。

**实现要点（必须满足）**

1. **`classify` 三条规则**（顺序固定）：
   1. `kind == Watchdog && ctx == GuestRun` ⇒ `Severity::Fatal`；
   2. `kind == Trap` ⇒ `Severity::InfoNoDegrade`；
   3. `severity_table` 命中 ⇒ 表里的值；
   4. 未命中 ⇒ `Severity::InfoNoDegrade`（**不降级**）。
2. **`report` 的顺序**：`classify` → 入流 → 分发 → 查策略 → 降级 ⇒
   **流里存的是分级后的结果**（`severities` 与 `signals` 同序）。
3. **保留带**：FATAL / CRITICAL 的信号**另外**进 `retained`，**永不丢弃**。
4. **崩溃冻结**：一旦有 FATAL 信号入流 ⇒ `frozen = true` ⇒ `signals` 不再丢最旧。
5. **`Health` 派生**：有 FATAL ⇒ `Critical`；有 CRITICAL / WARNING ⇒ `Degraded`；否则 `Ok`。
   **只降不升**。
6. **幂等**：`watchdog_fired` 为真时，`report` **不重复触发降级**（D-115 的核心判据）。
7. **角色边界**：`Role::Product` 的 `DispatchResult` **不含**任何日志条目；
   `Role::Admin` 的含 `log_ref`。⇒ 断言必须是「**Product 的结果里搜不到任何日志文本**」。
8. **诊断包不含原始日志**：`DiagnosticKit` 的字段里**没有**日志条目 ⇒ 断言「诊断包里
   不含 `LogEntry`」。
9. **`Unsupported` 不是失败**：本批只有 `Terminate` 有实现；其余动作 ⇒
   `DegradeOutcome::Unsupported` ⇒ **不重试**、**不**报 `E_HMS_01`。
10. **`Terminate` 的实现**：置 `terminated = true`（宿主据此停止接受新动作）。
11. **重试 ≤3**：动作执行失败 ⇒ 重试，**至多 3 次** ⇒ 仍失败 ⇒ `Failed` ⇒
    **终态上报 `E_HMS_01_DEGRADE_FAILED`**（一次运行只上报一次）。
12. **指标环**：容量 4096，满则丢最旧。

### 3.3 宿主看门狗（`kernel_host`）（裁决 F + G）

```rust
/// 宿主时间辅助预算（毫秒）—— LLD M-12 §3.5 的 `hostBudgetUs`（台账 D-115 的默认值）
pub const HOST_WATCHDOG_BUDGET_MS: u64 = 200;
/// 滑动窗口确认次数 —— LLD M-12 §3.5「需连续 3 个检查窗口均无进展才触发」
pub const HOST_WATCHDOG_CONFIRMATIONS: u32 = 3;

/// 一个检查窗口的判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOutcome {
    /// 有进展
    Progress,
    /// 无进展（停滞或宿主时间超窗）
    NoProgress,
}

/// **纯函数**：窗口判定（可单测，不依赖真实时钟）
pub fn classify_window(elapsed_ms: u64, stalled: bool) -> WindowOutcome {
    if stalled || elapsed_ms > HOST_WATCHDOG_BUDGET_MS {
        WindowOutcome::NoProgress
    } else {
        WindowOutcome::Progress
    }
}

/// 宿主看门狗状态
pub struct HostWatchdog {
    streak: u32,          // 连续无进展窗口数
    fired: bool,          // 幂等标志
}
impl HostWatchdog {
    pub fn new() -> Self;
    /// 喂一个窗口的结果。返回 `true` 表示**本次触发了**（且只在首次为 true）
    pub fn observe(&mut self, outcome: WindowOutcome) -> bool;
    /// 当前连续无进展窗口数
    pub fn streak(&self) -> u32;
    /// 是否已触发
    pub fn fired(&self) -> bool;
}
```

**宿主侧的接线（`kernel_host/mod.rs`）**

* **窗口粒度** = **引擎的一个批次**：
  * `step_in_chunks` 的**每一个 chunk**（`DEFAULT_INSTRUCTION_BUDGET = 10 000` 条指令）；
  * `continue_until_event` 的**每一次调用**（M-17 ⇒ `engine.run_until(..)` 跑满一个
    指令预算窗口）。
* ⚠️ **接线落点的精确位置（第二次订正的依据，见 §1.7）**：
  * **`step_in_chunks`（`:2049`）**：看门狗判定必须落在**它自己的 `while remaining > 0`
    循环体内**。理由：现有实现是 `let executed = self.backend.step_n(chunk)?;` ⇒
    chunk 一旦停滞就**当场 `?` 上抛**，循环直接退出 ⇒ **三次确认在循环外根本
    攒不起来**。⇒ 正确形态是「chunk 停滞 ⇒ 记一次窗口 ⇒ `streak < 3` 就**继续下一个
    chunk**（`remaining` 不减，因为那一条没跑成）⇒ `streak >= 3` 才返回 `Err`」。
  * **`on_run` 的 `Continue` 分支（`:1948`）**：`continue_until_event` 一次调用 = 一个
    窗口。⇒ 正确形态是「停滞 ⇒ 记一次窗口 ⇒ `streak < 3` 就**再调一次**
    `continue_until_event`（重入引擎）⇒ `streak >= 3` 才返回 `Err`」。
    正常情况第一次就是 `Ok`（跑满预算）⇒ **不会**进入重入。
  * ⇒ 两处共用**同一个** `HostWatchdog` 状态（连续性是它存在的意义）。
* **每个窗口**：
  1. 记 `Instant::now()`；
  2. 跑引擎；
  3. 算 `elapsed_ms`；
  4. `stalled` = 该批次返回 `Err` **且** `family == Engine` **且**
     `scenario == E_ENG_03_NO_PROGRESS`（**只认这一个码** —— 别的错误不是活锁）；
  5. `classify_window(elapsed_ms, stalled)` ⇒ `WindowOutcome`；
  6. `watchdog.observe(outcome)` ⇒ 若返回 `true` ⇒ **触发处置**（见下）。
* **触发处置**（顺序固定）：
  1. `hms.report(HealthSignal { src: Engine, kind: Watchdog, ctx: GuestRun,
     err_code: Some("E_ENG-03"), t_virt_ps: <M-12 的 now_ps>, t_host_ms })`
     ⇒ `classify` ⇒ `Severity::Fatal` ⇒ `hms.state() == Health::Critical`；
  2. 记 **一条** M-21 日志：`level = Error`、`ctx = "ENG-WATCHDOG"`、
     `err_code = Some("E_ENG-03")`、`msg` 说明「第 N 个窗口无进展，已终止」；
  3. `hms.mark_watchdog_fired()` ⇒ 幂等；
  4. `hms.degrade(DegradeAction::Terminate)` ⇒ 终止内核。
* **错误传播**：
  * 第 1、2 个窗口的停滞 **不**上抛 ⇒ 宿主**继续**跑（这正是「三次确认」的意义）；
  * 第 3 个窗口 ⇒ 停滞错误**上抛**给调用方（`Err(E_ENG_03)`）⇒ 一次运行只报一次。
* ⚠️ **重入不改状态**：M-12 的守卫触发时只是 `return Err`，**不改引擎状态** ⇒ 宿主重入
  引擎必须得到**相同**的引擎快照（判别式 §5 D-7）。

### 3.4 M-23 归档接口（`m23_datastore`）（裁决 K）

```rust
pub trait DataStore {
    // …既有五方法不动…
    /// 归档一段命名内容（诊断包 / 导出物）。
    ///
    /// ⚠️ **通用字节接口**：M-23 **不依赖** M-22 ⇒ 诊断包由 M-22 序列化成字节后传入。
    /// 这样 M-22 → M-23 是**单向**的，不产生模块环。
    fn archive(&mut self, name: &str, bytes: &[u8]) -> Result<(), PlatformError>;
}
```

* `InMemoryDataStore` 补实现：`archives: HashMap<String, Vec<u8>>`，
  **同名覆盖**（后写覆盖先写）⇒ 提供 `get_archive(&self, name) -> Option<&[u8]>` 供测试读回。
* ⚠️ **Storage 族此前零场景码**（第三次订正的实测依据）：契约层有 `Storage` 族标签与
  `STORE_FAMILY`，但**一个场景码都没有**（LLD 第六章把 `E_STORE` 记为「已声明未接线」
  族）⇒ A-5 的「用 Storage 族的合法场景码」在冻结规格写下时**无处可取**。
  ⇒ **裁定**：`E_STORE_01_ARCHIVE_FAILED: u16 = 1` 定义在
  **`crates/kernel/src/m23_datastore/mod.rs`**（**拥有该场景的模块**），
  **不上契约层**（契约层本批冻结为「只加两个 HMS 场景码常量」，§7）。
  ⚠️ **这不是违反 D-120**：D-120 禁的是「**同一个码有两份定义**」，
  而**既有惯例**本来就是「场景码住在拥有该场景的模块里」——
  M-14 的 `E_BUS_04/06/09/10/11/12`、M-08 的 `E_CTX_07`、M-11 的 `E_LDR_01/02`、
  M-17 的 `E_INT_01`、宿主的 `E_OBS_02` **全都在内核 crate 里**。
  ⇒ 全仓仍是**单一定义处** ✔。口径本身的分散问题另立 **D-132**（§8.2）。
* ⚠️ **既有五方法的签名与语义一条不改**（`the_trait_has_exactly_the_five_methods` 这条
  现有用例**必须改名为六方法**并同步断言 —— **这是本批唯一被允许的既有断言改动**，
  且**只许加一条、不许放松其余**）。

### 3.5 M-17 无进展守卫（D-114，裁决 N）

```rust
// crates/kernel/src/m17_backend/mod.rs —— `step_n` 的循环里
// ⚠️ 复用 M-12 的常量，**不复制字面量**（D-120 纪律：场景码与窗口常量只有一个定义处）
use riscv_sim_kernel::m12_engine::NO_PROGRESS_WINDOW;   // 实际路径按 crate 结构调整
```

* `step_n` 的循环里维护 `no_progress: u32`：
  * `Ok(ExecOutcome::InterruptTaken)` 分支：**虚拟时间没前进** ⇒
    `no_progress += 1`；`no_progress >= NO_PROGRESS_WINDOW` ⇒
    **返回 `Err(PlatformError::new(ErrorFamily::Engine, E_ENG_03_NO_PROGRESS, ..))`**；
  * 任何**有指令退休**（`Advanced` / `Stop { consumed: true }`）⇒ `no_progress = 0`。
* ⚠️ **口径与 M-12 一致**：中断交付**不算**进展（VD-F3）。
* ⚠️ **触发条件苛刻**：`step_n(n)` 的循环最多跑 `n` 次 ⇒ 只有 `n > NO_PROGRESS_WINDOW`
  才可能触发（台账 D-114 原文：「自旋的触发条件比 M-12 苛刻」）⇒ 判别式用例必须用
  `n = NO_PROGRESS_WINDOW + 1` 以上。

---

## §4 裁决清单（A~N）

| 编号 | 裁决 | 依据 |
| --- | --- | --- |
| **A** | `HMS_FATAL` 作废；用 `HealthSignal{kind: Watchdog, ctx: GuestRun}` ⇒ `Severity::Fatal` | §2.1 |
| **B** | `EV_WATCHDOG` **不是**事件；契约层**不加**事件变体；日志 `ctx = "ENG-WATCHDOG"` | §2.2 |
| **C** | M-21 五级无 `Fatal`；M-22 的 `Severity` 四值 | §2.3 |
| **D** | `Health` 保留但改**派生**语义；`Severity` 是单条信号的级别；二者**不是**一回事 | §2.4 |
| **E** | 环形缓冲**五带**（各带独立容量），WASM 形态 4000/1000/1000/500/500 = 7000 | §2.5 |
| **F** | 看门狗**全在宿主侧**；M-12 **零改动**；窗口 = 引擎的一个批次 | §1.5 / §0.4 ⑥⑧ |
| **G** | 停滞错误**不立即上抛**；连续 3 个窗口才上抛 ⇒ 这就是「滑动窗口三次确认」 | §3.3 |
| **H** | 降级动作本批只实现 `Terminate`；其余 `Unsupported`（**不是**失败、不重试、不报错码） | §0.4 ⑦ / §2.9 |
| **I** | 日志 / 健康**不上契约层**（进程内通道）⇒ 本批**零新增** DTO / 命令 / 事件；⚠️ **既有**的 `Admin` 命令与 `AdminOp` 三变体**保持不动**（`DrainLogs` 要真接通必须先有 DTO ⇒ D-129）；前端零改动 | §2.7 |
| **J** | `role` 由宿主设定，默认 `Product`；MVP 单会话 ⇒ `session_id` 恒 0 | §2.8 |
| **K** | M-23 归档用**通用字节接口** `archive(name, bytes)` ⇒ M-22 → M-23 单向 | §3.4 |
| **L** | `Severity` / `LogLevel` / `HealthSignal` 等**不上契约层**（进程内类型） | §2.7 |
| **M** | 「零分配」**不承诺**；保留长度上界 + UTF-8 边界截断 + `truncated` 标记 | §2.6 |
| **N** | `NO_PROGRESS_WINDOW` **复用 M-12 的常量**，M-17 不复制字面量 | §3.5 |

---

## §5 验收清单

> 每条验收都要求**至少一条用例**把落点钉住。**判别力**列写明「改坏哪里会变红」——
> 这是本规格的硬要求：**一条都不会变红的验收等于没写**。

### 5.1 Lane A · 契约层 + M-23 归档（A-1 ~ A-6）

| 编号 | 验收 | 判别力 |
| --- | --- | --- |
| A-1 | `E_HMS_01_DEGRADE_FAILED` / `E_HMS_02_FATAL_TERMINATED` 在契约层，族 = `Health`（tag `HMS`），**全仓只有一处定义** | 在内核里另写一份字面量 ⇒ 完整性用例红 |
| A-2 | 族标签完整性用例仍过（新增族则红 —— 本批**不加**新族 ⇒ 必须仍绿） | 误加 `Log` 族 ⇒ 红 |
| A-3 | `DataStore` trait 现有**六**方法（既有五方法签名**一字未改** + 新增 `archive`） | 改既有签名 ⇒ 现有用例红 |
| A-4 | `InMemoryDataStore::archive` 同名覆盖 + `get_archive` 读回**逐字节相同** | 不覆盖 / 不读回 ⇒ 红 |
| A-5 | 归档失败（注入失败实现）⇒ `E_HMS_01`? **不** —— 归档失败走 `ErrorFamily::Storage` | 用错族 ⇒ 红 |
| A-6 | `the_trait_has_exactly_the_five_methods` 改名并同步断言为**六**（**只许加，不许放松**） | 断言被放松 ⇒ 复核时人工比对 |

### 5.2 Lane B · M-21（B-1 ~ B-10）

| 编号 | 验收 | 判别力 |
| --- | --- | --- |
| B-1 | 五级齐全、`as_str` 逐字为 `TRACE/DEBUG/INFO/WARN/ERROR`、**无 `Fatal`** | 漏 `Debug` / 多 `Fatal` ⇒ 红 |
| B-2 | 过滤：阈值之上的**不**入带；被过滤的**不计** `written` 但**计** `dropped_by_filter` | 计数口径错 ⇒ 红 |
| B-3 | 截断：`msg` 200 字节 ⇒ 落盘 ≤128 字节 + `truncated = true`；**按 UTF-8 字符边界**（中文不被切半） | 不截 / 切半字符 ⇒ 红 |
| B-4 | 溢出：容量 2 的带写 3 条 ⇒ 丢最旧 + 该带 `overflow_count == 1`；**别的带不受影响** | 计数错 / 跨带丢 ⇒ 红 |
| B-5 | `query` 是**纯读**：两次结果相同，且 query 后缓冲内容不变 | query 推进游标 ⇒ 红 |
| B-6 | `set_level` 返回**旧**阈值；切换后旧阈值不再写入 | 返回新值 ⇒ 红 |
| B-7 | 导出 CSV/JSON 含**全部 8 列**；**虚拟时间非 0** | 漏列 / 时间恒 0 ⇒ 红 |
| B-8 | **导出往返**：导出 → 解析 → 逐字段**等于**原条目（含 `truncated`） | 转义错 ⇒ 红 |
| B-9 | `session_id` 过滤生效 | 过滤不生效 ⇒ 红 |
| B-10 | `LevelCapacities::server_profile()` = 100000/10000/10000/5000/5000 | 数字错 ⇒ 红 |

### 5.3 Lane C · M-22（C-1 ~ C-12）

| 编号 | 验收 | 判别力 |
| --- | --- | --- |
| C-1 | `classify`：Watchdog+GuestRun ⇒ `Fatal` | 判成 `Critical` ⇒ 红 |
| C-2 | `classify`：Trap ⇒ `InfoNoDegrade` | 判成 `Warning` ⇒ 红 |
| C-3 | `classify`：`severity_table` 命中 ⇒ 表值；未命中 ⇒ `InfoNoDegrade` | 兜底判成 `Warning` ⇒ 红 |
| C-4 | `report` **先分级再入流** ⇒ 流里的 severity 是分级后的值 | 入流的是原始 kind ⇒ 红 |
| C-5 | FATAL / CRITICAL 进**保留带**，流满也不丢 | 丢保留带 ⇒ 红 |
| C-6 | 有 FATAL ⇒ `frozen` ⇒ 流不再丢最旧 | 不冻结 ⇒ 红 |
| C-7 | `Health` 派生：有 FATAL ⇒ `Critical`；只有 WARNING ⇒ `Degraded`；无 ⇒ `Ok` | 派生错 ⇒ 红 |
| C-8 | **幂等**：`watchdog_fired` 为真 ⇒ 重复 `report` **不重复触发降级** | 重复触发 ⇒ 红（D-115 的核心） |
| C-9 | **角色边界**：`Role::Product` 的分发结果里**搜不到任何日志文本**；`Role::Admin` 的**含** `log_ref` | 越界 ⇒ 红 |
| C-10 | 降级：未实现动作 ⇒ `Unsupported`（**不重试**、**不**报 `E_HMS_01`） | 当失败重试 3 次 ⇒ 红 |
| C-11 | 降级：可注入的执行器失败 ⇒ 重试**至多 3 次** ⇒ `Failed` ⇒ **恰好一次** `E_HMS_01` | 重试次数错 / 重复上报 ⇒ 红 |
| C-12 | 诊断包**不含原始日志**；归档后**字节可读回**（走 M-23） | 含日志 / 读不回 ⇒ 红 |

### 5.4 Lane D · D-114 + D-115（D-1 ~ D-10）

| 编号 | 验收 | 判别力 |
| --- | --- | --- |
| D-1 | `step_n` 遇**反复交付**的中断（`n > NO_PROGRESS_WINDOW`）⇒ 返回 `Err(E_ENG_03)`，**不**自旋 | 无守卫 ⇒ 挂死（用例超时 ⇒ 红） |
| D-2 | `step_n` 的**既有**语义一条不变（既有断言不改） | 改坏 ⇒ 红 |
| D-3 | 宿主：连续 **3** 个无进展窗口 ⇒ `hms.report` 被调 ⇒ `Severity::Fatal` ⇒ `Health::Critical` | 少一次 / 不报 ⇒ 红 |
| D-4 | **只报一次**：再喂几个窗口 ⇒ `watchdog_fired` 仍为真、`report` 调用次数**不增** | 重复上报 ⇒ 红（幂等） |
| D-5 | **WATCHDOG 日志存在**：M-21 环里有一条 `ctx == "ENG-WATCHDOG"`、`err_code == "E_ENG-03"` 的条目 | 没记日志 ⇒ 红 |
| D-6 | **不多报**：只喂 **1** 个 / **2** 个窗口 ⇒ `report` **未**被调、M-21 环**无** `ENG-WATCHDOG` | 提前上报 ⇒ 红 |
| D-7 | **重入不改状态**：三次重入后引擎快照 == 一次重入后（除虚拟时间外一致） | 状态漂移 ⇒ 红 |
| D-8 | `classify_window` 是**纯函数**：`(elapsed > 200, stalled = false)` ⇒ `NoProgress`；`(0, true)` ⇒ `NoProgress`；`(0, false)` ⇒ `Progress` | 判定错 ⇒ 红 |
| D-9 | **前端零改动**：`git diff --stat` 里 `front/` **零命中**；契约层**零新增** DTO / 命令 / 事件（⚠️ **既有**的 `Admin` / `AdminOp` 保持不动，宿主对该命令**仍返回 `E_CTX-05`**） | 前端被改 / `Admin` 被启用 ⇒ 红（人工核对） |
| D-10 | 停滞错误**第 3 次才上抛**；第 1、2 次宿主**继续**跑 | 立即上抛 ⇒ 红（G 裁决） |

### 5.5 本批的**唯一**判别式（源码层面成立）

> **换掉「守卫触发」这个事实 ⇒ 宿主处置链路整条变红；换掉任何一个环节 ⇒ 至少一条变红。**

链路：

```
M-12/M-17 守卫触发  ──►  Err(E_ENG_03_NO_PROGRESS)
        │
        ├─► 宿主窗口判定 classify_window(elapsed, stalled) ──► NoProgress
        │        └─► HostWatchdog::observe ──► 第 3 次 ⇒ 触发
        │
        └─► hms.report(HealthSignal{Watchdog, GuestRun})
                 ├─► classify ⇒ Severity::Fatal
                 ├─► 入流 + 保留带 + frozen
                 ├─► Health::Critical
                 ├─► 幂等标志 watchdog_fired
                 └─► M-21 环里一条 ctx="ENG-WATCHDOG" 的日志
```

**三处「换个说法就会变红」的地方**（变异自检的靶子）：

1. 守卫的错误码写成别的 ⇒ D-3 / D-5 红（宿主只认 `E_ENG_03`）。
2. `classify` 的 watchdog 规则改成 `Critical` ⇒ D-3 红。
3. 幂等标志去掉 ⇒ D-4 红。

---

## §6 门禁与变异要求

### 6.1 九项门禁（与前批一致）

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --no-fail-fast --lib --tests`
4. `tools/check-front.sh`（**应保持 312 用例 / 57 suite 全绿** —— 本批前端零改动）
5. `tools/golden-run.sh --self-check` + `--keep-going`（68/68）
6. `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
7. `node tools/browser-verify.mjs`（真 Chrome）
8. `tools/boot-osz.sh`（**VERDICT MATCH**）
9. **变异自检**：每 lane 至少一条**对照**（只加注释）必须 **0 红**。

### 6.2 变异要求（每 lane）

* 变异必须**能编译**；
* 记录**完整红集**（不是「至少一条红」）；
* 还原用 `cp` + `md5` + `cmp` + `touch`；**禁** `git checkout -- <file>`；
* 备份名用**相对路径**做键；
* 变异期间**不改**会被编译的其它源文件；
* **必须有超时**（`tools/watchdog.pl <秒> <cmd>`；124 = HANG）。
* ⚠️ **macOS 新链接二进制首 exec 会被安全评估卡住**（实测 51 秒墙钟 / 0.012 秒 CPU）
  ⇒ **串行**预热全部测试二进制，**不要并行**；评估期间杀进程 ⇒ 下次重来。

### 6.3 本批的**特有**要求

* **既有断言只许增不许放松**：唯一允许的改动是 A-6（trait 方法数 5 → 6）。
* **禁止**为了让用例变绿而放松既有断言。
* **新增用例必须落在**所属模块的 `#[cfg(test)] mod tests`（Lane B / C）或
  `crates/kernel/tests/`（Lane D 的集成用例）。

---

## §7 破坏性变更

| 变更 | 影响 | 处置 |
| --- | --- | --- |
| `m21_logging::Level` → `LogLevel`（去掉 `Fatal`、加 `Debug`） | 仓内**零外部引用**（§1.1）⇒ 无外部破坏 | 直接改 |
| `m21_logging::LogRecord` → `LogEntry`（8 字段） | 同上 | 直接改 |
| `m22_hms::Hms::report(&str)` → `report(HealthSignal) -> Severity` | 同上（§1.2） | 直接改 |
| `m22_hms::Health` 语义变更（盲升 → 派生） | 同上 | 直接改；**语义变化要在模块文档里写明** |
| `DataStore` trait **加**一个方法 `archive` | **对 trait 的实现方是破坏性的** | `InMemoryDataStore` 同批补实现；A-3 / A-6 钉住 |
| 宿主停滞错误的**传播时机**（第 3 次才上抛） | 调用方看到错误的**时刻**变了 | §3.3「错误传播」写死；D-10 钉住 |
| 契约层 | **零新增** DTO / 命令 / 事件 / 错误族 | 只加两个 HMS **场景码常量**（不新增族 ⇒ 不改 `error_families!`） |

---

## §8 台账关系

### 8.1 本批闭环

| 欠账 | 闭环口径 |
| --- | --- |
| **D-115** | 宿主侧时间预算（`HOST_WATCHDOG_BUDGET_MS = 200`）+ 滑动窗口三次确认（`HOST_WATCHDOG_CONFIRMATIONS = 3`）+ **HMS 同步终止**（`Severity::Fatal` ⇒ `Health::Critical` + `DegradeAction::Terminate`）+ **`WATCHDOG` 日志**（M-21 环里 `ctx = "ENG-WATCHDOG"`）+ **幂等标志**（`watchdog_fired`）—— 判别式 D-3 / D-4 / D-5 / D-6 / D-10。⚠️ **代价如实记录**：因 M-12 的 `no_progress` 是局部变量（§1.7），三次确认实际空转 **3 × 1024** 次 |
| **D-114** | M-17 `step_n` 接上**同形状**的无进展守卫（复用 M-12 的 `NO_PROGRESS_WINDOW` ⇒ 不复制字面量）⇒ 同一个不变式（M-16 的 `active_irq` 单级防护）的**第二个落点**被堵上 —— 判别式 D-1 / D-2 |

### 8.2 本批新登记（**四**条）

| 编号 | 事项 | 优先级 |
| --- | --- | --- |
| **D-128** | **M-21 的「零分配」在 Rust 下不成立**：LLD §3.1 / §3.3 要求「log() 期间不分配大块内存（槽位预分配）」，本批按「长度上界 + UTF-8 边界截断 + `truncated` 标记」**近似满足**并如实登记 ⇒ 「定长槽」语义降级为「有界长度」 | P2 |
| **D-129** | **日志 / 健康信号的跨进程载体缺失**：本批判定为「进程内通道，前端不订阅」（角色边界）⇒ 服务端形态（P1）的「管理员跨进程收集」**没有契约载体**。⚠️ **具体证据（第一次订正）**：契约层**已有** `CommandPayload::Admin { op: AdminOp }` 与 `AdminOp::DrainLogs`（注释就写着「Dump the kernel log buffer (M-21)」）—— **空壳入口在，缺的是日志条目的 DTO 与回复事件** ⇒ 宿主对该命令只能一律返回 `E_CTX-05`「管理通道未启用」⇒ 管理员通道**接不上**。届时需上契约层 DTO + 回复事件 | P1（随服务端形态） |
| **D-130** | **降级动作只落地 `Terminate`**：`SnapshotRestore` / `Pause` / `Disconnect` / `TraceDown` 声明但 `Unsupported` ⇒ 它们分别要 M-20 / M-04 / M-07 / M-19 | P2 |
| **D-131** | **MVP 单会话 + 角色降级**：`session_id` 恒 0；`role` 由宿主设定、默认 `Product`；LLD M-21 §3.4 的三层鉴权（令牌）**未做** | P2 |
| **D-132** | **场景码定义处分散在契约层与内核 crate**：契约层有 `PROTO` / `ENG` / `PLT` / `DBG` / `SNAP` / `PER` 等族的场景码，内核 crate 里又有 `BUS` / `CTX` / `LDR` / `INT` / `OBS` / `STORE` 等族的场景码 ⇒ 「场景码住哪」这条纪律**没有统一口径**（D-120 只禁了「同一码两份定义」，没禁「定义处分散」）。⚠️ 本批新增的 `E_STORE_01` 又添了一处。⇒ 需要一条明确口径（哪些族必须上契约层 —— 判据应是「**跨模块共享 / 前端要按码分派**」） | P2 |

### 8.3 本批**不**动的欠账

D-94（S 扩展 / PMP / 计数器）· D-87（访存轨迹带值）· D-89（裁决 G 粒度）
⇒ 三条**保持未做**，原状登记。

---

## §9 lane 划分与文件所有权

> **lane 按「文件所有权」切**：每个 lane 独占一组文件 ⇒ 合流时**零冲突**。

| Lane | 文件（**独占**） | 事项 | 依赖 |
| --- | --- | --- | --- |
| **A** | `crates/contract/src/error.rs` · `crates/kernel/src/m23_datastore/` | HMS 场景码 + M-23 `archive` 接口 + `InMemoryDataStore` 实现 | 无 |
| **B** | `crates/kernel/src/m21_logging/` | M-21 全量落地 | 无（不依赖 A） |
| **C** | `crates/kernel/src/m22_hms/` | M-22 全量落地 | A（场景码 `E_HMS_01`） |
| **D** | `crates/kernel/src/m17_backend/` · `crates/kernel/src/kernel_host/` | D-114 + D-115 接线 + 集成用例 | **A + B + C 全部合入 `integration/v028` 之后**才启动 |

### 9.1 执行顺序（本机 8 核 / 8 GiB ⇒ **串行为主**）

1. **Lane A**（薄）与 **Lane B**（M-21）**并行**开工，各自带独立 `CARGO_TARGET_DIR`。
   ⚠️ 两个 cargo 同时编译在 8 核 / 8 GiB 上可承受，但**验证阶段必须串行**
   （并发 cargo ⇒ 假象 + swap 风暴）。
2. A 与 B 都完成后 ⇒ 各自 `--no-ff` 合进 `integration/v028`。
3. **Lane C**（M-22）**必须等 A 与 B 都合入 `integration/v028`** 才能开工：
   它引用契约层的 `E_HMS_01_DEGRADE_FAILED`（Lane A 的产出）**和** M-21 的
   `LogEntry`（Lane B 的产出）⇒ **提前开工必然编译不过**。
   ⇒ Lane C 的 worktree 要**基于 `integration/v028`** 重建（不是基于 `main`）。
4. **Lane D**（接线）必须等 A / B / C **全部**合入 `integration/v028` 才启动
   （它要用到三者的真实 API）。
5. 全量门禁在 `integration/v028` 上跑。
6. `--no-ff` 合回 `main` ⇒ 收口文档。

### 9.2 分支与工作目录

* 分支：`feature/v028-contract-store` / `feature/v028-logging` / `feature/v028-hms` /
  `feature/v028-watchdog`；集成分支 `integration/v028`。
* worktree 一律落在 **`/Users/zyzs/lczyzs/ai_workspace/v028/`**（含各 lane 的
  `CARGO_TARGET_DIR`）⇒ 仓内不留构建产物。
* 规格分支 `docs/stage-13-spec`。

---

## §10 规格自检（合 main 前必须过）

* 表格列数一致（Markdown）；
* 标题层级不跳级；
* 代码围栏配平；
* 裁决编号（A~N）无悬空引用；
* 章节完整（§0~§10 齐）；
* §5 的每个验收编号在 §5 内被引用，§4 的每个裁决编号在正文被引用。
