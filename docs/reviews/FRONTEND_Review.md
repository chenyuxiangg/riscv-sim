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
> **16 项全部闭环**，另捕获并修复 **4 项评审漏项**（#17 / #18 见 §七·2，**#19 / #20 见 §七·2.1**）；新增前端门禁 **21 用例 / 7 suite 全绿**（`tools/check-front.sh`），并以 **10 组变异测试**证明门禁能抓住同类回归。
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
| 16 | 低 | ✅ | 新增 `front/tests/dom-stub.mjs` + `front/tests/frontend.test.mjs`（19 用例）+ `tools/check-front.sh` + `.github/workflows/ci.yml` | `tools/check-front.sh` → 19 pass / 0 fail |
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

新增门禁（`tools/check-front.sh` → `node --test "front/tests/*.test.mjs"`）：**19 用例 / 6 suite / 0 失败**，零第三方依赖，Node ≥ 22。

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
