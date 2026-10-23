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
//! ## 连线**不**进描述（裁决 G）
//!
//! 「UART 的 RX 就绪 → PLIC 的源 10」是**器件之间的信号连线**，不是
//! 地址空间 ⇒ 描述覆盖「有什么、在哪、参数是多少」，**不覆盖「谁连谁」**
//! ⇒ 换平台若要改连线，**仍要改宿主代码**（台账 D-125）。

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use riscv_sim_contract::error::{PlatformError, PLATFORM_FAMILY};
use riscv_sim_contract::platform::{DeviceSpec, PlatformDescription, RegionSpec};
use riscv_sim_contract::port::Peripheral;

use crate::m14_bus::Bus;
use crate::m15_peripheral::clint::Clint;
use crate::m15_peripheral::gpio::GpioBank;
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
// 宿主侧的加载错误码（E_PLT-09 .. E_PLT-11）
// ---------------------------------------------------------------------------

/// 请求的**内置平台 id** 不存在。
///
/// ⚠️ **为什么宿主自己带码**：契约层拥有的是**描述的声明规则**（V-1 ~ V-9
/// ⇒ `E_PLT-01` .. `E_PLT-08`）⇒ 「**按 id 取不到内置描述**」这条规则
/// 属于**宿主**（谁负责按 id 找描述）⇒ 按契约层 `error.rs` 的规矩
/// （「谁拥有这个场景，谁拥有这个码」），码住在**拥有它的模块**里。
pub const E_PLT_09_UNKNOWN_PLATFORM: u16 = 9;

/// 描述文件**读不到**（路径不存在 / 权限不足 / 读失败）。
pub const E_PLT_10_DESCRIPTION_UNREADABLE: u16 = 10;

/// 描述文件**解析不了**（不是合法 JSON / 形状不对 ⇒ 校验规则 **V-8**
/// 的 serde 错误）。
pub const E_PLT_11_DESCRIPTION_MALFORMED: u16 = 11;

// ---------------------------------------------------------------------------
// 加载
// ---------------------------------------------------------------------------

/// 全部内置描述 —— **(id, JSON 原文)**。
///
/// ⚠️ **顺序是「自动选档的同分兜底顺序」**：裁决 E 说「`priority` 小者
/// 优先」⇒ 同分时谁先谁后由这里决定 ⇒ 顺序**不是**语义（语义在
/// `priority` 里），但它必须**稳定**。
const BUILTIN_DESCRIPTIONS: &[(&str, &str)] = &[
    (PLATFORM_ID_DEFAULT, BUILTIN_JSON_DEFAULT),
    (PLATFORM_ID_QEMU_VIRT, BUILTIN_JSON_QEMU_VIRT),
    (PLATFORM_ID_MINIMAL, BUILTIN_JSON_MINIMAL),
];

/// 参与**自动选档**的内置描述。
///
/// ⚠️ **为什么 `minimal` 不在这里**（规格 §3 裁决 E 的一处**空白**）：
/// `minimal` 是**演示**平台（规格 §0.1 第 6 条「第三份**演示**描述」）
/// ⇒ 它的存在是为了证明「**换描述、宿主零改动**」⇒ 它**只能被显式
/// 指定**。
///
/// ⚠️ **为什么这不是「按 id 硬编码内存图」**：这里列的是「**哪些平台
/// 参与自动挑档**」，**不是**地址、**不是**器件、**不是**预算 ⇒ 换一份
/// 描述的**内容**不需要改这里（§2.5 对照表的最后一行禁的是「按 id 硬
/// 编码**预算**」）。
///
/// ⚠️ **若把 `minimal` 放进候选集，既有行为会变**：一段「比 `default`
/// 的低段 RAM 大**一个字节**」的镜像（`0x0..0x10_0001`）装不进
/// `default` / `qemu-virt`，却装得进 `minimal` 的 2 MiB 低段 ⇒ 它会
/// **成为候选** ⇒ 宿主改用 `minimal` ⇒ 今天报 `E_BUS-01` 的那次装载
/// **悄悄成功**了 ⇒ 规格 §3 裁决 E 的等价性论证只覆盖了「镜像**装得进**
/// 低段」的情形，**没覆盖**这一种 ⇒ 这是规格的一处**空白**，由本模块
/// 按「演示平台只显式指定」的口径收拢。
const AUTO_SELECTABLE_IDS: &[&str] = &[PLATFORM_ID_DEFAULT, PLATFORM_ID_QEMU_VIRT];

/// 解析一段描述文本 —— **唯一的解析器**。
///
/// ⚠️ 内置路径与文件路径**共用这一个解析器 + 契约层那一个校验器**
/// ⇒ 没有两份语义（§2.5 对照表第一行）。
///
/// ⚠️ **serde 的错误**（V-8：未知器件种类 / 缺参数 / 参数类型错）在**解析
/// 阶段**就失败 ⇒ 报 [`E_PLT_11_DESCRIPTION_MALFORMED`]；**声明规则**的
/// 错误（V-1 ~ V-7 / V-9）由契约层的 `validate` 报 ⇒ `E_PLT-01` ..
/// `E_PLT-08`。
pub fn parse_description(json: &str) -> Result<PlatformDescription, PlatformError> {
    let description: PlatformDescription = serde_json::from_str(json).map_err(|err| {
        PlatformError::new(
            PLATFORM_FAMILY,
            E_PLT_11_DESCRIPTION_MALFORMED,
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

/// 全部**内置描述**（自动选档的候选集）。
///
/// ⚠️ 候选集**包含 `minimal`** —— 裁决 E 说候选按 `priority` 小者优先，
/// `minimal` 的 `priority` 是 30（最大）⇒ 它**排在最后**⇒ 绝大多数情况
/// 下选不到它。
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
            E_PLT_10_DESCRIPTION_UNREADABLE,
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
        .map(|region| region.base())
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
}

impl BuiltDevice {
    /// 一个**宿主不直接碰**的器件（今天只有 GPIO 是这样）。
    fn opaque<P>(device: P) -> Self
    where
        P: Peripheral + 'static,
    {
        BuiltDevice {
            peripheral: Rc::new(RefCell::new(device)),
            uart: None,
            clint: None,
            plic: None,
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
        }
    }
}

/// 描述 → 器件实例。**一个 match、一个落点**（§2.4）。
///
/// ⚠️ **新增一种器件种类 ⇒ 必须改这一个 match** ⇒ 这是**不可消除**的
/// 边界 ⇒ 规格明说：**平台（内存图）零代码，器件种类要代码**。
fn build_device(spec: &DeviceSpec, base: u32) -> Result<BuiltDevice, PlatformError> {
    Ok(match spec {
        DeviceSpec::Gpio { id } => BuiltDevice::opaque(GpioBank::new(id.clone(), base)),
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
    })
}

// ---------------------------------------------------------------------------
// 建图
// ---------------------------------------------------------------------------

/// 建完图之后，宿主**要留着的**器件句柄。
#[derive(Debug, Default)]
pub struct MappedDevices {
    /// 第一个 UART16550（`None` = 这份描述里没有 UART）。
    pub uart0: Option<Rc<RefCell<Uart16550>>>,
    /// 第一个 CLINT。
    pub clint: Option<Rc<RefCell<Clint>>>,
    /// 第一个 PLIC。
    pub plic: Option<Rc<RefCell<Plic>>>,
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
/// 护栏预算 → bus.set_max_region_bytes(描述.region_budget_bytes)
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
    // 校验**先**于建图 ⇒ 不合规的描述不留任何区域。
    description.validate()?;

    // 护栏预算：描述说了算（§2.5 对照表最后一行）。
    bus.set_max_region_bytes(u64::from(description.region_budget_bytes));

    let mut devices = MappedDevices::default();
    // ⚠️ 按 **base 升序**建图（与总线的 `BTreeMap` 顺序一致）⇒ 建图
    // 顺序是**确定的**，不依赖描述里的书写顺序。
    for region in regions_by_base(description) {
        match region {
            RegionSpec::Ram { base, size } => {
                bus.map_ram(*base, *size, &[])?;
            }
            RegionSpec::Rom { base, size } => {
                bus.map_rom(*base, *size, &[])?;
            }
            RegionSpec::Peripheral { base, device } => {
                let BuiltDevice {
                    peripheral,
                    uart,
                    clint,
                    plic,
                } = build_device(device, *base)?;
                // ⚠️ 窗口大小**由器件自己报**（裁决 D）。
                let window = peripheral.borrow().size();
                bus.map_peripheral_instance(*base, window, peripheral)?;
                // ⚠️ **第一个**同种器件被宿主认领（描述里没有第二个
                // UART 的语义；今天的宿主也只有一路 console）。
                if devices.uart0.is_none() {
                    devices.uart0 = uart;
                }
                if devices.clint.is_none() {
                    devices.clint = clint;
                }
                if devices.plic.is_none() {
                    devices.plic = plic;
                }
            }
        }
    }
    Ok(devices)
}

// ---------------------------------------------------------------------------
// 自动选档（裁决 E）
// ---------------------------------------------------------------------------

/// 一个地址区间是否被描述的**某个 RAM 区域**完整覆盖。
///
/// ⚠️ **只看 RAM**：ROM 不是 RAM（裁决 E 说「取它的**全部 RAM 区域**」）。
fn covered_by_ram(description: &PlatformDescription, lo: u32, hi: u32) -> bool {
    description.regions.iter().any(|region| match region {
        RegionSpec::Ram { base, size } => lo >= *base && hi <= base.saturating_add(*size),
        RegionSpec::Rom { .. } | RegionSpec::Peripheral { .. } => false,
    })
}

/// 按镜像**实际用到的地址**挑平台描述（裁决 E）。
///
/// 判据：
///
/// 1. 对每个描述，取它的**全部 RAM 区域**；
/// 2. 镜像的**每个**地址区间 `(lo, hi)` 必须被**某一个 RAM 区域**完整
///    覆盖（`lo >= base && hi <= base + size`）；
/// 3. 候选描述按 `priority` **小者优先**（内置：`default` = 10 ·
///    `qemu-virt` = 20）；
/// 4. **无候选 ⇒ 回落 `default`**（与今天「`continue` ⇒ 最终返回
///    Default」等价）。
///
/// ⚠️ **与今天的等价性**（B-9 的用例钉住）：今天只查**高段**窗口，
/// 描述化查**全部 RAM 区域**⇒ `default` 的 `priority` 最小 ⇒ 低段镜像
/// 仍然选 `default` ✓。
///
/// ⚠️ **为什么候选集里没有 `minimal`**（规格 §3 裁决 E 的一处**空白**，
/// 详见 [`AUTO_SELECTABLE_IDS`]）：`minimal` 是**演示**平台 ⇒ 它**只能
/// 被显式指定** ⇒ 若它参与自动选档，一段「比 `default` 的低段 RAM 大
/// 一个字节」的镜像会**改选 `minimal`** ⇒ 今天报 `E_BUS-01` 的那次装载
/// 会**悄悄成功** ⇒ **既有行为变了**。
pub fn choose_description(ranges: &[(u32, u32)]) -> Result<PlatformDescription, PlatformError> {
    let mut candidates: Vec<PlatformDescription> = builtin_descriptions()?
        .into_iter()
        .filter(|description| AUTO_SELECTABLE_IDS.contains(&description.id.as_str()))
        .filter(|description| {
            ranges
                .iter()
                .all(|(lo, hi)| covered_by_ram(description, *lo, *hi))
        })
        .collect();
    candidates.sort_by_key(|description| description.priority);
    match candidates.into_iter().next() {
        Some(description) => Ok(description),
        // ⚠️ 无候选 ⇒ 回落 `default`（**今天**也是这样：两档都装不下 ⇒
        // 返回 Default ⇒ 随后由 M-14 报「未映射访问」）。
        None => builtin_description(PLATFORM_ID_DEFAULT),
    }
}
