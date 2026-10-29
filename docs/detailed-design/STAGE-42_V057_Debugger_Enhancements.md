# STAGE-42 · v0.57 批 · 调试器增强批（符号查询 + 条件断点表达式 + 整段转储）

> **状态**：冻结（实现与验收以本文件为准）
> **契约层版本**：`SCHEMA_VERSION` **8 → 9**（`SIM_STATE_SCHEMA_VERSION` 仍 **2** / `PLATFORM_SCHEMA_VERSION` 仍 **1** / `PLATFORM_RULES_VERSION` 仍 **3** ⇒ ⇒ **另外三个版本号一个都不升**）
> **触发点**：用户在 v0.56 批交付后的直接反馈 —— 「**UI 不支持按符号断点，比如我希望断点到 main 函数，当前无法做到**」⇒ ⇒ 用户在台账 §2.1 **候选清单**里看到的顺序（条件断点表达式 + 整段转储 + 符号查询 ⇒ v9）**由用户拍板整批做掉**。
> **界面形态**（用户拍板）：**地址与符号分两个入口** ⇒ ⇒ 另开一个「符号」输入框 + **候选下拉**（**不**做「一个框自动分流」）。

---

## §1 为什么要做这一批

### §1.1 三件事各自的缺口长什么样（**不是猜的**）

| 能力 | 今天的状况 | 缺口的**真正位置** |
| --- | --- | --- |
| **按符号断点** | 界面只有「断点地址（十六进制）」输入框（v0.54 批 · STAGE-39 加的）⇒ ⇒ 只能填**数值地址** | **契约层没有「问符号」的出口** ⇒ ⇒ 前端**无从知道** `main` 在哪 ⇒ ⇒ 界面**结构上**做不到 |
| **条件断点** | 契约的 `Breakpoint` 只有 `{id, addr, source_hint, enabled}` ⇒ ⇒ **没有条件字段** | **契约形状**（M-08 LLD §2.1.1 有 `cond`，但契约冻结期不能扩）⇒ ⇒ M-08 的 `hit_check(pc)` **只看地址** |
| **整段转储** | 契约的 `ReadMemory { addr, size }` 的 `size` 是 `WriteSize`（**1 / 2 / 4 字节**三档）⇒ ⇒ 一次最多看 **4 字节** | **契约形状**（要一段 ⇒ 要一个「长度」字段 ⇒ ⇒ **新命令**） |

⚠️ **「符号查询」这一条的关键勘察结论**：**内核早就有按名字查符号的能力** —— M-11 的 `SymbolTable::lookup(name)`（解析 ELF 的 `.symtab`）+ `SymbolIndex::lookup_symbol`（M-11 的**端口**）+ `KernelHost::lookup_symbol(name)`（宿主已公开的 API，v0.20 检视 B · D-4 闭环时把它从死代码救活）⇒ ⇒ **能力在，缺的是出口** ⇒ ⇒ 本批**不重写**符号解析 ⇒ ⇒ 前端**零** ELF 解析代码。

⚠️ **另一条关键勘察结论**：**真机门禁的 E-7 段早就在按符号名下断点** —— 但它是在**仓外侧**做的：`tools/browser-verify.mjs` 用零依赖的 `tools/elf-symtab.mjs` **自己解析** osz 的 `.symtab` 拿到调度器地址（`first_schedule = 0x80002550`）⇒ ⇒ 再把**数值地址**发给内核 ⇒ ⇒ **平台自己的界面**做不到同一件事 ⇒ ⇒ 这就是「能力在、入口缺」的**活证据**。

### §1.2 定案事实（**逐条实测 / 逐条读源码**）

| # | 事实 | 出处 |
| --- | --- | --- |
| F-1 | `KernelHost::lookup_symbol(name) -> Option<model::Symbol>` **已存在且是公开 API** | `crates/kernel/src/kernel_host/mod.rs:1920` |
| F-2 | `SymbolIndex` 是 M-11 的**唯一**符号索引（`.symtab` 解析 + DWARF 行表），由 M-06 的 `debug_sections` 喂出来 | `crates/kernel/src/m11_symbol/mod.rs:99` |
| F-3 | M-06 的白名单**含** `.symtab` / `.strtab` ⇒ ⇒ **未 strip 的 ELF 有符号**；**strip 过就没有** | `crates/kernel/src/m06_elf/elf.rs:68` |
| F-4 | 断点命中检测**只有一个入口**：`BreakpointTable::hit_check(pc)`（按地址精确匹配 + `enabled`） | `crates/kernel/src/m08_breakpoint/mod.rs:129` |
| F-5 | 命中检测挂在**取指之前** ⇒ ⇒ 命中时该指令**未执行**、状态零副作用 | `crates/kernel/src/m17_backend/mod.rs:420` |
| F-6 | 断点检查在**两处**被调：`IssExecutor::step`（指令路径）与时间驱动路径（`continue_until_event`） | `m17_backend/mod.rs:422` 与 `:1065` |
| F-7 | `SimState.gpr: [u32; 32]` + `SimState.pc` ⇒ ⇒ 条件求值所需的**寄存器与 PC 全在这里** ⇒ ⇒ **不需要新端口** | `crates/contract/src/model.rs:190` |
| F-8 | `Bus::read_only(addr, width)` 是**调试视角**的读：���计统计、**不**触发探针、**不**匹配观察点 | `crates/kernel/src/m14_bus/mod.rs:2066` |
| F-9 | `ExecutionBackend::read_memory(addr, size)` 走的正是 `read_only` ⇒ ⇒ 「调试读不污染 guest 可观测性」这条性质**已有用例钉住** | `kernel_host/tests.rs:584` |
| F-10 | `DebugMeta`（含 `Breakpoint`）**不在快照载荷里** ⇒ ⇒ 给 `Breakpoint` **加字段**不动 `SIM_STATE_SCHEMA_VERSION` | `crates/contract/src/snapshot.rs:55` |
| F-11 | `Vec<u8>` 在线上是 **JSON 数组**（`[104,105]`），**不是** base64、**不是**字符串 | `crates/contract/src/protocol.rs:636` |
| F-12 | 前端契约镜像在 `front/lib/message.js`（`SCHEMA_VERSION = 8`）⇒ ⇒ 升版**要改镜像** | `front/lib/message.js:97` |
| F-13 | `front/tests/contract.test.mjs` **从 Rust 源码解析变体集合** ⇒ ⇒ Rust 加变体而 JS 镜像没加 ⇒ ⇒ **门禁当场变红** | `front/tests/contract.test.mjs:573` |
| F-14 | 契约层的场景码**住在契约层**的判据：「跨 crate 共享」**或**「前端要按码分派」⇒ ⇒ 本批两个新码**前端都要显示** ⇒ ⇒ **住契约层** | `tools/error_code_check.py` 判据表 |
| F-15 | M-08 的模块文档写着「条件断点因此**未实现**（记为台账 **D-32** 的缺口）」⇒ ⇒ **D-32 早已在 v0.21 闭环**（7 项契约缺口，**不含**条件断点）⇒ ⇒ **那是一句过期引用** ⇒ ⇒ 本批**顺手订正它** | `crates/kernel/src/m08_breakpoint/mod.rs:16` vs `docs/reviews/REVIEWS.md` D-32 行 |

---

## §2 契约层新增面（**逐字冻结**）

### §2.1 新增 DTO：**零个**

⇒ ⇒ 复用契约层**已有**的 `model::Symbol { name: String, addr: Option<u32>, size: Option<u32> }` ⇒ ⇒ **零新 DTO**。

### §2.2 新增命令：**两条**

```rust
// crates/contract/src/protocol.rs · CommandPayload
/// 按**名字**查符号（M-11 · v0.57 批）。
///
/// ⚠️ **一条命令同时服务两种用法**（裁决 B）：
/// - **精确命中** ⇒ `SymbolResolved.symbol` 里有它；
/// - **只给前缀** ⇒ `SymbolResolved.candidates` 里有候选。
ResolveSymbol {
    /// 符号名。**空字符串或纯空白 ⇒ 拒绝**（`E_DBG_02`）。
    name: String,
},

/// 转储一段内存（M-10 · v0.57 批）。
///
/// ⚠️ **原样拒，不静默截断**（裁决 D）：`len == 0` 或 `len > DUMP_MAX_BYTES`
/// ⇒ ⇒ `E_DBG_03`。
DumpMemory {
    /// 起始**物理**地址。
    addr: u32,
    /// 字节数（`1..=DUMP_MAX_BYTES`）。
    len: u32,
},
```

### §2.3 新增事件：**两条**

```rust
// crates/contract/src/protocol.rs · EventPayload
/// 符号查询的回复（M-11 · v0.57 批）。
SymbolResolved {
    /// **回显**请求里的名字（前端按它对上号）。
    name: String,
    /// **精确命中**的那一条（**没有 ⇒ `None`**；⚠️ 「没有」**不是**错误）。
    symbol: Option<Symbol>,
    /// **前缀候选**（名字以请求名**开头**的那些，按名字**升序**；
    /// **不含**精确命中那条；条数 ≤ `SYMBOL_CANDIDATE_LIMIT`）。
    candidates: Vec<Symbol>,
    /// 该镜像的**符号总数**（`0` ⇒ 这个镜像**没有符号表**）。
    total: u32,
},

/// 一段内存的**字节**（M-10 · v0.57 批）。
MemoryDump {
    /// **回显**起始地址。
    addr: u32,
    /// 读到的字节（长度 == 请求的 `len`；⚠️ 线上是 **JSON 数组**，
    /// **不是** base64、**不是**字符串 —— 与 `Console` 事件同口径）。
    bytes: Vec<u8>,
},
```

### §2.4 `Breakpoint` DTO：**加一个可选字段**

```rust
// crates/contract/src/model.rs
pub struct Breakpoint {
    pub id: u32,
    pub addr: u32,
    pub source_hint: Option<String>,
    pub enabled: bool,
    /// **条件表达式**（v0.57 批新增 · 裁决 F）。
    ///
    /// ⚠️ `None` ⇒ **无条件断点** ⇒ ⇒ **既有行为一字不变**。
    /// ⚠️ `Some(expr)` ⇒ 命中该地址时**还要**满足 `expr` 才停（裁决 H）。
    /// ⚠️ **表达式非法 ⇒ `SetBreakpoint` 被拒**（`E_DBG_02`），断点表**不变**
    ///   （裁决 I）。
    #[serde(default)]
    pub condition: Option<String>,
}
```

⚠️ **`#[serde(default)]` ⇒ 旧载荷仍能解析** ⇒ ⇒ **不是**破坏性格式变更 ⇒ ⇒ `SIM_STATE_SCHEMA_VERSION` **不升**（裁决 A）。

### §2.5 新增契约常量：**两个**

```rust
// crates/contract/src/lib.rs（与 SCHEMA_VERSION 同居）
/// 转储长度上限（字节）—— 裁决 D。
pub const DUMP_MAX_BYTES: u32 = 4096;
/// 符号候选条数上限 —— 裁决 B。
pub const SYMBOL_CANDIDATE_LIMIT: u32 = 16;
```

⚠️ **两个上限是**跨层**的 ⇒ ⇒ 前端镜像里也要有一份 ⇒ ⇒ **两份字面量 = 两个真相** ⇒ ⇒ 用**跨层夹具**钉住（裁决 K）：`crates/contract/fixtures/debug-limits.json` = `{"dump_max_bytes": 4096, "symbol_candidate_limit": 16}` ⇒ ⇒ **Rust 侧**断言常量 == 夹具、**前端用例**读**同一个文件**断言镜像常量 == 夹具（与 `admin-dto.json` 同形先例）。

### §2.6 新增错误码：**两个**（住契约层 · 判据见 F-14）

```rust
// crates/contract/src/error.rs
/// 条件表达式**解析失败**（语法错 / 未知寄存器名 / 空表达式）。
pub const E_DBG_02_CONDITION_INVALID: u16 = 2;
/// 转储长度越界（`len == 0` 或 `len > DUMP_MAX_BYTES`）。
pub const E_DBG_03_DUMP_TOO_LARGE: u16 = 3;
```

⚠️ **族选 `DBG`（不是 `CTX`）**：契约层 `error.rs` 对 `Debug` 族的定义逐字是「Debug services: breakpoint, step, register/memory (M-08, M-09, M-10)」⇒ ⇒ 本批两个码一个是**断点条件**、一个是**内存读**，**两条都在该族的定义范围内** ⇒ ⇒ 落 `DBG`。
⚠️ **与 M-08 LLD §6.1 的「本模块错误码归属 E_CTX 族」并存但不一致**（M-08 既有的 `E_CTX-07` **不动**）⇒ ⇒ **有意偏差，如实登记，不藏**（进台账的已知边界）。

### §2.7 topic：**两个**

| 命令 | 回复事件 | topic |
| --- | --- | --- |
| `ResolveSymbol` | `SymbolResolved` | `debug.symbol` |
| `DumpMemory` | `MemoryDump` | `debug.dump` |

⚠️ topic 命名与既有 `debug.disasm` / `debug.source` / `debug.export` / `debug.trace` **同族** ⇒ ⇒ 前端镜像的 `EVENT_TOPIC` 表**加两行**。

### §2.8 版本号

| 常量 | 旧 | 新 | 理由 |
| --- | --- | --- | --- |
| `SCHEMA_VERSION` | 8 | **9** | 新增两条命令 + 两条事件 ⇒ ⇒ **信封变了** |
| `SIM_STATE_SCHEMA_VERSION` | 2 | **2** | 快照载荷**不含** `DebugMeta`（F-10）⇒ ⇒ 一字不动 |
| `PLATFORM_SCHEMA_VERSION` | 1 | **1** | 内存图描述**一字不改** |
| `PLATFORM_RULES_VERSION` | 3 | **3** | 合法性规则集**一字不改** |

---

## §3 裁决 A~P

- **裁决 A**：**新增面全是「加」** —— 既有命令 / 事件的形状**一字不改**；只有 `Breakpoint` **加一个可选字段** ⇒ ⇒ **零破坏性变更**（详见 §7）。
- **裁决 B**：**符号查询 = 一条命令 + 两个字段**（`symbol` 精确命中 / `candidates` 前缀候选）⇒ ⇒ 「填全名直接下断点」与「填前缀挑候选」**共用一条命令** ⇒ ⇒ **不**另开「列出全部符号」的命令（osz 的符号表有几千条 ⇒ 全量回传会撑爆事件）。
- **裁决 C**：**符号解析住在内核（M-11）** ⇒ ⇒ 前端**零** ELF 解析代码 ⇒ ⇒ 仓内**仍只有一套** ELF 解析器（M-06）与**一套**符号表（M-11）⇒ ⇒ **不复制** `tools/elf-symtab.mjs` 的算法进前端（它只是**门禁的独立参照物**，不是产品代码）。
- **裁决 D**：**转储是「一段连续字节」**（`addr` + `len`），上限 `DUMP_MAX_BYTES = 4096` ⇒ ⇒ **原样拒**（`len == 0` 或超限 ⇒ `E_DBG_03`）⇒ ⇒ **不静默截断**（与 STAGE-15 的「原样拒」同纪律）。
- **裁决 E**：**转储走调试视角**（`Bus::read_only`）⇒ ⇒ **不计**总线统计、**不**触发探针、**不**匹配观察点 ⇒ ⇒ 调试器的窥视**不污染** guest 的可观测性（与 F-9 既有性质**同一条路**）。
- **裁决 F**：**条件表达式挂在 `Breakpoint` 上**（可选字段）⇒ ⇒ `None` ⇒ **无条件断点** ⇒ ⇒ 既有断点语义**一字不变** ⇒ ⇒ 老脚本口不受影响。
- **裁决 G**：**条件表达式 v1 只支持「寄存器 / PC / 字面量」** ⇒ ⇒ **不含内存读** ⇒ ⇒ 理由：读内存要过总线（未映射 ⇒ 陷阱）⇒ ⇒ v1 **不引入**那套语义 ⇒ ⇒ 内存侧的观察走**整段转储**（裁决 D）。
- **裁决 H**：**条件在「取指之前」求值**（与断点命中**同一时刻**）⇒ ⇒ 条件为**假** ⇒ ⇒ **不停**（继续执行下一条）⇒ ⇒ 条件为**真** ⇒ ⇒ 停（`HaltReason::Breakpoint`）⇒ ⇒ **PC 停在断点地址**、该指令**未执行**（与既有断点语义**完全一致**，F-5）。
- **裁决 I**：**条件表达式解析失败 ⇒ `SetBreakpoint` 被拒**（`E_DBG_02`）⇒ ⇒ 断点表**不变**（与「地址非法 ⇒ 表不变」**同一条纪律**）⇒ ⇒ 前端显示**内核原文**（不吞）。
- **裁决 J**：**条件求值失败在 v1 结构上不可能**（只有寄存器读 ⇒ 永远读得到）⇒ ⇒ **不**设计「求值出错」的语义 ⇒ ⇒ **不**给它一个假的错误码（免得那条路**永远不可达**）。
- **裁决 K**：**两个上限用跨层夹具钉住**（`crates/contract/fixtures/debug-limits.json`）⇒ ⇒ Rust 常量与前端镜像常量**都由夹具判定** ⇒ ⇒ **两个真相**结构上不可能。
- **裁决 L**：**地址与符号分两个入口**（用户拍板）⇒ ⇒ 「符号」框**只**收符号名、「地址」框**只**收十六进制地址 ⇒ ⇒ **不**做「一个框自动分流」⇒ ⇒ 填错时的失败模式**更清楚**（地址框填了 `main` ⇒ 地址框报「不是合法地址」；符号框填了 `0x100` ⇒ 符号框报「没有这个符号」）。
- **裁决 M**：**符号查询按「显式动作」触发**（「查找」按钮 / 回车）⇒ ⇒ **一次输入 = 一次查询** ⇒ ⇒ **没有**「边打字边查」的乱序问题 ⇒ ⇒ 真机门禁**确定性**。
- **裁决 N**：**候选下拉是「可点的列表」**（每个候选一个按钮 ⇒ ⇒ **点一下直接下断点**）⇒ ⇒ 理由：`<select>` 要「先选中、再点按钮」**两步**才能下断点 ⇒ ⇒ 多一步**零收益**。
- **裁决 O**：**条件输入框住在「断点」卡**（与地址框、符号框同卡）⇒ ⇒ 地址断点与符号断点**共用**同一套条件输入 ⇒ ⇒ **不为符号断点单开一个条件框**。
- **裁决 P**：**转储 UI 住在「寄存器 / 内存」视图**（M-10 那张卡）⇒ ⇒ 断点相关的一切住在「调试」视图 ⇒ ⇒ **两个视图各管各的**（不混）。
- **裁决 Q**：**前端不预校验**（长度 / 条件表达式）⇒ ⇒ 一律发命令 ⇒ ⇒ **内核是唯一真相**（本仓纪律：前端不写第二份校验器）⇒ ⇒ 前端预校验会让内核那条上限路径**永远走不到** ⇒ ⇒ 真机断言**永远红不了** ⇒ ⇒ 那是**假绿**。

---

## §4 条件表达式语法（v1 · 冻结）

### §4.1 EBNF

```ebnf
expr     = or_expr ;
or_expr  = and_expr , { "||" , and_expr } ;
and_expr = cmp_expr , { "&&" , cmp_expr } ;
cmp_expr = unary , [ relop , unary ] ;
relop    = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
unary    = "!" , unary | primary ;
primary  = "(" , expr , ")" | operand ;
operand  = number | reg ;
number   = dec | hex ;                       (* dec = [0-9]+ ; hex = "0x" , hex_digit+ *)
reg      = "pc" | "x" , ( "0".."9" | "1".."2" , "0".."9" | "3" , "0".."1" ) ;
```

⚠️ **ABI 别名**（在 `reg` 的位置上等价于某个 `xN`）：
`zero`=`x0` · `ra`=`x1` · `sp`=`x2` · `gp`=`x3` · `tp`=`x4` · `t0`~`t2`=`x5`~`x7` · `s0`=`x8` · `fp`=`x9` · `s1`=`x9` · `a0`~`a7`=`x10`~`x17` · `s2`~`s11`=`x18`~`x27` · `t3`~`t6`=`x28`~`x31`。
（⚠️ `fp` 与 `s1` **都**映射 `x9` ⇒ ⇒ 是**同一个**寄存器的两个名字。）

### §4.2 语义

- **值类型**：单��� **u32**（RV32 的 GPR 与 PC 都是 u32）。
- **比较**：**无符号**（`<` / `<=` / `>` / `>=` 按 u32 比）⇒ ⇒ **不**有符号溢出语义。
- **真值**：数值 **≠ 0** 即真。
- **`&&` / `||`**：**短路**（左假则右**不求值** / 左真则右**不求值**）⇒ ⇒ 副作用为零（本来也没有副作用）⇒ ⇒ 短路只是**求值顺序**的定义。
- **优先级**：`||` < `&&` < 比较 < `!` < 一元/括号。
- **求值时刻**：**取指之前**，此时**该指令未执行** ⇒ ⇒ 条件里看到的寄存器值 = **断点地址那条指令执行前**的值（裁决 H）。

### §4.3 例子

| 表达式 | 含义 |
| --- | --- |
| `x1 == 5` | `x1` 等于 5 时停 |
| `a0 != 0` | `a0` 非零时停（ABI 别名） |
| `pc >= 0x80001000` | PC 越过某地址时停 |
| `x1 == 5 && x2 != 0` | 两条同时成立才停 |
| `x1 == 5 \|\| x2 == 7` | 任一成立就停 |
| `!(x1 == 0)` | 取反 |
| `s0 == sp` | 两个寄存器比 |

### §4.4 非法表达式 ⇒ **原样拒**（`E_DBG_02`）

| 输入 | 为什么非法 |
| --- | --- |
| `""` / `"   "` | **空**表达式（前端**不许**发空串 ⇒ 发 `null`） |
| `x32` | 寄存器号越界（只有 `x0`~`x31`） |
| `foo` | 未知寄存器名 |
| `x1 ==` | 语法错（缺右操作数） |
| `x1 = 5` | 非法运算符（`=` 不是 `==`） |
| `(x1 == 5` | 括号不配对 |
| `0x` / `123abc` | 非法字面量 |

⚠️ **错误消息**必须**含**「哪一段不对」的线索（内核原文透传，前端**不吞**）⇒ ⇒ 例如 `条件表达式无效：未知寄存器名 "foo"`。

---

## §5 界面落点（**视图组件** → **DOM 契约**）

### §5.1 调试视图 · 断点卡（M-02）

```html
<!-- 既有：地址入口（一字不改） -->
<div class="addr-input" data-addr-input="breakpoint">
    <input data-bp-addr-input placeholder="断点地址（十六进制）">
    <button data-action="set-breakpoint">加断点</button>
    <span class="addr-error" data-bp-addr-error></span>
</div>

<!-- 新增 ①：条件（可选）· 裁决 O -->
<div class="cond-input" data-cond-input="breakpoint">
    <input data-bp-cond-input placeholder="条件（可选），例如 x1 == 5" spellcheck="false" autocomplete="off">
    <span class="addr-error" data-bp-cond-error></span>
</div>

<!-- 新增 ②：符号入口（独立）· 裁决 L / M / N -->
<div class="symbol-input" data-symbol-input="breakpoint">
    <input data-bp-symbol-input placeholder="符号名（例如 main）" spellcheck="false" autocomplete="off">
    <button data-action="lookup-symbol">查找</button>
    <span class="addr-error" data-bp-symbol-error></span>
    <div class="symbol-candidates" data-bp-symbol-candidates><!-- 候选按钮 --></div>
</div>
```

- **候选按钮**（裁决 N，**形状钉死**）：
  `<button type="button" class="symbol-candidate" data-action="set-symbol-breakpoint" data-addr="0x80001234" data-symbol="main">main @ 0x80001234</button>`
  ⇒ ⇒ **点一下直接下断点**（带上条件框里的条件，若非空）。
  ⇒ ⇒ `data-addr` 是 **`0x` + 8 位小写十六进制**；`data-symbol` 是**符号名**；按钮文本是 `<名字> @ <地址>`。
- **精确命中**也在候选列表里（**第一个**）⇒ ⇒ 用户填了全名 ⇒ ⇒ 列表里就是那一条 ⇒ ⇒ 点它。
- **候选为空且无精确命中** ⇒ ⇒ 显示「**没有这个符号**」+（`total == 0` ⇒ ⇒ 追加「**这个镜像没有符号表（可能被 strip 了）**」；`total > 0` ⇒ ⇒ 追加「**这个镜像共有 N 个符号**」）。

### §5.2 寄存器 / 内存视图（M-10 那张卡）

```html
<!-- 既有：地址 + 宽度 + 读（一字不改） -->
<!-- 新增 ③：整段转储 -->
<div class="dump-input" data-dump-input="memory">
    <input data-dump-addr-input placeholder="转储起始地址（十六进制）">
    <input data-dump-len-input placeholder="字节数（≤ 4096）" value="256">
    <button data-action="dump-memory">转储</button>
    <span class="addr-error" data-dump-error></span>
</div>
<div class="mem-dump" data-mem-dump><!-- 十六进制转储表格 --></div>
```

- 转储结果渲染成 **`offset | 十六进制（每行 16 字节）| ASCII`** 三列，**形状钉死**（⇒ ⇒ 真机门禁要按**具体值**断言）：
  ```html
  <div class="mem-dump" data-mem-dump>
      <table class="dump-table">
          <thead><tr><th>偏移</th><th>十六进制</th><th>ASCII</th></tr></thead>
          <tbody>
              <tr class="dump-row" data-dump-off="00000000">
                  <td class="dump-off">00000000</td>
                  <td class="dump-hex">13 00 00 00 ...</td>
                  <td class="dump-ascii">....</td>
              </tr>
          </tbody>
      </table>
  </div>
  ```
  - **偏移列**：**8 位小写十六进制**、**不带** `0x`。
  - **十六进制列**：**每字节 2 位小写十六进制**、**空格分隔**、**每行 16 字节**（末行可不足 16 个）。
  - **ASCII 列**：可打印 ASCII（`0x20`..`0x7e`）**原样**，其余一律 `.`。
  - 字节序按**地址递增**（低地址在前）。
- ⚠️ **前端不预校验长度**（裁决 Q）⇒ ⇒ 一律发命令 ⇒ ⇒ 内核拒（`E_DBG-03`）⇒ ⇒ 前端**照样要把内核原文显示出来**（不吞、不改写）。
- ⚠️ **转储结果住在 M-03 视图状态**（`regmem.dump = { addr, bytes }`）⇒ ⇒ **不**另存一份。

### §5.3 断点列表（既有卡片的列表区）

- 既有行是 `<code class="addr">0x…</code>` + 「断点 id = 地址」。
- 新增：**符号名标签**（若这个断点是从符号来的）⇒ ⇒ `<span class="note">main</span>`。
- ⚠️ 断点列表**仍是**「本会话由界面请求过的断点」⇒ ⇒ **不是**内核断点集的镜像（既有边界**原样保留**）。

---

## §6 判别力清单（验收 · **落地前先钉住**）

> ⚠️ **判据不许是空的**：每一条都要能回答「**被观测的对象整个没了，这条判据还会绿吗？**」⇒ ⇒ 需要「对象存在」的前置断言的都带一条。

⚠️ **编号空间**（与本仓既有批次同口径 ⇒ ⇒ **不要**照抄别批的编号范围）：
- **A 组** = 契约层（Rust）⇒ ⇒ **每批范围**。
- **K 组** = 内核（Rust）⇒ ⇒ **每批范围**（**不叫 B** ⇒ ⇒ 因为 **B 是真机输出的编号空间**）。
- **N 组** = 前端 Node 门禁 ⇒ ⇒ **每批范围**（与 STAGE-41 同口径）。
- **B 组** = **真机门禁** ⇒ ⇒ **跨批次唯一**（接 STAGE-41 的 B-76 ⇒ ⇒ 本批从 **B-77** 起）。
- **X 组** = 交叉 ⇒ ⇒ **每批范围**。

### §6.1 契约层（A 组）

| # | 判据 |
| --- | --- |
| A-1 | `SCHEMA_VERSION == 9`（字面量断言） |
| A-2 | `ResolveSymbol` / `DumpMemory` 两条命令的**线上形状**逐字钉住（`{"ResolveSymbol":{"name":"main"}}` / `{"DumpMemory":{"addr":0,"len":16}}`） |
| A-3 | `SymbolResolved` / `MemoryDump` 两条事件的**线上形状**逐字钉住 |
| A-4 | `Breakpoint` 的 `condition` 字段：`None` ⇒ 线上**没有**这个键；`Some("x1 == 5")` ⇒ 线上是**字符串** |
| A-5 | **旧载荷仍能解析**：不带 `condition` 的 `Breakpoint` JSON ⇒ 解析成功且 `condition == None` ⇒ ⇒ **证明 `#[serde(default)]` 生效**（不是破坏性变更） |
| A-6 | 两个新错误码的**值与名相符**（`E_DBG_02 → 2` / `E_DBG_03 → 3`）且族已声明 |
| A-7 | 两个上限常量 == `fixtures/debug-limits.json` 的值 |
| A-8 | `SIM_STATE_SCHEMA_VERSION` **仍 2** / `PLATFORM_SCHEMA_VERSION` **仍 1** / `PLATFORM_RULES_VERSION` **仍 3** |

### §6.2 内核（K 组）

| # | 判据 |
| --- | --- |
| K-1 | `SymbolTable::lookup_prefix("ma")` ⇒ 返回**名字以 `ma` 开头**的符号、**按名字升序**、条数 ≤ 上限 |
| K-2 | `lookup_prefix` **不含**精确命中之外的东西（不是子串匹配：`main` **不**该匹配 `domain`） |
| K-3 | 宿主 `ResolveSymbol`：**精确命中** ⇒ `symbol` 有值、`candidates` **不含**它；**只有前缀命中** ⇒ `symbol == None` 且 `candidates` 有值 |
| K-4 | 宿主 `ResolveSymbol`：**空名字** ⇒ `E_DBG_02`，**不发** `SymbolResolved` |
| K-5 | 宿主 `DumpMemory`：`len == 0` / `len > 4096` ⇒ `E_DBG_03`，**不发** `MemoryDump` |
| K-6 | 宿主 `DumpMemory`：读一段 ⇒ `bytes.len() == len` 且**逐字节**等于期望 |
| K-7 | 宿主 `DumpMemory`：**不计入**总线统计（`bus().stats().reads` 不变）⇒ ⇒ **证明**走的是 `read_only`（裁决 E） |
| K-8 | 宿主 `DumpMemory`：**不**触发读观察点（下一条 guest 指令不得误报观察点命中） |
| K-9 | 条件表达式解析器：§4.4 的**每一行**都是**一条红**（各一条用例） |
| K-10 | 条件表达式求值器：§4.3 的**每一行**都是**一条红**（各一条用例） |
| K-11 | `SetBreakpoint` 带**非法条件** ⇒ `E_DBG_02` 且**断点表不变**（`len()` 不变）⇒ ⇒ **证明**「原样拒」（裁决 I） |
| K-12 | 条件为**假** ⇒ 跑到断点地址**不停**（继续执行）⇒ ⇒ **PC 最终越过**那个地址 |
| K-13 | 条件为**真** ⇒ 停，`HaltReason::Breakpoint`，**PC == 断点地址**，且该指令**未执行**（后一条指令没跑） |
| K-14 | **无条件断点**（`condition == None`）⇒ 行为与既有**逐字相同**（既有用例全绿） |
| K-15 | 条件断点在**时间驱动路径**（`continue_until_event`）上**同样**生效（两条路径都要判 —— F-6） |

### §6.3 前端（N 组）

| # | 判据 |
| --- | --- |
| N-1 | 前端镜像的 `SCHEMA_VERSION == 9`，且**镜像常量** == `fixtures/debug-limits.json` |
| N-2 | `Command.resolveSymbol("main")` / `Command.dumpMemory(0x1000, 16)` 的**载荷键名**与 Rust 侧逐字一致（跨层门禁会自动比） |
| N-3 | 符号框填 `main` + 点「查找」⇒ ⇒ **发出一条** `ResolveSymbol` 命令（**不多发**、**不少发**） |
| N-4 | 收到 `SymbolResolved` ⇒ ⇒ 候选区**渲染出**候选（**断言候选个数**，不是「非空」） |
| N-5 | 候选区**为空且**无精确命中 ⇒ ⇒ 显示「没有这个符号」 |
| N-6 | `total == 0` ⇒ ⇒ 显示「没有符号表」那一行（**另一条文案**）⇒ ⇒ 两态**可区分** |
| N-7 | 点候选 ⇒ ⇒ **发出** `SetBreakpoint`，其 `addr` == 候选地址、`source_hint == 候选名` |
| N-8 | 条件框非空 ⇒ ⇒ `SetBreakpoint` 的 `condition` **逐字**等于输入框内容 |
| N-9 | 条件框为空 ⇒ ⇒ `condition` 是 **`null`**（**不是**空串） |
| N-10 | 断点列表**显示**符号名（断言那行文本里**有**符号名） |
| N-11 | 转储：填地址 + 长度 + 点「转储」⇒ ⇒ 发出 `DumpMemory`，参数逐字一致 |
| N-12 | 收到 `MemoryDump` ⇒ ⇒ 转储区渲染出**行数与字节数**（断言具体数字，不是「非空」） |
| N-13 | `E_DBG_02` / `E_DBG_03` ⇒ ⇒ **内核原文**显示在对应的错误位上（**不吞**） |

### §6.4 真机门禁（B 组 · B-77 ~ B-87）

> ⚠️ **真机段的断言编号是**跨批次唯一**的 ⇒ ⇒ 接 STAGE-41 的 B-76 ⇒ ⇒ 从 **B-77** 起。

| # | 判据 | 落点 |
| --- | --- | --- |
| B-77 | 调试视图里**同时有**「符号」框与「条件」框（⇒ ⇒ 用户实际会去的那一屏） | `symbolUi` 段 |
| B-78 | 在**没有符号表**的镜像上查符号 ⇒ ⇒ 界面说清「这个镜像没有符号表」（⇒ ⇒ 与「有表但没这个符号」**两态可区分**） | `symbolUi` 段 |
| B-79 | **非法条件** ⇒ ⇒ **内核拒**（`E_DBG-02`）⇒ ⇒ **内核原文**上屏（⇒ ⇒ 条件这条路真的接到了内核 ⇒ ⇒ 前端没自己判） | `symbolUi` 段 |
| B-80 | 寄存器/内存视图里**有**「转储」入口 | `memDump` 段 |
| B-81 | 转储 `0x0` 起 16 字节 ⇒ ⇒ 第一行十六进制**逐字**等于 demo 固件的头 16 字节（**独立参照物** ⇒ ⇒ 读的是真内存） | `memDump` 段 |
| B-82 | 转储结果的**行数与偏移列**对得上（16 字节 = 1 行 · 偏移 `00000000`） | `memDump` 段 |
| B-83 | 转储长度越界 ⇒ ⇒ **内核拒**（`E_DBG-03`）⇒ ⇒ **内核原文**上屏 | `memDump` 段 |
| B-84 | 界面里按**名字的前缀**查符号 ⇒ ⇒ 候选下拉里**出现**那个符号 | `symbolBreakpoint` 段 |
| B-85 | 候选上的地址与**独立参照物**（仓内 `tools/elf-symtab.mjs` 按名字查的）**逐字相同** | `symbolBreakpoint` 段 |
| B-86 | 点候选 ⇒ ⇒ 断点列表里出现**那个地址**，且那一行**写着符号名** | `symbolBreakpoint` 段 |
| B-87 | 撤掉之后 ⇒ ⇒ 断点列表里**没有**那个地址了（⇒ ⇒ 不给后面的段留尾巴） | `symbolBreakpoint` 段 |

⚠️ **三段的落点顺序是硬约束**：
- `symbolUi` / `memDump` ⇒ ⇒ **必须排在 `osz` 之前**（osz 段把镜像换成仓外的真 RTOS ⇒ ⇒ 那个镜像**有**符号表 ⇒ ⇒ 「没有符号表」那一态就**永远测不到**；转储的期望值也会变）。
- `symbolBreakpoint` ⇒ ⇒ **必须排在 `osz` 之后**（它要**真 osz** 的 `.symtab`）⇒ ⇒ **没有 osz ELF 时一起跳过**（不是判负）。

### §6.5 变异（T 组 · 施工后填实测红集）

见 §9。

---

## §7 破坏性变更（**逐条列明**）

| # | 变更 | 破坏面 | 缓解 |
| --- | --- | --- | --- |
| 1 | `SCHEMA_VERSION` 8 → **9** | 旧前端 ↔ 新内核：版本不匹配 ⇒ ⇒ `E_PROTO-01` | 前后端**同批**升级（镜像与契约层一起改） |
| 2 | `Breakpoint` **加** `condition` 字段 | **无**（`#[serde(default)]` ⇒ 旧载荷可解析 ⇒ ⇒ A-5 钉住） | — |
| 3 | 新增两条命令 / 两条事件 / 两个 topic | **无**（既有形状一字不改） | — |
| 4 | 新增两个错误码 | **无**（新号，未被占用） | — |
| 5 | `SIM_STATE_SCHEMA_VERSION` / `PLATFORM_SCHEMA_VERSION` / `PLATFORM_RULES_VERSION` | **无**（**都不升**） | — |

⚠️ **没有**「旧快照读不出来」这类破坏（快照载荷不含 `DebugMeta`，F-10）。

---

## §8 施工期订正

（施工后填：**逐条**写「原判断 → 实测 → 改法」。）

---

## §9 变异自检

（施工后填：**真变异** + **对照** + **红集** + **叠加检验**。）

---

## §10 本规格的自检脚本

`tools/stage42_check.py` —— 校验本规格的**八项 + 子项**（表格列数 / 标题跳级 / 围栏配平 /
裁决 A~Q 无悬空引用 / 章节完整 / 验收编号交叉引用 / 关键字面量与**跨层源代码锚点** /
**跨层夹具**的两个上限值）。

⚠️ **表格切分复用 `tools/doc_check.py` 的那一份**（**一个约束一个落点**）⇒ ⇒ **禁**自己
再写一份朴素 `split("|")`。

⚠️ **验收编号空间**（与 STAGE-41 同口径）⇒ ⇒ **A / K / N / X 是每批范围**、**B 是真机
输出的跨批次唯一空间**（本批 B-77 ~ B-87）⇒ ⇒ 内核那组**不叫 B**。
