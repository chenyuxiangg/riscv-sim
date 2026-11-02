# AGENTS.md

RISC-V 仿真/调试平台 —— 多模块协作的端到端工程。详细设计在 `docs/detailed-design/`，高层设计在 `docs/{architecture,requirements,module-design-spec}/`。

> 本文件给 AI Agent 读取，遵循 [agents.md](https://agents.md) 开放标准，可被 OpenCode / Codex / Cursor / Aider / Devin / Gemini CLI 等 Agent 共用。
>
> ⚠️ **本文档 ≠ 面向人的项目介绍**：人读的是 [`README.md`](./README.md)（是什么 / 功能 / 快速开始 / 门禁）。**改了代码或数字，两处都要同步。**

---

## 目录

- [Project overview](#project-overview)
- [Setup commands](#setup-commands)
- [Project layout](#project-layout)
- [Module map（23 模块 + 内核宿主）](#module-map23-模块--内核宿主)
- [Code style](#code-style)
- [Testing instructions](#testing-instructions)
- [PR & commit conventions](#pr--commit-conventions)
- [工作循环（Agent 必读）](#工作循环agent-必读)
- [Security](#security)

---

## Project overview

基于详细设计文档，这是一个 **RISC-V（RV32IMC）指令集仿真与调试平台**：自研 ISS + 事件驱动仿真内核 + 零构建浏览器工作台，能跑真实 M 模式 RTOS，并与 Spike 逐字节对拍。

### 技术栈（自架构 §9.7 选型矩阵 + 用户决策）

| 层 | 选型 |
| --- | --- |
| 仿真内核 | Rust（自研 RV32IMC ISS，spike 风格），编译为 native + `wasm32-unknown-unknown`（wasm-pack） |
| 前端 | 原生 JavaScript / ES modules（**零构建、零安装**；波形图走 CDN 的 `echarts` 5.5.1，不可达时优雅降级）。**ES module 与 Worker 均要求同源 HTTP(S)**，`file://` 直开会被浏览器拦截 |
| 3D 引擎（P1） | three.js（内联） |
| 持久化（MVP） | IndexedDB（前端）/ SQLite（服务端 P1） |
| 传输 | postMessage（浏览器内 Worker）/ WebSocket（服务端 P1）；统一 `riscv_sim_contract::protocol::Message` 信封 |
| WASM 工具链 | rustup + `wasm32-unknown-unknown` + `wasm-pack` |
| 正确性对拍 | Spike golden + riscv-tests（外部工具，不进主代码） |
| 测试 | headless CLI + 断言引擎（LLD §9.7 矩阵） |

### 模块划分（23 个详细设计模块，5 横层 + 3 横切）

- **仿真核心**（M-12 ~ M-17、M-20）：事件驱动引擎、RISC-V ISS、总线/内存映射、外设模型、中断控制器、执行后端、快照与确定性事件流
- **调试与探针**（M-08、M-09、M-11、M-18、M-19）：断点管理、单步引擎、DWARF/反汇编/trace 导出、探针、波形管道
- **前端 / 工作台**（M-01、M-02、M-03）：工作台装配与多视图路由、视图组件库、视图状态管理
- **基础设施**（M-04、M-05、M-07、M-10、M-21、M-22、M-23）：会话管理、命令路由与事件分发、传输适配器、寄存器/内存服务、日志、健康管理、持久化 DataStore
- **文件与解析**（M-06）：ELF / Raw-HEX 解析器

### 关键架构约束（继承自 AD §2.4 设计原则）

- **契约先行**（LLD §2.5）：模块间只引 `riscv_sim_contract` 定义的 DTO/事件/错误码/端口签名；内部实现语言不受约束
- **GPL 合规**（AD §2.5 R2）：自研 BSD/MIT 内核；不复用 QEMU 源码；如需 QEMU 走进程隔离
- **零构建**（NFR-05）：前端无打包步骤，原生 ES modules
- **确定性范围**（AD §8.1 D0，评审 R8）：种子 + 时间戳化输入事件序列 → 结果唯一
- **一个约束一个落点**：同一件事**只**有一个定义处；同一判据**只**有一个判定入口

### 规格与追踪文档

- `docs/architecture/RISC-V_Architecture_Design.html` — 4+1 视图 + ADR
- `docs/requirements/RISC-V_Requirements_v1.4.html` — 需求文档
- `docs/module-design-spec/RISC-V_Module_Design_Specification.html` — 23 模块 LLD
- `docs/detailed-design/M-*.html` — 23 篇模块详细设计
- `docs/detailed-design/STAGE-*.md` — **冻结规格**（每个功能批次一份：事实 / 裁决 / 施工期订正 / 变异自检）
- `docs/reviews/REVIEWS.md` — **检视与欠账的单一入口**（未闭环问题 + 优先级）
- `docs/progress/PROGRESS.md` — 当前需求完成情况
- `docs/plans/PRODUCT_BOARD_PLAN.md` — 产品 / 开发板架构落地计划

---

## Setup commands

工具链：Rust stable（rustup）+ `wasm32-unknown-unknown` target + `wasm-pack`。

| 动作 | 命令 |
| --- | --- |
| 安装依赖 | `rustup target add wasm32-unknown-unknown && cargo install wasm-pack` |
| 构建（native） | `cargo build` / `cargo build --release` |
| 起前端 | `python3 tools/serve-front.py`（**唯一落点**，带 `Cache-Control: no-store`） |
| 构建 WASM | `tools/build-wasm.sh` 或 `--release` |
| 测试（Rust） | `cargo test --workspace --no-fail-fast --lib --tests`（⚠️ 不带 `--lib --tests` 会因 doc-test **假挂死**） |
| 测试（前端） | `tools/check-front.sh`（819 用例 / 160 suite，Node ≥ 22） |
| 测试（WASM 桥） | `tools/build-wasm.sh` → `node tools/wasm-smoke.mjs`（产物缺失则跳过） |
| 测试（浏览器真机） | `tools/browser-verify.mjs`（需 Chrome + WASM 产物 + 起前端；不进默认门禁） |
| 测试（黄金对拍） | `tools/fetch-golden-deps.sh` → `tools/golden-run.sh --self-check` → `--keep-going` |
| 测试（真实 RTOS） | `tools/boot-osz.sh`（需仓外 osz ELF + Spike；不进默认门禁） |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 格式 | `cargo fmt --all` |

> **判「全绿」**：看退出码 + `grep -cE "^test result: FAILED"` 为 0。汇总失败数在 `awk -F'[ ;]'` 的 **`$7`**（不是 `$6` —— 连续分隔符产生空字段 ⇒ `$6` 恒为 0 = **假的**）。

> **黄金对拍参考侧 ISA** 必须是 `rv32imc_zicsr_zicclsm_zifencei_zicntr`（`zicclsm` 决定 Spike 是否执行非对齐访问；`zicntr` 决定标准影子 CSR 可读）。

> **跑真机前确认 8080 空闲**：`lsof -nP -iTCP:8080 -sTCP:LISTEN`。上一批遗留的孤儿 `http.server`（工作目录是已删的 worktree ⇒ 任何路径 404）会让脚本判「没在跑」而自起一个 ⇒ **端口冲突** ⇒ 看着像「浏览器门禁坏了」。

---

## Project layout

```
riscv_sim/
├── Cargo.toml                # workspace
├── rust-toolchain.toml       # stable + wasm32 target
├── crates/
│   ├── contract/             # 契约层（DTO/事件/错误码/端口签名，所有模块都引它）
│   │                         #   error.rs / protocol.rs / model.rs / numstr.rs / snapshot.rs / port.rs / csr.rs
│   ├── kernel/               # 仿真内核（16 个子模块 M-06/M-08/.../M-23 + kernel_host 装配层）
│   └── server/               # 服务端形态（P1 占位，MVP 不强制）
├── front/                    # 原生 JS 前端（ES modules，零构建）
│   ├── index.html / app.js / style.css
│   ├── views/                # M-01 / M-02 / M-03
│   ├── services/             # M-04 / M-05 / M-07 / M-10 / M-23（板档案持久化 m23_boardstore.js）
│   ├── lib/                  # 契约 JS 镜像 message.js + 传输帧 frame.js + 纯函数（run-loop/readback/wave-samples/…）
│   └── dev/                  # mock 内核 + demo 数据（仅开发期）
├── tools/                    # 构建与门禁脚本（见下方清单）
├── tests/
│   ├── unit/                 # 单元与集成测试
│   └── golden/               # Spike 对拍 harness（env / canary / corpus.txt / README.md）
└── docs/                     # 88 篇设计文档（26 HTML + 62 Markdown）+ reviews/（1 入口 + 9 份模块报告）
```

### `tools/` 脚本

| 脚本 | 作用 |
| --- | --- |
| `build-wasm.sh` | wasm-pack 包装（`--target web`，命名导出的原生 ESM） |
| `check-front.sh` | 前端门禁（node:test）+ 末尾追加「台账前端两行归属口径 + 历史回归」 |
| `serve-front.py` | **起前端唯一落点**（零依赖，只服务 `front/`，带 `no-store`） |
| `browser-verify.mjs` | 浏览器真机复验（零依赖 CDP，按需运行；启动时查 `staleRustSources` ⇒ 产物旧了报错退出） |
| `wasm-smoke.mjs` | WASM 桥冒烟（Node 驱动真 wasm 产物） |
| `fetch-golden-deps.sh` | 拉 golden 外部依赖（riscv-tests + 交叉工具链）到仓库外 |
| `golden-run.sh` | Spike 对拍（含变异自检；`--check-mem` = 参考侧 RAM 窗口门禁项） |
| `elf-symtab.mjs` | 零依赖 ELF32 .symtab 解析器 |
| `ui-image-fixture.mjs` | 真机门禁的固件夹具（机器码由编码器从助记符推出，禁手抄十六进制） |
| `stage*_check.py` | 每份冻结规格的 7~10 项自检（切分口径复用 `doc_check.split_row`，禁自己写） |
| `mutation_v072.py` | v0.72 批的**变异自检驱动**（T-1 ~ T-7）—— 规格 §7.2 的「实测红集」**可复算**（每条前还原**全部**目标文件 + 复跑基线 + 信号处理还原） |
| `error_code_check.py` | 场景码口径自检（唯一定义 / 值名一致 / 族已声明 / 前端×内核不撞号） |
| `ledger_check.py` | 台账口径自检（`--ledger` 真台账 / `--self-test` 夹具） |
| `front_test_attrib.py` | 台账 §0.1 前端两行「测试」列的唯一算法（从当前树重算） |
| `doc_check.py` | Markdown 文档自检 7 项 |
| `watchdog.pl` | 可移植超时看门狗（fork + 进程组 SIGKILL；124 = HANG） |

---

## Module map（23 模块 + 内核宿主）

> 状态：✅ 落地并通过门禁 ｜ 🟡 部分落地（缺口见 REVIEWS）。
> 版本变更史不在此重复，见 `docs/progress/PROGRESS-history.md` 与各 STAGE 规格。

### 仿真核心

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-12 | 事件驱动仿真引擎 | `crates/kernel/src/m12_engine/` | ✅ |
| M-13 | RISC-V ISS（RV32IMC） | `crates/kernel/src/m13_iss/` | ✅ |
| M-14 | 总线与内存映射 | `crates/kernel/src/m14_bus/` | ✅ |
| M-15 | 外设模型层 | `crates/kernel/src/m15_peripheral/` | ✅ |
| M-16 | 中断控制器 | `crates/kernel/src/m16_interrupt/` | ✅ |
| M-17 | ExecutionBackend 执行后端 | `crates/kernel/src/m17_backend/` | ✅ |
| M-20 | 快照与确定性事件流记录 | `crates/kernel/src/m20_snapshot/` | 🟡 快照已落地；输入事件流/CrashKit 仍 stub |

### 调试与探针

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-08 | 断点管理器 | `crates/kernel/src/m08_breakpoint/` | ✅ |
| M-09 | 单步引擎 | `crates/kernel/src/m09_step/` | ✅ |
| M-11 | 符号表/DWARF/反汇编/trace 导出 | `crates/kernel/src/m11_symbol/` | ✅ |
| M-18 | 探针 | `crates/kernel/src/m18_probe/` | ✅ |
| M-19 | 波形管道 | `crates/kernel/src/m19_waveform/` | ✅ |

### 前端 / 工作台

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-01 | 工作台装配与多视图路由 | `front/views/m01_workbench.js` | ✅ |
| M-02 | 视图组件库 | `front/views/m02_viewcomponents.js` | ✅ |
| M-03 | 视图状态管理 | `front/views/m03_viewstore.js` | ✅ |

### 基础设施

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-04 | 会话管理器 | `front/services/m04_session.js` | ✅ |
| M-05 | 命令路由与事件分发 | `front/services/m05_commandrouter.js` | ✅ |
| M-07 | 传输适配器 | `front/services/m07_transport.js` | ✅ |
| M-10 | 寄存器内存服务 | `front/services/m10_registermemory.js` | ✅ |
| M-21 | 日志系统 | `crates/kernel/src/m21_logging/` | ✅ |
| M-22 | 健康管理 HMS | `crates/kernel/src/m22_hms/` | ✅ |
| M-23 | 数据存储与持久化 DataStore | `crates/kernel/src/m23_datastore/` + `front/services/m23_boardstore.js` | 🟡 前端 IndexedDB 已落地；SQLite 服务端 stub |

### 文件与解析

| 编号 | 模块 | 物理位置 | 状态 |
| --- | --- | --- | --- |
| M-06 | ELF / Raw-HEX 解析器 | `crates/kernel/src/m06_elf/` | ✅ |

### 内核宿主（装配层 · 不是 23 模块之一）

| 模块 | 职责 | 物理位置 |
| --- | --- | --- |
| 内核宿主 | 协议装配：契约命令 ↔ 内核调用；镜像装载 / 内存图 / 探针管道 / 外设中断通路 / 看门狗 | `crates/kernel/src/kernel_host/` |

---

## Code style

- **Rust**：
  - Edition 2021，MSRV 1.75（见 workspace `rust-version`）
  - `cargo fmt --all` 默认配置；`cargo clippy --workspace --all-targets -- -D warnings`（已配 `unsafe_code = "forbid"`、`unused_must_use = "deny"`）
  - 公开 API 显式标 `pub`；不对外暴露的内部状态用 `pub(crate)` 或 `_private` 字段
  - 所有跨模块通信走 `riscv_sim_contract`，不直接 `pub use` 对方内部类型
- **JavaScript**：
  - 原生 ES modules（`<script type="module">`），无打包器
  - 缩进 4 空格，单引号
  - 文件名 kebab-case：`m01-workbench.js` / `m05-commandrouter.js`

---

## Testing instructions

- **单元测试**：`cargo test --workspace --no-fail-fast --lib --tests`（含各模块 `#[cfg(test)] mod tests`）
- **前端门禁**：`tools/check-front.sh`（`node --test`，含最小 DOM stub）——**ESM 链接期错误只有这条门禁能抓住**，Rust 测试覆盖不到
- **前端改动后必须真机复验**：stub 覆盖状态与行为，覆盖不了**挂载时序**——「首次事件派发早于订阅」这类缺陷对 stub 完全隐形。改挂载顺序 / 事件订阅关系后，必须起 `tools/serve-front.py` 用浏览器实跑一遍（别用 `python3 -m http.server`，它不吃缓存）
- **WASM 桥冒烟**：`tools/build-wasm.sh` → `node tools/wasm-smoke.mjs`（Node 里驱动真 wasm 产物跑完整桥）
- **真机复验**：`tools/browser-verify.mjs` —— 真 Chrome + 真 WASM 内核，**断言值**（`x1 == 0x0000002a`）。需先 `build-wasm.sh`（产物缺失 → 退出 127 = 前置缺失，不是验证失败）
- **黄金对拍**：`tools/golden-run.sh` 用 Spike 作外部 oracle；两侧跑**同一个 ELF**，判决与失败用例号都要一致
- **测试位置约定**：
  - 单元测试就近写在 `crates/<crate>/src/<module>/mod.rs` 内 `#[cfg(test)] mod tests`
  - 跨模块集成测试放 `tests/unit/`（顶层 `tests/`，Rust 自动识别）
  - Spike 对拍脚本放 `tools/`，语料 / env / 金丝雀 / 说明放 `tests/golden/`；候选侧 runner 是 `crates/kernel/src/bin/riscv-golden.rs`（harness 代码，**不是 23 模块之一**）

---

## PR & commit conventions

详见 [`CONTRIBUTING.md`](./CONTRIBUTING.md)（含 SSH 配置、conventional commits 完整规范、PR 流程、提交前自检清单、不允许做的事）。

要点速览：

- 默认分支：`main` — **只接受从检视闭环的分支同步过来**，不接受直接往 `main` 提交
- 分支命名：`<type>/<scope>`（如 `feature/m12-engine-quantum`、`fix/m13-iss-bug-123`）
- Commit message：**纯文本**（不用 markdown），固定五要素、各占一行：
  1. `类型：Feature` 或 `类型：Bug`
  2. `说明：<一句话>`
  3. `测试用例：<新增/改动了哪些用例，覆盖什么>`
  4. `测试报告：<实际跑的命令与结果>`
  5. `遗留问题：<已知未解决项；没有就写「无」>`
  - ⛔ 不要写 `feat:` / `fix:` 前缀、不要用 markdown 表格 / 加粗 / 列表 / 代码块
  - ⚠️ 用 `git commit -F <file>`（不要 heredoc）
- 仓库 SSH：`git@github.com:chenyuxiangg/riscv-sim.git`（HTTPS 不支持）
- PR：CI 绿 + 至少 1 个 reviewer 批准；commit message 关联 issue 或 ADR 编号
- 架构变更（涉及 ADR、模块边界）：需更新 `docs/` 下对应设计文档后再合入

---

## 工作循环（Agent 必读）

**核心两句话**：改动一旦完成就 commit，不要留在工作区；**绝不直接往 `main` 提交**。

1. **开分支**（每个模块 / 每件事一个分支）
   ```bash
   git checkout main
   git pull --rebase origin main        # main 必须与 origin/main 一致
   git checkout -b feature/m19-waveform  # 或 fix/... / docs/... / chore/...
   ```
   `main` 上**不允许**有未同步的本地提交。

2. **开发 + commit**（在分支上）
   1. 改代码
   2. 过三关门禁：`cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo test --workspace --no-fail-fast --lib --tests`
   3. `git add` 用**显式路径**——不要 `git add -A` / `.` / `<目录>`（仓库可能有并行会话同时改动，目录级 add 会收进别人的改动）
   4. `git commit`：五要素纯文本；一个 commit 一个逻辑单元
   5. 报告：commit hash、门禁结果、哪些改动**没有**提交及原因

3. **检视（review 闭环）**
   - 检视产物：**`docs/reviews/REVIEWS.md`**（实证复现 + 问题汇总 + 逐项闭环核对；模块级报告另见 `docs/reviews/M-XX_*_Review.md`）
   - 缺陷在**同一个分支**上追加 fix commit 闭环（不重写已推送历史）
   - 检视未闭环完 → **不合并**，`main` 保持不动

4. **同步回 main**
   - 检视闭环 + 门禁全绿后，才把分支同步到 `main`（squash merge 或 fast-forward + merge）
   - 同步后删本地分支 + 远端分支；同步动作需向用户确认后执行

**配套约束**：`git push` 需用户明确指示，不自行推送；`git push --force` 永远禁止。提交前若工作区出现非本次会话产生的改动，先确认归属。修 bug 优先**追加新 commit**，不重写已推送历史。

---

## Security

- 不提交任何密钥、令牌、`.env`（已写入 `.gitignore`）
- WASM 沙箱（ADR §5.1）+ 跨层统一转义（NFR-04）：固件仅在 sandbox 内执行；前端字符串统一 escape，畸形字节按替换符处理
- GPL 合规（R2）：不复用 QEMU 源码；自研 BSD/MIT 内核（spike 风格借鉴非拷贝）
- 会话隔离（FR-13）：每个 session id 独立 snapshot 命名空间，admin 操作走专用 `AdminOp` 通道
