//! 兼容档平台常量 —— GPIO / 默认内存图 / QEMU virt 地址、器件 id 与波特率。
//!
//! ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这些值的
//! **唯一落点** ⇒ 下面这些 `pub const` **不再**是宿主的取值来源 ⇒ 它们
//! **保留**只为**公开 API 兼容** ⇒ 别再拿它们当「真相」。

/// GPIO 外设的基址（PL061 惯例）。
///
/// 与默认内存图的两段 RAM（`0x0000_0000` / `0x8000_0000`）都不重叠。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_GPIO_BASE: u32 = 0x4000_0000;

/// GPIO 外设的实例 id（同时是 M-18 通道的 source）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const GPIO_PERIPH_ID: &str = "GPIO0";

/// GPIO 的信号名（M-15 `gpio::SIGNAL_OUT`）。
pub const GPIO_SIGNAL_NAME: &str = "OUT";

/// 波形通道的显示标签。
pub const GPIO_SIGNAL_LABEL: &str = "GPIO0.OUT";

/// GPIO `OUT` 信号的位宽（32 个引脚 → 32 位位图）。
///
/// 与 M-18 LLD 表 68 的「valueWidth = 按端口位宽」一致。
pub const GPIO_VALUE_WIDTH: u8 = 32;

/// 默认内存图 · 低段 RAM 的基址（复位向量）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_LOW_BASE: u32 = 0x0000_0000;

/// 默认内存图 · 低段 RAM 的大小（1 MiB）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_LOW_SIZE: u32 = 0x0010_0000;

/// 默认内存图 · 高段 RAM 的基址（常规 RISC-V 内存，与 golden 对拍侧一致）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_HIGH_BASE: u32 = 0x8000_0000;

/// 默认内存图 · 高段 RAM 的大小（4 MiB）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const DEFAULT_RAM_HIGH_SIZE: u32 = 0x0040_0000;

/// QEMU virt 的 UART0 基址。
///
/// ⚠️ 与 osz 的 `platform/qemu_virt/include/platform.h` 里的
/// `UART0_BASE_ADDR` **逐字一致**（0x1000_0000）⇒ 两边零改动。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_UART0_BASE: u32 = 0x1000_0000;

/// QEMU virt 的 CLINT 基址（osz 的 `CLINT_BASE_ADDR`）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_CLINT_BASE: u32 = 0x0200_0000;

/// QEMU virt 的 PLIC 基址（osz 的 `PLIC_BASE_ADDR`）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_PLIC_BASE: u32 = 0x0C00_0000;

/// QEMU virt 的 RAM 起点（ITCM）。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_RAM_BASE: u32 = 0x8000_0000;

/// QEMU virt 的 RAM 长度（**128 MiB** = ITCM 60M + INIT 4M + DTCM 64M）。
///
/// ⚠️ 这 128 MiB **不是**「随便给的大一点」——osz 的链接脚本把三个区
/// 排成 `ITCM(0x8000_0000,60M)` / `INIT(0x83C0_0000,4M)` /
/// `DTCM(0x8400_0000,64M)`，而它的 best-fit 分配器
/// （`kernel/base/mem/bestfit/src/bestfit.c`）**会在堆尾写一个节点**：
/// `end_node = (MEM_NODE_INFO *)((UINT8 *)DEFAULT_HEAP_END - MEM_NODE_INFO_SIZE)`
/// ⇒ 堆尾是 `0x8800_0000` 附近 ⇒ **必须**真的映射到那里。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_RAM_SIZE: u32 = 0x0800_0000;

/// QEMU virt 档的**每区域护栏预算**（v0.26 · STAGE-11 裁决 B / G）。
///
/// ⚠️ **为什么它必须存在**：M-14 的 `max_region_bytes`（D-10 的防 OOM
/// 护栏）**默认**是 16 MiB ⇒ 声明一段 128 MiB 的 RAM 会被**挡下**
/// （`E_BUS-09`）⇒ 宿主必须**按自己的内存图**把预算抬到它声明的大小。
///
/// ⚠️ **它不是「护栏没了」**：护栏**还在**、**没被删**，只是它的**数值**
/// 由宿主的内存图决定（`Bus::with_max_region_bytes` / `set_max_region_bytes`
/// 的文档一直写着「use this constructor only when the host has its own
/// budget policy」——这句话在 v0.26 之后**才真正成立**）。
///
/// ⚠️ **Default 档不抬**：它那两段 RAM 只有 1 MiB / 4 MiB ⇒ 保持默认的
/// 16 MiB ⇒ 护栏在默认档上**一字未变**。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_MAX_REGION_BYTES: u64 = QEMU_VIRT_RAM_SIZE as u64;

/// ⚠️ **已退役**（v0.26 · STAGE-11 裁决 G）：单个 RAM 区域的大小（16 MiB）。
///
/// ⚠️ **为什么退役**：它存在**只因为** M-14 的每区域上限是 16 MiB ⇒
/// 128 MiB **必须**拆成 8 个区域。v0.26 之后 QEMU virt 档是**一个**
/// 128 MiB 的区域 ⇒ 这个数**不再**是「一个区域的大小」⇒ 留着它就是
/// 留一个**陷阱**（谁再拿它去映射就又回到 8 段）。
///
/// ⚠️ **保留而不删**：它是 `pub` 常量（crate 的公开 API）⇒ 删掉是破坏性
/// 变更 ⇒ 这里只**标退役**并说明**为什么**。
pub const QEMU_VIRT_RAM_REGION: u32 = 0x0100_0000;

/// QEMU virt 的时钟频率（**10 MHz**）。
///
/// ⚠️ 与 osz 的 `SYSTEM_BASE_FREQ`（10 000 000）**逐字一致**；CLINT 的
/// `mtime` / `mtimecmp` 单位是**周期**，换算必须用这个数。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const QEMU_VIRT_FREQ_HZ: u64 = 10_000_000;

/// PLIC 的机器上下文号（osz 用 hart0 的 machine context 0）。
pub const PLIC_CONTEXT_MACHINE: u32 = 0;

/// UART0 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const UART0_PERIPH_ID: &str = "UART0";

/// CLINT 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const CLINT_PERIPH_ID: &str = "CLINT";

/// PLIC 的外设 id。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const PLIC_PERIPH_ID: &str = "PLIC";

/// UART0 的波特率。
///
/// ⚠️ 平台的 UART 是**捕获环**（写进环就是「已发出」），波特率**不参与**
/// 时序计算；这个值只是让 `ps_per_bit` 有个确定的分母。
///
/// ⚠️ **v0.27 之后**：平台描述（`platforms/*.json`）才是这个值的
/// **唯一落点** ⇒ 这个常量**不再**是宿主的取值来源 ⇒ 保留只为
/// **公开 API 兼容**。
pub const UART0_BAUD_RATE: u32 = 115_200;
