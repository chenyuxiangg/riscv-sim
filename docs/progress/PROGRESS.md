# Project Progress · 项目进度

> **Document version**: v0.7.3
> **Created**: 2026-10-19
> **Last updated**: 2026-10-19
> **Status**: active
> **Owner**: zyzs
> **Source of truth**: this document, synced with git history (`git log` + `git tag`)

## v0.7.3 变更摘要

按 `docs/reviews/M-15_PeripheralModel_Review.md` §五「复查中新发现的遗留问题」落地 3 项（**N1 DRY 回归 / N2 IN 回读输出脚 / N3 去重位图放宽**）。M-15 review 现已 100% 模块内可独立闭环（跨模块欠账 #6/#11 仍按用户要求挂账不动）。

### review 落地表（M-15 §五）

| # | 严重度 | 修复 |
| --- | --- | --- |
| **N1** | 中 | 抽 `m15_peripheral/reg.rs` 共享 `register_offset` + `extract_subword` 两个 helper；`uart.rs` / `gpio.rs` / `timer.rs` 三处 `use super::reg::{...}` 引入；消除 review #5 修复引入的 6 份函数体重复。DRY 回归彻底收回 |
| **N2** | 中 | 新增 `GpioBank::pin_levels()` 私有方法，返回 `(outputs & directions) | (inputs & !directions)`；`IN` 寄存器改用 `pin_levels()` 回读。固件写 OUT 驱动 LED 后能从 IN 读到引脚电平。**N5「GPIO 翻转可见」+ M-18 波形 pipeline 都依赖这个**（review §五「二次复查」明确标注） |
| **N3** | 低 | `take_pending_irqs()` 去重位图从 `line < 16` 放宽到 `line < 32`；注释同步：「line ∈ [16, 32) previously bypassed dedup entirely, contradicting the documented unique-line contract」；line ≥ 32 的边角情况保留无去重（u32 位图兜不住） |

### 测试覆盖（M-15：25 → 29 条，+4）

- `gpio_in_reflects_output_latch_on_output_direction_pins`（N2）
- `gpio_in_reflects_input_pins_and_output_latch_simultaneously`（N2 边角：同字节混合输入/输出 pin）
- `gpio_take_pending_irqs_dedupes_lines_in_upper_half`（N3：line 17 + line 31 双向验证）
- `reg_helpers_are_dry`（N1：DRY surface 显式断言）

### 决策记录（v0.7.3）

1. **N1 DRY 集中到 `reg.rs` 而非 trait** —— 三处 helper 只用两个内联函数，没必要抽 trait；crate 内 `pub(crate)` 子模块足够。开 `mod.rs` 子模块而不是 `pub trait RegisterAccess`，避免给外设强加不必要的接口
2. **N2 IN 寄存器按 directions 选 inputs/outputs**，匹配 PL061 / STM32 GPIO 的 mental model。固件写 OUT[0]=1 后读 IN[0] 返 1；写 OUT[1]=0 后读 IN[1] 返 0；输入脚仍由 `set_external_inputs` 驱动
3. **N3 line ≥ 32 不去重** —— `u32` 位图物理上限；MVP 几乎遇不到（M-15 现状 IRQs < 32），但保留路径注释以备 P1 演进

### 进度影响

- **M-15 review §五 闭环核对**：4 项遗留 → **0 项遗留**
- **M-15 review 总闭环率**：15 完全闭环 + 1 半闭环（#6 跨 M-12）+ 1 有意保留（#18）+ 1 推迟（#11 跨 M-14/M-15）→ **15 完全闭环 + 1 半闭环 + 1 有意保留 + 1 推迟**（与 v0.7.1 一致，新发现 3 项已全部处理）
- **测试覆盖**：M-15 25 → 29；workspace lib 74 → 78（+4）
- **N2 是 N5「GPIO 翻转可见」+ M-18 波形的前置依赖**（review §五二次复查明确）；本轮把它落地

`cargo test --workspace` = **83 passed / 0 failed**（lib 78 + integration 5；v0.7.2 = 79 → v0.7.3 = +4）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.7.2 变更摘要

按 `docs/reviews/M-14_BusMemoryMap_Review.md` §四 闭环核对，落地剩余可独立处理的 6 项（#10/#11/#12/#14/#16 + 新发现 N5/N6）。**2 项跨模块挂账不修**（#6 需 M-06 联改；#9 影响 m06_load_to_bus + m02_viewcomponents，按用户要求跨模块先不动）。

### review 落地表（M-14）

| # | 原严重度 | 状态 | 修复 |
| --- | --- | --- | --- |
| 10 | 低 | ✅ 闭环 | 删 `assert_no_overlap` belt-and-braces 分支（`mod.rs:463-475`）—— `prev.contains(base)` 为真已蕴含 `prev.base + prev.size > base`，第二分支恒不可达 |
| 11 | 低 | ✅ 闭环 | `read_bytes` / `write_bytes` 改返 `Result<u32, _>` / `Result<(), _>`；越界统一抛 `E_BUS-04`，与 `read_region_byte` / `write_region_byte` 口径一致；`read_region` / `write_region` 同步透传 |
| 12 | 低 | ✅ 闭环 | `Region::contains` 与 `Bus::crosses_region` 用 `checked_add` 显式拒绝回绕：地址溢出返 `false` / `Crossing::Unmapped`，不再依赖「map_region checked_add 已守住」的偶然安全 |
| 14 | 低 | ✅ 闭环 | `byte_read_past_end_of_region_yields_e_bus_04` 改名 + 收紧断言 → `byte_read_past_end_of_region_yields_e_bus_01`，`assert_eq!(err.scenario, 1)`（Byte 路径走 `read_aligned` 的 `lookup` miss，恒返 1） |
| 16 | 低 | ✅ 闭环 | 文件末尾「Tests cover」清单更新到 v0.6.1 + 本轮新增的 hook ok / 三分 / E_BUS-05 / E_BUS-07 / Byte 越界；删除「Top-level error helpers」孤立横幅（N5） |
| 6 | 中 | ⏸ 挂账 | 跨模块，需 M-06 `ParseLimits` 联改（`total_image_bytes` / `max_region_bytes`）。本轮不动 |
| 9 | 中 | ⏸ 挂账 | 跨模块，影响 `crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js`；收紧 `Region.target` 可见性需同步改两侧取数方式。本轮不动 |

### review 落地表（M-15）

- v0.7.1 已闭环 18/19；唯一挂账 #11（跨模块 Peripheral size ↔ Bus map_peripheral size 一致性校验）延 M3 接线 PR
- 本轮无新增 M-15 修改

### 进度影响

- 测试覆盖：M-14 测试 27 条不变（修改走的是收紧断言 + Result 透传，已有覆盖守住）
- workspace lib 74 + integration 5 = **79 passed / 0 failed**
- 影响可观测数据正确性的项（review #1~#5）100% 闭环，本次收紧的是 #10~#16 整洁批次

`cargo test --workspace` = **79 passed / 0 failed**（同 v0.7.1）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

### 决策记录（v0.7.2）

1. **#11 read_bytes / write_bytes 改 Result 不会破坏现有调用路径** —— 调用方 `read_region` / `write_region` 透传 Result；`read_aligned` / `write_aligned` 已在 Result 链上；唯一变化是测试签名（已经是 `Result<u32, _>`），无需补新测试。
2. **#12 checked_add 拒绝回绕** —— `map_region` 已经用 `checked_add` 守住「base + size 不溢出 u32」，所以 `Region::contains` 的 checked_add 是防御性深度 —— 当外部代码（如未来 P1 多板互联）从 `region_tree` 之外直接传 addr 时，不会因回绕误判「contains=true」。

---

## v0.7.1 变更摘要

按 `docs/reviews/M-15_PeripheralModel_Review.md` 落地 19 项 review 意见。**3 阻断 + 4 高 + 3 中 + 9 低 + 跨模块 1（#11）**。契约层只用 additive 默认方法，不破坏 M-0 冻结。

### review 落地表

| # | 严重度 | 修复 |
| --- | --- | --- |
| 1 | **阻断** | Uart16550 加 `pub fn take_tx() -> Option<u8>`，host bridge 消费 TX 字节；`refresh_status` 同步翻 TX_READY |
| 2 | **阻断** | contract::Peripheral trait **additive** 加 `fn read_clear(&mut self, …)` 默认方法（不破坏 `read(&self, …)` 签名）；Uart16550 实现 `read_word_mut` 清 RX FIFO 并 refresh status |
| 3 | **阻断** | GPIO TOGGLE 改成 **多 pin mask toggle**：`outputs ^= value & PIN_COUNT_MASK`；固件 `1<<3` 翻 pin 3、`(1<<3)|(1<<0)` 同时翻两 pin；同步改 `tests.rs` |
| 4 | 高 | UART 新增 `status_bits::TX_OVERRUN = 0x8`；`transmit()` 溢出置 TX_OVERRUN 位（不再复用 RX_OVERRUN） |
| 5 | 高 | 三个外设的 read/write 改 `match offset & !0x3` 做寄存器选择；`extract_subword(word, offset, size)` 按 WriteSize 切字节/半字/字 |
| 6 | 高 | 删 `raise_irq()` 死函数 + `InterruptSink` import；M-12 真正调用 `tick()` 留给 M-18 PR（v0.6.1 已留 wiring PR 范围） |
| 7 | 高 | 删 `directions_complement_or_zero` 死函数 + `let _ = edges;`；`set_external_inputs` 真用 `directions`：只对配置为输入的 pin 触发边沿 IRQ |
| 8 | 中 | Timer 删 `set_mtime`；MTIME_LO/MTIME_HI 写权限改为 no-op（mtime 只读由引擎驱动）；保留 `set_mtimecmp`（固件可写 deadline） |
| 9 | 中 | `PeripheralRegistry::register()` 改返 `Result<&mut Self, RegisterError>`；`RegisterError::Duplicate(String)` 含冲突 id |
| 10 | 中 | `pub use riscv_sim_contract::port::Peripheral;` 直接转出（删除 `PeripheralTrait` 别名） |
| 11 | 中 | **跨模块留 M3 接线 PR**：Peripheral size ↔ Bus map_peripheral size 一致性校验在 wiring PR 做 |
| 12 | 低 | Uart/Timer `irq_line` 改 `pub(crate)` 字段 + `set_irq_line()`/`irq_line()` 公开 setter/getter |
| 13 | 低 | `take_pending_irqs()` 加 line bitmap 去重；多 pin 绑同一 line 只返一次 |
| 14 | 低 | UART STATUS 清 overrun 改 W1C 极性（写 1 清零）；同步改 `tests.rs:111` 旧「写 0 才清」断言 |
| 15 | 低 | `ps_per_bit()` 在 `baud_rate > 1e9` 时钳制到 1 ps（避免零长度帧） |
| 16 | 低 | status 位维护只走 `refresh_status()` 单一写入点；`transmit`/`receive` 只改 `tx`/`rx` 字段 |
| 17 | 低 | 新增 `FidelityLevel` 枚举（Behavioural / Transactional / TimingApprox / CycleAccurate）；`describe()` 走真实字段而非硬编码 |
| 18 | 低 | 删 `pub(crate) type PeripheralWrite = PendingWrite` 死别名；`force_irq_for_test` 加 `#[cfg(test)]` |
| 19 | 低 | tests 16 → 25（+9）：`take_tx` / `read_clear` / TX_OVERRUN / W1C / TOGGLE mask / 方向生效 / 同 id 注册 / 子偏移字节访问 × 3 |

### 契约层变更（additive · 不破坏 M-0）

- `riscv_sim_contract::port::Peripheral` trait 加默认方法 `fn read_clear(&mut self, offset: u32, size: WriteSize) -> Result<u32, PlatformError>` —— 默认实现透传到 `read(offset, size)`，让所有现有外设自动获得「非破坏读」语义。需要 read-clear 的外设（如 Uart16550）override 即可。零 breaking change。

### 决策记录（v0.7.1）

1. **#2 用 additive 默认方法解 #2，不开 M-0 解冻窗口** —— 与 PROGRESS §0 M-0 冻结策略一致；契约层只增不破。
2. **#3 TOGGLE 选「多 pin mask toggle」**（而非 review 推荐的「单 pin low-5-bits」）。理由：原 doc 本身有歧义（low 5 bits = pin number 与 1<<pin 写法不可同时成立），mask toggle 是最固件友好、且支持原子多 pin 翻转的语义。doc 同步改为「each bit set in `value` flips that pin's `out` atomically」。
3. **#14 UART STATUS 采用 W1C**（写 1 清零）—— 主流 PL011/16550 硬件约定；智能灯自研固件可控（W1/Q1 决议）。
4. **#8 Timer mtime 只读由引擎驱动** —— 删 `set_mtime` + MTIME_LO/HI 写权限 no-op；mtimecmp 保留固件可写（设下一 deadline）。两个时间基（总线侧 timer vs 引擎虚拟时间）语义不再混。
5. **#17 加 `FidelityLevel` 枚举** —— LLD §2.1.1 fidelity 元数据真实落地载体；MVP 全部填 `Behavioural`，P1 演进可填更高等级。
6. **#11 跨模块 size 一致性校验延 M3 接线 PR** —— 本 PR 只动 M-15 自身；M-14 ↔ M-15 接线时由 M-14 `map_peripheral(base, size, &dyn Peripheral)` 一并加 `debug_assert_eq!`。
7. **#6 删 `raise_irq` 死函数**；M-12 真正调用 `tick()` 留 M-18 PR —— 与 v0.6.1 review #13 wiring 边界同范围（peripheral-driven ticks、IRQ sink 注入）。

### 进度影响

- **§5.4 验收节点**：v0.7 声称的 N5「GPIO 翻转可见」内核前置条件真正成立 —— TOGGLE 翻转、UART TX 字节可被 host 取出、RX 可多次读清。
- **测试覆盖**：M-15 16 → 25 单测（+9）；workspace lib 65 → 74（+9）。
- **跨模块挂账**：M-12 PeripheralTick 空 TODO（m12_engine:155-159）+ M-14 ↔ M-15 接线，留给 M-18 / M3 wiring PR。

`cargo test --workspace` = **79 passed / 0 failed**（lib 74 + integration 5；v0.7 = 70 → v0.7.1 = +9）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

---

## v0.7 变更摘要

按 §6.3 推进顺序完成 **M-15 UART/GPIO/Timer**（6d 估算，本轮实际 0.5d 增量——继承 v0.6.1 已签约接口，无新协议冻结点）。MVP 三个行为级外设全部到位。

**落地的子组件**（拆 5 个文件，按子组件独立 review + 测试）：

| 子模块 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| `Uart16550` 行为级（1-byte FIFO + 8 字节寄存器窗口 + TX/RX 引脚状态 + 1e9/baud 分频） | `crates/kernel/src/m15_peripheral/uart.rs` | 5 单测 | ✅ |
| `GpioBank` 32-pin（DIR/OUT/IN/TOGGLE + 边沿触发 IRQ latch + `take_pending_irqs`） | `crates/kernel/src/m15_peripheral/gpio.rs` | 5 单测 | ✅ |
| `TimerRiscv`（64-bit mtime + mtimecmp + sticky pending IRQ via `take_pending_irq`） | `crates/kernel/src/m15_peripheral/timer.rs` | 3 单测 | ✅ |
| `PeripheralRegistry`（BTreeMap + register/get/get_mut/iter/describe + Manual `Debug`） | `crates/kernel/src/m15_peripheral/mod.rs` | 2 单测 | ✅ |
| 跨外设演示：UART TX → 桥接 → GPIO 翻转（智能灯最小子模块） | `crates/kernel/src/m15_peripheral/tests.rs` | 1 单测 | ✅ |

`cargo test --workspace` = **70 passed / 0 failed**（lib 65 + integration 5；v0.6.1 = 54 → v0.7 = 70，新增 16）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

### 决策记录（v0.7）

1. **Uart16550 MVP 选 1-byte FIFO**（评审 15-9）—— 比真实 16-byte 简单，足以覆盖 E2E-01 智能灯「字符回环」场景；E2E-01 N5 验收后可演进。
2. **GpioBank 32-pin 固定** —— MVP 不做可配置 pin 数（评审 R5，P1 演进）；`PIN_COUNT_MASK = u32::MAX` 常量化。
3. **TimerRiscv 用 `take_pending_irq()` 公开 API 替代 `InterruptSink::raise()`** —— contract `Peripheral::tick` 签名只返 `Vec<PendingWrite>`，M-16 的权威 IRQ 源上线后 Timer 降级为 no-op（评审 N3 fallback 路径）。
4. **本 PR 不动 M-14 ↔ M-15 接线** —— `RegionTarget::Peripheral(PeripheralId)` 当前仍返 `E_BUS-05 not-wired`（v0.6.1 review #13 已签约定锁）；M-14 typed handle 升级到 `Box<dyn Peripheral>` / `Rc<dyn Peripheral>` 是单独 PR 的范围，本 PR 保持 wiring 边界干净。
5. **`m15_peripheral` 拆 uart/gpio/timer/tests 子文件** —— 与 LLD §3.2-3.4 子组件对齐，方便独立子模块代码评审与回归测试；`mod.rs` 只保留公开 API + `PeripheralRegistry` + re-exports。
6. **`PeripheralRegistry` 用 `BTreeMap<String, Box<dyn Peripheral>>`** —— key 是 `Peripheral::id()` 字符串 id；MVP 单线程；`Send + Sync` 留 M-12 P2 SMP 阶段再加。

### 进度影响

- **§5.4 验收节点**：解锁 **N5 「GPIO 翻转可见」** 的内核前置条件 —— 智能灯固件可通过 M-15 GPIO 翻转驱动；待 M-18 探针 + M-19 波形 + M-07 transport 接入后可达 UI 层。
- **§6.3 下个目标**：从「M-15 UART/GPIO/Timer」迁移到「**M-18 探针**」（3d 估算）—— 解锁波形管道 + 进一步探针源挂总线/外设。
- **累计人日**：v0.6.1 = 16.5 → v0.7 ≈ 17（约 +0.5d 增量；M-15 大部分形态已在 v0.1 stub 阶段预留）。

---

## v0.6.1 变更摘要

按 `docs/reviews/M-14_BusMemoryMap_Review.md` 的 16 项意见修 M-14：M-15 接线前必做的 4 项 + 6 项整洁 + 1 项测试，共落地 11 处修复。

**修复（review 编号）**：
1. **#1** `BusAccess.width` 文档纠偏 — 「每次访问触发一次，跨区间拆分不逐字节上报」（原文档写反）。
2. **#2** hook 时机改到 dispatch 出口；新增 `BusAccess.ok: bool` + `err_scenario: Option<u16>`，失败的访问也上报 `ok=false`。消费者按 `ok` 过滤。
3. **#3** `BusStats` 拆分 `ram_hits` / `rom_hits` / `periph_hits`，ROM 写静默丢弃不再计入 `ram_hits`（评审 14-4）。`periph_hits` 在 M-15 接线时启用。
4. **#4** `periph_hits` 字段活了 — M-15 接线时即可累加。
5. **#5** `crosses_region` 改成 `Crossing::{Aligned, Crosses, Unmapped}` 三态；`unaligned_splits` 仅在两端都命中但跨区时 +1，新 `unmapped_splits` 在至少一端未映射时 +1；与原模糊统计脱钩。
6. **#7** `init.len() > size` 拆分出独立 `E_BUS-07 init-too-large`（不再共用 `E_BUS-04` 拓扑溢出）。
7. **#8** 删死函数 `validate_width`（`BusWidth` 是封闭枚举，不需要运行时校验）。
8. **#13** 补 `map_peripheral` 签约测试 — peripheral read/write 恒返 `E_BUS-05 not-wired`，等 M-15 接线后改测试即可。
9. **#14** 收紧 `read_past_end_of_region_yields_e_bus_01` 断言（去掉"or 4"宽松口径）。
10. **#15** 删 `Region::end_inclusive` 死公开 API。

**跳过（影响面大或跨模块，留后续）**：
- **#6** `max_region_bytes` 上限（需 M-06 `ParseLimits` 联改） — 影响面大，本 PR 不动。
- **#9** Region / RamBackend 字段 `pub → pub(crate)` — 影响所有 kernel crate 内部代码；不动。
- **#10/11/12** belt-and-braces 分支 / 静默吞边界 / `wrapping_add` — 安全偶然已成立，但显式化需要跨测试改动。

`cargo test --workspace` = **54 passed / 0 failed**（lib tests 49 + integration tests 5）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

## v0.5 变更摘要

**M1 数据底座第二步完成**：M-06 ELF / Raw / HEX 解析器 + M-14 / M-13 端到端集成，3 人日估算法里实际投入 ~1.5d。已落地：

| 子项 | 文件 | 测试数 | 状态 |
| --- | --- | --- | --- |
| M-06 ELF32 LE RV32 自写解析器（**MVP 决策：不引入 `object` crate**） | `crates/kernel/src/m06_elf/elf.rs` | 7 单测 | ✅ |
| M-06 Intel HEX 解析器（含 0x02/0x04 扩展地址 + 末段覆盖 06-4） | `crates/kernel/src/m06_elf/hex.rs` | 7 单测 | ✅ |
| M-06 Raw blob 解析器（统一 `BUS_SIZE` 边界） | `crates/kernel/src/m06_elf/mod.rs` | 2 单测 | ✅ |
| M-06 公开 API：`Loader` trait + `DefaultLoader` + `ParseLimits` + `Kind::{Elf,Hex,RawHex}` + `detect_kind` | `crates/kernel/src/m06_elf/mod.rs` | 2 单测 | ✅ |
| 端到端集成：M-06 → M-14 → M-13 跑通 ADDI（ELF + Raw + HEX 三格式都验） | `crates/kernel/tests/m06_load_to_bus.rs` | 5 集成测试 | ✅ |

`cargo test --workspace` = **47 passed / 0 failed**（lib tests 42 + integration tests 5）；`cargo clippy --workspace --all-targets -- -D warnings` 通过；`cargo fmt --all -- --check` 通过。

解锁里程碑 **PROGRESS.md §5.4 N2「数据底座可加载」达成**：拖 ELF / Raw / HEX 进项目 → M-06 解析 → M-14 map → M-13 fetch+execute 拿到 `x1=42`，全链路贯通。

剩余工作见 §5 阶段计划。


## v0.3 变更摘要

**MVP 范围决策**：从 v0.2 的「选项 B（~30 人日，DWARF 留 P1 降级）」**升级到「选项 C（§13.6 全量 90 人日）」**。理由：完整 13 P0 覆盖，不留降级；含 DWARF 行表 + 源码联动（MVP 风险最大项）、自动化 CI 与场景脚本断言、多板互联接口预留。

**M0 协议冻结**（5d）已 ✅ 完成；剩余 **85 人日** 待开发。详见 §5 阶段计划、§4 WBS、§8 前置条件。

## v0.2 变更摘要（保留追溯）

Q1-Q7 全部答复（见 §8）：
- Q1 智能灯固件 = **自研**（~200 行 RV32IMC 最小固件）
- Q2 ISS 指令集 = **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集）
- Q3 持久化 = **IndexedDB**（用 `idb` ~5KB 包装，非 localStorage）
- Q4 Worker URL = **同源 URL**（HTTP 部署，`./kernel-worker.js`）
- Q5 行为判定 = **canvas 比对**（虚拟 LED 渲染 + 像素 diff + 视觉对照）
- Q6 性能基线 = **macOS**（M3 Apple Silicon）
- Q7 Spike 对拍 = **必做**（FR-04，每条指令 golden）

---

## 0. TL;DR

RISC-V 仿真/调试平台项目处于 **MVP 启动期**。架构 + 23 篇模块详细设计已归档；3-crate Cargo workspace + 23 个子模块骨架已建立；M-12 事件驱动引擎和 M-13 RV32I ISS 实现了最小子集（`addi` 单指令可执行，`cargo test --workspace` 两个核心单测 PASS）。

按架构文档 §13.6 WBS 估算，**当前完成度约 13% / 90 人日**。下一步沿 §13.6 关键路径推进，目标：完成 **E2E-01 智能灯验收主线**（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）。

> ⚠️ **重要纠偏**：早期对话里把"MVP"误读为"两个单元测试通过"。重新校准后，正确定义见 §2。

---

## 1. 当前状态（截至 v0.1）

### 1.1 已完成（Done）

| 类别 | 内容 |
| --- | --- |
| 设计文档 | 27 篇（arch 1 + requirements 1 + module-design-spec 1 + detailed-design 23 + AGENTS.md + README.md） |
| 工程骨架 | 3-crate Cargo workspace：`riscv_sim_contract` / `riscv_sim_kernel` / `riscv_sim_server` |
| 契约层 | 5 个核心数据模型（`Project` / `SimState` / `DebugMeta` / `Waveform` / `Message`），`CommandPayload` / `EventPayload` 协议信封，13 个 `ErrorFamily`，6 个 port trait（`CommandBus` / `EventBus` / `ExecutionBackend` / `Peripheral` / `InterruptSink` / `Probe`） |
| 内核子模块 | 16 个 M- 模块全部建好（M-12 + M-13 实际实现，其余 14 个为公开端口/结构体形状的 stub） |
| 前端骨架 | `index.html` + `app.js`（hash 路由）+ 5 个空视图 stub（M-01/02/03/04/05/07/10） |
| 工具链脚本 | `tools/build-wasm.sh`（wasm-pack no-modules 包装） |
| 测试 | `cargo test --workspace` 通过，2 个核心单测 PASS：`m12_engine::engine_advances_time` + `m13_iss::addi_executes` |
| 版本控制 | git 初始化，main 分支，一次 initial commit（`c69e570`） |

### 1.2 部分完成（In progress）

| 模块 | 现状 | 缺口 |
| --- | --- | --- |
| **M-12 事件驱动引擎** | `Engine<B, I>` 调度器骨架，`run_until_idle` 推进 1µs 量子 → 发 `EventPayload::Tick` → 排空到期事件；`SchedEvent::{PeripheralTick, TimerExpiry, Commit}` 派发 | 没接 ISS 驱动指令；instruction budget 未生效；`PeripheralTick` / `Commit` 派发是空实现 |
| **M-13 RV32I ISS** | 手工 decoder 拆字段（opcode/rd/rs1/rs2/funct3/funct7/imm_I），Executor 实现 `ADDI`（`x[rd] = x[rs1] + imm`，`PC += 4`） | 只 1 条指令；缺 LUI / AUIPC / LOAD / STORE / BRANCH / JAL / JALR / M 扩展 / C 扩展；非法指令返 trap |
| **M-06 ELF 解析** | `Loader` trait + `DefaultLoader`（假实现：bytes 整体塞地址 0） | 没用 `object` crate，ELF header / PT_LOAD 段都没解析 |
| **M-06 ELF / Raw / HEX** | v0.5 ✅ ELF32 LE RV32 自写（无 `object` crate 依赖）+ Raw + Intel HEX（含 0x02/0x04 扩展地址）；`Loader` trait + `DefaultLoader` + `ParseLimits` + 4 个错误码 LDR-01..04（畸形 / 超限 / 越界）；E_LDR-XX 不新增 contract family，沿用 `Loader` family | 18 单测 + 5 端到端集成测试（M-06 → M-14 → M-13 三格式都跑通 ADDI） |
| **M-14 总线** | v0.4 ✅ 完整 `Bus`（BTreeMap 区间表 + 读写分发 + 跨区间两阶段 + trace hook + 统计 + 6 个错误码）；`BusMaster` trait + blanket impl；`PeripheralId` 占位 typed handle | 单元测试覆盖 map / unmap / lookup / read / write 8/16/32 / ROM 写忽略 / 跨区间 split / unmapped trap / hook / BusMaster trait 派发；M-13 ↔ M-14 集成测试 (`step_one_via_bus`) 通过 |
| **M-15 外设** | `PeripheralRegistry` + `DummyUart`（读 0 / 写忽略） | 行为级 UART（FR-07 波特率/CTS 流控）未实现；GPIO / Timer 完全空；M-14 typed handle 当前用 `PeripheralId(u32)`，待 M-15 升级到 `Rc<dyn Peripheral>` |
| **M-16 中断控制器** | `InterruptController` + `BTreeSet<u16>` 存已 raise 的 IRQ | 没 `mip/mie/mcause` CSR 联动；没接入 `step_one` 的中断采样 |
| **M-17 执行后端** | `SelfIssBackend<B, I>` 包装 Engine + Iss，实现 `ExecutionBackend` 全部 11 个方法（多为 stub） | `step_n` 永远 `Ok(0)`，没真驱动 ISS；断点 / 监视点切换是空操作 |
| **M-18 探针** | `InMemoryProbe::observe/drain`（修了上次 `last_mut` bug 和 `drain_waveform` swap hack）+ `ProbeRegistry::drain_waveform` | 没任何 signal source 挂到 bus / peripheral；M-14 trace hook 已是良接口，`set_hook(Box<...>)` 装上即可 |
| **M-19 波形** | `WaveformChannel` push/last 桩 | 没和 ProbeRegistry 联动 |
| **M-20 快照** | `SnapshotManager` 内存存 `Vec<SnapshotRecord>` | 没 M-23 持久化；没崩溃自动快照触发 |
| **M-21 日志** | `LogBuffer` 4096 环形 + `Level::{Trace..Fatal}` | 没接 `tracing` crate；没 admin 通道暴露 |
| **M-22 HMS** | `Hms::report` 单调 `Ok → Degraded → Critical` | 没 watchdog 联动；没降级动作 |
| **M-23 DataStore** | `DataStore` trait（put/get/list Project + SnapshotRecord） | 没 IndexedDB / SQLite 实现 |
| **前端** | v0.4 ✅ M-01 工作台装配 + hash 路由 + role×capBits 复校验；M-03 视图状态管理；M-02 4 视图组件（productTreeView 已渲染 M-14 内存映射 demo；debugView / waveformView / terminalView 接口形状落地）；app.js / index.html / style.css 已迁移 | 接 kernel 待 M-07 transport 接入 |

### 1.3 完全未做（Not started）

- 真实 ELF / DWARF 解析（FR-05）
- 调试服务（断点 / 单步 / Reg/Mem 视图 / 反汇编·源码联动）
- 观测管道 wire-up（探针源 → 边沿编码 → 增量推送 → 波形 UI）
- 前端多视图可见化
- postMessage 传输（M-07）+ 会话管理（M-04）+ 命令路由（M-05）
- IndexedDB 持久化
- Worker 韧性 + watchdog + 崩溃自动恢复
- Spike golden 对拍
- 智能灯 E2E-01 验收

---

## 2. MVP 范围（按 AD §11.1 + §3.4 + §13.3）

### 2.1 MVP 定义

> **MVP（P0）**：13 条 P0 功能（FR-01~12 + FR-23）· 2D 产品树/原理图 · 调试联动 + 波形 · 多视图 + 快照 · WASM 零安装
> —— 架构文档 §11.1

**部署形态**：浏览器 WASM 单文件自包含（file:// 直接打开，NFR-05）。本项目经用户决策可放宽为「可部署服务」双形态。

**装配边界**（§3.4 评审 L1）：
- ✅ 嵌入式工作台（反汇编/断点/单步、Reg/Mem、trace、波形、项目树）— MVP 全量
- ✅ 产品工作台基础面（搭建/运行/波形）— MVP 子集
- ❌ 产品行为判定 / 系统管理员工作台 — P1
- ✅ HMS MVP 基线 = 崩溃自动快照 + 恢复（§10.7 三条）
- ❌ HMS 完整分角色分发 — P1

### 2.2 验收主线（§13.3 E2E-01 智能灯）

> 智能灯固件跑通（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 判定）贯穿五横层

| 步骤 | 涉及层 | 涉及模块 | 当前可达性 |
| --- | --- | --- | --- |
| 搭建产品树 | L0/L1 前端 | M-01 / M-02 | ⚠️ M-01/M-02 视图层落地，productTreeView 渲染内存映射 demo；kernel 数据未接（M-07 待接） |
| ELF 加载 | 调试服务层 | M-06 | ⚠️ 假实现 |
| 运行 | 仿真内核层 | M-12 + M-13 + M-14 | ✅ **M-13 ↔ M-14 集成通过**：`step_one_via_bus` 跑通 ADDI，经 bus.fetch 寄存器 x1=42，PC=4 |
| GPIO 波形 | 观测管道层 | M-15 + M-18 + M-19 | ❌ 无探针源 |
| 单步/内存 | 调试服务层 | M-08 / M-09 / M-10 | ❌ stub |
| 行为判定 | L0 前端 | M-01 | ❌ |

**结论**：E2E-01 第一步 **1/6 步端到端可达**（运行）。搭建层 + 内存映射 UI 已就位，等待 M-07 transport 接入。

### 2.3 MVP 不做的事（明确边界）

- 3D 渲染 / 帧缓冲（FR-21 / FR-22）— P1
- 多板互联（FR-24~27，里程碑 B）— P1
- GDB RSP / QEMU 双内核 — P2
- 多核 SMP — P2
- DWARF 源码联动可降级（§13.6 最大风险项）：MVP 退化为「反汇编 + 符号表，无源码联动」，仍算 MVP 范围

---

## 3. 关键路径（§13.6）

> 关键路径依赖：协议冻结 → ISS → 调试服务/观测管道 → 前端；DWARF 与前端视图可并行
> —— 架构文档 §13.6

```
[已 ✅] 协议冻结          (contract crate 完成, 5 人日)
   ↓
[   1  ] 产品树/内存映射  (M-01/M-02/M-14, 4 人日)        ─┐
[   2  ] ELF 解析         (M-06, 3 人日)                    │
   ↓                                                        │ 并行可行
[   3  ] ISS 全集 RV32IMC (M-13, 12 人日)                   │ DWARF [4*]
[   4  ] 事件引擎         (M-12, 6 人日)                    │ 10 人日
[   5  ] 外设 UART/GPIO/Timer (M-15, 6 人日)                │
[   6  ] 调试服务         (M-08/M-09/M-10/M-11, 8 人日)     │
[   7  ] 观测管道         (M-18/M-19, 6 人日)              ─┘
   ↓
[   8  ] 前端视图/路由/波形 UI (M-01/M-02/M-03, 10 人日)
   ↓
[   9  ] 持久化 IndexedDB (M-23, 3 人日)
[  10  ] Worker 韧性 + watchdog + HMS (M-22, 3 人日)
   ↓
[验收]  E2E-01 智能灯 跑通
```

**已做的人日**（v0.5 估算）：
- ✅ 协议 5
- ⚠️ ISS 2.5 / 12（ADDI + step_one_via_bus，1.5d 增量）
- ⚠️ 引擎 1 / 6（调度器骨架）
- ✅ 总线 2 / 4（v0.4 完成 Bus + BusMaster + 跨区间 split；剩余 4-KiB 分页 / 重叠告警等留 P1，详见 §7）
- ✅ ELF 3 / 3（v0.5 完成）
- ⚠️ 探针 0.5 / 6（修了 drain 但无源；M-14 trace hook 已就位）
- ⚠️ 前端 3.5 / 10（v0.4 M-01/M-02/M-03 viewstore + 4 视图组件；剩余 waveform UI 渲染等后续）

**累计 ~16.5 / 90 人日 ≈ 18%**（v0.5 完成后）。

---

## 4. 完整 WBS（按 AD §13.6）

| # | 模块 | 人日 | 前置依赖 | 风险/降级 | 状态 | 备注 |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 命令/事件协议 | 5 | — | 中；先冻结 | ✅ | contract crate |
| 1 | 产品树 / 内存映射 | 4 | 协议 | 低；区间树 | ✅ | v0.4 M-01/M-02 视图层 + M-03 viewstore 完成（~2.5d）；kernel 数据待 M-07 |
| 2 | ELF / Raw / HEX 解析 | 3 | — | 中；畸形输入容量上限 | ✅ | v0.5 M-06 完整 Loader；ELF32 LE / Raw / IHEX 三格式 + 18 单测 + 5 集成测试 |
| 3 | ISS RV32IMC | 12 | WASM 工具链 | 中；Spike 对拍 | ⚠️ | M-13 只 ADDI |
| 4 | 事件驱动引擎 | 6 | ISS 接口 | 中；确定性夹具 | ⚠️ | M-12 调度器 |
| 5 | 外设 UART/GPIO/Timer | 6 | 总线接口 | 低；分级建模 | ✅ | v0.7 Uart16550(1-byte FIFO + 波特率分频) + GpioBank(32-pin 边沿 IRQ) + TimerRiscv(mtime/mtimecmp) + PeripheralRegistry；16 单测全绿；M-14 ↔ M-15 接线下一 PR |
| 6 | 调试服务 | 8 | ISS、协议 | 中；依赖 hook | ❌ | M-08/09/10 stub |
| 7 | 观测管道 | 6 | ISS 钩子 | 中；差异化核心 | ⚠️ | M-14 trace hook 接口就位，等 M-18 接 |
| 8 | DWARF 行表 + 源码联动 | 10 | ELF 解析 | **最大风险** | ❌ | 可降级 |
| 9 | 前端视图 / 路由 / 波形 UI | 10 | 协议、观测管道 | 中；零构建 | ⚠️ | v0.4 路由 + 4 视图组件；缺 waveform 渲染 |
| 10 | 持久化 IndexedDB | 3 | 数据模型 | 低 | ❌ | M-23 trait |
| 11 | Worker 韧性 + watchdog + HMS | 3 | postMessage | 中；guest 失控 | ❌ | M-22 占位 |
| 12 | 场景脚本/断言（接口预留） | 2 | 协议、确定性 | 低 | ❌ | P1 起步 |
| 13 | 多板互联（接口预留） | 2 | Peripheral port | 中 | ❌ | P1 立项 |
| **合计** | | **90** | | | | |

---

## 5. 完整开发计划（C 方案 · §13.6 全量 90 人日）

### 5.1 阶段切分

按架构 §13.6 WBS 全量开发，关键路径 **70 人日**（含 10 人日联调 buffer），并行支线 20 人日。

| 阶段 | 名称 | 人日 | 关键依赖 | 阶段交付 | 状态 |
| --- | --- | --- | --- | --- | --- |
| **M0** | 协议冻结 | 5 | — | contract crate + 5 model + 6 port + 13 错误族 | ✅ |
| **M1** | 数据底座 | 7 | M0 | 产品树视图 + 内存映射 + ELF/Raw/HEX 解析 | ⚠️ |
| **M2** | 内核 | 18 | M0 | ISS RV32IMC 全集（I+M+C） + 事件驱动引擎 + 量子化调度 | ⚠️ |
| **M3** | 外设 | 6 | M1（总线接口） | UART 行为级（波特率/CTS）+ GPIO + Timer + 注册表 | ✅ |
| **M4** | 观测 | 6 | M2（ISS 钩子） | 探针 + 边沿编码 + 波形通道 + 增量推送 | ⚠️ |
| **M5** | 调试 + 反汇编 | 18 | M2, M1（ELF） | 断点 + 单步（StepOver/Out/Line）+ Reg/Mem + 反汇编视图 + **DWARF 行表 + 源码联动** | ❌ |
| **M6** | 前端 | 10 | M0, M4 | 多视图 hash 路由 + 波形 UI + 嵌入式工作台 + 产品工作台基础面 | ⚠️ |
| **M7** | 横切 | 8 | M6 | IndexedDB 持久化 + Worker 韧性/watchdog + HMS 崩溃快照恢复 + 场景脚本/headless CLI | ❌ |
| **M8** | 多板预留 | 2 | M3 | L1 外设 port 抽象 + L3 事件队列消息槽（不绑定链路） | ❌ |
| **M9** | 联调 + CI + E2E-01 | 10 | 全部 | 智能灯端到端验收 + Spike golden 对拍 CI + macOS 性能 baseline | ❌ |
| **合计** | | **90** | | | |


### 5.2 关键路径图

```
M0 (5d) ── M1 (7d) ── M2 (18d) ── M3 (6d) ── M4 (6d) ── M6 (10d) ── M7 (8d) ── M9 (10d)
                  │                   │            │             │          │          │
                  └── M5/DWARF(10d) ───┘            │             │          │          │
                                                  M5/调试(8d) ── ┘          M8(2d) ───┘
```

### 5.3 13 P0 覆盖矩阵（C 方案 = 100%）

| FR | 内容 | 责任模块 | 阶段 |
| --- | --- | --- | --- |
| FR-01 | 图形化搭建（多节点、多外设） | M-01 / M-02 / M-14 | M1 + M6 |
| FR-02 | 元器件事例化、地址映射配置 | M-01 / M-02 / M-14 / M-15 | M1 + M3 |
| FR-03 | 外设地址映射与查看/修改 | M-14 / M-15 + M-10 UI | M3 + M6 |
| FR-04 | RV32I/IM、伪指令与别名 | M-13 + tests/golden/ | M2 + M9 |
| FR-05 | 反汇编·源码联动 | M-11 反汇编 + M-06 DWARF 行表 | M5 |
| FR-06 | 异常/中断、向量表配置 | M-12 量子化 + M-16 中断控制器 | M2 + M3 |
| FR-07 | UART 波特率/CTS 流控 | M-15 行为级 UART | M3 |
| FR-08 | 断点/单步 | M-08 断点 + M-09 单步引擎 + M-10 UI | M5 + M6 |
| FR-09 | 寄存器/内存查看修改 | M-10 UI + ExecutionBackend 接口 | M5 + M6 |
| FR-10 | GPIO 波形/逻辑分析 | M-15 GPIO + M-18 探针 + M-19 波形 | M3 + M4 + M6 |
| FR-11 | 变量监视、手动读写（地址型） | M-08 watchpoint + M-10 watch UI | M5 + M6 |
| FR-12 | 确定性、可复现 | M-12 量子化 + M-20 快照 + M-23 落盘 | M2 + M7 |
| FR-23 | 多视图独立路由 | M-01 视图装配 + M-03 view store | M6 |

### 5.4 验收节点（可演示里程碑）

| 节点 | 阶段 | 验收标准 |
| --- | --- | --- |
| **N1** 协议冻结 | M0 | contract crate 编译 + DTO roundtrip 序列化测试通过 |
| **N2** 数据底座可加载 | M1 | 拖 ELF 进项目 → 看到 product tree + 内存映射表 + ELF section 列表 |
| **N3** 单指令可执行 | M2 | `addi x1, x0, 42` 跑完能读 x1=42（已有 `addi_executes` 测试） |
| **N4** 全 RV32IMC 跑通 | M2 | Spike golden 对拍通过全部 71 条指令 |
| **N5** GPIO 翻转可见 | M3 + M4 | 智能灯固件跑起来，前端波形图看到 LED 引脚翻转 |
| **N6** 调试联动 | M5 + M6 | 断点停 → 看 Reg/Mem → 单步（StepOver/Out） → 反汇编 + 源码联动 |
| **N7** E2E-01 智能灯验收 | M9 | 智能灯 6 步全绿（搭建 → 加载 → 运行 → 波形 → 单步 → 内存 → 行为判定） |
| **N8** MVP 完整交付 | M9 | 13 P0 全覆盖 + 性能 baseline ≥5 MIPS + 场景脚本可重放 |

### 5.5 第一刀建议（M1 数据底座，7 人日）

按关键路径顺序，**M0 已完成**。下一阶段 M1 建议分两步：

1. **M-14 总线 + M-01/02 产品树视图**（4 人日）— 先有数据底座，UI 后接
2. **M-06 ELF/Raw/HEX 解析**（3 人日）— 引入 `object` crate（BSD-2-Clause），解 PT_LOAD 段写进 bus；DWARF 留到 M5

不走「先扩 ISS」理由：M-13 扩到 RV32IMC 全集（12d）跟 M-14 弱依赖，可并行走；M-14 是后续所有「加载 → 跑」的物理基础，先打地基更稳。

### 5.6 E2E-01 智能灯固件（自研 · Q1 决议）

按 Q1 决定，自研约 200 行 RV32IMC 最小固件，1 个 GPIO 输入（按钮）+ 1 个 GPIO 输出（LED）+ main 循环（轮询按钮 → 翻转 LED）。工具链：`riscv64-unknown-elf-gcc` 编译为静态 ELF（无 libc，bare-metal）。

验收：在 M9 用 Spike 跑同一固件 + 在我们内核跑同一固件，比较 LED 引脚时序一致。

---

## 6. 模块依赖关系（按模块推进原则）

> 本节是 v0.6 引入的工作准则：**按模块独立完成（不按功能）**，每个模块收口经
> `cargo test --workspace` + `clippy -D warnings` + `fmt --check` 三关。
> 依赖关系决定下一个模块的选择，E2E-01 验收节点决定推进顺序。

### 6.1 依赖表

**内核 crate（`riscv_sim_kernel`）**

| 模块 | 依赖 | 状态 | 备注 |
| --- | --- | --- | --- |
| M-14 总线 | contract | ✅ v0.4 | 叶子；所有访存的物理基础 |
| M-06 ELF/Raw/HEX | contract + M-14（写路径） | ✅ v0.5 | 叶子；M-06 → M-14 端到端集成已通 |
| M-21 日志 | contract | ❌ | 最叶子（基础设施） |
| M-15 UART/GPIO/Timer | contract + M-14 | ✅ v0.7 | Uart16550 + GpioBank(32-pin) + TimerRiscv + PeripheralRegistry；16 单测全绿；接线下一 PR |
| M-13 ISS RV32IMC | contract + M-14 | ⚠️ (ADDI only) | M-14 已就绪，可扩 71 条 |
| M-11 DWARF/反汇编/trace | contract + M-06 | ❌ | M-06 ✅；DWARF 行表 06-7 |
| M-18 探针 | contract + M-14 + M-15 | ❌ | **下一目标**；等 M-15 ✅ |
| M-19 波形 | M-18 + contract | ❌ | 等 M-18 |
| M-16 中断控制器 | M-14 + M-15 | ❌ | 等 M-15 |
| M-12 事件驱动引擎 | M-13 + M-14 + M-16 | ⚠️ (骨架) | 等 M-13 + M-16 |
| M-17 执行后端 | M-12 + M-13 + M-14 | ⚠️ (骨架) | 等 M-12 + M-13 |
| M-08 断点 | M-17 + M-13 | ❌ | 等 M-17 |
| M-09 单步 | M-17 + M-13 | ❌ | 等 M-17 |
| M-20 快照 | M-12 + M-13 | ❌ | 等 M-12 + M-13 |
| M-23 DataStore | M-20 + contract | ❌ | 等 M-20 |
| M-22 HMS | M-21 + M-17 + M-12 | ❌ | 等 M-17 + M-21 |

**前端（`front/`）**

| 模块 | 依赖 | 状态 | 备注 |
| --- | --- | --- | --- |
| M-01 工作台装配 | (DOM 本身) | ✅ v0.4 | 叶子 |
| M-02 视图组件 | M-01 + M-03 + 数据通道 | ✅ v0.4 | 4 视图接口形状已落地 |
| M-03 View Store | (前端本身) | ✅ v0.4 | 叶子 |
| M-07 transport | contract `Message` | ❌ | 叶子；绑定 kernel WASM |
| M-04 会话管理 | M-07 | ❌ | 等 M-07 |
| M-05 命令路由 | M-04 + M-07 | ❌ | 等 M-04 + M-07 |
| M-10 registermemory 服务 | M-17 后端 + M-07 | ❌ | 等 M-17 + M-07 |

### 6.2 推进顺序（按依赖叶子 + E2E-01 主线）

```
                                    [已完成 ✅]
   M-14 总线 ──────────────┬────── M-06 ELF/Raw/HEX
       │                  │
       ├──[可立即]── M-15 UART/GPIO/Timer ── M-18 探针 ── M-19 波形
       │                  │
       │                M-16 中断控制器
       │                  │
       ├──── M-13 ISS 全集扩（I 47 + M 8 + C 16, 71 条）
       │                  │
       ├──── M-12 事件引擎扩（指令预算 / 量子化 / peripheral tick）
       │                  │
       ├──── M-17 ExecutionBackend（SelfIssBackend）
       │                  │
       ├──── M-08 断点  ──┴── M-09 单步
       │
       └── M-20 快照 ── M-23 DataStore ── M-22 HMS ── M-21 日志（同步）

   [前端顺序]
   M-01/02/03 ── M-07 transport ── M-05 命令路由 ── M-04 会话 ── M-10 服务
```

### 6.3 下个目标

按依赖图 + E2E-01 §5.4 N5「GPIO 翻转可见」优先级（**M-15 已 ✅ v0.7**，下一 PR 也属 §6.3 范围）：

**M-18 探针**（3 人日）

子组件拆分（方便测试 + 评审）：
- `m18_probe::Probe` trait 扩展（contract 既有 `observe/drain` 接口）
- `m18_probe::InMemoryProbe` 升级（已有 stub 修 `drain_waveform` swap hack）
- `m18_probe::ProbeRegistry` 装到 `Bus` 的 trace hook + `Peripheral::tick` 出口
- 信号源：总线读写（已有 hook 接口）+ GPIO OUT 翻转 + UART TX pin 状态 + Timer mtime 采样
- 边沿编码（rising/falling/toggle） + 通道标签（address / pin / signal）

验收：
- `cargo test -p riscv_sim_kernel m18_probe` ≥ 8 单测（总线读写 / GPIO 翻转 / UART pin / Timer 周期各 ≥1；边沿编码 + drain 边界）
- `cargo clippy --workspace --all-targets -- -D warnings` 通过
- `cargo fmt --all -- --check` 通过
- 演示用例：写 GPIO OUT 引脚翻转 → probe drain 拿到 rising + falling 两条事件

预计 v0.8 落地。解锁 M-19 波形管道（§6.2 顺序）。

### 6.4 风险与降级（按 AD §12）

| ID | 风险 | MVP 应对 | 降级方案 |
| --- | --- | --- | --- |
| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | **v0.3 决议：纳入 MVP（M5 阶段，10 人日）** | 失败时回退到「反汇编 + 符号表」，但 MVP 仍按完整 DWARF 排期 |
| R8 | 确定性范围 | 种子 + 时间戳化输入事件序列（AD §8.1 D0） | — |
| R10 | 多板互联复杂度 | MVP 单板不阻塞；接口预留 | — |
| R12 | 性能 5-10 MIPS 估算基线 | 解释执行 MVP | 演进解锁 JIT/QEMU |
| L1 | 装配边界 | §3.4 评审 L1 子集 | — |
| L3 | 客户端输入威胁 | 客户端低；服务端放大时强制 | — |

---

| R2 | GPL 合规（QEMU 传染） | 自研 BSD/MIT 内核；QEMU 走进程隔离（P2） | — |
| R3 | Worker 韧性 / guest 失控 | MVP 必做：watchdog + terminate | 主线程分片兜底（无 Worker 环境） |
| R5/N2 | 快照 RPO ≤ 2s | 每 10s 虚拟时间或关键事件触发落盘 | 缩短触发间隔（资源代价） |
| R7 | DWARF 解析工作量大 | **v0.3 决议：纳入 MVP（M5 阶段，10 人日）** | 失败时回退到「反汇编 + 符号表」，但 MVP 仍按完整 DWARF 排期 |
| R8 | 确定性范围 | 种子 + 时间戳化输入事件序列（AD §8.1 D0） | — |
| R10 | 多板互联复杂度 | MVP 单板不阻塞；接口预留 | — |
| R12 | 性能 5-10 MIPS 估算基线 | 解释执行 MVP | 演进解锁 JIT/QEMU |
| L1 | 装配边界 | §3.4 评审 L1 子集 | — |
| L3 | 客户端输入威胁 | 客户端低；服务端放大时强制 | — |

---

## 7. 决策记录（按时间倒序）

| 日期 | 决策 | 依据 |
| --- | --- | --- |
| 2026-10-19 | **M-14 区间管理用 `BTreeMap<u32, Region>`**（标准库红黑树）而非手写 AVL 增强 BST | MVP 区间数 ≤ 8，BTreeMap O(log n) + 简单正确；手写 AVL 不增加明显性能但增加错误面 |
| 2026-10-19 | **M-14 Peripheral typed handle 用 `PeripheralId(u32)` 占位** | M-15 当前只有 DummyUart，没必要立即引入 `Rc<dyn Peripheral>` 生命周期；评审 14-11 留待 M-15 真实现 UART/GPIO 时升级 |
| 2026-10-19 | **M-14 写跨区间两阶段：阶段一预检全部 byte slice → 阶段二统一执行** | LLD §3.2 评审 14-1 原子性要求：任一未映射 → 整笔失败，不允许 RAM 已写 + 异常上报的部分成功状态 |
| 2026-10-19 | **M-14 trace hook 类型 `Box<dyn FnMut(&BusAccess)>`（无 Send bound）** | MVP 单线程模拟；Send bound 留 M-12 P2 SMP 阶段再加 |
| 2026-10-19 | **M-13 接入 M-14 加 `step_one_via_bus`，保留 `step_one` 函数指针老接口** | 双通道：函数指针接口零依赖用于纯解释器单测；`&mut Bus` 接口用于真实总线分发 |
| 2026-10-19 | **M-01 capBits MVP 内置 role 默认集合**；M-05 negotiate 接入后改从协议取 | M-05 还没实现；URL `?role=product|embedded` 切换角色作为 MVP 临时入口 |
| 2026-10-19 | **`render → ` CustomEvent(`riscv:render`) 派发**，M-02 监听 | M-01 switchView 返回值是个内部接口；通过 CustomEvent 让任意视图组件都可挂载 |
| 2026-10-19 | workspace `missing_docs = "allow"` + `derivable_impls = "allow"` | M-0 冻结后尚未补全公开字段 doc；不为此重新 churn 协议代码；M-0 后续收敛时切回 deny |
| 2026-10-19 | m12_engine `instruction_budget` 字段加 `#[allow(dead_code)]`；QueuedEvent 翻转 Ord 加 `non_canonical_partial_ord_impl = "allow"` | M-12 调度骨架尚未接 ISS 指令预算；BinaryHeap min-heap trick 需要非规范 Ord。等 M-12 完成 ISS 接通后回收 |
| 2026-10-19 | self.best_effort **第一步落入约 2.5 人日**（M-14 + M-01/02/03 + M-13 接入） | 比 §5.5 估计的 4 人日提前 1.5d，部分归功于 BTreeMap 取代手写 AVL |
| 2026-10-19 | 自研 ISS 用 **Rust**（非 C） | 用户明确要求 |
| 2026-10-19 | MVP 部署形态可**目录形式**（不必单文件自包含） | 用户放宽 NFR-05 |
| 2026-10-19 | 骨架策略：先做 stub | 用户 |
| 2026-10-19 | 实施起点：先跑通 MVP | 用户（**误读**为「M-12 + M-13 单测通过」非「E2E-01 跑通」） |
| 2026-10-19 | MVP 范围以 **AD §13.6** 为准（13 P0 + 90 人日） | 重读架构文档后校准 |
| 2026-10-19 | `contract::WatchAccess` 三个变体补 doc；其他 variant 字段 doc 暂以 `#![warn(missing_docs)]` 跳过 | Rust 1.99 严格度 |
| 2026-10-19 | `m18_probe::drain_waveform` 重写为 `values_mut()` + `Probe::drain()`，替换原 `mem::swap` hack | cargo test 失败 |
| 2026-10-19 | `m12_engine::run_until_idle` 改为「每量子推进 → 排空到期」循环 | 原语义让 `engine_advances_time` 测试只走 1 量子 |
| 2026-10-19 | `m17_backend::SelfIssBackend` bound 从 `BusMaster` 改为 `EventBus` | `Engine` 方法要 EventBus bound |
| 2026-10-19 | `m15_peripheral::PeripheralRegistry` 拿掉 `derive(Debug)`，手写只输出 count | `Box<dyn Peripheral>` 不 Debug |
| 2026-10-19 | **Q1** 智能灯固件 = **自研**（不外取） | 用户答复 |
| 2026-10-19 | **Q2** ISS = **RV32IMC 全集**（I+M+C，含 16-bit 压缩指令） | 用户答复 |
| 2026-10-19 | **Q3** 持久化 = **IndexedDB**（用 `idb` 包装，BSD-3，~5KB） | 用户答复（接受 +0.3-0.5 人日换容量） |
| 2026-10-19 | **Q4** Worker URL = **同源**（HTTP 部署） | 用户答复 |
| 2026-10-19 | **Q5** 行为判定 = **canvas 比对**（虚拟 LED + 像素 diff） | 用户答复 |
| 2026-10-19 | **Q6** 性能基线硬件 = **macOS / Apple Silicon** | 用户答复 |
| 2026-10-19 | **Q7** Spike golden 对拍 = **必做**（FR-04） | 用户答复 |
| 2026-10-19 | **MVP 范围** = **选项 C**：§13.6 全量 90 人日（含 DWARF、CI、场景脚本、多板接口）；不降级 | 用户决议（v0.2 提议 B/C 后选 C） |
| 2026-10-19 | **M-06 ELF 解析自写而非引入 `object` crate** | M-06 §3.1 MVP 范围窄（ELF32 LE RV32 ET_EXEC），自写 ~150 行覆盖所有错误码；`object` 0.36 体积偏大；P1 阶段如需 ELF64 / non-RV32 ELF 再引入 |
| 2026-10-19 | **M-06 错误码使用 `ErrorFamily::Loader`（contract 既有）+ scenarios 01..04** | 设计文档用 E_PROTO-NN（既有），contract 没 PROTO family；用 Loader family + additive scenarios（PROGRESS.md §7 决策）最不破坏 M-0 冻结 |
| 2026-10-19 | **M-06 ELF bss gap（p_memsz > p_filesz）切为独立 segment** | 避免重复 PT_LOAD；M-14 `map_ram(base, size, &[])` 自然全 0 初始化，对齐评审 14-5 |
| 2026-10-19 | **M-06 HEX 解析「连续合并 + 同基地址后行覆盖 + 重叠段不分摊」三规则** | 06-4：连续地址合并节省 M-14 segment slots；同 base 后行覆盖避免 data 长度不一的 snapshot 问题；重叠段留给 M-14 E_BUS-03 错误码上抛 |
| 2026-10-19 | **M-06 集成测试放在 `crates/kernel/tests/*.rs`**（Cargo 默认 integration test 路径） | AGENTS.md 写的 `tests/unit/` 在顶层没 crate，cargo 不会发现；改为 crates/kernel/tests 是 Rust 标准 |
| 2026-10-19 | **v0.6 决策：按模块独立完成（不按功能）** | 23 模块按依赖关系 + E2E-01 主线，每个模块收口经 cargo test / clippy / fmt 三关，独立子模块；方便测试与代码评审。先做依赖叶子（依赖最少），再做核心，最后做派生 |
| 2026-10-19 | **M-15 Uart16550 1-byte FIFO**（评审 15-9） | MVP 智能灯「字符回环」场景够用；E2E-01 N5 验收后可演进到 16-byte 真实硬件 FIFO |
| 2026-10-19 | **M-15 GpioBank 32-pin 固定**（评审 R5） | `PIN_COUNT_MASK = u32::MAX`；MVP 不做可配置 pin 数，P1 演进 |
| 2026-10-19 | **M-15 TimerRiscv 用 `take_pending_irq()` 公开 API 替代 `InterruptSink::raise()`** | contract `Peripheral::tick` 签名只返 `Vec<PendingWrite>`，无法直接调 IRQ sink；M-16 权威 IRQ 源上线后 Timer 降级为 no-op（评审 N3 fallback） |
| 2026-10-19 | **M-15 ↔ M-14 接线单独 PR** | 本 PR 只完成 M-15 行为级外设本体；M-14 typed handle 从 `PeripheralId(u32)` 占位升级到 `Box<dyn Peripheral>` / `Rc<dyn Peripheral>` 委托，留给 wiring PR（评审 v0.6.1 #13 已签约定锁） |
| 2026-10-19 | **M-15 拆 uart/gpio/timer/tests 5 个子文件** | 与 LLD §3.2-3.4 子组件对齐；`mod.rs` 只保留公开 API + `PeripheralRegistry` + re-exports；按子组件独立 review + 测试 |
| 2026-10-19 | **M-15 `PeripheralRegistry` 用 `BTreeMap<String, Box<dyn Peripheral>>`** | key = `Peripheral::id()`；MVP 单线程；`Send + Sync` 留 M-12 P2 SMP 阶段再加 |
| 2026-10-19 | **M-15 review #2 修复用 additive 默认方法 `read_clear`**，不开 M-0 解冻窗口 | contract::Peripheral trait 加 `fn read_clear(&mut self, …) -> Result<u32, _>` 默认透传到 `read`；Uart16550 override 清 RX FIFO；零 breaking change |
| 2026-10-19 | **M-15 review #3 GPIO TOGGLE 改「多 pin mask toggle」** | `outputs ^= value & PIN_COUNT_MASK`；固件 `1<<pin` 翻对应 pin；多 pin mask 原子翻转；doc 同步改为「each bit set in `value` flips that pin's `out` atomically」 |
| 2026-10-19 | **M-15 review #14 UART STATUS 采用 W1C 极性**（写 1 清零） | 主流 PL011/16550 硬件约定；同步改 tests.rs |
| 2026-10-19 | **M-15 review #8 Timer mtime 只读由引擎驱动** | 删 `set_mtime`；MTIME_LO/HI 写权限改为 no-op；mtimecmp 保留固件可写（设 deadline）；两个时间基语义不再混 |
| 2026-10-19 | **M-15 review #17 加 `FidelityLevel` 枚举** | LLD §2.1.1 fidelity 元数据真实载体；MVP 全部 Behavioural；P1 演进可填更高等级 |
| 2026-10-19 | **M-15 review #11 跨模块 size 一致性校验延 M3 接线 PR** | M-14 `map_peripheral(base, size, &dyn Peripheral)` 加 `debug_assert_eq!(p.size(), region_size)` |
| 2026-10-19 | **M-14 review #10/#11/#12/#14/#16/N5 收紧闭环** | 删 belt-and-braces 死分支；`read_bytes`/`write_bytes` 改返 `Result` 与 `read_region_byte` 一致；`contains`/`crosses_region` 用 `checked_add` 显式拒绝地址回绕；Byte 越界测试收紧为 `E_BUS-01`；更新文件末尾「Tests cover」清单 + 删孤立横幅。**跨模块 #6/#9 挂账不动** |
| 2026-10-19 | **M-14 review #6 跨模块挂账**：分配上限 | `Bus::prepare_bytes` 仍是裸 `vec![0u8; size as usize]`；需与 M-06 `ParseLimits` 联合改动加 `total_image_bytes` / `max_region_bytes`；畸形固件可在 WASM 内 OOM 触发 worker 崩溃 |
| 2026-10-19 | **M-15 review §五 N1/N2/N3 闭环** | N1：抽 `m15_peripheral/reg.rs` 共享 `register_offset`/`extract_subword`，消除 review #5 引入的 6 份重复；N2：GPIO `IN` 寄存器回读输出脚（`pin_levels()` = outputs & directions \| inputs & !directions），解锁 N5「GPIO 翻转可见」+ M-18 波形；N3：去重位图 `line < 16` 放宽到 `line < 32`，与文档「unique IRQ line per pin」承诺一致 |
| 2026-10-19 | **M-15 review §五 N2 优先级最高** | review §五「二次复查」明确标注：固件通过 OUT 驱动 LED 后无法从 IN 读回引脚电平，M-18 波形与 N5 验收都依赖此。已落地 |

---

## 8. 已澄清问题（v0.2 全部答复）

| 编号 | 问题 | 答复 | 对实施的影响 |
| --- | --- | --- | --- |
| Q1 | 智能灯固件来源 | **自研** ~200 行 RV32IMC 最小固件（按钮 GPIO 输入 + LED GPIO 输出 + 简单延时循环） | M-15 必须有 GPIO 输入 + 输出；M-11 反汇编要看得懂它；M-06 ELF 解析要能装它 |
| Q2 | ISS 指令集范围 | **RV32IMC 全集**（I 47 + M 8 + C 16，无压缩子集） | M-13 估 **12 人日**；C 扩展需 PC 步长 2/4 切换 + 16-bit 解码表 |
| Q3 | 持久化后端 | **IndexedDB**（用 `idb` 包装，~5KB BSD-3 库） | M-23 估 **3.5 人日**（含 `onupgradeneeded` schema 声明）；架构 §7.4 容量模型原样适用；M-05 CommandRouter 须 async 化 |
| Q4 | Worker URL | **同源 URL**（`./kernel-worker.js`，HTTP 部署） | M-07 简化（无 blob URL 内联代码）；MVP 假定 `python3 -m http.server 8080 --directory front` 或等价 HTTP 部署 |
| Q5 | 行为判定 | **canvas 比对**（虚拟 LED 渲染到 canvas + 像素 diff + 视觉对照） | M-01 加一个产品行为视图（虚拟 LED 灯泡 + 状态机动画）；可选自动化：每 N ms 截 canvas vs 期望图 |
| Q6 | 性能基线硬件 | **macOS**（Apple Silicon M3 / 评测机） | 性能验收在该硬件上实测 ≥5 MIPS 估算基线；CI runner 也是 macOS |
| Q7 | Spike golden 对拍 | **必做**（FR-04，每条已实现指令对拍） | `tests/golden/` 用 Spike 作为 oracle 跑同一程序，对比 `m13_iss` 的 register / memory 输出；CI 必跑；MVP 起步可只对 ADDI + 关键 LOAD/STORE/BRANCH |

### v0.2 锁定后的实施前置条件

- **依赖**：需要安装 `riscv64-unknown-elf-gcc` 工具链（编译自研固件） + `spike`（golden oracle），以及 npm 上没有依赖所以前端 `idb` 走 ESM CDN 或本地拷贝
- **环境变量**：可能需要 `SPIKE_BIN=/path/to/spike` 给 golden test 用
- **CI**：GitHub Actions macOS runner + Spike 安装步骤

---

## 9. 文档版本历史

| 版本 | 日期 | 变更 | 作者 |
| --- | --- | --- | --- |
| v0.1 | 2026-10-19 | 初稿：建立项目状态、§13.6 WBS 校准、关键路径、下一里程碑 E2E-01、决策与风险 | agent (mavis) |
| v0.2 | 2026-10-19 | MVP 范围从「选项 B（~30 人日）」升级到「选项 C（90 人日）」；补 Q1-Q7 已答复，DWARF/CI/场景脚本/多板接口预留纳入 MVP | agent (mavis) |
| v0.3 | 2026-10-19 | 嵌入 v0.2 决策表；WBS 同步；E2E-01 验收标准定锚 | agent (mavis) |
| v0.4 | 2026-10-19 | M1 数据底座第一步完成：M-14 总线（20 单测）+ M-13 ↔ M-14 集成（+2 单测）+ M-01/M-02/M-03 前端装配 + 4 视图组件；WBS 行 #1 #7 #9 状态推进；累计人日 10.5 → 15；E2E-01 第 1 步可达 | agent (mavis) |
| v0.5 | 2026-10-19 | M1 数据底座第二步完成：M-06 ELF / Raw / HEX 解析（18 单测）+ M-06 → M-14 → M-13 端到端集成（5 测试，三格式全跑通 ADDI）；ELF32 LE 自写不加 `object` 依赖；累计人日 15 → 16.5；**§5.4 N2 「数据底座可加载」达成** | agent (mavis) |
| v0.6 | 2026-10-19 | 策略调整：按模块独立完成（不按功能），梳理 23 模块依赖关系表（§6.1）+ 推进顺序图（§6.2）+ 下一目标 **M-15 UART/GPIO/Timer（6d）**；未实施新代码 | agent (mavis) |
| v0.6.1 | 2026-10-19 | 按 `docs/reviews/M-14_BusMemoryMap_Review.md` 修 M-14：落实 #1-#5 + #7-#8 + #13-#15 共 11 处（hook 时机 / stats 分派 / 三态 Crossing / E_BUS-07 / BusAccess.ok）；test 20 → 27；累计 lib 49 + integration 5 = **54 全绿**；跨 PR 不动 #6 / #9 / #10-12 | agent (mavis) |
| v0.7 | 2026-10-19 | **M-15 UART/GPIO/Timer 完成**：Uart16550（1-byte FIFO + 波特率分频）+ GpioBank（32-pin 边沿 IRQ latch）+ TimerRiscv（mtime/mtimecmp sticky pending IRQ）+ PeripheralRegistry；拆 uart/gpio/timer/tests 5 子文件；16 单测全过；lib 49 → 65（+16），workspace 累计 **70 全绿**；WBS #5 ✅；阶段 M3 ✅；§6.3 下一目标切到 M-18 探针 | agent (mavis) |
| v0.7.1 | 2026-10-19 | 按 `docs/reviews/M-15_PeripheralModel_Review.md` 落地 19 项：**3 阻断 + 4 高 + 3 中 + 9 低 + 1 跨模块（#11 延 M3）**。契约层只用 additive 默认方法 `Peripheral::read_clear`（不开 M-0 解冻窗口）；UART 加 `take_tx()`、TX_OVERRUN 独立位、STATUS W1C；GPIO TOGGLE 改多 pin mask toggle、方向生效、pending 去重；Timer mtime 只读由引擎驱动；加 `FidelityLevel` 枚举；tests 16 → 25（+9）；lib 65 → 74（+9）；workspace 累计 **79 全绿** | agent (mavis) |
| v0.7.2 | 2026-10-19 | 按 `docs/reviews/M-14_BusMemoryMap_Review.md` §四 闭环核对，落地剩余 6 项可独立处理（#10 belt-and-braces 删 / #11 read_bytes-write_bytes 改 Result / #12 wrapping_add 改 checked_add / #14 Byte 越界测试收紧 / #16 Tests cover 清单更新 + N5 删孤立横幅）；**2 项跨模块挂账**（#6 需 M-06 联改分配上限 / #9 影响 m06_load_to_bus + m02_viewcomponents 收紧 Region 可见性），按用户要求先不动。M-15 review v0.7.1 后无新增修改。workspace 累计 **79 全绿** | agent (mavis) |
| v0.7.3 | 2026-10-19 | 按 `docs/reviews/M-15_PeripheralModel_Review.md` §五 复查遗留 3 项：**N1** 抽 `reg.rs` 共享 helper 消除 review #5 引入的 6 份重复（DRY 回归）；**N2** GPIO `IN` 寄存器回读输出脚（`pin_levels()` = outputs & directions \| inputs & !directions），解锁 N5「GPIO 翻转可见」+ M-18 波形；**N3** 去重位图 `line < 16` 放宽到 `line < 32` 同步注释。tests 25 → 29（+4）；workspace 累计 **83 全绿** | agent (mavis) |
| v0.2 | 2026-10-19 | 答复 Q1-Q7：自研固件 / RV32IMC 全集 / IndexedDB / 同源 Worker / canvas 比对 / macOS / Spike 必做；M-13 工作量上调至 12 人日；§5 step 2 同步更新；新增 §8 「已澄清问题」表与实施前置条件 | agent (mavis) |
| v0.3 | 2026-10-19 | MVP 范围从 B（30d, DWARF 留 P1）升级到 **C（§13.6 全量 90 人日）**；§5 重写为 10 阶段计划（M0-M9）+ 13 P0 覆盖矩阵 + 8 个验收节点 + 关键路径图；§6 R7 改「纳入 MVP」；M0 已完成，剩余 85 人日 | agent (mavis) |

---

## 10. 设计文档引用

- **AD §11.1** 演进路线（里程碑时间轴）— MVP 范围定义
- **AD §13.6** WBS + 关键路径 + 人日估算
- **AD §13.3** 智能灯 E2E-01 验收主线
- **AD §3.4** 角色驱动工作台装配 + §3.4 评审 L1 最小化边界
- **AD §5.2** 量子化混合调度（1µs 量子）
- **AD §8.1** 确定性 D0
- **AD §8.4** 安全沙箱
- **AD §12** 风险清单 R1~R12
- **需求 FR-01~12 + FR-23** 13 条 P0
- **需求 §3.2** 智能灯 E2E-01 业务主线
- **LLD §2.5** 契约层定义（contract crate 唯一来源）
- **M-12 / M-13 / M-14 / M-06 / M-15 / M-18 / M-19 / M-08** 模块详细设计

---

## 11. 仓库与命令

```bash
# 验证
source $HOME/.cargo/env
cd /Users/zyzs/lczyzs/code/riscv_sim
cargo test --workspace          # 2 个 MVP 单测 PASS

# 提交
git status                      # 改动一览
git add -p                      # 交互式暂存
git commit -m "feat(m12): ..."  # conventional commits

# WASM 构建
tools/build-wasm.sh             # 输出 front/assets/pkg/

# 前端启动
python3 -m http.server 8080 --directory front
```

---

## 12. 不在本文档范围

- 详细 API 文档（各模块 `///` doc comment + 详细设计文档）
- Spike golden 对拍脚本（`tests/golden/`）
- 端到端测试夹具（智能灯固件）
- CI 配置（`.github/workflows/` 或等价）
- 部署脚本（`tools/deploy.sh`）
