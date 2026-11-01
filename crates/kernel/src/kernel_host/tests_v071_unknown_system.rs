//! STAGE-47 · v0.71 批 —— **「未知系统」门禁**的宿主侧门禁（K 组）。
//!
//! ⚠️ **本文件钉的是「内核怎么判」**：镜像要的地址区间 ⟷ **当前生效的内存
//! 分区** ⇒ ⇒ 判不过就**不建图、不写总线**。
//!
//! ⚠️ **断言全部落在输出上**：命令回的事件 ⇒ ⇒ 没有一条断言是「我调用了
//! 某个内部函数」⇒ ⇒ **纯函数**那部分（`compatibility`）另有**单元用例**。
//!
//! ⚠️ **判别力**：
//! - 把「完全落不在」也判成认得 ⇒ ⇒ **K-2 变红**（X-1）；
//! - 把「部分落不在」也判成不认得 ⇒ ⇒ **K-3 变红**（用户拍的**中档**就没了）；
//! - 把判定挪到建图之后 ⇒ ⇒ **K-2 变红**（顺序是硬约束）；
//! - 判定用**自动选档**那份描述而不是用户的板 ⇒ ⇒ **K-5 变红**。

use riscv_sim_contract::model::{
    ImageKind, ImageRef, ImageSource, ProductTree, Project, ProjectVersion, TreeNode,
};
use riscv_sim_contract::platform::{PlatformDescription, RegionSpec};
use riscv_sim_contract::protocol::{CommandPayload, EventPayload};

use super::tests_watchdog::{dispatch, errors_of};
use super::KernelHost;

/// 一条 40 字节的**裸镜像**（落在基址 0 ⇒ ⇒ 与仓内 demo 固件同形）。
const IMAGE: [u8; 40] = [0x93; 40];

/// 一条 RAM 在 `base`、长 `size` 的描述（**用户的板**）。
fn board_with_ram(base: u32, size: u32) -> PlatformDescription {
    PlatformDescription {
        schema_version: riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        id: "user-board".to_string(),
        name: "User board".to_string(),
        priority: 100,
        // ⚠️ 用户配的板**不**参与自动选档（它是**点名**用的）。
        auto_selectable: false,
        description: None,
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        max_region_bytes: None,
        regions: vec![RegionSpec::Ram { base, size }],
    }
}

/// 一个只含那块裸镜像的工程。
fn project() -> Project {
    Project {
        id: "proj-unknown-system".to_string(),
        name: "unknown-system".to_string(),
        tree: ProductTree {
            root: TreeNode {
                name: "MainBoard".to_string(),
                component_id: None,
                children: Vec::new(),
            },
        },
        images: vec![ImageRef {
            name: "demo.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(IMAGE.to_vec()),
        }],
        version: ProjectVersion {
            schema: 1,
            isa: "RV32IMC".to_string(),
            toolchain: "test".to_string(),
        },
    }
}

/// 配一张**用户的板**（⇒ ⇒ 之后 `LoadProject` 就**用**它）。
fn set_user_board(host: &mut KernelHost, description: PlatformDescription) {
    let events = dispatch(
        host,
        CommandPayload::SetMemoryMap {
            description,
            expected_revision: None,
        },
    );
    let errors = errors_of(&events);
    assert!(
        errors.is_empty(),
        "配用户的板就失败了（{:?}）⇒ ⇒ 后面那些判据没有意义",
        errors
    );
}

/// 装那个工程 ⇒ ⇒ 回的事件。
fn load(host: &mut KernelHost) -> Vec<EventPayload> {
    dispatch(host, CommandPayload::LoadProject(project()))
}

/// 事件里有没有 `E-PLT-17`（「未知系统」那个码）。
///
/// ⚠️ **线上形态是 `E_PLT-17`**（族与场景码之间是**连字符**）⇒ ⇒ Rust
///   常量名是 `E_PLT_17_BOARD_MISMATCH` ⇒ ⇒ 两者**不是一个字符串** ⇒ ⇒
///   断言必须用**线上形态** ⇒ ⇒ 否则那条断言**恒假** ⇒ ⇒ 零判别力。
fn is_unknown_system(events: &[EventPayload]) -> bool {
    errors_of(events).iter().any(|(code, _)| code == "E_PLT-17")
}

// ---------------------------------------------------------------------------

/// **K-1** —— 镜像要的地址**完全落在**用户的板的 RAM 里 ⇒ ⇒ 装得上。
#[test]
fn k1_image_inside_the_user_board_loads() {
    let mut host = KernelHost::new();
    set_user_board(&mut host, board_with_ram(0x0000_0000, 0x0001_0000));

    let events = load(&mut host);

    let errors = errors_of(&events);
    assert!(
        errors.is_empty(),
        "镜像要的地址**完全落在**用户的板上却装不上（{:?}）⇒ ⇒ 判定太严",
        errors
    );
    assert!(
        !is_unknown_system(&events),
        "完全落在时**不该**报「未知系统」"
    );
}

/// **K-2** —— 镜像要的地址**完全落不在**用户的板上 ⇒ ⇒ **拒**（「未知系统」）。
#[test]
fn k2_image_outside_the_user_board_is_rejected() {
    let mut host = KernelHost::new();
    // ⚠️ 这块板**没有**基址 0 的 RAM ⇒ ⇒ 裸镜像（落在 0）**完全落不在**。
    set_user_board(&mut host, board_with_ram(0x8000_0000, 0x0001_0000));
    let pc_before = host.pc();

    let events = load(&mut host);

    assert!(
        is_unknown_system(&events),
        "镜像要的地址**完全落不在**用户的板上却装上了 ⇒ ⇒ 「未知系统」**永不出现**\
         ⇒ ⇒ 那个判据零判别力（事件={:?}）",
        errors_of(&events)
    );
    // ⚠️ **拒 ⇒ ⇒ 不建图、不写总线** ⇒ ⇒ PC 不会动。
    assert_eq!(
        host.pc(),
        pc_before,
        "拒了之后 PC 变了 ⇒ ⇒ 图已经建了 / 总线已经写了 ⇒ ⇒ 顺序错了"
    );
}

/// **K-3** —— 镜像要的地址**部分落不在**（中档）⇒ ⇒ **不拒**。
#[test]
fn k3_partially_outside_is_not_rejected() {
    let mut host = KernelHost::new();
    // ⚠️ 这块板的 RAM @ 0 **只有 16 字节** ⇒ ⇒ 40 字节的镜像**一半在外面**
    // ⇒ ⇒ 用户拍的是**中档** ⇒ ⇒ **不拒** ⇒ ⇒ 撞的那部分由**总线**去报。
    set_user_board(&mut host, board_with_ram(0x0000_0000, 16));

    let events = load(&mut host);

    assert!(
        !is_unknown_system(&events),
        "**部分**落不在也拒了 ⇒ ⇒ 用户拍的**中档**被做成了**严档**\
         （事件={:?}）",
        errors_of(&events)
    );
}

/// **K-4** —— **必要对照**：「完全落不在」与「部分落不在」是**两个不同的判据**。
///
/// ⚠️ 这一条是 **K-2 / K-3 的判别力来源**：若两者被**同一个布尔**代替 ⇒ ⇒
///   「部分」会被判成「完全不」⇒ ⇒ ⇒ **中档**消失 ⇒ ⇒ 而 **K-3** 会红 ⇒ ⇒
///   ⇒ 这一条把「两者**必须**分开」**钉死**。
#[test]
fn k4_missing_and_partial_are_two_different_verdicts() {
    let board = board_with_ram(0x0000_0000, 16);
    let board_ranges = super::platform::board_intervals(&board);
    let verdict = super::platform::compatibility(&[(0, 40)], &board_ranges);
    assert!(
        verdict.partial,
        "40 字节的镜像落在 16 字节的 RAM 上 ⇒ ⇒ 应当是**部分**落不在"
    );
    assert!(
        !verdict.missing,
        "**部分**落不在**不该**同时算「完全落不在」⇒ ⇒ 两者是**两个判据**"
    );

    let empty_board = board_with_ram(0x8000_0000, 16);
    let empty_ranges = super::platform::board_intervals(&empty_board);
    let verdict = super::platform::compatibility(&[(0, 40)], &empty_ranges);
    assert!(
        verdict.missing,
        "镜像要的地址**完全落不在**⇒ ⇒ 应当判成「完全落不在」"
    );
    assert!(
        !verdict.partial,
        "「完全落不在」**不该**同时算「部分落不在」⇒ ⇒ 两者是**两个判据**"
    );
}

/// **K-5** —— 判定用的是**用户配的板**（`user_board`）⇒ ⇒ 用户刚配的板
/// **不会**被自己的固件判成「不认得」。
#[test]
fn k5_the_verdict_uses_the_users_board() {
    let mut host = KernelHost::new();
    // ⚠️⚠️ **夹具必须让「用户的板」与「旧图」的判定结果**相反** ⇒ ⇒ 否则
    //   这条**分辨不出**判定用的是哪一份 ⇒ ⇒ **零判别力**。
    //
    //   · 用户的板**没有**基址 0 的 RAM ⇒ ⇒ 镜像（落在 0）**完全落不在**它
    //     上面 ⇒ ⇒ 正确判定 ⇒ ⇒ **拒**；
    //   · `self.platform`（**旧图** = 内置 default 板）**有**基址 0 的 RAM
    //     ⇒ ⇒ 若判定错用旧图 ⇒ ⇒ **不拒** ⇒ ⇒ ⇒ 两者结果**相反** ⇒ ⇒
    //     ⇒ 这条夹具**才**抓得住「判定用错了哪一份」。
    set_user_board(&mut host, board_with_ram(0x8000_0000, 0x0001_0000));

    let events = load(&mut host);

    assert!(
        is_unknown_system(&events),
        "用户的板**没有**那块地址却**没**判成「不认得」⇒ ⇒ 判定用的**不是**用户的板\
         （而是旧图 ⇒ ⇒ 事件={:?}）",
        errors_of(&events)
    );
}

/// **必要对照** —— **用户没配板** ⇒ ⇒ 走**自动选档** ⇒ ⇒ **不**判「未知系统」。
///
/// ⚠️ 这是**存量链路零影响**的那条护栏 ⇒ ⇒ 平台的真机门禁（脚本口、
/// demo 固件、osz）全都走这条路 ⇒ ⇒ 它们**必须**照旧装得上。
#[test]
fn k6_without_a_user_board_the_auto_selection_still_works() {
    let mut host = KernelHost::new();

    // ⚠️ **没有** `SetMemoryMap` ⇒ ⇒ 从未配过板 ⇒ ⇒ 走自动选档。
    let events = load(&mut host);

    assert!(
        !is_unknown_system(&events),
        "用户**没配板**时也报「未知系统」⇒ ⇒ **存量链路被破坏**了\
         （事件={:?}）",
        errors_of(&events)
    );
}
