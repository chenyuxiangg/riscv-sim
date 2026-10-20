# M-14 / M-15 / M-18 接线 · 代码审查报告

> **审查对象**：`crates/kernel/src/m14_bus/`、`crates/kernel/src/m15_peripheral/`、`crates/kernel/src/m18_probe/`（含新增集成测试 `crates/kernel/tests/m14_m15_m19_wiring.rs`）
> **审查基准**：`docs/detailed-design/M-14_BusMemoryMap.html`（§3.2 读写分发、§6.1 错误码）、`M-15_PeripheralModel.html`（§4.4.4 外设接口、§9.3 探针对接）、`M-18_Probe.html`（§1.3 MVP 范围、§2.1.1 表 68 通道定义）、`docs/reviews/CROSS_MODULE_DEBT.md`（D-07 / D-10 / D-12 / D-14）、`riscv_sim_contract::port`
> **审查日期**：2026-10-20
> **审查方式**：接线 PR 自审 + 端到端集成测试实证（真实 `Bus` + 真实 `PeripheralRegistry` + 真实 `ProbeRegistry` + 真实 `WaveformPipeline`，无一处绕过）
> **门禁状态**：`cargo fmt --all -- --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 干净；`cargo test --workspace` **234 passed / 0 failed**（基线 203 → 新增 31）
> **结论**：**D-12 / D-14 的内核侧闭合**，N5「GPIO 翻转可见」的**内核路径 100% 打通**（固件写 OUT → M-14 派发 → M-15 上报 → M-18 判沿 → M-19 通道出现边沿，有集成测试为证）。附带闭合 **D-10 的 M-14 侧**。仍有 3 项需要协调者决策的欠账（见 §五），其中 2 项涉及契约层。

---

## 一、这次接通的三根线

台账 §1.2 画的断点图，本 PR 全部接上：

```
M-14 Bus ──bus hook──▶ M-18 ProbeRegistry ──EdgeSink──▶ M-19 WaveformPipeline ──▶ CSV/VCD
   ✅ 已通                  ✅ 已通                        ✅ 已通
                    │
                    │ on_probe（外设信号）           ← 本 PR 接上
                    ▼
              M-15 外设 ──▶ ???
              ❌ 三个外设均未 override on_probe     ← 本 PR 接上
```

| 欠账 | 状态 | 落地位置 |
| --- | --- | --- |
| **D-12** M-14 ↔ M-15 总线接线 + size 一致性校验 | ✅ 闭合 | `m14_bus::Bus::map_peripheral_instance` / `BoundPeripheral` / `RegionTarget::PeripheralBound`；size 不一致 → **E_BUS-08** |
| **D-14** 总线读外设必须走 `read_clear` | ✅ 闭合 | `m14_bus` 的读路径一律调 `Peripheral::read_clear`（含字节切片路径）；有专门回归测试锁死「总线绝不调 `read`」 |
| **M-15 → M-18** 外设信号产生边沿 | ✅ 闭合 | `GpioBank` / `Uart16550` / `TimerRiscv` 三个外设 override `Peripheral::on_probe`；M-18 新增 `ProbeRegistry::attach_signal`（LLD §1.3 的 `attachSignal`） |
| **D-10（M-14 侧）** 区域大小上限 | ✅ 闭合 | `prepare_bytes` 分配**之前**校验上限 → **E_BUS-09**；上限可配（默认 16 MiB） |

### 接线三步法（协调者可在 M-12 启动序列里直接复用）

```rust
// 1) M-15 建实例并登记（注册表是实例的所有者）
peripherals.register(GpioBank::new("GPIO0", GPIO_BASE))?;

// 2) M-18 把外设信号绑到通道，并把回调装进外设
let handle = peripherals.handle("GPIO0").unwrap();
{
    let mut p = handle.borrow_mut();
    ProbeRegistry::attach_signal(
        Rc::clone(&probes), &mut *p,
        m15_peripheral::gpio::SIGNAL_OUT, CH_GPIO0,
        ChannelKind::SingleBit, 1,
    )?;
}

// 3) M-14 把实例挂到总线上
bus.map_peripheral_instance(GPIO_BASE, GPIO_WINDOW, handle)?;
```

参考实现见 `crates/kernel/tests/m14_m15_m19_wiring.rs` 的 `build()`。

---

## 二、关键设计决策与依据

### 2.1 总线怎么持有外设实例（`Rc<RefCell<dyn Peripheral>>`）

M-14 LLD §3.2 的分工是「**命中外设转调 M-15 的 Peripheral.read/write**」——实例归 M-15，总线只持有引用。M-15 LLD §4.4.4 的 `attach(instance, baseAddr)` 同样是「调用 M-14 mapRegion；登记 instanceTable」。两侧合起来要求的是**共享所有权**，不是转移。

选 `Rc<RefCell<dyn Peripheral>>` 而不是 `Box<dyn Peripheral>`：

- M-15 的 `PeripheralRegistry` 继续做实例所有者（instanceTable），新增 `handle(id)` 交出共享句柄；
- 总线与引擎拿到的是**同一个实例**——引擎仍能 `tick` 它、读它的状态，总线同时能把访存派发进去；
- 契约层 `Peripheral` 是 `!Send` 的，MVP 单线程，`Rc` 与 M-18 已有的 `Rc<RefCell<ProbeRegistry>>` 同一姿态；不引入 `Arc`/`Mutex` 的无谓开销。

M-14 **只依赖 `riscv_sim_contract::port::Peripheral`**，不引用任何 M-15 具体类型（AGENTS.md 硬约束）。

> **代价（已在报告里交代）**：M-15 `PeripheralRegistry::get` / `get_mut` / `iter` 的返回类型从 `&dyn Peripheral` / `&mut dyn Peripheral` 变成 `Ref` / `RefMut`。全代码库唯一的使用方是 M-15 自己的单测，已同步更新。`register()` / `describe()` / `len()` 等签名不变。

### 2.2 信号名：为什么是 `"OUT"` / `"TX"` / `"CMP"`

三份文档对信号名的说法并不一致，本 PR 取**仓库已实现的约定**：

| 信号 | 本 PR 采用 | 依据 | 其它文档的说法 |
| --- | --- | --- | --- |
| GPIO 输出 | `"OUT"` | 契约 `port.rs` 的 `ProbeSink` 文档明写 `Signal id is a stable string per peripheral (e.g. "OUT", "TX")`；`m18_probe/channel.rs` 的 `ChannelDef::signal` 文档举例 `"read"`, `"OUT"`, `"TX"`；仓库既有测试 `m18_to_m19_waveform.rs` 就是 `ChannelDef::single_bit(CH_GPIO0, "GPIO0", "OUT")` | M-15 LLD 示例代码用 `"gpio0"`；M-18 LLD 表 68 写 `signal=gpio0` |
| UART 发送 | `"TX"` | 同上（契约文档 + `channel.rs` 文档） | M-15 LLD 只写「TX/RX 状态、CTS 电平」，无具体名字 |
| 定时器比较 | `"CMP"` | 与 `"OUT"`/`"TX"` 同风格；LLD 本身三处自相矛盾（`timer_cmp` / `timer-compare` / 「比较」） | M-15 LLD 用 `"timer_cmp"`；M-18 LLD 表 68 用 `"timer-compare"` |

**信号名是常量**：M-15 侧 `gpio::SIGNAL_OUT` / `uart::SIGNAL_TX` / `timer::SIGNAL_CMP`，M-18 的通道绑定用同一常量，两边不可能漂移。若协调者决定改用 LLD 的 `timer-compare` / `gpio0`，**只改 M-15 的常量即可**，全链路自动跟随。

### 2.3 通道绑定：为什么 GPIO 是单 bit、UART/Timer 是脉冲

M-18 的通道定义（`channel.rs`）里 `source` = 外设实例 id、`signal` = 该 source 内的信号名，`(source, signal)` 正是注册表 lookup 表的键——这也是 `wire_signal_probe` 过滤信号名的依据。

- **CH_GPIO0（GPIO `"OUT"`）**：`SingleBit`。仓库既有约定（`m18_to_m19_waveform.rs` 与 M-19 侧 `ChannelMeta::single_bit("GPIO0.OUT")`）。⚠️ 注意：GPIO 上报的值是**引脚位图**（32 位），单 bit 通道的分类只看最低位——**即只有 pin 0 有升降沿语义**。多脚位场景应改用 `MultiBit` 通道（`value_width = 32`，对应 M-18 LLD 的「valueWidth = 按端口位宽」）。本 PR 的集成测试只驱动 pin 0，单 bit / 多 bit 两种绑定结论一致。
- **CH_UART_TX（UART `"TX"`，新增常量 4）**：`Pulse`。每发一个字节都是**一个事件**——连发两个相同字节是两个不同事件，多 bit 通道会把第二个当作「同值无沿」吃掉。通道表是开放表（M-18 LLD §1.3「多通道扩展」），新增第 5 个常量不影响既有 4 个。
- **CH_TIMER_CMP（Timer `"CMP"`）**：`Pulse`。比较命中是事件流；M-18 LLD 表 68 明确 ch=0 的 `valueWidth = 64`（多 bit 语义），但同样存在「连续两次命中值相同被吃掉」的问题，故取 Pulse。

### 2.4 边沿的时间戳从哪来（`Peripheral::tick`）

契约里 `Peripheral::read` / `write` **不带时间参数**，而 `ProbeSink` 要求 `(signal, value, now_ps)`。契约已冻结，无法给读写加时间参数。

本 PR 的做法：外设记住 `Peripheral::tick(now_ps)` 里收到的**权威虚拟时间**（M-12 是时钟所有者），探针上报时用它打戳。

- 优点：只用契约里已有的方法，不动契约；与 M-12 的量子循环天然对齐。
- 现状缺口：**M-12 目前没有真正调用 `Peripheral::tick`**（台账 **D-07**，Wave 2 范围，本 PR 未做）。所以在本 PR 里，外设的时钟由宿主手动 `tick()` 推进——集成测试就是这么做的。**D-07 一旦落地，戳就是精确的，无需改本 PR 的任何代码。**

### 2.5 总线读外设一律 `read_clear`（D-14）

`Peripheral::read_clear` 的默认实现委托 `read`，所以对非破坏性外设调用它是安全的；但对 UART RX 这类**读即清**的寄存器，调 `read` 就会静默复现「RX 只能收 1 字节」的原缺陷，且**没有任何编译期提示**。

本 PR 的总线读路径（对齐读 + 跨区间字节切片读）**一律走 `read_clear`**，并用一条专门的单测锁死这个约定：`peripheral_read_uses_read_clear_not_read`（mock 外设分别计数 `read` 与 `read_clear`，断言 `read` 计数恒为 0）。

### 2.6 外设错误的错误码映射

M-14 LLD §6.1：外设内部错误 → **E_BUS-05**（与未映射 E_BUS-01 可区分）。本 PR 把外设返回的 `PlatformError` 转成 `E_BUS-05`，但把**原始错误文本嵌进 message**（`peripheral read failed: E_PER-02: …`），宿主仍能看出根因。

---

## 三、端到端验证（N5 闭合的证据）

`crates/kernel/tests/m14_m15_m19_wiring.rs`（11 个用例，全部走真实跨模块路径）：

| 用例 | 验证内容 |
| --- | --- |
| `firmware_gpio_write_reaches_the_waveform_pipeline` | **N5 核心**：固件写 OUT → M-18 积累边沿 → M-19 通道出现期望边沿（值 / 种类 / 时间戳）；电平未变不产生边沿；下降沿 / 上升沿判定正确 |
| `gpio_edge_is_visible_in_the_csv_export` | 边沿在 CSV 导出里可见（`GPIO0.OUT` + 时间戳 1000） |
| `peripheral_window_size_must_match_the_peripheral_size` | **D-12**：size 不一致 → E_BUS-08，且不留下任何 region |
| `overlapping_peripheral_window_is_rejected` | 外设窗口重叠 → E_BUS-03，旧配置保留 |
| `access_past_the_peripheral_window_is_rejected` | 越界访问 → E_BUS-01 |
| `peripheral_dispatch_is_counted_in_the_bus_stats` | 外设命中计入 `periph_hits` |
| `uart_rx_read_twice_does_not_return_the_same_byte` | **D-14**：连读两次 UART RX，第二次拿不到同一个字节（第三次同理） |
| `uart_tx_byte_reaches_the_waveform_pipeline` | UART `"TX"` 信号 → 波形边沿 |
| `timer_compare_hit_reaches_the_waveform_pipeline` | Timer `"CMP"` 信号 → 波形边沿 |
| `oversized_region_is_rejected_without_allocating` | **D-10（M-14 侧）**：1 GiB 区域 → E_BUS-09，不分配 |
| `the_region_size_cap_is_configurable` | 上限可配（构造参数 + 运行期 setter） |

另有单测：M-14 侧 11 个（实例派发 / `read_clear` 路径 / 外设错误映射 / E_BUS-08 / 忙实例不 panic / id-only 旧路径 / D-10 五个）、M-15 侧 6 个（三个外设的上报语义 + 无探针时不报错 + 注册表与句柄同实例）、M-18 侧 3 个（`attach_signal` 绑定 / 幂等 / 冲突拒绝）。

---

## 四、D-10（M-14 侧）的具体做法

台账 D-10 原文：M-06 侧已由另一分支（`feature/m06-loader-hardening`）加上 `ParseLimits::total_image_bytes` / `max_region_bytes`，但 **M-14 侧仍是裸 `vec![0u8; size as usize]`**——直接调 `Bus::map_ram` 的路径仍可申请 GiB 级内存。

本 PR 的 M-14 侧：

1. `Bus` 新增 `max_region_bytes` 字段，默认 `DEFAULT_MAX_REGION_BYTES = 16 MiB`（与 M-06 侧 `max_region_bytes` 默认同一口径；**M-14 不依赖 `m06_elf`，常量在本模块内定义**，两侧各自独立生效，合并顺序无要求）。
2. 上限在**分配之前**校验：`prepare_bytes` 先查上限再 `vec![0u8; size]`（**不能先分配后检查**）。
3. `map_region` 另有一道闸——所有区域类型（含外设窗口，它同样占一段地址区间）统一过上限。
4. 可配：`Bus::with_max_region_bytes(…)` 构造 + `Bus::set_max_region_bytes(…)` 运行期调整 + `Bus::max_region_bytes()` 读回。
5. 越限 → **E_BUS-09**（`ErrorFamily::Bus`，沿用本模块既有的 scenario 编号习惯；E_BUS-08 已被 D-12 的 size 不一致占用）。

> **合并后状态**：D-10 的两侧合流，M-06 侧管「解析出来的段」，M-14 侧管「任何直接映射路径」，**两侧各自独立生效**，谁先合都行。

---

## 五、剩余欠账 / 需要协调者决策

### 5.1 需要动契约（**本 PR 未动，请协调者决策**）

| # | 诉求 | 理由 | 建议 |
| --- | --- | --- | --- |
| C1 | 契约 `Peripheral` 增加 **预检方法**（M-15 LLD N1 的 `periph_precheck`） | M-14 LLD §3.2 的跨区间写两阶段要求「外设片不可访问 → 整笔失败（E_BUS-05），不产生部分提交」。契约里没有预检接口，本 PR 只能在提交阶段逐字节调 `write`——**若外设在提交中途拒绝，RAM 片已写、外设片未写**，与 LLD 的「不部分提交」承诺有偏差 | 契约加一个带默认实现的 `fn precheck(&self, offset: u32) -> Result<(), PlatformError>`（默认 OK），M-14 预检阶段调用。**additive，不破坏现有实现** |
| C2 | 契约给外设一个**总线时刻注入**通道（如 `fn set_now_ps(&mut self, now_ps: u64)`，或让 `write` 带上时间） | 契约的 `read`/`write` 不带时间参数，总线**无法**把「这次访存的精确时刻」告诉外设；本 PR 只能用 `Peripheral::tick` 里的上一次量子时刻打戳。D-07 落地后两者相差最多一个量子 | 若要求边沿时间戳精确到访存，需要契约扩展；否则维持现状（量子级精度对波形足够） |
| C3 | 契约的 `Peripheral` 需要 `Debug` 之外的可观测手段？ | —— | **不需要**，已用 `ProbeSlot` 包装解决，未改契约 |

### 5.2 需要改他人文件（**本 PR 未动**）

| # | 诉求 | 归属 |
| --- | --- | --- |
| O1 | **D-07**：M-12 的 `dispatch` 里 `PeripheralTick` 分支仍是空 TODO，M-12 需真正调用 `Peripheral::tick(now_ps)` | `m12_engine`（另一并行会话 / Wave 2）。**一旦落地，本 PR 的边沿时间戳自动变精确，无需改本 PR 代码** |
| O2 | 契约 `port.rs` 的 `ProbeSink` 文档举例 `"OUT"`/`"TX"`，与本 PR 的信号名一致；但 M-18 LLD 表 68 写的是 `gpio0`/`timer-compare` | **文档层面对齐**：M-15 LLD / M-18 LLD 的信号名是否统一到 `"OUT"`/`"TX"`/`"CMP"`，或反过来改代码里的常量。**改常量成本 = 1 行**，本 PR 已把常量收敛到 M-15 侧 |
| O3 | `docs/reviews/CROSS_MODULE_DEBT.md`（D-10 / D-12 / D-14 状态）与 `docs/progress/PROGRESS.md`（N5 前置条件）需要协调者统一更新 | 协调者保留区 |
| O4 | `crates/kernel/src/lib.rs` / `m17_backend`：M-17 将来要用 M-14 派发外设访存，需要知道 `map_peripheral_instance` 的存在 | 协调者保留区 |

### 5.3 本 PR 自身遗留的已知限制（不阻塞，已在代码注释里标注）

1. **M-15 寄存器文件没有字节合并语义**：写 OUT 的一个字节会替换整个寄存器（`outputs = value`），而不是只改那 8 位。这是 M-15 既有的模型精度问题（不是接线引入的），但在跨区间字节切片写里会被放大（同一寄存器可能被写多次）。本 PR 未改 M-15 的寄存器语义。
2. **跨区间写触及外设时是逐字节派发**（对齐 M-14 LLD §3.2 的 phase-2 伪码），对「字对齐」固件无影响；非对齐跨区间访问的外设侧语义是近似值。
3. **id-only 的 `map_peripheral(base, size, id)` 保留为旧路径**：总线里没有实例，访问恒返 E_BUS-05。按 brief 要求保留签名（`m06_load_to_bus.rs` 依赖），新接线一律用 `map_peripheral_instance`。
4. **GPIO 单 bit 通道只看 pin 0**（见 §2.3），多脚位场景请用 `MultiBit` 通道。
5. **UART 只上报 `"TX"`，未上报 `"RX"` / CTS**：brief 只要求 TX；RX 需要另一个通道常量（可加 `CH_UART_RX = 5`），留待后续。
6. **外设窗口的 `base` 不校验与 `Peripheral::base_addr()` 一致**：总线的 base 是权威，`base_addr()` 只作参考。size 则强制一致（D-12 要求）。

---

## 六、门禁与验证记录

```
$ cargo fmt --all -- --check
（干净）

$ cargo clippy --workspace --all-targets -- -D warnings
（干净；workspace 已配置 unsafe_code = "forbid"、unused_must_use = "deny"）

$ cargo test --workspace
running 213 tests   → 213 passed; 0 failed     （内核单测，基线 193 → +20）
running 5 tests     → 5 passed; 0 failed       （m06_load_to_bus，未改动）
running 11 tests    → 11 passed; 0 failed      （m14_m15_m19_wiring，新增）
running 5 tests     → 5 passed; 0 failed       （m18_to_m19_waveform，未改动）
合计 234 passed / 0 failed（基线 203 → +31）
```

未触碰的文件（按 brief 的所有权约束）：`crates/contract/**`、`crates/kernel/src/lib.rs`、`crates/kernel/src/m12_engine/**`、`crates/kernel/src/m17_backend/**`、`crates/kernel/tests/m06_load_to_bus.rs`、`crates/kernel/tests/m18_to_m19_waveform.rs`、`docs/reviews/CROSS_MODULE_DEBT.md`、`docs/progress/PROGRESS.md`、`AGENTS.md`、`Cargo.toml`。
