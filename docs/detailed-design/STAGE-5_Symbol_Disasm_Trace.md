# STAGE-5 · M-11 符号表 / 反汇编 / trace 导出 —— 落地规格（冻结）

> **状态**：**冻结**（本批的契约与裁决以本文为准）
> **起草**：v0.20 批（承接 v0.19 的 M-16 批做法：规格先行 → 分 lane 施工 → 两路检视 → 变异自检 → 收口）
> **依据**：`docs/detailed-design/M-11_SymbolDwarfTrace.html`（LLD v1.1）· `docs/reviews/REVIEWS.md`（台账 §2 / §3.10）· `docs/architecture/RISC-V_Architecture_Design.html`
> **契约层**：`crates/contract/src/`（`schema_version` 本批 **2 → 3**，见裁决 E）

---

## 0. 本批要解决的问题

M-11 自 v0.10 起是 **53 行 stub**（只有 `SymbolService` 端口 + `TraceFormat` 枚举），是 23 个模块里**唯一从未开工**的「PO 主承担模块」（LLD §1.1：MVP 必需）。台账里已经有**两条债**在等它：

| 台账 | 内容 | 本批如何闭环 |
| --- | --- | --- |
| **D-50** | M-19 的环形缓冲在生产路径上「只写不读」——边沿进了环，但**没有任何人读这个环**；M-19 自带的 `export_csv` / `export_vcd` **没接进契约** | 裁决 D：新增导出命令，**波形导出的数据源改为 M-19 的环**（不是 M-18 的边沿存储） |
| **D-51** | M-18 的访存 trace 环在宿主里**从不被 drain**——每条访存都进环，常驻 3~5 MB，**边沿之外的数据全部白攒** | 裁决 D：访存 trace **接到 M-11 的 traceBuffer**（导出数据源），环不再无消费者 |

另有 **🟡-4** 记着「`riscv-golden` 里有一套**第二套 ELF 符号读取器**（`find_symbol`），作者自披露「M-11 落地后应切换过去」」——本批的符号表能力落地后，golden runner 可以切到 M-11（**P1 项，不在本批强制**）。

---

## 1. 范围与切分

LLD 给了 M-11 四项职责（符号表 / DWARF 行表 / 反汇编 / trace 导出），评审 P-07 明确「DWARF 解析是 **10 人日最大风险项**」。本批按**风险从低到高**切成两波：

### Wave 1（本批 v0.20 的主体）

| 编号 | 内容 | 主要文件 |
| --- | --- | --- |
| **W1.1** | **M-06 带出调试节区**：解析 ELF **节区表**，按白名单提取 `.symtab` / `.strtab` / `.debug_line` / `.debug_line_str` / `.debug_str` / `.debug_abbrev` / `.debug_info` | `crates/kernel/src/m06_elf/` |
| **W1.2** | **M-11 符号表**：解析 `.symtab` + `.strtab` ⇒ `lookup_symbol` / `symbol_at` | `crates/kernel/src/m11_symbol/` |
| **W1.3** | **M-11 反汇编**：复用 M-13 的 `Decoder`（裁决 A）+ **自写 ISA 助记符渲染器**（裁决 C）+ 按地址缓存 + 自修改代码失效（裁决 G） | `crates/kernel/src/m11_symbol/` |
| **W1.4** | **契约层**：`schema_version = 3` + 4 条新命令 + 3 个新事件 + 2 个新 DTO（裁决 E）+ 前端 JS 镜像同步 | `crates/contract/src/` · `front/lib/message.js` |
| **W1.5** | **宿主接线**：4 条命令 → 3 个事件的规范映射（形状见 §3.3） | `crates/kernel/src/kernel_host/` |
| **W1.6** | **导出接线**：M-18 的访存 trace → M-11 的 `traceBuffer`；导出命令按 `source` 路由到 M-19 或 M-11（裁决 D）⇒ **闭环 D-50 + D-51** | `crates/kernel/src/kernel_host/` · `crates/kernel/src/m11_symbol/` |
| **W1.7** | **前端**：`view.debug` 增加**反汇编面板**（PC 高亮 V-12）+ **导出**入口 | `front/views/` · `front/tests/` |
| **W1.8** | 端到端 + 两路独立检视 + 变异自检 + 收口 | — |

### Wave 2（本批后半 / 下一批 v0.21）

| 编号 | 内容 |
| --- | --- |
| **W2.1** | **M-11 DWARF 行表**：解析 `.debug_line`（**v2 / v3 / v4**，裁决 F）⇒ `resolve_line`（两态区分：无 DWARF `{NULL,0}` vs 未命中 `{NULL,0xFFFFFFFF}`，LLD 11-4） |
| **W2.2** | **前端源码高亮**：单步 / 断点暂停后按 `resolve_line` 结果高亮源码行 |

> ⚠️ **契约一次定型**：Wave 1 就把 `ResolveLine` 命令与对应事件加进契约（Wave 1 时**恒返回降级态**），Wave 2 **只改 M-11 内部实现** ⇒ `schema_version` 本批**只升一次**（2 → 3），Wave 2 不再升。

### 明确不做（本批 out of scope）

- **伪指令别名**（`mv` / `li` / `ret` / `j`）——裁决 C 排除。
- **DWARF v5** 行表（`.debug_line_str` + v5 行表头）——裁决 F 明确降级。
- **`.debug_info` / DWARF 表达式 / 变量值读取**（GDB 式的变量检查）——不做。
- **trace 压缩 / 持久化**（LLD 预留 P2）。
- **JSON 导出的 schema 版本化**（LLD 预留 P2）。
- **源码路径归一缓存**（LLD 预留 P2，跨 ELF 会话复用）。
- **`riscv-golden` 切换到 M-11 的符号查询**（🟡-4 的迁移，P1）。

---

## 2. 裁决（覆盖 LLD 措辞，须先改 `docs/`）

> 以下每条都**与 M-11 LLD v1.1 的现有措辞冲突或补充**。按项目纪律「模块边界 / ADR 变更先改 `docs/`」，本节落地时**同步订正** `M-11_SymbolDwarfTrace.html` 的对应小节。

### 裁决 A · 反汇编复用 M-13 的解码器（**零第二套解码**）

- **现状**：LLD §3.1 写「内置 RV32IMC opcode→助记符映射表」；§3.4 写「M-17 readMem 取指 + **独立** RV32IMC 解码映射表」，并设「**一致性测试门禁**（11-5）」防两套解码语义漂移。
- **实际**：M-13 已有**完整** RV32IMC 解码器（`crates/kernel/src/m13_iss/decoder.rs` 的 `Decoder::decode(raw, pc) -> Decoded`，含 RV32I 41 条 + RV32M 8 条 + RV32C 26 条），`Decoded` 是**结构化字段**（opcode / funct3 / funct7 / rd / rs1 / rs2 / imm / raw / pc / len / compressed / compressed_raw / illegal）。
- **裁决**：**M-11 复用 M-13 的 `Decoder`**（唯一解码真相），M-11 只负责**把 `Decoded` 渲染成文本**。
- **理由**：LLD 担心的「两套解码语义漂移」**结构上不存在**——比「独立表 + 门禁去防」更强（门禁只能发现漂移，不能防止）。
- **对 LLD 的订正**：§3.1 伪代码里的 `decode_mnemonic(code, width)` 改为「`m13_decode(raw, pc) -> Decoded` → `render_mnemonic(&Decoded) -> String`」；§3.4 的「独立表 + 一致性测试门禁」两列**删除**，替换为「**唯一解码真相 = M-13**（漂移风险结构上不存在）」；§7.2(2)「解码表驱动」改为「**解码复用 M-13 + 渲染表独立**（新增指令只动 M-13，渲染表只动 M-11 的文本映射）」。

### 裁决 B · M-06 带出**调试节区原始字节**，M-11 负责解析

- **现状冲突**：LLD §1.1 / §2.3 / §5.2 / §9.1.1 说「M-06 loadELF **下发符号表 + DWARF 行表**」；而 `crates/kernel/src/m06_elf/mod.rs` 的 `ParseLimits` 文档注释写「`symbol_count` / `line_entry_count` bound the DWARF tables and are **enforced by M-11, which owns that parsing**」。两处矛盾。
- **实际**：M-06 的 ELF 解析**只走程序头表**（`PT_LOAD`，见 `m06_elf/elf.rs` 的 `PT_LOAD` 常量），**完全不读节区表**；`LoadImage` 只有 `segments` + `entry`，**没有任何调试信息**。
- **裁决**：
  - **M-06 新增节区表解析**，按**白名单**提取调试节区，产出 `LoadImage.debug_sections: Vec<(String, Vec<u8>)>`；
  - **M-11 解析这些字节**（符号表语义、DWARF 行表程序都是 M-11 的活）。
- **理由**：
  1. M-06 是**唯一 ELF 容器解析者**——让 M-11 另起一套 ELF 解析器会再造一个 🟡-4（台账已经为 `riscv-golden` 的第二套解析器记了一笔账）；
  2. `ParseLimits` 的注释已经站在 M-11 拥有解析这一侧；
  3. 节区表解析**机械且小**（ELF 格式固定），而 `.debug_line` 的**行表程序**才是真正的工作量。
- **白名单与上限**：只提取 `.symtab` / `.strtab` / `.debug_line` / `.debug_line_str` / `.debug_str` / `.debug_abbrev` / `.debug_info`；**白名单节区总字节上限**纳入 `ParseLimits`（新增 `debug_section_bytes`，默认 8 MiB）——**不把整个 ELF 带进内存**（ELF 文件上限 16 MiB，而调试节区通常远小于此）。
- **对 LLD 的订正**：§1.1 / §2.3 / §5.2 / §9.1.1 的「M-06 下发符号表 + 行表」措辞统一改为「**M-06 下发调试节区原始字节，M-11 解析**」。

### 裁决 C · 渲染只出 **ISA 助记符**，压缩指令保留 `c.` 前缀

- **裁决**：
  - **不做伪指令别名**：`mv`（=`addi rd,rs,0`）、`li`（=`addi`/`lui` 组合）、`ret`（=`jalr x0,ra,0`）、`j`（=`jal`）一律**还原为 ISA 助记符**；
  - **压缩指令保留 `c.` 前缀**（`c.addi sp,-16` 而非 `addi sp,sp,-16`）。
- **理由**：
  1. 伪指令别名是**汇编器语义**，不是 ISA 语义——`mv` 需要知道源寄存器是不是 `x0`，引入第二套判断；反汇编器应当显示**ISA 实际执行的指令**；
  2. `c.` 前缀与 M-13 的 `Decoded.compressed` 标志**一一对应**，可逐字段对拍；
  3. 便于把「渲染结果」与 M-13 的解码结果做**逐字段断言**（而非文本比对）。
- **黄金参照**：**`riscv-none-elf-objdump -d -M no-aliases`**（已实测：本机 xPack GCC 15.2.0 的 `objdump` 在该模式下输出 `c.addi sp,-16` / `c.mv s0,a0` 形态，与本裁决一致）。
- **对 LLD 的订正**：LLD 未提别名问题，**无需改**；但 §3.4 的差异对照表「独立表」一列按裁决 A 改。

### 裁决 D · 一条导出命令、两条数据源，**宿主路由**（M-11 不依赖 M-19）

- **裁决**：契约层新增 **一条** 命令 `ExportTrace { source, format, offset, limit }`：
  - `source = Waveform` ⇒ 数据源是 **M-19 的环**（波形边沿）⇒ 闭环 **D-50**；
  - `source = Execution` ⇒ 数据源是 **M-11 自己的 `traceBuffer`**（指令 + 访存）⇒ 闭环 **D-51**；
  - **路由在宿主（`kernel_host`）**：宿主按 `source` 调 M-19 或 M-11 ⇒ **M-11 不依赖 M-19，M-19 也不依赖 M-11**，LLD §9.1 的依赖方向**不变**。
- **理由**：LLD §9.1 的依赖表里 M-11 不依赖 M-19；若让 M-11 直接去读 M-19 的环，就新增了一条跨层依赖。宿主本来就是**装配层**（协议装配：契约命令 ↔ 内核调用），路由放它是它的本职。
- **格式**：`format ∈ {Csv, Vcd, Json}`：
  - `Waveform` + `Vcd` ⇒ M-19 的 `export_vcd`（已有）；`Waveform` + `Csv` ⇒ M-19 的 `export_csv`（已有）；`Waveform` + `Json` ⇒ **拒绝**（M-19 无 JSON 波形导出，报 `E_CTX-03`）；
  - `Execution` + `Csv` / `Json` ⇒ M-11 的执行轨迹导出（新增）；`Execution` + `Vcd` ⇒ **拒绝**（执行轨迹不是信号波形，VCD 语义不适用）。
- **对 LLD 的订正**：LLD §1.3 / §3.3 只描述 M-11 自己的 trace 导出（CSV/VCD/JSON），本裁决**把 `Vcd` 从 M-11 的格式集里去掉**（M-11 的 trace 是**指令/访存轨迹**，不是**信号波形**；VCD 的语义是「信号值随时间变化」，与轨迹不符）⇒ M-11 的格式集 = **`Csv` / `Json`**。

### 裁决 E · `schema_version` 2 → **3**

- **理由**：本批新增**消息变体**（4 条命令 + 3 个事件）⇒ 属契约层的**结构性扩展**。v0.17 批（契约 v2）新增 3 条命令 + 3 个事件时**升了版本**（1 → 2），本批沿用同一惯例。
- **兼容**：前端 `front/lib/message.js` 的镜像**同批同步**（枚举 → tag 的映射表 + 构造函数）；`front/lib/frame.js` 的 CRC 覆盖**规范 JSON**⇒ 新变体自动被覆盖，**无需改帧格式**。
- **连带**：`crates/contract/src/lib.rs` 的 `SCHEMA_VERSION` 常量 + 其文档注释；`front/lib/message.js` 的 `SCHEMA_VERSION` 镜像；契约层「版本不一致」的处理路径（`Negotiate` 的 `schema_version` 回显）。

### 裁决 F · DWARF **只支持 v2 / v3 / v4**；v5 **明确降级**

- **实测依据**：本机 xPack GCC 15.2.0 **默认 DWARF 5** ⇒ 产物含 `.debug_line_str` 节区；`-gdwarf-4` ⇒ 无该节区。v5 的行表头（目录 / 文件表用 `DW_FORM_line_strp` 等）与 v2~v4 的行表程序**结构不同**。
- **裁决**：M-11 的行表解析器**支持 v2 / v3 / v4**；遇到 v5（版本号 ≥ 5）⇒ **`resolve_line` 返回「无 DWARF」降级态**（`{file: "", line: 0}`），**不报错、不静默给出错误的行号**。
- **测试夹具纪律**：DWARF 用例的固件**必须用 `-gdwarf-4` 编译**；且**必须有一条用例钉住「v5 产物 ⇒ 降级」**（防止 v5 被「部分解析」成错误行号）。
- **对 LLD 的订正**：LLD §1.2「输入上限」与 §2.1.1 的 `dwarfSub` 描述里补「**支持版本 = v2/v3/v4；v5 降级**」。

### 裁决 G · 反汇编缓存的失效通道 = **M-18 访存写事件**，**只失效对应地址**

- **裁决**：`disasmCache` 按地址索引；收到 M-18 的访存**写**事件（`kind = 2`）⇒ **只失效被写地址**（LLD 11-8 / §2.2「写内存失效对应 pc」）。
- **口径**：**不是**整表清空（LLD §2.2 明确「失效对应地址」）——整表清空会让自修改代码场景把整段反汇编缓存丢掉。
- ⚠️ **注意 M-18 的方向语义**：M-18 的 `TraceDir` 有 `Read` / `Write`，而 LLD 11-6 的映射是「`dir=0` → kind=1（读）、`dir=1` → kind=2（写）」⇒ **以 M-18 的 `TraceDir` 枚举为准**，不新造一套方向常量。

### 裁决 H · trace 性能开关（评审 R9）是一条**命令 + 一个事件**

- **裁决**：`SetTraceEnabled { enabled }` 命令；`TraceState { enabled, len, dropped }` 事件。
- **零开销口径**：`enabled = false` 时 `record_trace` **立即返回**（不分配、不入环）；`enabled = true` 时若仿真正在运行 ⇒ 事件里回显 `enabled` 供前端显示**降速提示**（LLD §6.1「trace 开启 → 明确降速提示」）。
- **溢出语义**：环形缓冲满 ⇒ **每次仅丢一条最旧**（LLD 11-2：写旧 head 位置再前移），`dropped` 计数准确、**不告警风暴**。

---

## 3. 契约层新增形状（**精确定义，lane 照此实现，不得自行发明**）

### 3.1 命令（`CommandPayload` 新增 4 个变体）

```rust
    /// 反汇编序列（M-11）。返回 `EventPayload::Disasm`。
    Disasm {
        /// 起始地址。
        addr: u32,
        /// 条数上限（内核侧另有 `MAX_DISASM_COUNT` 防御上限）。
        count: u32,
    },
    /// 地址 → 源码行（M-11）。返回 `EventPayload::SourceLocation`。
    ResolveLine {
        /// 待解析地址（通常是 PC）。
        addr: u32,
    },
    /// 导出（M-11 / M-19）。返回 `EventPayload::TraceExport`。
    ExportTrace {
        /// 数据源。
        source: TraceSource,
        /// 导出格式。
        format: TraceFormat,
        /// 续导起点（条目偏移；首次为 0）。
        offset: u64,
        /// 本块最多返回多少条目（0 = 内核默认上限）。
        limit: u32,
    },
    /// trace 记录开关（M-11，评审 R9）。返回 `EventPayload::TraceState`。
    SetTraceEnabled {
        /// true = 开启记录。
        enabled: bool,
    },
```

### 3.2 事件（`EventPayload` 新增 3 个变体）

```rust
    /// 反汇编序列结果。
    Disasm {
        /// 指令行序列（可能短于请求条数 ⇒ 截断）。
        entries: Vec<DisasmEntryDto>,
    },
    /// 地址 → 源码行结果。
    SourceLocation {
        /// 被解析的地址。
        addr: u32,
        /// 源码文件路径（无 DWARF / 未命中 ⇒ 空串）。
        file: String,
        /// 1 基行号（无 DWARF ⇒ 0；未命中 ⇒ `u32::MAX`）。
        line: u32,
    },
    /// 导出块结果。
    TraceExport {
        /// 数据源（回显）。
        source: TraceSource,
        /// 格式（回显）。
        format: TraceFormat,
        /// 本块起点（条目偏移）。
        offset: u64,
        /// 下一块起点（= 本块末尾 + 1；已到末尾 ⇒ 等于 `total`）。
        next_offset: u64,
        /// 筛选命中的总条目数。
        total: u64,
        /// 本块序列化字节数。
        bytes: u64,
        /// 本块内容（文本）。
        chunk: String,
    },
    /// trace 记录状态（开关 / 当前长度 / 丢弃计数）。
    TraceState {
        /// 开关状态。
        enabled: bool,
        /// 当前条目数。
        len: u32,
        /// 累计丢弃（溢出）计数。
        dropped: u64,
    },
```

### 3.3 DTO

```rust
/// 反汇编条目（M-11 → 契约层）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisasmEntryDto {
    /// 指令地址。
    pub addr: u32,
    /// 机器码原码（压缩指令 = 16 位原码，对齐 M-13 的 `Decoded.raw`）。
    pub code: u32,
    /// ISA 助记符（压缩指令保留 `c.` 前缀；裁决 C）。
    pub mnemonic: String,
    /// 指令宽度（2 / 4 字节）。
    pub width: u8,
}

/// 导出数据源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceSource {
    /// 波形边沿（数据源 = M-19 的环；裁决 D）。
    Waveform,
    /// 执行轨迹（指令 + 访存；数据源 = M-11 的 traceBuffer）。
    Execution,
}

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceFormat {
    /// CSV。
    Csv,
    /// VCD（**仅** `source = Waveform` 合法）。
    Vcd,
    /// JSON（**仅** `source = Execution` 合法）。
    Json,
}
```

> ⚠️ **命名冲突处置**：M-11 stub 里已有一个 `TraceFormat { Csv, Vcd, Txt }`（`crates/kernel/src/m11_symbol/mod.rs`）。**契约层的 `TraceFormat` 是唯一真相** ⇒ M-11 stub 的那个**删除**，改用 `riscv_sim_contract::model::TraceFormat`。⚠️ 契约层**不要**造 `Txt` 变体——LLD 的格式集是 CSV/VCD/JSON，而 `Txt` 无人使用（stub 自造的）。

### 3.4 命令 → 事件的规范映射（宿主照此实现）

| 命令 | 成功事件 | 失败事件 | 备注 |
| --- | --- | --- | --- |
| `Disasm` | `Disasm { entries }` | `Error` | 序列**可能短于** `count`（未映射 ⇒ 提前结束，LLD 11-3）；**不是错误** |
| `ResolveLine` | `SourceLocation { addr, file, line }` | `Error` | **两态**：无 DWARF ⇒ `file=""` `line=0`；有 DWARF 但未命中 ⇒ `file=""` `line=u32::MAX`（LLD 11-4） |
| `ExportTrace` | `TraceExport { .. }` | `Error` | 非法 `source`/`format` 组合 ⇒ `E_CTX-03`；`offset` 越界 ⇒ 空块（`next_offset = offset`） |
| `SetTraceEnabled` | `TraceState { enabled, len, dropped }` | `Error` | 幂等 |

### 3.5 事件主题（前端 `EVENT_TOPIC` 镜像同步）

```
Disasm:         "debug.disasm"
SourceLocation: "debug.source"
TraceExport:    "debug.export"
TraceState:     "debug.trace"
```

⚠️ 主题前缀 `debug.` 是**新增命名空间**（既有前缀：`sim.` / `waveform.` / `snapshot.` / `link.` / `tick` / `halted` / `error` / `memory.read`）⇒ 前端的 `EVENT_TOPIC` 与 Rust 契约层的「变体 → topic」映射表**必须同时改**，且**逐字一致**（v0.17 批立的「表驱动、唯一真相 = `front/lib/message.js` 的 `EVENT_TOPIC`」纪律继续适用）。

---

## 4. M-11 内部结构（双子组件，LLD 评审 P-07）

```
crates/kernel/src/m11_symbol/
├── mod.rs           # 端口 + 公开类型 + 子模块声明
├── dwarf.rs         # ② DWARF 子组件（Wave 2；Wave 1 只留空壳与降级态）
├── render.rs        # 助记符渲染器（Decoded → String；裁决 A / C）
├── disasm.rs        # 反汇编序列 + 缓存 + 失效（裁决 G）
├── symbols.rs       # .symtab 解析 + 名称/地址双向查询
└── trace.rs         # traceBuffer 环形 + 导出（CSV / JSON）
```

- **依赖方向**：`m11_symbol` → `m13_iss::Decoder`（裁决 A）· `m11_symbol` → `riscv_sim_contract`（DTO）· **不依赖** `m19_waveform`（裁决 D）。
- **M-06 → M-11 的数据流**：宿主 `LoadProject` 时把 M-06 产出的 `debug_sections` 交给 M-11 的符号表 / DWARF 解析器（**内核内部传递，不过契约层**——契约层只传**查询结果**，不传调试节区字节）。

---

## 5. 黄金参照与验收判据

### 5.1 黄金参照（本批的「真值」）

| 参照 | 工具 | 用于 |
| --- | --- | --- |
| **反汇编** | `riscv-none-elf-objdump -d -M no-aliases <elf>` | 逐条比对 `addr` / `code` / `mnemonic` / `width` |
| **地址→行** | `riscv-none-elf-addr2line -e <elf> <addr>` | 比对 `file` / `line`（Wave 2） |
| **符号** | `riscv-none-elf-nm -n <elf>` | 比对 `symbol_at` / `lookup_symbol` |
| **指令语义** | M-13 的 `Decoded` 字段 | 比对 `mnemonic` 的**每个操作数**（不是文本比对） |

> ⚠️ 参照工具的**落点在仓库外**（`$HOME/riscv-sim-golden/toolchain`），**不进仓库**；测试用例**不依赖**参照工具存在（参照对拍是**人工验收 + 可选脚本**），**单测必须自包含**（用**内联的字节夹具**）。

### 5.2 验收清单（**逐条可测**，与冻结规格 §6 的对账表一一对应）

| # | 验收项 | 判据 |
| --- | --- | --- |
| 1 | 反汇编序列正确 | 对已知 text 段 `disasm(addr, count)`，`addr`/`code`/`mnemonic`/`width` 与 `objdump -M no-aliases` **逐条一致** |
| 2 | 压缩指令正确 | 压缩指令 `code` = **16 位原码**、`width = 2`、`mnemonic` 带 `c.` 前缀 |
| 3 | **无 DWARF 独立可用** | 无调试节区的 ELF ⇒ `disasm` 正常 + `resolve_line` 返回 `{file:"", line:0}`（降级） |
| 4 | 缓存命中一致 | 同一地址 `disasm` 两次 ⇒ 结果**逐字段相同**（含 `code`） |
| 5 | **自修改代码缓存失效** | 写指令地址后重新 `disasm` ⇒ 该地址**重新取指**（`code` 变化），**其它地址的缓存条目仍在**（不是整表清空，裁决 G） |
| 6 | 符号按地址查 | `symbol_at(addr)` ⇒ 命中的符号名与 `nm -n` 一致 |
| 7 | 符号按名查 | `lookup_symbol(name)` ⇒ 地址与 `nm` 一致；未知名 ⇒ `None`（**不是错误**） |
| 8 | 符号表上限 | 超过 `ParseLimits.symbol_count` ⇒ **拒绝**（不是截断后静默少给） |
| 9 | 调试节区上限 | 调试节区总字节超 `debug_section_bytes` ⇒ **拒绝**（`E_BUS` 族或 `E_CTX` 族，**一场景一码**） |
| 10 | `Disasm` 命令 → 事件 | 宿主 `handle` 一条 `Disasm` ⇒ **恰好一条** `Disasm` 事件，形状与 §3.2 精确一致 |
| 11 | `ResolveLine` 命令 → 事件 | ⇒ 恰好一条 `SourceLocation` 事件；**两态可区分**（`line=0` vs `line=u32::MAX`） |
| 12 | `ExportTrace`（波形） | `source=Waveform, format=Csv` ⇒ 数据**来自 M-19 的环**（**不是** M-18 的边沿存储）；`format=Vcd` ⇒ 合法 |
| 13 | `ExportTrace`（执行） | `source=Execution, format=Csv/Json` ⇒ 数据来自 M-11 的 `traceBuffer`；`format=Vcd` ⇒ **拒绝** `E_CTX-03` |
| 14 | **导出续导** | 大 trace 分块 ⇒ `next_offset` 递进；用 `next_offset` 续传 ⇒ **不重不漏**（条目数 = `total`） |
| 15 | trace 开关 | `SetTraceEnabled(false)` ⇒ **零记录**（`len` 不变）；`(true)` ⇒ 开始记录 |
| 16 | **trace 溢出** | 环满后连续写 ⇒ **每次仅丢一条最旧**（`dropped` 准确），**不告警风暴** |
| 17 | **D-51 闭环** | M-18 的访存 trace 环在宿主里**有消费者**（不再只写不读）⇒ 常驻内存有界 |
| 18 | **D-50 闭环** | 波形导出的数据源是 **M-19 的环** ⇒ M-19 的环**有读者** |
| 19 | 前端反汇编面板 | `view.debug` 渲染反汇编序列，**PC 行高亮**（V-12） |
| 20 | 前端导出 | 前端可发起导出并拿到 `TraceExport` 事件的 `chunk` |
| 21 | `schema_version` | 契约层与前端镜像**都是 3**；`Negotiate` 回显 3 |
| 22 | 主题表一致 | Rust 契约层与 `front/lib/message.js` 的「变体 → topic」映射**逐字一致** |

> ⚠️ **每一条验收项都必须在变异自检里有对应的反向变异**（v0.19 批立的纪律：**有正向用例、没有反向变异** = 判别力缺口）。

---

## 6. 分 lane 与文件所有权（并行写，串行验）

> ⚠️ **本批沿用 v0.19 批的教训**：并行**写**代码可以，但**验证必须串行**——本仓所有 lane 共用同一个 `CARGO_TARGET_DIR`，并行构建会互相**换掉产物**，跑出「别人的用例在我树里红 / 测试总数忽变」的**假象**（本批之前已命中两次）。

| Lane | 所有权文件 | 依赖 |
| --- | --- | --- |
| **A · 契约层** | `crates/contract/src/{lib.rs,protocol.rs,model.rs,error.rs}` · `front/lib/message.js` | 无（**先做**，其它 lane 照 §3 写） |
| **B · M-06 节区** | `crates/kernel/src/m06_elf/{mod.rs,elf.rs,tests.rs,tests_malformed.rs}` | 无 |
| **C · M-11 核心** | `crates/kernel/src/m11_symbol/**` | A（DTO）· B（节区）· M-13（Decoder，**只读**） |
| **D · 宿主接线** | `crates/kernel/src/kernel_host/**` · `crates/kernel/tests/**` | A · C |
| **E · 前端** | `front/views/**` · `front/tests/**` | A（主题表 / DTO 镜像）· C（事件形状） |

⚠️ **M-13 的 `decoder.rs` 是只读的**——裁决 A 之后，M-11 **不得**改 M-13 的任何文件（若发现 M-13 需要改 ⇒ 说明裁决 A 站不住 ⇒ **停下来重新裁决**，不得顺手改）。

---

## 7. 与台账的关系

| 台账 | 本批动作 |
| --- | --- |
| **D-50** | ✅ **闭环**（裁决 D：波形导出数据源 = M-19 的环；验收 #12 / #18） |
| **D-51** | ✅ **闭环**（裁决 D：访存 trace 进 M-11 的环；验收 #13 / #17） |
| **🟡-4**（第二套 ELF 符号读取器） | 📌 **本批提供能力**（M-11 的符号查询），**迁移本身留 P1** |
| **D-13**（字段可见性收紧） | 📌 仍待做，与本批无关 |
| **D-30**（M-12 独立模块级 review） | 📌 仍待做，与本批无关 |
| **D-75**（内核严格等号 vs JS 前向兼容不对称） | 📌 本批 `schema_version = 3` 会**再触一次**这个不对称 ⇒ 在收口轮**显式记录** |

---

## 8. 冻结边界

- 本文**冻结**：§2 的 8 条裁决 · §3 的契约形状 · §4 的模块结构 · §5.2 的 22 条验收清单 · §6 的 lane 划分。
- 实现过程中若发现本文**自相矛盾**或**与代码现状冲突** ⇒ **先改本文**（并记入 REVIEWS.md §5 变更索引），**再改代码**。
- 本批**不修改**：`M-06` 的 `segments` / `entry` 语义 · `M-13` 的任何文件 · `M-19` 的环实现 · 契约层既有命令与事件的形状（**只增不改**）。
