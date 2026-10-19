# M-14 总线与内存映射 · 代码审查报告

> **审查对象**：`crates/kernel/src/m14_bus/mod.rs`（800 行）+ `crates/kernel/src/m14_bus/tests.rs`（259 行）
> **审查基准**：`docs/detailed-design/M-14_BusMemoryMap.html`（LLD §3.2 评审项 14-1 ~ 14-12）、`docs/progress/PROGRESS.md` v0.5
> **审查日期**：2026-10-19
> **审查方式**：全文件通读 + 调用方交叉核对（M-06 / M-13 / M-17）+ 测试盲区盘点
> **门禁状态**：`cargo test --workspace` 47 passed / 0 failed；`clippy -D warnings` 干净；`fmt --check` 干净
> **结论**：核心设计（两阶段写、原子失败、BTreeMap 区间表）正确，**未发现阻断性正确性缺陷**；发现 5 项需在 M-15 接入前处理的契约/统计问题 + 13 项整洁与健壮性问题。三条现有门禁对下表全部无感知。

---

## 一、问题汇总表

| # | 严重度 | 位置 | 问题 | 证据 / 影响 | 建议修复 |
| --- | --- | --- | --- | --- | --- |
| 1 | **高** | `mod.rs:119-121` | `BusAccess.width` 文档称「跨区间拆分**按字节切片**逐个触发 hook」，与实现相反 | 实现仅在 `read`/`write` 入口调一次 `fire_hook`（`:503`、`:564`），拆分循环内无 hook；`tests.rs:223` 明确断言 `"one event per dispatch, not per byte"`。**文档与实现、测试三方中两方一致，文档是错的**。M-18 边沿编码若照此文档实现会与实际行为不符 | 修正 `BusAccess.width` 文档为「每次访问触发一次，跨区间拆分不逐字节上报」；若 M-18 确需逐字节边沿，则改实现并同步改 `tests.rs:223` |
| 2 | **高** | `mod.rs:503`、`:564` | hook 在访问合法性校验**之前**触发，失败的访问也会上报 | 未映射地址、ROM 被丢弃的写都会产生 `BusAccess` 事件。波形/trace 消费者会把不存在的访问当作真实发生过 | 移到校验通过后再 `fire_hook`，或在 `BusAccess` 增加 `ok: bool` / `err_scenario` 字段由消费者自行过滤（推荐后者，信息更完整） |
| 3 | **高** | `mod.rs:527`、`:587`、`:637` | `ram_hits` 对 **ROM 读**和**被静默丢弃的 ROM 写**同样 +1 | 字段名承诺「landings on RAM regions」（`:101`），实际 ROM 操作也计入。`tests.rs:241-244` 固化了该宽松口径（1 读 + 1 写 = 2）。任何消费该计数的面板会显示错误数据 | 按 `RegionType` 分派：`Ram` → `ram_hits`，`Rom` → 新增 `rom_hits` 或不计，`Peripheral` → `periph_hits`。修正后需同步改 `tests.rs:241` |
| 4 | **中** | `mod.rs:104` | `periph_hits` 是死字段，全代码库零写入点 | `grep periph_hits` 仅命中字段声明处。M-15 接入前该字段永远为 0，若有前端消费会恒显示「无外设命中」 | M-15 接线时补上；或在接线前删除字段，避免暴露永不更新的统计项 |
| 5 | **中** | `mod.rs:531`、`:615` | `unaligned_splits` 在「跨区间」与「任一侧未映射」两种情况下都 +1 | `crosses_region`（`:748-761`）的 `_ => true` 分支把未映射也判为跨界。未映射访问与 unaligned 访问是完全不同的语义，混在一个计数器里 | 拆分 `crosses_region` 返回原因（`Crossing` / `Unmapped` / `Aligned`），分别计入不同计数 |
| 6 | **中** | `mod.rs:356`；配合 `m06_elf/elf.rs:279` | 分配无上限：`prepare_bytes` 执行 `vec![0u8; size as usize]`，`size` 最大可达 ~4 GiB | `map_region` 只校验 `base + size ≤ BUS_SIZE`（4 GiB）。一个声明 4 GiB 段的调用即可申请 4 GiB。浏览器 WASM 内即 OOM → worker 崩溃。M-06 侧 `ParseLimits` 同样无 `total_bytes` 字段，两者叠加使畸形固件可撬动任意大分配 | `ParseLimits` 增加 `total_image_bytes`（如 64 MiB）与 `max_region_bytes`（如 16 MiB）；`Bus` 增加 `max_region_bytes` 构造参数，超限返回 E_BUS-04 |
| 7 | **中** | `mod.rs:340-359` | `init.len() > size` 复用 E_BUS-04（overflow） | 「初始化数据比区域还长」与「base+size 溢出」是完全不同的错误，共用 scenario 4 会让上层无法区分，排查困难 | 分配独立 scenario（E_BUS-07 init-too-large），或在 LLD §3.4 错误码表中补充说明 |
| 8 | **中** | `mod.rs:780-784` | `validate_width` 恒返回 `Ok`，无任何实际校验 | 死函数。字节序宽度合法性本应在派发点拦截，目前完全依赖 `BusWidth` 枚举本身的封闭性 | 删除该函数；或实现真实校验（如未来加入对齐要求 `addr % width == 0`）并在文档注明 |
| 9 | **中** | `mod.rs:165-203`（**跨模块**：波及 M-06 加载路径 + 前端内存映射表） | `Region` / `RamBackend` / `RomBackend` 字段全部 `pub` | 外部持有 `&mut Region` 即可直接改 `bytes`，**绕过 stats 计数与 trace hook**，破坏 M-14 的可观测性保证。违反 `AGENTS.md`「不在 contract 暴露的内部状态用 `pub(crate)`」。**跨模块影响**：`crates/kernel/tests/m06_load_to_bus.rs` 与 `front/views/m02_viewcomponents.js` 的 `renderMapTable` 都消费 region 字段，收紧可见性需同步调整这两处的取数方式；M-06 的加载路径若要预置 ROM 内容也需改走 `map_rom(base, size, init)` 而非事后写 `bytes` | `bytes` / `target` 改 `pub(crate)` 或私有，对外只暴露 `base` / `size` / `kind` 只读访问器；`regions()` 返回迭代器时给出只读视图 || 10 | 低 | `mod.rs:425-437` | `assert_no_overlap` 的 belt-and-braces 分支恒不可达，注释自认（`:427`） | `prev.contains(base)` 为真已蕴含 `prev_end > base`，第二个分支永不可达；`checked_add` 返回 `None` 时静默跳过检查 | 删除该分支（`:425-437`），减少误导性代码 |
| 11 | 低 | `mod.rs:731-746` | `read_bytes` / `write_bytes` 用 `unwrap_or(0)` / `if let Some` 静默吞掉越界 | 与同文件 `read_region_byte`（`:665-679`）的报错行为不一致。当前路径上不可达（`crosses_region` 保证多字节访问两端同区），但属隐患：将来放宽派发条件即变成静默数据损坏 | 统一改为返回 `Result` 并 fail-fast，与 `read_region_byte` 口径一致 |
| 12 | 低 | `mod.rs:213-215`、`:748-761` | `contains` 与 `crosses_region` 均使用 `wrapping_add` | 地址回绕。当前安全：`map_region` 的 `checked_add` 使 `0xFFFF_FFFF` 永远不可被映射，回绕后的多字节访问必在两阶段预检的 `0xFFFF_FFFF` 字节上失败为 E_BUS-01。**安全是偶然的，不是显式的** | 显式拒绝 `addr + width > BUS_SIZE` 的访问，返回 E_BUS-04，不依赖回绕行为 |
| 13 | 低 | `mod.rs:325-337`、`:649-726` | `map_peripheral` 是公开 API 但 Peripheral 区域读写恒返回 E_BUS-05「not wired」，且**零测试覆盖** | 测试文件无任何 `map_peripheral` 调用。公开 API 在无测试、无文档说明未接线状态的情况下暴露 | 接线前将 `map_peripheral` 标记 `#[cfg]` 门控或标 `unstable`，或补一条「当前恒返回 E_BUS-05」的契约测试锁定行为 |
| 14 | 低 | `tests.rs:140-149` | `read_past_end_of_region_yields_e_bus_01_or_04` 断言放宽为「1 或 4」 | 该访问实际**恒**走拆分路径返回 1（`lookup(0x5010)` 为 `None` → `_ => true`）。断言放宽说明写测试时对预期行为不确定，等于没有回归保护 | 收紧为 `assert_eq!(err.scenario, 1)`，并补一条 `Byte` 宽度越界的对照用例 |
| 15 | 低 | `tests.rs` 全文 | 测试盲区：`unaligned_splits`、`periph_hits`、`map_peripheral`、peripheral E_BUS-05、`init` 超长（`:344` 分支）、`init` 短于 `size` 时的零填充、`set_hook(None)` 清除 hook、地址回绕边界 —— 均无测试。其中 `Region::end_inclusive`（`:208`）经全工作区检索**零调用**，是既无测试也无消费者的死公开 API | 这些正是上表中 #4、#5、#7、#13 所在代码路径。现有 16 条测试覆盖的是 happy path 与既有错误码 | 逐条补齐；建议优先补 #7 与 #13 两类（当前完全无保护）。`end_inclusive` 若 P1 分页表用不到可直接删除 |
| 16 | 低 | `mod.rs:786-796` | 文件末尾的测试清单注释与实际测试内容不符 | 注释列举了「stats accounting」，但未提及 `unmap` 幂等、`BusMaster` trait 派发、ROM 跨区间写跳过等实际存在的用例 | 同步注释，或改为指向 `tests.rs` 的模块级 doc |

---

## 二、值得肯定的设计（评审 14-x 落地情况）

| 评审项 | 设计点 | 落地情况 |
| --- | --- | --- |
| 14-1 | 跨区间写两阶段（预检 → 提交） | ✅ `mod.rs:590-638`，`tests.rs:179` 验证了「预检失败时整笔不生效」，原子性有回归保护 |
| 14-3 | map 失败保留旧配置 | ✅ `map_region` 全部校验在 `insert` 之前，`tests.rs:43-44` 有断言 |
| 14-4 | ROM 写静默丢弃 | ✅ `write_region` / `write_region_byte` 的 `Rom` 分支，含跨区间场景 `tests.rs:190` |
| 14-12 | `size == 0` 在 map 期拒绝 | ✅ `tests.rs:29`，E_BUS-06 |
| — | 区间表选型 | ✅ `BTreeMap<base, Region>` + `range()` 三向重叠判定，O(log n)，决策记录见 PROGRESS §7 |
| — | `BusHook` 不带 `Send` bound | ✅ MVP 单线程合理，且文档明确标注了 P2 SMP 时的升级路径 |

---

## 三、建议处理顺序

1. **M-15 接线前必做**：#1（文档纠偏，避免 M-18 照错文档实现）、#2、#3、#4 —— 这四项直接决定 M-18 观测管道的数据正确性，一旦 M-18 按错误契约实现，返工面覆盖整个波形链路。
2. **M-05 暴露 BusStats 前必做**：#3、#4、#5 —— 统计口径会经协议层固化给前端，改动成本随时间上升。
3. **健壮性**：#6、#7、#9 —— 与 M-06 的 `ParseLimits` 一起做，#6 是两个模块的联合改动，单改 M-14 不完整。
4. **整洁项**：#8、#10、#11、#12、#14、#15、#16 —— 可随时做，建议合并成一次 `refactor(m14): 清理死代码与统计口径` 提交。

---

## 四、闭环核对（2026-10-19 复查）

代码规模 800 → **951 行**；M-14 测试 16 → **27 条**；`cargo test --workspace` **74 passed / 0 failed**；`clippy -D warnings` 与 `fmt --check` 均干净。**所有会影响可观测数据正确性的项（#1~#5）已全部闭环**，且实现质量高于报告建议：`fire_hook` 采纳的是「加 `ok` / `err_scenario` 字段」而非「移到校验后」，信息更完整；`BusHook` 的文档直接引用本报告编号说明旧措辞为何是错的。统计口径重构为 `bump_read_hit` / `bump_write_hit` 按 `RegionType` 分派，ROM 写显式不计数并写清了理由。

| # | 原严重度 | 状态 | 落地证据 |
| --- | --- | --- | --- |
| 1 | 高 | ✅ 闭环 | `BusHook` 文档改写并**显式引用本报告 #1/#2 说明旧措辞之误**（`mod.rs:131-136`）；`BusAccess.width` 同步改为「跨区间时 `width` 仍报原始宽度」（`:143-145`） |
| 2 | 高 | ✅ 闭环 | `BusAccess` 新增 `ok: bool` 与 `err_scenario: Option<u16>`（`:160-163`），`fire_hook` 签名随之改为四参（`:556`、`:648`）。新增 `hook_carries_ok_flag_for_failed_accesses` + `hook_succeed_entry_has_ok_true` |
| 3 | 高 | ✅ 闭环 | 新增 `rom_hits` 字段 + `bump_read_hit`/`bump_write_hit` 按 `RegionType` 分派（`:757-774`），ROM 写显式不计数并注明评审 14-4 依据。测试 `stats_split_between_ram_rom_periph_hits` |
| 4 | 中 | ✅ 闭环 | `periph_hits` 已在 `bump_*_hit` 中接线（`:761`、`:772`），字段文档明确说明「M-15 接线前恒为 0」并引用本报告 #13 |
| 5 | 中 | ✅ 闭环 | 新增 `unmapped_splits` 字段，`crosses_region` 改为返回三值分类 `Outcome`（`:915-916`），拆分路径分别计数（`:566`、`:570`、`:657`、`:661`） |
| 6 | 中 | ❌ **未闭环** | `map_ram`/`map_rom`/`map_peripheral` 签名不变，无 `MAX_REGION` 类常量，`prepare_bytes` 仍是裸 `vec![0u8; size as usize]`（`:394`）。该项本就需与 M-06 的 `ParseLimits` 联合改动（`total_image_bytes` / `max_region_bytes`），M-06 侧未动 |
| 7 | 中 | ✅ 闭环 | 新增 **E_BUS-07**「init 超长」独立错误码（`:378-393`），不再复用 E_BUS-04。测试 `map_rejects_init_too_large_with_e_bus_07` |
| 8 | 中 | ✅ 闭环 | `validate_width` 已删除，并在文件末尾留下说明性注释记录删除理由（闭合枚举无需运行时校验） |
| 9 | 中 | ❌ **未闭环** | `Region.target` 仍为 `pub`，`RamBackend.bytes` 仍为 `pub Vec<u8>`（`:200` 附近）。外部持有 `&mut Region` 仍可绕过 stats 计数与 trace hook 直接改内存 |
| 10 | 低 | ✅ 闭环 | `assert_no_overlap` 现仅保留 `prev.contains(base)` 一处判定（`:462`），belt-and-braces 分支已整体删除 |
| 11 | 低 | ✅ 闭环 | `read_bytes` 改为返回 `Result<u32, PlatformError>`（`:866`），`write_bytes` 同步改为 `Result`；越界不再静默吞为 0 或丢弃，与 `read_region_byte` 口径一致 |
| 12 | 低 | ✅ 闭环 | `Region::contains` 改用 `checked_add` 并附详细理由注释（`:244-258`，说明为何旧 `wrapping_add` 会在边界处误匹配）；`crosses_region` 同样改 `checked_add`，溢出显式路由到 `Crossing::Unmapped`（`:900-914`）。地址回绕从「偶然安全」变为**显式拒绝** |
| 13 | 低 | ✅ 闭环 | 新增 `peripheral_read_returns_e_bus_05_until_m15_wired` + write 版，**正面锁定「M-15 未接线」契约**，并在注释中写明「M-15 落地后本测试改为期望成功派发并校验 `periph_hits` 递增」 |
| 14 | 低 | ✅ 闭环 | 原「1 或 4」用例拆为两条并全部收紧：Word 读取与 Byte 读取现在都是 `assert_eq!(err.scenario, 1, …)`（`tests.rs:169`），放宽断言已彻底移除 |
| 15 | 低 | ✅ 大幅闭环 | 测试 16 → 27 条，新增覆盖：hook ok 标志、统计三分、peripheral E_BUS-05、E_BUS-07、Byte 越界。`end_inclusive`（零调用死公开 API）已删除 |
| 16 | 低 | ✅ 闭环 | 文件末尾「Tests cover」清单已重写，逐条标注对应 review 编号（`#2` / `#3` / `#7` / `#13`），并把指针改为「see `tests.rs`」 |

**统计：模块内部项 100% 闭环**（#1~#5 行为项 + #7 错误码 + #8 死函数 + #10~#12 fail-fast/回绕 + #13 测试契约 + #14 断言 + #15 覆盖率 + #16 清单）。**仅剩跨模块的 #6（与 M-06 联合的分配上限，上收为台账 D-10）与 #9（字段可见性，上收为台账 D-13）未闭环**，按约定暂不处理。

未闭环的 2 项均为跨模块：#6 需与 M-06 的 `ParseLimits` 联合改动（上收为台账 D-10），#9 需同步调整 M-06 集成测试与前端 `renderMapTable`（上收为台账 D-13）。**模块内部可独立闭环的条目已全部清零。**

### 复查中新发现的遗留项

| # | 严重度 | 位置 | 问题 | 状态 |
| --- | --- | --- | --- | --- |
| N5 | 低 | `mod.rs` 末尾「Top-level error helpers」段 | `validate_width` 删除后，该段落下**只剩注释、没有函数**的孤立分节横幅 | ✅ 已闭环（横幅随删除一并移除） |
| N6 | 低 | `tests.rs` Byte 越界用例 | 恒返回 scenario 1，`\|\| scenario == 4` 的放宽断言无存在必要（#14 遗留） | ✅ 已闭环（随 #14 一并收紧为 `assert_eq!(err.scenario, 1, "byte past end must be E_BUS-01")`） |

---

## 五、审查边界声明

- 本报告仅覆盖 M-14 模块自身与其直接调用方（M-06 的 `BUS_SIZE` 引用、M-13 的 `step_one_via_bus`、M-17 的 `BusMaster` 绑定）。
- 未审查：M-13 ISS 指令语义、M-06 解析器内部逻辑、M-12 调度器、前端 M-01/02/03 —— 这些的问题在项目级全量审查中另行记录。M-15 的审查见 `docs/reviews/M-15_PeripheralModel_Review.md`。
- 门禁结论为**实际执行结果**（2026-10-19，`cargo test --workspace` / `clippy` / `fmt`），非引用文档声明。§四 的闭环核对同为复查当日实测。
