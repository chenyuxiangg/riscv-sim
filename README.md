# riscv_sim

**RISC-V（RV32IMC）指令集仿真与调试平台** —— 自研 ISS + 事件驱动仿真内核 + 零构建浏览器工作台。

一个可以**真的跑起来**的 RISC-V 仿真器：自带指令集解释器、总线与外设模型、中断控制器、断点 / 单步 / 波形 / DWARF 源码行，在浏览器里给一个**真实的 M 模式 RTOS** 装上终端、让它跑起来、并与 **Spike 参考侧逐字节比对**。

> **本文档面向人。** 给 AI Agent 的项目定义与工程纪律在 [`AGENTS.md`](./AGENTS.md)；贡献流程（分支 / commit / PR 约定）在 [`CONTRIBUTING.md`](./CONTRIBUTING.md)；**全部欠账与检视记录**在 [`docs/reviews/REVIEWS.md`](./docs/reviews/REVIEWS.md)。

---

## 目录

- [这是什么](#这是什么)
- [能力快照](#能力快照)
- [快速开始](#快速开始)
- [架构一览](#架构一览)
- [模块地图](#模块地图)
- [正确性：三层证据](#正确性三层证据)
- [工程门禁](#工程门禁)
- [已知缺口](#已知缺口)
- [目录结构](#目录结构)
- [文档索引](#文档索引)

---

## 这是什么

三句话说明它由什么组成：

1. **一个自研的 RV32IMC 指令集仿真器**（Rust）—— RV32I 41 条 + RV32M 8 条 + CSR 指令族 + `mret` / `wfi`，外加 RV32C 压缩指令 26 条；带**架构陷阱**（未映射访存 ⇒ cause 5 / 7）、`mepc` 三路径口径、`mstatus` WARL 掩码。
2. **一个围绕它的事件驱动仿真内核** —— 1 µs 量子 + 唯一主循环；总线 / 内存映射 / 外设模型（**ns16550 UART · CLINT · PLIC · GPIO**）/ 中断控制器；断点、观察点、单步、探针、波形管道、符号表 / DWARF 行表 / 反汇编 / trace 导出；会话快照（**六段**）可落盘可还原。
3. **一个零构建的浏览器工作台** —— 原生 ES modules（**无打包、无第三方依赖**）+ WASM 内核，**6 个视图**：产品树 / 调试 / 寄存器内存 / 波形 / 源码 / 终端。

它的**验收标准不是「能跑一条指令」**，而是「**能跑一个真实 RTOS，且输出与 Spike 逐字节相同**」——见 [正确性](#正确性三层证据)。

---

## 能力快照

> **口径**：以下数字**全部从当前树重算**（2026-10-23，`main` @ `1acf582`）。不是引用历史结论。

### 指令与体系结构

| 项 | 数字 | 说明 |
| --- | --- | --- |
| 基础 opcode | **57** | RV32I **41** + RV32M **8** + CSR 指令 **6** + `mret` / `wfi` **2** |
| 压缩指令 | **26** | RV32C 基集全覆盖，字段级展开（`c.` 前缀保留，不做伪指令别名） |
| 已实现 CSR | **10** | `mstatus` `misa` `mie` `mtvec` `mscratch` `mepc` `mcause` `mtval` `mip` `mhartid`（另有伪 CSR `irq_clear` `0x7C0`；`mtime` 只读且**真值走 CLINT 的 MMIO 窗口**，不走 CSR —— 未在文件里的 CSR 一律读回 **0**，**不陷阱**） |
| 陷阱原因 | **11** 变体 | 10 个架构码（含 **cause 1 取指访问故障**） + 1 个内部 `OutOfRange`；⚠️ **`misa` 仍只读** ⇒ 台账 **D-95** |
| `misa` | `0x4000_1104` | MXL=1 + I / M / C |

### 契约层（模块之间**只**通过它通信）

| 项 | 数字 |
| --- | --- |
| `schema_version` | **5** |
| 命令 | **17** |
| 事件 | **15** |
| 错误族 | **14**（`CTX` `ISS` `ENG` `BUS` `PER` `INT` `DBG` `OBS` `SNAP` `LDR` `HMS` `STORE` `TRX` `PROTO`） |

### 调试与观测

| 项 | 数字 |
| --- | --- |
| 断点 | 按地址索引 + id 反查；**取指前**命中 |
| 观察点 | 地址 + 方向（读 / 写）+ 宽度匹配；**追加式**总线 hook（不顶掉别人的） |
| 单步粒度 | **4 种**（`Instruction` / `Over` / `Out` / `Line`）——⚠️ 后三种**显式降级**为指令级（`degradation()` 把降级说出来，**不悄悄降级**） |
| DWARF 行表 | `.debug_line` **v2 / v3 / v4 / v5** 真解析（行号程序 + 行号状态机 + 特殊操作码 + 按单元降级 + 多单元归并） |
| 反汇编 | **复用 M-13 的解码器** ⇒ **零第二套解码表**（漂移结构上不可能） |
| 导出格式 | 波形 → **CSV / VCD**；执行轨迹 → **CSV / JSON** |
| 会话快照 | **6 段**：`cp_meta` `m12_engine` `m13_state` `m14_map` `m15_periph` `m16_intc`（`SESSION_SNAPSHOT_SCHEMA_VERSION = 2`） |

### 外设与中断

| 器件 | 状态 |
| --- | --- |
| **UART（ns16550）** | 8 个字节寄存器（THR/RHR/DLL · IER/DLM · FCR · LCR · MCR · LSR · MSR · SCR）· `LCR.DLAB` 真切换 · **无损 TX 捕获环** 4 KiB · **16 字节深 RX FIFO**（满时丢最新并锁存溢出位） |
| **CLINT** | `msip` / `mtimecmp`（分步 64 位写）/ `mtime` 64 位读，**单位是周期** |
| **PLIC** | priority / pending / enable / threshold / claim + complete；claim 判据**严格大于门限** |
| **GPIO** | PL061 惯例布局（`0x4000_0000`），驱动探针边沿 |
| **中断控制器（M-16）** | 挂起队列按 `(虚拟时间, 插入序)` **保序** · 同线去重 · 优先级仲裁 · 量子边界采样 · **拉取式**交付（取走即消失）· `mie` + `mstatus.MIE` 双闸门 |

### 平台与内存图

| 项 | 状态 |
| --- | --- |
| **平台描述** | **一份 JSON 数据**（`crates/kernel/src/kernel_host/platforms/*.json`，编译进二进制）承载**地址空间 + 器件清单** ⇒ 契约层有**唯一**校验器（V-1 ~ V-12）⇒ **换一份描述、宿主代码零改动** |
| **内置描述** | `default`（低段 1 MiB + 高段 4 MiB + GPIO）· `qemu-virt`（低段 1 MiB + 高段 128 MiB + UART0 / CLINT / PLIC / GPIO）· `minimal`（演示用，`auto_selectable: false` ⇒ **只能被显式指定**） |
| **自动选档** | 按镜像**实际用到的地址**挑描述（**区间在外层** · 候选按 `priority` 小者优先）⇒ 与历史行为**逐字等价**（六条等价性用例钉住） |
| **外设窗口大小** | **不写进描述** ⇒ 由器件自己报（`Peripheral::size()`）⇒ 只有一个真相源 |

### 工程规模

| 项 | 数字 |
| --- | --- |
| 契约层 | 9 493 行 |
| 仿真内核 | 59 198 行（含 16 个子模块 + 内核宿主装配层） |
| 前端 | 15 364 行（`front/` 下**git 跟踪**的源码，含 `front/tests/`；⚠️ **不含** `front/assets/` —— 那是 wasm-pack 的**构建产物**且被 git 忽略，计进去会让这个数**随「跑没跑过 wasm 构建」变** ⇒ 不可复现） |
| 设计文档 | **50 篇**（26 篇 HTML + 24 篇 Markdown） |
| 测试 | **1 416** 条 Rust（契约 lib 170 + 内核 lib 1 087 + 集成 142 + runner 17）+ **312** 条前端（57 suite） |

---

## 快速开始

### 依赖

```bash
# Rust stable + WASM target + wasm-pack
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# 前端门禁需要 Node ≥ 22（零依赖，只用 node:test）
node --version
```

可选（**不进默认门禁**，按需运行）：

- **Spike**（黄金对拍与真实 RTOS 对拍的参考侧）
- **RISC-V 交叉工具链**（编 osz 等真实固件）
- **Chrome**（`tools/browser-verify.mjs` 的真机复验）

### 编译与测试

```bash
cargo build                                    # native
cargo test --workspace --no-fail-fast --lib --tests   # 1 416 条
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

> ⚠️ **不要**用裸 `cargo test --workspace`：它会带上 doc-test，在 macOS 上会**假挂死**。用 `--lib --tests`（或加 `--offline`）。

### 前端门禁

```bash
tools/check-front.sh        # node:test 冒烟门禁 —— 312 用例 / 57 suite
```

### 构建浏览器内核（WASM）

```bash
tools/build-wasm.sh             # 调试版
tools/build-wasm.sh --release   # 发布版
```

产物落在 `front/assets/pkg/`（`--target web` ⇒ **原生 ES module + 具名导出**，零构建前端可直接 `import`）。

### 打开前端

```bash
python3 -m http.server 8080 --directory front
# 浏览器访问 http://localhost:8080
```

> ⚠️ **必须走 HTTP(S)**：前端用 **Worker + ES module**，两者都要求同源 ⇒ `file://` 直开会被浏览器拦截。

### 跑一个真实 RTOS（osz）并与 Spike 对拍

```bash
tools/boot-osz.sh --elf /path/to/osz.elf
```

判决口径是「**两侧 UART TX 字节流逐字节相同**」（真实 RTOS 没有 riscv-tests 那套 HTIF `tohost` 判决 ⇒ 不能复用黄金对拍的老路）。

⚠️ **osz 的产物在仓外**（不进仓库）：默认读 `$OSZ_ELF`，其次 `/tmp/osz-artifacts/osz.elf`。判定为 `VERDICT MATCH / MISMATCH / NO-VERDICT`。

### 黄金对拍（riscv-tests）

```bash
tools/fetch-golden-deps.sh        # 拉 riscv-tests + 交叉工具链到**仓库外**
tools/golden-run.sh --self-check  # 变异金丝雀自检（两侧都必须报 FAIL 3）
tools/golden-run.sh --keep-going  # 全量语料 —— 68 例
```

---

## 架构一览

```
                    ┌──────────────────────────────────────────┐
   浏览器 (ESM)     │  M-01 工作台  →  M-02 视图库  →  M-03 状态 │
   零构建 / 零依赖  │  M-04 会话 · M-05 命令路由 · M-07 传输     │
                    │  M-10 寄存器内存服务                        │
                    └───────────────┬──────────────────────────┘
                                    │ 契约信封（schema_version = 5）
                        ┌───────────┴────────────┐
                        │                        │
                 postMessage（Worker）        WebSocket（P1）
                        │                        │
              ┌─────────▼────────────────────────────┐
              │  内核宿主（装配层 · 非 23 模块之一）    │
              │  16 条命令 → 事件的规范映射 · 镜像装载  │
              └──┬───────┬───────┬───────┬───────────┘
                 │       │       │       │
        ┌────────▼──┐ ┌──▼─────┐ ┌▼─────┐ ┌▼──────────────┐
        │ M-12 引擎  │ │ M-13   │ │ M-14 │ │ M-16 中断      │
        │ 事件/量子  │ │ ISS    │ │ 总线 │ │ 挂起/仲裁/交付  │
        │ 唯一主循环 │ │ RV32IMC│ │ 内存图│ └───────────────┘
        └────┬──────┘ └──┬─────┘ └──┬──┘
             │           │        │
             │      ┌────▼────┐ ┌─▼──────────────┐
             │      │ M-15    │ │ M-17 执行后端    │
             │      │ 外设模型 │ │ 断点/观察点/单步 │
             │      │ UART·CLINT│ └────────────────┘
             │      │ ·PLIC·GPIO│
             │      └────┬────┘
             │           │
        ┌────▼───────────▼───────────────────────────┐
        │ 调试与观测：M-08 断点 · M-09 单步 · M-11 符号 │
        │ /DWARF/反汇编/trace · M-18 探针 · M-19 波形   │
        └──────────────────────────────────────────────┘
```

**三条贯穿全仓的规矩**

1. **契约先行** —— 模块之间只引 `riscv_sim_contract` 定义的 DTO / 事件 / 错误码 / 端口签名；内部实现语言不受约束。
2. **一个约束一个落点** —— 同一件事**只**有一个定义处（例如 `mstatus` 的 WARL 掩码住在契约层，M-13 只引用不复制）。
3. **确定性** —— 种子 + 时间戳化输入事件序列 ⇒ 结果唯一。

---

## 模块地图

23 个模块（5 横层 + 3 横切）+ 1 个内核宿主装配层。

### 仿真核心

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| **M-12** | 事件驱动仿真引擎（事件队列 + 1 µs 量子 + 唯一主循环 `run_until`） | ✅ |
| **M-13** | RISC-V ISS（RV32IMC）—— 解码 / 执行 / 架构陷阱 / CSR | ✅ |
| **M-14** | 总线与内存映射（region 表 + 追加式 hook + 统计） | ✅ |
| **M-15** | 外设模型层（UART / CLINT / PLIC / GPIO） | ✅ |
| **M-16** | 中断控制器（挂起队列 / 仲裁 / 拉取式交付） | ✅ |
| **M-17** | ExecutionBackend 执行后端（驱动 M-13、接总线与断点表） | ✅ |
| **M-20** | 快照与确定性事件流记录（六段会话快照组装） | 🟡 部分落地 |

### 调试与探针

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| **M-08** | 断点管理器（断点表 + 观察点表） | ✅ |
| **M-09** | 单步引擎（4 粒度 + **显式降级**） | ✅ |
| **M-11** | 符号表 / DWARF / 反汇编 / trace 导出 | ✅ |
| **M-18** | 探针（判沿 + 访存轨迹） | ✅ |
| **M-19** | 波形管道（边沿编码 + 环 + CSV/VCD 分块导出） | ✅ |

### 前端 / 工作台

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| **M-01** | 工作台装配与多视图路由（hash 路由） | ✅ |
| **M-02** | 视图组件库（**7** 个视图 —— v0.25 补了落地视图 `embedded`） | ✅ |
| **M-03** | 视图状态管理 | ✅ |

### 基础设施

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| **M-04** | 会话管理器（状态机 + 断线重连） | ✅ |
| **M-05** | 命令路由与事件分发（**表驱动**路由） | ✅ |
| **M-07** | 传输适配器（Worker + WebSocket；帧 CRC-32） | ✅ |
| **M-10** | 寄存器内存服务（订阅 + 读写寄存器） | ✅ |
| **M-21** | 日志系统（**进程内**五带环形缓冲 + CSV / JSON 导出） | ✅ **v0.28 全量落地** |
| **M-22** | 健康管理 HMS（分级 / 保留带 / 分角色分发 / 降级） | ✅ **v0.28 全量落地** |
| **M-23** | 数据存储与持久化 DataStore | 🟡 仅内存实现（⚠️ **IndexedDB / SQLite 后端仍 stub**；v0.28 增**通用字节归档**接口 `archive(name, bytes)`） |

### 文件与解析

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| **M-06** | ELF / Raw-HEX 解析器（**全仓唯一的 ELF 容器解析者**） | ✅ |

---

## 正确性：三层证据

本项目把「对不对」拆成三层，**每层都有可复现的门禁**：

### 第一层 · 指令级（黄金对拍）

riscv-tests 语料 **68 例**（`rv32ui` 42 + `rv32mi` 13 + `rv32um` 8 + `rv32uc` 1 + 本地 4），候选侧自研 runner 与 **Spike** 逐例比对判决。

```bash
tools/golden-run.sh --self-check   # 变异金丝雀：两侧都必须报 FAIL 3
tools/golden-run.sh --keep-going   # 全量 —— 68/68
```

⚠️ 金丝雀只覆盖「判决运输路径」，**不证明成功路径有判别力** —— 判别力由**全量对拍**提供。

### 第二层 · 系统级（真实 RTOS）

一个**真实**的 M 模式 RTOS（osz）在本平台上跑到 shell 提示符，**两侧 UART TX 字节流逐字节相同**。

```bash
tools/boot-osz.sh
# VERDICT MATCH 两侧 console 字节流逐字节相同（31 字节）
```

### 第三层 · 界面级（真浏览器）

真 Chrome（CDP）里驱动前端，断言**值**而不是「非空」：断点真的停住、反汇编真的带出源码行号、UART 真的收到字节。

```bash
node tools/browser-verify.mjs
```

> **为什么三层都要**：单元测试全绿 ≠ 前端不白屏 ≠ 平台与参考侧一致。历史上这三层各抓出过**只有那一层能抓**的真缺陷。

---

## 工程门禁

九项门禁，**失败项数必须为 0**。

| # | 门禁 | 覆盖 |
| --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | 格式 |
| 2 | `cargo clippy --workspace --all-targets -- -D warnings` | 静态检查 |
| 3 | `cargo test --workspace --no-fail-fast --lib --tests` | **1 416** 条 Rust 用例（⚠️ **不带 `--lib --tests` 会因 doc-test 假 HANG**；⚠️ 判「全绿」要看**退出码** + `grep -cE "^test result: FAILED"` 为 **0** —— 汇总的失败数在 `awk -F'[ ;]'` 的 **`$7`**（**不是 `$6`**）） |
| 4 | `tools/check-front.sh` | **312** 用例 / **57** suite（ESM 链接期错误只有它能抓） |
| 5 | `tools/golden-run.sh --self-check` + `--keep-going` | 指令级判决（**68/68**） |
| 6 | `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | 真 WASM 产物的桥接冒烟 |
| 7 | `node tools/browser-verify.mjs` | 真 Chrome 里的端到端断言 |
| 8 | `tools/boot-osz.sh` | 真实 RTOS 与 Spike 逐字节对拍（**需仓外 osz ELF + 本机 Spike**，不进默认门禁） |
| 9 | **变异自检**（每批至少一条「对照变异 0 红」） | **判别力**的证据：真变异必须**精确红**在预期的那几条上，对照变异必须 **0 红** ⇒ 证明门禁不是「逢改必红」 |

---

## 已知缺口

**完整的欠账清单在 [`docs/reviews/REVIEWS.md`](./docs/reviews/REVIEWS.md)**（含每一条的实证依据、阻塞关系与闭环方式）。

✅ **v0.28 批已闭环两条**：**D-114**（M-17 `step_n` 接上与 M-12 同形状的**无进展守卫** ⇒ 反复交付同一中断从**活锁挂死**变成**报错退出**）· **D-115**（**宿主看门狗**：时间预算 200 ms + 滑动窗口三次确认 + **HMS 同步终止** + `WATCHDOG` 日志 + **幂等标志** ⇒ 「活锁变成失败」推进到「失败**有处置、有记录**」）。

✅ **v0.27 批已闭环两条**：**D-123**（内存图通用化 A + B 两批 ⇒ **换一份内存图描述、宿主代码零改动**这条唯一判别式成立）· **D-82**（CLINT 固件侧 MMIO ⇒ 台账原文已被实测推翻 ⇒ 真实缺口只是「缺 guest 指令级端到端判别力」⇒ 已补 `crates/kernel/tests/d82_clint_end_to_end.rs`）。

当前几条要紧的：

| 欠账 | 说的是什么 |
| --- | --- |
| **D-124** | 契约层**无 MemoryMap 命令与事件** ⇒ 前端拿不到**真实**内存图（描述只在内核与宿主内部流动）⇒ 前端的内存映射表仍是 mock 数据（⚠️ 裁决 I 故意裁剪：描述不进会话载荷、不升契约版本） |
| **D-125** | 器件之间的**连线**仍写死在宿主 ⇒ **换平台改连线仍要改代码**（描述只覆盖「地址空间」那一半） |
| **D-126** | 参考侧对拍脚本里三份**手抄地址副本**与描述要**手工对齐**（⚠️ 裁决 J 故意不动：那是「对齐 Spike」的职责） |
| **D-127** | 契约层 `csr.rs` 的「三个空间」表格把 `irq_clear` 列在「internal **line**」一栏 ⇒ **与实现的位掩码语义不符** ⇒ 清线 3 该写 `8` 不是 `3` |
| **D-129** | 契约层**无日志 / 健康的 DTO 与回复事件** ⇒ 服务端形态下「管理员跨进程收集」**没有契约载体**（⚠️ 契约层**已有** `Admin` / `AdminOp::DrainLogs` 空壳入口，**缺的是 DTO** ⇒ 宿主对该命令仍一律返回 `E_CTX-05`）⇒ 需**升契约版本** + 前端镜像 |
| **D-94** | S 扩展 / PMP / 计数器缺失（工作量 ≈ 实现半个特权级） |
| **M-23** | **持久化后端仍只有内存实现**（IndexedDB / SQLite 未做）⇒ 跨会话持久化要等它 |
| — | UART 的 **TX 捕获环（4 KiB）仍不进会话快照**（RX 侧已进） |

---

## 目录结构

```
riscv_sim/
├── crates/
│   ├── contract/          # 契约层：DTO / 事件 / 错误码 / 端口签名（所有模块都引它）
│   ├── kernel/            # 仿真内核：16 个子模块 + 内核宿主装配层
│   └── server/            # 服务端形态（P1 占位）
├── front/                 # 原生 JS 前端（ES modules，零构建零依赖）
│   ├── views/             # M-01 / M-02 / M-03
│   ├── services/          # M-04 / M-05 / M-07 / M-10
│   ├── lib/               # 契约 JS 镜像 + 传输帧（CRC-32）
│   └── dev/               # mock 内核 + demo 数据（仅开发期）
├── tools/                 # 构建与门禁脚本（wasm-pack 包装 / 对拍 / 真机复验）
├── tests/golden/          # 黄金对拍 harness（零 CSR 最小 env + 变异金丝雀 + 语料）
└── docs/                  # 设计文档（架构 / 需求 / 模块规格 / 23 模块详设 / 9 份冻结规格 / 台账）
```

---

## 文档索引

| 文档 | 内容 |
| --- | --- |
| [`docs/architecture/RISC-V_Architecture_Design.html`](./docs/architecture/RISC-V_Architecture_Design.html) | 4+1 视图 + ADR |
| [`docs/requirements/RISC-V_Requirements_v1.4.html`](./docs/requirements/RISC-V_Requirements_v1.4.html) | 13 条 P0 需求 + 6 层概念空间 |
| [`docs/module-design-spec/RISC-V_Module_Design_Specification.html`](./docs/module-design-spec/RISC-V_Module_Design_Specification.html) | 23 个模块的 LLD |
| `docs/detailed-design/M-01..M-23_*.html` | **23 篇模块详细设计** |
| `docs/detailed-design/STAGE-1..STAGE-9_*.md` | **9 份冻结规格**（WASM 桥 / 契约 v2 / 会话快照 / 中断 / 符号 / CSR 黄金 / DWARF 行表 / osz RTOS / v0.24 遗留） |
| [`docs/reviews/REVIEWS.md`](./docs/reviews/REVIEWS.md) | **检视与欠账的唯一入口**（§4 有 9 份模块报告索引） |
| [`docs/progress/PROGRESS.md`](./docs/progress/PROGRESS.md) | 改动的完整叙事、决策记录与工作量 |
| [`AGENTS.md`](./AGENTS.md) | 给 AI Agent 的项目定义与工程纪律 |
| [`CONTRIBUTING.md`](./CONTRIBUTING.md) | 贡献流程（分支 / commit / PR） |

---

## 许可证

**MIT OR Apache-2.0**（见 workspace `Cargo.toml`）。

⚠️ **合规**：仿真内核**自研**，不复用 QEMU 源码（GPL）；如需引入 QEMU，走**进程隔离**。
