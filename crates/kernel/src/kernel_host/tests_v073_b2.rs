// v0.73 批 · STAGE-49 · **B2** —— `base` 可缺省 + **装配期分配**。
//
// ## 这一套盯住什么
//
// 规格 `docs/detailed-design/STAGE-49_V073_Platform_Schema_V2.md` §4 里与
// **B2** 有关的那几条：
//
// - **A-3**：`base` **缺省**的描述**能解析** ⇒ ⇒ 新格式**能用**；
// - **K-1**：**装配期分配** —— 一份**不带** `base` 的描述 ⇒ ⇒ 内核分配出
//   **确定**的基址 ⇒ ⇒ **两次装得到同一个图**；
// - **K-2**：⚠️ **必要对照** —— 分配结果**不等于**任何「按 base 升序」的
//   猜测 ⇒ ⇒ 分配**真的**发生着；
// - **K-3**：**老描述逐字不变** —— 带 `base` 的老描述 ⇒ ⇒ 装配结果与
//   升版前**逐字相同**；
// - **K-4**：**装不下 ⇒ 报既有码**（`E_PLT-07`）⇒ ⇒ **零新场景码**。
//
// ## ⚠️ **边界如实登记**
//
// ⚠️ **分配器在内核侧**（[`super::platform::assign_bases`]）—— 契约层**看不到**
//   外设的**窗口大小**（那是 **M-15 注册表**的事 · 裁决 D）⇒ ⇒ ⇒ 「分配 +
//   判重叠」**必须**在**同一处**（一个约束一个落点）。
//
// ⚠️ **分配结果 ≠ 用户选的地址** —— 缺 `base` 的段**由内核决定**落在哪 ⇒ ⇒
//   用户**明确写过**的基址**一个都不改**（⇒ ⇒ 「用户自己决定落在哪」那条
//   纪律**不被推翻**）。
//
// ⚠️ 断言值是**写死的字面量**，不从被测实现反推。

use riscv_sim_contract::error::{E_PLT_07_OVER_BUDGET, PLATFORM_FAMILY};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};

use super::platform;
use super::KernelHost;

/// 一份**缺 `base`** 的描述（两段 RAM：低段 + 高段）。
///
/// ⚠️ **声明顺序 = 低段在前**（⇒ ⇒ 「按声明顺序分配」与「按 base 升序」
///   在这份描述上**结果不同** ⇒ ⇒ ⇒ 这正是 **K-2** 的判别力来源）。
fn description_without_bases() -> PlatformDescription {
    PlatformDescription {
        schema_version: riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
        id: "auto-base".to_string(),
        name: "Auto base".to_string(),
        priority: 100,
        auto_selectable: false,
        region_budget_bytes: 16 * 1024 * 1024,
        max_region_bytes: None,
        regions: vec![
            RegionSpec::Ram {
                base: None,
                size: 0x1000_0000,
            },
            RegionSpec::Ram {
                base: None,
                size: 0x2000_0000,
            },
        ],
        description: None,
        wiring: None,
        roles: None,
        pins: None,
    }
}

/// **A-3** —— `base` **缺省**的描述**能解析** ⇒ ⇒ 新格式**能用**。
#[test]
fn a3_a_description_without_base_parses() {
    let json = r#"{
        "schema_version": 2,
        "id": "auto-base",
        "name": "Auto base",
        "priority": 100,
        "auto_selectable": false,
        "region_budget_bytes": 16777216,
        "regions": [
            { "kind": "ram", "size": 1048576 },
            { "kind": "ram", "size": 2097152 }
        ]
    }"#;
    let description = platform::parse_description(json).expect("缺 base 的描述必须能解析");
    assert_eq!(description.regions.len(), 2);
    assert!(
        description
            .regions
            .iter()
            .all(|region| region.base().is_none()),
        "两份描述里的段**都**没有基址（缺省优先 ⇒ ⇒ 解析出来就是缺省）"
    );
}

/// **K-1** —— 装配期分配：一份**不带** `base`** 的描述 ⇒ ⇒ 内核分配出
/// **确定**的基址 ⇒ ⇒ **两次装得到同一个图**。
#[test]
fn k1_a_description_without_base_gets_a_determined_layout() {
    let description = description_without_bases();
    let first = platform::assign_bases(&description).expect("分配必须成功");
    let second = platform::assign_bases(&description).expect("分配必须成功");
    // ⚠️ **两次分配的结果逐字相同** ⇒ ⇒ **确定** ⇒ ⇒ ⇒ **可复算**。
    assert_eq!(
        first, second,
        "同一份描述两次分配必须得到**同一个**图（确定 · 可复算）"
    );
    // ⚠️ **分配出来的基址是写死的字面量**（⇒ ⇒ 不从实现反推）。
    assert_eq!(first.regions[0].base(), Some(0x0000_0000));
    assert_eq!(first.regions[1].base(), Some(0x1000_0000));
}

/// **K-2** —— ⚠️ **必要对照** —— 分配结果**不等于**任何「按 base 升序」的
/// 猜测 ⇒ ⇒ 分配**真的**发生着。
///
/// ⚠️ **对照怎么做**：把「缺 `base` 当成 `base = 0`」那个**天真算法**算一遍
///   ⇒ ⇒ 它给出的两个基址**都是 0** ⇒ ⇒ ⇒ 分配结果与它**不同** ⇒ ⇒ ⇒
///   分配**真的**发生着（不是「缺省当 0」的换皮）。
#[test]
fn k2_the_allocation_is_not_a_naive_guess() {
    let description = description_without_bases();
    let assigned = platform::assign_bases(&description).expect("分配必须成功");
    // ⚠️ **天真算法**：把缺省当成 0。
    let naive: Vec<Option<u32>> = description
        .regions
        .iter()
        .map(|region| region.base().or(Some(0)))
        .collect();
    assert_eq!(
        naive,
        vec![Some(0), Some(0)],
        "对照基准：缺省当 0 ⇒ ⇒ 两个段的基址**都是** 0"
    );
    assert_ne!(
        assigned
            .regions
            .iter()
            .map(|region| region.base())
            .collect::<Vec<_>>(),
        naive,
        "分配结果必须**不等于**「缺省当 0」那个天真猜测 ⇒ ⇒ 分配真的发生着"
    );
}

/// **K-3** —— **老描述逐字不变**：带 `base` 的老描述 ⇒ ⇒ 装配结果与升版前
/// **逐字相同** ⇒ ⇒ **存量板档案零风险**。
#[test]
fn k3_an_old_description_is_unchanged_byte_for_byte() {
    // ⚠️ **内置描述**就是「老格式」（`schema_version = 1` · 每段**都带** base）。
    let old =
        platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt");
    let assigned = platform::assign_bases(&old).expect("分配必须成功");
    assert_eq!(
        assigned, old,
        "老描述（每段都带 base）⇒ ⇒ 分配之后**一个字节都不变**（逐字相同）"
    );
    // ⚠️ **老描述（`schema_version = 1`）⇒ ⇒ 仍能装**（缺省优先）。
    assert!(
        old.validate().is_ok(),
        "老描述（schema_version = 1）必须仍然合法（缺省优先）"
    );
}

/// **K-4** —— **装不下 ⇒ 报既有码**（`E_PLT-07`）⇒ ⇒ **零新场景码**。
#[test]
fn k4_a_region_that_cannot_fit_reports_an_existing_code() {
    let mut description = description_without_bases();
    // ⚠️ **装不下**：三段 RAM 的**总大小超过整个地址空间**（2^32）⇒ ⇒
    //   前两段占满 [0, 2^32) ⇒ ⇒ ⇒ 第三段**无处可放**。
    //   （⚠️ **注意**：单段 `size = u32::MAX` **装得下** ⇒ ⇒ `[0, 2^32)`
    //   恰好是整个地址空间 ⇒ ⇒ ⇒ 那样测不到「装不下」那条路。）
    description.regions = vec![
        RegionSpec::Ram {
            base: None,
            size: 0x8000_0000,
        },
        RegionSpec::Ram {
            base: None,
            size: 0x8000_0000,
        },
        RegionSpec::Ram {
            base: None,
            size: 0x0000_0001,
        },
    ];
    // ⚠️ **先让预算这一关放行**（否则 V-7 会先拒 ⇒ ⇒ 那样就测不到分配器
    //   那条路了）⇒ ⇒ 预算调到最大。
    description.region_budget_bytes = u32::MAX;
    let err = platform::assign_bases(&description).expect_err("装不下 ⇒ ⇒ 必须报错");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(
        err.scenario, E_PLT_07_OVER_BUDGET,
        "装不下 ⇒ ⇒ 报**既有码** `E_PLT-07`（⇒ ⇒ 零新场景码）"
    );
}

/// **K-4b** —— ⚠️ **必要对照** —— 一段**装得下**的缺基址 RAM ⇒ ⇒ **不报**
/// 错 ⇒ ⇒ ⇒ 上一条不是「凡缺基址就报错」。
#[test]
fn k4b_a_region_that_fits_does_not_report_an_error() {
    let mut description = description_without_bases();
    description.regions = vec![RegionSpec::Ram {
        base: None,
        size: 0x1000,
    }];
    let assigned = platform::assign_bases(&description).expect("装得下 ⇒ ⇒ 必须成功");
    assert_eq!(assigned.regions[0].base(), Some(0));
}

/// **K-4c** —— 缺基址的段**不会**撞上用户**明确写过**的基址 ⇒ ⇒ 「用户自己
/// 决定落在哪」那条纪律**不被推翻**。
#[test]
fn k4c_the_allocation_skips_the_bases_the_user_wrote_down() {
    let mut description = description_without_bases();
    // ⚠️ **用户明确写**了 `0` ⇒ ⇒ 分配器必须**跳过**它。
    description.regions = vec![
        RegionSpec::Ram {
            base: Some(0),
            size: 0x1000,
        },
        RegionSpec::Ram {
            base: None,
            size: 0x1000,
        },
    ];
    let assigned = platform::assign_bases(&description).expect("分配必须成功");
    assert_eq!(
        assigned.regions[0].base(),
        Some(0),
        "用户写的基址一个都不改"
    );
    assert_eq!(
        assigned.regions[1].base(),
        Some(0x1000),
        "缺基址的段必须**跳过**用户写过的区间"
    );
}

/// **K-4d** —— 装板（走宿主那条路）⇒ ⇒ 缺基址的板**真的**装得上 ⇒ ⇒
/// 映射表里那些段**真的有地址**。
#[test]
fn k4d_a_board_without_bases_really_loads() {
    let mut host = KernelHost::new();
    let mut description = description_without_bases();
    description.id = "auto-base-host".to_string();
    // ⚠️ **预算要够大**（⇒ ⇒ 否则 V-7 会先拒 ⇒ ⇒ 那样测不到「装得上」）。
    description.region_budget_bytes = 0x4000_0000;
    let events = host.handle_message(&riscv_sim_contract::protocol::Message::command(
        1,
        riscv_sim_contract::protocol::CommandPayload::SetMemoryMap {
            description: description.clone(),
            expected_revision: None,
        },
    ));
    // ⚠️ **没有错误事件** ⇒ ⇒ 装上了。
    assert!(
        !events.iter().any(|event| matches!(
            event,
            riscv_sim_contract::protocol::EventPayload::Error { .. }
        )),
        "缺基址的板必须装得上（事件：{events:?}）"
    );
    // ⚠️ **生效图里那些段真的有地址** ⇒ ⇒ 分配**真的**发生着。
    let memory_map = events
        .iter()
        .find_map(|event| match event {
            riscv_sim_contract::protocol::EventPayload::MemoryMap { description, .. } => {
                Some(description.clone())
            }
            _ => None,
        })
        .expect("必须有一条 MemoryMap 事件");
    assert_eq!(memory_map.regions.len(), 2);
    for region in &memory_map.regions {
        assert!(
            region.base().is_some(),
            "生效图里的每一段都**必须有**基址（分配过）"
        );
    }
}

/// **K-4e** —— ⚠️ **必要对照** —— 外设段的缺基址分配 ⇒ ⇒ **跳过**用户写过的
/// 区间 ⇒ ⇒ ⇒ 分配器**知道**外设的窗口大小（⇒ ⇒ 那是 **M-15 注册表**
/// 那份真相）。
#[test]
fn k4e_a_peripheral_without_a_base_skips_the_occupied_interval() {
    let mut description = description_without_bases();
    description.regions = vec![
        RegionSpec::Ram {
            base: Some(0),
            size: 0x1000,
        },
        RegionSpec::Peripheral {
            base: None,
            device: DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 64,
                contexts: 16,
            },
        },
    ];
    let assigned = platform::assign_bases(&description).expect("分配必须成功");
    assert_eq!(assigned.regions[0].base(), Some(0));
    // ⚠️ **PLIC 的窗口大小 = M-15 注册表那份** ⇒ ⇒ 分配必须落在它**之后**。
    let window = crate::m15_peripheral::types::window_bytes("plic").expect("PLIC 在注册表里");
    let base = assigned.regions[1].base().expect("外设也要分到基址");
    assert!(
        base >= 0x1000,
        "外设的基址必须**跳过** RAM 占的区间（base = {base:#x}）"
    );
    assert!(
        u64::from(base) + u64::from(window) <= u64::from(u32::MAX),
        "外设的窗口必须**装得下**"
    );
}

/// **K-4f** —— ⚠️ **显式段只是「占住区间」，不是「屏障」** ⇒ ⇒ 缺基址的段
/// 落在**第一个**空隙（⇒ ⇒ **低位**）⇒ ⇒ **不是**被推到高位显式段**之后**。
///
/// ⚠️ **为什么这条重要**（v0.73 施工期订正）：规格 §2.3 写的是「从当前游标
///   往上找**第一个装得下的**空隙 ⇒ ⇒ **跳过**被别的段占掉的区间」⇒ ⇒ 显式
///   段**只是占用**（⇒ ⇒ 撞上就跳过）⇒ ⇒ **不是**屏障 ⇒ ⇒ ⇒ 判据必须**真的
///   区分**这两种读法（⇒ ⇒ 否则「屏障」那个读法会**悄悄**进来）。
#[test]
fn k4f_an_explicit_region_is_not_a_barrier() {
    let mut description = description_without_bases();
    // ⚠️ **显式段在高位**、**缺基址段在后** ⇒ ⇒ 缺基址段**必须**落在低位
    //   的**第一个**空隙（⇒ ⇒ **不是**高位之后）。
    description.regions = vec![
        RegionSpec::Ram {
            base: Some(0x8000_0000),
            size: 0x1000,
        },
        RegionSpec::Ram {
            base: None,
            size: 0x1000,
        },
    ];
    let assigned = platform::assign_bases(&description).expect("分配必须成功");
    assert_eq!(
        assigned.regions[1].base(),
        Some(0x0000_0000),
        "缺基址的段必须落在**第一个**空隙（显式段只是占住区间、**不是**屏障）"
    );
}
