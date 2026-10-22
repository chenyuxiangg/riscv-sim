//! M-15 unit tests (LLD §7 / M-15 v1.2 §7.1).
//!
//! Coverage (v0.7.1 — M-15 review remediation):
//! - UART register map + 1-byte FIFO + overrun (separate TX/RX bits) +
//!   take_tx + read_clear + W1C status + sub-register byte access
//!
//! ⚠️ **v0.23（Lane B）**：UART 从「自定义两寄存器（DATA@0x0 /
//! STATUS@0x4，字宽度）」重做成 **ns16550 兼容**（`THR/RHR/DLL@0x0` ·
//! `IER/DLM@0x1` · `FCR@0x2` · `LCR@0x3` · `MCR@0x4` · `LSR@0x5` ·
//! `MSR@0x6` · `SCR@0x7`，**字节偏移**），并加了**无损 TX 捕获环**。
//! ⇒ 本文件里**属于 UART** 的那几条用例按新布局改了（逐条改动见
//! v0.23 批的交卷记录）；新布局自己的用例写在 `uart.rs` 内部的
//! `#[cfg(test)] mod tests` 里。
//! - GPIO directions, OUT latch, TOGGLE register (doc semantics:
//!   `1<<pin` flips), IN driven externally with direction-aware edge
//!   IRQ, deduplicated pending IRQs, sub-register byte access
//! - Timer mtime / mtimecmp advance + tick-based IRQ arming +
//!   mtime read-only enforcement + sub-register byte access
//! - PeripheralRegistry register/get/iter/describe + duplicate id
//!   detection
//! - Cross-peripheral interaction: UART TX → host bridge → GPIO LED

use super::*;
use riscv_sim_contract::model::WriteSize;
use riscv_sim_contract::port::ProbeSink;
use riscv_sim_contract::session::{
    ClintStateDto, GpioStateDto, M15PeriphDto, PeripheralSessionState, PlicStateDto, UartStateDto,
    GPIO_PIN_COUNT,
};
use std::cell::RefCell;
use std::rc::Rc;

// ---------------------------------------------------------------------------
// Probe reporting (D-12) — the M-15 → M-18 signal source layer.
// ---------------------------------------------------------------------------

/// Recorded probe observations: `(signal, value, now_ps)`.
type ProbeLog = Rc<RefCell<Vec<(String, u64, u64)>>>;

/// Install a probe sink that records every observation as
/// `(signal, value, now_ps)`.
fn recording_probe() -> (ProbeLog, ProbeSink) {
    let log: ProbeLog = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&log);
    (
        log,
        Box::new(move |signal: &str, value: u64, now_ps: u64| {
            sink.borrow_mut().push((signal.to_string(), value, now_ps));
        }),
    )
}

#[test]
fn gpio_reports_out_changes_through_the_probe() {
    // D-12: a pin level change reaches the probe pipeline; an unchanged
    // level must not (it would spam the waveform).
    let (log, sink) = recording_probe();
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.on_probe(Some(sink));
    gpio.set_directions(0xFFFF_FFFF);
    gpio.tick(1_000).unwrap();

    // 初值 0 → 写 0：电平没变 → 不上报。
    gpio.write(0x4, WriteSize::Word, 0).unwrap();
    assert!(
        log.borrow().is_empty(),
        "an unchanged level must not report"
    );

    gpio.write(0x4, WriteSize::Word, 0b101).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, gpio::SIGNAL_OUT);
    assert_eq!(log[0].1, 0b101, "the reported value is the pin bitmap");
    assert_eq!(log[0].2, 1_000, "the timestamp is the peripheral's clock");
}

#[test]
fn gpio_toggle_also_reports_the_new_level() {
    let (log, sink) = recording_probe();
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.on_probe(Some(sink));
    gpio.set_directions(0xFFFF_FFFF);
    gpio.write(0xC, WriteSize::Word, 1 << 3).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, gpio::SIGNAL_OUT);
    assert_eq!(log[0].1, 1 << 3);
}

#[test]
fn uart_reports_tx_bytes_through_the_probe() {
    let (log, sink) = recording_probe();
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    uart.on_probe(Some(sink));
    uart.tick(500).unwrap();

    uart.write(0x0, WriteSize::Byte, u32::from(b'A')).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, uart::SIGNAL_TX);
    assert_eq!(log[0].1, u64::from(b'A'), "the reported value is the byte");
    assert_eq!(log[0].2, 500);
}

#[test]
fn timer_reports_compare_hits_through_the_probe() {
    let (log, sink) = recording_probe();
    let mut tim = TimerRiscv::new("TIM0", 0, 7);
    tim.on_probe(Some(sink));
    tim.set_mtimecmp(1_000);

    tim.tick(500).unwrap();
    assert!(log.borrow().is_empty(), "no hit before the deadline");

    tim.tick(2_000).unwrap();
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].0, timer::SIGNAL_CMP);
    assert_eq!(log[0].1, 1, "the reported value is the hit flag");
    assert_eq!(log[0].2, 2_000);
}

#[test]
fn a_peripheral_without_a_probe_reports_nothing() {
    // 未挂探针的外设不能因为上报路径而出错。
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.write(0x4, WriteSize::Word, 0b1).unwrap();
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
}

#[test]
fn registry_handle_shares_the_same_instance() {
    // D-12: the registry owns the instance; the bus gets a *handle* to
    // the very same object, so a write through the handle is visible
    // through the registry (and vice versa).
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0, 9600, 4)).unwrap();
    let handle = reg.handle("UART0").expect("handle");

    {
        let mut p = handle.borrow_mut();
        let _ = p.write(0x0, WriteSize::Byte, 0x41);
    }
    // ⚠️ **v0.23**：`0x4` 现在是 **MCR**，不再是状态寄存器；能隔着
    // `dyn Peripheral` 看见的「同一个实例」的证据是**会话状态**里的
    // TX 捕获环队头。
    let exported = reg
        .get("UART0")
        .unwrap()
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(state) = exported else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(
        state.tx,
        Some(0x41),
        "the registry and the handle must see the same instance"
    );
}

// ---------------------------------------------------------------------------
// PeripheralRegistry
// ---------------------------------------------------------------------------

#[test]
fn registry_register_get_iter_describe() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x2000_0000, 115_200, 5))
        .unwrap();
    reg.register(GpioBank::new("GPIO0", 0x4000_0000)).unwrap();
    reg.register(TimerRiscv::new("TIM0", 0x6000_0000, 7))
        .unwrap();

    assert_eq!(reg.len(), 3);
    assert!(!reg.is_empty());

    assert!(reg.get("UART0").is_some());
    assert!(reg.get("GPIO0").is_some());
    assert!(reg.get("TIM0").is_some());
    assert!(reg.get("nope").is_none());

    let mut ids = reg.iter().map(|p| p.id().to_string()).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, vec!["GPIO0", "TIM0", "UART0"]);

    assert_eq!(reg.describe("UART0"), Some("behavioural-level"));
    assert_eq!(reg.describe("nope"), None);
}

#[test]
fn registry_duplicate_id_returns_register_error() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0, 9600, 4)).unwrap();
    let err = reg
        .register(Uart16550::new("UART0", 0, 115_200, 5))
        .unwrap_err();
    assert_eq!(err, RegisterError::Duplicate("UART0".to_string()));
    // Original entry must still be present (no silent replacement).
    let uart = reg.get("UART0").unwrap();
    assert_eq!(uart.size(), uart::UART_WINDOW);
}

#[test]
fn registry_get_mut_allows_state_changes() {
    let mut reg = PeripheralRegistry::new();
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    // 先塞一个 RX 字节，好让下面那条 `read_clear` 有东西可清。
    uart.receive(b'Z');
    reg.register(uart).unwrap();
    {
        let u = reg.get_mut("UART0").expect("registered");
        assert_eq!(u.id(), "UART0");
    }
    // Mutate via the typed trait object: write THR → 进 TX 捕获环。
    // `get_mut` hands out a `RefMut` guard (the registry owns the
    // instance behind a `RefCell`), so the binding must be mutable to
    // reach the `&mut self` methods.
    let mut u = reg.get_mut("UART0").expect("registered");
    let _ = u.write(0x0, WriteSize::Byte, 0x41);
    // Verify the mutation took effect: 捕获环的队头就是那个字节。
    let exported = u.session_state().expect("the UART participates");
    let PeripheralSessionState::Uart(state) = exported else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(state.tx, Some(0x41), "the byte must be in the capture ring");
    // M-15 review #2: read_clear drains the read-destructive state — in
    // the ns16550 layout the **RHR @ 0x0** is the only such register.
    let read_back = u.read_clear(0x0, WriteSize::Byte).unwrap();
    assert_eq!(read_back, u32::from(b'Z'));
    let exported = u.session_state().expect("the UART participates");
    let PeripheralSessionState::Uart(state) = exported else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(state.rx, None, "the RX FIFO must have been drained");
}

#[test]
fn peripheral_trait_direct_re_export_is_resolvable() {
    // M-15 review #10: external code must be able to write
    // `m15_peripheral::Peripheral` (the previous `PeripheralTrait`
    // alias forced callers back to `riscv_sim_contract::port::Peripheral`).
    fn _accepts_peripheral(_p: &dyn Peripheral) {}
    let uart = Uart16550::new("UART0", 0, 9600, 0);
    _accepts_peripheral(&uart);
}

// ---------------------------------------------------------------------------
// Uart16550
// ---------------------------------------------------------------------------

#[test]
fn uart_register_window_layout() {
    let uart = Uart16550::new("UART0", 0x2000_0000, 115_200, 5);
    assert_eq!(uart.id(), "UART0");
    assert_eq!(uart.base_addr(), 0x2000_0000);
    assert_eq!(uart.size(), uart::UART_WINDOW);
    // ⚠️ **v0.23**：ns16550 的寄存器文件是 **8 个字节寄存器**
    // （`0x0..=0x7`），窗口必须正好盖住它们。
    assert_eq!(uart::UART_WINDOW, 0x8);
    // Initial LSR: THRE(0x02) | TX_IDLE(0x20) | TEMT(0x40) = 0x62;
    // RX 是空的 ⇒ LSR_RX_VALID 清。
    let lsr = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(lsr, 0x62);
    assert_eq!(lsr & u32::from(uart::lsr_bits::RX_VALID), 0);
}

#[test]
fn uart_transmit_then_take_tx_drains_byte() {
    // M-15 review #1: previously the TX byte was unreachable. Now
    // `take_tx` returns the byte and clears the busy state.
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'A'));
    assert!(
        !uart.tx_pin(),
        "TX pin busy while the capture ring holds a byte"
    );

    let drained = uart.take_tx();
    assert_eq!(drained, Some(b'A'));
    assert!(uart.tx_pin(), "TX pin idle after drain");
    // ⚠️ **v0.23**：状态寄存器是 **LSR @ 0x5**（不是旧布局的 0x4）。
    let lsr = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(lsr & u32::from(uart::lsr_bits::TX_IDLE), 0x20);
}

#[test]
fn uart_transmit_overflow_latches_tx_overrun_bit() {
    // M-15 review #4: TX overflow used to set RX_OVERRUN. Now
    // TX_OVERRUN is a separate bit.
    // ⚠️ **v0.23**：TX 不再是 1 字节 FIFO，而是一个**可配置容量的捕获
    // 环**；环满时的策略是「丢**最新**的那个字节」+ 在 **LSR 的 bit3**
    // 锁存 overrun（RX overrun 仍在 bit2，两者是分开的位）。
    let mut uart = Uart16550::new_with_tx_capacity("UART0", 0, 9600, 0, 2);
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'A'));
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'B'));
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'C')); // overflow
    let lsr = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(
        lsr & u32::from(uart::lsr_bits::TX_OVERRUN),
        0x08,
        "the TX overrun is latched in LSR bit 3"
    );
    assert_eq!(
        lsr & u32::from(uart::lsr_bits::OE),
        0,
        "the RX overrun is a separate bit and must stay clear"
    );
    // 丢的是**最新**的那个字节 ⇒ 留下的是最早的两个。
    assert_eq!(uart.drain_tx(), vec![b'A', b'B']);
}

#[test]
fn uart_status_clear_is_w1c() {
    // M-15 review #14: W1C (write-1-to-clear). Writing 0 must NOT
    // clear the overrun bit; writing the bit's value must.
    // ⚠️ **v0.23**：状态寄存器是 ns16550 的 **LSR @ 0x5**，RX overrun
    // 在 bit2。
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    // ⚠️ **v0.24**：RX FIFO 有 16 字节深 ⇒ 要触发 overrun 必须**先填满**。
    for i in 0..uart::RX_FIFO_CAPACITY {
        uart.receive(i as u8);
    }
    uart.receive(0xFF); // triggers RX overrun
    let s = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(s & u32::from(uart::lsr_bits::OE), 0x04);

    // Write 0 — bit stays.
    let _ = uart.write(0x5, WriteSize::Byte, 0);
    let s = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(s & u32::from(uart::lsr_bits::OE), 0x04);

    // Write the OE bit — clears the RX overrun.
    let _ = uart.write(0x5, WriteSize::Byte, u32::from(uart::lsr_bits::OE));
    let s = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(s & u32::from(uart::lsr_bits::OE), 0);
}

#[test]
fn uart_read_clear_drains_rx_fifo() {
    // M-15 review #2: RX FIFO read used to be non-destructive.
    // `read_clear` (default-trait method) takes the byte out.
    // ⚠️ **v0.23**：RX 寄存器是 ns16550 的 **RHR @ 0x0**，状态在
    // **LSR @ 0x5**。
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    uart.receive(b'Z');
    let first = uart.read_clear(0x0, WriteSize::Byte).unwrap();
    assert_eq!(first, u32::from(b'Z'));
    // LSR_RX_VALID must clear.
    let lsr = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(lsr & u32::from(uart::lsr_bits::RX_VALID), 0);
    // Second read returns 0 (FIFO empty), no overrun latched.
    let second = uart.read_clear(0x0, WriteSize::Byte).unwrap();
    assert_eq!(second, 0);
    let lsr = uart.read(0x5, WriteSize::Byte).unwrap();
    assert_eq!(lsr & u32::from(uart::lsr_bits::OE), 0);
}

#[test]
fn uart_ps_per_bit_matches_baud_rate_and_floors_at_one() {
    let uart = Uart16550::new("UART0", 0, 115_200, 0);
    assert_eq!(uart.ps_per_bit(), 8_680); // 1e9 / 115_200 = 8680.55 → 8680
    let uart = Uart16550::new("UART0", 0, 9_600, 0);
    assert_eq!(uart.ps_per_bit(), 104_166); // 1e9 / 9_600 = 104166.66 → 104166
                                            // M-15 review #15: pathological baud > 1e9 must not return 0.
    let uart = Uart16550::new("UART0", 0, 2_000_000_000, 0);
    assert_eq!(uart.ps_per_bit(), 1);
}

#[test]
fn uart_undefined_offset_returns_e_per_02() {
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    let err = uart.write(0x10, WriteSize::Byte, 0).unwrap_err();
    assert_eq!(err.scenario, 2);
    let err = uart.read(0x10, WriteSize::Byte).unwrap_err();
    assert_eq!(err.scenario, 2);
}

#[test]
fn uart_sub_register_byte_access() {
    // M-15 review #5: reading offset 0x1 (within the DATA word) must
    // return the high byte of RX FIFO, not an undefined-offset error.
    let mut uart = Uart16550::new("UART0", 0, 9600, 0);
    uart.receive(0xAB);
    assert_eq!(uart.read(0x0, WriteSize::Byte).unwrap(), 0xAB);
    assert_eq!(uart.read(0x1, WriteSize::Byte).unwrap(), 0x00);
    assert_eq!(uart.read(0x2, WriteSize::Byte).unwrap(), 0x00);
    assert_eq!(uart.read(0x3, WriteSize::Byte).unwrap(), 0x00);
    // Halfword reads span two bytes.
    assert_eq!(uart.read(0x0, WriteSize::Half).unwrap(), 0x00AB);
    // Sub-register read on STATUS register (offset 0x4 word).
    let status = uart.read(0x4, WriteSize::Word).unwrap();
    let lo = status & 0xFFFF;
    assert_eq!(uart.read(0x4, WriteSize::Half).unwrap(), lo);
    assert_eq!(
        uart.read(0x5, WriteSize::Byte).unwrap(),
        (status >> 8) & 0xFF
    );
}

// ---------------------------------------------------------------------------
// GpioBank
// ---------------------------------------------------------------------------

#[test]
fn gpio_set_directions_and_out_latch() {
    let mut gpio = GpioBank::new("GPIO0", 0x4000_0000);
    gpio.set_directions(0b1100); // pins 2,3 = output
    assert_eq!(gpio.read(0x0, WriteSize::Byte).unwrap(), 0b1100);
    let _ = gpio.write(0x4, WriteSize::Byte, 0b1010);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0b1010);
}

#[test]
fn gpio_toggle_uses_documented_pin_bit_semantics() {
    // M-15 review #3: `1 << pin` flips that pin's output. Previous
    // semantics required bit 5 to be set, which conflicted with
    // documentation and confused firmware authors.
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0);

    // value = 1 << 3 → flip pin 3
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 3);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1 << 3);

    // Toggle again → clears.
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 3);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 0);

    // value = 1 << 0 → flip pin 0
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
}

#[test]
fn gpio_external_inputs_latch_pending_irqs_only_for_input_pins() {
    // M-15 review #7: previously `directions` was stored but never
    // consulted. Now external-input edge IRQs latch only for pins
    // configured as input (direction bit = 0).
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 11); // input pin
    gpio.set_irq_line_for_pin(1, 12); // output pin (per next line)
    gpio.set_directions(0b10); // pin 0 = input, pin 1 = output

    // Rising edge on pin 0 (input) → IRQ latched.
    gpio.set_external_inputs(0b01);
    assert_eq!(gpio.take_pending_irqs(), vec![11]);

    // Rising edge on pin 1 (output) → NO IRQ (output pin transitions
    // come from firmware via OUT/TOGGLE, not from the wire).
    gpio.set_external_inputs(0b11);
    assert!(gpio.take_pending_irqs().is_empty());
}

#[test]
fn gpio_out_write_latches_irq_on_each_edge() {
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(2, 13);
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    // First OUT write: pin 2 rises 0 → 1.
    let _ = gpio.write(0x4, WriteSize::Byte, 1 << 2);
    assert_eq!(gpio.take_pending_irqs(), vec![13]);
    // Second OUT write: pin 2 stays high → no edge.
    let _ = gpio.write(0x4, WriteSize::Byte, 1 << 2);
    assert!(gpio.take_pending_irqs().is_empty());
    // Third OUT write: pin 2 falls 1 → 0.
    let _ = gpio.write(0x4, WriteSize::Byte, 0);
    assert_eq!(gpio.take_pending_irqs(), vec![13]);
}

#[test]
fn gpio_take_pending_irqs_dedupes_by_line() {
    // M-15 review #13: multiple pins binding the same IRQ line should
    // not produce duplicate entries in `take_pending_irqs`.
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 11);
    gpio.set_irq_line_for_pin(1, 11);
    gpio.set_irq_line_for_pin(2, 12);
    // Trigger edges on pins 0 and 1 (both → line 11) and pin 2 (→ line 12).
    let _ = gpio.write(0x4, WriteSize::Byte, 0b111);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&11));
    assert!(pending.contains(&12));
}

#[test]
fn gpio_in_write_is_a_no_op_for_firmware_driven_lines() {
    let mut gpio = GpioBank::new("GPIO0", 0);
    let before = gpio.read(0x8, WriteSize::Byte).unwrap();
    let _ = gpio.write(0x8, WriteSize::Byte, 0xFF);
    let after = gpio.read(0x8, WriteSize::Byte).unwrap();
    assert_eq!(before, after);
}

#[test]
fn gpio_in_reflects_output_latch_on_output_direction_pins() {
    // M-15 review N2: firmware writing OUT to drive a pin must be
    // able to read the level back via IN. Previously IN returned
    // only `self.inputs`, breaking the "data direction register
    // controls who drives the pin" mental model. Required for
    // E2E-01 N5 "GPIO 翻转可见" + M-18 波形 pipeline to observe
    // firmware-driven edges.
    let mut gpio = GpioBank::new("GPIO0", 0);
    // Pins 0,1 = output; pins 2,3 = input.
    gpio.set_directions(0b0011);
    // Drive OUT[0]=1, OUT[1]=0.
    let _ = gpio.write(0x4, WriteSize::Byte, 0b0001);
    // Set external inputs for the input pins.
    gpio.set_external_inputs(0b1100);
    let in_reg = gpio.read(0x8, WriteSize::Byte).unwrap();
    // pin 0 (out, 1) + pin 2 (in, 1) + pin 3 (in, 1) = 0b1101
    assert_eq!(in_reg, 0b1101);
}

#[test]
fn gpio_take_pending_irqs_dedupes_lines_in_upper_half() {
    // M-15 review N3: previously the dedup bitmap only covered
    // `line < 16`; lines in [16, 32) bypassed dedup entirely. Now
    // the bitmap covers `line < 32` (full u16 space minus 0xFFFF).
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_irq_line_for_pin(0, 17);
    gpio.set_irq_line_for_pin(1, 17);
    gpio.set_irq_line_for_pin(2, 31);
    // Trigger edges on pins 0 and 1 (both → line 17) and pin 2 (→ line 31).
    let _ = gpio.write(0x4, WriteSize::Byte, 0b111);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&17));
    assert!(pending.contains(&31));
}

#[test]
fn gpio_in_reflects_input_pins_and_output_latch_simultaneously() {
    // Edge case for N2: same byte mixes input + output pins; IN
    // must return the bitwise merge.
    let mut gpio = GpioBank::new("GPIO0", 0);
    // pin 0 = output, pin 1 = input.
    gpio.set_directions(0b01);
    let _ = gpio.write(0x4, WriteSize::Byte, 0b01); // OUT[0] = 1
    gpio.set_external_inputs(0b10); // external drives pin 1 high
    let in_reg = gpio.read(0x8, WriteSize::Byte).unwrap();
    // pin 0 (out=1) + pin 1 (in=1) = 0b11
    assert_eq!(in_reg, 0b11);
}

#[test]
fn reg_helpers_are_dry() {
    // M-15 review N1: smoke test that `super::reg` exports the
    // expected helpers with the review #5 semantics. If the dedup
    // regresses (e.g. someone re-inlines the helpers in one of the
    // three peripheral modules), this test will still pass — but
    // its existence makes the DRY surface explicit.
    use super::reg::{extract_subword, register_offset};

    assert_eq!(register_offset(0x1), 0x0);
    assert_eq!(register_offset(0x4), 0x4);
    assert_eq!(register_offset(0x7), 0x4);

    // Word at offset 0x1 should yield the byte at position 1.
    let word = 0xAABBCCDD;
    assert_eq!(extract_subword(word, 0x0, WriteSize::Byte), 0xDD);
    assert_eq!(extract_subword(word, 0x1, WriteSize::Byte), 0xCC);
    assert_eq!(extract_subword(word, 0x2, WriteSize::Byte), 0xBB);
    assert_eq!(extract_subword(word, 0x3, WriteSize::Byte), 0xAA);
    assert_eq!(extract_subword(word, 0x0, WriteSize::Half), 0xCCDD);
    assert_eq!(extract_subword(word, 0x2, WriteSize::Half), 0xAABB);
    assert_eq!(extract_subword(word, 0x0, WriteSize::Word), word);
}

#[test]
fn gpio_sub_register_byte_access() {
    // M-15 review #5: register-internal byte reads must not return
    // E_PER-02 (offset "not defined").
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0x0403_0201);
    // Word reads at offset 0x0 / 0x1 / 0x2 / 0x3 all return same word.
    let w0 = gpio.read(0x0, WriteSize::Word).unwrap();
    assert_eq!(w0, 0x0403_0201);
    let w1 = gpio.read(0x1, WriteSize::Word).unwrap();
    assert_eq!(w1, 0x0403_0201);
    // Byte reads extract the corresponding byte.
    assert_eq!(gpio.read(0x0, WriteSize::Byte).unwrap(), 0x01);
    assert_eq!(gpio.read(0x1, WriteSize::Byte).unwrap(), 0x02);
    assert_eq!(gpio.read(0x2, WriteSize::Byte).unwrap(), 0x03);
    assert_eq!(gpio.read(0x3, WriteSize::Byte).unwrap(), 0x04);
}

// ---------------------------------------------------------------------------
// TimerRiscv
// ---------------------------------------------------------------------------

#[test]
fn timer_read_split_between_lo_and_hi_halves() {
    // M-15 review #8: mtime is read-only (engine-driven via tick).
    // We set mtime by ticking the timer to the target value.
    let mut tim = TimerRiscv::new("TIM0", 0, 9);
    tim.tick(0x1122_3344_5566_7788).unwrap();
    assert_eq!(tim.read(0x0, WriteSize::Word).unwrap(), 0x5566_7788);
    assert_eq!(tim.read(0x4, WriteSize::Word).unwrap(), 0x1122_3344);

    // mtimecmp is firmware-writable in either order.
    let _ = tim.write(0x8, WriteSize::Word, 0xDEAD_BEEF);
    let _ = tim.write(0xC, WriteSize::Word, 0x0000_0001);
    assert_eq!(tim.mtimecmp(), 0x0000_0001_DEAD_BEEF);
}

#[test]
fn timer_mtime_writes_from_firmware_are_silently_ignored() {
    // M-15 review #8: mtime is owned by the engine. Firmware writes
    // to MTIME_LO / MTIME_HI must be no-ops (no error, but no effect).
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    tim.tick(0xAAAA).unwrap();
    let _ = tim.write(0x0, WriteSize::Word, 0xFFFF_FFFF);
    let _ = tim.write(0x4, WriteSize::Word, 0xFFFF_FFFF);
    // mtime must still be what tick set it to.
    assert_eq!(tim.mtime(), 0xAAAA);
}

#[test]
fn timer_tick_advances_mtime_and_arms_irq_at_compare() {
    let mut tim = TimerRiscv::new("TIM0", 0, 7);
    tim.set_mtimecmp(1_000_000); // 1 ms

    // Advance to mtime = 500µs — no IRQ yet.
    tim.tick(500_000).unwrap();
    assert_eq!(tim.mtime(), 500_000);
    assert_eq!(tim.take_pending_irq(), None);

    // Advance past 1 ms — IRQ arm.
    tim.tick(2_000_000).unwrap();
    assert_eq!(tim.mtime(), 2_000_000);
    assert_eq!(tim.take_pending_irq(), Some(7));
    // IRQ is consumed (sticky arm only after re-comparison).
    assert_eq!(tim.take_pending_irq(), None);
}

#[test]
fn timer_undefined_offset_returns_e_per_02() {
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    let err = tim.read(0x20, WriteSize::Byte).unwrap_err();
    assert_eq!(err.scenario, 2);
    let err = tim.write(0x20, WriteSize::Byte, 0).unwrap_err();
    assert_eq!(err.scenario, 2);
}

#[test]
fn timer_sub_register_byte_access() {
    // M-15 review #5: byte/half reads inside a word register.
    let mut tim = TimerRiscv::new("TIM0", 0, 0);
    tim.tick(0xAABBCCDD).unwrap();
    assert_eq!(tim.read(0x0, WriteSize::Byte).unwrap(), 0xDD);
    assert_eq!(tim.read(0x1, WriteSize::Byte).unwrap(), 0xCC);
    assert_eq!(tim.read(0x2, WriteSize::Byte).unwrap(), 0xBB);
    assert_eq!(tim.read(0x3, WriteSize::Byte).unwrap(), 0xAA);
    // Halfword spans 2 bytes within the same word.
    assert_eq!(tim.read(0x0, WriteSize::Half).unwrap(), 0xCCDD);
    assert_eq!(tim.read(0x2, WriteSize::Half).unwrap(), 0xAABB);
}

// ---------------------------------------------------------------------------
// Cross-peripheral test: UART TX → host bridge → GPIO LED
// ---------------------------------------------------------------------------

#[test]
fn uart_byte_drives_gpio_led_pin() {
    let mut uart = Uart16550::new("UART0", 0, 9600, 4);
    let mut gpio = GpioBank::new("GPIO0", 0);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(0, 11);

    // The host bridge translates one UART TX byte into a LED pin toggle:
    // a single-shot "echo back" that demonstrates how peripheral
    // outputs compose without M-14 wiring yet.
    let _ = uart.write(0x0, WriteSize::Byte, u32::from(b'T'));
    assert!(!uart.tx_pin(), "TX pin should reflect busy state");

    // Bridge: drain UART TX byte and toggle GPIO pin 0 (the LED).
    assert_eq!(uart.take_tx(), Some(b'T'));
    assert!(uart.tx_pin());

    // Toggle pin 0 with doc semantics: 1<<0.
    let _ = gpio.write(0xC, WriteSize::Word, 1 << 0);
    assert_eq!(gpio.read(0x4, WriteSize::Byte).unwrap(), 1);
    let pending = gpio.take_pending_irqs();
    assert_eq!(pending, vec![11]);
}

// ---------------------------------------------------------------------------
// Session snapshot (STAGE-3 §3.3) — segment 4 (peripheral internal state).
// ---------------------------------------------------------------------------

#[test]
fn uart_session_state_round_trips_every_field() {
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    uart.write(0x0, WriteSize::Byte, u32::from(b'A'))
        .expect("tx");
    uart.receive(b'Z');
    uart.tick(1_234).expect("tick");

    let exported = uart
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(state) = exported.clone() else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(state.id, "UART0");
    assert_eq!(
        state.tx,
        Some(b'A'),
        "the TX capture ring head must be carried"
    );
    assert_eq!(state.rx, Some(b'Z'), "the RX byte must be carried");
    assert_eq!(state.irq_line, 4);
    assert_eq!(
        state.now_ps, 1_234,
        "the peripheral's virtual time must be carried"
    );
    assert_eq!(
        state.status, 0x63,
        "the LSR must be carried: RX_VALID(0x01) | THRE(0x02) | TX_IDLE(0x20) | TEMT(0x40)"
    );

    // Restore into a *fresh* UART → the exported state must come back
    // field for field. A dropped field (status / irq_line / now_ps)
    // shows up here, not as a silent divergence later on.
    let mut fresh = Uart16550::new("UART0", 0x1000, 9600, 0);
    fresh
        .restore_session_state(&exported)
        .expect("the UART must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
}

#[test]
fn gpio_session_state_round_trips_every_field() {
    let mut gpio = GpioBank::new("GPIO0", 0x2000);
    gpio.set_directions(0xFFFF_FFFF);
    gpio.set_irq_line_for_pin(3, 12);
    gpio.set_irq_line_for_pin(31, 20);
    gpio.write(0x4, WriteSize::Word, 0x0000_0008).expect("out");
    gpio.set_external_inputs(0x0000_00F0);
    gpio.tick(4_321).expect("tick");

    let exported = gpio
        .session_state()
        .expect("the GPIO participates in the session state");
    let PeripheralSessionState::Gpio(state) = exported.clone() else {
        panic!("a GPIO must export a GPIO state");
    };
    assert_eq!(state.id, "GPIO0");
    assert_eq!(state.directions, 0xFFFF_FFFF);
    assert_eq!(state.outputs, 0x8);
    assert_eq!(state.inputs, 0xF0);
    assert_eq!(
        state.pending_irqs, 0x8,
        "the sticky pending vector is carried"
    );
    assert_eq!(state.now_ps, 4_321);
    assert_eq!(
        state.irq_lines.len(),
        GPIO_PIN_COUNT,
        "one IRQ line per pin must be carried"
    );
    assert_eq!(state.irq_lines[3], 12, "the lines are in **pin order**");
    assert_eq!(state.irq_lines[31], 20, "the lines are in **pin order**");
    assert_eq!(
        state.irq_lines[0], 0xFFFF,
        "an unconfigured pin keeps its default"
    );

    let mut fresh = GpioBank::new("GPIO0", 0x2000);
    fresh
        .restore_session_state(&exported)
        .expect("the GPIO must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
}

#[test]
fn timer_session_state_round_trips_every_field() {
    let mut tim = TimerRiscv::new("TIM0", 0x3000, 7);
    tim.set_mtimecmp(9_000);
    tim.tick(5_000).expect("tick");

    let exported = tim
        .session_state()
        .expect("the timer participates in the session state");
    let PeripheralSessionState::Timer(state) = exported.clone() else {
        panic!("a timer must export a timer state");
    };
    assert_eq!(state.id, "TIM0");
    assert_eq!(state.irq_line, 7);
    assert_eq!(state.mtime, 5_000);
    assert_eq!(state.mtimecmp, 9_000);

    let mut fresh = TimerRiscv::new("TIM0", 0x3000, 0);
    fresh
        .restore_session_state(&exported)
        .expect("the timer must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported),
        "the round-trip must be field-for-field equal"
    );
    assert_eq!(fresh.mtime(), 5_000, "mtime must be restored");
    assert_eq!(fresh.mtimecmp(), 9_000, "mtimecmp must be restored");
}

#[test]
fn restoring_a_state_of_the_wrong_kind_is_rejected() {
    // A UART must not swallow a GPIO state: the kinds are not
    // interchangeable, and applying one would move another peripheral's
    // registers into this one.
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let gpio_state = PeripheralSessionState::Gpio(GpioStateDto {
        id: "UART0".to_string(),
        directions: 0,
        outputs: 0,
        inputs: 0,
        irq_lines: vec![0xFFFF; GPIO_PIN_COUNT],
        pending_irqs: 0,
        now_ps: 0,
    });
    let err = uart
        .restore_session_state(&gpio_state)
        .expect_err("a UART must reject a GPIO state");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

#[test]
fn restoring_a_state_that_names_another_peripheral_is_rejected() {
    // The id inside the payload is the peripheral's own — a state that
    // names a different peripheral was handed to the wrong instance.
    let mut uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let foreign = PeripheralSessionState::Uart(UartStateDto {
        id: "UART1".to_string(),
        tx: Some(b'X'),
        rx: None,
        status: 0,
        irq_line: 1,
        now_ps: 0,

        ..UartStateDto::default()
    });
    let err = uart
        .restore_session_state(&foreign)
        .expect_err("a state of another peripheral must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn gpio_rejects_a_state_with_the_wrong_number_of_irq_lines() {
    // A short list would leave the other pins' lines undefined; a long
    // one would carry lines that do not exist. Neither is repaired
    // silently.
    let mut gpio = GpioBank::new("GPIO0", 0x2000);
    let bad = PeripheralSessionState::Gpio(GpioStateDto {
        id: "GPIO0".to_string(),
        directions: 0,
        outputs: 0,
        inputs: 0,
        irq_lines: vec![1, 2, 3],
        pending_irqs: 0,
        now_ps: 0,
    });
    let err = gpio
        .restore_session_state(&bad)
        .expect_err("a malformed GPIO state must be rejected");
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **一个数，一个落点**：GPIO 导出的每引脚 IRQ 线**条数**必须等于
/// **契约层**导出的引脚数。
///
/// M-15 的 `PIN_COUNT` **引用**契约层的 `GPIO_PIN_COUNT`，不是自己再写
/// 一份 `32`。这条用例钉的是那个**引用关系**：哪天 M-15 改成写死一个
/// `32`、而契约层的常数动了，导出的条数与契约层要求的对不上，载荷再也
/// 过不了自己的校验（而 GPIO 自己的校验也会拒它）——**两个真相**。
#[test]
fn the_gpio_exports_exactly_one_irq_line_per_contract_pin() {
    let gpio = GpioBank::new("GPIO0", 0x2000);
    let exported = gpio
        .session_state()
        .expect("the GPIO participates in the session state");
    let PeripheralSessionState::Gpio(s) = exported else {
        panic!("a GPIO must export a GPIO state");
    };
    assert_eq!(
        s.irq_lines.len(),
        GPIO_PIN_COUNT,
        "GPIO 导出的 IRQ 线条数必须等于契约层的引脚数——M-15 不得自己写一份 32"
    );
}

#[test]
fn registry_export_state_groups_by_kind_and_orders_by_id() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART1", 0x1000, 9600, 1))
        .expect("uart1");
    reg.register(GpioBank::new("GPIO0", 0x2000)).expect("gpio");
    reg.register(Uart16550::new("UART0", 0x3000, 9600, 2))
        .expect("uart0");
    reg.register(TimerRiscv::new("TIM0", 0x4000, 3))
        .expect("timer");

    let state = reg.export_state();
    assert_eq!(state.uarts.len(), 2);
    assert_eq!(state.uarts[0].id, "UART0", "the groups are ordered by id");
    assert_eq!(state.uarts[1].id, "UART1");
    assert_eq!(state.gpios.len(), 1);
    assert_eq!(state.gpios[0].id, "GPIO0");
    assert_eq!(state.timers.len(), 1);
    assert_eq!(state.timers[0].id, "TIM0");
}

#[test]
fn registry_load_state_restores_every_peripheral() {
    let mut source = PeripheralRegistry::new();
    source
        .register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    source
        .register(GpioBank::new("GPIO0", 0x2000))
        .expect("gpio");
    source
        .register(TimerRiscv::new("TIM0", 0x3000, 7))
        .expect("timer");
    // Give them some state through the register interface.
    source
        .get_mut("UART0")
        .expect("uart")
        .write(0x0, WriteSize::Byte, u32::from(b'Q'))
        .expect("tx");
    source
        .get_mut("GPIO0")
        .expect("gpio")
        .write(0x0, WriteSize::Word, 0xF0)
        .expect("dir");
    source
        .get_mut("TIM0")
        .expect("timer")
        .write(0x8, WriteSize::Word, 4_242)
        .expect("mtimecmp");

    let state = source.export_state();

    // A fresh registry with the same ids — the restore must reproduce
    // the captured state field for field.
    let mut target = PeripheralRegistry::new();
    target
        .register(Uart16550::new("UART0", 0x1000, 9600, 0))
        .expect("uart");
    target
        .register(GpioBank::new("GPIO0", 0x2000))
        .expect("gpio");
    target
        .register(TimerRiscv::new("TIM0", 0x3000, 0))
        .expect("timer");
    target.load_state(&state).expect("load_state");

    assert_eq!(
        target.export_state(),
        state,
        "the restored state must be field-for-field equal"
    );
}

#[test]
fn registry_load_state_rejects_an_unknown_id() {
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    // The payload describes the **live** peripheral **and** an unknown
    // one: the "every live peripheral is described" check is satisfied,
    // so only the "every id in the payload is live" check can catch the
    // unknown id. (Two checks, two directions — a test that only had
    // the unknown entry would be satisfied by either of them.)
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,

                ..UartStateDto::default()
            },
            UartStateDto {
                id: "NOPE".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,

                ..UartStateDto::default()
            },
        ],
        ..Default::default()
    };
    let err = reg
        .load_state(&state)
        .expect_err("an unknown peripheral id must be rejected");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn registry_load_state_writes_nothing_when_one_id_is_unknown() {
    // "Validate before apply" (STAGE-3 §5): the valid entry must not
    // be applied when another entry is rejected.
    let mut reg = PeripheralRegistry::new();
    reg.register(Uart16550::new("UART0", 0x1000, 9600, 4))
        .expect("uart");
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: Some(b'Z'),
                rx: None,
                status: 0,
                irq_line: 9,
                now_ps: 42,

                ..UartStateDto::default()
            },
            UartStateDto {
                id: "GHOST".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,

                ..UartStateDto::default()
            },
        ],
        ..Default::default()
    };
    let err = reg
        .load_state(&state)
        .expect_err("the unknown id must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);

    let restored = reg.export_state();
    assert_eq!(
        restored.uarts[0].irq_line, 4,
        "the valid entry must not have been applied"
    );
    assert_eq!(restored.uarts[0].now_ps, 0);
}

#[test]
fn collect_session_state_reads_the_live_bus_instances() {
    // The host binds its peripherals straight onto the bus (it keeps no
    // registry) — the free functions are its entry point.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    let tim = share(TimerRiscv::new("TIM0", 0x3000, 5));
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, uart), (0x2000, gpio), (0x3000, tim)];

    let state = collect_session_state(&instances);
    assert_eq!(state.uarts.len(), 1);
    assert_eq!(state.uarts[0].id, "UART0");
    assert_eq!(state.gpios.len(), 1);
    assert_eq!(state.gpios[0].id, "GPIO0");
    assert_eq!(state.timers.len(), 1);
    assert_eq!(state.timers[0].id, "TIM0");
}

#[test]
fn collect_session_state_orders_each_group_by_id() {
    // The bus order is a *placement*, not an identity: two UARTs bound
    // in the "wrong" order must still be exported in id order.
    let uart1 = share(Uart16550::new("UART1", 0x1000, 9600, 1));
    let uart0 = share(Uart16550::new("UART0", 0x2000, 9600, 2));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart1), (0x2000, uart0)];

    let state = collect_session_state(&instances);
    assert_eq!(state.uarts.len(), 2);
    assert_eq!(state.uarts[0].id, "UART0", "the group is ordered by id");
    assert_eq!(state.uarts[1].id, "UART1");
}

#[test]
fn collect_session_state_releases_its_borrows() {
    // The collection must not hold a borrow across the loop: the
    // restore path takes a **mutable** borrow of the very same cell,
    // and holding both at once would panic.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, Rc::clone(&uart))];
    let _state = collect_session_state(&instances);

    let mut periph = uart.borrow_mut();
    periph
        .write(0x0, WriteSize::Byte, u32::from(b'X'))
        .expect("the borrow must be free");
}

#[test]
fn restore_session_states_round_trips_the_bus_instances() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    {
        let mut p = uart.borrow_mut();
        p.write(0x0, WriteSize::Byte, u32::from(b'K')).expect("tx");
    }
    {
        let mut p = gpio.borrow_mut();
        p.write(0x0, WriteSize::Word, 0x0F0F_0F0F).expect("dir");
    }
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, Rc::clone(&uart)), (0x2000, Rc::clone(&gpio))];
    let state = collect_session_state(&instances);

    // Fresh instances, same ids.
    let fresh_uart = share(Uart16550::new("UART0", 0x1000, 9600, 0));
    let fresh_gpio = share(GpioBank::new("GPIO0", 0x2000));
    let fresh: Vec<(u32, SharedPeripheral)> = vec![(0x1000, fresh_uart), (0x2000, fresh_gpio)];
    restore_session_states(&fresh, &state).expect("restore");

    assert_eq!(
        collect_session_state(&fresh),
        state,
        "the restored state must be field-for-field equal"
    );
}

#[test]
fn restore_session_states_rejects_an_unknown_id() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart)];
    // The payload describes the **live** peripheral **and** an unknown
    // one, so the "every live peripheral is described" check is
    // satisfied and only the "every id in the payload is live" check
    // can catch the unknown id.
    let state = M15PeriphDto {
        uarts: vec![
            UartStateDto {
                id: "UART0".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,

                ..UartStateDto::default()
            },
            UartStateDto {
                id: "GHOST".to_string(),
                tx: None,
                rx: None,
                status: 0,
                irq_line: 0,
                now_ps: 0,

                ..UartStateDto::default()
            },
        ],
        ..Default::default()
    };
    let err = restore_session_states(&instances, &state)
        .expect_err("an unknown peripheral id must be rejected");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

#[test]
fn restore_session_states_rejects_a_payload_that_misses_a_live_peripheral() {
    // The other direction: a live peripheral that the payload does not
    // describe would silently keep its initial state — the restored
    // session would differ from the captured one.
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, uart)];
    let err = restore_session_states(&instances, &M15PeriphDto::default())
        .expect_err("a payload that misses a live peripheral must be rejected");
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

// ---------------------------------------------------------------------------
// 「先校验后写入」的第二层（STAGE-3 §3.3）—— 只读的接受性检查。
// ---------------------------------------------------------------------------

/// ⚠️ **核心用例**：一个**合法**的 UART + 一个**畸形**的 GPIO
/// （`irq_lines` 只有 3 条）→ 回灌必须**报错**，且 **UART 的状态一字
/// 未改**。
///
/// ## 这条用例在钉什么
///
/// 载荷的写入顺序是「先 UART、后 GPIO」（载荷按类型分组）。如果畸形
/// 只在**写入路径**里被发现，那么：
///
/// 1. UART 被**改写**（`tx` / `irq_line` / `now_ps` 全变成载荷里的值）；
/// 2. 写 GPIO 时才发现 `irq_lines` 的条数不对 → 返回 `Err`；
/// 3. 函数返回 `Err`，**会话已经坏了**——半个应用完的快照比被拒的快照
///    更糟（STAGE-3 §5）。
///
/// 所以「先全量**只读**校验，再写入」必须是真的：校验不通过时，**一个
/// 字节都不许写**。
///
/// ## 判别式
///
/// 断言落在**具体字段值**上（`tx` 的字节、`irq_line`、`now_ps`），不是
/// 「没崩」：夹具里 UART 的活状态与载荷里的 UART 状态**逐字段不同**，
/// 所以「写过了」与「没写过」**可区分**。
#[test]
fn restore_session_states_writes_nothing_when_one_state_is_malformed() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    // 活 UART 的现场状态：TX FIFO 里有一个字节，IRQ 行是 4，时钟是 0。
    {
        let mut p = uart.borrow_mut();
        p.write(0x0, WriteSize::Byte, u32::from(b'K')).expect("tx");
    }
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, Rc::clone(&uart)), (0x2000, Rc::clone(&gpio))];

    // 载荷：一个**合法**的 UART（与活状态逐字段不同）+ 一个**畸形**的
    // GPIO（`irq_lines` 只有 3 条）。
    let state = M15PeriphDto {
        uarts: vec![UartStateDto {
            id: "UART0".to_string(),
            tx: Some(b'Z'),
            rx: None,
            status: 0,
            irq_line: 9,
            now_ps: 42,

            ..UartStateDto::default()
        }],
        gpios: vec![GpioStateDto {
            id: "GPIO0".to_string(),
            directions: 0xF0,
            outputs: 0xF0,
            inputs: 0,
            irq_lines: vec![1, 2, 3],
            pending_irqs: 0,
            now_ps: 0,
        }],
        timers: vec![],
        ..M15PeriphDto::default()
    };

    let err = restore_session_states(&instances, &state).expect_err("畸形的 GPIO 状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
    assert_eq!(
        err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE,
        "必须是 E_PER-04（这个外设不接受这份状态）"
    );

    // ⚠️ **UART 的状态一字未改**——它是**合法**的那一个，却因为别人
    // 畸形而被连坐。
    let after = collect_session_state(&instances);
    assert_eq!(
        after.uarts[0].tx,
        Some(b'K'),
        "合法的 UART 不许被写过（载荷里是 b'Z'）"
    );
    assert_eq!(
        after.uarts[0].irq_line, 4,
        "合法的 UART 不许被写过（载荷里是 9）"
    );
    assert_eq!(
        after.uarts[0].now_ps, 0,
        "合法的 UART 不许被写过（载荷里是 42）"
    );

    // 畸形的那个自然也没写。
    assert_eq!(after.gpios[0].outputs, 0, "畸形的 GPIO 也不许被写过");
    assert_eq!(after.gpios[0].directions, 0);
}

/// ⚠️ **只读**预检（[`validate_session_states`]）与写入路径**同口径**，
/// 而且**一个字节都不写**。
///
/// 宿主在碰总线**之前**跑这个预检——恢复链的第 1 / 2 步（`load_map` /
/// `load_pages`）已经写过内存了，第 3 步才发现外设状态畸形的话，失败
/// 后的会话是「内存来自载荷、寄存器来自恢复前」的**拼接体**。
#[test]
fn the_read_only_preflight_rejects_a_malformed_state_and_writes_nothing() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    {
        let mut p = uart.borrow_mut();
        p.write(0x0, WriteSize::Byte, u32::from(b'K')).expect("tx");
    }
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, Rc::clone(&uart)), (0x2000, Rc::clone(&gpio))];

    let state = M15PeriphDto {
        uarts: vec![UartStateDto {
            id: "UART0".to_string(),
            tx: Some(b'Z'),
            rx: None,
            status: 0,
            irq_line: 9,
            now_ps: 42,

            ..UartStateDto::default()
        }],
        gpios: vec![GpioStateDto {
            id: "GPIO0".to_string(),
            directions: 0,
            outputs: 0,
            inputs: 0,
            irq_lines: vec![1, 2, 3],
            pending_irqs: 0,
            now_ps: 0,
        }],
        timers: vec![],
        ..M15PeriphDto::default()
    };

    let err =
        validate_session_states(&instances, &state).expect_err("畸形的 GPIO 状态必须被预检拒掉");
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);

    let after = collect_session_state(&instances);
    assert_eq!(after.uarts[0].tx, Some(b'K'), "预检不许写任何东西");
    assert_eq!(after.uarts[0].irq_line, 4);
    assert_eq!(after.uarts[0].now_ps, 0);
}

/// 预检**放行**合法载荷时，也**一个字节都不写**。
#[test]
fn the_read_only_preflight_accepts_a_well_formed_state_and_writes_nothing() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x1000, Rc::clone(&uart))];
    let state = collect_session_state(&instances);

    validate_session_states(&instances, &state).expect("合法的载荷必须通过预检");

    let after = collect_session_state(&instances);
    assert_eq!(
        after.uarts[0].irq_line, 4,
        "预检不许写任何东西（IRQ 行还是活实例的值）"
    );
}

/// ⚠️ **UART 自己的规则**（只有 UART 知道 STATUS 位的含义）：STATUS 里
/// 出现**未定义的位** → 拒。
///
/// 契约层查不了这个：它认识 UART 状态的**形状**，不认识 STATUS 的
/// **语义**。所以这一条必须由外设自己在**写入之前**拦下来。
#[test]
fn a_uart_state_with_undefined_status_bits_is_rejected_before_anything_is_written() {
    let uart = share(Uart16550::new("UART0", 0x1000, 9600, 4));
    let gpio = share(GpioBank::new("GPIO0", 0x2000));
    let instances: Vec<(u32, SharedPeripheral)> =
        vec![(0x1000, Rc::clone(&uart)), (0x2000, Rc::clone(&gpio))];

    let state = M15PeriphDto {
        uarts: vec![UartStateDto {
            id: "UART0".to_string(),
            tx: None,
            rx: None,
            // 0x8000 = 一个这个模型从来没有过的位。
            status: 0x8000,
            irq_line: 4,
            now_ps: 0,

            ..UartStateDto::default()
        }],
        gpios: vec![GpioStateDto {
            id: "GPIO0".to_string(),
            directions: 0,
            outputs: 0,
            inputs: 0,
            irq_lines: vec![0xFFFF; GPIO_PIN_COUNT],
            pending_irqs: 0,
            now_ps: 0,
        }],
        timers: vec![],
        ..M15PeriphDto::default()
    };

    let err = restore_session_states(&instances, &state).expect_err("未定义的 STATUS 位必须被拒");
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
    // 拒绝时**别的外设也不许被动过**。
    let after = collect_session_state(&instances);
    assert_eq!(after.gpios[0].outputs, 0, "被拒的载荷不许动任何外设");
}

/// ⚠️ **UART 的 LSR 与 FIFO 自相矛盾** → 拒。
///
/// `LSR_RX_VALID` 说「RX FIFO 里有字节」，可 FIFO 里**没有**字节——这样
/// 的状态不可能由这个 UART 产生（`LSR` 的 RX 位是 RX FIFO 的**唯一**
/// 写者），还原进去会让固件读到一份**说谎**的寄存器。
///
/// ⚠️ **v0.23**：状态寄存器从旧布局的自定义 STATUS 变成 ns16550 的
/// **LSR**，RX 位从 `0x2` 挪到了 **bit0（0x01）**。
#[test]
fn a_uart_state_whose_status_contradicts_its_fifos_is_rejected() {
    let uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let bad = PeripheralSessionState::Uart(UartStateDto {
        id: "UART0".to_string(),
        tx: None,
        rx: None,
        // LSR_RX_VALID 置位，可 RX FIFO 是空的。
        status: 0x1,
        irq_line: 4,
        now_ps: 0,

        ..UartStateDto::default()
    });
    let err = uart
        .validate_session_state(&bad)
        .expect_err("自相矛盾的 UART 状态必须被拒");
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **三个外设各自覆写**了 `validate_session_state`——**不能**靠契约
/// 层的默认放行。
///
/// 判别式是**行为**，不是白盒检查「谁覆写了」：
///
/// - GPIO 的规则（`irq_lines` 的条数）必须**拒**；
/// - UART 的规则（未定义的 STATUS 位）必须**拒**；
/// - 定时器必须**接受**它自己的状态（它没有别的规则）。
#[test]
fn every_participating_peripheral_overrides_its_own_acceptance_check() {
    // GPIO：条数不对 → 拒。
    let gpio = GpioBank::new("GPIO0", 0x2000);
    let short = PeripheralSessionState::Gpio(GpioStateDto {
        id: "GPIO0".to_string(),
        directions: 0,
        outputs: 0,
        inputs: 0,
        irq_lines: vec![1, 2, 3],
        pending_irqs: 0,
        now_ps: 0,
    });
    assert!(
        gpio.validate_session_state(&short).is_err(),
        "GPIO 必须拒掉条数不对的 irq_lines"
    );

    // UART：未定义的 STATUS 位 → 拒。
    let uart = Uart16550::new("UART0", 0x1000, 9600, 4);
    let bad_status = PeripheralSessionState::Uart(UartStateDto {
        id: "UART0".to_string(),
        tx: None,
        rx: None,
        status: 0x8000,
        irq_line: 4,
        now_ps: 0,

        ..UartStateDto::default()
    });
    assert!(
        uart.validate_session_state(&bad_status).is_err(),
        "UART 必须拒掉未定义的 STATUS 位"
    );

    // 定时器：它自己的状态 → 接受。
    let mut tim = TimerRiscv::new("TIM0", 0x3000, 7);
    tim.set_mtimecmp(9_000);
    let own = tim
        .session_state()
        .expect("the timer participates in the session state");
    assert!(
        tim.validate_session_state(&own).is_ok(),
        "定时器必须接受自己的状态"
    );
}

// ---------------------------------------------------------------------------
// v0.24 · 会话快照 v2：CLINT / PLIC 进快照（D-117）+ UART 的七个寄存器
// （D-118）
// ---------------------------------------------------------------------------

/// 一台**写满了寄存器**的 ns16550 —— 用来断言「快照里带的是那七个
/// 寄存器的**具体值**」。
///
/// ⚠️ `DLL` / `DLM` 必须在 `DLAB` 置位时写（否则写进去的是 `THR` /
/// `IER`）⇒ 夹具里的**顺序**是语义的一部分。
fn uart_with_registers_written() -> Uart16550 {
    let mut u = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    // LCR = 0x80（DLAB）| 0x03（8N1）⇒ 除数锁存器可见。
    u.write(uart::reg::LCR, WriteSize::Byte, 0x83)
        .expect("set LCR with DLAB");
    u.write(uart::reg::THR, WriteSize::Byte, 0x0C)
        .expect("write DLL");
    u.write(uart::reg::IER, WriteSize::Byte, 0x01)
        .expect("write DLM");
    // 回到 8N1（DLAB 清）⇒ `0x1` 又是 IER 了。
    u.write(uart::reg::LCR, WriteSize::Byte, 0x03)
        .expect("clear DLAB");
    u.write(uart::reg::IER, WriteSize::Byte, 0x01)
        .expect("write IER");
    u.write(uart::reg::FCR, WriteSize::Byte, 0x07)
        .expect("write FCR");
    u.write(uart::reg::MCR, WriteSize::Byte, 0x0F)
        .expect("write MCR");
    u.write(uart::reg::SCR, WriteSize::Byte, 0x5A)
        .expect("write SCR");
    u
}

/// ⚠️ **I-1a**：UART 的会话状态**带** ns16550 的七个寄存器 —— 断言
/// **具体值**，不是「非空」。
#[test]
fn the_uart_session_state_carries_the_seven_ns16550_registers() {
    let u = uart_with_registers_written();
    let exported = u
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(s) = exported else {
        panic!("a UART must export a UART state");
    };

    assert_eq!(s.lcr, 0x03, "LCR 必须带（DLAB 已清）");
    assert_eq!(s.ier, 0x01, "IER 必须带");
    assert_eq!(s.fcr, 0x07, "FCR 必须带");
    assert_eq!(s.mcr, 0x0F, "MCR 必须带");
    assert_eq!(s.scr, 0x5A, "SCR 必须带");
    assert_eq!(s.dll, 0x0C, "DLL 必须带");
    assert_eq!(s.dlm, 0x01, "DLM 必须带");
}

/// ⚠️ **I-1b**：快照**往返一次**后，那七个寄存器（含 **`DLAB` 位**）
/// **不变**。
///
/// ⚠️ **判别力**：这条用例能抓住「还原时漏了一个字段」——尤其是
/// **`LCR`**：漏了它，除数锁存器的**访问模式**就丢了（`DLAB` 清 ⇒
/// `0x0` 是 `THR` 而不是 `DLL`）⇒ guest 之后写 `0x0` 会把字节**发出去**
/// 而不是写波特率除数。
#[test]
fn the_uart_session_state_round_trips_the_registers_including_dlab() {
    // 夹具特意让 **DLAB 置位**（除数锁存器可见）。
    let mut u = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    u.write(uart::reg::LCR, WriteSize::Byte, 0x83)
        .expect("set LCR with DLAB");
    u.write(uart::reg::THR, WriteSize::Byte, 0x0C)
        .expect("write DLL");
    u.write(uart::reg::IER, WriteSize::Byte, 0x01)
        .expect("write DLM");

    let exported = u
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(s) = &exported else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(s.lcr, 0x83, "夹具：LCR 的 DLAB 位是置位的");
    assert_eq!(s.dll, 0x0C);
    assert_eq!(s.dlm, 0x01);

    let mut fresh = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    fresh
        .restore_session_state(&exported)
        .expect("the UART must accept its own state");

    assert_eq!(
        fresh.session_state(),
        Some(exported.clone()),
        "往返必须逐字段相等——七个寄存器一个都不能少"
    );
    assert_eq!(
        fresh.line_control(),
        0x83,
        "LCR（含 DLAB 位）必须被还原——否则除数锁存器的访问模式就丢了"
    );
    assert!(fresh.dlab_enabled(), "还原之后 DLAB 必须仍然是置位的");
    assert_eq!(fresh.divisor(), 0x010C, "除数锁存器必须被还原");
}

/// ⚠️ **反面**：**还原漏掉 `LCR`** 时这条会红（判别力的自证）。
///
/// 夹具故意造一个「`LCR` 不带 `DLAB`、但 `DLL`/`DLM` 有值」的载荷 ⇒
/// 还原它之后除数还在、但访问模式错了 ⇒ `dlab_enabled()` 必须是 `false`
/// ⇒ 这与上面那条（`true`）形成**对照**。
#[test]
fn a_payload_without_dlab_restores_without_dlab() {
    let mut fresh = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    let state = PeripheralSessionState::Uart(UartStateDto {
        id: "UART0".to_string(),
        lcr: 0x03,
        dll: 0x0C,
        dlm: 0x01,
        ..UartStateDto::default()
    });
    fresh
        .restore_session_state(&state)
        .expect("the UART must accept a well-formed state");
    assert!(!fresh.dlab_enabled(), "DLAB 必须**照实**还原（这里是清）");
    assert_eq!(fresh.divisor(), 0x010C, "除数本身仍然被还原");
}

/// ⚠️ **I-2**：CLINT 的会话状态带 `mtimecmp`（**具体周期数**），而且
/// **往返一次后不变**。
#[test]
fn the_clint_session_state_carries_mtimecmp_and_round_trips() {
    let mut clint = Clint::with_frequency("CLINT0", clint::CLINT_BASE, 1, 10_000_000);
    // `mtimecmp` 是 64 位 ⇒ 分两半写（低半再高半）。
    clint
        .write(0x4000, WriteSize::Word, 0x89AB_CDEF)
        .expect("mtimecmp low");
    clint
        .write(0x4004, WriteSize::Word, 0x0123_4567)
        .expect("mtimecmp high");
    clint.write(0x0, WriteSize::Word, 1).expect("msip = 1");
    clint.tick(1_000_000_000).expect("tick");

    let exported = clint
        .session_state()
        .expect("the CLINT participates in the session state");
    let PeripheralSessionState::Clint(s) = &exported else {
        panic!("a CLINT must export a CLINT state");
    };
    assert_eq!(s.id, "CLINT0");
    assert_eq!(
        s.mtimecmp_cycles,
        vec![0x0123_4567_89AB_CDEF],
        "mtimecmp 必须带（单位：周期）"
    );
    assert_eq!(s.msip, vec![1], "msip 的原始值必须带");
    assert_eq!(s.now_ps, 1_000_000_000, "器件的虚拟时间必须带");
    assert_eq!(
        s.mtime_cycles, 10_000,
        "mtime 必须带（1 ms @ 10 MHz = 10000 周期）"
    );

    let mut fresh = Clint::with_frequency("CLINT0", clint::CLINT_BASE, 1, 10_000_000);
    fresh
        .restore_session_state(&exported)
        .expect("the CLINT must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported.clone()),
        "往返必须逐字段相等"
    );
    assert_eq!(fresh.mtimecmp_cycles(), 0x0123_4567_89AB_CDEF);
    assert!(fresh.msip_pending(), "msip 的挂起位必须被还原");
    assert_eq!(
        fresh.mtime_cycles(),
        10_000,
        "还原之后 mtime 必须从还原点继续走，而不是回到「虚拟时间的函数」"
    );
}

/// ⚠️ **I-3**：PLIC 的会话状态带优先级 / 使能 / 门限（**具体值**），而且
/// **往返一次后不变**。
#[test]
fn the_plic_session_state_carries_priority_enable_and_threshold_and_round_trips() {
    let mut plic = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    plic.set_priority(1, 3);
    plic.enable(0, 1, true);
    plic.set_threshold(0, 1);
    plic.request(1);
    plic.tick(500).expect("tick");

    let exported = plic
        .session_state()
        .expect("the PLIC participates in the session state");
    let PeripheralSessionState::Plic(s) = &exported else {
        panic!("a PLIC must export a PLIC state");
    };
    assert_eq!(s.id, "PLIC0");
    assert_eq!(
        s.priorities,
        vec![0, 3, 0, 0],
        "每个源的优先级必须带（下标即源号；源 0 是保留源）"
    );
    assert_eq!(
        s.enables,
        vec![vec![0x0000_0002], vec![0x0000_0000]],
        "每个 context 的使能位图必须带（context 0 使能了源 1）"
    );
    assert_eq!(s.thresholds, vec![1, 0], "每个 context 的门限必须带");
    assert_eq!(s.pending, vec![0x0000_0002], "挂起位图必须带（源 1 挂着）");
    assert_eq!(s.now_ps, 500);

    let mut fresh = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    fresh
        .restore_session_state(&exported)
        .expect("the PLIC must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported.clone()),
        "往返必须逐字段相等"
    );
    assert_eq!(fresh.priority(1), Some(3), "优先级必须被还原");
    assert!(fresh.enabled(0, 1).unwrap(), "使能位必须被还原");
    assert_eq!(fresh.threshold(0), Some(1), "门限必须被还原");
    assert!(fresh.pending(1), "挂起位必须被还原");
}

/// ⚠️ **I-4a**：CLINT 的 `validate_session_state()` **不是**默认放行 ——
/// **外设 id 对不上**必须被拒。
#[test]
fn a_clint_rejects_a_state_that_names_another_peripheral() {
    let clint = Clint::new("CLINT0", clint::CLINT_BASE);
    let foreign = PeripheralSessionState::Clint(ClintStateDto {
        id: "CLINT1".to_string(),
        msip: vec![0],
        mtimecmp_cycles: vec![0],
        mtime_cycles: 0,
        now_ps: 0,
    });
    let err = clint
        .validate_session_state(&foreign)
        .expect_err("id 对不上的状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

/// ⚠️ **I-4b**：CLINT 的**数量与器件能力不符**必须被拒（器件 2 个 hart，
/// 载荷只描述了 1 个）。
#[test]
fn a_clint_rejects_a_state_with_the_wrong_number_of_harts() {
    let clint = Clint::with_harts("CLINT0", clint::CLINT_BASE, 2);
    let bad = PeripheralSessionState::Clint(ClintStateDto {
        id: "CLINT0".to_string(),
        msip: vec![0],
        mtimecmp_cycles: vec![0],
        mtime_cycles: 0,
        now_ps: 0,
    });
    let err = clint
        .validate_session_state(&bad)
        .expect_err("hart 数对不上的状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **I-4c**：PLIC 的**源数与器件能力不符**必须被拒（器件 4 个源，载荷
/// 只描述了 3 个）。
#[test]
fn a_plic_rejects_a_state_with_the_wrong_number_of_sources() {
    let plic = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    let bad = PeripheralSessionState::Plic(PlicStateDto {
        id: "PLIC0".to_string(),
        priorities: vec![0, 0, 0],
        enables: vec![vec![0], vec![0]],
        thresholds: vec![0, 0],
        pending: vec![0],
        now_ps: 0,
    });
    let err = plic
        .validate_session_state(&bad)
        .expect_err("源数对不上的状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **I-4d**：PLIC 的 **context 数与器件能力不符**必须被拒。
#[test]
fn a_plic_rejects_a_state_with_the_wrong_number_of_contexts() {
    let plic = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    let bad = PeripheralSessionState::Plic(PlicStateDto {
        id: "PLIC0".to_string(),
        priorities: vec![0, 0, 0, 0],
        enables: vec![vec![0]],
        thresholds: vec![0],
        pending: vec![0],
        now_ps: 0,
    });
    let err = plic
        .validate_session_state(&bad)
        .expect_err("context 数对不上的状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **反面**：**数量对得上**的载荷必须被接受 —— 否则上面那几条可能
/// 只是「校验器什么都拒」。
#[test]
fn a_clint_and_a_plic_accept_a_state_that_matches_their_capability() {
    let clint = Clint::with_harts("CLINT0", clint::CLINT_BASE, 2);
    let good = PeripheralSessionState::Clint(ClintStateDto {
        id: "CLINT0".to_string(),
        msip: vec![0, 0],
        mtimecmp_cycles: vec![0, 0],
        mtime_cycles: 0,
        now_ps: 0,
    });
    assert!(
        clint.validate_session_state(&good).is_ok(),
        "数量对得上的 CLINT 状态必须被接受"
    );

    let plic = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    let good = PeripheralSessionState::Plic(PlicStateDto {
        id: "PLIC0".to_string(),
        priorities: vec![0, 0, 0, 0],
        enables: vec![vec![0], vec![0]],
        thresholds: vec![0, 0],
        pending: vec![0],
        now_ps: 0,
    });
    assert!(
        plic.validate_session_state(&good).is_ok(),
        "数量对得上的 PLIC 状态必须被接受"
    );
}

/// ⚠️ **附加 3**：器件**不匹配**时（把 CLINT 的状态喂给 PLIC）⇒ 报
/// `E_PER-04` 那一族（**族 + 场景码**都断言）。
#[test]
fn a_clint_state_handed_to_a_plic_is_rejected() {
    let plic = Plic::new("PLIC0", plic::PLIC_BASE);
    let clint_state = PeripheralSessionState::Clint(ClintStateDto {
        id: "PLIC0".to_string(),
        msip: vec![0],
        mtimecmp_cycles: vec![0],
        mtime_cycles: 0,
        now_ps: 0,
    });
    let err = plic
        .validate_session_state(&clint_state)
        .expect_err("PLIC 必须拒掉 CLINT 的状态");
    assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
    assert_eq!(
        err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE,
        "场景码必须是 E_PER-04"
    );

    // 反过来也一样：CLINT 不得吞掉 PLIC 的状态。
    let clint = Clint::new("CLINT0", clint::CLINT_BASE);
    let plic_state = PeripheralSessionState::Plic(PlicStateDto {
        id: "CLINT0".to_string(),
        priorities: vec![0],
        enables: vec![vec![0]],
        thresholds: vec![0],
        pending: vec![0],
        now_ps: 0,
    });
    let err = clint
        .validate_session_state(&plic_state)
        .expect_err("CLINT 必须拒掉 PLIC 的状态");
    assert_eq!(err.family, ErrorFamily::Peripheral, "族必须是 PER");
    assert_eq!(
        err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE,
        "场景码必须是 E_PER-04"
    );
}

/// ⚠️ **附加 2**：收集顺序 —— 同一组内**按 id 升序**（与既有的
/// uarts / gpios / timers 规矩一致）。
#[test]
fn the_collected_state_orders_the_new_groups_by_id() {
    let clint_b = share(Clint::new("CLINTB", 0x0200_0000));
    let clint_a = share(Clint::new("CLINTA", 0x0200_1000));
    let plic_b = share(Plic::new("PLICB", 0x0C00_0000));
    let plic_a = share(Plic::new("PLICA", 0x0C00_1000));
    let instances: Vec<(u32, SharedPeripheral)> = vec![
        (0x0200_0000, clint_b),
        (0x0200_1000, clint_a),
        (0x0C00_0000, plic_b),
        (0x0C00_1000, plic_a),
    ];

    let dto = collect_session_state(&instances);
    assert_eq!(dto.clints.len(), 2);
    assert_eq!(dto.clints[0].id, "CLINTA", "CLINT 组必须按 id 升序");
    assert_eq!(dto.clints[1].id, "CLINTB");
    assert_eq!(dto.plics.len(), 2);
    assert_eq!(dto.plics[0].id, "PLICA", "PLIC 组必须按 id 升序");
    assert_eq!(dto.plics[1].id, "PLICB");
}

/// ⚠️ **双向匹配**对两个新器件**同样成立**：活着的 CLINT 必须被载荷
/// 描述 —— 否则它的状态会被**静默丢掉**。
#[test]
fn a_live_clint_that_the_payload_does_not_describe_is_rejected() {
    let clint = share(Clint::new("CLINT0", 0x0200_0000));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x0200_0000, clint)];

    // 载荷里**没有**这个 CLINT ⇒ 双向匹配失败。
    let state = M15PeriphDto {
        clints: Vec::new(),
        ..M15PeriphDto::default()
    };
    let err = restore_session_states(&instances, &state).expect_err("活着的 CLINT 必须被载荷描述");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);

    // 反过来：载荷里**多**了一个不存在的 CLINT ⇒ 同样被拒。
    let clint = share(Clint::new("CLINT0", 0x0200_0000));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x0200_0000, clint)];
    let state = M15PeriphDto {
        clints: vec![ClintStateDto {
            id: "GHOST".to_string(),
            msip: vec![0],
            mtimecmp_cycles: vec![0],
            mtime_cycles: 0,
            now_ps: 0,
        }],
        ..M15PeriphDto::default()
    };
    let err =
        restore_session_states(&instances, &state).expect_err("载荷里多出来的 CLINT 必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_05_PERIPHERAL_SET_MISMATCH);
}

/// ⚠️ **两阶段纪律**对两个新器件**同样成立**：畸形状态必须被**只读预检**
/// 拒掉，且**一个字节都不许写**。
#[test]
fn the_read_only_preflight_rejects_a_malformed_clint_state() {
    let clint = share(Clint::with_harts("CLINT0", 0x0200_0000, 2));
    let instances: Vec<(u32, SharedPeripheral)> = vec![(0x0200_0000, Rc::clone(&clint))];

    // 畸形：载荷只描述了 1 个 hart，器件有 2 个。
    let state = M15PeriphDto {
        clints: vec![ClintStateDto {
            id: "CLINT0".to_string(),
            msip: vec![0],
            mtimecmp_cycles: vec![0],
            mtime_cycles: 0,
            now_ps: 0,
        }],
        ..M15PeriphDto::default()
    };
    // 只读预检**不许写**：器件的会话状态必须**逐字段**不变。
    let before = clint.borrow().session_state();
    let err = validate_session_states(&instances, &state)
        .expect_err("畸形的 CLINT 状态必须在只读预检里被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
    let after = clint.borrow().session_state();
    assert_eq!(
        before, after,
        "预检不许写一个字节——器件的会话状态必须逐字段不变"
    );
}

/// ⚠️ **正面**：两个新器件的**完整往返**（导出 → 还原 → 再导出）必须
/// **逐字段相等**。
#[test]
fn the_clint_and_plic_round_trip_through_the_registry() {
    let mut reg = PeripheralRegistry::new();
    let mut clint = Clint::with_frequency("CLINT0", clint::CLINT_BASE, 1, 10_000_000);
    clint
        .write(0x4000, WriteSize::Word, 0x0000_0000_0000_1000)
        .expect("mtimecmp");
    clint.write(0x0, WriteSize::Word, 1).expect("msip");
    clint.tick(1_000_000_000).expect("tick");
    reg.register(clint).expect("clint");

    let mut plic = Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2);
    plic.set_priority(2, 5);
    plic.enable(1, 2, true);
    plic.set_threshold(1, 0);
    plic.request(2);
    reg.register(plic).expect("plic");

    let exported = reg.export_state();
    assert_eq!(exported.clints.len(), 1);
    assert_eq!(exported.plics.len(), 1);

    let mut fresh = PeripheralRegistry::new();
    fresh
        .register(Clint::with_frequency(
            "CLINT0",
            clint::CLINT_BASE,
            1,
            10_000_000,
        ))
        .expect("clint");
    fresh
        .register(Plic::with_capacity("PLIC0", plic::PLIC_BASE, 4, 2))
        .expect("plic");
    fresh
        .load_state(&exported)
        .expect("两个新器件必须接受自己的状态");

    assert_eq!(
        fresh.export_state(),
        exported,
        "往返必须逐字段相等——CLINT 与 PLIC 的状态一个都不能少"
    );
}

// ---------------------------------------------------------------------------
// v0.24 · 接收 FIFO 加深（16 字节）后的会话快照
// ---------------------------------------------------------------------------

/// ⚠️ **接收 FIFO 是整个队列**：UART 的会话状态必须带**全部**待读字节
/// （不只是队头那一个）⇒ 否则快照往返会**丢字节** ⇒ 还原出来的会话与
/// 采快照的那次**不是同一个**。
#[test]
fn the_uart_session_state_carries_the_whole_rx_queue() {
    let mut u = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    for byte in [0x41u8, 0x42, 0x43] {
        u.receive(byte);
    }

    let exported = u
        .session_state()
        .expect("the UART participates in the session state");
    let PeripheralSessionState::Uart(s) = &exported else {
        panic!("a UART must export a UART state");
    };
    assert_eq!(
        s.rx_queue,
        vec![0x41, 0x42, 0x43],
        "整个接收队列必须带（不只是队头）"
    );
    assert_eq!(s.rx, Some(0x41), "rx 是**队头**那一个字节");
    assert_eq!(
        s.status & u32::from(uart::lsr_bits::RX_VALID),
        u32::from(uart::lsr_bits::RX_VALID),
        "FIFO 非空 ⇒ LSR 的 RX_VALID 必须置位"
    );
}

/// ⚠️ **往返一次后队列逐字节相同** —— 还原出来的 UART 读出来的必须是
/// **同样顺序**的同样三个字节。
#[test]
fn the_uart_session_state_round_trips_the_whole_rx_queue() {
    let mut u = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    for byte in [0x41u8, 0x42, 0x43] {
        u.receive(byte);
    }
    let exported = u
        .session_state()
        .expect("the UART participates in the session state");

    let mut fresh = Uart16550::new("UART0", 0x1000_0000, 115_200, 4);
    fresh
        .restore_session_state(&exported)
        .expect("the UART must accept its own state");
    assert_eq!(
        fresh.session_state(),
        Some(exported.clone()),
        "往返必须逐字段相等——接收队列一个字节都不能少"
    );

    for byte in [0x41u8, 0x42, 0x43] {
        assert_eq!(
            fresh.read_clear(0x0, WriteSize::Byte).unwrap(),
            u32::from(byte),
            "还原之后的 UART 读出来的必须是**同样顺序**的同样字节"
        );
    }
}

/// ⚠️ **队列比器件的 FIFO 深** ⇒ 必须被拒（还原时必然要**截断** ⇒
/// 丢字节 ⇒ 那不是「还原」，是「还原成另一个东西」）。
#[test]
fn a_uart_state_with_a_longer_queue_than_the_fifo_is_rejected() {
    let u = Uart16550::new("UART0", 0x1000, 9600, 4);
    let bad = PeripheralSessionState::Uart(UartStateDto {
        id: "UART0".to_string(),
        rx: Some(0x00),
        rx_queue: vec![0x00; uart::RX_FIFO_CAPACITY + 1],
        status: u32::from(uart::lsr_bits::RX_VALID),
        ..UartStateDto::default()
    });
    let err = u
        .validate_session_state(&bad)
        .expect_err("比 FIFO 深的队列必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}

/// ⚠️ **`rx` 与 `rx_queue` 的队头不一致** ⇒ 器件侧也必须拒（载荷自相
/// 矛盾：它同时说「队头是 0x41」和「队头是 0x42」）。
#[test]
fn a_uart_state_whose_head_disagrees_with_its_queue_is_rejected() {
    let u = Uart16550::new("UART0", 0x1000, 9600, 4);
    let bad = PeripheralSessionState::Uart(UartStateDto {
        id: "UART0".to_string(),
        rx: Some(0x41),
        rx_queue: vec![0x42, 0x43],
        status: u32::from(uart::lsr_bits::RX_VALID),
        ..UartStateDto::default()
    });
    let err = u
        .validate_session_state(&bad)
        .expect_err("队头与队列不一致的状态必须被拒");
    assert_eq!(err.family, ErrorFamily::Peripheral);
    assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
}
