//! 宿主事件的订阅 / 回复主题名。
//!
//! ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC` 逐字相同**
//! ——Rust 侧没有集中的「变体 → topic」映射表（唯一真相在前端），两侧的
//! 一致性靠「同一个字面量 + 各自被测试钉住」保证。
//!
//! ⚠️ 契约 v3 的四个调试面主题是**命令的回复**（STAGE-5 §9.1），前端靠传输
//! 帧的 `reqId` 关联 ⇒ **不需要先订阅**。不要据此实现「订阅后才推送」。

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

/// 订阅主题：状态增量（`EventPayload::SimStateDelta`）。
///
/// M-10 的增量订阅落点。与 `TOPIC_REGISTERS` 的波形兼容流并存：
/// 生产路径用这条自定义事件，旧路径保留给尚未改接的客户端。
pub const TOPIC_STATE_DELTA: &str = "sim.state.delta";

/// 订阅主题：探针波形。
pub const TOPIC_WAVEFORM: &str = "sim.waveform";

/// 主题：反汇编序列（`EventPayload::Disasm` 的回复）。
pub const TOPIC_DISASM: &str = "debug.disasm";

/// 主题：地址 → 源码行（`EventPayload::SourceLocation` 的回复）。
pub const TOPIC_SOURCE: &str = "debug.source";

/// 主题：导出块（`EventPayload::TraceExport` 的回复）。
pub const TOPIC_EXPORT: &str = "debug.export";

/// 主题：轨迹记录状态（`EventPayload::TraceState` 的回复）。
pub const TOPIC_TRACE: &str = "debug.trace";

/// 主题：符号查询的回复（`EventPayload::SymbolResolved` 的回复）。
///
/// ⚠️ **登记用，不是订阅开关**（与上面四个常量同口径）⇒ ⇒ 前端靠传输帧的
/// `reqId` 关联「命令 → 回复」⇒ ⇒ **不需要先订阅**。
///
/// ⚠️ 与前端 `EVENT_TOPIC` 里的 `SymbolResolved` **逐字相同**（STAGE-42
/// §2.7）⇒ ⇒ 两侧的一致性由 `crates/kernel/tests/m11_host_bridge.rs` 里
/// 那条用例钉住。
pub const TOPIC_SYMBOL: &str = "debug.symbol";

/// 主题：整段转储的回复（`EventPayload::MemoryDump` 的回复）。
///
/// ⚠️ **登记用，不是订阅开关**（与上面四个常量同口径）。
pub const TOPIC_DUMP: &str = "debug.dump";

/// console 输出的 topic（与前端 `EVENT_TOPIC.Console` **逐字一致**）。
pub const TOPIC_CONSOLE: &str = "sim.console";

/// 主题：板级内存图（`EventPayload::MemoryMap`）。
///
/// ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC.MemoryMap`
/// 逐字相同**（= `"memory.map"`）⇒ 前端订阅它才收得到那张图。
///
/// ⚠️ 它归 **`memory.*` 族**（与 `memory.read` 同族）—— 这是事件主题族
/// 的既有命名法（STAGE-14 §1.8），不是 `sim.*`。
pub const TOPIC_MEMORY_MAP: &str = "memory.map";

/// 主题：器件类型注册表（`EventPayload::DeviceTypes` · STAGE-43 · v0.66）。
///
/// ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC.DeviceTypes`
/// 逐字相同**（= `"device.types"`）⇒ 前端订阅它才收得到那张表。
///
/// ⚠️ 它归 **`device.*` 族** —— 这是**器件**这一族自己的命名法，与
/// `sim.*` / `memory.*` / `debug.*` **平级**（不是它们的一部分）。
pub const TOPIC_DEVICE_TYPES: &str = "device.types";

/// 主题：内置板模板清单（`EventPayload::PlatformTemplates` · STAGE-52 ·
/// v0.76 · 裁决 M）。
///
/// ⚠️ **值必须与前端 `front/lib/message.js` 的 `EVENT_TOPIC.PlatformTemplates`
/// 逐字相同**（= `"platform.templates"`）。
///
/// ⚠️ **归 `platform.*` 族** —— 平台级（内置板模板）自己的命名法。
pub const TOPIC_PLATFORM_TEMPLATES: &str = "platform.templates";

/// 主题：逐实例状态（`EventPayload::DeviceInstances` · STAGE-43 · v0.66）。
///
/// ⚠️ **值必须与前端 `front/lib/message.js` 的
/// `EVENT_TOPIC.DeviceInstances` 逐字相同**（= `"device.instances"`）。
pub const TOPIC_DEVICE_INSTANCES: &str = "device.instances";
