# 跨模块欠账台账 · Cross-Module Debt Ledger

> **定位**：本文件是**全部 review 欠账的唯一追踪入口**——跨模块条目与模块内条目都在这里汇总状态。模块问题的**详细分析、实证记录与修复建议**见各模块报告（§八 索引）。
> **建立日期**：2026-10-19
> **状态核实日期**：2026-10-20（v0.11.0 并行开发后重新逐条回源码核对，非引用历史结论）
> **基线**：`cargo test --workspace` **406 passed / 0 failed**（契约 lib 12 + kernel lib 356 + 集成 38）；`clippy -D warnings` 干净（已删 `non_canonical_partial_ord_impl` allowance）；`fmt --check` 干净；前端门禁 **119 用例 / 21 suite**
> **关键结论**：v0.10 记录的「三条现有门禁对本台账全部 5 项阻断级条目均无感知」**已不成立**——本轮补上了 3 类最小门禁（M-13 x0 / M-12 确定性 / M-06 畸形输入），**3 条阻断级条目全部消除**。
> **本轮（2026-10-20 独立检视轮）新增**：4 路独立检视（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）共查出 **22 项**：1 阻断级（M-13 SYSTEM 未校验 funct3，已修）+ 2 高危（F-3 总线 hook panic，已修；F-1 快照语义承诺，已订正文档）+ 若干中低危。**该轮阻断级与高危已全部闭环**，中低危分流为 D-24~D-35。详见 §四·1。
> **收口轮（2026-10-20，分支合 main 前）新增**：3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）共查出 **37 项**（去重后）：**0 阻断 + 5 高危 + 32 中低危**。**三路判定均为 FAIL**，逐项修复后 **34 项闭环 / 3 项留账**（D-38 / D-39 / D-40）。⚠️ 另有 2 条**已闭环但带残留工作量**的条目——V-A 🟠-1 与 V-C H-1——其剩余部分登记为 **D-36 / D-37**（**高危，未做**）。完整证据见 `REVIEW_ROUND_2026-10-20_CONTRACT_DTO.md`。
> **D-24 已闭环（2026-10-20 收口轮）**：契约层新增可序列化快照 DTO（`crates/contract/src/snapshot.rs`），`ExecutionBackend` 增 `snapshot_full` / `restore_full`（additive 默认实现），M-12 ↔ DTO 双向转换落地，M-17 接线完成。⚠️ **该 DTO 只覆盖 M-12/M-17 段（引擎）**，不含外设（M-15）/ 中断控制器（M-16）/ 内存映射——**落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 组装**，见 **D-36 / D-37**。
> **维护约定**：本文件是**活文档**。模块报告的闭环章节记录该模块的细节，本台账只保留**汇总状态 + 跨模块拓扑 + 处理顺序**；两者冲突时以本台账的核实日期为准。

---

## 一、全局状态快照（2026-10-20 · v0.11.0 并行开发后）

### 1.1 模块级闭环总览

| 模块 | 代码 | 测试 | 报告项 | 闭环 | 未闭环 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| **M-14 总线** | 1 361 → 1 932 | **38** | 16 + 接线 PR | **16** | 0 | 🟢 **接线 PR 后全清** |
| **M-15 外设** | 1 424 → 1 748 | **35** | 19 + N1~N4 = 23 | **23** | 0 | 🟢 **接线 PR 后全清** |
| **M-18 探针** | 1 893 → 2 254 | **36** | 15 | **13** | 2（低危） | 🟡 差 2 项低危 |
| **M-19 波形** | 3 897 | 85 | 16 | **16** | 0 | 🟢 **已清零**（`cee6b61`） |
| **前端 M-01/02/03** | 1 123 → 1 594 | **21** | 20（16 首审 + 4 漏项） | **20** | 0 | 🟢 **已清零**（`fc83e1e`~`6fc9e1f`） |
| **M-06 加载器** | 1 189 → 2 652 | **52** | **17（v0.11 首次模块级 review）** | **13** | 4（留账并说明理由） | 🟢 **已审查 + 加固**（v0.11，检视轮再修 3 项） |
| **M-12 引擎** | 226 → 1 373 | **22** | 台账 3 项（D-05/06/17） | **3** | 0（**但无独立模块级 review** → D-30） | 🟢 **已加固**（v0.11，收口轮再增快照 DTO 转换层） |
| **契约层** | 1 163 → 1 851 | **12** | 检视轮 D-24 根因修复 | **1** | 0 | 🟢 **新增可序列化快照 DTO**（收口轮） |
| **M-13 ISS** | 456 → 2 962 | **88** | 14（v0.11 首次模块级 review + 检视轮补 #14） | **14** | 0 | 🟢 **RV32I 全集落地**（v0.11，检视轮修 4 项） |
| **前端 M-04/05/07/10** | 66 → 2 220 | **71** | 7 项契约缺口 + 3 自查缺陷 | 3（已修） | 7（契约缺口 → D-32，留 Wave 2） | 🟢 **已实现**（v0.11） |
| **M-08/09/11/16/17/20/21/22/23** | 591 → 645 | — | 无 | — | — | ⚪ 纯 stub，未开工（M-17 因快照 DTO 接线 +51 行；M-20/M-23 仍待按 LLD 接入 → D-37） |

> **统计口径**（2026-10-20 检视轮重算，V4 指出此前数字失真）：「代码」= 该模块目录下**全部 `.rs` 文件的总行数**（含测试文件）；「测试」= 该模块目录下 `#[test]` 的**用例条数**。前端：「代码」= `front/views`（1 042）+ `front/app.js` / `index.html` / `style.css`（552）= 1 594；M-04/05/07/10 = `front/services`（1 241）+ `front/lib`（414）+ `front/dev`（565）= 2 220。集成测试另计：M-06 5 + 接线 13 + M-18→M-19 5 + 快照 DTO 15 = **38 条**。前端门禁单列：**119 用例 / 21 suite**。

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
| **合计** | **172** | **143** | **29** |

> **两级计数的口径**：本表数的是**检视发现**（finding），不是台账条目（D-xx）。一个 finding 被闭环后，若它指向的**剩余工作量**需要单独立项，就登记为新的 D-xx，**不回填本表的未闭环格**。收口轮即此情形：**D-36 / D-37 / D-41** 是 V-A 🟠-1 与 V-C H-1 两条**已闭环但带残留**的发现所派生的工作量单——前者（措辞收窄 / LLD 同步）已闭环，后者（外设与 INTC 状态序列化、M-20/M-23 接线、M-13/M-14/M-15 状态导出接口）单独立账。
> 因此「高危 12 条中 9 条闭环」与「合计未闭环 29」**不矛盾**：前者是 §三 表的条目口径，后者是本表的发现口径。

### 1.2 跨模块链路连通性

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

---

## 二、阻断级（5 项 · **3 项已闭环**，余 2 项）

| ID | 归属模块 | 来源 | 问题 | 实证依据 | 阻塞关系 |
| --- | --- | --- | --- | --- | --- |
| **D-01** | 前端 `m02_viewcomponents.js` ← `m01_workbench.js` | 首次全量审查 P0-1 | `m02:19` 导入 `activeView`，`m01:126` 的 `let activeView` **没有 export** → ES module 链接期失败 | ✅ **已闭环**（`fc83e1e`）：m02 删掉用不到的 `activeView` import；`node --test front/tests/` 链接期无错 | ~~阻塞 D-02~~（已解除） |
| **D-02** | 前端 `app.js` ← `m03_viewstore.js` | 首次全量审查 P0-1 | `app.js:10` 导入 `mountViewStore`，但 `m03` 只导出 `setState/getState/snapshot/subscribe/reset` | ✅ **已闭环**（`fc83e1e`）：m03 补 `mountViewStore`，写法对齐四个 service | 已解除 |
| **D-03** | M-06 `hex.rs` | 首次全量审查 P0-2 | `:112` 错误格式化里 `bytes.len() - 5` 在 `bytes.len() == 4` 时整数下溢 panic | ✅ **已闭环**（v0.11，commit `34cc38b`）：改为饱和减法；**反向验证**——把修复改回去，`tests_malformed.rs` 的表驱动 + fuzz 用例双双 panic 失败 | 已解除；并补上 D-23 门禁第 2 项（畸形输入表驱动 + 固定种子 fuzz） |
| **D-04** | M-13 ISS | 首次全量审查 P0-3 | **x0 未硬连线为 0**：`executor.rs:97` 与 `mod.rs:161` 均裸写 `state.gpr[d.rd as usize] = result`，读侧 `state.gpr[d.rs1 as usize]` 无保护 | ✅ **已闭环**（v0.11，commit `71a3b05`）：引入 `read_gpr` / `write_gpr`，写侧 `rd == 0` 丢弃写入、读侧恒返回 0，**全模块无旁路**（T1 自查确认只有这两个函数碰寄存器文件） | 已解除；**Q7 Spike 对拍的前置条件已清**——工具链亦已装好（Spike 1.1.1-dev + xpack GCC 15.2.0），见 PROGRESS v0.11.0 |
| **D-05** | M-12 引擎 | 首次全量审查 | `snapshot()` 只返回 `SimState`，`restore()` **不还原 `now_ps`、不处理事件队列** | ✅ **已闭环**（v0.11，commit `86dfa4b`）：`state.virtual_time_ps` 成为时间唯一真相（契约里本就为这个留的字段），`EngineSnapshot` 在 m12 内部承载队列完整快照，新增 `snapshot_full` / `restore_full`；**旧签名原样保留**，`m17_backend` 零改动继续编译 | 已解除；D-23 门禁第 4 项（snapshot→restore→继续推进的确定性断言）同步补上 |

> **前端 D-01 / D-02 的关键实测**：在 `/tmp` 副本上**仅补这两处导出**（约 3 行），bootstrap 即完全跑通（最小 DOM stub 验证）——**这是前端启动路径上的全部阻断点**，修完没有隐藏的链接期错误。

---

## 三、跨模块高危（9 → **12** 项）

| ID | 归属模块 | 来源 | 问题 | 阻塞关系 / 备注 |
| --- | --- | --- | --- | --- |
| **D-06** | M-12 | 首次全量审查 + M-15 报告 #6 | `run_until_idle` 每个事件耗 1µs 量子；事件在 t=1e12 ps 即空转 1e9 次 | ✅ **已闭环**（v0.11，commit `1723610`）：改为「空闲区间批量推进」——时钟直接跳到下一个到期事件，迭代次数从 O(时间/量子) 降到 **O(事件数)**。性能基线测试断言 1e12 ps 跨度 64 事件 → **步数恰为 64**、步数 < 1e9、Tick 数 == 事件数 |
| **D-07** | M-12 | 首次全量审查 + M-15 报告 #6 | `dispatch` 的 `PeripheralTick` 分支是空 TODO | ◐ **2026-10-20 推进**：v0.11 在 m12 内留好**接缝**（`PeripheralTicker` 回调：`tick(paddr, now_ps) -> Result<Vec<PendingWrite>, PlatformError>`，语义对齐契约 `Peripheral::tick`，返回的写并入 `state.pending_writes`），`Engine` 泛型不变、`m17_backend` 不受影响。**接线 PR 已把 m14 侧准备好**：总线现在持有外设实例（`BoundPeripheral`），外设 `tick` 也改为记权威虚拟时间。**剩最后一步**：M-12 的 `dispatch` 真正调用它（Wave 2 单独做，两侧接口已就位，**接线 PR 的代码无需改动**） |
| **D-08** | M-06 `hex.rs` | 首次全量审查 | `finalise` 超 `segment_count` 时 `truncate()` **静默丢固件数据**且不报错 | ✅ **已闭环**（v0.11，commit `e7fb3e0`）：改 `Result` + 报错。⚠️ T3 复核后定性为「防御性收口」——当前控制流下 `truncate` 实际不可达（循环内长度检查先于 EOF 早退拦截），但路径已封死 |
| **D-09** | M-06 `elf.rs` | 首次全量审查 | `read_u16`/`read_u32` 为 `panic!`，而上方注释承诺「returns PlatformError(E_LDR-01)」 | ✅ **已闭环**（v0.11，commit `0258a34`）：改返回 `Result` 并与注释一致，调用点同步改为错误传播，移除 `.expect` |
| **D-10** | **M-06 + M-14 联合** | M-14 报告 #6 | `ParseLimits` 无 `total_image_bytes` / `max_region_bytes`；M-14 `prepare_bytes` 裸 `vec![0u8; size as usize]`，`size` 最大可达 ~4 GiB | ✅ **已闭环（两侧齐备）**：M-06 侧（v0.11 `374d647`）—— `ParseLimits` 新增 `total_image_bytes`（默认 64 MiB）/ `max_region_bytes`（默认 16 MiB）并在解析时校验；M-14 侧（v0.11 `60dbbb8`）—— `prepare_bytes` 引入**区域大小上限**，超出返 E_BUS-09，**不先分配后检查**，`map_ram` / `map_region` 两条分配路径均受保护。84 字节畸形 ELF 已无法申请 GiB 级内存 |
| **D-11** | M-13 ISS | 首次全量审查 | `BusExecutor` 复制 `Executor` 的 ADDI 逻辑，`bus` 字段挂 `#[allow(dead_code)]` 从未被读 | ✅ **已闭环**（v0.11，commit `e068295`）：改为 `BusMemory`（只做访存适配）+ 共享的 `MemoryAccess` 端口，**指令语义收敛到唯一一条 `execute_one()`**；两条路径的非法指令语义统一为 `Trapped { IllegalInstruction }`（回归测试断言两路一致）。LOAD/STORE 另有专门测试证明**必经 M-14 总线**（否则会静默丢掉 stats 与 M-18 hook） |
| **D-12** | M-14 ↔ M-15 ↔ M-18 接线 | M-14 报告 #13 / M-15 报告 #11 | `Bus::map_peripheral(base, size, PeripheralId)` 的 size 与 `Peripheral::size()` 无一致性校验 | ✅ **已闭环**（v0.11 接线 PR，3 个 commit）：① 总线经 `Rc<RefCell<dyn Peripheral>>` **共享句柄**持有外设实例（`BoundPeripheral` + `RegionTarget::PeripheralBound`），新增 `Bus::map_peripheral_instance`，挂载时校验 `size == periph.size()` 不一致返 **E_BUS-08**；② M-15 注册表改共享句柄（`handle()` / `share()`），三外设 override `on_probe` 上报 `OUT` / `TX` / `CMP` 信号；③ M-18 新增 `ProbeRegistry::attach_signal`（LLD §1.3 的 `attachSignal`）+ `CH_UART_TX = 4`。**N5「GPIO 翻转可见」的内核路径 100% 闭合**——`crates/kernel/tests/m14_m15_m19_wiring.rs` 13 个端到端用例为证（固件写 GPIO OUT → M-14 派发 → M-15 上报 → M-18 判沿 → M-19 波形通道出现期望边沿） |
| **D-13** | M-14 | M-14 报告 #9 | `Region.target` / `RamBackend.bytes` 仍为 `pub` | ⏳ **未动**：收紧可见性要同步改 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable`——后者与 v0.11 的 T4 前端服务层改动相邻，**留 Wave 2 一并做**避免来回冲突 |
| **D-14** | 契约 + M-14 | M-15 报告 N4 | `Peripheral::read_clear` 默认实现委托 `read`，**漏覆写的外设静默获得非破坏性读** | ✅ **已闭环**（v0.11 `22107a0`）：总线读外设**一律调 `read_clear`**（不是 `read`），`Uart16550` 正确 override（读 RX 后清空 FIFO 状态）。回归测试：连续读两次 UART RX，第二次不再拿到同一字节；M-14 侧另有 `read_clear` 路径计数用例证明确实走了破坏性读 |
| **D-24** | 契约 + M-12 + M-17 | 检视 V2 · F-1 | **`ExecutionBackend::restore` 不是确定性还原**：`SimState` 不含事件队列（契约无此空间），`Engine::restore` 只还原平台状态与虚拟时钟，队列原样保留 | ✅ **已闭环（收口轮，commit `3016de0`）——根因已除**。检视 V2 实证：`schedule A; snapshot; schedule B; restore; run` → **A 与 B 都触发**，而从快照时刻出发的运行只有 A。**处置**（收口轮 `3016de0` / 自证修复 `560226c` / 检视闭环 `3893626`）：① 契约层新增 `crates/contract/src/snapshot.rs`（`EngineSnapshotDto` / `QueuedEventDto` / `SchedEventDto`，全部可序列化 + `validate()`）；② `ExecutionBackend` 增 `snapshot_full` / `restore_full`，**additive 默认实现**，两端**对称地大声失败**（`E_SNAP-03`）——不用软性的「没有」以免调用方的 `if let Some(..)` 分支静默跳过还原；③ M-12 新增 `snapshot.rs` 做 `EngineSnapshot ↔ EngineSnapshotDto` 双向转换 + `Engine::snapshot_dto` / `restore_dto`（**先校验后应用**）；④ M-17 `SelfIssBackend` 接线完成，并删掉那个**写而不读**的 `state` 死字段。**partial 的 `snapshot` / `restore` 保持原样**（仍如实标注为非确定性），公开 API 只增不减。**证据**：`cargo test --workspace` 375 → **406**；D-24 原始复现有两条用例分别钉住 partial 路径的发散与 full 路径的不发散；收口轮的 5 项 + 检视后补的 8 项变异自检**全部变红**。⚠️ **意义必须限定**：该 DTO 只覆盖 **M-12/M-17 段（引擎）**——外设（M-15）/ 中断控制器（M-16）/ 内存映射**不在**其中，**落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 的 `Snapshot{cpMeta, m13State, m14Map, m15Periph}` 组装**（→ **D-36 / D-37**） |
| **D-36** | 契约 + M-15 + M-16 | 收口轮检视 V-A · H-1 | **快照 DTO 不含外设与中断控制器状态**：`EngineSnapshotDto` 只覆盖 M-12/M-17 段（队列 + 平台状态 + 插入序 + 虚拟时间）。UART 的 RX/TX FIFO 与 STATUS、定时器的 `mtime`/`mtimecmp`、GPIO 的方向/输出锁存、已拉起的 IRQ 行**全都不在快照里** | ⏳ **未做**：契约层 doc 已收窄措辞（「完整引擎级快照」而非「完整快照」），但**落盘完整会话仍缺 M-15 / M-16 的状态序列化**。M-20 LLD §2.1.1 的 `Snapshot` 结构里本来就有 `m15Periph` 一段，**按 LLD 走即可**，但 M-15 / M-16 至今没有 `loadPeriph` / 状态导出接口。**这是「持久化一个会话」的真正剩余工作量** |
| **D-37** | 契约 + M-20 + M-23 | 收口轮检视 V-C · H-1 | **契约 DTO 与 M-20/M-23 已冻结的 `Snapshot` 结构不一致，且 M-20/M-23 未接线**：M-20/M-23 LLD §2.1.1（评审 20-1 / 23-1）定义的是 `Snapshot{cpMeta, m13State, m14Map, m15Periph}`；契约 DTO 是 `{schema_version, state, queue, next_seq, instruction_budget}`。`SnapshotRecord` 至今**无 serde derive**，M-23 无法直接序列化它 ⏳ **已登记未做**（**矛盾已裁决**）：⚖️ **2026-10-20 协调者裁决——以详细设计 §2.1.1 为准**。LLD 总纲表12 / §2 持久化数据格式 / §4.6.4.1 / 表84 / 表85 曾把快照载荷写成「SimState + DebugMeta」，与 M-20/M-23 详细设计 §2.1.1（评审 20-1 / 23-1）的完整单对象冲突；**快照载荷 = `Snapshot{cpMeta, m13State, m14Map, m15Periph}`**，**`DebugMeta` 是项目级独立存储**（评审 23-8，键 `pdebug:`+projectId，不随快照回滚），**不属快照载荷**。**总纲侧已同步订正**（表12 新增 `Snapshot` 模型行 + `SimState` 用途收窄为「暂停态视图」·表13 补 `snapshotFull`/`restoreFull` 并把 `snapshot()/restore()` 订正为**非确定性**的 partial 语义 ·表84/表85 的 `saveSnapshot/loadSnapshot` 输入改为完整单对象）；M-23 详细设计 §1.1/§1.2/§2.1.1 的陈旧措辞同批订正。**但实现侧仍未动**：M-20 的 `SnapshotRecord` 仍吃 partial 的 `SimState`、M-23 的 `DataStore::put_snapshot` 仍以它为载体，M-15 / M-16 也还没有 `loadPeriph` / 状态导出接口 —— 开工 M-20 时须按 LLD 组装完整载荷 |

---

## 四、跨模块中低危（8 → **23** 项）

| ID | 归属模块 | 来源 | 问题 |
| --- | --- | --- | --- |
| **D-15** | M-06 `elf.rs` | 首次全量审查 | 缺 `p_filesz <= p_memsz` 校验；畸形 ELF 会建出超过声明内存的段 | ✅ **已闭环**（v0.11，commit `9a9d578`）：补校验 + 测试 |
| **D-16** | M-13 ISS | 首次全量审查 | `Decoded` 无指令长度字段，PC 恒 `+4` | ✅ **已闭环**（v0.11，commit `bdf5f1b`）：`Decoded` 增 `pub len: u8`（字节数），PC 按 `d.len` 步进。RV32C 本轮不做，但结构已能承接 |
| **D-17** | M-12 | 首次全量审查 | `schedule()` 把 seq 计数塞进 `state.event_queue_head`；`restore()` 后可能与队列中已有事件撞号 | ✅ **已闭环**（v0.11，commit `86dfa4b`）：seq 计数独立化，`event_queue_head` 的语义明确为「单调插入序计数器」；`restore` 时把它顶到队列已占用 seq 之上，恢复后不撞号 |
| **D-18** | 前端 `m02` | 首次全量审查 + 前端报告 #3 | ✅ **已闭环**（`37fe8da`）：去掉 `closest("[data-region-base]")` 前置条件（该按钮在 `<table>` 外）；连带修掉评审漏项——`seedDemoBus()` 此前**从未定义**，去掉前置条件后按钮会直接抛 `ReferenceError` |
| **D-19** | 前端 `m01` | 首次全量审查 + 前端报告 #7 | ✅ **已闭环**（`4738aa6`）：不再是 no-op——新增 `captureUiState()` / `restoreUiState()`，切走前采集、切回后还原 UI 交互态（LLD §3.3 要求的「按 viewId 存 UI 交互态」此前从未采集） |
| **D-20** | 前端 `m02` | 首次全量审查 + 前端报告 #4 | ✅ **已闭环**（`37fe8da`）：`onDelta` 去掉 `escapeHtml()`，字节同步进 M-03 store |
| **D-21** | M-06 `mod.rs` | 首次全量审查 | `detect_kind` 未知扩展名一律回落 `Elf`，拖入无扩展名 raw 文件报 "bad ELF magic" | ✅ **已闭环**（v0.11，commit `189f500`）：新增 `detect_kind_from_bytes` / `detect_kind_with_hint`，按 **magic bytes 嗅探**（ELF magic / IHEX 行首 / raw 兜底），扩展名只作辅助 |
| **D-22** | M-06 `mod.rs` | 首次全量审查 | `Loader::load_with_limits` 默认实现 `let _ = limits;` 静默忽略 limits；`parse_raw` 复用 `elf_size` 限制 raw 文件 | ✅ **已闭环**（v0.11，commit `189f500`）：`Loader` trait 的 `load_with_limits` 改为**必须实现**（全库仅 `DefaultLoader` 一个实现，无其它消费方），`ParseLimits` 逐项生效；HEX 路径改用独立 `hex_size` 限额 |
| **D-25** | M-15 | 检视 V2 · F-5 附带 | **UART 探针信号只实现了 `TX`**：M-15 LLD §外设能力表规定 UART 应上报「TX / RX 状态、CTS 电平」三路，实际只有 `"TX"`（字节流） | ⏳ **未做**：`"RX"` / CTS 两路没有实现，也**从未进过台账**。接真实内核后 UART RX 不会出现在波形里。随 M-15 下一波补 |
| **D-26** | M-16 | 检视 V2 · F-5 附带 | **M-18 的 ch=2（中断脉冲）无生产者**：M-16 仍是 44 行 stub（`m16_interrupt/mod.rs` 「Status: stub — port adapted; no CSR plumbing yet」） | ⏳ **未做**：M-16 LLD 规定 `sample_irq_pulse(now_ps, irq_line)` 直连 M-18，实现侧 `ProbeRegistry::sample_irq_pulse` 已就位但无人调用。中断脉冲通道在波形里永远不出现。随 M-16 开工一并做 |
| **D-27** | M-15 + M-18 | 检视 V2 · F-5 附带 | **M-18 的 ch=3（按键）无生产者**：M-15 根本没有按键外设（目录仅 gpio / uart / timer） | ⏳ **未做**：LLD 表里的 ch=3 是预留位。实现侧留空即可，**但 LLD 需注明「P1 预留、暂无生产者」**（本轮已随 F-5 的 LLD 对齐一并标注） |
| **D-28** | M-15 + M-18 | 检视 V2 · F-9 | **GPIO SingleBit 通道与 32-pin 位图错位**：`GpioBank::write` 把 32 位位图报给 SingleBit 通道，而 `ChannelDef::classify` 对 SingleBit 只看最低位 | ⏳ **未做**：多脚位场景下**只有 pin 0 的升降沿被识别**，其余 31 脚的状态变化被同一分类器吞掉。接线测试只验证了 pin 0。**这是 MVP 演示多脚位波形时的真实限制**（接线报告 §5.3 已自陈） |
| **D-29** | M-14 | 检视 V2 · F-4 | **E_BUS-05 把三种本应区分的子情况合并成同一 error code + scenario**：「外设主动抛错」/「外设实例忙（`try_write` 失败）」/「外设未接线（id-only 旧路径）」三者都是 family=Bus, scenario=5 | ⏳ **未做**：原 E_PER-02 之类的信息只剩在 `message` 文本里，前端要靠字符串匹配才能区分。**「实例忙」是 M-14 自身状态，不是外设主动抛错**，不该与外设错误共用。接线报告 §2.6 已诚实记录 |
| **D-30** | M-12 | 检视 V2 · §八 | **M-12 至今没有独立模块级 review** | ⏳ **未做**：v0.11 的 D-05 / D-06 / D-17 有加固与自测，但 M-12 **从未被独立检视过**——与 M-06 盲区同性质。M-12 是事件驱动引擎，其快照/时序语义是全平台的确定性基础，**风险高于其它模块** |
| **D-31** | M-18 | 检视 V2 · Batch B 报告 | **`make_bus_hook` 在构造时需 `reg.borrow()`**（取 `borrow_contended` 计数句柄），即「构建 hook 时注册表不可被借用」 | ⏳ **已记录**：与同模块 `wire_signal_probe` 的既有约束同构，不算缺陷，但确实是新增的**构造期前置条件**。彻底消除需把计数器改为构造时传入的独立 `Rc<Cell<u64>>`（**属公开 API 签名变更**，需决策） |
| **D-32** | 契约 + 前端 | 检视 V4 · F6 附带（前端服务层报告） | **契约缺口 7 项**：无寄存器快照事件 / 命令、无写寄存器命令、无 `negotiate`、`u64` 超 2⁵³ 精度、LLD 的 `E_PROTO` 族在契约 `ErrorFamily` 无对应、`crc` 未实现、无 `simStateDelta` 变体 | ⏳ **未做**：随 `schema_version=2` 一并处理（见 §6.2 优先级 5）。T5 另提「外设跨区间写需 `precheck` 端口」，一并归入 |
| **D-33** | 前端 M-01/02/03 | 检视 V4 · Batch C 报告 | **落地视图 `embedded` 没有 M-02 组件**：首次进入工作台只渲染出「嵌入式工作台」标题 + 空面板 | ⏳ **未做**：**非本轮引入**，此前无人用真实浏览器走过这条路径——`tools/browser-verify.mjs` 的「正文非空」断言一上来就把它照出来了（脚本把该检查改放有组件的 `#/regmem` 视图，并在前端服务层报告第七节记为第 7 条欠账）。要么补 `embedded` 的 M-02 组件，要么把它从落地视图列表里去掉 |
| **D-34** | 契约 | 检视 V4 · F11 | **命名歧义**：`model.rs` 同时存在 `Waveform`（history 形）与 `WaveformDelta`（live 形），而 `EventPayload::WaveformDelta` 包装的是 `Waveform` 而不是 `WaveformDelta` | ⏳ **已记录**：**设计问题，不是缺陷**——两条路径都有用（M-19 review 的 shape note 已注明）。但读 `model.rs` 的人会被名字误导。改名属契约层变更，留 `schema_version=2` 一并处理（与 D-32 同期） |
| **D-35** | 前端 mock | 检视 V4 · F13 | **mock 内核 PC 永远 +4，无 RV32C** | ⏳ **已记录**：压缩指令是 2 字节，mock 的单步 PC 永远按 4 步进。**仅影响 mock 训练出来的 UI**——接真实内核（C 扩展落地后）行为会突变。随 C 扩展一并处理 |
| **D-38** | 工具链 + M-12 | 收口轮检视 V-C · L-2 | **`tools/build-wasm.sh` 跑不通**：`wasm-pack build --target no-modules ... crates/kernel` 报 `Ensure that you have "wasm-bindgen" as a dependency in your Cargo.toml file`——kernel 没有 `wasm-bindgen` 依赖 | ⏳ **未做**：**pre-existing**（main 与 HEAD 上都失败，**非本分支引入**），但 **AGENTS.md「Setup commands」记载的 WASM 构建路径当前是坏的**，任何人照着做都会卡住。修法：给 kernel 加 `wasm-bindgen` 依赖（契约层 DTO 需 `#[wasm_bindgen]` 导出时才需要，本轮未做导出） |
| **D-39** | 契约 | 收口轮检视 V-A · 🟢-6 | **`validate()` 不检查「队列最早到期时刻 ≥ `state.virtual_time_ps`」**：一个 `at_ps` 早于当前虚拟时间的快照能通过校验 | ⏳ **未做**：引擎产出的快照永远满足该不变量，故当前是**防御性检查缺口**而非真缺陷。补法：`validate()` 增加一条检查（可复用 `E_SNAP_02` 队列畸形码，或新开场景码） |
| **D-41** | 契约 + M-13 | 收口轮检视 V-C · H-1 附带（pre-existing） | **契约层 `SimState` 缺「内存页」与「外设状态」两个字段**：LLD 总纲表12 把 `SimState` 的关键字段列为「PC、通用寄存器、CSR、**内存页**、**外设状态**、虚拟时间、事件序号」，而契约层现行的 `SimState` 只有 `gpr/pc/csrs/pending_writes/virtual_time_ps/event_queue_head/seed` | ⏳ **已登记未做**：**pre-existing**（不是本分支引入），但收口轮的裁决把问题**钉实**了——快照载荷已定为 `Snapshot{cpMeta, m13State, m14Map, m15Periph}`，其中 `m13State` 负责「指令级状态（寄存器/内存视图）」、`m14Map` 负责「总线/内存映射状态」、`m15Periph` 负责「外设状态」；**内存页与外设状态因此不进 `SimState`，而是被拆到了 M-13/M-14/M-15 三段里**（表12 已加注说明现行实现是设计意图的子集）。**这意味着「`SimState` 直接落盘即可还原会话」这条路被正式放弃**，M-20 必须按三段分别采集。待做：M-13 / M-14 / M-15 各自补状态导出与载入接口（`loadState` / `loadMap` / `loadPeriph`，LLD 评审 20-1 已列） |
| **D-40** | 契约 | 收口轮检视 V-C · L-3 | **`EngineSnapshotDto::default()` 是一个「合法但空」的快照**：`validate()` 对它返回 `Ok(())`，宿主若把 `default()` 落盘，得到的与「真实快照」在类型上不可区分 | ⏳ **未做**：契约层不提供 `Default` 是 breaking change，但可加 `is_empty()` 供宿主**显式拒绝**空快照。影响有限（宿主自造夹具时的常见误用） |

### 4.1 检视轮（2026-10-20）小结

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

## 五、工程门禁缺口（根因级）

| ID | 问题 | 说明 |
| --- | --- | --- |
| **D-23** | 三条门禁对上述 5 项阻断**全部无感知** | `cargo test` 只覆盖 Rust；D-01/D-02 是 ESM 链接期错误，只能在浏览器或 Node 暴露；D-03 是 `panic!` 而非 `Err`，测试没喂这个输入；D-04/D-05 现有用例恰好绕开了触发条件。**这是「47→70→74→203 全绿却仍白屏 + 可 panic」的根因**。✅ **2026-10-20 实质闭环**：4 类最小门禁**全部补齐**（第 1 项 v0.10，第 2~4 项 v0.11），且 3 条阻断级条目（D-03 / D-04 / D-05）本轮全部消除 |

**最小门禁清单（2026-10-20 全部就位）**：

| # | 门禁 | 覆盖 | 状态 |
| --- | --- | --- | --- |
| 1 | `tools/check-front.sh`（node:test 冒烟门禁 + CI job） | D-01 / D-02 及未来任何 ESM 链接回归；另覆盖前端全部 16 项评审意见 | ✅ **v0.10 完成**（`ed66337`）—— 复审 16 组变异测试，3 处真盲区已封、位置耦合已消除、DOM stub 5 处保真度缺口补齐；**v0.11 扩至 109 用例 / 19 suite**（新增 mock 内核链路、命令路由、会话生命周期、寄存器内存服务） |
| 2 | M-06 畸形输入表驱动用例 | D-03 / D-15 | ✅ **v0.11 完成**（`955055f`）—— 新建 `crates/kernel/src/m06_elf/tests_malformed.rs`：**20 条表驱动畸形用例 + 2 条固定种子 fuzz**，覆盖空文件 / 4 字节短记录 / 坏 magic / `filesz > memsz` / 超长段 / count 不符 / 截断 header / 非 UTF-8 / 整数溢出。**每个畸形输入必须 `Err`，不得 panic**；并做了**反向验证**（把 D-03 修复改回去，用例双双 panic 失败） |
| 3 | M-13 `addi x0, x0, 42` → `gpr[0] == 0` 断言 | D-04 | ✅ **v0.11 完成**（`71a3b05`）—— 另补「先写 x1 再 `addi x0, x1, 1`，x0 仍为 0」 |
| 4 | M-12「snapshot → restore → 继续推进」确定性断言 | D-05 | ✅ **v0.11 完成**（`50d7de8`）—— 断言 Tick 轨迹 / IRQ 轨迹 / 步数 / 量子数 / 虚拟时间 / SimState / 剩余队列逐项相等，另加「还原后 == 从未还原」的更强形式 |

---

## 六、建议处理顺序（含阻塞拓扑与投入产出比）

### 6.1 批次拓扑（2026-10-20 v0.11.0 后重排）

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
  D-07        → M-12 真正调用 Peripheral::tick（接缝已留，待 m14 持有外设注册表）
  D-13        → 字段可见性收紧（需同步改集成测试与前端 renderMapTable）
  D-24        → ✅ 已闭环（契约新增可序列化 DTO）
  D-30        → M-12 独立模块级 review
  M-13 M/C 扩展 → RV32IMC 全集补完
  Spike golden 对拍 → 工具链已就绪（Spike 1.1.1-dev + xpack GCC 15.2.0）
  契约缺口 7 项（D-32）→ 随 schema_version=2 一并处理
```

**关键路径变化（2026-10-20）**：**D-04 不再是瓶颈**——x0 已硬连线，Spike 工具链亦已装好并端到端验证通过（`spike --isa=rv32imc` 退出码 0）。**下一个瓶颈从「修欠账」转为「补能力」**：M-13 的 M/C 扩展、Spike golden 对拍、以及 D-07 的外设 tick 接线。

**v0.11 的方法论教训**：这批任务**全部无前置依赖、文件所有权零交集**，因此可以 5 路同时施工、实测 **0 merge 冲突**。下一批的并行切分应继续按「文件所有权」而非「模块边界」来切。

### 6.2 投入产出比排序（跨模块全局 · 2026-10-20 重排）

| 优先级 | 动作 | 成本 | 收益 |
| --- | --- | --- | --- |
| ~~1~~ | ~~修 D-01 + D-02（前端 2 处导出）~~ | ~~3 行~~ | ✅ v0.10 完成 |
| ~~2~~ | ~~加 CI 的 ESM 导入检查~~ | ~~1 行 CI~~ | ✅ v0.10 完成 |
| ~~3~~ | ~~修 D-04 + D-03 + D-05~~ | ~~各 1 处~~ | ✅ v0.11 完成 |
| ~~4~~ | ~~接通 M-14 → M-15 → M-18 三处断点~~ | ~~独立 PR~~ | ✅ v0.11 完成（N5 闭合） |
| ~~5~~ | ~~M-13 扩 RV32IMC 全集~~ | ~~12 人日~~ | ✅ v0.11 完成 **RV32I 41 条**（M/C 扩展待做） |
| **1** | **Spike golden 对拍**（工具链已就绪） | ~4d | **FR-04 必做项**；Q7 决议 |
| **2** | **M-13 M 扩展（8 条）+ C 扩展（16 条）** | ~5d | RV32IMC 全集补完 |
| **3** | **D-07 外设 tick 接线**（m12 接缝已留） | ~1d | 定时器时序推进 / FIFO 时间效应全链路生效 |
| **4** | **WASM 导出层 + 真实 worker 接入** | ~2d | 真实内核上浏览器（当前是 mock 内核） |
| **5** | **契约缺口 7 项** | ~2d | 与 D-32 同批（`schema_version=2`） |
| **5b** | **D-36 + D-37 + D-41：快照落盘对齐 LLD** | ~3d | 外设 / 中断控制器状态序列化（M-15 / M-16，D-36）+ M-20 按 LLD §2.1.1 组装完整 `Snapshot{cpMeta, m13State, m14Map, m15Periph}` + M-23 改吃该结构（D-37）+ M-13 / M-14 / M-15 补状态导出与载入接口（D-41）。⚖️ **LLD 矛盾已于 2026-10-20 裁决：以详细设计 §2.1.1 为准**（总纲侧已订正） |

---

## 七、整体判断

**做得好的部分**：4 个已开发模块（M-14/15/18/19）合计 9 831 行、194 个测试，模块内部质量普遍高于平均——M-19 的双源解耦、序列号 + DropRange 补洞、k-way 归并导出都是扎实的设计；M-15 的契约 additive 扩展策略（`read_clear` / `on_probe` 默认实现）让 M-0 冻结未被破坏。**这一轮的 review → 闭环 → 复查循环非常有效**，65 项闭环里有相当一部分是「以测试固化既有正确行为」而非改实现，说明代码作者的判断力是可靠的。

**v0.11 新增的好消息**：5 路并行交付证明**这个仓库的模块边界足够干净**——M-13 / M-12 / M-06 / 前端服务层 / 三处接线五组任务，文件所有权零交集，5 个独立 worktree 同时施工，最终 **0 merge 冲突、0 越界改动**（`crates/contract/**`、`lib.rs`、`m17_backend` 全部零改动）。子会话的自查还额外挖出 3 个真缺陷（S/B/J 立即数符号扩展位宽、WebSocket 握手前误报 connected、会话重连无限递归），其中后两个是**只有真实浏览器能暴露**的时序缺陷。

**结构性问题**：

1. **模块做完了，模块之间没接上**。M-14 ↔ M-15 ↔ M-18 三处断点，把 4 个高质量模块的实际价值压在了「总线 trace」这一条窄通路上。这是当前**投入产出比最高的修复方向**——不需要写新功能，只需要接线。
2. **门禁覆盖不到前端与畸形输入**。203 个 Rust 测试全绿的同时前端白屏、M-06 能被一个 4 字节畸形记录打崩。两条最小门禁（ESM 导入检查 + 畸形输入表驱动）能防住绝大部分这类问题。
3. **9 个模块仍是纯 stub**（645 行）——M-08/09/11/16/17/20/21/22/23，调试服务、DWARF、持久化、HMS 全部待开工。按 §3 关键路径，**M-13 扩 RV32IMC 全集（12 人日）是主线**，但它被 D-04（x0）挡在前面。

---

## 八、模块报告索引与模块内状态

| 模块 | 报告 | 状态 |
| --- | --- | --- |
| M-14 | `M-14_BusMemoryMap_Review.md` §四 | ✅ 模块内 100% 闭环；#6 上收为 D-10、#9 上收为 D-13（跨模块，均未动） |
| M-15 | `M-15_PeripheralModel_Review.md` §五 | ✅ 模块内 100% 闭环（N1/N2/N3 已落地）；#11 上收为 D-12、N4 上收为 D-14 |
| M-18 | `M-18_Probe_Review.md` §一·五 | 13/15 闭环；#10 契约 `Probe` trait 死契约（与 M-15 同源）、#12 `drain_waveform` 借用交错（均低危） |
| M-19 | `M-19_WaveformPipeline_Review.md` | ✅ 16/16 闭环（`cee6b61`），含 1 项阻断级（`Clone` + `capacity()` debug panic，M-18 同步修复） |
| 前端 M-01/02/03 | `FRONTEND_Review.md` | 🟢 2026-10-20 首次模块级 review → **同日全部闭环**（20/20，含 3 阻断与 4 项评审漏项）；新增前端门禁 38 用例 / 7 suite，`fc83e1e`~`6fc9e1f` + 复审闭环。⚠️ 其中 2 项漏项（#19 / #20）**只有真实浏览器能暴露**——修完白屏后视图体仍全空。**复审（§八）确认 16 项闭环无误，但门禁存在 3 处盲区**（#10 / #11 / #12 改回去仍绿）**+ 1 处位置耦合缺陷**（`app.js:81` 的 Promise.all 下标解构）——✅ **2026-10-20 复审闭环**：3 处盲区已封、位置耦合已消除、stub 5 处保真度缺口补齐（详见 §八·7） |
| M-13 | `M-13_ISS_Review.md` | ✅ **v0.11 首次模块级 review**（13 项问题全部闭环，含 1 项影响全部负立即数的真实缺陷：S/B/J 型立即数符号扩展位宽少算一位）；RV32I 41 条落地，M/C 扩展待做。**2026-10-20 检视轮**：初版**漏报** 1 项阻断级分类缺陷（SYSTEM 未校验 `funct3`，§一 #14），已由 `caf7f82` 修复；另有 3 项覆盖漏洞（`sequential` 步进 / 地址回绕 / 子字存储截断契约）补齐；§一 #6 的「契约要求非对齐 trap」已订正（契约层零处提及） |
| M-12 | **尚无模块报告** | ⚠️ v0.11 已完成 D-05 / D-06 / D-17 的**加固与自测**，但**未做独立模块级 review**（T2 的任务范围不含 review 文档）。M-12 从未被独立检视过——与 M-06 同性质的盲区，**已登记为 D-30，列为 Wave 2 待办** |
| **收口轮汇总** | `REVIEW_ROUND_2026-10-20_CONTRACT_DTO.md` | 📋 **收口轮 3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）的完整证据归档**：37 项发现逐条实证、3 张变异测试表、**逐条闭环对照表**、数字核对表、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
| **检视轮汇总** | `REVIEW_ROUND_2026-10-20.md` | 📋 **本轮 4 路独立检视的完整证据归档**（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）：22 项发现、逐项实证、变异测试结论、修复与反向验证记录、F-5 专项裁决依据、数字口径订正。**AGENTS.md 要求的「检视产物」** |
| M-06 | `M-06_ELFLoader_Review.md` | ✅ **v0.11 首次模块级 review**（17 项问题 / 12 项闭环 / 5 项留账并说明理由）——**该模块诞生以来第一次被系统检视，原是台账最大的盲区**。**2026-10-20 检视轮**：V3 判定 PASS（~26 000 次随机 / 边界输入 0 panic）；报告的 D-10「M-14 侧未做」状态已过时（`60dbbb8` 早已补齐），D-22 的保证性质已补注（契约性保证而非回归保护），#17 嗅探假阳性已由 `fd3d036` 修复 |
| 前端 M-04/05/07/10 | `FRONTEND_ServiceLayer_Review.md` | ✅ **v0.11 首次模块级 review**（服务层从空壳到真跑通；7 项契约缺口留 Wave 2） |

**下一步建议**：**D-04 / D-05 / M-06 盲区均已消除**。下一个盲区是 **M-11 符号表/DWARF**（10 人日，最大风险项，从未开工）与 **9 个纯 stub 模块**（M-08/09/11/16/17/20/21/22/23）。Spike 工具链已就绪，**M-13 阶段 4 golden 对拍可立即启动**。

---

## 九、状态变更记录

| 日期 | 变更 | 说明 |
| --- | --- | --- |
| 2026-10-19 | 台账建立 | 从首次全量审查（仅存在于对话、未归档）与 M-14 / M-15 两份模块报告中提取全部跨模块条目，逐条回溯源码核实当前状态 |
| 2026-10-19 | M-15 报告闭环 | 15 项完全闭环 / #6 半闭环（上收为 D-07）/ #11 按设计推迟（上收为 D-12）/ #18 有意保留 |
| 2026-10-19 | M-14 报告闭环 | 9 项完全闭环 / #14 半闭环 / 6 项推迟；#6 上收为 D-10、#9 上收为 D-13 |
| 2026-10-19 | **M-19 波形管道落地 + M-18 → M-19 握手完成** | 契约新增 `port::EdgeSink` / `port::DeltaSink` 驱动端口与 `model::EdgeEvent` / `WaveformDelta` / `DropRange` 三个 DTO（全部 additive）；M-18 `ProbeRegistry` 增可选项 `edge_sink`，原先丢弃的 `EdgeKind` 随 `EdgeEvent` 下发。**M-18 报告 §四.2「M-19 真正消费 M-18 边沿事件」这条待办闭合**。⚠️ **D-12 本体（M-14 ↔ M-15 size 一致性）未动**；M-14 ↔ M-18 的 bus hook 与 M-15 ↔ M-18 的 `on_probe` 接线也仍未做 |
| 2026-10-20 | **全模块闭环状态审计 + 前端首次模块级 review** | 新建 `FRONTEND_Review.md`（前端 16 项）。**M-19 报告 16/16 闭环**（`cee6b61`，含 1 项阻断级 `Clone`+`capacity()` debug panic，M-18 同源缺陷同步修复，测试 64→85）。**M-14 / M-15 模块内已 100% 闭环**。**M-18 剩 2 项低危**（契约 `Probe` 死 trait + `drain_waveform` 借用交错）。**前端首审 16 项全部未动，含 2 阻断**——实证确认修掉那 2 处 ESM 导出（约 3 行）后 bootstrap 即完全跑通，即前端启动路径上的全部阻断点。**5 条阻断级台账条目无一闭环**。基线升至 workspace 203 全绿 |
| 2026-10-20 | **D-12 状态更新为「部分推进」** | M-18 → M-19 已通（`set_edge_sink` + `EdgeSink` + 端到端集成测试）；M-14 → M-15 仍返 E_BUS-05；M-15 → M-18 三个外设均未 override `on_probe`。观测管道目前**只有「总线访存」一条通路**能到达 M-19，「外设信号」那条（智能灯 LED 引脚）仍断——**N5「GPIO 翻转可见」卡在接线而非内核** |
| 2026-10-20 | **前端评审 20/20 全部闭环** | `FRONTEND_Review.md` 16 项首审意见 + 2 项评审漏项全部闭环：D-01/D-02 白屏阻断（ESM 导出缺失）已修，bootstrap 实测跑通；视图层收尾（demo 按钮恒不触发 / 终端双层转义 / 活动标签从不高亮）已修；`switchView`/`setCapabilities` 状态同步与 LLD §3.3 的 UI 交互态采集已落地；escape 单点化、面板改用 DOM 构造、role→默认视图显式映射、demo 数据隔离到 `front/dev/` 已完成；状态栏/页脚文案改为从真实状态推导。**评审漏项 4 项**：`seedDemoBus()` 从未定义（去掉 #3 的前置条件后会抛 `ReferenceError`）、`refreshAll()` 会把当前面板藏掉（能力位一翻就白屏）、**`switchView` 同视图分支不 `showPanel()`（初始面板不显示）**、**首次 `riscv:render` 事件被丢弃（挂载顺序导致订阅晚于派发 → 视图体永远空白）**。后两项是在**真实浏览器**复验时才发现的——DOM stub 测试对它们完全隐形（测试都在 bootstrap 之后才路由）。**新增前端门禁**：`front/tests/` 38 用例 + `tools/check-front.sh` + `.github/workflows/ci.yml`（仓库此前无 CI），并以 10 组变异测试证明门禁有效 |
| 2026-10-20 | **v0.11.0 · 首次「多路并行开发」：5 路 worktree 隔离同时施工，全绿后统一收口** | 5 个任务拆成 5 个隔离 worktree + 5 个独立分支同时开工，**文件所有权零交集 → 0 merge 冲突、0 越界改动**（`crates/contract/**` / `lib.rs` / `m17_backend` 全部零改动）。**3 条阻断级全部消除**：D-03（hex 下溢 panic，附反向验证）、D-04（x0 硬连线）、D-05（快照还原 now_ps + 事件队列）。**高危 7/9 闭环**：D-06（推进从 O(时间/量子) 降到 O(事件数)）、D-08、D-09、D-10（M-06 侧）、D-11（执行路径收敛为唯一 `execute_one()`）、D-15、D-16、D-17、D-21、D-22。**D-23 门禁缺口 4 → 0**：M-06 畸形输入表驱动 + fuzz（20 + 2 条）、M-13 x0 断言、M-12 确定性断言全部补齐。**M-13 落地 RV32I 41 条**（LLD 48 条清单的 40 条 + FENCE.I）+ 模块级 review；**M-12 加固** + 模块级 review；**M-06 首次模块级 review**（17 项 / 12 闭环 / 5 留账）；**前端 M-04/05/07/10 从 66 行空壳到真跑通**（门禁 38 → 109 用例 / 19 suite，Chrome 真机复验）。**工具链落地**：本机 Homebrew 损坏（2023-12 版 + 阿里云镜像停更）→ 从 GitHub 直取 main 并切分支，tap `riscv-software-src/riscv` 装 **Spike 1.1.1-dev** + `riscv-pk`；GCC 走 **xpack riscv-none-elf-gcc 15.2.0**（darwin-arm64，rv32imc）。端到端验证 `spike --isa=rv32imc` 退出码 0。**协调者决策**：① 非对齐访存默认策略改回 LLD 口径 `Allow`（`Trap` 降为 opt-in 严格模式）——默认 `Trap` 会让 ISS 在任何触碰非对齐地址的程序上与 Spike 分叉；② 回写 M-13 LLD §3.3 的 LUI/AUIPC 执行列（§3.1 说 imm 已含左移，§3.3 写 `imm<<12` 是二次左移）；③ 删掉 workspace 里已无对象的 `non_canonical_partial_ord_impl` allowance（删后 clippy 仍干净）；④ `EngineSnapshot` 不可序列化 → M-20/M-23 落盘被卡，**契约新增 DTO vs kernel 引入 serde 留 Wave 2 决策**。**基线**：`cargo test --workspace` 203 → **314**（+111）；前端门禁 38 → **109 / 19 suite**（+71） |
| 2026-10-20 | **并入全模块状态，删除独立 `STATUS.md`** | 原 `STATUS.md`（全模块快照）内容并入本台账 §一（模块级闭环总览）、§1.2（跨模块链路连通性）、§六（投入产出比排序）、§七（整体判断）。**理由**：避免状态分散在 4 处（4 份模块报告闭环章节 × 4 + 台账 + STATUS + PROGRESS）产生漂移。本台账定位升级为**全部 review 欠账的唯一追踪入口**；模块报告只保留该模块的细节分析 |
| 2026-10-20 | **前端分支独立复审（`fix/frontend-review-closure`，11 commit）** | 结论见 `FRONTEND_Review.md` §八。**首审 16 项 + 4 项评审漏项确认真实闭环**（16 组变异测试逐项验证，11 组被门禁抓住）。但复审另查出 **3 处门禁盲区**（#10 createPanel 改回 innerHTML、#11 状态栏改回硬编码、#12 页脚改回硬编码 —— 改回去门禁仍 21 pass / 0 fail）+ **1 处位置耦合缺陷**（`app.js:81` 的 `Promise.all` 数组 7 元素 vs 解构 6 槽，插入一个 mount 即静默错绑 `transport`，门禁抓不住，用户已见过页脚 `role=?` 症状）+ **DOM stub 5 处保真度缺口**（`^=`/`$=` 选择器算符未实现退化为「属性存在即匹配」；无滚动模型使「终端滚动还原」用例成恒等式；`once`/`stopPropagation` 未实现；`document.addEventListener` 与 `window.location` 缺失）+ 文档用例数 4 处不一致。**SAB / COOP-DEP 语义校正经抽检准确**（COOP/COEP 是服务器响应头，SAB 的真正条件是跨源隔离）。基线：`cargo test --workspace` 203 ✓ / `check-front.sh` **35 passed / 7 suite** ✓ / CI YAML 合法且 exec 位已入库 ✓。✅ **复审 15 项发现全部闭环**（见 §八·7）：3 处盲区已封、位置耦合已消除、stub 5 处保真度缺口补齐 |
| 2026-10-20 | **前端复审闭环二次复验**（`445c863`/`b3045b7`/`32b3387`/`2390bb6`） | 结论见 `FRONTEND_Review.md` §八·八。**§八·7 的 15 项闭环声明逐条独立验证，15/15 属实**，无「声称已修但没修」项；11 组声称变异全部复现变红，**另 5 组自建探针**（transport↔workspace 值互换、`describeTransport` 常量化）也被新断言抓住——覆盖强度强于声明。**但本轮改动自身引入 5 项新问题**：R-15 stub 的 `DOMContentLoaded` 在动态 import 完成**之前**派发（实测「import 后才注册监听」→ 0 次，真实浏览器 1 次）→ 前端若监听该事件会「浏览器跑、门禁不跑」**假绿**，而 R-12 自测只断言函数存在、不断言事件派发；R-16 `document` 的 capture 监听器永不触发；R-17 新增的 `!/innerHTML/.test(m01源码)` 静态断言**过宽**（加一句含该词的注释即 1 fail 假红）；R-18 门禁强依赖 dev-only 全局 `window.riscv_demo`（删掉 `exposeDevHelpers` → 3 fail 假红）；R-19 stub `removeEventListener` 移除的是 `capture=true` 条目（DOM 规范要求移除 `capture=false`）。另 2 处文档残留：`PROGRESS.md:67` 未提 `package.json` 已移到 `front/`、`FRONTEND_Review.md:14` 的「21 组变异」应为 27 组。**`front/` 与 `crates/` 零改动**。✅ **2026-10-20 二次复验闭环**：作者侧确认 §八·7 的 15 项声明 **15/15 属实**，并修掉本轮改动自身引入的 5 项新问题（R-15 `DOMContentLoaded` 派发时机造成假绿 / R-16 document 缺捕获相位 / R-17 静态断言过宽致假红 / R-18 门禁依赖 dev-only 全局 / R-19 removeEventListener 移除错相位），门禁扩至 **38 用例 / 7 suite**，三轮累计 32 组变异中 13 组必须变红者全部被抓住、3 组预期变绿者各有理由 |
| 2026-10-20 | **独立检视轮：4 路并行检视 → 逐项修复（v0.11.1）** | Wave 1 交付后**先检视、后合 main**，4 路独立检视（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）共查出 **22 项**。**1 阻断级**：M-13 SYSTEM opcode 未校验 `funct3` → CSR 形态编码（`0x00001073`）被误判为 ECALL，mcause 报 11 而非 2（`caf7f82` 修复 + 3 条用例，**变异自检：改回原样 4 条用例变红**）。**2 高危**：总线 hook 用裸 `borrow_mut()` → 持锁时重入 panic（`bd647dd` 修复 + 3 条用例）；`ExecutionBackend::restore` 的「确定性快照」承诺与实现不符（`7254a00` 订正文档，根因转 **D-24**）。**中低危已修**：M-06 IHEX 嗅探改结构判据（`0x3A` 开头的 raw 二进制不再误判）、bss gap 基址 `checked_add`、M-14 外设路径区域上限覆盖、M-13 的 `sequential` 步进 / 地址回绕 / 子字存储截断契约 3 处覆盖漏洞、LLD 探针信号名对齐实现口径（3 份 HTML，零代码风险）。**新登记 D-24~D-35 共 12 条**。**数字失真已订正**（V4 发现）：台账 / PROGRESS / AGENTS 的代码行数与测试数此前低估 3~4 倍，现按「模块目录全部 .rs 行数 / `#[test]` 条数」统一重算并写明口径。**基线**：`cargo test --workspace` 314 → **375**（lib 352 + 集成 23）；前端门禁 109 → **119 / 21 suite** |
| 2026-10-20 | **收口轮：契约新增可序列化快照 DTO —— D-24 根因闭环** | 用户决策「契约新增可序列化 DTO」后由协调者直接实现（契约层变更不走子会话）。**根因回顾**：D-24 的根因不是「契约没空间」，而是**两头堵死**——M-12 的 `EngineSnapshot` 含事件队列但不可序列化（kernel crate 刻意无 serde），契约层的 `SimState` 又装不下队列本身，于是「确定性的快照存在，却落不了盘」，M-20 / M-23 的持久化被卡死。**处置**（commit `3016de0`）：① 契约新增 `crates/contract/src/snapshot.rs`（`EngineSnapshotDto` / `QueuedEventDto` / `SchedEventDto` + `validate()`，检查队列规范序 / seq 唯一 / next_seq 不撞号）；② `ExecutionBackend` 增 `snapshot_full` / `restore_full`，采用**本仓库既有的 additive 默认实现模式**（M-15 review #2）——不支持完整快照的后端 `snapshot_full` 返 `None`、`restore_full` **大声报错** `E_SNAP-03` 而非静默跳过；③ M-12 新增 `snapshot.rs` 做 `EngineSnapshot ↔ EngineSnapshotDto` 双向转换 + `Engine::snapshot_dto` / `restore_dto`；④ M-17 `SelfIssBackend` 接线；⑤ `Engine` 增 `intc()` 只读访问器、`SelfIssBackend` 增 `engine()` / `engine_mut()`。**公开 API 只增不减**，partial 的 `snapshot` / `restore` 保持原样并继续如实标注为非确定性。**证据**：`cargo test --workspace` 375 → **392**（契约 lib 8 + kernel lib 354 + 集成 30）；**5 项变异自检全部变红**（M-1 还原不恢复队列 / M-2 DTO 丢队列 / M-3 默认 restore_full 静默成功 / M-4 M-17 谎称不支持 / M-5 validate 去 seq 查重）。**台账数字同步重算**：M-12 1 060 → **1 292**（测 18 → 20）· 契约层 1 163 → **1 544**（测 0 → 8）· stub 组 591 → **633** · 集成测试 23 → **30** |
| 2026-10-20 | **⚖️ LLD 内部矛盾裁决：以详细设计 §2.1.1 为准** | 收口轮检视 V-C 揭出 LLD 自身自相矛盾：**总纲表12 / §2 持久化数据格式 / §4.6.4.1 / 表84 / 表85** 说快照载荷是「`SimState + DebugMeta`」，而 **M-20 / M-23 详细设计 §2.1.1**（评审 20-1 / 23-1）说快照载荷是「完整单对象 `Snapshot{cpMeta, m13State, m14Map, m15Periph}`」。**协调者裁决：以详细设计 §2.1.1 为准**（依据：详细设计是 M-20↔M-23 双侧对齐冻结的既有契约资产，且评审 23-8 已明确 `DebugMeta` 是**项目级独立存储**、不随快照回滚——「SimState + DebugMeta」那个写法既装不下内存与外设状态，也把 `DebugMeta` 错置进了快照载荷）。**同步订正**：① **LLD 总纲**——表12 新增 `Snapshot` 模型行、`SimState` 关键字段加注「现行实现为子集（→ D-41）」、用途收窄为「暂停态视图（FR-09）」；表13 补 `snapshotFull()/restoreFull()` 两行（V-C H-3 的总纲部分，此前只补了 M-17 详细设计）、并把 `snapshot()/restore()` 的「全状态快照/恢复」订正为**非确定性**的 partial 语义；表84 / 表85 的 `saveSnapshot/loadSnapshot` 输入改为完整单对象；② **M-23 详细设计** §1.1 / §1.2 / §2.1.1 的三处陈旧措辞；③ **M-20 详细设计** §2.1.1 加 v1.3 裁决段（声明本节为权威定义方）；④ **契约层 `snapshot.rs` 模块 doc** 新增「Where the persisted-session format lives」段，写明 DTO 是 M-12/M-17 段的承载类型、完整 `Snapshot` 由 M-20 组装。**裁决后剩余工作**：D-36（外设 / INTC 状态不在 DTO）+ D-37（M-20/M-23 未接线）+ **D-41**（新登记：契约层 `SimState` 缺「内存页 / 外设状态」字段——裁决把这两个字段正式划归 M-13 / M-14 / M-15 三段，「`SimState` 直接落盘即可还原会话」这条路正式放弃） |
| 2026-10-20 | **收口轮检视闭环：3 路独立检视（V-A 契约 / V-B 内核 / V-C 跨模块）→ 逐项修复** | 分支合 main 前**先检视、后合**（AGENTS.md 工作循环）。3 路检视员互不可见，各自独立跑门禁 + 变异测试，**判定均为 FAIL**，共查出 **37 项**（去重后）：**0 阻断 + 5 高危 + 32 中低危**（V-B H-1 与 V-C H-2 是同一条缺陷，两路独立发现）。**34 项闭环 / 3 项留账**（D-38 / D-39 / D-40），另有 2 条**已闭环但带残留工作量**的条目 → **D-36 / D-37**。**闭环要点**：① **高危**——契约层 doc 做了它兑现不了的承诺（「完整 / 逐位还原 / 确定性往返」，而外设与中断控制器状态根本不在 DTO 里）→ 措辞收窄 + 新登记 **D-36**；`validate()` 未接入任何生产还原路径 → `restore_full` 改为**先校验后应用**（畸形快照被拒且引擎原样不动）；② **覆盖漏洞 8 处全部补上**——独立检视实测出「改回去 0 条用例变红」的盲区（还原丢 `instruction_budget` / 丢 `pending_writes` / 忽略 `next_seq` / 落盘 JSON 形状无测试钉住 / 规范序 `seq` 平局无覆盖 / 插入序镜像不一致无检查 / 内层 `SimState` 版本无检查 / `restore_full` 不校验），逐个补用例后**8 项变异自检全部变红**；③ **API 收敛**——`snapshot_full` 与 `restore_full` 改为**两端对称地大声失败**（`E_SNAP-03`），杜绝 `if let Some(..)` 的静默跳过路径；删掉 `SelfIssBackend` 的**写而不读**死字段 `state`；删掉全仓零使用的 `Engine::bus()`；④ **三份 LLD 同步更新**（M-17 端口表 / M-20 §2.1.1 / M-23 §2.1.1）——AGENTS.md 要求「涉及 ADR、模块边界需先更新设计文档」；⑤ **「M-20/M-23 持久化不再被卡」的过度声明已收窄**——DTO 只覆盖 M-12/M-17 段，完整会话仍需 M-20 按 LLD 组装（→ **D-37**）。**基线**：`cargo test --workspace` 392 → **406**（契约 lib 12 + kernel lib 356 + 集成 38）；台账数字再算一遍：M-12 → **1 373**（测 22）· 契约层 → **1 851**（测 12）· stub 组 → **645** · 集成 → **38**；台账 §1.1 合计行 → **172 / 143 / 29**（新增「收口轮新增」行 37 / 34 / 3）；另订正 6 处**既有**失真数字（§1.1 合计行的「v0.11 新增报告」行 50/40/4 → **23/21/2** 使行和自洽 · 前端拆分 views 1 045→**1 042** / shell 549→**552** / services 1 213→**1 241** / dev 593→**565** · §1.2 用例数 11→**13** · §七 4 模块合计 8 575/170 → **9 831/194** · stub 组 591 → **645** · §4.1 处置列按子项列举的口径说明） |
