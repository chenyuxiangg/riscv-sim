# STAGE-36 · v0.51 · 波形真图（echarts）

> **状态**：冻结（施工前）
> **批次**：第 35 批（v0.51）· UI 改版三批里的**第 3 批**
> **来源**：用户拍板的《riscv-sim-UI 建议原型》第三段 —— 波形视图要有**真图**，不是通道列表
> **前置**：STAGE-34（v0.49 · 骨架与视觉）· STAGE-35（v0.50 · 视图内容形态 + 断点 UI）
> **台账**：见 [`docs/reviews/REVIEWS.md`](../reviews/REVIEWS.md) §2.1 第 34 批块「下一批候选」

---

## §0 概述

v0.49 批把骨架与视觉换掉，v0.50 批把视图内部的内容形态换掉并给断点补上页面入口。**本批是三批里的最后一批**：把波形视图从「通道列表」升级成「**真图**」。

⚠️ **本批与前两批最大的不同**：它**第一次引入一个外部依赖**（echarts）。⇒ ⇒ 这不是「顺手加个库」，而是**一个必须显式登记的架构破口**（台账 v0.49 批已登记：**用户拍板引入 echarts 画波形** ⇒ ⇒ **破 ADR-6 / NFR-05 的「零依赖」** ⇒ ⇒ 落在 **v0.51 批** ⇒ ⇒ 破口必须是显式的）。

⚠️ **但实测把那条登记的措辞修正了一半**（详见 §1.2 事实 F11 与 §8 订正 1）：**NFR-05 原文里根本没有「零依赖」三个字** —— 它讲的是「**零安装**（用户侧无构建/安装步骤）」。⇒ ⇒ **真正要改的只有 ADR-6 与架构 §11.2** ⇒ ⇒ **NFR-05 一个字都不用动**。

⇒ ⇒ **本批的破口清单（很小，要说清楚）**：

| # | 破的是什么 | 怎么补 | 落点 |
| --- | --- | --- | --- |
| 1 | ADR-6 §9.6：「**零依赖在 MVP 保持**」 | 改成「零依赖在 MVP 保持**除波形图外**；波形图按 **P1 同款受控变体**（可信 CDN + 能力协商降级）承载」 | `docs/architecture/RISC-V_Architecture_Design.html` §9.6 |
| 2 | 架构 §11.2：「演进**不引入外部依赖**」 | 改成「演进**默认**不引入外部依赖；**波形图**是已登记的受控破口（CDN + 降级）」 | 同上 §11.2 |
| 3 | 需求文档 NFR-05 | ⛔ **不改**（它讲的是零安装，不是零依赖）⇒ ⇒ **如实登记这条订正** | `docs/requirements/RISC-V_Requirements_v1.4.html` |

⚠️ **「零构建」仍然成立** —— 本批不加任何打包步骤，echarts 直接以浏览器原生 ES module 动态 `import()` 加载。

---

## §1 目标与边界

### §1.1 本批要达成的

1. **波形视图有真图** —— 用 echarts 画**阶梯波形**，x 轴是虚拟时间、y 轴是信号值；多通道多 series。
2. **数据源先到位** —— 内核**早就有**完整的边沿样本（`Edge { t_ps, value }`），但前端**只留计数、丢掉样本** ⇒ ⇒ 本批第一步是**留住样本**。
3. **优雅降级** —— echarts 不可达 ⇒ ⇒ 显示「**波形库不可用**」+ **保留通道列表**（不白屏）。
4. **降级路径永远可测** —— 不靠「碰巧没网」⇒ ⇒ 波形库来源**可覆盖**⇒ ⇒ 真机门禁**主动**触发降级。
5. **破口显式化** —— ADR-6 / §11.2 措辞订正 + README 已知缺口 + 用户指南已知限制。
6. **缩放 / 平移** —— FR-10 要求 ⇒ ⇒ echarts `dataZoom`。

### §1.2 事实表（实测 · F1~F16）

| 编号 | 事实 | 证据 |
| --- | --- | --- |
| F1 | 前端波形视图**只拿到「通道名 + 边沿计数 + 末值」** —— `front/app.js` 的 `wireWaveform` 把内核推来的 `WaveformHistory.streams[].edges`（每条 `{t_ps, value}`）**只**用来累加计数与取末值，**边沿样本本身被丢掉** | `front/app.js` 的 `wireWaveform`（`entry.edges += edges.length` / `entry.lastValue = edges[edges.length - 1].value`） |
| F2 | 契约层**早就有完整的边沿样本** —— `model::WaveformHistory { streams: Vec<WaveformStream> }` → `WaveformStream { id, label, edges: Vec<Edge> }` → `Edge { t_ps: u64, value: i64 }` ⇒ ⇒ **零新 DTO / 零新命令 / 零新事件** | `crates/contract/src/model.rs:423 / 432 / 461` |
| F3 | `t_ps` 在线上是**十进制字符串**（契约 v2 §2.2）⇒ ⇒ 前端必须走 `front/lib/message.js` 的 `psToNumber()` 读回 ⇒ ⇒ **不许 `Number(str)` 硬转**（`psToNumber` 对非十进制整数字符串与 > 2^53 都**抛**） | `crates/contract/src/numstr.rs` + `front/lib/message.js:403` |
| F4 | 波形推送的**频率 = 每轮运行后一次**（宿主 `waveform_event(false)` 在 `step_n` / `continue` 每轮末尾调用）⇒ ⇒ 不是每条指令一次 ⇒ ⇒ 但**每步一次** ⇒ ⇒ 图**不能每次重建**（会闪）⇒ ⇒ 需要**持久图表宿主** | `crates/kernel/src/kernel_host/mod.rs:1752` |
| F5 | 内核侧每通道环容量 = `RING_CAP`（M-19 §1.1：2×10⁵ 事件）⇒ ⇒ 前端的样本上限**必然小于**内核 ⇒ ⇒ 「图上的点数」与「边沿总数」**天然不同** ⇒ ⇒ 界面上**必须分开标注** | `crates/kernel/src/m19_waveform/ring.rs:21` |
| F6 | 宿主对**单个事件**的边沿总量有上限（`MAX_WAVEFORM_EDGES_PER_EVENT`）⇒ ⇒ 截断**保留最新**的边沿 ⇒ ⇒ 前端拿到的每批边沿**本身就是「最近的」** ⇒ ⇒ 前端再截断时也保留最新的 ⇒ ⇒ 语义一致 | `crates/kernel/src/kernel_host/mod.rs:1767` |
| F7 | 现有真机判据 `waveformChecks` 断言的是**通道名**与 **`\b1 edges\b` 文本** ⇒ ⇒ 「N edges」这段文本是**稳定钩子** ⇒ ⇒ 本批**一个字不动** ⇒ ⇒ 真图是**另一张卡片** | `tools/browser-verify.mjs` 的 `waveformChecks` |
| F8 | `m02` 的渲染模型 = 每次 `body.innerHTML = html` ⇒ ⇒ 视图里的 DOM **每次都被重建** ⇒ ⇒ echarts 实例挂在容器元素上 ⇒ ⇒ 要不闪就得**把宿主节点搬过去** ⇒ ⇒ `front/tests/dom-stub.mjs` **已有** `parentNode` / `appendChild` / `removeChild` ⇒ ⇒ 替身够用 ⇒ ⇒ **不扩展 dom-stub** | `front/views/m02_viewcomponents.js` 的 `applyRenderEvent` + `front/tests/dom-stub.mjs:267` |
| F9 | `ViewComponent` 目前只有 `render` / `onCommand` / `onDelta` / `mountFor` ⇒ ⇒ 图表需要在渲染**之后**拿容器 ⇒ ⇒ 要加一个**可选**的 `afterRender(body, state)` 钩子 ⇒ ⇒ 只影响声明它的视图 ⇒ ⇒ 既有视图**零改动** | `front/views/m02_viewcomponents.js:71` |
| F10 | **ADR-6 原文**（架构 §9.6）：「**零依赖在 MVP 保持、P1 允许 CDN + 降级**」；「P1 3D = 方案 C 的受控变体：three.js 走可信 CDN，离线环境 3D 视图降级为 2D/树形（能力协商降级，§11.2）」⇒ ⇒ **波形图走 CDN + 降级 = 架构里已有的「受控变体」** ⇒ ⇒ 本批是**提前动用**那条，不是新开一个破口 | `docs/architecture/RISC-V_Architecture_Design.html` §9.6 |
| F11 | ⚠️ **NFR-05 原文里没有「零依赖」** —— 需求文档 §4.2 的 NFR-05 是「现代 Chrome/Edge/Safari/Firefox；移动端 375px 可读、正文 ≥14px；**零安装**（用户侧无构建/安装步骤）；**同源 HTTP 部署直接打开可用**（目录形态，ES module 与 Worker 均要求同源）」⇒ ⇒ **它讲的是「零安装」，不是「零依赖」** ⇒ ⇒ **NFR-05 一个字都不用改** ⇒ ⇒ 台账 v0.49 批写的「破 ADR-6 / NFR-05 的『零依赖』」**这个说法本身是错的**（实测证伪） | `docs/requirements/RISC-V_Requirements_v1.4.html` §4.2 的 NFR-05 行 |
| F12 | echarts 的 ESM 产物在 jsdelivr 上**可达**且带 `access-control-allow-origin: *` ⇒ ⇒ 浏览器 `import()` 跨源可用 ⇒ ⇒ 产物 **1 030 623 字节** | `curl -I https://cdn.jsdelivr.net/npm/echarts@5.5.1/dist/echarts.esm.min.js` |
| F13 | `formatVirtualTime` 现在住在 `front/app.js`（app 层）⇒ ⇒ 视图层要格式化时间轴**不能 import app.js**（app.js import m01/m02 ⇒ ⇒ 循环依赖）⇒ ⇒ 时间格式化要**搬到一个共享落点** | `front/app.js:201` + `front/app.js:17-19` |
| F14 | 降级路径**必须可达**：CDN 不可达时「波形库不可用」⇒ ⇒ 真机门禁要**能主动触发降级** ⇒ ⇒ 不能只靠「碰巧没网」⇒ ⇒ 需要一个**可覆盖的波形库来源** | 本批的设计要求（F14 是**约束**，不是现状） |
| F15 | M-19 §4.19 说波形是**阶梯**（staircase，由边沿回放得出）⇒ ⇒ 图要用**阶梯线** ⇒ ⇒ 不是斜线 | `crates/contract/src/model.rs:414` 的注释 |
| F16 | FR-10（需求 §4.3 视图清单）要求波形视图「探针通道渲染；**缩放/平移**；导出 CSV/VCD」⇒ ⇒ 缩放/平移 = echarts `dataZoom`；导出动作**现在只在调试视图**（`export-trace` 命令，源 = Waveform）⇒ ⇒ 本批**只做真图** ⇒ ⇒ 导出按钮的落点差异**如实登记** | `docs/requirements/RISC-V_Requirements_v1.4.html` §4.3 + `front/app.js` 的 `case "export-trace"` |

### §1.3 本批**不做**

| 不做 | 为什么 |
| --- | --- |
| ⛔ **改需求文档 NFR-05** | 实测它讲的是「零安装」，不是「零依赖」（F11）⇒ ⇒ 改它反而是**改错的东西** |
| ⛔ **把 echarts vendor 进仓** | 那会让「离线不可用」这个**已知缺口**不存在 ⇒ ⇒ 降级路径变成**装饰**（永远走不到）⇒ ⇒ 用户拍板的语义是「离线 / CDN 不可达时优雅降级」 |
| ⛔ **改通道列表的任何文本** | 「N edges」是稳定钩子（F7）⇒ ⇒ 既有真机判据一字不改 |
| ⛔ **加波形导出按钮** | FR-10 把导出列在波形视图下，但导出动作**现在就在调试视图**（F16）⇒ ⇒ 本批只做真图 ⇒ ⇒ 落点差异**如实登记**（§4） |
| ⛔ **新契约命令 / DTO / 事件** | 边沿样本**早就有**（F2）⇒ ⇒ 契约层零升版 |
| ⛔ **在 UI 层做地址/时间校验** | 留契约层 |
| ⛔ **改内核 / 宿主** | 本批**只改前端 + 两处文档措辞** |

---

## §2 裁决

| 编号 | 裁决 | 理由 |
| --- | --- | --- |
| A | **稳定钩子一字不动**（STAGE-35 裁决 A 的延续）⇒ ⇒ 「N edges」文本、`.channel-list > li` 结构、`data-view-body="waveform"`、`data-view-tab` 等**全部保留** ⇒ ⇒ **真图是新增卡片**，不改通道列表 ⇒ ⇒ 既有真机判据（`waveLabel0 === "GPIO0.OUT"`、`/\b1 edges\b/`）**一字不改** | F7 |
| B | **边沿样本留住 + 有上限** ⇒ ⇒ 落点 = 视图状态 `wave.channels[i].samples`（每通道**最近 N 条**）⇒ ⇒ **上限 = 512**，超出丢**最旧**的 ⇒ ⇒ **边沿计数 `edges` 继续累加总数**（**不受**上限影响）⇒ ⇒ ⇒ **总数与显示条数是两个数，界面上分开标注**（裁决 O 的 series 名） | F5 + F6 |
| C | **边沿累加剥成纯函数** ⇒ ⇒ `front/lib/wave-samples.js` 的 `mergeEdges(prev, stream, limit)` ⇒ ⇒ 只读 `prev` + 只写返回值 ⇒ ⇒ **Node 侧零 DOM** 就能测 ⇒ ⇒ 与 v0.31 批「差量合成剥成纯函数」同形 | v0.31 批先例 |
| D | **`t_ps` 必须走 `psToNumber`** ⇒ ⇒ 非法形态**丢弃那一条边沿**并继续（**不让整条流崩掉**）⇒ ⇒ ⛔ **不许** `Number(str)` 硬转 ⇒ ⇒ 被丢弃的那条**仍计入 `edges` 总数**（那是内核口径）⇒ ⇒ ⇒ **计数与样本是两个数** | F3 |
| E | **echarts 走可信 CDN**（ADR-6 的 P1 受控变体）⇒ ⇒ URL **唯一字面量**住 `front/lib/echarts-loader.js` ⇒ ⇒ 浏览器原生**动态 `import()`** ⇒ ⇒ ⛔ **不加打包步骤**（「零构建」仍然成立）⇒ ⇒ 离线时 catch ⇒ ⇒ 降级 | F10 + F12 |
| F | **加载失败不抛**，返回 `null` ⇒ ⇒ 界面显示「**波形库不可用**」⇒ ⇒ **通道列表仍在**（不白屏）⇒ ⇒ 降级状态**必须可见**（不是静默） | F14 |
| G | **波形库来源可覆盖** ⇒ ⇒ `setChartSource("off" \| url)` ⇒ ⇒ **脚本口暴露** ⇒ ⇒ 真机门禁**主动**触发降级 ⇒ ⇒ ⇒ **降级路径永远有判别力**（不靠「碰巧没网」） | F14 |
| H | **图表状态住视图状态** `wave.chart = { status, reason? }`，`status ∈ {"idle","loading","ok","degraded"}` ⇒ ⇒ 视图层**纯渲染**（按 status 分支）⇒ ⇒ Node 侧可测降级文案 ⇒ ⇒ app 层**写**、视图层**只读** | STAGE-35 裁决 L/K 的同形 |
| I | **图表 option 构造剥成纯函数** ⇒ ⇒ `front/views/components/wave-chart.js` 的 `buildWaveOption(channels)` ⇒ ⇒ Node 侧可测「画的是不是真数据」⇒ ⇒ 与裁决 C 同形 | v0.31 批先例 |
| J | **x 轴是 value 轴**（虚拟时间 ps），⛔ **不是 category 轴** ⇒ ⇒ 否则时间等距 ⇒ ⇒ 时序失真 | F2 |
| K | **阶梯线**（`step: "end"`）⇒ ⇒ 与 M-19 §4.19 的 staircase 语义一致 ⇒ ⇒ **不是**斜线 | F15 |
| L | **持久图表宿主** ⇒ ⇒ 视图组件对象上持有**一个** `<div>`（`waveformView._chartHost`）⇒ ⇒ 每次渲染把它**搬**进新插槽（`appendChild` + `removeChild`）⇒ ⇒ ⇒ echarts 实例**不重建**（不闪）⇒ ⇒ ⚠️ 它是**渲染资源**，不是用户状态 ⇒ ⇒ 不住视图状态 | F4 + F8 |
| M | **`afterRender` 是**可选**钩子** ⇒ ⇒ `applyRenderEvent` 在 `body.innerHTML = html` **之后**调用它（若视图声明了）⇒ ⇒ **只影响声明它的视图** ⇒ ⇒ 既有视图**零改动** ⇒ ⇒ 零回归 | F9 |
| N | **契约层零升版** ⇒ ⇒ `SCHEMA_VERSION` 仍 **8** / `SIM_STATE_SCHEMA_VERSION` 仍 **2** / `PLATFORM_SCHEMA_VERSION` 仍 **1** / `PLATFORM_RULES_VERSION` 仍 **3** ⇒ ⇒ **零**新 DTO / **零**新命令 / **零**新事件 ⇒ ⇒ 前端镜像**不用改版本号** | F2 |
| O | **ADR-6 与 §11.2 措辞订正 + NFR-05 不改** ⇒ ⇒ 只改「零依赖在 MVP 保持」那一句 + 「演进不引入外部依赖」那一条 ⇒ ⇒ ⛔ **NFR-05 一个字都不动** ⇒ ⇒ ⇒ 并把这条订正**如实登记**（F11） | F10 + F11 |

---

## §3 验收

### §3.1 A 组（前端门禁 · 新增 `front/tests/wave-chart.test.mjs`）

| 编号 | 判据 |
| --- | --- |
| A-1 | 纯函数 `mergeEdges(null, stream)` ⇒ ⇒ 返回 `{id, label, edges: N, samples: N 条}`，每条样本是 `{t, v}`（`t` 是**数字**） |
| A-2 | `mergeEdges(prev, stream)` **累加** ⇒ ⇒ `edges` 是**总数**、`samples.length` 是 `min(总数, 512)` ⇒ ⇒ ⇒ **两者独立** |
| A-3 | **上限生效** ⇒ ⇒ 喂 600 条 ⇒ ⇒ `samples.length === 512` 且**保留的是最近 512 条**（第一条 = 原第 89 条） |
| A-4 | **`t_ps` 走 `psToNumber`** ⇒ ⇒ 喂一条非法 `t_ps`（`"1e+21"`）⇒ ⇒ 那条**被丢弃**（不进 samples）⇒ ⇒ 但 `edges` 计数**仍然 +1**（内核口径）⇒ ⇒ 其它边沿**不受影响** |
| A-5 | `buildWaveOption` 的 series 数据**逐条**等于喂进去的样本 ⇒ ⇒ `series[0].data` 逐条 `[[t, v], ...]` 相等（**不是**只比长度） |
| A-6 | **多通道** ⇒ ⇒ series 数量与顺序 = 通道数量与顺序 ⇒ ⇒ 每个 series 的数据 = 对应通道的样本 |
| A-7 | **x 轴是 value 轴** ⇒ ⇒ `option.xAxis.type === "value"`（⛔ 不是 `"category"`） |
| A-8 | **阶梯线** ⇒ ⇒ 每个 series 的 `step === "end"` |
| A-9 | **空通道** ⇒ ⇒ `buildWaveOption([])` 返回 `series: []` 的 option，**不抛** |
| A-10 | **降级文案** ⇒ ⇒ 喂 `status = "degraded"` ⇒ ⇒ 渲染结果含「**波形库不可用**」⇒ ⇒ 喂 `status = "ok"` ⇒ ⇒ **不含**那四个字 |
| A-11 | **加载失败不抛** ⇒ ⇒ `loadECharts()` 在**加载不了**时返回 `null` 而不是抛 ⇒ ⇒ Node 里 `import("https://…")` 必然失败 ⇒ ⇒ 这条**在 Node 里天然可测** |
| A-12 | **URL 唯一落点** ⇒ ⇒ 全文扫描 `front/lib/echarts-loader.js` ⇒ ⇒ 那个 CDN URL 字面量**恰好出现一次** ⇒ ⇒ 防止「第二个来源」 |
| A-13 | **`setChartSource("off")`** ⇒ ⇒ `loadECharts()` 返回 `null`（**不发网络请求**）⇒ ⇒ `getChartSource()` 回读是 `"off"` ⇒ ⇒ 可**改回**默认值 |
| A-14 | **稳定钩子未动** ⇒ ⇒ 波形视图的 `render(state)` 输出里，通道列表仍是 `<li>` + `N edges` 文本 ⇒ ⇒ ⛔ **不含**任何新加的计数后缀 |
| A-15 | **真图是另一张卡片** ⇒ ⇒ 输出里有 `[data-wave-chart-slot]` ⇒ ⇒ 且它**不在** `.channel-list` 里 |
| A-16 | **显示条数与总数分开标注** ⇒ ⇒ 每个 series 的 `name` 里带**两个数**（`显示条数/总数`）⇒ ⇒ 例如 `GPIO0.OUT（3/3）` ⇒ ⇒ 若只显示一个数 ⇒ ⇒ 红 |

### §3.2 B 组（真机门禁 · 新增 `waveChart` 段 · B-27 ~ B-32）

⚠️ **B 组编号接着 STAGE-35 的 B-26 往后排** —— B 组落在**同一个真机门禁输出**里 ⇒ ⇒ **编号空间跨批次共享**（查占用表要像查错误码那样查）。

| 编号 | 判据 | 依赖网络？ |
| --- | --- | --- |
| B-27 | 波形图卡片**存在**（`[data-wave-chart]` 在波形视图里）⇒ ⇒ 且**不在** `.channel-list` 里 | 否 |
| B-28 | echarts **真的画出来了** ⇒ ⇒ 容器上有 `_echarts_instance_` 属性（**echarts 自己写的**，⇒ ⇒ 不是界面自报）⇒ ⇒ 且容器里有 `<canvas>` | **是** |
| B-29 | 图上的**数据点条数** = 该通道**保留的样本数**（⛔ **不是**边沿总数）⇒ ⇒ 界面自报的 `data-wave-chart-points` 与**视图状态里的 samples 长度**一致 ⇒ ⇒ ⇒ 若 option 传错字段 ⇒ ⇒ 两边不等 ⇒ ⇒ 红 | **是** |
| B-30 | **降级路径**（脚本口把波形库来源设成 `"off"`）⇒ ⇒ 波形卡片显示「**波形库不可用**」⇒ ⇒ **且通道列表仍在**（不白屏）⇒ ⇒ ⇒ **不依赖网络** ⇒ ⇒ **永远可跑** | 否 |
| B-31 | **降级可恢复** ⇒ ⇒ 把来源**设回默认** ⇒ ⇒ 图**又画出来了** ⇒ ⇒ 「关掉再打开」不是单向 ⇒ ⇒ 证明降级不是死路 | **是** |
| B-32 | **稳定钩子仍在** ⇒ ⇒ `.channel-list > li` 在、`GPIO0.OUT` 在、`N edges` 文本在 ⇒ ⇒ 既有判据没被本批改坏 | 否 |

⚠️ **B-28 / B-29 / B-31 依赖网络** ⇒ ⇒ **波形库不可达时这三条必须明确 SKIP（不是判负）** ⇒ ⇒ 且要在输出里**写明「本条未跑」** ⇒ ⇒ ⛔ **不许**静默跳过（静默跳过 = 判别力为零且没人知道）。

⚠️ **B-30 是本批的降级判别力主体** ⇒ ⇒ 它**不依赖网络** ⇒ ⇒ ⇒ **降级逻辑永远有人管**。

### §3.3 C 组（交叉）

| 编号 | 判据 |
| --- | --- |
| C-1 | **ADR-6 已改** ⇒ ⇒ 架构文档 §9.6 的 ADR-6 段落**含**「波形」与「降级」⇒ ⇒ **NFR-05 未改** ⇒ ⇒ 需求文档的 NFR-05 段落**不含**「echarts」 |
| C-2 | 台账必须登记本批（`v0.51` + `STAGE-36`） |
| C-3 | 冻结规格必须含 §8 施工期订正，且**每条订正自己那段里**都有「已改成」 |
| C-4 | `front/lib/echarts-loader.js` 必须存在，且导出 `loadECharts` / `setChartSource` / `getChartSource` |
| C-5 | **前端镜像零升版** ⇒ ⇒ `front/lib/message.js` 的 `SCHEMA_VERSION` 仍 **8** |

---

## §4 台账与文档边界

### §4.1 台账

- **销账 0 条、新登记 0 条** —— 本批是**能力增强 + 一条已登记架构取舍的落地**，不是欠账闭环 ⇒ ⇒ 「**真实未闭环**」**62 → 62 不变** ⇒ ⇒ §0.1 合计那三个数 **538 / 402 / 139** **全部不变** ⇒ ⇒ §1.3 标记区 **34 条不变**。
- **已知边界（登记，不销账、不新登记）**：
  - ⚠️ **离线 / CDN 不可达时波形视图没有真图**（只有通道列表 + 「波形库不可用」）⇒ ⇒ 这是**用户拍板的取舍**，写进 README「已知缺口」与用户指南「已知限制」。
  - ⚠️ **图上只显示每通道最近 512 条边沿**，而「N edges」是**总数** ⇒ ⇒ 两个数在界面上**分开标注**（裁决 B / O）。
  - ⚠️ **波形导出动作只在调试视图**（FR-10 把导出列在波形视图下）⇒ ⇒ 本批只做真图 ⇒ ⇒ 落点差异**如实登记**（F16）。

### §4.2 文档

| 文档 | 改什么 |
| --- | --- |
| `docs/architecture/RISC-V_Architecture_Design.html` | **§9.6 ADR-6** 的决策段 + **§11.2** 兼容性策略那一条（裁决 O） |
| `docs/requirements/RISC-V_Requirements_v1.4.html` | ⛔ **一个字都不改**（F11）⇒ ⇒ 但在**台账**里如实登记这条订正 |
| `README.md` | 能力快照表 + 规模表 + 门禁表 + **已知缺口**（离线波形不可用）+ 批次叙事 + 文档索引 |
| `AGENTS.md` | 详细设计清单加 STAGE-36 + 工具清单 + 前端用例数 + 目录树篇数 + `browser-verify` 段数 |
| `docs/progress/PROGRESS.md` | 文档版本 + 变更摘要（五节）+ §9 版本历史表 |
| `docs/USER_GUIDE.md` | §3.5 波形（真图 + 降级）+ §8 已知限制 + §9 规格份数 |
| `docs/reviews/REVIEWS.md` | §0.1 前端两行 + §0.1 重算段 + §2.1 第 35 批块 |

---

## §5 版本与契约面

| 常量 | 值 | 本批是否变 |
| --- | --- | --- |
| `SCHEMA_VERSION` | **8** | ⛔ 不变 |
| `SIM_STATE_SCHEMA_VERSION` | **2** | ⛔ 不变 |
| `PLATFORM_SCHEMA_VERSION` | **1** | ⛔ 不变 |
| `PLATFORM_RULES_VERSION` | **3** | ⛔ 不变 |
| 前端镜像 `SCHEMA_VERSION` | **8** | ⛔ 不变 |

⇒ ⇒ **零**新 DTO · **零**新命令 · **零**新事件 ⇒ ⇒ **内核 / 宿主零改动** ⇒ ⇒ **前端零升版** ⇒ ⇒ **不制造 D-143 的存量问题**。

---

## §5.1 本批新增的**接口契约**（并行分工的**逐字**依据）

> ⚠️ **这些签名是「一个约束一个落点」的钉子** —— 各 lane **按本节实现**，不许各自发挥。

### §5.1.1 `front/lib/wave-samples.js`（新 · 纯函数 · 零 import）

```js
/** 每通道保留的边沿样本上限。 */
export const WAVE_SAMPLE_LIMIT = 512;

/**
 * 把内核推来的一批边沿并进一个通道条目。
 * @param {{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}|null} prev
 * @param {{id:string,label:string,edges:Array<{t_ps:string|number,value:number}>}} stream
 * @param {number} [limit]
 * @returns {{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}}
 */
export function mergeEdges(prev, stream, limit = WAVE_SAMPLE_LIMIT);

/**
 * 从通道条目里取出「可画图」的通道（至少有一条样本的）。
 * @param {Array<{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}>} channels
 * @returns {Array<{id:string,label:string,edges:number,samples:Array<{t:number,v:number}>}>}
 */
export function chartableChannels(channels);
```

### §5.1.2 `front/lib/echarts-loader.js`（新 · 加载器）

```js
/** echarts 的**唯一**来源字面量（全仓只此一处）。 */
export const ECHARTS_URL = "https://cdn.jsdelivr.net/npm/echarts@5.5.1/dist/echarts.esm.min.js";

/** 关闭波形库的哨兵值。 */
export const ECHARTS_SOURCE_OFF = "off";

/** @returns {string} 当前来源（默认 = `ECHARTS_URL`）。 */
export function getChartSource();

/**
 * 换波形库来源（`"off"` = 关掉；给 URL = 换源）。
 * @param {string} next
 * @returns {string} 换之后的来源
 */
export function setChartSource(next);

/**
 * 加载 echarts。
 * ⚠️ **失败不抛** ⇒ ⇒ 返回 `null`（⇒ ⇒ 界面走降级）。
 * @returns {Promise<object|null>}
 */
export function loadECharts();

/**
 * 已加载的 echarts 模块（同步读；没加载好是 `null`）。
 * @returns {object|null}
 */
export function getECharts();
```

### §5.1.3 `front/views/components/wave-chart.js`（新 · 纯函数 + 渲染片段）

```js
/** 通道条目 → echarts option。⚠️ **纯函数**：不碰 DOM、不读全局。 */
export function buildWaveOption(channels);

/**
 * 波形图卡片的 HTML 片段（按图表状态分支）。
 * @param {{status?:string, channels?:Array, limit?:number}} chart
 * @returns {string}
 */
export function waveChartHtml(chart);

/** 降级提示的文案（**唯一落点**）。 */
export const WAVE_CHART_DEGRADED_TEXT = "波形库不可用";
```

⚠️ `waveChartHtml` 的输出必须含：
- `status === "degraded"` ⇒ ⇒ 含 **`WAVE_CHART_DEGRADED_TEXT`** 与 `data-wave-chart-status="degraded"`；
- 其它状态 ⇒ ⇒ 含 **`data-wave-chart-slot`**（图表宿主插槽）。

### §5.1.4 视图状态形状（app 层写 · 视图层只读）

```js
// 视图状态 "waveform" ⇒ wave
{
  channels: [
    { id: "GPIO0.OUT", label: "GPIO0.OUT", edges: 3, samples: [ {t: 0, v: 0}, ... ] }
  ],
  chart: { status: "idle" | "loading" | "ok" | "degraded", reason?: string }
}
```

### §5.1.5 `front/views/m02_viewcomponents.js` 的两处改动

1. `applyRenderEvent`：`body.innerHTML = html;` **之后**⇒ ⇒ `if (typeof c.afterRender === "function") c.afterRender(body, detail.state ?? {});`
2. `waveformView` 新增：
   - `_chartHost`（持久 `<div>`，裁决 L）
   - `afterRender(body, state)`：把 `_chartHost` **搬**进 `[data-wave-chart-slot]`；若 `getECharts()` 非 `null` ⇒ ⇒ `echarts.init(host)` + `setOption(buildWaveOption(channels))`；并写 `data-wave-chart-points` 与 `data-wave-chart-status` 两个**自报**属性。

### §5.1.6 脚本口（`front/app.js`）

```js
window.riscv_waveform = {
    /** 当前波形库来源。 */
    chartSource: () => getChartSource(),
    /**
     * 换波形库来源（`"off"` = 关掉波形图；给 URL = 换源）。
     * @param {string} next
     */
    setChartSource: (next) => { /* setChartSource(next) + 重新加载 + 更新状态 */ },
};
```

---

## §6 判别力清单

1. **纯函数 `mergeEdges` 的判别力** —— 变异「上限改成 20000」⇒ ⇒ A-3 红 ⇒ ⇒ 「上限真的生效」有人管。
2. **纯函数 `mergeEdges` 的「保留最近」** —— 变异「截断时保留**最旧**的」⇒ ⇒ A-3 红 ⇒ ⇒ ⇒ 「保留最近」与「保留最旧」**判决不同** ⇒ ⇒ A-3 有判别力。
3. **`t_ps` 读回** —— 变异「`psToNumber` 换成 `Number(str)`」⇒ ⇒ A-4 红 ⇒ ⇒ ⇒ 「走不走契约层的读回器」有人管。
4. **`buildWaveOption` 的数据** —— 变异「series 数据取 `edges` 计数而不是样本」⇒ ⇒ A-5 红 ⇒ ⇒ ⇒ 「画的是不是真数据」有人管。
5. **x 轴类型** —— 变异「`xAxis.type` 改成 `"category"`」⇒ ⇒ A-7 红 ⇒ ⇒ ⇒ 「时间轴会不会被当成类别轴」有人管。
6. **阶梯语义** —— 变异「`step` 改成 `"linear"`」⇒ ⇒ A-8 红 ⇒ ⇒ ⇒ 「波形是不是阶梯」有人管。
7. **降级分支不是空转** —— 变异「`status === "degraded"` 分支删掉」⇒ ⇒ A-10 **红**（不是空转 ⇒ ⇒ 因为 A-10 断言**那四个字出现**）⇒ ⇒ ⇒ 「降级文案」有人管。
8. **加载失败不抛** —— 变异「`loadECharts` 改成不 catch（让异常冒出去）」⇒ ⇒ A-11 红 ⇒ ⇒ ⇒ 「失败会不会炸掉整条流」有人管。
9. **URL 唯一落点** —— 变异「在别处再写一份 CDN URL」⇒ ⇒ A-12 红 ⇒ ⇒ ⇒ 「第二个来源」有人管。
10. **`setChartSource("off")` 真的不发请求** —— 变异「`"off"` 分支删掉（照常去 import）」⇒ ⇒ A-13 红 ⇒ ⇒ ⇒ 「关闭是不是真的关闭」有人管。
11. **稳定钩子** —— 变异「把通道列表的 `N edges` 改成 `N 边沿`」⇒ ⇒ A-14 红 ⇒ ⇒ ⇒ 「既有判据的字面量」有人管。
12. **真图是另一张卡片** —— 变异「把图表插槽塞进 `.channel-list` 里」⇒ ⇒ A-15 红 ⇒ ⇒ ⇒ 「新旧两块分得开」有人管。
13. **显示条数与总数分开** —— 变异「series 名只带一个数」⇒ ⇒ A-16 红 ⇒ ⇒ ⇒ 「两个数不会混成一个」有人管。
14. **真机：echarts 真的画了** —— 变异「`afterRender` 不调 `setOption`」⇒ ⇒ B-28 红（`_echarts_instance_` 在但没图）/ B-29 红 ⇒ ⇒ ⇒ 「画布里真的有东西」有人管。
15. **真机：降级永远可测** —— 变异「`setChartSource("off")` 不触发重渲染」⇒ ⇒ B-30 红 ⇒ ⇒ ⇒ 「降级不是装饰」有人管。
16. **真机：稳定钩子没被改坏** —— 变异「通道列表文案改了」⇒ ⇒ B-32 红 ⇒ ⇒ ⇒ 「既有真机判据还成立」有人管。
17. **规格自检：7g 的窗口必须截断** —— ⚠️ **本条是写规格时当场踩到的**：第一版 7g 用「§9.6 之后 **4000 字符**的窗口」找「波形」⇒ ⇒ **那个窗口会溢出到 §9.7「逐模块技术选型矩阵」**⇒ ⇒ 那一节**本来就有**「波形管道」⇒ ⇒ ⇒ **判据恒真**⇒ ⇒ ⇒ **零判别力** ⇒ ⇒ 修法：① 窗口**截到「9.7」之前** ② 锚点换成 **「echarts」**（**只可能由本批写进去**的字面量）⇒ ⇒ ⇒ **通用判据**：**任何「在某文件某段里找某字面量」的判据 ⇒ ⇒ 必须先问「那段之外会不会本来就有它」** ⇒ ⇒ 否则就是**空转通过**的第四种形态。
18. **规格自检：NFR-05 那条是「坏夹具型」** —— 它现在是**绿的**（需求文档本来就没有「echarts」）⇒ ⇒ 它的判别力在**将来**（有人去改**不该改**的那份文档时变红）⇒ ⇒ ⇒ **必须配一条变异**（T8）来证明它**真的能红** ⇒ ⇒ 否则它只是「恰好绿」。

---

## §7 变更影响面

| 面 | 影响 |
| --- | --- |
| **契约层** | ⛔ 零改动 |
| **内核 / 宿主** | ⛔ 零改动 |
| **前端 · app 层** | `wireWaveform` 留住 samples + 加载 echarts + 写 `wave.chart` + 脚本口 |
| **前端 · 视图层** | `afterRender` 钩子（可选）+ `waveformView` 接线 + 持久图表宿主 |
| **前端 · 新模块** | `front/lib/wave-samples.js` · `front/lib/echarts-loader.js` · `front/views/components/wave-chart.js` |
| **前端 · 样式** | `front/style.css` 加波形图相关类 |
| **前端 · 判据** | 新增 `front/tests/wave-chart.test.mjs`；`tools/browser-verify.mjs` 新增 `waveChart` 段（段名表 13 → **14**） |
| **文档** | 架构 §9.6 / §11.2（裁决 O）· README · AGENTS · PROGRESS · USER_GUIDE · 台账 |
| **仓外依赖** | **无** ⇒ ⇒ 但**运行时**依赖 jsdelivr 可达（= 已知缺口） |

---

## §8 施工期订正

> ⛔ **留空**，施工期逐条填。
> 格式：`### 订正 N · 标题` ⇒ ⇒ 每段必须含「**规格原文** / **实测** / **已改成**」三节。
> ⚠️ **反向引用检查必须排除本节**（否则本节引用的编号会污染别处的检查）。

---

## §9 变异自检

> ⚠️ **§9 的「期望红集」列在施工期填成「实测红集」** —— 与 STAGE-34 / STAGE-35 同纪律。
> ⚠️ **每条变异前必须还原到基线**（`cp` + `md5` + `cmp` + `touch` 当前时间；⛔ 禁 `git checkout --` / `git restore`）。
> ⚠️ **收尾无条件还原**（`try/finally`）。
> ⚠️ **备份名带路径**（同名文件到处都是 ⇒ 只用文件名会撞车）。
> ⚠️ **「没跑到测试」与「测试都过了」必须分开报**（输出里有没有 `test result:` ⇒ ⇒ 没有就是错误，不许读成「0 红」）。
> ⚠️ **对照变异**必须落在「断言盯的不是它」的地方 ⇒ ⇒ 期望 **0 红**。

| 编号 | 变异 | 期望红集（施工期填**实测**） |
| --- | --- | --- |
| T1 | 上限改成 20000 | |
| T2 | 截断时保留**最旧**的 | |
| T3 | `psToNumber` 换成 `Number(str)` | |
| T4 | series 数据取 `edges` 计数而不是样本 | |
| T5 | `xAxis.type` 改成 `"category"` | |
| T6 | 降级分支（`status === "degraded"`）删掉 | |
| T7 | `setChartSource("off")` 分支删掉（照常去 import） | |
| T8 | **把需求文档 NFR-05 段落加上「echarts」** ⇒ ⇒ 抓「有人去改**不该改**的那份文档」⇒ ⇒ 期望 `stage36_check.py` 的 **7j 红** | |
| T-C1 | 对照：把 x 轴的名字从「虚拟时间 (ps)」改成「虚拟时间(ps)」（**只少一个空格** ⇒ ⇒ 语义等价 ⇒ ⇒ 没有断言盯轴名的确切写法）⇒ ⇒ 期望 **0 红** | |

---

## §10 自检脚本

`tools/stage36_check.py`，与 `tools/stage35_check.py` **同构**（⛔ **禁**自己再写一份表格切分 ⇒ ⇒ `from doc_check import split_row`）：

| 检查项 | 内容 |
| --- | --- |
| 1 | 文件存在 · §0~§10 齐全 |
| 2 | 表格列数一致（切分复用 `doc_check.split_row`） |
| 3 | 事实表编号 **F1 ~ F16** 连续 |
| 4 | 验收编号 **A-1 ~ A-16** / **B-27 ~ B-32** / **C-1 ~ C-5** 连续 ⇒ ⇒ 反向引用允许引用前几批已定义的编号 |
| 5 | §8 每条订正**自己那段里**都有「已改成」⇒ ⇒ **逐条判**、不是数全局次数 |
| 6 | 四个版本号的值（§5 的表是唯一权威处）⇒ ⇒ 仍 **8 / 2 / 1 / 3** |
| 7 | 裁决表行首字母 **A ~ O** 齐全（⛔ **不要**把字母加粗 ⇒ ⇒ 必须 `\| A \| **内容** \|`） |
| 7b | `front/lib/echarts-loader.js` 必须存在且导出 `loadECharts` / `setChartSource` / `getChartSource` |
| 7c | `front/lib/wave-samples.js` 必须存在且导出 `mergeEdges` / `WAVE_SAMPLE_LIMIT` |
| 7d | `front/views/components/wave-chart.js` 必须存在且导出 `buildWaveOption` / `waveChartHtml` |
| 7e | `front/app.js` 必须有 `window.riscv_waveform` 与 `setChartSource` |
| 7f | `front/views/m02_viewcomponents.js` 必须有 `afterRender` 与 `data-wave-chart-slot` |
| 7g | **ADR-6 已改** ⇒ ⇒ 架构文档 §9.6 段落（**必须截到 9.7 之前** ⇒ ⇒ 否则会溢出到「逐模块技术选型矩阵」⇒ ⇒ 那一节**本来就有**「波形管道」⇒ ⇒ 判据**恒真** ⇒ ⇒ **零判别力**）⇒ ⇒ 含「**echarts**」（**只可能由本批写进去**的字面量 ⇒ ⇒ 真正的判别力锚点」）与「波形」与「降级」；**NFR-05 未改** ⇒ ⇒ 需求文档 NFR-05 段落**不含**「echarts」 |
| 7h | 台账必须登记本批（`v0.51` + `STAGE-36`） |
| 7i | 脚本必须自指存在 |
| 7j | §9 变异编号齐全（`T1 ~ T8` + `T-C1`）⇒ ⇒ ⚠️ **`stage35_check.py` 没有这一项** ⇒ ⇒ **不要**照抄它的缺口 |
