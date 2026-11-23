# 历史规格自检（`tools/stage*_check.py`）重锚口径

> **这份文档回答一件事**：历史批次的规格自检**该钉什么**，才不会「后面每做一批就红一片」。
> ⚠️ 台账号、批次顺序与欠账真相仍在 [`../reviews/REVIEWS.md`](../reviews/REVIEWS.md)；
> 这里是**判据口径**的权威（v0.141 批确立）。

## 1 三条病根

历史自检大面积假红，只有三种来源，**都不是产品退化**：

1. **落点漂了（钉单文件）**：视图层 `front/views/m02_viewcomponents.js` 拆成 `front/views/components/*.js`；
   `front/app.js` 的接线拆到 `front/views/app/*.js`；宿主 `crates/kernel/src/kernel_host/mod.rs`
   拆成 `mod.rs` + 按域子模块。⇒ ⇒ 判据写着「某文件里必须有 X」⇒ ⇒ **文件没了**，X 还在。
2. **留痕搬家（钉现台账）**：台账规约是**闭环即整行移入** `REVIEWS-archive.md`（§1），
   逐版批次记录进 `PROGRESS-history.md`。⇒ ⇒ 判据只读 `REVIEWS.md` ⇒ ⇒ 登记过的事**找不到**。
3. **时点断言（钉「本批的值」）**：`SCHEMA_VERSION` 必须是 8 / 本批不许动 `SIM_STATE_SCHEMA_VERSION` /
   本批零新场景码 / 不得出现 `E_PLT_17`。⇒ ⇒ **后续批次一落地必红**，且它判的**不是能力**。

## 2 三条替换口径

| 病根 | 旧口径（假红） | 新口径（活判据） |
| --- | --- | --- |
| 落点漂 | `read("front/app.js")` | **认层**：`read_layer(REL, "front/views/app/*.js")` / `"front/views/components/*.js"` / `"crates/kernel/src/kernel_host/*.rs"`；钩子类认 `front/**/*.js` |
| 留痕搬家 | 只读 `REVIEWS.md` | **三处合体**：`REVIEWS.md` + `REVIEWS-archive.md` + `PROGRESS-history.md` |
| 时点断言 | `值 == 本批的值` | 见 §3 的四种活形态 |

## 3 时点断言的四种活形态（强度只增不减）

1. **单调下界**：版本常量 `>=` 本批落地值。⇒ 升版不红、**回退**才红。
2. **单源对账**：前端镜像 `===` 契约层常量（`crates/contract/src/lib.rs`）。⇒ 两边**漂了**才红。
3. **修前 → 修后（反向判）**：规格 §1.2 的「修前事实」不再要求原文还在源里（修复**必然**删掉它），
   改判**修后不许回来**（例：three-spaces 表的 internal line 行不许再把 `irq_clear` 归进「who uses it」）。
4. **编号空间不变式**：不再判「本批没新增」，改判
   ① `E_PLT_<n>_NAME: u16 = <m>` 的 `n == m`（编号与值不许错位）；
   ② 同一编号不许被两个非别名常量名占用；
   ③ 用例编号空间（如基址规划的 `N-1~N-10`）不许越界、条数有下界。

⚠️ **仍允许兜底的只有「修前事实」**：规格 §1.2 里那些「修掉就必然从源里消失」的行
（例：旧常量名被改名、旧语句被重构）⇒ ⇒ 原文的正确落点是**留痕**（台账 + 归档 + 进度归档）。
⚠️ **兜底不是放宽**：兜底只对**明确登记过**的行开放（`FALLBACK_LEDGER_ROWS`），且名单里每一条都要写清理由。

## 4 不做的事

- ⛔ **不就地换名糊弄**：锚点消失时，先判断「能力还在吗」——
  - 能力在（函数/钩子改名）⇒ ⇒ 钉**能力**（如 `planBaseGaps` 替代已删的真机段名）；
  - 能力也消失 ⇒ ⇒ 在脚本里写明「由 XX 批注销、仓内无留痕」并**移出硬判据**，不许静默删。
- ⛔ **不靠放宽断言换绿**：任何一条口径替换都要在脚本注释里写「改前判什么 / 改后判什么 / 为什么改」。
- ⛔ **不为历史批次补做产品**：本文件的改动**只碰 `tools/stage*_check.py`**，产品侧零改动。

## 5 已退役的能力（v0.77 批 · 2026-11-10）

**六件事被合法退役，代码里已经没有**（这一批是破坏性的，但 `PROGRESS.md` / `REVIEWS.md`
当时**没有留痕** —— 本文件与 PROGRESS 的追溯行补记）：

| 退役的东西 | 提交 | 替代它的是什么 |
| --- | --- | --- |
| 描述里的 `wiring` / `bridge` 段 + 连线报告三个 DTO（`BridgeReportDto` / `DroppedWireDto` / `WireDropReason`） | `16c01a7` · `eccc15c` | **每个外设自己的 `irq_line`**（中断源表就是描述本身） |
| `pins` 段（`PinBinding` / `normalizePins`），`E_PLT_19` 变保留号 | `eccc15c` | 同上（引脚不再单独声明） |
| pinmux 器件 + `PinSpec` / `PinFunction` / `PinDirection` / `parse_gpio_pin_name` / `pinmux_routes` / `E_PLT_23` | `83aafb9` | GPIO **整块一个中断源**（提交 9671021） |
| 前端连线编辑器（`bridge-editor.js` / `bridge-report.js` / `bridgeReport` 真机段） | `c0f375b` | `front/views/components/irq-editor.js`（`renderIrqEditor` · `data-irq-editor` · `data-irq-rows`） |
| 旧术语痕迹（`wiring_report` 等） | `980d9af` | — |

⇒ ⇒ **判据口径**：这些能力**不再需要** ⇒ ⇒ 对应断言**移除**（不是换名、也不是新造反向判据），
只保留「**同一编号不许被另一个含义占用**」这类与能力无关的不变式。

## 6 当前状态

- 历史规格自检：**39 个脚本全绿**（`for f in tools/stage*_check.py; do python3 "$f"; done`）。
- v0.141 批：22 绿 → 33 绿（27~46）。
- v0.142 批：33 绿 → **39 绿**（47~52）——
  - `stage47` / `stage51`：纯漂移（认层 + 版本下界 + 镜像对账 + 台账三处合体）。
  - `stage48` / `stage50`：能力整条退役（锚点移除，只留「实例表 / 引脚数落点」与编号不变式）。
  - `stage49`：`base` / `roles` / `assign_bases` 活着（重钉）；`pins` 退役（`E_PLT_19` 留保留号不变式）。
  - `stage52`：模板导入那一半活着（认层到 `topics.rs` / `events.rs` / `project.rs`、
    前端搬到 `product-panel.js` 的 `open-templates` / `use-template`）；pinmux 那一半退役。
