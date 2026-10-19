# M-18 探针 · 代码审查报告

> **审查对象**：`crates/kernel/src/m18_probe/` 全部 5 个文件（`mod.rs` 57 行 + `channel.rs` 129 行 + `trace_stream.rs` 197 行 + `registry.rs` 500 行 + `tests.rs` 540 行，共 1423 行） + 契约层 `contract/src/{port,model}.rs` 的 M-18 增量
> **审查基准**：`docs/detailed-design/M-18_Probe.html`（v0.8 §1~§7）、`docs/progress/PROGRESS.md` v0.8、`riscv_sim_contract::port::{Peripheral::on_probe, ProbeSink, Probe}`
> **审查日期**：2026-10-19
> **审查方式**：全文件通读 + 契约层增量核对 + 端到端路径梳理（bus hook + periph signal）
> **门禁状态**：`cargo test --workspace` 98 passed / 0 failed（M-18 专项 20 passed）；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：**未发现模块内阻断级缺陷**。M-18 是当前质量最好的模块——边沿判定逻辑经充分测试、drop-oldest 环行 + 生命周期计数器实现稳健、「首次观察必发」的细节修正有清晰的工程判断（`registry.rs:155-157`）。主要风险落在性能（外设观察热路径的字符串分配）与契约层技术债（未使用的 `Probe` trait + TraceEvent 的 `dir: u8` 魔数）。**所有跨模块待办已在台账中**（D-07 / D-12），按约定不动。

---

## 一、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 建议修复 |
| --- | --- | --- | --- | --- | --- |
| 1 | **中** | `registry.rs:339-348` | `record_signal` 热路径在每次外设观察时执行 **两次 `String::to_string()`** | GPIO OUT 每次翻转、UART 每次 TX 字节、Timer 每次 IRQ 都会进这条路径。`self.lookup.get(&(periph_id.to_string(), signal.to_string()))`（`:342`）与后续 `record_signal` 内部记录又对同一对字符串做一次查找 + 二次分配。智能灯固件跑一分钟可产生百万级事件，此处的分配热路径不可忽视 | 改用 `Rc<str>` / `Arc<str>` 作 `BTreeMap` 键；或引入 `lookup.get_by_ref(&str, &str)` 避免分配。或把 `(periph_id, signal)` 一次性 intern 进 `string_cache` |
| 2 | **中** | `registry.rs:388-433` | **通道禁用/重启用之间的 `prev` 状态未恢复** | `prev` 缓存在 `BTreeMap<u8, Option<u64>>`（`:158`），但**只在 `sample_inner` 走到边沿判定时才被更新**（`:436`）；通道关闭期间的状态完全丢失。一旦 channel 被关闭后重新启用，**第一次观察无论值是否与关闭前相同都会作为「首次观察」发一次**——破坏了边沿检测的语义 | 在 `set_rate` 关闭通道时保留 `prev` 缓存；启用时按当前最新硬件值做一次「重新同步」调用 `prev.insert(ch, Some(current))` 后再开 `master_enable`。或在 `define_channel` 时要求外部预填 prev |
| 3 | **中** | `registry.rs:285-310` | `wire_signal_probe` 返回的闭包持 `Rc<RefCell<>>` 并每次 `borrow_mut()`——**调用方若同时持借用会 panic** | 测试 `wire_signal_probe_routes_periph_signal_to_channel`（tests.rs:411-427）演示了正确的交错顺序（先 `sink(...)` 再 `reg.borrow()`），但**契约仅写在一个方法注释里**。M-15 接通时三个外设的 `on_probe` 实现各自会捕获 `reg`，若使用方持有 `borrow` 时收到外设的边沿回调就会 panic | 在 `wire_signal_probe` 的 pub doc 显式说明「调用方不得在调用 `sink` 时持有 `reg.borrow()`」；或改用 `try_borrow_mut` + 失败时静默丢弃并计入新的 `borrow_panics: u64` 统计计数 |
| 4 | 低 | `registry.rs:437-450` | `let kind = classified?; let _ = kind;` 死代码 | 局部变量 `kind`（line 437）立即被同名的 `let _ = kind;`（line 450）覆盖并丢弃。`Edge` 结构体确实没带 kind 字段，所以这是有意的「边界走过」标记，但写法误导 | 改为 `let _edge_kind = classified?;` 或在函数顶 doc 明确说明「kind 算出来只为走通 classify 路径，不携带」；或给 `Edge` 加可选 `kind: Option<EdgeKind>` 字段 |
| 5 | 低 | `tests.rs:251-255`（注释） | 节流测试断言对了，但内嵌的算法说明写错了 | 测试断言 `throttle=4 → emit on 4th, 8th`，实测确实如此。但注释（lines 251-255）写「only when it reaches 0 does the sample emit」——**实际代码是「counter = 1 时就发」（`registry.rs:403` 的 `if *counter > 1`）**，不是「counter = 0」。读测试的人会被误导 | 把注释改成「counter decrements from N to 1; emission lands when `*counter == 1`」 |
| 6 | 低 | `registry.rs:247` | `define_channel` 把「定义通道」与「启用通道」耦合 | 行内注释 `auto-enable (caller can clear)` 但**目前唯一能改 enable 的途径是 `set_rate`**——这意味着「先注册一堆通道、再统一开启指定子集」必须 `define_channel` × N + `set_rate` 两步，**「先定义后启用」**的场景不够直接 | 文档化这条两步骤用法；或加 `SampleCfg::disable_channel(ch)` 让单点可控。也可考虑默认不 auto-enable，让 caller 必须显式 enable |
| 7 | 低 | `registry.rs:362-386` | `sample_irq_pulse` 的前置条件（必须先 `define_channel(CH_IRQ_PULSE, Pulse)`）只写在行内注释 | 公开 API 上看不到这个不变量。调用方 `sample_irq_pulse` 拿到 `None` 会以为是没脉冲，其实是没注册通道。问题描述与 `#2` 类似但更严重——「无信号」与「未注册」无法区分 | 在 `sample_irq_pulse` 的 pub doc 显式声明前置；或自动注册为 Pulse 通道（但与「channel ownership explicit」的设计相悖）。`ChannelNotFound` 错误码更可取 |
| 8 | 低 | `trace_stream.rs:60-69` | `with_capacity(0)` 直接 `assert!` panic | 构造器崩溃而非返回 `Result`，与同仓库 `prepare_bytes` 等错误返回模式不一致。对调用方没有「构造失败但可恢复」路径 | 改为 `Result<Self, …>` 或在校验阶段直接 `panic!` 时附更明确的「use `with_capacity(TRACE_CAP)` 或 `default()`」使用建议 |
| 9 | 低 | `channel.rs:112-128` `ChannelDef::classify` | `SingleBit` 通道假设 `prev` 和 `next` 必为 0/1，但**不强制截断** | 若调用方传 `prev=5, next=1`（违例输入），不会返回 `Value`（因为 `prev == 0` 与 `next == 0` 都假），确实返回 `Value`，**但 SingleBit 的语义已被破坏**。同理 `prev=5, next=0` 不会返回 `Falling`（因为 `prev != 0` 已 true，next == 0 命中）——这倒是对的。当前没暴露 bug，但单测 `multi_bit_value_change_does_not_classify_edges`（tests.rs:128-148）的注释把这点隐式带上 | 在 `classify` 入口对 SingleBit 通道做 `let prev = prev & 1; let next = next & 1;` 截断；或契约层文档明确「SingleBit 通道 caller 必须只传 0/1」 |
| 10 | 低 | `contract/src/port.rs:158-167` | 契约 `Probe` trait **全代码库零实现** | grep 整个 `crates/` 没有 `impl Probe for`（除 `From<ProbeError> for PlatformError` 这种无关的）。`make_bus_hook` 与 `wire_signal_probe` 走的是 M-18 自己的 `ProbeSink` 类型。`Probe` trait 与 M-18 的 `ProbeRegistry` 命名重叠 | 三选一：① 删 `Probe` trait（最干净）；② 保留 trait，**写一个 `impl Probe for ProbeRegistry` 适配层**（让外部代码能用 trait object）；③ 文档明确「contract 保留用于外部实现，kernel 内部用 `ProbeRegistry`」 |
| 11 | 低 | `model.rs:TraceEvent` | `dir: u8` 魔数（0=Read, 1=Write） | `registry.rs:322-325` 硬编码 `0u8 / 1u8`；`AccessDir` 在 M-14 是正经 enum，TraceEvent 应有 `TraceDir` 配套 | 引入 `enum TraceDir { Read, Write }` 配 `From<AccessDir>` |
| 12 | 低 | `registry.rs:469-486` | `drain_waveform` 在迭代 `self.channels` 时 `self.edges.remove(ch)` | 字段各自独立，可借用检查通过；**但未来若把 channels 与 edges 合并到一个内部结构会立刻破坏**。一个 `assert!(self.channels.contains_key(ch))` 风格的文档断言会更有韧性 | 拆分 `drain_waveform` 为「先快照 keys、再分别 remove」两步；或在迭代前一次性 swap 出整个 edges 哈希 |
| 13 | 低 | `trace_stream.rs:102-117` | 环行正确性依赖 `Vec::with_capacity` 不超额分配 | `Vec::with_capacity(N)` 文档承诺分配恰好 N；N+1 推送才 `Vec::push` 的扩容路径会让 `head` 模运算落空。**目前正确**，但**鲁棒性依赖一个稳定但未在编译期保证的 Vec 行为**。若有人改用 `arrayvec` 或自定义 ring buffer，公式要重新核对 | 内部用 `[Option<TraceEvent>; N]` + 编译期常量；或加 `debug_assert!(self.buf.capacity() == self.initial_cap)` |
| 14 | 低 | `trace_stream.rs:36-48` | `TraceStream` 没有 `Clone` 实现 | 调试 / 跨线程传递场景会受限（虽然 MVP 单线程）。同模块 `ProbeRegistry` 也是 `#[derive(Debug)]` 而非 `Clone` | 按需加 `Clone`（含 seq/head/len 状态，dropped 是单调的所以可 Copy） |
| 15 | 低 | `tests.rs` | 测试盲区：`ChannelDef::classify` 单元测试（直接覆盖 `prev=5, next=1` 等违例输入）、`set_rate` 的 throttle 状态重置行为（注释说「不重置」但没测试锁）、`TraceEvent` dir 反向映射（`From<AccessDir>` 若实现）、`prev` 跨 enable/disable 的回归 | 全部针对本次复查发现的小风险点 | 逐条补齐；M-18 整体测试已 20 条，缺这几条不影响门禁 |

## 一·五、闭环核对（2026-10-19 19:37 复查）

代码规模 1423 → **1800 行**（+377）；M-18 测试 20 → **29 条**（+9）；`cargo test --workspace` 107 passed / 0 failed；`clippy -D warnings` 与 `fmt --check` 均干净。

| # | 原严重度 | 状态 | 落地证据 |
| --- | --- | --- | --- |
| 1 | 中 | ✅ 闭环 | `wire_signal_probe` 闭包**直接路由到 `sample_inner`**，跳过 `record_signal` 的 `(periph_id.to_string(), signal.to_string())` 二次分配；新增 `borrow_contended: Rc<Cell<u64>>` + `try_borrow_mut` 失败时累加并发冲突计数，**不 panic** |
| 2 | 中 | ✅ 闭环 | 新增 `prev_survives_enable_disable_cycle`（tests.rs:588）+ `prev_survives_master_disable_cycle`（:622）**两条测试锁死不变量**：BTreeMap 的 `prev` 在通道关闭 / 主开关关闭期间不会被清空，重新启用后第一次「相同值」仍被正确判为边沿重复（不是「首次观察」）。原报告里的「状态丢失」推断是错的——关闭时早退在 master/enable 检查（:484-494），**根本没走到会清 prev 的代码路径**。所以本项的修复是「以测试固化既有正确行为」，不是改实现 |
| 3 | 中 | ✅ 闭环 | 同 #1：`try_borrow_mut` 替代 `borrow_mut`，失败时增加 `borrow_contended` 计数并**静默丢弃**该观察（不 panic、不阻塞）。新增 `wire_signal_probe_does_not_panic_on_contention` + `wire_signal_probe_normal_path_increments_zero_contention` 两条测试 |
| 4 | 低 | ✅ 闭环 | 改为 `let _kind = classified?;`，命名直接表达「算出来仅为走通路径」 |
| 5 | 低 | ✅ 闭环 | tests.rs 内的 throttle 算法注释已重写为「counter 4→3→2→1 在 counter==1 时发」的逐拍 trace，与 `registry.rs:498-500` 的 `if *counter > 1` 逻辑一致 |
| 6 | 低 | ✅ 闭环 | `define_channel` 的 pub doc 显式标注 **「Auto-enable side effect (M-18 review #6, 2026-10-19)」**，并给出 `cfg.enable &= !(1 << CH_GPIO0)` 的批量注册后清理写法 |
| 7 | 低 | ✅ 闭环 | `sample_irq_pulse` 公开 doc 显式声明「there is no way to distinguish 'no pulse arrived' from 'channel not registered'」并指明「callers should set up the channel once at boot and not rely on the silent no-op path」 |
| 8 | 低 | ✅ 闭环 | `with_capacity(0)` 仍 `assert!`（契约层风格不一致属低优先），但 panic 信息升级为「use `default()` for MVP capacity (2×10⁵); use this constructor only when you need a non-default size」，给出明确使用建议 |
| 9 | 低 | ✅ 闭环 | `ChannelDef::classify` 在 `SingleBit` 分支入口对 `prev` 和 `next` 各做 `& 1` 截断；新增 `classify_masks_single_bit_inputs` 测试覆盖 `prev=5, next=1` 等违例输入 |
| 10 | 低 | ❌ **未闭环** | 契约 `Probe` trait（`port.rs:158-170`）原样保留，全代码库仍无 `impl Probe for`，`ProbeRegistry` 改名为 `make_bus_hook` / `wire_signal_probe` 而非实现该 trait。属契约层技术债，**与 M-15 报告 #10 重复**，建议在 M-0 下一窗口统一清理 |
| 11 | 低 | ✅ 闭环 | 契约层新增 `pub enum TraceDir { Read, Write }` + `Default` impl（`model.rs:413-432`），`TraceEvent.dir: u8` 改为 `dir: TraceDir`（:454），并附 `from_u8`/`as_u8` 与 `AccessDir` 的双向 `From`。`registry.rs:322-325` 不再硬编码魔数 |
| 12 | 低 | ❌ **未闭环** | `drain_waveform`（`:549-565`）在迭代 `self.channels` 时调用 `self.edges.remove(ch)` 的借用交错模式未动。当前可工作（不同字段），但未来合并两表会立刻 break；建议拆为「先快照 keys、再分别 remove」两步 |
| 13 | 低 | ✅ 闭环 | `TraceStream` 新增 `initial_cap: usize` 字段（`:57`），`with_capacity` 写入该字段；`push` / `drain` / `capacity` 全部改用 `self.initial_cap` 而非 `self.buf.capacity()`，并加 `debug_assert_eq!(self.buf.capacity(), self.initial_cap)`（`:103`）和 `debug_assert!(self.len <= self.initial_cap)`。**从此不再依赖 `Vec::with_capacity` 不超额分配这一未被编译期保证的行为** |
| 14 | 低 | ✅ 闭环 | `TraceStream` 改 `#[derive(Debug, Clone)]` |
| 15 | 低 | ✅ 大幅闭环 | 测试 20 → 29 条，新增覆盖：throttle 状态跨 set_rate、`prev` 跨 enable/disable 与 master/disable、SingleBit LSB 截断、`wire_signal_probe` 在借用冲突下不 panic、normal 路径下 contention 计数为 0。**但** 「`ChannelDef::classify` 直接单元测试」是顺手补的（被 #9 测试覆盖），「`TraceEvent::dir` 反向映射」未补独立测试 |

**统计：模块内部 13/15 项闭环，2 项未闭环（#10 契约层技术债，与 M-15 报告 #10 重复；#12 `drain_waveform` 借用交错，属未来重构风险）**。

#10 列入**契约层技术债**清单；#12 与 M-14 报告的「`regions()` 返回迭代器」建议可合并入一次 `refactor(m18): drain + iteration patterns` PR。

---

## 二、跨模块欠账（已在台账追踪，本报告不重复）

| D-ID | 描述 | 状态 |
| --- | --- | --- |
| **D-12** | M-14 ↔ M-18 接线（`bus.set_hook(Some(reg.make_bus_hook()))`）和 M-15 ↔ M-18 接线（`periph.on_probe(Some(reg.wire_signal_probe(...)))`）**两端都没接**。`make_bus_hook` 与 `wire_signal_probe` 两个 helper 已就位但**全代码库零调用方**（grep `make_bus_hook` / `wire_signal_probe` 仅命中 `registry.rs` 自身 + `tests.rs`） | ❌ 接线延迟，PROGRESS v0.8 明确「独立 PR，本轮不动」。> ⚠️ 编号口径：台账 `CROSS_MODULE_DEBT.md` 的 D-12 指 **M-14 ↔ M-15**（size 一致性），与本报告此处用法不同。v0.9 完成了 **M-18 → M-19** 这一段（新 `port::EdgeSink`），本行两段接线**仍未做** |
| **D-07** | M-12 调度器从未调用 `Peripheral::tick`，M-18 的 `record_signal` 端到端不可达 | ❌ 跨模块欠账 |
| — | M-19 波形管道仍是 stub（42 行），不在本审查范围 | P1 阶段 → **v0.9 已落地完整 `WaveformPipeline`**，并新增 `port::EdgeSink` 端口完成 M-18 → M-19 握手（本报告 §四.2 的待办闭合） |

**模块内未发现阻断级或高危缺陷。** `#1`~`#3` 是中危、可独立处理；`#4`~`#15` 是低危整洁项。`#10`（契约 `Probe` trait 零实现）是契约层技术债，建议放在 M-0 下一窗口统一清理。

---

## 三、值得肯定的设计

| 点 | 说明 |
| --- | --- |
| 边沿判定语义明确 | tests.rs 用 16 个测试完整覆盖 §18-2/§18-3/§18-4 三类通道（SingleBit 升降沿、MultiBit 任意值变化、Pulse 每事件发）；sample_inner 的「首次观察必发」修正（registry.rs:155-157）是对设计 doc 伪代码的工程化修补，避免了首条观察被静默吞掉 |
| Drop-oldest 环 + 生命周期计数器 | `trace_stream.rs` 的 `seq` / `dropped` 永不重置的设计使 M-11 跨多次 `drain` 仍能追踪读位置；`first_fill_preserves_insertion_order` 与 `overflow_drops_oldest_and_increments_dropped_counter` + `drain_resets_state_but_keeps_lifetime_counters` 三件套锁死了所有关键不变量 |
| 端到端路径双轨设计 | `make_bus_hook` 接管 M-14 全量访存，`wire_signal_probe` 接管 M-15 边沿事件；`record_bus_access` / `record_signal` / `sample_irq_pulse` 三个 entry point 都路由到统一的 `sample_inner` 边沿判定——避免判定逻辑分散在三个地方漂移 |
| 契约 additive 模式 | `Peripheral::on_probe` 用默认 no-op（与 M-15 review #2 的 `read_clear` 默认实现同模式），M-0 冻结范围内不破坏其他外设 |
| 通道冲突语义清晰 | `define_channel` 返回 `ChannelConflict` 错误（含 `existing_source`）且**保留原绑定**（tests.rs:288 显式断言），与 M-15 的 `RegisterError::Duplicate` 行为一致；`conflict_warns` 计数器提供 observability |
| 文档与 LLD §7.1 覆盖矩阵对应 | tests.rs 头注释的 13 行覆盖率表与 M-18 v0.8 §7.1 一一对应，可作为追溯索引 |

---

## 四、建议处理顺序

1. **M-14 / M-15 接线 PR（独立排期）**——#1 / #2 / #3 都在端到端可达性被接通后才能真正暴露；当前 `make_bus_hook` / `wire_signal_probe` 是设计完整但「无人调用的好代码」。**优先级**：`#1`（性能）> `#3`（panic 风险）> `#2`（边沿语义）> `#10`（契约层清理）。
2. **接线 PR 中必须配套**：M-12 的 `Peripheral::tick` 调用（D-07）——否则 #2 修的 `prev` 跨 enable/disable 行为仍走不到外设；M-19 真正消费 `drain_waveform`（当前仍是 stub）——否则 M-18 边缘事件的「产出」没有「消费者」。
3. **`#10` 契约 `Probe` trait**：建议在 M-0 下一窗口统一清理（与 `ProbeSink` 的去向一并决定），而不是混在 M-18 接线 PR 里。
4. **`#4`~`#15` 整洁项**：合并为 `chore(m18): 清理死代码与统计口径` 提交，可在接线 PR 之后做。

---

## 五、审查边界声明

- 本报告仅覆盖 M-18 模块自身；契约 `Probe` trait 与 `ProbeSink` 的去向是技术债级记录，归到 `#10`，不在 M-18 接线 PR 范围。
- M-19 波形管道（42 行 stub）与 M-11 trace 导出（待审查）不在本范围；M-15 / M-14 / M-12 的接线见台账 D-07 / D-12。
- 门禁结论为 2026-10-19 实际执行结果。
