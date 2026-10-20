# 检视与欠账总纲 · Reviews & Debt Ledger

> **定位**：本文件是 **`docs/reviews/` 的唯一入口**，也是**全部 review 欠账的唯一追踪入口**——跨模块条目与模块内条目都在这里汇总状态。
> 模块问题的**详细分析、实证记录与修复建议**见各模块报告（§4 索引）；**每次改动的来龙去脉、决策与工作量**见 `docs/progress/PROGRESS.md`。
> **建立日期**：2026-10-19（台账）· 2026-10-20（并入两轮检视汇总）
> **状态核实日期**：2026-10-21（D-46 探针管道闭环后重新逐条回源码核对，非引用历史结论）
> **基线**：`cargo test --workspace` **417 passed / 0 failed**（契约 lib 12 + kernel lib 356 + 集成 38 + golden runner 11）；`clippy -D warnings` 干净（已删 `non_canonical_partial_ord_impl` allowance）；`fmt --check` 干净；前端门禁 **119 用例 / 21 suite**
> **关键结论**：v0.10 记录的「三条现有门禁对本台账全部 5 项阻断级条目均无感知」**已不成立**——本轮补上了 3 类最小门禁（M-13 x0 / M-12 确定性 / M-06 畸形输入），**3 条阻断级条目全部消除**。
> **独立检视轮（2026-10-20，v0.11.1）**：4 路独立检视（V1 M-13 / V2 M-12+接线 / V3 M-06 / V4 前端+收口）共查出 **22 项**：1 阻断级（M-13 SYSTEM 未校验 funct3，已修）+ 2 高危（F-3 总线 hook panic，已修；F-1 快照语义承诺，已订正文档）+ 若干中低危。**该轮阻断级与高危已全部闭环**，中低危分流为 D-24~D-35。详见 §3.2。
> **收口轮（2026-10-20，分支合 main 前）**：3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）共查出 **37 项**（去重后）：**0 阻断 + 5 高危 + 32 中低危**。**三路判定均为 FAIL**，逐项修复后 **34 项闭环 / 3 项留账**（D-38 / D-39 / D-40）。⚠️ 另有 2 条**已闭环但带残留工作量**的条目——V-A 🟠-1 与 V-C H-1——其剩余部分登记为 **D-36 / D-37**（**高危，未做**）。详见 §3.1。
> **D-24 已闭环（2026-10-20 收口轮）**：契约层新增可序列化快照 DTO（`crates/contract/src/snapshot.rs`），`ExecutionBackend` 增 `snapshot_full` / `restore_full`（additive 默认实现），M-12 ↔ DTO 双向转换落地，M-17 接线完成。⚠️ **该 DTO 只覆盖 M-12/M-17 段（引擎）**，不含外设（M-15）/ 中断控制器（M-16）/ 内存映射——**落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 组装**，见 **D-36 / D-37**。
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
| **M-14 总线** | 1 361 → **2 375** | **42** | 16 + 接线 PR + D-46 检视轮 | **16** | 0 |
| **M-15 外设** | 1 424 → 1 748 | **35** | 19 + N1~N4 = 23 | **23** | 0 |
| **M-18 探针** | 1 893 → **2 319** | **36** | 15 + D-46 检视轮 | **13** | 2（低危） |
| **M-19 波形** | 3 897 | 85 | 16 | **16** | 0 |
| **前端 M-01/02/03** | 1 123 → **1 702** | **21** | 20（16 首审 + 4 漏项） | **20** | 0 |
| **M-06 加载器** | 1 189 → 2 652 | **52** | **17（v0.11 首次模块级 review）** | **13** | 4（留账并说明理由） |
| **M-12 引擎** | 226 → **1 419** | **22** | 台账 3 项（D-05/06/17） | **3** | 0（**但无独立模块级 review** → D-30） |
| **契约层** | 1 163 → **1 877** | **12** | 检视轮 D-24 根因修复 | **1** | 0 |
| **M-13 ISS** | 456 → **2 989** | **88** | 14（v0.11 首次模块级 review + 检视轮补 #14） | **14** | 0 |
| **前端 M-04/05/07/10** | 66 → **2 701** | **71** | 7 项契约缺口 + 3 自查缺陷 | 3（已修） | 7（契约缺口 → D-32，留 Wave 2） |
| **M-08/09/11/16/17/20/21/22/23** | 591 → **2 124** | **41** | 无（→ **D-45** 已闭环） | — | — |
| **内核宿主（装配层 · 非 23 模块之一）** | **1 677** | **34** | 第 1 阶段新增（→ **D-46** 已闭环）+ D-46 检视轮 | — | — |

> **模块状态速查**（每模块一行 ✅/🟡/⚪ + 一句话说明）见 `AGENTS.md` 的模块状态表——**本表只留台账特有的行数 / 测试数 / 闭环数**。
> **统计口径**（2026-10-20 检视轮重算，V4 指出此前数字失真；**第 1 阶段后重算**）：「代码」= 该模块目录下**全部 `.rs` 文件的总行数**（含测试文件）；「测试」= 该模块目录下 `#[test]` 的**用例条数**。前端：「代码」= `front/views`（1 045）+ `front/app.js` / `index.html` / `style.css`（657）= 1 702；M-04/05/07/10 = `front/services`（1 285）+ `front/lib`（414）+ `front/dev`（671）+ `front/kernel-worker.js`（331，WASM 传输宿主，属 M-07 族）= 2 701。集成测试另计：M-06 5 + 接线 13 + M-18→M-19 5 + 快照 DTO 15 + **D-46 探针管道 15** = **53 条**。前端门禁单列：**152 用例 / 28 suite**。

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
| **D-46 检视轮新增** | **21 项发现**（V-A 9 + V-C 10 + V-B 2） | **18** | 3（登记为 **D-50 / D-51 / D-52**，其中 D-50 / D-51 是「已闭环但带残留」的派生欠账） |
| **合计** | **208** | **173** | **35** |

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
>           ──EdgeSink──▶ M-19 管道 ──drain──▶ 契约 WaveformDelta ──▶ 前端波形视图
> ```
>
> 证据：`crates/kernel/tests/d46_probe_pipeline.rs` **11 条** + 前端 `front/tests/waveform.test.mjs` **9 条** + `tools/wasm-smoke.mjs` 对**真 wasm 产物**的 5 项断言 + `tools/browser-verify.mjs` 在**真 Chrome** 里的 2 项断言。

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

> **2026-10-20 实测：RV32I 语料 42/42 全通**，M 扩展 8 例 + C 扩展 1 例为 `pending`（M-13 未实现，见 **D-43**）。
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
| **D-45** | 前端 M-10 + `app.js` ↔ 内核宿主 | 第 1 阶段真机复验 | **装载工程后内核侧订阅静默失效，寄存器推送停掉、视图停在旧值、不报任何错**：`LoadProject` 清空内核侧的主题集合（新工程 = 新会话，FR-13），而宿主本地的「已订阅」标志还立着 → 幂等的 `ensureSubscribed()` 什么都不发 | ✅ **已闭环**（第 1 阶段）：① M-10 新增 `resubscribe()`（**强制**重发订阅，无视本地标志）与 `loadProject(project)`——**「装载 + 强制重新订阅」绑成一个动作**，不变量的归属落在服务而不是调用点的注释里；② `app.js` 的启动流程与「reset」动作都改走 `registerMemory.loadProject(...)`；③ 前端门禁加 2 条回归用例（`regmem.test.mjs`），**反向验证**：把 `resubscribe()` 改成空操作 → 2 条变红；把 `loadProject()` 的重新订阅删掉 → 1 条变红。**这个缺陷在 mock 内核时代就存在**（mock 的 `reset()` 同样清主题），是第 1 阶段接上**真内核** + 真机断言「x1 必须是 42」才暴露的——**PC 会前进、x1 不动**。⚠️ 教训：真机断言必须断言**值**，只断言「内核已就绪」抓不到这一类 |
| **D-46** | M-18 / M-19 ↔ M-14 ↔ 前端 | 第 1 阶段已知限制 | **波形视图在第 1 阶段是空的**：真实探针管道（M-18 `ProbeRegistry` + M-19 `WaveformPipeline`）**没有接到总线的 bus hook 上**，所以 `sim.waveform` 订阅下不产生任何探针流 | ✅ **已闭环**（`feat/d46-probe-pipeline`）。**四根线一次接上**：① **M-14 加 `add_hook`（追加式）**——`set_hook` 保留「替换全部」语义，`add_hook` 是它的 additive 孪生；M-17 的观察点 hook 改用 `add_hook`，**后装的探针 hook 不再把观察点 hook 顶掉**（这是接线的前提：两个消费者都得看见每一次访存）；② **M-14 增 `peripheral_instances()`**——装配层据此给每个外设实例挂上 M-18 的探针回调（`attach_signal`）；③ **M-17 增 `Bus::tick_peripherals(now_ps)`** 并在**每条指令**前同步一次外设时钟——M-15 的 GPIO 在 `tick` 里记权威虚拟时间，边沿才带得上**真实时间戳**（否则时间轴恒为 0，等于半个假象）；④ **内核宿主装管道**——`ProbeRegistry`（判沿 + 访存 trace 环）→ `EdgeSink` → `WaveformPipeline`（环形缓冲 + 导出）→ 契约 `EventPayload::WaveformDelta`。⚠️ **契约事件取自 M-18 自己的边沿存储**；M-19 的环**只写不读**（CSV/VCD 导出还没接进契约 → 台账 **D-50**）；宿主默认内存图**新增 GPIO 外设**（`0x4000_0000`，PL061 惯例）——**没有外设就没有信号可观察**。**前端**：订阅内核侧 `sim.waveform`（**强制**重订，`LoadProject` 会清主题集合，与 D-45 同一个坑）→ 落进 M-03 视图状态 → M-02 波形视图渲染通道列表；过滤判别式是「**不是** `cpu.` 开头的寄存器流」，**不是**「是 `probe.` 前缀」——真内核的通道 id 是 `"{外设 id}.{信号名}"`（`GPIO0.OUT`），写成只认 `probe.` 会把真通道**全部过滤掉**；边沿计数**累加**（内核每次只推增量，覆盖的话视图上永远只有最后一批）。**demo 固件真的驱动 GPIO**（0x14..0x1c：`lui x5, 0x40000` / `addi x6, x0, 1` / `sw x6, 4(x5)`）——否则管道通了也没有边沿可推。**证据**：`crates/kernel/tests/d46_probe_pipeline.rs` **10 条**端到端用例（通道列表 / 真实边沿 / 时间戳 / `LoadProject` 后管道仍在 / 观察点与探针 hook **共存** / 两份 demo 固件防漂移）；前端 `front/tests/waveform.test.mjs` **9 条**；`tools/wasm-smoke.mjs` 对**真 wasm 产物**断言探针流（**+5 项**：通道列表 / 刚订阅时无边沿 / 真实边沿 / 边沿值 / 边沿时间戳 ≠ 0）；`tools/browser-verify.mjs` 在**真 Chrome**里断言波形视图列出 `GPIO0.OUT` **且带边沿数**（不是「非空」）。**检视轮（2026-10-21，`feat/d46-probe-pipeline`）**：三路独立检视（V-A 内核 / V-B 前端与工具 / V-C 一致性与文档）共查出 **19 项**（V-A 9 + V-C 10）：**4 中危 + 15 低危**，V-A / V-C 判 FAIL。**全部已在本分支闭环**（其中 2 项在新一轮检视开始前就被 `d0d0b66` 修掉了），其中 **V-A 🟠-1（发布源无上限 + M-19 死代码）** 是本轮最重要的一条：见 §3.5 |
| **D-47** | 前端 M-07 ↔ 内核宿主 | 第 1 阶段检视 V-B · 中危 1 | **传输帧 `seq` 有两个序列空间**：内核事件帧用 `msg.seq`（内核自己的单调计数器），宿主合成的错误帧用 `hostSeq`（另一个从 1 起的计数器）——同一 Worker 内帧 seq 可以从 11 跳回 1 | ⏳ **未做**（**P1 前必须拆开**）：今天**没有活影响**——M-05 用的是契约 `Message.seq`（`m05_commandrouter.js:162`），不是帧 seq；M-07 的 `resumeBySeq` 尚未实现。但 M-07 LLD 定义帧 `seqNo` 是「发送序列号（**单调递增**，断线续拉锚点；缺口 → E_PROTO-05）」，mock 内核用的是**单一**计数器（帧 seq 与 `Message.seq` 同源），两套实现口径不一致。做 P1 续拉 / 对账之前必须统一：**要么**宿主层维护独立的传输层单调 seq（帧 seq 与 `Message.seq` 解耦），**要么**在桥规格 §4 补一条「宿主合成帧的 seq 规则」并让实现对齐（⚠️ 直接让宿主帧接着内核 seq 排会**撞号**——内核的下一个事件仍会用自己的计数器） |
| **D-48** | 前端 M-07 ↔ WASM 宿主 | 第 1 阶段检视 V-C · 中危 4 | **`E_TRX-04` 一个码被三种语义复用**：M-07 里表示「传输通道错误」（Worker `error` 事件 / WebSocket 错误），`front/kernel-worker.js` 里表示「WASM 宿主侧错误」（init 失败 / 内核抛错 / 返回值非法）——前端无法据码区分「通道挂了」与「内核算错了」 | ⏳ **未做**（已在本轮把口径写进桥规格 §4.1）：本阶段**功能上无害**——两条路径的处置不同（前者把传输置为 `closed`，后者只拒掉单条命令）。但 P1 做断线续拉 / 错误分类时应当**拆码**（传输通道错误另给一个码，如 `E_TRX-05`），否则前端无法按码分流 |
| **D-49** | 契约层（`ErrorFamily`） | 第 1 阶段检视 V-C · 高危 2 | **`E_PROTO-08` / `E_PROTO-01` 不属于契约的任何一个 `ErrorFamily` 族**：契约只有 `CTX / ISS / ENG / BUS / PER / INT / DBG / OBS / SNAP / LDR / HMS / STORE / TRX` **13 个族，没有 `PROTO`**。这两个码是实现侧硬编码的字面量（`kernel_host` + `front/lib/message.js`），以字符串形式出现在 `EventPayload::Error.code`（该字段是 `String`，无族约束） | ⏳ **未做**（已在本轮把口径写进桥规格 §2.1）：**契约层冻结**，本轮**不新增** `ErrorFamily::Proto`——那是契约变更，需要另起流程并解冻 `crates/contract/src/error.rs`。⚠️ 连带影响：`port.rs` / `model.rs` 注释里引用的 `E_PROTO-05`（M-07 `resumeBySeq`）同样**查无此族**。P1 做断线续拉时，要么在契约变更流程里正式加 `Proto` 族，要么把 `E_PROTO-*` 全部并入既有的 `Transport` 族 |

---

### 1.3 跨模块中低危

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
| **D-38** | 工具链 + M-12 | 收口轮检视 V-C · L-2 | **`tools/build-wasm.sh` 跑不通**：`wasm-pack build --target no-modules ... crates/kernel` 报 `Ensure that you have "wasm-bindgen" as a dependency in your Cargo.toml file`——kernel 没有 `wasm-bindgen` 依赖 | ✅ **已闭环（2026-10-20 第 0 步，`build(wasm)`）**：① kernel 增 `wasm-bindgen` 依赖，**target-specific**（`[target.'cfg(target_arch = "wasm32")'.dependencies]`）——**native 依赖树实测 0 命中**，原生构建的依赖面与体积不受影响；② 新增 `crates/kernel/src/wasm_api.rs`（`#[cfg(target_arch = "wasm32")]`）导出 `kernel_version()` / `protocol_schema_version()` / `ping()`；③ **修掉脚本的输出路径 bug**——`wasm-pack build <crate-dir>` 以 **crate 目录**为工作目录，相对 `--out-dir` 会落进 `crates/kernel/front/assets/pkg`，改为绝对路径；④ **修掉脚本的 target 选型错误**——原注释「刻意不用 `--target web`」是误解：需要 bundler 的是 `--target bundler`；`--target no-modules` 产出的是 **CommonJS 风格 IIFE、无 `export` 语句**，原生 ES module 前端**根本 import 不到它的导出**（动态 `import()` 得到空命名空间）；`--target web` 产出**带命名导出的原生 ESM**（`export function kernel_version()`）+ 用 `import.meta.url` 定位 `.wasm`，正是零构建前端要的形态。**证据**：`tools/build-wasm.sh` 跑通，产物 61 K wasm + 8 K ESM glue 落 `front/assets/pkg/`；Node 22 实测 `kernel_version()=0.1.0` / `protocol_schema_version()=1` / `ping("你好, RISC-V")=pong:你好, RISC-V`（UTF-8 往返正确）。⚠️ **本条只闭合「构建路径可跑通 + 导出可调用」**；真正的 `process_message(json) → json` 协议桥属**第 1 阶段**，此处**刻意不做半成品**——一个会静默丢命令的半截桥，正是 D-24「快照 API 看着完整实则不是」那一类事故 |
| **D-39** | 契约 | 收口轮检视 V-A · 🟢-6 | **`validate()` 不检查「队列最早到期时刻 ≥ `state.virtual_time_ps`」**：一个 `at_ps` 早于当前虚拟时间的快照能通过校验 | ⏳ **未做**：引擎产出的快照永远满足该不变量，故当前是**防御性检查缺口**而非真缺陷。补法：`validate()` 增加一条检查（可复用 `E_SNAP_02` 队列畸形码，或新开场景码） |
| **D-41** | 契约 + M-13 | 收口轮检视 V-C · H-1 附带（pre-existing） | **契约层 `SimState` 缺「内存页」与「外设状态」两个字段**：LLD 总纲表12 把 `SimState` 的关键字段列为「PC、通用寄存器、CSR、**内存页**、**外设状态**、虚拟时间、事件序号」，而契约层现行的 `SimState` 只有 `gpr/pc/csrs/pending_writes/virtual_time_ps/event_queue_head/seed` | ⏳ **已登记未做**：**pre-existing**（不是本分支引入），但收口轮的裁决把问题**钉实**了——快照载荷已定为 `Snapshot{cpMeta, m13State, m14Map, m15Periph}`，其中 `m13State` 负责「指令级状态（寄存器/内存视图）」、`m14Map` 负责「总线/内存映射状态」、`m15Periph` 负责「外设状态」；**内存页与外设状态因此不进 `SimState`，而是被拆到了 M-13/M-14/M-15 三段里**（表12 已加注说明现行实现是设计意图的子集）。**这意味着「`SimState` 直接落盘即可还原会话」这条路被正式放弃**，M-20 必须按三段分别采集。待做：M-13 / M-14 / M-15 各自补状态导出与载入接口（`loadState` / `loadMap` / `loadPeriph`，LLD 评审 20-1 已列） |
| **D-40** | 契约 | 收口轮检视 V-C · L-3 | **`EngineSnapshotDto::default()` 是一个「合法但空」的快照**：`validate()` 对它返回 `Ok(())`，宿主若把 `default()` 落盘，得到的与「真实快照」在类型上不可区分 | ⏳ **未做**：契约层不提供 `Default` 是 breaking change，但可加 `is_empty()` 供宿主**显式拒绝**空快照。影响有限（宿主自造夹具时的常见误用） |
| **D-42** | M-13 + M-14 | 第 2 阶段 golden 对拍（2026-10-20） | **「与 Spike 一致允许非对齐」的表述缺了一个必要限定**：Spike 只在 **Zicclsm 扩展启用**时才*执行*非对齐访问——`riscv/mmu.cc` 的 `is_misaligned_enabled()` 就是 `proc->extension_enabled(EXT_ZICCLSM)`，为假时直接 `throw trap_load_address_misaligned`。**stock `spike --isa=rv32imc` 是抛陷阱的**，而 M-13 的默认 `MisalignedPolicy::Allow`（执行）与它**相反**，只与「启用了 Zicclsm 的 Spike」一致 | ✅ **已闭环（v0.12）**：本轮对拍已把参考侧 ISA 钉为 `rv32imc_zicclsm_zifencei` 规避（口径见 `tests/golden/README.md`），M-13 的 `executor.rs` 注释已补上限定语，**LLD 措辞已于 v0.12 订正**（M-13 §3.6(2)/§5.2.1 与 M-14 §3.2/§6.2(1) 均补上「前提是参考侧 Spike 启用 Zicclsm 扩展」）。另需决策：`MisalignedPolicy::Trap` 一旦被启用，是否需要配套的陷阱投递（依赖 M-16，第 5 阶段） |
| **D-43** | M-13 | 第 2 阶段 golden 对拍（2026-10-20） | **M 扩展（8 条）与 C 扩展（1 条）尚未实现**，golden 语料 51 例中 **9 例无法与 Spike 对拍**（`rv32um/*` 8 例 + `rv32uc/rvc` 1 例，现状是 `IllegalInstruction` 陷阱） | ⏳ **已登记未做**：`tests/golden/corpus.txt` 已把这 9 例标为 `pending`——对拍时它们报 `expected-gap` 而**不**计为失败，语料覆盖率因此始终可见；M-13 补齐 M / C 后把标记翻成 `active`，即自动纳入门禁。**第 3 阶段**的工作项 |
| **D-44** | golden harness | 第 2 阶段检视 V-B · O-1 | **变异金丝雀的判别力只覆盖「判决运输路径」**：金丝雀执行的是 `li` / `beqz` / `sll` / `or` / `sw` / `auipc` / `j` 这 7 条指令构成的自检路径，**不在该路径上的指令被改错，金丝雀在原理上不可能察觉**（它根本不执行它们）。实测：注入 10 个 RV32I ALU 缺陷，**8 个漏检**；唯一被金丝雀抓到的两个是 `add` 与 `or`——`add` 是因为金丝雀恰好就是 `add.S`（**用例选择的巧合，不是判别力**）。全量对拍（Spike 作独立 oracle）则 **10/10 全抓到** | ⏳ **已登记未做**：金丝雀的定位已**下调**为「协议握手自检」并在 README 写明「它不证明什么」；并补了**阳性对照**（未改动的上游 `add.S` 两侧都必须 PASS）关掉「永远报 FAIL」这条假绿通道。待做两种更强形式：① **判决序号覆盖式变异**——对语料每个用例的每个 test number N 生成期望值篡改变体，要求两侧同报 `FAIL N`（把「失败路径」扩成「每个断言一条失败路径」，覆盖**被测指令本身**）；② **候选侧故障注入**——`riscv-golden --fault-inject` 随机翻转一条指令的语义，要求语料必须变红 |
| **D-50** | M-19 | D-46 检视 V-C · 🟡-6 | **M-19 的环形缓冲在生产路径上「只写不读」**：M-18 → M-19 的 `EdgeSink` 握手是通的（边沿确实进了环），但**没有任何人读这个环**——CSV / VCD 导出还没接进契约，前端拿到的波形事件取自 M-18 自己的边沿存储 | ⏳ **已登记未做**（D-46 检视轮）：⚠️ **不是「没接线」，是「接了但没人用」**——这正是「实现齐了 ≠ 接上了」的另一种形态。M-19 的环是**导出的数据源**（CSV / VCD），而导出属于 M-11 的活。**待做**：M-11 落地时把 `WaveformPipeline::export_csv` / `export_vcd` 接进契约（新增一条「导出」命令或一条导出事件），并让宿主在导出时**从 M-19 的环取数**而不是从 M-18 的边沿存储取数。⚠️ 在那之前，**M-19 的环是一份纯内存开销**（每通道 200 000 条上限）——但它同时是「边沿历史的唯一副本」，删不得 |
| **D-51** | M-18 | D-46 检视 V-A · 🟡-9 | **M-18 的访存 trace 环在宿主里从不被 drain**：`TraceStream` 的上限是 `TRACE_CAP = 200 000` 条，宿主装了总线 hook 之后**每一条访存都进环**，却**没有任何消费者**——M-11（trace 导出）还是 stub。后果：常驻内存 3~5 MB，且**边沿之外的数据全部白攒** | ⏳ **已登记未做**（D-46 检视轮）：trace 环是 **M-11 的数据源**（访存轨迹 / 指令覆盖率，FR-14），在 M-11 落地前没有消费者。⚠️ **不能简单地在宿主里定期 drain 掉**——那会把 M-11 要用的历史冲掉。**待做**：M-11 落地时接一条「宿主 → M-11」的取数通道；或先给 `TraceStream` 加一个**更小**的上限（当前 200 000 条对 MVP 偏大），把常驻内存压到可接受量级，等 M-11 落地再放大 |
| **D-52** | M-18 + 内核宿主 | D-46 检视 V-A · 🟡-6 / 🟡-7 | **探针管道装配失败是「静默」的，缺判别力**：内核宿主装管道时，`define_channel` / `attach_signal` 的失败原本被 `let _ =` 吞掉，`attach_peripheral_probes` 因 `try_borrow_mut` 失败跳过的外设也**不吭声**——通道没接上时，波形视图就是空的，而宿主**一声不吭** | ◐ **2026-10-21 推进（D-46 检视轮）**：① 失败改为**计数**（`KernelHost::probe_wiring_failures()`）——可见了；② `attach_peripheral_probes` 增加**运行期重试**（每轮 `Run` 前重试一次，`attach_signal` 对同一 `(source, signal)` 幂等，重复调用不冲突）。⏳ **剩余**：计数器的**失败路径没有对应用例**（健康路径只能断言「计数为 0」）——反向变异 **N8** 实测「不计数」的变异**不会**让任何用例变红，即这条路径的判别力是 0。**待做**：造一个可复现的装配失败（例如让宿主对着一个已占用通道号 `define_channel`），补一条「失败被计数」的用例 |
| **D-53** | M-14 + M-18 + 内核宿主 | D-46 检视 V-A · 🟡-1 / 🟡-4 / 🟡-8 | **三处「静默降级」已修，但其中两处的失败路径仍无判别力**：① **M-18 边沿存储无上限**（不订阅时只进不出，实测堆 30 万条）→ 已加 `EDGE_STORE_CAP`（drop-oldest + 计数）；② **单条波形事件无上限**（实测一条 8 MB 事件）→ 已加 `MAX_WAVEFORM_EDGES_PER_EVENT`（保留最新 + 计数）；③ **一个外设 `tick` 报错会让其余外设错过时钟同步** → 已改成「继续 tick 后面的，报第一个错」 | ◐ **2026-10-21 推进（D-46 检视轮）**：①②③ 三处**都**已修，且**都**配了判别力用例（`the_m18_edge_store_is_bounded` / `a_published_waveform_event_never_exceeds_the_edge_budget` / `a_failing_peripheral_does_not_stop_the_others_from_ticking`），反向变异 **N4 / N5 / N7** 全部变红。⏳ **剩余**：M-18 的淘汰是**成批**的（超上限时丢最旧的 1/4，不是逐条）——批大小 `EDGE_STORE_EVICT` 本身没有用例钉住（改它不影响任何断言）。**待做**：补一条「存储长度在 `[CAP - EVICT, CAP]` 区间内」的断言，把批大小也钉住 |

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

### 2.2 投入产出比排序（跨模块全局 · 2026-10-20 重排）

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

### 2.3 整体判断

**做得好的部分**：4 个已开发模块（M-14/15/18/19）合计 9 831 行、194 个测试，模块内部质量普遍高于平均——M-19 的双源解耦、序列号 + DropRange 补洞、k-way 归并导出都是扎实的设计；M-15 的契约 additive 扩展策略（`read_clear` / `on_probe` 默认实现）让 M-0 冻结未被破坏。**这一轮的 review → 闭环 → 复查循环非常有效**，65 项闭环里有相当一部分是「以测试固化既有正确行为」而非改实现，说明代码作者的判断力是可靠的。

**v0.11 新增的好消息**：5 路并行交付证明**这个仓库的模块边界足够干净**——M-13 / M-12 / M-06 / 前端服务层 / 三处接线五组任务，文件所有权零交集，5 个独立 worktree 同时施工，最终 **0 merge 冲突、0 越界改动**（`crates/contract/**`、`lib.rs`、`m17_backend` 全部零改动）。子会话的自查还额外挖出 3 个真缺陷（S/B/J 立即数符号扩展位宽、WebSocket 握手前误报 connected、会话重连无限递归），其中后两个是**只有真实浏览器能暴露**的时序缺陷。

**结构性问题**：

1. **模块做完了，模块之间没接上**。M-14 ↔ M-15 ↔ M-18 三处断点，把 4 个高质量模块的实际价值压在了「总线 trace」这一条窄通路上。这是当前**投入产出比最高的修复方向**——不需要写新功能，只需要接线。
2. **门禁覆盖不到前端与畸形输入**。203 个 Rust 测试全绿的同时前端白屏、M-06 能被一个 4 字节畸形记录打崩。两条最小门禁（ESM 导入检查 + 畸形输入表驱动）能防住绝大部分这类问题。
3. **9 个模块仍是纯 stub**（645 行）——M-08/09/11/16/17/20/21/22/23，调试服务、DWARF、持久化、HMS 全部待开工。按 §2.1 关键路径，**M-13 扩 RV32IMC 全集（12 人日）是主线**，但它被 D-04（x0）挡在前面。

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
| 🟡-3 | M-14 措辞说「并进 `pending_writes`」——但 `pending_writes` 在生产代码里**只写不读** | ✅ 措辞收窄：「提交路径待 D-07 落地」 |
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

V-B 的范围是 `front/` 与 `tools/`（前端接线、桥工具、真机复验脚本）。
它抓到两条**前端门禁的判别力缺口**——两条都是「**用例都在，但守的不是
那条不变量**」：

#### 🟠-1 「装载工程后探针流被强制重订」**根本没有被守**

`loadProjectIntoKernel` 此前是**私有**函数，前端门禁里**一条用例都碰不到
它**。V-B 实测：把里面的 `await waveform.subscribeProbeStream()` 删掉，

```
tools/check-front.sh → # tests 152 / # pass 152 / # fail 0
```

——**全绿**。而这正是 D-46 最核心的不变量（内核的 `LoadProject` 会清空主题
集合，新工程 = 新会话，FR-13；不重订的话探针流**静默停掉**）。与 **D-45
同一个坑**。

**处置**：`app.js` 导出 `loadProjectIntoKernel`（导出是为了让它可测），
`front/tests/waveform.test.mjs` 补 2 条用例：三步的**顺序**都做了，以及
「重订必须**真的发一条** `Subscribe {sim.waveform}` 命令」。
**判别力**：删掉第 3 步 → **2 条用例变红**（此前 0 条）。

#### 🟡-2 共享单例 + 异步 bootstrap = 用例之间的**竞态**

`app.js` 是**引导入口**——**导入即启动**。它的 `bootstrap()` 是**异步**的，
里面会调 `waveform.reset()`；而 M-03 的视图状态是**模块级单例**。于是那条
reset 可能落在**任意两个用例之间**，把上一个用例刚推进 store 的通道清掉。

V-B 的 probe D2 实测（修复前）：

```
pushed  -> 1
after 0ms  -> 0      ← bootstrap 的 reset 落下来了
after 500ms-> 0
```

⚠️ 也就是说：`front/tests/waveform.test.mjs` 当时 154/154 全绿，**只因为
时序恰好对**。这不是「用例写对了」，是「运气好」。

**处置**：`app.js` 的自启动加开关（`globalThis.__RVSIM_NO_BOOTSTRAP__`），
需要导入但不要副作用的测试先设上它；浏览器路径不受影响。
**判别力**：这条的判据**不是**「用例变红」（原缺陷是**偶发**的），
而是**探针**：同一个脚本连跑 N 遍，store 里的通道数必须**始终**是 1。

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

### 3.5.6 反向变异自检（20 条）

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

## §4 模块报告索引与模块内状态

| 模块 | 报告 | 状态 |
| --- | --- | --- |
| M-14 | `M-14_BusMemoryMap_Review.md` §四 | ✅ 模块内 100% 闭环；#6 上收为 D-10、#9 上收为 D-13（跨模块，均未动） |
| M-15 | `M-15_PeripheralModel_Review.md` §五 | ✅ 模块内 100% 闭环（N1/N2/N3 已落地）；#11 上收为 D-12、N4 上收为 D-14 |
| M-18 | `M-18_Probe_Review.md` §一·五 | 13/15 闭环；#10 契约 `Probe` trait 死契约（与 M-15 同源）、#12 `drain_waveform` 借用交错（均低危） |
| M-19 | `M-19_WaveformPipeline_Review.md` | ✅ 16/16 闭环（`cee6b61`），含 1 项阻断级（`Clone` + `capacity()` debug panic，M-18 同步修复） |
| 前端 M-01/02/03 | `FRONTEND_Review.md` | 🟢 2026-10-20 首次模块级 review → **同日全部闭环**（20/20，含 3 阻断与 4 项评审漏项）；新增前端门禁 38 用例 / 7 suite，`fc83e1e`~`6fc9e1f` + 复审闭环。⚠️ 其中 2 项漏项（#19 / #20）**只有真实浏览器能暴露**——修完白屏后视图体仍全空。**复审（`FRONTEND_Review.md` §八）确认 16 项闭环无误，但门禁存在 3 处盲区**（#10 / #11 / #12 改回去仍绿）**+ 1 处位置耦合缺陷**（`app.js:81` 的 Promise.all 下标解构）——✅ **2026-10-20 复审闭环**：3 处盲区已封、位置耦合已消除、stub 5 处保真度缺口补齐（详见 `FRONTEND_Review.md` §八·7） |
| M-13 | `M-13_ISS_Review.md` | ✅ **v0.11 首次模块级 review**（13 项问题全部闭环，含 1 项影响全部负立即数的真实缺陷：S/B/J 型立即数符号扩展位宽少算一位）；RV32I 41 条落地，M/C 扩展待做。**2026-10-20 检视轮**：初版**漏报** 1 项阻断级分类缺陷（SYSTEM 未校验 `funct3`，`M-13_ISS_Review.md` §一 #14），已由 `caf7f82` 修复；另有 3 项覆盖漏洞（`sequential` 步进 / 地址回绕 / 子字存储截断契约）补齐；`M-13_ISS_Review.md` §一 #6 的「契约要求非对齐 trap」已订正（契约层零处提及） |
| M-12 | **尚无模块报告** | ⚠️ v0.11 已完成 D-05 / D-06 / D-17 的**加固与自测**，但**未做独立模块级 review**（T2 的任务范围不含 review 文档）。M-12 从未被独立检视过——与 M-06 同性质的盲区，**已登记为 D-30，列为 Wave 2 待办** |
| **收口轮汇总** | **本文件 §3.1** | 📋 **收口轮 3 路独立检视（V-A 契约 / V-B 内核接线 / V-C 跨模块文档）的完整证据归档**：37 项发现逐条实证、3 张变异测试表、**逐条闭环对照表**、数字核对表、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
| **第 1 阶段检视轮汇总** | **本文件 §3.4** | 📋 **第 1 阶段 3 路独立检视（V-A Rust 内核 / V-B 前端与桥 / V-C 跨模块与文档）的完整证据归档**：13 项发现逐条实证、**13 项反向变异自检全部变红**、逐条闭环对照表（22 行）、方法论沉淀。**AGENTS.md 要求的「检视产物」**——本分支合 main 的依据 |
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
