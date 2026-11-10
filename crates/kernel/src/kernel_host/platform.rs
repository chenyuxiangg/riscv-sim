//! 平台描述的**加载** · **器件工厂** · **建图**（v0.27 · D-123-B）。
//!
//! 冻结规格：`docs/detailed-design/STAGE-12_V027_Platform_Description.md`
//! §1（格式）· §2.2（宿主落点）· §2.4（器件工厂）· §3（裁决 A~J）。
//!
//! ## 这个模块是什么
//!
//! v0.27 之前，宿主的内存图是**写死在代码里**的两档（`Default` /
//! `QemuVirt`）⇒ 地址是源码里的字面量、器件是源码里的构造调用 ⇒
//! 「换一块板子」＝「改宿主源码」。
//!
//! 本模块把内存图变成**数据**：一份**平台描述**（JSON）说清楚「有什么、
//! 在哪、参数是多少」⇒ 宿主按描述建图 ⇒ **换板子不用改宿主**。
//!
//! ## 三个「一个约束一个落点」（§2.5）
//!
//! | 约束 | 唯一落点 | 本模块**不得**新增的第二处 |
//! | --- | --- | --- |
//! | 描述的**校验** | 契约层 `PlatformDescription::validate` | **不自写一份校验**（直接调它） |
//! | 器件种类 → 构造 | 本模块的 [`build_device`] | **不在 `map_memory` 里再写一个 match** |
//! | 外设**窗口大小** | 器件自己的 `Peripheral::size()` | 描述里**不出现**窗口大小 |
//! | 每区域**预算** | 描述的 `region_budget_bytes` | **不按 `id` 硬编码**预算 |
//!
//! ## 内置描述为什么**编译进**二进制（裁决 F）
//!
//! WASM 产物是**单文件**（wasm-pack）⇒ 浏览器里**没有文件系统** ⇒
//! 运行时读文件不可能 ⇒ 内置描述**必须**在编译期进产物（`include_str!`）。
//!
//! ⚠️ **零逻辑改动**的诚实口径：运行时路径（[`KernelHost::new_with_platform_file`]）
//! 换平台**一字不改**；内置描述要加**一行** `include_str!` 登记（**数据
//! 登记**，不是逻辑改动）。
//!
//! ## 连线**进**描述（v0.34 · D-125 —— 取代 STAGE-12 裁决 G）
//!
//! ⚠️ **v0.27（STAGE-12 裁决 G）说的「连线不进描述」已经作废**：那时的
//! 理由是「描述覆盖**有什么、在哪、参数是多少**，不覆盖**谁连谁**」⇒ ⇒
//! 「UART 的 RX 就绪 → PLIC 的源 10」这条约束就**只**写在宿主代码里
//! （台账 **D-125**：两个数已经漂了 —— 描述里 `irq_line: 1` / 前端 10 /
//! 宿主 10，而描述里那个 1 **从来没生效过**）。
//!
//! **v0.34（冻结规格 STAGE-19 §3.1 · 裁决 E）**把连线收成**数据**：描述
//! 多了**可选**的 `wiring` 段 ⇒ ⇒ 宿主在**建图**时从**生效的描述**编译出
//! 一张**连线表** ⇒ ⇒ 「换一块板改连线」不用改宿主。
//!
//! ⚠️ **默认连线仍住在宿主里，但只有一处**（[`default_wiring`]）：那个写死
//! 的 PLIC source 常量**已删**（保留 = 又一个真相源）⇒ ⇒ 宿主源码里**没有
//! 第二处**写着那个源号。
//!
//! ⚠️ **`irq_line` 的语义不动**（STAGE-19 裁决 D）：它是 **M-15 内部中断
//! 线号**，**不是** PLIC source，且当前不接进任何行为（死字段）⇒ ⇒ 连线
//! **只**由 `wiring` 表达。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use riscv_sim_contract::error::{
    PlatformError, E_PLT_05_OVERFLOW, E_PLT_07_OVER_BUDGET, E_PLT_09_UNKNOWN_PLATFORM,
    E_PLT_10_PLATFORM_FILE_UNREADABLE, E_PLT_11_PLATFORM_FILE_MALFORMED, PLATFORM_FAMILY,
};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};
use riscv_sim_contract::port::{Peripheral, TimeSource};

use crate::m14_bus::Bus;
use crate::m15_peripheral::clint::Clint;
use crate::m15_peripheral::gpio::GpioBank;
use crate::m15_peripheral::pinmux::PinmuxDevice;
use crate::m15_peripheral::plic::Plic;
use crate::m15_peripheral::uart::Uart16550;

// ---------------------------------------------------------------------------
// 内置描述（编译期进产物 —— 裁决 F）
// ---------------------------------------------------------------------------

/// 内置描述 `default` —— **今天的 Default 档**，逐字复现。
pub const BUILTIN_JSON_DEFAULT: &str = include_str!("platforms/default.json");

/// 内置描述 `qemu-virt` —— **今天的 QemuVirt 档**，逐字复现。
pub const BUILTIN_JSON_QEMU_VIRT: &str = include_str!("platforms/qemu-virt.json");

/// 内置描述 `minimal` —— **换描述、宿主零改动**的证据（一份与上面两档
/// **都不同**的内存图）。
pub const BUILTIN_JSON_MINIMAL: &str = include_str!("platforms/minimal.json");

/// 内置描述 `default` 的 id（**平台主键**）。
pub const PLATFORM_ID_DEFAULT: &str = "default";

/// 内置描述 `qemu-virt` 的 id（**平台主键**）。
pub const PLATFORM_ID_QEMU_VIRT: &str = "qemu-virt";

/// 内置描述 `minimal` 的 id（**平台主键**）。
pub const PLATFORM_ID_MINIMAL: &str = "minimal";

// ---------------------------------------------------------------------------
// 宿主侧的三个场景码（E_PLT-09 .. E_PLT-11）—— **契约层所有**
// ---------------------------------------------------------------------------
//
// ⚠️ **码住在契约层**（`crates/contract/src/error.rs`）：规格 §1.6 的
// V-10 ~ V-12 是**宿主侧**的规则（按 id 取不到描述 / 文件读不到 / 文件
// 解析不了），但**码归契约层所有** ⇒ 宿主**只引用**，**不自建** ⇒
// 同一个码在两处出现 = 一个约束两个落点。
//
// ⇒ 这里**不再**定义本地常量，只从契约层**引用**。

// ---------------------------------------------------------------------------
// 加载
// ---------------------------------------------------------------------------

/// 全部内置描述 —— **(id, JSON 原文)**。
///
/// ⚠️ **顺序是内置清单的稳定顺序**：**v0.76（STAGE-52 · B3）**自动选档
/// 已废 ⇒ ⇒ 顺序只决定**模板清单**的展示次序（裁决 M）⇒ ⇒ 必须**稳定**。
const BUILTIN_DESCRIPTIONS: &[(&str, &str)] = &[
    (PLATFORM_ID_DEFAULT, BUILTIN_JSON_DEFAULT),
    (PLATFORM_ID_QEMU_VIRT, BUILTIN_JSON_QEMU_VIRT),
    (PLATFORM_ID_MINIMAL, BUILTIN_JSON_MINIMAL),
];

/// 解析一段描述文本 —— **唯一的解析器**。
///
/// ⚠️ 内置路径与文件路径**共用这一个解析器 + 契约层那一个校验器**
/// ⇒ 没有两份语义（§2.5 对照表第一行）。
///
/// ⚠️ **serde 的错误**（V-8：未知器件种类 / 缺参数 / 参数类型错）在**解析
/// 阶段**就失败 ⇒ 报 [`E_PLT_11_PLATFORM_FILE_MALFORMED`]；**声明规则**的
/// 错误（V-1 ~ V-7 / V-9）由契约层的 `validate` 报 ⇒ `E_PLT-01` ..
/// `E_PLT-08`。
pub fn parse_description(json: &str) -> Result<PlatformDescription, PlatformError> {
    let description: PlatformDescription = serde_json::from_str(json).map_err(|err| {
        PlatformError::new(
            PLATFORM_FAMILY,
            E_PLT_11_PLATFORM_FILE_MALFORMED,
            format!("平台描述不是合法的 JSON：{err}"),
        )
    })?;
    description.validate()?;
    Ok(description)
}

/// 按 id 取**内置描述**（解析 + 校验）。
pub fn builtin_description(id: &str) -> Result<PlatformDescription, PlatformError> {
    let Some((_, json)) = BUILTIN_DESCRIPTIONS
        .iter()
        .find(|(builtin_id, _)| *builtin_id == id)
    else {
        return Err(PlatformError::new(
            PLATFORM_FAMILY,
            E_PLT_09_UNKNOWN_PLATFORM,
            format!("没有 id 为 {id} 的内置平台描述"),
        ));
    };
    parse_description(json)
}

/// 全部**内置描述**（**模板清单** · v0.76 · STAGE-52 · 裁决 M）。
///
/// ⚠️ **这里返回的是「全部」**：前端模板库**查**这份清单 ⇒ ⇒ 模板导入
///   由**用户显式选一块** ⇒ ⇒ 宿主**不按 id 硬编码**候选集 ⇒ ⇒
///   `auto_selectable` **已废弃**（B3 · 字段删除）。
pub fn builtin_descriptions() -> Result<Vec<PlatformDescription>, PlatformError> {
    BUILTIN_DESCRIPTIONS
        .iter()
        .map(|(_, json)| parse_description(json))
        .collect()
}

/// 从**文件路径**读一份描述（运行时路径 —— 裁决 F）。
///
/// ⚠️ 与内置路径**共用** [`parse_description`] ⇒ **同一个解析器 + 同一个
/// 校验器**。
pub fn description_from_file(path: &Path) -> Result<PlatformDescription, PlatformError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        PlatformError::new(
            PLATFORM_FAMILY,
            E_PLT_10_PLATFORM_FILE_UNREADABLE,
            format!("平台描述文件 {} 读不到：{err}", path.display()),
        )
    })?;
    parse_description(&text)
}

/// 描述的区域，按 **base 升序**。
///
/// ⚠️ **为什么需要它**：总线的 `export_map()` 遍历的是 `BTreeMap` ⇒
/// 天然按 base 升序 ⇒ 「描述的第 i 个区域」与「导出地图的第 i 个区域」
/// 要能一一对应 ⇒ 这里给出**同样顺序**的视图（描述本身允许乱序）。
///
/// ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段排**前面**（`None` <
/// `Some`）⇒ ⇒ 排序**稳定** ⇒ ⇒ 缺基址的段之间**保持声明顺序** ⇒ ⇒ ⇒
/// **可复算**。
///
/// ⚠️ **调用方必须先分配基址**（[`assign_bases`]）⇒ ⇒ 否则「第一个
/// UART」那个口径**没有意义**（它还没有位置）⇒ ⇒ ⇒ 那条纪律由**调用
/// 时序**保证（[`map_platform`] 里**第一句**就是分配）。
pub fn regions_by_base(description: &PlatformDescription) -> Vec<&RegionSpec> {
    let mut regions: Vec<&RegionSpec> = description.regions.iter().collect();
    regions.sort_by_key(|region| region.base());
    regions
}

/// 描述里某个**外设器件**的基址（按器件 `id` 查）。
///
/// ⚠️ 「找不到 ⇒ `None`」：一份描述里可以有**好几个**器件 ⇒ 按 `id`
/// 定位是唯一的办法（基址不会重复 —— 重叠会被校验规则 V-6 拒掉）。
pub fn device_base(description: &PlatformDescription, device_id: &str) -> Option<u32> {
    description
        .regions
        .iter()
        .filter(|region| region.device_id() == Some(device_id))
        .filter_map(|region| region.base())
        .next()
}

// ---------------------------------------------------------------------------
// 器件工厂（§2.4）
// ---------------------------------------------------------------------------

/// 一个**造好的器件**。
///
/// ⚠️ **为什么它带两样东西**：规格 §2.4 写的是「工厂返回
/// `Rc<RefCell<dyn Peripheral>>`」⇒ 但宿主**还要**留住 UART0 / CLINT /
/// PLIC 的**具体类型**句柄（`drain_tx` / `mtimecmp_ps` / `claim` **不是**
/// `Peripheral` trait 的方法）⇒ 而 `Rc<RefCell<dyn Peripheral>>` **无法**
/// 再转回具体类型（`RefCell<T>` 对 `T` 不变，`Rc::downcast` 只认
/// `dyn Any`）⇒ 工厂返回**两者**：给总线的 trait object 视图 + 给宿主的
/// 具体类型句柄。
///
/// ⚠️ **这不影响「一个 match、一个落点」**：器件种类的 `match` **只有**
/// [`build_device`] 那一处 ⇒ 这里只是**搬运**，不再判断种类。
pub struct BuiltDevice {
    /// 给**总线**的 trait object 视图（`map_peripheral_instance` 要它）。
    peripheral: Rc<RefCell<dyn Peripheral>>,
    /// 宿主留着的 UART 句柄（`None` = 这个器件不是 UART）。
    uart: Option<Rc<RefCell<Uart16550>>>,
    /// 宿主留着的 CLINT 句柄。
    clint: Option<Rc<RefCell<Clint>>>,
    /// 宿主留着的 PLIC 句柄。
    plic: Option<Rc<RefCell<Plic>>>,
    /// 宿主留着的 **GPIO** 句柄（v0.74 · STAGE-50 · 引脚级连线的源）。
    ///
    /// ⚠️ **为什么 GPIO 现在要留句柄**（v0.73 批它是 `Opaque`）：引脚级连线
    ///   要**读它的 pending IRQ**（⇒ ⇒ 抬 PLIC 的源）⇒ ⇒ ⇒ **拿不到句柄
    ///   就抬不了** ⇒ ⇒ ⇒ **一个约束一个落点**：句柄**必须**在这儿。
    gpio: Option<Rc<RefCell<GpioBank>>>,
    /// 宿主留着的 **pinmux** 句柄（v0.76 · STAGE-52 · C1 决议）。
    ///
    /// ⚠️ **为什么 pinmux 要留句柄**：固件**写它的寄存器**切当前功能、宿主
    ///   **读当前功能**查「功能 → 中断源」⇒ ⇒ 拿不到句柄就切不了、抬不了
    ///   ⇒ ⇒ **一个约束一个落点**：句柄**必须**在这儿。
    pinmux: Option<Rc<RefCell<PinmuxDevice>>>,
}

impl BuiltDevice {
    /// 一个**宿主不直接碰**的器件。
    fn opaque<P>(device: P) -> Self
    where
        P: Peripheral + 'static,
    {
        BuiltDevice {
            peripheral: Rc::new(RefCell::new(device)),
            uart: None,
            clint: None,
            plic: None,
            gpio: None,
            pinmux: None,
        }
    }

    /// UART —— 宿主要留句柄（console 方向）。
    fn uart(device: Uart16550) -> Self {
        let handle = Rc::new(RefCell::new(device));
        BuiltDevice {
            peripheral: handle.clone(),
            uart: Some(handle),
            clint: None,
            plic: None,
            gpio: None,
            pinmux: None,
        }
    }

    /// CLINT —— 宿主要留句柄（`mtimecmp` 喂给 M-16）。
    fn clint(device: Clint) -> Self {
        let handle = Rc::new(RefCell::new(device));
        BuiltDevice {
            peripheral: handle.clone(),
            uart: None,
            clint: Some(handle),
            plic: None,
            gpio: None,
            pinmux: None,
        }
    }

    /// **GPIO** —— 宿主要留句柄（**引脚级连线**的源 · v0.74 · STAGE-50）。
    fn gpio(device: GpioBank) -> Self {
        let handle = Rc::new(RefCell::new(device));
        BuiltDevice {
            peripheral: handle.clone(),
            uart: None,
            clint: None,
            plic: None,
            gpio: Some(handle),
            pinmux: None,
        }
    }

    /// PLIC —— 宿主要留句柄（外部中断的源头）。
    fn plic(device: Plic) -> Self {
        let handle = Rc::new(RefCell::new(device));
        BuiltDevice {
            peripheral: handle.clone(),
            uart: None,
            clint: None,
            plic: Some(handle),
            gpio: None,
            pinmux: None,
        }
    }
}

/// 描述 → 器件实例。**一个 match、一个落点**（§2.4）。
///
/// ⚠️ **新增一种器件种类 ⇒ 必须改这一个 match** ⇒ 这是**不可消除**的
/// 边界 ⇒ 规格明说：**平台（内存图）零代码，器件种类要代码**。
fn build_device(spec: &DeviceSpec, base: u32) -> Result<BuiltDevice, PlatformError> {
    Ok(match spec {
        DeviceSpec::Gpio { id, .. } => BuiltDevice::gpio(GpioBank::new(id.clone(), base)),
        DeviceSpec::Uart16550 {
            id,
            irq_line,
            baud_rate,
        } => BuiltDevice::uart(Uart16550::new(id.clone(), base, *baud_rate, *irq_line)),
        DeviceSpec::Clint { id, freq_hz, harts } => {
            BuiltDevice::clint(Clint::with_frequency(id.clone(), base, *harts, *freq_hz))
        }
        DeviceSpec::Plic {
            id,
            sources,
            contexts,
        } => BuiltDevice::plic(Plic::with_capacity(id.clone(), base, *sources, *contexts)),
        // ⚠️ **v0.73（STAGE-49 · G）**：这台器件**内核早就实现了**（M-15
        //   注册表里的第五个类型）⇒ ⇒ 这里只是让**描述能写出它** ⇒ ⇒ 参数
        //   **逐字**对齐 `TimerRiscv::new(id, base, irq_line)`。
        DeviceSpec::TimerRiscv { id, irq_line } => BuiltDevice::opaque(
            crate::m15_peripheral::timer::TimerRiscv::new(id.clone(), base, *irq_line),
        ),
    })
}

// ---------------------------------------------------------------------------
// 器件实例表（STAGE-48 · v0.72 · D-177 B）
// ---------------------------------------------------------------------------

/// 宿主手里**真正持有的**器件句柄 —— 实例表里的那一格。
///
/// ⚠️ **它取代了「三个固定角色句柄」**（裁决 A）⇒ ⇒ 「板上的器件实例」
/// **不再**被塌缩成 `uart0` / `clint` / `plic` ⇒ ⇒ **第二个 UART 也有
/// 句柄** ⇒ ⇒ 第二个 UART 的线**真的**能通电（裁决 E）。
#[derive(Debug, Clone)]
pub enum DeviceHandle {
    /// UART16550 —— 宿主能从它抬 `rx_ready`，也能当 console。
    Uart(Rc<RefCell<Uart16550>>),
    /// CLINT —— `time` / `mtimecmp` 的**唯一**来源。
    Clint(Rc<RefCell<Clint>>),
    /// PLIC —— 外部中断的源头。
    Plic(Rc<RefCell<Plic>>),
    /// **GPIO**（v0.74 · STAGE-50 · 引脚级连线的源）。
    ///
    /// ⚠️ v0.73 批它是 `Opaque`（宿主**不**持有专用句柄）⇒ ⇒ 引脚级连线
    ///   **抬不了** ⇒ ⇒ ⇒ 现在**必须**持有句柄（一个约束一个落点）。
    Gpio(Rc<RefCell<GpioBank>>),
    /// **pinmux**（v0.76 · STAGE-52 · C1 决议 —— 功能切换 + 中断路由）。
    Pinmux(Rc<RefCell<PinmuxDevice>>),
    /// 宿主**不**持有专用句柄的器件（今天的 `timer_riscv`）⇒ ⇒ 只登记
    /// 「**它在**」⇒ ⇒ 「它能不能当线的两端」**判得出来**（⇒ ⇒ `source-type` /
    /// `target-type` 而不是 `source-unknown`）。
    Opaque,
}

/// 器件实例表 —— **按 id 索引**（STAGE-48 · v0.72 · D-177 B）。
///
/// ⚠️ **它与建图结果同源**（在**同一次**遍历里产出）⇒ ⇒ 不是「另算一遍」
/// ⇒ ⇒ 不可能与总线上的实际布局对不上。
///
/// ⚠️ **顺序 = base 升序**（建图遍历的顺序）⇒ ⇒ 「第一个 UART / 第一个
/// PLIC」与建图时**同序** ⇒ ⇒ **存量行为逐字不变**（裁决 D）。
#[derive(Debug, Default)]
pub struct DeviceInstanceTable {
    entries: Vec<DeviceInstanceEntry>,
}

/// **角色解析**的结果（STAGE-49 · v0.73 · D-177 的 C）。
///
/// ⚠️ **角色 = 器件 id**（不是句柄）⇒ ⇒ 句柄**一律**从实例表按 id 查
/// ⇒ ⇒ 表是**唯一真相源**（裁决 B）。
#[derive(Debug, Clone, Default)]
pub struct ResolvedRoles {
    /// **console** 角色的器件 id（`None` ⇒ 这块板**没有** UART）。
    pub console: Option<String>,
    /// **时间源**角色的器件 id（`None` ⇒ 没有 CLINT）。
    pub time_source: Option<String>,
    /// **主 PLIC** 角色的器件 id。
    ///
    /// ⚠️ **`None` ⇒ 不限** ⇒ ⇒ **所有** PLIC 都抬外部线 ⇒ ⇒ **今天的
    /// 行为逐字不变**（v0.72 批裁决 F）。
    pub main_plic: Option<String>,
}

/// **角色解析**（STAGE-49 · D-177 的 C）：描述里的 `roles` **优先**；
/// **缺省** ⇒ ⇒ **今天的口径**（第一个 UART / 第一个 CLINT）。
///
/// ⚠️ **缺省行为逐字不变**（裁决 E）⇒ ⇒ 这是**加法**⇒ ⇒ 存量板档案
/// **零影响**。
///
/// ⚠️ 描述里点名了**不存在**或**类型不对**的器件 ⇒ ⇒ **在校验阶段就被拒**
/// （契约层 V-20 · `E_PLT-18`）⇒ ⇒ ⇒ **这里不可能拿到坏 id** ⇒ ⇒ ⇒
/// **不静默回落**。
pub fn resolve_roles(
    description: &PlatformDescription,
    devices: &DeviceInstanceTable,
) -> ResolvedRoles {
    let declared = description.roles.as_ref();
    ResolvedRoles {
        console: declared
            .and_then(|r| r.console.clone())
            .or_else(|| devices.first_uart().map(|e| e.id.clone())),
        time_source: declared
            .and_then(|r| r.time_source.clone())
            .or_else(|| devices.first_clint().map(|e| e.id.clone())),
        main_plic: declared.and_then(|r| r.main_plic.clone()),
    }
}

/// 实例表里的一格。
#[derive(Debug, Clone)]
pub struct DeviceInstanceEntry {
    /// 器件 id（描述里的那个 id）。
    pub id: String,
    /// 器件种类（`"uart16550"` / `"plic"` / `"clint"` / `"gpio"`）。
    pub kind: String,
    /// 宿主持有的句柄。
    pub handle: DeviceHandle,
}

impl DeviceInstanceTable {
    /// 登记一个实例（**建图遍历的顺序** = base 升序 ⇒ ⇒ 「第一个」口径）。
    fn push(&mut self, entry: DeviceInstanceEntry) {
        self.entries.push(entry);
    }

    /// 按 id 查一格。
    pub fn get(&self, id: &str) -> Option<&DeviceInstanceEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 按 id 查一个 **UART** 句柄。
    pub fn uart(&self, id: &str) -> Option<Rc<RefCell<Uart16550>>> {
        match self.get(id).map(|e| &e.handle) {
            Some(DeviceHandle::Uart(h)) => Some(Rc::clone(h)),
            _ => None,
        }
    }

    /// 按 id 查一个 **PLIC** 句柄。
    pub fn plic(&self, id: &str) -> Option<Rc<RefCell<Plic>>> {
        match self.get(id).map(|e| &e.handle) {
            Some(DeviceHandle::Plic(h)) => Some(Rc::clone(h)),
            _ => None,
        }
    }

    /// 按 id 查一个 **CLINT** 句柄。
    pub fn clint(&self, id: &str) -> Option<Rc<RefCell<Clint>>> {
        match self.get(id).map(|e| &e.handle) {
            Some(DeviceHandle::Clint(h)) => Some(Rc::clone(h)),
            _ => None,
        }
    }

    /// 按 id 查一个 **GPIO** 句柄（**引脚级连线的源** · v0.74 · STAGE-50）。
    ///
    /// ⚠️ **为什么 GPIO 现在有句柄**（v0.73 批它是 `Opaque`）：引脚级连线要
    ///   **读它的 pending IRQ** ⇒ ⇒ ⇒ **拿不到句柄就抬不了**。
    pub fn gpio(&self, id: &str) -> Option<Rc<RefCell<GpioBank>>> {
        match self.get(id).map(|e| &e.handle) {
            Some(DeviceHandle::Gpio(h)) => Some(Rc::clone(h)),
            _ => None,
        }
    }

    /// 按 id 查一个 **pinmux** 句柄（v0.76 · STAGE-52 · C1 决议）。
    pub fn pinmux(&self, id: &str) -> Option<Rc<RefCell<PinmuxDevice>>> {
        match self.get(id).map(|e| &e.handle) {
            Some(DeviceHandle::Pinmux(h)) => Some(Rc::clone(h)),
            _ => None,
        }
    }

    /// 遍历所有 **pinmux** 句柄（v0.76 · STAGE-52 —— 装配期喂功能数）。
    pub fn iter_pinmux(&self) -> Vec<Rc<RefCell<PinmuxDevice>>> {
        self.entries
            .iter()
            .filter_map(|e| match &e.handle {
                DeviceHandle::Pinmux(h) => Some(Rc::clone(h)),
                _ => None,
            })
            .collect()
    }

    /// **第一个** UART（base 升序）—— **console 角色**（裁决 D：逐字不变）。
    pub fn first_uart(&self) -> Option<&DeviceInstanceEntry> {
        self.entries.iter().find(|e| e.kind == "uart16550")
    }

    /// **第一个** PLIC（base 升序）。
    pub fn first_plic(&self) -> Option<&DeviceInstanceEntry> {
        self.entries.iter().find(|e| e.kind == "plic")
    }

    /// **第一个** CLINT（base 升序）—— **时间角色**（裁决 D：逐字不变）。
    pub fn first_clint(&self) -> Option<&DeviceInstanceEntry> {
        self.entries.iter().find(|e| e.kind == "clint")
    }

    /// **全部** PLIC 句柄（base 升序）—— 裁决 F：外部线由**任一** PLIC 抬。
    pub fn plic_handles(&self) -> Vec<Rc<RefCell<Plic>>> {
        self.entries
            .iter()
            .filter(|e| e.kind == "plic")
            .filter_map(|e| match &e.handle {
                DeviceHandle::Plic(h) => Some(Rc::clone(h)),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 建图
// ---------------------------------------------------------------------------

/// 建完图之后，宿主**要留着的**器件句柄。
///
/// ⚠️ **v0.72（STAGE-48 · D-177 B）**：三个固定角色句柄（`uart0` /
/// `clint` / `plic`）**已删** ⇒ ⇒ 代之以 [`DeviceInstanceTable`]（**按 id
/// 索引**）⇒ ⇒ 「板上的器件实例」**不再**被塌缩成三个角色。
///
/// ⚠️ **角色 = id，句柄 = 从表里查出来的**（裁决 B）⇒ ⇒ 表是**唯一真相
/// 源** ⇒ ⇒ 「console 是哪个 / 时间源是哪个」只由**角色 id**决定。
#[derive(Debug, Default)]
pub struct MappedDevices {
    /// **逐实例**清单（STAGE-43 · v0.66 · D-175）—— 谁、什么类型、绑到
    /// 哪个基址、窗口多大。
    ///
    /// ⚠️ **它与建图结果同源**（在**同一次**遍历里产出）⇒ ⇒ 不是「另算
    /// 一遍」⇒ ⇒ 不可能与总线上的实际布局对不上。
    pub instances: Vec<riscv_sim_contract::model::DeviceInstanceInfo>,
    /// **按 id 索引**的器件实例表 —— 宿主手里**真正持有**的那些句柄。
    pub handles: DeviceInstanceTable,
    /// **角色解析**的结果（STAGE-49 · D-177 的 C）—— 哪个 id 扮演哪个角色。
    pub roles: ResolvedRoles,
}

/// 按描述**建图**。
///
/// 形状（§2.2）：
///
/// ```text
/// 描述 → 校验（契约层） → 逐条区域：
///     ram        → bus.map_ram(base, size, &[])
///     rom        → bus.map_rom(base, size, &[])
///     peripheral → 工厂造实例 → bus.map_peripheral_instance(base, 实例.size(), 实例)
/// 护栏预算 → bus.set_max_region_bytes(描述.max_region_bytes ?? 描述.region_budget_bytes)
/// ```
///
/// ⚠️ **校验在建图之前**：不合规的描述**一个区域都不留**（B-5）⇒
/// 「声明一段超预算的 RAM」在**建图前**就被 `E_PLT-07` 拒掉。
///
/// ⚠️ **外设窗口大小由器件自己报**（`实例.size()`）⇒ 描述里没有窗口
/// 大小（裁决 D）⇒ 没有第二个真相源。
pub fn map_platform(
    bus: &mut Bus,
    description: &PlatformDescription,
) -> Result<MappedDevices, PlatformError> {
    // ⚠️ **v0.73（STAGE-49 · B2）**：**先分配基址**，**再**校验，**再**建图。
    //
    // ⚠️ **为什么分配在校验之前**（§7.1 订正 6 ①）：契约层的 V-5（溢出）
    // 与 V-6（重叠）都**要基址**⇒ ⇒ 缺基址的段**判不了**⇒ ⇒ ⇒ 分配
    // 完了才判得出来 ⇒ ⇒ ⇒ **分配器自己**判「分配出来的图重叠不重叠」
    // ⇒ ⇒ ⇒ 仍报**既有码 `E_PLT-06`**（⇒ ⇒ **零新场景码**）。
    //
    // ⚠️ **老描述 ⇒ 分配是恒等变换**（⇒ ⇒ **一个字节都不变**）⇒ ⇒
    // 存量板档案**零风险**（裁决 K）。
    let assigned = assign_bases(description)?;
    // 校验**先**于建图 ⇒ 不合规的描述不留任何区域。
    assigned.validate()?;
    let description = &assigned;

    // 护栏预算：描述说了算（§2.5 对照表最后一行）。
    //
    // ⚠️ **v0.31（STAGE-16 §3.5 · D-138）**：**运行期**上界 =
    // `max_region_bytes`（缺省 = **声明期**那个）⇒ 与拆字段之前**逐字
    // 相同** ⇒ 既有断言（「`bus.max_region_bytes() == region_budget_bytes`」）
    // **一条不改**。
    //
    // ⚠️ **两层护栏各管各的**：声明期那条判 V-7（`E_PLT_07`），运行期这条
    // 判**物理占用**（`E_BUS_09`）⇒ 两个码**故意不复用**。
    bus.set_max_region_bytes(u64::from(
        description
            .max_region_bytes
            .unwrap_or(description.region_budget_bytes),
    ));

    let mut devices = MappedDevices::default();
    // ⚠️ 按 **base 升序**建图（与总线的 `BTreeMap` 顺序一致）⇒ 建图
    // 顺序是**确定的**，不依赖描述里的书写顺序。
    for region in regions_by_base(description) {
        // ⚠️ **v0.73（STAGE-49 · B2）**：基址**一定已经分配好了**
        // （[`assign_bases`] 是**第一句**）⇒ ⇒ 这里**不该**是 `None` ⇒ ⇒
        // 但**不静默**编一个（⇒ ⇒ 编一个 = 第二个真相源）⇒ ⇒ ⇒ 报出来。
        let Some(base) = region.base() else {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_05_OVERFLOW,
                "区域没有基址（装配期分配没有生效）",
            ));
        };
        match region {
            RegionSpec::Ram { size, .. } => {
                bus.map_ram(base, *size, &[])?;
            }
            RegionSpec::Rom { size, .. } => {
                bus.map_rom(base, *size, &[])?;
            }
            RegionSpec::Peripheral { device, .. } => {
                let BuiltDevice {
                    peripheral,
                    uart,
                    clint,
                    plic,
                    gpio,
                    pinmux,
                } = build_device(device, base)?;
                // ⚠️ 窗口大小**由器件自己报**（裁决 D）。
                let window = peripheral.borrow().size();
                bus.map_peripheral_instance(base, window, peripheral)?;
                // ⚠️ **逐实例**清单（STAGE-43 · 裁决 K / L）—— 与建图**同源**
                // ⇒ ⇒ 「谁绑在哪儿」**不需要**另算一遍。
                devices
                    .instances
                    .push(riscv_sim_contract::model::DeviceInstanceInfo {
                        id: device.id().to_string(),
                        kind: device.kind().to_string(),
                        base: Some(base),
                        window_bytes: window,
                    });
                // ⚠️ **实例表**（STAGE-48 · D-177 B）：**每一个**器件实例都
                // 进表 ⇒ ⇒ **第二个 UART 也有句柄** ⇒ ⇒ 它的线**真的**能
                // 通电（裁决 E）。
                //
                // ⚠️ **GPIO 没有专用句柄** ⇒ ⇒ 记成 `Opaque` ⇒ ⇒ 「它在」
                // 这件事**判得出来**（⇒ ⇒ 线的两端是 GPIO 时报
                // `source-type` / `target-type`，**不是** `source-unknown`）。
                let handle = match (uart, clint, plic, gpio, pinmux) {
                    (Some(u), None, None, None, None) => DeviceHandle::Uart(u),
                    (None, Some(c), None, None, None) => DeviceHandle::Clint(c),
                    (None, None, Some(p), None, None) => DeviceHandle::Plic(p),
                    (None, None, None, Some(g), None) => DeviceHandle::Gpio(g),
                    (None, None, None, None, Some(x)) => DeviceHandle::Pinmux(x),
                    _ => DeviceHandle::Opaque,
                };
                devices.handles.push(DeviceInstanceEntry {
                    id: device.id().to_string(),
                    kind: device.kind().to_string(),
                    handle,
                });
            }
        }
    }
    // ⚠️ **角色解析**（STAGE-49 · D-177 的 C）：描述里的 `roles` **优先**，
    // **缺省** ⇒ ⇒ 今天的口径（第一个 UART / 第一个 CLINT）⇒ ⇒ **逐字不变**。
    devices.roles = resolve_roles(description, &devices.handles);

    Ok(devices)
}

// ---------------------------------------------------------------------------
// 时钟适配器（D-151 · 冻结规格 STAGE-26 §3.2）
// ---------------------------------------------------------------------------

/// **CLINT ⇒ `TimeSource`** 的适配器（D-151）。
///
/// ⚠️ **它包的是宿主已经持有的那一个 CLINT 句柄** ⇒ ⇒ 与总线上的那个
/// CLINT 是**同一个实例** ⇒ ⇒ `time` 与 `mtime` 是**同一份**读数 ⇒ ⇒
/// **不另造一个 CLINT**（那会变成两份真相）。
///
/// ⚠️ **用 `try_borrow`** ⇒ ⇒ 争用时**不 panic**、降级为 **0** ⇒ ⇒ 与
/// [`BoundPeripheral::instance_id`](crate::m14_bus::BoundPeripheral::instance_id)
/// **同形**。
///
/// ⚠️ **单位 = 周期（tick）** ⇒ ⇒ 与 CLINT 的 `mtime_cycles()` 逐字相同
/// ⇒ ⇒ 不是 ps、不是指令数（STAGE-26 裁决 G）。
pub struct ClintTimeSource {
    /// 宿主持有的**那一个** CLINT 句柄。
    clint: Rc<RefCell<Clint>>,
}

impl TimeSource for ClintTimeSource {
    fn time_ticks(&self) -> u64 {
        // ⚠️ **争用 ⇒ 0，不 panic**：平台的规矩是「不许 panic」（WASM 里
        // panic = Worker 崩）⇒ ⇒ 拿不到读数就报 0 ⇒ ⇒ 「没有真值源就报 0」
        // 那条规则（裁决 H）在**争用**这一支上同样成立。
        match self.clint.try_borrow() {
            Ok(clint) => clint.mtime_cycles(),
            Err(_) => 0,
        }
    }
}

/// 一个「外设 → PLIC 源号」绑定（装载期从描述编译）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeripheralIrqKind {
    Uart,
    Gpio,
}

#[derive(Debug, Clone)]
pub struct PeripheralIrq {
    pub id: String,
    pub kind: PeripheralIrqKind,
    pub source: u16,
}

/// 从描述里收集**每个中断外设自己的 PLIC 源号**（UART / GPIO）。
pub fn compile_peripheral_irqs(description: &PlatformDescription) -> Vec<PeripheralIrq> {
    let mut out = Vec::new();
    for region in description.regions.iter() {
        let RegionSpec::Peripheral { device, .. } = region else {
            continue;
        };
        match device {
            DeviceSpec::Uart16550 { id, irq_line, .. } => out.push(PeripheralIrq {
                id: id.clone(),
                kind: PeripheralIrqKind::Uart,
                source: *irq_line,
            }),
            DeviceSpec::Gpio { id, irq_line } => out.push(PeripheralIrq {
                id: id.clone(),
                kind: PeripheralIrqKind::Gpio,
                source: *irq_line,
            }),
            _ => {}
        }
    }
    out
}

/// 把宿主的 CLINT 句柄**适配**成 `Rc<dyn TimeSource>`。
///
/// ⚠️ **幂等**：重复调用只是多造一个**指向同一个 CLINT** 的适配器 ⇒ ⇒
/// 时钟仍然只有**一份**。
pub fn clint_time_source(clint: &Rc<RefCell<Clint>>) -> Rc<dyn TimeSource> {
    Rc::new(ClintTimeSource {
        clint: Rc::clone(clint),
    })
}

// 「未知系统」门禁（v0.71 · STAGE-47 · 台账 D-175 候选 C）
// ---------------------------------------------------------------------------

/// 镜像要的地址 ⟷ **当前板**的关系（STAGE-47 · 纯函数 · **一处落点**）。
///
/// ⚠️ **三个字段**是**三件不同的事** ⇒ ⇒ **不许**用同一个布尔代替它们
/// ⇒ ⇒ 否则「部分落不在」会被误判成「完全不落不在」⇒ ⇒ ⇒ 那就把用户拍的
/// **中档**做成了**严档** ⇒ ⇒ ⇒ **用户拍的档就没了**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoardCompatibility {
    /// **每个**区间都**完全落在**某个分区里 ⇒ ⇒ 这块板**完全认得**它。
    pub fully_covered: bool,
    /// 至少有一个区间**完全落不在**任何分区里（**一个字节都不在**）
    /// ⇒ ⇒ ⇒ **「未知系统」** ⇒ ⇒ **拒**。
    pub missing: bool,
    /// 至少有一个区间**部分落在**（有交集但不完整）
    /// ⇒ ⇒ ⇒ **中档**：**不拒** ⇒ ⇒ 撞的那部分由**总线**自己去报。
    pub partial: bool,
}

/// 一块板**覆盖**了哪些地址区间（**只看 RAM / ROM**）。
///
/// ⚠️ **外设窗口不参与**（STAGE-47 裁决 E）⇒ ⇒ 窗口大小**不在描述里**
/// （`RegionSpec::Peripheral` **故意**没有那个字段）⇒ ⇒ 要算就得引
/// 「器件自报」那份 ⇒ ⇒ **本批不引** ⇒ ⇒ **已知边界**，如实登记。
///
/// ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段 ⇒ ⇒ **跳过**
/// （⇒ ⇒ 它还没有位置 ⇒ ⇒ 分配之后才有）⇒ ⇒ ⇒ **不**把它当成
/// 「占用 [0, size)」⇒ ⇒ ⇒ **不静默**编一个基址。
pub fn board_intervals(description: &PlatformDescription) -> Vec<(u32, u32)> {
    description
        .regions
        .iter()
        .filter_map(|region| match region {
            RegionSpec::Ram { base, size } | RegionSpec::Rom { base, size } => {
                base.map(|base| (base, base.saturating_add(*size)))
            }
            RegionSpec::Peripheral { .. } => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 装配期分配基址（v0.73 · STAGE-49 · B2）
// ---------------------------------------------------------------------------

/// **分配基址**：给**缺 `base`** 的段按**声明顺序**分配一个装得下的基址。
///
/// ⚠️ **这是 B2 的唯一落点**（一个约束一个落点）⇒ ⇒ 契约层**不**分配
/// （⇒ ⇒ 契约层看不到外设的窗口大小 —— 那是 **M-15 注册表**的事）。
///
/// ⚠️ **规则**（冻结规格 §2.3 · 裁决 H）：
/// - **顺序 = 描述里的声明顺序**（**不是** base 升序 ⇒ ⇒ 因为还没有 base）；
/// - 每段**从当前游标往上**找**第一个装得下的**空隙 ⇒ ⇒ **跳过**别的段
///   已经占住的区间；
/// - **结果确定**（同样的输入 ⇒ ⇒ 同样的输出）⇒ ⇒ ⇒ **可复算**。
///
/// ⚠️ **游标从 0 开始**：地址空间的**最低点** ⇒ ⇒ 第一段缺基址的段落在
/// **最低的可装下处** ⇒ ⇒ **确定**（不是「随机找一处」）。
///
/// ⚠️ **老描述逐字不变**（裁决 K）：**每一段都带 `base`** ⇒ ⇒ **一个字节
/// 都不改** ⇒ ⇒ ⇒ 返回的描述与输入**完全相同** ⇒ ⇒ 存量板档案**零风险**。
///
/// ⚠️ **零新场景码**（裁决 I）：
/// - **装不下**（地址空间放不下这一段）⇒ ⇒ **`E_PLT_07`**（超预算）
///   —— 那正是「这段声明的大小超过每区域预算」**已经**在契约层 V-7 判过
///   的那件事（⇒ ⇒ **复用既有码**，不造第三个）；
/// - **分配出来之后互相重叠** ⇒ ⇒ **`E_PLT_06`**（既有码）⇒ ⇒ ⇒ **零新码**。
///
/// @param description — 描述（**不被修改**）
/// @returns 一份**每段都有基址**的描述（老描述 ⇒ ⇒ 与输入逐字相同）
pub fn assign_bases(
    description: &PlatformDescription,
) -> Result<PlatformDescription, PlatformError> {
    let mut assigned = description.clone();
    // ⚠️ **已占用的区间**：先收下**显式带基址**的那些 ⇒ ⇒ 缺基址的段
    //   **跳过**它们（⇒ ⇒ 不会撞上用户明确写过的地址）。
    let mut occupied: Vec<(u64, u64)> = Vec::new();
    for region in &assigned.regions {
        let (Some(base), Some(size)) = (region.base(), region_footprint(region)) else {
            continue;
        };
        occupied.push((u64::from(base), u64::from(base) + u64::from(size)));
    }

    // ⚠️ **游标**：从 **0** 往上走（⇒ ⇒ 找**第一个**装得下的空隙）。
    let mut cursor: u64 = 0;
    for index in 0..assigned.regions.len() {
        // ⚠️ **已有基址 ⇒ 不动它**（⇒ ⇒ 老描述逐字不变 ⇒ ⇒ 用户明确
        //   写过的地址**一个都不改**）。
        let Some(size) = region_footprint(&assigned.regions[index]) else {
            continue;
        };
        if assigned.regions[index].base().is_some() {
            // ⚠️ **不推进游标**（v0.73 施工期订正）：规格 §2.3 写的是
            //   「从当前游标往上找**第一个装得下的**空隙 ⇒ ⇒ **跳过**被别的段
            //   占掉的区间」⇒ ⇒ 显式段**只是「占住区间」**（⇒ ⇒ 撞上就跳过）
            //   ⇒ ⇒ **不是**「屏障」（⇒ ⇒ 屏障的读法会让「高位显式段在前、
            //   缺基址段在后」的板把缺基址段推到**高位之后**⇒ ⇒ ⇒ 那与
            //   「第一个空隙」**不符**）⇒ ⇒ ⇒ 游标**只在真的分配时**前进。
            continue;
        }
        // ⚠️ **从游标往上**找**第一个**装得下的空隙（⇒ ⇒ 跳过已被占的）。
        let mut candidate = cursor;
        let mut placed: Option<u64> = None;
        while candidate + u64::from(size) <= u64::from(u32::MAX) + 1 {
            let end = candidate + u64::from(size);
            let clashes = occupied.iter().any(|&(lo, hi)| candidate < hi && lo < end);
            if !clashes {
                placed = Some(candidate);
                break;
            }
            // ⚠️ **跳过**撞上的那个区间 ⇒ ⇒ 往上找下一个。
            let hit_end = occupied
                .iter()
                .filter(|&&(lo, hi)| candidate < hi && lo < end)
                .map(|&(_, hi)| hi)
                .max()
                .unwrap_or(end);
            candidate = hit_end;
        }
        let Some(base) = placed else {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_07_OVER_BUDGET,
                format!("区域（缺基址）的大小 {:#x} 在地址空间里装不下", size),
            ));
        };
        assigned.regions[index].set_base(base as u32);
        occupied.push((base, base + u64::from(size)));
        cursor = base + u64::from(size);
    }
    Ok(assigned)
}

/// **一段占多大**（字节）—— 外设的**窗口大小**从 **M-15 注册表**取
/// （⇒ ⇒ 那是**唯一**那份真相 · 裁决 D）。
///
/// ⚠️ **契约层看不到**这个大小（描述里**故意**没有窗口大小那个字段）⇒ ⇒
/// ⇒ ⇒ **这**就是分配器**必须在内核侧**的理由。
fn region_footprint(region: &RegionSpec) -> Option<u32> {
    match region {
        RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => Some(*size),
        RegionSpec::Peripheral { device, .. } => {
            crate::m15_peripheral::types::window_bytes(device.kind())
        }
    }
}

/// 镜像要的地址区间 ⟷ 板的分区（STAGE-47 · **纯函数**）。
///
/// ⚠️ **半开区间**（`[lo, hi)`）⇒ ⇒ 与装载时算出来的 `ranges` **同一口径**。
///
/// ⚠️ **三个结果**（§2.2）：
/// * **完全落不在**（`missing`）⇒ ⇒ **「未知系统」**⇒ ⇒ **拒**；
/// * **部分落不在**（`partial`）⇒ ⇒ **中档**：**不拒** ⇒ ⇒ 撞了让总线去报；
/// * 全都完全落在 ⇒ ⇒ **认得**。
///
/// @param mirror_ranges — 镜像要落的区间（`[lo, hi)`）
/// @param board — 板覆盖的区间（`[lo, hi)`）
pub fn compatibility(mirror_ranges: &[(u32, u32)], board: &[(u32, u32)]) -> BoardCompatibility {
    let mut missing = false;
    let mut partial = false;
    for &(lo, hi) in mirror_ranges {
        // ⚠️ **空区间不算**（镜像没有字节）⇒ ⇒ 否则「零长度」会被判成
        //   「落不在」⇒ ⇒ ⇒ **空转的红**。
        if hi <= lo {
            continue;
        }
        let fully = board.iter().any(|&(b_lo, b_hi)| lo >= b_lo && hi <= b_hi);
        if fully {
            continue;
        }
        let overlaps = board.iter().any(|&(b_lo, b_hi)| lo < b_hi && b_lo < hi);
        if overlaps {
            partial = true;
        } else {
            missing = true;
        }
    }
    BoardCompatibility {
        fully_covered: !missing && !partial,
        missing,
        partial,
    }
}
