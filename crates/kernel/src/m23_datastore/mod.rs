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
//!
//! # 通用字节归档（v0.28 · 冻结规格 STAGE-13 §3.4 · 裁决 K）
//!
//! [`DataStore::archive`] 归档一段**命名**的字节内容（诊断包 / 导出物）。
//!
//! ⚠️ **M-23 不依赖 M-22**：诊断包由 M-22 序列化成**字节**后传进来 ⇒
//! M-22 → M-23 是**单向**的，**不产生模块环**。
//!
//! ⚠️ **归档失败走 `Storage` 族（`STORE`），不是 HMS 族**：归档失败是**存储层**
//! 的故障，与健康管理（降级 / 终止）不是一回事 ⇒ 调用方按**族**分流，不去读
//! message 文本。
//!
//! # 场景码（v0.43 批 · 台账 **D-157** · 裁决 E）
//!
//! ⇒ ⇒ **一个码一个含义**：原先 M-22 的两个归档触发点**共用**一个码 ⇒ ⇒ 现在
//! **各用各的**：
//!
//! | 码 | 含义 | 谁报 |
//! | --- | --- | --- |
//! | [`E_STORE_04_ARCHIVE_BACKEND_MISSING`] | **没有归档通道**（宿主未注入后端） | M-22 |
//! | [`E_STORE_05_KIT_SERIALIZE_FAILED`] | **诊断包序列化失败** | M-22 |
//! | [`E_STORE_06_ARCHIVE_WRITE_FAILED`] | **归档写入失败**（后端拒绝写） | M-23 |
//!
//! ⚠️ **号段让开前端**（裁决 G · §2.4）：前端服务层自产的 STORE 码占 `01` /
//! `02` / `03` ⇒ ⇒ 内核从 **04** 起 ⇒ ⇒ **不重叠**。

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
    /// 归档一段**命名**内容（诊断包 / 导出物）。
    ///
    /// ⚠️ **通用字节接口**：M-23 **不依赖** M-22 ⇒ 诊断包由 M-22 序列化成
    /// **字节**后传入。这样 M-22 → M-23 是**单向**的，**不产生模块环**
    /// （冻结规格 STAGE-13 §3.4 · 裁决 K）。
    ///
    /// **同名**再写 ⇒ **覆盖**（后写为准），与快照同口径（存储里只有一份）。
    ///
    /// ⚠️ **失败族 = `Storage`（`STORE`）**，**不是** HMS：归档失败是**存储层**
    /// 的故障，与健康管理（降级 / 终止）不是一回事 ⇒ 调用方按**族 + 场景码**
    /// 分流，不去匹配 message 文本。
    fn archive(&mut self, name: &str, bytes: &[u8]) -> Result<(), PlatformError>;
}

/// Family used by errors raised from this module.
pub const STORE_FAMILY: ErrorFamily = ErrorFamily::Storage;

/// 场景码：**没有归档通道**（宿主**没有**注入归档后端）。
///
/// ⚠️ **v0.43 批新增**（冻结规格 STAGE-28 §2.3 · 台账 **D-157** · 裁决 E）：
/// M-22 的两个归档触发点原先**共用**一个码 ⇒ ⇒ 「**一个线上码两个含义**」
/// ⇒ ⇒ 现在**各用各的**：本码 = **没有归档通道**，序列化失败那个是
/// [`E_STORE_05_KIT_SERIALIZE_FAILED`]。
///
/// ⚠️ **号段让开前端**（裁决 G · §2.4）：前端服务层自产的 STORE 码占 `01` /
/// `02` / `03` ⇒ ⇒ 内核从 **04** 起 ⇒ ⇒ **不重叠** ⇒ ⇒ 「一个码一个含义」。
pub const E_STORE_04_ARCHIVE_BACKEND_MISSING: u16 = 4;

/// 场景码：**诊断包序列化失败**（`serde_json` 序列化诊断包失败）。
///
/// ⚠️ **v0.43 批新增**（STAGE-28 §2.3 · D-157 · 裁决 E）：见
/// [`E_STORE_04_ARCHIVE_BACKEND_MISSING`] 的说明 —— 这两个触发点原先**共用**
/// 一个码 ⇒ ⇒ 现在**各用各的**。
pub const E_STORE_05_KIT_SERIALIZE_FAILED: u16 = 5;

/// 场景码：**归档写入失败**（后端**拒绝**写入）。
///
/// ⚠️ **v0.43 批新增**（STAGE-28 §2.3 · D-157 · 裁决 E）：这个码承接的是
/// **M-23 自己**的场景 —— [`DataStore::archive`] 的**后端拒绝写入**（配额满 /
/// 目录不存在 / IndexedDB 事务回滚 …）⇒ ⇒ 与 M-22 那两个触发点**不是**一回事
/// ⇒ ⇒ **各用各的**（一个码一个含义）。
///
/// ⚠️ **谁拥有它**：**M-23**（拥有该场景的模块）—— 契约层 `error_families!` 的
/// 文档写死了这条规矩（「场景码住在拥有该场景的模块里」—— `E_BUS-*` 就在 M-14
/// 里），而 Storage 的场景码只能落在这里。
///
/// ⚠️ **v0.28 批只冻结了 `archive` 的**签名**，没有冻结 Storage 族的场景码；
/// 而在此之前 `E_STORE` 是 LLD §6 记的「**已声明未接线**」族 —— 族标签在
/// 契约层，**场景码一个都没有** ⇒ ⇒ 这三个码都落在这里。
pub const E_STORE_06_ARCHIVE_WRITE_FAILED: u16 = 6;

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

    /// trait 的**六个**方法都在签名层面被钉住：少一个、多一个、或者类型
    /// 变了，这里都会编译不过。
    ///
    /// ⚠️ 判别式是**可编译**的——「trait 少了一个方法」会让**所有**实现方
    /// 编译失败，而不是「测试变红」。这里的作用是**让形状漂移立刻炸**。
    ///
    /// ⚠️ **v0.28 批（STAGE-13 §3.4 · 裁决 K）**：第六个方法是 `archive`
    /// （通用字节归档）。既有五方法的**签名与语义一条没改**，只**加**了一条
    /// `archive` 的断言——**不许放松**其余五条。
    #[test]
    fn the_trait_has_exactly_the_six_methods() {
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
        fn archive(d: &mut impl DataStore, name: &str, bytes: &[u8]) -> Result<(), PlatformError> {
            d.archive(name, bytes)
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

        // 第六个方法：通用字节归档。
        archive(&mut store, "kit-1", b"kit bytes").expect("archive");
    }

    /// 一个**总是失败**的 DataStore——只有 `archive` 会被用到。
    ///
    /// ⚠️ 这是 A-5 的**被测对象**：归档失败**必须**落在 `Storage` 族
    /// （`STORE`），**不是** HMS 族（健康管理那条线）。
    struct FailingArchiveStore;

    impl DataStore for FailingArchiveStore {
        fn put_project(&mut self, _p: &Project) -> Result<(), PlatformError> {
            Ok(())
        }
        fn get_project(&self, _id: &str) -> Result<Option<Project>, PlatformError> {
            Ok(None)
        }
        fn put_snapshot(&mut self, _s: &SessionSnapshotDto) -> Result<(), PlatformError> {
            Ok(())
        }
        fn get_snapshot(&self, _id: u64) -> Result<Option<SessionSnapshotDto>, PlatformError> {
            Ok(None)
        }
        fn list_projects(&self) -> Result<Vec<String>, PlatformError> {
            Ok(Vec::new())
        }
        fn archive(&mut self, _name: &str, _bytes: &[u8]) -> Result<(), PlatformError> {
            Err(PlatformError::new(
                STORE_FAMILY,
                E_STORE_06_ARCHIVE_WRITE_FAILED,
                "归档区不可写",
            ))
        }
    }

    /// ⚠️ **归档失败 ⇒ `Storage` 族**（STAGE-13 §5.1 A-5）。
    ///
    /// 归档失败是**存储层**的故障（写不进去 / 后端不可用），与健康管理
    /// （降级 / 终止）**不是**一回事 ⇒ 族必须是 `STORE`。
    ///
    /// 断言落在 **family + scenario** 上，**不**匹配 message 文本——
    /// message 是给人看的，族与码才是机器分流依据。
    ///
    /// ⚠️ **v0.43 批（D-157）**：**后端拒绝写入**是 **M-23 自己**的场景 ⇒ ⇒
    /// 它的码是 `E_STORE-06` ⇒ ⇒ 与 M-22 那两个触发点（`E_STORE-04` /
    /// `E_STORE-05`）**各用各的**。
    #[test]
    fn a_failed_archive_reports_the_storage_family() {
        let mut store = FailingArchiveStore;

        let err = store
            .archive("kit-1", b"kit bytes")
            .expect_err("归档必须失败");

        assert_eq!(err.family, ErrorFamily::Storage, "归档失败的族必须是 STORE");
        assert_eq!(err.family.as_str(), "STORE");
        assert_eq!(
            err.scenario, E_STORE_06_ARCHIVE_WRITE_FAILED,
            "场景码必须是 E_STORE-06（**归档写入失败**）"
        );
        assert_ne!(
            err.family,
            ErrorFamily::Health,
            "归档失败**不得**走 HMS 族 —— 那条线是健康管理（降级 / 终止）"
        );
        assert_eq!(STORE_FAMILY, ErrorFamily::Storage);
    }

    /// ⚠️ **M-23 的生产代码永不报 HMS 族**（STAGE-13 §5.1 A-5 的判别式半边）。
    ///
    /// 上半条（[`a_failed_archive_reports_the_storage_family`]）钉的是**接口
    /// 契约**；这半条钉的是**源码**：M-23 的生产代码里一旦出现 HMS 族的构造点
    /// （`ErrorFamily::Health` / `HMS_FAMILY`），这里就红。
    ///
    /// 只扫**生产代码**（`#[cfg(test)]` 之前的那一半）⇒ 测试里提到族名不算。
    #[test]
    fn the_module_never_reports_the_health_family() {
        for (name, text) in [
            ("mod.rs", include_str!("mod.rs")),
            ("memory.rs", include_str!("memory.rs")),
        ] {
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            assert!(
                !production.contains("ErrorFamily::Health"),
                "{name} 的生产代码不得构造 HMS 族（归档失败走 STORE）"
            );
            assert!(
                !production.contains("HMS_FAMILY"),
                "{name} 的生产代码不得引用 HMS_FAMILY"
            );
        }
    }

    // ------------------------------------- D-157 · 拆号（v0.43 批）

    /// ⚠️ **v0.43 批（D-157 · 裁决 E）**：三个新码的**值与名一致**，且**不与前端
    /// 占的 `01` / `02` / `03` 重叠**。
    ///
    /// ⚠️ **判别力**：把 `E_STORE_05_…` 的值写成 `4` ⇒ 本条变红（值与名不符）；
    /// 把 `E_STORE_04_…` 的值写成 `1` ⇒ 本条变红（撞前端占的号）。
    #[test]
    fn the_new_store_codes_are_numbered_and_do_not_clash_with_the_front_end() {
        assert_eq!(
            E_STORE_04_ARCHIVE_BACKEND_MISSING, 4,
            "「没有归档通道」= E_STORE-04 ⇒ 值必须是 4"
        );
        assert_eq!(
            E_STORE_05_KIT_SERIALIZE_FAILED, 5,
            "「诊断包序列化失败」= E_STORE-05 ⇒ 值必须是 5"
        );
        assert_eq!(
            E_STORE_06_ARCHIVE_WRITE_FAILED, 6,
            "「归档写入失败」= E_STORE-06 ⇒ 值必须是 6"
        );

        // 前端服务层自产的 STORE 码占 01 / 02 / 03 ⇒ ⇒ 内核**不许**撞。
        for front_num in [1u16, 2, 3] {
            assert_ne!(
                E_STORE_04_ARCHIVE_BACKEND_MISSING, front_num,
                "内核的新码撞上了前端占的 E_STORE-0{front_num}"
            );
            assert_ne!(
                E_STORE_05_KIT_SERIALIZE_FAILED, front_num,
                "内核的新码撞上了前端占的 E_STORE-0{front_num}"
            );
            assert_ne!(
                E_STORE_06_ARCHIVE_WRITE_FAILED, front_num,
                "内核的新码撞上了前端占的 E_STORE-0{front_num}"
            );
        }

        // 三个码**互不相同** ⇒ 「一个码一个含义」。
        assert_ne!(
            E_STORE_04_ARCHIVE_BACKEND_MISSING, E_STORE_05_KIT_SERIALIZE_FAILED,
            "「没有归档通道」与「诊断包序列化失败」不许共用一个码"
        );
        assert_ne!(
            E_STORE_05_KIT_SERIALIZE_FAILED, E_STORE_06_ARCHIVE_WRITE_FAILED,
            "「诊断包序列化失败」与「归档写入失败」不许共用一个码"
        );
    }

    /// ⚠️ **v0.43 批（D-157 · 裁决 E）**：被拆掉的那个旧归档失败码，在**整个
    /// `crates/` 树**里**零命中**。
    ///
    /// ⚠️ **为什么扫全树**（而不只扫本模块）：「旧码还留在某个角落」⇒ ⇒ 下一个人
    /// 会以为还能用 ⇒ ⇒ **删除也要干净**（规格 §3.2）。
    ///
    /// ⚠️ **针脚是拼出来的** ⇒ ⇒ 本用例的**源码里**不出现那个字面量 ⇒ ⇒ 它
    /// **不会把自己扫成红**（自指的断言分不出「改行为」与「没改行为」）。
    ///
    /// ⚠️ **判别力**：把旧码写回任何一处（定义 / 导入 / 用例 / 注释）⇒ 本条
    /// 立刻变红。
    #[test]
    fn the_retired_archive_failure_code_is_gone_from_the_whole_crates_tree() {
        let needle = ["E_STORE", "01", "ARCHIVE", "FAILED"].join("_");
        let crates_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate 目录的上级必须是 crates/")
            .to_path_buf();

        let mut hits: Vec<String> = Vec::new();
        let mut stack = vec![crates_root.clone()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).expect("目录必须读得到");
            for entry in entries {
                let entry = entry.expect("目录项必须读得到");
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                // ⚠️ 非 UTF-8（**二进制**）⇒ 读不动 ⇒ 跳过（那不是源码）。
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                if text.contains(&needle) {
                    hits.push(path.display().to_string());
                }
            }
        }

        assert!(
            hits.is_empty(),
            "被拆掉的旧归档失败码在 crates/ 树里仍有残留：{hits:?}"
        );
    }
}
