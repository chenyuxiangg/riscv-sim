# STAGE-33 · v0.48 · 加载固件入口可见性批（规格 · 冻结）

> **本规格由统筹方独占修改。** 施工中每改一字，在 §8 记一条订正（格式：`### 订正 N · 标题`，每段含「规格原文 / 实测 / 已改成」）。
>
> **冻结日期**：2026-11-02 · **上一批**：STAGE-32（v0.47 · 固件加载 UI 批）

## §0 一句话

把「加载固件」这个入口从**藏在第二个标签页的最下面**挪到**页头常驻** ⇒ ⇒ 任何视图、任何时刻都一眼可见 ⇒ ⇒ 契约层零升版、内核与宿主零改动。

⚠️ **本批是「上一批做错了位置」的返工批**，不是新功能：STAGE-32 把入口做出来了，但做在 `product` 视图的**最末尾** ⇒ ⇒ 默认落地页是 `embedded` ⇒ ⇒ 用户打开页面**根本看不到** ⇒ ⇒ 用户原话：「界面上还是没有『加载固件』的按钮」。

---

## §1 范围与边界

### §1.1 本批做

1. **页头常驻**一个「加载固件」控件（`front/index.html` 的 `<header class="topbar">`）：文件选择器 + **「加载固件」按钮** + 一行**短状态**。
2. **两个入口共用同一份状态**（M-03 视图状态 `product.imageLoad`）⇒ ⇒ 顶栏读它、产品视图写它 ⇒ ⇒ **不是第二个真相**。
3. **「读字节 + 存 pending」那段从视图层提到共享组件**（`front/views/components/image-loader.js`）⇒ ⇒ 顶栏（app 层）与产品视图（视图层）**共用一段** ⇒ ⇒ **不写第二套**。
4. **命令处理抽成一个函数**（`handleLoadImageCommand`）⇒ ⇒ 视图层的 `riscv:command` 与页头按钮**共用它** ⇒ ⇒ **一条装载路径**。
5. **真机门禁加判据**：**默认落地页上就有这个入口** + **页头这条路径真能装载**。

### §1.2 本批不做

- ⛔ **不改默认落地页**（仍是 `embedded` 工作台）—— 入口常驻就够了，动路由是另一件事。
- ⛔ **不删产品视图里那块面板**（它保留在原位；状态同源）。
- ⛔ **不改内核 / 宿主 / 契约层**，⛔ **不升任何版本号**。
- ⛔ **不支持 IHEX**（沿用 STAGE-32 的已知边界）。
- ⛔ **不自动跑**（沿用「装完就停」）。
- ⛔ **不给顶栏造第二份状态**（顶栏只**读** `product.imageLoad`）。

---

## §2 事实表（F1~F12 · 实测）

| 号 | 事实 | 依据 |
| --- | --- | --- |
| F1 | **默认落地视图是 `embedded`**，不是 `product` | `front/views/m01_workbench.js:163` 的 `let currentRole = "embedded"`；`?role=` 才可覆盖 |
| F2 | 加载器面板挂在 `productTreeView.render()` 的**最末尾** | `front/views/m02_viewcomponents.js:172` 的 `${renderImageLoader(state?.imageLoad)}` 在 `product-grid` **之后** |
| F3 | 真浏览器实测（`http://localhost:8123/#/product`）：产品视图页面高 **1132 px**，文件选择器在 **y = 763** ⇒ ⇒ **不滚动看不到** | 真 Chrome 1280×720 实测（元素 rect） |
| F4 | **app 层已经在管页头的 DOM**（`#seed-demo` 按钮） | `front/app.js` 的 `wireDevSeeding()`：`document.getElementById("seed-demo")` |
| F5 | `m03_viewstore.js` 的 `reset()` **会清空订阅者**（`subscribers.clear()`）⇒ ⇒ 挂在 `subscribe` 上的同步会**永久失联** | `front/views/m03_viewstore.js` 的 `reset()` 实现（⚠️ 今天**没人调**它 ⇒ ⇒ 是**潜在**脆弱，不是现网 bug） |
| F6 | `riscv:render` 是 M-01 的重渲染事件（派发在 `window` 上）⇒ ⇒ **不受 `reset()` 影响** | `front/views/m01_workbench.js:483` |
| F7 | 真机门禁的探针用 `document.querySelector("[data-image-input]")` 与 `querySelector('[data-action="load-image"]')` ⇒ ⇒ **页头在 DOM 里排在 `main` 之前** ⇒ ⇒ 页头一旦也放输入框，**这两条查询会命中页头那个** ⇒ ⇒ 判据口径会**悄悄变** | `tools/browser-verify.mjs` 的 `IMAGE_LOADER_PROBE`；`front/index.html` 的 `<header>` 在第 10 行、`<main>` 在第 25 行 |
| F8 | 产品视图的状态条用的是 `data-image-load-status` ⇒ ⇒ 顶栏的状态条**必须换个属性名**，否则又撞 | `front/views/components/image-loader.js` 的 `renderImageLoader` |
| F9 | `pendingImage()` / `loadImageFileIntoPending()` 现在住在**视图层**（`front/views/m02_viewcomponents.js`）⇒ ⇒ 顶栏（app 层）**够不着** ⇒ ⇒ 要共用就得**提到共享组件** | `front/views/m02_viewcomponents.js:263` / `:282` |
| F10 | app 层的 `case "load-image"` 是**唯一**把字节变成工程并装载的地方 | `front/app.js:930` |
| F11 | 契约层的 `ImageSource::Bytes` **早就够用** ⇒ ⇒ 本批**零**契约变更 | `crates/contract/src/model.rs` |
| F12 | 用户的判据原话：「界面上还是没有『加载固件』的按钮」⇒ ⇒ 判据 = **默认落地页上就有这个按钮**，不是「某个视图里有」 | 用户反馈（v0.47 合 main 之后） |

---

## §3 裁决

| 号 | 裁决 |
| --- | --- |
| A | 入口放**页头**（`.topbar`）⇒ ⇒ 任何视图可见 ⇒ ⇒ **不改默认落地页**（F1） |
| B | 页头按钮的文案 = **「加载固件」**（与用户原话逐字一致 ⇒ ⇒ 用户一眼认得出） |
| C | 两个入口**共用同一份状态**（`product.imageLoad`）⇒ ⇒ 顶栏**只读**它 ⇒ ⇒ **不是第二个真相** |
| D | 「读字节 + 存 pending」那段**提到共享组件** `front/views/components/image-loader.js` ⇒ ⇒ 视图层与 app 层**共用一段**（F9）⇒ ⇒ **不写第二套** |
| E | 页头的 `change` 监听挂在 **app 层**（页头不属于任何视图面板 ⇒ ⇒ 视图层**管不到**）⇒ ⇒ 且 app 层**已经**在管页头（F4） |
| F | 页头的「加载固件」按钮**直接调**命令处理函数 ⇒ ⇒ **不绕** `riscv:command`（那是视图层→app 层的通道；页头本来就是 app 层） |
| G | 命令处理**抽成一个函数**（`handleLoadImageCommand`）⇒ ⇒ 视图层的命令与页头按钮**共用它**（F10）⇒ ⇒ **一条装载路径**（`loadProjectIntoKernel`） |
| H | 顶栏的状态条用**另一个属性名**（`data-image-brief`）⇒ ⇒ 不与产品视图的 `data-image-load-status` 撞（F8） |
| I | 两个输入框用 `data-image-load-from` **按位置取值**（`topbar` / `product`）⇒ ⇒ 真机门禁的选择器**无歧义**（F7） |
| J | 顶栏状态同步挂在 **`riscv:render` 事件**上 ⇒ ⇒ **不**挂 `subscribe`（F5：会被 `reset()` 永久清掉）⇒ ⇒ 挂事件（F6）不受影响 |
| K | 页头入口**不发** `ContinueRun`（装完就停，与 STAGE-32 裁决 D 一致） |
| L | 契约层**零升版**、内核与宿主**零改动**、零新 DTO / 零新命令 / 零新事件（F11） |
| M | 产品视图里那块面板**保留在原位**（用户已定案「页头常驻」⇒ ⇒ **不做**「页头 + 视图置顶」那个变体） |

---

## §4 验收

### §4.1 前端门禁（A 组 · Node 侧）

| 号 | 验收 |
| --- | --- |
| A-1 | 页头渲染出**一个** `<input type="file">`（`data-image-load-from="topbar"`）与一个**「加载固件」**按钮 |
| A-2 | 两个入口的 `data-image-load-from` 取值**互不相同**（`topbar` / `product`）⇒ ⇒ 门禁选择器无歧义 |
| A-3 | 页头选文件后**发出一条** `load-image`（或等价调用），载荷含**字节**与**文件名** |
| A-4 | 「读字节 + 存 pending」那段**只有一处实现**（视图层与 app 层共用同一函数） |
| A-5 | 页头的「加载固件」按钮走**同一条**命令处理函数（与视图层那条同一个） |
| A-6 | 顶栏状态条显示**文件名与字节数**（与产品视图那份**同源**） |
| A-7 | 装载后**五条流全部重订**（寄存器 / 状态增量 / 探针 / console / 内存图） |
| A-8 | **不自动跑** ⇒ ⇒ 装载后**没有** `ContinueRun` 命令发出去 |
| A-9 | 装载失败时显示**内核原文**（含错误码），⛔ 不吞 |
| A-10 | 契约层**零变更**且四个版本常量**一个都没动** |

### §4.2 真机门禁（B 组 · 真 Chrome）

| 号 | 验收 |
| --- | --- |
| B-1 | **默认落地页（`embedded`，不切到 `#/product`）上，页头就有「加载固件」按钮**⇒ ⇒ **这正是用户这次的判据**（F12） |
| B-2 | 页头按钮的文案**逐字**是「加载固件」 |
| B-3 | 真机里**在页头**真选文件（CDP `DOM.setFileInputFiles` 打到 `input[data-image-load-from="topbar"]`）⇒ ⇒ `change` 真触发 ⇒ ⇒ 页头状态条显示**文件名与字节数** |
| B-4 | 点页头的「加载固件」⇒ ⇒ **真装载** ⇒ ⇒ 内核换成了夹具固件（寄存器特征值变了 + 终端出现 `UIL`） |
| B-5 | 装载后**不自动跑**（⛔ 没有 `ContinueRun` ⇒ ⇒ PC 不动） |
| B-6 | 页头这条路径**没有**绕过界面（⛔ 不许用脚本口代替 ⇒ ⇒ 否则 B-3 / B-4 恒绿） |

### §4.3 交叉（C 组）

| 号 | 验收 |
| --- | --- |
| C-1 | 契约层**零变更**（`git diff` 里 `crates/contract/` 为空） |
| C-2 | 内核 crate **零变更**（`git diff` 里 `crates/kernel/` 为空） |
| C-3 | 四个版本常量**一个都没动**（`SCHEMA_VERSION` = **8** · `SIM_STATE_SCHEMA_VERSION` = **2** · `PLATFORM_SCHEMA_VERSION` = **1** · `PLATFORM_RULES_VERSION` = **3**）—— ⚠️ **本行是这四个值的唯一权威处** |
| C-4 | 前端镜像的 `SCHEMA_VERSION` 仍 **8** |
| C-5 | `tools/error_code_check.py` **0 发现**（前端没新造码） |
| C-6 | 源码视图的**源码树仍不进内核**（本批没顺手改坏那条老规矩） |

---

## §5 台账

本批**不销账、不新登记**（平台能力增强，不是欠账闭环）⇒ ⇒ **真实未闭环数不变**。

⚠️ 但**要记一条教训**（写进台账的批次叙事）：**「功能做出来了」不等于「用户找得到」** ⇒ ⇒ STAGE-32 的判据只验「某个视图里那个入口能不能用」，**没验「用户打开页面能不能看见」** ⇒ ⇒ 真机门禁必须有一条**站在默认落地页上**的断言（B-1）⇒ ⇒ **判据要问用户真正会问的那个问题**。

---

## §6 判别力清单

落地前必须先钉住「**能变红的**」：

| # | 判据 | 变红条件 |
| --- | --- | --- |
| 1 | B-1（默认落地页上就有入口） | 把页头控件删掉 ⇒ ⇒ 在默认视图上找不到它 |
| 2 | B-3（页头真选文件） | 把页头的 `change` 监听摘掉 ⇒ ⇒ 状态条永远是「未选」 |
| 3 | B-4（页头这条路径真装载） | 把页头按钮的 handler 改成 `return` ⇒ ⇒ 内核没换镜像 |
| 4 | A-4（「读字节 + 存 pending」只有一处） | 在视图层**另写一份** ⇒ ⇒ 两处实现会漂移 |
| 5 | A-5（共用同一个命令处理函数） | 页头另开一条装载路径 ⇒ ⇒ 重订流与清残留会分叉 |
| 6 | A-8（**不**自动跑） | 装载后加一次 `ContinueRun` |

⚠️ **B-1 的判别力来源**：它站在**默认落地页**上 ⇒ ⇒ 只在 `product` 视图里找得到入口的实现 ⇒ ⇒ **必然红** ⇒ ⇒ 这正是 STAGE-32 缺的那一条。

---

## §7 变更影响面

| 文件 | 改什么 |
| --- | --- |
| `front/index.html` | 页头加**挂载点** `<span data-image-load-slot>`（控件本体由共享组件渲染 ⇒ ⇒ `accept` 列表与按钮文案**只有一处**） |
| `front/style.css` | 页头控件的样式 |
| `front/views/components/image-loader.js` | 新增 `renderImageLoaderControl` / `describeImageLoadShort`；**搬入** `pendingImage` / `loadImageFileIntoPending` |
| `front/views/m02_viewcomponents.js` | 改成 import 那两个搬走的函数；产品视图的输入框加 `data-image-load-from="product"` |
| `front/app.js` | 抽 `handleLoadImageCommand`；接页头的 `change` 与按钮；挂 `riscv:render` 同步顶栏状态 |
| `front/tests/load-image.test.mjs` | **加** A 组用例 |
| `tools/browser-verify.mjs` | **改**选择器（按位置）+ **加** B 组断言 |
| `tools/stage33_check.py` | **新增** · 本规格自检 |
| `docs/reviews/REVIEWS.md` | 台账加本批批次记录 + 那条教训 |

⚠️ **契约层 / 内核 / 宿主：一行都不改**（C-1 / C-2）。

---

## §8 施工期订正

（施工中每改规格一字，在此记一条。格式：`### 订正 N · 标题`，每段含「规格原文 / 实测 / 已改成」。）

---

## §9 变异自检

| 号 | 变异 | 期望变红 |
| --- | --- | --- |
| T1 | 把**页头控件**整块删掉 | B-1 / B-2 / B-3 / B-4 |
| T2 | 把**页头的 `change` 监听**摘掉 | B-3 |
| T3 | 把**页头按钮的 handler** 改成 `return` | B-4 |
| T4 | 把 `handleLoadImageCommand` **复制一份**给页头用（两条装载路径） | A-5 |
| T5 | 装载后**加一次** `ContinueRun` | A-8 |
| T-C1 | **对照**：把页头按钮的文案从「加载固件」改成「装载」（语义不变） | **0 红** |

⚠️ **对照变异必须 0 红** —— 否则说明那条路根本没被覆盖。

---

## §10 自检脚本

`tools/stage33_check.py`（与 `stage32_check.py` 同构）：七项 + 7b~7i。

| 项 | 判什么 |
| --- | --- |
| 1~3 | Markdown 表格列数一致 / 标题层级不跳级 / 代码围栏配平 |
| 4 | 裁决编号 A~M 无悬空引用 |
| 5 | 章节 §0~§10 完整 |
| 6 | 验收编号交叉引用无未定义项（A-1~A-10 / B-1~B-6 / C-1~C-6） |
| 7 | 关键约束字面量钉住 |
| 7b | §1.2 的事实表编号 **F1~F12 连续** |
| 7c | §8 的每条订正**自己那段里**都有「已改成」 |
| 7d | 四个版本号的值（**§4.3 的 C-3 行是唯一权威处**） |
| 7e | **页头必须有加载入口**：`front/index.html` 里必须出现 `data-image-load-from="topbar"` 与「加载固件」 |
| 7f | **app 层必须有页头接线**：`front/app.js` 里必须出现 `topbar` 与 `handleLoadImageCommand` |
| 7g | **两个入口必须能区分**：`front/views/components/image-loader.js` 里必须同时出现 `data-image-load-from`、`"topbar"`、`"product"`（两个位置的取值**定义在**同一处）与「加载固件」文案 |
| 7h | 台账必须登记本批（`v0.48` + `STAGE-33`） |
| 7i | `tools/stage33_check.py` 自己**必须存在**（自指） |
