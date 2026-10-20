# M-06 ELF / Raw / HEX 解析器 · 代码审查报告

> **审查对象**：`crates/kernel/src/m06_elf/`（`mod.rs` / `elf.rs` / `hex.rs` / `tests.rs` / `tests_malformed.rs`）
> **审查基准**：`docs/detailed-design/M-06_ElfRawHexParser.html`（LLD §4.6）、`docs/reviews/CROSS_MODULE_DEBT.md`（D-03 / D-08 / D-09 / D-10 / D-15 / D-21 / D-22 / D-23）、`AGENTS.md`
> **审查日期**：2026-10-20
> **审查方式**：全文件通读 + 调用方交叉核对（`crates/kernel/tests/m06_load_to_bus.rs` / 前端 / contract）+ 台账逐条复核 + 畸形输入自建语料 fuzz
> **门禁状态**：`cargo test --workspace` **232 passed / 0 failed**（基线 203）；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：M-06 是台账里**唯一从未做过模块级 review 的已开发模块**。本次共查出 **17 项**问题（1 阻断 / 3 高 / 5 中 / 8 低），**已闭环 12 项**（含唯一阻断项 D-03），**未修 5 项**（4 项为设计取舍或跨模块，1 项需 M-14 侧配套）。**D-03 修复前，任意畸形 HEX 文件可让 WASM worker 崩溃**——这是本模块此前最严重的缺陷，也是三条既有门禁全部失明的原因。

---

## 一、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 1 | **阻断** | `hex.rs:112`（旧） | 「declared count != actual payload」错误分支里写 `bytes.len() - 5`，当记录解码后不足 5 字节时**整数下溢 panic** | D-03。实测复现：输入 `:01234567` → `panicked at hex.rs:112: attempt to subtract with overflow`。在 WASM 中 panic = worker 崩溃，等于「拖入一个畸形 HEX 文件就能打挂仿真器」 | ✅ 已修（`34cc38b`） |
| 2 | 高 | `hex.rs:247`（旧） | `finalise` 在合并后段数超 `segment_count` 时 `truncate()`，**静默丢弃固件数据**且不报错 | D-08。主机拿到的是一个「看起来合法」的截断镜像，缺失的固件字节要到运行时才暴露 | ✅ 已修（`e7fb3e0`）。**复核补充**：在当前控制流下该分支实际**不可达**——循环内的 `buf.len() > segment_count` 检查会在 EOF 早退前先拦下超限输入。故本次修复是「把静默丢数据改成显式报错」的防御性收口，而非修复一条可观测的数据丢失路径 |
| 3 | 高 | `elf.rs:301` / `:308`（旧） | `read_u16` / `read_u32` 是 `panic!`，而函数上方注释承诺「returns PlatformError(E_LDR-01)」——**文档与实现自相矛盾** | D-09。解析不可信输入的路径上留 panic。虽然 `parse` 已预校验边界使其不可达，但「不可达」是调用顺序耦合出来的结论，不是类型系统保证 | ✅ 已修（`0258a34`）：改为 `Result`，顺带移除 `phnum * e_phentsize` 处的 `.expect(...)` panic |
| 4 | 高 | `mod.rs:113`（旧） | `Loader::load_with_limits` 默认实现 `let _ = limits;` ——**静默丢弃调用方的限额** | D-22。限额形同虚设：调用方收紧 `segment_count` / 文件大小上限，解析器一概不理会 | ✅ 已修（`189f500`）：`load_with_limits` 去掉默认实现改为**必须实现**，`load` 反过来成为带默认限额便捷方法 |
| 5 | 中 | `mod.rs:122`（旧） | `detect_kind` 对未知扩展名一律回落 `Elf` | D-21。拖入无扩展名 raw 文件只会得到 "bad ELF magic"，用户无从判断该换什么格式 | ✅ 已修（`189f500`）：新增 `detect_kind_from_bytes`（magic bytes 嗅探）与 `detect_kind_with_hint`（magic 优先、扩展名辅助）；`detect_kind` 未知扩展名改回落 `RawHex` |
| 6 | 中 | `elf.rs` PT_LOAD 循环 | 缺 `p_filesz <= p_memsz` 校验 | D-15。畸形 ELF 会被接受：按 `p_filesz` 切字节、按 `p_memsz` 算 bss 尾巴，建出的段与文件头声明的内存自相矛盾 | ✅ 已修（`9a9d578`）：走 E_LDR-01 |
| 7 | 中 | `mod.rs` `ParseLimits` | 缺 `total_image_bytes` / `max_region_bytes`：**限输入文件大小 ≠ 限镜像大小** | D-10。`.bss` 尾巴根本不存放在文件里，一个 84 字节的畸形 ELF 就能声明 GiB 级内存并让加载器去分配 | ✅ M-06 侧已修（`374d647`）。**M-14 侧未做**（`prepare_bytes` 裸 `vec![0u8; size]`）——见 §五 |
| 8 | 中 | `mod.rs` `parse_raw` | raw 文件复用 `elf_size` 限额 | D-22。语义混淆：「16 MiB ELF 上限」被当成了「16 MiB raw 上限」，raw 镜像的大小本就该独立设定 | ✅ 已修（`189f500`）：`ParseLimits` 新增 `hex_size` / `raw_size`，每个限额字段只管一种输入 |
| 9 | 中 | `hex.rs` `append_or_merge` | IHEX 的 **count=0 数据记录**会产出一行零长度段 | 本次自排查新增。`LoadImage.segments` 里出现 0 字节段 → M-14 的 `map_ram(base, size.max(1), data)` 会映射出 1 字节幽灵 RAM；地址若与真实段重叠还会报 E_BUS-03，症状极难定位 | ✅ 已修（`0744411`）：空 payload 直接忽略（选「忽略」而非「拒绝」，不损失任何固件数据） |
| 10 | 中 | 整个模块 | 缺少「畸形输入必须 `Err`、不得 panic」这条**可执行契约** | D-23 门禁缺口第 2 项。台账结论：三条既有门禁对畸形输入整类缺陷完全无感知——D-03 是个 `panic!` 而非 `Err`，测试从未喂过那个输入 | ✅ 已修（`955055f`）：`tests_malformed.rs`，20 条表驱动畸形用例 + 2 条固定种子 fuzz 用例。**反向验证**：临时回退 D-03 的修复后，表驱动与 fuzz 用例双双 panic 失败——门禁确实能抓住复发 |
| 11 | 低 | `hex.rs:98` | `count > HEX_RECORD_MAX_DATA`（0xFF）**恒不可达**：`count` 取自单个字节 `bytes[0] as usize`，最大就是 255 | 死分支。LLD §3.3 伪码里有同一条检查（`if (count > 255)`），实现忠实照搬 | ⬜ 未修：保留以对齐 LLD。真正的行内长度约束由 `bytes.len() != count + 5` 承担 |
| 12 | 低 | `hex.rs:143` | `ext_high.checked_add(addr_field)` 的溢出分支**恒不可达** | 死分支。`ext_high` 上限 `0xFFFF << 16 = 0xFFFF_0000`，`addr_field` 上限 `0xFFFF`，和恰为 `0xFFFF_FFFF`——刚好是 `u32::MAX`，不溢出。实际拦截溢出的是下一行的 `base.checked_add(bytes_len)` | ⬜ 未修：防御性代码，保留无害 |
| 13 | 低 | `elf.rs:346` | `e_entry` 不与已加载段做交叉校验 | 入口地址落在任何段之外也能加载成功，要到 M-13 取指时才以总线错误暴露 | ⬜ 未修：按 LLD §6.2（6）「Raw 基址衔接」的口径，前端不预判映射；M-13/M-14 侧运行时校验是正确归属 |
| 14 | 低 | `elf.rs:134` / `:141` | `e_ehsize` 读出后即丢弃（`let _ = (...)`），不校验是否等于 52 | 轻微：畸形头部的 `e_ehsize` 不被检查。`e_phentsize` 有校验，`e_ehsize` 没有，二者不对称 | ⬜ 未修：MVP 不使用该字段；补校验会多一条拒绝路径而收益有限 |
| 15 | 低 | `elf.rs:180` | 段数预算卡的是 `e_phnum`（**全部**程序头数），不是 PT_LOAD 数 | 过度保守：100 个程序头但只有 2 个 PT_LOAD 的 ELF 会被拒（100 > 64），尽管最终只产出 2 个段 | ⬜ 未修（已加严）。本轮另补了「**输出**段数也过 `segment_count`」的检查——一个带 `.bss` 尾巴的 PT_LOAD 会产出**两个**段，只卡 `e_phnum` 约束不到真正交给 M-14 的段数 |
| 16 | 低 | `mod.rs` `ParseLimits` | `symbol_count` / `line_entry_count` 两个字段**在 M-06 侧不生效** | M-06 不解析符号表与 DWARF 行表（属 M-11）。字段留着是为了对齐 LLD §2.1.1 的 `parseLimit` 结构 | ⬜ 未修（已在 `ParseLimits` 文档中写明由 M-11 执行）。属跨模块欠账，见 §五 |
| 17 | 低 | `mod.rs` `detect_kind_from_bytes` | magic bytes 嗅探存在**假阳性**：raw 二进制若首字节为 `0x3A`（`:`），会被判成 HEX 并解析失败 | 概率低（首字节恰为 `:` 的 raw 固件罕见），但后果是「该文件加载不了」而非「降级处理」 | ⬜ 未修：按 brief 规定「IHEX 以 `:` 开头」的规则实现。已记录为已知限制，建议后续把「首行是合法 IHEX 记录（含校验和）」作为更严的判据 |

### 1.1 架构与依赖观察（不计严重度，供决策参考）

| 观察 | 说明 |
| --- | --- |
| **M-06 → M-14 依赖方向** | `mod.rs` 里 `pub use crate::m14_bus::BUS_SIZE;`，M-06 直接依赖 M-14 的常量。LLD §9.1 把 M-06 定位为「前端交互层」、M-14 为内核侧——实现把 M-06 放进了 Rust 内核并反向依赖 M-14，与 LLD 的分层描述有出入。**依赖方向本身仍在 LLD §9.1 允许的「M-06 → M-14」之内**，但分层归属值得与协调者确认 |
| **wasm32 的 `usize` 宽度** | 本模块编译为 `wasm32-unknown-unknown`，其 `usize` 是 **32 位**。`p_offset + p_filesz` 在 wasm32 上**会**溢出 u32，`elf.rs` 里原有的 `checked_add` 因此是必要的（不是死代码）；同理 `total_image_bytes` / `max_region_bytes` 一律用 `u64` 而非 `usize`，避免 wasm32 上的截断。审查时**不能**按 64 位 native 的直觉判断这些 `checked_*` 是冗余的 |
| **段内容拷贝** | ELF 段用 `to_vec()` 拷贝，文件大小与镜像大小各占一份内存。已被 `elf_size` / `total_image_bytes` 双重封顶，MVP 可接受 |

---

## 二、值得肯定的设计

| 评审项 | 设计点 | 落地情况 |
| --- | --- | --- |
| LLD §3.3 (06-4) | HEX 归并策略：同基址后行覆盖、相邻地址合并成段 | ✅ `hex.rs` `append_or_merge` + `finalise` 两级归并，用例 `hex_last_wins_overwrites_same_base` / `hex_adjacent_records_merge_into_single_segment` 覆盖 |
| LLD §6.2 (1) | HEX 畸形四校验（前缀 / 长度 / 校验和 / 记录类型） | ✅ 四项齐全，且校验和先于记录类型判定（符合 IHEX 惯例） |
| LLD §6.2 (2) | 段数上限前置校验 | ✅ 本轮由「只卡 `e_phnum`」加严到「卡 `e_phnum` + 输出段数」 |
| LLD §3.1 | 畸形 ELF 拒绝（魔数 / class / version / machine / 头界） | ✅ 全部在 `elf.rs` 前段完成，且**顺序**合理：长度 → magic → class → data → version → 类型 → 机种 → 段数 → 头界 |
| LLD §2.2 | bss 段仅登记、零填充由内核侧按 size 清零 | ✅ M-06 把 bss 尾巴作为独立的零填充段输出，与 M-14 的整零初始化语义对齐；`crates/kernel/tests/m06_load_to_bus.rs` 的端到端用例验证了 .text/.bss 被映射成两块不重叠的 RAM |
| — | 不引入 `object` crate | ✅ 手写 32 位 LE 解析器，依赖面与 WASM 体积可控，GPL 合规无风险（与 PROGRESS.md v0.5 决策一致） |
| — | 错误码收敛 | ✅ 全部走 `ErrorFamily::Loader` + scenario 1/2/3 三档，LLD 的 E_PROTO-03/04 语义映射清晰；`LOADER_FAMILY` 公开常量便于下游不必拼写族名 |
| — | 确定性 | ✅ 纯函数式输出，同一输入必得同一 `LoadImage`（归并前后顺序由 `sort_by_key` 固定） |

---

## 三、建议处理顺序

1. **已完成（阻断级）**：D-03 panic 消除。任何畸形文件都不能打挂 worker，这是加载器的第一属性。
2. **已完成（高）**：D-08 / D-09 / D-22 —— 静默丢数据、panic 路径、限额失效，三者都是「看起来在防、实际没防」。
3. **已完成（中）**：D-21 嗅探、D-15 校验、D-10（M-06 侧）、D-23 门禁 #2。
4. **待协调者指派**：D-10 的 M-14 侧配套（`prepare_bytes` 的裸 `vec![0u8; size]`）——M-06 侧已保证交给 M-14 的段不超预算，但 M-14 自身仍需自查，两者缺一不可。
5. **待 M-11 落地时闭环**：`symbol_count` / `line_entry_count` 的实际执行。
6. **可选打磨**（不影响正确性）：#11 / #12 两条死分支的清理，#14 `e_ehsize` 校验，#17 嗅探假阳性的判据收紧。

---

## 四、闭环核对（2026-10-20）

代码规模 1189 → **2483 行**（含 1178 行测试）；M-06 测试 18 → **47 条**；`cargo test --workspace` **232 passed / 0 failed**（基线 203）；`clippy -D warnings` 与 `fmt --check` 均干净。

| # | 原严重度 | 状态 | 落地证据 |
| --- | --- | --- | --- |
| 1 | 阻断 | ✅ 闭环 | `hex.rs` 错误分支改 `bytes.len().saturating_sub(5)`；用例 `hex_rejects_short_record_without_panicking`；**反向验证**：临时回退修复后新门禁立即 panic 失败 |
| 2 | 高 | ✅ 闭环 | `finalise` 改返回 `Result`，超段数报 E_LDR-02 而非 `truncate()`；另把合并判定的 `wrapping_add` 改 `checked_add`（`0xFFFF_FFF0 + 0x10` 回绕成 `0x0` 会被误判为地址连续，把固件字节搬到错误地址）；3 条 `finalise` 私有辅助函数用例 + 1 条端到端用例 |
| 3 | 高 | ✅ 闭环 | `read_u16` / `read_u32` 改 `Result<u16/u32, PlatformError>`，全部调用点 `?` 传播；移除 `phnum * e_phentsize` 的 `.expect(...)`；4 条读函数用例（越界 / 极端偏移 / 正常解码） |
| 4 | 高 | ✅ 闭环 | `Loader` trait 的 `load_with_limits` 去掉默认实现改为必须实现；`load` 成为带默认限额的便捷方法；4 条用例验证 `elf_size` / `raw_size` / `hex_size` / `segment_count` 逐项生效。**保证性质说明**：限额是**契约性保证**——所有解析路径都必须读 `limits`，且 trait 的默认实现已删除（漏实现就编译不过）。但它**不是**由回归测试锁死的：检视 V3 的变异测试删掉某条路径的限额检查后，M-06 的 47 条用例仍全绿。后续若改动限额校验，需靠人工复核而非依赖门禁 |
| 5 | 中 | ✅ 闭环 | 新增 `detect_kind_from_bytes`（ELF magic / IHEX 首行 `:`）与 `detect_kind_with_hint`（magic 优先、扩展名辅助、扩展名明确指向某格式时仍尊重它以给出精确错误）；5 条用例 |
| 6 | 中 | ✅ 闭环 | PT_LOAD 循环内新增 `p_filesz > p_memsz` → E_LDR-01；用例 `elf_rejects_filesz_greater_than_memsz` |
| 7 | 中 | ✅ 闭环 | M-06 侧已加 `total_image_bytes`（默认 64 MiB）/ `max_region_bytes`（默认 16 MiB）并在 ELF / HEX / raw 三条路径逐项校验（5 条用例）。**M-14 侧亦已闭环**（`60dbbb8`：`prepare_bytes` 在分配前校验区域上限，超限报 E_BUS-09；`map_region` 另有一道闸）。两侧缺一不可，现两侧均在 |
| 8 | 中 | ✅ 闭环 | `ParseLimits` 新增 `hex_size` / `raw_size`；HEX 与 raw 各自使用，不再借用 `elf_size` |
| 9 | 中 | ✅ 闭环 | `append_or_merge` 对空 payload 直接返回；用例 `hex_ignores_zero_length_data_records` |
| 10 | 中 | ✅ 闭环 | `tests_malformed.rs`：20 条表驱动畸形用例（覆盖 brief 要求的 9 类）+ 2 条固定种子 fuzz 用例（随机字节 2000 轮 / 合法镜像变异 2000 轮） |
| 11 | 低 | ⬜ 未修 | 保留以对齐 LLD §3.3 伪码；已在本文档记录为死分支 |
| 12 | 低 | ⬜ 未修 | 防御性代码，保留无害 |
| 13 | 低 | ⬜ 未修 | 按 LLD §6.2（6）口径归 M-13/M-14 运行时校验 |
| 14 | 低 | ⬜ 未修 | MVP 不使用该字段 |
| 15 | 低 | ⚠️ 已加严 | 输出段数新增 `segment_count` 校验（`e_phnum` 的保守性保留） |
| 16 | 低 | ⬜ 未修 | 已在 `ParseLimits` 文档中写明由 M-11 执行 |
| 17 | 低 | ✅ 已修 | 嗅探判据由「首字节是 `:`」收紧为结构判据：记录标记 + 长度 ≥ 11 + 长度为奇数 + 声明字节数与实际数据长度一致 + 标记后全为 hex 数字。首字节恰为 `0x3A` 的 raw 二进制不再被误判为 HEX。判据与 IHEX 解析器自身的校验（`bytes.len() == count + 5`）一致，合法文件仍被识别。3 条用例（raw blob / 4 种畸形记录 / 最短 EOF 记录） |

**统计：阻断 / 高严重度项 100% 闭环**（#1~#4），中严重度 7/7 闭环（#7 的 M-14 侧已由 `60dbbb8` 补齐），低严重度按「设计取舍 / 跨模块 / 收益有限」分流，0 项被遗忘；#17 的嗅探假阳性已由检视 V3 跟进修复。

### 4.1 与 LLD 的偏离登记

| 偏离 | LLD 原文 | 本实现 | 理由 |
| --- | --- | --- | --- |
| 限额字段 | `parseLimit = { elfSize, segmentCount, symbolCount, lineEntryCount, dwarfDepth }` | 增加 `hex_size` / `raw_size` / `total_image_bytes` / `max_region_bytes` | LLD §2.2 的「输入上限」只描述了文件大小维度；D-10 证明「文件大小」不足以约束「镜像大小」。新增字段全部为**加法**，不改变既有字段语义。`dwarfDepth` 未加——M-06 不解析 DWARF |
| 错误码 | E_PROTO-03（畸形）/ E_PROTO-04（超限） | `ErrorFamily::Loader` scenario 1 / 2（+ 3 地址越界） | 契约层已冻结的 `ErrorFamily::Loader` 正是 LLD 两种语义的载体；M-14 报告 §四 亦按此口径 |
| 分层 | M-06 = 前端交互层 | M-06 实现于 Rust 内核，并依赖 M-14 的 `BUS_SIZE` | 见 §1.1；依赖方向仍在 LLD §9.1 允许范围内，分层归属请协调者确认 |

---

## 五、剩余风险与跨模块欠账

| 项 | 状态 | 需要谁 | 说明 |
| --- | --- | --- | --- |
| **D-10 的 M-14 侧** | ✅ **已闭合** | — | M-14 `prepare_bytes` 已加区域大小上限（`60dbbb8`）：分配前校验，超限报 E_BUS-09，`map_region` 另有一道闸。检视 V2 补测发现外设映射路径只受单层防线守护，已由 `0fbd6e7` 补上用例 |
| **`symbol_count` / `line_entry_count`** | ❌ 未闭合 | M-11 | M-06 不解析符号表 / DWARF 行表，两个字段在 M-06 侧不生效。已在 `ParseLimits` 文档中写明归属 |
| **D-23 门禁缺口第 3~4 项** | ❌ 未闭合 | 协调者 | 本轮只补了 M-06 相关的第 2 项（畸形输入表驱动用例）。台账中的其余门禁项不属 M-06 |
| **契约冻结** | ✅ 无诉求 | — | 本轮**未修改** `crates/contract/**`。新增的 `ParseLimits` 字段在 kernel 内部，未触及契约层；M-05 若要下发这些限额，需协调者决定是否进契约 |
| **公开 API 变更** | ⚠️ 需知会 | 协调者 | ① `Loader` trait 的 `load_with_limits` 由「有默认实现」变为「必须实现」（trait 破坏性变更；全代码库仅 `DefaultLoader` 一个实现，`front/` 与 `crates/server/` 均未引用 M-06，无外部影响）② `detect_kind` 未知扩展名的返回值由 `Elf` 改为 `RawHex` ③ `ParseLimits` 新增 4 个字段（均为加法，构造点均用 `..Default::default()`） |
| **嗅探假阳性**（#17） | ✅ 已修 | — | 判据已由「首字节是 `:`」收紧为结构判据（见 §四 #17）。raw 二进制首字节为 `0x3A` 时不再被误判成 HEX |
| **`ET_DYN` 不支持** | ⬜ 已知限制 | — | MVP 只接受 `ET_EXEC`；PIE 固件会被拒（E_LDR-01），错误信息明确 |
| **`e_entry` 不校验**（#13） | ⬜ 已知限制 | — | 入口地址落在段外可加载成功，运行时才暴露 |

---

## 六、审查方法与可复现命令

```bash
cd /Users/zyzs/lczyzs/code/riscv_sim-wt-t3
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace          # 232 passed / 0 failed
```

D-03 的复现输入（修复前会 panic，修复后返回 E_LDR-01）：

```
:01234567
:00000001FF
```

门禁有效性的反向验证（临时把 `hex.rs` 的 `saturating_sub(5)` 改回 `- 5`）：

```
test m06_elf::tests_malformed::malformed_inputs_are_rejected_without_panicking ... FAILED
test m06_elf::tests_malformed::mutated_valid_images_never_panic ... FAILED
panicked at crates/kernel/src/m06_elf/hex.rs:121:21: attempt to subtract with overflow
```

---

*M-06 ELF / Raw / HEX 解析器 · 模块级代码审查报告 · 2026-10-20 · 分支 `feature/m06-loader-hardening`*
