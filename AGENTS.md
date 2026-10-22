# AGENTS.md

RISC-V 仿真/调试平台 — 多模块协作的端到端工程，详细设计已落地在 `docs/detailed-design/`，高层级设计在 `docs/{architecture,requirements,module-design-spec}/`。

> 本文件给 AI Agent 读取，遵循 [agents.md](https://agents.md) 开放标准，可被 OpenCode / Codex / Cursor / Aider / Devin / Gemini CLI 等 Agent 共用。

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

## Setup commands

工具链：Rust stable（rustup） + `wasm32-unknown-unknown` target + `wasm-pack`（前端 wasm-pack；服务端原生 Cargo）。

- Install deps:    `rustup target add wasm32-unknown-unknown && cargo install wasm-pack`
- Build (native):  `cargo build`（开发）/ `cargo build --release`（发布）
- Build (WASM):    `tools/build-wasm.sh` 或 `tools/build-wasm.sh --release`
- Test:            `cargo test --workspace`（单元 + 集成 + 黄金对拍）
- Test (前端):    `tools/check-front.sh`（node:test 冒烟门禁，**208 用例 / 35 suite**；Node ≥ 22）
- Test (WASM 桥): `node tools/wasm-smoke.mjs`（需先 `tools/build-wasm.sh`；在 Node 里驱动真 wasm 产物跑完整桥，产物不在则跳过）
- Test (浏览器真机): `tools/browser-verify.mjs`（零依赖 CDP 脚本，需本机 Chrome + 已构建的 WASM 产物 + `python3 -m http.server 8080 --directory front`；**不进默认门禁**，按需运行）
- Test (黄金对拍): `tools/fetch-golden-deps.sh`（拉 riscv-tests + 交叉工具链到**仓库外**）→ `tools/golden-run.sh --self-check`（变异金丝雀）→ `tools/golden-run.sh --keep-going`（全量）
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
│   │                         #   numstr.rs（虚拟时间的十进制字符串 serde 辅助，v2 新增）
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
│   ├── browser-verify.mjs    # 浏览器真机复验（零依赖 CDP，按需运行，不进默认门禁）
│   ├── wasm-smoke.mjs        # WASM 桥冒烟（Node 驱动真 wasm 产物，按需运行）
│   ├── fetch-golden-deps.sh  # 拉 golden 外部依赖（riscv-tests + 交叉工具链）到仓库外
│   └── golden-run.sh         # Spike 对拍：同一 ELF 两侧跑，比对判决（含变异自检）
├── tests/
│   ├── unit/                 # 单元测试
│   └── golden/               # Spike 对拍 harness
│       ├── env/              # 零 CSR 最小 env（riscv_test.h + link.ld）——绕开陷阱投递
│       ├── canary/           # 变异金丝雀（两侧都必须报 FAIL 3）
│       ├── corpus.txt        # 51 例语料清单（march + status）
│       └── README.md         # 协议、语料边界、Zicclsm 口径
└── docs/                     # 39 篇设计文档（含 STAGE-1 / STAGE-2 两份规格）+ reviews/（1 入口 + 9 份模块报告）
```

## Project layout

### 23 个模块的代码位置映射

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | `front/views/m01_workbench.js` | ✅ v0.10 闭环（20/20 评审） |
| M-02 | 视图组件库 | `front/views/m02_viewcomponents.js` | ✅ v0.10 闭环；v0.11 加调试命令条 + 寄存器/内存视图；✅ **v0.20：反汇编面板**（PC 高亮 V-12 / 压缩指令原样显示 / 导出入口与「继续导出」按钮） |
| M-03 | 视图状态管理 | `front/views/m03_viewstore.js` | ✅ v0.10 闭环 |
| M-04 | 会话管理器 | `front/services/m04_session.js` | ✅ v0.11 实现（状态机 + 断线重连） |
| M-05 | 命令路由与事件分发 | `front/services/m05_commandrouter.js` | ✅ v0.11 实现（订阅/退订/一次性订阅）；v0.11.1 补 `once()` 一次性语义的观察与回归用例；✅ **v0.17 契约 v2**：topic 路由**表驱动**（唯一真相 = `front/lib/message.js` 的 `EVENT_TOPIC`），新增 3 个事件变体的路由（`Registers` → `sim.registers` / `Negotiated` → `sim.negotiated` / `SimStateDelta` → `sim.state.delta`）+ `WaveformHistory` 换 topic（`waveform.delta` → `waveform.history`）——**3 条新命令**（`ReadRegisters` / `WriteRegister` / `Negotiate`）的回复经同一条表驱动路径落位 |
| M-06 | ELF / Raw-HEX 解析器 | `crates/kernel/src/m06_elf/` | ✅ v0.11 加固 + **首次模块级 review**（D-03 阻断级消除）；v0.11.1 检视轮再修 IHEX 嗅探判据 + bss gap 溢出；✅ **v0.20：带出调试节区原始字节**（`LoadImage.debug_sections` + `ParseLimits.debug_section_bytes = 8 MiB`）——M-06 仍是**唯一**的 ELF 容器解析者，M-11 只解析 M-06 带出的节区字节 ⇒ 台账 🟡-4 的「第二套 ELF 解析器」**不成立** |
| M-07 | 传输适配器 | `front/services/m07_transport.js` | ✅ v0.11 实现（Worker + WebSocket 双适配器）；**v0.13 第 1 阶段起默认挂真 WASM 内核**（`front/kernel-worker.js`），mock 退化为测试替身；✅ **v0.17 契约 v2：传输帧新增 `crc` 字段**（`front/lib/frame.js` 实现 CRC-32 / IEEE 802.3 反射式，覆盖 payload 的**规范 JSON**（递归排序键）；8 位小写 hex；**缺失 → 放行**（旧生产者）/ **算错 → 拒绝该帧**（`E_PROTO-06`））——`crc` 属**前端传输帧**，**不在**契约信封上 |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | ✅ **v0.13 第 1 阶段实现**（53 行 stub → 457 行）：断点表（按 addr 索引 + id 反查）/ 观察点表（地址 + 方向 + 宽度匹配）/ `hit_check` 取指前命中 / 未对齐与重复注册拒 `E_CTX-07` |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | ✅ **v0.13 第 1 阶段实现**（71 行 stub → 308 行）：`StepKind` 四种粒度 + `Plan` + `StepOutcome`；`Over`/`Out`/`Line` **显式降级**为指令级（`degradation()` 把降级说出来，不悄悄降级） |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | ✅ v0.11 实现（订阅寄存器/内存快照）；v0.11.1 补断线重连后重订阅；**v0.13 补 `resubscribe()` / `loadProject()`**——装载工程后内核侧订阅会失效，必须**强制**重订（幂等的 `ensureSubscribed()` 不够，台账 **D-45**）；✅ **v0.17 契约 v2：接 `ReadRegisters` / `WriteRegister` 命令 + `Registers` 事件**（`readRegisters()` / `writeRegister()` + `applyRegisters()` 落缓存；⚠️ `count = 0` **回复空区间**、**不是不回复**——见冻结规格 §5） |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | ✅ **v0.20 落地**（53 行 stub → **3 161 行 / 75 条用例**）：五个子模块（`render.rs` 助记符渲染 / `symbols.rs` `.symtab` 解析 / `disasm.rs` 反汇编序列 + 按地址缓存 / `dwarf.rs` 行表降级态 / `trace.rs` 轨迹环 + CSV/JSON 导出）+ `SymbolService` 端口；**反汇编复用 M-13 的 `Decoder` + `classify`**（裁决 A ⇒ **零第二套解码表**——LLD §3.4 的「独立表 + 一致性门禁」作废，漂移**结构上不可能**）；**只出 ISA 助记符、压缩指令保留 `c.` 前缀、不做伪指令别名**（裁决 C，黄金参照 = `objdump -d -M no-aliases`）；**M-11 不依赖 M-19**（裁决 D ⇒ 导出由宿主路由）；⚠️ **DWARF 只识别不解析**（v2/v3/v4 识别、v5 报版本号；`resolve()` 在**所有**状态下都返回无 DWARF ⇒ **绝不**返回「未命中」——那等于宣称「查过行表、没命中」；Wave 2 才给行号）；⚠️ **访存轨迹不含访存值**（要改 M-18 的环布局，P1） |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | ✅ v0.11 加固（快照还原 now_ps+事件队列、推进不空转、seq 不撞号）；v0.11.2 增契约快照 DTO 转换层（`snapshot_dto` / `restore_dto`，**先校验后应用**）+ `Engine::reset()`；✅ **v0.15 D-07：唯一主循环 `run_until(executor, until_ps)`（LLD §3.1 结构）+ `InstructionExecutor` 内部接缝（M-12 不依赖 M-13）+ `tick_peripherals` 唯一外设时钟同步点 + `TIME_QUANTUM_PS` 订正为 1 µs**；⚠️ **至今无独立模块级 review**（台账 D-30） |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | ✅ v0.11 **RV32I 全集 41 条** + 模块级 review；v0.11.1 检视轮修 SYSTEM funct3 阻断级缺陷；v0.12 **RV32I 与 Spike 逐例对拍通过**（语料 51 例中 42 例判决一致；失败用例号一致性由变异金丝雀证明）；v0.13 取指改走 `Bus::read_fetch`（**取指不匹配观察点**，V-A 🟠-2）；✅ **v0.16 M+C 扩展落地**（**RV32M 8 条 + RV32C 26 条**）：`compressed.rs` 字段级展开 + `alu.rs` 乘除 8 函数 + `Decoded` 增 `compressed`/`compressed_raw`/`illegal`；**golden 语料 51/51 全通**（D-43 闭环）；✅ **v0.19 M-16 落地批**（**6 856 行 / 187 条用例**，v0.19 批重算）：新增 **CSR 指令族**（`csrrw` / `csrrs` / `csrrc` / `csrrwi` / `csrrsi` / `csrrci`）+ **`mret`** + **唯一**陷阱入口 `enter_trap`（`mepc` / `mcause` / `mtval` 写 CSR、`mstatus.MIE` 关中断、跳 `mtvec`）；**`TrapCause` 拆三变体**（取指 0 / 读 4 / 写 6 —— **D-63**）；`mtval` 落 CSR `0x343`（**D-64**）；⚠️ **`StepOutcome` 只剩 `Advanced` / `Trapped`**（`HostPause` / `Breakpoint` / `Watchpoint` 三个死变体已删 —— **D-59 / D-72**）；⚠️ **M-13 只管写 CSR + 回报「刚写了哪个 CSR」**（`Iss::take_last_csr_written`），平台副作用在 M-17 |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | ✅ v0.4 完成 + v0.7.2 补完；v0.11 接线 PR；v0.13 新增 `read_only()`（调试读不计入统计 / trace / 观察点）与 `load_bytes()`（镜像装载的**唯一**写路径），并给 `BusAccess` 增 `is_fetch` 标记（**取指 vs 数据访问**）；**D-46 增 `add_hook()`（追加式 hook，`set_hook` 保留「替换全部」语义）+ `peripheral_instances()` + `tick_peripherals()`**；✅ **v0.18 会话快照：新增 `export_map()` / `load_map()` + `export_pages()` / `load_pages()`**（`load_map` 重建 **RAM 清零** / **ROM 原地保留内容**（`base`/`size` 不符报 `E_BUS-12`） / **外设区域原地保留**；新增 `E_BUS-10/11/12`） |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | ✅ v0.7 三外设 + v0.11 接线 PR；✅ **v0.18 会话快照：新增 `session_state()` / `restore_session_state()` + 自由函数 `collect_session_state()` / `restore_session_states()` / `validate_session_states()`**（回灌**两阶段**：先全量**只读**校验，全过才写一个字节） |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | ✅ **v0.19 落地**（44 行 stub → **1 453 行**，31 条用例）：挂起队列（按 `(request_time_ps, insert_seq)` **保序**）/ **同线去重** / 优先级仲裁（最高优先，同级按插入序）/ 量子边界 `poll()`（刷新 `mie` 使能 + 定时器电平轮询 + `mstatus.MIE` 闸门 + `active_irq` 单级防护）/ **拉取式**交付（`take_intr` **取走即消失**）/ `export_state()` + `load_state()`（**整体替换**）/ 脉冲回调（**D-26** 的生产者）；⚠️ **`mie` 位映射的唯一互译点是契约层 `mie_bit_for_line`**（`line` 空间 1/2/3 ↔ `mie` 位 11/7）；⚠️ **`mip` 无生产者**（guest 读 `mip` 恒得 0 → 台账 **D-84**，P1） |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | ✅ **v0.13 第 1 阶段实现**（stub → **2 069 行**，v0.19 批重算）：`step_n` 真正驱动 M-13 ISS（断点取指前命中 / 陷阱 / 观察点总线 hook 命中）、`read_memory` / `write_memory` 接 M-14 总线、`set_breakpoint` / `set_watchpoint` 接 M-08 表；v0.11.2 的 `snapshot_full` / `restore_full` 接线保持；**D-46：观察点 hook 改 `add_hook`（不顶掉别人的）+ 每条指令前同步一次外设时钟（边沿才有真实时间戳）**；✅ **v0.19 M-16 落地批**：取指边界按「`now_ps - last_poll_ps >= TIME_QUANTUM_PS`」**采样** → `poll()` → **拉取式**取走交付 → `enter_trap`（**中断不停批次**）；**`irq_clear` CSR 写的副作用**（掩码 **bit L = 线 L**）落在 M-17；⚠️ **判据是「这条指令写了 `irq_clear`」**（M-13 回报），**不是**「值变了」——后者会把**第二次**清除**静默丢弃** ⇒ 那条线被**永久堵死**（v0.19 检视轮 **F-1**）；✅ **v0.15 D-07：`IssExecutor` 实现 M-12 的 `InstructionExecutor` 接缝（指令驱动与时间驱动**共用一条** ISS 路径）+ `install_peripheral_ticker`（外设 tick 接缝）+ `pending_writes` 真正提交到总线 + `continue_until_event` 走唯一主循环**；✅ **v0.20：指令边界把虚拟时间告诉总线**（`self.bus.set_now_ps(now_ps)`）——M-14 的 `BusAccess::ts_ps` 由此才有真实时间；⚠️ **此前生产路径**从未调用过它** ⇒ 访存轨迹的时间戳**恒为 0**（导出看着有数据，时间轴却是平的） |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | ✅ v0.8 完整 `ProbeRegistry` + v0.9 `EdgeSink` + v0.11 接线 PR；v0.11.1 修总线 hook 的 RefCell 重入 panic；**D-46：正式挂上生产总线**（此前只在测试里接线） |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | ✅ v0.9 完整 `WaveformPipeline`：边沿编码 + 每通道环 + ≤20ms 批量推送 + CSV/VCD 分块导出；✅ **v0.17 契约 v2 的命名边界**：**history 形**（`schema_version + streams[]`）随内容改名 → `model::WaveformHistory` + 事件变体 `EventPayload::WaveformHistory` + topic `waveform.history`；**live 批次形**（`seq_start/seq_end/t_virt/entries`）**保留原名** `model::WaveformDelta`（**不是**死代码，是 M-11 / **D-50** 的落点，本批未上线） |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | ✅ **v0.18 会话快照：按六段组装**（`capture_session()`）+ `record()` 落记录（`SnapshotRecord { id, payload, label }`——**无第二份 `cp_meta`**）+ `validate_session()`（**只校验、不还原**；⚠️ **同名不同义** = 「一码多义」的变体，有源码级守卫防回退）。⚠️ 输入事件流 / `CrashKit` 等**仍 stub** |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | stub（含 `LogBuffer`） |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | stub（含 `Hms`） |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` | ⚠️ **部分落地**（v0.18）：`DataStore::put_snapshot()` / `get_snapshot()` 改吃 `SessionSnapshotDto`（**写入前先 `validate()`**）+ 新增 `InMemoryDataStore`（键 = `cp_meta.id`）。⚠️ **IndexedDB / SQLite 后端仍 stub** |
| **内核宿主**（装配层 · **不是 23 个模块之一**） | 协议装配：契约命令 ↔ 内核调用 | `crates/kernel/src/kernel_host/` | ✅ **v0.13 第 1 阶段新增**（**5 694 行** / **91 测试**，v0.20 批重算）：16 条命令 → 事件的**规范映射**（形状见 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1）/ 镜像装载（ELF + RawHex）/ 默认内存图 / 寄存器全量与增量波形流 / `handle_json` JSON 桥；**D-46：装上探针管道**（M-18 判沿 → M-19 环 → 契约 `WaveformHistory`）+ 默认内存图映射 GPIO 外设；✅ **v0.17 契约 v2：3 条新命令接线**（`ReadRegisters` / `WriteRegister` / `Negotiate` → `Registers` / `Negotiated`）；✅ **v0.20 契约 v3：4 条调试面命令接线**（`Disasm` / `ResolveLine` / `ExportTrace` / `SetTraceEnabled`）——**宿主路由导出**（`Waveform` ⇒ M-19 的环 / `Execution` ⇒ M-11 的轨迹环）⇒ **闭环 D-50 / D-51** |

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
