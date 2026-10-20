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

## Setup commands

工具链：Rust stable（rustup） + `wasm32-unknown-unknown` target + `wasm-pack`（前端 wasm-pack；服务端原生 Cargo）。

- Install deps:    `rustup target add wasm32-unknown-unknown && cargo install wasm-pack`
- Build (native):  `cargo build`（开发）/ `cargo build --release`（发布）
- Build (WASM):    `tools/build-wasm.sh` 或 `tools/build-wasm.sh --release`
- Test:            `cargo test --workspace`（单元 + 集成 + 黄金对拍）
- Test (前端):    `tools/check-front.sh`（node:test 冒烟门禁，119 用例 / 21 suite；Node ≥ 22）
- Test (浏览器真机): `tools/browser-verify.mjs`（零依赖 CDP 脚本，需本机 Chrome + `python3 -m http.server 8080 --directory front`；**不进默认门禁**，按需运行）
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
│   ├── views/                # M-01 / M-02 / M-03
│   ├── services/             # M-04 / M-05 / M-07 / M-10
│   ├── dev/                  # mock 内核 + demo 数据（仅开发期）
│   ├── lib/                  # 契约 JS 镜像（message.js）+ 传输帧（frame.js）
│   └── style.css
├── tools/
│   ├── build-wasm.sh         # wasm-pack 包装（无依赖 web/bundler，输出 no-modules）
│   ├── check-front.sh        # 前端门禁（node:test）
│   └── browser-verify.mjs    # 浏览器真机复验（零依赖 CDP，按需运行，不进默认门禁）
├── tests/
│   ├── unit/                 # 单元测试
│   └── golden/               # Spike 对拍测试（Wave 2 落地；工具链已就绪）
└── docs/                     # 27 篇设计文档 + reviews/（1 入口 + 9 份模块报告）
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
| M-07 | 传输适配器 | `front/services/m07_transport.js` | ✅ v0.11 实现（Worker + WebSocket 双适配器；当前接 mock 内核） |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | stub |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | stub |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | ✅ v0.11 实现（订阅寄存器/内存快照）；v0.11.1 补断线重连后重订阅 |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | stub |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | ✅ v0.11 加固（快照还原 now_ps+事件队列、推进不空转、seq 不撞号）；v0.11.2 增契约快照 DTO 转换层（`snapshot_dto` / `restore_dto`，**先校验后应用**）+ `Engine::reset()`；⚠️ **至今无独立模块级 review**（台账 D-30） |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | ✅ v0.11 **RV32I 全集 41 条** + 模块级 review；v0.11.1 检视轮修 SYSTEM funct3 阻断级缺陷；M/C 扩展待做 |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | ✅ v0.4 完成 + v0.7.2 补完；v0.11 接线 PR |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | ✅ v0.7 三外设 + v0.11 接线 PR |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | stub |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | stub（含 `SelfIssBackend`）；v0.11.2 接线 `snapshot_full` / `restore_full`（两端对称地大声失败 `E_SNAP-03`），删掉写而不读的死字段 `state`，`reset()` 接 `Engine::reset()` |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | ✅ v0.8 完整 `ProbeRegistry` + v0.9 `EdgeSink` + v0.11 接线 PR；v0.11.1 修总线 hook 的 RefCell 重入 panic |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | ✅ v0.9 完整 `WaveformPipeline`：边沿编码 + 每通道环 + ≤20ms 批量推送 + CSV/VCD 分块导出 |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | stub（含 `SnapshotManager`）；⚠️ 其 LLD §2.1.1 的 `Snapshot{cpMeta, m13State, m14Map, m15Periph}` 与契约 DTO **尚未对齐**，`SnapshotRecord` 仍吃 partial `SimState` 且无 serde derive（台账 **D-37**） |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | stub（含 `LogBuffer`） |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | stub（含 `Hms`） |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` | stub（含 `DataStore` trait） |

> **MVP 跑通判据**（首个能 cargo test 通过的功能闭环）：
> - `m12_engine::tests::engine_advances_time`（事件队列 + 时间推进）
> - `m13_iss::tests::addi_executes`（RV32I ADDI 解释执行，x1=42）
>
> **v0.11 起新增判据**（`cargo test --workspace` **406** 全绿 = 契约 lib 12 + kernel lib 356 + 集成 38）：
> - `m13_iss::tests_rv32i::*`（RV32I 41 条指令 + 黄金编码 + 边界 + CSR 形态 SYSTEM 陷阱 + 步进/回绕/子字存储）
> - `m12_engine::tests::*`（snapshot→restore→继续推进的确定性）
> - `m06_elf::tests_malformed::*`（畸形输入表驱动 + fuzz，均不得 panic）
> - `crates/kernel/tests/m14_m15_m19_wiring.rs`（N5「GPIO 翻转可见」端到端 + 外设路径区域上限）
>
> **v0.11.1 检视轮新增判据**（4 路独立检视后补齐）：
> - `m18_probe::tests::*`（总线 hook 在 `RefCell` 争用下不 panic）
> - `m06_elf::tests::*`（IHEX 嗅探结构判据：raw 二进制首字节 `0x3A` 不再误判；4 GiB 总线边界）
>
> **v0.11.2 快照 DTO 判据**（契约新增可序列化 DTO，D-24 根因闭环）：
> - `crates/contract/src/snapshot.rs::tests::*`（DTO 的 JSON **精确形状** + `validate()` 六类拒绝）
> - `crates/kernel/tests/m12_m17_snapshot_dto.rs`（**15 条**：partial 路径的发散 vs full 路径的不发散 + 畸形快照拒绝 + 被拒后引擎原样不动 + JSON 落盘往返 + M-17 接线 + 契约默认实现）
> - ⚠️ **口径限定**：该 DTO 只覆盖 **M-12/M-17 段（引擎）**，**不含**外设（M-15）/ 中断控制器（M-16）/ 内存映射——落盘完整会话仍需 M-20/M-23 按各自 LLD §2.1.1 组装（台账 **D-36 / D-37**）
>
> **v0.11.2 收口轮判据**（分支合 main 前 3 路独立检视的闭环，commit `3893626`）：
> - 8 项**闭环变异自检全部变红**（口径：`cargo test --workspace --no-fail-fast`——默认 fail-fast 会截断红测条数）
> - 台账合计行 **172 / 143 / 29**（新增「收口轮新增」行 37 / 34 / 3）· 检视产物 `docs/reviews/REVIEWS.md`（§3.1）

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
- WASM 端到端：`tools/build-wasm.sh` 后用 `python3 -m http.server 8080 --directory front` 起 HTTP 服务，浏览器访问 `http://localhost:8080/`（**不能 `file://` 直开**：ES module 与 Worker 均要求同源）
- 黄金对拍：`tests/golden/` 用 Spike 作为外部 oracle，CI 中调 `spike --isa=rv32imc` 跑同一程序对比 `m13_iss` 输出
- 测试位置约定：
  - 单元测试就近写在 `crates/<crate>/src/<module>/mod.rs` 内 `#[cfg(test)] mod tests`
  - 跨模块集成测试放 `tests/unit/`（顶层 `tests/` 目录，Rust 自动识别为集成测试）
  - Spike 对拍脚本放 `tests/golden/`，可执行 `.py` 或 `.sh`

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
