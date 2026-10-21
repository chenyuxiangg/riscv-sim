//! Port signatures for the two key abstractions (LLD §3.2.4 / §3.2.5,
//! AD §4.2 / §4.3) and the observability pipeline.
//!
//! Every concrete module exposes a `*Port` trait. Alternative
//! implementations (e.g. a QEMU-backed ExecutionBackend) just implement
//! the same trait and pass the contract tests (LLD §7.1).

use crate::error::{ErrorFamily, PlatformError};
use crate::model::{PendingWrite, SimState};
use crate::protocol::{CommandPayload, EventPayload};
use crate::snapshot::{EngineSnapshotDto, E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT};

/// Callback signature for peripheral signal reports (M-18 v0.8 §18-2,
/// additive — see M-15 review #2 for the additive default-method pattern).
///
/// The peripheral invokes this when its own state changes in a way
/// observable to the waveform (e.g. GPIO OUT transition, UART TX pin
/// level). The closure receives `(signal_id, value, now_ps)`. Signal id
/// is a stable string per peripheral (e.g. `"OUT"`, `"TX"`); M-18 looks
/// up the channel bound to `(periph_id, signal_id)` and decides whether
/// to emit an edge event.
///
/// MVP scope: single-threaded (no `Send`); matches M-14's `BusHook`
/// posture. When the production stack adds multithreaded dispatchers,
/// add the `+ Send` bound and re-export a new alias (the contract stays
/// additive — existing peripherals do not need to change).
pub type ProbeSink = Box<dyn FnMut(&str, u64, u64)>;

/// Driven port: front-end → session / command router (M-05).
///
/// The session owns the receive loop; it converts inbound [`Message`]s
/// into kernel-internal commands.
pub trait CommandBus {
    /// Submit a command for dispatch. Returns once the command is queued
    /// (not necessarily executed). Use the event stream for completion.
    fn submit(&mut self, cmd: CommandPayload) -> Result<(), PlatformError>;
}

/// Driving port: kernel → event bus (M-05).
pub trait EventBus {
    /// Broadcast an event to all subscribers. Returns an error if the
    /// bus is full (caller should apply back-pressure).
    fn publish(&mut self, evt: EventPayload) -> Result<(), PlatformError>;
}

/// Driven port: the abstract execution backend (M-17).
///
/// Two implementations exist:
/// - `SelfIssBackend` — in-tree RV32IMC ISS (M-13).
/// - `QemuBackend` — process-isolated QEMU via GDB RSP (P1, MIT-process
///   isolation, not linked).
///
/// Both honour the [`StepN`] / [`Continue`] / [`Pause`] semantics and
/// report back via [`EventBus`].
pub trait ExecutionBackend {
    /// Reset to entry state.
    fn reset(&mut self) -> Result<(), PlatformError>;

    /// Execute exactly `n` ISS instructions. Returns the number actually
    /// executed (may be less if a trap / breakpoint fires).
    fn step_n(&mut self, n: u32) -> Result<u32, PlatformError>;

    /// Run until the next breakpoint, watchpoint, or trap, **or until the
    /// engine's instruction budget for this run window is exhausted**.
    ///
    /// The quantum boundary is a **preemption point** (the scheduler may
    /// yield there), **not** a stop condition.
    ///
    /// **Contract v2 wording (D-57).** The v1 wording said "or quantum
    /// boundary", which contradicted the implementation: the engine ran
    /// until its instruction budget was exhausted (10 000 instructions =
    /// 10 quanta). The implementation is the one that was kept — it is
    /// the semantics D-07 closed and the `d07_time_driven.rs` suite
    /// pins — so the *documentation* was corrected instead.
    fn continue_until_event(&mut self) -> Result<(), PlatformError>;

    /// Pause as soon as possible (at the next quantum boundary).
    fn pause(&mut self) -> Result<(), PlatformError>;

    /// Capture a **partial** snapshot of the platform state.
    ///
    /// The returned [`SimState`] carries the platform state, the
    /// virtual clock and the event-queue insertion counter — but **not**
    /// the pending event queue, which the contract has no room for. A
    /// snapshot taken this way therefore cannot be restored
    /// deterministically; see [`Self::restore`].
    fn snapshot(&self) -> Result<SimState, PlatformError>;

    /// Restore a **partial** snapshot of the platform state.
    ///
    /// The platform state and the virtual clock are restored, but the
    /// pending event queue is not part of [`SimState`] and stays as it
    /// is. Restoring is therefore **not** the inverse of
    /// [`Self::snapshot`] in the deterministic sense: events queued
    /// after the snapshot was taken are still pending and will still
    /// fire, so a run after a restore can diverge from an uninterrupted
    /// run with the same seed.
    ///
    /// Callers that need a deterministic round-trip must snapshot the
    /// event queue as well. Use [`Self::snapshot_full`] /
    /// [`Self::restore_full`] for that — they carry the queue in the
    /// contract's serializable [`EngineSnapshotDto`].
    fn restore(&mut self, state: &SimState) -> Result<(), PlatformError>;

    /// Capture a **complete engine-level** snapshot: the platform state
    /// **and** the pending event queue, in a form that can be persisted.
    ///
    /// Unlike [`Self::snapshot`] (partial), the result round-trips
    /// deterministically through [`Self::restore_full`]: the queue
    /// contents, the queue order, the insertion counter and the virtual
    /// time all survive (D-05 / FR-12). This is the snapshot form M-20
    /// persists and M-23 stores.
    ///
    /// **Scope limit**: it captures the *engine*. It does **not** capture
    /// peripheral models (M-15) or the interrupt controller (M-16) — a
    /// host that persists only this DTO reproduces the engine, not a
    /// whole session. See [`crate::snapshot`] for the full list.
    ///
    /// Default: an error (`E_SNAP-03`) — the backend does not support
    /// complete snapshots. The default is **loud on purpose**: a soft
    /// "no snapshot available" answer invites the natural caller shape
    /// `if let Some(dto) = b.snapshot_full()? { … }`, whose `None` branch
    /// silently skips the restore and leaves the caller believing it
    /// resumed a deterministic state (D-24). Failing on the *first* call
    /// makes "this backend has no complete snapshot" impossible to miss.
    fn snapshot_full(&self) -> Result<EngineSnapshotDto, PlatformError> {
        Err(PlatformError::new(
            ErrorFamily::Snapshot,
            E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT,
            "backend does not support complete (deterministic) snapshots",
        ))
    }

    /// Restore a snapshot previously produced by [`Self::snapshot_full`].
    ///
    /// The snapshot is **validated before it is applied** — an unknown
    /// schema version, a malformed event queue or an insertion-counter
    /// mismatch is rejected with an error rather than silently loaded
    /// (FR-12). Implementations that override this method are expected
    /// to keep that property.
    ///
    /// Default: an error (`E_SNAP-03`) — the backend does not support
    /// deterministic restore, and **nothing is changed**. Failing loudly
    /// is deliberate: silently skipping the restore would leave the
    /// caller believing it had resumed a deterministic state when it had
    /// not (D-24).
    fn restore_full(&mut self, _snapshot: &EngineSnapshotDto) -> Result<(), PlatformError> {
        Err(PlatformError::new(
            ErrorFamily::Snapshot,
            E_SNAP_03_UNSUPPORTED_FULL_SNAPSHOT,
            "backend does not support deterministic snapshot restore",
        ))
    }

    /// Read a single memory location.
    fn read_memory(&self, addr: u32, size: crate::model::WriteSize) -> Result<u32, PlatformError>;

    /// Write a single memory location.
    fn write_memory(
        &mut self,
        addr: u32,
        size: crate::model::WriteSize,
        value: u32,
    ) -> Result<(), PlatformError>;

    /// Toggle a breakpoint (M-08). The backend is responsible for
    /// translating PC breakpoints to per-instruction trap points.
    fn set_breakpoint(&mut self, addr: u32, enabled: bool) -> Result<(), PlatformError>;

    /// Toggle a watchpoint (M-10).
    fn set_watchpoint(
        &mut self,
        addr: u32,
        size: crate::model::WriteSize,
        write: bool,
        enabled: bool,
    ) -> Result<(), PlatformError>;
}

/// Driven port: a single peripheral (M-15).
///
/// Peripherals register on the bus, expose a register window, and may
/// raise interrupts through [`crate::port::InterruptSink`]. They are
/// polled at the end of each quantum for time-driven side effects.
pub trait Peripheral {
    /// Stable peripheral id (e.g. `"UART0"`, `"TIM1"`).
    fn id(&self) -> &str;
    /// Base address on the bus.
    fn base_addr(&self) -> u32;
    /// Size in bytes of the register window.
    fn size(&self) -> u32;
    /// Read from a register. Width depends on the offset (see the
    /// module-level register map).
    fn read(&self, offset: u32, size: crate::model::WriteSize) -> Result<u32, PlatformError>;
    /// Read-and-clear a register (e.g. UART RX FIFO). Defaults to a
    /// plain [`Self::read`] for peripherals whose read is non-destructive.
    /// M-15 review #2: this is the additive way to express read-clear
    /// without breaking the `&self` invariant of [`Self::read`].
    fn read_clear(
        &mut self,
        offset: u32,
        size: crate::model::WriteSize,
    ) -> Result<u32, PlatformError> {
        self.read(offset, size)
    }
    /// Write to a register.
    fn write(
        &mut self,
        offset: u32,
        size: crate::model::WriteSize,
        value: u32,
    ) -> Result<(), PlatformError>;
    /// Called on **every instruction boundary** (and on every quantum
    /// boundary). The default quantum is 1 µs, but the tick is **not**
    /// quantised to it: the probe pipeline needs edge timestamps that are
    /// exact to a single instruction, so the tick runs at instruction
    /// granularity. Implementations must therefore be **cheap** and
    /// **idempotent within one virtual instant**.
    ///
    /// **Contract v2 wording (D-58).** The v1 wording said "called once
    /// per quantum (1 µs by default)", which contradicted the
    /// implementation: the tick runs at instruction granularity, because
    /// D-46 introduced that so probe edges carry an exact-to-one-
    /// instruction timestamp. Quantising it back would throw that away,
    /// so the *documentation* was corrected instead.
    fn tick(&mut self, now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError>;
    /// Install / replace a probe callback (M-18 v0.8, additive — see
    /// M-15 review #2 for the additive default-method pattern).
    ///
    /// The peripheral invokes this when its own state changes in a way
    /// observable to the waveform (e.g. GPIO OUT transition, UART TX
    /// pin level). Pass `None` to detach.
    ///
    /// Default = no-op; peripherals that wish to expose signals
    /// override this. M-15 review §五 N1 keeps the default benign so
    /// existing peripherals (Uart16550, GpioBank, TimerRiscv,
    /// DummyUart) do not need to change unless they actually want to
    /// report signals.
    fn on_probe(&mut self, _cb: Option<ProbeSink>) {}

    /// This peripheral's share of the **session snapshot** (segment 4 of
    /// the session payload — see
    /// [`crate::session::SessionSnapshotDto`]).
    ///
    /// Returns `None` when this peripheral contributes **no** session
    /// state — the default, and the right answer for a peripheral whose
    /// whole state is derivable from the bus (a plain register window,
    /// say). M-15's UART / GPIO / timer override it.
    ///
    /// ⚠️ **Additive** — this method has a default implementation, so
    /// every existing implementor keeps compiling and keeps its
    /// behaviour. Adding a method to a trait is an **additive** change
    /// (the contract's "only add, never change" rule), not a breaking
    /// one; the wire protocol is unaffected, so
    /// [`crate::SCHEMA_VERSION`] does **not** move.
    fn session_state(&self) -> Option<crate::session::PeripheralSessionState> {
        None
    }

    /// **Read-only** check: "can I accept this state?" — **writes
    /// nothing**.
    ///
    /// ⚠️ **This method is not optional in spirit, but it is additive in
    /// form.** The restore path's write stage may hide integrity
    /// constraints that **only this peripheral knows** (e.g. the GPIO's
    /// `irq_lines` must have exactly [`crate::session::GPIO_PIN_COUNT`]
    /// entries). If those checks live **only** in the write path, then
    /// "restore the UART first, discover the malformed GPIO afterwards"
    /// leaves a **half-applied session** (the UART has already been
    /// overwritten) behind an `Err`. A half-applied snapshot is worse
    /// than a rejected one (STAGE-3 §5).
    ///
    /// ⚠️ The default implementation **lets everything through**
    /// (`Ok(())`): it must be **additive** — every existing implementor
    /// keeps compiling and keeps its behaviour, so the wire protocol
    /// does not move and [`crate::SCHEMA_VERSION`] stays put. A
    /// peripheral that participates in the session snapshot **must**
    /// override it; that requirement is pinned by **behavioural** tests
    /// (not by a white-box check of "who overrode it").
    fn validate_session_state(
        &self,
        _state: &crate::session::PeripheralSessionState,
    ) -> Result<(), PlatformError> {
        Ok(())
    }

    /// Restore this peripheral's session state.
    ///
    /// The default implementation **rejects** any state with
    /// [`ErrorFamily::Peripheral`] — a peripheral that does not
    /// participate in the session snapshot must not **silently swallow**
    /// a state meant for it: that would drop state without a word, and
    /// the restored session would quietly differ from the captured one.
    fn restore_session_state(
        &mut self,
        state: &crate::session::PeripheralSessionState,
    ) -> Result<(), PlatformError> {
        let _ = state;
        Err(PlatformError::new(
            crate::error::ErrorFamily::Peripheral,
            E_PER_04_UNSUPPORTED_SESSION_STATE,
            format!(
                "外设 {} 不支持会话状态回灌——快照里有它的状态，但它没有实现",
                self.id()
            ),
        ))
    }
}

/// Scenario code: this peripheral does not accept the session state it was
/// handed — the default [`Peripheral::restore_session_state`] rejects it.
///
/// ⚠️ **A named constant, not a literal.** The default implementation used
/// to spell the code as a bare `4`; a bare number has no meaning attached
/// to it, so the same number could later be reused for something else
/// without anybody noticing (one code, two meanings — the same trap as
/// D-69's `E_PP` / `E_PER` and D-32's `E_PROTO-02`).
pub const E_PER_04_UNSUPPORTED_SESSION_STATE: u16 = 4;

/// Scenario code: the peripheral set in a session payload does **not** match
/// the live instances — either the payload names an id that is not live, or
/// a live peripheral that participates in the session state is not
/// described by the payload.
///
/// ⚠️ Both directions are checked on purpose: skipping either one would
/// leave the restored session **silently different** from the captured one.
pub const E_PER_05_PERIPHERAL_SET_MISMATCH: u16 = 5;

/// Driven port: peripheral → interrupt controller (M-15 → M-16).
pub trait InterruptSink {
    /// Raise an IRQ line. Idempotent.
    ///
    /// ⚠️ **This call may fail because the controller is busy** (the
    /// implementation may be reached while the controller is already
    /// borrowed — e.g. a `RefCell` contention in the kernel host). The
    /// failure is reported with the interrupt family's scenario code
    /// (`E_INT_01`, "controller busy") — the exact code is the
    /// implementation's, the contract only fixes the **meaning**:
    /// `Err` means **this IRQ line was NOT raised**. A caller that
    /// ignores the result believes the line is up while it is not.
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError>;
    /// Lower an IRQ line.
    ///
    /// ⚠️ Same failure semantics as [`Self::raise`]: `Err` means **this
    /// IRQ line was NOT lowered**.
    fn lower(&mut self, irq: u16) -> Result<(), PlatformError>;
}

/// Driven port: a probe source (M-18). The probe pipeline attaches one of
/// these to every bus signal and every peripheral output it wants to
/// capture for waveform rendering.
pub trait Probe {
    /// Stable probe id (e.g. `"BUS.A1.write"`).
    fn id(&self) -> &str;
    /// Called on every observed change. The probe is responsible for
    /// edge-encoding the value and appending it to its internal stream.
    fn observe(&mut self, now_ps: u64, value: u64) -> Result<(), PlatformError>;
    /// Snapshot the probe's accumulated edges (e.g. for waveform export).
    fn drain(&mut self) -> Result<Vec<crate::model::Edge>, PlatformError>;
}

/// Driven port: M-18 probe → M-19 waveform pipeline (M-19 v0.9 §1.1).
///
/// M-18 has already judged the edge (rising / falling / value) by the
/// time it calls this port; the implementation (M-19's `encodeEdge`)
/// only encodes the event into the per-channel ring buffer and
/// accumulates it into the pending push batch.
///
/// This is the port the M-19 design doc calls *"M-18 的 m19Encode 即本
/// 模块 encodeEdge 入口"*. It lives in the contract so that neither
/// side has to depend on the other's crate internals — consistent
/// with the "契约先行" rule (LLD §2.5).
///
/// Implementations must be cheap: this is called on the probe hot path
/// for every observed edge.
pub trait EdgeSink {
    /// Encode one edge event into the waveform pipeline.
    fn encode_edge(&mut self, ev: crate::model::EdgeEvent);
}

/// Driven port: M-19 waveform pipeline → delta subscriber (M-19 v0.9 §3.2).
///
/// The subscriber is whatever forwards the incremental waveform to
/// the front-end (M-03's W1 subscription, relayed by M-05 / M-07).
///
/// **Failure semantics:** observation events are explicitly allowed to
/// be lost (review P-09 — "观测类事件不要求可靠投递、允许节流合并").
/// A subscriber that returns an error causes M-19 to **drop that
/// batch** and record the lost sequence range; it does not retry and
/// it does not block the simulation. The consumer can later refill the
/// missing window over the resume channel (M-07 `resumeBySeq` /
/// `E_PROTO-05`).
pub trait DeltaSink {
    /// Deliver one batch of waveform edges.
    fn push_delta(&mut self, delta: &crate::model::WaveformDelta) -> Result<(), PlatformError>;
}

#[cfg(test)]
mod tests {
    //! Pins the **three default implementations** of [`Peripheral`] that
    //! the session snapshot depends on.
    //!
    //! ⚠️ **These are the contract's *additivity* guarantees.** They are
    //! invisible in the kernel: M-15's three peripherals **all** override
    //! the session methods, so changing a default there would change
    //! **nothing** observable — and nothing would go red. The property
    //! has to be pinned **here**, on the contract, where it is written.

    use super::*;
    use crate::model::WriteSize;
    use crate::session::{PeripheralSessionState, UartStateDto};

    /// A peripheral that implements **only** the mandatory methods —
    /// it does **not** override any of the three session-snapshot
    /// defaults. This is what an "existing implementor" looks like
    /// (the trait's default methods are what keep it compiling).
    struct BarePeripheral;

    impl Peripheral for BarePeripheral {
        fn id(&self) -> &str {
            "BARE0"
        }
        fn base_addr(&self) -> u32 {
            0x4000_1000
        }
        fn size(&self) -> u32 {
            0x10
        }
        fn read(&self, _offset: u32, _size: WriteSize) -> Result<u32, PlatformError> {
            Ok(0)
        }
        fn write(
            &mut self,
            _offset: u32,
            _size: WriteSize,
            _value: u32,
        ) -> Result<(), PlatformError> {
            Ok(())
        }
        fn tick(&mut self, _now_ps: u64) -> Result<Vec<PendingWrite>, PlatformError> {
            Ok(Vec::new())
        }
    }

    fn uart_state() -> PeripheralSessionState {
        PeripheralSessionState::Uart(UartStateDto {
            id: "BARE0".to_string(),
            tx: Some(0x41),
            rx: None,
            status: 0,
            irq_line: 3,
            now_ps: 0,
        })
    }

    /// ⚠️ **The default is `None`** — a peripheral that contributes no
    /// session state says so, instead of contributing an empty state
    /// that would look like "this peripheral had no state" rather than
    /// "this peripheral is not part of the snapshot".
    #[test]
    fn a_peripheral_that_does_not_override_contributes_no_session_state() {
        let p = BarePeripheral;
        assert!(p.session_state().is_none());
    }

    /// ⚠️ **The default lets everything through** (`Ok(())`).
    ///
    /// This is the **additivity** guarantee: a peripheral written
    /// against the contract *before* the session snapshot existed must
    /// keep compiling **and keep working**. If the default rejected
    /// every state, every such implementor would start failing its
    /// restore — a **breaking** change smuggled in as an "additive"
    /// one.
    #[test]
    fn the_default_acceptance_check_lets_everything_through() {
        let p = BarePeripheral;
        let state = uart_state();
        assert!(
            p.validate_session_state(&state).is_ok(),
            "默认的 validate_session_state 必须放行——它是加法性的保证"
        );
    }

    /// ⚠️ **The default rejects** — a peripheral that does not implement
    /// the restore must **not silently swallow** a state meant for it:
    /// that drops state without a word, and the restored session would
    /// quietly differ from the captured one.
    #[test]
    fn the_default_restore_rejects_instead_of_silently_succeeding() {
        let mut p = BarePeripheral;
        let err = p
            .restore_session_state(&uart_state())
            .expect_err("默认实现必须报错，不能静默成功");
        assert_eq!(err.family, crate::error::ErrorFamily::Peripheral);
        assert_eq!(err.scenario, E_PER_04_UNSUPPORTED_SESSION_STATE);
    }
}
