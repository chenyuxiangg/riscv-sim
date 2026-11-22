# STAGE-54 · v0.79 中断波形通道（冻结规格）

> 批次：2026-11-18 · 分支 `feature/irq-waveform-channels`
> 触发：用户指出「波形初值是 3」不是电平，而是**中断线号**；随后要求波形展示
> 软中断 / 定时器 / 所有外设中断，并支持前端筛选。

## 1 结论（三条拍板）

| # | 决定 | 依据 |
| --- | --- | --- |
| 1 | **保持现有中断概念空间**（CLINT+PLIC，L2 线 1/3/4，cause 3/7/11） | 上一轮已确认 PLIC 模型下 cause 与 PLIC 源号**不在同一空间**，合并会撞号 |
| 2 | 波形改为**每条中断一个 0/1 通道**，删掉「脉冲」通道 | 脉冲通道把**线号当值**画，表达不了「有/无」与「落」 |
| 3 | 前端支持筛选，**默认只勾核中断**；外设通道随平台动态出现 | 2026-11-18 用户明确 |

## 2 通道表

| 通道 | 流 id | label | 值 | 数据来源 |
| --- | --- | --- | --- | --- |
| software | `irq.core.software` | `software · IRQ 3` | 0/1 | M-16 `request/clear`（CLINT `msip`） |
| timer | `irq.core.timer` | `timer · IRQ 7` | 0/1 | M-16 `poll` 抬线 / `set_timer_compare` 撤线 |
| external | `irq.core.external` | `external · MEIP` | 0/1 | 宿主 `sync_interrupts`（PLIC 有/无可交付源） |
| 设备 N | `irq.dev.{id}` | `{id} · IRQ {source}` | 0/1 | 宿主 `set_source_level(src, level)` 处同步采样 |

⚠️ **external 的标签不写 `IRQ 11`**：cause 11 与某台设备的 PLIC 源 11 同数字
（tpl-soc 的 GPIO0 就是源 11），写 `MEIP` 才是无歧义的名字。

## 3 通道号（内部键，UI 不解析）

| ch | 用途 |
| --- | --- |
| 0..4 | M-18 表 68 既有通道（`TIM0.CMP` / `GPIO0.OUT` / `PULSE`(空闲) / `KEY0.IN` / `UART0.TX`） |
| 16 / 17 / 18 | software / timer / external |
| 19 + 声明序号 | 设备通道 |

⚠️ 为什么不用 cause / 源号当 `ch`：二者**会撞**（11/11、3/3）；且 `ch` 是 `u8`，
PLIC 源号按规范可到 1023。**对外身份是流 id 与 label，不是这个数字。**

## 4 采样：事件驱动，不是每拍轮询

单比特通道只在 0↔1 变化时产生边沿 ⇒ 重复请求不产生额外边沿。

| 事件 | 写入 |
| --- | --- |
| M-16 `request()` 真的登记了一条挂起 | 该线通道写 1 |
| M-16 `clear()`（含 timer 撤线、软中断落、外部无可交付源） | 该线通道写 0 |
| 宿主 `Plic::set_source_level(src, level)` | 该设备通道写 `level` |

⚠️ 设备通道画的是**网关电平**（设备有没有在请求 PLIC），`external` 画的是
**核视角**。PLIC 源落下不 retract 已送进 PLIC 的请求（Renode 口径），两条通道
**允许**不同步。

## 5 动态适应

- 通道表在 `install_probe_pipeline()` 里按 `peripheral_irqs` **现算**；
- `rebuild_platform()` ⇒ `map_memory`（重编译 `peripheral_irqs`）⇒
  `install_probe_pipeline` ⇒ 产品视图加/删器件后波形自动多/少一条；
- 前端筛选状态按**流 id** 保存（`mergeIrqSelection`）⇒ 增删器件不丢勾选。

## 6 前端

| 元素 | 行为 |
| --- | --- |
| 分组 | `核中断` / `外设中断`（按流 id 前缀，不按 label 文本） |
| 复选框 | 每通道一项，显示 `N edges` |
| 快捷 | `全选` / `仅核` / `清空` |
| 默认 | 核 3 条勾选；外设全部不勾；非中断通道（`GPIO0.OUT`）保持可见 |
| 重画 | 勾选变化只重算 ECharts option，不重新 dump；自报属性同步刷新 |

## 7 删除项

- `CH_IRQ_PULSE`、`ProbeRegistry::sample_irq_pulse`、`IRQ_PULSE_*` 常量；
- 原先靠「脉冲」验证的两条用例改为**电平语义**（`single_bit` 去重、值必须为 1）。

## 8 测试

| 层 | 用例 |
| --- | --- |
| M-18 | `pulse_channel_emits_every_event`（改测试专用通道）+ `single_bit_level_channel_coalesces_same_value` |
| M-16 → M-18 | 一次 `request()` ⇒ 恰好一条 0→1 边沿，值为 **1**（不是线号） |
| D-46 | 通道 id 集合含 3 条核中断 + 每台设备；设备 label 必须是 `名字 · IRQ 号` |
| 前端 | 分组 / 默认只勾核 / 合并保留选择 / 筛选区 HTML |

## 9 文档同步（已补齐）

| 文档 | 处置 |
| --- | --- |
| 本规格（HTML 版） | `STAGE-54_V079_IRQ_Waveform_Channels.html`，由 `tools/build_stage54_doc.py` 数据驱动生成（13 表 + SVG 流程图 + 移动端视图） |
| `M-16_InterruptController.html` | 版本行升 v1.8 + 第 1 章前插入 v0.79 修订说明（脉冲通道退役、`pulse_sink` 抬线 / `clear_sink` 撤线） |
| `M-18_Probe.html` | 版本行升 v1.4 + 第 1 章前插入 v0.79 修订说明（18-2 脉冲通道退役、改单比特电平通道） |
| `STAGE-4_Interrupt_Controller.md` | 台账上方加 v0.79 变更段；D-26 行标注「v0.79 起已退役」；顺带修一处未闭合的反引号 |

**口径**：旧文档正文保留原文作为历史记录，只在开头加「已被取代」的说明 —— 不当成事实来源，
也不做无意义的全文重写。

**云端归档**：`https://my.feishu.cn/file/FMpJb2mKFoiAzLxPSZdcNAFQn3P`（file_token `FMpJb2mKFoiAzLxPSZdcNAFQn3P`，size 26727，version `7709177917162392824`）。

## 10 剩余欠账

- 门禁工具改造 R-1~R-4（分段重跑 / 失败即 dump / 采集通道 health check / wasm mtime 前置）仍欠；
  - ⚠️ **订正（2026-11-22 逐条核对代码）**：本条**已过时** —— **R-1~R-4 已于 v0.80 落地**、**R-7 / R-8** 亦已落地；
    真余项只剩 **§4 总验收演练** + **R-5**（`--trace`）+ **R-6**（clippy 前移）。原文保留作历史记录，
    现口径见 [`../plans/GATE_TOOLING_PREREQ.md`](../plans/GATE_TOOLING_PREREQ.md) §6；
- 器件级二级通道（UART RX/TX 分开）本批明确不做。
