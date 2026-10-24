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

use riscv_sim_contract::error::{
    PlatformError, E_PLT_09_UNKNOWN_PLATFORM, E_PLT_10_PLATFORM_FILE_UNREADABLE,
    E_PLT_11_PLATFORM_FILE_MALFORMED, PLATFORM_FAMILY,
};
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
/// ⚠️ **顺序是「自动选档的同分兜底顺序」**：裁决 E 说「`priority` 小者
/// 优先」⇒ 同分时谁先谁后由这里决定 ⇒ 顺序**不是**语义（语义在
/// `priority` 里），但它必须**稳定**。
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

/// 全部**内置描述**（自动选档的**候选来源**）。
///
/// ⚠️ **这里返回的是「全部」**：哪一份**参与**自动选档，由描述自己的
/// `auto_selectable` 字段决定（裁决 E）⇒ 宿主**不按 id 硬编码**候选集
/// ⇒ 「演示平台只能被显式指定」这条规则写在**数据里**，不在代码里。
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
/// ## 判据（规格 §3 裁决 E 的**逐字**写法）
///
/// 1. **候选描述** = 全部 `auto_selectable == true` 的内置描述 ⇒ 演示
///    平台（`auto_selectable: false`）**只能被显式指定**；
/// 2. **区间在外层**逐个过：对每个区间 `(lo, hi)`，在候选里找**能完整
///    覆盖它**的那些（`lo >= base && hi <= base + size`，在它的**全部
///    RAM 区域**里找）中 `priority` **最小**的那个：
///    - **找不到** ⇒ 跳过这个区间（= 今天的 `continue`）；
///    - 找到的是 **`default`** ⇒ **继续下一个区间**（= 今天「被 Default
///      覆盖 ⇒ 不 return」）；
///    - 找到的是**别的** ⇒ **立即**选它（= 今天的 `return QemuVirt`）；
/// 3. **全部区间过完 ⇒ 回落 `default`**。
///
/// ⚠️ **「区间在外层」这条结构不能倒过来**（规格 §5.2 B-9 的补充）：
/// 「描述在外层、找第一个能装下**全部**区间的描述」在今天的两个真实
/// 描述上**结果相同**，但它**不是**同一条规则 ⇒ 用例用一个「区间落在
/// qemu-virt 高段 + 区间落在两档之外」的镜像把它**钉住**（区间在外层
/// ⇒ `qemu-virt`；描述在外层 ⇒ `default`）。
///
/// ⚠️ **为什么 `minimal` 不参与**（裁决 E 的**数据化**）：`minimal` 的
/// **低段**是 2 MiB（比 `default` 的 1 MiB 大）⇒ 若它参与选档，一段
/// 「比 `default` 低段大**一个字节**」的镜像会**改选 `minimal`** ⇒ 今天
/// 报 `E_BUS-01` 的那次装载会**悄悄成功** ⇒ 「今天只查高段」这个差异
/// 由 `auto_selectable: false` **显式承载在数据里** ⇒ 宿主**不按 id
/// 硬编码**候选集。
pub fn choose_description(ranges: &[(u32, u32)]) -> Result<PlatformDescription, PlatformError> {
    // ① 候选：全部 `auto_selectable == true` 的描述（裁决 E）。
    let candidates: Vec<PlatformDescription> = builtin_descriptions()?
        .into_iter()
        .filter(|description| description.auto_selectable)
        .collect();

    // ② **区间在外层**逐个过。
    for &(lo, hi) in ranges {
        // 覆盖这个区间的候选里，`priority` 最小的那个。
        let best = candidates
            .iter()
            .filter(|description| covered_by_ram(description, lo, hi))
            .min_by_key(|description| description.priority);

        match best {
            // 找不到 ⇒ 跳过这个区间（= 今天的 `continue`）。
            None => continue,
            // 找到的是 `default` ⇒ 继续下一个区间（= 今天「被 Default
            // 覆盖 ⇒ 不 return」）。
            Some(description) if description.id == PLATFORM_ID_DEFAULT => continue,
            // 找到的是别的 ⇒ **立即**选它（= 今天的 `return QemuVirt`）。
            Some(description) => return Ok(description.clone()),
        }
    }

    // ③ 全部区间过完 ⇒ 回落 `default`（**今天**也是这样：两档都装不下 ⇒
    // 返回 Default ⇒ 随后由 M-14 报「未映射访问」）。
    builtin_description(PLATFORM_ID_DEFAULT)
}
