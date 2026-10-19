//! Port signatures for the two key abstractions (LLD §3.2.4 / §3.2.5,
//! AD §4.2 / §4.3) and the observability pipeline.
//!
//! Every concrete module exposes a `*Port` trait. Alternative
//! implementations (e.g. a QEMU-backed ExecutionBackend) just implement
//! the same trait and pass the contract tests (LLD §7.1).

use crate::error::PlatformError;
use crate::model::{PendingWrite, SimState};
use crate::protocol::{CommandPayload, EventPayload};

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

    /// Run until the next breakpoint, watchpoint, trap, or quantum boundary.
    fn continue_until_event(&mut self) -> Result<(), PlatformError>;

    /// Pause as soon as possible (at the next quantum boundary).
    fn pause(&mut self) -> Result<(), PlatformError>;

    /// Capture a deterministic snapshot.
    fn snapshot(&self) -> Result<SimState, PlatformError>;

    /// Restore a deterministic snapshot.
    fn restore(&mut self, state: &SimState) -> Result<(), PlatformError>;

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
    /// Called once per quantum (1µs by default). Returns the writes
    /// the peripheral wants to commit to the bus this quantum.
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
}

/// Driven port: peripheral → interrupt controller (M-15 → M-16).
pub trait InterruptSink {
    /// Raise an IRQ line. Idempotent.
    fn raise(&mut self, irq: u16) -> Result<(), PlatformError>;
    /// Lower an IRQ line.
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
