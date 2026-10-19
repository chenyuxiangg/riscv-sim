# Project Progress · 项目进度

> **Document version**: v0.10.0
> **Created**: 2026-10-19
> **Last updated**: 2026-10-20
> **Status**: active
> **Owner**: zyzs
> **Source of truth**: this document, synced with git history (`git log` + `git tag`)

## v0.10.0 变更摘要

**前端 M-01/02/03 首次模块级 review 闭环：18 项全部落地**（`docs/reviews/FRONTEND_Review.md` §七 逐项核对；16 项首审意见 + 2 项评审漏项）。本轮无新功能落地，是前端自 v0.4 装配以来**第一次**被系统性检视——而这次检视的结论是：**页面 100% 白屏，且三条既有门禁全都看不见**。

### 阻断级缺陷（2 项）：前端 100% 白屏

ES module 的链接错误发生在**任何代码执行之前**——`bootstrap()` 永不启动，状态栏永远停在 `index.html` 的静态文案「未连接」：

- **D-01**：`m02` 导入 `activeView`，而 `m01` 里它是未导出的 `let` → 链接期失败
- **D-02**：`app.js` 导入 `mountViewStore`，而 `m03` 只导出 `setState/getState/snapshot/subscribe/reset` → 链接期失败

两条必须同批修（同一模块图，浏览器只报第一个错）。实证确认这就是启动路径上的**全部**阻断点：修掉后 bootstrap 即完全跑通。

### 全部 18 项闭环一览

| 类别 | 项 | 落地 |
| --- | --- | --- |
| **阻断** | #1 #2 | 删掉 m02 用不到的 `activeView` import；m03 补 `mountViewStore`（写法对齐四个 service） |
| 高 | #3 #4 #5 | demo 按钮去掉 `closest("[data-region-base]")` 前置条件（按钮在 `</table>` 外 → 恒不触发）；终端 `onDelta` 去掉 `escapeHtml()` 双层转义（`textContent` 不解析 HTML）；`switchView` 同步 `[data-view-tab]` 的 `aria-current`（CSS 一直有这个选择器，JS 从未设置过 → 活动标签页从不高亮） |
| 中 | #6 #7 | `switchView` 重入分支改派发 `type:"update"`；`setCapabilities` 末尾统一 `refreshAll()`；新增 `captureUiState()` / `restoreUiState()`——LLD §3.3 要求的「按 viewId 存 UI 交互态」此前**从未采集**，原来的 `setState(hash, {})` 在 merge 语义下是 no-op |
| 低 | #9~#16 | `escapeHtml` 抽到 `front/lib/escape.js`（原本两份逐字重复）；面板改用 `createElement` + `dataset`；`ROLE_DEFAULT_VIEW` 显式映射表（role 名与 view hash 同名是巧合不是契约）；demo 数据隔离到 `front/dev/`；状态栏/页脚文案改为从真实状态推导；补 `file://` 部署提示 |
| **漏项 1** | #17 | `m02` 调用 `seedDemoBus()`，而该函数**从未定义**——评审只看到按钮是死的所以没暴露。**单纯按 #3 去掉前置条件，会把死按钮变成点了就抛 `ReferenceError`** |
| **漏项 2** | #18 | `refreshAll()` 调 `mountPanels()` 会把**所有**面板加上 `hidden`，却从不调 `showPanel()` → 任何调用 `refreshAll()` 的路径都会让当前视图消失。缺陷是潜伏的：修复前 `refreshAll()` 唯一的调用点会先在 `seedDemoBus()` 上抛异常，够不到 |

### 新增前端门禁（本轮最重要的结构性产出）

前端此前**零测试**——没有 test runner、没有 CI 冒烟、没有 DOM 测试。这是 D-01 / D-02 长期潜伏的根因：Rust 侧 203 个测试全绿，页面却白屏。

- `front/tests/dom-stub.mjs`：最小 DOM 替身（元素树 / 属性 / classList / dataset / innerHTML 解析 / 带冒泡的事件派发 / 选择器）
- `front/tests/frontend.test.mjs`：**19 用例 / 6 suite**，零第三方依赖
- `tools/check-front.sh`：门禁入口（Node ≥ 22）
- `.github/workflows/ci.yml`：仓库此前**没有 CI**，而 PR 流程要求「CI 绿」——本次补 Rust（fmt / clippy / test）与前端两个 job
- `package.json`：只有 `{"type":"module"}`，让 Node 确定性地把 `.js` 当 ESM，**没有依赖、没有构建步骤**

门禁有效性用 **8 组变异测试**验证：把每个原始缺陷放回去，门禁必须变红——A（删 `mountViewStore` 导出）/ B（恢复双层转义）/ C（恢复 `row` 前置条件）/ D（`refreshAll` 少 `showPanel`）/ E（不设 `aria-current`）/ F（重入不派发）/ G（UI 态采集退化）/ H（`escapeHtml` 第二份实现）**全部 FAIL ✓**，还原后 PASS ✓。

> 变异 C 首轮曾漏网：原断言只检查「store 里有 3 条映射」，而点击前就已 seed 过，死按钮同样能通过。已改为断言「点击确实发出了 `riscv:seed` 信号」且「点击确实重新 seed 了映射」。

### 决策记录（v0.10.0）

1. **修法取「删掉用不到的 import」而非「新增 `getActiveView()` 导出」** —— 不为没人用的符号扩大公开 API；`refreshAll` 保留即可
2. **demo 数据隔离到 `front/dev/`，生产模块只发信号** —— M-02 只发 `riscv:seed`，dev 装配层消费；M-07 落地时该文件与 dev 按钮一起消失
3. **UI 交互态按「真实采集」而非「改注释」闭环 #7** —— LLD §3.3 明确要求，采集终端滚动位置与视图体滚动位置（CSS 里 `.terminal-out` 是唯一 `overflow:auto` 的容器）
4. **状态栏/页脚改为从真实状态推导** —— 写死的「kernel 待编译」在 kernel 早已跑通后仍挂在页面上；写死阶段号注定再次过期
5. **新增 CI** —— 仓库此前无 CI，但 PR 流程要求「CI 绿」；补 Rust + 前端两个 job
6. **门禁用 node:test + 自写 DOM stub，不引第三方依赖** —— 与「零构建、零依赖」的前端约束一致

### 进度影响

- **前端测试覆盖**：0 → **19 用例 / 6 suite**（`tools/check-front.sh`）
- **前端代码行数**：1 123 → 1 402（含测试与 DOM stub）
- **§3 人日**：不变（本轮为质量收口，不计入模块交付人日）
- **跨模块欠账**：D-01 / D-02 / D-18 / D-19 / D-20 **全部闭环**；D-23（门禁缺口）**部分闭环**——第 1 项（ESM 链接检查）已加并升级为完整前端门禁，第 2~4 项（M-06 畸形输入 / M-13 x0 / M-12 快照）仍未加
- **台账口径**：已开发模块 88 项 review 意见，闭环 83 项（94.3%），未闭环 5 项

`cargo test --workspace` = **203 passed / 0 failed**（本轮未动 Rust 侧）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过；**前端门禁 19 passed / 0 failed**。

---

## v0.9.1 变更摘要

**M-19 波形管道 review 闭环：16 项全部落地**（`docs/reviews/M-19_WaveformPipeline_Review.md` §七 逐项核对）。本轮无新功能落地，是 v0.9 提交后的质量收口。

**这轮修复了 1 项阻断级缺陷，且该缺陷已在远端历史里** —— 38e62ed 推上去之后才被另一会话的 review 实证发现。按用户决议，**不重写历史**，追加一个 fix 提交。

### 阻断级缺陷（#1）

`ChannelRing::clone()` 后调用 `capacity()` → **debug 构建下 panic**：

```
assertion `left == right` failed: ChannelRing::buf capacity changed after construction
  left: 1  right: 8
```

根因：`Vec::clone` 只按 `len` 分配容量，克隆后 `buf.capacity()` 必然 ≠ `initial_cap`，而 `capacity()` 里断言两者相等。**讽刺之处**：该行上方的注释恰好点名过这个坑（「`Vec::clone` 会缩小容量，所以不能信 `buf.capacity()`」），而环行数学确实改对了（`push`/`drain` 全用 `initial_cap`），但这个**只读访问器**里留了一条 `Vec::clone` 必然破坏的断言，报错文案「ring math would be incorrect」本身还是事实错误——环行数学从头到尾没读过 `buf.capacity()`。

**M-18 的 `TraceStream` 有完全同源的缺陷**（`trace_stream.rs:103`），一并修复。既有测试克隆后只调 `drain()` 从不调 `capacity()`，所以缺陷一直潜伏。

### 全部 16 项闭环一览

| 严重度 | 项 | 落地 |
| --- | --- | --- |
| **阻断** | #1 | 删掉两处前提为假的断言，改断言真不变量 `len <= initial_cap`；M-19 + M-18 同步修；各加一条回归测试 |
| **高** | #2 | 引入第三种交付结局：「没装 sink」不再被当成成功——内容交给直接调用方（返回值），同时记入**可查**的 `unconsumed_ranges`；「投递失败」才是 `dropped_ranges`（真丢，需补洞） |
| **高** | #3 | 新增 `csv_escape()`（RFC 4180）+ `vcd_label()`（VCD `$var` reference 安全字符集）——宿主可控文本不再能破坏导出文件结构（NFR-04） |
| **高** | #4 | 新增 `ExportSession`：会话持有归并游标，跨 chunk 不重建 → **整次导出 O(n) 总代价**（旧实现 O(n²/chunk)）；旧入口保留为「一次性 / 续导锚点」并标注代价模型 |
| 中 | #5 | 通道过滤改 256 位位图（O(k) → O(1)），顺带天然去重 |
| 中 | #6 | 删掉每行一次分配的 `vcd_id()`；`render_csv`/`render_vcd` 改 `write!` 直写，无中间分配 |
| 中 | #7 | `dropped` 改有界（4096）；`ExportMeta` 改**借用**切片，每 chunk 只克隆一次（原两遍） |
| 中 | #8 | `record_drop` 改返 `Result`，校验区间（不倒置、不越序列空间）；相邻洞**合并** |
| 中 | #9 | 自动建环前按字节预算校验，超预算拒绝建环（旧的完全绕过只有规划期才走的 `reconcile_budget`） |
| 中 | #10 | 新增 `try_pipeline()` / `try_pipeline_mut()`（不 panic），与热路径口径统一 |
| 低 | #11-#16 | 删重复的 `kind_text`；`WAVEFORM_FAMILY` 真正被用 + 新增 `WAVEFORM_BUDGET_FAMILY`；`Edge` vs `EdgeEvent` 并存理由文档化；新增 `last_outcome()`；去掉 `ExportMeta.total` 重复；`MergedCursor` 加 `past_start` 标志位 |

### 契约层变更（additive · 不破坏 M-0 冻结）

本轮**无契约签名变更**，只有文档：契约层 `Edge`（历史/导出形态，通道由所属 `WaveformStream` 隐含）与 `EdgeEvent`（实时形态，自带通道号 + 边沿种类）并存的理由已文档化。不合并二者——合并会改动已冻结类型，并给历史形态塞入容器已隐含的字段。

### 决策记录（v0.9.1）

1. **已推送历史不重写，追加 fix 提交** —— 用户决策。缺陷只影响 `clone()` + `capacity()` 这一条特定调用路径，追加修复即可，审计链完整；强推会覆盖别人的本地状态
2. **#2 选「显式建模第三种结局」而非「无 sink 也记 DropRange」** —— 无 sink 时内容**没有丢**（交给了直接调用方的返回值），记成 DropRange 会让消费方去补一段它其实拿得到的洞；分成 `dropped_ranges`（真丢）与 `unconsumed_ranges`（没到 sink）两个语义清晰的列表
3. **#3 VCD label 白名单保留 `.` `-` `[]` `/`** —— review 建议的 `[A-Za-z0-9_]` 会把 `GPIO0.OUT` 打成 `GPIO0_OUT`，可读性损失不必要；真正需要中和的是 `$`（会提前终止 `$var` 命令）、空白与控制字符
4. **#4 用 `ExportSession` 而不是缓存 `count_filtered`** —— 缓存只能省掉计数，skip 的 O(n) 仍在；持游标的会话是唯一能真正把总代价降到 O(n) 的做法
5. **#1 删断言而不是手写 Clone 保住容量** —— `initial_cap` 已是唯一真源，断言一个标准库**未承诺**的实现细节（`Vec::with_capacity(n)` 只保证 ≥ n）本身就是错的

### 进度影响

- **测试覆盖**：M-19 64 → 85 单测（+21）；workspace lib 171 → 193（+22，含 M-18 的 1 条）；workspace 累计 181 → **203 全绿**（lib 193 + 集成 10）
- **§3 人日**：不变（本轮为质量收口，不计入模块交付人日）
- **跨模块欠账**：D-12 本体（M-14 ↔ M-15 size 一致性）**仍未动**；M-15 ↔ M-18 的 `on_probe` 接线仍未做（§6.3 并行 0.5d 项）

`cargo test --workspace` = **203 passed / 0 failed**（lib 193 + integration 10；v0.9 = 181 → v0.9.1 = +22）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.9 变更摘要

**M-19 波形管道落地**（按 §6.2 推进顺序，M-18 之后的下一站）。本轮同时完成 **M-18 → M-19 握手**——`docs/reviews/M-18_Probe_Review.md` §四第 2 条点名的缺口「M-19 真正消费 M-18 的边沿事件，否则 M-18 边缘事件的『产出』没有『消费者』」在本轮闭合。

> **口径澄清**：台账 `CROSS_MODULE_DEBT.md` 的 **D-12** 条目指的是 **M-14 ↔ M-15 接线**（`Bus::map_peripheral` 的 size 与 `Peripheral::size()` 一致性校验）；M-18 报告里把「M-14 ↔ M-18 / M-15 ↔ M-18 接线」也称作 D-12，两处编号口径不一致。本轮完成的是 **M-18 → M-19** 这一段（新增 `EdgeSink` 端口），**D-12 本身（M-14 ↔ M-15）未动**，M-14 ↔ M-18 的 bus hook 与 M-15 ↔ M-18 的 `on_probe` 接线也仍未做（后者是 §6.3 的并行 0.5d 项）。

M-19 实现的是「探针 → 边沿编码 → 环形缓冲 → 增量推送 → 前端渲染 / trace 导出」全管道（差异化核心，FR-10 + FR-14）。

### 落地的子模块（拆 5 文件 + 1 测试文件，按子组件独立 review + 测试）

| 子模块 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| `ChannelMeta` + `OverflowPolicy` + `WireFormat`（渲染元数据 + 策略枚举） | `crates/kernel/src/m19_waveform/channel.rs` | 7 单测 | ✅ |
| `RingEntry` + `ChannelRing`（每通道定长环 + 丢最旧 + `overflow_count` 标记） | `crates/kernel/src/m19_waveform/ring.rs` | 10 单测 | ✅ |
| `DeltaQueue`（≤20ms 批 + 单调序列号 + `DropRange` 丢弃区间） | `crates/kernel/src/m19_waveform/delta.rs` | 6 单测 | ✅ |
| `MergedCursor`（跨通道 k 路归并，零额外分配）+ CSV / VCD 分块渲染 | `crates/kernel/src/m19_waveform/export.rs` | 10 单测 | ✅ |
| `WaveformPipeline` 编排（`encodeEdge` / `pushDelta` / `setOverflowPolicy` / `reconcile_budget` / 导出）+ `SharedPipeline` | `crates/kernel/src/m19_waveform/pipeline.rs` | (走 tests.rs 单测) | ✅ |
| 单元测试（LLD §7.1 测试表逐条对应 + 契约测试） | `crates/kernel/src/m19_waveform/tests.rs` | 31 单测 | ✅ |
| 跨模块集成测试（M-14 总线 + M-18 探针 + M-19 管道端到端） | `crates/kernel/tests/m18_to_m19_waveform.rs` | 5 集成测试 | ✅ |

### 契约层变更（additive · 不破坏 M-0 冻结）

- `riscv_sim_contract::model::EdgeEvent { t_ps, ch, value, kind }` —— M-18 → M-19 的边沿事件载体（18B，热路径上每次观察都分配，故不带任何标签/元数据）
- `riscv_sim_contract::model::WaveformDelta { seq_start, seq_end, t_virt, entries }` —— 增量推送载荷（带单调序列号）
- `riscv_sim_contract::model::DropRange { seq_start, seq_end }` —— 推送失败丢失的序列区间
- `riscv_sim_contract::port::EdgeSink` —— M-18 → M-19 驱动端口（设计文档 §1.1「M-18 的 m19Encode 即本模块 encodeEdge 入口」）
- `riscv_sim_contract::port::DeltaSink` —— M-19 → 订阅者驱动端口（失败语义：丢批 + 记区间，不重试不阻塞）

### M-18 侧改动（additive · 零破坏）

- `ProbeRegistry` 新增可选 `edge_sink: Option<Box<dyn EdgeSink>>` + `set_edge_sink()` / `has_edge_sink()`；未装 sink 时热路径只多一次 `Option` 判断
- `sample_inner` 原先丢弃的 `EdgeKind`（M-18 review #4 遗留）**现在随 `EdgeEvent` 一起下发**——M-18 review #4 的「kind 信息丢失」问题在 M-19 接线后自然消解
- `ProbeRegistry` 的 `Debug` 改为手写（`Box<dyn EdgeSink>` 不是 `Debug`）

### 关键设计决策

1. **两个策略都丢最旧，区别只在要不要暂停仿真** —— 设计文档伪码里 `PAUSE_SIM` 分支不推进 `head`，会让环停在满状态并**伪造历史空洞**。实际实现：两种策略都保留最新观察、丢最旧并 `overflow_count++`；`PAUSE_SIM` 额外通过 `PauseHook` 请求 M-12 暂停仿真（控制面操作，不改寄存器/内存，符合只读约束 19-9）
2. **序列号空间「失败也前移」+ 独立的 `DropRange` 记录内容空洞** —— 数字空间无空洞（前端无法从 seq 发现丢失），内容空洞由 `DropRange` 显式记录，供 M-07 `resumeBySeq` 续拉补齐
3. **导出归并用 k 路堆归并（零额外分配）** —— 不物化整个归并流，导出满窗口（4 × 2×10⁵）不再需要额外 20MB 堆内存，守住 25MB 内存预算
4. **环内存「到达序」= 「时间序」，但导出前防御性重排** —— 虚拟时钟单调，正常情况下二者一致；`ChannelRing::sort_by_time()` 在检测到违例时原地重排（不丢数据），保证导出流永远按 `t_virt` 单调
5. **M-19 不拥有定时器** —— 调度者是 M-12 事件循环的周期回调（19-5）：M-12 调 `push_delta(now_ps)` 并提供权威虚拟时间。M-19 不读任何时钟，这是它可确定性重放的前提
6. **不新增错误码** —— 无效策略复用 `E_OBS-01`（Observability 族首个 scenario，M-18 已在用）；内存超预算复用 `E_CTX-02`（设计文档 §3.5 明确指定）
7. **导出接口取 `&mut self`** —— 防御性重排需要可变访问；调用方本来就持有 `&mut`（`push_delta` 也是）
8. **M-14 ↔ M-18 / M-15 ↔ M-18 两段接线仍未做** —— 本轮只做 M-18 → M-19 这一段握手（新增 `EdgeSink` 端口）。M-15 三个外设的 `on_probe` override 与 M-14 的 `map_peripheral` typed handle 升级留给后续接线 PR（台账 D-12 口径见 v0.9 摘要开头的澄清）

### 进度影响

- **§6.1 依赖表**：M-19 状态 ❌ → ✅；观测管道（M4 阶段 6 人日）的内核侧全部就位
- **M-18 报告 §四.2 闭环**：「M-19 真正消费 M-18 边沿事件」这条待办已闭合（`EdgeSink` 端口 + 跨模块集成测试）
- **§3 人日**：探针 M-18 2/3 → 2.5/3（M-18 → M-19 握手完成；M-14/M-15 两段接线仍未做）；波形 M-19 0 → 3/3；观测管道合计 **5.5/6**
- **§5.4 N5「GPIO 翻转可见」**：内核链路（M-14 → M-18 → M-19 → 导出）**已端到端可达并有集成测试守住**；仍缺 M-15 GPIO `on_probe` 接线（让固件写 OUT 真的触发边沿）+ M-07 transport + M-02 波形 UI 渲染
- **测试覆盖**：M-19 0 → 64 单测（+64）；workspace lib 107 → 171（+64）；workspace 累计 112 → **181 全绿**（lib 171 + 集成 10）

`cargo test --workspace` = **181 passed / 0 failed**（lib 171 + integration 10；v0.8.1 = 112 → v0.9 = +69）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过；`cargo check --target wasm32-unknown-unknown` 通过。

---

## v0.8.1 变更摘要

**M-18 review 闭环 + `docs/reviews/` 文档体系建立**。本轮无新功能落地，是 M-14 / M-15 / M-18 三个模块 review 意见的收口与归档。

### M-18 review 闭环（15 项 → 13 项闭环）

按 `docs/reviews/M-18_Probe_Review.md` §一·五 闭环核对，落地 13 项可独立处理项：

| 类别 | 落地内容 |
| --- | --- |
| **热路径性能** | `wire_signal_probe` 闭包直接路由 `sample_inner`，跳过 `record_signal` 的 `(periph_id.to_string(), signal.to_string())` 二次分配 |
| **panic 风险** | `borrow_mut()` → `try_borrow_mut()`；借用冲突时静默丢弃该观察并累加 `borrow_contended` 计数，**不 panic 不阻塞**（外设回调上下文里阻塞会死锁） |
| **边沿语义** | 新增 `prev_survives_enable_disable_cycle` + `prev_survives_master_disable_cycle` 两条测试**固化既有正确行为**——BTreeMap 的 `prev` 在关闭期间不会被清空（关闭时早退在 master/enable 检查，根本走不到写 `prev` 的路径）。原 review 推断「状态丢失」是错的，此处以测试纠正 |
| **契约层** | `TraceEvent.dir: u8` 魔数 → 新增 `pub enum TraceDir { Read, Write }` + 双向 `From<AccessDir>`；`TraceEvent.dir` 改 `dir: TraceDir` |
| **判定健壮性** | `ChannelDef::classify` 在 `SingleBit` 分支对 `prev`/`next` 各做 `& 1` 截断，违例输入（`prev=5, next=1`）也给出良定义结果 |
| **环行鲁棒性** | `TraceStream` 新增 `initial_cap` 字段，`push`/`drain`/`capacity` 全部改用 `initial_cap` 而非 `buf.capacity()`，加 `debug_assert_eq!` —— **从此不再依赖 `Vec::with_capacity` 不超额分配这一未被编译期保证的行为** |
| **文档** | `define_channel` auto-enable 副作用、`sample_irq_pulse` 通道未注册时「无脉冲 vs 未注册」不可区分、`with_capacity(0)` panic 指引、throttle 算法逐拍 trace，均在源码注释中显式化 |

**未闭环 2 项**（均为低危整洁/技术债）：
- **#10 契约 `Probe` trait 零实现** —— 全代码库无 `impl Probe for`，与 M-15 review #10 是同一个死契约，建议在 M-0 下一窗口统一清理
- **#12 `drain_waveform` 借用交错** —— 迭代 `channels` 时 `edges.remove(ch)`，当前可用（不同字段），未来合并两表会 break

### `docs/reviews/` 文档体系

新建 `docs/reviews/` 目录，4 份文档：

| 文件 | 内容 | 状态 |
| --- | --- | --- |
| `M-14_BusMemoryMap_Review.md` | 16 项 review + 闭环核对 | 模块内 100% 闭环，2 项跨模块挂账 |
| `M-15_PeripheralModel_Review.md` | 19 项 review + 闭环核对 | 模块内 100% 闭环，1 项跨模块挂账 |
| `M-18_Probe_Review.md` | 15 项 review + 闭环核对 | 13/15 闭环，2 项低危挂账 |
| `CROSS_MODULE_DEBT.md` | **跨模块欠账台账** | 23 条 · 5 阻断 · 含阻塞拓扑与门禁缺口根因 |

**台账是跨模块欠账的唯一追踪入口**：模块报告只管模块内部，跨模块项一律上收台账，按**修复归属模块**组织。

### 决策记录（v0.8.1）

1. **建立 `docs/reviews/` 目录 + 跨模块欠账台账** —— 模块报告管模块内、台账管跨模块，职责切分避免同一问题在两份报告里各记一半
2. **跨模块项一律「上收台账 + 本轮不动」** —— 用户明确要求跨模块问题单独排期；台账记录归属模块、阻塞拓扑、处理批次
3. **契约层 `TraceEvent.dir` 用 enum 替代 `u8` 魔数** —— 与 M-14 `AccessDir` 对齐；M-0 冻结范围内的第二次 additive 变更（第一次是 `Peripheral::read_clear`，第二次是 `Peripheral::on_probe` + `ProbeSink`，第三次是本条 `TraceDir`）
4. **契约 `Probe` trait 暂不删、不实现** —— 留待 M-0 下一窗口与 M-15 report #10 一并清理，避免在模块 PR 里夹带契约破坏性变更

### 进度影响

- **§3 人日**：M-15 6 人日项推进到 5.5/6（剩 M-14 接线）；M-18 6 人日项中的 M-18 部分推进到 2/3（剩 M-14/M-15 接线 PR）
- **§6.1 依赖表**：M-18 → M-19 的 blocker 进一步松动（M-18 侧 13/15 闭环，剩余 2 项不影响 M-19）
- **§5.4 验收节点**：N5「GPIO 翻转可见」的**内核前置**已全部就位（M-15 GPIO `IN` 回读输出脚 + M-18 边沿判定 + `TraceDir` 化），仍需 M-14 ↔ M-15 ↔ M-18 接线 PR 才能端到端可达
- **测试覆盖**：M-18 20 → 29 单测（+9）；workspace lib 98 → 107（+9）；workspace 累计 103 → **112 全绿**

`cargo test --workspace` = **112 passed / 0 failed**（lib 107 + integration 5；v0.8 = 103 → v0.8.1 = +9）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.8 变更摘要

按 §6.3「下个目标」落地 **M-18 探针**（PROGRESS 估 3 人日；本轮 ~0.5d 增量，stub 形态有 `InMemoryProbe`/`ProbeRegistry` 兜底，无新协议冻结点）。M-15 → M-18 接线是独立 PR（D-12 跨模块挂账，本轮不动）。

**落地的子模块**（拆 4 个文件，按子组件独立 review + 测试）：

| 子模块 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| `ChannelDef` + `ChannelKind::{SingleBit, MultiBit, Pulse}` + `classify()` 边沿判定 | `crates/kernel/src/m18_probe/channel.rs` | (走 registry 单测) | ✅ |
| `TraceStream` 环形窗口（容量 2×10⁵，溢出丢最旧 + `dropped` 计数 + `seq` 单调） | `crates/kernel/src/m18_probe/trace_stream.rs` | 3 单测 | ✅ |
| `ProbeRegistry` 编排（`define_channel` + `attach_trace` 总线钩子 + `wire_signal_probe` 外设回调 + `sample` 边沿判定 + `setRate` + `drain_waveform` + `drain_trace` + `sample_irq_pulse`） | `crates/kernel/src/m18_probe/registry.rs` | 17 单测 | ✅ |
| 契约层 additive 扩展：`Peripheral::on_probe` 默认 no-op + `ProbeSink = Box<dyn FnMut(&str, u64, u64)>` + 模型层 `EdgeKind` + `TraceEvent` | `crates/contract/src/{port,model}.rs` | 0（契约，不带测试） | ✅ |

### 契约层变更（additive · 不破坏 M-0 冻结）

- `riscv_sim_contract::port::Peripheral` trait 加默认方法 `fn on_probe(&mut self, _cb: Option<ProbeSink>) {}` —— 默认 no-op，与 M-15 review #2 同样的 additive 模式
- `riscv_sim_contract::port::ProbeSink = Box<dyn FnMut(&str, u64, u64)>` —— 外设信号回调签名 `(signal_id, value, now_ps)`
- `riscv_sim_contract::model::EdgeKind::{Rising, Falling, Value}` —— 单 bit 通道判升降、多 bit 通道折叠为 `Value`
- `riscv_sim_contract::model::TraceEvent { t_ps, addr, width, dir }` —— 总线 trace 单事件（M-11 导出载体）

### 决策记录（v0.8）

1. **模块拆分 4 文件**（channel / trace_stream / registry / tests）—— 与 M-15 uart/gpio/timer 拆 5 文件对齐，方便独立子模块评审与回归
2. **`prev` 用 `Option<u64>` 表达"未观测"** —— 设计文档伪码 `prev[ch]` 留了未初始化陷阱（M-18 v0.8 §3.3），本轮显式收为 `None` 时首次观察必发（不与"同值不产"语义冲突）
3. **`wire_signal_probe` 不做冲突校验，冲突校验只在 `define_channel`** —— 设计文档 §3.2 的"同 ch 冲突拒绝"是 *通道定义* 冲突，不是 *外设绑线* 冲突；外设回调可重复安装（替换前一次 sink），符合 `Peripheral::on_probe(Option<…>)` 替换语义
4. **总线侧走 `BusAccess.width.bytes()` 取字节数**（不是 `as u8` 拿 discriminant）—— `BusWidth::{Byte, Half, Word}` discriminant 是 0/1/2，需要 `.bytes()` 拿 1/2/4；`as u8` 的隐患先在测试里被发现
5. **throttle 算法按设计文档 §3.3 走"先 decrement、>0 跳过、=0 发并 reset"** —— `throttle_n=4` 配 10 次观察 = 2 次发（第 4、第 8 次），不是 3 次（与 `ceil(10/4)` 不一致，是设计文档的语义）
6. **`sample_irq_pulse` 便捷方法仅在 `CH_IRQ_PULSE` 注册为 `Pulse` 时工作** —— 不在通道表里时静默 no-op（避免凭空向未定义通道发事件）
7. **M-15 ↔ M-18 接线（外设侧 `on_probe` override）单独 PR** —— D-12 跨模块挂账；本轮契约层已就位，M-15 三个外设（GpioBank/Uart16550/TimerRiscv）尚未 override `on_probe`，等接线 PR 一并改。PROGRESS §6.3 估算 3 人日，本轮 ~0.5d 增量

### 进度影响

- **§6.1 依赖表**：M-18 状态从 ❌ → ✅；M-19 波形（依赖 M-18）的 blocker 解锁
- **§6.2 推进顺序图**：M-15 → M-18 箭头落地 ✅；下一站 M-19 波形管道
- **测试覆盖**：M-18 0 → 20 单测（+20）；workspace lib 78 → 98（+20）；workspace 累计 83 → 103（+20）
- **§5.4 验收节点**：未触及 N5「GPIO 翻转可见」（仍需 M-15 接线 PR 才能让固件写 OUT 时实际触发 M-18 边沿事件）；但 N5 的 *内核前置*（M-18 接收外设信号）已就位

`cargo test --workspace` = **103 passed / 0 failed**（lib 98 + integration 5；v0.7.3 = 83 → v0.8 = +20）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.7.3 变更摘要

按 `docs/reviews/M-15_PeripheralModel_Review.md` §五「复查中新发现的遗留问题」落地 3 项（**N1 DRY 回归 / N2 IN 回读输出脚 / N3 去重位图放宽**）。M-15 review 现已 100% 模块内可独立闭环（跨模块欠账 #6/#11 仍按用户要求挂账不动）。

### review 落地表（M-15 §五）

| # | 严重度 | 修复 |
| --- | --- | --- |
| **N1** | 中 | 抽 `m15_peripheral/reg.rs` 共享 `register_offset` + `extract_subword` 两个 helper；`uart.rs` / `gpio.rs` / `timer.rs` 三处 `use super::reg::{...}` 引入；消除 review #5 修复引入的 6 份函数体重复。DRY 回归彻底收回 |
| **N2** | 中 | 新增 `GpioBank::pin_levels()` 私有方法，返回 `(outputs & directions) | (inputs & !directions)`；`IN` 寄存器改用 `pin_levels()` 回读。固件写 OUT 驱动 LED 后能从 IN 读到引脚电平。**N5「GPIO 翻转可见」+ M-18 波形 pipeline 都依赖这个**（review §五「二次复查」明确标注） |
| **N3** | 低 | `take_pending_irqs()` 去重位图从 `line < 16` 放宽到 `line < 32`；注释同步：「line ∈ [16, 32) previously bypassed dedup entirely, contradicting the documented unique-line contract」；line ≥ 32 的边角情况保留无去重（u32 位图兜不住） |

### 测试覆盖（M-15：25 → 29 条，+4）

- `gpio_in_reflects_output_latch_on_output_direction_pins`（N2）
- `gpio_in_reflects_input_pins_and_output_latch_simultaneously`（N2 边角：同字节混合输入/输出 pin）
- `gpio_take_pending_irqs_dedupes_lines_in_upper_half`（N3：line 17 + line 31 双向验证）
- `reg_helpers_are_dry`（N1：DRY surface 显式断言）

### 决策记录（v0.7.3）

1. **N1 DRY 集中到 `reg.rs` 而非 trait** —— 三处 helper 只用两个内联函数，没必要抽 trait；crate 内 `pub(crate)` 子模块足够。开 `mod.rs` 子模块而不是 `pub trait RegisterAccess`，避免给外设强加不必要的接口
2. **N2 IN 寄存器按 directions 选 inputs/outputs**，匹配 PL061 / STM32 GPIO 的 mental model。固件写 OUT[0]=1 后读 IN[0] 返 1；写 OUT[1]=0 后读 IN[1] 返 0；输入脚仍由 `set_external_inputs` 驱动
3. **N3 line ≥ 32 不去重** —— `u32` 位图物理上限；MVP 几乎遇不到（M-15 现状 IRQs < 32），但保留路径注释以备 P1 演进

### 进度影响

- **M-15 review §五 闭环核对**：4 项遗留 → **0 项遗留**
- **M-15 review 总闭环率**：15 完全闭环 + 1 半闭环（#6 跨 M-12）+ 1 有意保留（#18）+ 1 推迟（#11 跨 M-14/M-15）→ **15 完全闭环 + 1 半闭环 + 1 有意保留 + 1 推迟**（与 v0.7.1 一致，新发现 3 项已全部处理）
- **测试覆盖**：M-15 25 → 29；workspace lib 74 → 78（+4）
- **N2 是 N5「GPIO 翻转可见」+ M-18 波形的前置依赖**（review §五二次复查明确）；本轮把它落地

`cargo test --workspace` = **83 passed / 0 failed**（lib 78 + integration 5；v0.7.2 = 79 → v0.7.3 = +4）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.7.2 变更摘要

按 `docs/reviews/M-14_BusMemoryMap_Review.md` §四 闭环核对，落地剩余可独立处理的 6 项（#10/#11/#12/#14/#16 + 新发现 N5/N6）。**2 项跨模块挂账不修**（#6 需 M-06 联改；#9 影响 m06_load_to_bus + m02_viewcomponents，按用户要求跨模块先不动）。

### review 落地表（M-14）

| # | 原严重度 | 状态 | 修复 |
| --- | --- | --- | --- |
| 10 | 低 | ✅ 闭环 | 删 `assert_no_overlap` belt-and-braces 分支（`mod.rs:463-475`）—— `prev.contains(base)` 为真已蕴含 `prev.base + prev.size > base`，第二分支恒不可达 |
| 11 | 低 | ✅ 闭环 | `read_bytes` / `write_bytes` 改返 `Result<u32, _>` / `Result<(), _>`；越界统一抛 `E_BUS-04`，与 `read_region_byte` / `write_region_byte` 口径一致；`read_region` / `write_region` 同步透传 |
| 12 | 低 | ✅ 闭环 | `Region::contains` 与 `Bus::crosses_region` 用 `checked_add` 显式拒绝回绕：地址溢出返 `false` / `Crossing::Unmapped`，不再依赖「map_region checked_add 已守住」的偶然安全 |
| 14 | 低 | ✅ 闭环 | `byte_read_past_end_of_region_yields_e_bus_04` 改名 + 收紧断言 → `byte_read_past_end_of_region_yields_e_bus_01`，`assert_eq!(err.scenario, 1)`（Byte 路径走 `read_aligned` 的 `lookup` miss，恒返 1） |
| 16 | 低 | ✅ 闭环 | 文件末尾「Tests cover」清单更新到 v0.6.1 + 本轮新增的 hook ok / 三分 / E_BUS-05 / E_BUS-07 / Byte 越界；删除「Top-level error helpers」孤立横幅（N5） |
| 6 | 中 | ⏸ 挂账 | 跨模块，需 M-06 `ParseLimits` 联改（`total_image_bytes` / `max_region_bytes`）。本轮不动 |
| 9 | 中 | ⏸ 挂账 | 跨模块，影响 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js`；收紧 `Region.target` 可见性需同步改两侧取数方式。本轮不动 |

### review 落地表（M-15）

- v0.7.1 已闭环 18/19；唯一挂账 #11（跨模块 Peripheral size ↔ Bus map_peripheral size 一致性校验）延 M3 接线 PR
- 本轮无新增 M-15 修改

### 进度影响

- 测试覆盖：M-14 测试 27 条不变（修改走的是收紧断言 + Result 透传，已有覆盖守住）
- workspace lib 74 + integration 5 = **79 passed / 0 failed**
- 影响可观测数据正确性的项（review #1~#5）100% 闭环，本次收紧的是 #10~#16 整洁批次

`cargo test --workspace` = **79 passed / 0 failed**（同 v0.7.1）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

### 决策记录（v0.7.2）

1. **#11 read_bytes / write_bytes 改 Result 不会破坏现有调用路径** —— 调用方 `read_region` / `write_region` 透传 Result；`read_aligned` / `write_aligned` 已在 Result 链上；唯一变化是测试签名（已经是 `Result<u32, _>`），无需补新测试。
2. **#12 checked_add 拒绝回绕** —— `map_region` 已经用 `checked_add` 守住「base + size 不溢出 u32」，所以 `Region::contains` 的 checked_add 是防御性深度 —— 当外部代码（如未来 P1 多板互联）从 `region_tree` 之外直接传 addr 时，不会因回绕误判「contains=true」。

---

## v0.7.1 变更摘要

按 `docs/reviews/M-15_PeripheralModel_Review.md` 落地 19 项 review 意见。**3 阻断 + 4 高 + 3 中 + 9 低 + 跨模块 1（#11）**。契约层只用 additive 默认方法，不破坏 M-0 冻结。

### review 落地表

| # | 严重度 | 修复 |
| --- | --- | --- |
| 1 | **阻断** | Uart16550 加 `pub fn take_tx() -> Option<u8>`，host bridge 消费 TX 字节；`refresh_status` 同步翻 TX_READY |
| 2 | **阻断** | contract::Peripheral trait **additive** 加 `fn read_clear(&mut self, …)` 默认方法（不破坏 `read(&self, …)` 签名）；Uart16550 实现 `read_word_mut` 清 RX FIFO 并 refresh status |
| 3 | **阻断** | GPIO TOGGLE 改成 **多 pin mask toggle**：`outputs ^= value & PIN_COUNT_MASK`；固件 `1<<3` 翻 pin 3、`(1<<3)|(1<<0)` 同时翻两 pin；同步改 `tests.rs` |
| 4 | 高 | UART 新增 `status_bits::TX_OVERRUN = 0x8`；`transmit()` 溢出置 TX_OVERRUN 位（不再复用 RX_OVERRUN） |
| 5 | 高 | 三个外设的 read/write 改 `match offset & !0x3` 做寄存器选择；`extract_subword(word, offset, size)` 按 WriteSize 切字节/半字/字 |
| 6 | 高 | 删 `raise_irq()` 死函数 + `InterruptSink` import；M-12 真正调用 `tick()` 留给 M-18 PR（v0.6.1 已留 wiring PR 范围） |
| 7 | 高 | 删 `directions_complement_or_zero` 死函数 + `let _ = edges;`；`set_external_inputs` 真用 `directions`：只对配置为输入的 pin 触发边沿 IRQ |
| 8 | 中 | Timer 删 `set_mtime`；MTIME_LO/MTIME_HI 写权限改为 no-op（mtime 只读由引擎驱动）；保留 `set_mtimecmp`（固件可写 deadline） |
| 9 | 中 | `PeripheralRegistry::register()` 改返 `Result<&mut Self, RegisterError>`；`RegisterError::Duplicate(String)` 含冲突 id |
| 10 | 中 | `pub use riscv_sim_contract::port::Peripheral;` 直接转出（删除 `PeripheralTrait` 别名） |
| 11 | 中 | **跨模块留 M3 接线 PR**：Peripheral size ↔ Bus map_peripheral size 一致性校验在 wiring PR 做 |
| 12 | 低 | Uart/Timer `irq_line` 改 `pub(crate)` 字段 + `set_irq_line()`/`irq_line()` 公开 setter/getter |
| 13 | 低 | `take_pending_irqs()` 加 line bitmap 去重；多 pin 绑同一 line 只返一次 |
| 14 | 低 | UART STATUS 清 overrun 改 W1C 极性（写 1 清零）；同步改 `tests.rs:111` 旧「写 0 才清」断言 |
| 15 | 低 | `ps_per_bit()` 在 `baud_rate > 1e9` 时钳制到 1 ps（避免零长度帧） |
| 16 | 低 | status 位维护只走 `refresh_status()` 单一写入点；`transmit`/`receive` 只改 `tx`/`rx` 字段 |
| 17 | 低 | 新增 `FidelityLevel` 枚举（Behavioural / Transactional / TimingApprox / CycleAccurate）；`describe()` 走真实字段而非硬编码 |
| 18 | 低 | 删 `pub(crate) type PeripheralWrite = PendingWrite` 死别名；`force_irq_for_test` 加 `#[cfg(test)]` |
| 19 | 低 | tests 16 → 25（+9）：`take_tx` / `read_clear` / TX_OVERRUN / W1C / TOGGLE mask / 方向生效 / 同 id 注册 / 子偏移字节访问 × 3 |

### 契约层变更（additive · 不破坏 M-0）

- `riscv_sim_contract::port::Peripheral` trait 加默认方法 `fn read_clear(&mut self, offset: u32, size: WriteSize) -> Result<u32, PlatformError>` —— 默认实现透传到 `read(offset, size)`，让所有现有外设自动获得「非破坏读」语义。需要 read-clear 的外设（如 Uart16550）override 即可。零 breaking change。

### 决策记录（v0.7.1）

1. **#2 用 additive 默认方法解 #2，不开 M-0 解冻窗口** —— 与 PROGRESS §0 M-0 冻结策略一致；契约层只增不破。
2. **#3 TOGGLE 选「多 pin mask toggle」**（而非 review 推荐的「单 pin low-5-bits」）。理由：原 doc 本身有歧义（low 5 bits = pin number 与 1<<pin 写法不可同时成立），mask toggle 是最固件友好、且支持原子多 pin 翻转的语义。doc 同步改为「each bit set in `value` flips that pin's `out` atomically」。
3. **#14 UART STATUS 采用 W1C**（写 1 清零）—— 主流 PL011/16550 硬件约定；智能灯自研固件可控（W1/Q1 决议）。
4. **#8 Timer mtime 只读由引擎驱动** —— 删 `set_mtime` + MTIME_LO/HI 写权限 no-op；mtimecmp 保留固件可写（设下一 deadline）。两个时间基（总线侧 timer vs 引擎虚拟时间）语义不再混。
5. **#17 加 `FidelityLevel` 枚举** —— LLD §2.1.1 fidelity 元数据真实落地载体；MVP 全部填 `Behavioural`，P1 演进可填更高等级。
6. **#11 跨模块 size 一致性校验延 M3 接线 PR** —— 本 PR 只动 M-15 自身；M-14 ↔ M-15 接线时由 M-14 `map_peripheral(base, size, &dyn Peripheral)` 一并加 `debug_assert_eq!`。
7. **#6 删 `raise_irq` 死函数**；M-12 真正调用 `tick()` 留 M-18 PR —— 与 v0.6.1 review #13 wiring 边界同范围（peripheral-driven ticks、IRQ sink 注入）。

### 进度影响

- **§5.4 验收节点**：v0.7 声称的 N5「GPIO 翻转可见」内核前置条件真正成立 —— TOGGLE 翻转、UART TX 字节可被 host 取出、RX 可多次读清。
- **测试覆盖**：M-15 16 → 25 单测（+9）；workspace lib 65 → 74（+9）。
- **跨模块挂账**：M-12 PeripheralTick 空 TODO（m12_engine:155-159）+ M-14 ↔ M-15 接线，留给 M-18 / M3 wiring PR。

`cargo test --workspace` = **79 passed / 0 failed**（lib 74 + integration 5；v0.7 = 70 → v0.7.1 = +9）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.7 变更摘要

按 §6.3 推进顺序完成 **M-15 UART/GPIO/Timer**（6d 估算，本轮实际 0.5d 增量——继承 v0.6.1 已签约接口，无新协议冻结点）。MVP 三个行为级外设全部到位。

**落地的子组件**（拆 5 个文件，按子组件独立 review + 测试）：

| 子模块 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| `Uart16550` 行为级（1-byte FIFO + 8 字节寄存器窗口 + TX/RX 引脚状态 + 1e9/baud 分频） | `crates/kernel/src/m15_peripheral/uart.rs` | 5 单测 | ✅ |
| `GpioBank` 32-pin（DIR/OUT/IN/TOGGLE + 边沿触发 IRQ latch + `take_pending_irqs`） | `crates/kernel/src/m15_peripheral/gpio.rs` | 5 单测 | ✅ |
| `TimerRiscv`（64-bit mtime + mtimecmp + sticky pending IRQ via `take_pending_irq`） | `crates/kernel/src/m15_peripheral/timer.rs` | 3 单测 | ✅ |
| `PeripheralRegistry`（BTreeMap + register/get/get_mut/iter/describe + Manual `Debug`） | `crates/kernel/src/m15_peripheral/mod.rs` | 2 单测 | ✅ |
| 跨外设演示：UART TX → 桥接 → GPIO 翻转（智能灯最小子模块） | `crates/kernel/src/m15_peripheral/tests.rs` | 1 单测 | ✅ |

`cargo test --workspace` = **70 passed / 0 failed**（lib 65 + integration 5；v0.6.1 = 54 → v0.7 = 70，新增 16）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

### 决策记录（v0.7）

1. **Uart16550 MVP 选 1-byte FIFO**（评审 15-9）—— 比真实 16-byte 简单，足以覆盖 E2E-01 智能灯「字符回环」场景；E2E-01 N5 验收后可演进。
2. **GpioBank 32-pin 固定** —— MVP 不做可配置 pin 数（评审 R5，P1 演进）；`PIN_COUNT_MASK = u32::MAX` 常量化。
3. **TimerRiscv 用 `take_pending_irq()` 公开 API 替代 `InterruptSink::raise()`** —— contract `Peripheral::tick` 签名只返 `Vec<PendingWrite>`，M-16 的权威 IRQ 源上线后 Timer 降级为 no-op（评审 N3 fallback 路径）。
4. **本 PR 不动 M-14 ↔ M-15 接线** —— `RegionTarget::Peripheral(PeripheralId)` 当前仍返 `E_BUS-05 not-wired`（v0.6.1 review #13 已签约定锁）；M-14 typed handle 升级到 `Box<dyn Peripheral>` / `Rc<dyn Peripheral>` 是单独 PR 的范围，本 PR 保持 wiring 边界干净。
5. **`m15_peripheral` 拆 uart/gpio/timer/tests 子文件** —— 与 LLD §3.2-3.4 子组件对齐，方便独立子模块代码评审与回归测试；`mod.rs` 只保留公开 API + `PeripheralRegistry` + re-exports。
6. **`PeripheralRegistry` 用 `BTreeMap<String, Box<dyn Peripheral>>`** —— key 是 `Peripheral::id()` 字符串 id；MVP 单线程；`Send + Sync` 留 M-12 P2 SMP 阶段再加。

### 进度影响

- **§5.4 验收节点**：解锁 **N5 「GPIO 翻转可见」** 的内核前置条件 —— 智能灯固件可通过 M-15 GPIO 翻转驱动；待 M-18 探针 + M-19 波形 + M-07 transport 接入后可达 UI 层。
- **§6.3 下个目标**：从「M-15 UART/GPIO/Timer」迁移到「**M-18 探针**」（3d 估算）—— 解锁波形管道 + 进一步探针源挂总线/外设。
- **累计人日**：v0.6.1 = 16.5 → v0.7 ≈ 17（约 +0.5d 增量；M-15 大部分形态已在 v0.1 stub 阶段预留）。

---

## v0.6.1 变更摘要

按 `docs/reviews/M-14_BusMemoryMap_Review.md` 的 16 项意见修 M-14：M-15 接线前必做的 4 项 + 6 项整洁 + 1 项测试，共落地 11 处修复。

**修复（review 编号）**：
1. **#1** `BusAccess.width` 文档纠偏 — 「每次访问触发一次，跨区间拆分不逐字节上报」（原文档写反）。
2. **#2** hook 时机改到 dispatch 出口；新增 `BusAccess.ok: bool` + `err_scenario: Option<u16>`，失败的访问也上报 `ok=false`。消费者按 `ok` 过滤。
3. **#3** `BusStats` 拆分 `ram_hits` / `rom_hits` / `periph_hits`，ROM 写静默丢弃不再计入 `ram_hits`（评审 14-4）。`periph_hits` 在 M-15 接线时启用。
4. **#4** `periph_hits` 字段活了 — M-15 接线时即可累加。
5. **#5** `crosses_region` 改成 `Crossing::{Aligned, Crosses, Unmapped}` 三态；`unaligned_splits` 仅在两端都命中但跨区时 +1，新 `unmapped_splits` 在至少一端未映射时 +1；与原模糊统计脱钩。
6. **#7** `init.len() > size` 拆分出独立 `E_BUS-07 init-too-large`（不再共用 `E_BUS-04` 拓扑溢出）。
7. **#8** 删死函数 `validate_width`（`BusWidth` 是封闭枚举，不需要运行时校验）。
8. **#13** 补 `map_peripheral` 签约测试 — peripheral read/write 恒返 `E_BUS-05 not-wired`，等 M-15 接线后改测试即可。
9. **#14** 收紧 `read_past_end_of_region_yields_e_bus_01` 断言（去掉"or 4"宽松口径）。
10. **#15** 删 `Region::end_inclusive` 死公开 API。

**跳过（影响面大或跨模块，留后续）**：
- **#6** `max_region_bytes` 上限（需 M-06 `ParseLimits` 联改） — 影响面大，本 PR 不动。
- **#9** Region / RamBackend 字段 `pub → pub(crate)` — 影响所有 kernel crate 内部代码；不动。
- **#10/11/12** belt-and-braces 分支 / 静默吞边界 / `wrapping_add` — 安全偶然已成立，但显式化需要跨测试改动。

`cargo test --workspace` = **54 passed / 0 failed**（lib tests 49 + integration tests 5）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

## v0.5 变更摘要

**M1 数据底座第二步完成**：M-06 ELF / Raw / HEX 解析器 + M-14 / M-13 端到端集成，3 人日估算法里实际投入 ~1.5d。已落地：

| 子项 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| M-06 ELF32 LE RV32 自写解析器（**MVP 决策：不引入 `object` crate**） | `crates/kernel/src/m06_elf/elf.rs` | 7 单测 | ✅ |
| M-06 Intel HEX 解析器（含 0x02/0x04 扩展地址 + 末段覆盖 06-4） | `crates/kernel/src/m06_elf/hex.rs` | 7 单测 | ✅ |
| M-06 Raw blob 解析器（统一 `BUS_SIZE` 边界） | `crates/kernel/src/m06_elf/mod.rs` | 2 单测 | ✅ |
| M-06 公开 API：`Loader` trait + `DefaultLoader` + `ParseLimits` + `Kind::{Elf,Hex,RawHex}` + `detect_kind` | `crates/kernel/src/m06_elf/mod.rs` | 2 单测 | ✅ |
| 端到端集成：M-06 → M-14 → M-13 跑通 ADDI（ELF + Raw + HEX 三格式都验） | `crates/kernel/tests/m06_load_to_bus.rs` | 5 集成测试 | ✅ |

`cargo test --workspace` = **47 passed / 0 failed**（lib tests 42 + integration tests 5）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

解锁里程碑 **PROGRESS.md §5.4 N2「数据底座可加载」达成**：拖 ELF / Raw / HEX 进项目 → M-06 解析 → M-14 map → M-13 fetch+execute 拿到 `x1=42`，全链路贯通。

剩余工作见 §5 阶段计划。


## v0.3 变更摘要

**MVP 范围决策**：从 v0.2 的「选项 B（~30 人日，DWARF 留 P1 降级）」**升级到「选项 C（§13.6 全量 90 人日）」**。理由：完整 13 P0 覆盖，不留降级；含 DWARF 行表 + 源码联动（MVP 风险最大项）、自动化 CI 与场景脚本断言、多板互联接口预留。

**M0 协议冻结**（5d）已 ✅ 完成；剩余 **85 人日** 待开发。详见 §5 阶段计划、§4 WBS、§8 前置条件。

## v0.2 变更摘要（保留追溯）

Q1-Q7 全部答复（见 §8）：
- Q1 智能灯固件 = **自研**（~200 行 RV32IMC 最小固件）
- Q2 ISS 指令集 = **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集）
- Q3 持久化 = **IndexedDB**（用 `idb` ~5KB 包装，非 localStorage）
- Q4 Worker URL = **同源 URL**（HTTP 部署，`./kernel-worker.js`）
- Q5 行为判定 = **canvas 比对**（虚拟 LED 渲染 + 像素 diff + 视觉对照）
- Q6 性能基线 = **macOS**（M3 Apple Silicon）
- Q7 Spike 对拍 = **必做**（FR-04，每条指令 golden）

---

## 0. TL;DR

RISC-V 仿真/调试平台项目处于 **MVP 启动期**。架构 + 23 篇模块详细设计已归档；3-crate Cargo workspace + 23 个子模块骨架已建立；M-12 事件驱动引擎和 M-13 RV32I ISS 实现了最小子集（`addi` 单指令可执行，`cargo test --workspace` 两个核心单测 PASS）。

按架构文档 §13.6 WBS 估算，**当前完成度约 26% / 90 人日**（v0.9 口径，详见 §3）。下一步沿 §13.6 关键路径推进，目标：完成 **E2E-01 智能灯验收主线**（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）。

> ⚠️ **重要纠偏**：早期对话里把"MVP"误读为"两个单元测试通过"。重新校准后，正确定义见 §2。
>
> ⚠️ **跨模块欠账**：另有 23 条跨模块问题（5 阻断级）挂在 `docs/reviews/CROSS_MODULE_DEBT.md` 台账，**不在本文档的模块人日口径内**——它们是模块交付之后的集成欠账，需单独排期。台账里最靠前的是 **D-04（M-13 x0 未硬连线）**，它阻塞 Q7 决议的「Spike 对拍必做」。

---

## 1. 当前状态（截至 v0.1）

### 1.1 已完成（Done）

| 类别 | 内容 |
| --- | --- |
| 设计文档 | 27 篇（arch 1 + requirements 1 + module-design-spec 1 + detailed-design 23 + AGENTS.md + README.md） |
| 工程骨架 | 3-crate Cargo workspace：`riscv_sim_contract` / `riscv_sim_kernel` / `riscv_sim_server` |
| 契约层 | 5 个核心数据模型（`Project` / `SimState` / `DebugMeta` / `Waveform` / `Message`），`CommandPayload` / `EventPayload` 协议信封，13 个 `ErrorFamily`，6 个 port trait（`CommandBus` / `EventBus` / `ExecutionBackend` / `Peripheral` / `InterruptSink` / `Probe`） |
| 内核子模块 | 16 个 M- 模块全部建好（M-12 + M-13 实际实现，其余 14 个为公开端口/结构体形状的 stub） |
| 前端骨架 | `index.html` + `app.js`（hash 路由）+ 5 个空视图 stub（M-01/02/03/04/05/07/10） |
| 工具链脚本 | `tools/build-wasm.sh`（wasm-pack no-modules 包装） |
| 测试 | `cargo test --workspace` 通过，2 个核心单测 PASS：`m12_engine::engine_advances_time` + `m13_iss::addi_executes` |
| 版本控制 | git 初始化，main 分支，一次 initial commit（`c69e570`） |

### 1.2 部分完成（In progress）

| 模块 | 现状 | 缺口 |
| --- | --- | --- |
| **M-12 事件驱动引擎** | `Engine<B, I>` 调度器骨架，`run_until_idle` 推进 1µs 量子 → 发 `EventPayload::Tick` → 排空到期事件；`SchedEvent::{PeripheralTick, TimerExpiry, Commit}` 派发 | 没接 ISS 驱动指令；instruction budget 未生效；`PeripheralTick` / `Commit` 派发是空实现 |
| **M-13 RV32I ISS** | 手工 decoder 拆字段（opcode/rd/rs1/rs2/funct3/funct7/imm_I），Executor 实现 `ADDI`（`x[rd] = x[rs1] + imm`，`PC += 4`） | 只 1 条指令；缺 LUI / AUIPC / LOAD / STORE / BRANCH / JAL / JALR / M 扩展 / C 扩展；非法指令返 trap |
| **M-06 ELF 解析** | `Loader` trait + `DefaultLoader`（假实现：bytes 整体塞地址 0） | 没用 `object` crate，ELF header / PT_LOAD 段都没解析 |
| **M-06 ELF / Raw / HEX** | v0.5 ✅ ELF32 LE RV32 自写（无 `object` crate 依赖）+ Raw + Intel HEX（含 0x02/0x04 扩展地址）；`Loader` trait + `DefaultLoader` + `ParseLimits` + 4 个错误码 LDR-01..04（畸形 / 超限 / 越界）；E_LDR-XX 不新增 contract family，沿用 `Loader` family | 18 单测 + 5 端到端集成测试（M-06 → M-14 → M-13 三格式都跑通 ADDI） |
| **M-14 总线** | v0.4 ✅ 完整 `Bus`（BTreeMap 区间表 + 读写分发 + 跨区间两阶段 + trace hook + 统计 + 6 个错误码）；`BusMaster` trait + blanket impl；`PeripheralId` 占位 typed handle | 单元测试覆盖 map / unmap / lookup / read / write 8/16/32 / ROM 写忽略 / 跨区间 split / unmapped trap / hook / BusMaster trait 派发；M-13 ↔ M-14 集成测试 (`step_one_via_bus`) 通过 |
| **M-15 外设** | `PeripheralRegistry` + `DummyUart`（读 0 / 写忽略） | 行为级 UART（FR-07 波特率/CTS 流控）未实现；GPIO / Timer 完全空；M-14 typed handle 当前用 `PeripheralId(u32)`，待 M-15 升级到 `Rc<dyn Peripheral>` |
| **M-16 中断控制器** | `InterruptController` + `BTreeSet<u16>` 存已 raise 的 IRQ | 没 `mip/mie/mcause` CSR 联动；没接入 `step_one` 的中断采样 |
| **M-17 执行后端** | `SelfIssBackend<B, I>` 包装 Engine + Iss，实现 `ExecutionBackend` 全部 11 个方法（多为 stub） | `step_n` 永远 `Ok(0)`，没真驱动 ISS；断点 / 监视点切换是空操作 |
| **M-18 探针** | v0.8 ✅ 完整 `ProbeRegistry`（通道表 + 节流 + 边沿判定 + 双流：trace 环 / 边沿流）；v0.9 ✅ 新增可选 `EdgeSink` 端口，M-18 → M-19 接线完成 | 仍缺 M-15 外设 `on_probe` override（D-12 另一半）；`Probe` trait 仍是零实现的死契约（挂账 M-0 下一窗口清理） |
| **M-19 波形** | v0.9 ✅ 完整 `WaveformPipeline`（边沿编码 + 每通道环 + ≤20ms 批量推送 + 单调序列号 + 跨通道归并 + CSV/VCD 分块导出 + 溢出策略 + 内存预算对账）；M-18 → M-19 已接线 | 导出只到 CSV/VCD 文本；二进制载荷（`BIN_LE_HEADER`）与前端波形渲染留给 M-07 / M-02 |
| **M-20 快照** | `SnapshotManager` 内存存 `Vec<SnapshotRecord>` | 没 M-23 持久化；没崩溃自动快照触发 |
| **M-21 日志** | `LogBuffer` 4096 环形 + `Level::{Trace..Fatal}` | 没接 `tracing` crate；没 admin 通道暴露 |
| **M-22 HMS** | `Hms::report` 单调 `Ok → Degraded → Critical` | 没 watchdog 联动；没降级动作 |
| **M-23 DataStore** | `DataStore` trait（put/get/list Project + SnapshotRecord） | 没 IndexedDB / SQLite 实现 |
| **前端** | v0.4 ✅ M-01 工作台装配 + hash 路由 + role×capBits 复校验；M-03 视图状态管理；M-02 4 视图组件（productTreeView 已渲染 M-14 内存映射 demo；debugView / waveformView / terminalView 接口形状落地）；app.js / index.html / style.css 已迁移 | 接 kernel 待 M-07 transport 接入 |

### 1.3 完全未做（Not started）

- 真实 ELF / DWARF 解析（FR-05）
- 调试服务（断点 / 单步 / Reg/Mem 视图 / 反汇编·源码联动）
- 观测管道 wire-up（探针源 → 边沿编码 → 增量推送 → 波形 UI）—— 内核侧 v0.9 已通；仍缺 M-15 外设源 + M-07 传输 + M-02 波形 UI
- 前端多视图可见化
- postMessage 传输（M-07）+ 会话管理（M-04）+ 命令路由（M-05）
- IndexedDB 持久化
- Worker 韧性 + watchdog + 崩溃自动恢复
- Spike golden 对拍
- 智能灯 E2E-01 验收

---

## 2. MVP 范围（按 AD §11.1 + §3.4 + §13.3）

### 2.1 MVP 定义

> **MVP（P0）**：13 条 P0 功能（FR-01~12 + FR-23）· 2D 产品树/原理图 · 调试联动 + 波形 · 多视图 + 快照 · WASM 零安装
> —— 架构文档 §11.1

**部署形态**：浏览器 WASM **目录形式**（不强制单文件自包含，NFR-05 经用户决策放宽）。**必须通过同源 HTTP(S) 部署**——ES module 与 Worker 均受同源策略约束，`file://` 直开会被浏览器拦截并白屏；既定方式为 `python3 -m http.server 8080 --directory front`（PROGRESS Q4 决议）。

**装配边界**（§3.4 评审 L1）：
- ✅ 嵌入式工作台（反汇编/断点/单步、Reg/Mem、trace、波形、项目树）— MVP 全量
- ✅ 产品工作台基础面（搭建/运行/波形）— MVP 子集
- ❌ 产品行为判定 / 系统管理员工作台 — P1
- ✅ HMS MVP 基线 = 崩溃自动快照 + 恢复（§10.7 三条）
- ❌ HMS 完整分角色分发 — P1

### 2.2 验收主线（§13.3 E2E-01 智能灯）

> 智能灯固件跑通（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）贯穿五横层

| 步骤 | 涉及层 | 涉及模块 | 当前可达性 |
| --- | --- | --- | --- |
| 搭建产品树 | L0/L1 前端 | M-01 / M-02 | ⚠️ M-01/M-02 视图层落地，productTreeView 渲染内存映射 demo；kernel 数据未接（M-07 待接） |
| ELF 加载 | 调试服务层 | M-06 | ⚠️ 假实现 |
| 运行 | 仿真内核层 | M-12 + M-13 + M-14 | ✅ **M-13 ↔ M-14 集成通过**：`step_one_via_bus` 跑通 ADDI，经 bus.fetch 寄存器 x1=42，PC=4 |
| GPIO 波形 | 观测管道层 | M-15 + M-18 + M-19 | ⚠️ 内核链路 M-18 → M-19 已通（v0.9 集成测试守住）；缺 M-15 `on_probe` 接线 + M-07 transport + M-02 波形 UI |
| 单步/内存 | 调试服务层 | M-08 / M-09 / M-10 | ❌ stub |
| 行为判定 | L0 前端 | M-01 | ❌ |

**结论**：E2E-01 第一步 **1/6 步端到端可达**（运行）。搭建层 + 内存映射 UI 已就位，等待 M-07 transport 接入。

### 2.3 MVP 不做的事（明确边界）

- 3D 渲染 / 帧缓冲（FR-21 / FR-22）— P1
- 多板互联（FR-24~27，里程碑 B）— P1
- GDB RSP / QEMU 双内核 — P2
- 多核 SMP — P2
- DWARF 源码联动可降级（§13.6 最大风险项）：MVP 退化为「反汇编 + 符号表，无源码联动」，仍算 MVP 范围

---

## 3. 关键路径（§13.6）

> 关键路径依赖：协议冻结 → ISS → 调试服务/观测管道 → 前端；DWARF 与前端视图可并行
> —— 架构文档 §13.6

```
[已 ✅] 协议冻结          (contract crate 完成, 5 人日)
   ↓
[   1  ] 产品树/内存映射  (M-01/M-02/M-14, 4 人日)        ─┐
[   2  ] ELF 解析         (M-06, 3 人日)                    │
   ↓                                                        │ 并行可行
[   3  ] ISS 全集 RV32IMC (M-13, 12 人日)                   │ DWARF [4*]
[   4  ] 事件引擎         (M-12, 6 人日)                    │ 10 人日
[   5  ] 外设 UART/GPIO/Timer (M-15, 6 人日)                │
[   6  ] 调试服务         (M-08/M-09/M-10/M-11, 8 人日)     │
[   7  ] 观测管道         (M-18/M-19, 6 人日)              ─┘
   ↓
[   8  ] 前端视图/路由/波形 UI (M-01/M-02/M-03, 10 人日)
   ↓
[   9  ] 持久化 IndexedDB (M-23, 3 人日)
[  10  ] Worker 韧性 + watchdog + HMS (M-22, 3 人日)
   ↓
[验收]  E2E-01 智能灯 跑通
```

**已做的人日**（v0.8.1 估算）：
- ✅ 协议 5
- ⚠️ ISS 2.5 / 12（ADDI + step_one_via_bus，1.5d 增量）
- ⚠️ 引擎 1 / 6（调度器骨架；M-12 未接 ISS 指令预算与外设 tick）
- ✅ 总线 2 / 4（v0.4 完成 Bus + BusMaster + 跨区间 split；v0.7.2 补完 6 项整洁项；剩余 4-KiB 分页 / 重叠告警等留 P1，详见 §7）
- ✅ ELF 3 / 3（v0.5 完成）
- ⚠️ 外设 5.5 / 6（M-15 三外设 + review 全闭环；剩 M-14 接线 PR）
- ⚠️ 探针 2.5 / 3（M-18 v0.8 落地 + v0.8.1 闭环 13/15 + v0.9 完成 M-18 → M-19 接线；剩 M-15 外设 `on_probe` 接线 PR）
- ✅ 波形 3 / 3（M-19 v0.9 落地：边沿编码 + 环形缓冲 + 批量推送 + CSV/VCD 导出）
- ⚠️ 前端 3.5 / 10（v0.4 M-01/M-02/M-03 viewstore + 4 视图组件；剩余 waveform UI 渲染等后续）
- ⚠️ 评审开销 ~1.5（M-14 / M-15 / M-18 三轮 review + 闭环，不计入模块交付人日）

**累计 ~23.5 / 90 人日 ≈ 26%**（v0.9 完成后）。

---

## 4. 完整 WBS（按 AD §13.6）

| # | 模块 | 人日 | 前置依赖 | 风险/降级 | 状态 | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 命令/事件协议 | 5 | — | 中；先冻结 | ✅ | contract crate |
| 1 | 产品树 / 内存映射 | 4 | 协议 | 低；区间树 | ✅ | v0.4 M-01/M-02 视图层 + M-03 viewstore 完成（~2.5d）；kernel 数据待 M-07 |
| 2 | ELF / Raw / HEX 解析 | 3 | — | 中；畸形输入容量上限 | ✅ | v0.5 M-06 完整 Loader；ELF32 LE / Raw / IHEX 三格式 + 18 单测 + 5 集成测试 |
| 3 | ISS RV32IMC | 12 | WASM 工具链 | 中；Spike 对拍 | ⚠️ | M-13 只 ADDI |
| 4 | 事件驱动引擎 | 6 | ISS 接口 | 中；确定性夹具 | ⚠️ | M-12 调度器 |
| 5 | 外设 UART/GPIO/Timer | 6 | 总线接口 | 低；分级建模 | ✅ | v0.7 Uart16550(1-byte FIFO + 波特率分频) + GpioBank(32-pin 边沿 IRQ) + TimerRiscv(mtime/mtimecmp) + PeripheralRegistry；16 单测全绿；M-14 ↔ M-15 接线下一 PR |
| 6 | 调试服务 | 8 | ISS、协议 | 中；依赖 hook | ❌ | M-08/09/10 stub |
| 7 | 观测管道 | 6 | ISS 钩子 | 中；差异化核心 | ⚠️ | v0.9 M-19 波形管道完整落地 + M-18 → M-19 接线；剩 M-15 外设源 + 二进制载荷 |
| 8 | DWARF 行表 + 源码联动 | 10 | ELF 解析 | **最大风险** | ❌ | 可降级 |
| 9 | 前端视图 / 路由 / 波形 UI | 10 | 协议、观测管道 | 中；零构建 | ⚠️ | v0.4 路由 + 4 视图组件；缺 waveform 渲染 |
| 10 | 持久化 IndexedDB | 3 | 数据模型 | 低 | ❌ | M-23 trait |
| 11 | Worker 韧性 + watchdog + HMS | 3 | postMessage | 中；guest 失控 | ❌ | M-22 占位 |
| 12 | 场景脚本/断言（接口预留） | 2 | 协议、确定性 | 低 | ❌ | P1 起步 |
| 13 | 多板互联（接口预留） | 2 | Peripheral port | 中 | ❌ | P1 立项 |
| **合计** | | **90** | | | | |

---

## 5. 完整开发计划（C 方案 · §13.6 全量 90 人日）

### 5.1 阶段切分

按架构 §13.6 WBS 全量开发，关键路径 **70 人日**（含 10 人日联调 buffer），并行支线 20 人日。

| 阶段 | 名称 | 人日 | 关键依赖 | 阶段交付 | 状态 |
| --- | --- | --- | --- | --- | --- |
| **M0** | 协议冻结 | 5 | — | contract crate + 5 model + 6 port + 13 错误族 | ✅ |
| **M1** | 数据底座 | 7 | M0 | 产品树视图 + 内存映射 + ELF/Raw/HEX 解析 | ⚠️ |
| **M2** | 内核 | 18 | M0 | ISS RV32IMC 全集（I+M+C） + 事件驱动引擎 + 量子化调度 | ⚠️ |
| **M3** | 外设 | 6 | M1（总线接口） | UART 行为级（波特率/CTS）+ GPIO + Timer + 注册表 | ✅ |
| **M4** | 观测 | 6 | M2（ISS 钩子） | 探针 + 边沿编码 + 波形通道 + 增量推送 | ⚠️ |
| **M5** | 调试 + 反汇编 | 18 | M2, M1（ELF） | 断点 + 单步（StepOver/Out/Line）+ Reg/Mem + 反汇编视图 + **DWARF 行表 + 源码联动** | ❌ |
| **M6** | 前端 | 10 | M0, M4 | 多视图 hash 路由 + 波形 UI + 嵌入式工作台 + 产品工作台基础面 | ⚠️ |
| **M7** | 横切 | 8 | M6 | IndexedDB 持久化 + Worker 韧性/watchdog + HMS 崩溃快照恢复 + 场景脚本/headless CLI | ❌ |
| **M8** | 多板预留 | 2 | M3 | L1 外设 port 抽象 + L3 事件队列消息槽（不绑定链路） | ❌ |
| **M9** | 联调 + CI + E2E-01 | 10 | 全部 | 智能灯端到端验收 + Spike golden 对拍 CI + macOS 性能 baseline | ❌ |
| **合计** | | **90** | | | |


### 5.2 关键路径图

```
M0 (5d) ── M1 (7d) ── M2 (18d) ── M3 (6d) ── M4 (6d) ── M6 (10d) ── M7 (8d) ── M9 (10d)
                  │                   │            │             │          │          │
                  └── M5/DWARF(10d) ───┘            │             │          │          │
                                                  M5/调试(8d) ── ┘          M8(2d) ───┘
```

### 5.3 13 P0 覆盖矩阵（C 方案 = 100%）

| FR | 内容 | 责任模块 | 阶段 |
| --- | --- | --- | --- |
| FR-01 | 图形化搭建（多节点、多外设） | M-01 / M-02 / M-14 | M1 + M6 |
| FR-02 | 元器件事例化、地址映射配置 | M-01 / M-02 / M-14 / M-15 | M1 + M3 |
| FR-03 | 外设地址映射与查看/修改 | M-14 / M-15 + M-10 UI | M3 + M6 |
| FR-04 | RV32I/IM、伪指令与别名 | M-13 + tests/golden/ | M2 + M9 |
| FR-05 | 反汇编·源码联动 | M-11 反汇编 + M-06 DWARF 行表 | M5 |
| FR-06 | 异常/中断、向量表配置 | M-12 量子化 + M-16 中断控制器 | M2 + M3 |
| FR-07 | UART 波特率/CTS 流控 | M-15 行为级 UART | M3 |
| FR-08 | 断点/单步 | M-08 断点 + M-09 单步引擎 + M-10 UI | M5 + M6 |
| FR-09 | 寄存器/内存查看修改 | M-10 UI + ExecutionBackend 接口 | M5 + M6 |
| FR-10 | GPIO 波形/逻辑分析 | M-15 GPIO + M-18 探针 + M-19 波形 | M3 + M4 + M6 |
| FR-11 | 变量监视、手动读写（地址型） | M-08 watchpoint + M-10 watch UI | M5 + M6 |
| FR-12 | 确定性、可复现 | M-12 量子化 + M-20 快照 + M-23 落盘 | M2 + M7 |
| FR-23 | 多视图独立路由 | M-01 视图装配 + M-03 view store | M6 |

### 5.4 验收节点（可演示里程碑）

| 节点 | 阶段 | 验收标准 |
| --- | --- | --- |
| **N1** 协议冻结 | M0 | contract crate 编译 + DTO roundtrip 序列化测试通过 |
| **N2** 数据底座可加载 | M1 | 拖 ELF 进项目 → 看到 product tree + 内存映射表 + ELF section 列表 |
| **N3** 单指令可执行 | M2 | `addi x1, x0, 42` 跑完能读 x1=42（已有 `addi_executes` 测试） |
| **N4** 全 RV32IMC 跑通 | M2 | Spike golden 对拍通过全部 71 条指令 |
| **N5** GPIO 翻转可见 | M3 + M4 | 智能灯固件跑起来，前端波形图看到 LED 引脚翻转 |
| **N6** 调试联动 | M5 + M6 | 断点停 → 看 Reg/Mem → 单步（StepOver/Out） → 反汇编 + 源码联动 |
| **N7** E2E-01 智能灯验收 | M9 | 智能灯 6 步全绿（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 行为判定） |
| **N8** MVP 完整交付 | M9 | 13 P0 全覆盖 + 性能 baseline ≥5 MIPS + 场景脚本可重放 |

### 5.5 第一刀建议（M1 数据底座，7 人日）

按关键路径顺序，**M0 已完成**。下一阶段 M1 建议分两步：

1. **M-14 总线 + M-01/02 产品树视图**（4 人日）— 先有数据底座，UI 后接
2. **M-06 ELF/Raw/HEX 解析**（3 人日）— 引入 `object` crate（BSD-2-Clause），解 PT_LOAD 段写进 bus；DWARF 留到 M5

不走「先扩 ISS」理由：M-13 扩到 RV32IMC 全集（12d）跟 M-14 弱依赖，可并行走；M-14 是后续所有「加载 → 跑」的物理基础，先打地基更稳。

### 5.6 E2E-01 智能灯固件（自研 · Q1 决议）

按 Q1 决定，自研约 200 行 RV32IMC 最小固件，1 个 GPIO 输入（按钮）+ 1 个 GPIO 输出（LED）+ main 循环（轮询按钮 → 翻转 LED）。工具链：`riscv64-unknown-elf-gcc` 编译为静态 ELF（无 libc，bare-metal）。

验收：在 M9 用 Spike 跑同一固件 + 在我们内核跑同一固件，比较 LED 引脚时序一致。

---

## 6. 模块依赖关系（按模块推进原则）

> 本节是 v0.6 引入的工作准则：**按模块独立完成（不按功能）**，每个模块收口经
> `cargo test --workspace` + `clippy -D warnings` + `fmt --check` 三关。
> 依赖关系决定下一个模块的选择，E2E-01 验收节点决定推进顺序。

### 6.1 依赖表

**内核 crate（`riscv_sim_kernel`）**

| 模块 | 依赖 | 状态 | 备注 |
| --- | --- | --- | --- |
| M-14 总线 | contract | ✅ v0.4 | 叶子；所有访存的物理基础 |
| M-06 ELF/Raw/HEX | contract + M-14（写路径） | ✅ v0.5 | 叶子；M-06 → M-14 端到端集成已通 |
| M-21 日志 | contract | ❌ | 最叶子（基础设施） |
| M-15 UART/GPIO/Timer | contract + M-14 | ✅ v0.7 | Uart16550 + GpioBank(32-pin) + TimerRiscv + PeripheralRegistry；16 单测全绿；接线下一 PR |
| M-13 ISS RV32IMC | contract + M-14 | ⚠️ (ADDI only) | M-14 已就绪，可扩 71 条 |
| M-11 DWARF/反汇编/trace | contract + M-06 | ❌ | M-06 ✅；DWARF 行表 06-7 |
| M-18 探针 | contract + M-14 + M-15 | ✅ v0.8 | ProbeRegistry 完整；v0.9 完成 M-18 → M-19 接线；M-15 外设源待接线 |
| M-19 波形 | M-18 + contract | ✅ v0.9 | WaveformPipeline 完整（边沿编码 + 环 + 批量推送 + CSV/VCD 导出） |
| M-16 中断控制器 | M-14 + M-15 | ❌ | 等 M-15 |
| M-12 事件驱动引擎 | M-13 + M-14 + M-16 | ⚠️ (骨架) | 等 M-13 + M-16 |
| M-17 执行后端 | M-12 + M-13 + M-14 | ⚠️ (骨架) | 等 M-12 + M-13 |
| M-08 断点 | M-17 + M-13 | ❌ | 等 M-17 |
| M-09 单步 | M-17 + M-13 | ❌ | 等 M-17 |
| M-20 快照 | M-12 + M-13 | ❌ | 等 M-12 + M-13 |
| M-23 DataStore | M-20 + contract | ❌ | 等 M-20 |
| M-22 HMS | M-21 + M-17 + M-12 | ❌ | 等 M-17 + M-21 |

**前端（`front/`）**

| 模块 | 依赖 | 状态 | 备注 |
| --- | --- | --- | --- |
| M-01 工作台装配 | (DOM 本身) | ✅ v0.4 | 叶子 |
| M-02 视图组件 | M-01 + M-03 + 数据通道 | ✅ v0.4 | 4 视图接口形状已落地 |
| M-03 View Store | (前端本身) | ✅ v0.4 | 叶子 |
| M-07 transport | contract `Message` | ❌ | 叶子；绑定 kernel WASM |
| M-04 会话管理 | M-07 | ❌ | 等 M-07 |
| M-05 命令路由 | M-04 + M-07 | ❌ | 等 M-04 + M-07 |
| M-10 registermemory 服务 | M-17 后端 + M-07 | ❌ | 等 M-17 + M-07 |

### 6.2 推进顺序（按依赖叶子 + E2E-01 主线）

```
                                    [已完成 ✅]
   M-14 总线 ──────────────┬────── M-06 ELF/Raw/HEX
       │                  │
       ├──[可立即]── M-15 UART/GPIO/Timer ── M-18 探针 ── M-19 波形
       │                  │                            │
       │                M-16 中断控制器                │ ← v0.9 止步于此
       │                  │
       ├──── M-13 ISS 全集扩（I 47 + M 8 + C 16, 71 条）  ★ 下一目标
       │                  │
       ├──── M-12 事件引擎扩（指令预算 / 量子化 / peripheral tick）
       │                  │
       ├──── M-17 ExecutionBackend（SelfIssBackend）
       │                  │
       ├──── M-08 断点  ──┴── M-09 单步
       │
       └── M-20 快照 ── M-23 DataStore ── M-22 HMS ── M-21 日志（同步）

   [前端顺序]
   M-01/02/03 ── M-07 transport ── M-05 命令路由 ── M-04 会话 ── M-10 服务
```

### 6.3 下个目标

按依赖图 + E2E-01 §5.4 主线（**M-19 已 ✅ v0.9**，观测管道内核侧封顶），下一个瓶颈是 **M-13 ISS 全集扩**：

**M-13 ISS RV32IMC 全集**（12 人日）

为什么是它：M-13 是关键路径上**唯一 gate 7 个下游模块**的节点——M-12 / M-17 / M-08 / M-09 / M-20 / M-23 / M-22 全部等它。同时 E2E-01 的「运行 / 单步 / 内存」三步也都等它（M-13 现在只有 `ADDI` 一条指令）。

子组件拆分（分阶段交付，每阶段独立评审 + 回归）：
- **阶段 1（~3d）**：D-04 `x0` 硬连线（`CROSS_MODULE_DEBT.md` 阻断项，阻塞 Spike 对拍）+ RV32I 剩余指令（LUI / AUIPC / LOAD / STORE / BRANCH / JAL / JALR / 全部立即数与寄存器立即数形式）
- **阶段 2（~2d）**：M 扩展 8 条（MUL / MULH / MULHSU / DIV / DIVU / REM / REMU）
- **阶段 3（~3d）**：C 扩展 16 条（压缩指令，含非法压缩码处理）
- **阶段 4（~4d）**：Spike golden 对拍（Q7 决议「必做」，FR-04 每条指令 golden）—— 依赖 `riscv64-unknown-elf-gcc` + `spike --isa=rv32imc`

验收：
- `cargo test -p riscv_sim_kernel m13_iss` 覆盖 71 条指令的解码 + 执行语义（含边界：溢出 / 除零 / 未对齐 / 非法码 trap）
- `tests/golden/` Spike 对拍全绿（阶段 4 交付）
- `cargo clippy --workspace --all-targets -- -D warnings` 通过
- `cargo fmt --all -- --check` 通过
- 演示用例：M-06 加载 ELF → M-14 映射 → M-13 跑通一段含 LOAD/STORE/BRANCH 的程序，读回内存结果正确

**并行 0.5d（建议同 PR 带上）**：M-15 → M-18 外设接线（D-12 剩余一半）—— `GpioBank` / `Uart16550` / `TimerRiscv` override `Peripheral::on_probe`，让固件写 OUT 真的触发 M-18 边沿 → M-19 波形。做完这一步，**N5「GPIO 翻转可见」的内核路径 100% 闭合**（只剩 M-07 transport + M-02 波形 UI 两个前端件）。

预计 v0.10 落地阶段 1 + 并行接线。解锁 M-12 事件引擎扩 + M-16 中断控制器可并行启动。

### 6.4 风险与降级（按 AD §12）

| ID | 风险 | MVP 应对 | 降级方案 |
| --- | --- | --- | --- |
| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | **v0.3 决议：纳入 MVP（M5 阶段，10 人日）** | 失败时回退到「反汇编 + 符号表」，但 MVP 仍按完整 DWARF 排期 |
| R8 | 确定性范围 | 种子 + 时间戳化输入事件序列（AD §8.1 D0） | — |
| R10 | 多板互联复杂度 | MVP 单板不阻塞；接口预留 | — |
| R12 | 性能 5-10 MIPS 估算基线 | 解释执行 MVP | 演进解锁 JIT/QEMU |
| L1 | 装配边界 | §3.4 评审 L1 子集 | — |
| L3 | 客户端输入威胁 | 客户端低；服务端放大时强制 | — |

---

| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | **v0.3 决议：纳入 MVP（M5 阶段，10 人日）** | 失败时回退到「反汇编 + 符号表」，但 MVP 仍按完整 DWARF 排期 |
| R8 | 确定性范围 | 种子 + 时间戳化输入事件序列（AD §8.1 D0） | — |
| R10 | 多板互联复杂度 | MVP 单板不阻塞；接口预留 | — |
| R12 | 性能 5-10 MIPS 估算基线 | 解释执行 MVP | 演进解锁 JIT/QEMU |
| L1 | 装配边界 | §3.4 评审 L1 子集 | — |
| L3 | 客户端输入威胁 | 客户端低；服务端放大时强制 | — |

---

## 7. 决策记录（按时间倒序）

| 日期 | 决策 | 依据 |
| --- | --- | --- |
| 2026-10-19 | **开发流程定案：所有模块开发走「基于 main 拉新分支 → 分支上开发并 commit → 检视闭环 → 同步回 main」** | 用户决策。此前 `CONTRIBUTING.md` §8 写着「禁止直接 commit 到 main」，但实际做法是直接提交到 main（v0.7~v0.9.1 五个提交都在 main 上，无分支无 PR）——文档与实际不一致。现在以实际流程为准统一文档：**分支承载未检视的工作，main 承载已检视闭环的工作**；`main` 始终与 `origin/main` 一致且保持可回滚 |
| 2026-10-19 | **M-19 review 的 16 项一次性闭环，不重写已推送历史，追加 fix 提交** | 用户决策。阻断级缺陷（`clone()` + `capacity()` debug panic）已在 38e62ed 推上远端；缺陷只影响该条特定调用路径，追加修复即可保住审计链，强推反而会覆盖别人的本地状态 |
| 2026-10-19 | **#2 拆成 `dropped_ranges`（真丢）与 `unconsumed_ranges`（没到 sink）两个列表** | 无 sink 时内容**没有丢**（交给直接调用方的返回值），记成 DropRange 会让消费方去补一段它实际拿得到的洞；两种结局语义不同，不该混在一个列表里 |
| 2026-10-19 | **#1 删断言而非手写 Clone 保住容量** | `initial_cap` 已是环行数学的唯一真源（M-18 review #13 的规则）；断言 `Vec::with_capacity(n)` 的实际容量等于 n 是在断言一个标准库**未承诺**的实现细节 |
| 2026-10-19 | **#3 VCD label 白名单保留 `.` `-` `[]` `/`** | review 建议的 `[A-Za-z0-9_]` 会把 `GPIO0.OUT` 打成 `GPIO0_OUT`；真正需要中和的是 `$`（提前终止 `$var` 命令）、空白与控制字符 |
| 2026-10-19 | **#4 用 `ExportSession` 持游标，而不是缓存 `count_filtered`** | 缓存只能省掉计数，skip 的 O(n) 仍在；持游标的会话是唯一能把总代价真正降到 O(n) 的做法。旧入口保留为「一次性 / 续导锚点」并标注代价模型 |
| 2026-10-19 | **M-19 溢出策略：`PAUSE_SIM` 与 `DROP_OLDEST_MARK` 都丢最旧，区别只在要不要请求暂停仿真** | 设计文档伪码的 `PAUSE_SIM` 分支不推进 `head`，会让环停在满状态并**伪造历史空洞**（违反「不伪造数据」）；实际观察已发生，拒绝记录它等于造假。暂停是控制面操作，不改寄存器/内存（19-9） |
| 2026-10-19 | **序列号空间「失败也前移」+ 独立 `DropRange` 记录内容空洞** | 数字空间保持无空洞（前端无法从 seq 推断丢失），内容空洞由 `DropRange` 显式记录，供 M-07 `resumeBySeq` / `E_PROTO-05` 续拉补齐（19-3 / 19-7） |
| 2026-10-19 | **导出归并用 k 路堆归并，不物化归并流** | 导出满窗口（4 × 2×10⁵）不额外占 20MB，守住 25MB 内存预算（19-2）；环内「到达序 = 时间序」由单调虚拟时钟保证，导出前再防御性重排 |
| 2026-10-19 | **M-19 不拥有定时器，调度者是 M-12 周期回调** | 19-5；M-19 不读任何时钟是其可确定性重放的前提（AD §8.1 D0） |
| 2026-10-19 | **M-19 不新增错误码**：无效策略复用 `E_OBS-01`，内存超预算复用 `E_CTX-02` | 设计文档 §6.1「不新增错误码」+ §3.5 明确指定 `E_CTX-02` |
| 2026-10-19 | **契约新增 `EdgeSink` / `DeltaSink` 两个驱动端口 + `EdgeEvent` / `WaveformDelta` / `DropRange` 三个 DTO** | 「契约先行」：M-18 → M-19 的交接必须两侧都不依赖对方 crate 内部实现（LLD §2.5）；M-0 冻结范围内的第四次 additive 变更 |
| 2026-10-19 | **M-18 `sample_inner` 原先丢弃的 `EdgeKind` 随 `EdgeEvent` 下发** | M-18 review #4 遗留的「kind 信息丢失」在 M-19 接线后自然消解，无需改契约 `Edge` 结构（保 M-0 冻结） |
| 2026-10-19 | **建立 `docs/reviews/` 目录 + `CROSS_MODULE_DEBT.md` 跨模块欠账台账** | 模块报告管模块内问题、跨模块台账管跨模块问题，职责切分避免同一问题在两份报告里各记一半；台账按**修复归属模块**组织并记录阻塞拓扑与处理批次 |
| 2026-10-19 | **跨模块 review 项一律「上收台账 + 本轮不动」，单独排期** | 用户明确要求跨模块问题（#6 分配上限 / #9 字段可见性 / #11 接线 size 一致性 / #12 M-12 tick）不在模块 PR 里夹带改动 |
| 2026-10-19 | **契约 `TraceEvent.dir` 用 `enum TraceDir` 替代 `u8` 魔数** | 与 M-14 `AccessDir` 对齐；M-0 冻结范围内的第三次 additive 变更（前两次：`Peripheral::read_clear`、`Peripheral::on_probe` + `ProbeSink`） |
| 2026-10-19 | **契约 `Probe` trait 暂不删也不实现，留待 M-0 下一窗口统一清理** | 全代码库零 `impl Probe for`，与 M-15 report #10 是同一个死契约；不在模块 PR 里做契约破坏性变更 |
| 2026-10-19 | **M-14 区间管理用 `BTreeMap<u32, Region>`**（标准库红黑树）而非手写 AVL 增强 BST | MVP 区间数 ≤ 8，BTreeMap O(log n) + 简单正确；手写 AVL 不增加明显性能但增加错误面 |
| 2026-10-19 | **M-14 Peripheral typed handle 用 `PeripheralId(u32)` 占位** | M-15 当前只有 DummyUart，没必要立即引入 `Rc<dyn Peripheral>` 生命周期；评审 14-11 留待 M-15 真实现 UART/GPIO 时升级 |
| 2026-10-19 | **M-14 写跨区间两阶段：阶段一预检全部 byte slice → 阶段二统一执行** | LLD §3.2 评审 14-1 原子性要求：任一未映射 → 整笔失败，不允许 RAM 已写 + 异常上报的部分成功状态 |
| 2026-10-19 | **M-14 trace hook 类型 `Box<dyn FnMut(&BusAccess)>`（无 Send bound）** | MVP 单线程模拟；Send bound 留 M-12 P2 SMP 阶段再加 |
| 2026-10-19 | **M-13 接入 M-14 加 `step_one_via_bus`，保留 `step_one` 函数指针老接口** | 双通道：函数指针接口零依赖用于纯解释器单测；`&mut Bus` 接口用于真实总线分发 |
| 2026-10-19 | **M-01 capBits MVP 内置 role 默认集合**；M-05 negotiate 接入后改从协议取 | M-05 还没实现；URL `?role=product|embedded` 切换角色作为 MVP 临时入口 |
| 2026-10-19 | **`render → ` CustomEvent(`riscv:render`) 派发**，M-02 监听 | M-01 switchView 返回值是个内部接口；通过 CustomEvent 让任意视图组件都可挂载 |
| 2026-10-19 | workspace `missing_docs = "allow"` + `derivable_impls = "allow"` | M-0 冻结后尚未补全公开字段 doc；不为此重新 churn 协议代码；M-0 后续收敛时切回 deny |
| 2026-10-19 | m12_engine `instruction_budget` 字段加 `#[allow(dead_code)]`；QueuedEvent 翻转 Ord 加 `non_canonical_partial_ord_impl = "allow"` | M-12 调度骨架尚未接 ISS 指令预算；BinaryHeap min-heap trick 需要非规范 Ord。等 M-12 完成 ISS 接通后回收 |
| 2026-10-19 | self.best_effort **第一步落入约 2.5 人日**（M-14 + M-01/02/03 + M-13 接入） | 比 §5.5 估计的 4 人日提前 1.5d，部分归功于 BTreeMap 取代手写 AVL |
| 2026-10-19 | 自研 ISS 用 **Rust**（非 C） | 用户明确要求 |
| 2026-10-19 | MVP 部署形态可**目录形式**（不必单文件自包含） | 用户放宽 NFR-05 |
| 2026-10-19 | 骨架策略：先做 stub | 用户 |
| 2026-10-19 | 实施起点：先跑通 MVP | 用户（**误读**为「M-12 + M-13 单测通过」非「E2E-01 跑通」） |
| 2026-10-19 | MVP 范围以 **AD §13.6** 为准（13 P0 + 90 人日） | 重读架构文档后校准 |
| 2026-10-19 | `contract::WatchAccess` 三个变体补 doc；其他 variant 字段 doc 暂以 `#![warn(missing_docs)]` 跳过 | Rust 1.99 严格度 |
| 2026-10-19 | `m18_probe::drain_waveform` 重写为 `values_mut()` + `Probe::drain()`，替换原 `mem::swap` hack | cargo test 失败 |
| 2026-10-19 | `m12_engine::run_until_idle` 改为「每量子推进 → 排空到期」循环 | 原语义让 `engine_advances_time` 测试只走 1 量子 |
| 2026-10-19 | `m17_backend::SelfIssBackend` bound 从 `BusMaster` 改为 `EventBus` | `Engine` 方法要 EventBus bound |
| 2026-10-19 | `m15_peripheral::PeripheralRegistry` 拿掉 `derive(Debug)`，手写只输出 count | `Box<dyn Peripheral>` 不 Debug |
| 2026-10-19 | **Q1** 智能灯固件 = **自研**（不外取） | 用户答复 |
| 2026-10-19 | **Q2** ISS = **RV32IMC 全集**（I+M+C，含 16-bit 压缩指令） | 用户答复 |
| 2026-10-19 | **Q3** 持久化 = **IndexedDB**（用 `idb` 包装，BSD-3，~5KB） | 用户答复（接受 +0.3-0.5 人日换容量） |
| 2026-10-19 | **Q4** Worker URL = **同源**（HTTP 部署） | 用户答复 |
| 2026-10-19 | **Q5** 行为判定 = **canvas 比对**（虚拟 LED + 像素 diff） | 用户答复 |
| 2026-10-19 | **Q6** 性能基线硬件 = **macOS / Apple Silicon** | 用户答复 |
| 2026-10-19 | **Q7** Spike golden 对拍 = **必做**（FR-04） | 用户答复 |
| 2026-10-19 | **MVP 范围** = **选项 C**：§13.6 全量 90 人日（含 DWARF、CI、场景脚本、多板接口）；不降级 | 用户决议（v0.2 提议 B/C 后选 C） |
| 2026-10-19 | **M-06 ELF 解析自写而非引入 `object` crate** | M-06 §3.1 MVP 范围窄（ELF32 LE RV32 ET_EXEC），自写 ~150 行覆盖所有错误码；`object` 0.36 体积偏大；P1 阶段如需 ELF64 / non-RV32 ELF 再引入 |
| 2026-10-19 | **M-06 错误码使用 `ErrorFamily::Loader`（contract 既有）+ scenarios 01..04** | 设计文档用 E_PROTO-NN（既有），contract 没 PROTO family；用 Loader family + additive scenarios（PROGRESS.md §7 决策）最不破坏 M-0 冻结 |
| 2026-10-19 | **M-06 ELF bss gap（p_memsz > p_filesz）切为独立 segment** | 避免重复 PT_LOAD；M-14 `map_ram(base, size, &[])` 自然全 0 初始化，对齐评审 14-5 |
| 2026-10-19 | **M-06 HEX 解析「连续合并 + 同基地址后行覆盖 + 重叠段不分摊」三规则** | 06-4：连续地址合并节省 M-14 segment slots；同 base 后行覆盖避免 data 长度不一的 snapshot 问题；重叠段留给 M-14 E_BUS-03 错误码上抛 |
| 2026-10-19 | **M-06 集成测试放在 `crates/kernel/tests/*.rs`**（Cargo 默认 integration test 路径） | AGENTS.md 写的 `tests/unit/` 在顶层没 crate，cargo 不会发现；改为 crates/kernel/tests 是 Rust 标准 |
| 2026-10-19 | **v0.6 决策：按模块独立完成（不按功能）** | 23 模块按依赖关系 + E2E-01 主线，每个模块收口经 cargo test / clippy / fmt 三关，独立子模块；方便测试与代码评审。先做依赖叶子（依赖最少），再做核心，最后做派生 |
| 2026-10-19 | **M-15 Uart16550 1-byte FIFO**（评审 15-9） | MVP 智能灯「字符回环」场景够用；E2E-01 N5 验收后可演进到 16-byte 真实硬件 FIFO |
| 2026-10-19 | **M-15 GpioBank 32-pin 固定**（评审 R5） | `PIN_COUNT_MASK = u32::MAX`；MVP 不做可配置 pin 数，P1 演进 |
| 2026-10-19 | **M-15 TimerRiscv 用 `take_pending_irq()` 公开 API 替代 `InterruptSink::raise()`** | contract `Peripheral::tick` 签名只返 `Vec<PendingWrite>`，无法直接调 IRQ sink；M-16 权威 IRQ 源上线后 Timer 降级为 no-op（评审 N3 fallback） |
| 2026-10-19 | **M-15 ↔ M-14 接线单独 PR** | 本 PR 只完成 M-15 行为级外设本体；M-14 typed handle 从 `PeripheralId(u32)` 占位升级到 `Box<dyn Peripheral>` / `Rc<dyn Peripheral>` 委托，留给 wiring PR（评审 v0.6.1 #13 已签约定锁） |
| 2026-10-19 | **M-15 拆 uart/gpio/timer/tests 5 个子文件** | 与 LLD §3.2-3.4 子组件对齐；`mod.rs` 只保留公开 API + `PeripheralRegistry` + re-exports；按子组件独立 review + 测试 |
| 2026-10-19 | **M-15 `PeripheralRegistry` 用 `BTreeMap<String, Box<dyn Peripheral>>`** | key = `Peripheral::id()`；MVP 单线程；`Send + Sync` 留 M-12 P2 SMP 阶段再加 |
| 2026-10-19 | **M-15 review #2 修复用 additive 默认方法 `read_clear`**，不开 M-0 解冻窗口 | contract::Peripheral trait 加 `fn read_clear(&mut self, …) -> Result<u32, _>` 默认透传到 `read`；Uart16550 override 清 RX FIFO；零 breaking change |
| 2026-10-19 | **M-15 review #3 GPIO TOGGLE 改「多 pin mask toggle」** | `outputs ^= value & PIN_COUNT_MASK`；固件 `1<<pin` 翻对应 pin；多 pin mask 原子翻转；doc 同步改为「each bit set in `value` flips that pin's `out` atomically」 |
| 2026-10-19 | **M-15 review #14 UART STATUS 采用 W1C 极性**（写 1 清零） | 主流 PL011/16550 硬件约定；同步改 tests.rs |
| 2026-10-19 | **M-15 review #8 Timer mtime 只读由引擎驱动** | 删 `set_mtime`；MTIME_LO/HI 写权限改为 no-op；mtimecmp 保留固件可写（设 deadline）；两个时间基语义不再混 |
| 2026-10-19 | **M-15 review #17 加 `FidelityLevel` 枚举** | LLD §2.1.1 fidelity 元数据真实载体；MVP 全部 Behavioural；P1 演进可填更高等级 |
| 2026-10-19 | **M-15 review #11 跨模块 size 一致性校验延 M3 接线 PR** | M-14 `map_peripheral(base, size, &dyn Peripheral)` 加 `debug_assert_eq!(p.size(), region_size)` |
| 2026-10-19 | **M-14 review #10/#11/#12/#14/#16/N5 收紧闭环** | 删 belt-and-braces 死分支；`read_bytes`/`write_bytes` 改返 `Result` 与 `read_region_byte` 一致；`contains`/`crosses_region` 用 `checked_add` 显式拒绝地址回绕；Byte 越界测试收紧为 `E_BUS-01`；更新文件末尾「Tests cover」清单 + 删孤立横幅。**跨模块 #6/#9 挂账不动** |
| 2026-10-19 | **M-14 review #6 跨模块挂账**：分配上限 | `Bus::prepare_bytes` 仍是裸 `vec![0u8; size as usize]`；需与 M-06 `ParseLimits` 联合改动加 `total_image_bytes` / `max_region_bytes`；畸形固件可在 WASM 内 OOM 触发 worker 崩溃 |
| 2026-10-19 | **M-15 review §五 N1/N2/N3 闭环** | N1：抽 `m15_peripheral/reg.rs` 共享 `register_offset`/`extract_subword`，消除 review #5 引入的 6 份重复；N2：GPIO `IN` 寄存器回读输出脚（`pin_levels()` = outputs & directions \| inputs & !directions），解锁 N5「GPIO 翻转可见」+ M-18 波形；N3：去重位图 `line < 16` 放宽到 `line < 32`，与文档「unique IRQ line per pin」承诺一致 |
| 2026-10-19 | **M-15 review §五 N2 优先级最高** | review §五「二次复查」明确标注：固件通过 OUT 驱动 LED 后无法从 IN 读回引脚电平，M-18 波形与 N5 验收都依赖此。已落地 |
| 2026-10-19 | **M-18 契约层 additive 默认方法**（v0.8） | `Peripheral::on_probe(_cb: Option<ProbeSink>) {}` 默认 no-op；与 M-15 review #2 `read_clear` 同样的 additive 模式；M-0 冻结窗口不开 |
| 2026-10-19 | **M-18 双源探针 + 双流分离** | trace 走 `BusAccess` 钩子 → 独立 `TraceStream` 环形（FR-14 导出）；波形走 `ProbeSink` 回调 → 边沿事件 → M-19 渲染；两流不混 |
| 2026-10-19 | **M-18 边沿判定 `prev` 用 `Option<u64>`**（v0.8） | 设计文档 §3.3 伪码 `prev[ch]` 留了未初始化陷阱（首次观察必发但代码路径会"同值不产"过滤掉）；本轮显式收为 `None` 表达"未观测"，首观察必发 |
| 2026-10-19 | **M-18 冲突校验只在 `define_channel`，不在 `wire_signal_probe`** | 设计文档 §3.2 的"同 ch 冲突拒绝"指 *通道定义* 冲突；外设回调可重复安装（替换前一次 sink），符合 `Option<…>` 替换语义 |
| 2026-10-19 | **M-18 `BusWidth` 字节数走 `.bytes()` 不是 `as u8`** | `BusWidth` discriminant 是 0/1/2，需要 `.bytes()` 拿 1/2/4；测试 `make_bus_hook_routes_record_bus_access` 第一次就抓到这个错 |
| 2026-10-19 | **M-18 throttle 算法 = "decrement; >0 跳过; =0 发并 reset"**（v0.8） | 设计文档 §3.3 语义；`throttle_n=4` 配 10 次观察 → 2 次发（第 4、第 8 次），不是 `ceil(10/4)=3`；M-18 测试注释已注明 |
| 2026-10-19 | **M-18 ↔ M-15 接线单独 PR**（v0.8 决议） | 契约层 `Peripheral::on_probe` 已就位；M-15 三个外设（GpioBank/Uart16550/TimerRiscv）尚未 override `on_probe`，等接线 PR（D-12 跨模块挂账）。本轮不动 |

---

## 8. 已澄清问题（v0.2 全部答复）

| 编号 | 问题 | 答复 | 对实施的影响 |
| --- | --- | --- | --- |
| Q1 | 智能灯固件来源 | **自研** ~200 行 RV32IMC 最小固件（按钮 GPIO 输入 + LED GPIO 输出 + 简单延时循环） | M-15 必须有 GPIO 输入 + 输出；M-11 反汇编要看得懂它；M-06 ELF 解析要能装它 |
| Q2 | ISS 指令集范围 | **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集） | M-13 估 **12 人日**；C 扩展需 PC 步长 2/4 切换 + 16-bit 解码表 |
| Q3 | 持久化后端 | **IndexedDB**（用 `idb` 包装，~5KB BSD-3 库） | M-23 估 **3.5 人日**（含 `onupgradeneeded` schema 声明）；架构 §7.4 容量模型原样适用；M-05 CommandRouter 须 async 化 |
| Q4 | Worker URL | **同源 URL**（`./kernel-worker.js`，HTTP 部署） | M-07 简化（无 blob URL 内联代码）；MVP 假定 `python3 -m http.server 8080 --directory front` 或等价 HTTP 部署 |
| Q5 | 行为判定 | **canvas 比对**（虚拟 LED 渲染到 canvas + 像素 diff + 视觉对照） | M-01 加一个产品行为视图（虚拟 LED 灯泡 + 状态机动画）；可选自动化：每 N ms 截 canvas vs 期望图 |
| Q6 | 性能基线硬件 | **macOS**（Apple Silicon M3 / 评测机） | 性能验收在该硬件上实测 ≥5 MIPS 估算基线；CI runner 也是 macOS |
| Q7 | Spike golden 对拍 | **必做**（FR-04，每条已实现指令对拍） | `tests/golden/` 用 Spike 作为 oracle 跑同一程序，对比 `m13_iss` 的 register / memory 输出；CI 必跑；MVP 起步可只对 ADDI + 关键 LOAD/STORE/BRANCH |

### v0.2 锁定后的实施前置条件

- **依赖**：需要安装 `riscv64-unknown-elf-gcc` 工具链（编译自研固件） + `spike`（golden oracle），以及 npm 上没有依赖所以前端 `idb` 走 ESM CDN 或本地拷贝
- **环境变量**：可能需要 `SPIKE_BIN=/path/to/spike` 给 golden test 用
- **CI**：GitHub Actions macOS runner + Spike 安装步骤

---

## 9. 文档版本历史

| 版本 | 日期 | 变更 | 作者 |
| v0.10.0 | 2026-10-20 | **前端 M-01/02/03 首次模块级 review 闭环 18/18**（`docs/reviews/FRONTEND_Review.md` §七）：**修掉 2 项阻断级白屏缺陷**（D-01/D-02 ESM 链接期失败 → 页面 100% 空白）+ 5 项功能缺陷 + 9 项整洁项，**另捕获 2 项评审漏项**（`seedDemoBus()` 从未定义 / `refreshAll()` 会把当前面板藏掉）。**新增前端门禁**：`front/tests/` 19 用例（最小 DOM stub，零第三方依赖）+ `tools/check-front.sh` + `.github/workflows/ci.yml`（仓库此前无 CI），并以 8 组变异测试证明门禁能抓住同类回归。前端测试覆盖 0 → 19；台账 D-01/D-02/D-18/D-19/D-20 闭环，D-23 部分闭环 | agent (mavis) |
| --- | --- | --- | --- |
| v0.1 | 2026-10-19 | 初稿：建立项目状态、§13.6 WBS 校准、关键路径、下一里程碑 E2E-01、决策与风险 | agent (mavis) |
| v0.2 | 2026-10-19 | MVP 范围从「选项 B（~30 人日）」升级到「选项 C（90 人日）」；补 Q1-Q7 已答复，DWARF/CI/场景脚本/多板接口预留纳入 MVP | agent (mavis) |
| v0.3 | 2026-10-19 | 嵌入 v0.2 决策表；WBS 同步；E2E-01 验收标准定锚 | agent (mavis) |
| v0.4 | 2026-10-19 | M1 数据底座第一步完成：M-14 总线（20 单测）+ M-13 ↔ M-14 集成（+2 单测）+ M-01/M-02/M-03 前端装配 + 4 视图组件；WBS 行 #1 #7 #9 状态推进；累计人日 10.5 → 15；E2E-01 第 1 步可达 | agent (mavis) |
| v0.5 | 2026-10-19 | M1 数据底座第二步完成：M-06 ELF / Raw / HEX 解析（18 单测）+ M-06 → M-14 → M-13 端到端集成（5 测试，三格式全跑通 ADDI）；ELF32 LE 自写不加 `object` 依赖；累计人日 15 → 16.5；**§5.4 N2 「数据底座可加载」达成** | agent (mavis) |
| v0.6 | 2026-10-19 | 策略调整：按模块独立完成（不按功能），梳理 23 模块依赖关系表（§6.1）+ 推进顺序图（§6.2）+ 下一目标 **M-15 UART/GPIO/Timer（6d）**；未实施新代码 | agent (mavis) |
| v0.6.1 | 2026-10-19 | 按 `docs/reviews/M-14_BusMemoryMap_Review.md` 修 M-14：落实 #1-#5 + #7-#8 + #13-#15 共 11 处（hook 时机 / stats 分派 / 三态 Crossing / E_BUS-07 / BusAccess.ok）；test 20 → 27；累计 lib 49 + integration 5 = **54 全绿**；跨 PR 不动 #6 / #9 / #10-12 | agent (mavis) |
| v0.7 | 2026-10-19 | **M-15 UART/GPIO/Timer 完成**：Uart16550（1-byte FIFO + 波特率分频）+ GpioBank（32-pin 边沿 IRQ latch）+ TimerRiscv（mtime/mtimecmp sticky pending IRQ）+ PeripheralRegistry；拆 uart/gpio/timer/tests 5 子文件；16 单测全过；lib 49 → 65（+16），workspace 累计 **70 全绿**；WBS #5 ✅；阶段 M3 ✅；§6.3 下一目标切到 M-18 探针 | agent (mavis) |
| v0.7.1 | 2026-10-19 | 按 `docs/reviews/M-15_PeripheralModel_Review.md` 落地 19 项：**3 阻断 + 4 高 + 3 中 + 9 低 + 1 跨模块（#11 延 M3）**。契约层只用 additive 默认方法 `Peripheral::read_clear`（不开 M-0 解冻窗口）；UART 加 `take_tx()`、TX_OVERRUN 独立位、STATUS W1C；GPIO TOGGLE 改多 pin mask toggle、方向生效、pending 去重；Timer mtime 只读由引擎驱动；加 `FidelityLevel` 枚举；tests 16 → 25（+9）；lib 65 → 74（+9）；workspace 累计 **79 全绿** | agent (mavis) |
| v0.7.2 | 2026-10-19 | 按 `docs/reviews/M-14_BusMemoryMap_Review.md` §四 闭环核对，落地剩余 6 项可独立处理（#10 belt-and-braces 删 / #11 read_bytes-write_bytes 改 Result / #12 wrapping_add 改 checked_add / #14 Byte 越界测试收紧 / #16 Tests cover 清单更新 + N5 删孤立横幅）；**2 项跨模块挂账**（#6 需 M-06 联改分配上限 / #9 影响 m06_load_to_bus + m02_viewcomponents 收紧 Region 可见性），按用户要求先不动。M-15 review v0.7.1 后无新增修改。workspace 累计 **79 全绿** | agent (mavis) |
| v0.7.3 | 2026-10-19 | 按 `docs/reviews/M-15_PeripheralModel_Review.md` §五 复查遗留 3 项：**N1** 抽 `reg.rs` 共享 helper 消除 review #5 引入的 6 份重复（DRY 回归）；**N2** GPIO `IN` 寄存器回读输出脚（`pin_levels()` = outputs & directions \| inputs & !directions），解锁 N5「GPIO 翻转可见」+ M-18 波形；**N3** 去重位图 `line < 16` 放宽到 `line < 32` 同步注释。tests 25 → 29（+4）；workspace 累计 **83 全绿** | agent (mavis) |
| v0.8 | 2026-10-19 | 按 §6.3「下个目标」落地 **M-18 探针**：4 子模块（channel/trace_stream/registry/tests）+ 契约层 additive 扩展（`Peripheral::on_probe` 默认 no-op + `ProbeSink` 类型别名 + `EdgeKind` 枚举 + `TraceEvent` 结构）。双源探针架构（总线 trace 走 `BusAccess` 钩子，外设信号走 `ProbeSink` 回调）；边沿判定 5 条规则（master switch / channel enable / throttle / pulse 旁路 / 同值过滤）；`TraceStream` 环形 2×10⁵ 容量 + dropped 计数 + seq 单调；`wire_signal_probe` 走 `Rc<RefCell<>>` 闭包桥接外设。M-15 ↔ M-18 接线是独立 PR（D-12 跨模块挂账）。tests 0 → 20（+20）；workspace 累计 **103 全绿** | agent (mavis) |
| v0.8.1 | 2026-10-19 | **M-18 review 闭环 13/15** + **建立 `docs/reviews/` 文档体系**。M-18 落地 13 项：热路径去字符串分配（闭包直连 `sample_inner`）/ `try_borrow_mut` 消除 borrow panic（加 `borrow_contended` 计数）/ `prev` 跨 enable-disable 两条测试固化既有正确行为（原 review 推断有误，以测试纠正）/ 契约 `TraceEvent.dir` 改 `enum TraceDir` / `classify` SingleBit 做 `&1` 截断 / `TraceStream` 加 `initial_cap` 脱离 `Vec` 容量假设 / 4 处公开 doc 补全。**未闭环 2 项低危**：契约 `Probe` trait 死契约（与 M-15 report #10 同源，延 M-0 窗口）、`drain_waveform` 借用交错。新建 `docs/reviews/` 4 份文档（3 份模块报告 + `CROSS_MODULE_DEBT.md` 跨模块台账，23 条 / 5 阻断）。M-18 tests 20 → 29（+9）；workspace lib 98 → 107（+9）；workspace 累计 103 → **112 全绿**；累计人日 16.5 → ~21（≈23%） | agent (mavis) |
| v0.2 | 2026-10-19 | 答复 Q1-Q7：自研固件 / RV32IMC 全集 / IndexedDB / 同源 Worker / canvas 比对 / macOS / Spike 必做；M-13 工作量上调至 12 人日；§5 step 2 同步更新；新增 §8 「已澄清问题」表与实施前置条件 | agent (mavis) |
| v0.3 | 2026-10-19 | MVP 范围从 B（30d, DWARF 留 P1）升级到 **C（§13.6 全量 90 人日）**；§5 重写为 10 阶段计划（M0-M9）+ 13 P0 覆盖矩阵 + 8 个验收节点 + 关键路径图；§6 R7 改「纳入 MVP」；M0 已完成，剩余 85 人日 | agent (mavis) |

---

## 10. 设计文档引用

- **AD §11.1** 演进路线（里程碑时间轴）— MVP 范围定义
- **AD §13.6** WBS + 关键路径 + 人日估算
- **AD §13.3** 智能灯 E2E-01 验收主线
- **AD §3.4** 角色驱动工作台装配 + §3.4 评审 L1 最小化边界
- **AD §5.2** 量子化混合调度（1µs 量子）
- **AD §8.1** 确定性 D0
- **AD §8.4** 安全沙箱
- **AD §12** 风险清单 R1~R12
- **需求 FR-01~12 + FR-23** 13 条 P0
- **需求 §3.2** 智能灯 E2E-01 业务主线
- **LLD §2.5** 契约层定义（contract crate 唯一来源）
- **M-12 / M-13 / M-14 / M-06 / M-15 / M-18 / M-19 / M-08** 模块详细设计

**代码审查报告**（`docs/reviews/`，v0.8.1 起建立）：

| 文件 | 覆盖范围 | 闭环状态 |
| --- | --- | --- |
| `M-14_BusMemoryMap_Review.md` | M-14 总线 16 项 | 模块内 100%；#6 / #9 跨模块挂账 |
| `M-15_PeripheralModel_Review.md` | M-15 外设 19 项 | 模块内 100%；#6 / #11 跨模块挂账 |
| `M-18_Probe_Review.md` | M-18 探针 15 项 | 13/15；#10 / #12 挂账 |
| `CROSS_MODULE_DEBT.md` | **跨模块欠账台账**（23 条 · 5 阻断） | **跨模块问题的唯一追踪入口** |

---

## 11. 仓库与命令

```bash
# 验证
source $HOME/.cargo/env
cd /Users/zyzs/lczyzs/code/riscv_sim
cargo test --workspace          # 112 passed / 0 failed（lib 107 + integration 5）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# 提交
git status                      # 改动一览
git add -p                      # 交互式暂存
git commit -m "feat(m12): ..."  # conventional commits

# WASM 构建
tools/build-wasm.sh             # 输出 front/assets/pkg/

# 前端启动
python3 -m http.server 8080 --directory front
```

**代码审查与跨模块欠账**（`docs/reviews/`）：

- 模块内部问题 → 对应模块报告（`M-14` / `M-15` / `M-18`）
- **跨模块问题 → `docs/reviews/CROSS_MODULE_DEBT.md` 台账**（按修复归属模块组织，含阻塞拓扑与处理批次）
- 当前台账：23 条 · 5 阻断级 · 关键路径最靠前的是 **D-04（M-13 x0 未硬连线）**——Q7 决议「Spike 对拍必做」，越晚修 golden 用例返工面越大

---

## 12. 不在本文档范围

- 详细 API 文档（各模块 `///` doc comment + 详细设计文档）
- Spike golden 对拍脚本（`tests/golden/`）
- 端到端测试夹具（智能灯固件）
- CI 配置（`.github/workflows/` 或等价）
- 部署脚本（`tools/deploy.sh`）
