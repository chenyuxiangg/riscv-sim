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
    PlatformError, E_PLT_09_UNKNOWN_PLATFORM, E_PLT_10_PLATFORM_FILE_UNREADABLE,
    E_PLT_11_PLATFORM_FILE_MALFORMED, PLATFORM_FAMILY,
};
use riscv_sim_contract::platform::{
    DeviceSpec, PlatformDescription, RegionSpec, WIRING_EVENT_RX_READY,
};
use riscv_sim_contract::port::{Peripheral, TimeSource};

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
    /// 宿主**不**持有专用句柄的器件（今天的 GPIO）⇒ ⇒ 只登记「**它在**」
    /// ⇒ ⇒ 「GPIO 能不能当线的两端」**判得出来**（⇒ ⇒ `source-type` /
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
                // ⚠️ **逐实例**清单（STAGE-43 · 裁决 K / L）—— 与建图**同源**
                // ⇒ ⇒ 「谁绑在哪儿」**不需要**另算一遍。
                devices
                    .instances
                    .push(riscv_sim_contract::model::DeviceInstanceInfo {
                        id: device.id().to_string(),
                        kind: device.kind().to_string(),
                        base: Some(*base),
                        window_bytes: window,
                    });
                // ⚠️ **实例表**（STAGE-48 · D-177 B）：**每一个**器件实例都
                // 进表 ⇒ ⇒ **第二个 UART 也有句柄** ⇒ ⇒ 它的线**真的**能
                // 通电（裁决 E）。
                //
                // ⚠️ **GPIO 没有专用句柄** ⇒ ⇒ 记成 `Opaque` ⇒ ⇒ 「它在」
                // 这件事**判得出来**（⇒ ⇒ 线的两端是 GPIO 时报
                // `source-type` / `target-type`，**不是** `source-unknown`）。
                let handle = match (uart, clint, plic) {
                    (Some(u), None, None) => DeviceHandle::Uart(u),
                    (None, Some(c), None) => DeviceHandle::Clint(c),
                    (None, None, Some(p)) => DeviceHandle::Plic(p),
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

/// 把宿主的 CLINT 句柄**适配**成 `Rc<dyn TimeSource>`。
///
/// ⚠️ **幂等**：重复调用只是多造一个**指向同一个 CLINT** 的适配器 ⇒ ⇒
/// 时钟仍然只有**一份**。
pub fn clint_time_source(clint: &Rc<RefCell<Clint>>) -> Rc<dyn TimeSource> {
    Rc::new(ClintTimeSource {
        clint: Rc::clone(clint),
    })
}

// ---------------------------------------------------------------------------
// 连线表（v0.34 · D-125 · 冻结规格 STAGE-19 §3.1）
// ---------------------------------------------------------------------------

/// 宿主侧**编译出来的一条连线**。
///
/// ⚠️ **它不是契约层的 `WiringSpec`**：契约层那份是**描述里的形状**（字段
/// 名就是线上的名字）；宿主这份是**编译后的形状**——「谁的事件 ⇒ 抬哪个
/// 源」已经判完了 ⇒ 运行时只需要这两个数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wiring {
    /// 源器件的 `id`（描述里的器件 id ⇒ 覆盖键的第一半）。
    pub from_device: String,
    /// 事件名（**只有** `rx_ready` ⇒ 契约层那个常量）。
    pub event: String,
    /// 目标器件的 `id`（PLIC）。
    pub to_device: String,
    /// 目标 PLIC 的**源号**。
    pub source: u32,
}

/// 描述里**第一个** UART 的器件 id。
///
/// ⚠️ **口径 = 建图时「第一个同种器件被宿主认领」那一条**（按 base 升序）
/// ⇒ ⇒ 「宿主手里那路 UART」与「这里的第一个 UART」**永远是同一个**。
fn first_uart_id(description: &PlatformDescription) -> Option<String> {
    regions_by_base(description)
        .into_iter()
        .find_map(|region| match region {
            RegionSpec::Peripheral {
                device: DeviceSpec::Uart16550 { id, .. },
                ..
            } => Some(id.clone()),
            _ => None,
        })
}

/// 描述里**第一个** PLIC 的 `(器件 id, 源数)`。
///
/// ⚠️ **口径同 [`first_uart_id`]**（按 base 升序的第一个 PLIC）⇒ ⇒ 与宿主
/// 手里那个 PLIC 句柄**是同一个**。
fn first_plic(description: &PlatformDescription) -> Option<(String, u32)> {
    regions_by_base(description)
        .into_iter()
        .find_map(|region| match region {
            RegionSpec::Peripheral {
                device: DeviceSpec::Plic { id, sources, .. },
                ..
            } => Some((id.clone(), *sources)),
            _ => None,
        })
}

/// **默认连线**（宿主里**唯一**那处）—— 冻结规格 STAGE-19 §3.1 · 裁决 E。
///
/// ⚠️ **它说的是「这块板上的第一个 UART / 第一个 PLIC」**：宿主今天写死的
/// 行为就是「**第一个** UART 的 RX 就绪 ⇒ 抬 PLIC 的**源 10**」⇒ ⇒ 用
/// **角色**（而不是写死的器件 id）表述 ⇒ ⇒ 存量行为**一字未变**（B-2）。
///
/// ⚠️ **源号 10 在整个宿主里只写在这里**：那个写死的 PLIC source 常量
/// **已删**（保留 = 又一个真相源；名字见冻结规格 STAGE-19 §3.1 · 裁决 E）
/// ⇒ ⇒ 「换一块板改连线」不用改宿主（D-125）。
///
/// ⚠️ **两端器件缺一 ⇒ 没有默认连线**（不是空指针）：描述里没有 UART 或
/// 没有 PLIC ⇒ 这根线**不生效**（B-3）。
pub fn default_wiring(description: &PlatformDescription) -> Vec<Wiring> {
    let (uart_id, (plic_id, plic_sources)) =
        match (first_uart_id(description), first_plic(description)) {
            (Some(uart_id), Some(plic)) => (uart_id, plic),
            _ => return Vec::new(),
        };
    let wire = Wiring {
        from_device: uart_id,
        event: WIRING_EVENT_RX_READY.to_string(),
        to_device: plic_id,
        source: 10,
    };
    // ⚠️ **源号越界 ⇒ 不接这根线**：PLIC 的源数不够 ⇒ 抬不起来 ⇒ ⇒
    // （今天的行为也一样：PLIC 的 `request` 忽略越界的源）⇒ ⇒ 描述里的
    // `wiring` 仍可以给它一个合法的源号。
    if wire.source >= plic_sources {
        return Vec::new();
    }
    vec![wire]
}

/// **连线编译的结果** —— 表 + **丢线报告**（STAGE-48 · v0.72 · D-177 A）。
#[derive(Debug, Clone, Default)]
pub struct WiringCompile {
    /// **活着的**线（宿主真的抬得了的那些）。
    pub wires: Vec<Wiring>,
    /// **丢线报告** —— 「你写的 N 根线里有 M 根当前抬不了」。
    pub report: riscv_sim_contract::model::WiringReportDto,
}

/// 描述里某个 PLIC 的**源数**（`None` ⇒ 描述里没这个 PLIC）。
///
/// ⚠️ **用描述里的那个数**（不是从 PLIC 实例里读）⇒ ⇒ 借用的争用**不
/// 影响**判定 ⇒ ⇒ 「拿不到就不判越界」那种 fail-open **不需要**。
fn plic_sources_of(description: &PlatformDescription, id: &str) -> Option<u32> {
    regions_by_base(description)
        .into_iter()
        .find_map(|region| match region {
            RegionSpec::Peripheral {
                device:
                    DeviceSpec::Plic {
                        id: plic_id,
                        sources,
                        ..
                    },
                ..
            } if plic_id == id => Some(*sources),
            _ => None,
        })
}

/// 一根线**抬不了**的**第一条**理由（`Ok` ⇒ 抬得了）。
///
/// ⚠️ **五条判据分开算**（STAGE-48 §2.1）⇒ ⇒ 界面能说清「**为什么**」
/// ⇒ ⇒ 「不知道抬不了」与「抬不了」是**两件事**。
fn check_wire(
    description: &PlatformDescription,
    devices: &DeviceInstanceTable,
    wire: &Wiring,
) -> Result<(), riscv_sim_contract::model::WireDropReason> {
    use riscv_sim_contract::model::WireDropReason;

    // ---- 源器件 ----
    let Some(source) = devices.get(&wire.from_device) else {
        return Err(WireDropReason::SourceUnknown);
    };
    if source.kind != "uart16550" {
        return Err(WireDropReason::SourceType);
    }

    // ---- 目标器件 ----
    let Some(target) = devices.get(&wire.to_device) else {
        return Err(WireDropReason::TargetUnknown);
    };
    if target.kind != "plic" {
        return Err(WireDropReason::TargetType);
    }

    // ---- 源号范围 ----
    let Some(sources) = plic_sources_of(description, &wire.to_device) else {
        return Err(WireDropReason::TargetUnknown);
    };
    if wire.source >= sources {
        return Err(WireDropReason::SourceOutOfRange);
    }

    Ok(())
}

/// 从**生效的描述**编译出**连线表** + **丢线报告**（冻结规格 STAGE-19 §3.1
/// · **STAGE-48 §3 裁决 C**）。
///
/// ```text
/// 生效描述 ──► 编译连线表 ──► sync_interrupts 按表抬线
///   ├─ 默认连线：第一个 UART.rx_ready ──► 第一个 PLIC.source 10
///   └─ 描述里的 wiring：按 (from.device, from.event) **覆盖**同键的默认项
/// ```
///
/// ⚠️ **同一时刻一个键只有一根线**（覆盖 ⇒ **不**追加）⇒ ⇒ 「同一时刻只有
/// 一个 source」是**构造保证**的，不是运行时判出来的。
///
/// ⚠️ **v0.72（STAGE-48 · D-177 B）**：末尾那个 `retain`（只留「第一个 UART
/// → 第一个 PLIC」）**已删** ⇒ ⇒ 代之以「**查得到就留、查不到就记一笔**」
/// ⇒ ⇒ **丢线不再静默**（裁决 C）⇒ ⇒ **第二个 UART 的线也留在表里**
/// ⇒ ⇒ 配合 [`crate::kernel_host::KernelHost`] 侧「逐线查实例」
/// （裁决 E）⇒ ⇒ **第二个 UART 的连线也真的通电**。
///
/// ⚠️ **默认连线**（[`default_wiring`]）**逐字不变** ⇒ ⇒ 存量行为一字不改
/// （裁决 D）⇒ ⇒ 「第一个 UART / 第一个 PLIC」这个口径**仍是描述里的
/// 那一个**。
pub fn compile_wiring(
    description: &PlatformDescription,
    devices: &DeviceInstanceTable,
) -> WiringCompile {
    use riscv_sim_contract::model::{DroppedWireDto, WireDropReason};

    // ---- 1) 默认连线（**逐字不变**；`false` = 不是用户写的）----
    let mut entries: Vec<(Wiring, bool)> = default_wiring(description)
        .into_iter()
        .map(|w| (w, false))
        .collect();

    // ---- 2) 描述里的连线：按 (from.device, from.event) 覆盖同键 ----
    //
    // ⚠️ **被顶掉的那些**（同键的前一条）**要记一笔**（裁决 K）⇒ ⇒ 否则
    // 「你写了 2 根、只有 1 根生效」这件事**仍然静默**。
    let mut shadowed: Vec<Wiring> = Vec::new();
    for spec in description.wiring.iter().flatten() {
        let wire = Wiring {
            from_device: spec.from.device.clone(),
            event: spec.from.event.clone(),
            to_device: spec.to.device.clone(),
            source: spec.to.source,
        };
        match entries
            .iter()
            .position(|(w, _)| w.from_device == wire.from_device && w.event == wire.event)
        {
            Some(idx) => {
                let (old, was_written_by_user) = entries[idx].clone();
                // ⚠️ 被顶掉的若是**默认连线** ⇒ ⇒ **不记**（它不是用户写的
                // ⇒ ⇒ 裁决 J：`declared` 只数用户写的线）。
                if was_written_by_user {
                    shadowed.push(old);
                }
                entries[idx] = (wire, true);
            }
            None => entries.push((wire, true)),
        }
    }

    // ---- 3) 端点检查：**查得到就留、查不到就记一笔**（裁决 C）----
    let mut wires: Vec<Wiring> = Vec::new();
    let mut dropped: Vec<DroppedWireDto> = Vec::new();
    let mut live = 0u32;
    for (wire, written_by_user) in entries.iter() {
        match check_wire(description, devices, wire) {
            Ok(()) => {
                wires.push(wire.clone());
                if *written_by_user {
                    live += 1;
                }
            }
            Err(reason) => dropped.push(DroppedWireDto {
                from_device: wire.from_device.clone(),
                event: wire.event.clone(),
                to_device: wire.to_device.clone(),
                source: wire.source,
                reason,
            }),
        }
    }

    // ---- 4) 被顶掉的那几根（**用户写的**才算数）----
    for wire in shadowed {
        dropped.push(DroppedWireDto {
            from_device: wire.from_device.clone(),
            event: wire.event.clone(),
            to_device: wire.to_device.clone(),
            source: wire.source,
            reason: WireDropReason::Overridden,
        });
    }

    // ⚠️ **恒等式**：`declared == live + dropped.len()`（裁决 I）⇒ ⇒ 一个判据
    // 管三件事。
    //
    // ⚠️ ⚠️ **这里**刻意**不写** `debug_assert_eq!` —— **恒等式的落点只有
    // 一处**（`tests_v072_wiring.rs` 的 **K-12**）⇒ ⇒ 两处都写 = 一个约束
    // 两个落点 ⇒ ⇒ 而且那个 `debug_assert`（**release 里被编掉**⇒ ⇒ 生产
    // 上**根本不守**）⇒ ⇒ 更糟的是它会**先炸** ⇒ ⇒ 变异自检的红集**全被它
    // 掩盖** ⇒ ⇒ 归因失真（本批**实测踩到**：T-4 的红集里 10 条全是它）。
    let declared = description.wiring.iter().flatten().count() as u32;

    WiringCompile {
        wires,
        report: riscv_sim_contract::model::WiringReportDto {
            declared,
            live,
            dropped,
        },
    }
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

// ---------------------------------------------------------------------------
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
pub fn board_intervals(description: &PlatformDescription) -> Vec<(u32, u32)> {
    description
        .regions
        .iter()
        .filter_map(|region| match region {
            RegionSpec::Ram { base, size } | RegionSpec::Rom { base, size } => {
                Some((*base, base.saturating_add(*size)))
            }
            RegionSpec::Peripheral { .. } => None,
        })
        .collect()
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
