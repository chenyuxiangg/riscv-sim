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
//! ⚠️ **v0.57 批（STAGE-42）** 在这张表**之外**又加了两条命令：
//!
//! | 命令 | 内核行为 | 回复事件 |
//! | --- | --- | --- |
//! | `ResolveSymbol{name}` | M-11 的 `SymbolTable`（精确命中 + 前缀候选） | `SymbolResolved{name,symbol,candidates,total}` |
//! | `DumpMemory{addr,len}` | M-14 的 `read_only`（**逐字节** · **调试视角**） | `MemoryDump{addr,bytes}` |
//!
//! ⚠️ **这两条也是「回复事件」，不经 topic 过滤**（同 STAGE-5 §9.1）⇒ ⇒
//! 前端靠传输帧的 `reqId` 关联 ⇒ ⇒ **不需要先订阅**。
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
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use riscv_sim_contract::csr::{IRQ_SOFTWARE, IRQ_TIMER_COMPARE, IRQ_UART_RX_READY};
use riscv_sim_contract::error::{
    ErrorFamily, PlatformError, DEBUG_FAMILY, E_DBG_02_CONDITION_INVALID, E_ENG_03_NO_PROGRESS,
    E_HMS_02_FATAL_TERMINATED, E_PLT_22_NO_BOARD, E_PROTO_01_VERSION_MISMATCH, PLATFORM_FAMILY,
    PROTO_FAMILY,
};
use riscv_sim_contract::model::{
    DeltaRegister, Edge, ImageKind, ImageRef, ImageSource, Project, WatchExpr, WatchValue,
    WaveformHistory, WaveformStream, WriteSize, SIM_STATE_SCHEMA_VERSION,
};
use riscv_sim_contract::platform::PlatformDescription;
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::{
    CommandPayload, Direction, EventPayload, HaltReason, Message, Payload,
};
use riscv_sim_contract::snapshot::DEFAULT_INSTRUCTION_BUDGET;
use riscv_sim_contract::SCHEMA_VERSION;

use crate::m06_elf::{DefaultLoader, Kind as LoadKind, ParseLimits};
use crate::m08_breakpoint::watch_expr::WatchTarget;
use crate::m11_symbol::{Disassembler, SymbolIndex, TraceBuffer, TRACE_CAP};
use crate::m15_peripheral;
use crate::m16_interrupt::InterruptController;
use crate::m17_backend::SelfIssBackend;
use crate::m18_probe::{
    ChannelDef, ChannelKind, ProbeRegistry, SampleCfg, CH_GPIO0, CH_IRQ_EXTERNAL, CH_IRQ_SOFTWARE,
    CH_IRQ_SRC_BASE, CH_IRQ_TIMER,
};
use crate::m19_waveform::{ChannelMeta, SharedPipeline, WaveformPipeline};
use crate::m20_snapshot::SnapshotManager;
use crate::m21_logging::{LogBuffer, LogLevel};
use crate::m22_hms::{
    DegradeAction, HealthSignal, Hms, Role, Severity, SignalCtx, SignalKind, SignalSrc,
};

/// 管理员通道：**一次日志导出**的条目数**上界**（v0.44 · STAGE-29 · B-6）。
///
/// ⚠️ **为什么必须有上界**：管理员通道**跨进程边界** ⇒ ⇒ 一次
/// `DrainLogs` 若不设上界，就能把**整个日志环**搬过边界（WASM 形态默认
/// 7 000 条、服务端形态 13 万条）⇒ ⇒ 载荷体积不可控 ⇒ ⇒ 收消息的一侧
/// 会被一次 admin 请求打爆。
///
/// ⚠️ **为什么是 256**：与 M-22 的
/// [`RETAINED_BAND_CAPACITY`](crate::m22_hms::RETAINED_BAND_CAPACITY)（= 256）**同族**
/// ——「取证要留多少」的同一量级 ⇒ ⇒ 一条日志约 200 字节 ⇒ ⇒ 一次导出
/// 约 50 KB ⇒ ⇒ 对 `postMessage` / JSON 载荷都是**温和**的量。
/// ⚠️ **将来调小不影响语义**（只是更早截断）⇒ ⇒ 截断**必须明确告知**
/// （**不是静默** · 见 [`KernelHost::on_admin`]）。
pub const ADMIN_DRAIN_MAX_ENTRIES: usize = 256;

/// 管理员通道：**诊断包**这个 op 的名字（`AdminOp::Custom` 的字符串）。
///
/// ⚠️ **它是一个约定，不是一个类型**（台账 D-129 的已知边界 4）⇒ ⇒ 只有
/// **逐字等于** `"diag"` 的 `Custom` 才走诊断包 ⇒ ⇒ **别的** `Custom`
/// 字符串一律回 `E_CTX-05`（**不猜**别的 op 名）。
pub const ADMIN_DIAG_OP: &str = "diag";

/// 管理员通道：**截断说明**那条日志的 `ctx`（≤ 24 字节 · M-21 的上限）。
pub const ADMIN_DRAIN_LOG_CTX: &str = "ADMIN-DRAIN";

/// 管理通道未启用时**原样**回的那句消息（**一字不改** · X-3）。
///
/// ⚠️ **默认行为一字不变**（裁决 G）：宿主对 `Admin` 命令的**默认**回应
/// **必须**与 v0.43 之前**逐字相同** ⇒ ⇒ 这句**逐字保留**。
///
/// ⚠️ **为什么开启通道后 `KillGuest` / `PersistProject` 也回这一句**
/// （裁决 H · 不假装实现了）：**一个码一个含义** ⇒ ⇒ `E_CTX-05` 这个码
/// 只表示「管理通道对这个 op 没开」⇒ ⇒ 括号里那句「admin op 不在当前会话
/// 的能力面内」对**未实现的 op** 同样**成立**（它们确实不在能力面内）
/// ⇒ ⇒ 不给同一个码**换第二个含义**。
pub const ADMIN_CHANNEL_DISABLED_MESSAGE: &str =
    "管理通道未启用（admin op 不在当前会话的能力面内）";

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

/// 核中断通道的流 id 前缀（M-18 的 id = `"{source}.{signal}"`）。
pub const IRQ_CORE_SOURCE: &str = "irq.core";

/// 外设中断通道的流 id 前缀。
pub const IRQ_DEV_SOURCE: &str = "irq.dev";

/// 软中断通道标签（**外设名 + 中断号**的同一写法：名字 + 号）。
pub const IRQ_LABEL_SOFTWARE: &str = "software · IRQ 3";

/// 定时器中断通道标签。
pub const IRQ_LABEL_TIMER: &str = "timer · IRQ 7";

/// 外部中断通道标签。
///
/// ⚠️ **不写 "IRQ 11"**：cause 11 与某台设备的 PLIC 源 11 同数字 ⇒ 写
/// `MEIP` 才是无歧义的那个名字（源号在设备通道那一侧出现）。
pub const IRQ_LABEL_EXTERNAL: &str = "external · MEIP";

/// 外设通道标签：`"{id} · IRQ {source}"`（**外设名 + 中断号**）。
pub fn irq_device_label(id: &str, source: u16) -> String {
    format!("{id} · IRQ {source}")
}

/// 线号 → 核中断通道。未知名（已退役的线 2）⇒ `None`。
fn irq_ch_for_line(line: u16) -> Option<u8> {
    // ⚠️ 必须用 `if` 比**值**，不能 `match` 常量名 —— 常量若不在作用域里，
    // `match` 会把它当成**绑定**（irrefutable）⇒ 第一条臂吞掉所有线号，
    // 编译器只给一句"unused variable"，波形却全画到一条通道上。
    if line == IRQ_SOFTWARE {
        Some(CH_IRQ_SOFTWARE)
    } else if line == IRQ_TIMER_COMPARE {
        Some(CH_IRQ_TIMER)
    } else if line == IRQ_UART_RX_READY {
        Some(CH_IRQ_EXTERNAL)
    } else {
        None
    }
}

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
    classify_window, classify_window_with_clock, HostWatchdog, WindowOutcome,
    HOST_WATCHDOG_BUDGET_MS, HOST_WATCHDOG_CONFIRMATIONS, WATCHDOG_ERR_CODE, WATCHDOG_LOG_CTX,
};

/// 事件主题名（与前端 `EVENT_TOPIC` 逐字对齐）—— 定义在 [`topics`] 子模块，这里转出。
pub use topics::*;

/// 兼容档平台常量（GPIO / 默认内存图 / QEMU virt 地址、器件 id、波特率）——
/// 定义在 [`platform_consts`] 子模块，这里转出。
pub use platform_consts::*;

/// 宿主内部的窗口判定结果（不是公开 API —— 见 [`watchdog::WindowVerdict`]）。
use self::watchdog::WindowVerdict;

/// 宿主单调时钟（v0.28 · D-115 的**修复**）—— **不能**直接用
/// `std::time::Instant`：它在 `wasm32-unknown-unknown` 上会 **panic**
/// ⇒ WASM 构建一启动就崩。详见 [`clock`] 模块。
use self::clock::{HostClock, HostInstant};

use self::image::ParsedImage;

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

/// 一条观察表达式（原文 + 已解析目标）。解析在**设置时**完成。
struct WatchEntry {
    expr: WatchExpr,
    target: WatchTarget,
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
    /// 上一次已发布的 `SimStateDelta` 寄存器值（与波形那份分开记账）。
    delta_published_regs: [u32; 32],
    /// 上一次已发布的 `SimStateDelta` PC。
    delta_published_pc: u32,
    /// 是否已经发过全量 `SimStateDelta`。
    delta_published_valid: bool,
    /// M-18 探针注册表（D-46）—— 判沿 + 访存 trace 环。
    probes: Option<Rc<RefCell<ProbeRegistry>>>,
    /// M-19 波形管道（D-46）—— 边沿的环形缓冲（导出用）。
    waveform: Option<SharedPipeline>,
    /// 探针管道**装配失败**的累计次数（V-A 检视 · 🟡-6 / 🟡-7）。
    ///
    /// 装配期的 `define_channel` / `attach_signal` 失败原本被 `let _ =`
    /// 静默吞掉——通道没接上，波形视图就是空的，而**没有任何报错**。
    /// 现在失败是**可见的**：计数器 + [`KernelHost::probe_bridge_failures`]。
    probe_bridge_failures: u32,
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

    /// **逐实例**清单（STAGE-43 · v0.66 · D-175）—— **当前生效的图**里的
    /// 每个器件实例：谁、什么类型、绑在哪个基址、窗口多大。
    ///
    /// ⚠️ **建图时顺带产出** ⇒ ⇒ 与总线上的实际布局**同源** ⇒ ⇒ 不另算
    /// 一遍（裁决 K）。
    ///
    /// ⚠️ **它是「生效的」那一份** ⇒ ⇒ 用户配的那块板（`user_board`）里
    /// **还没生效**的实例，由 [`KernelHost::device_instances_event`] 按
    /// 「与生效图逐字段比对」**推导**出来（`base = None` ⇒ 待生效）⇒ ⇒
    /// **不另存**第二份清单（少一个状态就少一处可能不同步的地方）。
    device_instances: Vec<riscv_sim_contract::model::DeviceInstanceInfo>,

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

    /// **按 id 索引**的器件实例表（v0.72 · STAGE-48 · D-177 B）—— 宿主手里
    /// **真正持有**的那些句柄。
    ///
    /// ⚠️ **它取代了三个固定角色句柄**（`uart0` / `clint` / `plic`）⇒ ⇒
    /// 「板上的器件实例」**不再**被塌缩成三个角色 ⇒ ⇒ **第二个 UART 也有
    /// 句柄** ⇒ ⇒ 第二个 UART 的线**真的**能通电。
    ///
    /// ⚠️ **角色 = id，句柄 = 从表里查出来的**（裁决 B）⇒ ⇒ 表是**唯一
    /// 真相源** ⇒ ⇒ 「console 是哪个 / 时间源是哪个」只由**角色 id**决定
    /// ⇒ ⇒ 少一个状态就少一处可能不同步的地方。
    devices: platform::DeviceInstanceTable,
    /// **外设中断源表** —— 从**生效的描述**编译：每个 UART/GPIO 一个 PLIC 源号。
    ///
    /// ⚠️ 建图时重算（[`KernelHost::map_memory`]）⇒ 换板即换表 ⇒ 宿主源码里**没有**第二个「PLIC 源号」。
    peripheral_irqs: Vec<platform::PeripheralIrq>,
    /// **外设中断通道表**：`(PLIC 源号, 通道号)`，随平台重建。
    ///
    /// ⚠️ 通道号取「声明序号 + 基址」而不是源号本身：源号按 PLIC 规范
    /// 可到 1023，`ch` 是 `u8` 装不下。这张表就是二者的**唯一**映射。
    irq_src_channels: Vec<(u32, u8)>,

    /// console / 时间源 / 主 PLIC。
    ///
    /// ⚠️ **缺省 ⇒ 今天的口径逐字不变**（第一个 UART / 第一个 CLINT）⇒ ⇒
    /// 存量板档案**零影响**。
    roles: platform::ResolvedRoles,
    /// 已经从 UART 取走、但**还没推给前端**的 console 字节。
    ///
    /// ⚠️ **必须**及时把 UART 的字节取走（捕获环满了会丢最新的字节），
    /// 但前端不一定订阅了 console ⇒ 中间需要一个攒着的缓冲。
    console_backlog: Vec<u8>,
    /// M-22 健康管理（v0.28 · 台账 **D-115**）—— 看门狗触发的**处置**走它。
    ///
    /// ⚠️ **不提供 `Clone`**（M-22 持有 trait object 字段）⇒ 宿主**持有**
    /// 它，不复制它。
    ///
    /// ⚠️ **角色也住在这里**（v0.45 · STAGE-30 · 裁决 B）：角色的**唯一存储**
    /// 是 M-22 那个 `role` 字段 ⇒ ⇒ 宿主**不另存一份** ⇒ ⇒ **零状态新增**
    /// ⇒ ⇒ **不存在**「宿主与 `Hms` 角色不一致」那种状态。
    hms: Hms,
    /// M-21 日志缓冲 —— 与 [`KernelHost::hms`] **共享同一份**。
    ///
    /// ⚠️ **一个真相**：`Hms::install_log_source` 收的是
    /// `Rc<RefCell<LogBuffer>>`（形状与 M-18 的探针表同款）⇒ 宿主持有
    /// `Rc`、把它**克隆**给 HMS ⇒ 管理员通道看到的日志关联与宿主记的日志
    /// 是**同一条**。存两份 `LogBuffer` 就是两个真相。
    log_buffer: Rc<RefCell<LogBuffer>>,
    /// 降级派发记录 —— 与降级执行器**共享同一份**（v0.46 · STAGE-31 · 裁决 **I**）。
    ///
    /// ⚠️ **一个真相**：宿主与 [`degrade::HostDegradeExecutor`] 拿的是**同一个**
    /// `Rc` ⇒ ⇒ 「执行器被问过几次」这件事**只有一份**记录 ⇒ ⇒ 宿主不需要
    /// 另存一份（另存就是两个真相）。
    ///
    /// ⚠️ **它只记数 + 记最后一次动作** ⇒ ⇒ **不记内容** ⇒ ⇒ 「派发路径真的
    /// 走过」这件事在宿主层**看得见**（[`KernelHost::degrade_dispatch_total`] /
    /// [`KernelHost::degrade_dispatch_last`]）。
    degrade_dispatch: Rc<RefCell<degrade::DegradeDispatch>>,
    /// 宿主看门狗（v0.28 · **D-115**）—— 两条驱动路径（`StepN` / `Continue`）
    /// 共用**同一个**状态（连续性是它存在的意义）。
    watchdog: HostWatchdog,
    /// 会话起点（宿主单调时钟的零点）—— M-21 / M-22 的 `time_host_ms` 口径。
    started_at: HostClock,
    /// 管理员通道的日志导出**被上界截掉**的条数（累计 · B-6）。
    ///
    /// ⚠️ **可见的丢弃**（不静默）：截断发生时除了在 M-21 的环里写一条
    /// 说明（见 [`KernelHost::on_admin`]），这里还留一个**机器可读**的计数
    /// ⇒ 「少了一批」这件事**两处都看得见**。
    admin_drain_truncated: u64,
    /// 上一次「命中断点」日志的**虚拟时间**（皮秒）—— **≥1s 限频**的唯一状态。
    ///
    /// ⚠️ 用**虚拟时间**而不是宿主墙钟：日志顺序必须可复现（同一份快照
    /// 两次回放的日志序列逐条相同）。
    bp_hit_log_last_ps: Option<u64>,
    /// 限频窗口内被压掉的命中日志条数（下一条放行日志里如实带出）。
    bp_hit_log_suppressed: u64,
    /// 观察表达式（按 id 覆盖）。**不参与**运行期停止判断。
    watch_items: Vec<WatchEntry>,
    /// 上一次轮询的观察值（用于计算 `changed`）。
    watch_last: BTreeMap<u32, WatchValue>,
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
        // ⚠️ v0.46 · STAGE-31（裁决 **H**）：**默认降级策略** = `Fatal →
        // Terminate` ⇒ ⇒ **默认行为一字不变**（验收 **C-6**）⇒ ⇒ 「默认」由
        // **策略表**承载 ⇒ ⇒ **不是**「宿主直接调 `degrade`」。
        //
        // ⚠️ 宿主里**只有** `hms.report(...)` 这一处会报出 `Fatal`（看门狗
        // 处置）⇒ ⇒ 这条策略**只在**看门狗触发时生效 ⇒ ⇒ 别的路径**不受
        // 影响**。
        hms.set_degrade_policy(Severity::Fatal, DegradeAction::Terminate);
        let mut host = Self {
            backend: SelfIssBackend::new(sinks.clone(), sinks.clone()),
            sinks,
            topics: BTreeSet::new(),
            seq: 0,
            snapshot_counter: 0,
            published_regs: [0; 32],
            published_pc: 0,
            published_valid: false,
            delta_published_regs: [0; 32],
            delta_published_pc: 0,
            delta_published_valid: false,
            probes: None,
            waveform: None,
            probe_bridge_failures: 0,
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
            // STAGE-43 · D-175：逐实例清单由**建图**产出 ⇒ 新会话先空着。
            device_instances: Vec::new(),
            // STAGE-16 裁决 A：修订号**从 0 起**。
            map_revision: 0,
            // STAGE-16 裁决 C 第 ④ 步：新会话**还没有**装载过的工程。
            last_project: None,
            // ⚠️ v0.72（STAGE-48 · D-177 B）：实例表在**建图时**产出 ⇒
            // 新会话先空着。
            devices: platform::DeviceInstanceTable::default(),
            // ⚠️ v0.34（D-125）：连线表在**建图时**编译（`map_memory`）⇒
            // 这里只是**初值**（还没有建图 ⇒ 还没有表）。
            peripheral_irqs: Vec::new(),
            irq_src_channels: Vec::new(),
            // ⚠️ v0.72（STAGE-48 · D-177 A）：丢线报告**随图**重算 ⇒ 这里
            // 只是**初值**（还没有建图 ⇒ 没有线 ⇒ 0 根）。
            roles: platform::ResolvedRoles::default(),
            console_backlog: Vec::new(),
            hms,
            log_buffer,
            // ⚠️ v0.46 · STAGE-31：派发记录**只有一份** ⇒ ⇒ 宿主与执行器
            // 共享它（一个真相）。
            degrade_dispatch: Rc::new(RefCell::new(degrade::DegradeDispatch::default())),
            watchdog: HostWatchdog::new(),
            started_at: HostClock::new(),
            // ⚠️ 管理通道**默认关**（裁决 E）⇒ 宿主对 `Admin` 命令的**默认**
            // 回应**一字不变**（仍是 `E_CTX-05`）。
            //
            // ⚠️ 角色的**唯一存储**在 `Hms` 里（裁决 B）⇒ ⇒ 这里**不再**有
            // 一个独立布尔旋钮 ⇒ ⇒ 默认角色是 `Product`（`Hms::new()` 就是
            // `with_role(Role::Product)`）⇒ ⇒ 准入判据天然为「关」。
            admin_drain_truncated: 0,
            bp_hit_log_last_ps: None,
            bp_hit_log_suppressed: 0,
            watch_items: Vec::new(),
            watch_last: BTreeMap::new(),
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

    /// 按**角色**造一个会话（角色 = 管理通道的**唯一**准入判据 · v0.45 ·
    /// STAGE-30 · 裁决 C / D / E）。
    ///
    /// ⚠️ **默认角色 `Product`** ⇒ ⇒ 宿主对 `Admin` 命令的**默认**回应**仍然**
    /// 是 `E_CTX-05` ⇒ ⇒ **默认行为一字不变**（裁决 E）⇒ ⇒ 零回归风险。
    ///
    /// ⚠️ **角色取代布尔开关**（裁决 D）：`Role::Admin` ⇒ 管理通道开；
    /// `Role::Product` ⇒ 关 ⇒ ⇒ **独立布尔旋钮已消失** ⇒ ⇒ 「角色」与
    /// 「开关」两个状态**再也打不了架**（D-131 陷阱①的结构性风险消除）。
    ///
    /// ⚠️ **走唯一写入口**（[`KernelHost::set_role`]）⇒ ⇒ 宿主**不另存**一份
    /// 角色 ⇒ ⇒ 角色的**唯一存储**是 `Hms.role`（裁决 B）。
    ///
    /// ⚠️ **本批只兑现两个 op**（v0.44 · STAGE-29 裁决 H）：`DrainLogs` 与
    /// `Custom("diag")` ⇒ ⇒ `KillGuest` / `PersistProject` **仍**回
    /// `E_CTX-05`（**不假装实现了** · X-4）。
    pub fn with_role(mut self, role: Role) -> Self {
        self.set_role(role);
        self
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
                host.probe_bridge_failures += 1;
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
        // ⚠️ **v0.72（STAGE-48 · D-177 B）**：句柄**全部**进**按 id 索引**
        // 的实例表 ⇒ ⇒ 「第二个 UART」**也有句柄** ⇒ ⇒ 它的线**真的**能
        // 通电（裁决 E）。
        self.devices = devices.handles;
        // ⚠️ **角色**（STAGE-49 · D-177 的 C）：描述里声明了就用它，没声明就
        // **今天那个口径**（第一个 UART / 第一个 CLINT）⇒ ⇒ **逐字不变**。
        self.roles = devices.roles;
        // ⚠️ **逐实例**清单（STAGE-43 · v0.66 · D-175）—— **建图时**顺带
        // 产出 ⇒ ⇒ 与总线上的实际布局**同源** ⇒ ⇒ 不另算一遍。
        self.device_instances = devices.instances;
        // ⚠️ **D-151**：把**同一个** CLINT 适配成**只读**时钟端口，装进后端
        // ⇒ ⇒ guest 读 `time` 与宿主调试读 `time` **同源**（STAGE-26 验收
        // B-9）⇒ ⇒ 平台时钟的真相**只有一份**（CLINT 自己那份）。
        //
        // ⚠️ **角色 = id，句柄 = 从表里查出来的**（裁决 B）⇒ ⇒ 时间角色
        // 仍是「**第一个** CLINT」（裁决 D：逐字不变）。
        //
        // ⚠️ **没有 CLINT 的板 ⇒ 没有时钟** ⇒ ⇒ `time` / `timeh` 报 **0**
        // ⇒ ⇒ `default` / `minimal` 两档与黄金 runner 的行为**逐字不变**。
        // ⚠️ **时间源角色**（STAGE-49 · D-177 的 C）：声明了就用它，没声明
        // ⇒ ⇒ **第一个 CLINT**（今天的口径，逐字不变）。
        // ⚠️ ⚠️ **只走 `resolve_roles` 那一个落点**（v0.73 施工期订正）：这里
        //   **原来自己**又做了一遍「缺省 ⇒ 第一个 CLINT」的回落 ⇒ ⇒ ⇒
        //   「缺省行为不变」这条性质被**两道**护栏保证 ⇒ ⇒ ⇒ **结构上不可
        //   证伪** ⇒ ⇒ ⇒ 变异 T-4 **实测零红** ⇒ ⇒ ⇒ ⇒ 现在这里**不**再
        //   自己回落 ⇒ ⇒ ⇒ 「回落」这条**只有一处**（一个约束一个落点）。
        let time_source = self
            .roles
            .time_source
            .as_deref()
            .and_then(|id| self.devices.clint(id))
            .map(|clint| platform::clint_time_source(&clint));
        self.backend.install_time_source(time_source);
        self.platform = description.clone();
        // ⚠️ **v0.34（D-125）**：**连线表在建图时从生效的描述编译**（冻结
        // 规格 STAGE-19 §3.1）⇒ ⇒ 「换一块板改连线」不用改宿主 ⇒ ⇒
        // 宿主抬线时读的是**这张表**，不是源码里的一个数。
        //
        // ⚠️ **v0.72（STAGE-48 · D-177 A/B）**：**表与丢线报告一次编译**
        // ⇒ ⇒ 两者**同源** ⇒ ⇒ 报告说的**就是**表里的那些线 ⇒ ⇒ 不会
        // 出现「报告说丢了、表里还在」这种自相矛盾。
        self.peripheral_irqs = platform::compile_peripheral_irqs(description);
        Ok(())
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
        // ---- IRQ 电平通道表：核 3 条 + 每台已接线设备 1 条 ----
        //
        // ⚠️ **按平台现算**：`peripheral_irqs` 是建图时从描述编译出来的
        // ⇒ 产品视图新增器件 / 改中断号 ⇒ `rebuild_platform()` ⇒ 这里重跑
        // ⇒ 波形自动多一条；删器件同理自动消失。
        //
        // ⚠️ `ch` 只是**内部键**（0..4 已被表 68 占，且 cause 与 PLIC 源号
        // 会撞号）：核用 16/17/18，设备用 19 + 声明序号。对外身份是流 id
        // （`irq.core.software` / `irq.dev.uart0`）与 label（`uart0 · IRQ 10`）。
        let mut irq_defs: Vec<(u8, String, String, String)> = vec![
            (
                CH_IRQ_SOFTWARE,
                IRQ_CORE_SOURCE.to_string(),
                "software".to_string(),
                IRQ_LABEL_SOFTWARE.to_string(),
            ),
            (
                CH_IRQ_TIMER,
                IRQ_CORE_SOURCE.to_string(),
                "timer".to_string(),
                IRQ_LABEL_TIMER.to_string(),
            ),
            (
                CH_IRQ_EXTERNAL,
                IRQ_CORE_SOURCE.to_string(),
                "external".to_string(),
                IRQ_LABEL_EXTERNAL.to_string(),
            ),
        ];
        let mut irq_src_channels: Vec<(u32, u8)> = Vec::new();
        for (i, irq) in self.peripheral_irqs.iter().enumerate() {
            let ordinal = match u8::try_from(i) {
                Ok(v) => v,
                // 设备多到 `u8` 装不下是平台错误（不是运行时错误）⇒ 记一次
                // 桥接失败，后面的设备不再建通道（可见，不静默）。
                Err(_) => {
                    self.probe_bridge_failures += 1;
                    break;
                }
            };
            let ch = CH_IRQ_SRC_BASE.saturating_add(ordinal);
            irq_defs.push((
                ch,
                IRQ_DEV_SOURCE.to_string(),
                irq.id.clone(),
                irq_device_label(&irq.id, irq.source),
            ));
            irq_src_channels.push((u32::from(irq.source), ch));
        }

        let shared = SharedPipeline::new({
            let mut p = WaveformPipeline::new();
            p.define_channel(
                CH_GPIO0,
                ChannelMeta::multi_bit(GPIO_SIGNAL_LABEL, GPIO_VALUE_WIDTH),
            );
            // ⚠️ 中断通道一律 **single_bit**：注册表只在 0↔1 变化时发边沿
            // ⇒ 画出来就是「有中断=1 / 没有=0」的阶梯，不会像旧脉冲通道
            // 那样把**线号**当电平。
            for (ch, _source, _signal, label) in irq_defs.iter() {
                p.define_channel(*ch, ChannelMeta::single_bit(label.clone()));
            }
            p
        });

        // M-18 → M-19 握手：每条边沿同时进 M-19 的环形缓冲。
        registry
            .borrow_mut()
            .set_edge_sink(Some(Box::new(shared.clone())));

        // M-18 的通道定义（通道号与 M-19 的**必须**一致）。
        //
        // ⚠️ 失败**不能静默吞掉**：通道没接上时波形视图就是空的。失败计入
        // `probe_bridge_failures`，由 [`KernelHost::probe_bridge_failures`] 暴露。
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
            self.probe_bridge_failures += 1;
        }

        for (ch, source, signal, _label) in irq_defs.iter() {
            if registry
                .borrow_mut()
                .define_channel(ChannelDef::single_bit(*ch, source.clone(), signal.clone()))
                .is_err()
            {
                self.probe_bridge_failures += 1;
            }
        }

        // 打开探针：master + 通道。
        let mut cfg = SampleCfg {
            master_enable: true,
            ..Default::default()
        };
        cfg.enable_channel(CH_GPIO0);
        // ⚠️ 全部中断通道都**开着采样**：筛选是**前端**的事 —— 后端若按
        // 勾选关采样，用户中途勾上一项就永远没有历史可看。
        for (ch, _source, _signal, _label) in irq_defs.iter() {
            cfg.enable_channel(*ch);
        }
        registry.borrow_mut().set_rate(cfg);
        self.irq_src_channels = irq_src_channels;

        // M-16 → M-18 的**电平**回调：抬线写 1、撤线写 0。
        //
        // ⚠️ 为什么不是一条「脉冲」通道：脉冲把**线号当值**画 —— 既看不出
        // 「有/无」，也表达不了「落了」。抬/撤两条回调合起来，每条线就有了
        // 自己的 0/1 边沿流；注册表按值去重 ⇒ 重复请求不产生额外边沿。
        //
        // ⚠️ 回调里**不许回头碰控制器**（`request()` / `clear()` 期间控制器
        // 是借出的）⇒ 只碰探针表，且 `try_borrow_mut` 争用时丢弃（与访存
        // hook 同一口径）。
        let rise_reg = Rc::clone(&registry);
        let rise = Box::new(move |now_ps: u64, line: u16| {
            let Some(ch) = irq_ch_for_line(line) else {
                return;
            };
            let Ok(mut reg) = rise_reg.try_borrow_mut() else {
                return;
            };
            let _ = reg.sample(now_ps, ch, 1);
        });
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => intc.set_pulse_sink(rise),
            Err(_) => {
                // 控制器正被占用 ⇒ 抬线回调没装上：这一轮不会上升沿。
                self.probe_bridge_failures += 1;
            }
        }

        let fall_reg = Rc::clone(&registry);
        let fall = Box::new(move |now_ps: u64, line: u16| {
            let Some(ch) = irq_ch_for_line(line) else {
                return;
            };
            let Ok(mut reg) = fall_reg.try_borrow_mut() else {
                return;
            };
            let _ = reg.sample(now_ps, ch, 0);
        });
        match self.intc.controller.try_borrow_mut() {
            Ok(mut intc) => intc.set_clear_sink(fall),
            Err(_) => {
                self.probe_bridge_failures += 1;
            }
        }

        // M-14 总线 → M-18 的访存 trace 环。
        // `make_bus_hook` 自带争用处理：注册表被占用时丢弃这条访存并
        // 计数，**不阻塞**总线派发路径。
        self.backend
            .bus_mut()
            .add_hook(ProbeRegistry::make_bus_hook(Rc::clone(&registry)));

        self.probes = Some(Rc::clone(&registry));
        self.waveform = Some(shared.clone());

        // ⚠️ v0.46 · STAGE-31（裁决 **I**）：降级执行器**随句柄一起装** ⇒ ⇒
        // 句柄换新时执行器**一起换** ⇒ ⇒ **结构上不可能**持有过期句柄。
        //
        // ⚠️ **装在这一段的末尾**：句柄（`registry` / `shared`）**已经**是
        // 最终的那一份 ⇒ ⇒ 执行器拿到的与宿主**永远是同一份**（一个真相）。
        //
        // ⚠️ 这一行是「降级**走派发路径**」的**唯一**前提：没有它 ⇒ ⇒
        // M-22 只会走自己的内置实现 ⇒ ⇒ `degrade_dispatch_last()` 恒空。
        self.hms
            .install_executor(Box::new(degrade::HostDegradeExecutor::new(
                Rc::clone(&registry),
                shared,
                Rc::clone(&self.degrade_dispatch),
            )));
    }

    /// 采一次**设备级**中断电平（PLIC 网关电平），喂给该设备的 0/1 通道。
    ///
    /// ⚠️ **只在电平真的变了**才产生边沿（注册表按值去重）⇒ 每拍无条件调用
    /// 即可，开销只多一次比较。
    ///
    /// ⚠️ 画的是**网关电平**（设备有没有在请求 PLIC），**不是**「核有没有
    /// 看到」—— 后者是 `external · MEIP` 那条通道。PLIC 源落下**不 retract**
    /// 已送进 PLIC 的请求（Renode 口径），所以两条通道**允许**不同步。
    fn sample_irq_source(&self, source: u32, level: bool) {
        let Some(ch) = self
            .irq_src_channels
            .iter()
            .find(|(s, _)| *s == source)
            .map(|(_, ch)| *ch)
        else {
            return;
        };
        let Some(registry) = self.probes.as_ref() else {
            return;
        };
        let Ok(mut reg) = registry.try_borrow_mut() else {
            return;
        };
        let now_ps = self.backend.engine().now_ps();
        let _ = reg.sample(now_ps, ch, u64::from(level));
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
            // `probe_bridge_failures`。
            let Ok(mut periph) = handle.try_borrow_mut() else {
                self.probe_bridge_failures += 1;
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
                self.probe_bridge_failures += 1;
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
        // ⚠️ **标签在这一层补**：M-18 的 `ChannelDef` 只有 (source, signal)，
        // 流 id 就是 `"{source}.{signal}"` ⇒ ⇒ 标签默认等于 id。而用户要的是
        // **外设名 + 中断号**（`uart0 · IRQ 10`）⇒ ⇒ 这里按 id 反查板上的
        // 中断表把它写上。**不动契约**（`WaveformStream.label` 本来就是给它
        // 用的那个字段）。
        for stream in wave.streams.iter_mut() {
            if let Some(rest) = stream.id.strip_prefix("irq.dev.") {
                if let Some(irq) = self.peripheral_irqs.iter().find(|i| i.id == rest) {
                    stream.label = irq_device_label(&irq.id, irq.source);
                }
            } else if stream.id == "irq.core.software" {
                stream.label = IRQ_LABEL_SOFTWARE.to_string();
            } else if stream.id == "irq.core.timer" {
                stream.label = IRQ_LABEL_TIMER.to_string();
            } else if stream.id == "irq.core.external" {
                stream.label = IRQ_LABEL_EXTERNAL.to_string();
            }
        }
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
            Payload::Command(cmd) => self.dispatch(*cmd),
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
            CommandPayload::StepOver => self.on_step_over(),
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
            CommandPayload::SetBreakpointEnabled { id, enabled } => {
                // 幂等：id 不存在不是错误；回执带**当前真实表**。
                let _ = self.backend.set_breakpoint_enabled(id, enabled);
                vec![EventPayload::Breakpoints {
                    breakpoints: self.backend.list_breakpoints(),
                }]
            }
            CommandPayload::ListBreakpoints => vec![EventPayload::Breakpoints {
                breakpoints: self.backend.list_breakpoints(),
            }],
            CommandPayload::SetWatchpoint(wp) => match self.backend.set_watchpoint_with_id(wp) {
                Ok(()) => vec![self.tick()],
                Err(err) => vec![self.error_from(&err)],
            },
            CommandPayload::ClearWatchpoint { id } => {
                let _ = self.backend.clear_watchpoint_by_id(id);
                vec![self.tick()]
            }
            CommandPayload::SetWatchExpr(expr) => self.on_set_watch_expr(expr),
            CommandPayload::ClearWatchExpr { id } => {
                self.watch_items.retain(|item| item.expr.id != id);
                self.watch_last.remove(&id);
                vec![self.watch_values_event()]
            }
            CommandPayload::ListWatchExprs => vec![self.watch_values_event()],
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
            // ⚠️ STAGE-43 · v0.66 · D-175 —— **内核侧中间层**的两条**只读**
            // 通道：器件类型注册表 + 逐实例状态。
            //
            // ⚠️ **只读** ⇒ ⇒ 不判错 ⇒ ⇒ 零新场景码。
            CommandPayload::ListDeviceTypes => vec![EventPayload::DeviceTypes {
                types: crate::m15_peripheral::types::device_type_infos(),
            }],
            CommandPayload::ListDeviceInstances => vec![self.device_instances_event()],
            // ⚠️ v0.76 · STAGE-52 · 裁决 M —— **模板库**：清单 + 取完整描述。
            //
            // ⚠️ **前端不抄模板数据** ⇒ ⇒ 内置板清单（名称 / 优先级 / 器件
            // 数）**由内核报** ⇒ ⇒ 前端要哪块再按 `id` 取**完整描述**导入
            // 草稿。
            CommandPayload::ListPlatformTemplates => {
                vec![self.platform_templates_event()]
            }
            CommandPayload::GetPlatformTemplate { id } => {
                match platform::builtin_description(&id) {
                    Ok(description) => vec![EventPayload::PlatformTemplate {
                        description: Box::new(description),
                    }],
                    Err(_) => {
                        // ⚠️ 未知模板 id ⇒ 拒 **E_PLT-22**（与「无板装载」
                        //   同一个码）⇒ ⇒ 引导用户从模板库列出的模板选择。
                        let detail = format!(
                            "没有这块模板：{id} —— 请从模板库列出的模板中选择（default / minimal / qemu-virt）"
                        );
                        vec![self.error_from(&PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_22_NO_BOARD,
                            detail,
                        ))]
                    }
                }
            }
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
            CommandPayload::Admin { op } => self.on_admin(op),
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
                filter,
            } => self.on_export_trace(source, format, offset, limit, filter),
            CommandPayload::SetTraceEnabled { enabled } => self.on_set_trace_enabled(enabled),
            // ⚠️ **契约 v9 的两条新命令**（STAGE-42 · v0.57 批）。
            //
            // ⚠️ **符号查询与整段转储**—— 前端**零** ELF 解析代码 ⇒ ⇒
            // 解析住在内核（M-11），转储走调试视角（M-14 的 `read_only`）。
            CommandPayload::ResolveSymbol { name } => self.on_resolve_symbol(name),
            CommandPayload::DumpMemory { addr, len } => self.on_dump_memory(addr, len),
            // ⚠️ 契约 v5 的 console **输入**方向（STAGE-9 裁决 A）。
            //
            // ⚠️ **不复用 `WriteMemory`**：写 UART 的寄存器地址在技术上
            // 就是「写内存」，但它**绕过外设的语义层**（不走接收 FIFO、
            // 不置 `LSR` 的 `RX_VALID`、不抬中断线）⇒ guest 读 `RHR`
            // 拿到**旧值**而 `LSR` 说「没有数据」⇒ **自相矛盾**。
            CommandPayload::WriteConsole { bytes } => self.on_write_console(bytes),
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
            elf_isa: loaded.elf_isa,
        })
    }

    /// 执行一批指令（`None` = 跑到停止条件，`Some(n)` = 恰好 n 条）。
    fn on_run(&mut self, steps: Option<u32>) -> Vec<EventPayload> {
        self.on_run_inner(steps, false)
    }

    /// **步过**：后端识别调用并记下返回地址停点（不进入断点表），宿主把
    /// 它当作一次「一步」的运行窗口收尾（事件、tick、波形、停下原因同一套）。
    fn on_step_over(&mut self) -> Vec<EventPayload> {
        self.on_run_inner(Some(1), true)
    }

    fn on_run_inner(&mut self, steps: Option<u32>, step_over: bool) -> Vec<EventPayload> {
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
        let run = if step_over {
            self.backend.step_over().map(|_| ())
        } else {
            match steps {
                Some(n) => self.step_in_chunks(n),
                // ⚠️ **`Continue` 分支就是看门狗的接线落点**（规格 §3.3）：
                // 一次 `continue_until_event` = **一个**检查窗口 ⇒ 停滞时
                // **不立即上抛**，而是**记一次窗口** ⇒ 连续三个窗口无进展才触发
                // 处置（裁决 G）。
                None => self.run_continue_windows(),
            }
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
        // M-10 的增量状态：只发订阅了 `sim.state.delta` 的客户端。
        if let Some(delta) = self.state_delta_event(false) {
            events.push(delta);
        }
        // 探针波形（D-46）：本轮运行里外设状态变化产生的边沿。
        if let Some(wave) = self.waveform_event(false) {
            events.push(wave);
        }
        let reason = self.backend.last_stop_reason().or_else(|| {
            // `Continue` 跑到预算上限也是一个合法的停止条件（契约里
            // 最接近的取值是 `EndOfExecution`）；`StepN` 跑满 n 条是
            // **正常完成**，不是停止。
            if steps.is_none() && !step_over {
                Some(HaltReason::EndOfExecution)
            } else {
                None
            }
        });
        if let Some(reason) = reason {
            let pc = self.backend.engine().snapshot().pc;
            // 命中日志：**≥1s 限频**（同一次连续运行里同一断点反复停住时，
            // 不让日志环被同一条消息刷满）。
            if reason == HaltReason::Breakpoint {
                self.log_breakpoint_hit(pc);
            }
            events.push(EventPayload::Halted { reason, pc });
        }
        // 停下（或本窗口跑完）之后轮询一次观察表达式：值变化 / 求值期
        // 错误都在同一个回执里带回。设置 / 列表命令也会走同一个方法。
        if !self.watch_items.is_empty() {
            let watch = self.watch_values_event();
            events.push(watch);
        }
        events
    }

    /// 设置 / 覆盖一条观察表达式。**语法非法 ⇒ 表不变**。
    fn on_set_watch_expr(&mut self, expr: WatchExpr) -> Vec<EventPayload> {
        let target = match WatchTarget::parse(&expr.expr) {
            Ok(target) => target,
            Err(err) => {
                return vec![self.error_from(&PlatformError::new(
                    DEBUG_FAMILY,
                    E_DBG_02_CONDITION_INVALID,
                    err.message(),
                ))];
            }
        };
        if let Some(item) = self
            .watch_items
            .iter_mut()
            .find(|item| item.expr.id == expr.id)
        {
            item.expr = expr;
            item.target = target;
        } else {
            self.watch_items.push(WatchEntry { expr, target });
        }
        vec![self.watch_values_event()]
    }

    /// 对全部观察表达式求值一次，并计算 `changed`。
    ///
    /// 内存表达式走 M-17 的 `read_memory`（最终到 M-14 总线）；该地址当前
    /// 未映射 ⇒ **该项**带 `error`，其他项照常求值（不让一条坏观察项把整
    /// 张表打掉）。
    fn watch_values_event(&mut self) -> EventPayload {
        let state = match self.backend.snapshot() {
            Ok(state) => state,
            Err(err) => return self.error_from(&err),
        };
        let mut values = Vec::with_capacity(self.watch_items.len());
        let mut next = BTreeMap::new();
        for item in &self.watch_items {
            let id = item.expr.id;
            let (value, error) = match item.target.value(&state) {
                Some(v) => (Some(v), None),
                None => match item.target.address(&state) {
                    Some(addr) => match self.backend.read_memory(addr, WriteSize::Word) {
                        Ok(v) => (Some(v), None),
                        Err(err) => (None, Some(format!("读取 0x{addr:08x} 失败：{err}"))),
                    },
                    None => (None, Some("无法解析出观察地址".to_string())),
                },
            };
            let previous = self.watch_last.get(&id);
            let changed = match previous {
                Some(prev) => prev.value != value || prev.error != error,
                None => true,
            };
            let value_dto = WatchValue {
                id,
                expr: item.expr.expr.clone(),
                value,
                error,
                changed,
            };
            next.insert(id, value_dto.clone());
            values.push(value_dto);
        }
        self.watch_last = next;
        EventPayload::WatchValues { values }
    }

    /// 写一条「命中断点」日志，**≥1s 限频**。
    ///
    /// ⚠️ 被压掉的条数会累计，并在**下一条放行**的日志里如实带出
    /// ⇒ ⇒ 不是静默丢弃。
    fn log_breakpoint_hit(&mut self, pc: u32) {
        const BP_HIT_LOG_THROTTLE_PS: u64 = 1_000_000_000_000; // 1s
        let now_ps = self.backend.engine().now_ps();
        let allow = match self.bp_hit_log_last_ps {
            None => true,
            Some(last) => now_ps.saturating_sub(last) >= BP_HIT_LOG_THROTTLE_PS,
        };
        if !allow {
            self.bp_hit_log_suppressed = self.bp_hit_log_suppressed.saturating_add(1);
            return;
        }
        let suppressed = self.bp_hit_log_suppressed;
        self.bp_hit_log_suppressed = 0;
        self.bp_hit_log_last_ps = Some(now_ps);
        let msg = if suppressed == 0 {
            format!("命中断点：pc=0x{pc:08x}")
        } else {
            format!("命中断点：pc=0x{pc:08x}（限频窗口内另有 {suppressed} 次命中未记日志）")
        };
        let t_host_ms = self.host_now_ms();
        self.log_buffer
            .borrow_mut()
            .log(LogLevel::Info, "breakpoint", &msg, "", now_ps, t_host_ms);
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
        //    ⚠️ **v0.43 批（D-135 · 裁决 C / K）**：**走带时钟可用性的那个**
        //    ⇒ ⇒ `clock_available` 由**宿主**取（**谁持有时钟谁回答**）⇒ ⇒
        //    **宿主时钟读不到时，「时间预算」那一路恒不触发**（**不是**「按 0
        //    算」），而「**引擎停滞**」那一路**照常**。
        let outcome =
            classify_window_with_clock(elapsed_ms, stalled, self.started_at.is_available());
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
    /// 4. 上报终止错误。
    ///
    /// ⚠️ **降级由 M-22 的策略分支触发（宿主不自己调）** —— v0.46 · STAGE-31
    /// 裁决 **G**：① 上报健康信号那一步里，M-22 查 `degrade_policy` ⇒ ⇒
    /// 命中 `Fatal → Terminate` ⇒ ⇒ M-22 **问**宿主装的降级执行器 ⇒ ⇒ 执行器
    /// 表态 `Done` ⇒ ⇒ M-22 置 `terminated`。
    ///
    /// ⚠️ **为什么宿主不再自己调 `degrade`**：「策略分支 ＋ 宿主直做」并存 ⇒ ⇒
    /// **重复降级** ⇒ ⇒ 降级在宿主里**只有一个入口**（M-22 的策略分支）。
    ///
    /// ⚠️ **顺序要点**：降级发生在**幂等标志置位之前** ⇒ ⇒ `report` 里的降级
    /// 闸门（`!watchdog_fired`）才**开着** ⇒ ⇒ 策略分支**能**生效。
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
        //    ⚠️ **降级就在这一步发生**（M-22 查策略 ⇒ 问执行器）⇒ ⇒ 宿主
        //    **不再**自己调 `degrade`（裁决 G）。
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
        // ④ 上报终止错误（由**宿主**上报 —— 见上面的场景码分工）。
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

    /// 取一次指，拿机器码（规格 §9.4：M-18 的事件不带数据）。
    ///
    /// ⚠️ **先查 M-11 的反汇编缓存**（规格 §9.4 的「M-11 的 `disasmCache`
    /// 可以兜住它」）：缓存里有 ⇒ **不用**再取一次指 ⇒ 少一次总线读。
    ///
    /// ⚠️ 缓存里的 `code` 是**原码**（压缩指令 = 16 位原码），与下面现场
    /// 取指的口径**一致** ⇒ 两条路给出同一个值。
    ///
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
        let want_state_delta = topic == TOPIC_STATE_DELTA;
        let want_waveform = topic == TOPIC_WAVEFORM;
        let want_console = topic == TOPIC_CONSOLE;
        let want_memory_map = topic == TOPIC_MEMORY_MAP;
        let want_device_types = topic == TOPIC_DEVICE_TYPES;
        let want_device_instances = topic == TOPIC_DEVICE_INSTANCES;
        let want_platform_templates = topic == TOPIC_PLATFORM_TEMPLATES;
        self.topics.insert(topic);
        let mut events = vec![self.tick()];
        if want_registers {
            // The snapshot **is** the response — the caller asked for the
            // current register file.
            if let Some(wave) = self.register_waveform(true) {
                events.push(wave);
            }
        }
        if want_state_delta {
            // 订阅先给全量，之后每次运行窗口只给增量。
            if let Some(delta) = self.state_delta_event(true) {
                events.push(delta);
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
        // ⚠️ STAGE-43 · v0.66 · D-175 —— 「订阅即强制推一条」**同一个套路**
        // ⇒ ⇒ 前端不必等下一次建图才拿到实例状态。
        if want_device_types {
            // ⚠️ **类型注册表是静态的** ⇒ ⇒ 推一次就够（但**每次订阅**都推
            // ⇒ ⇒ 重订后前端手里那份**一定**是当前内核的）。
            events.push(EventPayload::DeviceTypes {
                types: crate::m15_peripheral::types::device_type_infos(),
            });
        }
        if want_device_instances {
            events.push(self.device_instances_event());
        }
        // ⚠️ v0.76 · STAGE-52 · 裁决 M —— 模板库：**订阅即推一条清单**
        //（与 `device.types` 同一个套路）⇒ ⇒ 前端**不必**等下一次改板
        // 才拿到模板清单。
        if want_platform_templates {
            events.push(self.platform_templates_event());
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

    /// 状态增量（`SimStateDelta`）。
    ///
    /// - `all = true` → **全量**：32 个 GPR + PC。
    /// - `all = false` → **增量**：只带值变过的寄存器；PC 变了才带 `Some`。
    ///
    /// 没有订阅 `sim.state.delta` 时返回 `None`（不发事件）。
    fn state_delta_event(&mut self, all: bool) -> Option<EventPayload> {
        if !self.topics.contains(TOPIC_STATE_DELTA) {
            return None;
        }
        let state = self.backend.engine().snapshot();
        let now_ps = state.virtual_time_ps;
        let mut registers: Vec<DeltaRegister> = Vec::new();
        for i in 0..32usize {
            let changed =
                !self.delta_published_valid || self.delta_published_regs[i] != state.gpr[i];
            if all || changed {
                registers.push(DeltaRegister {
                    index: i as u32,
                    value: state.gpr[i],
                });
            }
            self.delta_published_regs[i] = state.gpr[i];
        }
        let pc_changed = !self.delta_published_valid || self.delta_published_pc != state.pc;
        self.delta_published_pc = state.pc;
        self.delta_published_valid = true;
        if registers.is_empty() && !pc_changed {
            return None;
        }
        Some(EventPayload::SimStateDelta {
            registers,
            pc: if all || pc_changed {
                Some(state.pc)
            } else {
                None
            },
            t_ps: now_ps,
        })
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

    // -- 会话级快照（STAGE-3）------------------------------------------------
    //
    // 下面这一组是**宿主调试 / 测试读面**：它们把会话的当前状态原样
    // 摊出来，供端到端用例逐项比对「恢复前」与「恢复后」。
    //
    // ⚠️ 它们**不是契约层**的东西，也不进 `SCHEMA_VERSION`——契约层冻结
    // 的是**线上**的 DTO，这里是**进程内**的观察窗口。
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

/// 管理员通道（v0.44 · 台账 **D-129** · 冻结规格 STAGE-29 §3.1 ~ §3.3）。
///
/// ⚠️ 覆盖验收 **B-1 ~ B-6** 与边界 **X-3 / X-4** ⇒ ⇒ 默认关时回应**逐字
/// 不变**，开启后**恰好一个**回复事件，未实现的 op **不假装实现了**。
#[cfg(test)]
mod tests_admin_channel;

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

/// 事件主题名常量（与前端 `EVENT_TOPIC` 逐字对齐）。
pub mod topics;

/// 兼容档平台常量（GPIO / 默认内存图 / QEMU virt 地址、器件 id、波特率）。
pub mod platform_consts;

/// 镜像解析产物与调试面纯函数映射（不访问宿主私有字段）。
mod image;

/// 会话级快照采集 / 还原与段 5 中断状态导出 / 回灌。
mod session;

/// console / UART 方向：TX 捕获环取字节与 RX FIFO 注入。
mod console;

/// 中断控制器（M-16）通路：端口装配、源同步、IRQ / 定时器包装与状态只读。
mod intc;

/// 引擎 / 总线状态只读面：指令计数、总线句柄、虚拟时间、PC / 寄存器 / 内存读 / 引擎快照。
mod state;

/// 可观测只读面：M-22 健康 / 降级计数与动作、M-21 日志环快照。
mod diagnostics;

/// 调试面可观测计数：波形管道、探针边沿 / trace 环、符号表 / DWARF / 反汇编缓存、执行轨迹环。
mod inspect;

/// 调试面事件：反汇编 / 行表 / 符号解析 / 内存转储 / 导出，trace 环转移 / 丢弃 / drain。
mod debug;

/// 工程 / 板级内存图族：装载卸载、整份 / 差量换图、修订号检查与图 / 模板 / 实例事件。
mod project;

#[cfg(test)]
mod tests_watchdog;

/// 宿主侧**降级执行器**（v0.46 批 · STAGE-31 · 台账 **D-130** · 裁决 **J / M**）。
///
/// ⚠️ 它**不是**公开的：它是宿主的**内部装配件**（宿主把 M-18 / M-19 的句柄
/// 交给它 ⇒ ⇒ 它与宿主**同一份**）⇒ ⇒ 外部**不需要**看见它。
mod degrade;

/// 宿主层**降级**的判别力用例（v0.46 批 · STAGE-31 · 验收 **C-1 ~ C-6**）。
///
/// ⚠️ 它盯的是「降级**走派发路径**」这件事：M-22 **问**执行器（而不是自己
/// 硬编码）⇒ ⇒ 执行器**够得到**的那一档（`TraceDown`）**真的**动了 M-18 的
/// 采样与 M-19 的溢出策略。
#[cfg(test)]
mod tests_degrade;

/// v0.57 批（STAGE-42）的**内核侧**判别力用例 —— 验收 **B-1 ~ B-15**。
///
/// ⚠️ 它盯的是「宿主这条路**真的接上了**」⇒ ⇒ 符号查询、整段转储、条件
/// 断点三件事**各**有宿主层的用例（不是只有单元层的）。
#[cfg(test)]
mod tests_v057_debug;

/// 复位语义（v0.59 批 · 用户报的现象「复位后无法再运行」）—— **装载新工程 =
/// 新会话 ⇒ 内核可复活**。
///
/// ⚠️ 它钉的是**第三段**（内核侧）：**看门狗终止态跨 `LoadProject` 存活**⇒ ⇒
/// 复位救不回一个被看门狗杀死的会话 ⇒ ⇒ 「复位回到初始态」（FR-06 验收）
/// **做不到**。
///
/// ⚠️ 前两段（前端）落在 `front/tests/reset-semantics.test.mjs` 与真机门禁的
/// `loadImage` 段 ⇒ ⇒ 三段**分层**钉。
#[cfg(test)]
mod tests_v059_reset;

/// STAGE-43 · v0.66 批 —— **内核侧中间层**的宿主侧门禁（K 组）。
///
/// ⚠️ 盯的是「内核怎么算」：器件类型注册表的内容 + 逐实例状态的**推导**
/// （已绑定 / 待生效）⇒ ⇒ 断言全部落在**命令回的事件**上。
#[cfg(test)]
mod tests_v066_devices;
#[cfg(test)]
mod tests_v071_unknown_system;

/// **描述格式升版**（v0.73 · STAGE-49 · D-175 的 B2 / G / D1 + D-177 的 C）。
///
/// ⚠️ 本批**施工中** ⇒ ⇒ 这一组先盯**G**（`timer_riscv` 进描述格式）⇒ ⇒
/// 其余三项（B2 / D1 / D-177 的 C）落地后再往这里加。
#[cfg(test)]
mod tests_v073_schema;

/// **v0.75（STAGE-51 · D-175 的 F）** —— MCU 概念：ISA 门禁的 K 组 / A-3 / X-1。
#[cfg(test)]
mod tests_v075_mcu;
#[cfg(test)]
mod tests_v076_templates;

/// **B2**（v0.73 · STAGE-49 · D-175 的 B2）—— `base` 可缺省 + **装配期分配**。
///
/// ⚠️ **分配器在内核侧**（[`platform::assign_bases`]）—— 契约层**看不到**外设的
///   **窗口大小**（那是 **M-15 注册表**的事 · 裁决 D）⇒ ⇒ ⇒ 「分配 + 判重叠」
///   **必须**在**同一处**（一个约束一个落点）。
#[cfg(test)]
mod tests_v073_b2;

/// 平台描述的加载 · 器件工厂 · 建图（v0.27 · D-123-B）。
///
/// ⚠️ **它是公开的**：外部（headless runner / 将来的 server crate）可能
/// 要**按 id 取一份描述**或**从文件读一份描述** ⇒ 入口在
/// [`KernelHost::new_with_platform`] / [`KernelHost::new_with_platform_file`]，
/// 描述本身在 [`platform`]。
pub mod platform;
