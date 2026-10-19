# riscv_sim

RISC-V（RV32IMC）指令集仿真与调试平台。

## 当前状态

- ✅ **设计文档完整**：27 篇文档已归档
  - 1 篇架构设计（4+1 视图 + ADR）
  - 1 篇需求分析（v1.3）
  - 1 篇模块设计说明书
  - 23 篇模块详细设计（M-01..M-23）
- ✅ **工程骨架就位**：Cargo workspace（contract / kernel / server 三 crate）+ 原生 JS 前端
- ✅ **MVP 重点（M-12 引擎 + M-13 ISS）已写骨架 + 单元测试**：能跑通 `addi x1, x0, 42` 这条指令
- ⏳ **其他 14 个模块（M-06/08/09/11/14/15/16/17/18/19/20/21/22/23）**：stub 状态，端口已定义，实现为 TODO(MVP)
- ⏳ **前端 7 个模块（M-01/02/03/04/05/07/10）**：stub 状态，hash 路由 + 工作台骨架已就位
- ⏳ **未 `git init`**：等 Rust 工具链装好并 `cargo check` 通过后初始化

## 技术栈（来源：AD §9.7 选型矩阵 + 用户决策）

| 层 | 选型 |
|---|---|
| 仿真内核 | **Rust**（自研 RV32IMC ISS），编译为 native + WASM |
| 仿真引擎 | 事件驱动（1µs 量子） |
| 外设建模 | 分级 Peripheral（寄存器接口 + 行为时序） |
| 前端 | **原生 JavaScript**（ES modules），零构建 |
| 3D 引擎（P1） | three.js |
| 持久化 | IndexedDB（前端 MVP）/ 服务端 P1 |
| 传输 | postMessage（Worker）/ WebSocket（服务端 P1） |
| WASM 工具链 | rustup + wasm32 target + wasm-pack |
| 正确性对拍 | Spike golden + riscv-tests（外部工具） |

## 快速开始

### 1. 装 Rust 工具链

```bash
# macOS (Homebrew)
brew install rustup-init && rustup-init -y --default-toolchain stable

# 或用 rustup 官方脚本
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 装 WASM target + wasm-pack
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
```

### 2. 编译并跑测试

```bash
cargo build                         # 编译 native
cargo test --workspace              # 跑所有单元测试
cargo clippy --workspace -- -D warnings
```

### 3. 构建浏览器 WASM

```bash
tools/build-wasm.sh                 # 调试版
tools/build-wasm.sh --release       # 发布版（更小）
```

输出到 `front/assets/pkg/riscv_sim_kernel_bg.wasm`。

### 4. 打开前端

```bash
# 任何静态服务器都行（避免 file:// 的 Worker 限制）
python3 -m http.server 8080 --directory front
# 浏览器访问 http://localhost:8080
```

## 模块全景（按职责）

### 仿真核心
- M-12 事件驱动仿真引擎 — **MVP 重点** ✅
- M-13 RISC-V ISS (RV32IMC) — **MVP 重点** ✅
- M-14 总线与内存映射
- M-15 外设模型层
- M-16 中断控制器
- M-17 ExecutionBackend 执行后端
- M-20 快照与确定性事件流记录

### 调试与探针
- M-08 断点管理器
- M-09 单步引擎
- M-11 符号表 / DWARF / 反汇编 / trace 导出
- M-18 探针
- M-19 波形管道

### 前端 / 工作台
- M-01 工作台装配与多视图路由
- M-02 视图组件库
- M-03 视图状态管理

### 基础设施
- M-04 会话管理器
- M-05 命令路由与事件分发
- M-07 传输适配器
- M-10 寄存器内存服务
- M-21 日志系统
- M-22 健康管理 HMS
- M-23 数据存储与持久化 DataStore

### 解析器
- M-06 ELF / Raw-HEX 解析器

详细说明见 `docs/detailed-design/` 对应文件。

## 目录

| 路径 | 用途 |
| --- | --- |
| `AGENTS.md` | 给 AI Agent 读取的项目定义（agents.md 标准） |
| `Cargo.toml` | Rust workspace manifest |
| `crates/contract/` | 共享契约层（DTO/事件/错误码/端口签名） |
| `crates/kernel/` | 仿真内核（16 个子模块） |
| `crates/server/` | 服务端形态（P1 占位） |
| `front/` | 原生 JS 前端（零构建） |
| `tools/build-wasm.sh` | wasm-pack 包装 |
| `tests/` | 集成测试 + Spike 对拍 |
| `docs/architecture/` | 架构设计 |
| `docs/requirements/` | 需求分析 |
| `docs/module-design-spec/` | 模块设计说明书 |
| `docs/detailed-design/` | 23 篇模块详细设计 |

## 下一步

1. **装 Rust 工具链**（见上方"快速开始"）
2. **`cargo test --workspace` 验证 MVP 跑通**（M-12 引擎 + M-13 ISS 测试）
3. **按实施批次推进**：
   - 第 1 批：M-12 / M-13 / M-14 / M-15 / M-16 / M-17（仿真核心闭环）
   - 第 2 批：M-08 / M-09 / M-10 / M-11 / M-18 / M-19 / M-20（调试与观测）
   - 第 3 批：M-04 / M-05 / M-07 / M-21 / M-22 / M-23 / M-06 + 前端全部模块
4. **接入 Spike 对拍**（`tests/golden/` 目录占位，待补脚本）
5. **`git init` 初始化仓库 + 首次提交**

---

设计文档来源：[豆包云盘 - 详细设计合集](https://www.doubao.com/drive/shr/LvPXfddHzlxcVldau9KctmCKnBb)
