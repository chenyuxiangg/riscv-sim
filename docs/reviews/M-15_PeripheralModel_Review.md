# M-15 外设模型层 · 代码审查报告

> **审查对象**：`crates/kernel/src/m15_peripheral/` 全部 5 个文件（`mod.rs` 132 行、`uart.rs` 186 行、`gpio.rs` 203 行、`timer.rs` 161 行、`tests.rs` 272 行，共 954 行）
> **审查基准**：`docs/detailed-design/M-15_PeripheralModel.html`（评审 15-1 / 15-7 / 15-9 / N3 / R5）、`docs/progress/PROGRESS.md` v0.7、`riscv_sim_contract::port::Peripheral`
> **审查日期**：2026-10-19
> **审查方式**：全文件通读 + 契约端口交叉核对 + **三项关键结论以临时用例实证复现**（用例已删除）
> **门禁状态**：`cargo test --workspace` 70 passed / 0 failed（lib 65 + 集成 5，M-15 专项 16 passed）；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：寄存器映射设计清晰、定时器 LO/HI 拆半逻辑正确、16 条测试提供了不错的结构性覆盖。但发现 **3 个阻断级功能缺陷**——UART 的 TX 输出完全不可达、RX 生命周期只能收 1 字节、GPIO TOGGLE 的文档语义与实现互斥。其中 3 条已被现有测试**固化**为「正确行为」，因此门禁全绿。三条缺陷直接否决 PROGRESS v0.7 声称的 N5「GPIO 翻转可见」内核前置条件与 E2E-01「字符回环」场景。

---

## 一、实证复现记录

审查中为三条关键结论写了临时用例，实测输出（用例已删除，仓库无残留）：

```
# 结论 2：UART RX 读取后不清空
RX first='Z' second='Z' equal=true
STATUS after 2 reads = 0b0011  RX_FULL still set = true
after 2nd receive, RX_OVERRUN set = true

# 结论 3：GPIO TOGGLE 文档语义 vs 实现语义
doc-semantics write (1<<3)        -> OUT = 0b0      ← 按模块文档写，不翻转
impl-semantics write ((1<<5)|3)   -> OUT = 0b1000   ← 只有这个能翻转
doc-semantics write (1<<0)        -> OUT = 0b1000   ← 仍不翻转

# 结论 5：寄存器内子偏移字节访问
GPIO read offset 0x1 => Err("E_PER-02: GPIO offset 0x1 not defined")
UART read offset 0x1 => Err("E_PER-02: UART offset 0x1 not defined")

# 附带：mod.rs:113 的 re-export 名字错误使外部用例无法编译
error[E0599]: no method named `read` found for struct `Uart16550`
  ← 文档承诺写 m15_peripheral::Peripheral，实际别名是 PeripheralTrait
```

---

## 二、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 建议修复 |
| --- | --- | --- | --- | --- | --- |
| 1 | **阻断** | `uart.rs:48`、`:74-100` | UART 的 TX 字节**没有任何公开取出路径**，固件输出永久不可达 | `tx` 为私有字段，全代码库仅 `transmit()` 写、`tx_pin()`/`refresh_status()` 读，无 `take_tx()`/`drain_tx()`。固件写第一个字节后 `tx` 永远 `Some` → `tx_pin()` 永久 false（永久 busy）→ 后续 `transmit()` 全部走 overrun 分支。**FR-07 串口、Q5 canvas 行为判定、terminalView 全部无法落地** | 增加 `pub fn take_tx(&mut self) -> Option<u8>`（取出并清空，同步 `refresh_status`），由 M-12/M-07 的主机桥接消费 |
| 2 | **阻断** | `uart.rs:144`；根因在 `contract/src/port.rs:97` | UART 的 RX FIFO **读取后永不清空**，整个生命周期只能收到 1 个字节 | `read(&self, …)` 拿 `&self` 无法清 `self.rx`。实测：连读两次都返回 `'Z'`，`RX_FULL` 持续置位，第二次 `receive()` 必然置 overrun。**E2E-01「字符回环」场景不可能工作**。根因是契约层结构缺陷——`Peripheral::read` 的 `&self` 签名从结构上无法表达 read-clear FIFO，非局部 bug | 契约层给 `read` 增加 `&mut self`，或新增 `fn read_clear(&mut self, …)` 默认方法；M-15 在 M0 解冻窗口内处理（见 §四） |
| 3 | **阻断** | `gpio.rs:11-12` vs `:171-183` | GPIO TOGGLE 的**文档语义与实现互斥**，按文档写的固件永远翻不动 LED | 文档：「low 5 bits select a pin number; writes of 1 to that pin's `out` flip it」；实现：`let pin = value & 0x1F; if (value >> 5) & 1 == 1`——判的是**第 5 位**，不是「该 pin 位是否为 1」。实测 `1<<3`、`1<<0` 均不翻转，只有 `(1<<5)|3` 生效。`tests.rs:153` 用的正是实现语义，**测试把代码固化、掩盖了文档矛盾**。**N5「GPIO 翻转可见」前提不成立** | 二选一并同步另一侧：① 按文档实现（`if value >> pin & 1 == 1 { flip }`）并改 `tests.rs:153`；② 按实现改文档。建议 ①——`1<<pin` 写法固件更直观，且与「写 OUT 寄存器」的 mental model 一致 |
| 4 | 高 | `uart.rs:78` | TX FIFO 溢出置的是 **RX_OVERRUN** 位 | 无 TX overrun 位定义，把两个不同条件合并到一个标志位。固件在 TX 停顿后去「清 RX overrun」会被误导。`tests.rs:93-96` 断言了该行为，**把错误语义固化为预期** | 新增 `status_bits::TX_OVERRUN`（如 0x8），`transmit()` 溢出置 TX 位；同步改 `tests.rs:93` |
| 5 | 高 | `uart.rs:142`、`:154`；`gpio.rs:136`、`:150`；`timer.rs:102`、`:119` | 三个外设**全部忽略 `WriteSize`，且不支持寄存器内子偏移** | 全部按精确字对齐 offset 匹配。实测 `gpio.read(0x1, Byte)` 与 `uart.read(0x1, Byte)` 均返回 E_PER-02。固件对 4 字节寄存器窗口内做 `lb`/`sb` 会拿到虚假的 E_PER-02。契约文档（`port.rs:95-96`）明写「Width depends on the offset」，实现与之矛盾 | ① 把 `match offset` 改为 `match offset & !0x3`（或 `offset >> 2`）做寄存器选择，宽度只决定返回值的截断/合并；② 现有测试全程传 `WriteSize::Byte` 但只用字对齐 offset（`tests.rs:71`、`:85`、`:141` 等），**系统性掩盖了这个缺口**，需补子偏移用例 |
| 6 | 高 | `contract/src/port.rs:107`；`mod.rs:102` | IRQ 抬升路径结构性缺失，FR-06 无实现 | `Peripheral::tick` 只返回 `Vec<PendingWrite>`，无处抬升 IRQ。PROGRESS v0.7 决策 #3 确认改用 `take_pending_irq()` 拉模型，但：① `raise_irq()` 留在代码里且**零调用**，其文档称「Wraps the Result to surface E_PER-04」而函数体只是透传 `sink.raise`，并不映射任何错误码；② 拉模型要求主机每量子轮询，而 M-12 的 `PeripheralTick` 分派目前是空 TODO（`m12_engine/mod.rs:155-159`），**`Peripheral::tick` 全链路无人调用** | 删除死代码 `raise_irq`，或在契约层补 `InterruptSink` 注入；M-18 阶段前必须让 M-12 真正调用 `tick`，否则定时器与 FIFO 时序推进均不发生 |
| 7 | 高 | `gpio.rs:64-66`、`:74-77`、`:117-122` | **GPIO 方向配置完全失效**——`directions` 被存取但从不参与任何行为判定 | `set_external_inputs` 调用 `directions_complement_or_zero()`，该函数**恒返回 0**（`:121`），于是 `edges` 恒为 0 并被 `let _ = edges;` 立即丢弃；真正的边沿逻辑用 `prev ^ value`，不看方向。`write(OUT)`、`:171` 的 TOGGLE 也都不查方向。结果：`directions` 字段只被 `read(DIR)` 回读，对 IN/OUT/IRQ 行为零影响。函数名承诺「方向取反」却是常量，属误导性死代码 | 删除 `directions_complement_or_zero` 与 `let _ = edges;`；让 `set_external_inputs` 的边沿判定真正用 `directions`（输出脚的变化不应产生输入边沿）；或明确文档声明「MVP 方向仅作寄存器回显」并在 LLD 记录 |
| 8 | 中 | `timer.rs:155` vs `:67-69`、`:123-128` | `tick` 无条件覆写 `mtime`，使 `set_mtime` 与固件写 `MTIME_*` 立即失效 | `tick(now_ps)` 执行 `self.mtime = now_ps`，任何此前的 `set_mtime()` 或固件写入在下一次 tick 后消失。两个 API 语义互斥。且把「总线侧定时器」与「引擎虚拟时间」两个时间基混为一谈 | 明确 `mtime` 归属：要么 `tick` 做增量推进（`self.mtime = self.mtime + delta`），要么删除 `set_mtime` 与固件写权限并在文档写明 mtime 只读由引擎驱动 |
| 9 | 中 | `mod.rs:57-60` | `register()` 静默覆盖同 id 外设 | `let _ = self.by_id.insert(...)` 显式丢弃被替换的 `Box`，旧外设被静默 drop。两个同名 GPIO bank → 先注册的那个无声消失 | 改用 `entry()` 冲突时返回 `Result` 或至少 `eprintln!` 告警；MVP 单板场景可先加断言 |
| 10 | 中 | `mod.rs:110-113` | `Peripheral` 的 re-export 名字与自身文档不符 | 文档承诺「external code can write `m15_peripheral::Peripheral`」，实际别名是 `PeripheralTrait`。外部代码必须回到 `riscv_sim_contract::port::Peripheral`——本次审查写实证用例时**首轮编译即因此失败**（见 §一）。这也削弱了「契约层是唯一来源」的可发现性 | 改为 `pub use riscv_sim_contract::port::Peripheral;`（模块内无同名冲突时可直接转出） |
| 11 | 中 | `m14_bus/mod.rs:325-337`（跨模块） | `Peripheral::size()` 与 `Bus::map_peripheral(base, size, …)` 的 size 无一致性校验 | 两者独立取值，不一致时总线会派发到外设窗口内、外设返回 E_PER-02。M-14 审查报告 #13 已记录 M-14 侧现状，M-15 侧同样无防御 | M3 接线 PR 中加 `debug_assert_eq!(p.size(), region_size)` 或在 `map_peripheral` 接受 `&dyn Peripheral` 由外设自带 size |
| 12 | 低 | `uart.rs:54`、`timer.rs:40` vs `gpio.rs:109` | 同一模块内三种外设封装风格不一致 | UART/Timer 的 `irq_line` 是 `pub` 字段可任意改写， GPIO 用私有字段 + `set_irq_line_for_pin()` setter | 统一为私有字段 + setter / 构造参数 |
| 13 | 低 | `gpio.rs:93-106` | `take_pending_irqs()` 不去重 | 多 pin 绑定同一 line 时产生重复条目。文档（`:91-92`）断言「the IRQ line per pin is unique」，但 `set_irq_line_for_pin` 不做唯一性校验，文档承诺无代码保障 | 去重后再返回，或在 `set_irq_line_for_pin` 检测重复并拒绝 |
| 14 | 低 | `uart.rs:163-165` | STATUS 清 overrun 的极性与常见 W1C 约定相反 | `if value & RX_OVERRUN == 0 { clear }`——写 0 才清零，写 1 不清。多数 UART/PL061 风格是「写 1 清零」（W1C）。`tests.rs:111` 固化了写 0 的行为 | 确认智能灯固件预期后统一；若采用 W1C 则改为 `if value & bit != 0 { clear }` 并改测试 |
| 15 | 低 | `uart.rs:108-115` | `ps_per_bit()` 在 `baud_rate > 1e9` 时返回 0 | 0 ps/bit 物理上无意义；后续接位级发射器时会产生零长度帧或死循环 | 钳制到最小 1 ps，或在构造时校验 baud 范围 |
| 16 | 低 | `uart.rs:82`、`:93`、`:117-128` | `status` 位维护双轨 | `transmit`/`receive` 手工改位，`refresh_status()` 又从 `tx`/`rx` 重新推导，两条路径易漂移（`write(STATUS)` 里两者都调了一次） | 只保留 `refresh_status()` 作为唯一写入点，`transmit`/`receive` 只改 `tx`/`rx` |
| 17 | 低 | `mod.rs:90-96` | `describe()` 是空抽象 | 恒返回 `"behavioural-level"`，代码中不存在任何 fidelity 等级字段，文档所引 M-15 §2.1.1 的 fidelity 元数据无落地载体 | 引入真实等级字段，或推迟到 P1 再暴露该 API |
| 18 | 低 | `mod.rs:120-121`、`timer.rs:85-88` | 生产代码携带死别名与 test-only 方法 | `PeripheralWrite` 为 `pub(crate)` + `#[allow(dead_code)]`；`force_irq_for_test` 挂在 `TimerRiscv` 上 | 前者删除；后者改 `#[cfg(any(test, feature = "testing"))]` |
| 19 | 低 | `tests.rs` 全文 | 测试盲区：TOGGLE 文档语义、寄存器内子偏移字节访问（#5）、连续两字节 RX（#2）、TX 字节取出（#1）、同 id 重复注册（#9）、`set_external_inputs` 的方向语义（#7）、M-12 对 `tick` 的调用（#6） | 这七项恰好是上表全部阻断级与高危项所在路径 | 逐条补齐；**#1 / #2 / #3 建议在修实现的同时把测试改成断言正确语义**，否则会再次固化错误行为 |

---

## 三、值得肯定的部分

| 点 | 说明 |
| --- | --- |
| 定时器 LO/HI 拆半读取 | `timer.rs:103-116` 用 `offset & 0x4 == 0` 选择高低半字，Rust 中 `&` 优先级高于 `==`，四个 offset 的映射经推演**全部正确**。写法略绕但无误 |
| 定时器 mtimecmp 组字 | `timer.rs:129-135` 的掩码保留另一半，固件可按任意顺序写 64 位值，行为正确 |
| 边沿 IRQ latch | GPIO 的 OUT 写入 / TOGGLE / 外部输入三条路径都实现了「变化才锁存」（`gpio.rs:159-168`），且 `take_pending_irqs()` 消费后清零，`tests.rs:181-195` 对上升沿/保持/下降沿三态有完整断言 |
| 定时器 IRQ 粘滞语义 | `tick` 中 `&& !self.pending_irq` 保证单次比较只 arm 一次，`tests.rs:223-238` 覆盖了「未到期不 arm / 到期 arm / 消费后为 None」 |
| 错误码统一 | 三个外设的未定义 offset 统一走 `ErrorFamily::Peripheral` scenario 2，各有专项测试（`tests.rs:125`、`:241`） |
| 子文件拆分 | 按 LLD §3.2-3.4 子组件拆 uart/gpio/timer/tests，`mod.rs` 只留公开 API 与 registry，评审粒度合理 |

---

## 四、建议处理顺序

1. **先解契约阻塞，再改实现**：#2 的根因在 `contract::port::Peripheral::read(&self, …)`，属 M0 冻结范围。需与用户确认是否开一次 M-0 解冻窗口；若不解冻，则 RX 语义只能停留在 MVP 限制并应在 E2E-01 验收标准中显式排除「连续多字节接收」。
2. **修 #1 / #3 即可解锁 N5**：UART TX 取出 API 与 GPIO TOGGLE 语义二选一改完，PROGRESS v0.7 声称的「N5 内核前置条件达成」才成立。建议同时改 `tests.rs:153`，否则测试仍锁错语义。
3. **#4 / #5 在 M-18 接线前完成**：#5 决定固件能否用 `lb`/`sb` 访问寄存器，是固件可用性的硬门槛；#4 影响状态位语义。
4. **#6 跨 M-15 / M-12 / M-16 三模块**：需单独排期，删除 `raise_irq` 死代码可先行。
5. **#7 ~ #18 可合并为一次 `refactor(m15)`**，但 #7（方向失效）与 #9（静默覆盖）建议单独提交，因为它们会改变现有行为。

---

## 五、闭环核对（2026-10-19 复查）

代码规模 954 → **1318 行**；M-15 测试 16 → **25 条**；`cargo test --workspace` **74 passed / 0 failed**；`clippy -D warnings` 与 `fmt --check` 均干净。新增测试经抽查确认**断言的是修复后的正确语义**（`gpio_toggle_uses_documented_pin_bit_semantics` 断言 `1<<3` 翻转、`uart_read_clear_drains_rx_fifo` 断言 `RX_FULL` 清零、`uart_status_clear_is_w1c` 断言写 0 不清/写 1 才清），不是把旧行为改个名字重测。契约层采用**加性**方案（新增带默认实现的 `read_clear`），未破坏 M-0 冻结的其他端口。

| # | 原严重度 | 状态 | 落地证据 |
| --- | --- | --- | --- |
| 1 | 阻断 | ✅ 闭环 | 新增 `take_tx()`（`uart.rs:111`），测试 `uart_transmit_then_take_tx_drains_byte` |
| 2 | 阻断 | ✅ 闭环（待接线） | 契约新增 `Peripheral::read_clear` 默认方法（`contract/port.rs`），UART 覆写为 `read_word_mut`（`uart.rs:215`、`:275`）。**但 M-14 仍对 Peripheral 区域返回 E_BUS-05，尚无调用方**——修复已就位，端到端可达性待 M-14 接线 PR |
| 3 | 阻断 | ✅ 闭环 | 改为 mask-toggle 语义（`gpio.rs:185-196`，`self.outputs ^= mask`），测试断言文档语义 |
| 4 | 高 | ✅ 闭环 | 新增 `status_bits::TX_OVERRUN = 0x8`（`uart.rs:51`），`refresh_status_after_tx_overrun` 独立置位（`:169`） |
| 5 | 高 | ✅ 闭环 | 三个外设均改用 `register_offset()` + `extract_subword()`，各有 `*_sub_register_byte_access` 测试 |
| 6 | 高 | ⚠️ **半闭环** | M-15 侧：死代码 `raise_irq` 已删除。**M-12 侧 `PeripheralTick` 分派仍是空 TODO**（`m12_engine/mod.rs:155-159`），`Peripheral::tick` 端到端仍无人调用——定时器时序推进与 FIFO 时间效应均不发生。属跨模块欠账，非 M-15 范围 |
| 7 | 高 | ✅ 闭环 | `set_external_inputs` 改用 `input_mask = !self.directions`（`gpio.rs:86`），死函数 `directions_complement_or_zero` 与 `let _ = edges;` 已删除；测试 `gpio_external_inputs_latch_pending_irqs_only_for_input_pins` |
| 8 | 中 | ✅ 闭环 | 采纳「mtime 只读」方案：`MTIME_*` 固件写改为 `Ok(())` 静默忽略（`timer.rs:151`），`set_mtime` 已删除，测试 `timer_mtime_writes_from_firmware_are_silently_ignored` |
| 9 | 中 | ✅ 闭环 | `register()` 改返回 `Result`，新增 `RegisterError::Duplicate`（`mod.rs:89-96`），测试 `registry_duplicate_id_returns_register_error` |
| 10 | 中 | ✅ 闭环 | 改为 `pub use riscv_sim_contract::port::Peripheral;`（`mod.rs:31`），测试 `peripheral_trait_direct_re_export_is_resolvable` |
| 11 | 中 | ❌ **未闭环** | `Bus::map_peripheral(base, size, PeripheralId)` 签名不变，M-14 侧无 `Peripheral::size()` 一致性校验。**按设计推迟到接线 PR**，接线时需一并处理 |
| 12 | 低 | ✅ 闭环 | UART / Timer 的 `irq_line` 改私有 + `set_irq_line()` / `irq_line()`，与 GPIO 风格统一 |
| 13 | 低 | ✅ 闭环 | `take_pending_irqs` 加去重位图（`gpio.rs:96-111`），测试 `gpio_take_pending_irqs_dedupes_by_line` |
| 14 | 低 | ✅ 闭环 | 改为标准 W1C（`uart.rs:233-238`），测试 `uart_status_clear_is_w1c` |
| 15 | 低 | ✅ 闭环 | `ps_per_bit()` 加 `.max(1)` 钳制（`uart.rs:149`），测试 `uart_ps_per_bit_matches_baud_rate_and_floors_at_one` |
| 16 | 低 | ✅ 闭环 | `refresh_status()` 成为唯一写入点并保留 overrun 粘滞位（`uart.rs:155-167`） |
| 17 | 低 | ✅ 闭环 | 新增 `FidelityLevel` 四级枚举 + `label()`（`mod.rs:50-58`），`describe()` 改为调用它 |
| 18 | 低 | ◐ 已知悉 | `PeripheralWrite` 死别名已删除；`force_irq_for_test` 保留 `#[cfg(test)]` 并加注 review 编号——**判定为有意保留**，可接受 |
| 19 | 低 | ✅ 大幅闭环 | 测试盲区从 7 项补到覆盖 #1/#2/#3/#4/#5/#7/#8/#9/#13/#14/#15 |

**统计：15 项完全闭环，1 项半闭环（#6 跨 M-12），1 项有意保留（#18），1 项按设计推迟（#11）。3 项阻断级缺陷（#1/#2/#3）全部真实修复并有回归测试锁定。**

### 复查中新发现的遗留问题

| # | 严重度 | 位置 | 问题 | 建议 |
| --- | --- | --- | --- | --- |
| N1 | 中 | `uart.rs:185-197`、`gpio.rs:141-152`、`timer.rs:115-126` | #5 的修复把 `register_offset` + `extract_subword` **逐字复制了 3 份**（6 个函数体），属本次修复引入的 DRY 回归 | 抽到 `m15_peripheral/reg.rs` 共享，三处 `use` |
| N2 | 中 | `gpio.rs:220` | `IN` 寄存器恒返回 `self.inputs`（仅由 `set_external_inputs` 写入），**不回读输出脚**。#7 让 `directions` 真正生效后这个缺口更显眼：固件通过 OUT 驱动 LED 后无法从 IN 读回引脚电平 | 读 IN 时对 `directions & 1` 为 1 的脚用 `outputs` 位覆盖。N5 与 M-18 波形（LED 引脚）都依赖这个 |
| N3 | 低 | `gpio.rs:101-111` | 去重位图是 `u32` 却只覆盖 `line < 16`，`line ∈ [16,32)` 直接跳过去重；注释理由「只有 32 bits headroom」与代码不符 | 放宽到 `line < 32` 并同步注释 |
| N4 | 低 | `contract/port.rs` `read_clear` | 默认实现委托给 `read`，**漏覆写的外设会静默获得非破坏性读**；M-14 接线时若调 `read` 而非 `read_clear`，UART RX 问题会原样复现且无编译期提示 | 接线 PR 的检查项；或在文档标注「RX 类外设必须覆写」。**已上收为台账 D-14** |

**二次复查（2026-10-19 18:59）**：N1 / N2 / N3 三项**尚未处理**，代码与首次复查时一致 —— `register_offset` + `extract_subword` 仍为 3 份重复（`uart.rs:185`/`195`、`gpio.rs:141`/`146`、`timer.rs:115`/`120`）；`gpio.rs:220` 的 `reg::IN => Ok(self.inputs)` 仍未回读输出脚；`gpio.rs:101` 仍是 `line < 16` 上限。三项均属 M-15 模块内可独立闭环项，与跨模块欠账无关，可随时处理。其中 **N2 优先级最高**——M-18 波形与 N5「GPIO 翻转可见」都依赖引脚电平可读。

**三次复查（2026-10-19 19:10）**：N1 / N2 / N3 三项**全部真实闭环**，各有专项测试保护：

| # | 状态 | 落地证据 |
| --- | --- | --- |
| N1 | ✅ 闭环 | 抽出公共模块 `m15_peripheral/reg.rs`（36 行），三个外设统一 `use super::reg::{extract_subword, register_offset};`；`uart.rs:188` 留了「Moved to `super::reg::extract_subword` in M-15 review N1」的迁移说明便于追溯；测试 `reg_helpers_are_dry` 验证三处使用同一来源 |
| N2 | ✅ 闭环 | 新增 `pin_levels()` 辅助方法（`gpio.rs:156-160`）：`output_pins = outputs & directions`、`input_pins = inputs & !directions`，二者按位或合并。`read_word(reg::IN)` 改为 `Ok(self.pin_levels())`（`gpio.rs:231`）。**符合多数 GPIO 控制器惯例：输出脚 IN 回读输出锁存，输入脚 IN 反映外部驱动**。测试 `gpio_in_reflects_output_latch_on_output_direction_pins` + `gpio_in_reflects_input_pins_and_output_latch_simultaneously` |
| N3 | ✅ 闭环 | `line < 16` 改为 `line < 32`（`gpio.rs:101`），位图覆盖整个 32 位 IRQ line 空间；注释明确说明「Lines >= 32 cannot be represented in the 32-bit dedup bitmap; emit them as-is without dedup」。测试 `gpio_take_pending_irqs_dedupes_lines_in_upper_half` |

**M-15 模块内可独立闭环项 100% 清零**。代码规模 1318 → **1424 行**（新增 `reg.rs` 36 行 + GPIO 23 行 + tests 75 行）；M-15 测试 25 → **29 条**；`cargo test --workspace` 78 passed / 0 failed；`clippy -D warnings` 与 `fmt --check` 均干净。**唯一仍 open 的项是跨模块的 #6（M-12 调度 + M-14 接线），按约定不动，对应台账 D-07 / D-12 / D-14。**

---

## 六、审查边界声明

- 本报告仅覆盖 M-15 模块自身；M-14 侧对 `RegionTarget::Peripheral` 的处理（恒返 E_BUS-05）与统计口径问题见 `docs/reviews/M-14_BusMemoryMap_Review.md`。
- #6、#11、#7 涉及跨模块事实（M-12 的空 `PeripheralTick` 分派、M-14 的 `map_peripheral` 签名），已回溯核对源码，结论可靠，但**修复归属不在 M-15 单一模块内**。
- 门禁结论与 §一 实证输出均为 2026-10-19 实际执行结果；临时验证用例已删除，仓库无残留。
- 现有 16 条 M-15 测试对 #4（TX/RX overrun 混用）、#3（TOGGLE 语义）、#14（W1C 极性）**锁定了当前行为**，修实现时必须同步改测试，否则会出现「改对代码、测试却红」的反向困扰。
