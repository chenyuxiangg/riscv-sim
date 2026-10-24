//! Lane B · 宿主侧的**板级内存图 · 差量 / 卸载 / 预算拆分**
//! （v0.31 · STAGE-16 §5 · 验收 **B-1 ~ B-14**）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-16_V031_Board_Delta_Unload_Budget.md`
//! §1（裁决 A~N）· §3（内核宿主的处理）· §5（B 组验收）。
//!
//! ## 这个文件钉住的是**宿主**这一段的十四件事
//!
//! | 验收 | 一句话 |
//! | --- | --- |
//! | **B-1** | `SetMemoryMap`（`expected_revision = None`）⇒ 行为与 v0.29 **逐字一致** |
//! | **B-2** | `SetMemoryMap` 带**过期**的 `expected_revision` ⇒ **`E_PLT-13`** ⇒ 什么都不动 |
//! | **B-3** | `SetMemoryMap` 带**正确**的 `expected_revision` ⇒ 接受 ⇒ 修订号 **+1** |
//! | **B-4** | `PatchMemoryMap`（改一段的 size）⇒ 那段**逐字段**变，其余段**一字未动** |
//! | **B-5** | `PatchMemoryMap`（删一段）⇒ 该段**消失**，其余段**逐字段**不变 |
//! | **B-6** | `max_region_bytes` 给一个**比声明期小**的值 ⇒ 写入超限 ⇒ **`E_BUS-09`** |
//! | **B-7** | **不给** `max_region_bytes` ⇒ `bus.max_region_bytes() == region_budget_bytes` |
//! | **B-8** | `PatchMemoryMap`（改大到与邻段重叠）⇒ **`E_PLT-06`** ⇒ 什么都不动 |
//! | **B-9** | 差量要删的那段**不存在** ⇒ **`E_PLT-14`** ⇒ 什么都不动 |
//! | **B-10** | 差量为**空** ⇒ 接受（合成结果 = 原图）⇒ 修订号 **+1** |
//! | **B-11** | `UnloadProject` ⇒ **之后改板立即生效**（`pending == false`） |
//! | **B-12** | `UnloadProject` ⇒ 用户的板**留着** ⇒ 镜像**留着** |
//! | **B-13** | `UnloadProject` ⇒ 宿主的主题集合**清空**（订阅失效） |
//! | **B-14** | 装载工程时**待生效的板被应用** ⇒ 生效图 = 那块板 ⇒ 修订号 **+1** |
//!
//! ⚠️ **断言值一律写架构字面量**（`"E_PLT-13"` / `"E_PLT-14"` / `"E_PLT-06"`
//! / `"E_BUS-09"` / `0x5000_0000` …）—— 从被测实现反推出来的断言是**假绿**。
//!
//! ⚠️ **既有断言一条不改**：v0.29 的那批用例在
//! [`tests_board_memory_map.rs`](super::tests_board_memory_map) 里 ⇒ 本文件
//! **只增不改** ⇒ 「行为与 v0.29 逐字一致」（B-1）由那边守着。
//!
//! ⚠️ **「什么都不动」怎么观察**：① 生效的图 = 事件带回来的 `description`
//! ② 用户的板 = **推导量** `pending`（用户的板 ≠ 生效的图 ⇒ 待生效）
//! ③ 修订号 = 事件带回来的 `revision` ⇒ 三者一起看 ⇒ 「什么都没动」才
//! **可证**，而不是「看起来没动」。

use super::*;

use riscv_sim_contract::model::{ProductTree, ProjectVersion};
use riscv_sim_contract::platform::{DeviceSpec, RegionSpec, PLATFORM_SCHEMA_VERSION};
use riscv_sim_contract::protocol::MemoryMapOp;

use crate::m14_bus::BusWidth;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 一条 `MemoryMap` 事件的**四个字段**（裁决 M）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct MemoryMapState {
    description: PlatformDescription,
    pending: bool,
    loaded: bool,
    revision: u64,
}

/// 取第一条 `MemoryMap` 事件（**四个字段**都要）。
fn first_memory_map(events: &[EventPayload]) -> MemoryMapState {
    for e in events {
        if let EventPayload::MemoryMap {
            description,
            pending,
            loaded,
            revision,
        } = e
        {
            return MemoryMapState {
                description: description.clone(),
                pending: *pending,
                loaded: *loaded,
                revision: *revision,
            };
        }
    }
    panic!("没有 MemoryMap 事件：{events:?}");
}

/// 取第一条 `Error` 事件的错误码。
fn first_error_code(events: &[EventPayload]) -> String {
    for e in events {
        if let EventPayload::Error { code, .. } = e {
            return code.clone();
        }
    }
    panic!("没有 Error 事件：{events:?}");
}

/// 造一个装着**一段裸机器码**的工程（镜像落在基址 0）。
///
/// ⚠️ 与 v0.29 那份夹具**同形**（镜像落在 0 ⇒ 低段必须有 RAM）。
fn demo_project() -> Project {
    Project {
        id: "proj-demo".to_string(),
        name: "demo".to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "demo-firmware.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(super::DEMO_FIRMWARE_BYTES.to_vec()),
        }],
        version: ProjectVersion::default(),
    }
}

/// 一张「用户配的板」—— 与 v0.29 那张**逐字同形**。
///
/// ⚠️ **三段**：低段 64 KiB RAM（放镜像）+ `0x5000_0000` 起的 4 KiB ROM +
/// `0x6000_0000` 的 GPIO。
fn user_board() -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "user-board".to_string(),
        name: "User board".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        region_budget_bytes: 16 * 1024 * 1024,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个。
        max_region_bytes: None,
        regions: vec![
            RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0001_0000,
            },
            RegionSpec::Rom {
                base: 0x5000_0000,
                size: 0x0000_1000,
            },
            RegionSpec::Peripheral {
                base: 0x6000_0000,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
        ],
    }
}

/// 一张**只有 RAM** 的板（运行期上界那一条用例用 —— 见 B-6 的口径说明）。
///
/// ⚠️ **为什么只有 RAM**：外设窗口的大小也走运行期上界那条检查
/// ⇒ 板里有 GPIO 的话，「上界 = 1 字节」会在**建图**时就报 `E_BUS_09`
/// ⇒ 钉的就不是「**写入**超限」而是「建图超限」了 ⇒ 判据被换掉了。
fn ram_only_board(max_region_bytes: Option<u32>) -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "ram-only".to_string(),
        name: "RAM only".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        region_budget_bytes: 16 * 1024 * 1024,
        max_region_bytes,
        regions: vec![RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0001_0000,
        }],
    }
}

/// 当前生效的图（走一次订阅拿**四个字段**）。
fn current_state(host: &mut KernelHost) -> MemoryMapState {
    let events = dispatch(
        host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    first_memory_map(&events)
}

// ---------------------------------------------------------------------------
// B-1 · `SetMemoryMap`（`expected_revision = None`）⇒ 与 v0.29 逐字一致
// ---------------------------------------------------------------------------

/// **B-1** —— `SetMemoryMap`（**不带**期望修订号）⇒ 行为与 v0.29 **逐字
/// 一致**：未装载 ⇒ **立即生效**（生效图逐字段等于那份描述）⇒ 没有待生效
/// 的 ⇒ 修订号 **+1**（第一次被接受的变更）。
#[test]
fn b1_set_memory_map_without_an_expected_revision_behaves_exactly_as_before() {
    let mut host = KernelHost::new();
    let board = user_board();

    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(
        state.description, board,
        "事件带回来的必须是**这份**描述（逐字一致）"
    );
    assert!(
        !state.pending,
        "未装载 ⇒ 立即生效 ⇒ 没有「待生效」的（STAGE-14 §3.2 ③）"
    );
    assert!(!state.loaded, "没装载过镜像 ⇒ `loaded` 必须是假");
    assert_eq!(state.revision, 1, "第一次被接受的变更 ⇒ 修订号 0 → 1");

    // 总线按新图**重建**（与 v0.29 的 B-1 同一条判据）。
    assert_eq!(
        host.platform_id(),
        "user-board",
        "生效的图必须是用户配的那张"
    );
    let regions = host.memory_map().regions;
    assert_eq!(
        regions.len(),
        3,
        "描述里有三段 ⇒ 总线上也得是三段：{regions:?}"
    );
    assert_eq!(
        regions[0].size, 0x0001_0000,
        "低段 RAM 的大小必须与描述一致"
    );
    assert_eq!(regions[1].base, 0x5000_0000, "ROM 的基址");
    assert_eq!(regions[2].base, 0x6000_0000, "GPIO 的基址");
}

/// **B-1（负向半）** —— **不带**期望修订号 ⇒ **不检查**修订号 ⇒ 哪怕带上
/// 一个**过期**的号也一样被接受（`None` 才是「不检查」，带值就是检查）。
///
/// ⚠️ 这里钉的是 `None` 的**语义**：不带 ⇒ 旧调用方**不受影响**（裁决 B）。
#[test]
fn b1_a_missing_expected_revision_never_conflicts() {
    let mut host = KernelHost::new();
    // 先接受一次 ⇒ 修订号变成 1。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    // 再带一个**过期**的号（0）⇒ **被拒** ⇒ 这就是「带值就是检查」。
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: Some(0),
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-13",
        "带值 ⇒ 要检查 ⇒ 过期的号必须被拒：{events:?}"
    );

    // 而 `None` ⇒ 同一个号**不会**被拒。
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(state.revision, 2, "不检查 ⇒ 照样被接受 ⇒ 修订号 +1");
}

// ---------------------------------------------------------------------------
// B-2 / B-3 · 修订号检查
// ---------------------------------------------------------------------------

/// **B-2** —— `SetMemoryMap` 带**过期**的 `expected_revision` ⇒ 报
/// **`E_PLT-13`** ⇒ **图、用户的板、修订号三者都不变**。
#[test]
fn b2_a_stale_expected_revision_is_rejected_and_changes_nothing() {
    let mut host = KernelHost::new();
    // 先接受一次 ⇒ 修订号 1、生效的图 = 用户板、用户的板 = 用户板。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let before = current_state(&mut host);
    assert_eq!(before.revision, 1);
    assert!(!before.pending, "用户的板就是生效的图 ⇒ 没有待生效的");

    // 拿一个**过期**的修订号（0）提交另一张板 ⇒ 必须被明确拒绝。
    let other = ram_only_board(None);
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: other.clone(),
            expected_revision: Some(0),
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-13",
        "过期的期望修订号必须是 E_PLT-13（族 PLT / 码 13）：{events:?}"
    );

    // 三者**都不变**：生效的图、用户的板（⇒ `pending` 仍是假）、修订号。
    let after = current_state(&mut host);
    assert_eq!(
        after.description, before.description,
        "被拒 ⇒ 生效的图不许变"
    );
    assert!(
        !after.pending,
        "被拒 ⇒ **用户的板**不许被换掉（用户的板还是生效的图 ⇒ 不待生效）"
    );
    assert_eq!(after.revision, before.revision, "被拒 ⇒ 修订号不许变");
}

/// **B-2（次序）** —— **修订号检查排在 `validate()` 之前**（裁决 B）⇒ 两个错
/// **不会互相盖住**。
///
/// ⚠️ 判据：一份**既过期**又**不合规**的提交 ⇒ 报的是 **`E_PLT-13`**
/// （而不是 `E_PLT-07`）⇒ 「你是不是基于旧状态」**更先**被回答。
#[test]
fn b2_the_revision_check_comes_before_the_description_validation() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );

    // 一份**超预算**的描述（违反 V-7 ⇒ 本该报 `E_PLT-07`）+ **过期**的
    // 期望修订号（0）。
    let mut over_budget = user_board();
    over_budget.region_budget_bytes = 0x1000;
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: over_budget,
            expected_revision: Some(0),
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-13",
        "修订号检查排在 validate() 之前 ⇒ 先报冲突：{events:?}"
    );

    // ⚠️ **同一个描述** + **正确**的期望修订号 ⇒ 这时才轮到 `validate()` ⇒
    // 报 `E_PLT-07` ⇒ 证明两个错**各归各的**（不是同一个错被换了名字）。
    let mut over_budget = user_board();
    over_budget.region_budget_bytes = 0x1000;
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: over_budget,
            expected_revision: Some(1),
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-07",
        "修订号对得上 ⇒ 才轮到描述校验：{events:?}"
    );
}

/// **B-3** —— `SetMemoryMap` 带**正确**的 `expected_revision` ⇒ 接受 ⇒
/// 修订号 **+1**。
#[test]
fn b3_a_correct_expected_revision_is_accepted_and_bumps_the_revision() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    assert_eq!(first_memory_map(&events).revision, 1, "第一次 +1");

    // 带上**当前**的修订号 ⇒ 接受 ⇒ 再 +1。
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: Some(1),
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(state.revision, 2, "带上正确的号 ⇒ 接受 ⇒ 修订号 +1");
    assert_eq!(
        state.description,
        user_board(),
        "接受之后生效的图逐字等于提交的那份"
    );
}

// ---------------------------------------------------------------------------
// B-4 / B-5 / B-8 / B-9 / B-10 · 差量
// ---------------------------------------------------------------------------

/// **B-4** —— `PatchMemoryMap`（改一段的 size）⇒ 合成后**生效图**的那一段
/// **逐字段**等于新值 ⇒ **其它段一字未动**。
#[test]
fn b4_patching_one_region_size_changes_only_that_region() {
    let mut host = KernelHost::new();
    let board = user_board();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::UpsertRegion {
                spec: RegionSpec::Ram {
                    base: 0x0000_0000,
                    size: 0x0002_0000,
                },
            }],
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(state.revision, 2, "差量被接受 ⇒ 修订号 +1");

    // 那一段**逐字段**等于新值。
    let patched = state
        .description
        .regions
        .iter()
        .find(|region| region.base() == 0x0000_0000)
        .expect("低段 RAM 必须还在");
    assert_eq!(
        patched,
        &RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0002_0000,
        },
        "改过的那一段必须**逐字段**等于新值"
    );

    // ⚠️ **其它段一字未动**（逐条比，不是比段数）。
    let others: Vec<&RegionSpec> = state
        .description
        .regions
        .iter()
        .filter(|region| region.base() != 0x0000_0000)
        .collect();
    let before_others: Vec<&RegionSpec> = board
        .regions
        .iter()
        .filter(|region| region.base() != 0x0000_0000)
        .collect();
    assert_eq!(others, before_others, "没被改的段必须**逐字段**不变");
    assert_eq!(
        state.description.regions.len(),
        3,
        "改一段**不增不减**段（按 base 命中 ⇒ 替换，不是追加）"
    );
}

/// **B-5** —— `PatchMemoryMap`（删一段）⇒ 该段**消失** ⇒ 其余段**逐字段**
/// 不变。
#[test]
fn b5_patching_a_removal_drops_that_region_only() {
    let mut host = KernelHost::new();
    let board = user_board();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::RemoveRegion { base: 0x5000_0000 }],
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);

    assert!(
        !state
            .description
            .regions
            .iter()
            .any(|region| region.base() == 0x5000_0000),
        "被删的那一段必须消失：{:?}",
        state.description.regions
    );
    assert_eq!(state.description.regions.len(), 2, "删一段 ⇒ 段数减一");
    let kept: Vec<&RegionSpec> = state
        .description
        .regions
        .iter()
        .filter(|region| region.base() != 0x5000_0000)
        .collect();
    let before_kept: Vec<&RegionSpec> = board
        .regions
        .iter()
        .filter(|region| region.base() != 0x5000_0000)
        .collect();
    assert_eq!(kept, before_kept, "其余段必须**逐字段**不变");
}

/// **B-8** —— `PatchMemoryMap`（把一段改大到与邻段重叠）⇒ **明确拒**
/// （`E_PLT-06`）⇒ **什么都不动** ⇒ 台账判别力 ④。
///
/// ⚠️ 「改大到与邻段重叠」：低段 RAM（`0x0000_0000` / 64 KiB）改大到
/// `0x5000_1000` 字节 ⇒ 它就**盖住**了 `0x5000_0000` 那段 ROM ⇒ V-6。
#[test]
fn b8_patching_a_region_into_an_overlap_is_rejected_and_changes_nothing() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let before = current_state(&mut host);

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::UpsertRegion {
                spec: RegionSpec::Ram {
                    base: 0x0000_0000,
                    size: 0x5000_1000,
                },
            }],
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-06",
        "重叠必须是 E_PLT-06（V-6）：{events:?}"
    );

    let after = current_state(&mut host);
    assert_eq!(
        after.description, before.description,
        "被拒 ⇒ 生效的图不许变"
    );
    assert!(!after.pending, "被拒 ⇒ 用户的板不许被换掉");
    assert_eq!(after.revision, before.revision, "被拒 ⇒ 修订号不许变");
}

/// **B-9** —— 差量要删的那一段**不存在** ⇒ **`E_PLT-14`** ⇒ **什么都不动**。
#[test]
fn b9_removing_a_region_that_does_not_exist_is_rejected_and_changes_nothing() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let before = current_state(&mut host);

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::RemoveRegion { base: 0xDEAD_0000 }],
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-14",
        "删不存在的段必须是 E_PLT-14（族 PLT / 码 14）：{events:?}"
    );

    let after = current_state(&mut host);
    assert_eq!(
        after.description, before.description,
        "被拒 ⇒ 生效的图不许变"
    );
    assert!(!after.pending, "被拒 ⇒ 用户的板不许被换掉");
    assert_eq!(after.revision, before.revision, "被拒 ⇒ 修订号不许变");
}

/// **B-10** —— `PatchMemoryMap` 差量为**空** ⇒ 接受（合成结果 = 原图）⇒
/// 修订号 **+1**。
///
/// ⚠️ 前端**不会**发空差量，但宿主**必须**能接住（冻结规格 §5 B-10）。
#[test]
fn b10_an_empty_patch_is_accepted_and_still_bumps_the_revision() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let before = current_state(&mut host);

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: Vec::new(),
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(
        state.description, before.description,
        "空差量 ⇒ 合成结果 = 原图（**逐字段**不变）"
    );
    assert_eq!(
        state.revision,
        before.revision + 1,
        "空差量**照样**被接受 ⇒ 修订号 +1"
    );
}

/// **B-4/B-5 的补充** —— 差量的**基底是当前生效的图**（裁决 I）⇒ 差量
/// **按 `base` 定位 ⇒ 与数组顺序无关**（台账判别力 ③）。
///
/// ⚠️ 判据：把描述里的段**重排**（数组顺序变了）⇒ 差量仍然命中**同一条**
/// ⇒ 合成结果**逐字段**与「重排后按 base 定位」的那一份相同。
#[test]
fn b4_the_delta_is_located_by_base_not_by_array_order() {
    let mut host = KernelHost::new();
    // 一张**段顺序被打乱**的板（内容与 `user_board()` 相同，只是顺序不同）。
    let mut shuffled = user_board();
    shuffled.regions.rotate_left(1);
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: shuffled.clone(),
            expected_revision: None,
        },
    );

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::UpsertRegion {
                spec: RegionSpec::Rom {
                    base: 0x5000_0000,
                    size: 0x0000_2000,
                },
            }],
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    let patched = state
        .description
        .regions
        .iter()
        .find(|region| region.base() == 0x5000_0000)
        .expect("ROM 必须在");
    assert_eq!(
        patched,
        &RegionSpec::Rom {
            base: 0x5000_0000,
            size: 0x0000_2000,
        },
        "按 base 定位 ⇒ 命中**同一条**（与数组顺序无关）"
    );
    assert_eq!(
        state.description.regions.len(),
        3,
        "命中 ⇒ **替换**，不是追加"
    );
}

/// **差量的合成是原子的**（裁决 J）—— 一串操作里**后一条**失败 ⇒ **前面
/// 那几条也不许生效**（不许半张图）。
#[test]
fn b4_a_failing_op_in_the_middle_rolls_the_whole_patch_back() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let before = current_state(&mut host);

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![
                MemoryMapOp::UpsertRegion {
                    spec: RegionSpec::Ram {
                        base: 0x0000_0000,
                        size: 0x0003_0000,
                    },
                },
                MemoryMapOp::RemoveRegion { base: 0xDEAD_0000 },
            ],
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-14",
        "后一条失败 ⇒ 报 E_PLT-14：{events:?}"
    );

    let after = current_state(&mut host);
    assert_eq!(
        after.description, before.description,
        "原子 ⇒ 前一条**也不许**生效（不能留半张图）"
    );
    assert_eq!(after.revision, before.revision, "被拒 ⇒ 修订号不许变");
}

// ---------------------------------------------------------------------------
// B-6 / B-7 · 运行期上界（声明期 / 运行期两层护栏）
// ---------------------------------------------------------------------------

/// **B-6** —— 描述里 `max_region_bytes` 给一个**比声明期小**的值 ⇒
/// **写入**超限 ⇒ **`E_BUS-09`** ⇒ **两层护栏能分别触发**（裁决 K）。
///
/// ⚠️ **口径**：声明期那条（V-7 / `E_PLT-07`）判的是**声明的 size**；
/// 运行期那条（`E_BUS_09`）判的是**物理占用**（真正落地的页）⇒ 这份板
/// 的声明 size（64 KiB）**远小于**声明期预算（16 MiB）⇒ 声明期那条
/// **放行** ⇒ 到了运行期，写一下就**超限** ⇒ 两个码**各归各的**。
#[test]
fn b6_a_smaller_runtime_cap_makes_a_write_hit_the_bus_guard() {
    let mut host = KernelHost::new();
    // 运行期上界 = **1 字节**（声明期上界 = 16 MiB ⇒ 明显更小）。
    let board = ram_only_board(Some(1));

    // ⚠️ **声明期那条放行**（V-7：64 KiB ≤ 16 MiB）⇒ 板被接受。
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(
        state.description, board,
        "声明期护栏**放行**（V-7 判的是声明 size）"
    );

    // ⚠️ **运行期那条拦下**：写一下就要落地一整页（4 KiB）⇒ 超过 1 字节。
    let err = host
        .bus_mut()
        .write(0x0000_0000, BusWidth::Word, 0x1234_5678)
        .expect_err("超过运行期上界的写必须被拒");
    assert_eq!(
        err.family,
        ErrorFamily::Bus,
        "运行期那条是总线侧的错（族 BUS）：{err:?}"
    );
    assert_eq!(err.scenario, 9, "E_BUS-09（场景码 9）");
}

/// **B-6（负向半）** —— **不给**运行期上界 ⇒ 同样的写**成功** ⇒ 两层
/// 护栏**分别**触发（不是「一起触发」）。
#[test]
fn b6_without_a_runtime_cap_the_same_write_succeeds() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: ram_only_board(None),
            expected_revision: None,
        },
    );
    host.bus_mut()
        .write(0x0000_0000, BusWidth::Word, 0x1234_5678)
        .expect("没有运行期上界 ⇒ 同样的写必须成功");
    assert_eq!(
        host.read_memory(0x0000_0000, WriteSize::Word)
            .expect("读回刚写的那个字"),
        0x1234_5678,
        "内容必须真的写进去了"
    );
}

/// **B-7** —— 描述里**不给** `max_region_bytes` ⇒ `bus.max_region_bytes() ==
/// region_budget_bytes` ⇒ **既有断言一条不改**。
#[test]
fn b7_without_a_runtime_cap_the_runtime_bound_equals_the_declared_budget() {
    let mut host = KernelHost::new();
    let board = user_board();
    assert_eq!(board.max_region_bytes, None, "这份板**不给**运行期上界");
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    assert_eq!(
        host.bus().max_region_bytes(),
        u64::from(board.region_budget_bytes),
        "不给运行期上界 ⇒ 运行期上界 = **声明期**那个"
    );
}

/// **B-7（给了的时候）** —— 给了运行期上界 ⇒ 总线上就是**那个**值（不是
/// 声明期的那个）⇒ 两层真的**各管各的**。
#[test]
fn b7_a_given_runtime_cap_is_the_one_the_bus_uses() {
    let mut host = KernelHost::new();
    let board = ram_only_board(Some(0x2000));
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    assert_eq!(
        host.bus().max_region_bytes(),
        0x2000,
        "给了运行期上界 ⇒ 总线上就是那个值"
    );
    assert_ne!(
        host.bus().max_region_bytes(),
        u64::from(board.region_budget_bytes),
        "它**不等于**声明期那个 ⇒ 两层不是同一个东西"
    );
}

// ---------------------------------------------------------------------------
// B-11 / B-12 / B-13 · 卸载
// ---------------------------------------------------------------------------

/// **B-11** —— `UnloadProject` ⇒ `image_loaded` 变假 ⇒ **之后改板立即
/// 生效**（事件 `pending == false` 且**生效图**逐字段等于新板）⇒ 台账
/// D-139 判别力 ⑤。
#[test]
fn b11_after_unloading_a_board_change_takes_effect_immediately() {
    let mut host = KernelHost::new();
    // 装载 ⇒ 之后改板只能「待生效」。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    assert!(current_state(&mut host).pending, "已装载 ⇒ 改板只能待生效");

    // 卸载。
    let events = dispatch(&mut host, CommandPayload::UnloadProject);
    let state = first_memory_map(&events);
    assert!(!state.loaded, "卸载 ⇒ `loaded` 变假：{events:?}");
    assert_eq!(state.revision, 1, "卸载**不 +1**（卸载不改图）");

    // ⇒ 之后的改板**立即生效**。
    let board = ram_only_board(None);
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let state = first_memory_map(&events);
    assert_eq!(
        state.description, board,
        "卸载之后 ⇒ 改板**立即生效**（生效图逐字段等于新板）"
    );
    assert!(!state.pending, "立即生效 ⇒ 没有待生效的");
    assert_eq!(state.revision, 2, "改板被接受 ⇒ 修订号 +1");
}

/// **B-12** —— `UnloadProject` ⇒ 用户的板**留着** ⇒ 镜像**留着** ⇒
/// 「重新装载」不必重新选文件。
#[test]
fn b12_unloading_keeps_the_user_board_and_the_mirror_bytes() {
    let mut host = KernelHost::new();
    let project = demo_project();
    dispatch(&mut host, CommandPayload::LoadProject(project.clone()));
    let board = user_board();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );

    let events = dispatch(&mut host, CommandPayload::UnloadProject);
    let state = first_memory_map(&events);

    // ⚠️ **用户的板留着** ⇒ 生效的图还是旧的（用户的板还没生效）⇒
    // `pending` 仍是**真**。
    assert!(
        state.pending,
        "用户的板留着 ⇒ 它还没生效 ⇒ 待生效为真：{events:?}"
    );
    assert_eq!(
        state.description.id, "default",
        "用户的板留着 ⇒ 生效的图**没变**"
    );

    // ⚠️ **镜像字节留着** ⇒ 宿主**记得**最后一个装载成功的工程。
    let last = host.last_project().expect("卸载**不许**清掉最后一个工程");
    assert_eq!(last, &project, "镜像字节留在宿主 ⇒ 重新装载不必重新选文件");
}

/// **B-13** —— `UnloadProject` ⇒ 宿主的主题集合**清空**（订阅失效）⇒
/// 前端**必须重订**。
#[test]
fn b13_unloading_clears_the_topic_set() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_REGISTERS.to_string(),
        },
    );
    assert_eq!(host.topics.len(), 2, "两条订阅都在（前置条件）");

    dispatch(&mut host, CommandPayload::UnloadProject);
    assert!(
        host.topics.is_empty(),
        "卸载 ⇒ 订阅全部失效（前端必须重订）：{:?}",
        host.topics
    );

    // ⚠️ 重订之后**又能**收到 ⇒ 「失效」不是「坏了」。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let state = first_memory_map(&events);
    assert!(
        !state.loaded,
        "重订 ⇒ 推来的图带着 `loaded = false`（会话已卸空）"
    );
}

/// **卸载的回事件不是空数组** —— 前端 M-05 靠**第一个**带 `reqId` 的事件
/// 落地 Promise ⇒ 返回空数组会让前端**永远等不到**。
#[test]
fn b13_the_unload_reply_is_not_empty() {
    let mut host = KernelHost::new();
    let events = dispatch(&mut host, CommandPayload::UnloadProject);
    assert!(
        !events.is_empty(),
        "卸载必须回**至少一条**事件（前端靠它落地 Promise）：{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::MemoryMap { .. })),
        "卸载的回事件里必须有 MemoryMap（带生效的图 + pending + loaded + revision）：{events:?}"
    );
}

/// **卸载清的是运行态** —— 寄存器 / 事件队列 / 断点 / 观察点。
#[test]
fn b11_unloading_clears_the_running_state() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 3 });
    assert_ne!(host.pc(), 0, "跑了几条指令 ⇒ PC 不为零（前置条件）");

    dispatch(&mut host, CommandPayload::UnloadProject);
    assert_eq!(host.pc(), 0, "卸载 ⇒ 寄存器归零（PC 回 0）");
    assert_eq!(host.now_ps(), 0, "卸载 ⇒ 虚拟时钟归零（事件队列也清了）");
}

// ---------------------------------------------------------------------------
// B-14 · 装载工程时**待生效的板被应用**
// ---------------------------------------------------------------------------

/// **B-14** —— 装载工程时**待生效的板被应用** ⇒ 生效图**逐字段**等于那块
/// 板 ⇒ 修订号 **+1** ⇒ `pending == false`。
#[test]
fn b14_loading_applies_the_pending_board_and_bumps_the_revision() {
    let mut host = KernelHost::new();
    // 先装载一次 ⇒ 之后的改板只能「待生效」。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let before = current_state(&mut host);
    assert_eq!(before.revision, 0, "还没改过板 ⇒ 修订号 0");
    assert!(!before.pending, "从未配过板 ⇒ 没有待生效的");

    // 配一块板（已装载 ⇒ 待生效）⇒ 修订号 +1。
    let board = user_board();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let pending_state = current_state(&mut host);
    assert_eq!(pending_state.revision, 1, "改板被接受 ⇒ 修订号 +1");
    assert!(pending_state.pending, "已装载 ⇒ 改板只能待生效");

    // 重新装载 ⇒ **待生效的板被应用** ⇒ 生效图 = 那块板 ⇒ 修订号再 +1。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let after = current_state(&mut host);
    assert_eq!(
        after.description, board,
        "待生效的板被应用 ⇒ 生效图**逐字段**等于那块板"
    );
    assert!(
        !after.pending,
        "板被应用了 ⇒ 没有待生效的（`pending == false`）"
    );
    assert_eq!(after.revision, 2, "待生效的板被应用 ⇒ 修订号 **+1**");
    assert_eq!(
        host.platform_id(),
        "user-board",
        "生效的图必须是用户配的那块板"
    );
}

/// **B-14（负向半）** —— **没有**待生效的板时装载 ⇒ 走**自动选档** ⇒ 修订号
/// **不 +1**（裁决 A 只把「待生效的板被应用」算作一次变更）。
#[test]
fn b14_loading_without_a_pending_board_does_not_bump_the_revision() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let state = current_state(&mut host);
    assert_eq!(
        state.revision, 0,
        "自动选档**不是**「待生效的板被应用」⇒ 修订号不 +1"
    );
    assert_eq!(host.platform_id(), "default", "走自动选档");
}

// ---------------------------------------------------------------------------
// 订阅即推送（§3.4）· 修订号自动刷新
// ---------------------------------------------------------------------------

/// **§3.4** —— `Subscribe { topic: "memory.map" }` 的即时推送**带
/// `loaded` 与 `revision`** ⇒ 前端**重订之后**修订号自动刷新 ⇒ **不会
/// 误报冲突**。
#[test]
fn b3_the_memory_map_push_carries_the_loaded_flag_and_the_revision() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );

    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let state = first_memory_map(&events);
    assert!(state.loaded, "推送必须带 `loaded`（已装载）");
    assert_eq!(
        state.revision, 1,
        "推送必须带 `revision` ⇒ 前端重订之后**不会误报冲突**"
    );
}
