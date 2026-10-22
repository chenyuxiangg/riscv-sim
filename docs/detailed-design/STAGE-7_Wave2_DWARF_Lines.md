# STAGE-7 · Wave 2：DWARF 行表真解析 + 源码高亮（冻结规格）

> **状态**：冻结（v0.22 批的开工依据）
> **基线**：`b443eb5`（v0.21 批合入 main）
> **分支**：`feature/wave2-dwarf-lines`
> **取代**：STAGE-5 规格 §1「Wave 1 / Wave 2 的切分」中 Wave 2 的**范围**部分
> （裁决 F 被本文件**裁决 A** 显式修订，见下）

---

## §0 背景

v0.20 批把 M-11 落地，但 **DWARF 只识别、不解析**：`DwarfLines::resolve` 在
**所有**状态下恒返回 `LineLookup::NoDwarf` ⇒ 契约的 `SourceLocation { file, line }`
**恒为空** ⇒ 调试器停在「我知道这个地址在哪，但说不出它是源码第几行」。

本批把这条补上，并把**前端源码高亮**一起做（用户决策：内核侧 + 客户端一起做）。

### §0.1 三个必须先说清的事实（**全部实测**，不是推演）

| 事实 | 证据 | 后果 |
| --- | --- | --- |
| **本机工具链默认产出 DWARF 5** | `riscv-none-elf-gcc` 15.2.0 编译 `-g` 产物，`readelf --debug-dump=rawline` 显示 `DWARF Version: 5`，且含 `.debug_line_str` 节区 | 只支持 v2~v4 ⇒ **在主力工具链上恒降级** ⇒ 裁决 A |
| **契约层已有完整形状** | 契约 v3 已有 `SourceLocation { addr, file, line }`（STAGE-5 加）与 `LineEntry { addr, file, line }` | **Lane A（DWARF 线）内核侧零契约改动** ⇒ 裁决 D |
| **前端目前没有文件选择器** | 全仓无 `<input type="file">` / `FileReader` / `showDirectoryPicker`；ELF 靠 `LoadProject` 的 `ImageSource::Bytes` 送入 | 源码装载通道要**新建** ⇒ 裁决 E |

### §0.2 顺带带上的欠账

- **D-99**（P1 · 规格↔实现分歧）：M-13 LLD §5.2.1 明写「访存异常 5/7 ⇒ 同步陷入，
  `mtval` 记访存地址」，实现**没有** ⇒ 总线错误原样上抛 ⇒ guest 的 trap handler
  根本没机会跑。实测：**参考侧陷入 cause 7，我们 abort**。
- **D-93 / D-98 / D-100 / D-101**：v0.21 批新登记的四条中低危。

---

## §1 裁决

### 裁决 A：DWARF 版本支持集**扩到 v5**（⚠️ **修订 STAGE-5 裁决 F**）

STAGE-5 裁决 F 冻结的是「只支持 v2/v3/v4，v5 明确降级」。**本批修订它**。

**依据**（实测，见 §0.1）：本机主力工具链默认产出 **DWARF 5**。若坚持 v5 降级，
Wave 2 交付的就是一个「**在主力工具链上永远答不出行号**」的功能 —— 这正是本项目
反复登记的「半成品」形态（D-24「快照 API 看着完整实则不是」、D-38「构建路径」）。

**这不是沉默扩展**：STAGE-5 的理由是「v5 的行表头（目录 / 文件表用
`DW_FORM_line_strp`）与 v2~v4 的行表程序结构不同 ⇒ 明确降级，不猜」。
本批**正面解决**这个差异（裁决 C），而不是放弃 v5。

> ⚠️ 前批冻结裁决被修订时，必须留下「修订依据 + 实测证据」，否则就成了「谁想改就改」。

### 裁决 B：**不解析** `.debug_info` / `.debug_abbrev`；直接解析 `.debug_line` 里的**全部**行号程序单元，按地址归并

**做法**：`.debug_line` 节区里可以有多个行号程序单元（每个 CU 一个）。逐个解析，
把产生的行记录按地址归并成一张表。

**理由**：
1. 行号程序**自带**地址与文件索引 —— 足以回答「地址 → 行」；
2. 解析 `.debug_info` + `.debug_abbrev` 拿到 `DW_AT_stmt_list`（CU → 行表偏移）
   是 **DIEs / 属性 / 缩写表**三套格式的全解析 —— 评审 P-07 判定的「M-11 最大的
   风险项」；
3. 而 `.debug_info` 路线**唯一多出来的收益**是「知道这个地址属于哪个 CU」——
   **我们不需要**（地址 → 行的答案不依赖 CU 归属）。

⇒ **风险从「DWARF 全家桶」降到「一个行号状态机 + 两张表」**。

**代价（必须承认）**：v2~v4 的**目录索引 0** 指的是「编译单元的 `DW_AT_comp_dir`」
（在 `.debug_info` 里）。不解析 `.debug_info` ⇒ 目录索引 0 的文件**只有文件名、没有
目录前缀**。这是**如实可接受**的降级（文件名本身通常足以定位），**不得**假装拿到了
完整路径。

### 裁决 C：v5 的 form 支持边界（**支持什么 / 不支持什么，逐条列明**）

| 用途 | 接受的 form | 不接受的 form |
| --- | --- | --- |
| 路径（目录 / 文件名） | `DW_FORM_string`（内联 NUL 结尾）、`DW_FORM_line_strp`（u32 偏移 → `.debug_line_str`）、`DW_FORM_strp`（u32 偏移 → `.debug_str`） | **`DW_FORM_strx*` / `DW_FORM_strx1..4`**（需要 `.debug_str_offsets` + 编译单元基址） |
| 目录索引 / 时间戳 / 大小 | `DW_FORM_udata`、`DW_FORM_data1/2/4/8`、`DW_FORM_block`（只取长度） | 其余（`data16` 等） |

**遇到不支持的 form ⇒ 整个行号程序单元降级**（不猜、不部分解析）。
降级后该单元**不贡献任何行记录** —— **绝不**产出「一半对一半错」的行号表。

### 裁决 D：契约层**保持 v3，不升版**

**理由**：
1. 契约 v3 已有 `SourceLocation { addr, file, line }`（STAGE-5 加）与
   `LineEntry { addr, file, line }` —— **Wave 2a 的 DWARF 线（Lane A）内核侧
   零契约改动**；
2. 契约层自 v0.10 起冻结，已升过两次（v2、v3），每次都要写升版规格 + 同步 JS 镜像 +
   更新前端门禁 + 改 `wasm-smoke` + 改 `browser-verify`；
3. 「反汇编面板逐条行号」需要给 `DisasmEntryDto` 加字段 ⇒ **登记为 D-102**，
   等契约 v4 一起做（那一批会顺带收 D-47 / D-48 等传输层欠账）。

**本批的契约层改动**（⚠️ **2026-10-22 订正**：原文写「改动量 = 0」，**那是错的**）：
- **不升版**（`schema_version` 仍 **3**）· **不加命令** · **不加事件** ·
  **不加 DTO 字段** · `front/lib/message.js` **未动**
  ⇒ **线格式零变化** ⇒ 前端的契约镜像对拍门禁不需要跟着改；
- **但契约层确实多了两个常量**（Lane C 的 D-93 / D-98 落点）：
  `MSTATUS_WARL_MASK` 与 `MEPC_ALIGN_MASK`（`crates/contract/src/csr.rs`）——
  它们是**架构常量的唯一真相**（M-13 只引用、不复制字面量），**不在线格式里**。

⇒ **裁决 D 的结论不变**（不升版），但「改动量 = 0」这句**假陈述**必须改掉：
**照抄它会让下一批以为契约层一个字节都没动过**。

### 裁决 E：源码投递通道 = **前端本地读，不进内核**

**理由**：源码文本是**调试器客户端的资产**，不是仿真状态。内核持有它会带来三个
无谓代价：契约要加命令、内存要装下整个源码树、快照语义被污染。

**通道**：
- 前端「源码视图」提供两个入口：
  - `<input type="file" webkitdirectory multiple>` —— 选**目录**；
  - `<input type="file" multiple>` —— 选**多个文件**；
- 读成文本 ⇒ 存进 **M-03 视图状态**（**不发**给内核）；
- ⛔ **不新增任何内核命令 / 契约命令**。

**为什么不用 File System Access API**：`showDirectoryPicker()` 只在 Chromium 系
可用，Firefox / Safari 没有 ⇒ 会让「零依赖、跨浏览器」的前端出现一条只在部分浏览器
工作的路径。`<input type="file">` 是**所有**现代浏览器的原生能力，且**零依赖**。

### 裁决 F：源码文件匹配 = **路径尾部逐段匹配，唯一命中才用**

DWARF 里的 `file` 是**编译期路径**（可能是 `../../src/main.c` 或 `main.c`）；
本地源码树的 key 是**相对路径**（如 `src/main.c`）。两者不一定逐字相等。

**规则**：
1. 两边都按 `/` 切段；
2. 从**尾部**逐段比，**全部相同** ⇒ 命中候选；
3. **唯一命中** ⇒ 直接用它；
4. **多个命中** ⇒ **列出候选让用户选**（不静默挑第一个）；
5. **零命中** ⇒ 显示「源码未装载 / 匹配不到」。

**三态必须如实区分**：「没有源码树」/「有源码树但匹配不到」/「匹配到多个」——
**不合并成一个「无源码」**。

### 裁决 G：D-99 的陷阱化口径 —— **只有「未映射访存」转陷阱**

RISC-V 的访问异常：**cause 5 = load access fault**、**cause 7 = store / AMO access fault**。

| 情况 | 处置 |
| --- | --- |
| 访存地址**未映射**（`E_BUS-01`） | ⇒ **同步陷入**：load ⇒ cause **5**，store ⇒ cause **7**；`mtval` = **故障地址** |
| 其它总线错误（如区域过大 = 配置错误） | ⇒ **原样上抛**，不转陷阱 |

⚠️ **load 与 store 必须区分**（5 与 7 是两个数）—— 写死一个就会让一侧与参考侧分歧。
⚠️ 陷入发生时 **PC 不得前进**（`mepc` = 出错指令的地址，guest handler 返回后重执行它）。

### 裁决 H：黄金语料**加两条**「未映射访存」用例（store + load）

- 参考侧**实测**陷入 **cause 7**（store）与 **cause 5**（load）；
- 我们修完后应**同样陷入** ⇒ 两侧判决一致；
- ⚠️ 判据必须是「两侧都陷入、且 `mcause` / `mtval` **逐位相同**」，
  **不是**「都不报错了」—— 后者对「两侧都 abort」也是绿的；
- ⚠️ **必须两条**（**不是**一条）：RISC-V 把访问异常**拆成两个码**
  （load = 5、store = 7），而**一条用例只能观察一个** ⇒ 验收 B-1（store ⇒ 7）
  与 B-2（load ⇒ 5）**各自需要一条**。只加一条就必然有一侧**裸奔**。
  **实测佐证**：把 store 的 cause 从 7 改成 5 的反向变异里，
  `unmapped_store` 变红而 `unmapped_load` **仍然绿** ⇒ 5/7 的分界
  **是可分别观察的**，两条用例缺一不可。

### 裁决 I：Lane C 四条欠账的边界

| ID | 做什么 | 判别式 |
| --- | --- | --- |
| **D-93** | `mret` 取 PC 时把 `mepc` 的**最低 1 位**当作零（**不是**写侧掩码、**不是**低两位） | 「`mret` 之后 PC 的最低 1 位为 0」 |
| **D-98** | `mstatus` 加 **WARL 掩码**（保留位写被忽略） | 「`csrw mstatus, 0xFFFFFFFF` ⇒ 读回的保留位为 0」 |
| **D-100** | `pending_lines()` 改 `Option<Vec<u16>>`；`None` ⇒ `project_mip` **跳过写** | 「`None` 时 `mip` **不被写**」（需一条测试专造争用） |
| **D-101** | ① `restore_full` 走 CSR 表过滤；② M-06 校验 `.symtab` 的 `sh_entsize`（不符即拒收） | ①「恢复含未实现 CSR 的快照 ⇒ guest 读回 0」②「`sh_entsize != 16` ⇒ 拒收」 |

> ⚠️ **D-93 的口径订正（2026-10-22，参考侧实测后）**
>
> 台账 D-93 原文写「RISC-V 要求 `mret` 把 **`mepc[1:0]`** 当作零」。**这个口径对不上本平台**：
> 它是 **IALIGN=32**（无 C 扩展）下的表述，而本平台的 `misa` = `0x40001104`
> **声明支持 C 扩展**（bit 2）⇒ **IALIGN = 16** ⇒ 正确的掩码是 `~0x1`（只清最低 1 位）。
>
> **参考侧实测（Spike，同一个探针 ELF 两侧跑）**：
>
> | 探针 | 测什么 | Spike（参考侧） | 结论 |
> | --- | --- | --- | --- |
> | 探针 1 | 陷入门把哪个地址写进 `mepc`（压缩指令 `c.sw` 位于 `0x8000014a`，低两位 `0b10`） | `tohost = 330` ⇒ `mepc = 0x8000014a`（**保留 bit 1**） | 陷入门**不丢 bit 1** |
> | 探针 3 | `mret` 用什么掩码取 PC（两个压缩 store 落在 `4k` 与 `4k+2`，访问两个不同的未映射地址） | 退出码 **255 = 0xFF** ⇒ 落回 `4k+2` | **`mret` 用 `~1`（IALIGN=16），不是 `~3`** |
> | 探针 4 | **用户** `csrw mepc` 的写被不被掩 | `csrw mepc, 0x1003` 读回 **0x1002** | **用户写按 `~1` 掩**（`mepc` 是 WARL） |
>
> ⇒ **裁决 I 的 C-1 因此明确三条路径各自的语义**：
> ① **用户写 `csrw mepc`** ⇒ 按 IALIGN 掩 **`~0x1`**（契约层 `MEPC_ALIGN_MASK = 0xFFFFFFFE`）；
> ② **陷入门**写 `mepc` ⇒ **不丢 bit 1**（用 `csr_write_raw` 写精确 PC）；
> ③ **`mret` 取 PC** ⇒ 再掩 **`~0x1`**（`mepc_aligned`，幂等）。
>
> ⚠️ **一条不可观测的边界（如实登记）**：① 与 ② 在**合法指令地址上不可区分**
> —— 每条合法指令地址都是偶数（bit 0 恒为 0），`~1` 动不了它 ⇒
> 参考侧在陷入门那条路径上究竟是「写精确 PC」还是「写被掩的 PC」，**无法证明**。
> 平台用 `csr_write_raw` 写精确 PC，是因为它把「陷入门要回到出错的那条指令」
> 这件事**显式**写下来了；这**不是**一条有实测支撑的差异。
>
> ⚠️ **为什么这条值得单独立判据**：写侧掩码（若掩成 `~3`）会把 `mepc` 从
> `0x8000014a` 变成 `0x80000148` —— 与参考侧**逐位不同**，而**黄金语料不覆盖**
> （上游 env 总是 `la` 一个 4 字节对齐地址）⇒ 没有实测就会被固化下来。
> ⇒ 本批新增黄金语料行 `local/compressed_trap` 把这条口径钉在**判决层**。


### 裁决 J：诚实性红线（**贯穿全批**）

1. **v5 未知 form / 畸形节区 / 截断 ⇒ 降级**（`NoDwarf`），
   **绝不**产出「猜出来的行号」；
2. **「查过行表、没命中」⇒ `Missed`**；**「没查」⇒ `NoDwarf`** ——
   两态**不得混**（Wave 1 的最大诚实性资产，Wave 2 **不得**把它弄丢）；
3. **前端三态**（无源码树 / 匹配不到 / 匹配到多个）**不得合并**；
4. **规格里的「实测基线」必须写明「测在哪」** —— v0.21 的主线教训
   （`MPP` 那条探针测在陷阱之外 ⇒ 结论无效）。

---

## §2 验收清单

### 2.1 Lane A · DWARF 行表 + 源码高亮

- [x] **A-1** v2 行表程序能被解析 ⇒ 已知地址给出**正确**的 `file` + `line`
- [x] **A-2** v3 同上
- [x] **A-3** v4 同上
- [x] **A-4** **v5 同上**（用本机工具链 `-g` 产物实测 ⇒ 真实产物，不是合成夹具）
- [x] **A-5** 行号程序**状态机**正确：标准操作码（copy / advance_pc / advance_line /
      set_file / set_column / negate_stmt / const_add_pc / fixed_advance_pc）
      与扩展操作码（end_sequence / set_address / define_file）
- [x] **A-6** **特殊操作码**（adjusted opcode）的地址 / 行增量计算正确
- [x] **A-7** `end_sequence` 之后**不**把行记录延续到下一段（区间不串）
- [x] **A-8** 地址**未命中**任何行记录 ⇒ `Missed`（**不是** `NoDwarf`）
- [x] **A-9** 无 `.debug_line` ⇒ `NoDwarf`
- [x] **A-10** **畸形**输入（截断 / `unit_length` 越界 / 未知 form / 未知 opcode）
      ⇒ **降级**，**不 panic**，**不给**猜出来的行号
- [x] **A-11** v5 的 `DW_FORM_line_strp` 字符串**真的**从 `.debug_line_str` 取到
- [x] **A-12** `DW_FORM_strx*` 出现 ⇒ **整个单元降级**（不部分解析）
- [x] **A-13** **多个行号程序单元**（多个 CU）⇒ 归并成一张表，**都能查到**
- [x] **A-14** 宿主 `ResolveLine` 命令返回**真**的 `file` + `line`（契约形状不变）
- [x] **A-15** 前端源码视图：**装载目录 / 装载多文件** ⇒ 源码树进 M-03 视图状态
- [x] **A-16** 前端：**当前 PC ⇒ `ResolveLine` ⇒ 高亮对应行**
- [x] **A-17** 前端：**路径尾部逐段匹配**命中唯一文件 ⇒ 正确高亮
- [x] **A-18** 前端：**多个候选** ⇒ 列出候选，**不**静默挑一个
- [x] **A-19** 前端：**零命中 / 无源码树** ⇒ 如实显示，**不**显示错误的行号
- [x] **A-20** 前端：**源码不进内核**（无新增契约命令；`LoadProject` 后源码树仍在）

### 2.2 Lane B · D-99 访存异常陷阱化

- [x] **B-1** store 触碰**未映射地址** ⇒ 陷入 cause **7**，`mtval` = 故障地址
- [x] **B-2** load 触碰**未映射地址** ⇒ 陷入 cause **5**，`mtval` = 故障地址
- [x] **B-3** 陷入时 **PC 不前进**（`mepc` = 出错指令地址）
- [x] **B-4** 其它总线错误（如区域过大）⇒ **仍原样上抛**，不转陷阱
- [x] **B-5** 黄金语料**新用例**：两侧**都陷入**且 `mcause` / `mtval` **逐位相同**
- [x] **B-6** 黄金对拍语料 **64 → 67 例**仍**全绿**
  > ⚠️ **本条订正（2026-10-22，V-C 检视 F-09）**：原文写「64 → 66」，而**实际是
  > 64 → 67** —— 66 是 Lane B 合并时的数（`846cec0`），之后统筹方又补了
  > `compressed_trap` 一条（`8192c87`）⇒ **67**。
  > 原因是 RISC-V 把访问异常**拆成两个码**（load = 5、store = 7），而
  > **一条用例只能观察一个** ⇒ 验收 B-1（store ⇒ 7）与 B-2（load ⇒ 5）
  > **各自需要一条**。只加一条就必然有一侧（5/7 的另一半）**裸奔**。
  > 这正是 v0.19 批「K-38~K-42：回报判据在六条 CSR 指令臂上各写了一遍，
  > 曾经只有一条臂有反向证明」的同一类教训。
  > **实测佐证**：把 store 的 cause 从 7 改成 5 的反向变异里，
  > `unmapped_store` 变红而 `unmapped_load` **仍然绿** ⇒ 5/7 的分界
  > **是可分别观察的**，两条用例缺一不可。


### 2.3 Lane C · 四条欠账

- [x] **C-1** `mret` 后 PC 的**最低 1 位**为 0（**IALIGN = 16** ⇒ 掩 `~1`；**不是**低两位）
  > ⚠️ **本条订正（2026-10-22，V-C 检视 F-07）**：原文写「**低两位**为 0」，那是
  > **IALIGN = 32**（无 C 扩展）的口径。平台 `misa = 0x4000_1104` **声明支持
  > C 扩展** ⇒ **IALIGN = 16** ⇒ 只清**最低 1 位**。裁决 I 的表**早已**改成
  > 最低 1 位，**只有**这条验收文本漏改 ⇒ 两条口径**互相矛盾**会让验收判定
  > 自相冲突。实测（Spike 1.1.1-dev）：`mret` 落回 `4k+2` ⇒ 掩 `~1`，**不是** `~3`。
- [x] **C-2** `mstatus` 保留位写被忽略（读回保留位为 0）
- [x] **C-3** `pending_lines()` 争用 ⇒ `None` ⇒ `mip` **不被写**
- [x] **C-4** 恢复含未实现 CSR 的快照 ⇒ guest 读回 **0**
- [x] **C-5** `sh_entsize != 16` ⇒ **拒收**


> **勾选说明（2026-10-22 · 收口轮）**：31 条验收**全部勾选**。
> ⚠️ **勾选不等于「合上」**——每条的**反向变异红集**（证明改坏实现时它真的变红）
> 逐条记在 `docs/reviews/REVIEWS.md`：
> **§3.13.5**（收口方自己的前端修复六条）· **§3.13.5.1**（Lane W1 · M-11 DWARF 五条）·
> **§3.13.5.2**（Lane W3 · 真机复验两条）· **§3.13.5.3**（Lane W2 · M-12 引擎八条）。
>
> ⚠️ 其中 **A-16 / A-19 / A-20** 三条**在检视轮之前是「名义落地、实质没判别力」**：
> A-16 的用例**手工**往视图状态里塞 `sim.pc`，绕开了生产接线（源码视图**永远**拿不到 PC ⇒
> 按钮恒问地址 0）；A-20 的用例传的是**空壳 stub**，生产里真正动源码树的那段代码**从未执行**；
> A-19 把内核的两种「没有答案」抹平成「尚未解析」。⇒ 三条都在本批**修好并逐条做过反向变异**
> （红集见 §3.13.5）**之后**才勾选。
>
> ⚠️ **三条修复 lane 各自又带出了新的验收证据**（不只是「修已有的中危」）：
> Lane W1 补的 **v5 / v2~v4 文件索引基址**两条用例钉住了 **A-1 ~ A-4** 背后的规则
> （变异 M03 / M04 的红集见 §3.13.5.1）；Lane W2 的 **VD-F1** 修法让
> **A-14（宿主 `ResolveLine` 返回真 `file` + `line`）**那条路**多了一条「引擎自己产出的快照必须过契约自己的校验器」的旁证
> （变异 D 的红集见 §3.13.5.3）；Lane W3 给 **A-16** 补上了**真机侧**的值断言
>（`data-addr` **等于**当前 PC 的具体十进制值，变异见 §3.13.5.2）。
>
> ⚠️ **全批门禁（8 项）在三条修复 lane 合入后重跑过一遍，失败项数 0**
>（`cargo fmt` / `cargo clippy -D warnings` / `cargo test` 1 083 全绿 / `check-front.sh` 245 用例 41 suite /
> `golden-run.sh` 两条 / `build-wasm.sh` + `wasm-smoke.mjs` / `browser-verify.mjs` 真 Chrome）。

### 2.4 全批门禁（**8 项，串行**）

`cargo fmt --all -- --check` · `cargo clippy --workspace --all-targets -- -D warnings` ·
`cargo test --workspace --no-fail-fast --lib --tests` ·
`tools/check-front.sh` · `tools/golden-run.sh --self-check` + `--keep-going` ·
`tools/build-wasm.sh` + `node tools/wasm-smoke.mjs` · `node tools/browser-verify.mjs`

---

## §3 边界：明确**不做**的事

| 不做 | 理由 |
| --- | --- |
| 契约层升 v4 / `DisasmEntryDto` 加行号字段 | 裁决 D ⇒ 登记 **D-102** |
| 解析 `.debug_info` / `.debug_abbrev` / DIE / 缩写表 | 裁决 B |
| `DW_FORM_strx*` 支持 | 裁决 C |
| 源码文本进内核 / 进契约 | 裁决 E |
| 「反汇编面板逐条行号」 | 裁决 D（等 D-102） |
| **取指侧**的未映射访问（cause 1，instruction access fault） | 裁决 G 只覆盖**数据**访问（load / store）。取指侧的未映射访问仍**原样上抛为总线错误** ⇒ 与参考侧有分歧，登记为 **D-103** |
| S 扩展 / PMP / 计数器 | **D-94**，工作量等于实现半个特权级，不搭车 |

---

## §4 文件所有权（**并行切分依据**）

> 铁律：lane **可并行写**，**验证必须串行**；并行 lane 各自**独立 `CARGO_TARGET_DIR`**。

| Lane | 拥有的文件 |
| --- | --- |
| **A** · DWARF + 前端源码视图 | `crates/kernel/src/m11_symbol/dwarf.rs`、`crates/kernel/src/m11_symbol/mod.rs`、`crates/kernel/src/kernel_host/mod.rs`、`crates/kernel/src/kernel_host/tests_m11.rs`、`front/views/components/source-view.js`（新）、`front/views/m01_workbench.js`、`front/views/m02_viewcomponents.js`、`front/views/m03_viewstore.js`、`front/index.html`、`front/tests/**` |
| **B** · D-99 | `crates/kernel/src/m13_iss/executor.rs`、`crates/kernel/src/m13_iss/mod.rs`、`crates/kernel/src/bin/riscv-golden.rs`、`tests/golden/**` |
| **C** · D-93/98/100/101 | `crates/kernel/src/m13_iss/csr.rs`、`crates/kernel/src/m12_engine/mod.rs`、`crates/kernel/src/m16_interrupt/mod.rs`、`crates/kernel/src/m17_backend/mod.rs`、`crates/contract/src/csr.rs`、`crates/kernel/src/m11_symbol/symbols.rs` |

**零交集自检**：A∩B = ∅ · A∩C = ∅（`m11_symbol/mod.rs` vs `m11_symbol/symbols.rs` 是**不同文件**）·
B∩C = ∅（`m13_iss/mod.rs` + `executor.rs` vs `m13_iss/csr.rs` 是**不同文件**）。

⚠️ **Lane A 不得枚举 `TrapCause` 的变体**（Lane B 会加两个 ⇒ 会编译不过）。
⚠️ **Lane B 不得改 `m13_iss/csr.rs`**（Lane C 拥有它）。
