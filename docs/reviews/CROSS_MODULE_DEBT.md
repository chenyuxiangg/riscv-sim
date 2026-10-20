# 跨模块欠账台账 · Cross-Module Debt Ledger

> **定位**：本文件是**全部 review 欠账的唯一追踪入口**——跨模块条目与模块内条目都在这里汇总状态。模块问题的**详细分析、实证记录与修复建议**见各模块报告（§八 索引）。
> **建立日期**：2026-10-19
> **状态核实日期**：2026-10-20（v0.11.0 并行开发后重新逐条回源码核对，非引用历史结论）
> **基线**：`cargo test --workspace` **314 passed / 0 failed**（lib 314 + 集成 10）；`clippy -D warnings` 干净（已删 `non_canonical_partial_ord_impl` allowance）；`fmt --check` 干净；前端门禁 **109 用例 / 19 suite**
> **关键结论**：v0.10 记录的「三条现有门禁对本台账全部 5 项阻断级条目均无感知」**已不成立**——本轮补上了 3 类最小门禁（M-13 x0 / M-12 确定性 / M-06 畸形输入），**3 条阻断级条目全部消除**。
> **维护约定**：本文件是**活文档**。模块报告的闭环章节记录该模块的细节，本台账只保留**汇总状态 + 跨模块拓扑 + 处理顺序**；两者冲突时以本台账的核实日期为准。

---

## 一、全局状态快照（2026-10-20 · v0.11.0 并行开发后）

### 1.1 模块级闭环总览

| 模块 | 代码 | 测试 | 报告项 | 闭环 | 未闭环 | 状态 |
| --- | --- | --- | --- | --- | --- | --- |
| **M-14 总线** | 1 361 → 1 900 | **38** | 16 + 接线 PR | **16** | 0 | 🟢 **接线 PR 后全清** |
| **M-15 外设** | 1 424 → 1 550 | **35** | 19 + N1~N4 = 23 | **23** | 0 | 🟢 **接线 PR 后全清** |
| **M-18 探针** | 1 893 → 2 100 | **32** | 15 | **13** | 2（低危） | 🟡 差 2 项低危 |
| **M-19 波形** | 3 897 | 85 | 16 | **16** | 0 | 🟢 **已清零**（`cee6b61`） |
| **前端 M-01/02/03** | 1 123 → 1 402 | **21** | 20（16 首审 + 4 漏项） | **20** | 0 | 🟢 **已清零**（`fc83e1e`~`6fc9e1f`） |
| **M-06 加载器** | 1 189 → 1 999 | **47** | **17（v0.11 首次模块级 review）** | **12** | 5（留账并说明理由） | 🟢 **已审查 + 加固**（v0.11） |
| **M-12 引擎** | 226 → 490 | **18** | 台账 3 项（D-05/06/17） | **3** | 0（**但无独立模块级 review**） | 🟢 **已加固**（v0.11） |
| **M-13 ISS** | 456 → 2 481 | **77** | 13（v0.11 首次模块级 review） | **13** | 0 | 🟢 **RV32I 全集落地**（v0.11） |
| **前端 M-04/05/07/10** | 66 → 1 900+ | **71** | 7 项契约缺口 + 3 自查缺陷 | 3（已修） | 7（契约缺口，留 Wave 2） | 🟢 **已实现**（v0.11） |
| **M-08/09/11/16/17/20/21/22/23** | 591 | — | 无 | — | — | ⚪ 纯 stub，未开工 |

**v0.11.0 后台账口径**：阻断级 **5 → 0**（3 条本轮闭环），高危 9 条中 **8 条闭环**（余 D-13），中低危 8 条中 6 条闭环，D-23 门禁缺口 **4 → 0**。

| 口径 | 项数 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| M-14 | 16 | 14 | 2（跨模块） |
| M-15 | 23 | 22 | 1（N4 上收台账 D-14） |
| M-18 | 15 | 13 | 2（低危） |
| M-19 | 16 | 16 | 0 |
| 前端 M-01/02/03 | 20 | 20 | 0 |
| **v0.11 新增报告** | M-06 17 + M-13 13 + 前端服务层 10 + 接线 PR 10 | 40 | 4（契约缺口） |
| **合计** | **110** | **106** | **4** |

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
> **N5「GPIO 翻转可见」的内核路径 100% 闭合**——`crates/kernel/tests/m14_m15_m19_wiring.rs` 的 11 个端到端用例为证。
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

## 三、跨模块高危（9 项）

| ID | 归属模块 | 来源 | 问题 | 阻塞关系 / 备注 |
| --- | --- | --- | --- | --- |
| **D-06** | M-12 | 首次全量审查 + M-15 报告 #6 | `run_until_idle` 每个事件耗 1µs 量子；事件在 t=1e12 ps 即空转 1e9 次 | ✅ **已闭环**（v0.11，commit `1723610`）：改为「空闲区间批量推进」——时钟直接跳到下一个到期事件，迭代次数从 O(时间/量子) 降到 **O(事件数)**。性能基线测试断言 1e12 ps 跨度 64 事件 → **步数恰为 64**、步数 < 1e9、Tick 数 == 事件数 |
| **D-07** | M-12 | 首次全量审查 + M-15 报告 #6 | `dispatch` 的 `PeripheralTick` 分支是空 TODO | ◐ **2026-10-20 推进**：v0.11 在 m12 内留好**接缝**（`PeripheralTicker` 回调：`tick(paddr, now_ps) -> Result<Vec<PendingWrite>, PlatformError>`，语义对齐契约 `Peripheral::tick`，返回的写并入 `state.pending_writes`），`Engine` 泛型不变、`m17_backend` 不受影响。**接线 PR 已把 m14 侧准备好**：总线现在持有外设实例（`BoundPeripheral`），外设 `tick` 也改为记权威虚拟时间。**剩最后一步**：M-12 的 `dispatch` 真正调用它（Wave 2 单独做，两侧接口已就位，**接线 PR 的代码无需改动**） |
| **D-08** | M-06 `hex.rs` | 首次全量审查 | `finalise` 超 `segment_count` 时 `truncate()` **静默丢固件数据**且不报错 | ✅ **已闭环**（v0.11，commit `e7fb3e0`）：改 `Result` + 报错。⚠️ T3 复核后定性为「防御性收口」——当前控制流下 `truncate` 实际不可达（循环内长度检查先于 EOF 早退拦截），但路径已封死 |
| **D-09** | M-06 `elf.rs` | 首次全量审查 | `read_u16`/`read_u32` 为 `panic!`，而上方注释承诺「returns PlatformError(E_LDR-01)」 | ✅ **已闭环**（v0.11，commit `0258a34`）：改返回 `Result` 并与注释一致，调用点同步改为错误传播，移除 `.expect` |
| **D-10** | **M-06 + M-14 联合** | M-14 报告 #6 | `ParseLimits` 无 `total_image_bytes` / `max_region_bytes`；M-14 `prepare_bytes` 裸 `vec![0u8; size as usize]`，`size` 最大可达 ~4 GiB | ✅ **已闭环（两侧齐备）**：M-06 侧（v0.11 `374d647`）—— `ParseLimits` 新增 `total_image_bytes`（默认 64 MiB）/ `max_region_bytes`（默认 16 MiB）并在解析时校验；M-14 侧（v0.11 `60dbbb8`）—— `prepare_bytes` 引入**区域大小上限**，超出返 E_BUS-09，**不先分配后检查**，`map_ram` / `map_region` 两条分配路径均受保护。84 字节畸形 ELF 已无法申请 GiB 级内存 |
| **D-11** | M-13 ISS | 首次全量审查 | `BusExecutor` 复制 `Executor` 的 ADDI 逻辑，`bus` 字段挂 `#[allow(dead_code)]` 从未被读 | ✅ **已闭环**（v0.11，commit `e068295`）：改为 `BusMemory`（只做访存适配）+ 共享的 `MemoryAccess` 端口，**指令语义收敛到唯一一条 `execute_one()`**；两条路径的非法指令语义统一为 `Trapped { IllegalInstruction }`（回归测试断言两路一致）。LOAD/STORE 另有专门测试证明**必经 M-14 总线**（否则会静默丢掉 stats 与 M-18 hook） |
| **D-12** | M-14 ↔ M-15 ↔ M-18 接线 | M-14 报告 #13 / M-15 报告 #11 | `Bus::map_peripheral(base, size, PeripheralId)` 的 size 与 `Peripheral::size()` 无一致性校验 | ✅ **已闭环**（v0.11 接线 PR，3 个 commit）：① 总线经 `Rc<RefCell<dyn Peripheral>>` **共享句柄**持有外设实例（`BoundPeripheral` + `RegionTarget::PeripheralBound`），新增 `Bus::map_peripheral_instance`，挂载时校验 `size == periph.size()` 不一致返 **E_BUS-08**；② M-15 注册表改共享句柄（`handle()` / `share()`），三外设 override `on_probe` 上报 `OUT` / `TX` / `CMP` 信号；③ M-18 新增 `ProbeRegistry::attach_signal`（LLD §1.3 的 `attachSignal`）+ `CH_UART_TX = 4`。**N5「GPIO 翻转可见」的内核路径 100% 闭合**——`crates/kernel/tests/m14_m15_m19_wiring.rs` 11 个端到端用例为证（固件写 GPIO OUT → M-14 派发 → M-15 上报 → M-18 判沿 → M-19 波形通道出现期望边沿） |
| **D-13** | M-14 | M-14 报告 #9 | `Region.target` / `RamBackend.bytes` 仍为 `pub` | ⏳ **未动**：收紧可见性要同步改 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable`——后者与 v0.11 的 T4 前端服务层改动相邻，**留 Wave 2 一并做**避免来回冲突 |
| **D-14** | 契约 + M-14 | M-15 报告 N4 | `Peripheral::read_clear` 默认实现委托 `read`，**漏覆写的外设静默获得非破坏性读** | ✅ **已闭环**（v0.11 `22107a0`）：总线读外设**一律调 `read_clear`**（不是 `read`），`Uart16550` 正确 override（读 RX 后清空 FIFO 状态）。回归测试：连续读两次 UART RX，第二次不再拿到同一字节；M-14 侧另有 `read_clear` 路径计数用例证明确实走了破坏性读 |

---

## 四、跨模块中低危（8 项）

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

第 3 批 · 🔄 接线 PR（v0.11 收口中）
  D-12 + D-14 → M-14 ↔ M-15 ↔ M-18 三处断点
  D-10        → M-06 ParseLimits（✅ 已做）+ M-14 prepare_bytes（接线 PR 内）

第 4 批 · Wave 2
  D-07        → M-12 真正调用 Peripheral::tick（接缝已留，待 m14 持有外设注册表）
  D-13        → 字段可见性收紧（需同步改集成测试与前端 renderMapTable）
  M-13 M/C 扩展 → RV32IMC 全集补完
  Spike golden 对拍 → 工具链已就绪（Spike 1.1.1-dev + xpack GCC 15.2.0）
  契约缺口 7 项 → 随 schema_version=2 一并处理
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
| **5** | **契约缺口 7 项 + `EngineSnapshot` 序列化** | ~2d | M-20/M-23 落盘的前提 |

---

## 七、整体判断

**做得好的部分**：4 个已开发模块（M-14/15/18/19）合计 8 575 行、170 个测试，模块内部质量普遍高于平均——M-19 的双源解耦、序列号 + DropRange 补洞、k-way 归并导出都是扎实的设计；M-15 的契约 additive 扩展策略（`read_clear` / `on_probe` 默认实现）让 M-0 冻结未被破坏。**这一轮的 review → 闭环 → 复查循环非常有效**，65 项闭环里有相当一部分是「以测试固化既有正确行为」而非改实现，说明代码作者的判断力是可靠的。

**v0.11 新增的好消息**：5 路并行交付证明**这个仓库的模块边界足够干净**——M-13 / M-12 / M-06 / 前端服务层 / 三处接线五组任务，文件所有权零交集，5 个独立 worktree 同时施工，最终 **0 merge 冲突、0 越界改动**（`crates/contract/**`、`lib.rs`、`m17_backend` 全部零改动）。子会话的自查还额外挖出 3 个真缺陷（S/B/J 立即数符号扩展位宽、WebSocket 握手前误报 connected、会话重连无限递归），其中后两个是**只有真实浏览器能暴露**的时序缺陷。

**结构性问题**：

1. **模块做完了，模块之间没接上**。M-14 ↔ M-15 ↔ M-18 三处断点，把 4 个高质量模块的实际价值压在了「总线 trace」这一条窄通路上。这是当前**投入产出比最高的修复方向**——不需要写新功能，只需要接线。
2. **门禁覆盖不到前端与畸形输入**。203 个 Rust 测试全绿的同时前端白屏、M-06 能被一个 4 字节畸形记录打崩。两条最小门禁（ESM 导入检查 + 畸形输入表驱动）能防住绝大部分这类问题。
3. **9 个模块仍是纯 stub**（591 行）——M-08/09/11/16/17/20/21/22/23，调试服务、DWARF、持久化、HMS 全部待开工。按 §3 关键路径，**M-13 扩 RV32IMC 全集（12 人日）是主线**，但它被 D-04（x0）挡在前面。

---

## 八、模块报告索引与模块内状态

| 模块 | 报告 | 状态 |
| --- | --- | --- |
| M-14 | `M-14_BusMemoryMap_Review.md` §四 | ✅ 模块内 100% 闭环；#6 上收为 D-10、#9 上收为 D-13（跨模块，均未动） |
| M-15 | `M-15_PeripheralModel_Review.md` §五 | ✅ 模块内 100% 闭环（N1/N2/N3 已落地）；#11 上收为 D-12、N4 上收为 D-14 |
| M-18 | `M-18_Probe_Review.md` §一·五 | 13/15 闭环；#10 契约 `Probe` trait 死契约（与 M-15 同源）、#12 `drain_waveform` 借用交错（均低危） |
| M-19 | `M-19_WaveformPipeline_Review.md` | ✅ 16/16 闭环（`cee6b61`），含 1 项阻断级（`Clone` + `capacity()` debug panic，M-18 同步修复） |
| 前端 M-01/02/03 | `FRONTEND_Review.md` | 🟢 2026-10-20 首次模块级 review → **同日全部闭环**（20/20，含 3 阻断与 4 项评审漏项）；新增前端门禁 38 用例 / 7 suite，`fc83e1e`~`6fc9e1f` + 复审闭环。⚠️ 其中 2 项漏项（#19 / #20）**只有真实浏览器能暴露**——修完白屏后视图体仍全空。**复审（§八）确认 16 项闭环无误，但门禁存在 3 处盲区**（#10 / #11 / #12 改回去仍绿）**+ 1 处位置耦合缺陷**（`app.js:81` 的 Promise.all 下标解构）——✅ **2026-10-20 复审闭环**：3 处盲区已封、位置耦合已消除、stub 5 处保真度缺口补齐（详见 §八·7） |
| M-13 | `M-13_ISS_Review.md` | ✅ **v0.11 首次模块级 review**（13 项问题全部闭环，含 1 项影响全部负立即数的真实缺陷：S/B/J 型立即数符号扩展位宽少算一位）；RV32I 41 条落地，M/C 扩展待做 |
| M-12 | **尚无模块报告** | ⚠️ v0.11 已完成 D-05 / D-06 / D-17 的**加固与自测**，但**未做独立模块级 review**（T2 的任务范围不含 review 文档）。M-12 从未被独立检视过——与 M-06 同性质的盲区，**列为 Wave 2 待办** |
| M-06 | `M-06_ELFLoader_Review.md` | ✅ **v0.11 首次模块级 review**（17 项问题 / 12 项闭环 / 5 项留账并说明理由）——**该模块诞生以来第一次被系统检视，原是台账最大的盲区** |
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
