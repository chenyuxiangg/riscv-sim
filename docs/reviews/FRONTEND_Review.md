# 前端 M-01 / M-02 / M-03 · 代码审查报告

> **审查对象**：`front/` 全部 8 个文件（`index.html` 33 + `app.js` 65 + `style.css` 184 + `views/m01_workbench.js` 352 + `views/m02_viewcomponents.js` 379 + `views/m03_viewstore.js` 76 + `services/m04_session.js` 8 + `services/m05_commandrouter.js` 8 + `services/m07_transport.js` 11 + `services/m10_registermemory.js` 7，共 1123 行）
> **审查基准**：`docs/detailed-design/M-01_Workbench.html` / `M-02_ViewComponents.html` / `M-03_ViewStore.html`、LLD §3.1~§3.4、NFR-04（跨层统一转义）、NFR-05（部署形态）、PROGRESS Q4（同源 URL / HTTP 部署）
> **审查日期**：2026-10-20
> **审查方式**：全文件通读 + **ESM 链接实证**（Node 22 加载 `front/app.js`）+ **最小 DOM stub 跑通 bootstrap 实证**
> **门禁状态**：前端**无任何测试**（无 test runner、无 CI 冒烟）——这是本报告全部阻断项长期潜伏的直接原因
> **结论**：**2 项阻断级缺陷导致前端 100% 白屏**，且经实证确认**这就是启动路径上的全部阻断点**（修掉约 3 行即可跑通）。另发现 5 项功能/可用性缺陷、9 项整洁项。设计层面（角色×能力位复校验、组件三函数形状、视图注册表开放可扩展）是干净的，问题集中在**接线与收尾**。
>
> ---
>
> ## ✅ 闭环状态（2026-10-20 · 分支 `fix/frontend-review-closure`）
>
> **16 项全部闭环**，另捕获并修复 **4 项评审漏项**（#17 / #18 见 §七·2，**#19 / #20 见 §七·2.1**）；新增前端门禁 **35 用例 / 7 suite 全绿**（`tools/check-front.sh`），并以 **21 组变异测试**证明门禁能抓住同类回归（含复审新增的 11 组，见 §八·7）。
>
> ⚠️ **#19 / #20 是评审漏项，且只有真实浏览器能暴露**：修完白屏后，页面能起来，但**每个视图体都是空的**——DOM stub 测试对此完全隐形（见 §七·2.1）。逐项核对见 **§七**。

---

## 一、实证复现记录

### 1.1 当前状态：ESM 链接失败 → 白屏

```
$ node --input-type=module -e "import('./app.js')"
FAIL: The requested module './m01_workbench.js' does not provide an export named 'activeView'
```

浏览器行为一致：ES module 的链接错误发生在**任何代码执行之前**，`bootstrap()` 永不启动，状态栏永远停在 `index.html` 的静态文案「未连接」。

### 1.2 关键验证：这两处是否就是全部阻断点？

在 `/tmp` 副本上（**未触碰仓库**）仅补两处导出：

```js
// m01_workbench.js
export let activeView = VIEW_REGISTRY.get("embedded") ?? null;   // +export
// m03_viewstore.js
export async function mountViewStore() { return Promise.resolve(); }  // +新增
```

结果：

```
LINK OK  →  随后仅剩 "document is not defined"（Node 无 DOM，属预期）
用最小 DOM stub 跑完整 bootstrap：
BOOTSTRAP OK — app.js 链接并执行成功
```

**结论：修掉这 2 处导出，前端即可正常启动，后续没有隐藏的链接期错误。**

### 1.3 活动标签页从不高亮

```
$ grep -rn "aria-current" front/
front/style.css:58:.tab[aria-current="page"] { color: var(--amber); ... }
front/index.html:13-18:  <a ... data-view-tab="product">产品</a>  （共 6 个）
```

**CSS 有样式、HTML 有属性，但全前端 JS 无一处设置 `aria-current`、无一处查询 `data-view-tab`** —— 活动态的视觉线索完全缺失。

---

## 二、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 建议修复 |
| --- | --- | --- | --- | --- | --- |
| 1 | **阻断** | `views/m02_viewcomponents.js:19` | `import { activeView, refreshAll } from "./m01_workbench.js"` —— **`activeView` 在 `m01_workbench.js:126` 是 `let`，没有 export** | 实证（§1.1）。ESM 链接期失败 → 整个 `app.js` 及其依赖图不执行 → **白屏**。这是全项目当前**唯一的前端 P0** | ① 推荐：m01 改成 `export function getActiveView() { return activeView; }`，m02 改用函数调用（导出可变 `let` 虽然合法，但 ESM live binding 会让 m02 持有的是**同一变量**而非快照，语义上更易出错）；② 或直接 `export let activeView`（m02 实际上从未使用它，可考虑**直接删掉这个 import**——见 #8） |
| 2 | **阻断** | `app.js:10` | `import { mountViewStore } from "./views/m03_viewstore.js"` —— **m03 只导出 `setState/getState/snapshot/subscribe/reset`，没有 `mountViewStore`** | 实证：修掉 #1 后立即暴露 `does not provide an export named 'mountViewStore'` | m03 补 `export async function mountViewStore() { return Promise.resolve(); }`（与 m04/m05/m07/m10 四个 service 的空挂载风格一致），或从 `app.js` 删掉该导入。**必须与 #1 同批修**——只修一个仍然白屏 |
| 3 | 高 | `views/m02_viewcomponents.js:114-115` | 「刷新 demo 映射」按钮**恒不触发** | `const row = target.closest?.("[data-region-base]")` 要求存在表格行祖先，但该按钮渲染在 `</table>` **之外**（`:161`）→ `row` 恒为 `null` → 整个 `if` 不成立 → 按钮是死的 | 去掉 `row &&` 前置条件（按钮本身的 `data-action` 已是充分判据）。顺带：`row` 变量在修掉后完全无用，应一并删除 |
| 4 | 高 | `views/m02_viewcomponents.js:279` | `out.textContent = (out.textContent ?? "") + escapeHtml(bytes)` —— **双层转义** | `textContent` 不解析 HTML，`escapeHtml` 的 `&lt;` 会**字面显示**。固件输出 `<` 会显示成 `&lt;` | 去掉 `escapeHtml`：`out.textContent = (out.textContent ?? "") + bytes;`。`textContent` 赋值天然防 XSS，转义反而多余 |
| 5 | 高 | `style.css:58` + `index.html:13-18` | **活动标签页从不高亮，用户无从判断当前视图** | 实证（§1.3）：CSS 写了 `.tab[aria-current="page"]`，JS 无处设置；`data-view-tab` 也从未被查询 | `switchView` 里同步标签态：遍历 `[data-view-tab]`，命中 `target.hash` 的设 `aria-current="page"`，其余 `removeAttribute`。顺带可用 `data-view-tab` 做选择器，省掉 hash 字符串匹配 |
| 6 | 中 | `views/m01_workbench.js:220-222` + `:207-212` | `switchView` 命中同一视图时早返回 `type:"noop"` 且**不派发 render 事件**；`setCapabilities` 在当前视图仍被允许时也**不重渲染** | `route()` 走到已激活的 hash 不会刷新内容；能力位变更后视图内容不更新 → 状态陈旧 | 两处补 `dispatchRenderEvent`：`switchView` 的 noop 分支也发一次 `type:"update"`；`setCapabilities` 末尾统一调 `refreshAll()`（该函数已存在且语义正确） |
| 7 | 中 | `views/m01_workbench.js:224-226` | `viewStore.setState(activeView.hash, {})` —— 注释写「Save current view state to the store」，但 m03 的 `setState` 是 **merge** 语义（`m03:27` 的 `{...prev, ...patch}`），传 `{}` 是 **no-op** | 不会丢数据（状态一直在 m03 的 Map 里），但**注释与实现不符**，且视图的折叠态/波形缩放/终端滚动位置**从未被采集**——LLD §3.3 要求的「按 viewId 存 UI 交互态」实际只有内核状态、没有 UI 态 | 要么把注释改成「m03 已持有状态，这里无需转存（merge 语义下传 `{}` 是 no-op）」；要么补真实的 UI 态采集（`switchView` 前读 DOM 的 `scrollTop`/`classList` 写入 store） |
| 8 | 低 | `views/m02_viewcomponents.js:19` | `activeView` 被 import 但**在 m02 里从未使用** | grep 确认 m02 全文无 `activeView` 引用。这正是 #1 的成因——为一个用不到的符号建立跨模块依赖 | **直接删掉这个 import**（连带 `refreshAll` 保留）。这样 #1 消失得最干净，且不新增公开 API |
| 9 | 低 | `views/m01_workbench.js:330` + `views/m02_viewcomponents.js:74` | `escapeHtml` **两份逐字重复** | 两处实现完全相同（5 个字符全转义） | 抽到 `front/lib/escape.js` 或让 m02 从 m01 导入（m02 已经 import m01 了） |
| 10 | 低 | `views/m01_workbench.js:305` | `createPanel` 里 `data-view-body="${v.hash}"` **未转义** | `v.hash` 来自常量表，当前安全；但这是**把数据拼进 HTML 字符串**的模式，一旦将来 hash 来自外部就出问题 | 与 #9 一并处理：改用 `document.createElement` + `dataset` 赋值，彻底不拼 HTML 字符串 |
| 11 | 低 | `app.js:59` | 状态栏写死 `"MVP 就绪 — kernel 待编译"` | kernel **早已编译**，M-13 / M-14 / M-18 / M-19 都已跑通（workspace 203 测试全绿）。用户看到的是过期信息 | 改为从实际状态推导，或至少改成「工作台已就绪 — kernel 待接线」 |
| 12 | 低 | `index.html:26-29` | footer 写死 `"MVP 第一刀：M-14 总线 + M-01/02/03 工作台（M1 数据底座 4 人日）"` | 同样过期；项目已推进到 M-19 | 改为不写死版本号的通用文案，或由 JS 填充 |
| 13 | 低 | `index.html:31` | `<script type="module" src="app.js">` —— **ES module 强制 CORS，`file://` 直接打开会被浏览器拦** | PROGRESS Q4 已定「同源 URL / HTTP 部署」，`python3 -m http.server` 是既定方式。但 `AGENTS.md` 里「浏览器 WASM 单文件自包含（file:// 直接打开，NFR-05）」的表述与实现不符（用户已放宽为目录形式，文档没同步）。双击打开 → 空白页且无提示 | ① `index.html` 加一行提示「需通过 HTTP 打开（ES module + Worker 均要求同源）」；② 同步 `AGENTS.md` / `PROGRESS.md` 里的部署形态表述 |
| 14 | 低 | `views/m01_workbench.js:169` | `mountedViews.get(role)` —— role 取值 `"product"`/`"embedded"` **恰好与 view hash 同名** | 这是**巧合而非契约**。若将来 role 名与 view hash 不一致，`mountedViews.get(role)` 静默返回 `null`，回落到「第一个视图」，不会报错 | 显式建立 role→默认视图的映射表，不依赖命名巧合 |
| 15 | 低 | `views/m02_viewcomponents.js:346-379` + `index.html:20` | dev-only 的 `buildDemoStoreState` 假数据与生产视图组件**混在同一模块**，且页面上有面向用户的「刷新 demo 映射」按钮 | prod/dev 边界不清；正式接上 M-07 后，demo 数据源仍在同一渲染路径上，容易忘了替换 | 把 demo 数据与 seeder 移到 `front/dev/` 或用 `import.meta.env`-式的开关隔离；MVP 阶段可接受，但应显式标注 |
| 16 | 低 | `front/` 整体 | **前端零测试**：无 test runner、无 CI 冒烟、无 DOM 测试 | 这是 #1 / #2 两个 P0 长期潜伏到今天的**根因**——Rust 侧 203 个测试全绿，前端却从未被任何自动化检查过 | ① 最低成本（必做）：CI 加一步 `node --input-type=module -e "import('./front/app.js')"` 捕获 ESM 链接回归（我本次正是用这个手段定位的）；② 进阶：用 node:test + 最小 DOM stub 覆盖 `route()` / `switchView()` / `setCapabilities()` 的纯逻辑 |

---

## 三、跨模块欠账（台账已追踪，本报告不重复）

| D-ID | 描述 | 状态 |
| --- | --- | --- |
| **D-01 / D-02** | 前端两处 ESM 导出缺失 | 本报告 #1 / #2，**完全未动** |
| **D-03** | `m02_viewcomponents.js` 的 demo 按钮恒不触发 | 本报告 #3，**完全未动** |
| **D-04** | `m01` 的 `switchView` 状态转存是 no-op | 本报告 #7，**完全未动** |
| **D-05** | `m02` 的 `onDelta` 双层转义 | 本报告 #4，**完全未动** |
| **D-18 ~ D-20** | 台账中前端相关的 3 项 | 分别对应本报告 #3 / #7 / #4，**完全未动** |

**前端自 v0.4 落地以来，除 v0.4 本身的 4 视图组件外，**`front/` 目录 5 轮 PROGRESS 迭代（v0.5~v0.9）**未再被改动过一行**——本报告是它的第一次模块级 review。

---

## 四、值得肯定的设计

| 点 | 说明 |
| --- | --- |
| **角色 × 能力位复校验** | `route()` 做两级检查（视图是否存在 → 当前能力位是否允许），不通过则回落 `FALLBACK_VIEW` 并记 E_CTX-05（`m01:185-197`）。这是 LLD §3.2 的核心安全要求，实现完整 |
| **视图注册表开放可扩展** | `VIEW_REGISTRY` 是 `Map<hash, ViewDescriptor>`，新增视图不改路由逻辑；`capability` 字段把「谁能看这个视图」与「视图本身」解耦 |
| **组件三函数形状清晰** | `render(storeState)` / `onCommand(ev)` / `onDelta(delta)` 三个函数对应「渲染 / 命令 / 增量」三条数据流，LLD §2.1.1 的形状被严格遵守 |
| **render 事件用 CustomEvent 分发** | M-01 与 M-02 之间只通过 `riscv:render` 事件通信，不互相 import 实现细节（除了 #8 那个多余的单向 import）——解耦方向正确 |
| **escapeHtml 覆盖完整** | 5 个字符（`& < > " '`）全转义，含单引号（很多实现会漏）。所有来自 `state` 的文本都过了它（`m02:136`、`:154`、`:185`、`:244`、`:267`） |
| **CSS 组织清晰** | 变量化的 dark 主题、语义化类名、`:root` 统一色板、`@media (max-width: 767px)` 响应式、终端区 `pre-wrap` + `word-break` 防止长行破版 |
| **终端区用 textContent 赋值** | 天然防 XSS（只是被 #4 额外套了层多余的 escapeHtml） |

---

## 五、建议处理顺序

1. **#1 + #2 必须同批修**（约 3 行）——**修完前端立即从白屏变成可用**，这是全项目投入产出比最高的一处。修法建议取 #8 的组合：m02 删掉用不到的 `activeView` import（消除 #1）+ m03 补 `mountViewStore`（消除 #2）。
2. **#3 / #4 / #5 一起做**——都是十几行的视图层收尾，做完前端才算「能看能用」：按钮能点、终端不显示字面 `&lt;`、活动标签页有高亮。
3. **#16 最低成本那一步（CI 加一行 ESM 导入检查）建议立刻做**——它就是本次定位 #1/#2 的手段，成本几乎为零，能防住同类回归。
4. **#6 / #7 / #9~#16** 合并为一次 `chore(front)` 提交。#13 需要同步改 `AGENTS.md` / `PROGRESS.md` 的部署形态表述，属于文档同步，建议与代码分开提。

---

## 六、审查边界声明

- 本报告仅覆盖 `front/` 目录 + 前端相关的 NFR-04 / NFR-05 约束。契约层与内核不受影响。
- 「ESM 链接实证」在 `/tmp` 副本上完成，**仓库内文件零改动**；验证脚本与副本已删除。
- 门禁结论：前端**无门禁**——这本身是本报告 #16 的内容。

---

## 七、闭环核对（2026-10-20 · 分支 `fix/frontend-review-closure`）

**结论：§二 的 16 项全部闭环；另捕获并修复 2 项评审漏项；新增前端门禁并以变异测试证明其有效。**

### 7.1 逐项闭环

| # | 严重度 | 状态 | 落点 | 实证 |
| --- | --- | --- | --- | --- |
| 1 | **阻断** | ✅ | `m02` 删掉 `activeView` import（该符号在 m02 全文从未被引用） | 链接期无错，bootstrap 成功；门禁「app.js 链接成功并完成挂载」用例 |
| 2 | **阻断** | ✅ | `m03` 补 `export async function mountViewStore()`，写法对齐 m04/m05/m07/m10 | 同上；变异 A（删掉该导出）→ 门禁变红 |
| 3 | 高 | ✅ | `m02.onCommand` 去掉 `closest("[data-region-base]")` 前置条件 | 变异 C（恢复该前置条件）→ 门禁变红 |
| 4 | 高 | ✅ | `m02.onDelta` 去掉 `escapeHtml()`，字节同步进 M-03 store | 终端实测 `textContent === "<x>&y"`；变异 B → 门禁变红 |
| 5 | 高 | ✅ | `m01` 新增 `syncTabState()`，切换视图同步 `[data-view-tab]` 的 `aria-current` | 路由 `#/debug` 后唯一高亮 `debug`；变异 E → 门禁变红 |
| 6 | 中 | ✅ | `switchView` 重入分支改派发 `type:"update"`；`setCapabilities` 末尾统一 `refreshAll()` | 变异 F（重入不派发）→ 门禁变红 |
| 7 | 中 | ✅ | 新增 `captureUiState()` / `restoreUiState()`：切走前采集、切回后还原（终端滚动位置 + 视图体滚动位置） | 终端 `scrollTop 42` 跨切换还原；变异 G → 门禁变红 |
| 8 | 低 | ✅ | 随 #1 一并消除（不再为用不到的符号建立跨模块依赖） | `grep activeView front/` 仅命中 m01 自身定义 |
| 9 | 低 | ✅ | 抽到 `front/lib/escape.js`；`m02` re-export 保持 API 不破，`m01` 随 DOM 构造化不再需要 | 静态检查「escapeHtml 只有一份实现」；变异 H → 门禁变红 |
| 10 | 低 | ✅ | `createPanel()` 改用 `createElement` + `dataset`，视图名走 `textContent` | 面板结构由 DOM 调用构造，不再拼 HTML 字符串 |
| 11 | 低 | ✅ | `m07.mountTransport()` 返回 `{mode, connected}`；状态栏由该描述符推导 | 状态栏实测「工作台已就绪 — kernel transport 未连接（M-07 待接线）」 |
| 12 | 低 | ✅ | 页脚去版本化；`#build-info` 由 JS 填充（role + 已挂载视图名） | 页脚实测「当前工作台：role=embedded · 产品视图 / …」 |
| 13 | 低 | ✅ | `index.html` 加 inline classic script 检测 `file:` 并给出 HTTP 部署指引；页脚常驻部署提示；`build-wasm.sh` 末尾提示同步 | 静态检查「index.html 含 file:// 部署提示」；文档侧见 `AGENTS.md` / `PROGRESS.md` 同步 |
| 14 | 低 | ✅ | 新增 `ROLE_DEFAULT_VIEW` 显式映射表；`assembleWorkspace()` 与 `route()` 的兜底都改走它 | 角色切换后视图集合断言通过 |
| 15 | 低 | ✅ | demo 数据移到 `front/dev/demo-state.js`；M-02 只发 `riscv:seed` 信号，dev 装配层消费 | `front/dev/` 与 `front/views/` 边界清晰；生产模块不再持有假数据 |
| 16 | 低 | ✅ | 新增 `front/tests/dom-stub.mjs` + `front/tests/frontend.test.mjs`（35 用例 / 7 suite）+ `tools/check-front.sh` + `.github/workflows/ci.yml` | `tools/check-front.sh` → 35 pass / 0 fail |
| **19** | 中 | ✅ | 漏项，见 §七·2.1 | 真机：初始面板可见；变异 J → 门禁变红 |
| **20** | **阻断** | ✅ | 漏项，见 §七·2.1 | 真机：产品视图完整渲染（产品树 / 内存映射）；变异 I → 门禁变红 |

### 7.2 评审漏项（本次修复中新发现）

| # | 严重度 | 问题 | 为何评审没抓到 | 落点 |
| --- | --- | --- | --- | --- |
| **17** | 高 | `m02:116` 调用 `seedDemoBus()`，该函数在整个 `front/` **从未定义** | 评审看到按钮被 `row &&` 前置条件挡住，判定为「死按钮」，因此没有沿着调用链往下看。**单纯按 #3 去掉前置条件，会把死按钮变成点了就抛 `ReferenceError`** | 改为调用 `seedBus()` 信号；demo 数据由 `front/dev/demo-state.js` 提供 |
| **18** | 中 | `refreshAll()` 调 `mountPanels()` 会把**所有**面板加上 `hidden`，却从不调 `showPanel()` → 任何调用 `refreshAll()` 的路径都会让当前视图消失（白屏） | 评审只指出「能力位变更后视图内容不更新」，未追到 `refreshAll` 的实现；且该缺陷是**潜伏**的——修复前 `refreshAll()` 唯一的调用点（`m02.onCommand`）会先在 `seedDemoBus()` 上抛异常，够不到 | `refreshAll()` 补 `showPanel(activeView)`；实测：修复前 `visible=true → false`，修复后保持 `true` |

### 7.2.1 真机验证暴露的两个漏项（stub 测试完全隐形）

§七·1 全部闭环后，用**真实浏览器**（`python3 -m http.server` + Electron，截图核对）复验，发现页面能起来但**每个视图体都是空的**——评审的 16 项修完仍不可用。这两项**对 DOM stub 测试完全隐形**：测试都在 bootstrap 之后才调 `route()`，那时监听早已就位；只有真实浏览器的「刷新页面 / 带 hash 直接打开」路径会命中。

| # | 严重度 | 问题 | 为何评审与 stub 都抓不到 | 落点 |
| --- | --- | --- | --- | --- |
| **19** | 中 | `switchView()` 的「同视图早返回」分支不调 `showPanel()`，而 `mountPanels()` 会把**所有**面板加上 `hidden`。bootstrap 的首次路由恰好命中该分支（初始视图是预置的）→ 当前面板再也没被点亮 → **主区域整块空白** | 评审只追了「视图内容不更新」，没注意面板**可见性**是另一条独立的状态线 | 该分支补 `showPanel(target)` |
| **20** | **阻断** | **首次 `riscv:render` 事件被丢弃**：`mountWorkbench()` 内部做初始路由并派发**第一个** render 事件，而订阅方 `mountViewComponents()` 在 `Promise.all([mountWorkbench(), mountViewComponents(), ...])` 里**排在后面**（数组字面量从左到右求值）→ 事件派发时还没人订阅 → **视图体永远空白** | 评审做的是「静态 + 链接期」分析，没做运行期时序分析；stub 测试则因为都在 bootstrap 之后触发路由而系统性错过 | ① `app.js` 把 `mountViewComponents()` 排到 `Promise.all` 首位；② `m01` 的初始路由改用 `queueMicrotask` 延到当前任务之后——两处独立地保证「订阅先于首次派发」 |

**真机验证结论**（修复后，截图证据）：页面正常启动不再白屏；初始视图完整渲染（产品视图 / 产品树 / 内存映射 (M-14) / 提示段）；活动标签页高亮；状态栏由 transport 状态推导；页脚由 JS 填充 role 与已挂载视图列表。

### 7.3 门禁与变异测试

新增门禁（`tools/check-front.sh` → `node --test "front/tests/*.test.mjs"`）：**35 用例 / 7 suite / 0 失败**，零第三方依赖，Node ≥ 22（复审后新增 14 条，含 7 条 DOM stub 保真度自测）。

门禁有效性用变异测试验证——把每个原始缺陷放回去，门禁必须变红：

| 变异 | 对应缺陷 | 门禁结果 |
| --- | --- | --- |
| A · 删掉 `mountViewStore` 导出 | #2 / D-02 | 🔴 FAIL ✓ |
| B · 恢复终端双层转义 | #4 / D-20 | 🔴 FAIL ✓ |
| C · 恢复 `row` 前置条件 | #3 / D-18 | 🔴 FAIL ✓ |
| D · `refreshAll` 少 `showPanel` | #18 | 🔴 FAIL ✓ |
| E · 不设 `aria-current` | #5 | 🔴 FAIL ✓ |
| F · 重入不再派发 render | #6 | 🔴 FAIL ✓ |
| G · UI 态采集退化为 `setState(hash, {})` | #7 / D-19 | 🔴 FAIL ✓ |
| H · `escapeHtml` 出现第二份实现 | #9 | 🔴 FAIL ✓ |
| I · 退回原始挂载顺序 + 同步路由 | #20 | 🔴 FAIL ✓（`innerHTML 长度 0`） |
| J · 少 `showPanel(target)` | #19 | 🔴 FAIL ✓ |
| — 全部还原 | — | 🟢 PASS ✓ |

> 变异 C **首轮曾漏网**：原断言只检查「store 里有 3 条映射」，而点击前就已 seed 过，死按钮同样能通过。已改为断言「点击确实发出了 `riscv:seed` 信号」且「点击确实重新 seed 了映射」，复测通过。

### 7.4 未闭环观察项（本次未修，非评审项）

| 观察 | 严重度 | 说明 |
| --- | --- | --- |
| 标签栏不按能力位过滤 | 低 | `product` 角色仍显示「调试 / 寄存器」标签，点进去会落到 fallback 视图。属产品决策（隐藏标签 vs 显示但禁用），不在评审 16 项内，留待 M-01 接线 M-05 negotiate 时一并定 |
| 文件名用下划线而非 kebab-case | 低 | `AGENTS.md` 规定 kebab-case（`m01-workbench.js`），现状是 `m01_workbench.js`。改名会波及全部 import，且与评审 16 项无关，留作独立重构 |
| demo 按钮仍是生产视图内的 dev 入口 | 低 | #15 的既定处置是「MVP 阶段可接受，但应显式标注」——已隔离到 `front/dev/` 并在 UI 上标注 dev only |

### 7.5 闭环 commit

| commit | 内容 |
| --- | --- |
| `fc83e1e` | fix(front): 修复前端白屏阻断（ESM 导出缺失 D-01/D-02） |
| `37fe8da` | fix(front): 视图层收尾 — demo 按钮可用 / 终端不双层转义 / 活动标签高亮 |
| `4738aa6` | fix(front): 视图状态同步 + UI 交互态采集（switchView / setCapabilities / refreshAll） |
| `97c85ac` | refactor(front): escape 单点化 / 面板改用 DOM 构造 / role→默认视图显式映射 |
| `bb59e6a` | chore(front): 状态栏/页脚文案改为从真实状态推导 + 补 file:// 部署提示 |
| `ed66337` | test(front): 新增前端冒烟门禁（node:test + DOM stub + CI） |
| `6fc9e1f` | fix(front): 修复「视图体全空」——首次 render 事件被丢弃 + 初始面板不显示 |

---

## 八、复审（2026-10-20 · 独立复检 `fix/frontend-review-closure`）

> **复审范围**：`git diff main..fix/frontend-review-closure`（11 commit，32 文件，+1644/−221）。**只读检视，未改动 `front/` 与 `crates/` 任何文件。**
> **复审方法**：① 逐文件通读改动；② **16 组变异测试**（把每项修复改回去，看门禁是否仍绿）；③ DOM stub 保真度实测（8 项探针）；④ 文档 72 处改写的语义抽检；⑤ 亲跑三门禁。
> **基线复核**：`cargo test --workspace` 203 passed ✓ · `cargo clippy -D warnings` ✓ · `cargo fmt --check` ✓ · `tools/check-front.sh` **21 passed** ✓ · CI YAML 合法、exec 位已入库（`100755`）、引号 glob 在 Node 22 生效 ✓
> **总体结论**：**16 项首审意见 + 4 项评审漏项确认真实闭环**（逐项变异验证，见 §八·3）。**但门禁本身存在 3 处盲区、DOM stub 有 5 处保真度缺口、1 处位置耦合缺陷**——它们不会让当前代码出错，但会让**同类回归再次静默通过**。

### 8.1 已确认的发现（16 项）

| # | 严重度 | 位置 | 问题 | 证据 | 建议 |
| --- | --- | --- | --- | --- | --- |
| **R-01** | **高** | `app.js:81` | `Promise.all` 的**数组长度（7）与解构槽数（6）不同步**，靠位置耦合绑定 | 数组 7 元素 `[mountViewComponents, mountWorkbench, mountViewStore, mountSession, mountCommandRouter, mountTransport, mountRegisterMemory]`，解构 `[, workspace, , , , transport]` → `transport` ← index 5 = `mountTransport()`，**当前正确**。但**变异测试：在 index 2 插入一个 mount → `transport` 静默改绑到 `mountRegisterMemory()`（undefined）→ 状态栏降级为「未连接」，门禁 21 pass / 0 fail** | 改具名对象解构（对象字面量求值顺序仍是源码顺序，顺序保证不变但不再错位）：<br>`const { components, workspace, transport } = await Promise.all({ components: mountViewComponents(), workspace: mountWorkbench(), … })` |
| **R-02** | **高** | `front/tests/frontend.test.mjs:75`、`:58-69` | **门禁对 #10 / #11 / #12 三项修复完全无感知** | 三项变异均 **21 pass / 0 fail**：① #10 `createPanel` 改回 innerHTML 字符串拼接；② #11 状态栏改回硬编码；③ #12 页脚改回硬编码。成因：页脚断言是 `/role=(product|embedded)/`，**硬编码的 "role=embedded" 同样匹配**；状态栏断言只排除「启动失败」与「未连接」两个具体字符串，降级文案「工作台已就绪 — kernel transport 未连接」两条都满足 | 断言改为**绑定真实状态值**而非匹配宽泛正则：页脚断言包含 `workspace.views` 里的**视图名**（硬编码写不出来）；状态栏断言随 `transport.connected` 变化而变化（跑两遍 transport 状态比对） |
| **R-03** | 中 | `front/tests/dom-stub.mjs:285-288` | **`^=` / `$=` / `~=` / `|=` 选择器算符未实现，退化为「属性存在即匹配」** | `matchesSimple` 只对 `=` 比较值，其余算符直接跳过值比较。**实测 `[data-view-tab^="deb"]` → stub 返回 `t1`（`data-view-tab="product"`），真实浏览器返回 `t2`（`data-view-tab="debug"`）**；`[data-view-tab$="bug"]` 同样返回 `t1` | 实现这四个算符；或**遇到未实现算符直接抛错**——静默假绿比明确失败危险得多 |
| **R-04** | 中 | `front/tests/dom-stub.mjs:111-112` + `front/style.css:87` | **stub 无滚动模型 → 「终端滚动位置还原」这条用例是恒等式** | `scrollTop`/`scrollHeight` 是普通数据属性，**`scrollHeight` 恒 0**。实测 `out.scrollTop = out.scrollHeight` → 0（真实浏览器会滚到底）。于是 `frontend.test.mjs:176` 的 `assert.equal(…scrollTop, 42)` **只证明「赋了值」，不证明「浏览器滚过去了」**。更关键：`.view-body { min-height: 200px; }` **没有 `overflow:auto`** → `captureUiState` 采集的 `body.scrollTop` 在真实浏览器里**恒为 0**（`.view-body` 不是滚动容器），只有 `.terminal-out`（`max-height:280px; overflow:auto`）是真滚动容器 | ① 给 `.view-body` 加 `overflow:auto`（否则 `body.scrollTop` 这条采集采的是一个恒 0 的值）；② 或删掉 `body.scrollTop` 采集；③ 测试改成断言「发生了赋值」而非「值生效」，并注明真实浏览器仍需复验 |
| **R-05** | 中 | `front/tests/dom-stub.mjs:204-228` | stub **忽略 `addEventListener` 第三参**（capture/once/passive），且 **`stopPropagation` 不被尊重** | 实测：`{ once: true }` 监听被触发 **2 次**（真实浏览器 1 次）；子节点 `stopPropagation()` 后祖先监听仍收到 **1 次**（真实浏览器 0 次）——`dispatchEvent` 无条件沿 `parentNode` 上溯，不看 `cancelBubble` | 当前前端不用这些 API → 今天无实际故障。但 stub 允许写出「浏览器里行为不同」的代码而测试照绿，属**假绿来源**。建议至少实现 `once` 与 `stopPropagation` |
| **R-06** | 中 | `front/views/m01_workbench.js:402-407` | `captureUiState` 写的 `ui` 键在 merge 语义下**整块替换** | `setState` 是浅 merge（`m03:27`），`{ ui: {...} }` → `next.ui` 整体被替换。**与内核状态（`sim`/`wave`/`term`/`tree`）是兄弟键，不互相污染 ✓**（这一顾虑可以排除）。但若将来往 `ui` 下再加键（如 `ui.zoom`），一次 `captureUiState` 就会抹掉它 | 改函数式派生保留既有子键：`setState(viewId, prev => ({ ...prev, ui: { ...(prev.ui ?? {}), scrollTop, terminalScrollTop } }))` |
| **R-07** | 低 | `front/views/m02_viewcomponents.js:277-288` | `onDelta` **同时写 store 与 DOM**，理论上可重复追加 | `setState` 累加 `term.bytes`，随后 `out.textContent = (out.textContent ?? "") + bytes` **再基于 DOM 当前值追加**。若 `setState` 的订阅者触发重渲染，DOM 会先从 store 重建（已含该字节），再被追加一次 → **重复追加**。**当前不可达**：`m03.subscribe` 无任何调用方，且 `onDelta` 无人调用（M-07 未接线）→ **推测级风险**，接线时会变成真实路径 | `onDelta` 只写 store，DOM 更新统一交给 `applyRenderEvent`；或反过来只写 DOM，但内容从 store 重新计算而非累加 `out.textContent` |
| **R-08** | 低 | `front/app.js:76-80` | **注释把功劳记给了数组顺序，实际是 `queueMicrotask` 在兜底** | 注释称「Mounting the workbench first would drop that event」——但有 `queueMicrotask`（`m01:461`）后**即使 workbench 先 mount 也不会丢事件**。变异测试佐证：单独去掉 `queueMicrotask` → **21 pass / 0 fail**（数组顺序仍保证订阅先注册）；**同时**去掉 microtask 且交换数组顺序 → 19 pass / 2 fail ✓ | 两处保险是**冗余**的（好设计，门禁锁的是「结果」而非「机制」）。但注释会误导后来者以为可以随意调整数组顺序。建议改为「两处冗余保险：数组顺序保证同步订阅先注册，queueMicrotask 保证异步 mount 也不会错过」 |
| **R-09** | 低 | `AGENTS.md:48`、`PROGRESS.md:63`、`CROSS_MODULE_DEBT.md:112`、`FRONTEND_Review.md:14` | **门禁用例数 4 处不一致** | 实际 `node --test` 报 **`# tests 21 / # suites 6`**。三处写「**19 用例**」、一处写「21 用例 / **7 suite**」（suite 数错） | 统一为 **21 用例 / 6 suite** |
| **R-10** | 低 | `tools/check-front.sh:19-23` | 脚本宣称「needs Node >= 22」但**不校验版本** | `command -v node` 通过即往下跑。`node --test` 的**带参 glob** 支持是 Node **21+**（`describe`/`test` 是 18+）→ Node 18/20 上会报出难以理解的错误。CI 用 `setup-node@v4` + `node-version: 22` 固定 ✓，本地开发可能踩坑 | 加一行版本判断：`node -e "process.exit(+process.versions.node.split('.')[0] >= 22 ? 0 : 1)"` |
| **R-11** | 低 | 根 `package.json` + `.gitignore:32` | `"type":"module"` 位于**仓库根**，影响全仓库 `.js`；`.gitignore` 只忽略 `front/node_modules/`，**没有根 `node_modules/`** | 实测 `front/` 之外**无任何 `.js` 文件** → **今天无影响 ✓**。但根 package.json 使未来任意新增 `.js` 都被当 ESM（CommonJS 脚本会直接报错）；若有人在根跑 `npm install` 会生成未被忽略的 `node_modules/` | 把 package.json 移到 `front/`（更贴合「前端门禁」的归属），或在 `.gitignore` 补 `node_modules/` |
| **R-12** | 低 | `front/tests/dom-stub.mjs:386-393` | stub 的 `document` **没有 `addEventListener`** | 实测 `typeof D.addEventListener === "undefined"`。真实 DOM 有。当前代码不用 → 无影响；将来加 `document.addEventListener("DOMContentLoaded", …)` 会直接 TypeError（**假红**而非假绿，可接受） | 补一个最小实现（`DOMContentLoaded` 触发一次即可） |
| **R-13** | 低 | `front/tests/dom-stub.mjs:395-401` | `dom.window.location` 不存在（只设了全局 `location`） | 实测 `undefined`。当前代码用全局 `location` ✓，但 `window.location` 是浏览器里最自然的写法 | `StubWindow` 补 `location` 引用 |
| **R-14** | 低 | `front/tests/dom-stub.mjs:160-164` | `textContent` 赋值后 `innerHTML` 返回 `""`（真实浏览器返回内容） | 实测 `b.textContent = "raw"` → `b.innerHTML === ""`，真实浏览器是 `"raw"` | 修 stub：赋值时同步 `_html` |
| **R-15** | 低 | `front/views/m01_workbench.js:245-262` | `switchView` **同视图分支不调 `restoreUiState`**，与正常切换分支（`:298`）不一致 | 同视图重入时 render 重建 DOM → 滚动位置被重置。变异测试：去掉 `showPanel` 会被抓住（20/1），**去掉 `restoreUiState` 不会**——无对应断言 | 同视图分支补 `restoreUiState(target.hash)` |

### 8.2 推测级观察（未实测 / 未在本轮验证）

| 项 | 推测 | 依据 |
| --- | --- | --- |
| seed 按钮在真实浏览器点击未送达 | **判定为工具问题**的结论我无法独立验证（浏览器工具的合成点击未触发页面事件，含用户自加的全局 click 捕获也未触发）。门禁侧覆盖了按钮 → `riscv:seed` 信号 → store 更新的完整链路（`frontend.test.mjs:193-215`），逻辑上等价；但**真机路径未被独立确认** | 复审未复现真机点击，仅复核了门禁覆盖范围 |
| `onDelta` 的重复追加（R-07） | **推测**：当前 `subscribe` 无调用方、`onDelta` 无调用方 → 不可达。M-07 接线时需重新评估 | 代码路径静态分析 |
| 真实浏览器下 `terminalScrollTop` 的还原 | **推测**：stub 的 `scrollTop` 是普通属性，真实浏览器中 `scrollTop = 42` 在内容不足 280px 时会被**钳制为 0** → 还原可能静默失效。需真机 + 足够长的终端内容才能确认 | CSS 事实 + 浏览器滚动语义 |
| 72 处文档改写的**全部**语义 | 抽检了 SAB/COOP-DEP 校正（**准确**）、部署形态（**一致**）、file:// 残留表述（**已清扫**）；其余 60+ 处未逐条核 | 抽检，非全量 |

### 8.3 变异测试矩阵（16 组 · 复审独立执行）

| 变异 | 门禁结果 | 判定 |
| --- | --- | --- |
| D-01：m02 重新 import 未导出的 `activeView` | 0 pass / **2 fail** | ✅ 抓住 |
| D-02：删 `mountViewStore` 导出 | 0 pass / **2 fail** | ✅ 抓住 |
| #3：恢复 `row` 前置判断 | 20 / **1 fail** | ✅ 抓住 |
| #4：恢复终端双层转义 | 20 / **1 fail** | ✅ 抓住 |
| #5：移除 `syncTabState` 调用 | 19 / **2 fail** | ✅ 抓住 |
| #6：恢复 `switchView` 的 noop 早返回 | 19 / **2 fail** | ✅ 抓住 |
| #7：移除 `captureUiState` | 20 / **1 fail** | ✅ 抓住 |
| #13：删 `file://` 提示 | 20 / **1 fail** | ✅ 抓住 |
| #19：同视图分支去掉 `showPanel` | 20 / **1 fail** | ✅ 抓住 |
| #20 + 数组顺序：双失效 | 19 / **2 fail** | ✅ 抓住 |
| #16：把 `describe(` 改名（门禁自检） | 1 / **1 fail** | ✅ 抓住 |
| **#10**：`createPanel` 改回 innerHTML 字符串 | **21 / 0 fail** | ❌ **盲区** |
| **#11**：状态栏改回硬编码 | **21 / 0 fail** | ❌ **盲区** |
| **#12**：页脚改回硬编码 | **21 / 0 fail** | ❌ **盲区** |
| **#14**：改回 `mountedViews.get(role)` | **21 / 0 fail** | ⚪ 等效变异（当前两种写法行为相同） |
| **#2**：Promise.all 中间插入一个 mount（下标错位） | **21 / 0 fail** | ❌ **盲区** |

**结论：11/16 组被抓住，3 处真盲区（#10 / #11 / #12），1 处等效变异，1 处位置耦合盲区（#2）。**

### 8.4 复审确认闭环无误的项

| 项 | 复核方式 | 结论 |
| --- | --- | --- |
| D-01 修法 | 变异测试 | ✅ 选「m02 删掉用不到的 `activeView` import」而非新增 export——最小改动、不新增公开 API，门禁能抓住回归 |
| D-02 | 变异测试 | ✅ |
| #3 / #4 / #5 / #6 / #7 | 变异测试 | ✅ 门禁全部能抓住 |
| #9 escape 单点化 | 变异测试 | ✅ 源码文本计数断言能抓住 |
| #13 file:// 提示 | 变异测试 | ✅ 静态断言能抓住 |
| #19 / #20 | 变异测试 | ✅ 双失效组合能被抓住 |
| 时序双保险 | 变异测试 | ✅ 冗余且互补：数组顺序保证同步订阅先注册，`queueMicrotask` 保证异步 mount 也不错过；门禁锁「结果」而非「机制」——这是**好门禁设计** |
| CI 可跑性 | 亲跑 | ✅ YAML 合法；`tools/check-front.sh` 的 exec 位已入库（`100755`）；`node --test "front/tests/*.test.mjs"` 的**引号 glob 在 Node 22 生效**（实测 21 passed） |
| `package.json` `"type":"module"` | 全仓 `find` | ✅ `front/` 之外无任何 `.js` 文件 → 今天零影响 |
| SAB / COOP-DEP 语义校正 | 文档抽检 | ✅ **准确**：COOP/COEP 是**服务器响应头**，`file://` 无服务器故无该头；SAB 可用性的真正条件是**跨源隔离**（COOP+COEP）。把条件从「file:// 下无 COOP/COEP 时」改为「无 COOP/COEP 时」是正确的，且与新的部署形态（目录 + 同源 HTTP）自洽 |
| 部署形态定案 | 文档抽检 | ✅ 目录形态 + 同源 HTTP 与 ES module 的 CORS 硬约束一致；AGENTS.md / CONTRIBUTING.md / 需求 v1.4 / AD v2.8 已同步；残留「file:// 直接打开」硬指标表述已清扫（仅存于历史说明与版本记录，属正确留痕） |
| 需求 v1.3 → v1.4 改名 | 交叉引用 | ✅ AGENTS.md / CONTRIBUTING.md 引用已同步，无悬空链接 |

### 8.5 建议的最小修补（按投入产出比）

| 优先 | 动作 | 成本 | 收益 |
| --- | --- | --- | --- |
| 1 | **R-01** 改具名对象解构 | ~3 行 | 消除位置耦合；这是唯一一处「代码当前正确但结构脆弱」的问题 |
| 2 | **R-02** 三条断言改为绑定真实状态值 | ~6 行 | 把 3 处盲区变成有效回归锁 |
| 3 | **R-09** 用例数统一为 21 / 6 suite | 4 处各 1 词 | 文档一致性 |
| 4 | **R-04** 给 `.view-body` 加 `overflow:auto` 或删掉 `body.scrollTop` 采集 | ~2 行 | 让「UI 交互态采集」这条 LLD §3.3 要求真正可验证 |
| 5 | **R-03** stub 遇未实现算符抛错 | ~5 行 | 防止未来写出静默假绿的选择器测试 |
| 6 | R-06 / R-07 / R-08 / R-10 ~ R-15 | 小 | 整洁与一致 |

### 8.6 复审边界声明

- 复审**只读**：`front/` 与 `crates/` 下**零改动**；变异测试在 `/tmp` 的副本中执行，已清理，仓库无残留。
- 本节记录的是**复审意见**，不代表已修复；修复由作者按 §八·5 顺序执行。
- 复审未覆盖：真实浏览器行为（Electron + HTTP 服务）、`tools/build-wasm.sh` 的 WASM 产物、CSS 渲染效果、跨浏览器兼容性。


### 8.7 复审意见闭环（2026-10-20 · 作者侧）

复审 §八·1 的 **15 项发现** + §八·5 的修补清单全部落地：3 处门禁盲区封住、1 处位置耦合消除、DOM stub 的 5 处保真度缺口补齐。门禁由 21 用例 / 6 suite 扩到 **35 用例 / 7 suite**。

| # | 严重度 | 状态 | 落点 | 验证 |
| --- | --- | --- | --- | --- |
| **R-01** | 高 | ✅ | `app.js` 改**具名**挂载：挂载项收进 `pending` 对象，键→值映射由同一对象派生（`Object.entries(pending).map(async ([k, p]) => [k, await p])`），不再有下标耦合 | 变异「插入一个 mount」→ 门禁**仍绿**（⚪ 无害，失败模式已不复存在）；变异「transport 槽位真实错绑」→ 门禁**变红** ✓ |
| **R-02** | 高 | ✅ | 三条断言改为绑定真实状态：页脚断言工作台快照的 role **与全部视图名**；状态栏断言 = `describeTransport(实际 transport)`，且推导随 transport 变化、**transport 缺失读起来不同**（否则错绑不可见）；另加「M-01 不得用 innerHTML 拼面板」静态断言 | 变异 #10 / #11 / #12 三项改回去 → 门禁**全部变红** ✓ |
| **R-03** | 中 | ✅ | stub 实现 `= / ^= / $= / *= / ~= / \|=` 六种属性算符；**未实现的语法显式抛错**（不再静默退化） | 变异「`^=` 退化为属性存在即匹配」→ 门禁变红 ✓ + stub 自测 1 条 |
| **R-04** | 中 | ✅ | `.view-body` 无 `overflow`、**不是滚动容器** → 删掉 `body.scrollTop` 采集（真实浏览器里恒 0 的死值），只保留真实滚动容器 `.terminal-out`；测试断言采集到的键集合恰为 `["terminalScrollTop"]` | 变异「恢复采集 body.scrollTop」→ 门禁变红 ✓ |
| **R-05** | 中 | ✅ | stub 支持 `once` / `capture` 选项与 `stopPropagation`；**冒泡阶段顺序修正为目标 → 根**（此前反了，父节点先于子节点触发，致 stopPropagation 失效） | 变异「摘掉 once 支持」→ 门禁变红 ✓ + stub 自测 2 条 |
| **R-06** | 中 | ✅ | `captureUiState` 改函数式派生，`ui` **按键合并**而非整块替换 | 变异「改回整块替换」→ 门禁变红 ✓ |
| **R-07** | 低 | ✅ | `onDelta` 只写 store，DOM 写 store 的**权威值**（不再基于 DOM 累加） | 变异「改回 DOM 累加」→ 门禁变红 ✓（新用例模拟订阅者夹击重渲染） |
| **R-08** | 低 | ✅ | `app.js` 注释改为准确表述：数组顺序与 `queueMicrotask` 是**两道冗余保险**，各自的作用分别写明 | — |
| **R-09** | 低 | ✅ | 用例数统一为 **35 用例 / 7 suite**（AGENTS.md / PROGRESS.md / 台账 / 本文件） | 亲跑 → `# tests 35 / # suites 7` |
| **R-10** | 低 | ✅ | `tools/check-front.sh` 增加 Node 主版本 ≥ 22 的显式校验（否则 Node 18/20 上报出难以理解的错） | — |
| **R-11** | 低 | ✅ | `package.json` 移到 `front/`（`"type":"module"` 只影响 front/ 下的 `.js`，仓库根不再被波及）；`.gitignore` 补根 `node_modules/` | `front/` 内 `npm run test:front` 与根目录 `tools/check-front.sh` 均 35 passed ✓ |
| **R-12** | 低 | ✅ | stub 的 `document` 支持 `addEventListener`，装好 DOM 后于微任务里派发一次 `DOMContentLoaded` | stub 自测 ✓ |
| **R-13** | 低 | ✅ | stub 的 `window.location` 指向与全局 `location` 同一对象 | stub 自测 ✓ |
| **R-14** | 低 | ✅ | stub 的 `textContent` 赋值同步 `_html`（转义后的文本，与真实浏览器一致） | stub 自测 ✓ |
| **R-15** | 低 | ✅ | `switchView` 同视图分支补 `restoreUiState(target.hash)` | 变异「去掉同视图分支的 restoreUiState」→ 门禁变红 ✓ |

**变异矩阵复跑（10 组需变红 + 1 组预期无害）**：

| 变异 | 门禁 | 判定 |
| --- | --- | --- |
| #10 `createPanel` 改回 innerHTML 拼接 | 变红 | ✅ 抓住 |
| #11 状态栏改回硬编码（历史文案） | 变红 | ✅ 抓住 |
| #12 页脚改回硬编码（缺视图名） | 变红 | ✅ 抓住 |
| 插入一个 mount（位置耦合） | **仍绿** | ⚪ 预期无害——具名绑定下该变异不再产生错绑；真实错绑（transport 槽位）单独验证 → 变红 ✓ |
| stub `^=` 算符退化 | 变红 | ✅ 抓住 |
| stub `once` 支持被摘掉 | 变红 | ✅ 抓住 |
| `onDelta` 改回 DOM 累加 | 变红 | ✅ 抓住 |
| 恢复采集恒 0 的 `body.scrollTop` | 变红 | ✅ 抓住 |
| 同视图分支去掉 `restoreUiState` | 变红 | ✅ 抓住 |
| `captureUiState` 改回整块替换 `ui` | 变红 | ✅ 抓住 |
| 全部还原 | 绿 | ✅ |

> **复审建议的具名对象写法有一处笔误**：`Promise.all` 只接受可迭代对象，传普通对象会抛 `TypeError`。实现改为「挂载项收进具名对象 → 由 `Object.entries(pending)` 派生键→值映射 → `Promise.all` 解析」，同样消除下标耦合；对象字面量的求值顺序仍保证 `components` 先于 `workspace` 挂载。
>
> **本节只记录复审意见的闭环，不改复审结论本身**（§八·1~§八·4 为复审原文，保留原样以便对照）。
