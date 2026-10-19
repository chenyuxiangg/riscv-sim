# 跨模块欠账台账 · Cross-Module Debt Ledger

> **定位**：本文件是**跨模块欠账的唯一追踪入口**。模块内部问题归各模块报告（见 §六），本台账只收录**修复归属跨越模块边界**的条目，以及尚未归档到任何报告的项目级发现。
> **建立日期**：2026-10-19
> **状态核实日期**：2026-10-19（全部条目均经源码回溯核对，非引用历史结论）
> **基线**：`cargo test --workspace` 74 passed / 0 failed；`clippy -D warnings` 干净；`fmt --check` 干净
> **关键结论**：**三条现有门禁对本台账全部 5 项阻断级条目均无感知**——Rust 侧全绿，前端白屏，畸形 HEX 输入 panic。

---

## 一、阻断级（5 项 · 建议优先清零）

| ID | 归属模块 | 来源 | 问题 | 实证依据 | 阻塞关系 |
| --- | --- | --- | --- | --- | --- |
| **D-01** | 前端 `m02_viewcomponents.js` ← `m01_workbench.js` | 首次全量审查 P0-1 | `m02:19` 导入 `activeView`，`m01:126` 的 `let activeView` **没有 export** → ES module 链接期失败 | `node --input-type=module -e "import('./front/app.js')"` → `SyntaxError: ...does not provide an export named 'activeView'`（复查仍在） | **阻塞 D-02**（同一模块图，浏览器只报第一个错）；阻塞 M-06/M-13 全部前端可视化 |
| **D-02** | 前端 `app.js` ← `m03_viewstore.js` | 首次全量审查 P0-1 | `app.js:10` 导入 `mountViewStore`，但 `m03` 只导出 `setState/getState/snapshot/subscribe/reset` | 临时移除 D-01 导入后实测：`SyntaxError: ...does not provide an export named 'mountViewStore'` | 阻塞 D-01 的可见性；两条都修完前端才可能启动 |
| **D-03** | M-06 `hex.rs` | 首次全量审查 P0-2 | `:112` 错误格式化里 `bytes.len() - 5` 在 `bytes.len() == 4` 时整数下溢 panic | `:01234567` → `panicked at hex.rs:112: attempt to subtract with overflow`（复查仍在） | 阻塞 M-06 P1 阶段的畸形输入加固；WASM 内等于 worker 崩溃 |
| **D-04** | M-13 ISS | 首次全量审查 P0-3 | **x0 未硬连线为 0**：`executor.rs:97` 与 `mod.rs:161` 均裸写 `state.gpr[d.rd as usize] = result`，读侧 `state.gpr[d.rs1 as usize]` 无保护 | 复查：两处均未加 `if d.rd != 0` 守卫，也无 `read_gpr()` 辅助 | **阻塞 Q7 Spike 对拍（FR-04，必做）**——第一条 golden 用例即红；越晚修，golden 用例返工面越大 |
| **D-05** | M-12 引擎 | 首次全量审查 | `snapshot()` 只返回 `SimState`，`restore()` **不还原 `now_ps`、不处理事件队列** | `m12_engine/mod.rs:171`、`:176` 复查未变 | 阻塞 FR-12 确定性重放；阻塞 M-20 快照 / M-23 落盘 |

---

## 二、跨模块高危（9 项）

| ID | 归属模块 | 来源 | 问题 | 阻塞关系 / 备注 |
| --- | --- | --- | --- | --- |
| **D-06** | M-12 | 首次全量审查 | `run_until_idle` 每个事件耗 1µs 量子；事件在 t=1e12 ps 即空转 1e9 次 | 阻塞 **N8 性能基线 ≥5 MIPS**；建议改为 `now = min(now+QUANTUM, next_event_at)` |
| **D-07** | M-12 | 首次全量审查 + M-15 报告 #6 | `dispatch` 的 `PeripheralTick` 分支是空 TODO（`m12_engine/mod.rs:155-159`） | **同时阻塞 M-15 的 `Peripheral::tick`**：定时器时序推进、FIFO 时间效应目前全链路不发生。M-15 侧 `raise_irq` 死代码已删，此项是剩余欠账 |
| **D-08** | M-06 `hex.rs` | 首次全量审查 | `finalise:247` 超 `segment_count` 时 `truncate()` **静默丢固件数据**且不报错 | 应改 `Result` + LDR-02 |
| **D-09** | M-06 `elf.rs` | 首次全量审查 | `read_u16`/`read_u32`（`:301`、`:308`）为 `panic!`，而上方注释承诺「returns PlatformError(E_LDR-01)」 | 当前不可达（`parse` 已校验边界），但解析不可信输入的路径不应留 panic |
| **D-10** | **M-06 + M-14 联合** | M-14 报告 #6 | `ParseLimits` 无 `total_image_bytes` / `max_region_bytes`；M-14 `prepare_bytes:394` 裸 `vec![0u8; size as usize]`，`size` 最大可达 ~4 GiB | **两模块同一件事的两面**：84 字节畸形 ELF 即可申请 GiB 级内存，浏览器内 OOM。单改任一模块都不完整 |
| **D-11** | M-13 ISS | 首次全量审查 | `BusExecutor`（`m13/mod.rs:140`）复制 `Executor` 的 ADDI 逻辑，`bus` 字段挂 `#[allow(dead_code)]` 从未被读 | LOAD/STORE 一旦实现会**静默绕过总线**（丢 stats 与 M-18 hook）；两条路径非法指令语义还不同（`Trapped` vs `Err`） |
| **D-12** | M-14 ↔ M-15 接线 | M-14 报告 #13 / M-15 报告 #11 | `Bus::map_peripheral(base, size, PeripheralId)` 的 size 与 `Peripheral::size()` 无一致性校验 | 按 PROGRESS 决策 #4 推迟到接线 PR。**接线时必须一并处理**，否则总线会派发到外设窗口外、外设返 E_PER-02 |
| **D-13** | M-14 | M-14 报告 #9 | `Region.target` / `RamBackend.bytes` 仍为 `pub` | 跨模块波及：`crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable` 均消费 region 字段，收紧可见性需同步改这两处 |
| **D-14** | 契约 + M-14 | M-15 报告 N4 | `Peripheral::read_clear` 默认实现委托 `read`，**漏覆写的外设静默获得非破坏性读** | M-14 接线时若调 `read` 而非 `read_clear`，UART RX「只能收 1 字节」的原缺陷会**静默复现且无编译期提示**。建议列入接线 PR 检查单 |

---

## 三、跨模块中低危（8 项）

| ID | 归属模块 | 来源 | 问题 |
| --- | --- | --- | --- |
| **D-15** | M-06 `elf.rs` | 首次全量审查 | 缺 `p_filesz <= p_memsz` 校验；畸形 ELF 会建出超过声明内存的段 |
| **D-16** | M-13 ISS | 首次全量审查 | `Decoded` 无指令长度字段，PC 恒 `+4`；RV32IMC（Q2 决议，需 2/4 字节步进）落地前必须先改结构 |
| **D-17** | M-12 | 首次全量审查 | `schedule()` 把 seq 计数塞进 `state.event_queue_head`；`restore()` 后可能与队列中已有事件撞号，破坏确定性排序 |
| **D-18** | 前端 `m02` | 首次全量审查 | 「刷新 demo 映射」按钮恒不触发：`m02:115` 要求 `closest("[data-region-base]")` 非空，但按钮在 `<table>` 外 |
| **D-19** | 前端 `m01` | 首次全量审查 | `m01:225` `setState(activeView.hash, {})` 注释称「保存状态」，因 merge 语义实为 no-op——非数据丢失，但意图与实现不符 |
| **D-20** | 前端 `m02` | 首次全量审查 | `onDelta` 用 `textContent = ... + escapeHtml(bytes)` 双层转义，会显示字面 `&lt;` |
| **D-21** | M-06 `mod.rs` | 首次全量审查 | `detect_kind` 未知扩展名一律回落 `Elf`，拖入无扩展名 raw 文件报 "bad ELF magic"；建议按 magic bytes 嗅探 |
| **D-22** | M-06 `mod.rs` | 首次全量审查 | `Loader::load_with_limits` 默认实现 `let _ = limits;` 静默忽略 limits；`parse_raw` 复用 `elf_size` 限制 raw 文件 |

---

## 四、工程门禁缺口（根因级）

| ID | 问题 | 说明 |
| --- | --- | --- |
| **D-23** | 三条门禁对上述 5 项阻断**全部无感知** | `cargo test` 只覆盖 Rust；D-01/D-02 是 ESM 链接期错误，只能在浏览器或 Node 暴露；D-03 是 `panic!` 而非 `Err`，测试没喂这个输入；D-04/D-05 现有用例恰好绕开了触发条件。**这是「47→70→74 全绿却仍白屏 + 可 panic」的根因** |

**建议补的最小门禁**：
1. `node --input-type=module -e "import('./front/app.js')"` 作为 CI 一步（可捕获 D-01/D-02 及未来任何 ESM 链接回归）；
2. M-06 增加畸形输入表驱动用例（短记录 / 坏 magic / `filesz > memsz` / 超长段），覆盖 D-03、D-15；
3. M-13 增加 `addi x0, x0, 42` → `gpr[0] == 0` 断言，作为 D-04 的回归锁；
4. M-12 增加「snapshot → restore → 继续推进」的确定性断言，覆盖 D-05。

---

## 五、建议处理顺序（含阻塞拓扑）

```
第 1 批 · 立刻可做，无前置
  D-01 ─┐
  D-02 ─┴─→ 前端恢复可启动（两者必须同批，只修一个仍白屏）
  D-03  → M-06 panic 消除
  D-04  → 解锁 Spike 对拍的前置条件（Q7 必做）
  D-23  → 补上述 4 类门禁，防止同类问题复发

第 2 批 · 里程碑相关
  D-05 + D-17  → FR-12 确定性（M-20 / M-23 的共同前提）
  D-06        → N8 ≥5 MIPS 性能基线
  D-16        → M-13 扩 RV32IMC 前必须先做，否则解码结构要返工
  D-11        → 同上；且在 LOAD/STORE 落地前必须先做

第 3 批 · 接线 PR（独立排期）
  D-07        → M-12 真正调用 Peripheral::tick（同时解 M-15 报告 #6 半闭环）
  D-12 + D-14 → M-14 ↔ M-15 接线 PR 的强制检查项
  D-10        → M-06 ParseLimits + M-14 prepare_bytes 联合改动
  D-13        → 字段可见性收紧（需同步改集成测试与前端 renderMapTable）

第 4 批 · 整洁批次，可合并一次 refactor
  D-08 D-09 D-15 D-18 D-19 D-20 D-21 D-22
```

**关键路径提示**：`D-04` 是关键路径上最靠前的一项——Q7 决议「Spike 对拍必做（FR-04）」，而 M-13 扩指令集（M2 阶段，12 人日）排在它之后。x0 问题越晚修，需要重跑的 golden 用例越多。

---

## 六、模块内部欠账（不在本台账范围）

以下条目**归属单一模块**，已记录在各自报告中，本台账不重复：

| 模块 | 报告 | 剩余欠账 |
| --- | --- | --- |
| M-14 | `M-14_BusMemoryMap_Review.md` §四 | #10 / #11 / #12 / #14（半）/ #16 共 5 项整洁项 + N5 / N6；#9 已上收为 D-13 |
| M-15 | `M-15_PeripheralModel_Review.md` §五 | N1（三处 `register_offset`/`extract_subword` 重复）、N2（GPIO `IN` 不回读输出脚，阻塞 N5/M-18 波形）、N3（去重位图 `< 16` 上限）、N4 已上收为 D-14；#11 已上收为 D-12 |
| M-13 / M-12 / M-06 / 前端 | 尚无模块报告 | 本台账 D-04~D-06、D-08~D-09、D-11、D-15~D-22 是其**首批**待审条目 |

**下一步建议**：D-04（M-13 x0）与 D-05/M-06（M-12 快照）可作为下一轮模块报告的对象；`front/` 与 `m06_elf/` 的模块级审查尚未做过，是当前最大的盲区。

---

## 七、状态变更记录

| 日期 | 变更 | 说明 |
| --- | --- | --- |
| 2026-10-19 | 台账建立 | 从首次全量审查（仅存在于对话、未归档）与 M-14 / M-15 两份模块报告中提取全部跨模块条目，逐条回溯源码核实当前状态 |
| 2026-10-19 | M-15 报告闭环 | 15 项完全闭环 / #6 半闭环（上收为 D-07）/ #11 按设计推迟（上收为 D-12）/ #18 有意保留 |
| 2026-10-19 | M-14 报告闭环 | 9 项完全闭环 / #14 半闭环 / 6 项推迟；#6 上收为 D-10、#9 上收为 D-13 |
