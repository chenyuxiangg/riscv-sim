//! M-14 总线与内存映射 (Bus & Memory Map).
//!
//! Owns the address decoder: given a physical address, return the
//! device that owns the range (a RAM page, a peripheral register, or
//! "no device" → bus-error trap). All bus masters (ISS, peripherals,
//! debug services) go through this module.
//!
//! TODO(MVP): page-granular RAM (4 KiB pages, sparse), address
//! decoder, access-width check, misaligned-access trap.
//!
//! Status: stub — public port only.

use riscv_sim_contract::error::{ErrorFamily, PlatformError};
use riscv_sim_contract::model::WriteSize;

/// Bus address space size. 4 GiB covers the entire RV32 physical
/// address space.
pub const BUS_SIZE: u64 = 0x1_0000_0000;

/// Where a bus access lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusTarget {
    /// Read / write to a generic RAM page.
    Ram,
    /// Read / write to a peripheral register window.
    Peripheral { id: u32 },
    /// No device at this address. Read-as-zero; write traps the engine.
    None,
}

impl Default for BusTarget { fn default() -> Self { Self::None } }

/// Bus master — anything that performs reads / writes.
pub trait BusMaster {
    /// Read `size` bytes at `paddr`.
    fn read(&self, paddr: u32, size: WriteSize) -> Result<u32, PlatformError>;
    /// Write `size` bytes at `paddr`.
    fn write(&mut self, paddr: u32, size: WriteSize, value: u32) -> Result<(), PlatformError>;
}

/// Family used by errors raised from this module.
pub const BUS_FAMILY: ErrorFamily = ErrorFamily::Bus;
