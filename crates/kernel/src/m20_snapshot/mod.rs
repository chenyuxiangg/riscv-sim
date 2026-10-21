//! M-20 快照与确定性事件流记录 (Snapshot & Deterministic Event Flow).
//!
//! 拥有**会话级快照**的组装与内存记录，落盘委托给 M-23（DataStore）。
//! 冻结规格 = `docs/detailed-design/STAGE-3_Session_Snapshot.md`；
//! 载荷形状由契约层 [`riscv_sim_contract::session`] 定义（**已冻结，本模块
//! 不得改它的形状**）。
//!
//! # 本模块解决什么（台账 D-37）
//!
//! 旧版的 `SnapshotRecord` 吃的是 **partial 的 `SimState`**——它只有寄存器 /
//! PC / CSR / 虚拟时间，**没有**事件队列、没有内存页、没有内存映射、没有外设
//! 内部状态、没有已拉起的 IRQ 行。于是「落盘一个快照」还原不出一个会话，
//! 而**两个落盘格式并存**（旧的 partial 与新的完整）等于「哪个是真的说不清」。
//!
//! 本模块把形状换成**唯一**的一种：
//!
//! ```text
//! SnapshotRecord { id, cp_meta, payload: SessionSnapshotDto, label }
//! ```
//!
//! 其中 `payload` 是契约层的 [`SessionSnapshotDto`]——**六段齐全**（段 0 检查点
//! 元数据 / 段 1 引擎 / 段 2 内存页 / 段 3 内存映射 / 段 4 外设 / 段 5 INTC）。
//!
//! # M-20 是**记录器**，不是**采集器**
//!
//! ⚠️ **六段由调用方提供**（内核宿主持有总线 / 外设 / 中断控制器，由它去调
//! M-14 的 `export_pages` / `export_map`、M-15 的 `export_state`、M-16 的
//! `export_state`）。这样 M-20 **不依赖** M-14 / M-15 / M-16——依赖方向单向，
//! M-20 只认契约层的 DTO。
//!
//! > 与冻结规格 §3.5 的**差异**（本实现为准）：规格里 `capture_session` 直接收
//! > `&Bus` / `&PeripheralRegistry` / `&InterruptController`；本实现收**已经导出
//! > 好的 DTO**。两者产出**同一种** `SessionSnapshotDto`，差别只在**谁去采集**。
//!
//! # 虚拟时间的唯一落点
//!
//! ⚠️ 旧结构里的 `at_ps: u64` 字段**消失了**：虚拟时间由 `cp_meta.t_virt` 承担
//! （契约里已是**十进制字符串**形态，见契约 v2 §2 数值策略）。同一个量**只有
//! 一个落点**——两处各存一份就要在恢复时挑一个赢家，输家默默腐烂。
//!
//! # 恢复路径
//!
//! [`SnapshotManager::restore_session`] **只做校验**，不把状态灌回各模块——
//! 灌回去是内核宿主的事（它同时持有总线 / 外设 / INTC）。⚠️ **校验必须发生在
//! 任何状态被写入之前**：半个应用完的快照比被拒绝的快照更糟（它留下的会话状态
//! 没有任何检查点描述过，下游也分辨不出来）。

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::session::{
    CheckpointMetaDto, M13StateDto, M14MapDto, M15PeriphDto, M16IntcDto, SessionSnapshotDto,
    SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use riscv_sim_contract::snapshot::EngineSnapshotDto;

/// A recorded snapshot, plus the metadata needed to reload it.
///
/// ⚠️ **形状已换**（台账 D-37）：本结构**不再**直接持有 partial 的 `SimState`，
/// 也不再有裸的 `at_ps: u64`。载荷的唯一载体是契约层的
/// [`SessionSnapshotDto`]（六段齐全）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    /// Stable id (monotonic).
    pub id: u64,
    /// 检查点元数据（段 0）。
    ///
    /// ⚠️ 它是 [`Self::payload`] 里 `cp_meta` 的**副本**，不是第二份真相——
    /// 两者永远相等（`record()` 从同一处取值）。存在的理由是「列表视图不必
    /// 展开整个载荷就能显示时间戳与检查点号」。
    pub cp_meta: CheckpointMetaDto,
    /// The session payload itself — the **only** carrier of the state.
    pub payload: SessionSnapshotDto,
    /// Free-form human label.
    pub label: String,
}

impl Default for SnapshotRecord {
    fn default() -> Self {
        Self {
            id: 0,
            cp_meta: CheckpointMetaDto::default(),
            payload: SessionSnapshotDto::default(),
            label: String::new(),
        }
    }
}

/// Snapshot manager. Holds recent snapshots in memory; the actual
/// persistence is delegated to M-23 (DataStore).
#[derive(Debug, Default)]
pub struct SnapshotManager {
    next_id: u64,
    records: Vec<SnapshotRecord>,
}

impl SnapshotManager {
    /// New empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// 组装**完整**会话载荷（六段全齐）。
    ///
    /// 六段**由调用方提供**——M-20 是**记录器**不是**采集器**：内核宿主同时
    /// 持有总线 / 外设 / 中断控制器，由它去调 M-14 / M-15 / M-16 的导出接口，
    /// M-20 因此**不依赖**这三个模块。
    ///
    /// * `cp_meta` **原样**放进载荷（段 0）。⚠️ 调用方要自己保证
    ///   `cp_meta.version == SESSION_SNAPSHOT_SCHEMA_VERSION`——本方法**不**改写
    ///   它（改写就是「悄悄替你造数据」），版本不对时 [`Self::restore_session`]
    ///   会拒。
    /// * 载荷的 `schema_version` 恒为 [`SESSION_SNAPSHOT_SCHEMA_VERSION`]。
    pub fn capture_session(
        &mut self,
        cp_meta: CheckpointMetaDto,
        engine: EngineSnapshotDto,
        m13_state: M13StateDto,
        m14_map: M14MapDto,
        m15_periph: M15PeriphDto,
        m16_intc: M16IntcDto,
    ) -> SessionSnapshotDto {
        SessionSnapshotDto {
            schema_version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            cp_meta,
            m12_engine: engine,
            m13_state,
            m14_map,
            m15_periph,
            m16_intc,
        }
    }

    /// 记一条快照进内存列表，分配**单调递增**的 id。
    ///
    /// ⚠️ 本方法**不做校验**——它返回 `SnapshotRecord` 而不是 `Result`。
    /// 合法性在**恢复入口**（[`Self::restore_session`]）把关：**进**的时候宽容，
    /// **用**的时候严格，比「存的时候就拒」更实用（存一份坏快照不会毁掉已有
    /// 的记录）。
    pub fn record(
        &mut self,
        payload: SessionSnapshotDto,
        label: impl Into<String>,
    ) -> SnapshotRecord {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let rec = SnapshotRecord {
            id,
            cp_meta: payload.cp_meta.clone(),
            payload,
            label: label.into(),
        };
        self.records.push(rec.clone());
        rec
    }

    /// Look up a snapshot by id.
    pub fn get(&self, id: u64) -> Option<&SnapshotRecord> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Iterate all stored snapshots, newest first.
    pub fn iter(&self) -> impl Iterator<Item = &SnapshotRecord> {
        self.records.iter().rev()
    }

    /// 校验一份会话载荷是否可以**被应用**。
    ///
    /// ⚠️ 本方法**只做校验**，**不**把状态灌回任何模块——那是内核宿主的事
    /// （它同时持有总线 / 外设 / INTC，且必须按冻结规格 §3.7 的**顺序**恢复：
    /// 内存映射 → 内存页 → 外设 → INTC → 引擎）。
    ///
    /// ⚠️ **校验发生在任何写入之前**（冻结规格 §5）：全部检查通过之前，调用方
    /// 不应该写任何状态。半个应用完的快照比被拒绝的快照更糟。
    pub fn restore_session(&self, payload: &SessionSnapshotDto) -> Result<(), PlatformError> {
        payload.validate()
    }
}

/// Family used by errors raised from this module.
///
/// Re-exported from the contract crate, which owns the error-code families
/// — defining a second constant with the same name and value here would
/// make the two ambiguous the moment a module imports both.
pub use riscv_sim_contract::snapshot::SNAPSHOT_FAMILY;

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::error::ErrorFamily;
    use riscv_sim_contract::model::{SimState, SIM_STATE_SCHEMA_VERSION};
    use riscv_sim_contract::session::{MemPageDto, RegionDto, UartStateDto};
    use riscv_sim_contract::snapshot::{QueuedEventDto, SchedEventDto, SNAPSHOT_SCHEMA_VERSION};

    // ------------------------------------------------------------------
    // 夹具
    // ------------------------------------------------------------------

    fn cp_meta(id: u32) -> CheckpointMetaDto {
        CheckpointMetaDto {
            id,
            t_virt: 12_345,
            t_host: 42,
            base_seq: 3,
            version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        }
    }

    fn engine() -> EngineSnapshotDto {
        EngineSnapshotDto {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            state: SimState {
                schema_version: SIM_STATE_SCHEMA_VERSION,
                virtual_time_ps: 1_000,
                pc: 0x8000_0004,
                event_queue_head: 1,
                ..SimState::default()
            },
            queue: vec![QueuedEventDto {
                at_ps: 2_000,
                seq: 0,
                event: SchedEventDto::PeripheralTick {
                    paddr: 0x4000_0000,
                    at_ps: 2_000,
                },
            }],
            next_seq: 1,
            instruction_budget: 1_000,
        }
    }

    fn m13_state() -> M13StateDto {
        M13StateDto {
            mem_pages: vec![MemPageDto {
                base: 0x1000,
                bytes: vec![1, 2, 3, 4],
            }],
        }
    }

    fn m14_map() -> M14MapDto {
        M14MapDto {
            regions: vec![RegionDto {
                base: 0x1000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            }],
        }
    }

    fn m15_periph() -> M15PeriphDto {
        M15PeriphDto {
            uarts: vec![UartStateDto {
                id: "UART0".to_string(),
                tx: Some(0x41),
                rx: None,
                status: 0,
                irq_line: 1,
                now_ps: 500,
            }],
            ..M15PeriphDto::default()
        }
    }

    fn m16_intc() -> M16IntcDto {
        M16IntcDto { raised: vec![1, 3] }
    }

    /// 走一遍 M-20 自己的组装路径，得到一份**合法**的会话载荷。
    fn sample_payload(mgr: &mut SnapshotManager, id: u32) -> SessionSnapshotDto {
        mgr.capture_session(
            cp_meta(id),
            engine(),
            m13_state(),
            m14_map(),
            m15_periph(),
            m16_intc(),
        )
    }

    // ------------------------------------------------------------------
    // 组装
    // ------------------------------------------------------------------

    /// 六段**逐段**原样落进载荷——判别式落在**被依赖方自己身上**
    /// （段的内容），而不是「调用没 panic」。
    #[test]
    fn capture_session_assembles_the_six_segments_verbatim() {
        let mut mgr = SnapshotManager::new();
        let payload = sample_payload(&mut mgr, 7);

        assert_eq!(
            payload.schema_version, SESSION_SNAPSHOT_SCHEMA_VERSION,
            "会话载荷的版本号必须由 M-20 钉死"
        );
        assert_eq!(payload.cp_meta, cp_meta(7), "段 0 必须原样落位");
        assert_eq!(payload.m12_engine, engine(), "段 1 必须原样落位");
        assert_eq!(payload.m13_state, m13_state(), "段 2 必须原样落位");
        assert_eq!(payload.m14_map, m14_map(), "段 3 必须原样落位");
        assert_eq!(payload.m15_periph, m15_periph(), "段 4 必须原样落位");
        assert_eq!(payload.m16_intc, m16_intc(), "段 5 必须原样落位");
    }

    /// 组装出来的载荷**必须能过自己的校验**——否则 M-20 造出了一个谁也
    /// 还原不了的快照。
    #[test]
    fn capture_session_produces_a_payload_that_validates() {
        let mut mgr = SnapshotManager::new();
        let payload = sample_payload(&mut mgr, 1);
        payload.validate().expect("M-20 组装出来的载荷必须自洽");
    }

    // ------------------------------------------------------------------
    // 内存记录
    // ------------------------------------------------------------------

    /// id **单调**且互不相同；载荷与 `cp_meta` **原样**存下。
    #[test]
    fn record_allocates_monotonic_ids_and_stores_the_payload_verbatim() {
        let mut mgr = SnapshotManager::new();
        let p1 = sample_payload(&mut mgr, 1);
        let p2 = sample_payload(&mut mgr, 2);

        let r1 = mgr.record(p1.clone(), "first");
        let r2 = mgr.record(p2.clone(), "second");

        assert!(r2.id > r1.id, "id 必须单调递增：{} 然后 {}", r1.id, r2.id);
        assert_ne!(r1.id, r2.id, "id 不得撞号");
        assert_eq!(r1.payload, p1, "载荷必须原样存下");
        assert_eq!(r1.cp_meta, p1.cp_meta, "段 0 必须原样存下");
        assert_eq!(r1.label, "first");
        assert_eq!(r2.payload, p2);
        assert_eq!(r2.label, "second");
    }

    /// `get(id)` 必须能取回**那条**记录（按 id 索引，不是「随便给一条」）。
    #[test]
    fn get_returns_the_record_identified_by_its_id() {
        let mut mgr = SnapshotManager::new();
        let p1 = sample_payload(&mut mgr, 1);
        let p2 = sample_payload(&mut mgr, 2);
        let r1 = mgr.record(p1.clone(), "first");
        let r2 = mgr.record(p2.clone(), "second");

        assert_eq!(mgr.get(r1.id).map(|r| r.payload.clone()), Some(p1));
        assert_eq!(mgr.get(r2.id).map(|r| r.payload.clone()), Some(p2));
        assert!(mgr.get(r1.id.wrapping_add(9_999)).is_none());
    }

    /// `iter()` 是**最新在前**。
    #[test]
    fn iter_yields_the_newest_record_first() {
        let mut mgr = SnapshotManager::new();
        let p1 = sample_payload(&mut mgr, 1);
        let p2 = sample_payload(&mut mgr, 2);
        let r1 = mgr.record(p1, "first");
        let r2 = mgr.record(p2, "second");

        let ids: Vec<u64> = mgr.iter().map(|r| r.id).collect();
        assert_eq!(
            ids,
            vec![r2.id, r1.id],
            "iter 必须是最新在前（旧版就是这个语义，别改）"
        );
    }

    // ------------------------------------------------------------------
    // 恢复路径（只校验）
    // ------------------------------------------------------------------

    /// 合法载荷**必须**被接受——否则恢复路径把好快照也拒了。
    #[test]
    fn restore_session_accepts_a_well_formed_payload() {
        let mut mgr = SnapshotManager::new();
        let payload = sample_payload(&mut mgr, 1);
        mgr.restore_session(&payload).expect("合法载荷必须被接受");
    }

    /// ⚠️ **版本不认识 → 拒**（台账 D-37 的核心防线）。
    ///
    /// 断言用**字面量** `5`（= `E_SNAP-05`），不用实现里引用的常量——
    /// 自指的断言分不出「改行为」与「没改行为」。
    #[test]
    fn restore_session_rejects_an_unknown_schema_version() {
        let mut mgr = SnapshotManager::new();
        let mut payload = sample_payload(&mut mgr, 1);
        payload.schema_version = 999;

        let err = mgr.restore_session(&payload).expect_err("未知版本必须被拒");
        assert_eq!(err.family, ErrorFamily::Snapshot, "族必须是 SNAP");
        assert_eq!(err.family.as_str(), "SNAP");
        assert_eq!(err.scenario, 5, "必须是 E_SNAP-05（版本不受支持）");
    }

    /// ⚠️ **结构畸形 → 拒**（不只是版本问题）。
    ///
    /// 判别式落在**载荷本身**：把区域表打乱成非升序，恢复必须拒。
    #[test]
    fn restore_session_rejects_a_malformed_segment_table() {
        let mut mgr = SnapshotManager::new();
        let mut payload = sample_payload(&mut mgr, 1);
        // 故意打乱：base 大的在前
        payload.m14_map.regions = vec![
            RegionDto {
                base: 0x2000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            },
            RegionDto {
                base: 0x1000,
                size: 0x1000,
                kind: "ram".to_string(),
                target: String::new(),
            },
        ];

        let err = mgr.restore_session(&payload).expect_err("结构畸形必须被拒");
        assert_eq!(err.family, ErrorFamily::Snapshot);
        assert_eq!(err.scenario, 6, "必须是 E_SNAP-06（会话载荷畸形）");
    }

    /// 内存页落在**任何 RAM 区域之外** → 拒（静默忽略 = 丢状态）。
    #[test]
    fn restore_session_rejects_a_page_outside_every_ram_region() {
        let mut mgr = SnapshotManager::new();
        let mut payload = sample_payload(&mut mgr, 1);
        payload.m13_state.mem_pages[0].base = 0x9000;

        mgr.restore_session(&payload)
            .expect_err("越界的内存页必须被拒");
    }

    /// ⚠️ **被拒的载荷不得动已有记录**——半个应用完的快照比被拒绝的快照更糟。
    #[test]
    fn a_rejected_payload_leaves_the_record_list_untouched() {
        let mut mgr = SnapshotManager::new();
        let good = sample_payload(&mut mgr, 1);
        let rec = mgr.record(good.clone(), "good");

        let mut bad = good.clone();
        bad.schema_version = 999;
        assert!(mgr.restore_session(&bad).is_err());

        assert_eq!(mgr.iter().count(), 1, "记录数不得变");
        let kept = mgr.get(rec.id).expect("记录必须还在");
        assert_eq!(kept.payload, good, "已有记录的内容不得被改写");
    }
}
