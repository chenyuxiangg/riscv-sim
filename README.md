# riscv_sim

**RISC-V（RV32IMC）指令集仿真与调试平台** —— 自研 ISS + 事件驱动仿真内核 + 零构建浏览器工作台。

一个可以**真的跑起来**的 RISC-V 仿真器：自带指令集解释器、总线与外设模型、中断控制器、断点 / 单步 / 波形 / DWARF 源码行；在浏览器里给一个**真实的 M 模式 RTOS** 装上终端、让它跑起来，并与 **Spike 参考侧逐字节比对**。

- 面向人的项目介绍：本文档
- 给 AI Agent 的项目定义与工程纪律：[`AGENTS.md`](./AGENTS.md)
- 贡献流程（分支 / commit / PR）：[`CONTRIBUTING.md`](./CONTRIBUTING.md)
- **欠账与评审台账**（唯一入口）：[`docs/reviews/REVIEWS.md`](./docs/reviews/REVIEWS.md)
- 当前需求完成情况：[`docs/progress/PROGRESS.md`](./docs/progress/PROGRESS.md)

---

## 目录

- [这是什么](#这是什么)
- [功能一览](#功能一览)
- [快速开始](#快速开始)
- [架构一览](#架构一览)
- [模块地图](#模块地图)
- [正确性：三层证据](#正确性三层证据)
- [工程门禁](#工程门禁)
- [目录结构](#目录结构)
- [文档索引](#文档索引)
- [许可证](#许可证)

---

## 这是什么

三句话说明它由什么组成：

1. **一个自研的 RV32IMC 指令集仿真器**（Rust）—— RV32I 41 条 + RV32M 8 条 + CSR 指令族 + `mret` / `sret` / `wfi`，外加 RV32C 压缩指令 26 条；带架构陷阱、`mepc` 三路径口径、`mstatus` WARL 掩码、S / U 模式与 PMP 物理内存保护。
2. **一个围绕它的事件驱动仿真内核** —— 1 µs 量子 + 唯一主循环；总线 / 内存映射 / 外设模型（UART · CLINT · PLIC · GPIO）/ 中断控制器；断点、观察点、单步、探针、波形管道、符号表 / DWARF / 反汇编 / trace 导出；会话快照可落盘可还原。
3. **一个零构建的浏览器工作台** —— 原生 ES modules + WASM 内核，多视图（产品树 / 调试 / 波形 / 源码 / 终端 / 落地 / 板级内存图）；可在界面上加载固件、自定义开发板、规划基址。

它的验收标准不是「能跑一条指令」，而是「**能跑一个真实 RTOS，且输出与 Spike 逐字节相同**」——见 [正确性](#正确性三层证据)。

---

## 功能一览

> 数字为当前树快照口径，由仓内 `tools/count_scale.py` 从当前树重算。

### 指令与体系结构

| 项 | 数值 |
| --- | --- |
| 基础 opcode | **58**（RV32I 41 + RV32M 8 + CSR 指令 6 + `mret`/`sret`/`wfi` 3） |
| 压缩指令 | **26**（RV32C 基集全覆盖，字段级展开） |
| 已实现 CSR | **51**（M 15 + S/U 9 + 委托与计数使能 5 + PMP 20 + 伪 CSR 2） |
| 陷阱原因 | **11** 变体 |
| 特权模式 | M / S / U 三级 + PMP 物理内存保护 |
| 硬件计数器 | Zicntr（`mcycle` / `minstret` + 用户态影子） |

### 契约层

| 项 | 数值 |
| --- | --- |
| `schema_version` | **11** |
| 命令 / 事件 / 错误族 | 29 / 21 / 15 |

### 调试与观测

| 项 | 内容 |
| --- | --- |
| 断点 | 按地址索引 + 取指前命中 + 条件表达式 |
| 观察点 | 地址 + 方向 + 宽度匹配 · 追加式总线 hook |
| 单步 | 4 粒度（Instruction / Over / Out / Line，后三种显式降级） |
| DWARF | `.debug_line` v2/v3/v4/v5 真解析 |
| 反汇编 | 复用 ISS 解码器（零第二套解码表） |
| 导出 | 波形 CSV/VCD · 执行轨迹 CSV/JSON |
| 会话快照 | 6 段，可落盘可还原 |

### 外设与中断

| 器件 | 说明 |
| --- | --- |
| UART（ns16550） | 8 字节寄存器 + DLAB 真切换 + 无损 TX 捕获环 + 16 字节 RX FIFO |
| CLINT | `msip` / `mtimecmp` / `mtime` |
| PLIC | priority / pending / enable / threshold / claim + complete |
| GPIO | PL061 布局，驱动探针边沿；**引脚级 IRQ**（`pin_irq`，一次性边沿语义）可路由进 PLIC |
| 中断控制器 | 保序挂起队列 + 同线去重 + 优先级仲裁 + 拉取式交付 |

### 平台与内存图

| 项 | 说明 |
| --- | --- |
| 平台描述 | 一份 JSON 数据承载地址空间 + 器件清单；契约层唯一校验器；换描述、宿主代码零改动 |
| 内置描述 | `default` / `qemu-virt` / `minimal`（后者只能显式指定） |
| 自动选档 | 按镜像实际用到的地址挑描述 |
| 板级编辑 | 浏览器里定制 RAM / ROM / 外设窗口、差量改板、板档案持久化、基址规划、器件类型/实例注册表、**器件连线编辑面**（加 / 删连线） |
| 器件实例 | 宿主按 **id 索引**持有每个器件实例 ⇒ ⇒ 多个 UART / PLIC **各有各的连线**，不再塌缩成三个固定角色 |
| 连线报告 | 内核在**接受描述的那一刻**说清「你写了 N 根线里有 M 根当前抬不了」⇒ ⇒ 逐条带原因 ⇒ ⇒ **不静默丢线** |
| 引脚级连线 | **v0.74 起**：连线可落到**引脚级**（`from.pin`）+ **通用连接路由**（事件词表化 ⇒ ⇒ `rx_ready`（`uart16550`）· `pin_irq`（`gpio`）⇒ ⇒ 按描述的 `wiring` 编译，不再硬编码「UART→PLIC 一条」） |
| MCU 概念 | **v0.75 起**：板可声明 **MCU**（ISA / 厂商 / 型号 / 向量表）；**装载时**按板 ISA 校验固件（位宽不匹配 / 板不含 `c` 却带 RVC ⇒ 拒 `E_PLT-20`；isa 词法非法 ⇒ `E_PLT-21`）；板级视图**用户自配**、内置板 =「从内置模版开始」模板入口、基址 **16 进制** |
| 模板导入 + 无板门禁 | **v0.76 起**：平台**不兜底**（**auto_selectable 废弃** · 自动选档链路删除）⇒ ⇒ 启动无生效板 = 合法初始态 ⇒ ⇒ 装载/运行拒 `E_PLT-22` + 引导「先从模板导入或配置一块板」⇒ ⇒ 内置板降级为**模板预设**（清单由内核报 · 前端「从模板导入」显式选一块 · 老描述带旧键照常解析） |
| 引脚复用 | **v0.76 起**：板描述 `pins[].functions[]`（**功能列表**：`{ name（用户自由命名）· direction（input/output/bidir）· irq: { source } }`）· 独立 **pinmux 器件**（每引脚选择寄存器 `0x4×引脚号` · 固件写索引动态切功能）· 中断绑「**引脚 × 当前功能**」（同引脚不同功能可拉不同源）· `wiring` 分层不变 · 前端编辑面压缩为只读展示 |

### 工程规模

| 项 | 数值 |
| --- | --- |
| 契约层 | **16 208** 行（11 个 `.rs`） |
| 仿真内核 | **78 280** 行（95 个 `.rs`，含宿主装配层） |
| 前端 | **41 303** 行（`front/` 下 git 跟踪源码） |
| 设计文档 | **88** 篇（26 HTML + 62 Markdown） |
| 测试 | **1 917** 条 Rust + **845** 条前端（168 suite）+ **292** 条真机断言（**34** 段） |

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

可选（不进默认门禁，按需运行）：**Spike**（黄金对拍参考侧）、**RISC-V 交叉工具链**（编 osz 等真实固件）、**Chrome**（`tools/browser-verify.mjs` 真机复验）。

### 编译与测试

```bash
cargo build                                    # native
cargo test --workspace --no-fail-fast --lib --tests   # 1 917 条
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
tools/check-front.sh        # 前端门禁 —— 845 用例 / 168 suite
```

> ⚠️ **不要**用裸 `cargo test --workspace`：它会带上 doc-test，在 macOS 上会**假挂死**。用 `--lib --tests`。

### 构建浏览器内核（WASM）并打开前端

```bash
tools/build-wasm.sh             # 调试版（产物落 front/assets/pkg/）
tools/serve-front.py            # 起前端（浏览器访问 http://127.0.0.1:8080）
```

> ⚠️ **用 `tools/serve-front.py`，别用 `python3 -m http.server`**：后者不发 `Cache-Control`，浏览器按「启发式新鲜度」缓存 ES module，会吃旧代码，症状像「改了没生效」。
> 万一旧副本把页面砸成白屏：**硬刷新 `Cmd/Ctrl + Shift + R`**。前端必须走 HTTP(S)（Worker + ES module 要求同源）。

### 跑一个真实 RTOS（osz）并与 Spike 对拍

```bash
tools/boot-osz.sh --elf /path/to/osz.elf   # 两侧 UART TX 字节流逐字节相同
```

### 黄金对拍（riscv-tests）

```bash
tools/fetch-golden-deps.sh        # 拉 riscv-tests + 交叉工具链到仓库外
tools/golden-run.sh --self-check  # 变异金丝雀自检
tools/golden-run.sh --keep-going  # 全量语料 —— 77 例
```

> 📖 **第一次用这个平台 ⇒ 看 [`docs/USER_GUIDE.md`](./docs/USER_GUIDE.md)**（怎么用每个视图 · 怎么换一块板 · 命令行怎么跑 · 报错了怎么查）。

---

## 架构一览

```
浏览器 (ESM，零构建/零安装)            内核（Rust，编成 WASM）
┌───────────────────────────┐  契约信封    ┌───────────────────────────┐
│ M-01 工作台               │ ──────────▶ │ 内核宿主（装配层）         │
│ M-02 视图库 · M-03 状态   │             │ 命令 → 事件规范映射 · 装载 │
│ M-04 会话 · M-05 路由     │  postMessage │ ┌────┬────┬────┬────┬────┐│
│ M-07 传输 · M-10 寄存器   │             │ │M-12│M-13│M-14│M-15│M-16││
└───────────────────────────┘             │ │引擎│ ISS│总线│外设│中断││
                                           │ └────┴────┴────┴────┴────┘│
                                           │ M-17 执行后端 · M-08/09 断点│
                                           │ M-11 符号/DWARF/反汇编/trace│
                                           │ M-18 探针 · M-19 波形 · M-20│
                                           │ M-21 日志 · M-22 HMS · M-23│
                                           └───────────────────────────┘
```

**三条贯穿全仓的规矩**

1. **契约先行** —— 模块之间只引 `riscv_sim_contract` 定义的 DTO / 事件 / 错误码 / 端口签名。
2. **一个约束一个落点** —— 同一件事**只**有一个定义处。
3. **确定性** —— 种子 + 时间戳化输入事件序列 ⇒ 结果唯一。

---

## 模块地图

23 个模块（5 横层 + 3 横切）+ 1 个内核宿主装配层。物理位置见 [`AGENTS.md`](./AGENTS.md) 的模块映射表。

### 仿真核心

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| M-12 | 事件驱动仿真引擎（事件队列 + 1 µs 量子 + 唯一主循环） | ✅ |
| M-13 | RISC-V ISS（RV32IMC）—— 解码 / 执行 / 架构陷阱 / CSR | ✅ |
| M-14 | 总线与内存映射（region 表 + 追加式 hook + 统计） | ✅ |
| M-15 | 外设模型层（UART / CLINT / PLIC / GPIO） | ✅ |
| M-16 | 中断控制器（挂起队列 / 仲裁 / 拉取式交付） | ✅ |
| M-17 | ExecutionBackend 执行后端（驱动 M-13、接总线与断点表） | ✅ |
| M-20 | 快照与确定性事件流记录 | 🟡 部分落地 |

### 调试与探针

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| M-08 | 断点管理器（断点表 + 观察点表） | ✅ |
| M-09 | 单步引擎（4 粒度 + 显式降级） | ✅ |
| M-11 | 符号表 / DWARF / 反汇编 / trace 导出 | ✅ |
| M-18 | 探针（判沿 + 访存轨迹） | ✅ |
| M-19 | 波形管道（边沿编码 + 环 + CSV/VCD 导出） | ✅ |

### 前端 / 工作台

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | ✅ |
| M-02 | 视图组件库 | ✅ |
| M-03 | 视图状态管理 | ✅ |

### 基础设施

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| M-04 | 会话管理器（状态机 + 断线重连） | ✅ |
| M-05 | 命令路由与事件分发（表驱动路由） | ✅ |
| M-07 | 传输适配器（Worker + WebSocket；帧 CRC-32） | ✅ |
| M-10 | 寄存器内存服务 | ✅ |
| M-21 | 日志系统（进程内五带环形缓冲 + CSV/JSON 导出） | ✅ |
| M-22 | 健康管理 HMS（分级 / 保留带 / 分角色分发 / 降级） | ✅ |
| M-23 | 数据存储与持久化 DataStore | 🟡 前端侧已落地；服务端形态未做 |

### 文件与解析

| 模块 | 职责 | 状态 |
| --- | --- | --- |
| M-06 | ELF / Raw-HEX 解析器（全仓唯一的 ELF 容器解析者） | ✅ |

---

## 正确性：三层证据

本项目把「对不对」拆成三层，**每层都有可复现的门禁**：

### 第一层 · 指令级（黄金对拍）

riscv-tests 语料 **77 例**，候选侧自研 runner 与 **Spike** 逐例比对判决。

```bash
tools/golden-run.sh --self-check   # 变异金丝雀：两侧都必须报 FAIL 3
tools/golden-run.sh --keep-going   # 全量 —— 77/77
```

### 第二层 · 系统级（真实 RTOS）

一个**真实**的 M 模式 RTOS（osz）在本平台跑到 shell 提示符，**两侧 UART TX 字节流逐字节相同**。

```bash
tools/boot-osz.sh   # VERDICT MATCH
```

### 第三层 · 界面级（真浏览器）

真 Chrome（CDP）里驱动前端，断言**值**而不是「非空」：断点真的停住、反汇编真的带出源码行号、UART 真的收到字节、配的板真的生效并持久化。

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
| 3 | `cargo test --workspace --no-fail-fast --lib --tests` | 1 917 条 Rust 用例 |
| 4 | `tools/check-front.sh` | 845 用例 / 168 suite |
| 5 | `tools/golden-run.sh --self-check` + `--keep-going` | 指令级判决（77/77） |
| 6 | `tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` | 真 WASM 产物桥接冒烟 |
| 7 | `node tools/browser-verify.mjs` | 真 Chrome 端到端断言（292 ✓ / 0 ✗ · 34 段） |
| 8 | `tools/boot-osz.sh` | 真实 RTOS 与 Spike 逐字节对拍（需仓外 osz ELF + Spike，不进默认门禁） |
| 9 | **变异自检**（每批至少一条「对照变异 0 红」） | 判别力的证据：真变异必须精确红在预期处，对照变异必须 0 红 |

另有台账口径自检（`tools/ledger_check.py`）、场景码口径自检（`tools/error_code_check.py`）、前端归属口径自检（`tools/front_test_attrib.py`）、文档自检（`tools/doc_check.py` 及各 `stage*_check.py`）等附属门禁。

---

## 目录结构

```
riscv_sim/
├── crates/
│   ├── contract/          # 契约层：DTO / 事件 / 错误码 / 端口签名
│   ├── kernel/            # 仿真内核：16 个子模块 + 内核宿主装配层
│   └── server/            # 服务端形态（P1 占位）
├── front/                 # 原生 JS 前端（ES modules，零构建 · 波形图走 CDN）
│   ├── views/             # M-01 / M-02 / M-03
│   ├── services/          # M-04 / M-05 / M-07 / M-10 / M-23
│   ├── lib/               # 契约 JS 镜像 + 传输帧（CRC-32）
│   └── dev/               # mock 内核 + demo 数据（仅开发期）
├── tools/                 # 构建与门禁脚本
├── tests/golden/          # 黄金对拍 harness + 变异金丝雀 + 语料
└── docs/                  # 设计文档 / 冻结规格 / 台账 / 用户指南
```

---

## 文档索引

| 文档 | 内容 |
| --- | --- |
| [`docs/USER_GUIDE.md`](./docs/USER_GUIDE.md) | **平台用户指南**（怎么用：起前端 · 各视图 · 换板 · 命令行 · 排错） |
| [`docs/architecture/RISC-V_Architecture_Design.html`](./docs/architecture/RISC-V_Architecture_Design.html) | 4+1 视图 + ADR |
| [`docs/requirements/RISC-V_Requirements_v1.4.html`](./docs/requirements/RISC-V_Requirements_v1.4.html) | 需求文档 |
| [`docs/module-design-spec/RISC-V_Module_Design_Specification.html`](./docs/module-design-spec/RISC-V_Module_Design_Specification.html) | 23 模块 LLD |
| `docs/detailed-design/M-*.html` | **23 篇模块详细设计** |
| `docs/detailed-design/STAGE-*.md` | **冻结规格**（每个功能批次一份，含事实 / 裁决 / 施工期订正 / 变异自检） |
| [`docs/reviews/REVIEWS.md`](./docs/reviews/REVIEWS.md) | **检视与欠账唯一入口**（未闭环问题 + 优先级） |
| [`docs/plans/PRODUCT_BOARD_PLAN.md`](./docs/plans/PRODUCT_BOARD_PLAN.md) | 产品 / 开发板架构落地计划 |
| [`docs/progress/PROGRESS.md`](./docs/progress/PROGRESS.md) | 当前需求完成情况 |
| [`AGENTS.md`](./AGENTS.md) | 给 AI Agent 的项目定义与工程纪律 |
| [`CONTRIBUTING.md`](./CONTRIBUTING.md) | 贡献流程（分支 / commit / PR） |

---

## 许可证

**MIT OR Apache-2.0**（见 workspace `Cargo.toml`）。

⚠️ **合规**：仿真内核**自研**，不复用 QEMU 源码（GPL）；如需引入 QEMU，走**进程隔离**。
