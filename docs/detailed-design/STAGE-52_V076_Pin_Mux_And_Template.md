# STAGE-52 · v0.76 · 模板导入（不兜底生效板）＋ 引脚复用（固件动态切换）

> 第 60 批 / v0.76（2026-11-04 用户拍板 · 设计定稿 · 开发随本规格启动）。
> D-175 的 D 的剩余部分（引脚的复用功能 / 方向 + 固件动态切换）＋ 平台不再自动选档兜底生效板（模板导入）。

## §1 用户拍板口径（原文要点）

1. **平台不应该默认给用户 qemu-virt 配置**，应提供类似「从模板导入」的功能导入预设 —— **默认平台不兜底生效板**。
2. **引脚复用是动态的**：一个 pin 现在是 UART TX、过会儿是 I2C SDA —— **用户定义**（板描述），**固件代码也能修改**（运行时寄存器切换）。
3. **平台以引脚为原子感知活动**：pin 上有数据过来时，平台要能感知到「是哪个引脚」→ 拉「当前功能对应的那根中断」。
4. **B3 决议**：`auto_selectable` 字段**废弃**（不要了）。
5. **B1 决议**：无生效板时装载 / 运行 ⇒ **拒 + 引导**（先导入模板或配置板 · 不静默兜底）。
6. **B2 决议**：「未知系统」门禁判据从「自动选档候选」改为「**当前生效板**的 RAM 范围」。
7. **信号名开放**：不做封闭词表 —— 引脚配置成什么完全由用户定义，后续还可能有其他外设接入（平台只提供**底层基座**机制）。

## §2 现状勘察（代码事实 · 2026-11-04）

### 2.1 自动选档兜底（本批第一件要废除的）

| 事实 | 位置 |
| --- | --- |
| 内核 `choose_platform` / `choose_description(ranges)`：镜像地址落在 `auto_selectable == true` 的内置板 RAM 范围 → 自动挑（priority 小者优先）→ 装不下回落 `default` | `crates/kernel/src/kernel_host/mod.rs:1367` · `platform.rs:1257` |
| 内置板 3 块：`default`（auto=true）/ `minimal`（auto=false）/ `qemu-virt`（auto=true） | `crates/kernel/src/kernel_host/platforms/*.json` |
| 契约 `PlatformDescription.auto_selectable: bool`（serde default/skip） | `crates/contract/src/platform.rs` |
| 前端 `defaultDraft()` 显式写死 `auto_selectable: false`（用户板不参与自动选档） | `front/views/components/memory-map.js:366` |
| 装载时：`user_board` Some ⇒ 应用用户板；None ⇒ `choose_platform`（必然装得下 ⇒ 「未知系统」不该出现） | `mod.rs:2753-2844` |

### 2.2 「未知系统」门禁（B2 要改判据的那条）

- 现在：**只对用户配的板**判 —— `board_intervals(board)` + `compatibility(ranges, board_ranges)`，完全落不在 ⇒ `E_PLT-17`（`E_PLT_17_BOARD_MISMATCH`，v0.71 先例）。
- 中档口径（用户 2026-11-01 拍板）：只有「完全落不在」才拒；「部分落不在」不拒（撞的部分由总线报既有码）。
- None 分支走自动选档 ⇒ 不判（自动选档按地址挑，必然装得下）。

### 2.3 引脚表达现状（两套 · 分层裁决已钉）

| 表达 | 语义 | 落点 |
| --- | --- | --- |
| `pins` 段：`PinBinding { device, pin, signal }`（v0.73 D1） | 「哪个引脚接到哪个信号」（器件内部接线）—— **今天只被 V-21 校验形状（器件在不在），内核不读、不产生行为** | `crates/contract/src/platform.rs:336,597` · V-21 在 `:1284` |
| `wiring.from.pin`（v0.74 D） | 「这根线的源是哪个引脚」（连线寻址）→ 宿主 `set_irq_line_for_pin(pin, wire.source)` | `platform.rs:989` |

- **分层裁决**（STAGE-50 §7.1 订正 5/10）：`pins` 段说「这个引脚接到哪个信号」· `wiring` 段说「哪个信号接到 PLIC 的哪个源」⇒ 各写各的那一层、中间层翻译；⛔ **不许** `pins` 段直接写 `irq_lines[pin]`（互相覆盖 ⇒ 行为随「谁后到」漂 ⇒ 判别力失效）。
- 事件词表 `WIRING_EVENTS`（`rx_ready` 电平 / `pin_irq` 一次性）+ `wiring_event_info()` 唯一判定入口 —— **这是「事件名合法」的唯一落点模式**；本批**信号名不词表化**（用户拍板开放），但**方向**是平台枚举（封闭）。

### 2.4 GPIO 器件已有能力（引脚语义的器件侧底座）

- `GpioBank`：`directions`（DIR 寄存器 1=输出/0=输入，**固件**写）、`outputs`（OUT 锁存）、`inputs`（IN，宿主可喂）、`irq_lines: [u16; PIN_COUNT]`、`set_external_inputs`（模拟按键/传感器）。
- `GPIO_PIN_COUNT` 契约层唯一落点；`DeviceTypeInfo` 有引数字段（STAGE-50 裁决 F，前端查不抄）。
- UART 无引脚概念（rx/tx 是内部队列）。

### 2.5 契约 / 协议现状

- `PLATFORM_SCHEMA_VERSION = 4` · `PLATFORM_RULES_VERSION = 6` · 信封 `SCHEMA_VERSION = 11` · `SIM_STATE_SCHEMA_VERSION = 2`。
- `KINDS: [&str; 5] = ["gpio", "uart16550", "clint", "plic", "timer_riscv"]`（唯一落点 `:774`）；前端 `DEVICE_KINDS` 逐字镜像（`memory-map.js:151`）。
- 场景码：`E_PLT_01..E_PLT_21`（`E_PLT_21_MCU_INVALID` 最大）；`E_PLT_17_BOARD_MISMATCH` = 「未知系统」。
- 内核 `isa_gate_error`（v0.75）：装载 / 应用待生效板时判（K-2 口径：提交 SetMemoryMap 只记待生效）。
- 器件工厂：`platform.rs:319` 起 `DeviceSpec::match`（Gpio/Uart16550/Clint/Plic/TimerRiscv）→ `BuiltDevice`；`resolve_roles`（`roles.time_source` 等）。
- 快照：`SIM_STATE_SCHEMA_VERSION = 2`；器件**逐实例状态**进快照（`DeviceStateDto` 机制，板级 `memory.map` 另存）。

### 2.6 前端现状

- `currentDraft(state) = state?.draft ?? defaultDraft()`（v0.75：生效板不再自动实例化；defaultDraft 无器件）。
- 「从内置模版开始」按钮（`data-action="load-current"`）= 把**当前生效板**导入草稿 —— 不是模板列表。
- 只读横幅：「当前生效板：QEMU virt（自动选择 · 仅用于运行 · 只读）」—— v0.75 顺手整改① 产物。
- 产品树 = 草稿派生视图（`draftDevices` = `kind === "peripheral"` 的段）；空态提示「还没有器件实例」。

## §3 裁决 A~N

| # | 裁决 | 理由 |
| --- | --- | --- |
| **A** | **一次升版**：`PLATFORM_SCHEMA_VERSION` **4 → 5**（`auto_selectable` 删除 + `pins` 段升级为引脚规格）· `PLATFORM_RULES_VERSION` **6 → 7**（新规则 V-24 引脚规格 · V-25 pinmux 器件 · V-26 无板装载）⇒ ⇒ 信封 `SCHEMA_VERSION` **不动**（仍 11 · 线格式无感先例 v0.75）· `SIM_STATE_SCHEMA_VERSION` **不动**（仍 2 · pinmux 寄存器状态走既有逐实例器件状态机制） | 一次升版封顶三件（F-10 快照不动） |
| **B** | **`auto_selectable` 字段废弃**（契约层 `PlatformDescription` 删除该字段；`defaultDraft` 不再写死 false；三块内置板 json 删除该键）⇒ ⇒ **老描述带 `auto_selectable` 键 ⇒ 仍能解析**（serde 忽略未知字段 · 契约 `validate` 不管该键 ⇒ 存量板档案零风险） | B3 决议 + 缺省优先 |
| **C** | **自动选档链路删除**：`choose_description` / `choose_platform` 不再被装载流程调用（函数可保留仅供测试对照 · 调用点摘除）⇒ ⇒ `user_board` None 且无生效板 ⇒ **装载 / 运行 ⇒ 拒 + 引导**（新码 `E_PLT_22_NO_BOARD`，前端提示「先导入模板或配置板」） | B1 决议 · 不静默兜底 |
| **D** | **「未知系统」门禁判据改为「即将生效的板」的 RAM 范围**：`user_board` Some ⇒ 应用前按 `user_board` 判（现状逐字）；None ⇒ **不再自动选** ⇒ 直接「无板」拒（裁决 C）⇒ ⇒ 判据语义 = 「当前生效板 / 即将生效的板装不下这个固件」 | B2 决议 |
| **E** | **`pins` 段升级为引脚规格**：`Vec<PinSpec>`，`PinSpec { device, pin, functions: Vec<PinFunction> }`，`PinFunction { name: String（用户自由命名）, direction: PinDirection（平台封闭枚举）, irq: Option<PinIrq { source: u32 }> }` ⇒ ⇒ **无 `pins` 段 ⇒ 无引脚规格**（缺省优先 · 存量零风险） | 用户拍板 · 功能列表形态 |
| **F** | **`PinDirection` = 平台枚举**（`Input` / `Output` / `Bidir`）⇒ ⇒ 方向是**平台行为语义**（输出⇒固件写 OUT 驱动 · 输入⇒允许喂 `set_external_inputs` · 双向两可）⇒ ⇒ **封闭**、装载时校验 | 平台基座：方向归平台、功能名归用户 |
| **G** | **信号名 `name` 开放**：自由字符串、**不词表化**；平台只做跨描述一致性（同名功能名在同一引脚内唯一）⇒ ⇒ 外设扩展沿用 `KINDS` 注册制，不限 GPIO/UART | 用户拍板（开放 · 可扩展） |
| **H** | **新场景码**：`E_PLT_22_NO_BOARD`（无生效板装载/运行 ⇒ 拒 + 引导）· `E_PLT_23_PINMUX_INVALID`（pinmux 寄存器写非法功能名 / 源号越界等运行时错误）⇒ ⇒ 引脚规格**静态**非法（器件不在 · 引脚号越界 · 方向非法 · 同引脚重复声明）**复用既有 `E_PLT_19_PINS_INVALID`**（V-21 语义扩展） | 场景码语义唯一 · 静态/运行时分层 |
| **I** | **pinmux = 独立器件**（`DeviceSpec::Pinmux { id, base, pins: Vec<String> }` · `KINDS` 5 → 6）：寄存器按引脚组织（每引脚一个**功能选择**字段，固件写当前功能名/索引）⇒ ⇒ **平台通用基座** · 服务所有器件的引脚 · 不塞进 GPIO | C1 决议 |
| **J** | **中断绑「引脚 × 当前功能」**：平台感知引脚活动 → 查 **pinmux 当前功能** → 取该 `PinFunction.irq.source` → 抬那个源 ⇒ ⇒ 同一引脚不同时刻、不同功能可拉**不同**中断线（STM32 PA9 先例）⇒ ⇒ **平台不解析功能名**（基座原则） | C2 修正决议 |
| **K** | **分层实现**：`PinFunction.irq` 是**声明**（描述层）⇒ 内核经 **pinmux 器件**翻译成运行时路由（pinmux 持有「引脚 → 当前功能 → 源」映射）⇒ ⇒ ⛔ **不许**直接写 GPIO 的 `irq_lines[pin]`（STAGE-50 禁令保持）⇒ ⇒ 与 `wiring.from.pin`（`set_irq_line_for_pin` 路径）**并行不互写** | 分层裁决落地 |
| **L** | **冲突校验**：同一引脚若同时有 `PinFunction.irq` 与 `wiring.from.pin` 绑定 ⇒ ⇒ 两者源号**必须一致**（不一致 ⇒ 拒 `E_PLT-19`）⇒ ⇒ 一致时两条路径殊途同归 ⇒ 判别力不失效 | 防「谁后到漂」 |
| **M** | **前端「从模板导入」**：内置板清单由**内核报**（新协议：内置板模板列表查询 ⇒ 名称/描述/器件数）⇒ ⇒ 前端模板库 UI（3 预设可见 · 选一块导入草稿 = `draftFromDescription`）⇒ ⇒ `load-current` 按钮升级为模板库入口 · 前端**不抄**模板数据 | 一个真相源 · 前端查不抄 |
| **N** | **范围**：完整闭环（模板库 + 不兜底 + 引脚规格 + pinmux 器件 + 固件可切 + 真机验证切换后中断跟着变）；**前端编辑面压缩为展示**（引脚规格只读展示 · 编辑面留后续批） | 用户认可的范围取舍 |

> ⚠️ **V 编号**：V-21（引脚表）语义**扩展**为「引脚规格校验」（器件在 + 引脚号 + 方向 + 同引脚唯一 + 冲突）；新规则 **V-24**（引脚规格形状 · 码 E_PLT-19 复用）· **V-25**（pinmux 器件形状 + 运行时判据 · 码 E_PLT-23）· **V-26**（无板装载 ⇒ E_PLT-22）。`PLATFORM_RULES_VERSION` 6 → 7。

## §4 判别力清单（落地前先钉住）

### §4.1 契约与跨层（A 组）

| 编号 | 判据 |
| --- | --- |
| **A-1** | `PLATFORM_SCHEMA_VERSION` == **5**；`PLATFORM_RULES_VERSION` == **7**；`SCHEMA_VERSION` == **12**（本批升版：模板协议新增四条 ⇒ 线上形状变了 ⇒ 信封 11 → 12）；`SIM_STATE_SCHEMA_VERSION` == **2** |
| **A-2** | `auto_selectable` 从契约层**删除**（线上形状无此键）⇒ 老描述**带该键仍能解析**且仍合法（serde 忽略未知字段） |
| **A-3** | `pins` 段**无**时 ⇒ 无引脚规格、无 pinmux 行为（存量零风险）；`pins` 段**有**时 ⇒ 每个 `PinSpec` 含 `functions: [ { name, direction, irq? } ]`，线上形态逐字钉住 |
| **A-4** | `PinDirection` 封闭：`input` / `output` / `bidir`，其它值 ⇒ 拒（`E_PLT-19`） |
| **A-5** | `PinFunction.irq.source` 为 `Option<u32>`；同一 `PinSpec` 内功能名**唯一** |
| **A-6** | `KINDS` == `["gpio","uart16550","clint","plic","timer_riscv","pinmux"]`（顺序 = 报告顺序）⇒ 前端 `DEVICE_KINDS` 逐字镜像 |
| **A-7** | 新码 `E_PLT_22_NO_BOARD` / `E_PLT_23_PINMUX_INVALID` 唯一落点；`E_PLT-19` 语义扩展覆盖引脚规格静态非法 |

### §4.2 内核（K 组）

| 编号 | 判据 |
| --- | --- |
| **K-1** | **无板装载拒 + 引导**：无 `user_board`、无生效板时 LoadProject ⇒ `E_PLT-22`，机器/生效板/修订号**全不动** |
| **K-2** | **自动选档摘除**：装载流程 `None` 分支**不再调用** `choose_description`（调用点摘除 · 函数保留仅供测试对照） |
| **K-3** | **未知系统 B2**：生效板（用户板或已应用板）RAM **完全落不进**固件地址 ⇒ `E_PLT-17`（判据 = 即将生效的板，不再有自动选档候选） |
| **K-4** | **pinmux 通电**：描述声明 pinmux + 引脚规格（`gpio0.pin3` 功能 `gpio_in`·`irq.source=11`）⇒ 固件写 pinmux 选择 `gpio_in` ⇒ 该引脚活动 ⇒ **源 11 被抬** ⇒ guest claim 得到它 |
| **K-5** | **切换跟着变**：同一引脚两个功能（`uart_tx`·源 3 / `gpio_in`·源 11）⇒ 固件切功能 ⇒ 活动后拉**当前功能**的源（先 3 后 11 可验证）⇒ **同一引脚两时刻两中断** |
| **K-6** | **分层禁令**：pinmux 路径**不写** GPIO `irq_lines[pin]`（通过 pinmux 器件映射抬源）⇒ 与 wiring 路径并行不互写 |
| **K-7** | **冲突校验**：引脚同时有 `PinFunction.irq` 与 `wiring.from.pin` ⇒ 源号不一致 ⇒ 拒 `E_PLT-19` |
| **K-8** | **方向行为**：`PinFunction.direction = output` 的引脚 ⇒ 固件写 OUT 驱动电平可见；`= input` ⇒ `set_external_inputs` 可喂、固件读 IN 拿到 |
| **K-9** | ⚠️ **必要对照**：无 `pins` / 无 pinmux 的存量描述**照装**（自动选档摘除后：先导入模板再装 · 行为一致） |
| **K-10** | **pinmux 运行时**：写不存在的功能名 / 源号越界 ⇒ `E_PLT-23`（报错不静默） |

### §4.3 前端（N 组）

| 编号 | 判据 |
| --- | --- |
| **N-1** | 前端镜像**四个版本号**与契约层逐字相同；`DEVICE_KINDS` 逐字含 `pinmux` |
| **N-2** | **模板库**：从模板导入可见 **3 块预设**（default / minimal / qemu-virt · 数据来自内核）⇒ 选一块导入草稿 ⇒ 器件/段/MCU 都在 |
| **N-3** | **无板状态**：无生效板时界面提示「先导入模板或配置板」· 装载/运行引导（不显示「自动选择」） |
| **N-4** | **只读展示**：生效板/草稿的引脚规格（功能名 · 方向 · irq 源）只读展示 ⇒ 前端**零处**自己判合法性（内核唯一校验器） |
| **N-5** | `defaultDraft()` 不再含 `auto_selectable` 键 |

### §4.4 真机门禁（B 组 · B-168 ~ B-174）

| 编号 | 判据 |
| --- | --- |
| **B-168** | 真机打开模板库 ⇒ 3 预设可见 ⇒ 导入 qemu-virt ⇒ 草稿带 CLINT/PLIC/UART0/GPIO0 |
| **B-169** | ⚠️ **必要对照**：**不导入不配板** ⇒ 装载/运行 ⇒ 界面显示 `E_PLT-22` + 引导提示（不静默兜底） |
| **B-170** | **B2 真机**：导入 minimal（RAM 小）→ 装大镜像（qemu-virt 才装得下的地址）⇒ `E_PLT-17`「未知系统」 |
| **B-171** | **pinmux 通电**：描述带 pinmux + 引脚规格（`gpio0.pin3 → gpio_in · 源 11`）⇒ 固件写选择 ⇒ 活动 ⇒ guest claim 到源 11 打出预期标记 |
| **B-172** | **切换跟着变**：同一引脚两功能两源 ⇒ 固件先选 A（claim 源 A）再切 B（claim 源 B）⇒ 同一引脚两时刻两中断 |
| **B-173** | ⚠️ **必要对照**：老描述（无 `pins` / 无 pinmux）⇒ 导入模板后照装、跑通（存量零风险） |
| **B-174** | ⚠️ **必要对照**：带 `auto_selectable` 键的老板描述 ⇒ 仍能解析、导入、装载（B 裁决） |

### §4.5 交叉（X 组）

| 编号 | 判据 |
| --- | --- |
| **X-1** | Rust 侧四个版本号 == 前端镜像四个版本号；`KINDS` 与 `DEVICE_KINDS` 逐字相同 |
| **X-2** | `tools/stage52_check.py` 按**跨批纪律**（X-2 明文 · 先例 stage50）：版本号**性质断言**（`PLATFORM_SCHEMA_VERSION` ∈ SUPPORTED · `PLATFORM_RULES_VERSION` 只许升）；新码放行归各批自守 |
| **X-3** | 契约层「引脚规格是否合法」== 内核装载时判据（同一 V-21 语义 · 不双写） |

## §5 变异计划（`tools/mutation_v076.py` · 五条铁律）

| 变异 | 改什么 | 预期红集 |
| --- | --- | --- |
| **T-1** | pinmux 器件不接线（删「当前功能 → 源」映射） | **K-4**（通电判据） |
| **T-2** | 中断绑引脚不绑功能（切功能不换源） | **K-5**（切换跟着变） |
| **T-3** | 冲突校验删掉（两源不一致也放行） | **K-7** |
| **T-4** | 无板装载走回自动选档（choose_description 复活） | **K-1 + K-2**（B-169 同语义） |
| **T-5** | 未知系统门禁判回自动选档候选 | **K-3** |
| **T-6** | 方向枚举放行任意字符串 | **A-4**（契约） |
| **T-7** | **对照**：只改注释 | **零红**（必须） |
| **N-4** | 顺手整改反向：前端退回「自动选择生效板」横幅 | **N-3**（无板状态提示） |

> ⚠️ 红集**只能由工具跑出来**（先例 STAGE-50 §7.2 / STAGE-51 §7.2）；表中是**预期**，施工期以实测回填。

## §6 本批明确不做

1. **引脚规格的编辑面**（前端只读展示 · 编辑留后续批）。
2. **功能切换的「候选列表」校验**（功能名开放 · 平台不解析外设语义）。
3. **信号级路由**（`wiring.from.signal` —— 后续外设需要时再加 · 本批不封死）。
4. **MUX 寄存器复杂语义**（多路复用矩阵 / AF 寄存器组 —— 本批 pinmux = 每引脚一个功能选择字段，够验证「切换跟着变」）。
5. **快照升版**（pinmux 状态走既有逐实例器件机制 · `SIM_STATE_SCHEMA_VERSION` 不动）。

## §7 施工期订正（施工中回填 · 不是预测）

> 施工期撞见的事实订正 / 实测红集回填写在这里（先例 STAGE-50 §7 / STAGE-51 §7）。

### 7.1 施工期事实订正

1. **A-4 判别力补半（A-4b）**：原 A-4 只断「非法值拒」⇒ 把 `rename_all`
   改坏也**看起来像**「封闭更严」⇒ 补 `a4b_legal_directions_still_parse`
   （`input` / `output` / `bidir` 必须照常解析为对应变体）⇒ 变异 T-6 才有
   击杀面。契约层测试 303 条（+1）。
2. **K-1 / M-1 / M-2 / M-3 用例描述式命名**：模板套件这四个用例名**不带**
   `k1_`/`m1_` 前缀（`loading_without_a_board_...` 等）⇒ 变异工具红集解析
   需**已知映射表**补桶（`mutation_v076.py` 的 `KNOWN`）。
3. **前端「自动选择」残留文案清理**：`renderEffectiveBanner` 旧文案
   「（自动选择 · 仅用于运行 · 只读）」是 v0.75 残留 ⇒ B3 摘除后**不存在**
   「自动选择」概念 ⇒ 改为「（仅用于运行 · 只读）」（N-3 判据同源）。
4. **T-1 红集连带**：`take(0)` 禁掉整条 2.5 分支 ⇒ K-5 / K-6 / K-8 与 K-4
   **同源**一起红（都依赖「沿 → 抬当前功能源」的同一路径）⇒ 判别力口径 =
   **预期 ⊆ 实测**（红集可重叠，落点不同）。

### 7.2 变异自检实测红集（`tools/mutation_v076.py` · 全部还原 + 基线复验 + 信号安全）

| 变异 | 改了什么 | 实测红集（`--json`） | 判据 |
| --- | --- | --- | --- |
| **T-1** | pinmux 不接线（`pinmux_routes.iter().take(0)`） | 红集 = `K-4`（连带 K-5 / K-6 / K-8） | §5 · 通电判据 |
| **T-2** | 中断绑引脚不绑功能（`functions.first()` 不切） | 红集 = `K-5` | §5 · 切功能换源 |
| **T-3** | 冲突校验删（`if false && *wired != irq.source`） | 红集 = `K-7` | §5 · E_PLT-19 |
| **T-4** | 无板装载不拒（None 分支 `if false` 包 return） | 红集 = `K-1` | §5 · B1 拒无板 |
| **T-5** | 未知系统判据失效（`if false && verdict.missing`） | 红集 = `K-3` | §5 · B2 判据 |
| **T-6** | 方向枚举 rename 破坏（`SCREAMING_SNAKE_CASE`） | 红集 = `A-4`（A-4b 合法面） | §5 · 封闭 |
| **T-7** | **对照**（只改注释） | **零红** ✓（对照） | §5 · 非对照 |
| **N-4** | 无板引导面退回「自动选择生效板」横幅 | 红集 = `N-3` | §5 · 前端不显示自动选择 |

## §8 收口检查（施工中逐项勾）

- [ ] 全量门禁：cargo test（workspace 全绿）· fmt · clippy -D warnings（0 错）· 前端全量 · WASM 重建 + 真机 smoke
- [ ] 真机门禁 B-168 ~ B-174 全绿（含必要对照）
- [ ] 变异自检（T-1~T-7 + N-4，红集实测回填 §7.2）
- [ ] stage52_check / stage51_check（跨批：不得因本批改动而红）· stage50_check 仍全过
- [ ] error_code_check（新码唯一落点）· ledger_check（D-175 当前列更新）· doc_check（基线内）
- [ ] 文档收口：README / AGENTS / USER_GUIDE / PROGRESS / REVIEWS（D-175 剩项）

## §9 与台账的关系

- **D-175**：本批做掉 **D 的剩余部分（引脚的复用功能 / 方向 + 固件动态切换）** + **模板导入（不兜底）** ⇒ ⇒ 台账「当前」列更新 ⇒ ⇒ **D-175 剩 β（已取消）** ⇒ ⇒ 本批后 D-175 是否整条销账：**路线 β 取消 ⇒ 无剩项**（引脚的复用功能/方向做完即整条闭环）⇒ 处置进 `REVIEWS-archive.md` §1（若第 60 批完整落地）。
- **PROGRESS** 60 行 ⏳已拍板未开发 → ✅已落地；61 行待定空出。
- **PRODUCT_BOARD_PLAN** §3 第 60 行同步。
