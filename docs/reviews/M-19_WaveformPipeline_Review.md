# M-19 波形管道 · 代码审查报告

> **审查对象**：`crates/kernel/src/m19_waveform/` 全部 6 个文件（`mod.rs` 76 + `channel.rs` 237 + `ring.rs` 388 + `delta.rs` 201 + `export.rs` 508 + `pipeline.rs` 689 + `tests.rs` 768，共 2867 行）+ 契约层增量（`contract/src/{port,model}.rs` 的 `EdgeSink` / `DeltaSink` / `EdgeEvent`）+ 集成测试 `crates/kernel/tests/m18_to_m19_waveform.rs`（174 行）
> **审查基准**：`docs/detailed-design/M-19_WaveformPipeline.html`（v0.9 §1~§8）、`docs/progress/PROGRESS.md` v0.8.1、`riscv_sim_contract::port::{EdgeSink, DeltaSink}`
> **审查日期**：2026-10-19
> **审查方式**：全文件通读 + 契约层增量核对 + **三条关键结论以临时用例实证复现** + 导出性能实测（用例已删除）
> **门禁状态**：`cargo test --workspace` 176 passed / 0 failed（lib 171 + integration 5；M-19 专项 64 passed）；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：M-19 是**本项目目前完成度最高的模块**——双源解耦（ring 历史窗口 / delta 推送流）、k-way 归并导出、批次序列号 + 显式 DropRange 补洞、内存预算规划、溢出双策略，六个子模块各有专项测试，并有一条**真实跨模块端到端集成测试**（`m18_to_m19_waveform.rs` 走真实 `Bus` → `ProbeRegistry` → `WaveformPipeline` → CSV/VCD）。发现 **1 项阻断级缺陷**（`Clone` 后 `capacity()` debug panic，已实证）+ **3 项高危**（推送丢失不记 DropRange 违反模块自身不变量 / CSV 未转义破坏 NFR-04 / 导出 O(n²/chunk)）+ 8 项中低危。

---

## 一、实证复现记录

### 1.1 `ChannelRing::clone()` 后调用 `capacity()` → debug panic

```
ring clone len=1
thread 'probe_ring_clone_capacity' panicked at crates/kernel/src/m19_waveform/ring.rs:140:9:
assertion `left == right` failed: ChannelRing::buf capacity changed after construction; ring math would be incorrect
  left: 1
 right: 8
```

**同一模式在 M-18 的 `TraceStream` 上也已实证**（该文件仍在，已 staged 待提交）：

```
thread panicked at crates/kernel/src/m18_probe/trace_stream.rs:103:9:
assertion `left == right` failed: TraceStream::buf capacity changed after construction; ring math would be incorrect
  left: 2
 right: 200000
```

### 1.2 未安装 `delta_sink` 时推送丢失，且**不记录 DropRange**

```
pushed batch seq=0..5 entries=5
dropped_ranges = []
dropped_batches = 0
```

5 条边沿事件被取走、序列号推进到 5，但**没有任何 DropRange 记录这次丢失**。

### 1.3 CSV 导出未转义 label → 列结构被破坏

```
CSV:
# riscv-sim waveform export (M-19)
# entries=1 overflow_marked=false dropped_ranges=0
t_ps,ch,label,value,edge
1,0,a,b"c,5,Value          ← 5 列表头，这行被解析成 6 个字段
```

### 1.4 导出性能实测（release，200k 条目 / 20 个 chunk）

```
export 200k entries in 20 chunks took 84.6705ms (4.233525ms/chunk)
```

---

## 二、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 建议修复 |
| --- | --- | --- | --- | --- | --- |
| 1 | **阻断** | `ring.rs:140-145`（M-18 同源：`trace_stream.rs:103-108`） | `ChannelRing` 同时 `derive(Clone)` 且 `capacity()` 里 `debug_assert_eq!(self.buf.capacity(), self.initial_cap)`——**`Vec::clone` 只按 `len` 分配容量，克隆后必然不相等** | 实证 panic（§1.1）。**讽刺的是本文件 `ring.rs:95-99` 的注释恰好点名了这个风险**：「Stored explicitly rather than read from `buf.capacity()` (M-18 review #13: `Vec::clone` is allowed to shrink the capacity, so trusting `capacity()` would break the ring math)」——然后又在 `capacity()` 里断言了 `Vec::clone` 必然破坏的那个等式。**断言的报错信息「ring math would be incorrect」是事实错误**：环行数学用的是 `initial_cap`，完全正确；错的只是断言的前提。现有测试 `clone_is_independent`（`ring.rs:327`）从不调用克隆体的 `capacity()`，所以缺陷潜伏 | ① 删掉 `debug_assert_eq!`（`initial_cap` 已是唯一真源）；② 或手写 `Clone` 用 `Vec::with_capacity(initial_cap)` 重建以保住容量；③ 或把断言改成 `self.buf.capacity() >= self.initial_cap.min(self.buf.len())`。**M-18 的 `TraceStream` 必须同步修**（已 staged，随本次提交进入历史） |
| 2 | **高** | `pipeline.rs:303-318` + `delta.rs:17-21` | **未安装 `delta_sink` 时，批次被取走却没有任何 DropRange 记录** | 实证（§1.2）。`delta.rs:17-21` 的模块不变量明写：「Because the *numbers* stay gap-free, a consumer cannot detect the loss from the sequence alone. The content hole is therefore recorded explicitly as a `DropRange`」——但 `push_delta` 在 `delta_sink == None` 时走 `failed = false` 分支（`:307-310`），批次既没送达也没记洞。**消费方看到 seq 0..5 后接 5..10，永远无法知道 0..5 丢了**。这直接违反模块自己声明的核心契约 | `push_delta` 在 `delta_sink` 为 `None` 时也应 `record_drop`（内容丢失 = 需要补洞）；或把 `delta_sink` 缺失建模为「无消费者」而非「推送失败」并在文档显式声明该语义，并让消费方能查询「未被消费的批次区间」 |
| 3 | **高** | `export.rs:289-297` | **CSV 导出的 `label` 未做 CSV 转义** | 实证（§1.3）。`label` 来自 `ChannelMeta::label`，是宿主自由填入的 `String`（`define_channel(ch, meta)`），含 `,` / `"` / 换行即破坏列结构。**违反项目 NFR-04「跨层统一转义」**。VCD 路径同源问题：`export.rs:323` 的 `$var ... {label} $end` 中 label 含 `$end` 或空格会破坏 VCD 语法 | 加 `csv_escape()`（含 `,` `"` `\n` 时加引号并把 `"` 翻倍）；VCD 侧对 label 做标识符字符白名单过滤（只保留 `[A-Za-z0-9_]`），非法字符替换为 `_` |
| 4 | **高** | `export.rs:495-501` | **每个 chunk 都从 0 重新走一遍合并流** → 整体 O(n²/chunk) | `cursor.skip(next_offset)`（`:501`）把前 `next_offset` 条**重新走一遍**；`count_filtered`（`:499`）每个 chunk 全量扫描；`sort_by_time`（`:495-497`）每个 chunk 重跑 `is_time_ordered()` 全量检查。实测 200k/20 chunk = 84.7ms（release）；**按 4×200k 满窗（80 chunk）外推约 1.3s**，浏览器 WASM 调试构建下会更差。根因是 `next_offset` 被设计成「合并流索引」而非「可续传的游标状态」 | ① 让 `ExportHandle` 持有一个可续传的 `MergedCursor`（`Arc<Mutex<..>>`），chunk 之间不重建；② 或把 `next_offset` 换成 per-channel 的 `(ch, offset)` 组合，续传时直接定位；③ 至少把 `count_filtered` 的结果缓存到 `self` 上按环行代数失效 |
| 5 | 中 | `export.rs:73-78` | `ExportFilter::includes_channel` 用 `Vec::contains` → **每条边沿 O(k) 线性查找** | `channels: Option<Vec<u8>>` 是 `Vec`，`list.contains(&ch)` 在 800k 条 × k 个通道的导出里是 k 倍放大。`with_channel` 还会在已有列表上 `push`，**不检查重复** | 内部改用 `BTreeSet<u8>`（`ExportFilter` 是 pub 字段，序列化契约要注意）；或保留 `Vec` 但在 `includes_channel` 里先构造一次位图缓存 |
| 6 | 中 | `export.rs:353-355` | `vcd_id(ch)` 每次调用 `format!("ch{ch}")` → **每个值变化行一次 String 分配** | 满窗导出 = 80 万次堆分配。`render_csv` 的 `format!` 同理（每行一次中间 String 再 push 进 out） | `vcd_id` 改为静态表 `&'static str`（256 项预生成）或用栈上 `[u8; 4]` buffer；`render_csv`/`render_vcd` 改用 `write!(&mut out, ...)` 直接写入目标 String，避免中间分配 |
| 7 | 中 | `delta.rs:33,97-103` + `pipeline.rs:523,552` | `DeltaQueue.dropped` 无限增长，且**每个 chunk 被完整克隆两次** | `export()` 里 `self.delta.dropped_ranges().to_vec()`（`:523`）再 `meta.dropped_ranges` 移进 `ExportHandle`（`:552`）。传输持续失败的长跑场景下 dropped 列表无界增长，导出开销随之线性上升 | `dropped` 改为有界环形（保留最近 N 个洞）；或提供 `take_dropped_ranges()` 让消费方主动取走；至少避免每个 chunk 克隆全量 |
| 8 | 中 | `delta.rs:96-103` | `record_drop` 接受任意区间，**不校验与 `seq_no` 的关系** | 公共 API 允许调用方登记任意 `seq_start..seq_end`，包括超出已分配序列空间或与既有洞重叠的区间。消费方会据此向 M-07 请求补窗，可能补出不存在的数据 | `record_drop` 校验 `range.seq_start <= range.seq_end <= self.seq_no`，越界返回错误或断言；同时合并与既有洞相邻/重叠的区间 |
| 9 | 中 | `pipeline.rs:439-463` vs `:252-255` | 内存预算是**纯规划期检查**，而 `encode_edge` 的自动建环路径**完全绕过预算** | `reconcile_budget(channel_count, ring_cap)` 只检查「计划」；但 `encode_edge`（`:252-255`）对未声明通道会 `or_insert_with(|| ChannelRing::with_capacity(self.ring_cap))` 自动建环。宿主若喂入超出 `BUDGET_CHANNELS`（=4）的通道数，实际占用**静默突破 25.6 MB 预算**且永不触发 reconcile。文档（`:436-438`）承认这是规划期检查，但没承认自动建环这条旁路 | ① `encode_edge` 的自动建环改为走一个带预算检查的 `ensure_ring(ch)`；② 或在 `reconcile_budget` 里额外校验「当前已建环数 + 计划数 ≤ BUDGET_CHANNELS」；③ 或把 `MEM_BUDGET_BYTES` 改成基于实际 `rings.len()` 的动态检查 |
| 10 | 中 | `pipeline.rs:661-668` vs `:676-688` | `SharedPipeline::pipeline_mut()` 用 `borrow_mut()`（**会 panic**），而 `encode_edge` 用 `try_borrow_mut()`（永不 panic）——语义不对称 | 模块在 `SharedPipeline` 文档（`:631-634`）里明确学到「probe 热路径里 panic 不可恢复」，但宿主侧的 `pipeline_mut()` 仍是 `borrow_mut()`。宿主在持有 `pipeline()` 引用时调 `pipeline_mut()` 就会 panic。热路径安全了、控制面反而更脆 | `pipeline_mut()` 改用 `try_borrow_mut()` 并返回 `Result`，或在文档写明「调用方不得同时持有 `pipeline()` 引用」；两种都可，但**要与 `encode_edge` 的口径一致** |
| 11 | 低 | `export.rs:359-361` vs `channel.rs:179-185` | `kind_text` 与 `edge_kind_text` **逐字重复** | 两个函数体完全相同，`kind_text` 只是 `edge_kind_text` 的再导出 | 删掉 `kind_text`，调用方直接用 `edge_kind_text`（或反过来保留一个） |
| 12 | 低 | `mod.rs:69` | `WAVEFORM_FAMILY` 常量**定义后从未使用** | grep 全仓仅命中定义处。模块实际用的是 `ErrorFamily::Observability`（`channel.rs:170`）与 `ErrorFamily::Context`（`pipeline.rs:455`） | 删除常量，或让所有错误构造走它 |
| 13 | 低 | `contract/src/model.rs:374` vs `:474` | 契约层存在**两个近乎重复的边沿类型**：`Edge { t_ps, value }` 与 `EdgeEvent { t_ps, ch, value, kind }` | `Edge` 被 M-18 的 `edges: BTreeMap<u8, Vec<Edge>>` 与 `WaveformStream.edges` 使用；`EdgeEvent` 是 M-19 的实时流。M-18 因此**把同一条边沿存成两种形状**（一份进 `edges`，一份进 `EdgeSink`） | 形态区分有合理性（导出形态 vs 实时形态，model.rs:470-472 有说明），但可考虑 `Edge` 复用 `EdgeEvent`（`Edge` 加 `ch`/`kind` 字段 + `#[serde(default)]`），或明确文档化「两种形态并存」的理由 |
| 14 | 低 | `pipeline.rs:618-622` | `impl EdgeSink for WaveformPipeline` 丢弃 `EncodeOutcome` | `encode_edge` 返回 `{overflowed, pause_requested}`，但 trait 签名是 `fn encode_edge(&mut self, ev)` 返回 `()`。溢出信息只能通过 `overflow_count(ch)` / `pause_requests` 事后查询 | 契约层 `EdgeSink` 可考虑返回 `Result` 或加一个 `last_outcome()` 观察者方法；当前由 pipeline 自处理 pause hook，尚可接受 |
| 15 | 低 | `export.rs:107-114` vs `:118-135` | `ExportMeta.total` 与 `ExportHandle.total` 重复同一数值 | 两个结构体都带 `total`，构造时从同一个 `total` 变量填。消费方要记住该用哪个 | 保留 `ExportHandle.total`（对外契约），`ExportMeta` 仅供渲染函数内部使用 |
| 16 | 低 | `export.rs:216-219` | `t_start` 过滤用 `continue` 逐条跳过，窄时间窗 + 宽历史窗时是 O(n) 空转 | 合并流已按时间有序，一旦 `t >= start` 就不该再有更早的条目——可以提前 break。当前实现每跳一条都要做一次 heap pop/push | 在 `next_entry` 里维护「已越过 start」的标志位，之后不再走 `continue` 分支 |

---

## 三、跨模块欠账（台账已追踪，本报告不重复）

| 事项 | 状态 |
| --- | --- |
| **D-12（M-14 ↔ M-15 ↔ M-18 接线）** | **本轮实质推进**：M-18 新增 `set_edge_sink`（`registry.rs:604`），M-19 实现 `EdgeSink`（`pipeline.rs:618`）与 `SharedPipeline`（`:676`），并有**真实跨模块集成测试** `crates/kernel/tests/m18_to_m19_waveform.rs`（6 个用例，走真实 `Bus` → `ProbeRegistry` → `WaveformPipeline` → CSV/VCD）。**M-19 → M-15 侧（外设 `on_probe` override）仍未做** |
| **D-07（M-12 调度器调用 `Peripheral::tick`）** | 仍未闭环。M-19 的 `push_delta` 依赖 M-12 的周期回调，文档（`pipeline.rs:24-31`）明写「M-19 does not own a timer」——目前**没有任何调用方**驱动 `push_delta` |
| **M-19 → 前端（M-03 W1 订阅 / M-05 / M-07）** | `DeltaSink` 端口已定义，**无实现**。波形数据目前推不到浏览器 |
| **M-11 trace 导出** | 未审查；`TraceStream` 的 `drain_trace` 是其数据源 |

**M-19 模块内未发现其他阻断问题。** #1 之外的 15 项均可在模块内独立处理。

---

## 四、值得肯定的设计

| 点 | 说明 |
| --- | --- |
| **双源解耦** | `ChannelRing`（历史窗口，供导出与前端重绘）与 `DeltaQueue`（实时推送流）严格分离，与 M-18 把「总线 trace」和「边沿流」分开的思路一致。模块文档 §「Boundaries」把这条边界写得很清楚 |
| **序列号 + 显式 DropRange 补洞** | 序列空间 gap-free、失败批次记 `DropRange` 供 M-07 `resumeBySeq` 补窗——这是分布式推送的成熟做法（#2 指出的正是这条契约的一个漏洞） |
| **k-way 归并不materialize** | `MergedCursor` 用 `BinaryHeap<Reverse<(t_ps, ch, idx)>>` 做归并，**不materialize整个合并流**，满窗导出不需要额外 20 MB。同时间戳按通道号打破平局 → **输出确定性**（`export_is_deterministic_for_the_same_input` 有测试） |
| **溢出双策略 + 只读边界** | `DropOldestMark` / `PauseSim` 两策略都只丢最旧（不伪造数据），`PauseSim` 通过 `PauseHook` 请求暂停且**不碰寄存器/内存**——严格守住 M-19「read-only observation」约束（评审 19-9） |
| **不自带时钟** | 虚拟时间一律由 M-12 提供，M-19 无 wall-clock 依赖 → 确定性可重放（FR-12） |
| **诚实的不变量** | `ring.rs:95-99` 主动写明 `Vec::clone` 会缩小容量这个坑、`delta.rs:17-21` 主动写明「序列号本身检测不到丢失，所以要用 DropRange 显式记录」——**作者对边界的认识很清楚**，只是 #1 的断言恰好踩了自己写下的坑 |
| **错误码零新增** | 复用 `Observability`（配置错误）与 `Context`（资源上限），不破坏 M-0 冻结 |
| **真实端到端集成测试** | `crates/kernel/tests/m18_to_m19_waveform.rs` 刻意走跨模块真实路径而非直接调 `encode_edge`，并覆盖「总线访存进入 trace 但**不污染波形**」这一关键隔离性质 |

---

## 五、建议处理顺序

1. **#1 必修，且 M-18 同步修**：`TraceStream` 已在 staged 状态（本次提交会带进去），**在提交前修掉**，否则把一个已实证的 debug panic 写进历史。最小改动是删掉两处 `debug_assert_eq!`（`initial_cap` 已是唯一真源，断言前提是错的）。
2. **#2 必修**（契约违反）：`push_delta` 在无 sink 时记 `DropRange`，或显式建模「无消费者」语义并让消费方可查。**在 M-19 → 前端接线之前必须定下来**，否则接上线后消费方拿不到补洞信息。
3. **#3 必修**（NFR-04）：CSV/VCD 转义，属于「宿主可控文本进入导出文件」的安全边界。
4. **#4 建议在接线 PR 之前处理**：导出性能直接决定「N5 智能灯跑完能多快导出 VCD」的用户体感；1.3s 满窗在浏览器里是可感知的。
5. **#5~#16 合并为一次 `chore(m19)`**：位图化通道过滤、去 `vcd_id` 分配、`dropped` 有界化、`record_drop` 校验、预算旁路收口、`pipeline_mut` 口径统一、去重导出小函数。

---

## 六、审查边界声明

- 本报告覆盖 M-19 模块自身 + 契约层本轮增量（`EdgeSink` / `DeltaSink` / `EdgeEvent`）+ 集成测试。契约 `Probe` trait（死契约）与 M-15 report #10 同源，不在本报告重复计数。
- M-18 的 `TraceStream`（#1 同源缺陷）与 M-11 trace 导出未在本报告详审，但 #1 的修复必须与 M-18 同步进行。
- 门禁结论与 §一 实证输出均为 2026-10-19 实际执行结果；临时验证用例已删除，`crates/kernel/tests/` 下仅余 `m06_load_to_bus.rs` 与 `m18_to_m19_waveform.rs`。

---

## 七、闭环核对（2026-10-19 · PROGRESS v0.9.1）

本报告 16 项问题**全部闭环**，逐项落地表：

| # | 严重度 | 闭环 | 落地内容 |
| --- | --- | --- | --- |
| 1 | **阻断** | ✅ | 删掉 `ChannelRing::capacity()` 与 M-18 `TraceStream::capacity()` 里前提为假的 `debug_assert_eq!(buf.capacity(), initial_cap)`，改断言真正的不变量 `len <= initial_cap`。`initial_cap` 是环行数学的唯一真源（M-18 review #13 的规则），断言的报错文案「ring math would be incorrect」本身是事实错误——环行数学从未读 `buf.capacity()`。**M-18 同源缺陷同步修复**。回归测试 `clone_then_capacity_does_not_panic`（M-19 + M-18 各一条） |
| 2 | **高** | ✅ | 引入第三种交付结局并显式建模：「装了 sink 且投递成功」= 已送达；「装了 sink 但投递失败」= **内容丢失** → `dropped_ranges`（消费方据此走 M-07 `resumeBySeq` 补洞）；「**没装 sink**」= 内容交给**直接调用方**（返回值）→ `unconsumed_ranges`（可查：这些批次从未到达任何 sink）。旧实现把第三种当成功处理，批次被取走、序列号推进却什么都不记，违反模块自身声明的不变量。回归测试 `review_2_*` 两条 |
| 3 | **高** | ✅ | 新增 `csv_escape()`（RFC 4180：含 `,` `"` `\n` `\r` 时加引号、内部 `"` 翻倍）+ `vcd_label()`（VCD `$var` reference 的安全字符集；`$` / 空白 / 控制字符 → `_`，`.` `-` `[]` `/` 保留）。回归测试 `csv_escapes_host_supplied_labels` / `vcd_label_cannot_break_the_var_command` / `review_3_hostile_labels_do_not_break_the_export` |
| 4 | **高** | ✅ | 新增 `ExportSession`：会话持有归并游标，跨 chunk 不重建，**整次导出 O(n) 总代价**（旧实现每 chunk 从 0 重 skip，O(n²/chunk)）。`export_csv/export_vcd` 保留为「一次性 / 续导锚点」入口并标注代价模型。`MergedCursor::advances()` 暴露游标推进次数，回归测试 `review_4_sequential_export_is_linear_in_total_entries` 断言整次导出只推进 n 次 |
| 5 | 中 | ✅ | `ExportFilter::channels` 改 `ChannelSet`（256 位位图），`includes_channel` O(1)；位图天然去重（旧的 `Vec` 允许重复 `with_channel`）。测试 `channel_set_is_a_bitmask` |
| 6 | 中 | ✅ | 删掉 `vcd_id()`（每值变化行一次 `format!` 分配），`render_csv` / `render_vcd` 改 `write!` 直写目标 `String`，无中间分配 |
| 7 | 中 | ✅ | `dropped` 改有界 `VecDeque`（`DROPPED_HISTORY_CAP = 4096`）；`ExportMeta` 改**借用** `&[DropRange]`，每 chunk 只克隆一次（旧的每 chunk 克隆两遍） |
| 8 | 中 | ✅ | `record_drop` 改返 `Result`，校验区间不倒置且不越过已分配序列空间（否则消费方会向 M-07 请求不存在的数据）；相邻/重叠区间**合并**成一个连续洞。测试 `inverted_drop_range_is_rejected` / `drop_range_past_the_sequence_space_is_rejected` / `overlapping_drop_ranges_merge` |
| 9 | 中 | ✅ | 新增 `ensure_ring()`：自动建环前按**字节预算**校验（`已建环数+1 × ring_cap × 32B ≤ MEM_BUDGET_BYTES`），超预算则拒绝建环并计入 `rings_refused`；事件仍进实时推送流（只缺历史窗口）。旧实现自动建环完全绕过只有规划期才走的 `reconcile_budget`。测试 `review_9_automatic_ring_creation_respects_the_memory_budget` |
| 10 | 中 | ✅ | 新增 `try_pipeline()` / `try_pipeline_mut()`（返 `BorrowConflict` 不 panic），与热路径 `try_borrow_mut` 口径统一；`pipeline()` / `pipeline_mut()` 保留但文档明示会 panic 及何时该用 try 版。测试 `review_10_shared_pipeline_has_non_panicking_accessors` |
| 11 | 低 | ✅ | 删掉 `export::kind_text`（与 `channel::edge_kind_text` 逐字重复），统一用后者 |
| 12 | 低 | ✅ | `WAVEFORM_FAMILY` 现在被 `channel::invalid_config` 真正使用；另加 `WAVEFORM_BUDGET_FAMILY`（`ErrorFamily::Context`）供内存预算错误使用 |
| 13 | 低 | ✅ | 契约层 `Edge` 与 `EdgeEvent` 并存的理由**文档化**：前者是历史/导出形态（通道由所属 `WaveformStream` 隐含），后者是实时形态（自带通道号与边沿种类）。不合并——合并会改动已冻结类型并给历史形态塞入容器已隐含的字段 |
| 14 | 低 | ✅ | 新增 `WaveformPipeline::last_outcome()` 观察者方法（`EdgeSink` 端口签名返回 `()`，编码结果需另行取回）。测试 `review_14_last_outcome_exposes_the_encode_result` |
| 15 | 低 | ✅ | `ExportMeta` 去掉与 `ExportHandle` 重复的 `total` 字段；`total` 只由 `render_csv` 参数传入 |
| 16 | 低 | ✅ | `MergedCursor` 增加 `past_start` 标志位：归并流按时间有序，越过窗口起点后不再逐条重做 `t_start` 比较 |

**门禁**：`cargo test --workspace` = **203 passed / 0 failed**（lib 193 + integration 10；v0.9 = 181 → v0.9.1 = +22）；`clippy --workspace --all-targets -- -D warnings` 干净；`fmt --all -- --check` 干净。

**遗留（不属本报告范围）**：契约 `Probe` trait 仍是零实现的死契约（与 M-15 report #10 同源），留待 M-0 下一窗口统一清理。

