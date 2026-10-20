//! M-12 ↔ 契约层快照 DTO 的双向转换。
//!
//! M-12 内部有两套快照形态：
//!
//! - [`SimState`] —— 契约层的**部分**快照：平台状态 + 虚拟时钟 + 插入序
//!   计数器，**装不下事件队列**。
//! - [`EngineSnapshot`] —— 引擎的**完整**快照：额外携带待处理事件队列与
//!   队列顺序，因此可以逐位还原引擎（D-05）。
//!
//! 但 [`EngineSnapshot`] **不可序列化** —— kernel crate 刻意不依赖 serde，
//! 契约层也放不下 kernel 的内部类型。这正是 D-24 的根因：确定性的快照
//! 存在，却**落不了盘**。
//!
//! 本模块补上第三块拼图：契约层的可序列化镜像
//! [`EngineSnapshotDto`]。转换是**一一对应、无损**的，因此
//! `EngineSnapshot → DTO → JSON → DTO → EngineSnapshot` 的往返不会丢
//! 任何东西。
//!
//! 方向：
//!
//! ```text
//! EngineSnapshot --From--> EngineSnapshotDto --serde--> JSON
//! EngineSnapshotDto --From--> EngineSnapshot
//! ```
//!
//! 落盘方（M-20 / M-23）只需要认识 DTO，不需要认识 M-12 的内部类型 —
//! 这正是「契约先行」要的效果。

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::port::{EventBus, InterruptSink};
use riscv_sim_contract::snapshot::{
    EngineSnapshotDto, QueuedEventDto, SchedEventDto, SNAPSHOT_SCHEMA_VERSION,
};

use super::queue::QueuedEvent;
use super::{Engine, EngineSnapshot, SchedEvent};

// ---------------------------------------------------------------------------
// 事件载荷
// ---------------------------------------------------------------------------

impl From<&SchedEvent> for SchedEventDto {
    fn from(event: &SchedEvent) -> Self {
        match event {
            SchedEvent::PeripheralTick { paddr, at_ps } => SchedEventDto::PeripheralTick {
                paddr: *paddr,
                at_ps: *at_ps,
            },
            SchedEvent::TimerExpiry { irq, at_ps } => SchedEventDto::TimerExpiry {
                irq: *irq,
                at_ps: *at_ps,
            },
            SchedEvent::Commit(write) => SchedEventDto::Commit(write.clone()),
        }
    }
}

impl From<SchedEventDto> for SchedEvent {
    fn from(dto: SchedEventDto) -> Self {
        match dto {
            SchedEventDto::PeripheralTick { paddr, at_ps } => {
                SchedEvent::PeripheralTick { paddr, at_ps }
            }
            SchedEventDto::TimerExpiry { irq, at_ps } => SchedEvent::TimerExpiry { irq, at_ps },
            SchedEventDto::Commit(write) => SchedEvent::Commit(write),
        }
    }
}

// ---------------------------------------------------------------------------
// 队列条目
// ---------------------------------------------------------------------------

impl From<&QueuedEvent> for QueuedEventDto {
    fn from(event: &QueuedEvent) -> Self {
        QueuedEventDto {
            at_ps: event.at_ps,
            seq: event.seq,
            event: SchedEventDto::from(&event.event),
        }
    }
}

impl From<QueuedEventDto> for QueuedEvent {
    fn from(dto: QueuedEventDto) -> Self {
        QueuedEvent {
            at_ps: dto.at_ps,
            seq: dto.seq,
            event: dto.event.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 完整快照
// ---------------------------------------------------------------------------

impl From<&EngineSnapshot> for EngineSnapshotDto {
    fn from(snapshot: &EngineSnapshot) -> Self {
        EngineSnapshotDto {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            state: snapshot.state.clone(),
            queue: snapshot.queue.iter().map(QueuedEventDto::from).collect(),
            next_seq: snapshot.next_seq,
            instruction_budget: snapshot.instruction_budget,
        }
    }
}

impl From<&EngineSnapshotDto> for EngineSnapshot {
    fn from(dto: &EngineSnapshotDto) -> Self {
        EngineSnapshot {
            state: dto.state.clone(),
            queue: dto.queue.iter().cloned().map(QueuedEvent::from).collect(),
            next_seq: dto.next_seq,
            instruction_budget: dto.instruction_budget,
        }
    }
}

// ---------------------------------------------------------------------------
// 引擎便捷方法
// ---------------------------------------------------------------------------

impl<B: EventBus, I: InterruptSink> Engine<B, I> {
    /// 引擎状态的**可序列化完整快照**。
    ///
    /// 与 [`Engine::snapshot_full`] 等价，只是结果落在契约层的
    /// [`EngineSnapshotDto`] 上 —— 可以直接交给 M-20 落盘 / M-23 存储。
    pub fn snapshot_dto(&self) -> EngineSnapshotDto {
        EngineSnapshotDto::from(&self.snapshot_full())
    }

    /// 由契约层的 [`EngineSnapshotDto`] 还原引擎。
    ///
    /// 与 [`Engine::restore_full`] 等价：队列内容与顺序、插入序计数器、
    /// 虚拟时间、每量子指令预算全部还原，还原后继续推进与「从未还原过」
    /// 的运行产生完全相同的事件序列（D-05 / FR-12）。
    ///
    /// **先校验、后应用**：快照来自不可信存储时，未知 schema 版本、畸形
    /// 事件队列、插入序计数器不一致一律返回错误，且**引擎保持原样**
    /// （FR-12）。校验通过才落到引擎上。
    pub fn restore_dto(&mut self, dto: &EngineSnapshotDto) -> Result<(), PlatformError> {
        dto.validate()?;
        self.restore_full(&EngineSnapshot::from(dto));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::{PendingWrite, SimState, WriteSize};

    #[test]
    fn sched_event_round_trips_through_the_dto() {
        let events = [
            SchedEvent::PeripheralTick {
                paddr: 0x4000_0000,
                at_ps: 1_500,
            },
            SchedEvent::TimerExpiry {
                irq: 5,
                at_ps: 2_500,
            },
            SchedEvent::Commit(PendingWrite {
                paddr: 0x2000_0000,
                size: WriteSize::Word,
                value: 0x1234_5678,
                source: "UART0".to_string(),
            }),
        ];
        for event in events {
            let dto = SchedEventDto::from(&event);
            let back = SchedEvent::from(dto);
            assert_eq!(event, back);
        }
    }

    #[test]
    fn engine_snapshot_round_trips_through_the_dto() {
        let snapshot = EngineSnapshot {
            state: SimState {
                virtual_time_ps: 9_000,
                pc: 0x8000_0010,
                event_queue_head: 4,
                ..SimState::default()
            },
            queue: vec![
                QueuedEvent {
                    at_ps: 10_000,
                    seq: 0,
                    event: SchedEvent::PeripheralTick {
                        paddr: 0x4000_0000,
                        at_ps: 10_000,
                    },
                },
                QueuedEvent {
                    at_ps: 11_000,
                    seq: 1,
                    event: SchedEvent::TimerExpiry {
                        irq: 3,
                        at_ps: 11_000,
                    },
                },
            ],
            next_seq: 4,
            instruction_budget: 777,
        };
        let dto = EngineSnapshotDto::from(&snapshot);
        assert_eq!(dto.schema_version, SNAPSHOT_SCHEMA_VERSION);
        let back = EngineSnapshot::from(&dto);
        assert_eq!(snapshot, back);
    }
}
