//! v0.26 批（Lane B）的**TX 捕获环进会话快照**用例。
//!
//! 冻结规格：`docs/detailed-design/STAGE-11_V026_Memory_Map_A.md`
//! §2.2 的 **B-5**（TX 捕获环进快照）与 **B-6**（`tx_dropped` 进快照）。
//!
//! ## 这一层测的是**整条链**
//!
//! 器件侧的用例在 `crates/kernel/src/m15_peripheral/tests.rs`；本文件测
//! 的是**宿主这一层**：写 THR → [`KernelHost::capture_session`] → 载荷里
//! 的 `tx_ring` → [`KernelHost::restore_session`] → 宿主取走字节。
//!
//! ⚠️ **为什么用 QemuVirt 档**：宿主取 UART 字节的那个出口
//! （`drain_console`）只认**宿主自己挂的那台 UART** ⇒ 只有 QemuVirt 档
//! 有（默认档没有 UART）。

use riscv_sim_contract::session::{CheckpointMetaDto, SESSION_SNAPSHOT_SCHEMA_VERSION};
use riscv_sim_kernel::kernel_host::{KernelHost, MemoryProfile, QEMU_VIRT_UART0_BASE};
use riscv_sim_kernel::m14_bus::BusWidth;

// ---------------------------------------------------------------------------
// 架构字面量（**不从被测实现反推**）
// ---------------------------------------------------------------------------

/// 捕获环的默认深度（4 KiB）—— M-15 的 ns16550 默认值。
const TX_CAPTURE_CAPACITY: usize = 4096;

/// B-5 用的三个字节。
const PAYLOAD_BYTES: [u8; 3] = *b"ABC";

/// B-6 用的溢出字节数（比环深 ⇒ 必然丢）。
const OVERFLOW_BYTES: usize = 4;

/// 检查点元数据（段 0）。
fn meta() -> CheckpointMetaDto {
    CheckpointMetaDto {
        id: 7,
        t_virt: 0,
        t_host: 0,
        base_seq: 0,
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
    }
}

/// 造一个 QemuVirt 档的宿主（它带 UART0）。
fn qemuvirt_host() -> KernelHost {
    let host = KernelHost::new_with_profile(MemoryProfile::QemuVirt);
    assert_eq!(
        host.probe_wiring_failures(),
        0,
        "换档失败会**静默退回默认档** ⇒ 失败次数必须是 0"
    );
    host
}

/// 往 UART0 的 THR 写一个字节。
fn write_thr(host: &mut KernelHost, byte: u8) {
    host.bus_mut()
        .write(QEMU_VIRT_UART0_BASE, BusWidth::Byte, u32::from(byte))
        .expect("写 THR");
}

// ---------------------------------------------------------------------------
// B-5 · TX 捕获环进快照
// ---------------------------------------------------------------------------

#[test]
fn the_tx_capture_ring_travels_into_the_session_snapshot() {
    let mut host = qemuvirt_host();
    for byte in PAYLOAD_BYTES {
        write_thr(&mut host, byte);
    }

    let payload = host.capture_session(meta());

    // ---- 载荷里必须有那台 UART，且 `tx_ring` 是**三个**字节 ----
    assert_eq!(
        payload.m15_periph.uarts.len(),
        1,
        "载荷里必须恰好有一台 UART：{:#?}",
        payload.m15_periph.uarts
    );
    let uart = &payload.m15_periph.uarts[0];
    assert_eq!(uart.id, "UART0", "UART 的 id 必须能对上活实例");
    assert_eq!(
        uart.tx_ring,
        PAYLOAD_BYTES.to_vec(),
        "捕获环的**全部**待取字节必须进快照（不是只有队头）"
    );
    assert_eq!(
        uart.tx,
        Some(PAYLOAD_BYTES[0]),
        "`tx` 必须等于 `tx_ring` 的**队头**"
    );

    // ---- 载荷必须自洽（契约层的 `validate()` 查 `tx` / `tx_ring` 不变式）----
    payload.validate().expect("采出来的载荷必须通过校验");
}

#[test]
fn the_tx_capture_ring_survives_a_snapshot_round_trip() {
    let mut source = qemuvirt_host();
    for byte in PAYLOAD_BYTES {
        write_thr(&mut source, byte);
    }
    let payload = source.capture_session(meta());

    // ---- 还原进一个**全新的**宿主 ----
    let mut target = qemuvirt_host();
    target
        .restore_session(&payload)
        .expect("还原必须成功（载荷自洽 ⇒ 预检通过）");

    assert_eq!(
        target.console_pending(),
        PAYLOAD_BYTES.len(),
        "还原后捕获环里必须有**三个**字节"
    );

    // ---- 宿主取走字节 ⇒ 必须**逐字节相同** ----
    assert_eq!(
        target.drain_console(),
        PAYLOAD_BYTES.to_vec(),
        "还原后取走的字节必须与写进去的**逐字节相同**"
    );
}

// ---------------------------------------------------------------------------
// B-6 · `tx_dropped` 进快照
// ---------------------------------------------------------------------------

#[test]
fn the_dropped_byte_counter_travels_into_the_session_snapshot() {
    let mut host = qemuvirt_host();

    // ---- 把捕获环灌满，再多写 4 个字节 ⇒ 必然丢 4 个 ----
    for i in 0..TX_CAPTURE_CAPACITY {
        write_thr(&mut host, (i % 251) as u8);
    }
    for _ in 0..OVERFLOW_BYTES {
        write_thr(&mut host, b'Z');
    }

    let payload = host.capture_session(meta());
    let uart = &payload.m15_periph.uarts[0];
    assert!(
        uart.tx_dropped > 0,
        "溢出丢弃的累计计数必须进快照（丢失要**看得见**），实际是 {}",
        uart.tx_dropped
    );
    assert_eq!(
        uart.tx_dropped, OVERFLOW_BYTES as u64,
        "丢弃计数必须**精确**等于实际丢掉的字节数"
    );
    assert_eq!(
        uart.tx_ring.len(),
        TX_CAPTURE_CAPACITY,
        "环里保留的必须是最早的那 {} 个字节",
        TX_CAPTURE_CAPACITY
    );
}

#[test]
fn the_dropped_byte_counter_survives_a_snapshot_round_trip() {
    let mut source = qemuvirt_host();
    for i in 0..TX_CAPTURE_CAPACITY {
        write_thr(&mut source, (i % 251) as u8);
    }
    for _ in 0..OVERFLOW_BYTES {
        write_thr(&mut source, b'Z');
    }
    let payload = source.capture_session(meta());

    let mut target = qemuvirt_host();
    target.restore_session(&payload).expect("还原必须成功");

    // ---- 还原后的计数必须**等于**载荷里的值（覆盖，不是累加）----
    let restored = target.capture_session(meta());
    assert_eq!(
        restored.m15_periph.uarts[0].tx_dropped, OVERFLOW_BYTES as u64,
        "还原后的丢弃计数必须**等于**载荷里的值（快照 = 状态）"
    );
}
