# 独立检视轮 · 契约快照 DTO（2026-10-20 收口轮）

> **轮次**：收口轮（分支合 main 前）
> **被检视对象**：分支 `feat/contract-snapshot-dto`，基线 `main` = `5b062ef`，检视时 HEAD = `7c6d523`
> **范围**：本轮新增的**契约层可序列化快照 DTO**（`crates/contract/src/snapshot.rs`）+ M-12 转换层 + M-17 接线 + 三份 LLD + 全部数字声明
> **组织方式**：3 路检视员**互不可见**（各自独立 worktree / 独立探针 / 独立变异沙箱），各自独立跑门禁与变异测试
> **判定**：**三路全部 FAIL**

---

## 0. 检视员与范围划分

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

## 1. V-A 报告（契约层）

### 1.1 判定与门禁

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

### 1.2 「可序列化」卖点是否仍成立

**成立**。`serde` 是**非可选**依赖，DTO 的 `Serialize`/`Deserialize` derive 与 JSON 格式无关——探针实测 `EngineSnapshotDto` 在 `default-features = false` 下仍满足 `Serialize + DeserializeOwned`；`json` feature 只提供 JSON 格式。

JS 镜像**现在不需要**：`grep -rn "EngineSnapshotDto\|snapshot_full\|restore_full" front/` → **0 命中**；`front/lib/message.js` 自述只镜像 `protocol::Message`。

### 1.3 发现清单

**🔴 阻断级：未发现。**

#### 🟠 高危

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

#### 🟠-2 `validate()` 对两个「真相源」完全失查

- `state.event_queue_head` 与顶层 `next_seq` 不一致 → 不报
- 内层 `SimState.schema_version` 与外层 `EngineSnapshotDto.schema_version` 不一致 → 不报
- `SimState.schema_version` **全仓库无人校验**

实证：`2a event_queue_head=999 vs next_seq=3 -> Ok(())` / `2b state.schema_version=7 vs outer=1 -> Ok(())` / `3h state.schema_version=99 in JSON -> deserialize=Ok(99) validate=Ok(Ok(()))`。

**为什么是高危**：`validate()` 的 doc 自述用途是「快照可能来自**不可信存储**，畸形队列必须被拒绝——否则确定性保证（FR-12）**在无人察觉的情况下**丢失」。而它恰恰漏掉了唯一能暴露「存进来的快照不是这份引擎写出来的」的那个字段。

更危险的一条路径：`SNAPSHOT_SCHEMA_VERSION` 是**编译期常量**，生产端永远写自己。若有人改了 `SimState` 的形状并只把 `SimState.schema_version` 顶到 2（很自然，因为它就在那个结构体里），外层仍是 1 → 老读者 `validate()` 通过 → 而 serde **默认忽略未知字段** → 状态被**静默误读**。

**另一处**：`event_queue_head` 与 `next_seq` 分歧的可达后果是**推演**（in-tree 的 `Engine::restore_full` 恰好用顶层 `next_seq` 并有 `floor` 兜底，**把这个洞掩盖了**，所以现有测试永远抓不到它）。契约层无法为第三方 backend 兜底这一点是确定的。

#### 🟡 中危

| # | 问题 | 实证 |
| --- | --- | --- |
| 🟡-1 | 本分支新引入 `cargo test -p riscv_sim_contract --no-default-features` 编译失败 | main 通过 / HEAD 3 个 E0433 |
| 🟡-2 | 落盘 JSON 形状**无任何测试钉住** | 变异 `Commit` → `CommitWrite`（契约 + 内核同步改，语义等价，纯线格式破坏）→ `passed=392 failed=0` **GREEN**。原因：唯一形状测试是子串断言 `json.contains("commit")`，而 `"commit_write".contains("commit")` 为真。**对照实验**：字段名 `at_ps` → `due_ps`（测试不改）→ **立刻编译失败** `error[E0609]` ⇒ 字段名被钉住、**变体名完全没被钉住** |
| 🟡-3 | 三个 `at_ps` 字段的 doc 完全同义，但**合法地不相等** | 引擎把队列项到期时刻归一化到 `max(at_ps, now_ps)`，而**载荷里的 at_ps 保留原值**。探针：`QueuedEventDto.at_ps = 1000` / `payload at_ps = 0` / `validate() = Ok(())`。且两个字段**语义不同**：`PeripheralTick` 载荷的 at_ps 在 `dispatch` 里是**活的**，`TimerExpiry` 的是**死的**。未来实现者极可能「顺手统一」，那会改变行为 |
| 🟡-4 | `snapshot_full` 默认实现返回软 `Ok(None)`，留出一条**静默不还原**的路径 | 探针（能还原、不能取快照的后端 + 最自然的 `if let Some(dto) = b.snapshot_full()? { b.restore_full(&dto)?; }` 写法）：`restore_full was actually called? false` → **整条还原被静默跳过**。这**正是 D-24 描述的失败模式**。`restore_full` 的大声失败只能保护「已经走到 restore_full」的调用方，保护不了这条路径 |

#### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| 🟢-1 | `validate()` 的**规范序 `seq` 平局裁决**无任何测试覆盖 | 变异 `key <= prev` → `key.0 < prev.0` → **GREEN** |
| 🟢-2 | `model.rs:164-170` doc 事实过期：「the contract has no room for the queue itself」——本分支新增的 snapshot 模块恰恰给了契约层装队列的空间 | `sed -n '162,172p'` |
| 🟢-3 | `ErrorFamily::Snapshot` 的 doc 仍只写 "(M-20)"，但该族现在也承载契约层 DTO 校验码 | `crates/contract/src/error.rs:60-61` |
| 🟢-4 | `SNAPSHOT_FAMILY` 常量在两个 crate 里**重复定义**同值 | `contract/src/snapshot.rs:48` 与 `kernel/src/m20_snapshot/mod.rs:83` |
| 🟢-5 | `EngineSnapshotDto::default()` 的 `instruction_budget = 0`，与引擎默认 `DEFAULT_INSTRUCTION_BUDGET = 10_000` 不一致 | `default DTO: budget=0`；`2g instruction_budget=0 -> Ok(())` |
| 🟢-6 | `validate()` 不检查「队列最早到期时刻 ≥ `state.virtual_time_ps`」 | `2e event at_ps=2000 < virt_time=100000 -> Ok(())` |

### 1.4 变异测试表（V-A 自测，15 项）

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

### 1.5 V-A 的未验证项（明示）

前端真机、WASM 构建、M-20/M-23 真实持久化实现、spike 黄金对拍均未跑；M-12 转换层仅做**只读对照**，其内部正确性只被间接覆盖（D-30 仍开）。

---

## 2. V-B 报告（内核接线与测试有效性）

### 2.1 判定

**FAIL**。D-24 原始缺陷已实证闭环，无功能回归，无门禁失败，但**测试有效性存在系统性缺口**。

### 2.2 变异自检的独立复现

实测 `cargo test --workspace --no-fail-fast`：

| 变异 | 文档声明红测数 | **实测** | 判定 |
| --- | --- | --- | --- |
| M-1 还原不恢复队列 | 4 | 集成 4（`@Engine::restore_full` 则是 kernel-lib 5 + 集成 4 = 9） | ✅ |
| M-2 DTO 丢队列 | 2 | **6**（kernel-lib 1 + 集成 5） | ❌ 少报 |
| M-3 默认 `restore_full` 静默成功 | 1 | 1 | ✅ |
| M-4 M-17 谎称不支持 | 1 | **2** | ❌ 少报 |
| M-5 `validate()` 去 seq 查重 | 1 | 1 | ✅ |

**结论方向成立**（5 项确实全部变红），但**条数在任何口径下都复现不出**。根因：**`cargo test` 默认 fail-fast**，前一个 test target 失败会中止后续 target，红测条数被系统性截断。

### 2.3 发现清单

**🔴 阻断：未发现。**

#### 🟠 高危

**H-1 落盘加载路径不做输入校验——契约层明文承诺的「畸形快照必须被拒绝」在唯一还原入口上不成立，且零测试覆盖**

契约层原文（`snapshot.rs:151-157`）：

> A snapshot may come from an **untrusted store** (IndexedDB in the browser, a server-side store in P1), so a malformed queue must be **rejected rather than silently loaded** — otherwise the determinism guarantee (FR-12) is lost without anybody noticing.

现实：`m12_engine/snapshot.rs:141-143` `restore_dto` 明写「还原**不做**校验」；`m17_backend/mod.rs:103-107` `SelfIssBackend::restore_full` **既不校验、文档里也只字未提校验**。

全仓证据：`grep -rn "\.validate()" --include=*.rs crates/` → **仅 6 处契约单测 + 1 处集成测试**，**无任何生产代码调用点**。

实证：`restore_full(dto{schema_version:12345, queue:[1 条]})` → 返回 **`Ok`**，队列被还原；`Engine::restore_dto` 接受 `schema_version=99`、重复 seq、非规范序队列，**全部静默接受**（同样输入 `validate()` 则正确报错）。

**变异证据（最硬的一条）**：在 M-17 `restore_full` 首行加 `snapshot.validate()?;` → **0 条用例变红** ⇒「畸形 DTO 被拒绝」这个行为**没有任何测试兜住**。

**影响**：未知 schema 版本被**静默按 v1 解释**（字段重命名/重排即语义错乱）；重复 `seq` 的事件在 `BinaryHeap` 中 `Ord` 相等，出队次序由堆布局决定而非规范序 ⇒ **FR-12 确定性在无任何报错的情况下丢失**。这恰是本轮新开的「确定性快照首次可以落盘」最需要防的场景。

#### 🟡 中危

| # | 问题 | 实证 |
| --- | --- | --- |
| M-1 | `SelfIssBackend.state` 是死字段；新增「保持同步」注释描述了代码并不维持的不变式；`reset()` 仍是空操作 | `m17_backend/mod.rs:24` 声明，全文件 **3 处写、0 处读**；变异 B7（删同步）→ 0 red；变异 C2（彻底删字段 + 全部写入）→ 0 red 且 `clippy -D warnings` 仍零告警 ⇒ 字段完全无用。`reset()` 探针：reset 后引擎 `pending=2`、`snapshot_full().queue=2`、`next_seq=2` ⇒ **reset() 对外完全不可观测**。新注释「so the two never disagree」在 `reset()` 之后**恰恰不成立** |
| M-2 | 把已知缺陷钉成活动断言，会挡住将来的正当修复 | `m12_m17_snapshot_dto.rs:78-96` 断言 partial 路径必然 `[1,2]`。若将来有人修好 `Engine::restore`，该用例变红，修复者极易误判为「引入回归」而回退修复。且该用例**未**断言台账的另一半「从快照时刻出发的运行只有 A」 |
| M-3 | 变异自检的「变红条数」不可复现 | 见 §2.2。文档数字 4/2/1/1/1 在两种口径下都复现不出（4 只在「仅数本次新增的 4 条集成用例」这一口径下成立；2 无法复现） |
| M-4 | 台账 §1.1 的欠账构成与 D-24 闭环不自洽 | 实际 D-25~D-35 共 **11 条**登记在册且未闭环；改动前 19 = 12 + 7，闭环 D-24 后应为 18 = **11 + 7**；文档写成 12 + 6，凭空挪了一条。「4 闭环 = 1 阻断 + 2 高危 + D-24 根因」**重复计数**——D-24 的「来源」列写的就是「检视 V2 · F-1」 |
| M-5 | DTO 路径对「虚拟时钟 / pending_writes / 指令预算 / next_seq」**完全不敏感** | 变异 B3（`restore_full` 丢弃整个 `state`）→ 仅 **1 red**（且是**既有** M-12 单测），**本次新增的 7 条集成用例 0 条变红**；B2（丢 `instruction_budget`）→ **0 red**；B5（丢 `pending_writes`）→ **0 red**；B8（忽略 `next_seq`）→ **0 red**。文档明写「虚拟时间、每量子指令预算全部还原」，但这两项**在整条链路上无任何断言** |

#### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| L-1 | 「M-17 +41 行」与实际 **+42** 不符 | `git diff --numstat` → `42 0`（合计 591→633 是对的，只是增量写错 1） |
| L-2 | `EngineSnapshotDto::default()` 的 `instruction_budget = 0`，而 `Engine::new` 是 10 000 | 默认 DTO 不代表任何真实引擎快照 |
| L-3 | 「队列顺序在往返后保持」是**结构性保证**，不是本次修复带来的 | 变异 B13（`from_ordered_vec` 乱序入队）→ **0 red**，出队顺序不变——`EventQueue` 是 `BinaryHeap`，出队序由 `QueuedEvent::Ord`（`(at_ps, seq)`）决定，与入队顺序无关。⇒ **同刻定序不必另写测试**；DTO 里 `queue` 数组的书写顺序其实无关紧要（`validate()` 却又要求它规范——偏严但无害） |
| L-4 | `Engine::bus()` 访问器**零用例使用** | `intc()` 有 5 条用例依赖（变异 B6 → 5 red）；`bus()` 属「为对称而加」的公开 API |
| L-5 | `state.event_queue_head` 在 DTO 中是**非权威字段** | 还原时被 `next_seq` 覆盖，`validate()` 不检查二者关系 |

### 2.4 V-B 的变异覆盖漏洞清单（0 条用例变红）

1. **B2** 还原丢弃 `instruction_budget` → DTO 路径的预算还原**零断言**，而文档两处明写「每量子指令预算全部还原」
2. **B4** 去掉 `next_seq` 防御性抬升 → DTO 路径的 **D-17（还原后不撞号）零覆盖**（既有 `restore_does_not_reuse_a_queued_seq` 只覆盖 partial `restore`）
3. **B5** DTO→Engine 丢弃 `pending_writes` → 转换层对 `pending_writes` **零覆盖**（单测夹具该字段为空）
4. **B8** 还原忽略 DTO 的 `next_seq` → 引擎还原后的 `next_seq` **无任何断言**（DTO 层有，引擎层无）
5. **C3** 给 `restore_full` **加上** `validate()` → **「畸形快照被拒绝」这一契约承诺零覆盖**（H-1 的直接证据）

（B7 / C2 为死代码证明、B13 为结构性良性，不计入漏洞。）

### 2.5 V-B 的未验证项（明示）

真实浏览器 / WASM 构建未跑；`m18_probe` / `m13_iss` 等其他模块的回归深度未做跨模块变异；**H-1 的「修复优先级」是 V-B 的判断**——若协调者决定把校验责任下放到 M-20/M-23 持久化层（当前是 stub），H-1 可降级为「新增台账条目」；但**在此之前，台账「确定性快照首次可以落盘」与「高危已全部闭环」的表述应加限定**。

---

## 3. V-C 报告（跨模块影响与文档一致性）

### 3.1 判定

**FAIL**。

### 3.2 发现清单

**🔴 阻断：未发现。**

#### 🟠 高危

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

#### 🟡 中危

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

#### 🟢 低危

| # | 问题 | 实证 |
| --- | --- | --- |
| L-1 | `crates/kernel/Cargo.toml` 的注释「the shipped WASM binary stays free of serde」不准确 | 契约层**本来就**是 serde 的硬依赖。准确表述应为「dev-dependency 只让**测试**能用 `serde_json`；serde 本身在契约层早已是硬依赖」。**实测代价为 0 字节**（出货 wasm 293 B 两侧相同；DTO 用上时边际 +481 B） |
| L-2 | `tools/build-wasm.sh` 在 main 与 HEAD 上都跑不通 —— pre-existing | `wasm-pack build --target no-modules ... crates/kernel` → `Error: Ensure that you have "wasm-bindgen" as a dependency in your Cargo.toml file`。⇒ AGENTS.md 记载的 WASM 构建路径当前是坏的 |
| L-3 | `EngineSnapshotDto::default()` 是一个「**合法但空**」的快照 | `validate()` 对它返回 `Ok(())`。宿主若把 `default()` 落盘，得到的与「真实快照」在类型上不可区分 |
| L-4 | 「5 项变异自检」的红测条数在 2 处被少报 | M-2 声明 2 实测 5；M-4 声明 1 实测 2。方向保守，实质结论成立 |

### 3.3 V-C 明确记录的「未发现」

- **无夹带的无关改动**：`git diff --numstat main..HEAD` 12 个文件全部与本次目标相关（契约 3 + kernel 4 + Cargo.lock/Cargo.toml 2 + 文档 3），`front/` **0 文件改动**
- **无对既有 API 的破坏**：两个新方法均带默认实现；全仓仅 2 个实现者；`SimState` / `EngineSnapshot` / `EventQueue` 的既有字段与方法签名**一字未改**
- **无前端回归**：119 用例 / 21 suite 全绿
- **无服务端影响**：`crates/server/` 零改动，编译通过
- **无格式 / lint 问题**
- **D-24 根因确实闭环**

### 3.4 V-C 的数字核对表（节选，全部逐格复核）

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

### 3.5 V-C 揭示的 LLD 内部矛盾（**需协调者裁决**）

- LLD **总纲**表12/表84/表85 说 M-23 `saveSnapshot/loadSnapshot` 输入是「**SimState + DebugMeta**」，而 M-20/M-23 **详细设计**§2.1.1/§9.1.1 说输入是「`Snapshot{cpMeta, m13State, m14Map, m15Periph}` 完整单对象」—— **总纲与详细设计互相矛盾**
- 总纲表12 说 `SimState` 应含「内存页、外设状态」，契约里现行的 `SimState` **两者皆无**（pre-existing 欠账，新 DTO 沿用了这个缺口）

⇒ 因此「DTO 该对齐哪一边」不能只答「对齐 M-20 LLD」，**需要协调者先裁决 LLD 内部的这个矛盾**。

---

## 4. 闭环对照表

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

### 4.1 闭环统计

| 口径 | 项数 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| V-A | 12 | 11 | 1（🟢-6 → D-39） |
| V-B | 11 | 11 | 0 |
| V-C | 15 | 13 | 2（L-2 → D-38 / L-3 → D-40） |
| **去重后** | **37** | **34** | **3** |

> V-B H-1 与 V-C H-2 是**同一条缺陷**（两路独立发现），去重后 38 → 37。
> 其中 2 条「闭环但带残留」的条目（V-A 🟠-1 → D-36、V-C H-1 → D-37）——缺陷本身已处置，**剩余工作量另行登记**。

### 4.2 闭环证据

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

## 5. 台账欠账登记

| ID | 级别 | 来源 | 摘要 |
| --- | --- | --- | --- |
| **D-36** | 高危 | V-A 🟠-1 | 快照 DTO 不含外设（M-15）与中断控制器（M-16）状态 |
| **D-37** | 高危 | V-C H-1 | 契约 DTO 与 M-20/M-23 已冻结的 `Snapshot` 结构不一致，且 M-20/M-23 未接线；**含 LLD 总纲 vs 详细设计的矛盾待裁决** |
| **D-38** | 中危 | V-C L-2 | `tools/build-wasm.sh` 在 main 与 HEAD 上都跑不通（AGENTS.md 记载的构建路径是坏的） |
| **D-39** | 低危 | V-A 🟢-6 | `validate()` 不检查「队列最早到期时刻 ≥ `virtual_time_ps`」 |
| **D-40** | 低危 | V-C L-3 | `EngineSnapshotDto::default()` 是「合法但空」的快照，宿主无从拒绝 |

---

## 6. 方法论沉淀

1. **变异测试是本轮唯一能发现「文档说做了、代码没做」的工具**。V-B 的 C3（在 `restore_full` 加 `validate()` → **0 red**）与 V-A 的 m05b（变体改名 → **392/392 全绿**）都是**加法**方向的变异——不是「删功能看是否变红」，而是「加正确性看是否有人守」。这类盲区用常规「删功能」式变异**测不出来**。
2. **fail-fast 会系统性截断红测条数**。同一变异 fail-fast 下 5 条、`--no-fail-fast` 下 9 条。**所有变异计数必须显式写明口径**，否则下一个检视员会卡在同一处（V-B M-3 即为此而开）。
3. **多条检视员独立撞上同一条缺陷，是缺陷「足够显眼」的强信号**。V-B H-1 与 V-C H-2 在互不可见的条件下各自被发现，且 V-C 额外给出了 `BinaryHeap` 的 `Ord` 只看 `(at_ps, seq)` 这一根因。
4. **过度承诺比缺陷更难发现，也更贵**。V-A 🟠-1 与上一轮 F-1 是**同一类错误**——契约 doc 承诺了它兑现不了的语义。它不会让任何测试变红，只会在未来的实现者手里变成事故。
5. **「文档数字」本身就是需要被检视的产物**。V-C 用一张表逐格复核出 8 处失真数字（含 pre-existing 的），且其中「合计行不自洽」是**逐格重算**才暴露的——肉眼扫读看不出来。
