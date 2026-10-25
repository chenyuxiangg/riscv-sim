//! 内核宿主（Kernel Host）—— **协议装配层**。
//!
//! ## 它是什么
//!
//! `kernel_host` **不是** 23 个详细设计模块之一。它是那些模块的**装配**：
//! 把冻结的 `riscv_sim_contract::protocol::Message` **命令**翻译成
//! M-12 / M-13 / M-14 / M-08 / M-17 的调用，再把结果翻译回契约的
//! `EventPayload`。
//!
//! 它在架构里的位置，就是前端 `front/dev/mock-kernel.js` 那个「真内核的
//! 对应物」的位置——mock 内核当年是为了在真内核还没有导出层时让整条前端
//! 链路跑起来而存在的；`kernel_host` 是它的**真身**，而 mock 退化为测试
//! 替身。
//!
//! ## 它不是什么
//!
//! - **不是契约层**。契约（`crates/contract/**`）冻结，本模块只**使用**
//!   它的 DTO / 事件 / 错误码，一个字段都不加。
//! - **不是新模块**。M-12 仍然是平台状态与虚拟时钟的唯一持有者，M-13 仍然
//!   是指令解释器，M-14 仍然是内存映射，M-08 仍然是断点表。本模块**不持有**
//!   任何一份它们的状态——它只做翻译。
//!
//! ## 形状
//!
//! 形状由 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` 冻结：
//! `handle_message(&Message) -> Vec<EventPayload>` 是内核侧的核心，
//! `handle_json(&str) -> String` 是 WASM 桥的入口（`wasm_api.rs` 只做
//! `#[wasm_bindgen]` 包装）。
//!
//! ## 第 1 阶段的已知边界
//!
//! - **M-12 的调度循环没被启用**：本阶段没有定时器 / 事件队列，事件队列
//!   恒空，所以指令执行直接由 M-17 驱动 M-13，M-12 承担的是**平台状态与
//!   虚拟时钟的唯一持有者**。接线外设时再把调度循环打开。
//!
//! ## 探针管道（D-46）
//!
//! 探针管道是**接上了**的：
//!
//! ```text
//! guest 写外设 → M-14 派发 → M-15 判「状态真变了」→ report(signal)
//!    → M-18 判沿 → 边沿进 M-18 自己的存储
//!                   └→ 同时进 M-19 的环（EdgeSink 端口）
//!    → 本模块从 M-18 的存储取边沿 → 契约 EventPayload::WaveformHistory
//! ```
//!
//! ⚠️ **契约事件取自 M-18，不是 M-19**。M-19 的环也装了、边沿也一并进去，
//! 但它目前**只写不读**——CSV/VCD 导出**还没接进契约**（台账 D-50）。
//!
//! - **总线 hook 是追加式的**（M-14 `add_hook`）：M-17 的观察点 hook 与
//!   M-18 的访存 trace hook **共存**，后装的不会把先装的顶掉。
//! - **通道列表与边沿走同一个事件形状**（契约冻结，没有单独的「通道列表」
//!   变体）：订阅 `sim.waveform` 时**强制**推一次（哪怕还没有边沿），
//!   前端据此列出「有这条信号，只是还没变化」。
//! - **波形的时间轴靠外设时钟**：M-15 的 GPIO 在 `tick(now_ps)` 里记下
//!   当前时间，边沿才带得上真实时间戳。M-17 的步进循环每条指令同步一次
//!   外设时钟（D-07 的时间驱动调度器落地后由 M-12 的量子边界接管）。
//!
//! ## 契约 v2 的三个新命令
//!
//! 冻结规格：`docs/detailed-design/STAGE-2_Contract_V2.md` §5。
//!
//! | 命令 | 内核行为 | 回复事件 |
//! | --- | --- | --- |
//! | `ReadRegisters{index,count}` | 读 M-13 的 `gpr`；`index`/`count` **钳到** `[0,32)`；`count = 0` → **回复空区间**（`values` 为空数组） | `Registers{index,values}`（`index` = **实际**起点） |
//! | `WriteRegister{index,value}` | 写 M-13 的 `gpr`；`index == 0` → **丢弃**（D-04）但**仍回复**；`index > 31` → **拒绝**（`E_DBG-01`） | `Registers{index,values}`（`values.len() == 1`） |
//! | `Negotiate{min_schema_version,cap_bits}` | `min_schema_version > SCHEMA_VERSION` → **拒绝**（`E_PROTO-01`） | `Negotiated{schema_version,cap_bits,kernel}` |
//!
//! ⚠️ **读是钳位、写是拒绝**：写钳位会**静默**写到别的寄存器。
//!
//! ⚠️ **回复事件不走 topic 过滤**：`Registers` / `Negotiated` 是命令的
//! **直接回复**，不是订阅流——订阅没建立也必须送达（前端收不到就白发了）。
//!
//! ⚠️ **`SimStateDelta` 本批不接线**：契约 v2 冻结了它的**形状**，但本
//! 项目反复吃过「没有生产者的接口 = 死接口」的亏（台账 D-66）。它的
//! 生产者（谁在什么时候产生增量）等 M-10 的增量订阅落地后再定——
//! 在那之前接线只会造出一个「有形状、没内容」的变体。
//!
//! ## 契约 v3 的四个调试面命令（M-11 · STAGE-5）
//!
//! 冻结规格：`docs/detailed-design/STAGE-5_Symbol_Disasm_Trace.md`。
//!
//! | 命令 | 内核行为 | 回复事件 |
//! | --- | --- | --- |
//! | `Disasm{addr,count}` | M-11 的 `Disassembler`；取指回调接 M-14 的总线 | `Disasm{entries}` |
//! | `ResolveLine{addr}` | M-11 的 `DwarfLines`（Wave 1 = 只识别不解析） | `SourceLocation{addr,file,line}` |
//! | `ExportTrace{source,format,offset,limit}` | **宿主路由**：`Waveform` ⇒ M-19 的环；`Execution` ⇒ M-11 的轨迹环 | `TraceExport{..}` |
//! | `SetTraceEnabled{enabled}` | M-11 的轨迹环开关（裁决 H） | `TraceState{enabled,len,dropped}` |
//!
//! ⚠️ **这四个事件是「回复事件」，不经 topic 过滤**（STAGE-5 §9.1）：
//! 前端靠传输帧的 `reqId` 关联「命令 → 回复」⇒ **不需要先订阅**。
//! [`TOPIC_DISASM`] 等四个常量是**登记用**的（与前端 `EVENT_TOPIC`
//! 逐字相同），**不要**据此实现「订阅后才推送」。
//!
//! ⚠️ **两条闭环**（本模块是它们的落点）：
//!
//! - **D-51**：M-18 的访存 trace 环**只写不读** ⇒ 宿主在每个事件批次
//!   跑完后把环里的事件**立刻**转进 M-11 的轨迹环（**drain 与转移必须
//!   同时发生**——只 drain 会把 M-11 要用的历史冲掉）。
//! - **D-50**：波形导出的数据源是 **M-19 的环**，**不是** M-18 的边沿
//!   存储 ⇒ 否则 M-19 的环永远是「只写不读」。
//!
//! ## 会话级快照（STAGE-3）
//!
//! 冻结规格：`docs/detailed-design/STAGE-3_Session_Snapshot.md`。
//!
//! 宿主是**采集器**，M-20 是**记录器**：六段由本模块从各自的持有者那里
//! 收齐，再交给 [`crate::m20_snapshot::SnapshotManager::capture_session`]
//! 组装成契约层的 [`SessionSnapshotDto`]。
//!
//! ```text
//! 段 0 cp_meta     ← 调用方给的
//! 段 1 m12_engine  ← M-12 引擎（M-17 的 snapshot_full）
//! 段 2 m13_state   ← M-14 总线的内存页（RAM 内容）
//! 段 3 m14_map     ← M-14 总线的区域拓扑
//! 段 4 m15_periph  ← M-15 外设的内部状态（按 id 匹配）
//! 段 5 m16_intc    ← M-16 已拉起的 IRQ 行
//! ```
//!
//! ⚠️ **恢复顺序是规格 §3.7 的不变量，不可换**：校验 → 区域拓扑 → 内存页
//! → 外设状态 → IRQ 行 → 引擎段。页写不进不存在的区域；外设状态写不进
//! 未绑定的实例；引擎段**最后**还原（它会覆盖 `gpr` / `pc` / 虚拟时间）。
//!
//! ## 契约 v5 的 console 输入方向（STAGE-9 裁决 A）
//!
//! | 命令 | 内核行为 | 回复事件 |
//! | --- | --- | --- |
//! | `WriteConsole{bytes}` | 字节**逐个**进 UART0 的接收 FIFO ⇒ `LSR` 的 `RX_VALID` 置位 ⇒ 跑一次**同步器**（UART RX 就绪 **且** `IER` 的 RX 使能位开着 ⇒ PLIC 源 10 ⇒ M-16 的外部中断线 ⇒ guest 的 `mcause` = **`0x8000_000B`**） | `Tick{..}`（**至少一个**事件 —— 前端靠它落地 Promise） |
//!
//! ⚠️ **抬线是同步器的活，不是命令的活**：命令只负责「把字节放进去 +
//! 让同步器跑一次」⇒ **一条命令不产生两个真相**。
//!
//! ⚠️ **空字节是幂等的**：什么都不做，但**照样回复**。
//!
//! ⚠️ **不复用 `WriteMemory`**：那会绕过外设的语义层（不走 FIFO、不置
//! `RX_VALID`、不抬线）⇒ guest 读 `RHR` 拿到旧值而 `LSR` 说「没有数据」
//! ⇒ **自相矛盾**。
//!
//! ## 平台描述（v0.27 · D-123-B）
//!
//! 内存图**不再是源码里的两档常量**，而是**一份数据**：契约层的
//! [`riscv_sim_contract::platform::PlatformDescription`]（JSON 形态见
//! [`platform`] 模块里的三份**内置描述**）。
//!
//! | 约束 | 唯一落点 |
//! | --- | --- |
//! | 描述的**校验** | 契约层 `PlatformDescription::validate` |
//! | 器件种类 → 构造 | [`platform`] 模块的器件工厂（**一个 match**） |
//! | 外设**窗口大小** | 器件自己的 `Peripheral::size()` |
//! | 每区域**预算** | 描述的 `region_budget_bytes` |
//!
//! ⇒ 下面那些 `pub const`（地址、器件 id、波特率、频率、预算）**不再是
//! 宿主的取值来源** ⇒ 它们**保留**只为**公开 API 兼容** ⇒ 别再拿它们
//! 当「真相」：真相在 `platforms/*.json` 里。
//!
//! ⚠️ [`KernelHost::memory_profile`] 仍是**兼容视图**（按当前描述的 `id`
//! 投影，裁决 C）⇒ 真实 id 用 [`KernelHost::platform_id`]。

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;

use riscv_sim_contract::csr::IRQ_UART_RX_READY;
use riscv_sim_contract::error::{
    ErrorFamily, PlatformError, E_ENG_03_NO_PROGRESS, E_HMS_02_FATAL_TERMINATED,
    E_PLT_13_REVISION_CONFLICT, E_PLT_14_REGION_NOT_FOUND, E_PROTO_01_VERSION_MISMATCH,
    PLATFORM_FAMILY, PROTO_FAMILY,
};
use riscv_sim_contract::model::{
    Edge, ImageKind, ImageRef, ImageSource, Project, TraceEvent, TraceFormat, TraceSource,
    WaveformHistory, WaveformStream, WriteSize, SIM_STATE_SCHEMA_VERSION,
};
use riscv_sim_contract::platform::{PlatformDescription, WIRING_EVENT_RX_READY};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::{
    CommandPayload, Direction, EventPayload, HaltReason, MemoryMapOp, Message, Payload,
};
use riscv_sim_contract::session::{
    CheckpointMetaDto, M13StateDto, M14MapDto, M15PeriphDto, M16IntcDto, SessionSnapshotDto,
};
use riscv_sim_contract::snapshot::{EngineSnapshotDto, DEFAULT_INSTRUCTION_BUDGET};
use riscv_sim_contract::SCHEMA_VERSION;

use crate::m06_elf::{DefaultLoader, Kind as LoadKind, ParseLimits};
use crate::m11_symbol::disasm::fetch_instruction_code as m11_fetch_instruction_code;
use crate::m11_symbol::trace::{E_CTX_03_ILLEGAL_EXPORT_FORMAT, TRACE_FAMILY};
use crate::m11_symbol::{
    Disassembler, LineLookup, SymbolIndex, SymbolService, TraceBuffer, TRACE_CAP,
};
use crate::m14_bus::{Bus, BusWidth};
use crate::m15_peripheral;
use crate::m15_peripheral::clint::Clint;
use crate::m15_peripheral::plic::Plic;
use crate::m15_peripheral::uart::Uart16550;
use crate::m16_interrupt::InterruptController;
use crate::m17_backend::{SelfIssBackend, SharedInterruptPort};
use crate::m18_probe::{ChannelDef, ChannelKind, ProbeRegistry, SampleCfg, CH_GPIO0, CH_IRQ_PULSE};
use crate::m19_waveform::{ChannelMeta, ExportFilter, SharedPipeline, WaveformPipeline};
use crate::m20_snapshot::SnapshotManager;
use crate::m21_logging::{LogBuffer, LogEntry, LogLevel, LogQuery};
use crate::m22_hms::{DegradeAction, Health, HealthSignal, Hms, SignalCtx, SignalKind, SignalSrc};

/// GPIO 外设的基址（PL061 惯例）。
///
/// 与默认内存图的两段 RAM（`0x0000_0000` / `0x8000_0000`）都不重叠。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_GPIO_BASE: u32 = 0x4000_0000;

/// GPIO 外设的实例 id（同时是 M-18 通道的 source）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const GPIO_PERIPH_ID: &str = "GPIO0";

/// GPIO 的信号名（M-15 `gpio::SIGNAL_OUT`）。
pub const GPIO_SIGNAL_NAME: &str = "OUT";

/// **demo 固件**（40 字节，RV32I 手写编码，小端）。
///
/// 这是**演示平台**的默认镜像，也是前端 `front/dev/demo-firmware.js`
/// 里那份的**同一个程序**——两边必须**逐字节相同**，否则前端看到的和
/// 内核跑的不是同一个程序（而两边各自都有测试，绿着测不同的固件）。
///
/// ```text
/// 0x00  addi x1, x0, 42
/// 0x04  addi x2, x0, 7
/// 0x08  add  x3, x1, x2
/// 0x0c  sw   x3, 0x100(x0)
/// 0x10  jal  x0, 0x14
/// 0x14  lui  x5, 0x40000      ← x5 = GPIO 基址
/// 0x18  addi x6, x0, 1
/// 0x1c  sw   x6, 4(x5)        ← GPIO0.OUT = 1（探针边沿，D-46）
/// 0x20  addi x4, x0, 1        ← 断点靶点
/// 0x24  jal  x0, 0            ← 自环
/// ```
///
/// ⚠️ **单一权威副本**：Rust 侧**只有这一份**——`kernel_host` 的单元测试
/// 与 `crates/kernel/tests/d46_probe_pipeline.rs` 都引用它，不各自抄一份。
/// 之前抄了三份（JS + 两份 Rust），改一份另两份照旧通过，正是「三份副本
/// 各自有测试」这种最坏的漂移形态。
///
/// 防漂移的用例在 `crates/kernel/tests/d46_probe_pipeline.rs`：它**直接读**
/// `front/dev/demo-firmware.js` 的字节数组来比对。
pub const DEMO_FIRMWARE_BYTES: [u8; 40] = [
    0x93, 0x00, 0xa0, 0x02, 0x13, 0x01, 0x70, 0x00, 0xb3, 0x81, 0x20, 0x00, 0x23, 0x20, 0x30, 0x10,
    0x6f, 0x00, 0x40, 0x00, 0xb7, 0x02, 0x00, 0x40, 0x13, 0x03, 0x10, 0x00, 0x23, 0xa2, 0x62, 0x00,
    0x13, 0x02, 0x10, 0x00, 0x6f, 0x00, 0x00, 0x00,
];

/// 波形通道的显示标签。
pub const GPIO_SIGNAL_LABEL: &str = "GPIO0.OUT";

/// GPIO `OUT` 信号的位宽（32 个引脚 → 32 位位图）。
///
/// 与 M-18 LLD 表 68 的「valueWidth = 按端口位宽」一致。
pub const GPIO_VALUE_WIDTH: u8 = 32;

/// **中断脉冲**通道（M-16 → M-18，接缝 #5 / 台账 D-26）的 source id。
///
/// M-18 的流 id 是 `"{source}.{signal}"` ⇒ 这条通道的 id 是
/// `"M16.irq_pulse"`。
pub const IRQ_PULSE_SOURCE: &str = "M16";

/// **中断脉冲**通道的信号名。
pub const IRQ_PULSE_SIGNAL: &str = "irq_pulse";

/// **中断脉冲**通道的显示标签（与 M-18 那一侧逐字相同）。
pub const IRQ_PULSE_LABEL: &str = "M16.irq_pulse";

/// **中断脉冲**通道的值宽（脉冲的「值」是中断线号 ⇒ 64 位足够）。
pub const IRQ_PULSE_VALUE_WIDTH: u8 = 64;

/// 订阅主题：寄存器快照。
///
/// 契约里**没有**寄存器快照事件（D-32 缺口），所以寄存器值只能走
/// `EventPayload::WaveformHistory` 的边沿编码流——这是契约内**唯一**的
/// 载体。流 id 与 mock 内核逐字一致，前端视图零改动。
///
/// ⚠️ 契约 v2 之后，`EventPayload::Registers`（契约 v2 新增）才是
/// **一等公民**的寄存器载体；这条边沿流是**兼容路径**（前端视图仍
/// 靠它渲染），两者并存。
pub const TOPIC_REGISTERS: &str = "sim.registers";

/// 订阅主题：探针波形。
pub const TOPIC_WAVEFORM: &str = "sim.waveform";

// ---------------------------------------------------------------------------
// 契约 v3 的四个调试面主题（STAGE-5 §3.5 / §9.6）
// ---------------------------------------------------------------------------
//
// ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC` 逐字相同**
// ——Rust 侧没有集中的「变体 → topic」映射表（唯一真相在前端），两侧的
// 一致性靠「同一个字面量 + 各自被测试钉住」保证。
//
// ⚠️ **登记用，不是订阅开关**：这四个事件是**命令的回复**（STAGE-5
// §9.1），前端靠传输帧的 `reqId` 关联 ⇒ **不需要先订阅**。不要据此
// 实现「订阅后才推送」——那会让前端等不到回复。

/// 主题：反汇编序列（`EventPayload::Disasm` 的回复）。
pub const TOPIC_DISASM: &str = "debug.disasm";

/// 主题：地址 → 源码行（`EventPayload::SourceLocation` 的回复）。
pub const TOPIC_SOURCE: &str = "debug.source";

/// 主题：导出块（`EventPayload::TraceExport` 的回复）。
pub const TOPIC_EXPORT: &str = "debug.export";

/// 主题：轨迹记录状态（`EventPayload::TraceState` 的回复）。
pub const TOPIC_TRACE: &str = "debug.trace";

/// console 输出的 topic（与前端 `EVENT_TOPIC.Console` **逐字一致**）。
pub const TOPIC_CONSOLE: &str = "sim.console";

// ---------------------------------------------------------------------------
// 契约 v6 的板级内存图主题（STAGE-14 §1.8 · §3.2）
// ---------------------------------------------------------------------------

/// 主题：板级内存图（`EventPayload::MemoryMap`）。
///
/// ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC.MemoryMap`
/// 逐字相同**（= `"memory.map"`）⇒ 前端订阅它才收得到那张图。
///
/// ⚠️ 它归 **`memory.*` 族**（与 `memory.read` 同族）—— 这是事件主题族
/// 的既有命名法（STAGE-14 §1.8），不是 `sim.*`。
pub const TOPIC_MEMORY_MAP: &str = "memory.map";

/// 宿主侧保留的 console 字节上限（**还没推给前端**的那些）。
///
/// ⚠️ **为什么要有上限**：UART 的捕获环是 4 KiB，满了会**丢最新**的
/// 字节 ⇒ 宿主必须**及时**把字节取走，否则 console 会静默丢字。
/// 而前端可能还没订阅 ⇒ 宿主得先攒着，攒多少得有个上限（丢**最旧**的，
/// 因为最近发生的事更有用）。
pub const CONSOLE_BACKLOG_MAX: usize = 64 * 1024;

/// 场景码：M-19 的波形管道**不可用**（`RefCell` 争用 / 尚未装配）。
///
/// ⚠️ 这是**内部瞬时**状态，不是客户端的请求有错 ⇒ 它**不**用 `E_CTX-03`
///（那是「请求参数 / 组合不合法」）。
///
/// ⚠️ **场景号是 2**：场景 1 已经是 M-19 的「通道配置非法」
/// （`m19_waveform::channel::invalid_config`）——一个场景码一个真相，
/// 不能两条不同的语义共用一个码。
pub const E_OBS_02_PIPELINE_UNAVAILABLE: u16 = 2;

/// 默认内存图 · 低段 RAM 的基址（复位向量）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_LOW_BASE: u32 = 0x0000_0000;

/// 默认内存图 · 低段 RAM 的大小（1 MiB）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_LOW_SIZE: u32 = 0x0010_0000;

/// 默认内存图 · 高段 RAM 的基址（常规 RISC-V 内存，与 golden 对拍侧一致）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_HIGH_BASE: u32 = 0x8000_0000;

/// 默认内存图 · 高段 RAM 的大小（4 MiB）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_HIGH_SIZE: u32 = 0x0040_0000;

// ------------------------------------------------------------------
// v0.23 批 · QEMU virt 兼容档（真实 RTOS 的落地形态）
// ------------------------------------------------------------------

/// QEMU virt 的 UART0 基址。
///
/// ⚠️ 与 osz 的 `platform/qemu_virt/include/platform.h` 里的
/// `UART0_BASE_ADDR` **逐字一致**（0x1000_0000）⇒ 两边零改动。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_UART0_BASE: u32 = 0x1000_0000;

/// QEMU virt 的 CLINT 基址（osz 的 `CLINT_BASE_ADDR`）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_CLINT_BASE: u32 = 0x0200_0000;

/// QEMU virt 的 PLIC 基址（osz 的 `PLIC_BASE_ADDR`）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_PLIC_BASE: u32 = 0x0C00_0000;

/// QEMU virt 的 RAM 起点（ITCM）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QEMU virt 的 RAM 长度（**128 MiB** = ITCM 60M + INIT 4M + DTCM 64M）。
///
/// ⚠️ 这 128 MiB **不是**「随便给的大一点」——osz 的链接脚本把三个区
/// 排成 `ITCM(0x8000_0000,60M)` / `INIT(0x83C0_0000,4M)` /
/// `DTCM(0x8400_0000,64M)`，而它的 best-fit 分配器
/// （`kernel/base/mem/bestfit/src/bestfit.c`）**会在堆尾写一个节点**：
/// `end_node = (MEM_NODE_INFO *)((UINT8 *)DEFAULT_HEAP_END - MEM_NODE_INFO_SIZE)`
/// ⇒ 堆尾是 `0x8800_0000` 附近 ⇒ **必须**真的映射到那里。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_RAM_SIZE: u32 = 0x0800_0000;

/// QEMU virt 档的**每区域护栏预算**（v0.26 · STAGE-11 裁决 B / G）。
///
/// ⚠️ **为什么它必须存在**：M-14 的 `max_region_bytes`（D-10 的防 OOM
/// 护栏）**默认**是 16 MiB ⇒ 声明一段 128 MiB 的 RAM 会被**挡下**
/// （`E_BUS-09`）⇒ 宿主必须**按自己的内存图**把预算抬到它声明的大小。
///
/// ⚠️ **它不是「护栏没了」**：护栏**还在**、**没被删**，只是它的**数值**
/// 由宿主的内存图决定（`Bus::with_max_region_bytes` / `set_max_region_bytes`
/// 的文档一直写着「use this constructor only when the host has its own
/// budget policy」——这句话在 v0.26 之后**才真正成立**）。
///
/// ⚠️ **Default 档不抬**：它那两段 RAM 只有 1 MiB / 4 MiB ⇒ 保持默认的
/// 16 MiB ⇒ 护栏在默认档上**一字未变**。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_MAX_REGION_BYTES: u64 = QEMU_VIRT_RAM_SIZE as u64;

/// ⚠️ **已退役**（v0.26 · STAGE-11 裁决 G）：单个 RAM 区域的大小（16 MiB）。
///
/// ⚠️ **为什么退役**：它存在**只因为** M-14 的每区域上限是 16 MiB ⇒
/// 128 MiB **必须**拆成 8 个区域。v0.26 之后 QEMU virt 档是**一个**
/// 128 MiB 的区域 ⇒ 这个数**不再**是「一个区域的大小」⇒ 留着它就是
/// 留一个**陷阱**（谁再拿它去映射就又回到 8 段）。
///
/// ⚠️ **保留而不删**：它是 `pub` 常量（crate 的公开 API）⇒ 删掉是破坏性
/// 变更 ⇒ 这里只**标退役**并说明**为什么**。
pub const QEMU_VIRT_RAM_REGION: u32 = 0x0100_0000;

/// QEMU virt 的时钟频率（**10 MHz**）。
///
/// ⚠️ 与 osz 的 `SYSTEM_BASE_FREQ`（10 000 000）**逐字一致**；CLINT 的
/// `mtime` / `mtimecmp` 单位是**周期**，换算必须用这个数。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_FREQ_HZ: u64 = 10_000_000;

/// PLIC 的机器上下文号（osz 用 hart0 的 machine context 0）。
pub const PLIC_CONTEXT_MACHINE: u32 = 0;

/// UART0 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const UART0_PERIPH_ID: &str = "UART0";

/// CLINT 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const CLINT_PERIPH_ID: &str = "CLINT";

/// PLIC 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const PLIC_PERIPH_ID: &str = "PLIC";

/// UART0 的波特率。
///
/// ⚠️ 平台的 UART 是**捕获环**（写进环就是「已发出」），波特率**不参与**
/// 时序计算；这个值只是让 `ps_per_bit` 有个确定的分母。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const UART0_BAUD_RATE: u32 = 115_200;

/// 宿主的**内存图档位**。
///
/// ⚠️ **为什么不是一档**：QEMU virt 档要铺 **128 MiB** 的地址空间
/// ⇒ 每个宿主实例都要**登记**那么大一段。
///
/// ⚠️ **v0.26 · STAGE-11 裁决 A 之后登记成本 ≈ 0**：M-14 的 RAM 改成
/// **页级懒分配**（4 KiB 页、**写时**分配、读没写过的页回 0）⇒ 登记
/// 一段 128 MiB 的 region **不占物理内存** ⇒ 「默认档不铺 128 MiB」现在
/// 是为了**少造一条总线**，不再是为了**省内存**。
///
/// ⚠️ 在裁决 A 落地**之前**（旧 M-14 的 `map_ram` 用 `vec![0u8; size]`）
/// 那 128 MiB 是**真的**物理内存 ⇒ 本仓有上千条用例、每条都可能造一个
/// 宿主 ⇒ 默认档铺满会直接吃掉几个 GiB、测试跑不动 ⇒ 两档的分工保留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryProfile {
    /// 默认档：低段 1 MiB + 高段 4 MiB RAM + 一个 GPIO 外设。
    ///
    /// 这是**今天**的行为，一个字节都没变。
    #[default]
    Default,
    /// QEMU virt 兼容档：铺满 `0x8000_0000` 起的 128 MiB RAM，并挂上
    /// ns16550 UART0 / CLINT / PLIC 三个 QEMU virt 外设。
    ///
    /// ⚠️ 这是**真实 RTOS（osz）能跑起来**的那一档：osz 的地址全部落在
    /// `0x8000_0000..0x8800_0000`，且它的堆尾在 `0x8800_0000` 附近。
    QemuVirt,
}

/// 波形（寄存器流）的 schema 版本。
const WAVEFORM_SCHEMA_VERSION: u32 = 1;

/// 内核**授予**的能力位（契约 v2 · `Negotiated.cap_bits`）。
///
/// 语义取自 M-17 LLD §2.1.1 的 `capBitsSet`（**低 3 位** = 能力分级）：
///
/// | 位 | 含义 |
/// | --- | --- |
/// | bit0 | L1 —— 基础调试（运行 / 单步 / 断点 / 寄存器 / 内存） |
/// | bit1 | L2 —— 指令 / 访存钩子（探针、trace、覆盖率） |
/// | bit2 | L3 —— 快照 / 事件流检查点 |
///
/// 本仓库的内核是**自研 ISS**（`self_iss`），M-17 LLD 明确它协商到 **L3**
/// （全量位）。QEMU 后端（P1）会声明 L1 子集并把 L2 / L3 位清零。
pub const KERNEL_CAP_BITS: u64 = 0b111;

/// 内核描述串（契约 v2 · `Negotiated.kernel`）。
///
/// 格式 `"{crate}/{version} {ISA}"`，与契约规格给的例子
/// `"riscv_sim_kernel/0.1.0 RV32IMC"` 同形：版本 + ISA 串。
pub fn kernel_descriptor() -> String {
    format!(
        "{}/{} {}",
        env!("CARGO_PKG_NAME"),
        crate::VERSION,
        riscv_sim_contract::model::ISA
    )
}

/// **单个波形事件**里最多带多少条边沿。
///
/// 一条 8 MB 的事件（8 000 条边沿）会把 WASM 的 `postMessage` 与前端的
/// JSON 解析同时压垮——V-A 检视实测的量级。边沿条数必须有上限。
///
/// 截断**保留最新的**边沿（正在发生的事比历史重要），并计数
/// （[`KernelHost::waveform_edges_truncated`]）——不静默丢。
pub const MAX_WAVEFORM_EDGES_PER_EVENT: usize = 4096;

/// 场景码：中断控制器正被占用（`RefCell` 争用）。
///
/// ⚠️ **定义在 M-17**（[`crate::m17_backend::E_INT_01_CONTROLLER_BUSY`]），
/// 这里**转出**它：M-17 的 [`crate::m17_backend::SharedInterruptPort`] 是
/// **第一个**会真的撞上 `RefCell` 争用的地方（它在**指令边界**上借控制
/// 器，而同一批次里外设还会经 `InterruptSink::raise` 借**同一个**控制器）。
/// 一个场景码**只能有一份定义** —— 两处各写一个 `1`，将来漂移了没人知道。
///
/// ⚠️ 对外的路径一个字都没变：`riscv_sim_kernel::kernel_host::E_INT_01_CONTROLLER_BUSY`
/// 照旧可用。
pub use crate::m17_backend::E_INT_01_CONTROLLER_BUSY;

/// 宿主看门狗（v0.28 · 台账 **D-115** · 冻结规格 STAGE-13 §3.3）。
///
/// ⚠️ **定义在 [`watchdog`] 子模块**、这里**转出**：看门狗是**宿主侧**的
/// 东西（裁决 F），但它的**判定**是纯函数 ⇒ 谁都能单独测它。
///
/// 一个常量 / 一个判定**只能有一份定义** —— 两处各写一个 `200`，将来漂移了
/// 没人知道。
pub use watchdog::{
    classify_window, HostWatchdog, WindowOutcome, HOST_WATCHDOG_BUDGET_MS,
    HOST_WATCHDOG_CONFIRMATIONS, WATCHDOG_ERR_CODE, WATCHDOG_LOG_CTX,
};

/// 宿主内部的窗口判定结果（不是公开 API —— 见 [`watchdog::WindowVerdict`]）。
use self::watchdog::WindowVerdict;

/// 宿主单调时钟（v0.28 · D-115 的**修复**）—— **不能**直接用
/// `std::time::Instant`：它在 `wasm32-unknown-unknown` 上会 **panic**
/// ⇒ WASM 构建一启动就崩。详见 [`clock`] 模块。
use self::clock::{HostClock, HostInstant};

/// 会话标签（M-21 的 `session_id`）。
///
/// ⚠️ **MVP 是单会话** ⇒ 恒为 `0`（规格 §2.8 裁决 J）—— 如实登记，不编造
/// 一个「看起来像多会话」的数字。
const HOST_SESSION_ID: u32 = 0;

/// 中断汇与 M-16 控制器之间的**共享状态**。
///
/// ⚠️ **共享同一份**：[`HostSinks`]（引擎的 `InterruptSink`）与
/// [`KernelHost`]（会话载荷的采集 / 回灌）必须看到**同一个**控制器——
/// 两份就是**两个真相**（本项目反复踩过的坑：同一个量两个落点，恢复时
/// 必须挑赢家，输家默默腐烂）。
#[derive(Debug, Default)]
struct IntcLink {
    /// M-16 的中断控制器 —— 已拉起 IRQ 行的**唯一**落点。
    ///
    /// ⚠️ **多一层 `Rc`**：M-17 的执行器也要访问**同一个**控制器（采样 /
    /// 交付 / 清除），而执行器要的是 `&mut dyn InterruptPort` ⇒ 控制器得
    /// 住在 `Rc<RefCell<…>>` 后面，宿主把一份克隆交给 M-17
    /// （[`crate::m17_backend::SharedInterruptPort`]）。
    controller: Rc<RefCell<InterruptController>>,
    /// 委派失败（`RefCell` 争用）的累计次数。
    ///
    /// ⚠️ 委派**不许 panic**：M-14 的总线 hook 在 `RefCell` 争用下 panic
    /// 是本项目**已经修过**的坑（V-A 检视 F-3），同一个坑不许踩第二次。
    /// 争用时**记一次失败**并**返回 `Err`**——失败是**可见的**（这个
    /// 计数器），而且**响**的：返回 `Ok(())` 会让调用方以为这条 IRQ 行
    /// 拉起来了，其实它被丢了。
    failures: Cell<u32>,
}

/// 引擎事件汇（事件总线 + 中断汇）。
///
/// 引擎在调度循环里发布的 `Tick` 会被收集到这里。共享句柄让宿主能取走
/// 它们——第 1 阶段队列恒空，所以取走的是空列表，但接线外设之后这条路
/// 已经在了。
#[derive(Debug, Default, Clone)]
struct HostSinks {
    events: Rc<RefCell<Vec<EventPayload>>>,
    /// 与 [`KernelHost`] **共享**的 M-16 控制器（见 [`IntcLink`]）。
    intc: Rc<IntcLink>,
}

impl EventBus for HostSinks {
    fn publish(&mut self, evt: EventPayload) -> Result<(), PlatformError> {
        self.events.borrow_mut().push(evt);
        Ok(())
    }
}

impl InterruptSink for HostSinks {
    /// 拉起一条 IRQ 行 —— **委派**给 M-16 的控制器。
    ///
    /// ⚠️ 以前这里是**空实现**（`Ok(())`）——那正是会话载荷段 5
    /// （`m16_intc`）**恒为空**的直接原因。现在它真的接到一个控制器上，
    /// 于是「段 5 恒为空」的成因精确落在「M-15 没有生产路径」上，而不是
    /// 「intc 是个空壳」。
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.delegate(|intc| intc.raise(irq))
    }

    /// 撤下一条 IRQ 行 —— **委派**给 M-16 的控制器。
    fn lower(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.delegate(|intc| intc.lower(irq))
    }
}

impl HostSinks {
    /// 把一次中断操作委派给 M-16 的控制器。
    ///
    /// ⚠️ **拿不到就记一次失败并返回 `Err`**——绝不 `unwrap` /
    /// `borrow_mut` 硬取：争用下 panic 是本项目已修过的坑（V-A 检视
    /// F-3）。而且**必须响**：同一个失败条件在两条路径上必须是**同一种
    /// 结果**——委派路径原来返回 `Ok(())`（引擎以为这条 IRQ 拉起来了，
    /// 其实被丢了），而回灌路径返回 `Err`（`E_INT_01`）。「同一个量两个
    /// 真相」——下一个接线「M-15 pending IRQ → `intc.raise()`」的人会
    /// 直接踩上那个哑掉的口子，所以这里统一成**报错**。
    ///
    /// ⚠️ 失败**计数**保留（诊断用）：[`KernelHost::intc_delegation_failures`]
    /// 暴露累计次数。计数与返回值是**两件事**：计数器回答「发生过几次」，
    /// 返回值回答「这一次成没成」。
    fn delegate<F>(&self, op: F) -> Result<(), PlatformError>
    where
        F: FnOnce(&mut InterruptController) -> Result<(), PlatformError>,
    {
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => op(&mut intc),
            Err(_) => {
                self.intc
                    .failures
                    .set(self.intc.failures.get().wrapping_add(1));
                Err(PlatformError::new(
                    ErrorFamily::Interrupt,
                    E_INT_01_CONTROLLER_BUSY,
                    "中断控制器正被占用，这条 IRQ 行**没有**被拉起 / 撤下",
                ))
            }
        }
    }
}

/// 内核宿主 —— 一个会话的全部状态。
pub struct KernelHost {
    /// 执行后端（M-17）：它持有 M-12 引擎、M-13 ISS、M-14 总线与 M-08 的
    /// 断点 / 观察点表。
    backend: SelfIssBackend<HostSinks, HostSinks>,
    /// 引擎事件汇（与后端共享）。
    sinks: HostSinks,
    /// 已订阅的主题。
    topics: BTreeSet<String>,
    /// 出参事件的序号（内核自己的单调计数器，从 1 起）。
    seq: u64,
    /// 快照 id 计数器。
    snapshot_counter: u64,
    /// 上次**已发布**的寄存器值 —— 增量波形靠它做差。
    published_regs: [u32; 32],
    /// 上次已发布的 PC。
    published_pc: u32,
    /// 是否已经发布过（全量快照之后才为真）。
    published_valid: bool,
    /// M-18 探针注册表（D-46）—— 判沿 + 访存 trace 环。
    probes: Option<Rc<RefCell<ProbeRegistry>>>,
    /// M-19 波形管道（D-46）—— 边沿的环形缓冲（导出用）。
    waveform: Option<SharedPipeline>,
    /// 探针管道**装配失败**的累计次数（V-A 检视 · 🟡-6 / 🟡-7）。
    ///
    /// 装配期的 `define_channel` / `attach_signal` 失败原本被 `let _ =`
    /// 静默吞掉——通道没接上，波形视图就是空的，而**没有任何报错**。
    /// 现在失败是**可见的**：计数器 + [`KernelHost::probe_wiring_failures`]。
    probe_wiring_failures: u32,
    /// 发布波形事件时因**超出单事件边沿上限**而截断的条数
    /// （V-A 检视 · 🟠-1）。
    waveform_edges_truncated: u64,
    /// M-16 的中断控制器 —— 与 [`HostSinks`] **共享同一份**（STAGE-3 段 5）。
    intc: Rc<IntcLink>,
    /// M-20 的快照管理器 —— 会话载荷的**记录器**（STAGE-3）。
    ///
    /// ⚠️ 宿主**持有**它，但**不自己组装**载荷：六段由宿主采集，组装交给
    /// M-20（规格 §3.5）——这样 M-20 不依赖 M-14 / M-15 / M-16，三段可以
    /// 各自演进。
    snapshots: SnapshotManager,
    /// M-11 的反汇编器（按地址缓存 + 自修改代码失效，裁决 G）。
    disassembler: Disassembler,
    /// M-11 的索引：**符号表 + DWARF 行表**（`SymbolService` 端口的生产实现）。
    ///
    /// ⚠️ **装载工程时无条件重建**（STAGE-5 §9.2）：装载新工程 = 新会话，
    /// 旧工程的符号留着就是**两个真相**。
    ///
    /// ⚠️ **为什么合成一个索引**（STAGE-7 裁决 J）：端口的 `resolve_line`
    /// 必须能分清「查过行表、没命中」（`Missed`）与「没查」（`NoDwarf`）⇒
    /// 端口必须**同时**持有符号表与行表。
    index: SymbolIndex,
    /// M-11 的执行轨迹环（指令 + 访存）。
    ///
    /// ⚠️ 它是**访存历史的持有者**（D-51）：M-18 的环是**中转站**，
    /// 宿主每跑完一个批次就把 M-18 的环**转移**到这里 ⇒ M-18 的环不再
    /// 无界增长，而 M-11 要用的历史一条不少。
    trace: TraceBuffer,
    /// 被**丢弃**的 M-18 访存事件数。
    ///
    /// ⚠️ **可见的丢弃**（不静默）：被丢掉的事件有两类 —— ① `load_bytes`
    /// 逐字节写镜像产生的**宿主侧**噪声（不是访客的执行历史）；② trace
    /// 开关**关闭**期间的事件（「关闭 ⇒ 零记录」，裁决 H）。计数暴露给
    /// 调用方，免得「轨迹环里少了一批」变成一件没人知道的事。
    trace_events_discarded: u64,
    /// 当前生效的**平台描述**（v0.27 · D-123-B）。
    ///
    /// ⚠️ **它取代了「档位」**：内存图不再是源码里的两档常量，而是**一份
    /// 数据**（契约层的 `PlatformDescription`）⇒ 「换板子」不用改宿主
    /// 源码（§2.2）。
    ///
    /// ⚠️ [`KernelHost::memory_profile`] 仍然存在 —— 它是这份描述的**兼容
    /// 视图**（按 `id` 投影，裁决 C）⇒ 既有调用点一字不改。
    platform: riscv_sim_contract::platform::PlatformDescription,

    /// 用户配的板（**跨 `LoadProject` 保持**）—— STAGE-14 §2.1 裁决 A 的那个字段。
    ///
    /// ⚠️ **它就是「用户意图」本身**，不是「待生效的那一份」——
    /// 用户的板一旦配过，就**一直**在那儿 ⇒ `LoadProject` 始终优先用它建图
    /// （§2.2 的施工期订正）。
    ///
    /// ⚠️ **为什么只留一个字段**：初版写的是「待生效的那一份」+ 单独一个
    /// `pending` 标志 ⇒ 「配了板但没生效」与「标志忘了翻」是**两种不同的
    /// 错**，前者推导不出来、后者无从发现 ⇒ 「待生效」改成**推导量**
    /// （见 [`KernelHost::memory_map_pending`]）⇒ 少一个状态就少一处可能
    /// 不同步的地方。
    ///
    /// ⚠️ **不持久化**（裁决 N）：刷新页面即失 ⇒ MVP 单会话。
    user_board: Option<PlatformDescription>,
    /// 当前会话是否已经装载过镜像（STAGE-14 §2.3 裁决 C）。
    ///
    /// ⚠️ v0.31（STAGE-16）**补上了卸载命令** ⇒ 卸载之后改板**立即
    /// 生效**（D-139）⇒ 这个字段不再锁死「改板只能重新装载才生效」。
    image_loaded: bool,

    /// 板级内存图的**修订号**（v0.31 · STAGE-16 裁决 A）—— 宿主内存图
    /// 状态的**单调递增计数器**（初值 0）。
    ///
    /// ⚠️ **它住在宿主，不进 `PlatformDescription`** ⇒ 落库的板**不带**
    /// 修订号 ⇒ v0.30 的板档案条目形状（`{id, description}`）**一字不改**
    /// ⇒ D-143 的存量风险**不因本批增加**。
    ///
    /// ⚠️ **每次被接受**的内存图变更 **+1**：① `SetMemoryMap` 被接受
    /// ② `PatchMemoryMap` 被接受 ③ 装载工程时**待生效的板被应用**。
    ///
    /// ⚠️ **卸载不 +1**（卸载不改图，只改「有没有装载镜像」）。
    map_revision: u64,

    /// 最后一个**装载成功**的工程（v0.31 · STAGE-16 裁决 C 第 ④ 步）。
    ///
    /// ⚠️ **镜像字节留在宿主** ⇒ 卸载之后**不必重新选文件**就能重装。
    ///
    /// ⚠️ **卸载不许清它** —— 清了的话「重新装载工程」就得重新选文件，
    /// 裁决 C 第 ④ 步就落空了。
    last_project: Option<Project>,

    /// UART0 的句柄（**只有** QEMU virt 档才有）—— console 字节从这里取。
    uart0: Option<Rc<RefCell<Uart16550>>>,
    /// CLINT 的句柄（**只有** QEMU virt 档才有）—— `mtimecmp` 从这里喂给 M-16。
    clint: Option<Rc<RefCell<Clint>>>,
    /// PLIC 的句柄（**只有** QEMU virt 档才有）—— 外部中断的源头。
    plic: Option<Rc<RefCell<Plic>>>,
    /// **连线表**（v0.34 · D-125）—— 从**生效的描述**编译出来的。
    ///
    /// ⚠️ **它在建图时编译**（[`KernelHost::map_memory`]）⇒ ⇒ 换一块板
    /// 就是换一张表 ⇒ ⇒ 宿主源码里**没有**第二个「PLIC 源号」。
    ///
    /// ⚠️ **它不是第二个真相源**：表里的每一行都来自**生效的描述**
    /// （默认连线或描述里的 `wiring`）⇒ ⇒ 描述是真相，表是它的投影。
    wiring: Vec<platform::Wiring>,
    /// 已经从 UART 取走、但**还没推给前端**的 console 字节。
    ///
    /// ⚠️ **必须**及时把 UART 的字节取走（捕获环满了会丢最新的字节），
    /// 但前端不一定订阅了 console ⇒ 中间需要一个攒着的缓冲。
    console_backlog: Vec<u8>,
    /// M-22 健康管理（v0.28 · 台账 **D-115**）—— 看门狗触发的**处置**走它。
    ///
    /// ⚠️ **不提供 `Clone`**（M-22 持有 trait object 字段）⇒ 宿主**持有**
    /// 它，不复制它。
    hms: Hms,
    /// M-21 日志缓冲 —— 与 [`KernelHost::hms`] **共享同一份**。
    ///
    /// ⚠️ **一个真相**：`Hms::install_log_source` 收的是
    /// `Rc<RefCell<LogBuffer>>`（形状与 M-18 的探针表同款）⇒ 宿主持有
    /// `Rc`、把它**克隆**给 HMS ⇒ 管理员通道看到的日志关联与宿主记的日志
    /// 是**同一条**。存两份 `LogBuffer` 就是两个真相。
    log_buffer: Rc<RefCell<LogBuffer>>,
    /// 宿主看门狗（v0.28 · **D-115**）—— 两条驱动路径（`StepN` / `Continue`）
    /// 共用**同一个**状态（连续性是它存在的意义）。
    watchdog: HostWatchdog,
    /// 会话起点（宿主单调时钟的零点）—— M-21 / M-22 的 `time_host_ms` 口径。
    started_at: HostClock,
}

impl std::fmt::Debug for KernelHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The backend is not `Debug` (it owns a `dyn Peripheral` bus), so
        // the summary is written by hand — a one-line state dump is what
        // a log line needs, not the whole machine.
        f.debug_struct("KernelHost")
            .field("pc", &self.backend.engine().pc())
            .field("now_ps", &self.backend.engine().now_ps())
            .field("topics", &self.topics)
            .field("seq", &self.seq)
            // M-11 的四个服务对象（契约 v3 的调试面）——它们的状态是
            // 「会话里发生了什么」的一部分，日志行要看得见。
            .field("disasm_cache", &self.disassembler.cache_len())
            .field("symbols", &self.index.symbol_count())
            .field("dwarf", &self.index.dwarf_status())
            .field("trace_len", &self.trace.len())
            .field("trace_dropped", &self.trace.dropped())
            // v0.28 · D-115：看门狗与健康状态是「会话里发生了什么」的
            // 一部分 ⇒ 日志行要看得见（否则「内核为什么停了」查不出来）。
            .field("watchdog_streak", &self.watchdog.streak())
            .field("watchdog_fired", &self.watchdog.fired())
            .field("health", &self.hms.state())
            .finish()
    }
}

impl Default for KernelHost {
    fn default() -> Self {
        Self::new()
    }
}

impl KernelHost {
    /// 创建一个空会话（内存图已建立，尚未加载任何镜像）。
    ///
    /// ⚠️ 今天的默认档 = 内置描述 `default`（v0.27 · D-123-B）⇒
    /// **一个字节都没变**，只是那段「档位」变成了**一份数据**。
    pub fn new() -> Self {
        // ⚠️ 内置描述是**编译期常量**（`include_str!`）⇒ 它的合法性由
        // 用例钉住（`tests_platform.rs`）⇒ 这里取不到只可能是「发布的
        // JSON 坏了」⇒ 那时**必须有声音**，不能静默。
        let description = platform::builtin_description(platform::PLATFORM_ID_DEFAULT)
            .expect("内置平台描述 default 必须是合法的（由 tests_platform.rs 钉住）");
        Self::with_description(description)
    }

    /// 按**平台描述**造一个会话（描述 → 建图）。
    ///
    /// ⚠️ **建图失败 ⇒ 没有宿主**（返回 `Err`）⇒ 不会留下一个「半个平台」。
    fn with_description(description: PlatformDescription) -> Self {
        // ⚠️ 中断控制器**先**造出来，再交给 `HostSinks`——两者**共享同一份**
        // （`Rc` 克隆），否则引擎拉起的 IRQ 行与宿主导出的段 5 就是两个真相。
        let intc = Rc::new(IntcLink::default());
        let sinks = HostSinks {
            events: Rc::new(RefCell::new(Vec::new())),
            intc: Rc::clone(&intc),
        };
        // ⚠️ **M-21 与 M-22 共享同一份日志缓冲**（v0.28 · D-115）：
        // 宿主持有 `Rc`、把它**克隆**给 HMS ⇒ 管理员通道的「日志关联」
        // 与宿主记的日志是同一条 ⇒ 两份缓冲就是两个真相。
        //
        // ⚠️ **角色默认 `Product`**（规格 §2.8 裁决 J）⇒ 产品 / 嵌入式
        // 角色**看不见**原始日志（角色边界）。
        let log_buffer = Rc::new(RefCell::new(LogBuffer::new(HOST_SESSION_ID)));
        let mut hms = Hms::new();
        hms.install_log_source(Rc::clone(&log_buffer));
        let mut host = Self {
            backend: SelfIssBackend::new(sinks.clone(), sinks.clone()),
            sinks,
            topics: BTreeSet::new(),
            seq: 0,
            snapshot_counter: 0,
            published_regs: [0; 32],
            published_pc: 0,
            published_valid: false,
            probes: None,
            waveform: None,
            probe_wiring_failures: 0,
            waveform_edges_truncated: 0,
            intc,
            snapshots: SnapshotManager::new(),
            // M-11 的四个服务对象（契约 v3 的调试面）。
            //
            // ⚠️ `TraceBuffer` 的 `enabled` **默认 true**（STAGE-5 §9.8
            // 的裁决）：D-51 要求访存 trace「有消费者」，默认不写会让导出
            // 永远是空的。用户可以用 `SetTraceEnabled(false)` 关掉它。
            //
            // ⚠️ `dwarf` 是 `Option`：`None` = 「这个会话没有 DWARF」
            // （降级态，`file=""` / `line=0`）。**空会话**还没有装载任何
            // 镜像 ⇒ 确实没有 DWARF —— 不必为了造一个「空的行表」而
            // `unwrap` 一个永远走不到的错误分支（WASM 里 panic = Worker 崩）。
            disassembler: Disassembler::new(),
            index: SymbolIndex::default(),
            trace: TraceBuffer::new(TRACE_CAP),
            trace_events_discarded: 0,
            platform: description.clone(),
            // STAGE-14 §2.1 裁决 A：用户配的板住在**宿主会话内** ⇒ 新会话
            // 一开始**没有**配过板。
            user_board: None,
            // STAGE-14 §2.3 裁决 C：新会话**还没装载过**镜像。
            image_loaded: false,
            // STAGE-16 裁决 A：修订号**从 0 起**。
            map_revision: 0,
            // STAGE-16 裁决 C 第 ④ 步：新会话**还没有**装载过的工程。
            last_project: None,
            uart0: None,
            clint: None,
            plic: None,
            // ⚠️ v0.34（D-125）：连线表在**建图时**编译（`map_memory`）⇒
            // 这里只是**初值**（还没有建图 ⇒ 还没有表）。
            wiring: Vec::new(),
            console_backlog: Vec::new(),
            hms,
            log_buffer,
            watchdog: HostWatchdog::new(),
            started_at: HostClock::new(),
        };
        // The default memory map cannot fail on a fresh bus (the regions do
        // not overlap and are far below the per-region budget). If it ever
        // does, the failure is reported by the next `LoadProject` — which
        // re-maps the memory anyway.
        let _ = host.map_memory(&description);
        // The interrupt controller reaches **two** places: the engine's
        // `InterruptSink` (peripherals *raise* IRQ lines) and M-17's
        // executor (quantum-boundary *sampling* / pull-style *delivery* /
        // `irq_clear` side effect). Both must see the **same** controller —
        // two controllers would be two truths.
        host.install_interrupt_port();
        // The time-driven link (D-07) also needs to be connected here:
        // whoever hangs the peripheral onto the bus is also the one who
        // registers it with the engine — otherwise `PeripheralTick` is
        // always a no-op and the peripheral's clock never advances.
        host.install_peripheral_ticker();
        host.install_probe_pipeline();
        // 外设的探针回调**也要在这里装**——`KernelHost::new()` 造出来的
        // 宿主已经有一张默认内存图（里面有一个 GPIO），它上面的写必须
        // 能产生边沿。
        //
        // ⚠️ 漏掉这一步的后果很隐蔽：所有**先 `LoadProject`** 的用法都
        // 走 `rebuild_platform()`（它会重装一遍），所以缺口一直没人看见；
        // 只有「刚建好、还没装载工程」的宿主会**静默**观察不到任何边沿。
        host.attach_peripheral_probes();
        host
    }

    /// 按**平台 id** 显式建图（内置描述）。
    ///
    /// ⚠️ **这是「换平台」的主入口**：换一份内置描述 ⇒ 内存图整个换掉，
    /// 宿主**零逻辑改动**。
    ///
    /// ⚠️ 描述**不合法 ⇒ 没有宿主**（`Err`）⇒ 不会留下「半个平台」。
    pub fn new_with_platform(id: &str) -> Result<Self, PlatformError> {
        let description = platform::builtin_description(id)?;
        Ok(Self::with_description(description))
    }

    /// 按**文件路径**加载描述并建图（运行时路径 —— 裁决 F）。
    ///
    /// ⚠️ 与内置路径**共用同一个解析器 + 同一个校验器**（§2.5）⇒ 没有
    /// 两份语义。
    ///
    /// ⚠️ 描述文件**不合法 ⇒ 没有宿主**（`Err`）。
    pub fn new_with_platform_file(path: &std::path::Path) -> Result<Self, PlatformError> {
        let description = platform::description_from_file(path)?;
        Ok(Self::with_description(description))
    }

    /// 装上 M-16 的中断控制器**端口**（接缝 #2 / #3 / #4）。
    ///
    /// 宿主是**唯一**知道「引擎那条 `InterruptSink` 通路」与「M-17 的
    /// 采样 / 交付通路」必须看到**同一个**控制器的地方——它把同一个
    /// `Rc<RefCell<InterruptController>>` 的两份引用分别交给两边。
    ///
    /// ⚠️ 幂等：重复调用只是换掉后端里的端口盒子，控制器还是**同一个**。
    fn install_interrupt_port(&mut self) {
        self.backend
            .install_interrupt_port(Box::new(SharedInterruptPort::new(Rc::clone(
                &self.intc.controller,
            ))));
    }

    /// 创建一个**指定内存图档位**的会话。
    ///
    /// ⚠️ **默认档不铺 128 MiB** —— QEMU virt 档要铺的那 128 MiB 在
    /// **裁决 A 落地之前**是**真的**物理内存（旧 M-14 的 `map_ram` 用
    /// `vec![0u8; size]`）⇒ 每个宿主实例都吃 128 MiB。headless 跑真实
    /// RTOS 时显式用 [`MemoryProfile::QemuVirt`]。
    ///
    /// ⚠️ **裁决 A 之后**（RAM 改页级懒分配）那 128 MiB 的**登记成本
    /// ≈ 0**（只有写过的页才占物理内存）⇒ 两档的分工保留是为了**少造
    /// 一条总线**，不再是为了省内存。
    pub fn new_with_profile(profile: MemoryProfile) -> Self {
        // ⚠️ **兼容入口**（裁决 C）：`MemoryProfile` 是公开 API ⇒ 保留 ⇒
        // 它现在只做一件事——**把档位翻译成内置描述的 id** ⇒ 13 个既有
        // 调用点**一字不改**。
        let id = match profile {
            MemoryProfile::Default => platform::PLATFORM_ID_DEFAULT,
            MemoryProfile::QemuVirt => platform::PLATFORM_ID_QEMU_VIRT,
        };
        match Self::new_with_platform(id) {
            Ok(host) => host,
            Err(_err) => {
                // 换档失败 ⇒ 退回默认档并**记一次**（不 panic）。
                //
                // ⚠️ 一个刚建好的宿主上只有「默认档能建成」这一条不变式；
                // 真失败说明描述或总线出了问题 —— 退回默认档让调用方
                // 至少拿到一个能用的宿主。
                let mut host = Self::new();
                host.probe_wiring_failures += 1;
                host
            }
        }
    }

    /// 建立内存图（按**描述**）。
    ///
    /// ⚠️ **零逻辑改动**（B-6 的判别式）：这个函数里**没有**任何地址
    /// 字面量、没有任何器件构造调用、没有 `match profile` ⇒ 「换描述、
    /// 宿主零改动」在**源码层面**成立。
    ///
    /// ⚠️ 校验与建图**都**在 [`platform`] 模块里（校验器在**契约层**，
    /// 器件工厂在**宿主**）⇒ 这里只做「把描述交给它」这一件事。
    fn map_memory(&mut self, description: &PlatformDescription) -> Result<(), PlatformError> {
        // ⚠️ 借用的作用域要收窄：`bus` 是 `self.backend` 的可变借用，
        // 而下面要把新外设的句柄写回 `self` 的字段 —— 两个借用不能重叠。
        let devices = platform::map_platform(self.backend.bus_mut(), description)?;
        self.uart0 = devices.uart0;
        self.clint = devices.clint;
        self.plic = devices.plic;
        // ⚠️ **D-151**：把**同一个** CLINT 适配成**只读**时钟端口，装进后端
        // ⇒ ⇒ guest 读 `time` 与宿主调试读 `time` **同源**（STAGE-26 验收
        // B-9）⇒ ⇒ 平台时钟的真相**只有一份**（CLINT 自己那份）。
        //
        // ⚠️ **没有 CLINT 的板 ⇒ 没有时钟** ⇒ ⇒ `time` / `timeh` 报 **0**
        // ⇒ ⇒ `default` / `minimal` 两档与黄金 runner 的行为**逐字不变**。
        let time_source = self.clint.as_ref().map(platform::clint_time_source);
        self.backend.install_time_source(time_source);
        self.platform = description.clone();
        // ⚠️ **v0.34（D-125）**：**连线表在建图时从生效的描述编译**（冻结
        // 规格 STAGE-19 §3.1）⇒ ⇒ 「换一块板改连线」不用改宿主 ⇒ ⇒
        // 宿主抬线时读的是**这张表**，不是源码里的一个数。
        self.wiring = platform::compile_wiring(description);
        Ok(())
    }

    /// 按镜像**实际用到的地址**挑平台描述（裁决 E）。
    ///
    /// ⚠️ **为什么需要这一步**：默认档的高段 RAM 只有 4 MiB
    /// （`0x8000_0000..0x8040_0000`），而真实 RTOS（osz）的镜像落在
    /// `0x8000_0000`（代码）、`0x83C0_0000`（init）、`0x8400_0000`（数据 /
    /// bss / 栈 / 堆）三处 ⇒ 默认档**装不下**。
    ///
    /// ⚠️ **判据是「镜像用到的地址落在哪一档的映射范围内」**，不是
    /// 「镜像是不是某个特定的 ELF」—— 这样别的 RTOS 也能受益，而规则
    /// 只有一条、写在代码里。
    ///
    /// ⚠️ **判据本身在 [`platform`] 里**（描述化，裁决 E）：今天它查的是
    /// 「两档的**高段**窗口」；描述化之后它查的是「每个**参与自动选档**
    /// 的描述（`auto_selectable == true`）的**全部 RAM 区域**」，
    /// **区间在外层**逐个过，候选按 `priority` 小者优先，**无候选 ⇒ 回落
    /// `default`** ⇒ **与今天逐字等价**（B-9 的六条用例钉住）。
    fn choose_platform(&self, ranges: &[(u32, u32)]) -> Result<PlatformDescription, PlatformError> {
        platform::choose_description(ranges)
    }

    /// 把外设的中断源**同步**到 M-16（v0.23）。
    ///
    /// 三条通路，逐条说清楚：
    ///
    /// 1. **CLINT 的 `mtimecmp`（单位：周期）→ M-16 的定时器比较值
    ///    （单位：ps）**。M-16 的 `poll()` 自己管「`mtime >= mtimecmp`
    ///    ⇒ 挂起定时器线」，它只缺一个比较值 ⇒ 宿主每轮把 CLINT 的
    ///    `mtimecmp` 换算成 ps 喂给它。
    /// 2. **UART 的 RX 就绪 → PLIC 的源（**按连线表**）**。真实 RTOS 的
    ///    UART 中断是**外部中断**（经 PLIC 到 `MEIP`）⇒ 中断号落在 PLIC
    ///    上，不在 M-16 上。
    ///
    ///    ⚠️ **v0.34（D-125）**：那个源号**不再是源码里的字面量** ⇒ 它来自
    ///    **连线表**（默认连线或描述里的 `wiring`）⇒ ⇒ 换一块板改连线
    ///    **不用改宿主**。
    /// 3. **PLIC 的待交付源 → M-16 的外部中断线**。⚠️ **claim 是 guest
    ///    的权利**：平台这一侧**只读** PLIC 的「当前该交付哪个源」，
    ///    **不代 guest claim**（`Plic::claim` 不清挂起位，所以读它不会
    ///    抢走 guest 的源）。
    ///
    /// ⚠️ **这一段是「宿主唯一知道外设挂了哪些」的地方**（同
    /// [`SelfIssBackend::install_peripheral_ticker`] 的理由）—— 外设
    /// 本身不持有 M-16 的引用（M-15 LLD 的形状），不知道该往哪抬线。
    fn sync_interrupts(&mut self) {
        let Some(clint) = self.clint.clone() else {
            return;
        };
        let Some(plic) = self.plic.clone() else {
            return;
        };
        let now_ps = self.backend.engine().now_ps();

        // ---- 1) 定时器：CLINT 的 mtimecmp（周期）→ M-16（ps） ----
        //
        // ⚠️ `mtimecmp == 0` 在 CLINT 里表示「无期限」；M-16 里 `0` 也
        // 表示「定时器关闭」⇒ 两边**同义**，直接传过去即可。
        let cmp_ps = match clint.try_borrow() {
            Ok(c) => c.mtimecmp_ps(),
            Err(_) => return,
        };
        self.intc.controller.borrow_mut().set_timer_compare(cmp_ps);

        // ---- 2) UART 的 RX 就绪 → PLIC 的源（**按连线表**）----
        if let Some(uart) = self.uart0.clone() {
            let rx_ready = match uart.try_borrow() {
                Ok(u) => {
                    // LSR bit0 = RX 有字节；IER bit0 = 允许 RX 中断。
                    u.line_status() & 0x01 != 0 && u.interrupt_enable() & 0x01 != 0
                }
                Err(_) => return,
            };
            if rx_ready {
                if let Ok(mut p) = plic.try_borrow_mut() {
                    // ⚠️ **源号来自连线表**（v0.34 · D-125）⇒ ⇒ 表里是
                    // 「这个事件接到哪个源」⇒ ⇒ 换板改连线不用改宿主。
                    for wire in self.wiring.iter() {
                        // ⚠️ **事件名用契约层那个常量**（不写字面量）⇒ ⇒
                        // 事件名只有一处定义。
                        if wire.event != WIRING_EVENT_RX_READY {
                            continue;
                        }
                        p.request(wire.source);
                    }
                }
            }
        }

        // ---- 3) PLIC 的待交付源 → M-16 的外部中断线 ----
        //
        // ⚠️ 用**外部中断线**（`IRQ_UART_RX_READY` = 1）：平台的内部线号
        // 空间里 1 与 2 都映射到**架构 cause 码 11**（外部中断）⇒
        // guest 读到的 `mcause` 是 `0x8000_000B`，与 RISC-V 架构一致。
        let claimed = match plic.try_borrow_mut() {
            Ok(mut p) => p.claim(PLIC_CONTEXT_MACHINE),
            Err(_) => return,
        };
        if let Some(_source) = claimed {
            let _ = self.intc.controller.borrow_mut().request(
                IRQ_UART_RX_READY,
                now_ps,
                "plic",
                crate::m16_interrupt::TRIGGER_LEVEL,
            );
        }
    }

    /// 取走 UART0 **已经发出**的字节（console 输出）。
    ///
    /// ⚠️ **这是「guest 写进捕获环」的字节**，不是「平台写出去的字节」——
    /// 平台的 UART 是**捕获环**模型（Lane B），没有真正的串口线。
    ///
    /// ⚠️ **取走即清空**：调用方拿走的就是全部，顺序不变、不丢不重。
    ///
    /// ⚠️ **它先 `collect_console()` 再取 backlog** —— 因为 UART 的字节
    /// 可能已经被 [`Self::collect_console`] 收进 backlog 了（那是「及时
    /// 取走、免得捕获环满了丢字节」的要求）。只读 UART 的话会拿到空。
    pub fn drain_console(&mut self) -> Vec<u8> {
        self.collect_console();
        std::mem::take(&mut self.console_backlog)
    }

    /// **直接从 UART 的捕获环**取走字节（**不碰** backlog）。
    ///
    /// ⚠️ **为什么必须单独一个函数**：`collect_console`（收进 backlog）与
    /// `drain_console`（收走 backlog）都需要「从 UART 取字节」这一步。
    /// 如果两者都直接调 `drain_console`，而 `drain_console` 又要
    /// `collect_console` ⇒ **无限递归 ⇒ 栈溢出**。
    ///
    /// ⚠️ 这个坑是**真的踩过**的：接线用例写完、跑绿之后才把
    /// `drain_console` 改成「先 collect 再取 backlog」，release 构建当场
    /// 栈溢出（debug 构建的栈帧更大，反而先崩在别处）。
    fn drain_uart(&mut self) -> Vec<u8> {
        let Some(uart) = self.uart0.clone() else {
            return Vec::new();
        };
        // ⚠️ 绑到局部变量：`match` 的临时值会活到块尾，而 `uart` 在那
        // 之前就被 drop 了 ⇒ 借用检查器不通过。
        let out = match uart.try_borrow_mut() {
            Ok(mut u) => u.drain_tx(),
            // ⚠️ 争用 ⇒ **不 panic**，返回空并让调用方下一轮再取。
            Err(_) => Vec::new(),
        };
        out
    }

    /// console 方向**已经发出**但还没被取走的字节数。
    pub fn console_pending(&self) -> usize {
        let Some(uart) = self.uart0.as_ref() else {
            return 0;
        };
        let n = match uart.try_borrow() {
            Ok(u) => u.tx_pending(),
            Err(_) => 0,
        };
        n
    }

    /// 把 UART 的 TX 捕获环**取空**，攒进宿主的 backlog。
    ///
    /// ⚠️ **必须**及时取走：捕获环（4 KiB）满了会**丢最新的**字节 ⇒
    /// 不取走就是静默丢 console 内容。
    pub fn collect_console(&mut self) {
        let chunk = self.drain_uart();
        if chunk.is_empty() {
            return;
        }
        self.console_backlog.extend_from_slice(&chunk);
        if self.console_backlog.len() > CONSOLE_BACKLOG_MAX {
            // 丢**最旧**的（最近发生的事更有用）。
            let excess = self.console_backlog.len() - CONSOLE_BACKLOG_MAX;
            self.console_backlog.drain(0..excess);
        }
    }

    /// 造一个 `Console` 事件（**订阅了** console topic 才有）。
    ///
    /// ⚠️ **没订阅就返回 `None`** —— console 是**流**，不是命令的直接
    /// 回复；没订阅就不推（与波形事件同一条口径）。
    pub fn console_event(&mut self) -> Option<EventPayload> {
        if !self.topics.contains(TOPIC_CONSOLE) {
            return None;
        }
        // ⚠️ 先把 UART 里**还没收走**的字节收进 backlog —— 否则订阅的
        // 那一刻正好赶在「跑完那一批」与「收走」之间，就会漏掉。
        self.collect_console();
        if self.console_backlog.is_empty() {
            return None;
        }
        let bytes = std::mem::take(&mut self.console_backlog);
        Some(EventPayload::Console { bytes })
    }

    /// 已经执行的指令总数（headless runner 的预算判据）。
    pub fn total_executed(&self) -> u64 {
        self.backend.total_executed()
    }

    /// 当前生效的内存图档位（**兼容视图**，裁决 C）。
    ///
    /// ⚠️ 它是当前生效的**平台描述的 `id` 的投影**：`default` ⇒
    /// `Default`、`qemu-virt` ⇒ `QemuVirt`、**其它 id ⇒ `Default`**。
    ///
    /// ⚠️ **诚实但信息量低**：新平台（例如 `minimal`）会返回 `Default`
    /// ⇒ 想知道**真实**的平台请用 [`KernelHost::platform_id`]。
    ///
    /// ⚠️ 这个 API 是**公开**的（5 个调用点）⇒ 既有断言**一条不改**。
    pub fn memory_profile(&self) -> MemoryProfile {
        match self.platform.id.as_str() {
            platform::PLATFORM_ID_QEMU_VIRT => MemoryProfile::QemuVirt,
            _ => MemoryProfile::Default,
        }
    }

    /// 当前生效的**平台 id**（**真实**的 id 字符串）。
    ///
    /// ⚠️ 这是 v0.27 **新增**的接口：内存图不再只有两档 ⇒ 「档位」这个
    /// 概念已经装不下真实的信息 ⇒ 真实 id 由这里给出。
    pub fn platform_id(&self) -> &str {
        &self.platform.id
    }

    /// 当前生效的**连线表**（v0.34 · D-125 · STAGE-19 §3.1）。
    ///
    /// ⚠️ **它是从生效的描述编译出来的** ⇒ 换一块板就是换一张表 ⇒ ⇒
    /// 宿主源码里**没有**第二个「PLIC 源号」。
    ///
    /// ⚠️ **可观察**：B-5 用它证明「描述里的 `wiring` **覆盖**了默认连线的
    /// 同键项 ⇒ **不**出现两条连线」—— 那是**表**的性质，不是运行时碰巧
    /// 抬对了线。
    pub fn wiring_table(&self) -> &[platform::Wiring] {
        &self.wiring
    }

    /// 板级内存图的**修订号**（v0.31 · STAGE-16 裁决 A）。
    ///
    /// ⚠️ **单调递增**的计数器 ⇒ 前端提交时把它作为
    /// `expected_revision` 带上 ⇒ 并发丢更新**看得见**。
    pub fn map_revision(&self) -> u64 {
        self.map_revision
    }

    /// 最后一个**装载成功**的工程（v0.31 · STAGE-16 裁决 C 第 ④ 步）。
    ///
    /// ⚠️ **镜像字节住在它里面** ⇒ 卸载之后**不必重新选文件**就能重装。
    ///
    /// ⚠️ `None` = 这个会话**还没装载成功过**任何工程。
    pub fn last_project(&self) -> Option<&Project> {
        self.last_project.as_ref()
    }

    /// 只读地看总线（诊断 / 测试用）。
    pub fn bus(&self) -> &Bus {
        self.backend.bus()
    }

    /// 可变地看总线（装载镜像 / 测试用）。
    pub fn bus_mut(&mut self) -> &mut Bus {
        self.backend.bus_mut()
    }

    /// M-16 的**定时器比较值**（ps）—— guest 写下的 `mtimecmp` 换算后的值。
    ///
    /// ⚠️ `0` = 定时器关闭（M-16 的口径）。
    pub fn intc_timer_compare_ps(&self) -> u64 {
        match self.intc.controller.try_borrow() {
            Ok(c) => c.timer_compare(),
            Err(_) => 0,
        }
    }

    /// M-16 当前**挂起**的中断线（内部线号）。
    pub fn intc_pending_lines(&self) -> Vec<u16> {
        match self.intc.controller.try_borrow() {
            Ok(c) => c.pending_lines(),
            Err(_) => Vec::new(),
        }
    }

    /// 把外设的中断源**同步**到 M-16（宿主接线的公开入口）。
    ///
    /// ⚠️ 正常运行路径里它由 [`Self::on_run`] 自动调用；暴露出来是为了
    /// 让「宿主接线真的接上了」这件事**可以被单独观察**（台账教训：
    /// 「加了守卫」≠「守卫接上了」⇒ 要查它的**比较点**）。
    pub fn sync_interrupts_now(&mut self) {
        self.sync_interrupts();
    }

    /// 往 UART0 的**接收** FIFO 塞一个字节（宿主侧的 console 输入）。
    ///
    /// ⚠️ 平台的 UART 是**捕获环**模型：TX 方向是「guest 写进来、
    /// 宿主取走」；RX 方向是「宿主塞进去、guest 取走」。
    pub fn uart_receive(&mut self, byte: u8) {
        let Some(uart) = self.uart0.clone() else {
            return;
        };
        let done = match uart.try_borrow_mut() {
            Ok(mut u) => {
                u.receive(byte);
                true
            }
            Err(_) => false,
        };
        let _ = done;
    }

    /// 装上探针管道（D-46）：M-14 总线 → M-18 判沿 → M-19 环形缓冲。
    ///
    /// 装的是**四根线**：
    ///
    /// 1. M-14 的总线 hook → M-18 的访存 trace 环（M-11 导出用）；
    /// 2. M-18 → M-19 的 `EdgeSink` 握手（每条边沿同时进 M-19 的环）；
    /// 3. M-18 的通道定义（与 M-19 的通道号一一对应）；
    /// 4. M-19 的通道定义 + `SharedPipeline` 句柄。
    ///
    /// ⚠️ 必须在**总线建好之后**调用：`reset_memory_map`（装载新工程时）
    /// 会换上一条**全新的**总线，总线上挂的 hook 一并不见了——所以那里
    /// 也要重装一遍。
    fn install_probe_pipeline(&mut self) {
        let registry = Rc::new(RefCell::new(ProbeRegistry::new()));

        // M-19 的环形缓冲：边沿的长期历史 + CSV/VCD 导出。
        //
        // 通道元数据必须与 M-18 那一侧**一致**：GPIO 的 `OUT` 信号是
        // **32 位引脚位图**（M-15 `gpio::SIGNAL_OUT` 的定义），不是
        // 单个引脚的电平。所以它是 **multi-bit / 32 位**——M-18 LLD
        // 表 68 的「valueWidth = 按端口位宽」。把它当单比特通道，
        // 32 个引脚就只剩 pin 0 有信息了（M-15 的 `gpio.rs` 头注释
        // 明确写了这条）。
        let shared = SharedPipeline::new({
            let mut p = WaveformPipeline::new();
            p.define_channel(
                CH_GPIO0,
                ChannelMeta::multi_bit(GPIO_SIGNAL_LABEL, GPIO_VALUE_WIDTH),
            );
            // 中断脉冲通道（接缝 #5）—— 标签与 M-18 那一侧**逐字相同**
            // （M-18 的流 id 是 `"{source}.{signal}"`）。
            p.define_channel(
                CH_IRQ_PULSE,
                ChannelMeta::multi_bit(IRQ_PULSE_LABEL, IRQ_PULSE_VALUE_WIDTH),
            );
            p
        });

        // M-18 → M-19 握手：每条边沿同时进 M-19 的环形缓冲。
        registry
            .borrow_mut()
            .set_edge_sink(Some(Box::new(shared.clone())));

        // M-18 的通道定义（通道号与 M-19 的**必须**一致）。
        //
        // ⚠️ 失败**不能静默吞掉**（V-A 检视 · 🟡-6）：通道没接上时，
        // 波形视图就是空的，而宿主**一声不吭**。失败计入
        // `probe_wiring_failures`，由 [`KernelHost::probe_wiring_failures`]
        // 暴露。
        if registry
            .borrow_mut()
            .define_channel(ChannelDef::multi_bit(
                CH_GPIO0,
                GPIO_PERIPH_ID,
                GPIO_SIGNAL_NAME,
                GPIO_VALUE_WIDTH,
            ))
            .is_err()
        {
            self.probe_wiring_failures += 1;
        }

        // M-16 → M-18 的**中断脉冲**通道（接缝 #5 / 台账 D-26）。
        //
        // ⚠️ 通道**必须**注册成 `Pulse`：脉冲通道「每次观测都发」，
        // 同一根线上的重复请求**不合并**（多比特通道会把第二次相同值
        // 吞掉）。
        if registry
            .borrow_mut()
            .define_channel(ChannelDef::pulse(
                CH_IRQ_PULSE,
                IRQ_PULSE_SOURCE,
                IRQ_PULSE_SIGNAL,
            ))
            .is_err()
        {
            self.probe_wiring_failures += 1;
        }

        // 打开探针：master + 通道。
        let mut cfg = SampleCfg {
            master_enable: true,
            ..Default::default()
        };
        cfg.enable_channel(CH_GPIO0);
        cfg.enable_channel(CH_IRQ_PULSE);
        registry.borrow_mut().set_rate(cfg);

        // M-16 的脉冲回调 → M-18 的 `sample_irq_pulse`。
        //
        // ⚠️ **不许在回调里回头碰控制器**：回调是在控制器**借出**的状态下
        // 被调用的（`intc.request()` ⇒ `pulse_sink(now_ps, line)`），回头
        // 碰控制器就是**自引用借用** ⇒ `RefCell` 争用 ⇒ panic。
        //
        // ⚠️ 探针注册表被占用时**丢弃**这条脉冲（`try_borrow_mut`），不
        // panic：M-18 的访存 hook 也是这么处理争用的。
        let pulse_registry = Rc::clone(&registry);
        let sink = Box::new(move |now_ps: u64, line: u16| {
            let Ok(mut reg) = pulse_registry.try_borrow_mut() else {
                return;
            };
            let _ = reg.sample_irq_pulse(now_ps, u64::from(line));
        });
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => intc.set_pulse_sink(sink),
            Err(_) => {
                // 控制器正被占用 ⇒ 脉冲回调**没装上**：这一轮的脉冲探针
                // 不会有边沿。失败**可见**（计数器），不静默。
                self.probe_wiring_failures += 1;
            }
        }

        // M-14 总线 → M-18 的访存 trace 环。
        // `make_bus_hook` 自带争用处理：注册表被占用时丢弃这条访存并
        // 计数，**不阻塞**总线派发路径。
        self.backend
            .bus_mut()
            .add_hook(ProbeRegistry::make_bus_hook(Rc::clone(&registry)));

        self.probes = Some(registry);
        self.waveform = Some(shared);
    }

    /// 给已映射的外设装上探针回调（M-18 `attach_signal`）。
    ///
    /// 这一步把「外设状态变化」接到「M-18 的通道」上——波形流是从**这里**
    /// 来的，不是从总线访存来的（总线访存只进 trace 环）。
    fn attach_peripheral_probes(&mut self) {
        let Some(registry) = self.probes.clone() else {
            return;
        };
        for (_base, handle) in self.backend.bus().peripheral_instances() {
            // 外设正被总线派发路径占用时跳过——下一次 attach 会补上。
            //
            // ⚠️ 但「跳过」是**有代价**的：这一轮里该外设的写不会产生
            // 任何边沿，而宿主**不吭声**（V-A 检视 · 🟡-7）。失败计入
            // `probe_wiring_failures`。
            let Ok(mut periph) = handle.try_borrow_mut() else {
                self.probe_wiring_failures += 1;
                continue;
            };
            if periph.id() != GPIO_PERIPH_ID {
                continue;
            }
            if ProbeRegistry::attach_signal(
                Rc::clone(&registry),
                &mut *periph,
                GPIO_SIGNAL_NAME,
                CH_GPIO0,
                ChannelKind::MultiBit,
                GPIO_VALUE_WIDTH,
            )
            .is_err()
            {
                self.probe_wiring_failures += 1;
            }
        }
    }

    /// 重建整个平台：内存图 + 探针管道 + 外设探针回调。
    ///
    /// 装载新工程时走这里——新工程 = 新会话，内存图与探针管道都要重来。
    fn rebuild_platform(&mut self) -> Result<(), PlatformError> {
        self.backend.reset_memory_map();
        // ⚠️ 借用不能重叠（`&mut self` + `&self.platform`）⇒ 先取一份。
        let description = self.platform.clone();
        self.map_memory(&description)?;
        // 时间驱动链路（D-07）也要重建——新外设是新实例，旧的 tick
        // 事件指着旧实例。
        self.install_peripheral_ticker();
        self.install_probe_pipeline();
        self.attach_peripheral_probes();
        Ok(())
    }

    /// 装上外设 tick 接缝，并把总线上的外设登记到引擎里（D-07）。
    ///
    /// **只有宿主知道它往总线上挂了哪些外设**——这正是这一步必须住在
    /// 宿主、而不是后端里的原因：后端看不见宿主映射的外设。
    fn install_peripheral_ticker(&mut self) {
        self.backend.install_peripheral_ticker();
    }

    /// 取一批波形边沿，推成契约事件（D-46）。
    ///
    /// - `force = true`（订阅时）：即使还没有边沿也要推一次——前端要看到
    ///   **通道列表**，否则它不知道「有这条信号，只是还没变化」。
    /// - `force = false`（每轮运行后）：只在真有边沿时推。
    fn waveform_event(&mut self, force: bool) -> Option<EventPayload> {
        if !self.topics.contains(TOPIC_WAVEFORM) {
            return None;
        }
        let registry = self.probes.as_ref()?;
        let mut reg = registry.try_borrow_mut().ok()?;
        let mut wave = reg.drain_waveform();
        if wave.streams.is_empty() {
            return None;
        }
        if !force && wave.streams.iter().all(|s| s.edges.is_empty()) {
            return None;
        }
        // 单事件的边沿**总量**有上限（V-A 检视 · 🟠-1）。
        //
        // 一次 `drain_waveform()` 可能带上万条边沿——长时间没订阅时边沿
        // 会攒起来（虽然 M-18 那侧已经定容了，但攒的是**上限**那么多）。
        // 一条 8 MB 的事件会把 WASM 的 `postMessage` 与前端的 JSON 解析
        // 同时压垮。
        //
        // 截断**保留最新的**边沿（正在发生的事比历史重要），并计数
        // （`waveform_edges_truncated`）——不静默丢。
        let total: usize = wave.streams.iter().map(|s| s.edges.len()).sum();
        if total > MAX_WAVEFORM_EDGES_PER_EVENT {
            let per_stream = (MAX_WAVEFORM_EDGES_PER_EVENT / wave.streams.len().max(1)).max(1);
            let mut dropped = 0usize;
            for stream in wave.streams.iter_mut() {
                if stream.edges.len() > per_stream {
                    let excess = stream.edges.len() - per_stream;
                    dropped += excess;
                    let start = stream.edges.len() - per_stream;
                    stream.edges.drain(0..start);
                }
            }
            self.waveform_edges_truncated += dropped as u64;
        }
        Some(EventPayload::WaveformHistory(wave))
    }

    /// M-19 波形管道（D-46）—— 边沿的环形缓冲。
    ///
    /// 供「**M-18 → M-19 的握手真的在跑**」这类断言用：把
    /// `set_edge_sink` 那行去掉的话，环里的条数恒为 0，断言立刻变红
    /// （V-A 检视 · 🟠-1 —— 此前 M-19 在生产路径上是**死代码**，
    /// 删掉握手整套用例照样全绿）。
    pub fn waveform_pipeline(&self) -> Option<&SharedPipeline> {
        self.waveform.as_ref()
    }

    /// 探针管道**装配失败**的累计次数（V-A 检视 · 🟡-6 / 🟡-7）。
    ///
    /// 非零 = 有通道或回调没接上，波形视图会少东西——但宿主不会报错，
    /// 所以这个计数器是唯一的信号。
    pub fn probe_wiring_failures(&self) -> u32 {
        self.probe_wiring_failures
    }

    /// 因超出 [`MAX_WAVEFORM_EDGES_PER_EVENT`] 而被截断的边沿条数。
    ///
    /// 非零 = 有一条事件里的边沿**不全**（保留的是最新的那些）。
    pub fn waveform_edges_truncated(&self) -> u64 {
        self.waveform_edges_truncated
    }

    /// M-18 的边沿存储因达到**每通道上限**而丢弃的边沿条数。
    ///
    /// 非零 = 边沿时间轴上有一个**洞**（最旧的那批被淘汰了）。它与
    /// [`KernelHost::waveform_edges_truncated`] 是**两层**上限：这一层
    /// 管**内存**，那一层管**单条事件的大小**。
    pub fn probe_edges_dropped(&self) -> u64 {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.dropped_edge_count())
            .unwrap_or(0)
    }

    /// 当前总线上装了几个 bus hook。
    ///
    /// 正常情况下是 **2 个**：M-17 的观察点匹配器 + M-18 的访存 trace 环
    /// （D-46）。**换内存图会换总线**——新总线上的 hook 从零开始，
    /// 管道必须在上面重装一遍，否则访存 trace 环会**静默**收不到任何
    /// 访存（M-11 的数据源就断了）。
    pub fn bus_hook_count(&self) -> usize {
        self.backend.bus().hook_count()
    }

    // -- M-11 调试面的可观测性（契约 v3） -----------------------------------

    /// M-11 的反汇编缓存里有多少条目（按地址索引）。
    ///
    /// 用来钉住「**只**失效被写的那个地址」（裁决 G）：失效一个地址之后，
    /// 这个数应当**减少 1**，而不是清零。
    pub fn disasm_cache_len(&self) -> usize {
        self.disassembler.cache_len()
    }

    /// M-11 的反汇编缓存**命中**过多少次（`Disassembler::lookup`）。
    ///
    /// ⚠️ 规格 §9.4 说「`disasmCache` 可以兜住（访存轨迹那次重复取指）」
    /// ⇒ 这个计数是那句话的**可观测**证据：访存轨迹转移时若命中缓存，
    /// 宿主的**总线读次数**就少了一次。
    pub fn disasm_cache_hits(&self) -> u64 {
        self.disassembler.lookup_hits()
    }

    /// M-11 的符号表里有多少条符号。
    pub fn symbol_count(&self) -> usize {
        self.index.symbol_count()
    }

    /// 按**名字**查符号（走 M-11 的 [`SymbolService`] **端口**）。
    ///
    /// 未知名 ⇒ `None`（**不是错误**，验收 #7）。
    ///
    /// ⚠️ **走端口而不是直接摸 `SymbolTable`**（v0.20 检视 B · D-4）：端口
    /// 此前**全仓 0 处实现** ⇒ 它是**死代码**，「按地址查符号」这条能力
    /// 也**没有生产路径**。⇒ 宿主走端口 ⇒ 端口有实现者、也有调用者。
    pub fn lookup_symbol(&self, name: &str) -> Option<riscv_sim_contract::model::Symbol> {
        self.index.lookup_symbol(name).ok().flatten()
    }

    /// 按**地址**查符号（走 M-11 的 [`SymbolService`] **端口**）—— 命中
    /// 「起始地址 ≤ `addr` 且**最近**的那个」。
    ///
    /// 比所有符号的起始地址都小 ⇒ `None`（**不是错误**）。
    pub fn symbol_at(&self, addr: u32) -> Option<riscv_sim_contract::model::Symbol> {
        self.index.symbol_at(addr).ok().flatten()
    }

    /// M-11 的 DWARF 状态（`Absent` = 这个会话没有 DWARF）。
    pub fn dwarf_status(&self) -> crate::m11_symbol::DwarfStatus {
        self.index.dwarf_status()
    }

    /// M-11 的执行轨迹环里有多少条目。
    pub fn trace_len(&self) -> usize {
        self.trace.len()
    }

    /// M-11 的执行轨迹环累计丢弃了多少条（溢出）。
    pub fn trace_dropped(&self) -> u64 {
        self.trace.dropped()
    }

    /// 执行轨迹的记录开关（裁决 H）。
    pub fn trace_enabled(&self) -> bool {
        self.trace.is_enabled()
    }

    /// M-18 的访存 trace 环里现在有多少条目。
    ///
    /// ⚠️ 这是 **D-51 的闭环观测点**：宿主每跑完一个批次就把它**转移**进
    /// M-11 的环 ⇒ 跑完一批之后这个数应当回到 **0**（M-18 的环不再
    /// 无界增长）。
    pub fn probe_trace_len(&self) -> usize {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.trace_len())
            .unwrap_or(0)
    }

    /// M-18 的访存 trace 环**累计丢了多少条**（环溢出，丢最旧）。
    ///
    /// ⚠️ **非零 = 失效通道有损**（v0.20 检视 A · 重要 1）：这个环同时是
    /// **反汇编缓存的失效通道**（裁决 G：访存写事件 ⇒ 失效对应地址）⇒
    /// 写事件被丢掉 ⇒ 缓存条目不失效 ⇒ **反汇编给出旧机器码**。
    ///
    /// ⇒ 宿主把 `StepN` 按指令预算**分块**（[`KernelHost::step_in_chunks`]）
    /// ⇒ 单块事件数 ≪ 环的容量 ⇒ 这个数**恒为 0**。
    pub fn probe_trace_dropped(&self) -> u64 {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.trace_dropped())
            .unwrap_or(0)
    }

    /// M-18 的**边沿存储**里现在有多少条边沿。
    ///
    /// ⚠️ 这是 **D-50 的判别点**：M-18 的边沿存储与 M-19 的环是**两份**
    /// 数据。把 M-18 的存储抽干（订阅会推一次波形历史）之后，波形导出
    /// **仍然**有内容 ⇒ 它读的是 M-19 的环。
    pub fn probe_edge_count(&self) -> usize {
        self.probes
            .as_ref()
            .and_then(|reg| reg.try_borrow().ok())
            .map(|reg| reg.edge_count())
            .unwrap_or(0)
    }

    /// 被**丢弃**的 M-18 访存事件数（宿主侧的镜像写 + trace 关闭期间的事件
    /// + 切换开关时的清场）。
    ///
    /// 非零 = 有事件**没有**进 M-11 的执行轨迹环。丢弃**可见**，不静默。
    pub fn trace_events_discarded(&self) -> u64 {
        self.trace_events_discarded
    }

    // -- JSON 桥 ------------------------------------------------------------

    /// 处理一条契约消息的 JSON 文本，返回一批**事件**消息的 JSON 文本。
    ///
    /// 这是 WASM 桥的入口（`wasm_api::process_message`）。它**永不** panic：
    /// WASM 里 panic = 整个 Worker 崩，所以任何畸形输入都变成一条
    /// `E_PROTO-08` 错误事件。
    pub fn handle_json(&mut self, json: &str) -> String {
        let events = match serde_json::from_str::<Message>(json) {
            Ok(msg) => self.handle_envelope(msg),
            Err(err) => vec![EventPayload::Error {
                code: "E_PROTO-08".to_string(),
                message: format!("消息不是合法的契约信封：{err}"),
            }],
        };
        let messages: Vec<Message> = events.into_iter().map(|e| self.make_event(e)).collect();
        match serde_json::to_string(&messages) {
            Ok(text) => text,
            Err(err) => {
                let fallback = vec![Message::event(
                    0,
                    EventPayload::Error {
                        code: "E_PROTO-08".to_string(),
                        message: format!("事件序列化失败：{err}"),
                    },
                )];
                serde_json::to_string(&fallback).unwrap_or_else(|_| "[]".to_string())
            }
        }
    }

    /// 处理一条契约消息，返回它产生的事件。
    ///
    /// 这是**内核侧的核心入口**：`handle_json` 是它的 JSON 外壳。
    /// 调用方拿到的顺序与 `docs/detailed-design/STAGE-1_Wasm_Bridge.md`
    /// §3.1 的规范映射表一致。
    pub fn handle_message(&mut self, msg: &Message) -> Vec<EventPayload> {
        self.handle_envelope(msg.clone())
    }

    /// 校验信封并派发命令。
    fn handle_envelope(&mut self, msg: Message) -> Vec<EventPayload> {
        // 版本必须**完全**匹配，不能只拒低的：契约的 serde 派生没有
        // `deny_unknown_fields`，一个来自更高版本 schema 的消息会被
        // **静默地**少解析几个字段——那比直接拒绝糟得多。宁可拒掉，
        // 也不要 misinterpret（V-A 检视 · ⚪-6）。
        if msg.schema_version != SCHEMA_VERSION {
            let err = PlatformError::new(
                PROTO_FAMILY,
                E_PROTO_01_VERSION_MISMATCH,
                format!(
                    "协议版本不兼容：{} != {}",
                    msg.schema_version, SCHEMA_VERSION
                ),
            );
            return vec![self.error_from(&err)];
        }
        if msg.direction != Direction::Command {
            return vec![EventPayload::Error {
                code: "E_PROTO-08".to_string(),
                message: "内核只接受 Command 方向的消息".to_string(),
            }];
        }
        match msg.payload {
            Payload::Command(cmd) => self.dispatch(cmd),
            Payload::Event(_) => vec![EventPayload::Error {
                code: "E_PROTO-08".to_string(),
                message: "收到 Event 方向的消息".to_string(),
            }],
        }
    }

    /// 分配一个出参事件序号并包成契约消息。
    fn make_event(&mut self, payload: EventPayload) -> Message {
        self.seq = self.seq.wrapping_add(1);
        Message::event(self.seq, payload)
    }

    // -- 命令派发 -----------------------------------------------------------

    /// 把一条命令翻译成一串事件。
    ///
    /// 规范映射表见 `docs/detailed-design/STAGE-1_Wasm_Bridge.md` §3.1。
    fn dispatch(&mut self, cmd: CommandPayload) -> Vec<EventPayload> {
        match cmd {
            CommandPayload::Nop => vec![self.tick()],
            CommandPayload::LoadProject(project) => self.on_load_project(project),
            CommandPayload::Continue => self.on_run(None),
            CommandPayload::StepN { n } => self.on_run(Some(n)),
            CommandPayload::Pause => self.on_pause(),
            CommandPayload::SetBreakpoint(bp) => match self.backend.set_breakpoint_with_id(bp) {
                Ok(()) => vec![self.tick()],
                Err(err) => vec![self.error_from(&err)],
            },
            CommandPayload::ClearBreakpoint { id } => {
                // 幂等：id 不存在不是错误。
                let _ = self.backend.clear_breakpoint_by_id(id);
                vec![self.tick()]
            }
            CommandPayload::SetWatchpoint(wp) => match self.backend.set_watchpoint_with_id(wp) {
                Ok(()) => vec![self.tick()],
                Err(err) => vec![self.error_from(&err)],
            },
            CommandPayload::ClearWatchpoint { id } => {
                let _ = self.backend.clear_watchpoint_by_id(id);
                vec![self.tick()]
            }
            CommandPayload::ReadMemory { addr, size } => {
                match self.backend.read_memory(addr, size) {
                    Ok(value) => vec![EventPayload::MemoryRead { addr, size, value }],
                    Err(err) => vec![self.error_from(&err)],
                }
            }
            CommandPayload::ReadRegisters { index, count } => self.on_read_registers(index, count),
            CommandPayload::WriteRegister { index, value } => self.on_write_register(index, value),
            CommandPayload::Negotiate {
                min_schema_version,
                cap_bits,
            } => self.on_negotiate(min_schema_version, cap_bits),
            CommandPayload::WriteMemory { addr, size, value } => {
                match self.backend.write_memory(addr, size, value) {
                    Ok(()) => {
                        // ⚠️ 调试器写内存**也是**一次总线写 ⇒ M-18 的访存
                        // trace 环里多了一条写事件 ⇒ **两件事**跟着它走：
                        // ① 它要**立刻**进 M-11 的轨迹环（D-51：drain 与
                        //    转移必须同时发生）；
                        // ② 它要**失效**对应地址的反汇编缓存（裁决 G：
                        //    自修改代码）。
                        self.transfer_trace();
                        let mut events = vec![self.tick()];
                        // 调试器写内存**也会**产生探针边沿（写 GPIO 的
                        // OUT 寄存器就是改电平）。不推的话这条边沿要等到
                        // 下一轮 `Run` 才出去——用户盯着波形视图却看不到
                        // 自己刚写的那一下（V-A 检视 · 🟡-8）。
                        if let Some(wave) = self.waveform_event(false) {
                            events.push(wave);
                        }
                        events
                    }
                    Err(err) => vec![self.error_from(&err)],
                }
            }
            CommandPayload::Subscribe { topic } => self.on_subscribe(topic),
            // ⚠️ 契约 v6 的板级内存图（STAGE-14 §3.2）——用户在界面上配的
            // 那张板从这里进宿主。
            //
            // ⚠️ v0.31（STAGE-16）：`SetMemoryMap` **多一个**
            // `expected_revision`（`None` ⇒ 不检查），并**新增**两条命令
            // `PatchMemoryMap`（差量）与 `UnloadProject`（卸载）。
            CommandPayload::SetMemoryMap {
                description,
                expected_revision,
            } => self.on_set_memory_map(description, expected_revision),
            CommandPayload::PatchMemoryMap {
                ops,
                expected_revision,
            } => self.on_patch_memory_map(ops, expected_revision),
            CommandPayload::UnloadProject => self.on_unload_project(),
            CommandPayload::Unsubscribe { topic } => {
                self.topics.remove(&topic);
                vec![self.tick()]
            }
            CommandPayload::Snapshot => {
                self.snapshot_counter = self.snapshot_counter.wrapping_add(1);
                vec![EventPayload::SnapshotCreated {
                    id: self.snapshot_counter,
                }]
            }
            CommandPayload::Restore { state } => {
                // 快照的 `schema_version` 必须对得上，否则**拒绝**。
                // 契约的 `SimState` 带这个字段，而 `Engine::restore` 不
                // 校验它——一份版本错配的快照会把 PC 与整份寄存器换掉，
                // 而调用方连一个错都看不到（V-A 检视 · 🟡-6）。
                //
                // 错误码复用契约已有的 `E_SNAP-05`（不支持的状态 schema
                // 版本）——一场景一码。
                if state.schema_version != SIM_STATE_SCHEMA_VERSION {
                    vec![EventPayload::Error {
                        code: "E_SNAP-05".to_string(),
                        message: format!(
                            "SimState 的 schema_version 不支持：{} != {}",
                            state.schema_version, SIM_STATE_SCHEMA_VERSION
                        ),
                    }]
                } else {
                    match self.backend.restore(&state) {
                        Ok(()) => vec![self.tick()],
                        Err(err) => vec![self.error_from(&err)],
                    }
                }
            }
            CommandPayload::Admin { .. } => vec![EventPayload::Error {
                code: "E_CTX-05".to_string(),
                message: "管理通道未启用（admin op 不在当前会话的能力面内）".to_string(),
            }],
            // ⚠️ 契约 v3 的四个调试面命令（STAGE-5 §3.4 的规范映射表）。
            //
            // ⚠️ 这四个分支**曾经**是「故意返回错误」的占位（契约 v3
            // 到位、M-11 未接线的那个窗口期）。现在它们是**真实现**——
            // 占位分支已被删除（不留「有形状、没内容」的变体）。
            CommandPayload::Disasm { addr, count } => self.on_disasm(addr, count),
            CommandPayload::ResolveLine { addr } => self.on_resolve_line(addr),
            CommandPayload::ExportTrace {
                source,
                format,
                offset,
                limit,
            } => self.on_export_trace(source, format, offset, limit),
            CommandPayload::SetTraceEnabled { enabled } => self.on_set_trace_enabled(enabled),
            // ⚠️ 契约 v5 的 console **输入**方向（STAGE-9 裁决 A）。
            //
            // ⚠️ **不复用 `WriteMemory`**：写 UART 的寄存器地址在技术上
            // 就是「写内存」，但它**绕过外设的语义层**（不走接收 FIFO、
            // 不置 `LSR` 的 `RX_VALID`、不抬中断线）⇒ guest 读 `RHR`
            // 拿到**旧值**而 `LSR` 说「没有数据」⇒ **自相矛盾**。
            CommandPayload::WriteConsole { bytes } => self.on_write_console(bytes),
        }
    }

    /// 装载工程：复位 → 重建平台 → 加载镜像 → 换新会话（清订阅）。
    fn on_load_project(&mut self, project: Project) -> Vec<EventPayload> {
        let _ = self.backend.reset();
        self.backend.clear_debug_state();

        // ---- 第一步：**先解析**，不写总线 ----
        //
        // ⚠️ 顺序是刻意的：**档位要按镜像用到的地址挑** ⇒ 必须先知道
        // 镜像落在哪些地址，才知道该建哪一张内存图。
        //
        // 旧顺序是「先建图、再装载」—— 那只对「所有镜像都落在同一张
        // 图里」成立，真实 RTOS（osz）不成立。
        let mut parsed: Vec<ParsedImage> = Vec::new();
        for image in &project.images {
            match self.parse_image(image) {
                Ok(p) => parsed.push(p),
                Err(err) => return vec![self.error_from(&err)],
            }
        }

        let mut entry = 0u32;
        // M-06 带出的**调试节区原始字节**（裁决 B）⇒ 交给 M-11 解析。
        //
        // ⚠️ 多个镜像的节区**合并**（每个工程可能有好几个镜像）。
        let mut debug_sections: Vec<(String, Vec<u8>)> = Vec::new();
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        for p in &parsed {
            // A raw blob has entry 0; it must not clobber an ELF's
            // entry point.
            if p.entry != 0 {
                entry = p.entry;
            }
            debug_sections.extend(p.debug_sections.iter().cloned());
            for (addr, bytes) in &p.segments {
                ranges.push((*addr, addr.saturating_add(bytes.len() as u32)));
            }
        }

        // ---- 第二步：挑平台描述 + 重建平台 ----
        //
        // ⚠️ **重建平台**：**内存图 + 探针管道 + 外设探针回调**一起重来。
        //
        // ⚠️ 不能只 `reset_memory_map()` + 建图：
        // `reset_memory_map` 换上一条**全新**的总线，总线上挂的 hook
        // （M-17 的观察点 hook + M-18 的访存 trace hook）**一起没了**，
        // 外设的探针回调也随旧外设一起没了。探针管道必须在新的总线上
        // 重新装一遍（D-46）。
        //
        // ⚠️ **挑档判据已经描述化**（裁决 E）：挑出来的是**一份描述**
        // （不是「档位」枚举）⇒ 内存图由它决定。
        //
        // ⚠️ **STAGE-14 §3.2 第 2 步**：**配过板** ⇒ 用**用户的板**建图
        // （跳过自动选档）；**从未配过板** ⇒ 走今天的自动选档（**逐字不变**，
        // 验收 B-5）。
        //
        // ⚠️ **读，不消耗**（§2.2 的施工期订正）：初版这里是
        // `pending_platform.take()` ⇒ 用完就没了 ⇒ **用户装载前配的板会被
        // 紧接着的那次 `LoadProject` 覆盖掉** ⇒「装载前立即生效」成一句
        // 空话 ⇒ 用户的板是**用户意图**，**跨 `LoadProject` 保持**。
        //
        // ⚠️ **v0.31（STAGE-16 裁决 A 第 ③ 条）**：**待生效的板被应用** ⇒
        // 修订号 **+1** ⇒ 「待生效」是一次**生效的内存图变更** ⇒ 前端
        // 手里的修订号必须跟着更新（否则下一次提交会误报冲突）。
        //
        // ⚠️ 「待生效」是**推导量**（用户的板 ≠ 当前生效的图）⇒ 判据与
        // [`KernelHost::memory_map_pending`] **同一口径** ⇒ 这里**不**另立
        // 一个标志位。
        match &self.user_board {
            Some(board) => {
                if self.platform != *board {
                    // ⚠️ **待生效**（用户的板 ≠ 当前生效的图）⇒ 它在这里
                    // **被应用** ⇒ 生效的图变了 ⇒ 修订号 +1。
                    self.map_revision += 1;
                }
                self.platform = board.clone();
            }
            None => match self.choose_platform(&ranges) {
                Ok(description) => self.platform = description,
                Err(err) => return vec![self.error_from(&err)],
            },
        }
        if let Err(err) = self.rebuild_platform() {
            return vec![self.error_from(&err)];
        }

        // ---- 第三步：把镜像**写进**总线 ----
        for p in &parsed {
            for (addr, bytes) in &p.segments {
                if let Err(err) = self.backend.bus_mut().load_bytes(*addr, bytes) {
                    return vec![self.error_from(&err)];
                }
            }
        }
        self.backend.set_pc(entry);

        // ⚠️ **无条件重建** M-11 的索引（STAGE-5 §9.2）：装载新工程 = 新
        // 会话 ⇒ 旧的符号表 / 行表 / 反汇编缓存 / 轨迹环**全部**作废。
        //
        // ⚠️ 重建**失败**（符号表畸形 / 超上限）⇒ **拒绝**这次装载
        // （`E_LDR-01` / `E_LDR-02`），**不**截断后静默少给（验收 #8）。
        if let Err(err) = self.rebuild_symbol_index(&debug_sections) {
            return vec![self.error_from(&err)];
        }

        // ⚠️ 镜像装载本身会写总线（`load_bytes` 逐字节写）⇒ M-18 的访存
        // trace 环里多了一批**宿主侧**的写。
        //
        // 它们**不是**访客的执行历史 ⇒ **丢掉**（不是转移）。
        //
        // ⚠️ 为什么这里可以丢，而 D-51 说「不能定期 drain 掉」：D-51 怕的
        // 是把**访客执行历史**冲掉；装载镜像发生在**任何访客执行之前**，
        // 环里此刻只有宿主侧的镜像写。而且轨迹环是**定容**的 ⇒ 把这批噪声
        // 塞进去会把访客的执行历史挤出去。
        self.discard_m18_trace();

        // A new project is a new session: the old topic subscriptions die
        // with it (FR-13 — every session id owns its namespace). The host
        // has to subscribe again after a `LoadProject`.
        self.topics.clear();
        // The next register push must be a **full** snapshot.
        self.published_valid = false;

        // ⚠️ **装载成功** ⇒ 本会话**已经装载过镜像**（STAGE-14 §2.3 裁决 C）。
        //
        // ⚠️ 置真的位置在**全部步骤成功之后** ⇒ 装载**失败**（中途
        // `return`）时它**保持假** ⇒ 「有没有装载过」这个判据不会撒谎。
        self.image_loaded = true;

        // ⚠️ **镜像字节留在宿主**（v0.31 · STAGE-16 裁决 C 第 ④ 步）：
        // 记住**最后一个装载成功的工程** ⇒ 卸载之后**不必重新选文件**就能
        // 重装。
        //
        // ⚠️ 置的位置在**全部步骤成功之后** ⇒ 装载失败的工程**不被记住**
        // （记住一个装不上的工程没有意义）。
        self.last_project = Some(project);

        let mut events = vec![self.tick()];
        if let Some(wave) = self.register_waveform(true) {
            events.push(wave);
        }
        events
    }

    /// 「用户配的板」是不是**还没生效**（STAGE-14 §3.2 ④ 的**推导量**）。
    ///
    /// ⚠️ **推导，不单独存**：用户的板 ≠ 当前生效的图 ⇒ 还没生效。
    ///
    /// ⚠️ **为什么是推导量**：存两个字段（「配的板」+ 一个 `pending` 标志）
    /// ⇒ 「配了板但没生效」与「标志忘了翻」是**两种不同的错** ⇒ 前者推导
    /// 不出来、后者无从发现 ⇒ 只留一个字段就少一处可能不同步的地方。
    ///
    /// ⚠️ `PlatformDescription` 派生了 `PartialEq` ⇒ 这个比较是**类型安全**的。
    fn memory_map_pending(&self) -> bool {
        match &self.user_board {
            Some(board) => self.platform != *board,
            None => false,
        }
    }

    /// **设置板级内存图**（用户在界面上配的那张板）。
    ///
    /// ⚠️ **顺序固定**（STAGE-16 §3.1，冻结规格 —— 在 v0.29 的五步前
    /// **插一步**修订号检查，其余顺序**一字不改**）：
    ///
    /// ```text
    /// ① 修订号检查（expected_revision 若给出且 ≠ 当前 ⇒ E_PLT_13）⇒ 什么都不动
    /// ② description.validate()        ← 契约层那个唯一校验器
    /// ③ user_board = Some(description) ← 记下**用户意图**
    /// ④ 若 !image_loaded ⇒ 立即生效（换图 + 重建总线）
    /// ⑤ map_revision += 1
    /// ⑥ 回 EventPayload::MemoryMap { description, pending, loaded, revision }
    /// ```
    ///
    /// ⚠️ **第 ① 步排在第 ② 步之前**（裁决 B）⇒ 「你是不是基于旧状态」
    /// 比「这份描述合不合法」**更先**回答 ⇒ 两个错不会互相盖住。
    ///
    /// ⚠️ **校验失败时「什么都不动」**：不合规的描述**不能**污染宿主当前的
    /// 内存图 ⇒ `self.platform` 与 `user_board` 都保持原样。
    ///
    /// ⚠️ **已装载镜像时不重建总线**（裁决 B）：正在跑的 guest **一字不受
    /// 影响** ⇒ 新板等下一次 `LoadProject` 生效（事件回 `pending = true`）。
    ///
    /// ⚠️ **重建失败时 `user_board` 保留**：用户的板还在，只是**没生效**
    /// ⇒ `pending` 变真 ⇒ 用户改板这件事**没有丢**。
    fn on_set_memory_map(
        &mut self,
        description: PlatformDescription,
        expected_revision: Option<u64>,
    ) -> Vec<EventPayload> {
        // ---- ① 修订号检查（**排在 validate() 之前** —— 裁决 B）----
        //
        // ⚠️ 失败 ⇒ **什么都不动**（图、用户的板、修订号三者都不变）。
        if let Err(err) = self.check_revision(expected_revision) {
            return vec![self.error_from(&err)];
        }

        // ---- ② 校验（契约层那个唯一校验器 ⇒ E_PLT-01..E_PLT-16）----
        //
        // ⚠️ 失败 ⇒ **什么都不动**（当前图与用户的板都不碰）⇒ 只回一条
        // `Error` 事件。
        if let Err(err) = description.validate() {
            return vec![self.error_from(&err)];
        }

        // ---- ③ 记下**用户配的板**（裁决 A：住在宿主会话内）----
        //
        // ⚠️ 存的是**一份拷贝** ⇒ 第 ④ 步可以把原件直接换进 `self.platform`
        // （`PlatformDescription` 是 `Clone` 的数据，不是什么昂贵的东西）。
        //
        // ⚠️ **不消耗**它：用户的板是**用户意图** ⇒ 跨 `LoadProject` 保持
        // （§2.2 的施工期订正）。
        self.user_board = Some(description.clone());

        // ---- ④ 未装载过镜像 ⇒ **立即生效**（裁决 B）----
        if !self.image_loaded {
            // ⚠️ `rebuild_platform()` 读的是 `self.platform` ⇒ 先换图。
            let old = self.platform.clone();
            self.platform = description;
            if let Err(err) = self.rebuild_platform() {
                // ⚠️ **恢复旧图**：重建的第一步就是 `reset_memory_map()` ⇒
                // 失败后总线可能是**空的** ⇒ 不恢复的话用户拿到的是一张
                // 空总线（比报错更糟）。
                //
                // ⚠️ 恢复是**尽力而为**（`let _ =`）：**原始错误**才是要
                // 报出去的那一个 ⇒ 不被恢复的成败掩盖。
                self.platform = old;
                let _ = self.rebuild_platform();
                // ⚠️ `user_board` **保留**（用户的板还在，只是没生效）。
                //
                // ⚠️ **修订号不 +1**：重建失败 ⇒ 这次变更**没被接受**。
                return vec![self.error_from(&err)];
            }
        }

        // ---- ⑤ 修订号 +1（裁决 A：每次**被接受**的变更）----
        self.map_revision += 1;

        // ---- ⑥ 回「当前生效的图」+「用户的板有没有生效」（裁决 D / M）----
        vec![self.memory_map_event()]
    }

    /// **差量更新**板级内存图（v0.31 · STAGE-16 §3.2 · D-137）。
    ///
    /// ⚠️ **顺序固定**（冻结规格）：
    ///
    /// ```text
    /// ① 修订号检查（⇒ E_PLT_13）⇒ 什么都不动
    /// ② 合成整图：以 self.platform（当前生效的图）为底，逐条施加 ops
    /// ③ 合成结果整体 validate()（⇒ E_PLT-*）⇒ 什么都不动
    /// ④ user_board = Some(合成结果)
    /// ⑤ 若 !image_loaded ⇒ 换图 + rebuild_platform()（失败 ⇒ 恢复旧图、用户的板保留）
    /// ⑥ map_revision += 1
    /// ⑦ 回 EventPayload::MemoryMap { description, pending, loaded, revision }
    /// ```
    ///
    /// ⚠️ **合成期间不碰任何状态**（第 ② 步在**内存里的副本**上做）⇒
    /// 第 ② / ③ 步失败 ⇒ 宿主的图、用户的板、修订号**三者都不变**。
    ///
    /// ⚠️ **整体 `validate()`**（裁决 J）⇒ 差量改完**仍**按整图校验
    /// （V-6 重叠检查、V-7 预算都在其中）。
    ///
    /// ⚠️ **按 `base` 定位 ⇒ 与数组顺序无关**（台账判别力 ③）。
    ///
    /// ⚠️ **v0.32 · STAGE-17 裁决 A**：第 ② 步的合成已剥成**纯函数**
    /// [`compose_memory_map`] ⇒ 它**只读** `self.platform` + **只写**返回值
    /// ⇒ ⇒ 「合成期间不碰任何状态」这条既有语义**一字不改** ⇒ ⇒ 而「两条
    /// 路径产出的图必须一致」这条承诺从此有了一条**可执行的断言**
    /// （`tests_board_delta_equivalence.rs` · B-1 ~ B-7）。
    fn on_patch_memory_map(
        &mut self,
        ops: Vec<MemoryMapOp>,
        expected_revision: Option<u64>,
    ) -> Vec<EventPayload> {
        // ---- ① 修订号检查（**排在合成之前** —— 裁决 B）----
        if let Err(err) = self.check_revision(expected_revision) {
            return vec![self.error_from(&err)];
        }

        // ---- ② 合成整图（**纯函数** ⇒ 合成期间不碰任何状态）----
        //
        // ⚠️ **基底 = 当前生效的图**（`self.platform`，裁决 I）⇒ 与前端
        // 算差量的基准（最近一次 `memory.map` 事件带回来的生效图）**是同
        // 一个东西** ⇒ 不需要额外的「草稿基座」状态。
        //
        // ⚠️ **合成失败 ⇒ 什么都不动**（第 ② / ③ 步失败 ⇒ 宿主的图、
        // 用户的板、修订号**三者都不变**）⇒ 只回一条 `Error` 事件。
        //
        // ⚠️ **校验不在纯函数里**（v0.32 · STAGE-17 裁决 A）：纯函数**不跑**
        // `validate()` ⇒ 校验**留在第 ③ 步**（契约层那个唯一校验器）⇒
        // ⇒ **既有语义一字不改**（裁决 M）。
        let synthesized = match compose_memory_map(&self.platform, &ops) {
            Ok(synthesized) => synthesized,
            Err(err) => return vec![self.error_from(&err)],
        };

        // ---- ③ 合成结果**整体**校验（契约层那个唯一校验器）----
        //
        // ⚠️ 失败 ⇒ **什么都不动** ⇒ 只回一条 `Error` 事件。
        if let Err(err) = synthesized.validate() {
            return vec![self.error_from(&err)];
        }

        // ---- ④ 记下**用户配的板**（合成结果本身就是整图）----
        self.user_board = Some(synthesized.clone());

        // ---- ⑤ 未装载过镜像 ⇒ **立即生效**----
        if !self.image_loaded {
            let old = self.platform.clone();
            self.platform = synthesized;
            if let Err(err) = self.rebuild_platform() {
                // ⚠️ **恢复旧图**（同 `on_set_memory_map` 的恢复路径）。
                self.platform = old;
                let _ = self.rebuild_platform();
                // ⚠️ `user_board` **保留**；修订号**不 +1**（没被接受）。
                return vec![self.error_from(&err)];
            }
        }

        // ---- ⑥ 修订号 +1（裁决 A）----
        self.map_revision += 1;

        // ---- ⑦ 回事件（裁决 D / M）----
        vec![self.memory_map_event()]
    }

    /// **卸载工程**（v0.31 · STAGE-16 §3.3 · D-139 · 裁决 C）。
    ///
    /// ⚠️ **六步固定**（冻结规格）：
    ///
    /// ```text
    /// ① backend.reset() + backend.clear_debug_state()  ← 清寄存器 / 事件队列 / 断点 / 观察点
    /// ② self.topics.clear()          ← 新会话 ⇒ 订阅全部失效 ⇒ 前端必须重订
    /// ③ self.image_loaded = false    ← ⇒ 下一次改板立即生效
    /// ④ 镜像字节留在宿主（last_project）⇒ 不必重新选文件
    /// ⑤ 用户的板（user_board）留着  ← 它是用户意图，不是运行态
    /// ⑥ 回至少一条事件
    /// ```
    ///
    /// ⚠️ **修订号不 +1**（卸载不改图，只改「有没有装载镜像」⇒ 裁决 A）。
    ///
    /// ⚠️ **回事件不能是空数组**：前端 M-05 靠**第一个**带 `reqId` 的事件
    /// 落地 Promise ⇒ 返回空数组会让前端**永远等不到**。
    fn on_unload_project(&mut self) -> Vec<EventPayload> {
        // ---- ① 清**运行态**（寄存器 / 事件队列 / 断点 / 观察点）----
        let _ = self.backend.reset();
        self.backend.clear_debug_state();

        // ---- ② 新会话 ⇒ 订阅全部失效（前端**必须重订**）----
        self.topics.clear();

        // ---- ③ 「有没有装载镜像」变假 ⇒ **之后改板立即生效**----
        self.image_loaded = false;

        // ---- ④ 镜像字节**留着**（`last_project` 不清）----
        // ---- ⑤ 用户的板**留着**（`user_board` 不动）----

        // ---- ⑥ 回**至少一条**事件 ----
        vec![self.memory_map_event()]
    }

    /// **修订号检查**（v0.31 · STAGE-16 裁决 B）。
    ///
    /// ⚠️ **`None` ⇒ 不检查**（旧调用方 / 脚本仍能用）。
    ///
    /// ⚠️ **`Some(r)` 且 `r != 当前` ⇒ 明确拒绝**（`E_PLT_13`）⇒
    /// **什么都不动**（图、用户的板、修订号三者都不变）⇒ 并发丢更新
    /// **看得见**。
    fn check_revision(&self, expected_revision: Option<u64>) -> Result<(), PlatformError> {
        match expected_revision {
            None => Ok(()),
            Some(expected) if expected == self.map_revision => Ok(()),
            Some(expected) => Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_13_REVISION_CONFLICT,
                format!(
                    "期望的修订号 {expected} 与宿主当前的修订号 {} 不一致（图已被改过）",
                    self.map_revision
                ),
            )),
        }
    }

    /// 一条 `MemoryMap` 事件 —— **板级内存图的完整状态**（裁决 M）。
    ///
    /// ⚠️ **四个字段**：`description`（当前生效的图）· `pending`（用户的板
    /// 有没有生效 —— 推导量）· `loaded`（有没有装载镜像）· `revision`
    /// （修订号）。
    ///
    /// ⚠️ **只有这一个落点** ⇒ §3.1 第 ⑥ 步 · §3.2 第 ⑦ 步 · §3.3 第 ⑥ 步
    /// · §3.4 的即时推送 —— 四处**同形**。
    fn memory_map_event(&self) -> EventPayload {
        EventPayload::MemoryMap {
            description: self.platform.clone(),
            pending: self.memory_map_pending(),
            loaded: self.image_loaded,
            revision: self.map_revision,
        }
    }

    /// **解析**一个镜像（**不写总线**），返回入口地址、段内容与调试节区。
    ///
    /// ⚠️ **为什么拆成「解析」与「写入」两步**：内存图档位要按镜像
    /// 用到的地址挑 ⇒ 必须先知道地址，才知道该建哪张图。
    ///
    /// ⚠️ 调试节区**不过契约层**（契约只传查询结果，不传节区字节）⇒
    /// 它在**内核内部**从 M-06 交给 M-11（STAGE-5 §4）。
    fn parse_image(&mut self, image: &ImageRef) -> Result<ParsedImage, PlatformError> {
        let bytes = match &image.source {
            ImageSource::Bytes(bytes) => bytes.clone(),
            ImageSource::Url(url) => {
                return Err(PlatformError::new(
                    ErrorFamily::Loader,
                    1,
                    format!(
                        "镜像 {} 的来源是 URL（{url}）：内核不联网，宿主必须先把字节取回来",
                        image.name
                    ),
                ));
            }
        };
        let loader = DefaultLoader;
        let limits = ParseLimits::default();
        let loaded = match image.kind {
            ImageKind::Elf => loader.load_with_limits(&bytes, LoadKind::Elf, &limits)?,
            // The contract's `ImageRef` carries no base address, so a raw
            // blob always lands at 0 — which is the reset vector.
            ImageKind::RawHex => loader.load_raw_at(&bytes, 0, &limits)?,
        };
        Ok(ParsedImage {
            entry: loaded.entry,
            segments: loaded.segments,
            debug_sections: loaded.debug_sections,
        })
    }

    /// 执行一批指令（`None` = 跑到停止条件，`Some(n)` = 恰好 n 条）。
    fn on_run(&mut self, steps: Option<u32>) -> Vec<EventPayload> {
        // 运行期**重试**装探针回调（V-A 检视 · 🟡-7）。
        //
        // 装配那一刻外设可能正被总线派发路径占用（`try_borrow_mut` 失败）
        // → 那一轮漏掉了。没有重试的话，这个漏掉的回调**永远不会**补上，
        // 那个信号从此再也不产生边沿，而宿主一声不吭。
        //
        // `attach_signal` 对同一 `(source, signal)` 是**幂等**的，所以重复
        // 调用不会冲突。
        self.attach_peripheral_probes();

        // ⚠️ **跑之前先同步一次外设中断源**（v0.23）。
        //
        // guest 在**上一批**里写下的 `mtimecmp` / PLIC 配置，只有宿主
        // 把它喂给 M-16 才生效 ⇒ 不先同步，这一批里定时器不会来。
        self.sync_interrupts();
        // ⚠️ **先把 UART 的字节取走**（捕获环满了会丢最新的字节）。
        self.collect_console();

        let mut events = Vec::new();
        // `Continue` → **时间驱动**：走 M-12 的唯一主循环（量子化执行 +
        // 到期事件派发 + 外设时钟推进）。一轮的边界是引擎自己的指令预算
        // （`DEFAULT_INSTRUCTION_BUDGET`），跑满就让出给宿主。
        // `StepN` → **指令驱动**：恰好 n 条（调试器语义）。
        //
        // 两条路径共用同一个执行器（`m17_backend::IssExecutor`），所以
        // 「按指令跑」与「按时间跑」不会各自长出一套 ISS 逻辑（D-07）。
        let run = match steps {
            Some(n) => self.step_in_chunks(n),
            // ⚠️ **`Continue` 分支就是看门狗的接线落点**（规格 §3.3）：
            // 一次 `continue_until_event` = **一个**检查窗口 ⇒ 停滞时
            // **不立即上抛**，而是**记一次窗口** ⇒ 连续三个窗口无进展才触发
            // 处置（裁决 G）。
            None => self.run_continue_windows(),
        };
        match run {
            Ok(()) => {}
            Err(err) => {
                // The error comes **first**: it carries the command's
                // `reqId`, so M-05 rejects the command instead of letting
                // it time out. The state-reflecting events follow as
                // pushes.
                events.push(self.error_from(&err));
            }
        }
        // ⚠️ **D-51 的落点**：本批跑完后，把 M-18 的访存 trace 环**立刻**
        // 转进 M-11 的轨迹环。
        //
        // 「立刻」不是优化，是**要求**：M-18 的环（`TRACE_CAP = 200_000`）
        // 在 M-11 落地前**只写不读**（常驻 3~5 MB）；而它又是 M-11 要用的
        // 历史的**唯一来源** ⇒ drain 与转移必须同时发生，只 drain 会把
        // 历史冲掉。
        self.transfer_trace();
        // ⚠️ **跑之后再同步一次**（v0.23）：guest 在这一批里写下的
        // `mtimecmp` / PLIC 配置，下一批才用得上。
        self.sync_interrupts();
        // ⚠️ 跑完再取一次 —— 这一批里 guest 可能又写了 console。
        self.collect_console();
        // console 事件（v0.23 · 契约 v4）：订阅了才推。
        if let Some(console) = self.console_event() {
            events.push(console);
        }
        events.push(self.tick());
        if let Some(wave) = self.register_waveform(false) {
            events.push(wave);
        }
        // 探针波形（D-46）：本轮运行里外设状态变化产生的边沿。
        if let Some(wave) = self.waveform_event(false) {
            events.push(wave);
        }
        let reason = self.backend.last_stop_reason().or_else(|| {
            // `Continue` 跑到预算上限也是一个合法的停止条件（契约里
            // 最接近的取值是 `EndOfExecution`）；`StepN` 跑满 n 条是
            // **正常完成**，不是停止。
            if steps.is_none() {
                Some(HaltReason::EndOfExecution)
            } else {
                None
            }
        });
        if let Some(reason) = reason {
            let pc = self.backend.engine().snapshot().pc;
            events.push(EventPayload::Halted { reason, pc });
        }
        events
    }

    /// 跑 `n` 条指令，**按指令预算分块**（`DEFAULT_INSTRUCTION_BUDGET`）。
    ///
    /// ⚠️ **为什么要分块**（v0.20 检视 A · 重要 1）：
    /// M-18 的访存 trace 环是**有界**的（`TRACE_CAP = 200_000`，溢出丢最旧）
    /// ⇒ 它同时是**反汇编缓存的失效通道**（裁决 G：访存写事件 ⇒ 失效对应
    /// 地址）⇒ 环一旦溢出，**写事件被丢掉 ⇒ 缓存条目不失效 ⇒ 反汇编给出
    /// 旧机器码**（调试器给出**错误**的指令）。
    ///
    /// ⇒ 分块 ⇒ 每块至多 `DEFAULT_INSTRUCTION_BUDGET`（10 000）条指令，
    /// 每条至多 5 次总线访问（1 次取指 + 至多 4 个非对齐字节片）⇒ 单块
    /// 事件数 ≤ 50 000 ≪ 200 000 ⇒ **失效通道永远无损**。
    ///
    /// ⚠️ **顺带消掉一处上界不对称**：`Continue` 走时间驱动主循环，一轮就是
    /// `DEFAULT_INSTRUCTION_BUDGET` 条；而 `StepN` 此前**无上界** ⇒ 一条
    /// `StepN { n: 500000 }` 能把宿主堵住很久，并让上面的环溢出。
    ///
    /// ⚠️ **语义不变**：仍然「恰好 n 条」——中途撞上断点 / 观察点 / 陷阱 /
    /// 暂停就**停在那里**，剩下的**不**跑（与一次性 `step_n(n)` 一致）。
    fn step_in_chunks(&mut self, n: u32) -> Result<(), PlatformError> {
        // ⚠️ **幂等**：内核已被看门狗终止 ⇒ 不再跑（规格 §3.3 的处置
        // 第 ④ 步之后，内核就是死的）。
        if self.watchdog.fired() {
            return Err(self.terminated_error("内核已被看门狗终止，不再接受新的运行"));
        }
        let mut remaining = n;
        while remaining > 0 {
            let chunk = remaining.min(DEFAULT_INSTRUCTION_BUDGET);
            // ⚠️ **看门狗判定必须落在它自己的 `while remaining > 0` 循环体内**
            // （规格 §3.3「接线落点的精确位置」）：现有实现是
            // `let executed = self.backend.step_n(chunk)?;` ⇒ chunk 一旦停滞
            // 就**当场 `?` 上抛**、循环直接退出 ⇒ **三次确认在循环外根本
            // 攒不起来**。
            // ⇒ 正确形态：「chunk 停滞 ⇒ 记一次窗口 ⇒ `streak < 3` 就
            // **继续下一个 chunk**（`remaining` 不减，因为那一条没跑成）⇒
            // `streak >= 3` 才返回 `Err`」。
            let started = HostInstant::now();
            let result = self.backend.step_n(chunk);
            let elapsed_ms = started.elapsed_ms_since();
            let executed = match self.judge_window(elapsed_ms, &result) {
                WindowVerdict::Completed { executed } => executed,
                // ⚠️ **停滞（未达确认次数）⇒ 吞掉**并继续下一个 chunk ——
                // `remaining` **不减**（那一条指令没跑成，算不上进度）。
                WindowVerdict::Stalled => {
                    // ⚠️ **块间转移照样要做**（D-51：drain 与转移必须同时发生）
                    // ⇒ 停滞的批次也不能让 M-18 的环涨起来。
                    self.transfer_trace();
                    continue;
                }
                // 看门狗触发（处置已做）⇒ 上抛；别的错误 ⇒ 照旧上抛。
                WindowVerdict::Fired(err) | WindowVerdict::Failed(err) => return Err(err),
            };
            remaining -= executed;
            // ⚠️ **块没跑满 ⇒ 提前停**（停止条件命中）⇒ 剩下的**不**再跑。
            if executed < chunk {
                break;
            }
            // ⚠️ **块间转移**（D-51：drain 与转移必须同时发生）⇒ M-18 的
            // 环不会涨到它的上限 ⇒ 失效通道保持无损。
            self.transfer_trace();
        }
        Ok(())
    }

    /// `Continue` 的驱动循环 —— **每个窗口一次 `continue_until_event`**。
    ///
    /// ⚠️ **接线落点**（规格 §3.3）：`on_run` 的 `Continue` 分支。一次
    /// `continue_until_event` = 引擎跑满一个指令预算窗口 = **一个窗口**。
    ///
    /// ⚠️ **停滞不立即上抛**（裁决 G）：第 1、2 个窗口的停滞被**吞掉**，
    /// 宿主**重入引擎**继续跑；第 3 个窗口才触发处置并上抛。
    ///
    /// ⚠️ **重入是安全的**：M-12 的守卫触发时只是 `return Err`，**不改任何
    /// 引擎状态**（不推进虚拟时间、不改寄存器、不动事件队列的游标）⇒ 宿主
    /// 再次调 `run_until` 得到**相同**的引擎快照（验收 **D-7**）。
    ///
    /// ⚠️ **实际空转 3 × `NO_PROGRESS_WINDOW` 次**：M-12 的 `no_progress` 是
    /// `run_until` 的**局部变量** ⇒ **每次重入都从 0 重新计数** ⇒ 三次确认
    /// 实际空转的是 3 × 1024 次，**不是** 1024 次（否则后来人会以为
    /// 「1024 次就停」）。
    fn run_continue_windows(&mut self) -> Result<(), PlatformError> {
        loop {
            match self.observe_continue_window() {
                WindowVerdict::Completed { .. } => return Ok(()),
                // 第 1、2 个窗口：吞掉停滞 ⇒ **重入引擎**再跑一个窗口。
                WindowVerdict::Stalled => continue,
                // 看门狗触发（处置已做）⇒ 上抛终止错误。
                WindowVerdict::Fired(err) => return Err(err),
                // 别的错误（不是活锁）⇒ 照旧上抛。
                WindowVerdict::Failed(err) => return Err(err),
            }
        }
    }

    /// 跑**一个**检查窗口（时间驱动）并判定（规格 §3.3 的六步）。
    fn observe_continue_window(&mut self) -> WindowVerdict {
        // ⚠️ **幂等**：看门狗已经触发过 ⇒ 内核已被终止 ⇒ **不再跑引擎**。
        // （否则死循环：引擎仍然是死的，而 `observe` 不会再返回 `true` ⇒
        // 停滞永远被吞掉 ⇒ 永远重入。）
        if self.watchdog.fired() {
            return WindowVerdict::Fired(
                self.terminated_error("内核已被看门狗终止，不再接受新的运行"),
            );
        }
        // ① 记起点（宿主单调时钟）
        let started = HostInstant::now();
        // ② 跑引擎（一次调用 = 一个指令预算窗口）
        let result = self.backend.continue_until_event().map(|()| 0u32);
        // ③ 算耗时
        let elapsed_ms = started.elapsed_ms_since();
        // ④⑤⑥ 判定 + 喂给看门狗
        self.judge_window(elapsed_ms, &result)
    }

    /// 把**一个窗口**的结果喂给看门狗（规格 §3.3 的第 ④⑤⑥ 步）。
    ///
    /// ⚠️ **只认一个码**：`stalled` 要求 family == `Engine` **且** scenario ==
    /// `E_ENG-03` —— 别的错误**不是**活锁（它们照旧上抛）。
    fn judge_window(
        &mut self,
        elapsed_ms: u64,
        result: &Result<u32, PlatformError>,
    ) -> WindowVerdict {
        // ④ `stalled` = 该批次返回 `Err` **且** family == Engine **且**
        //    scenario == E_ENG_03。
        let stalled = match result {
            Err(err) => err.family == ErrorFamily::Engine && err.scenario == E_ENG_03_NO_PROGRESS,
            Ok(_) => false,
        };
        // ⑤ 窗口判定（**纯函数** ⇒ 不依赖真实时钟 ⇒ 可单测）
        let outcome = classify_window(elapsed_ms, stalled);
        // ⑥ 喂给看门狗 ⇒ 返回 `true` 表示**本次触发了**
        if self.watchdog.observe(outcome) {
            let err = self.handle_watchdog_trigger(elapsed_ms, stalled);
            return WindowVerdict::Fired(err);
        }
        match result {
            Ok(executed) => WindowVerdict::Completed {
                executed: *executed,
            },
            // 停滞但**未达**确认次数 ⇒ 吞掉这个错误（宿主继续跑）。
            Err(_) if stalled => WindowVerdict::Stalled,
            // 别的错误 ⇒ 照旧上抛。
            Err(err) => WindowVerdict::Failed(err.clone()),
        }
    }

    /// 看门狗触发后的**处置**（顺序固定 · 规格 §3.3）。
    ///
    /// 1. 上报健康信号（M-22）⇒ `classify` 判成 `Severity::Fatal` ⇒
    ///    `Health::Critical`；
    /// 2. 记**一条** M-21 日志（`ctx = "ENG-WATCHDOG"`）；
    /// 3. 置幂等标志（`mark_watchdog_fired`）；
    /// 4. 降级：`DegradeAction::Terminate` ⇒ 终止内核。
    ///
    /// ⚠️ **场景码分工**（规格 §8.1）：`E_HMS_01_DEGRADE_FAILED` 由 **M-22**
    /// 上报；**`E_HMS_02_FATAL_TERMINATED` 由宿主（这里）上报** ⇒ 前端据此
    /// 区分「内核被终止」与「普通错误」。
    fn handle_watchdog_trigger(&mut self, elapsed_ms: u64, stalled: bool) -> PlatformError {
        let now_ps = self.backend.engine().now_ps();
        let t_host_ms = self.host_now_ms();
        let streak = self.watchdog.streak();
        // ① 健康信号（**先分级再入流** ⇒ 流里存的是 `Severity::Fatal`）。
        //    ⚠️ `err_code` 用**引擎侧**的停滞码（`E_ENG-03`）—— 那是让宿主
        //    判定「停滞」的那个码；HMS 自己的终止码是 `E_HMS-02`（下面报）。
        self.hms.report(HealthSignal {
            src: SignalSrc::Engine,
            kind: SignalKind::Watchdog,
            ctx: SignalCtx::GuestRun,
            err_code: Some(WATCHDOG_ERR_CODE.to_string()),
            t_virt_ps: now_ps,
            t_host_ms,
        });
        // ② M-21 环里一条 `WATCHDOG` 日志（`ctx` 12 字节 / `err_code` 9 字节
        //    ⇒ 都在 M-21 的上限内 ⇒ **不会被截断**）。
        let msg = format!("看门狗触发：连续 {streak} 个检查窗口无进展，已终止内核");
        self.log_buffer.borrow_mut().log(
            LogLevel::Error,
            WATCHDOG_LOG_CTX,
            &msg,
            WATCHDOG_ERR_CODE,
            now_ps,
            t_host_ms,
        );
        // ③ 幂等标志（一次运行只处置一次）。
        self.hms.mark_watchdog_fired();
        // ④ 降级：终止内核（M-22 置 `terminated` ⇒ 宿主据此停止接受新动作）。
        let _outcome = self.hms.degrade(DegradeAction::Terminate);
        // ⑤ 上报终止错误（由**宿主**上报 —— 见上面的场景码分工）。
        let detail = if stalled {
            format!("{msg}（停滞场景码 {WATCHDOG_ERR_CODE}）")
        } else {
            format!("{msg}（宿主时间超窗：本窗口 {elapsed_ms} ms > {HOST_WATCHDOG_BUDGET_MS} ms）")
        };
        self.terminated_error(&detail)
    }

    /// 看门狗终止错误（**唯一**的构造落点 · 规格 §8.1）。
    fn terminated_error(&self, detail: &str) -> PlatformError {
        PlatformError::new(
            ErrorFamily::Health,
            E_HMS_02_FATAL_TERMINATED,
            detail.to_string(),
        )
    }

    /// 宿主单调时间（毫秒）—— M-21 / M-22 的 `time_host_ms` 口径。
    fn host_now_ms(&self) -> u64 {
        self.started_at.elapsed_ms()
    }

    /// 宿主请求暂停。
    fn on_pause(&mut self) -> Vec<EventPayload> {
        let _ = self.backend.pause();
        let pc = self.backend.engine().snapshot().pc;
        vec![
            self.tick(),
            EventPayload::Halted {
                reason: HaltReason::UserPause,
                pc,
            },
        ]
    }

    /// 读一段通用寄存器（契约 v2 · `ReadRegisters`）。
    ///
    /// 语义逐字取自 `docs/detailed-design/STAGE-2_Contract_V2.md` §5
    /// （`count = 0` 的口径经父会话订正：见下面「回复空区间」）：
    ///
    /// - `index` / `count` **钳到** `[0, 32)`；`index` 钳到 `0..31`
    ///   （即 x31），区间右端也钳到寄存器表末尾；
    /// - `count == 0` → **回复空区间**（`values` 是空数组）；
    /// - 回复 `Registers { index, values }` 里的 `index` 是**实际**起点，
    ///   `values.len()` 是**实际**条数。
    ///
    /// ⚠️ **读是钳位、写是拒绝**：读越界没有副作用，钳到最近的合法寄存器
    /// 是合理的降级；写越界若也钳位，会**静默**写到别的寄存器。
    ///
    /// ⚠️ **走引擎状态**：寄存器值来自 M-17 → M-12 的那份 [`SimState`]，
    /// 宿主**不持有**影子副本（否则会出现「事件发出去了、引擎里的值没变」
    /// 这种两边漂移）。
    fn on_read_registers(&mut self, index: u32, count: u32) -> Vec<EventPayload> {
        let start = index.min(31);
        // 区间右端不能越过寄存器表末尾：`start + count <= 32`。
        let count = count.min(32 - start);
        // ⚠️ `count == 0` **照样回复**（`values` 是空数组）：请求必须被
        // **回答**。M-05 按 `reqId` 配对请求与回复——「没有回复」与「消息
        // 丢失」在协议上**不可区分**，零回复会被前端当成传输故障，在
        // 500 ms 后以 `E_CTX-06`（命令超时）拒绝：用户看到的是「内核没
        // 应答」，而不是「读到了空区间」。
        let gpr = self.backend.register_file();
        let end = (start + count) as usize;
        let values = gpr[start as usize..end].to_vec();
        vec![EventPayload::Registers {
            index: start,
            values,
        }]
    }

    /// 写**一个**通用寄存器（契约 v2 · `WriteRegister`）。
    ///
    /// 语义逐字取自 `docs/detailed-design/STAGE-2_Contract_V2.md` §5：
    ///
    /// - `index == 0` → **丢弃**（D-04：x0 恒等于零），但**仍回复**
    ///   （回复里的值恒为 0）；
    /// - `index > 31` → **拒绝**（`E_DBG-01`），寄存器文件**不变**；
    /// - 回复 `Registers { index, values }`，`values.len() == 1`。
    ///
    /// 回复里的值是**写完之后从引擎状态读回来的**实际值——不是命令里
    /// 带来的那个值：x0 的写被丢弃了，回 0 才是真的。
    fn on_write_register(&mut self, index: u32, value: u32) -> Vec<EventPayload> {
        if let Err(err) = self.backend.set_gpr(index, value) {
            // `index > 31`：拒绝（`E_DBG-01`）。寄存器文件**原样不动**。
            return vec![self.error_from(&err)];
        }
        let actual = self.backend.register_file()[index as usize];
        vec![EventPayload::Registers {
            index,
            values: vec![actual],
        }]
    }

    /// 协议 / 能力协商（契约 v2 · `Negotiate`）。
    ///
    /// - 宿主要的**最低**版本高于内核支持的版本 → **拒绝**（`E_PROTO-01`）；
    /// - 否则回 `SCHEMA_VERSION` 与内核**授予**的能力位
    ///   （[`KERNEL_CAP_BITS`]）。
    ///
    /// ⚠️ **最低版本，不是最高版本**：宿主说「我最低能说 v2」= 宿主懂
    /// v2 的全部语法；内核回它自己认得的版本（= `SCHEMA_VERSION`）。
    /// 宿主说「我最低能说 v3」= 宿主要内核至少懂 v3，而内核只懂 v2 →
    /// 拒。
    fn on_negotiate(&mut self, min_schema_version: u32, _host_cap_bits: u64) -> Vec<EventPayload> {
        if min_schema_version > SCHEMA_VERSION {
            let err = PlatformError::new(
                PROTO_FAMILY,
                E_PROTO_01_VERSION_MISMATCH,
                format!(
                    "宿主要求的最低协议版本 {min_schema_version} 高于内核支持的 {SCHEMA_VERSION}"
                ),
            );
            return vec![self.error_from(&err)];
        }
        // ⚠️ 宿主**提供**的 `cap_bits` 在这里**不参与**裁决：契约规格
        // §5 只说「回内核授予的 cap_bits」，没定义「取交集」之类的规则。
        // 宁可按规格字面实现（内核授予什么就是什么），也不自己发明一条
        // 规格里没有的语义——那样前端按位降级的依据就成了猜测。
        vec![EventPayload::Negotiated {
            schema_version: SCHEMA_VERSION,
            cap_bits: KERNEL_CAP_BITS,
            kernel: kernel_descriptor(),
        }]
    }

    // -- 契约 v5 的 console 输入方向（STAGE-9 裁决 A） ---------------------

    /// 往 console UART **写**（契约 v5 · `WriteConsole`）。
    ///
    /// ⚠️ **抬线是同步器的活，不是命令的活**：命令只负责「把字节放进去 +
    /// 让同步器跑一次」。**一条命令不产生两个真相** —— 宿主**不**另写
    /// 一份抬线逻辑（那会让「UART 就绪」与「中断线」各有一个判据）。
    ///
    /// ## 这个命令做**三件事**，顺序不能换
    ///
    /// 1. **把字节逐个塞进 UART 的接收 FIFO**（走
    ///    [`Self::uart_receive`] ⇒ `Uart16550::receive`）。
    /// 2. **`LSR` 的 `RX_VALID`（bit0）随之置位** —— 这是 **UART 模型
    ///    自己的事**（`receive()` 已经做了）⇒ 宿主**不手动改 LSR**
    ///    （手动改就是「第二个真相」：寄存器与 FIFO 各说各话）。
    /// 3. **调一次同步器**（[`Self::sync_interrupts`]）⇒ UART 的 RX 就绪
    ///    **且** `IER` 的 RX 中断使能位开着 ⇒ **按连线表**抬 PLIC 的源
    ///    （v0.34 · D-125：那个源号来自**描述**，不是源码里的字面量）⇒
    ///    PLIC 交付 ⇒ **M-16 的外部中断线**被拉起（`IRQ_UART_RX_READY` = 1）
    ///    ⇒ guest 看到的 `mcause` 是 **`0x8000_000B`**（架构码 11）。
    ///
    /// ## 回复
    ///
    /// ⚠️ **必须至少有一个事件**：前端的 M-05 命令路由是「**第一个**带
    /// `reqId` 的事件 resolve 掉这个命令」⇒ 返回空数组 ⇒ 前端的
    /// Promise **永远不落地**（只能等超时）。⇒ 与 `SetBreakpoint` /
    /// `ClearBreakpoint` / `Nop` 一致：返回 `vec![self.tick()]`。
    ///
    /// ## 空字节
    ///
    /// `bytes` 为空 ⇒ **什么都不做**（不碰 FIFO、不跑同步器），但
    /// **照样回复** —— 幂等，**不是**错误。
    ///
    /// ⚠️ **本批不做「往哪个 UART 写」的选择**：平台只有 UART0（且只有
    /// QEMU virt 档才映射它）。默认档没有 UART ⇒ 字节无处可去 ⇒
    /// [`Self::uart_receive`] 什么都不做（**不报错**）。
    fn on_write_console(&mut self, bytes: Vec<u8>) -> Vec<EventPayload> {
        if bytes.is_empty() {
            // ⚠️ 幂等：什么都不做，但**照样回复**（前端要看到回复落地）。
            return vec![self.tick()];
        }
        for byte in bytes {
            self.uart_receive(byte);
        }
        // ⚠️ 同步器**跑一次** ⇒ 外设的中断源被同步到 M-16（UART RX 就绪
        // ⇒ PLIC 源 10 ⇒ M-16 的外部中断线）。
        self.sync_interrupts();
        vec![self.tick()]
    }

    // -- 契约 v3 的调试面（M-11 · STAGE-5） ---------------------------------

    /// 反汇编（契约 v3 · `Disasm`）。
    ///
    /// * 取指来源 = **M-14 的总线**（经 M-11 注入的回调）：M-11 **不**
    ///   依赖 M-17 / M-14 ⇒ 依赖方向单向（LLD §9.1）。
    /// * 读未映射 ⇒ **提前结束序列**、返回**部分序列**（**不是**错误，
    ///   LLD 11-3）——M-11 的 `disasm` 自己处理，宿主只把结果透传。
    /// * `count > MAX_DISASM_COUNT` ⇒ **拒绝**（`E_CTX-02`，M-11 自己
    ///   抛的，宿主只透传）。
    ///
    /// ⚠️ 取指**必须真的只读 `size` 字节**（「读一个字取低半」会让映射区
    /// 末端的压缩指令越界误报，LLD 11-1）——见 [`fetch_bytes`]。
    fn on_disasm(&mut self, addr: u32, count: u32) -> Vec<EventPayload> {
        // 借用拆分：`self.backend`（总线）、`self.index`（行表）与
        // `self.disassembler`（可变）是**三个不同的字段** ⇒ 可以同时借。
        //
        // ⚠️ **不 `clone()` 索引**：osz 的符号表 + 行表不小，每条 `Disasm`
        // 都克隆一遍是**白烧**（STAGE-9 裁决 C 的接线要求）。
        let bus = self.backend.bus();
        let index = &self.index;
        let disassembler = &mut self.disassembler;
        let mut read = |addr: u32, size: u32| fetch_bytes(bus, addr, size);
        // ⚠️ 结果**先落到局部变量**：`Err` 分支要调 `self.error_from`
        // （`&self`），而 `self.disassembler` 的可变借用在调用返回后就
        // 结束了 —— 不落局部的话，match 的借用区间会横跨整个 match。
        let result =
            disassembler.disasm_with_lines(addr, count, &mut read, |a| index.resolve_line_entry(a));
        match result {
            Ok(entries) => vec![EventPayload::Disasm { entries }],
            Err(err) => vec![self.error_from(&err)],
        }
    }

    /// 地址 → 源码行（契约 v3 · `ResolveLine`）。
    ///
    /// ⚠️ **两态可区分**（LLD 11-4）：没有 DWARF ⇒ `file = ""` / `line = 0`；
    /// 有 DWARF 但地址没命中 ⇒ `file = ""` / `line = u32::MAX`。
    ///
    /// ⚠️ **诚实口径**：宿主**不自己编造行号**，只把 M-11 的判定原样映射
    /// 成契约形状（映射点见 [`source_location_event`]`）。
    /// ⚠️ **v0.22 批（Wave 2a）**：M-11 的 `dwarf.rs` 已换成**真解析器**
    /// （`.debug_line` 的 v2/v3/v4/v5 行号程序）⇒ 这里**第一次**能拿到
    /// 真 `file` + 真 `line`；「查过行表、没命中」（`Missed`，`line =
    /// u32::MAX`）与「没查」（`NoDwarf`，`line = 0`）**仍然分得开**。
    fn on_resolve_line(&mut self, addr: u32) -> Vec<EventPayload> {
        // ⚠️ **走 M-11 的端口**（`SymbolService::resolve_line`）—— 端口
        // 自己分得清「查过行表、没命中」（`Missed`）与「没查」（`NoDwarf`），
        // 宿主**不自己编造**行号（STAGE-7 裁决 J）。
        let lookup = self.index.resolve_line(addr).unwrap_or(LineLookup::NoDwarf);
        vec![source_location_event(addr, lookup)]
    }

    /// 导出（契约 v3 · `ExportTrace`）—— **宿主路由**（STAGE-5 裁决 D）。
    ///
    /// | `source` | `format` | 数据源 |
    /// | --- | --- | --- |
    /// | `Waveform` | `Csv` / `Vcd` | **M-19 的环**（闭环 D-50） |
    /// | `Waveform` | `Json` | **拒绝**（`E_CTX-03`） |
    /// | `Execution` | `Csv` / `Json` | **M-11 的轨迹环**（闭环 D-51） |
    /// | `Execution` | `Vcd` | **拒绝**（`E_CTX-03`） |
    ///
    /// ⚠️ **路由在宿主**：M-11 不依赖 M-19，M-19 也不依赖 M-11 ⇒ LLD §9.1
    /// 的依赖方向不变。
    ///
    /// ⚠️ **`offset` 的单位是「条目数」**，M-19 的 `next_offset` 也是条目
    /// 索引 ⇒ **直接透传**，不换算成字节。
    fn on_export_trace(
        &mut self,
        source: TraceSource,
        format: TraceFormat,
        offset: u64,
        limit: u32,
    ) -> Vec<EventPayload> {
        match source {
            TraceSource::Waveform => self.export_waveform(format, offset),
            TraceSource::Execution => self.export_execution(format, offset, limit),
        }
    }

    /// 轨迹记录开关（契约 v3 · `SetTraceEnabled`，裁决 H）。
    ///
    /// * `enabled = true` ⇒ 宿主开始把访存轨迹**记录**进 M-11 的环；
    /// * `enabled = false` ⇒ 停（**零开销**：宿主**不调用** `record`，
    ///   M-11 的 `record` 自己也立即返回 —— 规格 §9.7 的「双保险」）。
    ///
    /// ⚠️ **drain 照做**（不受开关管）：M-18 的环是**有界**的，不 drain
    /// 它会涨到上限（常驻几 MB）⇒ 「关闭 trace」反而更费内存。
    ///
    /// 回复里回显 `enabled` + `len` + `dropped`（前端据此显示降速提示，
    /// 评审 R9）。
    ///
    /// ⚠️ **幂等**：重复设置同一个值是安全的（规格 §3.4）。
    fn on_set_trace_enabled(&mut self, enabled: bool) -> Vec<EventPayload> {
        // ⚠️ **先清场、再切开关**：开关**切换**的那一刻做一次 drain，把
        // 「上一个状态」攒下的事件丢掉。
        //
        // - 打开：丢掉「关闭期间」攒的那批 —— 用户说「现在开始记录」，
        //   把旧的那批倒回来会违反「关闭 ⇒ 零记录」；
        // - 关闭：丢掉「切换之前」攒的那批 —— 它们属于上一个状态。
        //
        // ⚠️ 正常情况下此刻环里**已经是空的**（每批都 drain 过了）⇒ 这一步
        // 是**防御性**的：万一上一批在转移之前就出错了（例如访存报错），
        // 环里可能还留着半批 ⇒ 切换时清掉，免得它们混进下一个状态。
        self.discard_m18_trace();
        self.trace.set_enabled(enabled);
        vec![EventPayload::TraceState {
            enabled,
            len: self.trace.len() as u32,
            dropped: self.trace.dropped(),
        }]
    }

    /// 波形导出（`source = Waveform`）—— 数据源 = **M-19 的环**。
    ///
    /// ⚠️ **D-50 的要点**：数据源必须是 M-19 的环，**不是** M-18 的边沿
    /// 存储 —— 否则 M-19 的环永远是「只写不读」。
    ///
    /// ⚠️ `limit` 在这条路上**不适用**：M-19 的导出有自己的分块尺寸
    /// （`EXPORT_CHUNK = 10_000` 条），它的 API 里没有 limit 参数 ⇒ 宿主
    /// **不伪造**一个 limit（伪造会让续导协议算错 `next_offset`）。契约的
    /// `limit` 只约束 `source = Execution` 那条路。
    fn export_waveform(&mut self, format: TraceFormat, offset: u64) -> Vec<EventPayload> {
        if format == TraceFormat::Json {
            // M-19 没有 JSON 波形导出 ⇒ **拒绝**，不「换一个格式凑合」。
            return vec![self.error_from(&illegal_export(
                "波形导出不支持 JSON（M-19 只产出 CSV / VCD）",
            ))];
        }
        let Some(pipeline) = self.waveform.clone() else {
            return vec![self.error_from(&pipeline_unavailable(
                "波形管道尚未装配（没有可读的 M-19 环）",
            ))];
        };
        let Ok(mut pipe) = pipeline.try_pipeline_mut() else {
            // 管道正被占用（M-18 正在推边沿）——这是**内部瞬时**状态，
            // 不是客户端的请求有错。报出来而不是静默给一个空块（空块
            // 会被前端当成「这段会话没有波形」）。
            return vec![self.error_from(&pipeline_unavailable(
                "M-19 的波形管道正被占用（RefCell 争用），本次导出未执行",
            ))];
        };
        let filter = ExportFilter::all();
        let next_offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let handle = match format {
            TraceFormat::Csv => pipe.export_csv(&filter, next_offset, None),
            TraceFormat::Vcd => pipe.export_vcd(&filter, next_offset, None),
            // 上面已经拒了。
            TraceFormat::Json => unreachable!("JSON 波形导出在上面已被拒绝"),
        };
        vec![waveform_export_event(handle, format, offset)]
    }

    /// 执行轨迹导出（`source = Execution`）—— 数据源 = **M-11 的轨迹环**。
    ///
    /// ⚠️ `TraceFormat::Vcd` 由 M-11 自己拒（`E_CTX-03`）：执行轨迹不是
    /// 信号波形，VCD 语义不适用（STAGE-5 裁决 D）。
    fn export_execution(
        &mut self,
        format: TraceFormat,
        offset: u64,
        limit: u32,
    ) -> Vec<EventPayload> {
        match self.trace.export(format, offset, limit) {
            Ok(chunk) => vec![EventPayload::TraceExport {
                source: TraceSource::Execution,
                format,
                offset: chunk.offset,
                next_offset: chunk.next_offset,
                total: chunk.total,
                bytes: chunk.bytes,
                chunk: chunk.chunk,
                // ⚠️ M-11 的 `ExportChunk` **没有** `overflow_marked` ⇒ 用
                // 环的丢弃计数算（丢了最旧的条目 ⇒ 这份导出**不是**完整
                // 历史）。
                overflow_marked: self.trace.dropped() > 0,
            }],
            Err(err) => vec![self.error_from(&err)],
        }
    }

    /// 把 M-18 的访存 trace 环**转移**进 M-11 的轨迹环（闭环 D-51）。
    ///
    /// ⚠️ **drain 与转移必须同时发生**（台账 D-51）：「在宿主里定期 drain
    /// 掉」会把 M-11 要用的历史冲掉 ⇒ 这里**从不**单独 drain。
    ///
    /// ⚠️ **drain 照做，record 才受开关管**（规格 §9.7 的「零开销」口径）：
    /// 宿主**不调用** `record`、M-11 的 `record` 自己也立即返回（双保险）。
    ///
    /// ⚠️ **缓存失效与记录开关无关**（裁决 G）—— 这是本批修掉的一个
    /// **真实缺陷**：缓存失效是**正确性**问题（「访客改了代码，反汇编必须
    /// 跟着变」），记录开关是**性能**问题（「要不要把访存记进环里」）。
    /// 两者耦合 ⇒ 关闭 trace 期间访客自改代码 ⇒ 缓存**陈旧** ⇒ 用户看到的
    /// 反汇编是**旧的**。⇒ 失效**始终**执行，**只有 record** 受开关管。
    ///
    /// ⚠️ **关闭期间为什么还要 drain**：M-18 的环是**有界**的（`TRACE_CAP
    /// = 200_000`，常驻几 MB）⇒ 不 drain 它会涨到上限 ⇒ 「关闭 trace」反而
    /// 更费内存。drain 的成本 ≪ record 的成本（record 要 clone + 入环）。
    fn transfer_trace(&mut self) {
        let enabled = self.trace.is_enabled();
        for ev in self.drain_m18_trace() {
            // ⚠️ **写 ⇒ 失效反汇编缓存**（裁决 G 的失效通道 = M-18 的写
            // 事件）。**先**失效，**后**决定要不要记录 —— 顺序不能反：
            // 失效是正确性，record 是性能。
            if !ev.is_fetch {
                self.disassembler.invalidate_on_access(ev.addr, ev.dir);
            }
            if !enabled {
                // 关闭 ⇒ **零记录**（裁决 H / 验收 #15）：事件被 drain 掉，
                // 但**不进** M-11 的环。
                //
                // ⚠️ 丢弃**可见**（计数暴露给调用方），不静默。
                self.trace_events_discarded += 1;
                continue;
            }
            if ev.is_fetch {
                // 取指 ⇒ 指令条目（kind = 0）。
                //
                // ⚠️ M-18 的事件**不带数据** ⇒ 机器码要**额外取一次指**
                // （规格 §9.4 承认这是重复取指的性能代价；`disasmCache`
                // 能兜住一部分 —— 见 [`fetch_instruction_code`]）。
                let code = self.fetch_instruction_code(ev.addr);
                self.trace.record_instruction(ev.t_ps, ev.addr, code);
            } else {
                self.trace.record(&ev);
            }
        }
    }

    /// 把 M-18 的访存 trace 环**丢掉**（**有理由**的丢弃，不是「定期 drain」）。
    ///
    /// 两个调用点：
    ///
    /// 1. **装载工程后**：环里是 `load_bytes` 逐字节写镜像产生的**宿主侧**
    ///    写 —— 不是访客的执行历史（而且轨迹环是定容的，塞进去会把访客的
    ///    执行历史挤出去）。
    /// 2. **切换 trace 开关时**：用户说「现在开始记录」⇒ 记录从此刻开始；
    ///    把上一个状态攒下的那批倒回来会违反「关闭 ⇒ 零记录」。
    ///
    /// ⚠️ 丢弃**可见**（计数暴露给调用方），不静默。
    fn discard_m18_trace(&mut self) {
        let events = self.drain_m18_trace();
        self.trace_events_discarded += events.len() as u64;
    }

    /// 从 M-18 的环里**取走**全部访存事件（**唯一**的 drain 点）。
    ///
    /// ⚠️ 探针注册表正被占用（总线派发路径里）⇒ 这一轮**放弃**：事件
    /// 留在环里（**不丢**），下一轮再取。
    fn drain_m18_trace(&mut self) -> Vec<TraceEvent> {
        let Some(registry) = self.probes.clone() else {
            return Vec::new();
        };
        let Ok(mut reg) = registry.try_borrow_mut() else {
            return Vec::new();
        };
        reg.drain_trace()
    }

    /// 取一次指，拿机器码（规格 §9.4：M-18 的事件不带数据）。
    ///
    /// ⚠️ **先查 M-11 的反汇编缓存**（规格 §9.4 的「M-11 的 `disasmCache`
    /// 可以兜住它」）：缓存里有 ⇒ **不用**再取一次指 ⇒ 少一次总线读。
    ///
    /// ⚠️ 缓存里的 `code` 是**原码**（压缩指令 = 16 位原码），与下面现场
    /// 取指的口径**一致** ⇒ 两条路给出同一个值。
    ///
    /// ⚠️ **取指口径只有一份实现**（M-11 的 `disasm::fetch_instruction_code`
    ///）：先读 2 字节判宽度（低 2 位 ≠ `0b11` ⇒ 压缩指令，16 位原码就够），
    /// 否则再读 4 字节。⚠️ **必须真的只读 `size` 字节**（「读一个字取低半」
    /// 会让映射区**末端**的压缩指令越界 ⇒ 被误报成「未映射」，LLD 11-1）。
    ///
    /// ⚠️ 读不到 ⇒ `0`。`0` **不是**一条真的指令（全零的低 2 位是
    /// `0b00` ⇒ 非法压缩指令）⇒ 它唯一可能的含义是「取指失败，机器码
    /// 未知」。**不猜值**（与 M-11 对访存值的口径一致）。
    fn fetch_instruction_code(&mut self, addr: u32) -> u32 {
        if let Some(entry) = self.disassembler.lookup(addr) {
            return entry.code;
        }
        let bus = self.backend.bus();
        let mut read = |addr: u32, size: u32| fetch_bytes(bus, addr, size);
        m11_fetch_instruction_code(&mut read, addr).unwrap_or(0)
    }

    /// 重建 M-11 的索引（装载工程时**无条件**重建，STAGE-5 §9.2）。
    ///
    /// ⚠️ **必须重建，不能缓存**：装载新工程 = 新会话（D-45 的教训：装载
    /// 后内核侧的订阅 / 索引会失效，幂等的「已就绪」检查不够）。
    ///
    /// ⚠️ **节区可能为空**（裸机镜像 / 无节区表）⇒ 那也要能构造出**空的**
    /// 符号表 / 行表：M-11 的 API 表达「空」的方式是**空表 + 降级态**，
    /// 不是错误。
    ///
    /// ⚠️ **反汇编缓存必须清空**：旧工程的机器码留在缓存里，新工程在
    /// 同一个地址上的指令会被**旧机器码**顶掉。
    ///
    /// ⚠️ **轨迹环也要清空**：轨迹是**会话级**的数据，两个会话的轨迹
    /// 混在一起就是「两个真相」。
    fn rebuild_symbol_index(
        &mut self,
        sections: &[(String, Vec<u8>)],
    ) -> Result<(), PlatformError> {
        // 先把索引**算出来**再落位 ⇒ 要么都换掉，要么都不换
        // （不会留下「符号是新的、行表是旧的」这种拼接体）。
        self.index = SymbolIndex::from_sections(sections)?;
        self.disassembler = Disassembler::new();
        self.trace.clear();
        Ok(())
    }

    /// 订阅一个主题。
    fn on_subscribe(&mut self, topic: String) -> Vec<EventPayload> {
        let want_registers = topic == TOPIC_REGISTERS;
        let want_waveform = topic == TOPIC_WAVEFORM;
        let want_console = topic == TOPIC_CONSOLE;
        let want_memory_map = topic == TOPIC_MEMORY_MAP;
        self.topics.insert(topic);
        let mut events = vec![self.tick()];
        if want_registers {
            // The snapshot **is** the response — the caller asked for the
            // current register file.
            if let Some(wave) = self.register_waveform(true) {
                events.push(wave);
            }
        }
        if want_waveform {
            // 探针波形（D-46）：**强制**推一次，即使还没有边沿。
            //
            // 前端要看到**通道列表**——「有这条信号，只是还没变化」和
            // 「根本没有这条信号」是两回事。前者只靠边沿是表达不出来的：
            // 第一批边沿到来之前，前端会一直显示「尚无探针信号」。
            if let Some(wave) = self.waveform_event(true) {
                events.push(wave);
            }
        }
        if want_console {
            // console（v0.23）：**补推**已经攒下的字节。
            //
            // ⚠️ 前端常常是「先订阅、后启动」，而 guest 的第一批输出
            // 在它订阅之前就产生了 ⇒ 不补推的话，前端永远看不到启动
            // 阶段那几行。
            if let Some(console) = self.console_event() {
                events.push(console);
            }
        }
        if want_memory_map {
            // 板级内存图（STAGE-14 §3.2 · 裁决 J）：**立即**推一条当前生效
            // 的图。
            //
            // ⚠️ 前端靠它拿到**初始**的内存图（验收 B-6）⇒ 不推的话前端
            // 的内存映射表在第一次改板之前是空的。
            //
            // ⚠️ 与 `sim.registers` / `waveform.history` **同一套路**
            // （「订阅即强制推一条」）。
            //
            // ⚠️ **v0.31（STAGE-16 §3.4）**：推送内容与 §3.1 第 ⑥ 步
            // **同形** ⇒ **带 `loaded` 与 `revision`** ⇒ 前端**重订之后**
            // 修订号自动刷新 ⇒ **不会误报冲突**。
            events.push(self.memory_map_event());
        }
        events
    }

    // -- 事件构造 -----------------------------------------------------------

    /// 一条 `Tick` 事件（权威时钟 + PC）。
    fn tick(&self) -> EventPayload {
        let state = self.backend.engine().snapshot();
        EventPayload::Tick {
            elapsed_ps: state.virtual_time_ps,
            pc: state.pc,
        }
    }

    /// 寄存器波形。
    ///
    /// - `all = true` → **全量**（32 条 GPR + PC）。
    /// - `all = false` → **增量**（只带值变过的寄存器 + 变过的 PC）。
    ///
    /// 没有订阅 `sim.registers` 时返回 `None`（不发事件）。
    fn register_waveform(&mut self, all: bool) -> Option<EventPayload> {
        if !self.topics.contains(TOPIC_REGISTERS) {
            return None;
        }
        let state = self.backend.engine().snapshot();
        let now_ps = state.virtual_time_ps;
        let mut streams: Vec<WaveformStream> = Vec::new();
        for i in 0..32usize {
            let changed = !self.published_valid || self.published_regs[i] != state.gpr[i];
            if all || changed {
                streams.push(WaveformStream {
                    id: format!("cpu.gpr.x{i}"),
                    label: format!("x{i}"),
                    edges: vec![Edge {
                        t_ps: now_ps,
                        value: u64::from(state.gpr[i]),
                    }],
                });
            }
            self.published_regs[i] = state.gpr[i];
        }
        let pc_changed = !self.published_valid || self.published_pc != state.pc;
        if all || pc_changed {
            streams.push(WaveformStream {
                id: "cpu.pc".to_string(),
                label: "pc".to_string(),
                edges: vec![Edge {
                    t_ps: now_ps,
                    value: u64::from(state.pc),
                }],
            });
        }
        self.published_pc = state.pc;
        self.published_valid = true;
        if streams.is_empty() {
            return None;
        }
        Some(EventPayload::WaveformHistory(WaveformHistory {
            schema_version: WAVEFORM_SCHEMA_VERSION,
            streams,
        }))
    }

    /// 把平台错误翻译成契约的 `Error` 事件。
    fn error_from(&self, err: &PlatformError) -> EventPayload {
        EventPayload::Error {
            code: format!("E_{}-{:02}", err.family.as_str(), err.scenario),
            message: err.message.clone(),
        }
    }

    /// 取走引擎在调度循环里发布的事件（第 1 阶段恒为空）。
    pub fn drain_engine_events(&mut self) -> Vec<EventPayload> {
        let mut events = self.sinks.events.borrow_mut();
        std::mem::take(&mut *events)
    }

    /// 当前虚拟时间（皮秒）。
    pub fn now_ps(&self) -> u64 {
        self.backend.engine().now_ps()
    }

    /// 整体健康状态（M-22 派生 · v0.28 · D-115）。
    ///
    /// ⚠️ **派生**语义：有 FATAL 信号 ⇒ `Critical`；只有 CRITICAL / WARNING
    /// ⇒ `Degraded`；否则 `Ok` ⇒ **只降不升**。
    pub fn health(&self) -> Health {
        self.hms.state()
    }

    /// 看门狗是否已经触发过（幂等标志 · M-22）。
    pub fn watchdog_fired(&self) -> bool {
        self.hms.watchdog_fired()
    }

    /// M-21 日志环的快照（管理员通道的读出口 · v0.28 · D-115）。
    ///
    /// ⚠️ **纯读**：不改缓冲、不动计数。
    /// ⚠️ 借不到缓冲（别处正持有着可变借用）⇒ **退化成空列表**而不是
    /// panic —— 诊断路径不该把内核打挂。
    pub fn log_entries(&self) -> Vec<LogEntry> {
        self.log_buffer
            .try_borrow()
            .map(|buffer| buffer.query(&LogQuery::default()))
            .unwrap_or_default()
    }

    /// 事件队列里**待处理**的事件数（D-07）。
    ///
    /// 时间驱动链路的心跳：跑过一轮之后，队列里应当留着一件待派发的
    /// 外设 tick 事件（下一个量子边界的那一次）。指令驱动的路径
    /// （`StepN`）不排期它 —— 所以这个数字就是「走的是哪条路」的可
    /// 观测证据。
    pub fn pending_event_count(&self) -> usize {
        self.backend.engine().pending_event_count()
    }

    /// 当前 PC。
    pub fn pc(&self) -> u32 {
        self.backend.engine().pc()
    }

    /// 读一个内存位置（不计入总线统计）。
    pub fn read_memory(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        self.backend.bus().read_only(addr, size.into())
    }

    // -- 会话级快照（STAGE-3）------------------------------------------------
    //
    // 下面这一组是**宿主调试 / 测试读面**：它们把会话的当前状态原样
    // 摊出来，供端到端用例逐项比对「恢复前」与「恢复后」。
    //
    // ⚠️ 它们**不是契约层**的东西，也不进 `SCHEMA_VERSION`——契约层冻结
    // 的是**线上**的 DTO，这里是**进程内**的观察窗口。

    /// 当前会话的**通用寄存器**（32 个 GPR）。
    ///
    /// ⚠️ **x0 恒为 0**（D-04）：取的是后端的寄存器文件，它已经把 x0
    /// 归零——一份被损坏的快照不能让 x0 泄出非零值。
    pub fn registers(&self) -> [u32; 32] {
        self.backend.register_file()
    }

    /// 当前会话的**引擎段**（队列内容 + 队列顺序 + 插入序计数器 + 指令
    /// 预算 + 平台状态）。
    ///
    /// ⚠️ 一个 [`EngineSnapshotDto`] 就覆盖了「事件队列 / 插入序 / 指令
    /// 预算」三项——它们在 DTO 里是**分开的字段**，不是一个笼统的
    /// 「引擎状态」。
    pub fn engine_snapshot(&self) -> EngineSnapshotDto {
        self.backend.engine().snapshot_dto()
    }

    /// 当前会话的**外设内部状态**（段 4 的同一份导出）。
    pub fn peripheral_states(&self) -> M15PeriphDto {
        m15_peripheral::collect_session_state(&self.backend.bus().peripheral_instances())
    }

    /// 当前会话的**已拉起 IRQ 行**（段 5 的同一份导出）。
    pub fn intc_state(&self) -> M16IntcDto {
        self.intc_state_inner()
    }

    /// 当前会话的**内存映射拓扑**（段 3 的同一份导出）。
    pub fn memory_map(&self) -> M14MapDto {
        self.backend.bus().export_map()
    }

    /// 拉起一条 IRQ 行（走的是**引擎那条路**——同一个委派）。
    ///
    /// ⚠️ 目前**没有生产路径**会调它：M-15 的 pending IRQ 采样点属于
    /// **M-16 落地**的范畴（台账 D-63 / D-64 / D-72 / D-74）。本方法
    /// 让「段 5 恒为空」的成因精确落在「M-15 没有生产路径」上，而不是
    /// 「intc 是个空壳」。
    pub fn raise_irq(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.sinks.raise(irq)
    }

    /// 撤下一条 IRQ 行。
    pub fn lower_irq(&mut self, irq: u16) -> Result<(), PlatformError> {
        self.sinks.lower(irq)
    }

    /// **清除**一条 IRQ 行（接缝 #7：调试器 / 固件清中断）。
    ///
    /// 语义与 M-16 的 [`InterruptController::clear`] 逐字相同：移除挂起项 +
    /// 撤销线电平 + 复位 `active_irq`。
    ///
    /// ⚠️ 争用（`RefCell` 拿不到）时**记一次失败并返回 `Err`**——与
    /// [`HostSinks::delegate`] **同一种结果**：同一个失败条件必须是
    /// 同一个口径（`Err` + 计数 +1），否则「同一个量两个真相」。
    pub fn clear_irq(&mut self, line: u16) -> Result<(), PlatformError> {
        self.sinks.delegate(|intc| {
            intc.clear(line);
            Ok(())
        })
    }

    /// 设置**定时器比较值**（接缝 #6 / 裁决 E）。
    ///
    /// `mtime >= mtimecmp` 时按**电平**语义挂起 line 3（timer-compare），
    /// 于是「定时器 → 仲裁 → 交付 → 陷入」整条链可以端到端验证。
    ///
    /// ⚠️ **固件侧的 MMIO 通路推迟到 P1**（裁决 E / 台账 D-82）：今天
    /// guest 自己写不了 `mtimecmp`，**宿主 API** 是本批的入口。
    ///
    /// ⚠️ `0` 表示**定时器关闭**（否则 `mtime >= 0` 恒成立，那是一条
    /// 永不消失的中断）。
    pub fn set_timer_compare(&mut self, mtimecmp_ps: u64) -> Result<(), PlatformError> {
        self.sinks.delegate(|intc| {
            intc.set_timer_compare(mtimecmp_ps);
            Ok(())
        })
    }

    /// 中断委派失败（`RefCell` 争用）的累计次数。
    ///
    /// 非零 = 有一次中断操作**没有真的落到控制器上**——这条 IRQ 行丢了。
    pub fn intc_delegation_failures(&self) -> u32 {
        self.intc.failures.get()
    }

    /// 采集一份**会话级**快照载荷（六段齐全，STAGE-3 §3.5）。
    ///
    /// 宿主是**采集器**，M-20 是**记录器**：六段分别来自各自的持有者
    /// （M-12 引擎 / M-14 总线 / M-15 外设 / M-16 控制器），最后交给
    /// [`SnapshotManager::capture_session`] 组装。
    ///
    /// ⚠️ **段 0 的 `t_virt` 由宿主代填**——它被设成引擎当前的虚拟时间
    /// （`m12_engine.state.virtual_time_ps`），**调用方给的值会被覆盖**。
    /// 这不是「造数据」：检查点的虚拟时间**就是**引擎的虚拟时间，两者是
    /// 同一个量；留两个落点就是「同一个量两个真相」（恢复时必须挑赢家，
    /// 输家默默腐烂）。段 0 的其余字段（检查点 id / 宿主时间 / 基线序列号）
    /// **仍然是调用方给的**，宿主不改写。
    ///
    /// ⚠️ **采集期若控制器被占用，段 5 会是空的**——载荷里**没有**
    /// 「本段不完整」的标记，一个空段 5 与「真的没有拉起任何 IRQ」**在
    /// 载荷里不可区分**。要判断采到没有，看
    /// [`Self::intc_delegation_failures`]：它非零就说明采集期发生过争用。
    /// （本批**不改**这个行为——改 `capture_session` 的返回类型会波及整条
    /// 链，而争用在生产路径上不可达；台账留账。）
    ///
    /// ⚠️ **段 2 只含 RAM 内容**（契约层裁决 B：寄存器的唯一落点是
    /// `m12_engine.state`，再存一份就是两个真相）。
    pub fn capture_session(&mut self, cp_meta: CheckpointMetaDto) -> SessionSnapshotDto {
        // ⚠️ 采集顺序：**先**把六段收齐（每段都是**独立**的一份导出），
        // **再**交给 M-20 组装——M-20 不碰 M-14 / M-15 / M-16。
        let engine = self.backend.engine().snapshot_dto();
        let m13_state = M13StateDto {
            mem_pages: self.backend.bus().export_pages(),
        };
        let m14_map = self.backend.bus().export_map();
        let m15_periph =
            m15_peripheral::collect_session_state(&self.backend.bus().peripheral_instances());
        let m16_intc = self.intc_state_inner();
        // ⚠️ 段 0 的 `t_virt` **代填**成引擎的虚拟时间——调用方给的值
        // 被覆盖（见本方法的文档）。
        let cp_meta = CheckpointMetaDto {
            t_virt: engine.state.virtual_time_ps,
            ..cp_meta
        };
        self.snapshots
            .capture_session(cp_meta, engine, m13_state, m14_map, m15_periph, m16_intc)
    }

    /// 还原一份**会话级**快照载荷（STAGE-3 §3.7）。
    ///
    /// ## 顺序是**不变量**，不可换
    ///
    /// ```text
    /// 0. payload.validate()           ← 先校验：一个字都还没改
    /// 0.5 外设状态的**只读**预检        ← 一个字都还没改（见下）
    /// 1. M-14 load_map(map)           ← 重建区域拓扑
    /// 2. M-14 load_pages(pages)       ← 把 RAM 内容写进去（需要区域已存在）
    /// 3. M-15 restore_session_states  ← 外设内部状态
    /// 4. M-16 load_state(intc)        ← 已拉起的 IRQ 行
    /// 5. M-17 restore_full(engine)    ← 最后还原引擎段（覆盖 gpr / pc / 虚拟时间）
    /// ```
    ///
    /// 页写不进不存在的区域；外设状态写不进未绑定的实例；引擎段**最后**
    /// 还原——它会**覆盖** `gpr` / `pc` / 虚拟时间。
    ///
    /// ## 「校验先于应用」是真的
    ///
    /// - 第 0 步失败 → **总线、外设、intc、引擎一个字节都不许动**；
    /// - 第 0.5 步（外设状态的**只读**预检）失败 → 同样**一个字节都不许
    ///   动**：它跑在 `load_map` / `load_pages` **之前**，所以「外设状态
    ///   畸形」不会留下「内存来自载荷、寄存器来自恢复前」的**拼接体**；
    /// - 第 1 / 2 / 3 步各自的消费者（Lane A 写的）**已经是**「全量校验
    ///   → 再写入」，所以**每一步失败时，那一步自己没写**；之前步骤写下的
    ///   内容是**合法**的（前面每步都通过了各自的校验）。
    ///
    /// ## ⚠️ 什么**不在**载荷里（STAGE-3 §7）
    ///
    /// **断点表 / 观察点表 / 探针环 / 波形环 / 总线统计计数器**都**不在**
    /// 会话载荷里。所以「还原出的会话等价吗」这个问题的答案**取决于目标
    /// 宿主**：**同宿主**还原会保留它们（它们本来就在那儿），**跨宿主**
    /// 还原会**丢**它们（新宿主里那些表是空的）。本方法不改变这一点——
    /// 规格冻结如此——只是**说清楚**，免得调用方以为载荷是自足的。
    pub fn restore_session(&mut self, payload: &SessionSnapshotDto) -> Result<(), PlatformError> {
        // ---- 第 0 步：先校验。一个字都还没改。 ----
        payload.validate()?;

        // ---- 第 0.5 步：外设状态的**只读**预检。一个字都还没改。 ----
        //
        // ⚠️ 这一步跑在 `load_map` / `load_pages` **之前**，是为了不让
        // 「外设状态畸形」留下一具**拼接体**：第 1 / 2 步已经写过内存，
        // 第 3 步才因外设畸形而失败的话，失败后的会话是「内存来自载荷、
        // 寄存器来自恢复前、外设来自恢复前」的**缝合怪**——而调用方只拿到
        // 一个 `Err`。
        //
        // ⚠️ 预检用的是 M-15 的**同一个**入口（[`m15_peripheral::validate_session_states`]），
        // 宿主不重写任何一条规则——一个约束一个落点。
        let instances = self.backend.bus().peripheral_instances();
        m15_peripheral::validate_session_states(&instances, &payload.m15_periph)?;

        // ---- 第 1 步：重建区域拓扑（RAM / ROM 重建；外设原地保留）。 ----
        self.backend.bus_mut().load_map(&payload.m14_map)?;

        // ---- 第 2 步：回灌 RAM 内容（页要写进**已存在**的区域里）。 ----
        self.backend
            .bus_mut()
            .load_pages(&payload.m13_state.mem_pages)?;

        // ---- 第 3 步：外设内部状态（按 id 双向匹配；检查全在写入之前）。 ----
        m15_peripheral::restore_session_states(&instances, &payload.m15_periph)?;

        // ---- 第 3.5 步：作废 M-11 的**会话级**缓存与历史。 ----
        //
        // ⚠️ 还原会**换掉内存里的机器码** ⇒ 反汇编缓存里那些按地址缓存的
        // 条目**可能陈旧** ⇒ 必须**作废**（否则 `Disasm` 给出**上个会话**的
        // 指令 ⇒ 调试器给出**错误**的指令）。
        //
        // ⚠️ 轨迹环是**会话级历史** ⇒ 还原后是**新会话** ⇒ 旧历史必须
        // **清空**（否则两个会话的轨迹混在一起，而 `overflow_marked` 仍说
        // 「没溢出」⇒ 前端会把不连续的轨迹当完整会话）。
        //
        // ⚠️ **符号表与行表是「工程级」的**（它们来自已装载工程的 ELF，
        // 还原会话**不换工程**）⇒ **保留**，不清。
        //
        // ⚠️ 这与 STAGE-5 §9.2 是**同一条规矩**（「装载新工程 = 新会话 ⇒
        // 旧的索引全部作废」）：还原会话也是新会话。
        self.disassembler = Disassembler::new();
        self.trace.clear();

        // ---- 第 4 步：已拉起的 IRQ 行。 ----
        self.load_intc_state(&payload.m16_intc)?;

        // ---- 第 5 步：最后还原引擎段（队列 / 寄存器 / 虚拟时间）。 ----
        self.backend.restore_full(&payload.m12_engine)?;

        Ok(())
    }

    /// 读出当前已拉起的 IRQ 行（段 5 的同一份导出）。
    ///
    /// ⚠️ 争用时（`RefCell` 拿不到）**记一次失败**并返回**空**段 5 ——
    /// 失败是**可见的**（[`Self::intc_delegation_failures`]），不是静默的
    /// 「丢一段状态却不吭声」。
    ///
    /// ⚠️ 但**载荷里没有「本段不完整」的标记**：空段 5 与「真的没有拉起
    /// 任何 IRQ」在载荷里**不可区分**。要判断采到没有，看
    /// [`Self::intc_delegation_failures`]。本批**不改**这个行为（改
    /// `capture_session` 的返回类型会波及整条链，而争用在生产路径上
    /// 不可达）——台账留账，下一批处理。
    fn intc_state_inner(&self) -> M16IntcDto {
        match self.intc.controller.try_borrow() {
            Ok(intc) => intc.export_state(),
            Err(_) => {
                self.intc
                    .failures
                    .set(self.intc.failures.get().wrapping_add(1));
                M16IntcDto::default()
            }
        }
    }

    /// 回灌已拉起的 IRQ 行（段 5 的消费者）。
    ///
    /// ⚠️ 争用（`RefCell` 拿不到）时**记一次失败并返回 `Err`**——与
    /// [`HostSinks::delegate`] **同一种结果**：同一个失败条件必须是
    /// 同一个口径（`Err` + 计数 +1），否则「同一个量两个真相」。
    fn load_intc_state(&self, state: &M16IntcDto) -> Result<(), PlatformError> {
        let mut intc = match self.intc.controller.try_borrow_mut() {
            Ok(intc) => intc,
            Err(_) => {
                self.intc
                    .failures
                    .set(self.intc.failures.get().wrapping_add(1));
                return Err(PlatformError::new(
                    ErrorFamily::Interrupt,
                    E_INT_01_CONTROLLER_BUSY,
                    "中断控制器正被占用，无法回灌已拉起的 IRQ 行",
                ));
            }
        };
        intc.load_state(state);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 差量合成（v0.32 · STAGE-17 裁决 A · D-145）
// ---------------------------------------------------------------------------

/// **差量合成** —— 把一串差量操作施加到一张**基座**图上，返回**整图**
/// （v0.32 · STAGE-17 裁决 A · **D-145** 闭环）。
///
/// ⚠️ **纯函数**：只读 `base` + 只写返回值 ⇒ **不碰宿主任何状态**
/// （生效的图 / 用户的板 / 修订号 / 总线）⇒ ⇒ 合成失败 ⇒ 宿主状态
/// **一个字都不动**。
///
/// ⚠️ **不进契约层**（裁决 A）：它是**宿主内部**的实现细节 ⇒ 契约层的
/// 公开表面**一字未改** ⇒ 前端镜像**不需要**升版。
///
/// ⚠️ **不跑 `validate()`**：校验**留在调用点**
/// （[`KernelHost::on_patch_memory_map`] 的第 ③ 步）⇒ ⇒ **既有语义一字
/// 不改**（裁决 M）⇒ ⇒ 这个函数可以被**单独调用**来测「合成逻辑」。
///
/// ⚠️ **合成顺序与语义照搬 v0.31 的循环体**（裁决 M）：
/// ① `UpsertRegion` —— 按 **`spec.base()`** 定位 ⇒ 命中**替换** / 未命中
///    **追加**（⇒ 与数组顺序无关）
/// ② `RemoveRegion` —— 定位不到 ⇒ **`E_PLT_14`**，**不静默忽略**
/// ③ `SetBudget` —— **两个预算字段一起改**（声明期 + 运行期）
fn compose_memory_map(
    base: &PlatformDescription,
    ops: &[MemoryMapOp],
) -> Result<PlatformDescription, PlatformError> {
    // ---- ① 复制 `base` 到工作副本（⇒ 原件一个字都不动）----
    let mut synthesized = base.clone();

    // ---- ② 逐条施加 `ops` ----
    for op in ops {
        match op {
            MemoryMapOp::UpsertRegion { spec } => {
                // ⚠️ 按 **`spec.base`** 定位：命中 ⇒ 替换；未命中 ⇒ 追加。
                let region_base = spec.base();
                match synthesized
                    .regions
                    .iter()
                    .position(|region| region.base() == region_base)
                {
                    Some(index) => synthesized.regions[index] = spec.clone(),
                    None => synthesized.regions.push(spec.clone()),
                }
            }
            MemoryMapOp::RemoveRegion { base } => {
                // ⚠️ 定位不到 ⇒ **明确报错**，不静默忽略（裁决 N）。
                match synthesized
                    .regions
                    .iter()
                    .position(|region| region.base() == *base)
                {
                    Some(index) => {
                        synthesized.regions.remove(index);
                    }
                    None => {
                        return Err(PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_14_REGION_NOT_FOUND,
                            format!("差量要删的那一段（基址 {base:#010x}）不在当前生效的图里"),
                        ));
                    }
                }
            }
            MemoryMapOp::SetBudget {
                region_budget_bytes,
                max_region_bytes,
            } => {
                // ⚠️ **两个预算字段一起改**（声明期 + 运行期）。
                synthesized.region_budget_bytes = *region_budget_bytes;
                synthesized.max_region_bytes = *max_region_bytes;
            }
        }
    }

    // ---- ③ 返回工作副本 ----
    Ok(synthesized)
}

// ---------------------------------------------------------------------------
// 契约 v3 调试面的自由函数
// ---------------------------------------------------------------------------

/// M-06 解析出的**调试节区**（名字 + 原始字节）。
///
/// ⚠️ 它**不过契约层**（契约只传查询结果，不传节区字节）⇒ 在**内核
/// 内部**从 M-06 交给 M-11（STAGE-5 §4 的数据流）。
type DebugSections = Vec<(String, Vec<u8>)>;

/// **解析后**的镜像（**还没写进总线**）。
///
/// ⚠️ 「解析」与「写入」分成两步，是因为内存图档位要按镜像用到的
/// **地址**挑 —— 见 [`KernelHost::parse_image`]。
struct ParsedImage {
    /// 入口地址（raw blob 是 0）。
    entry: u32,
    /// `(物理地址, 段内容)`。
    segments: Vec<(u32, Vec<u8>)>,
    /// 调试节区的原始字节（`(节名, 内容)`）。
    debug_sections: DebugSections,
}

/// 取指 / 读内存的**宽度映射**。
///
/// ⚠️ `BusWidth` **没有** `From<u32>` ⇒ 宽度要自己映射。
/// ⚠️ **必须真的只读 `size` 字节**（「读一个字取低半」会让映射区**末端**
/// 的压缩指令越界 ⇒ 被误报成「未映射」，LLD 11-1）。
fn fetch_bytes(bus: &Bus, addr: u32, size: u32) -> Result<u32, PlatformError> {
    let width = match size {
        1 => BusWidth::Byte,
        2 => BusWidth::Half,
        4 => BusWidth::Word,
        other => {
            return Err(PlatformError::new(
                ErrorFamily::Bus,
                1,
                format!("取指宽度 {other} 字节不受支持（只支持 1 / 2 / 4）"),
            ))
        }
    };
    // ⚠️ 用 `read_only`（不计入统计 / 不触发总线 hook）：调试器的读**不是**
    // 访客的访存 ⇒ 它不能进 M-18 的访存 trace 环，也不能进观察点匹配。
    bus.read_only(addr, width)
}

/// M-11 的行表判定 → 契约事件的**唯一**映射点（LLD 11-4 的两态）。
///
/// ⚠️ 抽成自由函数是为了让它**可测**：Wave 1 的 `DwarfLines::resolve` 恒
/// 返回降级态 ⇒ 「有 DWARF 但未命中」这一态在生产路径上**走不到**，
/// 只有直接喂三种 `LineLookup` 才能钉住这个映射。
fn source_location_event(addr: u32, lookup: LineLookup) -> EventPayload {
    match lookup {
        LineLookup::Found(entry) => EventPayload::SourceLocation {
            addr,
            file: entry.file,
            line: entry.line,
        },
        // 有 DWARF 但地址没命中 ⇒ `line = u32::MAX`（**不是** 0）。
        LineLookup::Missed => EventPayload::SourceLocation {
            addr,
            file: String::new(),
            line: u32::MAX,
        },
        // 压根没有 DWARF ⇒ `line = 0`。
        LineLookup::NoDwarf => EventPayload::SourceLocation {
            addr,
            file: String::new(),
            line: 0,
        },
    }
}

/// 「这个 `source` / `format` 组合不合法」的错误（`E_CTX-03`）。
///
/// ⚠️ **复用 M-11 已定义的常量**（一条错误码一个真相）：宿主**不**再
/// 定义一份 `E_CTX-03`——M-11 的 `trace` 模块里那份才是权威。
fn illegal_export(message: &str) -> PlatformError {
    PlatformError::new(
        TRACE_FAMILY,
        E_CTX_03_ILLEGAL_EXPORT_FORMAT,
        message.to_string(),
    )
}

/// M-19 的 `ExportHandle` → 契约的 `TraceExport` 事件（**唯一的映射点**）。
///
/// ⚠️ **为什么抽成自由函数**：这条映射里有**两处**「形状对不上」需要人工
/// 翻译 —— M-19 的 `ExportHandle` **没有** `offset` 字段（用请求里的回显）
/// **也没有** `bytes` 字段（用块的长度）。把它们藏在一个方法里 ⇒ 没人能
/// 单独测这条映射 ⇒ **v0.20 检视 B 实测**：把 `overflow_marked` 改成
/// `false`，**795 + 5 条用例全绿** ⇒ 「导出窗口不完整却没人知道」正是
/// 这个字段要防的事，而它在 `Waveform` 这一侧**没有判别力**。
///
/// ⚠️ 抽出来之后，那条映射**可以被单独钉住**（见 `waveform_export_event`
/// 的用例）。
fn waveform_export_event(
    handle: crate::m19_waveform::export::ExportHandle,
    format: TraceFormat,
    offset: u64,
) -> EventPayload {
    EventPayload::TraceExport {
        source: TraceSource::Waveform,
        format,
        // ⚠️ M-19 的 `ExportHandle` **没有** `offset` 字段 ⇒ 用**请求
        // 里的** offset 回显（它就是本块的起点）。
        offset,
        next_offset: handle.next_offset as u64,
        total: handle.total as u64,
        // ⚠️ M-19 的 `ExportHandle` **没有** `bytes` 字段 ⇒ 用块的长度。
        bytes: handle.chunk.len() as u64,
        chunk: handle.chunk,
        // ⚠️ **窗口不完整的标记必须原样传出去**（D-50 那一侧的「不完整」
        // 也得看得见）——这一行**曾经**没有任何判别力。
        overflow_marked: handle.overflow_marked,
    }
}

/// 「M-19 的波形管道不可用」的错误（`E_OBS-02`）。
fn pipeline_unavailable(message: &str) -> PlatformError {
    PlatformError::new(
        ErrorFamily::Observability,
        E_OBS_02_PIPELINE_UNAVAILABLE,
        message.to_string(),
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_session_snapshot;

#[cfg(test)]
mod tests_contract_v2;

#[cfg(test)]
mod tests_intc;

#[cfg(test)]
mod tests_m11;

#[cfg(test)]
mod tests_osz_wiring;

#[cfg(test)]
mod tests_v024;

#[cfg(test)]
mod tests_platform;

/// **D-151** —— `time` / `timeh` 的真值接 M-15 的 CLINT（冻结规格
/// STAGE-26 §4.2 / 裁决 F~K）。
///
/// ⚠️ **判别力全部落在这里**（裁决 J）：参考侧在**黄金配置**下 `time`
/// **恒读 0** ⇒ ⇒ 两侧**不可比** ⇒ ⇒ ⛔ **不许**新增查 `time` 的黄金语料
/// ⇒ ⇒ 五条判别式**全部**是 Rust 单测。
#[cfg(test)]
mod tests_time_source;

/// 器件连线**数据化**（v0.34 · D-125 · STAGE-19 · 验收 B-1 ~ B-5）。
///
/// ⚠️ 它钉的是**宿主这一段**：描述里的 `wiring` ⇒ 抬到哪个源 / 默认连线
/// ⇒ 存量行为一字未变 / 缺一端 ⇒ 不抬线 / 写死的那个常量已消失。
#[cfg(test)]
mod tests_wiring_v034;

/// 板级内存图（v0.29 · STAGE-14 · 验收 B-1 ~ B-9）。
///
/// ⚠️ 它钉的是**宿主**这一段：立即生效 / 待生效 / 装载时应用 /
/// 订阅即推送。
#[cfg(test)]
mod tests_board_memory_map;

/// 板级内存图 · **差量 / 卸载 / 预算拆分**（v0.31 · STAGE-16 · 验收
/// B-1 ~ B-14）。
///
/// ⚠️ 与 [`tests_board_memory_map`]（v0.29 的那批）**分开一个文件** ⇒
/// 「既有断言一条不改」这件事由 `git diff` **一眼可见**。
#[cfg(test)]
mod tests_board_delta_unload;

/// 板级内存图 · **差量与整份两条路径的等价性**（v0.32 · STAGE-17 · 验收
/// B-1 ~ B-7 · 台账 **D-145** 闭环）。
///
/// ⚠️ 与 [`tests_board_delta_unload`]（v0.31 的那批）**分开一个文件** ⇒
/// 「纯函数化是**等价重构**」由「那边**一条不改**仍绿」**证明**。
#[cfg(test)]
mod tests_board_delta_equivalence;

/// 宿主看门狗（v0.28 · 台账 **D-115** · 冻结规格 STAGE-13 §3.3）。
///
/// ⚠️ **它是公开的**：判定（[`classify_window`]）与状态机
/// （[`HostWatchdog`]）是**纯逻辑** ⇒ 谁都能单独测它；而**接线**留在宿主
/// 内部（裁决 F：看门狗**全在宿主侧**，M-12 零改动）。
pub mod watchdog;

/// 宿主单调时钟（v0.28 · D-115 的**修复**）。
///
/// ⚠️ **它是公开的**：native 与 `wasm32` 各有一份实现，**对外形状一样**
/// ⇒ 调用方（看门狗 / M-21 / M-22）**不需要**知道自己在哪个 target 上。
///
/// ⚠️ **为什么要有它**：`std::time::Instant::now()` 在
/// `wasm32-unknown-unknown` 上**直接 panic** ⇒ 宿主若直接用它，
/// **WASM 构建一启动就崩**（实测 panic 落在 `KernelHost::with_description`）。
pub mod clock;

#[cfg(test)]
mod tests_watchdog;

/// 平台描述的加载 · 器件工厂 · 建图（v0.27 · D-123-B）。
///
/// ⚠️ **它是公开的**：外部（headless runner / 将来的 server crate）可能
/// 要**按 id 取一份描述**或**从文件读一份描述** ⇒ 入口在
/// [`KernelHost::new_with_platform`] / [`KernelHost::new_with_platform_file`]，
/// 描述本身在 [`platform`]。
pub mod platform;
