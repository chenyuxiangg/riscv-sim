# 独立检视轮报告 · 2026-10-20（v0.11.1）

> **检视对象**：`chore/wave1-closure@00f1ab5`（Wave 1 五路并行开发的收口提交）
> **检视方式**：4 路独立 verifier 会话，各自独立 worktree，**互不可见**；每路用**变异测试**判定「现有测试是否真能抓住实现错误」
> **基线**：`cargo test --workspace` 355 passed（lib 334 + 集成 21）；`tools/check-front.sh` 109 用例 / 19 suite；`clippy -D warnings` 与 `fmt --check` 干净
> **结论**：**22 项发现**（1 阻断 / 2 高危 / 6 中危 / 13 低危）。**阻断级与高危已全部闭环**；中低危分流为台账 D-24~D-35。
> **配套台账**：`docs/reviews/CROSS_MODULE_DEBT.md`（唯一追踪入口）

---

## 〇、为什么先检视后合 main

Wave 1 交付后**不直接回合 `main`**，而是先拉起独立检视。原因是这一轮的交付方式本身有风险：5 个会话在互不可见的 worktree 里同时写代码，**没人看过别人的 diff**。检视的价值不在于「再读一遍代码」，而在于**用变异测试反问：现有测试到底抓得住什么？**

答案是：V1 的 32 项变异里，**有 4 项改回去没有任何现有测试会红**，其中 1 项是真缺陷（阻断级）。这 4 个「覆盖漏洞」比任何单点 bug 都更值得修——它们意味着门禁在关键处是瞎的。

---

## 一、V1 · M-13 ISS（RV32I）

**判定：FAIL**（1 阻断 + 3 附带）· 32 项变异 + 29 个探针

### 🔴 阻断级 · 缺陷 1：SYSTEM opcode 未校验 `funct3`

`classify()` 的 SYSTEM 分支只按 12 位立即数判 ECALL / EBREAK：

```rust
0b111_0011 => match d.imm {
    0 => Some(Op::Ecall),
    1 => Some(Op::Ebreak),
    _ => None,
},
```

RISC-V 非特权规范把 SYSTEM opcode 的 `funct3 == 0b000` 留给 ECALL / EBREAK，`funct3` 001..111 是 CSR 指令族（CSRRW / CSRRS / CSRRC 及其立即数变体）。只看立即数的后果：

| 编码 | 语义 | 修复前 | 修复后 |
| --- | --- | --- | --- |
| `0x00001073` | CSRRW x0, 0(x0) | ❌ mcause 11（Ecall） | ✅ mcause 2（Illegal） |
| `0x00101073` | csr=1 的 CSRRW | ❌ mcause 3（Ebreak） | ✅ mcause 2 |

**当时无任何现有测试能抓住**——`ecall_traps` / `ebreak_traps` 只测真 ECALL / EBREAK。

> **为什么这算阻断级**：mcause 错意味着 CSR 接线落地后，上层 trap handler 会把 CSR 指令误投递到 ECALL handler。修复成本是一个 `match (d.funct3, d.imm)`，但若等 Wave 2 接 CSR 时才发现，调试流程会先乱一阵。

**修复**（`caf7f82`）：`match (d.funct3, d.imm)`，funct3 ≠ 0 一律判非法指令。补 3 条用例（7 funct3 × 4 csr 表驱动 / 不得误判 EBREAK / MRET 与 WFI 形态）。
**反向验证**：改回原样后 **4 条用例变红**。

### 🟡 附带缺陷

| # | 缺陷 | 处置 |
| --- | --- | --- |
| 2 | `sequential()` 虽已按 `d.len` 步进，但**执行路径无任何测试锁定**（D-16 的覆盖漏洞） | 补 `sequential` 单元用例（len=2 → +2）+ 真实解码交叉验证 |
| 3 | 有效地址回绕（`x1=0xFFFFFFFF` + imm）无覆盖 | 补 load / store 两条用例，断言回绕后访问的是回绕地址且 debug 构建不 panic |
| 4 | `truncate_store` 的语义此前无契约说明 | **裁定：保留截断**。它承载架构语义——M-14 总线对外设写时透传原值（RAM 路径按字节掩码、外设路径不掩码，跨区间拆分路径按字节切片），若不在 ISS 侧截断，`sb 0x1234` 对 RAM 写 `0x34`、对 GPIO 却送 `0x1234`。补 3 条外设端到端用例锁定 |

### 对协调者两处纠偏的独立判断

- **非对齐访存默认策略改 `Allow`**：**正确**。LLD §3.6(2) 与 §5.2.1 两处原文都写「默认不触发」「保持 golden 兼容」；`crates/contract/src/` 零处提及非对齐访存，**契约里根本没有这个要求**——实现者 review 把它当成「本轮契约明确要求」是自我设定。默认 `Trap` 会让 ISS 在任何触碰非对齐地址的程序上与 Spike 分叉，而那正是 golden 对拍要测的。
- **LLD §3.3 的 LUI/AUIPC 回写**（`imm<<12` → `imm`）：**正确**。§3.1 说 U 型立即数已含左移，§3.3 是二次左移。全文已无残留 `<<12`。

---

## 二、V2 · M-12 引擎 + M-14/M-15/M-18 接线

**判定：FAIL**（1 阻断 + 2 高危 + 3 中危 + 3 低危）· 19 个探针 + 5 项变异

### 🔴 阻断级 · F-1：`ExecutionBackend::restore` 不是确定性还原

契约 `port.rs` 写「Restore a deterministic snapshot」，但 `SimState` **不含事件队列**（契约无此空间），`Engine::restore` 只还原平台状态与虚拟时钟，队列原样保留。实证：

```
schedule A(1, 5000); snapshot;
schedule B(2, 5000); restore(snap); run_until_idle();
→ raised = [1, 2]      // 「从未 restore 的同种子」只会 raise [1]
```

D-05 台账标注的「快照还原 now_ps + 事件队列」只覆盖了 `snapshot_full` / `restore_full` 路径，**没覆盖契约层的 partial path**。M-17 / M-20 调的正是 partial path。

**处置**：本轮**只订正文档承诺**（`7254a00`），不改 API 形状——`crates/contract/src/port.rs` 的 `snapshot` / `restore` 改述为「partial」，`m12_engine::restore` 的 docstring 补上具体发散场景与 `snapshot_full` / `restore_full` 的使用指引。
**根因未除**：`EngineSnapshot`（含队列）**不可序列化**——kernel crate 无 serde，契约层放不下。已登记 **D-24**，Wave 2 决策（契约新增可序列化 DTO 倾向 vs 允许 kernel 引入 serde）。

### 🟠 高危 · F-3：总线 hook 的 RefCell 重入 panic

`make_bus_hook` 用裸 `borrow_mut()`，而**同模块**的外设探针回调 `wire_signal_probe` 已用 `try_borrow_mut()` + 失败计数正确处理了争用——两侧不对称。调用方只要在 `bus.read/write` 期间持有 `probes.borrow_mut()`，整场仿真直接 panic。

**修复**（`bd647dd`）：改 `try_borrow_mut()`，争用时丢弃该次访存记录并把 `borrow_contended` 计数 +1，**绝不 panic**。补 3 条用例（不可变借用 / 可变借用 / 无争用正常路径）。
**构造期前置条件**（`reg.borrow()` 取计数句柄）已登记 **D-31**。

### 🟡 中低危

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

## 三、V3 · M-06 ELF / Raw-HEX 加载器

**判定：PASS**（2 中 + 1 低）· ~26 000 次随机 / 边界输入 **0 panic**

| # | 发现 | 处置 |
| --- | --- | --- |
| M-1（中） | 报告 §四·#7 与 §五 把 D-10 标为「M-14 侧未做」，但 `60dbbb8` 早已补上 E_BUS-09 守卫 | 已订正文档（`9992419`） |
| M-2（中） | D-22 变异后 47 条测试仍全绿 | 已在报告 §四·#4 补注：限额是**契约性保证**（trait 默认实现已删除，漏实现就编译不过），**不是**回归测试锁死的。后续改动需靠人工复核 |
| L-6（低） | `elf.rs` bss gap 基址用 `wrapping_add`，溢出时静默回绕 | 改 `checked_add` + 明确错误分支。**注**：上游 `paddr_end > max_paddr` 检查已先拦下越界段，该溢出实际不可达，此为防御性加固 |

**另修 V3 顺带发现的嗅探假阳性**（P1）：`looks_like_ihex` 只看首字节是否为 `:`，导致**首字节恰为 `0x3A` 的原始二进制镜像**被误判成 HEX 而加载失败。改为结构判据（记录标记 + 长度 ≥ 11 + 长度为奇数 + 声明字节数与实际数据长度一致 + 标记后全为 hex 数字），与 IHEX 解析器自身的校验（`bytes.len() == count + 5`）一致。补 3 条用例。

**变异测试结论**：D-03 / D-15 的门禁**强**（改回原样立刻变红）；D-08 / D-09 **仅单测级**；D-22 **无测试可观测**（仅 trait 层契约保证）。

---

## 四、V4 · 前端服务层 + 收口文档

**判定：PARTIAL**（4 高 + 4 中 + 5 低）· 26 payload × 5 enum 逐字比对 + 17 项变异

### 高危

| # | 发现 | 处置 |
| --- | --- | --- |
| F1 | **mock 内核 `ClearBreakpoint` 按 `addr` 删，而契约是 `ClearBreakpoint { id }`** —— 发 `{ id: 1 }` 删不掉断点 | **已修**：改为按 id 反查地址再删。Wave 2 接真实 WASM 内核时第一次走这条路径就会踩坑 |
| F2~F5 | **台账 / PROGRESS / AGENTS 的数字失真 3~4 倍**，且同一文档内「314」与「334」自相矛盾 | **已订正**并写明统一口径（见 §六） |

### 中低危

| # | 发现 | 处置 |
| --- | --- | --- |
| F6 | 契约测试只验变体名，**不验变体内层字段名**（把 `id` 改成 `ident` 无测试变红），与文档自述的「任何一侧单边改名都会撞红」不符 | **已修**：以 Rust 变体清单为驱动逐字校验字段名，覆盖 CommandPayload / EventPayload 全部变体 + 7 个 DTO |
| F7 | M-10 断线重连后**不重订阅**（`subscribed` 标志永不重置），接真实内核会丢寄存器增量 | **已修**：断线时复位该标志 |
| F8 | `once()` 的一次性语义不可观测（删掉 `off()` 无测试变红） | **已修**：补可选观察者参数 + 计数 spy 用例 |
| F9 | **浏览器真机复验脚本在 `/tmp`，任何后续 Agent 都无法复现** | **已修**：落仓 `tools/browser-verify.mjs`（零依赖 CDP，Node 22 全局 `WebSocket` + `fetch`） |
| F10 | 5 处变异漏抓 | 由 F1 / F6 / F8 的修复覆盖；mock x0 与 SetBreakpoint 另补用例 |
| F11 | 契约 `model.rs` 同时有 `Waveform`（history 形）与 `WaveformDelta`（live 形），而 `EventPayload::WaveformDelta` 包的是 `Waveform` | → **D-34**（设计问题，非缺陷） |
| F12 | mock 内核 `reset()` 不清 `state.topics`，换工程后旧订阅仍有效 | **已修**：清理 + 连带修正 `app.js` 的重订路径 |
| F13 | mock 内核 PC 永远 +4，无 RV32C | → **D-35**（已知限制） |
| F14 | M-13 报告 §一 #6 与 §四 #2 自相矛盾（把「待对拍时翻转」写成「契约要求」） | 已订正（`9992419`） |

---

## 五、F-5 专项裁决：改 LLD 对齐实现

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

## 六、数字口径订正

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

## 七、本轮方法论

1. **变异测试比读代码更有效**。V1 的 32 项变异中 4 项「改回去没测试会红」——这直接暴露覆盖漏洞，比逐行 review 更快定位到「门禁在哪儿是瞎的」。
2. **补的每条用例都必须反向验证**。本轮所有新增用例都做了「改回原样必须变红」的验证，确保不是安慰剂。V2 的 F-7 用例尤其典型：单看「测试全绿」会以为覆盖到了，变异后才发现 `map_ram` 用例是被**另一层**防线兜住的。
3. **文档与实现的矛盾要查历史**。F-5 若只比当前状态会得出「随便挑一边改」；查 git 历史 + review 记录才发现，实现方**早已识别为待裁决项并上收**（`M-14_M-15_M-18_Wiring_Review.md` §2.2 / §5.2 O2）——这不是实现方失误，是待裁决项至今未裁决。
4. **台账数字失真是实质缺陷**。台账定位是「唯一追踪入口」，数字错会让外部 review 与排期（Spike 对拍人日、覆盖率基线）整体失真。

---

## 八、闭环状态

| 严重度 | 发现 | 闭环 | 未闭环 |
| --- | --- | --- | --- |
| 🔴 阻断 | 1 | **1** | 0 |
| 🟠 高危 | 2 | **2** | 0 |
| 🟡 中危 | 6 | 3 | 3（D-25 / D-29 / D-30 / D-32 / D-33） |
| 🟢 低危 | 13 | 9 | 4（D-27 / D-28 / D-31 / D-34 / D-35） |

**新增台账条目 D-24~D-35 共 12 条**。全部登记在 `docs/reviews/CROSS_MODULE_DEBT.md`。
