//! Lane B · 宿主侧的**板级内存图**（STAGE-14 §5.2 · 验收 B-1 ~ B-9）。
//!
//! 本文件钉住的是**宿主**这一段的七件事：
//!
//! | 验收 | 一句话 |
//! | --- | --- |
//! | **B-1** | `SetMemoryMap`（未装载）⇒ 总线按新图**重建** |
//! | **B-2** | 描述不合规 ⇒ **`E_PLT-07`**（族 `PLT`）且**什么都不动** |
//! | **B-3** | 已装载 ⇒ **不重建总线**（guest 一字不受影响）+ `pending = true` |
//! | **B-4** | 配过板 ⇒ `LoadProject` 用**用户的图**建图（⚠️ **含「装载前配的板」这一路**） |
//! | **B-5** | **从未配过板** ⇒ `LoadProject` 走**自动选档**（逐字不变） |
//! | **B-6** | `Subscribe { "memory.map" }` ⇒ **立即**推一条当前图 |
//! | **B-7** | 配的板里**器件真的挂上了**（四个实例都可读） |
//! | **B-8** | **RAM 大小真的可定制**（8 MiB 之内成功 / 之外架构陷阱） |
//! | **B-9** | 既有宿主断言**一条未改**（由 `git diff` 复核，见交付报告） |
//!
//! ⚠️ **「待生效」是推导量**（STAGE-14 §3.2 ④，第三次订正）：宿主存的是
//! **用户配的板**（用户意图，**跨 `LoadProject` 保持**），「待生效」=
//! 「用户的板 ≠ 当前生效的图」⇒ **不单独存标志**。
//!
//! ⚠️ **断言值一律写架构字面量**（`"E_PLT-07"` / `5` / `7` / `"memory.map"`）
//! —— 从被测实现反推出来的断言是**假绿**。
//!
//! ⚠️ **B-8 的地址口径**：验收原文写「配 8 MiB ⇒ 写 8 MiB 处**成功**、
//! 写 8 MiB 之外 ⇒ 架构陷阱」⇒ 这两条只有在「8 MiB 这个**地址**落在
//! 区域**里面**」时才能同时成立 ⇒ 本文件用的板是
//! 「低段 64 KiB（放镜像）+ `0x0001_0000` 起的 8 MiB RAM」⇒
//! `0x0080_0000`（8 MiB 那个地址）在区域**里面**、`0x0081_0000` 在
//! 区域**外面**。

use super::*;

use riscv_sim_contract::csr::{CSR_MCAUSE, CSR_MTVAL};
use riscv_sim_contract::model::{ProductTree, ProjectVersion};
use riscv_sim_contract::platform::{DeviceSpec, RegionSpec, PLATFORM_SCHEMA_VERSION};

use crate::m15_peripheral::gpio::GPIO_WINDOW;

// ---------------------------------------------------------------------------
// 夹具
// ---------------------------------------------------------------------------

/// 派发一条命令，返回它产生的事件。
fn dispatch(host: &mut KernelHost, cmd: CommandPayload) -> Vec<EventPayload> {
    host.handle_message(&Message::command(1, cmd))
}

/// 造一个挂着 demo 固件的工程。
///
/// ⚠️ demo 固件引用**宿主那一份共享常量**（`kernel_host::DEMO_FIRMWARE_BYTES`）
/// ⇒ 不抄第二份。
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

/// 造一个装着**一段裸机器码**的工程（镜像落在基址 0）。
fn raw_project(id: &str, bytes: Vec<u8>) -> Project {
    Project {
        id: id.to_string(),
        name: id.to_string(),
        tree: ProductTree::default(),
        images: vec![ImageRef {
            name: "raw.bin".to_string(),
            kind: ImageKind::RawHex,
            source: ImageSource::Bytes(bytes),
        }],
        version: ProjectVersion::default(),
    }
}

/// 取第一条 `MemoryMap` 事件（描述 + `pending` 标志）。
///
/// ⚠️ **v0.31（STAGE-16 裁决 M）**：事件**多两个**字段（`loaded` ·
/// `revision`）⇒ 这里用 `..` **只取**本文件要的那两个 ⇒ **既有断言一条
/// 不改**（新字段由 STAGE-16 那份用例单独钉）。
fn first_memory_map(events: &[EventPayload]) -> (PlatformDescription, bool) {
    for e in events {
        if let EventPayload::MemoryMap {
            description,
            pending,
            ..
        } = e
        {
            return (description.clone(), *pending);
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

/// 从引擎快照里读一个 CSR。
fn csr_value(host: &KernelHost, csr: u16) -> u32 {
    host.engine_snapshot()
        .state
        .csrs
        .iter()
        .find(|(num, _)| *num == csr)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 一张「用户配的板」——形状与内置档位**明显不同**。
///
/// ⚠️ 低段留了 64 KiB 给镜像（裸镜像落在基址 0）⇒ 否则装载会失败。
fn user_board() -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "user-board".to_string(),
        name: "User board".to_string(),
        priority: 100,
        // ⚠️ 它**不**参与自动选档（用户配的板是**点名**用的）。
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
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

/// 一张挂着**四种器件**的板（UART / CLINT / PLIC / GPIO）。
fn board_with_four_devices() -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "user-board-devices".to_string(),
        name: "User board (four devices)".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
        max_region_bytes: None,
        regions: vec![
            RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0001_0000,
            },
            RegionSpec::Peripheral {
                base: 0x7000_0000,
                device: DeviceSpec::Gpio {
                    id: "GPIO0".to_string(),
                },
            },
            RegionSpec::Peripheral {
                base: 0x7001_0000,
                device: DeviceSpec::Uart16550 {
                    id: "UART0".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                },
            },
            RegionSpec::Peripheral {
                base: 0x7002_0000,
                device: DeviceSpec::Clint {
                    id: "CLINT0".to_string(),
                    freq_hz: 10_000_000,
                    harts: 1,
                },
            },
            RegionSpec::Peripheral {
                base: 0x7004_0000,
                device: DeviceSpec::Plic {
                    id: "PLIC0".to_string(),
                    sources: 64,
                    contexts: 16,
                },
            },
        ],
    }
}

/// 一张「RAM 大小真的可定制」的板：低段 64 KiB + `0x0001_0000` 起的 8 MiB。
///
/// ⚠️ 8 MiB 那个**地址**（`0x0080_0000`）落在 8 MiB 区域**里面**
/// （区域 = `[0x0001_0000, 0x0081_0000)`）⇒ 「写 8 MiB 处成功」与
/// 「写 8 MiB 之外 ⇒ 陷阱」两条能**同时**成立。
fn board_with_eight_mib_ram() -> PlatformDescription {
    PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "user-board-8mib".to_string(),
        name: "User board (8 MiB RAM)".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
        max_region_bytes: None,
        regions: vec![
            RegionSpec::Ram {
                base: 0x0000_0000,
                size: 0x0001_0000,
            },
            RegionSpec::Ram {
                base: 0x0001_0000,
                size: 0x0080_0000,
            },
        ],
    }
}

/// 把「描述」与「总线上的实际内存图」**逐条**对账。
///
/// ⚠️ 外设窗口的**大小由器件自己报**（描述里没有这一项 —— 裁决 D）
/// ⇒ 外设只对「基址 + 种类」，RAM / ROM 对「基址 + 大小 + 种类」。
fn assert_bus_matches_description(description: &PlatformDescription, host: &KernelHost) {
    let regions = host.memory_map().regions;
    assert_eq!(
        regions.len(),
        description.regions.len(),
        "段数必须与描述逐条一致：{regions:?} vs {:?}",
        description.regions
    );
    for spec in &description.regions {
        let (base, kind) = match spec {
            RegionSpec::Ram { base, .. } => (*base, "ram"),
            RegionSpec::Rom { base, .. } => (*base, "rom"),
            RegionSpec::Peripheral { base, .. } => (*base, "peripheral"),
        };
        let found = regions
            .iter()
            .find(|r| r.base == base)
            .unwrap_or_else(|| panic!("总线上没有基址 {base:#x} 那一段：{regions:?}"));
        assert_eq!(found.kind, kind, "基址 {base:#x} 的种类必须与描述一致");
        match spec {
            RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => {
                assert_eq!(found.size, *size, "基址 {base:#x} 的大小必须与描述一致");
            }
            RegionSpec::Peripheral { .. } => {
                // 大小由器件自报 ⇒ 不在这里对账。
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 指令编码（B-8 的 guest 侧）
// ---------------------------------------------------------------------------

/// I 型编码。
fn enc_i(imm: i32, rs1: u8, funct3: u8, rd: u8, opcode: u8) -> u32 {
    (((imm as u32) & 0xFFF) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((rd as u32) << 7)
        | (opcode as u32)
}

/// U 型编码（`lui`）。
fn enc_u(imm: u32, rd: u8, opcode: u8) -> u32 {
    (imm << 12) | ((rd as u32) << 7) | (opcode as u32)
}

/// S 型编码（`sw` / `sb` / `sh`）。
fn enc_s(imm: u32, rs2: u8, rs1: u8, funct3: u8, opcode: u8) -> u32 {
    ((imm >> 5) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((funct3 as u32) << 12)
        | ((imm & 0x1F) << 7)
        | (opcode as u32)
}

/// 陷阱处理器的入口（`mtvec` 指向它）。
const TRAP_HANDLER: u32 = 0x40;
/// 处理器把 `mcause` 存到哪儿（低段 RAM 里）。
const MCAUSE_DUMP_ADDR: u32 = 0x0000_0100;
/// guest 要碰的那个**未映射**地址（落在 8 MiB RAM **之外**）。
const UNMAPPED_ADDR: u32 = 0x0081_0000;

/// 一段会**越界访存**的 guest 程序。
///
/// ```text
/// 0x00  lui  x1, 0x0            x1 = 0
/// 0x04  addi x1, x1, 0x40       x1 = 0x40（陷阱向量）
/// 0x08  csrw mtvec, x1          mtvec = 0x40
/// 0x0c  lui  x3, 0x810          x3 = 0x810000（未映射）
/// 0x10  lw   x4, 0(x3)          ← 越界读 ⇒ 架构陷阱
/// ...
/// 0x40  csrr x5, mcause         x5 = mcause
/// 0x44  sw   x5, 0x100(x0)      把 mcause 落到低段 RAM
/// 0x48  jal  x0, 0              自旋
/// ```
///
/// ⚠️ 编码**逐位**写死（不是从被测实现反推的）：`csrw mtvec, x1` =
/// `csrrw x0, 0x305, x1` ⇒ `0x30509073`；`csrr x5, mcause` =
/// `csrrs x5, 0x342, x0` ⇒ `0x34202F73`。
fn out_of_bounds_program(load: bool) -> Vec<u8> {
    let mut image = vec![0u8; 0x50];
    let mut put = |addr: u32, word: u32| {
        let at = addr as usize;
        image[at..at + 4].copy_from_slice(&word.to_le_bytes());
    };
    put(0x00, enc_u(0x000, 1, 0x37)); // lui  x1, 0x0
    put(0x04, enc_i(0x40, 1, 0, 1, 0x13)); // addi x1, x1, 0x40
    put(0x08, enc_i(0x305, 1, 1, 0, 0x73)); // csrw mtvec, x1
    put(0x0C, enc_u(0x810, 3, 0x37)); // lui  x3, 0x810
    if load {
        put(0x10, enc_i(0, 3, 2, 4, 0x03)); // lw  x4, 0(x3)
    } else {
        put(0x10, enc_s(0, 4, 3, 2, 0x23)); // sw  x4, 0(x3)
    }
    put(TRAP_HANDLER, enc_i(0x342, 0, 2, 5, 0x73)); // csrr x5, mcause
    put(TRAP_HANDLER + 4, enc_s(MCAUSE_DUMP_ADDR, 5, 0, 2, 0x23)); // sw   x5, 0x100(x0)
    put(TRAP_HANDLER + 8, 0x0000_006F); // jal  x0, 0（自旋）
    image
}

// ---------------------------------------------------------------------------
// B-1 · 未装载时立即生效 ⇒ 总线按新图重建
// ---------------------------------------------------------------------------

/// **B-1** —— `SetMemoryMap`（**未装载**）⇒ 总线**按新图重建**：段数 /
/// 基址 / 大小与描述**逐条一致**。
///
/// ⚠️ **不重建就红**：重建之前总线上是**内置默认档**（RAM `0` / RAM
/// `0x8000_0000` / GPIO `0x4000_0000`）⇒ 段数、基址、大小、种类全都
/// 对不上。
#[test]
fn b1_set_memory_map_before_loading_rebuilds_the_bus_to_match_the_description() {
    let mut host = KernelHost::new();
    let board = user_board();

    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(description, board, "事件带回来的必须是**这份**描述");
    assert!(
        !pending,
        "未装载 ⇒ 立即生效 ⇒ 没有「待生效」的（STAGE-14 §3.2 ③）"
    );

    // 总线按新图重建 ⇒ 与描述逐条一致。
    assert_bus_matches_description(&board, &host);
    // 旧默认档的那几段**必须没了**（重建 ⇒ 换掉，不是叠加）。
    let regions = host.memory_map().regions;
    assert!(
        !regions.iter().any(|r| r.base == 0x8000_0000),
        "默认档的高段 RAM 必须被换掉：{regions:?}"
    );
    assert!(
        !regions
            .iter()
            .any(|r| r.base == 0x4000_0000 && r.kind == "peripheral"),
        "默认档在 0x4000_0000 的 GPIO 必须被换掉：{regions:?}"
    );
}

/// **B-1（负向半）** —— 重建之后**旧默认档的段一个都不剩**，且描述里的
/// 段**一个都不少**（段数一致 ⇒ 没有多也没有少）。
#[test]
fn b1_the_rebuilt_bus_has_exactly_the_described_regions() {
    let mut host = KernelHost::new();
    let board = user_board();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );

    let regions = host.memory_map().regions;
    assert_eq!(regions.len(), 3, "描述里有三段 ⇒ 总线上也得是三段");
    let bases: Vec<u32> = regions.iter().map(|r| r.base).collect();
    assert_eq!(
        bases,
        vec![0x0000_0000, 0x5000_0000, 0x6000_0000],
        "基址必须逐条一致（总线按 base 升序导出）"
    );
    assert_eq!(regions[0].size, 0x0001_0000, "RAM 大小必须与描述一致");
    assert_eq!(regions[0].kind, "ram");
    assert_eq!(regions[1].size, 0x0000_1000, "ROM 大小必须与描述一致");
    assert_eq!(regions[1].kind, "rom");
    assert_eq!(regions[2].kind, "peripheral");
    assert_eq!(
        regions[2].size, GPIO_WINDOW,
        "外设窗口的大小由器件自己报（描述里没有这一项）"
    );
}

// ---------------------------------------------------------------------------
// 恢复路径 · 新图建不起来 ⇒ 旧图必须被恢复（STAGE-14 §3.2 ③）
// ---------------------------------------------------------------------------

/// **恢复路径** —— 描述**合法**但**建不起图**（两个器件的窗口互相重叠）
/// ⇒ 宿主把**原始错误**报出来，并**恢复旧图**。
///
/// ⚠️ **为什么必须恢复**：`rebuild_platform()` 的第一步就是
/// `reset_memory_map()` ⇒ 失败之后总线可能是**空的** ⇒ 不恢复的话用户
/// 拿到的是一张**空总线**（比报错更糟）。
#[test]
fn b1_the_old_map_is_restored_when_the_new_one_cannot_be_mapped() {
    let mut host = KernelHost::new();
    let before = host.memory_map();

    // CLINT 的窗口是 64 KiB、PLIC 的窗口是 2 MiB ⇒ 把 PLIC 放进 CLINT 的
    // 窗口里 ⇒ 建图时**重叠** ⇒ 建不起来。
    let overlapping = PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "overlapping".to_string(),
        name: "Overlapping".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        region_budget_bytes: 16 * 1024 * 1024,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
        max_region_bytes: None,
        regions: vec![
            RegionSpec::Peripheral {
                base: 0x7002_0000,
                device: DeviceSpec::Clint {
                    id: "CLINT0".to_string(),
                    freq_hz: 10_000_000,
                    harts: 1,
                },
            },
            RegionSpec::Peripheral {
                base: 0x7002_1000,
                device: DeviceSpec::Plic {
                    id: "PLIC0".to_string(),
                    sources: 64,
                    contexts: 16,
                },
            },
        ],
    };

    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: overlapping,
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_BUS-03",
        "建图失败必须报**原始**错误（窗口重叠 = E_BUS-03）：{events:?}"
    );

    // 旧图**恢复**了（不是一张空总线，也不是半张新图）。
    assert_eq!(
        host.memory_map(),
        before,
        "建图失败 ⇒ 旧图必须被恢复（不能留一张空总线）"
    );
    assert_eq!(host.platform_id(), "default", "当前图必须还是旧的");

    // ⚠️ **用户的板保留**（STAGE-14 §3.2 ③）：用户的板还在，只是**没生效**
    // ⇒ 「待生效」是**推导量**（用户的板 ≠ 当前生效的图）⇒ 它变真。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(
        description.id, "default",
        "当前生效的图必须还是旧的（用户的板没生效）"
    );
    assert!(
        pending,
        "建图失败 ⇒ 用户的板保留 ⇒ 用户的板还没生效 ⇒ 待生效为真"
    );
}

// ---------------------------------------------------------------------------
// B-2 · 描述不合规 ⇒ E_PLT-07，且什么都不动
// ---------------------------------------------------------------------------

/// **B-2** —— 描述不合规（声明 size 超过 `region_budget_bytes`）⇒
/// 报 **`E_PLT-07`**。
///
/// ⚠️ 判据是**族 + 码**（`PLT` + `07`）—— 契约层的 `CTX` 族**没有**
/// 具名场景码 ⇒ 只能这么判，不能靠 message 文本。
#[test]
fn b2_an_over_budget_description_is_rejected_with_e_plt_07() {
    let mut host = KernelHost::new();
    let bad = PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "too-big".to_string(),
        name: "Too big".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        // 预算 4 KiB，段却要 64 KiB ⇒ 违反 V-7。
        region_budget_bytes: 0x1000,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
        max_region_bytes: None,
        regions: vec![RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0001_0000,
        }],
    };

    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: bad,
            expected_revision: None,
        },
    );
    assert_eq!(
        first_error_code(&events),
        "E_PLT-07",
        "超预算必须是 E_PLT-07（族 PLT / 码 07）：{events:?}"
    );
}

/// **B-2（负向半）** —— 校验失败时**什么都不动**：当前的图没被污染，
/// 也没有「待生效」的配置被存下来。
#[test]
fn b2_a_rejected_description_changes_nothing() {
    let mut host = KernelHost::new();
    let bad = PlatformDescription {
        schema_version: PLATFORM_SCHEMA_VERSION,
        id: "too-big".to_string(),
        name: "Too big".to_string(),
        priority: 100,
        auto_selectable: false,
        description: None,
        // ⚠️ v0.34（D-125）：契约层给描述加了**可选**的 `wiring` 段 ⇒
        // 这里显式写「没有连线」⇒ 走**默认连线**（存量行为一字未变）。
        wiring: None,
        region_budget_bytes: 0x1000,
        // ⚠️ v0.31（D-138）：运行期上界**缺省** ⇒ 等于声明期那个
        max_region_bytes: None,
        regions: vec![RegionSpec::Ram {
            base: 0x0000_0000,
            size: 0x0001_0000,
        }],
    };
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: bad,
            expected_revision: None,
        },
    );

    // 当前生效的图**一字未动**（还是内置默认档），而且**用户的板没被记下**
    // ⇒ 没有待生效的。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(
        description,
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置默认档必须存在"),
        "不合规的描述不许污染当前图"
    );
    assert!(!pending, "不合规的描述不许被存成「待生效」");
    assert_bus_matches_description(&description, &host);
}

// ---------------------------------------------------------------------------
// B-3 · 已装载 ⇒ 不重建总线 + pending = true
// ---------------------------------------------------------------------------

/// **B-3** —— 已装载镜像时 `SetMemoryMap` ⇒ **不重建总线**（正在跑的
/// guest **一字不受影响**）+ 事件 `pending = true`。
#[test]
fn b3_set_memory_map_after_loading_is_pending_and_leaves_the_guest_alone() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(&mut host, CommandPayload::StepN { n: 3 });

    let before_state = host.engine_snapshot().state;
    let before_map = host.memory_map();

    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert!(
        pending,
        "已装载 ⇒ 新板只能「待生效」（STAGE-14 §2.2 裁决 B）：{events:?}"
    );
    assert_eq!(
        description.id, "default",
        "事件带的是**当前生效**的图（裁决 D），不是用户配的那份"
    );

    // 总线**没有**被重建。
    assert_eq!(host.memory_map(), before_map, "已装载时改板不许重建总线");

    // 正在跑的 guest **一字不受影响**。
    let after_state = host.engine_snapshot().state;
    assert_eq!(after_state.pc, before_state.pc, "PC 不能动");
    assert_eq!(after_state.gpr, before_state.gpr, "通用寄存器不能动");
    assert_eq!(
        after_state.virtual_time_ps, before_state.virtual_time_ps,
        "虚拟时间不能动"
    );
}

// ---------------------------------------------------------------------------
// B-4 · 配过板 ⇒ LoadProject 用用户的图建图（用户的板跨装载保持）
// ---------------------------------------------------------------------------

/// **B-4** —— 配过板时 `LoadProject` ⇒ **用用户的图**建图（**不**走自动
/// 选档）。
///
/// ⚠️ 这里是「**已装载**之后改板 ⇒ 待生效 ⇒ 重新装载」那一路。
#[test]
fn b4_load_project_uses_the_user_board_instead_of_auto_selection() {
    let mut host = KernelHost::new();
    // 先装载 ⇒ 之后的改板才是「待生效」。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(host.platform_id(), "default", "先走一次自动选档");

    let board = user_board();
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    assert!(
        first_memory_map(&events).1,
        "已装载时改板 ⇒ 用户的板还没生效 ⇒ 待生效为真"
    );

    // 重新装载 ⇒ 用**用户的图**建图。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        host.platform_id(),
        "user-board",
        "配过板时**不许**走自动选档"
    );
    assert_bus_matches_description(&board, &host);
}

/// **B-4（回归 · 本轮返工的核心判别式）** —— **装载前**配的板 ⇒ 装载工程
/// ⇒ 图**仍然是用户的板**。
///
/// ⚠️ **这条钉的是初版规格的那个洞**：初版把「待生效的那一份」在装载时
/// `take()` 掉 ⇒ **用户装载前配的板会被紧接着的那次 `LoadProject` 用自动
/// 选档覆盖掉** ⇒「装载前立即生效」成**一句空话**（配了板、看着生效了、
/// 一装载就白配）。
///
/// ⇒ 新模型：用户的板是**用户意图**，**跨 `LoadProject` 保持** ⇒ 装载前
/// 配的板**活过**装载，而且**每一次**装载都用它建图。
#[test]
fn b4_a_board_configured_before_loading_survives_the_load() {
    let mut host = KernelHost::new();
    let board = user_board();

    // ---- 装载**前**配板 ⇒ 未装载 ⇒ **立即生效** ----
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(
        description, board,
        "未装载时用户的板**立即生效**（§2.2 裁决 B）"
    );
    assert!(!pending, "已经生效了 ⇒ 没有待生效的");
    assert_eq!(host.platform_id(), "user-board");

    // ---- 装载工程 ⇒ **用户的板保持**（不被自动选档覆盖）----
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        host.platform_id(),
        "user-board",
        "装载前配的板必须活过装载（初版规格的洞）"
    );
    assert_bus_matches_description(&board, &host);

    // ---- 再装一次 ⇒ **还是**用户的板（跨 `LoadProject` 保持）----
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(
        host.platform_id(),
        "user-board",
        "用户的板跨 `LoadProject` 保持 ⇒ 第二次装载也用它"
    );
    assert_bus_matches_description(&board, &host);

    // ---- 「待生效」是**推导量**：用户的板 == 当前生效的图 ⇒ 不待生效 ----
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let (_, pending) = first_memory_map(&events);
    assert!(
        !pending,
        "用户的板就是当前生效的图 ⇒ 没有待生效的（推导量）"
    );
}

// ---------------------------------------------------------------------------
// B-5 · 用户从未配过板 ⇒ 自动选档（逐字不变）
// ---------------------------------------------------------------------------

/// **B-5** —— **用户从未配过板**时 `LoadProject` ⇒ 走今天的自动选档
/// ⇒ **逐字等价**（既有断言一条不改）。
#[test]
fn b5_load_project_without_a_user_board_still_auto_selects() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));

    // 自动选档挑中的是内置的 `default`（demo 镜像落在低段 ⇒ 两张
    // `auto_selectable` 的内置档都能装下 ⇒ **priority 小者**胜）。
    assert_eq!(host.platform_id(), "default");
    let expected =
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置默认档必须存在");
    assert_bus_matches_description(&expected, &host);
}

/// **B-5（正向）** —— 一个**从未配过板**的宿主 ⇒ 装载工程 ⇒ 走自动选档
/// ⇒ 图是**内置默认档**，而且**不是**任何用户板。
///
/// ⚠️ **条件是「从未配过板」**，不是「没有待生效配置」⇒ 本条是 B-5 在
/// 新模型下的**判别式**：只要用户配过一次板，就**再也不**自动选档。
#[test]
fn b5_a_host_that_never_configured_a_board_auto_selects() {
    let mut host = KernelHost::new();

    // 装载前先看一眼：当前生效的图是**内置默认档**，而且**没有**用户的板
    // ⇒ 「待生效」为假。
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(
        description,
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置默认档必须存在")
    );
    assert!(!pending, "从未配过板 ⇒ 没有待生效的");

    // 装载工程 ⇒ 自动选档。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_eq!(host.platform_id(), "default", "从未配过板 ⇒ 必须走自动选档");
    let expected =
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置默认档必须存在");
    assert_bus_matches_description(&expected, &host);
}

/// **B-5（负向半）** —— 装载**失败**时**不许**把「已装载」置真 ⇒ 之后的
/// `SetMemoryMap` 仍然**立即生效**（裁决 C 的判据不许撒谎）。
#[test]
fn b5_a_failed_load_does_not_mark_the_session_as_loaded() {
    let mut host = KernelHost::new();
    // 先把板换成「低段 64 KiB + 8 MiB」的那张 ⇒ 总共 8 MiB + 64 KiB。
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_with_eight_mib_ram(),
            expected_revision: None,
        },
    );
    // 一个**装不下**的镜像（9 MiB > 8 MiB + 64 KiB）⇒ 装载失败。
    let events = dispatch(
        &mut host,
        CommandPayload::LoadProject(raw_project("too-big", vec![0u8; 0x90_0000])),
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "装不下的镜像必须报错：{events:?}"
    );
    // 装载失败 ⇒ 「已装载」**保持假** ⇒ 之后的改板**立即生效**
    // （`pending = false`）。
    let events = dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: user_board(),
            expected_revision: None,
        },
    );
    let (_, pending) = first_memory_map(&events);
    assert!(
        !pending,
        "装载失败 ⇒ 会话仍算「未装载」⇒ 改板立即生效：{events:?}"
    );
}

// ---------------------------------------------------------------------------
// B-6 · 订阅即推送
// ---------------------------------------------------------------------------

/// **B-6** —— `Subscribe { topic: "memory.map" }` ⇒ **立即**收到一条
/// 当前生效的图。
#[test]
fn b6_subscribing_to_memory_map_pushes_the_current_map_right_away() {
    let mut host = KernelHost::new();
    let events = dispatch(
        &mut host,
        CommandPayload::Subscribe {
            topic: TOPIC_MEMORY_MAP.to_string(),
        },
    );
    let (description, pending) = first_memory_map(&events);
    assert_eq!(
        description,
        platform::builtin_description(platform::PLATFORM_ID_DEFAULT).expect("内置默认档必须存在"),
        "订阅时推的必须是**当前生效**的图"
    );
    assert!(!pending, "刚建好的会话没有待生效的板");
}

/// **B-6（topic 字面量）** —— topic 字符串必须与前端
/// `front/lib/message.js` 的 `EVENT_TOPIC.MemoryMap` **逐字一致**。
#[test]
fn b6_the_memory_map_topic_is_spelled_memory_map() {
    assert_eq!(TOPIC_MEMORY_MAP, "memory.map");
}

// ---------------------------------------------------------------------------
// B-7 · 器件真的挂上了
// ---------------------------------------------------------------------------

/// **B-7** —— 配的板里 **MMIO 器件真的挂上了**：四个器件**各自实例**在
/// 总线上（基址与 id 逐条一致），而且**每个都读得出东西**。
#[test]
fn b7_the_user_board_really_mounts_its_peripheral_devices() {
    let mut host = KernelHost::new();
    let board = board_with_four_devices();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );

    // 四个实例都在总线上，基址与器件 id 逐条一致。
    let instances: Vec<(u32, String)> = host
        .bus()
        .peripheral_instances()
        .iter()
        .map(|(base, p)| (*base, p.borrow().id().to_string()))
        .collect();
    assert_eq!(
        instances,
        vec![
            (0x7000_0000, "GPIO0".to_string()),
            (0x7001_0000, "UART0".to_string()),
            (0x7002_0000, "CLINT0".to_string()),
            (0x7004_0000, "PLIC0".to_string()),
        ],
        "四个器件实例必须**真的**挂在线上（基址 + id 逐条一致）"
    );

    // 每个实例**都读得出东西**（不是「描述里有这一条」）。
    for (base, id) in &instances {
        assert!(
            host.read_memory(*base, WriteSize::Byte).is_ok(),
            "器件 {id} 在 {base:#x} 必须可读"
        );
    }
}

/// **B-7（UART 往返）** —— UART 那个实例是**活的**：guest 写 THR 的字节
/// 真的进了宿主的 console 通道。
#[test]
fn b7_the_mounted_uart_really_carries_bytes() {
    let mut host = KernelHost::new();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_with_four_devices(),
            expected_revision: None,
        },
    );

    // THR 在偏移 0（`LCR.DLAB == 0` 时）。
    host.bus_mut()
        .write(0x7001_0000, BusWidth::Byte, u32::from(b'Z'))
        .expect("写 UART 的 THR 必须成功");
    assert_eq!(
        host.drain_console(),
        vec![b'Z'],
        "UART 实例必须真的把字节送进 console 通道"
    );
}

// ---------------------------------------------------------------------------
// B-8 · RAM 大小真的可定制
// ---------------------------------------------------------------------------

/// **B-8（总线层）** —— 配 8 MiB ⇒ **写 8 MiB 处成功**、写 8 MiB **之外**
/// ⇒ 失败（未映射）。
#[test]
fn b8_the_ram_size_is_really_customizable() {
    let mut host = KernelHost::new();
    // 先装载（⇒ 之后的改板是「待生效」），再改板，再重新装载 ⇒ 用用户的图。
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    let board = board_with_eight_mib_ram();
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board.clone(),
            expected_revision: None,
        },
    );
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    assert_bus_matches_description(&board, &host);

    // 8 MiB 那个地址落在 8 MiB 区域**里面** ⇒ 写成功。
    host.bus_mut()
        .write(0x0080_0000, BusWidth::Word, 0x1234_5678)
        .expect("写 8 MiB 处必须成功（它在配出来的 8 MiB RAM 里面）");
    assert_eq!(
        host.read_memory(0x0080_0000, WriteSize::Word)
            .expect("读 8 MiB 处"),
        0x1234_5678,
        "8 MiB 处的内容必须真的写进去了"
    );

    // 8 MiB 区域**之外** ⇒ 未映射 ⇒ 失败。
    let err = host
        .bus_mut()
        .write(0x0081_0000, BusWidth::Word, 0x1234_5678)
        .expect_err("8 MiB 之外必须写不进去");
    assert_eq!(
        err.family,
        ErrorFamily::Bus,
        "未映射的写是总线侧的错（族 BUS）：{err:?}"
    );
    assert_eq!(err.scenario, 1, "未映射 = E_BUS-01（场景码 1）");
}

/// **B-8（guest 层 · 越界读）** —— 真的跑一段越界访存的 guest 指令 ⇒
/// **架构陷阱 `mcause = 5`**（load access fault）。
#[test]
fn b8_an_unmapped_load_traps_with_cause_5() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_with_eight_mib_ram(),
            expected_revision: None,
        },
    );
    dispatch(
        &mut host,
        CommandPayload::LoadProject(raw_project(
            "out-of-bounds-load",
            out_of_bounds_program(true),
        )),
    );
    // ⚠️ 跑的是**用户的板**（8 MiB 那张）⇒ 陷阱的成因是「越过了这张板
    // 的 8 MiB RAM」，而不是「随便哪张板都越界」。
    assert_eq!(
        host.platform_id(),
        "user-board-8mib",
        "guest 必须跑在用户配的那张板上"
    );

    // ⚠️ **一批指令跑到陷阱就停**（M-17 的批处理语义：陷阱是**停止原因**）
    // ⇒ 陷阱处理器在**下一批**里才跑。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "越界访存是**架构陷阱**，不是宿主错误：{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Trap,
                ..
            }
        )),
        "这一批必须停在陷阱上：{events:?}"
    );

    assert_eq!(
        csr_value(&host, CSR_MCAUSE),
        5,
        "未映射的读 ⇒ 架构陷阱 cause 5（load access fault）"
    );
    assert_eq!(
        csr_value(&host, CSR_MTVAL),
        UNMAPPED_ADDR,
        "mtval = 出错的地址"
    );

    // 下一批 ⇒ 陷阱处理器真的跑了 ⇒ 它把 mcause 落到了低段 RAM。
    dispatch(&mut host, CommandPayload::StepN { n: 4 });
    assert_eq!(
        host.read_memory(MCAUSE_DUMP_ADDR, WriteSize::Word)
            .expect("读 mcause 的落点"),
        5,
        "陷阱处理器必须真的执行过（它把 mcause 存了下来）"
    );
}

/// **B-8（guest 层 · 越界写）** —— 越界**写** ⇒ **架构陷阱 `mcause = 7`**
/// （store access fault）。
#[test]
fn b8_an_unmapped_store_traps_with_cause_7() {
    let mut host = KernelHost::new();
    dispatch(&mut host, CommandPayload::LoadProject(demo_project()));
    dispatch(
        &mut host,
        CommandPayload::SetMemoryMap {
            description: board_with_eight_mib_ram(),
            expected_revision: None,
        },
    );
    dispatch(
        &mut host,
        CommandPayload::LoadProject(raw_project(
            "out-of-bounds-store",
            out_of_bounds_program(false),
        )),
    );
    // ⚠️ 同上：跑的是**用户的板**。
    assert_eq!(
        host.platform_id(),
        "user-board-8mib",
        "guest 必须跑在用户配的那张板上"
    );

    // ⚠️ **一批指令跑到陷阱就停**（M-17 的批处理语义）⇒ 陷阱处理器在
    // **下一批**里才跑。
    let events = dispatch(&mut host, CommandPayload::StepN { n: 12 });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventPayload::Error { .. })),
        "越界访存是**架构陷阱**，不是宿主错误：{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            EventPayload::Halted {
                reason: HaltReason::Trap,
                ..
            }
        )),
        "这一批必须停在陷阱上：{events:?}"
    );

    assert_eq!(
        csr_value(&host, CSR_MCAUSE),
        7,
        "未映射的写 ⇒ 架构陷阱 cause 7（store access fault）"
    );
    assert_eq!(
        csr_value(&host, CSR_MTVAL),
        UNMAPPED_ADDR,
        "mtval = 出错的地址"
    );

    dispatch(&mut host, CommandPayload::StepN { n: 4 });
    assert_eq!(
        host.read_memory(MCAUSE_DUMP_ADDR, WriteSize::Word)
            .expect("读 mcause 的落点"),
        7,
        "陷阱处理器必须真的执行过（它把 mcause 存了下来）"
    );
}
