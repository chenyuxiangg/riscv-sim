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
//! - **M-12 的调度循环没被启用**：本阶段没有外设 / 定时器，事件队列恒空，
//!   所以指令执行直接由 M-17 驱动 M-13，M-12 承担的是**平台状态与虚拟
//!   时钟的唯一持有者**。接线外设时再把调度循环打开。
//! - **没有探针管道**：M-18 / M-19 没有接到总线上，所以 `sim.waveform`
//!   订阅下**不产生**探针流。波形视图在第 1 阶段是空的——这是已知限制，
//!   不是假数据。

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::{
    Edge, ImageKind, ImageRef, ImageSource, Project, Waveform, WaveformStream, WriteSize,
};
use riscv_sim_contract::port::{EventBus, ExecutionBackend, InterruptSink};
use riscv_sim_contract::protocol::{
    CommandPayload, Direction, EventPayload, HaltReason, Message, Payload,
};
use riscv_sim_contract::SCHEMA_VERSION;

use crate::m06_elf::{DefaultLoader, Kind as LoadKind, ParseLimits};
use crate::m12_engine::DEFAULT_INSTRUCTION_BUDGET;
use crate::m17_backend::SelfIssBackend;

/// 订阅主题：寄存器快照。
///
/// 契约里**没有**寄存器快照事件（D-32 缺口），所以寄存器值只能走
/// `EventPayload::WaveformDelta` 的边沿编码流——这是契约内**唯一**的
/// 载体。流 id 与 mock 内核逐字一致，前端视图零改动。
pub const TOPIC_REGISTERS: &str = "sim.registers";

/// 订阅主题：探针波形。
pub const TOPIC_WAVEFORM: &str = "sim.waveform";

/// 默认内存图 · 低段 RAM 的基址（复位向量）。
pub const DEFAULT_RAM_LOW_BASE: u32 = 0x0000_0000;

/// 默认内存图 · 低段 RAM 的大小（1 MiB）。
pub const DEFAULT_RAM_LOW_SIZE: u32 = 0x0010_0000;

/// 默认内存图 · 高段 RAM 的基址（常规 RISC-V 内存，与 golden 对拍侧一致）。
pub const DEFAULT_RAM_HIGH_BASE: u32 = 0x8000_0000;

/// 默认内存图 · 高段 RAM 的大小（4 MiB）。
pub const DEFAULT_RAM_HIGH_SIZE: u32 = 0x0040_0000;

/// 波形（寄存器流）的 schema 版本。
const WAVEFORM_SCHEMA_VERSION: u32 = 1;

/// 引擎事件汇（事件总线 + 中断汇）。
///
/// 引擎在调度循环里发布的 `Tick` 会被收集到这里。共享句柄让宿主能取走
/// 它们——第 1 阶段队列恒空，所以取走的是空列表，但接线外设之后这条路
/// 已经在了。
#[derive(Debug, Default, Clone)]
struct HostSinks {
    events: Rc<RefCell<Vec<EventPayload>>>,
}

impl EventBus for HostSinks {
    fn publish(&mut self, evt: EventPayload) -> Result<(), PlatformError> {
        self.events.borrow_mut().push(evt);
        Ok(())
    }
}

impl InterruptSink for HostSinks {
    fn raise(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
    }
    fn lower(&mut self, _irq: u16) -> Result<(), PlatformError> {
        Ok(())
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
    pub fn new() -> Self {
        let sinks = HostSinks::default();
        let mut host = Self {
            backend: SelfIssBackend::new(sinks.clone(), sinks.clone()),
            sinks,
            topics: BTreeSet::new(),
            seq: 0,
            snapshot_counter: 0,
            published_regs: [0; 32],
            published_pc: 0,
            published_valid: false,
        };
        // The default memory map cannot fail on a fresh bus (the two
        // regions do not overlap and are far below the 16 MiB per-region
        // cap). If it ever does, the failure is reported by the next
        // `LoadProject` — which re-maps the memory anyway.
        let _ = host.map_default_memory();
        host
    }

    /// 建立默认内存图（低段 1 MiB + 高段 4 MiB RAM）。
    fn map_default_memory(&mut self) -> Result<(), PlatformError> {
        let bus = self.backend.bus_mut();
        bus.map_ram(DEFAULT_RAM_LOW_BASE, DEFAULT_RAM_LOW_SIZE, &[])?;
        bus.map_ram(DEFAULT_RAM_HIGH_BASE, DEFAULT_RAM_HIGH_SIZE, &[])?;
        Ok(())
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
        if msg.schema_version < SCHEMA_VERSION {
            return vec![EventPayload::Error {
                code: "E_PROTO-01".to_string(),
                message: format!(
                    "协议版本不兼容：{} < {}",
                    msg.schema_version, SCHEMA_VERSION
                ),
            }];
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
            CommandPayload::WriteMemory { addr, size, value } => {
                match self.backend.write_memory(addr, size, value) {
                    Ok(()) => vec![self.tick()],
                    Err(err) => vec![self.error_from(&err)],
                }
            }
            CommandPayload::Subscribe { topic } => self.on_subscribe(topic),
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
            CommandPayload::Restore { state } => match self.backend.restore(&state) {
                Ok(()) => vec![self.tick()],
                Err(err) => vec![self.error_from(&err)],
            },
            CommandPayload::Admin { .. } => vec![EventPayload::Error {
                code: "E_CTX-05".to_string(),
                message: "管理通道未启用（admin op 不在当前会话的能力面内）".to_string(),
            }],
        }
    }

    /// 装载工程：复位 → 重建内存图 → 加载镜像 → 换新会话（清订阅）。
    fn on_load_project(&mut self, project: Project) -> Vec<EventPayload> {
        let _ = self.backend.reset();
        self.backend.clear_debug_state();
        self.backend.reset_memory_map();
        if let Err(err) = self.map_default_memory() {
            return vec![self.error_from(&err)];
        }

        let mut entry = 0u32;
        for image in &project.images {
            match self.load_image(image) {
                Ok(image_entry) => {
                    // A raw blob has entry 0; it must not clobber an ELF's
                    // entry point.
                    if image_entry != 0 {
                        entry = image_entry;
                    }
                }
                Err(err) => return vec![self.error_from(&err)],
            }
        }
        self.backend.set_pc(entry);

        // A new project is a new session: the old topic subscriptions die
        // with it (FR-13 — every session id owns its namespace). The host
        // has to subscribe again after a `LoadProject`.
        self.topics.clear();
        // The next register push must be a **full** snapshot.
        self.published_valid = false;

        let mut events = vec![self.tick()];
        if let Some(wave) = self.register_waveform(true) {
            events.push(wave);
        }
        events
    }

    /// 加载一个镜像，返回它的入口地址。
    fn load_image(&mut self, image: &ImageRef) -> Result<u32, PlatformError> {
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
        for (paddr, segment) in &loaded.segments {
            self.backend.bus_mut().load_bytes(*paddr, segment)?;
        }
        Ok(loaded.entry)
    }

    /// 执行一批指令（`None` = 跑到停止条件，`Some(n)` = 恰好 n 条）。
    fn on_run(&mut self, steps: Option<u32>) -> Vec<EventPayload> {
        let budget = steps.unwrap_or(DEFAULT_INSTRUCTION_BUDGET);
        let mut events = Vec::new();
        match self.backend.step_n(budget) {
            Ok(_) => {}
            Err(err) => {
                // The error comes **first**: it carries the command's
                // `reqId`, so M-05 rejects the command instead of letting
                // it time out. The state-reflecting events follow as
                // pushes.
                events.push(self.error_from(&err));
            }
        }
        events.push(self.tick());
        if let Some(wave) = self.register_waveform(false) {
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

    /// 订阅一个主题。
    fn on_subscribe(&mut self, topic: String) -> Vec<EventPayload> {
        let want_registers = topic == TOPIC_REGISTERS;
        let want_waveform = topic == TOPIC_WAVEFORM;
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
            // 第 1 阶段没有真实探针管道（M-18/M-19 未接总线），因此这里
            // **不产生**探针流。波形视图在第 1 阶段是空的——已知限制，
            // 不是假数据。
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
        Some(EventPayload::WaveformDelta(Waveform {
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

    /// 当前 PC。
    pub fn pc(&self) -> u32 {
        self.backend.engine().pc()
    }

    /// 读一个内存位置（不计入总线统计）。
    pub fn read_memory(&self, addr: u32, size: WriteSize) -> Result<u32, PlatformError> {
        self.backend.bus().read_only(addr, size.into())
    }
}

#[cfg(test)]
mod tests;
