# Project Progress · 项目进度

> **本文件跟踪「当前需求的完成情况」**；逐版本详细变更日志已归档至 [`PROGRESS-history.md`](./PROGRESS-history.md)（同源，git history 是最终真相）。
> **欠账的真相在台账** [`../reviews/REVIEWS.md`](../reviews/REVIEWS.md)；批次顺序与依赖在 [`../plans/PRODUCT_BOARD_PLAN.md`](../plans/PRODUCT_BOARD_PLAN.md)。

- **当前里程碑**：**v0.116**（2026-11-21 · 装配层拆分 Layer B12：协议桥外壳 4 方法入新 `bridge.rs`）
- **下一批**：**第 62 批**（主题待定）· **仍欠**：门禁工具改造 [`GATE_TOOLING_PREREQ.md`](../plans/GATE_TOOLING_PREREQ.md) 的 **R-1~R-4**（本批按用户拍板先行，欠账顺延）
- **Owner**：zyzs

---

## §1 当前需求跟踪表

> 来源：产品计划 §3 批次序列。**四段链路**：L1 读 · L2 写改板 · L3 写连线 · L4 固件↔板绑定。

| 批 | 版本 | 主题 | 状态 | 里程碑 |
| --- | --- | --- | --- | --- |
| 52 | v0.68 | A + H：产品树改成「板定义的编辑面」+ 类型清单只查内核 | ✅ 已落地 | **L2 通了** |
| 53 | v0.69 | 支线：陈旧面收尾（D-176 C+B · D-173 A · D-174 B） | ✅ 已落地 | D-174 做法 A 仍挂起 |
| 54 | v0.70 | B1：基址规划面板（用户自规划，平台提供入口） | ✅ 已落地 | B2（内核分配）未做 |
| 55 | v0.71 | C：「未知系统」门禁（内核判定，`E_PLT-17`） | ✅ 已落地 | **L4 通了** |
| 56 | v0.72 | **D-177 A+B**：宿主不再塌缩器件实例（丢线可见 + 消费实例表） | ✅ 已落地 | **L3 地基补上** |
| **57** | **v0.73** | 描述格式一次升版（B2 + G + D1 + D-177 的 C） | ✅ 已落地 | 描述形状定版（⚠️ 破坏性：`PLATFORM_SCHEMA_VERSION` 1 → 2） |
| **58** | **v0.74** | **D-175 的 D + E**（引脚级连线 + 通用连接路由）+ **D-177 的 D**（前端连线编辑面） | ✅ 已落地 | **L3 通了**（⚠️ 破坏性：`PLATFORM_SCHEMA_VERSION` 2 → 3） |
| **59** | **v0.75** | **D-175 的 F：MCU 概念**（ISA/厂商/型号/向量表） | ✅ 已落地 | **装载时按板 ISA 校验固件**（位宽 + RVC 单向 `c` ⇒ `E_PLT-20/21`）+ 板级视图**用户自配**（内置板=「从内置模版开始」模板入口）+ 基址 **16 进制**（⚠️ 破坏性：`PLATFORM_SCHEMA_VERSION` 3 → 4） |
| 60 | v0.76 | 模板导入 + 不兜底生效板 · 引脚的复用功能 / 方向 + 固件动态切换（pinmux 器件） | ✅ 已落地 | 平台不兜底（auto_selectable 废弃 · 无板装载拒 `E_PLT-22` + 引导 · 内置板降级为模板预设 · 前端「从模板导入」）· 引脚复用（`pins[].functions[]` 功能列表 · 独立 pinmux 器件 · 中断绑「引脚 × 当前功能」· 前端只读展示）· ⚠️ 破坏性：描述格式 4 → 5（`PLATFORM_SCHEMA_VERSION` 4 → 5） |
| **61** | **v0.77** | **每区域预算整体退役**（STAGE-53 · 方案 A 根治：段 size 即容量） | ✅ 已落地 | 删 `region_budget_bytes` / `max_region_bytes` 两个键 + 删 V-7/V-16 + 删 `MemoryMapOp::SetBudget` + M-14 去掉 `DEFAULT_MAX_REGION_BYTES`（改为按**本段 size 分页取整**的容量）· 前端删预算列 / 差量 / 三个死渲染函数 + 补错误横幅与存完刷新档案 · 门禁脚本 token 同步 · ⚠️ 破坏性：描述格式 5 → 6、信封 15 → 16（`E_PLT-07` 保留号 retired） |
| 62 | v0.78 | M-16 线 2（UART tx-empty）退役 | ✅ 已落地 | 删 `IRQ_UART_TX_EMPTY` 及其两处映射分支 · 线号收敛为 `1`（外部）/ `3`（timer）/ `4`（software），**线 2 空缺不重排** · 真 16550 只有一根 IRQ（RX/TX 共源同 `MEIE` 位）· 新增三条钉「线 2 无位号 / 无 cause / 不投影 mip」 |
| 63 | v0.79 | 中断波形通道 + 前端筛选（STAGE-54） | ✅ 已落地 | 波形改为**每条中断一个 0/1 电平通道**：核 3 条（`software · IRQ 3` / `timer · IRQ 7` / `external · MEIP`）+ **每台已接线设备 1 条**（`uart0 · IRQ 10`，按 `peripheral_irqs` 现算 ⇒ 加器件自动出现）· 删「中断脉冲」通道（把线号当值画的那条）· 抬线/撤线回调写 1/0，设备级取 PLIC 网关电平 · 前端分组筛选（默认**只勾核中断**，全选/仅核/清空），选择按**流 id**保留 ⇒ 平台变更不丢 |
| 64 | v0.80 | 门禁工具改造 R-1~R-4（+R-7 计时） | ✅ 已落地 | `tools/browser-verify.mjs`：`--only/--from` 分段跑（未选中段标 SKIP）· 失败断言挂 `evidence=`（原始值 + DOM 快照路径 + 最近 6 次求值）· 采集通道自检（`E_HARNESS_CHANNEL` · 退出码 2，与产品失败分开）· wasm 比源码旧时自动重建 · `[timing]` 段级计时 · 分段跑**永不**报「通过」（改报「定位跑通过 N/M 段」） |
| 65 | v0.81 | 波形「右端保持点」（修「只有上升沿、看着断了」） | ✅ 已落地 | `front/views/components/wave-chart.js` 增 `holdToEnd`/`holdPointCount`：通道只在电平变化才有样本 ⇒ `step:"end"` 只在**样本之间**保持 ⇒ 抬了一直没落的线画到最后一个样本就停 ⇒ 补**一个**右端保持点（`t`=全局最右样本、`v`=该通道最后的值，**不新增跳变**）；界面把「保持点数」单独报 `data-wave-chart-hold-points`，`data-wave-chart-points` 仍只报真样本 ⇒ 门禁 B-29 判别力不变（实测 点数 512 = 样本 512、边沿 600） |
| 66 | v0.82 | UART FIFO 触发水位 + 字符超时 + 中断仲裁对齐规范 | ✅ 已落地 | `uart.rs`：`fifo_enabled()`/`rx_trigger_level()`/`rx_interrupt_pending()`（`FE=0` 走 16450「非空」，`FE=1` 走「深度 ≥ 水位 ∨ 字符超时」）+ `tick` 里 `4 × 10bit/baud` 挂 `CTI`（IIR `0b1100`，读 RBR/IIR 清）；`kernel_host` 的 UART 源电平改调 `rx_interrupt_pending()`（不再写死 `IER∧LSR`）；M-16 `default_priority` 改成规范序 `MEI(2) > MSI(1) > MTI(0)` |
| 67 | v0.83 | 门禁种板入口迁移（`#/memory-map` 已删） | ✅ 已落地 | `activateBoard`/`waitForBoardApplied`/`seedBoard` 三件套（模版点选 ⇒ `load-archive` 兜底，三条独立证据任一）· 26 处 `submit-memory-map` ⇒ `load-archive` · 30 处 `viewId` ⇒ `product` · 23 处残留路由 ⇒ `#/product`；`--only loadImage` 122 s → 23 s |
| 68 | v0.84 | 门禁用例有效性审计 + 快失败/裁剪 | ✅ 已落地 | `uartEcho` `4 × 90 s` ⇒ 30 s 快失败（380 s → 30.3 s）· `terminalScreen` 回显改前置（与 uartEcho B-63 同义，裁 1 条）⇒ 全量 1006 s/64 ✗ ⇒ 659 s/43 ✗ |
| 69 | v0.85 | 门禁失效用例按**裁决 B**重写（37 → 36 段） | ✅ 已落地 | `boardTree` 退役并入 `deviceRegistry`（两段都在断「产品视图挂上了」）· `memoryMap` 重写为「模版对话框 + 加载生效」· `deviceRegistry` 改读内核 `DeviceTypes`/`DeviceInstances` 事件（DOM 卡片组件是**只 import 不调用**的死代码）· `boardArchive` 修 `BV_BOARD_ARCHIVE_PROBE` **未定义标识符**（整段 `ReferenceError`）· `boardDelta` 数据源改内核 `MemoryMap` 帧 +「修订号 +1」改成「前进」+ T-4 整份路径改用本段变体（旧源随编辑器消失 ⇒ `E_PROTO-08`）⇒ 四段 4/4 · 3/3 · 4/4 · 11/11 全绿；全量 540 s/39 ✗ |
| 71 | v0.87 | 带 osz 的全量**也** `0 ✗`（夹具板 PLIC 源号 1 → 10 · B-63/B-86/B-144 判据去瞬时依赖） | ✅ 已落地 | **根因（实测）**：夹具板 `BV_MCU_BOARD` 把 UART0 的 `irq_line` 写成 **1**，而 osz 按 qemu-virt DTS 用 PLIC **源 10**（`UART_INT_NUM(26)-16`），内置 `qemu-virt.json` 本来就是 10 ⇒ 源号错位 ⇒ `deliverable_source` 永远 None ⇒ 字节进 FIFO（`LSR=0x63`）但 guest 一次不读（等待后仍 `0x63`）、`IIR=0x04` 证明设备侧条件成立 ⇒ 回显假红。**改动**：①夹具 `irq_line: 1 → 10`；②`B-63` 失败现场取证（发送后 LSR/IIR、等待后 LSR、PLIC enable/pending/prio/threshold）；③`B-86` 采样从「睡 900 ms 一次」改**轮询 3 s + 最多补切一次路由**，并在点候选前**清掉条件框**（`symbolUi` 段 B-79 故意留的非法条件 `x1 ==` 会被候选按钮带上去 ⇒ 内核合法拒 ⇒ 列表空 ⇒ 假红）；④`B-144` 的判据从瞬时的 `.pw-pending` 改成「生效板 id 真的变成这块板」（`waitForBoardId`）。**验证**：`OSZ_ELF=… 全量` ⇒ **31/31 段、0 跳过、0 ✗、rc=0**，8 条已知缺陷单列；`B-63 matched=true batches=201`、等待后 `LSR=0x62`、`pending=0x0` |
| 76 | v0.92 | **按影响面选测**（R-8）：改完只跑受影响的段 / 用例（拿不准一律全量） | ✅ 已落地 | **动机**：全量门禁 4~15 分钟、前端 758 条 ⇒ 改一行也跑全量 ⇒ 「改完立刻验」成本上不成立 ⇒ 红点攒到收口才暴露。**做法**：新增 `tools/impact.mjs` 出「计划」（段 / 前端用例 / cargo 包 / 是否动文档）；门禁加 `--impact`（`RUN_POLICY.onlySet`，被跳过的段**显式**标 `SKIP（--impact ⇒ 本段不在影响面内）`）；`tools/check-front.sh --impact` 只跑选中用例。**两条硬规矩**：① 只许缩小必跑集合 —— 认不出来 / 扇出大 / 共享底座（`app.js`、`m02`、`crates/kernel` …）⇒ **收敛到全量**，计划工具失败也全量；② 前端用例集合**算出来**而不抄：按 `front/tests/*.test.mjs` 的静态 import 闭包（含 `importFront("lib/x.js")` 与「正文点名路径」）推导。**验证**：`impact.mjs --selftest` 8 条不变量全成立；`check-front.sh` 全量 **758/758**；`check-front.sh --impact` 在纯工具改动下正确跳过用例；真机 `--impact` 的收敛/窄跑在后续改动中实测（见台账下一行） |
| 77 | v0.94 | 源码路径匹配吸收目录选择器的顶层文件夹前缀（真机「解析当前 PC」代码区空白） | ✅ 已落地 | **根因（浏览器实测 + 离线比对）**：目录选择器给的 `webkitRelativePath` 带选中文件夹名作第一段（`osz/targets/.../main.c`），而 DWARF 绝对路径的 `comp_dir` 尾部是编译机目录名（`…/osz-main/targets/.../main.c`）⇒ 旧规则要求本地 key **整串**作后缀 ⇒ 首段 `osz ≠ osz-main` ⇒ 整树零命中。内核 DWARF 回包本身正确（`0x80000a18 → task.c:141`、`0x800003ac → main.c:86`），断在前端匹配这一层；代码区只剩「选中一个源码文件后显示内容」⇒ 用户观感是「点了没反应」。**改动**：`matchSourceFile` 抽出 `keySuffixMatches`，本地 key 从每个后缀起再按「较短串是较长串后缀」比（丢掉的只是本地前导前缀，DWARF 侧一段不丢）；歧义仍按裁决 F 列候选、不静默挑第一个。**验证**：`check-front.sh` 全量 **773/773**；隔离浏览器装 `lczyzs/osz.elf` + 仅带 `osz/` 前缀的源码树，`0x80000a18` 命中 `osz/kernel/base/task/src/task.c`、`0x800003ac` 命中 `osz/targets/qemu_virt/src/main.c`，代码区正常渲染 |
| 78 | v0.95 | 源码路径前缀映射表（用户自填「编译期前缀 ⇒ 本地前缀」） | ✅ 已落地 | **动机**：v0.94 吸收的是本地侧前导前缀；当编译期目录结构与本地树在中段就不一致（重命名目录 / 换工程根）时，纯后缀匹配仍会零命中 ⇒ 给用户一条显式重写的退路。**改动**：① 纯函数 `matchSourceFile(tree, dwarfPath, remaps?)` 新增第三参 `[{from, to}]`：`from` 命中 DWARF 路径段前缀后剥掉，`to` 留空 = 剥目录、填了 = 拼本地前缀；原始路径始终参与匹配，多条映射按序重写出候选变体，跨变体合并去重，歧义照旧列候选。② 源码视图加「路径前缀映射」折叠区：多条 `from ⇒ to` 输入行 + 添加 / 删除，`change` 落 M-03；纯前端资产，不发命令、不进内核、仅本会话（与源码树同一口径，不碰存储层）。**验证**：`check-front.sh` 全量 **779/779**；node 用例覆盖剥目录 / 拼前缀 / 前缀不匹配回退后缀 / 多条映射第二条命中 / 跨映射歧义；隔离浏览器实测添加、改字段、删除三条 UI 链路无 JS 报错 |
| 79 | v0.96 | 前端视觉改 hs-fpv 中性浅色风（白底中性灰阶 + 近黑主交互） | ✅ 已落地 | **动机**：原深蓝暗色 + 青色主题观感偏旧；参照 hs-fpv.com 的 shadcn 中性浅色风重做配色。**改动**：① `:root` 令牌整体换浅色：页面底 `#f4f5f7`、卡片/侧边栏白、内嵌块 `#f5f5f5`、边框 `#e5e5e5/#eeeeee`、文字三级 `#171717/#525252/#737373`；主交互 `--accent` 近黑 `#171717`（参考站 `--primary` 即近黑），琥珀 `#d97706` / 绿 `#16a34a` / 红 `#dc2626` 只做状态点缀；圆角 10px → 8px；字体走系统无衬栈（不引外部 Barlow，离线不跳字）；终端/追踪块保留深色面板。② 清掉 §20 产品工作区一批早已失效的旧令牌引用（`--ink/--ink2/--teal/--bg3/--err/--brand/--muted`）；12 处裸 `rgba` 写死色改 `color-mix()` 令牌派生；源码当前行高亮从 JS 内联样式收进 CSS `.src-line.is-current`；卡片加轻阴影、导航选中态改 6% 黑 wash。③ A-8 令牌钉值用例同步到新浅色规格。**验证**：`check-front.sh` 全量 **779/779**；隔离浏览器逐视图核对计算样式，工作台/调试/波形/产品/终端配色落点正确，无 JS 报错（仅预存在 favicon 404） |
| 80 | v0.97 | 按钮 / 卡片 / 输入框控件风格对齐 hs-fpv（胶囊折中 + 12px 卡片） | ✅ 已落地 | **取舍（用户拍板）**：按钮走「胶囊折中」——全圆角、34px 高、font-medium、近黑焦点环，不照搬落地页 44px；卡片走「折中 12px」——外层卡 12px 圆角 + 16px 内边距 + hover 边框加深，内嵌面板 10px，保住调试台密度。**改动**：① 令牌补圆角梯度 `--r/--r-md/--r-sm/--r-xs/--r-pill` 与加深边框 `--line-strong`；② 通用按钮改胶囊 34px、白底 neutral-300 边近黑字、primary 近黑反白、run 绿底反白，`focus-visible` 近黑双环；种子钮 / 断点钮 / 符号候选 / 终端发送 / 产品视图按钮统一紧凑胶囊；③ 外层卡 12px/16px 内边距 + 轻阴影 + hover 边框转 `--line-strong`；内嵌面板 10px；输入框统一 8px 圆角、对齐高度、`focus` 近黑边 + 浅色 focus ring；弹窗补浮起阴影；④ A-8 令牌钉值用例同步圆角梯度与 `--line-strong`。**验证**：`check-front.sh` 全量 **779/779**；浏览器实测按钮 34px/999px、卡片 12px/16px、内嵌面板 10px、输入框 8px，无 JS 报错 |
| 81 | v0.98 | `.source-input` 文件选择控件与源码工具条对齐浅色令牌风 | ✅ 已落地 | **改动**：① `.source-input` 从 `.image-loader-control` 作用域提为共享规则，覆盖源码目录 / 源码文件 / 固件镜像三处 file 选择；文件框统一 30px 高、`--r-sm` 圆角、`--panel` 底 + `--line-strong` 边，`focus` 近黑边 + `--accent-wash` 环；原生 `::file-selector-button` 改次按钮胶囊（`--r-pill`、12px、font-medium、hover `--panel2`）；② `.debug-toolbar` 补 flex 对齐；`.source-view` 内动作按钮统一紧凑胶囊（对齐 `.pw-btn` 外观）；③ 颜色 / 圆角全走 `:root` 令牌，无新增写死字面量。**验证**：`check-front.sh` 全量 **779/779** |
| 82 | v0.99 | 基础控件层收口：调试 / 波形 / 串口中断视图裸按钮与裸输入框适配令牌风 | ✅ 已落地 | **排查**：调试视图（反汇编 / 单步 / 断点 / 观察 / trace 导出 / 读寄存器 / 读内存）、波形视图（全选 / 仅核 / 清空 / 导出 VCD）、板级与中断编辑器里一批按钮没挂专用类，仍是浏览器原生默认外观；观察表达式 / trace 过滤 / 步数 / 中断号等文本框与下拉框同样裸奔。**改动**：① 新增元素级基础控件层——`button` 默认紧凑胶囊（30px、`--r-pill`、`--panel` 底 + `--line-strong` 边、12px/500、近黑 focus 双环），`input/select` 默认 30px、`--r-sm`、白底 + 浅边 + focus 环；专用类（`.btn/.pw-btn/.addr-field/.terminal-input/.reg-edit` 等）特异性更高照常覆盖；复选框 / 文件框排除文本框外观。② 补 `.terminal-raw-btn`（原始字节开关）胶囊外观与 `is-selected` accent wash 选中态。③ 踩坑订正：基础输入规则最初用 `input[type=text]` 属性选择器，特异性高于类名，把 `.terminal-input` 深色终端底盖成白底；改回元素级 `input/select` 后类规则正常生效。**验证**：`check-front.sh` 全量 **779/779**；浏览器逐视图实测按钮全胶囊白底浅边、终端输入框深色底恢复、地址框 34px/13px 不变，控制台仅预存在 a11y 提示与 favicon 404 |
| 83 | v0.100 | 移除 seed demo 开发占位链路（顶栏 dev 按钮 + fixture + seed 接线） | ✅ 已落地 | **背景**：「seed demo 映射」是 M-07 传输层落地前的 MVP 占位——点击把写死的产品树（MainBoard→GPIO0/UART0）+ 清零 sim 灌进视图状态；不塞假内存图（v0.29 已去掉）。内核/传输链路就绪后该 fixture 已成第二个真相源风险。**改动**：① 删 `index.html` 顶栏 `#seed-demo` 按钮与 `.seed-btn` CSS；② app.js 摘除 `wireDevSeeding()`、`requestDemoSeed()`、`riscv:seed` 监听、`exposeDevHelpers()`（`window.riscv_demo`）及对应 import；③ m02 摘除 `seedBus` 导出；④ 删除 `dev/demo-state.js`（`buildDemoStoreState/seedDemoState`）；⑤ 清掉 `memory-map.test.mjs` 死 import 与 `frontend.test.mjs` 注释里的 seed/demo 按钮旧提法；⑥ 用户手册删掉「重新 seed demo 状态」dev-only 说明。内置 demo 固件（`dev/mock-project.js` / `demo-firmware.js`）属启动回落脚手架，本批保留。**验证**：`check-front.sh` 全量 **779/779**；浏览器复核顶栏无 seed 按钮、`window.riscv_demo` 为 undefined，控制台仅预存在 favicon 404 与 46 条 a11y 提示 |
| 84 | v0.101 | 串口终端改 xterm.js 直输模式（逐键进 UART0，无独立输入框 / 发送钮） | ✅ 已落地 | **动机**：旧终端是「独立输入框 + 发送按钮」的行缓冲模式——整段草稿点发送才发出，不像真实串口终端 ⇒ ⇒ 用户拍板上 xterm.js。**改动**：① 新增 `front/lib/xterm-loader.js`（与 echarts-loader 同口径：锁版本 jsdelivr ESM、动态 `import()`、失败不抛返回 `null`、实例 + pending 去重、CSS 幂等注入、Node 门禁短路不碰网络）；② m02 终端视图改直输：持久宿主随重渲染搬运（仿波形图 `_chartHost` 裁决 L），xterm `onData` 逐键回传 ⇒ 纯函数 `terminalInputBytes(data, eol)` 映射字节（UTF-8 编码；回车按当前档 EOL 映射；Ctrl-C / 退格 / 方向键 CSI 原样透传）⇒ `dispatchTerminalBytes` 累计 RX 并发 `console-send`；③ 加载由 **app 层**发起（视图层只同步读 `getXterm()`）：终端视图露面 / 首路 console 字节触发，状态 `idle/loading/ok/degraded` 住视图状态，`degraded` 纳入防重入守卫（否则「落 degraded ⇒ 发布渲染 ⇒ 渲染监听再触发」成死循环）；④ 自研 `terminal-screen.js` 仿真器保留为离线 / CDN 不可达降级路径与 Node 测试路径；原始字节档仍走降级 `<pre>`；⑤ 旧草稿态 `term.draft`、输入框 / 发送钮链路与样式下线；RX 计数就地刷新；xterm 内容按 store 全量对齐（从原始字节档切回屏幕档时补齐期间只进降级屏的字节）。**验证**：`check-front.sh` 全量 **777/777**；归属 **485 / 292** 无 TIE；隔离浏览器复核 xterm 核心 / Fit / CSS 均 200、无旧输入框与发送钮、逐键直输 RX=1、回车补 CR RX=2、原始字节档切换宿主 / pre 互显、xterm 实例不随重渲染重建，控制台无报错 |
| 85 | v0.102 | clippy lint 收口（12 处既有告警清零）+ 唤醒一条 v0.82 休眠中断用例 | ✅ 已落地 | **背景**：main 上 `clippy -D warnings` 有 12 处既有告警（主库 5 + 测试 7）；其中 `tests_intc.rs` 的 FIFO 水位用例因上一个测试函数缺闭合 brace，被嵌套成内部项、自 v0.82 引入起从未被执行（clippy 报「never used」+「cannot test inner items」）。**改动**：① `watch_expr.rs`：`Option::map` 手写法改 `map()`、多余恒等 match 化简、`Default::default()` 后的字段赋值并入初始化；② `trace.rs` doc 续行缩进；③ `uart.rs` 两处 `[b'a', b'b', b'c']` 改 `*b"abc"`；④ `m16_interrupt/tests.rs` 单元素 for 循环展开为直写；⑤ 补回 `tests_intc.rs` 缺失的函数闭合 brace，唤醒休眠用例；⑥ 订正该用例判别式——探针首条观测总会记一条电平 0 边沿，断言从「边沿历史为空」改为「不出现值 1 的抬升沿 / 末边沿为 0」；⑦ 两个 8 参函数（`Bus::fire_hook`、`IssExecutor::new`）暂留 `#[allow(clippy::too_many_arguments)]` 并注明理由，留待装配层拆分批次收成事件 / deps 结构；⑧ 清掉 `kernel_host/mod.rs` 一段挂空的 `#[cfg(test)]` 孤儿文档块（D-177 测试已并入他处）。**验证**：`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` 20 套件 **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：既有 `cargo fmt` 格式差异（contract/platform.rs、若干 tests 文件等，非本批引入）顺延 |
| 86 | v0.103 | 装配层拆分 Layer A：纯常量 / 主题名外移子模块 | ✅ 已落地 | **背景**：`kernel_host/mod.rs` 5.5k 行，先做低风险一层——把不依赖宿主私有字段的纯常量与无状态串常量外移，`KernelHost` 结构体与主 `impl` 块本批不动。**改动**：① 新增 `kernel_host/topics.rs`：14 个事件订阅 / 回复主题名（寄存器 / 状态增量 / 波形 / 四个调试面 / 符号 / 转储 / console / 内存图 / 器件类型 / 模板 / 实例），文档注释随常量整体迁移；② 新增 `kernel_host/platform_consts.rs`：GPIO 基址 / 实例 id / 信号名 / 位宽、默认两段 RAM 基址与大小、QEMU virt 地址 / RAM 长度 / 护栏预算 / 退役区大小 / 频率、PLIC 上下文号、UART0/CLINT/PLIC 外设 id 与波特率；③ `mod.rs` 以 `pub use topics::*;` / `pub use platform_consts::*;` 再导出，对外路径 `kernel_host::CONST` 一字不变，兄弟测试模块经 `super::` 引用照旧解析。**验证**：`cargo check --workspace` 通过；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：`KernelHost` 156 个方法的按域拆分留 Layer B 独立批次；既有 `cargo fmt` 格式债顺延 |
| 87 | v0.104 | 装配层拆分 Layer B1：镜像解析产物与调试面纯函数外移 `image.rs` | ✅ 已落地 | **背景**：Layer A 只外移了常量；本批继续把与宿主私有字段解耦的类型 / 自由函数从 `mod.rs` 抽出，`KernelHost` 结构体与主 `impl` 块仍不动。**改动**：① 新增 `kernel_host/image.rs`：`DebugSections` 别名、`ParsedImage` 结构（`pub(crate)` 字段），以及 `load_segment_overlap_error`（跨镜像段重叠检查）、`fetch_bytes`（取指宽度映射 + 只读旁路）、`source_location_event`（行表两态 → 契约事件）、`illegal_export`（E_CTX-03 错误）、`waveform_export_event`（M-19 ExportHandle → TraceExport）五个纯函数；② `mod.rs` 经 `use self::image::{...}` 引入，测试模块改从 `super::image::` 取 `ParsedImage` / 重叠检查，`ElfIsaFacts` 改从 `crate::m06_elf` 直引；③ 清理随之不再使用的 `ElfIsaFacts` / `BusWidth` / `E_CTX_03` / `TRACE_FAMILY` 父级导入；`pipeline_unavailable` 因依赖留在 `mod.rs` 的 `E_OBS_02` 常量，本批未外移。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：149 个 `KernelHost` 方法的按域拆分（commands / devices / probe / session 等）耦合紧密，留作下一个专注批次；两个 8 参函数仍留 `allow(too_many_arguments)`；既有 `cargo fmt` 格式债顺延 |
| 88 | v0.105 | 装配层拆分 Layer B2：会话快照族入 `session.rs`，两个 8 参函数收口依赖束 | ✅ 已落地 | **背景**：B1 只解耦了纯函数；本批开始按域拆主 `impl` 块，同时把两个长期挂 `allow(too_many_arguments)` 的多参函数收成结构束。**改动**：① 新增 `kernel_host/session.rs`，迁入会话快照族 4 个方法：`capture_session` / `restore_session` / `intc_state_inner` / `load_intc_state`；子模块作为后代可访问宿主私有字段，跨父模块被调的 `intc_state_inner` 提为 `pub(super)`；`restore_full` 所在 `ExecutionBackend` trait 在子模块显式补导入；② M-14 `Bus::fire_hook` 七个并列访存参数收成 `BusAccessArgs`（addr / width / dir / value / ok / err_scenario / is_fetch，`ts_ps` 仍由总线时钟补），四个调用点（读数据 / 两条取指 / 写）按 `is_fetch` 与 `dir` 就位；③ M-17 `IssExecutor::new` 七个协作对象借用收成 `IssExecutorDeps<'a>`，`step_over_return` 仍单独传；构造器可见性降为 `pub(crate)`，两个内部调用点（`step_n` / `continue_until_event`）先组依赖束；④ 移除两处 `#[allow(clippy::too_many_arguments)]`，清掉 `mod.rs` 随之闲置的 `CheckpointMetaDto` / `M13StateDto` / `SessionSnapshotDto` 导入。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` 复跑 **777/777** rc=0（首跑单红为已知偶发 worker 订阅噪声，复跑归零）。**未动**：其余 145 个 `KernelHost` 方法按域分组（commands / devices / probe-waveform-trace / admin / queries）尚未盘点；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 89 | v0.106 | 装配层拆分 Layer B3：console / UART 族入 `console.rs` | ✅ 已落地 | **背景**：B2 迁出会话快照族后，本批按域拆第二族——console / UART 双向收发，内聚度高、私有辅助只在族内互调。**改动**：① 新增 `kernel_host/console.rs`，迁入 8 个方法：TX 侧 `drain_console` / `console_uart` / `drain_uart` / `console_pending` / `collect_console` / `console_event`，RX 侧 `uart_receive` / `uart_receive_on`；子模块作为后代访问 `console_backlog` / `roles` / `devices` / `topics` 私有字段；② 私有辅助 `console_uart` / `drain_uart` 仅族内调用，保持私有；公开方法可见性不变，父模块 `on_run_inner` / `tick` / `on_write_console` 等跨模块调用照常解析；③ 子模块显式导入 `Rc` / `RefCell` / `Uart16550` / `EventPayload` 与 `super` 的 `CONSOLE_BACKLOG_MAX` / `TOPIC_CONSOLE`；④ 清掉 `mod.rs` 随之闲置的 `Uart16550` 导入。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 137 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 90 | v0.107 | 装配层拆分 Layer B4：中断控制器（M-16）族入 `intc.rs` | ✅ 已落地 | **背景**：B3 迁出 console 族后，本批拆第三族——中断控制器通路，方法薄、内聚于 `intc` / `sinks` / `devices` / `roles` 字段。**改动**：① 新增 `kernel_host/intc.rs`，迁入 11 个方法：端口装配 `install_interrupt_port`、外设源同步 `sync_interrupts`、只读 `intc_timer_compare_ps` / `intc_pending_lines` / `sync_interrupts_now` / `intc_state`，IRQ 包装器 `raise_irq` / `lower_irq` / `clear_irq` / `set_timer_compare` / `intc_delegation_failures`；② 被父模块构造器与命令路径调用的 `install_interrupt_port` / `sync_interrupts` 提为 `pub(super)`；`session.rs` 的 `intc_state_inner` 由 `pub(super)` 提为 `pub(crate)` 以放行兄弟模块；③ `raise` / `lower` 是 `InterruptSink` trait 方法，子模块显式补 trait 导入；④ 子模块导入 `SharedInterruptPort` / `M16IntcDto` / `Rc` / `PLIC_CONTEXT_MACHINE` / 中断线号常量与 `super::platform`；清掉 `mod.rs` 随之闲置的 `SharedInterruptPort` / `M16IntcDto` 导入；探针侧采样 `sample_irq_source` 耦合 M-18 通道，留父模块由子模块后代访问。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 126 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 91 | v0.108 | 装配层拆分 Layer B5：引擎/总线状态只读簇入 `state.rs` | ✅ 已落地 | **背景**：B4 迁出中断族后，本批拆依赖最窄的只读簇——只挂 `backend` 字段的引擎/总线状态读面。**改动**：① 新增 `kernel_host/state.rs`，迁入 9 个方法：`total_executed` / `bus` / `bus_mut` / `now_ps` / `pending_event_count` / `pc` / `read_memory` / `registers` / `engine_snapshot`；② 子模块显式导入 `Bus` / `WriteSize` / `PlatformError` / `EngineSnapshotDto`；③ 清掉 `mod.rs` 随之闲置的 `Bus` / `EngineSnapshotDto` 导入（`DEFAULT_INSTRUCTION_BUDGET` 仍被分块步进路径使用，保留）；④ HMS 健康 / 降级 / 日志环那半只读面（`health` / `watchdog_fired` / `degrade_*` / `log_entries`）耦合 `hms` / `degrade_dispatch` / `log_buffer` 字段，留父模块下一批再切。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 117 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 92 | v0.109 | 装配层拆分 Layer B6：HMS/日志只读簇入 `diagnostics.rs` | ✅ 已落地 | **背景**：B5 迁出引擎/总线只读簇后，本批拆另一半可观测只读面——M-22 健康 / 降级计数与动作、M-21 日志环快照，耦合 `hms` / `degrade_dispatch` / `log_buffer` 三个字段。**改动**：① 新增 `kernel_host/diagnostics.rs`，迁入 6 个方法：`health` / `watchdog_fired` / `degrade_dispatch_total` / `degrade_dispatch_last` / `degrade_failures` / `log_entries`；② 子模块显式导入 `LogEntry` / `LogQuery` / `DegradeAction` / `Health` / `PlatformError`；③ 清掉 `mod.rs` 随之闲置的 `Health` 类型导入（`DegradeAction` 仍被构造器 `DegradeAction::Terminate` 使用、`LogEntry` / `LogQuery` 仍被管理员通道 `drain_log_entries` 使用，均保留）；子模块命名 `diagnostics.rs` 以避免与 `crate::m22_hms` 歧义。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 111 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 93 | v0.110 | 装配层拆分 Layer B7：符号/探针/波形/轨迹计数只读簇入 `inspect.rs` | ✅ 已落地 | **背景**：B6 迁出 HMS/日志只读簇后，本批拆调试面可观测计数族——M-19 波形管道、M-18 探针边沿 / trace 环、M-11 符号表 / DWARF / 反汇编缓存、M-11 执行轨迹环的只读读面，内聚于 `waveform` / `probes` / `backend` / `disassembler` / `index` / `trace` 字段。**改动**：① 新增 `kernel_host/inspect.rs`，迁入 19 个方法：`waveform_pipeline` / `probe_bridge_failures` / `waveform_edges_truncated` / `probe_edges_dropped` / `bus_hook_count` / `disasm_cache_len` / `disasm_cache_hits` / `symbol_count` / `lookup_symbol` / `symbol_at` / `dwarf_status` / `lookup_symbol_prefix` / `trace_len` / `trace_dropped` / `trace_enabled` / `probe_trace_len` / `probe_trace_dropped` / `probe_edge_count` / `trace_events_discarded`；② 子模块显式导入 `SymbolService`（trait 方法 `lookup_symbol` / `symbol_at` / `lookup_prefix` 解析所需）与 `SharedPipeline`；符号 / DWARF 类型签名沿用全路径；③ `mod.rs` 的 `SymbolService` 导入保留（`resolve_line` trait 方法仍在调试面路径使用）。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` 首跑 2 条已知偶发 worker/订阅噪声，复跑 **777/777** rc=0。**未动**：其余约 92 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债（m18_probe 导入块等）顺延 |
| 94 | v0.111 | 装配层拆分 Layer B8：调试面事件 / trace 环搬运族入 `debug.rs` | ✅ 已落地 | **背景**：B7 迁出计数只读簇后，本批拆调试面事件族——反汇编 / 行表 / 符号解析 / 内存转储 / 轨迹与波形导出，以及 M-18 访存 trace 环向 M-11 轨迹环的转移 / 丢弃 / drain。**改动**：① 新增 `kernel_host/debug.rs`，迁入 11 个方法：`on_disasm` / `on_resolve_line` / `on_resolve_symbol` / `on_dump_memory` / `on_export_trace` / `on_set_trace_enabled` / `export_waveform` / `export_execution` / `transfer_trace` / `discard_m18_trace` / `drain_m18_trace`；② 被父模块调度与运行路径直接调用的 8 个方法（6 个 `on_*` + `transfer_trace` + `discard_m18_trace`）提为 `pub(super)`，族内辅助 `export_waveform` / `export_execution` / `drain_m18_trace` 保持私有；③ 子模块显式导入 `SymbolService` trait（`resolve_line` 解析所需）、`LineLookup` / `ExportFilter` / `TraceEvent` / `TraceFilter` / `TraceFormat` / `TraceSource` / `EventPayload` 及 `image` 自由函数 `fetch_bytes` / `illegal_export` / `source_location_event` / `waveform_export_event` 与父模块私有自由函数 `pipeline_unavailable`；④ 清掉 `mod.rs` 随之闲置的 `E_DBG_03_DUMP_TOO_LARGE` / `E_DBG_04_SYMBOL_NAME_EMPTY` / `TraceEvent` / `TraceFilter` / `TraceFormat` / `TraceSource` / `DUMP_MAX_BYTES` / `SYMBOL_CANDIDATE_LIMIT` / `LineLookup` / `SymbolService` / `ExportFilter` 与 `image` 四个自由函数导入；⑤ `tests_m11.rs` 改经 `super::image::` 显式导入 `source_location_event` / `waveform_export_event`，并补 `LineLookup` 显式导入（`use super::*` 不再透出父模块已移除的重导出）。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 81 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债顺延 |
| 95 | v0.112 | 装配层拆分 Layer B9：工程/内存图族入 `project.rs` | ✅ 已落地 | **背景**：B8 迁出调试面事件族后，本批拆工程 / 板级内存图族——工程装载与卸载、整份 / 差量换图、修订号检查、内存图 / 模板清单 / 逐实例状态事件，以及差量合成纯函数。**改动**：① 新增 `kernel_host/project.rs`，迁入 10 个方法：`on_load_project` / `memory_map_pending` / `on_set_memory_map` / `on_patch_memory_map` / `on_unload_project` / `check_revision` / `memory_map_event` / `platform_templates_event` / `device_instances_event` / `reported_device_instances`，外加自由函数 `compose_memory_map`（提 `pub(crate)` 供等价性测试直调）；② 被父模块调度与订阅推送路径直接调用的 7 个方法（4 个 `on_*` + `memory_map_event` / `device_instances_event` / `platform_templates_event`）提为 `pub(super)`，族内辅助 `memory_map_pending` / `check_revision` / `reported_device_instances` 保持私有；③ 子模块显式导入 `ExecutionBackend` trait（`backend.reset` 解析所需）、`HostWatchdog`、`ParsedImage` / `load_segment_overlap_error`、`Project` / `PlatformTemplateInfo` / `PlatformDescription` / `RegionSpec` / `EventPayload` / `MemoryMapOp` 与本族错误码；④ 清掉 `mod.rs` 随之闲置的 `E_PLT_13_REVISION_CONFLICT` / `E_PLT_14_REGION_NOT_FOUND` / `E_PLT_17_BOARD_MISMATCH` / `PlatformTemplateInfo` / `RegionSpec` / `MemoryMapOp` / `load_segment_overlap_error` 导入；⑤ `tests_board_delta_equivalence.rs` 改经 `super::project::compose_memory_map` 显式导入。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：其余约 71 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债顺延 |
| 96 | v0.113 | 装配层拆分 Layer B10：管理员通道族并入既有 `diagnostics.rs` | ✅ 已落地 | **背景**：B9 迁出工程 / 内存图族后，本批拆管理员通道族——角色准入与 op 分发、日志截断导出、禁用错误构造，以及固件 ISA 位宽 / 压缩扩展板级校验关联函数；按既定蓝图并入既有 `diagnostics.rs`（可观测 / 诊断面同族，不单开模块）。**改动**：① `kernel_host/diagnostics.rs` 追加 4 个方法：`on_admin`（提 `pub(super)`，父模块命令路由直调）、`admin_channel_disabled_error`（提 `pub(crate)`，族内调用 + `tests_admin_channel.rs` 兄弟模块直调）、`drain_log_entries`（保持私有，仅族内 `on_admin` 调用）、`isa_gate_error`（提 `pub(crate)` 关联函数，`project.rs` 跨兄弟调用）；② 子模块显式导入 `ParsedImage`、父模块常量 `ADMIN_CHANNEL_DISABLED_MESSAGE` / `ADMIN_DIAG_OP` / `ADMIN_DRAIN_LOG_CTX` / `ADMIN_DRAIN_MAX_ENTRIES`、`Role` / `LogLevel` / `LogEntryDto` / `E_PLT_20_FW_ISA_MISMATCH` / `PLATFORM_FAMILY` / `mcu_isa_info` / `PlatformDescription` / `AdminOp` / `EventPayload`；③ 清掉 `mod.rs` 随之闲置的 `LogEntryDto` / `E_PLT_20_FW_ISA_MISMATCH` / `mcu_isa_info` / `AdminOp` / `LogEntry` / `LogQuery` 导入；④ `tests_admin_channel.rs` 补显式导入 `AdminOp` / `LogEntryDto` / `LogEntry` / `LogQuery`（`use super::*` 不再透出父模块已移除的重导出）。**验证**：`cargo check --workspace` 零警告；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` 首跑 1 条已知偶发 worker/订阅噪声，复跑 **777/777** rc=0。**未动**：其余约 67 个 `KernelHost` 方法按域分组尚未迁移；既有 `cargo fmt` 格式债顺延 |
| 97 | v0.114 | 纯 rustfmt 排版归一收口既有格式债 | ✅ 已落地 | **背景**：B10 收口后，工作区残留 12 个既有文件的 rustfmt 排版差异（import 块换行、数组 / 调用断行、尾随逗号、多余空行），本批单独收口，零行为变化。**改动**：按 rustfmt 规则归一 `contract/platform.rs`、`kernel_host/platform.rs`、4 个 `tests_board*` / `tests_platform`、`m14_bus/mod.rs` + `tests.rs`、`m16_interrupt/tests.rs`、`kernel/tests` 下 3 个文件；未改任何标识符、字面量、控制流或断言。**验证**：`cargo fmt --check` rc=0；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：功能逻辑与接口一字未改 |
| 98 | v0.115 | 装配层拆分 Layer B11：残留只读访问器族分归三模块 | ✅ 已落地 | **背景**：B10 收口后，本批拆剩余只读访问器与诊断包出口，按内聚域分归既有子模块。**改动**：① 入 `diagnostics.rs` 6 个：角色族 `role` / `set_role` / `admin_enabled` / `admin_drain_truncated_entries`，诊断包出口 `export_diagnostic_kit` / `archive_diagnostic_kit`；② 入 `project.rs` 6 个工程 / 平台只读：`memory_profile` / `platform_id` / `peripheral_irq_table` / `map_revision` / `last_project` / `device_table`；③ 入 `session.rs` 2 个会话快照只读：`peripheral_states` / `memory_map`。方法仍挂在 `impl KernelHost` 上；对外公开 API 保持 `pub`（被 bin `osz-boot` 与集成测试 `kernel/tests` 直调，提 `pub(crate)` 会触发 E0624 / dead_code），族内后代访问不受影响。子模块显式导入 `DiagnosticKit` / `MemoryProfile` / `M14MapDto` / `M15PeriphDto`；清掉 `mod.rs` 随之闲置的 `M14MapDto` / `M15PeriphDto` / `DiagnosticKit` 导入。**验证**：`cargo check --workspace` 零警告；`cargo fmt --check` rc=0；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：基础辅助 `terminated_error` / `host_now_ms` / `error_from` 留父模块；其余协议桥 / 装配构造 / 命令执行调度族尚未迁移 |
| 99 | v0.116 | 装配层拆分 Layer B12：协议桥外壳入新 `bridge.rs` | ✅ 已落地 | **背景**：B11 收口后，本批拆协议桥最外层的信封 / 序列化入口；命令派发枢纽 `dispatch` 与命令执行族互调最密，留待后续整族搬迁。**改动**：① 新增 `kernel_host/bridge.rs`，迁入 4 个方法：`handle_json`（WASM 桥 JSON 外壳，保持 `pub`）、`handle_message`（内核侧核心入口，保持 `pub`）、`handle_envelope`（版本 / 方向校验后派发，私有）、`make_event`（事件序号包装，私有）；② 子模块显式导入 `PlatformError` / `E_PROTO_01_VERSION_MISMATCH` / `PROTO_FAMILY` / `Direction` / `EventPayload` / `Message` / `Payload` / `SCHEMA_VERSION`；③ 清掉 `mod.rs` 随之闲置的 `Direction` / `Message` / `Payload` 导入（`CommandPayload` / `EventPayload` / `HaltReason` 仍被派发与执行族使用，保留）；④ 9 个 lib 内测试文件补显式 `Message`（部分合并 `AdminOp` / `CommandPayload` / `MemoryMapOp`）导入——`use super::*` 不再透出父模块已移除的重导出。**验证**：`cargo check --workspace` 零警告；`cargo fmt --check` rc=0；`cargo clippy --workspace --all-targets -- -D warnings` rc=0；`cargo test --workspace --lib --tests` **1898 passed / 0 failed**；前端 `check-front.sh` **777/777** rc=0。**未动**：`dispatch` 命令派发枢纽、`on_subscribe` / `tick` / `register_waveform` / `state_delta_event` / `waveform_event` 订阅事件构造族，以及装配构造 / 命令执行调度族尚未迁移 |
| 75 | v0.91 | `B-94` 收口：trace 卡片**只留一种导出入口**（用户拍板「保留导出执行轨迹」） | ✅ 已落地 | **缺陷**：trace 卡片里**新旧两套按钮并存** —— 老的「导出执行轨迹 (CSV)/(JSON)」（只出**第一块**到 `<pre>`）＋ v0.63 加的「下载轨迹到本地」（整份翻页落盘）⇒ 同一个动作两颗按钮、语义还不一样，用户不知道该点哪颗。**拍板**：保留「导出执行轨迹」⇒ **一个入口、一份结果**。**改动**：① 视图删掉「下载轨迹到本地」那颗（连同 `exportDownloadLabel`）；② 「导出执行轨迹 (CSV)/(JSON)」与「导出波形 (VCD)」都带 `data-whole="1"`；③ `app.js` 的 `export-trace` 遇 `whole` 直接走 `downloadWholeTrace`（整份翻完 + 落盘，不再重复发第一块），`case "download-trace-local"` 删除 ⇒ 代码里**只剩一条落盘路径**；④ 前端用例与门禁探针同步换到新布局（`B-94` 摘 `known` 变硬判据、`B-97` 点「导出执行轨迹 (CSV)」、`B-103` 改成「开关 + 整份导出 ×2、平行下载 0」）。⑤ **顺带根治一段段间竞态**：`unknownSystem` 段把「内核收了新板图」当成配板完成 ⇒ 但配板是**两步**（`SetMemoryMap` + 一次 `images: []` 的 `LoadProject`）⇒ 第二步没落地就取基准 ⇒ 全量顺序下读到**上一段的旧图 / 旧修订号** ⇒ 「拒之后图变了」误红两条。修法：加一段「**等配板落定**」（新板图 + `loaded=false` 的空态同时成立，上限 6 s）再取基准。**验证**：`check-front.sh` **758/758**；`--only traceExport` B-94~B-99 全 ✓（B-97 进度 `90000 → 100000 / 100000`）；`--only debugDedup` B-100~B-104 全 ✓；`OSZ_ELF` 全量 ⇒ **31/31 段、0 跳过、0 ✗、rc=0**（266 ✓），已知缺陷 **0** 条 |
| 74 | v0.90 | 差量两条收口（`PatchMemoryMap` 判据的**基线取错数组** · 门禁自身 bug） | ✅ 已落地 | **根因（实测）**：门禁量基线时读的是 `window.__bvIn`（**入向**：内核 → 前端，每一条事件都长一格），却拿这个数字去 `slice` `window.__bvWire`（**出向**：前端 → 内核，只有命令才长）⇒ 切片几乎总是**空** ⇒ 「线上帧里没有 `PatchMemoryMap`」**恒成立** ⇒ v0.86 据此把两条判据判成「产品缺口 · load-archive 入口下差量不可达」并挂 `known`——**结论是错的**。**反证**：在 `applyProductBoard` 里插一处临时探针，现场读到 `{hasBase:true, pending:false, mode:"patch", ops:1, baseId:"browser-verify-board", draftId:"browser-verify-board", expectedRevision:3}`；同时 `__bvWire` 全文统计 `nPatch=1, nSet=0` ⇒ **差量一直在走**，产品侧没有缺口。**改动**：① `wireBaseline` 改为取 `__bvWire` 自身长度（同一个数组同一把尺）；② 两条判据摘 `known` 变硬判据、去掉「已知产品缺口」字样；③ 旧结论留注释供追溯；④ 临时探针已全部删除。**验证**：`--only boardDelta` ⇒ 差量两条由 ⚠ 转 ✓（帧长 311 字节、载荷含 `UpsertRegion` + 被改段基址）；`OSZ_ELF=… 全量` ⇒ **31/31 段、0 跳过、0 ✗、rc=0**，已知缺陷 **1** 项（只剩 `B-94`） |
| 73 | v0.89 | ISA 门禁三条收口（`B-162/162b/164`）+「重新装载工程」入口补齐（**方案 B**） | ✅ 已落地 | **根因（实测）**：`submitBoard` 走 `load-archive`＝`applyProductBoard` ⇒ 配板那次 `images: []` 的内部装载把 **`loadedProject` 覆盖成空工程**（`front/app.js:2293` 无条件赋值）⇒ 之后点「重新装载工程」重发的是**空工程** ⇒ 内核这一步**没有固件可判** ⇒ `E_PLT-20` 永不出现（三条恒红）；`B-162b` 的期望（「拒后还是上一段那块 rv32ima 板」）也落空——板在配板那一刻就已**合法**换掉。**更靠前的一层（真产品缺陷）**：`reload-project` 在 `app.js` 里**有分支**，但视图层**从来没发过它**（产品视图只有「重新载入」`reload-product`）⇒ 「重新装载工程」这个能力在界面上是**空的**；配合上一条，配完板再想装固件**没有任何入口**。**改动**：① `loadProjectIntoKernel` 增 `options.remember`，`applyProductBoard` 的内部装载传 `remember: false` ⇒ 配板不再抹掉「用户当前工程（含固件）」；② 产品视图补齐「重新装载工程」按钮（`data-action="reload-project"`）+ `m02` 的 `reload-project` 分支（`state.loaded !== true` 不发）⇒ 用户能把原固件重装进新板，板/固件不兼容时由**内核**在装载那一刻拒；③ 门禁 `B-162`/`B-164` 摘 `known` 变硬判据，`B-162b` 换口径为「拒后图 id/修订号逐字相同 + `loaded` 仍假」（同 ⑩）；④ 新增前端用例「配板的内部空装载不覆盖当前工程」。**验证**：`--only mcuConcept` **7/7 全绿**（`B-162` 通知=`E_PLT-20: 固件是 RV32…rv64ima 是 RV64`；`B-164` 通知=`E_PLT-20: …EF_RISCV_RVC…rv32i 不含 c`）；`check-front.sh` **759/759**；`OSZ_ELF=… 全量` ⇒ **31/31 段、0 跳过、0 ✗、rc=0**，已知缺陷 **3** 项（仅剩差量 2 + `B-94` 1） |
| 72 | v0.88 | 门禁第二项收口：`B-142` / ⑩ **判据窗口订正**（+ 内核 `loaded` 语义订正） | ✅ 已落地 | **根因（实测）**：产品视图「配板」不是「只记一张图」——`applyProductBoard`（`front/app.js`，创建/从模版加载/重新载入三条通路共用）干的是 `SetMemoryMap` + **一次 `images: []` 的 `LoadProject`**，后者是「让待生效的板生效」的那次装载、**本质是新会话**（`backend.reset()` ⇒ 总线换新、镜像清空、PC 归 0）⇒ 机器在**配板那一刻**就已经空了，`0x10 → 0x0` 是**配板的合法后果**，不是「拒」造成的 ⇒ 旧判据拿错窗口去指控它（且「正在跑的访客 + 板待生效 + 装载被拒」这条状态在产品 UI 里**不可达**）。⑩ 更糟：数据源读的是 v0.68 就删掉的产品视图折叠表（`[data-region-*]`）⇒ **恒 0 行**、期望值又默认「图还是 demo 板」⇒ 双重恒假。**改动**：① `B-142` 基准挪到**配板之后**，断言「PC 不被写 + 无半装载 + 机器里没有可执行内容（拒后再单步不前进）」；② ⑩ 数据源换内核 `memory.map` 事件、期望改成「与配板那张图 `id`/`revision` 逐字相同」，并**去掉 `known`** 变硬判据；③ 两条前置（「配板＝换图 + 开新会话」）明写出来，防「0 == 0」恒真；④ **新判据当场抓出一条真缺陷**：空工程装载后内核仍报 `loaded=true` ⇒ `kernel_host` 改成 `image_loaded = 镜像里有没有字节`（`has_image_bytes`），否则界面在 PC=0 的空机器上挂「已装载」，且后续 `SetMemoryMap` 永远进不了「立即生效」分支；新增内核用例 `b5b_an_empty_project_does_not_mark_the_session_as_loaded`。**验证**：`--only unknownSystem` 12/12 全绿（0 ✗ 0 ⚠）；内核 `cargo test -p riscv_sim_kernel` 全通过；`OSZ_ELF=… 全量` ⇒ **31/31 段、0 跳过、0 ✗、rc=0**，已知缺陷 **6** 项（B-142/⑩ 销账） |
| 70 | v0.86 | 门禁**收口**（33 → 31 段 · 全量 **0 ✗**） | ✅ 已落地 | 五类剩余红逐条核实：①**判据过期**（`mcu`/`uiLayout` 的 `code.textContent` 改读 `input.value`、`uiLayout` 的等待条件写成 `0x00000000`）；②**段间隐式耦合**（`unknownSystem` / `uiLayout` / `boardDelta` / `boardOrdering` 各自 `seedBoard` 自备前态，`T-6` 判据从「恰好 +1」放宽为「严格前进」）；③**读已删 UI**（乱序段 `[data-action="unload-project"]` ⇒ 判产品视图渲染体；乱序段改一段 size ⇒ 改走 `load-archive`，删死函数 `bvEditFirstRamSizeExpression`）；④**无实体语汇**（`templateLibrary`〔`data-tpl-library` 前端 0 命中〕与 `pinmuxPower`〔`pins`/`pinmux` 契约 0 实体〕两段**退役**，33 → 31，连带删 `waitForTemplateLibrary`）；⑤**差量帧误读旧帧**（`__bvIn` 全段共用 ⇒ 加 `wireBaseline` 只扫新增；基底改读回内核描述）；新增 per-check `known` 机制（报告打 ⚠、`failed` 过滤、末尾单列「已知产品缺陷」），跳过段不再参与判定 ⇒ 全量 `rc=0`（889 s），8 条已知缺陷单列（B-94 新旧按钮并存 / B-142+⑩ 拒后机器与图被清空 / B-162+162b+164 ISA 门禁未生效 / 差量两条：`load-archive` 入口下不可达） |
| 61-前置 | — | 门禁工具改造（开工前置） | ⏳ 仍欠 | ⚠️ **R-1~R-4**（分段重跑 / 失败即 dump 证据 / 采集通道 health check / wasm mtime 前置），见 [`../plans/GATE_TOOLING_PREREQ.md`](../plans/GATE_TOOLING_PREREQ.md)；**R-5~R-7** 随批顺手整改 |

**顺手批**（可插任意两批之间，**只列未闭环的** ⇒ ⇒ 闭环的从这里划掉、销账的处置进 `REVIEWS-archive.md`）：**D-159**（真机门禁在有负载时假红 · **根因已定位**：判据等的是**时长**而不是**状态** ⇒ ⇒ 三处改成「等状态」⇒ ⇒ 剩**做法 B**（不节流环境跑门禁：`Emulation.setCPUThrottlingRate` 那一路未做）⇒ ⇒ **未销账**）。

---

## §2 近期批次（v0.66 ~ v0.72）

| 版本 | 批主题 | 关键落点 |
| --- | --- | --- |
| v0.66 | 器件类型注册表 / 逐实例状态（STAGE-43） | `device.types` / `device.instances` 两条只读通道；M-15 类型注册表；宿主逐实例状态 ⇒ **L1 通** |
| v0.67 | 「读回」批 | 变化高亮基准改为「本次运行/单步开始前」；停下自动读回；读过的内存单元按同宽度重读 |
| v0.68 | 产品树编辑面（STAGE-44） | 树 = 草稿派生视图；加/删/改器件与编辑器共用同一份草稿；类型下拉只列内核已实现 ⇒ **L2 通** |
| v0.69 | 陈旧面收尾（STAGE-45） | 反汇编不再默默过期（跟随 PC 开关）；落地视图删重复虚拟时间；归属工具响亮失败 |
| v0.70 | 基址规划面板（STAGE-46） | 板级内存图编辑器内置规划面板：列出空隙 → 挑一条填进目标行；冲突仍由内核判（`E_PLT-06`） |
| v0.71 | 「未知系统」门禁（STAGE-47） | 内核拿镜像地址区间跟当前板分区比（中档严格度，`fully_covered`/`missing`/`partial` 分开算）；判不过拒且不建图；`E_PLT-17`；门禁自查产物新鲜度 |
| v0.72 | 宿主不再塌缩器件实例（STAGE-48） | 宿主改**按 id 索引的实例表**；连线编译「查得到就留、查不到就记一笔」；`sync_interrupts` **逐线查两端** ⇒ ⇒ **第二个 UART 的连线真的通电**；**丢线过 wire**（`memory.map` 多一个 `bridge_report`）⇒ ⇒ 契约**升 v11** |
| v0.73 | 描述格式一次升版（STAGE-49） | `base` **可缺省**（⇒ ⇒ 内核在装配期分配）；`timer_riscv` **能写进描述**；`pins` 段（**只定形状**）；`roles` 段（**显式声明角色**）⇒ ⇒ ⚠️ **描述格式升版**（`PLATFORM_SCHEMA_VERSION` 1 → 2 · `PLATFORM_RULES_VERSION` 3 → 4 ⇒ ⇒ 契约**信封** `SCHEMA_VERSION` **不动** ⇒ ⇒ 11） |
| v0.74 | 引脚级连线 + 通用连接路由（STAGE-50） | `bridge.from` 加**可选**的 `pin`（⇒ ⇒ 连到**具体引脚**）；连线事件**词表化**（`rx_ready` = **电平** / `pin_irq` = **一次性** ⇒ ⇒ 「事件名是否合法」**一个落点**）；**引脚数由内核报**（前端**查**不抄）；前端**连线编辑面**（`front/views/components/bridge-editor.js`）⇒ ⇒ **界面上画得出线** ⇒ ⇒ **L3 通了** |

---

## §3 版本里程碑一览

> 一行一版。逐版落点 / 门禁 / 台账口径详见 [`PROGRESS-history.md`](./PROGRESS-history.md)。

### 起步与地基（v0.3 ~ v0.12）

| 版本 | 里程碑 |
| --- | --- |
| v0.3 | MVP 范围决策 |
| v0.5 | M1 数据底座第二步完成 |
| v0.6.1 | 修复（review 编号） |
| v0.7 | M-15 UART/GPIO/Timer |
| v0.7.1 | 评审闭环（阻断/高/中/低） |
| v0.7.2 | 跨模块挂账 2 项不修 |
| v0.7.3 | N1 DRY 回归 / N2 IN 回读输出脚 / N3 去重位图放宽 |
| v0.8 | M-18 探针落地 |
| v0.8.1 | M-18 review 闭环 + `docs/reviews/` 文档体系建立 |
| v0.9 | M-19 波形管道落地 |
| v0.9.1 | M-19 review 闭环 |
| v0.10 | 前端 M-01/02/03 首次模块级 review 闭环 |
| v0.11 | 首次「多路并行开发」 |
| v0.11.1 | 先检视、后合 main |
| v0.11.2 | 契约新增可序列化 DTO |
| v0.12 | D-38 闭环 + RV32I golden 对拍打通（Spike 作 oracle） |

### 内核核心成形（v0.13 ~ v0.23）

| 版本 | 里程碑 |
| --- | --- |
| v0.13.0 | 第 1 阶段：WASM 导出层 + 前端接真实内核 + 真机复验 |
| v0.13.1 | 第 1 阶段三路独立检视 + 闭环 |
| v0.14.0 | D-46 闭环：探针管道接上 |
| v0.14.1 | D-46 检视轮闭环 |
| v0.15.0 | D-07 闭环：唤醒时间驱动链路 |
| v0.15.1 | D-07 检视轮闭环 |
| v0.16.0 | RV32M（8 条）+ RV32C（26 条）扩展落地 |
| v0.17.0 | 契约层 v2 升版 |
| v0.18.0 | 会话级快照载荷 |
| v0.19.0 | M-16 中断控制器落地 |
| v0.20.0 | M-11 符号表/DWARF/反汇编/trace 导出落地 |
| v0.21.0 | CSR/陷阱黄金覆盖（对拍首次照出 CSR 语义缺口） |
| v0.22.0 | Wave 2a：DWARF 行表真解析 + 访存异常陷阱化 |
| v0.23.0 | 让真实 M 模式 RTOS（osz）跑起来 |

### 正确性深化（v0.24 ~ v0.40）

| 版本 | 里程碑 |
| --- | --- |
| v0.24.0 | 把 v0.23 遗留一次做掉 |
| v0.24.1 | 纯文档批 |
| v0.25.0 | 取指路径对齐 + 前端补洞 |
| v0.26.0 | 内存图通用化 A 批 |
| v0.27.0 | 内存图通用化 B 批 + D-82 闭环 |
| v0.28.0 | 活锁处置链路批 |
| v0.29.0 | 前端可配置的板级内存图批 |
| v0.30.0 | 板级能力闭环批 |
| v0.31.0 | 板级差量 · 卸载 · 预算拆分批 |
| v0.32.0 | 差量等价性 · 乱序到达 · 门禁注册表批 |
| v0.33.0 | 器件身份 · 板档案规则版本 · 台账口径 · 默认门禁等价性批 |
| v0.34.0 | 口径收敛批 |
| v0.35.0 | D-94 勘察批 |
| v0.36.0 | Zicntr 计数器批 |
| v0.37.0 | PMP 批 |
| v0.38.0 | S / U 模式批 |
| v0.39.0 | PMP 逐片判定 + D-152 闭环批 |
| v0.40.0 | PMP 特权口径 + D-153 闭环批 |

### 契约与工程纪律（v0.41 ~ v0.50）

| 版本 | 里程碑 |
| --- | --- |
| v0.41.0 | `mtval` · `time` · 空洞一致探测器批 |
| v0.42.0 | 日志 · 健康 · 契约口径批 |
| v0.43.0 | 场景码撞号 · 诊断包可见性批 |
| v0.44.0 | 契约层 v8 · 管理员跨进程载体 · 场景码落位收口批 |
| v0.45.0 | 角色准入收口批 |
| v0.46.0 | 降级机制正确化批 |
| v0.47.0 | 固件加载 UI 批 |
| v0.48.0 | 加载固件入口可见性批 |
| v0.49.0 | UI 视觉与布局批 |
| v0.50.0 | 视图内容形态 + 断点 UI 批 |

### 前端与调试体验（v0.51 ~ v0.62）

| 版本 | 里程碑 |
| --- | --- |
| v0.51.0 | 波形真图批 |
| v0.52.0 | 连续运行批 |
| v0.53.0 | 运行中隐藏活值 + 读内存告警批 |
| v0.54.0 | 调试器基础能力批 |
| v0.55.0 | 串口 RX 中断堵死修复批 |
| v0.56.0 | 终端屏幕仿真批 |
| v0.57.0 | 调试器增强批（按符号断点 + 条件断点表达式 + 整段转储） |
| v0.58.0 | 加载固件入口收敛批 |
| v0.59.0 | 复位语义回归批 |
| v0.60.0 | trace 记录开关可观测性批 |
| v0.61.0 | 开发环境缓存批 |
| v0.62.0 | trace 导出可用性批 |

### 产品链路推进（v0.63 ~ v0.71）

| 版本 | 里程碑 |
| --- | --- |
| v0.63.0 | 调试视图去重批 |
| v0.64.0 | 读内存可用性批 |
| v0.65.0 | 寄存器 / 内存窗口批 |
| v0.66.0 | 器件类型注册表 / 逐实例状态批（L1 通） |
| v0.67.0 | 「读回」批 |
| v0.68.0 | 产品树编辑面批（L2 通） |
| v0.69.0 | 陈旧面收尾批 |
| v0.70.0 | 基址规划面板批（B1） |
| v0.71.0 | 「未知系统」门禁批（L4 通） |
| v0.72.0 | 宿主不再塌缩器件实例批（L3 地基补上） |
| v0.73.0 | 描述格式一次升版批（形状定版） |
| v0.74.0 | 引脚级连线 + 通用连接路由 + 前端连线编辑面批（**L3 通了**） |
