# Project Progress · 项目进度

> **Document version**: v0.1
> **Created**: 2026-10-19
> **Last updated**: 2026-10-19
> **Status**: active
> **Owner**: zyzs
> **Source of truth**: this document, synced with git history (`git log` + `git tag`)

---

## 0. TL;DR

RISC-V 仿真/调试平台项目处于 **MVP 启动期**。架构 + 23 篇模块详细设计已归档；3-crate Cargo workspace + 23 个子模块骨架已建立；M-12 事件驱动引擎和 M-13 RV32I ISS 实现了最小子集（`addi` 单指令可执行，`cargo test --workspace` 两个核心单测 PASS）。

按架构文档 §13.6 WBS 估算，**当前完成度约 13% / 90 人日**。下一步沿 §13.6 关键路径推进，目标：完成 **E2E-01 智能灯验收主线**（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）。

> ⚠️ **重要纠偏**：早期对话里把"MVP"误读为"两个单元测试通过"。重新校准后，正确定义见 §2。

---

## 1. 当前状态（截至 v0.1）

### 1.1 已完成（Done）

| 类别 | 内容 |
| --- | --- |
| 设计文档 | 27 篇（arch 1 + requirements 1 + module-design-spec 1 + detailed-design 23 + AGENTS.md + README.md） |
| 工程骨架 | 3-crate Cargo workspace：`riscv_sim_contract` / `riscv_sim_kernel` / `riscv_sim_server` |
| 契约层 | 5 个核心数据模型（`Project` / `SimState` / `DebugMeta` / `Waveform` / `Message`），`CommandPayload` / `EventPayload` 协议信封，13 个 `ErrorFamily`，6 个 port trait（`CommandBus` / `EventBus` / `ExecutionBackend` / `Peripheral` / `InterruptSink` / `Probe`） |
| 内核子模块 | 16 个 M- 模块全部建好（M-12 + M-13 实际实现，其余 14 个为公开端口/结构体形状的 stub） |
| 前端骨架 | `index.html` + `app.js`（hash 路由）+ 5 个空视图 stub（M-01/02/03/04/05/07/10） |
| 工具链脚本 | `tools/build-wasm.sh`（wasm-pack no-modules 包装） |
| 测试 | `cargo test --workspace` 通过，2 个核心单测 PASS：`m12_engine::engine_advances_time` + `m13_iss::addi_executes` |
| 版本控制 | git 初始化，main 分支，一次 initial commit（`c69e570`） |

### 1.2 部分完成（In progress）

| 模块 | 现状 | 缺口 |
| --- | --- | --- |
| **M-12 事件驱动引擎** | `Engine<B, I>` 调度器骨架，`run_until_idle` 推进 1µs 量子 → 发 `EventPayload::Tick` → 排空到期事件；`SchedEvent::{PeripheralTick, TimerExpiry, Commit}` 派发 | 没接 ISS 驱动指令；instruction budget 未生效；`PeripheralTick` / `Commit` 派发是空实现 |
| **M-13 RV32I ISS** | 手工 decoder 拆字段（opcode/rd/rs1/rs2/funct3/funct7/imm_I），Executor 实现 `ADDI`（`x[rd] = x[rs1] + imm`，`PC += 4`） | 只 1 条指令；缺 LUI / AUIPC / LOAD / STORE / BRANCH / JAL / JALR / M 扩展 / C 扩展；非法指令返 trap |
| **M-06 ELF 解析** | `Loader` trait + `DefaultLoader`（假实现：bytes 整体塞地址 0） | 没用 `object` crate，ELF header / PT_LOAD 段都没解析 |
| **M-14 总线** | `BusMaster` trait + `BusTarget::{Ram, Peripheral, None}` | 没真 RAM 页表、没地址译码、没 peripheral 分发 |
| **M-15 外设** | `PeripheralRegistry` + `DummyUart`（读 0 / 写忽略） | 行为级 UART（FR-07 波特率/CTS 流控）未实现；GPIO / Timer 完全空 |
| **M-16 中断控制器** | `InterruptController` + `BTreeSet<u16>` 存已 raise 的 IRQ | 没 `mip/mie/mcause` CSR 联动；没接入 `step_one` 的中断采样 |
| **M-17 执行后端** | `SelfIssBackend<B, I>` 包装 Engine + Iss，实现 `ExecutionBackend` 全部 11 个方法（多为 stub） | `step_n` 永远 `Ok(0)`，没真驱动 ISS；断点 / 监视点切换是空操作 |
| **M-18 探针** | `InMemoryProbe::observe/drain`（修了上次 `last_mut` bug 和 `drain_waveform` swap hack）+ `ProbeRegistry::drain_waveform` | 没任何 signal source 挂到 bus / peripheral |
| **M-19 波形** | `WaveformChannel` push/last 桩 | 没和 ProbeRegistry 联动 |
| **M-20 快照** | `SnapshotManager` 内存存 `Vec<SnapshotRecord>` | 没 M-23 持久化；没崩溃自动快照触发 |
| **M-21 日志** | `LogBuffer` 4096 环形 + `Level::{Trace..Fatal}` | 没接 `tracing` crate；没 admin 通道暴露 |
| **M-22 HMS** | `Hms::report` 单调 `Ok → Degraded → Critical` | 没 watchdog 联动；没降级动作 |
| **M-23 DataStore** | `DataStore` trait（put/get/list Project + SnapshotRecord） | 没 IndexedDB / SQLite 实现 |
| **前端** | index.html + hash 路由 + 5 个空视图 mount 函数 | 全部视图是 `Promise.resolve()` 空 stub；没接 kernel；没 waveform UI |

### 1.3 完全未做（Not started）

- 真实 ELF / DWARF 解析（FR-05）
- 调试服务（断点 / 单步 / Reg/Mem 视图 / 反汇编·源码联动）
- 观测管道 wire-up（探针源 → 边沿编码 → 增量推送 → 波形 UI）
- 前端多视图可见化
- postMessage 传输（M-07）+ 会话管理（M-04）+ 命令路由（M-05）
- IndexedDB 持久化
- Worker 韧性 + watchdog + 崩溃自动恢复
- Spike golden 对拍
- 智能灯 E2E-01 验收

---

## 2. MVP 范围（按 AD §11.1 + §3.4 + §13.3）

### 2.1 MVP 定义

> **MVP（P0）**：13 条 P0 功能（FR-01~12 + FR-23）· 2D 产品树/原理图 · 调试联动 + 波形 · 多视图 + 快照 · WASM 零安装
> —— 架构文档 §11.1

**部署形态**：浏览器 WASM 单文件自包含（file:// 直接打开，NFR-05）。本项目经用户决策可放宽为「可部署服务」双形态。

**装配边界**（§3.4 评审 L1）：
- ✅ 嵌入式工作台（反汇编/断点/单步、Reg/Mem、trace、波形、项目树）— MVP 全量
- ✅ 产品工作台基础面（搭建/运行/波形）— MVP 子集
- ❌ 产品行为判定 / 系统管理员工作台 — P1
- ✅ HMS MVP 基线 = 崩溃自动快照 + 恢复（§10.7 三条）
- ❌ HMS 完整分角色分发 — P1

### 2.2 验收主线（§13.3 E2E-01 智能灯）

> 智能灯固件跑通（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）贯穿五横层

| 步骤 | 涉及层 | 涉及模块 | 当前可达性 |
| --- | --- | --- | --- |
| 搭建产品树 | L0/L1 前端 | M-01 / M-02 | ❌ 前端 stub |
| ELF 加载 | 调试服务层 | M-06 | ⚠️ 假实现 |
| 运行 | 仿真内核层 | M-12 + M-13 + M-14 | ⚠️ engine 调度器 + ISS ADDI 单步可，但**未串通** |
| GPIO 波形 | 观测管道层 | M-15 + M-18 + M-19 | ❌ 无探针源 |
| 单步/内存 | 调试服务层 | M-08 / M-09 / M-10 | ❌ stub |
| 行为判定 | L0 前端 | M-01 | ❌ |

**结论**：E2E-01 当前 **0/6 步端到端可达**。

### 2.3 MVP 不做的事（明确边界）

- 3D 渲染 / 帧缓冲（FR-21 / FR-22）— P1
- 多板互联（FR-24~27，里程碑 B）— P1
- GDB RSP / QEMU 双内核 — P2
- 多核 SMP — P2
- DWARF 源码联动可降级（§13.6 最大风险项）：MVP 退化为「反汇编 + 符号表，无源码联动」，仍算 MVP 范围

---

## 3. 关键路径（§13.6）

> 关键路径依赖：协议冻结 → ISS → 调试服务/观测管道 → 前端；DWARF 与前端视图可并行
> —— 架构文档 §13.6

```
[已 ✅] 协议冻结          (contract crate 完成, 5 人日)
   ↓
[   1  ] 产品树/内存映射  (M-01/M-02/M-14, 4 人日)        ─┐
[   2  ] ELF 解析         (M-06, 3 人日)                    │
   ↓                                                        │ 并行可行
[   3  ] ISS 全集 RV32IMC (M-13, 12 人日)                   │ DWARF [4*]
[   4  ] 事件引擎         (M-12, 6 人日)                    │ 10 人日
[   5  ] 外设 UART/GPIO/Timer (M-15, 6 人日)                │
[   6  ] 调试服务         (M-08/M-09/M-10/M-11, 8 人日)     │
[   7  ] 观测管道         (M-18/M-19, 6 人日)              ─┘
   ↓
[   8  ] 前端视图/路由/波形 UI (M-01/M-02/M-03, 10 人日)
   ↓
[   9  ] 持久化 IndexedDB (M-23, 3 人日)
[  10  ] Worker 韧性 + watchdog + HMS (M-22, 3 人日)
   ↓
[验收]  E2E-01 智能灯 跑通
```

**已做的人日**（v0.1 估算）：
- ✅ 协议 5
- ⚠️ ISS 2 / 12（只 ADDI 子集）
- ⚠️ 引擎 1 / 6（调度器骨架）
- ⚠️ 总线 0.5 / 4（trait only）
- ⚠️ ELF 0.5 / 3（假 loader）
- ⚠️ 探针 0.5 / 6（修了 drain 但无源）
- ⚠️ 前端 1 / 10（骨架 + 空 stub）

**累计 ~10.5 / 90 人日 ≈ 12%**。

---

## 4. 完整 WBS（按 AD §13.6）

| # | 模块 | 人日 | 前置依赖 | 风险/降级 | 状态 | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 命令/事件协议 | 5 | — | 中；先冻结 | ✅ | contract crate |
| 1 | 产品树 / 内存映射 | 4 | 协议 | 低；区间树 | ⚠️ | M-01/02 stub |
| 2 | ELF / Raw / HEX 解析 | 3 | — | 中；畸形输入容量上限 | ⚠️ | M-06 假实现 |
| 3 | ISS RV32IMC | 12 | WASM 工具链 | 中；Spike 对拍 | ⚠️ | M-13 只 ADDI |
| 4 | 事件驱动引擎 | 6 | ISS 接口 | 中；确定性夹具 | ⚠️ | M-12 调度器 |
| 5 | 外设 UART/GPIO/Timer | 6 | 总线接口 | 低；分级建模 | ❌ | M-15 only DummyUart |
| 6 | 调试服务 | 8 | ISS、协议 | 中；依赖 hook | ❌ | M-08/09/10 stub |
| 7 | 观测管道 | 6 | ISS 钩子 | 中；差异化核心 | ⚠️ | M-18 drain 修了 |
| 8 | DWARF 行表 + 源码联动 | 10 | ELF 解析 | **最大风险** | ❌ | 可降级 |
| 9 | 前端视图 / 路由 / 波形 UI | 10 | 协议、观测管道 | 中；零构建 | ⚠️ | index.html + 5 空 stub |
| 10 | 持久化 IndexedDB | 3 | 数据模型 | 低 | ❌ | M-23 trait |
| 11 | Worker 韧性 + watchdog + HMS | 3 | postMessage | 中；guest 失控 | ❌ | M-22 占位 |
| 12 | 场景脚本/断言（接口预留） | 2 | 协议、确定性 | 低 | ❌ | P1 起步 |
| 13 | 多板互联（接口预留） | 2 | Peripheral port | 中 | ❌ | P1 立项 |
| **合计** | | **90** | | | | |

---

## 5. 下一里程碑：E2E-01 智能灯端到端

按 §13.3 倒推，下一轮要做的纵向切片（让 E2E-01 6 步全绿）：

1. **M-14 总线** — 4 KiB 稀疏 RAM 页表，区间树地址译码（4 → 实际可减到 1.5 人日）
2. **M-13 ISS 扩到 RV32I 完整基指令**（ADDI + LUI + AUIPC + LOAD + STORE + BRANCH + JAL + JALR）— 智能灯至少要 BRANCH/LOAD/STORE；扩到全集约 8 人日，MVP 子集可压到 3
3. **M-12 引擎接通 ISS** — 在每量子 instruction budget 内调 `iss.step_one`；engine 现在 run_until_idle 不会触发 ISS，需要新加 `run_quantum_with_iss` 路径
4. **M-08 断点接通** — `iss.step_one` 在 fetch 前查 pc 命中就返 `StepOutcome::Breakpoint`
5. **M-06 ELF 真解析** — 引入 `object` crate（BSD-2-Clause，合规），解 PT_LOAD 段写进 bus；DWARF 留 P1
6. **M-15 + M-18 + M-19 联动** — UART TX 引脚挂探针；observe 进 `InMemoryProbe`；`WaveformChannel.push` 给前端
7. **M-10 + 前端最小可见** — 产品树视图（DOM 树形结构）、波形视图（canvas 边沿时间轴）、一个 Run 按钮、能 drop ELF
8. **M-07 + M-04 + M-05** postMessage 传输 — Worker 内核 ↔ 主线程 UI
9. **M-23 IndexedDB** — Project + SnapshotRecord 落盘，原子写
10. **M-22 HMS MVP** — watchdog 每 N 量子检查宿主时间预算，超时自动快照 + 抛 `HaltReason::Watchdog`

**验收**：智能灯 ELF 拖进浏览器 → 自动加载 → 点 Run → 看到 LED 引脚 GPIO 翻转的波形 → 单步停在断点 → 改内存 → 继续 → 行为符合预期。

---

## 6. 风险与降级（按 AD §12）

| ID | 风险 | MVP 应对 | 降级方案 |
| --- | --- | --- | --- |
| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | 推迟到 P1 | MVP 退化为「反汇编 + 符号表」 |
| R8 | 确定性范围 | 种子 + 时间戳化输入事件序列（AD §8.1 D0） | — |
| R10 | 多板互联复杂度 | MVP 单板不阻塞；接口预留 | — |
| R12 | 性能 5-10 MIPS 估算基线 | 解释执行 MVP | 演进解锁 JIT/QEMU |
| L1 | 装配边界 | §3.4 评审 L1 子集 | — |
| L3 | 客户端输入威胁 | 客户端低；服务端放大时强制 | — |

---

## 7. 决策记录（按时间倒序）

| 日期 | 决策 | 依据 |
| --- | --- | --- |
| 2026-10-19 | 自研 ISS 用 **Rust**（非 C） | 用户明确要求 |
| 2026-10-19 | MVP 部署形态可**目录形式**（不必单文件自包含） | 用户放宽 NFR-05 |
| 2026-10-19 | 骨架策略：先做 stub | 用户 |
| 2026-10-19 | 实施起点：先跑通 MVP | 用户（**误读**为「M-12 + M-13 单测通过」非「E2E-01 跑通」） |
| 2026-10-19 | MVP 范围以 **AD §13.6** 为准（13 P0 + 90 人日） | 重读架构文档后校准 |
| 2026-10-19 | `contract::WatchAccess` 三个变体补 doc；其他 variant 字段 doc 暂以 `#![warn(missing_docs)]` 跳过 | Rust 1.99 严格度 |
| 2026-10-19 | `m18_probe::drain_waveform` 重写为 `values_mut()` + `Probe::drain()`，替换原 `mem::swap` hack | cargo test 失败 |
| 2026-10-19 | `m12_engine::run_until_idle` 改为「每量子推进 → 排空到期」循环 | 原语义让 `engine_advances_time` 测试只走 1 量子 |
| 2026-10-19 | `m17_backend::SelfIssBackend` bound 从 `BusMaster` 改为 `EventBus` | `Engine` 方法要 EventBus bound |
| 2026-10-19 | `m15_peripheral::PeripheralRegistry` 拿掉 `derive(Debug)`，手写只输出 count | `Box<dyn Peripheral>` 不 Debug |

---

## 8. 待澄清问题

| 编号 | 问题 | 阻塞谁 | 选项 |
| --- | --- | --- | --- |
| Q1 | E2E-01 智能灯固件是哪一个？自己写一个最小固件，还是用 riscv-tests / Spike 提供的？ | M-06 ELF + M-13 ISS 选型 | (a) 自写 200 行 RV32I 智能灯（最简）；(b) 用 riscv-tests 现有基准；(c) Spike 自带 dhrystone / qsort 之一 |
| Q2 | ISS 指令集是只 RV32I 还是 RV32IMC 全集？ | M-13 工作量 | (a) MVP 子集 RV32I（约 47 条 → 压到 ~30 条足够智能灯）；(b) RV32IMC 全集 |
| Q3 | 持久化 MVP 选 IndexedDB 还是 localStorage？ | M-23 | 架构说 IndexedDB（容量），但 localStorage 简单 |
| Q4 | 前端 Worker 是用同源 URL 还是 blob URL？ | M-07 | 架构说 file:// 下 blob URL；现代浏览器默认同源 OK |
| Q5 | 智能灯验证判定逻辑放哪？前端 L0 还是后端断言引擎？ | M-01 / M-22 | 架构说 L0 终点；具体是 DOM 渲染 LED 灯状态还是 canvas 颜色对比 |
| Q6 | 性能基线 5-10 MIPS 在什么硬件上？ | 性能验收 | 用户机 (M3) 还是泛指 |
| Q7 | Spike golden 对拍是否在 MVP 必做？ | M-13 验证 | 架构说「逐模块验证 FR-04」；MVP 可只对 ADDI |

---

## 9. 文档版本历史

| 版本 | 日期 | 变更 | 作者 |
| --- | --- | --- | --- |
| v0.1 | 2026-10-19 | 初稿：建立项目状态、§13.6 WBS 校准、关键路径、下一里程碑 E2E-01、决策与风险 | agent (mavis) |

---

## 10. 设计文档引用

- **AD §11.1** 演进路线（里程碑时间轴）— MVP 范围定义
- **AD §13.6** WBS + 关键路径 + 人日估算
- **AD §13.3** 智能灯 E2E-01 验收主线
- **AD §3.4** 角色驱动工作台装配 + §3.4 评审 L1 最小化边界
- **AD §5.2** 量子化混合调度（1µs 量子）
- **AD §8.1** 确定性 D0
- **AD §8.4** 安全沙箱
- **AD §12** 风险清单 R1~R12
- **需求 FR-01~12 + FR-23** 13 条 P0
- **需求 §3.2** 智能灯 E2E-01 业务主线
- **LLD §2.5** 契约层定义（contract crate 唯一来源）
- **M-12 / M-13 / M-14 / M-06 / M-15 / M-18 / M-19 / M-08** 模块详细设计

---

## 11. 仓库与命令

```bash
# 验证
source $HOME/.cargo/env
cd /Users/zyzs/lczyzs/code/riscv_sim
cargo test --workspace          # 2 个 MVP 单测 PASS

# 提交
git status                      # 改动一览
git add -p                      # 交互式暂存
git commit -m "feat(m12): ..."  # conventional commits

# WASM 构建
tools/build-wasm.sh             # 输出 front/assets/pkg/

# 前端启动
python3 -m http.server 8080 --directory front
```

---

## 12. 不在本文档范围

- 详细 API 文档（各模块 `///` doc comment + 详细设计文档）
- Spike golden 对拍脚本（`tests/golden/`）
- 端到端测试夹具（智能灯固件）
- CI 配置（`.github/workflows/` 或等价）
- 部署脚本（`tools/deploy.sh`）
