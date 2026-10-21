//! M-23 数据存储与持久化 DataStore (Data Store & Persistence).
//!
//! 拥有 `Project` 与**会话级快照载荷**的持久化。MVP 后端是浏览器
//! `IndexedDB`（前端）或本地 `SQLite` 文件（服务端）；后端在宿主启动时选定，
//! 内核从不直接看见后端。
//!
//! # 快照载荷改吃契约 DTO（台账 D-37）
//!
//! ⚠️ **旧签名 `put_snapshot(&mut self, s: &SnapshotRecord)` 已经消失。**
//! 理由：**两个落盘格式并存 = 「哪个是真的说不清」**。旧签名吃的
//! `SnapshotRecord` 是 partial 的（只有寄存器 / PC / CSR / 虚拟时间），落盘出来
//! 的东西**还原不了一个会话**——UART 的 RX/TX FIFO、定时器的
//! `mtime`/`mtimecmp`、GPIO 的方向 / 输出锁存、已拉起的 IRQ 行**全都不在**。
//!
//! 现在落盘的载体是契约层的 [`SessionSnapshotDto`]——**六段齐全**（冻结规格 =
//! `docs/detailed-design/STAGE-3_Session_Snapshot.md`）。
//!
//! `SnapshotRecord`（内存态记录，M-20 的东西）**仍然存在**，但它**不再**是落盘
//! 载体：它是「内存里的一条记录」，落盘的格式由契约 DTO 说了算。
//!
//! # 本模块提供什么
//!
//! * [`DataStore`] —— trait（宿主选后端）。
//! * [`memory::InMemoryDataStore`] —— **可用的参考实现**：MVP 的**兜底 /
//!   测试替身**，不是最终后端（最终是前端 IndexedDB / 服务端 SQLite）。它的
//!   价值在于**把语义钉死**：键怎么取、覆盖还是报错、找不到返回什么。

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::Project;
use riscv_sim_contract::session::SessionSnapshotDto;

/// 参考实现（内存态兜底 / 测试替身）——见 [`memory`]。
pub mod memory;

/// Storage backend. Implementations are out-of-tree at MVP and
/// selected by the host (front-end or server).
pub trait DataStore {
    /// Persist a project.
    fn put_project(&mut self, p: &Project) -> Result<(), PlatformError>;
    /// Load a project by id.
    fn get_project(&self, id: &str) -> Result<Option<Project>, PlatformError>;
    /// 持久化**会话级**快照载荷。
    ///
    /// ⚠️ **不再吃 partial 的 `SnapshotRecord`**（台账 D-37）——载体是契约层的
    /// [`SessionSnapshotDto`]（六段齐全）。
    ///
    /// **键** = 载荷里的 `cp_meta.id`（检查点 id，单调递增）。
    ///
    /// ⚠️ **实现方必须先校验再落盘**（[`SessionSnapshotDto::validate`]）：
    /// 落一份**自己都还原不了**的载荷，等于把一个坏状态永久留在存储里——
    /// 而存储里的东西**没人会再去校验一遍**。
    fn put_snapshot(&mut self, s: &SessionSnapshotDto) -> Result<(), PlatformError>;
    /// 读取会话级快照载荷（按 `cp_meta.id` 查）。
    ///
    /// 找不到返回 `Ok(None)`——「没有」是**正常状态**，不是错误。
    fn get_snapshot(&self, id: u64) -> Result<Option<SessionSnapshotDto>, PlatformError>;
    /// List all known project ids, newest first.
    fn list_projects(&self) -> Result<Vec<String>, PlatformError>;
}

/// Family used by errors raised from this module.
pub const STORE_FAMILY: ErrorFamily = ErrorFamily::Storage;

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::model::{ProductTree, ProjectVersion};

    /// 一个最小但**完整**的项目。
    fn project(id: &str) -> Project {
        Project {
            id: id.to_string(),
            name: format!("项目 {id}"),
            tree: ProductTree::default(),
            images: Vec::new(),
            version: ProjectVersion::default(),
        }
    }

    /// trait 的**五个**方法都在签名层面被钉住：少一个、多一个、或者类型
    /// 变了，这里都会编译不过。
    ///
    /// ⚠️ 判别式是**可编译**的——「trait 少了一个方法」会让**所有**实现方
    /// 编译失败，而不是「测试变红」。这里的作用是**让形状漂移立刻炸**。
    #[test]
    fn the_trait_has_exactly_the_five_methods() {
        fn assert_impl<T: DataStore>() {}
        assert_impl::<memory::InMemoryDataStore>();

        // 逐个方法的存在性 + 签名（用泛型函数把类型钉住）。
        fn put_project(d: &mut impl DataStore, p: &Project) -> Result<(), PlatformError> {
            d.put_project(p)
        }
        fn get_project(d: &impl DataStore, id: &str) -> Result<Option<Project>, PlatformError> {
            d.get_project(id)
        }
        fn put_snapshot(
            d: &mut impl DataStore,
            s: &SessionSnapshotDto,
        ) -> Result<(), PlatformError> {
            d.put_snapshot(s)
        }
        fn get_snapshot(
            d: &impl DataStore,
            id: u64,
        ) -> Result<Option<SessionSnapshotDto>, PlatformError> {
            d.get_snapshot(id)
        }
        fn list_projects(d: &impl DataStore) -> Result<Vec<String>, PlatformError> {
            d.list_projects()
        }

        let mut store = memory::InMemoryDataStore::new();
        let p = project("p1");
        put_project(&mut store, &p).expect("put");
        assert!(get_project(&store, "p1").expect("get").is_some());
        assert!(list_projects(&store)
            .expect("list")
            .contains(&"p1".to_string()));

        let payload = SessionSnapshotDto::default();
        put_snapshot(&mut store, &payload).expect("put snapshot");
        assert!(get_snapshot(&store, 0).expect("get").is_some());
    }
}
