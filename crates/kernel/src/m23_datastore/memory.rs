//! `InMemoryDataStore` —— **可用的参考实现**（MVP 的兜底 / 测试替身）。
//!
//! # ⚠️ 它**不是**最终后端
//!
//! 最终后端是**前端 IndexedDB** / **服务端 SQLite**（M-23 LLD §1.1）。本实现
//! 存在的理由不是「先凑合能用」，而是**把语义钉死**：键怎么取、同 id 再写是
//! 覆盖还是报错、找不到返回什么、写入前要不要校验——这些**每一个**都是「两个
//! 真相」的候选点，先在一个**看得见**的实现里定下来，后端接上时才有东西可对。
//!
//! # 语义（逐条写死，不留给后端自己发挥）
//!
//! | 场景 | 行为 |
//! | --- | --- |
//! | 快照的键 | 载荷里的 `cp_meta.id`（检查点 id，单调递增） |
//! | 写入前 | **先校验**（[`SessionSnapshotDto::validate`]），不合法 → 拒，**不落盘** |
//! | 同 id、同内容 | **幂等**（状态不变） |
//! | 同 id、不同内容 | **覆盖**（检查点 id 单调，正常不会撞；撞了就以**后写**为准） |
//! | 快照找不到 | `Ok(None)`（「没有」是正常状态，不是错误） |
//! | 项目找不到 | `Ok(None)` |
//! | `list_projects` | **最新在前**（trait 的既有语义） |
//!
//! ⚠️ **覆盖而不是报错**，是因为「同 id 不同内容」在正常流程里**不会发生**
//! （检查点 id 单调递增）；真发生了，多半是**调用方**的 bug——但那时**覆盖**
//! 至少不会把会话**变成两个真相**（覆盖 = 存储里只有一份，后来的读到的就是
//! 后写的那份；报错 = 存储里留着旧的，调用方拿到的却是报错，两边对不上）。

use std::collections::BTreeMap;

use riscv_sim_contract::error::PlatformError;
use riscv_sim_contract::model::Project;
use riscv_sim_contract::session::SessionSnapshotDto;

use super::DataStore;

/// 内存态 DataStore——MVP 兜底 / 测试替身。
#[derive(Debug, Default, Clone)]
pub struct InMemoryDataStore {
    /// 项目表（按 `Project::id` 索引）。
    projects: BTreeMap<String, Project>,
    /// 项目 id 的**写入顺序**（旧的在前）——`list_projects` 把它**倒过来**返回。
    project_order: Vec<String>,
    /// 快照表（按 `cp_meta.id` 索引）。
    snapshots: BTreeMap<u64, SessionSnapshotDto>,
}

impl InMemoryDataStore {
    /// 新的空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 存着的项目数。
    pub fn project_count(&self) -> usize {
        self.projects.len()
    }

    /// 存着的快照数。
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }
}

impl DataStore for InMemoryDataStore {
    /// 同 id **覆盖**；被覆盖的项目回到「最新」的位置。
    fn put_project(&mut self, p: &Project) -> Result<(), PlatformError> {
        // 先摘掉旧位置，保证「同 id 再写」之后它排到最新（`list_projects` 是
        // 「最新在前」——重写一个项目就是「它又变新了」）。
        self.project_order.retain(|id| id != &p.id);
        self.project_order.push(p.id.clone());
        self.projects.insert(p.id.clone(), p.clone());
        Ok(())
    }

    fn get_project(&self, id: &str) -> Result<Option<Project>, PlatformError> {
        Ok(self.projects.get(id).cloned())
    }

    /// ⚠️ **先校验再落盘**：落一份自己都还原不了的载荷 = 把一个坏状态永久留在
    /// 存储里，而存储里的东西**没人会再校验一遍**。
    fn put_snapshot(&mut self, s: &SessionSnapshotDto) -> Result<(), PlatformError> {
        s.validate()?;
        // 键 = 检查点 id。
        self.snapshots.insert(u64::from(s.cp_meta.id), s.clone());
        Ok(())
    }

    fn get_snapshot(&self, id: u64) -> Result<Option<SessionSnapshotDto>, PlatformError> {
        Ok(self.snapshots.get(&id).cloned())
    }

    /// **最新在前**。
    fn list_projects(&self) -> Result<Vec<String>, PlatformError> {
        Ok(self.project_order.iter().rev().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::error::ErrorFamily;
    use riscv_sim_contract::model::{ProductTree, ProjectVersion};
    use riscv_sim_contract::session::{
        CheckpointMetaDto, M13StateDto, M14MapDto, M15PeriphDto, M16IntcDto, MemPageDto, RegionDto,
        SESSION_SNAPSHOT_SCHEMA_VERSION,
    };
    use riscv_sim_contract::snapshot::{EngineSnapshotDto, SNAPSHOT_SCHEMA_VERSION};

    // ------------------------------------------------------------------
    // 夹具
    // ------------------------------------------------------------------

    /// 一份**六段齐全**的会话载荷。
    ///
    /// ⚠️ 内存页落在 RAM 区域内、区域表按 base 升序、IRQ 行升序——否则
    /// `validate()` 会拒，夹具本身就成了「一直红」的噪声源。
    fn sample_payload(id: u32) -> SessionSnapshotDto {
        SessionSnapshotDto {
            schema_version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            cp_meta: CheckpointMetaDto {
                id,
                t_virt: 12_345,
                t_host: 42,
                base_seq: 3,
                version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            },
            m12_engine: EngineSnapshotDto {
                schema_version: SNAPSHOT_SCHEMA_VERSION,
                ..EngineSnapshotDto::default()
            },
            m13_state: M13StateDto {
                mem_pages: vec![MemPageDto {
                    base: 0x1000,
                    bytes: vec![1, 2, 3, 4],
                }],
            },
            m14_map: M14MapDto {
                regions: vec![RegionDto {
                    base: 0x1000,
                    size: 0x1000,
                    kind: "ram".to_string(),
                    target: String::new(),
                }],
            },
            m15_periph: M15PeriphDto {
                uarts: vec![riscv_sim_contract::session::UartStateDto {
                    id: "UART0".to_string(),
                    tx: Some(0x41),
                    rx: None,
                    status: 0,
                    irq_line: 1,
                    now_ps: 500,
                }],
                ..M15PeriphDto::default()
            },
            m16_intc: M16IntcDto { raised: vec![1, 3] },
        }
    }

    fn project(id: &str) -> Project {
        Project {
            id: id.to_string(),
            name: format!("项目 {id}"),
            tree: ProductTree::default(),
            images: Vec::new(),
            version: ProjectVersion::default(),
        }
    }

    // ------------------------------------------------------------------
    // 快照落盘往返
    // ------------------------------------------------------------------

    /// 存进去 → 取出来 → **逐字段相等**。
    ///
    /// 判别式落在**被依赖方自己身上**（取出来的内容），不是「调用没 panic」。
    #[test]
    fn a_snapshot_round_trips_field_by_field() {
        let mut store = InMemoryDataStore::new();
        let payload = sample_payload(7);

        store.put_snapshot(&payload).expect("落盘必须成功");
        let back = store
            .get_snapshot(u64::from(payload.cp_meta.id))
            .expect("读取必须成功")
            .expect("刚存的快照必须读得到");

        assert_eq!(back, payload, "整份载荷必须逐字段相等");
        // 逐段再钉一遍——整份相等失败时报错能指出是哪一段。
        assert_eq!(back.cp_meta, payload.cp_meta);
        assert_eq!(back.m12_engine, payload.m12_engine);
        assert_eq!(back.m13_state, payload.m13_state);
        assert_eq!(back.m14_map, payload.m14_map);
        assert_eq!(back.m15_periph, payload.m15_periph);
        assert_eq!(back.m16_intc, payload.m16_intc);
        assert_eq!(
            back.m13_state.mem_pages[0].bytes, payload.m13_state.mem_pages[0].bytes,
            "内存页的**字节**必须一字不差"
        );
    }

    /// 键 = `cp_meta.id`——按**别的**键存（写入序 / 长度）都不行。
    #[test]
    fn snapshots_are_keyed_by_checkpoint_meta_id() {
        let mut store = InMemoryDataStore::new();
        let payload = sample_payload(42);

        store.put_snapshot(&payload).expect("落盘");
        assert!(
            store.get_snapshot(42).expect("读取").is_some(),
            "必须能用 cp_meta.id 取回"
        );
        assert!(
            store.get_snapshot(43).expect("读取").is_none(),
            "别的 id 必须取不到"
        );
    }

    /// 同 id、**同内容** → 幂等（存储状态不变）。
    #[test]
    fn rewriting_the_same_snapshot_is_idempotent() {
        let mut store = InMemoryDataStore::new();
        let payload = sample_payload(5);

        store.put_snapshot(&payload).expect("第一次落盘");
        store.put_snapshot(&payload).expect("第二次落盘");

        assert_eq!(store.snapshot_count(), 1, "同 id 不得存成两份");
        let back = store.get_snapshot(5).expect("读取").expect("必须还在");
        assert_eq!(back, payload);
    }

    /// 同 id、**不同内容** → **覆盖**（后写为准），且存储里仍然只有一份。
    #[test]
    fn rewriting_the_same_id_with_different_content_overwrites() {
        let mut store = InMemoryDataStore::new();
        let first = sample_payload(5);
        let mut second = sample_payload(5);
        second.m13_state.mem_pages[0].bytes = vec![9, 9, 9];
        second.cp_meta.t_virt = 99_999;

        store.put_snapshot(&first).expect("第一次落盘");
        store.put_snapshot(&second).expect("第二次落盘");

        assert_eq!(store.snapshot_count(), 1, "同 id 不得存成两份");
        let back = store.get_snapshot(5).expect("读取").expect("必须还在");
        assert_eq!(back, second, "后写的必须赢");
        assert_ne!(back, first, "旧内容不得残留");
    }

    /// 找不到 → `Ok(None)`（**不是**错误）。
    #[test]
    fn an_unknown_snapshot_id_is_none_not_an_error() {
        let store = InMemoryDataStore::new();
        let got = store.get_snapshot(999).expect("读取不报错");
        assert!(got.is_none(), "找不到必须是 None");
    }

    /// ⚠️ **畸形载荷被拒**，且**没落盘**。
    ///
    /// 断言用**字面量** `5`（= `E_SNAP-05`），不用实现里引用的常量——
    /// 自指的断言分不出「改行为」与「没改行为」。
    #[test]
    fn a_malformed_payload_is_rejected_before_it_is_stored() {
        let mut store = InMemoryDataStore::new();
        let mut bad = sample_payload(1);
        bad.schema_version = 999;

        let err = store.put_snapshot(&bad).expect_err("畸形载荷必须被拒");
        assert_eq!(err.family, ErrorFamily::Snapshot, "族必须是 SNAP");
        assert_eq!(err.family.as_str(), "SNAP");
        assert_eq!(err.scenario, 5, "必须是 E_SNAP-05（版本不受支持）");
        assert_eq!(store.snapshot_count(), 0, "被拒的载荷不得落盘");
    }

    /// ⚠️ **JSON 落盘往返**：序列化成 JSON 字符串 → 反序列化回来 → **逐字段
    /// 相等**。
    ///
    /// 这条证明契约 DTO **真的可持久化**——一个过不了 JSON 往返的 DTO 不是
    /// 持久化格式，只是一个内存结构。
    #[test]
    fn the_payload_survives_a_json_round_trip() {
        let mut store = InMemoryDataStore::new();
        let payload = sample_payload(11);

        store.put_snapshot(&payload).expect("落盘");
        let back = store
            .get_snapshot(11)
            .expect("读取")
            .expect("刚存的快照必须读得到");

        let json = serde_json::to_string(&back).expect("序列化成 JSON");
        let parsed: SessionSnapshotDto = serde_json::from_str(&json).expect("反序列化回 DTO");

        assert_eq!(parsed, payload, "JSON 往返后必须逐字段相等");
        assert_eq!(parsed.cp_meta, payload.cp_meta);
        assert_eq!(parsed.m12_engine, payload.m12_engine);
        assert_eq!(parsed.m13_state, payload.m13_state);
        assert_eq!(parsed.m14_map, payload.m14_map);
        assert_eq!(parsed.m15_periph, payload.m15_periph);
        assert_eq!(parsed.m16_intc, payload.m16_intc);
        assert_eq!(
            parsed.m13_state.mem_pages[0].bytes, payload.m13_state.mem_pages[0].bytes,
            "内存页的字节必须一字不差"
        );
    }

    /// ⚠️ **数值策略**：虚拟时间在线上是**十进制字符串**，不是 JSON 数字。
    ///
    /// 判别式落在**线上形状**上——`u64_decimal` 的反序列化器**两种形式都收**，
    /// 所以「能反序列化」证明不了什么，得看**线上到底长什么样**。
    #[test]
    fn the_json_wire_form_keeps_virtual_times_as_decimal_strings() {
        let payload = sample_payload(11);
        let json = serde_json::to_string(&payload).expect("序列化");

        assert!(
            json.contains("\"t_virt\":\"12345\""),
            "cp_meta.t_virt 必须是十进制字符串：{json}"
        );
        assert!(
            json.contains("\"now_ps\":\"500\""),
            "外设的 now_ps 必须是十进制字符串：{json}"
        );
        assert!(
            !json.contains("\"t_virt\":12345"),
            "虚拟时间不得以 JSON 数字出现：{json}"
        );
    }

    // ------------------------------------------------------------------
    // 项目存取
    // ------------------------------------------------------------------

    /// 项目存取往返。
    #[test]
    fn a_project_round_trips() {
        let mut store = InMemoryDataStore::new();
        let p = project("p1");

        store.put_project(&p).expect("落盘");
        let back = store.get_project("p1").expect("读取").expect("必须读得到");
        assert_eq!(back, p, "项目必须逐字段相等");
        assert!(store.get_project("nope").expect("读取").is_none());
    }

    /// `list_projects` 是**最新在前**（trait 的既有语义）。
    #[test]
    fn list_projects_is_newest_first() {
        let mut store = InMemoryDataStore::new();
        store.put_project(&project("p1")).expect("p1");
        store.put_project(&project("p2")).expect("p2");
        store.put_project(&project("p3")).expect("p3");

        assert_eq!(
            store.list_projects().expect("list"),
            vec!["p3".to_string(), "p2".to_string(), "p1".to_string()],
            "必须是最新在前"
        );

        // 重写 p1 → 它变成最新，且**不重复**。
        store.put_project(&project("p1")).expect("重写 p1");
        let ids = store.list_projects().expect("list");
        assert_eq!(
            ids,
            vec!["p1".to_string(), "p3".to_string(), "p2".to_string()]
        );
        assert_eq!(store.project_count(), 3, "重写不得让项目数变多");
    }
}
