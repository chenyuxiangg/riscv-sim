//! Platform description — the **declared** memory map (v0.27 · D-123-B).
//!
//! # What this is
//!
//! A **platform description** is the data form of "what memory map does
//! this board have": which regions exist, where they start, how big they
//! are, and which peripheral device sits at which base address. It is
//! the input that lets the host build a memory map **without hard-coding
//! one** — before this module, the host had two profiles (`Default` /
//! `QemuVirt`) whose addresses and device constructions were literals in
//! the host's source code, so "use a different board" meant "edit the
//! host".
//!
//! The frozen spec is
//! `docs/detailed-design/STAGE-12_V027_Platform_Description.md` §1 (the
//! format) and §2.1 (this file's landing spot).
//!
//! # Where the rules live
//!
//! **All** of the description's validation rules live in
//! [`PlatformDescription::validate`] — one constraint, one landing spot.
//! The host must not write a second validator, and neither must the
//! front end when it starts mirroring this shape.
//!
//! # Three layers, three sets of codes
//!
//! | layer | what it judges | codes |
//! | --- | --- | --- |
//! | **serde** | the *shape* — unknown device kind, missing parameter, wrong parameter type | serde's own error (V-8) |
//! | **this module** | the *declaration* — version, id, regions, sizes, overlaps, budget, duplicate device id, empty device id, device parameters' legal value domain, and each peripheral's PLIC source id | `E_PLT-01`..`E_PLT-08` · `E_PLT-12` · `E_PLT-15` · `E_PLT-16` |
//! | **the host** | the *lookup* — unknown platform id, unreadable file, malformed file, a stale revision, a delta that targets a missing region | `E_PLT-09`..`E_PLT-11` · `E_PLT-13` · `E_PLT-14` (owned **here**, judged by the host) |
//! | **M-14** | the *live bus* — unmapped access, region over the runtime budget | `E_BUS-*` |
//!
//! ⚠️ The two budget rules are **not** the same constraint and must not
//! be merged: [`E_PLT_07_OVER_BUDGET`] judges the **declared** size
//! (before anything is mapped), M-14's `E_BUS-09` judges the **physical
//! occupancy** (after mapping — RAM is lazily allocated since v0.26, so
//! a region may declare a large size and cost nothing).
//!
//! ⚠️ **v0.31 (D-138)**: the **numbers** behind those two rules used to
//! be the **same field** (`region_budget_bytes` fed both) ⇒ they are
//! split into two: `region_budget_bytes` stays the **declaration-time**
//! ceiling (judged here by V-7) and
//! [`PlatformDescription::max_region_bytes`] is the **runtime** ceiling
//! (judged by M-14's `E_BUS-09`). Absent ⇒ the runtime one **equals**
//! the declaration-time one ⇒ an old description behaves **byte for
//! byte** as before.
//!
//! ⚠️ **The host-side codes live here too** (V-10 .. V-12): the *rules*
//! are the host's, but a code with **two** landing spots is a code with
//! two meanings ⇒ the contract layer owns every `E_PLT-*` number.
use crate::error::{
    PlatformError, E_PLT_01_SCHEMA_VERSION, E_PLT_02_EMPTY_ID, E_PLT_03_NO_REGIONS,
    E_PLT_04_ZERO_SIZE, E_PLT_05_OVERFLOW, E_PLT_06_OVERLAP, E_PLT_07_OVER_BUDGET,
    E_PLT_08_DUPLICATE_DEVICE_ID, E_PLT_12_PARAMETER_OUT_OF_RANGE, E_PLT_15_DEVICE_EMPTY_ID,
    E_PLT_16_IRQ_SOURCE_INVALID, E_PLT_18_ROLES_INVALID, E_PLT_21_MCU_INVALID,
    E_PLT_24_MULTIPLE_PLIC, PLATFORM_FAMILY,
};
use crate::session::{REGION_KIND_PERIPHERAL, REGION_KIND_RAM, REGION_KIND_ROM};
use serde::{Deserialize, Serialize};
/// Format version of the **platform description**.
///
/// ⚠️ **This is not the session payload's version** and it is not the
/// contract's [`crate::SCHEMA_VERSION`] either: a platform description
/// is **not** a session payload — it never travels inside
/// [`crate::session::SessionSnapshotDto`] — so it carries its **own**
/// version number and the two never move together.
///
/// ⚠️ The description's format version is also **not** the contract
/// schema version: the contract version governs the **message shape**
/// (what the front end mirrors), the description version governs the
/// **memory-map description shape** (what the host reads). Adding this
/// module therefore requires **no** front-end change at all.
/// ⚠️ **v0.73（STAGE-49 · B2）**：**描述格式升版** **1 → 2** ⇒ ⇒
/// **版本 2** = 区域（`RegionSpec`）的 `base` **可缺省**（⇒ ⇒ 缺省 =
/// 由内核在**装配期**分配）⇒ ⇒ ⇒ **存量板档案要迁**（⇒ ⇒ 但**缺省
/// 优先**：老描述**带** `base` ⇒ ⇒ **仍能装** ⇒ ⇒ **一个字节都不变**）。
///
/// ⚠️ **版本 1** = 区域**必须**带 `base`（⇒ ⇒ 那时还没有「由内核分配」
/// 这件事）⇒ ⇒ ⇒ 老描述**逐字**按老办法装（⇒ ⇒ **零风险**）。
/// ⚠️ **v0.78**：描述格式已收敛为外设中断号映射；本版本号只记录格式演进。
/// **可选**的 `pin`（引脚级寻址）⇒ ⇒ 老描述**逐字不变**（⇒ ⇒ 存量板档案
/// **零风险**）。
/// ⚠️ **v0.75（STAGE-51 · D-175 的 F）**：**3 → 4** ⇒ ⇒ 顶层加了**可选**
/// `mcu` 段（MCU 概念：ISA / 厂商 / 型号 / 向量表）⇒ ⇒ 老描述**缺省不带键**
/// ⇒ ⇒ **逐字不变**（⇒ ⇒ 存量板档案**零风险** ⇒ ⇒ **缺省优先迁移**）。
/// ⚠️ **v0.76（STAGE-52）**：**4 → 5** ⇒ ⇒ **删 `auto_selectable`**（模板
///   导入 · 不兜底生效板 · B3 决议）＋ **`pins` 段升级为引脚规格**
///   （`PinSpec` 功能列表 · 引脚的复用功能 / 方向 + 固件动态切换）⇒ ⇒
///   老描述**带 `auto_selectable` 键仍能解析**（serde 忽略未知字段 ⇒ ⇒
///   **缺省优先迁移** ⇒ ⇒ 存量板档案**零风险**）。
pub const PLATFORM_SCHEMA_VERSION: u32 = 5;
/// **本构建能读**的平台描述格式版本（V-1 判的就是它）。
///
/// ⚠️ **v0.73（STAGE-49 · B2）**：**`[1, 2]`** ⇒ ⇒ ⇒ **缺省优先**：老描述
/// （`schema_version = 1`）⇒ ⇒ **仍能装**（⇒ ⇒ 它的区域**都带** `base`
/// ⇒ ⇒ 装配结果**逐字不变**）⇒ ⇒ ⇒ **存量板档案零风险**。
///
/// ⚠️ **为什么不是一个数**：`PLATFORM_SCHEMA_VERSION` 是「**本构建写**的
/// 那个版本」（⇒ ⇒ 前端**写**它）⇒ ⇒ 而「**本构建能读**的那些版本」是
/// **另一件事**（⇒ ⇒ 读比写**宽** ⇒ ⇒ 这是**格式兼容**的常态）⇒ ⇒ ⇒
/// **两个数，两个含义**（一个约束一个落点）。
///
/// ⚠️ **版本 0 与 > 2 不在其中** ⇒ ⇒ 「完全不认识」与「未来格式」都
/// **明确拒**（⇒ ⇒ 既有码 `E_PLT-01`）⇒ ⇒ ⇒ **不静默硬解**。
/// ⚠️ **v0.74（STAGE-50）**：`[1, 2, 3]` ⇒ ⇒ 版本 **2** 的老描述（存量板
///   档案）**仍能装**（⇒ ⇒ **缺省优先**）⇒ ⇒ 版本 **3** 是本批写的。
/// ⚠️ **v0.76（STAGE-52）**：`[1, 2, 3, 4, 5]` ⇒ ⇒ 版本 **4** 的老描述
///   （存量板档案 · 带 `auto_selectable` 键）**仍能装**（serde 忽略未知键）
///   ⇒ ⇒ 版本 **5** 是本批写的。
pub const SUPPORTED_PLATFORM_SCHEMA_VERSIONS: &[u32] = &[1, 2, 3, 4, 5];
/// The version of the platform description's **legality rule set**.
///
/// ⚠️ **This is not [`PLATFORM_SCHEMA_VERSION`]** and it is not the
/// contract's [`crate::SCHEMA_VERSION`] either ⇒ the three numbers mean
/// three different things and none of them replaces another:
///
/// | constant | judges | v0.44 |
/// | --- | --- | --- |
/// | [`crate::SCHEMA_VERSION`] | the **message** shape (the contract envelope) | `8` |
/// | [`PLATFORM_SCHEMA_VERSION`] | the **description** shape (what a description looks like) | `1` |
/// | `PLATFORM_RULES_VERSION` | the **legality rules** (which descriptions are legal) | `3` |
///
/// ⚠️ **Version 1** = the rule set **before** V-17 ⇒ **version 2** = the
/// rule set **with** V-17 (every peripheral device's `id` must be
/// non-empty) ⇒ **version 3** = the rule set with the later cross-field
/// checks ⇒ ⇒ each step adds **exactly** the rules it names and nothing
/// else.
///
/// ⚠️ **v0.73（STAGE-49）**：**version 4** = the rule set **with**
/// V-20（角色声明）/ V-21（引脚表）/ **V-22**（缺 `base` ⇒ ⇒ 由装配期
/// 分配 ⇒ ⇒ **V-5 / V-6 在基址未定的那一段上判不了** ⇒ ⇒ ⇒ **分配器
/// 负责**）⇒ ⇒ ⇒ **规则集跟着描述格式走**（描述格式升到 **2** ⇒ ⇒ 这个
/// 也升到 **4**）⇒ ⇒ ⇒ **一次升版**（⇒ ⇒ §4.1「一次升版、一次迁移」）。
///
/// ⚠️ **v0.34 (D-125)**: the rule set **tightened** ⇒ 2 → 3 ⇒ the boards
/// stored under version 2 go through v0.33's migration path (an
/// **explicit** prompt, the kernel's verdict, then the stored entry is
/// **upgraded**) ⇒ ⇒ the front end writes **no** second validator ⇒ the
/// legality of a description is judged by **this** module only.
///
/// ⚠️ **Every time the rule set is tightened ⇒ this number goes +1.**
/// That is what makes 「存量怎么办」 (what happens to the boards stored
/// earlier) **decidable** instead of a matter of taste: a stored board
/// can carry the rule version it was accepted under ⇒ the front end can
/// say 「this board was saved under the old rules」 instead of silently
/// letting it fail at submit time.
///
/// ⚠️ **Tightening a rule does NOT change the description's shape** ⇒
/// [`PLATFORM_SCHEMA_VERSION`] does **not** move with it ⇒ ⇒ no stored
/// description becomes unreadable.
///
/// ⚠️ **v0.73（STAGE-49）**：这一批**两件事同时发生** ⇒ ⇒ 描述格式**改了**
/// （`base` 可缺省 ⇒ ⇒ `PLATFORM_SCHEMA_VERSION` 1 → 2）⇒ ⇒ **规则集也
/// 跟着走**（3 → 4）⇒ ⇒ ⇒ **存量板档案要迁**（⇒ ⇒ **缺省优先**：老描述
/// **带** `base` ⇒ ⇒ **仍能装** ⇒ ⇒ 前端**显式提示**「按旧规则存的」
/// ⇒ ⇒ **不自动迁** ⇒ ⇒ 用户**手动**才升级）。
/// ⚠️ **v0.74（STAGE-50）**：**4 → 5** ⇒ ⇒ 连线事件**词表扩项**
/// （`pin_irq`）⇒ ⇒ V-18 的判据从「单常量比较」改成「**查词表**」⇒ ⇒
/// **规则收紧** ⇒ ⇒ **规则集跟着 +1**。
/// ⚠️ **v0.75（STAGE-51）**：**规则集 6** ⇒ ⇒ 加 **V-23**（`mcu` 段词法/
/// 形状校验；码 `E_PLT-21`）⇒ ⇒ V-22 编号不变（仍属装配期分配器）。
/// ⚠️ **v0.76（STAGE-52）**：**规则集 7** ⇒ ⇒ V-21 语义**扩展**（引脚规格
///   `PinSpec`：器件在 + 引脚号 + 方向 + 同引脚唯一 + 冲突）＋ 加 **V-24**
///   **V-26**（无板装载 ⇒ 码 `E_PLT-22`）。
/// ⚠️ **v0.78**：**规则集 8** ⇒ ⇒ 加 **V-27**（一块板最多一个 PLIC；
///   码 `E_PLT-24`）。
pub const PLATFORM_RULES_VERSION: u32 = 8;
/// The **upper bound** of a CLINT's timer frequency (1 THz).
///
/// ⚠️ The platform's smallest time granularity is **1 ps**
/// (`PS_PER_SEC = 1e12`) ⇒ a frequency above 1 THz means a period
/// **shorter than one picosecond** ⇒ the platform cannot express it (the
/// device model's `ps_per_cycle()` would compute `0`).
///
/// ⚠️ This is the **only** place the number lives: M-15 / the host
/// **reference** it, they never copy it (one constant, one truth — the
/// same discipline as v0.22's `MSTATUS_WARL_MASK`).
pub const MAX_CLINT_FREQ_HZ: u64 = 1_000_000_000_000;
/// The **upper bound** of the number of harts a CLINT serves (4096).
///
/// ⚠️ From the device model's register layout (not from taste): the
/// `msip` area is `0x0 + 4×hart` and must not run into the `mtimecmp`
/// area at `0x4000` ⇒ `hart < 4096`; the `mtimecmp` area
/// (`0x4000 + 8×hart`) must not run into the `mtime` area at `0xBFF8` ⇒
/// the same bound, from another road.
pub const MAX_CLINT_HARTS: u32 = 4096;
/// The **upper bound** of the number of interrupt sources a PLIC has
/// (1024).
///
/// ⚠️ From the device model's register layout: the Priority area is
/// `0x0 + 4×source` and must not run into the Pending area at `0x1000`
/// ⇒ `source < 1024`; the Enable area gives the same bound from another
/// road (32 words per context ⇒ `ceil(sources/32) ≤ 32`).
pub const MAX_PLIC_SOURCES: u32 = 1024;
/// The **upper bound** of the number of interrupt contexts a PLIC has
/// (16).
///
/// ⚠️ From the device model's register layout: the Threshold / Claim area
/// is `0x200000 + 0x1000×ctx` and must stay inside the `0x210000` window
/// ⇒ `ctx ≤ 16`.
pub const MAX_PLIC_CONTEXTS: u32 = 16;
/// **GPIO 缺省中断号** —— 描述里没写 `irq_line` 时取 `11`
/// （与 QEMU virt 的 GPIO 源习惯一致；`0` 是保留号，永远不合法）。
pub fn default_gpio_irq_line() -> u16 {
    11
}
/// A platform description — the declared memory map of one board.
///
/// The wire form is JSON (the only serialization format the repository
/// already uses — see the frozen spec §1.1), e.g.:
///
/// ```json
/// {
///   "schema_version": 1,
///   "id": "qemu-virt",
///   "name": "QEMU virt",
///   "priority": 20,
///   "region_budget_bytes": 134217728,
///   "regions": [
///     { "kind": "ram", "base": 0, "size": 1048576 },
///     { "kind": "peripheral", "base": 1073741824,
///       "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } }
///   ]
/// }
/// ```
///
/// ⚠️ **v0.76（STAGE-52 · B3 决议）**：`auto_selectable` 字段**废弃** ⇒ ⇒
/// 老描述**带这个键仍能解析**（serde 忽略未知字段 ⇒ ⇒ 存量板档案零风险）；
/// 内置板降级为**模板预设**，由用户**显式导入**（`query_templates` 那条线）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformDescription {
    /// Format version of this description — must be
    /// [`PLATFORM_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Platform identifier — the platform's **primary key** (built-in
    /// descriptions are selected by it). Must not be empty.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// 模板展示顺序 — **smaller wins**（模板库列表排序用 · 不再参与自动选档）。
    pub priority: u32,
    /// Per-region budget in bytes — the ceiling for one region's
    /// **declared** size.
    ///
    /// ⚠️ This is a **declaration-time** budget. The runtime budget
    /// (M-14's `E_BUS-09`) measures the **physical occupancy** instead
    /// (RAM is lazily allocated) ⇒ two constraints, two layers, two
    /// codes.
    ///
    /// ⚠️ **v0.31 (D-138)**: this field used to carry **two** meanings
    /// (the declaration-time ceiling **and** the runtime physical
    /// occupancy ceiling) ⇒ they are split: the declaration-time one
    /// stays here (V-7 / [`E_PLT_07_OVER_BUDGET`]), the runtime one
    /// moved to [`PlatformDescription::max_region_bytes`].
    pub region_budget_bytes: u32,
    /// The **runtime** ceiling for one region's **physical occupancy**
    /// (optional) — `None` means **equal to** `region_budget_bytes`.
    ///
    /// ⚠️ **v0.31 (D-138)**: this is the **second half** of the split —
    /// the declaration-time ceiling is [`Self::region_budget_bytes`]
    /// (judged by V-7 / `E_PLT_07`), the runtime physical occupancy
    /// ceiling is **this** one (judged by M-14's `E_BUS-09`). Two
    /// layers, two semantics, two codes — the two are **not** merged.
    ///
    /// ⚠️ **`None` means "the same as the declaration-time budget"**,
    /// **not** "unlimited" ⇒ an old description (one without this key)
    /// behaves **byte for byte** as before. To say "unlimited"
    /// explicitly, write a large enough value (`u32::MAX` ≈ 4 GiB − 1)
    /// ⇒ one number is harder to misread than a vague word.
    ///
    /// ⚠️ **No `skip_serializing_if`**: the key is **always** written
    /// (the value may be `null`) ⇒ the wire shape is **unique** ⇒ the
    /// contract layer and the front-end mirror can be compared field by
    /// field.
    #[serde(default)]
    pub max_region_bytes: Option<u32>,
    /// The declared regions — may be listed in any order (the overlap
    /// check sorts by `base` first).
    pub regions: Vec<RegionSpec>,
    /// Human-readable note — the compensation for JSON's inability to
    /// carry comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// **角色声明**（v0.73 · STAGE-49 · D-177 的 C）—— 哪个实例扮演 console /
    /// 时间源 / 主 PLIC。
    ///
    /// ⚠️ **缺省 ⇒ 今天的口径逐字不变**（裁决 E）⇒ ⇒ **加法**⇒ ⇒ 存量板档案
    /// **零影响**。
    ///
    /// ⚠️ **指了不存在的器件 ⇒ ⇒ 拒**（`E_PLT-18`）⇒ ⇒ **不静默回落**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<DeviceRoles>,
    /// **MCU 段**（v0.75 · STAGE-51 · D-175 的 F）—— 这块板上的处理器
    /// （α 路线：一块板一个 MCU）。
    ///
    /// ⚠️ **可选** ⇒ ⇒ **缺省不带键** ⇒ ⇒ 无 `mcu` ⇒ **无 ISA 校验** ⇒ ⇒
    /// 存量板档案**逐字不变**（缺省优先迁移）。
    ///
    /// ⚠️ **本批唯一行为**：给 `mcu.isa` 时，内核装载固件**按 ISA 校验**
    /// （class + c 位）⇒ ⇒ 不匹配 ⇒ `E_PLT-20`；`mcu` 段自身词法非法 ⇒
    /// V-23 拒 `E_PLT-21`。vendor / model / vector_table 仅身份数据。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcu: Option<McuSpec>,
}
/// One declared region — internally tagged by **`kind`**.
///
/// ⚠️ The internal tag's key is literally `kind`, matching the region
/// kind vocabulary of [`crate::session::RegionDto::kind`] — so a
/// description needs **no** translation when it is compared with an
/// exported memory map.
///
/// ⚠️ **v0.73（STAGE-49 · B2）**：三个变体的 `base` 都从 `u32` 改成
/// **`Option<u32>`** ⇒ ⇒ **缺省 = 由内核在装配期分配**（⇒ ⇒ 这是**本批
/// 唯一改形状**的那一项 ⇒ ⇒ `PLATFORM_SCHEMA_VERSION` 1 → 2）。
///
/// ⚠️ **缺省优先**：老描述**带** `base` ⇒ ⇒ **逐字按老办法**装 ⇒ ⇒
/// **一个字节都不变** ⇒ ⇒ 存量板档案**零影响**。
///
/// ⚠️ **`skip_serializing_if`**：缺 `base` 的那段 ⇒ ⇒ 线上**不带** `base`
/// 键 ⇒ ⇒ 与前端镜像**同形** ⇒ ⇒ 「缺省」在**线上是可表达的**（不是
/// `null`、不是 `0`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RegionSpec {
    /// Ordinary memory (lazily allocated since v0.26).
    Ram {
        /// Interval base address — **`None`** = 由内核在装配期分配
        /// （v0.73 · STAGE-49 · B2）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<u32>,
        /// Interval size in bytes.
        size: u32,
    },
    /// Read-only memory (writes are silently dropped).
    Rom {
        /// Interval base address — **`None`** = 由内核在装配期分配
        /// （v0.73 · STAGE-49 · B2）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<u32>,
        /// Interval size in bytes.
        size: u32,
    },
    /// A peripheral window.
    ///
    /// ⚠️ **There is deliberately no `size` field here**: the window size
    /// is the device's own register map (`Peripheral::size()` in the
    /// contract layer's `port` module), so writing it into the
    /// description would create a **second source of truth** that goes
    /// stale the moment a device's register map changes.
    Peripheral {
        /// Interval base address — **`None`** = 由内核在装配期分配
        /// （v0.73 · STAGE-49 · B2）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<u32>,
        /// The device sitting at `base`.
        device: DeviceSpec,
    },
}
/// **角色声明**（v0.73 · STAGE-49 · D-177 的 C）—— 哪个器件实例扮演哪个角色。
///
/// ⚠️ **三个角色**：`console`（宿主跟它说话的那台 · 今天 = 第一个 UART）·
///   `time_source`（`time` / `mtimecmp` 的来源 · 今天 = 第一个 CLINT）·
///   `main_plic`（外部中断线从哪儿来 · 今天 = 第一个 PLIC）。
///
/// ⚠️ **缺省 ⇒ 今天的口径逐字不变**（裁决 E）⇒ ⇒ **加法**⇒ ⇒ 存量板档案
/// **零影响**（STAGE-16 的同形先例）。
///
/// ⚠️ **指了不存在的器件 ⇒ ⇒ 拒**（`E_PLT-18`）⇒ ⇒ **不静默回落**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeviceRoles {
    /// 哪台器件是 **console**（缺省 = 第一个 UART）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console: Option<String>,
    /// 哪台器件是**时间源**（缺省 = 第一个 CLINT）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_source: Option<String>,
    /// 哪台器件是**主 PLIC**（缺省 = 第一个 PLIC）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_plic: Option<String>,
}
/// **MCU 段**（v0.75 · STAGE-51 · D-175 的 F）—— 这块板上的处理器是什么。
///
/// ⚠️ **α 路线（一块板一个 MCU）**：它是**顶层属性**，**不是**器件、**不是**
/// 角色 ⇒ ⇒ 缺省（无 `mcu` 键）⇒ ⇒ **无 ISA 校验**（存量行为**逐字不变**）。
///
/// ⚠️ **本批唯一内核行为 = 按 `isa` 校验固件**（V-23 + 内核装载门禁）；
/// `vendor` / `model` / `vector_table` 是**身份数据**（前端展示、契约往返、
/// 内核接受但不据此行为）⇒ ⇒ **边界如实登记**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McuSpec {
    /// **ISA 串**（如 `rv32ima`）—— 词法见 [`mcu_isa_info`]（**一个落点**）。
    pub isa: String,
    /// 厂商（自由文本）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// 型号（自由文本）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 向量表（可选 · 声明 + 展示，不做校验）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector_table: Option<VectorTableSpec>,
}
/// **向量表声明**（v0.75 · STAGE-51 · D-175 的 F）—— 这颗 MCU 的
/// 复位/中断向量表布局（**数据手册信息**，非运行时行为）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorTableSpec {
    /// 向量表首地址（RISC-V 惯例 = 复位入口）。
    pub base: u32,
    /// 表项数（× 4 字节 = 表长）。
    pub entries: u32,
}
/// **解析出的 ISA 事实**（V-23 与内核装载门禁共用 —— **一个落点**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McuIsaInfo {
    /// 基础位宽：`32` 或 `64`。
    pub bits: u32,
    /// 声明了压缩指令扩展（`c`）。
    pub has_c: bool,
    /// 声明了嵌入式扩展（`e`）。
    pub has_e: bool,
}
/// **ISA 词法解析 —— 唯一落点**（v0.75 · STAGE-51 · D-175 的 F）。
///
/// ⚠️ 契约层 V-23 用它判 `mcu.isa` 合不合法 ⇒ ⇒ 内核装载门禁用它取
/// class / c 位判据 ⇒ ⇒ **一次查表，两处都用**（与事件词表同款「一个落点」）。
///
/// ⚠️ 词法：`rv` + `32`|`64` + 扩展字母串（字符集 `{i,e,m,a,f,d,c}`，
/// **去重 + 规范序** `i e m a f d c`）；大小写**归一**（`"RV32IMA"` ⇒ 合法，
/// 语义 = `"rv32ima"`）。`"rv32"`（无扩展字母）与 `"arm"` 等 ⇒ `None`。
///
/// ⚠️ 只做**词法**：m/a/f/d 等扩展在 ELF 头里查不到 ⇒ ⇒ 词法合法**不等于**
/// 固件能验证 —— 那件事在内核装载门禁（本批能查的只有 class 与 c 位）。
pub fn mcu_isa_info(isa: &str) -> Option<McuIsaInfo> {
    let s = isa.trim().to_ascii_lowercase();
    let rest = s.strip_prefix("rv")?;
    let (bits_str, ext) = rest.split_at(2);
    let bits: u32 = match bits_str {
        "32" => 32,
        "64" => 64,
        _ => return None,
    };
    if ext.is_empty() {
        return None;
    }
    // 去重 + 规范序：i e m a f d c（重复或乱序 ⇒ 非法 ⇒ 归一化后逐字比较）。
    let order: &[char] = &['i', 'e', 'm', 'a', 'f', 'd', 'c'];
    let mut prev = -1i32;
    let mut has_c = false;
    let mut has_e = false;
    for ch in ext.chars() {
        let idx = order.iter().position(|&c| c == ch)?;
        let i = idx as i32;
        if i <= prev {
            return None;
        }
        prev = i;
        if ch == 'c' {
            has_c = true;
        }
        if ch == 'e' {
            has_e = true;
        }
    }
    Some(McuIsaInfo { bits, has_c, has_e })
}
/// One declared peripheral device — internally tagged by **`device **`.
///
/// ⚠️ The internal tag's key is literally `device` (**not** `kind` —
/// that one is already the region's tag, and **not** `type`): the wire
/// form is `{"device": "uart16550", "id": "UART0", ...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "device", rename_all = "snake_case")]
pub enum DeviceSpec {
    /// A GPIO bank.
    Gpio {
        /// The device id — the key the restore path matches on.
        id: String,
        /// 这块 **GPIO 整块**在 PLIC 上的中断源号（`1..plic.sources`，`0` 保留）。
        ///
        /// ⚠️ **整块一个号**：任意引脚有事件就抬这一个源，全部清空才撤 ⇒
        ///   不按引脚分号。缺省 `11`（与 QEMU virt 的 GPIO 源习惯一致）。
        #[serde(default = "default_gpio_irq_line")]
        irq_line: u16,
    },
    /// An ns16550 UART.
    Uart16550 {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The interrupt line this UART raises.
        irq_line: u16,
        /// The nominal baud rate.
        baud_rate: u32,
    },
    /// A CLINT (timer / software interrupt).
    Clint {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The timer frequency in Hz.
        freq_hz: u64,
        /// The number of harts this CLINT serves.
        harts: u32,
    },
    /// A PLIC (interrupt controller).
    Plic {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The number of interrupt sources.
        sources: u32,
        /// The number of interrupt contexts.
        contexts: u32,
    },
    /// A RISC-V style timer (`mtime` / `mtimecmp`，**无** hart 数组).
    ///
    /// ⚠️ **v0.73（STAGE-49 · G）**：内核**早就实现了**这台器件（M-15 注册表
    ///   的第五个类型 `timer_riscv`）⇒ ⇒ 但描述格式**写不出来**它 ⇒ ⇒ 这
    ///   就是 STAGE-43 裁决 J 说的「内核实现了、描述还写不出来」⇒ ⇒ 本变体
    ///   **补上那个缺口**。
    ///
    /// ⚠️ **参数只有两个**（`id` + `irq_line`）⇒ ⇒ 与内核的
    ///   `TimerRiscv::new(id, base, irq_line)` **逐字对齐** ⇒ ⇒ **不**加
    ///   `freq_hz` / `harts`（那两样是 **CLINT** 的，不是它的）⇒ ⇒ ⇒ **不造
    ///   第二个真相**。
    TimerRiscv {
        /// The device id — the key the restore path matches on.
        id: String,
        /// The interrupt line this timer raises.
        irq_line: u16,
    },
}
impl DeviceSpec {
    /// **Every kind a platform description can express** — the *instantiable*
    /// set (STAGE-43 · v0.66 · D-175 · 裁决 H / I).
    ///
    /// ⚠️ **This is NOT the same thing as "what the kernel implements"**:
    /// - **this** = what the *description format* can write (today **5**);
    /// - the kernel's *device type registry* (M-15) = what the kernel
    ///   implements (today **5**).
    ///
    /// ⇒ ⇒ The invariant is **"instantiable ⊆ implemented"** ⇒ ⇒ the
    /// reverse direction is **not** required.
    ///
    /// ⚠️ **v0.73（STAGE-49 · G）**：**两边相等了** ⇒ ⇒ 此前 STAGE-43 裁决 J
    ///   说的「内核实现了、描述还写不出来」的那一条（`timer_riscv`）**补上了**
    ///   ⇒ ⇒ ⇒ 本清单**多**了 `timer_riscv` ⇒ ⇒ 前端的器件下拉**跟着多**
    ///   一项（跨层门禁**逐字**比这两份 ⇒ ⇒ 少一侧就红）。
    ///
    /// ⚠️ **One landing spot** (裁决 I) ⇒ ⇒ the front end's `DEVICE_KINDS`
    /// is checked against **this** list by a cross-layer gate ⇒ ⇒ a new
    /// variant here turns the front end **red** instead of drifting.
    pub const KINDS: [&'static str; 5] = ["gpio", "uart16550", "clint", "plic", "timer_riscv"];
}
impl PlatformDescription {
    /// Validate this description against the frozen rules.
    ///
    /// The rules, in the order they are checked (the first violated one
    /// is the one reported):
    ///
    /// | rule | judgement | code |
    /// | --- | --- | --- |
    /// | V-1 | `schema_version != PLATFORM_SCHEMA_VERSION` | `E_PLT-01` |
    /// | V-2 | the **description's** `id` is empty | `E_PLT-02` |
    /// | V-3 | `regions` is empty | `E_PLT-03` |
    /// | V-4 | a RAM / ROM region's `size == 0` | `E_PLT-04` |
    /// | V-5 | `base + size` overflows `u32` | `E_PLT-05` |
    /// | V-6 | two regions overlap | `E_PLT-06` |
    /// | V-7 | a RAM / ROM region's **declared** size `> region_budget_bytes` | `E_PLT-07` |
    /// | V-8 | unknown device kind / missing parameter / wrong parameter type | **serde** (not here) |
    /// | V-9 | two peripheral devices share an `id` | `E_PLT-08` |
    /// | V-27 | a board declares **more than one** `plic` device | `E_PLT-24` |
    /// | V-13 | a device parameter is **below** its legal lower bound | `E_PLT-12` |
    /// | V-14 | a device parameter is **above** its legal upper bound | `E_PLT-12` |
    /// | V-15 | an interrupt peripheral's `irq_line` is not a valid PLIC source id (only when the map has a PLIC) | `E_PLT-12` |
    /// | V-16 | `max_region_bytes` is given and is **zero** (the runtime ceiling must be ≥ 1) | `E_PLT-12` |
    /// | V-17 | a peripheral device's `id` is empty | `E_PLT-15` |
    /// | V-15b | two interrupt peripherals on one board declare the same PLIC source id | `E_PLT-16` |
    ///
    /// ⚠️ **V-2 judges the *description's* `id`** — the **device's** `id`
    /// is judged by **V-17** ⇒ they are two different objects with two
    /// different codes ⇒ neither rule may be used for the other.
    ///
    /// ⚠️ **V-8 lives in the serde layer, not here.** An unknown device
    /// kind or a missing parameter fails during deserialization, before
    /// any value exists to validate ⇒ this method never sees it. That is
    /// why the **declaration** rules carry **eight** scenario codes and
    /// not nine.
    ///
    /// ⚠️ **V-10 ~ V-12 are not judged here either** — they are the
    /// *host-side* lookup rules (unknown platform id, unreadable file,
    /// malformed file). Their codes are owned by the contract layer
    /// (`E_PLT-09` .. `E_PLT-11`) so that a code has exactly one landing
    /// spot, but the **host** is the one that detects them.
    ///
    /// ⚠️ **V-13 ~ V-15 were appended after V-9** (v0.30 · D-141) ⇒ the
    /// report order of V-1 ~ V-9 is **unchanged, byte for byte** ⇒ no
    /// existing judgement moves. They **share one** scenario code
    /// (`E_PLT-12`): a single field out of range and a broken cross-field
    /// relation mean the same thing — 「某个器件参数的值不在它的合法取值域内」
    /// ⇒ one code, not two.
    ///
    /// ⚠️ **V-16 was appended after V-15** (v0.31 · D-138) ⇒ the report
    /// order of V-1 ~ V-15 is **unchanged, byte for byte** ⇒ the
    /// existing "V-2 before V-9 / V-13" order assertions still hold. It
    /// **shares** `E_PLT-12` as well: a zero runtime ceiling is a
    /// parameter outside its legal value domain ⇒ one code, not two.
    ///
    /// ⚠️ **V-17 was appended after V-16** (v0.33 · D-142) ⇒ the report
    /// order of V-1 ~ V-16 is **unchanged, byte for byte** ⇒ in
    /// particular **V-9 still comes first** ⇒ **two** devices with an
    /// empty `id` are still rejected by **V-9 / `E_PLT-08`** ⇒ V-17
    /// only ever fires for a **single** empty `id`.
    ///
    /// ⚠️ **Judgements never look at the message text** — they assert
    /// `family == Platform && scenario == <code>`. Rewording a message
    /// must not turn a test red, and a message must never carry the
    /// meaning.
    ///
    /// ⚠️ **V-7 is a declaration-time check and is deliberately not M-14's
    /// `E_BUS-09`**: v0.26 made RAM lazily allocated, so the runtime
    /// guard measures the **physical occupancy** and no longer catches a
    /// huge *declaration* ⇒ the description layer is the only place left
    /// that can ⇒ it must. Two layers, two semantics, two codes.
    pub fn validate(&self) -> Result<(), PlatformError> {
        // V-1 — the format version this build speaks.
        //
        // ⚠️ **v0.73（STAGE-49 · B2）**：**接受版本 1 与版本 2** ⇒ ⇒
        // **缺省优先**（规格 §2.2）：老描述（`schema_version = 1` ⇒ ⇒ 它的
        // 区域**都带** `base`）⇒ ⇒ **仍能装** ⇒ ⇒ **一个字节都不变** ⇒ ⇒
        // 存量板档案**零风险**（裁决 K）。
        //
        // ⚠️ **为什么接受两个版本**：描述格式升版（1 → 2）**只**加了「`base`
        // 可缺省」这一件事 ⇒ ⇒ **版本 1 的描述是版本 2 的子集** ⇒ ⇒ ⇒
        // 「老格式」**不是**「坏格式」⇒ ⇒ ⇒ **不**该被拒。
        //
        // ⚠️ **版本 0 与 > 2 一律拒**（⇒ ⇒ 「完全不认识」与「未来格式」都
        // 不许**硬解**）⇒ ⇒ ⇒ 仍是**既有码 `E_PLT-01`**（⇒ ⇒ **零新码**）。
        if !SUPPORTED_PLATFORM_SCHEMA_VERSIONS.contains(&self.schema_version) {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_01_SCHEMA_VERSION,
                format!(
                    "平台描述的格式版本不受支持：{} 不在支持的版本 {:?} 里",
                    self.schema_version, SUPPORTED_PLATFORM_SCHEMA_VERSIONS
                ),
            ));
        }

        // V-2 — the platform's primary key must name something.
        if self.id.is_empty() {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_02_EMPTY_ID,
                "平台描述的 id 不能为空",
            ));
        }

        // V-3 — no region at all is no memory map.
        if self.regions.is_empty() {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_03_NO_REGIONS,
                "平台描述没有声明任何区域",
            ));
        }

        // V-4 + V-5 — per region, in the author's order.
        //
        // ⚠️ Only RAM / ROM have a **declared** size here; a peripheral
        // window's size is the device's own register map (裁决 D), so
        // there is nothing to check for it at this layer.
        //
        // ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段（⇒ ⇒ 由内核在
        // 装配期分配）⇒ ⇒ **V-4 与 V-7 照判**（它们只管**大小** ⇒ ⇒ 与
        // 基址无关）⇒ ⇒ 但 **V-5（溢出）判不了**（⇒ ⇒ 那是**基址**的事）
        // ⇒ ⇒ ⇒ **V-5 由装配期分配器负责**（分配器知道分配出来的基址
        // ⇒ ⇒ 越界 ⇒ ⇒ **报既有码 `E_PLT-05`**）⇒ ⇒ ⇒ **零新场景码**。
        for region in &self.regions {
            let Some(size) = region.declared_size() else {
                continue;
            };
            if size == 0 {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_04_ZERO_SIZE,
                    format!("区域 {:#x} 的大小为 0", region.base().unwrap_or(0)),
                ));
            }
            // ⚠️ **基址未定 ⇒ ⇒ V-5 判不了** ⇒ ⇒ 跳过（分配器负责）。
            let Some(base) = region.base() else {
                continue;
            };
            if base.checked_add(size).is_none() {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_05_OVERFLOW,
                    format!("区域 {:#x} + {:#x} 越过了 u32 的地址空间", base, size),
                ));
            }
        }

        // V-6 — overlap.
        //
        // ⚠️ **Sorted by `base` first** (the description may list the
        // regions in any order) and then compared pair by pair. The
        // description holds a handful of regions, so the quadratic scan
        // costs nothing and needs no "adjacent pairs are enough"
        // argument to be obviously complete.
        //
        // ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段 ⇒ ⇒ **这里判
        // 不了**（它还没有位置）⇒ ⇒ ⇒ **不静默**跳过它 ⇒ ⇒ ⇒ **装配期
        // 分配器**在分配完之后**自己**判重叠（⇒ ⇒ 仍报**既有码
        // `E_PLT-06`**）⇒ ⇒ ⇒ **零新场景码**。
        //
        // ⚠️ **排序键**：缺 `base` 的段排在**前面**（`None` < `Some`）⇒ ⇒
        // 排序**稳定**（`sort_by_key` 是稳定的）⇒ ⇒ ⇒ 缺基址的段之间
        // **保持声明顺序** ⇒ ⇒ ⇒ 判据顺序**可复算**。
        let mut sorted: Vec<&RegionSpec> = self.regions.iter().collect();
        sorted.sort_by_key(|region| region.base());
        for i in 0..sorted.len() {
            for j in (i + 1)..sorted.len() {
                // ⚠️ **判不出来**（有一段缺基址）⇒ ⇒ **跳过这一对**
                // ⇒ ⇒ ⇒ **不**当成「不重叠」⇒ ⇒ ⇒ 分配器负责最终裁决。
                let Some(overlap) = regions_overlap(sorted[i], sorted[j]) else {
                    continue;
                };
                if overlap {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_06_OVERLAP,
                        format!(
                            "区域 {:#x} 与区域 {:#x} 重叠",
                            sorted[i].base().unwrap_or(0),
                            sorted[j].base().unwrap_or(0)
                        ),
                    ));
                }
            }
        }

        // V-7 — the **declared** size against the description's budget.
        //
        // ⚠️ `>` and not `>=`: a region whose size is **exactly** the
        // budget is allowed (the built-in `qemu-virt` description declares
        // a 128 MiB RAM against a 128 MiB budget) — the same wording as
        // M-14's runtime guard.
        for region in &self.regions {
            let Some(size) = region.declared_size() else {
                continue;
            };
            if size > self.region_budget_bytes {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_07_OVER_BUDGET,
                    format!(
                        "区域 {:#x} 声明的大小 {:#x} 超过了每区域预算 {:#x}",
                        region.base().unwrap_or(0),
                        size,
                        self.region_budget_bytes
                    ),
                ));
            }
        }

        // V-9 — two devices with one id.
        //
        // ⚠️ The restore path matches a device state onto a live instance
        // **by id** ⇒ two entries with one id are two states competing
        // for one instance. The check spans **all** device kinds (a UART
        // and a GPIO may not share an id either) — same rule as the
        // session payload's own duplicate-id check.
        let mut seen: Vec<&str> = Vec::new();
        for region in &self.regions {
            let Some(id) = region.device_id() else {
                continue;
            };
            if seen.contains(&id) {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_08_DUPLICATE_DEVICE_ID,
                    format!("外设器件的 id 重复：{id}"),
                ));
            }
            seen.push(id);
        }

        // V-27 — one board, **one** PLIC (v0.78).
        //
        // ⚠️ **Why declaration-time**：外设抬源与 claim 共用**同一个**
        // PLIC（`main_plic`，缺省 = 第一个）⇒ ⇒ 第二个 PLIC 永远收不到
        // 外设源 ⇒ ⇒ 允许它等于允许一份「装得上但中断半通」的板描述 ⇒ ⇒
        // 在声明期拒掉，理由还能被看见。
        //
        // ⚠️ **数量上限，不是 id 冲突**：两个 PLIC 用不同 id 依然要被拒 ⇒
        // 不复用 `E_PLT_08`（那个判的是同 id）⇒ ⇒ 自己一个码。
        //
        // ⚠️ **编号最大、位置最前**：V-27 是**后加**的规则（编号只表示
        // 身份），但它必须排在 V-13 ~ V-15 之前 ⇒ ⇒ 多 PLIC 是**更根本**
        // 的板形状错误；否则一份「两个 PLIC + 越界 irq_line」会先报
        // `E_PLT-12`（V-15），把真正的根因藏起来。改成前置后，**单 PLIC
        // 的板**报错顺序逐字不变（多 PLIC 的板以前反正也装不上）。
        let plic_ids: Vec<&str> = self
            .regions
            .iter()
            .filter_map(|region| match region {
                RegionSpec::Peripheral {
                    device: DeviceSpec::Plic { id, .. },
                    ..
                } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        if plic_ids.len() > 1 {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_24_MULTIPLE_PLIC,
                format!(
                    "一块板最多声明一个 PLIC（实际 {} 个：{}）",
                    plic_ids.len(),
                    plic_ids.join(", ")
                ),
            ));
        }

        // V-13 / V-14 / V-15 — the devices' **legal value domain**
        // (v0.30 · D-141 · frozen spec STAGE-15 §3.1).
        //
        // ⚠️ **Appended after V-9** ⇒ the report order of V-1 ~ V-9 is
        // unchanged, byte for byte (the frozen spec's acceptance A-9).
        //
        // ⚠️ **All three rules share one scenario code** (`E_PLT-12`): a
        // single field out of range and a broken cross-field relation
        // mean the same thing — 「某个器件参数的值不在它的合法取值域内」
        // ⇒ one code, not two.
        //
        // ⚠️ The description layer **rejects as written** — it does not
        // "helpfully" correct the value (the device models clamp
        // `harts` / `freq_hz` / `sources` / `contexts` to at least 1, which
        // would silently hide the mistake ⇒ the model layer's clamping is
        // only a fallback for code paths that bypass the description).

        // V-13 — the **lower** bounds (one field at a time).
        for region in &self.regions {
            let RegionSpec::Peripheral { device, .. } = region else {
                continue;
            };
            for (field, value, lower, _) in device.numeric_parameters() {
                if value < lower {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（{kind}）的参数 {field} = {value} 小于合法下界 {lower}",
                            id = device.id(),
                            kind = device.kind(),
                        ),
                    ));
                }
            }
        }

        // V-14 — the **upper** bounds (one field at a time).
        //
        // ⚠️ Only the fields that have an upper bound at all take part:
        // `uart16550.baud_rate` has **none** (the RISC-V architecture does
        // not cap the baud rate) ⇒ **no number is invented** for it.
        for region in &self.regions {
            let RegionSpec::Peripheral { device, .. } = region else {
                continue;
            };
            for (field, value, _, upper) in device.numeric_parameters() {
                let Some(upper) = upper else {
                    continue;
                };
                if value > upper {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（{kind}）的参数 {field} = {value} 超过合法上界 {upper}",
                            id = device.id(),
                            kind = device.kind(),
                        ),
                    ));
                }
            }
        }

        // V-15 — the **cross-field** rule: a UART's `irq_line` must be a
        // valid PLIC source id.
        //
        // ⚠️ **Judged only when the map has a PLIC** (裁决 G): no PLIC does
        // **not** make the board illegal — it just has no interrupt
        // routing ⇒ `irq_line` is meaningless but harmless ⇒ refusing such
        // a board would refuse a legal one.
        //
        // ⚠️ V-27 already guarantees **at most one** PLIC ⇒ the bound is
        // that PLIC's `sources`; there is no "smallest across many" left.
        let plic_sources = self.regions.iter().filter_map(|region| {
            let RegionSpec::Peripheral {
                device: DeviceSpec::Plic { sources, .. },
                ..
            } = region
            else {
                return None;
            };
            Some(*sources)
        });
        if let Some(min_sources) = plic_sources.min() {
            for region in &self.regions {
                let RegionSpec::Peripheral { device, .. } = region else {
                    continue;
                };
                // ⚠️ **v0.73（STAGE-49 · G）**：`timer_riscv` **也**带
                //   `irq_line` ⇒ ⇒ **同一条规则**管它 ⇒ ⇒ 否则「UART 的
                //   irq_line 要判、计时器的 irq_line 不判」⇒ ⇒ **同一个量两个
                //   口径** ⇒ ⇒ ⇒ 一个约束**一个落点**。
                //
                // ⚠️ **v0.78（本批）**：GPIO **整块**一个源号 ⇒ ⇒ 同一条
                //   规则也管它 ⇒ ⇒ 三个外设一个口径。
                let (id, irq_line) = match device {
                    DeviceSpec::Gpio { id, irq_line } => (id, *irq_line),
                    DeviceSpec::Uart16550 { id, irq_line, .. } => (id, *irq_line),
                    DeviceSpec::TimerRiscv { id, irq_line } => (id, *irq_line),
                    _ => continue,
                };
                // `0` 是保留号（PLIC 永不交付源 0）⇒ ⇒ 写了 `0` 就是非法。
                if irq_line == 0 || u32::from(irq_line) >= min_sources {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_12_PARAMETER_OUT_OF_RANGE,
                        format!(
                            "器件 {id}（{kind}）的参数 irq_line = {irq_line} 不是合法的 PLIC 中断源号（取值 1..{min_sources}）",
                            kind = device.kind(),
                        ),
                    ));
                }
            }
        }

        // V-15b — 同一块板上**一个源号只能属于一个外设**。
        //
        // ⚠️ PLIC 的源号是**板级资源** ⇒ 两个外设共用一个号 ⇒ guest 读
        //   `claim()` 拿到号也分不出是谁 ⇒ ⇒ 声明期就拒（`E_PLT-16`）。
        let mut seen_sources: Vec<(u16, &str)> = Vec::new();
        for region in &self.regions {
            let RegionSpec::Peripheral { device, .. } = region else {
                continue;
            };
            let (id, irq_line) = match device {
                DeviceSpec::Gpio { id, irq_line } => (id, *irq_line),
                DeviceSpec::Uart16550 { id, irq_line, .. } => (id, *irq_line),
                DeviceSpec::TimerRiscv { id, irq_line } => (id, *irq_line),
                _ => continue,
            };
            if let Some((_, other)) = seen_sources.iter().find(|(line, _)| *line == irq_line) {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_16_IRQ_SOURCE_INVALID,
                    format!(
                        "器件 {id}（{kind}）的中断号 {irq_line} 与器件 {other} 重复：同一块板上一个中断号只能属于一个外设",
                        kind = device.kind(),
                    ),
                ));
            }
            seen_sources.push((irq_line, id.as_str()));
        }

        // V-16 — the **runtime** ceiling's legal value domain
        // (v0.31 · D-138 · frozen spec STAGE-16 §2.3).
        //
        // ⚠️ **Appended after V-15** ⇒ the report order of V-1 ~ V-15 is
        // unchanged, byte for byte.
        //
        // ⚠️ **Why zero is illegal**: a runtime ceiling of **zero** would
        // make **every** region exceed it ⇒ the board could not be used
        // at all ⇒ it is rejected **here** (at declaration time) rather
        // than at the first write.
        //
        // ⚠️ The rule is judged on the **declared** value only: `None`
        // means "the same as the declaration-time budget" (and that one
        // is judged by V-7) ⇒ no number is invented here.
        if let Some(max_region_bytes) = self.max_region_bytes {
            if max_region_bytes == 0 {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_12_PARAMETER_OUT_OF_RANGE,
                    format!(
                        "运行期每区域的物理占用上界 {max_region_bytes} 为 0（必须 ≥ 1，否则每一段都超限）"
                    ),
                ));
            }
        }

        // V-17 — a device must have an **identity**
        // (v0.33 · D-142 · frozen spec STAGE-18 §2.1).
        //
        // ⚠️ **Appended after V-16** ⇒ the report order of V-1 ~ V-16 is
        // unchanged, byte for byte ⇒ in particular **V-9 still comes
        // first** ⇒ **two** devices with an empty `id` are still rejected
        // by V-9 / `E_PLT-08` ⇒ V-17 only ever fires for **one** of them.
        //
        // ⚠️ **Why it matters**: the restore path matches a device state
        // onto a live instance **by id** ⇒ a device with an **empty** id
        // matches nothing ⇒ its state is **silently lost** on restore ⇒
        // the declaration is rejected here, at declaration time, where
        // the reason can still be seen.
        //
        // ⚠️ **The judgement is `is_empty()`** — **not**
        // `trim().is_empty()` ⇒ the very same wording as V-2 ⇒ the
        // front end and the kernel do not diverge on what "empty" means
        // ⇒ `id: " "` (one space) is **legal** (a deliberate choice, not
        // an oversight).
        //
        // ⚠️ The device's `id` is read through the **same** accessor as
        // V-9's (`RegionSpec::device_id()`) ⇒ there is **one** way to
        // get a device's id in this module ⇒ no second spelling of it.
        for region in &self.regions {
            let Some(id) = region.device_id() else {
                continue;
            };
            if id.is_empty() {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_15_DEVICE_EMPTY_ID,
                    format!(
                        "外设器件（基址 {:#x}）的 id 为空",
                        region.base().unwrap_or(0)
                    ),
                ));
            }
        }

        // V-20 — the **role declaration** must name devices that are there
        // (v0.73 · STAGE-49 · D-177 的 C).
        //
        // ⚠️ **Appended after V-19** ⇒ the report order of V-1 ~ V-19 is
        // unchanged, byte for byte.
        //
        // ⚠️ **One code** (`E_PLT_18`) for the two shapes（器件**不存在** /
        // 器件**类型不对**）⇒ ⇒ 「**这份角色声明不合法**」⇒ ⇒ one code, not
        // two (同 E_PLT-12 / E_PLT-16 的做法).
        //
        // ⚠️ **不复用 `E_PLT_16`**：那个意思是「**这条连线**非法」⇒ ⇒ 角色
        // 指向一个不存在的器件是**另一个对象**⇒ ⇒ 一码两义 ⇒ 禁止（§7.1 订正 1）.
        if let Some(roles) = self.roles.as_ref() {
            let declared: Vec<(&str, &str)> = self
                .regions
                .iter()
                .filter_map(|region| match region {
                    RegionSpec::Peripheral { device, .. } => Some((device.id(), device.kind())),
                    _ => None,
                })
                .collect();
            for (role, id, wanted_kind) in [
                ("console", roles.console.as_deref(), "uart16550"),
                ("time_source", roles.time_source.as_deref(), "clint"),
                ("main_plic", roles.main_plic.as_deref(), "plic"),
            ] {
                let Some(id) = id else {
                    continue;
                };
                match declared.iter().find(|(did, _)| *did == id) {
                    None => {
                        return Err(PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_18_ROLES_INVALID,
                            format!("角色 {role} 指向的器件 {id} 不在本描述声明的器件集合里"),
                        ));
                    }
                    Some((_, kind)) if *kind != wanted_kind => {
                        return Err(PlatformError::new(
                            PLATFORM_FAMILY,
                            E_PLT_18_ROLES_INVALID,
                            format!("角色 {role} 指向的器件 {id} 是 {kind}，不是 {wanted_kind}"),
                        ));
                    }
                    Some(_) => {}
                }
            }
        }

        // V-23 — the **`mcu` segment** must be well-formed (v0.75 ·
        // STAGE-51 · D-175 的 F).
        //
        // ⚠️ **编号为什么是 V-23 而不是 V-22**：V-22 已被「缺 `base` ⇒ ⇒
        // 由装配期分配」占用（v0.73 的 B2，见规则集文档）⇒ ⇒ MCU 校验
        // **顺延到 V-23**（一次一个编号、一码一义）。
        //
        // ⚠️ **缺省（无 `mcu` 段）⇒ ⇒ 不校验**（存量行为逐字不变 —— 缺省
        // 优先迁移）。
        //
        // ⚠️ **只判 mcu 段自身形状**：isa 词法（[`mcu_isa_info`]，唯一落点）
        // 非法 ⇒ 拒；向量表表项数 0 ⇒ 拒。
        //
        // ⚠️ **「固件与 MCU 不匹配」不在这里判** —— 那是内核装载门禁的事
        // （`E_PLT-20`）⇒ ⇒ 这里是「mcu 段自己写错了」（`E_PLT-21`）⇒ ⇒
        // **两个对象、两个码**（一码一义纪律照旧）。
        if let Some(mcu) = self.mcu.as_ref() {
            if mcu_isa_info(&mcu.isa).is_none() {
                return Err(PlatformError::new(
                    PLATFORM_FAMILY,
                    E_PLT_21_MCU_INVALID,
                    format!(
                        "mcu 段的 isa 词法非法：{:?}（应为 rv32/rv64 + 扩展字母串 {{i,e,m,a,f,d,c}}）",
                        mcu.isa
                    ),
                ));
            }
            if let Some(vt) = mcu.vector_table.as_ref() {
                if vt.entries == 0 {
                    return Err(PlatformError::new(
                        PLATFORM_FAMILY,
                        E_PLT_21_MCU_INVALID,
                        format!("mcu 段向量表的表项数不能为 0（base={:#x}）", vt.base),
                    ));
                }
            }
        }

        Ok(())
    }
    /// The RAM window this description declares — the **base-maximal**
    /// RAM region's `(base, size)`; `None` when the description declares
    /// **no** RAM region at all.
    ///
    /// ⚠️ **This is the single landing spot of "which RAM segment is *the*
    /// one"** (frozen spec STAGE-19 §6.2 · 裁决 I): the candidate side
    /// and the reference side both ask **this** method ⇒ the two can no
    /// longer drift apart ⇒ the hand-copied addresses go away.
    ///
    /// ⚠️ **"Base-maximal", not "largest"**: the segment whose **base** is
    /// the highest one wins, whatever its size ⇒ the rule is a **total
    /// order on the base**, not a comparison of sizes ⇒ it is decided by
    /// the description alone and never by the host's preferences.
    ///
    /// ⚠️ **The size is widened to `u64`** — a RAM region's declared size
    /// is a `u32` in [`RegionSpec`] ⇒ the widening is **lossless** and it
    /// keeps the window's two halves in the same unit family as the
    /// reference side's decimal rendering.
    pub fn ram_window(&self) -> Option<(u32, u64)> {
        self.regions
            .iter()
            .filter_map(|region| match region {
                // ⚠️ **v0.73（STAGE-49 · B2）**：**缺 `base`** 的段 ⇒ ⇒
                // **跳过**（⇒ ⇒ 它还没有位置 ⇒ ⇒ 分配之后才有）⇒ ⇒ ⇒
                // **不**把它当成「基址 0」。
                RegionSpec::Ram { base, size } => base.map(|base| (base, u64::from(*size))),
                RegionSpec::Rom { .. } | RegionSpec::Peripheral { .. } => None,
            })
            .max_by_key(|(base, _)| *base)
    }
}
impl RegionSpec {
    /// The region's base address — **`None`** when it is **not declared**
    /// (v0.73 · STAGE-49 · B2 ⇒ ⇒ 由内核在装配期分配)。
    ///
    /// ⚠️ **这是 base 的唯一读取落点**（一个约束一个落点）⇒ ⇒ 想知道
    /// 「这段的基址定了没有」的地方**都调它** ⇒ ⇒ 不会有一处读 `Option`
    /// 的字段、另一处读别的。
    pub fn base(&self) -> Option<u32> {
        match self {
            RegionSpec::Ram { base, .. }
            | RegionSpec::Rom { base, .. }
            | RegionSpec::Peripheral { base, .. } => *base,
        }
    }
    /// **装配期分配之后**把基址**写回**这一段（v0.73 · STAGE-49 · B2）。
    ///
    /// ⚠️ **这是基址的唯一写入落点**（一个约束一个落点）⇒ ⇒ 分配器**不**
    /// 直接改 `RegionSpec` 的字段（⇒ ⇒ 绕过这个方法就绕过了「一处改」的
    /// 纪律）。
    pub fn set_base(&mut self, base: u32) {
        match self {
            RegionSpec::Ram { base: slot, .. }
            | RegionSpec::Rom { base: slot, .. }
            | RegionSpec::Peripheral { base: slot, .. } => *slot = Some(base),
        }
    }
    /// The region's **declared** size in bytes — `None` for a peripheral
    /// window, whose size belongs to the device's register map (裁决 D).
    pub fn declared_size(&self) -> Option<u32> {
        match self {
            RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => Some(*size),
            RegionSpec::Peripheral { .. } => None,
        }
    }
    /// The region kind — one of the three vocabulary strings of
    /// [`crate::session`] (`"ram"` / `"rom"` / `"peripheral"`).
    pub fn kind(&self) -> &'static str {
        match self {
            RegionSpec::Ram { .. } => REGION_KIND_RAM,
            RegionSpec::Rom { .. } => REGION_KIND_ROM,
            RegionSpec::Peripheral { .. } => REGION_KIND_PERIPHERAL,
        }
    }
    /// The peripheral device's id — `None` for a RAM / ROM region.
    pub fn device_id(&self) -> Option<&str> {
        match self {
            RegionSpec::Peripheral { device, .. } => Some(device.id()),
            RegionSpec::Ram { .. } | RegionSpec::Rom { .. } => None,
        }
    }
}
impl DeviceSpec {
    /// The device id — the key the restore path matches on.
    pub fn id(&self) -> &str {
        match self {
            DeviceSpec::Gpio { id, .. }
            | DeviceSpec::Uart16550 { id, .. }
            | DeviceSpec::Clint { id, .. }
            | DeviceSpec::Plic { id, .. }
            | DeviceSpec::TimerRiscv { id, .. } => id,
        }
    }
    /// The device kind on the wire (`"gpio"` / `"uart16550"` / `"clint"` /
    /// `"plic"`) — the value of the `device` internal tag.
    pub fn kind(&self) -> &'static str {
        match self {
            DeviceSpec::Gpio { .. } => "gpio",
            DeviceSpec::Uart16550 { .. } => "uart16550",
            DeviceSpec::Clint { .. } => "clint",
            DeviceSpec::Plic { .. } => "plic",
            DeviceSpec::TimerRiscv { .. } => "timer_riscv",
        }
    }
    /// The device's **numeric** parameters and their legal value domain.
    ///
    /// The tuple is `(field name, value, lower bound, upper bound)`; the
    /// upper bound is `None` when the architecture gives **no** upper bound
    /// — that is the case for `uart16550.baud_rate` (the RISC-V
    /// architecture does not cap the baud rate ⇒ **no number is
    /// invented** for it).
    ///
    /// ⚠️ `gpio` has **no** numeric parameter ⇒ it yields an empty list ⇒
    /// **no** new rule applies to it.
    ///
    /// ⚠️ The bounds live in **this** module ([`MAX_CLINT_FREQ_HZ`] and
    /// friends) ⇒ one definition, everybody else references them.
    ///
    /// ⚠️ **The bounds are looked up in [`Self::parameter_domain`]** (the
    /// **one** landing spot) ⇒ ⇒ the judge (V-13 / V-14) and the kernel's
    /// device type registry (STAGE-43 · 裁决 E) read the **same** place ⇒
    /// ⇒ the two can never drift.
    fn numeric_parameters(&self) -> Vec<(&'static str, u64, u64, Option<u64>)> {
        let domain = Self::parameter_domain(self.kind());
        self.numeric_values()
            .into_iter()
            .filter_map(|(name, value)| {
                domain
                    .iter()
                    .find(|(n, _, _)| *n == name)
                    .map(|(_, lower, upper)| (name, value, *lower, *upper))
            })
            .collect()
    }
    /// The **numeric values** of this instance's parameters — the value
    /// half of [`Self::numeric_parameters`].
    ///
    /// ⚠️ **`irq_line` is deliberately absent**: it is judged by the
    /// **cross-field** rule V-15 (it must be a valid PLIC source id) ⇒ ⇒
    /// it has **no** static domain ⇒ ⇒ it is not part of the domain table.
    fn numeric_values(&self) -> Vec<(&'static str, u64)> {
        match self {
            DeviceSpec::Gpio { .. } => Vec::new(),
            DeviceSpec::Uart16550 { baud_rate, .. } => {
                vec![("baud_rate", u64::from(*baud_rate))]
            }
            DeviceSpec::Clint { freq_hz, harts, .. } => {
                vec![("freq_hz", *freq_hz), ("harts", u64::from(*harts))]
            }
            DeviceSpec::Plic {
                sources, contexts, ..
            } => vec![
                ("sources", u64::from(*sources)),
                ("contexts", u64::from(*contexts)),
            ],
            // ⚠️ **这台器件没有数值参数**（只有 `id` + `irq_line`，而 `irq_line`
            //   归**跨字段**规则 V-15 管）⇒ ⇒ 空列表。
            DeviceSpec::TimerRiscv { .. } => Vec::new(),
        }
    }
    /// The **legal value domain** of one device kind — `(name, lower,
    /// upper)` per numeric parameter.
    ///
    /// ⚠️ **This is the one landing spot of the bounds** (STAGE-43 · v0.66 ·
    /// 裁决 E) ⇒ ⇒ **both** readers come here:
    /// - the judge (V-13 / V-14) — via [`Self::numeric_parameters`];
    /// - the kernel's **device type registry** (M-15) — which reports the
    ///   domain to the front end ⇒ ⇒ the front end **never** writes a
    ///   second interval table.
    ///
    /// ⚠️ **An unknown kind yields an empty domain** — that is the honest
    /// answer for a type the description format cannot express.
    ///
    /// ⚠️ **v0.73（STAGE-49 · G）**：`timer_riscv` **不再**是这种类型 ⇒ ⇒
    ///   描述**能写出它**了 ⇒ ⇒ 它**自己**的域是**空**（它没有数值参数）⇒ ⇒
    ///   下面**显式**列出它（不靠「未知类型 ⇒ 空」那条兜底）⇒ ⇒ ⇒ **域的
    ///   归属**看得见。
    ///
    /// @param {string} kind — the device kind tag (e.g. `"uart16550"`)
    /// @returns {Array<[string, number, number|null]>}
    pub fn parameter_domain(kind: &str) -> Vec<(&'static str, u64, Option<u64>)> {
        match kind {
            "gpio" => Vec::new(),
            "uart16550" => vec![("baud_rate", 1, None)],
            "clint" => vec![
                ("freq_hz", 1, Some(MAX_CLINT_FREQ_HZ)),
                ("harts", 1, Some(u64::from(MAX_CLINT_HARTS))),
            ],
            "plic" => vec![
                ("sources", 1, Some(u64::from(MAX_PLIC_SOURCES))),
                ("contexts", 1, Some(u64::from(MAX_PLIC_CONTEXTS))),
            ],
            // ⚠️ **`timer_riscv` 没有数值参数**（`irq_line` 归 V-15）⇒ ⇒ 空。
            "timer_riscv" => Vec::new(),
            _ => Vec::new(),
        }
    }
}
/// What the description **knows** about one region's extent.
///
/// ⚠️ A peripheral window declares only its **base**: its size is the
/// device's register map (裁决 D), so at this layer it is unknown.
#[derive(Debug, Clone, Copy)]
enum DeclaredExtent {
    /// A RAM / ROM region — a **known** half-open interval `[lo, hi)`.
    Interval { lo: u64, hi: u64 },
    /// A peripheral window — only the base address is declared.
    BaseOnly { addr: u64 },
}
/// Do two declared regions overlap?
///
/// ⚠️ The peripheral case is deliberately conservative: the window's
/// size is unknown here, but it is **never empty** (every device's
/// register map occupies at least one byte) ⇒ a peripheral whose base
/// falls inside a declared interval **does** overlap it, and two
/// peripherals sharing a base overlap each other. These are exactly the
/// overlaps M-14 will report at mapping time — the description layer
/// catches them earlier and with its own code.
///
/// ⚠️ **v0.73（STAGE-49 · B2）**：**三态**返回 —— `None` = **判不出来**
/// ⇒ ⇒ 那是因为**有一段还没有基址**（⇒ ⇒ 由装配期分配器负责 ⇒ ⇒ ⇒
/// 分配器**自己**判重叠 ⇒ ⇒ 仍报既有码 `E_PLT-06`）⇒ ⇒ ⇒ **不静默**
/// 把「判不出来」当成「不重叠」（那会让一块真的重叠的板**装上去**）。
fn regions_overlap(a: &RegionSpec, b: &RegionSpec) -> Option<bool> {
    // ⚠️ **先取基址**（`Option<u32>`）⇒ ⇒ 有一段没定 ⇒ ⇒ `?` 早退 ⇒ ⇒
    //   整个函数返回 `None`（= **判不出来**）⇒ ⇒ ⇒ **不**把它当成「不重叠」。
    let a_base = a.base()?;
    let b_base = b.base()?;
    let a = match a {
        RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => DeclaredExtent::Interval {
            lo: u64::from(a_base),
            hi: u64::from(a_base) + u64::from(*size),
        },
        RegionSpec::Peripheral { .. } => DeclaredExtent::BaseOnly {
            addr: u64::from(a_base),
        },
    };
    let b = match b {
        RegionSpec::Ram { size, .. } | RegionSpec::Rom { size, .. } => DeclaredExtent::Interval {
            lo: u64::from(b_base),
            hi: u64::from(b_base) + u64::from(*size),
        },
        RegionSpec::Peripheral { .. } => DeclaredExtent::BaseOnly {
            addr: u64::from(b_base),
        },
    };
    Some(match (a, b) {
        (
            DeclaredExtent::Interval { lo: a_lo, hi: a_hi },
            DeclaredExtent::Interval { lo: b_lo, hi: b_hi },
        ) => a_lo < b_hi && b_lo < a_hi,
        (DeclaredExtent::Interval { lo, hi }, DeclaredExtent::BaseOnly { addr }) => {
            lo <= addr && addr < hi
        }
        (DeclaredExtent::BaseOnly { addr }, DeclaredExtent::Interval { lo, hi }) => {
            lo <= addr && addr < hi
        }
        (DeclaredExtent::BaseOnly { addr: a_addr }, DeclaredExtent::BaseOnly { addr: b_addr }) => {
            a_addr == b_addr
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    /// The `qemu-virt` built-in description, as the frozen spec §1.2 /
    /// §1.4 / §1.5 spells it.
    ///
    /// ⚠️ The addresses and sizes here are **architecture literals**, not
    /// values read back from the implementation ⇒ the test cannot pass by
    /// construction.
    fn sample() -> PlatformDescription {
        PlatformDescription {
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "qemu-virt".to_string(),
            name: "QEMU virt".to_string(),
            priority: 20,
            region_budget_bytes: 134_217_728,
            max_region_bytes: None,
            regions: vec![
                RegionSpec::Ram {
                    base: Some(0),
                    size: 1_048_576,
                },
                RegionSpec::Ram {
                    base: Some(0x8000_0000),
                    size: 134_217_728,
                },
                RegionSpec::Peripheral {
                    base: Some(0x4000_0000),
                    device: DeviceSpec::Gpio {
                        irq_line: 11,
                        id: "GPIO0".to_string(),
                    },
                },
                RegionSpec::Peripheral {
                    base: Some(0x1000_0000),
                    device: DeviceSpec::Uart16550 {
                        id: "UART0".to_string(),
                        irq_line: 1,
                        baud_rate: 115_200,
                    },
                },
                RegionSpec::Peripheral {
                    base: Some(0x0200_0000),
                    device: DeviceSpec::Clint {
                        id: "CLINT".to_string(),
                        freq_hz: 10_000_000,
                        harts: 1,
                    },
                },
                RegionSpec::Peripheral {
                    base: Some(0x0C00_0000),
                    device: DeviceSpec::Plic {
                        id: "PLIC".to_string(),
                        sources: 64,
                        contexts: 16,
                    },
                },
            ],
            description: None,
            roles: None,
            mcu: None,
        }
    }
    fn keys(value: &serde_json::Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("必须是对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }
    // -----------------------------------------------------------------
    // A-1 — the format version
    // -----------------------------------------------------------------
    /// **A-1** — the description's format version is the **current** one and
    /// this build **accepts** it.
    ///
    /// ⚠️ **v0.74（STAGE-50 · 施工期订正）**：这一条**原来**钉的是**字面量**
    ///   （先是 1、后是 2）⇒ ⇒ ⇒ **每一次合法升版都会把它打死**（v0.73 批
    ///   实测踩过：STAGE-48 的门禁被本批**合法**升版打死）⇒ ⇒ ⇒ **改成
    ///   不随批次失效的性质**：
    ///   - **当前版本必须被「本构建能读的版本列表」收着** ⇒ ⇒ 「升了版却
    ///     忘了把它加进可读列表」这个缺陷**照样红**（⇒ ⇒ **判别力不丢**）；
    ///   - **老版本仍在列表里** ⇒ ⇒ **缺省优先**（存量板档案零风险）。
    ///
    /// ⚠️ **具体数值**（当前是 3）归**最新**那一批的门禁守
    ///   （`tools/stage50_check.py`）⇒ ⇒ ⇒ **一个版本号只有一处钉它的地方**。
    #[test]
    fn a1_the_platform_schema_version_is_supported_by_this_build() {
        assert!(
            SUPPORTED_PLATFORM_SCHEMA_VERSIONS.contains(&PLATFORM_SCHEMA_VERSION),
            "当前描述格式版本必须被「本构建能读的版本列表」收着（升了版却忘了加进列表 ⇒ ⇒ 老描述会被拒）"
        );
        for old in [1u32, 2] {
            assert!(
                SUPPORTED_PLATFORM_SCHEMA_VERSIONS.contains(&old),
                "版本 {old} 必须仍被接受（缺省优先：存量板档案零风险）"
            );
        }
    }
    /// **A-1 / V-1** — a description whose `schema_version` is not the one
    /// this build speaks is rejected with `E_PLT-01`.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **not** on the
    /// message text.
    ///
    /// ⚠️ **v0.73（STAGE-49 · B2）**：V-1 现在**接受版本 1 与版本 2**
    /// （⇒ ⇒ **缺省优先**：老描述仍能装）⇒ ⇒ ⇒ 「版本不对」的例子改成
    /// **不支持**的版本（⇒ ⇒ 0 与 99）⇒ ⇒ ⇒ **判别力保留**（仍断「不静默
    /// 硬解未知格式」）。
    #[test]
    fn v1_a_wrong_schema_version_is_rejected() {
        let mut desc = sample();
        desc.schema_version = 99;
        let err = desc.validate().expect_err("版本不对必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_01_SCHEMA_VERSION);
        assert!(err.to_string().starts_with("E_PLT-01: "));
    }
    /// **A-1 / V-1（v0.73 · STAGE-49 · B2）** —— 老描述（`schema_version = 1`）
    /// ⇒ ⇒ **仍然合法**（⇒ ⇒ **缺省优先** ⇒ ⇒ 存量板档案**零风险**）。
    #[test]
    fn v1_an_old_schema_version_is_still_accepted() {
        let mut desc = sample();
        desc.schema_version = 1;
        assert!(
            desc.validate().is_ok(),
            "老描述（schema_version = 1）必须仍然合法（缺省优先）：{:?}",
            desc.validate()
        );
    }
    // -----------------------------------------------------------------
    // A-2 — the key sets are pinned
    // -----------------------------------------------------------------
    /// **A-2** — the description's **top-level key set** is pinned.
    ///
    /// ⚠️ `description` is **absent** when it is `None` (it carries
    /// `skip_serializing_if`) ⇒ the key set is the seven mandatory keys.
    ///
    /// ⚠️ **v0.76（STAGE-52 · B3 决议）**：`auto_selectable` **不在其中**
    /// （字段已废弃 · 线上不再有该键 ⇒ ⇒ 老描述带键靠 serde 忽略未知键兜底）。
    ///
    /// ⚠️ **v0.31**: `max_region_bytes` **is** one of them too ⇒ it is
    /// **always** written (the value may be `null`) ⇒ the wire shape is
    /// **unique** ⇒ the contract layer and the front-end mirror can be
    /// compared field by field.
    #[test]
    fn a2_the_top_level_key_set_is_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        assert_eq!(
            keys(&json),
            vec![
                "id",
                "max_region_bytes",
                "name",
                "priority",
                "region_budget_bytes",
                "regions",
                "schema_version",
            ],
            "平台描述的顶层键集必须精确固定（七个）"
        );

        // With a note attached, the optional key **appears** — and still
        // nothing else does.
        let mut with_note = sample();
        with_note.description = Some("人读说明".to_string());
        let json = serde_json::to_value(with_note).expect("json");
        assert_eq!(
            keys(&json),
            vec![
                "description",
                "id",
                "max_region_bytes",
                "name",
                "priority",
                "region_budget_bytes",
                "regions",
                "schema_version",
            ],
            "带说明的描述只多出 description 一个键"
        );
    }
    /// **A-2 (续)** — a description that **omits** `auto_selectable` parses
    /// with the default `true`.
    /// **A-2（v0.76 · STAGE-52 · B3 决议）** — `auto_selectable` 字段**废弃**
    /// ⇒ ⇒ 老描述**带这个键仍能解析**（serde 忽略未知字段 ⇒ ⇒ 存量板档案
    /// **零风险**）。
    ///
    /// ⚠️ **The key is silently ignored** — 契约层 `PlatformDescription` 不再
    /// 有该字段 ⇒ ⇒ 解析成功 + 描述合法 = 判据（不需要再断言字段值）。
    #[test]
    fn a2_legacy_auto_selectable_key_is_ignored() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,

            "region_budget_bytes": 4096,
            "regions": [ { "kind": "ram", "base": 0, "size": 1024 } ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        desc.validate().expect("带废弃键的老描述仍必须合法");
        assert_eq!(desc.id, "x");
    }
    /// **A-2** — the **region** key sets are pinned, one per `kind`.
    ///
    /// ⚠️ A peripheral region carries **no** `size`: the window size is
    /// the device's register map (裁决 D) ⇒ a `size` key here would be a
    /// second source of truth.
    #[test]
    fn a2_the_region_key_sets_are_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        assert_eq!(regions.len(), 6);

        let ram = keys(&regions[0]);
        assert_eq!(ram, vec!["base", "kind", "size"], "RAM 条目的键集");

        let peripheral = keys(&regions[2]);
        assert_eq!(
            peripheral,
            vec!["base", "device", "kind"],
            "外设条目的键集（没有 size）"
        );
    }
    /// **A-2** — the **device** key sets are pinned, one per device kind.
    #[test]
    fn a2_the_device_key_sets_are_pinned() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");

        let device =
            |index: usize| -> &serde_json::Value { regions[index].get("device").expect("device") };

        assert_eq!(
            keys(device(2)),
            vec!["device", "id", "irq_line"],
            "GPIO 的键集（整块一个中断号）"
        );
        assert_eq!(
            keys(device(3)),
            vec!["baud_rate", "device", "id", "irq_line"],
            "UART 的键集"
        );
        assert_eq!(
            keys(device(4)),
            vec!["device", "freq_hz", "harts", "id"],
            "CLINT 的键集"
        );
        assert_eq!(
            keys(device(5)),
            vec!["contexts", "device", "id", "sources"],
            "PLIC 的键集"
        );
    }
    // -----------------------------------------------------------------
    // A-3 — one rule, one scenario code
    // -----------------------------------------------------------------
    /// **A-3 / V-2** — an empty platform id is rejected with `E_PLT-02`.
    #[test]
    fn v2_an_empty_id_is_rejected() {
        let mut desc = sample();
        desc.id = String::new();
        let err = desc.validate().expect_err("空 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_02_EMPTY_ID);
    }
    /// **A-3 / V-3** — a description with no region at all is rejected
    /// with `E_PLT-03`.
    #[test]
    fn v3_a_description_without_regions_is_rejected() {
        let mut desc = sample();
        desc.regions.clear();
        let err = desc.validate().expect_err("没有区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_03_NO_REGIONS);
    }
    /// **A-3 / V-4** — a RAM region of size zero is rejected with
    /// `E_PLT-04`.
    ///
    /// ⚠️ Same meaning as M-14's `E_BUS-06`, **different layer** ⇒
    /// different code.
    #[test]
    fn v4_a_zero_sized_region_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![RegionSpec::Ram {
            base: Some(0x1000),
            size: 0,
        }];
        let err = desc.validate().expect_err("大小为 0 的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_04_ZERO_SIZE);
    }
    /// **A-3 / V-5** — a region whose `base + size` leaves the 32-bit
    /// address space is rejected with `E_PLT-05`.
    #[test]
    fn v5_a_region_overflowing_u32_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![RegionSpec::Ram {
            base: Some(0xFFFF_F000),
            size: 0x2000,
        }];
        let err = desc.validate().expect_err("越界的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_05_OVERFLOW);
    }
    /// **A-3 / V-6** — two overlapping regions are rejected with
    /// `E_PLT-06`.
    ///
    /// ⚠️ The regions are listed **out of order** (the later one starts
    /// lower) ⇒ the check must sort by `base` first, otherwise it would
    /// miss the overlap.
    #[test]
    fn v6_two_overlapping_regions_are_rejected() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Ram {
                base: Some(0x2000),
                size: 0x1000,
            },
            RegionSpec::Ram {
                base: Some(0x1000),
                size: 0x2000,
            },
        ];
        let err = desc.validate().expect_err("重叠的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_06_OVERLAP);
    }
    /// **A-3 / V-6** — a peripheral window whose base falls inside a
    /// declared RAM interval overlaps it.
    ///
    /// ⚠️ The window's **size** is unknown at this layer (it is the
    /// device's register map) ⇒ the judgement is "the base lies inside
    /// the interval", which is the conservative reading and agrees with
    /// what M-14 reports when the map is actually built.
    #[test]
    fn v6_a_peripheral_inside_a_ram_region_overlaps_it() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Ram {
                base: Some(0x1000),
                size: 0x1000,
            },
            RegionSpec::Peripheral {
                base: Some(0x1800),
                device: DeviceSpec::Gpio {
                    irq_line: 11,
                    id: "GPIO0".to_string(),
                },
            },
        ];
        let err = desc.validate().expect_err("落在 RAM 区间内的外设必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_06_OVERLAP);
    }
    /// **A-3 / V-7** — a RAM region whose **declared** size exceeds the
    /// description's budget is rejected with `E_PLT-07`.
    ///
    /// ⚠️ **Not** M-14's `E_BUS-09`: that one judges the **physical
    /// occupancy** at runtime, this one judges the **declaration**.
    #[test]
    fn v7_a_region_over_the_declared_budget_is_rejected() {
        let mut desc = sample();
        desc.region_budget_bytes = 1024;
        desc.regions = vec![RegionSpec::Ram {
            base: Some(0x1000),
            size: 2048,
        }];
        let err = desc.validate().expect_err("超预算的区域必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_07_OVER_BUDGET);
    }
    /// **A-3 / V-7** — a region whose size is **exactly** the budget is
    /// allowed (`>` and not `>=`).
    ///
    /// ⚠️ The built-in `qemu-virt` description declares a 128 MiB RAM
    /// against a 128 MiB budget ⇒ `>=` would reject a valid platform.
    #[test]
    fn v7_a_region_exactly_at_the_budget_is_allowed() {
        let mut desc = sample();
        desc.region_budget_bytes = 134_217_728;
        desc.regions = vec![RegionSpec::Ram {
            base: Some(0x8000_0000),
            size: 134_217_728,
        }];
        assert!(desc.validate().is_ok(), "恰好等于预算的区域必须被接受");
    }
    /// **A-3 / V-8** — an unknown device kind fails during
    /// **deserialization**, not during validation.
    ///
    /// ⚠️ This is why the family has **eight** scenario codes and not
    /// nine: the shape rule lives in the serde layer.
    #[test]
    fn v8_an_unknown_device_kind_fails_in_the_serde_layer() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 1024,
            "regions": [ { "kind": "peripheral", "base": 0, "device": { "device": "nvme", "id": "X" } } ]
        }"#;
        let err =
            serde_json::from_str::<PlatformDescription>(raw).expect_err("未知器件种类必须解析失败");
        let text = err.to_string();
        assert!(
            text.contains("nvme"),
            "serde 的报错必须点名那个未知的器件种类，实际：{text}"
        );
    }
    /// **A-3 / V-8** — a device with a **missing** parameter fails in the
    /// serde layer too.
    #[test]
    fn v8_a_missing_device_parameter_fails_in_the_serde_layer() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 1024,
            "regions": [ { "kind": "peripheral", "base": 0, "device": { "device": "uart16550", "id": "U0" } } ]
        }"#;
        assert!(
            serde_json::from_str::<PlatformDescription>(raw).is_err(),
            "缺参数的器件条目必须解析失败"
        );
    }
    /// **A-3 / V-9** — two peripheral devices sharing an id are rejected
    /// with `E_PLT-08`.
    ///
    /// ⚠️ The two devices are of **different kinds** (a GPIO and a UART)
    /// ⇒ the check spans kinds, exactly like the session payload's own
    /// duplicate-id rule.
    #[test]
    fn v9_a_duplicated_device_id_is_rejected() {
        let mut desc = sample();
        desc.regions = vec![
            RegionSpec::Peripheral {
                base: Some(0x1000),
                device: DeviceSpec::Gpio {
                    irq_line: 11,
                    id: "DUP".to_string(),
                },
            },
            RegionSpec::Peripheral {
                base: Some(0x2000),
                device: DeviceSpec::Uart16550 {
                    id: "DUP".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                },
            },
        ];
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID);
    }
    /// **A-3** — a well-formed description passes validation.
    ///
    /// ⚠️ Without this, "everything is rejected" would satisfy every
    /// rejection case above.
    #[test]
    fn a_valid_description_passes_validation() {
        assert!(sample().validate().is_ok());
    }
    // -----------------------------------------------------------------
    // A-4 — the round trip
    // -----------------------------------------------------------------
    /// **A-4** — the description survives a JSON round trip field by
    /// field.
    #[test]
    fn the_description_survives_a_round_trip() {
        let original = sample();
        let text = serde_json::to_string(&original).expect("serialize");
        let back: PlatformDescription = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, original);
        assert_eq!(back.id, original.id);
        assert_eq!(back.name, original.name);
        assert_eq!(back.priority, original.priority);
        assert_eq!(back.region_budget_bytes, original.region_budget_bytes);
        assert_eq!(back.regions, original.regions);
        assert_eq!(back.description, original.description);
    }
    /// **A-4** — the maximum budget round-trips; **one more** is rejected
    /// in the **serde** layer.
    ///
    /// ⚠️ The budget is a `u32` (not a `u64`): it is a **per-region**
    /// byte budget and the address space is 4 GiB ⇒ a budget beyond
    /// `u32::MAX` is meaningless. Out-of-range values therefore fail
    /// during deserialization — they never reach `validate()`.
    #[test]
    fn the_maximum_budget_round_trips_and_one_more_is_rejected_by_serde() {
        let mut desc = sample();
        desc.region_budget_bytes = u32::MAX;
        let text = serde_json::to_string(&desc).expect("serialize");
        let back: PlatformDescription = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back.region_budget_bytes, 4_294_967_295);

        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 4294967296,
            "regions": [ { "kind": "ram", "base": 0, "size": 1024 } ]
        }"#;
        assert!(
            serde_json::from_str::<PlatformDescription>(raw).is_err(),
            "超过 u32::MAX 的预算必须在 serde 阶段被拒"
        );
    }
    // -----------------------------------------------------------------
    // A-5 — the internal tag keys
    // -----------------------------------------------------------------
    /// **A-5** — the internal tags are literally `kind` (regions) and
    /// `device` (devices) — **not** `type`, **not** `t`.
    #[test]
    fn the_internal_tag_keys_are_kind_and_device() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");

        let ram = regions[0].as_object().expect("ram");
        assert_eq!(ram.get("kind").and_then(|v| v.as_str()), Some("ram"));
        assert!(ram.get("type").is_none(), "区域条目不许有 type 键");
        assert!(ram.get("t").is_none(), "区域条目不许有 t 键");

        let peripheral = regions[3].as_object().expect("peripheral");
        assert_eq!(
            peripheral.get("kind").and_then(|v| v.as_str()),
            Some("peripheral")
        );
        let device = peripheral
            .get("device")
            .and_then(|v| v.as_object())
            .expect("device");
        assert_eq!(
            device.get("device").and_then(|v| v.as_str()),
            Some("uart16550")
        );
        assert!(device.get("type").is_none(), "器件条目不许有 type 键");
        assert!(device.get("kind").is_none(), "器件条目不许有 kind 键");
    }
    /// **A-5** — the description layer and the snapshot topology speak
    /// **one** region-kind vocabulary.
    ///
    /// ⚠️ The three constants live in the contract layer (`session`) and
    /// the kernel's `m14_bus` re-exports them ⇒ the description's `kind`
    /// values are literally those strings ⇒ a description and an exported
    /// memory map need **no** translation.
    #[test]
    fn the_region_kinds_are_the_snapshot_topology_vocabulary() {
        assert_eq!(REGION_KIND_RAM, "ram");
        assert_eq!(REGION_KIND_ROM, "rom");
        assert_eq!(REGION_KIND_PERIPHERAL, "peripheral");

        assert_eq!(
            RegionSpec::Ram {
                base: Some(0),
                size: 1
            }
            .kind(),
            "ram"
        );
        assert_eq!(
            RegionSpec::Rom {
                base: Some(0),
                size: 1
            }
            .kind(),
            "rom"
        );
        assert_eq!(
            RegionSpec::Peripheral {
                base: Some(0),
                device: DeviceSpec::Gpio {
                    irq_line: 11,
                    id: "G".to_string()
                }
            }
            .kind(),
            "peripheral"
        );

        // …and the **wire** form agrees with the vocabulary.
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        assert_eq!(regions[0].get("kind").and_then(|v| v.as_str()), Some("ram"));
        assert_eq!(
            regions[2].get("kind").and_then(|v| v.as_str()),
            Some("peripheral")
        );
    }
    /// **A-5** — the device kinds on the wire agree with
    /// [`DeviceSpec::kind`].
    #[test]
    fn the_device_kinds_on_the_wire_agree_with_the_helper() {
        let json = serde_json::to_value(sample()).expect("json");
        let regions = json
            .get("regions")
            .and_then(|v| v.as_array())
            .expect("regions");
        let wire = |index: usize| -> &str {
            regions[index]
                .get("device")
                .and_then(|v| v.get("device"))
                .and_then(|v| v.as_str())
                .expect("device tag")
        };
        assert_eq!(
            wire(2),
            DeviceSpec::Gpio {
                id: "G".into(),
                irq_line: 11
            }
            .kind()
        );
        assert_eq!(
            wire(3),
            DeviceSpec::Uart16550 {
                id: "U".into(),
                irq_line: 1,
                baud_rate: 115_200
            }
            .kind()
        );
        assert_eq!(
            wire(4),
            DeviceSpec::Clint {
                id: "C".into(),
                freq_hz: 10_000_000,
                harts: 1
            }
            .kind()
        );
        assert_eq!(
            wire(5),
            DeviceSpec::Plic {
                id: "P".into(),
                sources: 64,
                contexts: 16
            }
            .kind()
        );
    }
    // -----------------------------------------------------------------
    // The frozen example
    // -----------------------------------------------------------------
    /// The frozen spec's example (§1.2 / §1.4 / §1.5) parses into the
    /// expected value, field by field.
    #[test]
    fn the_frozen_example_description_parses() {
        let raw = r#"{
            "schema_version": 1,
            "id": "qemu-virt",
            "name": "QEMU virt",
            "priority": 20,

            "region_budget_bytes": 134217728,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "rom", "base": 4096, "size": 65536 },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert_eq!(desc.schema_version, 1);
        assert_eq!(desc.id, "qemu-virt");
        assert_eq!(desc.name, "QEMU virt");
        assert_eq!(desc.priority, 20);
        assert_eq!(desc.region_budget_bytes, 134_217_728);
        assert_eq!(desc.description, None);
        assert_eq!(desc.regions.len(), 3);
        assert_eq!(
            desc.regions[0],
            RegionSpec::Ram {
                base: Some(0),
                size: 1_048_576
            }
        );
        assert_eq!(
            desc.regions[1],
            RegionSpec::Rom {
                base: Some(4096),
                size: 65_536
            }
        );
        assert_eq!(
            desc.regions[2],
            RegionSpec::Peripheral {
                base: Some(1_073_741_824),
                device: DeviceSpec::Uart16550 {
                    id: "UART0".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                }
            }
        );
    }
    // -----------------------------------------------------------------
    // A-6 / A-8 — the error family and the untouched contract version
    // -----------------------------------------------------------------
    /// **A-6** — the platform error family is registered with its
    /// **three-character** wire tag.
    #[test]
    fn a6_the_platform_family_is_registered() {
        assert_eq!(crate::error::ErrorFamily::Platform.as_str(), "PLT");
        assert_eq!(PLATFORM_FAMILY, crate::error::ErrorFamily::Platform);
        assert!(
            crate::error::ALL_FAMILIES.contains(&crate::error::ErrorFamily::Platform),
            "Platform 必须出现在族清单里"
        );
    }
    /// **A-6** — the scenario codes render as the frozen spec's codes,
    /// digit for digit.
    ///
    /// ⚠️ The three **host-side** codes (V-10 .. V-12) are listed here
    /// too: they are owned by the contract layer even though the host is
    /// the one that detects them ⇒ a code has exactly one landing spot.
    #[test]
    fn a6_the_platform_scenario_codes_render_as_the_spec_spells_them() {
        use crate::error::{
            E_PLT_09_UNKNOWN_PLATFORM, E_PLT_10_PLATFORM_FILE_UNREADABLE,
            E_PLT_11_PLATFORM_FILE_MALFORMED,
        };
        let cases = [
            (E_PLT_01_SCHEMA_VERSION, "E_PLT-01"),
            (E_PLT_02_EMPTY_ID, "E_PLT-02"),
            (E_PLT_03_NO_REGIONS, "E_PLT-03"),
            (E_PLT_04_ZERO_SIZE, "E_PLT-04"),
            (E_PLT_05_OVERFLOW, "E_PLT-05"),
            (E_PLT_06_OVERLAP, "E_PLT-06"),
            (E_PLT_07_OVER_BUDGET, "E_PLT-07"),
            (E_PLT_08_DUPLICATE_DEVICE_ID, "E_PLT-08"),
            (E_PLT_09_UNKNOWN_PLATFORM, "E_PLT-09"),
            (E_PLT_10_PLATFORM_FILE_UNREADABLE, "E_PLT-10"),
            (E_PLT_11_PLATFORM_FILE_MALFORMED, "E_PLT-11"),
            (E_PLT_12_PARAMETER_OUT_OF_RANGE, "E_PLT-12"),
            (E_PLT_13_REVISION_CONFLICT, "E_PLT-13"),
            (E_PLT_14_REGION_NOT_FOUND, "E_PLT-14"),
        ];
        for (scenario, expected) in cases {
            let err = PlatformError::new(PLATFORM_FAMILY, scenario, "x");
            assert_eq!(
                err.to_string(),
                format!("{expected}: x"),
                "场景码 {scenario} 渲染成了 {err}"
            );
        }
    }
    /// **A-8** — the contract layer's **session payload** version moved
    /// to 6 when PLIC session state gained gateway levels and active stacks.
    #[test]
    fn a8_the_session_payload_version_is_six() {
        assert_eq!(crate::session::SESSION_SNAPSHOT_SCHEMA_VERSION, 6);
    }
    // -----------------------------------------------------------------
    // STAGE-15 · v0.30 · D-141 — the devices' legal value domain
    // (V-13 ~ V-15 · `E_PLT-12`)
    //
    // ⚠️ The **A-n** labels below are the frozen spec STAGE-15 §5.1's
    // acceptance items (A-1 ~ A-9) — **not** the older "A-n" labels the
    // STAGE-12 cases above carry ⇒ every label here says "STAGE-15".
    // -----------------------------------------------------------------
    /// A description made of the given **peripheral devices** only.
    ///
    /// ⚠️ Each device gets its own window base ⇒ the map has **no**
    /// overlap (V-6) ⇒ the cases below are about the **device
    /// parameters**, not about the regions.
    fn board_with(devices: Vec<DeviceSpec>) -> PlatformDescription {
        PlatformDescription {
            schema_version: PLATFORM_SCHEMA_VERSION,
            id: "board-under-test".to_string(),
            name: "Board under test".to_string(),
            priority: 1,
            region_budget_bytes: 134_217_728,
            max_region_bytes: None,
            regions: devices
                .into_iter()
                .enumerate()
                .map(|(index, device)| RegionSpec::Peripheral {
                    base: Some(0x1000_0000u32 + (index as u32) * 0x1_0000),
                    device,
                })
                .collect(),
            description: None,
            roles: None,
            mcu: None,
        }
    }
    /// Assert that a description is rejected with the platform family's
    /// `E_PLT-12` scenario code.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **never** on the
    /// message text ⇒ rewording the message must not turn any case red.
    fn assert_parameter_out_of_range(desc: &PlatformDescription, what: &str) {
        let err = desc.validate().expect_err(what);
        assert_eq!(err.family, PLATFORM_FAMILY, "{what} 的族不是 Platform");
        assert_eq!(
            err.scenario, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "{what} 的场景码不是 E_PLT-12（实际 {err}）"
        );
    }
    // -------------------------------------------------------------
    // STAGE-15 A-1 — V-13: the lower bounds
    // -------------------------------------------------------------
    /// **STAGE-15 A-1 / V-13** — a UART whose `baud_rate` is **zero** is
    /// rejected with `E_PLT-12`.
    ///
    /// ⚠️ The map has **no PLIC** ⇒ V-15 does not apply ⇒ this case is
    /// about the **single field** alone.
    #[test]
    fn v13_a_zero_baud_rate_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 0,
        }]);
        assert_parameter_out_of_range(&desc, "波特率为 0 的 UART");
    }
    /// **STAGE-15 A-2 / V-13** — a CLINT whose `freq_hz` is **zero** is
    /// rejected with `E_PLT-12`.
    #[test]
    fn v13_a_zero_clint_frequency_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 0,
            harts: 1,
        }]);
        assert_parameter_out_of_range(&desc, "频率为 0 的 CLINT");
    }
    /// **STAGE-15 A-3 / V-13** — a CLINT with **zero harts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_clint_without_harts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 0,
        }]);
        assert_parameter_out_of_range(&desc, "hart 数为 0 的 CLINT");
    }
    /// **STAGE-15 A-3 / V-13** — a PLIC with **zero sources** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_plic_without_sources_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 0,
            contexts: 16,
        }]);
        assert_parameter_out_of_range(&desc, "中断源数为 0 的 PLIC");
    }
    /// **STAGE-15 A-3 / V-13** — a PLIC with **zero contexts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v13_a_plic_without_contexts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 0,
        }]);
        assert_parameter_out_of_range(&desc, "context 数为 0 的 PLIC");
    }
    // -------------------------------------------------------------
    // STAGE-15 A-4 — V-14: the upper bounds
    // -------------------------------------------------------------
    /// **STAGE-15 A-4 / V-14** — a CLINT whose `freq_hz` is **one above
    /// 1 THz** is rejected with `E_PLT-12`.
    ///
    /// ⚠️ The bound is written as an **architecture literal**
    /// (`1_000_000_000_001`), not as `MAX_CLINT_FREQ_HZ + 1` ⇒ a wrong
    /// constant cannot make this case pass.
    #[test]
    fn v14_a_clint_frequency_above_one_terahertz_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1_000_000_000_001,
            harts: 1,
        }]);
        assert_parameter_out_of_range(&desc, "频率超过 1 THz 的 CLINT");
    }
    /// **STAGE-15 A-4 / V-14** — a CLINT with **4097 harts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_clint_with_more_than_4096_harts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 4097,
        }]);
        assert_parameter_out_of_range(&desc, "hart 数超过 4096 的 CLINT");
    }
    /// **STAGE-15 A-4 / V-14** — a PLIC with **1025 sources** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_plic_with_more_than_1024_sources_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1025,
            contexts: 16,
        }]);
        assert_parameter_out_of_range(&desc, "中断源数超过 1024 的 PLIC");
    }
    /// **STAGE-15 A-4 / V-14** — a PLIC with **17 contexts** is rejected
    /// with `E_PLT-12`.
    #[test]
    fn v14_a_plic_with_more_than_16_contexts_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 17,
        }]);
        assert_parameter_out_of_range(&desc, "context 数超过 16 的 PLIC");
    }
    // -------------------------------------------------------------
    // STAGE-15 A-5 — the boundary values are legal
    // -------------------------------------------------------------
    /// **STAGE-15 A-5 / V-13** — a baud rate of **1** is legal (the lower
    /// bound is inclusive).
    #[test]
    fn v13_the_lowest_legal_baud_rate_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 1,
        }]);
        assert!(desc.validate().is_ok(), "波特率 1 必须被接受");
    }
    /// **STAGE-15 A-5 / V-13** — a CLINT frequency of **1 Hz** is legal.
    #[test]
    fn v13_the_lowest_legal_clint_frequency_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "频率 1 Hz 必须被接受");
    }
    /// **STAGE-15 A-5 / V-13** — **one hart** is legal.
    #[test]
    fn v13_the_lowest_legal_clint_hart_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单 hart 的 CLINT 必须被接受");
    }
    /// **STAGE-15 A-5 / V-13** — **one source** is legal.
    #[test]
    fn v13_the_lowest_legal_plic_source_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1,
            contexts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单中断源的 PLIC 必须被接受");
    }
    /// **STAGE-15 A-5 / V-13** — **one context** is legal.
    #[test]
    fn v13_the_lowest_legal_plic_context_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 1,
        }]);
        assert!(desc.validate().is_ok(), "单 context 的 PLIC 必须被接受");
    }
    /// **STAGE-15 A-5 / V-14** — a CLINT frequency of exactly
    /// **1 THz** is legal (the upper bound is inclusive).
    #[test]
    fn v14_the_highest_legal_clint_frequency_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 1_000_000_000_000,
            harts: 1,
        }]);
        assert!(desc.validate().is_ok(), "1 THz 的 CLINT 必须被接受");
    }
    /// **STAGE-15 A-5 / V-14** — exactly **4096 harts** are legal.
    #[test]
    fn v14_the_highest_legal_clint_hart_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Clint {
            id: "CLINT0".to_string(),
            freq_hz: 10_000_000,
            harts: 4096,
        }]);
        assert!(desc.validate().is_ok(), "4096 个 hart 的 CLINT 必须被接受");
    }
    /// **STAGE-15 A-5 / V-14** — exactly **1024 sources** are legal.
    #[test]
    fn v14_the_highest_legal_plic_source_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 1024,
            contexts: 16,
        }]);
        assert!(desc.validate().is_ok(), "1024 个中断源的 PLIC 必须被接受");
    }
    /// **STAGE-15 A-5 / V-14** — exactly **16 contexts** are legal.
    #[test]
    fn v14_the_highest_legal_plic_context_count_is_accepted() {
        let desc = board_with(vec![DeviceSpec::Plic {
            id: "PLIC0".to_string(),
            sources: 64,
            contexts: 16,
        }]);
        assert!(desc.validate().is_ok(), "16 个 context 的 PLIC 必须被接受");
    }
    /// **STAGE-15 A-5 / V-14** — the baud rate has **no upper bound**.
    ///
    /// ⚠️ The RISC-V architecture does not cap the baud rate ⇒ the
    /// description layer must not invent one ⇒ the largest value the
    /// field can hold is still legal.
    #[test]
    fn v14_the_baud_rate_has_no_upper_bound() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: u32::MAX,
        }]);
        assert!(desc.validate().is_ok(), "波特率没有上界 ⇒ 不该被拒");
    }
    // -------------------------------------------------------------
    // STAGE-15 A-6 / A-7 — V-15: the cross-field rule
    // -------------------------------------------------------------
    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **20** is rejected with `E_PLT-12`.
    ///
    /// ⚠️ PLIC source ids are **0-based** ⇒ the legal ids of a 16-source
    /// PLIC are `0 ..= 15` ⇒ `20` is outside them.
    #[test]
    fn v15_an_irq_line_beyond_the_plic_source_count_is_rejected() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 20,
                baud_rate: 115_200,
            },
        ]);
        assert_parameter_out_of_range(&desc, "中断线超出 PLIC 源数的 UART");
    }
    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **15** (the highest legal id) is accepted.
    #[test]
    fn v15_the_highest_legal_irq_line_is_accepted() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 15,
                baud_rate: 115_200,
            },
        ]);
        assert!(desc.validate().is_ok(), "irq_line 15 必须被接受");
    }
    /// **STAGE-15 A-6 / V-15** — with a PLIC of **16 sources**, a UART
    /// whose `irq_line` is **16** — one past the highest legal id — is
    /// rejected with `E_PLT-12`.
    ///
    /// ⚠️ PLIC source ids are **0-based** ⇒ a 16-source PLIC's legal ids
    /// are `0 ..= 15` ⇒ `16` is the first **illegal** one. This is the
    /// case that separates `<` from `<=`.
    #[test]
    fn v15_an_irq_line_equal_to_the_plic_source_count_is_rejected() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 16,
                baud_rate: 115_200,
            },
        ]);
        assert_parameter_out_of_range(&desc, "中断线等于 PLIC 源数的 UART");
    }
    /// **STAGE-15 A-7 / V-15** — a map with **no PLIC** and a UART whose
    /// `irq_line` is **65535** is accepted.
    ///
    /// ⚠️ **No PLIC does not make the board illegal** (裁决 G): the board
    /// simply has no interrupt routing ⇒ `irq_line` is meaningless but
    /// harmless ⇒ refusing it would refuse a legal board.
    #[test]
    fn v15_an_irq_line_without_any_plic_is_not_judged() {
        let desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 65_535,
            baud_rate: 115_200,
        }]);
        assert!(
            desc.validate().is_ok(),
            "没有 PLIC 时 irq_line 不该被判非法"
        );
    }
    // -------------------------------------------------------------
    // STAGE-15 A-8 — the built-in descriptions stay legal
    // -------------------------------------------------------------
    /// The built-in `default` description — a **hand-written copy** of
    /// `kernel_host/platforms/default.json`.
    ///
    /// ⚠️ **The contract layer does not depend on the kernel crate** ⇒
    /// the equivalent copy lives **here** (the parameters are copied
    /// field by field) ⇒ the dependency direction stays one-way.
    ///
    /// ⚠️ **This is the only copy** ⇒ the STAGE-15 A-8 cases **and** the
    /// STAGE-18 A-3 case both read it ⇒ there is no second copy that
    /// could drift from this one unnoticed.
    ///
    /// ⚠️ **If a copy drifts from the real file**, the **kernel** side
    /// catches it (`kernel_host::tests_platform`'s
    /// `the_builtin_descriptions_are_well_formed` reads the **real**
    /// files) ⇒ the two layers have one job each: **this** crate judges
    /// the **rules**, the **host** judges the **files**.
    const BUILTIN_DEFAULT_RAW: &str = r#"{
            "schema_version": 1,
            "id": "default",
            "name": "Default",
            "priority": 10,
            "region_budget_bytes": 16777216,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0" } },
                { "kind": "ram", "base": 2147483648, "size": 4194304 }
            ]
        }"#;
    /// The built-in `minimal` description — a **hand-written copy** of
    /// `kernel_host/platforms/minimal.json`.
    ///
    /// ⚠️ See the note in [`BUILTIN_DEFAULT_RAW`] (the contract layer
    /// does not depend on the kernel crate; this is **the** copy).
    const BUILTIN_MINIMAL_RAW: &str = r#"{
            "schema_version": 1,
            "id": "minimal",
            "name": "Minimal (zero-change demo platform)",
            "priority": 30,
            "region_budget_bytes": 16777216,
            "regions": [
                { "kind": "ram", "base": 0, "size": 2097152 },
                { "kind": "ram", "base": 268435456, "size": 16777216 }
            ]
        }"#;
    /// The built-in `qemu-virt` description — a **hand-written copy** of
    /// `kernel_host/platforms/qemu-virt.json`.
    ///
    /// ⚠️ See the note in [`BUILTIN_DEFAULT_RAW`] (the contract layer
    /// does not depend on the kernel crate; this is **the** copy).
    const BUILTIN_QEMU_VIRT_RAW: &str = r#"{
            "schema_version": 1,
            "id": "qemu-virt",
            "name": "QEMU virt",
            "priority": 20,
            "region_budget_bytes": 134217728,
            "regions": [
                { "kind": "ram", "base": 0, "size": 1048576 },
                { "kind": "peripheral", "base": 33554432,
                  "device": { "device": "clint", "id": "CLINT", "freq_hz": 10000000, "harts": 1 } },
                { "kind": "peripheral", "base": 201326592,
                  "device": { "device": "plic", "id": "PLIC", "sources": 64, "contexts": 16 } },
                { "kind": "peripheral", "base": 268435456,
                  "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0" } },
                { "kind": "ram", "base": 2147483648, "size": 134217728 }
            ]
        }"#;
    /// The three built-in descriptions as **hand-written copies**, in the
    /// order the kernel crate ships them.
    ///
    /// ⚠️ **One copy, several readers** ⇒ the STAGE-15 A-8 cases and the
    /// STAGE-18 A-3 case all read **these** ⇒ the contract layer never
    /// reaches into the kernel crate's tree.
    fn builtin_copies() -> [(&'static str, &'static str); 3] {
        [
            ("default", BUILTIN_DEFAULT_RAW),
            ("minimal", BUILTIN_MINIMAL_RAW),
            ("qemu-virt", BUILTIN_QEMU_VIRT_RAW),
        ]
    }
    /// **STAGE-15 A-8** — the built-in `default` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ This is a **hand-written copy** of
    /// `kernel_host/platforms/default.json` — the contract layer does
    /// **not** depend on the kernel crate ⇒ the equivalent copy lives
    /// here (the parameters are copied field by field) ⇒ the copy itself
    /// lives in [`BUILTIN_DEFAULT_RAW`].
    #[test]
    fn a8_the_builtin_default_description_is_still_legal() {
        let desc: PlatformDescription =
            serde_json::from_str(BUILTIN_DEFAULT_RAW).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 default 描述必须仍然合法");
    }
    /// **STAGE-15 A-8** — the built-in `minimal` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ This is a **hand-written copy** of
    /// `kernel_host/platforms/minimal.json` — see the note in the
    /// `default` case above.
    #[test]
    fn a8_the_builtin_minimal_description_is_still_legal() {
        let desc: PlatformDescription =
            serde_json::from_str(BUILTIN_MINIMAL_RAW).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 minimal 描述必须仍然合法");
    }
    /// **STAGE-15 A-8** — the built-in `qemu-virt` description is still
    /// legal under the new rules.
    ///
    /// ⚠️ This is a **hand-written copy** of
    /// `kernel_host/platforms/qemu-virt.json` — see the note in the
    /// `default` case above.
    ///
    /// ⚠️ Its PLIC has **16 contexts** ⇒ it sits **exactly on** the upper
    /// bound ⇒ a `>=` comparison would reject the kernel's own board.
    #[test]
    fn a8_the_builtin_qemu_virt_description_is_still_legal() {
        let desc: PlatformDescription =
            serde_json::from_str(BUILTIN_QEMU_VIRT_RAW).expect("必须能解析");
        assert!(desc.validate().is_ok(), "内置 qemu-virt 描述必须仍然合法");
    }
    // -------------------------------------------------------------
    // STAGE-15 A-9 — the report order of V-1 ~ V-9 does not move
    // -------------------------------------------------------------
    /// **STAGE-15 A-9** — a description that breaks **both** V-2 (empty
    /// `id`) and V-13 (a zero baud rate) reports **V-2**.
    ///
    /// ⚠️ The new rules were **appended after V-9** ⇒ the report order
    /// of the old rules is untouched.
    #[test]
    fn a9_the_empty_id_is_reported_before_a_parameter_violation() {
        let mut desc = board_with(vec![DeviceSpec::Uart16550 {
            id: "UART0".to_string(),
            irq_line: 0,
            baud_rate: 0,
        }]);
        desc.id = String::new();
        let err = desc.validate().expect_err("空 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_02_EMPTY_ID,
            "同时违反 V-2 与 V-13 时必须先报 V-2（实际 {err}）"
        );
    }
    /// **STAGE-15 A-9** — a description that breaks **both** V-9 (two
    /// devices share an `id`) and V-13 (a zero baud rate) reports **V-9**.
    #[test]
    fn a9_the_duplicated_device_id_is_reported_before_a_parameter_violation() {
        let desc = board_with(vec![
            DeviceSpec::Uart16550 {
                id: "DUP".to_string(),
                irq_line: 0,
                baud_rate: 0,
            },
            DeviceSpec::Gpio {
                irq_line: 11,
                id: "DUP".to_string(),
            },
        ]);
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "同时违反 V-9 与 V-13 时必须先报 V-9（实际 {err}）"
        );
    }
    // -------------------------------------------------------------
    // STAGE-16 · v0.31 · D-137 / D-138 / D-139 — the **A group**
    // (frozen spec STAGE-16 §5).
    //
    // ⚠️ The **A-n** labels below are the frozen spec STAGE-16 §5's
    // acceptance items (A-1 ~ A-12) — **not** the older "A-n" labels the
    // STAGE-12 / STAGE-15 cases above carry ⇒ every label here says
    // "STAGE-16" and every test name says "s16_a".
    //
    // ⚠️ **Why the protocol-shaped cases live here**: the A group is the
    // **contract layer's** group ⇒ its cases are collected in one place
    // ⇒ the protocol-side DTOs are imported from
    // [`crate::protocol`] (the contract layer's own module — no cycle,
    // no second copy of a DTO).
    // -------------------------------------------------------------
    use crate::error::{E_PLT_13_REVISION_CONFLICT, E_PLT_14_REGION_NOT_FOUND};
    use crate::protocol::{CommandPayload, EventPayload, MemoryMapOp};
    /// **STAGE-16 A-1** — the **contract envelope**'s schema version is
    /// pinned to a **literal** ⇒ the constant cannot vouch for itself.
    ///
    /// ⚠️ Asserted against the **literal**, not against the constant
    /// it also defines ⇒ the constant cannot vouch for itself.
    ///
    /// ⚠️ **v0.44** (STAGE-29 · 裁决 A) moved the envelope version
    /// **7 → 8** ⇒ ⇒ the literal follows ⇒ ⇒ the *name* follows too
    /// ⇒ ⇒ a name that still said "seven" would be a lie.
    ///
    /// ⚠️ **v0.57** (STAGE-42) moved it **8 → 9**（符号查询 + 整段转储 +
    /// 条件断点表达式）⇒ ⇒ 同理：literal 与 name 一起跟。
    ///
    /// ⚠️ **v0.72** (STAGE-48 · D-177 A) moved it **10 → 11**（`MemoryMap`
    /// 事件多一个字段 ⇒ ⇒ **线上形状变了**）⇒ ⇒ 同理。
    ///
    /// ⚠️ **v0.76** (STAGE-52 · 裁决 M) moved it **11 → 12**（模板库协议：
    /// 命令 / 事件各加两个 ⇒ ⇒ **线上形状变了**）⇒ ⇒ 同理。
    #[test]
    fn s16_a1_the_contract_schema_version_is_thirteen() {
        assert_eq!(crate::SCHEMA_VERSION, 15);
    }
    /// **STAGE-16 A-2** — the **description format**'s version is still
    /// `1`.
    ///
    /// ⚠️ The format only **gained** one field with a `serde(default)`
    /// ⇒ it is **not** a breaking change ⇒ raising the number would
    /// invalidate **every** board stored by v0.30 (D-143) ⇒ it stays.
    #[test]
    fn s16_a2_the_platform_schema_version_is_still_one() {
        // ⚠️ **v0.73（STAGE-49 · B2）**：这一条**当时**的判据（格式**没变**
        // ⇒ ⇒ 版本**不动**）**已经被本批推翻**（⇒ ⇒ 本批**改了**形状 ⇒ ⇒
        // 版本**动了** 1 → 2）⇒ ⇒ ⇒ **断言跟着**（⇒ ⇒ 数字是 2）⇒ ⇒
        // **判别力保留**（它钉的仍是「这个数就是当前描述格式版本」）。
        //
        // ⚠️ ⚠️ **v0.74（STAGE-50 · 施工期订正）**：**字面量**（2 → 3）⇒ ⇒
        //   ⇒ **每一次合法升版都会把它打死**⇒ ⇒ ⇒ **改成不随批次失效的
        //   性质**：「**从版本 1 到当前版本，每一个都被接受**」⇒ ⇒ 那就是
        //   「**缺省优先**」这条纪律在**列表**上的样子（⇒ ⇒ 存量板档案
        //   零风险）⇒ ⇒ ⇒ **判别力更强**（列表乱了也红）⇒ ⇒ ⇒ **具体数值**
        //   归**最新**那一批的门禁守（`tools/stage50_check.py`）。
        assert!(
            SUPPORTED_PLATFORM_SCHEMA_VERSIONS
                .windows(2)
                .all(|pair| pair[0] < pair[1]),
            "可读版本列表必须**严格递增**（乱了 ⇒ ⇒ 老版本可能悄悄不被接受）"
        );
        assert_eq!(
            SUPPORTED_PLATFORM_SCHEMA_VERSIONS
                .last()
                .copied()
                .unwrap_or_default(),
            PLATFORM_SCHEMA_VERSION,
            "可读版本列表的**最后一项**必须就是当前版本（从版本 1 到当前版本，每一个都被接受）"
        );
    }
    /// **STAGE-16 A-3** — the three built-in descriptions parse
    /// **unchanged** ⇒ `max_region_bytes` is `None` ⇒ their behaviour is
    /// **byte for byte** the same.
    ///
    /// ⚠️ **Why the hand-written copies**: the contract layer does
    /// **not** depend on the kernel crate ⇒ the equivalent copies live
    /// here (the parameters are copied field by field).
    #[test]
    fn s16_a3_the_builtin_descriptions_parse_unchanged() {
        let builtins = [
            r#"{
                "schema_version": 1,
                "id": "default",
                "name": "Default",
                "priority": 10,

                "region_budget_bytes": 16777216,
                "regions": [
                    { "kind": "ram", "base": 0, "size": 1048576 },
                    { "kind": "peripheral", "base": 1073741824,
                      "device": { "device": "gpio", "id": "GPIO0" } },
                    { "kind": "ram", "base": 2147483648, "size": 4194304 }
                ]
            }"#,
            r#"{
                "schema_version": 1,
                "id": "minimal",
                "name": "Minimal (zero-change demo platform)",
                "priority": 30,

                "region_budget_bytes": 16777216,
                "regions": [
                    { "kind": "ram", "base": 0, "size": 2097152 },
                    { "kind": "ram", "base": 268435456, "size": 16777216 }
                ]
            }"#,
            r#"{
                "schema_version": 1,
                "id": "qemu-virt",
                "name": "QEMU virt",
                "priority": 20,

                "region_budget_bytes": 134217728,
                "regions": [
                    { "kind": "ram", "base": 0, "size": 1048576 },
                    { "kind": "peripheral", "base": 33554432,
                      "device": { "device": "clint", "id": "CLINT", "freq_hz": 10000000, "harts": 1 } },
                    { "kind": "peripheral", "base": 201326592,
                      "device": { "device": "plic", "id": "PLIC", "sources": 64, "contexts": 16 } },
                    { "kind": "peripheral", "base": 268435456,
                      "device": { "device": "uart16550", "id": "UART0", "irq_line": 1, "baud_rate": 115200 } },
                    { "kind": "peripheral", "base": 1073741824,
                      "device": { "device": "gpio", "id": "GPIO0" } },
                    { "kind": "ram", "base": 2147483648, "size": 134217728 }
                ]
            }"#,
        ];
        for raw in builtins {
            let desc: PlatformDescription = serde_json::from_str(raw).expect("内置描述必须能解析");
            assert_eq!(
                desc.max_region_bytes, None,
                "内置描述不含 max_region_bytes ⇒ 缺省必须是 None"
            );
            assert!(desc.validate().is_ok(), "内置描述 {} 必须仍然合法", desc.id);
        }
    }
    /// **STAGE-16 A-4** — an **old** description (a JSON without
    /// `max_region_bytes`) parses ⇒ `max_region_bytes == None`.
    ///
    /// ⚠️ This is the "old descriptions keep working" half of 裁决 E: the
    /// field carries `#[serde(default)]` ⇒ **not** naming it is legal.
    ///
    /// ⚠️ **Measured note (v0.31, serde 1.0.229)**: serde's derive
    /// already fills a **missing `Option` field** with `None` ⇒ the
    /// attribute is belt-and-braces ⇒ what this case pins is the
    /// **behaviour** (an old description parses and stays legal), which
    /// is the acceptance criterion.
    #[test]
    fn s16_a4_an_old_description_without_the_new_field_parses() {
        let raw = r#"{
            "schema_version": 1,
            "id": "x",
            "name": "x",
            "priority": 1,
            "region_budget_bytes": 4096,
            "regions": [ { "kind": "ram", "base": 0, "size": 1024 } ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        assert_eq!(
            desc.max_region_bytes, None,
            "旧描述缺这个键 ⇒ 必须是 None（= 等于声明期那个预算）"
        );
        assert!(desc.validate().is_ok(), "旧描述必须仍然合法");
    }
    /// **STAGE-16 A-5** — `max_region_bytes = Some(0)` is **rejected**
    /// with `E_PLT-12`.
    ///
    /// ⚠️ A runtime ceiling of **zero** would make **every** region
    /// exceed it ⇒ the board would be unusable ⇒ it is rejected at
    /// declaration time.
    #[test]
    fn s16_a5_a_zero_runtime_ceiling_is_rejected() {
        let mut desc = sample();
        desc.max_region_bytes = Some(0);
        let err = desc.validate().expect_err("运行期上界为 0 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "V-16 的场景码必须是 E_PLT-12（实际 {err}）"
        );
    }
    /// **STAGE-16 A-6** — `max_region_bytes = Some(1)` is **legal**: the
    /// lower bound itself is a legal value.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **not** on the
    /// message text.
    #[test]
    fn s16_a6_the_lowest_legal_runtime_ceiling_is_accepted() {
        let mut desc = sample();
        desc.max_region_bytes = Some(1);
        assert!(desc.validate().is_ok(), "运行期上界的下界 1 本身必须合法");
    }
    /// **STAGE-16 A-7** — V-16 is reported **after** the old rules ⇒ the
    /// existing "V-2 first" / "V-9 first" order assertions still hold.
    ///
    /// ⚠️ **Why not "V-15 before V-16"?** V-15 and V-16 **share** the
    /// scenario code `E_PLT-12` ⇒ their relative order **cannot** be
    /// told apart by the code (and the judgement never looks at the
    /// message text) ⇒ the order is pinned through the rules with
    /// **distinct** codes (V-2 / V-9), which are earlier than V-15 and
    /// therefore earlier than V-16 as well.
    #[test]
    fn s16_a7_the_old_rules_are_reported_before_v16() {
        // V-2 (empty id) + V-16 (zero runtime ceiling) ⇒ V-2 first.
        let mut desc = sample();
        desc.id = String::new();
        desc.max_region_bytes = Some(0);
        let err = desc.validate().expect_err("空 id 必须被拒");
        assert_eq!(
            err.scenario, E_PLT_02_EMPTY_ID,
            "同时违反 V-2 与 V-16 时必须先报 V-2（实际 {err}）"
        );

        // V-9 (two devices share an id) + V-16 ⇒ V-9 first.
        let mut desc = board_with(vec![
            DeviceSpec::Uart16550 {
                id: "DUP".to_string(),
                irq_line: 0,
                baud_rate: 115_200,
            },
            DeviceSpec::Gpio {
                irq_line: 11,
                id: "DUP".to_string(),
            },
        ]);
        desc.max_region_bytes = Some(0);
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "同时违反 V-9 与 V-16 时必须先报 V-9（实际 {err}）"
        );
    }
    /// **STAGE-16 A-8** — `E_PLT_13` / `E_PLT_14` are **new** scenario
    /// codes ⇒ each means exactly one thing ⇒ none of them reuses
    /// `E_PLT_08` or `E_PLT-12`.
    #[test]
    fn s16_a8_the_new_scenario_codes_are_distinct_from_the_old_ones() {
        assert_eq!(E_PLT_13_REVISION_CONFLICT, 13);
        assert_eq!(E_PLT_14_REGION_NOT_FOUND, 14);

        assert_ne!(
            E_PLT_13_REVISION_CONFLICT, E_PLT_08_DUPLICATE_DEVICE_ID,
            "修订号冲突不得复用「器件 id 重复」"
        );
        assert_ne!(
            E_PLT_13_REVISION_CONFLICT, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "修订号冲突不得复用「参数越界」"
        );
        assert_ne!(
            E_PLT_14_REGION_NOT_FOUND, E_PLT_08_DUPLICATE_DEVICE_ID,
            "段不存在不得复用「器件 id 重复」"
        );
        assert_ne!(
            E_PLT_14_REGION_NOT_FOUND, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "段不存在不得复用「参数越界」"
        );

        // …and they render as the frozen spec spells them.
        assert_eq!(
            PlatformError::new(PLATFORM_FAMILY, E_PLT_13_REVISION_CONFLICT, "x").to_string(),
            "E_PLT-13: x"
        );
        assert_eq!(
            PlatformError::new(PLATFORM_FAMILY, E_PLT_14_REGION_NOT_FOUND, "x").to_string(),
            "E_PLT-14: x"
        );
    }
    /// **STAGE-16 A-9** — the `MemoryMapOp` **wire shape** is pinned, one
    /// case per variant ⇒ and the nested `RegionSpec` stays
    /// **internally tagged** ⇒ **the two layers' rules differ**.
    ///
    /// ⚠️ **External tagging** ⇒ the wire key is the **Rust variant
    /// name** (`UpsertRegion` / `RemoveRegion` / `SetBudget`) ⇒ there is
    /// **no** `op` key (that would be the internal tagging).
    ///
    /// ⚠️ The nested `RegionSpec` is **internally tagged** ⇒ its wire
    /// form is `{"kind": "ram", ...}` ⇒ the two layers write their own
    /// rules.
    #[test]
    fn s16_a9_the_memory_map_op_wire_shape_is_pinned() {
        let upsert = MemoryMapOp::UpsertRegion {
            spec: RegionSpec::Ram {
                base: Some(0),
                size: 1_048_576,
            },
        };
        assert_eq!(
            serde_json::to_string(&upsert).expect("json"),
            r#"{"UpsertRegion":{"spec":{"kind":"ram","base":0,"size":1048576}}}"#
        );

        let remove = MemoryMapOp::RemoveRegion { base: 0 };
        assert_eq!(
            serde_json::to_string(&remove).expect("json"),
            r#"{"RemoveRegion":{"base":0}}"#
        );

        let budget = MemoryMapOp::SetBudget {
            region_budget_bytes: 16_777_216,
            max_region_bytes: None,
        };
        assert_eq!(
            serde_json::to_string(&budget).expect("json"),
            r#"{"SetBudget":{"region_budget_bytes":16777216,"max_region_bytes":null}}"#
        );

        // The two layers' tag rules differ: the outer key is the variant
        // name, the inner one is `kind`.
        let value = serde_json::to_value(&upsert).expect("json");
        assert!(
            value.get("UpsertRegion").is_some(),
            "外层键必须是变体名（外部标签）"
        );
        assert!(
            value.get("op").is_none(),
            "不许有 op 键（那是内部标签 ⇒ 裁决 H 的硬约束）"
        );
        let spec = value
            .get("UpsertRegion")
            .and_then(|v| v.get("spec"))
            .expect("spec");
        assert_eq!(
            spec.get("kind").and_then(|v| v.as_str()),
            Some("ram"),
            "内层的 RegionSpec 仍是内部标签（kind）"
        );
    }
    /// **STAGE-16 A-10** — a `RegionSpec` **nested in** a `MemoryMapOp`
    /// survives the round trip ⇒ serde's nested internal tag limit is
    /// **not** hit.
    ///
    /// ⚠️ This is 裁决 H's discriminating case: the delta operation
    /// carries a **whole** region spec ⇒ if the two tag rules were
    /// mixed up, the spec would not come back.
    ///
    /// ⚠️ **Measured note (v0.31, serde 1.0.229)**: the round trip
    /// succeeds under **both** taggings ⇒ what actually holds the tag
    /// rule is the **pinned wire shape** (A-9), not this round trip.
    #[test]
    fn s16_a10_a_region_spec_nested_in_a_memory_map_op_round_trips() {
        let specs = [
            RegionSpec::Ram {
                base: Some(0),
                size: 1_048_576,
            },
            RegionSpec::Rom {
                base: Some(0x0010_0000),
                size: 0x0001_0000,
            },
            RegionSpec::Peripheral {
                base: Some(0x1000_0000),
                device: DeviceSpec::Gpio {
                    irq_line: 11,
                    id: "GPIO0".to_string(),
                },
            },
            RegionSpec::Peripheral {
                base: Some(0x2000_0000),
                device: DeviceSpec::Uart16550 {
                    id: "UART0".to_string(),
                    irq_line: 1,
                    baud_rate: 115_200,
                },
            },
        ];
        for spec in specs {
            let op = MemoryMapOp::UpsertRegion { spec: spec.clone() };
            let text = serde_json::to_string(&op).expect("json");
            let back: MemoryMapOp = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("嵌套的内部标签必须读得回来（{text}）：{e}"));
            assert_eq!(
                back,
                MemoryMapOp::UpsertRegion { spec },
                "嵌套的 RegionSpec 必须逐字段原样回来（{text}）"
            );
        }
    }
    /// **STAGE-16 A-11** — an **old payload** (one without
    /// `expected_revision`) still parses ⇒ `expected_revision == None`
    /// ⇒ the old caller is **not** affected.
    ///
    /// ⚠️ **Measured note (v0.31, serde 1.0.229)**: serde's derive
    /// already fills a **missing `Option` field** with `None` ⇒ the
    /// frozen spec's `#[serde(default)]` on `expected_revision` is
    /// belt-and-braces ⇒ what this case pins is the **behaviour** (an old
    /// payload parses ⇒ `None` ⇒ the revision is **not** checked), not
    /// the presence of the attribute.
    #[test]
    fn s16_a11_an_old_payload_without_the_expected_revision_still_parses() {
        // `SetMemoryMap` without the key.
        let mut value = serde_json::to_value(CommandPayload::SetMemoryMap {
            description: sample(),
            expected_revision: Some(9),
        })
        .expect("json");
        value["SetMemoryMap"]
            .as_object_mut()
            .expect("object")
            .remove("expected_revision");
        match serde_json::from_value::<CommandPayload>(value).expect("旧载荷必须能解析") {
            CommandPayload::SetMemoryMap {
                expected_revision, ..
            } => assert_eq!(
                expected_revision, None,
                "旧载荷缺这个键 ⇒ 必须解析成 None（= 不检查修订号）"
            ),
            other => panic!("expected a SetMemoryMap, got {other:?}"),
        }

        // `PatchMemoryMap` without the key.
        let mut value = serde_json::to_value(CommandPayload::PatchMemoryMap {
            ops: vec![MemoryMapOp::RemoveRegion { base: 0 }],
            expected_revision: Some(9),
        })
        .expect("json");
        value["PatchMemoryMap"]
            .as_object_mut()
            .expect("object")
            .remove("expected_revision");
        match serde_json::from_value::<CommandPayload>(value).expect("旧载荷必须能解析") {
            CommandPayload::PatchMemoryMap {
                ops,
                expected_revision,
            } => {
                assert_eq!(
                    expected_revision, None,
                    "旧载荷缺这个键 ⇒ 必须解析成 None（= 不检查修订号）"
                );
                assert_eq!(
                    ops,
                    vec![MemoryMapOp::RemoveRegion { base: 0 }],
                    "差量操作必须原样回来"
                );
            }
            other => panic!("expected a PatchMemoryMap, got {other:?}"),
        }
    }
    // -------------------------------------------------------------
    // STAGE-18 · v0.33 · D-142 / D-143 — the **A group**
    // (frozen spec STAGE-18 §5).
    //
    // ⚠️ The **A-n** labels below are the frozen spec STAGE-18 §5's
    // acceptance items (A-1 ~ A-7) — **not** the older "A-n" labels the
    // STAGE-12 / STAGE-15 / STAGE-16 cases above carry ⇒ every label here
    // says "STAGE-18" and every test name says "s18".
    // -------------------------------------------------------------
    /// **STAGE-18 A-1** — a board with **one** device whose `id` is empty
    /// is **rejected** with `E_PLT-15`.
    ///
    /// ⚠️ The judgement is on `family` + `scenario`, **not** on the
    /// message text.
    ///
    /// ⚠️ **Why "one"**: with **two** empty ids, V-9 fires first (A-2)
    /// ⇒ the case that reaches V-17 at all is exactly the **single** one.
    #[test]
    fn s18_a1_a_single_empty_device_id_is_rejected() {
        let desc = board_with(vec![DeviceSpec::Gpio {
            id: String::new(),
            irq_line: 11,
        }]);
        let err = desc.validate().expect_err("空 id 的器件必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_15_DEVICE_EMPTY_ID,
            "V-17 的场景码必须是 E_PLT-15（实际 {err}）"
        );
    }
    /// **STAGE-18 A-2** — **two** devices with an empty `id` are still
    /// rejected by **V-9 / `E_PLT-08`** ⇒ V-17 does **not** take that
    /// case away from V-9.
    #[test]
    fn s18_a2_two_empty_device_ids_are_still_rejected_by_v9() {
        let desc = board_with(vec![
            DeviceSpec::Gpio {
                id: String::new(),
                irq_line: 11,
            },
            DeviceSpec::Gpio {
                id: String::new(),
                irq_line: 11,
            },
        ]);
        let err = desc.validate().expect_err("两个空 id 的器件必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "两个空 id 仍然归 V-9 / E_PLT-08（实际 {err}）"
        );
    }
    /// **STAGE-18 A-3** — the three **built-in** descriptions are still
    /// legal under V-17 ⇒ the **stock risk is zero**: the boards the
    /// kernel ships with have **no** empty device id.
    ///
    /// ⚠️ **These are hand-written copies** — the contract layer does
    /// **not** depend on the kernel crate ⇒ the equivalent copies live
    /// here (the parameters are copied field by field) ⇒ the dependency
    /// direction stays one-way.
    ///
    /// ⚠️ **The copies are the STAGE-15 A-8 ones** ([`BUILTIN_DEFAULT_RAW`]
    /// / [`BUILTIN_MINIMAL_RAW`] / [`BUILTIN_QEMU_VIRT_RAW`]) ⇒ **one**
    /// copy, several readers ⇒ no second copy that could drift from the
    /// first one unnoticed.
    ///
    /// ⚠️ **If a copy drifts from the real file**, the **kernel** side
    /// catches it (`kernel_host::tests_platform`'s
    /// `the_builtin_descriptions_are_well_formed` reads the **real**
    /// files) ⇒ the two layers have one job each: **this** crate judges
    /// the **rules**, the **host** judges the **files**.
    #[test]
    fn s18_a3_the_builtin_descriptions_are_still_legal() {
        for (name, raw) in builtin_copies() {
            let desc: PlatformDescription = serde_json::from_str(raw)
                .unwrap_or_else(|e| panic!("内置描述 {name} 必须能解析：{e}"));
            assert_eq!(desc.id, name, "内置描述的 id 必须与文件名一致");
            let outcome = desc.validate();
            assert!(
                outcome.is_ok(),
                "内置描述 {name} 必须仍然合法（实际 {:?}）",
                outcome.err()
            );
        }
    }
    /// **STAGE-18 A-4** — the **description's** `id` is empty ⇒ still
    /// rejected by **V-2 / `E_PLT-02`** ⇒ V-17 did not take that case
    /// away from V-2 either.
    #[test]
    fn s18_a4_an_empty_description_id_is_still_rejected_by_v2() {
        let mut desc = sample();
        desc.id = String::new();
        let err = desc.validate().expect_err("空的描述 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_02_EMPTY_ID,
            "描述的 id 归 V-2 / E_PLT-02（实际 {err}）"
        );
    }
    /// **STAGE-18 A-5** — the **rule set**'s version and the **shape**'s
    /// version are **not** the same thing ⇒ only the first one ever moves.
    ///
    /// ⚠️ **v0.34 (D-125)**: the rule set was **tightened** again (V-18 /
    /// V-19) ⇒ the rule set's version is now **3** ⇒ the literal `2` of
    /// this case is **history** ⇒ what survives is the **invariant** it
    /// pinned (the two numbers are never the same) ⇒ the rule set's
    /// **current** number is pinned by the STAGE-19 case
    /// ([`s19_a6_the_rules_version_is_three_and_the_other_two_versions_are_untouched`]).
    ///
    /// ⚠️ Asserted against the **literals**, not against the constants
    /// they define ⇒ a constant cannot vouch for itself.
    #[test]
    fn s18_a5_the_shape_version_is_still_one_and_never_equals_the_rules_version() {
        // ⚠️ **v0.73（STAGE-49 · B2）**：形状版本 **1 → 2** ⇒ ⇒ 断言跟到
        // **2** ⇒ ⇒ **判别力保留**（它钉的仍是「形状版本 == 当前那个」+
        // 「它与规则集版本**永不相等**」）。
        //
        // ⚠️ ⚠️ **v0.74（STAGE-50 · 施工期订正）**：**字面量**（2 → 3）⇒ ⇒
        //   ⇒ **每一次合法升版都会把它打死**⇒ ⇒ ⇒ **字面量那半**改成
        //   「**当前形状版本必须被本构建接受**」（⇒ ⇒ **判别力不丢**：升了
        //   版却忘了加进可读列表 ⇒ ⇒ 照样红）⇒ ⇒ ⇒ **具体数值**归**最新**
        //   那一批的门禁守（`tools/stage50_check.py`）。
        assert!(
            SUPPORTED_PLATFORM_SCHEMA_VERSIONS.contains(&PLATFORM_SCHEMA_VERSION),
            "当前形状版本必须被本构建接受（升了版却忘了加进可读列表 ⇒ ⇒ 老描述会被拒）"
        );
        assert_ne!(
            PLATFORM_RULES_VERSION, PLATFORM_SCHEMA_VERSION,
            "规则集版本与形状版本是两个不同的东西，永不相等"
        );
    }
    /// **STAGE-18 A-6** — **regression**: a non-empty device `id` behaves
    /// **exactly** as before ⇒ V-17 changed nothing for the legal cases.
    #[test]
    fn s18_a6_a_non_empty_device_id_behaves_exactly_as_before() {
        // The sample board (five devices, all with non-empty ids) is legal.
        assert!(
            sample().validate().is_ok(),
            "非空 id 的一切既有行为必须一字不变"
        );

        // Two devices sharing a **non-empty** id are still rejected by
        // V-9 / `E_PLT-08` ⇒ V-17 did not swallow that case either.
        let desc = board_with(vec![
            DeviceSpec::Gpio {
                irq_line: 11,
                id: "DUP".to_string(),
            },
            DeviceSpec::Uart16550 {
                id: "DUP".to_string(),
                irq_line: 1,
                baud_rate: 115_200,
            },
        ]);
        let err = desc.validate().expect_err("重复的器件 id 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "非空 id 的重复仍然归 V-9 / E_PLT-08（实际 {err}）"
        );
    }
    /// **STAGE-18 A-7** — a device `id` that is **one space** is
    /// **legal** ⇒ the judgement is `is_empty()`, **not**
    /// `trim().is_empty()` ⇒ the front end and the kernel use **one**
    /// wording for "empty".
    #[test]
    fn s18_a7_a_blank_device_id_is_legal() {
        let desc = board_with(vec![DeviceSpec::Gpio {
            irq_line: 11,
            id: " ".to_string(),
        }]);
        assert!(
            desc.validate().is_ok(),
            "只含一个空格的 id 必须合法（判据是 is_empty，不 trim）"
        );
    }
    /// The new scenario code means **exactly one thing** ⇒ it is
    /// distinct from the three codes it must not be confused with
    /// (the description's empty id / two devices sharing an id / a
    /// parameter out of range).
    #[test]
    fn s18_the_new_scenario_code_is_distinct_from_the_ones_it_must_not_reuse() {
        assert_eq!(E_PLT_15_DEVICE_EMPTY_ID, 15);
        assert_ne!(
            E_PLT_15_DEVICE_EMPTY_ID, E_PLT_02_EMPTY_ID,
            "器件的 id 为空不得复用「描述的 id 为空」"
        );
        assert_ne!(
            E_PLT_15_DEVICE_EMPTY_ID, E_PLT_08_DUPLICATE_DEVICE_ID,
            "器件的 id 为空不得复用「两个器件共用一个 id」"
        );
        assert_ne!(
            E_PLT_15_DEVICE_EMPTY_ID, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "器件的 id 为空不得复用「器件参数越界」"
        );
        assert_eq!(
            PlatformError::new(PLATFORM_FAMILY, E_PLT_15_DEVICE_EMPTY_ID, "x").to_string(),
            "E_PLT-15: x"
        );
    }
    // -------------------------------------------------------------
    // STAGE-19 · v0.34 · D-125 — the **A group**
    // (frozen spec STAGE-19 §5).
    //
    // ⚠️ The **A-n** labels below are the frozen spec STAGE-19 §5's
    // acceptance items (A-1 ~ A-7) — **not** the older "A-n" labels the
    // STAGE-12 / STAGE-15 / STAGE-16 / STAGE-18 cases above carry ⇒
    // every label here says "STAGE-19" and every test name says "s19".
    // -------------------------------------------------------------
    /// 一块「有 UART 与 PLIC」的板 —— 两条新规则都拿它当底板。
    ///
    /// ⚠️ PLIC 的 `sources = 16` ⇒ 合法源号是 `0 ..= 15` ⇒ 越界样本
    /// 写 `16` 与 `99` 都能越界。
    fn board_with_uart_and_plic() -> PlatformDescription {
        board_with(vec![
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 1,
                baud_rate: 115_200,
            },
            DeviceSpec::Plic {
                id: "PLIC".to_string(),
                sources: 16,
                contexts: 1,
            },
        ])
    }
    /// **STAGE-19 A-4** — **two** devices with an empty `id` are still
    /// rejected by **V-9 / `E_PLT-08`** ⇒ the new rules did not take
    /// that case away from V-9.
    #[test]
    fn s19_a4_two_empty_device_ids_are_still_rejected_by_v9() {
        let desc = board_with(vec![
            DeviceSpec::Gpio {
                id: String::new(),
                irq_line: 11,
            },
            DeviceSpec::Gpio {
                id: String::new(),
                irq_line: 11,
            },
        ]);
        let err = desc.validate().expect_err("两个空 id 的器件必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, E_PLT_08_DUPLICATE_DEVICE_ID,
            "两个空 id 仍然归 V-9 / E_PLT-08（实际 {err}）"
        );
    }
    /// **STAGE-19 A-5** — the new scenario code means **exactly one
    /// thing** ⇒ it is distinct from the four codes it must not be
    /// confused with.
    #[test]
    fn s19_a5_the_new_scenario_code_is_distinct_from_the_ones_it_must_not_reuse() {
        assert_eq!(E_PLT_16_IRQ_SOURCE_INVALID, 16);
        assert_ne!(
            E_PLT_16_IRQ_SOURCE_INVALID, E_PLT_02_EMPTY_ID,
            "连线非法不得复用「描述的 id 为空」"
        );
        assert_ne!(
            E_PLT_16_IRQ_SOURCE_INVALID, E_PLT_08_DUPLICATE_DEVICE_ID,
            "连线非法不得复用「两个器件共用一个 id」"
        );
        assert_ne!(
            E_PLT_16_IRQ_SOURCE_INVALID, E_PLT_12_PARAMETER_OUT_OF_RANGE,
            "连线非法不得复用「器件参数越界」"
        );
        assert_ne!(
            E_PLT_16_IRQ_SOURCE_INVALID, E_PLT_15_DEVICE_EMPTY_ID,
            "连线非法不得复用「器件的 id 为空」"
        );
        assert_eq!(
            PlatformError::new(PLATFORM_FAMILY, E_PLT_16_IRQ_SOURCE_INVALID, "x").to_string(),
            "E_PLT-16: x"
        );
    }
    /// **STAGE-19 A-6** — the **rule set**'s version is `3` and the
    /// **description format**'s version did **not** move.
    ///
    /// ⚠️ Asserted against the **literals**, not against the constants
    /// they define ⇒ a constant cannot vouch for itself.
    ///
    /// ⚠️ **Why only the rule set moves**: the rule set was **tightened**
    /// (two more rules) ⇒ the **shape** did not change (one optional
    /// field with a default) ⇒ `PLATFORM_SCHEMA_VERSION` stays **1** ⇒ ⇒
    /// **every board stored by v0.30 (D-143) stays valid**.
    ///
    /// ⚠️ **v0.44** (STAGE-29 · 裁决 A) moved the **contract envelope**'s
    /// version on purpose (the admin channel's reply side is a **wire
    /// shape** ⇒ ⇒ it must be versioned) ⇒ ⇒ the old wording ("the
    /// **other two** versions are untouched") would be a lie ⇒ ⇒ this
    /// test now pins the two versions that **did** stay, and the
    /// envelope version is pinned by
    /// `protocol::tests::the_schema_version_is_eight`.
    #[test]
    fn s19_a6_the_rules_version_is_three_and_the_description_version_is_still_one() {
        // ⚠️ **v0.73（STAGE-49 · B2）**：**两个都动了**（⇒ ⇒ 描述格式**改**
        // 了形状 ⇒ ⇒ 规则集**跟着**走）⇒ ⇒ 规则集 **3 → 4**、形状 **1 → 2**
        // ⇒ ⇒ ⇒ 断言跟到新值 ⇒ ⇒ **判别力保留**（它钉的仍是「这两个数
        // 就是当前那两个」）。
        //
        // ⚠️ ⚠️ **v0.74（STAGE-50 · 施工期订正）**：**字面量**（规则集 4 → 5 ·
        //   形状 2 → 3）⇒ ⇒ ⇒ **每一次合法升版都会把它打死**⇒ ⇒ ⇒ 改成
        //   **不随批次失效的性质**：
        //   - **规则集版本 ≥ 形状版本**（⇒ ⇒ 「规则集**跟着**形状走」那条
        //     纪律**结构上**成立 ⇒ ⇒ 规则集不许落在形状后面）；
        //   - **两者永不相等**（⇒ ⇒ 它们是**两个**不同的数）。
        //   ⇒ ⇒ ⇒ **具体数值**归**最新**那一批的门禁守
        //   （`tools/stage50_check.py`）⇒ ⇒ ⇒ **判别力不丢**（「形状变了却
        //   忘了动版本」这类缺陷**照样**被别处的判据抓住）。
        // ⚠️ **常量断言** ⇒ ⇒ 写成 `const _: () = assert!(…)` ⇒ ⇒ 否则
        //   `clippy::assertions_on_constants` 会把这条**打红**（⇒ ⇒ 门禁红）。
        const _: () = assert!(PLATFORM_RULES_VERSION >= PLATFORM_SCHEMA_VERSION);
        const _: () = assert!(
            PLATFORM_RULES_VERSION != PLATFORM_SCHEMA_VERSION,
            "规则集版本与描述格式版本是两个不同的东西，永不相等"
        );
    }
    /// **STAGE-19 补强** — the **RAM window** is the **base-maximal**
    /// RAM region ⇒ the single landing spot both sides ask.
    ///
    /// ⚠️ **"Base-maximal", not "largest"**: a **small** segment at a
    /// **high** base wins ⇒ the rule is a comparison of the **base**
    /// alone ⇒ the host cannot pick a different one by taste.
    #[test]
    fn s19_the_ram_window_is_the_base_maximal_ram_region() {
        let mut desc = sample();
        // The sample board has a 1 MiB RAM at 0 and a 128 MiB RAM at
        // 0x8000_0000 ⇒ the latter is the one.
        assert_eq!(desc.ram_window(), Some((0x8000_0000, 134_217_728)));

        // A **small** segment at a **higher** base wins ⇒ the size does
        // not decide it.
        desc.regions.push(RegionSpec::Ram {
            base: Some(0x9000_0000),
            size: 4096,
        });
        assert_eq!(desc.ram_window(), Some((0x9000_0000, 4096)));

        // A ROM segment is **not** a RAM window.
        let mut desc = sample();
        desc.regions = vec![RegionSpec::Rom {
            base: Some(0x9000_0000),
            size: 4096,
        }];
        assert_eq!(desc.ram_window(), None, "没有 RAM 段时必须返回 None");
    }
    /// **X-1** — 同一块板上两个外设**不能共用**一个 PLIC 源号。
    #[test]
    fn x1_two_peripherals_must_not_share_one_plic_source() {
        let raw = r#"{
            "schema_version": 5,
            "id": "dup-irq",
            "name": "Dup irq",
            "priority": 1,
            "region_budget_bytes": 4096,
            "regions": [
                { "kind": "ram", "base": 0, "size": 4096 },
                { "kind": "peripheral", "base": 268435456,
                  "device": { "device": "uart16550", "id": "UART0", "irq_line": 10, "baud_rate": 115200 } },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0", "irq_line": 10 } },
                { "kind": "peripheral", "base": 201326592,
                  "device": { "device": "plic", "id": "PLIC0", "sources": 64, "contexts": 16 } }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        let err = desc.validate().unwrap_err();
        assert_eq!(err.scenario, 16, "中断号重复必须拒");
    }
    /// **X-3** — 一块板最多一个 PLIC：两个 PLIC 实例必须拒。
    ///
    /// ⚠️ 多 PLIC 的板在真实世界里只出现在分区多 hart 或厂商定制控制器
    /// （如 Andes 双 PLIC）的场景；本平台只有一个 hart 上下文，且外设源号
    /// 的抬线路径写死了第一个 PLIC ⇒ ⇒ 第二个 PLIC 永远收不到外设源
    /// ⇒ ⇒ 与其「装得上但半通」，不如在声明期拒掉。
    #[test]
    fn x3_a_board_must_not_declare_more_than_one_plic() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC0".to_string(),
                sources: 64,
                contexts: 16,
            },
            DeviceSpec::Plic {
                id: "PLIC_AUX".to_string(),
                sources: 32,
                contexts: 4,
            },
        ]);
        let err = desc.validate().expect_err("两个 PLIC 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, 24, "多 PLIC 必须报 E_PLT-24（实际 {err}）");
    }

    /// **X-3b** — 多 PLIC + 越界 `irq_line` ⇒ 先报「多 PLIC」而不是 `E_PLT-12`。
    ///
    /// ⚠️ 多 PLIC 是更根本的板形状错误；若 V-27 排在 V-15 之后，用户会先
    /// 看到「中断号越界」，改完中断号才发现板根本不该有两个 PLIC。
    #[test]
    fn x3b_multiple_plic_is_reported_before_irq_line_range() {
        let desc = board_with(vec![
            DeviceSpec::Plic {
                id: "PLIC_SMALL".to_string(),
                sources: 16,
                contexts: 16,
            },
            DeviceSpec::Plic {
                id: "PLIC_BIG".to_string(),
                sources: 64,
                contexts: 16,
            },
            DeviceSpec::Uart16550 {
                id: "UART0".to_string(),
                irq_line: 20,
                baud_rate: 115_200,
            },
        ]);
        let err = desc.validate().expect_err("两个 PLIC 必须被拒");
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(
            err.scenario, 24,
            "多 PLIC 必须先于 irq_line 越界被报出（实际 {err}）"
        );
    }

    /// **X-2** — GPIO 的中断号与 UART 一样受合法取值域约束（> 0 且 < sources）。
    #[test]
    fn x2_gpio_irq_line_is_a_legal_plic_source() {
        let raw = r#"{
            "schema_version": 5,
            "id": "bad-gpio-irq",
            "name": "Bad gpio irq",
            "priority": 1,
            "region_budget_bytes": 4096,
            "regions": [
                { "kind": "ram", "base": 0, "size": 4096 },
                { "kind": "peripheral", "base": 1073741824,
                  "device": { "device": "gpio", "id": "GPIO0", "irq_line": 64 } },
                { "kind": "peripheral", "base": 201326592,
                  "device": { "device": "plic", "id": "PLIC0", "sources": 64, "contexts": 16 } }
            ]
        }"#;
        let desc: PlatformDescription = serde_json::from_str(raw).expect("必须能解析");
        let err = desc.validate().unwrap_err();
        assert_eq!(err.scenario, 12, "GPIO 中断号越界必须拒");
    }
}
