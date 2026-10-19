# Project Progress · 项目进度

> **Document version**: v0.3
> **Created**: 2026-10-19
> **Last updated**: 2026-10-19
> **Status**: active
> **Owner**: zyzs
> **Source of truth**: this document, synced with git history (`git log` + `git tag`)

## v0.3 变更摘要

**MVP 范围决策**：从 v0.2 的「选项 B（~30 人日，DWARF 留 P1 降级）」**升级到「选项 C（§13.6 全量 90 人日）」**。理由：完整 13 P0 覆盖，不留降级；含 DWARF 行表 + 源码联动（MVP 风险最大项）、自动化 CI 与场景脚本断言、多板互联接口预留。

**M0 协议冻结**（5d）已 ✅ 完成；剩余 **85 人日** 待开发。详见 §5 阶段计划、§4 WBS、§8 前置条件。

## v0.2 变更摘要（保留追溯）

Q1-Q7 全部答复（见 §8）：
- Q1 智能灯固件 = **自研**（~200 行 RV32IMC 最小固件）
- Q2 ISS 指令集 = **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集）
- Q3 持久化 = **IndexedDB**（用 `idb` ~5KB 包装，非 localStorage）
- Q4 Worker URL = **同源 URL**（HTTP 部署，`./kernel-worker.js`）
- Q5 行为判定 = **canvas 比对**（虚拟 LED 渲染 + 像素 diff + 视觉对照）
- Q6 性能基线 = **macOS**（M3 Apple Silicon）
- Q7 Spike 对拍 = **必做**（FR-04，每条指令 golden）

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

## 5. 完整开发计划（C 方案 · §13.6 全量 90 人日）

### 5.1 阶段切分

按架构 §13.6 WBS 全量开发，关键路径 **70 人日**（含 10 人日联调 buffer），并行支线 20 人日。

| 阶段 | 名称 | 人日 | 关键依赖 | 阶段交付 | 状态 |
| --- | --- | --- | --- | --- | --- |
| **M0** | 协议冻结 | 5 | — | contract crate + 5 model + 6 port + 13 错误族 | ✅ |
| **M1** | 数据底座 | 7 | M0 | 产品树视图 + 内存映射 + ELF/Raw/HEX 解析 | ⚠️ |
| **M2** | 内核 | 18 | M0 | ISS RV32IMC 全集（I+M+C） + 事件驱动引擎 + 量子化调度 | ⚠️ |
| **M3** | 外设 | 6 | M1（总线接口） | UART 行为级（波特率/CTS）+ GPIO + Timer + 注册表 | ❌ |
| **M4** | 观测 | 6 | M2（ISS 钩子） | 探针 + 边沿编码 + 波形通道 + 增量推送 | ⚠️ |
| **M5** | 调试 + 反汇编 | 18 | M2, M1（ELF） | 断点 + 单步（StepOver/Out/Line）+ Reg/Mem + 反汇编视图 + **DWARF 行表 + 源码联动** | ❌ |
| **M6** | 前端 | 10 | M0, M4 | 多视图 hash 路由 + 波形 UI + 嵌入式工作台 + 产品工作台基础面 | ⚠️ |
| **M7** | 横切 | 8 | M6 | IndexedDB 持久化 + Worker 韧性/watchdog + HMS 崩溃快照恢复 + 场景脚本/headless CLI | ❌ |
| **M8** | 多板预留 | 2 | M3 | L1 外设 port 抽象 + L3 事件队列消息槽（不绑定链路） | ❌ |
| **M9** | 联调 + CI + E2E-01 | 10 | 全部 | 智能灯端到端验收 + Spike golden 对拍 CI + macOS 性能 baseline | ❌ |
| **合计** | | **90** | | | |


### 5.2 关键路径图

```
M0 (5d) ── M1 (7d) ── M2 (18d) ── M3 (6d) ── M4 (6d) ── M6 (10d) ── M7 (8d) ── M9 (10d)
                  │                   │            │             │          │          │
                  └── M5/DWARF(10d) ───┘            │             │          │          │
                                                  M5/调试(8d) ── ┘          M8(2d) ───┘
```

### 5.3 13 P0 覆盖矩阵（C 方案 = 100%）

| FR | 内容 | 责任模块 | 阶段 |
| --- | --- | --- | --- |
| FR-01 | 图形化搭建（多节点、多外设） | M-01 / M-02 / M-14 | M1 + M6 |
| FR-02 | 元器件事例化、地址映射配置 | M-01 / M-02 / M-14 / M-15 | M1 + M3 |
| FR-03 | 外设地址映射与查看/修改 | M-14 / M-15 + M-10 UI | M3 + M6 |
| FR-04 | RV32I/IM、伪指令与别名 | M-13 + tests/golden/ | M2 + M9 |
| FR-05 | 反汇编·源码联动 | M-11 反汇编 + M-06 DWARF 行表 | M5 |
| FR-06 | 异常/中断、向量表配置 | M-12 量子化 + M-16 中断控制器 | M2 + M3 |
| FR-07 | UART 波特率/CTS 流控 | M-15 行为级 UART | M3 |
| FR-08 | 断点/单步 | M-08 断点 + M-09 单步引擎 + M-10 UI | M5 + M6 |
| FR-09 | 寄存器/内存查看修改 | M-10 UI + ExecutionBackend 接口 | M5 + M6 |
| FR-10 | GPIO 波形/逻辑分析 | M-15 GPIO + M-18 探针 + M-19 波形 | M3 + M4 + M6 |
| FR-11 | 变量监视、手动读写（地址型） | M-08 watchpoint + M-10 watch UI | M5 + M6 |
| FR-12 | 确定性、可复现 | M-12 量子化 + M-20 快照 + M-23 落盘 | M2 + M7 |
| FR-23 | 多视图独立路由 | M-01 视图装配 + M-03 view store | M6 |

### 5.4 验收节点（可演示里程碑）

| 节点 | 阶段 | 验收标准 |
| --- | --- | --- |
| **N1** 协议冻结 | M0 | contract crate 编译 + DTO roundtrip 序列化测试通过 |
| **N2** 数据底座可加载 | M1 | 拖 ELF 进项目 → 看到 product tree + 内存映射表 + ELF section 列表 |
| **N3** 单指令可执行 | M2 | `addi x1, x0, 42` 跑完能读 x1=42（已有 `addi_executes` 测试） |
| **N4** 全 RV32IMC 跑通 | M2 | Spike golden 对拍通过全部 71 条指令 |
| **N5** GPIO 翻转可见 | M3 + M4 | 智能灯固件跑起来，前端波形图看到 LED 引脚翻转 |
| **N6** 调试联动 | M5 + M6 | 断点停 → 看 Reg/Mem → 单步（StepOver/Out） → 反汇编 + 源码联动 |
| **N7** E2E-01 智能灯验收 | M9 | 智能灯 6 步全绿（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 行为判定） |
| **N8** MVP 完整交付 | M9 | 13 P0 全覆盖 + 性能 baseline ≥5 MIPS + 场景脚本可重放 |

### 5.5 第一刀建议（M1 数据底座，7 人日）

按关键路径顺序，**M0 已完成**。下一阶段 M1 建议分两步：

1. **M-14 总线 + M-01/02 产品树视图**（4 人日）— 先有数据底座，UI 后接
2. **M-06 ELF/Raw/HEX 解析**（3 人日）— 引入 `object` crate（BSD-2-Clause），解 PT_LOAD 段写进 bus；DWARF 留到 M5

不走「先扩 ISS」理由：M-13 扩到 RV32IMC 全集（12d）跟 M-14 弱依赖，可并行走；M-14 是后续所有「加载 → 跑」的物理基础，先打地基更稳。

### 5.6 E2E-01 智能灯固件（自研 · Q1 决议）

按 Q1 决定，自研约 200 行 RV32IMC 最小固件，1 个 GPIO 输入（按钮）+ 1 个 GPIO 输出（LED）+ main 循环（轮询按钮 → 翻转 LED）。工具链：`riscv64-unknown-elf-gcc` 编译为静态 ELF（无 libc，bare-metal）。

验收：在 M9 用 Spike 跑同一固件 + 在我们内核跑同一固件，比较 LED 引脚时序一致。

---

## 6. 风险与降级（按 AD §12）

| ID | 风险 | MVP 应对 | 降级方案 |
| --- | --- | --- | --- |
| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | **v0.3 决议：纳入 MVP（M5 阶段，10 人日）** | 失败时回退到「反汇编 + 符号表」，但 MVP 仍按完整 DWARF 排期 |
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
| 2026-10-19 | **Q1** 智能灯固件 = **自研**（不外取） | 用户答复 |
| 2026-10-19 | **Q2** ISS = **RV32IMC 全集**（I+M+C，含 16-bit 压缩指令） | 用户答复 |
| 2026-10-19 | **Q3** 持久化 = **IndexedDB**（用 `idb` 包装，BSD-3，~5KB） | 用户答复（接受 +0.3-0.5 人日换容量） |
| 2026-10-19 | **Q4** Worker URL = **同源**（HTTP 部署） | 用户答复 |
| 2026-10-19 | **Q5** 行为判定 = **canvas 比对**（虚拟 LED + 像素 diff） | 用户答复 |
| 2026-10-19 | **Q6** 性能基线硬件 = **macOS / Apple Silicon** | 用户答复 |
| 2026-10-19 | **Q7** Spike golden 对拍 = **必做**（FR-04） | 用户答复 |
| 2026-10-19 | **MVP 范围** = **选项 C**：§13.6 全量 90 人日（含 DWARF、CI、场景脚本、多板接口）；不降级 | 用户决议（v0.2 提议 B/C 后选 C） |

---

## 8. 已澄清问题（v0.2 全部答复）

| 编号 | 问题 | 答复 | 对实施的影响 |
| --- | --- | --- | --- |
| Q1 | 智能灯固件来源 | **自研** ~200 行 RV32IMC 最小固件（按钮 GPIO 输入 + LED GPIO 输出 + 简单延时循环） | M-15 必须有 GPIO 输入 + 输出；M-11 反汇编要看得懂它；M-06 ELF 解析要能装它 |
| Q2 | ISS 指令集范围 | **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集） | M-13 估 **12 人日**；C 扩展需 PC 步长 2/4 切换 + 16-bit 解码表 |
| Q3 | 持久化后端 | **IndexedDB**（用 `idb` 包装，~5KB BSD-3 库） | M-23 估 **3.5 人日**（含 `onupgradeneeded` schema 声明）；架构 §7.4 容量模型原样适用；M-05 CommandRouter 须 async 化 |
| Q4 | Worker URL | **同源 URL**（`./kernel-worker.js`，HTTP 部署） | M-07 简化（无 blob URL 内联代码）；MVP 假定 `python3 -m http.server 8080 --directory front` 或等价 HTTP 部署 |
| Q5 | 行为判定 | **canvas 比对**（虚拟 LED 渲染到 canvas + 像素 diff + 视觉对照） | M-01 加一个产品行为视图（虚拟 LED 灯泡 + 状态机动画）；可选自动化：每 N ms 截 canvas vs 期望图 |
| Q6 | 性能基线硬件 | **macOS**（Apple Silicon M3 / 评测机） | 性能验收在该硬件上实测 ≥5 MIPS 估算基线；CI runner 也是 macOS |
| Q7 | Spike golden 对拍 | **必做**（FR-04，每条已实现指令对拍） | `tests/golden/` 用 Spike 作为 oracle 跑同一程序，对比 `m13_iss` 的 register / memory 输出；CI 必跑；MVP 起步可只对 ADDI + 关键 LOAD/STORE/BRANCH |

### v0.2 锁定后的实施前置条件

- **依赖**：需要安装 `riscv64-unknown-elf-gcc` 工具链（编译自研固件） + `spike`（golden oracle），以及 npm 上没有依赖所以前端 `idb` 走 ESM CDN 或本地拷贝
- **环境变量**：可能需要 `SPIKE_BIN=/path/to/spike` 给 golden test 用
- **CI**：GitHub Actions macOS runner + Spike 安装步骤

---

## 9. 文档版本历史

| 版本 | 日期 | 变更 | 作者 |
| --- | --- | --- | --- |
| v0.1 | 2026-10-19 | 初稿：建立项目状态、§13.6 WBS 校准、关键路径、下一里程碑 E2E-01、决策与风险 | agent (mavis) |
| v0.2 | 2026-10-19 | 答复 Q1-Q7：自研固件 / RV32IMC 全集 / IndexedDB / 同源 Worker / canvas 比对 / macOS / Spike 必做；M-13 工作量上调至 12 人日；§5 step 2 同步更新；新增 §8 「已澄清问题」表与实施前置条件 | agent (mavis) |
| v0.3 | 2026-10-19 | MVP 范围从 B（30d, DWARF 留 P1）升级到 **C（§13.6 全量 90 人日）**；§5 重写为 10 阶段计划（M0-M9）+ 13 P0 覆盖矩阵 + 8 个验收节点 + 关键路径图；§6 R7 改「纳入 MVP」；M0 已完成，剩余 85 人日 | agent (mavis) |

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
