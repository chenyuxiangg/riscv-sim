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
- `docs/detailed-design/STAGE-1_Wasm_Bridge.md` — **第 1 阶段 WASM 桥接口规格（冻结）**：契约命令 ↔ 事件的规范映射表

## Setup commands

工具链：Rust stable（rustup） + `wasm32-unknown-unknown` target + `wasm-pack`（前端 wasm-pack；服务端原生 Cargo）。

- Install deps:    `rustup target add wasm32-unknown-unknown && cargo install wasm-pack`
- Build (native):  `cargo build`（开发）/ `cargo build --release`（发布）
- Build (WASM):    `tools/build-wasm.sh` 或 `tools/build-wasm.sh --release`
- Test:            `cargo test --workspace`（单元 + 集成 + 黄金对拍）
- Test (前端):    `tools/check-front.sh`（node:test 冒烟门禁，152 用例 / 28 suite；Node ≥ 22）
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
└── docs/                     # 28 篇设计文档（含 STAGE-1 桥规格）+ reviews/（1 入口 + 9 份模块报告）
```

## Project layout

### 23 个模块的代码位置映射

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | `front/views/m01_workbench.js` | ✅ v0.10 闭环（20/20 评审） |
| M-02 | 视图组件库 | `front/views/m02_viewcomponents.js` | ✅ v0.10 闭环；v0.11 加调试命令条 + 寄存器/内存视图 |
| M-03 | 视图状态管理 | `front/views/m03_viewstore.js` | ✅ v0.10 闭环 |
| M-04 | 会话管理器 | `front/services/m04_session.js` | ✅ v0.11 实现（状态机 + 断线重连） |
| M-05 | 命令路由与事件分发 | `front/services/m05_commandrouter.js` | ✅ v0.11 实现（订阅/退订/一次性订阅）；v0.11.1 补 `once()` 一次性语义的观察与回归用例 |
| M-06 | ELF / Raw-HEX 解析器 | `crates/kernel/src/m06_elf/` | ✅ v0.11 加固 + **首次模块级 review**（D-03 阻断级消除）；v0.11.1 检视轮再修 IHEX 嗅探判据 + bss gap 溢出 |
| M-07 | 传输适配器 | `front/services/m07_transport.js` | ✅ v0.11 实现（Worker + WebSocket 双适配器）；**v0.13 第 1 阶段起默认挂真 WASM 内核**（`front/kernel-worker.js`），mock 退化为测试替身 |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | ✅ **v0.13 第 1 阶段实现**（53 行 stub → 457 行）：断点表（按 addr 索引 + id 反查）/ 观察点表（地址 + 方向 + 宽度匹配）/ `hit_check` 取指前命中 / 未对齐与重复注册拒 `E_CTX-07` |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | ✅ **v0.13 第 1 阶段实现**（71 行 stub → 308 行）：`StepKind` 四种粒度 + `Plan` + `StepOutcome`；`Over`/`Out`/`Line` **显式降级**为指令级（`degradation()` 把降级说出来，不悄悄降级） |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | ✅ v0.11 实现（订阅寄存器/内存快照）；v0.11.1 补断线重连后重订阅；**v0.13 补 `resubscribe()` / `loadProject()`**——装载工程后内核侧订阅会失效，必须**强制**重订（幂等的 `ensureSubscribed()` 不够，台账 **D-45**） |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | stub |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | ✅ v0.11 加固（快照还原 now_ps+事件队列、推进不空转、seq 不撞号）；v0.11.2 增契约快照 DTO 转换层（`snapshot_dto` / `restore_dto`，**先校验后应用**）+ `Engine::reset()`；⚠️ **至今无独立模块级 review**（台账 D-30） |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | ✅ v0.11 **RV32I 全集 41 条** + 模块级 review；v0.11.1 检视轮修 SYSTEM funct3 阻断级缺陷；v0.12 **RV32I 与 Spike 逐例对拍通过**（语料 51 例中 42 例判决一致；失败用例号一致性由变异金丝雀证明）；v0.13 取指改走 `Bus::read_fetch`（**取指不匹配观察点**，V-A 🟠-2）；M/C 扩展待做（第 3 阶段，台账 D-43） |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | ✅ v0.4 完成 + v0.7.2 补完；v0.11 接线 PR；v0.13 新增 `read_only()`（调试读不计入统计 / trace / 观察点）与 `load_bytes()`（镜像装载的**唯一**写路径），并给 `BusAccess` 增 `is_fetch` 标记（**取指 vs 数据访问**）；**D-46 增 `add_hook()`（追加式 hook，`set_hook` 保留「替换全部」语义）+ `peripheral_instances()` + `tick_peripherals()`** |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | ✅ v0.7 三外设 + v0.11 接线 PR |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | stub |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | ✅ **v0.13 第 1 阶段实现**（stub → 925 行）：`step_n` 真正驱动 M-13 ISS（断点取指前命中 / 陷阱 / 观察点总线 hook 命中）、`read_memory` / `write_memory` 接 M-14 总线、`set_breakpoint` / `set_watchpoint` 接 M-08 表；v0.11.2 的 `snapshot_full` / `restore_full` 接线保持；**D-46：观察点 hook 改 `add_hook`（不顶掉别人的）+ 每条指令前同步一次外设时钟（边沿才有真实时间戳）** |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | ✅ v0.8 完整 `ProbeRegistry` + v0.9 `EdgeSink` + v0.11 接线 PR；v0.11.1 修总线 hook 的 RefCell 重入 panic；**D-46：正式挂上生产总线**（此前只在测试里接线） |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | ✅ v0.9 完整 `WaveformPipeline`：边沿编码 + 每通道环 + ≤20ms 批量推送 + CSV/VCD 分块导出 |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | stub（含 `SnapshotManager`）；⚠️ 其 LLD §2.1.1 的 `Snapshot{cpMeta, m13State, m14Map, m15Periph}` 与契约 DTO **尚未对齐**，`SnapshotRecord` 仍吃 partial `SimState` 且无 serde derive（台账 **D-37**） |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | stub（含 `LogBuffer`） |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | stub（含 `Hms`） |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` | stub（含 `DataStore` trait） |
| **内核宿主**（装配层 · **不是 23 个模块之一**） | 协议装配：契约命令 ↔ 内核调用 | `crates/kernel/src/kernel_host/` | ✅ **v0.13 第 1 阶段新增**（1 517 行 / 34 测试）：16 条命令 → 事件的**规范映射**（形状见 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1）/ 镜像装载（ELF + RawHex）/ 默认内存图 / 寄存器全量与增量波形流 / `handle_json` JSON 桥；**D-46：装上探针管道**（M-18 判沿 → M-19 环 → 契约 `WaveformDelta`）+ 默认内存图映射 GPIO 外设 |

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
- Commit message：Conventional Commits 1.0.0（`feat:` / `fix:` / `docs:` / `refactor:` / `test:` / `perf:` / `build:` / `ci:` / `chore:` / `revert:`）
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
4. `git commit`：Conventional Commits；**一个 commit 一个逻辑单元**，跨模块的多件事拆成多个 commit
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
