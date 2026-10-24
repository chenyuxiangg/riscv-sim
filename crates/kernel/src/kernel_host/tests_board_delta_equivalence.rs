//! Lane A · 宿主侧的**板级内存图 · 差量与整份两条路径的等价性**
//! （v0.32 · STAGE-17 §3 + §5 · 验收 **B-1 ~ B-7** · 台账 **D-145** 闭环）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-17_V032_Delta_Equivalence_Ordering_Gate.md`
//! §1（裁决 A~N）· §3（内核宿主）· §5（B 组验收）。
//!
//! ## 这个文件钉住的是**宿主**这一段的七件事
//!
//! | 验收 | 一句话 |
//! | --- | --- |
//! | **B-1** | `compose_memory_map` 是**纯函数** ⇒ 同样的输入两次调用结果相同 ⇒ 且**不碰**宿主状态 |
//! | **B-2** | 差量路径**仍然**是「合成 → 整体校验 → 一次性应用」 |
//! | **B-3** | **等价性**：同一组改动（改一段 + 删一段 + 改预算）⇒ 两条路径的**生效图逐字段相同** |
//! | **B-4** | 等价性覆盖**三种改动** ⇒ 三种**分别**验 |
//! | **B-5** | 差量里含**非法**操作 ⇒ 差量路径**拒** ⇒ 整份路径**也拒** ⇒ ⇒ **两边都什么都不动** |
//! | **B-6** | 差量为**空** ⇒ 合成结果**逐字段等于**基座 ⇒ ⇒ 与整份提交同一张图**等价** |
//! | **B-7** | `compose` 失败 ⇒ 宿主的图 / 用户的板 / 修订号**三者都不动** |
//!
//! ⚠️ **断言值一律写架构字面量**（`"E_PLT-06"` / `"E_PLT-14"` / `0x5000_0000`
//! / `0x0100_0000` …）—— 从被测实现反推出来的断言是**假绿**。
//!
//! ⚠️ **既有断言一条不改**：v0.31 批的那 24 条在
//! [`tests_board_delta_unload.rs`](super::tests_board_delta_unload) 里 ⇒ 本文件
//! **另起一个** ⇒ 「纯函数化是**等价重构**」这件事由「那边**一条不改**仍绿」
//! **证明**（§6 的破坏性变更那张表）。
//!
//! ⚠️ **ops 必须手写**（规格 §3.3）⇒ ⇒ 本文件**不复制**前端的差量算法
//! ⇒ ⇒ 它验的是**内核的合成逻辑** ⇒ ⇒ 「前端算出来的 ops 对不对」由真机层
//! （裁决 C 的第二层）验。
//!
//! ⚠️ **「逐字段相同」用规范形式比**（裁决 B）：① 对象**键排序** ⇒ 键序
//! 不进判据（顺序是**线上形状**的事，不是**图的内容**的事）② `regions`
//! 数组**按基址排序** ⇒ 段的**列表顺序**也不进判据（两条路径各自的段序
//! 天然不同：整份路径用调用方给的顺序、差量路径保留基座的顺序）⇒ ⇒ 段的
//! **集合**与每一段的**每一个字段**仍然被**逐条**比。

use super::*;

use riscv_sim_contract::platform::{DeviceSpec, RegionSpec, PLATFORM_SCHEMA_VERSION};
use riscv_sim_contract::protocol::MemoryMapOp;

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

/// 一张「用户配的板」—— 与 v0.31 那张**逐字同形**。
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

/// **基座** —— 两个宿主共用的那张板（规格 §3.3 ①「同一张 user_board」）。
fn base_board() -> PlatformDescription {
    user_board()
}

// ---------------------------------------------------------------------------
// 规范形式（键序无关 —— 裁决 B）
// ---------------------------------------------------------------------------

/// 一张描述的**规范形式** ⇒ 用来比「逐字段相同」。
///
/// ⚠️ **键序无关**：对象键**递归排序** ⇒ 键的顺序**不进判据**。
///
/// ⚠️ **段序不进判据**：`regions` 数组**按基址排序** ⇒ 两条路径各自的段
/// 列表顺序（整份路径 = 调用方给的顺序、差量路径 = 基座的顺序）**不会**
/// 让等价性判红 ⇒ 而段的**集合**与每一段的**每一个字段**仍然被**逐条**比。
fn canonical(description: &PlatformDescription) -> String {
    let value = serde_json::to_value(description).expect("描述必须能序列化成 JSON");
    let canonical = canonical_value(value);
    serde_json::to_string(&canonical).expect("规范形式必须能序列化成字符串")
}

/// 递归地把 JSON 值变成**规范形式**（对象键排序 + `regions` 按基址排序）。
fn canonical_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                let mut item = canonical_value(map.get(&key).cloned().unwrap_or_default());
                if key == "regions" {
                    if let serde_json::Value::Array(items) = &mut item {
                        items.sort_by_key(|item| {
                            item.get("base").and_then(|base| base.as_u64()).unwrap_or(0)
                        });
                    }
                }
                out.insert(key, item);
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonical_value).collect())
        }
        other => other,
    }
}

/// 宿主的**三个可观测量** —— 「什么都没动」就靠这三个一起看。
///
/// ⚠️ ① 生效的图 = `self.platform` ② 用户的板 = `self.user_board`
/// ③ 修订号 = `self.map_revision`。
#[derive(Debug, Clone, PartialEq, Eq)]
struct HostSnapshot {
    effective: PlatformDescription,
    user_board: Option<PlatformDescription>,
    revision: u64,
}

/// 拍一张宿主状态的快照。
fn snapshot(host: &KernelHost) -> HostSnapshot {
    HostSnapshot {
        effective: host.platform.clone(),
        user_board: host.user_board.clone(),
        revision: host.map_revision(),
    }
}

/// 断言两张快照**逐字段相同**（规范形式）。
fn assert_same_snapshot(before: &HostSnapshot, after: &HostSnapshot, what: &str) {
    assert_eq!(
        canonical(&after.effective),
        canonical(&before.effective),
        "{what}：生效的图不许变"
    );
    assert_eq!(
        after.user_board.as_ref().map(canonical),
        before.user_board.as_ref().map(canonical),
        "{what}：用户的板不许变"
    );
    assert_eq!(after.revision, before.revision, "{what}：修订号不许变");
}

/// 造一个宿主，并把**同一张板**配上去（⇒ 两个宿主的**基座相同**）。
fn host_with_board(board: &PlatformDescription) -> KernelHost {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    // ⚠️ 配板必须**被接受**（否则这个夹具本身就不成立）。
    assert_eq!(
        first_memory_map(&events).revision,
        1,
        "配板必须被接受（否则夹具不成立）：{events:?}"
    );
    host
}

/// 走**整份路径**（`SetMemoryMap`）⇒ 返回**生效图**。
fn apply_whole(host: &mut KernelHost, target: &PlatformDescription) -> PlatformDescription {
    let events = dispatch(
        host,
        CommandPayload::SetMemoryMap {
            description: target.clone(),
            expected_revision: None,
        },
    );
    first_memory_map(&events).description
}

/// 走**差量路径**（`PatchMemoryMap`）⇒ 返回**生效图**。
fn apply_delta(host: &mut KernelHost, ops: &[MemoryMapOp]) -> PlatformDescription {
    let events = dispatch(
        host,
        CommandPayload::PatchMemoryMap {
            ops: ops.to_vec(),
            expected_revision: None,
        },
    );
    first_memory_map(&events).description
}

// ---------------------------------------------------------------------------
// B-1 · `compose_memory_map` 是**纯函数**
// ---------------------------------------------------------------------------

/// **B-1** —— 同样的 `base` + 同样的 `ops` ⇒ 两次调用**逐字段相同**；
/// 且调用前后宿主的图 / 用户的板 / 修订号**三者都不变**。
#[test]
fn b1_compose_is_a_pure_function() {
    let host = host_with_board(&base_board());
    let before = snapshot(&host);

    // ⚠️ **ops 手写**（规格 §3.3）⇒ 不复制前端的差量算法。
    let ops = vec![
        MemoryMapOp::UpsertRegion {
            spec: RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0002_0000,
            },
        },
        MemoryMapOp::RemoveRegion { base: 0x5000_0000 },
        MemoryMapOp::SetBudget {
            region_budget_bytes: 0x0080_0000,
            max_region_bytes: Some(0x0040_0000),
        },
    ];

    let first = compose_memory_map(&host.platform, &ops).expect("合成必须成功");
    let second = compose_memory_map(&host.platform, &ops).expect("合成必须成功");

    assert_eq!(
        canonical(&first),
        canonical(&second),
        "同样的 base + 同样的 ops ⇒ 两次调用**逐字段相同**"
    );
    assert_ne!(
        canonical(&first),
        canonical(&host.platform),
        "⚠️ **对照**：这一组 ops 真的改了图 ⇒ 否则「两次相同」是**假绿**（什么都没变）"
    );

    // ⚠️ **纯** = 不碰宿主任何状态（图 / 用户的板 / 修订号三者都不变）。
    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "调用纯函数之后");
}

// ---------------------------------------------------------------------------
// B-2 · 差量路径**仍然**是「合成 → 整体校验 → 一次性应用」
// ---------------------------------------------------------------------------

/// **B-2（整体校验）** —— **两条 op 合起来**才重叠（单看每一条都合法）⇒
/// 被拒（`E_PLT-06`）⇒ ⇒ 证明校验判的是**整张合成出来的图**，**不是**逐条
/// 判（逐条判会漏掉这一种）。
#[test]
fn b2_the_delta_path_validates_the_whole_composed_map() {
    let mut host = host_with_board(&base_board());
    let before = snapshot(&host);

    // ⚠️ 第一条：`0x1000_0000` 起 4 KiB —— 与基座的任何一段都不重叠。
    // ⚠️ 第二条：`0x1000_0800` 起 4 KiB —— 与基座的任何一段都不重叠，
    //    **但与第一条重叠** ⇒ 只有**整图**校验才看得见。
    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![
                MemoryMapOp::UpsertRegion {
                    spec: RegionSpec::Ram {
                        base: 0x1000_0000,
                        size: 0x0000_1000,
                    },
                },
                MemoryMapOp::UpsertRegion {
                    spec: RegionSpec::Ram {
                        base: 0x1000_0800,
                        size: 0x0000_1000,
                    },
                },
            ],
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-06",
        "两条 op 合起来才重叠 ⇒ 必须被**整图**校验拦下：{events:?}"
    );

    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "被拒之后");
}

/// **B-2（一次性应用）** —— 一串操作里**后一条**失败 ⇒ **前面那一条也不许
/// 生效**（不许半张图）。
#[test]
fn b2_the_delta_path_applies_the_result_in_one_go() {
    let mut host = host_with_board(&base_board());
    let before = snapshot(&host);

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

    // ⚠️ **前面那一条也不许生效**（不能留半张图）。
    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "被拒之后");
    let ram = after
        .effective
        .regions
        .iter()
        .find(|region| region.base() == 0x0000_0000)
        .expect("低段 RAM 必须还在");
    assert_eq!(
        ram,
        &RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0001_0000,
        },
        "**一次性应用** ⇒ 前面那条合法的 op 也不许生效"
    );
}

/// **B-2（次序）** —— **合成排在校验之前** ⇒ 合成失败时报的是**合成的错**
/// （`E_PLT-14`），**不是**整图的错（`E_PLT-06`）。
///
/// ⚠️ 这一条钉的是**报哪一个错** ⇒ ⇒ 「逐条判」的实现会在**第一条**就报
/// `E_PLT-06` ⇒ ⇒ 本条会红。
#[test]
fn b2_the_composition_comes_before_the_whole_map_validation() {
    let mut host = host_with_board(&base_board());
    let before = snapshot(&host);

    let events = dispatch(
        &mut host,
        CommandPayload::PatchMemoryMap {
            ops: vec![
                // ⚠️ 这条单独看会让整图重叠（V-6 ⇒ `E_PLT-06`）。
                MemoryMapOp::UpsertRegion {
                    spec: RegionSpec::Ram {
                        base: 0x0000_0000,
                        size: 0x5000_1000,
                    },
                },
                // ⚠️ 这条**合成时**就失败（`E_PLT-14`）。
                MemoryMapOp::RemoveRegion { base: 0xDEAD_0000 },
            ],
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-14",
        "合成排在校验之前 ⇒ 报**合成**的错，不是整图的错：{events:?}"
    );

    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "被拒之后");
}

// ---------------------------------------------------------------------------
// B-3 / B-4 · 两条路径的**等价性**
// ---------------------------------------------------------------------------

/// 目标图 —— **基座 + 三种改动**（改一段 + 删一段 + 改预算）。
///
/// ⚠️ **手写**（规格 §3.3）⇒ 它是「用户想要的那张板」，不是「算出来的」。
fn target_with_three_changes() -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "user-board".to_string(),
        name: "User board".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ 改预算：声明期 16 MiB → 8 MiB。
        region_budget_bytes: 0x0080_0000,
        // ⚠️ 改预算：运行期上界 缺省 → 4 MiB。
        max_region_bytes: Some(0x0040_0000),
        regions: vec![
            // ⚠️ 改一段：低段 RAM 64 KiB → 128 KiB。
            RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0002_0000,
            },
            // ⚠️ 删一段：`0x5000_0000` 那段 ROM 没了。
            RegionSpec::Peripheral {
                base: 0x6000_0000,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
        ],
    }
}

/// 表达**同一组改动**的差量（**手写** —— 规格 §3.3）。
fn three_change_ops() -> Vec<MemoryMapOp> {
    vec![
        // ⚠️ 改一段：低段 RAM 64 KiB → 128 KiB。
        MemoryMapOp::UpsertRegion {
            spec: RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0002_0000,
            },
        },
        // ⚠️ 删一段：`0x5000_0000` 那段 ROM。
        MemoryMapOp::RemoveRegion { base: 0x5000_0000 },
        // ⚠️ 改预算：两个字段一起改。
        MemoryMapOp::SetBudget {
            region_budget_bytes: 0x0080_0000,
            max_region_bytes: Some(0x0040_0000),
        },
    ]
}

/// **B-3** —— 同一组改动（改一段 + 删一段 + 改预算）⇒ **整份路径**与
/// **差量路径** ⇒ **生效图逐字段相同**。
#[test]
fn b3_the_two_paths_produce_the_same_effective_map() {
    let base = base_board();
    let target = target_with_three_changes();
    let ops = three_change_ops();

    // ⚠️ **两个宿主 · 同一基座**（规格 §3.3 ①）。
    let mut host_whole = host_with_board(&base);
    let mut host_delta = host_with_board(&base);

    let effective_whole = apply_whole(&mut host_whole, &target);
    let effective_delta = apply_delta(&mut host_delta, &ops);

    assert_eq!(
        canonical(&effective_whole),
        canonical(&effective_delta),
        "两条路径的**生效图**必须**逐字段相同**"
    );
    assert_eq!(
        canonical(&effective_whole),
        canonical(&target),
        "整份路径的生效图必须**逐字段等于**目标图"
    );
    assert_eq!(
        canonical(&effective_delta),
        canonical(&target),
        "差量路径的生效图必须**逐字段等于**目标图"
    );

    // ⚠️ **对照**：这一组改动**真的**改到了图 ⇒ 否则「两条路径相同」是
    // **假绿**（两边都没变）。
    assert_ne!(
        canonical(&effective_delta),
        canonical(&base),
        "差量**真的**生效了（不是两边都没变）"
    );
}

/// **B-4（改一段）** —— 只改**一段** ⇒ 两条路径的生效图**逐字段相同**。
#[test]
fn b4_equivalence_holds_for_changing_one_region() {
    let base = base_board();
    let mut target = base.clone();
    target.regions[0] = RegionSpec::Ram {
        base: 0x0000_0000,
        size: 0x0002_0000,
    };
    let ops = vec![MemoryMapOp::UpsertRegion {
        spec: RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0002_0000,
        },
    }];

    let mut host_whole = host_with_board(&base);
    let mut host_delta = host_with_board(&base);

    let effective_whole = apply_whole(&mut host_whole, &target);
    let effective_delta = apply_delta(&mut host_delta, &ops);

    assert_eq!(
        canonical(&effective_whole),
        canonical(&effective_delta),
        "「改一段」⇒ 两条路径的生效图必须**逐字段相同**"
    );
    assert_eq!(
        canonical(&effective_delta),
        canonical(&target),
        "「改一段」⇒ 生效图必须**逐字段等于**目标图"
    );
}

/// **B-4（删一段）** —— 只**删一段** ⇒ 两条路径的生效图**逐字段相同**。
#[test]
fn b4_equivalence_holds_for_removing_one_region() {
    let base = base_board();
    let mut target = base.clone();
    target.regions.retain(|region| region.base() != 0x5000_0000);
    let ops = vec![MemoryMapOp::RemoveRegion { base: 0x5000_0000 }];

    let mut host_whole = host_with_board(&base);
    let mut host_delta = host_with_board(&base);

    let effective_whole = apply_whole(&mut host_whole, &target);
    let effective_delta = apply_delta(&mut host_delta, &ops);

    assert_eq!(
        canonical(&effective_whole),
        canonical(&effective_delta),
        "「删一段」⇒ 两条路径的生效图必须**逐字段相同**"
    );
    assert_eq!(
        canonical(&effective_delta),
        canonical(&target),
        "「删一段」⇒ 生效图必须**逐字段等于**目标图"
    );
    assert!(
        !effective_delta
            .regions
            .iter()
            .any(|region| region.base() == 0x5000_0000),
        "「删一段」⇒ 那段必须真的没了：{:?}",
        effective_delta.regions
    );
}

/// **B-4（改预算）** —— 只**改预算** ⇒ 两条路径的生效图**逐字段相同**。
#[test]
fn b4_equivalence_holds_for_changing_the_budget() {
    let base = base_board();
    let mut target = base.clone();
    target.region_budget_bytes = 0x0080_0000;
    target.max_region_bytes = Some(0x0040_0000);
    let ops = vec![MemoryMapOp::SetBudget {
        region_budget_bytes: 0x0080_0000,
        max_region_bytes: Some(0x0040_0000),
    }];

    let mut host_whole = host_with_board(&base);
    let mut host_delta = host_with_board(&base);

    let effective_whole = apply_whole(&mut host_whole, &target);
    let effective_delta = apply_delta(&mut host_delta, &ops);

    assert_eq!(
        canonical(&effective_whole),
        canonical(&effective_delta),
        "「改预算」⇒ 两条路径的生效图必须**逐字段相同**"
    );
    assert_eq!(
        canonical(&effective_delta),
        canonical(&target),
        "「改预算」⇒ 生效图必须**逐字段等于**目标图"
    );
    // ⚠️ **两个预算字段一起改**（声明期 + 运行期）。
    assert_eq!(effective_delta.region_budget_bytes, 0x0080_0000);
    assert_eq!(effective_delta.max_region_bytes, Some(0x0040_0000));
}

// ---------------------------------------------------------------------------
// B-5 · 非法操作 ⇒ 两条路径**都拒** ⇒ **都什么都不动**
// ---------------------------------------------------------------------------

/// **B-5（重叠）** —— 差量里含**非法**操作（改成与邻段重叠）⇒ **差量路径
/// 拒**（`E_PLT-06`）⇒ **整份路径也拒**（同样那张非法图 ⇒ `E_PLT-06`）⇒
/// ⇒ **两边都什么都不动**。
///
/// ⚠️ **判据是「都拒 + 都不动」，不是「码相同」**（裁决 D）—— 下面两处
/// `assert_eq!` 各自钉的是**各自那一条路径**的**架构字面量**；等价性的
/// 判据落在「两边都拒」与「两边都不动」这两件事上。
#[test]
fn b5_an_illegal_overlap_is_rejected_by_both_paths_and_changes_nothing() {
    let base = base_board();
    // ⚠️ **同样那张非法图**：低段 RAM 改大到 `0x5000_1000` 字节 ⇒ 它盖住了
    // `0x5000_0000` 那段 ROM（V-6 ⇒ `E_PLT-06`）。
    let mut illegal_target = base.clone();
    illegal_target.regions[0] = RegionSpec::Ram {
        base: 0x0000_0000,
        size: 0x5000_1000,
    };
    let illegal_ops = vec![MemoryMapOp::UpsertRegion {
        spec: RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x5000_1000,
        },
    }];

    // ---- 差量路径 ----
    let mut host_delta = host_with_board(&base);
    let before_delta = snapshot(&host_delta);
    let events = dispatch(
        &mut host_delta,
        CommandPayload::PatchMemoryMap {
            ops: illegal_ops,
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-06",
        "差量路径：重叠必须是 E_PLT-06（V-6）：{events:?}"
    );
    let after_delta = snapshot(&host_delta);
    assert_same_snapshot(&before_delta, &after_delta, "差量路径被拒之后");

    // ---- 整份路径 ----
    let mut host_whole = host_with_board(&base);
    let before_whole = snapshot(&host_whole);
    let events = dispatch(
        &mut host_whole,
        CommandPayload::SetMemoryMap {
            description: illegal_target,
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-06",
        "整份路径：同样那张非法图也必须被拒：{events:?}"
    );
    let after_whole = snapshot(&host_whole);
    assert_same_snapshot(&before_whole, &after_whole, "整份路径被拒之后");

    // ⚠️ **对照**：两条路径的**起点**确实相同（同一张基座）。
    assert_eq!(
        canonical(&before_delta.effective),
        canonical(&before_whole.effective),
        "两个宿主的基座必须**同一张**"
    );
}

/// **B-5（删不存在的段）** —— 差量里含**非法**操作（删一段**不存在**的）⇒
/// **差量路径拒**（`E_PLT-14`）⇒ **什么都不动**。
///
/// ⚠️ **整份路径没有「段不存在」这个概念**（裁决 D 的括号）⇒ 它无法表达
/// 「删一段不存在的」⇒ 它提交的那张图（= 基座那张，**合法**）⇒ **接受**
/// ⇒ ⇒ 这一支的判据落在「**差量侧**：拒 + 什么都不动」；而「**都拒 + 都不
/// 动**」那一条由上面的**重叠**场景承担（那里整份路径**也**能拒）。
#[test]
fn b5_a_removal_of_a_missing_region_is_rejected_and_changes_nothing() {
    let base = base_board();
    let mut host = host_with_board(&base);
    let before = snapshot(&host);

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

    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "差量路径被拒之后");
}

// ---------------------------------------------------------------------------
// B-6 · 差量为**空** ⇒ 合成结果 == 基座
// ---------------------------------------------------------------------------

/// **B-6** —— 差量为**空** ⇒ 合成结果**逐字段等于**基座 ⇒ ⇒ 与「整份提交
/// 同一张图」**等价**。
#[test]
fn b6_an_empty_delta_composes_to_the_base_and_equals_the_whole_path() {
    let base = base_board();

    // ---- 纯函数那一半：合成结果 == 基座 ----
    let composed = compose_memory_map(&base, &[]).expect("空差量必须合成成功");
    assert_eq!(
        canonical(&composed),
        canonical(&base),
        "空差量 ⇒ 合成结果**逐字段等于**基座"
    );

    // ---- 两条路径那一半：生效图相同 ----
    let mut host_whole = host_with_board(&base);
    let mut host_delta = host_with_board(&base);

    let effective_whole = apply_whole(&mut host_whole, &base);
    let effective_delta = apply_delta(&mut host_delta, &[]);

    assert_eq!(
        canonical(&effective_whole),
        canonical(&effective_delta),
        "空差量 ⇒ 与「整份提交同一张图」**等价**"
    );
    assert_eq!(
        canonical(&effective_delta),
        canonical(&base),
        "空差量 ⇒ 生效图**逐字段等于**基座"
    );
}

// ---------------------------------------------------------------------------
// B-7 · `compose` 失败 ⇒ 宿主**什么都不动**
// ---------------------------------------------------------------------------

/// **B-7** —— `compose` 失败 ⇒ 宿主的图 / 用户的板 / 修订号**三者都不动**
/// （既有语义**未改** —— 裁决 M）。
#[test]
fn b7_a_failed_composition_changes_nothing_in_the_host() {
    let mut host = host_with_board(&base_board());
    let before = snapshot(&host);

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
        "合成失败必须是 E_PLT-14：{events:?}"
    );

    // ⚠️ **三个可观测量**（图 / 用户的板 / 修订号）**都不动**。
    let after = snapshot(&host);
    assert_same_snapshot(&before, &after, "合成失败之后");
    assert_eq!(
        after.user_board.as_ref().map(canonical),
        Some(canonical(&base_board())),
        "用户的板还是**原来那张**（它是**用户意图**，失败的那次改板没有替换它）"
    );
}
