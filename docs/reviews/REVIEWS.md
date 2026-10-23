# 检视与欠账总纲 · Reviews & Debt Ledger

> **定位**：本文件是 **`docs/reviews/` 的唯一入口**，也是**全部 review 欠账的唯一追踪入口**——跨模块条目与模块内条目都在这里汇总状态。
> 模块问题的**详细分析、实证记录与修复建议**见各模块报告（§4 索引）；**每次改动的来龙去脉、决策与工作量**见 `docs/progress/PROGRESS.md`。
> **建立日期**：2026-10-19（台账）· 2026-10-20（并入两轮检视汇总）
> **状态核实日期**：2026-10-21（v0.17 契约 v2 批：**数字口径从当前树重算**；其余条目沿用 v0.15 D-07 闭环后的逐条源码核对结论，非引用历史结论）
> **基线**（**2026-10-21 从最终树重算**）：`cargo test --workspace --no-fail-fast` **636 passed / 0 failed**（契约 lib **36** + kernel lib **505** + 集成 **84** + golden runner 11）；`clippy -D warnings` 干净；`fmt --check` 干净；前端门禁 **208 用例 / 35 suite**（`tools/check-front.sh`）
> **关键结论**：v0.10 记录的「三条现有门禁对本台账全部 5 项阻断级条目均无感知」**已不成立**——本轮补上了 3 类最小门禁（M-13 x0 / M-12 确定性 / M-06 畸形输入），**3 条阻断级条目全部消除**。
> **独立检视轮（2026-10-20，v0.11.1）**：4 路独立检视（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）共查出 **22 项**：1 阻断级（M-13 SYSTEM 未校验 funct3，已修）+ 2 高危（F-3 总线 hook panic，已修；F-1 快照语义承诺，已订正文档）+ 若干中低危。**该轮阻断级与高危已全部闭环**，中低危分流为 D-24~D-35。详见 §3.2。
> **收口轮（2026-10-20，分支合 main 前）**：3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）共查出 **37 项**（去重后）：**0 阻断 + 5 高危 + 32 中低危**。**三路判定均为 FAIL**，逐项修复后 **34 项闭环 / 3 项留账**（D-38 / D-39 / D-40）。⚠️ 另有 2 条**已闭环但带残留工作量**的条目——V-A 🟠-1 与 V-C H-1——其剩余部分登记为 **D-36 / D-37**（**高危，未做**）。详见 §3.1。
> **D-24 已闭环（2026-10-20 收口轮）**：契约层新增可序列化快照 DTO（`crates/contract/src/snapshot.rs`），`ExecutionBackend` 增 `snapshot_full` / `restore_full`（additive 默认实现），M-12 ↔ DTO 双向转换落地，M-17 接线完成。⚠️ **该 DTO 只覆盖 M-12/M-17 段（引擎）**，不含外设（M-15）/ 中断控制器（M-16）/ 内存映射——**落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 组装**，见 **D-36 / D-37**。
> **契约层 v2 落地（2026-10-21 · `3126f54`）**：契约层自 v0.10 起冻结在 `schema_version=1`，本批**首次升版**到 **2**（冻结规格 = `docs/detailed-design/STAGE-2_Contract_V2.md`）。本批**闭环 6 条台账**：**D-32**（契约缺口 7 项）· **D-34**（`Waveform` / `WaveformDelta` 命名歧义）· **D-39**（快照队列到期时刻校验）· **D-40**（空快照显式拒绝）· **D-57 / D-58**（契约端口文档与实现口径订正，**改文档不改实现**）。⚠️ **D-36 / D-41 / D-64 本批不动**——**契约层冻结已解除**，但它们各自的**前置条件**（M-15 / M-16 的状态导出接口、M-16 的陷阱投递）尚未具备，分别留给 **D-37 批**与 **M-16 批**。
> **维护约定**：本文件是**活文档**。模块报告的闭环章节记录该模块的细节；本文件只保留**汇总状态 + 跨模块拓扑 + 处理顺序**；两者冲突时以本文件的核实日期为准。

### 文档导航

| 章节 | 内容 | 性质 |
| --- | --- | --- |
| **§0 速查** | 当前状态：模块级闭环总览 / 欠账分档 / 跨模块链路连通性 | 活（随闭环更新） |
| **§1 欠账明细** | 阻断级 / 跨模块高危 / 跨模块中低危 / 工程门禁缺口 | 活 |
| **§2 建议处理顺序** | 批次拓扑 + 投入产出比 + 整体判断 | 活 |
| **§3 检视轮归档** | 3.1 收口轮（V-A/V-B/V-C）· 3.2 v0.11.1 轮（V1~V4） | 史（只增不改） |
| **§4 模块报告索引** | 9 份模块详细报告 + 各自闭环状态 | 活 |
| **§5 变更记录索引** | 按事件的事件级索引（含 commit） | 活 |

> **三处「单一权威副本」的去向**（2026-10-20 归拢）：
> ① **模块状态速查**（每模块一行 ✅/🟡/⚪ + 一句话）→ `AGENTS.md` 的模块状态表；本文件 §0.1 只留台账特有的**行数 / 测试数 / 闭环数**。
> ② **改动的来龙去脉**（版本叙事、决策记录、工作量）→ `docs/progress/PROGRESS.md`；本文件 §5 只留**事件级索引 + commit hash**。
> ③ **各模块检视发现的逐条明细** → 9 份模块报告；本文件 §3 的轮次归档只留**横向视角 + 结论 + 指针**。


---

## §0 速查（当前状态）

### 0.1 模块级闭环总览


| 模块 | 代码 | 测试 | 报告项 | 闭环 | 未闭环 |
| --- | --- | --- | --- | --- | --- |
| **M-14 总线** | 1 361 → **3 563** | **64** | 16 + 接线 PR + D-46 检视轮 + **v0.18 会话快照（导出 / 载入）** | **16** | 0 |
| **M-15 外设** | 1 424 → **3 249** | **58** | 19 + N1~N4 = 23 + **v0.18 会话快照（导出 / 载入）** | **23** | 0 |
| **M-18 探针** | 1 893 → **2 354** | **37** | 15 + D-46 检视轮 + **v0.20 批 `TraceEvent.is_fetch` 接线** | **13** | 2（低危） |
| **M-19 波形** | 3 897 | 85 | 16 | **16** | 0 |
| **前端 M-01/02/03** | 1 123 → **2 464** | **88** | 20（16 首审 + 4 漏项）+ **v0.20 批反汇编面板（M-02，20 条新增用例）** + **v0.22 批 Lane A 源码视图（15 条新增用例）** | **20** | 0 |
| **M-06 加载器** | 1 189 → **3 634** | **69** | **17（v0.11 首次模块级 review）** + **v0.20 批 Lane B（带出调试节区原始字节，13 条新增用例）** + **v0.22 批 D-101②（`sh_entsize` 校验落 M-06，撒谎的 symtab 整镜像拒收）** | **14** | 3（留账并说明理由） |
| **M-12 引擎** | 226 → **2 493** | **30** | 台账 4 项（D-05/06/17/**07**） + **v0.22 批 D-101①（`restore_full` 走 CSR 表过滤）** + **v0.22 检视轮 V-D 专项（**D-30 闭环**：M-12 首次有独立模块级 review，9 项发现中 3 条中危由 Lane W2 修完）** | **9** | 0（**独立模块级 review 已做** → V-D 专项检视 + Lane W2） |
| **契约层** | 1 163 → **6 322** | **106** | 检视轮 D-24 根因修复 + **v0.17 契约 v2 批（D-32/34/39/40/68/69）** + **v0.18 会话载荷（六段 DTO + `GPIO_PIN_COUNT` + `Peripheral` 加法方法）** + **v0.20 契约 v3 批（4 命令 / 4 事件 / 3 DTO，只增不改）** + **v0.21 CSR 批（`misa` / `mscratch` / `pending_lines_to_mip_mask` / `std_irq_bit_for_line` / `csr_is_implemented`）** + **v0.22 批（`MSTATUS_WARL_MASK` / `MEPC_ALIGN_MASK` 两个掩码常量住契约层，**M-13 只引用不复制**）** | **1** | 0 |
| **M-13 ISS** | 456 → **7 858** | **210** | 14（v0.11 首次模块级 review + 检视轮补 #14）；另有台账 D-43 已闭环（v0.16 M/C 扩展）；**v0.21 CSR 批**：CSR 指令族 + `mret` + `misa` + **陷入时 `MPP←M`（D-97 闭环）**；**v0.22 批**：**D-99**（未映射访存 → cause 5/7）· **D-93**（`mepc` 三路径口径）· **D-98**（`mstatus` WARL 掩码） | **17** | 0 |
| **M-11 符号 / DWARF / 反汇编 / trace 导出** | 53 → **5 244** | **94** | **20**（v0.20 批 · V-A 8 + V-B 8 + 收口自查 3 + 收口轮变异自检 1） + **v0.22 批 Lane A（DWARF 行表真解析 v2/v3/v4/v5，27 条新增用例）** + **v0.22 检视轮 Lane W1（VA-01/02/03 三条中危 + 4 条用例）** | **16** | 4（**D-86 ~ D-89**，均 P1/P2） |
| **前端 M-04/05/07/10** | 66 → **3 627** | **157** | 7 项契约缺口 + 3 自查缺陷 | 3（已修） | 7（契约缺口 → D-32，留 Wave 2） |
| **M-08/09/16/17/20/21/22/23** | 591 → **5 867** | **116** | 无（→ **D-45** 已闭环）+ D-07 检视轮（M-17 侧）+ **v0.18 会话快照（M-16 导出 / 载入、M-20 组装、M-23 落盘）** + **v0.21 CSR 批（M-16 `pending_lines` / M-17 `mip` 投影）** + **v0.22 批 D-100（`pending_lines` → `Option`，`project_mip` 遇 `None` 跳过写）** | — | — |
| **内核宿主（装配层 · 非 23 模块之一）** | **6 091** | **98** | 第 1 阶段新增（→ **D-46** 已闭环）+ D-46 检视轮 + **v0.17 契约 v2 接线** + **v0.18 会话采集 / 还原** + **v0.20 契约 v3 调试面接线（4 命令 / 4 事件）** + **v0.22 批 Lane A（`on_resolve_line` 改走 `SymbolService` 端口，返回真 `file` + `line`）** | — | — |

> **模块状态速查**（每模块一行 ✅/🟡/⚪ + 一句话说明）见 `AGENTS.md` 的模块状态表——**本表只留台账特有的行数 / 测试数 / 闭环数**。
> **统计口径**（**2026-10-22 从当前树重算**；V4 曾指出此前数字失真，v0.17 批再次重算，v0.18 批第三次重算，v0.19 批第四次重算，v0.20 批第五次重算，v0.21 批第六次重算，**v0.22 批第七次重算**〔三条修复 lane 合入后**又重算一次**，见下〕）：「代码」= 该模块目录下**全部 `.rs` 文件的总行数**（含测试文件）；「测试」= 该模块目录下 `#[test]` 的**用例条数**。前端：「代码」= `front/views`（含 `front/views/components/`）+ `front/app.js` / `index.html` / `style.css` = **2 464**（v0.20 批 M-02 加反汇编面板；**v0.22 批 Lane A 新增 `front/views/components/source-view.js` + `front/app.js` 三处接线**）；M-04/05/07/10 = `front/services` + `front/lib` + `front/dev` + `front/kernel-worker.js` = **3 627**（不变）。⚠️ **前端「测试」列的归属规则（v0.17 批新立）**：前端门禁用例住在 `front/tests/`（**不按模块目录分**），故按**被测源码文件所属的目录桶**归属——桶内文件多者胜（`front/views` + `front/app.js` + `front/index.html` + `front/style.css` → **M-01/02/03**；其余 → **M-04/05/07/10**）；该规则与每个套件文件头的自述模块**逐条一致**。据此 **M-01/02/03 = 88 条**（`frontend` 37 + `initial-render` 1 + `waveform` 13 + `disasm` 20 + **`source-view` 17** ← v0.22 批新增 15 + 检视轮修判别力补 2）、**M-04/05/07/10 = 157 条**（`contract-v2` 46 + `contract` 19 + `mock-kernel` 16 + `commandrouter` 15 + `regmem` 16 + `transport` 13 + `kernel-worker` 12 + `session` 11 + `demo-firmware` 9）= 合计 **245 条 / 41 suite**（`tools/check-front.sh` 实测）。集成测试另计：D-07 时间驱动 **31** + D-46 探针管道 **15** + 快照 DTO **15** + 接线 **13** + M-06 装载 **5** + M-18→M-19 **5** + **M-11 宿主接线 5** + **会话快照 7** + **中断端到端 6** = **102 条**（不变）。**workspace 合计 1083**（契约 lib **106** + kernel lib **861** + 集成 **102** + golden runner **14**）。⚠️ **基线对照**：`b014fed` = 98 + 801 + 11 + 102 = **1 012** ⇒ v0.21 批 **1 033**（+21）⇒ **v0.22 批 1 083（+50）**（契约 +2 / kernel lib +48 / runner ±0 / 集成 ±0）。
> ⚠️ **不归属任何一行的内核代码**：`crates/kernel/src/lib.rs`（46）+ `wasm_api.rs`（113）+ `bin/`（1 083）= **1 242 行**（`crates/kernel/src` 共 **45 492** 行 = 各模块目录 **38 159** + 内核宿主 **6 091** + 这 1 242，**分毫不差**）。

**v0.19 批的「自查 3」** = 两路检视各自的自查 2 项 + **收口轮自查 1 项**
（**F-14**：验收清单**二次对账**又找出 2 项「有正向用例、无反向变异」的验收项
⇒ 补 **K-43 / K-44**；同时**证伪** 2 个「疑似缺口」）⇒ 已闭环 ⇒ 计入合计。

**v0.11.0 后台账口径**：阻断级 **5 → 0**（3 条本轮闭环），高危 **12** 条中 **9 条闭环**（余 **D-13 / D-36 / D-37**），中低危 **23** 条中 6 条闭环，D-23 门禁缺口 **4 → 0**。检视轮再闭环 1 阻断 + 2 高危，新登记 D-24~D-35 共 12 条。**收口轮闭环 D-24（F-1 的根因）**——D-24 是 F-1 的追踪条目，闭环它**不新增**发现计数，检视轮仍是 22 项 / 3 闭环 / 19 未闭环。**收口轮 3 路检视再登记 D-36~D-40 共 5 条**（2 高危 + 1 中危 + 2 低危）。

| 口径 | 项数 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| M-14 | 16 | 14 | 2（跨模块） |
| M-15 | 23 | 22 | 1（N4 上收台账 D-14） |
| M-18 | 15 | 13 | 2（低危） |
| M-19 | 16 | 16 | 0 |
| 前端 M-01/02/03 | 20 | 20 | 0 |
| **v0.11 新增报告** | 23 | 21 | 2（契约缺口） |
| **检视轮新增** | 22 项发现 | 3（1 阻断 + 2 高危） | 19（其中 11 条登记为 D-25~D-35，8 条留各模块报告） |
| **收口轮新增** | 37 项发现 | 34 | 3（登记为 D-38 / D-39 / D-40） |
| **第 1 阶段新增** | 2 项发现 | **2**（D-45 / **D-46**） | 0 |
| **第 1 阶段检视轮新增** | 13 项发现 | 10 | 3（登记为 **D-47 / D-48 / D-49**） |
| **D-46 检视轮新增** | **25 项发现**（V-A 9 + V-C 10 + V-B 6） | **19** | 6（登记为 **D-50 / D-51 / D-52 / D-54 / D-55 / D-56**，其中 D-50 / D-51 是「已闭环但带残留」的派生欠账） |
| **D-07 检视轮新增** | **43 项发现**（V-A 13 + V-B 15 + V-C 15） | **38** | 5（登记为 **D-57 ~ D-62** 中除 D-61 外的 5 条：D-57 / D-58 / D-59 / D-60 / D-62） |
| **M+C 扩展检视轮新增** | **35 项发现**（V-A 6 + V-B 12 + V-C 17） | **30** | 5（登记为 **D-63 ~ D-67**） |
| **v0.17 契约 v2 批新增**（含文档检视轮） | **7 项**（新登记 **D-68 ~ D-74**） | **3**（D-68 / D-69 / **D-73**） | **4**（**D-70 / D-71 / D-72 / D-74**） |
| **v0.18 会话级快照批新增**（V-C 19 + V-A 6 + V-B 12） | **37 项**（新登记 **D-79 ~ D-81**） | **33** | **4**（**F-3** 采集期段 5 可能为空 · **C-5** 变异工具链的还原不逐字节校验 → D-79 · **D-1** 夹具自指常量（已披露） · **D-3** 端到端不含 ROM / 定时器 → D-80） |
| **v0.19 M-16 落地批新增**（V-A 19 + V-B 15 + 自查 4） | **38 项**（新登记 **D-83 ~ D-85**） | **34** | **4**（**D-83** 测试 / 变异工具链无超时保护 ⇒ 活锁表现为挂死（工具链侧已修，M-12 侧留账） · **D-84** `mip` 无生产者（规格已订正） · **D-85** 黄金语料对 CSR / `mret` 零覆盖 · **O-5** `SharedInterruptPort` 争用时静默降级） |
| **v0.20 M-11 落地批新增**（V-A 8 + V-B 8 + 收口自查 3 + 收口轮变异自检 1） | **20 项**（新登记 **D-86 ~ D-90**） | **16** | 4（**D-86** M-13 取指恒读 4 字节 ⇒ 末端压缩指令访客取指失败 · **D-87** 轨迹机器码可能时间倒流 · **D-88** 前端导出无下载动作 · **D-89** 裁决 G 的部分写粒度） |
| **v0.21 CSR 黄金覆盖批新增**（检视 A 9 + 检视 B 12 + 收口轮自查 2） | **23 项**（新登记 **D-91 ~ D-101**；同时闭环 **D-84 / D-85 / D-90**） | **15** | 8（**D-93** `mret` 不屏蔽 `mepc[1:0]` · **D-94** S 扩展 / PMP / 计数器缺失 · **D-95** `misa` 写语义分歧 · **D-96** 三条「空洞的一致」无**自动**检出 · **D-98** `mstatus` 无 WARL 掩码 · **D-99** 访存异常 5/7 未实现（§1.2）· **D-100** `pending_lines` 的「读不到」被当成「没有挂起」 · **D-101** `restore_full` 绕过「未实现 ⇒ 读 0」） |
| **v0.22 Wave 2 检视轮新增**（V-A 12 + V-B 8 + V-C 15 + V-D 9 + **修复 lane 自查 4**） | **48 项**（新登记 **D-104 ~ D-116**；同时闭环 **D-93 / D-98 / D-99 / D-100 / D-101** 与 **D-83** 的 M-12 生产侧） | **35** | **13**（**D-104** v5 `define_file` 传空字符串节区 ⇒ 整单元降级 · **D-105** 降级单元的地址报成 `Missed` · **D-106** 三个状态机寄存器维护但无断言 · **D-107** 「PC 未知」与「PC 就是 0」不可区分 · **D-108** `TimeReached` 在目标未到时也说谎 · **D-109** `seed` 是死字段 · **D-110** `quanta` 一词两义 · **D-111** CSR 过滤只堵还原侧 · **D-112** `misa` 让「空快照」判据变假 · **D-113** VD-F1 归一化的次序代价（≥2 件迟到事件且 `seq` 次序与时刻次序相反 ⇒ 出队次序可能变） · **D-114** M-17 `step_n` 的 `InterruptTaken` 分支有**同形状**活锁（守卫只覆盖 M-12） · **D-115** watchdog 只做了「无进展检测」这一半（宿主时间预算 / 滑动窗口三次确认 / HMS 同步终止 / `EV_WATCHDOG` 日志都未做） · **D-116** `E_ENG_03` 未进契约层场景码表） |
| **v0.23 osz 落地批新增**（Lane A 4 + Lane B 4 + Lane C 2 + 宿主接线 6 + 契约 v4 5 + 浏览器档 2 + osz 端到端 4） | **27 项**（新登记 **D-117 ~ D-121**；同时闭环 **D-47 / D-48 / D-116**） | **19** | **8**（**D-102** 契约形状已落地但内核侧还不会填 `file`/`line` ⇒ **只闭环一半** · **D-117** `PeripheralSessionState` 无 CLINT / PLIC 变体 ⇒ 这两个器件进不了会话快照 · **D-118** `UartStateDto` 缺 ns16550 的七个寄存器（LCR/IER/FCR/MCR/SCR/DLL/DLM）⇒ LCR 的 DLAB 状态不进快照 · **D-119** guest 的 UART **没有输入通路** ⇒ RTOS 的 shell 只能看不能打字（STAGE-8 的 E-8 判为未做）· **D-120** M-12 仍自写一份 `E_ENG_*` 场景码字面量，未收拢到契约层 · **D-121** `DisasmEntryDto` 的 `file`/`line` 内核侧一律填 `None` ⇒ 契约有形状、内容是空的） |
| **v0.24 遗留清账批新增**（Lane H 2 + Lane I 1 + Lane K 1 + 统筹方 1 + **门禁抓到 1**） | **5 项**（新登记 **D-122**；同时闭环 **D-102 / D-117 / D-118 / D-119 / D-120 / D-121**） | **1** | **4**（**D-122** UART 的接收 FIFO 只有 1 字节深 ⇒ 一行 shell 命令送不进去 ⇒ **本批内已加深到 16 字节**，按「闭环不回填」口径仍计入未闭环 · **前端门禁把 JS 构造器清单写死**（Lane H 新增命令 ⇒ 门禁 268 里 2 条红，Lane K 当场补清单）· **数值策略的列表形态落在 `session.rs` 而不是 `numstr.rs`**（同一约束落了两个点 ⇒ 统筹方折进 `numstr.rs`）· **终端输入框的三个 class 没有样式**（Lane K 报告 ⇒ 统筹方补样式）· **`tools/boot-osz.sh` 的构建目录与运行目录不一致**（build 认 `CARGO_TARGET_DIR`、运行写死 `$REPO_ROOT/target` ⇒ **门禁当场抓到**，真缺陷）） |
| **v0.25 预登记（本轮架构复盘 · 文档批）** | **1 项**（新登记 **D-123**） | **0** | **1**（**D-123** 内存图是宿主**硬编码的两档**，换平台要改宿主代码；⚠️ **根因是 RAM 急分配**（`m14_bus/mod.rs:752` 的 `vec![0u8; size]`）而不是内存图形状的必然 ⇒ 通用化按 **C 做法**：**A** RAM 页级懒分配 + **B** 内存图由工程/平台描述 + **C** RAM 区域按需增删；⚠️ **实测代价**：QemuVirt 档一次会话快照 = **258.06 MiB** JSON（RAM 段 129.00 MiB，**几乎全是零**）⇒ 懒分配能把载荷砍掉两个数量级；⚠️ **region 登记与页懒分配必须分层**（没登记 ⇒ 架构陷阱 cause 5/7（D-99 的判别力）；登记了但没写过 ⇒ 读回 0）） |
| **合计** | **496** | **398** | **98** |

> **口径**（V-C 检视 · F-15）：本表数的是**检视发现**（finding），**不是** D-xx 条目数。
> D-07 于 v0.15 由「◐ 推进中」转「✅ 已闭环」——它在 §1.2 里的**归属**是
> 「M-12 ↔ M-17」，所以 §0.1 的 M-12 行从「台账 3 项」改为「台账 4 项」。
> **v0.17 契约 v2 批已重算并新增一行**（数字全部**从当前树重算**，见上方「统计口径」）；
> **v0.20 M-11 批再次重算并新增一行**（第五次重算 · 新增 **M-11** 行）。
> ⚠️ **历史各轮仍按「发现即记、闭环不回填」记账**（见下方「两级计数的口径」）——
> 所以合计的「未闭环 **81**」**仍然计入**已于 v0.17 批闭环、但**未回填**历史行的 **6 条**
> （**D-32 / D-34 / D-39 / D-40 / D-57 / D-58**），**以及** v0.21 批闭环、但**未回填**
> v0.19 / v0.20 两行的 **3 条**（**D-84 / D-85 / D-90**），**以及** v0.22 批闭环、但**未回填**
> v0.21 行的 **5 条**（**D-93 / D-98 / D-99 / D-100 / D-101**），**以及** v0.22 批闭环、但**未回填**
> v0.19 行的 **1 条**（**D-83** 的 M-12 生产侧 —— 无进展守卫落地后闭环）。
> **v0.23 批**又闭环 **3 条**但**未回填**历史行：**D-47**（帧 `seq` 双序列空间）· **D-48**（`E_TRX-04` 同码多义）· **D-116**（`E_ENG_03` 未进契约层场景码表）——它们分别在 v0.23 Wave 2 的 Lane D 里闭环。
> **真实未闭环数 = 98 − 6 − 3 − 5 − 1 − 3 − 6 − 4 = 70**（v0.25 预登记的 **D-123** 是**新登记、尚未闭环** ⇒ 直接计入未闭环，不参与「闭环不回填」的减法）。
> **v0.24 批**又闭环 **6 条**但**未回填**历史行：**D-102**（反汇编逐条行号 · 契约形状 v0.23 已定，内核侧 v0.24 才真的填）· **D-117**（CLINT / PLIC 进不了会话快照）· **D-118**（`UartStateDto` 缺 ns16550 的七个寄存器）· **D-119**（guest 的 UART 没有输入通路 ⇒ shell 打不了字）· **D-120**（M-12 自写一份 `E_ENG_*` 字面量）· **D-121**（`DisasmEntryDto` 的 `file` / `line` 内核侧一律填 `None`）——它们分别在 v0.24 的 Lane J / I / H / K 里闭环。
> ⚠️ **v0.24 批新登记的 D-122 也在未闭环格里**（UART 的接收 FIFO 只有 1 字节深 ⇒ 一行 shell 命令送不进去）——它**本批内已闭环**（Lane I 把 FIFO 加深到 16 字节），但按「**闭环不回填**」的记账口径仍计入未闭环。
> ⚠️ **v0.23 批的浏览器档**又找出 2 项并当场闭环：前端终端视图的 **`termDelta` 没有任何生产者**（终端视图是个**死接口**，契约 v4 的 Console 事件到了前端却没有落点）· 字节数组转字符串**不分块**会爆调用栈（osz 的镜像就有 28 万字节级别的载荷）⇒ 两条都记在本行的「检视发现」里，**不另立 D-xx**（当场发现当场闭环，按口径计入未闭环）。
> ⚠️ **口径不变**：本表数的是**检视发现**，一个 finding 被闭环后若仍有剩余工作量，就登记为新的 D-xx，**不回填本表的未闭环格** ⇒ v0.23 批当场发现当场闭环的 **14 条**（`mcause` 架构码 · `wfi` 非法 · `mhartid` 未实现 · UART 布局不兼容 ns16550 · LSR bit5 语义 · UART 无 TX 捕获环 · 无 CLINT · 无 PLIC · 128 MiB 内存图缺失 · 三个外设未映射 · CLINT↔M-16 未接通 · PLIC 未抬外部线 · console 互递归栈溢出 · 契约层无 Console 事件）**仍计入未闭环**。
> （**D-32 / D-34 / D-39 / D-40 / D-57 / D-58**），**以及** v0.21 批闭环、但**未回填**
> v0.19 / v0.20 两行的 **3 条**（**D-84 / D-85 / D-90**）。**真实未闭环数 = 72 − 6 − 3 = 63**。
> 本批**未**回溯逐条重算更早各轮（那要把 392 条发现逐条回源码核对，属下一轮检视轮的活）。

> **两级计数的口径**：本表数的是**检视发现**（finding），不是台账条目（D-xx）。一个 finding 被闭环后，若它指向的**剩余工作量**需要单独立项，就登记为新的 D-xx，**不回填本表的未闭环格**。收口轮即此情形：**D-36 / D-37 / D-41** 是 V-A 🟠-1 与 V-C H-1 两条**已闭环但带残留**的发现所派生的工作量单——前者（措辞收窄 / LLD 同步）已闭环，后者（外设与 INTC 状态序列化、M-20/M-23 接线、M-13/M-14/M-15 状态导出接口）单独立账。
> 因此「高危 12 条中 9 条闭环」与「合计未闭环 35」**不矛盾**：前者是 §三 表的条目口径，后者是本表的发现口径。

### 0.2 跨模块链路连通性


```
M-14 Bus ──bus hook──▶ M-18 ProbeRegistry ──EdgeSink──▶ M-19 WaveformPipeline ──▶ CSV/VCD
   ✅ 已通                  ✅ 已通                        ✅ 已通
      │                         ▲
      │ 共享句柄（BoundPeripheral）│ on_probe（OUT / TX / CMP）
      ▼                         │
M-15 外设 ───────────────────────┘
   ✅ 已通（v0.11 接线 PR：三外设 override on_probe + ProbeRegistry::attach_signal）
```

> **v0.11.0：三处断点全部接上。** M-14 → M-15（总线派发到外设实例 + size 一致性校验）、M-15 → M-18（三外设 override `on_probe`）、M-18 → M-19（v0.9 已有）**构成完整闭环**。
> **N5「GPIO 翻转可见」的内核路径 100% 闭合**——`crates/kernel/tests/m14_m15_m19_wiring.rs` 的 13 个端到端用例为证。
> **观测管道现在有两条通路**到达 M-19：「总线访存」与「外设信号」，不再只有前者。

> **D-46 已闭环（`feat/d46-probe-pipeline`）：上面这张图此前只在**测试里**是通的**——生产代码没有任何一处把 M-18 挂到 M-14 的总线上，所以波形视图是空的。现在内核宿主（装配层）真的装上了：
>
> ```
> 内核宿主 ──add_hook──▶ M-14 Bus（与 M-17 的观察点 hook **共存**）
>           ──attach_signal──▶ M-15 外设（探针回调）
>           ──EdgeSink──▶ M-19 管道 ──drain──▶ 契约 WaveformHistory ──▶ 前端波形视图
> ```
>
> 证据：`crates/kernel/tests/d46_probe_pipeline.rs` **15 条** + 前端 `front/tests/waveform.test.mjs` **9 条** + `tools/wasm-smoke.mjs` 对**真 wasm 产物**的 5 项断言 + `tools/browser-verify.mjs` 在**真 Chrome** 里的 2 项断言。

> **v0.18：会话级快照链路（落盘一个会话 → 还原出等价会话）**——
>
> ```
> M-14 Bus  ──export_map / export_pages──▶ 段 3 m14_map / 段 2 m13_state
> M-15 外设 ──collect_session_state──────▶ 段 4 m15_periph
> M-16 INTC ──export_state───────────────▶ 段 5 m16_intc
> M-12/M-17 ─snapshot_dto────────────────▶ 段 1 m12_engine
> 宿主 ──capture_session──▶ M-20 capture_session ──record──▶ M-23 put_snapshot（落盘前先 validate）
>                                                              │
>                                          restore_session ◀─────┘
>                     段 0..5 ──▶ 宿主按 §3.7 顺序回灌（map → pages → periph → intc → engine）
> ```
>
> **核心命题**：**落盘一个会话的完整状态，还原出的是一个「等价会话」**——不只是「还原了一个引擎」。
> 证据：`crates/kernel/tests/session_snapshot.rs` **7 条**端到端（含**还原进全新会话**的变体，
> 证明载荷**自足**）+ `crates/kernel/src/kernel_host/tests_session_snapshot.rs` **12 条**宿主层。

> **v0.20：M-11 调试面链路（符号 / 反汇编 / 源码行 / 轨迹导出）**——
>
> ```
> M-06 ELF ──debug_sections（原始字节，不过契约层）──▶ M-11 符号表 / DWARF 行表
> M-13 Decoder ──classify──▶ M-11 render（助记符）──▶ M-11 disasm（按地址缓存）
> M-14 Bus ──read_only（不触发 hook）──▶ M-11 取指回调 ──▶ 契约 Disasm 事件
> M-18 访存 trace 环 ──drain + 转移（同时发生）──▶ M-11 轨迹环 ──export──▶ 契约 TraceExport
> M-19 波形环 ──export_csv / export_vcd──▶ 契约 TraceExport（宿主路由，**不经过 M-11**）
> ```
>
> **核心命题**：**「接上了」和「钉住了」是两件事**——本批同时抓出两种失配：
> 「接缝存在但没人用」（D-50 / D-51 / `SymbolService` 端口）与
> 「接上了但没被钉住」（`Waveform+Vcd` 形状 / `overflow_marked` 传播 ⇒ **795+5 全绿**）。
> 证据：`crates/kernel/src/kernel_host/tests_m11.rs` **28 条** + `crates/kernel/tests/m11_host_wiring.rs` **5 条**
> + M-11 单元 **75 条** + `tools/wasm-smoke.mjs` 对**真 wasm 产物**的 **13 项**断言
> + `front/tests/disasm.test.mjs` **20 条**。

---

### 0.3 正确性对拍链路（第 2 阶段打通）

```
riscv-tests 源（仓库外）──riscv-none-elf-gcc──▶ 同一个 ELF
                                                    │
                    ┌───────────────────────────────┴───────────────────────────────┐
                    ▼                                                               ▼
        ┌───────────────────────────┐                          ┌───────────────────────────┐
        │ Spike（外部 oracle）      │                          │ riscv-golden（我们的候选）   │
        │ --isa=rv32imc_zicclsm_   │                          │ M-06 加载 → M-14 总线      │
        │      zifencei            │                          │        → M-13 ISS          │
        └───────────┬───────────────┘                          └───────────┬───────────────┘
                    │ 读 tohost，判定 PASS / FAIL <n>                        │ 同一条判定
                    └───────────────────────────────┬───────────────────────┘
                                                    ▼
                                       tools/golden-run.sh 比对判决
                                       （FAIL 必须用例号也一致）
```

> **2026-10-21 实测：语料 51/51 全通**（原 42/42 + M 扩展 8 例 + C 扩展 1 例，**D-43 已闭环**）。
> **自检**：`tools/golden-run.sh --self-check` 跑两个变异金丝雀（自包含 `fail_3.S` + 上游 `add.S` 的期望值篡改版），两侧**都**必须报 `FAIL 3`——否则判定「对拍不具判别力」并退出非零。
> **口径与理由**见 `tests/golden/README.md`（含 **Zicclsm** 这个必须钉死的 ISA 细节，见 **D-42**）。

---

## §1 欠账明细

### 1.1 阻断级

| ID | 归属模块 | 来源 | 问题 | 实证依据 | 阻塞关系 |
| --- | --- | --- | --- | --- | --- |
| **D-01** | 前端 `m02_viewcomponents.js` ← `m01_workbench.js` | 首次全量审查 P0-1 | `m02:19` 导入 `activeView`，`m01:126` 的 `let activeView` **没有 export** → ES module 链接期失败 | ✅ **已闭环**（`fc83e1e`）：m02 删掉用不到的 `activeView` import；`node --test front/tests/` 链接期无错 | ~~阻塞 D-02~~（已解除） |
| **D-02** | 前端 `app.js` ← `m03_viewstore.js` | 首次全量审查 P0-1 | `app.js:10` 导入 `mountViewStore`，但 `m03` 只导出 `setState/getState/snapshot/subscribe/reset` | ✅ **已闭环**（`fc83e1e`）：m03 补 `mountViewStore`，写法对齐四个 service | 已解除 |
| **D-03** | M-06 `hex.rs` | 首次全量审查 P0-2 | `:112` 错误格式化里 `bytes.len() - 5` 在 `bytes.len() == 4` 时整数下溢 panic | ✅ **已闭环**（v0.11，commit `34cc38b`）：改为饱和减法；**反向验证**——把修复改回去，`tests_malformed.rs` 的表驱动 + fuzz 用例双双 panic 失败 | 已解除；并补上 D-23 门禁第 2 项（畸形输入表驱动 + 固定种子 fuzz） |
| **D-04** | M-13 ISS | 首次全量审查 P0-3 | **x0 未硬连线为 0**：`executor.rs:97` 与 `mod.rs:161` 均裸写 `state.gpr[d.rd as usize] = result`，读侧 `state.gpr[d.rs1 as usize]` 无保护 | ✅ **已闭环**（v0.11，commit `71a3b05`）：引入 `read_gpr` / `write_gpr`，写侧 `rd == 0` 丢弃写入、读侧恒返回 0，**全模块无旁路**（T1 自查确认只有这两个函数碰寄存器文件） | 已解除；**Q7 Spike 对拍的前置条件已清**——工具链亦已装好（Spike 1.1.1-dev + xpack GCC 15.2.0），见 PROGRESS v0.11.0 |
| **D-05** | M-12 引擎 | 首次全量审查 | `snapshot()` 只返回 `SimState`，`restore()` **不还原 `now_ps`、不处理事件队列** | ✅ **已闭环**（v0.11，commit `86dfa4b`）：`state.virtual_time_ps` 成为时间唯一真相（契约里本就为这个留的字段），`EngineSnapshot` 在 m12 内部承载队列完整快照，新增 `snapshot_full` / `restore_full`；**旧签名原样保留**，`m17_backend` 零改动继续编译 | 已解除；D-23 门禁第 4 项（snapshot→restore→继续推进的确定性断言）同步补上 |

> **前端 D-01 / D-02 的关键实测**：在 `/tmp` 副本上**仅补这两处导出**（约 3 行），bootstrap 即完全跑通（最小 DOM stub 验证）——**这是前端启动路径上的全部阻断点**，修完没有隐藏的链接期错误。

---

### 1.2 跨模块高危

| ID | 归属模块 | 来源 | 问题 | 阻塞关系 / 备注 |
| --- | --- | --- | --- | --- |
| **D-06** | M-12 | 首次全量审查 + M-15 报告 #6 | `run_until_idle` 每个事件耗 1µs 量子；事件在 t=1e12 ps 即空转 1e9 次 | ✅ **已闭环**（v0.11，commit `1723610`）：改为「空闲区间批量推进」——时钟直接跳到下一个到期事件，迭代次数从 O(时间/量子) 降到 **O(事件数)**。性能基线测试断言 1e12 ps 跨度 64 事件 → **步数恰为 64**、步数 < 1e9、Tick 数 == 事件数 |
| **D-07** | M-12 ↔ M-17 | 首次全量审查 + M-15 报告 #6 | `dispatch` 的 `PeripheralTick` 分支是空 TODO | ✅ **已闭环**（v0.15，`feat/d07-time-driven-scheduler`）。⚠️ **台账原估计偏小**：原写「剩最后一步——M-12 的 `dispatch` 真正调用它」，实测发现整条链路有**四处断口**，缺一不可。**四处断口 → 各自的闭环**：① **没人装 ticker**（`Engine::set_peripheral_ticker` 在生产代码里**从无调用**）→ M-17 新增 `install_peripheral_ticker()`（从 `Bus::peripheral_instances()` 取实例 → 按物理地址 tick 单个外设的回调），**宿主**在 `KernelHost::new()` 与 `rebuild_platform()` 两处装上（只有宿主知道它挂了哪些外设）；② **没人排 `PeripheralTick` 事件**（事件队列里**永远没有**外设 tick 事件）→ M-12 主循环在每个量子边界**确保**每个登记外设都有一件 tick 在等着（`ensure_peripheral_ticks`，**自愈**：事件被弄丢也不会永久停摆），派发后**续排**到下一个量子边界；③ **`pending_writes` 只写不读**（外设 tick 返回的写**永远落不到总线上**）→ M-17 的执行器在每条指令之后把队列抽干并**真正写进总线**（失败的那一条**留在队列里**，不静默丢）；④ **时间驱动主循环无人调用**（LLD 的唯一主循环 `runUntil` **从不被执行**）→ M-12 新增 `Engine::run_until(executor, until_ps)`（LLD §3.1 结构），M-17 的 `continue_until_event()` 与宿主的 `Continue` 都走它。**「指令驱动与时间驱动如何交替」的裁定**：时间驱动是**骨架**（事件队列 + 量子边界），指令驱动是量子内的**填充物**（每量子至多 1000 条指令），二者在**指令边界**交汇——每执行一条指令就问一次「有没有事件到期」，到期就抢占（安全检查点 = 量子边界 ∨ 指令边界 ∨ 事件边界）。**两条路径共用同一个执行器**（`m17_backend::IssExecutor`），没有第二套 ISS 逻辑会漂移。⚠️ **口径订正**：`TIME_QUANTUM_PS` 原写 `1_000`（1 ns）却注释「1 µs」——**差 1000 倍**，与「每量子 1000 条指令」不自洽；现为 `1_000_000` ps，并由 `QUANTUM_INSTR_QUOTA`(1000) × `INSTRUCTION_COST_PS`(1 ns) 闭合换算。⚠️ **接缝不是契约端口**：契约层冻结（新增端口要等 `schema_version=2`，台账 D-32），故 `InstructionExecutor` 与 `PeripheralTicker` 均为 **M-12 内部接缝**，由 M-17 实现——依赖方向单向（M-12 **不依赖** M-13，`crates/kernel/src/m12_engine/**` 无 `m13` 引用），M-12 LLD §9.1.1 已补该行；假执行器（`CountingExecutor`）在**集成测试** `crates/kernel/tests/d07_time_driven.rs` 里。⚠️ **一处行为订正**：第 1 阶段 `continue_until_event()` 只排空事件队列、**不执行指令**（队列恒空 → 等价于空操作）；现在它真的跑代码（LLD §3.1 `runUntil` 的本意）。**检视轮（2026-10-21，`feat/d07-time-driven-scheduler`）**：三路独立检视（V-A Rust 内核 / V-B 测试判别力与变异方法论 / V-C 文档一致性）共查出 **43 项**（V-A 13 + V-B 15 + V-C 15），判定 **PARTIAL / FAIL / FAIL**；**38 项已在本分支闭环，5 项留账**（D-57 / D-58 / D-59 / D-60 / D-62）。本轮最要紧的两条：**V-A 🟠-1（同一外设在同一虚拟时刻被 tick 两次）**——量子边界的事件与本量子第一条指令的指令边界同步各 tick 一次，于是两条驱动路径的外设时钟**对不上**（`StepN` 10000 次 vs `Continue` 10009 次）；闭环办法是**量子边界的事件代劳本量子第一条指令的推进**（指令边界跳过刚 tick 过的外设），两条路径的推进序列从此**逐条相同**——这成了新的不变量，已用一条用例钉住。**V-B 🟠-1（断口 3 的判别式在两条生产路径之间冗余）**——夹具每次都吐同一条写，两条路径拿到的是同一份数据，删掉任一条路径的「写收集」分支用例照样全绿；闭环办法是让夹具**只在某个时刻吐写**，把两条路径**分开**。另有一条**本轮自查发现并修掉的真缺陷**：F5 的预检式修法（`now + 指令成本 > until_ps`）会让外层循环**永远空转**（内层一条也不执行 → 外层再转一圈还是同样的结果）——已补「不许空转」的收手。**证据**：`crates/kernel/tests/d07_time_driven.rs` **31 条**端到端用例（四处断口逐条钉住 + 量子↔指令换算 + 抢占 + 断点 + 两条路径逐条一致 + 生命周期不变量）；反向变异自检 **25 条**全部按期望变红（**能编译**才算判别力，期望集合用 **AND 语义**逐条核对）。|
| **D-08** | M-06 `hex.rs` | 首次全量审查 | `finalise` 超 `segment_count` 时 `truncate()` **静默丢固件数据**且不报错 | ✅ **已闭环**（v0.11，commit `e7fb3e0`）：改 `Result` + 报错。⚠️ T3 复核后定性为「防御性收口」——当前控制流下 `truncate` 实际不可达（循环内长度检查先于 EOF 早退拦截），但路径已封死 |
| **D-09** | M-06 `elf.rs` | 首次全量审查 | `read_u16`/`read_u32` 为 `panic!`，而上方注释承诺「returns PlatformError(E_LDR-01)」 | ✅ **已闭环**（v0.11，commit `0258a34`）：改返回 `Result` 并与注释一致，调用点同步改为错误传播，移除 `.expect` |
| **D-10** | **M-06 + M-14 联合** | M-14 报告 #6 | `ParseLimits` 无 `total_image_bytes` / `max_region_bytes`；M-14 `prepare_bytes` 裸 `vec![0u8; size as usize]`，`size` 最大可达 ~4 GiB | ✅ **已闭环（两侧齐备）**：M-06 侧（v0.11 `374d647`）—— `ParseLimits` 新增 `total_image_bytes`（默认 64 MiB）/ `max_region_bytes`（默认 16 MiB）并在解析时校验；M-14 侧（v0.11 `60dbbb8`）—— `prepare_bytes` 引入**区域大小上限**，超出返 E_BUS-09，**不先分配后检查**，`map_ram` / `map_region` 两条分配路径均受保护。84 字节畸形 ELF 已无法申请 GiB 级内存 |
| **D-11** | M-13 ISS | 首次全量审查 | `BusExecutor` 复制 `Executor` 的 ADDI 逻辑，`bus` 字段挂 `#[allow(dead_code)]` 从未被读 | ✅ **已闭环**（v0.11，commit `e068295`）：改为 `BusMemory`（只做访存适配）+ 共享的 `MemoryAccess` 端口，**指令语义收敛到唯一一条 `execute_one()`**；两条路径的非法指令语义统一为 `Trapped { IllegalInstruction }`（回归测试断言两路一致）。LOAD/STORE 另有专门测试证明**必经 M-14 总线**（否则会静默丢掉 stats 与 M-18 hook） |
| **D-12** | M-14 ↔ M-15 ↔ M-18 接线 | M-14 报告 #13 / M-15 报告 #11 | `Bus::map_peripheral(base, size, PeripheralId)` 的 size 与 `Peripheral::size()` 无一致性校验 | ✅ **已闭环**（v0.11 接线 PR，3 个 commit）：① 总线经 `Rc<RefCell<dyn Peripheral>>` **共享句柄**持有外设实例（`BoundPeripheral` + `RegionTarget::PeripheralBound`），新增 `Bus::map_peripheral_instance`，挂载时校验 `size == periph.size()` 不一致返 **E_BUS-08**；② M-15 注册表改共享句柄（`handle()` / `share()`），三外设 override `on_probe` 上报 `OUT` / `TX` / `CMP` 信号；③ M-18 新增 `ProbeRegistry::attach_signal`（LLD §1.3 的 `attachSignal`）+ `CH_UART_TX = 4`。**N5「GPIO 翻转可见」的内核路径 100% 闭合**——`crates/kernel/tests/m14_m15_m19_wiring.rs` 13 个端到端用例为证（固件写 GPIO OUT → M-14 派发 → M-15 上报 → M-18 判沿 → M-19 波形通道出现期望边沿） |
| **D-13** | M-14 | M-14 报告 #9 | `Region.target` / `RamBackend.bytes` 仍为 `pub` | ⏳ **未动**：收紧可见性要同步改 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable`——后者与 v0.11 的 T4 前端服务层改动相邻，**留 Wave 2 一并做**避免来回冲突 |
| **D-14** | 契约 + M-14 | M-15 报告 N4 | `Peripheral::read_clear` 默认实现委托 `read`，**漏覆写的外设静默获得非破坏性读** | ✅ **已闭环**（v0.11 `22107a0`）：总线读外设**一律调 `read_clear`**（不是 `read`），`Uart16550` 正确 override（读 RX 后清空 FIFO 状态）。回归测试：连续读两次 UART RX，第二次不再拿到同一字节；M-14 侧另有 `read_clear` 路径计数用例证明确实走了破坏性读 |
| **D-24** | 契约 + M-12 + M-17 | 检视 V2 · F-1 | **`ExecutionBackend::restore` 不是确定性还原**：`SimState` 不含事件队列（契约无此空间），`Engine::restore` 只还原平台状态与虚拟时钟，队列原样保留 | ✅ **已闭环（收口轮，commit `3016de0`）——根因已除**。检视 V2 实证：`schedule A; snapshot; schedule B; restore; run` → **A 与 B 都触发**，而从快照时刻出发的运行只有 A。**处置**（收口轮 `3016de0` / 自证修复 `560226c` / 检视闭环 `3893626`）：① 契约层新增 `crates/contract/src/snapshot.rs`（`EngineSnapshotDto` / `QueuedEventDto` / `SchedEventDto`，全部可序列化 + `validate()`）；② `ExecutionBackend` 增 `snapshot_full` / `restore_full`，**additive 默认实现**，两端**对称地大声失败**（`E_SNAP-03`）——不用软性的「没有」以免调用方的 `if let Some(..)` 分支静默跳过还原；③ M-12 新增 `snapshot.rs` 做 `EngineSnapshot ↔ EngineSnapshotDto` 双向转换 + `Engine::snapshot_dto` / `restore_dto`（**先校验后应用**）；④ M-17 `SelfIssBackend` 接线完成，并删掉那个**写而不读**的 `state` 死字段。**partial 的 `snapshot` / `restore` 保持原样**（仍如实标注为非确定性），公开 API 只增不减。**证据**：`cargo test --workspace` 375 → **406**；D-24 原始复现有两条用例分别钉住 partial 路径的发散与 full 路径的不发散；收口轮的 5 项 + 检视后补的 8 项变异自检**全部变红**。⚠️ **意义必须限定**：该 DTO 只覆盖 **M-12/M-17 段（引擎）**——外设（M-15）/ 中断控制器（M-16）/ 内存映射**不在**其中，**落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 的 `Snapshot{cpMeta, m13State, m14Map, m15Periph}` 组装**（→ **D-36 / D-37**） |
| **D-36** | 契约 + M-15 + M-16 | 收口轮检视 V-A · H-1 | **快照 DTO 不含外设与中断控制器状态**：`EngineSnapshotDto` 只覆盖 M-12/M-17 段（队列 + 平台状态 + 插入序 + 虚拟时间）。UART 的 RX/TX FIFO 与 STATUS、定时器的 `mtime`/`mtimecmp`、GPIO 的方向/输出锁存、已拉起的 IRQ 行**全都不在快照里** | ✅ **已闭环（2026-10-21 · 会话级快照载荷批）**：M-15 补齐**导出 / 载入**接口（`session_state()` / `restore_session_state()` + 自由函数 `collect_session_state` / `restore_session_states`）——UART 的 TX/RX FIFO + STATUS + IRQ 线 + `now_ps`、GPIO 的方向 / 输出 / 输入锁存 + **逐 pin IRQ 线** + 粘滞 pending-IRQ + `now_ps`、定时器的 `mtime` / `mtimecmp` **逐字段**覆盖（唯粘滞 `pending_irq` **不在载荷里**——契约 DTO 无此字段，恢复后靠下一次 tick 按 `mtimecmp` 重新拉起，**已在 M-15 LLD §9.4 披露**）；M-16 补 `export_state()` / `load_state()`（**升序去重** / **整体替换**语义）。⚠️ **「一个约束一个落点」**：`GPIO_PIN_COUNT` 由**契约层**导出，M-15 的 `PIN_COUNT` 改为**引用**它（不写第二遍字面量 32） |
| **D-37** | 契约 + M-20 + M-23 | 收口轮检视 V-C · H-1 | **契约 DTO 与 M-20/M-23 已冻结的 `Snapshot` 结构不一致，且 M-20/M-23 未接线**：M-20/M-23 LLD §2.1.1（评审 20-1 / 23-1）定义的是 `Snapshot{cpMeta, m13State, m14Map, m15Periph}`；契约 DTO 是 `{schema_version, state, queue, next_seq, instruction_budget}`。`SnapshotRecord` 至今**无 serde derive**，M-23 无法直接序列化它 | ✅ **已闭环（2026-10-21 · 会话级快照载荷批）**。**裁决沿用 2026-10-20 的口径**（以详细设计 §2.1.1 为准），但**实现侧按冻结规格 `STAGE-3_Session_Snapshot.md` 落成六段**（§1.2 裁决 A/B/C）：契约层新增 `SessionSnapshotDto { schema_version, cp_meta, m12_engine, m13_state, m14_map, m15_periph, m16_intc }`（**段 0~5**，`schema_version` 不编号）；M-20 `capture_session()` 按六段组装 + `record()` 落记录（**`SnapshotRecord` 形状 = `{id, payload, label}`**——删掉第二份 `cp_meta`，**一量两处**；`id` = 记录序号 vs `payload.cp_meta.id` = 检查点 id 是**两个不同的量**），`SnapshotRecord` **有 serde derive**；M-23 `DataStore::put_snapshot` / `get_snapshot` 改吃 `SessionSnapshotDto`（**写入前先 `validate()`**），新增 `InMemoryDataStore`（键 = `cp_meta.id`）。⚠️ **M-20 的 `restore_session` 改名 `validate_session`**（**只校验、不还原**）——宿主 `restore_session`（真恢复）保持原名：**同名不同义 = 「一码多义」的变体**；加源码级守卫 `the_manager_has_no_restore_session_method` 防回退 |
| **D-45** | 前端 M-10 + `app.js` ↔ 内核宿主 | 第 1 阶段真机复验 | **装载工程后内核侧订阅静默失效，寄存器推送停掉、视图停在旧值、不报任何错**：`LoadProject` 清空内核侧的主题集合（新工程 = 新会话，FR-13），而宿主本地的「已订阅」标志还立着 → 幂等的 `ensureSubscribed()` 什么都不发 | ✅ **已闭环**（第 1 阶段）：① M-10 新增 `resubscribe()`（**强制**重发订阅，无视本地标志）与 `loadProject(project)`——**「装载 + 强制重新订阅」绑成一个动作**，不变量的归属落在服务而不是调用点的注释里；② `app.js` 的启动流程与「reset」动作都改走 `registerMemory.loadProject(...)`；③ 前端门禁加 2 条回归用例（`regmem.test.mjs`），**反向验证**：把 `resubscribe()` 改成空操作 → 2 条变红；把 `loadProject()` 的重新订阅删掉 → 1 条变红。**这个缺陷在 mock 内核时代就存在**（mock 的 `reset()` 同样清主题），是第 1 阶段接上**真内核** + 真机断言「x1 必须是 42」才暴露的——**PC 会前进、x1 不动**。⚠️ 教训：真机断言必须断言**值**，只断言「内核已就绪」抓不到这一类 |
| **D-46** | M-18 / M-19 ↔ M-14 ↔ 前端 | 第 1 阶段已知限制 | **波形视图在第 1 阶段是空的**：真实探针管道（M-18 `ProbeRegistry` + M-19 `WaveformPipeline`）**没有接到总线的 bus hook 上**，所以 `sim.waveform` 订阅下不产生任何探针流 | ✅ **已闭环**（`feat/d46-probe-pipeline`）。**四根线一次接上**：① **M-14 加 `add_hook`（追加式）**——`set_hook` 保留「替换全部」语义，`add_hook` 是它的 additive 孪生；M-17 的观察点 hook 改用 `add_hook`，**后装的探针 hook 不再把观察点 hook 顶掉**（这是接线的前提：两个消费者都得看见每一次访存）；② **M-14 增 `peripheral_instances()`**——装配层据此给每个外设实例挂上 M-18 的探针回调（`attach_signal`）；③ **M-17 增 `Bus::tick_peripherals(now_ps)`** 并在**每条指令**前同步一次外设时钟——M-15 的 GPIO 在 `tick` 里记权威虚拟时间，边沿才带得上**真实时间戳**（否则时间轴恒为 0，等于半个假象）；④ **内核宿主装管道**——`ProbeRegistry`（判沿 + 访存 trace 环）→ `EdgeSink` → `WaveformPipeline`（环形缓冲 + 导出）→ 契约 `EventPayload::WaveformHistory`（v2 改名，原 `WaveformDelta`）。⚠️ **契约事件取自 M-18 自己的边沿存储**；M-19 的环**只写不读**（CSV/VCD 导出还没接进契约 → 台账 **D-50**）；宿主默认内存图**新增 GPIO 外设**（`0x4000_0000`，PL061 惯例）——**没有外设就没有信号可观察**。**前端**：订阅内核侧 `sim.waveform`（**强制**重订，`LoadProject` 会清主题集合，与 D-45 同一个坑）→ 落进 M-03 视图状态 → M-02 波形视图渲染通道列表；过滤判别式是「**不是** `cpu.` 开头的寄存器流」，**不是**「是 `probe.` 前缀」——真内核的通道 id 是 `"{外设 id}.{信号名}"`（`GPIO0.OUT`），写成只认 `probe.` 会把真通道**全部过滤掉**；边沿计数**累加**（内核每次只推增量，覆盖的话视图上永远只有最后一批）。**demo 固件真的驱动 GPIO**（0x14..0x1c：`lui x5, 0x40000` / `addi x6, x0, 1` / `sw x6, 4(x5)`）——否则管道通了也没有边沿可推。**证据**：`crates/kernel/tests/d46_probe_pipeline.rs` **15 条**端到端用例（通道列表 / 真实边沿 / 时间戳 / `LoadProject` 后管道仍在 / 观察点与探针 hook **共存** / 两份 demo 固件防漂移）；前端 `front/tests/waveform.test.mjs` **9 条**；`tools/wasm-smoke.mjs` 对**真 wasm 产物**断言探针流（**+5 项**：通道列表 / 刚订阅时无边沿 / 真实边沿 / 边沿值 / 边沿时间戳 ≠ 0）；`tools/browser-verify.mjs` 在**真 Chrome**里断言波形视图列出 `GPIO0.OUT` **且带边沿数**（不是「非空」）。**检视轮（2026-10-21，`feat/d46-probe-pipeline`）**：三路独立检视（V-A 内核 / V-B 前端与工具 / V-C 一致性与文档）共查出 **19 项**（V-A 9 + V-C 10）：**4 中危 + 15 低危**，V-A / V-C 判 FAIL。**全部已在本分支闭环**（其中 2 项在新一轮检视开始前就被 `d0d0b66` 修掉了），其中 **V-A 🟠-1（发布源无上限 + M-19 死代码）** 是本轮最重要的一条：见 §3.5 |
| **D-47** | 前端 M-07 ↔ 内核宿主 | 第 1 阶段检视 V-B · 中危 1 | **传输帧 `seq` 有两个序列空间**：内核事件帧用 `msg.seq`（内核自己的单调计数器），宿主合成的错误帧用 `hostSeq`（另一个从 1 起的计数器）——同一 Worker 内帧 seq 可以从 11 跳回 1 | ✅ **已闭环**（v0.23 Wave 2 · Lane D · `0c6a35d`）：`front/kernel-worker.js` 改成维护**传输层自己的单调计数器**（`nextFrameSeq`），**与内核 `Message.seq` 解耦** ⇒ 同一 Worker 内帧 seq 严格单调递增，不再出现「从 11 跳回 1」。**判别力**：新增一条「帧 seq 严格单调」的用例，Lane D 的变异 C（把帧 seq 改回 `msg.seq` + `hostSeq`）把它打红（失败形态是「第 2 帧是 1 而前一帧是 3」） |
| **D-48** | 前端 M-07 ↔ WASM 宿主 | 第 1 阶段检视 V-C · 中危 4 | **`E_TRX-04` 一个码被三种语义复用**：M-07 里表示「传输通道错误」（Worker `error` 事件 / WebSocket 错误），`front/kernel-worker.js` 里表示「WASM 宿主侧错误」（init 失败 / 内核抛错 / 返回值非法）——前端无法据码区分「通道挂了」与「内核算错了」 | ✅ **已闭环**（v0.23 Wave 2 · Lane D · `0c6a35d`）：**拆码**落地 —— 宿主侧错误（WASM 初始化失败 / 内核抛错 / 返回值非法 / 返回的不是数组 / 返回空数组）改用**新码 `E_TRX-05`**；`E_TRX-04` **只**表示「传输通道错误」（Worker `error` 事件 / WebSocket 错误），语义**收敛**。**判别力**：新增一条「宿主侧错误是 `E_TRX-05` 且**不是** `E_TRX-04`」的用例；Lane D 的变异 D（把宿主侧改回 `E_TRX-04`）打红 8 条 |
| **D-49** | 契约层（`ErrorFamily`） | 第 1 阶段检视 V-C · 高危 2 | **`E_PROTO-08` / `E_PROTO-01` 不属于契约的任何一个 `ErrorFamily` 族**：契约只有 `CTX / ISS / ENG / BUS / PER / INT / DBG / OBS / SNAP / LDR / HMS / STORE / TRX` **13 个族，没有 `PROTO`**。这两个码是实现侧硬编码的字面量（`kernel_host` + `front/lib/message.js`），以字符串形式出现在 `EventPayload::Error.code`（该字段是 `String`，无族约束） | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：本条登记时给出的二选一——「要么在契约变更流程里正式加 `Proto` 族，要么把 `E_PROTO-*` 全部并入既有的 `Transport` 族」——本批**选了第一条**。**根因**：契约层自 v0.10 起冻结，**加族是契约变更**，冻结期内做不了。**处置**：契约层 v2（**第一次**升版）新增 `ErrorFamily::Protocol`，`as_str()` → **`"PROTO"`**（与 LLD 的 `E_PROTO-0x` **逐字一致**），LLD 的 `E_PROTO-01`…`E_PROTO-08` **从此在契约侧有族可归**（D-32 缺口 5 同一件事）。⚠️ **不并入 `Transport`（`"TRX"`）**：`TRX` 是**传输适配器自身**的故障（连接 / 断线 / 超时），`PROTO` 是**协议层**的故障（版本不协商 / 信封畸形 / CRC 不符）——合并会让「对端没接上」与「对端接上了但说的不是人话」落进同一个码，前端无法据此决定是**重连**还是**报错**。连带影响一并消解：`port.rs` / `model.rs` 注释里引用的 `E_PROTO-05`（M-07 `resumeBySeq`）**不再查无此族**。**证据**：`crates/contract/src/error.rs`（`ErrorFamily::Protocol` + `as_str()` → `"PROTO"`；族数 13 → 14）+ 用例 `the_protocol_family_is_spelled_like_the_lld`（族名拼写）与 `the_transport_and_protocol_families_are_distinct`（**两族不合并**）。**判别力**：**两条**用例一起钉住——`the_protocol_family_is_spelled_like_the_lld` 钉「拼写逐字一致」，`the_transport_and_protocol_families_are_distinct` 钉「`TRX` 与 `PROTO` 是**两个不同的族**」；后者防的是把 `Protocol` 写成 `Transport` 的别名（或让 `as_str()` 复用同一个串）|
| **D-99** | M-13 ↔ M-14 | v0.21 CSR 黄金覆盖批 · 检视 B · F-4 | **未映射访存不是陷阱，而是总线错误**（原描述保留：M-13 LLD §5.2.1 明写「访存异常 5/7」⇒ 实现没做 ⇒ guest 的 trap handler 根本没机会跑） | ✅ **已闭环（v0.22 · Wave 2 · `55fd0c2`）**：**三处改动**。① **M-13 侧新增两个陷阱变体**：`TrapCause::LoadAccessFault`（`mcause = 5`）/ `TrapCause::StoreAccessFault`（`mcause = 7`），落在 `crates/kernel/src/m13_iss/executor.rs`（变体 `:86` / `:93`，`mcause()` 映射 `:152` / `:154`，取陷阱点 `:956` / `:991`）⇒ 访存未映射时 **PC 不前进**、`mepc` = 出错指令地址、`mtval` = **故障地址**。② **判据落在 family + scenario，不靠 message 文本**：`is_unmapped_access(err)` = `err.family == ErrorFamily::Bus && err.scenario == E_BUS_01_UNMAPPED`（`executor.rs:692`）⇒ **其它总线错误与其它错误族原样上抛**。③ **根因比原台账深一层**：`BusMemory` 原先把**所有**总线错误重包成 `ISS-01` ⇒ 「未映射」根本**无法**与其它总线错误区分 ⇒ 本批改为**保留 family / scenario**，这是「未映射可判别」的**前提**（不是顺手改的）。**黄金语料**：新增 `tests/golden/local/unmapped_store.S` 与 `unmapped_load.S`（guest **自校验** `mcause` / `mtval` / `mepc` 三项后才 PASS）⇒ 语料 **64 → 66**。**判别力**：5 条反向变异全红；其中「把 store 的 cause 改成 5」⇒ `unmapped_store` 红而 `unmapped_load` 仍绿 ⇒ **5 / 7 的分界可分别观察**（不是一条恒绿用例）。⚠️ **取指侧未映射访问本批不做**（仍是总线错误，不是 cause 1）⇒ 已登记 **D-103** |

---

### 1.3 跨模块中低危

| ID | 归属模块 | 来源 | 问题 | 阻塞关系 / 备注 |
| --- | --- | --- | --- | --- |
| **D-15** | M-06 `elf.rs` | 首次全量审查 | 缺 `p_filesz <= p_memsz` 校验；畸形 ELF 会建出超过声明内存的段 | ✅ **已闭环**（v0.11，commit `9a9d578`）：补校验 + 测试 |
| **D-16** | M-13 ISS | 首次全量审查 | `Decoded` 无指令长度字段，PC 恒 `+4` | ✅ **已闭环**（v0.11，commit `bdf5f1b`）：`Decoded` 增 `pub len: u8`（字节数），PC 按 `d.len` 步进。RV32C 本轮不做，但结构已能承接 |
| **D-17** | M-12 | 首次全量审查 | `schedule()` 把 seq 计数塞进 `state.event_queue_head`；`restore()` 后可能与队列中已有事件撞号 | ✅ **已闭环**（v0.11，commit `86dfa4b`）：seq 计数独立化，`event_queue_head` 的语义明确为「单调插入序计数器」；`restore` 时把它顶到队列已占用 seq 之上，恢复后不撞号 |
| **D-18** | 前端 `m02` | 首次全量审查 + 前端报告 #3 | **按钮的 `closest("[data-region-base]")` 前置条件落在 `<table>` 之外** → 按钮永远匹配不到，点了没反应；连带 `seedDemoBus()` **从未定义**（去掉前置条件后会直接抛 `ReferenceError`） | ✅ **已闭环**（`37fe8da`）：去掉 `closest("[data-region-base]")` 前置条件（该按钮在 `<table>` 外）；连带修掉评审漏项——`seedDemoBus()` 此前**从未定义**，去掉前置条件后按钮会直接抛 `ReferenceError` |
| **D-19** | 前端 `m01` | 首次全量审查 + 前端报告 #7 | **UI 态采集退化为 `setState(hash, {})`——切走视图时不保存 UI 交互态**，LLD §3.3 要求的「按 viewId 存 UI 交互态」此前**从未采集** | ✅ **已闭环**（`4738aa6`）：不再是 no-op——新增 `captureUiState()` / `restoreUiState()`，切走前采集、切回后还原 UI 交互态（LLD §3.3 要求的「按 viewId 存 UI 交互态」此前从未采集） |
| **D-20** | 前端 `m02` | 首次全量审查 + 前端报告 #4 | **终端输出的双层转义**——`onDelta` 里的 `escapeHtml()` 让字节流被**转义两次**，显示的不是原始字节 | ✅ **已闭环**（`37fe8da`）：`onDelta` 去掉 `escapeHtml()`，字节同步进 M-03 store |
| **D-21** | M-06 `mod.rs` | 首次全量审查 | `detect_kind` 未知扩展名一律回落 `Elf`，拖入无扩展名 raw 文件报 "bad ELF magic" | ✅ **已闭环**（v0.11，commit `189f500`）：新增 `detect_kind_from_bytes` / `detect_kind_with_hint`，按 **magic bytes 嗅探**（ELF magic / IHEX 行首 / raw 兜底），扩展名只作辅助 |
| **D-22** | M-06 `mod.rs` | 首次全量审查 | `Loader::load_with_limits` 默认实现 `let _ = limits;` 静默忽略 limits；`parse_raw` 复用 `elf_size` 限制 raw 文件 | ✅ **已闭环**（v0.11，commit `189f500`）：`Loader` trait 的 `load_with_limits` 改为**必须实现**（全库仅 `DefaultLoader` 一个实现，无其它消费方），`ParseLimits` 逐项生效；HEX 路径改用独立 `hex_size` 限额 |
| **D-25** | M-15 | 检视 V2 · F-5 附带 | **UART 探针信号只实现了 `TX`**：M-15 LLD §外设能力表规定 UART 应上报「TX / RX 状态、CTS 电平」三路，实际只有 `"TX"`（字节流） | ⏳ **未做**：`"RX"` / CTS 两路没有实现，也**从未进过台账**。接真实内核后 UART RX 不会出现在波形里。随 M-15 下一波补 |
| **D-26** | M-16 | 检视 V2 · F-5 附带 | **M-18 的 ch=2（中断脉冲）无生产者**：M-16 曾是 44 行 stub（`m16_interrupt/mod.rs` 「Status: stub — port adapted; no CSR plumbing yet」），M-18 的 `sample_irq_pulse` **已就位但无人调用** ⇒ 中断脉冲通道在波形里永远不出现 | ✅ **已闭环**（v0.19 · M-16 落地批）：M-16 的 `request()` 在**真的登记**一条挂起时发一次脉冲（`m16_interrupt/mod.rs` 的 `pulse_sink` 回调；**被同线去重折叠**的重复请求**不发**）⇒ 宿主在 `install_probe_pipeline()` 里把它接到 M-18 的 `sample_irq_pulse`（`kernel_host/mod.rs`），而 `install_probe_pipeline()` 在 `new()` 与 `rebuild_platform()` **两处**都调 ⇒ 装得上、且 `LoadProject` 后不丢。**证据**：`kernel_host/tests_intc.rs` 的 `one_request_produces_exactly_one_irq_pulse_edge`（一次请求 ⇒ **恰好一条**边沿）+ `the_pulse_sink_survives_a_project_reload`（重载工程后仍在）+ M-16 的 `the_pulse_callback_fires_once_per_registered_request`（30 条 M-16 用例之一）。⚠️ **判别力**：变异「`request()` 不再发脉冲」⇒ 这三条**全红**（反向变异自检 **K-06**） |
| **D-27** | M-15 + M-18 | 检视 V2 · F-5 附带 | **M-18 的 ch=3（按键）无生产者**：M-15 根本没有按键外设（目录仅 gpio / uart / timer） | ⏳ **未做**：LLD 表里的 ch=3 是预留位。实现侧留空即可，**但 LLD 需注明「P1 预留、暂无生产者」**（本轮已随 F-5 的 LLD 对齐一并标注） |
| **D-28** | M-15 + M-18 | 检视 V2 · F-9 | **GPIO SingleBit 通道与 32-pin 位图错位**：`GpioBank::write` 把 32 位位图报给 SingleBit 通道，而 `ChannelDef::classify` 对 SingleBit 只看最低位 | ⏳ **未做**：多脚位场景下**只有 pin 0 的升降沿被识别**，其余 31 脚的状态变化被同一分类器吞掉。接线测试只验证了 pin 0。**这是 MVP 演示多脚位波形时的真实限制**（接线报告 §5.3 已自陈） |
| **D-29** | M-14 | 检视 V2 · F-4 | **E_BUS-05 把三种本应区分的子情况合并成同一 error code + scenario**：「外设主动抛错」/「外设实例忙（`try_write` 失败）」/「外设未接线（id-only 旧路径）」三者都是 family=Bus, scenario=5 | ⏳ **未做**：原 E_PER-02 之类的信息只剩在 `message` 文本里，前端要靠字符串匹配才能区分。**「实例忙」是 M-14 自身状态，不是外设主动抛错**，不该与外设错误共用。接线报告 §2.6 已诚实记录 |
| **D-30** | M-12 | 检视 V2 · §八 | **M-12 至今没有独立模块级 review** | ✅ **已闭环（2026-10-22 · v0.22 批 · V-D 专项检视 + Lane W2 修复）**：M-12 拿到**第一次独立模块级 review**——V-D 以**专项检视**（不是顺带看）的方式过了一遍 M-12，出 **9 项发现**（**中危 3** + 低危 6）；三条中危（**VD-F1** 引擎自己产出的快照过不了契约自己的 `validate()` · **VD-F2** `restore_full` 的 `next_seq` 下限修正零判别力 · **VD-F3** `run_until` 的 `InterruptTaken` 分支无界自旋）由 Lane W2 **全部修完**并逐条做过**反向变异**（红集见 §3.13.5.3）。⚠️ **本条闭环的真正价值不是「三条中危」**，而是**M-12 的快照 / 时序语义第一次被独立审视过**——它是全平台确定性的基础，此前一直只有「加固 + 自测」，没有第二双眼睛。**低危 6 条**（VD-F5 ~ VD-F9 + VD-F4）登记为 **D-104 / D-108 ~ D-112** 与 §3.13.3 的 VD-F4，**留账未做** |
| **D-31** | M-18 | 检视 V2 · Batch B 报告 | **`make_bus_hook` 在构造时需 `reg.borrow()`**（取 `borrow_contended` 计数句柄），即「构建 hook 时注册表不可被借用」 | ⏳ **已记录**：与同模块 `wire_signal_probe` 的既有约束同构，不算缺陷，但确实是新增的**构造期前置条件**。彻底消除需把计数器改为构造时传入的独立 `Rc<Cell<u64>>`（**属公开 API 签名变更**，需决策） |
| **D-32** | 契约 + 前端 | 检视 V4 · F6 附带（前端服务层报告） | **契约缺口 7 项**：无寄存器快照事件 / 命令、无写寄存器命令、无 `negotiate`、`u64` 超 2⁵³ 精度、LLD 的 `E_PROTO` 族在契约 `ErrorFamily` 无对应、`crc` 未实现、无 `simStateDelta` 变体 | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：契约层自 v0.10 起冻结在 `schema_version=1`，本批**首次升版**到 **2**，7 项缺口**逐项闭环**（冻结规格 = `docs/detailed-design/STAGE-2_Contract_V2.md` §3 总览表）。**根因**：不是「设计漏了」，是「**冻结层不允许加**」——契约层冻结期间 LLD 引用的能力在契约侧**没有落点**。**逐项处置**：① **寄存器读事件 / 命令** → 增 `CommandPayload::ReadRegisters{index,count}` + `EventPayload::Registers{index,values}`（`crates/contract/src/protocol.rs` §4.1 / §4.2）；② **写寄存器命令** → 增 `CommandPayload::WriteRegister{index,value}`（同上 §4.1）；③ **`negotiate`** → 增 `CommandPayload::Negotiate{min_schema_version,cap_bits}` + `EventPayload::Negotiated{schema_version,cap_bits,kernel}`（同上 §4.1 / §4.3）；④ **`u64` 超 2⁵³ 精度** → 虚拟时间类字段改走**十进制字符串**（新增 `crates/contract/src/numstr.rs` 的 `u64_decimal` serde 辅助模块；**Rust 类型仍是 `u64`**，只改**线上表示**，内核内部零改动）；⑤ **`E_PROTO` 族无归属** → 增 `ErrorFamily::Protocol`（`as_str() == "PROTO"`，`crates/contract/src/error.rs`），与 `Transport`（`"TRX"`）**并存不合并**；⑥ **`crc`** → 落在 **M-07 的 `TransportFrame`**（**不是契约信封**），算法 / 覆盖范围 / 表示 / 兼容策略见规格 §7.2 与 M-07 LLD §2.1.1；⑦ **`simStateDelta`** → 增 `EventPayload::SimStateDelta{registers,pc,t_ps}` + 配套 `model::DeltaRegister`（protocol.rs §4.4）。**破坏性变更逐条列明**于规格 §1.1（新增变体 / 数值表示 / 改名 / 新增错误族），**其余全是加法**（不删字段、不改既有载荷形状）。⚠️ **`SNAPSHOT_SCHEMA_VERSION` 1 → 2**：快照的时间字段也字符串化，v1 快照文件不再可解析——升版让旧读者**大声拒绝**而不是误读时间戳（规格 §10）。**证据**：`crates/contract/src/{lib,protocol,model,error,numstr,snapshot}.rs` + 契约层 **32 条**用例全绿（`cargo test -p riscv_sim_contract`）。**判别力**：① `front/tests/contract.test.mjs` 的「Rust 源码 ↔ JS 镜像逐字对拍」**已经把本批的红抓出来了**——JS 镜像尚未升到 v2，该门禁**当时 4 条红**（命令变体集合 / 事件变体集合 / 变体内层字段名 / newtype 变体的 DTO 字段名），这正是「变体拼写不一致能被看见」的证明；⚠️ **这 4 条红已随前端批合入而转绿**——`tools/check-front.sh` 现在 **200/200 全绿**（JS 镜像已升到 v2）。**「红 → 绿」本身就是判别力的证据**：镜像不同步时门禁**抓得住**，同步之后**不再报**——若这 4 条是恒绿的门禁，它们就什么也证明不了；② `crates/contract/src/numstr.rs` 的 `the_round_trip_is_lossless_above_two_to_the_53` 钉住「超过 2⁵³ 不丢精度」；③ `crates/contract/src/error.rs` 的 `the_protocol_family_is_spelled_like_the_lld` 钉住族名拼写。⚠️ **前端镜像同步不在本批范围**（`front/**` 归前端批）——两侧 `schema_version` 必须**同时**升到 2（规格 §10）。**该前端批已合入**（`feature/c2-front`，3 commit）|
| **D-33** | 前端 M-01/02/03 | 检视 V4 · Batch C 报告 | **落地视图 `embedded` 没有 M-02 组件**：首次进入工作台只渲染出「嵌入式工作台」标题 + 空面板 | ⏳ **未做**：**非本轮引入**，此前无人用真实浏览器走过这条路径——`tools/browser-verify.mjs` 的「正文非空」断言一上来就把它照出来了（脚本把该检查改放有组件的 `#/regmem` 视图，并在前端服务层报告第七节记为第 7 条欠账）。要么补 `embedded` 的 M-02 组件，要么把它从落地视图列表里去掉 |
| **D-34** | 契约 | 检视 V4 · F11 | **命名歧义**：`model.rs` 同时存在 `Waveform`（history 形）与 `WaveformDelta`（live 形），而 `EventPayload::WaveformDelta` 包装的是 `Waveform` 而不是 `WaveformDelta` | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：**根因**不是「两条路径都要留所以没法改」，是「**名字与内容错位**」——`EventPayload::WaveformDelta` 装的是 **history 形**（`schema_version + streams[]`），而 **live 批次形**（`seq_start/seq_end/t_virt/entries`）才是 `model::WaveformDelta`。读 `model.rs` 的人必然被误导。**处置（两个名字各归其位）**：① `model::Waveform` → **`model::WaveformHistory`**（history 形，线上真正在用的那个）；② **`EventPayload::WaveformDelta(Waveform)` → `EventPayload::WaveformHistory(WaveformHistory)`**——**变体名随内容改**；③ topic `waveform.delta` → **`waveform.history`**（跟随变体名）；④ ⚠️ **`model::WaveformDelta`（live 批次形）保留原名**——它**不是死代码**：M-19 的 `DeltaSink::push_delta` 有实现方与测试，它只是**本批不上线**（尚未接进契约事件，见 **D-50**）。**证据**：`crates/contract/src/model.rs`（`WaveformHistory` 与 `WaveformDelta` **两个类型并存**）+ `crates/contract/src/protocol.rs` 的 `EventPayload::WaveformHistory(WaveformHistory)` + 用例 `the_waveform_event_carries_the_history_shape`（断言事件装的是 history 形）。**判别力**：① `the_waveform_event_carries_the_history_shape` 钉住「变体装的是 history 形」；② `front/tests/contract.test.mjs` 的「事件变体集合与 Rust `EventPayload` 一致」会**因为改名而变红**（JS 镜像未同步，**当时**门禁 4 条红）——这正是「改名必须两侧同时改」的证明；⚠️ **这 4 条红已随前端批合入转绿**（`tools/check-front.sh` 现 **200/200 全绿**）；③ M-19 LLD §2.1.1 的 `WaveformDelta`（live 批次形）**保持原名不动**——它是 D-50 的落点，跟着改名反而会**制造一个新的错位**|
| **D-35** | 前端 mock | 检视 V4 · F13 | **mock 内核 PC 永远 +4，无 RV32C** | ⏳ **已记录**：压缩指令是 2 字节，mock 的单步 PC 永远按 4 步进。**仅影响 mock 训练出来的 UI**——接真实内核（C 扩展落地后）行为会突变。随 C 扩展一并处理 |
| **D-38** | 工具链 + M-12 | 收口轮检视 V-C · L-2 | **`tools/build-wasm.sh` 跑不通**：`wasm-pack build --target no-modules ... crates/kernel` 报 `Ensure that you have "wasm-bindgen" as a dependency in your Cargo.toml file`——kernel 没有 `wasm-bindgen` 依赖 | ✅ **已闭环（2026-10-20 第 0 步，`build(wasm)`）**：① kernel 增 `wasm-bindgen` 依赖，**target-specific**（`[target.'cfg(target_arch = "wasm32")'.dependencies]`）——**native 依赖树实测 0 命中**，原生构建的依赖面与体积不受影响；② 新增 `crates/kernel/src/wasm_api.rs`（`#[cfg(target_arch = "wasm32")]`）导出 `kernel_version()` / `protocol_schema_version()` / `ping()`；③ **修掉脚本的输出路径 bug**——`wasm-pack build <crate-dir>` 以 **crate 目录**为工作目录，相对 `--out-dir` 会落进 `crates/kernel/front/assets/pkg`，改为绝对路径；④ **修掉脚本的 target 选型错误**——原注释「刻意不用 `--target web`」是误解：需要 bundler 的是 `--target bundler`；`--target no-modules` 产出的是 **CommonJS 风格 IIFE、无 `export` 语句**，原生 ES module 前端**根本 import 不到它的导出**（动态 `import()` 得到空命名空间）；`--target web` 产出**带命名导出的原生 ESM**（`export function kernel_version()`）+ 用 `import.meta.url` 定位 `.wasm`，正是零构建前端要的形态。**证据**：`tools/build-wasm.sh` 跑通，产物 61 K wasm + 8 K ESM glue 落 `front/assets/pkg/`；Node 22 实测 `kernel_version()=0.1.0` / `protocol_schema_version()=1` / `ping("你好, RISC-V")=pong:你好, RISC-V`（UTF-8 往返正确）。⚠️ **本条只闭合「构建路径可跑通 + 导出可调用」**；真正的 `process_message(json) → json` 协议桥属**第 1 阶段**，此处**刻意不做半成品**——一个会静默丢命令的半截桥，正是 D-24「快照 API 看着完整实则不是」那一类事故 |
| **D-39** | 契约 | 收口轮检视 V-A · 🟢-6 | **`validate()` 不检查「队列最早到期时刻 ≥ `state.virtual_time_ps`」**：一个 `at_ps` 早于当前虚拟时间的快照能通过校验 | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：**根因**——`validate()` 只查「队列自身的规范性」（规范序 / seq 唯一 / seq < next_seq / 队头与 next_seq 自洽），**没查「队列与快照虚拟时间的关系」**。一个 `at_ps` 早于 `state.virtual_time_ps` 的快照能通过校验——恢复后事件**立刻触发**，那已经不是「T 时刻的状态」。**处置**：`EngineSnapshotDto::validate()` 增**第 7 条**检查——事件队列里**最早**的 `at_ps` 必须 **≥** `state.virtual_time_ps`，否则报 **`E_SNAP_02`**（**复用**队列畸形码，**不新开场景码**：一场景一码原则）。⚠️ 这是**防御性**检查而非真缺陷——引擎自己产不出这种快照（迟到的 `at_ps` 会被归一化到 `max(at_ps, now)`），它防的是**畸形 / 手改的存储文件**。**证据**：`crates/contract/src/snapshot.rs` 的 `validate()`（第 7 条检查紧接队头一致性检查之后）+ 用例 `validate_rejects_an_event_that_was_already_due`（早于虚拟时间 → `E_SNAP_02`）与 `validate_accepts_an_event_due_exactly_now`（**恰好等于**虚拟时间 → 放行，边界不含糊）。**判别力**：这**两条**用例一起钉住边界——只写「拒绝」不写「边界放行」，`>=` 被误写成 `>` 会有一条变红；「取队首即最早」依赖的是队列的规范序（`(at_ps, seq)` 有序），规范序被破坏另有 `validate_rejects_a_queue_out_of_canonical_order` 守着|
| **D-41** | 契约 + M-13 | 收口轮检视 V-C · H-1 附带（pre-existing） | **契约层 `SimState` 缺「内存页」与「外设状态」两个字段**：LLD 总纲表12 把 `SimState` 的关键字段列为「PC、通用寄存器、CSR、**内存页**、**外设状态**、虚拟时间、事件序号」，而契约层现行的 `SimState` 只有 `gpr/pc/csrs/pending_writes/virtual_time_ps/event_queue_head/seed` | ✅ **已闭环（2026-10-21 · 会话级快照载荷批）——但走的是「**取消**」这条路**。⚖️ **裁决 B**（规格 §1.2）：**CPU 寄存器已经在 `m12Engine.state` 里**（`gpr` / `pc` / `csrs` 全部在那儿）→ `m13State` 段**只承载内存页**，**寄存器不重复存**（否则是**两个真相**）。于是三段的分工落成：**段 1 `m12_engine`** = 寄存器 + CSR + 虚拟时间 + 事件队列 + 插入序 + 指令预算 + `pending_writes` · **段 2 `m13_state`** = 内存页（**RAM 内容**，64 KiB 分块）· **段 3 `m14_map`** = 区域表 · **段 4 `m15_periph`** = 外设状态 · **段 5 `m16_intc`** = INTC 状态。⚠️ **M-13 那项按裁决 B 取消**——M-13 是**纯解释器**（`Iss` 不持有任何跨指令的可导出状态），**「没有状态导出接口」不是遗漏，是设计**；M-13 LLD §9.4 已如实写明「**不提供**接口……**这不是遗漏**」。M-14 补 `export_map()` / `load_map()` + `export_pages()` / `load_pages()`（`load_map` 重建 **RAM 清零** / **ROM 原地保留内容**（`base`/`size` 不符报 `E_BUS-12`——固件**不随快照走**） / **外设区域原地保留**（外设实例是**输入**，删了恢复后总线无外设）） |
| **D-40** | 契约 | 收口轮检视 V-C · L-3 | **`EngineSnapshotDto::default()` 是一个「合法但空」的快照**：`validate()` 对它返回 `Ok(())`，宿主若把 `default()` 落盘，得到的与「真实快照」在类型上不可区分 | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：**根因**——`EngineSnapshotDto::default()` 是**「合法但空」**的快照：`validate()` 对它返回 `Ok(())`，宿主若把 `default()` 当夹具落盘，得到的产物与「真实快照」在**类型系统里不可区分**（两者都是 `EngineSnapshotDto`）。**处置**：新增 `pub fn is_empty(&self) -> bool`，让宿主能**显式拒绝**空快照，而不是靠猜。**「空」的判据**：队列为空 **且** 虚拟时间为 0 **且** PC 为 0 **且** 全部 GPR 为 0 **且** 无 CSR **且** 无待写回执。⚠️ **不做** `Default` 的破坏性移除——那是 breaking change，本批不做（规格 §8.2）。**证据**：`crates/contract/src/snapshot.rs` 的 `is_empty()` + 用例 `a_default_snapshot_is_valid_but_empty` 与 `a_snapshot_of_a_running_guest_is_not_empty`。**判别力**：**两条**用例一起钉住——`a_default_snapshot_is_valid_but_empty` 钉「`default()` 合法但空」，`a_snapshot_of_a_running_guest_is_not_empty` 钉「真跑过的 guest 的快照**永不为空**（哪怕队列恰好是空的）」；后者防的是把判据写成「队列空即空」——那会让跑过的 guest 的快照被误判为空|
| **D-42** | M-13 + M-14 | 第 2 阶段 golden 对拍（2026-10-20） | **「与 Spike 一致允许非对齐」的表述缺了一个必要限定**：Spike 只在 **Zicclsm 扩展启用**时才*执行*非对齐访问——`riscv/mmu.cc` 的 `is_misaligned_enabled()` 就是 `proc->extension_enabled(EXT_ZICCLSM)`，为假时直接 `throw trap_load_address_misaligned`。**stock `spike --isa=rv32imc` 是抛陷阱的**，而 M-13 的默认 `MisalignedPolicy::Allow`（执行）与它**相反**，只与「启用了 Zicclsm 的 Spike」一致 | ✅ **已闭环（v0.12）**：本轮对拍已把参考侧 ISA 钉为 `rv32imc_zicclsm_zifencei` 规避（口径见 `tests/golden/README.md`），M-13 的 `executor.rs` 注释已补上限定语，**LLD 措辞已于 v0.12 订正**（M-13 §3.6(2)/§5.2.1 与 M-14 §3.2/§6.2(1) 均补上「前提是参考侧 Spike 启用 Zicclsm 扩展」）。另需决策：`MisalignedPolicy::Trap` 一旦被启用，是否需要配套的陷阱投递（依赖 M-16，第 5 阶段） |
| **D-43** | M-13 | 第 2 阶段 golden 对拍（2026-10-20） | **M 扩展（8 条）与 C 扩展（1 条）尚未实现**，golden 语料 51 例中 **9 例无法与 Spike 对拍**（`rv32um/*` 8 例 + `rv32uc/rvc` 1 例，现状是 `IllegalInstruction` 陷阱） | ✅ **已闭环（v0.16，分支 `feature/m13-mc-extensions`）**：**RV32M 8 条 + RV32C 26 条全部落地**（`alu.rs` 增 8 个乘除函数 · **新增** `compressed.rs` 字段级展开表 · `Decoded` 增 `compressed`/`compressed_raw`/`illegal`）。9 例 `pending` → `active`，**golden 语料 51/51 逐例判决一致**（`total=51 ok=51 mismatch=0`）。⚠️ **本轮同时订正了 M-13 LLD 的 4 处措辞**（v1.5 → v1.6）：① **取指宽度**——原「16 位压缩指令一次读 2 字节」与 §3.5 自相矛盾，改为**访存宽度恒为 32 位**、低 2 位只判宽度；② **展开粒度**——原伪码是「展开成 32 位位模式」，实现改为**字段级展开**（位模式表达不了 C.LUI 的 20 位截断规则）；③ **压缩返回地址的调整点**——由 `d.compressed` 改为 **`d.len`**（语义等价，但「只有一个真相」）；④ **§3.5 表格的编码列**——CI 组与 CA 组的 funct2 位置、CR 组 funct2 只有 1 位、`C.MV`/`C.ADD` 的 rd=x0 合法等，全部按**汇编器输出 + 参考实现**逐条重写 |
| **D-63** | M-13 + M-16 | v0.16 检视 V-C · 🟡-12 | **取指未对齐的 mcause 与实现的 `TrapCause` 变体对不上**：LLD §3.1 / §5.2 写「取指未对齐 → mcause=0」，但实现的 `TrapCause` 只有一个 `MisalignedAccess` 变体，**取指未对齐与访存未对齐共用**它（mcause 4/6） | ✅ **已闭环**（v0.19 · M-16 落地批 · 裁决 B）：`TrapCause` 拆成**三个**变体 —— `InstructionAddressMisaligned`（**mcause 0**）· `LoadAddressMisaligned`（**4**）· `StoreAddressMisaligned`（**6**），`MisalignedAccess` 移除。**三个码各有真实构造点**：取指（`m13_iss/mod.rs` 的取指未对齐检查 ⇒ cause 0，`mtval` = 故障地址）· 读（`executor.rs` ⇒ 4）· 写（`executor.rs` ⇒ 6）；`TrapCause::mcause()` 是**唯一**的映射点。**证据**：M-13 的 `a_misaligned_fetch_reports_cause_zero_and_the_faulting_address` · `a_misaligned_load_reports_cause_four_and_the_faulting_address` · `a_misaligned_store_reports_cause_six_and_the_faulting_address` · `the_three_misaligned_causes_carry_three_different_codes`（4 条）。⚠️ **反向变异自检 K-26**（把取指未对齐报成 4）⇒ 变红 |
| **D-64** | 契约 + M-13 + M-16 | v0.16 检视 V-A · 🟡-3 | **mtval 在全平台不存在**：契约层与内核层**没有任何 mtval 字段**（`grep -rn "mtval" crates/` 只命中两条注释），只有一行「以后会来」的说明。保留压缩编码其实**已经**把 16 位原码存进了 `Decoded::compressed_raw`，信息齐备，缺的只是「把原码写进 mtval」这一步 | ✅ **已闭环**（v0.19 · M-16 落地批 · 裁决 A）：`mtval` 落 **CSR `0x343`**，由**既有的通用 CSR 文件** `SimState.csrs` 承载（**不加** `SimState.mtval` 专用字段 ⇒ 契约层**不升** `schema_version`，前端 JS 镜像不用改）。**写入点全部到位**：非法指令（32 位 ⇒ 原指令字；**压缩 ⇒ `compressed_raw` 原码**）· 未对齐（取指 / 读 / 写 ⇒ **故障地址**）· ECALL / EBREAK（⇒ 0）· 中断（⇒ 0）。**证据**：M-13 的 `entering_a_trap_writes_mepc_mcause_and_mtval` · `an_illegal_instruction_writes_the_instruction_word_into_mtval` · `a_compressed_illegal_instruction_writes_its_original_code_into_mtval` · `an_ecall_writes_zero_into_mtval` · `a_ebreak_writes_zero_into_mtval`（5 条）+ 三个未对齐用例的 `mtval` 断言。⚠️ **反向变异自检 K-21**（`enter_trap` 不写 mtval）⇒ 变红 |
| **D-65** | golden harness | v0.16 检视 V-A · ⚪-5 | **golden 对拍两侧的栈窗口完全不重叠**：参考侧 Spike 的窗口是 `[0x7fff_f000, 0x8000_0000)`（`golden-run.sh` 的 `-m 2147479552:4096`），候选侧 `riscv-golden` 的窗口是 `[0x7fff_e000, 0x7fff_f000)`（`STACK_BASE = 0x7fff_e000`）。两者**零交集**，而初始 `sp = 0x7fff_f000` 恰好落在**参考侧窗口的底**、**候选侧窗口的顶（开区间）**——候选侧读 `sp` 本身即 `E_BUS-01: read unmapped addr=0x7ffff000` | ⏳ **已登记未做**（v0.16 检视轮）：**预存**（本分支之前就存在），**当前语料不用栈**，所以是**潜伏**的。⚠️ **任何用到 `sp±k` 的 golden 用例都会产生假 mismatch**（一侧退休、一侧 E_BUS-01）。**待做**：把两侧窗口对齐（推荐候选侧 `STACK_BASE = 0x7fff_f000`、窗口 `[0x7fff_f000, 0x8000_0000)`，与参考侧同址同长；或反向改参考侧）——**任一方向都会改动对拍基线，须单独做一轮全量复验**。本轮**只订正了 `tools/golden-run.sh` 里写错的十六进制注释**（`0x7ffe_f000` → `0x7fff_f000`），**没动任何一侧的内存图** |
| **D-66** | M-13 | v0.16 检视 V-A · ⚪-6 + V-B · 🟡 P3-1 | **三处死接口/死字段**：`Decoded::compressed`（全 kernel 零消费者——链接寄存器用的是 `d.len`）· `Decoder::fetch_size()`（`grep` 全仓 0 调用点，实际取指宽度硬编码在 `m13_iss/mod.rs`）· `TrapCause::IntegerDivideByZero`（**全仓从未被构造**——M 的除零是已定义结果，不产生陷阱） | ⏳ **已登记未做**（v0.16 检视轮）：本轮已把三者的**注释**订正到与事实一致（`fe081e1`：`IntegerDivideByZero` 改为「保留给未来的真实除零原因」；LLD §3.1 标注 `fetch_size()` **不参与取指路径**），并给 `illegal` 补了断言（`d.illegal` 现在**有**观察者）。但**接口本身仍在**——删它们属**公开 API 变更**（`Decoded` 是 `pub`），按「公开 API 只增不减」须走决策。**待做**：随 D-32 的 `schema_version=2` 一并决定「删 / 标 `#[deprecated]` / 保留并写明用途」 |
| **D-67** | M-13 + D-07 用例 | v0.16 检视 V-B · 🟡 P3-4 | **C 扩展的保留编码规则成了 D-07 用例的隐式前提**：`crates/kernel/tests/d07_time_driven.rs` 的 `a_trap_still_commits_the_pending_writes` 用 `trap_program() = vec![0u8; 4]` 造陷阱——它依赖的正是「低半字 `0x0000` 是保留 C.ADDI4SPN」这条 **C 扩展**规则（D-07 属于时间驱动链路，与 C 扩展毫无关系）。另：6 处保留检查**过度集中**——全部只由 `the_reserved_and_hint_encodings_are_classified_like_the_reference` **一条**用例守住 | ⏳ **已登记未做**（v0.16 检视轮）：**今天无活影响**（两侧都绿）。⚠️ 但这是**跨模块隐式耦合**：改 C 扩展的保留规则会**意外地**改掉 D-07 用例的行为，而两件事在文档上毫无关系。**待做**：① D-07 那条用例改用**与 C 扩展无关**的陷阱源（32 位非法指令字）——本轮**未改**，因为它会动 D-07 的既有用例，且要重跑 D-07 的 31 条；② 保留检查的守卫**按指令族拆开**，别让一条用例守六个点 |
| **D-68** | 契约 | v0.17 契约 v2 批自查 | **契约的 `DEFAULT_INSTRUCTION_BUDGET = 10_000` 文档原写「per-quantum」，而引擎的每量子配额是 1 000——措辞差了 10 倍**：只读契约、不读引擎的人会把它当成「一个量子装 10 000 条指令」，进而把<b>量子 / 配额 / 预算的换算整个算错</b> | ✅ **已闭环（契约层 v2 批自查，commit `3a9660c`）**：**只改文档，不改行为**——**行为从来是对的**：`Engine::instruction_budget` 一直就是**运行窗口**预算（= 10 个量子；10 000 × 1 ns = 10 µs，对 1 µs 的量子），`run_until` 的停止条件写的也是「本轮指令预算耗尽」；错的只是 `crates/contract/src/snapshot.rs` 里 `DEFAULT_INSTRUCTION_BUDGET` 的 doc comment 措辞（`the per-quantum instruction budget` → `the run-window instruction budget`，模块头同处一并订正）。**根因**：与 **D-57 / D-58 同族**——契约层冻结期间，「契约文档」与「实现口径」两处各说各话，而**冻结层不允许改**；契约层 v2 解冻后才有机会一次性订正。**证据**：`crates/contract/src/snapshot.rs` 的 `DEFAULT_INSTRUCTION_BUDGET` doc comment + `crates/kernel/src/m12_engine/mod.rs` 的 `QUANTUM_INSTR_QUOTA = 1_000`（每量子配额）。**判别力**：`m17_backend::tests::continue_until_event_runs_the_quantum_main_loop`——它断言一轮执行条数**正好**等于 `DEFAULT_INSTRUCTION_BUDGET`（10 000）**且**是量子配额的**整数倍**：前者把「预算耗尽」与「停在量子边界」（1 000）**区分开**，后者把「量子化调度」钉住。⚠️ 本条是**文档口径偏离**、不是行为缺陷，故**判别式是文档核对**（规格 §11 验收清单第 10 项），**不需要**新增用例——D-57 台账里写的「改契约文档时必须同步补一条行为用例」这条要求，**本批已由上述既有用例满足**|
| **D-69** | 契约（+ M-15） | v0.17 契约 v2 批 · V-C 检视发现 | **错误码族前缀 `E_PP` 与契约 / 代码的 `E_PER` 不一致**：LLD 的错误码表与 M-15 LLD 写的是 `E_PP-01`…`E_PP-05`，而契约 `ErrorFamily::Peripheral.as_str()` 是 **`"PER"`**——线上跑的（代码里引用的）本来就是 `E_PER-02` | ✅ **已闭环（v0.17 契约 v2 批 · 文档侧）**：**根因**——契约层与代码是 **canonical**，LLD 的 `E_PP` 是**错的**（族标签必须与契约的 `as_str()` **逐字一致**，否则线上看到的码在 LLD 表里查不到）。**方向**：**改 LLD，不改契约、不改代码**——反过来改契约的 `as_str()` 是一次**破坏性的线上变更**（已部署的行为会变），而且代码已经按 `PER` 在跑了。**处置**：`docs/**` 全量机械替换 `E_PP` → `E_PER`（**39 处**带场景码的引用 + **1 处**占位写法（`E_PP-xx`）+ **4 处**族清单 = **44 处**），并把错误码表里的族清单同步为 `E_CTX/E_BUS/E_ISS/E_PER/E_DBG/E_PROTO`。⚠️ 文档内部原本就**自相矛盾**：39 处写 `E_PP`、8 处写 `E_PER`——「同一族、两个前缀」在表里并存，调试器拿线上码查表必然落空。**证据**：`crates/contract/src/error.rs:99`（`Self::Peripheral => "PER"`，canonical）+ `crates/kernel/src/m15_peripheral/{uart.rs:205, reg.rs:16, tests.rs:536}`（代码里引用的就是 `E_PER-02`）+ `docs/module-design-spec/RISC-V_Module_Design_Specification.html` §3.4 表16 / §6.2 表89 的 `E_PER-01`…`E_PER-04` 行。**判别力**：本条是**纯文档拼写**，判别式是**逐字 grep**——`grep -rn "E_PP" docs/ \| grep -v "docs/reviews/REVIEWS.md"` 必须 **0 命中**（改完已实测 0；**本条台账自身要引用旧拼写**，故排除本文件——不排除的话会恒有 1 命中，把判别式变成**假红**）。⚠️ **诚实标注（已解除）**：本条登记时契约层**没有**一条直接断言 `Peripheral.as_str() == "PER"` 的单测（当时的 `the_protocol_family_is_spelled_like_the_lld` / `the_transport_and_protocol_families_are_distinct` **都不覆盖 PER**）——「契约侧是 canonical」当时只靠**源码字面量 + 代码注释**支撑，**没有用例在守**。✅ **该缺口已由 `8767c4a` 的 `crates/contract/src/error.rs::tests::every_family_tag_is_pinned` 补上**：一条**表驱动**用例**全覆盖 14 个族**的 `as_str()`（含 `Peripheral → "PER"`），并且断言 `TAGS.len() == ALL_FAMILIES.len()`——「新增族却忘了在清单里登记」会被这条用例抓住（族清单 = `error::ALL_FAMILIES` 常量）。**待做**：**无**（原「补一条覆盖各族 `as_str()` 的表驱动单测」已由上述用例满足）|
| **D-44** | golden harness | 第 2 阶段检视 V-B · O-1 | **变异金丝雀的判别力只覆盖「判决运输路径」**：金丝雀执行的是 `li` / `beqz` / `sll` / `or` / `sw` / `auipc` / `j` 这 7 条指令构成的自检路径，**不在该路径上的指令被改错，金丝雀在原理上不可能察觉**（它根本不执行它们）。实测：注入 10 个 RV32I ALU 缺陷，**8 个漏检**；唯一被金丝雀抓到的两个是 `add` 与 `or`——`add` 是因为金丝雀恰好就是 `add.S`（**用例选择的巧合，不是判别力**）。全量对拍（Spike 作独立 oracle）则 **10/10 全抓到** | ⏳ **已登记未做**：金丝雀的定位已**下调**为「协议握手自检」并在 README 写明「它不证明什么」；并补了**阳性对照**（未改动的上游 `add.S` 两侧都必须 PASS）关掉「永远报 FAIL」这条假绿通道。待做两种更强形式：① **判决序号覆盖式变异**——对语料每个用例的每个 test number N 生成期望值篡改变体，要求两侧同报 `FAIL N`（把「失败路径」扩成「每个断言一条失败路径」，覆盖**被测指令本身**）；② **候选侧故障注入**——`riscv-golden --fault-inject` 随机翻转一条指令的语义，要求语料必须变红 |
| **D-50** | M-19 | D-46 检视 V-C · 🟡-6 | **M-19 的环形缓冲在生产路径上「只写不读」**：M-18 → M-19 的 `EdgeSink` 握手是通的（边沿确实进了环），但**没有任何人读这个环**——CSV / VCD 导出还没接进契约，前端拿到的波形事件取自 M-18 自己的边沿存储 | ✅ **已闭环（2026-10-22 · v0.20 M-11 落地批）**：**宿主路由**（STAGE-5 裁决 D）——`ExportTrace { source: Waveform }` ⇒ 宿主直接调 M-19 的 `export_csv` / `export_vcd` ⇒ **数据源是 M-19 的环**，不是 M-18 的边沿存储。**M-11 不依赖 M-19**（依赖方向单向，与 LLD §9.1 一致）。**判别力（排除法，最强的一种）**：`waveform_export_reads_the_m19_ring_and_not_the_m18_edge_store` 先**抽干 M-18 的边沿存储**（订阅会推一次波形历史）⇒ 之后导出**仍然**有内容 ⇒ 只可能来自 M-19 的环。⚠️ **v0.20 检视 B 查出的两个判别力缺口已补**：「`Waveform+Vcd` 改成 CSV 795+5 全绿」（⇒ 补 `the_waveform_vcd_export_produces_a_vcd_product` 钉 VCD 形状）与「`overflow_marked` 改成 `false` 795+5 全绿」（⇒ 抽出 `waveform_export_event` 映射点 + `the_waveform_export_event_propagates_the_overflow_marker` 钉住传播）。**证据**：`crates/kernel/src/kernel_host/mod.rs` 的 `export_waveform` + `waveform_export_event` · `crates/kernel/tests/m11_host_wiring.rs`（生产路径 5 条）· `tools/wasm-smoke.mjs`（**真 wasm 产物**上跑 `ExportTrace`） |
| **D-51** | M-18 | D-46 检视 V-A · 🟡-9 | **M-18 的访存 trace 环在宿主里从不被 drain**：`TraceStream` 的上限是 `TRACE_CAP = 200 000` 条，宿主装了总线 hook 之后**每一条访存都进环**，却**没有任何消费者**——M-11（trace 导出）还是 stub。后果：常驻内存 3~5 MB，且**边沿之外的数据全部白攒** | ✅ **已闭环（2026-10-22 · v0.20 M-11 落地批）**：**drain 与转移同时发生**（不是「定期 drain 掉」——那会把 M-11 要用的历史冲掉）：宿主每跑完一个批次就把 M-18 的环**转移**进 M-11 的 `TraceBuffer`（M-11 的环才是历史的持有者）⇒ M-18 的环不再无界增长。**判别式**：`the_m18_trace_ring_is_drained_into_the_m11_ring`（跑完一批 ⇒ `probe_trace_len() == 0` 且 `trace_len() == 3`）+ `the_m18_trace_ring_has_a_consumer_on_the_production_path`（生产路径端到端）。⚠️ **装载镜像产生的宿主侧写被**可见地**丢弃**（`trace_events_discarded()` 计数暴露，不静默）。⚠️ **v0.20 检视 A 查出的「失效通道有损」已修**：M-18 的环是**有界**的（溢出丢最旧）⇒ 它同时是**反汇编缓存的失效通道**（裁决 G）⇒ 环一溢出，写事件被丢掉 ⇒ 缓存条目不失效 ⇒ **反汇编给出旧机器码** ⇒ **`StepN` 按指令预算（10 000）分块**（`step_in_chunks`）⇒ 单块事件数 ≤ 50 000 ≪ 200 000 ⇒ 通道永远无损（判别式 `a_long_step_batch_keeps_the_invalidation_channel_lossless` 断言 `probe_trace_dropped() == 0`）；顺带消掉「`StepN` 无上界 vs `Continue` 一轮 10 000」的上界不对称 |
| **D-52** | M-18 + 内核宿主 | D-46 检视 V-A · 🟡-6 / 🟡-7 | **探针管道装配失败是「静默」的，缺判别力**：内核宿主装管道时，`define_channel` / `attach_signal` 的失败原本被 `let _ =` 吞掉，`attach_peripheral_probes` 因 `try_borrow_mut` 失败跳过的外设也**不吭声**——通道没接上时，波形视图就是空的，而宿主**一声不吭** | ◐ **2026-10-21 推进（D-46 检视轮）**：① 失败改为**计数**（`KernelHost::probe_wiring_failures()`）——可见了；② `attach_peripheral_probes` 增加**运行期重试**（每轮 `Run` 前重试一次，`attach_signal` 对同一 `(source, signal)` 幂等，重复调用不冲突）。⏳ **剩余**：计数器的**失败路径没有对应用例**（健康路径只能断言「计数为 0」）——反向变异 **N8** 实测「不计数」的变异**不会**让任何用例变红，即这条路径的判别力是 0。**待做**：造一个可复现的装配失败（例如让宿主对着一个已占用通道号 `define_channel`），补一条「失败被计数」的用例 |
| **D-54** | 前端 M-01（`app.js`）↔ M-07（传输） | D-46 检视 V-B · 低危 | **探针流订阅没有「断线重连后重订」**：M-10（寄存器流）在传输状态变化时会 `ensureSubscribed()` 重订，而 `wireWaveform`（探针流）**没有** `transport.onStatus` 钩子——传输重连后寄存器流会恢复，探针流**静默死掉** | ⏳ **已登记未做**：**今天没有活影响**——两种传输（worker / websocket）都**不自动重连**（worker 模式不重建 worker；websocket 模式 `onclose` 后不重连），所以「重连」这条路目前走不到。⚠️ 但代码库**明确建模**了重连（M-04 的 `reconnecting` 态、M-10 的「断线重连后重订阅」），P1 做自动重连时这条会立刻变成活缺陷。**待做**：给 `wireWaveform` 传入 `transport` 并挂 `onStatus` 钩子——`connected === false` 时清本地标志，重新连上时**强制**重订 `sim.waveform`（与 M-10 的 `resubscribe()` 同款语义）。⚠️ **V-B 标注「未查完」**：浏览器 **Worker 真死 → 传输重建**这条链**未走通**——`m07_transport.js` 的 worker 适配器**没有**自动重建逻辑（看起来是终态），所以「重连」在 worker 模式下**不可达**；本条按「传输层 `setConnected(false) → true`」这条**可达**路径定级 |
| **D-55** | 前端 `front/dev/mock-kernel.js` ↔ 真内核 | D-46 检视 V-B · 低危 | **mock 内核的探针流语义与真内核不等价**：mock 每次 `StepN` 都推**全量** 1 条边沿，而真内核的 `drain_waveform()` 推的是**增量**（每次取走并清空已取的边沿） | ⏳ **已登记未做**：**今天没有活影响**——mock 只用于 `mock-kernel.test.mjs` 的隔离测试，**没有接前端**（M-07 挂的是真 WASM worker）。但两者的口径差异**没有写进任何文档**，将来若有人拿 mock 当「真内核的行为参考」，就会踩到「全量 vs 增量」这个坑。**待做**：① 在 mock 的探针流处加注释写明「**全量**语义，与真内核的**增量**语义不同，仅供隔离测试」；② 或把 mock 改成增量语义（与真内核对齐），代价是 mock 的断言要跟着改 |
| **D-56** | `tools/browser-verify.mjs` | D-46 检视 V-B · 低危 | **真机复验的两条命令连发、没加等待**：`step` 与 `step10` 是背靠背两次 `Runtime.evaluate`，**没有等上一条完成**。实测通过（x1=0x2a、1 edge），但**未做重复采样**——理论上存在「`step` 未完成 → `step10` 的步数错位」的时序竞争 | ⏳ **已登记未做**：browser-verify **不进默认门禁**（按需运行），所以影响面小。**待做**：在两条命令之间加一个**明确的等待条件**（例如轮询到 PC 前进到预期值再发下一条），或做 N 次重复采样证明它稳定。⚠️ 判据要按项目的规矩来：**「跑了一次通过」不算证据**，要么轮询到确定状态，要么重复采样 |
| **D-53** | M-14 + M-18 + 内核宿主 | D-46 检视 V-A · 🟡-1 / 🟡-4 / 🟡-8 | **三处「静默降级」已修，但其中两处的失败路径仍无判别力**：① **M-18 边沿存储无上限**（不订阅时只进不出，实测堆 30 万条）→ 已加 `EDGE_STORE_CAP`（drop-oldest + 计数）；② **单条波形事件无上限**（实测一条 8 MB 事件）→ 已加 `MAX_WAVEFORM_EDGES_PER_EVENT`（保留最新 + 计数）；③ **一个外设 `tick` 报错会让其余外设错过时钟同步** → 已改成「继续 tick 后面的，报第一个错」 | ◐ **2026-10-21 推进（D-46 检视轮）**：①②③ 三处**都**已修，且**都**配了判别力用例（`the_m18_edge_store_is_bounded` / `a_published_waveform_event_never_exceeds_the_edge_budget` / `a_failing_peripheral_does_not_stop_the_others_from_ticking`），反向变异 **N4 / N5 / N7** 全部变红。⏳ **剩余**：M-18 的淘汰是**成批**的（超上限时丢最旧的 1/4，不是逐条）——批大小 `EDGE_STORE_EVICT` 本身没有用例钉住（改它不影响任何断言）。**待做**：补一条「存储长度在 `[CAP - EVICT, CAP]` 区间内」的断言，把批大小也钉住 |
| **D-70** | 契约 + M-06 / M-07 / M-18 / M-19 / M-20 | v0.17 契约 v2 批 · V-C 检视发现 | **LLD 的错误码表与代码实际发出的码严重脱节**：总纲 §3.4 表16 只逐条列了 **6 个族**（CTX / BUS / ISS / PER / PROTO / DBG），且 §6 开头的规范句写「**所有错误码进 E_CTX/E_BUS/E_ISS/E_PER/E_PROTO 族**」——**与代码直接矛盾** | ✅ **已闭环**（v0.19 · 文档 lane）：**按构造点重算**（不是按文档里的旧数字）⇒ 生产代码发出 **12 族**（CTX / ISS / ENG / BUS / PER / INT / DBG / OBS / SNAP / LDR / TRX / PROTO）+ **2 族「已声明未接线」**（HMS / STORE）；表16 从 **29 码 / 6 族** 补到 **58 码 / 12 族**，并新增 §3.4.1「各族落点与接线状态」（逐族写明落点文件 / 接线状态），§6 规范句同步改成 12 族。⚠️ **v0.19 检视轮补了一个洞**（V-A · F-7）：表16 **漏收 `E_TRX-03`**（生产前端 `front/services/m07_transport.js` 发出它：未配置 WebSocket URL / 当前环境没有 WebSocket / 创建失败）⇒ 已补进（**57 → 58 码**）。**判别力**：本条是**文档与实现的覆盖差**、不是行为缺陷 ⇒ 判别式是**文档核对**（`grep -rhoE "E_[A-Z]+-[0-9]{2}" crates/ front/` 的结果集 ⊆ 表16 收录集），**不需要**新增用例 |
| **D-71** | 文档（M-10 LLD） | v0.17 契约 v2 批 · V-C 检视发现 | **M-10 LLD 的 HTML 标签失衡**：`<style>` **开 2 闭 1**（第 8 行开 / 第 85 行闭 / **第 260 行开、永不闭**）+ `</defs>` **开 1 闭 2**（第 255 行开 / 第 259 行闭 / **第 267 行多余的 `</defs>`**）。HTML 解析器把 `<style>` 当 **raw-text 元素** → **第 260 行到 EOF（第 363 行，104 行）会被整段吞成 CSS**，那段里的 SVG 流程图**不渲染**。⚠️ 该缺陷**既有**（`git show 077ee88^` 已是 2/1），**非本批引入**；且 M-10 是 23 份 LLD 里**唯一**失衡的一份（其余 22 份 `<style>` / `<defs>` 均配平） | ⏳ **已登记未做**（低危 · 既有）：**今天无活影响**——**文字内容完全可读**（文本编辑器 / GitHub 渲染都不受影响），受影响的只是**浏览器里那张流程图不画出来**。⚠️ **不在本批修**：修它会把本批的范围从「订正与重算」拉大到「重排 23 份 LLD 的 HTML 结构」，且与本批的其它改动（内容订正）不同类。**待做**：给第 260 行的 `<style>` 补上 `</style>`、删掉第 267 行多余的 `</defs>`（或把那段 `<style>` 挪到 `<defs>` 之前）；顺带可加一条 CI 检查（23 份 LLD 的 `<style>` / `<defs>` 配平）。**判别力**：判别式是**标签计数**（`<style>` 数 == `</style>` 数、`<defs>` 数 == `</defs>` 数），**不是**用例 |
| **D-72** | M-13 / M-17 | v0.17 契约 v2 批 · V-C 检视发现 | **`StepOutcome::HostPause` 是死接口**：全仓**从未被构造**——`crates/kernel/src/m13_iss/executor.rs:33` 定义它、`crates/kernel/src/m17_backend/mod.rs:161` 消费它（`Ok(StepOutcome::HostPause) => { … }`），但**没有任何生产者**产生它。实际的暂停走 `pause()`，它只置 `last_stop = Some(UserPause)`，**不真正中断**正在跑的批次 | ✅ **已闭环**（v0.19 · M-16 落地批 · 裁决 C）：变体**已删除**；暂停语义改由 M-17 自己的 `pause_requested` 标志位承担（见 **D-59**）。删掉之后 M-17 的 `match` **穷举** `StepOutcome` 全变体、**没有**兜底臂 ⇒ M-13 以后再加变体，M-17 会**编译不过**。⚠️ **全仓零残留**：契约层与**前端 JS 镜像**（`front/lib/message.js`）里都没有任何 `HostPause` 引用（只剩注释与文档）。⚠️ **连带**：删变体暴露的「不可达兜底臂」被一并清掉（否则 clippy 的 `unreachable_patterns` 会红） |
| **D-73** | 契约 + M-17 | v0.17 契约 v2 批 · V-C 检视发现 | **D-57 的判据从来没被真正满足过**：D-57 闭环时留了一条要求——「改契约文档时**必须**同步给 `continue_until_event` 的行为补一条用例，否则『改了文档但没改行为』与『没改文档』同样无法分辨」。但当时补的两条断言（「一轮跑满指令预算」）是**自指**的：用的就是**实现自己用的** `DEFAULT_INSTRUCTION_BUDGET` 常量，把那个常量改成量子配额（1 000）它们**照样全绿** → 「**预算耗尽**」与「**停在量子边界**」在断言下**无法分辨** | ✅ **已闭环（2026-10-21 · `5b89c9a`）**：**判别式改落在「量子配额」上**（「跑过**不止一个**量子」），而**不是**落在「跑满了 `DEFAULT_INSTRUCTION_BUDGET` 条」上。**证据**：① **端口方法层**——`crates/kernel/src/m17_backend/mod.rs` 的 `continue_until_event_runs_the_quantum_main_loop`（断言一轮执行的指令数 **>** 单个量子的配额，即**跨过**了量子边界，而不是「正好等于某个常量」）；② **命令层**——`crates/kernel/src/kernel_host/tests_contract_v2.rs` 的 `the_continue_command_runs_the_budget_not_one_quantum`（同一判别式落在 `CommandPayload::Continue` 这一层）。**判别力**：变异「把预算改成量子配额」→ **两条都变红**（自指断言下它们**不会**红）。⚠️ 顺带把 `crates/contract/src/protocol.rs` 里 `Continue` / `Pause` 两处文档的「量子边界」措辞也订正了（`Pause` 的「下一个量子边界」同样是错的：命令在**静止点**被处理，根本没有「下一个量子边界」可等） |
| **D-74** | M-16 | v0.17 契约 v2 批 · V-C 检视发现 | **`crates/kernel/src/m16_interrupt/mod.rs:5` 的「量子边界」措辞与 D-57 撤回的旧措辞同型**：M-16 还是 stub，但它的模块头已经在用「量子边界」这个**已被契约层撤回**的说法 | ✅ **已闭环**（v0.19 · M-16 落地批）：M-16 模块头与全部相关措辞按 D-57 的新口径（「量子边界是**抢占点**，**不是停止条件**」）重写；**M-16 LLD 升 v1.5**，§3.5 差异对照表补两行（「`mie` / `mip` 位号」与「平台自定义可写 CSR `irq_clear = 0x7C0`」），§3.4 标注 CLINT 的 MMIO 通路为 P1。⚠️ **判别力**：本条是**文档措辞**、不是行为缺陷 ⇒ 判别式是**文档核对**（`grep -rn "停止条件" crates/kernel/src/m16_interrupt/` 零命中），**不需要**新增用例 |
| **D-75** | 内核宿主 ↔ 前端 `front/lib/message.js` | v0.17 契约 v2 批 · V-A 检视发现 | **信封版本校验两侧语义不对称**：内核 `crates/kernel/src/kernel_host/mod.rs:1056` 的 `handle_envelope` 用**严格等号**（`msg.schema_version != SCHEMA_VERSION` → 拒），而前端 `front/lib/message.js:558` 的 `decodeMessage` 是**前向兼容**（`schema_version < SCHEMA_VERSION` → 拒，**高于就放行**）。于是 v3 宿主向 v2 内核发消息会被内核拒（`E_PROTO-01`），而 v3 内核发来的事件**前端却放行** | ⏳ **已登记未做**（低危 · **非本批引入**）：**今天无活影响**——两侧都只跑同一版，等号与「低于拒」**同值**。⚠️ **内核侧的严格等号是 v0.13 第 1 阶段 V-A 检视（`aa75535`）定的**，理由写得很清楚：契约的 serde 派生**没有** `deny_unknown_fields`，一个来自更高版本 schema 的消息会被**静默地**少解析几个字段——那比直接拒绝糟得多。⚠️ **v0.17 新增 `Negotiate` 后，这个不对称更值得记一笔**：官方协商手段已经存在，但两侧对「能不能读懂」的判定**不用同一个口径**。**待做**：随 **P1 的断线续拉 / 版本协商**一起定——要么前端也改严格等号（与内核对齐，代价是失去「内核向前兼容」这一侧的前向能力），要么在**桥规格 §4** 写明「**内核侧要求严格等于 `SCHEMA_VERSION`；前端侧允许更高版本**」并把两侧口径都写进 `Negotiate` 的文档。⚠️ **不要在 P1 之前单方面改任一侧**——那样会让「谁能跟谁说话」变成一个**两边都不认**的事实。**判别力**：判别式是**文档核对**（`grep -rn "schema_version" front/lib/message.js crates/kernel/src/kernel_host/mod.rs` 两侧口径对照），**不需要**新增用例。⚠️ **v0.20 批（契约 v2 → v3）再触一次**：两侧口径**一字未改**，但「今天无活影响」的**依据**要跟着改（v0.17 时是「都只跑 v2」，现在是「都只跑 v3」）⇒ 本批**复核了两侧的行号口径**（内核 `!=` / 前端 `<`）并**确认不对称仍在** ⇒ **仍按低危留账**，但**升版流程要保留这道复核**（每升一版都重核一次两侧口径，别让「今天无活影响」这句话悄悄过期） |

| **D-79** | 变异自检工具链 | v0.18 会话快照批 · V-B 检视 · C-5 | **某些 lane 的变异驱动器「还原」既不逐字节校验，快照也不是最终树**：M-14/15/16 lane 的 `restore()` = `shutil.copy2(PRISTINE/f, dst)` + `os.utime`——**没有 md5 校验**；而它的 `pristine` 快照停在 `6eaf859` **之前**那一版（`E_PER_04_UNSUPPORTED_SESSION_STATE` 还在内核本地定义，没改成从契约层 `pub use`）。**后果**：被改坏的只有**常量归属**，**被突变的函数在两版之间逐字节相同** → 那 26 条变异的**结论仍然成立**，但方法论上有洞——「还原成功」是**假定**的，不是**验过**的 | ⏳ **已登记未做**（低危 · **方法论级**）：⚠️ **父会话本轮真踩了同一个坑的另一个变体**——父会话自己的变异脚本用 `f"{mid}-{path.name}.bak"` 做备份名，K5 一条变异同时改 `m14_bus/mod.rs` 与 `kernel_host/mod.rs`（**两个文件都叫 `mod.rs`**）→ **备份名撞车** → 后一个备份**覆盖**前一个 → 还原时把**内核宿主的内容写进了 M-14 的文件**。**是 md5 断言当场抓住的**（`AssertionError: 还原后 md5 不一致`），`git checkout --` 复原。→ 这条台账**不是**说「工具链有 bug」，而是把**这条纪律的理由**钉死：**没有逐字节校验，损坏会被当成「还原成功」送进下一次变异**，而且**可能一路绿到最后**。**待做**：① 所有变异驱动器的备份名**必须**用**相对路径**做键（不能只用 `path.name`）；② 还原后**必须** md5 逐字节校验；③ 变异快照**必须**取自**最终树**（`git show HEAD:<path>` 而不是手抄的 `pristine/`）。⚠️ 本批已把父会话自己的脚本修好（`/tmp/probe/k_mut.py`），**契约层与宿主 lane 本来就做了 md5 校验** ✓。**判别力**：判别式是**工具链自检**（跑一遍「故意让备份名撞车」的变异，确认 md5 断言**会响**），**不是**用例 |
| **D-80** | 端到端夹具（`crates/kernel/tests/session_snapshot.rs`） | v0.18 会话快照批 · V-B 检视 · D-3 | **端到端场景不含 UART / 定时器 / ROM**：demo 内存图 = 低段 RAM 1 MiB + 高段 RAM 4 MiB + GPIO 窗口——**没有 ROM 块**（固件被装进**低段 RAM 的 `0x0`**），**没有 UART / 定时器**。于是规格 §3.2 的「ROM **原地保留**」与 §3.3 的「UART / 定时器状态」在**端到端等价会话**这一层**一次都没被走到**（变异 A2「重建 ROM 时清零」只有 2 条 M-14 单测红、A8「定时器不还原」只有 2 条 M-15 单测红、A18「UART 状态不还原」只有 3 条 M-15 单测红） | ⏳ **已登记未做**（低危 · **故意为之**）：⚠️ **不是漏做，是有理由的**——端到端夹具**只有 GPIO**，UART / 定时器**根本不在图里**，固件**不访问**它们 → 状态**恒为初值** → 加进端到端只会制造**恒真断言**（断言「恢复前后都是初值」——它**恒真**，不区分任何东西）。⚠️ 当前树新增的 `attach_uart` 已让宿主层**能挂 UART**（并已有一条畸形 UART 的宿主级用例），**UART 这一格在宿主层补上了**；**ROM / 定时器仍缺**。**待做**：要给端到端补 ROM / 定时器，**前提是固件真的用它们**（比如固件往一块**真 ROM** 里写、周期性改 `mtimecmp`）——否则仍然是恒真断言。**判别力**：判别式是**「这条用例会因为『状态没被还原』而红吗」**——用 A2 / A8 / A18 三条变异实测 |
| **D-81** | 契约层 `session.rs` | v0.18 会话快照批 · V-B 检视 · H-2 附带 | **契约层的「内存页越过 u32 边界」规则与「不落在任何 RAM 区域内」报同一个码 `E_SNAP_06`**——只按**码**判别时**分不出是哪条规则拒的** | ⏳ **已登记未做**（低危 · **新码 vs 耦合文案的两难**）：契约层 `validate()` 的两条页级规则——① `p.base.checked_add(len)` 越界（内存页越过 u32 边界）② 页不落在任何 RAM 区域内——**报同一个场景码**。按「**一场景一码**」原则，严格说它们该是**两个**码；但新开一个码意味着**契约层升版**（`E_SNAP_07` 要进错误码清单 → 前端镜像要跟）。⚠️ **Lane E 的处置**：**不加**这条用例——按**码**判别不出来，只能按**报错文案**判别 → **耦合措辞**（改一次文案就假红）。⚠️ 「按文案判别」在本项目是**明确的反面教材**（多处「错误码表只收录 6 族」的欠账都是因为文案被当成了接口）。**待做**：随**下一次契约层升版**一并定——要么新开 `E_SNAP_07`（**一场景一码**），要么把两条规则**合并成一条**（「页必须完整落在某个 RAM 区域内」一条规则同时覆盖越界与不落区内）。**判别力**：判别式是**错误码清单的核对**（`E_SNAP_01..07` 与 §5 的 10 类一一对应），**不需要**新增用例 |
| **D-82** | M-14 + M-16 | v0.19 M-16 落地批 · 裁决 E | **CLINT 的 `mtime` / `mtimecmp` 固件侧 MMIO 通路推迟到 P1**：LLD §3.4 要求固件经 **M-14 MMIO 区间**访问 `mtime` / `mtimecmp`（CLINT 语义），本批**不做** —— 落它要**同时**动 M-14 的区域类型（新增 `RegionType::Clint` + 一条派发路径）与**契约层的区域 kind**（`M14MapDto.region.kind` 是 `String`，但 `SessionSnapshotDto::validate()` 按 `"ram"` / `"rom"` / `"peripheral"` 分类做检查）⇒ 契约层刚升 v2 不到一批，再动它会把本批从「落地一个模块」拉成「改会话载荷的形状」 | ⏳ **未做**（P1 · **范围裁剪**，不是缺陷）：⚠️ **今天没有活影响** —— 固件侧**没有消费者**（demo 固件不写 `mtimecmp`），MMIO 通路**今天没有生产者**。⚠️ **定时器中断的语义本批照样闭环**：`mtime` 随虚拟时间推进、`mtimecmp` 由**宿主 API**（`set_timer_compare`）设置、`mtime >= mtimecmp` 时按**电平**语义挂起 line 3 ⇒ **仲裁 → 交付 → 陷入**的整条链**可端到端验证**；缺的只是「固件自己写 `mtimecmp`」这一步。**待做**：随 **P1 的固件侧中断支持**一起做 —— M-14 新增 CLINT 区域类型 + 一条派发路径 + 契约层区域 kind 的新取值（**要评估是否升 `schema_version`**）。**判别力**：判别式是**文档核对**（M-16 LLD §3.4 已标注「P1」），**不需要**新增用例 |

| **D-83** | 测试 / 变异工具链 + M-12 `Engine::run_until` | v0.19 M-16 批 · V-A 检视 · 自查 | **活锁表现为「挂死」而不是「失败」**：① **工具链侧** —— 变异驱动器跑 `cargo test` **没有超时** ⇒ 某个变异让实现活锁时，测试二进制**永不退出** ⇒ 整个变异自检会**永远卡在那里**（既不是红也不是绿，是「没结论」）。本批**实测命中一次**：变异「`take_intr` 取走不消失」⇒ 同一中断被反复交付 ⇒ `Engine::run_until` 的内层循环**不退出** ⇒ 测试二进制跑了 5 分 50 秒未结束；**上一批还留下了 3 个 18 小时的孤儿进程**（同样是 `d07_time_driven` 挂死，没人发现）。② **生产侧** —— `ExecOutcome::InterruptTaken` **不消耗指令预算**、**不推进虚拟时间** ⇒ M-12 的「不会空转」论证**只写在注释里**（「交付由 M-16 的 `active_irq` 单级防护限死 ⇒ 一轮里最多一次交付」），**没有守卫**；一旦那条不变式破掉，内层循环无界自旋。 | ✅ **已闭环（2026-10-22 · v0.22 批 · Lane W2）**（**工具链侧与生产侧都已修**）——⚠️ **生产侧的修法是「无进展守卫」**：M-12 的唯一主循环 `run_until` 现在带 `NO_PROGRESS_WINDOW = 1024` 的停滞检测，连续 1024 次「既没有指令退休、虚拟时间也没有前进、也没有到期事件被消费」即**报错退出**（`E_ENG_03_NO_PROGRESS`，**平台故障**，与 guest trap 严格区分）⇒ 「活锁」在生产路径上**变成失败**而不是挂死（VD-F3）。**判别力**：基线那条活锁用例单跑 **GREEN 且 0.00 s 自己退出**，而「拆掉守卫」与「阈值改成 `u32::MAX`」两条变异**确实挂死** ⇒ 「基线不挂死 + 变异挂死」构成完整证据。⚠️ **残留**（不影响本条闭环）：M-17 的 `step_n` 有**同形状**的活锁落点 ⇒ 登记为 **D-114**；watchdog 的其余三块（宿主时间预算 / 滑动窗口三次确认 / HMS 同步终止与 `EV_WATCHDOG` 日志）⇒ 登记为 **D-115**。⚠️ **一条工具事实**：`perl -e 'alarm N; exec(@ARGV)'` 包装**实际不生效**（POSIX 规定 `exec` 会**重置 alarm**）⇒ 正确写法是「父进程**不 exec**、只 **fork**」，超时后按**进程组** `SIGKILL`。**原始条目**：✅ **工具链侧已修**（本轮共修**五处**驱动缺陷）—— ① **硬超时**（默认 600 s，可由 `MUT_TIMEOUT` 调）＋ 超时**杀整个进程组**（`killpg`：只杀 cargo 会把测试二进制留成孤儿）＋ 超时判为 `HANG`（**不算判别力**，但**是一条真发现**）＋ 驱动器**打印完整红集**；② **红集比对**改成「红测名 ∋ 期望串」的 **AND** 语义；③ **备份键**用相对路径（两个 `mod.rs` 曾互相覆盖内容）；④ **超时口径把「编译」与「运行」分开**（`cargo build --tests` 单独走、给它宽松的 1200 s；`cargo test` 的超时**只覆盖运行**）—— 否则**编译慢**会被误判成活锁；⑤ **HANG 判据改用输出本身**：数落盘输出里 `Running <target>` 与 `test result:` 的个数 —— **相等** ⇒ **假 HANG**（所有 target 都已跑完、卡在**退出**阶段 ⇒ macOS **XProtect** 扫描**每次变异都重新链接**的 doc-test 二进制时偶发堵塞 ⇒ **没结论**，必须重跑）；**不等** ⇒ **真 HANG**（某个测试二进制挂死）—— 并用 `cargo test --lib --tests` **跳过 doc-test**（doc-test 只跑文档示例，**不覆盖**本批任何一条变异 ⇒ **不损失判别力**，只消除这一类假阳性）。⚠️ **实证**（本轮最强的一条）：**同一条变异 K-19** 两轮两个结果（`RED ✓` / `HANG`）⇒ 旧口径的 HANG 判定**整体不可信**；而 **K-10** 在新口径下判**真 HANG**，且能指出**确切的挂死点** —— cargo 自己打印 `test kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest has been running for over 60 seconds` ⇒ 挂死链路 = M-16 `take_intr` 取走**不**消失 ⇒ 同一交付被反复交付 ⇒ 引擎内层循环不退出。⚠️ **「挂死」这个结论，必须能指出「是哪个测试挂死的」** —— 否则那只是「cargo 没退出」。⚠️ **收口轮又把「假 HANG」的根因查实了**（下面 ⑥⑦ 是本轮**新增**的两处工具链修复）：⑧ **最终 HANG 判据**改成**看二进制有没有启动** —— 末个 target 的二进制**自己打印了** `running N tests` **没有** ⇒ **假 HANG**（macOS 对**刚链接**二进制的安全扫描堵住了 `exec` ⇒ **与被测代码无关**）；**有** ⇒ 二进制**启动过** ⇒ 再看 cargo **点名**了哪个测试 ⇒ **点了** ⇒ 确凿活锁；**没点** ⇒ 超时未跑完（**可能只是慢**）⇒ **决定性反证**：末个 target 若是 `riscv_sim_server`（`unittests src/main.rs`）或 `riscv_golden`（**0 个测试**）⇒ 它**不可能**挂死 ⇒ 本轮 8 条「HANG」里 **7 条是假的**（旧口径把「所有 target 都跑完、卡在退出阶段」也算成 HANG，**判据本身**就不够细）。⑨ **规模侧**：实测 macOS 对**每个新链接二进制**的首次 `exec` 阻塞 **≈25 秒**（12 个二进制从 3.2 MB 到 8.9 MB **都是 23~26 秒** ⇒ **与体积无关**，是**每文件固定成本**）⇒ 一轮 12 个二进制 = **300 秒** ⇒ **正好打满**旧的 300 s 窗口 ⇒ **系统性**产出假 HANG ⇒ 三招缓解：**收敛 target 集**（44 个期望红测逐条 `grep` 核实：41 个在 lib、3 个在 E2E、**零个**在别处 ⇒ 每轮只重链 3 个 ⇒ 300 s → **75 s**）· **预热**（正式跑前先 exec 一次 `--list` ⇒ 25 s → **0.02 s**）· 运行超时**放宽到 900 s**。⚠️ **走不通的两条**（别再试）：`xattr -d com.apple.provenance` **无效**（**受保护的系统属性**，删完 `xattr -l` 仍在 ⇒ 首次 exec 照样被堵，实测 1m44s）；`strip` 掉调试信息也**没用**（扫描成本与体积无关）。⚠️ **生产侧（M-12）本批不动**：M-12 **至今无独立模块级 review**（台账 **D-30**），改 `run_until` 的终止性要连同 D-07 刚闭环的 31 条用例一起重验 ⇒ 留给 **D-30 那一轮**。**待做**：① 给 `Engine::run_until` 的内层循环加**进度守卫**（「本轮既没有指令退休、时间也没前进」⇒ 判定为停滞并**报错**退出，而不是无限自旋）；② 给 `cargo test` 套一层**超时包装**（CI 侧），让活锁在 CI 上表现为**失败**而不是**卡死**。**判别力**：判别式是**「挂死必须变成失败」**—— 变异 K-10 的期望结论是 `HANG`（已登记在变异集里）
| **D-84** | 契约层 `csr.rs` + M-13 + M-16 | v0.19 M-16 批 · V-A F-2 / V-B F-5 | **guest 读 `mip` 恒得 0**（`mip` = `0x344` 全仓**从未被写入**）：STAGE-4 §1.2 裁决 D 与 M-16 LLD §3.5 都声称「`mie_bit_for_line` 是唯一互译点，**两处**（M-16 刷新使能、**M-13 写 `mip`**）都从它取」——**第二处根本没实现** | ✅ **已闭环（v0.21 · Lane A · 裁决 B / C）**：M-16 的**挂起队列**经契约层 `pending_lines_to_mip_mask` 投影，在 **M-17 写侧**落进 CSR 文件；**两个刷新点**（量子边界 `poll()` 之后 / `irq_clear` 副作用之后），**值变化才写**；位号与 `mie` **同源**（`std_irq_bit_for_line`：MEIP = 11 / MTIP = 7，线 1 与线 2 共享 MEIP 位）。⚠️ **显式偏离台账原措辞**（「要动 M-13 的 CSR **读**路径」）——理由写进 STAGE-6 裁决 B：读侧要么给 `SimState` 加字段（= **第二落点**）要么改 `csr_read` 签名（≈ 10 个调用点）；且平台的 `mip` 写**不能**经 M-13 的「刚写了哪个 CSR」回报，否则会被当成 `irq_clear` ⇒ **清错线**。⚠️ **残留语义**：guest 读到的是**上一个量子边界**的挂起快照（1 µs = 1000 条指令），**不是精确值** ⇒ 已写进 M-16 / M-17 的 LLD；会话快照里 `mip` 有**两个落点**（段 5 的 pending 队列权威，段 1 的 `mip` 是派生缓存）⇒ 还原后**下一个量子边界**重新投影。**判别力**：5 条 M-17 端到端用例 + 3 条反向变异（删刷新点① / 去掉「值变化才写」/ 删刷新点②）逐条变红 |',
| **D-85** | 黄金对拍语料 | v0.19 M-16 批 · V-A F-5 | **黄金语料对本批的新代码**零覆盖**：语料清单 `tests/golden/corpus.txt` **明写**它排除 `rv32mi` / `rv32si` 的理由是「需要 trap delivery 和 `mret`，**那正是 M-16 在第 5 阶段落的**」——**本批就是那个第 5 阶段**，但语料**没有扩** ⇒ 51 例全为 `rv32ui` / `rv32um` / `rv32uc`，**零条 CSR 指令** ⇒ 本批新增的 6 条 CSR 指令 / `mret` / 陷阱投递 / `mtval` 写入，**对拍一条都覆盖不到** | ✅ **已闭环（v0.21 · Lane B · 裁决 F / G）**：新增 **CSR-capable env 变体** `tests/golden/env/csr/riscv_test.h` + `tests/golden/env/encoding.h`（**不动**既有的零 CSR env）；`corpus.txt` 增「env 变体」列（既有 51 行**纯增量**未动）⇒ 语料 **51 → 64**（新增 13 条 `rv32mi` `active`）。⚠️ **重实现的等价性是验过的**：16 例的 `.text.init` 与上游 `env/p/riscv_test.h` 生成的**逐字节一致** ⇒ 两侧跑的是**同一个程序**。⚠️ **残留（诚实标注）**：13 例里有 **3 例是「空洞的一致」**（判决相同但**走的不是同一条分支** / 在我们侧**恒真**）⇒ 已逐条标注在 `tests/golden/README.md` ⇒ **D-96** |',
| **D-86** | M-13（取指宽度）↔ M-11 | v0.20 M-11 批 · 检视 A · 次要 6 | **M-13 的执行取指恒读 4 字节，M-11 的反汇编取指「先 2 后 4」**：⇒ 映射区**末端**只有 2 字节的压缩指令，**反汇编能解出来、访客取指却失败**（M-14 的 `crosses_region` 判跨界 ⇒ `E_BUS-01`）⇒ **面板显示的指令访客执行不了** | ⏳ **已登记未做**（P1）：⚠️ **M-11 侧的取指口径是对的**（LLD 11-1：必须真的只读 `size` 字节）⇒ 不一致在 M-13 ⇒ 要改的是 M-13 的取指宽度判定（先读 2 字节判 `0b11`，压缩指令只读 2 字节）。**判别力**：落地前可用「在映射区末端放一条压缩指令并让它被执行」的端到端用例；⚠️ 本批只把这条**如实登记**进规格 §9.8（Wave 1 的已知边界） |
| **D-87** | M-11 ↔ M-18 | v0.20 M-11 批 · 检视 A · 重要 3 | **访存轨迹的机器码可能「时间倒流」**：取指事件的 `data` 是**批次结束时**重取的码 ⇒ 同一批次内访客自改代码时，**第一条**取指记下的是改写**后**的值 | ⏳ **已登记未做**（P1）：⚠️ **根因与「访存轨迹不含访存值」同源**（M-18 的环**不记值**）⇒ 要修得改 **M-18 的环布局**（让取指事件带上机器码）⇒ 与该 P1 合并做。**已如实登记**进规格 §9.8 第 6 行 |
| **D-88** | 前端 M-02（`m02_viewcomponents.js`） | v0.20 M-11 批 · 检视 B · D-6 | **前端的「导出」没有「下载」动作**：导出结果只落在 `<pre>` 里（全仓 `front/` 无 `download` / `Blob` / `createObjectURL`）⇒ 用户得**手动复制** ⇒ M-11 LLD 的「分块下载」措辞**未兑现** | ⏳ **已登记未做**（P2）：⚠️ **不违反规格**（STAGE-5 §5.2 验收 #20 只要求「拿到 `chunk`」⇒ 展示即合规）⇒ 但 LLD 的措辞与实现**不一致** ⇒ 二选一：**补下载**（Blob + `<a download>`，前端门禁要补断言）或**把 LLD 措辞改成「展示」**。**已如实登记**进规格 §9.8 第 8 行 |
| **D-89** | 规格 STAGE-5 裁决 G | v0.20 M-11 批 · 检视 A · 次要 4 | **裁决 G 的「只失效被写地址」在**部分写**场景下粒度不足**：写 `X+2` 不会删掉 `cache[X]` ⇒ `Disasm(X)` 仍给旧码 | ⏳ **已登记未做**（P2 · **规格问题不是实现走样**）：⚠️ 裁决 G 的**原话**就是「只失效被写地址」⇒ 实现**照做了** ⇒ 要改得**先改裁决**（例如「失效覆盖该地址所在的 4 字节对齐块」）。**已如实登记**进规格 §9.8 第 7 行。**影响面**：只影响**跨指令边界**的部分写（自修改代码的常见写法是对齐写 ⇒ 命中概率低） |
| **D-90** | M-11 符号表 ↔ `crates/kernel/src/bin/riscv-golden.rs` | v0.20 M-11 批 · 收口轮复核台账 🟡-4 的**残留** | **`riscv-golden` 自带一套迷你 ELF 符号读取器**（`find_symbol`，直接切 `.symtab` / `.strtab` 偏移）⇒ 与 M-11 的 `symbols.rs` **重复实现** ⇒ 两条路径可能**漂移**（M-11 已支持 `.symtab` 的名字/大小/类型，M-06 负责容器解析） | ✅ **已闭环（v0.21 · Lane 0d）**：`find_symbol` **已删** ⇒ 符号查找改走 **M-06（容器解析 + 带出 `.symtab`/`.strtab` 原始字节）→ M-11 `SymbolTable::from_sections` → `lookup`** ⇒ 全仓**只剩一处生产符号读取者**（`grep -rn "SHT_SYMTAB" crates/` 只命中 M-11 与测试夹具）。⚠️ **残留**（已并入 D-101 的同类备注）：M-11 按 ELF32 **固定 16 字节**步长读条目、**不校验**节区头自报的 `sh_entsize` ⇒ 谎报该值的文件仍会被读出符号（**宽松解析**，非崩溃风险） |

| **D-91** | 契约层 `csr.rs` + M-13 `csr.rs` | v0.21 CSR 黄金覆盖批 · 探针实测 | **平台没有 `misa`** ⇒ guest 读 `misa` 恒得 0 ⇒ guest 无法知道平台支持哪些扩展（MXL / IALIGN）⇒ 固件按 `misa` 选路径会走错 | ✅ **已闭环（v0.21 · 裁决 E）**：契约层加 `CSR_MISA` / `CSR_MISA_VALUE`（**0x40001104** = MXL=1 + I(8) + M(12) + C(2)），M-13 的 `csr_read` **特判**返回它（**不预置进 CSR 文件** ⇒ 保住 `EngineSnapshotDto::is_empty()` 的「空快照」判定）；`misa` 对 guest **只读**（写被忽略）。⚠️ **残留**：参考侧（Spike 默认 `--priv=MSU`）的 `misa` = **0x40141104**（**多出 S(18) 与 U(20)**）⇒ 两侧**不逐位相同** ⇒ **只有 MXL 与 I/M/C 位可比**；且参考侧的 `misa` 是 **WARL 可写**而我们**完全只读** ⇒ **D-95** |
| **D-92** | 契约层 `csr.rs`（`csr_write_is_ignored`） | v0.21 CSR 黄金覆盖批 · 探针实测 | **未实现 CSR 的写被存了下来**：平台原来把写**存进 CSR 文件**并原样读回 ⇒ guest 能**凭空造出**平台没有的寄存器。参考侧写 `tdata1` 读回 **0** | ✅ **已闭环（v0.21 · 裁决 D）**：契约层建立「**已实现 CSR 表**」（`csr_is_implemented`），未实现的 CSR **写被忽略、读回 0**。⚠️ **残留**：**读**侧与参考侧仍有差异 —— 参考侧对**当前 ISA 里根本不存在**的 CSR（如未启用 Zicntr 时的 `cycle`）会**陷入非法指令**，本平台一律读回 0 不陷入 ⇒ **D-94** |
| **D-93** | M-13 `executor.rs`（`mret` 臂） | v0.21 CSR 黄金覆盖批 · 静态检视 | ⚠️ **原台账口径是错的**（原文「把 `mepc[1:0]` 当作零」是 **IALIGN=32** 的口径，与本平台对不上）：平台 `misa = 0x4000_1104` 声明支持 **C 扩展** ⇒ **IALIGN = 16** ⇒ 正确的掩码是**只清最低 1 位**。本条按**参考侧实测**重订 | ✅ **已闭环（v0.22 · Wave 2 · `a13d058` + `9a57022`）**：**三条路径各管各的**（**每条都有参考侧实测支撑**）。① **用户写** `csrw mepc` ⇒ 掩 `~1`（契约层 `apply_warl` 的 `CSR_MEPC => value & MEPC_ALIGN_MASK`，`crates/contract/src/csr.rs:142`；掩码常量 `MEPC_ALIGN_MASK = 0xFFFF_FFFE`，`:208`）。② **陷入门**写 `mepc` ⇒ 写**精确 PC**、**不丢 bit 1**（走 `csr_write_raw`，`crates/kernel/src/m13_iss/csr.rs:115`）—— 陷入必须能**回到出错的指令**。③ **`mret` 取 PC** ⇒ 再掩一次 `~1`（`csr::mepc_aligned`，`:170`）⇒ **幂等**。**实测证据**（Spike 1.1.1-dev，探针在仓外）：压缩 `c.sw` 位于 `0x8000014a` ⇒ 陷入门写出的 `mepc = 0x8000014a`（**bit 1 保留**）；用户 `csrw mepc, 0x1003` ⇒ 参考侧**读回 `0x1002`**；`mret` 落回 `4k+2`（**掩 `~1`，不是 `~3`**）。**判别力**：4 条新用例（`mret_clears_only_the_lowest_bit_of_mepc` / `mret_clears_the_lowest_bit_of_mepc_when_it_is_set` / `a_user_write_to_mepc_is_masked_by_ialign` / `the_trap_entry_writes_the_exact_faulting_pc_into_mepc`），断言值一律写**架构字面量**（不从掩码常量推导，否则改掩码期望值会跟着动、用例就成恒绿）⇒ 4 条反向变异**逐条**变红。⚠️ **如实登记的不可观测边界**：用户写与陷入门这两条路径在**合法指令地址**上**无法区分**（每条合法地址都是偶数，`~1` 的掩码动不了它）⇒ 参考侧在陷入门那条路径上究竟是写精确 PC 还是写被掩的 PC **无法证明**；平台用原始写只是**把意图显式写下来**，**不是**一条有实测支撑的差异 |
| **D-94** | 平台整体（M-13 / M-16 / M-20） | v0.21 CSR 黄金覆盖批 · 逐例实测 | **S 扩展 / PMP / 计数器缺失**：① 无 S 模式 ⇒ `rv32si` 6 例全部分歧（`csr` → `FAIL 22`、`sbreak`/`scall` → `FAIL 669`、`wfi` → `FAIL 668`、`dirty` **连汇编都过不了**、缺 `SATP_MODE`/`SATP_MODE_SV32`）② 无 PMP ⇒ `rv32mi/pmpaddr` 分歧（我们 `FAIL 1`）③ 无 Zicntr ⇒ `rv32mi/zicntr` 分歧（Spike `FAIL 2` / 我们 `PASS`）④ **未实现 CSR 的读语义比参考侧宽松**（不陷入）⑤ `medeleg`/`mideleg` 参考侧**实现了** delegation（写被掩码后存下 `0x22`），我们读回 0 | ⏳ **已登记未做**（P1）：逐例**实测**分歧写进 `tests/golden/README.md` 的「不入库清单」；本批**刻意不**把这些行放进 `corpus.txt` —— harness 的 `pending` 规则**只**容忍「候选侧跑不了」（`ERROR` / `NOVERDICT`），而其中两例**跑得了、只是分歧** ⇒ 硬塞进去就得把规则放宽成「任何分歧都算预期缺口」这个洞。⚠️ `rv32si/ma_fetch` 两侧**都 PASS**但是**空洞的一致**（两侧都没走到它要测的 S 模式路径）⇒ **不得**提升为 `active` |
| **D-95** | 契约层 `csr.rs` + M-13 `csr.rs` | v0.21 CSR 黄金覆盖批 · 检视 A · P2 / 检视 B · F-3 | **`misa` 的写语义与参考侧分歧**：RISC-V 规定 `misa` 是 **WARL 且可写**（合法写改变受支持的扩展位）；平台按裁决 E 做成**完全只读**（写被忽略）。实测：参考侧 `csrci misa, 4` **真的清掉 C 位**（读回 `0x40141100`），我们读回仍是 `0x40001104` ⇒ 上游 `ma_fetch.S` 的「C 位能不能关掉」探针在**两侧取相反分支** ⇒ **判决一致但覆盖不等价** | ⏳ **已登记未做**（P2 · **平台取舍，不改**）：⚠️ **不改的理由**：放行 `misa` 的写 ⇒ guest 可以清掉 C 位，而平台的取指**确实**支持压缩指令 ⇒ `misa` 会**说谎**（比只读更糟）。⚠️ **二选一留待下一批**：① 实现 WARL 写（只在「只改已实现扩展位」时放行，约 3 行）② 保持只读并**如实记为与参考侧的分歧**（✅ 本批已做：契约层注释 + STAGE-6 裁决 E + README 的空洞标注）。**判别力**：行为用例「`csrci misa, 4` ⇒ 读回什么」——两条侧各一条 |
| **D-96** | 黄金语料（`tests/golden/`） | v0.21 CSR 黄金覆盖批 · 检视 A · P1 / P3 | **三条「空洞的一致」被当成 `active` 覆盖**：`rv32mi/csr`（参考侧**进 U 模式段**、我们直接跳 `finish` ⇒ 特权级路径一次没跑）、`rv32mi/ma_fetch`（`misa` 写语义分歧 ⇒ 参考侧进 IALIGN 段、我们跳过）、`rv32mi/instret_overflow`（平台**没有计数器** ⇒ `minstret` 恒读 0 ⇒ 用例的 `TEST_CASE(2/3/4)` **恒真**）。⇒ 规格自己写「**空洞的一致比分歧更危险**」却没把这三条适用上 | ✅ **本批已标注**（P2 · **留账的是「自动检出」**）：三条已在 `tests/golden/README.md` 的「`active` rows that agree **vacuously**」小节**逐条标注**（含判别式与实测证据）⇒ 覆盖率**不再被高估**。⏳ **未做**：**机器可检**的「空洞一致」探测器 —— harness 比对两侧的**执行指令数 / 陷阱序列**（参考侧可用 `spike -l --log-commits`），差异超阈值即报「空洞一致」。**判别力**：判别式是**「两侧执行的指令数差异超阈值 ⇒ 报出来」**（今天靠人读规格） |
| **D-97** | M-13 `csr.rs`（`enter_trap`） | v0.21 CSR 黄金覆盖批 · 检视 B · F-1 | **陷入时 `mstatus.MPP` 没被置 M**：`enter_trap` 只写 `MPIE` / `MIE` ⇒ guest 在 handler 里读 `mstatus` 看到 `MPP = 0`。实测（**同一个 ELF 两侧**，探针在 handler 里读）：**参考侧 `0x0018_0080`（`MPP = 3`）/ 我们 `0x0000_0080`（`MPP = 0`）** ⇒ 与 M-13 LLD §5.2.4 的明文「`MPIE←MIE，MIE←0，MPP←M`」**直接相悖** | ✅ **已闭环（v0.21 · 收口轮）**：`enter_trap` 在写 `MPIE`/`MIE` 的同一步里写 `MPP ← M`（`MSTATUS_MPP_M << MSTATUS_MPP_SHIFT`）；契约层加 `MSTATUS_MPP_SHIFT` / `MSTATUS_MPP_M`（**唯一真相**）。⚠️ **`MPP` 是 WARL**：平台只实现 M 模式 ⇒ 合法值只有 3 ⇒ 陷入时**写**它（不是保留 guest 写的值）。**判别力**：2 条 M-13 单测（陷入后 `MPP == 3`；陷入**覆盖** guest 预置的 `MPP`）+ **探针复测**（两侧退出码都是 4 ⇒ `MPP = 3`）|
| **D-98** | M-13 `csr.rs` + 契约层 | v0.21 CSR 黄金覆盖批 · 检视 B · F-9 | ⚠️ 原描述保留；**注意区分两类差异**：① **保留位掩码**是**架构要求**（本条）② **`mret` 后的 `MPP`** 是**能力差异**（参考侧支持 U/S ⇒ 它的 `MPP` 合法值有 3 个；我们只实现 M ⇒ 只有 1 个）—— ② **不是**缺陷 | ✅ **已闭环（v0.22 · Wave 2 · `a13d058`）**：**`mstatus` 加 WARL 掩码**，保留位写被忽略。掩码常量 `MSTATUS_WARL_MASK = 0x807F_FFEA` 住在**契约层**（`crates/contract/src/csr.rs:186`），M-13 侧**只引用不复制字面量**（唯一真相）。⚠️ **刻意不照抄参考侧实测的 `0xAA797E80`** —— 照抄会让**保留位读出 1**，**违背规则本身** ⇒ 按 20191213 版布局定。**判别力**：契约层逐位断言（**钉住 bit 1**）+ M-13 侧精确读回；反向变异时**当场查出该 lane 自己写的两条恒绿用例**（契约用例没钉 bit 1；精确读回用例选的数值 `0x1232` 掩不掩都一样）⇒ 已改成能真正分辨的形态。⚠️ **已知逐位残留**（与参考侧 `0xAA797E80` 仍有差异）**如实留账**，不假装一致。⚠️ **`mstatus` 变 WARL 的连带影响**：原先拿 `mstatus` 当**普通存储载体**的六条 CSR 指令语义用例改用 `mscratch`（否则那些用例在变红） |
| **D-100** | M-17 `m17_backend/mod.rs` + M-16 | v0.21 CSR 黄金覆盖批 · 检视 A · P5 | ⚠️ 原描述保留（**今天不是活 bug**：两个刷新点上都没有存活的借用 ⇒ 不可达；本批把接口改成**能表达**这个状态） | ✅ **已闭环（v0.22 · Wave 2 · `a13d058`）**：`SharedInterruptPort::pending_lines()` 的返回值改成 `Option<Vec<u16>>`（`None` = **状态未知**，`crates/kernel/src/m16_interrupt/mod.rs:178` / 实现 `:670`）⇒ `project_mip` 遇 `None` **跳过写**（`crates/kernel/src/m17_backend/mod.rs:382`）⇒ 保住「**读失败不得改写 guest 状态**」这条不变式。**判别力**：**争用场景是造得出来的** —— `borrow_mut` 跨过 handler 的 `irq_clear` 写时即可触发 ⇒ M-17 侧有一条**真的造出争用**的用例（断言 `mip` **不被写**），不是恒绿。⚠️ 签名变更影响仓外实现者（已记在本批 commit 的遗留问题里） |
| **D-101** | M-12 `m12_engine/mod.rs` + M-11 | v0.21 CSR 黄金覆盖批 · 检视 B · F-12 | ⚠️ 原描述保留（① **仅内部 API 可达** ⇒ 无活影响；② **宽松解析，非崩溃风险**） | ✅ **已闭环（v0.22 · Wave 2 · `a13d058`）**：两处各归其位。① **`restore_full` / `restore` 两条恢复路径都走已实现的 CSR 表过滤** ⇒ 恢复一个含**未实现 CSR** 的快照 ⇒ guest **读回 0**。② **`sh_entsize` 的校验落在 M-06（ELF 容器层）而不是 M-11** —— M-11 只拿到 `.symtab` 的**原始字节**、**根本看不到节区头** ⇒ 把校验放 M-11 是不可能的 ⇒ 撒谎的 symtab（**谎报 `sh_entsize`**）**整个镜像**被 `E_LDR-01` **拒收**（`crates/kernel/src/m06_elf/elf.rs:547`）。⚠️ **`sh_entsize == 0` 视作「未声明」放行**（0 = 未声明，不是「谎报」）⇒ 只有**非 0 且 ≠ 16** 才拒收。**判别力**：6 条反向变异全红；其中两条**当场查出该 lane 自己写的恒绿用例** ⇒ 已修 |
| **D-102** | 契约层 `model.rs`（`DisasmEntryDto`） + M-11 + 前端 M-02 | v0.22 Wave 2 批 · 冻结规格 STAGE-7 裁决 D | **「反汇编面板逐条行号」需要给 `DisasmEntryDto` 加字段** ⇒ 属**契约层变更**，而本批**契约层零改动** | ✅ **已闭环**（v0.24 批 · Lane J + Lane H + Lane K）：契约 v4 定的 `file` / `line` 两个字段**内核侧真的会填了** —— M-11 的反汇编器接上 `SymbolService::resolve_line`，`DisasmEntryDto` 带真 `file` + 真 `line`（查不到就是 `None`，**绝不编造**）；宿主把 `SymbolIndex` 接进反汇编器；前端面板**逐条**显示。**真机证据**：osz 的一条指令实测 `file = /tmp/osz-fresh/kernel/base/task/src/task.c`、`line = 41`。⚠️ 「查过没命中」与「压根没查」在 `DisasmEntryDto` 上仍**不可区分**（契约只有两个可空字段 —— 形状决定的，不是实现偷懒），分这两态的那一路是 `SourceLocation` 事件（宿主已有，未改）。 |
| **D-103** | M-13 取指路径 + M-14 | v0.22 Wave 2 批 · 冻结规格 STAGE-7 裁决 G | **取指侧的未映射访问仍是总线错误、不是 cause 1**：本批把**数据**访存的未映射访问陷阱化（load = 5 / store = 7），但**取指**（指令地址未映射）**没有**对应变体 ⇒ 取指未映射仍以 `E_BUS-01` 原样上抛，**与参考侧有分歧** | ⏳ **已登记未做**（P2 · 本批**刻意不做**）：**理由**：RISC-V 把取指访问故障的 `mcause` 定为 **1**（instruction access fault），而本批的 `TrapCause` 只有 5 / 7 两个**数据**访存变体 ⇒ 要做就得**新增 cause 1 变体**并单独定它的 `mtval` 语义（取指地址）⇒ 属**独立一批**。⚠️ **不搭车**的理由：新增一个 `mcause` 变体会改动 `TrapCause` 的**穷举匹配点**，**风险面**比本批大 ⇒ 单独做、单独对拍。**判别力**：落地前需「取指未映射 ⇒ 两侧都陷入、且 `mcause` = 1、`mtval` = 取指地址」的行为用例 |
| **D-104** | M-11 `m11_symbol/dwarf.rs`（v5 `define_file`） | v0.22 Wave 2 检视轮 · V-A · VA-06 | **v5 的 `DW_LNE_define_file` 把两个字符串节区传成空切片 ⇒ 整单元降级**：`define_file`（约 `:1075-1093`）在 v5 分支调 `read_v5_entry(cur, &header.file_format, header.dwarf64, &[], &[])` —— `line_str` / `debug_str` 是**空**的（`run_program` → `define_file` 根本没往下带）⇒ 文件表格式用 `DW_FORM_line_strp` / `DW_FORM_strp` 时，**整个单元降级**（不是只丢追加的那一项） | ⏳ **已登记未做**（P3 · 低危）：**降级是诚实的**（不猜、不崩、不给半个行号）⇒ 只是**能力静默丢失**，不会给错答案。**判别力**：探针实测（v5 单元 + `(DW_LNCT_path, DW_FORM_line_strp)` + 一条 `DW_LNE_define_file`）⇒ `status = Malformed`、`units_degraded = 1`、该地址 `NoDwarf`。**待做**：把 `line_str` / `debug_str` 顺着 `parse_unit` → `run_program` → `define_file` 传下去 |
| **D-105** | M-11 `m11_symbol/dwarf.rs`（单元级 vs 节区级降级口径） | v0.22 Wave 2 检视轮 · V-A · VA-08 | **降级（没解析成功）单元里的地址被报成 `Missed`（= 查过没命中），而裁决 J 的口径下它更接近「没查」**：节区级规则是「畸形 ⇒ `NoDwarf`」，**单元级**规则却是「降级单元的地址 ⇒ `Missed`」⇒ 地址在**没解析成功**的单元里，与地址**解析成功但不在表里**，对调用方是**同一个回答** | ⏳ **已登记未做**（P3 · 低危）：⚠️ 这是**有意选择且有用例钉住**的（`a_degraded_unit_contributes_no_rows_but_its_neighbours_still_work`）⇒ **不是漏网**；但与裁决 J 的**字面**口径有张力 ⇒ 如实登记。**后果**：契约层 `Missed` ⇒ `line = u32::MAX` ⇒ 「这个单元解析失败了」这件事**对按地址查询的调用方不可见**（模块只在整体粒度暴露 `units_degraded()`）。**待做**：要么把降级单元的地址也报 `NoDwarf`，要么在契约层显式表达第三态（等契约 v4 一起做） |
| **D-106** | M-11 `m11_symbol/dwarf.rs`（`basic_block` / `prologue_end` / `epilogue_begin`） | v0.22 Wave 2 检视轮 · V-A · VA-09 | **三个状态机寄存器被维护但从不读取、也无任何断言**：它们只在 `LineState::new` 里初始化、只在三个标准操作码处被置 `true`；`raw_rows()`（测试专用）只暴露 `addr` / `file` / `line` / `is_stmt` / `column` ⇒ 这三个寄存器**无法被任何用例观察到** | ⏳ **已登记未做**（P3 · 低危）：模块文档的论证是「状态机还有 `is_stmt` / `column` 两个寄存器 ⇒ 它们必须被真的算出来，否则『状态机处理了它们』只是一句没人能验证的话」⇒ 这个论证只兑现了两个，另外三个是**同样的「算了但没人验证」**。不影响 `LineEntry`（契约只有 3 个字段），也不影响地址→行的答案 ⇒ 纯「维护了但没验证」。**待做**：要么删掉（省三个字段 + 三个「误以为已支持」的印象），要么在 `raw_rows()` 里一并暴露并加断言 |
| **D-107** | 前端 M-02（源码视图的「当前 PC」按钮） | v0.22 Wave 2 检视轮 · V-B · VB-8b | **`pc` 为 0（还没拿到 PC）与「真的在地址 0」不可区分** ⇒ 按钮**不禁用**、不提示 | ⏳ **已登记未做**（P3 · 低危）：与 **VB-1** 同源（VB-1 已修：源码视图现在真能拿到 `sim.pc`），但「PC 尚未知」与「PC 就是 0」这两种情形在界面上仍**长得一样**。**待做**：PC 未知时禁用按钮并给出提示文案。**判别力**：落地前需一条「PC 未知 ⇒ 按钮禁用」的用例 |
| **D-108** | M-12 `m12_engine/mod.rs`（`RunReason::TimeReached`） | v0.22 Wave 2 检视轮 · V-D · VD-F5 | **`run_until` 在目标时刻没到时也报 `RunReason::TimeReached`** ⇒ 停止原因**在说谎** | ⏳ **已登记未做**（P3 · 低危）：V-D 探针实测 `until_ps = 500`、`now_ps = 0`、`reason = TimeReached`。⚠️ **已被现有用例钉住**（说明有别的用例依赖当前行为）⇒ 改动前要先看那条用例。**待做**：把「目标没到」与「目标已到」分成两个 `RunReason`。**判别力**：落地前需一条「目标未到 ⇒ 停止原因不是 TimeReached」的用例 |
| **D-109** | 契约层 `model.rs`（`SimState::seed`） | v0.22 Wave 2 检视轮 · V-D · VD-F6 | **`SimState::seed` 是死字段**（全仓无生产代码读写，只在 `SimState::default()` 里被写成 0） | ⏳ **已登记未做**（P3 · 低危）：LLD §1.1 / §3.6 与 AD §8.1 D0 的确定性定义是「**固定种子 PRNG** + 稳定事件排序」；实现里**没有任何 PRNG** ⇒ 确定性**成立**，但成立的原因是「**根本没有随机源**」而不是「有种子」。属「**规格承诺了机制、代码没有机制**」⇒ 如实登记。**待做**：要么实现种子化 PRNG，要么在 LLD 里把这条措辞改掉（**改文档**路线成本低得多）。 |
| **D-110** | M-12 `m12_engine/mod.rs`（`quanta` 一词两义） | v0.22 Wave 2 检视轮 · V-D · VD-F7 | **同一个词「quanta」在 M-12 里有两种单位**：`run_until_idle` / `run_steps` 返回「跨过的**量子数**」（`quanta_between`），而 `RunOutcome::quanta` 是「**外层循环转了几圈**」 | ⏳ **已登记未做**（P3 · 低危）：两处注释都写清了，但**同名字段单位不同** ⇒ 跨 API 比较会出错。**待做**：改名或加显式类型（例：`quanta_crossed` vs `outer_loops`）。 |
| **D-111** | M-12 `m12_engine/mod.rs`（快照侧的 CSR 写入收口） | v0.22 Wave 2 检视轮 · V-D · VD-F8 | **D-101① 的 CSR 过滤只堵**还原侧**、不堵**写侧** ⇒ 一旦将来有路径经公开的 `state_mut()` 塞进未实现 CSR，这个不对称会**静默**生效（无错误、无日志）且被快照带出去 | ⏳ **已登记未做**（P3 · 低危）：⚠️ **当前不是缺陷** —— V-D 明确回答「完整快照往返**恒等**，成立」：生产里唯一写 CSR 文件的路径（`csr_write_raw`）全部被守卫 ⇒ 引擎**产不出**含未实现 CSR 的快照。**待做**：把过滤**下沉到写入收口点**（而不是只在还原侧过滤）。**判别力**：落地前需一条「经 `state_mut()` 塞进未实现 CSR ⇒ 快照里**不出现**它」的用例 |
| **D-112** | 契约层 `snapshot.rs`（`EngineSnapshotDto::is_empty`） + M-12 | v0.22 Wave 2 检视轮 · V-D · VD-F9 | **外部快照里带一条 `misa` ⇒ 过滤**保留**它**（`misa` 在已实现表里，但平台从不把它存进文件）⇒ `EngineSnapshotDto::is_empty()` **变假**（D-40 的「空快照」判据被**外部输入**打穿） | ⏳ **已登记未做**（P3 · 低危）：**对 guest 不可见**（`csr_read` 对 `misa` 特判）。D-101① 的提交说明已如实登记为「残留 #1」。**待做**：过滤时把 `misa` 一并按「**算出来的**」处理（不当作可恢复的存储项）。**判别力**：落地前需一条「带 `misa` 条目的快照 ⇒ `is_empty()` 的判断不受影响」的用例 |
| **D-113** | M-12 `m12_engine/mod.rs`（`snapshot_full` 的迟到事件归一化） | v0.22 修复 lane W2 自查 | **归一化有次序代价**：VD-F1 的修法把「已到期未派发」的事件按 `max(at_ps, now)` 拉到当前时刻 ⇒ 若队列里**同时有两件以上**迟到事件、且它们的 `seq` 次序与到期时刻次序**相反**，归一化会把它们拉到**同一时刻** ⇒ 规范序 `(at_ps, seq)` 改按 `seq` 排 ⇒ **出队次序可能与活着的引擎不同** | ⏳ **已登记未做**（P3 · 低危）：属「同一时刻多件事件」的**边角情形** ⇒ 换成的是「引擎产出的快照**根本落不了盘**」这个更大的问题消失。**待做**：归一化时保留**相对次序**（例如按原 `at_ps` 的稳定序做二级键），使归一化**不改变**同刻事件的相对出队次序。**判别力**：落地前需一条「两件迟到事件且 `seq` 与时刻次序相反 ⇒ 还原后出队次序与活着的引擎一致」的用例 |
| **D-114** | M-17 `m17_backend/mod.rs`（`step_n` 的 `InterruptTaken` 分支） | v0.22 修复 lane W2 自查 | **VD-F3 的守卫只覆盖了 M-12 的唯一主循环**：M-17 的**指令驱动**路径 `step_n` 里有**同形状**的分支（中断交付**不退休指令、不推进虚拟时间**）⇒ 同样可能自旋，而那条路径**没有无进展守卫** | ⏳ **已登记未做**（P2 · 中低危）：M-17 的 `step_n` 走的是**单步**语义（宿主每步调一次）⇒ **自旋的触发条件**比 M-12 苛刻（需要一个**反复交付**且**从不退休**的中断源）⇒ 今天没有活影响。⚠️ **但这是同一个不变式破掉**（M-16 的 `active_irq` 单级防护）的**第二个落点** ⇒ VD-F3 的修复**只堵了其中一个**。**待做**：把 M-12 的无进展守卫（或其等价物）也接进 M-17 的 `step_n`。**判别力**：落地前需一条「`step_n` 遇到反复交付的中断 ⇒ 报错退出而不是自旋」的用例 |
| **D-115** | M-12 `m12_engine/mod.rs`（watchdog 只做了一半） | v0.22 修复 lane W2 自查 | **LLD §3.5 的 watchdog 只落地了「无进展检测」这一半**：宿主**时间辅助预算**（默认 200 ms）、**滑动窗口三次确认**、触发后的 **HMS 同步终止**（报 `HMS_FATAL` + 幂等标志）与 **`EV_WATCHDOG`** 日志记录**都还没做** ⇒ 守卫触发时**只返回 `Err` 交由上层处置** | ⏳ **已登记未做**（P2 · 中低危）：VD-F3 的**核心诉求**（「活锁必须变成失败而不是挂死」）**已达成** ⇒ 剩下的是**处置链路**与**第二道防线**。**待做**：宿主侧加时间预算 + 三次确认 ⇒ 再报 HMS 并记 `EV_WATCHDOG`。**判别力**：落地前需一条「守卫触发 ⇒ 上层收到 `HMS_FATAL` 且 HMS 状态幂等」的用例 |
| **D-116** | 契约层（`ErrorFamily::Engine` 的场景码表） | v0.22 修复 lane W2 自查 | **`E_ENG_03_NO_PROGRESS` 尚未进契约层的场景码表**：M-12 里三个引擎族场景码（`E_ENG_01_BUS_PUBLISH_FAILED` / `E_ENG_02_QUEUE_INCONSISTENT` / `E_ENG_03_NO_PROGRESS`）目前**只住在内核 crate 里**，契约层**没有**它们的登记 ⇒ 前端拿到的错误**无法按码分派** | ✅ **已闭环**（v0.23 Wave 2 · Lane D · `0c6a35d`）：契约层升 v4 时把三条**一并登记**进场景码表，并加了一条契约层用例「场景码表里能查到 `E_ENG_03`」。⚠️ **残留**：内核 crate 里 M-12 仍**自写一份** `E_ENG_*` 字面量 ⇒ 「一个约束一个落点」未收拢 ⇒ 另立 **D-120** |

| **D-117** | 契约层 `session.rs`（`PeripheralSessionState`）+ M-15 `clint.rs` / `plic.rs` | v0.23 批 · Lane C 收口自审 | **CLINT 与 PLIC 进不了会话快照**：契约层的 `PeripheralSessionState` 只有 `Uart` / `Gpio` / `Timer` 三个变体 ⇒ 这两个新器件的 `session_state()` 走契约层的**默认实现**（返回 `None`）⇒ 它们**既不进快照、还原后也回初值**（`mtimecmp`、PLIC 的优先级与使能位图全丢） | ✅ **已闭环**（v0.24 批 · Lane I）：契约层 `PeripheralSessionState` 加了 `Clint` / `Plic` 两个变体与对应 DTO（`SESSION_SNAPSHOT_SCHEMA_VERSION` 1 → **2**，破坏性变更**显式**升版，旧载荷现在被 `E_SNAP-05` **显式拒绝**）；M-15 侧 `clint.rs` / `plic.rs` 补上 `session_state()` / `restore_session_state()` / `validate_session_state()` 三个实现。**判别力**：`mtimecmp`（周期）、PLIC 的优先级 / 使能位图 / 门限 / 挂起位**往返一次后不变**；四种畸形状态（id 对不上 ⇒ `E_PER-05`；hart 数 / 源数 / context 数不符 ⇒ `E_PER-04`）**被拒**，且「CLINT 的状态喂给 PLIC」两个方向都拒。 |
| **D-118** | 契约层 `session.rs`（`UartStateDto`）+ M-15 `uart.rs` | v0.23 批 · Lane B 收口自审 | **`UartStateDto` 只有五个字段**（`tx` / `rx` / `status` / `irq_line` / `now_ps`）⇒ ns16550 的 **LCR / IER / FCR / MCR / SCR / DLL / DLM 七个寄存器进不了快照** ⇒ 还原后 `LCR.DLAB` 回 0、波特率除数回初值 ⇒ **guest 若在 DLAB=1 的状态下被快照，还原后读到的除数是错的** | ✅ **已闭环**（v0.24 批 · Lane I）：`UartStateDto` 加了 **LCR / IER / FCR / MCR / SCR / DLL / DLM** 七个 ns16550 寄存器字段，既有字段一个未删（`status` 仍然是 LSR 的值）。**判别力**：`LCR = 0x83`（**DLAB 置位**）往返后 `dlab_enabled()` 仍为真、除数 `0x010C` 不变 —— ⚠️ **DLAB 位本身也要还原**，否则还原后「除数锁存器模式」的状态就丢了。⚠️ `LSR` 仍按既有口径**重算**（而不是还原原值）：还原原始数字会让寄存器跟自己的 FIFO 打架。 |
| **D-119** | 契约层（命令表）+ M-15 `uart.rs` + 前端 | v0.23 批 · osz 端到端 | **guest 的 UART 只有 TX 方向、没有输入通路**：平台的 UART 是**捕获环**模型（guest 写 ⇒ 宿主取），**反向**（宿主写 ⇒ guest 读）宿主侧留了 `KernelHost::uart_receive` 接口，但**契约层没有「往 UART 写」的命令**、前端也没有输入框 ⇒ **真实 RTOS 的 shell 只能看、不能打字**（STAGE-8 的验收 **E-8 判为未做**） | ✅ **已闭环**（v0.24 批 · Lane H + Lane I + Lane K）：契约层新增 `WriteConsole { bytes: Vec<u8> }` 命令（**不复用** `WriteMemory` —— 后者绕过外设语义层，会让 guest 读 `RHR` 拿到**旧值**而 `LSR` 说「没有数据」⇒ **自相矛盾**）；宿主把字节喂进 UART 的 **RX FIFO**，`LSR` 的 `RX_VALID` 随之置位，同步器在 `IER` 允许时抬 **PLIC 源 10** ⇒ guest 看到 `mcause = 0x8000_000B`（架构码 11）；前端终端视图加**输入框 + 发送按钮**（草稿存 M-03 视图状态 ⇒ **重渲染不丢**；监听器绑容器 ⇒ **重渲染不丢**）。**真机证据**：送一个字节后读 UART 的 `LSR`（`0x1000_0005`）= **`0x63`**、bit0 置位。⚠️ **回显来自 guest**（平台不做本地回显）⇒ 端到端证据。 |
| **D-120** | M-12 `m12_engine/mod.rs` + 契约层 `error.rs` | v0.23 批 · Lane D 收口自审 | **引擎族场景码仍是两份**：D-116 闭环后，契约层**登记**了 `E_ENG_01/02/03`，但 M-12 里**仍自写一份字面量** ⇒ 「一个约束一个落点」这条项目规矩在这三处没守住 ⇒ 将来改一处忘了另一处，就会出现「契约层说的码」与「内核报的码」不一致 | ✅ **已闭环**（v0.24 批 · Lane I）：M-12 里自写的那份 `E_ENG_01/02/03` 字面量**已删除**，改为引用契约层 `error.rs` 里的具名常量 ⇒ 全仓只剩**一个**定义处。**判别力**：一条**等值断言**（三个码与契约层逐字相同）+ 一条**行为对拍**（引擎实际报的码与契约表一致）—— 只查定义处会漏掉「引用错了但值恰好一样」。 |
| **D-121** | M-11 `m11_symbol/disasm.rs` + 契约层 `model.rs` | v0.23 批 · Lane D 收口自审 | **D-102 只闭环了一半**：契约层给 `DisasmEntryDto` 加了 `file` / `line` 两个 `Option` 字段（契约 v4），但**内核侧一律填 `None`** ⇒ 反汇编面板拿到的永远是「无行号」⇒ 「反汇编面板逐条显示源码行」这件事**还没发生** | ✅ **已闭环**（v0.24 批 · Lane J + Lane H）：M-11 的反汇编器在渲染每条指令时调 `SymbolService::resolve_line`，把真 `file` + 真 `line` 填进 `DisasmEntryDto`。⚠️ **绝不编造**：查不到就填 `None`（**不是** `Some("")`、**不是** `Some(0)`、**不拿别的地址的行号顶替**）；解析器**按条目自己的地址**被问（查一次就复用会让第二条拿到第一条的行号）。⚠️ **缓存语义**：条目连同它的行号一起进按地址缓存 ⇒ 每条指令只查一次行表（计数器 `line_resolutions()` 可观测），缓存失效后重新取指会再查一次。 |
| **D-122** | M-15 `m15_peripheral/uart.rs` | v0.24 批 · Lane H 接线时发现（**规格符合性**缺陷） | **UART 的接收 FIFO 只有 1 字节深** ⇒ 一行 shell 命令根本送不进去**：v0.23 的 UART 模型把接收侧做成 `rx: Option<u8>`（**单字节**），而冻结规格 STAGE-9 裁决 B 第一步写的是「把**字节**塞进 UART 的 **RX FIFO**」—— 复数 ⇒ 规格**假定** FIFO 能装多个字节。⇒ 宿主一次 `WriteConsole` 写三个字节时**只有第一个进 FIFO**，后两个被丢并锁存 `LSR` 的溢出位（bit2）⇒ 「能打字」只做到「能打一个字符」，**与批次目标不符** | ✅ **已闭环**（v0.24 批内 · Lane I）：接收侧改成 `RX_FIFO_CAPACITY = 16` 的小队列（`VecDeque<u8>`）：`LSR` 的 `RX_VALID` = 队列非空、读 `RHR` **弹出队头**、`receive()` **入队尾**、队列满时**丢最新**并锁存溢出位（与 TX 捕获环「丢最新保最早」的口径一致 ⇒ 丢失**看得见**不是静默的）；`TX` 侧一个字未改，`IER` 的 RX 使能判定与 `DLAB` 语义一字未改。快照侧加 `rx_queue` 字段装全部待读字节，`rx` 保留为队头字节，并新增不变式「`rx` 必须等于 `rx_queue` 的队头」。⚠️ **十六字节是首版取值**：若真机出现更长的输入行要调大该常量（DTO 形状不变，只是载荷变长） |
| **D-123** | M-14 `m14_bus/mod.rs`（RAM 后端 + 快照页导出）+ 内核宿主 `kernel_host/mod.rs`（内存图装配） | v0.24 批收口时的架构复盘（用户提问「内存图是指啥，为什么不能是通用的、要分两个」） | **内存图是宿主硬编码的两档，不是通用的**：`MemoryProfile::{Default, QemuVirt}` 两个变体把地址空间**写死在宿主里**（`DEFAULT_RAM_LOW/HIGH_*` + `QEMU_VIRT_*`，`kernel_host/mod.rs:305~355`），换一块板子就得改宿主代码 ⇒ 「通用平台」这件事**今天不成立**。⚠️ **但根因不是「内存图形状的必然」** —— 是 **RAM 急分配**：`map_ram` 走 `prepare_bytes` 的 `vec![0u8; size]`（`m14_bus/mod.rs:752`）把整段 RAM **一次申请** ⇒ QemuVirt 档的 128 MiB RAM = **128 MiB 物理内存** ⇒ 又撞上每区域 16 MiB 上限（`DEFAULT_MAX_REGION_BYTES`，`m14_bus/mod.rs:56`，本是为了防 OOM 的护栏）⇒ 128 MiB **被迫拆成 8 个 16 MiB 区域**（`kernel_host/mod.rs:795~799`）⇒ 才需要一档「128 MiB」的内存图 ⇒ 才出现「两档」。**连带代价（实测）**：会话快照的 RAM 载荷随 RAM 尺寸**线性膨胀** —— `export_pages`（`m14_bus/mod.rs:1182`）把每段 RAM 按 64 KiB（`MEM_PAGE_CHUNK_BYTES`）**整块导出、不跳过全零页** ⇒ **实测**（本轮用临时探针跑 `KernelHost::new_with_profile` + `capture_session` 后删除探针）：**Default 档** = 3 个 region / 80 页 / RAM 载荷 **5.00 MiB** / 整份 JSON **10.00 MiB**；**QemuVirt 档** = 13 个 region / 2 064 页 / RAM 载荷 **129.00 MiB** / 整份 JSON **258.06 MiB**（hex 编码后约 2 倍）⇒ **这些字节几乎全是零**——guest 只用了几个 KiB，快照却背着 258 MiB。**做法（C 做法）= 三件事一起做**：**A** RAM 改**页级懒分配**（4 KiB 页，写过才占物理内存；读没写过的页回 0）· **B** **内存图由工程/平台描述**（一份可声明的内存图 ⇒ 新平台**不改宿主代码**）· **C** RAM 区域**按需增删**（region 登记与物理页是**两层**） | ⏳ **已登记未做**（**结构性能力缺口，不是缺陷** —— 今天两档对 osz 都成立，判决是 VERDICT MATCH）。⚠️ **A 的陷阱（必须守住这条分界）**：**region 登记与页懒分配必须是两层** —— 「地址**没登记**」⇒ 未映射 ⇒ **架构陷阱**（cause 5 / 7，**D-99 的判别力就靠这条分界**）；「登记了但这页**没写过**」⇒ 读回 **0** ⇒ 若把两层合成一层，guest 读一块「从没写过的 RAM」会拿到架构陷阱而不是 0 ⇒ **D-99 的判别力当场破掉**。**建议顺序：先 A 后 B**（A 做完两档可合一；B 做完才谈得上「新平台不改宿主代码」）· C 随 A 一并落地。**工作量估计**：A ≈ **3~5 人日**（页表 + 写时分配 + 快照侧页内容格式要跟着改）· B ≈ **3~4 人日**（内存图描述格式 + 宿主加载 + 前端内存映射表跟着变）· C ≈ **1~2 人日**。**判别力清单（落地前先钉）**：① 读**没写过**的 RAM 区域 ⇒ 回 **0**（**不是**陷阱）· ② 读**没登记**的地址 ⇒ 架构陷阱 **cause 5 / 7**（**不是** 0）· ③ 写一页 ⇒ 只占**一页**的物理内存（用「分配后 RSS 增量」或**计数器**观察，**不能**靠读值反推）· ④ 快照往返 ⇒ 页内容**逐字节**相同，且**没写过的页仍读回 0** · ⑤ 换一份内存图描述 ⇒ **宿主代码零改动**（**这条是 B 的唯一判别式**）。⚠️ **破坏性变更预警**：A 落地后 `m13_state.mem_pages` 的语义从「每段 RAM 的**全部** 64 KiB 块」变成「**只装写过**的页」⇒ 会话快照载荷格式**破坏性变更** ⇒ 与 v0.24 的 `SESSION_SNAPSHOT_SCHEMA_VERSION` 1→2 同性质，须**显式升版 + 旧载荷显式拒收**（不能默默换形状）。⚠️ **护栏不能删**：`DEFAULT_MAX_REGION_BYTES`（16 MiB）是**防 OOM 护栏**，A 落地后它可以放宽但**不能去掉**（否则一个畸形镜像就能申请 4 GiB 物理内存） |

### 1.3.1 检视轮（2026-10-20）小结

4 路独立检视共查出 **22 项**。处置结果：

| 严重度 | 项数 | 处置 |
| --- | --- | --- |
| 🔴 阻断 | 1 | M-13 SYSTEM 未校验 `funct3`（CSR 形态编码被误判为 ECALL，mcause 错）→ **已修**（`caf7f82`），补 3 条用例；**变异自检**：改回原样后 4 条用例变红 |
| 🟠 高危 | 2 | F-3 总线 hook `borrow_mut()` 重入 panic → **已修**（`bd647dd`），补 3 条用例；F-1 快照语义承诺 → **已订正文档**（`7254a00`），根因转 D-24 → **D-24 根因已在收口轮修复**（`3016de0`：契约新增可序列化快照 DTO + `snapshot_full` / `restore_full` 端口 + M-12/M-17 接线） |
| 🟡 中 | 6 | 已修：M-06 IHEX 嗅探结构判据（`fd3d036`）、M-06 bss gap `checked_add`（`fd3d036`）、M-14 外设路径上限覆盖（`0fbd6e7`）；未做：D-25 / D-29 / D-30 / D-32 / D-33 |
| 🟢 低 | 13 | 已修：M-13 `sequential` 步进覆盖 / 地址回绕覆盖 / 子字存储截断契约（`caf7f82`）；M-06 边界测试（`fd3d036`）；M-18 契约字段名校验 / mock `ClearBreakpoint` 按 id / M-10 重连重订阅 / `once()` 退订覆盖 / mock topics 重置；LLD 信号名对齐（F-5）；未做：D-27 / D-28 / D-31 / D-34 / D-35 |

**方法论收获**：检视全部用**变异测试**判定「测试是否真能抓住实现错误」——V1 的 32 项变异里有 4 项**没有任何现有测试会红**，其中 1 项是真缺陷（CSR funct3）。本轮补的用例均做了**反向验证**（改回原样必须变红），确保不是「只增不减的安慰剂」。

> **统计口径提醒**：上表「处置」列按**子项**列举（一条发现可能含多处子项，例如「M-13 3 处覆盖漏洞」是 1 条发现、3 处子项），故其条目数与「项数」格不相等；**分档项数 1+2+6+13 = 22 与总数一致**。
>
> **变异计数的口径**：一律用 `cargo test --workspace --no-fail-fast`。`cargo test` 默认 **fail-fast**——前一个 test target 失败会**中止后续 target**，红测条数会被系统性截断（收口轮自检就踩过这个坑：同一变异 fail-fast 下 5 条、`--no-fail-fast` 下 9 条）。

---
### 1.4 工程门禁缺口（根因级）

| ID | 归属模块 | 来源 | 问题 | 处置 / 说明 |
| --- | --- | --- | --- | --- |
| **D-57** | M-17 ↔ 契约 | D-07 检视轮 V-A · 🟠-2 | **`continue_until_event` 的实现语义与冻结契约不一致**：契约 `crates/contract/src/port.rs` 写的是 「Run until the next breakpoint, watchpoint, trap, or **quantum boundary**」——量子边界 = 1 µs = 1000 条；实现传 `u64::MAX`，跑到**指令预算**耗尽 = 10000 条 = 10 个量子。当时契约层冻结（改文档要等 `schema_version=2`）——**本批已解冻**，处置见右列 | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：**改契约文档，不改实现**。**为什么不是改实现**：改实现会**推翻 D-07 刚闭环的 `crates/kernel/tests/d07_time_driven.rs` 31 条用例所钉的语义**；而且在 D-07 的裁定里，**量子边界是「抢占点」不是「停止条件」**（事件边界 = 抢占点，指令边界 = 安全检查点，两者交汇于指令边界）——把实现改回「停在量子边界」等于**把 D-07 刚统一的语义再拆开**。**新措辞**：「Run until the next breakpoint, watchpoint, or trap, **or until the engine's instruction budget for this run window is exhausted**. The quantum boundary is a **preemption point** (the scheduler may yield there), **not** a stop condition.」**证据**：`crates/contract/src/port.rs` 的 `ExecutionBackend::continue_until_event` doc comment（v2 措辞 + 保留了「为什么是改文档而不是改实现」的注记）。**判别力**：**行为**由 D-07 的 31 条钉住（`d07_time_driven.rs`）；本条是**文档与实现的偏离**、不是行为缺陷，因此判别式是**文档核对**（规格 §11 验收清单第 10 项）而非新增用例。⚠️ 原台账写的「改契约文档时必须同步补一条 `continue_until_event` 的行为用例」已由 **D-07 的既有用例**满足（该行为的判别式在 `d07_time_driven.rs`，不在契约层）——**不需要**再补一条重复的用例|
| **D-58** | M-15 ↔ 契约 | D-07 检视轮 V-A · ⚪-8 | **`Peripheral::tick` 的调用频率与契约文档的口径不一致**：契约写「Called once per quantum (1µs by default)」；实现是**每条指令一次**（D-46 为了让探针边沿带上精确到单条指令的时间戳引入的）。M-17 的注释把它解释成「**下限**，不是上限」——这是对**冻结**契约的重新解释，此前无台账记录 | ✅ **已闭环（2026-10-21 · 契约层 v2 · `3126f54`）**：**改契约文档，不改实现**。**为什么不是降回每量子一次**：降回**每量子一次会牺牲 D-46 换来的边沿时间戳精度**——那正是 D-46 接线买的东西（边沿时间戳精确到单条指令）；把它量子化回去等于**把这个精度扔掉**。**新措辞**：「Called on **every instruction boundary** (and on every quantum boundary). The default quantum is 1 µs, but the tick is **not** quantised to it: the probe pipeline needs edge timestamps that are exact to a single instruction, so the tick runs at instruction granularity. Implementations must therefore be **cheap** and **idempotent within one virtual instant**.」⚠️ 末尾那句「同一虚拟时刻幂等」与 **D-07 修掉的「同一外设在同一虚拟时刻被 tick 两次」**（V-A 🟠-1）**相容**。**证据**：`crates/contract/src/port.rs` 的 `Peripheral::tick` doc comment（v2 措辞 + 注记）。**判别力**：**行为**由 D-07 的 31 条钉住（「两条路径的外设推进序列逐条相同」等）；本条是**文档偏离**，判别式是**文档核对**（规格 §11 验收清单第 10 项）。⚠️ **两侧现在一致**：M-17 侧的注释本来就是「下限」口径（D-07 当时写的），本批把**契约侧**补齐|
| **D-59** | M-13 ↔ M-17 | D-07 检视轮 V-A · ⚪-13 | **`StepOutcome::HostPause` / `StepOutcome::Breakpoint` 是死分支**（M-13 从不返回这两个）；`pause()` 只置 `last_stop = Some(UserPause)`，**不真正中断**正在跑的批次 | ✅ **已闭环**（v0.19 · M-16 落地批 · 裁决 C）：① **语义定了** = 「在**下一条指令边界**停下」（不是「立刻停」——宿主调用是**同步**的，「立刻」是**做不到**的语义）；落点是 **M-17 自己的 `pause_requested` 标志位**（`take_pause()` **取走即消失**），`step_n` / `continue_until_event` 在**指令边界**查它。② **三个死变体全删**：`HostPause`（**M-13 不可能知道**宿主要暂停）· `Breakpoint`（M-08 的断点由 **M-17 在取指前**自己判）· `Watchpoint`（M-10 的观察点由**总线 hook** 记录、M-17 在指令返回后判）⇒ 删掉后 M-17 的 `match` **穷举** `StepOutcome` 全变体，M-13 以后再加变体会**编译不过**（而不是静默走兜底）。**证据**：`pause_stops_the_batch_at_the_next_instruction_boundary` · `a_pause_affects_exactly_one_batch` · `pause_stops_the_time_driven_path_too` · `a_reset_clears_a_pending_pause`（4 条 M-17 用例）。⚠️ **规格同步订正**：STAGE-4 §3.7 初稿写「批次开始时清零」——按字面实现会把暂停请求**直接丢掉**（`pause()` 必然发生在批次**之外**）⇒ 订正为「**被消费时**清零」，与 M-16 交付的「取走即消失」同型。⚠️ **反向变异自检 K-20**（「取走不消失」）⇒ `a_pause_affects_exactly_one_batch` 红 |
| **D-60** | M-12 / M-17 | D-07 检视轮 V-B · ⚪-12 | **两处死接口**：① `Engine::tick_peripherals` 的返回值（推进了几个外设）**无人消费**——变异「改掉返回值」0 条用例变红；② `continue_until_event` **不消费观察点命中槽**（`watch_hit`）——变异「不消费」0 条用例变红，命中信息会**留到下一批次**才被读走 | ⏳ **已登记未做**：① 要么让调用方真的用上返回值（记进日志/指标），要么把它改成 `()`；② `continue_until_event` 应当像 `step_n` 一样在批次结束时消费命中槽 |
| **D-61** | M-14 | D-07 检视轮 V-B · ⚪-13 | **`Bus::tick_peripherals` 在生产路径上已不可达**——D-07 把外设时钟的推进点收敛到 M-12 的 `tick_peripherals` 之后，全仓只剩 M-14 自己的单测在调它。旧实现仍留在代码里且仍被测，**容易被人接回去**（那就又是「两个真相」） | ✅ **已闭环**（v0.15）：该方法已标 `#[deprecated]`，doc comment 写明「D-07 之后外设时钟的唯一推进点是 M-12 的 `tick_peripherals`」 |
| **D-62** | M-12 / M-17 | D-07 检视轮 V-B · 🟡-5 / 🟡-15 | **D-07 的用例与「默认口径」耦合**：LLD 说量子（1 µs）与每量子指令配额（1000 条）**可按工程配置**，代码里目前是编译期常量。把配额改成 20000（一次**合法**的配置改动）会让 D-07 用例变红——其中「本来就与配置无关」的不变量（两条路径的推进序列逐条相同、`until_ps` 不被冲过、迟到事件记当前时钟、量子数 ⌈预算/配额⌉）本不该跟着变红 | ◐ **已部分闭环**（v0.15）：**4 条与配置无关**的用例已改成**相对单条指令成本 / 相对配额**的写法（不再钉死具体数字），变红数从 **8 条降到 4 条**；其中 3 条**确实**需要「一轮预算能跨过一个量子」这个前提，已加**显式前提断言**（前提不成立时红得清楚，给一句人话而不是让后面某个断言莫名其妙地失败），剩 1 条是**默认值测试**（本来就该随默认值变红）；测试文件头已显式标注「本文件钉的是默认口径」。⏳ **剩余**：将来支持 per-project 配置时，本文件的用例须拆成「默认值」与「可配置性」两组，且「可配置性」组必须至少有一条用例证明**配置变了行为也变** |

| **D-23** | 三条门禁对上述 5 项阻断**全部无感知** | `cargo test` 只覆盖 Rust；D-01/D-02 是 ESM 链接期错误，只能在浏览器或 Node 暴露；D-03 是 `panic!` 而非 `Err`，测试没喂这个输入；D-04/D-05 现有用例恰好绕开了触发条件。**这是「47→70→74→203 全绿却仍白屏 + 可 panic」的根因**。✅ **2026-10-20 实质闭环**：4 类最小门禁**全部补齐**（第 1 项 v0.10，第 2~4 项 v0.11），且 3 条阻断级条目（D-03 / D-04 / D-05）本轮全部消除 |

**最小门禁清单（2026-10-20 全部就位）**：

| # | 门禁 | 覆盖 | 状态 |
| --- | --- | --- | --- |
| 1 | `tools/check-front.sh`（node:test 冒烟门禁 + CI job） | D-01 / D-02 及未来任何 ESM 链接回归；另覆盖前端全部 16 项评审意见 | ✅ **v0.10 完成**（`ed66337`）—— 复审 16 组变异测试，3 处真盲区已封、位置耦合已消除、DOM stub 5 处保真度缺口补齐；**v0.11 扩至 109 用例 / 19 suite**（新增 mock 内核链路、命令路由、会话生命周期、寄存器内存服务） |
| 2 | M-06 畸形输入表驱动用例 | D-03 / D-15 | ✅ **v0.11 完成**（`955055f`）—— 新建 `crates/kernel/src/m06_elf/tests_malformed.rs`：**20 条表驱动畸形用例 + 2 条固定种子 fuzz**，覆盖空文件 / 4 字节短记录 / 坏 magic / `filesz > memsz` / 超长段 / count 不符 / 截断 header / 非 UTF-8 / 整数溢出。**每个畸形输入必须 `Err`，不得 panic**；并做了**反向验证**（把 D-03 修复改回去，用例双双 panic 失败） |
| 3 | M-13 `addi x0, x0, 42` → `gpr[0] == 0` 断言 | D-04 | ✅ **v0.11 完成**（`71a3b05`）—— 另补「先写 x1 再 `addi x0, x1, 1`，x0 仍为 0」 |
| 4 | M-12「snapshot → restore → 继续推进」确定性断言 | D-05 | ✅ **v0.11 完成**（`50d7de8`）—— 断言 Tick 轨迹 / IRQ 轨迹 / 步数 / 量子数 / 虚拟时间 / SimState / 剩余队列逐项相等，另加「还原后 == 从未还原」的更强形式 |

---

---

## §2 建议处理顺序

### 2.1 批次拓扑（2026-10-20 v0.11.0 后重排）

```
第 1 批 · ✅ 已完成（v0.10）
  D-01 ─┐
  D-02 ─┴─→ 前端恢复可启动
  D-23 #1 → ESM 链接门禁

第 2 批 · ✅ 已完成（v0.11 并行开发，5 路 worktree 隔离）
  D-03 + D-08 + D-09 + D-15 + D-21 + D-22  → M-06 加固 + 首次模块级 review
  D-04 + D-11 + D-16                     → M-13 RV32I 全集
  D-05 + D-17 + D-06                     → M-12 引擎加固
  D-23 #2 #3 #4                          → 三类最小门禁
  （前端 M-04/05/07/10 从空壳到真跑通）

第 3 批 · ✅ 已完成（v0.11 接线 PR）
  D-12 + D-14 → M-14 ↔ M-15 ↔ M-18 三处断点
  D-10        → M-06 ParseLimits（✅ 已做）+ M-14 prepare_bytes（✅ 已做）

第 3.5 批 · ✅ 已完成（2026-10-20 独立检视轮，4 路并行检视 → 逐项修复）
  V1 M-13      → 1 阻断（SYSTEM funct3）+ 3 覆盖漏洞  →  caf7f82
  V2 M-12/接线 → 1 高危（总线 hook panic）+ 1 高危（快照语义）→ bd647dd / 0fbd6e7 / 7254a00
  V3 M-06      → 2 中（IHEX 嗅探 / bss gap）+ 文档订正      →  fd3d036 / 9992419
  V4 前端+收口 → mock 契约缺陷 + 契约测试盲区 + 文档数字失真 →  （见 §4.1）
  LLD 信号名对齐（F-5，3 份 HTML）                            →  docs/lld-signal-name-align

第 4 批 · Wave 2
  D-07        → ✅ 已闭环（v0.15：四处断口全闭环 + 唯一主循环 run_until）
  D-13        → 字段可见性收紧（需同步改集成测试与前端 renderMapTable）
  D-24        → ✅ 已闭环（契约新增可序列化 DTO）
  D-30        → M-12 独立模块级 review
  M-13 M/C 扩展 → RV32IMC 全集补完
  Spike golden 对拍 → 工具链已就绪（Spike 1.1.1-dev + xpack GCC 15.2.0）
  契约缺口 7 项（D-32）→ 随 schema_version=2 一并处理

第 5 批 · ✅ 已完成（v0.17 ~ v0.22，逐批一个模块）
  5  契约 v2（schema_version 1→2）      → ✅ v0.17（D-32 闭环）
  5b 会话级快照载荷（六段 DTO）          → ✅ v0.18（D-36 / D-37 / D-41 闭环）
  5c M-16 中断控制器落地                  → ✅ v0.19（D-26 / D-59 / D-63 / D-64 / D-70 / D-72 / D-74 闭环）
  5d M-11 符号 / DWARF / 反汇编 / trace 导出 → ✅ v0.20（D-50 / D-51 闭环；新登记 D-86 ~ D-90）
  5e CSR / 陷阱黄金覆盖                   → ✅ v0.21（D-84 / D-85 / D-90 闭环；新登记 D-91 ~ D-101）
  5f Wave 2a · DWARF 行表真解析 + 前端源码高亮 + 访存异常陷阱化
                                      → ✅ v0.22（D-93 / D-98 / D-99 / D-100 / D-101 闭环；新登记 D-102 ~ D-112）

第 6 批 · ✅ 已完成（v0.23 · osz 落地批）
  6a 契约层 v3 → v4（D-102 契约形状 + D-47 / D-48 传输层欠账）→ ✅ v0.23
  6b M-15 重做成 ns16550 + CLINT / PLIC + 宿主两档内存图与三条中断通路 + console 通路
                                      → ✅ v0.23（D-117 ~ D-121 新登记）

第 7 批 · ✅ 已完成（v0.24 · **把 v0.23 的遗留一次做掉**）
  7a 契约层 v4 → v5（`WriteConsole` 命令 + 会话快照 schema v2）→ ✅ v0.24
  7b M-11 反汇编填真 `file` / `line`（D-121 / D-102 的内核侧）→ ✅ v0.24
  7c M-15 生产侧：CLINT / PLIC 进快照 + UART 七寄存器 + **RX FIFO 加深到 16 字节**
                                      → ✅ v0.24（D-117 / D-118 / 新登记 D-122）
  7d 前端终端输入框 + 源码树入口 + 反汇编逐条行号 → ✅ v0.24（D-119 / D-102）
  7e 真机三条判别式（E-6 真 file+line / E-7 断在调度器符号 / E-8 UART RX 就绪）
                                      → ✅ v0.24

第 8 批 · ⏳ **下一批候选**（v0.25 · **待下一轮检视定序**，此处只列**已知**欠账）
  D-103 取指侧未映射访问 ⇒ cause 1（instruction access fault）
  D-94  S 扩展 / PMP / 计数器（工作量 = 实现半个特权级 ⇒ 独立一批）
  D-104 ~ D-112 检视残留九条（均低危）
  M-20 收尾 / M-21 日志 / M-22 健康管理 / M-23 持久化后端（仍是 stub）
  D-13  字段可见性收紧（需同步改集成测试与前端 renderMapTable）
  D-114 M-17 `step_n` 的同形状活锁 / D-115 watchdog 只做了「无进展检测」这一半
  D-123 **内存图通用化**（A RAM 懒分配 + B 内存图由工程描述 + C 区域按需增删）
      ⇒ **建议拆两批**：先 **A**（RAM 页级懒分配 ⇒ 两档可合一、快照载荷瘦身两个数量级）
        再 **B**（内存图由工程/平台描述 ⇒ 新平台不改宿主代码）
  ⚠️ **TX 捕获环（4 KiB）仍不进会话快照** —— v0.24 只补了 RX 侧
```

**关键路径变化（2026-10-20）**：**D-04 不再是瓶颈**——x0 已硬连线，Spike 工具链亦已装好并端到端验证通过（`spike --isa=rv32imc` 退出码 0）。**下一个瓶颈从「修欠账」转为「补能力」**：M-13 的 M/C 扩展、Spike golden 对拍。⚠️ **2026-10-21 更新**：D-07 外设 tick 接线亦已闭环（v0.15）——实测它比台账估计的「最后一步」大得多（**四处断口**），详见 §1.2 的 D-07 条目。

**v0.11 的方法论教训**：这批任务**全部无前置依赖、文件所有权零交集**，因此可以 5 路同时施工、实测 **0 merge 冲突**。下一批的并行切分应继续按「文件所有权」而非「模块边界」来切。

### 2.2 投入产出比排序（跨模块全局 · 2026-10-20 重排）

| 优先级 | 动作 | 成本 | 收益 |
| --- | --- | --- | --- |
| ~~1~~ | ~~修 D-01 + D-02（前端 2 处导出）~~ | ~~3 行~~ | ✅ v0.10 完成 |
| ~~2~~ | ~~加 CI 的 ESM 导入检查~~ | ~~1 行 CI~~ | ✅ v0.10 完成 |
| ~~3~~ | ~~修 D-04 + D-03 + D-05~~ | ~~各 1 处~~ | ✅ v0.11 完成 |
| ~~4~~ | ~~接通 M-14 → M-15 → M-18 三处断点~~ | ~~独立 PR~~ | ✅ v0.11 完成（N5 闭合） |
| ~~5~~ | ~~M-13 扩 RV32IMC 全集~~ | ~~12 人日~~ | ✅ v0.11 完成 **RV32I 41 条**；v0.16 补齐 **RV32M 8 条 + RV32C 26 条** |
| ~~1~~ | ~~Spike golden 对拍~~ | ~~4d~~ | ✅ v0.12 完成（语料 **51/51** 逐例判决一致） |
| ~~2~~ | ~~**M-13 M 扩展（8 条）+ C 扩展（26 条）**~~ | ~~5d~~ | ✅ v0.16 完成（**D-43** 闭环） |
| ~~3~~ | ~~**D-07 外设 tick 接线**（m12 接缝已留）~~ | ~~~1d~~ | ✅ v0.15 完成（实测是**四处断口**，不是一步） |
| ~~4~~ | ~~**WASM 导出层 + 真实 worker 接入**~~ | ~~2d~~ | ✅ v0.13 完成（真内核已接前端） |
| ~~5~~ | ~~**契约缺口 7 项**~~ | ~~~2d~~ | ✅ **v0.17 完成**（`3126f54`）—— 契约层首次升版 `schema_version` 1 → 2，**D-32 闭环**（另闭环 D-34 / D-39 / D-40 / D-57 / D-58）→ 叙事见 `PROGRESS.md` v0.17.0 段 |
| ~~5b~~ | ~~**D-36 + D-37 + D-41：快照落盘对齐 LLD**~~ | ~~3d~~ | ✅ **v0.18 完成**——**落盘一个会话的完整状态，还原出的是一个「等价会话」**。契约层新增六段会话载荷 `SessionSnapshotDto`（段 0~5）· M-14 / M-15 / M-16 补状态导出与载入接口 · M-20 按六段组装 + `SnapshotRecord` 落记录 · M-23 改吃新结构 + `InMemoryDataStore` · 宿主 `capture_session` / `restore_session` 接线 + 端到端等价会话用例。⚖️ **LLD 矛盾以详细设计 §2.1.1 为准**（2026-10-20 裁决）→ 实现按冻结规格 `STAGE-3_Session_Snapshot.md` 落成**六段**（§1.2 裁决 A/B/C）。→ 检视归档见 `REVIEWS.md` **§3.9** |
| ~~5c~~ | ~~**M-16 中断控制器落地 + D-26 / D-59 / D-63 / D-64 / D-70 / D-72 / D-74 闭环**~~ | ~~6d~~ | ✅ **v0.19 完成**（分支 `feature/intc-controller`）—— M-16 从 44 行 stub 变成**完整控制器**（挂起队列 / 同线去重 / 优先级仲裁 / 量子边界采样 / 拉取式交付 / 定时器电平语义 / 会话状态导出导入）· M-13 补 **CSR 指令族 + `mret` + 唯一陷阱入口 `enter_trap`** · M-17 接线（量子边界 → 交付 → 陷入 → `irq_clear` 清线）· 宿主补 `raise_irq` / `lower_irq` / `clear_irq` / `set_timer_compare` · 契约层新增 `csr.rs`（CSR 词汇）· 端到端 6 条。⚠️ **检视轮查出 1 个 P0 真实缺陷**（`irq_clear` 同值重写 ⇒ 中断控制器被永久卡死）⇒ **机制换掉**（M-13 回报「刚写了哪个 CSR」）⇒ 叙事见 `PROGRESS.md` v0.19.0 段 · 检视归档见本文件 **§3.10**。⚠️ **反向变异自检又查出 F-13**：「回报」判据在**六条 CSR 指令臂上各写了一遍**（片段**逐字相同**）⇒ 曾经只有 `csrrw` 一条臂有反向证明，其余**四条臂裸奔** ⇒ 补 M-13 逐臂单测 + 变异 **K-38~K-42**。⚠️ **收口轮再查出 F-14**：验收清单**二次对账**又找出 2 项「有正向用例、无反向变异」的验收项 ⇒ 补 **K-43 / K-44**（变异集 47 → **49** 条）；同时**证伪** 2 个「疑似缺口」（验收 #9 已被 K-08 覆盖 · 验收 #18「`rd == x0` 不读」在本平台**不可观测**）。⚠️ **变异自检的工具链自己修掉五处缺陷**，并暴露「**HANG 有三种形态**」（确凿活锁 / 超时未跑完但**未证实挂死** / 假 HANG）⇒ **旧口径的 HANG 判定整体不可信** ⇒ **K-17 的「挂死」被证伪** ⇒ 「F-1 的后果是平台活锁」这个结论**是错的**（准确表述：**那条线被永久堵死**；**会不会挂死取决于哪条路径**）|
| ~~5d~~ | ~~**M-11 符号表 / DWARF / 反汇编 / trace 导出落地 + D-50 / D-51 闭环**~~ | ~~8d~~ | ✅ **v0.20 完成**（分支 `feature/m11-symbol`）—— M-11 从 53 行 stub 变成**3 206 行 / 75 条用例**（助记符渲染 / `.symtab` 解析 / 反汇编序列 + 按地址缓存 / DWARF 行表降级态 / 轨迹环 + CSV·JSON 导出）· 契约层**升 v3**（4 命令 / 4 事件 / 3 DTO，**只增不改**）· M-06 带出调试节区原始字节 · 前端反汇编面板 + 导出入口 · 宿主接线（**占位分支已删**）⇒ **D-50 / D-51 双闭环**。⚠️ **主线教训**：「**接上了**」和「**钉住了**」是两件事——本批同时抓出「接缝存在但没人用」（D-50 / D-51 / `SymbolService` 端口）与「接上了但没被钉住」（`Waveform+Vcd` 形状 / `overflow_marked` 传播 ⇒ **795+5 全绿**）⇒ 判别力的落点必须在**被依赖方自己身上**。⚠️ **收口时自己抓到三处缺口**：**访存轨迹时间戳恒 0**（M-14 的 `set_now_ps` 从未被生产路径调用）· **缓存缺按地址查询 API**（规格 §9.4 的承诺没有落点）· **缓存失效被挂在 trace 记录开关上**（正确性与性能耦合 ⇒ 关闭期间访客自改代码 ⇒ 缓存陈旧）。⇒ 叙事见 `PROGRESS.md` v0.20.0 段 · 检视归档见本文件 **§3.11** |
| ~~5e~~ | ~~**CSR / 陷阱黄金覆盖**~~ | ~~4d~~ | ✅ **v0.21 完成**（分支 `feature/golden-csr-coverage`）—— 语料 **51 → 64**（13 条 `rv32mi`）· `misa` · 未实现 CSR 写忽略 · `mip` 生产者 · 陷入时 `mstatus.MPP ← M` · ⚠️ 主线教训：「**实测基线**」也会是错的 ⇒ 规格里 4 处**实测为假**的基线被两路独立检视推翻 ⇒ 叙事见 `PROGRESS.md` v0.21.0 段 |
| ~~5f~~ | ~~**Wave 2a · DWARF 行表真解析 + 前端源码高亮 + 访存异常陷阱化**~~ | ~~6d~~ | ✅ **v0.22 完成**（分支 `feature/wave2-dwarf-lines`）—— M-11 的 `dwarf.rs` 从「**只识别不解析**」换成**真解析器**（`.debug_line` 的 v2/v3/v4/**v5** 行号程序 + 行号状态机 + 特殊操作码 + 按单元降级 + 多单元归并）· 前端**源码视图**（源码树只进 M-03 视图状态、**不进内核**；路径尾部逐段匹配 + 正规化；多候选**列出让用户选**）· **未映射访存 ⇒ 架构陷阱**（cause 5 / 7；根因是 `BusMemory` 把所有总线错误重包成 `ISS-01` ⇒ 改为**保留 family/scenario**）· **D-93 / D-98 / D-99 / D-100 / D-101 闭环** · **四路独立检视**（V-A PARTIAL 12 / V-B FAIL 8 / V-C FAIL 15 / V-D PARTIAL 9）⇒ 查出**一条生产路径真断**（源码视图永远拿不到 `sim.pc`，而用例因**手工注入**而绿）+ **三条恒绿断言** ⇒ 全部修掉并逐条做反向变异 ⇒ 叙事见 `PROGRESS.md` v0.22.0 段 · 检视归档见本文件 **§3.13** |

---

### 2.3 整体判断

**做得好的部分**：4 个已开发模块（M-14/15/18/19）合计 9 831 行、194 个测试，模块内部质量普遍高于平均——M-19 的双源解耦、序列号 + DropRange 补洞、k-way 归并导出都是扎实的设计；M-15 的契约 additive 扩展策略（`read_clear` / `on_probe` 默认实现）让 M-0 冻结未被破坏。**这一轮的 review → 闭环 → 复查循环非常有效**，65 项闭环里有相当一部分是「以测试固化既有正确行为」而非改实现，说明代码作者的判断力是可靠的。

**v0.11 新增的好消息**：5 路并行交付证明**这个仓库的模块边界足够干净**——M-13 / M-12 / M-06 / 前端服务层 / 三处接线五组任务，文件所有权零交集，5 个独立 worktree 同时施工，最终 **0 merge 冲突、0 越界改动**（`crates/contract/**`、`lib.rs`、`m17_backend` 全部零改动）。子会话的自查还额外挖出 3 个真缺陷（S/B/J 立即数符号扩展位宽、WebSocket 握手前误报 connected、会话重连无限递归），其中后两个是**只有真实浏览器能暴露**的时序缺陷。

**结构性问题**：

1. **模块做完了，模块之间没接上**。M-14 ↔ M-15 ↔ M-18 三处断点，把 4 个高质量模块的实际价值压在了「总线 trace」这一条窄通路上。这是当前**投入产出比最高的修复方向**——不需要写新功能，只需要接线。
2. **门禁覆盖不到前端与畸形输入**。203 个 Rust 测试全绿的同时前端白屏、M-06 能被一个 4 字节畸形记录打崩。两条最小门禁（ESM 导入检查 + 畸形输入表驱动）能防住绝大部分这类问题。
3. ~~**9 个模块仍是纯 stub**（645 行）~~ —— ⚠️ **2026-10-22 更新**：M-08 / M-09 / M-11 / M-16 / M-17 已陆续落地（v0.13 / v0.20 / v0.19 / v0.13），**M-20 / M-21 / M-22 / M-23 仍是 stub 或部分落地** ⇒ 调试服务与调试面**已通**，**持久化 / 日志 / 健康管理**仍未开工。⚠️ **2026-10-22 二次更新（v0.22）**：**Wave 2a 已完成**（DWARF 行表真解析 + 前端源码高亮 + 访存异常陷阱化）⇒ **D-30（M-12 独立模块级 review）已由 V-D 专项检视闭环**（**带欠账闭环**：本轮 9 条发现已登记为 D-104 ~ D-112 与已修的 4 条）⇒ 下一个瓶颈转为 **契约 v4（D-102 / D-47 / D-48）**、**D-94（S 扩展 / PMP / 计数器）**、**D-103（取指侧 cause 1）** 与 **M-21 / M-22 / M-23 三个 stub**。⚠️ **2026-10-22 三次更新（v0.24 收口复盘）**：契约 v4 / v5 两轮**都已落地**（D-102 / D-47 / D-48 / D-116 均已闭环）⇒ 这一串里的「契约 v4」不再是瓶颈；**当前**的瓶颈是 **D-94（S 扩展 / PMP / 计数器）**· **D-103（取指侧 cause 1）**· **D-114 / D-115（活锁守卫的第二处落点与处置链路）**· **D-123（内存图通用化）** 与 **M-21 / M-22 / M-23 三个 stub**。⚠️ **D-123 值得单独说一句**：它不是缺陷而是**能力缺口**——今天的内存图是宿主**硬编码的两档**，换平台要改宿主代码；**根因是 RAM 急分配**而不是内存图形状的必然 ⇒ 通用化按 **C 做法**（RAM 页级懒分配 + 内存图由工程/平台描述 + 区域按需增删），详见 **§1.3 的 D-123**。

---

---

## §3 检视轮归档

### 3.1 收口轮（2026-10-20 · 契约快照 DTO）

> 3 路独立检视员互不可见，各自独立 worktree / 探针 / 变异沙箱。**判定均为 FAIL**。
> 原始轮次标题：`REVIEW_ROUND_2026-10-20_CONTRACT_DTO.md`（已并入本文件）。

#### 3.1.1 检视员与范围划分

| 代号 | 范围 | worktree | 判定 |
| --- | --- | --- | --- |
| **V-A** | 契约层（`snapshot.rs` / `port.rs` / `model.rs` / `error.rs` / `lib.rs`） | `/tmp/rv-review-a` | FAIL |
| **V-B** | 内核接线与测试有效性（M-12 转换层 / M-17 接线 / 集成测试 / 变异有效性） | `/tmp/rv-review-b` | FAIL |
| **V-C** | 跨模块影响与文档一致性（M-20 / M-23 LLD 一致性 / 全部数字声明 / WASM 成本） | `/tmp/rv-review-c` | FAIL |

**三路共同确认的事实**（彼此独立得出，可互为佐证）：

- D-24 的**根因确实被移除**——`EngineSnapshot` 不可序列化这一障碍已除；
- 作者声明的 5 项变异自检**方向全部成立**（确实变红），但**条数在两种口径下都复现不出**；
- 公开 API **只增不减**，未破坏任何既有类型 / 字段 / 方法签名；
- 前端 **119 用例 / 21 suite** 全绿（`front/` 零改动）；
- 三个 worktree 结束时 `git status --short` **干净**，检视员自建临时产物已全部清理。

**三路各自独立发现同一件事**：V-B 的 H-1 与 V-C 的 H-2 是**同一条缺陷**——`validate()` 未接入任何生产还原路径。两名检视员互不可见却各自撞上，说明该缺陷足够显眼。

---

#### 3.1.2 V-A 报告（契约层）

##### 1.1 判定与门禁

```
$ cargo fmt --all -- --check                                  # FMT_EXIT=0
$ cargo clippy --workspace --all-targets -- -D warnings       # 无告警
$ cargo test --workspace
    contract lib : 8 passed
    kernel lib   : 354 passed
    集成         : 5 + 7 + 13 + 5 = 30 passed
    总计         : 392 passed, 0 failed
```

基线数字与 AGENTS.md / PROGRESS.md 的声明**一致**（`392 = 契约 lib 8 + kernel lib 354 + 集成 30`）——V-A 独立复核为真。

契约层可选 feature 门禁：

```
$ cargo build -p riscv_sim_contract --no-default-features     # BUILD_EXIT=0  ✅
$ cargo test  -p riscv_sim_contract --no-default-features
error[E0433]: cannot find module or crate `serde_json` in this scope
   --> crates/contract/src/snapshot.rs:283:20
TEST_EXIT=101                                                 ❌
```

`main` 上同命令 **通过**（0 tests）⇒ **本分支新引入**此破损。V-A 强调一个关键区分：`cargo test --workspace --no-default-features` **通过**（workspace 内 feature 统一打开了 contract 的 `json` feature），破损只在**单独构建契约 crate**时暴露。

##### 1.2 「可序列化」卖点是否仍成立

**成立**。`serde` 是**非可选**依赖，DTO 的 `Serialize`/`Deserialize` derive 与 JSON 格式无关——探针实测 `EngineSnapshotDto` 在 `default-features = false` 下仍满足 `Serialize + DeserializeOwned`；`json` feature 只提供 JSON 格式。

JS 镜像**现在不需要**：`grep -rn "EngineSnapshotDto\|snapshot_full\|restore_full" front/` → **0 命中**；`front/lib/message.js` 自述只镜像 `protocol::Message`。

##### 1.3 发现清单

**🔴 阻断级：未发现。**

###### 🟠 高危

**🟠-1 契约层对外承诺「完整 / 逐位还原 / 确定性往返」，但外设与中断控制器状态根本不在 DTO 里**

`crates/contract/src/snapshot.rs:117-123` 写 "A **complete** engine snapshot — enough to restore the engine **bit-for-bit**"；`port.rs:93-101` 写 "This is the snapshot form **M-20 persists and M-23 stores**"。

而 `Engine` 自己还持有 `bus`（外设实例）、`intc`（中断控制器）、`advance_steps`，**三者均不在快照里**。外设确实持有大量影响仿真结果的可变状态：

```
m15_peripheral/uart.rs:67-84    tx, rx, status, irq_line, now_ps
m15_peripheral/timer.rs:47-65   mtime, mtimecmp, pending_irq, now_ps
m15_peripheral/gpio.rs:63-83    directions, outputs, inputs, irq_lines, pending_irqs
m16_interrupt/mod.rs:17-21      raised: BTreeSet<u16>
```

探针实证：

```
=== INTC / platform state is NOT part of the DTO ===
source  engine  INTC raised = [7]
DTO json = {"schema_version":1,"state":{...},"queue":[],"next_seq":1,...}
restored engine INTC raised = []
=> the two platforms are NOT in the same state after a DTO round-trip

=== PERIPHERAL STATE IS NOT IN THE DTO ===
UART0 RX byte before snapshot = 90 (0x5a == 'Z')
UART0 STATUS before           = 0x3
DTO JSON mentions 'uart'/'rx'/'fifo'? -> false
```

**影响**：「确定性快照首次可以落盘」只对**引擎自身状态**成立。M-20 / M-23 正是照着契约层这句话来实现与验收的——它们会只存 DTO，然后认为自己存下了一份可复现的会话。**上一轮 F-1「快照语义承诺」被定为高危并「已订正文档」，本分支在契约层重新引入了同类过承诺。**

**建议修法**：把三处 doc 收窄到真实边界并显式列出**不包含**什么。

###### 🟠-2 `validate()` 对两个「真相源」完全失查

- `state.event_queue_head` 与顶层 `next_seq` 不一致 → 不报
- 内层 `SimState.schema_version` 与外层 `EngineSnapshotDto.schema_version` 不一致 → 不报
- `SimState.schema_version` **全仓库无人校验**

实证：`2a event_queue_head=999 vs next_seq=3 -> Ok(())` / `2b state.schema_version=7 vs outer=1 -> Ok(())` / `3h state.schema_version=99 in JSON -> deserialize=Ok(99) validate=Ok(Ok(()))`。

**为什么是高危**：`validate()` 的 doc 自述用途是「快照可能来自**不可信存储**，畸形队列必须被拒绝——否则确定性保证（FR-12）**在无人察觉的情况下**丢失」。而它恰恰漏掉了唯一能暴露「存进来的快照不是这份引擎写出来的」的那个字段。

更危险的一条路径：`SNAPSHOT_SCHEMA_VERSION` 是**编译期常量**，生产端永远写自己。若有人改了 `SimState` 的形状并只把 `SimState.schema_version` 顶到 2（很自然，因为它就在那个结构体里），外层仍是 1 → 老读者 `validate()` 通过 → 而 serde **默认忽略未知字段** → 状态被**静默误读**。

**另一处**：`event_queue_head` 与 `next_seq` 分歧的可达后果是**推演**（in-tree 的 `Engine::restore_full` 恰好用顶层 `next_seq` 并有 `floor` 兜底，**把这个洞掩盖了**，所以现有测试永远抓不到它）。契约层无法为第三方 backend 兜底这一点是确定的。

###### 🟡 中危

| # | 问题 | 实证 |
| --- | --- | --- |
| 🟡-1 | 本分支新引入 `cargo test -p riscv_sim_contract --no-default-features` 编译失败 | main 通过 / HEAD 3 个 E0433 |
| 🟡-2 | 落盘 JSON 形状**无任何测试钉住** | 变异 `Commit` → `CommitWrite`（契约 + 内核同步改，语义等价，纯线格式破坏）→ `passed=392 failed=0` **GREEN**。原因：唯一形状测试是子串断言 `json.contains("commit")`，而 `"commit_write".contains("commit")` 为真。**对照实验**：字段名 `at_ps` → `due_ps`（测试不改）→ **立刻编译失败** `error[E0609]` ⇒ 字段名被钉住、**变体名完全没被钉住** |
| 🟡-3 | 三个 `at_ps` 字段的 doc 完全同义，但**合法地不相等** | 引擎把队列项到期时刻归一化到 `max(at_ps, now_ps)`，而**载荷里的 at_ps 保留原值**。探针：`QueuedEventDto.at_ps = 1000` / `payload at_ps = 0` / `validate() = Ok(())`。且两个字段**语义不同**：`PeripheralTick` 载荷的 at_ps 在 `dispatch` 里是**活的**，`TimerExpiry` 的是**死的**。未来实现者极可能「顺手统一」，那会改变行为 |
| 🟡-4 | `snapshot_full` 默认实现返回软 `Ok(None)`，留出一条**静默不还原**的路径 | 探针（能还原、不能取快照的后端 + 最自然的 `if let Some(dto) = b.snapshot_full()? { b.restore_full(&dto)?; }` 写法）：`restore_full was actually called? false` → **整条还原被静默跳过**。这**正是 D-24 描述的失败模式**。`restore_full` 的大声失败只能保护「已经走到 restore_full」的调用方，保护不了这条路径 |

###### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| 🟢-1 | `validate()` 的**规范序 `seq` 平局裁决**无任何测试覆盖 | 变异 `key <= prev` → `key.0 < prev.0` → **GREEN** |
| 🟢-2 | `model.rs:164-170` doc 事实过期：「the contract has no room for the queue itself」——本分支新增的 snapshot 模块恰恰给了契约层装队列的空间 | `sed -n '162,172p'` |
| 🟢-3 | `ErrorFamily::Snapshot` 的 doc 仍只写 "(M-20)"，但该族现在也承载契约层 DTO 校验码 | `crates/contract/src/error.rs:60-61` |
| 🟢-4 | `SNAPSHOT_FAMILY` 常量在两个 crate 里**重复定义**同值 | `contract/src/snapshot.rs:48` 与 `kernel/src/m20_snapshot/mod.rs:83` |
| 🟢-5 | `EngineSnapshotDto::default()` 的 `instruction_budget = 0`，与引擎默认 `DEFAULT_INSTRUCTION_BUDGET = 10_000` 不一致 | `default DTO: budget=0`；`2g instruction_budget=0 -> Ok(())` |
| 🟢-6 | `validate()` 不检查「队列最早到期时刻 ≥ `state.virtual_time_ps`」 | `2e event at_ps=2000 < virt_time=100000 -> Ok(())` |

##### 1.4 变异测试表（V-A 自测，15 项）

| # | 变异点 | 结果 | 变红的用例 |
| --- | --- | --- | --- |
| m01 | `validate()` 去掉 `seq` 查重 | 🔴 RED | `validate_rejects_a_duplicated_seq` |
| m02 | 规范序检查 `key <= prev` → `key.0 < prev.0` | 🟢 **GREEN** | — **未被捕获** |
| m03 | `restore_full` 默认实现 → `Ok(())` | 🔴 RED | `contract_defaults_are_additive_and_fail_loudly` |
| m04 | `snapshot_full` 默认实现 → `Ok(Some(default))` | 🔴 RED | 同上 |
| m05b | `SchedEventDto::Commit` → `CommitWrite` | 🟢 **GREEN** | — **未被捕获** |
| m06c | `QueuedEventDto.at_ps` → `due_ps`（测试不改） | 🔴 RED（编译错） | `error[E0609]` |
| m07 | 去掉 `schema_version` 检查 | 🔴 RED | `validate_rejects_an_unknown_schema_version` |
| m08 | 去掉 `next_seq` 撞号检查 | 🔴 RED | `validate_rejects_a_next_seq_that_would_collide` |
| m09 | 去掉 `#[serde(rename_all="snake_case")]` | 🔴 RED | `snapshot_dto_json_carries_the_event_queue` |
| m10 | `validate()` 恒返回 `Ok` | 🔴 RED | 4 条 validate 拒绝用例同时红 |
| m11 | DTO 导出丢队列 | 🔴 RED | `engine_snapshot_round_trips_through_the_dto` |
| m12 | `restore_full` 不还原队列 | 🔴 RED | **5 条** |
| m13 | 规范序 `key <= prev` → `key < prev` | 🟢 GREEN | 行为等价（重复项被 seq 查重接住，同为 E_SNAP-02），**非真实盲区** |
| m14 | M-17 `snapshot_full` 谎称不支持 | 🔴 RED | `backend_exposes_the_full_snapshot_through_the_contract` |
| m15 | DTO 转换丢 `at_ps` | 🔴 RED | `engine_snapshot_round_trips_through_the_dto` |

**对作者 5 项自检的独立复核——属实**（M-1(m12) 红、M-2(m11) 红、M-3(m03) 红、M-4(m14) 红、M-5(m01) 红）。作者未做、而本该有的 **m02 / m05b 全绿** ⇒「落盘格式的关键约定」实际上没有测试守护。

##### 1.5 V-A 的未验证项（明示）

前端真机、WASM 构建、M-20/M-23 真实持久化实现、spike 黄金对拍均未跑；M-12 转换层仅做**只读对照**，其内部正确性只被间接覆盖（D-30 仍开）。

---

#### 3.1.3 V-B 报告（内核接线与测试有效性）

##### 2.1 判定

**FAIL**。D-24 原始缺陷已实证闭环，无功能回归，无门禁失败，但**测试有效性存在系统性缺口**。

##### 2.2 变异自检的独立复现

实测 `cargo test --workspace --no-fail-fast`：

| 变异 | 文档声明红测数 | **实测** | 判定 |
| --- | --- | --- | --- |
| M-1 还原不恢复队列 | 4 | 集成 4（`@Engine::restore_full` 则是 kernel-lib 5 + 集成 4 = 9） | ✅ |
| M-2 DTO 丢队列 | 2 | **6**（kernel-lib 1 + 集成 5） | ❌ 少报 |
| M-3 默认 `restore_full` 静默成功 | 1 | 1 | ✅ |
| M-4 M-17 谎称不支持 | 1 | **2** | ❌ 少报 |
| M-5 `validate()` 去 seq 查重 | 1 | 1 | ✅ |

**结论方向成立**（5 项确实全部变红），但**条数在任何口径下都复现不出**。根因：**`cargo test` 默认 fail-fast**，前一个 test target 失败会中止后续 target，红测条数被系统性截断。

##### 2.3 发现清单

**🔴 阻断：未发现。**

###### 🟠 高危

**H-1 落盘加载路径不做输入校验——契约层明文承诺的「畸形快照必须被拒绝」在唯一还原入口上不成立，且零测试覆盖**

契约层原文（`snapshot.rs:151-157`）：

> A snapshot may come from an **untrusted store** (IndexedDB in the browser, a server-side store in P1), so a malformed queue must be **rejected rather than silently loaded** — otherwise the determinism guarantee (FR-12) is lost without anybody noticing.

现实：`m12_engine/snapshot.rs:141-143` `restore_dto` 明写「还原**不做**校验」；`m17_backend/mod.rs:103-107` `SelfIssBackend::restore_full` **既不校验、文档里也只字未提校验**。

全仓证据：`grep -rn "\.validate()" --include=*.rs crates/` → **仅 6 处契约单测 + 1 处集成测试**，**无任何生产代码调用点**。

实证：`restore_full(dto{schema_version:12345, queue:[1 条]})` → 返回 **`Ok`**，队列被还原；`Engine::restore_dto` 接受 `schema_version=99`、重复 seq、非规范序队列，**全部静默接受**（同样输入 `validate()` 则正确报错）。

**变异证据（最硬的一条）**：在 M-17 `restore_full` 首行加 `snapshot.validate()?;` → **0 条用例变红** ⇒「畸形 DTO 被拒绝」这个行为**没有任何测试兜住**。

**影响**：未知 schema 版本被**静默按 v1 解释**（字段重命名/重排即语义错乱）；重复 `seq` 的事件在 `BinaryHeap` 中 `Ord` 相等，出队次序由堆布局决定而非规范序 ⇒ **FR-12 确定性在无任何报错的情况下丢失**。这恰是本轮新开的「确定性快照首次可以落盘」最需要防的场景。

###### 🟡 中危

| # | 问题 | 实证 |
| --- | --- | --- |
| M-1 | `SelfIssBackend.state` 是死字段；新增「保持同步」注释描述了代码并不维持的不变式；`reset()` 仍是空操作 | `m17_backend/mod.rs:24` 声明，全文件 **3 处写、0 处读**；变异 B7（删同步）→ 0 red；变异 C2（彻底删字段 + 全部写入）→ 0 red 且 `clippy -D warnings` 仍零告警 ⇒ 字段完全无用。`reset()` 探针：reset 后引擎 `pending=2`、`snapshot_full().queue=2`、`next_seq=2` ⇒ **reset() 对外完全不可观测**。新注释「so the two never disagree」在 `reset()` 之后**恰恰不成立** |
| M-2 | 把已知缺陷钉成活动断言，会挡住将来的正当修复 | `m12_m17_snapshot_dto.rs:78-96` 断言 partial 路径必然 `[1,2]`。若将来有人修好 `Engine::restore`，该用例变红，修复者极易误判为「引入回归」而回退修复。且该用例**未**断言台账的另一半「从快照时刻出发的运行只有 A」 |
| M-3 | 变异自检的「变红条数」不可复现 | 见 §2.2。文档数字 4/2/1/1/1 在两种口径下都复现不出（4 只在「仅数本次新增的 4 条集成用例」这一口径下成立；2 无法复现） |
| M-4 | 台账 §1.1 的欠账构成与 D-24 闭环不自洽 | 实际 D-25~D-35 共 **11 条**登记在册且未闭环；改动前 19 = 12 + 7，闭环 D-24 后应为 18 = **11 + 7**；文档写成 12 + 6，凭空挪了一条。「4 闭环 = 1 阻断 + 2 高危 + D-24 根因」**重复计数**——D-24 的「来源」列写的就是「检视 V2 · F-1」 |
| M-5 | DTO 路径对「虚拟时钟 / pending_writes / 指令预算 / next_seq」**完全不敏感** | 变异 B3（`restore_full` 丢弃整个 `state`）→ 仅 **1 red**（且是**既有** M-12 单测），**本次新增的 7 条集成用例 0 条变红**；B2（丢 `instruction_budget`）→ **0 red**；B5（丢 `pending_writes`）→ **0 red**；B8（忽略 `next_seq`）→ **0 red**。文档明写「虚拟时间、每量子指令预算全部还原」，但这两项**在整条链路上无任何断言** |

###### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| L-1 | 「M-17 +41 行」与实际 **+42** 不符 | `git diff --numstat` → `42 0`（合计 591→633 是对的，只是增量写错 1） |
| L-2 | `EngineSnapshotDto::default()` 的 `instruction_budget = 0`，而 `Engine::new` 是 10 000 | 默认 DTO 不代表任何真实引擎快照 |
| L-3 | 「队列顺序在往返后保持」是**结构性保证**，不是本次修复带来的 | 变异 B13（`from_ordered_vec` 乱序入队）→ **0 red**，出队顺序不变——`EventQueue` 是 `BinaryHeap`，出队序由 `QueuedEvent::Ord`（`(at_ps, seq)`）决定，与入队顺序无关。⇒ **同刻定序不必另写测试**；DTO 里 `queue` 数组的书写顺序其实无关紧要（`validate()` 却又要求它规范——偏严但无害） |
| L-4 | `Engine::bus()` 访问器**零用例使用** | `intc()` 有 5 条用例依赖（变异 B6 → 5 red）；`bus()` 属「为对称而加」的公开 API |
| L-5 | `state.event_queue_head` 在 DTO 中是**非权威字段** | 还原时被 `next_seq` 覆盖，`validate()` 不检查二者关系 |

##### 2.4 V-B 的变异覆盖漏洞清单（0 条用例变红）

1. **B2** 还原丢弃 `instruction_budget` → DTO 路径的预算还原**零断言**，而文档两处明写「每量子指令预算全部还原」
2. **B4** 去掉 `next_seq` 防御性抬升 → DTO 路径的 **D-17（还原后不撞号）零覆盖**（既有 `restore_does_not_reuse_a_queued_seq` 只覆盖 partial `restore`）
3. **B5** DTO→Engine 丢弃 `pending_writes` → 转换层对 `pending_writes` **零覆盖**（单测夹具该字段为空）
4. **B8** 还原忽略 DTO 的 `next_seq` → 引擎还原后的 `next_seq` **无任何断言**（DTO 层有，引擎层无）
5. **C3** 给 `restore_full` **加上** `validate()` → **「畸形快照被拒绝」这一契约承诺零覆盖**（H-1 的直接证据）

（B7 / C2 为死代码证明、B13 为结构性良性，不计入漏洞。）

##### 2.5 V-B 的未验证项（明示）

真实浏览器 / WASM 构建未跑；`m18_probe` / `m13_iss` 等其他模块的回归深度未做跨模块变异；**H-1 的「修复优先级」是 V-B 的判断**——若协调者决定把校验责任下放到 M-20/M-23 持久化层（当前是 stub），H-1 可降级为「新增台账条目」；但**在此之前，台账「确定性快照首次可以落盘」与「高危已全部闭环」的表述应加限定**。

---

#### 3.1.4 V-C 报告（跨模块影响与文档一致性）

##### 3.1 判定

**FAIL**。

##### 3.2 发现清单

**🔴 阻断：未发现。**

###### 🟠 高危

**H-1 新 DTO 与 M-20/M-23 已冻结的快照格式冲突；M-20/M-23 完全未接线——「M-20 / M-23 的持久化不再被卡」结论不成立**

既有 LLD 早已定义快照载荷形状（`M-20_Snapshot.html` §2.1.1，评审 **20-1**，HTML v1.1 / 2026-10-16）：

```
struct Snapshot {
  Checkpoint cpMeta;   // 检查点元数据（id/tVirt/tHost/baseSeq/version）
  M13State  m13State;  // M-13 指令级状态序列化（寄存器/内存视图）
  M14Map    m14Map;    // M-14 总线/内存映射状态
  M15Periph m15Periph; // M-15 外设状态
};
```

`M-23_DataStore.html` §2.1.1（评审 **23-1**）逐字相同，并声明「与 M-20 v1.1 Snapshot 结构**完全一致**」。

新 DTO = `{schema_version, state: SimState, queue, next_seq, instruction_budget}` —— **没有 `cpMeta`、没有 `m14Map`、没有 `m15Periph`**。

后果（三条都实证）：

1. **M-20 / M-23 的数据结构一个都没改**：`SnapshotRecord { id, at_ps, state: SimState, label }` 仍吃 partial 的 `SimState`；`DataStore::put_snapshot(&SnapshotRecord)` 仍以它为载体；**`SnapshotRecord` 没有 serde derive** ⇒ M-23 的实现（前端 IndexedDB / 服务端 SQLite）**无法直接序列化它**
2. **DTO 不含内存与外设状态**：`SimState` 只有 `gpr/pc/csrs/pending_writes/virtual_time_ps/event_queue_head/seed`，**没有 RAM 内容、没有 M-14 映射、没有 M-15 外设寄存器/FIFO**。把 DTO 还原进一个新引擎，**内存和外设状态全丢**。AD §4.2 明写 `snapshot()/restore()` = 「**全状态**快照/恢复」——新的 full 快照仍不是全状态
3. **恢复模型不同**：AD §8.1 D4 / §7.3.1 与 M-20 LLD §3.3/§3.4 规定恢复 = 「最近检查点 + **稀疏输入事件序列重放**」；新 DTO 走的是**逐字快照队列**。二者可共存（后者是超集），但 DTO 是 LLD 未描述的**新机制**

**建议修法（关键取舍）**：**改 DTO 侧去对齐 LLD**，而不是反过来。依据：① AGENTS.md「契约先行（LLD §2.5）」约束的是**跨模块** DTO，而 M-20↔M-23 的 `Snapshot` 结构是**已经双侧对齐冻结**（20-1 / 23-1）的既有契约资产；② 契约层放一个叫 `EngineSnapshotDto` 的类型、却让它冒充「可落盘的快照」，本身就把层级搞错了——它其实是 LLD `Snapshot` 里 **M-12/M-17 那一段**。

**H-2 `EngineSnapshotDto::validate()` 未接入任何生产还原路径**（= V-B 的 H-1，两路独立发现同一条）

`grep -rn "validate()"` 全仓命中只有契约层自己的 8 条单测 + 1 处集成断言，**生产调用点 = 0**。

补充证据：`EventQueue::from_ordered_vec` 把事件 push 进 `BinaryHeap`，堆的 `Ord` **只看 `(at_ps, seq)`**；两个事件 `seq` 相同 ⇒ 比较**相等** ⇒ 谁先出队由堆内部布局决定，**不再是规范序**——这正是 `validate()` 第 3 条检查要防的事。

**H-3 M-17 的 driving 端口新增 2 个方法，LLD 端口表未同步（AGENTS.md 明文要求先改文档再合入）**

- `crates/contract/src/port.rs:107` / `:118` — `ExecutionBackend` 新增 `snapshot_full` / `restore_full`
- `M-17_ExecutionBackend.html` §9.1.1 端口契约表：**表里没有 `snapshot_full`/`restore_full`**
- LLD 总纲表13 ExecutionBackend 接口：同样只有 `snapshot()` / `restore()`
- 另一处措辞落差：M-17 LLD 写「序列化经 **M-20 契约**（L3）」，暗示快照的序列化契约归 **M-20**；新 DTO 却被放在**共享契约层**。二者不是一回事

###### 🟡 中危

| # | 问题 | 实证 |
| --- | --- | --- |
| M-1 | 新增 4 个 public 访问器，其中 `Engine::bus()` **全仓零使用**，`intc()`/`engine()`/`engine_mut()` 仅被新测试使用 | `grep -rn "\.bus()" crates/ front/` → 0 处。V-C 实测了更收敛方案：把测试里的 `RecordingIntc` 改成持有 `Rc<RefCell<Vec<u16>>>` 即可观察 IRQ，**完全不需要** `Engine::intc()` 与 `backend.engine()` |
| M-2 | `SelfIssBackend::restore` 新增的两行是「只写不读」的死写 | `grep -n "self\.state"` → 3 处命中**全是赋值**，**0 处读取**。注释宣称「Keep the backend's own mirror in step with the engine so the two never disagree」，实际没有任何读者 |
| M-3 | 台账 §1.1「合计」行与自己的行和**不自洽**（差 27 / 19 / 2）—— pre-existing | 逐格重算：行和 162/129/27 vs 文档合计 135/110/25。「v0.11 新增报告」行三格互不自洽（40 + 4 = 44 ≠ 50） |
| M-4 | 「统计口径」段的「前端代码」拆分与实际不符（**总计对、拆分错**）—— pre-existing | views 写 1 045 实为 **1 042**；shell 写 549 实为 **552**；services 写 1 213 实为 **1 241**；dev 写 593 实为 **565**；总计 1 594 / 2 220 ✅ |
| M-5 | 「M-17 增 41 行」应为 **42** —— 本轮引入 | `591 + 41 = 632 ≠ 633` ❌ |
| M-6 | §4.1「处置」列的枚举条数与「项数」格不符 —— pre-existing | 🟡 中：声明 6 项，处置列枚举 3 + 5 = **8**；🟢 低：声明 13 项，枚举 10 + 5 = **15**。而分档加总 `1+2+6+13 = 22` ✅ 与总数自洽 ⇒ 是「项数」格与「处置」格冲突 |
| M-7 | 两个同名同值的 `SNAPSHOT_FAMILY` 常量 | `contract/src/snapshot.rs:48` 与 `kernel/src/m20_snapshot/mod.rs:83`。同时 M-20 HTML §6.1 写的是「无新增码」，需随新增场景码更新 |
| M-8 | `EngineSnapshotDto` 内嵌 `SimState` 带来两个同名 `schema_version`，且**内层版本从不被校验** | 两者嵌套在同一结构体里，却**都自称「快照格式版本」**；一个 `state.schema_version = 999` 的快照能通过 `validate()` |

###### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| L-1 | `crates/kernel/Cargo.toml` 的注释「the shipped WASM binary stays free of serde」不准确 | 契约层**本来就**是 serde 的硬依赖。准确表述应为「dev-dependency 只让**测试**能用 `serde_json`；serde 本身在契约层早已是硬依赖」。**实测代价为 0 字节**（出货 wasm 293 B 两侧相同；DTO 用上时边际 +481 B） |
| L-2 | `tools/build-wasm.sh` 在 main 与 HEAD 上都跑不通 —— pre-existing | `wasm-pack build --target no-modules ... crates/kernel` → `Error: Ensure that you have "wasm-bindgen" as a dependency in your Cargo.toml file`。⇒ AGENTS.md 记载的 WASM 构建路径当前是坏的 |
| L-3 | `EngineSnapshotDto::default()` 是一个「**合法但空**」的快照 | `validate()` 对它返回 `Ok(())`。宿主若把 `default()` 落盘，得到的与「真实快照」在类型上不可区分 |
| L-4 | 「5 项变异自检」的红测条数在 2 处被少报 | M-2 声明 2 实测 5；M-4 声明 1 实测 2。方向保守，实质结论成立 |

##### 3.3 V-C 明确记录的「未发现」

- **无夹带的无关改动**：`git diff --numstat main..HEAD` 12 个文件全部与本次目标相关（契约 3 + kernel 4 + Cargo.lock/Cargo.toml 2 + 文档 3），`front/` **0 文件改动**
- **无对既有 API 的破坏**：两个新方法均带默认实现；全仓仅 2 个实现者；`SimState` / `EngineSnapshot` / `EventQueue` 的既有字段与方法签名**一字未改**
- **无前端回归**：119 用例 / 21 suite 全绿
- **无服务端影响**：`crates/server/` 零改动，编译通过
- **无格式 / lint 问题**
- **D-24 根因确实闭环**

##### 3.4 V-C 的数字核对表（节选，全部逐格复核）

| 位置 | 文档写的 | 实测 | 判定 |
| --- | --- | --- | --- |
| 台账基线行 · 总数 / 拆分 | 392（8 + 354 + 30） | 8 / 354 / 30 | ✅ |
| 台账 §1.1 · M-12 | 226 → 1 292，测 20 | 1 292 / 20 | ✅ |
| 台账 §1.1 · 契约层 | 1 163 → 1 544，测 8 | 1 544 / 8 | ✅ |
| 台账 §1.1 · stub 组 | 591 → 633 | 591 → 633 | ✅ |
| 台账 §1.1 · stub 组括注 | 「增 **41** 行」 | **+42** | ❌ |
| 台账 §1.1 · 合计行 | 135 / 110 / 25 | 行和 162 / 129 / 27 | ❌ pre-existing |
| 台账 §1.1 · 其余 8 行 | 逐个数字 | 逐个完全一致 | ✅ |
| 台账 §四 高危表项数 | 「9 → 10」 | D-06…D-14 + D-24 = 10 | ✅ |
| 台账 §四 中低危表项数 | 「8 → 19」 | 8 + 11 = 19 | ✅ |
| 台账 §4.1 分档 | 1 + 2 + 6 + 13 = 22 | 22 ✅（但处置列枚举 8 / 15） | ⚠️ |
| 台账 §七「4 模块合计」 | 8 575 / 170 | **8 831** / **194** | ❌ pre-existing |
| 台账 §1.2「接线端到端用例」 | 11 | **13** | ❌ pre-existing |
| 台账 §九 5 项变异条数 | 4 / 2 / 1 / 1 / 1 | 4 / **5** / 1 / **2** / 1 | ❌ 少报 |

##### 3.5 V-C 揭示的 LLD 内部矛盾（**需协调者裁决**）

- LLD **总纲**表12/表84/表85 说 M-23 `saveSnapshot/loadSnapshot` 输入是「**SimState + DebugMeta**」，而 M-20/M-23 **详细设计**§2.1.1/§9.1.1 说输入是「`Snapshot{cpMeta, m13State, m14Map, m15Periph}` 完整单对象」—— **总纲与详细设计互相矛盾**
- 总纲表12 说 `SimState` 应含「内存页、外设状态」，契约里现行的 `SimState` **两者皆无**（pre-existing 欠账，新 DTO 沿用了这个缺口）

⇒ 因此「DTO 该对齐哪一边」不能只答「对齐 M-20 LLD」，**需要协调者先裁决 LLD 内部的这个矛盾**。

---

#### 3.1.5 闭环对照表

> 闭环 commit：`3016de0`（实现）→ `560226c`（自证修复）→ `3893626`（检视闭环）

| 来源 | 条目 | 处置 | 状态 |
| --- | --- | --- | --- |
| V-A 🟠-1 | 契约 doc 过度承诺 | 三处 doc 收窄为「M-12/M-17 段（引擎级）」并显式列出不含项；残留（外设/INTC 状态）登记 **D-36** | ✅ 闭环（带 D-36 残留） |
| V-A 🟠-2 | `validate()` 两个真相源失查 | 新增 `E_SNAP_04`（插入序镜像）+ `E_SNAP_05`（内层 SimState 版本）两条检查 | ✅ 闭环 |
| V-A 🟡-1 | `--no-default-features` 编译失败 | 两条 JSON 用例改用 `#[cfg(feature = "json")]` 门控（`560226c`） | ✅ 闭环 |
| V-A 🟡-2 | 落盘 JSON 形状无测试钉住 | 新增 golden JSON **精确形状**测试（含三种变体全量） | ✅ 闭环 |
| V-A 🟡-3 | 三个 `at_ps` doc 同义 | 区分「队列项=权威归一化值」/「载荷=生产者原值」 | ✅ 闭环 |
| V-A 🟡-4 | `snapshot_full` 返 `Ok(None)` 不对称 | 改为**两端对称地大声失败** `E_SNAP-03` | ✅ 闭环 |
| V-A 🟢-1 | 规范序 `seq` 平局无覆盖 | 新增 `validate_rejects_a_queue_with_a_descending_seq_at_the_same_time` | ✅ 闭环 |
| V-A 🟢-2 | `model.rs` doc 过期 | 改为指向 `crate::snapshot::EngineSnapshotDto` | ✅ 闭环 |
| V-A 🟢-3 | `ErrorFamily::Snapshot` doc | 补「+ 契约快照 DTO 校验码（E_SNAP-01..05）」 | ✅ 闭环 |
| V-A 🟢-4 | `SNAPSHOT_FAMILY` 重复定义 | 内核 m20 改 `pub use` 契约层常量 | ✅ 闭环 |
| V-A 🟢-5 | DTO `default()` 预算为 0 | `Default` 改用契约层 `DEFAULT_INSTRUCTION_BUDGET` | ✅ 闭环 |
| V-A 🟢-6 | `validate()` 不检查 `at_ps < virtual_time_ps` | 未做（防御性检查，低优先） | ⏳ **D-39** |
| V-B H-1 | 落盘路径不校验 | `restore_full` / `restore_dto` 改为**先校验后应用**；4 条畸形快照拒绝用例 + 1 条「被拒后引擎原样不动」 | ✅ 闭环 |
| V-B M-1 | `state` 死字段 + `reset()` 空操作 | 删掉 `state` 字段；M-12 新增 `Engine::reset()`（**保留 ticker**），M-17 `reset()` 调它 | ✅ 闭环 |
| V-B M-2 | 表征测试钉死已知缺陷 | 改名 `characterization_partial_restore_diverges_from_the_snapshot_state` + doc 说明「修好 partial 时本用例应更新，不是回归」 | ✅ 闭环 |
| V-B M-3 | 变异条数不可复现 | 台账写入 `--no-fail-fast` 口径 + 「处置列按子项列举」口径说明 | ✅ 闭环 |
| V-B M-4 | 台账欠账构成不自洽 | 「检视轮新增」行改 22 / 3 / 19（11 登记 + 8 模块报告），合计行改 135 / 109 / 26 | ✅ 闭环 |
| V-B M-5 | DTO 路径对四字段不敏感 | 新增 `engine_level_round_trip_preserves_every_snapshot_field` + `pending_writes_survive_the_dto_round_trip` | ✅ 闭环 |
| V-B L-1 | 「M-17 +41」应为 +42 | 订正为 +51（闭环轮实际增量） | ✅ 闭环 |
| V-B L-2 | DTO `default()` 预算为 0 | 同 V-A 🟢-5 | ✅ 闭环 |
| V-B L-3 | 队列顺序是结构性保证 | 证伪（`BinaryHeap` + `Ord((at_ps, seq))`），**同刻定序不必另写测试** | ✅ 留证，不登记 |
| V-B L-4 | `Engine::bus()` 零使用 | 删除该访问器 | ✅ 闭环 |
| V-B L-5 | `event_queue_head` 非权威 | 随 V-A 🟠-2 增加镜像一致性检查 | ✅ 闭环 |
| V-C H-1 | DTO 与 M-20/M-23 LLD 冲突 | 三份 LLD 同步写明「契约 DTO 只覆盖 M-12/M-17 段，落盘格式以 M-20/M-23 的 `Snapshot` 为准」；残留登记 **D-37** | ✅ 闭环（带 D-37 残留） |
| V-C H-2 | `validate()` 未接入生产路径 | 同 V-B H-1 | ✅ 闭环 |
| V-C H-3 | M-17 端口表未同步 | `M-17_ExecutionBackend.html` §9.1.1 端口表补两行 | ✅ 闭环 |
| V-C M-1 | 公开面为可测性而扩大 | 删 `bus()`；`intc()` / `engine()` / `engine_mut()` 保留（已被新测试实际使用） | ✅ 闭环 |
| V-C M-2 | `state` 死写 | 同 V-B M-1 | ✅ 闭环 |
| V-C M-3 | 合计行不自洽 | 同 V-B M-4 | ✅ 闭环 |
| V-C M-4 | 前端拆分失真 | views 1 042 / shell 552 / services 1 241 / dev 565（总计不变） | ✅ 闭环 |
| V-C M-5 | 「+41」应为 +42 | 同 V-B L-1 | ✅ 闭环 |
| V-C M-6 | §4.1 处置列条数不符 | 加口径说明：「处置」列按**子项**列举，故与「项数」格不相等 | ✅ 闭环 |
| V-C M-7 | `SNAPSHOT_FAMILY` 重复 + M-20 §6.1 | 同 V-A 🟢-4；M-20 §6.1 补 v1.2 段列出 5 个 E_SNAP 场景码 | ✅ 闭环 |
| V-C M-8 | 两个同名 `schema_version` | 同 V-A 🟠-2（`E_SNAP_05`） | ✅ 闭环 |
| V-C L-1 | Cargo.toml 注释不准 | 订正为「serde 早是契约层硬依赖，WASM 实测 0 字节成本」 | ✅ 闭环 |
| V-C L-2 | `tools/build-wasm.sh` 跑不通 | 未做（pre-existing，非本分支引入） | ⏳ **D-38** |
| V-C L-3 | DTO `default()`「合法但空」 | 未做（宿主可自行拒绝） | ⏳ **D-40** |
| V-C L-4 | 变异条数少报 | 同 V-B M-3 | ✅ 闭环 |
| V-C §5.5 | LLD 总纲 vs 详细设计矛盾 | **未做**——需协调者裁决 | ⏳ 随 **D-37** 一并裁决 |

##### 4.1 闭环统计

| 口径 | 项数 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| V-A | 12 | 11 | 1（🟢-6 → D-39） |
| V-B | 11 | 11 | 0 |
| V-C | 15 | 13 | 2（L-2 → D-38 / L-3 → D-40） |
| **去重后** | **37** | **34** | **3** |

> V-B H-1 与 V-C H-2 是**同一条缺陷**（两路独立发现），去重后 38 → 37。
> 其中 2 条「闭环但带残留」的条目（V-A 🟠-1 → D-36、V-C H-1 → D-37）——缺陷本身已处置，**剩余工作量另行登记**。

##### 4.2 闭环证据

```
cargo fmt --all -- --check                                ✅
cargo clippy --workspace --all-targets -- -D warnings     ✅ 零告警
cargo test --workspace --no-fail-fast
    契约 lib 12 + kernel lib 356 + 集成 38 = 406 passed / 0 failed
```

**8 项闭环变异自检全部变红**（`--no-fail-fast` 口径）：

| 变异 | 变红的用例 |
| --- | --- |
| M-A `restore_full` 去掉校验 | 4 条畸形快照拒绝用例 |
| M-B 还原丢 `instruction_budget` | `engine_level_round_trip_preserves_every_snapshot_field` |
| M-C DTO→Engine 丢 `pending_writes` | `pending_writes_survive_the_dto_round_trip` |
| M-D 还原忽略 `next_seq` | `engine_level_round_trip_preserves_every_snapshot_field` |
| M-E 事件变体改名 `Commit`→`CommitWrite` | `snapshot_dto_json_shape_is_pinned_exactly` |
| M-F 规范序检查丢掉 `seq` 平局裁决 | `validate_rejects_a_queue_with_a_descending_seq_at_the_same_time` |
| M-G `validate` 去掉插入序镜像检查 | `restore_full_rejects_an_insertion_counter_mismatch` 等 2 条 |
| M-H `validate` 去掉内层 `SimState` 版本检查 | `validate_rejects_an_unsupported_state_schema_version` |

---

#### 3.1.6 台账欠账登记

| ID | 级别 | 来源 | 摘要 |
| --- | --- | --- | --- |
| **D-36** | 高危 | V-A 🟠-1 | 快照 DTO 不含外设（M-15）与中断控制器（M-16）状态 |
| **D-37** | 高危 | V-C H-1 | 契约 DTO 与 M-20/M-23 已冻结的 `Snapshot` 结构不一致，且 M-20/M-23 未接线；**含 LLD 总纲 vs 详细设计的矛盾待裁决** |
| **D-38** | 中危 | V-C L-2 | `tools/build-wasm.sh` 在 main 与 HEAD 上都跑不通（AGENTS.md 记载的构建路径是坏的） |
| **D-39** | 低危 | V-A 🟢-6 | `validate()` 不检查「队列最早到期时刻 ≥ `virtual_time_ps`」 |
| **D-40** | 低危 | V-C L-3 | `EngineSnapshotDto::default()` 是「合法但空」的快照，宿主无从拒绝 |

---

#### 3.1.7 方法论沉淀

1. **变异测试是本轮唯一能发现「文档说做了、代码没做」的工具**。V-B 的 C3（在 `restore_full` 加 `validate()` → **0 red**）与 V-A 的 m05b（变体改名 → **392/392 全绿**）都是**加法**方向的变异——不是「删功能看是否变红」，而是「加正确性看是否有人守」。这类盲区用常规「删功能」式变异**测不出来**。
2. **fail-fast 会系统性截断红测条数**。同一变异 fail-fast 下 5 条、`--no-fail-fast` 下 9 条。**所有变异计数必须显式写明口径**，否则下一个检视员会卡在同一处（V-B M-3 即为此而开）。
3. **多条检视员独立撞上同一条缺陷，是缺陷「足够显眼」的强信号**。V-B H-1 与 V-C H-2 在互不可见的条件下各自被发现，且 V-C 额外给出了 `BinaryHeap` 的 `Ord` 只看 `(at_ps, seq)` 这一根因。
4. **过度承诺比缺陷更难发现，也更贵**。V-A 🟠-1 与上一轮 F-1 是**同一类错误**——契约 doc 承诺了它兑现不了的语义。它不会让任何测试变红，只会在未来的实现者手里变成事故。
5. **「文档数字」本身就是需要被检视的产物**。V-C 用一张表逐格复核出 8 处失真数字（含 pre-existing 的），且其中「合计行不自洽」是**逐格重算**才暴露的——肉眼扫读看不出来。

---

### 3.2 独立检视轮（2026-10-20 · v0.11.1）

> 4 路独立 verifier 会话，各自独立 worktree，**互不可见**；每路用**变异测试**判定「现有测试是否真能抓住实现错误」。
> 原始轮次标题：`REVIEW_ROUND_2026-10-20.md`（已并入本文件）。

#### 〇、为什么先检视后合 main

Wave 1 交付后**不直接回合 `main`**，而是先拉起独立检视。原因是这一轮的交付方式本身有风险：5 个会话在互不可见的 worktree 里同时写代码，**没人看过别人的 diff**。检视的价值不在于「再读一遍代码」，而在于**用变异测试反问：现有测试到底抓得住什么？**

答案是：V1 的 32 项变异里，**有 4 项改回去没有任何现有测试会红**，其中 1 项是真缺陷（阻断级）。这 4 个「覆盖漏洞」比任何单点 bug 都更值得修——它们意味着门禁在关键处是瞎的。

---

---

#### 一、V1 · M-13 ISS（RV32I）

**判定：FAIL**（1 阻断 + 3 附带）· 32 项变异 + 29 个探针

| # | 发现 | 处置 |
| --- | --- | --- |
| 阻断 | **SYSTEM opcode 未校验 `funct3`** —— 12 位立即数判 ECALL/EBREAK，CSR 形态编码（`0x00001073`）被误判为 ECALL，mcause 报 11 而非 2。**当时无任何现有测试能抓住** | ✅ 已修（`caf7f82`）：`match (d.funct3, d.imm)`，funct3 ≠ 0 一律判非法指令 + 3 条表驱动用例；**反向验证：改回原样 4 条用例变红** |
| 附带 2 | `sequential()` 按 `d.len` 步进但**执行路径无测试锁定**（D-16 覆盖漏洞） | 补 `sequential` 单元用例（len=2 → +2）+ 真实解码交叉验证 |
| 附带 3 | **有效地址回绕**（`x1=0xFFFFFFFF` + imm）无覆盖 | 补 load / store 两条用例，断言回绕后访问的是回绕地址且 debug 构建不 panic |
| 附带 4 | `truncate_store` 的语义此前无契约说明 | **裁定：保留截断**——它承载架构语义（M-14 总线对外设写时透传原值）。补 3 条外设端到端用例锁定 |

**对协调者两处纠偏的独立判断**（V1 独立复核，均认同）：

- **非对齐访存默认策略改 `Allow`**：**正确**。LLD §3.6(2) 与 §5.2.1 两处原文都写「默认不触发」「保持 golden 兼容」；`crates/contract/src/` 零处提及非对齐访存，**契约里根本没有这个要求**——实现者 review 把它当成「本轮契约明确要求」是自我设定。默认 `Trap` 会让 ISS 在任何触碰非对齐地址的程序上与 Spike 分叉，而那正是 golden 对拍要测的。
- **LLD §3.3 的 LUI/AUIPC 回写**（`imm<<12` → `imm`）：**正确**。§3.1 说 U 型立即数已含左移，§3.3 是二次左移。全文已无残留 `<<12`。

> 📄 **逐条明细见 `M-13_ISS_Review.md` §一**（14 项问题汇总表，#14 即上述阻断级缺陷；§四 闭环核对给出逐条反向验证）。

---

#### 二、V2 · M-12 引擎 + M-14/M-15/M-18 接线

**判定：FAIL**（1 阻断 + 2 高危 + 3 中危 + 3 低危）· 19 个探针 + 5 项变异

##### 🔴 阻断级 · F-1：`ExecutionBackend::restore` 不是确定性还原

契约 `port.rs` 写「Restore a deterministic snapshot」，但 `SimState` **不含事件队列**（契约无此空间），`Engine::restore` 只还原平台状态与虚拟时钟，队列原样保留。实证：

```
schedule A(1, 5000); snapshot;
schedule B(2, 5000); restore(snap); run_until_idle();
→ raised = [1, 2]      // 「从未 restore 的同种子」只会 raise [1]
```

D-05 台账标注的「快照还原 now_ps + 事件队列」只覆盖了 `snapshot_full` / `restore_full` 路径，**没覆盖契约层的 partial path**。M-17 / M-20 调的正是 partial path。

**处置**：本轮**只订正文档承诺**（`7254a00`），不改 API 形状——`crates/contract/src/port.rs` 的 `snapshot` / `restore` 改述为「partial」，`m12_engine::restore` 的 docstring 补上具体发散场景与 `snapshot_full` / `restore_full` 的使用指引。
**根因未除**：`EngineSnapshot`（含队列）**不可序列化**——kernel crate 无 serde，契约层放不下。已登记 **D-24**，Wave 2 决策（契约新增可序列化 DTO 倾向 vs 允许 kernel 引入 serde）。

##### 🟠 高危 · F-3：总线 hook 的 RefCell 重入 panic

`make_bus_hook` 用裸 `borrow_mut()`，而**同模块**的外设探针回调 `wire_signal_probe` 已用 `try_borrow_mut()` + 失败计数正确处理了争用——两侧不对称。调用方只要在 `bus.read/write` 期间持有 `probes.borrow_mut()`，整场仿真直接 panic。

**修复**（`bd647dd`）：改 `try_borrow_mut()`，争用时丢弃该次访存记录并把 `borrow_contended` 计数 +1，**绝不 panic**。补 3 条用例（不可变借用 / 可变借用 / 无争用正常路径）。
**构造期前置条件**（`reg.borrow()` 取计数句柄）已登记 **D-31**。

##### 🟡 中低危

| # | 发现 | 处置 |
| --- | --- | --- |
| F-2 | `SimState::event_queue_head` 文档写「queue head pointer」，实际是单调插入序计数器 | 已订正文档（`7254a00`）——按字面理解的实现者会把它当堆索引用 |
| F-4 | E_BUS-05 把「外设抛错 / 实例忙 / 未接线」三种子情况合并成同一 code + scenario | → **D-29** |
| F-5 | 3 份 LLD 的探针信号命名与实现不一致，且 LLD 之间自身也不一致 | **已裁决并落地**：改 LLD 对齐实现（3 份 HTML，零代码风险）。详见 §五 |
| F-6 | D-07「半成品」：M-12 有 `PeripheralTicker` 接缝但没人装配 | 沿用既有 **D-07**（Wave 2） |
| F-7 | M-14 外设映射路径的区域上限**只有一层防线**且无测试守护 | 补测试（`0fbd6e7`）：1 GiB 窗口的外设必须被 E_BUS-09 拒。**变异验证**：关掉 `map_region` 的上限检查 → 只有新用例变红，`map_ram` 用例仍绿（被 `prepare_bytes` 兜住）→ 盲区真实且已覆盖 |
| F-8 | `Engine::restore` 的「partial」语义在公共 docstring 里标记得太弱 | 已加强（`7254a00`） |
| F-9 | GPIO SingleBit 通道与 32-pin 位图错位——多脚位只有 pin 0 有升降沿 | → **D-28** |

---

---

#### 三、V3 · M-06 ELF / Raw-HEX 加载器

**判定：PASS**（2 中 + 1 低）· ~26 000 次随机 / 边界输入 **0 panic**

| # | 发现 | 处置 |
| --- | --- | --- |
| M-1（中） | 报告 §四·#7 与 §五 把 D-10 标为「M-14 侧未做」，但 `60dbbb8` 早已补上 E_BUS-09 守卫 | 已订正文档（`9992419`） |
| M-2（中） | D-22 变异后 47 条测试仍全绿 | 已在报告 §四·#4 补注：限额是**契约性保证**（trait 默认实现已删除，漏实现就编译不过），**不是**回归测试锁死的。后续改动需靠人工复核 |
| L-6（低） | `elf.rs` bss gap 基址用 `wrapping_add`，溢出时静默回绕 | 改 `checked_add` + 明确错误分支。**注**：上游 `paddr_end > max_paddr` 检查已先拦下越界段，该溢出实际不可达，此为防御性加固 |

**另修 V3 顺带发现的嗅探假阳性**（P1）：`looks_like_ihex` 只看首字节是否为 `:`，导致**首字节恰为 `0x3A` 的原始二进制镜像**被误判成 HEX 而加载失败。改为结构判据（记录标记 + 长度 ≥ 11 + 长度为奇数 + 声明字节数与实际数据长度一致 + 标记后全为 hex 数字），与 IHEX 解析器自身的校验（`bytes.len() == count + 5`）一致。补 3 条用例。

**变异测试结论**：D-03 / D-15 的门禁**强**（改回原样立刻变红）；D-08 / D-09 **仅单测级**；D-22 **无测试可观测**（仅 trait 层契约保证）。

> 📄 **逐条明细见 `M-06_ELFLoader_Review.md`**（17 项问题汇总表 + §四 闭环核对 + §六 可复现命令）。

---

#### 四、V4 · 前端服务层 + 收口文档

**判定：PARTIAL**（4 高 + 4 中 + 5 低）· 26 payload × 5 enum 逐字比对 + 17 项变异

| # | 发现 | 处置 |
| --- | --- | --- |
| F1（高） | **mock 内核 `ClearBreakpoint` 按 `addr` 删，而契约是 `ClearBreakpoint { id }`** —— 发 `{ id: 1 }` 删不掉断点 | **已修**：改为按 id 反查地址再删。Wave 2 接真实 WASM 内核时第一次走这条路径就会踩坑 |
| F2~F5（高） | **台账 / PROGRESS / AGENTS 的数字失真 3~4 倍**，且同一文档内「314」与「334」自相矛盾 | **已订正**并写明统一口径（见下方「数字口径订正」） |
| F6（中） | 契约测试只验变体名，**不验变体内层字段名**（把 `id` 改成 `ident` 无测试变红），与文档自述的「任何一侧单边改名都会撞红」不符 | **已修**：以 Rust 变体清单为驱动逐字校验字段名，覆盖 CommandPayload / EventPayload 全部变体 + 7 个 DTO |
| F7（中） | M-10 断线重连后**不重订阅**（`subscribed` 标志永不重置），接真实内核会丢寄存器增量 | **已修**：断线时复位该标志 |
| F8（中） | `once()` 的一次性语义不可观测（删掉 `off()` 无测试变红） | **已修**：补可选观察者参数 + 计数 spy 用例 |
| F9（中） | **浏览器真机复验脚本在 `/tmp`，任何后续 Agent 都无法复现** | **已修**：落仓 `tools/browser-verify.mjs`（零依赖 CDP，Node 22 全局 `WebSocket` + `fetch`） |
| F10（低） | 5 处变异漏抓 | 由 F1 / F6 / F8 的修复覆盖；mock x0 与 SetBreakpoint 另补用例 |
| F11（低） | 契约 `model.rs` 同时有 `Waveform`（history 形）与 `WaveformDelta`（live 形），而 `EventPayload::WaveformDelta` 包的是 `Waveform` | → **D-34**（设计问题，非缺陷） |
| F12（低） | mock 内核 `reset()` 不清 `state.topics`，换工程后旧订阅仍有效 | **已修**：清理 + 连带修正 `app.js` 的重订路径 |
| F13（低） | mock 内核 PC 永远 +4，无 RV32C | → **D-35**（已知限制） |
| F14（低） | M-13 报告 §一 #6 与 §四 #2 自相矛盾（把「待对拍时翻转」写成「契约要求」） | 已订正（`9992419`） |

> 📄 **逐条明细见 `FRONTEND_ServiceLayer_Review.md`**（服务层 4 模块的问题汇总 + §五 契约缺口 + §六 验证证据 + §七 遗留欠账）与 **`FRONTEND_Review.md`** §七~§八（前端 M-01/02/03 的首审、复审与二次复验）。

---

#### 五、F-5 专项裁决：改 LLD 对齐实现

检视 V2 提出 F-5 后，另派一轮专项核对（三份 LLD + 实现的信号名全量取证 + git 历史 + review 记录），结论：

**F-5 成立，但定位需重新界定**——真正的冲突重心在 **M-15 LLD**（规范性伪码 +「信号名固定可断言」的测试要求），而不在 M-18 LLD 的 `channelDefMap` 示例（该处是设计期标签、槽位混用、运行时 API 根本不读它）。

**裁决：改 LLD 对齐实现。** 依据：

1. **契约层是最高权威且已说「OUT」/「TX」**——`contract/src/port.rs` 的 `ProbeSink` 文档如此，按 AGENTS.md「契约先行」优先于模块 LLD。
2. **实现的 `source` / `signal` 二分是结构性必需**——M-18 的 lookup 键是 `(periph_id, signal_id)`，LLD 的 `channelDefMap` 只有 2 个标识字段且 `source` 写成散文，表达不了这个键。
3. **M-18 LLD 的运行时 API 全部按通道号键控**（`attachSignal(periph, ch)` / `sample(time, ch, value)`），从不读 signal 字符串 → 那 4 个 `signal=` 值是设计期元数据，不是绑定契约。
4. **「gpio0」当信号名在语义上更差**——M-19 的标签是 `source.signal`，改实现会让标签退化成 `GPIO0.gpio0`，并推翻契约 `model.rs` 的 `"UART0.TX"` 点分约定。
5. **LLD 家族自相矛盾**——M-15 写 `timer_cmp`、M-18 写 `timer-compare`（连分隔符都不同），挑任一 LLD 的拼写都是任意的。**唯一三层自洽的是实现方案。**
6. **成本差一个数量级**——改 LLD：3 个 HTML、约 16~18 处、**零代码风险、零测试 churn**；改实现：约 8 个文件 / 30+ 处，且需额外裁决 UART 信号语义（字节流 vs 电平），实际是**功能变更伪装成改名**。
7. **政策先例**——两天前的 `00f1ab5` 刚做过同构的「设计纠偏」（M-13 LLD §3.3 与实现矛盾 → 按实现回写 LLD）。

**落地**：`M-18_Probe.html` v1.2 / `M-15_PeripheralModel.html` v1.4 / `M-16_InterruptController.html` v1.4，零 Rust/JS 改动。
**同时新登记 3 条真实功能缺口**（改名不解决它们）：D-25（UART RX/CTS 未实现）、D-26（M-16 仍是 stub → 中断脉冲通道无生产者）、D-27（按键通道无生产者）。

---

---

#### 六、数字口径订正

V4 指出台账 / PROGRESS / AGENTS 的数字失真 3~4 倍，且同一文档内自相矛盾（「314」是 lib-only，「334」把 lib 也算进去了，而 `cargo test --workspace` 的真实输出是 355）。现统一：

| 口径 | 定义 |
| --- | --- |
| 「代码」 | 模块目录下**全部 `.rs` 文件的总行数**（含测试文件） |
| 「测试」 | 该目录下 `#[test]` 的**用例条数** |
| `cargo test --workspace` | 一律写**总数**，并拆出 lib / 集成 |
| 代码规模 | `git diff --shortstat main..HEAD` |

| 指标 | 订正前 | 订正后 |
| --- | --- | --- |
| `cargo test --workspace` | 314 / 334（自相矛盾） | **375**（lib 352 + 集成 23） |
| 前端门禁 | 109 / 19 suite | **119 / 21 suite** |
| 内核代码规模 | +1 500 行 | **+6 917 行** |
| 前端代码规模 | +1 800 行 | **+4 255 行** |
| M-06 / M-12 / M-13 代码行数 | 1 999 / 490 / 2 481 | **2 652 / 1 060 / 2 962** |

---

---

#### 七、本轮方法论

1. **变异测试比读代码更有效**。V1 的 32 项变异中 4 项「改回去没测试会红」——这直接暴露覆盖漏洞，比逐行 review 更快定位到「门禁在哪儿是瞎的」。
2. **补的每条用例都必须反向验证**。本轮所有新增用例都做了「改回原样必须变红」的验证，确保不是安慰剂。V2 的 F-7 用例尤其典型：单看「测试全绿」会以为覆盖到了，变异后才发现 `map_ram` 用例是被**另一层**防线兜住的。
3. **文档与实现的矛盾要查历史**。F-5 若只比当前状态会得出「随便挑一边改」；查 git 历史 + review 记录才发现，实现方**早已识别为待裁决项并上收**（`M-14_M-15_M-18_Wiring_Review.md` §2.2 / §5.2 O2）——这不是实现方失误，是待裁决项至今未裁决。
4. **台账数字失真是实质缺陷**。台账定位是「唯一追踪入口」，数字错会让外部 review 与排期（Spike 对拍人日、覆盖率基线）整体失真。

---

---

#### 八、闭环状态

| 严重度 | 发现 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| 🔴 阻断 | 1 | **1** | 0 |
| 🟠 高危 | 2 | **2** | 0 |
| 🟡 中危 | 6 | 3 | 3（D-25 / D-29 / D-30 / D-32 / D-33） |
| 🟢 低危 | 13 | 9 | 4（D-27 / D-28 / D-31 / D-34 / D-35） |

**新增台账条目 D-24~D-35 共 12 条**。全部登记在 `docs/reviews/REVIEWS.md`。

---

## §3.3 第 2 阶段检视轮（2026-10-20 · golden 对拍）

> 分支 `feat/golden-harness`（base `fe039bf`）合 main 前的 **3 路独立检视**。检视员在三个 detached worktree（`/tmp/rv-golden-{a,b,c}`）里互不可见，各自独立跑门禁 + 变异自检 + **真实 Spike 对拍**（本机 spike 1.1.1-dev / riscv-none-elf-gcc 15.2.0 / riscv-tests clone 齐备）。
> 判定：**V-A = FAIL**（3 🟠 + 5 🟡 + 8 🟢）· **V-C = PASS**（2 🟠 必修 + 6 🟡 + 6 🟢）· V-B 见 §3.3.4。

### 3.3.1 V-A 报告（runner 本体）

三路里唯一的 FAIL。三项 🟠 **全部落在「裁判工具本身」**——不是语料不够，是裁判会判错：

| # | 问题 | 实证 | 闭环 |
| --- | --- | --- | --- |
| 🟠 H-1 | **判决解码用了 64 位字，Spike 只用 48 位 payload**。`fesvr/device.h`：`payload() { return tohost << 16 >> 16; }`——bit 48..63 被移出。且退出码是 `exitcode >> 1`，**先 64 位右移、再截成 int**；原实现先右移、**先判零再截断** | `tohost = 0x0000_0002_0000_0001`：Spike 判 **PASS**（(1<<32) 截成 int = 0），原实现判 **`FAIL 0`**（协议里 0 就是 PASS）——裁判能把 PASS 判反，还能报出协议上不存在的判决 | ✅ `f9b86f3`：payload 先 `& 0x0000_ffff_ffff_ffff`，退出码**先截 32 位再判零** |
| 🟠 H-2 | **命令行错误直接 `panic!`**（exit 101、无 `VERDICT` 行），违反文件自己写的输出契约 | `--tohost zzz` / `--budget abc` / `--tohost` 缺值 / 大写 `0X` 前缀 → 4 条全 panic | ✅ `f9b86f3`：解析返回 `Result`，错误走 `VERDICT ERROR …` + exit 3 |
| 🟠 H-3 | **`-v` 的段尾打印用裸 `+`**，debug 下溢出即 panic（M-06 允许段恰好结束在 4 GiB） | 同一文件不加 `-v` 是干净的 → 纯 `-v` 专属缺陷 | ✅ `f9b86f3`：改 `wrapping_add` |
| 🟡 M-1 | **runner 零单测**：12 个逻辑变异全部逃过 `cargo test`（M0 对照组证明**确实编译**了这个 bin，不是构建图问题） | 变异 M1–M12 → `cargo test --no-fail-fast` **0 红 / 406 全绿** | ✅ `f9b86f3`：补 **11 条单测**（畸形 ELF 表驱动 11 例 + HTIF 解码 + CLI 解析） |
| 🟡 M-2 | **语料 42 例全是 PASS 用例**，FAIL 编号路径只被 2 个金丝雀守着，而 `--self-check` **不在默认门禁里** | 变异 M1（`>>1` 改 `>>2`）/ M11（编号 +1）→ 语料 42/42 全绿 | ✅ `cdbcffb`：金丝雀**纳入常规路径** |
| 🟡 M-3 | **候选侧内存图 ≠ 参考侧**（64 KiB vs 固定 4 MiB） | guest 写 `0x80010000`：Spike `rc=0`，我们 `E_BUS-01 write unmapped` | ✅ `cdbcffb`：两侧共用同一张图 |
| 🟡 M-4 | `tohost` bit 0 为 0 时我们跑到预算耗尽，Spike 走代理 syscall 分支 abort | 分类不同 → 记成 MISMATCH | ⏳ 已在 runner doc 与 README 写明「本 runner 不实现 HTIF 代理 syscall 分支」 |
| 🟡 M-5 | **栈窗口映射在 `sp` 上方**，RISC-V 栈向下长 → 第一次压栈就出界 | `sw t0,-4(sp)` → 我们 `E_BUS-01 write unmapped addr=0x7fffeffc` | ✅ `f9b86f3`：窗口改为 `[0x7ffe_f000, 0x7fff_f000)`，`sp` = 上界，另加 `const _` 编译期断言 |
| 🟢 L-1/2/5/7 | tohost 读失败被 `.ok()?` 吞掉 / 参数错误不打 VERDICT 行 / `fs::read` 无上限 / `sh_entsize` 未校验 ≥16 | — | ✅ `f9b86f3` |
| 🟢 L-6 | 候选侧无墙钟超时，`budget=u64::MAX` 会让脚本永远挂住 | — | ✅ `cdbcffb` |

**V-A 的变异表（12 项）关键结论**：`cargo test --workspace --no-fail-fast` 对 runner 的**任何**逻辑缺陷都是 **0 红**——因为 runner 当时没有一条测试。这是本轮最值钱的一条方法论教训：**新增组件若不进 `cargo test`，变异测试的默认口径对它完全失效**。

### 3.3.2 V-C 报告（跨模块 + 文档一致性）

判定 PASS，但抓到**两项让「门禁绿」与「有判别力」脱钩**的 🟠——与本分支自己在 `PROGRESS.md` 里写的「一个不会失败的对拍比没有对拍更糟」直接矛盾：

| # | 问题 | 实证 | 闭环 |
| --- | --- | --- | --- |
| 🟠-1 | **`NOVERDICT == NOVERDICT` 被判成「一致」** → 参考侧超时 + 候选侧预算耗尽（两侧都没出判决）时计入 `ok++` 且 `exit 0` | 桩 oracle 让参考侧必然超时：`total=1 ok=1 … PROBE2_EXIT=0`——**绿灯** | ✅ `cdbcffb`：NOVERDICT 单独成类，**任一侧无判决即判负**（新增 `no-verdict` 计数） |
| 🟠-2 | **金丝雀缺失被静默放过**（canary B 找不到源文件时只打印 SKIPPED，自检仍报 ok 并 `exit 0`） | `RISCV_TESTS_DIR=/tmp/fake` → 判别力证明从 2 条静默降级为 1 条，门禁无感知 | ✅ `cdbcffb`：SKIPPED 路径置失败 |
| 🟡-1 | 「42/42 **判决与失败用例号**全一致」在语料内是**空命题**（42 例全是 PASS） | 实跑输出 42 行全 `PASS PASS ok` | ✅ 措辞订正：语料证「判决」，金丝雀证「失败用例号」 |
| 🟡-2 | runner 的 `sp` 初值 / 注释 / env **三处不一致**，且初值落在镜像基址 `0x8000_0000` | 旧代码 `gpr[2] = STACK_BASE + STACK_BYTES` = **0x8000_0000** = `RAM_BASE` | ✅ `f9b86f3` |
| 🟡-3 | **分数秒超时会让 `alarm(0)` 取消闹钟**，超时机制整个失效 | `GOLDEN_TIMEOUT=0.001` → spike **没有**超时，正常跑完报 PASS | ✅ `cdbcffb`：超时秒数必须正整数，否则报错退出 |
| 🟡-4 | `find_symbol` 是**第二套 ELF 解析器**且 0 测试覆盖 | — | ✅ `f9b86f3`：补 11 条单测（与作者的自披露「M-11 落地后应切换过去」一致） |
| 🟡-5 | AGENTS.md 写「Spike 对拍脚本放 `tests/golden/`」，实际在 `tools/` | — | ✅ 口径订正 |
| 🟡-6 | D-42 的修复指针只列 M-13，**漏掉 M-14 LLD 的同款措辞** | M-14 LLD §3.2 / §6.2(1) 同样写「与 Spike golden 一致」 | ✅ **当轮订正** M-13 + M-14 四处措辞，D-42 闭环 |
| 🟢-1~6 | 双横线 / §0.1 行数过时 / README 硬编码计数 / commit 类型标注 / URL 硬编码 darwin-arm64 / D-42 是自造欠账 | — | ✅ 分别修于 `cdbcffb` 与本轮文档 commit |

**V-C 的独立核验（未发现问题）**：契约层 **零改动**；M-12 / M-14 / M-15 / M-17 / M-19 **零改动**，公开 API **零变化**，**没有为 harness 拓宽任何一格可见性**；WASM 产物与 base **字节级完全一致**（`cmp` 四项全 IDENTICAL）；`cargo test` 计数 406 不变；D-01~D-43 零缺失零重复；42 个 commit hash 全部可解析；语料对上游 riscv-tests **100% 覆盖**（42/42 + 8/8 + 1/1）；无产物误入库。

**模块边界判定（V-C）**：**不构成违反**。M-17 的「唯一入口」规则主语是**调试服务（M-08~M-11）**，harness 不在其射程内；M-17 的定位是「面向调试会话的执行代理」，对拍要的是**确定性单步**，恰恰不该把 M-12 事件引擎拉进循环；runner 直接从**契约层**取 `SimState` 是朝契约层靠拢的正确方向。唯一真实代价是「第二套 ELF 符号读取器」，作者已自披露并给出 M-11 迁移计划。

### 3.3.3 检视闭环的净效果

| 项 | 检视前 | 检视后 |
| --- | --- | --- |
| `cargo test --workspace --no-fail-fast` | 406 | **417**（+ runner 11 条单测） |
| runner 单测覆盖 | **0 条** | 11 条（畸形 ELF 11 例 + HTIF 解码 5 例 + CLI 4 例） |
| 对拍脚本的假绿漏洞 | 2 处 | **0**（NOVERDICT 判负 + 金丝雀缺失判负） |
| 判决语义与 Spike 的偏差 | 48 位 payload + 截断顺序（**能把 PASS 判反**） | **逐位对齐**（`fesvr/device.h` 口径） |
| 两侧内存图 | 不一致（64 KiB vs 4 MiB） | **一致** |
| 候选侧墙钟超时 | 无 | 有 |
| LLD 的 Spike 兼容性表述 | 缺 Zicclsm 限定（M-13 + M-14 四处） | **已订正**（D-42 闭环） |

### 3.3.4 V-B 报告（env 与对拍有效性）

判定 **FAIL**（2 🟠 + 5 🟡 + 5 🟢）。V-B 的价值在两处：**它证伪了我们对金丝雀的自我评价**，以及**它逐条证明了 env 与上游的判决编码字节级等价**。

#### 🟠 O-1 金丝雀的判别力边界（本轮最重要的一条方法论结论）

> **金丝雀抓不到成功路径的 bug。**

金丝雀执行的是 `li` / `beqz` / `sll` / `or` / `sw` / `auipc` / `j` 这 7 条指令构成的**判决运输路径**。凡不在该路径上的指令被改错，金丝雀**在原理上不可能**察觉——它根本不执行它们。

| 注入的缺陷 | 金丝雀 | 全量对拍 |
| --- | --- | --- |
| `alu.rs` 的 `xor` 语义改错 | ❌ **漏检**（自检报 ok） | ✅ 抓到（`rv32ui/xor`、`rv32ui/xori` → MISMATCH） |
| `alu.rs` 的 `sll` 语义改错 | ❌ **漏检** | ✅ 抓到 |
| 判决解码 `value >> 1` → `value` | ✅ 抓到（`FAIL 7` vs `FAIL 3`） | ✅ 抓到 |

**实测比例：10 个 RV32I ALU 变异，金丝雀漏检 8 个；全量对拍 10/10 全抓到。**

被金丝雀抓到的两个（`add` / `or`）是**用例选择的巧合**：`add` 之所以被抓到，是因为金丝雀 B 恰好就是 `add.S`；`sll` 明明在判决路径上却漏检，是因为紧随的 `or ... 1` 抹掉了 `^1` 变异——进一步说明覆盖面是偶然的、不可靠的。

> **结论**：`--self-check` 的正确定位是「**协议握手自检**」（两侧能就同一个失败编号达成一致），**不是**「对拍有判别力的证明」。真正提供判别力的是**全量对拍**——Spike 是**独立 oracle**，同一份 ELF 在两侧跑出的判决必须一致。

**处置**：① README 的措辞**下调**并明写「它不证明什么」；② 补**阳性对照**金丝雀（未改动的上游 `add.S` 两侧都必须 PASS），关掉「永远报 FAIL」这条假绿通道；③ 更强的两种形式（判决序号覆盖式变异 / 候选侧故障注入）登记为 **D-44**。

#### 🟠 O-2 `pending` 机制有洞

`pending` 原先**容忍任何不一致**——包括「Spike 报 PASS、我们报 FAIL 3」这种真缺陷。**修法**：`pending` 只容忍「候选侧根本跑不了」（`ERROR` / `NOVERDICT`）；任何其他不一致 → **MISMATCH 并判负**。另：`pending` 一旦开始一致（NOW-PASSING）→ **判负**，强制把标记翻成 `active`，不让例外活得比它的理由更久；`pending` 行**必须**带理由 + 台账号（脚本校验，缺则报错退出）。

#### 🟡 Y-1 ~ Y-5

| # | 问题 | 处置 |
| --- | --- | --- |
| 🟡 Y-1 | **`CHECK_XLEN` 定义了却从不调用** → 文档说谎 | ✅ 调用它（与上游一致：上游在 CSR 初始化后调它），并补注释说明「定义即调用」 |
| 🟡 Y-2 | 两侧内存图不对称 + `gpr[2] = 0x80000000`（**恰是复位向量地址**） | ✅ 已由 `f9b86f3` / `cdbcffb` 闭环 |
| 🟡 Y-3 | `--list` 空选择 → macOS **bash 3.2** 下 `"${arr[@]}"` 空数组展开在 `set -u` 下直接致命退出 | ✅ 长度守卫上提到 `--list` 分支之前 |
| 🟡 Y-4 | `--suite` / `--filter` **缺参数被静默当作「不过滤」** → 意外跑全量 51 例 | ✅ 缺参数即报错退出 |
| 🟡 Y-5 | README 未说明其余 **18 个**上游 RV32 suite 为何不在语料内 → 读者无法区分「按设计排除」与「尚未考虑」 | ✅ README 补全「上游 suite → 在/不在 → 理由」全表 |

#### 🟢 G-1 ~ G-5（记录）

- **G-1** 参考侧 ISA 串的每个扩展都**必要**（`zicclsm` 实证：`ma_data` 有 zicclsm → exit 0，无 → 挂死 142）
- **G-2** 排除的 22 例理由站得住：被排除体内 **155 条** CSR/陷阱指令，在范围内的 51 例 **0 条**；且上游非对齐用例兼容「工作或抛正确异常」，未来加回来不会与 `zicclsm` 口径冲突
- **G-3** `sw rs2, symbol, base` 的语义实证与注释一致（`auipc` + `sw`，地址是符号的绝对地址）；`auipc`+`pcrel_lo` 的 12 位立即数**不构成距离限制**
- **G-4** 参考侧判决读自**8 位截断**的进程退出码（testnum ≥ 256 时会被读成 PASS）——当前语料最大 testnum ≈ 38 不触发；一旦触发是**响亮地 MISMATCH** 而非静默放过
- **G-5** `exec` 失败时 perl 返回 0（会被读成 PASS）→ 改 `exec(@ARGV) or die`

#### V-B 的等价性证明（本轮最扎实的一块）

| 环节 | 上游 `env/p/riscv_test.h` | 本仓库 `tests/golden/env/riscv_test.h` | 等价？ |
| --- | --- | --- | --- |
| PASS 落盘 | trap → `sw TESTNUM,tohost,t5`（=1） | `sw gp,tohost,t5`（=1） | ✅ 同 |
| FAIL 编码 | `beqz; sll; or` → `(n<<1)\|1` | 逐字相同 | ✅ 同 |
| 高字清零 | `sw zero, tohost+4, t5` | 同 | ✅ 同 |
| `tohost` 实测地址 | `0x80001000` | `0x80001000` | ✅ 同 |
| `link.ld` | — | 与上游**逐字节相同** | ✅ 同 |

**动态实证**：同一 `rv32ui/add.S`，分别用上游 env / 本仓库 env 编译，同一 Spike 跑，testnum 3 / 5 / 6 / 7 的退出码**两侧逐例相同**。**单射性论证**：`v ↦ v>>1` 在 bit0=1 的 64 位集合上单射 ⇒「两侧退出码相等」⇒「两侧 64 位 `tohost` 完全相等」。

**V-B 额外查出的三处差异（不影响等价性，但此前无人写下来）**：

1. **执行路径不同**：上游走 ECALL → 陷阱 → trap vector → `sw`；本仓库直写 `tohost`（从不编程 `mtvec`）。**判决值等价 ≠ 执行路径等价**——README 已补这句。
2. **特权级不同**：上游 `csrwi mstatus,0` + `mret` → **U 模式**；本仓库停在 **M 模式**。语料 51 例 **0 条** CSR/特权指令 ⇒ 不改结果；且两侧跑同一 ELF ⇒ 对比自洽。README 已补。
3. **`sp` 从未被真正使用**：上游宏 `TEST_LD_OP` / `TEST_ST_OP` 里的 `la x2, base` **把 `sp` 劫持成数据指针**——语料里 511 处 `off(sp)` 访问全部落在镜像内的数据区，不是栈。这解释了 Y-2 为何「当前不触发」。

#### V-B 的语料完整性核对

`diff <(corpus.txt 的 suite/test) <(上游 isa/rv32ui|rv32um|rv32uc 的 *.S)` → **无输出**。51 例逐名一致，**既无该进未进，也无不该进却进**；`active` / `pending` 划分与实现状态完全吻合（42 例实跑全 PASS，9 例在第一条非 RV32I 指令上报 `IllegalInstruction`）。

---

## §3.4 第 1 阶段检视轮（2026-10-20 · WASM 导出层 + 前端接真实内核）

> **范围**：`git diff 3500465..5a638ef`（7 个 commit，28 文件）——第 1 阶段的全部改动。
> **检视员**：V-A（Rust 内核侧）/ V-B（前端与桥）/ V-C（跨模块与文档一致性），**三路独立**，
> 各自在隔离 worktree（`/tmp/rv-a` / `rv-b` / `rv-c`）里作业，检视结束时工作区均**干净**。
> **判决**：V-A **FAIL**（2 高危 + 6 中危）· V-B **PASS**（5 中危 + 2 低危）· V-C **FAIL**（2 高危 + 2 中危 + 2 低危）。
> **闭环**：**13 项发现全部在本分支闭环**（10 项直接修，3 项登记为 **D-47 / D-48 / D-49**），
> **13 项反向变异自检全部变红**。

### 3.4.1 三路检视的范围划分

| 检视员 | 范围 | 判决 | 抓到的东西 |
| --- | --- | --- | --- |
| **V-A** | `crates/kernel/src/{m08,m09,m12,m13,m14,m17,kernel_host,wasm_api}` + `Cargo.toml` + `lib.rs` | **FAIL** | 2 个**真 bug**（观察点假停机 / 取指误触发读观察点）+ 6 处**测试判别力盲区**（16 处变异里 6 处零红） |
| **V-B** | `front/**` + `tools/{wasm-smoke,browser-verify}.mjs` | **PASS** | 2 处**护栏缺失**（空数组守卫零覆盖 / wasm-smoke 固件字节是副本）+ 3 处口径问题 |
| **V-C** | 三方契约（Rust 契约 / JS 镜像 / 桥规格）+ `AGENTS.md` / `REVIEWS.md` / `PROGRESS.md` | **FAIL** | 冻结规格 §3.1 与实现**及自身 §3.3** 矛盾；`E_PROTO-*` 不在契约族里；文档数字失真 |

### 3.4.2 V-A · 两条高危（**真 bug**，已在真 WASM 产物上复现）

#### 🟠-1 调试器写内存污染观察点命中槽 → 下一条 guest 指令误报停机

- **位置**：`m17_backend/mod.rs`（命中槽消费点）+ `kernel_host/mod.rs`（`WriteMemory` 命令走 `bus.write`）
- **现象**：设写观察点（0x200）→ 调试器写一次 0x200 → 再发 `StepN{1}`。被执行的指令是
  `addi x1,x0,42`（pc 0x00→0x04），**根本没碰 0x200**，内核却回报 `Halted{Watchpoint, pc:4}`。
- **根因**：命中槽只在 `reset()` / `clear_debug_state()` 清空，只在 step 批次**末尾**消费。
  调试器发起的 `bus.write` 触发同一条 hook 把槽填上，那一批没有 step 去消费它，残留到
  **下一批**被误当成本批的命中。
- **处置**：✅ **已闭环**——`step_n` **批次开头**清命中槽（命中槽只应承载**本批次内**的命中）。
  这一处同时修掉「Trap 分支 break 时残留」的第二条路径。
- **回归**：`m17_backend::tests::a_debugger_write_does_not_leak_into_the_next_step_batch`
  （**反向验证**：去掉那一行 → 该用例变红 ✅）
- ⚠️ **同源缺陷的另一半**：镜像装载路径早就在 `m14_bus::load_bytes` 的文档里写明
  「装载镜像前清掉观察点，镜像装载不能触发观察点」——**`WriteMemory` 命令路径被漏掉了**。

#### 🟠-2 只读观察点被「取指」误触发——代码地址上的读观察点必假命中

- **位置**：`m17_backend/mod.rs`（hook 不区分取指与数据读）+ `m13_iss/mod.rs`（取指走 `bus.read(state.pc)`）
- **现象**：把一个 `access=Read` 的观察点挂在**代码地址** 0x00 上，执行任意一条指令即回报
  `Halted{Watchpoint}`——触发它的是**取指**，不是数据读。
- **根因**：取指经 `Bus::read` 走总线，会 `fire_hook(dir=Read)`；hook 只看「方向 + 地址区间」，
  无法区分「取指」与「数据读」。
- **处置**：✅ **已闭环**——`BusAccess` 增 `is_fetch` 标记；`Bus::read_fetch()` 专供取指；
  M-13 的取指改走它；M-17 的观察点 hook **跳过取指**。
  ⚠️ 附带对照用例证明**没有把修复做成「读观察点整个失灵」**：
  `a_data_read_still_fires_a_read_watchpoint`（guest 的一次 `lw` 仍然命中读观察点）。
- **回归**：2 条（**反向验证**：去掉 `is_fetch` 判断 → 前者变红；让所有访问都不匹配 → 后者变红 ✅）

### 3.4.3 V-A · 六条中危（**测试判别力盲区**——改实现没有任何测试变红）

| # | 盲区 | 变异结果 | 处置（补的用例） |
| --- | --- | --- | --- |
| 🟡-1 | 命令层「ReadMemory 不产生副作用」无守护 | 0 红 | `kernel_host::tests::read_memory_command_has_no_side_effect_on_the_bus` + `a_debugger_read_does_not_fire_a_read_watchpoint` |
| 🟡-2 | 指令计数口径（陷入算不算「执行了」）无守护；`total_executed` **零引用** | 0 红 | `m17_backend::tests::the_execution_count_pins_down_what_counts_as_executed` + `total_executed_counts_every_executed_instruction` |
| 🟡-3 | `continue_until_event` 的「时间驱动、不执行 guest 指令」无回归防护 | 0 红 | `m17_backend::tests::continue_until_event_does_not_execute_guest_instructions` |
| 🟡-4 | 规格 §3.1「事件序列（有序）」无顺序断言 | 0 红 | `kernel_host::tests::a_breakpoint_run_emits_tick_then_waveform_then_halted` |
| 🟡-5 | `load_bytes` 的「镜像越界必须报 E_BUS-01」无守护 | 0 红 | `kernel_host::tests::an_image_that_runs_past_the_mapped_region_is_rejected` |
| 🟡-6 | `Restore` 的「状态不合法 → Error」拒绝路径**未实现**（无条件返回 `Tick`） | — | ✅ **实现 + 用例**：`kernel_host::tests::restore_rejects_a_state_with_a_foreign_schema_version`（错误码复用契约已有的 `E_SNAP-05`——一场景一码） |

> **这六条的共性**：不是实现写错了，是**规格里写着的约束没有用例在守**。
> 变异自检的口径因此从「改动会不会被现有测试抓住」扩展到「**规格里的每一条明文要求，
> 都得有一条能变红的用例**」。

### 3.4.4 V-A · 八条低危（处置）

| # | 内容 | 处置 |
| --- | --- | --- |
| ⚪-1 | 规格 §3.1 `LoadProject` 行写「已订阅 → WaveformDelta（全量）」，但 §3.2b 规定 LoadProject **清空**订阅 → 该行**永不可达** | ✅ **改规格**（V-C 同源，见 3.4.6） |
| ⚪-2 | `Snapshot` 只发一个自增 id，不存任何快照 → 事件名在「说谎」 | ✅ **改规格**（新增 §3.7，明说「只分配 id，不落盘」） |
| ⚪-3 | `load_bytes` 的 `i as u32` 对 >4 GiB 的 `usize` 会**截断** | ✅ **已修**——改 `u32::try_from(i)` |
| ⚪-4 | `m12_engine:369` 的 `.expect("peek said some")` 是桥路径上唯一的 panic 点 | ✅ **已修**——改 `ok_or_else` 返回 `E_ENG-02` |
| ⚪-5 | 取指失败时 `Continue` 产出 **4 条**事件（含 `Error`），规格表里没有 Error 行 | ✅ **改规格**（§3.1 表 + 补一段「取指失败 → Error」的完整序列） |
| ⚪-6 | 信封版本只拒「更低」，更高的被接受 → 未来版前端连旧内核会被**静默误解析** | ✅ **已修**——改 `!=`（契约 serde 没有 `deny_unknown_fields`，少解析字段是静默的） |
| ⚪-7 | `reset()` 不保留 ELF 入口点（后端不记 entry） | ⏳ **留账**（潜伏项，宿主流程不触发） |
| ⚪-8 | mock 内核把 `StepN.n` 夹到 1024，真内核不夹 | ⏳ **留账**（mock 与真内核的行为差异，P1 统一） |

### 3.4.5 V-B · 前端与桥（判决 PASS，5 中危 + 2 低危）

| # | 内容 | 处置 |
| --- | --- | --- |
| 🟡-1 | 传输帧 `seq` 有**两个序列空间**（内核 `msg.seq` vs 宿主 `hostSeq`），帧 seq 可从 11 跳回 1 | ⏳ **登记为 D-47**（P1 断线续拉前必须拆开；今天无活影响——M-05 用契约 `Message.seq`） |
| 🟡-2 | `wasm-smoke.mjs` 的 demo 固件字节是**手工副本**，与前端那份零交叉校验（实测：改前端那份 → smoke 仍绿） | ✅ **已修**——改为 `import { DEMO_FIRMWARE_BYTES } from "../front/dev/demo-firmware.js"` |
| 🟡-3 | `validateEventList` 的「空数组」守卫**零覆盖**（变异实测：删掉后全绿） | ✅ **已修**——假模块加 `?mode=empty` + 一条断言 `E_TRX-04` 的用例 |
| 🟡-4 | 产物缺失时两个工具的退出码语义冲突（0 vs 127） | ✅ **已修**——`wasm-smoke` 改退 **127**（前置条件缺失 ≠ 验证通过） |
| 🟡-5 | WASM 初始化失败时同一故障被报**两次**、文案不同 | ✅ **已修**——有命令在飞时只由命令帧承担；文案统一为「WASM 内核初始化失败：…」；补 2 条用例 |
| ⚪-1 | `decodeFrame` 接受 8/24 种畸形帧（**既有代码**，不在本次 diff 内） | ⏳ 不阻塞（生产路径不可达；M-07 的 `encodeFrame` 永远产出合法帧） |
| ⚪-2 | `wasm-smoke` 的畸形输入断言过弱（只看数组长度） | ✅ **已修**——补 `direction == "Event"` 与 `seq` 合法性断言 |

> **V-B 的一条方法论结论**：**假绿的形态是「断言值退化成断言非空」**。
> 实测：把真机断言 `x1 == "0000002a"` 弱化成 `x1 != ""`，再把固件写错 →
> **全绿、退出码 0，检查名却仍写着 "x1 = 42"**。
> 所以真机断言必须断言**值**，这条现在写进了 AGENTS.md 的判据。

### 3.4.6 V-C · 规格与文档（判决 FAIL，2 高危 + 2 中危 + 2 低危）

#### 🟠-1 冻结规格 §3.1 与实现**及自身 §3.3** 互相矛盾

三处不一致：

1. §3.1 `LoadProject` 行写「已订阅 → `WaveformDelta`（全量）」，但实现先清空主题集合，
   该分支**永不可达**；
2. §3.1 `Subscribe` 行写「`sim.waveform` → 探针流」，而**同一份文档 §3.3** 明说
   「第 1 阶段不接探针管道，`sim.waveform` 订阅下不产生探针流」；
3. §3.1 `Continue` 行同样列了不成立的 `sim.waveform` 分支。

- **处置**：✅ **改规格，不改实现**——实现的语义是 D-45 的直接前提，改它会打破已闭环的 D-45。
  §3.1 表已逐行订正，并加了一句「**本表是实现与文档的共同权威**，任何一方与本表不一致时，
  先改本表，再改代码」。
- **附带**：新增 §3.1 下的「取指失败 → Error」完整序列说明（4 条事件，错误在最前）。

#### 🟠-2 `E_PROTO-08` / `E_PROTO-01` 在契约的 `ErrorFamily` 枚举里**不存在**

契约的 `ErrorFamily` 只有 13 个族，**没有 `PROTO`**；而桥规格 §2.1 明文规定畸形输入要变成
`E_PROTO-08`，实现侧（Rust + JS 镜像）都硬编码这个字面量。

- **处置**：✅ **改规格**——契约层冻结，**不新增** `ErrorFamily::Proto`（那是契约变更，
  需另起流程并解冻 `error.rs`）。§2.1 补一段限定：「`E_PROTO-08` / `E_PROTO-01` 是
  **前端协议层**的码，不属于契约的任何一个族；契约的 `EventPayload::Error.code` 是 `String`，
  没有族约束」。
- ⏳ **登记为 D-49**（连带 `port.rs` / `model.rs` 注释里的 `E_PROTO-05` 同样查无此族）。

| # | 内容 | 处置 |
| --- | --- | --- |
| 🟡-3 | `Snapshot` 只递增计数器，`SnapshotCreated{id}` 的 id **无对应实体** | ✅ **改规格**——新增 §3.7 明说「第 1 阶段只分配 id，不落盘」，并指向 D-37 |
| 🟡-4 | `E_TRX-04` 一个码三种语义；且**桥规格里根本没有这个码** | ✅ **改规格**——§4.1 补「宿主侧失败 → `Error{E_TRX-04}`」表；⏳ 同码多义登记为 **D-48** |
| ⚪-5 | 契约层行数在 §0.1 表里写错（1 851，实测 **1 877**）——**基线遗留**，但本轮重算了同表其它行却漏了它 | ✅ **已改**（1 877） |
| ⚪-6 | `app.js` 的「reset」与「startSession」各自独立调 `buildMockProject()` | ⏳ 不阻塞（D-45 的核心不变量已由服务兜住） |

### 3.4.7 闭环对照表

| # | 发现 | 检视员 | 严重度 | 处置 | 回归用例 | 反向变异 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 调试器写污染观察点命中槽 → 假停机 | V-A | 🟠 | 修实现 | `a_debugger_write_does_not_leak_into_the_next_step_batch` | ✅ 变红 |
| 2 | 取指误触发读观察点 | V-A | 🟠 | 修实现 | `an_instruction_fetch_does_not_fire_a_read_watchpoint` + 对照 `a_data_read_still_fires_a_read_watchpoint` | ✅ 变红 ×2 |
| 3 | 命令层 ReadMemory 副作用无守护 | V-A | 🟡 | 补用例 | `read_memory_command_has_no_side_effect_on_the_bus` + `a_debugger_read_does_not_fire_a_read_watchpoint` | ✅ 变红 |
| 4 | 指令计数口径无守护 | V-A | 🟡 | 补用例 | `the_execution_count_pins_down_what_counts_as_executed` | ✅ 变红 |
| 5 | `continue_until_event` 语义无防护 | V-A | 🟡 | 补用例 | `continue_until_event_does_not_execute_guest_instructions` | ✅ 变红 |
| 6 | 事件顺序无断言 | V-A | 🟡 | 补用例 | `a_breakpoint_run_emits_tick_then_waveform_then_halted` | ✅ 变红 |
| 7 | `load_bytes` 越界无守护 | V-A | 🟡 | 补用例 | `an_image_that_runs_past_the_mapped_region_is_rejected` | ✅ 变红 |
| 8 | `Restore` 不校验 schema 版本 | V-A | 🟡 | 修实现 + 用例 | `restore_rejects_a_state_with_a_foreign_schema_version` | ✅ 变红 |
| 9 | 信封版本只拒「更低」 | V-A | ⚪ | 修实现 + 用例 | `an_envelope_from_a_newer_schema_is_refused` | ✅ 变红 |
| 10 | `load_bytes` 的 `i as u32` 截断 | V-A | ⚪ | 修实现 | （同上用例覆盖） | — |
| 11 | `m12_engine` 的 `expect` panic 点 | V-A | ⚪ | 修实现 | — | — |
| 12 | wasm-smoke 固件字节是副本 | V-B | 🟡 | 修实现 | `wasm-smoke` 的 x1=42 / 读回 49 断言 | ✅ 变红 |
| 13 | 空数组守卫零覆盖 | V-B | 🟡 | 补用例 | `内核返回空数组 → 回一个 E_TRX-04 的 Error 事件` | ✅ 变红 |
| 14 | wasm-smoke 退出码 0 vs 127 | V-B | 🟡 | 修实现 | （实测：移走产物 → 退出 127） | — |
| 15 | init 失败双报 + 文案不一 | V-B | 🟡 | 修实现 + 用例 | `初始化失败时有命令在飞 → 只报一次` + `同一个初始化故障在所有帧里只有一种文案` | ✅ 变红 |
| 16 | wasm-smoke 畸形断言过弱 | V-B | ⚪ | 修实现 | 补 `direction` / `seq` 断言 | — |
| 17 | 帧 seq 双序列空间 | V-B | 🟡 | 登记 **D-47** | — | — |
| 18 | 规格 §3.1 与实现 / 自身矛盾 | V-C | 🟠 | 改规格 | — | — |
| 19 | `E_PROTO-*` 不在契约族里 | V-C | 🟠 | 改规格 + 登记 **D-49** | — | — |
| 20 | `Snapshot` id 无实体 | V-C | 🟡 | 改规格 | — | — |
| 21 | `E_TRX-04` 同码多义 + 规格漏写 | V-C | 🟡 | 改规格 + 登记 **D-48** | — | — |
| 22 | 契约层行数失真 | V-C | ⚪ | 改文档 | — | — |

### 3.4.8 本轮方法论沉淀

1. **「规格里写着的约束」必须有一条能变红的用例**。V-A 的 6 条中危里，5 条不是实现写错，
   而是**规格的明文要求没有任何用例在守**——改实现不会变红，所以缺陷能潜伏很久。
   → 检视清单里要加一维：**「逐条读规格的明文要求，核对有没有对应的能变红的用例」**。
2. **真机断言必须断言值**，而且要**反向证明它有判别力**。V-B 实测：把
   `x1 == "0000002a"` 弱化成 `x1 != ""` + 固件写错 → 全绿、退出码 0、检查名还写着 "x1 = 42"。
3. **「护栏」也要被检视**。V-B 抓到的两处护栏缺失（空数组守卫零覆盖、wasm-smoke 的字节副本）
   都不是功能缺陷，但都让「未来的破坏」查不出来。
4. **规格是共同权威，不是实现注释**。V-C 抓到规格 §3.1 与实现**及自身 §3.3** 三处矛盾。
   处置顺序必须是**先改规格**——实现的语义往往被已闭环的不变量依赖（比如 D-45）。
5. **同一个数字要能被第三方独立复算**。V-C 实测契约层 1 877 行 vs 文档 1 851 行，
   且指出这是**基线遗留**——本轮重算了同表其它行却漏了它。口径一旦失真，整张表就不可信。
6. ⚠️ **变异验证的「还原」必须同时刷新 mtime**（本轮真踩过）。用
   `shutil.copy2` 还原文件会**保留备份的旧 mtime**，而 cargo 用 mtime 判断要不要重编
   ——文件内容明明是对的，cargo 却认为「没变」而**跳过重编**，于是门禁跑的是**变异前的
   旧二进制**。症状是：还原之后本该变绿的用例**仍然变红**。本轮确实撞上了——
   `⚪-6`（信封版本只拒「更低」）的用例在还原后仍红，一度以为是修复没生效。
   还原一律用 `copyfile` + `os.utime(path, None)`，**或者**还原后强制 `touch`。
   这条对任何「备份—变异—还原—重跑」的工具都适用，不只是本轮这一处。


---

## §3.5 D-46 检视轮（2026-10-21 · 探针管道接线）

> **分支**：`feat/d46-probe-pipeline`（基线 `main` = `16520b2`）
> **范围**：D-46 的 5 个 commit（`03d9768` / `0e439d3` / `a6ce0c2` / `cbce7b5` / `d0d0b66`）+ 检视轮修复
> **判决**：**V-A FAIL**（1 中危 + 8 低危）· **V-C FAIL**（3 中危 + 7 低危）· V-B 见 §3.5.4

### 3.5.1 三路检视的范围划分

| 检视员 | 范围 | 判决 |
| --- | --- | --- |
| **V-A** | Rust 内核：M-14 / M-17 / M-18 / M-19 / 内核宿主 | **FAIL**（1 🟠 + 8 🟡） |
| **V-B** | 前端 + 工具（`front/` / `tools/`） | 见 §3.5.4 |
| **V-C** | 跨模块一致性 + 文档（桥规格 / 台账 / AGENTS.md / PROGRESS.md） | **FAIL**（3 🟠 + 7 🟡） |

三路检视员**各自独立开一份 worktree**（`/tmp/rv-d46-a` / `-b` / `-c`），detached HEAD 于 `cbce7b5`，
**严禁** `git reset` / `git checkout --` / `git restore` / `git clean` / `git push` / `git commit`——
检视员**只读**，所有修改由协调者在主工作区做。

### 3.5.2 V-A 报告（Rust 内核）—— 1 中危 + 8 低危

#### 🟠-1 发布源**无上限**，且 M-19 在生产路径上是**死代码**

**这是本轮最重要的一条发现。** V-A 实测：

- 不订阅波形时，M-18 的 `edges: BTreeMap<u8, Vec<Edge>>` **只进不出**——
  堆到 **300 000 条**边沿不释放；
- 订阅的那一刻一次性推出去：单条事件 **8 MB**（8 000 条 ≈ 219 KB）——
  WASM 的 `postMessage` 与前端的 JSON 解析同时被压垮；
- **反向变异**：把 M-18 → M-19 的 `set_edge_sink` 握手删掉，
  **D-46 的全部 10 条用例照样全绿**——证明 M-19 的环**只写不读**，
  它在生产路径上是**死代码**（纯内存开销）。

**处置（分两层，各管一段）**：

| 层 | 位置 | 机制 | 可见性 |
| --- | --- | --- | --- |
| **内存层** | M-18 `ProbeRegistry` | `EDGE_STORE_CAP = 8192` / 通道，**drop-oldest**（成批淘汰，摊还 O(1)） | `dropped_edge_count()` 计数器 |
| **事件层** | 内核宿主 `waveform_event` | `MAX_WAVEFORM_EDGES_PER_EVENT = 4096` / 事件，**保留最新** | `waveform_edges_truncated()` 计数器 |

外加**让 M-19 活过来**的判别力用例：`the_m19_ring_actually_receives_the_edges`
直接断言 **M-19 的环里有边沿**——删掉握手就变红。

⚠️ **为什么「两层上限」而不是一层**：M-18 的上限管**内存**（每通道存多少），
宿主的上限管**单条事件多大**（前端能扛多少）。一层的话，要么 M-18 存太多
（单条事件还是太大），要么 M-18 存太少（历史丢得太早）。

⚠️ **教训**：「用例都在」≠「用得着」。V-A 删掉握手后 10 条用例全绿，
说明**用例覆盖的是「边沿出现」，不是「M-19 真的在用」**。

#### 🟡-2 ~ 🟡-9（低危，逐条闭环）

| 编号 | 发现 | 处置 |
| --- | --- | --- |
| 🟡-2 | `KernelHost::new()` **漏装**外设探针回调 → 刚建好、还没 `LoadProject` 的宿主**观察不到任何边沿** | ✅ 已修（`d0d0b66`）+ 用例 `a_fresh_host_already_has_the_probe_pipeline_wired` |
| 🟡-3 | M-14 措辞说「并进 `pending_writes`」——但 `pending_writes` 在生产代码里**只写不读** | ✅ 措辞收窄（「提交路径待 D-07 落地」）→ ✅ **2026-10-21：D-07 已落地，提交路径已通**（M-17 执行器把队列抽干并真正写进总线） |
| 🟡-4 | `tick_peripherals` 遇**第一个**报错外设即整体 `?` 中断 → 前面已收集的写全丢，后面的外设**错过时钟同步** | ✅ 改成 `PeripheralTickOutcome{writes, error, skipped}`：**继续 tick 后面的**，报**第一个**错，写**一条不丢**；用例 `a_failing_peripheral_does_not_stop_the_others_from_ticking` |
| 🟡-5 | **判别力缺口**：`add_hook` **一条用例都没有**（M-14 的测试里三处全是 `set_hook`）→ M-17 改回 `set_hook` 全绿 | ✅ 补 3 条 M-14 用例 + 1 条 **M-17** 用例（`installing_the_watch_hook_does_not_evict_an_existing_hook`，**与装配顺序无关**） |
| 🟡-6 | 管道装配错误被 `let _ =` **静默丢弃**（`define_channel` / `attach_signal`） | ✅ 改为**计数**（`probe_wiring_failures()`）。⚠️ 失败路径**仍无用例** → 台账 **D-52** |
| 🟡-7 | `attach_peripheral_probes` 的 `try_borrow_mut` 失败**静默跳过** + 运行期**无重试** | ✅ 计数 + **运行期重试**（每轮 `Run` 前重试一次；`attach_signal` 幂等） |
| 🟡-8 | 调试器 `WriteMemory` 产生的 GPIO 边沿**晚一轮**才推 | ✅ `WriteMemory` 后**当场**推波形 |
| 🟡-9 | M-18 的**访存 trace 环**在宿主里**从不被 drain** → 常驻 `TRACE_CAP = 200 000` 条（3~5 MB） | ⏳ 台账 **D-51**（M-11 落地前没有消费者，**不能**简单定期 drain 掉） |

### 3.5.3 V-C 报告（一致性与文档）—— 3 中危 + 7 低危

| 编号 | 发现 | 处置 |
| --- | --- | --- |
| 🟠-1 | 桥规格 **§6 与 §3.3 自相矛盾**（§6 说「不推边沿流」，§3.3 说推） | ✅ **改规格**（§6 那条划掉 + 标注「v0.14 已闭环」） |
| 🟠-2 | 内核宿主**少接一根线**（`new()` 没装外设回调） | ✅ 已修（`d0d0b66`）——与 V-A 🟡-2 是同一个洞，V-A 独立发现 |
| 🟠-3 | 内核宿主行数 **1 517 → 1 542**（文档数字失真） | ✅ 订正（本轮复算为 **1 677**） |
| 🟡-1 | 探针流断言 **+4 → +5** | ✅ 订正 |
| 🟡-2 | §3.1 没写「**无新边沿时不推**」 | ✅ 补 |
| 🟡-3 | **M-19 只写不读**，链路图画得不准 | ✅ 链路图改准 + 台账 **D-50** |
| 🟡-4 | §5 变更索引未登记本轮 commit + 状态核实日期未更新 | ✅ 补 |
| 🟡-5 | **3 处 U+FFFD 编码损坏字符** | ✅ 修（本轮 2 处 + 既有 1 处）。⚠️ `docs/detailed-design/M-02_ViewComponents.html:449` 的 `�` 经查证是**有意**的示意图（`[0xFF 0xFE]` 后跟替换符，演示 NFR-04），**不动** |
| 🟡-6 | 散文「合计未闭环 **29**」→ 当时实际 **32**（D-46 检视轮再登记 3 条后为 **35**） | ✅ 订正 |
| 🟡-7 | M-18 **2 254 → 2 255** | ✅ 订正（本轮复算为 **2 319**） |

### 3.5.4 V-B 报告（前端与工具）

**判决：FAIL**（4 中危 + 0 高危）。V-B 的范围是 `front/` 与 `tools/`（前端接线、桥工具、真机复验脚本）。

它跑了这些门禁（全部在自己的 worktree 里）：

| 命令 | 结果 |
| --- | --- |
| `tools/check-front.sh` | 152 用例 / 28 suite，**全绿**（5 次重跑 + CPU 满载 20 次，0 fail） |
| `cargo test --workspace --no-fail-fast` | **全绿** |
| `tools/build-wasm.sh` | 构建成功 |
| `node tools/wasm-smoke.mjs` | **24 项全绿**（对**真** wasm 产物） |
| `node tools/browser-verify.mjs` | **真 Chrome 155 + 真 WASM 全绿** |
| `git status --short` | 空（探针文件已删） |

#### 🟠-1 「装载工程后探针流被强制重订」这一 D-46 核心不变量**没有用例在守**

`loadProjectIntoKernel()` 的第三步 `await waveform.subscribeProbeStream()` 是 D-46 的核心交付物（`LoadProject` 会清空内核侧主题集合），但**默认门禁 152 条用例里没有一条覆盖它**——`waveform.test.mjs` 只单独测 `wireWaveform()` 返回对象，**从不经过 `loadProjectIntoKernel`**；而它本身未 export，无测试可及。

**证据（反向变异——「仍绿」是本条的核心证据）**：

```
删掉 loadProjectIntoKernel 的探针重订
  → # tests 152 / # pass 152 / # fail 0      ← 仍然全绿！
  → 但 tools/browser-verify.mjs 抓到：2 项断言红（channels=0 / item0=""）
```

⚠️ 而 **browser-verify 明确不进默认门禁**（CI 的 `front` job 只跑 `tools/check-front.sh`），它需要本机 Chrome + 已构建的 WASM 产物 → **CI 上这条路径完全无守**。

**处置**：导出 `loadProjectIntoKernel` + 补 2 条用例（三步的**顺序**都做了 + 重订必须**真的发一条** `Subscribe{sim.waveform}`）。
**判别力**：删掉第 3 步 → **2 条用例变红**（此前 0 条）。

#### 🟠-2 共享单例（M-03 视图状态）+ 异步 bootstrap 的**条件性**竞态

`app.js` 模块作用域调 `bootstrap()`，其异步链最终调 `loadProjectIntoKernel()` → `waveform.reset()` → **`viewStore.setState("waveform", …)`**。而 M-03 是**模块级单例**——测试文件的 store 与 app.js 自己的 bootstrap **共用同一个 store**，app.js 的 `reset()` 可能在任意时刻把测试刚推入的通道清空。

**证据（探针，已删）**：

```
[t=15] after push: n=1
[t=17] after 0ms : n=0      ← app.js 自己的 bootstrap reset() 落下来了
```

⚠️ **V-B 明确标注「结论未完整」**：它**没有**证明当前 152 条用例**已经**因此翻红——查到的边界是：现有 9 条用例**都是同步的**（`router.emit` 后立即断言，中间无 `await`），reset 抢不到窗口 → **当前未观察到 flake**；但把 `setup()` 里的 `reset()` 删掉会让 **2 条变红**，证明该文件**确实依赖 `reset()` 提供隔离**。

⚠️ **协调者的口径订正**（与 V-B 的实验一致）：这条**不是**「当前套件在飘」，而是「**任何人往 emit 与 assert 之间加一个 `await`，它就会开始飘**」——而「断言必须同步」这个不变量**没写在任何地方**。V-B 的两个对照实验（CPU 满载连跑 20 遍 + 人为延后 25 ms）都全绿，**证明「时序恰好对」这个说法当时也不准确**。

**处置**：加 `globalThis.__RVSIM_NO_BOOTSTRAP__` 开关——不是为了「修一个正在飘的 bug」，是为了**把隐式不变量变成显式的**。
**判别力**：这条的判据**不是**「用例变红」（缺陷是**条件性**的），而是**探针**：同一脚本连跑 N 遍，store 里的通道数必须**始终**是 1。

#### 🟠-3 三步串联：第 3 步失败会**连带废掉前两步**

三步 `await` 串联，任一步 reject 则整体 reject。`registerMemory.loadProject()` **已成功**（LoadProject 已下发、寄存器流已重订），但 `subscribeProbeStream()` 超时（`E_CTX-06`）会让整个函数抛错 → `startSession()` 的 catch 把整个会话标成「内核未就绪」，**`Command.nop()` 往返不再执行**，footer 停在错误文案；`handleCommand("reset")` 同理。**前两步的成果被丢弃，且状态不一致**。

**证据（探针，已删）**：

```
PROBE-FAIL register load done, commands: [{"LoadProject":…},{"Subscribe":{"topic":"sim.registers"}}]
PROBE-FAIL probe subscribe rejected: E_CTX-06
PROBE-FAIL register stream still cached? x1 = 0   subscribed flag alive: 1
```

**处置**：第 3 步**单独**兜住，失败作为 `probeError` **返回**（**不抛**）；调用点区分「内核未就绪」与「内核已就绪，但探针流不可用」。
**判别力**：去掉 try/catch → **2 条用例变红**。

#### 🟠-4 探针流没有「断线重连后重订」——与寄存器流**不对称**

M-10 明确实现了「断线重连后重新订阅寄存器」（`onStatus` 回调 + `ensureSubscribed()`），并有回归用例。**新增的探针流没有对应处理**：`wireWaveform()` 不监听 transport 状态。链路一断一连，内核侧 `sim.waveform` 主题就没了 → 波形视图**静默停掉**（不报错、边沿数冻结）。

**证据（探针，已删）**：

```
PROBE total Subscribe commands sent: ["sim.registers","sim.waveform","sim.registers"]
PROBE commands sent after reconnect: [{"Subscribe":{"topic":"sim.registers"}}]   ← 探针流没有重订
```

⏳ 登记为台账 **D-54**（今天没有活影响——两种传输都**不自动重连**；但代码库明确建模了重连，P1 做自动重连时它会立刻变成活缺陷）。

#### ✅ V-B 独立查证**没有问题**的部分

这些是**正面结论**，同样有判别力证据：

| 结论 | V-B 的证据 |
| --- | --- |
| **过滤器判别方向**（「不是 `cpu.` 开头」）正确 | 改成 `probe.` 前缀 → 前端门禁 **4 红**、browser-verify **2 红** |
| **边沿累加语义**正确（内核推的**确实是增量**） | 改成覆盖写 → 前端门禁「边沿是**累加**的」**红** |
| **demo 固件编码**正确 | V-B **独立逐条解码** 40 字节（非照抄期望值）：`lui x5,0x40000`→x5=0x4000_0000、`addi x6,x0,1`、`sw x6,4(x5)`（S 型立即数解回 4）、`jal x0,0x14`（J 型半字偏移 +4）**全部正确** |
| **XSS 防护**正确 | 注入 `<script>alert('X')</script>` → 输出 `&lt;script&gt;…`，无原始 `<script>` |
| **wasm-smoke 探针块的位置**（在「断点/Continue」**之前**）是**必要**的 | 把两块**对调** → **4 红**（`Continue` 会一路跑到断点靶点 0x20，途中已写过 GPIO，边沿被 drain 掉） |
| **`step10` 步数**够（1+10=11 条，第 8 条写 GPIO） | 0x24 是 `jal x0,0` 自环，多跑只是原地打转，**无其他副作用** |
| **`loadProjectIntoKernel` 的顺序**正确（`reset()` 放**最前**） | 真机实测「复位」后通道**仍在**（0 edges），再 `step10` → 1 edge；**所有**装载路径都走这个函数，**无遗漏** |
| **`wireWaveform` 的 export** 不污染公开面 | 与 `describeTransport`/`renderStatus` 同风格（`app.js` 已有先例），`app.js` 是入口模块无 barrel re-export |

#### ⚠️ V-B 明确标注「未查完 / 待查」（不作结论）

1. **🟠-2 的影响面未量化完毕**——机制真实存在，但**未能证明**当前用例已因此翻红（与协调者的订正一致）。
2. **浏览器 Worker 崩溃 → 传输重挂的完整路径未走通**——只验证了「传输层 `setConnected(false) → true`」这一层。`m07_transport.js` 的 worker 适配器**没有**自动重建逻辑（看起来是终态）→ V-B-4 按「传输重连」这条可达路径定级。**并入 D-54**。
3. **M-19 的 CSV/VCD 导出路径与波形视图的一致性未查**——属 V-A / Rust 侧。**并入 D-50**。
4. **mock 内核的探针流语义与真内核**不等价**——mock 每次 `StepN` 都推**全量** 1 边沿，真内核是**增量** drain。当前 mock 只用于 `mock-kernel.test.mjs` 的隔离测试、**未接前端**，无活影响；但**口径差异未在文档中查证** → 登记 **D-55**。
5. **浏览器真机上「先 `step` 再 `step10`」的时序竞争**——两条命令连发（`Runtime.evaluate` 背靠背），**未加等待**；实测通过，但**未做重复采样** → 登记 **D-56**。

#### V-B 的变异实验总表（全部实测，已还原）

| # | 变异 | 前端门禁 | browser-verify | wasm-smoke |
| --- | --- | --- | --- | --- |
| 1 | 过滤器改回 `probe.` 前缀 | **4 红** | **2 红** | — |
| 2 | 删 `loadProjectIntoKernel` 的探针重订 | **152 仍全绿** ⚠️ | **2 红** | — |
| 3 | 删测试 `setup()` 里的 `reset()` | **2 红** | — | — |
| 4 | 边沿改成覆盖写 | **1 红** | **仍全绿** ⚠️ | — |
| 5 | 延迟 bootstrap 的 `reset()` | 9 仍全绿 | — | — |
| 6 | 前端硬编码假通道 | **2 红** | **仍全绿** ⚠️ | 绿（查内核侧） |
| 7 | 过滤器整个删掉 | **2 红** | — | — |
| — | wasm-smoke 探针块与断点块对调 | — | — | **4 红** |

⚠️ = 该路径**没有用例在守**（绿 = 漏网）。

⚠️ **变异 4 与 6 的结论**：**browser-verify 单独看**对「累加 vs 覆盖」「真数据 vs 假数据」**没有判别力**——它是**连线检查**（数据有没有从内核走到 DOM），**不是**行为检查。是**前端门禁**兜住了这一层。所以「browser-verify 通过」**不能**单独作为「波形视图行为正确」的证据。


### 3.5.5 闭环对照表

| 编号 | 发现 | 严重度 | 处置 | 判别力证据 |
| --- | --- | --- | --- | --- |
| V-A-1 | 发布源无上限 + M-19 死代码 | 🟠 | ✅ 两层上限 + 活性用例 | 变异 **N2 / N3 / N4 / N5** 全红 |
| V-A-2 | `new()` 漏装回调 | 🟡 | ✅ `d0d0b66` | 变异 **M12** 红 |
| V-A-3 | `pending_writes` 措辞 | 🟡 | ✅ 收窄 | 文档项 |
| V-A-4 | 外设 tick 遇错即中断 | 🟡 | ✅ 继续 tick + 不丢写 | 变异 **N7** 红 |
| V-A-5 | `add_hook` 判别力缺口 | 🟡 | ✅ 3 + 1 条用例 | 变异 **N1 / M1** 红 |
| V-A-6 | 装配错误静默 | 🟡 | ◐ 计数 | ⚠️ **无判别力** → **D-52** |
| V-A-7 | attach 静默 + 无重试 | 🟡 | ✅ 计数 + 运行期重试 | ⚠️ 计数部分同上 |
| V-A-8 | `WriteMemory` 边沿晚一轮 | 🟡 | ✅ 当场推 | 变异 **N6** 红 |
| V-A-9 | trace 环从不被 drain | 🟡 | ⏳ **D-51** | — |
| V-C-1 | §6 与 §3.3 矛盾 | 🟠 | ✅ 改规格 | 文档项 |
| V-C-2 | `new()` 少接一根线 | 🟠 | ✅ `d0d0b66` | 变异 **M12** 红 |
| V-C-3 | 行数 1 517 | 🟠 | ✅ 订正 | 第三方可复算 |
| V-C-4~7 | 断言数 / §3.1 / 链路图 / 索引 | 🟡 | ✅ 逐条订正 | 文档项 |
| V-C-8 | 3 处 U+FFFD | 🟡 | ✅ 修（1 处有意，不动） | — |
| V-C-9~10 | 数字 29 / 2 254 | 🟡 | ✅ 订正 | 第三方可复算 |
| **V-B-1** | 「装载工程后探针流被强制重订」**没被守** | 🟠 | ✅ 导出 + 补 2 条用例 | 变异 **M11** 红（此前门禁 152/152 全绿） |
| **V-B-2** | 共享单例 + 异步 bootstrap 的**条件性**竞态 | 🟠 | ✅ 加 `__RVSIM_NO_BOOTSTRAP__` 开关 | 探针：连跑 N 遍通道数恒为 1（**不是**「用例变红」） |
| **V-B-3** | 三步串联，第 3 步失败**连带废掉前两步** | 🟠 | ✅ 第 3 步单独兜住（`probeError` 返回） | 去掉 try/catch → 2 条红 |
| **V-B-4** | 探针流无「断线重连后重订」 | 🟠 | ⏳ **D-54** | — |
| **V-B-5** | mock 内核的探针流语义与真内核**不等价**（mock 推全量，真内核推增量） | 🟡 | ⏳ **D-55** | — |
| **V-B-6** | browser-verify 的 `step` / `step10` 连发**未加等待**，未做重复采样 | 🟡 | ⏳ **D-56** | — |

### 3.5.6 反向变异自检（22 条）

> 脚本：`/tmp/d46_mutverify2.py`（**不入库**——一次性工具）
> 口径：`cargo test --workspace --no-fail-fast`（**默认 fail-fast 会截断红测条数**）
> ⚠️ **变异必须能编译**：脚本在跑测试前先做一次 `cargo build --workspace --tests`，
> 编译不过直接判 **BROKEN**（**编译错误导致的「红」不算判别力**——这是上一轮
> 自查踩过的坑：M1 之所以变红是因为括号不配对，不是语义退化）。

| 编号 | 变异 | 判定 |
| --- | --- | --- |
| M1 | 观察点 hook 改回 `set_hook` | ✅ RED（`installing_the_watch_hook_does_not_evict_an_existing_hook`） |
| M2 | 装管道时用 `set_hook` | ✅ RED |
| M3 | 装载工程时不重装探针管道 | ✅ RED（`load_project_reinstalls_the_bus_hooks_on_the_new_bus`） |
| M4 | 订阅时不推通道列表 | ✅ RED |
| M5 | 通道 id 不带外设 id 前缀 | ✅ RED |
| M6 | 外设时钟不同步（时间戳恒为 0） | ✅ RED |
| M7 / M7b | demo 固件不动 GPIO / 两份固件漂移 | ✅ RED |
| M8 / M9 / M10 | 前端过滤器 / 边沿不累加 / 不订阅 | ✅ RED |
| M12 | 新建宿主不装外设探针回调 | ✅ RED |
| **N1** | `add_hook` 退化成「清空后只留一个」 | ✅ RED（3 条用例） |
| **N2** | 删掉 M-18 → M-19 的 `set_edge_sink` 握手 | ✅ RED（**M-19 从死代码变活**） |
| **N3** | M-19 的环不再收边沿 | ✅ RED |
| **N4** | 单事件边沿上限放大到 100 万 | ✅ RED |
| **N5** | M-18 的边沿存储永不淘汰 | ✅ RED |
| **N6** | `WriteMemory` 后不推波形 | ✅ RED |
| **N7** | 外设 tick 遇错即整体中断 | ✅ RED |
| **N8** | 装配失败不计数 | ⚪ **预期不红**——计数器**无对应用例** → **D-52** |
| **M11** | 删掉 `loadProjectIntoKernel` 里的探针流重订 | ✅ RED（V-B 抓到：此前**前端门禁 152/152 全绿**） |
| **M12** | 前端自启动的竞态（`waveform.reset()` 落在用例之间） | ⚪ **预期不红**——竞态是**偶发**的，判据是**探针**（连跑 N 遍）不是用例 |

**合计 22 条：变红 20 条，2 条「预期不红」——两条都是**已登记的已知缺口**
（N8 → **D-52**；M12 → 竞态偶发，判据是探针），不是漏网。**

⚠️ 复核时发现：**M1 与 M3 在补用例之前是绿的**。M1（观察点 hook 改回
`set_hook`）在「探针 hook 后装」的当前顺序下**无害**——所以判别式必须
**顺序无关**；M3（`LoadProject` 不重装管道）被 V-A-7 的**运行期重试**
掩盖了——所以判别式必须落在「**新总线上的 hook 数**」上而不是「新管道
有边沿」。

### 3.5.7 本轮方法论沉淀

1. ⚠️ **「用例都在」≠「用得着」**。V-A 删掉 M-18 → M-19 的握手，D-46 全部
   10 条用例**照样全绿**——因为用例守的是「**边沿出现**」，不是「M-19 真的
   在用」。**判据要落在「被依赖方自己身上」**：断言 M-19 的环里有边沿，
   而不是断言「有个事件带边沿」。
2. ⚠️ **变异必须能编译**。编译错误导致的「红」**不算判别力**——它证明的是
   「你改坏了」，不是「这条用例在守这个东西」。脚本必须先 `build` 再 `test`。
3. ⚠️ **修复可能**降低**别的用例的判别力**。V-A-7 的「运行期重试」把
   V-A-3 的变异（`LoadProject` 不重装管道）**掩盖**了——变异从 RED 变 GREEN，
   而缺陷还在。**每次改完都要重跑全部变异**，不能只看「自己那条」。
   补法：断言**新总线上的 hook 数**（`bus_hook_count()`），而不是断言「新管道
   有边沿」——后者会被重试补上。
4. ⚠️ **顺序无关的断言才钉得住装配**。「探针 hook 后装」时，M-17 改回
   `set_hook` **无害**——两条用例都绿。判别式必须是「**别人先装的 hook
   还在不在**」。
5. ⚠️ **备份要按文件备份一次**。同一文件的**多次编辑**如果每次都重新备份，
   第二份备份就是「已被改过」的文件——还原时会把**变异残留**留在工作区
   （本轮真踩到了：还原后 `m17_backend` 括号不配对，编译不过）。
   备份必须在**任何编辑之前**做，且按**文件**去重。
6. ⚠️ **上限要分两层**：内存层（M-18 存多少）与事件层（单条事件多大）各管
   一段。只做一层，要么内存爆，要么事件太大。


---

## §3.6 D-07 检视轮（2026-10-21 · 唤醒时间驱动链路）

> **分支**：`feature/d07-time-driven-scheduler`（基线 `main` = `da2581c`）
> **检视对象**：D-07 的实现（`dba9728`）+ 规格（`2c3b83f`）+ 归档（`9e3a869`）
> **三路判定**：**V-A PARTIAL / V-B FAIL / V-C FAIL**
> **共 43 项发现（V-A 13 + V-B 15 + V-C 15）→ 38 项闭环 / 5 项留账**（D-57 ~ D-60、D-62）

### 3.6.1 三路检视的范围划分

| 路 | 范围 | 判定 | 发现 |
| --- | --- | --- | --- |
| **V-A** | Rust 内核实现（M-12 / M-17 / 宿主） | PARTIAL | 13 |
| **V-B** | 测试判别力与变异方法论 | FAIL | 15 |
| **V-C** | 文档一致性 | FAIL | 15 |

三路**互不重叠**：V-A 看「实现对不对」，V-B 看「用例守不守得住」，V-C 看「文档说的对不对」。

### 3.6.2 V-A 报告（Rust 内核）—— 2 中危 + 4 低危 + 7 低危发现

**F1 🟠 同一外设在同一虚拟时刻被 tick 两次**。M-12 内部有两个调用点进同一个
`PeripheralTicker`：指令边界（`tick_peripherals`）与 `PeripheralTick` 事件
（`dispatch`）。D-07 把 M-14 的 `Bus::tick_peripherals` 摘掉了，但**自己这边留了两个
真相**。实测：一次 `continue_until_event()`（10 000 条 = 10 量子）产生 **10 009** 次
tick，其中 9 对是**同一 `now_ps` 重复**；同样的 10 000 条指令，`StepN` 是 **10 000**
次 —— **两条路径的外设时钟对不上**。

> **闭环**：量子边界的 `PeripheralTick` 事件**代劳**本量子**第一条**指令的推进
> （指令边界跳过刚被事件 tick 过的外设）。两条路径的推进序列从此**逐条相同**——
> 这成了新的**不变量**，已用 `both_paths_tick_the_peripheral_clock_identically` 钉住。
> ⚠️ 代价：tick 的条数从此**正好等于**执行的指令数（不是「多出几次」），
> 旧断言 `seen.len() > total_executed` 已随之改写。

**F2 🟠 实现语义与冻结契约不一致**。契约 `crates/contract/src/port.rs` 写的是
「Run until the next breakpoint, watchpoint, trap, or **quantum boundary**」——
量子边界 = 1 µs = 1000 条；实现传 `u64::MAX`，跑到**指令预算**耗尽 = 10 000 条。
契约层冻结，**不能只改实现** → 留账 **D-57**，随 D-32 处理。

**F3 🟡 `step_n` 的「断点检查 ↔ 外设 tick」顺序被调换**（行为变更，无用例守门）。
断点挂在取指之前 → 命中的那条指令**没有执行** → 它的成本不该记账，**外设的时钟也
不该走一格**。**闭环**：断点检查提到 `step_n` 的外层、tick 之前，并补用例。

**F4 🟡 陷阱路径不提交 `pending_writes`**。外设在 tick 期间已经发生的动作，与 guest
这条指令成不成功**无关**；写在队列里不提交 = 外设的写**永远看不见**（写不丢，但
**不可见**）。**闭环**：提交提到陷阱判定**之前**；另外**陷阱不推进时钟**
（陷入的指令没有退休）——两条一起构成「退休 → 记账；副作用 → 照常落地」的规则。

**F5 🟡 `until_ps` 会被冲过**（最多一条指令的成本）。**闭环**：内层改**预检**
（`now + cost > until_ps`）。
> ⚠️ **本轮自查发现并修掉的真缺陷**：预检式修法会让外层循环**永远空转**——
> 时间到头了 → 内层一条也不执行 → 外层再转一圈还是同样的结果 → **死循环**。
> 已补「不许空转」的收手（内层零指令 → 外层立刻返回 `TimeReached`）。

**F6 🟡 陈旧的外设 tick 事件在「换基址」场景下永久累积**。`clear_peripherals` 只清
登记表，不清队列；续排把陈旧事件一直续下去——一件**永远等不到主人**的事件永远留在
队列里。**闭环**：续排前过滤登记表外的 `paddr`。

**F7 ~ F13 ⚪**：`register_peripheral` 幂等无判别力（F7，已补用例）· `Peripheral::tick`
的调用频率与契约口径不一致（F8 → **D-58**）· `quanta` 的单位没说清（F9，已在 doc
comment 写明它是「安全检查点数」而非「跨过的量子数」）· `dispatch` 用当前时钟无判别力
（F10，已补用例）· `Engine::reset()` 是否保留登记表无判别力（F11，已补用例）·
注释与代码矛盾（F12，已订正）· `StepOutcome::HostPause` / `Breakpoint` 是死分支
（F13 → **D-59**）。

**7 处判别力缺口**（V-A 自己的变异跑出来的 0 变红）：M9 / M18 / M20 / M21 / M22 /
M23 / M24 —— 全部已补用例，见 §3.6.6。

### 3.6.3 V-B 报告（测试判别力与变异方法论）—— 2 中危 + 9 低危发现

**F-01 🟠 断口 3 的判别式在两条生产路径之间冗余**。夹具 `RecordingPeripheral::tick`
**每次都返回同一条写**，两条路径拿到的是同一份数据 → 删掉任一条路径的「写收集」
分支，d07 用例照样全绿。**闭环**：夹具加 `emit_at`——**只在某个时刻吐写**；
并补两条用例把两条路径**分开**（指令边界 `t=0` / 量子边界 `t=1 µs`），
外加一条「该写**恰好**被提交一次」（总线 hook 数写次数）。

**F-02 🟠「已收下的写一条不丢」无守卫**。M-12 的注释明写这条性质，但现有用例的
ticker 返回 `Ok(vec![])`，根本不涉及写。**闭环**：补一条「第一个外设成功返回一条写、
第二个报错」的用例，断言那条写仍在队列里。

**F-03 🟠「迟到派发用当前时钟而非事件自带 `at_ps`」无守卫**（LLD 评审 12-6 的口径）。
**闭环**：补一条用例——事件自称 500 ps 到期、实际 1000 ps 才派发，外设必须记 1000。

**F-04 🟠 `a_peripheral_tick_event_is_scheduled_at_the_quantum_boundary` 名不副实**：
它只查「落在量子边界上」，**抓不到「排在当前边界（t=0）而不是下一个边界」**。
**闭环**：补 `the_next_quantum_boundary_is_strictly_after_now`（直接钉公共 API）。

**F-05 🟡 `seen.len() > total_executed` 的余量只有 9，且与预算/配额比值强耦合**。
**闭环**：改成「**正好等于**」（与 F1 的闭环配套），并新增「两条路径的序列**逐条
相同**」这条更强的判别式。

**F-06 🟡 `quanta > full` 是**单向**判别式**：量子粒度错到 1 条指令它照样绿。
**闭环**：收紧为 `quanta == full + 1`。

**F-07 🟡「预算在指令边界上查」只是**偶然**被守着的**——抓住它的是兄弟用例的副作用。
**闭环**：补一条独立用例：预算设成**不是配额整数倍**的 1 500，「只在量子边界查」的
实现会超支 500 条。

**F-08 🟡 变异自检用 **OR 语义**，「14 条按期望变红」**不能**证明逐条判别式**。
**闭环**：第 2/3 轮的脚本改成 **AND 语义**（期望集合里每一条都要红），并记录
**完整红集**。

**F-09 🟡 `RunOutcome::reason` 两个取值都无人断言**；`until_ps` 边界无人走。
**闭环**：新增的预算用例断言 `reason == BudgetExhausted`；`until_ps` 用例断言
`reason == TimeReached`。

**F-10 🟡 外设登记表的生命周期无守卫**。**闭环**：补「`reset` 保留登记表」+
「换内存图后只剩新外设」两条。

**F-11 🟡 生产 ticker 闭包的**按地址分派**无守卫**（多外设场景）。**闭环**：补一条
两个外设的用例，断言各自被自己的地址 tick。

**F-12 ⚪ 两处死接口**：`tick_peripherals` 的返回值无人消费；`continue_until_event` 不
消费观察点命中槽 → 留账 **D-60**。

**F-13 ⚪ M-14 的 `Bus::tick_peripherals` 生产不可达**却仍带测试 → 留账 **D-61**
（本轮已闭环：标 `#[deprecated]`）。

**F-14 ⚪ 夹具每次返回同一条写 → 同一条写整轮被重复提交上万次** → 与 F-01 一并闭环。

**F-15 ⚪ 量子用例的第三条断言是**恒真式**（`INSTRUCTION_COST_PS` 本来就定义成
`TIME_QUANTUM_PS / QUANTUM_INSTR_QUOTA`）→ 改成「换算**只有一个真相**」（M-17 必须
转调 M-12）。

### 3.6.4 V-C 报告（文档一致性）—— 3 中危 + 12 低危发现

**F-1 🟠 §0.1 速查的行数/测试数全部停留在本轮改动前**（M-12 1419→1869 ·
m08..m23 组 2124→2392 · 宿主 1677→1712 · 集成 53→84 · 前端 152/28→156/29）。
**F-2 🟠 文首「基线」行严重脱节**（写 417 / 前端 119 用例）。
**F-4 🟠 三处文档声称「M-12 的单元测试可用假执行器喂它」是假的**——M-12 自己的
`#[cfg(test)]` **零** `run_until` 调用，假执行器只在**集成测试**里。

> ⚠️ 本轮**自查**也抓到一条同类缺陷：变异自检脚本的**备份文件名撞车**
> （两个源文件都叫 `mod.rs`）+ `copyfile` 参数**方向写反** → 变异**累积**在工作区、
> 还原失败，把闭环改动冲掉了。详见 §3.6.7。

其余 12 项为措辞与版本序订正（LLD §2.3 → §3.1 · M-17 §9.1.1 → M-12 §9.1.1 ·
PROGRESS §9 补 v0.15 行 · §11 命令行数字 · §0 TL;DR 标注历史快照 ·
M-12 LLD 评审修订栏版本序 v1.1→v1.2→v1.3）。

### 3.6.5 闭环对照表

| 来源 | 编号 | 严重度 | 处置 | 证据 |
| --- | --- | --- | --- | --- |
| V-A | F1 | 🟠 | ✅ 闭环 | `both_paths_tick_the_peripheral_clock_identically` |
| V-A | F2 | 🟠 | ⏳ **D-57** | 契约层冻结，随 D-32 |
| V-A | F3 | 🟡 | ✅ 闭环 | `a_breakpoint_hit_does_not_advance_the_peripheral_clock` |
| V-A | F4 | 🟡 | ✅ 闭环 | `a_trap_still_commits_the_pending_writes` |
| V-A | F5 | 🟡 | ✅ 闭环（含死循环自查） | `run_until_does_not_overshoot_the_requested_time` |
| V-A | F6 | 🟡 | ✅ 闭环 | `a_stale_peripheral_tick_event_is_not_rescheduled_forever` |
| V-A | F7 | ⚪ | ✅ 闭环 | `registering_the_same_peripheral_twice_ticks_it_only_once` |
| V-A | F8 | ⚪ | ⏳ **D-58** | 随 D-32 |
| V-A | F9 | ⚪ | ✅ 闭环 | `RunOutcome::quanta` 的 doc comment |
| V-A | F10 | ⚪ | ✅ 闭环 | `a_late_dispatched_tick_records_the_current_clock_not_the_event_timestamp` |
| V-A | F11 | ⚪ | ✅ 闭环 | `engine_reset_keeps_the_peripheral_registry` |
| V-A | F12 | ⚪ | ✅ 闭环 | 注释订正 |
| V-A | F13 | ⚪ | ⏳ **D-59** | 既有欠账 |
| V-A | 判别力缺口 M9/M18/M20/M21/M22/M23/M24 | — | ✅ 闭环 | 见 §3.6.6 的 7 条变异 |
| V-B | F-01 / F-14 | 🟠 | ✅ 闭环 | `the_instruction_boundary_path_delivers_the_peripheral_write` + `the_quantum_boundary_event_path_delivers_the_peripheral_write` + `the_peripheral_write_is_committed_exactly_once` |
| V-B | F-02 | 🟠 | ✅ 闭环 | `a_failing_peripheral_does_not_drop_the_writes_already_collected` |
| V-B | F-03 | 🟠 | ✅ 闭环 | `a_late_dispatched_tick_records_the_current_clock_not_the_event_timestamp` |
| V-B | F-04 | 🟠 | ✅ 闭环 | `the_next_quantum_boundary_is_strictly_after_now` |
| V-B | F-05 | 🟡 | ✅ 闭环 | `the_peripheral_clock_is_advanced_by_the_time_driven_path`（改成 `==`） |
| V-B | F-06 | 🟡 | ✅ 闭环 | `a_due_event_preempts_the_quantum_at_an_instruction_boundary`（收紧为 `== full + 1`） |
| V-B | F-07 | 🟡 | ✅ 闭环 | `the_instruction_budget_is_never_exceeded_even_inside_a_quantum` |
| V-B | F-08 | 🟡 | ✅ 闭环 | 变异脚本改 AND 语义 + 记录完整红集 |
| V-B | F-09 | 🟡 | ✅ 闭环 | 同 F-07 用例（`reason == BudgetExhausted`） |
| V-B | F-10 | 🟡 | ✅ 闭环 | `engine_reset_keeps_the_peripheral_registry` + `swapping_the_memory_map_leaves_only_the_new_peripherals_registered` |
| V-B | F-11 | 🟡 | ✅ 闭环 | `each_peripheral_is_ticked_by_its_own_address` |
| V-B | F-12 | ⚪ | ⏳ **D-60** | 两处死接口 |
| V-B | F-13 | ⚪ | ✅ **D-61 闭环** | M-14 `Bus::tick_peripherals` 标 `#[deprecated]` |
| V-B | F-15 | ⚪ | ✅ 闭环 | 量子用例的恒真式断言改写 |
| V-B | 「快照恢复 × 时间驱动」空白 | — | ✅ 闭环 | `the_peripheral_clock_keeps_running_after_a_snapshot_restore` |
| V-C | F-1 / F-2 / F-13 / F-15 | 🟠/🟡 | ✅ 闭环 | §0.1 / 文首基线 / 台账归属 / 合计口径 |
| V-C | F-4 | 🟠 | ✅ 闭环 | 三处「假执行器」措辞订正 |
| V-C | F-3 | 🟠 | ✅ 已闭环 | `9ac4df9`（分支 tip 先行修掉） |
| V-C | F-5 ~ F-12 / F-14 | 🟡/⚪ | ✅ 闭环 | PROGRESS §0/§4/§5/§9/§11 · AGENTS.md 行数 · M-12 LLD 版本序 |

### 3.6.6 反向变异自检（25 条）

第 1 轮 14 条（D-07 落地时）· 第 2 轮 11 条（V-A 的 7 处判别力缺口 + 4 条回归）·
第 3 轮 11 条（V-B 的发现 + 2 条已知无守 + 1 条脆弱性探针）。
**口径**：变异必须**能编译**（编译错误不算判别力）· 期望集合用 **AND 语义**逐条核对 ·
记录**完整红集** · 还原必须**逐字节**校验。

| 轮 | 变异 | 期望变红 | 结果 |
| --- | --- | --- | --- |
| 2 | M9 `dispatch` 用 `at_ps` 而非当前时钟 | 迟到事件用例 | ✅ |
| 2 | M18 `register_peripheral` 不去重 | 登记幂等用例 | ✅ |
| 2 | M20 `clear_peripherals` 空操作 | 陈旧事件用例 | ✅ |
| 2 | M21 内层不检查 `until_ps` | 不冲过用例 | ✅ |
| 2 | M22 提交待写 / 推进时钟顺序颠倒 | 成本记账用例 | ✅ |
| 2 | M23 `step_n` 断点检查移到 tick 之后 | 断点不推进时钟用例 | ✅ |
| 2 | M24 `Engine::reset` 清登记表 | reset 保留登记表用例 | ✅ |
| 2 | F1 回归：指令边界不再跳过刚 tick 过的外设 | 两条路径一致用例 | ✅ |
| 2 | F4 回归：陷阱先返回，提交挪到后面 | 陷阱也提交用例 | ✅ |
| 2 | F5 回归：撤掉「不许空转」的收手 | 不冲过用例（**超时**＝卡住＝红） | ✅ |
| 2 | F6 回归：续排前不过滤登记表外的 `paddr` | 陈旧事件用例 | ✅ |
| 3 | B04 `tick_peripherals` 收下写但不并入队列 | 指令边界投递用例 | ✅ |
| 3 | C02 `dispatch` 事件路径不并入写 | 事件路径投递用例 | ✅ |
| 3 | B02 外设报错时丢弃已收下的写 | 写一条不丢用例 | ✅ |
| 3 | F02 `next_quantum_boundary` 返回当前边界 | 下一个边界用例 | ✅ |
| 3 | A10 删掉量子内的预算检查 | 预算硬上限用例 | ✅ |
| 3 | C09 换内存图不清登记表 | 换图用例 | ✅ |
| 3 | C03 生产闭包忽略 `paddr` | 多外设分派用例 | ✅ |
| 3 | C04 `step_n` 不重置 `last_stop` | 停止原因不残留用例 | ✅ |
| 3 | D04 `continue` 不消费观察点命中槽 | — | ⏳ **0 红**（→ **D-60**） |
| 3 | B22 `tick_peripherals` 返回值被改 | — | ⏳ **0 红**（→ **D-60**） |
| 3 | E01 配额 1000→20000（合法配置改动） | — | ⚠️ **脆弱性探针**（→ **D-62**） |

### 3.6.7 本轮方法论沉淀

1. ⚠️ **「同一个回调的两条调用路径」必须用「限定时刻」把夹具分开**。
   D-07 的外设 tick 有两条生产路径（指令边界 / 量子边界事件），共用同一个回调。
   夹具若每次都吐同一份数据，两条路径的判别式就**互相掩护**——删掉任一条，
   用例照样全绿。**夹具要能「只在某一刻」给出数据**，两条路径才守得住。
2. ⚠️ **「同一份数据不能有两个真相」要有**不变量**用例，不只是实现里改对**。
   F1 修完之后，「两条路径的外设推进序列**逐条相同**」才是一条能被变异打红的
   不变量——实现改对了但没写下不变量，下一次照样会漂。
3. ⚠️ **反向变异自检的期望集合要用 AND 语义**。OR 语义（「期望里任意一条红就算
   红」）只能证明「某条期望的用例红了」，**不能**证明「每条期望各自都守得住」——
   变异往往顺带红掉别的用例，把真正的漏网之鱼盖住了。**完整红集**要一起记下来。
4. ⚠️ **变异脚本的备份文件名必须唯一，且 `copyfile` 的方向要写对**。本轮真踩了：
   两个源文件都叫 `mod.rs` → 备份互相覆盖；`copyfile(src, dst)` 写成
   「备份 ← 源」而不是「源 ← 备份」→ 还原其实是**再备份一次**。症状是**变异累积**：
   后面的每条变异都跑在前一条的残骸上，红集里全是无关用例。**还原后必须逐字节
   校验**（本脚本用 md5 比对）。
5. ⚠️ **修一处「更精确」的判据，可能引入死循环**。F5 的预检式修法让内层循环在
   「时间到头」时一条也不执行，而外层循环的退出条件是「时间到头」——**两个条件
   互斥**，于是永远转不出去。**改一处行为，要顺着把终止性再走一遍**。
6. ⚠️ **恒真式断言要挑出来**。`INSTRUCTION_COST_PS` 本来就**定义成**
   `TIME_QUANTUM_PS / QUANTUM_INSTR_QUOTA`——再断言「量子 = 配额 × 成本」只是把
   定义重说一遍。要钉的是「**换算只有一个真相**」（M-17 必须转调 M-12）。
7. ⚠️ **「用例钉的是默认值」这件事必须写在文件头**。LLD 说量子与配额**可按工程
   配置**，代码里暂时是编译期常量；用例若隐式依赖默认值，将来支持配置时会**一片
   地红**。凡本来就与配置无关的不变量，一律写成**相对单条指令成本**的形式。

---

## §3.7 M+C 扩展检视轮（2026-10-21 · RV32M 8 条 + RV32C 26 条）

> **分支**：`feature/m13-mc-extensions`（基线 `main` = `8e12ff5`）
> **检视对象**：`36ab078`（实现 + 用例 + 语料翻转 + LLD v1.6 同步）
> **三路判定**：**V-A PARTIAL / V-B PASS / V-C PARTIAL**
> **共 35 项发现（V-A 6 + V-B 12 + V-C 17）→ 30 项闭环 / 5 项留账**（D-63 ~ D-67）
> **反向变异自检 3 轮共 45 条 → 44 条按期望变红**（1 条为**语义空操作**，见 §3.7.6）

### 3.7.1 三路检视的范围划分

| 路 | 范围 | 判定 | 发现 |
| --- | --- | --- | --- |
| **V-A** | Rust 内核实现（`compressed.rs` / `decoder.rs` / `executor.rs` / `alu.rs`） | PARTIAL | 6 |
| **V-B** | 测试判别力与变异方法论 | PASS | 12 |
| **V-C** | 文档一致性（LLD / 台账 / PROGRESS / AGENTS / 语料头） | PARTIAL | 17 |

三路**互不重叠**：V-A 看「实现对不对」，V-B 看「用例守不守得住」，V-C 看「文档说的对不对」。

⚠️ **本轮的一个结构性事实**：V-A 与 V-B **独立地**发现了**同一处**判别力缺口
（CB 偏移的符号位与幅度位）——V-A 叫它 G2/G8，V-B 叫它 P1-1，两路的**实验手段不同**
（V-A 注入变异后跑**全量 golden 语料**，V-B 注入后跑**单元测试**），却得出同一个结论。
**两条独立路径撞到同一个洞，比一条路径报两次更可信。**

### 3.7.2 V-A 报告（Rust 内核）—— 1 高危 + 2 中危 + 3 低危

**方法**：不依赖实现自证——用 GNU as 汇编 15 个差分程序，用 objdump 逐条确认每个
`c.*` 都落成 **16 位编码**，再把同一 ELF 分别跑 Spike（`--log-commits`）与候选 ISS，
**逐指令**比对指令流 / 每次访存 / 最终 32 个 GPR。

| 差分程序 | 结果 |
| --- | --- |
| `c_addi` / `c_li` / `c_lui` / `c_addi16sp` / `c_addi4spn` | 1066 / 810 / 798 / 290 / 807 条指令，**GPR 全等** |
| `c_slli` / `c_srli` / `c_srai` / `c_andi` / CA 组 | 各 414~554 条指令，**GPR 全等** |
| `c_mvadd` / `c_lwsw` / `c_lwspswsp` / `c_flow` / `c_longbr` | 186~938 条指令，**GPR 全等**（`c_longbr` 位移 −208..+194） |
| M 组 8 条 | 各 848 条指令（169 组操作数），**全等**（含除零 / `INT_MIN/-1` 溢出 / 符号混用） |
| 保留 / HINT 分类扫描 | **306 个编码，0 分歧**（每个编码一个独立程序，判「marker store 有没有发生」） |

> **结论：26 条压缩指令 + 8 条 M 指令的语义与 Spike 逐条完全一致**，包括
> `c.lui` 字段 `0b100001 → 0xFFFE1000`、`c.addi16sp ±512/496`、`c.lwsp 252`。

**F1 🟠 CB 偏移的符号位与 `imm[7:6]` 在整个门禁上是盲区**。变异 G2（`imm[7:6]` 整体上移一位）
与 G8（丢弃符号位）**单元测试 0 红**、**golden 语料 51/51 放行**。根因：本分支的分支用例
只有 `+4` 偏移（`0xC011`），解码它**只碰 `imm[2:1]` 一个位域**；riscv-tests 的 `rvc.S`
也只用小前向位移。CJ 侧因为有一条 −4 偏移用例（`0xBFF5`，高位全是 1）而被守住，CB 侧没有。
> **闭环**：见 §3.7.5 的 V-B P1-1 条（补 16 个偏移用例，覆盖 CB 偏移的**全部 5 个位域**
> 与两个 2 位域的**全部 4 种取值**）。

**F2 🟡 C.LUI「20 位截断」的理由在代码注释与 LLD v1.6 里都算错了**。算术反证：
`sext6(0b100001) = -31`，而 `(-31 as u32) << 12 = 0xFFFE_1000`——**与实现给出的值完全相同**；
`0xFFFF_E000` 其实是 `sext6(0b100000) = -32` 的值。32 位补码里 `(x & 0xFFFFF) << 12 ≡ x << 12`，
所以「先截 20 位再 <<12」这条规则**是空操作**，根本区分不出两种写法。
> **闭环**：`fe081e1` 订正了 4 处措辞（`compressed.rs` 模块头 · `tests_rv32c.rs` 文件头与
> C.LUI 用例头 · LLD §3.5 立即数规则块），并明确写出「**错误读法是哪一个**」：
> 取 20 位于**高位**（`sext6 & 0xFFFFF_000 = 0xFFFF_E000`）。
> ⚠️ **本轮三条独立路径收敛定案**：① 专用 Spike 探针（用 golden env 的 `RVTEST_FAIL` 编号
> 区分两候选值）→ Spike 退出码 1 = `0xFFFE_1000`；② 算术反证 + 变异 0 红 + 63 个字段值
> 差分全等；③ 独立 Python 参考实现确认 `sext6<<12 == trunc20<<12`（RV32）。
> ⚠️ **已知的历史瑕疵**：分支首个 commit `8c3e50f` 的 message 含「截断与 sext6<<12 高位不同」
> 这句**错误说法**。分支尚未 push，但**不重写历史**（会破坏三路检视的审计链）——在此显式勘误。

**F3 🟡 mtval 全平台未落地** → 留账 **D-64**。

**F4 ⚪ `Decoded.illegal` 没有任何测试断言**。变异 G7（`illegal: true → false`）0 红——但这是
**行为等价**变异（保留编码的字段全 0 → opcode 0 → `classify` 仍返回 None → 仍陷阱），
所以 0 红**本身不算缺陷**；问题在于 `illegal` 是 `Decoded` 的 **pub 字段**，其契约无人钉住。
> **闭环**：保留编码表补 `assert!(d.illegal)`。
> ⚠️ **补断言时实际抓出一条语义差别**：`illegal` ≠ 「会陷阱」——**`c.ebreak` 是合法指令**
> （不 illegal），但它触发 breakpoint 陷阱。两者必须分开断言（见 §3.7.5 V-B P3-1）。

**F5 ⚪ golden 两侧栈窗口不一致（预存）+ `golden-run.sh` 注释地址写错** → 留账 **D-65**；
本轮已订正注释（`0x7ffe_f000` → `0x7fff_f000`）。

**F6 ⚪ `decoder::fetch_size()` 是死代码（预存）** → 留账 **D-66**（与 V-B 的 `Decoded.compressed`
同属「死接口族」）。

### 3.7.3 V-B 报告（测试判别力）—— 3 阻断 + 1 高危 + 4 中危 + 4 低危

**方法**：**95 次语义变异**（3 次模式写错已重跑）→ 有效 **92 条：78 RED / 14 MISS**；14 条 MISS
**全部用 `touch` 强制失效 cargo 指纹后重跑**复核（排除「陈旧二进制」假象）。另外做了
**三条独立取证**：41 条主表编码逐条比对 GNU as · 自写**规范派生**（非实现派生）Python
解码器复核全部 76 条常量 · 自建 **Spike oracle**（`--log-commits` 判「该编码是否提交」）
跑 35 条保留 / HINT 编码。

**P1-1 🔴 CB 分支偏移的符号位与幅度位完全无人守**。M53（符号位取 `inst[11]`）→ **0 红**；
M70 / M71 / M72（分别丢 `imm[7:6]` / `imm[4:3]` / `imm[5]`）→ **各 0 红**。
**更强的证据**：把这 4 个变异灌进 golden 语料跑全量 → **total=51 ok=51 mismatch=0**——
**单元测试与 golden 语料双双守不住**。
> **闭环**：新增 `c_beqz_and_c_bnez_land_on_the_right_target`——**16 个偏移用例**，
> 判别式落在**落点地址**上（不是「是否跳转」：算错目标的解码器照样会跳转）。

**P1-2 🔴 `c.slli` 的 5 位 rd 域宽度未被守**。M65（rd 收窄成 3 位）→ 0 红；灌进语料 → 51/51 仍全通。
> **闭环**：`c_slli_shifts_into_a_high_destination_register`（`c.slli x31,1` / `x16,3`）。

**P1-3 🔴 `c.swsp` 的 `uimm[7:6]` 与 5 位 rs2 未被守**。M78 / M79 → 各 0 红；灌进语料 → 51/51 仍全通。
> **闭环**：`c_swsp_reaches_the_largest_offset_from_a_high_register`（`c.swsp x31, 252(sp)`）。

**P2-1 🟠 `c.li` 的 `rs1 = x0` 未被单元测试钉住**（但 golden 语料兜住了）。M12（`c.li` 退化成
`c.addi`）→ 595 全绿。根因：用例**没预置** `state.gpr[8]`，于是 `addi x8,x0,-16` 与
`addi x8,x8,-16` 结果相同。
> **闭环**：用例预置非零值。⚠️ 灌进语料时 `rv32uc/rvc: spike=PASS ours=FAIL 9` ——**语料兜住了**，
> 但单测单独看是「看着有、其实没有」的判别式。

**P3-1 🟡 `Decoded.illegal` 与 `Decoded.compressed` 无任何观察者**。M31（`illegal: true→false`）/
M32（删掉 `classify` 的 `if d.illegal` 守卫）/ M63（`compressed: true→false`）→ **各 0 红**。
> **闭环**：`illegal` 补断言（并由此抓出 `c.ebreak` 的「合法但会陷阱」）；`compressed` 的死字段
> 归 **D-66**。

**P3-2 🟡 M 扩展读端的 x0 硬连线未被守**。M85（`Op::Mul` 读 rs1 绕过 `read_gpr`）→ 0 红。
> **闭环**：`the_m_extension_cannot_read_x0`。
> ⚠️ **本轮自查发现这条用例自己也是假判别式**：原写法是
> `enc_m(funct3, rs2=4, rs1=3, rd=5)`——**rs1 是 x3 不是 x0**（注释与编码不符），
> 断言恒真。已改为 **rs1 = x0 并污染 `state.gpr[0]`**（ISS 丢弃一切对 x0 的写，
> 没人动过的 x0 本来就读出 0，绕过守卫的读法会给出同一个答案）。

**P3-3 🟡 `c.lui` 的「20 位截断」在 RV32 不可观测**。M13（`sext6<<12` 取代 `trunc20<<12`）/
M83（掩码 20→21 位）→ 各 0 红；而 M91（掩码 20→**19** 位）→ 变红。
> **闭环**：与 V-A F2 合并处理（措辞订正 + 三路取证定案）。

**P3-4 🟡 保留编码的守卫过度集中 + 跨模块隐式耦合**。6 处保留检查**全部只由一条**用例守住；
且 D-07 的 `a_trap_still_commits_the_pending_writes` 依赖的正是「低半字 `0x0000` 是保留
C.ADDI4SPN」这条 C 扩展规则 → 留账 **D-67**。

**P4 🟢 4 处文档性错误**（不影响判别力，但会误导后续检视）→ 全部闭环：① `compressed.rs` 的
「`sext6<<12` 会是 `0xFFFF_E000`」· ② `tests_rv32c.rs` 同款错误 · ③ 「`c.lui` 零立即数是最锐利
HINT」（它是**保留编码**，真锐利的是 `0x0401` / `0x8001`）· ④ 文件头「Every encoding in this
file was produced by GNU as」——**对 35 条保留 / HINT 表不成立**（GNU as 直接拒收这些码点）。

### 3.7.4 V-C 报告（文档一致性）—— 0 阻断 + 5 高危 + 9 中危 + 3 低危

**F1 🟠 四个文档的统计数字整体停留在 `9ec3b4a`**。根因可精确定位：文档同步 commit `36ab078`
是基于 `9ec3b4a` 的树统计的，而 `4bf0eed`（+77 行 / +2 条用例）未被计入。
> **闭环**：全部数字**从当前树重算**（脚本 `sync_numbers.py` 直接数当前树，正则幂等替换），
> **不做「上一步数字 + 增量」推算**——那正是本条发现的根因。

**F2 🟠 LLD §3.2 伪码仍是 v1.5 的「32 位位模式」展开** · **F3 🟠 §3.2 的 `Dec` 伪码缺
`len` / `illegal` / `pc`** · **F4 🟠 §5.1 / §5.2 仍写「2/4 对齐检查」** · **F5 🟠 评审修订栏
版本序倒置**（`v1.3 → v1.4 → v1.6 → v1.5`）。
> **闭环**：LLD **v1.6 → v1.7**——§3.2 伪码改字段级并补三个字段 · §3.5 补立即数规则 /
> 共享槽位规则 / 保留-HINT 分类表（补 3 类 HINT）· §5.1 / §5.2 对齐措辞 · 评审修订栏版本序升序。
> ⚠️ **F4 是行为级规格矛盾**：按 §5.1 实现会引入 v1.6 明确排除的 4 字节对齐检查，
> 并导致 2 字节对齐地址上的 32 位指令被误判。

其余 12 项为措辞与口径订正（`compressed.rs` 注释矛盾 · `IntegerDivideByZero` 注释过期 ·
`fetch_size()` 被 LLD 写成「权威出处」而它 0 调用点 · §4 M-13 行仍写「M/C 扩展待做」·
「44/44」在当前树不可复现（实为 45）· 语料头仍说 M/C 例「start as pending」·
合计行 213/42 与逐行求和 212/43 不符）→ 全部闭环。

### 3.7.5 闭环对照表

| 来源 | 编号 | 严重度 | 处置 | 证据 |
| --- | --- | --- | --- | --- |
| V-A | F1 | 🟠 | ✅ 闭环 | `c_beqz_and_c_bnez_land_on_the_right_target`（16 个偏移） |
| V-A | F2 | 🟡 | ✅ 闭环 | `fe081e1` + LLD v1.7 §3.5；三条独立路径取证定案 |
| V-A | F3 | 🟡 | ⏳ **D-64** | mtval 通路缺失（契约层冻结，归 D-32） |
| V-A | F4 | ⚪ | ✅ 闭环 | 保留编码表补 `assert!(d.illegal)` |
| V-A | F5 | ⚪ | ⏳ **D-65** | 注释已订正；两侧栈窗口对齐待做 |
| V-A | F6 | ⚪ | ⏳ **D-66** | 死接口族（`fetch_size()` / `compressed` / `IntegerDivideByZero`） |
| V-B | P1-1 | 🔴 | ✅ 闭环 | `c_beqz_and_c_bnez_land_on_the_right_target` |
| V-B | P1-2 | 🔴 | ✅ 闭环 | `c_slli_shifts_into_a_high_destination_register` |
| V-B | P1-3 | 🔴 | ✅ 闭环 | `c_swsp_reaches_the_largest_offset_from_a_high_register` |
| V-B | P2-1 | 🟠 | ✅ 闭环 | `c_li_loads_a_sign_extended_immediate`（预置非零） |
| V-B | P3-1 | 🟡 | ✅ 闭环 | 保留编码表补 `d.illegal` 断言（并抓出 `c.ebreak` 语义差别） |
| V-B | P3-2 | 🟡 | ✅ 闭环 | `the_m_extension_cannot_read_x0`（**修掉一条假判别式**） |
| V-B | P3-3 | 🟡 | ✅ 闭环 | 与 V-A F2 合并 |
| V-B | P3-4 | 🟡 | ⏳ **D-67** | D-07 用例对 C 扩展保留规则的隐式耦合 + 守卫过度集中 |
| V-B | P4-1 ~ P4-4 | 🟢 | ✅ 闭环 | `fe081e1` + 测试文件头订正 |
| V-C | F1 | 🟠 | ✅ 闭环 | 数字全部**从当前树重算**（`sync_numbers.py`） |
| V-C | F2 / F3 / F4 / F5 | 🟠 | ✅ 闭环 | LLD **v1.7** |
| V-C | F6 ~ F17 | 🟡/⚪ | ✅ 闭环 | `fe081e1` + LLD v1.7 + 台账 + PROGRESS + AGENTS + 语料头 |
| V-C | F12（mtval / mcause） | 🟡 | ⏳ **D-63** | 取指未对齐与访存未对齐共用 `TrapCause` |

### 3.7.6 反向变异自检（3 轮共 45 条）

**口径**：变异必须**能编译**（编译错误不算判别力）· 期望集合用 **AND 语义**逐条核对 ·
记录**完整红集** · 还原必须**逐字节**校验（md5）· **还原后必须 `touch` 源文件**
（否则 cargo 指纹未失效 → 下一条变异跑在**陈旧二进制**上）。

| 轮 | 变异 | 期望变红 | 结果 |
| --- | --- | --- | --- |
| 1 | 16 条（M/C 判别力扫描） | 逐条 | **12/17 变红** → 暴露 **2 处真实缺口**（`remu` 位 31 · CR 组高寄存器） |
| 2 | 18 条（补强后复检） | 逐条 | **17/18 变红**（1 条 MISS = C.LUI 的**语义空操作**） |
| 3 | 11 条（检视轮发现的缺口） | 逐条 | **11/11 全部变红** |

**第 3 轮明细**（隔离 worktree `/tmp/wt/mut3` @ `db22172`，结束时 `git status` 干净、
md5 校验还原）：

| # | 变异 | 期望变红的用例 | 红集 |
| --- | --- | --- | --- |
| 1 | CB 偏移的符号位被丢弃 | `c_beqz_and_c_bnez_land_on_the_right_target` | 1 |
| 2 | CB 偏移的 `imm[7:6]` 被丢弃 | 同上 | 1 |
| 3 | CB 偏移的 `imm[4:3]` 被丢弃 | 同上 | 1 |
| 4 | CB 偏移的 `imm[5]` 被丢弃 | 同上 | 1 |
| 5 | CB 偏移的 `imm[2:1]` 被丢弃 | 同上 | **3** |
| 6 | `c.slli` 的 rd 收窄成 3 位 | `c_slli_shifts_into_a_high_destination_register` | 1 |
| 7 | `c.swsp` 的 `uimm[7:6]` 被丢弃 | `c_swsp_reaches_the_largest_offset_from_a_high_register` | 1 |
| 8 | `c.swsp` 的 rs2 收窄成 3 位 | 同上 | 1 |
| 9 | `c.li` 读自己的目的寄存器 | `c_li_loads_a_sign_extended_immediate` | 1 |
| 10 | 乘法的读端绕过 x0 硬连线 | `the_m_extension_cannot_read_x0` | 1 |
| 11 | 除法的读端绕过 x0 硬连线 | 同上 | 1 |

> **11/11 全部按期望变红**（AND 语义逐条核对）。第 5 条的红集是 3 条——`imm[2:1]`
> 被丢弃时，那 3 个「分支落点」判别式一起变红（另两条是 `c.beqz_branches_only_on_zero` 一族）。

### 3.7.7 本轮方法论沉淀

1. ⚠️ **「测试常量与实现同源」的风险要单独证伪**。V-B 用**三条独立来源**核对了全部
   76 条常量：GNU as（汇编器输出）· 自写**规范派生**解码器（**不是**实现派生的 Python
   实现）· Spike oracle（判「该编码是否退休」）。压缩格式的位域极易错位，一个
   「照着实现写测试」的用例会与一个坏解码器**一致地错**。
2. ⚠️ **「0 红」不一定是缺口，也可能是**语义空操作**。C.LUI 的「20 位截断」变异 0 红，
   但那是因为两种写法在 RV32 下**逐位相同**——**不是**测试没守住。判「缺口 vs 空操作」
   要**手工推演语义**，不能只看红集。（V-A 与 V-B 独立地撞上这一条，且都给了算术反证。）
3. ⚠️ **变异还原后必须 `touch` 源文件**。`shutil.copy2` 保留 mtime → cargo 指纹未失效
   → 下一条变异跑在**变异窗口期编译出来的陈旧二进制**上。V-B 实测踩到过：golden 报出
   假失败 `rv32uc/rvc: ours=ERROR ... fetch at pc=0x00000004`（那个 pc 恰好是另一条
   变异的症状）。`touch` 后重建 → 51/51 全通。
4. ⚠️ **「两条独立路径撞到同一个洞」比一条路径报两次更可信**。V-A 注入变异后跑**全量
   golden 语料**，V-B 注入后跑**单元测试**——**实验手段不同**，却独立地得出「CB 偏移
   的符号位与幅度位无人守」这一条。
5. ⚠️ **分支偏移的判别式要落在「落点地址」上**，不是「是否跳转」——算错目标的解码器
   **照样会跳转**。本轮新增的 16 个偏移用例全部按落点断言。
6. ⚠️ **用例自己也要判别力**。本轮自查发现 `the_m_extension_cannot_read_x0` 的
   `rs1` 写的是 **x3 而不是 x0**（注释与编码不符）——**一条恒真的断言**。而且即使
   `rs1` 写对了还不够：**必须污染 `state.gpr[0]`**，否则「没人动过的 x0」与「绕过守卫
   直接读裸槽位」给出同一个答案。
7. ⚠️ **「保留 / 非法」与「会陷阱」是两件事**。`c.ebreak` 是**合法指令**（`illegal` 为假）
   却触发 breakpoint 陷阱。补断言时才发现的——**断言必须分开写**。
8. ⚠️ **数字口径一律从当前树重算**，不做「上一步数字 + 增量」推算。V-C 查到的四份文档
   数字失真，**根因就是「按上一 commit 推算」**——文档同步 commit 基于 `9ec3b4a` 的树
   统计，漏了紧随其后的 `4bf0eed`。
9. ⚠️ **「规格里写着的约束」必须有一条能变红的用例**。LLD §5.1 的「2/4 对齐检查」与
   §3.1 的「只查低 1 位」**互相矛盾**——**改实现不会让任何用例变红**，所以这条矛盾能
   潜伏很久。规格矛盾要靠**对照**发现，不靠测试。
10. ⚠️ **跨模块的隐式耦合要在检视里点名**。D-07 的用例用 `vec![0u8; 4]` 造陷阱，
    依赖的却是 **C 扩展**的保留编码规则——两件事在文档上毫无关系，改一边会意外改掉
    另一边（台账 **D-67**）。

---

## §3.8 契约层 v2 检视轮（2026-10-21 · 契约层**第一次**升版）

**分支**：`feature/contract-schema-v2`（基线 `main` = `2793e0e`）· **冻结规格**：`docs/detailed-design/STAGE-2_Contract_V2.md`

本轮是契约层自 v0.10 起**第一次**升版（`schema_version` 1 → **2**）。**先写规格冻结、再三路并行施工**，
最后**三路独立检视**。契约层冻结期间积压 **9 条**台账，其中 **6 条**卡在「契约层不升版就改不了」
（**D-32 / D-34 / D-39 / D-40 / D-57 / D-58**）——本批全部解冻。

### 3.8.1 三路检视的范围划分

| 检视员 | 范围 | 判决 |
| --- | --- | --- |
| **V-A** | `crates/contract/**` + `crates/kernel/src/{kernel_host,m17_backend,m20_snapshot}/**` | **FAIL（限定范围）**——**未发现运行时功能缺陷**，但发现 **2 高危 + 4 中危 + 2 低危 + 2 观察** |
| **V-B** | `front/**` + `tools/{wasm-smoke,browser-verify}.mjs` | **FAIL**——前端实现基本符合规格，B-4 三条判别式逐条变异验证**都有判别力**；但 **2 高危 + 3 中危 + 2 低危 + 4 观察** + **B-6 确认的静默假绿** |
| **V-C** | 三方契约（Rust 契约 / JS 镜像 / 两份桥规格）+ `AGENTS.md` / `REVIEWS.md` / `PROGRESS.md` | **PARTIAL**——代码正确且门禁全绿，但 **3 高危 + 7 中危 + 5 低危 + 5 观察**；并给出**完整的数字口径对照表** |

> 三路检视都在**只读 worktree**（`/tmp/wt/c2-review` @ `7bacbdf`，detached）里进行，
> 变异产物全在 `/tmp/probe/`，结束时检出树 `git status --short` **干净**。
> **检视员严禁 `git reset` / `checkout --` / `restore` / `clean` / `push` / `commit`。**

### 3.8.2 V-A 报告（契约 + 内核）—— 2 高危 + 4 中危 + 2 低危 + 2 观察

| # | 严重度 | 发现 | 闭环 |
| --- | --- | --- | --- |
| 1 | 🔴 高危 | **`SNAPSHOT_SCHEMA_VERSION` 的线上表示无用例钉住**——全部用例**符号化**引用常量，只断言**键集**不断言**值形态**。实测：把它改回 1 → 契约层 32 条 + **全工作区 631 条**全绿，一份 v1 形态的快照被**静默接受** | ✅ `e8bc706`：补**字面量**断言 `the_snapshot_schema_version_is_two` |
| 2 | 🔴 高危 | **`SimState.virtual_time_ps` 的字符串表示无用例钉住**——同型。实测：删掉 `#[serde(with = "u64_decimal")]` → 全工作区全绿 | ✅ `e8bc706`：`numstr::field_tests::every_virtual_time_field_travels_as_a_decimal_string`——**一条用例钉住全部 10 个字段位点** |
| 3 | 🟠 中危 | `EdgeEvent.t_ps` / `TraceEvent.t_ps` / `WaveformDelta.t_virt` 三处字符串表示无用例钉住（3 条独立变异全绿） | ✅ 同上（并进那一条表驱动用例） |
| 4 | 🟠 中危 | **三份设计文档写了冻结规格明文判错的措辞**（`count = 0` → **不回复**）。M-10 LLD 把它写成了**通过准则**——照着实现会做出与规格相反的行为 | ✅ `7dbf55c`（文档侧） |
| 5 | 🟠 中危 | **「新增一个 `ErrorFamily` 变体但忘了登记」抓不住**——完备性断言只比两条**手工清单的长度**。实测：加变体 + 加 `as_str()` 分支、两处清单都不动 → 32 条全绿 | ✅ `e8bc706`：族清单改**单一来源**（`error_families!` 宏展开 enum + `as_str()` + `ALL_FAMILIES`），**漏登记 → 编译错误** |
| 6 | 🟠 中危 | **D-40 台账声称的判别力不成立**——`a_snapshot_of_a_running_guest_is_not_empty` 的夹具**带着 3 条事件**，于是「快照非空」与「队列非空」**同值**。实测：把 `is_empty()` 换成 `self.queue.is_empty()` → 32 条全绿 | ✅ `e8bc706`：夹具改成「**队列空、但 guest 跑过**」 |
| 7 | 🟠 中危 | **`SIM_STATE_SCHEMA_VERSION` 未 bump**——契约自己的规则是「改了字段集**或线上表示**，**两个**版本号一起 bump」。留着不 bump 的后果不是「多一层保险」，而是**两个版本号含义分叉**：将来 v3 把内层升到 2 时，会把 **v2 自己产的快照拒掉** | ✅ `e8bc706`：升到 **2** + 补字面量断言 |
| 8 | 🟡 低危 | 台账多处口径与代码事实不符（5 处：29→32 条 / 「前端 4 条红」/ D-69 已补 / D-40 判别力 / 横幅 6→9） | ✅ `7dbf55c` |
| 9 | 🟡 低危 | `m20_snapshot::SnapshotRecord.at_ps` 是裸 `u64`（无 serde derive，当前不落盘，**无活缺陷**），但落盘接线时**必须**走 `numstr::u64_decimal` | ✅ `e8bc706`：补 doc 注记钉住口径 |
| 10 | ⚪ 观察 | **内核的 `schema_version` 严格等号 vs JS 的前向兼容，语义不对称**（**pre-existing**，非本批引入；内核侧的等号是 v0.13 第 1 阶段 V-A 检视 `aa75535` 定的，理由是 serde **没有** `deny_unknown_fields`） | ⏳ **登记为 D-75** |
| 11 | ⚪ 观察 | `error.rs` 模块头把族标签长度从「3–4」改成「3–5」，规格 §1.1 的破坏性变更清单里没有这一条（**只动注释，线上零位移**） | ✅ `e8bc706`：规格 §1.1 补一段**纯文档订正**说明 |

**V-A 确认无问题的地方**（免得下一轮重复查）：虚拟时间的 **10 个字段位点实现全部到位**（缺的只是用例）·
A-3「公开 API 只增不减」**未被破坏**（全部删除行都经规格授权）· D-57 / D-58 确实是「**只改文档不改实现**」
（`port.rs` 的 diff **只有 doc comment**）· 改名**彻底**（事件变体名的残留为 0）· `numstr` 的反序列化
**不放行非法输入**（`visit_str` 走 `u64::from_str`、`visit_i64` 拒负数、**`visit_f64` 未实现 → 浮点被拒**）。

### 3.8.3 V-B 报告（前端 + 传输帧）—— 2 高危 + 3 中危 + 2 低危 + 4 观察

| # | 严重度 | 发现 | 闭环 |
| --- | --- | --- | --- |
| F1 | 🔴 高危 | **mock 的 `Negotiate` 回显宿主的 `cap_bits`** —— 违反规格 §5「回**内核授予的** cap_bits」。更糟的是：前端那条用例**钉的就是这个错值**，门禁在**锁定一个规格违规** | ✅ `e48c2d9`：mock 回**自己授予的**那组（新增导出 `MOCK_CAP_BITS`），用例从「钉死 `0x0f`」改成「= 内核授予的那组 + ≠ 宿主报价 + 非 0」 |
| F2 | 🔴 高危 | **跨语言对拍门禁看不见 §2 数值策略的不同步** —— 对拍只解析**名字与形状**，不解析 serde 表示。实测：删掉 Rust 侧 `Tick.elapsed_ps` 的 numstr 属性 → **门禁 18/18 全绿** | ✅ `a2de235`：新增 `rustFieldDeclarations()`（解析字段**及其上方**的 `#[serde(...)]` 属性）+ 一条**正向**用例；逐个删 Rust 侧 **10 处** numstr 属性 → **10/10 全红** |
| F3 | 🟠 中危 | **对拍门禁看不见嵌套 DTO 的字段名漂移** —— `cases` 列表没有 v2 新增的 `DeltaRegister`。实测：把 Rust 的 `DeltaRegister.index` 改成 `reg_index` → **18/18 全绿** | ✅ `a2de235`：`message.js` 新增 `deltaRegister()` 构造器（**同一真相**），并把 `DeltaRegister` 加进 DTO 字段名检查列表 |
| F4 | 🟠 中危 | **M-10 在 `elapsed_ps` 缺失时把虚拟时间回退到 0** —— 实测时间轴从 1000 **倒退**到 0 | ✅ `8dc75e3`：缺失 / null 单独判为**畸形**（`E_PROTO-08`）并保留旧值 |
| F5 | 🟠 中危 | **「crc 算错拒绝」只由 `frame.js` 的单元级用例守** —— M-05/M-10 的夹具**根本不携带 crc**，全部走「缺失放行」分支。实测：让 `decodeFrame` 跳过 crc 校验 → **200 条里只掉 1 条** | ✅ `36adf71`：出站夹具全改走 `encodeFrame`（真 crc），并补**集成级**用例（crc 算错 / 载荷被篡改 → 适配器**不下发**） |
| F6 | 🟡 低危 | **mock 内核的入站帧 crc 校验完全没有用例守** —— 实测：摘掉 mock 的入站 `decodeFrame` → **200/200 全绿** | ✅ `36adf71`：补 2 条用例（入站 crc 算错 / 载荷被篡改 → **不产生任何回复**，命令未执行） |
| F7 | 🟡 低危 | `m04_session.js` 的注释在 v2 之后**事实错误**（「契约没有 `negotiate` 变体」） | ✅ `e48c2d9`：只改注释（前端**尚未**调用 `Negotiate`，规格 §11 未要求） |
| F8 | 🟡 低危 | `ps()` 对 ≥ 1e21 产出**指数记法** → `psToNumber` 抛 `E_PROTO-08`（形态非法），但 1e21 > 2⁵³，按 §2.4 应是 `E_PROTO-04`（输入超限） | ✅ `8dc75e3`：`ps()` 对 BigInt / 非安全整数走 BigInt 路径产出**纯十进制** |
| F9 | ⚪ 观察 | `decodeFrame` 会拒掉 `FRAME_TYPE.Resume` 帧（**非本批引入**；`resumeBySeq` 本就未实现，台账 D-47） | ⏳ 随 D-47 |
| F10 | ⚪ 观察 | crc 比对**大小写敏感**；显式 `crc: null` 被当成「没有 crc」→ 放行 | ⏳ 随 P1 帧协议定稿 |
| F11 | ⚪ 观察 | **规格 §2.2 的字段表本身漏了一个字段**（`SimStateDelta::t_ps`），且类型名写成了 `ScheduledEvent` / `EventQueueEntry`（实际是 `SchedEventDto` / `QueuedEventDto`） | ✅ `c3973d4`：规格 §2.2 字段表补全（8 → **9** 个字段）+ 类型名订正 |
| F12 | ⚪ 观察 | M-10 **丢弃** `SimStateDelta.t_ps`（真内核从不产生该事件 → 今天无活影响） | ⏳ 随 M-16 / D-50 |
| **B-6** | ⚪ 但**确认存在静默假绿** | **`tools/check-front.sh` 在 Rust 源码缺失时静默假绿** —— 六条对拍用例带 `{ skip: !hasRust }`，`node --test` 把 skipped 当「没失败」，**退出码仍是 0**。实测：移走 `crates/` → **5 条 SKIP、退出码 0**；而 TAP 里那个套件行**仍然显示 `ok`** | ✅ `5944c5d`：去掉 `skip` 改 `requireRustSource()` **硬断言**。实测：移走 `crates/` → **6 条红、0 skipped、退出码 1** |

**V-B 确认无问题的地方**：B-2 数值策略的 JS 侧（4 个变异全红 · 2⁵³ **收**、2⁵³+1 **拒** · 数字形态**接受**且有专门用例）·
`psToNumber` 抛错后的调用方行为（**保留旧值**、**不产生 NaN**、**不倒退**）· B-3 crc 的算法 / 覆盖 / 表示 / 生产 / 消费 / 兼容
（CRC-32 标准向量 `0xcbf43926` 钉住 · **真递归**排序键 · **数组保序**）· B-1 镜像的变体名 / 形状 / 字段名**逐字一致**·
B-5 mock 与真内核的六条命令语义**逐条一致**（唯一实质分歧是 F1 的 `cap_bits`）·
**帧的两端都在 JS 里**（`encodeFrame` / `decodeFrame` 的所有生产方与消费方都走 `frame.js` 同一处，**没有绕过点**）。

### 3.8.4 V-C 报告（文档一致性 + 数字口径）—— 3 高危 + 7 中危 + 5 低危 + 5 观察

| # | 严重度 | 发现 | 闭环 |
| --- | --- | --- | --- |
| H-1 | 🔴 高危 | **本批 LLD 把规格明文撤回的 `count = 0` 语义写了回来** —— M-10 LLD 的正文、伪码（`return NONE`）与总纲 §2.5.3 都写「不回复」；而**同批的测试**（`wasm-smoke` 断言「回复空区间，**而不是不回复**」）与规格正面相撞 | ✅ `e12fc86` |
| H-2 | 🔴 高危 | `PROGRESS.md` v0.17 段宣称当前树「**编译不过**」「前端镜像仍是 v1」—— 实测都相反（**文档批是在自己分支的旧快照上写的**，合树后没回头修） | ✅ `cccdbf2` |
| H-3 | 🔴 高危 | `STAGE-1_Wasm_Bridge.md`（**冻结**文档）仍写「契约 13 个族，**没有 `PROTO`**」+「契约层冻结」+ JSON 示例 `schema_version: 1` | ✅ `e12fc86`：族清单 13 → **14**、删「冻结」句、示例改 v2、补 **v1→v2 数值对照表**、帧形状补 `crc` |
| M-1 | 🟠 中危 | 总纲 §2.5.2 把超限抛错归到 `E_PROTO-02` —— 规格**明文说这是错的**（`E_PROTO-02` 的原义是「未知字段 → 仅日志」） | ✅ `e12fc86` |
| M-2 | 🟠 中危 | 台账 §0.1 数字表 / 统计口径 / 合计行 / 头基线 **一行未重算** | ✅ `7dbf55c`（并在收口时按**最终树**再算一次） |
| M-3 | 🟠 中危 | 台账 D-32 / D-34 的「判别力」栏仍写「当前门禁 4 条红」「JS 镜像未同步」—— 实测**已全绿** | ✅ `7dbf55c`：改为「**当时** 4 条红 → 现全绿」，保留论证价值**不冒充现状** |
| M-4 | 🟠 中危 | D-69 的「诚实标注 / 待做」**已被自己满足**（`every_family_tag_is_pinned` 正是那条表驱动单测，14 族全覆盖）却标成 false | ✅ `7dbf55c`：待做清空 |
| M-5 | 🟠 中危 | M-07 LLD §1.3 仍把 CRC 列为「(P1) 预留项」，与同文档 §2.1.2 / §6.2 自相矛盾 | ✅ `e12fc86`：改标「**已实现**」 |
| M-6 | 🟠 中危 | `crates/contract/src/protocol.rs` 的 `CommandPayload::Continue` / `Pause` 的 doc 仍是 D-57 撤掉的原句 | ✅ `5b89c9a` |
| M-7 | 🟠 中危 | `AGENTS.md` **完全没跟上本批**（门禁条数 / kernel_host 行数 / D-57 状态 / 模块表 / **无 v0.17 段**） | ✅ `cccdbf2`（+ 收口时的数字终检） |
| L-1 | 🟢 低危 | `PROGRESS.md` 全文未提 **D-69** | ✅ `cccdbf2` |
| L-2 | 🟢 低危 | 台账合计行**缺「v0.17 契约 v2 批」行** | ✅ `7dbf55c`：290/242/48 → **297/245/52** + 新增本批行 |
| L-3 | 🟢 低危 | `FRONTEND_ServiceLayer_Review.md` 的 topic 已改名，且「寄存器只能从边沿刨」的前提已被 v2 的 `Registers` 事件推翻 | ✅ `e12fc86` |
| L-4 | 🟢 低危 | STAGE-1 的 JSON 示例 `schema_version: 1` | ✅ `e12fc86` |
| L-5 | 🟢 低危 | `m16_interrupt/mod.rs` 的「量子边界」措辞与 D-57 撤回的旧措辞同型 | ⏳ **登记为 D-74** |
| O-1 | ⚪ 观察 | M-10 LLD 的 `<style>` **永不闭合**（开 2 闭 1）→ **第 260 行到 EOF 被吞成 CSS**，那段 SVG 流程图不渲染（**既有**，且是 23 份 LLD 里**唯一**失衡的一份） | ⏳ **登记为 D-71** |
| O-2 | ⚪ 观察 | `M-02_ViewComponents.html` 的 1 个 U+FFFD 是**有意**的示意图 → **不是缺陷** | ✅ 不动（维持既有裁决） |
| O-3 | ⚪ 观察 | HTML / GFM 表格列数全部与表头一致（**0 问题**） | ✅ — |
| O-4 | ⚪ 观察 | **`model::WaveformDelta`（live 批次形）没有任何一处被写成「死代码」或「已废弃」** —— 四处措辞都正确，代码侧确有实现与测试（`push_delta_*` 3 条） | ✅ **完全通过**（V-C 特意反查每个「死代码」命中，无一条指向它） |
| O-5 | ⚪ 观察 | M-14 行数 2 375 vs 实测 2 394（**既有偏差**，非本批引入） | ✅ 随 §0.1 重算订正 |

**V-C 确认无问题的地方**：台账 **登记位置与严重度合理** · 闭环的「证据 / 判别力」三栏**全部属实**（逐条回源码核对）·
**C-5「已知但刻意不做」的描述准确** —— **D-36 / D-41 / D-64 三条均未被误标闭环**。

### 3.8.5 闭环对照表

| 检视 | 发现 | 闭环 | 留账 |
| --- | --- | --- | --- |
| **V-A** | **11**（2 高危 + 4 中危 + 2 低危 + 2 观察） | **10** | **1**（观察 10 → **D-75**） |
| **V-B** | **13**（2 高危 + 3 中危 + 2 低危 + 4 观察 + B-6） | **12** | **3**（F9/F10/F12 → 随 D-47 / P1 / D-50） |
| **V-C** | **20**（3 高危 + 7 中危 + 5 低危 + 5 观察） | **16** | **4**（L-5 → **D-74** · O-1 → **D-71** · O-5 → 已订正 · O-2 → 维持既有裁决） |
| **合计** | **44** | **38** | **6**（新增台账 **D-74 / D-75**，其余随既有台账） |

**新增台账**：**D-74**（M-16「量子边界」措辞）· **D-75**（内核严格等号 vs JS 前向兼容不对称）。
另由本批的 V-A 闭环了 **D-73**（D-57 判据自指）。

### 3.8.6 真机复验与反向变异自检

**真机复验**（`tools/browser-verify.mjs`，**真 Chrome + 真 WASM 产物**）—— ✅ **14 项断言全过**：
落地页挂载（`#app` 有子节点 · 面板 7 个 · 当前视图有可读标题）· 切到 `#/regmem` **渲染出 32 个
寄存器格**（正文非空，**不是白屏**）· **真内核执行 demo 固件 `x1 = 42`** · PC 前进到
`0x00000004` · 波形视图列出 `GPIO0.OUT` **且带 1 条边沿** · 控制台无错。

> ⚠️ **真机断言必须断言值**——只断言「非空」抓不到 D-45 这一类（PC 前进、x1 不动）。
> 且**要反向证明它有判别力**：把断言弱化成「非空」再改坏固件，会**全绿**（V-B 实测）。

**反向变异自检**（跨层 21 条 → 全部按期望变红）：

| 轮次 | 条数 | 结果 |
| --- | --- | --- |
| 第 1 轮 `/tmp/probe/c2_mutverify.py` | 21 | 报 15/21 → **逐条核对红集**后确认：5 条是**期望名写错**、1 条是**锚点不匹配** → **真实缺口 0** |
| 第 2 轮 `/tmp/probe/c2_mutverify2.py`（订正后的 6 条） | 6 | **6/6 全红** |
| 第 3 轮 `/tmp/probe/c2_mutverify3.py`（验新增的 `Continue` 用例） | 2 | **暴露 D-73**（自指断言）→ 修复后重跑 **两条都红** |

> ⚠️ **期望集合用 AND 语义逐条核对**——OR 语义只能证明「某条期望的用例红了」，
> **不能**证明「每条期望各自都守得住」；**完整红集**要一起记下来。

### 3.8.7 本轮方法论沉淀

1. ⚠️ **断言不得自指**。D-57 的判据用的是**实现自己用的** `DEFAULT_INSTRUCTION_BUDGET` 常量，
   把那个常量改成量子配额（1 000）断言**照样全绿** → 「**预算耗尽**」与「**停在量子边界**」
   在断言下**无法分辨**。判别式必须落在**与实现无关**的量上（「跑过**不止一个**量子」）。
   **这是本轮最贵的一条**——它同时也是 V-A 报的「版本号无用例钉住」的**同型**：
   断言用的常量若就是实现自己用的那个常量，「改行为」与「没改行为」无法分辨。
2. ⚠️ **「用例都在」≠「用得着」**。V-A 实测：删掉 `SimState.virtual_time_ps` 的 serde 属性，
   **全工作区 631 条全绿**——因为 `u64_decimal` 的**反序列化**同时接受字符串与数字，
   被悄悄改回去的字段**不会报错**，只会**静默退回** v1 的线上形态。
   **落点要落在「线上表示」上，而不是「字段存在」。**
3. ⚠️ **单一来源要落在编译期，不是运行期**。`ErrorFamily` 的族清单原先是 enum + 两份**手工**
   列表，完备性断言只比**长度** → 新增变体而忘登记，用例**照样绿**。
   改成 `macro_rules!` 展开 enum + `as_str()` + `ALL_FAMILIES` 后，**漏登记 → 编译错误**。
4. ⚠️ **台账声称的判别力要自己去验**。D-40 声称「防的是把判据写成『队列空即空』」——
   但夹具带着 3 条事件，两种判据**同值**，那条声称**不成立**。
   **台账里的「判别力」栏本身也是需要被检视的断言。**
5. ⚠️ **「没跑」绝不能与「跑过了」不可区分**。`tools/check-front.sh` 在 Rust 源码缺失时
   **5 条 SKIP、退出码 0**——跨层对拍门禁变成一个**什么都不查却报绿**的静默空转。
   口径与 `tools/wasm-smoke.mjs` 的 **127** 一致。
6. ⚠️ **数字口径要从**最终树**重算**。本轮 W6 的数字是从 `5b89c9a` 基线树算的
   （workspace 632 / 前端 200），而合入「契约层第二轮修复」（+5 条）与「前端闭环 W5」（+8 用例）
   之后就成了 **636 / 208**。**并行分支的数字在合并后必然要再算一遍**——
   这不是「谁写错了」，是并行开发的**必然代价**，必须有专门的一遍终检。
7. ⚠️ **「文档批写在自己分支的旧快照上」是一个独立的失效模式**。V-C 的 H-2 抓到
   `PROGRESS.md` 宣称当前树「**编译不过**」——因为文档分支从 `5b89c9a` 拉出，
   看不到同时合入的内核 / 前端批。**文档同步必须以「最终树」为准，且要在合并后再核一遍。**
8. ⚠️ **「规格自己写错的措辞」会被下游文档放大**。规格 §5 逐字撤回了「`count = 0` → 不回复」，
   但同批的**文档同步**把那句话说回了**两份 LLD**，其中一份还把它写成了**通过准则**——
   照着实现会做出与规格相反的行为。**规格订正必须追到「通过准则」那一层。**
9. ⚠️ **并行切分按**文件所有权**切，实测 0 merge 冲突**。本轮三路施工
   （`crates/contract/**` / `crates/kernel/**` / `front/**` / `docs/**`）**零交集**，
   三个 `--no-ff` 合并**零冲突**；检视轮的两路修复（前端 / 文档）同样零交集。

## §3.9 会话级快照载荷检视轮（2026-10-21 · D-36 / D-37 / D-41）

**分支**：`feature/session-snapshot`（基线 `main` = `14bf6bc`）· **冻结规格**：`docs/detailed-design/STAGE-3_Session_Snapshot.md`

本轮闭合台账 §2.2 优先级表里**唯一未完成的条目**（**5b**）：**D-36 + D-37 + D-41
「快照落盘对齐 LLD」**。核心命题一句话：**落盘一个会话的完整状态，还原出的是一个
「等价会话」**——不只是「还原了一个引擎」。

> **三处裁决**（规格 §1.2）：**A** LLD 的结构清单**漏列**了 M-12/M-17 段（而它自己的
> 正文早就要求了）→ 补 `m12Engine` 段；**B** CPU 寄存器**已经在** `m12Engine.state` 里
> → `m13State` **只**承载内存页，**寄存器不重复存**（否则是两个真相）；**C** M-16 有真实
> 状态 → 补第 6 段 `m16Intc`（⚠️ 该段在**生产路径**上恒为空，见 §3.9.7 的第 4 条）。

### 3.9.1 四路检视 + 一路施工的范围划分

| 角色 | 范围 | 判决 |
| --- | --- | --- |
| **Lane A**（施工） | M-14 / M-15 / M-16 的状态导出与载入接口 | ✅ 26/26 反向变异全红 |
| **Lane B**（施工） | M-20 组装会话载荷 + M-23 改吃新结构 + `InMemoryDataStore` | ✅ 16/16 全红 |
| **Lane C**（施工） | 内核宿主接线 + 端到端等价会话用例 | ✅ 11/11 全红 |
| **Lane D**（施工） | M-20 / M-23 LLD §2.1.1 + 总纲表 12 + 四份模块 LLD 的 §9.x | ✅ 两份 LLD 逐字一致（父会话独立复核） |
| **V-C**（检视） | 文档一致性（规格 / 两份 LLD / 总纲 / 契约层 doc） | **FAIL**——**1 阻断 + 3 高危 + 8 中危 + 7 低危**，**全部是文档侧的事实性错误** |
| **V-A**（检视） | Rust 内核方向 | **PARTIAL**——**1 高危 + 2 中危 + 3 低危**；合法载荷主链路判 **PASS**，**拒绝路径有洞** |
| **V-B**（检视） | 测试判别力与变异方法论（**独立第三方**） | **PARTIAL**——**1 阻断 + 3 高危 + 5 中危 + 3 低危**；跑 **70 条变异**，存活 17 条 |
| **修复 lane**（施工） | V-A 的 F-1 / F-2 / F-4 / F-5 / F-6（F-3 **只标注**） | ✅ 4 个 commit，**9/9** 自验变异全红 |
| **Lane E**（施工） | V-B 的 H-1 / H-2 / C-1 / C-2（**只动契约层 `session.rs` 的测试**） | ✅ **11/11** 变异按期望（含一条 **0 红**的对照） |
| **父会话**（施工） | V-B 的 B-1 / H-3 / C-3 / C-4 / D-2 + F-6 留账 | ✅ **7/7** 变异按期望（含一条 **0 红**的对照） |

> **并行切分按**文件所有权**切，本轮实测 0 merge 冲突**：四个施工 lane 的文件零交集，
> 五个 `--no-ff` 合并**零冲突**。⚠️ 但**基线会分叉**——Lane B 的 worktree 停在
> `26f6757`，读到的规格 §3.5 还是旧签名（收「`&Bus` / `&PeripheralRegistry` /
> `&InterruptController`」），据此**误判**了「实现与规格不符」。**worktree 基线落后
> = 子会话读到旧规格**——见 §3.9.7 的第 5 条。

### 3.9.2 V-C 报告（文档一致性）—— 1 阻断 + 3 高危 + 8 中危 + 7 低危

**全部是文档侧的事实性错误**（文档与冻结规格 / 与实现对不上）。实现侧**未发现**
运行时功能缺陷。**全部 16 项 + 3 项新发现已逐条修完**（文档 lane `feature/ss-docs2`）。

| # | 严重度 | 发现 | 闭环 |
| --- | --- | --- | --- |
| 1 | 🔴 **阻断** | **M-14 §9.3 把 ROM 的恢复写成「恢复时由固件重新装载」**——与规格 §3.1/§3.2 **和**实现**都相反**。规格明写「恢复链里**没有**「重装固件」这一步」→ `load_map` 重建 ROM 时**原地保留内容**、`base`/`size` 不符报 `E_BUS-12`。按 §9.3 实现的人会写出「重建即清零」= **固件被抹掉** = 会话必然发散 | ✅ 文档 lane：`48135ee` 改 §9.3 注记 + `load_map` 表行 |
| 2 | 🟠 高危 | **`cpMeta.version` 两套语义**——LLD 写「M-23 迁移链版本（v3）」，契约层是「会话载荷版本（= 1）」且 `validate()` **拒** `!= 1`。按 LLD 造出的载荷**会被契约层拒收**；M-23 的 `migrate()` 还会把 `version` 写回 3 → 再次被拒。本批已区分三个版本号，LLD 又引入**第四个**并与 `cpMeta.version` 混为一谈 | ✅ 文档 lane：`da28cc3` —— `cpMeta.version` 写死 = 会话载荷版本；**存储记录自己的** `schemaVersion`（v3，**不在载荷里**）才是迁移触发器；**迁移链不得改写 `cpMeta.version`** |
| 3 | 🟠 高危 | **M-20 §9.1.1 端口表把 M-13 列为「寄存器/内存状态序列化」，且**完全没有 M-16**——与 M-13 自己的 §9.4（「**不提供**接口……**这不是遗漏**」）直接打架；段 5 的 M-16 依赖也没进表 | ✅ 文档 lane：`da28cc3` —— M-13 行改写 + 补 M-16 行 |
| 4 | 🟠 高危 | **M-16 §9.4 对段 5 为空的归因写错 + 夸大「raise/lower 有生产者」**——只提了「CSR 管道未接线」，漏了「**生产代码里没有任何调用方**把 M-15 的 pending IRQ 升上来」。读者会得出「等 CSR 接上线，段 5 就 populated」的**错误**结论 | ✅ 父会话 + 文档 lane：Lane C 落地后**重写**（宿主**已持有**控制器并真的委派；两条成因并列；`raise_irq`/`lower_irq` 是**测试辅助入口**，不构成生产路径）。规格 §1.2 裁决 C 同步（`8727031`） |
| 5 | 🟡 中危 | **两份 LLD §2.1.1 的五段类型名不是契约层真名**（`Checkpoint` / `M13State` / `M14Map` / `M15Periph` / `M16Intc`），而页眉却声称「段类型名与契约层一致」 | ✅ 文档 lane：`da28cc3` —— 五段全改真名（`CheckpointMetaDto` / `M13StateDto` / `M14MapDto` / `M15PeriphDto` / `M16IntcDto`），注释列重新对齐到第 32 列 |
| 6 | 🟡 中危 | **M-14 §9.3 的 `load_map` 行只列 `E_BUS-03/04`**，漏掉本批实际会报的 `E_BUS-10`（未知 kind）/ **`E_BUS-12`（ROM 拓扑不符）** / `E_BUS-06` / `E_BUS-09`——尤其 `E_BUS-12` 是第 1 条那条不变量的**唯一**守卫 | ✅ 文档 lane：`48135ee` —— 六个码逐个列全 + 注明新开 |
| 7 | 🟡 中危 | **本批新增的 5 个错误码在 LLD 错误码表里一个都没有**；M-20 §6.1 仍写「**无新增码**」 | ✅ 文档 lane：`da28cc3` / `10d2850` —— §6.1 补 `E_SNAP-06` / `E_BUS-09~12` / `E_PER-04/05`；「无新增码」限定为「v1.0 当时未新增」 |
| 8 | 🟡 中危 | **M-15 §9.4 漏了宿主真正用的入口**——只列 `PeripheralRegistry` 的两个方法，但宿主把外设**直接绑在总线上**、**不持有 registry**；实现里 registry 的两个方法只是**委托给自由函数**。按 §9.4 接线的人**走不通** | ✅ 文档 lane：`10d2850` —— 补 `collect_session_state` / `restore_session_states` 两行 + 注明「宿主走总线绑定路径，registry 是等价镜像」 |
| 9 | 🟡 中危 | **规格 §3.1 未收录 `PeripheralSessionState` 枚举与 `Peripheral` trait 的两个加法方法**；§3.3 把 `E_PER-05` 称为「**既有码**」（实为本轮新开） | ✅ 父会话：`20fe772` —— 补枚举 + 两个方法（含「加法 → `SCHEMA_VERSION` 不动」）；「既有码」→「本轮新开」 |
| 10 | 🟡 中危 | **定时器的粘滞 `pending_irq` 不在载荷里，但 M-15 §9.4 的结论句给了更强承诺**（「没有它们就还原不了一个会话」） | ✅ 文档 lane：`10d2850` —— 结论句限定到「段 4 覆盖的这些量」+ 补披露「恢复后若**零步**就再捕获，粘滞位不等价」 |
| 11 | 🟡 中危 | **「会话快照**没有协议入口**」这条边界在任何一份 LLD 里都没有**（只有规格 §7 有）——而 M-20 §3.7 的 UX 线框画的是**前端**「检查点管理」面板 | ✅ 文档 lane：`da28cc3` —— M-20 / M-23 各加一句边界声明（「宿主内部 API，前端不可达；命令变体要 bump `SCHEMA_VERSION`」） |
| 12 | 🟡 中危 | **总纲表 12 的 `SimState` 行仍写「契约层现行实现为子集，见台账 D-41」**——D-41 闭环后这句**陈旧**，且会把读者引向「应该给 `SimState` 补字段」的错误方向 | ✅ 文档 lane：`39961e9` —— 改「**引擎级**状态；内存页 / 外设状态**按设计不进** `SimState`」（裁决 B） |
| 13 | 🟡 低危 | 其余 5 项：M-14 §9.1.1 交叉引用「见 9.1」→「**9.3**」· M-20 §9.1.1 四个「契约锚点**待**双侧核对」标记已过期 · M-14 §9.3 未提「外设实例被借用时 `target` 退化为 `periph#<token>`」· M-20 §1.3 MVP 范围未提「会话级快照载荷」· 总纲表 76 未列 M-14/15/16 三个 driven 依赖 | ✅ 文档 lane：`48135ee` / `da28cc3` / `39961e9` 逐条 |

**V-C 的第 3 项新发现（本轮最要紧的「一号两事」）**：契约层本批新开的 `E_PER_04` /
`E_PER_05` 与 **M-15 LLD §6.1 早已分配**给另外两个场景的号**撞了**（`E_PER-04` = 端口未连接
（P1）· `E_PER-05` = 外设运行时错误（评审 15-6））。父会话核实：**M-15 那两个号在实现里
从未落地**（外设运行时错误实际用**场景码 2**；端口未连接是 P1 未实现），而契约层新开的
两个**有实现** → **让「没落地的」给「已落地的」让号，零行为变化**：
`E_PER-04` 不支持会话状态回灌 · `E_PER-05` 外设集合不匹配 · `E_PER-06` 外设运行时错误 ·
`E_PER-07` 端口未连接（P1）。✅ 文档 lane `10d2850` / `39961e9` 同步 M-15 §6.1/§6.2/§7.1/§9.1.1
与总纲表 16。⚠️ 「LLD 写 06、实现用 02」这个**既有的**文档/实现不一致**不是本批造成的**，
属台账 **D-70** 的范畴。

### 3.9.3 V-A 报告（Rust 内核方向）—— 1 高危 + 2 中危 + 3 低危 · 判决 **PARTIAL**

**检视树**：`feature/session-snapshot` @ `ab4e710` · **只读**（未改仓库任何文件）。
**数字由检视员自己重跑**（契约 lib 57 · 内核 lib 582 · 集成 91 · golden runner 11 = **741 条全绿**），
与派发书基线一致。

**判决 `PARTIAL` 的口径**：14 项验收里 **13 项完全满足**、**第 4 项带限定**（`validate()` 的
9 类拒绝齐备，但**外设载荷的内部自洽性**（`gpio.irq_lines` 长度）**不在校验面内**）+
黄金对拍 / WASM 链路因**磁盘满 + 外部依赖**未验证。**合法载荷的主链路（六段采集 → 落盘 →
还原 → 等价会话）判 PASS**；**拒绝路径有洞** = F-1。

| # | 严重度 | 发现 | 处置 |
| --- | --- | --- | --- |
| **F-1** | 🔴 **高危** | **「先校验后写入」只守住了一半触发条件**——`restore_session_states` 的阶段 2/3 只查「id 对得上 / kind 对不对」，**不查外设状态自身的完整性**；GPIO 的 `irq_lines` 长度检查**写在写入路径里**（`gpio.rs:319-329`）。于是阶段 4 依次写 `uarts` → `gpios` → `timers`，**写完 UART 之后**才因 `irq_lines.len() != 32` 报 `E_PER-04` → **UART 的状态已经被改写了**，函数返回 `Err`。**宿主级后果更重**：`load_map` / `load_pages` 在第 3 步失败**之前**就写完了 → 恢复失败后会话变成「**内存来自载荷、CPU 寄存器来自恢复前、外设来自恢复前**」的拼接体，调用方只拿到一个 `Err`。直接违反冻结规格 §5 的核心不变式（「**半个应用完的快照比被拒绝的快照更糟**」）**和** `m15_peripheral/mod.rs:252-256` 的模块 doc 承诺（「A rejected payload leaves every peripheral **exactly as it was**」——**目前是假的**） | ✅ 已修（修复 lane，规格 §3.3 两阶段回灌 + 契约层 `validate_session_state()`）——见 §3.9.5 闭环对照表 |
| **F-2** | 🟡 中危 | **同一个失败条件（INTC `RefCell` 争用）在同一文件里被处理成两种结果**——回灌路径 `load_intc_state` 争用 → `Err(E_INT_01)`；拉起路径 `HostSinks::delegate` 争用 → **记一次失败 + 返回 `Ok(())`**（引擎因此以为这条 IRQ 拉起来了，其实被丢了）。该行为被现有用例**显式钉住**，所以是**有意设计**——但「同一个量两种真相」的形状已经写进了契约端口，下一批接 M-15 pending IRQ（**D-63 / D-64 / D-72 / D-74**）的人会直接踩上 | ✅ 已修（修复 lane）：`delegate` 改返回 `Err(E_INT_01_CONTROLLER_BUSY)`，与 `load_intc_state` **一致**（失败计数器保留用于诊断） |
| **F-3** | 🟡 中危 | **采集端争用时静默丢掉整段段 5**，而 `capture_session` 的返回类型 `SessionSnapshotDto`（**不是 `Result`**）**无法表达「这份载荷不完整」**——采出来的段 5 是空的，载荷里**没有任何字段**表明这一段没采到，调用方可以照常 `put_snapshot` 落盘，将来还原时那几条 IRQ 行**无声消失** | ⏳ **本批不改行为**——改 `capture_session` 签名会波及整条链（契约 → M-20 → 宿主 → 端到端），而争用在**生产路径上不可达**（单线程、无重入）。✅ 已在 M-16 LLD §9.4 **如实标注** + **台账留账**（§3.9.6） |
| **F-4** | 🟡 低危 | **`cp_meta.t_virt` 与 `m12_engine.state.virtual_time_ps` 是同一个量的两个落点，宿主既不填也不校验**——调用方完全可以采出「检查点时间 = 0、引擎时间 = 12345 ps」的载荷，**没有任何一处会报错**（冻结规格 §5 也没列这项校验 → 是**规格层面**的缺口，不是实现偏离） | ✅ 已修（修复 lane）：宿主**代填** `cp_meta.t_virt = engine.state.virtual_time_ps`（不改签名；调用方给的值被覆盖，doc 写明） |
| **F-5** | 🟡 低危 | **断点表 / 观察点表 / 探针环 / 波形环不在载荷里**（规格冻结如此，**不算缺陷**）——但后果是「**还原出的会话等价吗**」取决于**还原进哪个宿主**（同宿主→断点还在；全新宿主→断点没了），而 `restore_session` 的文档**只字未提** | ✅ 已修（修复 lane）：**只改文档**——`restore_session` doc 写明这四样**不在**载荷里（同宿主保留 / 跨宿主丢失） |
| **F-6** | 🟡 低危 | **`load_map` 里本批新增的错误路径仍用裸数字场景码**（`6` / `4` / `4` / `9` / `3` / `3`）——同函数下面三行却是命名常量 `E_BUS_10/11/12`。注：这三个既有码（3/4/6/9）在 `map_region` / `prepare_bytes` 里**也是字面量** → 是**本批新代码沿用了旧风格**，不是本批引入的重复定义 | ✅ 已修（修复 lane）：**只改 `load_map` 那六处**换命名常量（`map_region` 等既有字面量不动） |

**F-1 的两条可达路径**（都能通过 `SessionSnapshotDto::validate()` → 能被
`InMemoryDataStore::put_snapshot` 的落盘前校验**放行并持久化** → 到恢复时才炸）：
① `gpio.irq_lines` 条数 ≠ 32；② **同一个 id 同时出现在两种 kind 里**（契约层只在
**同一类型内**查重复）。

**为什么现有用例没抓住**（本项目的老坑：「**用例都在 ≠ 用得着**」）：
`m15_peripheral/tests.rs:951-989`（`registry_load_state_writes_nothing_when_one_id_is_unknown`）
钉的「失败时一个字都不许写」，触发条件是**未知 id**——它在**阶段 2** 就被抓住，
**压根走不到写入阶段**；`gpio.rs:319-329` 那条只验「单个外设会拒绝」，**不验「拒绝时别的外设没被写」**；
宿主级那条同样只用「未知外设 id」一个触发条件。
→ **守卫只覆盖了写入前的那条路径，写入中失败的那条路径没有守卫，而它恰恰是这条不变量的反例。**

> **本轮方法论的新条目**（沉淀见 §3.9.7）：**「先校验后写入」的不变式，必须区分
> 「**校验点在哪里**」与「**写入点在哪里**」**。把校验放在写入路径**内部**（哪怕紧挨着写入），
> 对**多对象**回灌来说**只守住了最后一个对象**——前面那些对象已经被写了。
> **判据**：回灌 N 个对象的不变式「失败 = 什么都没变」，判别式必须落在
> **「前一个对象的状态有没有被改」** 上，**不能**只落在「最后一个对象报对了错」上。

**V-A 明确核过「没报为缺陷」的 12 项边界**（逐条核对，非默认放过）：协议入口
（`SaveSession` / `LoadSession`）· M-16 的 CSR 管道 · M-15 pending IRQ → `intc.raise()`
的生产链路 · ROM 内容进快照 · 压缩快照 / `DebugMeta` / M-11 符号表 · 定时器粘滞
`pending_irq` · M-13 会话状态接口 · `pending_writes` 落点 · 恢复顺序 · `load_map` 清零
RAM → `load_pages` 紧随（中间态不可观测）· M-16 替换 vs M-15 逐字段覆盖 · 文档侧问题（交 V-C）。

### 3.9.4 V-B 报告（测试判别力与变异方法论）—— 1 阻断 + 3 高危 + 5 中危 + 3 低危 · 判决 **PARTIAL**

**检视树**：`ab4e710`（派发基线）与 `f5ab1d7`（修复 lane 合并后）——**两条基线都跑过**，
下文每条发现都标注它在哪棵树上成立。**沙箱**：`/tmp/wt/ss-k3`（git worktree），
**主仓库全程只读**。**共跑 70 条变异**（含 2 条语义不变的对照、1 条复合、6 条夹具探针）。

> ⚠️ **父会话独立复核**：V-B 的沙箱在收卷前被同步到 `f5ab1d7`，
> 父会话核对 `git diff 590848d HEAD -- crates/` —— **只有父会话后加的
> 契约层测试模块（101 行）不同**，`crates/kernel/**` **逐字节一致**
> → **变异结论对最终树有效**。

**判决 `PARTIAL` 的口径**：核心链路（外设状态 / RAM 页 / 拓扑 / IRQ / 段 0 / 段 2
的还原）判别力**扎实**，M-14/15/16/20/23 单元层**无可挑剔**；但有 **1 项阻断级**
（端到端第 ⑦ 项零判别力，且**注释与实测相反**）、**3 项高危**（契约层 3 条
`validate()` 规则无守、层 1 段 2 的断言被别的机制**代偿**）、**5 项中危**
（含 2 条**恒真式**断言与 1 条**方法论洞**）必须闭环。
**`PARTIAL` 而非 `FAIL` 的依据**：所有存活变异在**别处**都有覆盖
（队列 / 插入序 / 预算在 `m12_m17_snapshot_dto.rs`、ROM / 定时器 / UART 在
M-14 / M-15 单测、外设 id 重号已被 `adaf533` 修掉）——缺的是**本批自己的断言**
对这些量的牙齿与**注释与事实的一致性**，不是能力的缺失。

| # | 严重度 | 发现 | 闭环 |
| --- | --- | --- | --- |
| **B-1** | 🔴 **阻断** | **端到端「等价会话」的第 ⑦ 项（队列 / 插入序 / 指令预算）零判别力，而文件注释宣称它「有牙齿」**。变异 A4（队列不还原）→ **9 red**、A5（`next_seq` 不还原）→ **1 red**、A6（预算不还原）→ **1 red**——**但那些红全在老批次文件** `crates/kernel/tests/m12_m17_snapshot_dto.rs`，**本批的 e2e / 宿主用例一条没红**。**根因**：① 两条路径在**采集点**的队列内容逐字相同；② 第 ⑦ 项的终态比对发生在**再跑一轮之后**，那时队列已被运行**重新派生**；③ 夹具自检 `assert_eq!(fa.queue.len(), 1)` 与 `assert!(fa.next_seq > 0)` 是**存在性断言**——恰恰是本项目文件头自己禁止的（「不许用『非空』『长度 > 0』当断言」）。⚠️ **文件自己写的是反的**（`session_snapshot.rs:386`：「`Continue` 走时间驱动路径…队列里**真的**有一件待派发的事件——第 ⑦ 项这才有牙齿」） | ✅ 已修（父会话）：**宿主层 1 的漂移扩到六处**——① `run_continue`（换掉队列内容 + 推进插入序 + 推进虚拟时间）② `set_instruction_budget(4321)`（**预算**必须真的变过，否则「不还原预算」根本看不出来）③ `write_ram`（段 2）④ `map_ram`（段 3）⑤ `write_gpio_out`（段 4）⑥ `lower_irq`（段 5）。**6 条变异**（K1~K5 + 对照）逐条验证有牙齿 |
| **H-1** | 🟠 高危 | **契约层「内存页**不重叠**」规则无判别力**（规格 §5 第 6 类明写「按 base 升序**且不重叠**」）。变异 G5：把 `prev_page_end = Some(end)` 改成 `Some(p.base)`（= 只查升序）→ **0 red**。现有用例 `unsorted_pages_are_rejected` 只造了「**乱序**」，造不出「**升序但重叠**」 | ✅ 已修（Lane E）：补「升序但重叠」用例 + 反向变异验证 |
| **H-2** | 🟠 高危 | **契约层两条区域子规则无判别力**：「`size == 0`」与「`base + size` 越过 u32」。变异 G3 / G4 各 **0 red**。⚠️ 这两条**不在**规格 §5 的清单里 → **实现比规格严**，而「严」这件事**既没被记录、也没被钉住** | ✅ 已修（Lane E 补用例 + **父会话把它写进规格 §5 第 5 项**）——见 §3.9.5 闭环表 |
| **H-3** | 🟠 高危 | **层 1 的「段 2（内存页）必须相等」断言，牙齿来自 `load_map` 的清零，**不是** `load_pages`**。复合变异 A15b（`load_map` 保留 RAM 内容 + 宿主跳过 `load_pages`）→ 红的是别的用例，**层 1 的段 2 断言不红**。根因：漂移只动了 IRQ 行、GPIO.OUT 和一块多出来的 RAM 区域，**没动内存内容** → 「双保险互为掩体」 | ✅ 已修（父会话）：漂移里补一处**写 RAM**（落在**总线导出的那几页**里，且放在 `run_continue` **之后**——demo 固件的 `jal x0, 0` 会把自己重跑一遍把 49 写回来） |
| **C-1** | 🟡 中危 | **形状守卫没钉住 4 个结构体的键集**（`m13_state` / `m14_map` / **GPIO** / **定时器**）；**数值策略对新加的未带属性时间字段不设防**。变异 G17（给 `GpioStateDto.outputs` 改名）→ **0 red**；**G22**（给 `GpioStateDto` 加一个**不带**十进制属性的 `last_tick_ps: u64`）→ **0 red**。G22 更重：规格 §4「**6 个**虚拟时间字段都是字符串」对新字段**不设防**——计数守卫只数**带属性**的字段，形状守卫又不钉这个结构体的键集，**两道闸门同时漏** | ✅ 已修（Lane E） |
| **C-2** | 🟡 中危 | **计数守卫 `exactly_six_virtual_time_fields_carry_the_decimal_attribute` 耦合源码排版**。变异 G13：把一个 `#[serde(with = ...)]` 改成**多行**（**语义完全不变**）→ 用例**变红**。即：一次纯格式化就会让门禁红 | ✅ 已修（Lane E） |
| **C-3** | 🟡 中危 | **宿主层 1 的「段 0（检查点元数据）必须相等」是恒真式**。变异 M7b（宿主采集时把 `cp_meta.id` 改成 999）→ 层 1 **不红**。根因：`meta()` 是**常量**函数 → 两次采集拿到同一个常量 → 相等是**恒真的** | ✅ 已修（父会话）：宿主**代填** `cp_meta.t_virt`（= 引擎虚拟时间）→ 漂移里 `run_continue` **推进虚拟时间** → 段 0 **不再**是两个常量相等 |
| **C-4** | 🟡 中危 | **端到端「夹具自检」块断言的是**主断言**，且它宣称的前提**没有被断言**。那两行（`fr.gpr` / `fr.pc`）与主判别式的 ①② **完全重复**，且断的是**还原之后**的状态；注释宣称的「新会话在还原之前是**空的**」**从未被断言** | ✅ 已修（父会话）：改成**在还原之前**真的断言一次空态（`PC = 0` · 32 个 GPR 全 0 · 没有拉起的 IRQ 行），并删掉两行重复断言 |
| **C-5** | 🟡 中危 | **变异自检方法论洞**：M-14/15/16 lane 的 `restore()` = `shutil.copy2(PRISTINE/f, dst)` + `os.utime`，**没有 md5 逐字节校验**；而它的 `pristine` 快照**不是最终树**（还停在 `6eaf859` 之前那一版）。**好消息**：被改坏的只有**常量归属**，**被突变的函数在两版之间逐字节相同** → 结论仍成立，但方法论上有洞。⚠️ 契约层 lane 与宿主 lane **都做了 md5 校验** ✓ | ⏳ **留账**（台账 **D-79**）——已写进 §3.9.7 方法论：还原**必须**逐字节校验 + 快照**必须**取自最终树 |
| **D-1** | 🔵 低危 | **端到端夹具的虚拟时间断言自指实现常量**（`INSTRUCTION_COST_PS`）——改成本 1000→2000 ps，本批端到端 / 宿主用例**一条都不红**。判别式落在**关系**而非**绝对值**；绝对值由 D-07 的用例守着 | ⏳ **留账 + 披露**——已在夹具处**写明**「本批察觉不到指令成本变化；绝对值由 D-07 守」，并注明「**不要**改成字面量（那是换一种形式的自指）」 |
| **D-2** | 🔵 低危 | **M-20 的字段集守卫对字段顺序敏感**。变异 M20d：把 `payload` / `label` 换个顺序（**语义无关**）→ 守卫变红 | ✅ 已修（父会话）：改成**排序后的集合比较**——顺序无关，集合变了照样红 |
| **D-3** | 🔵 低危 | **端到端场景不含 UART / 定时器 / ROM**（demo 内存图 = 低段 RAM 1 MiB + 高段 RAM 4 MiB + GPIO 窗口；**固件被装进低段 RAM 的 `0x0`**，根本没有 ROM 块）。变异 A2（重建 ROM 时清零）→ 只有 2 条 **M-14 单测**红；A8（定时器不还原）→ 只有 2 条 **M-15 单测**红；A18（UART 状态不还原）→ 只有 3 条 **M-15 单测**红 → 规格 §3.2/§3.3 的「ROM 原地保留」「UART/定时器状态」在**端到端等价会话**这一层**一次都没被走到** | ⏳ **留账**（台账 **D-80**）——**故意为之**：端到端夹具只有 GPIO，UART / 定时器**根本不在图里**，固件不访问 → 状态恒初值 → 加进端到端只会制造**恒真断言**。⚠️ 但**当前树新增的 `attach_uart` 已补上 UART 这一格**；**ROM / 定时器仍缺** |

**V-B 明确核过「没报为缺陷」的部分**（逐条核对，非默认放过）：「同源比较自指」这一类
**基本被避开**——`the_lossless_restore_assertion_has_discriminating_power` **不是恒真式**
（A12「`export_state` 恒空」让它变红 ✓）· `the_malformed_session_code_is_its_own` /
`the_session_snapshot_schema_version_is_one` 用**字面量**而非实现常量 ✓（D-73 同型陷阱被避开）·
M-14 的 `region_kind_tag` 明确写「不引用实现自己的常量」✓ · M-23 的错误码断言用**字面量 5** ✓ ·
数值策略的判别式落在**线上表示**（`"t_virt":"12345"`）而非「字段存在」✓（G15 → **9 red**）·
「每轮预算 10_000」用**字面量**而非 `DEFAULT_INSTRUCTION_BUDGET` ✓ ·
M-14/15/16/20/23 单元测试层判别力**扎实**（14 条变异**全部被抓住**）。

**V-B 的自陈失误（值得记）**：它的第一版变异驱动器在「同一文件多处编辑」时**还原顺序写错**，
会把**已改坏的文件留在沙箱** → **污染最后三个批次**。它自己发现后加了
「每次变异前 `git status --porcelain` 必须为空」的守卫，并把受影响的三个批次**在干净树上重跑**，
结论以重跑为准。→ 这条进了 §3.9.7 的方法论。

---

### 3.9.5 闭环对照表

> 「判别式」列写的是**这条用例的牙齿落在哪**——不是「用例存在」，
> 是「**改坏实现它会不会红**」。

| 发现 | 处置 | 判别式 | 反向变异验证 |
| --- | --- | --- | --- |
| **B-1** 端到端第 ⑦ 项零判别力 | 宿主层 1 的漂移扩到**六处**（`run_continue` + `set_instruction_budget` + `write_ram` + `map_ram` + `write_gpio_out` + `lower_irq`） | 段 1 的**队列 / 插入序 / 指令预算**三项 + 段 0 的 `t_virt` | **K1**（队列不还原）**10 red** · **K2**（插入序不还原）**2 red** · **K3**（预算不还原）**2 red** · **K4**（虚拟时间不还原）**8 red**——**都含**宿主层 1 那一条用例 |
| **H-1** 内存页不重叠无守 | Lane E 补「**升序但重叠**」用例 | 契约层 `validate()` 第 6 类 | Lane E 自验（`prev_page_end = Some(end)` → `Some(p.base)` 必须变红） |
| **H-2** 两条区域子规则无守 | Lane E 补两条用例 + **父会话把它写进规格 §5 第 5 项** | 契约层 `validate()` 第 5 类的两条子规则 | Lane E 自验（各删一条必须变红） |
| **H-3** 段 2 断言被 `load_map` 清零代偿 | 漂移里补一处**写 RAM**（落在导出的页里） | 段 2 的**内存页内容** | **K5**（`load_map` 保留 RAM + 跳过 `load_pages` 的复合变异）——**4 red**，含宿主层 1 那一条 |
| **C-1** 4 个结构体键集没钉住 | Lane E 补齐形状守卫 + 加「新增时间字段必须被挡」的语义守卫 | 载荷的**精确形状** | Lane E 自验（给 GPIO 改名 / 加字段必须变红） |
| **C-2** 计数守卫耦合排版 | Lane E 改成**不依赖排版**的判别式 | 「**恰好 6 个**虚拟时间字段」 | Lane E 自验（把 serde 属性改成多行**不得**变红） |
| **C-3** 段 0 恒真式 | 宿主代填 `t_virt` + 漂移推进虚拟时间 | 段 0 的 **`t_virt`** | **K4**（虚拟时间不还原）——变红 |
| **C-4** 夹具自检重复主断言 | 改成**还原之前**真的断言空态 | 「全新会话还原前是空的」这个**前提** | 父会话复核（`pre.pc == 0` / `pre.gpr == [0;32]` / `pre.irq.raised == []`） |
| **C-5** 变异自检方法论洞 | ⏳ **留账 D-79** | — | — |
| **D-1** 夹具自指常量 | ⏳ **留账 + 披露注释** | — | — |
| **D-2** M-20 字段集守卫顺序敏感 | 改成**排序后的集合比较** | 「字段**集合**」 | **K6**（字段换顺序，**语义无关**）——**期望 0 红**（脆性消除）；**K7**（把 `cp_meta` 加回来）——**必须红**（守卫的牙齿还在） |
| **D-3** 端到端不含 UART/定时器/ROM | ⏳ **留账 D-80**（**故意为之**） | — | — |
| **F-3**（V-A）采集期段 5 可能为空 | ⏳ **留账**（本批**不改行为**） | — | — |
| **F-6**（V-A）`load_map` 的 4/6/9 三码无用例钉住 | ✅ 父会话补三条用例（`m14_bus/tests.rs`） | `E_BUS_04` / `E_BUS_06` / `E_BUS_09` 三个**具名**码 | 父会话复核（把常量改值 → 三条用例变红） |
| **F-1**（V-A）先校验后写入只守住一半 | ✅ 修复 lane（契约层两道闸门 + M-15 两阶段 + 宿主第 0.5 步） | 「**失败 = 什么都没变**」 | 修复 lane 的 **M1 / M2 / M5 / M6 / M7** 逐条变红 + 父会话的契约层 **M14 / M15** 变红 |

---

### 3.9.6 反向变异自检

| 批次 | 条数 | 全红 | 脚本 |
| --- | --- | --- | --- |
| 契约层（父会话） | **17** | **17** | `/tmp/probe/ss_contract_mut.py`（Lane E 改过契约层后**重跑仍 17/17**） |
| Lane A（M-14/15/16） | 26 | 26 | lane 自带 |
| Lane B（M-20/23） | 16 | 16 | lane 自带 |
| Lane C（宿主 + 端到端） | 11 | 11 | lane 自带 |
| 修复 lane（F-1~F-6） | **13**（M1~M13） | **9** | lane 自带（**M10 / M12 / M13 = 0 红 → 父会话已补三条用例**，见 §3.9.5 的 F-6 行） |
| 父会话 · 内核侧（B-1 / H-3 / C-3 / D-2） | **7**（K1~K7） | **7** | `/tmp/probe/k_mut.py` |
| Lane E（契约层 H-1 / H-2 / C-1 / C-2） | **11**（M1~M11） | **11**（M7 是**语义不变**的对照，**必须 0 红**） | lane 自带 |
| **V-B 检视轮**（独立第三方） | **70** | — | 存活变异 **17** 条 → 本批逐条闭环 |

> ⚠️ **本轮的方法论纪律**（每条都是本轮真踩到的）：
> ① 变异**必须能编译**（编译错误不算判别力）；② 还原**必须逐字节校验**（md5）
> 并 `touch` 源文件；③ 期望集合用 **AND 语义**（OR 语义只证明「某条红了」）；
> ④ **完整红集**要一起记；⑤ 变异**快照必须取自最终树**（C-5 的教训）；
> ⑥ **对照变异**（语义不变）必须 **0 红**（D-2 的 K6 就是一条对照）。

---

### 3.9.7 本轮方法论沉淀

> 本节是**跨轮可复用**的结论。写在这里是因为它们**不是**「这一批怎么做的」，
> 而是「**下一批怎么做才不会踩同一个坑**」。

1. **「先校验后写入」必须区分「校验点在哪」与「写入点在哪」**（F-1 的根因）。
   把校验放在**写入路径内部**（哪怕紧挨着写入），对**多对象**回灌来说
   **只守住了最后一个对象**——前面那些已经被写了。
   **判据**：判别式必须落在「**前一个对象的状态有没有被改**」上，
   **不能**只落在「最后一个对象报对了错」上。
   本批的落地形态：契约层加**加法**方法 `Peripheral::validate_session_state()`
   （默认**放行** → 保持加法性）· M-15 的 `restore_session_states` 改**两阶段**
   （先对**所有**实例只读校验，全过才写）· 宿主在 `load_map` / `load_pages`
   **之前**加第 **0.5** 步只读预检（否则「外设畸形」会留下一具
   「内存来自载荷、寄存器来自恢复前」的**缝合怪**）。

2. **一个约束一个落点**（`GPIO_PIN_COUNT`）。同一个数在两处各写一遍
   → 一处一动，载荷**静默停止校验**。本批把「每引脚一条 IRQ 线」这个数
   **由契约层导出**（载荷的形状依赖它），M-15 的 `PIN_COUNT` 改为**引用**它。
   **判据**：凡是「**载荷的形状**依赖某个数」的，那个数归**契约层**——
   外设实例可以跟随，但**不许自己再写一遍**。

3. **同一个失败条件必须是同一个口径**（F-2）。同一个量两个真相：
   INTC 争用在**回灌路径**报 `Err`、在**委派路径**返回 `Ok(())`——引擎因此
   以为那条 IRQ 拉起来了，其实被丢了。
   **判据**：凡是有「计数器记录失败 + 返回值」这种**双通道**的地方，
   必须明确写下：**计数**回答「发生过几次」，**返回值**回答「**这一次**成没成」。
   两者可以并存，但**不能一个响一个哑**。

4. **一个量的两个落点必须有人拍板**（F-4）。`cp_meta.t_virt` 与
   `m12_engine.state.virtual_time_ps` 是同一个量的两处；两边都不管 →
   可以采出「检查点时间 = 0、引擎时间 = 12345 ps」的载荷而**无人报错**。
   本批处置：**宿主代填**（调用方给的值被覆盖，doc 写明）——
   **代填一个**字段不是「造数据」，**留两个落点**才是。

5. **worktree 基线会分叉——派发前必须核对**。Lane B 的 worktree 停在
   `26f6757`，读到的规格 §3.5 还是**旧签名**，据此**误判**「实现与规格不符」。
   **规则**：派发前确认 worktree 基线 == 主工作区 HEAD；
   收卷时确认子会话**实际测的是哪棵树**（本轮父会话独立复核 V-B 所在的
   `/tmp/wt/ss-k3`：`git diff 8bc0ac7 HEAD -- crates/` **为空** → 变异结论
   对最终树**有效**）。

6. **子会话自报的「逐字一致」要独立复核**。父会话第一次用贪婪正则抓到
   **34 833 字符**的差异，**误以为**两份 LLD 不一致；收紧正则后是 **31 行**一致。
   本轮父会话独立复核：**`struct Snapshot` 块 404 字符**、
   **`v1.4 注记` 1212 字符 / 23 行**——**均逐字一致**。
   **规则**：正则的**范围**必须**显式界定**（「块」还是「注记」），
   且第一次结果异常时**先怀疑正则**，再怀疑文档。

7. **撞码时的让号方向**：让「**没落地的**」给「**已落地的**」让号。
   本批契约层新开的 `E_PER-04` / `E_PER-05` 与 M-15 LLD §6.1 **早已分配**给
   另外两个场景的号撞了——而 M-15 那两个号**在实现里从未落地** → 契约层这两个
   **有实现** → 让号，**零行为变化**。**前提是先核实「哪个真的在发」**，
   不能只读文档就判「撞了」。

8. **规格内部的口径必须自洽**（段号）。本批一度出现「§1.2 写第 6 段」
   与「§3 逐段用第 1~5 段」并存 → 统一为「**段 0~5**」
   （`cp_meta`=段 0 … `m16_intc`=段 5；`schema_version` **不编号**）。
   **判据**：新增**编号**时，先把所有编号**列一遍**再落笔。

9. **变异必须能编译**（本轮延续，第三次踩到同一类坑）。编译错误导致的「红」
   **不算判别力**。本轮四路施工 + 父会话的契约层自检合计
   **13 + 26 + 16 + 11 + 修复 lane 的新变异**条，全部按「能编译」的要求构造，
   还原后**逐字节校验**（md5）并 `touch` 源文件。

10. **文档订正必须追到「通过准则」那一层**（承 v0.17）。V-C 第 1 条：
    M-14 §9.3 把 ROM 的恢复写成「恢复时由固件重新装载」——**与规格 §3.1/§3.2
    和实现都相反**；照着实现会写出「重建即清零」= **固件被抹掉** = 会话必然发散。
    **一条错在「怎么实现」上的文档，比一条错在「做什么」上的文档更危险**——
    前者会让人**正确地做错事**。


---

## §3.10 M-16 中断控制器落地检视轮（2026-10-21 · v0.19）

### 3.10.1 两路独立检视的范围划分

| 检视员 | 范围 | 判决 |
| --- | --- | --- |
| **V-A** | Rust 内核侧（M-16 本体 + M-13 CSR/陷阱 + M-17 接线 + 契约层 + 文档一致性） | **FAIL**（1 高危 + 5 中危 + 6 低危 + 7 观察） |
| **V-B** | 测试判别力（只回答「把实现改坏，用例会红吗」） | **PARTIAL**（1 个 P0 缺陷被全绿放过 + 1 个 P1 缺口 + 4 个 P2 形状问题） |

⚠️ **本轮派发方式与前几轮不同**：两路检视员**都**被要求**只读静态检视**、**不许跑 cargo**
（本机另有变异自检在并行，且**多个 worktree 共享同一个 `CARGO_TARGET_DIR`** 会互相换掉
产物 —— 本批已实测命中**两次**假象）。检视员交付的是「**带 `file:line` 证据的缺陷清单 +
可执行的变异三元组**」，由父会话**统一**跑成一张可审计的红绿表 —— 这比各自乱跑更可信，
也让「变异必须能编译」「期望集合用 AND 语义」这两条口径**只有一份**。

### 3.10.2 两路**独立收敛**到同一个 P0：`irq_clear` 同值重写被静默丢弃

**这是本轮最重要的一条**，因为它同时满足三个「最坏形态」：

1. **可达**：guest 的 handler **每次都写同一个掩码** ⇒ 第二次写时「值没变」⇒
   清除被**静默丢弃** ⇒ `active_irq` 永不复位 ⇒ M-16 的单级嵌套防护把那条线
   **永久堵死**（任何线上的任何中断都再也交付不了）；且该卡死状态**会被会话快照
   静默持久化**（契约层校验 11~14 **全部通过**）。
2. **全绿放过**：两条声称覆盖该场景的用例（端到端的「能再来一次」+ M-17 的
   「`mret` 之后能再来一次」）**只数「handler 跑了几次」** ⇒ 那个量在缺陷路径上
   **照样成立**。
3. **把缺陷钉成了期望**：反向证明 —— **把清除真正做掉**反而会让那两条用例**变红**。

**已实测确认**（不是源码推演）：把断言推进到「第二次交付之后的控制器状态」后，
两条用例在**未修**的实现上**变红**（端到端报「挂起队列必须空：实际还挂着
`PendingIrqDto { line: 1, … }`」）。

**根因是「实现偏离了规格」**：STAGE-4 §4 接缝 #4 的原话是「M-17 在 CSR 写之后检查
『**刚写的 CSR 是不是 `irq_clear`**』」—— 实现却用「**值变了**」去**猜**。

**闭环办法 = 换机制，不是打补丁**：

- M-13 的 CSR 写路径**回报**「刚写了哪个 CSR」（`Iss::take_last_csr_written`）——
  回报由**执行臂自己**产生 ⇒ 「这条指令写没写」的判据**只有一份**，不会漂移；
- M-17 改用这个回报做判据，「值变了」的判据**删掉**；
- ⚠️ **回报**而不是**回调**：回调会让 M-13 持有一个闭包，而那个闭包要反过来借用
  M-17 自己的 `Iss` ⇒ **自引用借用**（Rust 不允许）—— 这与裁决 I 否掉「M-12 的
  `InterruptPoller` 接缝」是**同一个**理由。

### 3.10.3 闭环对照表

| # | 来源 | 严重度 | 发现 | 处置 |
| --- | --- | --- | --- | --- |
| F-1 | V-A + V-B | 🔴 高危 | `irq_clear` 同值重写 ⇒ 第二次清线被静默丢弃 ⇒ 中断控制器被永久卡死；两条覆盖用例**假绿**且把缺陷**钉成期望** | ✅ **已闭环**（commit `f2a00d2`）：机制换成「M-13 回报 + M-17 用回报」；两侧用例的断言推进到「第二次交付之后的控制器状态」。**已实测**：未修 ⇒ 红；修后 ⇒ 绿 |
| F-2 | V-A | 🟡 中危 | 规格裁决 D 声称 `mie_bit_for_line` 有**两处**消费者，而**第二处**（M-13 写 `mip`）根本没实现 | ✅ **规格已订正**（不留一句没人实现的规格）+ 登记 **D-84**（P1） |
| F-3 | V-A | 🟡 中危 | 验收 #6 的「**顺序**」完全没被钉住（形状用例把键**排序**后比对 ⇒ 按定义与顺序无关；`serde_json` 没开 `preserve_order` ⇒ 声明顺序根本不被保留） | ✅ **已闭环**：补一条用 `serde_json::to_string`（结构体直接序列化 ⇒ 按**声明顺序**写）断言键先后顺序的用例 |
| F-4 | V-A | 🟡 中危 | 本批自己的「文档关系表」（STAGE-4 §8）整条未落地（`REVIEWS.md` / `PROGRESS.md` / `AGENTS.md` 都没有 v0.19 记录） | ✅ **已闭环**：本节 + `PROGRESS.md` v0.19.0 段 + `AGENTS.md` v0.19 判据块 |
| F-5 | V-A | 🟡 中危 | 黄金对拍对本批的新代码**零覆盖**（语料**明写**排除 `rv32mi`/`rv32si` 的理由是「那正是 M-16 在第 5 阶段落的」——**本批就是那个第 5 阶段**，但语料没扩）⇒ 验收 #32 是**恒真判据** | ⏳ **登记为 D-85**（P1）：扩语料要同时改 `tests/golden/env/riscv_test.h`（它是 CSR-free 的替代实现）⇒ 是**独立**的一条活 |
| F-6 | V-A | 🟡 中危 | D-59 残留两个死变体：`StepOutcome::Breakpoint` / `Watchpoint`（M-17 的两臂**不可达**） | ✅ **已闭环**：D-59 台账行**逐字点名**的 `Breakpoint` 一并删除（`Watchpoint` 同类）⇒ M-17 的 `match` **穷举**全变体 |
| F-7 | V-A | 🟢 低危 | 表16 漏收 **`E_TRX-03`**（生产前端 `front/services/m07_transport.js` 发出它）⇒ D-70 留了个洞 | ✅ **已闭环**：补进（表16 **57 → 58 码 / 12 族**） |
| F-8 | V-A | 🟢 低危 | `front/lib/message.js` 留着契约层**已订正掉**的「Pause at the next **quantum** boundary」旧措辞 | ✅ **已闭环**：JS 镜像同步订正为「下一条**指令**边界」 |
| F-9 | V-A | 🟢 低危 | M-16 模块头指向**不存在**的「M-12 `InterruptPoller` 接缝」（裁决 I 已否） | ✅ **已闭环**：模块头改指 M-17 的执行器，并写明「为什么不是 M-12」（自引用借用） |
| F-10 | V-A | 🟢 低危 | 冻结规格**自相矛盾**：§4 接线表第 2 行仍写「M-12 新增 `InterruptPoller` 接缝」，而 §4.1 明写「接缝不在 M-12」 | ✅ **已闭环**：§4 接线表第 2 行订正 |
| F-11 | V-A | 🟢 低危 | M-17 的「⚠️ **手写编码**：仓库里没有 RISC-V 汇编器」与端到端夹具的说法**矛盾** | ✅ **已闭环**：两处固件的来源分别写清（手打 / 汇编器产出） |
| F-12 | V-A | 🟢 低危 | M-17 的注释把 F-1 的缺陷**说成了设计**（「写两次同一个值不会丢清除请求……第二次是空操作」） | ✅ **已闭环**（随 F-1 的修复一并重写） |
| F-3′ | V-B | 🟡 P2 | 中断路径的 `mtval == 0` 是**真空断言**（`csr_read` 对「文件里没有的 CSR」也返回 0） | ✅ **已闭环**：改成「**先断言这条 CSR 在文件里**，再断言值是 0」 |
| F-4′ | V-B | 🟡 P2 | `host.intc_state()` 争用时返回的**默认值正好等于断言值** ⇒ 潜伏假绿 | ✅ **已闭环**：补 `intc_delegation_failures() == 0` 断言 |
| F-2′ | V-B | 🟠 P1 | 定时器 `mtime >= mtimecmp` 的**等号边界**无人守（旧用例名叫「reaches」却用的是 `2000 > 1000`）⇒ 变异 `>=` → `>` 是 **0 红** | ✅ **已闭环**：补一条等号用例（变异自检 **K-11** 现在会红） |
| G-9 | V-B | 🟡 P2 | 端到端固件字节**写死**在 Rust 数组里，汇编源在**仓库外**（会丢）⇒ 字节与源之间**没有自动一致性检查** | ✅ **已闭环**：汇编源**进仓库**（`crates/kernel/tests/fixtures/intc_fw.S`）+ 补一条「关键指令编码」守卫用例（变异自检 **K-32** 现在会红） |
| — | V-B | 🟡 P2 | 旧形状载荷（只有 `raised`）只测了**反序列化**，没测 `validate()` | ✅ **已闭环**：补一条「旧形状必须仍然过校验」 |
| — | V-B | 🟡 P2 | 端到端「边界前」写死 8 条 ⇒ 配额一旦调小就会因**夹具**变红 | ✅ **已闭环**：条数**从量子配额算出来** |
| O-5 | V-A | 🔵 观察 | `SharedInterruptPort::take_intr` / `clear` 争用时**静默降级**（不报错） | ⏳ 留账（当前生产路径不可达） |
| O-1~O-4 / O-6~O-7 | V-A | 🔵 观察 | `InterruptSink::raise` 用**缓存**虚拟时间（首次 poll 前恒为 0）· `insert_seq` 的 `wrapping_add` · `load_state` 把 `priorities` 整体替换 · `load_state` 不还原 `pending_delivery` · 固件 21 条编码逐条核对**全对** · `QUANTUM_INSTR_QUOTA` 是自指常量（但判别式落在**关系**上，**可接受**） | ⏳ 留账 / 已核对 |
| — | 自查 | 🟠 中危 | **变异驱动器的红集比对逻辑有 bug**：`hit` 装的是「包含期望串的**输出行**」，却拿它跟「期望名**数量**」比大小、并用集合差算缺失 ⇒ **期望用例其实红了也被判成「缺」** | ✅ **已闭环**：改为「红测名 ∋ 期望串」的 AND 语义判定，并**打印完整红集** |
| — | 自查 | 🟠 中危 | **变异驱动器没有超时** ⇒ 活锁的变异让测试二进制**永不退出** ⇒ 整个自检**永远卡在那里**（既不是红也不是绿） | ✅ **工具链侧已闭环**（硬超时 + `killpg` 杀整个进程组 + `HANG` 判定）；**生产侧**（M-12 `run_until` 的无界自旋）登记为 **D-83**（留给 D-30 那一轮） |
| F-13 | 变异自检 | 🟠 中危 | **「回报」判据在六条 CSR 指令臂上各写了一遍，却只有一条臂有反向变异证明**：变异 **K-18b**（把回报退化成「值变了才回报」）实测**只红端到端那条**、缺 M-17 那条 —— 查下来**不是**用例没判别力，而是**变异覆盖面**：K-18b 的锚点在 **CSRRW 臂**（CSR 号 + 寄存器源），端到端固件写 `csrrw x0, irq_clear, t3`（命中 CSRRW）而 **M-17 的手打固件写 `csrrwi x0, irq_clear, 2`**（命中 **CSRWI** 臂）；六条臂的「写 + 回报」片段**逐字相同**，`replace(old, new, 1)` **只命中第一条** ⇒ `CSRRS` / `CSRRC` / `CSRSI` / `CSRCI` **四条臂裸奔**（既无固件用、也无单测断言 `take_last_csr_written`） | ✅ **已闭环**（补测 + 补变异）：新增 M-13 单测 `every_csr_instruction_reports_the_csr_number_it_wrote`（**逐臂**表驱动，6 条指令**各写两次**、**第二次是同一个值** ⇒ 必须回报 `Some(该 CSR 号)`）· `a_repeated_write_of_the_same_value_still_reports_the_csr`（**重复写同一个值**也必须回报 —— 这正是 F-1 的**现场**）与 `an_ignored_write_reports_nothing`（只读 CSR 的写被忽略 ⇒ **不回报** ⇒ 否则 M-17 会把「写了 `mip`」当成「写了 `irq_clear`」⇒ **清错线**）；变异集补 **K-38**（CSRWI 臂）· **K-39~K-42**（CSRRS / CSRRC / CSRSI / CSRCI 四条臂）逐条退化成「值变了才回报」⇒ **六条臂全部有反向证明**。⚠️ **「各写两次」是必需的**：只写**一次**时第一次的值确实从 0 变成 2 ⇒「值变了才回报」的退化**照样回报** ⇒ 这组变异**抓不住** ⇒ 判别力缺口。⚠️ **本条的实测还牵出两条工具链问题**（同一轮的 K-18 / K-18b 编译到的是**旧版测试集** ⇒ 结论混源）—— 见 §3.10.5 第 13 / 14 条。⚠️ 附带发现：M-17 固件用**立即数**形式、端到端固件用**寄存器**形式 —— 两份固件**互补覆盖**了两条臂，这是**好设计**，只是当时没人指出另外四条臂裸奔 |
| F-14 | 收口轮自查 | 🟠 中危 | **验收清单二次对账**又找出 2 项**有正向用例、但没有任何反向变异**的验收项 ⇒ 那两条用例**没有判别力**的证据：验收 #4 的第三条拒绝（「挂起队列**顺序错乱** ⇒ 拒」，用例 `an_out_of_order_pending_queue_is_rejected`）与验收 #5（「`validate()` **接受**旧形状载荷」，用例 `an_old_segment_5_payload_still_passes_the_validator`） | ✅ **已闭环**：变异集补 **K-43**（validate 不再拒「顺序错乱」）与 **K-44**（validate 不再接受「旧形状」）⇒ 变异集 47 → **49** 条。⚠️ **更重要的是这两次对账各自证伪了一个「疑似缺口」** —— 验收 #9（「未使能的高优先级**不阻塞**已使能的低优先级」）一度看着像缺口，**实际已被 K-08 覆盖**（用**实际红集**核实，不是凭变异描述推断）；验收 #18 的「`rd == x0` **不读**」在本平台**不可观测**（CSR 读**无副作用** ⇒ 「读了丢弃」与「不读」**行为等价**）⇒ 属**规格**该写明的实现事实（已订正 STAGE-4 §6 #18）。⇒ **教训**：**「疑似缺口」必须用实际红集核实** |
| F-15 | 收口轮自查 | 🟠 中危 | **8 条「HANG」里 7 条是假 HANG**：逐条落盘核对后发现，末个 target 的测试二进制**压根没启动**（输出里**没有**它自己打印的 `running N tests`），而那些末个 target 是 `riscv_sim_server` / `riscv_golden`（**0 个测试**）⇒ **不可能挂死** ⇒ 根因是 macOS 对**刚链接**二进制的**安全扫描**堵住 `exec`（实测**每文件固定 ≈25 秒**，与体积无关）⇒ 一轮 12 个二进制 = **300 s** ⇒ **正好打满**旧的 300 s 窗口 ⇒ **系统性**产出假 HANG | ✅ **已闭环**：① **HANG 判据改看「二进制启动没有」**（**确凿活锁** / **假 HANG** / **超时未跑完** 三形态，只看输出）；② **收敛 target 集**（44 个期望红测逐条 `grep` 核实 ⇒ **41 个在 lib** + **3 个在 E2E** + **零个**在别处 ⇒ 每轮只重链 **3 个**二进制 ⇒ 扫描成本 300 s → **75 s**）；③ **预热**（正式跑前先 exec 一次 `--list` ⇒ 25 s → **0.02 s**）；④ 运行超时**放宽到 900 s**；⑤ **全量重跑 49 条**（旧口径的 HANG 判定**整体不可信**）⇒ **只有 K-10 是真活锁**。⚠️ **走不通的两条**（别再试）：`xattr -d com.apple.provenance`（**受保护的系统属性**，删完仍在）· `strip`（扫描成本与体积无关） |

### 3.10.4 反向变异自检（49 条 · 主工作区串行 · 增量编译）

变异集 `/tmp/probe/k_mut_v19.py`（**49** 条），驱动器 `/tmp/probe/k_mut.py`。
下表由 `/tmp/probe/build_table.py` 从**六份日志**合并生成（`log1` → `run2` →
`rerun` → `run3` → `run4` → `run5`，**后者覆盖前者** ⇒ 同一条目取**最后一次**
实测），表里的「实测轮次」列让**每一行都可追溯到某一次跑**。

⚠️ **本轮修掉了驱动器自身的五处缺陷**（前四处会让结论**假绿**或**没结论**，
第五处会让**真活锁**被当成**假象**）：红集比对逻辑 · 无超时 · 备份键撞名风险 ·
**超时口径把「编译」与「运行」挤在同一个窗口里**（编译慢被误判成活锁）·
**doc-test 造成的假 HANG**。⇒ **教训**：**工具链自己也是被检视对象** —— 一条变异
自检结论（尤其「0 红」这种结论）**先怀疑工具**，再怀疑被测代码。

⚠️ **「HANG」有三种形态，必须分开**：**确凿活锁**（cargo 打印了
`test … has been running for over 60 seconds` ⇒ **点名了**挂死的那个测试）·
**超时未跑完（未证实挂死）**（某个 target 在超时窗口内没出结果，**但可能只是慢**
⇒ ⇒ **不构成活锁证据** ⇒ 需**加长超时重跑**才能定性）· **假 HANG**（末个 target 的
测试二进制**压根没启动** ⇒ macOS 对**刚链接**二进制的**安全扫描**堵住了 `exec`
⇒ **与被测代码无关**）⇒ 后者**既不证明活锁、也不证明判别力**，是**「没结论」**，
必须**重跑**。⇒ **最终判据（只看输出，不看时间）**：末个 target 的二进制**自己打印了**
`running N tests` **没有** ⇒ **假 HANG**；**有** ⇒ 二进制**启动过** ⇒ 再看 cargo
**点名**了哪个测试 ⇒ **点了** ⇒ 确凿活锁；**没点** ⇒ 超时未跑完（**可能只是慢**）。
⚠️ **决定性反证**：末个 target 若是 `unittests src/main.rs`（`riscv_sim_server`，
**0 个测试**）或 `unittests src/bin/riscv-golden.rs`（**0 个测试**）⇒ 它
**不可能**挂死 ⇒ 那种「HANG」**必然**是假的 ⇒ 本轮 8 条「HANG」里 **7 条是假的**，
**只有 K-10 是真活锁**。⇒ 根因与缓解见 **§3.10.5 第 17 条**。
⇒ 实证：**同一条变异 K-19**（`last_poll_ps`
每批次重置）在 `log1` 判 **RED ✓**、在 `rerun` 判 **HANG** ⇒ 同一变异两个结果
⇒ 旧口径的 HANG 判定**整体不可信**。
⚠️ ⚠️ **「超时未跑完」这一档里，多数是「慢」而不是「卡住」** —— 两条硬证据：
① **K-27**（CSRRS 的「`rs1 == x0` 仍写」⇒ 纯读语义失守 ⇒ 按语义**绝不该让
平台活锁**）的输出停在 `Running … riscv_golden.rs`，而它**只启动了 3 个
target** 就超时了 ⇒ 说明是**前面某个 target 慢**（kernel lib 的 683 条用例）
⇒ **机器负载**（实测 load ≈ 4.9）下 300 s 窗口不够；② **K-21** 停在
`Running unittests src/main.rs`（`riscv_sim_server`）—— 那个 target **一个测试
都没有** ⇒ **它不可能是挂死的那个** ⇒ 判「活锁」显然错了。⇒ **「最后启动的
target」≠「挂死的 target」**（cargo **并行**启动多个 test binary，输出停在的
那个只是**调度顺序上的最后一个**）⇒ **只有 cargo 自己点名的那一行**才算证据。
⇒ ⚠️ **新口径的实测结果（`run5` 全量重跑）**：旧口径判为 **HANG** 的 **6 条**
   （**K-17** / **K-18** / **K-19** / **K-21** / **K-22** / **K-24**）—— 在新口径下
   **全部判 RED ✓**；**唯一**判**确凿活锁**的是 **K-10**（`take_intr` 取走不消失）。
   ⇒ **旧口径的 HANG 判定被逐条证伪**，不是「少判了几条」而是「**判反了**」——
   每一条都能**正常跑完**并给出**完整红集**。
⇒ ⚠️ **本表每一行都出自全量重跑轮次** —— `run5`（49 条全量）· `run6`（5 条
   修复后的逐臂变异）· `run7`（4 条对照 · **全套** target）⇒ **旧口径的行一条都不认**。
⇒ **处理口径**：旧口径下的 HANG 判定**整体不可信** ⇒ 本轮**全量重跑**
（49 条，一条不省）；运行超时**放宽到 900 s**（内核 lib 本身只要 0.47 s ⇒
300 s 绝不够）；并把**参与判定的 target 集**收敛到「**能覆盖全部 44 个期望红测的
最小集**」（`--lib --test intc_end_to_end` —— 逐条 `grep -rl "fn <名>" crates/`
核实过：**41 个在 lib**、**3 个在 E2E**、**零个**在别处）⇒ 每轮只重链 **3 个**
二进制 ⇒ 系统扫描成本 300 s → **75 s**；**对照变异**另跑一轮**全套**
（`--lib --tests`），因为「0 红」这条结论的**范围**就是它的强度。
⇒ **不为了凑「全绿」的表而偷偷扩大超时**。

⚠️ **变异跑期间改源文件 ⇒ 结论混源**：本轮我**在变异驱动正在跑的当口**把
`every_csr…` 从「各臂写一次」改成「**每条臂写两次**（第二次同值）」并新增
`a_repeated_write…` ⇒ `rerun` 那一轮的 K-18 / K-18b 编译到的是**旧版测试集**
⇒ 它们的结论**不成立**（K-18b 抓不住 `every_csr…` 正是因为旧版只写一次 ⇒
第一次写时值确实变了 ⇒「值变了才回报」的退化**抓不住**）⇒ 已在 `run4` 重跑。
⇒ **判据**：一张可审计的红绿表里，**每一行都必须出自同一版测试集**。

⚠️ **对照变异 4 条**（语义不变 ⇒ 必须 0 红）+ **「预期 HANG」是本轮新引入的判定
类别**：挂死**不算判别力**，但它**是一条真发现**（见 **D-83**）。⚠️ **登记标准**：
只有**新口径**下实测判**确凿活锁**（cargo **点名了**挂死的那个测试）的条目才登记
—— 旧口径登记的 5 条里 **K-17 已被证伪**（新口径判 **RED ✓**）。
⚠️ **「会不会挂死」不是该缺陷的稳定特征**：**行为等价**的两个变异
（**K-17**「`irq_clear` 的副作用整个不生效」与 **K-18**「CSRRW 不回报」⇒ 效果
都是清线失效）**一个判 RED、一个判超时未跑完** ⇒ 取决于**哪条路径**先碰到那条
被堵死的线。⇒ 登记「预期 HANG」必须给出**同形态的实证**，且**不能事后为了让它
变绿而改成「预期」**。

⚠️ **变异的「覆盖面」不等于「用例的判别力」**：K-18b 实测**只红一条、缺一条**，
追下去发现是**变异只打在 `csrrw` 一条臂上**（六条 CSR 臂的代码**逐字相同**）⇒ 补
**K-38~K-42** 逐臂覆盖 ⇒ 见 §3.10.3 的 **F-13**。

⚠️ **变异集从 37 条扩到 49 条**是**两次逐条对账**的结果：
**第一次**（37 → 47）—— 把冻结规格 §6 的 **33 项验收**逐条映射到变异条目后，
发现 4 项**只有正向用例、没有反向变异** ⇒ 补 **K-33~K-37**（#21 未知 CSR 读 0 ·
#2 `active_irq` 默认值 · #4 `validate()` 的两条拒绝 · #24 非法指令 `mtval`）；
再把「F-1 修复所依赖的回报判据」按 **CSR 六条臂**展开 ⇒ 补 **K-38~K-42**。
**第二次**（47 → 49）—— 收口轮**再对一次**账，又找出 2 项**有正向用例、但
没有任何反向变异**的验收项 ⇒ 补 **K-43**（验收 #4 的第三条拒绝：「挂起队列
**顺序错乱** ⇒ 拒」）与 **K-44**（验收 #5：「`validate()` **接受**旧形状载荷」）。
⚠️ **这两次对账也各自证伪了「疑似缺口」** —— 靠的是**实际红集**而不是变异
描述：验收 #9「未使能的高优先级**不阻塞**已使能的低优先级」一度看着像缺口，
**实际已被 K-08 覆盖**（K-08 的实际红集里就有它）；而验收 #18 的「`rd == x0`
**不读**」在本平台**不可观测**（CSR 读**无副作用** ⇒ 「读了丢弃」与「不读」
**行为等价**）⇒ 属**规格**该写明的实现事实（已订正 STAGE-4 §6 #18），
**不是**判别力缺口。⇒ **教训**：**「疑似缺口」必须用实际红集核实** ——
凭变异描述推断会**误登记**（把「有正向用例」当成「有判别力」），
也会**误判缺口**（把「已被别的变异覆盖」当成「没人管」）。

<!-- MUT-TABLE:BEGIN -->
| # | 变异 | 判决 | 红测数 | 实测轮次 | 变异内容 |
| --- | --- | --- | --- | --- | --- |
| 1 | `K-01` | **RED** | 1 | run5 | M-16 poll：全局使能（mstatus.MIE）不再门控交付 |
| 2 | `K-02` | **RED** | 1 | run5 | M-16 poll：单级嵌套防护失效（已交付的中断可被覆盖） |
| 3 | `K-03` | **RED** | 1 | run5 | M-16 clear：不清掉「已登记但未取走」的交付 |
| 4 | `K-04` | **RED** | 7 | run5 | M-16 clear：active_irq 不复位（单级防护永远挡住下一次交付） |
| 5 | `K-05` | **RED** | 4 | run5 | M-16 request：同线去重失效（挂起项堆积） |
| 6 | `K-06` | **RED** | 3 | run5 | M-16 request：不再发中断脉冲（D-26 的生产者没了） |
| 7 | `K-07` | **RED** | 3 | run5 | M-16 arbitrate：优先级比较反转（交付最低优先级那条） |
| 8 | `K-08` | **RED** | 27 | run5 | M-16 refresh_enabled_mask：拿 line 号当 mie 位号（两套编号空间混淆） |
| 9 | `K-09` | **RED** | 6 | run5 | M-16 poll：交付的 cause 不带中断位（IRQ_BASE） |
| 10 | `K-10` | **HANG（确凿活锁）**　<sub>登记：预期活锁</sub> | 0 | run5 | M-16 take_intr：取走**不**消失 ⇒ 同一个交付被反复交付 ⇒ **活锁**（挂死） |
| 11 | `K-11` | **RED** | 1 | run5 | M-16 定时器：`>=` 改 `>` ⇒ 等号边界失守（V-B F-2 补的那条用例） |
| 12 | `K-12` | **RED** | 1 | run5 | M-16 load_state：改成「合并」而不是「整体替换」 |
| 13 | `K-13` | **RED** | 2 | run5 | M-16 request：队列不按 (时刻, 插入序) 规范序插入 |
| 14 | `K-14` | **RED** | 1 | run5 | M-16 refresh_enabled_mask：无 mie 映射的线被**静默清零** |
| 15 | `K-15` | **RED** | 4 | run5 | M-17：量子边界采样条件失效（每条指令都采样 ⇒ 随时交付） |
| 16 | `K-16` | **RED** | 9 | run5 | M-17：取走交付后**不陷入**（不进 handler） |
| 17 | `K-17` | **RED** | 4 | run5 | M-17：irq_clear 的副作用**整个不生效**（F-1 修复的判据被拆掉） |
| 18 | `K-18` | **RED** | 4 | run5 | M-13：CSRRW **不回报**写了哪个 CSR ⇒ M-17 收不到判据（F-1 修复的源头） |
| 19 | `K-18b` | **RED** | 3 | run5 | M-13：**只在「值变了」时回报** ⇒ 精确复现 F-1 的老机制（第二次清线被跳过） |
| 20 | `K-19` | **RED** | 1 | run5 | M-17：last_poll_ps 每批次重置（挂在执行器上 ⇒ 量子边界判据失效） |
| 21 | `K-20` | **RED** | 1 | run5 | M-17：pause 标志取走不消失（一次暂停影响所有批次） |
| 22 | `K-21` | **RED** | 8 | run5 | M-13 enter_trap：不写 mtval（D-64 的落点没了） |
| 23 | `K-22` | **RED** | 3 | run5 | M-13 enter_trap：mstatus.MPIE 不保存旧 MIE（mret 恢复不出来） |
| 24 | `K-23` | **RED** | 18 | run5 | M-13 enter_trap：不跳 mtvec（handler 根本不跑） |
| 25 | `K-24` | **RED** | 15 | run5 | M-13 enter_trap：mepc 写错（回不到被中断处） |
| 26 | `K-25` | **RED** | 3 | run5 | M-13 mret：MIE 不从 MPIE 恢复（中断回不来） |
| 27 | `K-26` | **RED** | 2 | run5 | M-13：取指未对齐报成 mcause 4（访存未对齐）—— D-63 的落点 |
| 28 | `K-27` | **RED** | 2 | run5 | M-13 CSRRS：rs1 == x0 时仍写（纯读语义失守） |
| 29 | `K-28` | **RED** | 6 | run5 | 契约：只读 CSR 的写不再被忽略（guest 能改 mip / mtime） |
| 30 | `K-29` | **RED** | 7 | run5 | 契约：mie_bit_for_line 把 timer 线映到 MEIE（位号空间错） |
| 31 | `K-30` | **RED** | 2 | run5 | 契约：段 5 的 mtime_ps 去掉十进制字符串（2^53 ps 会越界） |
| 32 | `K-31` | **RED** | 5 | run5 | 契约：段 5 的一个字段改 JSON 名 ⇒ 形状用例必须红（形状/顺序的守门人） |
| 33 | `K-32` | **RED** | 3 | run5 | 端到端夹具：irq_clear 掩码 2 改成 1 ⇒ 夹具防漂移守卫必须红 |
| 34 | `K-C1` | 0 红　<sub>对照（语义不变 ⇒ 必须 0 红）</sub> | 0 | run7 | 对照：仲裁的同优先级比较改写成等价形式（必须 0 红） |
| 35 | `K-C2` | 0 红　<sub>对照（语义不变 ⇒ 必须 0 红）</sub> | 0 | run7 | 对照：mtvec 基址掩码换成等价写法（必须 0 红） |
| 36 | `K-C3` | 0 红　<sub>对照（语义不变 ⇒ 必须 0 红）</sub> | 0 | run7 | 对照：只读判定的比较算子换写法（必须 0 红） |
| 37 | `K-C4` | 0 红　<sub>对照（语义不变 ⇒ 必须 0 红）</sub> | 0 | run7 | 对照：量子常量换个路径限定（必须 0 红） |
| 38 | `K-33` | **RED** | 4 | run5 | M-13 csr_read：未知 CSR 读成 1 而不是 0（验收 #21） |
| 39 | `K-34` | **RED** | 18 | run5 | 契约：active_irq 默认值改成 0（会被读成「行 0 是活动中断」）（验收 #2） |
| 40 | `K-35` | **RED** | 2 | run5 | 契约：validate 不再拒「挂起队列里有重复行」（验收 #4） |
| 41 | `K-36` | **RED** | 1 | run5 | 契约：validate 不再拒「已交付未清除的行不在队列里」（验收 #4） |
| 42 | `K-37` | **RED** | 2 | run5 | M-13：非法指令不把指令字写进 mtval（验收 #24） |
| 43 | `K-38` | **RED** | 2 | run6 | M-13：**CSRWI 臂**退化成「值变了才回报」⇒ M-17 的固件（csrrwi）也要被抓住 |
| 44 | `K-39` | **RED** | 1 | run6 | M-13：**CSRRS 臂**退化成「值变了才回报」 |
| 45 | `K-40` | **RED** | 1 | run6 | M-13：**CSRRC 臂**退化成「值变了才回报」 |
| 46 | `K-41` | **RED** | 1 | run6 | M-13：**CSRSI 臂**退化成「值变了才回报」 |
| 47 | `K-42` | **RED** | 1 | run6 | M-13：**CSRCI 臂**退化成「值变了才回报」 |
| 48 | `K-43` | **RED** | 1 | run5 | 契约：validate 不再拒「挂起队列顺序错乱」（验收 #4 的第三条拒绝） |
| 49 | `K-44` | **RED** | 26 | run5 | 契约：validate 不再接受「旧形状载荷」（验收 #5） |

**结果**：49 / 49 条**按期望**（全集 49 条）。**RED** 44 条 · **确凿活锁** 1 条 · **0 红（对照）** 0 条 · **超时未跑完（未证实挂死）** 0 条 · **HANG（假）** 0 条 · **不符期望** 4 条 · **未跑** 0 条。

> 「实测轮次」列是**可审计性**的关键：同一条目可能被跑过多次，本表取**最后一次**实测（`run7` > `run6` > `run5` > `run4` > `rerun` > `run2` > `log1`）。⚠️ **`log1` / `run2` / `rerun` 是旧口径**（编译与运行挤在同一个超时窗口里，且会撞上 doc-test 的扫描堵塞）⇒ 那些轮次的 **HANG 判定不可信**；⚠️ **`rerun` 那一轮还混源** —— 它跑在「`every_csr…` 还只写一次」的**旧版测试集**上 ⇒ 它的 K-18 / K-18b 结论已被 `run4` 取代。⇒ **`run5` 是全量重跑**（49 条一条不省，新判据 + AND 语义 + 900 s 窗口 + 收敛后的 target 集）；**`run6` 重跑 5 条修复后的逐臂变异**（`K-38~K-42` —— 原替换文本把 `csr::csr_read` 写成了裸 `csr_read` ⇒ **编译失败** ⇒ 变异无效）；**`run7` 重跑 4 条对照**（**全套** target —— 「0 红」这条结论的**范围**就是它的强度）。⚠️ **本表每一行都出自 `run5` / `run6` / `run7`** —— 旧口径的行**一条都不认**。

<details><summary>逐条红集（仅 RED 条目）</summary>

- `K-01`（1，run5）：`m16_interrupt::tests::the_global_enable_bit_gates_delivery_without_losing_the_pending_entry`
- `K-02`（1，run5）：`m16_interrupt::tests::a_delivered_interrupt_blocks_the_next_arbitration`
- `K-03`（1，run5）：`m16_interrupt::tests::clear_drops_a_delivery_that_was_not_taken_yet`
- `K-04`（7，run5）：`m16_interrupt::tests::one_timestamped_input_sequence_yields_one_fixed_delivery_sequence` · `m16_interrupt::tests::a_delivered_interrupt_blocks_the_next_arbitration` · `m16_interrupt::tests::clear_removes_the_queue_entry_the_level_and_the_active_line` · `m17_backend::tests::writing_irq_clear_clears_the_pending_entry_and_resets_active_irq` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-05`（4，run5）：`m16_interrupt::tests::the_timer_raises_line_three_when_mtime_reaches_the_compare_value` · `m16_interrupt::tests::a_folded_request_does_not_rewrite_the_queued_entry` · `m16_interrupt::tests::a_repeated_request_on_the_same_line_is_folded_into_one_queue_entry` · `m16_interrupt::tests::the_pulse_callback_fires_once_per_registered_request`
- `K-06`（3，run5）：`kernel_host::tests_intc::the_pulse_sink_survives_a_project_reload` · `kernel_host::tests_intc::one_request_produces_exactly_one_irq_pulse_edge` · `m16_interrupt::tests::the_pulse_callback_fires_once_per_registered_request`
- `K-07`（3，run5）：`m16_interrupt::tests::set_priority_changes_the_arbitration_outcome` · `m16_interrupt::tests::arbitration_takes_the_highest_priority_line` · `m16_interrupt::tests::one_timestamped_input_sequence_yields_one_fixed_delivery_sequence`
- `K-08`（27，run5）：`kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest` · `m16_interrupt::tests::arbitration_takes_the_highest_priority_line` · `m16_interrupt::tests::a_disabled_high_priority_line_does_not_block_an_enabled_low_priority_one` · `m16_interrupt::tests::a_delivered_interrupt_blocks_the_next_arbitration` · `m16_interrupt::tests::clear_removes_the_queue_entry_the_level_and_the_active_line` · `m16_interrupt::tests::clear_drops_a_delivery_that_was_not_taken_yet` · `m16_interrupt::tests::equal_priority_and_equal_time_lines_are_ordered_by_insert_seq` · `m16_interrupt::tests::equal_priority_lines_are_ordered_by_request_time` · `m16_interrupt::tests::export_and_load_round_trip_restores_the_whole_controller_state` · `m16_interrupt::tests::one_timestamped_input_sequence_yields_one_fixed_delivery_sequence` · `m16_interrupt::tests::take_intr_hands_out_exactly_what_poll_registered` · `m16_interrupt::tests::set_priority_changes_the_arbitration_outcome` · `m16_interrupt::tests::the_delivery_cause_encodes_the_interrupt_bit_and_the_line` · `m16_interrupt::tests::the_exported_segment_satisfies_the_contract_validator` · `m16_interrupt::tests::the_global_enable_bit_gates_delivery_without_losing_the_pending_entry` · `m16_interrupt::tests::the_quantum_boundary_refreshes_the_enable_mask_from_mie` · `m16_interrupt::tests::the_timer_raises_line_three_when_mtime_reaches_the_compare_value` · `m16_interrupt::tests::the_timer_raises_the_line_when_mtime_equals_the_compare_value` · `m17_backend::tests::an_interrupt_is_delivered_only_at_a_quantum_boundary` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `m17_backend::tests::the_delivery_does_not_count_as_an_executed_instruction` · `m17_backend::tests::writing_irq_clear_clears_the_pending_entry_and_resets_active_irq` · `the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `an_interrupt_is_delivered_only_after_the_first_quantum_boundary` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-09`（6，run5）：`m16_interrupt::tests::the_delivery_cause_encodes_the_interrupt_bit_and_the_line` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m16_interrupt::tests::one_timestamped_input_sequence_yields_one_fixed_delivery_sequence` · `m17_backend::tests::an_interrupt_is_delivered_only_at_a_quantum_boundary` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table`
- `K-11`（1，run5）：`m16_interrupt::tests::the_timer_raises_the_line_when_mtime_equals_the_compare_value`
- `K-12`（1，run5）：`m16_interrupt::tests::load_state_replaces_the_whole_state`
- `K-13`（2，run5）：`m16_interrupt::tests::the_exported_segment_satisfies_the_contract_validator` · `m16_interrupt::tests::the_queue_keeps_the_canonical_order_by_request_time`
- `K-14`（1，run5）：`m16_interrupt::tests::a_line_without_a_mie_bit_keeps_its_enable_bit`
- `K-15`（4，run5）：`m17_backend::tests::an_interrupt_is_delivered_only_at_a_quantum_boundary` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m17_backend::tests::the_quantum_boundary_sampling_survives_across_batches` · `an_interrupt_is_delivered_only_after_the_first_quantum_boundary`
- `K-16`（9，run5）：`kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m17_backend::tests::writing_irq_clear_clears_the_pending_entry_and_resets_active_irq` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `m17_backend::tests::an_interrupt_is_delivered_only_at_a_quantum_boundary` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it` · `the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `an_interrupt_is_delivered_only_after_the_first_quantum_boundary`
- `K-17`（4，run5）：`m17_backend::tests::writing_irq_clear_clears_the_pending_entry_and_resets_active_irq` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-18`（4，run5）：`m13_iss::tests_csr::a_repeated_write_of_the_same_value_still_reports_the_csr` · `m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-18b`（3，run5）：`m13_iss::tests_csr::a_repeated_write_of_the_same_value_still_reports_the_csr` · `m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-19`（1，run5）：`m17_backend::tests::the_quantum_boundary_sampling_survives_across_batches`
- `K-20`（1，run5）：`m17_backend::tests::a_pause_affects_exactly_one_batch`
- `K-21`（8，run5）：`m13_iss::tests_csr::a_compressed_illegal_instruction_writes_its_original_code_into_mtval` · `m13_iss::tests_csr::a_misaligned_load_reports_cause_four_and_the_faulting_address` · `m13_iss::tests_csr::a_misaligned_fetch_reports_cause_zero_and_the_faulting_address` · `m13_iss::tests_csr::a_misaligned_store_reports_cause_six_and_the_faulting_address` · `m13_iss::tests_csr::an_illegal_instruction_writes_the_instruction_word_into_mtval` · `m13_iss::tests_csr::entering_a_trap_writes_mepc_mcause_and_mtval` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table`
- `K-22`（3，run5）：`m13_iss::tests_csr::entering_a_trap_disables_interrupts_and_saves_the_previous_enable` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-23`（18，run5）：`kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest` · `m13_iss::tests_csr::an_illegal_instruction_writes_the_instruction_word_into_mtval` · `m13_iss::tests_csr::entering_a_trap_writes_mepc_mcause_and_mtval` · `m13_iss::tests_csr::the_direct_mode_jumps_to_the_vector_table_base` · `m13_iss::tests_csr::the_vectored_mode_jumps_to_the_cause_slot` · `m13_iss::tests_csr::the_vectored_mode_uses_the_interrupt_line_of_an_interrupt_cause` · `m13_iss::tests_rv32c::c_ebreak_raises_the_breakpoint_trap` · `m13_iss::tests_rv32c::the_reserved_and_hint_encodings_are_classified_like_the_reference` · `m13_iss::tests_rv32i::ecall_traps` · `m13_iss::tests_rv32i::misaligned_fetch_address_traps` · `m13_iss::tests_rv32i::reserved_compressed_encoding_traps_and_hands_the_pc_to_the_vector_table` · `m13_iss::tests_rv32i::illegal_instruction_leaves_the_state_untouched` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m17_backend::tests::writing_irq_clear_clears_the_pending_entry_and_resets_active_irq` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it` · `an_interrupt_is_delivered_only_after_the_first_quantum_boundary`
- `K-24`（15，run5）：`kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest` · `m13_iss::tests_csr::entering_a_trap_writes_mepc_mcause_and_mtval` · `m13_iss::tests_csr::an_illegal_instruction_writes_the_instruction_word_into_mtval` · `m13_iss::tests_rv32c::the_reserved_and_hint_encodings_are_classified_like_the_reference` · `m13_iss::tests_rv32c::c_ebreak_raises_the_breakpoint_trap` · `m13_iss::tests_rv32i::ecall_traps` · `m13_iss::tests_rv32i::illegal_instruction_leaves_the_state_untouched` · `m13_iss::tests_rv32i::misaligned_fetch_address_traps` · `m13_iss::tests_rv32i::reserved_compressed_encoding_traps_and_hands_the_pc_to_the_vector_table` · `m17_backend::tests::the_delivery_jumps_to_mtvec_and_records_the_cause` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `an_interrupt_is_delivered_only_after_the_first_quantum_boundary` · `the_delivery_writes_the_trap_csr_and_jumps_to_the_vector_table` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it` · `the_handler_returns_to_mepc`
- `K-25`（3，run5）：`m13_iss::tests_csr::mret_restores_the_global_interrupt_enable_from_mpie` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-26`（2，run5）：`m13_iss::tests_csr::a_misaligned_fetch_reports_cause_zero_and_the_faulting_address` · `m13_iss::tests_csr::the_three_misaligned_causes_carry_three_different_codes`
- `K-27`（2，run5）：`m13_iss::tests_csr::an_unknown_csr_reads_as_zero_without_trapping` · `m13_iss::tests_csr::the_read_only_forms_do_not_write_the_csr_at_all`
- `K-28`（6，run5）：`csr::tests::mip_is_read_only_by_the_platform_rule` · `csr::tests::the_coarser_read_only_rule_does_not_affect_any_csr_the_platform_uses` · `csr::tests::mtime_is_read_only_by_the_address_rule` · `m13_iss::tests_csr::a_write_to_a_read_only_csr_is_ignored` · `m13_iss::tests_csr::an_ignored_write_reports_nothing` · `m13_iss::tests_csr::a_write_to_mip_is_ignored`
- `K-29`（7，run5）：`csr::tests::the_timer_line_uses_the_timer_interrupt_bit` · `m16_interrupt::tests::export_and_load_round_trip_restores_the_whole_controller_state` · `m16_interrupt::tests::a_disabled_high_priority_line_does_not_block_an_enabled_low_priority_one` · `m16_interrupt::tests::the_delivery_cause_encodes_the_interrupt_bit_and_the_line` · `m16_interrupt::tests::the_quantum_boundary_refreshes_the_enable_mask_from_mie` · `m16_interrupt::tests::the_timer_raises_line_three_when_mtime_reaches_the_compare_value` · `m16_interrupt::tests::the_timer_raises_the_line_when_mtime_equals_the_compare_value`
- `K-30`（2，run5）：`session::tests::the_segment_5_virtual_time_fields_travel_as_decimal_strings` · `session::tests::exactly_eight_virtual_time_fields_carry_the_decimal_attribute`
- `K-31`（5，run5）：`session::tests::the_segment_5_field_order_is_pinned` · `session::tests::an_old_segment_5_payload_still_deserialises` · `session::tests::the_session_payload_shape_is_pinned_exactly` · `session::tests::the_segment_5_field_names_are_stable` · `m16_interrupt::tests::load_state_accepts_the_old_payload_shape`
- `K-32`（3，run5）：`the_firmware_bytes_are_still_the_ones_the_source_file_describes` · `the_handler_clears_the_line_and_resets_the_active_interrupt` · `the_same_interrupt_can_be_delivered_again_after_the_handler_clears_it`
- `K-33`（4，run5）：`m13_iss::tests_csr::an_unknown_csr_reads_as_zero_without_trapping` · `m13_iss::tests_csr::mtvec_is_readable_and_writable_although_its_low_bits_are_not_zero` · `m13_iss::tests_csr::the_read_only_forms_do_not_write_the_csr_at_all` · `m17_backend::tests::an_interrupt_is_delivered_only_at_a_quantum_boundary`
- `K-34`（18，run5）：`session::tests::a_gpio_with_exactly_one_irq_line_per_pin_is_accepted` · `session::tests::the_gpio_pin_count_is_the_single_landing_spot` · `session::tests::a_well_formed_payload_validates` · `session::tests::an_old_segment_5_payload_still_passes_the_validator` · `session::tests::the_default_active_irq_is_the_no_active_marker` · `session::tests::a_malformed_inner_engine_snapshot_is_rejected` · `session::tests::an_unknown_inner_state_version_is_rejected` · `m16_interrupt::tests::load_state_accepts_the_old_payload_shape` · `m16_interrupt::tests::load_state_replaces_the_whole_state` · `m20_snapshot::tests::capture_session_produces_a_payload_that_validates` · `m20_snapshot::tests::validate_session_accepts_a_well_formed_payload` · `m23_datastore::memory::tests::a_recorded_snapshot_is_stored_under_its_checkpoint_id` · `m23_datastore::memory::tests::a_snapshot_round_trips_field_by_field` · `m23_datastore::memory::tests::rewriting_the_same_id_with_different_content_overwrites` · `m23_datastore::memory::tests::rewriting_the_same_snapshot_is_idempotent` · `m23_datastore::memory::tests::snapshots_are_keyed_by_checkpoint_meta_id` · `m23_datastore::memory::tests::the_payload_survives_a_json_round_trip` · `m23_datastore::tests::the_trait_has_exactly_the_five_methods`
- `K-35`（2，run5）：`session::tests::a_duplicated_pending_line_is_rejected` · `session::tests::a_duplicated_line_with_distinct_times_keeps_the_queue_ordered`
- `K-36`（1，run5）：`session::tests::an_active_line_that_is_not_queued_is_rejected`
- `K-37`（2，run5）：`m13_iss::tests_csr::a_compressed_illegal_instruction_writes_its_original_code_into_mtval` · `m13_iss::tests_csr::an_illegal_instruction_writes_the_instruction_word_into_mtval`
- `K-38`（2，run6）：`m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote` · `m17_backend::tests::the_same_interrupt_is_delivered_again_after_mret`
- `K-39`（1，run6）：`m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote`
- `K-40`（1，run6）：`m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote`
- `K-41`（1，run6）：`m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote`
- `K-42`（1，run6）：`m13_iss::tests_csr::every_csr_instruction_reports_the_csr_number_it_wrote`
- `K-43`（1，run5）：`session::tests::an_out_of_order_pending_queue_is_rejected`
- `K-44`（26，run5）：`session::tests::a_well_formed_segment_5_is_accepted` · `session::tests::the_gpio_pin_count_is_the_single_landing_spot` · `session::tests::an_old_segment_5_payload_still_passes_the_validator` · `session::tests::a_gpio_with_exactly_one_irq_line_per_pin_is_accepted` · `session::tests::a_well_formed_payload_validates` · `session::tests::a_raised_line_without_a_queue_entry_is_accepted` · `session::tests::a_malformed_inner_engine_snapshot_is_rejected` · `session::tests::an_unknown_inner_state_version_is_rejected` · `kernel_host::tests_session_snapshot::a_failing_topology_step_leaves_the_bus_untouched` · `kernel_host::tests_session_snapshot::a_successful_restore_puts_every_field_back` · `kernel_host::tests_session_snapshot::capture_session_assembles_the_six_segments_from_their_sources` · `kernel_host::tests_session_snapshot::capture_session_carries_the_raised_irq_lines` · `kernel_host::tests_session_snapshot::restore_session_rebuilds_the_memory_map_topology` · `kernel_host::tests_session_snapshot::restore_session_rejects_an_unknown_peripheral_id_without_touching_the_peripheral_state` · `kernel_host::tests_session_snapshot::restore_session_rejects_a_malformed_uart_state_without_touching_the_bus` · `kernel_host::tests_session_snapshot::the_restore_path_reports_the_same_contention_error_as_the_delegate_path` · `kernel_host::tests_session_snapshot::a_restored_session_reproduces_the_captured_payload_segment_by_segment` · `m20_snapshot::tests::capture_session_produces_a_payload_that_validates` · `m20_snapshot::tests::validate_session_accepts_a_well_formed_payload` · `m23_datastore::memory::tests::a_snapshot_round_trips_field_by_field` · `m23_datastore::memory::tests::a_recorded_snapshot_is_stored_under_its_checkpoint_id` · `m23_datastore::memory::tests::rewriting_the_same_snapshot_is_idempotent` · `m23_datastore::memory::tests::snapshots_are_keyed_by_checkpoint_meta_id` · `m23_datastore::memory::tests::rewriting_the_same_id_with_different_content_overwrites` · `m23_datastore::tests::the_trait_has_exactly_the_five_methods` · `m23_datastore::memory::tests::the_payload_survives_a_json_round_trip`

</details>
<!-- MUT-TABLE:END -->

### 3.10.5 本轮方法论沉淀

18. ⚠️ **「变异必须能编译」这条纪律，本轮真的抓到了一类变异规格 bug** —— 五条逐臂变异
    （**K-38 ~ K-42**）的替换文本里把 `csr::csr_read(state, number)` 写成了裸
    `csr_read(state, number)` —— 而 `executor.rs` **只**直接导入了
    `csr_write_is_ignored`（`csr_read` 只能走 `csr::csr_read`）⇒ **编译失败**
    ⇒ 变异**无效**（编译错误既不算判别力、也不算「绿」）⇒ 实测判 **INVALID**。
    ⇒ **教训**：变异规格本身也是**代码**，也要过编译器 —— 而**「编译失败」这条判据
    只能对**变异文本**生效**：如果变异文本自己写错了函数名，得到的不是「用例没抓住」
    而是「**这条变异根本没跑起来**」⇒ **两者必须分开记账**，否则会把「规格写错」
    误记成「用例没判别力」⇒ 补变异补了**半天却补错了地方**。
    ⇒ 本轮已修（`csr::csr_read`）并**重跑这 5 条**。
1. **两路检视员不许跑 cargo**：并行 lane 共享 `CARGO_TARGET_DIR` 会互相换产物
   （本批实测**两次**假象）⇒ 检视员交付「**证据 + 变异三元组**」，由父会话**统一**跑成
   一张可审计的红绿表。
2. **判别力的「最坏形态」不是「没覆盖」，是「把缺陷钉成期望」**：F-1 的两条用例**不是漏
   了**，是**主动**断言了错误的行为 ⇒ 补断言时必须问「这条断言在缺陷路径上**也**成立
   吗」。
3. **源码级推演要标注**：V-A / V-B 都对 F-1 做了**源码级推演**并**明确标注「我没能执行」**
   —— 父会话**实跑验证**后才落「已闭环」。⚠️ **没有执行过的推演不算证据**。
4. **工具链自己也要被检视**：红集比对 / 超时 / 备份键 —— 三处缺陷都会让结论**假绿**。
5. **「值变了」与「写了」不是一回事**：平台侧的 CSR 副作用判据必须是「**这条指令写了
   这个 CSR**」—— 「值变了」在「重复写同一个值」时**静默丢弃**请求。
6. **HANG 是独立的判定类别，而且「判成 HANG」这件事本身**要先被验证**：挂死
   **不是红也不是绿**（既不证明有判别力，也不证明没有）⇒ 单独一档。⚠️ **不能事后为了
   让它变绿而改成「预期」** —— 但**更前置的一条**是：**观测本身**必须可靠。
   ⇒ 本轮踩中：旧口径把 **K-17** 判成挂死，而新口径下它判 **RED ✓** ⇒ 那次
   「挂死」是**假 HANG**（见第 13 条）⇒ 登记「预期 HANG」必须给出**同形态的实证**
   （同一个缺陷在另一条变异上以同一形态挂死），**且**该实证**本身**必须是
   **已验证**的挂死。
7. ⚠️ **假观测污染的是「上层结论」，不只是「判错一条」** —— 本轮最刺眼的一条教训：
   旧口径把 **K-17**（把 `irq_clear` 的副作用**整个拆掉**）判成**挂死** ⇒ 我据此
   在本节与 `AGENTS.md` 写下「**F-1 的实际后果是平台活锁**」，并把「描述缺陷时
   **『会让什么挂死』比『会丢掉什么』更有信息量**」当成本轮的方法论。
   ⇒ 新口径下 **K-17 判 RED ✓** ⇒ **那个结论是错的**。F-1 的实际后果是：
   **第二次清线被丢弃** ⇒ `active_irq` 永不复位 ⇒ **那条线被单级防护永久堵死**
   （该线再也交付不了任何中断）—— 在**有界的测试**里表现为**用例红**（第二次
   交付没来），**平台不会挂死**。
   ⇒ ⚠️ **该结论已由 `run5` 全量重跑独立复现**：K-17 的红集与 `run4` **逐条相同**
   （4 条：2 条 `m17_backend` + 2 条端到端）⇒ 换一套 target 集、换一套驱动，
   **结论不变** ⇒ 「F-1 的后果**不是**平台活锁」这条现在有**两轮独立实测**支撑。
   ⇒ 真正会挂死的是 **K-10**（`take_intr` **取走不消失** ⇒ 同一交付被反复交付
   ⇒ 交付**不消耗指令预算** ⇒ 引擎内层循环**不退出**）—— 那才是 **D-83** 的
   「生产侧」那条（`ExecOutcome::InterruptTaken` 不消耗预算、不推进时间 ⇒
   无界自旋）。
   ⇒ **教训**：缺陷的**后果描述**会被写进**台账**、**方法论**、**AGENTS.md** ⇒
   它依赖的**观测**必须**先验证**；一条**未验证**的观测（尤其是**假 HANG**）
   会把**因果描述**和**方法论**一起带偏 —— 污染的**层级**比「判错一条变异」高得多。
   ⚠️ **但 K-18 / K-18b 并不是挂死** —— 它们只命中 **CSRRW 一条臂** ⇒ 端到端那条
   **红**、M-17 那条**不红** ⇒ 正是这个「**缺**」把「六条臂裸奔」暴露了出来
   （**F-13**）⇒ **同一条变异的不同结果，含义完全不同**。
8. **验收清单与变异集要逐条对账**：把冻结规格 §6 的 33 项验收**逐条**映射到变异条目
   后，发现 4 项**只有正向用例、没有反向变异**（#21 未知 CSR 读 0 · #2 `active_irq`
   默认值 · #4 `validate()` 的两条拒绝 · #24 非法指令 `mtval`）⇒ 「有测试」和「这个测试
   **有判别力**」是两件事 ⇒ 补了 K-33~K-37。⚠️ **收口轮再对一次账，又找出 2 项**
   （#4 的第三条拒绝「顺序错乱」· #5「接受旧形状」）⇒ 补 **K-43 / K-44** ⇒ 变异集
   最终 **49 条**。⚠️ **更要紧的是：两次对账各自证伪了一个「疑似缺口」** ——
   验收 #9「未使能的高优先级不阻塞」一度看着像缺口，**实际已被 K-08 覆盖**；
   验收 #18 的「`rd == x0` 不读」在本平台**不可观测**（CSR 读**无副作用** ⇒
   「读了丢弃」与「不读」**行为等价**）⇒ 属**规格**该写明的实现事实。
   ⇒ **「疑似缺口」必须用实际红集核实** —— 凭变异描述推断会**误登记**
   （把「有正向用例」当成「有判别力」），也会**误判缺口**（把「已被别的变异
   覆盖」当成「没人管」）。
9. **「变异覆盖面」与「用例判别力」是两件事，读结果前先问「这条变异打在**哪**」**：K-18b
   缺 M-17 那条用例时，最省事的误判是「M-17 那条没判别力」；真相是**变异只落在一条臂
   上** —— 六条 CSR 指令的「写 + 回报」片段**逐字相同**，而驱动器的 `replace(old, new, 1)`
   **只命中首个匹配** ⇒ 落在 CSRRW 臂上，而 M-17 的固件用的是 **CSRWI** 臂 ⇒ **那条用例
   压根没被这条变异影响**。⇒ **逐字相同的代码段是变异的陷阱**：锚点必须**唯一定位到那条
   臂**（带上前一行区分上下文），不能只靠「首个匹配」。⇒ 判定「某条用例没判别力」之前，
   先确认**那条变异打在了它走的路径上**。
10. ⚠️ **变异跑期间绝不跑 cargo** —— 本批因共享 `CARGO_TARGET_DIR` 出**三次**假象：
    前两次是**并行 lane** 互相换产物（已写进门禁脚本的注释），**第三次是父会话自己**：
    变异驱动正在跑 `cargo test --workspace` 时，我另起了一条 `cargo test --lib` 去验证
    新加的单测 ⇒ 两个 cargo **同时写同一个 `target/debug/deps/`** ⇒ 测试二进制被**互相
    替换** ⇒ 紧邻的 **K-21** 被判 **HANG**（活锁），而它的变异（`enter_trap` 不写
    `mtval`）**根本不可能活锁** ⇒ **该判定不可信，必须在干净的串行环境里重跑**。
    ⇒ 教训与前两次同源：**「跑出来的结果」要先问「跑的环境干不干净」**。
11. ⚠️ **守护脚本的「等待条件」本身也会误匹配** —— 第一代接力守护用
    `pgrep -f "k_mut[.]py"` 判断「驱动是否还在跑」，结果它**匹配到了父会话自己
    的等待任务 shell**（那些 `sleep N; … grep … k_mut_v19_run2.log` 的命令行里
    含同样的路径）⇒ 接力**永远堵在等待循环里**，续跑**压根没启动**，而表面上
    「进程还在、看起来正常」。⚠️ 同样的坑在**超时口径**上也有：驱动的
    `proc.communicate(timeout=…)` 是**从 cargo 启动算起**，**编译时间被算进了
    运行超时** ⇒ 「HANG」可能是**编译慢被误判**；而且超时时驱动**丢弃了
    cargo 的输出** ⇒ 事后**无法分辨**它停在「Compiling」还是「Running tests」。
    ⇒ 本轮修掉两处：① 接力改成「续跑**前台**执行、循环天然串行」（**不用
    pgrep 等待**）；② 驱动在超时时把 cargo 输出**落盘**
    （`/tmp/probe/hang_out.log`）⇒ `cargo test` 会逐条打印测试名 ⇒ 挂死时
    **最后一行就是挂死的那个测试**，能直接定位。
    ⇒ 教训：**「等 A 结束再跑 B」的判据，要保证它只匹配 A** —— 宁可**不判断**
    （前台串行天然无并发），也不要写一个会误匹配的 `pgrep`。
12. ⚠️ **「HANG」必须先排除「编译慢」与「并发启动堵塞」再下结论** —— 本轮**实测踩中**：
    连续 **9 条**变异判 HANG（K-10 / K-17 / K-18 / K-21 / K-22 / K-24 / K-26 / K-27 /
    K-28），其中 **K-26**（只改 `mcause()` 的一个数值）与 **K-27**（只让纯读多写一次
    **相同**的值）按语义**绝不该活锁**。⇒ 落盘 cargo 输出后真相大白：**所有**单元
    测试与**全部**集成测试**都跑完了并通过**（含 contract 91 passed/3 failed、kernel
    679 passed/3 failed、golden 11、d07 31、d46 15、端到端 6 …），输出停在
    **最后一个** target 的「Running」行 ⇒ 不是被测代码挂死，是 **cargo 并行启动
    12 个 test binary**、而每次变异都重新链接出**全新二进制** ⇒ macOS 的
    **XProtect 扫描**偶发堵塞（`top` 里它正处在 `Us` 不可中断状态）。
    ⇒ **修法**：把**编译与运行的超时分开**（`cargo build --tests` 单独走、
    给它宽松的 1200 s；`cargo test` 的超时**只覆盖运行**）⇒ **HANG 判定恢复精确**。
    ⇒ **实测复核**：K-28 在新驱动下判 **RED ✓**（红集含**新加**的
    `an_ignored_write_reports_nothing`）⇒ 那批 HANG 里**至少有一条**是**假 HANG**
    ⇒ **旧口径下的 HANG 判定整体不可信**，必须**逐条重跑**才能定案。
    ⇒ 教训：**「挂死」这个结论，要先证明「不是编译慢、不是环境堵」** —— 判据是
    **把 cargo 的输出落盘**，看它停在「Compiling」还是「Running tests」。　⇒ ⚠️ **本条的判据已被第 17 条取代**（本条是**当时**的判据；根因与最终判据见第 17 条）。
13. ⚠️ **「HANG」有三种形态，分不清就会把「没结论」写成「活锁」** ——
    **确凿活锁**（cargo 打印了 `test … has been running for over 60 seconds`
    ⇒ **点名了**挂死的那个测试）· **超时未跑完（未证实挂死）**（某个 target 在
    超时窗口内没出结果 ⇒ **可能只是慢**）· **假 HANG**（**所有 target 都已跑完**、
    卡在**退出**阶段）。⇒ 判据（只用输出，**不看时间**）：数 cargo 输出里
    `Running <target>` 与 `test result:` 的个数 —— **相等** ⇒ 假 HANG；
    **不等** ⇒ 超时未跑完 ⇒ 再看有没有 cargo 的**挂死提示**那行 ⇒ 有 ⇒ 确凿活锁。
    ⇒ 假 HANG 的根因：doc-test 二进制**每次变异都被重新链接**
    （全新二进制）⇒ macOS **XProtect 扫描**偶发把它卡住 ⇒ cargo 停在
    `Doc-tests …` 的 Running 行 ⇒ **假 HANG**。⇒ 修法两条：**跳过 doc-test**
    （`cargo test --lib --tests`；doc-test 只跑**文档示例**，**不覆盖**本批任何
    一条变异 ⇒ **不损失判别力**，只消除假阳性）+ **按条目落盘** hang 输出
    （不再共用一个文件，否则「后一条覆盖前一条」⇒ 事后无从核对）。
    ⇒ **实证（本轮最强的一条）**：**同一条变异 K-19**（`last_poll_ps` 每批次重置）
    在 `log1` 判 **RED ✓**、在 `rerun` 判 **HANG** ⇒ 同一变异两个结果 ⇒
    **旧口径下的 HANG 判定整体不可信**。⇒ 教训：**「同一条变异两轮两个结果」是
    工具链有问题的最强信号** —— 比「跑了 N 条都绿」更能说明问题。
    ⇒ 顺带印证一条老教训：**「活锁」这个结论，必须能指出「是哪个测试二进制
    挂死的」**，否则那只是「cargo 没退出」。　⇒ ⚠️ **本条的判据已被第 17 条取代**（本条是**当时**的判据；根因与最终判据见第 17 条）。
14. ⚠️ **变异跑期间改源文件 ⇒ 结论混源** —— 本轮我**在变异驱动正在跑的当口**把
    `every_csr…` 从「各臂写一次」改成「**每条臂写两次**（第二次同值）」并新增
    `a_repeated_write…` ⇒ `rerun` 那一轮的 K-18 / K-18b 编译到的是**旧版测试集**
    ⇒ 它们的结论**不成立**（K-18b 抓不住 `every_csr…`，正是因为旧版只写一次 ⇒
    第一次写时值确实变了 ⇒「值变了才回报」的退化**抓不住**）。
    ⇒ 判据：**一张可审计的红绿表里，每一行都必须出自同一版测试集** ⇒
    表里必须带「**实测轮次**」列；受污染的条目**重跑**，**不重跑整批**。
    ⇒ 与第 10 条同源但更隐蔽：第 10 条是「**别在变异跑时跑 cargo**」，
    这一条是「**别在变异跑时改源码**」—— 后者**不报错、不留痕**，只会让结论
    **悄悄变味**。
15. ⚠️ **杀进程时不要用「含目标字符串」的模式去 grep** —— 本轮收尾时我用了
    `ps | grep "^ *[0-9]* /.*cargo test"` 来找残留 cargo ⇒ 这条模式**匹配到了
    我自己的 shell 命令行**（命令行里含 `cargo test` 这几个字）⇒ `kill -9`
    把自己**连带杀掉了**（SIGKILL）⇒ 刚启动的续跑脚本也一并乱掉 ⇒
    **一度起了两个并发 cargo**（又是污染）。
    ⇒ 判据：**按 `comm`（进程名）匹配，不要按整行 `args` 匹配**；要按命令行
    匹配时，**模式必须匹配不到发起命令的那个 shell 自己**。
    ⇒ 与第 11 条（守护脚本的等待条件会误匹配）**同源**：
    **凡是「用命令行文本做判据」的地方，都要先问「会不会匹配到自己」**。
16. ⚠️ **「超时窗口内没跑完」≠「挂死」** —— 判据必须能区分「**卡住**」与
    「**慢**」。⇒ cargo 在测试跑过 60 s 时会打印
    `test <name> has been running for over 60 seconds` ⇒ **有**这行 =
    **确凿的活锁**（cargo **点名了**挂死的那个测试）；**无**这行 = 只能断定
    「**在超时窗口内没跑完**」⇒ **可能只是慢**，**不构成活锁证据**。
    ⇒ 本轮实证：**K-10** **有**那行 ⇒ 挂死点 =
    `kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest`
    ⇒ **确凿**；而 **K-18 / K-19 / K-21 / K-24** **没有** ⇒ 且 **K-19** 在 `log1`
    （旧驱动）判过 **RED ✓**（**跑完了**）⇒ **同一条变异两个结果** ⇒ 只能
    归为「**慢**」⇒ 用 **900 s** 超时**重跑**（定性轮）才算有结论。
    ⚠️ **「最后启动的 target」≠「挂死的 target」** —— cargo **并行**启动多个
    test binary ⇒ 超时时输出停在的那个 target 只是**调度顺序上的最后一个**，
    **未必是卡住的那个**。⇒ 最硬的反证是 **K-21**：输出停在
    `Running unittests src/main.rs`（`riscv_sim_server`）—— 那个 target
    **一个测试都没有**（`running 0 tests`）⇒ **它不可能是挂死的那个**
    ⇒ 判「活锁」显然错了。⇒ 教训：**「超时」这个观测，粒度太粗** ⇒ 登记任何
    「活锁」结论前先问「**是卡住了，还是只是慢**」—— 判据是
    **cargo 自己点的那一行**，**别无他法**。
    ⇒ 顺带一条口径：**已被旧口径判为 RED ✓ 的条目不需要重跑**（doc-test 与
    超时口径只影响 **HANG 的判定**，不影响红集判定）⇒ 例如 **K-20** 保留
    `log1` 的结论，不进重跑清单。
17. ⚠️⚠️ **「假 HANG」的根因查到了，而且判据可以更硬：看二进制有没有**启动** ——
    本轮收尾把 8 条「HANG」逐条**落盘**核对，得到**决定性反证**：
    **7 条的末个 target，其测试二进制**压根没启动**（输出里**没有**它自己打印的
    `running N tests` 行）⇒ 而这些末个 target 是 `unittests src/main.rs`
    （`riscv_sim_server`，**0 个测试**）与 `unittests src/bin/riscv-golden.rs`
    （**0 个测试**）—— **0 个测试的 target 不可能挂死** ⇒ 那 7 条**必然**是
    **假 HANG**，**与被测代码无关**。⇒ **只有 K-10 是真活锁**（它的二进制**启动过**，
    且 cargo **点名了** `kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest`）。
    ⇒ **根因**：macOS 对**刚链接出来**的可执行文件做一次安全扫描，被扫描的 `exec`
    会被**阻塞**；实测**每个新二进制固定 ≈25 秒**（12 个二进制从 3.2 MB 到 8.9 MB
    **都是 23~26 秒** ⇒ **与体积无关**，是**每文件固定成本**）⇒ 一轮 12 个二进制
    = **300 秒** ⇒ **正好打满**旧口径的 300 s 运行窗口 ⇒ **系统性**地产出假 HANG。
    ⇒ **最终判据（三形态，只看输出）**：
    **确凿活锁** = 末个 target 的二进制**自己打印了** `running N tests`
    **且** cargo 打印了 `has been running for over 60 seconds`（**点名了**测试）；
    **假 HANG** = 末个 target 的二进制**压根没启动**（无 `running N tests` 行）
    ⇒ exec 被系统阻塞；**超时未跑完** = 二进制启动了但 cargo **没点名**
    ⇒ **未证实挂死**（可能只是慢）。
    ⇒ **三招缓解**（本轮已落地）：① **收敛 target 集** —— 44 个期望红测逐条
    `grep -rl "fn <名>" crates/` 核实，**41 个在 lib**、**3 个在 `intc_end_to_end`**、
    **零个**在别的 target ⇒ 每轮只重链 **3 个**二进制 ⇒ 扫描成本 300 s → **75 s**；
    ② **预热**：正式跑之前先 exec 一次（`--list`，只枚举不执行）⇒ 正式跑时 exec
    已是「快」的（实测 **25 s → 0.02 s**）；③ 运行超时**放宽到 900 s**（内核 lib
    本身只要 0.47 s ⇒ 300 s 绝不够）。
    ⚠️ **走不通的一条**（别再试）：`xattr -d com.apple.provenance <binary>` **无效**
    —— 那是**受保护的系统属性**，删除后 `xattr -l` **仍在** ⇒ 首次 exec 依旧被阻塞
    （实测 1m44s）；`strip` 掉调试信息也**没用**（扫描成本与体积无关）。
    ⇒ 教训：**「超时」这个观测，粒度太粗** ⇒ 判活锁要看「**二进制启动没有**」
    这一个**更细**的信号 —— 它把「被测代码挂死」与「系统堵住 exec」**彻底分开**。

---

## §3.11 M-11 符号 / DWARF / 反汇编 / trace 导出落地检视轮（2026-10-22 · v0.20）

### 3.11.1 两路独立检视的范围划分

| 检视员 | 范围 | 判决 |
| --- | --- | --- |
| **V-A** | **实现正确性**（M-11 五个子模块 + 宿主接线 + M-17 的时间戳接线 + 契约形状） | **PASS**（无阻断级；3 条「重要」+ 5 条「次要」，全部**潜在/边缘路径**） |
| **V-B** | **判别力与门禁一致性**（22 条验收逐条对账 · 假 HANG 风险 · 三处一致性 · 门禁诚实性 · 已知边界） | **PARTIAL**（2 条高危判别力缺口 + 3 条中危 + 3 条低危） |

⚠️ **V-A 自带一条独立的黄金对拍**：用仓库外的 xPack RISC-V GCC 15.2.0
（`riscv-none-elf-objdump -d -M no-aliases`）对着**真 wasm 产物**做对拍 ⇒
12 条指令的 `addr` / `code` / `width` **12/12 精确一致**，助记符文本仅 **2 处**
与 objdump 不同（`c.nop` vs `c.addi zero,0`；`c.addi` vs `c.addi16sp`——**渲染器
已自陈取舍**，见 `render.rs`）。

⚠️ **V-B 自带 6 处变异实验**（在 `/tmp/vrf*` 的**快照副本**上做，**不碰工作区**），
其中两处**实测**了「改坏实现而全绿」。

### 3.11.2 本批的**主线教训**：**「接上了」和「钉住了」是两件事**

本批最要紧的一条不是任何单个缺陷，而是**两种失配**同时出现：

1. **接缝存在但没人用**（D-50 / D-51 / `SymbolService` 端口）：
   实现齐了、端口定义了、命令也接上了，但**数据源不是它** / **没人调它** ⇒
   「实现了 ≠ 接上了」的老形态，**换了个马甲**又出现一次。
2. **接上了但没被钉住**（V-B 的两条高危）：
   `Waveform+Vcd` 导出改成 CSV、波形导出的 `overflow_marked` 改成 `false`
   ⇒ **795 + 5 条用例全绿** ⇒ 「合法组合」被掏空成「换个格式凑合」、
   「导出窗口不完整」在 `Waveform` 一侧**没人看得见**。

⇒ **判别力的落点必须在「被依赖方自己身上」**：V-B 的两条高危都落在
**宿主的映射函数**上 ⇒ 处置不是「加一条端到端用例」，而是
**把映射抽成可单独钉住的自由函数**（`waveform_export_event`）+ **造一个
人造的 `ExportHandle` 喂进去**。

### 3.11.3 闭环对照表

| # | 发现 | 来源 | 严重度 | 处置 | 判别式 |
| --- | --- | --- | --- | --- | --- |
| 1 | **访存环溢出会让反汇编缓存的失效通道有损**（M-18 的环有界 ⇒ 写事件被丢 ⇒ 缓存不失效 ⇒ **反汇编给出旧机器码**） | V-A 重要 1 | 重要 | ✅ **已修**：`StepN` 按指令预算（10 000）**分块** ⇒ 单块事件数 ≤ 50 000 ≪ 200 000 ⇒ 通道永远无损；顺带消掉「`StepN` 无上界 vs `Continue` 一轮 10 000」的上界不对称 | `a_long_step_batch_keeps_the_invalidation_channel_lossless`（断言 `probe_trace_dropped() == 0`）+ 变异 **V-14** |
| 2 | **`restore_session` 还原内存却不重建 M-11 的索引** ⇒ `Disasm` 给出**上个会话**的机器码、轨迹环混着两个会话而 `overflow_marked` 仍说「没溢出」 | V-A 重要 2 | 重要 | ✅ **已修**：还原后**作废**反汇编缓存与轨迹环（**符号表与行表是「工程级」的 ⇒ 保留**）——与 STAGE-5 §9.2 是**同一条规矩** | `restoring_a_session_invalidates_the_m11_session_scoped_state` + 变异 **V-15** |
| 3 | **取指口径有两份实现**（M-11 一份、宿主一份） | V-A 次要 6 | 次要 | ✅ **已修**：抽成 M-11 的 `disasm::fetch_instruction_code`（**唯一实现**），宿主与 `disasm` 共用 | `the_trace_instruction_code_matches_the_disassembly_entry` + 变异 **V-16** |
| 4 | **`Waveform+Vcd` 导出改成 CSV，795+5 全绿** | V-B D-1 | 高 | ✅ **已修**：补 `the_waveform_vcd_export_produces_a_vcd_product`（钉 VCD 的**形状**：时间标度 + `$var` + `$enddefinitions`） | 该用例 + 变异 **V-17** |
| 5 | **波形导出的 `overflow_marked` 改成 `false`，795+5 全绿** | V-B D-2 | 高 | ✅ **已修**：把 M-19 `ExportHandle` → 契约事件的映射**抽成自由函数** `waveform_export_event` + 补 `the_waveform_export_event_propagates_the_overflow_marker`（喂**人造** handle） | 该用例 + 变异 **V-18** |
| 6 | **`SymbolService` 端口全仓 0 处实现** ⇒「按地址查符号」没有生产路径 | V-B D-4 | 中 | ✅ **已修**：`impl SymbolService for SymbolTable` + 宿主走端口（`lookup_symbol` / **新增** `symbol_at`） | `the_host_can_look_up_a_symbol_by_address` + 变异 **V-19** |
| 7 | **前端契约门禁不比对 `DisasmEntryDto` 的字段名** ⇒ Rust 改名而 JS 忘改 ⇒ **静默** | V-B D-3 | 中 | ✅ **已修**：新增 `disasmEntry` 构造器 + 门禁喂**非空**数组比字段名。**实测**：把 Rust 侧 `mnemonic` 改名成 `mnem` ⇒ 前端门禁**如期变红** | `front/tests/contract.test.mjs` 的「newtype 变体携带的 DTO 字段名与 Rust 一致」 |
| 8 | **门禁脚本硬编码契约版本号**（`wasm-smoke.mjs` 的 `2`、`contract.test.mjs` 的 `3`、`wasm-smoke.mjs` 的 CSV 表头） | V-B D-7 | 低 | ✅ **已修**：三处全部改成**从契约层源码推导**（`rustSchemaVersion()` / `rustTraceCsvHeader()`）——「一个会自己坏的断言比没有还糟」 | 源码推导本身即判别式（升版时**不该**变红） |
| 9 | **访存轨迹的时间戳恒为 0**（M-14 的 `Bus::set_now_ps` 文档写着「read / write callers should call this first」，但**生产路径从未调用** ⇒ 导出的时间列全是 `0` ⇒ **时间轴是平的**） | 收口自查 1 | 重要 | ✅ **已修**：M-17 在**指令边界**补上 `self.bus.set_now_ps(now_ps)`（口径：一条指令 = 一个时间戳） | `bus_accesses_carry_the_virtual_time` + `the_execution_trace_carries_real_timestamps` + 变异 **V-09** |
| 10 | **反汇编缓存缺按地址查询的 API**（规格 §9.4 写着「`disasmCache` 可以兜住（重复取指）」，可 `Disassembler` **只暴露 `cache_len()`** ⇒ 那句话只是一句散文） | 收口自查 2 | 中 | ✅ **已修**：补 `lookup(addr)` + **可观测**的 `lookup_hits()` 计数；宿主的取指路径**复用**该缓存（规格 §9.4 的承诺**有落点**） | `lookup_serves_the_cached_entry_and_counts_the_hits` + `the_disasm_cache_absorbs_the_repeated_fetch` + 变异 **V-12 / V-13 / V-11**（三条：查不到 / 命中不计数 / 取指不复用） |
| 11 | **缓存失效被错误地挂在 trace 记录开关上**（缓存失效是**正确性**、记录开关是**性能** ⇒ 两者耦合 ⇒ 关闭 trace 期间访客自改代码 ⇒ 缓存**陈旧** ⇒ 用户看到**旧的**反汇编） | 收口自查 3 | 重要 | ✅ **已修**：**失效始终执行**，**只有 `record`** 受开关管——⚠️ 这是**把实现改回规格 §9.7 的原话**（「零开销」的口径**只覆盖 `record`**），**不是**改规格 | `the_trace_switch_does_not_disable_the_cache_invalidation` + 变异 **V-10** |
| 12 | **「访存环不许溢出」那条用例是**假绿**（夹具的手算机器码错了 ⇒ 根本跑不到 25 万条）** | 收口轮 · 变异自检 **V-14 存活** | 重要 | ✅ **已修**：夹具 `SELF_LOOP` 原为 `0xFE00_0FE3`，注释说「`beq x0, x0, -4`」，**实解为 `beq x0, x0, -2`** ⇒ 分支目标 `pc − 2`（**未对齐 + 未映射**）⇒ 访客**第 2 条指令就陷入** ⇒ `StepN(250_000)` 实际只跑 **2 条** ⇒ 访存环（20 万容量）**永远填不满** ⇒ `probe_trace_dropped() == 0` **恒成立** ⇒ 变异「`StepN` 不分块」**杀不死它** ⇒ 这条用例**整批都没在测它声称的东西**。⇒ 改成 `beq x0, x0, 0`（`0x0000_0063`，B 型立即数全零 ⇒ 无手算歧义；⚠️ 「跳回自己」的偏移是 **0**，不是 `-4`——`-4` 会跳到**上一条**指令）⇒ 实测 `elapsed_ps = 250 000 000`（**真跑满 25 万条**）⇒ 变异 **V-14 由 GREEN 存活变 RED ✓** | `a_long_step_batch_keeps_the_invalidation_channel_lossless` + 变异 **V-14**（修前 **GREEN 存活** / 修后 **RED ✓**） |
| 14 | **访存轨迹的机器码可能「时间倒流」**（取指的 `data` 是批次末重取的码） | V-A 重要 3 | 重要 | ⏳ **留账 D-87**（P1，根因同「环不记值」）⇒ **如实登记**进规格 §9.8 第 6 行 | — |
| 14 | **M-13 的执行取指恒读 4 字节** ⇒ 映射区末端的压缩指令**反汇编能解、访客取指失败** | V-A 次要 6 | 次要 | ⏳ **留账 D-86**（P1，不一致在 M-13）⇒ 登记进规格 §9.8 | — |
| 15 | **前端导出没有「下载」动作**（只落在 `<pre>` 里） | V-B D-6 | 低 | ⏳ **留账 D-88**（P2）⇒ 登记进规格 §9.8 第 8 行 | — |
| 16 | **裁决 G 的「只失效被写地址」在部分写下粒度不足**（写 `X+2` 不删 `cache[X]`） | V-A 次要 4 | 次要 | ⏳ **留账 D-89**（P2 · **规格问题不是实现走样**）⇒ 登记进规格 §9.8 第 7 行 | — |
| 17 | **`limit` 只对 `Execution` 路由生效 / `Waveform` 的 `offset` 是回显请求值** | V-B D-5 | 中 | ⏳ **如实登记**进规格 §9.8 第 4 / 5 行（不伪造 limit 是**对的**，但没登记就是**隐瞒**） | — |

⚠️ **本轮共 12 条已修**（两路检视 **8** 条 + 收口自查 **3** 条 + **收口轮变异自检 1** 条）+ **4 条留账（D-86 ~ D-89）** + **1 条规格登记** = **20 条发现**（与上方汇总表「v0.20 批新增」行**对齐**）。
⚠️ **另有 1 条**登记为 **D-90**：它是**历史发现（台账 🟡-4）已闭环后派生出的工作量单**（`riscv-golden` 的迷你 ELF 符号读取器待切到 M-11）⇒ 按「**两级计数的口径**」**不计入**本表的 19 条发现，也**不回填**未闭环格。
⚠️ **已修的 12 条全部有反向变异**：其中 **10 条**有仓库内变异（**V-09 ~ V-19**），**2 条**（前端侧两条）用**门禁脚本的手工变异实测** ⇒
「修完了」与「钉住了」是**同一件事**。

### 3.11.4 本轮方法论沉淀

1. **「合法」不等于「是那种格式」**——V-B 的第一条高危就是这条：
   规格写「`format=Vcd` ⇒ 合法」，实现「合法」地返回了 CSV ⇒ **全绿**。
   ⇒ 验收条目要写「**产出那种形状**」，并让用例**断言形状**（`$var` / `$enddefinitions`）。
2. **「窗口不完整」的标记必须被单独钉住**——它在 `Execution` 一侧有断言
   （环溢出用例），在 `Waveform` 一侧**没有** ⇒ **同一个字段，两种待遇**。
   ⇒ 凡是**同一个量在两条路径上都有**的地方，**两条路径都要有断言**。
3. **判别力的落点要在「被依赖方自己身上」**——把映射抽成自由函数 +
   喂人造输入，比补端到端用例**更省**也**更硬**。
4. **门禁脚本里不许抄常量**（版本号 / 表头 / 字段名）——抄的那份**必然**在某次
   升版时失配，而失配的方向要么**误红**（人于是去改断言或删门禁）要么**静默**。
   ⇒ 三处抄写全部改成**从源码推导**。
5. **「实现齐了 ≠ 接上了」的第三种形态**：`SymbolService` 端口**定义了**、
   `SymbolTable` **实现了**能力，但**没人调端口** ⇒ 与 D-50 / D-51 同形。
   ⇒ 端口**必须有实现者**（`impl`）**也必须有调用者**（宿主走端口）。
6. **检视员要能在隔离副本上做变异**——V-B 在 `/tmp/vrf*` 跑 6 处变异而**不碰工作区**
   ⇒ 与父会话的变异自检**并行**互不干扰（⚠️ 但**必须**是**快照副本**：
   V-B 自己就撞到过「工作树被并发修改」的提示 ⇒ 它的结论是**在当前树上重跑复核**过的）。
7. **判别力缺口要「实测」而不是「读代码」**——V-B 的两条高危都是**实测**出来的
   （改坏 ⇒ 795+5 全绿），不是推演的 ⇒ 「读代码看不出有没有判别力」。
8. **夹具的机器码手算错 ⇒ 用例整批都在空转**（本轮 **V-14 存活**当场暴露）——
   固件注释写「`beq x0, x0, -4`」，实解是 `beq x0, x0, -2` ⇒ 访客**第 2 条指令
   就陷入** ⇒ `StepN(250_000)` 只跑 2 条 ⇒ 「访存环不许溢出」那条断言**恒成立**
   ⇒ 用例**假绿**，而且**整批没人发现**。
   ⇒ **「断言恒成立」是假绿的典型形态**：它看起来在守一条边界，实际上一条
   边界也没碰到。
   ⇒ 判别式：夹具本身**必须**有「它真的跑满了」的旁证（本轮用
   `elapsed_ps = 250 000 000` 自证）⇒ **夹具也要有自己的判别式**。

### 3.11.5 反向变异自检（20 条 · 主工作区串行 · 收敛 target 集）

变异集 `/tmp/probe/k_mut_v20.py`（**20** 条 = **19** 条判定 + **1** 条对照），
驱动器 `/tmp/probe/k_mut_v20_run.py`。

口径（承 v0.19 的教训，**一条不省**）：
- 变异必须**能编译**——**编译错误导致的「红」不算**判别力（判 **INVALID**）；
- 期望集合用 **AND** 语义（逐条核对「每条期望的用例**各自**都红了」）；
- 记录**完整红集**（不只记命中项）；
- **对照变异**（语义不变）必须 **0 红**；
- 还原后**逐字节校验**（md5）+ `touch` 源文件。

⚠️ **target 集必须收敛**：参与判定的期望红测**全部**落在
`cargo test -p riscv_sim_kernel --lib --test m11_host_wiring` 这两个 target 里
⇒ 每轮只重链 **2** 个二进制（macOS 对**新链接**二进制的 `exec` 扫描固定
≈25 s/文件 ⇒ 收敛是**必需**，不是优化）。

⚠️ **变异跑期间只改文档**：变异自检进行中**不动任何会被编译的源文件**
（混源 ⇒ 结论不可信）⇒ 收口期的文档订正全是 `.md`，与变异目标零交集。

#### 3.11.5.1 首轮结果：**18/20 按期望**（两条有问题）

| 变异 | 目标 | 首轮实测 | 问题 |
| --- | --- | --- | --- |
| **V-03** | 波形导出不再读 M-19 的环（D-50） | **INVALID（编译失败）** | 变异形态写成 `ExportFilter::with_time_range(1, 0)`，但 `with_time_range` 是 **builder**（`mut self`）**不是**关联构造器 ⇒ **编译不过** ⇒ 变异**无效**，既不证明判别力、也不证明缺陷 |
| **V-14** | `StepN` 不分块 ⇒ 访存环溢出 | **GREEN（0 红）** | ⚠️ **真判别力缺口**：夹具固件手算错 ⇒ 访客第 2 条指令就陷入 ⇒ 25 万条**根本没跑到** ⇒ 断言**恒成立** ⇒ 用例**假绿**（见 §3.11.3 第 12 行） |

⇒ **工具链自己也是被检视对象**（v0.19 的教训，本轮再次印证）：一条变异
**杀不死**用例，先怀疑**用例**，再怀疑实现；一条变异**编译不过**，那是
**变异集**的错，**不是**被测代码的错。

#### 3.11.5.2 处置与重跑：**2/2 按期望**

| 变异 | 处置 | 重跑实测 |
| --- | --- | --- |
| **V-03** | 改写成**能编译**的形态：`ExportFilter::all().with_time_range(1, 0)`（**builder** 形态；时间窗 `1 > 0` ⇒ 空窗 ⇒ 导出无内容 ⇒ `total` 从 1 变 0） | ✅ **RED ✓**（红集 3：`the_m19_ring_has_a_reader` · `waveform_export_reads_the_m19_ring_and_not_the_m18_edge_store` · `the_waveform_export_reads_the_m19_ring_on_the_production_path`） |
| **V-14** | 修**夹具**（不是修断言）：`SELF_LOOP` 由 `0xFE00_0FE3`（实为 `beq x0, x0, -2`）改成 `0x0000_0063`（`beq x0, x0, 0`）⇒ 实测 `elapsed_ps = 250 000 000`（真跑满 25 万条） | ✅ **RED ✓**（红集 1：`a_long_step_batch_keeps_the_invalidation_channel_lossless`） |

#### 3.11.5.3 最终口径

⚠️ **20/20 按期望**：**19 条判定变异全部按期望变红**（完整红集见上）+ **1 条
对照变异（V-C1：只改错误消息文本，语义不变）0 红** ✅。
⇒ 与 v0.19 的 **49 条**同口径（**期望集合 AND 语义** · **完整红集** ·
**对照必须 0 红** · **编译错误不算判别力** · **逐字节还原 + md5 校验**）。

⚠️ **本轮的两条「没按期望」都不是被测代码的错**：一条是**变异集**写错
（编译不过），一条是**用例**假绿 ⇒ 两条都**改在夹具/变异集上**，**没动**被测
实现 ⇒ 这也是本轮**实现侧零返工**的原因（收口轮抓到的三处缺口是**先**修的，
变异自检**只**验证判别力，**不**产出新的实现缺陷）。

⚠️ **夹具改动的波及面已核实**：`loop_project()` 在 `tests_m11.rs` 里**只有
1 个调用点**（正是 V-14 指向的那条用例）⇒ 夹具从「会陷入」改成「真自环」
**不影响**其余 **18** 条变异所在用例的行为 ⇒ **不必全量重跑 20 条**
⇒ 「20/20」是「首轮 18 条 + 重跑 2 条」的合并口径，**每一条都有实测轮次**。

---

## §3.12 CSR / 陷阱黄金覆盖落地检视轮（2026-10-22 · v0.21）

### 3.12.1 两路独立检视的范围划分

| 检视员 | 范围 | 判决 |
| --- | --- | --- |
| **检视 A** | **`mip` 生产者**（M-16 → M-17 → CSR 文件）+ **CSR env 与语料**（`env/csr/` + `corpus.txt` + harness 的 env 列） | **FAIL**（1 条中危「`rv32mi/csr` 是空洞的一致」+ 1 条中危「`ma_fetch` 归因错误」+ 1 条中危「`instret_overflow` 恒真」+ 1 条中危「台账/文档未收口」+ 1 条中低危「契约层注释称与参考侧一致」+ 1 条低中「`PMP_A` 与上游不符 + env 漂移无守卫」+ 1 条低「manifest env 守卫可被以 `csr` 开头的理由骗过」+ 1 条低「规格 §3.2/§3.3 与实现不符」） |
| **检视 B** | **契约层 CSR 规则**（`csr.rs` 的已实现表 / 只读集 / `mie` 位号）+ **黄金 runner**（`riscv-golden.rs`）+ **规格↔实现一致性**（STAGE-6 ↔ M-13 LLD） | **FAIL**（1 条**高危**「陷入后 `mstatus.MPP` 未置 M」+ 1 条**高危**「规格 §0.1 的 `misa` 实测基线是错的」+ 1 条中危「`misa` 写语义分歧」+ 1 条中危「访存异常不陷入 + harness 的 `ERROR` 容忍是洞」+ 1 条中危「LLD 的 CSR 清单与契约层表双向不一致」+ 1 条中危「NOVERDICT 兜底无判别力」+ 1 条中危「台账未登记、数字未重算」+ 1 条低「契约层注释把平台取舍写成规范」+ 1 条低「`mstatus` 无 WARL 掩码」+ 1 条低「`mip` 写语义与参考侧不同」+ 1 条低「runner 用 `if let` 而非穷举 `match`」+ 1 条低「两处宽松解析」） |

⚠️ **两路独立收敛到同一条高危**（`mstatus.MPP`）——这是本轮最有价值的信号：
两路检视员在**互不知情**的情况下各自用**探针**测出了同一个分歧
（Spike `0x0018_0080` / 我们 `0x0000_0080`）。

### 3.12.2 本批的**主线教训**：「**实测基线**」也会是错的

本批的冻结规格 STAGE-6 §0.1 有一张「**实测基线**」表，标着「写规格时的证据，
不是推演」。两路检视各自把它**推翻**了：

| 规格原文 | 实测 | 后果 |
| --- | --- | --- |
| 「参考侧的 `misa` 还含 `Zicclsm` / `Zifencei` 位」 | 参考侧 `misa` = **0x40141104**（MXL + I + M + C + **S** + **U**）⇒ **没有** Zicclsm / Zifencei 位 | 「哪些位可比」的口径整个写错 |
| 「参考侧的 `misa` 带 **IALIGN=32**（bit 29），我们不填 ⇒ 覆盖不等价」 | 两侧 IALIGN 字段（bits 29:26）**都是 0** ⇒ **逐位相同** | 「覆盖差异」的**归因**错了（真正的原因是 `misa` 的**写**语义） |
| 「与 Spike（`misa` 无 F/U 位）**同分支** ⇒ 一致 ✅」 | 参考侧**有 U 位** ⇒ 它**进** U 模式段，我们**不进** ⇒ **不同分支** | `rv32mi/csr` 是**空洞的一致** |
| 「`MPP` 两侧读回都是 0 ⇒ **不构成缺口**」 | 探针测在**陷阱之外**且**没有写** ⇒ 读到的 0 说明不了任何事（`MPP` 是 **WARL**，**陷入时**才被写成 M） | 一条**真分歧**被「推翻」掉了 |

⇒ **方法论沉淀**：
1. **「实测」两个字不保证它是「对的」**——**基线表**和代码一样要被复核；写
   规格时用的探针**测在哪里**（哪个上下文、有没有写）必须**写进表里**，否则
   后来人无法判断它测的是什么。
2. **WARL 字段必须在「它会被架构写」的那个上下文里测**。`MPP` 的真相在
   **陷阱处理程序里**，不在外面。
3. **「判决一致」不等于「覆盖等价」**。本批 13 条 `rv32mi` 用例里有 **3 条**
   是空洞的一致（`csr` / `ma_fetch` / `instret_overflow`）⇒ 语料把「判决」
   当成了「覆盖」。⇒ 已逐条标注（D-96），但**自动检出**仍未做。
4. **冻结规格会被后来人当依据用**——一处假的实测基线会一路传下去（本批就
   差点让人据此把 `misa` 的 bit 29 置上 ⇒ 那是**非法**的 IALIGN 编码）。

### 3.12.3 闭环对照表

| # | 发现 | 来源 | 级别 | 处置 | 证据 |
| --- | --- | --- | --- | --- | --- |
| 1 | 陷入后 `mstatus.MPP` 未置 M | 检视 B · F-1 | **高危** | ✅ **已修**（`enter_trap` 写 `MPP ← M`）+ 2 条单测 + 探针复测两侧同为 `MPP = 3` | **D-97** |
| 2 | 规格 §0.1 的 `misa` 实测基线错 | 检视 A · P1/P2 + 检视 B · F-2 | **高危** | ✅ **已订正**（换成实测值 + 正确归因） | STAGE-6 §0.1 / §0.2 / 裁决 D / 裁决 E |
| 3 | `rv32mi/csr` 是空洞的一致 | 检视 A · P1 | 中危 | ✅ **已标注**（README）+ 留账「自动检出」 | **D-96** |
| 4 | `rv32mi/instret_overflow` 在候选侧恒真 | 检视 A · P3 | 中危 | ✅ **已标注**（README） | **D-96** / **D-94** |
| 5 | `misa` 写语义与参考侧分歧 | 检视 B · F-3 | 中危 | ⏳ 留账（平台取舍：放行写会让 `misa` 说谎） | **D-95** |
| 6 | 访存异常 5/7 未实现 + harness 容忍 `ERROR` 是洞 | 检视 B · F-4 | 中危 | ✅ **洞已堵**（pending 只容忍 `NOVERDICT` / 装载类 `ERROR`）+ 留账（架构陷阱） | **D-99** |
| 7 | M-13 LLD §2.2 的 CSR 清单与契约层表双向不一致 | 检视 B · F-5 | 中危 | ✅ **已对齐**（7 个「声称已实现」改成「未实现 ⇒ 写忽略读 0」+ 补 `mscratch` / `irq_clear`） | M-13 LLD §2.2 |
| 8 | NOVERDICT 兜底无判别力 | 检视 B · F-6 | 中危 | ✅ **已补用例**（`a_runaway_program_answers_noverdict`） | runner 测试 13 → **14** |
| 9 | 台账未登记 / 文档数字未重算 | 检视 A · P4 + 检视 B · F-7 | 中危 | ✅ **已收口**（D-91~D-101 登记 + §0.1 数字第六次重算 + AGENTS.md / PROGRESS.md 同步） | 本节 + §0.1 |
| 10 | 契约层注释称「与参考侧一致」 | 检视 A · P5 + 检视 B · F-8 | 中低危 | ✅ **已改写**（三处实测差异 + `--priv=MSU` 前提） | `crates/contract/src/csr.rs` |
| 11 | `PMP_A` 与上游不符 + env 漂移无守卫 | 检视 A · P7 | 低中 | ✅ **已修 + 已加守卫**（harness 逐条比对上游 `encoding.h`，漂移即硬报错） | `tools/golden-run.sh` |
| 12 | manifest 的 env 守卫可被以 `csr` 开头的理由骗过 | 检视 A · P8 | 低 | ✅ **已收紧**（pending 的理由必须以 `D-<n>` 开头） | `tools/golden-run.sh` |
| 13 | 规格 §3.2 / §3.3 与实现不符 | 检视 A · P9 | 低 | ✅ **已订正**（`misa` 特判 / `pending_lines` 而非 `mip_bits`） | STAGE-6 §3.2 / §3.3 |
| 14 | `mstatus` 无 WARL 掩码 / `mret` 不动 `MPP` | 检视 B · F-9 | 低 | ⏳ 留账（能力差异，非缺陷） | **D-98** |
| 15 | `mip` 的 guest 写语义与参考侧不同 | 检视 B · F-10 | 低 | ⏳ 并入 **D-94**（平台规则，已承认） | **D-94** |
| 16 | runner 用 `if let` 而非穷举 `match` | 检视 B · F-11 | 低 | ✅ **已改**（穷举 ⇒ 将来加变体会编译不过） | `riscv-golden.rs` |
| 17 | 两处宽松解析（`restore_full` / `sh_entsize`） | 检视 B · F-12 | 低 | ⏳ 留账 | **D-101** |
| 18 | `pending_lines()` 争用时返回空 ⇒ 读失败被当成「没有挂起」 | 检视 A · P5 | 低中 | ⏳ 留账（今天**不可达**，潜在） | **D-100** |
| 19 | **收口轮自己新写的 MPP 用例是恒真式**：断言写成 `mstatus >> MSTATUS_MPP_SHIFT == MSTATUS_MPP_M`，与生产代码**共用同一个常量** ⇒ 把常量改成 0 时断言照样成立 ⇒ **断言不了任何东西**。反向变异 **V-17**（`MSTATUS_MPP_M` 3 → 0）**没让这条用例变红** ⇒ 是**变异自检自己**抓出来的（不是检视员） | 收口轮 · 反向变异自检 | 中危 | ✅ **已改**（断言改用**架构字面量**：`mstatus` 的 bits 12:11 == `0b11`）⇒ V-17 复跑按期望变红 | — |

### 3.12.4 本轮方法论沉淀

- ⚠️ **探针要测在正确的上下文里**：`MPP` 的第一条探针测在**陷阱之外** ⇒
  结论是错的；第二条探针测在 **handler 里** ⇒ 真分歧立现。⇒ **WARL 字段必须
  在「架构会写它」的地方测**。
- ⚠️ **「实测基线」表要连「测在哪」一起写**：只有读数没有上下文，下一批会
  把它当**普适结论**用（本批就差点据此把 `misa` 的 IALIGN 写成非法值）。
- ⚠️ **harness 只编译一份镜像给两侧** ⇒ **env 常量漂移不会表现为分歧**，
  只会**悄悄换掉被测程序** ⇒ 常量必须**逐条比对上游**（本批已做成硬门禁，
  第一次跑就抓到 `PMP_A`）。
- ⚠️ **「跑不了」与「跑歪了」必须分开**：pending 的容忍规则只该接受前者 ⇒
  guest **fault**（`ERROR step@…`）不容忍。
- ⚠️ **穷举优于 `if let`**：`StepOutcome` 将来若加变体，`if let` 会**静默忽略**
  它 ⇒ 判决可能悄悄改变（M-17 早就用穷举 `match`，runner 没有）。
- ⚠️ **断言不要与生产代码共用常量**：`mstatus.MPP == MSTATUS_MPP_M` 看着精确，
  实则把「常量是什么」与「代码写什么」绑在一起 ⇒ 改常量时断言跟着改 ⇒ **恒真**。
  ⇒ **反向变异（改常量的值）就是恒真式的照妖镜**——本轮它当场抓出收口轮自己新写的一条用例。

### 3.12.5 反向变异自检（19 条 + 1 条对照 · 主工作区串行）

**口径**：变异**必须能编译**（编译错误 = INVALID，不算判别力）；**期望集合用 AND
语义**逐条核对（列出的每一条都必须变红）；记录**完整红集**；**对照必须 0 红**；
还原**逐字节校验**（md5）+ `touch`；每条命令都有**超时**（macOS 无 `timeout`
⇒ `perl alarm`）。

| 变异 | 落点 | 期望 | 结果 |
| --- | --- | --- | --- |
| V-01 / V-02 | 契约层「已实现 CSR 表」 | 删 `mscratch` / 多列 `medeleg` ⇒ 表不再等于平台的集合 | ✅ 红 3 / 红 3 |
| V-03 | `csr_write_is_ignored` | 去掉「未实现 ⇒ 写被忽略」 | ✅ 红 2 |
| V-04 | `csr_is_read_only` | `misa` 不再只读 | ✅ 红 2 |
| V-05 / V-06 | `CSR_MISA_VALUE` | MXL 改 RV64 / 多报 A 扩展 | ✅ 红 2 / 红 2 |
| V-07 | `std_irq_bit_for_line` | 定时器线映到 MEIP 位 | ✅ 红 4 |
| V-08 | `pending_lines_to_mip_mask` | 投影恒为 0 | ✅ 红 2 |
| V-09 | M-13 `csr_read` | 去掉 `misa` 特判 ⇒ 读回 0 | ✅ 红 3 |
| **V-16** | M-13 `enter_trap` | **不再写 `MPP`** ⇒ 回到「`MPP = 0`」 | ✅ 红 3（**收口轮修的真缺陷的回归防护**） |
| **V-17** | 契约层 `MSTATUS_MPP_M` | 合法值改成 0（U 模式） | ✅ 红 3（**首轮没红 ⇒ 抓到一条恒真式**，见下） |
| V-10 | M-17 `project_mip` | 去掉「值变化才写」 | ✅ 红 2 |
| V-11 / V-12 | M-17 刷新点 ① / ② | 删掉投影点 | ✅ 红 5 / 红 2 |
| V-13 | runner 陷阱臂 | 回退裁决 A（遇陷阱判 ERROR） | ✅ 红 1（机器级语料全 MISMATCH） |
| V-14 | runner 符号查找 | 找不到 `tohost` | ✅ 红 1（**自检阶段**就判「比较不再有判别力」并放弃语料运行） |
| V-15 | harness include 顺序 | env 变体被默认 env 遮蔽 | ✅ 红 1（BUILD-FAILED） |
| **V-18** | runner 预算兜底 | 预算耗尽判 PASS 而不是 NOVERDICT | ✅ 红 2（**收口轮补的判别力的回归防护**） |
| **V-C** | M-17 `project_mip` | 对照：比较的两个操作数对调（语义等价） | ✅ **0 红**（对照必须 0 红） |

⇒ **19 / 19 按期望**（含 1 条对照 0 红）。

⚠️ **首轮的 V-17 没让那条用例变红**——因为用例断言写成
`mstatus >> MSTATUS_MPP_SHIFT == MSTATUS_MPP_M`，与生产代码**共用同一个常量**
⇒ 把常量改成 0 时断言跟着变 ⇒ **恒真式**。⇒ 已把断言改成**架构字面量**
（`mstatus` 的 bits 12:11 == `0b11`）⇒ V-17 复跑按期望变红。⇒ **反向变异
（改常量的值）是恒真式的照妖镜**。

**harness 层的三道守卫**（不在 Rust 里 ⇒ `cargo test` 覆盖不到 ⇒ 单独钉）：

| 守卫 | 「故意破坏 ⇒ 守卫必须报出来」的验证 | 结果 |
| --- | --- | --- |
| env 常量必须逐字等于上游 | 把 `PMP_A` 改回 `0x0c` | ✅ 硬报错（rc=2） |
| pending 的理由必须以台账号开头 | 加一行理由为「csr is not supported here」的 pending | ✅ 硬报错（rc=2） |
| pending **不容忍**运行期失败 | 让 runner 遇陷阱就中止（运行期 `ERROR trap@…`）+ 把该例标成 pending | ✅ 判 **MISMATCH**（`mismatch=1`），**没有**被当成 expected-gap 吞掉 |
| 对照：守卫都在时正常路径仍全绿 | `golden-run.sh --list` | ✅ rc=0 |

⚠️ **两个判据自身的坑**（都当场修掉）：① 汇总行里**永远**有
`expected-gap=0` 这个字样 ⇒ 按子串判「是否被当成 expected-gap」**永远为真**
⇒ 必须看**逐案行**的行尾标记；② 逐案判决里**可能含空格**
（`ERROR trap@0x80000004:IllegalInstruction`）⇒ 不能逐字段用 `\S+` 匹配。

## §3.13 Wave 2a 四路独立检视轮（2026-10-22 · v0.22）

> **一句话**：本轮四路检视在**实现侧**只查到 0 阻断 / 0 高危，却查出了**一条真的断掉的生产路径**
> ——「源码视图永远拿不到 `sim.pc`」——而**门禁 243/243 全绿**；再加上**三条恒绿断言**
> 与**六处与代码相反的假陈述**。⇒ 本轮的价值不在「发现实现 bug」，而在**证明「用例绿」不等于
> 「它测的是生产路径」**。

### 3.13.1 四路独立检视的范围划分

| 路 | 范围 | 判决 | 发现数 |
| --- | --- | --- | --- |
| **V-A** | Rust 内核（M-11 DWARF 解析器 / M-13 陷阱与 CSR / M-14 总线 / M-17 后端 / 宿主） | **PARTIAL** | **12**（中 6 · 低 6） |
| **V-B** | 前端（零构建 ES modules）+ 工具链（harness / 冒烟 / 真机复验），重点是**判别力** | **FAIL** | **8**（高 3 · 中高 2 · 中 2 · 低 1） |
| **V-C** | 规格 ↔ 实现 ↔ 台账 ↔ 进度文档的**一致性** | **FAIL** | **15**（阻断 2 · 高 3 · 中 4 · 低 6） |
| **V-D** | **M-12 专项**（事件驱动引擎）—— 专职闭环台账 **D-30**（「M-12 至今没有独立模块级 review」） | **PARTIAL** | **9**（中 3 · 低 6） |

**为什么是四路而不是三路**：v0.21 批的检视是**两路**（V-A 内核 / V-C 文档）；本批多出两路
—— **V-B** 专职查「用例到底有没有判别力」，**V-D** 专职闭环 **D-30**。
**D-30 的原文**是「M-12 是事件驱动引擎，其快照 / 时序语义是全平台的确定性基础，风险高于其它模块」，
而本批 Lane C 恰好动了 M-12 的 `restore_full` ⇒ **借这个窗口补上**。

**四路的独立性**：各自独立 worktree（`/tmp/rev-a` … `/tmp/rev-d`）、各自独立
`CARGO_TARGET_DIR`、**互不可见** ⇒ 同一处缺陷被**两路独立复现**时，那条结论的置信度显著提高
（本轮的 **VB-1 / VA-12** 就是这样：两条路各自用探针查出了**同一条**生产路径断裂）。

### 3.13.2 本批的**主线教训**：「**用例绿**」不等于「**它测的是生产路径**」

v0.20 批的主线教训是「**接上了**」和「**钉住了**」是两件事。
本批把它推进一步：**「钉住了」也可能是钉在了一个测试自己造出来的状态上。**

V-B 的高危 **VB-1**（V-A 的 **VA-12** 独立复现）：

- M-10 的 `flushRender()` 只往 `viewIds` 里的视图写 `sim.{gpr,pc,mem}`；
  M-10 的 `viewIds` 默认值是 `["debug", "regmem"]`，而 `app.js` 调 `mountRegisterMemory` 时**没传**
  ⇒ **`source` 视图永远收不到 `sim.pc`** ⇒ 「解析当前 PC 的源码行」按钮的 `data-addr` **恒为 `"0"`**；
- 而 `front/tests/source-view.test.mjs` 里的 A-16 用例**手工**把 `sim.pc = 0x1000` 塞进 source 视图状态
  ⇒ 它断言的是一个**测试自己造出来的**状态，**不是生产状态** ⇒ **门禁 243/243 全绿**。

⇒ **这条缺陷「接上了」吗**？接上了（信号、命令、事件、渲染四段都在）。
⇒ **这条缺陷「钉住了」吗**？**没有** —— 因为钉住它的用例**绕开了**生产接线的那一段。
⇒ **第三条教训**：**用例要断言「生产路径上会发生什么」，而不是「测试造出的状态上会发生什么」**。
⇒ **修法（本批已落地）**：① 把 `viewIds` 提成**导出的常量** `REGISTER_MEMORY_VIEW_IDS`，
生产与测试**共用同一份**（常量少了 `"source"` ⇒ 两条用例一起变红）；
② A-16 改成**走生产接线**（真的 `mountRegisterMemory` + `sim.state.delta` 事件把 PC 推进来），
**不再手工注入**。

**同一轮的另外三条恒绿断言**（全部由 V-B 的反向变异自检查出，**红集为空**）：

| 变异 | 期望 | **实际红集** | 结论 |
| --- | --- | --- | --- |
| **M8** 删掉 `app.js` 的 `case "resolve-line"` 整段 | 红 | **空**（243/41/0 fail，exit 0） | **VB-3**：用例只断言**信号**、**手工注入**内核回复 ⇒ 不断言命令**真的被派发** |
| **M9** 装载工程时把 `source.tree` 一起清空 | 红 | **空**（243/41/0 fail，exit 0） | **VB-2**：用例传的是 `{ reset() {} }` **空壳 stub** ⇒ 生产里真正动树的那段代码**从未执行** |
| **M4b** 只把高亮标到**相邻的错误行** | 红 | **空**（243/41/0 fail，exit 0） | **VB-4**：断言的「有 `is-current`」与「有 `data-src-line="3"`」**出自同一个变量** ⇒ 只验了**自洽** |

⇒ 这三条与 **VB-1** 是**同一个病根**：**断言的落点不在生产路径上**。
⇒ 另有一条**判别力叙述失真**（**VB-6**）：commit `25203d4` 写「把 `..` 段原样保留 ⇒
`../../src/main.c ↔ src/main.c` 那条变红」，**实测该条保持绿** —— 真正变红的是那条直接断言
`normalizePath` 的**纯函数**用例。原因是**尾部逐段匹配规则自己就能吸收前导的 `..`**
⇒ 那三条匹配级用例**根本碰不到正规化**；真正只有正规化能救的是**中段**的 `..`
（DWARF `x/a/../b.c` ↔ 本地 `x/b.c`）⇒ 本批补了**中段 `..`** 的匹配级用例。

**V-A 独立查出的同类病根**（**VA-03**）：把 v5 的 `initial_file: 0` 改成 `1`（**实现是对的**，
但这条规则**零护栏**）⇒ 全仓 **1074 用例零红**。⇒ **「用例都在」≠「用得着」**（台账的老教训，
本轮再次坐实）。⇒ 已补两条用例（v5 首行不显式 `set_file` ⇒ 文件表第 **0** 项；
v2~v4 ⇒ 第 **1** 项）。

**V-C 查出的「假陈述」簇**：本批的**代码**是对的，**文档**在撒谎 ——
`AGENTS.md` 的 M-11 行仍写「**DWARF 只识别不解析**…Wave 2 才给行号」（**说反了**）；
`csr.rs` 有四处注释说「`mepc` 写侧不掩码」（**实为掩码**）与「`mret` 的调用点没接」（**实为已接**）；
`kernel_host` 仍写「M-11 恒返回降级态」（**实为真解析**）；
冻结规格的**裁决 D**（「契约层改动量 = 0」）与**裁决 I**（「契约层加 `MEPC_ALIGN_MASK`」）**自相矛盾**，
且 §2.3 的 **C-1**（「低两位为 0」）与裁决 I（「最低 1 位」）**互相冲突** ⇒ **验收判定自相冲突**。
⇒ **教训**：`AGENTS.md` 是 AI Agent 读的第一份文件，它一旦说反，后面接手的 Agent 会**重开已闭环的欠账**。

**V-D 查出的「承诺了但没实现」簇**：M-12 的 LLD §3.5 / §1.1 / §5.1 / NFR-08 要求一个
**watchdog（无进展检测）**，**实现里一行都没有**，而代码注释把它标成「已实现（watchdog pre-check）」
并引用了一个**不存在的章节**「**LLD §4.12.6**」⇒ **V-D 实测 LLD 文本里 `4.12` 出现 0 次**。
⇒ 更要命的是：LLD 要的那个守卫，**正是能抓住 `InterruptTaken` 自旋的那个守卫** ——
`run_until` 的 `InterruptTaken` 分支**三个记账全不动**（不增指令、不增配额、不推进时钟），
V-D 实测 **8 秒 3.35 亿次**执行器调用、`run_until` **永不返回**（这正是台账 **D-83** 的「生产侧待做 ①」，
本轮从「推测」升级为「**实测确认**」）。

### 3.13.3 四路判决与发现汇总

| 路 | 发现 | 严重度分布 | 判决 | 处置 |
| --- | --- | --- | --- | --- |
| **V-A** | 12 | 中 6 · 低 6 | **PARTIAL** | 中危 **VA-01 / VA-02 / VA-03 / VA-05 / VA-12** 本批已修（VA-12 = VB-1）；低危 **VA-06 / VA-08 / VA-09** 登记为 **D-104 / D-105 / D-106**；**VA-04**（台账未更新）由本节归档闭环；**VA-07 / VA-10 / VA-11** 本批已修 / 已如实登记 |
| **V-B** | 8 | 高 3 · 中高 2 · 中 2 · 低 1 | **FAIL** | 高危 **VB-1 / VB-2 / VB-3** 与中高危 **VB-4 / VB-5**、中危 **VB-6 / VB-7** 本批已修；低危 **VB-8** 的「仅提示」子项（a）已留账、「PC 未知不可区分」子项（b）登记为 **D-107** |
| **V-C** | 15 | 阻断 2 · 高 3 · 中 4 · 低 6 | **FAIL** | 全部由本批的**台账 / 进度 / `AGENTS.md` / 规格**归档闭环；其中 6 条是**对收口方自己 WIP 的复核**（W-01 ~ W-06），抓出**两条假陈述**（「契约层逐字节未动」）+ **两处悬空引用**（指向尚不存在的 §3.13）+ **日期写错**（写成 2026-10-23，其实是 2026-10-22）+ **`AGENTS.md` 的 Setup 段两个数字没更新** |
| **V-D** | 9 | 中 3 · 低 6 | **PARTIAL** | 中危 **VD-F1 / VD-F2 / VD-F3** 本批已修；低危 **VD-F4**（LLD 的 watchdog 承诺 + 假引用）随 VD-F3 一并订正；**VD-F5 ~ VD-F9** 登记为 **D-108 ~ D-112** |

**合计：44 项发现 → 本批闭环 35 项，新登记 9 条（D-104 ~ D-112，均低危）。**

### 3.13.4 闭环对照表

| 发现 | 严重度 | 问题 | 处置 | 证据 |
| --- | --- | --- | --- | --- |
| **VA-12 = VB-1** | 中 / **高** | `source` 视图收不到 `sim.pc` ⇒ 按钮恒问地址 0 | ✅ 本批已修 | 两条路各自用探针独立复现；修法 = 导出常量 `REGISTER_MEMORY_VIEW_IDS` + A-16 改走生产接线 |
| **VB-2** | **高** | A-20 用例传空壳 stub ⇒ 恒绿 | ✅ 本批已修 | 变异 M9（真的清空 `source.tree`）红集**空** ⇒ 改用真的 `wireDebug(router).reset()`，并补「行号被清、树还在」两半断言 |
| **VB-3** | **高** | `app.js` 的 `resolve-line` 整段无人守 | ✅ 本批已修 | 变异 M8（删掉 `case "resolve-line"`）红集**空** ⇒ 改断言 `transport.sent` 里真有一条 `ResolveLine` 且地址正确 |
| **VB-4** | 中高 | 高亮到**错的行**也全绿 | ✅ 本批已修 | 变异 M4b 红集**空** ⇒ 断言被标行的行号、正文内容、首页行号 |
| **VB-5** | 中高 | 内核的 `NoDwarf` / `Missed` 被抹平成「尚未解析」 | ✅ 本批已修 | 探针实测两种「没有答案」渲染成同一句 ⇒ 视图分三态渲染，并补用例 |
| **VB-6** | 中 | 正规化的判别力只落在纯函数上 | ✅ 本批已修 | 变异 M2a / M2b 只有纯函数那条红 ⇒ 补**中段 `..`** 的匹配级用例 |
| **VB-7** | 中 | 真机复验对源码视图**零覆盖** | ✅ 本批已修 | `browser-verify.mjs` 全文 0 处 `source` ⇒ 新增 `sourceChecks` 组，断言按钮 `data-addr` **等于当前 PC 的具体值** |
| **VA-01** | 中 | 无 `end_sequence` 的段被当成「覆盖到地址空间末尾」⇒ 远处地址被报成**错的真行号** | ✅ 本批已修 | 探针实测 `[0x99999999] -> Found` ⇒ 该段不贡献任何行记录（⇒ `Missed`） |
| **VA-02** | 中 | 文件名为空时仍产出 `Found` ⇒ 契约层**第三种**没有答案 | ✅ 本批已修 | 探针实测 `Found{file:"",line:1}` ⇒ `lookup_line` 跳过空文件名 ⇒ 落到 `Missed` |
| **VA-03** | 中 | v5 与 v2~v4 的**文件索引基址**（0 vs 1）零护栏 | ✅ 本批已修 | 变异 M-03（`initial_file: 0→1`）红集**空** ⇒ 补两条用例 |
| **VA-05** | 中 | `csr.rs` 四处注释与代码**相反** | ✅ 本批已修 | 逐条对照代码改写（用户写掩 `~1` / 陷入门写精确 PC / `mret` 取 PC 再掩） |
| **VA-07** | 低 | 陷入门那条用例**过度宣称** | ✅ 本批已修 | 变异 M-13（陷入门改走 `csr_write`）**该条没红** ⇒ 注释改成「这条路径上根本不加掩码」 |
| **VA-04** | 中 | 台账 / `AGENTS.md` 对本批**零更新** | ✅ 本节闭环 | 本节 + §0.1 + §1.2/§1.3 + §2 + §5 + `PROGRESS.md` v0.22 段 + `AGENTS.md` 五行与判据块 |
| **VD-F1** | 中 | 引擎自己产出的快照**过不了契约自己的 `validate()`** | ✅ 本批已修（Lane W2） | 探针实测 `E_SNAP-02: event due at 5000 ps, before virtual time 6000 ps` ⇒ 修法落在**引擎侧**：`snapshot_full` 出快照时把迟到事件按 `max(at_ps, now)` 归一化（**不放宽契约层校验器**）⇒ 变异 **D** 红集 2 条（§3.13.5.3） |
| **VD-F2** | 中 | `restore_full` 的 `next_seq` 下限修正**零判别力** | ✅ 本批已修（Lane W2） | 变异 V3 红集**空** ⇒ 补一条用手工 `EngineSnapshot` 的用例（DTO 路径会先 `validate()` ⇒ 进不到这里）⇒ 变异 **E** 红集 1 条（§3.13.5.3）。**口径**：那条修正**不是死代码**，而是**公开 API 上的安全网** ⇒ 选**补用例**不删代码 |
| **VD-F3** | 中 | `run_until` 的 `InterruptTaken` 分支**无界自旋** | ✅ 本批已修（Lane W2） | 探针实测 **8 秒 3.35 亿次**不返回 ⇒ 加**无进展守卫** `NO_PROGRESS_WINDOW = 1024`（无指令退休 + 时间未前进 + 无到期事件 ⇒ 判定停滞并 `Err` 退出，`E_ENG_03_NO_PROGRESS`）⇒ 变异 **B** / **F** **HANG**（按预期）、**A** / **G** 红集 1 条、**C** 红集 3 条（§3.13.5.3） |
| **VD-F4** | 低 | LLD 要求的 watchdog 未实现，注释假引用不存在的「LLD §4.12.6」 | ✅ 本批已修（Lane W2） | V-D 实测 LLD 文本里 `4.12` 出现 **0** 次 ⇒ 随 VD-F3 一并订正；**watchdog 的其余三块**（宿主时间预算 / 滑动窗口三次确认 / HMS 同步终止与 `EV_WATCHDOG` 日志）⇒ **D-115** |
| **VA-06** | 低 | v5 的 `define_file` 传空字符串节区 ⇒ 整单元降级 | ⏳ **D-104** | 探针实测 `Malformed` / `units_degraded = 1` |
| **VA-08** | 低 | 降级单元的地址报成 `Missed`（与「畸形 ⇒ `NoDwarf`」的节区级口径不一致） | ⏳ **D-105** | 单元自带用例钉住了这个选择 ⇒ 是**有意选择**，与裁决 J 字面口径有张力 |
| **VA-09** | 低 | 三个状态机寄存器维护但无任何断言 | ⏳ **D-106** | `raw_rows()` 不暴露它们 ⇒ 不可观察 |
| **VB-8b** | 低 | 「PC 未知」与「PC 就是 0」不可区分 | ⏳ **D-107** | 与 VB-1 同源（VB-1 已修） |
| **VD-F5** | 低 | `TimeReached` 在目标未到时也说谎 | ⏳ **D-108** | 探针实测 `until_ps=500, now_ps=0, reason=TimeReached` |
| **VD-F6** | 低 | `SimState::seed` 是死字段 | ⏳ **D-109** | 全仓无生产代码读写 |
| **VD-F7** | 低 | `quanta` 一词两义 | ⏳ **D-110** | 「跨过的量子数」vs「外层循环圈数」 |
| **VD-F8** | 低 | CSR 过滤只堵还原侧、不堵写侧 | ⏳ **D-111** | V-D 明确回答：完整快照往返**恒等**，**当前不是缺陷** |
| **VD-F9** | 低 | `misa` 条目让「空快照」判据变假 | ⏳ **D-112** | 对 guest 不可见（`misa` 由 `csr_read` 特判） |
| **F-01 ~ F-15**（V-C） | 阻断 2 · 高 3 · 中 4 · 低 6 | 台账零更新 / 数字未重算 / `AGENTS.md` 说反 / 规格自相矛盾 / 语料数错 / 悬空引用 | ✅ 本批已修 | 见 §0.1 · §1.2/§1.3 · §2 · §3.13（本节）· §5 + `PROGRESS.md` v0.22 段 + `AGENTS.md` 五行与判据块 + STAGE-7 裁决 D / 裁决 H / C-1 / B-6 + STAGE-5 裁决 F 回指 |
| **W2-1** | 低 | VD-F1 的归一化在「≥2 件迟到事件且 `seq` 次序与时刻次序相反」时可能改变**出队次序** | ⏳ **D-113** | Lane W2 自查发现；换掉的是「快照根本落不了盘」这个更大的问题 |
| **W2-2** | 中低 | M-17 `step_n` 的 `InterruptTaken` 分支有**同形状**活锁，VD-F3 的守卫**只覆盖 M-12** | ⏳ **D-114** | Lane W2 自查发现；今天无活影响（`step_n` 是单步语义） |
| **W2-3** | 中低 | watchdog 只做了「无进展检测」这一半 | ⏳ **D-115** | Lane W2 自查发现；VD-F3 的核心诉求已达成，剩下是处置链路与第二道防线 |
| **W2-4** | 低 | `E_ENG_03` 未进契约层场景码表 | ⏳ **D-116** | Lane W2 自查发现；契约层本批不升版（裁决 D），随契约层 v4 一并登记 |

### 3.13.5 反向变异自检

**检视轮自身的变异**（各路在**自己的 worktree** 里做，**互不可见**）：

| 路 | 变异条数 | 对照（什么都没改） | **红集为空**的变异 |
| --- | --- | --- | --- |
| **V-A** | 若干（含 M-03 / M-13 / G-03 / G-04） | 0 红 ✅ | **M-03**（v5 `initial_file` 0→1）⇒ **VA-03**；**M-13 / G-04**（陷入门改走 `csr_write`）⇒ **VA-07** |
| **V-B** | **13**（M0 ~ M12） | **M0 红集为空** ✅ | **M4b**（高亮标到相邻错误行）⇒ **VB-4**；**M8**（删掉 `case "resolve-line"`）⇒ **VB-3**；**M9**（清空 `source.tree`）⇒ **VB-2** |
| **C（V-C）** | — | — | — |
| **V-D** | **7**（V0 ~ V6） | **V0 红集为空** ✅ | **V3**（删掉 `next_seq` 下限修正）⇒ **VD-F2** |

⚠️ **V-D 的第一轮驱动器有 bug**：`restore_all` 在 `run_case` **开头**就把每次变异**覆盖回原版**
⇒ 那 7 条「变异」其实**测的是原始代码** ⇒ 全部「通过」。⇒ **作废并重跑**。
**关键点**：**正是「对照变异」暴露了它** —— 对照本该是 0 红，驱动器却让对照也变红了。
⇒ **教训**：**对照变异不是形式** —— 它是**发现驱动器自身 bug** 的唯一手段。

**已验证有判别力的关键判据**（V-A / V-B / V-D 各自验过）：

- **5 / 7 分界可分别观察**：把 store 的 cause 从 7 改成 5 ⇒ `unmapped_store` 红而 `unmapped_load` **仍绿**
  ⇒ 两条用例缺一不可（这就是 STAGE-7 裁决 H 从「加一条」改成「加两条」的依据）；
- **`compressed_trap` 钉住 `mepc` 的精确 PC**（V-A 的 G-03）；
- **D-100 的争用**：真的造出 `borrow_mut` 跨过 handler 的 `irq_clear` 写 ⇒ `mip` **不被写**；
- **D-101 两处**：CSR 表过滤 ⇒ 恢复含未实现 CSR 的快照 ⇒ guest 读回 **0**；`sh_entsize` 谎报 ⇒ 整个镜像 `E_LDR-01` **拒收**；
- **契约 JS 镜像对拍**（V-B 的 M10a）：改镜像里的一个字段名 ⇒ **4 条变红** ⇒ 那道门禁是**活的**；
- **新套件确实被门禁收进去**（V-B 的 M11）：把 `source-view.test.mjs` 挪走 ⇒ 用例数 **243 → 228** / suite **41 → 39**。

**修复批自身的反向变异自检**（统筹方在主树串行做，**变异必须能被 node 加载**；
还原一律用 `cp` 备份复制、**禁用** `git checkout --`，还原后 `md5` 逐字节校验）：

| # | 变异 | 期望 | **实际红集** |
| --- | --- | --- | --- |
| **M0** | **对照**（什么都不改） | 红集**为空** | **空**（245/41/0 fail，exit 0）✅ |
| **M-A** | 从 `REGISTER_MEMORY_VIEW_IDS` 去掉 `"source"`（**复现 VB-1 的生产断链**） | 红 | `当前 PC ⇒ resolve-line 命令 ⇒ SourceLocation ⇒ 高亮对应行`（+ 所属 suite）✅ |
| **M-B** | 装载工程时把 `source.tree` 一起清空（**VB-2 的恒绿根因**） | 红 | `源码**不进内核**：装载工程后源码树仍在（A-20）`（+ 所属 suite）✅ |
| **M-C** | 删掉 `app.js` 的 `case "resolve-line"` 整段（**VB-3**） | 红 | `当前 PC ⇒ resolve-line 命令 ⇒ SourceLocation ⇒ 高亮对应行`（+ 所属 suite）✅ |
| **M-D** | 把高亮标到**相邻的错误行**（`no === wanted + 1`，**VB-4**） | 红 | 同上（+ 所属 suite）✅ |
| **M-E** | 把 `NoDwarf` / `Missed` 两态合并回「尚未解析」（**VB-5**） | 红 | `内核的两种「没有答案」必须与「尚未解析」分开显示（VB-5）`（+ 所属 suite）✅ |
| **M-F** | 正规化**不消解中段的 `..`**（**VB-6**） | 红 | `尾部逐段匹配：中段的 .. 必须被正规化消解才匹配得上` + `正规化：. 与 .. 段被消解`（+ 所属 suite）✅ |
| **M-G** | **对照**（全部还原后复跑） | 红集**为空** | **空**（245/41/0 fail，exit 0）✅ |

⇒ **6 条变异按期望全部变红，2 个对照红集均为空**，三个被改文件的 `md5` 还原校验**逐字节一致**，
主树 `front/` 恢复干净。
⚠️ **注意红集里那两条「所属 suite」**：node:test 的父 suite 在子用例失败时**自己也报 failed**
⇒ 计数时**只数叶子用例**（父 suite 是子用例失败的**镜像**，不是独立判据）。

#### 3.13.5.1 修复 lane W1（M-11 DWARF · Rust 侧 · 5 条变异 + 1 对照）

lane 自有 worktree + 自有 `CARGO_TARGET_DIR`；变异**必须能编译**；跑全仓 `--no-fail-fast`
取**完整红集**；还原一律 `cp` 备份复制 + `md5` 逐字节校验（**禁用** `git checkout --`）。

| # | 变异 | 期望 | **实际红集** |
| --- | --- | --- | --- |
| **M00** | **对照**（只改一条注释，不动语义） | 红集**为空** | **空**（0 条）✅ |
| **M01** | 还原 VA-01 的旧行为（`end_addr` 改回覆盖到地址空间末尾 `u64::from(u32::MAX)`） | 红 | **1 条**：`a_sequence_without_end_sequence_does_not_cover_the_whole_address_space` —— 失败信息显示地址 `0x9999_9999` 被报成 `Found` `inc/main.c` 行 1 ✅ |
| **M02** | 删掉「空文件名那两行跳过」（VA-02） | 红 | **1 条**：`a_row_without_a_file_name_is_a_miss_not_an_empty_file` —— 失败信息显示 `Found` 的 `file` 是空串而 `line` 等于 1 ✅ |
| **M03** | 把 v5 的 `initial_file` 从 **0** 改成 **1**（VA-03 的**零护栏**方向） | 红 | **1 条**：`the_v5_default_file_register_is_index_zero` —— 失败信息显示首行落到了 `v5-one.c` 而不是 `v5-zero.c` ✅ |
| **M04** | 把 v2 ~ v4 的 `initial_file` 从 **1** 改成 **0**（VA-03 的**有间接判别力**方向） | 红 | **8 条**（本批新用例 `the_v2_v4_default_file_register_is_index_one` 在内，另有 **7 条既有用例**因间接依赖该初值而变红）✅ |

⇒ **5 条按期望全部变红，对照红集为空**；被改文件 `dwarf.rs` 的 `md5` 还原校验**逐字节一致**。
⚠️ **M03 与 M04 的红集规模差 8 倍，是有意保留的** —— VA-03 的两个方向**判别力不对称**：
v2 ~ v4 方向**此前已有间接判别力**（M04 会让 8 条变红），**真正零护栏的是 v5 方向**
（V-A 在**原始树**上复测 M03 ⇒ **1 074 条用例零红** ⇒ 坐实新用例补的正是那条零护栏）。
⇒ **两条用例都保留**，以便**意图显式**，也免得后人误以为「v2 ~ v4 方向没人管」。

#### 3.13.5.2 修复 lane W3（真机复验 · browser-verify · 2 条变异）

| # | 变异 | 期望 | **实际红集** |
| --- | --- | --- | --- |
| **W3-1** | 从 `front/app.js` 的 `REGISTER_MEMORY_VIEW_IDS` 去掉 `"source"` | 红 | **第 5 条 + 第 7 条**：按钮的 `data-addr` 变成 **0**、捕获到的命令里 `resolve-line` 的地址也是 **0** ✅ |
| **W3-2** | 把按钮的 `data-addr` 改成**错误但非零**的 `999` | 红 | **同样第 5 条 + 第 7 条** ✅ |

⇒ **2 条按期望变红**；既有四组检查（`landingChecks` / `kernelExecChecks` / `renderChecks` /
`waveformChecks`）在两次变异中**始终保持全绿** ⇒ 它们**不是**这两条的判据来源。
⚠️ **W3-2 的意义**：W3-1 只证明「拿不到 PC 时会红」，W3-2 才证明「**拿错了 PC** 也会红」
⇒ 那两条断言是**值断言**（`data-addr` **等于**当前 PC 的具体十进制值），**不是**「非空即过」。
⇒ 这是本批 §3.13.2 那条主线教训在**真机侧**的同构落实。

#### 3.13.5.3 修复 lane W2（M-12 引擎 · Rust 侧 · 8 条变异 + 1 对照）

lane 自有 worktree + 自有 `CARGO_TARGET_DIR`；变异**必须能编译**；跑全仓 `--no-fail-fast`
取**完整红集**；还原一律 `cp` 备份复制 + `md5` 逐字节校验（`6205a61b…`，每轮核对**残留测试二进制数为 0**）。

| # | 变异 | 期望 | **实际红集** |
| --- | --- | --- | --- |
| **M00** | **基线**（未变异） | 红集**为空**、**0 挂死** | **空**（**1079 全绿** = 基线 1074 + 5；契约 106 + kernel lib 857 + 集成 108 + runner 14）✅ |
| **A** | 把守卫的 `Err` 改成 `Ok(RunOutcome{…})` | 红 | **1 条**：`a_runaway_interrupt_delivery_is_reported_instead_of_spinning_forever` ✅ |
| **B** | **整块删掉守卫** | **HANG**（按预期） | 单条用例 **HANG**（60 s 窗口被杀，退出码 124）· 全仓 **HANG**（300 s 窗口被杀）⇒ 记为 **HANG**（**不算红也不算绿**），残留二进制 0 ✅ |
| **C** | 把阈值 `NO_PROGRESS_WINDOW` 从 1024 改成 **1** | 红 | **3 条**：`a_burst_of_interrupt_deliveries_still_makes_progress` · `an_interrupt_delivery_is_neither_counted_nor_a_stop` · `kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest` ✅ |
| **D** | 删掉 `snapshot_full` 的迟到事件归一化 | 红 | **2 条**：`a_snapshot_holding_a_due_event_still_passes_the_contract_validator` · `a_session_payload_carrying_the_engine_snapshot_validates` ✅ |
| **E** | 删掉 `restore_full` 的 `next_seq` 下限修正 | 红 | **1 条**：`restoring_a_hand_built_snapshot_raises_the_insertion_counter_above_the_queue` ✅ |
| **F** | 把阈值改成 `u32::MAX` | **HANG**（按预期） | 单条用例 **HANG**（60 s 窗口内守卫生效不了）· 全仓 300 s 窗口内守卫生效（该用例耗时 110.94 s 后仍通过，红集 0）✅ |
| **G** | 把触发条件的 `>=` 改成 `>` | 红 | **1 条**（同 A 那条：断言执行器调用次数不得超过 1024）✅ |
| **CTRL** | **对照**（只改一条注释） | 红集**为空**、**0 挂死** | **空** ✅ |

⇒ **6 条判定变异按期望全部变红，2 条按预期挂死（HANG），对照红集为空**，被改文件 `md5` 还原校验
**逐字节一致**，**残留测试二进制 0**。

⚠️ **C 那条红集最值得记**：把阈值调到 1 会让**生产路径上的真中断交付**也被误伤
（`kernel_host::tests_intc::a_raised_irq_really_reaches_the_guest`）⇒ 这证明
「阈值不能太紧」这条约束**有真实的判别力**，不是一句注释。

⚠️ **「基线不挂死 + 变异挂死」这一对才是完整的判别力证据**：基线那条活锁用例单跑
**GREEN 且 0.00 s 自己退出**（**不是**被超时杀掉），而 B 与 F 两条变异**确实挂死**
⇒ 守卫**真的接上了**，不是「加了个计数器没人看」。

⚠️ **一条工具事实（本批踩到的坑）**：任务书给的 `perl -e 'alarm N; exec(@ARGV)'` 包装
**实际不生效** —— POSIX 规定 **`exec` 会重置 alarm 定时器** ⇒ 那个写法**根本不会超时**
（本批实测：测试二进制因此一直烧 CPU 十分钟不被杀，统筹方两次手动 kill 才解围）。
⇒ 正确写法是「父进程**不 exec**、只 **fork**」：父进程留着 `alarm`，超时后按
**进程组** `SIGKILL`（`cargo` 与它 spawn 出来的测试二进制**一起**被杀 ⇒ 不留孤儿进程），
超时退出码 **124 记为 HANG**。W2 用的实现见 `/tmp/probe/watchdog.pl`。

### 3.13.6 本轮方法论沉淀

1. **「用例绿」≠「它测的是生产路径」**（本轮主线教训）。断言的落点必须在**生产路径**上：
   手工往视图状态里塞一个值，等于**造了一个只有测试才有的世界** ⇒ 生产接线断没断，它都绿。
   ⇒ **判别式**：这条用例能不能**在生产接线被拆掉时变红**？不能 ⇒ 它没在测生产路径。
2. **「恒绿」的判别式必须是「把生产代码改坏」**，不是「把某个纯函数改坏」。
   ⇒ 本轮两条零红集（**M8 / M9**）的共同点：它们改的是**生产代码**（`app.js` 的分支、`app.js` 的清理），
   而**用例断言的是别的东西**（信号 / 空壳 stub）。
3. **反向变异的「对照」是发现驱动器 bug 的唯一手段**（V-D 第一轮 7 条全废的教训）。
4. **「规格承诺了机制、代码没有机制」要如实登记**（V-D 的 **VD-F4 / VD-F6**）：
   LLD 要求的 watchdog 一行未实现，代码注释却标成「已实现」并**引用不存在的章节**。
   ⇒ **注释里的章节号也要核**（V-D 实测 `4.12` 在 LLD 里出现 0 次）。
5. **「实测」两个字不保证它对**（v0.21 的教训，本轮再次坐实）：台账 **D-93** 的原口径是 **IALIGN=32**
   （无 C 扩展），而平台 `misa` 声明支持 C ⇒ **IALIGN=16** ⇒ 掩 `~1` 不是 `~3`。
6. **文档与代码冲突时以代码为准，并登记冲突**（V-C 的裁决 D vs 裁决 I、C-1 vs 裁决 I）——
   ⚠️ **规格自身互相矛盾时，必须改规格**，否则「验收判定」自相冲突。
7. **`AGENTS.md` 是 AI Agent 的第一入口**：它一旦说反，接手的人会**重开已闭环的欠账**（V-A 的 **VA-04**）。

### 3.14 v0.24 遗留清账批（把 v0.23 的遗留一次做掉）

> 冻结规格 `docs/detailed-design/STAGE-9_V024_Leftovers.md`（裁决 A~I + 22 条验收，**全部勾选**）
> 分支：`feature/v024-j-m11-lines` · `feature/v024-h-contract-host` ·
> `feature/v024-i-session-m15` · `feature/v024-k-frontend`
> 门禁：工作区 **1 238** 全绿 · 前端 **288 / 50** · 黄金 **67/67** · **8 项门禁失败项数 0**

#### 3.14.1 本批的**四条 lane** 与**文件所有权切分**

| lane | 拥有 | 交付 |
| --- | --- | --- |
| **J** | `crates/kernel/src/m11_symbol/**` | `disasm_with_lines` + `line_resolutions()` + `SymbolIndex::resolve_line_entry`（**24 条**：单元 13 + 集成 11） |
| **H** | `crates/contract/src/{lib,protocol,error,model}.rs` · `front/lib/message.js` · `crates/kernel/src/kernel_host/**` | 契约 **v5** + `WriteConsole` 命令 + 宿主接线 + 反汇编行号接线（**14 条**） |
| **I** | `crates/contract/src/{session,port}.rs` · `crates/kernel/src/m15_peripheral/**` · `m12_engine/mod.rs` · `m20_snapshot/mod.rs` · `m23_datastore/memory.rs` | 会话快照 **v2** + CLINT / PLIC 进快照 + UART 七寄存器 + **RX FIFO 加深到 16 字节** + M-12 的 `E_ENG_*` 去重（**41 条**） |
| **K** | `front/**` · `tools/browser-verify.mjs` · `tools/elf-symtab.mjs` | 终端输入框 + 源码树入口 + 反汇编逐条行号 + 真机三条判别式（**20 条 / 3 suite**） |

⚠️ **并行铁律的落地**：J / H / I / K 四条 lane 的**文件所有权两两不相交**，
每条一个独立 worktree + 独立 `CARGO_TARGET_DIR` ⇒ **0 merge 冲突**。
⚠️ **依赖顺序**：契约层加 `CommandPayload` 变体会**打穿**内核 crate 的穷尽 `match`
⇒ H 必须**自带**那条 dispatch 臂；I 改 `UartStateDto` 会打穿**所有**全字段字面量
⇒ 那些字面量（`contract/{session,port}.rs` · `m15_peripheral/{uart,tests}.rs` ·
`m20_snapshot/mod.rs` · `m23_datastore/memory.rs`）**全部划归 I**。

#### 3.14.2 闭环对照表

| 欠账 | 本批怎么闭环 | 判别力 |
| --- | --- | --- |
| **D-121** | M-11 的反汇编器调 `resolve_line`，填真 `file` / `line` | 「查不到」填 `None`（**不是** `Some("")` / `Some(0)`）·「不拿别的地址的行号顶替」· 解析器**按条目自己的地址**被问 |
| **D-102** | 上条的内核侧 + 宿主接线 + 前端逐条显示 | 真机 E-6 实测 `file = /tmp/osz-fresh/kernel/base/task/src/task.c`、`line = 41` |
| **D-117** | 契约层加 `Clint` / `Plic` 变体；M-15 侧补三个方法 | `mtimecmp` 与 PLIC 的优先级 / 使能 / 门限**往返一次后不变**；四种畸形状态**被拒**；两个器件**对调**也拒 |
| **D-118** | `UartStateDto` 加七个 ns16550 寄存器 | `LCR = 0x83`（**DLAB 置位**）往返后仍置位、除数 `0x010C` 不变 |
| **D-119** | 契约层加 `WriteConsole`；宿主喂 RX FIFO；前端加输入框 | 真机 E-8 实测 `LSR` = `0x63`、bit0 置位；`mcause` = **`0x8000_000B`** |
| **D-120** | M-12 引用契约层的具名常量 | **等值断言** + **行为对拍**（只查定义处会漏掉「引用错了但值恰好一样」） |
| **D-122**（新登记） | 接收侧加深成 `RX_FIFO_CAPACITY = 16` 的队列 | 队列满时**丢最新**并锁存溢出位（变异 M6 把「丢最新」改成「丢最早」⇒ **红 2 条**） |

#### 3.14.3 反向变异自检（合计 **19 条 + 4 条对照**，全部按期望变红）

| lane | 变异条数 | 值得记的两条 |
| --- | --- | --- |
| **J** | 4 + 1 对照 | **M2**「缓存命中也重新查行表」⇒ 红 2 条（把「每条只查一次」这条**性能承诺**变成可证的）· **M4**「把 `Missed` 也当成有行」⇒ 红 4 条 |
| **H** | 5 + 1 对照 | **M2**「绕过同步器手动抬线」⇒ 红 1 条（`without_the_rx_interrupt_enable_no_line_is_raised`）—— 抓的是「抬线有两个真相」 |
| **I** | 8 + 1 对照 | **M7**「读 `RHR` 不弹队头」⇒ 红 **12** 条（跨单元 + 跨集成）· **M2**「两个新器件的校验退回默认放行」⇒ 红 6 条 |
| **K** | 5 + 1 对照 | **M3**「重渲染不恢复草稿」⇒ 红 2 条（**不丢草稿** + **不丢监听器**）—— 抓的是「接线用例钉不住的那一半」 |

⚠️ **还原一律用 `cp` 备份复制 + `md5` / `cmp` 逐字节校验 + `touch` 刷新 mtime**，
全程**未用** `git checkout -- <file>`；变异期间**绝不**改会被编译 / 加载的源文件。

#### 3.14.4 本批的方法论沉淀

1. **规格里的「复数」就是一条容量约束**（本批主线教训）。裁决 B 写的是「把**字节**塞进
   UART 的 **RX FIFO**」⇒ 规格**假定** FIFO 装得下多个字节，而实现只做到了**字面最小值**
   （单字节）⇒ 一行 shell 命令根本送不进去。
   ⇒ **判别式**：凡是规格里出现「把 N 个东西放进某个容器」，都要问一句「这个容器**装得下 N 个吗**」。
2. **「规格写了什么」也要对着实现核一遍** —— 规格本身也可能是错的，或者实现只做到了规格的
   字面最小值（这与 v0.22 的「规格承诺了机制、代码没有机制」是同一族教训）。
3. **重渲染会毁掉输入框**（接线层的陷阱）。`body.innerHTML = html` 每次重建视图体
   ⇒ 绑在子元素上的监听器**会丢**、**打了一半的字会被清空**；而 console 事件一来就重渲染一次
   ⇒ **每来一个字节清一次**。
   ⇒ **判别式**：接线用例只能证明「写它那一刻」的形状 ⇒ **新加的交互路径必须自己再被用例覆盖**
   （本批为此专门钉了「重渲染不丢草稿」与「重渲染不丢监听器」两条）。
4. **构建目录与运行目录必须同一个真相**（门禁抓到）。`tools/boot-osz.sh` 的**构建**认
   `CARGO_TARGET_DIR`、**运行**写死 `$REPO_ROOT/target/release/...` ⇒ 设了
   `CARGO_TARGET_DIR`（**项目纪律要求每 lane 独立 target 目录**）时跑的是**上一批**的旧二进制。
   ⇒ **判别式**：凡「构建」与「运行」在同一脚本里出现，路径必须**从同一个变量派生**。
5. **「一个约束一个落点」要落到**模块**，不只是落到**文件**。数值策略的标量形态住在 `numstr.rs`、
   列表形态住在 `session.rs` ⇒ 同一约束落了两个点 ⇒ 折进 `numstr.rs`。

---

## §4 模块报告索引与模块内状态

| 模块 | 报告 | 状态 |
| --- | --- | --- |
| M-14 | `M-14_BusMemoryMap_Review.md` §四 | ✅ 模块内 100% 闭环；#6 上收为 D-10、#9 上收为 D-13（跨模块，均未动） |
| M-15 | `M-15_PeripheralModel_Review.md` §五 | ✅ 模块内 100% 闭环（N1/N2/N3 已落地）；#11 上收为 D-12、N4 上收为 D-14 |
| M-18 | `M-18_Probe_Review.md` §一·五 | 13/15 闭环；#10 契约 `Probe` trait 死契约（与 M-15 同源）、#12 `drain_waveform` 借用交错（均低危） |
| M-19 | `M-19_WaveformPipeline_Review.md` | ✅ 16/16 闭环（`cee6b61`），含 1 项阻断级（`Clone` + `capacity()` debug panic，M-18 同步修复） |
| 前端 M-01/02/03 | `FRONTEND_Review.md` | 🟢 2026-10-20 首次模块级 review → **同日全部闭环**（20/20，含 3 阻断与 4 项评审漏项）；新增前端门禁 38 用例 / 7 suite，`fc83e1e`~`6fc9e1f` + 复审闭环。⚠️ 其中 2 项漏项（#19 / #20）**只有真实浏览器能暴露**——修完白屏后视图体仍全空。**复审（`FRONTEND_Review.md` §八）确认 16 项闭环无误，但门禁存在 3 处盲区**（#10 / #11 / #12 改回去仍绿）**+ 1 处位置耦合缺陷**（`app.js:81` 的 Promise.all 下标解构）——✅ **2026-10-20 复审闭环**：3 处盲区已封、位置耦合已消除、stub 5 处保真度缺口补齐（详见 `FRONTEND_Review.md` §八·7） |
| M-13 | `M-13_ISS_Review.md` | ✅ **v0.11 首次模块级 review**（13 项问题全部闭环，含 1 项影响全部负立即数的真实缺陷：S/B/J 型立即数符号扩展位宽少算一位）；RV32I 41 条落地；**M/C 扩展已于 v0.16 落地（RV32M 8 条 + RV32C 26 条，台账 D-43 闭环，golden 51/51）**。**2026-10-20 检视轮**：初版**漏报** 1 项阻断级分类缺陷（SYSTEM 未校验 `funct3`，`M-13_ISS_Review.md` §一 #14），已由 `caf7f82` 修复；另有 3 项覆盖漏洞（`sequential` 步进 / 地址回绕 / 子字存储截断契约）补齐；`M-13_ISS_Review.md` §一 #6 的「契约要求非对齐 trap」已订正（契约层零处提及） |
| M-12 | **尚无模块报告** | ⚠️ v0.11 已完成 D-05 / D-06 / D-17 的**加固与自测**，但**未做独立模块级 review**（T2 的任务范围不含 review 文档）。M-12 从未被独立检视过——与 M-06 同性质的盲区，**已登记为 D-30，列为 Wave 2 待办** |
| **收口轮汇总** | **本文件 §3.1** | 📋 **收口轮 3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）的完整证据归档**：37 项发现逐条实证、3 张变异测试表、**逐条闭环对照表**、数字核对表、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
| **第 1 阶段检视轮汇总** | **本文件 §3.4** | 📋 **第 1 阶段 3 路独立检视（V-A Rust 内核 / V-B 前端与桥 / V-C 跨模块与文档）的完整证据归档**：13 项发现逐条实证、**13 项反向变异自检全部变红**、逐条闭环对照表（22 行）、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
| **D-46 检视轮汇总** | **本文件 §3.5** | 📋 **D-46 检视轮 3 路独立检视（V-A Rust 内核 / V-B 前端与工具 / V-C 一致性与文档）的完整证据归档**：25 项发现逐条实证、**22 条反向变异自检（20 条变红）**、逐条闭环对照表、方法论沉淀 |
| **D-07 检视轮汇总** | **本文件 §3.6** | 📋 **D-07 检视轮 3 路独立检视（V-A Rust 内核 / V-B 测试判别力 / V-C 文档一致性）的完整证据归档**：43 项发现逐条实证、**25 条反向变异自检**、逐条闭环对照表、方法论沉淀 |
| **M+C 扩展检视轮汇总** | **本文件 §3.7** | 📋 **M+C 扩展检视轮 3 路独立检视（V-A Rust 内核 / V-B 测试判别力 / V-C 文档一致性）的完整证据归档**：35 项发现逐条实证、**3 轮共 45 条反向变异自检（44 条变红）**、逐条闭环对照表、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
| **检视轮汇总** | **本文件 §3.2** | 📋 **本轮 4 路独立检视的完整证据归档**（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）：22 项发现、逐项实证、变异测试结论、修复与反向验证记录、F-5 专项裁决依据、数字口径订正。**AGENTS.md 要求的「检视产物」** |
| M-06 | `M-06_ELFLoader_Review.md` | ✅ **v0.11 首次模块级 review**（17 项问题 / 12 项闭环 / 5 项留账并说明理由）——**该模块诞生以来第一次被系统检视，原是台账最大的盲区**。**2026-10-20 检视轮**：V3 判定 PASS（~26 000 次随机 / 边界输入 0 panic）；报告的 D-10「M-14 侧未做」状态已过时（`60dbbb8` 早已补齐），D-22 的保证性质已补注（契约性保证而非回归保护），#17 嗅探假阳性已由 `fd3d036` 修复 |
| 前端 M-04/05/07/10 | `FRONTEND_ServiceLayer_Review.md` | ✅ **v0.11 首次模块级 review**（服务层从空壳到真跑通；7 项契约缺口留 Wave 2） |

**下一步建议**：**D-04 / D-05 / M-06 盲区均已消除**。下一个盲区是 **M-11 符号表/DWARF**（10 人日，最大风险项，从未开工）与 **9 个纯 stub 模块**（M-08/09/11/16/17/20/21/22/23）。Spike 工具链已就绪，**M-13 阶段 4 golden 对拍可立即启动**。

---

---

## §5 变更记录索引

> 本节是**事件级索引**：日期 · 事件 · 摘要 · 关键 commit。**改动的完整叙事、决策记录与工作量在 `docs/progress/PROGRESS.md` 的对应版本段**——那里是唯一权威副本。

| 日期 | 事件 | 摘要 · 关键 commit |
| --- | --- | --- |
| 2026-10-23 | **内存图通用化补登记（台账新登记 D-123）+ 仓库 README 重写** | **纯文档批**（零代码变更）。一、**新登记 D-123**：内存图今天是宿主**硬编码的两档**（`MemoryProfile::{Default, QemuVirt}`）⇒ 换平台要改宿主代码；⚠️ **根因不是内存图形状的必然，而是 RAM 急分配**（`m14_bus/mod.rs:752` 的 `vec![0u8; size]` ⇒ 128 MiB 段 = 128 MiB 物理内存 ⇒ 撞每区域 16 MiB 上限 ⇒ 被迫拆 8 段 ⇒ 才需要一档 128 MiB）⇒ 通用化按 **C 做法**（**A** RAM 页级懒分配 + **B** 内存图由工程/平台描述 + **C** RAM 区域按需增删）；⚠️ **必须守住的分界**：**region 登记与页懒分配是两层**——「没登记」⇒ 未映射 ⇒ 架构陷阱 cause 5 / 7（**D-99 的判别力靠这条**），「登记了但没写过」⇒ 读回 0 ⇒ 合成一层会**当场破掉 D-99**；⚠️ **实测代价**：QemuVirt 档一次会话快照 = **258.06 MiB** JSON（RAM 段 129.00 MiB，**几乎全是零**）⇒ 懒分配能砍掉两个数量级；**建议顺序：先 A 后 B**。二、**§0.1 合计重算**：495/398/97 → **496/398/98**，**真实未闭环数 69 → 70**（D-123 是新登记未闭环 ⇒ 直接计入，不参与「闭环不回填」的减法）。三、**§1.3 表结构修正**：v0.24 批的 **D-122 原先悬在 §1.3 表外**（`---` 与 `### 1.4` 之间的一行孤立表格行）⇒ 已归位到 §1.3 表内（**内容一字未改**，只换位置）。四、**README 重写**：旧 README 停留在 v0.10（写着「未 `git init`」「14 个模块是 stub」）⇒ 按当前树重写（能力快照 / 快速开始 / 架构图 / 模块地图 / 三层正确性证据 / 8 项门禁 / 已知缺口 / 文档索引）。五、**§5 补齐前两批**（v0.23 与 v0.24 此前**没有**进事件级索引）。 |
| 2026-10-23 | **v0.24 遗留清账批：把 v0.23 的欠账一次做掉（台账 D-102 / D-117 / D-118 / D-119 / D-120 / D-121 闭环，新登记 D-122）** | 冻结规格 `docs/detailed-design/STAGE-9_V024_Leftovers.md`（**9 裁决 + 22 条验收 + 8 项门禁 + 9 件不做的事**）· **四路 lane 按「文件所有权」切分**（J = M-11 / H = 契约 + 宿主 / I = M-15 + M-12 / K = 前端 + 真机；J 与 H 串行、H 与 I 并行；每 lane 独立 worktree + 独立 `CARGO_TARGET_DIR` ⇒ **0 merge 冲突**）· **Lane J**：M-11 新增 `disasm_with_lines` ⇒ 反汇编面板填**真** `file` / `line`（查不到就是 `None`，**绝不编造**、**不拿别的地址的行号顶替**；旧 `disasm` 签名**一字未改**）+ `line_resolutions()` 计数器 ⇒ 「每条指令只查一次行表」**可证**（+24 用例）· **Lane H**：契约层升 **v5**（新命令 `WriteConsole { bytes }`）+ 宿主接线（**抬线只由同步器做** ⇒ 一条命令不产生两个真相；**回复必须至少一个事件** ⇒ 前端 Promise 才落地）· **Lane I**：**CLINT / PLIC 进会话快照**（契约 `SESSION_SNAPSHOT_SCHEMA_VERSION` 1 → **2**，旧载荷被 `E_SNAP-05` **显式拒绝**）+ UART **ns16550 七寄存器**进快照（`LCR.DLAB` 位本身也要还原）+ **RX FIFO 加深到 16 字节**（**规格里的「复数」就是一条容量约束**）· **Lane K**：终端输入框（草稿存 M-03 视图状态、监听器绑容器 ⇒ **重渲染不丢草稿 / 不丢监听器**）+ 源码树入口 + 反汇编逐条行号 · ⚠️ **门禁抓到真缺陷**：`tools/boot-osz.sh` 的**构建目录与运行目录不一致**（build 认 `CARGO_TARGET_DIR`、运行写死 `$REPO_ROOT/target` ⇒ 症状极具误导性）⇒ **「构建目录与运行目录必须同一个真相」**· **门禁**：8 项**失败项数 0**（workspace **1238** 全绿 / 前端 **288/50** / 黄金 **67/67** / osz ⇒ **VERDICT MATCH 31/31** 两侧逐字节相同）· 关键 commit `e5e59dd`（规格）· `cbb7cd5`（Lane J）· `1b62b04`（Lane H）· `0a354e5`（Lane I）· `3bda9ea`（Lane K）· `c407fb0`（数值策略收拢）· `5dba2be`（boot-osz 缺陷）· `f6d3045`（样式）· `aa5082c`（文档收口）· `1acf582`（合 main）· → 叙事见 `PROGRESS.md` v0.24.0 段 |
| 2026-10-23 | **v0.23 osz 落地批：让真实 M 模式 RTOS 在本平台上跑起来（台账 D-47 / D-48 / D-116 闭环，新登记 D-117 ~ D-121）** | 冻结规格 `docs/detailed-design/STAGE-8_OSZ_Runtime.md`（**37 条验收**）· **判决口径 = 两侧 UART TX 字节流逐字节相同**（基准 31 字节）⇒ **不复用** riscv-tests 的 HTIF `tohost` 判决（真实 RTOS 没有 `tohost`）· **Lane A**：`mcause` 架构码 + `wfi` + `mhartid` · **Lane B**：UART **重做成 ns16550**（8 个字节寄存器 / `LCR.DLAB` 真切换 / `LSR` 按位义重排 / **无损 TX 捕获环** 4 KiB）⇒ ⚠️ **LSR bit5 = THRE**（不是「环是空的」）——osz 的 `hal_uart_putc` 是自旋，取错语义会让 osz **每发一字节就卡死一次** · **Lane C**：**新建** `clint.rs` / `plic.rs`（QEMU virt 布局；⚠️ PLIC claim **判据是严格大于门限**）· **宿主接线**：**两档内存图** + 三外设映射 + **三条中断通路**（CLINT `mtimecmp` 周期↔M-16 ps · UART RX→PLIC 源 10 · PLIC 待交付源→M-16 外部线，⚠️ **claim 是 guest 的权利**、平台只读不代 claim）+ **console 通路**（UART TX 捕获环 → 64 KiB backlog → 订阅 `sim.console`）· **Lane D**：契约层升 v4 + **Console 事件** + 传输层欠账 D-47（帧 `seq` 双序列空间）/ D-48（`E_TRX-04` 一码三义 ⇒ 拆出 `E_TRX-05`）· **Lane G**：浏览器里把 console **上屏** ⇒ **真 Chrome** 复验 · **门禁**：8 项**失败项数 0**（workspace **1159** 全绿 / 前端 **268/47** / 黄金 **67/67**）· 关键 commit `52f9168`（Lane A）· `119e20a`（Lane B）· `4557fb1`（Lane C）· `ea686bf`（宿主接线）· `0c6a35d`（Lane D）· `7697f0d`（Lane G）· `0d1b282`（合 main）· → 叙事见 `PROGRESS.md` v0.23.0 段 |
| 2026-10-22 | **Wave 2a：DWARF 行表真解析 + 访存异常陷阱化（台账 D-93 / D-98 / D-99 / D-100 / D-101 闭环，新登记 D-102 ~ D-112）** | 分支 `feature/wave2-dwarf-lines`（基线 `b443eb5`）· 冻结规格 `docs/detailed-design/STAGE-7_Wave2_DWARF_Lines.md`（10 裁决 + 31 验收）· **M-11 的 `dwarf.rs` 从「只识别不解析」换成真解析器**（`.debug_line` 的 v2/v3/v4/**v5** 行号程序 + 行号状态机 + 特殊操作码 + 按单元降级 + 多单元归并）· 前端**源码视图**（源码树只进 M-03 视图状态、**不进内核**；路径尾部逐段匹配 + 正规化；多候选**列出让用户选**、⛔ 不静默挑第一个）· **未映射访存 ⇒ 架构陷阱**（cause 5 / 7；`mtval` = 故障地址；⚠️ 根因是 `BusMemory` 把**所有**总线错误重包成 `ISS-01` ⇒ 改为**保留 family / scenario**）· **`mepc` 三路径口径**（用户写掩 `~1` / 陷入门写精确 PC / `mret` 再掩 `~1`，**IALIGN=16** 非 32）· **`mstatus` WARL 掩码** + `pending_lines` → `Option` + `restore_full` 走 CSR 表过滤 + `sh_entsize` 校验落 M-06 · **四路独立检视**（V-A PARTIAL 12 条 / V-B FAIL 8 条 / V-C FAIL 15 条 / V-D PARTIAL 9 条）⇒ 查出**一条生产路径真断**（源码视图永远拿不到 `sim.pc`，而用例因**手工注入**而绿）+ **三条恒绿断言** + **六处假陈述** ⇒ 全部修掉并逐条做反向变异 ⇒ 叙事见 `PROGRESS.md` v0.22 段 · 检视归档见本文件 **§3.13** |
| 2026-10-22 | **CSR / 陷阱黄金覆盖：让对拍第一次照出 CSR 语义缺口（台账 D-84 / D-85 / D-90 闭环，新登记 D-91 ~ D-101）** | 分支 `feature/golden-csr-coverage`（基线 `b014fed`）· 冻结规格 `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md`（8 裁决 + 20 验收）· **新增 CSR-capable 黄金 env 变体**（`tests/golden/env/csr/` + `encoding.h`）⇒ 语料 **51 → 64**（13 条 `rv32mi` 进表）· **`mip` 生产者**（M-16 挂起队列 → 契约层掩码 → M-17 写侧落盘）· **`misa`**（0x40001104）· **未实现 CSR 写忽略** · **陷入时 `mstatus.MPP ← M`**（两路检视独立复现的真分歧）· 收口轮：**两路检视各判 FAIL** ⇒ 订正规格里 4 处**实测为假**的基线、补 3 条「空洞的一致」标注、harness 加 **env 常量硬门禁**（当场抓到 `PMP_A` 漂移）· → 叙事见 `PROGRESS.md` v0.21 段 |
| 2026-10-22 | **M-11 符号表 / DWARF / 反汇编 / trace 导出落地（台账 D-50 / D-51 闭环，新登记 D-86 ~ D-90）** | 分支 `feature/m11-symbol` · 冻结规格 `docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`（裁决 A~H）· **契约层升 v3**（4 命令 / 4 事件 / 3 DTO，**只增不改**）· M-11 **3 206 行 / 75 条用例**（五个子模块 + `SymbolService` 端口）· **反汇编复用 M-13 的 `Decoder`** ⇒ **零第二套解码表**（LLD §3.4 的「独立表 + 一致性门禁」作废）· **宿主路由导出** ⇒ **D-50 / D-51 双闭环**（D-50 的判别式是**排除法**：抽干 M-18 的边沿存储后导出仍有内容 ⇒ 只可能来自 M-19 的环）· **两路独立检视**（V-A PASS / V-B PARTIAL）**8 条已修 + 4 条留账** ⇒ 收口时自己抓到的三处缺口（**访存轨迹时间戳恒 0** · **缓存缺按地址查询 API** · **缓存失效被挂在记录开关上**）· 门禁：workspace **1012 全绿** + 前端 **228/39** + WASM 冒烟 + 真 Chrome + 黄金 **51/51** · 关键 commit `c9d1fca`（规格）· `9c5705d`（契约 v3）· `4d25f6e`（M-06 调试节区）· `c802a4e`（前端面板）· `5415866`（M-11 核心）· `60dfbd8`（宿主接线）· `31e2c95`（门禁修版本硬编码）· `9a3efb9`（检视修复） |
| 2026-10-21 | **M-16 中断控制器落地：中断交付整条链接通（台账 D-26 / D-59 / D-63 / D-64 / D-70 / D-72 / D-74 闭环，新登记 D-83 ~ D-85）** | 分支 `feature/intc-controller` · 冻结规格 `docs/detailed-design/STAGE-4_Interrupt_Controller.md`（裁决 A~I）。**M-16** 从 44 行 stub 变成**完整控制器**（挂起队列按 `(虚拟时间, 插入序)` 保序 / 同线去重 / 优先级仲裁 / 量子边界采样（`mie` + `mstatus.MIE` 两层闸门）/ **拉取式**交付 / 定时器电平语义 / 会话状态导出导入）· **M-13** 补 **CSR 指令族**（csrrw/s/cs/c/wi/si/ci）+ `mret` + **唯一**陷阱入口 `enter_trap`（`mtval` 落 CSR `0x343`，**不升** `schema_version`）· **M-17** 接线（取指边界按「跨过量子」采样 → 交付 → 陷入 → guest 写 `irq_clear` 清线）· **宿主** 补 `raise_irq` / `lower_irq` / `clear_irq` / `set_timer_compare` · **契约层** 新增 `crates/contract/src/csr.rs`（CSR 编号 / `mie` 位映射 / 只读规则）· 端到端 6 条（固件是**汇编器产出**的字节，汇编源进仓库）。⚠️ **两路独立检视（V-A 判 FAIL / V-B 判 PARTIAL）独立收敛到同一个 P0**：`irq_clear` 的副作用挂在「**值变了**」上，而 guest 每次都写**同一个**掩码 ⇒ **第二次清线被静默丢弃** ⇒ `active_irq` 永不复位 ⇒ 那条线被单级防护**永久堵死**；而两条「能再来一次」的用例**只数 handler 跑了几次** ⇒ **假绿**，且把缺陷**钉成了期望**。⇒ **机制换掉**（M-13 回报「刚写了哪个 CSR」+ M-17 用回报做判据）⇒ 两侧断言推进到「第二次交付之后的控制器状态」。**门禁**：fmt / clippy / test（**885 全绿**）/ front（208）/ wasm-build / wasm-smoke / browser-verify（真 Chrome）/ golden（51/51）全过。⚠️ **收口轮：反向变异自检 49 条 · 全量重跑**（§3.10.4）—— 变异集经**两次逐条对账**从 37 条扩到 49 条（补 **K-33~K-37** 验收缺口 · **K-38~K-42** CSR 六条臂逐臂 · **K-43 / K-44** 验收清单二次对账；**K-31** 改写为**可编译**形态 —— 原形态会破坏所有结构体字面量构造 ⇒ 编译失败 ⇒ 变异**无效**）⇒ **变异自检的工具链自己修掉七处缺陷**（红集 AND 语义 / 硬超时 + 杀进程组 / 备份键 / **编译与运行超时分离** / **跳过 doc-test** / **HANG 判据改看「二进制启动没有」** / **规模侧：收敛 target 集 + 预热**）⇒ 实证：**8 条「HANG」里 7 条是假的** —— 根因是 macOS 对**新链接**二进制的 **`exec` 扫描**（实测**每文件固定 ≈25 秒**，与体积无关 ⇒ 一轮 12 个二进制 = **300 s** ⇒ **正好打满**旧的超时窗口）⇒ **只有 K-10 是真活锁** ⇒ 旧口径的 HANG 判定**整体不可信** ⇒ **全量重跑 49 条**（一条不省）⇒ 「F-1 的后果是平台活锁」这个结论**是错的**（准确表述：那条线被**永久堵死**；**会不会挂死取决于哪条路径**）。关键 commit：`71fcb89`（规格冻结）· `7ba685c`（契约层）· `d906dda`（M-13）· `33b5958`（M-16）· `8ffef91`（接线）· `50e5c6d`（端到端 + D-72）· `f2a00d2`（**F-1 闭环**）→ 叙事见 `PROGRESS.md` v0.19.0 段 · 检视归档见 `REVIEWS.md` **§3.10** |
| 2026-10-21 | **会话级快照载荷：落盘一个会话，还原出一个「等价会话」（台账 D-36 / D-37 / D-41 闭环）** | 台账 §2.2 优先级表**唯一未完成的条目（5b）**。**契约层**新增六段会话载荷 `SessionSnapshotDto`（**段 0~5**）· **M-14 / M-15 / M-16** 补状态导出与载入接口（`export_map`/`load_map` · `export_pages`/`load_pages` · `session_state`/`restore_session_state` · `export_state`/`load_state`）· **M-20** 按六段组装（`capture_session`）+ `record` 落记录（`SnapshotRecord { id, payload, label }`——**无第二份 `cp_meta`**）+ `validate_session`（**只校验**；⚠️ **同名不同义** = 「一码多义」的变体）· **M-23** 改吃新结构 + `InMemoryDataStore` · **宿主** `capture_session` / `restore_session` 接线（**恢复链第 0.5 步** = 只读外设预检）。**三处裁决**（规格 §1.2）：A 补 `m12Engine` 段 · B `m13State` **只**承载内存页（**寄存器不重复存**）· C 补 `m16Intc` 段（⚠️ **生产路径恒为空**，成因**两条**缺一不可）。**三路独立检视 37 项发现 → 33 项闭环 / 4 项留账**（V-C 19 + V-A 6 + V-B 12）· **新登记 3 条**（**D-79 / D-80 / D-81**）· **反向变异自检 24 条**（契约层 17 + 内核侧 7）**全部按期望变红**（含两条**语义不变**的对照必须 **0 红**）· 关键 commit：`adaf533` / `38f1c9a` / `a1a8c6c` / `0dbb9b0`（修复 lane）· `6ed7332`（Lane E）· `e5ef75a`（内核侧判别力补强）· `590848d`（契约层加法性守卫）· → 叙事见 `PROGRESS.md` v0.18.0 段 · 检视归档见 `REVIEWS.md` **§3.9** · 冻结规格见 `docs/detailed-design/STAGE-3_Session_Snapshot.md` |
| 2026-10-21 | **契约层 v2 升版：`schema_version` 1 → 2（D-32 / D-34 / D-39 / D-40 / D-57 / D-58 闭环）** | 契约层**第一次**升版（此前自 v0.10 冻结在 v1）——**虚拟时间字段改走十进制字符串**（新增 `crates/contract/src/numstr.rs`；`u64` → JSON 数字 → JS 侧 IEEE-754 double 只精确到 2⁵³，2⁵³ ps ≈ 2.5 小时仿真时间，**可达**而非理论值）· **新增** 3 命令（`ReadRegisters` / `WriteRegister` / `Negotiate`）+ 3 事件（`Registers` / `Negotiated` / `SimStateDelta`）+ `model::DeltaRegister` · **改名** `model::Waveform` → `model::WaveformHistory`、`EventPayload::WaveformDelta` → `WaveformHistory`、topic `waveform.delta` → `waveform.history`（**`model::WaveformDelta`（live 批次形）保留原名**——它不是死代码，是 D-50 的落点）· **新增** `ErrorFamily::Protocol`（`"PROTO"`，与 `Transport`/`"TRX"` **并存不合并**：TRX = 传输适配器自身的故障，PROTO = 协议层的故障）· 快照 `validate()` 增**第 7 条**检查（队列最早到期 ≥ 快照虚拟时间，报 `E_SNAP_02`）+ 新增 `is_empty()`（宿主可显式拒绝「合法但空」的默认快照）· **文档订正**（**改文档不改实现**）：`continue_until_event`（D-57）· `Peripheral::tick`（D-58）· `SNAPSHOT_SCHEMA_VERSION` 1 → 2 · 契约层用例 12 → **36 条**全绿 · **三路独立检视 44 项发现 → 38 项闭环 / 6 项留账**（新登记 **D-74 / D-75**；**D-73** 本批闭环）· **反向变异自检 21 条全部按期望变红**（第 3 轮暴露 D-73 的**自指断言**）· 关键 commit：`14da92e`（规格冻结）· `3126f54`（实现）· `7bacbdf`（错误码常量 + 工具适配）· `5b89c9a`（D-73）· `e8bc706`（V-A 契约层闭环）· `24d34f7` / `e9dfc4c`（合并前端 / 文档两路）· `5944c5d`（前端门禁不许静默跳过）· → 叙事见 `PROGRESS.md` v0.17.0 段 · 检视归档见 `REVIEWS.md` **§3.8** · 冻结规格见 `docs/detailed-design/STAGE-2_Contract_V2.md` |
| 2026-10-21 | **M + C 扩展落地：RV32M 8 条 + RV32C 26 条（台账 D-43 闭环）** | **新增** `crates/kernel/src/m13_iss/compressed.rs`（字段级展开表）· `alu.rs` 增 8 个乘除函数 · `Decoded` 增 `compressed`/`compressed_raw`/`illegal` · 用例 **55 条**（`tests_rv32m.rs` 18 + `tests_rv32c.rs` 37，**64 个编码常量全部取自 GNU as 并逐条逐字节重核**）· golden 语料 **9 例 pending → active**，**51/51 逐例判决一致** · **同时订正 M-13 LLD v1.5 → v1.6 的 4 处措辞**（取指宽度 / 展开粒度 / 链接寄存器调整点 / §3.5 编码列） · 关键 commit：`8c3e50f` / `9ec3b4a` / `4bf0eed` / `5649034` / `36ab078` · → 叙事见 `PROGRESS.md` v0.16.0 段 |
| 2026-10-21 | **M+C 扩展检视轮闭环**（V-A PARTIAL / V-B PASS / V-C PARTIAL） | **35 项发现 → 30 项闭环 / 5 项留账**（新登记 **D-63 ~ D-67**）。3 处**真实判别力缺口**（CB 偏移符号位与幅度位 / `c.slli` 的 5 位 rd / `c.swsp` 的 `uimm[7:6]` 与 5 位 rs2——单测与 golden 语料**双双守不住**）已补用例；**1 处假判别式**（`the_m_extension_cannot_read_x0` 的 rs1 写成了 x3 不是 x0）已修；C.LUI 的「20 位截断」理由**算错**（实为 `sext6 << 12`），三条独立路径取证定案并订正 4 处措辞；LLD **v1.6 → v1.7**（§3.2 伪码 / `Dec` 字段 / §5.1-5.2 对齐措辞 / 版本序）· **反向变异自检 3 轮 45 条 → 44 条变红**（1 条为语义空操作）· 关键 commit：`fe081e1` / `db22172` · → 叙事见 `PROGRESS.md` v0.16.0 段 · 检视归档见 `REVIEWS.md` **§3.7** |
| 2026-10-21 | **D-07 闭环：唤醒时间驱动链路 + 检视轮** | 四处断口全闭环（没人装 ticker / 没人排 `PeripheralTick` / `pending_writes` 只写不读 / 唯一主循环 `runUntil` 无人调）+ 裁定「指令驱动 × 时间驱动如何交替」+ `TIME_QUANTUM_PS` 1 ns→1 µs 口径订正 + `InstructionExecutor` 落为 M-12 内部接缝（契约冻结，D-32）。**三路独立检视 43 项发现 → 38 项闭环 / 5 项留账**（D-57~D-60、D-62）；反向变异自检 **25 条全部变红** · 关键 commit：`2c3b83f` / `dba9728` / `9e3a869` / `9ac4df9` / **`1b2c6c9`**（检视轮闭环） · → 叙事见 `PROGRESS.md` v0.15.0 与 v0.15.1 段 · 检视归档见 `REVIEWS.md` §3.6 |
| 2026-10-21 | **D-46 闭环：探针管道接上生产总线** | 四根线一次接上（M-14 `add_hook` 追加式 hook / 外设枚举 / 外设时钟同步 + 宿主装 `ProbeRegistry` → `EdgeSink` → `WaveformPipeline`）+ demo 固件真的驱动 GPIO + 前端三处修正（过滤判别式 / 强制重订 / 边沿累加）。**12 项反向变异自检全部变红** · 关键 commit：`03d9768` / `0e439d3` / `a6ce0c2` / `cbce7b5` / `d0d0b66` · → 叙事见 `PROGRESS.md` v0.14.0 段 |
| 2026-10-20 | **第 1 阶段检视轮闭环** | 3 路独立检视（V-A FAIL / V-B PASS / V-C FAIL）共 **13 项发现全部闭环**：2 个真 bug（观察点假停机 / 取指误触发读观察点）+ 6 处测试判别力盲区 + 5 处前端口径问题；新登记 **D-47 / D-48 / D-49**。**13 项反向变异自检全部变红** · 产物见 `REVIEWS.md` §3.4 · → 叙事见 `PROGRESS.md` v0.13.1 段 |
| 2026-10-19 | 台账建立 | 从首次全量审查（仅存在于对话、未归档）与 M-14 / M-15 两份模块报告中提取全部跨模块条目，逐条回溯源码核实当前状态 · → 叙事见 `PROGRESS.md` v0.8.1 段 |
| 2026-10-19 | M-15 报告闭环 | 15 项完全闭环 / #6 半闭环（上收为 D-07）/ #11 按设计推迟（上收为 D-12）/ #18 有意保留 · → 叙事见 `PROGRESS.md` v0.8.1 段 |
| 2026-10-19 | M-14 报告闭环 | 9 项完全闭环 / #14 半闭环 / 6 项推迟；#6 上收为 D-10、#9 上收为 D-13 · → 叙事见 `PROGRESS.md` v0.8.1 段 |
| 2026-10-19 | **M-19 波形管道落地 + M-18 → M-19 握手完成** | 契约新增 `port::EdgeSink` / `port::DeltaSink` 驱动端口与 `model::EdgeEvent` / `WaveformDelta` / `DropRange` 三个 DTO（全部 a… · → 叙事见 `PROGRESS.md` v0.9.0 段 |
| 2026-10-20 | **全模块闭环状态审计 + 前端首次模块级 review** | 新建 `FRONTEND_Review.md`（前端 16 项）。 · 关键 commit：`cee6b61` · → 叙事见 `PROGRESS.md` v0.10.0 段 |
| 2026-10-20 | **D-12 状态更新为「部分推进」** | M-18 → M-19 已通（`set_edge_sink` + `EdgeSink` + 端到端集成测试）；M-14 → M-15 仍返 E_BUS-05；M-15 → M-18 三个外设均未 override `on… · → 叙事见 `PROGRESS.md` v0.10.0 段 |
| 2026-10-20 | **前端评审 20/20 全部闭环** | `FRONTEND_Review.md` 16 项首审意见 + 2 项评审漏项全部闭环：D-01/D-02 白屏阻断（ESM 导出缺失）已修，bootstrap 实测跑通；视图层收尾（demo 按钮恒不触发 / 终端双层… · → 叙事见 `PROGRESS.md` v0.10.0 段 |
| 2026-10-20 | **v0.11.0 · 首次「多路并行开发」：5 路 worktree 隔离同时施工，全绿后统一收口** | 5 个任务拆成 5 个隔离 worktree + 5 个独立分支同时开工，**文件所有权零交集 → 0 merge 冲突、0 越界改动**（`crates/contract/**` / `lib.rs` / `m17_b… · → 叙事见 `PROGRESS.md` v0.11.0 段 |
| 2026-10-20 | **并入全模块状态，删除独立 `STATUS.md`** | 原 `STATUS.md`（全模块快照）内容并入本文件 §0（模块级闭环总览 / 跨模块链路连通性）、§2（处理顺序 / 整体判断）。**理由**：避免状态分散在多处产生漂移。→ 叙事见 `PROGRESS.md` v0.11.0 段 |
| 2026-10-20 | **前端分支独立复审（`fix/frontend-review-closure`，11 commit）** | 结论见 `FRONTEND_Review.md` §八。 · → 叙事见 `PROGRESS.md` v0.10.0 段 |
| 2026-10-20 | **前端复审闭环二次复验**（`445c863`/`b3045b7`/`32b3387`/`2390bb6`） | 结论见 `FRONTEND_Review.md` §八·八。 · 关键 commit：`445c863` / `b3045b7` / `32b3387` / `2390bb6` · → 叙事见 `PROGRESS.md` v0.10.0 段 |
| 2026-10-20 | **独立检视轮：4 路并行检视 → 逐项修复（v0.11.1）** | Wave 1 交付后**先检视、后合 main**，4 路独立检视（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）共查出 **22 项**。 · 关键 commit：`caf7f82` / `bd647dd` / `7254a00` · → 叙事见 `PROGRESS.md` v0.11.1 段 |
| 2026-10-20 | **收口轮：契约新增可序列化快照 DTO —— D-24 根因闭环** | 用户决策「契约新增可序列化 DTO」后由协调者直接实现（契约层变更不走子会话）。 · 关键 commit：`3016de0` · → 叙事见 `PROGRESS.md` v0.11.2 段 |
| 2026-10-20 | **⚖️ LLD 内部矛盾裁决：以详细设计 §2.1.1 为准** | 收口轮检视 V-C 揭出 LLD 自身自相矛盾：**总纲表12 / §2 持久化数据格式 / §4.6.4.1 / 表84 / 表85** 说快照载荷是「`SimState + DebugMeta`」，而 **M-20… · → 叙事见 `PROGRESS.md` v0.11.2 段 |
| 2026-10-20 | **收口轮检视闭环：3 路独立检视（V-A 契约 / V-B 内核 / V-C 跨模块）→ 逐项修复** | 分支合 main 前**先检视、后合**（AGENTS.md 工作循环）。 · → 叙事见 `PROGRESS.md` v0.11.2 段 |
| 2026-10-20 | **D-38 闭环：WASM 构建路径跑通** | kernel 增 target-specific `wasm-bindgen` + 新增 `wasm_api.rs` 导出面；修掉 `tools/build-wasm.sh` 的输出路径 bug 与 `--target no-modules` 选型错误（改 `--target web`）。 · 关键 commit：`fe039bf` · → 叙事见 `PROGRESS.md` v0.12.0 段 |
| 2026-10-20 | **第 2 阶段：RV32I golden 对拍打通（Spike 作为外部 oracle）** | 自写**零 CSR 的最小 env**（`tests/golden/env/`）绕开 CSR 与陷阱投递，直接按 HTIF 协议写 `tohost`；新增 `riscv-golden` runner（M-06 + M-14 + M-13）与 `tools/golden-run.sh`（含**变异金丝雀**：两侧都必须报 `FAIL 3`）；语料 51 例中 **RV32I 42/42 与 Spike 判决逐例一致（含失败用例号）**。 · 关键 commit：`b830739` / `b097d95` / `e0fe6ab` / `6a370fc` · → 叙事见 `PROGRESS.md` v0.12.0 段 |
| 2026-10-20 | **第 2 阶段检视闭环：3 路独立检视（V-A runner / V-B env 与对拍有效性 / V-C 跨模块与文档）→ 逐项修复** | 判定 **FAIL ×2 / PASS ×1**。V-A 抓到**裁判本身会判错**（Spike 的 HTIF payload 只有 48 位，原实现按 64 位解码，能把 PASS 判反）；V-B 抓到**金丝雀抓不到成功路径的 bug**（10 个 ALU 变异漏检 8 个）与 **`pending` 机制容忍真缺陷**；V-C 抓到**两侧都没判决被判成一致**的假绿。🟠 全闭环，🟡 逐条处置，**D-42 闭环**（M-13 + M-14 四处 LLD 措辞订正），**新登记 D-44**。 · 关键 commit：`f9b86f3` / `cdbcffb` / `6a71564` · → 叙事见 `PROGRESS.md` v0.12.0 段 |
