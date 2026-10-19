# AGENTS.md

RISC-V 仿真/调试平台 — 多模块协作的端到端工程，详细设计已落地在 `docs/detailed-design/`，高层级设计在 `docs/{architecture,requirements,module-design-spec}/`。

> 本文件给 AI Agent 读取，遵循 [agents.md](https://agents.md) 开放标准，可被 OpenCode / Codex / Cursor / Aider / Devin / Gemini CLI 等 Agent 共用。

## Project overview

基于详细设计文档，这是一个 RISC-V（RV32IMC）指令集仿真与调试平台。

- **技术栈**（自架构 §9.7 选型矩阵 + 用户决策）：
  - **仿真内核**：Rust（自研 RV32IMC ISS，spike 风格），编译为 native + `wasm32-unknown-unknown`（wasm-pack）
  - **前端**：原生 JavaScript / ES modules（**零构建、零依赖**；MVP 不强制单文件自包含，可部署为服务）
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
- `docs/requirements/RISC-V_Requirements_v1.3.html` — 13 条 P0 + 6 层概念空间
- `docs/module-design-spec/RISC-V_Module_Design_Specification.html` — 23 模块 LLD
- `docs/detailed-design/M-01..M-23_*.html` — 23 篇模块详细设计

## Setup commands

工具链：Rust stable（rustup） + `wasm32-unknown-unknown` target + `wasm-pack`（前端 wasm-pack；服务端原生 Cargo）。

- Install deps:    `rustup target add wasm32-unknown-unknown && cargo install wasm-pack`
- Build (native):  `cargo build`（开发）/ `cargo build --release`（发布）
- Build (WASM):    `tools/build-wasm.sh` 或 `tools/build-wasm.sh --release`
- Test:            `cargo test --workspace`（单元 + 集成 + 黄金对拍）
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
│   ├── kernel/               # 仿真内核（含 16 个子模块 M-06/M-08/M-09/M-11/M-12/.../M-23）
│   └── server/               # 服务端形态（P1 占位，MVP 不强制）
├── front/                    # 原生 JS 前端（ES modules，零构建）
│   ├── index.html
│   ├── app.js                # hash 路由 + 启动引导
│   ├── views/                # M-01 / M-02 / M-03 stub
│   ├── services/             # M-04 / M-05 / M-07 / M-10 stub
│   └── style.css
├── tools/
│   └── build-wasm.sh         # wasm-pack 包装（无依赖 web/bundler，输出 no-modules）
├── tests/
│   ├── unit/                 # 单元测试
│   └── golden/               # Spike 对拍测试
└── docs/                     # 27 篇设计文档
```

## Project layout

### 23 个模块的代码位置映射

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | `front/views/m01_workbench.js` | stub |
| M-02 | 视图组件库 | `front/views/m02_viewcomponents.js` | stub |
| M-03 | 视图状态管理 | `front/views/m03_viewstore.js` | stub |
| M-04 | 会话管理器 | `front/services/m04_session.js` | stub |
| M-05 | 命令路由与事件分发 | `front/services/m05_commandrouter.js` | stub |
| M-06 | ELF / Raw-HEX 解析器 | `crates/kernel/src/m06_elf/` | stub |
| M-07 | 传输适配器 | `front/services/m07_transport.js` | stub |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | stub |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | stub |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | stub |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | stub |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | **MVP 重点**（已写调度器骨架） |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | **MVP 重点**（已写 `addi` 解释执行 + 测试） |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | stub |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | stub（含 DummyUart） |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | stub |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | stub（含 `SelfIssBackend`） |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | **MVP 重点**（v0.8 完整 `ProbeRegistry`；v0.9 加 `EdgeSink` 端口完成 M-18 → M-19 接线） |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | **MVP 重点**（v0.9 完整 `WaveformPipeline`：边沿编码 + 每通道环 + ≤20ms 批量推送 + CSV/VCD 分块导出） |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | stub（含 `SnapshotManager`） |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | stub（含 `LogBuffer`） |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | stub（含 `Hms`） |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` | stub（含 `DataStore` trait） |

> **MVP 跑通判据**（首个能 cargo test 通过的功能闭环）：
> - `m12_engine::tests::engine_advances_time`（事件队列 + 时间推进）
> - `m13_iss::tests::addi_executes`（RV32I ADDI 解释执行，x1=42）

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
- WASM 端到端：`tools/build-wasm.sh` 后浏览器跑 `front/index.html`
- 黄金对拍：`tests/golden/` 用 Spike 作为外部 oracle，CI 中调 `spike --isa=rv32imc` 跑同一程序对比 `m13_iss` 输出
- 测试位置约定：
  - 单元测试就近写在 `crates/<crate>/src/<module>/mod.rs` 内 `#[cfg(test)] mod tests`
  - 跨模块集成测试放 `tests/unit/`（顶层 `tests/` 目录，Rust 自动识别为集成测试）
  - Spike 对拍脚本放 `tests/golden/`，可执行 `.py` 或 `.sh`

## PR & commit conventions

详见 [`CONTRIBUTING.md`](./CONTRIBUTING.md)（含 SSH 配置、conventional commits 完整规范、PR 流程、提交前自检清单、不允许做的事）。

要点速览：
- 默认分支：`main`
- 分支命名：`<type>/<scope>`（如 `feature/m12-engine-quantum`、`fix/m13-iss-bug-123`）
- Commit message：Conventional Commits 1.0.0（`feat:` / `fix:` / `docs:` / `refactor:` / `test:` / `perf:` / `build:` / `ci:` / `chore:` / `revert:`）
- 仓库 SSH：`git@github.com:chenyuxiangg/riscv-sim.git`（HTTPS 不支持）
- PR：CI 绿 + 至少 1 个 reviewer 批准；commit message 关联 issue 或 ADR 编号
- 架构变更（涉及 ADR、模块边界）：需更新 `docs/` 下对应设计文档后再合入

## Security

- 不提交任何密钥、令牌、`.env`（已写入 `.gitignore`）
- WASM 沙箱（ADR §5.1）+ 跨层统一转义（NFR-04）：固件仅在 sandbox 内执行；前端字符串统一 escape，畸形字节按替换符处理
- GPL 合规（R2）：不复用 QEMU 源码；自研 BSD/MIT 内核（spike 风格借鉴非拷贝）
- 会话隔离（FR-13）：每个 session id 独立 snapshot 命名空间，admin 操作走专用 `AdminOp` 通道
