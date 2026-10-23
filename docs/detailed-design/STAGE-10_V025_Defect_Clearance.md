# STAGE-10 · v0.25 冻结规格 · 取指路径对齐 + 前端补洞

> **状态**：**已冻结**（2026-10-23）—— 本批的范围、裁决、验收清单与门禁以本文档为准。
> **上游**：`STAGE-7`（DWARF 行表 + 访存异常陷阱化 · D-99 的数据侧陷阱化）、`STAGE-8`（osz RTOS）、`STAGE-9`（v0.24 遗留）。
> **下游**：`STAGE-11`（内存图通用化 A 批 · D-123）—— **本批是它的前置**（取指宽度语义必须先定死）。
> **台账**：`docs/reviews/REVIEWS.md`（D-86 / D-103 / D-33 / D-88 / D-13 / D-123）。

---

## §0 背景与范围

### 0.1 本批的一句话

把**两处真缺陷**修掉，再把**两个用户可见的洞**补上。

v0.24 批让「反汇编面板显示源码行」「断点停在调度器」「shell 能打字」都通了。但复查发现：

1. **取指侧还有一处「面板显示的指令，访客执行不了」**（D-86 · P1）—— M-13 的执行取指**恒读 4 字节**，M-11 的反汇编取指**先 2 后 4** ⇒ 映射区**末端**只有 2 字节的压缩指令时，反汇编**能解出来**、访客取指却报 `E_BUS-01` ⇒ **面板与执行不一致**。
2. **取指未映射与参考侧分歧**（D-103 · P2）—— 数据侧的未映射访存已在 v0.22 陷阱化（cause 5 / 7），**取指侧**仍是 `E_BUS-01` 原样上抛，而 RISC-V 规定取指访问故障的 `mcause` 是 **1**。
3. **前端落地视图是空面板**（D-33）—— 嵌入式角色的落地视图 `embedded` **没有 M-02 组件** ⇒ 打开应用**第一眼**看到的是「嵌入式工作台」标题 + **空正文**。
4. **前端「导出」没有「下载」动作**（D-88 · P2）—— 导出结果只落在 `<pre>` 里，用户得**手动复制** ⇒ M-11 LLD 的「分块下载」措辞未兑现。

### 0.2 本批做的四件事

| # | 事情 | 欠账 | 性质 |
| --- | --- | --- | --- |
| 1 | 取指宽度：先读 2 字节判宽度，压缩指令**只读 2 字节** | **D-86** | 真缺陷（面板与执行不一致） |
| 2 | 取指未映射 ⇒ 架构陷阱 **cause 1**（`mtval` = 取指地址） | **D-103** | 真缺陷（与参考侧分歧） |
| 3 | 落地视图 `embedded` **补一个 M-02 组件** | **D-33** | 用户可见的洞 |
| 4 | 导出**补下载动作**（`Blob` + `<a download>`） | **D-88** | 用户可见的洞 |

### 0.3 本批**明确不做**的事（9 条）

| # | 不做 | 为什么 |
| --- | --- | --- |
| 1 | **D-123 内存图通用化**（A / B 批） | 下一批；且**依赖**本批先把取指宽度语义定死 |
| 2 | **D-13** 字段可见性收紧 | **跟着 D-123-A 一起做** —— A 本来就要改 `Region.target` / `RamBackend.bytes` ⇒ 单独做会被推翻 |
| 3 | TX 捕获环进会话快照 | 跟着 D-123-A —— 与 `mem_pages` 语义变更是**同一次**破坏性变更升版 |
| 4 | **D-94** S 扩展 / PMP / 计数器 | 工作量 ≈ 实现半个特权级 ⇒ 独立一批 |
| 5 | **D-114** M-17 `step_n` 的同形状活锁 | 与本批无交集 |
| 6 | **D-115** watchdog 处置链路 | **依赖 M-22 HMS**（触发后要报 `HMS_FATAL`）⇒ HMS 还没做 |
| 7 | **D-86 的 M-11 侧** | M-11 的取指口径**本来就是对的**（LLD 11-1：必须真的只读 `size` 字节）⇒ **不改 M-11** |
| 8 | 取指侧 **cause 0**（instruction address misaligned） | 已有（`TrapCause::InstructionAddressMisaligned`）⇒ 不动 |
| 9 | 3D 引擎 / 服务端形态 / IndexedDB 持久化 | P1 范围 |

---

## §1 裁决

### 裁决 A · 取指宽度：**总线侧**新增「一次取指」入口，**先 2 后 4**，**只 fire 一次 hook**

**口径**（与 M-11 的 `fetch_instruction_code` **同源**，不是新口径）：先读 2 字节，低 2 位 ≠ `0b11` ⇒ 压缩指令（2 字节）；否则补读 4 字节。

**落点**：**M-14 新增 `Bus::read_fetch_instruction(addr) -> Result<u32, PlatformError>`**；M-13 的取指改调它。

⚠️ **为什么落在 M-14 而不是 M-13**：若 M-13 直接调两次 `read_fetch` ⇒ **取指事件变成两个**（总线读计数翻倍 · 探针轨迹多一条 · hook 触发两次）⇒ 破掉「**一次取指 = 一个事件**」这条不变式 ⇒ 也破掉 D-46 已经钉住的判别力。

⚠️ **宽度按实际报**：压缩 ⇒ `BusWidth::Half`；32 位 ⇒ `BusWidth::Word`。事件的宽度 = **指令宽度**（2 字节的探查是实现细节，不单独成为事件）。

⚠️ **已核实的两处影响**：
- 观察点 hook **显式跳过 `is_fetch`**（`m17_backend/mod.rs:876`）⇒ **观察点不受影响**；
- M-18 的测试用**合成**的 `BusAccess`（`make_bus_access`）⇒ **没有测试断言真实取指的宽度** ⇒ 本批**新增**一条把新口径钉住。

⚠️ **32 位指令的物理读取**是「先探 2 字节 + 再读 4 字节」（共 6 字节），但**逻辑上仍是一次取指** ⇒ 事件数不变、事件宽度 = 指令宽度。

⚠️ **跨区取指不变**：32 位指令若跨两个**相邻** region，`read_inner` 的 `read_split` 路径照旧（探查与补读都走 `read_inner`）。

### 裁决 B · 取指未映射 ⇒ 架构陷阱 **cause 1**

- `TrapCause` 新增变体 `InstructionAccessFault`，`mcause() => 1`。
  - ⚠️ **变体↔码的映射只有一处**（`executor.rs` 的 `mcause()`）⇒ 只加这一处，不在别处复制字面量（项目规矩「一个约束一个落点」）。
- 判据**沿用**数据侧那条：`is_unmapped_access`（family == `Bus` **且** scenario == `E_BUS_01_UNMAPPED`）⇒ **「未映射」与「其它总线错误」仍然分得开**：其它错误（外设未接线、区域配置错误…）**原样上抛**，不被误判成 cause 1。
- `mtval` = **取指地址**（`state.pc`）。
- ⚠️ **依赖裁决 A**：没有 A 的话，映射区末端的**压缩指令**会被读 4 字节而失败 ⇒ **误报 cause 1**（一条本该执行的指令变成了陷阱）⇒ **A 必须先落**。

### 裁决 C · 落地视图：**给 `embedded` 补一个 M-02 组件**

⚠️ **已核实的形状**：`createPanel` 建出 `<section data-view-panel="embedded">` + `<h2>嵌入式工作台</h2>` + `<div class="view-body" data-view-body="embedded">`；M-02 注册的六个组件是 `product` / `debug` / `regmem` / `waveform` / `source` / `terminal` ⇒ **没有 `embedded`** ⇒ 渲染事件 `viewId: "embedded"` **找不到接收者** ⇒ **标题在、正文空**。

⚠️ **D-33 给的两条路**（补组件 / 从落地视图列表去掉）**本批选「补组件」**：
- 去掉它 ⇒ 嵌入式角色**没有落地视图** ⇒ tab 条上的「嵌入式」标签变成死标签（点了落到 `fallback`）⇒ 得同时改 tab 条与 `ROLE_DEFAULT_VIEW` ⇒ **改动面更大、用户拿到的信息更少**；
- 补组件 ⇒ 嵌入式角色拿到一个**概览页**（会话状态 + 子视图入口），六个子视图的入口从一页可达。

**组件的最小内容**（验收只看这几项，不规定实现）：
1. 标题与**连接状态**（连接 / 未连接）
2. **当前 PC** 与**虚拟时间**
3. **子视图入口**（调试 / 寄存器内存 / 波形 / 源码）—— 四个入口至少各有一个可点的跳转

### 裁决 D · 导出：**补下载动作**（不是改 LLD 措辞）

⚠️ D-88 给的两条路（补下载 / 把 LLD 措辞改成「展示」）**本批选「补下载」**：LLD 的「分块下载」是**已经写下的承诺**，改文档比兑现承诺便宜的做法是不改。

- 做法：`Blob` → `URL.createObjectURL` → `<a download="...">` → 点击后 `URL.revokeObjectURL`。
- ⚠️ **NFR-04（XSS）**：文件名与内容都走 `escapeHtml`；`download` 属性用 `setAttribute` 设置（**不**把未转义的值拼进 HTML 字符串）。
- ⚠️ **文件名由内容派生**（例如 `trace-execution.csv` / `trace-waveform.vcd`），**不是**写死的同一个名字。
- ⚠️ **`<a download>` 是「下载动作」的产物，不是面板里常驻的元素**（落地时由 Lane B 实测确定，写在这里免得下一个人按字面理解）：
  - **面板里常驻的是下载动作**（按钮）——因为 object URL **不能常驻**：裁决 D 同时要求「`download` 用 `setAttribute` 设置」与「点击后 `revokeObjectURL`」，若把 `<a download>` 常驻在面板里，它的 `blob:` href 就会**一直有效**（URL 不释放 ⇒ 泄漏）或**早就失效**（渲染时就释放 ⇒ 点不动）。
  - 点击时**当场**造出 `<a download>`（`href` = `blob:`、`download` = 派生出的文件名）→ 点它 → **下一个宏任务**里 `revokeObjectURL`（同步释放会打断**尚未开始**的下载）。
  - ⇒ 验收判据因此写成「面板里有下载动作，**点它**造出的 `<a download>` 满足…」而不是「面板里有一个 `<a download>`」。

### 裁决 E · 契约层：**不升版**，但**披露**取指宽度取值的变化

- 契约层 `SCHEMA_VERSION` **不动**（5）。
  - 理由：取指宽度的变化落在**总线内部**（`BusAccess.width` 的**取值**从 4 变 2），契约 DTO 的**形状不变** —— `model.rs` 里那个字段本来就是「Access width in bytes (**1, 2, 4**)」，2 **在既有取值域内** ⇒ 不构成破坏性变更。
- 会话快照 `SESSION_SNAPSHOT_SCHEMA_VERSION` **不动**（2）。
- ⚠️ **但必须披露**：访存轨迹 / trace 导出里，**压缩指令的取指条目宽度会从 4 变成 2** ⇒ 这是**可见的行为变化**，要在 PROGRESS 与台账里写明。

### 裁决 F · 黄金语料：**新写一条 `local unmapped_fetch`**

- **形状照抄** `tests/golden/local/unmapped_load.S`（同一套自包含写法）：guest 自己检查 `mcause == 1` / `mtval == 取指地址` / `mepc == 出错指令`，**三项全对才 PASS** ⇒ **判决在比较本身**（一个只问「两边都停了吗」的裁判，对两个都 abort 的平台也是绿的）。
- ⚠️ **必须自包含**（不 include riscv-tests 源码）⇒ 上游树变形也不受影响（同 `canary/fail_3.S` 的规矩）。
- corpus 加一行：`local unmapped_fetch rv32i_zicsr_zifencei active csr`。
- ⚠️ **riscv-tests 里没有这一条**：`rv32mi/ma_fetch.S` 测的是**错位**取指（cause **0**），**不是**未映射取指（cause **1**）⇒ **必须自己写**。

### 裁决 G · lane 切分：**两条**，D-86 与 D-103 **同一条 lane**

⚠️ D-103 的台账原文说要「单独做、单独对拍」—— **「单独对拍」本批满足**（它有**自己的一条黄金语料** `unmapped_fetch`，与 `unmapped_load` / `unmapped_store` 互不冗余：RISC-V 把取指 / 读 / 写三个访问故障编号成 **1 / 5 / 7**，用同一个码会让平台在**其中两条上**与参考侧一致、在**另一条上**分歧 ⇒ **一个编号一条语料**才让这个分裂可观察）。

⚠️ **但它与 D-86 必须在同一条 lane**：两者改的是**同一个函数**（M-13 的取指路径），且 **D-103 的正确行为依赖 D-86 的结果**（没有 A，末端压缩指令会被误报成 cause 1）⇒ 拆成两条并行 lane 只会得到「两个都改同一个文件」的冲突。

| lane | 拥有（文件所有权） | 做 |
| --- | --- | --- |
| **Lane A · 取指路径** | `crates/kernel/src/m13_iss/**` · `crates/kernel/src/m14_bus/**` · `crates/kernel/tests/**` · `tests/golden/local/**` · `tests/golden/corpus.txt` | 裁决 A + B + F |
| **Lane B · 前端** | `front/**` · `tools/browser-verify.mjs` | 裁决 C + D |

⚠️ **两条 lane 的文件所有权零交集** ⇒ 可并行施工、独立 worktree、独立 `CARGO_TARGET_DIR` ⇒ 0 merge 冲突。

---

## §2 验收清单

### 2.1 Lane A · 取指路径（A-1 ~ A-7）

- [x] **A-1** 映射区**末端**只有 2 字节的压缩指令 ⇒ **它被执行**
      > 证据：`crates/kernel/tests/m13_fetch_path.rs` 的
      > `a_compressed_instruction_in_the_last_two_bytes_of_a_region_executes` ——
      > 把 `c.li x8,-16` 放在 RAM 区**最后 2 字节**，访客**真跑到它**：
      > `x8 = 0xFFFF_FFF0`、PC `0xFE → 0x100`（前进 **2**）、总线 `reads=1` `ram_hits=1` `misses=0`
      > ⇒ 不是「反汇编能解出来」，是**执行了**。
      > 另有一条**反向半条**（`a_32_bit_instruction_that_does_not_fit_is_still_a_fault`）：
      > 32 位指令只落 2 字节时**仍然**是取指失败（不能因为宽度探查而放行）。
- [x] **A-2** 32 位指令的执行**不受影响**
      > 证据：`a_32_bit_instruction_still_advances_the_pc_by_four` —— PC `0 → 4`、`x1 = 42`。
- [x] **A-3** **一次取指 = 一个事件**，宽度 = 指令宽度
      > 证据：`one_fetch_is_one_event_and_the_width_is_the_instruction_width` ——
      > **两条**指令 ⇒ **两条**取指事件，宽度依次 `Half` / `Word`，`stats.reads = 2`（**不是 4**）。
      > `a_failed_fetch_is_still_exactly_one_event` —— **失败的**取指也只产生 **1** 条事件
      > （`ok = false`、`err_scenario = Some(1)`）。
      > M-14 侧另有 `read_fetch_instruction_fires_one_event_with_the_instruction_width`
      > 与 `read_fetch_instruction_counts_exactly_one_bus_access`。
      > ⚠️ **根因**：宽度探查走 `read_only`（**纯 peek**：不计次、不 fire hook）⇒ 落点另行补记 ⇒
      > **一次取指 = 一次读派发 + 一次落点 + 一个事件**（总线统计与改动前**逐条相同**，
      > 既有断言 `reads == 1` / `ram_hits == 1` **一条没改**）。
- [x] **A-4** 取指未映射 ⇒ 陷入
      > 证据：`an_unmapped_fetch_traps_with_the_instruction_access_fault` ——
      > `mcause == 1` · `mtval == 0x9000_0000` · PC 跳到 `mtvec`（`0x2000`）。
      > `the_unmapped_fetch_trap_records_the_pc_at_the_trap_entry_in_mepc` ——
      > `mepc == 0x9000_0000`（= 陷阱入口处的 PC）。
      > `the_unmapped_fetch_trap_does_not_advance_the_pc_before_recording_mepc` ——
      > PC **不前进**（先记 `mepc` 再跳陷阱向量）。
      > ⚠️ **`mepc` 的口径**：本平台 `enter_trap` 取 `mepc := state.pc`；参考侧**实测一致**
      > （Spike 报 `mepc = 0x9000_0000`）⇒ 两侧**不分歧**，不登记欠账。
- [x] **A-5** **未映射**才陷阱，**别的总线错误**原样上抛
      > 证据：`a_non_unmapped_bus_error_is_propagated_unchanged` —— 构造「外设未接线」
      > （`E_BUS-05`）⇒ `family = Bus`、`scenario = 5`、`mcause` **保持 0**、PC **不动**
      > ⇒ **不被**误判成 cause 1。
      > M-14 侧：`read_fetch_instruction_reports_a_non_unmapped_error_unchanged`。
- [x] **A-6** 新黄金语料两侧**逐字节**判决一致
      > 证据：`tests/golden/corpus.txt` 增一行
      > `local unmapped_fetch rv32i_zicsr_zifencei active csr`；
      > `tools/golden-run.sh --keep-going` 实测 **68/68 逐例一致**（基线 67 + 新增 1），
      > 该例 `spike=PASS ours=PASS`。
      > ⚠️ **语料本体在参考侧另测过两项**：用黄金工具链编、Spike 跑 ⇒
      > **退出码 0**；把 `mepc` 期望改成 `0` 的**反向对照** ⇒ **退出码 3**
      > （`*** FAILED *** (tohost = 3)`）⇒ `mepc` 那条断言**有判别力**，不是「压根没跑到」。
- [x] **A-7** `mcause` 的 **1** 只有**一个**定义处
      > 证据：`the_cause_1_mapping_lives_in_exactly_one_place` —— **扫全内核源码树**：
      > cause `1` 的字面量映射**恰好 1 处**（在 `crates/kernel/src/m13_iss/executor.rs` 的
      > `mcause()` 里），并核对整个「变体 → 码」映射表**共 11 行**且**都在那一个文件**里
      > ⇒ 「一个约束一个落点」是**可证的**，不是散文。

### 2.2 Lane B · 前端（B-1 ~ B-5）

- [x] **B-1** `embedded` **有** M-02 组件
      > 证据：`front/views/m02_viewcomponents.js` 新注册 `id: "embedded"` 的组件；
      > 前端门禁 `embedded-landing.test.mjs` 的两条（B-1）断言**组件注册表里查得到 `embedded`**
      > 且**落地视图就是 `embedded` 且它有组件**；真机 `active=embedded title="嵌入式工作台"`。
- [x] **B-2** 真机：落地视图**正文非空**
      > 证据：`tools/browser-verify.mjs` 的「正文非空」断言从 `renderChecks`（挂在 `#/regmem`）
      > **移到** `landingChecks` 并改查 `[data-view-body="embedded"]`，
      > 另加**两条实质内容**断言 ⇒ 真机实测
      > `embeddedBodyChars=1563` · `pc="0x00000000"` · `entries=4`
      > `targets=[debug,regmem,waveform,source]` · `connected="1"`。
      > ⚠️ **只判「非空」不够**（白屏也非空）⇒ 断言落到**具体内容**上。
- [x] **B-3** 导出面板有**下载动作**
      > 证据：`front/tests/trace-download.test.mjs` 断言面板里有下载动作、**点它**造出的
      > `<a download>` 的 `href` 以 `blob:` 开头、`download` 属性非空且等于
      > `trace-execution.csv` / `trace-waveform.vcd`（**文件名由内容派生**、两者**不同名**）、
      > 锚点**确实被点了**。
- [x] **B-4** 下载后**释放** object URL
      > 证据：同文件断言 `revokedUrls.length === 1` 且 `revokedUrls[0] === createdUrls[0]`
      > ⇒ 释放的**就是本次**那个 URL（不是别处的一次释放）。
      > ⚠️ 释放时机在点击后的**下一个宏任务**（同步释放会打断尚未开始的下载）。
- [x] **B-5** 前端门禁覆盖 B-1 / B-3 / B-4
      > 证据：新增 `front/tests/embedded-landing.test.mjs`（**12 条 / 4 suite**）与
      > `front/tests/trace-download.test.mjs`（**12 条 / 3 suite**）⇒ 门禁
      > **288 用例 / 50 suite → 312 用例 / 57 suite**，**0 红**。
      > `front/tests/dom-stub.mjs` 补 `click()`（真实 DOM 的 click 事件才是下载的触发点，
      > 缺它会让「下载动作没点任何东西」**静默通过**）。
      > ⚠️ **判别力实测**：把 `exportFileName` 改回写死 ⇒ **6 红**；只改注释 ⇒ **0 红**
      > （统筹方独立复验，还原后 md5 逐字节一致）。

---

## §3 门禁

### 3.1 常规八项（与历批相同）

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --no-fail-fast --lib --tests`
4. `tools/check-front.sh`
5. `tools/golden-run.sh --self-check`
6. `tools/golden-run.sh --keep-going`
7. `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs`
8. `node tools/browser-verify.mjs`

### 3.2 本批专属

| # | 门禁 | 覆盖 |
| --- | --- | --- |
| **S-1** | 新黄金语料 `local unmapped_fetch` 必须是 `active` 且**逐字节**判决一致 | A-4 / A-6 |
| **S-2** | 真机：**落地视图正文非空**（断言改回 `embedded`） | B-2 |
| **S-3** | 变异自检：每条变异**能编译**且**按期望变红**；每条 lane 至少一条**对照**（只改注释）**必须 0 红** | A / B 全部 |

**失败项数必须为 0。**

### 3.3 实测结果（v0.25 收口）

**失败项数 = 0。**

| 门禁 | 实测 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 通过（0 warning） |
| `cargo test --workspace --no-fail-fast --lib --tests` | **1 254** 条全绿（契约 lib **134** + kernel lib **980** + 集成 **123** + runner **17**） |
| `tools/check-front.sh` | **312 用例 / 57 suite**，**0 红**（基线 288 / 50） |
| `tools/golden-run.sh --self-check` | 三个金丝雀全 ok（`canary/fail_3` 两侧 FAIL 3 · `add` 两侧 PASS · `mutated add` 两侧 FAIL 3） |
| `tools/golden-run.sh --keep-going` | **68/68 逐例一致**（mismatch 0 / no-verdict 0 / build-failed 0） |
| `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | 通过 |
| `node tools/browser-verify.mjs`（真 Chrome） | 通过；落地页 **9** 条断言全绿（其中 v0.25 新增的 embedded 三条：`embeddedBodyChars=1563` · `pc="0x00000000"` · `entries=4 targets=[debug,regmem,waveform,source]` · `connected="1"`） |
| `tools/boot-osz.sh`（与 Spike 逐字节对拍） | **VERDICT MATCH** —— 两侧 console 字节流 **31** 字节逐字节相同 |

**本批专属三条**：

- **S-1**（新黄金语料 `local unmapped_fetch` 必须 `active` 且逐字节判决一致）✅ —— corpus 里它是
  `local unmapped_fetch rv32i_zicsr_zifencei active csr`，`--keep-going` 判 `spike=PASS ours=PASS`；
  ⚠️ 另外在参考侧另测了两项：**语料本体**（三项断言）Spike **退出码 0** · **反向对照**（`mepc` 期望改成 0）
  **退出码 3** ⇒ 该断言**有判别力**而不是「压根没跑到」。
- **S-2**（真机落地视图正文非空）✅ —— 真机断言已从 `#/regmem` **改回** `[data-view-body="embedded"]`，
  且不止判非空，还落到**具体内容**（PC 十六进制 + 四个入口标记 + 连接状态）。
- **S-3**（变异自检：每条变异能编译且按期望变红；每条 lane 至少一条对照 0 红）✅ ——
  Lane A **4 条 + 1 对照**（红 2 / 9 / 7 / 2 条，对照 **0** 条）· Lane B **4 条 + 1 对照**
  （红 9 / 2 / 1 / 真机 2 条，对照 **0** 条）；统筹方另在 Lane B 的工作树上独立复验了一条
  变异（「文件名不派生」）⇒ **6 红** 与一条只改注释的对照 ⇒ **0 红**，还原后 md5 与改前**逐字节一致**。

⚠️ **一轮跑出过三项失败（`cargo test` 报 124 / 黄金自检报 NOVERDICT / osz 对拍候选侧零字节）**——
根因**不是代码**：macOS 对**新链接二进制**的首次 `exec` 会做安全评估，
**实测单个二进制的 `--list` 耗 51 秒墙钟却只花 0.012 秒 CPU**（进程停在 `dyld` 阶段、只映射了主二进制）；
**同一二进制第二次 `exec` 只要 0.05 秒** ⇒ 评估完成后即缓存。
⚠️ **扫描期间杀掉进程 ⇒ 评估没做完 ⇒ 下一次又要重来** ⇒ 缓解手段是**串行预热**全部测试二进制
（让每次评估自然跑完），本批据此重跑即全绿。**候选侧输出是零字节而不是错误字节**也可佐证这不是代码缺陷。


---

## §4 变更记录

| 日期 | 事件 | 摘要 |
| --- | --- | --- |
| 2026-10-23 | **STAGE-10 起草并冻结** | v0.25 批：范围（4 件事 + 9 件不做的事）· 裁决 A~G · 验收 A-1~A-7 与 B-1~B-5 · 门禁 8 项 + S-1~S-3 |
| 2026-10-23 | **B-3 判别式措辞订正** | 裁决 D 落地时实测发现：**`<a download>` 不能常驻在面板里**（裁决 D 同时要求「`download` 用 `setAttribute` 设置」与「点击后 `revokeObjectURL`」⇒ 常驻的 `blob:` href 要么**一直有效**即泄漏、要么**早就失效**即点不动）⇒ B-3 的判别式由「导出面板里有一个 `<a download>` 链接」改成「面板里有下载动作（按钮）· **点它**造出的 `<a download>` 满足…」，裁决 D 末尾补了同一件事的说明。⚠️ **规格与实现冲突时改规格，但要写清为什么** —— 否则下一个人会按字面理解去面板里找一个常驻的链接 |
| 2026-10-23 | **`mepc` 口径修正（撤销第一次修正）** | 起草时按「参考侧 `mepc` 与平台**疑似分歧**」写下了「guest 里只断言 `mcause` + `mtval`、不查 `mepc`」，并预告要登记一条新欠账。⚠️ **复核后结论翻转**：那个「疑似分歧」来自一个**手写探针**，而它的 tohost 判决管道本身有缺陷（env 的 `RVTEST_FAIL` 宏含数字标签与探针自己的标签冲突 · `andi` 立即数越界 · `addi` 不能用寄存器源 · `write_tohost` 标签名冲突）⇒ **结论不可靠**。改用**黄金 env 的可靠管线**（`RVTEST_PASS` / `RVTEST_FAIL` 的 tohost 协议）复测：**正变体（`mepc` 期望 = 取指地址）Spike 退出码 0** · **反向对照（`mepc` 期望 = 0）退出码 3** ⇒ 断言**有判别力**、且 **Spike 的 `mepc` 就是取指地址** ⇒ 与本平台 `mepc := state.pc` **一致**、**两侧不分歧** ⇒ **撤销第一次修正**、语料**恢复三项断言**、**不登记欠账**。⇒ **教训**：「参考侧不一致」这个结论**必须有可复现的判别式**（反向对照）才敢拿去做**架构决策**；一个**只会亮红灯的探针**测出的「不一致」，第一嫌疑是**探针自己** |
| 2026-10-23 | **验收全部勾选 + 门禁通过** | A-1~A-7 与 B-1~B-5 **十二条全部勾选**并逐条补上可查的证据（用例名 + 实测值）；门禁 **8 项失败项数 0** + 本批专属 **S-1~S-3**（新语料两侧逐字节一致 · 真机落地视图正文非空且有实质内容 · 变异自检每条按期望变红、每条 lane 至少一条对照 0 红）。**关键 commit**：`1b8cab4`（起草）· `5d0200a`（合 main）· `863285f`（B-3 订正）· `a435390`（三处超时论断订正）· `f49420b`（Lane A）· `f35edf1`（Lane B）· `ea3b621` · `ccbf207`（合 main）· → 叙事见 `docs/progress/PROGRESS.md` **v0.25.0** 段 |
