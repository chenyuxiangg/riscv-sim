//! The **device type registry** (STAGE-43 · v0.66 · debt ledger **D-175**).
//!
//! ⚠️ **This is the one truth about "what the kernel implements"**（裁决 H）:
//! every device type the kernel has a model for appears here **once**, with
//! - its **register window size**（抄器件模型那一份常量 —— 裁决 C）;
//! - its **register map** —— **offsets only, no base address**（裁决 D）;
//! - its **instance parameters** and their legal value domain（**引用契约层
//!   那一份** —— 裁决 E）.
//!
//! ⚠️ **The registry is static** ⇒ ⇒ it is **not** a second store: it is
//! **computed** from the device models on every query ⇒ ⇒ it cannot drift
//! away from them.
//!
//! ⚠️ **It is NOT the same set as [`riscv_sim_contract::platform::DeviceSpec::KINDS`]**:
//! - the registry = what the **kernel implements**（today **5** —— it also
//!   has a timer）;
//! - `DeviceSpec::KINDS` = what the **description format** can write
//!   （today **4**）.
//!
//! ⇒ ⇒ The invariant is **"instantiable ⊆ implemented"** ⇒ ⇒ the timer is
//! reported honestly（裁决 J）⇒ ⇒ **not** hidden.

use riscv_sim_contract::model::{DeviceParamInfo, DeviceRegisterInfo, DeviceTypeInfo};
use riscv_sim_contract::platform::DeviceSpec;

use super::clint::{self, reg as clint_reg};
use super::gpio::{self, reg as gpio_reg};
use super::plic::{self, reg as plic_reg};
use super::timer::{self, reg as timer_reg};
use super::uart::{self, reg as uart_reg};

/// One registered device type.
///
/// ⚠️ The register list is **byte offsets within the window** —— never an
/// absolute address（裁决 D）⇒ ⇒ the same type can be bound to **any** base
/// address at integration time.
#[derive(Debug, Clone, Copy)]
pub struct DeviceTypeEntry {
    /// The type name —— byte-identical to the description's `device` tag.
    pub kind: &'static str,
    /// The register window size in bytes.
    pub window_bytes: u32,
    /// The register map —— `(name, byte offset within the window)`.
    pub registers: &'static [(&'static str, u32)],
}

/// ⚠️ **The CLINT's `msip` array starts at offset 0** —— the model spells
/// out the **stride** (`MSIP_STRIDE = 4`) but the base is 0（its own
/// comment says so）⇒ ⇒ the base is written here as `0x0`.
const CLINT_MSIP_BASE: u32 = 0x0;

/// Every device type the kernel implements —— **the registry**.
///
/// ⚠️ **The offsets come from the device models**（`gpio::reg::DIR` …）⇒ ⇒
/// the model and the registry **cannot** disagree about a layout.
pub const DEVICE_TYPES: &[DeviceTypeEntry] = &[
    DeviceTypeEntry {
        kind: "gpio",
        window_bytes: gpio::GPIO_WINDOW,
        registers: &[
            ("DIR", gpio_reg::DIR),
            ("OUT", gpio_reg::OUT),
            ("IN", gpio_reg::IN),
            ("TOGGLE", gpio_reg::TOGGLE),
        ],
    },
    DeviceTypeEntry {
        kind: "uart16550",
        window_bytes: uart::UART_WINDOW,
        registers: &[
            ("THR", uart_reg::THR),
            ("IER", uart_reg::IER),
            ("FCR", uart_reg::FCR),
            ("LCR", uart_reg::LCR),
            ("MCR", uart_reg::MCR),
            ("LSR", uart_reg::LSR),
            ("MSR", uart_reg::MSR),
            ("SCR", uart_reg::SCR),
        ],
    },
    DeviceTypeEntry {
        kind: "clint",
        window_bytes: clint::CLINT_WINDOW,
        registers: &[
            ("MSIP", CLINT_MSIP_BASE),
            ("MTIMECMP", clint_reg::MTIMECMP),
            ("MTIME", clint_reg::MTIME),
        ],
    },
    DeviceTypeEntry {
        kind: "plic",
        window_bytes: plic::PLIC_WINDOW,
        registers: &[
            ("PENDING", plic_reg::PENDING),
            ("ENABLE", plic_reg::ENABLE),
            ("THRESHOLD", plic_reg::THRESHOLD),
            ("CLAIM", plic_reg::CLAIM),
        ],
    },
    DeviceTypeEntry {
        kind: "timer_riscv",
        window_bytes: timer::TIMER_WINDOW,
        registers: &[
            ("MTIME_LO", timer_reg::MTIME_LO),
            ("MTIME_HI", timer_reg::MTIME_HI),
            ("MTIMECMP_LO", timer_reg::MTIMECMP_LO),
            ("MTIMECMP_HI", timer_reg::MTIMECMP_HI),
        ],
    },
];

/// The registry → the contract layer's DTO（**the wire shape**）.
///
/// ⚠️ **Sorted by kind name** ⇒ ⇒ the order is **deterministic**（the
/// contract freezes it: "顺序**按类型名升序**"）⇒ ⇒ two runs produce the
/// same bytes.
///
/// @returns {Array<object>} — `DeviceTypeInfo` values.
pub fn device_type_infos() -> Vec<DeviceTypeInfo> {
    let mut entries: Vec<&DeviceTypeEntry> = DEVICE_TYPES.iter().collect();
    entries.sort_by_key(|entry| entry.kind);
    entries
        .into_iter()
        .map(|entry| DeviceTypeInfo {
            kind: entry.kind.to_string(),
            window_bytes: entry.window_bytes,
            registers: entry
                .registers
                .iter()
                .map(|(name, offset)| DeviceRegisterInfo {
                    name: (*name).to_string(),
                    offset: *offset,
                    width_bits: 32,
                })
                .collect(),
            params: parameter_infos(entry.kind),
        })
        .collect()
}

/// A kind's parameter domain → the contract layer's DTO.
///
/// ⚠️ **The bounds come from the contract layer**（`DeviceSpec::parameter_domain`
/// —— the **one** landing spot, 裁决 E）⇒ ⇒ this module **never** writes an
/// interval of its own ⇒ ⇒ the front end can show the domain without
/// inventing a second table.
///
/// @param {string} kind — the device kind tag
/// @returns {Array<object>} — `DeviceParamInfo` values
pub fn parameter_infos(kind: &str) -> Vec<DeviceParamInfo> {
    DeviceSpec::parameter_domain(kind)
        .into_iter()
        .map(|(name, min, max)| DeviceParamInfo {
            name: name.to_string(),
            min: Some(min),
            max,
        })
        .collect()
}

/// The registered window size of one kind —— `None` when the kernel does
/// not implement it.
///
/// ⚠️ **The lookup is by name** ⇒ ⇒ the caller does not need to hold a
/// `DeviceTypeEntry`.
///
/// @param {string} kind — the device kind tag
/// @returns {number|null}
pub fn window_bytes(kind: &str) -> Option<u32> {
    DEVICE_TYPES
        .iter()
        .find(|entry| entry.kind == kind)
        .map(|entry| entry.window_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// STAGE-43 K-1 —— the registry holds the **five** types the kernel
    /// implements（the timer included — 裁决 J: it is reported honestly).
    #[test]
    fn the_registry_holds_the_five_types_the_kernel_implements() {
        let kinds: Vec<&str> = DEVICE_TYPES.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec!["gpio", "uart16550", "clint", "plic", "timer_riscv"]
        );
    }

    /// STAGE-43 K-2 —— every window size **equals** the model's own.
    #[test]
    fn every_window_size_equals_the_device_models_own() {
        assert_eq!(window_bytes("gpio"), Some(gpio::GPIO_WINDOW));
        assert_eq!(window_bytes("uart16550"), Some(uart::UART_WINDOW));
        assert_eq!(window_bytes("clint"), Some(clint::CLINT_WINDOW));
        assert_eq!(window_bytes("plic"), Some(plic::PLIC_WINDOW));
        assert_eq!(window_bytes("timer_riscv"), Some(timer::TIMER_WINDOW));
    }

    /// STAGE-43 K-3 —— the register offsets **are** the model's constants
    /// （one landing spot —— the registry cannot drift from the models）.
    #[test]
    fn the_register_offsets_are_the_models_own_constants() {
        let infos = device_type_infos();
        let gpio = infos.iter().find(|t| t.kind == "gpio").expect("gpio");
        assert_eq!(gpio.registers[0].name, "DIR");
        assert_eq!(gpio.registers[0].offset, gpio_reg::DIR);
        assert_eq!(gpio.registers[3].name, "TOGGLE");
        assert_eq!(gpio.registers[3].offset, gpio_reg::TOGGLE);

        let uart = infos.iter().find(|t| t.kind == "uart16550").expect("uart");
        assert_eq!(uart.registers[0].offset, uart_reg::THR);
        assert_eq!(uart.registers[7].offset, uart_reg::SCR);
    }

    /// STAGE-43 K-4 —— the parameter domain is the **contract layer's** one
    /// （裁决 E: the registry writes no interval of its own）.
    #[test]
    fn the_parameter_domain_comes_from_the_contract_layer() {
        let plic = parameter_infos("plic");
        let sources = plic.iter().find(|p| p.name == "sources").expect("sources");
        assert_eq!(sources.min, Some(1));
        assert_eq!(
            sources.max,
            Some(u64::from(riscv_sim_contract::platform::MAX_PLIC_SOURCES))
        );
        // ⚠️ `baud_rate` has **no** upper bound (the architecture gives none)
        // ⇒ ⇒ the registry must not invent one.
        let uart = parameter_infos("uart16550");
        let baud = uart.iter().find(|p| p.name == "baud_rate").expect("baud");
        assert_eq!(baud.min, Some(1));
        assert_eq!(baud.max, None);
    }

    /// STAGE-43 裁决 H —— **every instantiable kind is implemented** ⇒ ⇒
    /// the description format can never ask for a device the kernel cannot
    /// build.
    #[test]
    fn every_instantiable_kind_is_implemented_by_the_kernel() {
        for kind in DeviceSpec::KINDS {
            assert!(
                window_bytes(kind).is_some(),
                "描述格式能写的类型 {kind} 在内核的注册表里**没有** ⇒ ⇒ 用户能写一块装不上的板"
            );
        }
    }

    /// The wire order is **deterministic**（sorted by kind name）.
    #[test]
    fn the_wire_order_is_sorted_by_kind_name() {
        let kinds: Vec<String> = device_type_infos().into_iter().map(|t| t.kind).collect();
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted);
    }
}
