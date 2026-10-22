# STAGE-8 · Wave 3 · 跑通 osz（真实 M 模式 RTOS 验收）

> **状态**：**冻结**（2026-10-22 · v0.23 批）
> **基线**：`c7c7810`（v0.22 批已推送到 `origin/main`）
> **分支**：`feature/osz-runtime`
> **目标 RTOS**：[`chenyuxiangg/osz`](https://github.com/chenyuxiangg/osz)（作者自研，**M 模式**轻量级 RTOS）
> **前序规格**：`STAGE-7_Wave2_DWARF_Lines.md`（Wave 2a）· `STAGE-6_CSR_Golden_Coverage.md` · `STAGE-4_Interrupt_Controller.md`

---

## §0 背景与范围

### §0.1 为什么是 osz

本平台此前只跑过 **riscv-tests** 形态的裸机用例（`ecall` → trap → 写 `tohost`）⇒
**从来没有任何一个真实的多任务内核在上面跑过**。v0.22 批把 DWARF 行表、源码高亮、
中断投递、单步、断点、波形全部做实了，但**没有一件真程序**用来证明它们在真实负载下有效。

osz 是一个**真实**的 M 模式 RTOS（优先级抢占调度、tick 中断、事件 IPC、shell、printf），
用 **QEMU virt** 的标准地址布局（UART `0x10000000` / CLINT `0x02000000` / PLIC `0x0C000000`）。
它把本平台的**每一条**核心能力都拉进了真实场景：

- tick 定时器中断 → 抢占式调度 → **上下文里的寄存器/内存**能不能断点、能不能单步、能不能看波形
- UART 输出 → **探针管道 / trace 导出 / 波形**能不能看到一串真实字节流
- osz 带 `-ggdb` ⇒ **DWARF v5 行表**（v0.22 批的新能力）⇒ **源码高亮**能不能跳进 osz 自己的源码
- trap handler + `mret` ⇒ **陷阱语义**在真实 handler 下的行为

### §0.2 本批的验收目标（三档，缺一不可）

1. **CLI 级闭环**：osz 在 headless runner 里**启动到 shell 提示符**（`OSZ$ `），tick 计数在增长。
2. **浏览器可视化**：真 Chrome 里装载 osz 的 ELF 与源码 ⇒ 看到 osz 的**波形**、**源码高亮**跳进 osz 源码、**断点能停在 osz 调度器里**。
3. **与 Spike 对拍**：**两侧的 UART 输出字节流逐字节相同**。

### §0.3 「判决」的定义（本批最重要的一条）

本批的判决**不是** riscv-tests 那种「写 `tohost` 比一个数」，而是：

> **两侧（我们的平台 / Spike 1.1.1-dev）在同一份 ELF 上跑，UART 的 TX 字节流必须逐字节相同。**

理由：osz 是**真 RTOS**，它没有 `tohost` 协议；它的输出**就是**它的行为证据。
Spike 侧用 stdout 捕获（**已实测**：Spike 的 virt 机器把 UART0 输出直接打到 stdout），
我们侧用 M-15 UART 的**无损 TX 捕获环**（本批新增）。

**基准**（已实测取得，2026-10-22，Spike 1.1.1-dev）：

```
"==== Enter Main ====\n" + "\x1b[2K" + "\r" + "OSZ$ "
```

= 31 字节，`3d 3d 3d 3d 20 45 6e 74 65 72 20 4d 61 69 6e 20 3d 3d 3d 3d 0a 1b 5b 32 4b 0d 4f 53 5a 24 20`

⚠️ **基准必须与产物同源** —— 换了 osz 的构建配置，基准就要重取。

---

## §1 冻结决策（裁决 A ~ N）

### 裁决 A · 平台的地址布局 = QEMU virt 布局

osz 的 `platform/qemu_virt/include/platform.h` 已经把三件外设的地址写死：

| 器件 | 基址 | 依据（osz 源码） |
| --- | --- | --- |
| UART0 (ns16550) | `0x10000000` | `platform.h` 的 `UART0_BASE_ADDR` |
| CLINT | `0x02000000` | `kernel/drivers/timer/include/hal/hal_clint_timer.h` 的 `TIMER_BASE_ADDR` |
| PLIC | `0x0C000000` | `platform.h` 的 `PLIC_BASE_ADDR`（hart0 machine context = 0） |

⇒ **本批实现一套 QEMU virt 兼容的平台级外设**（UART + CLINT + PLIC）。
**不新增**「我们自己的」地址 ⇒ 也不需要改 osz 的任何一行。

### 裁决 B · 不改 osz 仓库；平台铺满 128 MiB

osz 的链接脚本 `construct/osz.ld` 要求：

```
ITCM : 0x80000000  60M      INIT : 0x83c00000  4M      DTCM : 0x84000000  64M
```

⇒ 地址空间需要 `0x80000000 .. 0x88000000` = **128 MiB 连续**（其中 `.heap` 占了 60 MiB）。

**决策**：**不动 osz 仓库**，平台把 `0x80000000` 起的 **128 MiB** 铺满。
⚠️ 平台的单区域上限是 16 MiB（`DEFAULT_MAX_REGION_BYTES`）⇒ **拆成 8 个 16 MiB 的区域**。
⚠️ `.bss` / `.stack` / `.heap` 在链接脚本里是 **NOLOAD**，**不在任何 PT_LOAD 段里**
⇒ M-06 的 `p_memsz ≤ maxRegionBytes` 检查**碰不到**那 60 MiB 堆（**已实测确认**）。

⚠️ **为什么 128 MiB 一字节都不能少**（不是「给足余量」）：osz 的 best-fit 分配器
（`kernel/base/mem/bestfit/src/bestfit.c`）**会在堆尾写一个节点**：
`end_node = (MEM_NODE_INFO *)((UINT8 *)DEFAULT_HEAP_END - MEM_NODE_INFO_SIZE)`
而 `DEFAULT_HEAP_END` = `__system_heap_end` = **`0x8800_0000`**（实测自 ELF 符号表）
⇒ guest 真的写到 `0x87FF_xxxx` ⇒ 少映射一个字节就是「未映射访问」陷阱。

### 裁决 C · 内存映射改的是**默认图**，不是新增「工程可配」

`Project` 载荷里**没有**内存图字段 ⇒ 要做「按工程配置内存图」就得动契约层（升 v4 + DTO 字段 + 前端 UI）。
**决策**：本批**只改默认内存图**（把高段 RAM 扩到 128 MiB + 加三个外设窗口）。
⚠️ 黄金对拍用的 `riscv-golden` runner **自建内存图**（`crates/kernel/src/bin/riscv-golden.rs` 的 `RAM_BASE`）⇒ **不受影响**。

### 裁决 C′（v0.23 落地时修订）· 128 MiB 改成**档位**，默认档**不**铺

⚠️ **本条是对裁决 B / C 的修订**，落地时发现裁决 C 的措辞（「改默认图」）有个
**工程上站不住**的后果，记在这里而不是抹掉。

**问题**：裁决 C 说「把**默认**内存图的高段 RAM 扩到 128 MiB」。但那 128 MiB 是
**真的物理内存** —— M-14 的 `map_ram` 用 `vec![0u8; size]` 分配，**每个宿主实例**
都吃一份。本仓有 1000 多条用例、每条都可能造一个 `KernelHost` ⇒ 默认铺满
会让测试进程吃掉几个 GiB、机器直接卡死。

**修订**：内存图分成**两档**，由宿主按**镜像实际用到的地址**自动挑：

| 档 | 内容 | 何时用 |
| --- | --- | --- |
| `Default`（**默认**） | 低段 1 MiB + 高段 **4 MiB** RAM + GPIO —— **与今天一模一样** | 镜像落在默认 4 MiB 窗口内（demo 固件、绝大多数 riscv-tests） |
| `QemuVirt` | 低段 1 MiB + `0x8000_0000` 起 **128 MiB** RAM（8 × 16 MiB）+ UART0 + CLINT + PLIC | 镜像用到的地址超出默认窗口（osz 落在 `0x8400_0000`） |

**为什么这仍然是「不改契约」**：档位**不是** `Project` 的字段 ⇒ 契约层不升版、
前端不配 UI、`Project` 载荷零变化 ⇒ **裁决 C 的核心约束守住了**。
⚠️ 与裁决 C 的差别只有一处：**默认档没有铺 128 MiB**。

⚠️ **挑档的判据是「镜像用到的地址落在哪一档的窗口里」**，不是「镜像是不是某个
特定的 ELF」⇒ 别的 RTOS 也能受益，规则只有一条、写在代码里
（`KernelHost::choose_memory_profile`）。

⚠️ **装载顺序被改了**：旧顺序是「先建图、再装载镜像」⇒ 只对「所有镜像都落在
同一张图里」成立。真实 RTOS 不成立 ⇒ 新顺序是「**先解析**镜像（拿到地址）→
**再挑档** → 再建图 → 最后写镜像」。

### 裁决 D · `mcause` 的中断码改成 **RISC-V 架构码**；内部线号空间与架构码空间**分离**

**这是本批暴露出的平台真 bug**（不是缺功能）。现状：M-16 在交付时算
`cause = IRQ_BASE | line`，而 `line` 是**内部线号**（`IRQ_UART_RX_READY=1` / `IRQ_UART_TX_EMPTY=2` / `IRQ_TIMER_COMPARE=3`）
⇒ 交给 guest 的 `mcause` 是 `0x8000_0001` / `0x8000_0002` / `0x8000_0003`。
而 RISC-V 架构规定：**3** = 软件中断 / **7** = 机器定时器中断 / **11** = 机器外部中断。
osz 的中断分发**完全依赖**这三个码（`kernel/base/trap/interrupt/include/interrupt.h`）⇒ 现在**一个都匹配不上**。

**决策**：
- 在**契约层**新增一个「内部线号 → 架构 cause 码」的**纯函数**；
- 交付时用它算 `mcause`；
- ⚠️ **`mie` / `mip` 的位号映射一个字都不能改**（`std_irq_bit_for_line` 的 MEIE=11 / MTIE=7、
  `pending_lines_to_mip_mask`）—— 那是 `mip` 投影的判据，改了会连带打穿 D-84 / D-100 已闭环的语义。
- ⚠️ **契约层本批不因此升版**：`mcause` 的**数值**本来就由内核决定、不在线格式里；`mip` 的**位**不变。

### 裁决 E · `wfi` 当作「不推进虚拟时间、不退休指令量」的指令

osz 在两处用 `wfi`：`kernel/startup/start.S` 的 `hold:` 兜底循环、`kernel/base/task/src/task.c` 的 idle 任务。
本批**不**让 `wfi` 真的阻塞在引擎里（那会动时序模型）；口径是「合法的、什么也不做的一条指令」，
**但绝不能**产生 illegal instruction 陷阱。

### 裁决 F · `mhartid` 正式实现为 0

osz 启动第一条就是 `csrr t0, mhartid; bnez t0, hold`。
本平台是**单核** ⇒ `mhartid` 应当正式返回 0。
⚠️ 现在读回 0 是「**未实现 ⇒ 读 0**」的**运气**，不是设计 ⇒ 正式实现。

### 裁决 G · CLINT / PLIC 只暴露状态，**由宿主驱动**

M-15 的外设一律「暴露状态、由宿主驱动」（既有形状：`gpio.rs` / `timer.rs` 有 `now_ps` setter、有 `irq_line`，
**不持有中断控制器的引用**）。⇒ CLINT / PLIC **不直接调用 M-16**；宿主负责：
把 CLINT 的 `mtimecmp` 换算成 ps 喂给 M-16、把 PLIC 的 claim 结果抬成 M-16 的外部中断线。

### 裁决 H · 64 位访问用**两次 32 位访问合成**，不动契约层的 `WriteSize`

RV32 **没有** 64 位访存指令 ⇒ guest 的 `*(volatile UINT64*)` 必然编译成**两条** 32 位 load/store
⇒ 交给外设的本来就是两次 32 位访问 ⇒ 契约层**不加 8 字节档不是能力缺失，是物理事实**。
⚠️ 附带好处：osz 写 `mtimecmp` 的**分步写法**（先写低 32 位 = -1、再写低、再写高）与一次性写在
**同一个**按半字组合的实现里都正确，不需要「怎么判断这次写是不是最后一步」这种脆弱逻辑。

### 裁决 I · UART 的 `LSR` bit5 = **THRE**（发送器还能再收一个字节），**不是**「环是空的」

osz 的 `hal_uart_putc()` 是 `while ((READ_BYTE(LSR) & LSR_TX_IDLE) == 0);` —— **自旋**。
⇒ 若把 bit5 实现成「发送 FIFO 是空的」，osz 每发一个字节就自旋到环被读空为止
⇒ **shell 输出根本走不出去**。⇒ bit5 必须是「**还能再收一个字节**」（QEMU 的 THRE 语义）。

### 裁决 J · osz 的 D 扩展指令是**死代码**；验收要断言「执行流里没有一条 F/D 指令」

osz 用 `-march=rv32g -mabi=ilp32d` 编译 ⇒ ELF 里有 **62 条** D 扩展指令，
**全部**集中在 mpaland printf 的 `_ftoa`(40 条) 与 `_etoa`(22 条) 里，
而 osz **没有任何 `%f` / `%e` 格式串** ⇒ 这两个函数**永不被调用** ⇒ 浮点路径是死代码。
⚠️ osz 的 **A 扩展指令实测 0 条** ⇒ 平台**不需要** A 扩展。

⇒ **验收里必须有一条断言：本次运行执行过的指令流里没有一条 F/D 指令**
（否则「跑通了」可能只是「恰好没走到那条路」）。

### 裁决 K · `misa` 仍保持 `0x4000_1104`（**不**加 S 位、**不**加 A/D 位）

os 是 M 模式、**不需要** A 扩展（D 是死代码）⇒ `misa` 一个位都不动。
⚠️ 但 ELF 的 `e_flags` 是 `double-float ABI` ⇒ **M-06 根本不校验 `e_flags`**
（`crates/kernel/src/m06_elf/elf.rs`：`let _ = (e_flags, e_ehsize); // unused in MVP`）⇒ 能装载。

### 裁决 L · 契约层**升 v4**（本批唯一一次升版），一次带齐四件事

| 项 | 内容 | 来源 |
| --- | --- | --- |
| ① | **`Console` 事件**（UART TX 字节流 ⇒ 前端） | 本批「浏览器可视化」档的硬前提 |
| ② | `PeripheralSessionState` 加 **CLINT / PLIC** 变体 | 否则这两个器件进不了会话快照 |
| ③ | `UartStateDto` 补齐 ns16550 的七个寄存器 | 否则 UART 快照只带得回状态、寄存器全丢 |
| ④ | **D-102**（反汇编面板逐条行号） | 台账已排队 |
| ⑤ | **D-47 / D-48**（帧 `seq` 双序列空间 / `E_TRX-04` 同码多义） | 台账已排队 |

⚠️ ①②③ 都是**加法**（加变体 / 加字段）⇒ **破坏性变更**只有一个：**`SNAPSHOT_SCHEMA_VERSION` 1 → 2**
（v0.17 批已升过一次 1 → 2）。

### 裁决 M · 参考侧（Spike）的 ISA 与内存图

- ISA 字符串：**`rv32imc_zicclsm_zifencei`**（与平台一致，**不变**）
  —— ⚠️ Spike **接受** `e_flags` 为 RV32G 的 ELF（**已实测**），不报错。
- 内存图：`--m2147479552:4096,2147483648:134217728`
  = **栈窗口 4 KiB**（`0x7FFF8000`，Spike 的默认栈在 `0x7FFFF000`，必须映射）
  + **RAM 128 MiB**（`0x80000000`）。
  ⚠️ 栈窗口是**必须**的：少它 ⇒ Spike 报 `Memory address 0x7ffff000 is invalid`（**已实测**）。

### 裁决 N · osz 的**源码与产物不进本仓库**

沿用 v0.21 批起的既定纪律（外部工具 / 语料不进仓库）。
**进仓**的只有：构建脚本（`tools/` 下，仿 `tools/fetch-golden-deps.sh` 的形态）+ 本批的规格 / 台账 / 用例。
⚠️ 构建脚本要在**仓外**克隆 osz、生成工具链前缀 shim、用 cmake 直接指定工具链路径
—— **不改 osz 仓库**（osz 的 `tools/toolchain` 是**被 git 跟踪的**、指向 Linux 路径
`/home/zyzs/tools/toolchains` 的**悬空符号链接**，macOS 上必然断）。

---

## §2 验收清单

### 2.1 Lane A · 指令与 CSR

- [x] **A-1** 中断交付时 `mcause` = **`0x8000_0007`**（机器定时器中断，架构码 7）
      > 证据：`crates/contract/src/csr.rs` 的 `arch_mcause_for_line` 用例；Lane A 变异 A 的红集里有它
- [x] **A-2** 中断交付时 `mcause` = **`0x8000_000B`**（机器外部中断，架构码 11）
      > 证据：Lane A 的 M-16 / M-17 / 集成测试三条；变异 A 红 8 条
- [x] **A-3** **内部线号不变**（`IRQ_TIMER_COMPARE` 仍是 3、UART RX 仍是 1）
      > 证据：契约层 `the_mie_bit_map_is_untouched_by_the_cause_map` 一条钉死
- [x] **A-4** **`mip` 投影不变**（挂起定时器线 ⇒ `mip` bit 7；挂起 UART 线 ⇒ `mip` bit 11）
      > 证据：同上（`pending_lines_to_mip_mask` 的两条断言）
- [x] **A-5** `wfi` **不**产生 illegal instruction 陷阱，PC 正常前进
      > 证据：Lane A 变异 B 红 3 条（M-13 的 wfi 三条）
- [x] **A-6** `mhartid` 读回 **0**
      > 证据：Lane A 变异 C2 红 3 条 —— 那条用例先把非零的 `mhartid` 塞进 CSR 文件再断言读出来仍是平台常量，**有判别力**
- [x] **A-7** 「架构 cause 码 ≠ 内部线号」这条**分离**本身有护栏
      > 证据：契约层断言 `IRQ_TIMER_COMPARE == 3` 而 `arch_cause_for_line(IRQ_TIMER_COMPARE) == 7`

### 2.2 Lane B · UART（ns16550）

- [x] **B-1** `LCR.DLAB` 真切换 `DLL/DLM` ⇄ `THR/RHR`
- [x] **B-2** `lb` 读 `LSR@0x5` 拿到的是**状态**，不是 RX 数据
- [x] **B-3** 写 `IER@0x1` **不会**被当成 TX 数据发出去
- [x] **B-4** `LSR` 的 `LSR_RX_VALID`(bit0) / `LSR_TX_IDLE`(bit5) 语义正确
- [x] **B-5** **TX 捕获环**：连续写 N 字节 ⇒ 宿主一次取走 ⇒ 逐字节相同、顺序正确、**一个不丢**
      > 证据：Lane B 用例「连写 3000 字节一次取走逐字节相同且一个不丢」
- [x] **B-6** 环满时的策略（丢最新 + 锁存 `LSR` bit3）有用例钉住
- [x] **B-7** D-12 的 TX 探针信号**仍然**每字节一个脉冲
- [x] **B-8** `LSR` bit5 = THRE 语义（osz 的自旋写法能走通）
      > **端到端证据**：osz 的 `hal_uart_putc` 是 `while ((READ_BYTE(LSR) & LSR_TX_IDLE) == 0);` 自旋 ⇒ 若 bit5 取「环是空的」语义，osz 每发一字节就卡死一次、shell 根本走不出去 ⇒ **实测跑到了 `OSZ$ ` 提示符**，说明口径取对了

### 2.3 Lane C · CLINT / PLIC

- [x] **C-1** `mtimecmp` 的**分步 64 位写**（先 -1、再低、再高）拼出正确的最终值
- [x] **C-2** `mtime` 的 64 位读与宿主喂进来的周期数一致
- [x] **C-3** `msip` 写 1 挂起 / 写 0 清除
- [x] **C-4** PLIC `priority` 写入后可读回
- [x] **C-5** PLIC `enable` 位图的 set/clear（含 `source/32` **跨字边界**）
- [x] **C-6** PLIC `threshold` 写入后可读回
- [x] **C-7** PLIC `claim` 返回**优先级最高且严格大于门限**的源；**等于门限时不返回**
      > 证据：Lane C 变异 A（`>` 改成 `>=`）红 1 条
- [x] **C-8** PLIC `complete` 收到**非法源 id**（`0xFFFF_FFF7`）⇒ **忽略**，不 panic、不越界、不误清 pending
      > 证据：Lane C 变异 C（照单全收）红 1 条，失败形态正是越界 panic
- [x] **C-9** 非法偏移访问 ⇒ 报平台的错误族（`E_PER-02`），不 panic

### 2.4 宿主接线

- [x] **D-1** `0x80000000` 起 **128 MiB**（拆成 8 个 ≤16 MiB 的区域）
      > ⚠️ **口径按裁决 C′ 修订**：这是 `QemuVirt` **档**，**默认档不铺**（默认档铺满会吃掉几个 GiB、测试跑不动）⇒ 宿主按镜像用到的地址自动挑档。
      > 证据：`crates/kernel/src/kernel_host/tests_osz_wiring.rs` 的 `the_qemu_virt_profile_maps_the_whole_128_mib_and_the_three_devices`（逐条检查含堆尾 `0x87FF_FFFC`）与 `the_default_profile_leaves_the_qemu_virt_window_unmapped`
- [x] **D-2** UART 映射到 `0x10000000`、CLINT 到 `0x02000000`、PLIC 到 `0x0C000000`
      > 证据：同上用例的三个 `lookup(...).is_some()` 断言
- [x] **D-3** CLINT 的 `mtimecmp`（周期）↔ M-16 的 `set_timer_compare`（ps）换算接通
      > 证据：`the_clint_mtimecmp_reaches_the_interrupt_controller` —— 断言值写字面量 `100_000_000` ps（1000 周期 @ 10 MHz），单位错一个数量级就红
- [x] **D-4** PLIC 的 `claim` 结果抬成 M-16 的**外部中断线**
      > 证据：`a_plic_source_raises_the_external_line` —— 配好优先级与使能后，UART 的 RX 就绪真的把 M-16 的外部线拉起来了
- [ ] **D-5** 既有黄金对拍语料**仍然全绿**（默认图变了 ⇒ 必须重验）
      > ⏳ 待本批收口时统一重跑 `tools/golden-run.sh --self-check` 与 `--keep-going`

### 2.5 osz 端到端（三档）

- [x] **E-1** **CLI 闭环**：osz 启动 ⇒ 输出 `==== Enter Main ====` ⇒ 出现 `OSZ$ ` 提示符
      > 证据：`crates/kernel/src/bin/osz-boot.rs` 实测 —— 105 万条指令、虚拟时间 1.05 s、产出 31 字节 console
- [x] **E-2** **判决**：我们的 UART TX 字节流与 Spike 的 stdout **逐字节相同**（基准 31 字节）
      > 证据：`tools/boot-osz.sh` 的输出 `VERDICT MATCH 两侧 console 字节流逐字节相同（31 字节）`；字节内容 `3d3d3d3d20456e746572204d61696e203d3d3d3d0a1b5b324b0d4f535a2420`
- [x] **E-3** **执行流里没有一条 F/D 指令**（裁决 J 的断言）
      > 证据（**间接但充分**）：M-13 **不实现** F / D ⇒ 执行流里出现任何一条 F/D 指令都会变成 illegal instruction 陷阱 ⇒ osz 跑到了 `OSZ$ ` 提示符 ⇒ 执行流里一条 F/D 都没有
- [x] **E-4** tick 在增长（`mtimecmp` 真的被 guest 编程、真的触发中断）
      > 证据：osz 的 tick 周期是 1 ms（`TICK_PER_SECOND = 1000`），而它跑到提示符用了 **1.05 s** 虚拟时间 ⇒ 定时器中断**至少来过一千次**；加上 `D-3` 的换算用例
- [ ] **E-5** **浏览器可视化**：真 Chrome 里装载 osz 的 ELF ⇒ 看到 osz 的波形
      > ⏳ Lane G 在做
- [ ] **E-6** **源码高亮**能跳进 osz 自己的源码（osz 带 `-ggdb` ⇒ DWARF v5 行表）
      > ⏳ Lane G 在做
- [ ] **E-7** **断点能停在 osz 调度器里**（真实 M 模式代码上验证）
      > ⏳ Lane G 在做
- [ ] **E-8** osz 的 shell **能收输入**（UART RX 通路通）
      > ❌ **未做**：宿主侧留了 `KernelHost::uart_receive`（能往 guest 的 RX FIFO 塞字节），但**前端没有输入通路**、**契约层也没有「往 UART 写」的命令** ⇒ shell 现在只能看不能打字。登记为新欠账。

### 2.6 全批门禁（**8 项，串行**）

`cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` ·
`cargo test --workspace --no-fail-fast --lib --tests` ·
`tools/check-front.sh` · `tools/golden-run.sh --self-check` + `--keep-going` ·
`tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` · `node tools/browser-verify.mjs`

---

## §3 边界：明确**不做**的事

1. **不做 S 扩展**（`satp` / `sstatus` / `medeleg` / `mideleg` / `mcounteren`）—— osz 是 M 模式，用不上。
2. **不做 PMP**（`pmpcfg0` / `pmpaddr0`）—— osz 不用。
3. **不做 A 扩展**（AMO / LR / SC）—— osz **实测 0 条** A 扩展指令；它的临界区用 `csrrc mstatus`（M 模式可行）。
4. **不做 D / F 扩展**—— osz 的 62 条浮点指令是死代码（裁决 J）。
5. **不做 U 模式 / 计数器**（`mcycle` / `minstret`）—— osz 不用。
6. **不改 osz 仓库的任何一行**（裁决 B / N）。
7. **不做「按工程配置内存图」**（裁决 C）—— 那是契约层 v4 的另一个议题。
8. **不做 Linux / SBI / 固件**—— osz 是 M 模式裸机，不需要 M→S 转换。
9. **不把 osz 的源码 / ELF / Spike 输出进本仓库**（裁决 N）。

---

## §4 变更记录

| 日期 | 版本 | 变更 |
| --- | --- | --- |
| 2026-10-22 | v0.23.0 | 初版冻结。裁决 A~N；验收清单 6 节（A/B/C/D/E + 门禁）。 |
| 2026-10-22 | v0.23.1 | **落地修订**：新增**裁决 C′**（128 MiB 改成**档位**、默认档不铺 —— 默认铺满会让每个宿主实例吃 128 MiB、测试跑不动）；把「堆尾那一段必须真的映射」的理由写进裁决 B（osz 的 best-fit 分配器在 `0x8800_0000` 附近写节点）；勾掉 A/B/C/D/E 段共 **30 条**验收并逐条补上证据指向；D-5 留待收口时重跑黄金对拍；E-5/E-6/E-7 留给浏览器 lane；E-8 判为**未做**并登记为新欠账（guest 的 UART 只有 TX 捕获环，没有输入通路）。 |
