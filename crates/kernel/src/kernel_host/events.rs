//! 订阅与事件构造族：主题订阅响应，以及 Tick / 寄存器波形 / 状态增量 /
//! 探针波形事件的构造。
//!
//! ⚠️ 本模块是 [`super::KernelHost`] 的后代子模块 ⇒ ⇒ 可访问宿主私有字段；
//! 方法仍挂在 `impl KernelHost` 上，对外调用点一字不变。被兄弟子模块
//! （`project.rs`）回调的 `tick` / `register_waveform` 提为 `pub(crate)`；
//! 仅被父模块调度与同族内部调用的方法保持 `pub(super)`。

use super::{
    irq_device_label, KernelHost, IRQ_LABEL_EXTERNAL, IRQ_LABEL_SOFTWARE, IRQ_LABEL_TIMER,
    MAX_WAVEFORM_EDGES_PER_EVENT, TOPIC_CONSOLE, TOPIC_DEVICE_INSTANCES, TOPIC_DEVICE_TYPES,
    TOPIC_MEMORY_MAP, TOPIC_PLATFORM_TEMPLATES, TOPIC_REGISTERS, TOPIC_STATE_DELTA, TOPIC_WAVEFORM,
};
use riscv_sim_contract::model::{DeltaRegister, Edge, WaveformHistory, WaveformStream};
use riscv_sim_contract::protocol::EventPayload;

/// 波形（寄存器流）的 schema 版本。
const WAVEFORM_SCHEMA_VERSION: u32 = 1;

impl KernelHost {
    // -- 订阅响应 -----------------------------------------------------------

    /// 订阅一个主题。
    pub(super) fn on_subscribe(&mut self, topic: String) -> Vec<EventPayload> {
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
    pub(crate) fn tick(&self) -> EventPayload {
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
    pub(crate) fn register_waveform(&mut self, all: bool) -> Option<EventPayload> {
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
    pub(super) fn state_delta_event(&mut self, all: bool) -> Option<EventPayload> {
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

    /// 探针波形（M-18 边沿存储）。
    pub(super) fn waveform_event(&mut self, force: bool) -> Option<EventPayload> {
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
}
