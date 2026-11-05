//! PinmuxDevice — 引脚复用控制器（v0.76 · STAGE-52 · C1 决议）。
//!
//! Register window:
//! - `0x00 + 4×pin` (rw)：**功能选择寄存器** —— 写该引脚的「当前功能索引」。
//!   MVP 最小形态（裁决 I）：每引脚一个 32 位功能索引；**不**建模 AF 矩阵。
//!
//! ⚠️ **平台不解析功能名**（裁决 G / J）：器件层只知道每引脚的**功能数量**
//! （装配期从描述 `pins` 段喂入）⇒ ⇒ 固件写索引 ≥ 功能数 ⇒ ⇒
//! **`E_PLT-23`**（运行时非法 · **不静默**）。
//!
//! ⚠️ 引脚号 = GPIO 引脚号（`0..GPIO_PIN_COUNT`）⇒ ⇒ 寄存器偏移 = `4×引脚号`
//!   ⇒ ⇒ 窗口 = `4 × GPIO_PIN_COUNT`。
//!
//! ⚠️ **分层禁令**（裁决 K）：本器件**不**写 GPIO 的 `irq_lines[pin]` ——
//!   它只持有「引脚 → 当前功能索引」；中断路由由宿主查「当前功能 → irq 源」。

use riscv_sim_contract::error::{PlatformError, E_PLT_23_PINMUX_INVALID, PLATFORM_FAMILY};
use riscv_sim_contract::model::{PendingWrite, WriteSize};
use riscv_sim_contract::port::Peripheral;
use riscv_sim_contract::session::GPIO_PIN_COUNT;

use super::reg::{extract_subword, register_offset};

/// 引脚数（一个落点：契约层 `GPIO_PIN_COUNT`；本文件只引用）。
const PIN_COUNT: u32 = GPIO_PIN_COUNT as u32;

/// 寄存器窗口 = 4 字节 × 每引脚。
pub const PINMUX_WINDOW: u32 = 4 * PIN_COUNT;

#[derive(Debug)]
pub struct PinmuxDevice {
    base: u32,
    id: String,
    /// 每引脚的**当前功能索引**（初始 0 = 第一个功能）。
    cfg: Vec<u32>,
    /// 每引脚的功能**数量**（装配期喂入 · 越界写 ⇒ `E_PLT-23`）。
    function_counts: Vec<u32>,
}

impl PinmuxDevice {
    pub fn new(id: impl Into<String>, base: u32) -> Self {
        Self {
            base,
            id: id.into(),
            cfg: vec![0; PIN_COUNT as usize],
            // ⚠️ 缺省 1 个功能：索引 0 总是合法 ⇒ ⇒ 未喂入功能数的引脚
            //   也能安全读写（写 0 恒合法）。
            function_counts: vec![1; PIN_COUNT as usize],
        }
    }

    /// 装配期：喂入某个引脚的功能数量（来自描述 `pins` 段）。
    ///
    /// ⚠️ 只有**声明了引脚规格**的引脚才喂 > 1 ⇒ ⇒ 其余引脚保持 1
    /// （写 0 恒合法 · 写 1+ ⇒ `E_PLT-23`）。
    pub fn set_function_count(&mut self, pin: u32, count: u32) {
        if let Some(slot) = self.function_counts.get_mut(pin as usize) {
            *slot = count.max(1);
            // 当前索引可能越出新的上限 ⇒ ⇒ 钳回 0（第一个功能）。
            let cur = self.cfg.get_mut(pin as usize).unwrap();
            if *cur >= count.max(1) {
                *cur = 0;
            }
        }
    }

    /// 某个引脚的**当前功能索引**（宿主同步中断用）。
    pub fn current_function(&self, pin: u32) -> Option<u32> {
        self.cfg.get(pin as usize).copied()
    }

    fn read_word(&self, word_offset: u32) -> Result<u32, PlatformError> {
        if word_offset % 4 != 0 || word_offset / 4 >= PIN_COUNT {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_23_PINMUX_INVALID,
                format!(
                    "pinmux 读偏移 0x{:x} 越界（引脚号 ≥ {})",
                    word_offset, PIN_COUNT
                ),
            ));
        }
        let pin = word_offset / 4;
        Ok(self.cfg[pin as usize])
    }

    fn write_word(&mut self, word_offset: u32, value: u32) -> Result<(), PlatformError> {
        if word_offset % 4 != 0 || word_offset / 4 >= PIN_COUNT {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_23_PINMUX_INVALID,
                format!(
                    "pinmux 写偏移 0x{:x} 越界（引脚号 ≥ {})",
                    word_offset, PIN_COUNT
                ),
            ));
        }
        let pin = word_offset / 4;
        let limit = self.function_counts[pin as usize];
        if value >= limit {
            return Err(PlatformError::new(
                PLATFORM_FAMILY,
                E_PLT_23_PINMUX_INVALID,
                format!(
                    "pinmux 引脚 {} 功能索引 {} 越界（该引脚只有 {} 个功能）",
                    pin, value, limit
                ),
            ));
        }
        self.cfg[pin as usize] = value;
        Ok(())
    }
}

impl Peripheral for PinmuxDevice {
    fn id(&self) -> &str {
        &self.id
    }
    fn base_addr(&self) -> u32 {
        self.base
    }
    fn size(&self) -> u32 {
        PINMUX_WINDOW
    }

    fn read(&self, offset: u32, size: WriteSize) -> Result<u32, PlatformError> {
        let word = self.read_word(register_offset(offset))?;
        Ok(extract_subword(word, offset, size))
    }

    fn write(&mut self, offset: u32, _size: WriteSize, value: u32) -> Result<(), PlatformError> {
        self.write_word(register_offset(offset), value)
    }

    fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riscv_sim_contract::error::PLATFORM_FAMILY;

    /// 器件层基础：读写功能索引、越界写拒 `E_PLT-23`、`set_function_count`
    /// 收缩上限时钳回 0。
    #[test]
    fn pinmux_register_semantics() {
        let mut mux = PinmuxDevice::new("PINMUX0", 0x4000_0000);
        assert_eq!(mux.size(), PINMUX_WINDOW);

        // 初始：功能数 1，写 0 合法、写 1 越界。
        assert_eq!(mux.read(0, WriteSize::Word).unwrap(), 0);
        assert_eq!(
            mux.write(0, WriteSize::Word, 1).unwrap_err().scenario,
            E_PLT_23_PINMUX_INVALID
        );

        // 喂入 2 个功能 ⇒ 写 0 / 1 都合法，写 2 越界。
        mux.set_function_count(0, 2);
        mux.write(0, WriteSize::Word, 1).unwrap();
        assert_eq!(mux.read(0, WriteSize::Word).unwrap(), 1);
        assert_eq!(
            mux.write(0, WriteSize::Word, 2).unwrap_err().scenario,
            E_PLT_23_PINMUX_INVALID
        );

        // 越界偏移（引脚号 ≥ PIN_COUNT）。
        let far = 4 * PIN_COUNT;
        let err = mux.write(far, WriteSize::Word, 0).unwrap_err();
        assert_eq!(err.family, PLATFORM_FAMILY);
        assert_eq!(err.scenario, E_PLT_23_PINMUX_INVALID);
    }

    /// `set_function_count` 收缩上限时把当前索引钳回 0。
    #[test]
    fn pinmux_shrink_clamps_current_to_zero() {
        let mut mux = PinmuxDevice::new("PINMUX0", 0x4000_0000);
        mux.set_function_count(5, 3);
        mux.write(4 * 5, WriteSize::Word, 2).unwrap();
        assert_eq!(mux.current_function(5), Some(2));
        mux.set_function_count(5, 1);
        assert_eq!(mux.current_function(5), Some(0));
    }
}
