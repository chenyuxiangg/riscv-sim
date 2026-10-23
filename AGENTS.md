# AGENTS.md

RISC-V 仿真/调试平台 — 多模块协作的端到端工程，详细设计已落地在 `docs/detailed-design/`，高层级设计在 `docs/{architecture,requirements,module-design-spec}/`。

> 本文件给 AI Agent 读取，遵循 [agents.md](https://agents.md) 开放标准，可被 OpenCode / Codex / Cursor / Aider / Devin / Gemini CLI 等 Agent 共用。
>
> ⚠️ **本文档 ≠ 面向人的项目介绍**：人读的那份是 [`README.md`](./README.md)（能力快照 / 快速开始 / 架构图 / 门禁 / 已知缺口，数字同样从当前树重算）—— **改了代码或数字，两处都要同步**。

## Project overview

基于详细设计文档，这是一个 RISC-V（RV32IMC）指令集仿真与调试平台。

- **技术栈**（自架构 §9.7 选型矩阵 + 用户决策）：
  - **仿真内核**：Rust（自研 RV32IMC ISS，spike 风格），编译为 native + `wasm32-unknown-unknown`（wasm-pack）
  - **前端**：原生 JavaScript / ES modules（**零构建、零依赖**；MVP 不强制单文件自包含，可部署为服务；**ES module 与 Worker 均要求同源 HTTP(S)**，`file://` 直开会被浏览器拦截）
  - **3D 引擎**（P1）：three.js（内联）
  - **持久化**（MVP）：IndexedDB（前端）/ SQLite（服务端 P1）
  - **传输**：postMessage（浏览器内 Worker）/ WebSocket（服务端 P1）；统一 `riscv_sim_contract::protocol::Message` 信封
  - **WASM 工具链**：rustup + `wasm32-unknown-unknown` target + `wasm-pack`
  - **正确性对拍**：Spike golden + riscv-tests（外部工具，不进主代码）
  - **测试**：headless CLI + 断言引擎（LLD §9.7 矩阵）

- **模块划分**（23 个详细设计模块，5 横层 + 3 横切）：
  - **仿真核心**（M-12 ~ M-17、M-20）：事件驱动引擎、RISC-V ISS、总线/内存映射、外设模型、中断控制器、执行后端、快照与确定性事件流
  - **调试与探针**（M-08、M-09、M-11、M-18、M-19）：断点管理、单步引擎、DWARF/反汇编/trace 导出、探针、波形管道
  - **前端 / 工作台**（M-01、M-02、M-03）：工作台装配与多视图路由、视图组件库、视图状态管理
  - **基础设施**（M-04、M-05、M-07、M-10、M-21、M-22、M-23）：会话管理、命令路由与事件分发、传输适配器、寄存器/内存服务、日志、健康管理、持久化 DataStore
  - **文件与解析**（M-06）：ELF / Raw-HEX 解析器

- **关键架构约束**（继承自 AD §2.4 设计原则）：
  - **契约先行**（LLD §2.5）：模块间只引 `riscv_sim_contract` 定义的 DTO/事件/错误码/端口签名；内部实现语言不受约束
  - **GPL 合规**（AD §2.5 R2）：自研 BSD/MIT 内核；不复用 QEMU 源码；如需 QEMU 走进程隔离
  - **零构建**（NFR-05）：前端无打包步骤，原生 ES modules
  - **确定性范围**（AD §8.1 D0，评审 R8）：种子 + 时间戳化输入事件序列 → 结果唯一

详细设计文档：
- `docs/architecture/RISC-V_Architecture_Design.html` — 4+1 视图 + ADR
- `docs/requirements/RISC-V_Requirements_v1.4.html` — 13 条 P0 + 6 层概念空间
- `docs/module-design-spec/RISC-V_Module_Design_Specification.html` — 23 模块 LLD
- `docs/detailed-design/M-01..M-23_*.html` — 23 篇模块详细设计
- `docs/reviews/REVIEWS.md` — **检视与欠账的单一入口**（欠账总表 + §3 两轮检视归档 + §4 模块报告索引 + §5 变更索引）
- `docs/reviews/M-*_Review.md` / `FRONTEND_*_Review.md` — 9 份模块详细检视报告
- `docs/detailed-design/STAGE-1_Wasm_Bridge.md` — **第 1 阶段 WASM 桥接口规格（冻结）**：契约命令 ↔ 事件的规范映射表（**线格式以 STAGE-2 为准**）
- `docs/detailed-design/STAGE-2_Contract_V2.md` — **契约层 v2 规格（冻结，`schema_version = 2`）**：数值策略 / 命令与事件新增 / 命名歧义处置 / 错误族与传输帧 `crc` / 快照 DTO 小修 / 端口文档订正
- `docs/detailed-design/STAGE-13_V028_Watchdog_HMS_Logging.md` — **第 13 阶段规格（v0.28 · 活锁处置链路批 · D-115 + D-114 + M-21 / M-22 全量落地，冻结）**：宿主看门狗（时间预算 200 ms + 滑动窗口三次确认 + HMS 同步终止 + `WATCHDOG` 日志 + 幂等标志）/ M-21 日志五带环形缓冲 / M-22 健康管理 HMS / M-23 通用字节归档 / M-17 无进展守卫 / 裁决 A~N / 验收 A-1~A-6 · B-1~B-10 · C-1~C-12 · D-1~D-10（⚠️ 经**九次**施工期实测订正）
- `docs/detailed-design/STAGE-12_V027_Platform_Description.md` — **第 12 阶段规格（v0.27 · 内存图通用化 B 批 · D-123-B + D-82，冻结）**：平台描述格式（JSON）/ 描述 DTO 与**唯一**校验器落契约层 `platform.rs`（V-1 ~ V-12 十二条规则）/ 器件工厂**双视图**（擦除视图 + 有类型句柄）/ 外设窗口大小由器件自报 / 自动选档的「区间在外层」算法与 `auto_selectable` 数据化 / **D-82 的定性**（台账原文已被实测推翻 ⇒ 不新增专用派发路径 ⇒ 改补 guest 指令级端到端判别力）/ 十七条既有测试锚点的逐条处置 / 验收 A-1 ~ A-8 · B-1 ~ B-12 · C-1 ~ C-4（⚠️ 经**七次**施工期实测订正）

## Setup commands

工具链：Rust stable（rustup） + `wasm32-unknown-unknown` target + `wasm-pack`（前端 wasm-pack；服务端原生 Cargo）。

- Install deps:    `rustup target add wasm32-unknown-unknown && cargo install wasm-pack`
- Build (native):  `cargo build`（开发）/ `cargo build --release`（发布）
- Build (WASM):    `tools/build-wasm.sh` 或 `tools/build-wasm.sh --release`
- Test:            `cargo test --workspace --no-fail-fast --lib --tests`（单元 + 集成 + 黄金对拍；⚠️ **不带 `--lib --tests` 会因 doc-test 假 HANG**）· ⚠️ **判「全绿」必须看退出码 + `grep -cE "^test result: FAILED"` 为 0**；汇总的失败数在 `awk -F'[ ;]'` 的 **`$7`**（**不是 `$6`** —— 连续分隔符会产生空字段 ⇒ `$6` 永远算出 0 = **假的**）
- Test (前端):    `tools/check-front.sh`（node:test 冒烟门禁，**312 用例 / 57 suite**；Node ≥ 22）
- Test (WASM 桥): `node tools/wasm-smoke.mjs`（需先 `tools/build-wasm.sh`；在 Node 里驱动真 wasm 产物跑完整桥，产物不在则跳过）
- Test (浏览器真机): `tools/browser-verify.mjs`（零依赖 CDP 脚本，需本机 Chrome + 已构建的 WASM 产物 + `python3 -m http.server 8080 --directory front`；**不进默认门禁**，按需运行）
- Test (黄金对拍): `tools/fetch-golden-deps.sh`（拉 riscv-tests + 交叉工具链到**仓库外**）→ `tools/golden-run.sh --self-check`（变异金丝雀）→ `tools/golden-run.sh --keep-going`（全量）
- Test (真实 RTOS 对拍): `tools/boot-osz.sh`（**不进默认门禁**：需要**仓外**的 osz ELF 与本机的 Spike；ELF 路径取 `$OSZ_ELF`，默认 `/tmp/osz-artifacts/osz.elf`）⇒ 判决口径是「**两侧 UART TX 字节流逐字节相同**」⇒ 输出 `VERDICT MATCH / MISMATCH / NO-VERDICT`
- Lint:            `cargo clippy --workspace --all-targets -- -D warnings`
- Format:          `cargo fmt --all`
- Typecheck:       N/A — Rust 编译器即类型检查

工程结构（5 横层 + 3 横切映射到 3 个 crate）：

```
riscv_sim/
├── Cargo.toml                # workspace
├── rust-toolchain.toml       # stable + wasm32 target
├── crates/
│   ├── contract/             # 契约层（DTO/事件/错误码/端口签名，所有模块都引它）
│   │                         #   error.rs（14 个错误族 + 场景码常量）/ protocol.rs / model.rs
│   │                         #   numstr.rs（虚拟时间的十进制字符串 serde 辅助：标量 u64_decimal + 列表 u64_decimal_list，v2 新增）
│   │                         #   snapshot.rs（可序列化快照 DTO）/ port.rs
│   ├── kernel/               # 仿真内核（16 个子模块 M-06/M-08/M-09/M-11/M-12/.../M-23
│   │                         #   + kernel_host 装配层（非 23 模块之一））
│   └── server/               # 服务端形态（P1 占位，MVP 不强制）
├── front/                    # 原生 JS 前端（ES modules，零构建）
│   ├── index.html
│   ├── app.js                # hash 路由 + 启动引导
│   ├── views/                # M-01 / M-02 / M-03
│   ├── services/             # M-04 / M-05 / M-07 / M-10
│   ├── dev/                  # mock 内核 + demo 数据（仅开发期）
│   ├── lib/                  # 契约 JS 镜像（message.js）+ 传输帧（frame.js）
│   └── style.css
├── tools/
│   ├── build-wasm.sh         # wasm-pack 包装（`--target web`，输出带命名导出的原生 ESM）
│   ├── check-front.sh        # 前端门禁（node:test）
│   ├── stage13_check.py      # STAGE-13 冻结规格的 7 项自检（表格列数 / 标题层级 / 代码围栏 / 裁决引用 / 章节完整 / 验收交叉引用 / 关键约束字面量）
│   ├── browser-verify.mjs    # 浏览器真机复验（零依赖 CDP，按需运行，不进默认门禁；osz 段含 E-6 / E-7 / E-8 三条判别式）
│   ├── wasm-smoke.mjs        # WASM 桥冒烟（Node 驱动真 wasm 产物，按需运行）
│   ├── fetch-golden-deps.sh  # 拉 golden 外部依赖（riscv-tests + 交叉工具链）到仓库外
│   ├── golden-run.sh         # Spike 对拍：同一 ELF 两侧跑，比对判决（含变异自检）
│   ├── elf-symtab.mjs        # 零依赖 ELF32 .symtab 解析器（真机段按**符号名**取地址）
│   └── watchdog.pl           # 可移植超时看门狗（fork + setpgrp + 进程组 SIGKILL；124 = HANG）
├── tests/
│   ├── unit/                 # 单元测试
│   └── golden/               # Spike 对拍 harness
│       ├── env/              # 零 CSR 最小 env（riscv_test.h + link.ld）——绕开陷阱投递
│       ├── canary/           # 变异金丝雀（两侧都必须报 FAIL 3）
│       ├── corpus.txt        # 68 例语料清单（march + status + env 变体）
│       └── README.md         # 协议、语料边界、Zicclsm 口径
└── docs/                     # 50 篇设计文档（26 篇 HTML + 24 篇 Markdown，含 STAGE-1 ~ STAGE-13 十三份规格）+ reviews/（1 入口 + 9 份模块报告）
```

## Project layout

### 23 个模块的代码位置映射

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | `front/views/m01_workbench.js` | ✅ v0.10 闭环（20/20 评审） |
| M-02 | 视图组件库 | `front/views/m02_viewcomponents.js` | ✅ v0.10 闭环；v0.11 加调试命令条 + 寄存器/内存视图；✅ **v0.20：反汇编面板**（PC 高亮 V-12 / 压缩指令原样显示 / 导出入口与「继续导出」按钮）；✅ **v0.25：落地视图 `embedded` 补组件（D-33）**（连接状态 + 当前 PC + 虚拟时间 + 四个子视图入口；⚠️ **PC 与虚拟时间只来自既有状态源**（`embedded` 已进 M-10 的 `REGISTER_MEMORY_VIEW_IDS`）⇒ **不造第二个真相**；**连接状态由 app 层 `publishLinkState` 从 M-07 推进** ⇒ 视图层不碰服务层；**跳转交 M-01 的 `route()`** ⇒ M-02 → M-01 单向依赖、M-02 不自行实现路由）；✅ **v0.25：导出补下载动作（D-88）**（`Blob` → `createObjectURL` → `<a download>` → 点击 → 下一宏任务 `revokeObjectURL`；⚠️ **文件名由内容派生**（`trace-execution.csv` / `trace-waveform.vcd`）且源与格式**不可信** ⇒ 消毒成 `[a-z0-9_-]`；⚠️ **`<a download>` 不能常驻**（常驻的 `blob:` href 要么一直有效即泄漏、要么早就失效即点不动）⇒ 面板里常驻的是**按钮**、锚点点击时当场造出） |
| M-03 | 视图状态管理 | `front/views/m03_viewstore.js` | ✅ v0.10 闭环 |
| M-04 | 会话管理器 | `front/services/m04_session.js` | ✅ v0.11 实现（状态机 + 断线重连） |
| M-05 | 命令路由与事件分发 | `front/services/m05_commandrouter.js` | ✅ v0.11 实现（订阅/退订/一次性订阅）；v0.11.1 补 `once()` 一次性语义的观察与回归用例；✅ **v0.17 契约 v2**：topic 路由**表驱动**（唯一真相 = `front/lib/message.js` 的 `EVENT_TOPIC`），新增 3 个事件变体的路由（`Registers` → `sim.registers` / `Negotiated` → `sim.negotiated` / `SimStateDelta` → `sim.state.delta`）+ `WaveformHistory` 换 topic（`waveform.delta` → `waveform.history`）——**3 条新命令**（`ReadRegisters` / `WriteRegister` / `Negotiate`）的回复经同一条表驱动路径落位 |
| M-06 | ELF / Raw-HEX 解析器 | `crates/kernel/src/m06_elf/` | ✅ v0.11 加固 + **首次模块级 review**（D-03 阻断级消除）；v0.11.1 检视轮再修 IHEX 嗅探判据 + bss gap 溢出；✅ **v0.20：带出调试节区原始字节**（`LoadImage.debug_sections` + `ParseLimits.debug_section_bytes = 8 MiB`）——M-06 仍是**唯一**的 ELF 容器解析者，M-11 只解析 M-06 带出的节区字节 ⇒ **M-11 没有引入第二套 ELF 解析器**；✅ **v0.21：台账 🟡-4 的残留动作已闭环（D-90）**——`riscv-golden` 自己的 `find_symbol` **已删**，改为 M-06 带出 `.symtab`/`.strtab` 原始字节 → M-11 的 `SymbolTable` 解析 ⇒ **全仓只剩一处生产符号读取者** |
| M-07 | 传输适配器 | `front/services/m07_transport.js` | ✅ v0.11 实现（Worker + WebSocket 双适配器）；**v0.13 第 1 阶段起默认挂真 WASM 内核**（`front/kernel-worker.js`），mock 退化为测试替身；✅ **v0.17 契约 v2：传输帧新增 `crc` 字段**（`front/lib/frame.js` 实现 CRC-32 / IEEE 802.3 反射式，覆盖 payload 的**规范 JSON**（递归排序键）；8 位小写 hex；**缺失 → 放行**（旧生产者）/ **算错 → 拒绝该帧**（`E_PROTO-06`））——`crc` 属**前端传输帧**，**不在**契约信封上 |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | ✅ **v0.13 第 1 阶段实现**（53 行 stub → 457 行）：断点表（按 addr 索引 + id 反查）/ 观察点表（地址 + 方向 + 宽度匹配）/ `hit_check` 取指前命中 / 未对齐与重复注册拒 `E_CTX-07` |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | ✅ **v0.13 第 1 阶段实现**（71 行 stub → 308 行）：`StepKind` 四种粒度 + `Plan` + `StepOutcome`；`Over`/`Out`/`Line` **显式降级**为指令级（`degradation()` 把降级说出来，不悄悄降级） |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | ✅ v0.11 实现（订阅寄存器/内存快照）；v0.11.1 补断线重连后重订阅；**v0.13 补 `resubscribe()` / `loadProject()`**——装载工程后内核侧订阅会失效，必须**强制**重订（幂等的 `ensureSubscribed()` 不够，台账 **D-45**）；✅ **v0.17 契约 v2：接 `ReadRegisters` / `WriteRegister` 命令 + `Registers` 事件**（`readRegisters()` / `writeRegister()` + `applyRegisters()` 落缓存；⚠️ `count = 0` **回复空区间**、**不是不回复**——见冻结规格 §5） |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | ✅ **v0.20 落地**（53 行 stub → **3 206 行 / 75 条用例**）：五个子模块（`render.rs` 助记符渲染 / `symbols.rs` `.symtab` 解析 / `disasm.rs` 反汇编序列 + 按地址缓存 / `dwarf.rs` 行表降级态 / `trace.rs` 轨迹环 + CSV/JSON 导出）+ `SymbolService` 端口；**反汇编复用 M-13 的 `Decoder` + `classify`**（裁决 A ⇒ **零第二套解码表**——LLD §3.4 的「独立表 + 一致性门禁」作废，漂移**结构上不可能**）；**只出 ISA 助记符、压缩指令保留 `c.` 前缀、不做伪指令别名**（裁决 C，黄金参照 = `objdump -d -M no-aliases`）；**M-11 不依赖 M-19**（裁决 D ⇒ 导出由宿主路由）；✅ **v0.22 批（5 244 行 / 94 条用例）**：`dwarf.rs` **440 → 2 147 行**，从「**只识别版本、不解析行表**」换成**真解析** `.debug_line`（**v2 / v3 / v4 / v5**）的行号程序 + 行号状态机（全部标准 + 扩展操作码，含真实产物才会发的 `DW_LNE_set_discriminator`）+ 特殊操作码 + **按单元降级** + **多单元归并**；新增 `SymbolIndex` 承载符号表与行表，`SymbolService::resolve_line` 从「**恒说没有 DWARF**」这个谎言变成**有真值**的实现；⚠️ **两态不得混**：「查过行表、没命中」= `Missed`，「没查」= `NoDwarf` ⇒ **绝不**产出猜出来的行号；⚠️ **不解析 `.debug_info` / `.debug_abbrev`**（裁决 B）⇒ v2~v4 的文件条目**没有编译目录**（如实降级，**从不伪造绝对路径**）；⚠️ **`DW_FORM_strx*` 整单元降级**（裁决 C）；⚠️ **访存轨迹不含访存值**（要改 M-18 的环布局，P1）；✅ **v0.22 检视轮 Lane W1 修三条中危**：未收尾的段**不再覆盖到地址空间末尾**（改为「末行地址加一」）· 空文件名**不再产出 `Found`**（落到 `Missed`，契约层**不加第三态**）· v5 与 v2~v4 的**文件索引基址**（0 vs 1）补两条用例钉住 ；✅ **v0.24 批（D-121 闭环）**：`Disassembler` 新增 `disasm_with_lines(addr, count, read, resolve)`（`resolve` 收地址、返 `Option<LineEntry>`）⇒ `DisasmEntryDto` 填**真** `file` + `line`（查不到就是 `None`，**绝不编造**、**不拿别的地址的行号顶替**）；旧 `disasm` 签名**一字未改**（走旧路的条目两个字段恒为 `None`）；新增**可观测**计数器 `line_resolutions()` ⇒「每条指令只查一次行表」是**可证的**不是散文；`SymbolIndex::resolve_line_entry` 把三态映射成 `Option`（`Found` ⇒ `Some`，`Missed` / `NoDwarf` ⇒ `None`）⇒ **24 条新用例**（单元 13 + 集成 11，集成测试自带手写的最小 DWARF 4 `.debug_line` 节区 ⇒ **仓库自包含**）；✅ **v0.27 批（§6.3 锚点 15）**：`tests_m11.rs:38` 的 `DEFAULT_RAM_*` / `DEFAULT_GPIO_BASE` 取值来源改成**从平台描述查**（`platform::builtin_description` + `regions_by_base` + `device_base`）⇒ **既有断言的语义不变**（改的是「取值来源」，不是删掉或放松断言） |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | ✅ v0.11 加固（快照还原 now_ps+事件队列、推进不空转、seq 不撞号）；v0.11.2 增契约快照 DTO 转换层（`snapshot_dto` / `restore_dto`，**先校验后应用**）+ `Engine::reset()`；✅ **v0.15 D-07：唯一主循环 `run_until(executor, until_ps)`（LLD §3.1 结构）+ `InstructionExecutor` 内部接缝（M-12 不依赖 M-13）+ `tick_peripherals` 唯一外设时钟同步点 + `TIME_QUANTUM_PS` 订正为 1 µs**；✅ **v0.22 批 D-101①：`restore_full` / `restore` 两条恢复路径都走已实现的 CSR 表过滤** ⇒ 恢复含**未实现 CSR** 的快照 ⇒ guest **读回 0**；✅ **v0.22 检视轮 Lane W2**（**2 493 行 / 30 条用例**）：**D-30 闭环**（M-12 **首次**有独立模块级 review）——**VD-F3**：唯一主循环 `run_until` 加**无进展守卫** `NO_PROGRESS_WINDOW = 1024` ⇒ 活锁**报错退出**（`E_ENG_03_NO_PROGRESS`，**平台故障**，与 guest trap 严格区分）⇒ **D-83 的 M-12 生产侧闭环**；**VD-F1**：`snapshot_full` 把迟到事件按 `max(at_ps, now)` **归一化** ⇒ 引擎产出的快照**能过契约自己的校验器**；**VD-F2**：`restore_full` 的 `next_seq` 下限修正**补判别力用例**（它是**公开 API 上的安全网**、不是死代码）；⚠️ **残留**：M-17 `step_n` 有**同形状**活锁（**D-114**）· watchdog 只做了「无进展检测」这一半（**D-115**） ；✅ **v0.24 批（D-120 闭环）**：M-12 里自写的那份 `E_ENG_01/02/03` 字面量**已删** ⇒ 改为引用**契约层** `error.rs` 的具名常量 ⇒ **全仓只剩一个定义处**（`grep "pub const E_ENG"` 的唯一命中就是契约层）；判别力不只看「定义处只有一处」：另配**等值断言** + **行为对拍**（引擎实际报的码与契约表一致） |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | ✅ v0.11 **RV32I 全集 41 条** + 模块级 review；v0.11.1 检视轮修 SYSTEM funct3 阻断级缺陷；v0.12 **RV32I 与 Spike 逐例对拍通过**（语料 51 例中 42 例判决一致；失败用例号一致性由变异金丝雀证明）；v0.13 取指改走 `Bus::read_fetch`（**取指不匹配观察点**，V-A 🟠-2）；✅ **v0.16 M+C 扩展落地**（**RV32M 8 条 + RV32C 26 条**）：`compressed.rs` 字段级展开 + `alu.rs` 乘除 8 函数 + `Decoded` 增 `compressed`/`compressed_raw`/`illegal`；**golden 语料 51/51 全通**（D-43 闭环）；✅ **v0.19 M-16 落地批**：新增 **CSR 指令族**（`csrrw` / `csrrs` / `csrrc` / `csrrwi` / `csrrsi` / `csrrci`）+ **`mret`** + **唯一**陷阱入口 `enter_trap`（`mepc` / `mcause` / `mtval` 写 CSR、`mstatus.MIE` 关中断、跳 `mtvec`）；**`TrapCause` 拆三变体**（取指 0 / 读 4 / 写 6 —— **D-63**）；`mtval` 落 CSR `0x343`（**D-64**）；⚠️ **`StepOutcome` 只剩 `Advanced` / `Trapped`**（`HostPause` / `Breakpoint` / `Watchpoint` 三个死变体已删 —— **D-59 / D-72**）；⚠️ **M-13 只管写 CSR + 回报「刚写了哪个 CSR」**（`Iss::take_last_csr_written`），平台副作用在 M-17；✅ **v0.21 批（7 052 行 / 194 条用例）**：`misa` 由 `csr_read` 特判返回 `0x40001104`（MXL=1 + I/M/C，**不预置进 CSR 文件**⇒ 保住快照「空」判定）；**陷入时 `mstatus.MPP ← M`**（`enter_trap` 写 `MPP`——⚠️ 原实现**漏写**，与参考侧实测分歧 ⇒ **D-97 已闭环**）；✅ **v0.22 批（7 843 行 / 210 条用例）· 闭环三条分歧**：**D-99**（未映射**数据**访存 ⇒ 架构陷阱 `cause 5`（load）/ `cause 7`（store），`mtval` = 故障地址；判据落在 **family + scenario**（`is_unmapped_access`）**不靠 message 文本**；⚠️ **根因**：`BusMemory` 原先把所有总线错误重包成 `ISS-01` ⇒ 「未映射」**无法**与其它总线错误区分 ⇒ 改为**保留 family / scenario**）· **D-93**（`mepc` 三路径各管各的：**用户写**掩 `~1`（IALIGN=16，非 32）· **陷入门**写**精确 PC** 不丢 bit 1 · **`mret` 取 PC** 再掩 `~1`）· **D-98**（`mstatus` 加 WARL 掩码，常量 `MSTATUS_WARL_MASK = 0x807F_FFEA` 住**契约层**、M-13 **只引用不复制字面量**）；⚠️ **取指侧**的未映射访问本批**不做**（仍是总线错误，不是 cause 1）⇒ 台账 **D-103**；⚠️ `misa` 仍**只读**（台账 **D-95**）；✅ **v0.25 批（D-103 闭环 · 裁决 B）**：**取指侧的未映射访问改走架构陷阱 cause 1** —— 新增 `TrapCause::InstructionAccessFault`、`mcause() => 1`（⚠️ **变体↔码的映射只有一处**，`mcause()` 之外不复制字面量）；判据沿用数据侧那条 `is_unmapped_access`（**family + scenario**，不靠 message 文本）⇒ **外设未接线等其它总线错误原样上抛、不被误判成 cause 1**；`mtval` = 取指地址、`mepc` = 陷阱入口处的 PC（⚠️ **依赖裁决 A 先落**：否则映射区末端的压缩指令会被误报成陷阱）；取指改走 `Bus::read_fetch_instruction`（**按指令真实占用的字节读** —— D-86 闭环）；**黄金语料**新增 `local/unmapped_fetch`（自包含，guest 自查 `mcause` / `mtval` / `mepc` **三项**）⇒ 语料 **67 → 68** |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | ✅ v0.4 完成 + v0.7.2 补完；v0.11 接线 PR；v0.13 新增 `read_only()`（调试读不计入统计 / trace / 观察点）与 `load_bytes()`（镜像装载的**唯一**写路径），并给 `BusAccess` 增 `is_fetch` 标记（**取指 vs 数据访问**）；**D-46 增 `add_hook()`（追加式 hook，`set_hook` 保留「替换全部」语义）+ `peripheral_instances()` + `tick_peripherals()`**；✅ **v0.18 会话快照：新增 `export_map()` / `load_map()` + `export_pages()` / `load_pages()`**（`load_map` 重建 **RAM 清零** / **ROM 原地保留内容**（`base`/`size` 不符报 `E_BUS-12`） / **外设区域原地保留**；新增 `E_BUS-10/11/12`）；✅ **v0.25 批（D-86 闭环 · 裁决 A）**：新增 **`Bus::read_fetch_instruction`** —— **一次取指 = 一个事件**、事件宽度 = **指令宽度**（压缩指令**只读 2 字节**，32 位读 4 字节）；⚠️ **宽度探查是「纯 peek」**（走 `read_only`：**不计次、不 fire hook**）⇒ 落点另行补记 ⇒ **一次取指 = 一次读派发 + 一次落点 + 一个事件**（统计与改动前逐条相同，既有断言一条没改）；⚠️ **落点在总线侧而不是 M-13**（M-13 侧调两次 `read_fetch` ⇒ 取指变**两个**事件 ⇒ 破掉「一次取指 = 一个事件」与 D-46 的判别力）；⚠️ **访存轨迹里压缩指令的取指条目宽度由 4 变 2**（裁决 E：契约层**不升版**，DTO 形状不变且 2 本就在取值域 1/2/4 内）；✅ **v0.26 批（D-123·A 闭环 · 裁决 A / B / C / D）**：`RamBackend` 从「一整块 `Vec<u8>`」改成 **4 KiB 页表**（`pages: BTreeMap<u32, Box<[u8]>>`，键 = region 内页号）——**写时分配**、读没写过的页回 **0**、三个**计数器**（`allocated_pages()` / `allocated_bytes()` / `ram_allocated_bytes()`）可观测；⚠️ **写路径三步顺序不可交换**（① 先查 region 边界 ⇒ `E_BUS-04` ② 再查物理占用上限 ⇒ `E_BUS-09` ③ 最后才物化页 ⇒ **越界写与超限写都不留下任何页**）；**护栏语义转换**：`max_region_bytes` 从「声明大小上限」改成「**按 region 种类定**的相对预算上限」（RAM 按**物理占用** · ROM 与**外设窗口**按声明大小）⇒ `E_BUS_09_TOO_LARGE` **保留原号**、**不新增码**；**快照页**改 4 KiB 粒度且**只导出写过的页**、`load_pages` 加**页网格对齐**校验并保留两阶段（校验不过 ⇒ RAM 原封不动）；**D-13 可见性收紧**（`Region` 的四个字段与 `RomBackend.bytes` 转模块内可见 + 只读访问器 `Region::base()` / `size()` / `kind()`）；✅ **v0.27 批（D-123·B 闭环 · 裁决 H）**：**M-14 零行为改动** —— 只允许 `REGION_KIND_RAM` / `_ROM` / `_PERIPHERAL` 三个常量的**归属上移**到契约层（`crates/contract/src/session.rs`）、M-14 改 `pub use` ⇒ **M-14 内部调用点的字面量一条不改**、既有断言一条不改（B-12）；⚠️ **描述层与 M-14 的关系**：描述的**消费者只有宿主** ⇒ M-14 不需要知道描述的存在 ⇒ 描述的 `kind` 词汇表**恰好**是 M-14 已有的三个值 ⇒ **不新增分类分支**；⚠️ **D-82 不做** `RegionType::Clint`、**不新增**专用派发路径 —— CLINT **早已挂在总线上**、M-14 的 `PeripheralBound` 派发路径**本来就是通用的**（`offset` **已是 region 相对偏移**）⇒ 新增专用路径 = **第二条派发路径** ⇒ 违反「一个约束一个落点」 |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | ✅ v0.7 三外设 + v0.11 接线 PR；✅ **v0.18 会话快照：新增 `session_state()` / `restore_session_state()` + 自由函数 `collect_session_state()` / `restore_session_states()` / `validate_session_states()`**（回灌**两阶段**：先全量**只读**校验，全过才写一个字节）；✅ **v0.23 批（真实 RTOS osz 落地）**：**UART 重做成 ns16550**（8 个字节寄存器 THR/RHR/DLL@0x0 · IER/DLM@0x1 · FCR@0x2 · LCR@0x3 · MCR@0x4 · LSR@0x5 · MSR@0x6 · SCR@0x7；`LCR.DLAB` 真切换；`LSR` 按位义重排；**无损 TX 捕获环**默认 4 KiB）+ **新建 `clint.rs` / `plic.rs`**（QEMU virt 布局：CLINT `msip`/`mtimecmp` 分步 64 位写/`mtime` 64 位读，**单位是周期**；PLIC priority/pending/enable/threshold/claim+complete，claim 判据**严格大于门限**、complete 忽略非法源 id）；⚠️ **`LSR` bit5 = THRE**（发送器还能再收一个字节）**不是**「环是空的」—— osz 的 `hal_uart_putc` 是自旋，取错语义会让 osz 每发一字节就卡死一次 ；✅ **v0.24 批（D-117 / D-118 / D-122 闭环）**：**CLINT 与 PLIC 进会话快照**（契约层加 `Clint` / `Plic` 两个变体，`SESSION_SNAPSHOT_SCHEMA_VERSION` 1 → **2**，旧载荷被 `E_SNAP-05` **显式拒绝**；器件侧补 `session_state()` / `restore_session_state()` / `validate_session_state()`，⚠️ **校验不再默认放行** ⇒ 畸形状态被拒）；**UART 的七个 ns16550 寄存器进快照**（`LCR` / `IER` / `FCR` / `MCR` / `SCR` / `DLL` / `DLM`，⚠️ **`LCR.DLAB` 位本身也要还原**）；**接收侧从单字节 `rx: Option<u8>` 加深成 `RX_FIFO_CAPACITY = 16` 的队列**（`VecDeque<u8>`：`RX_VALID` = 队列非空、读 `RHR` **弹队头**、`receive()` **入队尾**、满了**丢最新**并锁存 `LSR` 溢出位 ⇒ 与 TX 捕获环「丢最新保最早」同口径 ⇒ 丢失**看得见**不是静默的；⚠️ **TX 侧一个字未改**）⇒ **一行 shell 命令进得去 guest**（D-122 的根因）；快照侧加 `rx_queue` 装**整个**队列、`rx` 保留为队头字节，两者之间有一条不变式（`rx == rx_queue[0]`）；✅ **v0.26 批（裁决 E）**：**TX 捕获环进会话快照**——`UartStateDto` 新增 `tx_ring`（整环、队头在前）与 `tx_dropped`（丢弃的累计数 ⇒ **丢失看得见**），`tx` 保留为**队头**的冗余表示 ⇒ 不变式 `tx == tx_ring[0]` 与既有的 `rx == rx_queue[0]` **完全同构**；还原时整环灌回、**`tx` 不单独还原**、`tx_dropped` 取**覆盖**（不取累加）⇒ 快照 = 状态 |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | ✅ **v0.19 落地**（44 行 stub → **1 453 行**，31 条用例）：挂起队列（按 `(request_time_ps, insert_seq)` **保序**）/ **同线去重** / 优先级仲裁（最高优先，同级按插入序）/ 量子边界 `poll()`（刷新 `mie` 使能 + 定时器电平轮询 + `mstatus.MIE` 闸门 + `active_irq` 单级防护）/ **拉取式**交付（`take_intr` **取走即消失**）/ `export_state()` + `load_state()`（**整体替换**）/ 脉冲回调（**D-26** 的生产者）；⚠️ **`mie` 位映射的唯一互译点是契约层 `std_irq_bit_for_line`**（`line` 空间 1/2/3 ↔ `mie` 位 11/7；旧名 `mie_bit_for_line` 保留为别名）；✅ **v0.21：`mip` 已有生产者（D-84 闭环）**——M-16 的**挂起队列**经契约层 `pending_lines_to_mip_mask` 投影，在 **M-17 写侧**落进 CSR 文件（量子边界 `poll()` 之后 / `irq_clear` 副作用之后，**值变化才写**）⇒ guest 读 `mip` 能看到挂起线（**上一个量子边界**的快照，1 µs = 1000 条指令） |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | ✅ **v0.13 第 1 阶段实现**（stub → **2 329 行**，v0.21 批重算）：`step_n` 真正驱动 M-13 ISS（断点取指前命中 / 陷阱 / 观察点总线 hook 命中）、`read_memory` / `write_memory` 接 M-14 总线、`set_breakpoint` / `set_watchpoint` 接 M-08 表；v0.11.2 的 `snapshot_full` / `restore_full` 接线保持；**D-46：观察点 hook 改 `add_hook`（不顶掉别人的）+ 每条指令前同步一次外设时钟（边沿才有真实时间戳）**；✅ **v0.19 M-16 落地批**：取指边界按「`now_ps - last_poll_ps >= TIME_QUANTUM_PS`」**采样** → `poll()` → **拉取式**取走交付 → `enter_trap`（**中断不停批次**）；**`irq_clear` CSR 写的副作用**（掩码 **bit L = 线 L**）落在 M-17；⚠️ **判据是「这条指令写了 `irq_clear`」**（M-13 回报），**不是**「值变了」——后者会把**第二次**清除**静默丢弃** ⇒ 那条线被**永久堵死**（v0.19 检视轮 **F-1**）；✅ **v0.15 D-07：`IssExecutor` 实现 M-12 的 `InstructionExecutor` 接缝（指令驱动与时间驱动**共用一条** ISS 路径）+ `install_peripheral_ticker`（外设 tick 接缝）+ `pending_writes` 真正提交到总线 + `continue_until_event` 走唯一主循环**；✅ **v0.20：指令边界把虚拟时间告诉总线**（`self.bus.set_now_ps(now_ps)`）——M-14 的 `BusAccess::ts_ps` 由此才有真实时间；⚠️ **此前生产路径**从未调用过它** ⇒ 访存轨迹的时间戳**恒为 0**（导出看着有数据，时间轴却是平的）；✅ **v0.21：`mip` 生产者接线（D-84 闭环）**——`IssExecutor::project_mip` 把 M-16 的**挂起队列**投影写进 CSR 文件（**两个刷新点**：量子边界 `poll()` 之后 / `irq_clear` 副作用之后；**值变化才写**）⇒ 全仓**只有这一个** `CSR_MIP` 写入点；✅ **v0.22 批 D-100 闭环**：`SharedInterruptPort::pending_lines()` 改成 `Option<Vec<u16>>`（`None` = **状态未知**）⇒ `project_mip` 遇 `None` **跳过写** ⇒ 保住「**读失败不得改写 guest 状态**」这条不变式（判别式：真的造出争用时 `mip` **不被写**）；✅ **v0.28 批（D-114 闭环）**：`step_n` 接上与 M-12 **同形状**的**无进展守卫**（`no_progress` 计数 + `engine.now_ps()` 前后比较 ⇒ 中断交付**不算**进展，与 M-12 严格同口径）⇒ 连续达到 `NO_PROGRESS_WINDOW` 次即 `Err(E_ENG_03_NO_PROGRESS)`（族 `Engine`）⇒ 反复交付同一中断从**活锁挂死**变成**报错退出**；⚠️ **窗口常量引用 M-12 的定义**、**场景码引用契约层的定义** ⇒ 全仓各**只有一个**定义处；⚠️ **归零按 `outcome` 在 `match` 之前统一判定**（`Stop` 分支必然 `break` ⇒ 分支内归零是**死赋值** ⇒ `unused_assignments` 告警 ⇒ `-D warnings` 编译不过）；⚠️ **守卫触发只 `return Err`、不改引擎状态** ⇒ 宿主可安全重入；停滞**不**记成 guest 陷阱（`last_stop` 保持 `None`） |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | ✅ v0.8 完整 `ProbeRegistry` + v0.9 `EdgeSink` + v0.11 接线 PR；v0.11.1 修总线 hook 的 RefCell 重入 panic；**D-46：正式挂上生产总线**（此前只在测试里接线）；✅ **v0.20：访存轨迹加 `is_fetch` 标记**（取指 vs 数据访问**可区分**）——M-11 的反汇编缓存失效判据与轨迹导出**都靠它**分辨「取指」与「访客改内存」 |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | ✅ v0.9 完整 `WaveformPipeline`：边沿编码 + 每通道环 + ≤20ms 批量推送 + CSV/VCD 分块导出；✅ **v0.17 契约 v2 的命名边界**：**history 形**（`schema_version + streams[]`）随内容改名 → `model::WaveformHistory` + 事件变体 `EventPayload::WaveformHistory` + topic `waveform.history`；**live 批次形**（`seq_start/seq_end/t_virt/entries`）**保留原名** `model::WaveformDelta`（**不是**死代码，是 M-11 / **D-50** 的落点，本批未上线） |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | ✅ **v0.18 会话快照：按六段组装**（`capture_session()`）+ `record()` 落记录（`SnapshotRecord { id, payload, label }`——**无第二份 `cp_meta`**）+ `validate_session()`（**只校验、不还原**；⚠️ **同名不同义** = 「一码多义」的变体，有源码级守卫防回退）。⚠️ 输入事件流 / `CrashKit` 等**仍 stub** |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | ✅ **v0.28 全量落地**（84 行 stub → **1 075 行 / 24 条用例**）：**五带**环形缓冲（trace 4 000 / debug 1 000 / info 1 000 / warn 500 / error 500 = **7 000**）+ 每带溢出计数 + 入口过滤 + **UTF-8 字符边界截断**（24 / 12 / 128 字节）+ `truncated` 标记 + `query` **纯读** + CSV / JSON 导出（8 列全含、虚拟时间真实）+ `set_level` 返回旧值；⚠️ **「零分配」在 Rust 下不成立**（`String` 堆分配且长度可变 ⇒ 「定长槽」无直接对应物）⇒ 按「有界长度 + 截断标记」**近似满足并如实登记** ⇒ **D-128**；⚠️ **`query` 按级别分带返回** ⇒ **没有「全局最近」口径** ⇒ **D-134**；保留 `LOG_FAMILY`，删 `MAX_LOG_RECORDS` 与旧 `Level` / `LogRecord` |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | ✅ **v0.28 全量落地**（74 行 stub → **1 577 行 / 23 条用例**）：`Severity`（⚠️ **刻意不 derive `Ord`** —— 变体序会让 `Fatal` 成**最小值** = 公开 API 陷阱 ⇒ 公开 `severity_rank()`）+ `classify` **四条固定顺序规则** + 致命严重**保留带**（永不丢弃）+ 崩溃冻结 + `Health` **派生且只降不升** + 分角色分发（`Role::Product` 的结果**不含任何日志文本**）+ 降级**只实现 `Terminate`**（其余 `Unsupported` ⇒ **不重试、不报** `E_HMS_01`）+ 重试**至多 3 次** ⇒ 终态**恰好一次** `E_HMS_01` + 指标环 4 096 + 脱敏诊断包（**不含原始日志**）；⚠️ **归档与日志源必须注入**（`install_store()` / `install_log_source()`）⇒ **M-22 → M-21 / M-23 单向、无环**；⚠️ 保留带**无界增长** ⇒ **D-133** |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` | ⚠️ **部分落地**（v0.28 增 `archive(name, bytes)` **通用字节归档**接口 ⇒ `DataStore` 变**六方法**；⚠️ **IndexedDB / SQLite 后端仍 stub**）——v0.18：`DataStore::put_snapshot()` / `get_snapshot()` 改吃 `SessionSnapshotDto`（**写入前先 `validate()`**）+ 新增 `InMemoryDataStore`（键 = `cp_meta.id`）。⚠️ **IndexedDB / SQLite 后端仍 stub** |
| **内核宿主**（装配层 · **不是 23 个模块之一**） | 协议装配：契约命令 ↔ 内核调用 | `crates/kernel/src/kernel_host/` | ✅ **v0.13 第 1 阶段新增**；✅ **v0.28 批（D-115 闭环）**：**宿主看门狗**（`watchdog.rs`：时间预算 `HOST_WATCHDOG_BUDGET_MS = 200` + 滑动窗口 `HOST_WATCHDOG_CONFIRMATIONS = 3` + **纯函数** `classify_window` + 幂等 `HostWatchdog`）⇒ 接线落点**两处且都在循环体内**（`on_run` 的 `Continue` 分支 / `step_in_chunks` 的 `while remaining > 0` 循环体）⇒ 停滞**前 2 个窗口不上抛**、第 3 个才触发处置（上报 HMS ⇒ `Severity::Fatal` ⇒ `Health::Critical` → 记 **一条** `ctx = "ENG-WATCHDOG"` 日志 → 幂等标志 → `degrade(Terminate)` → 上抛 `E_HMS_02`）；⚠️ **宿主时钟不能直接用 `std::time::Instant`**（在 `wasm32-unknown-unknown` 上 **panic** ⇒ WASM 构建一启动就崩）⇒ 新增 `clock.rs` 抽象（native = `Instant`；wasm32 = JS `performance.now()`，无 `performance` 时**优雅降级为 0**）⇒ **D-135**；**10 369 行 / 150 测试**（v0.28 批重算）（**6 091 行** / **98 测试**，v0.22 批重算；**v0.25 批重算为 7 760 行 / 117 测试**）：16 条命令 → 事件的**规范映射**（形状见 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1）/ 镜像装载（ELF + RawHex）/ 默认内存图 / 寄存器全量与增量波形流 / `handle_json` JSON 桥；**D-46：装上探针管道**（M-18 判沿 → M-19 环 → 契约 `WaveformHistory`）+ 默认内存图映射 GPIO 外设；✅ **v0.17 契约 v2：3 条新命令接线**（`ReadRegisters` / `WriteRegister` / `Negotiate` → `Registers` / `Negotiated`）；✅ **v0.20 契约 v3：4 条调试面命令接线**（`Disasm` / `ResolveLine` / `ExportTrace` / `SetTraceEnabled`）——**宿主路由导出**（`Waveform` ⇒ M-19 的环 / `Execution` ⇒ M-11 的轨迹环）⇒ **闭环 D-50 / D-51**；✅ **v0.22 批 Lane A：`on_resolve_line` 改走 `SymbolService` 端口**（此前宿主**自己**判「没有 DWARF」）⇒ 第一次返回**真 `file` + 真 `line`**；✅ **v0.23 批：宿主接线**——**内存图分两档**（`MemoryProfile::Default` 与今天一模一样；`QemuVirt` 铺 `0x8000_0000` 起 **128 MiB**（8 × 16 MiB，受 M-14 每区域上限约束）+ ns16550 UART0 @ `0x1000_0000` + CLINT @ `0x0200_0000` + PLIC @ `0x0C00_0000`），⚠️ **档位按镜像用到的地址自动挑**（装载顺序改成「先解析 → 再挑档 → 再建图 → 最后写镜像」）；**三条外设中断通路**（CLINT `mtimecmp` 周期↔M-16 ps · UART RX→PLIC 源 10 · PLIC 待交付源→M-16 外部线，⚠️ **claim 是 guest 的权利**、平台只读不代 claim）；**console 通路**（UART TX 捕获环 → 64 KiB backlog → 订阅 `sim.console` 推契约 v4 的 `EventPayload::Console`） ；✅ **v0.24 批（D-119 闭环 / J-4）**：接住契约 v5 的新命令 `WriteConsole { bytes }` ⇒ 字节逐个进 UART 的 **RX FIFO**（复用 `uart_receive`）⇒ `LSR` 的 `RX_VALID` 由 UART 模型自己置位 ⇒ 命令末尾调**同步器**（⚠️ **抬线是同步器的活、命令不另写一份** ⇒ 一条命令不产生两个真相）⇒ `IER` 允许时抬 **PLIC 源 10** ⇒ guest 看到 `mcause = 0x8000_000B`（架构码 11）；⚠️ **回复至少一个事件**（前端 M-05 靠第一个带 `reqId` 的事件落地 Promise ⇒ 返回空数组会让前端**永远等不到**）；`on_disasm` 改调 Lane J 的 `disasm_with_lines`（解析器接 `SymbolIndex::resolve_line_entry`，**不相交的字段借用**、不用克隆）⇒ `Disasm` 事件**带真 `file` / `line`**；✅ **v0.26 批（裁决 G）**：**QemuVirt 档从 8 个 16 MiB 区域变 1 个 128 MiB 区域**（区域数 13 → 6）⇒ 护栏预算同步抬到 **128 MiB**（`QEMU_VIRT_MAX_REGION_BYTES`）⇒ guest 看到的地址空间**不变**；⚠️ `new_with_profile` 换档失败会**静默退回默认档**（不 panic）⇒ 任何「QemuVirt 档怎样怎样」的断言**必须先落在布局上** ；✅ **v0.27 批（D-123·B 闭环 · STAGE-12）**：**宿主新增 `platform.rs`**（描述加载 / 器件工厂 / 建图 / 自动选档）+ **三份内置描述**（`platforms/{default,qemu-virt,minimal}.json`，`include_str!` 编译进二进制）⇒ **`map_memory` 的源码里不再有任何地址字面量、任何器件构造调用、任何 `match profile`** ⇒ 「换一份内存图描述、宿主代码零改动」这条唯一判别式成立（B-6 / B-7 / B-8）；⚠️ **器件工厂返回「两种视图」**（擦除视图挂总线 + 有类型句柄供宿主 ⇒ `RefCell<T>` 不变、擦除视图降不回有类型 ⇒ 一个返回值满足不了两边）⇒ 而**器件种类的 `match` 仍然只有一处**；⚠️ **`MemoryProfile` 保留为兼容视图**（裁决 C：`memory_profile()` 按当前描述 id 返回，13 个调用点一字未改）⇒ 新增 `platform_id()` 返回真实 id；⚠️ **自动选档把「今天只查高段」这条差异显式承载在数据的 `auto_selectable` 字段里**（默认 `true`，演示平台 `false` ⇒ 只能被显式指定）⇒ **不是**藏在代码的一份 id 名单；⚠️ **契约层拥有全部 `E_PLT-01` .. `E_PLT-11` 场景码**（宿主侧三类失败也归契约层 ⇒ 宿主只引用、不自建） |

> **MVP 跑通判据**（首个能 cargo test 通过的功能闭环）：
> - `m12_engine::tests::engine_advances_time`（事件队列 + 时间推进）
> - `m13_iss::tests::addi_executes`（RV32I ADDI 解释执行，x1=42）
>
> **v0.11 起新增判据**（`cargo test --workspace` **417** 全绿 = 契约 lib 12 + kernel lib 356 + 集成 38 + golden runner 11）：
> - `m13_iss::tests_rv32i::*`（RV32I 41 条指令 + 黄金编码 + 边界 + CSR 形态 SYSTEM 陷阱 + 步进/回绕/子字存储）
> - `m12_engine::tests::*`（snapshot→restore→继续推进的确定性）
> - `m06_elf::tests_malformed::*`（畸形输入表驱动 + fuzz，均不得 panic）
> - `crates/kernel/tests/m14_m15_m19_wiring.rs`（N5「GPIO 翻转可见」端到端 + 外设路径区域上限）
>
> **v0.11.1 检视轮新增判据**（4 路独立检视后补齐）：
> - `m18_probe::tests::*`（总线 hook 在 `RefCell` 争用下不 panic）
> - `m06_elf::tests::*`（IHEX 嗅探结构判据：raw 二进制首字节 `0x3A` 不再误判；4 GiB 总线边界）
>
> **v0.12 黄金对拍判据**（第 2 阶段；**不进 `cargo test`**，需外部依赖）：
> - `tools/golden-run.sh --self-check` —— **三个变异对照**（自包含 `fail_3.S` 报 `FAIL 3`；**未改动的**上游 `add.S` 报 `PASS`；**篡改期望值的**上游 `add.S` 报 `FAIL 3`），两侧都要一致，不过 = **直接判负**。⚠️ 金丝雀只覆盖「判决运输路径」，**不证明成功路径有判别力**（实测 10 个 ALU 变异漏检 8 个）——判别力由**全量对拍**提供（10/10 抓到）。更强的自检形式见台账 **D-44**
> - `tools/golden-run.sh --keep-going` —— 全量语料：**RV32I 42/42 判决逐例一致**（本语料 42 例全为 PASS；**「失败用例号一致」由变异对照证明**）；M/C 扩展 9 例为 `pending`（**只容忍候选侧跑不了**，其他不一致一律判负；`pending` 行必须带理由 + 台账号）
> - ⚠️ **参考侧 ISA 必须是 `rv32imc_zicclsm_zifencei`**（理由见台账 D-42）；候选侧 runner 是 `crates/kernel/src/bin/riscv-golden.rs`
> - ⚠️ **两侧内存图必须一致**（脚本已统一：栈窗口 4 KiB + RAM 4 MiB）——否则「触碰镜像之外地址」的用例会一边 fault 一边正常
> - ⚠️ **任何一侧出不了判决（NOVERDICT）即判负**——「两侧都没判决」不等于「两侧一致」
>
> **v0.11.2 快照 DTO 判据**（契约新增可序列化 DTO，D-24 根因闭环）：
> - `crates/contract/src/snapshot.rs::tests::*`（DTO 的 JSON **精确形状** + `validate()` 六类拒绝）
> - `crates/kernel/tests/m12_m17_snapshot_dto.rs`（**15 条**：partial 路径的发散 vs full 路径的不发散 + 畸形快照拒绝 + 被拒后引擎原样不动 + JSON 落盘往返 + M-17 接线 + 契约默认实现）
> - ⚠️ **口径限定**：该 DTO 只覆盖 **M-12/M-17 段（引擎）**，**不含**外设（M-15）/ 中断控制器（M-16）/ 内存映射——落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 组装（台账 **D-36 / D-37**）
>
> **v0.11.2 收口轮判据**（分支合 main 前 3 路独立检视的闭环，commit `3893626`）：
> - 8 项**闭环变异自检全部变红**（口径：`cargo test --workspace --no-fail-fast`——默认 fail-fast 会截断红测条数）
> - 台账合计行 **172 / 143 / 29**（新增「收口轮新增」行 37 / 34 / 3）· 检视产物 `docs/reviews/REVIEWS.md`（§3.1）
>
> **v0.13 第 1 阶段判据**（WASM 导出层 + 前端接真实内核 + 真机复验）：
> - **接口形状冻结**：`docs/detailed-design/STAGE-1_Wasm_Bridge.md`——内核只认契约消息（`process_message(json) → json` 的出参是**事件数组**），传输帧（`sessionId`/`role`/`reqId`）归前端，**内核不解析**
> - `cargo test --workspace --no-fail-fast` **492 全绿**（契约 lib 12 + kernel lib 431 + 集成 38 + golden runner 11）
> - `tools/check-front.sh` **141 用例 / 25 suite**
> - `tools/wasm-smoke.mjs`——在 Node 里驱动**真** wasm 产物跑完整桥（Nop → LoadProject → Subscribe → StepN → ReadMemory → SetBreakpoint → Continue → 畸形输入）
> - `tools/browser-verify.mjs`——**真 Chrome** 复验，且**断言值**：`x1 == 0x0000002a`（demo 固件真的被执行了），不只是「内核已就绪」
> - **13 项反向变异自检全部变红**（第 1 阶段检视轮的闭环口径——见 `docs/reviews/REVIEWS.md` §3.4.7 的闭环对照表）
> - ⚠️ **真机断言必须断言值**：只断言「内核已就绪」抓不到 D-45 这一类（PC 前进、x1 不动）。且**要反向证明它有判别力**——把断言弱化成「非空」再改坏固件，会**全绿**（V-B 实测）
> - ⚠️ **规格里写着的约束，必须有一条能变红的用例**：V-A 的 6 条中危里 5 条属于此类——改实现不会变红，缺陷能潜伏很久
> - ⚠️ **台账新增** **D-47**（帧 `seq` 双序列空间）· **D-48**（`E_TRX-04` 同码多义）· **D-49**（`E_PROTO-*` 不在契约 `ErrorFamily` 族里）
>
> **v0.14 D-46 探针管道判据**（波形视图不再是空的）：
> - `cargo test --workspace --no-fail-fast` **512 全绿**（契约 lib 12 + kernel lib 436 + 集成 **53** + golden runner 11）
> - `crates/kernel/tests/d46_probe_pipeline.rs`（**15 条**：通道列表 / 真实边沿 / 边沿带**真实时间戳** / `LoadProject` 后管道仍在 / **观察点 hook 与探针 hook 共存** / **不经 `LoadProject` 也能产生边沿** / 两份 demo 固件防漂移 / **M-19 的环里真的有边沿** / **单事件边沿数有上限** / **M-18 的边沿存储有上限** / **`LoadProject` 后新总线上的 hook 装齐了**）
> - `front/tests/waveform.test.mjs`（**9 条**：真内核通道 id `GPIO0.OUT` 进得了视图状态 / 寄存器流不进波形视图 / 边沿**累加** / 强制重订 `sim.waveform`）
> - `tools/check-front.sh` **156 用例 / 29 suite**（v0.14.1 = 152 / 28）
> - `tools/wasm-smoke.mjs`——在 Node 里驱动**真** wasm 产物，**+5 项探针流断言**（通道列表 / 刚订阅时无边沿 / 真实边沿 / 边沿值 / 边沿时间戳 ≠ 0）
> - `tools/browser-verify.mjs`——**真 Chrome** 复验波形视图：**断言通道名 `GPIO0.OUT` + 边沿数**，不只是「非空」
> - ⚠️ **demo 固件真的驱动 GPIO**（0x14..0x1c：`lui x5, 0x40000` / `addi x6, x0, 1` / `sw x6, 4(x5)`）——否则管道通了也没有边沿可推，视图还是空的
> - ⚠️ **前端过滤判别式是「不是 `cpu.` 开头的寄存器流」，不是「是 `probe.` 前缀」**：真内核的通道 id 是 `"{外设 id}.{信号名}"`（`GPIO0.OUT`），写成只认 `probe.` 会把真通道**全部过滤掉**（第 1 阶段那版就是这么写的）
> - ⚠️ **总线 hook 是追加式的**：M-14 `add_hook`（新）+ `set_hook`（保留「替换全部」语义）。观察点 hook 与探针 hook 必须**共存**——单槽会互相顶掉
>
> **v0.14.1 D-46 检视轮判据**（发布源有上限 + M-19 不再是死代码 + 前端门禁不再有判别力缺口）：
> - **反向变异自检 22 条，20 条变红**（2 条「预期不红」= 已登记的 **D-52** 与「竞态偶发、判据是探针」）——脚本先 `cargo build --tests` 再跑测试，**编译错误导致的「红」不算判别力**
> - ⚠️ **两层上限各管一段**：M-18 `EDGE_STORE_CAP = 8192` / 通道（**内存**层，drop-oldest + 计数）+ 宿主 `MAX_WAVEFORM_EDGES_PER_EVENT = 4096` / 事件（**事件**层，保留最新 + 计数）
> - ⚠️ **「用例都在」≠「用得着」**：删掉 M-18 → M-19 的 `set_edge_sink` 握手，曾让 D-46 全部用例**照样全绿**（M-19 是**死代码**）。现在有用例直接断言 **M-19 的环里有边沿**
> - ⚠️ **修复可能掩盖别的用例的判别力**：V-A-7 的「运行期重试」曾把「`LoadProject` 不重装管道」的变异从 RED 变 GREEN。判别式必须是**顺序无关**的（断言「别人先装的 hook 还在」/「新总线上的 hook 数」）
> - ⚠️ **外设 `tick` 报错不许牵连别人**：一个外设报错，其余外设**照样 tick**（时钟同步不能被别人拖停），已收的写**一条不丢**
> - ⚠️ **前端门禁曾有两条判别力缺口**（V-B）：①「装载工程后探针流被强制重订」**根本没被守**（`loadProjectIntoKernel` 是私有函数，删掉重订 → 门禁 152/152 全绿）；② `app.js` **导入即启动**，它异步调的 `waveform.reset()` 能清掉已推入的通道（共享 M-03 单例）——⚠️ **当前套件并不飘**（断言是同步的，emit 与 assert 之间没有 `await`），但「断言必须同步」这个隐式不变量没写在任何地方 → 已加 `__RVSIM_NO_BOOTSTRAP__` 开关把它变成显式的；判据是**探针**（连跑 N 遍通道数恒为 1），不是「用例变红」
> - ⚠️ **判别式要顺序无关**：「探针 hook 后装」时观察点 hook 改回 `set_hook` **无害**；「运行期重试」会掩盖「`LoadProject` 不重装管道」——判别式必须落在「**新总线上的 hook 数**」上
> - ⚠️ **第 3 步失败不许牵连前两步**：`loadProjectIntoKernel` 的探针流重订失败**单独**兜住（`probeError` 返回，不抛）——否则「内核未就绪」的提示会**误导**（内核好得很，只是探针流订阅不上）
> - ⚠️ **探针流订阅没有「断线重连后重订」**（M-10 的寄存器流有）→ 台账 **D-54**（今天无活影响，两种传输都不自动重连；P1 做自动重连前必须补）
> - ⚠️ **browser-verify 单独看，对「累加 vs 覆盖」「真数据 vs 假数据」没有判别力**——它是**连线检查**，不是行为检查；那一层由**前端门禁**兜住。所以「browser-verify 通过」**不能**单独作为「波形视图行为正确」的证据
> - ⚠️ **mock 内核的探针流是「全量」语义，真内核是「增量」** → 台账 **D-55**（mock 未接前端，无活影响；但别拿 mock 当真内核的行为参考）

> **v0.15 D-07 判据**（唤醒时间驱动链路 · 四处断口全闭环）：
> - `cargo test --workspace --no-fail-fast` **544 全绿**（契约 lib 12 + kernel lib 437 + 集成 **84** + golden runner 11）
> - `crates/kernel/tests/d07_time_driven.rs` **31 条**端到端 —— 对着**四处断口**逐条钉判别式：① 没人装 ticker ② 没人排 `PeripheralTick` ③ `pending_writes` 只写不读 ④ 时间驱动主循环无人调用
> - **反向变异自检 25 条，全部按期望变红**（变异必须**能编译**，编译错误导致的「红」不算判别力；期望集合用 **AND 语义**逐条核对）
> - ⚠️ **台账写「剩最后一步」的时候要重新数断口**：D-07 原写「M-12 的 `dispatch` 真正调用它」——实测是**四处断口**，且那一处**根本走不到终点**
> - ⚠️ **「接缝存在」≠「接缝被接上」**：`PeripheralTicker` 从 v0.11 起就躺在 M-12 里，但生产代码**从没调用过** `set_peripheral_ticker`。判别式落在**被依赖方自己身上**（断言外设被 tick 过几次、写的值能不能从总线读回来）
> - ⚠️ **「指令驱动 vs 时间驱动」的裁定**：时间驱动是**骨架**（事件队列 + 量子边界），指令驱动是量子内的**填充物**（每量子 ≤1000 条），二者在**指令边界**交汇（事件边界 = 抢占点）。两条路径**共用同一个执行器**（`IssExecutor`）——没有第二套 ISS 逻辑会漂移
> - ⚠️ **量子是 1 µs 不是 1 ns**：`TIME_QUANTUM_PS` 原写 `1_000` 却注释「1 µs」（差 1000 倍）。现在 `TIME_QUANTUM_PS = QUANTUM_INSTR_QUOTA × INSTRUCTION_COST_PS` 闭合换算
> - ⚠️ **接缝不是契约端口**：契约层**当时**冻结（新增端口要等 `schema_version=2`，台账 **D-32**）→ `InstructionExecutor` / `PeripheralTicker` 都是 **M-12 内部接缝**，由 M-17 实现，**依赖方向单向**（M-12 不依赖 M-13）。M-12 LLD §9.1.1 已补该行（v0.17 起契约层已解冻，但这两者**仍然是内部接缝**，未升为契约端口）
> - ⚠️ **外设时钟只有一个推进点**：M-12 的 `tick_peripherals`（指令边界同步）+ `SchedEvent::PeripheralTick`（量子边界）走**同一个** ticker 接缝；M-14 的 `Bus::tick_peripherals` **不再**被 M-17 直接调用（否则是**两个真相**）
> ⚠️ **`continue_until_event` 的语义订正**：第 1 阶段它只排空队列、**不执行指令**（队列恒空 → 等价于空操作）；现在它真的跑代码（LLD §3.1 `runUntil` 的本意）
> - ✅ **D-57 已闭环**（v0.17 契约 v2 批）：契约端口文档已订正为「跑到**本轮指令预算耗尽**；量子边界是**抢占点**、**不是**停止条件」——**改文档不改实现**（改实现会**推翻** D-07 刚闭环的 31 条用例所钉的语义）。⚠️ **契约层冻结已解除**（`schema_version = 2`，台账 **D-32** 已闭环）。⚠️ **D-73**：当时的判据是**自指**的（用的就是实现自己的 `DEFAULT_INSTRUCTION_BUDGET` 常量，把常量改成量子配额它照样绿）→ 已由 `5b89c9a` 把判别式改落在**量子配额**上（「跑过**不止一个**量子」），**端口方法层 + 命令层各一条**
>
> **v0.15.1 D-07 检视轮判据**（三路独立检视 43 项发现 → 38 项闭环 / 5 项留账）：
> - `crates/kernel/tests/d07_time_driven.rs` **31 条**（检视轮从 12 条扩到 31 条）—— 新增「两条路径的外设推进序列**逐条相同**」「指令边界 / 事件路径**各自**投递外设的写」「该写**恰好**提交一次」「报错的外设**不丢**已收下的写」「下一个量子边界**严格在**当前时刻之后」「预算**硬上限**」「换内存图后登记表只含新外设」「多外设**按地址分派**」「快照恢复后外设时钟**继续走**」「停止原因**不跨轮残留**」
> - **反向变异自检第 2 轮 11 条 + 第 3 轮 11 条** —— ✅ **19 条按期望变红**；2 条确认**无守**（→ 台账 **D-60**）；1 条**脆弱性探针**（→ 台账 **D-62**）
> - ⚠️ **同一个回调的两条调用路径要用「限定时刻」把夹具分开**：夹具每次都吐同一份数据时，两条路径的判别式会**互相掩护**——删掉任一条，用例照样全绿
> - ⚠️ **「同一份数据不能有两个真相」要有不变量用例**：本轮 V-A 🟠-1（同一外设在同一虚拟时刻被 tick 两次）修完之后，「两条路径的推进序列**逐条相同**」才是一条能被变异打红的不变量
> - ⚠️ **反向变异自检的期望集合要用 AND 语义**：OR 语义只能证明「某条期望的用例红了」，**不能**证明「每条期望各自都守得住」；**完整红集**要一起记下来
> - ⚠️ **变异脚本的备份文件名必须唯一，且 `copyfile` 方向要写对**：本轮真踩了（两个源文件都叫 `mod.rs` + `copyfile` 方向写反）→ 变异**累积**、还原失败。**还原后必须逐字节校验**
> - ⚠️ **修一处「更精确」的判据，可能引入死循环**：`until_ps` 改预检后，外层主循环与内层停止条件互斥 → **永远空转**。改行为要顺着把**终止性**再走一遍
> - ⚠️ **恒真式断言要挑出来**：要钉的是「**换算只有一个真相**」（M-17 转调 M-12），不是把定义重说一遍
> - ⚠️ **「用例钉的是默认值」必须写在文件头**：凡与配置**无关**的不变量，一律写成**相对单条指令成本**的形式

> **v0.16 M+C 扩展判据**（RV32M 8 条 + RV32C 26 条落地 · 台账 D-43 闭环）：
> - `cargo test --workspace --no-fail-fast` **599 全绿**（契约 lib 12 + kernel lib **492**（原 437，+55）+ 集成 84 + golden runner 11）
> - `crates/kernel/src/m13_iss/tests_rv32m.rs` **18 条** + `tests_rv32c.rs` **37 条**（新增）；`tests_common.rs` 抽出共享脚手架供 `tests_rv32i` / `tests_rv32m` / `tests_rv32c` 三模块共用
> - **压缩编码常量全部取自 GNU as 并逐条核对**（不是手搓位模式）——否则「测试与实现同源」，一个坏解码器会和测试一起绿
> - `tools/golden-run.sh --keep-going` → **total=51 ok=51 mismatch=0**（`rv32um` 8 例 + `rv32uc/rvc` 双侧 PASS）；`tests/golden/corpus.txt` 9 例 `pending` → `active`
> - ⚠️ **压缩指令用「字段级展开」而不是展开成 32 位位模式**：位模式表达 C.LUI 的 20 位截断规则有歧义；字段级展开语义等价、少一轮编解码、立即数按自然宽度携带（**偏离 LLD 伪码，已在台账 + LLD §3.2 / §3.5 记一笔**）
> - ⚠️ **取指维持「一次读 4 字节」**：低 2 位只判**宽度**、不改**访存宽度**——否则总线访存次数翻倍、统计口径改变（LLD §3.1 已订正）
> - ⚠️ **不补 4 字节对齐检查**：C 扩展使 IALIGN=16，32 位指令可在 2 字节对齐地址执行（参考实现亦然）
> - ⚠️ **「保留编码 vs HINT」一律以 Spike 为准**：`c.lui` 零立即数 = 保留（陷阱）；`c.mv` / `c.add` rd=x0 = **合法空操作**（退休）；`c.swsp` rs2=x0 = **真写 0**
> - ⚠️ **CJ / CB 偏移拼出的就是带符号字节偏移**（CJ 12 位符号在 bit11 / CB 9 位符号在 bit8），**不是**还要再乘 2 的半字数；**C.ADDI16SP 同理**（低 4 位恒 0，别再 ×16）
> - ⚠️ **Q1/100 槽位同时装 CI 组与 CA 组**：靠 `inst[11:10] == 0b11` 区分；CA 组 funct2 在 `inst[6:5]`、CI 组 funct2 在 `inst[11:10]`；**CR 组 funct2 只有 1 位在 `inst[12]`**（寄存器域是完整的 `inst[11:7]`）
> - ⚠️ **C.SW 的 funct3 是 0b110**（0b101 是 C.FSW，属 F 扩展）
> - ⚠️ **C.LUI 的 20 位截断**：6 位字段 sext 后**截到 20 位 LUI 字段**再 `<<12`；字段 `0b100001` → `0xFFFE1000`（**参考实现专用探针实测**：退出码区分 `0xFFFE1000` / `0xFFFFE000`）。⚠️ 错法是把 20 位字段取在**高位**（`sext6 & 0xFFFFF000`）；而「`sext6 << 12`」在 32 位运算里**并不是**错法
> - ⚠️ **取指未对齐的 mcause 与实现的 `TrapCause` 变体对不上**（LLD 写 mcause=0，实现只有 `MisalignedAccess`）→ 台账 **D-63**（今天无活影响，M-16 接线时定）
>
> **v0.16.1 M+C 扩展检视轮判据**（三路独立检视 · 35 项发现 → 30 闭环 / 5 留账 · 归档 `docs/reviews/REVIEWS.md` §3.7）：
> - **反向变异自检 3 轮共 45 条 → 44 条按期望变红**（第 3 轮 11 条针对检视发现的缺口，**11/11 全红**）；1 条 MISS 是 C.LUI 的**语义空操作**（两种写法在 RV32 下逐位相同），**不是**缺口
> - **3 处真实判别力缺口已补用例**（单测与 golden 语料**双双守不住**）：CB 偏移的符号位与幅度位（16 个偏移用例，判别式落在**落点地址**）· `c.slli` 的 5 位 rd · `c.swsp` 的 `uimm[7:6]` 与 5 位 rs2
> - **1 处假判别式已修**：`the_m_extension_cannot_read_x0` 的 `rs1` 写的是 **x3 不是 x0**（注释与编码不符）→ 改为 rs1 = x0 并**污染 `state.gpr[0]`**（否则「没人动过的 x0」与「绕过守卫读裸槽位」结果相同）
> - ⚠️ **「保留编码」≠「会陷阱」**：`c.ebreak` 是**合法指令**（`illegal` 为假）却触发 breakpoint 陷阱——断言必须分开写
> - ⚠️ **变异还原后必须 `touch` 源文件**：`shutil.copy2` 保留 mtime → cargo 指纹未失效 → 下一条变异跑在**陈旧二进制**上（本轮真踩到过）
> - ⚠️ **数字口径一律从当前树重算**，不做「上一步数字 + 增量」推算（V-C 查到的四份文档数字失真，根因就是它）
> - **新登记 5 条台账**：**D-63**（取指未对齐 mcause）· **D-64**（mtval 全平台不存在）· **D-65**（golden 两侧栈窗口**完全不重叠**，预存）· **D-66**（三处死接口/死字段）· **D-67**（D-07 用例对 C 扩展保留规则的隐式耦合）
>
> **v0.17 契约 v2 判据**（契约层**第一次**升版 · `schema_version = 2` · 冻结规格 `docs/detailed-design/STAGE-2_Contract_V2.md`）：
> - `cargo test --workspace --no-fail-fast` **636 全绿**（契约 lib **36** + kernel lib **505** + 集成 **84** + golden runner 11）
> - `tools/check-front.sh` **208 用例 / 35 suite**（v0.16 = 156 / 29）
> - `tools/golden-run.sh --keep-going` → `total=51 ok=51 mismatch=0`（**D-43** 闭环；本批未动 M-13 / M-06，语料未变）
> - `tools/wasm-smoke.mjs` **34 项断言**（需先 `tools/build-wasm.sh`；**不进默认门禁**）
> - **契约层自带 36 条**用例（`cargo test -p riscv_sim_contract`）——v0.11 时只有 12 条
> - ⚠️ **虚拟时间走十进制字符串**：线上 `elapsed_ps` / `t_ps` / `virtual_time_ps` / `at_ps` 是**字符串**；其余 `u64` 仍是 JSON 数字。JS 侧读回时**超过 2⁵³ 必须抛错**（`E_PROTO-04`，**不是** `E_PROTO-02`——那个的原义是「未知字段 → 仅日志」，拿它抛错语义是反的）
> - ⚠️ **`ReadRegisters { count: 0 }` 回复空区间**（`values: []`，`index` 是钳位后的实际起点），**不是「不回复」**——M-05 按 `reqId` 配对，「没有回复」与「消息丢失」不可区分 → 会变成 500 ms 假超时 `E_CTX-06`
> - ⚠️ **两个 topic 命名空间不要混**：**订阅 topic**（`sim.registers` / `sim.waveform`）vs **事件 topic**（`tick` / `halted` / `memory.read` / `waveform.history`）；⚠️ `Registers` 的事件 topic **故意**取 `sim.registers`（与订阅 topic 同名，是有意的重合）
> - ⚠️ **命名边界**：**history 形**（`schema_version + streams[]`）→ `model::WaveformHistory` + `EventPayload::WaveformHistory` + topic `waveform.history`；**live 批次形**（`seq_start/seq_end/t_virt/entries`）**保留原名** `model::WaveformDelta`（**不是**死代码，是 M-11 / **D-50** 的落点）
> - ⚠️ **`crc` 属前端传输帧**（`front/lib/frame.js`），**不在**契约信封上；CRC-32 覆盖 payload 的**规范 JSON**（递归排序键）；**缺失 → 放行**（旧生产者）/ **算错 → 拒绝该帧**（`E_PROTO-06`）
> - ⚠️ **错误族 14 个**（`CTX/ISS/ENG/BUS/PER/INT/DBG/OBS/SNAP/LDR/HMS/STORE/TRX/PROTO`）——`PROTO`（协议层）与 `TRX`（传输层）**并存不合并**；族标签由 `error::tests::every_family_tag_is_pinned` **表驱动钉死**（台账 **D-69**）
> - ⚠️ **D-57 已闭环**：契约端口文档订正为「跑到**本轮指令预算耗尽**；量子边界是**抢占点**、**不是**停止条件」——**改文档不改实现**；**契约层冻结已解除**。⚠️ **D-73**：当时的判据是**自指**的 → 已把判别式改落在**量子配额**上（「跑过**不止一个**量子」），端口方法层 + 命令层各一条
> - **新登记 8 条台账**：**D-68**（指令预算措辞）· **D-69**（`E_PP` → `E_PER`）· **D-70**（LLD 错误码表只收录 **6 族 / 29 个码**，代码实际发 **11 族 / 36 个码**）· **D-71**（M-10 LLD 的 `<style>` 永不闭合 → 第 260 行到 EOF 被吞成 CSS）· **D-72**（`StepOutcome::HostPause` 死接口）· **D-73**（D-57 判据自指，**已闭环**）· **D-74**（M-16 模块头的「量子边界」措辞）· **D-75**（内核 `schema_version` **严格等号** vs 前端 `decodeMessage` **前向兼容**——语义不对称，**非本批引入**）
> - ⚠️ **`SIM_STATE_SCHEMA_VERSION` 必须跟着升版**（1 → **2**）：`virtual_time_ps` 改走字符串后，对 v1 读者**不兼容**。契约的规则是「改了字段集**或线上表示**，**两个**版本号一起 bump」——只 bump 外层会让两个版本号**含义分叉**，将来 v3 把内层升到 2 时会拒掉 **v2 自己产的快照**
> - ⚠️ **版本号 / 线上表示必须有**字面量**断言**：`SNAPSHOT_SCHEMA_VERSION` 与 `SimState.virtual_time_ps` 的字符串表示此前**无用例钉住**（全部**符号化**引用常量 / 只断言**键集**不断言**值形态**）——删掉 `#[serde(with = "u64_decimal")]` 全工作区照样全绿。现在是 `the_snapshot_schema_version_is_two` / `the_sim_state_schema_version_is_two` 两条**字面量**断言 + `numstr::field_tests` 的**一条**用例钉住**全部 10 个字段位点**
> - ⚠️ **错误族清单是**单一来源**（`error_families!` 宏展开 enum + `as_str()` + `ALL_FAMILIES`）：新增族漏登记 → **编译错误**。⚠️ 族标签长度是「**3–5** 字符」（`PROTO` 与 `STORE` 都是 5 字符）——**不是**「3–4」
> - ⚠️ **前端门禁不许静默跳过**：`front/tests/contract.test.mjs` 的六条 Rust↔JS 对拍用例带 `{ skip: !hasRust }` 时，`node --test` 把 skipped 当「没失败」、**退出码仍是 0** → 跨层对拍变成**什么都不查却报绿**的静默空转。已改**硬断言**（移走 `crates/` → **6 条红、退出码 1**）。⚠️ 口径与 `tools/wasm-smoke.mjs` 的 **127** 一致：**「没跑」绝不能与「跑过了」不可区分**
> - ⚠️ **断言不得自指**：判别式必须落在**与实现无关**的量上。若断言用的常量就是实现自己用的那个常量，「改行为」与「没改行为」**无法分辨**（**D-73** 就是这么潜伏的）
>
> **v0.18 会话级快照载荷判据**（台账 §2.2 优先级表 **5b** · D-36 / D-37 / D-41 闭环 · 冻结规格 `docs/detailed-design/STAGE-3_Session_Snapshot.md`）：
> - `cargo test --workspace --no-fail-fast` **767 全绿**（契约 lib **69** + kernel lib **596** + 集成 **91** + golden runner **11**）
> - **本批的核心命题**：**落盘一个会话的完整状态，还原出的是一个「等价会话」**——不只是「还原了一个引擎」。六段载荷 = `SessionSnapshotDto { schema_version, cp_meta, m12_engine, m13_state, m14_map, m15_periph, m16_intc }`（**段 0~5**，`schema_version` 不编号）
> - **三处裁决**（规格 §1.2）：**A** LLD 结构清单漏列 M-12/M-17 段 → 补 `m12Engine` · **B** CPU 寄存器**已经在** `m12Engine.state` 里 → `m13State` **只**承载内存页（**寄存器不重复存**，否则是两个真相）· **C** 补第 6 段 `m16Intc`（⚠️ 该段在**生产路径**上恒为空——**CSR 管道未接线** + **M-15 的 pending IRQ 没有生产路径升上来**，两条成因**缺一不可**）
> - ⚠️ **恢复链的第 0.5 步**（宿主在 `load_map` / `load_pages` **之前**的**只读**外设预检）：**应用顺序一字未改**（仍是规格 §3.7 的五步），只多了一个**不写任何东西**的步骤。**没有它**，「外设状态畸形」会留下一具**缝合怪**（内存来自载荷、寄存器来自恢复前）——而调用方只拿到一个 `Err`
> - ⚠️ **「先校验后写入」有两层闸门，缺一不可**：契约层 `validate()` 查**载荷的自洽**（跨类型 id 唯一 · `gpio.irq_lines` 恰好 `GPIO_PIN_COUNT` 条 · 区域 `size != 0` · `base + size` 不越 u32）；**外设层** `Peripheral::validate_session_state()` 查「**这个外设能不能接受这份状态**」（如 UART 的 STATUS 位与 FIFO 是否自洽）——**契约层查不了**，只有外设认识自己状态的含义
> - ⚠️ **一个约束一个落点**：`GPIO_PIN_COUNT` 由**契约层**导出（载荷的**形状**依赖它），M-15 的 `PIN_COUNT` **引用**它——同一个数写两遍 = 一处一动，载荷**静默停止校验**
> - ⚠️ **ROM 内容不进快照**（固件是**输入**不是状态）→ 恢复链**没有「重装固件」这一步** → `load_map` 重建 ROM 时**原地保留内容**（`base`/`size` 不符报 `E_BUS-12`，**不搬地址、不静默丢弃**）；**外设区域原地保留**（外设实例是**输入**，删了恢复后总线无外设）
> - ⚠️ **M-20 的 `restore_session` 改名 `validate_session`**（**只校验、不还原**）——宿主 `restore_session`（真恢复）保持原名：**同名不同义 = 「一码多义」的变体**；有源码级守卫 `the_manager_has_no_restore_session_method` 防回退
> - ⚠️ **会话快照没有协议入口**（`SaveSession` / `LoadSession` 要 bump `SCHEMA_VERSION`）——本批只到**宿主内部 API** 可达
> - ⚠️ **断点表 / 观察点表 / 探针环 / 波形环不在载荷里** → 「还原出的会话等价吗」**取决于还原进哪个宿主**（同宿主保留、跨宿主丢失）
> - ⚠️ **判别力必须落在「漂移覆盖面」上**：段 1 的断言要有牙齿，漂移必须**换掉队列内容 / 推进插入序 / 改掉指令预算**（`step_n` 是指令驱动 → 队列**恒空**、插入序**恒为 0**、预算**恒为默认值** → 「不还原」与「还原了」**不可分辨**）；段 2 的断言要有牙齿，漂移必须**写 RAM**（否则牙齿来自 `load_map` 的**清零**——**代偿**）
> - ⚠️ **变异自检 7/7 + 17/17 全按期望**（内核侧 K1~K7 · 契约层 M1~M17）；**对照变异**（语义不变）必须 **0 红**；**还原必须逐字节校验**（md5）——本轮父会话真踩了「两个 `mod.rs` 备份名撞车 → 内容互相覆盖」的坑，是 md5 断言当场抓住的
> - **新登记 3 条台账**：**D-79**（变异工具链的还原不逐字节校验 / 快照不是最终树）· **D-80**（端到端夹具不含 ROM / 定时器——**故意为之**，加进去只会造**恒真断言**）· **D-81**（契约层「内存页越界」与「不落 RAM 区内」报**同一个码**，按码判别不出）
> - → 检视归档见 `docs/reviews/REVIEWS.md` **§3.9** · 冻结规格见 `docs/detailed-design/STAGE-3_Session_Snapshot.md`

> **v0.19 M-16 中断控制器落地判据**（台账 §2.2 优先级表 **5c** · D-26 / D-59 / D-63 / D-64 / D-70 / D-72 / D-74 闭环 · 冻结规格 `docs/detailed-design/STAGE-4_Interrupt_Controller.md`）：
> - `cargo test --workspace --no-fail-fast` **885 全绿**（契约 lib **94** + kernel lib **683** + 集成 **97** + golden runner **11**）
> - **本批的核心命题**：**中断交付整条链接通** —— `request(line)` → 挂起 → 量子边界采样 → 仲裁 → 登记交付 → M-13 取指边界拉取 → `enter_trap` → handler → `irq_clear` 清线 → `mret` 回到 `mepc`
> - ⚠️ **判据是「这条指令写了 `irq_clear`」不是「值变了」**（v0.19 检视轮 **F-1** 的 P0）：guest 每次 handler 都写**同一个**掩码 ⇒ 「值变了」的判据把**第二次清除静默丢弃** ⇒ `active_irq` 永不复位 ⇒ 那条线被单级防护**永久堵死**。⇒ M-13 **回报**「刚写了哪个 CSR」（`Iss::take_last_csr_written`），M-17 用回报做判据
> - ⚠️ **「回报」判据在六条 CSR 指令臂上各写了一遍 ⇒ 六条臂都得回报**（v0.19 检视轮 **F-13**）：`csrrw` / `csrrs` / `csrrc` / `csrrwi` / `csrrsi` / `csrrci` 的「写 + 回报」片段**逐字相同**，而**变异锚点只命中首个匹配** ⇒ 曾经只有 `csrrw` 一条臂有反向证明（端到端固件写 `csrrw`、M-17 手打固件写 `csrrwi`，**两份固件互补覆盖了两条臂**），其余**四条臂裸奔**。⇒ 补 M-13 单测 `every_csr_instruction_reports_the_csr_number_it_wrote`（**逐臂**表驱动，六条臂**各写两次**、第二次是**同一个值** —— 只写一次抓不住「值变了才回报」的退化）+ `a_repeated_write_of_the_same_value_still_reports_the_csr`（F-1 的**现场**）+ `an_ignored_write_reports_nothing`（只读 CSR 的写被忽略 ⇒ **不回报**，否则 M-17 会把「写了 `mip`」当成「写了 `irq_clear`」⇒ **清错线**）⇒ 变异 **K-38~K-42** 逐臂反向证明
> - ⚠️ **变异自检里「HANG」是独立的一档，而且有「三种形态」**：**确凿活锁**（cargo 打印了 `test … has been running for over 60 seconds` ⇒ **点名了**挂死的那个测试）· **超时未跑完（未证实挂死）**（某个 target 在超时窗口内没出结果 ⇒ **可能只是慢**，**不构成活锁证据** ⇒ 需**加长超时重跑**才能定性）· **假 HANG**（**所有 target 都已跑完**、卡在**退出**阶段 ⇒ macOS **XProtect** 扫描**每次变异都重新链接**的 doc-test 二进制时偶发堵塞 ⇒ 既不证明活锁也不证明判别力，是**「没结论」**，必须**重跑**）。⇒ 判据（只用输出，不看时间）：数落盘输出里 `Running <target>` 与 `test result:` 的个数 —— **相等** ⇒ 假 HANG；**不等** ⇒ 超时未跑完 ⇒ 再看有没有 cargo 的**挂死提示**那行 ⇒ 有 ⇒ 确凿活锁。⇒ 实证：**同一条变异 K-19**（`last_poll_ps` 每批次重置）在第一轮判 **RED ✓**、在重跑轮判 **HANG** ⇒ **同一变异两个结果** ⇒ 旧口径的 HANG 判定**整体不可信**。⇒ 修法：`cargo test --workspace --no-fail-fast --lib --tests` ⇒ **跳过 doc-test**（doc-test 只跑文档示例，**不覆盖**本批任何一条变异 ⇒ **不损失判别力**，只消除这一类假阳性）
> - ⚠️ **判别力的「最坏形态」不是「没覆盖」，是「把缺陷钉成期望」**：F-1 那两条用例**只数「handler 跑了几次」** ⇒ 缺陷路径上**照样成立** ⇒ **反向证明**：把清除真正做掉反而让它们**变红**。⇒ 断言必须推进到「**第二次交付之后**的控制器状态」
> - ⚠️ **两个位号空间**：`line`（1/2/3）与 `mie` 位（11 / 7）是**两套**编号，唯一互译点是契约层 `mie_bit_for_line`；`mie` 存的是**位图**（使能线 1 要写 `1 << 11`）；`irq_clear` 的掩码**按线号索引**（bit L = 线 L，清线 1 写 `0b10`）
> - ⚠️ **契约层不升 `schema_version`**：`mtval` 落 CSR `0x343`（**既有的通用 CSR 文件** `SimState.csrs`）⇒ 不加 `SimState.mtval` 字段 ⇒ 前端 JS 镜像不用改；会话段 5 新增字段全 `#[serde(default)]` ⇒ 旧载荷仍可加载且**语义正确**
> - ⚠️ **接缝落在 M-17 的执行器里，M-12 `Engine` 零改动**（裁决 I）：「M-12 的 `InterruptPoller` 接缝」会要求 M-12 持有一个闭包，而那个闭包要反过来借用 M-17 自己的 `Iss` ⇒ **自引用借用**（Rust 不允许）
> - ⚠️ **三处裁决的落点**（规格 §1.2）：**A** `mtval` 落通用 CSR 文件（不升版）· **B** `TrapCause` 拆三变体（**D-63**）· **C** `pause()` = **下一条指令边界**停（`take_pause` **取走即消失**；初稿「批次开始时清零」会把请求**直接丢掉** ⇒ 规格已订正）
> - ⚠️ **三个死变体已删**（`StepOutcome` 只剩 `Advanced` / `Trapped`）⇒ M-17 的 `match` **穷举**全变体 ⇒ M-13 以后再加变体会**编译不过**
> - ⚠️ **夹具是汇编器产出的字节**（`crates/kernel/tests/fixtures/intc_fw.S` 进仓库 + `objdump` 核对 + 一条「关键指令编码」守卫用例）—— 仓库里**没有** RISC-V 汇编器 ⇒ 字节与源之间**没有自动一致性检查**
> - ⚠️ **黄金语料对本批新代码零覆盖**（`rv32mi` / `rv32si` 要 trap delivery 与 `mret`）⇒ 验收「51/51 不变」是**恒真判据** ⇒ 台账 **D-85**（P1）
> - ⚠️ **反向变异自检 49 条**（`REVIEWS.md` §3.10.4 的**完整红绿表**，含每条的**实测轮次**）—— 变异集由 37 条经**两次逐条对账**扩到 **49** 条（把冻结规格 §6 的 **33 项验收**逐条映射到变异条目：补 **K-33~K-37** 验收缺口 · **K-38~K-42** CSR 六条臂逐臂 · **K-43 / K-44** 二次对账）⇒ **六条 CSR 臂各有一条「退化成『值变了才回报』」的变异** ⇒ 判据落在**被依赖方自己身上**；**4 条对照**必须 **0 红**；⚠️ **「HANG」有三种形态**（**确凿活锁** / **超时未跑完但未证实** / **假 HANG**）⇒ **只有 cargo 自己点名的那一行**（`has been running for over 60 seconds`）才算活锁证据 ⇒ 旧口径的 HANG 判定**整体不可信**（**同一变异两轮两个结果**）⇒ **K-17 的「挂死」被证伪** ⇒ 「**F-1 的后果是平台活锁**」这个结论**是错的**（准确表述：**那条线被永久堵死**；**会不会挂死取决于哪条路径**——**K-10** 实测**确凿活锁**）
> - ⚠️⚠️ **「假 HANG」的根因查实了：macOS 对新链接二进制的 exec 扫描** —— 实测**每个**新二进制首次 `exec` 被阻塞 **≈25 秒**（12 个二进制从 3.2 MB 到 8.9 MB **都是 23~26 秒** ⇒ **与体积无关**，**每文件固定成本**）⇒ 一轮 12 个二进制 = **300 秒** ⇒ **正好打满**旧的 300 s 窗口 ⇒ **系统性**产出假 HANG。⇒ **最终判据（三形态，只看输出）**：末个 target 的二进制**自己打印了** `running N tests` **没有** ⇒ **假 HANG**（exec 被系统堵住，**与被测代码无关**）；**有** ⇒ 二进制**启动过** ⇒ cargo **点名**了挂死的测试 ⇒ **确凿活锁**；**没点** ⇒ **超时未跑完**（**可能只是慢**）⇒ **决定性反证**：末个 target 若是 `riscv_sim_server` / `riscv_golden`（**0 个测试**）⇒ 它**不可能**挂死 ⇒ 本轮 8 条「HANG」里 **7 条是假的**。⇒ **三招缓解**：**收敛 target 集**（44 个期望红测逐条 `grep` 核实：41 个在 lib、3 个在 E2E、**零个**在别处 ⇒ 每轮只重链 3 个 ⇒ 300 s → **75 s**）· **预热**（先 exec 一次 `--list` ⇒ 25 s → **0.02 s**）· 运行超时**放宽到 900 s**。⚠️ **两条走不通的**（别再试）：`xattr -d com.apple.provenance` **无效**（**受保护的系统属性**）· `strip` **无效**（与体积无关）
> - ⚠️ **变异自检的工具链自己也是被检视对象**（本轮修掉**七处**缺陷：红集比对逻辑 / 无超时 / 备份键撞名 / **超时口径把「编译」与「运行」挤在同一个窗口里**（编译慢被误判成活锁）/ **doc-test 造成的假 HANG** / **HANG 判据不够细**（改成看「二进制启动没有」）/ **规模侧**（收敛 target 集 + 预热））⇒ 尤其「**0 红**」这种结论，**先怀疑工具，再怀疑被测代码**
> - ⚠️ **变异跑期间绝不改任何会被编译的源文件**（本轮踩中：把 `every_csr…` 从「各写一次」改成「**各写两次**」的那一下，正好落在变异驱动运行中 ⇒ 那一轮的 K-18 / K-18b 编译到的是**旧版测试集** ⇒ 结论**混源**、**不成立**）⇒ 判据：**一张可审计的红绿表里，每一行都必须出自同一版测试集** ⇒ 表里带「**实测轮次**」列；受污染的条目**重跑**（不重跑整批）
> - ⚠️ **「用命令行文本做判据」要防自匹配**：收尾时用 `ps | grep "…cargo test…"` 找残留 cargo ⇒ **匹配到了我自己的 shell**（命令行里就含这几个字）⇒ `kill -9` 把自己连带杀了 ⇒ 刚起的续跑乱掉 ⇒ **一度起了两个并发 cargo**。⇒ 判据：**按进程名（`comm`）匹配，不要按整行 `args` 匹配**
> - **新登记 3 条台账**：**D-83**（测试 / 变异工具链无超时保护 ⇒ 活锁表现为**挂死**而不是失败；**工具链侧本轮共修七处**（硬超时 + 杀整个进程组 / 红集 AND 语义 / 备份键撞名 / **编译与运行超时分离** / **跳过 doc-test** / **HANG 判据改看「二进制启动没有」** / **规模侧：收敛 target 集 + 预热**）⇒ **8 条「HANG」里 7 条是假的** ⇒ **只有 K-10 是真活锁**；**M-12 生产侧留账**，留给 **D-30** 那一轮）· **D-84**（`mip` 无生产者 ⇒ guest 读不到挂起中断；规格已订正）· **D-85**（黄金语料对 CSR / `mret` / 陷阱投递零覆盖）
> - → 检视归档见 `docs/reviews/REVIEWS.md` **§3.10** · 冻结规格见 `docs/detailed-design/STAGE-4_Interrupt_Controller.md`

> **v0.20 M-11 符号表 / DWARF / 反汇编 / trace 导出落地判据**（台账 **D-50 / D-51 闭环** · 新登记 **D-86 ~ D-90** · 冻结规格 `docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`）：
> - **工作区 1012 全绿**（契约 lib **98** + kernel lib **801** + 集成 **102** + golden runner **11**）——`cargo test --workspace --no-fail-fast --lib --tests`
> - **M-11 落地**：**3 206 行 / 75 条用例**（`render` / `symbols` / `disasm` / `dwarf` / `trace` 五个子模块 + `SymbolService` 端口）；**反汇编复用 M-13 的 `Decoder` + `classify`**（裁决 A）⇒ **零第二套解码表** ⇒ LLD §3.4 的「独立表 + 一致性门禁」**作废**（门禁只能*发现*漂移，复用让漂移**结构上不可能**）
> - **宿主接线**（Lane D）：4 条调试面命令 → 4 个事件（**占位分支已删**，不留「有形状、没内容」的变体）；**导出走宿主路由**（`Waveform` ⇒ M-19 的环 / `Execution` ⇒ M-11 的轨迹环）⇒ **D-50 / D-51 双闭环**；**D-50 的判别式是排除法**（抽干 M-18 的边沿存储后导出仍有内容 ⇒ 只可能来自 M-19 的环）
> - ⚠️ **反汇编缓存的失效通道 = M-18 的访存写事件**（裁决 G）⇒ **只失效被写地址**（不是整表清空）；⚠️ **失效与记录开关必须解耦**（本轮修掉的真缺陷：耦合 ⇒ 关闭 trace 期间访客自改代码 ⇒ 缓存**陈旧** ⇒ 用户看到**旧的**反汇编）——缓存失效是**正确性**，记录开关是**性能**；⚠️ **失效通道必须永远无损** ⇒ **`StepN` 按指令预算（10 000）分块**（否则访存环溢出会丢掉写事件 ⇒ 缓存不失效）
> - ⚠️ **访存轨迹的时间轴必须真的动**：M-14 的 `BusAccess::ts_ps` 来自 `Bus::set_now_ps`，而它**只**由 M-17 在指令边界调用 ⇒ 没有这一步，时间戳**恒为 0**（本轮修掉的真缺陷）
> - ⚠️ **规格 §9.4 的「`disasmCache` 可以兜住（重复取指）」必须可观测** ⇒ `Disassembler::lookup(addr)` + `lookup_hits()` 计数（否则那句话只是一句散文）
> - ⚠️ **门禁脚本里不许抄常量**（版本号 / 表头 / 字段名）——本轮修掉的真缺陷：`tools/wasm-smoke.mjs` 硬编码 `2` ⇒ 契约升 v3 后**一直红着**；⇒ 三处全部改成**从契约层源码推导**；冒烟门禁**补上本批 4 条命令的 13 项断言**（在**真 wasm 产物**上）
> - ⚠️ **反向变异自检 20 条 ⇒ 20/20 按期望**（19 条判定**全部按期望变红** + 1 条对照 **0 红**）——首轮只 **18/20** ⇒ 两条「没按期望」**都不是实现的问题**：**V-03** 的变异形态写成 `ExportFilter::with_time_range(1, 0)`（**builder 不是关联构造器 ⇒ 编译不过** ⇒ 变异**无效**）⇒ 改成 `ExportFilter::all().with_time_range(1, 0)` ⇒ RED ✓；**V-14 变异存活** ⇒ **真判别力缺口**：夹具 `SELF_LOOP` 的机器码**手算错**（注释说 `beq x0, x0, -4`，实解 `beq x0, x0, -2` ⇒ 跳到未对齐未映射地址 ⇒ 访客**第 2 条指令就陷入** ⇒ `StepN(250_000)` **只跑 2 条** ⇒ 「访存环不许溢出」**恒成立** ⇒ **整批空转**）⇒ 改成 `beq x0, x0, 0`（`0x0000_0063`；⚠️「跳回自己」的偏移是 **0**）⇒ 实测 `elapsed_ps = 250 000 000` ⇒ RED ✓ ⇒ **「断言恒成立」是假绿的典型形态** ⇒ **夹具也要有自己的判别式**
> - ⚠️ **「接上了」与「钉住了」是两件事**（本轮主线教训）：`Waveform+Vcd` 导出改成 CSV、波形导出的 `overflow_marked` 改成 `false` ⇒ **795 + 5 条用例全绿** ⇒ 判别力的落点必须在**被依赖方自己身上**（把映射抽成**可单独钉住的自由函数** + 喂**人造输入**）
> - ⚠️ **Wave 1 的八处已知边界**（规格 §9.8，诚实标注）：访存轨迹**不含访存值**（P1）· 无时间/地址**筛选器**（P1）· **DWARF 只识别不解析**（Wave 2）· `limit` 只对 `Execution` 生效 · `Waveform` 的 `offset` 是回显请求值 · 轨迹机器码可能**时间倒流**（D-87）· 裁决 G 的**部分写粒度**（D-89）· 前端导出**无下载动作**（D-88）
> - → 检视归档见 `docs/reviews/REVIEWS.md` **§3.11** · 冻结规格见 `docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`

> **v0.21 黄金对拍的 CSR / 陷阱覆盖判据**（台账 **D-84 / D-85 / D-90 闭环** · 新登记 **D-91 ~ D-101** · 冻结规格 `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md`）：
> - **工作区 1 033 全绿**（契约 lib **104** + kernel lib **813** + 集成 **102** + golden runner **14**）——`cargo test --workspace --no-fail-fast --lib --tests`（基线 `b014fed` = 98 + 801 + 11 + 102 = **1 012** ⇒ 本批 **+21** 条）
> - **黄金语料 64/64**（`rv32mi` 新增 13 例 `active`；`total=64 ok=64 now-passing=0 expected-gap=0 mismatch=0 no-verdict=0 build-failed=0`）+ `--self-check` 三个变异对照两侧一致
> - **D-84（`mip` 生产者）**：M-16 的**挂起队列** → 契约层 `pending_lines_to_mip_mask` → **M-17 写侧**落进 CSR 文件（两个刷新点 + **值变化才写**）⇒ 全仓**只有一处** `CSR_MIP` 写入点
> - **D-85（CSR 语料）**：新增 **CSR-capable env 变体** `tests/golden/env/csr/` + `tests/golden/env/encoding.h`；语料清单增「env 变体」列 ⇒ 语料 **51 → 64**
> - **D-90（第二套 ELF 符号读取器）**：`riscv-golden` 的 `find_symbol` **已删** ⇒ 符号查找走 M-06 → M-11
> - ⚠️ **「判决一致」≠「覆盖等价」**（本批主线教训）：`rv32mi/csr` / `ma_fetch` / `instret_overflow` 三例两侧判决相同，但**走的是不同分支**（或在我们侧**恒真**）⇒ 已在 `tests/golden/README.md` 逐条标注（**D-96**）⇒ **不得**把它们当特权级 / 计数器覆盖的证据
> - ⚠️ **规格里的「实测基线」也可能是错的**：原文写「参考侧 `misa` 带 IALIGN=32、我们不填」——**实测两侧 IALIGN 字段都是 0**；真正的差异是**特权级两位**（参考侧 `0x40141104` 有 S/U，我们 `0x40001104` 没有）⇒ 冻结规格的**基线表**要按实测改写，不能按推演写
> - ⚠️ **探针要测在正确的上下文里**：`mstatus.MPP` 的第一条探针测在**陷阱之外**且**没有写** ⇒ 读到的 0 说明不了任何事（`MPP` 是 **WARL** 字段，**陷入时**才被写成 M）⇒ 改到 handler 里测，真相立现（参考侧 `0x0018_0080` / 我们 `0x0000_0080`）⇒ **D-97 已闭环**
> - ⚠️ **env 常量必须逐字等于上游**：harness 只编译**一份**镜像给两侧 ⇒ 常量漂移**不会**表现为分歧，只会**悄悄换掉被测程序** ⇒ `tools/golden-run.sh` 现在**逐条比对** `tests/golden/env/encoding.h` 与上游 `env/encoding.h`，不一致即**硬报错**（第一版就抓到 `PMP_A` `0x0c` vs `0x18`）
> - ⚠️ **「跑不了」与「跑歪了」必须分开**：harness 的 pending 容忍规则**只**容忍 `NOVERDICT` / 装载类 `ERROR` ⇒ guest **fault**（`ERROR step@…`）**不容忍**（参考侧那里是陷入，行为分歧）
> - → 检视归档见 `docs/reviews/REVIEWS.md` **§3.12** · 冻结规格见 `docs/detailed-design/STAGE-6_CSR_Golden_Coverage.md`
> **v0.22 Wave 2a 判据**（台账 **D-93 / D-98 / D-99 / D-100 / D-101 / D-30 / D-83 闭环** · 新登记 **D-102 / D-103 / D-113 ~ D-116** · 冻结规格 `docs/detailed-design/STAGE-7_Wave2_DWARF_Lines.md`）：
> - **工作区 1 083 全绿**（契约 lib **106** + kernel lib **861** + 集成 **102** + golden runner **14**）——`cargo test --workspace --no-fail-fast --lib --tests`（基线 `b014fed` = 98 + 801 + 11 + 102 = **1 012** ⇒ v0.21 = **1 033**（+21）⇒ 本批 **+50**）
> - **前端门禁 245 用例 / 41 suite**（基线 228 / 39 ⇒ **+17 / +2**）· **黄金语料 67/67**（`total=67 ok=67 now-passing=0 expected-gap=0 mismatch=0 no-verdict=0 build-failed=0`；语料 64 → **67**：`unmapped_store` / `unmapped_load` / `compressed_trap`）
> - **D-99（未映射访存 → 架构陷阱）**：load = **cause 5** / store = **cause 7**，`mtval` = **故障地址**；判据落在 **family + scenario**（`is_unmapped_access`）**不靠 message 文本**；⚠️ **根因**：`BusMemory` 原先把所有总线错误重包成 `ISS-01` ⇒ 「未映射」**无法**与其它总线错误区分 ⇒ 改为**保留 family / scenario**
> - **D-93（`mepc` 三路径）**：**用户写**掩 `~1`（**IALIGN=16**，平台 `misa` 声明支持 C ⇒ 不是 32）· **陷入门**写**精确 PC**、**不丢 bit 1**（`csr_write_raw`）· **`mret` 取 PC** 再掩 `~1`（`mepc_aligned`）⇒ 三者**幂等**
> - **D-98（`mstatus` WARL）**：掩码常量 `MSTATUS_WARL_MASK = 0x807F_FFEA` 住**契约层**、M-13 **只引用不复制字面量**；⚠️ **刻意不照抄**参考侧实测的 `0xAA797E80`（照抄会让**保留位读出 1**，**违背规则本身**）⇒ 逐位残留**如实留账**
> - **D-100（`pending_lines`）**：`Option<Vec<u16>>`（`None` = 状态未知）⇒ `project_mip` 遇 `None` **跳过写**；判别式是**真的造出争用**（`borrow_mut` 跨过 handler 的 `irq_clear` 写）时 `mip` **不被写**
> - **D-101（两处宽松解析）**：① `restore_full` / `restore` 都走 CSR 表过滤 ⇒ 恢复含**未实现 CSR** 的快照 ⇒ guest **读回 0**；② **`sh_entsize` 的校验落 M-06**（M-11 只拿到 `.symtab` **原始字节**、**看不到节区头**）⇒ 撒谎的 symtab **整个镜像**被 `E_LDR-01` 拒收；⚠️ `sh_entsize == 0` 视作「**未声明**」放行
> - **Lane A（DWARF 真解析）**：`.debug_line` 的 **v2 / v3 / v4 / v5** 行号程序 + 行号状态机 + 特殊操作码 + 按单元降级 + 多单元归并；⚠️ **两态不得混**：「查过行表、没命中」= `Missed`，「没查」= `NoDwarf` ⇒ **绝不**产出猜出来的行号
> - ⚠️ **契约层本批不升版、不加命令、不加 DTO 字段**（`schema_version` 仍 **3**、`front/lib/message.js` 未动 ⇒ **线格式零变化**）；新增的只有**两个掩码常量**（`MSTATUS_WARL_MASK` / `MEPC_ALIGN_MASK`，**不在线格式里**）⇒ 「反汇编面板逐条行号」登记 **D-102**、等契约 v4 一起做
> - ⚠️ **本批主线教训**：「**用例绿**」≠「**它测的是生产路径**」——v0.20 批说「**接上了**」和「**钉住了**」是两件事，本批推进一步：**「钉住了」也可能是钉在了一个测试自己造出来的状态上**。四路独立检视（V-A / V-B / V-C / V-D）查出一条**真的断掉的生产路径**：M-10 的 `viewIds` 默认值不含 `source` ⇒ 源码视图永远拿不到 `sim.pc` ⇒ 「解析当前 PC」按钮**恒问地址 0**；而 A-16 用例**手工**塞了 `sim.pc` ⇒ **门禁 243/243 全绿**。⇒ **判别式**：这条用例能不能**在生产接线被拆掉时变红**？不能 ⇒ 它没在测生产路径
> - ⚠️ **WARL 字段要逐路径测**：统筹方只测了「陷入门」就断言「写侧不掩码」，补测「用户写」才发现参考侧**是**掩的；⇒ **实测**两个字不保证它对（v0.21 批已栽过一次），且断言值要写**架构字面量**（不从掩码常量推导，否则用例会成**恒绿**）
> - **三条修复 lane（W1 · W2 · W3）已合入，每条都配了反向变异**（红集逐条见 `REVIEWS.md` **§3.13.5** 三个小节）：**W1**（M-11 三条中危）5 条全红 + 1 对照 0 红 · **W2**（M-12 三条中危）6 条全红 + **2 条按预期挂死（HANG）** + 1 对照 0 红 · **W3**（真机复验）2 条全红 ⇒ **STAGE-7 的 31 条验收全部勾选**
> - ⚠️ **「加了守卫」≠「守卫接上了」**：W2 的无进展计数器**自增了却从不被比较**（统筹方在工作树上实测发现）⇒ 守卫**空转** ⇒ 活锁**照样挂死** ⇒ **判别式**：查一个守卫接没接上，要**查它的比较点**，不能只查它的变量
>
> **v0.23 osz 落地批判据**（台账 **D-47 / D-48 / D-116 闭环** · 新登记 **D-117 ~ D-121** · 冻结规格 `docs/detailed-design/STAGE-8_OSZ_Runtime.md`，裁决 A~N + 修订 **C′**）：
> - **三档验收全部达成**：① **CLI 闭环**（osz 跑到 `OSZ$ ` 提示符，105 万条指令 / 1.05 s 虚拟时间 / 31 字节 console）· ② **浏览器可视化**（真 Chrome 里终端视图出现 `==== Enter Main ====` 与 ANSI 转义 `\x1b[2K`）· ③ **与 Spike 逐字节对拍**（`tools/boot-osz.sh` ⇒ **`VERDICT MATCH`**，两侧 31 字节一模一样）
> - **工作区 1 159 全绿**（契约 lib **115** + kernel lib **925** + 集成 **102** + `osz-boot` runner **3** + golden runner **14**）· **前端门禁 268 用例 / 47 suite**（基线 245 / 41）· **黄金语料 67/67** · **8 项门禁失败项数 0**
> - **三处平台真 bug**（此前没有 guest 需要它们，所以一直没暴露）：① `mcause` 报的是**内部线号**（`IRQ_BASE | line`）而架构规定机器中断是 **3 / 7 / 11** ⇒ 改成契约层的「线号 → 架构 cause 码」纯函数，⚠️ **`mie`/`mip` 的位号映射一个字都没改**；② **`wfi` 是非法指令** ⇒ 实现成合法的 nop（⚠️ 诚实代价：guest 的 idle 任务退化成**忙等**）；③ **`mhartid` 未实现** ⇒ 读回 0 是「未实现 ⇒ 读 0」的**运气** ⇒ 正式实现为 0，并配了一条**有判别力**的用例（先塞非零值再断言读出来的仍是平台常量）
> - **M-15**：UART 重做成 **ns16550**（8 个字节寄存器 + `LCR.DLAB` 真切换 + `LSR` 按位义重排 + **无损 TX 捕获环**）+ 新建 **CLINT / PLIC**。⚠️ **`LSR` bit5 = THRE**（发送器还能再收一个字节）**不是**「环是空的」—— osz 的 `hal_uart_putc` 是自旋，取错语义会让 osz 每发一字节就卡死一次
> - **宿主接线**：内存图分**两档**（`Default` 与今天一模一样；`QemuVirt` 铺 `0x8000_0000` 起 **128 MiB**（8 × 16 MiB，受 M-14 每区域上限约束）+ UART0 @ `0x1000_0000` + CLINT @ `0x0200_0000` + PLIC @ `0x0C00_0000`），⚠️ **档位按镜像用到的地址自动挑**（装载顺序改成「先解析 → 再挑档 → 再建图 → 最后写镜像」）；**三条外设中断通路**（CLINT `mtimecmp` 周期↔M-16 ps · UART RX→PLIC 源 10 · PLIC 待交付源→M-16 外部线；⚠️ **claim 是 guest 的权利**、平台只读不代 claim）；**console 通路**（UART TX 捕获环 → 64 KiB backlog → 订阅 `sim.console` 推契约 v4 的 `Console` 事件）
> - ⚠️ **为什么 128 MiB 一字节都不能少**：osz 的 best-fit 分配器**会在堆尾写节点**（`0x8800_0000` 附近）⇒ 少映射一个字节就是「未映射访问」陷阱。⚠️ **为什么不一档铺满**：那 128 MiB 是**真的物理内存**（`map_ram` 用 `vec` 分配），每个宿主实例都吃一份 ⇒ 仓里上千条用例 ⇒ 默认铺满会让测试进程吃掉几个 GiB ⇒ 规格**当批修订**成裁决 **C′**（而不是偷偷改实现）
> - **契约层升 v4**（本批唯一一次升版，一次带齐五项）：`SCHEMA_VERSION` 3 → 4；新增 **Console 事件**（`bytes` 是**原始字节数组**）；**D-102** 的 `file`/`line` 字段；**D-116** 的三条引擎族场景码；**D-47 / D-48** 的传输层欠账。⚠️ 新变体进了 **Rust ↔ JS 的逐字对照表**
> - ⚠️ **本批主线教训**：「**加了守卫**」≠「**守卫接上了**」在**接线**场景下又应验了一次 —— 宿主新增的 `collect_console` 与 `drain_console` **互相调用形成无限递归**，release 构建当场**栈溢出**，而那 7 条接线用例**当时全绿**（它们写在这段代码**之前**）⇒ **接线用例只能证明「写它那一刻」的形状**，新加的路径必须自己再被用例覆盖
> - ⚠️ **另一条**：**别让约定藏在 prose 里** —— 裁决 C 原本写「改**默认**内存图」，落地时发现「默认铺 128 MiB」会让测试跑不动 ⇒ 规格**当批修订**（裁决 **C′**）
> - ⚠️ **外部产物不进仓库**：osz 的源码 / ELF / Spike 输出都在**仓外**（`$OSZ_ELF`，默认 `/tmp/osz-artifacts/osz.elf`）⇒ `tools/boot-osz.sh` **不进默认门禁**；仓内只进**脚本与规则**
> - → 检视归档见 `docs/reviews/REVIEWS.md` **§3.13** · 冻结规格见 `docs/detailed-design/STAGE-7_Wave2_DWARF_Lines.md`

> **v0.24 遗留清账批判据**（台账 **D-102 / D-117 / D-118 / D-119 / D-120 / D-121 闭环** · 新登记 **D-122** · 冻结规格 `docs/detailed-design/STAGE-9_V024_Leftovers.md`，裁决 A~I）：
> - **一句话目标**：**让 osz 的 shell 真的能用**（能打字 **E-8** · 能看到源码行 **E-6** · 能断在调度器 **E-7**）⇒ STAGE-8 的 **37 条验收至此全部勾选**（v0.23 时是 34 勾 3 未勾）
> - **工作区 1 238 全绿**（契约 lib **134** + kernel lib **974** + 集成 **113** + `osz-boot` runner **3** + golden runner **14**）· **前端门禁 288 用例 / 50 suite**（基线 268 / 47）· **黄金语料 67/67** · **8 项门禁失败项数 0**（含本批专属的 `tools/boot-osz.sh` ⇒ **`VERDICT MATCH`**，两侧 31 字节逐字节相同）
> - **契约层升 v5**（本批唯一一次升版）：`SCHEMA_VERSION` 4 → 5，新增命令 **`WriteConsole { bytes: Vec<u8> }`**（线上形状 `{"WriteConsole":{"bytes":[104,105]}}` —— **字节数组**，不是字符串、不是 base64）。⚠️ **不复用 `WriteMemory`** —— 写 `0x1000_0000` 在技术上就是「写内存」，但它**绕过外设的语义层**（不走接收 FIFO、不置 `RX_VALID`、不抬中断线）⇒ guest 读 `RHR` 拿到**旧值**而 `LSR` 说「没有数据」⇒ **自相矛盾**
> - ⚠️ **两侧逐字对照由契约层的一条用例守着**（`include_str!` 读 `front/lib/message.js` 的源码断言 `WriteConsole` 拼写与版本号两侧一致）⇒ 镜像**不会漂移**
> - **会话快照升 v2**（破坏性变更**显式**升版，旧载荷被 `E_SNAP-05` **显式拒绝**）：`PeripheralSessionState` 加 `Clint` / `Plic` 两个变体；`UartStateDto` 加 **LCR / IER / FCR / MCR / SCR / DLL / DLM** 七个 ns16550 寄存器 + `rx_queue`（装**整个**接收队列，`rx` 保留为队头字节，不变式 `rx == rx_queue[0]`）
> - ⚠️ **`LCR.DLAB` 位本身也要还原** —— 否则还原后「除数锁存器模式」的状态就丢了；⚠️ **`LSR` 仍按既有口径重算**（还原原始数字会让寄存器跟自己的 FIFO 打架）
> - **D-122（本批新登记）· UART 的接收 FIFO 原本只有 1 字节深** ⇒ 一次送三个字节**只有第一个进 FIFO**，其余被丢并锁存 `LSR` 的溢出位 ⇒ 「能打字」只做到「能打一个字符」。⇒ 接收侧加深成 **`RX_FIFO_CAPACITY = 16`** 的队列（`VecDeque<u8>`；满了**丢最新**并锁存溢出位 ⇒ 与 TX 捕获环**丢最新保最早**同口径 ⇒ 丢失**看得见**不是静默的；⚠️ **TX 侧一个字未改**）
> - ⚠️ **规格里的「复数」就是一条容量约束**：裁决 B 写的是「把**字节**塞进 UART 的 **RX FIFO**」⇒ 规格**假定** FIFO 装得下 ⇒ 实现只做到规格的**字面最小值** ⇒ **规格本身也可能是错的**
> - **反汇编真带源码行**（D-121 / D-102 的内核侧）：`disasm_with_lines(addr, count, read, resolve)` ⇒ `DisasmEntryDto` 填真 `file` + 真 `line`（查不到就是 `None`，**绝不编造**、**不拿别的地址的行号顶替**）；旧 `disasm` 签名**一字未改**；**可观测**计数器 `line_resolutions()` ⇒「每条指令只查一次行表」是**可证的**不是散文；⚠️ 「查过没命中」与「压根没查」在 DTO 上**仍不可区分**（契约只有两个可空字段 —— **形状**决定的），分这两态的那一路是 `SourceLocation` 事件
> - **前端终端能打字**（D-119）：输入框 + 发送按钮 ⇒ 输入按 UTF-8 编码成字节发 `WriteConsole` ⇒ **清空输入框**；⚠️ **回显不是平台做的**（guest 自己回显 ⇒ **端到端**证据）；⚠️ **重渲染会毁掉输入框**（`body.innerHTML` 重建 ⇒ 监听器会丢、**打了一半的字会被清空**）⇒ 草稿存 **M-03 视图状态**、监听器绑**容器**做事件委托 ⇒ 各配一条用例钉住「重渲染不丢草稿」「重渲染不丢监听器」
> - **真机三条判别式（真 Chrome，全部断言具体值）**：**E-6** `file = /tmp/osz-fresh/kernel/base/task/src/task.c`、`line = 41`（断言 `file` 含 `.c`、`line` > 0）· **E-7** 按**符号名**从 osz 的 `.symtab` 取调度器地址（新增**零依赖**的 `tools/elf-symtab.mjs`；候选名 `first_schedule` / `schedule` / `os_preemp_reschedule` / `os_preemp_schedule_with_pm`，取第一个存在的）⇒ 实测 `first_schedule = 0x80002550`，命中时 `Halted { reason: Breakpoint, pc: 0x80002550 }` · **E-8** 送一个字节后读 UART 的 `LSR`（`0x1000_0005`）= **`0x63`**、bit0 置位
> - ⚠️ **E-7 的断点必须在「跑之前」下**：调度器在 shell 提示符出现**之前**就执行过了 ⇒ 先跑到提示符再下断点，那个符号**永远命中不了**
> - ⚠️ **门禁抓到一条真缺陷**：`tools/boot-osz.sh` 的**构建**认 `CARGO_TARGET_DIR`、**运行**却写死 `$REPO_ROOT/target/release/osz-boot` ⇒ 设了 `CARGO_TARGET_DIR`（**项目纪律要求每 lane 独立 target 目录**）时跑的是**上一批**的旧二进制 ⇒ 判决错得莫名其妙 ⇒ 一条约束一个落点：构建目录只有一个真相，并加「二进制不存在就**当场报错**」的守卫
> - ⚠️ **本批主线教训**：**「规格写了什么」也要对着实现核一遍** —— 规格的措辞里**复数**就是容量约束，别只看它写了哪个词
> - → 冻结规格见 `docs/detailed-design/STAGE-9_V024_Leftovers.md`（22 条验收**全部勾选**）· 检视归档见 `docs/reviews/REVIEWS.md` **§3.14**

> **v0.25 取指路径对齐 + 前端补洞批判据**（台账 **D-86 / D-103 / D-33 / D-88 闭环** · 新登记 **0** · 冻结规格 `docs/detailed-design/STAGE-10_V025_Defect_Clearance.md`，裁决 A~G）：
> - **一句话目标**：**面板显示的指令访客执行得了**（D-86）· **取指未映射走架构陷阱 cause 1 而不是宿主报错**（D-103）· **打开应用第一眼不是空面板**（D-33）· **导出能下载而不是手动复制**（D-88）⇒ 一次只做**四件**事，另**明写九件不做**的事（D-13 与 TX 捕获环进快照跟 **D-123-A** 一起做 · D-115 依赖没做的 M-22）
> - **工作区 1 254 全绿**（契约 lib **134** + kernel lib **980** + 集成 **123** + `osz-boot` runner **3** + golden runner **14**）· **前端门禁 312 用例 / 57 suite**（基线 288 / 50）· **黄金语料 68/68**（基线 67 + 新增 `local/unmapped_fetch`）· **8 项门禁失败项数 0** + 本批专属 **S-1~S-3**
> - ⚠️ **裁决 A 把取指宽度落在 M-14 而不是 M-13** —— M-13 侧调两次 `read_fetch` ⇒ **一次取指变成两个事件**（总线读计数翻倍、探针轨迹多一条、hook 触发两次）⇒ 破掉「**一次取指 = 一个事件**」这条不变式与 D-46 已钉住的判别力；新入口 `Bus::read_fetch_instruction`：先读 2 字节判宽度、压缩指令**只读 2 字节**、**只 fire 一次 hook**、事件宽度 = **指令宽度**
> - ⚠️ **宽度探查是「纯 peek」**（走 `read_only`：**不计次、不 fire hook**）⇒ 落点另行补记 ⇒ **一次取指 = 一次读派发 + 一次落点 + 一个事件** ⇒ 总线统计与改动前**逐条相同**（既有断言 `reads == 1` / `ram_hits == 1` **一条没改**）；若按字面用 `read_inner` 探查，32 位取指的 `ram_hits` 会变成 2 ⇒ 要么改既有断言、要么让统计**静默**漂移
> - ⚠️ **裁决 B 依赖裁决 A 先落**：没有取指宽度对齐的话，映射区**末端**的压缩指令会被读 4 字节而失败 ⇒ **误报 cause 1** ⇒ **一条本该执行的指令变成了陷阱**
> - ⚠️ **`cause 1` 的判据落在 family + scenario**（沿用数据侧那条 `is_unmapped_access`）⇒ **外设未接线等其它总线错误原样上抛、不被误判**；⚠️ **变体↔码的映射只有一处**（`mcause()`）⇒ 有源码级守卫扫全内核源码树（cause `1` 的字面量映射**恰好 1 处**）
> - ⚠️ **黄金语料一个编号一条**：`local/unmapped_fetch`（自包含，guest 自查 `mcause` / `mtval` / `mepc` **三项**全对才 PASS ⇒ **判决在比较本身**）⇒ 与 `unmapped_load` / `unmapped_store` **互不冗余**（RISC-V 把取指 / 读 / 写编号成 **1 / 5 / 7**，用同一个码会让平台在**其中两条上**与参考侧一致、在**另一条上**分歧 ⇒ **一个编号一条语料**才让这个分裂可观察）
> - ⚠️ **口径订正（我自己的错）**：早期用手写探针测参考侧 `mepc` 得到的「疑似与平台分歧」结论**不可靠**（那个探针的 tohost 判决管道有缺陷）⇒ 改用**黄金 env 的可靠管线**复测 ⇒ **正变体退出码 0 / 反向对照退出码 3** ⇒ Spike 的 `mepc` **就是取指地址**、与本平台 `mepc := state.pc` **一致** ⇒ **不登记欠账**、语料**恢复三项断言**。⇒ **教训**：「参考侧不一致」这个结论**必须有可复现的判别式**（反向对照）才敢拿去做**架构决策**；一个**只会亮红灯的探针**测出的「不一致」，第一嫌疑是**探针自己**
> - ⚠️ **前端三个「不造第二个真相」**：**PC 与虚拟时间只来自既有状态源**（`embedded` 已进 M-10 的 `REGISTER_MEMORY_VIEW_IDS`）· **连接状态由 app 层 `publishLinkState` 从 M-07 推进**（视图层不碰服务层）· **跳转交 M-01 的 `route()`**（M-02 → M-01 是 LLD §9.1 明列的**单向依赖**；⚠️ **不**改 `location.hash` —— 它依赖 `hashchange`，DOM 替身不触发 ⇒ 门禁根本断言不到跳转）
> - ⚠️ **真机只判「正文非空」是不够的**（白屏也非空）⇒ 断言落到**具体内容**：PC 的十六进制值 + 四个入口的 `data-*` 标记 + 连接状态；变异「把真机断言改看 `#/regmem`」⇒ 真机 **2 红**，而「正文非空」那条**仍是绿的** ⇒ **真正变红的是「实质内容」那条**
> - ⚠️ **`<a download>` 不能常驻在面板里**：裁决 D 同时要求「`download` 用 `setAttribute` 设置」与「点击后 `revokeObjectURL`」⇒ 常驻的 `blob:` href 要么**一直有效**（URL 不释放 ⇒ 泄漏）要么**早就失效**（渲染时就释放 ⇒ 点不动）⇒ 面板里常驻的是**按钮**，锚点**点击时当场造出**、点完在**下一个宏任务**里释放（同步释放会打断**尚未开始**的下载）⇒ **规格的 B-3 判别式已同步订正**（`863285f`）——**规格与实现冲突时改规格，但要写清为什么**
> - ⚠️ **一个静默通过的门禁缺口**：前端 DOM 替身**没有 `click()`** ⇒「下载动作没点任何东西」会**静默通过** ⇒ 补上 `click()`（真实 DOM 的 click 事件才是下载的触发点）
> - ⚠️ **三处历史错误的技术论断已订正**（`a435390`）：「`perl -e 'alarm N; exec(@ARGV)'` 因 exec 重置 alarm 而不超时」是**错的** —— 实测 **alarm 能跨 `exec` 存活**（rc=142 / 3 s）；那次「十分钟不被杀」的**真因**是 `alarm N` 的 `N` 是**字面量 bareword** ⇒ `alarm("N")` 不是数字 ⇒ **闹钟根本没设上** ⇒ **秒数要当变量传**（`my $t = shift; alarm $t;`）。推荐写法仍是「父进程**不 exec**、只 **fork**」，但理由改成「能按**进程组** `SIGKILL`（连 cargo spawn 的测试二进制一起杀 ⇒ 不留孤儿）、且有独立的 **124 = HANG** 退出码」
> - → 冻结规格见 `docs/detailed-design/STAGE-10_V025_Defect_Clearance.md`（12 条验收**全部勾选**）· 叙事见 `docs/progress/PROGRESS.md` **v0.25.0** 段

> **v0.26 内存图通用化 A 批判据**（台账 **D-13 闭环** · **D-123 部分闭环**（A 批落地、B 批留给 `STAGE-12`）· **D-10 语义修订**（护栏仍在）· 新登记 **0** · 冻结规格 `docs/detailed-design/STAGE-11_V026_Memory_Map_A.md`，裁决 A~G）：
> - **一句话目标**：**登记一段地址空间不再等于「立刻申请那么多物理内存」** ⇒ RAM 改 4 KiB 页级懒分配 · 护栏**换被测量的量**（**不删**）· 快照**只装写过的页**（载荷 **258.06 MiB → < 1 MiB**）· TX 捕获环进快照 · D-13 字段可见性收紧 ⇒ 一次只做**四件**事，另**明写十件不做**的事（B 批 / D-94 / D-87 / D-89 / D-114 / D-115 / D-82 / M-21·M-22·M-23 / ROM 懒分配 / 前端）
> - **工作区 1 292 全绿**（契约 lib **140** + kernel lib **1 001** + 集成 **134** + `osz-boot` runner **3** + golden runner **14**；基线 1 254 ⇒ **+38**）· **前端门禁 312 用例 / 57 suite**（未变）· **黄金语料 68/68**（未变）· **8 项门禁失败项数 0** + `tools/boot-osz.sh` **VERDICT MATCH**（两侧 console **31 字节逐字节相同**）+ 本批专属 **S-1~S-3**
> - ⚠️ **护栏是「换被测量的量」不是「删」**：懒分配之后「声明多大」**不再等于**「要占多少内存」⇒ 继续按声明大小卡就是**禁错了对象**（一个 128 MiB 的 region 登记成本是 0）⇒ 护栏改成「**单个 region 相对宿主预算的上限**」，**被测量的量按 region 种类定**：**RAM 按物理占用**（已物化页数 × 4096）· **ROM 按声明大小**（== 物理占用）· **外设窗口按声明大小**（它不持内存，占的是地址区间）⇒ **没有任何一路的保护被弄丢**（`oversized_rom_region_is_rejected_before_allocating` 与 `the_cap_also_guards_the_peripheral_window` **一条没删**）⇒ `E_BUS_09_TOO_LARGE` **保留原号、不新增码**（避免「一码多义」）⇒ 判据仍落在 `family == Bus && scenario == 9`、**不靠 message 文本**
> - ⚠️ **两条分界必须守住**：① **「region 登记」与「物理页」是两层**——读**没登记**的地址仍走**架构陷阱 cause 5 / 7**（D-99 的判别力：m13_iss 的 `an_unmapped_load_traps_with_cause_five` 与 `an_unmapped_store_traps_with_cause_seven` 两条**仍然全绿**）· 读**登记了但没写过**的页回 **0** ⇒ 合成一层 ⇒ guest 读一块「从没写过的 RAM」会拿到架构陷阱而不是 0 ② **「声明」与「占用」不是一回事**——声明 128 MiB **免费**（零物化页），但**真占**超预算 ⇒ `E_BUS-09` 且**不留下任何已物化的页**
> - ⚠️ **快照页为什么是 4 KiB 而不是 64 KiB**：4 KiB **恰好是物化粒度** ⇒ 「导出没跳过任何东西」是**结构上**成立的；64 KiB 会把**一个**物化页摊成 **16 条**导出条目、其中 15 条是零 ⇒ 与今天同形状的病 ⇒ 旧形状下 QemuVirt 档一次会话快照 = **258.06 MiB** JSON（RAM 段 129.00 MiB，**几乎全是零**）⇒ 新形状下**空会话的 `mem_pages` 是空的**、整份 JSON **< 1 MiB** ⇒ 破坏性变更：`SESSION_SNAPSHOT_SCHEMA_VERSION` **2 → 3**、旧载荷走 **E_SNAP-05 显式拒收**（**不默默换形状**）
> - ⚠️ **TX 捕获环进快照 = 与 `rx_queue` 完全同构**：捕获环里的字节是**访客已经发出去、还没被宿主取走**的内容 ⇒ 它**是会话状态** ⇒ 不进快照 ⇒ 一次快照/还原就**丢字节**（与 v0.24 的 `rx_queue` 同形状的问题，那次已修）⇒ 新增 `tx_ring`（整环、队头在前）+ `tx_dropped`（丢弃的累计数 ⇒ **丢失看得见**），`tx` 保留为**队头**的冗余表示 ⇒ 不变式 `tx == tx_ring[0]` 与 `rx == rx_queue[0]` **同形**
> - ⚠️ **三条规格订正**（施工期实测发现，本批最值钱的部分）：① **护栏漏了「外设窗口」那一路** ⇒ 照原文实现的话，那条既有保护会**静默消失** ② **页对齐用「绝对 4096 对齐」** ⇒ 导出的一页是 **region 相对**的一页 ⇒ 绝对对齐会**把平台自己导出的东西判成非法** ⇒ 快照往返直接断掉 ⇒ 改成「落在**该 region 的页网格**上」 ③ **「一次访问永不跨页」的算式不成立**（`off == 4095` 时 4 字节写会跨页）⇒ 正确的事实是「一次访问**最多跨两页**」⇒ 实现按「可能跨两页」写 ⇒ **教训**：「宽度 ≤ 4 字节」**推不出**「不跨页」，还要看**起点**在页内的位置；规格里的算式必须**逐个反例验一遍**
> - ⚠️ **`new_with_profile` 换档失败会静默退回默认档**（不 panic，只留一个 `probe_wiring_failures` 计数）⇒ 任何「QemuVirt 档怎样怎样」的断言**必须先断言布局**（区域总数 6 + 存在 `base == 0x8000_0000 && size == 128 MiB` 的 RAM 区域）再断言别的 ⇒ 否则**降级也会绿**、那条用例就是**白测**
> - ⚠️ **QemuVirt 从 8 个 16 MiB 区域变 1 个 128 MiB 区域** ⇒ guest 看到的地址空间**不变**（0x8000_0000 起连续 128 MiB；参考侧 Spike 的 `-m` 参数本来就是 `134217728` 字节）⇒ 区域数 **13 → 6**
> - → 冻结规格见 `docs/detailed-design/STAGE-11_V026_Memory_Map_A.md`（21 条验收**全部勾选**）· 叙事见 `docs/progress/PROGRESS.md` **v0.26.0** 段

## Code style

- **Rust**：
  - Edition 2021，MSRV 1.75（见 workspace `rust-version`）
  - `cargo fmt --all` 默认配置（rustfmt 默认）
  - `cargo clippy --workspace --all-targets -- -D warnings`（workspace 已配置 `unsafe_code = "forbid"`、`unused_must_use = "deny"`）
  - 模块公开 API 必须是 `pub` 显式标出；不在 contract 暴露的内部状态用 `pub(crate)` 或 `_private` 字段
  - 所有跨模块通信走 `riscv_sim_contract`，不直接 `pub use` 对方内部类型
- **JavaScript**：
  - 原生 ES modules（`<script type="module">`），无打包器
  - 缩进 4 空格，单引号
  - 文件名用 kebab-case：`m01-workbench.js` / `m05-commandrouter.js`

## Testing instructions

- 单元测试：`cargo test --workspace`（含各模块 `#[cfg(test)] mod tests`）
- **前端门禁**：`tools/check-front.sh`（`node --test`，含最小 DOM stub；见 `front/tests/`）——**ESM 链接期错误只有这条门禁能抓住**，Rust 测试全都覆盖不到
- **前端改动后必须真机复验**：`tools/check-front.sh` 的 DOM stub 覆盖**状态与行为**，覆盖不了**挂载时序**——「首次事件派发早于订阅」这类缺陷对 stub 完全隐形（2026-10-20 的 #20：修完白屏后视图体仍全空，只有真实浏览器能暴露）。改前端挂载顺序 / 事件订阅关系后，必须起 `python3 -m http.server 8080 --directory front` 用浏览器实跑一遍
- **WASM 桥冒烟**：`tools/build-wasm.sh` 后跑 `node tools/wasm-smoke.mjs`——在 Node 里驱动**真** wasm 产物跑完整桥（产物不在时**跳过**并退出 0，不进默认门禁）
- WASM 端到端：`tools/build-wasm.sh` 后用 `python3 -m http.server 8080 --directory front` 起 HTTP 服务，浏览器访问 `http://localhost:8080/`（**不能 `file://` 直开**：ES module 与 Worker 均要求同源）
- **真机复验**：`tools/browser-verify.mjs`——真 Chrome + 真 WASM 内核，**断言值**（`x1 == 0x0000002a`）。**需要先 `tools/build-wasm.sh`**（产物缺失 → 退出 127 = 前置条件缺失，不是验证失败）
- 黄金对拍：`tools/golden-run.sh` 用 Spike 作为外部 oracle（参考侧 ISA 必须是 **`rv32imc_zicclsm_zifencei`**——`zicclsm` 决定 Spike 是否执行非对齐访问，stock `--isa=rv32imc` 是抛陷阱的，见台账 D-42）。两侧跑**同一个 ELF**，判决与失败用例号都要一致
- 测试位置约定：
  - 单元测试就近写在 `crates/<crate>/src/<module>/mod.rs` 内 `#[cfg(test)] mod tests`
  - 跨模块集成测试放 `tests/unit/`（顶层 `tests/` 目录，Rust 自动识别为集成测试）
  - Spike 对拍**脚本**放 `tools/`（`golden-run.sh`）；**语料、env、金丝雀、说明**放 `tests/golden/`；候选侧 runner 是 `crates/kernel/src/bin/riscv-golden.rs`（**harness 代码，不是 23 个模块之一**）

## PR & commit conventions

详见 [`CONTRIBUTING.md`](./CONTRIBUTING.md)（含 SSH 配置、conventional commits 完整规范、PR 流程、提交前自检清单、不允许做的事）。

要点速览：
- 默认分支：`main` — **只接受从检视闭环的分支同步过来**，不接受直接往 `main` 提交
- 分支命名：`<type>/<scope>`（如 `feature/m12-engine-quantum`、`fix/m13-iss-bug-123`）
- Commit message：**纯文本**（**不用 markdown**），固定五要素、**各占一行**：
  1. `类型：Feature` 或 `类型：Bug`（本次修改的类型）
  2. `说明：<本次修改的详细说明，一句话>`
  3. `测试用例：<新增/改动了哪些用例，覆盖什么>`
  4. `测试报告：<实际跑的命令与结果>`
  5. `遗留问题：<已知未解决项；没有就写「无」>`
  - ⛔ **不要**写 `feat:` / `fix:` 这类前缀、**不要**用 markdown 表格 / 加粗 / 列表 / 代码块
  - ⚠️ 用 `git commit -F <file>`（**不要** heredoc）
- 仓库 SSH：`git@github.com:chenyuxiangg/riscv-sim.git`（HTTPS 不支持）
- PR：CI 绿 + 至少 1 个 reviewer 批准；commit message 关联 issue 或 ADR 编号
- 架构变更（涉及 ADR、模块边界）：需更新 `docs/` 下对应设计文档后再合入

### 工作循环（Agent 必读）· 拉分支 → 开发 → 检视 → 同步 main

**后续所有模块开发都走这条流程。** 核心两句话：

- **改动一旦完成就要 commit**，不要把改动留在工作区。
- **绝不直接往 `main` 提交** —— 所有开发都在从 `main` 拉出的新分支上做，检视闭环后才同步回 `main`。

#### 1. 开分支（每个模块 / 每件事一个分支）

```bash
git checkout main
git pull --rebase origin main        # main 必须与 origin/main 一致
git checkout -b feature/m19-waveform  # 或 fix/... / docs/... / chore/...
```

`main` 上**不允许**有未同步的本地提交。新工作一律落在分支上。

#### 2. 开发 + commit（在分支上）

1. 改代码
2. 过三关门禁：`cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace`
3. `git add` **用显式路径**——不要用 `git add -A` / `git add .` / `git add <目录>`。本仓库可能有并行会话在同时改动同一工作区，目录级 add 会把别人的改动一并收进你的 commit
4. `git commit`：commit message 按**上面的五要素纯文本格式**写；**一个 commit 一个逻辑单元**，跨模块的多件事拆成多个 commit
5. 报告：向用户说明 commit hash、门禁结果、以及哪些改动**没有**提交及原因

#### 3. 检视（review 闭环）

- 检视产物：**`docs/reviews/REVIEWS.md`**（含实证复现 + 问题汇总 + 逐项闭环核对；模块级报告另见 `docs/reviews/M-XX_<Module>_Review.md`）
- 检视发现的缺陷**在同一个分支上**追加 fix commit 闭环（不重写已推送的历史）
- 检视未闭环完 → **不合并**，`main` 保持不动

#### 4. 同步回 main

- 检视闭环 + 门禁全绿后，才把分支同步到 `main`（squash merge 或 fast-forward + merge）
- 同步完成后删本地分支 + 删远端分支
- 同步动作需向用户确认后执行

#### 配套约束

- **`git push` 需用户明确指示**，不自行推送；`git push --force` 永远禁止
- 提交前若工作区出现**非本次会话产生**的改动，先确认归属，再决定是否纳入本次 commit
- 修 bug 时优先**追加一个新 commit**，不重写已推送的历史

## Security

- 不提交任何密钥、令牌、`.env`（已写入 `.gitignore`）
- WASM 沙箱（ADR §5.1）+ 跨层统一转义（NFR-04）：固件仅在 sandbox 内执行；前端字符串统一 escape，畸形字节按替换符处理
- GPL 合规（R2）：不复用 QEMU 源码；自研 BSD/MIT 内核（spike 风格借鉴非拷贝）
- 会话隔离（FR-13）：每个 session id 独立 snapshot 命名空间，admin 操作走专用 `AdminOp` 通道
