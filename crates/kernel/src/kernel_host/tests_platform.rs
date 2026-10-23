//! Lane B —— **平台描述驱动的宿主**（v0.27 · D-123-B）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-12_V027_Platform_Description.md`
//! §5.2（验收 **B-1 ~ B-12**）· §2.2（宿主落点）· §3（裁决 A~J）。
//!
//! ## 这些用例在盯什么
//!
//! v0.27 之前，宿主的内存图是**写死在代码里**的两档 ⇒ 「换一块板子」
//! ＝「改宿主源码」。v0.27 把它变成**数据**（平台描述）⇒ 本文件盯住
//! 三件事：
//!
//! 1. **两份内置描述逐字复现今天的两档**（B-1 / B-2）—— 描述化**不许**
//!    改变任何一颗字节的地址、每一个器件的参数；
//! 2. **换描述、宿主零改动**（B-6 / B-7）—— 判别式在**源码**上，不在
//!    行为上（`map_memory` 里不许出现地址字面量、器件构造、`match
//!    profile`）；
//! 3. **约束各有一个落点**（B-3 / B-4 / B-5 / B-11）—— 窗口大小由器件
//!    自己报、护栏预算由描述说、校验由**契约层**做。
//!
//! ## 断言值一律写字面量
//!
//! 不从被测实现反推 —— 例如 B-3 的窗口大小写 **`0x8` / `0x1_0000` /
//! `0x21_0000` / `0x10`**（**架构字面量**），而不是
//! `UART_WINDOW` / `CLINT_WINDOW` 那种「跟实现同源」的写法 ⇒ 断言不能
//! 跟着实现一起漂移。

use std::path::PathBuf;

use riscv_sim_contract::error::{ErrorFamily, PlatformError, PLATFORM_FAMILY};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};

use super::platform;
use super::{KernelHost, MemoryProfile, DEFAULT_RAM_HIGH_BASE, DEFAULT_RAM_HIGH_SIZE};
use crate::m14_bus::Bus;

/// 内置描述 `default`（**今天的 Default 档**）。
fn default_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置描述 default")
}

/// 内置描述 `qemu-virt`（**今天的 QemuVirt 档**）。
fn qemu_virt_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_QEMU_VIRT).expect("内置描述 qemu-virt")
}

/// 内置描述 `minimal`（**换描述、宿主零改动**的证据）。
fn minimal_description() -> PlatformDescription {
    platform::builtin_description(platform::PLATFORM_ID_MINIMAL).expect("内置描述 minimal")
}

// ---------------------------------------------------------------------------
// B-1 / B-2 · 两份内置描述逐字复现今天的两档
// ---------------------------------------------------------------------------

/// **B-1** `default` 描述建出的内存图 == **今天的 Default 档**。
///
/// ⚠️ 逐条比对 **base / size / kind / target** 四项 + **区域数量** ⇒
/// 少一个区域、多一个区域、地址挪一位、器件 id 变了，**都会红**。
#[test]
fn the_default_description_builds_the_default_memory_map() {
    let host = KernelHost::new_with_platform(platform::PLATFORM_ID_DEFAULT).expect("default 平台");
    let regions = host.bus().export_map().regions;

    // ⚠️ **架构字面量**（今天的 Default 档）：低段 RAM 1 MiB ·
    // GPIO0 @ 0x4000_0000（窗口 16 字节）· 高段 RAM 4 MiB。
    let expected: [(u32, u32, &str, &str); 3] = [
        (0x0000_0000, 0x0010_0000, "ram", ""),
        (0x4000_0000, 0x0000_0010, "peripheral", "GPIO0"),
        (0x8000_0000, 0x0040_0000, "ram", ""),
    ];
    assert_eq!(
        regions.len(),
        expected.len(),
        "Default 档必须是三个区域（两段 RAM + 一个 GPIO）"
    );
    for (index, (base, size, kind, target)) in expected.iter().enumerate() {
        assert_eq!(regions[index].base, *base, "区域 {index} 的基址");
        assert_eq!(regions[index].size, *size, "区域 {index} 的大小");
        assert_eq!(regions[index].kind, *kind, "区域 {index} 的种类");
        assert_eq!(regions[index].target, *target, "区域 {index} 的器件");
    }
    // ⚠️ **按 base 升序**（`export_map` 遍历的是 `BTreeMap` ⇒ 天生升序）。
    for pair in regions.windows(2) {
        assert!(
            pair[0].base < pair[1].base,
            "导出的区域必须按 base 升序：{:?} 然后 {:?}",
            pair[0],
            pair[1]
        );
    }
}

/// **B-2** `qemu-virt` 描述建出的内存图 == **今天的 QemuVirt 档**。
///
/// ⚠️ **六个**区域：低段 RAM 1 MiB · CLINT @ 0x0200_0000 · PLIC @
/// 0x0C00_0000 · UART0 @ 0x1000_0000 · GPIO0 @ 0x4000_0000 · 高段 RAM
/// **一个** 128 MiB（v0.26 起不是 8 个 16 MiB 的）。
#[test]
fn the_qemu_virt_description_builds_the_qemu_virt_memory_map() {
    let host =
        KernelHost::new_with_platform(platform::PLATFORM_ID_QEMU_VIRT).expect("qemu-virt 平台");
    let regions = host.bus().export_map().regions;

    let expected: [(u32, u32, &str, &str); 6] = [
        (0x0000_0000, 0x0010_0000, "ram", ""),
        (0x0200_0000, 0x0001_0000, "peripheral", "CLINT"),
        (0x0C00_0000, 0x0021_0000, "peripheral", "PLIC"),
        (0x1000_0000, 0x0000_0008, "peripheral", "UART0"),
        (0x4000_0000, 0x0000_0010, "peripheral", "GPIO0"),
        (0x8000_0000, 0x0800_0000, "ram", ""),
    ];
    assert_eq!(
        regions.len(),
        expected.len(),
        "QemuVirt 档必须是六个区域（1 低段 + 1 高段 + GPIO + UART + CLINT + PLIC）"
    );
    for (index, (base, size, kind, target)) in expected.iter().enumerate() {
        assert_eq!(regions[index].base, *base, "区域 {index} 的基址");
        assert_eq!(regions[index].size, *size, "区域 {index} 的大小");
        assert_eq!(regions[index].kind, *kind, "区域 {index} 的种类");
        assert_eq!(regions[index].target, *target, "区域 {index} 的器件");
    }
}

// ---------------------------------------------------------------------------
// B-3 · 外设窗口大小由器件自己报
// ---------------------------------------------------------------------------

/// **B-3** 外设窗口大小**由器件自己报**（裁决 D）⇒ 描述里**没有**窗口
/// 大小 ⇒ 断言的是**架构字面量**。
///
/// ⚠️ 四个窗口：UART0 = **8 字节** · CLINT = **64 KiB** · PLIC =
/// **0x21_0000** · GPIO0 = **16 字节**。
#[test]
fn the_peripheral_window_size_comes_from_the_device() {
    let host =
        KernelHost::new_with_platform(platform::PLATFORM_ID_QEMU_VIRT).expect("qemu-virt 平台");
    let regions = host.bus().export_map().regions;

    let expected: [(u32, u32); 4] = [
        (0x1000_0000, 0x0000_0008), // UART0
        (0x0200_0000, 0x0001_0000), // CLINT
        (0x0C00_0000, 0x0021_0000), // PLIC
        (0x4000_0000, 0x0000_0010), // GPIO0
    ];
    for (base, window) in expected {
        let region = regions
            .iter()
            .find(|region| region.base == base)
            .unwrap_or_else(|| panic!("{base:#x} 上必须有外设窗口"));
        assert_eq!(
            region.size, window,
            "{base:#x} 的窗口大小必须由器件自己报（{window:#x} 字节）"
        );
    }

    // ⚠️ **描述里根本没有窗口大小**（裁决 D）⇒ 外设条目的「声明大小」是
    // `None` ⇒ 没有第二个真相源。
    for region in &qemu_virt_description().regions {
        if let RegionSpec::Peripheral { .. } = region {
            assert!(
                region.declared_size().is_none(),
                "外设条目不许带声明大小：窗口大小是器件的寄存器图"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// B-4 · 护栏预算
// ---------------------------------------------------------------------------

/// **B-4** 每区域的护栏预算**由描述说** ⇒ `bus.max_region_bytes()` 与
/// `region_budget_bytes` **逐字相等**。
#[test]
fn the_guardrail_budget_comes_from_the_description() {
    for (id, budget) in [
        (platform::PLATFORM_ID_DEFAULT, 16 * 1024 * 1024u64),
        (platform::PLATFORM_ID_QEMU_VIRT, 128 * 1024 * 1024u64),
    ] {
        let host = KernelHost::new_with_platform(id).expect("内置平台");
        assert_eq!(
            host.bus().max_region_bytes(),
            budget,
            "{id} 的每区域护栏预算"
        );
        let description = platform::builtin_description(id).expect("内置描述");
        assert_eq!(
            u64::from(description.region_budget_bytes),
            budget,
            "{id} 描述里的 region_budget_bytes"
        );
    }
}

// ---------------------------------------------------------------------------
// B-5 · V-7 生效（超预算 ⇒ 建图前就被拒）
// ---------------------------------------------------------------------------

/// **B-5** 描述声明一段**超过**预算的 RAM ⇒ **建图前**就被拒
/// （`E_PLT-07`）⇒ **一个区域都不留**。
///
/// ⚠️ 判据是 `family` + `scenario`，**不看** message 文本。
#[test]
fn an_over_budget_region_is_rejected_before_anything_is_mapped() {
    let description = PlatformDescription {
        schema_version: 1,
        id: "over-budget".to_string(),
        name: "Over budget".to_string(),
        priority: 5,
        auto_selectable: true,
        region_budget_bytes: 16 * 1024 * 1024,
        regions: vec![
            RegionSpec::Ram {
                base: 0,
                size: 1024,
            },
            RegionSpec::Ram {
                base: 0x8000_0000,
                size: 32 * 1024 * 1024, // ⚠️ 超过 16 MiB 的预算
            },
        ],
        description: None,
    };

    let mut bus = Bus::new();
    let err = platform::map_platform(&mut bus, &description).expect_err("超预算必须被拒");
    assert_eq!(err.family, PLATFORM_FAMILY, "错误族");
    assert_eq!(err.scenario, 7, "场景码必须是 E_PLT-07（声明期超预算）");
    assert!(
        bus.export_map().regions.is_empty(),
        "建图前就被拒 ⇒ 一个区域都不留"
    );
}

// ---------------------------------------------------------------------------
// B-6 · 零逻辑改动（**源码级**判别）
// ---------------------------------------------------------------------------

/// **B-6** `map_memory` 的**源码**里不许出现：任何地址字面量（`0x`）、
/// 任何器件构造调用、任何 `match profile`。
///
/// ⚠️ 这条用例**读自己的源码文本**（`include_str!` 读的是**真实的那个
/// 文件**）⇒ 判别式在**源码**上，不在行为上 ⇒ 「换描述、宿主零改动」
/// 在源码层面成立。
#[test]
fn map_memory_has_no_address_literal_and_no_device_construction() {
    // ⚠️ `include_str!` 的路径相对于**本文件** ⇒ 指向
    // `crates/kernel/src/kernel_host/mod.rs`（真的那一个）。
    let source = include_str!("mod.rs");
    let body = map_memory_body(source);

    assert!(
        !body.contains("0x"),
        "map_memory 里不许出现地址字面量（0x）：\n{body}"
    );
    for forbidden in [
        "GpioBank::new",
        "Uart16550::new",
        "Clint::with_frequency",
        "Plic::with_capacity",
    ] {
        assert!(
            !body.contains(forbidden),
            "map_memory 里不许出现器件构造调用（{forbidden}）：\n{body}"
        );
    }
    assert!(
        !body.contains("match profile"),
        "map_memory 里不许再按档位分支（match profile）：\n{body}"
    );
}

/// 从**源码文本**里切出 `map_memory` 的函数体。
///
/// ⚠️ **按花括号配平**找结尾 ⇒ 不靠行号 ⇒ 源码挪位置也不失效。
fn map_memory_body(source: &str) -> String {
    let start = source
        .find("fn map_memory(")
        .expect("mod.rs 里必须有 map_memory");
    let open = source[start..].find('{').expect("map_memory 必须有函数体");
    let body_start = start + open;
    let mut depth = 0usize;
    for (offset, ch) in source[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return source[body_start..body_start + offset + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("map_memory 的函数体没有配平的括号");
}

// ---------------------------------------------------------------------------
// B-7 · 换描述、宿主零改动
// ---------------------------------------------------------------------------

/// **B-7** `minimal` 描述（低段 2 MiB + 高段 16 MiB @ 0x1000_0000，**与
/// 两档都不同**）⇒ 显式建图 ⇒ 内存图**逐条**等于描述。
///
/// ⚠️ **没有任何一行宿主代码是为它而写的**（B-6 的源码判别式兜住）。
#[test]
fn the_minimal_description_builds_its_own_map_without_any_host_change() {
    let host = KernelHost::new_with_platform(platform::PLATFORM_ID_MINIMAL).expect("minimal 平台");
    let regions = host.bus().export_map().regions;

    let expected: [(u32, u32); 2] = [
        (0x0000_0000, 0x0020_0000), // 低段 2 MiB
        (0x1000_0000, 0x0100_0000), // 高段 16 MiB @ 0x1000_0000
    ];
    assert_eq!(regions.len(), expected.len(), "minimal 平台是**两段** RAM");
    for (index, (base, size)) in expected.iter().enumerate() {
        assert_eq!(regions[index].base, *base, "区域 {index} 的基址");
        assert_eq!(regions[index].size, *size, "区域 {index} 的大小");
        assert_eq!(regions[index].kind, "ram", "区域 {index} 的种类");
        assert_eq!(regions[index].target, "", "区域 {index} 没有器件");
    }

    // ⚠️ **逐条等于描述**（取值来源 = 描述，不是宿主常量）。
    let description = minimal_description();
    let described = platform::regions_by_base(&description);
    for (region, spec) in regions.iter().zip(described.iter()) {
        assert_eq!(region.base, spec.base(), "内存图与描述逐条相等（基址）");
        assert_eq!(region.kind, spec.kind(), "内存图与描述逐条相等（种类）");
        assert_eq!(
            region.size,
            spec.declared_size().expect("RAM 条目有声明大小"),
            "内存图与描述逐条相等（大小）"
        );
    }
}

// ---------------------------------------------------------------------------
// B-8 · 运行时路径（文件）
// ---------------------------------------------------------------------------

/// **B-8** 从一个**仓外临时文件**读描述 ⇒ 建图成功 ⇒ 与同名内置描述
/// **结果一致**。
///
/// ⚠️ 两条路径**共用同一个解析器 + 同一个校验器**（§2.5）⇒ 没有两份
/// 语义。
#[test]
fn a_description_loaded_from_a_file_matches_the_built_in_one() {
    let path = write_temp_description("default", &default_description());
    let host = KernelHost::new_with_platform_file(&path).expect("文件路径建图");
    let from_file = host.bus().export_map().regions;
    let built_in = KernelHost::new_with_platform(platform::PLATFORM_ID_DEFAULT)
        .expect("内置 default")
        .bus()
        .export_map()
        .regions;

    assert_eq!(
        from_file.len(),
        built_in.len(),
        "文件路径与内置路径的区域数量必须一致"
    );
    for (index, (file_region, builtin_region)) in from_file.iter().zip(built_in.iter()).enumerate()
    {
        assert_eq!(file_region.base, builtin_region.base, "区域 {index} 的基址");
        assert_eq!(file_region.size, builtin_region.size, "区域 {index} 的大小");
        assert_eq!(file_region.kind, builtin_region.kind, "区域 {index} 的种类");
        assert_eq!(
            file_region.target, builtin_region.target,
            "区域 {index} 的器件"
        );
    }
    assert_eq!(host.platform_id(), "default", "平台 id");
    assert_eq!(
        host.bus().max_region_bytes(),
        16 * 1024 * 1024,
        "护栏预算也一样"
    );

    // ⚠️ 顺手清掉临时文件（它在**仓外**）。
    let _ = std::fs::remove_file(&path);
}

/// 把一份描述写到**仓外**的临时文件（`std::env::temp_dir()`）。
fn write_temp_description(name: &str, description: &PlatformDescription) -> PathBuf {
    let json = serde_json::to_string_pretty(description).expect("描述能序列化");
    let mut path = std::env::temp_dir();
    path.push(format!("riscv-sim-platform-{name}.json"));
    std::fs::write(&path, json).expect("临时描述文件写得出");
    path
}

// ---------------------------------------------------------------------------
// B-9 · 自动选档（裁决 E：逐字等价于今天）
// ---------------------------------------------------------------------------

/// **B-9** 自动选档的**六条**等价性 + 与**今天**的判据**逐条对照**。
///
/// ⚠️ 「今天」的判据在测试里**照抄**一遍（参考实现）⇒ 两条路径对同一
/// 组区间必须给出**同一个平台** ⇒ 「逐字等价」是**可证的**，不是散文。
#[test]
fn the_auto_selection_is_equivalent_to_todays_rule() {
    // ⚠️ 六个区间：Default 高段 · QemuVirt 高段 · 两档之外 · 低段 ·
    // 低段**超出 1 MiB** · 只装得进**演示平台**的那一段。
    let cases: [(&str, &[(u32, u32)]); 6] = [
        ("Default 高段", &[(0x8000_0000, 0x8000_1000)]),
        ("QemuVirt 高段", &[(0x8040_0000, 0x8040_1000)]),
        ("两档之外", &[(0x9000_0000, 0x9000_1000)]),
        ("低段", &[(0x0000_0000, 0x0000_1000)]),
        ("低段超出 1 MiB", &[(0x0000_0000, 0x0010_0001)]),
        ("只装得进演示平台", &[(0x0000_0000, 0x0020_0001)]),
    ];
    for (label, ranges) in cases {
        let chosen = platform::choose_description(ranges).expect("选档");
        let today = choose_like_today(ranges);
        assert_eq!(
            chosen.id, today,
            "{label}：描述化的判据必须与今天的判据一致"
        );
    }

    // ⚠️ 六条**具体**的期望值（不靠参考实现兜底）。
    assert_eq!(
        platform::choose_description(&[(0x8000_0000, 0x8000_1000)])
            .expect("选档")
            .id,
        "default",
        "① Default 高段 ⇒ default"
    );
    assert_eq!(
        platform::choose_description(&[(0x8040_0000, 0x8040_1000)])
            .expect("选档")
            .id,
        "qemu-virt",
        "② QemuVirt 高段 ⇒ qemu-virt"
    );
    assert_eq!(
        platform::choose_description(&[(0x9000_0000, 0x9000_1000)])
            .expect("选档")
            .id,
        "default",
        "③ 两档之外 ⇒ 回落 default"
    );
    assert_eq!(
        platform::choose_description(&[(0x0000_0000, 0x0000_1000)])
            .expect("选档")
            .id,
        "default",
        "④ 低段 ⇒ default（两份真实描述的低段都是 1 MiB，default 的 priority 更小）"
    );

    // ⑤ 低段**超出 1 MiB**：两份真实描述的低段都装不下 ⇒ 无候选 ⇒ 回落
    // default ⇒ **随后报未映射**（与今天逐字一致）。
    let over_low = [(0x0000_0000u32, 0x0010_0001u32)];
    let chosen = platform::choose_description(&over_low).expect("选档");
    assert_eq!(chosen.id, "default", "⑤ 低段超出 1 MiB ⇒ 回落 default");
    assert!(
        !holds(&chosen, over_low[0]),
        "⑤ 回落的那一档**装不下**这段 ⇒ 随后的装载必然报未映射"
    );
}

/// 一份描述**装不装得下**某个区间（只看 RAM 区域 —— 与选档判据同口径）。
fn holds(description: &PlatformDescription, range: (u32, u32)) -> bool {
    description.regions.iter().any(|region| match region {
        RegionSpec::Ram { base, size } => range.0 >= *base && range.1 <= base.saturating_add(*size),
        RegionSpec::Rom { .. } | RegionSpec::Peripheral { .. } => false,
    })
}

/// **B-9 的补充**：「**区间在外层**」这条结构必须钉住。
///
/// ⚠️ 规格 §5.2 B-9 点名的形状：**一个区间落 default 高段、另一个落
/// qemu-virt 高段** ⇒ 结果必须是 `qemu-virt`。
#[test]
fn a_multi_range_image_selects_by_range_not_by_description() {
    // ① 规格点名的形状。
    let default_high_then_qemu_high = [
        (0x8000_0000u32, 0x8000_1000u32),
        (0x8040_0000u32, 0x8040_1000u32),
    ];
    assert_eq!(
        platform::choose_description(&default_high_then_qemu_high)
            .expect("选档")
            .id,
        "qemu-virt",
        "多区间镜像：遇到只被 qemu-virt 覆盖的区间 ⇒ 立即选 qemu-virt"
    );

    // ② **判别力**的那一条：第二个区间落在**两档之外** ⇒
    // 「区间在外层」⇒ 第一个区间就**立即**选了 qemu-virt；
    // 「描述在外层」（找第一个能装下**全部**区间的描述）⇒ **没有**描述
    // 装得下 ⇒ 回落 default ⇒ 两者**结果不同** ⇒ 这条用例真的钉住了
    // **结构**，不只是钉住了结果。
    let qemu_high_then_outside = [
        (0x8040_0000u32, 0x8040_1000u32),
        (0x9000_0000u32, 0x9000_1000u32),
    ];
    assert_eq!(
        platform::choose_description(&qemu_high_then_outside)
            .expect("选档")
            .id,
        "qemu-virt",
        "区间在外层 ⇒ 遇到只被 qemu-virt 覆盖的区间就立即选它（描述在外层会回落 default）"
    );

    // ③ 顺序也钉住：**倒过来** ⇒ 第一个区间落在两档之外（跳过）· 第二个
    // 落 default 高段（⇒ 继续）⇒ 全部过完 ⇒ 回落 default。
    let outside_then_default_high = [
        (0x9000_0000u32, 0x9000_1000u32),
        (0x8000_0000u32, 0x8000_1000u32),
    ];
    assert_eq!(
        platform::choose_description(&outside_then_default_high)
            .expect("选档")
            .id,
        "default",
        "两档之外的区间被跳过 · default 高段的区间让选档继续 ⇒ 回落 default"
    );
}

/// **B-9 的第六条**：`minimal` **永不**被自动选中。
///
/// ⚠️ 这条规则写在**数据里**：`minimal` 的 `auto_selectable` 是
/// `false` ⇒ 它**不参与**自动选档 ⇒ 只能被**显式**指定 ⇒ 宿主**不按
/// id 硬编码**候选集（那会是「代码知道哪个平台是演示平台」⇒ 换个人写
/// 描述就会破）。
#[test]
fn the_demo_platform_is_never_auto_selected() {
    // ⚠️ 三份内置描述的 `auto_selectable`（**数据**承载差异）。
    assert!(
        default_description().auto_selectable,
        "default 参与自动选档"
    );
    assert!(
        qemu_virt_description().auto_selectable,
        "qemu-virt 参与自动选档"
    );
    assert!(
        !minimal_description().auto_selectable,
        "minimal（演示平台）不参与自动选档 ⇒ 只能被显式指定"
    );

    // ⚠️ `minimal` 的低段 RAM 是 **2 MiB** ⇒ 这段（2 MiB + 1 字节）装得进
    // `minimal`、装不进 `default` 与 `qemu-virt`（两者的低段都是 1 MiB）。
    let only_minimal_fits = [(0x0000_0000u32, 0x0020_0001u32)];
    let chosen = platform::choose_description(&only_minimal_fits).expect("选档");
    assert_eq!(
        chosen.id, "default",
        "演示平台只被显式指定 ⇒ 自动选档不选它"
    );
    assert_eq!(
        choose_like_today(&only_minimal_fits),
        chosen.id,
        "与今天的判据一致"
    );

    // ⚠️ `minimal` 的高段窗口（0x1000_0000 起 16 MiB）同理。
    let in_minimal_high = [(0x1000_0000u32, 0x1000_1000u32)];
    assert_eq!(
        platform::choose_description(&in_minimal_high)
            .expect("选档")
            .id,
        "default",
        "演示平台的高段窗口也不参与自动选档"
    );
}

/// **今天**的判据（照抄被描述化替换掉的那段逻辑）—— 参考实现。
///
/// ⚠️ 它用**宿主常量**（`DEFAULT_RAM_HIGH_*` / `QEMU_VIRT_RAM_*`）⇒ 与
/// 「今天的行为」同源 ⇒ 差分测试才有意义。
fn choose_like_today(ranges: &[(u32, u32)]) -> &'static str {
    /// 一个地址区间是否落在 `[base, base+size)` 里。
    fn inside(base: u32, size: u32, lo: u32, hi: u32) -> bool {
        lo >= base && hi <= base.saturating_add(size)
    }
    for &(lo, hi) in ranges {
        let in_default_high = inside(DEFAULT_RAM_HIGH_BASE, DEFAULT_RAM_HIGH_SIZE, lo, hi);
        let in_qemu_high = inside(super::QEMU_VIRT_RAM_BASE, super::QEMU_VIRT_RAM_SIZE, lo, hi);
        if !in_default_high && !in_qemu_high {
            continue;
        }
        if !in_default_high {
            return "qemu-virt";
        }
    }
    "default"
}

// ---------------------------------------------------------------------------
// B-10 · 兼容 API（裁决 C）
// ---------------------------------------------------------------------------

/// **B-10** `memory_profile()` 仍是**兼容视图**，`platform_id()` 给**真实**
/// id，`new_with_profile` 两条路径与今天**逐字等价**。
///
/// ⚠️ **既有的 4 条 `memory_profile()` 断言一条不改** —— 这里只是**再**
/// 钉一遍投影规则。
#[test]
fn the_compatible_api_still_works() {
    let default = KernelHost::new();
    assert_eq!(default.memory_profile(), MemoryProfile::Default);
    assert_eq!(default.platform_id(), "default");

    let qemu = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_eq!(qemu.memory_profile(), MemoryProfile::QemuVirt);
    assert_eq!(qemu.platform_id(), "qemu-virt");

    let explicit = KernelHost::new_with_profile(MemoryProfile::Default);
    assert_eq!(explicit.memory_profile(), MemoryProfile::Default);
    assert_eq!(explicit.platform_id(), "default");

    // ⚠️ **新平台的兼容投影是 `Default`**（诚实但信息量低）⇒ 真实 id 用
    // `platform_id()` 拿。
    let minimal = KernelHost::new_with_platform(platform::PLATFORM_ID_MINIMAL).expect("minimal");
    assert_eq!(minimal.memory_profile(), MemoryProfile::Default);
    assert_eq!(minimal.platform_id(), "minimal");

    // ⚠️ `new_with_profile(QemuVirt)` 与 `new_with_platform("qemu-virt")`
    // **逐字等价**（同一条路径的两个入口）。
    let by_profile = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    let by_id = KernelHost::new_with_platform(platform::PLATFORM_ID_QEMU_VIRT).expect("qemu-virt");
    assert_eq!(
        by_profile.bus().export_map().regions.len(),
        by_id.bus().export_map().regions.len(),
        "两条入口的区域数量"
    );
    assert_eq!(
        by_profile.bus().max_region_bytes(),
        by_id.bus().max_region_bytes(),
        "两条入口的护栏预算"
    );
}

// ---------------------------------------------------------------------------
// B-11 · 描述坏了要报（**各自的**码）
// ---------------------------------------------------------------------------

/// **B-11** 坏的描述**各有各的码**（不是同一个）⇒ 断言 `family` +
/// `scenario`，**不看** message 文本。
#[test]
fn a_broken_description_reports_its_own_code() {
    // ① 版本不对 ⇒ E_PLT-01。
    let mut wrong_version = default_description();
    wrong_version.schema_version = 2;
    // ② 区域重叠 ⇒ E_PLT-06。
    let overlapping = PlatformDescription {
        schema_version: 1,
        id: "overlap".to_string(),
        name: "Overlap".to_string(),
        priority: 5,
        auto_selectable: true,
        region_budget_bytes: 16 * 1024 * 1024,
        regions: vec![
            RegionSpec::Ram {
                base: 0,
                size: 0x1000,
            },
            RegionSpec::Ram {
                base: 0x0800,
                size: 0x1000,
            },
        ],
        description: None,
    };
    // ③ 预算超了 ⇒ E_PLT-07。
    let over_budget = PlatformDescription {
        schema_version: 1,
        id: "budget".to_string(),
        name: "Budget".to_string(),
        priority: 5,
        auto_selectable: true,
        region_budget_bytes: 1024,
        regions: vec![RegionSpec::Ram {
            base: 0,
            size: 4096,
        }],
        description: None,
    };
    // ④ 器件 id 重复 ⇒ E_PLT-08。
    let duplicate_id = PlatformDescription {
        schema_version: 1,
        id: "dup".to_string(),
        name: "Duplicate".to_string(),
        priority: 5,
        auto_selectable: true,
        region_budget_bytes: 16 * 1024 * 1024,
        regions: vec![
            RegionSpec::Peripheral {
                base: 0x4000_0000,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
            RegionSpec::Peripheral {
                base: 0x4000_1000,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
        ],
        description: None,
    };

    let cases: [(&str, PlatformDescription, u16); 4] = [
        ("版本不对", wrong_version, 1),
        ("区域重叠", overlapping, 6),
        ("预算超了", over_budget, 7),
        ("器件 id 重复", duplicate_id, 8),
    ];
    let mut seen: Vec<u16> = Vec::new();
    for (label, description, scenario) in cases {
        let err = description.validate().expect_err("坏描述必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY, "{label}：错误族");
        assert_eq!(err.scenario, scenario, "{label}：场景码");
        assert!(!seen.contains(&err.scenario), "{label}：每个坏法各有各的码");
        seen.push(err.scenario);
    }
}

/// **B-11 的延伸**：宿主侧的三种「取不到描述」也各有各的码。
///
/// ⚠️ 契约层拥有的是**描述的声明规则**（`E_PLT-01` .. `E_PLT-08`）⇒
/// 「按 id 取不到」「文件读不到」「文件解析不了」这三条属于**宿主**
/// ⇒ 码住在宿主（`platform.rs`）。
#[test]
fn an_unusable_description_reports_its_own_code() {
    // ① 内置 id 不存在 ⇒ E_PLT-09。
    let err = platform::builtin_description("no-such-platform").expect_err("取不到内置描述");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(err.scenario, 9);

    // ② 文件读不到 ⇒ E_PLT-10。
    let missing = std::env::temp_dir().join("riscv-sim-platform-does-not-exist.json");
    let err = KernelHost::new_with_platform_file(&missing).expect_err("文件读不到");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(err.scenario, 10);

    // ③ 文件不是合法 JSON ⇒ E_PLT-11（serde 层，校验规则 V-8）。
    let mut path = std::env::temp_dir();
    path.push("riscv-sim-platform-malformed.json");
    std::fs::write(&path, "{ not json").expect("临时文件写得出");
    let err = KernelHost::new_with_platform_file(&path).expect_err("文件解析不了");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(err.scenario, 11);
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// S-2 · 描述往返（**逐字节**相等）
// ---------------------------------------------------------------------------

/// **S-2** 内置描述读出来 → 解析 → **重新序列化** ⇒ 与仓内文件**逐字节
/// 相等**（描述文件是**规范形式**，不会因为 serde 的字段顺序漂移）。
#[test]
fn the_builtin_descriptions_round_trip_byte_for_byte() {
    for (name, json) in [
        ("default", platform::BUILTIN_JSON_DEFAULT),
        ("qemu-virt", platform::BUILTIN_JSON_QEMU_VIRT),
        ("minimal", platform::BUILTIN_JSON_MINIMAL),
    ] {
        let description = platform::parse_description(json).expect("内置描述能解析");
        let canonical = serde_json::to_string_pretty(&description).expect("描述能序列化");
        assert_eq!(
            canonical,
            json.trim_end_matches('\n'),
            "{name}.json 必须是 serde 的规范形式（逐字节相等）"
        );
    }
}

// ---------------------------------------------------------------------------
// 描述本身的自洽性（不是验收项，是**地基**）
// ---------------------------------------------------------------------------

/// 三份内置描述都**合法**，且 `id` / `priority` 与文件里的名字一致。
#[test]
fn the_builtin_descriptions_are_well_formed() {
    let descriptions = platform::builtin_descriptions().expect("内置描述都合法");
    assert_eq!(descriptions.len(), 3, "三份内置描述");
    for description in descriptions {
        assert_eq!(
            description.schema_version,
            riscv_sim_contract::platform::PLATFORM_SCHEMA_VERSION,
            "{} 的格式版本",
            description.id
        );
        assert!(!description.id.is_empty(), "平台 id 不能为空");
        assert!(
            description
                .regions
                .iter()
                .any(|region| { matches!(region, RegionSpec::Ram { .. }) }),
            "{} 至少要有一段 RAM（否则镜像无处可放）",
            description.id
        );
    }
}

/// 宿主的**错误**仍然是契约层的 `PlatformError`（族 / 场景码可判别）。
#[test]
fn the_platform_error_carries_a_judgeable_code() {
    let err: PlatformError = platform::builtin_description("nope").expect_err("取不到");
    assert_eq!(err.family, PLATFORM_FAMILY);
    assert_eq!(err.family, ErrorFamily::Platform);
    assert_eq!(err.family.as_str(), "PLT");
}
